"""The tactical sub-game, batched on the GPU (MLX), exact to the Rust rules.

One call `step(s, a)` advances every scene of the batch by one action of its own: a unit action
(nothing, fortify, set up, found, move, attack, promote), a city strike, a side's end of turn, or the
round end. Within a side's turn the units act one after another (`a["slot"]`), every scene in parallel;
there is no Python loop over units or tiles inside a step. `masks(s, side, slot)` gives the legal
options of the unit `slot` in every scene (the rules' acceptance: the same options `ai-exam suite-dump`
lists).

Sources (Rust): `classic-rules/src/combat/{strength,damage,exchange,terrain,city,experience}.rs`,
`classic-rules/src/systems/combat/{mod,air,city,heal,experience,actions}.rs`, `kernel/src/navigation/
{astar,cost}.rs`, `kernel/src/derived/visibility.rs`, `kernel/src/systems/{movement,turn,units}.rs`,
`kernel/src/world/stacking.rs`.

The data-oriented split: what is static in a scene comes compiled from the Rust rules (per unit: the
combat-context percents, situational terms, flags, stats, a step-cost table per tile and direction,
where it may stand and withdraw, the change each promotion makes; per tile: terrain planes,
line-of-sight masks, the citadel's damage; per city: the strength without the garrison); the dynamics
(positions, hit points, moves, stances, promotions, zones of control, sight, reach, every combat
formula, withdrawal, captures, air strikes and interception, the RNG stream, healing, the turn order)
run here.

State `s` (dict of arrays, B scenes, U unit slots, C cities, T window tiles, NP promotions):
u_alive u_side u_tile u_hp u_mv u_mmax u_att u_st (0 ready, 1-2 fortified turns, 3 set up, 4 intercept)
u_xp u_kind u_level u_prom[B,U,NP] u_icpt u_mset, p_* (the slot's profile: promotions change it),
c_owner c_dmg c_strike c_pop, owner[B,T], rng[B] (uint64), n_slots, done, n_unmodelled.
"""

import mlx.core as mx

from . import exact
from .scene import N_CTX, CTX_FLANKING, CTX_INTERCEPTION, FLAG_DELTA, STAT_NAMES, DELTA_INT, MAX_MOVING

I32 = mx.int32
MOVE = 60
# Contexts (kernel `combat_slot`).
ANY, ATTACK, DEFENSE, VS_CITY, ATT_OPEN, ATT_ROUGH, DEF_OPEN, DEF_ROUGH, RATT_OPEN, RATT_ROUGH, DEF_VS_RANGED, RANGED_ATTACK, \
    VS_WOUNDED, VS_FORTIFIED, FRIENDLY, OUTSIDE_FRIENDLY, ATT_FRIENDLY, FLANKING, ADJ_FRIENDLY = range(19)
XP_ATT_MELEE, XP_DEF_MELEE, XP_ATT_CITY_MELEE, XP_ATT_RANGED, XP_DEF_RANGED, XP_ATT_CITY_RANGED = 5, 4, 5, 2, 2, 3
XP_ATT_AIR, XP_DEF_AIR, XP_ATT_CITY_AIR, XP_INTERCEPT = 4, 2, 4, 2
# Verbs and action types.
V_NONE, V_FORT, V_SETUP, V_FOUND, V_MOVE, V_ATTACK, V_STRIKE, V_PROMOTE = range(8)
A_PAD, A_UNIT, A_CITY, A_END, A_ROUND = range(5)
CITY_RANGE = 2
CITY_SIGHT = 3
INSTANT_HEAL = 50
CAPTURED_UNIT_HP = 50
MOUNTAIN_DAMAGE = 50
ST_INTERCEPT = 4
# Stack classes (kernel `StackClass`): one unit of a class per tile, air apart.
CL_MIL, CL_CIV, CL_EMB, CL_NAVAL, CL_NAVAL_CIV, CL_AIR = range(6)


def g1(arr, idx):
    """arr[b, idx[b]] for arr [B, N] (idx clamped; callers mask idx < 0)."""
    return mx.take_along_axis(arr, mx.maximum(idx, 0)[:, None], axis=1)[:, 0]


def gn(arr, idx):
    """arr[b, idx[b, ...]] for arr [B, N] and idx [B, ...]."""
    B = arr.shape[0]
    flat = mx.maximum(idx, 0).reshape(B, -1)
    return mx.take_along_axis(arr, flat, axis=1).reshape(idx.shape)


def grow(arr, slot):
    """arr[b, slot[b], ...] for arr [B, U, ...]: the slot's row."""
    shape = (arr.shape[0], 1) + arr.shape[2:]
    idx = mx.broadcast_to(mx.maximum(slot, 0).reshape((-1,) + (1,) * (arr.ndim - 1)), shape)
    return mx.take_along_axis(arr, idx, axis=1)[:, 0]


def g2(arr, slot, j):
    """arr[b, slot[b], j[b], ...] for arr [B, U, N, ...] (one gather over the flattened U x N axis)."""
    B, U, N = arr.shape[:3]
    flat = arr.reshape((B, U * N) + arr.shape[3:])
    return grow(flat, mx.maximum(slot, 0) * N + mx.maximum(j, 0))


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
    """arr[b, idx[b]] = val[b] where `where`[b] (arr [B, N] or [B, N, ...] with val [B, ...])."""
    N = arr.shape[1]
    hot = (mx.arange(N)[None, :] == idx[:, None]) & where[:, None]
    if arr.ndim > 2:
        hot = hot.reshape(hot.shape + (1,) * (arr.ndim - 2))
        return mx.where(hot, val[:, None], arr)
    return mx.where(hot, val[:, None] if val.ndim == 1 else val, arr)


class Sim:
    def __init__(self, geo, m, cost_pool, cstatic, n_pieces, move_radius=None, has_near_imp=True, los=None, extra=None,
                 dest_pool=None, withdraw_pool=None, has_air=False, features=None, tid=None, fast_damage=False):
        self.has_near_imp = has_near_imp
        self.fast = fast_damage
        # The rules the batch can meet (`Batch.features`); the others stay out of the compiled graph.
        self.f = {"promo": True, "withdraw": True, "capture_defeated": True, "spawn": True, "citadel": True, "mountains": True}
        self.f.update(features or {})
        self.has_air = has_air
        # Parity runs count the rare events (`parity.run`): totals by name, None otherwise.
        self.events = None
        self.geo = geo
        self.m = m
        self.cost_pool = cost_pool
        self.cs = cstatic
        self.n_pieces = n_pieces
        self.z = extra or {}
        # The scene templates: the static tables (tiles `m`, promotions and spawns `z`, line of sight,
        # the city plane) have one row per template; `tid` [B] maps each scene to its template (a
        # throughput batch replicates templates without copying their tables).
        Bt, self.T = m["hills"].shape
        self.tid = mx.arange(Bt).astype(I32) if tid is None else tid.astype(I32)
        self.B = self.tid.shape[0]
        self.dest_pool = dest_pool
        self.withdraw_pool = withdraw_pool
        self.iters = move_radius or geo.move_radius
        self._pack()
        g = geo
        self.terr_rough = m["rough"]
        self.cost_flat = cost_pool.reshape(-1)
        ct, ca = cstatic.get("tile_t", cstatic["tile"]), cstatic.get("alive_t", cstatic["alive"])
        Bc, C = ct.shape
        idx = mx.where(ca & (ct >= 0), (mx.arange(Bc) * self.T)[:, None] + ct, Bc * self.T)
        self.city_plane = mx.full((Bc * self.T + 1,), -1, dtype=I32).at[idx.reshape(-1)].maximum(
            mx.broadcast_to(mx.arange(C)[None, :], (Bc, C)).astype(I32).reshape(-1))[: Bc * self.T].reshape(Bc, self.T)
        if los is not None:
            self.los = los
            return
        # Per scene: line-of-sight bits per (tile, disc offset), W uint64 words per tile ([B, T * W]).
        lvl = m["sight_level"]
        eye = m["hills"].astype(I32)
        B, T, W = Bt, self.T, g.words
        words = []
        chunk = max(1, 4096 // max(1, g.o))
        for t0 in range(0, T, chunk):
            mid = g.disc_mid[t0: t0 + chunk]  # [tc, O, K]
            lv = mx.where(mid[None] >= 0, gn(lvl, mx.broadcast_to(mid[None], (B,) + mid.shape)), 0)
            blocked = (lv > eye[:, t0: t0 + chunk, None, None]) & (mid[None] >= 0)
            ok = ~mx.any(blocked, axis=3)  # [B, tc, O]
            ws = []
            for w in range(W):
                o0, o1 = 64 * w, min(64 * (w + 1), g.o)
                bitv = mx.left_shift(mx.array(1, dtype=mx.uint64), mx.arange(o1 - o0).astype(mx.uint64))
                ws.append(mx.sum(mx.where(ok[:, :, o0:o1], bitv[None, None, :], mx.array(0, dtype=mx.uint64)), axis=2))
            words.append(mx.stack(ws, axis=2))
            mx.eval(words[-1])
        self.los = mx.concatenate(words, axis=1).reshape(B, T * W)

    def tg(self, arr, *idx):
        """arr[tid[b], idx0[b], idx1[b], ...] of a template table (one gather over the flattened axes)."""
        flat = self.tid
        for k, i in enumerate(idx):
            flat = flat * arr.shape[k + 1] + mx.maximum(i, 0)
        rest = arr.shape[1 + len(idx):]
        return mx.take(arr.reshape((-1,) + rest), flat, axis=0)

    def mg(self, name, idx):
        """The tile plane `name` at window tiles `idx` [B, ...] of each scene's template."""
        tid = self.tid.reshape((-1,) + (1,) * (idx.ndim - 1))
        return mx.take(self.m[name].reshape(-1), tid * self.T + mx.maximum(idx, 0))

    def mplane(self, name):
        """The whole plane `name` per scene [B, T] (materialised: for the few whole-plane reads)."""
        return mx.take(self.m[name], self.tid, axis=0)

    def lg(self, idx):
        """The line-of-sight words at flat indices `idx` [B, ...] (tile * W + word)."""
        tid = self.tid.reshape((-1,) + (1,) * (idx.ndim - 1))
        return mx.take(self.los.reshape(-1), tid * self.los.shape[1] + mx.maximum(idx, 0))

    # Profile fields a promotion changes (ints added, bools or-ed) and that a spawn writes.
    D_INT = list(DELTA_INT.values()) + STAT_NAMES

    def _pack(self):
        """The promotion changes and the spawn profiles packed into a few tables, so that taking a
        promotion or creating a unit is one gather (not one per field)."""
        z = self.z
        if "z_d_ctx" in z:
            self.pz_int = mx.concatenate([z["z_d_ctx"]] + [z["z_d_" + k][..., None] for k in self.D_INT], axis=-1)  # [B, U, NP, 23 + NI]
            self.pz_bool = mx.stack([z["z_f_" + k] for k in FLAG_DELTA], axis=-1)  # [B, U, NP, NF]
        sp = sorted(k for k in z if k.startswith("sp_") and k not in ("sp_prom", "sp_level"))
        self.sp_keys = {"int": [], "bool": [], "vec": []}
        ints, bools, vecs = [], [], []
        for k in sp:
            v = z[k]
            if v.ndim == 2:
                (bools if v.dtype == mx.bool_ else ints).append(v)
                self.sp_keys["bool" if v.dtype == mx.bool_ else "int"].append(k[3:])
            else:
                vecs.append(v)
                self.sp_keys["vec"].append(k[3:])
        if ints:
            self.sp_int = mx.stack(ints, axis=-1)  # [B, NS, n]
            self.sp_bool = mx.stack(bools, axis=-1) if bools else None
            self.sp_vec = [z["sp_" + k] for k in self.sp_keys["vec"]]
            mx.eval(self.sp_int, *(x for x in [self.sp_bool] if x is not None))
        if "z_d_ctx" in z:
            mx.eval(self.pz_int, self.pz_bool)

    # ------------------------------------------------------------------ occupancy and sight

    def occupancy(self, s):
        """Military and civilian unit slot per tile (-1 none): the stacking rules allow one of each on
        land (aircraft stand apart: they neither defend nor block)."""
        B, T = self.B, self.T
        U = s["u_alive"].shape[1]
        slots = mx.broadcast_to(mx.arange(U)[None, :], (B, U)).astype(I32)
        base = (mx.arange(B) * T)[:, None]
        civ = s["p_civilian"]
        res = []
        for want in (False, True):
            ok = s["u_alive"] & (s["u_tile"] >= 0) & (civ == want) & ~s["p_air"]
            idx = mx.where(ok, base + s["u_tile"], B * T)
            arr = mx.full((B * T + 1,), -1, dtype=I32).at[idx.reshape(-1)].maximum(slots.reshape(-1))
            res.append(arr[: B * T].reshape(B, T))
        return res[0], res[1]

    def stack_class(self, s, tiles):
        """The stack class of every unit slot as it would stand on `tiles` [B, U]."""
        water = self.mg("water", tiles) > 0
        civ = s["p_civilian"]
        land = mx.where(water, CL_EMB, mx.where(civ, CL_CIV, CL_MIL))
        sea = mx.where(civ, CL_NAVAL_CIV, CL_NAVAL)
        return mx.where(s["p_air"], CL_AIR, mx.where(s["p_sea"], sea, land))

    def can_stop_at(self, s, owner, cls, tiles):
        """`stacking::can_stop_at` for a unit of `owner` [B] and class `cls` [B, N] on `tiles` [B, N] (the
        unit itself elsewhere): no unit of another owner there, no own unit of the class."""
        ut = mx.where(s["u_alive"], s["u_tile"], -2)  # [B, U]
        ucl = self.stack_class(s, mx.maximum(s["u_tile"], 0))
        on = ut[:, None, :] == tiles[:, :, None]  # [B, N, U]
        foreign = on & (s["u_side"][:, None, :] != owner[:, None, None])
        same = on & (s["u_side"][:, None, :] == owner[:, None, None]) & (ucl[:, None, :] == cls[:, :, None]) & (cls[:, :, None] != CL_AIR)
        return ~mx.any(foreign | same, axis=2) & (tiles >= 0)

    def los_word(self, src, o):
        """The line-of-sight word holding offset `o` of source tile `src` (same shapes)."""
        W = self.geo.words
        return self.lg(mx.maximum(src, 0) * W + mx.maximum(o, 0) // 64)

    def los_bit(self, src, dst):
        """Line of sight from window tile src to dst ([B, ...] each), within the sight disc."""
        o = self.geo.rel_index(src, dst)
        word = self.los_word(src, o)
        bit = mx.right_shift(word, (mx.maximum(o, 0) % 64).astype(mx.uint64)) & 1
        return (bit == 1) & (o >= 0)

    def unit_sight(self, s):
        water = self.mg("water", s["u_tile"])
        hills = self.mg("hills", s["u_tile"])
        land = mx.minimum(s["p_sight"] + hills.astype(I32), 7)
        emb = s["p_land"] & ~s["p_hovering"] & water
        return mx.where(emb, s["p_embarked_sight"], land)

    def sources_see(self, src, radius, valid, tiles):
        """[B, S, N]: does each source tile (radius, valid) [B, S] see each target tile [B, N]? Distances
        and disc offsets from per-tile coordinates by broadcasting, the line-of-sight word gathered per
        source and word."""
        geo = self.geo
        R = geo.radius
        W = geo.words
        sq = mx.take(geo.q, mx.maximum(src, 0)); sr = mx.take(geo.r, mx.maximum(src, 0))
        tq = mx.take(geo.q, mx.maximum(tiles, 0)); tr = mx.take(geo.r, mx.maximum(tiles, 0))
        dq = tq[:, None, :] - sq[:, :, None]
        dr = tr[:, None, :] - sr[:, :, None]
        d = (mx.abs(dq) + mx.abs(dr) + mx.abs(dq + dr)) // 2
        inside = (mx.abs(dq) <= R) & (mx.abs(dr) <= R)
        o = mx.take(geo.rel.reshape(-1), mx.clip(dq + R, 0, 2 * R) * (2 * R + 1) + mx.clip(dr + R, 0, 2 * R))
        if W == 1:
            word = self.lg(mx.maximum(src, 0))[:, :, None]  # [B, S, 1]
        else:
            Bs, S = src.shape
            allw = self.lg(mx.maximum(src, 0)[:, :, None] * W + mx.arange(W)[None, None, :])  # [B, S, W]
            word = mx.take_along_axis(allw, mx.maximum(o, 0) // 64, axis=2)
        bit = mx.right_shift(word, (mx.maximum(o, 0) % 64).astype(mx.uint64)) & 1
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
        tid = self.tid.reshape((-1,) + (1,) * (tiles.ndim - 1))
        return mx.where(tiles >= 0, mx.take(self.city_plane.reshape(-1), tid * self.T + mx.maximum(tiles, 0)), -1)

    def controls_and_blocks(self, s, side):
        """Per window tile, for the mover's side: a visible enemy unit stands there (blocks the step),
        and the tile exerts a zone of control (kernel `Mover::controls`: ships control land and sea, land
        units control land only, embarked units nothing, enemy cities always; a ship mover feels only
        ships and cities: `_ctrl_sea`)."""
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
        mil_sea = mx.where(mil >= 0, gn(s["p_sea"].astype(I32), mil) > 0, False)
        enemy_mil = mil_side == enemy[:, None]
        by_city = city_owner == enemy[:, None]
        ctrl_land = vis & ((enemy_mil & (mil_sea | ~self.mplane("water"))) | by_city)
        self._ctrl_sea = vis & ((enemy_mil & mil_sea) | by_city)
        return block, ctrl_land, vis, mil, civ, mil_side, civ_side, city, city_owner

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
        water_l = mx.where(lt >= 0, self.mg("water", lt) > 0, False)
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
        stop = gn(foreign.astype(I32), path) > 0
        first = first_index(stop)
        any_stop = mx.any(stop, axis=1)
        prev = mx.where(any_stop, g1(path, mx.maximum(first - 1, 0)), gl)
        final_tile = g1(lt, prev)
        final_left = g1(left, prev)
        return final_tile, final_left

    def path_costs(self, s, side, slot, iters=None, check_every=8):
        """The multi-turn move cost of unit `slot` of `side` to every tile of the map (kernel `astar`'s
        turn-aware scalar, all goals at once: a Bellman-Ford wavefront over [B, T, 6] until no tile
        improves): g [B, T] (2^30 unreached), the scale, and the decoded `PathCost` (turns before the
        last step, points left after it). Units and zones of control come from the mover's view, as
        for a move order. The strategy's "how fast troops arrive" reads this."""
        geo = self.geo
        B, T = self.B, self.T
        INF = 1 << 30
        block, ctrl, *_ = self.controls_and_blocks(s, side)
        ctrl = mx.where(g1(s["p_sea"], slot)[:, None], self._ctrl_sea, ctrl)
        tile = g1(s["u_tile"], slot)
        nbr = geo.nbr  # [T, 6]: p = the neighbour in direction d; the step p -> t goes (d + 3) % 6
        dprime = (mx.arange(6) + 3) % 6
        cid = g1(s["p_cost_id"], slot)
        pidx = mx.maximum(nbr, 0)[None] * 6 + dprime[None, None, :]  # [1, T, 6]
        code = mx.take(self.cost_flat, cid[:, None, None] * (T * 6) + pidx)
        code = mx.where((nbr >= 0)[None], code, -2)
        n1 = mx.take(nbr.reshape(-1), mx.maximum(nbr, 0) * 6 + ((dprime + 5) % 6)[None, :])
        n2 = mx.take(nbr.reshape(-1), mx.maximum(nbr, 0) * 6 + ((dprime + 1) % 6)[None, :])
        ci = ctrl.astype(I32)
        slowed = ((n1 >= 0)[None] & (gn(ci, mx.broadcast_to(n1[None], (B, T, 6))) > 0)) | \
                 ((n2 >= 0)[None] & (gn(ci, mx.broadcast_to(n2[None], (B, T, 6))) > 0))
        slowed = slowed & ~g1(s["p_ignore_zoc"], slot)[:, None, None]
        edge_ok = (code >= 0) & ~block[:, :, None]
        all_moves = (code == 255) | slowed
        # The budget (`MoveBudget::of_unit`) and the allowance per tile (`Allowance::full_on`).
        embarks = g1(s["p_embarks"], slot)
        water = self.mplane("water")
        here_water = gn(water.astype(I32), tile[:, None])[:, 0] > 0
        mmax = g1(s["u_mmax"], slot)
        land_a = mx.where(embarks & here_water, g1(s["p_moves_land"], slot), mmax)
        emb_a = mx.where(embarks, mx.where(here_water, mmax, g1(s["p_moves_embarked"], slot)), mmax)
        scale = mx.maximum(land_a, emb_a)
        full = mx.where(embarks[:, None] & water, emb_a[:, None], land_a[:, None])  # [B, T]
        full_from = gn(full, mx.broadcast_to(mx.maximum(nbr, 0)[None], (B, T, 6)))
        left0 = mx.minimum(g1(s["u_mv"], slot), g1(full, tile))
        start = scale - left0
        tiles = mx.arange(T)[None, :]
        g = mx.where(tiles == tile[:, None], start[:, None], INF)
        g = mx.where((land_a > 0)[:, None] & (tile >= 0)[:, None], g, INF)
        sc = scale[:, None, None]
        nb = mx.broadcast_to(mx.maximum(nbr, 0)[None], (B, T, 6))

        def relax(g):
            gp = gn(g, nb)  # [B, T, 6] the scalar at each predecessor
            turn = gp // sc
            used = gp % sc
            fresh = (used == 0) & (gp > 0)
            left = mx.where(fresh, full_from, sc - used)
            step = mx.where(all_moves, left, mx.minimum(code, left))
            after = mx.minimum(left - step, full[:, :, None])
            ng = turn * sc + sc - after
            ng = mx.where(edge_ok & (gp < INF) & (nbr >= 0)[None], ng, INF)
            return mx.minimum(g, mx.min(ng, axis=2))

        relax_c = mx.compile(relax)
        n = iters or T
        done = 0
        while done < n:
            prev = g
            for _ in range(check_every):
                g = relax_c(g)
            done += check_every
            if iters is None and bool(mx.all(g == prev).item()):
                break
        reached = g < INF
        turns = mx.where(g <= scale[:, None], 0, (g - 1) // mx.maximum(scale[:, None], 1))
        moves_left = mx.where(g <= scale[:, None], scale[:, None] - g, (turns + 1) * scale[:, None] - g)
        return g, scale, mx.where(reached, turns, -1), mx.where(reached, moves_left, -1)

    def promo_mask(self, s, slot, alive):
        """[B, NP]: the promotions the unit may take now (`validate_promote`: enough experience for its
        level, `available_promotions`: choosable for its kind, not held, prerequisites held)."""
        level = mx.maximum(g1(s["u_level"], slot), 1)
        pct = mx.maximum(g1(s["p_xp_needed_pct"], slot), -100)
        need = mx.maximum((10 * level * (level + 1) // 2) * (100 + pct) // 100, 1)
        held = grow(s["u_prom"], slot)  # [B, NP]
        fit = self.tg(self.z["z_fit"], slot)
        req, anyof = mx.take(self.z["z_req"], self.tid, axis=0), mx.take(self.z["z_anyof"], self.tid, axis=0)  # [B, NP, R] (-1 none)
        req_ok = mx.all((req < 0) | gn(held, req), axis=2)
        any_ok = ~mx.take(self.z["z_has_any"], self.tid, axis=0) | mx.any((anyof >= 0) & gn(held, anyof), axis=2)
        ready = alive & (g1(s["u_xp"], slot) >= need)
        return ready[:, None] & fit & ~held & req_ok & any_ok

    def masks(self, s, side, slot):
        """Legal options of unit `slot` of `side` in every scene: fortify, set up, found [B]; promote
        [B, NP]; move and attack [B, T] (window tiles). Also returns what `apply` needs (the wavefront)."""
        B, T = self.B, self.T
        geo = self.geo
        block, ctrl, vis, mil, civ, mil_side, civ_side, city, city_owner = self.controls_and_blocks(s, side)
        alive = g1(s["u_alive"], slot) & (g1(s["u_side"], slot) == side) & ~s["done"]
        tile = g1(s["u_tile"], slot)
        st = g1(s["u_st"], slot)
        mv = g1(s["u_mv"], slot)
        civilian = g1(s["p_civilian"], slot)
        water_here = self.mg("water", tile[:, None])[:, 0] > 0
        land = g1(s["p_land"], slot)
        fort = alive & ~((st == 1) | (st == 2)) & ~civilian & ~g1(s["p_no_defensive_bonus"], slot) & land & ~water_here
        setup = alive & g1(s["p_must_set_up"], slot) & (st != 3) & (mv > 0) & ~water_here
        fo = mx.take(self.m["found_ok"].reshape(-1), self.tid * (2 * T) + side * T + mx.maximum(tile, 0))
        found = alive & g1(s["p_founder"], slot) & fo
        promo = self.promo_mask(s, slot, alive) if self.f["promo"] else mx.zeros((B, s["u_prom"].shape[2]), dtype=mx.bool_)
        # Moves.
        ctrl = mx.where(g1(s["p_sea"], slot)[:, None], self._ctrl_sea, ctrl)
        left, lt = self.reach(s, slot, block, ctrl)
        L = lt.shape[1]
        ms_l = mx.where(lt >= 0, gn(mil_side, lt), -1)
        cs_l = mx.where(lt >= 0, gn(civ_side, lt), -1)
        foreign_l = ((ms_l >= 0) & (ms_l != side[:, None])) | ((cs_l >= 0) & (cs_l != side[:, None]))
        own_same = mx.where(civilian[:, None], cs_l == side[:, None], ms_l == side[:, None])
        if self.has_air:
            # The dump's script (`act::free_for`) keeps a military unit off a tile with own aircraft.
            ut = mx.where(s["u_alive"] & s["p_air"] & (s["u_side"] == side[:, None]), s["u_tile"], -2)
            own_air = mx.any(lt[:, :, None] == ut[:, None, :], axis=2) & (lt >= 0)
            own_same = own_same | (own_air & ~civilian[:, None])
        ok_l = (left >= 0) & (mx.arange(L)[None, :] > 0) & (lt >= 0) & ~foreign_l & ~own_same
        ok_l = ok_l & mx.where(lt >= 0, mx.take(geo.inner, mx.maximum(lt, 0)), False) & alive[:, None]
        # Attacks (over the attack disc of targets; aircraft over the window).
        att_d, tgt_d = self.attack_mask(s, side, slot, alive, vis, mil, civ, mil_side, civ_side, city, city_owner)
        out = {"fort": fort, "setup": setup, "found": found, "promo": promo, "move_l": ok_l, "attack_d": att_d, "tgt_d": tgt_d,
               "left": left, "lt": lt, "edges": self._edges, "foreign_l": foreign_l}
        if self.has_air:
            out["attack_air"] = self.air_attack_mask(s, side, slot, alive, vis, mil_side, civ_side, city, city_owner)
        return out

    def global_masks(self, mk):
        """The move and attack masks over the window [B, T] (the parity check reads them)."""
        B, T = self.B, self.T
        base = (mx.arange(B) * T)[:, None]
        def scatter(ok, tiles):
            return mx.zeros((B * T + 1,), dtype=I32).at[mx.where(ok & (tiles >= 0), base + tiles, B * T).reshape(-1)].add(1)[: B * T].reshape(B, T) > 0
        att = scatter(mk["attack_d"], mk["tgt_d"])
        if "attack_air" in mk:
            att = att | mk["attack_air"]
        return scatter(mk["move_l"], mk["lt"]), att

    def attack_mask(self, s, side, slot, alive, vis, mil, civ, mil_side, civ_side, city, city_owner):
        B, T = self.B, self.T
        geo = self.geo
        tile = g1(s["u_tile"], slot)
        tgt = mx.take(geo.att_tile, mx.maximum(tile, 0), axis=0)  # [B, OA]
        d = mx.broadcast_to(geo.att_dist[None], tgt.shape)
        ranged = g1(s["p_is_ranged"], slot)
        rng = g1(s["p_range"], slot)
        base_c = g1(s["p_base_combat"], slot)
        base_r = g1(s["p_base_ranged"], slot)
        civilian = g1(s["p_civilian"], slot)
        allowed = 1 + mx.clip(g1(s["p_attacks_per_turn"], slot), 0, 9)
        ready = alive & ~civilian & ((base_c > 0) | (base_r > 0)) & (g1(s["u_mv"], slot) > 0) & (g1(s["u_att"], slot) < allowed)
        ready = ready & ~g1(s["p_air"], slot)
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
        water_t = mx.where(tgt >= 0, self.mg("water", tgt) > 0, False)
        water_me = self.mg("water", tile[:, None])[:, 0] > 0
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
        rr = rr & t_vis
        los = self.los_bit(mx.broadcast_to(tile[:, None], tgt.shape), tgt)
        rr = rr & (g1(s["p_indirect_fire"], slot)[:, None] | los)
        rr = rr & mx.where(g1(s["p_ranged_only_in_domain"], slot)[:, None], water_t & (t_city < 0), True)
        ok = ready[:, None] & (tgt >= 0) & target_ok & (m_ok | rr)
        return ok, tgt

    def air_attack_mask(self, s, side, slot, alive, vis, mil_side, civ_side, city, city_owner):
        """An aircraft's strike targets [B, T] (`check_ranged_reach` for air: range over the window, a
        visible target, no line of sight needed)."""
        B, T = self.B, self.T
        tile = g1(s["u_tile"], slot)
        tiles = mx.broadcast_to(mx.arange(T)[None, :], (B, T)).astype(I32)
        d = self.geo.distance(tiles, mx.broadcast_to(tile[:, None], tiles.shape))
        allowed = 1 + mx.clip(g1(s["p_attacks_per_turn"], slot), 0, 9)
        ready = alive & g1(s["p_air"], slot) & (g1(s["p_base_ranged"], slot) > 0) & (g1(s["p_range"], slot) > 0)
        ready = ready & (g1(s["u_mv"], slot) > 0) & (g1(s["u_att"], slot) < allowed)
        enemy = 1 - side
        foreign_city = (city >= 0) & (city_owner != side[:, None]) & (city_owner >= 0)
        enemy_unit = (mil_side == enemy[:, None]) | (civ_side == enemy[:, None])
        own_there = (mil_side == side[:, None]) | (civ_side == side[:, None])
        target_ok = foreign_city | (enemy_unit & ~own_there)
        rng = mx.maximum(g1(s["p_range"], slot), 1)
        return ready[:, None] & (d >= 1) & (d <= rng[:, None]) & vis & target_ok

    def city_mask(self, s, side, cslot):
        """Targets of city `cslot`'s strike [B, T] (`plan_city_strike`)."""
        B, T = self.B, self.T
        geo = self.geo
        ct = g1(self.cs["tile"], cslot)
        owner = g1(s["c_owner"], cslot)
        mine = g1(self.cs["alive"], cslot) & (owner == side) & (ct >= 0) & ~s["done"]
        maxhp = self.city_maxhp(s, cslot)
        ready = mine & ~g1(s["c_strike"], cslot) & (g1(s["c_dmg"], cslot) < maxhp)
        block, ctrl, vis, mil, civ, mil_side, civ_side, city, city_owner = self.controls_and_blocks(s, side)
        tgt = mx.take(geo.att_tile, mx.maximum(ct, 0), axis=0)
        d = mx.broadcast_to(geo.att_dist[None], tgt.shape)
        enemy = 1 - side
        t_vis = mx.where(tgt >= 0, gn(vis.astype(I32), tgt) > 0, False)
        t_unit = (mx.where(tgt >= 0, gn(mil_side, tgt), -1) == enemy[:, None]) | (mx.where(tgt >= 0, gn(civ_side, tgt), -1) == enemy[:, None])
        ok = ready[:, None] & (tgt >= 0) & (d >= 1) & (d <= CITY_RANGE) & t_vis & t_unit
        base = (mx.arange(B) * T)[:, None]
        return mx.zeros((B * T + 1,), dtype=I32).at[mx.where(ok, base + tgt, B * T).reshape(-1)].add(1)[: B * T].reshape(B, T) > 0

    # ------------------------------------------------------------------ strengths

    FIGHTER_KEYS = ("base_combat", "base_ranged", "strength", "ranged_strength", "role", "domain", "class", "wounded_mod", "embarked_defense",
                    "civilian", "land", "sea", "air", "is_ranged", "no_defensive_bonus", "ranged_support_fire", "heavy_charge", "river_crossing",
                    "amphibious", "general_aura", "withdraw", "xp_percent", "heal_on_kill", "sapper", "capture_defeated", "suicide",
                    "evasion", "interception_taken", "dest_id", "cost_id")

    def fighter(self, s, slot, valid):
        f = {"slot": slot, "valid": valid & (slot >= 0)}
        for k in ("u_tile", "u_side", "u_hp", "u_st", "u_mv", "u_mmax"):
            f[k[2:]] = g1(s[k], slot)
        for k in self.FIGHTER_KEYS:
            f[k] = g1(s["p_" + k], slot)
        f["ctx"] = grow(s["p_ctx"], slot)
        for k in ("tctx", "ttype", "targ", "targ2", "tpct", "tprom"):
            f[k] = grow(s["p_" + k], slot)
        f["prom"] = grow(s["u_prom"], slot)
        f["water"] = self.mg("water", f["tile"][:, None])[:, 0] > 0
        f["embarked"] = f["land"] & f["water"]
        return f

    def context_sum(self, s, f, ctx, battle, other):
        """Σ of the unit's percents in the holding contexts, situational terms included (`context_sum`);
        a promotion's terms hold only while the unit has it."""
        total = mx.sum(mx.where(ctx, f["ctx"], 0), axis=1)
        in_ctx = mx.take_along_axis(ctx, f["tctx"], axis=1)
        tt, ta = f["ttype"], f["targ"]
        feat = self.mg("feature", battle[:, None])[:, 0]
        tkind = self.mg("terrain_kind", battle[:, None])[:, 0]
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
        tp = f["tprom"]
        gate = (tp < 0) | mx.take_along_axis(f["prom"], mx.maximum(tp, 0), axis=1)
        return total + mx.sum(mx.where(in_ctx & holds & gate, f["tpct"], 0), axis=1)

    def near_improvement(self, s, f):
        geo = self.geo
        disc = mx.take(geo.att_tile, mx.maximum(f["tile"], 0), axis=0)  # [B, OA]
        imp = mx.where(disc >= 0, self.mg("improvement", disc), -1)
        own = mx.where(disc >= 0, gn(s["owner"], disc), -1) == f["side"][:, None]
        d = geo.att_dist[None]
        hit = (imp[:, None, :] == f["targ"][:, :, None]) & own[:, None, :] & (d[:, None, :] <= f["targ2"][:, :, None])
        return mx.any(hit, axis=2)

    def generic(self, s, f, other, adjacency):
        """M_generic: great generals, enemy auras, flanking (melee attack only)."""
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
        m = m + mx.where(f["air"], 0, mx.sum(mx.where(aura, s["p_enemy_aura"], 0), axis=1))
        if other is not None and adjacency:
            d_o = self.geo.distance(tiles, mx.broadcast_to(other["tile"][:, None], tiles.shape))
            flank = own & ~s["p_civilian"] & ~s["p_air"] & (s["p_strength"] > 0) & (s["p_ranged_strength"] == 0) & (d_o == 1)
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
        water = self.mg("water", tiles) > 0
        own = s["u_alive"] & (tiles >= 0) & (s["u_side"] == f["side"][:, None]) & ~s["p_civilian"] & ~s["p_air"] & (mx.arange(U)[None, :] != f["slot"][:, None])
        return mx.any(own & (d == 1) & (water == f["water"][:, None]), axis=1)

    def ctxset(self, cols):
        B = self.B
        out = [mx.zeros((B,), dtype=mx.bool_)] * N_CTX
        for i, v in cols.items():
            out[i] = v if v.ndim else mx.broadcast_to(v, (B,))
        return mx.stack(out, axis=1)

    def tile_defense(self, s, tile, side):
        gdef = self.mg("ground_def", tile[:, None])[:, 0]
        idef = self.mg("improvement_def", tile[:, None])[:, 0]
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
        rough = self.mg("rough", to[:, None])[:, 0] > 0
        dv = d["valid"] if d is not None else mx.zeros_like(a["valid"])
        cols = {ANY: mx.array(tr), ATTACK: mx.array(tr), VS_CITY: vs_city, ATT_OPEN: ~vs_city & ~rough, ATT_ROUGH: ~vs_city & rough,
                FRIENDLY: friendly, ATT_FRIENDLY: friendly, OUTSIDE_FRIENDLY: ~friendly,
                VS_WOUNDED: dv & (d["hp"] < 100) if d is not None else dv, VS_FORTIFIED: dv & ((d["st"] == 1) | (d["st"] == 2)) if d is not None else dv,
                ADJ_FRIENDLY: self.friendly_adjacent(s, a)}
        ctx = self.ctxset(cols)
        m = self.context_sum(s, a, ctx, to, d if d is not None else None) + self.generic(s, a, d, True)
        m = m + mx.where(vs_city & self.near_sapper(s, a["side"], to), 50, 0)
        dirs = self.dir_between(a["tile"], to)
        river = mx.take(self.m["river"].reshape(-1), self.tid * (self.T * 6) + mx.maximum(a["tile"], 0) * 6 + mx.maximum(dirs, 0)) & (dirs >= 0)
        m = m + mx.where(river & ~(a["amphibious"] | a["river_crossing"]), -20, 0)
        water_to = self.mg("water", to[:, None])[:, 0] > 0
        m = m + mx.where(a["land"] & a["water"] & ~water_to & ~a["amphibious"], -50, 0)
        strength = self.finish(a["base_combat"], m)
        if d is not None:
            heavy = a["heavy_charge"] & dv & (d["withdraw"] <= 0)
            strength = mx.where(heavy, strength * 150 // 100, strength)
        # An embarked unit attacking water fights with the era's embarked defense.
        return mx.where(a["embarked"] & water_to, a["embarked_defense"], strength)

    def dir_between(self, a, b):
        nb = mx.take(self.geo.nbr, mx.maximum(a, 0), axis=0)  # [B, 6]
        hit = nb == b[:, None]
        return mx.max(mx.where(hit, mx.arange(6)[None, :], -1), axis=1)

    def defense_strength(self, s, d, a, ranged):
        tile = d["tile"]
        rough = self.mg("rough", tile[:, None])[:, 0] > 0
        friendly = gn(s["owner"], tile[:, None])[:, 0] == d["side"]
        rg = mx.array(ranged) if isinstance(ranged, bool) else ranged
        cols = {ANY: mx.array(True), DEFENSE: mx.array(True), DEF_VS_RANGED: rg, DEF_OPEN: ~rough, DEF_ROUGH: rough,
                FRIENDLY: friendly, OUTSIDE_FRIENDLY: ~friendly}
        if ranged is not True:
            cols[ADJ_FRIENDLY] = self.friendly_adjacent(s, d) & ~rg
        ctx = self.ctxset(cols)
        m = self.context_sum(s, d, ctx, tile, a) + self.generic(s, d, a, False)
        terrain = mx.where(d["water"], 0, self.tile_defense(s, tile, d["side"]))
        m = m + mx.where(~d["no_defensive_bonus"] | (terrain < 0), terrain, 0)
        fort = mx.where((d["st"] == 1) | (d["st"] == 2), d["st"], 0)
        m = m + mx.where(~d["ranged_support_fire"], fort * 20, 0)
        return mx.where(d["embarked"], d["embarked_defense"], self.finish(d["base_combat"], m))

    def ranged_strength(self, s, me, other, vs_city, target, attacking):
        rough = self.mg("rough", target[:, None])[:, 0] > 0
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

    def city_maxhp(self, s, cslot):
        """The city's most hit points: the army twin's schedule (`c_maxhp`, buildings) where the state has it."""
        return g1(s["c_maxhp"] if "c_maxhp" in s else self.cs["max_hp"], cslot)

    def city_strength(self, s, cslot, mil):
        ct = g1(self.cs["tile"], cslot)
        owner = g1(s["c_owner"], cslot)
        if "c_str0" in s:
            # The army twin's schedule of the city's strength without the garrison (techs, buildings, population).
            base = g1(s["c_str0"], cslot)
        else:
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

    def unit_damage(self, me, opp, my_damage, wounded_mod, roll):
        """`damage::melee_damage` / `ranged_damage` / `city_strike_damage` (a city: no wounded factor, pass 0
        damage): exact 128-bit by default, the float32 ratio with `fast_damage` (not the parity mode)."""
        return exact.unit_damage(me, opp, my_damage, wounded_mod, roll, fast=self.fast).astype(I32)

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
        wo = self.mg("water", old_tile[:, None])[:, 0] > 0
        wn = self.mg("water", new_tile[:, None])[:, 0] > 0
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

    def new_unit(self, s, side, tile, where, hp, mmax, kind, profile_from=None):
        """A unit the play creates, in the next free slot (`n_slots`): its state, and its profile from
        the spawn table row `profile_from` [B] (else the slot keeps the converter's captured-civilian
        profile). Returns (state, slot)."""
        U = s["u_alive"].shape[1]
        new = s["n_slots"]
        where = where & (new < U)
        z = mx.zeros_like(new)
        s = self.set_unit(s, new, where, alive=mx.ones(new.shape, dtype=mx.bool_), side=side, tile=tile, hp=hp, mv=z, mmax=mmax,
                          att=z, st=z, xp=z, kind=kind, level=mx.ones_like(new), icpt=z, mset=z)
        s["u_prom"] = put1(s["u_prom"], new, mx.zeros((self.B, s["u_prom"].shape[2]), dtype=mx.bool_), where)
        if profile_from is not None:
            known = where & (profile_from >= 0)
            s["u_prom"] = put1(s["u_prom"], new, self.tg(self.z["sp_prom"], profile_from), known)
            s = self.set_unit(s, new, known, level=self.tg(self.z["sp_level"], profile_from))
            ri = self.tg(self.sp_int, profile_from)  # [B, n]: one gather per table
            for i, k in enumerate(self.sp_keys["int"]):
                s["p_" + k] = put1(s["p_" + k], new, ri[:, i], known)
            if self.sp_bool is not None:
                rb = self.tg(self.sp_bool, profile_from)
                for i, k in enumerate(self.sp_keys["bool"]):
                    s["p_" + k] = put1(s["p_" + k], new, rb[:, i], known)
            for k, tab in zip(self.sp_keys["vec"], self.sp_vec):
                s["p_" + k] = put1(s["p_" + k], new, self.tg(tab, profile_from).astype(s["p_" + k].dtype), known)
        s["n_slots"] = mx.where(where, new + 1, new)
        return s, new

    def capture_civilians(self, s, captor, tile, where, civ):
        victim = gn(civ, tile[:, None])[:, 0]
        vside = g1(s["u_side"], victim)
        csid = g1(s["u_side"], captor)
        hit = where & (victim >= 0) & (vside != csid)
        kind = g1(s["p_capture_kind"], victim)
        moves = g1(s["p_capture_moves"], victim)
        s = self.set_unit(s, victim, hit, alive=mx.zeros(victim.shape, dtype=mx.bool_), tile=mx.full(victim.shape, -1, dtype=I32))
        spawn = hit & (kind >= 0) & ~g1(s["p_no_capture"], captor)
        # What the civilian becomes, as the captor's (the dump's spawn table; else the converter's
        # captured-civilian slot profile).
        if not self.f["spawn"]:
            s, _ = self.new_unit(s, csid, tile, spawn, mx.full(victim.shape, 100, dtype=I32), moves, kind)
            return s
        sp = g1(s["p_civ_spawn"], victim)
        known = sp >= 0
        water = self.mg("water", tile[:, None])[:, 0] > 0
        mmax = mx.where(water & self.tg(self.z["sp_land"], sp) & self.tg(self.z["sp_embarks"], sp), self.tg(self.z["sp_moves_embarked"], sp), self.tg(self.z["sp_moves_land"], sp))
        s, _ = self.new_unit(s, csid, tile, spawn, mx.full(victim.shape, 100, dtype=I32), mx.where(known, mmax, moves),
                             mx.where(known, self.tg(self.z["sp_kind"], sp), kind), profile_from=sp)
        return s

    def free_tile_near(self, s, owner, tile, cls, dest_id, where, land=None):
        """`free_tile_near`: the tile itself when a unit of class `cls` of `owner` may stop there, else
        the first neighbour (direction order) it may stand on (`dest_id`'s table) and stop at, else the
        tile. `land` [B]: a land unit, whose class on a water tile is the embarked one (per candidate)."""
        nb = mx.take(self.geo.nbr, mx.maximum(tile, 0), axis=0)  # [B, 6]
        cand = mx.concatenate([tile[:, None], nb], axis=1)  # [B, 7]
        cls_c = mx.broadcast_to(cls[:, None], cand.shape)
        if land is not None:
            cls_c = mx.where((self.mg("water", mx.maximum(cand, 0)) > 0) & land[:, None], CL_EMB, cls_c)
        stop = self.can_stop_at(s, owner, cls_c, cand)
        dest = mx.take(self.dest_pool.reshape(-1), (mx.maximum(dest_id, 0) * self.T)[:, None] + mx.maximum(cand, 0))
        ok = stop & (cand >= 0) & ((mx.arange(7)[None, :] == 0) | dest)
        i = first_index(ok)
        return mx.where(i < 7, mx.take_along_axis(cand, mx.minimum(i, 6)[:, None], axis=1)[:, 0], tile)

    def withdraw_tile(self, s, d, a_tile):
        """`withdraw_tile`: the defender's first neighbour (tile order) not next to the attacker, of its
        element and borders, where it may stop, without a city ([B], -1 none)."""
        nb = mx.take(self.geo.nbr, mx.maximum(d["tile"], 0), axis=0)  # [B, 6]
        da = self.geo.distance(nb, mx.broadcast_to(a_tile[:, None], nb.shape))
        wd = mx.take(self.withdraw_pool.reshape(-1), (mx.maximum(d["dest_id"], 0) * self.T)[:, None] + mx.maximum(nb, 0))
        water = self.mg("water", nb) > 0
        cls = mx.where(d["sea"][:, None], CL_NAVAL, mx.where(water, CL_EMB, CL_MIL))
        stop = self.can_stop_at(s, d["side"], cls, nb)
        ok = (nb >= 0) & (nb != a_tile[:, None]) & (da != 1) & wd & stop & (self.city_on(s, nb) < 0)
        best = mx.min(mx.where(ok, nb, 1 << 20), axis=1)
        return mx.where(best < (1 << 20), best, -1)

    def apply_unit(self, s, side, slot, verb, tgt, mk, take):
        """One unit action in every scene where `take`."""
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
        # Promote.
        if self.f["promo"]:
            s = self.promote(s, slot, tgt, take & (verb == V_PROMOTE))
        # Move.
        mvv = take & (verb == V_MOVE)
        lt, left = mk["lt"], mk["left"]
        hit = lt == tgt[:, None]
        left_t = mx.max(mx.where(hit, left, -1), axis=1)
        mvv = mvv & (left_t >= 0)
        # (the walk and the attack are the heavy parts: skipped where no row moves or strikes)
        if bool(mx.any(mvv).item()):
            end_tile, end_left = self.walk(s, slot, mk, tgt, mk["foreign_l"])
            s = self.set_unit(s, slot, mvv, tile=end_tile, mv=mx.maximum(end_left, 0), st=mx.zeros_like(slot))
            s = self.refresh_allowance(s, slot, tile, end_tile, mvv)
        # Attack.
        atk = take & (verb == V_ATTACK)
        if bool(mx.any(atk).item()):
            s = self.attack(s, side, slot, tgt, atk)
        return s

    def promote(self, s, slot, p, where):
        """Take promotion `p` (`Transaction::promote`): level + 1; an instant heal heals 50 HP and does
        not stay; any other joins the unit and adds its change to the profile (contexts, stats, flags,
        moves, sight, range; the movement tables of the new set)."""
        p = mx.maximum(p, 0)
        insta = self.tg(self.z["z_insta"], p)
        s = self.set_unit(s, slot, where, level=g1(s["u_level"], slot) + 1)
        s = self.set_unit(s, slot, where & insta, hp=mx.minimum(g1(s["u_hp"], slot) + INSTANT_HEAL, 100))
        keep = where & ~insta
        held = grow(s["u_prom"], slot)
        held = held | ((mx.arange(held.shape[1])[None, :] == p[:, None]) & keep[:, None])
        s["u_prom"] = put1(s["u_prom"], slot, held, keep)
        di = self.tg(self.pz_int, slot, p)  # [B, 23 + NI]: one gather for every change
        db = self.tg(self.pz_bool, slot, p)
        s["p_ctx"] = put1(s["p_ctx"], slot, grow(s["p_ctx"], slot) + di[:, :N_CTX], keep)
        for i, k in enumerate(self.D_INT):
            name = "p_" + k
            s[name] = put1(s[name], slot, g1(s[name], slot) + di[:, N_CTX + i], keep)
        for i, k in enumerate(FLAG_DELTA):
            name = "p_" + k
            s[name] = put1(s[name], slot, g1(s[name], slot) | db[:, i], keep)
        cm = self.tg(self.z["z_cmov"], slot, p)
        mset = g1(s["u_mset"], slot)
        nset = mx.where(cm >= 0, mset | mx.left_shift(mx.ones_like(cm), mx.maximum(cm, 0)), mset)
        cid = self.tg(self.z["z_combo_cost"], slot, nset)
        did = self.tg(self.z["z_combo_dest"], slot, nset)
        moved = keep & (cm >= 0)
        known = moved & (cid >= 0)
        s = self.set_unit(s, slot, moved, mset=nset)
        s["p_cost_id"] = put1(s["p_cost_id"], slot, cid, known)
        s["p_dest_id"] = put1(s["p_dest_id"], slot, did, known)
        s["n_unmodelled"] = s["n_unmodelled"] + (moved & ~known).astype(I32) + (keep & self.tg(self.z["z_odd"], slot, p)).astype(I32)
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
        ranged = a["is_ranged"] | a["air"]
        air = take & a["air"]
        d = self.fighter(s, mx.where(is_unit, dslot, cvslot), is_unit)
        c_str, c_rstr = self.city_strength(s, cslot, mil)
        c_maxhp = self.city_maxhp(s, cslot)
        c_hp = c_maxhp - g1(s["c_dmg"], cslot)
        a_dmg_taken = 100 - a["hp"]
        # Strengths: one of each kind per scene (the target, unit or city, is a per-scene flag).
        att_m = self.melee_attack_strength(s, a, d, to, is_city)
        att_r = self.ranged_strength(s, a, d, is_city, to, True)
        def_u = self.defense_strength(s, d, a, ranged)  # melee or vs ranged by the attacker's kind
        ranged_land = ~d["ranged_support_fire"] & ~d["sea"] & (d["base_ranged"] > 0)
        rs_d = self.ranged_strength(s, d, a, mx.zeros_like(take), d["tile"], False)
        def_u = mx.where(ranged & ~d["embarked"] & ranged_land, rs_d, def_u)
        att_sel = mx.where(ranged, att_r, att_m)
        def_sel = mx.where(is_city, c_str, def_u)
        melee_unit = is_unit & ~ranged
        melee_city = is_city & ~ranged
        ranged_any = take & ranged
        # Withdrawal before melee (`try_withdraw`): a free tile away from the attacker, then the roll.
        if self.f["withdraw"]:
            wtile = self.withdraw_tile(s, d, a["tile"])
            can_wd = melee_unit & (d["withdraw"] > 0) & (wtile >= 0)
            rng, wroll = exact.draw(s["rng"], mx.full((B,), 100, dtype=mx.int64), can_wd)
            withdrew = can_wd & (wroll < d["withdraw"].astype(mx.int64))
            melee_unit = melee_unit & ~withdrew
            if self.events is not None:
                self.events["withdrawals"] += int(mx.sum(withdrew).item())
                self.events["withdraw_rolls"] += int(mx.sum(can_wd).item())
        else:
            rng, withdrew, wtile = s["rng"], mx.zeros_like(take), d["tile"]
        # Interception of an air strike (`intercept`): evasion roll, interception roll, damage roll.
        icpt, g_slot, rng = self.intercept(s, a, d, to, air, is_unit | is_civ, rng)
        # Rolls: melee (unit or city) draws two, ranged one, melee on civilians none.
        two = melee_unit | melee_city
        rng, r1 = exact.draw(rng, mx.full((B,), 1200, dtype=mx.int64), two | ranged_any)
        rng, r2 = exact.draw(rng, mx.full((B,), 1200, dtype=mx.int64), two)
        s["rng"] = rng
        wm = a["wounded_mod"]
        # --- the defender withdrew: it steps back, the attacker takes the tile
        if self.f["withdraw"]:
            s = self.set_unit(s, d["slot"], withdrew, tile=wtile, st=mx.zeros_like(slot))
            s = self.refresh_allowance(s, d["slot"], d["tile"], wtile, withdrew)
        # The damage to the target (every kind of strike: one evaluation) and the answer of a melee.
        hurt_air = air & (icpt > 0)
        my_dmg = mx.where(hurt_air, mx.minimum(a_dmg_taken + icpt, 100), a_dmg_taken)
        d1 = self.unit_damage(att_sel, def_sel, my_dmg, mx.where(hurt_air, 0, wm), r1)
        d2 = self.unit_damage(def_sel, att_m, mx.where(is_city, 0, 100 - d["hp"]), d["wounded_mod"], r2)
        # --- melee unit vs unit
        to_def, to_att = exact.resolve_mutual_kill(d1, d["hp"], d2, a["hp"])
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
        # CaptureDefeatedEnemy (`try_capture_defeated`): the roll, then the defeated kind joins the
        # attacker with half HP next to it.
        # A captured unit placed on the conquered tile keeps the attacker out of it (`advance` asks
        # `can_stop_at`: one unit of a kind per tile).
        blocked = mx.zeros_like(win)
        if self.f["capture_defeated"]:
            s, got, spot = self.capture_defeated(s, a, d, win & a["capture_defeated"])
            blocked = got & (spot == to)
        s = self.finish_attack(s, slot, melee_unit & ~d_killed & ~a_killed, mx.ones_like(win))
        # A won melee, a withdrawal and a melee on civilians: capture the civilians there, advance, the
        # attack ends.
        moves_in = win | withdrew | (is_civ & ~ranged)
        s = self.capture_civilians(s, slot, to, moves_in, civ)
        s = self.advance(s, slot, to, moves_in & ~blocked)
        s = self.finish_attack(s, slot, moves_in, mx.zeros_like(win))
        # --- melee on a city
        to_city = mx.minimum(d1, c_hp)
        to_att_c = d2
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
        # --- ranged (and air strikes)
        civ_target = ranged_any & is_civ
        unit_target = ranged_any & is_unit
        city_target = ranged_any & is_city
        r_unit = mx.where(civ_target, exact.CIVILIAN_DAMAGE, d1)
        r_unit = mx.minimum(r_unit, d["hp"])
        r_city = mx.minimum(d1, mx.maximum(c_hp - 1, 0))
        s["c_dmg"] = put1(s["c_dmg"], cslot, g1(s["c_dmg"], cslot) + r_city, city_target)
        s, rk = self.strike_unit(s, d["slot"], r_unit, unit_target | civ_target)
        dxp = mx.where(air, XP_DEF_AIR, XP_DEF_RANGED)
        s = self.set_unit(s, d["slot"], unit_target & ~rk, xp=g1(s["u_xp"], d["slot"]) + self.xp_gain(dxp, d["xp_percent"]))
        s = self.heal_on_kill(s, slot, (unit_target | civ_target) & rk, a["heal_on_kill"])
        axp = mx.where(city_target, mx.where(air, XP_ATT_CITY_AIR, XP_ATT_CITY_RANGED), mx.where(air, XP_ATT_AIR, XP_ATT_RANGED))
        s = self.set_unit(s, slot, ranged_any, xp=g1(s["u_xp"], slot) + self.xp_gain(axp, a["xp_percent"]))
        # The aircraft takes the target's answer (a surviving target) and the interception.
        air_dead = mx.zeros_like(take)
        if self.has_air:
            alive_t = city_target | ((unit_target | civ_target) & ~rk)
            s["rng"], r3 = exact.draw(s["rng"], mx.full((B,), 1200, dtype=mx.int64), air & alive_t)
            new_d_hp = g1(s["u_hp"], d["slot"])
            ans = self.unit_damage(def_sel, att_sel, mx.where(city_target, 0, 100 - new_d_hp), mx.zeros_like(wm), r3)
            answer = mx.where(city_target | (unit_target & ~rk), ans, 0)
            total = mx.where(air, mx.where(alive_t, answer, 0) + icpt, 0)
            s, air_dead = self.strike_unit(s, slot, total, air & (total > 0))
        spent = ranged_any & ~air_dead & a["suicide"]
        s = self.set_unit(s, slot, spent, alive=mx.zeros(slot.shape, dtype=mx.bool_), tile=mx.full(slot.shape, -1, dtype=I32))
        s = self.finish_attack(s, slot, ranged_any & ~air_dead & ~a["suicide"], mx.ones_like(win))
        return s

    def intercept(self, s, a, d, to, air, unit_target, rng):
        """The best interceptor of the target tile (`best_interceptor`: an enemy of the attacker with an
        interception range covering the tile, a chance, interceptions left; an aircraft only on duty and
        unmoved, its chance by its hit points; not the defender), then the evasion and interception rolls
        and the damage roll. Returns (damage to the aircraft [B], interceptor slot, rng) and books the
        interception and its experience."""
        B = self.B
        U = s["u_alive"].shape[1]
        if not self.has_air:
            return mx.zeros((B,), dtype=I32), mx.full((B,), -1, dtype=I32), rng
        tiles = s["u_tile"]
        dist = self.geo.distance(tiles, mx.broadcast_to(to[:, None], tiles.shape))
        per_turn = 1 + mx.maximum(s["p_interceptions_per_turn"], 0)
        duty = ~s["p_air"] | ((s["u_st"] == ST_INTERCEPT) & (s["u_mv"] >= s["u_mmax"]))
        excl = unit_target[:, None] & (mx.arange(U)[None, :] == d["slot"][:, None])
        cand = s["u_alive"] & (tiles >= 0) & (s["u_side"] != a["side"][:, None]) & (s["p_air_intercept_range"] > 0) & \
            (dist <= s["p_air_intercept_range"]) & (s["p_intercept_chance"] > 0) & (s["u_icpt"] < per_turn) & duty & ~excl
        chance = mx.where(s["p_air"], mx.maximum(s["p_intercept_chance"], 0) * s["u_hp"] // 100, mx.maximum(s["p_intercept_chance"], 0))
        chance = mx.where(cand, chance, -1)
        best = mx.max(chance, axis=1)
        g_slot = first_index(cand & (chance == best[:, None]))
        has = air & mx.any(cand, axis=1)
        g_slot = mx.where(has, g_slot, -1)
        rng, r_ev = exact.draw(rng, mx.full((B,), 100, dtype=mx.int64), has)
        p1 = has & (r_ev >= a["evasion"].astype(mx.int64))
        rng, r_ch = exact.draw(rng, mx.full((B,), 100, dtype=mx.int64), p1)
        p2 = p1 & (r_ch < best.astype(mx.int64))
        rng, r_i = exact.draw(rng, mx.full((B,), 1200, dtype=mx.int64), p2)
        g = self.fighter(s, g_slot, p2)
        g_rng = self.ranged_strength(s, g, a, mx.zeros_like(air), g["tile"], False)
        g_def = self.defense_strength(s, g, a, True)
        s_int = mx.where((g["base_ranged"] > 0) & ~g["sea"], g_rng, g_def)
        s_int = s_int * (100 + g["ctx"][:, CTX_INTERCEPTION]) // 100
        s_att = self.ranged_strength(s, a, g, mx.zeros_like(air), g["tile"], True)
        # interception_damage: the wounded factor is 100 - damage (no 33 % rule), the rounded damage
        # times the attacker's InterceptionDamageTaken factor, truncated.
        dmg = exact.interception_damage(s_int, s_att, 100 - g["hp"], a["interception_taken"], r_i, fast=self.fast).astype(I32)
        dmg = mx.where(p2, dmg, 0)
        s["u_icpt"] = put1(s["u_icpt"], g_slot, g1(s["u_icpt"], g_slot) + 1, p2)
        s["u_xp"] = put1(s["u_xp"], g_slot, g1(s["u_xp"], g_slot) + self.xp_gain(XP_INTERCEPT, g["xp_percent"]), p2 & (dmg > 0))
        return dmg, g_slot, rng

    def capture_defeated(self, s, a, d, where):
        """`try_capture_defeated`: chance min(80, 10 + 40 x strength(attacker kind) / strength(victim
        kind)); the victim's kind joins the attacker's side with 50 HP and no moves next to it."""
        B = self.B
        chance = mx.minimum(10 + mx.maximum(a["strength"], 1) * 40 // mx.maximum(d["strength"], 1), 80)
        s["rng"], roll = exact.draw(s["rng"], mx.full((B,), 100, dtype=mx.int64), where)
        got = where & (roll < chance.astype(mx.int64))
        sp = g2(s["p_capture_spawn"], a["slot"], d["slot"])
        got = got & (sp >= 0)
        if self.events is not None:
            self.events["captures_of_defeated"] += int(mx.sum(got).item())
            self.events["capture_rolls"] += int(mx.sum(where).item())
        cls = mx.where(self.tg(self.z["sp_sea"], sp), CL_NAVAL, mx.where(self.tg(self.z["sp_civilian"], sp), CL_CIV, CL_MIL))
        spot = self.free_tile_near(s, a["side"], a["tile"], cls, self.tg(self.z["sp_dest_id"], sp), got, land=self.tg(self.z["sp_land"], sp))
        # The new unit's allowance on its tile (`moves_on`: embarked on water).
        water = self.mg("water", spot[:, None])[:, 0] > 0
        mmax = mx.where(water & self.tg(self.z["sp_land"], sp) & self.tg(self.z["sp_embarks"], sp), self.tg(self.z["sp_moves_embarked"], sp), self.tg(self.z["sp_moves_land"], sp))
        s, _ = self.new_unit(s, a["side"], spot, got, mx.full((B,), CAPTURED_UNIT_HP, dtype=I32), mmax, self.tg(self.z["sp_kind"], sp), profile_from=sp)
        return s, got, spot

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
        maxhp = self.city_maxhp(s, cslot)
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
        dmg = self.unit_damage(c_rstr, strength, mx.zeros_like(roll).astype(I32), mx.zeros_like(roll).astype(I32), roll)
        dmg = mx.where(is_civ, exact.CIVILIAN_DAMAGE, dmg)
        dmg = mx.minimum(dmg, d["hp"])
        s["c_strike"] = put1(s["c_strike"], cslot, mx.ones(cslot.shape, dtype=mx.bool_), take)
        s, killed = self.strike_unit(s, d["slot"], dmg, is_unit | is_civ)
        s = self.set_unit(s, d["slot"], is_unit & ~killed, xp=g1(s["u_xp"], d["slot"]) + self.xp_gain(XP_DEF_RANGED, d["xp_percent"]))
        return s

    # ------------------------------------------------------------------ the turn

    def heal_dose(self, s, moved):
        """`heal::heal_amount` of every slot [B, U] (0 where `moved`): the rate by place, the medic next to it."""
        tiles = s["u_tile"]
        water = self.mg("water", tiles) > 0
        owner = gn(s["owner"], tiles)
        friendly = owner == s["u_side"]
        cityt = self.city_on(s, tiles) >= 0
        no = (s["u_hp"] >= 100) | (s["p_land"] & water) | moved | (s["p_sea"] & ~friendly & ~s["p_heal_outside_friendly"])
        no = no | (s["p_air"] & ~cityt)
        enemy_host = (owner >= 0) & (owner != s["u_side"]) & (owner < 2)
        rate = mx.where(cityt, 25 + mx.where(friendly, s["p_heal_friendly"], s["p_heal_neutral"]),
                        mx.where(~friendly, mx.where(enemy_host, 10 + s["p_heal_enemy"], 10 + s["p_heal_neutral"]), 20 + s["p_heal_friendly"]))
        # Medic: the best HealAdjacent of an own unit next to the unit.
        dd = self.geo.distance(tiles[:, :, None], tiles[:, None, :])
        same = (s["u_side"][:, :, None] == s["u_side"][:, None, :]) & s["u_alive"][:, None, :] & (tiles[:, None, :] >= 0)
        medic = mx.max(mx.where(same & (dd == 1), s["p_heal_adjacent"][:, None, :], 0), axis=2)
        return mx.where(no, 0, rate + medic)

    def hold_units(self, s, which, heal=True):
        """`act::hold_unit` of the units `which` [B, U] (the trainers' one end of a unit's turn): a unit that has not
        moved heals now by `heal_amount` (not an AlwaysHeal one: the end of the turn heals it), then its moves are
        spent. `heal=False`: the old harness (moves spent by hand, no heal)."""
        if heal:
            unmoved = which & (s["u_mv"] >= s["u_mmax"]) & ~s["p_always_heal"] & s["u_alive"] & (s["u_tile"] >= 0)
            dose = self.heal_dose(s, ~unmoved)
            s["u_hp"] = mx.where(unmoved & (dose > 0), mx.clip(s["u_hp"] + dose, 1, 100), s["u_hp"])
        s["u_mv"] = mx.where(which, 0, s["u_mv"])
        return s

    def end_turn(self, s, side, take):
        """The end of `side`'s turn: heal its units, the citadels' and mountains' damage, restore moves,
        attacks and interceptions, its cities heal and may strike again; then the other side's turn
        starts (fortification +1)."""
        tiles = s["u_tile"]
        mine = s["u_alive"] & (tiles >= 0) & (s["u_side"] == side[:, None]) & take[:, None]
        moved = (s["u_mv"] < s["u_mmax"]) & ~s["p_always_heal"]
        dose = self.heal_dose(s, moved)
        s["u_hp"] = mx.where(mine, mx.clip(s["u_hp"] + dose, 1, 100), s["u_hp"])
        water = self.mg("water", tiles) > 0
        if self.f["citadel"]:
            # Citadels (`citadel_damage`): the strongest intact citadel of the enemy next to the unit.
            nb = mx.take(self.geo.nbr, mx.maximum(tiles, 0), axis=0)  # [B, U, 6]
            cit = mx.where(nb >= 0, self.mg("citadel_dmg", nb), 0)
            host = mx.where(nb >= 0, gn(s["owner"], nb), -1)
            hurt = mx.max(mx.where((cit > 0) & (host == (1 - s["u_side"])[:, :, None]), cit, 0), axis=2)
            hit = mine & (hurt > 0)
            dead = hit & (hurt >= s["u_hp"])
            s["u_hp"] = mx.where(hit & ~dead, s["u_hp"] - hurt, s["u_hp"])
            s["u_alive"] = s["u_alive"] & ~dead
            s["u_tile"] = mx.where(dead, -1, s["u_tile"])
            mine = mine & ~dead
        if self.f["mountains"]:
            # Mountains (`mountain_damage`): a unit that crosses them loses 50 HP standing on one, never dying.
            on_m = mine & s["p_cross_mountains"] & (self.mg("mountain", tiles) > 0)
            s["u_hp"] = mx.where(on_m, s["u_hp"] - mx.minimum(MOUNTAIN_DAMAGE, s["u_hp"] - 1), s["u_hp"])
        mmax = mx.where(s["p_embarks"] & water & s["p_land"], s["p_moves_embarked"], s["p_moves_land"])
        s["u_att"] = mx.where(mine, 0, s["u_att"])
        s["u_icpt"] = mx.where(mine, 0, s["u_icpt"])
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
        if "n_unmodelled" not in s:
            s["n_unmodelled"] = mx.zeros((self.B,), dtype=I32)
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
