"""MBTN: the typed tokens and the third parties (MBT, step 1.5) on top of the notes (MBN), one net (step 1.10).

The base of the phase is N175, an MBN net. The typed path exists only for an MB net (`MBTNet`), so a typed N175 needs this
class: both cross-attentions in every layer. Rust already reads both at once (`model.rs`: the layer's self-attention, then the
history, the notes and the third parties each added to the stream from the same queries, then the feed-forward), so this is the
MLX half of what Rust plays. A layer: h + o(self) + no(notes) + tp_o(third), then the feed-forward. Starts equal to its MBN base
(`init_mbt.convert`: every new projection and output is zero), so it plays N175's games.
"""
import math

import mlx.core as mx
import mlx.nn as nn

from .model import NEG
from .model_m import PLANS
from .model_mb import MBNNet, MBTNet


class MBTNNet(MBTNet, MBNNet):
    def body_t(self, b, um, pm):
        """The body on a batch dict with record v5's keys and the note stream b['ns_<key>'] (b['ns_off'] [B]: the stream masked)."""
        ns = {k: b['ns_' + k] for k in self.NOTE_STATE}
        return self.body_mbtn(b['tok'], b['dist'], b['tmask'], b['slot'], um, pm, b['gtok'], b['tok_ok'], b['mmean'], b['xtok'], b['third'], b['third_x'],
                              b.get('gthird'), b.get('third_ok'), b['xmask'], b['tdist'], ns, b['ns_off'])

    def body_b(self, b, um, pm):
        return self.body_t(b, um, pm), None

    def body_mbtn(self, tok, dist, tmask, slot, um, pm, gtok, tok_ok, mmean, xtok, third, third_x, gthird, third_ok, xmask, tdist, ns, off):
        b, t, _ = tok.shape
        h = self.embed_main(tok, xtok)
        has = slot >= 0
        us = um[mx.arange(b)[:, None], mx.maximum(slot, 0)]
        h = h + mx.where(has[..., None], self.um(us), 0.0)
        h = h + mx.where(tok_ok[..., None], self.m_tok(gtok), 0.0)
        pooled = self.m_pool(mmean)
        h = mx.concatenate([h[:, :1] + pooled[:, None, :], h[:, 1:]], axis=1)
        x = mx.concatenate([h, pm], axis=1)
        mask = mx.concatenate([tmask, mx.ones((b, PLANS), dtype=mx.bool_)], axis=1)
        is_plan = mx.concatenate([mx.zeros((t,), dtype=mx.bool_), mx.ones((PLANS,), dtype=mx.bool_)])
        pair_plan = is_plan[:, None] | is_plan[None, :]
        dpad = mx.pad(dist, [(0, 0), (0, PLANS), (0, PLANS)])
        te = self.embed_third(third, third_x, gthird, third_ok)
        tdpad = mx.pad(tdist, [(0, 0), (0, PLANS), (0, 0)])
        qmain = mx.concatenate([mx.ones((t,), dtype=mx.bool_), mx.zeros((PLANS,), dtype=mx.bool_)])
        nt, nmask, nslot, nage = self.note_tokens(ns)
        nmask = nmask & ~off[:, None]
        ne = self.n2(nn.relu(self.n1(nt)))
        qslot = mx.concatenate([slot, mx.full((b, PLANS), -1, dtype=slot.dtype)], axis=1)
        same = ((qslot[:, :, None] == nslot[:, None, :]) & (nslot[:, None, :] >= 0)).astype(mx.float32)
        for li, layer in enumerate(self.layers):
            x = self.layer_mbtn(layer, self.no[li], self.tp_o[li], x, dpad, mask, pair_plan, self.plan_bias[li], ne, nmask, same, nage,
                                self.nx_null[li], self.nx_same[li], self.nx_slope[li], te, xmask, tdpad, qmain, self.tp_null[li], self.tp_dist[li])
        return self.norm(x)

    @staticmethod
    def layer_mbtn(layer, no, tp_o, h, dist, key_mask, pair_plan, pbias, ne, nmask, same, nage, nnull, nsame, nslope, te, xmask, tdist, qmain, tnull, tdbias):
        b, t, d = h.shape
        heads = layer.heads
        dh = d // heads
        a = layer.norm1(h)
        q = layer.q(a).reshape(b, t, heads, dh).transpose(0, 2, 1, 3)
        k = layer.k(a).reshape(b, t, heads, dh).transpose(0, 2, 1, 3)
        v = layer.v(a).reshape(b, t, heads, dh).transpose(0, 2, 1, 3)
        sc = 1.0 / math.sqrt(dh)
        s = (q @ k.transpose(0, 1, 3, 2)) * sc
        bias = layer.dist_bias[:, dist].transpose(1, 0, 2, 3)
        bias = mx.where(pair_plan[None, None], pbias[None, :, None, None], bias)
        s = s + bias + mx.where(key_mask[:, None, None, :], 0.0, NEG)
        ctx = (mx.softmax(s, axis=-1) @ v).transpose(0, 2, 1, 3).reshape(b, t, d)
        # The notes: their own softmax over [null, notes], minus the slope x age, plus the bias of the query's own unit.
        nn_ = ne.shape[1]
        an = layer.norm1(ne)
        kn = layer.k(an).reshape(b, nn_, heads, dh).transpose(0, 2, 1, 3)
        vn = layer.v(an).reshape(b, nn_, heads, dh).transpose(0, 2, 1, 3)
        sn = (q @ kn.transpose(0, 1, 3, 2)) * sc - nslope[None, :, None, None] * nage[:, None, None, :] + same[:, None] * nsame[None, :, None, None]
        sn = sn + mx.where(nmask[:, None, None, :], 0.0, NEG)
        s0 = mx.broadcast_to(nnull[None, :, None, None], (b, heads, t, 1))
        wn = mx.softmax(mx.concatenate([s0, sn], axis=-1), axis=-1)
        ctxn = (wn[..., 1:] @ vn).transpose(0, 2, 1, 3).reshape(b, t, d)
        # The third parties: their own softmax over [null, third tokens] plus the distance bias of a main token's query.
        m = te.shape[1]
        at = layer.norm1(te)
        kt = layer.k(at).reshape(b, m, heads, dh).transpose(0, 2, 1, 3)
        vt = layer.v(at).reshape(b, m, heads, dh).transpose(0, 2, 1, 3)
        tb = tdbias[:, tdist].transpose(1, 0, 2, 3)
        tb = mx.where(qmain[None, None, :, None], tb, 0.0)
        st = (q @ kt.transpose(0, 1, 3, 2)) * sc + tb + mx.where(xmask[:, None, None, :], 0.0, NEG)
        t0 = mx.broadcast_to(tnull[None, :, None, None], (b, heads, t, 1))
        wt = mx.softmax(mx.concatenate([t0, st], axis=-1), axis=-1)
        ctxt = (wt[..., 1:] @ vt).transpose(0, 2, 1, 3).reshape(b, t, d)
        h = h + layer.o(ctx) + no(ctxn) + tp_o(ctxt)
        mm = layer.norm2(h)
        return h + layer.f2(nn.relu(layer.f1(mm)))
