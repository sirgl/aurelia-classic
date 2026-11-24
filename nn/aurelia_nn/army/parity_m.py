"""Logits parity of variant M (MLX, the memory unrolled over each play as in training) and its Rust forward
(`ai-exam army-parity`, which carries the memory decision by decision in the file's order) on recorded plays.

python -m aurelia_nn.army.parity_m --plays FILE --weights OUT/weights.safetensors --rust rust.jsonl [--plays-limit N]
Every decision of the first N whole plays (memory states included) is compared: the pointer's logits and the
labelled unit's action logits; max |delta| <= 1e-4.
"""
import argparse
import json

import mlx.core as mx

from .data import Flat, read_into
from .model_m import MemNet
from .model_mh import MHNet
from .train_m import DeviceM, C_GROUPS, body_any


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--plays', required=True)
    ap.add_argument('--weights', required=True)
    ap.add_argument('--rust', required=True)
    ap.add_argument('--plays-limit', type=int, default=60)
    ap.add_argument('--cpu', action='store_true', help='MLX on the CPU (no GPU lock needed)')
    ap.add_argument('--forced-memory', action='store_true', help="each turn read with Rust's memory after its step (the rust file's 'mem': ARMY_PARITY_MEMORY=1)")
    ap.add_argument('--worst', default=None, help='a JSONL of every decision: its max |delta| pointer / action, third tokens, family')
    a = ap.parse_args()
    if a.cpu:
        mx.set_default_device(mx.cpu)
    rust = [json.loads(l) for l in open(a.rust)]
    # The first plays only, streamed (the file is not read whole).
    from .train_m import Stream
    stream = Stream([a.plays])
    flat = stream.flat(list(range(min(a.plays_limit, len(stream)))))
    w = mx.load(a.weights)
    is_mn = 'n1.weight' in w and 'nx_slope' not in w
    if is_mn:
        from . import train_mn as TN
        from .model_mn import MNNet
        import aurelia_nn.army.train_m as TM
        TM.DeviceM.__init__ = TN._host_slots
        model = MNNet(promos=w['promo'].shape[0] - 1)
    elif 'm_in.weight' in w:
        # MB / MBA: the map encoded once per turn from the turn's first moment.
        from .model_mb import MBANet, MBHNet, MBNet, MBNNet
        from .model_mb import MBENet, MBTNet
        cls = MBTNet if 'ty_emb' in w else (MBNNet if 'nx_slope' in w else (MBHNet if 'hx_slope' in w else (MBANet if 'w_price' in w else (MBENet if 'ww' in w else MBNet))))
        model = cls(promos=w['promo'].shape[0] - 1, map_ch=w['m_in.weight'].shape[0])
    else:
        model = (MHNet if 'h1.weight' in w else MemNet)(promos=w['promo'].shape[0] - 1)
    model.load_weights(a.weights)
    tm = max(flat.t)
    dev = (TN.DeviceMN if is_mn else DeviceM)(flat, [1.0] * len(flat), tm, 128)
    P = dev.plays
    plays = [p for p in range(min(a.plays_limit, len(flat.plays))) if P.first[p] + P.count[p] <= len(rust)]
    worst_p = worst_a = 0.0
    checked = 0
    turns = 0
    from .model_mb import MBANet, MBNet, MBNNet, MBTNet
    from .train_m import map_feats, unroll_one_n
    has_map = isinstance(model, MBNet)
    is_mbn = isinstance(model, MBNNet)
    is_mbt = isinstance(model, MBTNet)
    third_decisions = 0
    worst_out = open(a.worst, 'w') if a.worst else None

    def with_map(b, maps, k):
        if has_map:
            map_feats(b, maps, mx.array([k], mx.int32), mx.array([0], mx.int32))

    for p in plays:
        ms = P.ms[p]
        maps = None
        if has_map:
            fd = P.fd[p]
            enc_args = (lambda b: (b['statics'], b['tok'], b['pos'], b['srel'])) if is_mbt else (lambda b: (b['statics'], b['tok'], b['pos']))
            maps = mx.stack([model.encode_turn(*enc_args(dev.batch_m(mx.array([j], mx.int32), tm, 1))) for j in fd])
        um, pm, alive = model.initial(1)
        Us, Ps = [um], [pm]
        ns = model.initial_notes(1) if (is_mn or is_mbn) else None
        NS = [ns]
        def hplan(b, k):
            return mx.stack([Ps[max(k - a, 0)][0, 0] for a in b['hage'][0].tolist()])[None]

        def forced(um, pm, alive, i):
            if not a.forced_memory or 'mem' not in rust[i]:
                return um, pm, alive
            v = rust[i]['mem']
            nu, npl = um.size, pm.size
            return (mx.array(v[:nu], mx.float32).reshape(um.shape), mx.array(v[nu:nu + npl], mx.float32).reshape(pm.shape),
                    mx.array(v[nu + npl:], mx.float32).reshape(alive.shape))

        for k, i in enumerate(ms):
            b = dev.batch_m(mx.array([i], mx.int32), tm, 1)
            b['hplan'] = hplan(b, k)
            with_map(b, maps, k)
            if is_mbn:
                # The net's own notes, as in play (no teacher forcing).
                b['n_teacher'] = mx.zeros((1,), mx.int32)
                for key in model.NOTE_STATE:
                    b['ns_' + key] = ns[key]
                b['ns_off'] = mx.array([False])
                um, pm, alive, nst, _, _ = unroll_one_n(model, b, mx.array([True]), mx.array([False]), um, pm, alive, False)
                ns = dict(zip(model.NOTE_STATE, nst))
                Us.append(um)
                Ps.append(pm)
                NS.append(ns)
                continue
            if is_mn:
                b['n_teacher'] = mx.zeros((1,), mx.int32)  # the net's own notes, as in play
                um, pm, alive, ns, _, _ = TN.unroll_one_mn(model, b, mx.array([True]), mx.array([False]), um, pm, alive, ns, None)
                Us.append(um)
                Ps.append(pm)
                NS.append(ns)
                continue
            hf = body_any(model, b, um, pm)
            if isinstance(model, MHNet):
                um, pm, alive, _ = model.step_mh(hf, b['tok'], b['slot'], b['tmask'], um, pm, alive, b['hist'], b['hmask'])
            else:
                um, pm, alive, _ = model.step(hf, b['tok'], b['slot'], b['tmask'], um, pm, alive)
            um, pm, alive = forced(um, pm, alive, i)
            Us.append(um)
            Ps.append(pm)
        turns += len(ms)
        for i in range(P.first[p], P.first[p] + P.count[p]):
            r = rust[i]
            c = max(1, flat.c[i])
            cg = next(x for x in C_GROUPS if c <= x)
            b = dev.batch_m(mx.array([i], mx.int32), tm, cg)
            k = P.turn[i]
            b['hplan'] = hplan(b, k)
            with_map(b, maps, k)
            if is_mbn:
                for key in model.NOTE_STATE:
                    b['ns_' + key] = NS[k][key]
                b['ns_off'] = mx.array([False])
            hf = model.body_mn(b['tok'], b['dist'], b['tmask'], b['slot'], Us[k], Ps[k], b['hist'], b['hmask'], b['hslot'], b['hplan'], NS[k]) if is_mn else body_any(model, b, Us[k], Ps[k])
            h = hf[:, :tm]
            pl = model.pointer_logits(h, b['sel'])[0].tolist()
            t = flat.t[i]
            dp = abs(r['ptr'][t] - pl[tm])
            for j in range(t):
                if r['ptr'][j] is not None:
                    dp = max(dp, abs(r['ptr'][j] - pl[j]))
            worst_p = max(worst_p, dp)
            da = 0.0
            third_decisions += flat.x[i] > 0
            if flat.ptr[i] >= 0:
                if is_mbt:
                    al = model.action_logits_t(h, b)[0].tolist()
                elif isinstance(model, MBANet):
                    al = model.action_logits_a(h, b['unit'], b['cand'], b['promo'], b['cmask'], b['gcand'], b['cand_ok'])[0][0].tolist()
                elif 'ww' in w:
                    al = model.action_logits_g(h, b['unit'], b['cand'], b['promo'], b['cmask'], b['gcand'], b['cand_ok'], b['tok'], b['slot'], Us[k], Ps[k])[0].tolist()
                elif has_map:
                    al = model.action_logits_g(h, b['unit'], b['cand'], b['promo'], b['cmask'], b['gcand'], b['cand_ok'])[0].tolist()
                else:
                    al = model.action_logits(h, b['unit'], b['cand'], b['promo'], b['cmask'])[0].tolist()
                for j, v in enumerate(r['act'][:len(al)]):
                    da = max(da, abs(v - al[j]))
                worst_a = max(worst_a, da)
            if worst_out:
                worst_out.write(json.dumps({'i': i, 'p': dp, 'a': da, 'x': flat.x[i], 't': t, 'fam': flat.plays[flat.play[i]]['family'], 'turn': k}) + '\n')
            checked += 1
    print(json.dumps({'plays': len(plays), 'decisions': checked, 'with_third_tokens': third_decisions, 'memory_updates': turns, 'max_abs_pointer': worst_p, 'max_abs_action': worst_a, 'ok': worst_p <= 1e-4 and worst_a <= 1e-4}))


if __name__ == '__main__':
    main()
