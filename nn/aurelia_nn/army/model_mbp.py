"""MBP: the MBN net (the notes) with the price input of step 1.8 (GATES.md, "step 1.8: the price in the RL reward").

The stake [worth in units / 8, time pressure, given] (MBA's price input) enters through two projections, both zero at the start,
so the net plays exactly as the MBN net it is made from:
- `w_price` [3, d]: added to the objective token (the place and the name MBA's has);
- `w_price_all` [3, d]: added to EVERY army token (the plan's "цена на каждом токене армии"; the plan's tokens are not army tokens).
`crates/qa/src/army_net/model.rs` is the same arithmetic in Rust.

python -m aurelia_nn.army.model_mbp --mbn runs/N175/weights.safetensors --out runs/NP175   (the zero start; also the Rust export)
"""
import argparse
import os

import mlx.core as mx
import mlx.nn as nn

from .model_m import PLANS
from .model_mb import MBHNet, MBNNet

PRICE_KEYS = ('w_price', 'w_price_all')


class MBPNet(MBNNet):

    def __init__(self, d=64, heads=4, layers=3, ff=128, promos=400, map_ch=32):
        super().__init__(d=d, heads=heads, layers=layers, ff=ff, promos=promos, map_ch=map_ch)
        self.config['price_all'] = True
        self.w_price = mx.zeros((3, d))
        self.w_price_all = mx.zeros((3, d))

    def body_b(self, b, um, pm):
        """The body on a batch dict (b['price'] [N, 3]; b['ns_*'] the note stream): (H, None) as MBN's."""
        ns = {k: b['ns_' + k] for k in self.NOTE_STATE}
        return self.body_mbp(b['tok'], b['dist'], b['tmask'], b['slot'], um, pm, b['gtok'], b['tok_ok'], b['mmean'], ns, b['ns_off'], b['price']), None

    def body_mbp(self, tok, dist, tmask, slot, um, pm, gtok, tok_ok, mmean, ns, off, price):
        """`MBNNet.body_mbn` with the stake: `price @ w_price_all` on every token, `price @ w_price` on the objective token."""
        b, t, _ = tok.shape
        h = self.e2(nn.relu(self.e1(tok)))
        has = slot >= 0
        us = um[mx.arange(b)[:, None], mx.maximum(slot, 0)]
        h = h + mx.where(has[..., None], self.um(us), 0.0)
        h = h + mx.where(tok_ok[..., None], self.m_tok(gtok), 0.0)
        h = h + (price @ self.w_price_all)[:, None, :]
        top = self.m_pool(mmean) + price @ self.w_price
        h = mx.concatenate([h[:, :1] + top[:, None, :], h[:, 1:]], axis=1)
        x = mx.concatenate([h, pm], axis=1)
        mask = mx.concatenate([tmask, mx.ones((b, PLANS), dtype=mx.bool_)], axis=1)
        is_plan = mx.concatenate([mx.zeros((t,), dtype=mx.bool_), mx.ones((PLANS,), dtype=mx.bool_)])
        pair_plan = is_plan[:, None] | is_plan[None, :]
        dpad = mx.pad(dist, [(0, 0), (0, PLANS), (0, PLANS)])
        nt, nmask, nslot, nage = self.note_tokens(ns)
        nmask = nmask & ~off[:, None]
        ne = self.n2(nn.relu(self.n1(nt)))
        qslot = mx.concatenate([slot, mx.full((b, PLANS), -1, dtype=slot.dtype)], axis=1)
        same = ((qslot[:, :, None] == nslot[:, None, :]) & (nslot[:, None, :] >= 0)).astype(mx.float32)
        for li, layer in enumerate(self.layers):
            x = MBHNet.layer_mbh(layer, self.no[li], x, dpad, mask, pair_plan, self.plan_bias[li], ne, nmask, same, nage,
                                 self.nx_null[li], self.nx_same[li], self.nx_slope[li])
        return self.norm(x)


def from_mbn(path):
    """An MBPNet with an MBN net's weights (the stake's projections zero)."""
    w = mx.load(path)
    model = MBPNet(promos=w['promo'].shape[0] - 1, map_ch=w['m_in.weight'].shape[0])
    own = dict(nn.utils.tree_flatten(model.parameters()))
    bad = [k for k in w if k not in own or own[k].shape != w[k].shape]
    if bad:
        raise SystemExit(f'not an MBN net of this shape: {bad[:5]}')
    model.load_weights(list(w.items()), strict=False)
    mx.eval(model.parameters())
    return model


def main():
    from .export import export
    ap = argparse.ArgumentParser()
    ap.add_argument('--mbn', required=True)
    ap.add_argument('--out', required=True)
    ap.add_argument('--sigma', type=float, default=0.0, help='draw the two projections N(0, sigma^2) (a parity test net, never trained)')
    ap.add_argument('--seed', type=int, default=1)
    a = ap.parse_args()
    mx.set_default_device(mx.cpu)
    model = from_mbn(a.mbn)
    if a.sigma > 0:
        mx.random.seed(a.seed)
        for k in PRICE_KEYS:
            setattr(model, k, mx.random.normal(getattr(model, k).shape) * a.sigma)
    os.makedirs(a.out, exist_ok=True)
    model.save_weights(os.path.join(a.out, 'weights.safetensors'))
    export(model, os.path.join(a.out, 'army-net.bin'))
    print('ok', a.out)


if __name__ == '__main__':
    main()
