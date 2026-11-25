"""The two fog families of twin package 3 (plan 13.2), ambush-fog and hunt-fog: the enemy's script and the win rules in
MLX, batched over scenes. Rust: `crates/qa/src/trainers/memory.rs` (`enemy_turn`, `judge`), `act.rs` (`move_by`,
`unit_turn`). The side's visibility in the observation is `armyobs.Obs.field` (the rules' sight: `Sim.seen_by`) and
the history's "into sight / gone" facts (`armyhist`) follow from it; nothing of that lives here.

The enemy (each piece of its seat in the pieces' order = the slot order):
- round 0 (rel 0): every piece stands;
- from rel 1, a piece that is not placed walks (`move_by` with the key (distance to its goal, tile)): the ambush stack
  to the hiding place, the worker to its first step at rel 1 and then to its forest. A piece is placed once a turn of
  rel >= 1 starts with it within 1 of the hiding place (the mark stays);
- a placed ambusher strikes (`unit_turn`, Close, Even) when one of ours stands within 3 of it on its route (2q + r <= 8
  and r x side >= 1, from the arena's centre), else it fortifies;
- every piece then stands (`act::stand`), the scene rule plays the rest of the seat.

The scene's private facts come from the dump's scene line (Rust `orders.info`, `orders.centre`; info only): the
side of the route (south = +1), the hiding place, the worker's first step, the arena's centre. Rust keeps the marks
"placed" in the scene's info; here they are `game.fog_placed` [B, U]. A game that starts at a later turn (the forced
checks of `armyenemy`) rebuilds them from the play's events.
"""

import mlx.core as mx

from . import armyobs as A
from .armyrule import _lex_min, close_unit
from .sim import g1, I32, V_FORT, V_MOVE

AMBUSH, HUNT = 0, 1
CODE = {"ambush-fog": AMBUSH, "hunt-fog": HUNT}
STRIKE_RANGE = 3
PLACED_RANGE = 1
REFUGE_RANGE = 2


def _window_tile(orders, tile):
    """A map tile id as the window's tile index (-1: none or outside the window)."""
    q0, r0, w, map_w = orders["window"]
    if tile is None or tile < 0:
        return -1
    x, y = tile % map_w - q0, tile // map_w - r0
    return y * w + x if 0 <= x < w and 0 <= y < w else -1


def _hex_distance(a, b, w):
    """Hex distance of two window tile indices (q = column, r = row)."""
    dq, dr = a % w - b % w, a // w - b // w
    return (abs(dq) + abs(dr) + abs(dq + dr)) // 2


def statics(game):
    """The per-row facts of the fog families [B, ...] (built once a game, at the full batch)."""
    st = getattr(game, "_fog", None)
    if st is not None and st["code"].shape[0] == game.B:
        return st
    U = game.U
    code, side, hide, step, cx, cy, n_pieces, ambush, civilian = [], [], [], [], [], [], [], [], []
    for sc, _, _ in game.plays:
        o = sc["orders"]
        fam = sc["play"].split("/")[0]
        info = o.get("info") or {}
        code.append(CODE.get(fam, -1))
        side.append(1 if info.get("side") == "south" else -1)
        hide.append(_window_tile(o, int(info["hide"])) if "hide" in info else -1)
        step.append(_window_tile(o, int(info["step"])) if "step" in info else -1)
        map_w = o["window"][3]
        centre = o.get("centre", 0)
        cx.append(centre % map_w)
        cy.append(centre // map_w)
        roles = list(o.get("roles", []))
        n_pieces.append(len(roles))
        roles = (roles + [""] * U)[:U]
        ambush.append([r == "ambush" for r in roles])
        civilian.append([r == "civilian" for r in roles])
    take = lambda v, dt=I32: mx.take(mx.array(v, dtype=dt), game.idx, axis=0)
    st = {"code": take(code), "side": take(side), "hide": take(hide), "step": take(step), "cx": take(cx), "cy": take(cy),
          "n_pieces": take(n_pieces), "ambush": take(ambush, mx.bool_), "civilian": take(civilian, mx.bool_)}
    st["any"] = any(c >= 0 for c in code)  # a python fact of the plays
    game._fog = st
    game.fog_placed = mx.zeros((game.B, U), dtype=mx.bool_)
    game.fog_started = False
    return st


def _placed_from_events(game, st):
    """Rows that start at rel >= 2 (the forced checks): the pieces that stood within 1 of the hiding place at the start
    of an enemy turn of rel >= 1 before this one, from the play's events (the state after our turn's end)."""
    rel = game.rel_now.tolist()
    idx = game.idx.tolist()
    hide = st["hide"].tolist()
    U = game.U
    out = [[False] * U for _ in range(game.B)]
    for b in range(game.B):
        if st["code"][b].item() < 0 or rel[b] < 2:
            continue
        sc = game.plays[idx[b]][0]
        o = sc["orders"]
        w = o["window"][2]
        units = [list(u) for u in sc["initial"]["units"]]
        for e in sc.get("events", []):
            for k, u in e["u"]:
                while len(units) <= k:
                    units.append([0, -1])
                units[k] = u
            if e["side"] != 0 or e["src"] != "end":
                continue
            turn = e["round"] - o["start_round"]
            if turn < 1 or turn >= rel[b]:
                continue
            for k, u in enumerate(units[:U]):
                if u[0] and u[8] == 1 and u[1] >= 0 and _hex_distance(u[1], hide[b], w) <= PLACED_RANGE:
                    out[b][k] = True
    return mx.array(out, dtype=mx.bool_)


def _distance_to(game, tiles, goal):
    """Hex distance of window tiles [B, N] to the window tile `goal` [B]."""
    return game.sim.geo.distance(mx.maximum(tiles, 0), mx.broadcast_to(mx.maximum(goal, 0)[:, None], tiles.shape))


def _walk(game, me, side, take, goal):
    """`act::move_by` with the key (distance to the goal, tile): to the reachable tile nearest to `goal` [B]; the own
    tile best: no move."""
    if not bool(mx.any(take).item()):
        return
    sim = game.sim
    sd = mx.full((game.B,), side, dtype=I32)
    mk = sim.masks(game.s, sd, me, rows=take)
    lt = mk["lt"]
    own = (mx.arange(lt.shape[1])[None, :] == 0) & (lt >= 0)
    options = (mk["move_l"] | own) & take[:, None]
    j, has = _lex_min([_distance_to(game, lt, goal), lt], options)
    go = has & (j != 0)
    game.s = sim.apply_unit(game.s, sd, me, mx.full((game.B,), V_MOVE, dtype=I32), mx.take_along_axis(lt, j[:, None], axis=1)[:, 0], mk, go)


def _fortify(game, me, side, take):
    if not bool(mx.any(take).item()):
        return
    sd = mx.full((game.B,), side, dtype=I32)
    mk = game.sim.masks(game.s, sd, me, rows=take)
    game.s = game.sim.apply_unit(game.s, sd, me, mx.full((game.B,), V_FORT, dtype=I32), mx.zeros((game.B,), dtype=I32), mk, take & mk["fort"])


def _ours_near(game, st, me, side):
    """[B]: one of our scene pieces stands within 3 of slot `me` on its route (2q + r <= 8, r x side >= 1 from the arena's
    centre)."""
    s, obs = game.s, game.obs
    slots = mx.arange(game.U)[None, :]
    ours = s["u_alive"] & (s["u_side"] == 1 - side) & (s["u_tile"] >= 0) & (slots < game.n_mine[:, None])
    q, r = obs.mapq(s["u_tile"])
    q, r = q - st["cx"][:, None], r - st["cy"][:, None]
    close = _distance_to(game, s["u_tile"], g1(s["u_tile"], me)) <= STRIKE_RANGE
    on_route = (2 * q + r <= 8) & (r * st["side"][:, None] >= 1)
    return mx.any(ours & close & on_route, axis=1)


def _mark_placed(game, st, rows):
    """A turn of rel >= 1 starts: the pieces within 1 of the hiding place are placed from now on."""
    s = game.s
    near_hide = _distance_to(game, s["u_tile"], st["hide"]) <= PLACED_RANGE
    mine = s["u_alive"] & (s["u_side"] == 1) & (s["u_tile"] >= 0)
    game.fog_placed = game.fog_placed | (rows[:, None] & (game.rel_now >= 1)[:, None] & mine & near_hide)


def _stand(game, me, side, rows):
    s = game.s
    alive = rows & g1(s["u_alive"], me) & (g1(s["u_side"], me) == side) & (g1(s["u_tile"], me) >= 0)
    game.s = game.sim.hold_units(s, (mx.arange(game.U)[None, :] == me[:, None]) & alive[:, None], heal=game.hold_heals)


def _piece_turn(game, st, me, rows, side):
    """`memory::enemy_turn` of the piece in slot `me` [B], in the rows where it is a live piece of the enemy's seat."""
    s = game.s
    rel = game.rel_now
    acting = rows & (rel >= 1)
    placed = g1(game.fog_placed, me)
    goal = mx.where((st["code"] == HUNT) & (rel == 1) & (st["step"] >= 0), st["step"], st["hide"])
    _walk(game, me, side, acting & ~placed, goal)
    ambusher = acting & placed & g1(st["ambush"], me)
    near = _ours_near(game, st, me, side)
    s = game.s
    alive = ambusher & g1(s["u_alive"], me) & (g1(s["u_side"], me) == side) & (g1(s["u_mv"], me) > 0)
    close_unit(game, me, side, alive & near)
    _fortify(game, me, side, ambusher & ~near)
    _stand(game, me, side, rows)


def enemy_turn(game, live, side=1):
    """The fog families' enemy turn (the other rows untouched)."""
    st = statics(game)
    if not st["any"]:
        return
    rows = live & (st["code"] >= 0)
    if not bool(mx.any(rows).item()):
        return
    if not game.fog_started:
        game.fog_started = True
        game.fog_placed = game.fog_placed | _placed_from_events(game, st)
    _mark_placed(game, st, rows)
    s = game.s
    slots = mx.arange(game.U)[None, :]
    piece = (slots >= game.n_mine[:, None]) & (slots < st["n_pieces"][:, None])
    for u in range(game.U):
        pu = rows & g1(piece, mx.full((game.B,), u, dtype=I32))
        if not bool(mx.any(pu).item()):
            continue
        me = mx.full((game.B,), u, dtype=I32)
        s = game.s
        alive = pu & g1(s["u_alive"], me) & (g1(s["u_side"], me) == side) & (g1(s["u_tile"], me) >= 0)
        if bool(mx.any(alive).item()):
            _piece_turn(game, st, me, alive, side)
            mx.eval(game.s)


def judge(game, rel, v):
    """`memory::judge` of the fog families' rows: the verdict [B] (1 won, 0 lost, -1 on) over `v`."""
    st = statics(game)
    if not st["any"]:
        return v
    s, obs = game.s, game.obs
    slots = mx.arange(game.U)[None, :]
    mine = s["u_alive"] & (s["u_side"] == 0) & (slots < game.n_mine[:, None])
    n_alive = mx.sum(mine, axis=1)
    lost = game.n_mine - n_alive
    limit = rel >= game.rounds
    q, r = obs.mapq(s["u_tile"])
    t = game.tab.t
    to_refuge = A.hexd(q - t["ref_q"][:, None], r - t["ref_r"][:, None])
    at_refuge = (n_alive > 0) & ~mx.any(mine & (to_refuge > REFUGE_RANGE), axis=1)
    ambush = mx.where(lost >= 2, 0, mx.where(at_refuge, 1, mx.where(limit, 0, -1)))
    caught = mx.any(st["civilian"] & ~(s["u_alive"] & (s["u_side"] == 1)), axis=1)
    hunt = mx.where(caught, 1, mx.where(limit | (n_alive == 0), 0, -1))
    v = mx.where(st["code"] == AMBUSH, ambush, v)
    return mx.where(st["code"] == HUNT, hunt, v)
