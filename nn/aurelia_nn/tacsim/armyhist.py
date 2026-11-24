"""The army net's history of facts in the MLX twin, batched over scenes: the twin of the history in
`crates/qa/src/army_net/observe.rs` (`History`, `log_before` / `log_after`, `note_turn_end`, `note_enemy_events`,
`history_tokens`), which variant MBH reads (`model_mb.MBHNet`).

A fact is written per command of an army unit of the learner's side that the rules carry out (the net's moves,
strikes, fortify, set up, promotions, pillage, intercept; the scene rule's promotions, shots and fortify; a unit's
"done" and its stand are no command): its kind (0 move, 1 strike a unit, 2 strike a city, 3 ranged, 4 fortify / set
up, 5 other), the subject tile (the target's, else the unit's tile after the command) against the objective (dq,
dr / 12, distance / 12), dealt / 100 (the target's hit points before minus after), taken / 100 (the unit's), a kill
(the target left with 0 or the city now ours), died, moved / 5, on a city (the target is a city). At our turn's start
the enemy's events since our last turn's end (the snapshot at our turn's end): our army units hit (kind 7: the hit
points lost, died, the unit in a foe city's fire), foes come into sight (kind 8) and gone (kind 9), each class in
entity-id order. The tokens of a moment: the facts of this turn and the last H_TURNS turns, the most recent first, at
most H_MAX, age / 3 in number 10, every number through f16; the army slot of the fact's unit (-1 the enemy's) and its
age in turns.

The state lives in the game's state dict `s` (so the compacted decision steps carry it): a ring of the last H_MAX
facts a row (`h_f` [B, R, F], `h_rnd`, `h_slot`, `h_n` facts written so far), the snapshot of our turn's end
(`h_end`, `h_ea`, `h_et`, `h_eh`, `h_es`, `h_eft`) and the plan token P_0 in force in the last H_TURNS turns
(`h_p0` [B, H_TURNS, d], for MBH's plan of a fact's turn).
"""

import mlx.core as mx

from . import armyobs as A
from .sim import g1, gn, I32

F_HIST = 20
H_MAX = 32
H_TURNS = 3
HK_MOVE, HK_UNIT, HK_CITY, HK_RANGED, HK_FORT, HK_OTHER, HK_HIT, HK_SEEN, HK_GONE = 0, 1, 2, 3, 4, 5, 7, 8, 9
F32 = mx.float32
# The candidate kinds (armyobs K_*) -> the fact's kind (observe.rs `log_before`): the command each candidate runs.
KIND_OF = {A.K_MOVE: HK_MOVE, A.K_UNIT: HK_UNIT, A.K_CITY: HK_CITY, A.K_RANGED: HK_RANGED, A.K_FORT: HK_FORT,
           A.K_SETUP: HK_FORT, A.K_PROMO: HK_OTHER, A.K_PILLAGE: HK_OTHER, A.K_OTHER: HK_OTHER}


def init(s, B, U, plan0=None):
    """The empty history of every row (a new play), in the state dict."""
    s["h_f"] = mx.zeros((B, H_MAX, F_HIST), dtype=F32)
    s["h_rnd"] = mx.zeros((B, H_MAX), dtype=I32)
    s["h_slot"] = mx.full((B, H_MAX), -1, dtype=I32)
    s["h_n"] = mx.zeros((B,), dtype=I32)
    s["h_end"] = mx.zeros((B,), dtype=mx.bool_)
    s["h_ea"] = mx.zeros((B, U), dtype=mx.bool_)
    s["h_et"] = mx.zeros((B, U), dtype=I32)
    s["h_eh"] = mx.zeros((B, U), dtype=I32)
    s["h_es"] = mx.zeros((B, U), dtype=mx.bool_)
    s["h_eft"] = mx.zeros((B, U), dtype=I32)
    if plan0 is not None:
        s["h_p0"] = mx.broadcast_to(plan0[None, None, :], (B, H_TURNS, plan0.shape[-1]))


def append(s, valid, f, slot, rnd):
    """Write the facts `f` [B, E, F] where `valid` [B, E], in the order of E, each row's at its round `rnd` [B] with
    the army slots `slot` [B, E]: the ring keeps the last H_MAX of each row."""
    R = H_MAX
    v = valid.astype(I32)
    rank = mx.cumsum(v, axis=1) - v
    count = mx.sum(v, axis=1)
    keep = valid & (rank >= (count - R)[:, None])
    pos = (s["h_n"][:, None] + rank) % R
    hot = (pos[:, :, None] == mx.arange(R, dtype=I32)[None, None, :]) & keep[:, :, None]  # [B, E, R]
    written = mx.any(hot, axis=1)
    hf = hot.astype(F32)
    newf = mx.einsum("ber,bef->brf", hf, f)
    hi = hot.astype(I32)
    news = mx.sum(hi * slot[:, :, None], axis=1)
    s["h_f"] = mx.where(written[:, :, None], newf, s["h_f"])
    s["h_slot"] = mx.where(written, news, s["h_slot"])
    s["h_rnd"] = mx.where(written, rnd[:, None], s["h_rnd"])
    s["h_n"] = s["h_n"] + count


def tokens(s, rnd, pm=None):
    """The history tokens now (`history_tokens`): hist [B, H_MAX, F] (f16, age / 3 in number 10), mask, slot (-1 none),
    age [B, H_MAX] (int), and with `pm` the plan of each fact's turn hplan [B, H_MAX, d]."""
    R = H_MAX
    k = mx.arange(R, dtype=I32)[None, :]
    n = s["h_n"][:, None]
    idx = (n - 1 - k) % R
    idx = mx.where(idx < 0, idx + R, idx)
    have = k < n
    hr = mx.take_along_axis(s["h_rnd"], idx, axis=1)
    age = rnd[:, None] - hr
    ok = have & (age <= H_TURNS) & (age >= 0)
    f = mx.take_along_axis(s["h_f"], idx[:, :, None], axis=1)
    col = mx.arange(F_HIST)[None, None, :] == 10
    f = mx.where(col, A.fdiv(age, 3.0)[:, :, None], f)
    f = mx.where(ok[:, :, None], A.f16(f), 0.0)
    slot = mx.where(ok, mx.take_along_axis(s["h_slot"], idx, axis=1), -1)
    age = mx.where(ok, age, 0)
    out = {"hist": f, "hmask": ok, "hslot": slot, "hage": age}
    if pm is not None:
        p0 = pm[:, 0]
        ring = s["h_p0"]
        a1 = mx.clip(age - 1, 0, H_TURNS - 1)
        past = mx.take_along_axis(ring, mx.broadcast_to(a1[:, :, None], a1.shape + (ring.shape[-1],)), axis=1)
        out["hplan"] = mx.where((age == 0)[:, :, None], p0[:, None, :], past)
    return out


def push_plan(s, pm_old, live):
    """After the memory's step: the plan in force in the turn that ended becomes age 1 (`p0_turns`)."""
    ring = s["h_p0"]
    new = mx.concatenate([pm_old[:, 0][:, None, :], ring[:, :H_TURNS - 1]], axis=1)
    s["h_p0"] = mx.where(live[:, None, None], new, ring)


# ------------------------------------------------------------------------------------------------- the numbers

def subject(game, tile, f):
    """Numbers 11-13 (`subject_numbers`): the window tile [B] against the objective."""
    t = game.tab.t
    q, r = game.obs.mapq(tile)
    dq, dr = q - t["obj_q"], r - t["obj_r"]
    f[11] = A.fdiv(dq, 12.0)
    f[12] = A.fdiv(dr, 12.0)
    f[13] = A.fdiv(A.hexd(dq, dr), 12.0)


def hp_at(game, s, tile):
    """`hp_at` for the learner (side 0) on window tiles [B]: (hit points, a city's, anything there)."""
    sim = game.sim
    city = sim.city_on(s, tile[:, None])[:, 0]
    is_city = (tile >= 0) & (city >= 0) & g1(sim.cs["alive"], mx.maximum(city, 0))
    chp = sim.city_maxhp(s, mx.maximum(city, 0)) - g1(s["c_dmg"], mx.maximum(city, 0))
    mil, civ = sim.occupancy(s)
    tl = mx.maximum(tile, 0)[:, None]
    ms, cv = gn(mil, tl)[:, 0], gn(civ, tl)[:, 0]
    # Rust's `hp_at`: the first unit of the tile whose owner is not the learner (the enemy's or the third seat's), not "the enemy's".
    other = lambda sl: (tile >= 0) & (sl >= 0) & (g1(s["u_side"], mx.maximum(sl, 0)) >= 1)
    foe_m, foe_c = other(ms), other(cv)
    uhp = mx.where(foe_m, g1(s["u_hp"], mx.maximum(ms, 0)), g1(s["u_hp"], mx.maximum(cv, 0)))
    hp = mx.where(is_city, chp, mx.where(foe_m | foe_c, uhp, 0))
    return hp, is_city, is_city | foe_m | foe_c


def before(game, me, kind, tgt, take):
    """What a command of the units `me` [B] needs for its outcome (`log_before`); `kind` [B] the fact's kind, `tgt` [B]
    the target's window tile for a strike (else ignored). None when no row takes an army unit's command."""
    s = game.s
    army = g1(game.tab.t["army"], me)
    ok = take & (army >= 0) & g1(s["u_alive"], me) & (g1(s["u_side"], me) == 0) & (kind >= 0)
    if not bool(mx.any(ok).item()):
        return None
    strike = (kind == HK_UNIT) | (kind == HK_CITY) | (kind == HK_RANGED)
    thp, tcity, there = hp_at(game, s, mx.where(strike, tgt, -1))
    return {"ok": ok, "me": me, "kind": kind, "slot": army, "hp": g1(s["u_hp"], me), "tile": g1(s["u_tile"], me),
            "tgt": mx.where(strike, tgt, -1), "has_t": strike, "thp": thp, "tcity": tcity, "there": strike & there}


def after(game, pre, rnd):
    """The outcome of the command (`log_after`) becomes a fact of round `rnd` [B]."""
    if pre is None:
        return
    s = game.s
    me = pre["me"]
    B = me.shape[0]
    alive = g1(s["u_alive"], me) & (g1(s["u_side"], me) == 0)
    now_t = g1(s["u_tile"], me)
    now_hp = mx.where(alive, g1(s["u_hp"], me), 0)
    z = mx.zeros((B,), dtype=F32)
    f = [z] * F_HIST
    for kk in (HK_MOVE, HK_UNIT, HK_CITY, HK_RANGED, HK_FORT, HK_OTHER):
        f[kk] = (pre["kind"] == kk).astype(F32)
    subj = mx.where(pre["has_t"], pre["tgt"], mx.where(alive, now_t, pre["tile"]))
    subject(game, subj, f)
    ahp, acity, athere = hp_at(game, s, pre["tgt"])
    after_hp = mx.where(athere & (acity == pre["tcity"]), ahp, 0)
    city = game.sim.city_on(s, mx.maximum(pre["tgt"], 0)[:, None])[:, 0]
    ours = pre["tcity"] & (city >= 0) & (g1(s["c_owner"], mx.maximum(city, 0)) == 0)
    th = pre["there"]
    f[14] = mx.where(th, A.fdiv(mx.maximum(pre["thp"] - after_hp, 0), 100.0), 0.0)
    f[16] = mx.where(th & ((after_hp == 0) | ours), 1.0, 0.0)
    f[15] = A.fdiv(mx.maximum(pre["hp"] - now_hp, 0), 100.0)
    f[17] = (~alive).astype(F32)
    q0, r0 = game.obs.mapq(pre["tile"])
    q1, r1 = game.obs.mapq(now_t)
    f[18] = mx.where(alive, A.fdiv(A.hexd(q1 - q0, r1 - r0), 5.0), 0.0)
    f[19] = pre["tcity"].astype(F32)
    ff = mx.stack(f, axis=1)[:, None, :]
    append(s, pre["ok"][:, None], ff, pre["slot"][:, None], rnd)


def log_cmd(game, me, kind, tgt, take, rnd, run):
    """`run()` applies the command; the fact of each row that took an army unit's command is written after it."""
    pre = before(game, me, kind, tgt, take)
    run()
    after(game, pre, rnd)


def turn_end(game, live):
    """`note_turn_end`: our army (alive, tile, hp) and the foes seen at our turn's end."""
    s = game.s
    army = (game.tab.t["army"] >= 0)
    a = s["u_alive"] & (s["u_side"] == 0) & army
    fo = s["u_alive"] & (s["u_side"] == 1) & (s["u_tile"] >= 0)
    if game.obs.fog_any:
        seen = game.sim.seen_by(s, mx.zeros((s["u_alive"].shape[0],), dtype=I32), mx.where(fo, s["u_tile"], -1))
        fo = fo & (seen | ~game.tab.t["fog"][:, None])
    L = live[:, None]
    s["h_end"] = mx.where(live, True, s["h_end"])
    s["h_ea"] = mx.where(L, a, s["h_ea"])
    s["h_et"] = mx.where(L, s["u_tile"], s["h_et"])
    s["h_eh"] = mx.where(L, s["u_hp"], s["h_eh"])
    s["h_es"] = mx.where(L, fo, s["h_es"])
    s["h_eft"] = mx.where(L, s["u_tile"], s["h_eft"])


def turn_start(game, live, rnd):
    """`note_enemy_events` at our turn's first look: our army units hit, foes come into sight, foes gone (each class in
    entity-id order), against the snapshot of our last turn's end (none before the first)."""
    s = game.s
    B, U = s["u_alive"].shape
    have = live & s["h_end"]
    if not bool(mx.any(have).item()):
        return
    order = mx.argsort(s["u_id"], axis=1).astype(I32)  # [B, U]: the slots by entity id
    tk = lambda a: mx.take_along_axis(a, order, axis=1)
    army = game.tab.t["army"]
    alive = s["u_alive"] & (s["u_side"] == 0) & (army >= 0)
    hp_now = mx.where(alive, s["u_hp"], 0)
    lost = mx.maximum(s["h_eh"] - hp_now, 0)
    hit = s["h_ea"] & (lost > 0)
    t_hit = mx.where(alive, s["u_tile"], s["h_et"])
    fd = game.obs.field(s, 0)
    q, r = game.obs.mapq(t_hit)
    dc = A.hexd(q[:, :, None] - fd["cq"][:, None, :], r[:, :, None] - fd["cr"][:, None, :])
    fire = mx.any(fd["foe_city"][:, None, :] & (dc <= A.CITY_RANGE), axis=2)
    fo = s["u_alive"] & (s["u_side"] == 1) & (s["u_tile"] >= 0)
    if game.obs.fog_any:
        seen = game.sim.seen_by(s, mx.zeros((B,), dtype=I32), mx.where(fo, s["u_tile"], -1))
        fo = fo & (seen | ~game.tab.t["fog"][:, None])
    new = fo & ~s["h_es"]
    gone = s["h_es"] & ~fo
    E = 3 * U
    zf = mx.zeros((B, U), dtype=F32)
    rows = []
    for kind, valid, tile in ((HK_HIT, hit, t_hit), (HK_SEEN, new, s["u_tile"]), (HK_GONE, gone, s["h_eft"])):
        f = [zf] * F_HIST
        f[kind] = mx.ones((B, U), dtype=F32)
        tq, tr = game.obs.mapq(tile)
        dq, dr = tq - game.tab.t["obj_q"][:, None], tr - game.tab.t["obj_r"][:, None]
        f[11] = A.fdiv(dq, 12.0)
        f[12] = A.fdiv(dr, 12.0)
        f[13] = A.fdiv(A.hexd(dq, dr), 12.0)
        if kind == HK_HIT:
            f[15] = A.fdiv(lost, 100.0)
            f[17] = (~alive).astype(F32)
            f[19] = fire.astype(F32)
            slot = army
        else:
            slot = mx.full((B, U), -1, dtype=I32)
        ff = mx.stack(f, axis=2)
        rows.append((tk(valid) & have[:, None], mx.take_along_axis(ff, order[:, :, None], axis=1), tk(slot)))
    valid = mx.concatenate([x[0] for x in rows], axis=1)
    ff = mx.concatenate([x[1] for x in rows], axis=1)
    slot = mx.concatenate([x[2] for x in rows], axis=1)
    assert valid.shape[1] == E
    append(s, valid, ff, slot, rnd)
    s["h_end"] = mx.where(have, False, s["h_end"])


# ------------------------------------------------------------------------------------------------- the dump's tokens

def unhex_f16(h):
    """The dump's f16 bits (hex, 4 digits a number) as floats."""
    import struct
    b = bytes.fromhex(h)
    return list(struct.unpack(">%de" % (len(b) // 2), b))


def of_moment(m):
    """The history tokens of a dump moment (`obsdump::moment` "hist"): ([[F numbers]], [slot], [age])."""
    hx = m.get("hist")
    if hx is None:
        return None
    v = unhex_f16(hx["f"])
    n = len(hx["slot"])
    return [v[i * F_HIST:(i + 1) * F_HIST] for i in range(n)], list(hx["slot"]), list(hx["age"])


def dump_tokens(ms):
    """The dump moments' history tokens as `tokens`' arrays (without the plan)."""
    B = len(ms)
    f = [[[0.0] * F_HIST for _ in range(H_MAX)] for _ in range(B)]
    mask = [[False] * H_MAX for _ in range(B)]
    slot = [[-1] * H_MAX for _ in range(B)]
    age = [[0] * H_MAX for _ in range(B)]
    for b, m in enumerate(ms):
        t = of_moment(m)
        if t is None:
            continue
        for i, (x, sl, a) in enumerate(zip(*t)):
            if i >= H_MAX:
                break
            f[b][i] = x; mask[b][i] = True; slot[b][i] = sl; age[b][i] = a
    return {"hist": mx.array(f, dtype=F32), "hmask": mx.array(mask), "hslot": mx.array(slot, dtype=I32), "hage": mx.array(age, dtype=I32)}


def plan_of(hx, pm, ring):
    """MBH's plan of each fact's turn for tokens `hx` (age 0: the plan now `pm[:, 0]`; else the ring of P_0 by age)."""
    age = hx["hage"]
    a1 = mx.clip(age - 1, 0, H_TURNS - 1)
    past = mx.take_along_axis(ring, mx.broadcast_to(a1[:, :, None], a1.shape + (ring.shape[-1],)), axis=1)
    return mx.where((age == 0)[:, :, None], pm[:, 0][:, None, :], past)


def plain_facts(game, which, kind, rnd):
    """Facts of a command without a target that changes nothing the fact reads (the scene rule's fortify), for the
    units `which` [B, U] in slot order: the kind, the unit's tile against the objective."""
    s = game.s
    B, U = which.shape
    army = game.tab.t["army"]
    ok = which & (army >= 0) & s["u_alive"] & (s["u_side"] == 0)
    if not bool(mx.any(ok).item()):
        return
    zf = mx.zeros((B, U), dtype=F32)
    f = [zf] * F_HIST
    f[kind] = mx.ones((B, U), dtype=F32)
    q, r = game.obs.mapq(s["u_tile"])
    dq, dr = q - game.tab.t["obj_q"][:, None], r - game.tab.t["obj_r"][:, None]
    f[11] = A.fdiv(dq, 12.0)
    f[12] = A.fdiv(dr, 12.0)
    f[13] = A.fdiv(A.hexd(dq, dr), 12.0)
    append(s, ok, mx.stack(f, axis=2), army, rnd)


def fact_kind(kind):
    """The fact's kind of the candidate kinds [B] (-1: "done", no command)."""
    out = mx.full(kind.shape, -1, dtype=I32)
    for k, v in KIND_OF.items():
        out = mx.where(kind == k, v, out)
    return out
