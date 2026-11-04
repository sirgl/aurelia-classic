"""Inspection and hand edits of a v2 checkpoint, no training
(docs/ai-net-model.md, «v2: inspection and hand edits»).

    python -m aurelia_nn.v2_inspect weights CKPT [--vs CKPT ...] [--combat-pretrained PT] --out W.json
        per layer: norms, spectral norm, effective rank, near-zero and
        outlier weights; the RMSNorm gains; the distance biases; the defer
        bias; what moved between checkpoints
    python -m aurelia_nn.v2_inspect forward CKPT --data DATA --holdout H --val-expert DIR --val-books DIR --out F.json
        a forward pass with recorders: dead and always-on units (ReLU² of
        the map blocks and the FFNs), the memory's GRU gates, attention per
        head (entropy, self, one-token, the distance bias's effect, mass by
        token kind), the candidate attention (null, near, the target), the
        combat module against the shards' previews (and the pretrained
        module), the decision heads' calibration, the defer bias, the value
        head; the decisions' logits of the calibration half (F.npz)
    python -m aurelia_nn.v2_inspect fit F.npz --out CAL.json
        per-head logit scales and the defer bias fitted on the calibration
        half (CE), scored on the evaluation half
    python -m aurelia_nn.v2_inspect edit CKPT --edit E [--edit E ...] --out NEW.pt
        a hand edit (`apply_edit`), written as a new checkpoint
    python -m aurelia_nn.v2_inspect bench CKPT ... --data ... --out-dir DIR
        v2_bench eval of each checkpoint, the data loaded once
    python -m aurelia_nn.v2_inspect table BASE.json EDIT.json ...
        the bench results as a Markdown table of changes from the first

The calibration half and the evaluation half: the held-out whole games,
books and ground games, by the parity of their index in sorted order
(`halves`); the fitted scalars never see the evaluation half.
"""

from __future__ import annotations

import argparse
import json
import math
import time
from pathlib import Path

import numpy as np
import torch
import torch.nn as nn
import torch.nn.functional as F

from .model_v2 import Block, CandidateAttention, Combat, HexBlock, N_BUCKETS, NO_POSITION, RMSNorm, relu2

# ---------------------------------------------------------------- weights


def matrix_stats(w: torch.Tensor) -> dict:
    """A weight matrix (more dimensions flattened after the first): norms,
    the singular values' spectral norm, effective rank (exp of the entropy
    of σ / Σσ) and stable rank (‖W‖²_F / σ²_max), near-zero weights (|w| <
    1 % of the RMS) and outliers (more than 6 standard deviations off)."""
    w = w.detach().float().reshape(w.shape[0], -1)
    s = torch.linalg.svdvals(w)
    p = s / s.sum().clamp_min(1e-12)
    eff = float(torch.exp(-(p * p.clamp_min(1e-12).log()).sum()))
    rms = float(w.square().mean().sqrt())
    fro = float(w.norm())
    std = float(w.std()) if w.numel() > 1 else 0.0
    return {"shape": list(w.shape), "fro": fro, "rms": rms, "spectral": float(s[0]), "eff_rank": eff, "eff_rank_share": eff / len(s),
            "stable_rank": fro**2 / max(float(s[0]) ** 2, 1e-12), "near_zero": float((w.abs() < 0.01 * rms).float().mean()),
            "outliers": int(((w - w.mean()).abs() > 6 * std).sum()) if std > 0 else 0, "max_over_rms": float(w.abs().max()) / max(rms, 1e-12)}


def weight_report(model: nn.Module) -> dict:
    out = {"matrices": {}, "norms": {}, "dist_bias": {}, "vectors": {}}
    for name, p in model.named_parameters():
        if p.dim() >= 2 and min(p.shape[0], p[0].numel()) > 1 and not name.endswith("dist_bias"):
            out["matrices"][name] = matrix_stats(p)
        elif p.dim() == 1 and not any(name == f"{m}.weight" for m, mod in model.named_modules() if isinstance(mod, RMSNorm)):
            out["vectors"][name] = {"n": p.numel(), "norm": float(p.norm()), "max_abs": float(p.abs().max())}
    for name, m in model.named_modules():
        if isinstance(m, RMSNorm):
            g = m.weight.detach().float()
            out["norms"][name] = {"n": g.numel(), "mean": float(g.mean()), "min": float(g.min()), "max": float(g.max()), "std": float(g.std()),
                                  "abs_below_0.1": float((g.abs() < 0.1).float().mean()), "negative": float((g < 0).float().mean())}
        if isinstance(m, (Block, CandidateAttention)):
            b = m.dist_bias.detach().float()
            out["dist_bias"][name] = {"per_head": b.tolist(), "range_per_head": (b[:, :NO_POSITION].max(1).values - b[:, :NO_POSITION].min(1).values).tolist()}
    if hasattr(model, "defer_bias"):
        out["defer_bias"] = float(model.defer_bias.detach())
    return out


def weight_diff(a: dict, b: dict) -> dict:
    """Per tensor of both state dicts: ‖b − a‖ / ‖a‖ and the cosine; summed
    by the module (the name's first part)."""
    per, by = {}, {}
    for k, va in a.items():
        vb = b.get(k)
        if vb is None or vb.shape != va.shape or not va.is_floating_point():
            continue
        va, vb = va.float(), vb.float()
        na = float(va.norm())
        d = float((vb - va).norm())
        cos = float((va.flatten() @ vb.flatten()) / max(na * float(vb.norm()), 1e-12))
        per[k] = {"rel_change": d / max(na, 1e-12), "cosine": cos}
        m = k.split(".")[0]
        acc = by.setdefault(m, [0.0, 0.0])
        acc[0] += d * d
        acc[1] += na * na
    return {"tensors": per, "modules": {m: math.sqrt(x[0]) / max(math.sqrt(x[1]), 1e-12) for m, x in by.items()}}


def combat_state(path: str) -> dict:
    """The combat module's state of a pretrained file or a checkpoint."""
    ck = torch.load(path, map_location="cpu", weights_only=False)
    st = ck["state"]
    if "att.weight" in st:
        return st
    return {k[len("combat."):]: v for k, v in st.items() if k.startswith("combat.")}


# ---------------------------------------------------------------- forward recorder


class Acc:
    """Running per-unit counts: active (> 0) and saturated (> hi or < lo)."""

    def __init__(self):
        self.n = 0
        self.pos = None
        self.sum = None

    def add(self, x: torch.Tensor):
        x = x.detach().float().reshape(-1, x.shape[-1])
        if self.pos is None:
            self.pos = torch.zeros(x.shape[1], dtype=torch.float64)
            self.sum = torch.zeros(x.shape[1], dtype=torch.float64)
        self.pos += (x > 0).sum(0).double()
        self.sum += relu2(x).sum(0).double()
        self.n += len(x)

    def report(self) -> dict:
        if self.pos is None:
            return {}
        rate = (self.pos / max(self.n, 1)).numpy()
        return {"n": self.n, "units": len(rate), "dead": int((rate == 0).sum()), "rare_below_0.1pct": int((rate < 1e-3).sum()),
                "always_on": int((rate == 1).sum()), "on_above_99pct": int((rate > 0.99).sum()), "mean_rate": float(rate.mean()),
                "dead_units": np.nonzero(rate == 0)[0].tolist()}


class Gate:
    """A GRU cell's update and reset gates over the rows it ran on."""

    def __init__(self):
        self.n = 0
        self.z_sum = self.z_hi = self.z_lo = self.r_hi = self.r_lo = None

    def add(self, cell: nn.GRUCell, x: torch.Tensor, h: torch.Tensor | None):
        if h is None:
            h = x.new_zeros(len(x), cell.hidden_size)
        gi = F.linear(x.float(), cell.weight_ih.float(), cell.bias_ih.float())
        gh = F.linear(h.float(), cell.weight_hh.float(), cell.bias_hh.float())
        hs = cell.hidden_size
        r = torch.sigmoid(gi[:, :hs] + gh[:, :hs])
        z = torch.sigmoid(gi[:, hs : 2 * hs] + gh[:, hs : 2 * hs])
        if self.z_sum is None:
            self.z_sum, self.z_hi, self.z_lo, self.r_hi, self.r_lo = (torch.zeros(hs, dtype=torch.float64) for _ in range(5))
        self.z_sum += z.sum(0).double()
        self.z_hi += (z > 0.99).sum(0).double()
        self.z_lo += (z < 0.01).sum(0).double()
        self.r_hi += (r > 0.99).sum(0).double()
        self.r_lo += (r < 0.01).sum(0).double()
        self.n += len(x)

    def report(self) -> dict:
        if self.z_sum is None:
            return {}
        n = max(self.n, 1)
        zh, zl = (self.z_hi / n).numpy(), (self.z_lo / n).numpy()
        return {"rows": self.n, "units": len(zh), "z_mean": float((self.z_sum / n).mean()),
                # z → 1 keeps the old state: a unit whose update gate is shut on 99 % of the rows never writes.
                "frozen_units_z_gt_0.99_on_99pct": int((zh > 0.99).sum()), "overwrite_units_z_lt_0.01_on_99pct": int((zl > 0.99).sum()),
                "z_saturated_share": float(((self.z_hi + self.z_lo) / n).mean()), "r_saturated_share": float(((self.r_hi + self.r_lo) / n).mean())}


KINDS = ("region", "entity", "global", "player", "aim", "slot")


class HeadAcc:
    """Per attention head: entropy (share of the maximum), the largest
    weight, rows with one weight > 0.9, the diagonal (self), the KL of the
    attention from the same without the distance bias, the mass by key
    kind; over valid query rows."""

    def __init__(self, heads: int):
        self.h = heads
        self.n = 0
        self.ent = torch.zeros(heads, dtype=torch.float64)
        self.mx = torch.zeros(heads, dtype=torch.float64)
        self.one = torch.zeros(heads, dtype=torch.float64)
        self.diag = torch.zeros(heads, dtype=torch.float64)
        self.kl = torch.zeros(heads, dtype=torch.float64)
        self.near = torch.zeros(heads, dtype=torch.float64)
        self.near0 = torch.zeros(heads, dtype=torch.float64)
        self.kind = torch.zeros(heads, len(KINDS), dtype=torch.float64)

    def add(self, p, p0, rows, kind, near):
        """p, p0 [B, H, N, N]; rows [B, N] valid queries; kind [N] key kinds;
        near [B, N, N] pairs within hex distance 2."""
        sel = rows[:, None, :].expand(-1, self.h, -1)  # [B, H, N]
        nvalid = (p > 0).sum(-1).clamp(min=2).float()
        lp = p.clamp_min(1e-12).log()
        ent = -(p * lp).sum(-1) / nvalid.log()
        kl = (p * (lp - p0.clamp_min(1e-12).log())).sum(-1)
        mx = p.max(-1).values
        diag = torch.diagonal(p, dim1=-2, dim2=-1)
        f = lambda t: (t * sel).sum((0, 2)).double()
        self.ent += f(ent)
        self.mx += f(mx)
        self.one += f((mx > 0.9).float())
        self.diag += f(diag)
        self.kl += f(kl)
        nf = near[:, None].float()
        self.near += f((p * nf).sum(-1))
        self.near0 += f((p0 * nf).sum(-1))
        for j in range(len(KINDS)):
            m = (kind == j).float()
            if m.any():
                self.kind[:, j] += f((p * m).sum(-1))
        self.n += int(rows.sum())

    def report(self) -> list:
        n = max(self.n, 1)
        return [{"entropy_share": float(self.ent[i] / n), "max_weight": float(self.mx[i] / n), "one_token_rows": float(self.one[i] / n),
                 "self": float(self.diag[i] / n), "kl_bias_nats": float(self.kl[i] / n), "near_mass": float(self.near[i] / n),
                 "near_mass_without_bias": float(self.near0[i] / n),
                 "by_kind": {k: float(self.kind[i, j] / n) for j, k in enumerate(KINDS)}} for i in range(self.h)]


class CandAcc:
    """The candidate attention per head: the null key's weight, entropy,
    the mass on entities at hex distance 0 and ≤ 2 of the candidate's tile
    (and what a uniform weighting would put there), the mass on the
    candidate's own target entity (attack candidates and others with a
    target)."""

    def __init__(self, heads: int):
        self.h = heads
        self.n = 0
        self.nt = 0
        z = lambda: torch.zeros(heads, dtype=torch.float64)
        self.null, self.ent, self.b0, self.b2, self.u0, self.u2, self.tgt, self.utgt = (z() for _ in range(8))

    def add(self, p_null, p_e, valid, bucket, ent_ok, tgt):
        """p_null [D, H, K, 1], p_e [D, H, K, E], valid [D, K], bucket [D, K, E],
        ent_ok [D, E], tgt [D, K] (−1: none)."""
        v = valid[:, None, :].float()
        f = lambda t: (t * v).sum((0, 2)).double()
        self.null += f(p_null[..., 0])
        p = torch.cat([p_null, p_e], -1)
        self.ent += f(-(p * p.clamp_min(1e-12).log()).sum(-1))
        ok = ent_ok[:, None, :].float()
        cnt = ok.sum(-1).clamp(min=1)  # [D, 1]
        for t, (acc, uacc) in ((0, (self.b0, self.u0)), (2, (self.b2, self.u2))):
            m = ((bucket <= t) & (bucket != NO_POSITION)).float() * ok  # [D, K, E]
            acc += f((p_e * m[:, None]).sum(-1))
            uacc += f((m.sum(-1) / cnt)[:, None].expand(-1, self.h, -1))
        has = (tgt >= 0) & valid
        if has.any():
            oh = F.one_hot(tgt.clamp(min=0), p_e.shape[-1]).float()  # [D, K, E]
            w = has[:, None, :].float()
            self.tgt += ((p_e * oh[:, None]).sum(-1) * w).sum((0, 2)).double()
            self.utgt += ((1.0 / cnt)[:, None].expand(-1, self.h, has.shape[1]) * w).sum((0, 2)).double()
            self.nt += int(has.sum())
        self.n += int(valid.sum())

    def report(self) -> list:
        n, nt = max(self.n, 1), max(self.nt, 1)
        return [{"null": float(self.null[i] / n), "entropy_nats": float(self.ent[i] / n), "at_tile": float(self.b0[i] / n),
                 "at_tile_uniform": float(self.u0[i] / n), "within_2": float(self.b2[i] / n), "within_2_uniform": float(self.u2[i] / n),
                 "on_target": float(self.tgt[i] / nt), "on_target_uniform": float(self.utgt[i] / nt), "candidates_with_target": self.nt}
                for i in range(self.h)]


class Calib:
    """HeadStats plus, per decision, its padded logits, the defer flags of
    its candidates and the choice (the calibration and the defer bias)."""

    def __init__(self, stats):
        self.stats = stats
        self.logits, self.defer, self.length, self.choice, self.head, self.wait = [], [], [], [], [], []

    def add(self, batch, pad, ce):
        from .model import padded_logits

        self.stats.add(batch, pad, ce)
        host = batch["np"]
        n = host["n_dec"]
        if not n:
            return
        pad = pad[:n].float()
        dfr = padded_logits(batch, batch["cand_defer"].float())[:n]
        ok = torch.isfinite(pad)
        self.length.append(ok.sum(1).cpu().numpy())
        self.logits.append(pad[ok].cpu().numpy().astype(np.float32))
        self.defer.append((dfr[ok] > 0).cpu().numpy())
        self.choice.append(batch["dec_choice"][:n].cpu().numpy())
        self.head.append(np.asarray(host["dec_head"][:n]))
        self.wait.append(np.asarray(host["dec_wait"][:n]))

    def flush(self):
        self.stats.flush()

    def report(self):
        return self.stats.report()

    def arrays(self) -> dict:
        cat = lambda xs, dt: np.concatenate(xs).astype(dt) if xs else np.zeros(0, dt)
        return {"logits": cat(self.logits, np.float32), "defer": cat(self.defer, bool), "length": cat(self.length, np.int32),
                "choice": cat(self.choice, np.int32), "head": cat(self.head, np.int8), "wait": cat(self.wait, bool)}


class Recorder:
    """Patches a NetV2 instance (not the class) to record activations,
    attention and the value head on every forward pass while `on`."""

    def __init__(self, model):
        self.model = model
        cfg = model.cfg
        self.on = False
        self.map = [Acc() for _ in model.map_blocks]
        self.ffn = [Acc() for _ in model.blocks]
        self.attn = [HeadAcc(cfg.n_attn_heads) for _ in model.blocks]
        self.cand = CandAcc(cfg.n_attn_heads)
        self.gru = {n: Gate() for n in ("player", "nation", "unit")}
        self.value, self.value_y = [], []
        self._off = None
        self._batch = None
        rec = self
        # A forward hook that returns a value replaces the output: these return None.
        def map_hook(i):
            def hook(m, inp, out):
                if rec.on:
                    rec.map[i].add(out)
            return hook

        def gru_hook(name):
            def hook(m, inp, out):
                if rec.on:
                    rec.gru[name].add(m, inp[0], inp[1] if len(inp) > 1 else None)
            return hook

        for i, blk in enumerate(model.map_blocks):
            blk.pw1.register_forward_hook(map_hook(i))
        for name in self.gru:
            getattr(model, f"gru_{name}").register_forward_hook(gru_hook(name))
        for i, blk in enumerate(model.blocks):
            blk.forward = lambda x, mask, bucket, blk=blk, i=i: rec.block_forward(blk, i, x, mask, bucket)
        ca = model.cand_attn
        ca.forward = lambda *a, ca=ca: rec.cand_forward(ca, *a)
        attend, score = model.attend, model.score

        def attend_w(emb, *mem):
            rec._off = emb["off"]
            rec._pos = (emb["pos_q"], emb["pos_r"], emb["pos_ok"])
            return attend(emb, *mem)

        def score_w(batch, tile_h, tokens, off):
            rec._batch = batch
            out = score(batch, tile_h, tokens, off)
            if rec.on:
                vt = batch["np"].get("value_target")
                if vt is not None:
                    ok = ~np.isnan(vt)
                    if ok.any():
                        v = model.value(batch, tokens, off).float().cpu().numpy()[: len(vt)]
                        rec.value.append(v[ok])
                        rec.value_y.append(vt[ok])
            return out

        model.attend = attend_w
        model.score = score_w

    def kinds(self, n: int, device) -> torch.Tensor:
        off = self._off
        k = torch.full((n,), 0, dtype=torch.long, device=device)
        k[off["ent"] : off["global"]] = 1
        k[off["global"]] = 2
        k[off["player"]] = 3
        if off.get("aim", -1) >= 0:
            k[off["aim"]] = 4
        k[off["slots"] :] = 5
        return k

    def block_forward(self, blk: Block, i: int, x, mask, bucket):
        b, n, d = x.shape
        h = blk.heads
        dh = d // h
        q, k, v = blk.qkv(blk.norm1(x)).split(d, dim=-1)
        q = blk.qn(q.view(b, n, h, dh)).transpose(1, 2)
        k = blk.kn(k.view(b, n, h, dh)).transpose(1, 2)
        v = v.view(b, n, h, dh).transpose(1, 2)
        s = (q @ k.transpose(-1, -2)) / math.sqrt(dh)
        keymask = ~mask[:, None, None, :]
        bias = blk.dist_bias[:, bucket].permute(1, 0, 2, 3)
        p = torch.softmax((s + bias).masked_fill(keymask, float("-inf")), -1)
        o = p @ v
        x = x + blk.out(o.transpose(1, 2).reshape(b, n, d))
        pre = blk.ff1(blk.norm2(x))
        if self.on:
            p0 = torch.softmax(s.masked_fill(keymask, float("-inf")), -1)
            near = (bucket <= 2) & (bucket != NO_POSITION)
            self.attn[i].add(p, p0, mask, self.kinds(n, x.device), near)
            self.ffn[i].add(pre[mask])
        return x + blk.ff2(relu2(pre))

    def cand_forward(self, ca: CandidateAttention, x, grid, dec_b, ent, ent_ok, bucket, pair):
        n, d = x.shape
        dd, kk = grid.shape
        h = ca.heads
        dh = d // h
        xs = torch.cat([x, x.new_zeros(1, d)]).index_select(0, grid.reshape(-1)).view(dd, kk, d)
        q = ca.qn(ca.q(ca.norm(xs)).view(dd, kk, h, dh)).transpose(1, 2)
        kv = ca.kv(ent).index_select(0, dec_b)
        if ca.pair_kv is not None and pair is not None:
            kv = kv + ca.pair_kv(pair)
        k, v = kv.split(d, dim=-1)
        e = k.shape[1]
        k = ca.kn(k.view(dd, e, h, dh)).transpose(1, 2)
        v = v.view(dd, e, h, dh).transpose(1, 2)
        scale = 1.0 / math.sqrt(dh)
        bk = bucket[:, None].expand(dd, h, kk, e)
        s = (q @ k.transpose(-1, -2)) * scale
        s = s + torch.gather((q @ ca.dist_k.transpose(-1, -2)[None]) * scale, 3, bk)
        s = s + ca.dist_bias[None, :, None, :].expand(dd, h, kk, N_BUCKETS).gather(3, bk)
        eok = ent_ok.index_select(0, dec_b)
        s = s.masked_fill(~eok[:, None, None, :], float("-inf"))
        s_null = (q * ca.null_k[None, :, None, :]).sum(-1, keepdim=True) * scale
        p = torch.softmax(torch.cat([s_null, s], dim=-1), dim=-1)
        p_null, p_e = p[..., :1], p[..., 1:]
        o = p_e @ v + p_null * ca.null_v[None, :, None, :]
        pb = torch.zeros(dd, h, kk, N_BUCKETS, device=x.device, dtype=p.dtype).scatter_add_(3, bk, p_e)
        o = o + pb @ ca.dist_v[None]
        if self.on and self._batch is not None:
            cent = self._batch["cand_ent"]
            tgt = torch.cat([cent, cent.new_full((1,), -1)]).index_select(0, grid.reshape(-1)).view(dd, kk)
            self.cand.add(p_null, p_e, grid < n, bucket, eok, tgt)
        o = o.transpose(1, 2).reshape(dd * kk, d)
        flat = torch.full((n + 1,), dd * kk, dtype=torch.long, device=x.device)
        flat = flat.index_put((grid.reshape(-1),), torch.arange(dd * kk, device=x.device))
        o = torch.cat([o, o.new_zeros(1, d)]).index_select(0, flat[:n])
        return ca.out(o)

    def report(self) -> dict:
        out = {"map_blocks": [a.report() for a in self.map], "ffn": [a.report() for a in self.ffn],
               "attention": [a.report() for a in self.attn], "candidate_attention": self.cand.report(),
               "gru": {k: g.report() for k, g in self.gru.items()}}
        if self.value:
            v, y = np.concatenate(self.value), np.concatenate(self.value_y)
            out["value"] = value_calibration(v, y)
        return out


def value_calibration(v: np.ndarray, y: np.ndarray) -> dict:
    """The value head against its target (the final points share): Pearson,
    MSE against the mean's, the least-squares line y ≈ a·v + b (a < 1:
    overconfident), and the binned means."""
    a, b = np.polyfit(v, y, 1) if len(v) > 2 else (float("nan"), float("nan"))
    qs = np.quantile(v, np.linspace(0, 1, 11))
    bins = []
    for lo, hi in zip(qs[:-1], qs[1:]):
        m = (v >= lo) & (v <= hi)
        if m.any():
            bins.append({"pred": float(v[m].mean()), "target": float(y[m].mean()), "n": int(m.sum())})
    return {"n": int(len(v)), "pearson": float(np.corrcoef(v, y)[0, 1]) if len(v) > 2 else None, "mse": float(((v - y) ** 2).mean()),
            "mse_mean": float(((y - y.mean()) ** 2).mean()), "slope": float(a), "intercept": float(b), "bins": bins}


# ---------------------------------------------------------------- calibration


def segments(arr: dict):
    """Torch views of `Calib.arrays`: flat logits and defer flags, the
    decision of each flat entry, each decision's chosen flat index."""
    length = torch.as_tensor(arr["length"], dtype=torch.long)
    seg = torch.repeat_interleave(torch.arange(len(length)), length)
    start = torch.cumsum(length, 0) - length
    return (torch.as_tensor(arr["logits"]), torch.as_tensor(arr["defer"]).float(), seg, start + torch.as_tensor(arr["choice"], dtype=torch.long),
            torch.as_tensor(arr["head"], dtype=torch.long), len(length))


def seg_ce(logits, seg, chosen, n):
    """Per decision: CE of the chosen entry and whether it is the argmax."""
    mx = torch.full((n,), float("-inf"), dtype=logits.dtype).scatter_reduce(0, seg, logits, "amax")
    lse = torch.zeros(n, dtype=logits.dtype).index_add_(0, seg, (logits - mx[seg]).exp()).log() + mx
    ce = lse - logits[chosen]
    # Top-1 as argmax picks it: the first maximal entry (ties to the lower index).
    is_max = logits >= mx[seg]
    first = torch.full((n,), len(logits), dtype=torch.long).scatter_reduce(0, seg, torch.where(is_max, torch.arange(len(logits)), len(logits)), "amin")
    return ce, first == chosen, lse


def apply_calibration(logits, defer, head, seg, scales: dict, defer_from: float, defer_to: float):
    """logits' = s_head · (logits − defer · defer_from) + defer · defer_to:
    exactly what scaling a head's logit layer and setting the defer bias
    does to the net."""
    s = torch.ones(int(head.max()) + 1 if len(head) else 1, dtype=logits.dtype)
    for h, v in scales.items():
        s[int(h)] = v
    base = logits - defer * defer_from
    return base * s[head][seg] + defer * defer_to


def ece(conf: np.ndarray, right: np.ndarray, bins: int = 15) -> float:
    edges = np.linspace(0, 1, bins + 1)
    idx = np.clip(np.digitize(conf, edges) - 1, 0, bins - 1)
    out = 0.0
    for b in range(bins):
        m = idx == b
        if m.any():
            out += m.mean() * abs(conf[m].mean() - right[m].mean())
    return float(out)


def calibration_report(arr: dict, heads: tuple, scales: dict | None = None, defer_from: float = 0.0, defer_to: float | None = None) -> dict:
    """Per head: decisions, top-1, mean confidence, ECE, CE; with `scales`
    and the defer bias, after them."""
    logits, defer, seg, chosen, head, n = segments(arr)
    if scales is not None:
        logits = apply_calibration(logits, defer, head, seg, scales, defer_from, defer_from if defer_to is None else defer_to)
    ce, right, lse = seg_ce(logits, seg, chosen, n)
    mx = torch.full((n,), float("-inf")).scatter_reduce(0, seg, logits, "amax")
    conf = (mx - lse).exp().numpy()
    right, ce = right.numpy(), ce.numpy()
    hd = head.numpy()
    out = {}
    for i, name in enumerate(heads):
        m = hd == i
        if m.any():
            out[name] = {"n": int(m.sum()), "top1": float(right[m].mean()), "confidence": float(conf[m].mean()), "ece": ece(conf[m], right[m]),
                         "ce": float(ce[m].mean())}
    out["all"] = {"n": int(n), "top1": float(right.mean()), "confidence": float(conf.mean()), "ece": ece(conf, right), "ce": float(ce.mean())}
    return out


def fit_calibration(arr: dict, n_heads: int, defer_bias: float, steps: int = 200) -> dict:
    """Per-head scales s_h and the defer bias minimising the mean CE on
    `arr` (LBFGS on log s and the bias)."""
    logits, defer, seg, chosen, head, n = segments(arr)
    logits, defer = logits.double(), defer.double()
    log_s = torch.zeros(n_heads, dtype=torch.float64, requires_grad=True)
    b = torch.tensor(float(defer_bias), dtype=torch.float64, requires_grad=True)
    opt = torch.optim.LBFGS([log_s, b], lr=0.5, max_iter=steps, line_search_fn="strong_wolfe")
    base = logits - defer * defer_bias
    hseg = head[seg]

    def closure():
        opt.zero_grad()
        z = base * log_s.exp()[hseg] + defer * b
        ce, _, _ = seg_ce(z, seg, chosen, n)
        loss = ce.mean()
        loss.backward()
        return loss

    opt.step(closure)
    return {"scales": {str(i): float(v) for i, v in enumerate(log_s.detach().exp())}, "defer_bias": float(b.detach()), "defer_from": float(defer_bias)}


def fit_defer_only(arr: dict, defer_bias: float, grid=None) -> dict:
    """The defer bias alone, by the mean CE over a grid (top-1 also listed)."""
    logits, defer, seg, chosen, head, n = segments(arr)
    base = logits - defer * defer_bias
    rows = []
    for b in (np.arange(-8.0, 2.01, 0.25) if grid is None else grid):
        ce, right, _ = seg_ce(base + defer * float(b), seg, chosen, n)
        rows.append({"defer_bias": float(b), "ce": float(ce.mean()), "top1": float(right.float().mean())})
    best = min(rows, key=lambda r: r["ce"])
    return {"grid": rows, "best_ce": best}


# ---------------------------------------------------------------- combat


def attack_pairs_tagged(shards: list) -> dict:
    """v2_bench.attack_pairs plus each pair's kind: ranged, the defender a
    city, the blow kills."""
    from .common import HEAD_UNIT
    from .model_v2 import combat_targets

    layout = shards[0].layout
    names = list(layout.names["candidate"])
    col = {n: i for i, n in enumerate(names)}
    en = list(layout.names["entity"])
    hp, own_city, foreign_city, cs = en.index("hp"), en.index("kind_own_city"), en.index("kind_foreign_city"), en.index("kind_city_state")
    out = {k: [] for k in ("ae", "at", "de", "dt", "y", "ranged", "city")}
    for s in shards:
        for t in range(len(s)):
            x = s.turn(t)
            et = x.entity_meta[:, 1]
            for d in x.decisions:
                if d.head != HEAD_UNIT or d.actor < 0 or et[d.actor] < 0:
                    continue
                r = d.rows
                ent = d.meta[:, 2]
                att = ((r[:, col["act_melee"]] + r[:, col["act_ranged"]]) > 0) & (r[:, col["preview_defender_hp"]] > 0) & (ent >= 0)
                if not att.any():
                    continue
                idx = np.nonzero(att)[0]
                de = ent[idx]
                ok = et[de] >= 0
                idx, de = idx[ok], de[ok]
                if not len(idx):
                    continue
                y = combat_targets(torch.as_tensor(r[idx].astype(np.float32)), names, torch.full((len(idx),), float(x.entities[d.actor, hp]))).numpy()
                out["ae"].append(np.repeat(x.entities[d.actor][None], len(idx), 0))
                out["at"].append(np.repeat(x.tiles[et[d.actor]][None], len(idx), 0))
                out["de"].append(x.entities[de])
                out["dt"].append(x.tiles[et[de]])
                out["y"].append(y)
                out["ranged"].append(r[idx, col["act_ranged"]] > 0)
                out["city"].append((x.entities[de][:, [own_city, foreign_city, cs]] > 0).any(1))
    return {k: (np.concatenate(v) if v else np.zeros((0,), np.float32)) for k, v in out.items()}


def combat_predict(module: Combat, pairs: dict) -> np.ndarray:
    f = lambda a: torch.as_tensor(np.asarray(a).astype(np.float32))
    with torch.no_grad():
        out, _ = module(f(pairs["ae"]), f(pairs["at"]), f(pairs["de"]), f(pairs["dt"]))
    return out.numpy()


def combat_report(modules: dict, pairs: dict) -> dict:
    """combat_metrics of each module, overall and by the pair's kind."""
    from .v2_bench import combat_metrics

    y = pairs["y"]
    groups = {"all": np.ones(len(y), bool), "melee": ~pairs["ranged"], "ranged": pairs["ranged"], "vs_city": pairs["city"], "vs_unit": ~pairs["city"],
              "kills": y[:, 2] > 0.5, "no_kill": y[:, 2] <= 0.5}
    out = {}
    for name, m in modules.items():
        pred = combat_predict(m, pairs)
        out[name] = {g: combat_metrics(pred[sel], y[sel]) for g, sel in groups.items() if sel.sum() > 1}
    return out


# ---------------------------------------------------------------- edits


def apply_edit(model, spec: str, ctx: dict | None = None) -> str:
    """One hand edit of a NetV2 (in place); returns what it did.

    combat=PATH          the combat module's weights from PATH (pretrained file or checkpoint)
    combat-mix=PATH:A    A · PATH's combat weights + (1 − A) · the model's
    mix=PATH:A           every weight A × PATH's (a checkpoint of the same net) + (1 − A) × the model's
    scale=HEAD:S         the decision head's logit layer × S (a temperature 1 / S)
    calib=CAL.json       `fit`'s per-head scales and defer bias
    defer=B              the defer bias set to B
    prune=F.json         the units `forward` found dead zeroed (map blocks and FFNs)
    damp=BLOCK.HEAD:F    attention head HEAD of block BLOCK's output × F (0 removes it)
    cand-damp=HEAD:F     the candidate attention's head HEAD's output × F
    dist=WHERE:F         the hex-distance bias (WHERE a block index, all, cand) × F about its mean
    gain=NAME:V          the RMSNorm NAME's gain clamped below at V (|g| < V → ±V)
    """
    kind, _, arg = spec.partition("=")
    with torch.no_grad():
        if kind == "combat":
            model.combat.load_state_dict(combat_state(arg))
            return f"combat module from {arg}"
        if kind == "combat-mix":
            path, _, a = arg.rpartition(":")
            a = float(a)
            src = combat_state(path)
            for k, p in model.combat.state_dict().items():
                p.copy_(a * src[k].to(p.dtype) + (1 - a) * p)
            return f"combat module {a} × {path} + {1 - a} × own"
        if kind == "mix":
            path, _, a = arg.rpartition(":")
            a = float(a)
            src = torch.load(path, map_location="cpu", weights_only=False)["state"]
            for k, p in model.state_dict().items():
                if k in src and src[k].shape == p.shape and p.is_floating_point():
                    p.copy_(a * src[k].to(p.dtype) + (1 - a) * p)
            return f"all weights {a} × {path} + {1 - a} × own"
        if kind == "scale":
            h, s = arg.split(":")
            h = model.heads.index(h) if not h.isdigit() else int(h)
            model.head_logit[h].weight.mul_(float(s))
            model.head_logit[h].bias.mul_(float(s))
            return f"head {model.heads[h]} logits × {s}"
        if kind == "calib":
            cal = json.loads(Path(arg).read_text())
            fit = cal.get("fit", cal)
            for h, s in fit["scales"].items():
                model.head_logit[int(h)].weight.mul_(s)
                model.head_logit[int(h)].bias.mul_(s)
            if hasattr(model, "defer_bias"):
                model.defer_bias.fill_(fit["defer_bias"])
            return f"per-head scales {fit['scales']}, defer bias {fit['defer_bias']:.3f}"
        if kind == "defer":
            model.defer_bias.fill_(float(arg))
            return f"defer bias {arg}"
        if kind == "prune":
            rep = json.loads(Path(arg).read_text())["recorder"]
            n = 0
            for blocks, key, a, b in ((model.map_blocks, "map_blocks", "pw1", "pw2"), (model.blocks, "ffn", "ff1", "ff2")):
                for blk, r in zip(blocks, rep[key]):
                    dead = torch.as_tensor(r.get("dead_units", []), dtype=torch.long)
                    if len(dead):
                        getattr(blk, a).weight[dead] = 0
                        getattr(blk, a).bias[dead] = -1.0
                        getattr(blk, b).weight[:, dead] = 0
                        n += len(dead)
            return f"{n} dead units zeroed"
        if kind == "damp":
            where, _, f = arg.partition(":")
            bi, hi = (int(x) for x in where.split("."))
            blk = model.blocks[bi]
            dh = blk.out.in_features // blk.heads
            blk.out.weight[:, hi * dh : (hi + 1) * dh] *= float(f)
            return f"block {bi} head {hi} output × {f}"
        if kind == "cand-damp":
            hi, _, f = arg.partition(":")
            ca = model.cand_attn
            dh = ca.out.in_features // ca.heads
            ca.out.weight[:, int(hi) * dh : (int(hi) + 1) * dh] *= float(f)
            return f"candidate attention head {hi} output × {f}"
        if kind == "dist":
            where, _, f = arg.partition(":")
            mods = [model.cand_attn] if where == "cand" else list(model.blocks) if where == "all" else [model.blocks[int(where)]]
            for m in mods:
                b = m.dist_bias
                # Scaled about the per-head mean over the positioned buckets: sharper (F > 1) or flatter.
                mean = b[:, :NO_POSITION].mean(1, keepdim=True)
                b[:, :NO_POSITION] = mean + float(f) * (b[:, :NO_POSITION] - mean)
            return f"distance bias of {where} × {f} about its mean"
        if kind == "gain":
            name, _, v = arg.partition(":")
            g = dict(model.named_modules())[name].weight
            v = float(v)
            small = g.abs() < v
            g[small] = torch.where(g[small] < 0, -v, v)
            return f"{name}: {int(small.sum())} gains below {v} set to ±{v}"
    raise ValueError(f"unknown edit {spec}")


def save_edited(src: str, model, edits: list[str], out: str):
    """The source checkpoint with the edited tensors and the edits listed
    (`surgery`); the source untouched."""
    ck = torch.load(src, map_location="cpu", weights_only=False)
    for k, v in model.state_dict().items():
        if k in ck["state"]:
            ck["state"][k] = v.detach().clone()
    ck["surgery"] = {"from": src, "edits": edits}
    Path(out).parent.mkdir(parents=True, exist_ok=True)
    torch.save(ck, out)


# ---------------------------------------------------------------- driver


def halves(shards: list) -> tuple[list, list]:
    """(calibration, evaluation): the odd and the even sorted indices."""
    s = sorted(shards, key=lambda x: str(x.path))
    return s[1::2], s[0::2]


def run_calib(model, shards, cfg, device, rec: Recorder | None = None, wpb: int = 4):
    """The decisions' logits (Calib) over the shards, the recorder on."""
    from .train_imitation import HeadStats, Prepared, prefetch, run_windows, window_turns
    from .data import WindowIndex

    cal = Calib(HeadStats(model.heads))
    if rec is not None:
        rec.on = True
    model.eval()
    with torch.no_grad():
        for prepared in prefetch(WindowIndex(shards, 32).batches(wpb), lambda b: Prepared(window_turns(b), cfg, 8)):
            run_windows(model, prepared, cfg, device, cal)
    if rec is not None:
        rec.on = False
    return cal


def forward_cmd(a):
    from .model import load_checkpoint
    from .v2_bench import eval_data, prepare_model

    torch.set_num_threads(a.threads)
    device = torch.device("cpu")
    data = eval_data(a.data, a.holdout, a.val_expert, a.val_books)
    model = prepare_model(load_checkpoint(a.checkpoint), device)
    cfg = model.cfg
    rec = Recorder(model)
    res = {"checkpoint": a.checkpoint, "sets": {}}
    arrays = {}
    t0 = time.time()
    for name in ("expert", "books", "ground"):
        cal_sh, ev_sh = halves(data[name])
        for part, sh in (("calib", cal_sh), ("eval", ev_sh)):
            sh = sh[: a.max_shards] if a.max_shards else sh
            # The recorder over a part of the evaluation half only (cost).
            r = rec if (part == "eval" and name in a.record) else None
            cal = run_calib(model, sh, cfg, device, r, a.windows_per_batch)
            arr = cal.arrays()
            for k, v in arr.items():
                arrays[f"{name}_{part}_{k}"] = v
            res["sets"][f"{name}_{part}"] = {"shards": len(sh), "decisions": int(len(arr["choice"])), **{k: v for k, v in cal.report().items() if k in ("ce", "all")}}
            print(f"{name} {part}: {len(sh)} shards, {len(arr['choice'])} decisions, ce {res['sets'][f'{name}_{part}']['ce']:.4f} ({time.time() - t0:.0f} s)", flush=True)
    cal = run_calib(model, data["held"], cfg, device, rec if "held" in a.record else None, a.windows_per_batch)
    for k, v in cal.arrays().items():
        arrays[f"held_eval_{k}"] = v
    print(f"held-out scenes ({time.time() - t0:.0f} s)", flush=True)
    res["recorder"] = rec.report()
    res["heads"] = list(model.heads)
    res["defer_bias"] = float(model.defer_bias) if hasattr(model, "defer_bias") else None
    res["calibration"] = {s: calibration_report(_arrays(arrays, s), model.heads) for s in sorted({k.rsplit("_", 1)[0] for k in arrays})}
    # The combat module against the previews, and the pretrained one on the same pairs.
    if model.combat is not None:
        modules = {"model": model.combat}
        if a.combat_pretrained:
            pre = Combat(data["held"][0].layout)
            pre.load_state_dict(combat_state(a.combat_pretrained))
            modules["pretrained"] = pre.eval()
        res["combat"] = {}
        for name, sh in (("held_scenes", data["held"]), ("ground_val", data["ground"]), ("expert_eval_half", halves(data["expert"])[1][: a.combat_shards])):
            pairs = attack_pairs_tagged(sh)
            if len(pairs["y"]):
                res["combat"][name] = combat_report(modules, pairs)
        print(f"combat ({time.time() - t0:.0f} s)", flush=True)
    np.savez_compressed(Path(a.out).with_suffix(".npz"), **arrays)
    res["seconds"] = time.time() - t0
    Path(a.out).write_text(json.dumps(res, indent=1))
    print(f"wrote {a.out}", flush=True)


def _arrays(arrays: dict, prefix: str) -> dict:
    return {k[len(prefix) + 1 :]: v for k, v in arrays.items() if k.startswith(prefix + "_") and k[len(prefix) + 1 :] in
            ("logits", "defer", "length", "choice", "head", "wait")}


def fit_cmd(a):
    z = dict(np.load(a.npz))
    meta = json.loads(Path(a.npz).with_suffix(".json").read_text())
    heads, b0 = tuple(meta["heads"]), meta["defer_bias"]
    sources = a.sources.split(",")
    join = lambda part: {k: np.concatenate([_arrays(z, f"{s}_{part}")[k] for s in sources]) for k in ("logits", "defer", "length", "choice", "head", "wait")}
    calib, ev = join("calib"), join("eval")
    fit = fit_calibration(calib, len(heads), b0)
    res = {"fit_on": [f"{s}_calib" for s in sources], "fit": fit, "defer_only": fit_defer_only(calib, b0)["best_ce"]}
    for name, arr in [("eval_half", ev)] + [(f"{s}_eval", _arrays(z, f"{s}_eval")) for s in sources] + [("held_eval", _arrays(z, "held_eval"))]:
        res[name] = {"before": calibration_report(arr, heads), "after": calibration_report(arr, heads, fit["scales"], b0, fit["defer_bias"]),
                     "defer_only": calibration_report(arr, heads, {}, b0, res["defer_only"]["defer_bias"])}
    Path(a.out).write_text(json.dumps(res, indent=1))
    e = res["eval_half"]
    print(json.dumps({"fit": fit, "eval_half_ce": [e["before"]["all"]["ce"], e["after"]["all"]["ce"]], "eval_half_top1": [e["before"]["all"]["top1"], e["after"]["all"]["top1"]]}, indent=1))


def weights_cmd(a):
    from .model import load_checkpoint

    model = load_checkpoint(a.checkpoint)
    res = {"checkpoint": a.checkpoint, "report": weight_report(model)}
    base = torch.load(a.checkpoint, map_location="cpu", weights_only=False)["state"]
    res["vs"] = {}
    for other in a.vs:
        st = torch.load(other, map_location="cpu", weights_only=False)["state"]
        res["vs"][other] = weight_diff(st, base)
    if a.combat_pretrained:
        pre = combat_state(a.combat_pretrained)
        mine = combat_state(a.checkpoint)
        res["combat_vs_pretrained"] = weight_diff(pre, mine)["tensors"]
    Path(a.out).write_text(json.dumps(res, indent=1))
    print(f"wrote {a.out}")


def edit_cmd(a):
    from .model import load_checkpoint

    model = load_checkpoint(a.checkpoint)
    done = [apply_edit(model, e) for e in a.edit]
    save_edited(a.checkpoint, model, a.edit, a.out)
    print("\n".join(done) + f"\nwrote {a.out}")


def bench_cmd(a):
    from .model import load_checkpoint
    from .v2_bench import eval_data, evaluate_model

    torch.set_num_threads(a.threads)
    data = eval_data(a.data, a.holdout, a.val_expert, a.val_books)
    out = Path(a.out_dir)
    out.mkdir(parents=True, exist_ok=True)
    for ck in a.checkpoints:
        dst = out / f"eval-{Path(ck).stem if Path(ck).stem != 'best' else Path(ck).parent.name}.json"
        if dst.exists() and not a.force:
            print(f"skip {dst}", flush=True)
            continue
        t0 = time.time()
        res = {"checkpoint": ck} | evaluate_model(load_checkpoint(ck), data, torch.device("cpu"), "off", a.windows_per_batch)
        dst.write_text(json.dumps(res, indent=1))
        print(f"wrote {dst} ({time.time() - t0:.0f} s)", flush=True)


BENCH_COLUMNS = (
    ("expert CE", lambda r: r["expert"]["ce"]), ("expert top-1", lambda r: r["expert"]["all"]),
    ("books CE", lambda r: r["books"]["ce"]), ("books top-1", lambda r: r["books"]["all"]),
    ("ground CE", lambda r: r["ground"]["ce"]), ("ground top-1", lambda r: r["ground"]["all"]),
    ("search move", lambda r: r["holdout_scenes"]["search"]["move"]["top1"]), ("search attack", lambda r: r["holdout_scenes"]["search"]["attack"]["top1"]),
    ("scripted move", lambda r: r["holdout_scenes"]["scripted"]["move"]["top1"]), ("scripted attack", lambda r: r["holdout_scenes"]["scripted"]["attack"]["top1"]),
    ("focus r", lambda r: r["sum_heads"]["focus"]["pearson"]), ("danger r", lambda r: r["sum_heads"]["danger"]["pearson"]),
    ("danger MAE", lambda r: r["sum_heads"]["danger"]["mae_pct"]), ("combat dealt MAE", lambda r: r["combat_holdout"]["dealt_mae_pct"]),
    ("probe blow captures R", lambda r: r["probe_candidate"]["move_blow_captures"]["recall"]),
)


def table_cmd(a):
    """A Markdown table of bench results, each row's change from the first."""
    rows = [(Path(f).stem.removeprefix("eval-"), json.loads(Path(f).read_text())) for f in a.files]
    base = {n: fn(rows[0][1]) for n, fn in BENCH_COLUMNS}
    print("| edit | " + " | ".join(n for n, _ in BENCH_COLUMNS) + " |")
    print("|---" * (len(BENCH_COLUMNS) + 1) + "|")
    for i, (name, r) in enumerate(rows):
        cells = []
        for n, fn in BENCH_COLUMNS:
            v = fn(r)
            cells.append(f"{v:.4f}" if i == 0 else f"{v - base[n]:+.4f}")
        print(f"| {name} | " + " | ".join(cells) + " |")


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)

    def data_args(p):
        p.add_argument("--data", required=True)
        p.add_argument("--holdout")
        p.add_argument("--val-expert")
        p.add_argument("--val-books")
        p.add_argument("--windows-per-batch", type=int, default=4)
        p.add_argument("--threads", type=int, default=4)

    p = sub.add_parser("weights")
    p.add_argument("checkpoint")
    p.add_argument("--vs", nargs="*", default=[])
    p.add_argument("--combat-pretrained")
    p.add_argument("--out", required=True)
    p = sub.add_parser("forward")
    p.add_argument("checkpoint")
    data_args(p)
    p.add_argument("--max-shards", type=int, default=0, help="per source and half (0: all)")
    p.add_argument("--record", default="expert,books,ground,held", help="the sets the recorder runs on (their evaluation half)")
    p.add_argument("--combat-pretrained")
    p.add_argument("--combat-shards", type=int, default=10)
    p.add_argument("--out", required=True)
    p = sub.add_parser("fit")
    p.add_argument("npz")
    p.add_argument("--sources", default="expert,books,ground")
    p.add_argument("--out", required=True)
    p = sub.add_parser("edit")
    p.add_argument("checkpoint")
    p.add_argument("--edit", action="append", required=True)
    p.add_argument("--out", required=True)
    p = sub.add_parser("bench")
    p.add_argument("checkpoints", nargs="+")
    data_args(p)
    p.add_argument("--out-dir", required=True)
    p.add_argument("--force", action="store_true")
    p = sub.add_parser("table")
    p.add_argument("files", nargs="+", help="bench results, the first the reference")
    a = ap.parse_args(argv)
    if a.cmd == "forward":
        a.record = set(a.record.split(","))
    {"weights": weights_cmd, "forward": forward_cmd, "fit": fit_cmd, "edit": edit_cmd, "bench": bench_cmd, "table": table_cmd}[a.cmd](a)


if __name__ == "__main__":
    main()
