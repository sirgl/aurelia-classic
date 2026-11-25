"""Twin package 7 (plan step 1.6, rule version 2): the families with civilians, guard, convoy and snipe, in MLX, batched over
scenes. Rust: `trainers/families.rs` (`civilian_turn`, `civilians_turn`, `civilian_hunt`, the snipe branch of `enemy_turn`,
`judge`) and `trainers/act.rs` (`unit_turn`, `best_blow`, `approach_strike`, `move_by`, the aims' keys), ported step for step;
the per-unit body follows twin package 2's `armyfield.py` (`blow`, `approach`, `move_aim`, `unit_turn`).

Under rule version 2 the scene rule plays the learner's civilians after the net's turn (`civilians_turn`: each civilian that
has not moved walks Careful to the settler's site / the refuge, a settler on or next to its site founds a city; the founding
ends the scene, `Sim.apply_unit` V_FOUND), before the rule's steps 1-4 (`armyrule.seat_turn`). The enemy:
- guard, convoy: each raider in the pieces' order: capture an unguarded civilian of ours it reaches (an attack from where it
  stands, else a move to the lowest reachable tile next to it and the attack), else a Careful blow that kills or trades well,
  else a walk to the nearest civilian of ours (the key: no tile within 2 of a city of ours, distance, tile); every piece stands;
- snipe: the civilian walks `To(refuge)` Careful, the escort `Guard(the civilian)` Careful (`Close` without a civilian).
Judges: guard (all our civilians alive at the end of the round counted from the deadline, or the raiders dead: won; a civilian
lost: lost), convoy (a city founded: won; no civilian of ours or the limit: lost), snipe (an enemy civilian dead or captured:
won; the limit or none of ours: lost).
The scene facts come from the dump's scene line (`orders.roles`, `tile` = convoy's site, `refuge`, `window`).
"""

import mlx.core as mx

from . import armyobs as A
from .armyrule import _lex_min, _strike, _threat
from .sim import g1, gn, I32, V_FOUND, V_MOVE

GUARD, CONVOY, SNIPE = 0, 1, 2
CODE = {"guard": GUARD, "convoy": CONVOY, "snipe": SNIPE}
TO, GUARD_AIM, CLOSE = 0, 1, 2
FAR = 99
CITY_ATTACK_RANGE = 2


def _window_tile(orders, tile):
    q0, r0, w, map_w = orders["window"]
    if tile is None or tile < 0:
        return -1
    x, y = tile % map_w - q0, tile // map_w - r0
    return y * w + x if 0 <= x < w and 0 <= y < w else -1


def statics(game):
    """The per-row facts [B, ...] (built once a game, at the full batch)."""
    st = getattr(game, "_civ", None)
    if st is not None and st["code"].shape[0] == game.B:
        return st
    U = game.U
    code, n_pieces, site, refuge, roles = [], [], [], [], []
    for sc, _, _ in game.plays:
        o = sc["orders"]
        fam = sc["play"].split("/")[0]
        code.append(CODE.get(fam, -1))
        rl = list(o.get("roles", []))
        n_pieces.append(len(rl))
        roles.append((rl + [""] * U)[:U])
        site.append(_window_tile(o, int(o["tile"])) if fam == "convoy" else -1)
        refuge.append(_window_tile(o, int(o["refuge"])) if o.get("refuge", -1) is not None and int(o.get("refuge", -1)) >= 0 else -1)
    take = lambda v, dt=I32: mx.take(mx.array(v, dtype=dt), game.idx, axis=0)
    role = lambda name: take([[r == name for r in rl] for rl in roles], mx.bool_)
    st = {"code": take(code), "n_pieces": take(n_pieces), "site": take(site), "refuge": take(refuge),
          "civilian": role("civilian"), "raider": role("raider"), "escort": role("escort")}
    st["any"] = any(c >= 0 for c in code)  # python facts of the plays: building the statics reads nothing from the device
    st["slots_civ"] = [u for u in range(U) if any(rl[u] == "civilian" for rl in roles)]
    game._civ = st
    game.civ_founded = mx.zeros((game.B,), dtype=mx.bool_)
    return st


def _is(st, *names):
    m = mx.zeros(st["code"].shape, dtype=mx.bool_)
    for n in names:
        m = m | (st["code"] == CODE[n])
    return m


def _any(m):
    return bool(mx.any(m).item())


def _stand(game, me, which):
    alive = which & g1(game.s["u_alive"], me)
    game.s = game.sim.hold_units(game.s, (mx.arange(game.U)[None, :] == me[:, None]) & alive[:, None], heal=game.hold_heals)


def _move(game, me, side, tile, go):
    if _any(go):
        sd = mx.full((game.B,), side, dtype=I32)
        mk = game.sim.masks(game.s, sd, me, rows=go)
        game.s = game.sim.apply_unit(game.s, sd, me, mx.full((game.B,), V_MOVE, dtype=I32), tile, mk, go)


def _dist_tiles(game, a, b):
    shape = mx.broadcast_shapes(a.shape, b.shape)
    return game.sim.geo.distance(mx.broadcast_to(mx.maximum(a, 0), shape), mx.broadcast_to(mx.maximum(b, 0), shape))


# ---------------------------------------------------------------------------------------------- targets, blows, keys


def _targets(game, side):
    """`targets_of`: our units' tiles, one entry a tile (the first piece there): [B, U] tiles and validity."""
    s = game.s
    U = s["u_alive"].shape[1]
    slots = mx.arange(U)[None, :]
    own = (slots < game.n_mine[:, None]) if side == 1 else ((slots >= game.n_mine[:, None]) & (slots < _n_pieces(game)[:, None]))
    pc = s["u_alive"] & (s["u_tile"] >= 0) & (s["u_side"] == 1 - side) & own
    same = (s["u_tile"][:, :, None] == s["u_tile"][:, None, :]) & pc[:, None, :] & (slots[:, None, :] < slots[:, :, None])
    marks = pc & ~mx.any(same, axis=2)
    return mx.where(marks, s["u_tile"], -1), marks


def _n_pieces(game):
    return statics(game)["n_pieces"]


def _fine_careful(dealt, taken, dhp, hp):
    kills = dealt >= dhp
    return kills | ((dealt >= 2 * taken) & (taken < hp // 2)), kills


def blow(game, me, side, take):
    """`act::best_blow` Careful from where `me` stands: (has one, the target tile) [B]."""
    s, sim, obs = game.s, game.sim, game.obs
    B = me.shape[0]
    tiles, valid = _targets(game, side)
    valid = valid & take[:, None] & (g1(s["u_mv"], me) > 0)[:, None]
    K = tiles.shape[1]
    here = g1(s["u_tile"], me)
    dealt, taken, dhp, ok = obs.preview_packed(s, me, mx.broadcast_to(here[:, None], (B, K)), mx.maximum(tiles, 0), valid, True)
    hp = g1(s["u_hp"], me)[:, None]
    fine, kills = _fine_careful(dealt, taken, dhp, hp)
    fine = fine & ok & valid
    mil, _ = sim.occupancy(s)
    t0 = mx.maximum(tiles, 0)
    m_sl = gn(mil, t0)
    capturer = (m_sl >= 0) & (gn(obs.kind(s, "range"), mx.maximum(m_sl, 0)) < 0)
    city = sim.city_on(s, t0) >= 0 if s["c_owner"].shape[1] else mx.zeros(t0.shape, dtype=mx.bool_)
    score = mx.where(kills, 10000, 0) + (100 - mx.maximum(dhp - dealt, 0)) * 10 + mx.where(capturer, 50, 0) + mx.where(city, 20, 0) - taken
    j, has = _lex_min([-score, tiles], fine)
    return has, mx.take_along_axis(tiles, j[:, None], axis=1)[:, 0]


def approach(game, me, side, take):
    """`act::approach_strike` Careful: (has one, the tile to move to, the target tile) [B]."""
    s, sim, obs, geo = game.s, game.sim, game.obs, game.sim.geo
    B = me.shape[0]
    mk = sim.masks(s, mx.full((B,), side, dtype=I32), me, rows=take)
    lt, left, ok_l = mk["lt"], mk["left"], mk["move_l"]
    L = lt.shape[1]
    tiles, valid = _targets(game, side)
    K = tiles.shape[1]
    rng = g1(obs.kind(s, "range"), me)
    reach = mx.where(rng >= 0, rng, 1)
    d = geo.distance(mx.broadcast_to(mx.maximum(lt, 0)[:, :, None], (B, L, K)), mx.broadcast_to(mx.maximum(tiles, 0)[:, None, :], (B, L, K)))
    pair = (ok_l & (left > 0) & take[:, None])[:, :, None] & valid[:, None, :] & (d <= reach[:, None, None])
    flat = pair.reshape(B, L * K)
    n = int(mx.max(mx.sum(flat, axis=1)).item())
    z = mx.zeros((B,), dtype=I32)
    if n == 0:
        return mx.zeros((B,), dtype=mx.bool_), z, z
    idx = mx.argsort(mx.where(flat, 0, 1), axis=1)[:, :n].astype(I32)
    ok_p = mx.take_along_axis(flat, idx, axis=1)
    frm = mx.take_along_axis(lt, idx // K, axis=1)
    to = mx.take_along_axis(tiles, idx % K, axis=1)
    dealt, taken, dhp, ok = obs.preview_packed(s, me, mx.maximum(frm, 0), mx.maximum(to, 0), ok_p, False)
    hp = g1(s["u_hp"], me)[:, None]
    fine, kills = _fine_careful(dealt, taken, dhp, hp)
    fine = fine & ok & ok_p
    score = mx.where(kills, 10000, 0) + (100 - mx.maximum(dhp - dealt, 0)) * 10 - taken
    j, has = _lex_min([_threat(game, s, side, frm), -score, frm, to], fine)
    pick = lambda a: mx.take_along_axis(a, j[:, None], axis=1)[:, 0]
    return has, pick(frm), pick(to)


def move_aim(game, me, side, take, aim, aim_tile):
    """`act::move_by` with the aim's key (To: (distance to the aim, threat); Guard: (distance to the ward, -min(nearest foe, 3),
    -defence); Close: (melee: max(nearest - 1, 0), ranged: |nearest - reach|; -defence)), the tile last: (moves, the tile) [B]."""
    s, sim, obs = game.s, game.sim, game.obs
    B = me.shape[0]
    mk = sim.masks(s, mx.full((B,), side, dtype=I32), me, rows=take)
    lt, ok_l = mk["lt"], mk["move_l"]
    L = lt.shape[1]
    own = (mx.arange(L)[None, :] == 0) & (lt >= 0)
    opt = (ok_l | own) & take[:, None]
    q, r = obs.mapq(lt)
    tiles, valid = _targets(game, side)
    tq, tr = obs.mapq(tiles)
    d = A.min_or(valid[:, None, :], A.hexd(q[:, :, None] - tq[:, None, :], r[:, :, None] - tr[:, None, :]), 2, FAR)
    aq, ar = obs.mapq(mx.maximum(aim_tile, 0)[:, None])
    dt = A.hexd(q - aq, r - ar)
    rng = g1(obs.kind(s, "range"), me)
    reach = mx.where(rng >= 0, rng, 1)[:, None]
    dfn = obs.defence(s, mx.maximum(lt, 0), side)
    thr = _threat(game, s, side, lt)
    close0 = mx.where((rng >= 0)[:, None], mx.abs(d - reach), mx.maximum(d - 1, 0))
    a = aim[:, None]
    k0 = mx.where(a == CLOSE, close0, dt)
    k1 = mx.where(a == TO, thr, mx.where(a == GUARD_AIM, -mx.minimum(d, 3), -dfn))
    k2 = mx.where(a == GUARD_AIM, -dfn, mx.zeros_like(lt))
    j, has = _lex_min([k0, k1, k2, lt], opt)
    go = has & (j != 0)
    return go, mx.take_along_axis(lt, j[:, None], axis=1)[:, 0]


def unit_turn(game, me, side, take, aim, aim_tile):
    """`act::unit_turn` Careful of the unit `me` [B] where `take` (a civilian: the aim's move only)."""
    s, B = game.s, game.B
    mine = take & g1(s["u_alive"], me) & (g1(s["u_side"], me) == side) & (g1(s["u_tile"], me) >= 0) & (g1(s["u_mv"], me) > 0)
    if not _any(mine):
        return
    civ = g1(game.obs.kind(s, "civ"), me)
    ok, t = blow(game, me, side, mine & ~civ)
    made = mine & ~civ & ok
    if _any(made):
        _strike(game, me, side, t, made)
    ap_rows = mine & ~civ & ~made
    if _any(ap_rows):
        ok2, frm, t2 = approach(game, me, side, ap_rows)
        ap = ap_rows & ok2
        if _any(ap):
            _move(game, me, side, frm, ap)
            _strike(game, me, side, t2, ap)
            made = made | ap
    s = game.s
    alive = mine & g1(s["u_alive"], me) & (g1(s["u_side"], me) == side)
    cont = alive & (g1(s["u_mv"], me) > 0) & ~made
    if _any(cont):
        go, tile = move_aim(game, me, side, cont, aim, aim_tile)
        _move(game, me, side, tile, go)
        ok3, t3 = blow(game, me, side, cont & ~civ)
        h3 = cont & ~civ & ok3
        if _any(h3):
            _strike(game, me, side, t3, h3)
    _stand(game, me, alive)


# ---------------------------------------------------------------------------------------------- the learner's civilians


def learner_pass(game, live):
    """Scene rule version 2, step 0b: the learner's civilians that have not moved play the civilian script (slot order)."""
    st = statics(game)
    if not st["any"]:
        return
    s, sim = game.s, game.sim
    B = game.B
    rows = live & (_is(st, "guard", "convoy"))
    if not _any(rows):
        return
    for u in st["slots_civ"]:
        s = game.s
        me = mx.full((B,), u, dtype=I32)
        can = rows & g1(st["civilian"], me) & g1(s["u_alive"], me) & (g1(s["u_side"], me) == 0) & (g1(s["u_tile"], me) >= 0) \
            & (g1(s["u_mv"], me) > 0) & (g1(s["u_mv"], me) >= g1(s["u_mmax"], me))
        if not _any(can):
            continue
        tile = g1(s["u_tile"], me)
        site = st["site"]
        conv = can & (st["code"] == CONVOY) & (site >= 0)
        sd = mx.zeros((B,), dtype=I32)
        mk = sim.masks(s, sd, me, rows=conv)
        near = conv & ((tile == site) | (mk["found"] & (_dist_tiles(game, tile, site) <= 1)))
        if _any(near):
            game.s = sim.apply_unit(s, sd, me, mx.full((B,), V_FOUND, dtype=I32), mx.zeros((B,), dtype=I32), mk, near)
            game.civ_founded = game.civ_founded | (near & game.s["done"])
        walk = can & ~near
        goal = mx.where((st["code"] == CONVOY) & (site >= 0), site, st["refuge"])
        goal = mx.where(goal >= 0, goal, mx.zeros_like(goal))
        unit_turn(game, me, 0, walk, mx.full((B,), TO, dtype=I32), goal)
        mx.eval(game.s)


# ---------------------------------------------------------------------------------------------- the enemy


def _our_civ_tiles(game, st):
    """[B, U]: our civilians (pieces) alive, with their tiles (-1 otherwise)."""
    s = game.s
    alive = st["civilian"] & s["u_alive"] & (s["u_side"] == 0) & (s["u_tile"] >= 0)
    return alive, mx.where(alive, s["u_tile"], -1)


def hunt_turn(game, live):
    """`families::civilian_hunt` of every raider of guard and convoy (pieces' order)."""
    st = statics(game)
    rows = live & _is(st, "guard", "convoy")
    if not _any(rows):
        return
    B, U = game.B, game.U
    sim, obs = game.sim, game.obs
    raiders = [u for u in range(U) if bool(mx.any(st["raider"][:, u] & _is(st, "guard", "convoy")).item())]
    for u in raiders:
        s = game.s
        me = mx.full((B,), u, dtype=I32)
        mine = rows & g1(st["raider"], me) & g1(s["u_alive"], me) & (g1(s["u_side"], me) == 1) & (g1(s["u_tile"], me) >= 0)
        if not _any(mine):
            continue
        acted = mx.zeros((B,), dtype=mx.bool_)
        for k in st["slots_civ"]:
            s = game.s
            ck = mx.full((B,), k, dtype=I32)
            ct = g1(s["u_tile"], ck)
            alive = g1(s["u_alive"], ck) & (g1(s["u_side"], ck) == 0) & (ct >= 0) & g1(st["civilian"], ck)
            guard = mx.any(s["u_alive"] & (s["u_side"] == 0) & (s["u_tile"] == ct[:, None]) & ~obs.kind(s, "civ"), axis=1)
            cand = mine & ~acted & alive & ~guard & (g1(s["u_mv"], me) > 0) & g1(s["u_alive"], me)
            if not _any(cand):
                continue
            here = g1(s["u_tile"], me)
            adj = cand & (_dist_tiles(game, here, ct) == 1)
            if _any(adj):
                _strike(game, me, 1, ct, adj)
                acted = acted | adj
            rest = cand & ~adj
            if _any(rest):
                mk = sim.masks(game.s, mx.full((B,), 1, dtype=I32), me, rows=rest)
                lt, left, ok_l = mk["lt"], mk["left"], mk["move_l"]
                nb = ok_l & (left > 0) & (_dist_tiles(game, lt, ct[:, None]) == 1) & rest[:, None]
                j, has = _lex_min([lt], nb)
                go = rest & has
                if _any(go):
                    _move(game, me, 1, mx.take_along_axis(lt, j[:, None], axis=1)[:, 0], go)
                    still = go & g1(game.s["u_alive"], me) & (g1(game.s["u_mv"], me) > 0)
                    _strike(game, me, 1, ct, still)
                    acted = acted | go
        s = game.s
        rest = mine & ~acted & g1(s["u_alive"], me)
        if _any(rest):
            ok, t = blow(game, me, 1, rest)
            hit = rest & ok
            if _any(hit):
                _strike(game, me, 1, t, hit)
            acted = acted | hit
            walk = mine & ~acted & g1(game.s["u_alive"], me)
            if _any(walk):
                s = game.s
                calive, ctile = _our_civ_tiles(game, st)
                here = g1(s["u_tile"], me)
                dgoal = mx.where(calive, _dist_tiles(game, mx.broadcast_to(here[:, None], ctile.shape), ctile), FAR)
                best = mx.min(dgoal, axis=1)
                key2 = mx.where(calive & (dgoal == best[:, None]), ctile, 1 << 20)
                goal = mx.min(key2, axis=1)
                has_goal = walk & mx.any(calive, axis=1)
                mk = sim.masks(s, mx.full((B,), 1, dtype=I32), me, rows=has_goal)
                lt, ok_l = mk["lt"], mk["move_l"]
                L = lt.shape[1]
                own = (mx.arange(L)[None, :] == 0) & (lt >= 0)
                opt = (ok_l | own) & has_goal[:, None]
                if s["c_owner"].shape[1]:
                    ctl = mx.where(sim.cs["alive"] & (s["c_owner"] == 0) & (sim.cs["tile"] >= 0), sim.cs["tile"], -1)
                    dc = game.sim.geo.distance(mx.broadcast_to(mx.maximum(lt, 0)[:, :, None], lt.shape + (ctl.shape[1],)), mx.broadcast_to(mx.maximum(ctl, 0)[:, None, :], lt.shape + (ctl.shape[1],)))
                    near_city = mx.any((ctl[:, None, :] >= 0) & (dc <= CITY_ATTACK_RANGE), axis=2).astype(I32)
                else:
                    near_city = mx.zeros_like(lt)
                dg = _dist_tiles(game, lt, goal[:, None])
                j, has = _lex_min([near_city, dg, lt], opt)
                go = has & (j != 0)
                _move(game, me, 1, mx.take_along_axis(lt, j[:, None], axis=1)[:, 0], go)
        _stand(game, me, mine)
        mx.eval(game.s)


def snipe_turn(game, live):
    """The snipe branch of `families::enemy_turn`: the civilian walks To(refuge), the escort Guards it, Careful."""
    st = statics(game)
    rows = live & _is(st, "snipe")
    if not _any(rows):
        return
    B, U = game.B, game.U
    for u in range(U):
        s = game.s
        me = mx.full((B,), u, dtype=I32)
        piece = rows & (u >= game.n_mine) & (u < st["n_pieces"]) & g1(s["u_alive"], me) & (g1(s["u_side"], me) == 1)
        if not _any(piece):
            continue
        # The ward: the first enemy civilian piece alive when the turn starts (the pieces' order).
        wd = st["civilian"] & s["u_alive"] & (s["u_side"] == 1) & (mx.arange(U)[None, :] >= game.n_mine[:, None])
        ward = mx.argmax(wd.astype(I32), axis=1).astype(I32)
        has_w = mx.any(wd, axis=1)
        wt = g1(s["u_tile"], ward)
        is_civ = g1(st["civilian"], me)
        aim = mx.where(is_civ, TO, mx.where(has_w, GUARD_AIM, CLOSE)).astype(I32)
        tile = mx.where(is_civ, st["refuge"], wt)
        unit_turn(game, me, 1, piece, aim, mx.where(tile >= 0, tile, mx.zeros_like(tile)))
        mx.eval(game.s)


def enemy_turn(game, live):
    """The families' enemy script (hooked after the other scripts, before the rule's steps)."""
    st = statics(game)
    if not st["any"]:
        return
    hunt_turn(game, live)
    snipe_turn(game, live)


# ---------------------------------------------------------------------------------------------- the judges


def judge(game, rel, v):
    """`families::judge` of guard, convoy, snipe: the verdict [B] (1 won, 0 lost, -1 on) over `v`."""
    st = statics(game)
    if not st["any"]:
        return v
    s = game.s
    U = game.U
    slots = mx.arange(U)[None, :]
    limit = rel >= game.rounds
    mine_p = (slots < game.n_mine[:, None])
    theirs_p = (slots >= game.n_mine[:, None]) & (slots < st["n_pieces"][:, None])
    civ_mine = st["civilian"] & mine_p
    civ_alive = mx.any(civ_mine & s["u_alive"] & (s["u_side"] == 0), axis=1)
    all_civ_alive = ~mx.any(civ_mine & ~(s["u_alive"] & (s["u_side"] == 0)), axis=1)
    combat_theirs = mx.sum(theirs_p & ~st["civilian"] & s["u_alive"] & (s["u_side"] == 1), axis=1)
    W, L, N = 1, 0, -1
    g = st["code"] == GUARD
    v = mx.where(g, mx.where(~all_civ_alive, L, mx.where((combat_theirs == 0) | limit, W, N)), v)
    c = st["code"] == CONVOY
    v = mx.where(c, mx.where(game.civ_founded, W, mx.where(~civ_alive | limit, L, N)), v)
    sn = st["code"] == SNIPE
    caught = mx.any(st["civilian"] & theirs_p & ~(s["u_alive"] & (s["u_side"] == 1)), axis=1)
    mine_n = mx.sum(mine_p & s["u_alive"] & (s["u_side"] == 0), axis=1)
    v = mx.where(sn, mx.where(caught, W, mx.where(limit | (mine_n == 0), L, N)), v)
    return v
