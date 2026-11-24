"""The memory's step at our turn's end in Rust's arithmetic order (`crates/qa/src/army_net/model.rs`, `memory_step` and
`write_notes_x`), on the CPU, for the parity of the notes (gate G7, line 2a).

The default path (`MemNet.step` on the GPU) sums in MLX's order and uses the GPU's exp / tanh. Rust sums a dot product in
eight lanes (`dot`), the softmax and the pooling over the tokens in token order, the norms in element order. The memory is
recurrent and its plan update has a steep gate (kappa 20), so a last-bit difference in a step can grow over the turns.
Here every product and sum is a separate float32 operation in Rust's order; exp and tanh are MLX's CPU functions (not
libm: within an ulp). The body's output `h` is an input (the GPU's): this removes the step's own difference, not the
body's.

Only the memory of an MB / MBN net (variant M without MH's progress numbers) is supported.
"""

import mlx.core as mx

from aurelia_nn.army.model_m import PLANS, SLOTS

F32 = mx.float32


def dense(x, wt, b):
    """Rust's `Dense::apply` on rows: y[o] = b[o] + dot(wt[o], x) (`dot`: eight lanes over the chunks of eight, the lanes
    summed as (l0 + l4) + (l1 + l5) + (l2 + l6) + (l3 + l7), then the tail in order). x [N, n_in], wt [n_out, n_in]."""
    n_in = x.shape[-1]
    chunks = n_in // 8
    acc = None
    for c in range(chunks):
        p = x[:, None, c * 8:(c + 1) * 8] * wt[None, :, c * 8:(c + 1) * 8]
        acc = p if acc is None else acc + p
    if acc is None:
        s = mx.zeros((x.shape[0], wt.shape[0]), dtype=F32)
    else:
        s = (acc[..., 0] + acc[..., 4]) + (acc[..., 1] + acc[..., 5])
        s = s + (acc[..., 2] + acc[..., 6])
        s = s + (acc[..., 3] + acc[..., 7])
    for i in range(chunks * 8, n_in):
        s = s + x[:, None, i] * wt[None, :, i]
    return b[None, :] + s


def dot(a, b):
    """Rust's `dot` of two row sets [..., n] (the same lanes as `dense`)."""
    n = a.shape[-1]
    chunks = n // 8
    acc = None
    for c in range(chunks):
        p = a[..., c * 8:(c + 1) * 8] * b[..., c * 8:(c + 1) * 8]
        acc = p if acc is None else acc + p
    s = (acc[..., 0] + acc[..., 4]) + (acc[..., 1] + acc[..., 5])
    s = s + (acc[..., 2] + acc[..., 6])
    s = s + (acc[..., 3] + acc[..., 7])
    for i in range(chunks * 8, n):
        s = s + a[..., i] * b[..., i]
    return s


def sig(v):
    return 1.0 / (1.0 + mx.exp(-v))


def gru(x, h, w):
    """Rust's `Gru::step` on rows: x [N, in], h [N, H]; w = (wx^T, bx, wh^T, bh)."""
    wxt, bx, wht, bh = w
    n = h.shape[-1]
    gx = dense(x, wxt, bx)
    gh = dense(h, wht, bh)
    r = sig(gx[:, :n] + gh[:, :n])
    z = sig(gx[:, n:2 * n] + gh[:, n:2 * n])
    c = mx.tanh(gx[:, 2 * n:] + r * gh[:, 2 * n:])
    return (1.0 - z) * c + z * h


def seq_sum(v, axis):
    """A sum in index order (Rust's `iter().sum()`)."""
    n = v.shape[axis]
    s = mx.take(v, 0, axis=axis)
    for i in range(1, n):
        s = s + mx.take(v, i, axis=axis)
    return s


class RustMemory:
    """The memory weights in Rust's layout (dense weights [n_out, n_in]) on the CPU."""

    def __init__(self, m):
        with mx.stream(mx.cpu):
            t = lambda a: mx.array(mx.transpose(a), dtype=F32)
            self.gu = (t(m.gu_wx), mx.array(m.gu_bx), t(m.gu_wh), mx.array(m.gu_bh))
            self.gp = (t(m.gp_wx), mx.array(m.gp_bx), t(m.gp_wh), mx.array(m.gp_bh))
            self.pq = (mx.array(m.pq.weight), mx.array(m.pq.bias))
            self.pk = (mx.array(m.pk.weight), mx.array(m.pk.bias))
            self.hyst = mx.array(m.hyst)
            self.free = (mx.array(m.free.weight), mx.array(m.free.bias)) if hasattr(m, "free") else None
            mx.eval(self.gu, self.gp, self.pq, self.pk, self.hyst, self.free)
        self.d = m.config["d"]

    def step(self, hfull, tok, slot, tmask, um, pm, alive):
        """`MemNet.step`'s outputs (um', pm', alive') in Rust's order. Same shapes; computed on the CPU."""
        with mx.stream(mx.cpu):
            out = self._step(hfull, tok, slot, tmask, um, pm, alive)
            mx.eval(out)
        return out

    def _step(self, hfull, tok, slot, tmask, um, pm, alive):
        B, T, F = tok.shape
        d = self.d
        ht = hfull[:, :T]
        has = (slot >= 0) & (slot < SLOTS) & tmask
        um2 = self._units(ht, tok, slot, has, um)
        cover = mx.zeros((B, SLOTS), dtype=F32)
        onehot = (mx.maximum(slot, 0)[..., None] == mx.arange(SLOTS)[None, None, :]) & has[..., None]
        cover = mx.max(onehot.astype(F32), axis=1)
        event = mx.minimum(seq_sum(alive * (1.0 - cover), axis=1), 1.0)
        # The plan tokens: attention over the tokens (in token order), the plan GRU, the hysteresis.
        n_tok = mx.sum(tmask, axis=1)
        keys = dense(ht.reshape(B * T, d), *self.pk).reshape(B, T, d)
        x_obj = tok[:, 0]
        out = []
        for k in range(PLANS):
            pk = pm[:, k]
            q = dense(pk, *self.pq)
            w = dot(q[:, None, :], keys) * (1.0 / (float(d) ** 0.5))
            w = mx.where(tmask, w, -mx.inf)
            mx_ = mx.max(w, axis=1, keepdims=True)
            e = mx.where(tmask, mx.exp(w - mx_), 0.0)
            total = seq_sum(e, axis=1)
            pooled = mx.zeros((B, d), dtype=F32)
            for j in range(T):
                a = e[:, j] / total
                pooled = mx.where((j < n_tok)[:, None], pooled + a[:, None] * ht[:, j], pooled)
            x = mx.concatenate([pooled, x_obj], axis=-1)
            pn = gru(x, pk, self.gp)
            diff = seq_sum((pn - pk) * (pn - pk), axis=1)
            norm = seq_sum(pk * pk, axis=1)
            delta = mx.sqrt(diff + 1e-12) / (mx.sqrt(norm + 1e-12) + 1e-3)
            theta, kappa, we = self.hyst[k, 0], self.hyst[k, 1], self.hyst[k, 2]
            c = sig(kappa * (delta - theta) + we * event)
            out.append(pk + c[:, None] * (pn - pk))
        pm2 = mx.stack(out, axis=1)
        return um2, pm2, cover

    def _units(self, ht, tok, slot, has, um):
        """Every army token's GRU on [H_i, f_i] and its slot's u; the later token of a slot wins (Rust's order)."""
        B, T, F = tok.shape
        d = self.d
        sl = mx.maximum(slot, 0)
        us = um[mx.arange(B)[:, None], sl]
        x = mx.concatenate([ht, tok], axis=-1).reshape(B * T, d + F)
        un = gru(x, us.reshape(B * T, -1), self.gu).reshape(B, T, -1)
        um2 = um
        for j in range(T):
            hit = (sl[:, j][:, None] == mx.arange(SLOTS)[None, :]) & has[:, j][:, None]
            um2 = mx.where(hit[..., None], un[:, j][:, None, :], um2)
        return um2

    def free_note(self, p0):
        """MBN's free numbers tanh(free(P_0)) in Rust's order."""
        with mx.stream(mx.cpu):
            v = mx.tanh(dense(p0, *self.free))
            mx.eval(v)
        return v
