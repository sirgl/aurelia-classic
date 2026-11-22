"""Throughput of the MLX tactical simulator: scenes stepped per second on the GPU, the whole loop on
the device (no host round trip inside a turn; the host only launches steps).

    python -m aurelia_nn.tacsim.bench DUMP.jsonl --batch 1000 --rounds 4 --policy random|net --mode seq|wave|group

A batch replicates the dumped scenes (each copy with its own combat RNG stream). A round is: for each
side, its units act one after another (step k = the k-th unit of the side in every scene: masks ->
policy -> apply), its cities may strike, its turn ends; then the round ends.

Policies: `random` (uniform over the legal options and "nothing", as the dump's script), `net` (a small
MLP over the acting unit's local disc, a stand-in for the big net: encode -> logits over the disc's
move and attack tiles and the unit verbs -> masked argmax), all on the GPU.

Modes (`net` only changes who decides; the rules applied are the same):
- `seq`: one unit per step (steps per side-turn = the most units a side has in the batch);
- `wave`: units out of contact (no enemy unit or city within their moves + 3) are decided together in one
  launch (their options do not depend on each other: their reach discs do not meet an enemy), applied in
  slot order inside it with a conflict check; units in contact go one by one;
- `group`: one launch decides every unit of the side from the turn-start state (the net outputs all
  units' actions at once); applied in slot order with a conflict check (an action no longer legal
  becomes "nothing").
"""

import argparse
import json
import time

import mlx.core as mx
import mlx.nn as nn

from .geometry import Geometry
from .scene import Batch, load_lines
from .sim import first_index, Sim, A_UNIT, A_CITY, A_END, A_ROUND, A_PAD, V_NONE, V_FORT, V_SETUP, V_FOUND, V_MOVE, V_ATTACK, V_STRIKE, g1, gn

I32 = mx.int32


def replicate(path, batch_size, limit=None, families=None, seed=1):
    from .parity import sight_radius
    dumps = load_lines(path, limit=limit, families=families)
    tb = Batch(dumps)
    mr = max(1, max(p["moves_land"] for d in dumps for p in d["scene"]["profiles"]) // 60)
    geo = Geometry(tb.w, radius=sight_radius(dumps), move_radius=max(mr, 4))
    has_near = any(t["cond"][0] == "other" for d in dumps for p in d["scene"]["profiles"] for t in p["terms"])
    has_air = any(p["domain"] == "Air" or p.get("air_intercept_range", 0) > 0 for d in dumps for p in d["scene"]["profiles"])
    kw = dict(move_radius=mr, has_near_imp=has_near, dest_pool=tb.dest_pool, withdraw_pool=tb.withdraw_pool, has_air=has_air)
    tsim = Sim(geo, tb.m, tb.cost_pool, tb.cstatic, tb.n_pieces, extra=tb.extra(), **kw)
    assert all(o >= 0 for o in geo.disc_to_local_list)
    idx = mx.arange(batch_size) % tb.b
    take = lambda a: mx.take(a, idx, axis=0)
    m = {k: take(v) for k, v in tb.m.items()}
    cs = {k: take(v) for k, v in tb.cstatic.items()}
    ex = {k: take(v) for k, v in tb.extra().items()}
    sim = Sim(geo, m, tb.cost_pool, cs, take(tb.n_pieces), los=take(tsim.los), extra=ex, **kw)
    s0 = tb.initial_state()
    s0["n_unmodelled"] = mx.zeros((tb.b,), dtype=mx.int32)
    s = {k: take(v) for k, v in s0.items()}
    # Each copy its own combat stream.
    salt = (mx.arange(batch_size).astype(mx.uint64) * 0x2545F4914F6CDD1D) | 1
    s["rng"] = s["rng"] ^ salt
    mx.eval(s, sim.los)
    return sim, s, tb, dumps


class TacticNet(nn.Module):
    """The stand-in policy: the acting unit's local disc (L tiles x F features) -> MLP -> logits."""

    def __init__(self, L, F=12, hidden=256):
        super().__init__()
        self.l1 = nn.Linear(L * F, hidden)
        self.l2 = nn.Linear(hidden, hidden)
        self.out = nn.Linear(hidden, 2 * L + 4)

    def __call__(self, x):
        h = nn.relu(self.l1(x))
        h = nn.relu(self.l2(h))
        return self.out(h)


def features(sim, s, slot, mk):
    """[B, L*F]: per tile of the acting unit's disc: own/enemy unit there (and its hp), city, hills,
    rough, water, reach (moves left), attack ok; the unit's own hp and moves."""
    lt = mk["lt"]
    B, L = lt.shape
    mil, civ = sim.occupancy(s)
    side = g1(s["u_side"], slot)
    ms = gn(mil, lt)
    has = (lt >= 0) & (ms >= 0)
    uside = mx.where(has, gn(s["u_side"], ms), -1)
    uhp = mx.where(has, gn(s["u_hp"], ms), 0).astype(mx.float32) / 100
    own = (uside == side[:, None]).astype(mx.float32)
    foe = ((uside >= 0) & (uside != side[:, None])).astype(mx.float32)
    city = (sim.city_on(s, lt) >= 0).astype(mx.float32)
    f = lambda plane: mx.where(lt >= 0, gn(plane.astype(I32), lt), 0).astype(mx.float32)
    reach = mx.maximum(mk["left"], 0).astype(mx.float32) / 300
    att = (mx.take_along_axis(mk["attack_d"], mx.broadcast_to(mx.maximum(sim.geo.local_to_disc, 0)[None], (B, L)), axis=1) & (sim.geo.local_to_disc >= 0)[None]).astype(mx.float32)
    hp = (g1(s["u_hp"], slot).astype(mx.float32) / 100)[:, None] * mx.ones((1, L))
    mv = (g1(s["u_mv"], slot).astype(mx.float32) / 300)[:, None] * mx.ones((1, L))
    x = mx.stack([own, foe, uhp * own, uhp * foe, city, f(sim.m["hills"]), f(sim.m["rough"]), f(sim.m["water"]), reach, att, hp, mv], axis=2)
    return x.reshape(B, -1)


def decide(sim, s, slot, mk, policy, net, key):
    """(verb, target tile) per scene from the masks."""
    lt = mk["lt"]
    B, L = lt.shape
    mv_l = mk["move_l"]
    at_l = mx.take_along_axis(mk["attack_d"], mx.broadcast_to(mx.maximum(sim.geo.local_to_disc, 0)[None], (B, L)), axis=1) & (sim.geo.local_to_disc >= 0)[None]
    unit_ok = mx.stack([mx.ones((B,), dtype=mx.bool_), mk["fort"], mk["setup"], mk["found"]], axis=1)
    legal = mx.concatenate([mv_l, at_l, unit_ok], axis=1)
    if policy == "random":
        score = mx.random.uniform(shape=legal.shape, key=key)
    else:
        score = net(features(sim, s, slot, mk))
    score = mx.where(legal, score, -1e9)
    pick = first_index(score == mx.max(score, axis=1, keepdims=True))
    is_move = pick < L
    is_att = (pick >= L) & (pick < 2 * L)
    li = mx.where(is_move, pick, pick - L)
    tgt = mx.where(is_move | is_att, g1(lt, mx.clip(li, 0, L - 1)), -1)
    verbs = mx.array([V_NONE, V_FORT, V_SETUP, V_FOUND], dtype=I32)
    verb = mx.where(is_move, V_MOVE, mx.where(is_att, V_ATTACK, mx.take(verbs, mx.clip(pick - 2 * L, 0, 3))))
    return verb, tgt


def kth_unit(s, side, k, sel=None):
    own = s["u_alive"] & (s["u_side"] == side[:, None])
    if sel is not None:
        own = own & sel
    rank = mx.cumsum(own.astype(I32), axis=1) - 1
    hit = own & (rank == k)
    return mx.where(mx.any(hit, axis=1), first_index(hit), -1)


def in_contact(sim, s, side):
    """Units of `side` with an enemy unit or city within their moves + 3 tiles (the others neither see a
    zone of control nor reach a target this turn: their options do not depend on the other side)."""
    tiles = s["u_tile"]
    d = sim.geo.distance(tiles[:, :, None], tiles[:, None, :])
    enemy = s["u_alive"][:, None, :] & (s["u_side"][:, None, :] != side[:, None, None])
    reach = (s["u_mmax"] // 60 + 3)[:, :, None]
    near = mx.any(enemy & (d <= reach), axis=2)
    ct = sim.cs["tile"]
    dc = sim.geo.distance(tiles[:, :, None], ct[:, None, :])
    ec = sim.cs["alive"][:, None, :] & (s["c_owner"][:, None, :] != side[:, None, None])
    near = near | mx.any(ec & (dc <= reach), axis=2)
    return near & s["u_alive"] & (s["u_side"] == side[:, None])


class Runner:
    def __init__(self, sim, policy, net, compile_=True):
        self.sim, self.policy, self.net = sim, policy, net
        B = sim.B
        self.B = B
        self.compile_ = compile_

        def unit_step(s, side, k, sel, key):
            slot = kth_unit(s, side, k, sel)
            mk = sim.masks(s, side, slot)
            verb, tgt = decide(sim, s, slot, mk, policy, net, key)
            a = {"typ": mx.where(slot >= 0, A_UNIT, A_PAD), "side": side, "slot": slot, "verb": verb, "tgt": tgt}
            return sim.step(s, a, mk, kinds=(A_UNIT,))

        def decide_fn(s, side, k, sel, key):
            slot = kth_unit(s, side, k, sel)
            mk = sim.masks(s, side, slot)
            verb, tgt = decide(sim, s, slot, mk, policy, net, key)
            return slot, verb, tgt

        def apply_fn(s, side, slot, verb, tgt):
            """Apply a decision taken on an earlier state: an action no longer legal becomes nothing."""
            mk = sim.masks(s, side, slot)
            legal = mx.where(verb == V_MOVE, mx.any(mk["move_l"] & (mk["lt"] == tgt[:, None]), axis=1),
                             mx.where(verb == V_ATTACK, mx.any(mk["attack_d"] & (mk["tgt_d"] == tgt[:, None]), axis=1), True))
            a = {"typ": mx.where(slot >= 0, A_UNIT, A_PAD), "side": side, "slot": slot, "verb": mx.where(legal, verb, V_NONE), "tgt": tgt}
            return sim.step(s, a, mk, kinds=(A_UNIT,))

        def city_step(s, side, c, key):
            cslot = mx.full((B,), c, dtype=I32)
            cm = sim.city_mask(s, side, cslot)
            score = mx.where(cm, mx.random.uniform(shape=cm.shape, key=key), -1.0)
            tgt = first_index(score == mx.max(score, axis=1, keepdims=True))
            ok = mx.any(cm, axis=1)
            a = {"typ": mx.full((B,), A_CITY, dtype=I32), "side": side, "slot": cslot, "verb": mx.where(ok, V_STRIKE, V_NONE), "tgt": tgt}
            return sim.step(s, a, {}, kinds=(A_CITY,))

        def end_step(s, side):
            return sim.end_turn(dict(s), side, mx.ones((B,), dtype=mx.bool_))

        def round_step(s):
            return sim.round_end(dict(s), mx.ones((B,), dtype=mx.bool_))

        c = mx.compile if compile_ else (lambda f: f)
        self.unit_step = c(unit_step)
        self.city_step = c(city_step)
        self.end_step = c(end_step)
        self.round_step = c(round_step)
        self.decide_fn = c(decide_fn)
        self.apply_fn = c(apply_fn)

    def group(self, s, side, n, sel, key):
        """n units decided on the same state (one net forward each, no sync between), then applied in
        order; the host syncs once after."""
        keys = mx.random.split(key, n)
        decided = [self.decide_fn(s, side, mx.array(k, dtype=I32), sel, keys[k]) for k in range(n)]
        for slot, verb, tgt in decided:
            s = self.apply_fn(s, side, slot, verb, tgt)
        return s


def play(runner, s, rounds, mode="seq", seed=0):
    """Plays `rounds`; returns (state, launches, seconds, units per side)."""
    sim = runner.sim
    B = sim.B
    key = mx.random.key(seed)
    U = s["u_alive"].shape[1]
    every = mx.ones((B, U), dtype=mx.bool_)
    n_side = [int(mx.max(mx.sum((s["u_alive"] & (s["u_side"] == side)).astype(I32), axis=1)).item()) for side in (0, 1)]
    C = sim.cs["tile"].shape[1]
    has_city = bool(mx.any(sim.cs["alive"]).item())
    launches = 0
    unit_launches = 0
    t0 = time.perf_counter()
    for r in range(rounds):
        for side_v in (0, 1):
            side = mx.full((B,), side_v, dtype=I32)
            K = n_side[side_v]
            if mode == "seq":
                for k in range(K):
                    key, sub = mx.random.split(key)
                    s = runner.unit_step(s, side, mx.array(k, dtype=I32), every, sub)
                    launches += 1
                    unit_launches += 1
                    mx.eval(s)
            elif mode == "group":
                key, sub = mx.random.split(key)
                s = runner.group(s, side, K, every, sub)
                launches += 1
                unit_launches += 1
                mx.eval(s)
            elif mode == "seq_async":
                for k in range(K):
                    key, sub = mx.random.split(key)
                    s = runner.unit_step(s, side, mx.array(k, dtype=I32), every, sub)
                unit_launches += K
                launches += 1
                mx.eval(s)
            elif mode == "wave":
                contact = in_contact(sim, s, side)
                far = ~contact
                n_far = int(mx.max(mx.sum((far & s["u_alive"] & (s["u_side"] == side_v)).astype(I32), axis=1)).item())
                n_con = int(mx.max(mx.sum(contact.astype(I32), axis=1)).item())
                if n_far:
                    # One compiled shape per side (K), the ranks past a scene's far units are empty slots.
                    key, sub = mx.random.split(key)
                    s = runner.group(s, side, K, far, sub)
                    launches += 1
                    unit_launches += 1
                    mx.eval(s)
                for k in range(n_con):
                    key, sub = mx.random.split(key)
                    s = runner.unit_step(s, side, mx.array(k, dtype=I32), contact, sub)
                    launches += 1
                    unit_launches += 1
                    mx.eval(s)
            if has_city:
                for c in range(C):
                    key, sub = mx.random.split(key)
                    s = runner.city_step(s, side, c, sub)
                    launches += 1
            s = runner.end_step(s, side)
            launches += 1
            mx.eval(s)
        s = runner.round_step(s)
        launches += 1
        mx.eval(s)
    return s, launches, unit_launches, time.perf_counter() - t0, n_side


def run(path, batches, rounds, policy, modes, limit=None, out=None, families=None):
    rows = []
    for B in batches:
        sim, s0, tb, dumps = replicate(path, B, limit=limit, families=families)
        net = None
        if policy == "net":
            net = TacticNet(sim.geo.l)
            mx.eval(net.parameters())
        runner = Runner(sim, policy, net)
        for mode in modes:
            # Warm-up (compilation) on one round, then the measurement.
            play(runner, dict(s0), 1, mode=mode, seed=99)
            t = time.perf_counter()
            s, launches, unit_launches, secs, n_side = play(runner, dict(s0), rounds, mode=mode, seed=1)
            row = {"batch": B, "policy": policy, "mode": mode, "rounds": rounds, "seconds": round(secs, 3),
                   "launches": launches, "unit_launches_per_side_turn": unit_launches / (2 * rounds), "ms_per_launch": round(1000 * secs / launches, 3),
                   "scene_rounds_per_s": round(B * rounds / secs, 1), "units_per_side": n_side, "templates": tb.b}
            rows.append(row)
            print(json.dumps(row), flush=True)
    if out:
        with open(out, "w") as f:
            json.dump(rows, f, indent=1)
    return rows


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("dump")
    ap.add_argument("--batch", default="1,1000,10000")
    ap.add_argument("--rounds", type=int, default=3)
    ap.add_argument("--policy", default="random")
    ap.add_argument("--modes", default="seq")
    ap.add_argument("--limit", type=int)
    ap.add_argument("--families")
    ap.add_argument("--out")
    o = ap.parse_args(argv)
    run(o.dump, [int(x) for x in o.batch.split(",")], o.rounds, o.policy, o.modes.split(","), o.limit, o.out,
        o.families.split(",") if o.families else None)


if __name__ == "__main__":
    main()
