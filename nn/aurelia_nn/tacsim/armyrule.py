"""The scene rule (the trainers' default; Rust `ARMY_SCENE_RULE=standard` turns it off: `crates/qa/src/trainers/scene_rule.rs`) in MLX, batched over
scenes. At the end of a seat's scripted part (the net on the learner's seat, the family script on the enemy's), for
that seat's units in every scene, in this order:

1. promotions: a unit with a promotion to take takes one (an instant heal below 50 HP, else the first promotion in
   content order that does not heal; `ai::combat::promotion_choices`);
2. city strikes: each city of the seat (by handle) strikes the foe unit the strike leaves with the fewest HP (a kill
   leaves 0; ties: the lowest tile), expected roll;
3. shots: each ranged unit with moves left (slot order) shoots from where it stands, same key, when the rules accept
   the shot (`Obs.preview`'s ok: no set-up, reach, sight);
4. stand and fortify: each unit ends its turn (`Sim.hold_units`: an unmoved one heals); an unmoved one (moves full
   when the step starts) fortifies where `Sim.masks`'s fortify condition holds, unless it is set up.

The arena is closed, so a seat's units in the window are its scene pieces (slot order = piece order).
"""

import mlx.core as mx

from . import armyhist as H
from . import armyobs as A
from . import exact
from .sim import g1, gn, I32, V_ATTACK, V_MOVE, V_SETUP, Sim

BIG = 1 << 20
# A strike's key: the HP it leaves x 4096 + the target's window tile (< 4096); NOKEY: no target.
NOKEY = 1 << 30


def on_rows(game, rows, fn):
    """`fn(take)` on the rows where `rows` [B] holds (the others are not touched): the game's per-row state is swapped for
    those rows' for the call and merged back (`Game._enter_rows`); `take` maps a full-size [B, ...] array to the
    rows in view. Where most rows are in, nothing is swapped. Rows are independent, so the result is the same."""
    n = int(mx.sum(rows).item())
    if n == 0:
        return
    if 4 * n >= 3 * game.B:
        fn(lambda a: a)
        return
    ids, pos = game._compact(rows, n)
    saved = game._enter_rows(ids)
    fn(lambda a: mx.take(a, ids, axis=0))
    game._leave_rows(saved, rows, pos)
    mx.eval(game.s)


def city_order(game):
    """[B, C]: the window's cities by handle (absent cities last)."""
    if getattr(game, "_city_order", None) is None:
        C = game.sim.cs["tile"].shape[1]
        rows = []
        for sc, _, _ in game.plays:
            h = list(sc["orders"].get("city_handles", []))[:C]
            h = h + [-1] * (C - len(h))
            rows.append(sorted(range(C), key=lambda c: (h[c] < 0, h[c], c)))
        game._city_order = mx.take(mx.array(rows, dtype=I32), game.idx, axis=0)
    return game._city_order


def promotions(game, side, live):
    s, sim = game.s, game.sim
    B, U = s["u_alive"].shape
    mine = live[:, None] & s["u_alive"] & (s["u_side"] == side) & (s["u_tile"] >= 0)
    level = mx.maximum(s["u_level"], 1)
    pct = mx.maximum(s["p_xp_needed_pct"], -100)
    need = mx.maximum((10 * level * (level + 1) // 2) * (100 + pct) // 100, 1)
    ready = mine & (s["u_xp"] >= need)
    if not bool(mx.any(ready).item()):
        return
    rd = ready.tolist()
    for u in range(U):
        if not any(r[u] for r in rd):
            continue
        s = game.s
        me = mx.full((B,), u, dtype=I32)
        av = sim.promo_mask(s, me, g1(ready, me))                       # [B, NP]
        NP = av.shape[1]
        insta = mx.take(sim.z["z_insta"], sim.tid, axis=0)               # [B, NP]
        ar = mx.arange(NP)[None, :]
        first = lambda m: mx.min(mx.where(m, ar, BIG), axis=1)
        heal, plain, anyp = first(av & insta), first(av & ~insta), first(av)
        low = g1(s["u_hp"], me) < 50
        pick = mx.where(low & (heal < BIG), heal, mx.where(plain < BIG, plain, anyp))
        take = anyp < BIG
        if bool(mx.any(take).item()):
            def run(s=s, me=me, pick=pick, take=take):
                game.s = sim.promote(s, me, mx.minimum(pick, NP - 1), take)
            if side == 0 and getattr(game, "facts", False):
                H.log_cmd(game, me, mx.full((B,), H.HK_OTHER, dtype=I32), mx.full((B,), -1, dtype=I32), take, _rnd(game), run)
            else:
                run()
            game.counts["rule promotions"] += int(mx.sum(take).item())
        mx.eval(game.s)


def city_strikes(game, side, live):
    sim = game.sim
    C = sim.cs["tile"].shape[1]
    order = city_order(game)
    for j in range(C):
        s = game.s
        B = s["u_alive"].shape[0]
        cs = order[:, j]
        ready = g1(sim.cs["alive"], cs) & (g1(s["c_owner"], cs) == side) & (g1(sim.cs["tile"], cs) >= 0) & ~s["done"] \
            & ~g1(s["c_strike"], cs) & (g1(s["c_dmg"], cs) < sim.city_maxhp(s, cs))
        on_rows(game, ready & live, lambda take, j=j: _city_strike(game, side, take(order)[:, j]))
        mx.eval(game.s)


def _city_strike(game, side, cs):
    """City `cs` [B] strikes in every row in view (the city is ready in each; the target may be none)."""
    s, sim, obs = game.s, game.sim, game.obs
    B, U = s["u_alive"].shape
    T = sim.T
    sd = mx.full((B,), side, dtype=I32)
    tiles = mx.broadcast_to(mx.arange(T, dtype=I32)[None], (B, T))
    ok = sim.city_mask(s, sd, cs)
    mil, civ = sim.occupancy(s)
    ms, cv = gn(mil, tiles), gn(civ, tiles)
    is_unit = ms >= 0
    dsl = mx.where(is_unit, ms, cv)
    hp = mx.where(dsl >= 0, gn(s["u_hp"], mx.maximum(dsl, 0)), 0)
    _, c_rstr = sim.city_strength(s, cs, mil)
    # The damage to each unit slot (a military unit on the tile; the tile gathers its slot's): per slot, not per tile
    # (the fighters' gathers over every window tile were the 18 GB peak of a 10 000 scene batch).
    slots = mx.broadcast_to(mx.arange(U, dtype=I32)[None], (B, U))
    dfn = obs.defense_vs_ranged(s, obs.fighter_n_slots(s, slots, mx.ones((B, U), dtype=mx.bool_)), obs.fighter_n_slots(s, mx.full((B, U), -1, dtype=I32), mx.zeros((B, U), dtype=mx.bool_)))
    dmg_u = A.expected_damage(mx.broadcast_to(c_rstr[:, None], (B, U)), dfn, mx.zeros((B, U), dtype=I32), mx.zeros((B, U), dtype=I32)).astype(I32)
    dmg = gn(dmg_u, mx.maximum(ms, 0))
    dmg = mx.where(is_unit, dmg, exact.CIVILIAN_DAMAGE)
    left = mx.maximum(hp - mx.minimum(dmg, hp), 0)
    key = mx.where(ok, left * 4096 + tiles, NOKEY)
    best = mx.min(key, axis=1)
    take = best < NOKEY
    t = (best % 4096).astype(I32)
    game.s = sim.city_strike(s, sd, cs, mx.where(take, t, 0), take)
    game.counts["rule city strikes"] += int(mx.sum(take).item())


def shooter_slots(game, side):
    """The slots that may hold a ranged unit of `side` (the start's ranged units, and every slot past the start's
    units: spawns)."""
    key = f"_shooters{side}"
    if getattr(game, key, None) is None:
        rk = game.tab.k["range"].tolist()
        U = game.U
        out = set()
        for sc, _, _ in game.plays:
            init = sc["initial"]["units"]
            if side == 1 and sc["scene"].get("n_spare"):
                out.update(range(sc["scene"]["n_main"], sc["scene"]["n_main"] + sc["scene"]["n_spare"]))  # the camp's spawns
            for k in range(U):
                if k >= len(init):
                    out.add(k)
                elif init[k][0] and init[k][8] == side and rk[init[k][9]] >= 0:
                    out.add(k)
        setattr(game, key, sorted(out))
    return getattr(game, key)


def shots(game, side, live):
    obs = game.obs
    for u in shooter_slots(game, side):
        s = game.s
        B = s["u_alive"].shape[0]
        me = mx.full((B,), u, dtype=I32)
        can = live & g1(s["u_alive"], me) & (g1(s["u_side"], me) == side) & (g1(s["u_tile"], me) >= 0) & (g1(obs.kind(s, "range"), me) >= 0) \
            & (g1(s["u_mv"], me) > 0) & ~g1(obs.kind(s, "civ"), me)
        on_rows(game, can, lambda take, u=u: _shoot(game, side, u))
        mx.eval(game.s)


def _shoot(game, side, u):
    """Slot `u`'s shot in every row in view (each holds a ranged unit of `side` with moves left in that slot)."""
    sim, obs = game.sim, game.obs
    s = game.s
    B, U = s["u_alive"].shape
    sd = mx.full((B,), side, dtype=I32)
    me = mx.full((B,), u, dtype=I32)
    can = mx.ones((B,), dtype=mx.bool_)
    tile = s["u_tile"]
    mil, civ = sim.occupancy(s)
    tl = mx.maximum(tile, 0)
    slots = mx.arange(U, dtype=I32)[None, :]
    foe = _foes(s, side)
    # One mark a tile: the defender (its military unit, else the civilian).
    dm, dc = gn(mil, tl), gn(civ, tl)
    mark = foe & ((dm == slots) | ((dm < 0) & (dc == slots)))
    frm = mx.broadcast_to(g1(tile, me)[:, None], (B, U))
    dealt, _, dhp, ok = obs.preview_packed(s, me, frm, tile, mark & can[:, None], True, ranged_only=True)
    ok = ok & mark & can[:, None]
    left = mx.maximum(dhp - dealt, 0)
    key = mx.where(ok, left * 4096 + tl, NOKEY)
    best = mx.min(key, axis=1)
    take = best < NOKEY
    t = (best % 4096).astype(I32)
    mk = sim.masks(s, sd, me, rows=take)

    def run():
        game.s = sim.apply_unit(s, sd, me, mx.full((B,), V_ATTACK, dtype=I32), mx.where(take, t, 0), mk, take)
    if side == 0 and getattr(game, "facts", False):
        H.log_cmd(game, me, mx.full((B,), H.HK_RANGED, dtype=I32), mx.where(take, t, -1), take, _rnd(game), run)
    else:
        run()
    game.counts["rule shots"] += int(mx.sum(take).item())


def stand_fortify(game, side, live, heal=True):
    s, sim = game.s, game.sim
    mine = live[:, None] & s["u_alive"] & (s["u_side"] == side) & (s["u_tile"] >= 0)
    # Only a piece that did not move or act this turn fortifies (its moves full when the step starts: the pieces the
    # stand heals); Rust `scene_rule::turn` step 4, amended 24 Nov.
    stood = mine & (s["u_mv"] > 0) & (s["u_mv"] >= s["u_mmax"])
    s = sim.hold_units(s, mine & (s["u_mv"] > 0), heal=heal)
    water = sim.mg("water", s["u_tile"]) > 0
    st = s["u_st"]
    fort = stood & ~((st == 1) | (st == 2) | (st == 3)) & ~s["p_civilian"] & ~s["p_no_defensive_bonus"] & s["p_land"] & ~water
    s["u_st"] = mx.where(fort, 1, st)
    game.s = s
    if side == 0 and getattr(game, "facts", False):
        H.plain_facts(game, fort, H.HK_FORT, _rnd(game))


def _rnd(game):
    """[B] the round of the learner's turn now (the history's facts)."""
    return mx.full((game.s["u_alive"].shape[0],), game.cur_rel, dtype=I32)


def seat_turn(game, side, live):
    """The rule's part of `side`'s turn in every live scene (after the seat's script, before its turn's end)."""
    promotions(game, side, live)
    city_strikes(game, side, live)
    shots(game, side, live)
    stand_fortify(game, side, live, heal=game.hold_heals)


def enemy_turn(game):
    """The enemy's turn under the rule: the family script (the assault families' field units), the rule, the end."""
    s = game.s
    B = game.B
    live = ~s["done"] & (game.verdict < 0)
    field_turn(game, live)
    # The fog families' scripts (twin package 3; `armyfog`): nothing where no row is one.
    from . import armyfog
    armyfog.enemy_turn(game, live)
    # Guard, convoy, snipe (twin package 7; `armyciv`).
    from . import armyciv
    armyciv.enemy_turn(game, live)
    # The camp families' barbarians (twin package 4; `armycamp`): the script, then after the rule the countdown and the spawn.
    from . import armycamp
    armycamp.enemy_turn(game, live)
    # The third-party families' enemy (twin package 8; `armyparty`).
    from . import armyparty
    armyparty.enemy_turn(game, live)
    seat_turn(game, 1, live)
    armycamp.barbarian_end(game, live)
    game.s = game.sim.end_turn(game.s, mx.full((B,), 1, dtype=I32), live)


# ---------------------------------------------------------------------------------------------- the family script
# The assault families' field units (`assault::enemy_turn`): each one's `act::unit_turn`, Close (melee) or Support
# (ranged), Nerve::Even, targets our live units' tiles, friends the enemy's pieces. Ported step for step: the blow from
# where it stands (`best_blow`), else the approach and blow (`approach_strike`), else the aim's move (`move_by`) and the
# blow from there; then the unit stands. Ties as Rust's: the lowest tile, the first target in the pieces' order.

def _foes(s, side):
    """The units at war with `side` (the other side; with a third seat the war table, `Sim.is_foe`)."""
    if "war" in s:
        return s["u_alive"] & (s["u_tile"] >= 0) & Sim.is_foe(s, mx.full((s["u_alive"].shape[0],), side, dtype=I32), s["u_side"])
    return s["u_alive"] & (s["u_tile"] >= 0) & (s["u_side"] == 1 - side)


def _only(game, s, foes):
    """A script's target list restricted to one tile where a foe stands on it (`game.script_only` [B], -1 none: the camp
    families' neutral strikes the unit on the camp while one stands there), or to the foes within `radius` of a tile
    (`game.script_near` = (tile [B], radius): the third party's race strikes what stands near the city)."""
    only = getattr(game, "script_only", None)
    near = getattr(game, "script_near", None)
    if near is not None:
        tile, radius = near
        d = game.sim.geo.distance(s["u_tile"], mx.broadcast_to(tile[:, None], s["u_tile"].shape))
        foes = foes & (d <= radius)
    if only is None:
        return foes
    there = foes & (s["u_tile"] == only[:, None])
    return mx.where(mx.any(there, axis=1)[:, None], there, foes)


def _marks(game, s, side):
    """Our units that defend their tile (one a tile: its military unit, else the civilian) [B, U]."""
    U = s["u_alive"].shape[1]
    mil, civ = game.sim.occupancy(s)
    tl = mx.maximum(s["u_tile"], 0)
    dm, dc = gn(mil, tl), gn(civ, tl)
    slots = mx.arange(U, dtype=I32)[None, :]
    return _only(game, s, _foes(s, side)) & ((dm == slots) | ((dm < 0) & (dc == slots)))


def _cols(game, s, side):
    """The strike targets of a script: (the tiles [B, N], the marks [B, N]): our units' tiles and, where the script names one
    (`game.script_city` [B] window tile, -1 none: the ally's city, the city the third party takes), that city's tile
    when the city is at war with `side`."""
    mark = _marks(game, s, side)
    to = s["u_tile"]
    city = getattr(game, "script_city", None)
    if city is None:
        return to, mark
    if getattr(game, "script_city_only", False):
        mark = mx.zeros_like(mark)
    sim = game.sim
    owner = _city_owner_at(game, s, city)
    foe = Sim.is_foe(s, mx.full((city.shape[0],), side, dtype=I32), owner) if "war" in s else owner == 1 - side
    return mx.concatenate([to, mx.maximum(city, 0)[:, None]], axis=1), mx.concatenate([mark, foe[:, None]], axis=1)


def _city_owner_at(game, s, city):
    """[B] the owner (side) of the city on the window tile `city` [B], -1 none (and in a scene with no city in the window)."""
    if s["c_owner"].shape[1] == 0:
        return mx.full(city.shape, -1, dtype=I32)
    cs = game.sim.city_on(s, mx.maximum(city, 0)[:, None])[:, 0]
    return mx.where((cs >= 0) & (city >= 0), g1(s["c_owner"], mx.maximum(cs, 0)), -1)


def _city_bonus(game, s, to):
    """`best_blow`'s city bonus (20) for the tiles `to` [B, N] that hold a city."""
    return mx.where(game.sim.city_on(s, to) >= 0, 20, 0)


def _lex_min(keys, mask):
    """The first index of the lexicographic minimum of `keys` (a list of [B, N] int arrays) where `mask` [B, N]."""
    m = mask
    for k in keys:
        best = mx.min(mx.where(m, k, NOKEY), axis=1)
        m = m & (k == best[:, None])
    j = A.first_index_eq(m.astype(I32), mx.ones((m.shape[0],), dtype=I32))
    return mx.minimum(j, m.shape[1] - 1), mx.any(mask, axis=1)  # no candidate: n; clamp (CPU MLX segfaults on the out-of-range gather)


def _blow(game, me, side, take):
    """`act::best_blow` (Even) from where `me` stands on our units: (has one, the target tile) [B]. The ties go to the first
    target of the script's list: the pieces' order, or the lowest tile where the script's targets are sorted (`game.script_sorted`).
    `game.script_nerve` 'careful' / 'reckless': the other nerves of `act::Nerve`."""
    s, obs = game.s, game.obs
    B, U = s["u_alive"].shape
    tile = g1(s["u_tile"], me)
    to, mark = _cols(game, s, side)
    N = to.shape[1]
    mark = mark & take[:, None] & (g1(s["u_mv"], me) > 0)[:, None]
    dealt, taken, dhp, ok = obs.preview_packed(s, me, mx.broadcast_to(tile[:, None], (B, N)), to, mark, True)
    hp = g1(s["u_hp"], me)[:, None]
    kills = dealt >= dhp
    fine = ok & mark & _nerve(game, kills, dealt, taken, hp)
    capt = mx.concatenate([~obs.kind(s, "civ") & (obs.kind(s, "range") < 0)] + ([mx.zeros((B, 1), dtype=mx.bool_)] if N > U else []), axis=1)
    score = mx.where(kills, 10000, 0) + (100 - mx.maximum(dhp - dealt, 0)) * 10 + mx.where(capt, 50, 0) + _city_bonus(game, s, to) - taken
    # Ties: the first target in the pieces' order (Rust's `ours` is `mine_alive`, not sorted).
    tie = to if getattr(game, "script_sorted", False) else mx.broadcast_to(mx.arange(N, dtype=I32)[None, :], (B, N))
    j, has = _lex_min([-score, tie], fine)
    game.blow_kill = mx.take_along_axis(kills, j[:, None], axis=1)[:, 0]  # the best blow kills (score >= 10 000)
    return has, mx.take_along_axis(to, j[:, None], axis=1)[:, 0]


def _nerve(game, kills, dealt, taken, hp):
    """`act::Nerve`: Even (the default), Careful (a kill, or twice what it takes and under half its hit points), Reckless."""
    nerve = getattr(game, "script_nerve", "even")
    if nerve == "reckless":
        return mx.ones_like(kills)
    if nerve == "careful":
        return kills | ((dealt >= 2 * taken) & (taken < hp // 2))
    return kills | ((dealt >= taken) & (taken < hp))


def _strike(game, me, side, tile, take):
    """`act::strike`: a unit that must set up does so first."""
    sim = game.sim
    B = me.shape[0]
    sd = mx.full((B,), side, dtype=I32)
    s = game.s
    need = take & g1(s["p_must_set_up"], me) & (g1(s["u_st"], me) != 3)
    if bool(mx.any(need).item()):
        mk = sim.masks(s, sd, me, rows=need)
        game.s = sim.apply_unit(s, sd, me, mx.full((B,), V_SETUP, dtype=I32), mx.zeros((B,), dtype=I32), mk, need & mk["setup"])
    mk = sim.masks(game.s, sd, me, rows=take)
    # The set-up spends a move: a unit that walked first and sets up has none left, and Rust refuses its strike.
    take = take & (g1(game.s["u_mv"], me) > 0)
    game.s = sim.apply_unit(game.s, sd, me, mx.full((B,), V_ATTACK, dtype=I32), mx.maximum(tile, 0), mk, take)


def _threat(game, s, side, tiles):
    """`act::threat_on(foe, x)`: our military units within their moves plus reach of each of `tiles` [B, N]."""
    obs = game.obs
    fd = obs.field(s, side)
    q, r = obs.mapq(tiles)
    d = A.hexd(q[:, :, None] - fd["uq"][:, None, :], r[:, :, None] - fd["ur"][:, None, :])
    milfoe = fd["milfoe"]
    ts = getattr(game, "script_threat", None)
    if ts is not None:
        # `act::threat_on(foe, x)` of a script whose `foe` is one nation (the camp barbarians': the learner's units only).
        milfoe = s["u_alive"] & (s["u_tile"] >= 0) & (s["u_side"] == ts) & ~fd["civ"]
    return mx.sum(milfoe[:, None, :] & (d <= fd["reach"][:, None, :]), axis=2)


def _approach(game, me, side, take):
    """`act::approach_strike`: (has one, the tile to move to, the target tile) [B]."""
    s, sim, obs, geo = game.s, game.sim, game.obs, game.sim.geo
    B, U = s["u_alive"].shape
    mk = sim.masks(s, mx.full((B,), side, dtype=I32), me, rows=take)
    lt, left, ok_l = mk["lt"], mk["left"], mk["move_l"]
    L = lt.shape[1]
    rng = g1(obs.kind(s, "range"), me)
    reach = mx.where(rng >= 0, rng, 1)
    tiles, mark = _cols(game, s, side)
    N = tiles.shape[1]
    d = geo.distance(mx.broadcast_to(lt[:, :, None], (B, L, N)), mx.broadcast_to(tiles[:, None, :], (B, L, N)))
    pair = (ok_l & (left > 0) & take[:, None])[:, :, None] & mark[:, None, :] & (d <= reach[:, None, None])
    flat = pair.reshape(B, L * N)
    K = int(mx.max(mx.sum(flat, axis=1)).item())
    if K == 0:
        return mx.zeros((B,), dtype=mx.bool_), mx.zeros((B,), dtype=I32), mx.zeros((B,), dtype=I32)
    idx = mx.argsort(mx.where(flat, 0, 1), axis=1)[:, :K].astype(I32)
    valid = mx.take_along_axis(flat, idx, axis=1)
    frm = mx.take_along_axis(lt, idx // N, axis=1)
    to = mx.take_along_axis(tiles, idx % N, axis=1)
    dealt, taken, dhp, ok = obs.preview_packed(s, me, frm, to, valid, False)
    hp = g1(s["u_hp"], me)[:, None]
    kills = dealt >= dhp
    fine = ok & valid & _nerve(game, kills, dealt, taken, hp)
    score = mx.where(kills, 10000, 0) + (100 - mx.maximum(dhp - dealt, 0)) * 10 - taken
    tie = to if getattr(game, "script_sorted", False) else idx % N
    j, has = _lex_min([_threat(game, s, side, frm), -score, frm, tie], fine)
    pick = lambda a: mx.take_along_axis(a, j[:, None], axis=1)[:, 0]
    return has, pick(frm), pick(to)


def _move_by_aim(game, me, side, take):
    """`act::move_by` with Close (melee) or Support (ranged): (moves, the tile) [B]; the own tile best: no move."""
    s, sim, obs = game.s, game.sim, game.obs
    B, U = s["u_alive"].shape
    mk = sim.masks(s, mx.full((B,), side, dtype=I32), me, rows=take)
    lt, left, ok_l = mk["lt"], mk["left"], mk["move_l"]
    L = lt.shape[1]
    own = (mx.arange(L)[None, :] == 0) & (lt >= 0)
    opt = (ok_l | own) & take[:, None]
    q, r = obs.mapq(lt)
    fd = obs.field(s, side)
    foes = _only(game, s, _foes(s, side))
    d_all = A.hexd(q[:, :, None] - fd["uq"][:, None, :], r[:, :, None] - fd["ur"][:, None, :])
    d_foe = d_all
    city = getattr(game, "script_city", None)
    if city is not None:
        # The script's city is one more target of its list (`targets(game)`): the distances look at it too.
        to, mark = _cols(game, s, side)
        cq, cr = obs.mapq(mx.maximum(city, 0)[:, None])
        d_city = A.hexd(q - cq, r - cr)[:, :, None]
        foes = mx.concatenate([foes, mark[:, -1:]], axis=1)
        d_foe = mx.concatenate([d_all, d_city], axis=2)
    d = A.min_or(foes[:, None, :], d_foe, 2, 99)
    rng = g1(obs.kind(s, "range"), me)
    ranged = (rng >= 0)[:, None]
    reach = mx.where(rng >= 0, rng, 1)[:, None]
    dfn = obs.defence(s, mx.maximum(lt, 0), side)
    melee_f = s["u_alive"] & (s["u_tile"] >= 0) & (s["u_side"] == side) & (obs.kind(s, "range") < 0) & ~obs.kind(s, "civ")
    cover = A.min_or(melee_f[:, None, :], d_all, 2, 99) <= 1
    thr = _threat(game, s, side, lt)
    i = lambda b: b.astype(I32)
    keys_close = [mx.maximum(d - 1, 0), -dfn, lt]
    keys_sup = [i(d <= 1), i(~(d <= reach)), i(~cover), thr, mx.abs(d - reach), -dfn, lt]
    jc, _ = _lex_min(keys_close, opt)
    js, has = _lex_min(keys_sup, opt)
    j = mx.where(ranged[:, 0], js, jc)
    go = has & (j != 0)
    return go, mx.take_along_axis(lt, j[:, None], axis=1)[:, 0]


def field_turn(game, live, side=1):
    """The family script of the assault families on every field unit (slot order = the pieces' order)."""
    B, U = game.B, game.U
    is_assault = getattr(game, "_is_assault", None)
    if is_assault is None:
        from .armyplay import ASSAULT
        # False: a python fact of the plays, so a game without an assault family reads nothing from the device here
        is_assault = game._is_assault = mx.array([f in ASSAULT for f in game.fam]) if any(f in ASSAULT for f in game.fam) else False
    if is_assault is False:
        return
    fl = mx.any(game.field & is_assault[:, None], axis=0).tolist()
    for u in [k for k in range(U) if fl[k]]:
        me = mx.full((B,), u, dtype=I32)
        s = game.s
        mine = live & is_assault & g1(game.field, me) & g1(s["u_alive"], me) & (g1(s["u_side"], me) == side) & (g1(s["u_tile"], me) >= 0) \
            & (g1(s["u_mv"], me) > 0)
        if not bool(mx.any(mine).item()):
            continue
        close_unit(game, me, side, mine)


def close_unit(game, me, side, mine):
    """One field unit's `act::unit_turn` (Close for a melee unit, Support for a ranged one, Even) in the rows `mine`
    [B] where slot `me` is a live unit of `side` with moves left."""
    sim = game.sim
    B, U = game.B, game.U
    sd = mx.full((B,), side, dtype=I32)
    s = game.s
    civ = g1(game.obs.kind(s, "civ"), me)
    ok, t = _blow(game, me, side, mine & ~civ)
    hit = mine & ~civ & ok
    if bool(mx.any(hit).item()):
        _strike(game, me, side, t, hit)
    rest = mine & ~civ & ~hit
    if bool(mx.any(rest).item()):
        ok2, frm, t2 = _approach(game, me, side, rest)
        ap = rest & ok2
        if bool(mx.any(ap).item()):
            mk = sim.masks(game.s, sd, me, rows=ap)
            game.s = sim.apply_unit(game.s, sd, me, mx.full((B,), V_MOVE, dtype=I32), frm, mk, ap)
            _strike(game, me, side, t2, ap)
        walk = mine & ~hit & ~ap
    else:
        walk = mine & ~hit
    if bool(mx.any(walk).item()):
        s = game.s
        walk = walk & (g1(s["u_mv"], me) > 0)
        go, tile = _move_by_aim(game, me, side, walk)
        if bool(mx.any(go).item()):
            mk = sim.masks(game.s, sd, me, rows=go)
            game.s = sim.apply_unit(game.s, sd, me, mx.full((B,), V_MOVE, dtype=I32), tile, mk, go)
        ok3, t3 = _blow(game, me, side, walk & ~civ)
        h3 = walk & ~civ & ok3
        if bool(mx.any(h3).item()):
            _strike(game, me, side, t3, h3)
    game.s = sim.hold_units(game.s, (mx.arange(U)[None, :] == me[:, None]) & mine[:, None], heal=game.hold_heals)
    mx.eval(game.s)
