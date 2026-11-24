"""The enemy's seat in the trainer scenes as the AI `standard` plays it, in MLX, batched over scenes (part port, part
schedule). What the Rust trainers do on the enemy's turn (`trainers::play_on`):

1. the family script first (`families::enemy_turn`): in the assault families the field units' `act::unit_turn`
   (Close / Support at even odds) — kept from `armyplay.Game.enemy_turn`;
2. then the seat's AI mind (`standard`) for the rest of the seat:
   a. the military advisor at the turn's start (`ai::combat::MilitaryAdvisor::start_of_turn`, `fixed_shots` with the
      fight fix of `standard`): the cities' strikes and the ranged units' shots, focused (`focus_plan`): ported, with
      one part left out — the shot's cost to the shooter (`Exposure::strike_cost`, from the danger net) is 0 here; the
      shooters are garrisons in their city, whose extra loss where they stand is small;
   b. each unit's decision (`planner_units::decide_military`): a garrison in its city charges an adjacent unit whose
      exchange is worth it (`tactics::valued_adjacent_target`, cost 0) when `charge` is on, else holds (fortifies);
      a unit of the seat outside its city at the scene's start walks its march (operations, tasks of the empire: it
      never strikes in the dumps) — that walk is a SCHEDULE from the Rust play (its tile after each enemy turn), like
      the economy's schedule;
3. the turn's end (`Sim.end_turn`).
"""

import mlx.core as mx

from . import armyobs as A
from .sim import g1, gn, put1, I32, V_ATTACK, V_FORT, V_MOVE

NEG = -(1 << 30)


def worth_of(game):
    """`battlefield::worth` of every slot's kind at level 0 (40 a civilian)."""
    s, tab = game.s, game.tab
    k = mx.maximum(s["u_kind"], 0)
    st = mx.take(tab.k["str"], k)
    rs = mx.where(mx.take(tab.k["range"], k) > 0, mx.take(tab.k["rstr"], k), 0)
    civ = mx.take(tab.k["civ"], k)
    return mx.where(civ, 40, 60 + 4 * mx.maximum(st, rs))


def plan_shots(game, side=1):
    """The advisor's plan (`fixed_shots` + `focus_plan`, cost 0): per scene the planned shots in order, city strikes
    first: shooter [B, S] (0..C-1 a city, C.. a unit slot + C; -1 none) and target slot [B, S]."""
    s, sim, obs = game.s, game.sim, game.obs
    B, U = s["u_alive"].shape
    C = sim.cs["tile"].shape[1]
    T = sim.T
    sd = mx.full((B,), side, dtype=I32)
    mil, civ = sim.occupancy(s)
    tile = s["u_tile"]
    tl = mx.maximum(tile, 0)
    # The marks: our units that defend their tile (the military one, else the civilian).
    foe = s["u_alive"] & (tile >= 0) & (s["u_side"] == 1 - side)
    slots = mx.arange(U, dtype=I32)[None, :]
    dm, dc = gn(mil, tl), gn(civ, tl)
    mark = foe & ((dm == slots) | ((dm < 0) & (dc == slots)))
    left0 = mx.where(mark, s["u_hp"], 0)
    worth = worth_of(game)
    # The shots' expected damage [B, S, U]: the cities' strikes, then the ranged units'.
    rows_d, rows_ok, keys = [], [], []
    tiles_all = mx.broadcast_to(mx.arange(T, dtype=I32)[None], (B, T))
    for c in range(C):
        cs = mx.full((B,), c, dtype=I32)
        ok_t = sim.city_mask(s, sd, cs)
        dsl = mx.where(gn(mil, tiles_all) >= 0, gn(mil, tiles_all), gn(civ, tiles_all))
        _, c_rstr = sim.city_strength(s, cs, mil)
        dfn = obs.defense_vs_ranged(s, obs.fighter_n_slots(s, dsl, dsl >= 0), obs.fighter_n_slots(s, mx.full((B, T), -1, dtype=I32), mx.zeros((B, T), dtype=mx.bool_)))
        dmg_t = A.expected_damage(mx.broadcast_to(c_rstr[:, None], (B, T)), dfn, mx.zeros((B, T), dtype=I32), mx.zeros((B, T), dtype=I32))
        rows_d.append(gn(dmg_t.astype(I32), tl))
        rows_ok.append(gn(ok_t.astype(I32), tl) > 0)
        keys.append(mx.full((B,), c, dtype=I32))
    ranged = obs.kind(s, "range") >= 0
    shooter_ok = s["u_alive"] & (tile >= 0) & (s["u_side"] == side) & ranged & (s["u_mv"] > 0) & ~obs.kind(s, "civ")
    handles = s["u_handle"]
    shooters_list = list(game.shooter_slots if getattr(game, "shooter_slots", None) is not None else range(U))
    for u in shooters_list:
        me = mx.full((B,), u, dtype=I32)
        here = g1(tile, me)
        frm = mx.broadcast_to(here[:, None], (B, U))
        dealt, _, dhp, ok = obs.preview(s, me, frm, tile, mark & g1(shooter_ok, me)[:, None], True)
        rows_d.append(dealt)
        rows_ok.append(ok & mark)
        keys.append((1 << 20) + mx.maximum(g1(handles, me), 0))
    S = len(rows_d)
    dealt = mx.stack(rows_d, axis=1)          # [B, S, U]
    valid = mx.stack(rows_ok, axis=1) & mark[:, None, :]
    dealt = mx.where(valid, mx.minimum(dealt, s["u_hp"][:, None, :]), 0)
    key = mx.stack(keys, axis=1)               # [B, S]: the shooters' order (cities, then units by handle)
    rank = mx.argsort(key, axis=1).astype(I32)  # rank[b, j] = the shooter of order j
    order_of = mx.argsort(rank, axis=1).astype(I32)  # order_of[b, s] = the order of shooter s
    used = mx.zeros((B, S), dtype=mx.bool_)
    left = left0
    out_s = mx.full((B, S), -1, dtype=I32)
    out_t = mx.full((B, S), -1, dtype=I32)
    out_n = mx.zeros((B,), dtype=I32)
    BIG = 1 << 20
    # Phase 1: kill plans (the fewest shots, the greatest value, the lowest tile).
    for _ in range(S):
        can = valid & ~used[:, :, None]                          # [B, S, U]
        sort_key = mx.where(can, -dealt * 64 + order_of[:, :, None], BIG)
        perm = mx.argsort(sort_key, axis=1).astype(I32)         # [B, S(sorted), U]
        d_sorted = mx.take_along_axis(mx.where(can, dealt, 0), perm, axis=1)
        cum = mx.cumsum(d_sorted, axis=1)
        enough = cum >= left[:, None, :]
        first = mx.min(mx.where(enough, mx.arange(S)[None, :, None], BIG), axis=1)
        n_need = mx.where(first < BIG, first + 1, BIG)
        live_mark = mark & (left > 0) & (n_need < BIG)
        value = left + worth
        tl_key = tl
        k1 = mx.where(live_mark, n_need, BIG)
        best_n = mx.min(k1, axis=1)
        c2 = live_mark & (k1 == best_n[:, None])
        best_v = mx.max(mx.where(c2, value, NEG), axis=1)
        c3 = c2 & (value == best_v[:, None])
        best_t = mx.min(mx.where(c3, tl_key, BIG), axis=1)
        c4 = c3 & (tl_key == best_t[:, None])
        m = A.first_index_eq(c4.astype(I32), mx.ones((B,), dtype=I32))
        go = best_n < BIG
        n = mx.where(go, best_n, 0)
        pm = mx.take_along_axis(perm, mx.broadcast_to(mx.minimum(m, U - 1)[:, None, None], (B, S, 1)), axis=2)[:, :, 0]  # [B, S]
        take_pos = (mx.arange(S)[None, :] < n[:, None]) & go[:, None]
        # Write the plan's shots in order.
        pos = out_n[:, None] + mx.arange(S)[None, :]
        hot = take_pos[:, :, None] & (pos[:, :, None] == mx.arange(S)[None, None, :])  # [B, j, slot]
        out_s = mx.where(mx.any(hot, axis=1), mx.sum(mx.where(hot, pm[:, :, None], 0), axis=1), out_s)
        out_t = mx.where(mx.any(hot, axis=1), m[:, None], out_t)
        out_n = out_n + n
        sel = mx.zeros((B, S), dtype=mx.bool_)
        sel = mx.any((pm[:, :, None] == mx.arange(S)[None, None, :]) & take_pos[:, :, None], axis=1)
        used = used | sel
        left = mx.where(go[:, None] & (mx.arange(U)[None, :] == m[:, None]), 0, left)
        if not bool(mx.any(go).item()):
            break
    # Phase 2: every shooter left, in its order, at the target its shot leaves closest to death.
    for j in range(S):
        sh = rank[:, j]
        free = ~mx.take_along_axis(used, sh[:, None], axis=1)[:, 0]
        d = mx.take_along_axis(dealt, mx.broadcast_to(sh[:, None, None], (B, 1, U)), axis=1)[:, 0]
        ok = mx.take_along_axis(valid, mx.broadcast_to(sh[:, None, None], (B, 1, U)), axis=1)[:, 0] & (left > 0) & free[:, None]
        val = mx.minimum(d, left) + mx.where(d >= left, worth, 0)
        ok = ok & (val > 0)
        rest = mx.maximum(left - d, 0)
        k1 = mx.where(ok, rest, BIG)
        b1 = mx.min(k1, axis=1)
        c2 = ok & (k1 == b1[:, None])
        b2 = mx.max(mx.where(c2, val, NEG), axis=1)
        c3 = c2 & (val == b2[:, None])
        b3 = mx.min(mx.where(c3, tl, BIG), axis=1)
        c4 = c3 & (tl == b3[:, None])
        m = A.first_index_eq(c4.astype(I32), mx.ones((B,), dtype=I32))
        go = b1 < BIG
        hot = go[:, None] & (mx.arange(S)[None, :] == out_n[:, None])
        out_s = mx.where(hot, sh[:, None], out_s)
        out_t = mx.where(hot, m[:, None], out_t)
        out_n = out_n + go.astype(I32)
        left = mx.where(go[:, None] & (mx.arange(U)[None, :] == m[:, None]), rest, left)
    # City strikes first, then the units' shots (the advisor's two proposal lists), each in plan order.
    is_unit = out_s >= C
    k = mx.where(out_s < 0, 3 * S, mx.where(is_unit, S, 0)) + mx.arange(S)[None, :]
    o = mx.argsort(k, axis=1)
    out_s = mx.take_along_axis(out_s, o, axis=1)
    # Shooter rows past the cities are the listed slots: back to slot numbers (C + slot).
    slot_of = mx.array(list(range(C)) + [C + u for u in shooters_list], dtype=I32)
    out_s = mx.where(out_s >= 0, mx.take(slot_of, mx.maximum(out_s, 0)), -1)
    return out_s, mx.take_along_axis(out_t, o, axis=1), C


def run_shots(game, side=1, live=None):
    s, sim = game.s, game.sim
    B = s["u_alive"].shape[0]
    sd = mx.full((B,), side, dtype=I32)
    shooter, target, C = plan_shots(game, side)
    S = shooter.shape[1]
    live = live if live is not None else mx.ones((B,), dtype=mx.bool_)
    for j in range(S):
        sh, tg = shooter[:, j], target[:, j]
        any_ = live & (sh >= 0)
        if not bool(mx.any(any_).item()):
            break
        s = game.s
        tgt_tile = mx.where(tg >= 0, g1(s["u_tile"], mx.maximum(tg, 0)), -1)
        alive_t = (tg >= 0) & g1(s["u_alive"], mx.maximum(tg, 0)) & (tgt_tile >= 0)
        city = any_ & (sh < C) & alive_t
        if bool(mx.any(city).item()):
            cs = mx.where(city, sh, 0)
            ok = gn(sim.city_mask(s, sd, cs).astype(I32), mx.maximum(tgt_tile, 0)[:, None])[:, 0] > 0
            take = city & ok
            game.s = sim.city_strike(s, sd, cs, mx.maximum(tgt_tile, 0), take)
            game.counts["enemy city strikes"] += int(mx.sum(take).item())
        unit = any_ & (sh >= C) & alive_t
        if bool(mx.any(unit).item()):
            s = game.s
            me = mx.where(unit, sh - C, 0)
            mk = sim.masks(s, sd, me)
            game.s = sim.apply_unit(s, sd, me, mx.full((B,), V_ATTACK, dtype=I32), mx.maximum(tgt_tile, 0), mk, unit)
            game.counts["enemy shots"] += int(mx.sum(unit).item())


def charge(game, me, side=1):
    """`tactics::valued_adjacent_target` with cost 0: (has one, the target tile)."""
    s, sim, obs = game.s, game.sim, game.obs
    B, U = s["u_alive"].shape
    tile = g1(s["u_tile"], me)
    to = s["u_tile"]
    foe = s["u_alive"] & (to >= 0) & (s["u_side"] == 1 - side)
    adj = sim.geo.distance(mx.broadcast_to(tile[:, None], (B, U)), to) == 1
    dealt, taken, dhp, ok = obs.preview(s, me, mx.broadcast_to(tile[:, None], (B, U)), to, foe & adj, True)
    hp = g1(s["u_hp"], me)[:, None]
    civ = game.obs.kind(s, "civ")
    worth = worth_of(game)
    kill = dealt >= dhp
    val_m = mx.minimum(dealt, dhp) + mx.where(kill, worth, 0) - taken
    val = mx.where(civ, dhp, val_m)
    fine = ok & (civ | (taken < hp)) & (val > 0)
    # The target priority is not ported: the value alone, the lowest tile on a tie.
    v = mx.where(fine, val, NEG)
    best = mx.max(v, axis=1)
    c2 = fine & (v == best[:, None])
    bt = mx.min(mx.where(c2, to, 1 << 20), axis=1)
    return best > NEG, bt


def enemy_turn(game, charge_on=False):
    """The enemy's turn: the family script (field units), the advisor's shots, the units' own decisions, the end."""
    s, sim = game.s, game.sim
    B, U = game.B, game.U
    side = 1
    sd = mx.full((B,), side, dtype=I32)
    live = ~s["done"] & (game.verdict < 0)
    game.field_script(live)
    run_shots(game, side, live)
    if not charge_on:
        # Vectorised over the slots (the same as the slot loop below without the charge): the marches, then every
        # holder fortifies where `Sim.masks`'s fortify condition holds (`Fortify` costs no move and touches no other unit).
        s = game.s
        mine = live[:, None] & s["u_alive"] & (s["u_side"] == side) & (s["u_tile"] >= 0) & ~game.obs.kind(s, "civ") & ~game.field
        walker = mine & game.walker
        R = game.walks.shape[1]
        r = mx.clip(game.rel_now, 0, R - 1)
        to = mx.take_along_axis(game.walks, mx.broadcast_to(r[:, None, None], (B, 1, U)), axis=1)[:, 0]
        to = mx.where(game.has_walks, to, -1)
        mil, _ = sim.occupancy(s)
        free = gn(mil, mx.maximum(to, 0)) < 0
        go = walker & (to >= 0) & (to != s["u_tile"]) & free
        s["u_tile"] = mx.where(go, to, s["u_tile"])
        s["u_mv"] = mx.where(go, 0, s["u_mv"])
        holder = mine & ~walker
        water = sim.mg("water", s["u_tile"]) > 0
        fort = holder & ~((s["u_st"] == 1) | (s["u_st"] == 2)) & ~s["p_civilian"] & ~s["p_no_defensive_bonus"] & s["p_land"] & ~water
        s["u_st"] = mx.where(fort, 1, s["u_st"])
        game.s = sim.end_turn(s, sd, live)
        return
    for u in range(U):
        s = game.s
        me = mx.full((B,), u, dtype=I32)
        mine = live & g1(s["u_alive"], me) & (g1(s["u_side"], me) == side) & (g1(s["u_tile"], me) >= 0) & ~g1(game.obs.kind(s, "civ"), me) & ~g1(game.field, me)
        if not bool(mx.any(mine).item()):
            continue
        walker = mine & g1(game.walker, me)
        if bool(mx.any(walker).item()):
            to = game.walk_to(me)
            ok = walker & (to >= 0) & (to != g1(s["u_tile"], me))
            mil, civ = sim.occupancy(s)
            free = gn(mil, mx.maximum(to, 0)[:, None])[:, 0] < 0
            go = ok & free
            if bool(mx.any(go).item()):
                s["u_tile"] = put1(s["u_tile"], me, to, go)
                s["u_mv"] = put1(s["u_mv"], me, mx.zeros_like(me), go)
                game.s = s
        holder = mine & ~walker
        if charge_on and bool(mx.any(holder).item()):
            ranged = g1(game.obs.kind(s, "range"), me) >= 0
            ok, t = charge(game, me, side)
            hit = holder & ~ranged & ok
            if bool(mx.any(hit).item()):
                mk = sim.masks(game.s, sd, me)
                game.s = sim.apply_unit(game.s, sd, me, mx.full((B,), V_ATTACK, dtype=I32), t, mk, hit)
                game.counts["enemy charges"] += int(mx.sum(hit).item())
        s = game.s
        if bool(mx.any(holder).item()):
            mk = sim.masks(s, sd, me)
            game.s = sim.apply_unit(s, sd, me, mx.full((B,), V_FORT, dtype=I32), mx.zeros((B,), dtype=I32), mk, holder & mk["fort"])
    game.s = sim.end_turn(game.s, sd, live)
