"""The camp families of twin package 4 (plan 1.9), camp and camp-race, in MLX, batched over scenes. Rust:
`crates/qa/src/trainers/camp.rs` (`enemy_turn`, `neutral_turn`, `neutral_rule`, `enter`, `judge`, `mark`), the barbarians'
camp in `classic-rules/src/systems/barbarians.rs`, `act.rs` (`unit_turn`, `move_by`). The third seat's slots, the war table
and the loader are `armythird`.

The turn order of a play with a third seat (the dump's events): the learner (net, scene rule), the third seat (its script, then
its rule part), then the barbarians (the family script, the scene rule, the countdown and the spawn).

The camp (state in `s`, so that the compacted steps carry it): `camp_tile` (window tile, -1 none), `camp_up` (the improvement
stands), `camp_cd` (the countdown `spawn_in`), `camp_by` (the side that cleared it, -1 none), `lost_at_clear`. A camp is
cleared by whoever ENTERS its tile (a unit of a side that is not the barbarians: the learner's and the neutral's moves, a
melee kill that advances). The countdown falls by 1 at each barbarian turn; at 0 the camp spawns the barbarians' strongest
land unit on its tile when no barbarian military unit stands on it (else it tries again next turn). NOT in the twin: the
wandering spawn from round 10 on (it needs the fog-placed camps' draws), and the countdown after the first spawn (Rust draws
8 + 0..4 scaled by the speed: 5 to 8 at Quick, unobservable: the twin takes 6).
"""

import mlx.core as mx

from . import armyobs as A
from . import armyrule as R
from .armyrule import _lex_min
from .sim import g1, I32, V_ATTACK, V_FORT, V_MOVE, CL_MIL

NEAR_CAMP = 3
LOSS_CAP = {"camp": 1, "camp-race": 2}
AFTER_FIRST = 6
MAX_NEARBY, NEARBY_RANGE, EARLIEST_RELEASE = 2, 4, 10  # `systems/barbarians.rs`
ROUND_SHIFT = 0  # the world's round of a dump's round: the wandering spawn of camp/0/82/0 comes at our turn of label 10


def active(s):
    return "camp_tile" in s


def _rows(game, live):
    """[B] the rows of a camp scene that still play."""
    s = game.s
    return live & (s["camp_tile"] >= 0)


# ------------------------------------------------------------------------------------------------ the camp itself

def check(game):
    """After a unit's action: a unit of a side that is not the barbarians stands on the camp tile: the camp is cleared."""
    s = game.s
    if not active(s):
        return
    on = s["u_alive"] & (s["u_tile"] == s["camp_tile"][:, None]) & (s["camp_tile"] >= 0)[:, None] & (s["u_side"] != 1) & (s["u_side"] >= 0)
    hit = s["camp_up"] & mx.any(on, axis=1)
    if not bool(mx.any(hit).item()):
        return
    first = mx.argmax(on.astype(I32), axis=1)
    s["camp_by"] = mx.where(hit, g1(s["u_side"], first.astype(I32)), s["camp_by"])
    s["camp_up"] = s["camp_up"] & ~hit
    game.s = s


def mark(game, live=None):
    """`camp::mark`: the first time the camp is seen cleared, what ours had lost by then."""
    s = game.s
    if not active(s):
        return
    mine = s["u_alive"] & (s["u_side"] == 0) & (mx.arange(game.U)[None, :] < game.n_mine[:, None])
    lost = game.n_mine - mx.sum(mine, axis=1)
    new = (s["camp_by"] >= 0) & (s["lost_at_clear"] < 0)
    s["lost_at_clear"] = mx.where(new, lost, s["lost_at_clear"])
    game.s = s


def guarded(s):
    """[B] a barbarian unit stands on the camp tile."""
    return mx.any(s["u_alive"] & (s["u_side"] == 1) & (s["u_tile"] == s["camp_tile"][:, None]), axis=1)


def barbarian_end(game, live):
    """`barbarians::camp_spawns` at the barbarians' turn end: the countdown falls; at zero an empty camp spawns the strongest
    land unit on its tile; a guarded camp (a barbarian military unit on it), from round `EARLIEST_RELEASE` on and with at
    most `MAX_NEARBY` barbarians within `NEARBY_RANGE`, spawns it on the first free tile near (`Sim.free_tile_near`); else it
    tries again next turn. The sea spawn of a coastal camp (rng 1 in 6) is not in the twin."""
    s = game.s
    if not active(s):
        return
    rows = _rows(game, live) & s["camp_up"]
    if not bool(mx.any(rows).item()):
        return
    sim = game.sim
    B, U = game.B, game.U
    cd = mx.where(rows, s["camp_cd"] - 1, s["camp_cd"])
    zero = rows & (cd <= 0) & (s["camp_row"] >= 0)
    guard = guarded(s)
    rnd = game.start + game.rel_now + ROUND_SHIFT
    sd1 = s["u_alive"] & (s["u_side"] == 1) & (s["u_tile"] >= 0)
    near = mx.sum(sd1 & (sim.geo.distance(mx.maximum(s["u_tile"], 0), mx.broadcast_to(mx.maximum(s["camp_tile"], 0)[:, None], s["u_tile"].shape)) <= NEARBY_RANGE), axis=1)
    wander = zero & guard & (rnd >= EARLIEST_RELEASE) & (near <= MAX_NEARBY)
    due = zero & ~guard
    sp = mx.maximum(s["camp_row"], 0)
    mmax = sim.tg(sim.z["sp_moves_land"], sp)
    spot = sim.free_tile_near(s, mx.ones((B,), dtype=I32), s["camp_tile"], mx.full((B,), CL_MIL, dtype=I32), sim.tg(sim.z["sp_dest_id"], sp), wander,
                              land=sim.tg(sim.z["sp_land"], sp))
    spawn = due | wander
    where_tile = mx.where(wander, spot, s["camp_tile"])
    # Not stopped: a wandering spawn whose tile does not hold (every neighbour taken) is not made.
    ok_stop = sim.can_stop_at(s, mx.ones((B,), dtype=I32), mx.full((B, 1), CL_MIL, dtype=I32), where_tile[:, None])[:, 0]
    spawn = spawn & (ok_stop | due)
    s["camp_cd"] = mx.where(spawn, AFTER_FIRST, mx.where(zero, 1, cd))
    s, slot = sim.new_unit(s, mx.ones((B,), dtype=I32), where_tile, spawn, mx.full((B,), 100, dtype=I32), mmax, sim.tg(sim.z["sp_kind"], sp),
                           profile_from=mx.where(spawn, s["camp_row"], -1), spare=True)
    s = sim.set_unit(s, slot, spawn & (slot < s["u_alive"].shape[1]), mv=mmax)
    game.counts["camp spawns"] += int(mx.sum(due).item())
    game.counts["camp wandering spawns"] += int(mx.sum(wander & spawn).item())
    game.s = s


# ------------------------------------------------------------------------------------------------ the units' turns

def _dist(game, a, b):
    return game.sim.geo.distance(mx.maximum(a, 0), mx.maximum(b, 0))


def _targets_near(game, side_of_script):
    """[B, U] -> for each row, is any target of the barbarians (ours in the scene's pieces, the third's) within 3 of the camp."""


def _camp_move(game, me, side, take, goal, kind):
    """`move_by` with Hold(goal): (dist, -defence) or To(goal): (dist, threat of the learner's units) for `me` [B] in `take`."""
    if not bool(mx.any(take).item()):
        return
    sim, obs = game.sim, game.obs
    s = game.s
    B = game.B
    sd = mx.full((B,), side, dtype=I32)
    mk = sim.masks(s, sd, me)
    lt = mk["lt"]
    own = (mx.arange(lt.shape[1])[None, :] == 0) & (lt >= 0)
    opt = (mk["move_l"] | own) & take[:, None]
    d = sim.geo.distance(mx.maximum(lt, 0), mx.broadcast_to(mx.maximum(goal, 0)[:, None], lt.shape))
    if kind == "hold":
        k2 = -obs.defence(s, mx.maximum(lt, 0), side)
    else:
        k2 = R._threat(game, s, side, lt)
    j, has = _lex_min([d, k2, lt], opt)
    go = has & (j != 0)
    game.s = sim.apply_unit(s, sd, me, mx.full((B,), V_MOVE, dtype=I32), mx.take_along_axis(lt, j[:, None], axis=1)[:, 0], mk, go)


def unit_turn(game, me, side, rows, aim, goal):
    """`act::unit_turn` (Even) of slot `me` [B] in `rows` [B], `aim` [B] int: 0 Close (melee) / Support (ranged), 1 Hold(goal),
    2 To(goal). The blow from where it stands, else the approach and blow (not Hold), else the aim's move and a blow from
    there; a unit that holds and stands on its tile fortifies; then it stands."""
    sim = game.sim
    B, U = game.B, game.U
    sd = mx.full((B,), side, dtype=I32)
    s = game.s
    civ = g1(game.obs.kind(s, "civ"), me)
    mine = rows & g1(s["u_alive"], me) & (g1(s["u_side"], me) == side) & (g1(s["u_tile"], me) >= 0) & (g1(s["u_mv"], me) > 0)
    if not bool(mx.any(mine).item()):
        return
    hold = mine & (aim == 1)
    ok, t = R._blow(game, me, side, mine & ~civ)
    hit = mine & ~civ & ok
    if bool(mx.any(hit).item()):
        R._strike(game, me, side, t, hit)
        check(game)
    rest = mine & ~civ & ~hit & ~hold
    ap = mx.zeros((B,), dtype=mx.bool_)
    if bool(mx.any(rest).item()):
        ok2, frm, t2 = R._approach(game, me, side, rest)
        ap = rest & ok2
        if bool(mx.any(ap).item()):
            mk = sim.masks(game.s, sd, me)
            game.s = sim.apply_unit(game.s, sd, me, mx.full((B,), V_MOVE, dtype=I32), frm, mk, ap)
            check(game)
            R._strike(game, me, side, t2, ap)
            check(game)
    walk = mine & ~hit & ~ap
    s = game.s
    walk = walk & (g1(s["u_mv"], me) > 0)
    if bool(mx.any(walk).item()):
        close = walk & (aim == 0)
        if bool(mx.any(close).item()):
            go, tile = R._move_by_aim(game, me, side, close)
            if bool(mx.any(go).item()):
                mk = sim.masks(game.s, sd, me)
                game.s = sim.apply_unit(game.s, sd, me, mx.full((B,), V_MOVE, dtype=I32), tile, mk, go)
        _camp_move(game, me, side, walk & (aim == 1), goal, "hold")
        _camp_move(game, me, side, walk & (aim == 2), goal, "to")
        check(game)
        ok3, t3 = R._blow(game, me, side, walk & ~civ)
        h3 = walk & ~civ & ok3
        if bool(mx.any(h3).item()):
            R._strike(game, me, side, t3, h3)
            check(game)
    # A unit that holds its tile and still stands on it fortifies.
    s = game.s
    on = hold & g1(s["u_alive"], me) & (g1(s["u_tile"], me) == goal) & (g1(s["u_mv"], me) > 0)
    if bool(mx.any(on).item()):
        mk = sim.masks(s, sd, me)
        game.s = sim.apply_unit(s, sd, me, mx.full((B,), V_FORT, dtype=I32), mx.zeros((B,), dtype=I32), mk, on & mk["fort"])
    game.s = sim.hold_units(game.s, (mx.arange(U)[None, :] == me[:, None]) & mine[:, None], heal=game.hold_heals)
    mx.eval(game.s)


def enemy_turn(game, live, side=1):
    """`camp::enemy_turn`: the barbarians' units in the pieces' order (the scene's own, then what the camp spawned): the unit on
    the camp holds it, the others close in (or support) when one of ours or of the neutral's is within 3 of the camp, else go to the camp."""
    s = game.s
    if not active(s):
        return
    rows = _rows(game, live)
    if not bool(mx.any(rows).item()):
        return
    B, U = game.B, game.U
    slots = mx.arange(U)[None, :]
    camp = s["camp_tile"]
    game.script_threat = 0
    game.script_sorted = True  # `camp::enemy_turn`'s targets: tiles sorted and deduplicated
    for u in range(U):
        me = mx.full((B,), u, dtype=I32)
        s = game.s
        mine = rows & g1(s["u_alive"], me) & (g1(s["u_side"], me) == side) & (g1(s["u_tile"], me) >= 0) & (g1(s["u_mv"], me) > 0) \
            & (u >= game.n_mine)
        if not bool(mx.any(mine).item()):
            continue
        tgt = s["u_alive"] & (s["u_tile"] >= 0) & (((s["u_side"] == 0) & (slots < game.n_mine[:, None])) | (s["u_side"] == 2))
        near = mx.any(tgt & (_dist(game, s["u_tile"], mx.broadcast_to(camp[:, None], s["u_tile"].shape)) <= NEAR_CAMP), axis=1)
        ranged = g1(game.obs.kind(s, "range"), me) >= 0
        on = g1(s["u_tile"], me) == camp
        aim = mx.where(on, 1, mx.where(near, 0, 2))
        unit_turn(game, me, side, mine, aim, camp)
    game.script_threat = None
    game.script_sorted = False


def _can_enter(game, side_slots, camp):
    """[B, K] whether each third-seat slot can reach the empty camp this turn, and its distance to it."""
    s, sim = game.s, game.sim
    B = game.B
    flags, dists = [], []
    for u in side_slots:
        me = mx.full((B,), u, dtype=I32)
        alive = g1(s["u_alive"], me) & (g1(s["u_side"], me) == 2) & (g1(s["u_tile"], me) >= 0) & (g1(s["u_mv"], me) > 0)
        mk = sim.masks(s, mx.full((B,), 2, dtype=I32), me)
        reach = mx.any((mk["lt"] == camp[:, None]) & mk["move_l"], axis=1)
        flags.append(alive & reach)
        dists.append(_dist(game, g1(s["u_tile"], me), camp))
    return mx.stack(flags, axis=1), mx.stack(dists, axis=1)


def _enter(game, rows, third_slots):
    """`camp::enter` for the neutral: when the camp stands and no barbarian is on it, the nearest unit that can reach it (the
    lowest id on a tie) walks onto it and stands. [B] rows where one did."""
    s = game.s
    camp = s["camp_tile"]
    ready = rows & s["camp_up"] & ~guarded(s)
    if not bool(mx.any(ready).item()):
        return mx.zeros((game.B,), dtype=mx.bool_)
    flags, dists = _can_enter(game, third_slots, camp)
    key = mx.where(flags, dists * 64 + mx.arange(len(third_slots))[None, :], 1 << 20)
    best = mx.argmin(key, axis=1)
    did = ready & (mx.min(key, axis=1) < (1 << 20))
    sim = game.sim
    B = game.B
    for k, u in enumerate(third_slots):
        me = mx.full((B,), u, dtype=I32)
        pick = did & (best == k)
        if not bool(mx.any(pick).item()):
            continue
        mk = sim.masks(game.s, mx.full((B,), 2, dtype=I32), me)
        game.s = sim.apply_unit(game.s, mx.full((B,), 2, dtype=I32), me, mx.full((B,), V_MOVE, dtype=I32), camp, mk, pick)
        game.s = sim.hold_units(game.s, (mx.arange(game.U)[None, :] == me[:, None]) & pick[:, None], heal=game.hold_heals)
    check(game)
    return did


def neutral_turn(game, live):
    """`camp::neutral_turn` and `neutral_rule`: the third seat's pieces (ranged first, then melee, then by id): enter the empty
    camp at the first chance, else Support / Close at even odds on the barbarians (the unit on the camp alone while one stands
    there); then the scene rule's part (promotions, shots, stand and fortify)."""
    s = game.s
    if not active(s):
        return
    rows = _rows(game, live) & mx.array([f == "camp-race" for f in game.fam])
    if not bool(mx.any(rows).item()):
        return
    B, U = game.B, game.U
    third_slots = third_slot_list(game)
    rk = game.obs.kind(game.s, "range")
    game.script_only = game.s["camp_tile"]
    game.script_threat = 1
    game.script_sorted = True  # `act::foe_tiles`: sorted and deduplicated
    # Ranged first, then melee, each in slot order (= id order). The kinds are the template's: static per slot.
    ranged_of = lambda u: g1(rk, mx.full((B,), u, dtype=I32))
    order = []
    for pass_ranged in (True, False):
        for u in third_slots:
            order.append((u, pass_ranged))
    for u, pass_ranged in order:
        me = mx.full((B,), u, dtype=I32)
        s = game.s
        is_r = ranged_of(u) >= 0
        # Only the row's own pieces take a turn of the loop (a dead one too: its turn is the chance of an entry).
        first, count = third_span(game)
        sel = rows & (is_r == pass_ranged) & (u >= first) & (u < first + count)
        if not bool(mx.any(sel).item()):
            continue
        entered = _enter(game, sel, third_slots)
        go = sel & ~entered
        # (Rust: `continue` after an entry; the unit that entered is not the one acting, the rest of the order goes on.)
        aim = mx.where(is_r, 0, 0)
        unit_turn(game, me, 2, go, aim, s["camp_tile"])
    _enter(game, rows, third_slots)
    game.script_only = None
    game.script_threat = None
    game.script_sorted = False
    mark(game)


def third_slot_list(game):
    """The slots that hold the third seat's pieces: the same in every row of a batch of camp scenes (static layout)."""
    key = "_third_slots"
    v = getattr(game, key, None)
    if v is None:
        lo = hi = 0
        P = [sc["scene"].get("n_main") for sc, _, _ in game.plays if sc["scene"].get("third") is not None]
        ks = [(sc["scene"]["third_slot0"], len(sc["scene"]["third"]["ids"])) for sc, _, _ in game.plays if sc["scene"].get("third") is not None]
        top = max(a + b for a, b in ks)
        v = list(range(min(a for a, _ in ks), top))
        setattr(game, key, v)
    return v


def third_span(game):
    """[B], [B]: the first slot of each row's third pieces and how many (the plays of a batch have different layouts)."""
    v = getattr(game, "_third_span", None)
    if v is None:
        first = mx.array([sc["scene"].get("third_slot0", 0) for sc, _, _ in game.plays], dtype=I32)
        count = mx.array([len(sc["scene"]["third"]["ids"]) if sc["scene"].get("third") else 0 for sc, _, _ in game.plays], dtype=I32)
        v = game._third_span = (mx.take(first, game.idx), mx.take(count, game.idx))
    return v


def neutral_rule(game, live):
    rows = _rows(game, live) & mx.array([f == "camp-race" for f in game.fam])
    if bool(mx.any(rows).item()):
        R.seat_turn(game, 2, rows)


# ------------------------------------------------------------------------------------------------ the judge

def judge(game, rel, v):
    """`camp::judge` of the camp rows: the verdict [B] (1 won, 0 lost, -1 on) over `v`."""
    s = game.s
    if not active(s):
        return v
    is_camp = s["camp_tile"] >= 0
    if not bool(mx.any(is_camp).item()):
        return v
    mark(game)
    mine = s["u_alive"] & (s["u_side"] == 0) & (mx.arange(game.U)[None, :] < game.n_mine[:, None])
    n_alive = mx.sum(mine, axis=1)
    cleared = s["camp_by"] >= 0
    lost = mx.where(cleared & (s["lost_at_clear"] >= 0), s["lost_at_clear"], game.n_mine - n_alive)
    cap = mx.array([LOSS_CAP.get(f, 0) for f in game.fam], dtype=I32)
    broken = lost > cap
    by_neutral = s["camp_by"] == 2
    by_me = s["camp_by"] == 0
    out = mx.where(by_neutral | broken, 0, mx.where(by_me, 1, mx.where((n_alive == 0) | (rel >= game.rounds), 0, -1)))
    return mx.where(is_camp, out, v)
