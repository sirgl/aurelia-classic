"""The third-party families of twin package 8 (plan 1.9) in MLX, batched over scenes: neutral-path, ally-siege, defend-friend,
closed-borders, spare-friends, neutral-joins-war, capital-race. Rust: `crates/qa/src/trainers/third_party.rs` (`keep_world`,
`third_turn`, `walk_turn`, `race_turn`, `third_rule`, `enemy_turn`, `mark`, `judge`), `act.rs` (`unit_turn`, `move_by`).
The third seat's slots, the war table and the loader are `armythird`; the camp families' scripts `armycamp` (`unit_turn`).

The turn order of a play (the dump's events): the learner (net, scene rule), the third seat (its script, then its rule part),
then the enemy (its script, the scene rule). The relations are the scene's given (`keep_world` every loop iteration):
- the learner and the enemy at war; the third at peace with the learner, and at war with the enemy where the family has an
  ally (ally-siege, defend-friend, capital-race, spare-friends: `armythird.ALLY`), else at peace with the enemy;
- neutral-joins-war: from the learner's turn of relative round `JOIN_AT` on, the third is at war with the learner too.

State kept in `s` (the compacted steps carry it): `pty_broken` (the loss cap broke, sticky: `cap_broken`), `pty_by`
(the side that took the target city first, -1 none: `taken_by`), `pty_lost` (what ours had lost by then).
"""

import mlx.core as mx

from . import armycamp
from . import armyobs as A
from . import armyrule as R
from .armyrule import _lex_min
from .armythird import ALLY, THIRD
from .sim import g1, I32, V_MOVE

JOIN_AT = 3  # neutral-joins-war: the round (relative) from which the neutral is at war with the learner
SHARE_WAIT = 3
LOSS_CAP = {"neutral-path": 2, "ally-siege": 1, "spare-friends": 1, "neutral-joins-war": 2, "defend-friend": 3, "closed-borders": 2,
            "capital-race": 5}
PILLAGE_K = 3
CAPTURE = ("ally-siege", "capital-race", "closed-borders")
NO_ARM = 1 << 20


def is_party(game):
    """[B] the rows of a third-party family (static per game)."""
    v = getattr(game, "_party_rows", None)
    if v is None:
        v = game._party_rows = mx.array([f in THIRD for f in game.fam])
    return v


def fam_rows(game, *names):
    """[B] the rows whose family is one of `names` (cached per set)."""
    cache = game.__dict__.setdefault("_fam_rows", {})
    if names not in cache:
        cache[names] = mx.array([f in names for f in game.fam])
    return cache[names]


def active(s):
    return "pty_by" in s


def has_family(game, names=THIRD):
    """Whether any row of the game is one of the families `names`: a python fact of the plays (`game.fam`), so a pass skips a
    game without the family before any device read (a host sync costs ~108 us, and a game of one family group has no other)."""
    cache = game.__dict__.setdefault("_has_family", {})
    key = tuple(names)
    if key not in cache:
        cache[key] = any(f in names for f in game.fam)
    return cache[key]


# ------------------------------------------------------------------------------------------------ the relations

FLIP = mx.array([[[False, False, True], [False, False, False], [True, False, False]]])  # the pair learner (0) / third (2)


def keep_world(game, rel):
    """`third_party::keep_world` at the start of the learner's turn of relative round `rel`: the third declares war on the
    learner (neutral-joins-war); the other relations hold from the start."""
    s = game.s
    if "war" not in s or rel < JOIN_AT:
        return
    rows = fam_rows(game, "neutral-joins-war")
    s["war"] = s["war"] | (rows[:, None, None] & FLIP)
    game.s = s


def setup(s, rows):
    """The sticky state of the rows [(scene line, moment)]."""
    B = len(rows)
    s["third_last"] = mx.array([sc["play"].split("/")[0] in THIRD for sc, _ in rows])
    s["pty_rows"] = mx.array([sc["play"].split("/")[0] in THIRD for sc, _ in rows])
    s["pty_broken"] = mx.zeros((B,), dtype=mx.bool_)
    s["pty_by"] = mx.full((B,), -1, dtype=I32)
    s["pty_lost"] = mx.full((B,), -1, dtype=I32)
    return s


# ------------------------------------------------------------------------------------------------ a move by a key

def _move_by(game, me, side, take, keys_of):
    """`act::move_by` of slot `me` [B] in `take`: the reachable tile with the least key (`keys_of(lt, mk)` -> a list of [B, L]
    int arrays; the tile id breaks the tie; the own tile counts and means no move)."""
    if not bool(mx.any(take).item()):
        return
    sim = game.sim
    s = game.s
    B = game.B
    sd = mx.full((B,), side, dtype=I32)
    mk = sim.masks(s, sd, me)
    lt = mk["lt"]
    own = (mx.arange(lt.shape[1])[None, :] == 0) & (lt >= 0)
    opt = (mk["move_l"] | own) & take[:, None]
    j, has = _lex_min(keys_of(lt, mk) + [lt], opt)
    go = has & (j != 0)
    game.s = sim.apply_unit(s, sd, me, mx.full((B,), V_MOVE, dtype=I32), mx.take_along_axis(lt, j[:, None], axis=1)[:, 0], mk, go)
    mx.eval(game.s)


def _hexd_to(game, lt, wq, wr):
    """The distance from the window tiles `lt` [B, L] to the map tiles (axial wq, wr [B])."""
    q, r = game.obs.mapq(lt)
    return A.hexd(q - wq[:, None], r - wr[:, None])


# ------------------------------------------------------------------------------------------------ the third seat

def _third_rows(game, live):
    s = game.s
    first, count = armycamp.third_span(game)
    return live & is_party(game), first, count


def _slots(game, live):
    """The pieces of the third seat of each row in turn: (the loop's slot index i, the slot [B], the rows where it is one)."""
    rows, first, count = _third_rows(game, live)
    top = int(mx.max(count).item()) if bool(mx.any(rows).item()) else 0
    for i in range(top):
        me = first + i
        s = game.s
        mine = rows & (i < count) & g1(s["u_alive"], me) & (g1(s["u_side"], me) == 2) & (g1(s["u_tile"], me) >= 0) & (g1(s["u_mv"], me) > 0)
        yield i, me, mine


def _waypoints(game):
    """neutral-path's walkers' waypoints: (q, r) up and down, per lane, [B] each, and whether the map holds them."""
    v = getattr(game, "_waypoints", None)
    if v is None:
        cq, cr, ok = [], [], []
        for sc, _, _ in game.plays:
            w = sc["scene"]["window"]
            # A mixed batch holds scenes of other families (and older dumps without `centre`): their rows are masked out.
            c = sc["orders"].get("centre", 0)
            cq.append(c % w["map_w"])
            cr.append(c // w["map_w"])
            ok.append((w["map_w"], w["map_h"]))
        v = game._waypoints = (mx.take(mx.array(cq, dtype=I32), game.idx), mx.take(mx.array(cr, dtype=I32), game.idx),
                               mx.take(mx.array([a for a, _ in ok], dtype=I32), game.idx), mx.take(mx.array([b for _, b in ok], dtype=I32), game.idx))
    return v


def walk_turn(game, live):
    """`walk_turn` (neutral-path): the walker `i` goes to and fro between the points three tiles either side of the
    centre on its lane (-2, 0, 1), switching every second round: `move_by` (distance to the point, tile)."""
    rows = live & fam_rows(game, "neutral-path")
    if not bool(mx.any(rows).item()):
        return
    cq, cr, mw, mh = _waypoints(game)
    for i, me, mine in _slots(game, rows):
        mine = mine & rows
        if not bool(mx.any(mine).item()):
            continue
        lane = (-2, 0, 1)[i % 3]
        up = ((game.rel_now // 2 + i) % 2) == 0
        wq = cq + lane
        wr = cr + mx.where(up, 3, -3)
        inside = (wq >= 0) & (wr >= 0) & (wq < mw) & (wr < mh)
        _move_by(game, me, 2, mine & inside, lambda lt, mk: [_hexd_to(game, lt, wq, wr)])


def _script(game, threat, city=None, near=None, nerve="even", only_city=False):
    """The knobs of the camp / party scripts (`armyrule._cols`, `_only`, `_threat`, `_nerve`) for the next calls."""
    game.script_threat = threat
    game.script_sorted = True  # the targets are `foe_tiles`-sorted
    game.script_city = city
    game.script_near = near
    game.script_nerve = nerve
    game.script_city_only = only_city


def _script_off(game):
    _script(game, None)
    game.script_sorted = False


def _stand(game, me, rows):
    """`act::stand` of slot `me` in `rows`: its turn is over (the moves heal an unmoved unit)."""
    game.s = game.sim.hold_units(game.s, (mx.arange(game.U)[None, :] == me[:, None]) & rows[:, None], heal=game.hold_heals)


def hold_turn(game, live):
    """defend-friend's third: each piece strikes what comes next to it at even odds or better, else holds (fortifies)."""
    rows = live & fam_rows(game, "defend-friend")
    if not bool(mx.any(rows).item()):
        return
    _script(game, 1)
    for _, me, mine in _slots(game, rows):
        armycamp.unit_turn(game, me, 2, mine & rows, mx.ones((game.B,), dtype=I32), g1(game.s["u_tile"], me))
    _script_off(game)


def hunt_turn(game, live):
    """neutral-joins-war's third after its declaration: a hunter (even odds or better; a shooter behind a melee unit)."""
    rows = live & fam_rows(game, "neutral-joins-war") & game.s["war"][:, 2, 0]
    if not bool(mx.any(rows).item()):
        return
    _script(game, 0)
    for _, me, mine in _slots(game, rows):
        armycamp.unit_turn(game, me, 2, mine & rows, mx.zeros((game.B,), dtype=I32), g1(game.s["u_tile"], me))
    _script_off(game)


def _city_owner(game, city):
    """[B] the owner (side) of the city on the window tile `city` [B], -1 none."""
    return R._city_owner_at(game, game.s, city)


def _reach_kills(game, me, side, city):
    """`reach_blow(city)`'s `dealt >= hp`: what `me` deals to the city from where it stands (in reach) or from a tile it can
    reach with a move left to strike, against the defender's hit points."""
    s, sim, obs = game.s, game.sim, game.obs
    B = game.B
    mk = sim.masks(s, mx.full((B,), side, dtype=I32), me)
    lt, left, ok_l = mk["lt"], mk["left"], mk["move_l"]
    tile = g1(s["u_tile"], me)
    rng = g1(obs.kind(s, "range"), me)
    reach = mx.where(rng >= 0, rng, 1)
    geo = sim.geo
    to = mx.broadcast_to(mx.maximum(city, 0)[:, None], lt.shape)
    own_ok = (geo.distance(tile, city) <= reach)[:, None]
    d1, _, h1, ok1 = obs.preview(s, me, tile[:, None], mx.maximum(city, 0)[:, None], own_ok, True)
    mv_ok = ok_l & (left > 0) & (geo.distance(lt, to) <= reach[:, None])
    d2, _, h2, ok2 = obs.preview(s, me, lt, to, mv_ok, False)
    best = mx.maximum(mx.max(mx.where(ok1 & own_ok, d1, -1), axis=1), mx.max(mx.where(ok2 & mv_ok, d2, -1), axis=1))
    hp = mx.maximum(mx.max(mx.where(ok1 & own_ok, h1, 0), axis=1), mx.max(mx.where(ok2 & mv_ok, h2, 0), axis=1))
    return (best >= 0) & (best >= hp)


def race_melee(game, me, sel, city):
    """`race_turn`'s melee unit: the capture (a blow that takes the city, struck or approached), else a kill near the city, else
    two tiles off the city; then it stands."""
    B = game.B
    cap = sel & (_city_owner(game, city) == 1) & _reach_kills(game, me, 2, city)
    done = mx.zeros((B,), dtype=mx.bool_)
    if bool(mx.any(cap).item()):
        _script(game, 1, city=city, nerve="reckless", only_city=True)
        ok, t = R._blow(game, me, 2, cap)
        hit = cap & ok
        R._strike(game, me, 2, t, hit)
        armycamp.check(game)
        rest = cap & ~hit
        ok2, frm, t2 = R._approach(game, me, 2, rest)
        ap = rest & ok2
        if bool(mx.any(ap).item()):
            mk = game.sim.masks(game.s, mx.full((B,), 2, dtype=I32), me)
            game.s = game.sim.apply_unit(game.s, mx.full((B,), 2, dtype=I32), me, mx.full((B,), V_MOVE, dtype=I32), frm, mk, ap)
            armycamp.check(game)
            R._strike(game, me, 2, t2, ap)
            armycamp.check(game)
        done = hit | ap
        _stand(game, me, done)
    kill = sel & ~done
    _script(game, 1, nerve="careful")
    ok, t = R._blow(game, me, 2, kill)
    hit = kill & ok & game.blow_kill
    R._strike(game, me, 2, t, hit)
    armycamp.check(game)
    _stand(game, me, hit)
    walk = sel & ~done & ~hit
    _script(game, 1, near=None)
    d = lambda lt, mk: [mx.abs(game.sim.geo.distance(lt, mx.broadcast_to(mx.maximum(city, 0)[:, None], lt.shape)) - 2), R._threat(game, game.s, 2, lt)]
    _move_by(game, me, 2, walk, d)
    _stand(game, me, walk)


def race_turn(game, live):
    """`race_turn` (ally-siege, capital-race): the shooters first (support the assault, strike what stands within 2 of the city
    and the city), then the melee units by id."""
    rows = live & fam_rows(game, "ally-siege", "capital-race")
    if not bool(mx.any(rows).item()):
        return
    city = _target_tile(game)
    for ranged_pass in (True, False):
        for _, me, mine in _slots(game, rows):
            is_r = g1(game.obs.kind(game.s, "range"), me) >= 0
            sel = mine & rows & (is_r == ranged_pass)
            if not bool(mx.any(sel).item()):
                continue
            if ranged_pass:
                _script(game, 1, city=city, near=(city, 2))
                armycamp.unit_turn(game, me, 2, sel, mx.zeros((game.B,), dtype=I32), city)
            else:
                race_melee(game, me, sel, city)
    _script_off(game)


def third_turn(game, live):
    """The third seat's turn of the party families: the family's script, then the scene rule's part (`third_rule`), then
    `mark`."""
    if not has_family(game):
        return
    rows = live & is_party(game)
    if not bool(mx.any(rows).item()):
        return
    walk_turn(game, rows)
    hold_turn(game, live)
    hunt_turn(game, live)
    race_turn(game, live)
    R.seat_turn(game, 2, rows)
    mark(game)


# ------------------------------------------------------------------------------------------------ the enemy

def _enemy_aim(game, ranged, ally_city):
    """[B] int: the enemy's aim by family: 1 Hold(own tile), 0 Close / Support (neutral-joins-war), defend-friend: 0 for a
    ranged unit, else 2 To(the ally's city)."""
    close = fam_rows(game, "neutral-joins-war")
    to_city = fam_rows(game, "defend-friend")
    return mx.where(close, 0, mx.where(to_city, mx.where(ranged, 0, 2), 1))


def enemy_turn(game, live, side=1):
    """`third_party::enemy_turn`: the enemy's pieces in order, `unit_turn` (Even) with the targets of ours and, where the third
    is at war with it, the third's. The units hold (neutral-path, ally-siege, closed-borders, spare-friends, capital-race),
    close in (neutral-joins-war; defend-friend: a melee unit goes to the ally's city, a shooter supports)."""
    if not has_family(game):
        return
    rows = live & is_party(game)
    if not bool(mx.any(rows).item()):
        return
    B, U = game.B, game.U
    city = _target_tile(game)
    # defend-friend: the ally's city is a target too (`with_city`).
    df = fam_rows(game, "defend-friend")
    # The city column only where a defend-friend row exists (CPU MLX segfaults on the unused column).
    _script(game, 0, city=mx.where(df, city, -1) if bool(mx.any(df & rows).item()) else None)
    for u in range(U):
        s = game.s
        me = mx.full((B,), u, dtype=I32)
        mine = rows & g1(s["u_alive"], me) & (g1(s["u_side"], me) == side) & (g1(s["u_tile"], me) >= 0) & (g1(s["u_mv"], me) > 0) \
            & (u >= game.n_mine) & (u < _n_main(game))
        if not bool(mx.any(mine).item()):
            continue
        ranged = g1(game.obs.kind(s, "range"), me) >= 0
        aim = _enemy_aim(game, ranged, city)
        goal = mx.where(aim == 1, g1(s["u_tile"], me), city)
        armycamp.unit_turn(game, me, side, mine, aim, goal)
    _script_off(game)


def _n_main(game):
    v = getattr(game, "_n_main", None)
    if v is None:
        v = game._n_main = mx.take(mx.array([sc["scene"].get("n_main", 0) for sc, _, _ in game.plays], dtype=I32), game.idx)
    return v


def _target_tile(game):
    """[B] the window tile of the scene's target city (defend-friend's `To`), -1 none."""
    s = game.s
    ct = game.sim.cs["tile"]
    if ct.shape[1] == 0:
        return mx.full((game.B,), -1, dtype=I32)
    return mx.where(game.has_target, g1(ct, mx.minimum(game.target, ct.shape[1] - 1)), -1)


# ------------------------------------------------------------------------------------------------ the judge

def _enemy_tiles(game):
    """[B, T] the enemy's improvements of spare-friends (the scene's `enemy_tiles`, map ids -> window tiles)."""
    v = getattr(game, "_enemy_tiles", None)
    if v is None:
        T = game.sim.T
        rows = []
        for sc, _, _ in game.plays:
            w = sc["scene"]["window"]
            row = [False] * T
            for t in ((sc["orders"].get("info") or {}).get("enemy_tiles") or "").split(","):
                if t:
                    x, y = int(t) % w["map_w"] - w["q0"], int(t) // w["map_w"] - w["r0"]
                    if 0 <= x < w["w"] and 0 <= y < w["h"]:
                        row[y * w["w"] + x] = True
            rows.append(row)
        v = game._enemy_tiles = mx.take(mx.array(rows), game.idx, axis=0)
    return v


def mark(game):
    """`third_party::mark` (capture families): the first time the target city is seen taken by someone but the enemy, who took it
    and what ours had lost by then."""
    s = game.s
    if not active(s):
        return
    if s["c_owner"].shape[1] == 0:
        return
    cap = fam_rows(game, *CAPTURE) & game.has_target
    ct = mx.minimum(game.target, s["c_owner"].shape[1] - 1)
    owner = g1(s["c_owner"], ct)
    new = cap & (owner >= 0) & (owner != 1) & (s["pty_by"] < 0)
    mine = s["u_alive"] & (s["u_side"] == 0) & (mx.arange(game.U)[None, :] < game.n_mine[:, None])
    lost = game.n_mine - mx.sum(mine, axis=1)
    s["pty_by"] = mx.where(new, owner, s["pty_by"])
    s["pty_lost"] = mx.where(new, lost, s["pty_lost"])
    game.s = s


def judge(game, rel, v):
    """`third_party::judge` of the party rows: the verdict [B] (1 won, 0 lost, -1 on) over `v`."""
    s = game.s
    if not active(s) or not has_family(game):
        return v
    mark(game)
    s = game.s
    U = game.U
    slots = mx.arange(U)[None, :]
    alive = s["u_alive"] & (s["u_tile"] >= 0)
    civ = game.obs.kind(s, "civ")
    mine = s["u_alive"] & (s["u_side"] == 0) & (slots < game.n_mine[:, None])
    n_alive = mx.sum(mine, axis=1)
    gone = game.n_mine - n_alive
    cap = mx.array([LOSS_CAP.get(f, 0) for f in game.fam], dtype=I32)
    limit = rel >= game.rounds
    counted = mx.where(s["pty_by"] >= 0, s["pty_lost"], gone)
    broken = s["pty_broken"] | (counted > cap)
    s["pty_broken"] = broken
    game.s = s
    theirs = alive & (s["u_side"] == 1) & (slots >= game.n_mine[:, None]) & (slots < _n_main(game)[:, None]) & ~civ
    left_e = mx.sum(theirs, axis=1)
    first, count = armycamp.third_span(game)
    third = alive & (s["u_side"] == 2) & (slots >= first[:, None]) & (slots < (first + count)[:, None]) & ~civ
    left_t = mx.sum(third, axis=1)
    all_dead = n_alive == 0
    out = v
    # neutral-path: the enemy's units all dead; neutral-joins-war: the enemy's and the neutral's.
    kill = mx.where(fam_rows(game, "neutral-joins-war"), left_e + left_t, left_e)
    kv = mx.where(broken | all_dead | (limit & (kill > 0)), 0, mx.where(kill == 0, 1, -1))
    out = mx.where(fam_rows(game, "neutral-path", "neutral-joins-war"), kv, out)
    # The capture families: who took the city first.
    by = s["pty_by"]
    cv = mx.where(by == 0, mx.where(broken, 0, 1), mx.where(by >= 0, 0, mx.where(broken | all_dead | limit, 0, -1)))
    out = mx.where(fam_rows(game, *CAPTURE), cv, out)
    # defend-friend: the ally's city is still the ally's (or the attackers dead) with at most 3 lost.
    if s["c_owner"].shape[1] == 0:
        held = mx.zeros((game.B,), dtype=mx.bool_)
    else:
        ct = mx.minimum(game.target, s["c_owner"].shape[1] - 1)
        held = game.has_target & (g1(s["c_owner"], ct) == 2)
    dv = mx.where(~held | broken | all_dead, 0, mx.where((left_e == 0) | limit, 1, -1))
    out = mx.where(fam_rows(game, "defend-friend"), dv, out)
    # spare-friends: 3 of the enemy's improvements pillaged.
    done = mx.sum(game.pillaged & _enemy_tiles(game), axis=1)
    sv = mx.where(broken | all_dead, 0, mx.where(done >= PILLAGE_K, 1, mx.where(limit, 0, -1)))
    out = mx.where(fam_rows(game, "spare-friends"), sv, out)
    return mx.where(is_party(game), out, v)
