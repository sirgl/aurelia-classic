"""The tactical sub-game, batched on the GPU (MLX), exact to the Rust rules.

One call `step(s, a)` advances every scene of the batch by one action of its own: a unit action
(nothing, fortify, set up, found, move, attack), a city strike, a side's end of turn, or the round end.
Within a side's turn the units act one after another (`a["slot"]`), every scene in parallel; there is
no Python loop over units or tiles inside a step. `masks(s, side, slot)` gives the legal options of
the unit `slot` in every scene (the rules' acceptance: the same options `ai-exam suite-dump` lists).

Sources (Rust): `classic-rules/src/combat/{strength,damage,exchange,terrain,city}.rs`,
`classic-rules/src/systems/combat/{mod,city,heal,experience}.rs`, `kernel/src/navigation/{astar,cost}.rs`,
`kernel/src/derived/visibility.rs`, `kernel/src/systems/{movement,turn,units}.rs`.

The data-oriented split: what is static in a scene comes compiled from the Rust rules (per unit: the
combat-context percents, situational terms, flags, stats, a step-cost table per tile and direction;
per tile: terrain planes, line-of-sight masks; per city: the strength without the garrison); the
dynamics (positions, hit points, moves, stances, zones of control, sight, reach, every combat formula,
the RNG stream, healing, the turn order) run here.

State `s` (dict of arrays, B scenes, U unit slots, C cities, T window tiles):
u_alive u_side u_tile u_hp u_mv u_mmax u_att u_st (0 ready, 1-2 fortified turns, 3 set up) u_xp u_kind,
p_* (the slot's profile), c_owner c_dmg c_strike c_pop, owner[B,T], rng[B] (uint64), n_slots, done.
"""

import mlx.core as mx

from . import exact
from .scene import N_CTX, CTX_FLANKING

I32 = mx.int32
MOVE = 60
# Contexts (kernel `combat_slot`).
ANY, ATTACK, DEFENSE, VS_CITY, ATT_OPEN, ATT_ROUGH, DEF_OPEN, DEF_ROUGH, RATT_OPEN, RATT_ROUGH, DEF_VS_RANGED, RANGED_ATTACK, \
    VS_WOUNDED, VS_FORTIFIED, FRIENDLY, OUTSIDE_FRIENDLY, ATT_FRIENDLY, FLANKING, ADJ_FRIENDLY = range(19)
XP_ATT_MELEE, XP_DEF_MELEE, XP_ATT_CITY_MELEE, XP_ATT_RANGED, XP_DEF_RANGED, XP_ATT_CITY_RANGED = 5, 4, 5, 2, 2, 3
# Verbs and action types.
V_NONE, V_FORT, V_SETUP, V_FOUND, V_MOVE, V_ATTACK, V_STRIKE = range(7)
A_PAD, A_UNIT, A_CITY, A_END, A_ROUND = range(5)
CITY_RANGE = 2
CITY_SIGHT = 3


def g1(arr, idx):
    """arr[b, idx[b]] for arr [B, N] (idx clamped; callers mask idx < 0)."""
    return mx.take_along_axis(arr, mx.maximum(idx, 0)[:, None], axis=1)[:, 0]


def gn(arr, idx):
    """arr[b, idx[b, ...]] for arr [B, N] and idx [B, ...]."""
    B = arr.shape[0]
    flat = mx.maximum(idx, 0).reshape(B, -1)
    return mx.take_along_axis(arr, flat, axis=1).reshape(idx.shape)


def first_index(mask, axis=-1):
    """The first index where `mask` holds along `axis` (its length when none): MLX's argmax over a short
    axis is ~40x slower than a min reduction, so the hot paths use this."""
    n = mask.shape[axis]
    shape = [1] * mask.ndim
    shape[axis] = n
    ids = mx.arange(n).reshape(shape)
    return mx.min(mx.where(mask, ids, n), axis=axis).astype(I32)


def argmin_last(x):
    """argmin over the last axis (first of the ties), through min reductions."""
    return first_index(x == mx.min(x, axis=-1, keepdims=True))


def put1(arr, idx, val, where):
    """arr[b, idx[b]] = val[b] where `where`[b]."""
    N = arr.shape[1]
    hot = (mx.arange(N)[None, :] == idx[:, None]) & where[:, None]
    return mx.where(hot, val[:, None] if val.ndim == 1 else val, arr)


class Sim:
    def __init__(self, geo, m, cost_pool, cstatic, n_pieces, move_radius=None, has_near_imp=True, los=None):
        self.has_near_imp = has_near_imp
        self.geo = geo
        self.m = m
        self.cost_pool = cost_pool
        self.cs = cstatic
        self.n_pieces = n_pieces
        self.B, self.T = m["hills"].shape
        self.U = None
        self.iters = move_radius or geo.move_radius
        g = geo
        # Per scene: line-of-sight bits per (tile, disc offset) for the radius-4 disc (one uint64 per tile).
        self.terr_rough = m["rough"]
        self.cost_flat = cost_pool.reshape(-1)
        ct, ca = cstatic["tile"], cstatic["alive"]
        Bc, C = ct.shape
        idx = mx.where(ca & (ct >= 0), (mx.arange(Bc) * self.T)[:, None] + ct, Bc * self.T)
        self.city_plane = mx.full((Bc * self.T + 1,), -1, dtype=I32).at[idx.reshape(-1)].maximum(
            mx.broadcast_to(mx.arange(C)[None, :], (Bc, C)).astype(I32).reshape(-1))[: Bc * self.T].reshape(Bc, self.T)
        if los is not None:
            self.los = los
            return
        mid = g.disc_mid  # [T, O, K]
        lvl = m["sight_level"]
        eye = m["hills"].astype(I32)
        B, T = self.B, self.T
        lv = mx.where(mid[None] >= 0, gn(lvl, mx.broadcast_to(mid[None], (B,) + mid.shape)), 0)
        blocked = (lv > eye[:, :, None, None]) & (mid[None] >= 0)
        ok = ~mx.any(blocked, axis=3)  # [B, T, O]
        bits = mx.left_shift(mx.array(1, dtype=mx.uint64), mx.arange(g.o).astype(mx.uint64))
        self.los = mx.sum(mx.where(ok, bits[None, None, :], mx.array(0, dtype=mx.uint64)), axis=2)
        self.terr_rough = m["rough"]
        self.cost_flat = cost_pool.reshape(-1)

    # ------------------------------------------------------------------ occupancy and sight

    def occupancy(self, s):
        """Military and civilian unit slot per tile (-1 none): the stacking rules allow one of each."""
        B, T = self.B, self.T
        U = s["u_alive"].shape[1]
        slots = mx.broadcast_to(mx.arange(U)[None, :], (B, U)).astype(I32)
        base = (mx.arange(B) * T)[:, None]
        civ = s["p_civilian"]
        res = []
        for want in (False, True):
            ok = s["u_alive"] & (s["u_tile"] >= 0) & (civ == want)
            idx = mx.where(ok, base + s["u_tile"], B * T)
            arr = mx.full((B * T + 1,), -1, dtype=I32).at[idx.reshape(-1)].maximum(slots.reshape(-1))
            res.append(arr[: B * T].reshape(B, T))
        return res[0], res[1]

    def los_bit(self, src, dst):
        """Line of sight from window tile src to dst ([B, ...] each), within the radius-4 disc."""
        o = self.geo.rel_index(src, dst)
        word = gn(self.los, src)
        bit = mx.right_shift(word, mx.maximum(o, 0).astype(mx.uint64)) & 1
        return (bit == 1) & (o >= 0)

    def unit_sight(self, s):
        water = gn(self.m["water"], s["u_tile"])
        hills = gn(self.m["hills"], s["u_tile"])
        land = mx.minimum(s["p_sight"] + hills.astype(I32), 7)
        emb = s["p_embarks"] & water
        return mx.where(emb, s["p_embarked_sight"], land)

    def sources_see(self, src, radius, valid, tiles):
        """[B, S, N]: does each source tile (radius, valid) [B, S] see each target tile [B, N]? Distances
        and disc offsets from per-tile coordinates by broadcasting, the line-of-sight word gathered once
        per source."""
        geo = self.geo
        R = geo.radius
        sq = mx.take(geo.q, mx.maximum(src, 0)); sr = mx.take(geo.r, mx.maximum(src, 0))
        tq = mx.take(geo.q, mx.maximum(tiles, 0)); tr = mx.take(geo.r, mx.maximum(tiles, 0))
        dq = tq[:, None, :] - sq[:, :, None]
        dr = tr[:, None, :] - sr[:, :, None]
        d = (mx.abs(dq) + mx.abs(dr) + mx.abs(dq + dr)) // 2
        inside = (mx.abs(dq) <= R) & (mx.abs(dr) <= R)
        o = mx.take(geo.rel.reshape(-1), mx.clip(dq + R, 0, 2 * R) * (2 * R + 1) + mx.clip(dr + R, 0, 2 * R))
        word = gn(self.los, src)  # [B, S]
        bit = mx.right_shift(word[:, :, None], mx.maximum(o, 0).astype(mx.uint64)) & 1
        ok = valid[:, :, None] & (src[:, :, None] >= 0) & (tiles[:, None, :] >= 0) & inside & (o >= 0) & (bit == 1)
        return ok & (d <= radius[:, :, None])

    def seen_by(self, s, side, tiles):
        """Is each tile in `tiles` [B, N] visible to `side` [B]: own units' sight, own cities' sight, own
        territory and its neighbours (kernel `visibility.rs`)."""
        src = s["u_tile"]  # [B, U]
        own = s["u_alive"] & (s["u_side"] == side[:, None]) & (src >= 0)
        vis = mx.any(self.sources_see(src, self.unit_sight(s), own, tiles), axis=1)
        ct = self.cs["tile"]
        cown = self.cs["alive"] & (s["c_owner"] == side[:, None]) & (ct >= 0)
        vis = vis | mx.any(self.sources_see(ct, mx.full(ct.shape, CITY_SIGHT, dtype=I32), cown, tiles), axis=1)
        # Own territory shows itself and its neighbours: the target or one of its neighbours is ours.
        tn = mx.take(self.geo.nbr, mx.maximum(tiles, 0), axis=0)  # [B, N, 6]
        owned_t = gn(s["owner"], tiles) == side[:, None]
        owned_n = (gn(s["owner"], tn) == side[:, None, None]) & (tn >= 0)
        vis = vis | owned_t | mx.any(owned_n, axis=2)
        return vis & (tiles >= 0)

    # ------------------------------------------------------------------ the acting unit's options

    def city_on(self, s, tiles):
        """City slot on each tile [B, N] (-1 none): cities do not move, a plane built once."""
        return mx.where(tiles >= 0, gn(self.city_plane, tiles), -1)

    def controls_and_blocks(self, s, side):
        """Per window tile, for the mover's side: a visible enemy unit stands there (blocks the step),
        and the tile exerts a zone of control (a visible enemy military land unit on land, an enemy city)."""
        B, T = self.B, self.T
        mil, civ = self.occupancy(s)
        enemy = 1 - side
        mil_side = mx.where(mil >= 0, gn(s["u_side"], mil), -1)
        civ_side = mx.where(civ >= 0, gn(s["u_side"], civ), -1)
        tiles = mx.broadcast_to(mx.arange(T)[None, :], (B, T)).astype(I32)
        # Visibility only where it is read: the tiles of units and cities ([B, U + C] targets, not [B, T]).
        ut = mx.where(s["u_alive"], s["u_tile"], -1)
        ct = mx.where(self.cs["alive"], self.cs["tile"], -1)
        at = mx.concatenate([ut, ct], axis=1)
        seen = self.seen_by(s, side, at)
        base = (mx.arange(B) * T)[:, None]
        idx = mx.where(at >= 0, base + at, B * T).reshape(-1)
        vis = mx.zeros((B * T + 1,), dtype=I32).at[idx].maximum(seen.astype(I32).reshape(-1))[: B * T].reshape(B, T) > 0
        enemy_unit = ((mil_side == enemy[:, None]) | (civ_side == enemy[:, None]))
        block = enemy_unit & vis
        city = self.city_on(s, tiles)
        city_owner = mx.where(city >= 0, gn(s["c_owner"], city), -1)
        ctrl = vis & (((mil_side == enemy[:, None]) & ~self.m["water"]) | (city_owner == enemy[:, None]))
        return block, ctrl, vis, mil, civ, mil_side, civ_side, city, city_owner

    def reach(self, s, slot, block, ctrl):
        """The acting unit's moves-left wavefront over its local disc (kernel `reachable_this_turn`):
        [B, L] moves left on arrival (-1 unreached) and the window tiles of the disc."""
        geo = self.geo
        B = self.B
        tile = g1(s["u_tile"], slot)
        lt = mx.take(geo.ltile, mx.maximum(tile, 0), axis=0)  # [B, L]
        lt = mx.where((tile >= 0)[:, None], lt, -1)
        L = lt.shape[1]
        lnbr = geo.lnbr  # [L, 6]
        pred_local = mx.broadcast_to(lnbr[None], (B, L, 6))
        pred_tile = mx.where(pred_local >= 0, gn(lt, pred_local), -1)
        dprime = (mx.arange(6) + 3) % 6
        cid = g1(s["p_cost_id"], slot)
        flat = cid[:, None, None] * (self.T * 6) + mx.maximum(pred_tile, 0) * 6 + dprime[None, None, :]
        code = mx.take(self.cost_flat, flat)
        code = mx.where((pred_tile >= 0) & (lt[:, :, None] >= 0), code, -2)
        # Zone of control on the edge p -> l: a controlling tile next to both (the two common neighbours of p).
        n1 = mx.take(geo.nbr.reshape(-1), mx.maximum(pred_tile, 0) * 6 + ((dprime + 5) % 6)[None, None, :])
        n2 = mx.take(geo.nbr.reshape(-1), mx.maximum(pred_tile, 0) * 6 + ((dprime + 1) % 6)[None, None, :])
        c1 = (n1 >= 0) & (gn(ctrl.astype(I32), n1) > 0)
        c2 = (n2 >= 0) & (gn(ctrl.astype(I32), n2) > 0)
        slowed = (c1 | c2) & ~g1(s["p_ignore_zoc"], slot)[:, None, None]
        blocked = mx.where(lt >= 0, gn(block.astype(I32), lt) > 0, True)
        mv = g1(s["u_mv"], slot)
        embarks = g1(s["p_embarks"], slot)
        land_full = g1(s["p_moves_land"], slot)
        emb_full = g1(s["p_moves_embarked"], slot)
        water_l = mx.where(lt >= 0, gn(self.m["water"].astype(I32), lt) > 0, False)
        full_l = mx.where(embarks[:, None] & water_l, emb_full[:, None], mx.where(embarks[:, None], land_full[:, None], 1 << 20))
        start = mx.minimum(mv, full_l[:, 0])
        left = mx.where(mx.arange(L)[None, :] == 0, start[:, None], -1)
        all_moves = (code == 255) | slowed
        for _ in range(self.iters):
            lp = mx.where(pred_local >= 0, gn(left, pred_local), -1)
            step = mx.where(all_moves, lp, mx.minimum(code, lp))
            cand = mx.where((lp > 0) & (code >= 0) & ~blocked[:, :, None], lp - step, -1)
            cand = mx.minimum(cand, full_l[:, :, None])
            left = mx.maximum(left, mx.max(cand, axis=2))
        self._edges = (code, all_moves, blocked, full_l, pred_local)
        return left, lt

    def walk(self, s, slot, mk, goal_tile, foreign):
        """The walk of a move order (kernel `walk`): the A* path to the goal, stepped until a foreign unit
        the mover did not see stands on the next tile (then the unit stops before it). The path is the
        A* parent chain: each tile's parent is the first expanded predecessor that gives its best cost,
        the expansion order (f, g, tile) with f = g + distance to the goal x one move."""
        code, all_moves, blocked, full_l, pred_local = mk["edges"]
        left, lt = mk["left"], mk["lt"]
        B, L = lt.shape
        geo = self.geo
        hit = lt == goal_tile[:, None]
        gl = mx.minimum(first_index(hit), L - 1)
        scale = mx.max(mx.where(mx.arange(L)[None, :] == 0, left, 0), axis=1)  # the start's moves
        g = mx.maximum(scale[:, None] - left, 0)
        h = geo.distance(lt, mx.broadcast_to(goal_tile[:, None], lt.shape)) * MOVE
        key = ((g + h) * 1024 + g) * 1024 + mx.maximum(lt, 0)
        key = mx.where(left >= 0, key, 1 << 30)
        lp = mx.where(pred_local >= 0, gn(left, pred_local), -1)
        step = mx.where(all_moves, lp, mx.minimum(code, lp))
        cand = mx.where((lp > 0) & (code >= 0) & ~blocked[:, :, None], lp - step, -1)
        cand = mx.minimum(cand, full_l[:, :, None])
        valid = (cand == left[:, :, None]) & (left[:, :, None] >= 0) & (pred_local >= 0)
        pkey = mx.where(valid, gn(key, pred_local), 1 << 30)
        best_d = argmin_last(pkey)  # [B, L]
        parent = mx.take_along_axis(pred_local, best_d[:, :, None], axis=2)[:, :, 0]
        parent = mx.where(mx.min(pkey, axis=2) < (1 << 30), parent, -1)
        # The path back from the goal.
        path = [gl]
        v = gl
        for _ in range(self.iters):
            pv = g1(parent, v)
            v = mx.where((v > 0) & (pv >= 0), pv, v)
            path.append(v)
        path = mx.stack(path[::-1], axis=1)  # start ... goal (repeats at the start)
        ptile = gn(lt, path)
        stop = gn(foreign.astype(I32), path) > 0
        first = first_index(stop)
        any_stop = mx.any(stop, axis=1)
        prev = mx.where(any_stop, g1(path, mx.maximum(first - 1, 0)), gl)
        final_tile = g1(lt, prev)
        final_left = g1(left, prev)
        return final_tile, final_left

    def masks(self, s, side, slot):
        """Legal options of unit `slot` of `side` in every scene: fortify, set up, found [B]; move and
        attack [B, T] (window tiles). Also returns what `apply` needs (the wavefront)."""
        B, T = self.B, self.T
        geo = self.geo
        block, ctrl, vis, mil, civ, mil_side, civ_side, city, city_owner = self.controls_and_blocks(s, side)
        alive = g1(s["u_alive"], slot) & (g1(s["u_side"], slot) == side) & ~s["done"]
        tile = g1(s["u_tile"], slot)
        st = g1(s["u_st"], slot)
        mv = g1(s["u_mv"], slot)
        civilian = g1(s["p_civilian"], slot)
        water_here = gn(self.m["water"].astype(I32), tile[:, None])[:, 0] > 0
        land = g1(s["p_land"], slot)
        fort = alive & ~((st == 1) | (st == 2)) & ~civilian & ~g1(s["p_no_defensive_bonus"], slot) & land & ~water_here
        setup = alive & g1(s["p_must_set_up"], slot) & (st != 3) & (mv > 0) & ~water_here
        fo = mx.take_along_axis(self.m["found_ok"].reshape(B, 2 * T), (side * T + mx.maximum(tile, 0))[:, None], axis=1)[:, 0]
        found = alive & g1(s["p_founder"], slot) & fo
        # Moves.
        left, lt = self.reach(s, slot, block, ctrl)
        L = lt.shape[1]
        ms_l = mx.where(lt >= 0, gn(mil_side, lt), -1)
        cs_l = mx.where(lt >= 0, gn(civ_side, lt), -1)
        foreign_l = ((ms_l >= 0) & (ms_l != side[:, None])) | ((cs_l >= 0) & (cs_l != side[:, None]))
        own_same = mx.where(civilian[:, None], cs_l == side[:, None], ms_l == side[:, None])
        ok_l = (left >= 0) & (mx.arange(L)[None, :] > 0) & (lt >= 0) & ~foreign_l & ~own_same
        ok_l = ok_l & mx.where(lt >= 0, mx.take(geo.inner, mx.maximum(lt, 0)), False) & alive[:, None]
        # Attacks (over the radius-4 disc of targets).
        att_d, tgt_d = self.attack_mask(s, side, slot, alive, vis, mil, civ, mil_side, civ_side, city, city_owner)
        return {"fort": fort, "setup": setup, "found": found, "move_l": ok_l, "attack_d": att_d, "tgt_d": tgt_d, "left": left, "lt": lt,
                "edges": self._edges, "foreign_l": foreign_l}

    def global_masks(self, mk):
        """The move and attack masks over the window [B, T] (the parity check reads them)."""
        B, T = self.B, self.T
        base = (mx.arange(B) * T)[:, None]
        def scatter(ok, tiles):
            return mx.zeros((B * T + 1,), dtype=I32).at[mx.where(ok & (tiles >= 0), base + tiles, B * T).reshape(-1)].add(1)[: B * T].reshape(B, T) > 0
        return scatter(mk["move_l"], mk["lt"]), scatter(mk["attack_d"], mk["tgt_d"])

    def attack_mask(self, s, side, slot, alive, vis, mil, civ, mil_side, civ_side, city, city_owner):
        B, T = self.B, self.T
        geo = self.geo
        tile = g1(s["u_tile"], slot)
        tgt = mx.take(geo.disc_tile, mx.maximum(tile, 0), axis=0)  # [B, O]
        d = mx.broadcast_to(geo.off_dist[None], tgt.shape)
        ranged = g1(s["p_is_ranged"], slot)
        rng = g1(s["p_range"], slot)
        base_c = g1(s["p_base_combat"], slot)
        base_r = g1(s["p_base_ranged"], slot)
        civilian = g1(s["p_civilian"], slot)
        allowed = 1 + mx.clip(g1(s["p_attacks_per_turn"], slot), 0, 9)
        ready = alive & ~civilian & ((base_c > 0) | (base_r > 0)) & (g1(s["u_mv"], slot) > 0) & (g1(s["u_att"], slot) < allowed)
        t_city = gn(city, tgt)
        t_city_owner = mx.where((tgt >= 0) & (t_city >= 0), gn(city_owner, tgt), -1)
        t_mil_side = mx.where(tgt >= 0, gn(mil_side, tgt), -1)
        t_civ_side = mx.where(tgt >= 0, gn(civ_side, tgt), -1)
        t_vis = mx.where(tgt >= 0, gn(vis.astype(I32), tgt) > 0, False)
        enemy = 1 - side
        foreign_city = (t_city >= 0) & (t_city_owner != side[:, None]) & (t_city_owner >= 0)
        enemy_unit_seen = t_vis & ((t_mil_side == enemy[:, None]) | (t_civ_side == enemy[:, None]))
        own_there = (t_mil_side == side[:, None]) | (t_civ_side == side[:, None])
        target_ok = foreign_city | (enemy_unit_seen & ~own_there)
        water_t = mx.where(tgt >= 0, gn(self.m["water"].astype(I32), tgt) > 0, False)
        water_me = gn(self.m["water"].astype(I32), tile[:, None])[:, 0] > 0
        embarked = g1(s["p_land"], slot) & water_me
        # Melee.
        melee_ok = ~ranged & ~g1(s["p_only_defensive"], slot) & (base_c > 0)
        m_ok = melee_ok[:, None] & (d == 1)
        m_ok = m_ok & mx.where(g1(s["p_city_attack_only"], slot)[:, None], t_city >= 0, True)
        land = g1(s["p_land"], slot)
        m_ok = m_ok & mx.where(land[:, None], ~water_t | embarked[:, None], water_t | (t_city >= 0))
        # Ranged.
        r_ok = ranged & (base_r > 0) & ((rng > 0) | g1(s["p_ranged_support_fire"], slot)) & ~embarked
        st = g1(s["u_st"], slot)
        r_ok = r_ok & mx.where(g1(s["p_must_set_up"], slot), st == 3, True)
        reach = mx.maximum(rng, 1)
        rr = r_ok[:, None] & (d >= 1) & (d <= reach[:, None])
        rr = rr & (t_vis | foreign_city & t_vis)
        los = self.los_bit(mx.broadcast_to(tile[:, None], tgt.shape), tgt)
        rr = rr & (g1(s["p_indirect_fire"], slot)[:, None] | los)
        rr = rr & mx.where(g1(s["p_ranged_only_in_domain"], slot)[:, None], water_t & (t_city < 0), True)
        ok = ready[:, None] & (tgt >= 0) & target_ok & (m_ok | rr)
        return ok, tgt

    def city_mask(self, s, side, cslot):
        """Targets of city `cslot`'s strike [B, T] (`plan_city_strike`)."""
        B, T = self.B, self.T
        geo = self.geo
        ct = g1(self.cs["tile"], cslot)
        owner = g1(s["c_owner"], cslot)
        mine = g1(self.cs["alive"], cslot) & (owner == side) & (ct >= 0) & ~s["done"]
        maxhp = g1(self.cs["max_hp"], cslot)
        ready = mine & ~g1(s["c_strike"], cslot) & (g1(s["c_dmg"], cslot) < maxhp)
        block, ctrl, vis, mil, civ, mil_side, civ_side, city, city_owner = self.controls_and_blocks(s, side)
        tgt = mx.take(geo.disc_tile, mx.maximum(ct, 0), axis=0)
        d = mx.broadcast_to(geo.off_dist[None], tgt.shape)
        enemy = 1 - side
        t_vis = mx.where(tgt >= 0, gn(vis.astype(I32), tgt) > 0, False)
        t_unit = (mx.where(tgt >= 0, gn(mil_side, tgt), -1) == enemy[:, None]) | (mx.where(tgt >= 0, gn(civ_side, tgt), -1) == enemy[:, None])
        ok = ready[:, None] & (tgt >= 0) & (d >= 1) & (d <= CITY_RANGE) & t_vis & t_unit
        base = (mx.arange(B) * T)[:, None]
        return mx.zeros((B * T + 1,), dtype=I32).at[mx.where(ok, base + tgt, B * T).reshape(-1)].add(1)[: B * T].reshape(B, T) > 0

    # ------------------------------------------------------------------ strengths

    def fighter(self, s, slot, valid):
        f = {"slot": slot, "valid": valid & (slot >= 0)}
        for k in ("u_tile", "u_side", "u_hp", "u_st"):
            f[k[2:]] = g1(s[k], slot)
        for k in ("base_combat", "base_ranged", "strength", "ranged_strength", "role", "domain", "class", "wounded_mod", "embarked_defense",
                  "civilian", "land", "sea", "is_ranged", "no_defensive_bonus", "ranged_support_fire", "heavy_charge", "river_crossing",
                  "amphibious", "general_aura", "withdraw", "xp_percent", "heal_on_kill", "sapper"):
            f[k] = g1(s["p_" + k], slot)
        f["ctx"] = mx.take_along_axis(s["p_ctx"], mx.maximum(slot, 0)[:, None, None], axis=1)[:, 0]
        for k in ("tctx", "ttype", "targ", "targ2", "tpct"):
            f[k] = mx.take_along_axis(s["p_" + k], mx.maximum(slot, 0)[:, None, None], axis=1)[:, 0]
        f["water"] = gn(self.m["water"].astype(I32), f["tile"][:, None])[:, 0] > 0
        f["embarked"] = f["land"] & f["water"]
        return f

    def context_sum(self, s, f, ctx, battle, other):
        """Σ of the unit's percents in the holding contexts, situational terms included (`context_sum`)."""
        total = mx.sum(mx.where(ctx, f["ctx"], 0), axis=1)
        in_ctx = mx.take_along_axis(ctx, f["tctx"], axis=1)
        tt, ta = f["ttype"], f["targ"]
        feat = gn(self.m["feature"], battle[:, None])[:, 0]
        tkind = gn(self.m["terrain_kind"], battle[:, None])[:, 0]
        ov = other["valid"] if other is not None else mx.zeros_like(f["valid"])
        orole = other["role"] if other is not None else mx.zeros_like(f["slot"])
        odom = other["domain"] if other is not None else mx.zeros_like(f["slot"])
        ocls = other["class"] if other is not None else mx.zeros_like(f["slot"])
        holds = (tt == 1) & (feat[:, None] == ta)
        holds = holds | ((tt == 2) & (feat[:, None] < 0) & (tkind[:, None] == ta))
        holds = holds | ((tt == 3) & ov[:, None] & (orole[:, None] == ta))
        holds = holds | ((tt == 4) & ov[:, None] & (odom[:, None] == ta))
        holds = holds | ((tt == 5) & ov[:, None] & (ocls[:, None] == ta) & (ta > 0))
        holds = holds | (tt == 6)
        if self.has_near_imp:
            holds = holds | ((tt == 7) & self.near_improvement(s, f))
        return total + mx.sum(mx.where(in_ctx & holds, f["tpct"], 0), axis=1)

    def near_improvement(self, s, f):
        geo = self.geo
        disc = mx.take(geo.disc_tile, mx.maximum(f["tile"], 0), axis=0)  # [B, O]
        imp = mx.where(disc >= 0, gn(self.m["improvement"], disc), -1)
        own = mx.where(disc >= 0, gn(s["owner"], disc), -1) == f["side"][:, None]
        d = geo.off_dist[None]
        hit = (imp[:, None, :] == f["targ"][:, :, None]) & own[:, None, :] & (d[:, None, :] <= f["targ2"][:, :, None])
        return mx.any(hit, axis=2)

    def generic(self, s, f, other, adjacency):
        """M_generic: great generals, enemy auras, flanking (melee attack only)."""
        B = self.B
        U = s["u_alive"].shape[1]
        tiles = s["u_tile"]
        slots = mx.arange(U)[None, :]
        d_me = self.geo.distance(tiles, mx.broadcast_to(f["tile"][:, None], tiles.shape))
        alive = s["u_alive"] & (tiles >= 0)
        own = alive & (s["u_side"] == f["side"][:, None]) & (slots != f["slot"][:, None])
        gg = own & s["p_great_general"] & (d_me <= 2)
        near = mx.any(gg, axis=1)
        stacked = mx.max(mx.where(gg & (d_me == 0), s["p_stacked_general"], 0), axis=1)
        m = mx.where(near & f["land"], 15 + stacked + f["general_aura"], 0)
        enemy = alive & (s["u_side"] != f["side"][:, None])
        aura = enemy & (s["p_enemy_aura"] != 0) & (d_me <= 2) & (d_me <= s["p_aura_range"])
        m = m + mx.sum(mx.where(aura, s["p_enemy_aura"], 0), axis=1)
        if other is not None and adjacency:
            d_o = self.geo.distance(tiles, mx.broadcast_to(other["tile"][:, None], tiles.shape))
            flank = own & ~s["p_civilian"] & (s["p_strength"] > 0) & (s["p_ranged_strength"] == 0) & (d_o == 1)
            count = mx.sum(flank.astype(I32), axis=1)
            fl = 10 * count
            raise_ = f["ctx"][:, CTX_FLANKING]
            fl = mx.where(raise_ > 0, fl * (100 + raise_) // 100, fl)
            m = m + mx.where(other["valid"], fl, 0)
        return m

    def friendly_adjacent(self, s, f):
        U = s["u_alive"].shape[1]
        tiles = s["u_tile"]
        d = self.geo.distance(tiles, mx.broadcast_to(f["tile"][:, None], tiles.shape))
        water = gn(self.m["water"].astype(I32), tiles) > 0
        own = s["u_alive"] & (tiles >= 0) & (s["u_side"] == f["side"][:, None]) & ~s["p_civilian"] & (mx.arange(U)[None, :] != f["slot"][:, None])
        return mx.any(own & (d == 1) & (water == f["water"][:, None]), axis=1)

    def ctxset(self, cols):
        B = self.B
        out = [mx.zeros((B,), dtype=mx.bool_)] * N_CTX
        for i, v in cols.items():
            out[i] = v if v.ndim else mx.broadcast_to(v, (B,))
        return mx.stack(out, axis=1)

    def tile_defense(self, s, tile, side):
        gdef = gn(self.m["ground_def"], tile[:, None])[:, 0]
        idef = gn(self.m["improvement_def"], tile[:, None])[:, 0]
        own = gn(s["owner"], tile[:, None])[:, 0]
        return gdef + mx.where((own < 0) | (own == side), idef, 0)

    def finish(self, base, m):
        return mx.maximum(base.astype(I32) * (100 + m), 100)

    def near_sapper(self, s, side, ctile):
        tiles = s["u_tile"]
        d = self.geo.distance(tiles, mx.broadcast_to(ctile[:, None], tiles.shape))
        return mx.any(s["u_alive"] & (tiles >= 0) & (s["u_side"] == side[:, None]) & s["p_sapper"] & (d <= 2), axis=1)

    def melee_attack_strength(self, s, a, d, to, vs_city):
        tr = True
        friendly = gn(s["owner"], to[:, None])[:, 0] == a["side"]
        rough = gn(self.m["rough"].astype(I32), to[:, None])[:, 0] > 0
        dv = d["valid"] if d is not None else mx.zeros_like(a["valid"])
        cols = {ANY: mx.array(tr), ATTACK: mx.array(tr), VS_CITY: vs_city, ATT_OPEN: ~vs_city & ~rough, ATT_ROUGH: ~vs_city & rough,
                FRIENDLY: friendly, ATT_FRIENDLY: friendly, OUTSIDE_FRIENDLY: ~friendly,
                VS_WOUNDED: dv & (d["hp"] < 100) if d is not None else dv, VS_FORTIFIED: dv & ((d["st"] == 1) | (d["st"] == 2)) if d is not None else dv,
                ADJ_FRIENDLY: self.friendly_adjacent(s, a)}
        ctx = self.ctxset(cols)
        m = self.context_sum(s, a, ctx, to, d if d is not None else None) + self.generic(s, a, d, True)
        cities_tile = to
        m = m + mx.where(vs_city & self.near_sapper(s, a["side"], cities_tile), 50, 0)
        dirs = self.dir_between(a["tile"], to)
        river = mx.take_along_axis(self.m["river"].reshape(self.B, -1), (mx.maximum(a["tile"], 0) * 6 + mx.maximum(dirs, 0))[:, None], axis=1)[:, 0] & (dirs >= 0)
        m = m + mx.where(river & ~(a["amphibious"] | a["river_crossing"]), -20, 0)
        water_to = gn(self.m["water"].astype(I32), to[:, None])[:, 0] > 0
        m = m + mx.where(a["land"] & a["water"] & ~water_to & ~a["amphibious"], -50, 0)
        strength = self.finish(a["base_combat"], m)
        if d is not None:
            heavy = a["heavy_charge"] & dv & (d["withdraw"] <= 0)
            strength = mx.where(heavy, strength * 150 // 100, strength)
        return strength

    def dir_between(self, a, b):
        nb = mx.take(self.geo.nbr, mx.maximum(a, 0), axis=0)  # [B, 6]
        hit = nb == b[:, None]
        return mx.max(mx.where(hit, mx.arange(6)[None, :], -1), axis=1)

    def defense_strength(self, s, d, a, ranged):
        tile = d["tile"]
        rough = gn(self.m["rough"].astype(I32), tile[:, None])[:, 0] > 0
        friendly = gn(s["owner"], tile[:, None])[:, 0] == d["side"]
        cols = {ANY: mx.array(True), DEFENSE: mx.array(True), DEF_VS_RANGED: mx.array(ranged), DEF_OPEN: ~rough, DEF_ROUGH: rough,
                FRIENDLY: friendly, OUTSIDE_FRIENDLY: ~friendly}
        if not ranged:
            cols[ADJ_FRIENDLY] = self.friendly_adjacent(s, d)
        ctx = self.ctxset(cols)
        m = self.context_sum(s, d, ctx, tile, a) + self.generic(s, d, a, False)
        terrain = mx.where(d["water"], 0, self.tile_defense(s, tile, d["side"]))
        m = m + mx.where(~d["no_defensive_bonus"] | (terrain < 0), terrain, 0)
        fort = mx.where((d["st"] == 1) | (d["st"] == 2), d["st"], 0)
        m = m + mx.where(~d["ranged_support_fire"], fort * 20, 0)
        return mx.where(d["embarked"], d["embarked_defense"], self.finish(d["base_combat"], m))

    def ranged_strength(self, s, me, other, vs_city, target, attacking):
        rough = gn(self.m["rough"].astype(I32), target[:, None])[:, 0] > 0
        friendly = gn(s["owner"], target[:, None])[:, 0] == me["side"]
        ov = other["valid"] if other is not None else mx.zeros_like(me["valid"])
        unit_target = ~vs_city if attacking else mx.zeros_like(vs_city)
        cols = {ANY: mx.array(True), VS_CITY: vs_city}
        if attacking:
            cols.update({ATTACK: mx.array(True), RANGED_ATTACK: mx.array(True), FRIENDLY: friendly, OUTSIDE_FRIENDLY: ~friendly,
                         RATT_OPEN: unit_target & ~rough, RATT_ROUGH: unit_target & rough})
        else:
            cols.update({DEFENSE: mx.array(True), DEF_VS_RANGED: mx.array(True)})
        if other is not None:
            cols[VS_WOUNDED] = ov & (other["hp"] < 100)
            cols[VS_FORTIFIED] = ov & ((other["st"] == 1) | (other["st"] == 2))
        ctx = self.ctxset(cols)
        m = self.context_sum(s, me, ctx, target, other) + self.generic(s, me, other, False)
        m = m + mx.where(vs_city & self.near_sapper(s, me["side"], target), 50, 0)
        if not attacking:
            terrain = self.tile_defense(s, me["tile"], me["side"])
            m = m + mx.where(~me["no_defensive_bonus"] | (terrain < 0), terrain, 0)
        return self.finish(me["base_ranged"], m)

    def defense_vs_ranged(self, s, d, a):
        ranged_land = ~d["ranged_support_fire"] & ~d["sea"] & (d["base_ranged"] > 0)
        rs = self.ranged_strength(s, d, a, mx.zeros_like(d["valid"]), d["tile"], False)
        ds = self.defense_strength(s, d, a, True)
        return mx.where(d["embarked"], d["embarked_defense"], mx.where(ranged_land, rs, ds))

    def city_strength(self, s, cslot, mil):
        ct = g1(self.cs["tile"], cslot)
        owner = g1(s["c_owner"], cslot)
        base = g1(self.cs["str0"], cslot) + (g1(s["c_pop"], cslot) - g1(self.cs["pop0"], cslot)) * 40
        gslot = gn(mil, ct[:, None])[:, 0]
        has_g = (gslot >= 0) & (ct >= 0)
        g_side = g1(s["u_side"], gslot)
        g_land = g1(s["p_land"], gslot)
        g_str = g1(s["p_strength"], gslot)
        garrison = has_g & (g_side == owner) & g_land & (g_str > 0)
        g = g_str * 100 * g1(s["u_hp"], gslot) // 100
        st = base + mx.where(garrison, g * 100 // 500, 0)
        st = mx.maximum(st, 1)
        rs = st * 75 // 100
        gsp = g1(self.cs["gsp"], cslot)
        rs = mx.where(garrison & (gsp != 0), rs * (100 + gsp) // 100, rs)
        rsp = g1(self.cs["rsp"], cslot)
        rs = mx.where(rsp > 0, rs * (100 + rsp) // 100, rs)
        return st, mx.maximum(rs, 1)

    # ------------------------------------------------------------------ applying actions

    def set_unit(self, s, slot, where, **vals):
        for k, v in vals.items():
            s["u_" + k] = put1(s["u_" + k], slot, v if isinstance(v, mx.array) else mx.full(slot.shape, v, dtype=s["u_" + k].dtype), where)
        return s

    def xp_gain(self, base, pct):
        return mx.where(pct != 0, base * mx.maximum(100 + pct, 0) // 100, base)

    def strike_unit(self, s, slot, dmg, where):
        """Damage (or destroy) unit `slot`; returns (state, killed)."""
        hp = g1(s["u_hp"], slot)
        killed = where & (dmg >= hp) & (dmg > 0)
        hurt = where & (dmg > 0) & ~killed
        s = self.set_unit(s, slot, hurt, hp=hp - dmg)
        s = self.set_unit(s, slot, killed, alive=mx.zeros(slot.shape, dtype=mx.bool_), tile=mx.full(slot.shape, -1, dtype=I32))
        return s, killed

    def finish_attack(self, s, slot, where, stopped):
        st = g1(s["u_st"], slot)
        mv = g1(s["u_mv"], slot)
        att = g1(s["u_att"], slot) + 1
        mv = mx.where(stopped, mx.maximum(mv - MOVE, 0), mv)
        allowed = 1 + mx.clip(g1(s["p_attacks_per_turn"], slot), 0, 9)
        mv = mx.where(~g1(s["p_move_after_attacking"], slot) & (att >= allowed), 0, mv)
        return self.set_unit(s, slot, where, st=mx.where(st == 3, 3, 0), mv=mv, att=att)

    def refresh_allowance(self, s, slot, old_tile, new_tile, where):
        wo = gn(self.m["water"].astype(I32), old_tile[:, None])[:, 0] > 0
        wn = gn(self.m["water"].astype(I32), new_tile[:, None])[:, 0] > 0
        change = where & (wo != wn) & g1(s["p_land"], slot)
        mmax = mx.where(wn & g1(s["p_embarks"], slot), g1(s["p_moves_embarked"], slot), g1(s["p_moves_land"], slot))
        mv = mx.minimum(g1(s["u_mv"], slot), mmax)
        return self.set_unit(s, slot, change, mmax=mmax, mv=mv)

    def advance(self, s, slot, to, where, mil=None):
        """The attacker moves into the conquered tile, paying its own step (`advance`)."""
        frm = g1(s["u_tile"], slot)
        d = self.dir_between(frm, to)
        cid = g1(s["p_cost_id"], slot)
        code = mx.take(self.cost_flat, cid * (self.T * 6) + mx.maximum(frm, 0) * 6 + mx.maximum(d, 0))
        mv = g1(s["u_mv"], slot)
        cost = mx.where((code >= 0) & (code != 255), mx.minimum(code, mv), mv)
        # can_stop_at: no other military unit of ours there (a civilian may share).
        # After a won fight the tile holds no other military unit (its defender is gone): no check.
        ok = where if mil is None else where & ((gn(mil, to[:, None])[:, 0] < 0) | (gn(mil, to[:, None])[:, 0] == slot))
        s = self.set_unit(s, slot, ok, tile=to, mv=mv - cost, st=mx.zeros_like(slot))
        return self.refresh_allowance(s, slot, frm, to, ok)

    def capture_civilians(self, s, captor, tile, where, civ):
        victim = gn(civ, tile[:, None])[:, 0]
        vside = g1(s["u_side"], victim)
        csid = g1(s["u_side"], captor)
        hit = where & (victim >= 0) & (vside != csid)
        kind = g1(s["p_capture_kind"], victim)
        moves = g1(s["p_capture_moves"], victim)
        s = self.set_unit(s, victim, hit, alive=mx.zeros(victim.shape, dtype=mx.bool_), tile=mx.full(victim.shape, -1, dtype=I32))
        spawn = hit & (kind >= 0) & ~g1(s["p_no_capture"], captor)
        U = s["u_alive"].shape[1]
        new = s["n_slots"]
        spawn = spawn & (new < U)
        s = self.set_unit(s, new, spawn, alive=mx.ones(new.shape, dtype=mx.bool_), side=csid, tile=tile, hp=mx.full(new.shape, 100, dtype=I32),
                          mv=mx.zeros_like(new), mmax=moves, att=mx.zeros_like(new), st=mx.zeros_like(new), xp=mx.zeros_like(new), kind=kind)
        s["n_slots"] = mx.where(spawn, new + 1, new)
        return s

    def apply_unit(self, s, side, slot, verb, tgt, mk, take):
        """One unit action in every scene where `take`."""
        B = self.B
        alive = g1(s["u_alive"], slot)
        take = take & alive
        tile = g1(s["u_tile"], slot)
        # Fortify, set up.
        s = self.set_unit(s, slot, take & (verb == V_FORT), st=mx.ones_like(slot))
        mv = g1(s["u_mv"], slot)
        s = self.set_unit(s, slot, take & (verb == V_SETUP), st=mx.full(slot.shape, 3, dtype=I32), mv=mx.maximum(mv - MOVE, 0))
        # Found: the scene ends (the convoy's win), the settler is spent.
        fnd = take & (verb == V_FOUND)
        s = self.set_unit(s, slot, fnd, alive=mx.zeros(slot.shape, dtype=mx.bool_), tile=mx.full(slot.shape, -1, dtype=I32))
        s["done"] = s["done"] | fnd
        # Move.
        mvv = take & (verb == V_MOVE)
        lt, left = mk["lt"], mk["left"]
        hit = lt == tgt[:, None]
        left_t = mx.max(mx.where(hit, left, -1), axis=1)
        mvv = mvv & (left_t >= 0)
        end_tile, end_left = self.walk(s, slot, mk, tgt, mk["foreign_l"])
        s = self.set_unit(s, slot, mvv, tile=end_tile, mv=mx.maximum(end_left, 0), st=mx.zeros_like(slot))
        s = self.refresh_allowance(s, slot, tile, end_tile, mvv)
        # Attack.
        s = self.attack(s, side, slot, tgt, take & (verb == V_ATTACK))
        return s

    def attack(self, s, side, slot, to, take):
        B = self.B
        mil, civ = self.occupancy(s)
        a = self.fighter(s, slot, take)
        cslot = self.city_on(s, to[:, None])[:, 0]
        is_city = take & (cslot >= 0)
        dslot = gn(mil, to[:, None])[:, 0]
        cvslot = gn(civ, to[:, None])[:, 0]
        is_unit = take & ~is_city & (dslot >= 0)
        is_civ = take & ~is_city & (dslot < 0) & (cvslot >= 0)
        ranged = a["is_ranged"]
        d = self.fighter(s, mx.where(is_unit, dslot, cvslot), is_unit)
        c_str, c_rstr = self.city_strength(s, cslot, mil)
        c_maxhp = g1(self.cs["max_hp"], cslot)
        c_hp = c_maxhp - g1(s["c_dmg"], cslot)
        a_dmg_taken = 100 - a["hp"]
        # Strengths.
        att_melee_unit = self.melee_attack_strength(s, a, d, to, mx.zeros_like(take))
        att_melee_city = self.melee_attack_strength(s, a, None, to, mx.ones_like(take))
        def_melee = self.defense_strength(s, d, a, False)
        att_ranged_unit = self.ranged_strength(s, a, d, mx.zeros_like(take), to, True)
        att_ranged_city = self.ranged_strength(s, a, None, mx.ones_like(take), to, True)
        def_ranged = self.defense_vs_ranged(s, d, a)
        # Rolls: melee (unit or city) draws two, ranged one, melee on civilians none.
        melee_unit = is_unit & ~ranged
        melee_city = is_city & ~ranged
        ranged_any = take & ranged
        two = melee_unit | melee_city
        rng, r1 = exact.draw(s["rng"], mx.full((B,), 1200, dtype=mx.int64), two | ranged_any)
        rng, r2 = exact.draw(rng, mx.full((B,), 1200, dtype=mx.int64), two)
        s["rng"] = rng
        wm = a["wounded_mod"]
        # --- melee unit vs unit
        to_def = exact.unit_damage(att_melee_unit, def_melee, a_dmg_taken, wm, r1).astype(I32)
        to_att = exact.unit_damage(def_melee, att_melee_unit, 100 - d["hp"], d["wounded_mod"], r2).astype(I32)
        to_def, to_att = exact.resolve_mutual_kill(to_def, d["hp"], to_att, a["hp"])
        s, d_killed = self.strike_unit(s, d["slot"], to_def, melee_unit)
        s, a_killed = self.strike_unit(s, slot, to_att, melee_unit)
        a_xp = g1(s["u_xp"], slot)
        s = self.set_unit(s, slot, melee_unit & ~a_killed, xp=a_xp + self.xp_gain(XP_ATT_MELEE, a["xp_percent"]))
        d_xp = g1(s["u_xp"], d["slot"])
        s = self.set_unit(s, d["slot"], melee_unit & ~d_killed, xp=d_xp + self.xp_gain(XP_DEF_MELEE, d["xp_percent"]))
        # A kill heals the killer (HealOnKill).
        s = self.heal_on_kill(s, d["slot"], melee_unit & a_killed & ~d_killed, d["heal_on_kill"])
        s = self.heal_on_kill(s, slot, melee_unit & d_killed & ~a_killed, a["heal_on_kill"])
        win = melee_unit & d_killed & ~a_killed
        s = self.finish_attack(s, slot, melee_unit & ~d_killed & ~a_killed, mx.ones_like(win))
        # A won melee and a melee on civilians: capture the civilians there, advance, the attack ends.
        moves_in = win | (is_civ & ~ranged)
        s = self.capture_civilians(s, slot, to, moves_in, civ)
        s = self.advance(s, slot, to, moves_in)
        s = self.finish_attack(s, slot, moves_in, mx.zeros_like(win))
        # --- melee on a city
        to_city = mx.minimum(exact.unit_damage(att_melee_city, c_str, a_dmg_taken, wm, r1).astype(I32), c_hp)
        to_att_c = exact.city_damage(c_str, att_melee_city, r2).astype(I32)
        falls = to_city >= c_hp
        to_att_c = mx.where(falls & (to_att_c >= a["hp"]), a["hp"] - 1, to_att_c)
        s["c_dmg"] = put1(s["c_dmg"], cslot, g1(s["c_dmg"], cslot) + to_city, melee_city)
        s, ac_killed = self.strike_unit(s, slot, to_att_c, melee_city)
        mc = melee_city & ~ac_killed
        s = self.set_unit(s, slot, mc, xp=g1(s["u_xp"], slot) + self.xp_gain(XP_ATT_CITY_MELEE, a["xp_percent"]))
        no_cap = g1(s["p_no_capture"], slot)
        s = self.finish_attack(s, slot, mc & ~falls, mx.ones_like(win))
        s["c_dmg"] = put1(s["c_dmg"], cslot, mx.minimum(g1(s["c_dmg"], cslot), c_maxhp - 1), mc & falls & no_cap)
        s = self.finish_attack(s, slot, mc & falls & no_cap, mx.ones_like(win))
        cap = mc & falls & ~no_cap
        s = self.capture_city(s, slot, cslot, to, cap, mil)
        # --- ranged
        civ_target = ranged_any & is_civ
        unit_target = ranged_any & is_unit
        city_target = ranged_any & is_city
        r_unit = exact.unit_damage(att_ranged_unit, def_ranged, a_dmg_taken, wm, r1).astype(I32)
        r_unit = mx.where(civ_target, exact.CIVILIAN_DAMAGE, r_unit)
        r_unit = mx.minimum(r_unit, d["hp"])
        r_city = mx.minimum(exact.unit_damage(att_ranged_city, c_str, a_dmg_taken, wm, r1).astype(I32), mx.maximum(c_hp - 1, 0))
        s["c_dmg"] = put1(s["c_dmg"], cslot, g1(s["c_dmg"], cslot) + r_city, city_target)
        s, rk = self.strike_unit(s, d["slot"], r_unit, unit_target | civ_target)
        s = self.set_unit(s, d["slot"], unit_target & ~rk, xp=g1(s["u_xp"], d["slot"]) + self.xp_gain(XP_DEF_RANGED, d["xp_percent"]))
        s = self.heal_on_kill(s, slot, (unit_target | civ_target) & rk, a["heal_on_kill"])
        axp = mx.where(city_target, self.xp_gain(XP_ATT_CITY_RANGED, a["xp_percent"]), self.xp_gain(XP_ATT_RANGED, a["xp_percent"]))
        s = self.set_unit(s, slot, ranged_any, xp=g1(s["u_xp"], slot) + axp)
        s = self.finish_attack(s, slot, ranged_any, mx.ones_like(win))
        return s

    def heal_on_kill(self, s, slot, where, heal):
        hp = g1(s["u_hp"], slot)
        return self.set_unit(s, slot, where & (heal > 0), hp=mx.minimum(hp + heal, 100))

    def capture_city(self, s, slot, cslot, tile, where, mil):
        """The city changes hands (the scene's end): building survival draws, the garrison dies, the owner
        changes, damage at most half, the attacker moves in."""
        draws = g1(self.cs["draws"], cslot)
        # The capture gold's roll (`take_capture_gold`), then a survival roll per building that may fall.
        s["rng"], _ = exact.draw(s["rng"], mx.full((self.B,), 100, dtype=mx.int64), where)
        for k in range(8):
            s["rng"], _ = exact.draw(s["rng"], mx.full((self.B,), 100, dtype=mx.int64), where & (draws > k))
        owner = g1(s["c_owner"], cslot)
        U = s["u_alive"].shape[1]
        on = s["u_alive"] & (s["u_tile"] == tile[:, None]) & (s["u_side"] == owner[:, None]) & where[:, None]
        s["u_alive"] = s["u_alive"] & ~on
        s["u_tile"] = mx.where(on, -1, s["u_tile"])
        s["c_owner"] = put1(s["c_owner"], cslot, g1(s["u_side"], slot), where)
        # A side left without a city is eliminated: its units go.
        held = mx.take_along_axis(self.cs["side_cities"], mx.clip(owner, 0, 1)[:, None], axis=1)[:, 0]
        gone = where & (held <= 1) & (owner >= 0)
        out = s["u_alive"] & (s["u_side"] == owner[:, None]) & gone[:, None]
        s["u_alive"] = s["u_alive"] & ~out
        s["u_tile"] = mx.where(out, -1, s["u_tile"])
        maxhp = g1(self.cs["max_hp"], cslot)
        s["c_dmg"] = put1(s["c_dmg"], cslot, mx.minimum(g1(s["c_dmg"], cslot), maxhp * 50 // 100), where)
        s["c_strike"] = put1(s["c_strike"], cslot, mx.ones(cslot.shape, dtype=mx.bool_), where)
        s = self.advance(s, slot, tile, where)
        s = self.finish_attack(s, slot, where, mx.zeros_like(where))
        s["done"] = s["done"] | where
        return s

    def city_strike(self, s, side, cslot, tgt, take):
        mil, civ = self.occupancy(s)
        dslot = gn(mil, tgt[:, None])[:, 0]
        cv = gn(civ, tgt[:, None])[:, 0]
        is_unit = take & (dslot >= 0)
        is_civ = take & (dslot < 0) & (cv >= 0)
        d = self.fighter(s, mx.where(is_unit, dslot, cv), is_unit)
        _, c_rstr = self.city_strength(s, cslot, mil)
        strength = self.defense_vs_ranged(s, d, None)
        s["rng"], roll = exact.draw(s["rng"], mx.full((self.B,), 1200, dtype=mx.int64), take)
        dmg = exact.city_damage(c_rstr, strength, roll).astype(I32)
        dmg = mx.where(is_civ, exact.CIVILIAN_DAMAGE, dmg)
        dmg = mx.minimum(dmg, d["hp"])
        s["c_strike"] = put1(s["c_strike"], cslot, mx.ones(cslot.shape, dtype=mx.bool_), take)
        s, killed = self.strike_unit(s, d["slot"], dmg, is_unit | is_civ)
        s = self.set_unit(s, d["slot"], is_unit & ~killed, xp=g1(s["u_xp"], d["slot"]) + self.xp_gain(XP_DEF_RANGED, d["xp_percent"]))
        return s

    # ------------------------------------------------------------------ the turn

    def end_turn(self, s, side, take):
        """The end of `side`'s turn: heal its units, restore moves and attacks, its cities heal and may
        strike again; then the other side's turn starts (fortification +1)."""
        B, T = self.B, self.T
        U = s["u_alive"].shape[1]
        tiles = s["u_tile"]
        mine = s["u_alive"] & (tiles >= 0) & (s["u_side"] == side[:, None]) & take[:, None]
        water = gn(self.m["water"].astype(I32), tiles) > 0
        owner = gn(s["owner"], tiles)
        friendly = owner == s["u_side"]
        cityt = self.city_on(s, tiles) >= 0
        moved = (s["u_mv"] < s["u_mmax"]) & ~s["p_always_heal"]
        no = (s["u_hp"] >= 100) | (s["p_land"] & water) | moved | (s["p_sea"] & ~friendly & ~s["p_heal_outside_friendly"])
        enemy_host = (owner >= 0) & (owner != s["u_side"]) & (owner < 2)
        rate = mx.where(cityt, 25 + mx.where(friendly, s["p_heal_friendly"], s["p_heal_neutral"]),
                        mx.where(~friendly, mx.where(enemy_host, 10 + s["p_heal_enemy"], 10 + s["p_heal_neutral"]), 20 + s["p_heal_friendly"]))
        # Medic: the best HealAdjacent of an own unit next to the unit.
        dd = self.geo.distance(tiles[:, :, None], tiles[:, None, :])
        same = (s["u_side"][:, :, None] == s["u_side"][:, None, :]) & s["u_alive"][:, None, :] & (tiles[:, None, :] >= 0)
        medic = mx.max(mx.where(same & (dd == 1), s["p_heal_adjacent"][:, None, :], 0), axis=2)
        dose = mx.where(no, 0, rate + medic)
        s["u_hp"] = mx.where(mine, mx.clip(s["u_hp"] + dose, 1, 100), s["u_hp"])
        mmax = mx.where(s["p_embarks"] & water & s["p_land"], s["p_moves_embarked"], s["p_moves_land"])
        s["u_att"] = mx.where(mine, 0, s["u_att"])
        s["u_mmax"] = mx.where(mine, mmax, s["u_mmax"])
        s["u_mv"] = mx.where(mine, mmax, s["u_mv"])
        cmine = self.cs["alive"] & (s["c_owner"] == side[:, None]) & take[:, None]
        s["c_dmg"] = mx.where(cmine, mx.maximum(s["c_dmg"] - 20, 0), s["c_dmg"])
        s["c_strike"] = mx.where(cmine, False, s["c_strike"])
        nxt = s["u_alive"] & (s["u_side"] == (1 - side)[:, None]) & take[:, None] & ((s["u_st"] == 1) | (s["u_st"] == 2))
        s["u_st"] = mx.where(nxt, mx.minimum(s["u_st"] + 1, 2), s["u_st"])
        return s

    def round_end(self, s, take):
        """The closed arena: units that are not scene pieces leave."""
        U = s["u_alive"].shape[1]
        extra = (mx.arange(U)[None, :] >= self.n_pieces[:, None]) & take[:, None] & s["u_alive"]
        s["u_alive"] = s["u_alive"] & ~extra
        s["u_tile"] = mx.where(extra, -1, s["u_tile"])
        return s

    def step(self, s, a, mk=None, kinds=(A_UNIT, A_CITY, A_END, A_ROUND)):
        """Apply one action per scene: a = {typ, side, slot, verb, tgt} ([B] each). `kinds`: the action
        types that may occur in this call (a batch stepping in lockstep passes one: the other branches are
        then not computed at all)."""
        s = dict(s)
        typ, side, slot, verb, tgt = a["typ"], a["side"], a["slot"], a["verb"], a["tgt"]
        if A_UNIT in kinds:
            unit = (typ == A_UNIT) & ~s["done"]
            if mk is None:
                mk = self.masks(s, side, slot)
            if "lt" in mk:  # a step with no unit action passes {} and skips the unit's masks
                s = self.apply_unit(s, side, slot, verb, tgt, mk, unit & (verb != V_NONE))
        if A_CITY in kinds:
            s = self.city_strike(s, side, slot, tgt, (typ == A_CITY) & (verb == V_STRIKE) & ~s["done"])
        if A_END in kinds:
            s = self.end_turn(s, side, typ == A_END)
        if A_ROUND in kinds:
            s = self.round_end(s, typ == A_ROUND)
        return s
