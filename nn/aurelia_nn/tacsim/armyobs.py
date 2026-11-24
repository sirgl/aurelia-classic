"""The army net's observation in MLX, batched over scenes: the twin of `crates/qa/src/army_net/observe.rs`
(work/nets/army-net/OBSERVATION-SCHEMA.md) on the simulator's state tables.

    python -m aurelia_nn.tacsim.armyobs DUMP.jsonl [--limit N] [--show 5]   # the parity check against Rust

From the tacsim state (`sim.py`: units by slot, cities, owners, sight) and what the play gives from above (the
orders: the objective, the army's slots, the map window's static planes; the far cities of both sides, outside
the window; the turn-start snapshot of the history, numbers 46 and 47), every scene of the batch at once:

- `tokens`: [B, 40, 48] (the objective; our units by id; the foe's units by id; our cities by tile; the foe's
  cities by tile), the distances [B, 40, 40] (clipped at 15), the window positions, the army slots;
- `candidates` of one unit per scene: "done", then the rules' legal list in its order (moves by tile, charges on
  units by handle, on cities by handle, fortify, shots by tile, set up, promotions by id), [B, 128, 40] with the
  promotion ids and positions; the strike previews (`act::preview`: the expected roll, the attacker as it would
  stand on the tile it strikes from) are the combat formulas of `sim.py` evaluated for many pairs at once.

Every number is rounded through f16, as Rust stores it. The parity source is `ARMY_OBS_DUMP` of `ai-exam suite`
(`army_net/obsdump.rs`): per play the scene line (the suite-dump scene, the orders, the kind table), per decision
of the net the state and the Rust builder's tokens and candidates.

Not twinned (counted apart by the parity check): units outside the 24 x 24 window (the simulator has no tiles
there), units the play created with a profile of the spawn table, the fog families' sight filter of the foes
(implemented, not yet checked: no fog family in the checked set), the history tokens of MH/MBH (the M and MB
nets read none), pillage.
"""

import argparse
import collections
import json
import sys
import time

import mlx.core as mx

from . import armyprice
from . import armythird
from . import exact
from .geometry import Geometry
from .scene import Batch, N_CTX, FLAG_DELTA
from .sim import Sim, BARBARIAN_BONUS, g1, gn, grow, put1, I32, ANY, ATTACK, DEFENSE, VS_CITY, ATT_OPEN, ATT_ROUGH, DEF_OPEN, DEF_ROUGH, \
    RATT_OPEN, RATT_ROUGH, DEF_VS_RANGED, RANGED_ATTACK, VS_WOUNDED, VS_FORTIFIED, FRIENDLY, OUTSIDE_FRIENDLY, ATT_FRIENDLY, \
    ADJ_FRIENDLY, CTX_FLANKING

F_TOK, F_ACT, T_MAX, C_MAX, D_MAX, AIMS, ARMS = 48, 40, 40, 128, 15, 8, 12
CITY_RANGE = 2
F32 = mx.float32
BIG = 1 << 28
# Candidate kinds (observe.rs `Kind`) and the rules' order of the command kinds (`knowledge::actions::order_key`).
K_DONE, K_MOVE, K_UNIT, K_CITY, K_RANGED, K_FORT, K_SETUP, K_PROMO, K_PILLAGE, K_OTHER = range(10)
GROUP = {K_MOVE: 0, K_UNIT: 4, K_CITY: 5, K_FORT: 6, K_RANGED: 25, K_SETUP: 27, K_PROMO: 28, K_PILLAGE: 31, K_OTHER: 34}
BASE = 2400
EXP_PART = 1199  # (RAND - 1): Roll::Expected adds (1200 - 1) x dr / 100 / 2


def f16(x):
    return x.astype(mx.float16).astype(F32)


def hexd(dq, dr):
    return (mx.abs(dq) + mx.abs(dr) + mx.abs(dq + dr)) // 2


def min_or(mask, d, axis, none=30):
    """min of `d` where `mask` along `axis`, `none` where nothing holds (Rust `.min().unwrap_or(30)`)."""
    m = mx.min(mx.where(mask, d, 1 << 20), axis=axis)
    return mx.where(m >= (1 << 20), none, m)


def first_index_eq(x, best):
    """The first index along the last axis where x == best [B] (ties: the first, as Rust's strict `>` scan)."""
    n = x.shape[-1]
    return mx.min(mx.where(x == best[:, None], mx.arange(n)[None, :], n), axis=-1).astype(I32)


def fdiv(a, b):
    return a.astype(F32) / b


class Tables:
    """What the play gives from above, per scene row (the template's orders, kinds, far cities)."""

    KIND_COLS = ["arm", "range", "moves", "civ", "str", "rstr", "land", "in_army", "base", "pillage", "icpt_range"]

    def __init__(self, scenes, U, C, tid):
        """`scenes`: the scene lines of the templates (obsdump.rs `start`); `tid` [B]: each row's template."""
        kinds = scenes[0]["kinds"]
        self.k = {}
        for j, name in enumerate(self.KIND_COLS):
            col = [row[j] for row in kinds]
            self.k[name] = mx.array(col, dtype=F32 if name == "base" else I32) if name not in ("civ", "land", "in_army", "pillage") else mx.array(col)
        nf = max(1, max(len(s["orders"]["far_cities"]) for s in scenes))
        cols = collections.defaultdict(list)
        for s in scenes:
            o = s["orders"]
            q0, r0, w, mw = o["window"]
            cols["q0"].append(q0); cols["r0"].append(r0); cols["mw"].append(mw)
            cols["aim"].append(o["aim"])
            t = o["tile"]
            cols["obj_q"].append(t % mw); cols["obj_r"].append(t // mw)
            ox, oy = t % mw - q0, t // mw - r0
            cols["obj_w"].append(oy * w + ox if 0 <= ox < w and 0 <= oy < w else -1)
            rf = o["refuge"]
            cols["ref_has"].append(rf >= 0); cols["ref_q"].append(max(rf, 0) % mw); cols["ref_r"].append(max(rf, 0) // mw)
            cols["optional"].append(o["optional"]); cols["start"].append(o["start_round"]); cols["rounds"].append(o["rounds"])
            cols["mean"].append(o["mean_strength"]); cols["fog"].append(o["fog"])
            army = [-1] * U
            for i, sl in enumerate(o["army"]):
                if 0 <= sl < U:
                    army[sl] = i
            cols["army"].append(army)
            far = o["far_cities"]
            cols["far_tile"].append([f[0] for f in far] + [-1] * (nf - len(far)))
            cols["far_side"].append([f[1] for f in far] + [-1] * (nf - len(far)))
            ch = o.get("city_handles", [])
            cols["c_handle"].append(ch + [-1] * (C - len(ch)))
            pg = o.get("promos", [])
            cols["promo_gid"].append(pg)
            cols["statics"].append(_unhex16(o["statics"], 576 * 16))
        npm = max(1, max(len(p) for p in cols["promo_gid"]))
        cols["promo_gid"] = [p + [1 << 20] * (npm - len(p)) for p in cols["promo_gid"]]
        tmpl = {}
        for k, v in cols.items():
            if k in ("ref_has", "optional", "fog"):
                tmpl[k] = mx.array(v)
            elif k == "mean":
                tmpl[k] = mx.array(v, dtype=F32)
            elif k == "statics":
                tmpl[k] = mx.array(v, dtype=F32).reshape(len(scenes), 576, 16)
            else:
                tmpl[k] = mx.array(v, dtype=I32)
        self.tmpl = tmpl
        t = {k: mx.take(v, tid, axis=0) for k, v in tmpl.items()}
        ft = t["far_tile"]
        t["far_q"] = mx.where(ft >= 0, ft % t["mw"][:, None], 0)
        t["far_r"] = mx.where(ft >= 0, ft // t["mw"][:, None], 0)
        self.t = t


def _unhex16(h, n):
    out = []
    for i in range(n):
        v = int(h[4 * i: 4 * i + 4], 16)
        out.append(_f16_bits(v))
    return out


def _f16_bits(v):
    sign = -1.0 if v & 0x8000 else 1.0
    e = (v >> 10) & 0x1F
    m = v & 0x3FF
    if e == 0:
        return sign * m * 2.0 ** -24
    if e == 31:
        return sign * float("inf") if m == 0 else float("nan")
    return sign * (1024 + m) * 2.0 ** (e - 25)


class Obs:
    """The builder over a simulator `sim` (its batch rows) and the tables `tab` of the same rows."""

    def __init__(self, sim, tab):
        self.sim, self.tab, self.geo = sim, tab, sim.geo
        self.t = tab.t
        # Static per batch (no host round-trip per call): any scene under fog; the most foe slots a row may have in
        # the candidates' blow previews (None: counted per call).
        self.fog_any = bool(mx.any(tab.t["fog"]).item())
        self.nfo = None

    # ------------------------------------------------------------------ shared facts

    def kind(self, s, name, slots=None):
        k = s["u_kind"] if slots is None else gn(s["u_kind"], slots)
        return mx.take(self.tab.k[name], mx.maximum(k, 0))

    def mapq(self, tile):
        """Map axial coordinates of window tiles [B, ...] (q0 + x, r0 + y)."""
        sh = (-1,) + (1,) * (tile.ndim - 1)
        q = mx.take(self.geo.q, mx.maximum(tile, 0)) + self.t["q0"].reshape(sh)
        r = mx.take(self.geo.r, mx.maximum(tile, 0)) + self.t["r0"].reshape(sh)
        return q, r

    def field(self, s, side=0):
        """The two sides' units and cities (observe.rs `Field`) as coordinates and masks. The learner's (side 0) foes are
        the enemy's units and cities only (`Field::of`: `orders.foe`, one nation; a third seat at war with us, the
        neutral of neutral-joins-war after its declaration, is no token and no foe of the observation); the other
        sides' scripts read the war table."""
        sim, t = self.sim, self.t
        B, U = s["u_alive"].shape
        foe = 1 - side
        uq, ur = self.mapq(s["u_tile"])
        far = s.get("u_far")
        alive = s["u_alive"] & (s["u_tile"] >= 0)
        if far is not None:
            # Units outside the window (the seat's new units in a far city, a piece that left): their map tiles.
            uq = mx.where(far, s["u_fq"], uq)
            ur = mx.where(far, s["u_fr"], ur)
            alive = alive | (s["u_alive"] & far)
        civ = self.kind(s, "civ")
        own = alive & (s["u_side"] == side)
        war = "war" in s and side != 0
        foes = alive & self.sim.is_foe(s, mx.full((B,), side, dtype=I32), s["u_side"]) if war else alive & (s["u_side"] == foe)
        if self.fog_any:
            seen = sim.seen_by(s, mx.full((B,), side, dtype=I32), mx.where(alive, s["u_tile"], -1))
            foes = foes & (seen | ~t["fog"][:, None])
        moves = self.kind(s, "moves")
        rng = self.kind(s, "range")
        reach = moves + mx.where(rng >= 0, rng, 1)
        ct = sim.cs["tile"]
        calive = sim.cs["alive"] & (ct >= 0)
        cq, cr = self.mapq(ct)
        # Cities of both sides: the window's (owner from the state) and the far ones (static).
        cq = mx.concatenate([cq, t["far_q"]], axis=1)
        cr = mx.concatenate([cr, t["far_r"]], axis=1)
        cside = mx.concatenate([mx.where(calive, s["c_owner"], -1), t["far_side"]], axis=1)
        return {"uq": uq, "ur": ur, "own": own, "foes": foes, "milfoe": foes & ~civ, "ownmil": own & ~civ, "civ": civ,
                "reach": reach, "cq": cq, "cr": cr, "own_city": cside == side, "foe_city": self.sim.is_foe(s, mx.full((B,), side, dtype=I32), cside) if war else (cside == foe), "side": side}

    def spot(self, s, fd, me, tq, tr, tile):
        """observe.rs `Spot::of` for tiles (map tq, tr; window `tile`) [B, N] of the unit `me` [B]."""
        uq, ur = fd["uq"], fd["ur"]
        mq, mr = g1(uq, me), g1(ur, me)
        U = uq.shape[1]
        d = hexd(tq[:, :, None] - uq[:, None, :], tr[:, :, None] - ur[:, None, :])  # [B, N, U]
        not_me = (mx.arange(U)[None, :] != me[:, None])[:, None, :]
        adj_foes = mx.sum(fd["milfoe"][:, None, :] & (d == 1), axis=2)
        friends = mx.sum(fd["ownmil"][:, None, :] & not_me & (d <= 1), axis=2)
        threat = mx.sum(fd["milfoe"][:, None, :] & (d <= fd["reach"][:, None, :]), axis=2)
        near = min_or(fd["milfoe"][:, None, :], d, 2)
        dc = hexd(tq[:, :, None] - fd["cq"][:, None, :], tr[:, :, None] - fd["cr"][:, None, :])
        fire = mx.any(fd["foe_city"][:, None, :] & (dc <= CITY_RANGE), axis=2)
        t = self.t
        obj = hexd(tq - t["obj_q"][:, None], tr - t["obj_r"][:, None])
        ref_c = min_or(fd["own_city"][:, None, :], dc, 2)
        refuge = mx.where(t["ref_has"][:, None], hexd(tq - t["ref_q"][:, None], tr - t["ref_r"][:, None]), ref_c)
        return {"dq": tq - mq[:, None], "dr": tr - mr[:, None], "def": self.defence(s, tile, fd["side"]),
                "hills": self.sim.mg("hills", tile) > 0, "rough": self.sim.mg("rough", tile) > 0, "fire": fire,
                "adj": adj_foes, "friends": friends, "threat": threat, "near": near, "obj": obj, "refuge": refuge,
                "on_obj": (tq == t["obj_q"][:, None]) & (tr == t["obj_r"][:, None]),
                "own_land": gn(s["owner"], tile) == fd["side"]}

    def defence(self, s, tile, side):
        """`tile_defense_percent` of tiles [B, N] for `side` (an int or [B, N])."""
        gdef = self.sim.mg("ground_def", tile)
        idef = self.sim.mg("improvement_def", tile)
        own = gn(s["owner"], tile)
        return gdef + mx.where((own < 0) | (own == side), idef, 0)

    @staticmethod
    def write_spot(sp, here, f):
        """Numbers 10..26 of candidates [B, N, F_ACT] (a list of columns)."""
        f[10] = fdiv(sp["dq"], 12.0)
        f[11] = fdiv(sp["dr"], 12.0)
        f[12] = fdiv(mx.maximum(mx.abs(sp["dq"]), mx.abs(sp["dr"])), 5.0)
        f[13] = fdiv(sp["def"], 100.0)
        f[14] = sp["hills"].astype(F32)
        f[15] = sp["rough"].astype(F32)
        f[16] = sp["fire"].astype(F32)
        f[17] = fdiv(sp["adj"], 3.0)
        f[18] = fdiv(sp["friends"], 3.0)
        f[19] = fdiv(sp["threat"], 4.0)
        f[20] = fdiv(sp["near"], 12.0)
        f[21] = fdiv(sp["near"] - here["near"], 5.0)
        f[22] = fdiv(sp["obj"], 12.0)
        f[23] = fdiv(sp["obj"] - here["obj"], 5.0)
        f[24] = fdiv(sp["refuge"], 12.0)
        f[25] = sp["on_obj"].astype(F32)
        f[26] = sp["own_land"].astype(F32)

    def promo_any(self, s):
        """[B, U]: the slots that may take a promotion now (`Sim.promo_mask` has one). A slot without the experience for its
        level has none, so only the slots some row holds ready are looked at (one host read)."""
        sim = self.sim
        B, U = s["u_alive"].shape
        level = mx.maximum(s["u_level"], 1)
        pct = mx.maximum(s["p_xp_needed_pct"], -100)
        need = mx.maximum((10 * level * (level + 1) // 2) * (100 + pct) // 100, 1)
        ready = s["u_alive"] & (s["u_xp"] >= need)
        rd = mx.any(ready, axis=0).tolist()
        out = []
        for u in range(U):
            slot = mx.full((B,), u, dtype=I32)
            out.append(mx.any(sim.promo_mask(s, slot, g1(s["u_alive"], slot)), axis=1) if rd[u] else mx.zeros((B,), dtype=mx.bool_))
        return mx.stack(out, axis=1)

    def statics(self, s, side=0):
        """The window's 16 static planes (`army_net::statics_of`, once per play at its start) [B, W*W, 16]."""
        sim, t = self.sim, self.t
        B = s["u_alive"].shape[0]
        T = sim.T
        tiles = mx.broadcast_to(mx.arange(T, dtype=I32)[None], (B, T))
        q, r = self.mapq(tiles)
        owner = s["owner"]
        city = sim.city_on(s, tiles)
        cown = mx.where(city >= 0, gn(mx.where(sim.cs["alive"], s["c_owner"], -1), mx.maximum(city, 0)), -1)
        river = mx.any(mx.take(sim.m["river"], sim.tid, axis=0), axis=2)
        pl = [mx.ones((B, T), dtype=F32), sim.mg("hills", tiles) > 0, sim.mg("mountain", tiles) > 0, sim.mg("water", tiles) > 0, sim.mg("rough", tiles) > 0,
              f16(fdiv(self.defence(s, tiles, side), 100.0)), river, sim.mg("road", tiles) > 0, owner == side, owner == 1 - side,
              sim.mg("improvement", tiles) >= 0, sim.mg("citadel_dmg", tiles) > 0,
              (q == t["obj_q"][:, None]) & (r == t["obj_r"][:, None]), t["ref_has"][:, None] & (q == t["ref_q"][:, None]) & (r == t["ref_r"][:, None]),
              cown == side, cown == 1 - side]
        return mx.stack([x.astype(F32) for x in pl], axis=2)

    # ------------------------------------------------------------------ tokens

    def tokens(self, s, rnd, prev_has, prev_tile_q, prev_tile_r, prev_hp, far_rows=None, side=0, far_unit_rows=None):
        """The tokens of every row (observe.rs `tokens`). `rnd` [B] the round; the history's snapshot per slot
        (`prev_*` [B, U]); `far_rows` [B, NF, F_TOK]: the far cities' rows (static: not in the window)."""
        sim, t = self.sim, self.t
        B, U = s["u_alive"].shape
        fd = self.field(s, side)
        foe = 1 - side
        army = t["army"]  # [B, U]: the army index (-1 none)
        in_army = army >= 0
        own, foes = fd["own"], fd["foes"]
        uq, ur = fd["uq"], fd["ur"]
        mean = t["mean"][:, None]
        rng = self.kind(s, "range")
        promos = self.promo_any(s) if sim.f["promo"] else mx.zeros((B, U), dtype=mx.bool_)
        promotable = own & in_army & promos
        may_shoot = (rng >= 0) & (s["u_att"] == 0)
        selectable = own & in_army & ((s["u_mv"] > 0) | promotable | may_shoot)
        # Unit rows [B, U, F_TOK].
        f = [mx.zeros((B, U), dtype=F32)] * F_TOK
        f[1] = own.astype(F32)
        f[2] = (~own).astype(F32)
        f[4] = fdiv(uq - t["obj_q"][:, None], 12.0)
        f[5] = fdiv(ur - t["obj_r"][:, None], 12.0)
        f[6] = fdiv(hexd(uq - t["obj_q"][:, None], ur - t["obj_r"][:, None]), 12.0)
        f[7] = fdiv(s["u_hp"], 100.0)
        f[8] = mx.where(s["u_mmax"] > 0, s["u_mv"].astype(F32) / mx.maximum(s["u_mmax"], 1).astype(F32), 0.0)
        f[9] = s["u_mmax"].astype(F32) / 60.0 / 5.0
        f[10] = (s["u_att"] == 0).astype(F32)
        f[11] = self.kind(s, "str").astype(F32) / mean
        f[12] = self.kind(s, "rstr").astype(F32) / mean
        f[13] = fdiv(mx.where(rng >= 0, rng, 0), 3.0)
        arm = self.kind(s, "arm")
        for a in range(ARMS + 1):
            f[14 + a] = (arm == a).astype(F32)
        st = s["u_st"]
        f[27] = ((st == 1) | (st == 2)).astype(F32)
        f[28] = (st == 3).astype(F32)
        water = sim.mg("water", s["u_tile"]) > 0
        f[29] = (self.kind(s, "land") & water).astype(F32)
        f[30] = fdiv(s["u_level"], 5.0)
        f[31] = fdiv(s["u_xp"], 60.0)
        f[32] = promotable.astype(F32)
        owner_side = mx.where(own, side, foe)
        f[33] = fdiv(self.defence(s, s["u_tile"], owner_side), 100.0)
        f[34] = (sim.mg("hills", s["u_tile"]) > 0).astype(F32)
        f[35] = (sim.mg("rough", s["u_tile"]) > 0).astype(F32)
        d = hexd(uq[:, :, None] - uq[:, None, :], ur[:, :, None] - ur[:, None, :])  # [B, U(this), U(other)]
        notself = ~mx.eye(U, dtype=mx.bool_)[None]
        dc = hexd(uq[:, :, None] - fd["cq"][:, None, :], ur[:, :, None] - fd["cr"][:, None, :])
        fire = mx.any(fd["foe_city"][:, None, :] & (dc <= CITY_RANGE), axis=2)
        adj = mx.sum(fd["milfoe"][:, None, :] & (d == 1), axis=2)
        friends = mx.sum(fd["ownmil"][:, None, :] & notself & (d <= 1), axis=2)
        threat = mx.sum(fd["milfoe"][:, None, :] & (d <= fd["reach"][:, None, :]), axis=2)
        near = min_or(fd["milfoe"][:, None, :], d, 2)
        ref_c = min_or(fd["own_city"][:, None, :], dc, 2)
        refuge = mx.where(t["ref_has"][:, None], hexd(uq - t["ref_q"][:, None], ur - t["ref_r"][:, None]), ref_c)
        near_own = mx.sum(fd["ownmil"][:, None, :] & (d == 1), axis=2)
        reach_own = mx.sum(fd["ownmil"][:, None, :] & (d <= fd["reach"][:, None, :]), axis=2)
        nearest_own = min_or(fd["ownmil"][:, None, :], d, 2)
        z = mx.zeros((B, U), dtype=F32)
        f[36] = mx.where(own, fire.astype(F32), z)
        f[37] = mx.where(own, fdiv(adj, 3.0), fdiv(near_own, 3.0))
        f[38] = mx.where(own, fdiv(friends, 3.0), z)
        f[39] = mx.where(own, fdiv(threat, 4.0), fdiv(reach_own, 4.0))
        f[40] = mx.where(own, fdiv(near, 12.0), fdiv(nearest_own, 12.0))
        f[41] = mx.where(own, fdiv(refuge, 12.0), z)
        f[42] = selectable.astype(F32)
        pq, pr = prev_tile_q, prev_tile_r
        f[46] = mx.where(prev_has, (s["u_hp"].astype(F32) - prev_hp.astype(F32)) / 100.0, 0.0)
        f[47] = mx.where(prev_has & ((pq != uq) | (pr != ur)), 1.0, 0.0)
        urows = mx.stack(f, axis=2)
        if far_unit_rows is not None and s.get("u_far") is not None:
            urows = mx.where(s["u_far"][:, :, None], far_unit_rows, urows)
        # Window city rows [B, C, F_TOK].
        ct = sim.cs["tile"]
        C = ct.shape[1]
        calive = sim.cs["alive"] & (ct >= 0)
        cq, cr = self.mapq(ct)
        mil, _ = sim.occupancy(s)
        cstr = self.city_strength_n(s, mx.broadcast_to(mx.arange(C, dtype=I32)[None], (B, C)), mil)
        maxhp = s.get("c_maxhp", sim.cs["max_hp"])
        cown = s["c_owner"] == side
        g = [mx.zeros((B, C), dtype=F32)] * F_TOK
        g[3] = mx.ones((B, C), dtype=F32)
        g[4] = fdiv(cq - t["obj_q"][:, None], 12.0)
        g[5] = fdiv(cr - t["obj_r"][:, None], 12.0)
        g[6] = fdiv(hexd(cq - t["obj_q"][:, None], cr - t["obj_r"][:, None]), 12.0)
        g[7] = (maxhp - s["c_dmg"]).astype(F32) / mx.maximum(maxhp, 1).astype(F32)
        g[11] = cstr.astype(F32) / 100.0 / mean
        g[12] = (~s["c_strike"]).astype(F32)
        g[43] = ((cq == t["obj_q"][:, None]) & (cr == t["obj_r"][:, None])).astype(F32)
        g[44] = fdiv(s["c_pop"], 10.0)
        g[45] = cown.astype(F32)
        crows = mx.stack(g, axis=2)
        NF = t["far_tile"].shape[1]
        if far_rows is None:
            far_rows = mx.zeros((B, NF, F_TOK), dtype=F32)
        # The order: our units by id, theirs by id, our cities by tile, theirs by tile; at most T_MAX - 1.
        uid = s["u_id"]
        ukey = mx.where(own, 1 * (1 << 24) + uid, mx.where(foes, 2 * (1 << 24) + uid, BIG))
        ctile = mx.where(ct >= 0, cr * t["mw"][:, None] + cq, 0)
        cside = mx.where(calive, s["c_owner"], -1)
        ckey = mx.where(cside == side, 3 * (1 << 24) + ctile, mx.where(cside == foe, 4 * (1 << 24) + ctile, BIG))
        fside = t["far_side"]
        fkey = mx.where(fside == side, 3 * (1 << 24) + t["far_tile"], mx.where(fside == foe, 4 * (1 << 24) + t["far_tile"], BIG))
        key = mx.concatenate([ukey, ckey, fkey], axis=1)
        rows = mx.concatenate([urows, crows, far_rows], axis=1)
        E = key.shape[1]
        n_ent = min(E, T_MAX - 1)
        order = mx.argsort(key, axis=1)[:, :n_ent]
        okey = mx.take_along_axis(key, order, axis=1)
        valid = okey < BIG
        trow = mx.take_along_axis(rows, order[:, :, None], axis=1)
        # The objective's row.
        o = [mx.zeros((B,), dtype=F32)] * F_TOK
        o[0] = mx.ones((B,), dtype=F32)
        for a in range(AIMS):
            o[7 + a] = (t["aim"] == a).astype(F32)
        left = mx.maximum(t["start"] + t["rounds"] - rnd, 0)
        o[7 + AIMS] = fdiv(left, 30.0)
        o[8 + AIMS] = fdiv(t["rounds"], 30.0)
        o[9 + AIMS] = t["optional"].astype(F32)
        own_army = own & in_army
        o[10 + AIMS] = fdiv(mx.sum(own_army, axis=1), 8.0)
        o[11 + AIMS] = fdiv(mx.sum(fd["milfoe"], axis=1), 8.0)
        o[12 + AIMS] = mx.sum(mx.where(own_army, s["u_hp"], 0), axis=1).astype(F32) / 800.0
        o[13 + AIMS] = mx.sum(mx.where(fd["milfoe"], s["u_hp"], 0), axis=1).astype(F32) / 800.0
        orow = mx.stack(o, axis=1)[:, None, :]
        feat = mx.concatenate([orow, mx.where(valid[:, :, None], trow, 0.0)], axis=1)
        feat = f16(feat)
        if feat.shape[1] < T_MAX:
            feat = mx.pad(feat, [(0, 0), (0, T_MAX - feat.shape[1]), (0, 0)])
            valid = mx.pad(valid, [(0, 0), (0, T_MAX - 1 - valid.shape[1])])
            order = mx.pad(order, [(0, 0), (0, T_MAX - 1 - order.shape[1])])
        tmask = mx.concatenate([mx.ones((B, 1), dtype=mx.bool_), valid], axis=1)
        # Tiles of the tokens: map coordinates, window positions; references (unit slot, city index).
        far_w = mx.full(t["far_tile"].shape, -1, dtype=I32)
        eq = mx.concatenate([uq, cq, t["far_q"]], axis=1)
        er = mx.concatenate([ur, cr, t["far_r"]], axis=1)
        ew = mx.concatenate([s["u_tile"], ct, far_w], axis=1)
        tq = mx.concatenate([t["obj_q"][:, None], mx.take_along_axis(eq, order, axis=1)], axis=1)
        tr = mx.concatenate([t["obj_r"][:, None], mx.take_along_axis(er, order, axis=1)], axis=1)
        pos = mx.concatenate([t["obj_w"][:, None], mx.take_along_axis(ew, order, axis=1)], axis=1)
        pos = mx.where(tmask, pos, -1)
        dist = mx.clip(hexd(tq[:, :, None] - tq[:, None, :], tr[:, :, None] - tr[:, None, :]), 0, D_MAX)
        dist = mx.where(tmask[:, :, None] & tmask[:, None, :], dist, 0)
        ent = mx.concatenate([mx.full((B, 1), -1, dtype=I32), order.astype(I32)], axis=1)
        ent = mx.where(tmask, ent, -1)
        unit_slot = mx.where((ent >= 0) & (ent < U), ent, -1)
        aslot = mx.where(unit_slot >= 0, gn(army, unit_slot), -1)
        aslot = mx.where(aslot < 16, aslot, -1)
        sel = mx.where(unit_slot >= 0, gn(selectable.astype(I32), unit_slot) > 0, False)
        return {"feat": feat, "mask": tmask, "n": mx.sum(tmask, axis=1), "dist": dist.astype(I32), "pos": pos, "slot": aslot,
                "unit": unit_slot, "ent": ent, "selectable": sel}

    # ------------------------------------------------------------------ the strike preview

    def preview(self, s, me, frm, to, valid, from_real, ranged_only=False):
        """`act::preview` of unit `me` [B] striking the tiles `to` [B, N] from `frm` [B, N] (`plan_attack_from`;
        `from_real`: from where it stands, `plan_attack`): (dealt, taken, defender hp, ok) [B, N], expected roll.
        `ranged_only`: every row's unit is a ranged one (the melee strengths and the answer are not computed)."""
        sim, geo = self.sim, self.geo
        B, U = s["u_alive"].shape
        N = to.shape[1]
        side = g1(s["u_side"], me)
        P = lambda k: g1(s["p_" + k], me)[:, None]
        how_ranged = (g1(self.kind(s, "range"), me) >= 0)[:, None]
        civ_kind = g1(self.kind(s, "civ"), me)[:, None]
        base_c, base_r = P("base_combat"), P("base_ranged")
        allowed = 1 + mx.clip(P("attacks_per_turn"), 0, 9)
        ready = ~civ_kind & ((base_c > 0) | (base_r > 0)) & (g1(s["u_mv"], me)[:, None] > 0) & (g1(s["u_att"], me)[:, None] < allowed)
        water_from = sim.mg("water", frm) > 0
        water_to = sim.mg("water", to) > 0
        land = P("land")
        embarked = land & water_from
        city = sim.city_on(s, to)
        is_city = city >= 0
        d_ft = geo.distance(frm, to)
        # Melee reach.
        melee = ~P("only_defensive") & ~P("air") & (base_c > 0) & (d_ft == 1)
        melee = melee & mx.where(P("city_attack_only"), is_city, True)
        melee = melee & mx.where(land, ~water_to | embarked, mx.where(P("sea"), water_to | is_city, False))
        # Ranged reach.
        rng = P("range")
        must = P("must_set_up")
        st_me = mx.where(from_real, g1(s["u_st"], me)[:, None], mx.where(must, 3, g1(s["u_st"], me)[:, None]))
        rr = (base_r > 0) & ((rng > 0) | P("ranged_support_fire")) & ~embarked
        rr = rr & mx.where(must, st_me == 3, True)
        rr = rr & (d_ft >= 1) & (d_ft <= mx.maximum(rng, 1))
        allt = mx.where(valid, to, -1)
        vis = sim.seen_by(s, side, allt)
        rr = rr & vis
        los = sim.los_bit(frm, to)
        rr = rr & (P("indirect_fire") | P("air") | los)
        rr = rr & mx.where(P("ranged_only_in_domain"), water_to & ~is_city, True)
        reach_ok = mx.where(how_ranged, rr, melee)
        # The defender: a city first (not ours), else the foe's combat unit, else its civilian (seen units only).
        mil, civ = sim.occupancy(s)
        c_owner = mx.where(is_city, gn(s["c_owner"], city), -1)
        m_sl, c_sl = gn(mil, to), gn(civ, to)
        m_side = mx.where(m_sl >= 0, gn(s["u_side"], m_sl), -1)
        c_side = mx.where(c_sl >= 0, gn(s["u_side"], c_sl), -1)
        own_there = (m_side == side[:, None]) | (c_side == side[:, None])
        seen_t = vis
        foe_mil = sim.is_foe(s, side[:, None], m_side) & seen_t
        foe_civ = sim.is_foe(s, side[:, None], c_side) & seen_t
        city_def = is_city & sim.is_foe(s, side[:, None], c_owner)
        unit_def = ~is_city & ~own_there & foe_mil
        civ_def = ~is_city & ~own_there & ~foe_mil & foe_civ
        ok = valid & ready & reach_ok & (city_def | unit_def | civ_def)
        dslot = mx.where(unit_def, m_sl, mx.where(civ_def, c_sl, -1))
        # Strengths.
        a = self.fighter_n(s, me, frm, st_me, N)
        dd = self.fighter_n_slots(s, dslot, unit_def)
        att_r = self.ranged_strength(s, a, dd, is_city & city_def, to, True)
        def_r = self.defense_vs_ranged(s, dd, a)
        cstr = self.city_strength_n(s, city, mil)
        c_max = gn(s.get("c_maxhp", sim.cs["max_hp"]), mx.maximum(city, 0))
        c_hp = c_max - gn(s["c_dmg"], mx.maximum(city, 0))
        ranged_atk = how_ranged
        if ranged_only:
            att, dfn = att_r, mx.where(city_def, cstr, def_r)
        else:
            att_m = self.melee_attack_strength(s, a, dd, to, is_city & city_def)
            def_m = self.defense_strength(s, dd, a, False)
            att = mx.where(ranged_atk, att_r, att_m)
            dfn = mx.where(city_def, cstr, mx.where(ranged_atk, def_r, def_m))
        my_dmg = 100 - g1(s["u_hp"], me)[:, None]
        wm = P("wounded_mod")
        dealt = expected_damage(att, dfn, mx.broadcast_to(my_dmg, att.shape), mx.broadcast_to(wm, att.shape))
        d_hp = mx.where(city_def, c_hp, gn(s["u_hp"], mx.maximum(dslot, 0)))
        d_wm = gn(s["p_wounded_mod"], mx.maximum(dslot, 0))
        if ranged_only:
            taken = mx.zeros_like(att)
        else:
            ans_city = expected_damage(cstr, att_m, mx.zeros_like(att), mx.zeros_like(att))
            ans_unit = expected_damage(def_m, att_m, 100 - d_hp, d_wm)
            taken = mx.where(ranged_atk | civ_def, 0, mx.where(city_def, ans_city, ans_unit))
        dealt = mx.where(civ_def, mx.where(ranged_atk, exact.CIVILIAN_DAMAGE, 0), dealt)
        return dealt.astype(I32), taken.astype(I32), d_hp.astype(I32), ok

    def preview_packed(self, s, me, frm, to, valid, from_real):
        """`preview` of the pairs where `valid` [B, N] holds, packed: each valid pair is a row of its own (the state gathered
        for it), not a column of a padded [B, N] (most rows have none or one). The results are zero where not valid."""
        B, N = valid.shape
        flat = valid.reshape(-1)
        P = int(mx.sum(flat).item())
        zero = mx.zeros((B, N), dtype=I32)
        if P == 0:
            return zero, zero, zero, mx.zeros((B, N), dtype=mx.bool_)
        if 2 * P >= B * N:
            return self.preview(s, me, frm, to, valid, from_real)
        idx = mx.argsort(mx.where(flat, 0, B * N) + mx.arange(B * N, dtype=I32))[:P].astype(I32)
        row = idx // N
        sub = {k: mx.take(v, row, axis=0) for k, v in s.items()}
        sim = self.sim
        saved = (sim.cs, sim.tid, sim.n_pieces, sim.B, self.tab, self.t)
        view = _rows_sim(sim, row)
        sim.cs, sim.tid, sim.n_pieces, sim.B = view.cs, view.tid, view.n_pieces, view.B
        self.tab = _rows_tab(self.tab, row)
        self.t = self.tab.t
        try:
            dealt, taken, dhp, ok = self.preview(sub, mx.take(me, row), mx.take(frm.reshape(-1), idx)[:, None], mx.take(to.reshape(-1), idx)[:, None],
                                                 mx.ones((P, 1), dtype=mx.bool_), from_real)
        finally:
            sim.cs, sim.tid, sim.n_pieces, sim.B, self.tab, self.t = saved
        back = lambda v: mx.zeros((B * N,), dtype=I32).at[idx].add(v[:, 0].astype(I32)).reshape(B, N)
        return back(dealt), back(taken), back(dhp), back(ok) > 0

    # The fighters of the preview: [B, N] fields (the attacker as it would stand on `frm`, the defenders by slot).

    def fighter_n(self, s, me, frm, st, N):
        B = me.shape[0]
        f = {"slot": mx.broadcast_to(me[:, None], (B, N)), "valid": mx.ones((B, N), dtype=mx.bool_), "tile": frm, "st": st}
        for k in ("u_side", "u_hp", "u_mv", "u_mmax"):
            f[k[2:]] = mx.broadcast_to(g1(s[k], me)[:, None], (B, N))
        for k in Sim.FIGHTER_KEYS:
            f[k] = mx.broadcast_to(g1(s["p_" + k], me)[:, None], (B, N))
        for k, src in (("ctx", "p_ctx"), ("tctx", "p_tctx"), ("ttype", "p_ttype"), ("targ", "p_targ"), ("targ2", "p_targ2"), ("tpct", "p_tpct"), ("tprom", "p_tprom")):
            v = grow(s[src], me)
            f[k] = mx.broadcast_to(v[:, None], (B, N) + v.shape[1:])
        # The held promotions are read only where a context gates on one: by (row of the slot, promotion), not gathered whole.
        f["prom_row"] = (s["u_prom"], mx.broadcast_to((mx.arange(B, dtype=I32) * s["u_prom"].shape[1] + mx.clip(me, 0, s["u_prom"].shape[1] - 1))[:, None], (B, N)))
        f["water"] = self.sim.mg("water", frm) > 0
        f["embarked"] = f["land"] & f["water"]
        return f

    def fighter_n_slots(self, s, slots, valid):
        f = {"slot": slots, "valid": valid & (slots >= 0)}
        sl = mx.clip(slots, 0, s["u_alive"].shape[1] - 1)
        for k in ("u_tile", "u_side", "u_hp", "u_st", "u_mv", "u_mmax"):
            f[k[2:]] = gn(s[k], sl)
        for k in Sim.FIGHTER_KEYS:
            f[k] = gn(s["p_" + k], sl)
        B, N = slots.shape
        U = s["u_alive"].shape[1]
        row = mx.arange(B, dtype=I32)[:, None] * U + sl
        for k, src in (("ctx", "p_ctx"), ("tctx", "p_tctx"), ("ttype", "p_ttype"), ("targ", "p_targ"), ("targ2", "p_targ2"), ("tpct", "p_tpct"), ("tprom", "p_tprom")):
            arr = s[src]
            rest = arr.shape[2:]
            f[k] = mx.take(arr.reshape((B * U,) + rest), row.reshape(-1), axis=0).reshape((B, N) + rest)
        f["prom_row"] = (s["u_prom"], row)
        f["water"] = self.sim.mg("water", f["tile"]) > 0
        f["embarked"] = f["land"] & f["water"]
        return f

    def ctxset(self, cols, shape):
        out = [mx.zeros(shape, dtype=mx.bool_)] * N_CTX
        for i, v in cols.items():
            out[i] = mx.broadcast_to(v, shape)
        return mx.stack(out, axis=-1)

    def context_sum(self, s, f, ctx, battle, other):
        sim = self.sim
        total = mx.sum(mx.where(ctx, f["ctx"], 0), axis=-1)
        in_ctx = mx.take_along_axis(ctx, f["tctx"], axis=-1)
        tt, ta = f["ttype"], f["targ"]
        feat = sim.mg("feature", battle)[..., None]
        tkind = sim.mg("terrain_kind", battle)[..., None]
        ov = other["valid"][..., None]
        holds = (tt == 1) & (feat == ta)
        holds = holds | ((tt == 2) & (feat < 0) & (tkind == ta))
        holds = holds | ((tt == 3) & ov & (other["role"][..., None] == ta))
        holds = holds | ((tt == 4) & ov & (other["domain"][..., None] == ta))
        holds = holds | ((tt == 5) & ov & (other["class"][..., None] == ta) & (ta > 0))
        holds = holds | (tt == 6)
        if sim.has_near_imp:
            holds = holds | ((tt == 7) & self.near_improvement(s, f))
        tp = f["tprom"]
        up, row = f["prom_row"]
        gate = (tp < 0) | mx.take(up.reshape(-1), row[..., None] * up.shape[2] + mx.maximum(tp, 0))
        return total + mx.sum(mx.where(in_ctx & holds & gate, f["tpct"], 0), axis=-1)

    def near_improvement(self, s, f):
        geo, sim = self.geo, self.sim
        disc = mx.take(geo.att_tile, mx.maximum(f["tile"], 0), axis=0)  # [B, N, OA]
        imp = mx.where(disc >= 0, sim.mg("improvement", disc), -1)
        B = disc.shape[0]
        own = mx.where(disc >= 0, gn(s["owner"], disc.reshape(B, -1)).reshape(disc.shape), -1) == f["side"][..., None]
        d = geo.att_dist
        hit = (imp[..., None, :] == f["targ"][..., :, None]) & own[..., None, :] & (d <= f["targ2"][..., :, None])
        return mx.any(hit, axis=-1)

    def generic(self, s, f, other, adjacency):
        """`Sim.generic` for [B, N] fighters: generals, enemy auras, flanking (melee attack only)."""
        geo = self.geo
        B, U = s["u_alive"].shape
        tiles = s["u_tile"][:, None, :]  # [B, 1, U]
        dist = lambda a, b: geo.distance(mx.broadcast_to(a, mx.broadcast_shapes(a.shape, b.shape)), mx.broadcast_to(b, mx.broadcast_shapes(a.shape, b.shape)))
        d_me = dist(tiles, f["tile"][..., None])
        alive = (s["u_alive"] & (s["u_tile"] >= 0))[:, None, :]
        slots = mx.arange(U)[None, None, :]
        own = alive & (s["u_side"][:, None, :] == f["side"][..., None]) & (slots != f["slot"][..., None])
        gg = own & s["p_great_general"][:, None, :] & (d_me <= 2)
        near = mx.any(gg, axis=-1)
        stacked = mx.max(mx.where(gg & (d_me == 0), s["p_stacked_general"][:, None, :], 0), axis=-1)
        m = mx.where(near & f["land"], 15 + stacked + f["general_aura"], 0)
        enemy = alive & self.sim.is_foe(s, f["side"][..., None], s["u_side"][:, None, :])
        aura = enemy & (s["p_enemy_aura"][:, None, :] != 0) & (d_me <= 2) & (d_me <= s["p_aura_range"][:, None, :])
        m = m + mx.where(f["air"], 0, mx.sum(mx.where(aura, s["p_enemy_aura"][:, None, :], 0), axis=-1))
        if other is not None and adjacency:
            d_o = dist(tiles, other["tile"][..., None])
            flank = own & ~s["p_civilian"][:, None, :] & ~s["p_air"][:, None, :] & (s["p_strength"][:, None, :] > 0) & (s["p_ranged_strength"][:, None, :] == 0) & (d_o == 1)
            count = mx.sum(flank.astype(I32), axis=-1)
            fl = 10 * count
            raise_ = f["ctx"][..., CTX_FLANKING]
            fl = mx.where(raise_ > 0, fl * (100 + raise_) // 100, fl)
            m = m + mx.where(other["valid"], fl, 0)
        if other is not None and "barb" in s:
            m = m + mx.where(s["barb"][:, None] & other["valid"] & (other["side"] == 1) & (f["side"] != 1), BARBARIAN_BONUS, 0)
        return m

    def friendly_adjacent(self, s, f):
        geo, sim = self.geo, self.sim
        B, U = s["u_alive"].shape
        tiles = s["u_tile"][:, None, :]
        sh = mx.broadcast_shapes(tiles.shape, f["tile"][..., None].shape)
        d = geo.distance(mx.broadcast_to(tiles, sh), mx.broadcast_to(f["tile"][..., None], sh))
        water = (sim.mg("water", s["u_tile"]) > 0)[:, None, :]
        own = (s["u_alive"] & (s["u_tile"] >= 0))[:, None, :] & (s["u_side"][:, None, :] == f["side"][..., None]) & ~s["p_civilian"][:, None, :] \
            & ~s["p_air"][:, None, :] & (mx.arange(U)[None, None, :] != f["slot"][..., None])
        return mx.any(own & (d == 1) & (water == f["water"][..., None]), axis=-1)

    def near_sapper(self, s, side, ctile):
        geo = self.geo
        tiles = s["u_tile"][:, None, :]
        sh = mx.broadcast_shapes(tiles.shape, ctile[..., None].shape)
        d = geo.distance(mx.broadcast_to(tiles, sh), mx.broadcast_to(ctile[..., None], sh))
        ok = (s["u_alive"] & (s["u_tile"] >= 0))[:, None, :] & (s["u_side"][:, None, :] == side[..., None]) & s["p_sapper"][:, None, :]
        return mx.any(ok & (d <= 2), axis=-1)

    def tile_defense(self, s, tile, side):
        return self.defence(s, tile, side)

    def melee_attack_strength(self, s, a, d, to, vs_city):
        sim, geo = self.sim, self.geo
        shape = to.shape
        friendly = gn(s["owner"], to) == a["side"]
        rough = sim.mg("rough", to) > 0
        dv = d["valid"]
        tr = mx.ones(shape, dtype=mx.bool_)
        cols = {ANY: tr, ATTACK: tr, VS_CITY: vs_city, ATT_OPEN: ~vs_city & ~rough, ATT_ROUGH: ~vs_city & rough,
                FRIENDLY: friendly, ATT_FRIENDLY: friendly, OUTSIDE_FRIENDLY: ~friendly,
                VS_WOUNDED: dv & (d["hp"] < 100), VS_FORTIFIED: dv & ((d["st"] == 1) | (d["st"] == 2)),
                ADJ_FRIENDLY: self.friendly_adjacent(s, a)}
        ctx = self.ctxset(cols, shape)
        m = self.context_sum(s, a, ctx, to, d) + self.generic(s, a, d, True)
        m = m + mx.where(vs_city & self.near_sapper(s, a["side"], to), 50, 0)
        nb = mx.take(geo.nbr, mx.maximum(a["tile"], 0), axis=0)  # [B, N, 6]
        hit = nb == to[..., None]
        dirs = mx.max(mx.where(hit, mx.arange(6), -1), axis=-1)
        tid = sim.tid.reshape((-1,) + (1,) * (to.ndim - 1))
        river = mx.take(sim.m["river"].reshape(-1), tid * (sim.T * 6) + mx.maximum(a["tile"], 0) * 6 + mx.maximum(dirs, 0)) & (dirs >= 0)
        m = m + mx.where(river & ~(a["amphibious"] | a["river_crossing"]), -20, 0)
        water_to = sim.mg("water", to) > 0
        m = m + mx.where(a["land"] & a["water"] & ~water_to & ~a["amphibious"], -50, 0)
        strength = mx.maximum(a["base_combat"].astype(I32) * (100 + m), 100)
        heavy = a["heavy_charge"] & dv & (d["withdraw"] <= 0)
        strength = mx.where(heavy, strength * 150 // 100, strength)
        return mx.where(a["embarked"] & water_to, a["embarked_defense"], strength)

    def defense_strength(self, s, d, a, ranged):
        sim = self.sim
        tile = d["tile"]
        shape = tile.shape
        rough = sim.mg("rough", tile) > 0
        friendly = gn(s["owner"], tile) == d["side"]
        tr = mx.ones(shape, dtype=mx.bool_)
        rg = mx.full(shape, bool(ranged))
        cols = {ANY: tr, DEFENSE: tr, DEF_VS_RANGED: rg, DEF_OPEN: ~rough, DEF_ROUGH: rough, FRIENDLY: friendly, OUTSIDE_FRIENDLY: ~friendly}
        if not ranged:
            cols[ADJ_FRIENDLY] = self.friendly_adjacent(s, d)
        ctx = self.ctxset(cols, shape)
        m = self.context_sum(s, d, ctx, tile, a) + self.generic(s, d, a, False)
        terrain = mx.where(d["water"], 0, self.defence(s, tile, d["side"]))
        m = m + mx.where(~d["no_defensive_bonus"] | (terrain < 0), terrain, 0)
        fort = mx.where((d["st"] == 1) | (d["st"] == 2), d["st"], 0)
        m = m + mx.where(~d["ranged_support_fire"], fort * 20, 0)
        return mx.where(d["embarked"], d["embarked_defense"], mx.maximum(d["base_combat"].astype(I32) * (100 + m), 100))

    def ranged_strength(self, s, me, other, vs_city, target, attacking):
        sim = self.sim
        shape = target.shape
        rough = sim.mg("rough", target) > 0
        friendly = gn(s["owner"], target) == me["side"]
        ov = other["valid"]
        tr = mx.ones(shape, dtype=mx.bool_)
        unit_target = ~vs_city if attacking else mx.zeros(shape, dtype=mx.bool_)
        cols = {ANY: tr, VS_CITY: vs_city}
        if attacking:
            cols.update({ATTACK: tr, RANGED_ATTACK: tr, FRIENDLY: friendly, OUTSIDE_FRIENDLY: ~friendly,
                         RATT_OPEN: unit_target & ~rough, RATT_ROUGH: unit_target & rough})
        else:
            cols.update({DEFENSE: tr, DEF_VS_RANGED: tr})
        cols[VS_WOUNDED] = ov & (other["hp"] < 100)
        cols[VS_FORTIFIED] = ov & ((other["st"] == 1) | (other["st"] == 2))
        ctx = self.ctxset(cols, shape)
        m = self.context_sum(s, me, ctx, target, other) + self.generic(s, me, other, False)
        m = m + mx.where(vs_city & self.near_sapper(s, me["side"], target), 50, 0)
        if not attacking:
            terrain = self.defence(s, me["tile"], me["side"])
            m = m + mx.where(~me["no_defensive_bonus"] | (terrain < 0), terrain, 0)
        return mx.maximum(me["base_ranged"].astype(I32) * (100 + m), 100)

    def defense_vs_ranged(self, s, d, a):
        ranged_land = ~d["ranged_support_fire"] & ~d["sea"] & (d["base_ranged"] > 0)
        rs = self.ranged_strength(s, d, a, mx.zeros(d["tile"].shape, dtype=mx.bool_), d["tile"], False)
        ds = self.defense_strength(s, d, a, True)
        return mx.where(d["embarked"], d["embarked_defense"], mx.where(ranged_land, rs, ds))

    def city_strength_n(self, s, city, mil):
        """`Sim.city_strength` for city slots [B, N] (-1: 1)."""
        sim = self.sim
        c = mx.maximum(city, 0)
        ct = gn(sim.cs["tile"], c)
        owner = gn(s["c_owner"], c)
        if "c_str0" in s:
            base = gn(s["c_str0"], c)
        else:
            base = gn(sim.cs["str0"], c) + (gn(s["c_pop"], c) - gn(sim.cs["pop0"], c)) * 40
        gslot = gn(mil, mx.maximum(ct, 0))
        has_g = (gslot >= 0) & (ct >= 0)
        gs = mx.maximum(gslot, 0)
        garrison = has_g & (gn(s["u_side"], gs) == owner) & gn(s["p_land"], gs) & (gn(s["p_strength"], gs) > 0)
        g = gn(s["p_strength"], gs) * 100 * gn(s["u_hp"], gs) // 100
        st = base + mx.where(garrison, g * 100 // 500, 0)
        return mx.where(city >= 0, mx.maximum(st, 1), 1)

    # ------------------------------------------------------------------ candidates

    def candidates(self, s, me, side=0):
        """The legal actions of unit `me` [B] (observe.rs `candidates`): "done" first, then the rules' order."""
        sim, geo, t = self.sim, self.geo, self.t
        B, U = s["u_alive"].shape
        sd = mx.full((B,), side, dtype=I32)
        fd = self.field(s, side)
        mk = sim.masks(s, sd, me)
        tile = g1(s["u_tile"], me)
        mq, mr = g1(fd["uq"], me), g1(fd["ur"], me)
        here = self.spot(s, fd, me, mq[:, None], mr[:, None], tile[:, None])
        mean = t["mean"][:, None]
        mmax = g1(s["u_mmax"], me)
        # Moves: the reachable tiles (its own aside) where the rules let it stop.
        left, lt = mk["left"], mk["lt"]
        L = lt.shape[1]
        mil, civ = sim.occupancy(s)
        civilian = g1(s["p_civilian"], me)
        ms_l = mx.where(lt >= 0, gn(mil, lt), -1)
        cs_l = mx.where(lt >= 0, gn(civ, lt), -1)
        ms_side = mx.where(ms_l >= 0, gn(s["u_side"], ms_l), -1)
        cs_side = mx.where(cs_l >= 0, gn(s["u_side"], cs_l), -1)
        foreign = ((ms_side >= 0) & (ms_side != side)) | ((cs_side >= 0) & (cs_side != side))
        own_same = mx.where(civilian[:, None], cs_side == side, ms_side == side)
        alive = g1(s["u_alive"], me) & (g1(s["u_side"], me) == side)
        move_ok = (left >= 0) & (mx.arange(L)[None, :] > 0) & (lt >= 0) & ~foreign & ~own_same & alive[:, None]
        lq, lr = self.mapq(lt)
        msp = self.spot(s, fd, me, lq, lr, lt)
        # The best blow from each destination over the foes (and the foe cities) in reach of it.
        rng_k = g1(self.kind(s, "range"), me)
        reach = mx.where(rng_k >= 0, rng_k, 1)
        # Only the foes' slots (the most any row has), not every slot: the pairs are [B, L, NF].
        foes_all = fd["foes"]
        if self.nfo is None:
            NFo = max(1, int(mx.max(mx.sum(foes_all, axis=1)).item()))
            fidx = mx.argsort(mx.where(foes_all, 0, 1) * U + mx.arange(U)[None, :], axis=1)[:, :NFo].astype(I32)
        else:
            # A fixed number of foe slots (no host round-trip): the nearest foes to the unit, then by slot; a foe out of
            # every destination's reach adds nothing, so only more foes than `nfo` within the unit's move and reach
            # would change the result.
            NFo = min(self.nfo, U)
            dme = hexd(fd["uq"] - mq[:, None], fd["ur"] - mr[:, None])
            fidx = mx.argsort(mx.where(foes_all, mx.minimum(dme, 63), 64) * U + mx.arange(U)[None, :], axis=1)[:, :NFo].astype(I32)
        foes = mx.take_along_axis(foes_all, fidx, axis=1)
        fq_, fr_ = mx.take_along_axis(fd["uq"], fidx, axis=1), mx.take_along_axis(fd["ur"], fidx, axis=1)
        ftile = mx.take_along_axis(s["u_tile"], fidx, axis=1)
        dlf = hexd(lq[:, :, None] - fq_[:, None, :], lr[:, :, None] - fr_[:, None, :])  # [B, L, NF]
        inr = foes[:, None, :] & (dlf <= reach[:, None, None]) & move_ok[:, :, None]
        frm = mx.broadcast_to(lt[:, :, None], (B, L, NFo)).reshape(B, L * NFo)
        tot = mx.broadcast_to(ftile[:, None, :], (B, L, NFo)).reshape(B, L * NFo)
        inr_f = inr.reshape(B, L * NFo)
        # The previews only of the pairs in reach (the rest give 0): the pairs packed first, as many columns as the
        # row with the most such pairs has (one count per call), then the best share scattered back per destination.
        N = L * NFo
        K = max(1, int(mx.max(mx.sum(inr_f, axis=1)).item()))
        if K < N:
            pk = mx.argsort(mx.where(inr_f, 0, 1) * N + mx.arange(N)[None, :], axis=1)[:, :K].astype(I32)
            inr_k = mx.take_along_axis(inr_f, pk, axis=1)
            dealt, _, dhp, pok = self.preview_packed(s, me, mx.take_along_axis(frm, pk, axis=1), mx.take_along_axis(tot, pk, axis=1), inr_k, False)
            share = mx.where(pok & inr_k, mx.minimum(dealt.astype(F32) / mx.maximum(dhp, 1).astype(F32), 1.0), 0.0)
            dest = (mx.arange(B)[:, None] * L + pk // NFo).reshape(-1)
            best = mx.zeros((B * L,), dtype=F32).at[dest].maximum(share.reshape(-1)).reshape(B, L)
        else:
            dealt, _, dhp, pok = self.preview(s, me, frm, tot, inr_f, False)
            share = mx.minimum(dealt.astype(F32) / mx.maximum(dhp, 1).astype(F32), 1.0)
            best = mx.max(mx.where(pok & inr_f, share, 0.0).reshape(B, L, NFo), axis=2)
        dlc = hexd(lq[:, :, None] - fd["cq"][:, None, :], lr[:, :, None] - fd["cr"][:, None, :])
        count = mx.sum(inr, axis=2) + mx.sum(fd["foe_city"][:, None, :] & (dlc <= reach[:, None, None]), axis=2)
        fm = [mx.zeros((B, L), dtype=F32)] * F_ACT
        self.write_spot(msp, here, fm)
        fm[27] = mx.where(mmax[:, None] > 0, mx.maximum(left, 0).astype(F32) / mx.maximum(mmax, 1).astype(F32)[:, None], 0.0)
        fm[28] = best
        fm[29] = fdiv(count, 3.0)
        move_feat = mx.stack(fm, axis=2)
        move_key = GROUP[K_MOVE] * (1 << 20) + mx.maximum(lt, 0)
        # Strikes (`validate_attack_tile`: a charge on a unit or a city is the unit's own attack on its tile, a ranged
        # unit shoots; `validate_ranged`): over the attack disc, every seen foreign unit (by handle), every foreign
        # city (by handle) and every such tile (a shot), where `plan_attack` holds.
        tgt = mx.take(geo.att_tile, mx.maximum(tile, 0), axis=0)
        tgt = mx.where((tile >= 0)[:, None], tgt, -1)
        OA = tgt.shape[1]
        ranged_kind = g1(self.kind(s, "range"), me) >= 0
        city = sim.city_on(s, tgt)
        is_city = city >= 0
        c_owner = mx.where(is_city, gn(s["c_owner"], mx.maximum(city, 0)), -1)
        m_sl, c_sl = gn(mil, mx.maximum(tgt, 0)), gn(civ, mx.maximum(tgt, 0))
        m_sl = mx.where(tgt >= 0, m_sl, -1)
        c_sl = mx.where(tgt >= 0, c_sl, -1)
        m_side = mx.where(m_sl >= 0, gn(s["u_side"], m_sl), -1)
        c_side = mx.where(c_sl >= 0, gn(s["u_side"], c_sl), -1)
        seen = sim.seen_by(s, sd, tgt)
        # The previews only where a foe's unit or city stands (elsewhere `plan_attack` has no defender: not ok), packed.
        isf = lambda x: sim.is_foe(s, sd[:, None], x)
        has_foe = (tgt >= 0) & (isf(m_side) | isf(c_side) | (is_city & isf(c_owner)))
        K = max(1, int(mx.max(mx.sum(has_foe, axis=1)).item()))
        if K < OA:
            pk = mx.argsort(mx.where(has_foe, 0, 1) * OA + mx.arange(OA)[None, :], axis=1)[:, :K].astype(I32)
            hk = mx.take_along_axis(has_foe, pk, axis=1)
            tk = mx.take_along_axis(tgt, pk, axis=1)
            d_k, t_k, h_k, o_k = self.preview_packed(s, me, mx.broadcast_to(tile[:, None], tk.shape), tk, hk, True)
            flat = (mx.arange(B)[:, None] * OA + pk).reshape(-1)
            back = lambda v, z: mx.full((B * OA,), z, dtype=v.dtype).at[flat].add(mx.where(hk, v, z).reshape(-1) - z).reshape(B, OA)
            dealt, taken, dhp = back(d_k, 0), back(t_k, 0), back(h_k, 0)
            sok = back((o_k & hk).astype(I32), 0) > 0
        else:
            dealt, taken, dhp, sok = self.preview(s, me, mx.broadcast_to(tile[:, None], tgt.shape), tgt, tgt >= 0, True)
        att_ok = sok
        tq, tr_ = self.mapq(tgt)
        ssp = self.spot(s, fd, me, tq, tr_, tgt)
        first = mx.where(isf(m_side), m_sl, mx.where(isf(c_side), c_sl, -1))
        if "u_arr" in s:
            # Rust's first foreign unit of the tile is the one that arrived first (`units_on` keeps the order of arrival).
            late = 1 << 30
            a_m = mx.where(isf(m_side), gn(s["u_arr"], mx.maximum(m_sl, 0)), late)
            a_c = mx.where(isf(c_side), gn(s["u_arr"], mx.maximum(c_sl, 0)), late)
            first = mx.where(isf(m_side) | isf(c_side), mx.where(a_m <= a_c, m_sl, c_sl), -1)
        fs = mx.maximum(first, 0)
        fk = gn(s["u_kind"], fs)
        fa = [mx.zeros((B, OA), dtype=F32)] * F_ACT
        self.write_spot(ssp, here, fa)
        me_hp = g1(s["u_hp"], me)[:, None]
        fa[30] = mx.where(sok, fdiv(dealt, 100.0), 0.0)
        fa[31] = mx.where(sok, fdiv(taken, 100.0), 0.0)
        fa[32] = mx.where(sok, fdiv(dhp, 100.0), 0.0)
        fa[33] = (sok & (dealt >= dhp)).astype(F32)
        fa[34] = (sok & (taken >= me_hp)).astype(F32)
        fa[35] = mx.where(first >= 0, (mx.take(self.tab.k["range"], mx.maximum(fk, 0)) >= 0).astype(F32), 0.0)
        fa[36] = mx.where(first >= 0, mx.take(self.tab.k["civ"], mx.maximum(fk, 0)).astype(F32), 0.0)
        fa[37] = is_city.astype(F32)
        fa[38] = mx.where(first >= 0, mx.take(self.tab.k["base"], mx.maximum(fk, 0)) / mean, 0.0)
        fa[39] = (~sok).astype(F32)
        handle = s["u_handle"]
        c_handle = t["c_handle"]
        # The groups of candidates: (base array, rows of it, key, valid, kind, pos, promo, local target). The numbers of a
        # candidate come from one of three base arrays (the moves', the strikes', the unit's own tile's), the same for every
        # kind of its group: they are gathered after the sort, and the kind's one-hot (columns 0..9) is added then.
        groups = []
        groups.append((0, None, move_key, move_ok, K_MOVE, lt, None, None))
        foe_city = is_city & isf(c_owner)
        strike_feat = mx.stack(fa, axis=2)
        for kind, ok, key in (
                (K_UNIT, att_ok & isf(m_side) & seen, GROUP[K_UNIT] * (1 << 20) + gn(handle, mx.maximum(m_sl, 0))),
                (K_UNIT, att_ok & isf(c_side) & seen, GROUP[K_UNIT] * (1 << 20) + gn(handle, mx.maximum(c_sl, 0))),
                (K_CITY, att_ok & foe_city, GROUP[K_CITY] * (1 << 20) + gn(c_handle, mx.maximum(city, 0))),
                (K_RANGED, att_ok & ranged_kind[:, None] & (foe_city | ((isf(m_side) | isf(c_side)) & seen)),
                 GROUP[K_RANGED] * (1 << 20) + mx.where(tgt >= 0, tr_ * t["mw"][:, None] + tq, 0))):
            groups.append((1, None, key, ok, kind, tgt, None, None))
        # Fortify, set up, promotions: the unit's own tile.
        hf = [mx.zeros((B, 1), dtype=F32)] * F_ACT
        self.write_spot(here, here, hf)
        # Pillage (`validate_pillage`): a kind that pillages, moves left, no city here, on land, something to pillage, not
        # our land (a foe's at war: the scenes at peace are not twinned). Intercept: a kind with an interception range, not
        # on duty yet.
        mv = g1(s["u_mv"], me)
        here_water = sim.mg("water", tile[:, None])[:, 0] > 0
        here_city = sim.city_on(s, tile[:, None])[:, 0] >= 0
        # The tile's owner: none, or a nation at war with ours (`validate_pillage`: "own tile", "not at war"; the third seat's
        # land in peace, an ally's, is refused).
        howner = gn(s["owner"], mx.maximum(tile, 0)[:, None])[:, 0]
        foe_land = Sim.is_foe(s, side, howner) if "war" in s else howner != side
        pill_ok = alive & g1(self.kind(s, "pillage"), me) & (mv > 0) & ~here_city & ~here_water & (gn(s["pillageable"], mx.maximum(tile, 0)[:, None])[:, 0] > 0) \
            & (howner != side) & ((howner < 0) | foe_land)
        icpt_ok = alive & (g1(self.kind(s, "icpt_range"), me) > 0) & (g1(s["u_st"], me) != 4)
        for kind, ok in ((K_FORT, mk["fort"]), (K_SETUP, mk["setup"]), (K_PILLAGE, pill_ok), (K_OTHER, icpt_ok)):
            groups.append((2, None, mx.full((B, 1), GROUP[kind] * (1 << 20), dtype=I32), ok[:, None], kind, tile[:, None], None, None))
        if sim.f["promo"]:
            # Only the promotion columns some row may take (the others would be padding: a key past the legal ones).
            pc = [j for j, a_ in enumerate(mx.any(mk["promo"], axis=0).tolist()) if a_]
            if pc:
                pcol = mx.array(pc, dtype=I32)
                NP = len(pc)
                gid = mx.take(t["promo_gid"], pcol, axis=1)
                groups.append((2, None, GROUP[K_PROMO] * (1 << 20) + gid, mx.take(mk["promo"], pcol, axis=1), K_PROMO, mx.broadcast_to(tile[:, None], (B, NP)), gid + 1, pcol))
        hfeat = mx.stack(hf, axis=2)
        big = mx.concatenate([move_feat, strike_feat, hfeat], axis=1)  # [B, L + OA + 1, F_ACT]
        base_at = [0, L, L + OA]
        keys, kinds, poss, promos, tgts, srcs = [], [], [], [], [], []
        for bi, _, key, ok, kind, pos, promo, loc in groups:
            n = key.shape[1]
            keys.append(mx.where(ok, key, BIG))
            kinds.append(mx.full((B, n), kind, dtype=I32))
            poss.append(pos)
            promos.append(promo if promo is not None else mx.zeros((B, n), dtype=I32))
            # The simulator's target of the action: the tile, or the promotion's local index.
            tgts.append(mx.broadcast_to(loc[None], (B, n)) if kind == K_PROMO else pos)
            srcs.append(base_at[bi] + (mx.arange(n, dtype=I32) if bi < 2 else mx.zeros((n,), dtype=I32)))
        key = mx.concatenate(keys, axis=1)
        src = mx.concatenate(srcs)
        kind = mx.concatenate(kinds, axis=1)
        pos = mx.concatenate(poss, axis=1)
        promo = mx.concatenate(promos, axis=1)
        tgt = mx.concatenate(tgts, axis=1)
        # Only as many columns as the row with the most legal actions has (the sort puts the legal ones first; the rest are
        # padding that `cmask` masks): one host read.
        n_list = min(key.shape[1], C_MAX - 1, max(1, int(mx.max(mx.sum(key < BIG, axis=1)).item())))
        order = mx.argsort(key, axis=1)[:, :n_list]
        valid = mx.take_along_axis(key, order, axis=1) < BIG
        kind = mx.take_along_axis(kind, order, axis=1)
        feat = mx.take_along_axis(big, mx.take(src, order)[:, :, None], axis=1)
        feat = feat + mx.pad((kind[:, :, None] == mx.arange(K_OTHER + 1, dtype=I32)[None, None, :]).astype(F32), [(0, 0), (0, 0), (0, F_ACT - K_OTHER - 1)])
        pos = mx.take_along_axis(pos, order, axis=1)
        promo = mx.take_along_axis(promo, order, axis=1)
        tgt = mx.take_along_axis(tgt, order, axis=1)
        # "Done" first.
        df = list(hf)
        df[K_DONE] = mx.ones((B, 1), dtype=F32)
        done = mx.stack(df, axis=2)
        feat = f16(mx.concatenate([done, mx.where(valid[:, :, None], feat, 0.0)], axis=1))
        cmask = mx.concatenate([alive[:, None], valid & alive[:, None]], axis=1)
        kind = mx.concatenate([mx.zeros((B, 1), dtype=I32), kind], axis=1)
        pos = mx.concatenate([tile[:, None], pos], axis=1)
        promo = mx.concatenate([mx.zeros((B, 1), dtype=I32), promo], axis=1)
        tgt = mx.concatenate([tile[:, None], tgt], axis=1)
        return {"tgt": tgt, "feat": feat, "mask": cmask, "n": mx.sum(cmask, axis=1), "kind": mx.where(cmask, kind, -1), "pos": mx.where(cmask, pos, -1),
                "promo": mx.where(cmask, promo, 0), "_mk": mk}


def expected_damage(me, opp, my_damage, wounded_mod):
    """`melee_damage` / `ranged_damage` with `Roll::Expected` (the same constants): the base plus (1200 - 1) x dr /
    100 / 2, scaled by the strengths, to whole hit points. A city (`my_damage` 0, `wounded_mod` 0) has dr 100."""
    dr = exact.wounded_factor(my_damage.astype(mx.int64), wounded_mod.astype(mx.int64))
    dmg = BASE * dr // 100 + (EXP_PART * dr // 100) // 2
    return exact.to_hp(exact.scale_by_ratio(dmg, me, opp))


# ---------------------------------------------------------------------------------------------- parity


def rebuild_profiles(sim, s, s0):
    """The profile changes of the promotions taken since the play's start (held now, not at the start): the
    state of a moment comes from Rust (`state_from`), its profiles from the scene's start plus these changes
    (`Sim.promote`'s profile part)."""
    B, U, NP = s["u_prom"].shape
    new = s["u_prom"] & ~s0["u_prom"]
    if not bool(mx.any(new).item()):
        return s
    for u in range(U):
        if not bool(mx.any(new[:, u]).item()):
            continue
        slot = mx.full((B,), u, dtype=I32)
        for p in range(NP):
            keep = new[:, u, p]
            if not bool(mx.any(keep).item()):
                continue
            pp = mx.full((B,), p, dtype=I32)
            di = sim.tg(sim.pz_int, slot, pp)
            db = sim.tg(sim.pz_bool, slot, pp)
            s["p_ctx"] = put1(s["p_ctx"], slot, grow(s["p_ctx"], slot) + di[:, :N_CTX], keep)
            for i, k in enumerate(sim.D_INT):
                name = "p_" + k
                s[name] = put1(s[name], slot, g1(s[name], slot) + di[:, N_CTX + i], keep)
            for i, k in enumerate(FLAG_DELTA):
                name = "p_" + k
                s[name] = put1(s[name], slot, g1(s[name], slot) | db[:, i], keep)
            cm = sim.tg(sim.z["z_cmov"], slot, pp)
            mset = g1(s["u_mset"], slot)
            nset = mx.where(cm >= 0, mset | mx.left_shift(mx.ones_like(cm), mx.maximum(cm, 0)), mset)
            cid = sim.tg(sim.z["z_combo_cost"], slot, nset)
            did = sim.tg(sim.z["z_combo_dest"], slot, nset)
            moved = keep & (cm >= 0)
            known = moved & (cid >= 0)
            s["u_mset"] = put1(s["u_mset"], slot, nset, moved)
            s["p_cost_id"] = put1(s["p_cost_id"], slot, cid, known)
            s["p_dest_id"] = put1(s["p_dest_id"], slot, did, known)
    return s


def load(path, limit=None, families=None):
    """The dump's plays: [(scene line, [moment lines], end line)]."""
    out = []
    for group in stream(path, limit=limit, families=families, group=1 << 30):
        out.extend(group)
    return out


# The moment fields the twin reads with a default when they are absent (a dump older than the field: the default silently
# stands for it, as the package-8 dumps of 13:41 without `ruins` / `upos`). `stream` counts the moments without each;
# every tool reports the count (`missing_report`), so a stale dump shows in its result.
MOMENT_FIELDS = ("wm", "cstr", "ruins", "upos", "kx")
MISSING = collections.Counter()


def missing_report():
    """{field: moments without it, "moments": all moments read} of the dumps read so far."""
    return dict(MISSING)


def stream(path, limit=None, families=None, group=40, every=1):
    """The dump's plays in groups of `group` plays (each moment kept when its index in the play % `every` == 0)."""
    plays = collections.OrderedDict()
    n = 0
    import gzip
    with (gzip.open(path, "rt") if str(path).endswith(".gz") else open(path)) as fh:
        cur = None
        for line in fh:
            d = json.loads(line)
            if d["type"] == "ev":
                # The event lines (`ARMY_OBS_EVENTS`) of the play whose scene line came last: kept on the scene.
                if cur in plays:
                    plays[cur][0].setdefault("events", []).append(d)
                continue
            name = d["play"]
            cur = name
            if families and name.split("/")[0] not in families:
                continue
            if d["type"] == "scene":
                if limit and n >= limit:
                    break
                n += 1
                plays[name] = [d, [], None, 0]
            elif name in plays:
                if d["type"] == "moment":
                    MISSING["moments"] += 1
                    for f in MOMENT_FIELDS:
                        if f not in d:
                            MISSING[f] += 1
                    plays[name][1].append(d if plays[name][3] % every == 0 else None)
                    plays[name][3] += 1
                else:
                    plays[name][2] = d
                    if len(plays) >= group and all(v[2] is not None for v in plays.values()):
                        yield [armythird.remap_play([sc, [m for m in ms if m is not None], e]) for sc, ms, e, _ in plays.values()]
                        plays = collections.OrderedDict()
    if plays:
        yield [armythird.remap_play([sc, [m for m in ms if m is not None], e]) for sc, ms, e, _ in plays.values()]


def build_templates(plays):
    """The plays' scenes as one template batch: (Batch, the template simulator, its options)."""
    from .parity import sight_radius
    dumps = []
    for sc, ms, _ in plays:
        slots = max([len(m["state"]["units"]) for m in ms] + [len(sc["initial"]["units"]), len(sc["scene"]["profiles"])])
        dumps.append({"scene": sc["scene"], "initial": sc["initial"], "slots": slots})
    tb = Batch(dumps)
    mr = max(1, max(p["moves_land"] for d in dumps for p in d["scene"]["profiles"]) // 60)
    geo = Geometry(tb.w, radius=sight_radius(dumps), move_radius=max(mr, 4), h=tb.h)
    has_near = any(t["cond"][0] == "other" for d in dumps for p in d["scene"]["profiles"] for t in p["terms"])
    has_air = any(p["domain"] == "Air" or p.get("air_intercept_range", 0) > 0 for d in dumps for p in d["scene"]["profiles"])
    kw = dict(move_radius=max(mr, 4), has_near_imp=has_near, dest_pool=tb.dest_pool, withdraw_pool=tb.withdraw_pool, has_air=has_air, features=tb.features())
    tsim = Sim(geo, tb.m, tb.cost_pool, tb.cstatic, tb.n_pieces, extra=tb.extra(), **kw)
    tb.s0_all = tb.initial_state()
    armythird.template_slots(tb, [sc for sc, _, _ in plays])
    return tb, tsim, kw


def build_rows(plays, rows, templates=None):
    """A simulator over the plays' scenes (templates) and a state over `rows` [(play index, moment)]."""
    tb, tsim, kw = templates or build_templates(plays)
    geo = tsim.geo
    idx = mx.array([pi for pi, _ in rows], dtype=I32)
    take = lambda a: mx.take(a, idx, axis=0)
    cs = {k: take(v) for k, v in tb.cstatic.items()}
    cs["tile_t"], cs["alive_t"] = tb.cstatic["tile"], tb.cstatic["alive"]
    sim = Sim(geo, tb.m, tb.cost_pool, cs, take(tb.n_pieces), los=tsim.los, extra=tb.extra(), tid=idx, **kw)
    s0 = {k: take(v) for k, v in tb.s0_all.items()}
    s = tb.state_from([m["state"] for _, m in rows])
    for k, v in s0.items():
        if k not in s:
            s[k] = v
    s = rebuild_profiles(sim, s, s0)
    s = armyprice.spawn_profiles(sim, s, [plays[pi][0] for pi, _ in rows])
    s = armythird.setup_rows(sim, s, [(plays[pi][0], m) for pi, m in rows])
    U = tb.u
    ids, handles = [], []
    for _, m in rows:
        ids.append((m["ids"] + [10 ** 6 + i for i in range(U)])[:U])
        handles.append((m["handles"] + [-1] * U)[:U])
    # Player-level effects the state record does not carry (an exogenous schedule from Rust): the wounded modifier.
    wm = [(m.get("wm", []) + [0] * U)[:U] for _, m in rows]
    if any(any(x) for x in wm):
        s["p_wounded_mod"] = mx.array(wm, dtype=s["p_wounded_mod"].dtype)
    if all("cstr" in m for _, m in rows):
        # The cities' strength without the garrison now (techs, buildings: exogenous), as the scene's `str0` at pop0.
        C = tb.c
        cs0 = [((m["cstr"] + [[0, 1]] * C)[:C]) for _, m in rows]
        s["c_str0"] = mx.array([[c[0] for c in row] for row in cs0], dtype=I32)
        s["c_maxhp"] = mx.array([[c[1] for c in row] for row in cs0], dtype=I32)
    T = tb.t
    s["pillageable"] = mx.array([[c == "1" for c in (m.get("pillageable") or "0" * T)] for _, m in rows]).astype(I32)
    # The ancient ruins still standing and the arrival order on the tiles (`armyruin`, `Sim.set_unit`).
    s["ruin"] = mx.array([[c == "1" for c in (m.get("ruins") or "0" * T)] for _, m in rows]).astype(I32)
    s["u_arr"] = mx.array([((list(m.get("upos") or []) + list(range(U)))[:U]) for _, m in rows], dtype=I32)
    s["n_arr"] = mx.full((len(rows),), 1 << 10, dtype=I32)
    s["rl"] = mx.array([m.get("round", 0) for _, m in rows], dtype=I32)
    s["u_id"] = mx.array(ids, dtype=I32)
    s["u_handle"] = mx.array(handles, dtype=I32)
    if not hasattr(tb, "tables0"):
        tb.tables0 = Tables([sc for sc, _, _ in plays], U, tb.c, mx.arange(len(plays), dtype=I32))
    tab = _rows_tab(tb.tables0, idx)
    return tb, sim, s, tab


def moment_inputs(plays, part, s, tab):
    """What a moment's tokens read besides the state (`Obs.tokens` keywords; s["u_far"], ["u_fq"], ["u_fr"] are set):
    the history's snapshot per slot, the round, the rows of the cities and units outside the window (Rust's rows:
    the simulator has no tiles there). Also the classes the twin leaves out, per row (None: twinned)."""
    B, U = s["u_alive"].shape
    mw = tab.t["mw"].tolist()
    ph = [[False] * U for _ in part]; pq = [[0] * U for _ in part]; pr = [[0] * U for _ in part]; php = [[0] * U for _ in part]
    NF = tab.t["far_tile"].shape[1]
    far = [[[0.0] * F_TOK for _ in range(NF)] for _ in part]
    skip = [None] * len(part)
    ufar = [[False] * U for _ in part]; ufq = [[0] * U for _ in part]; ufr = [[0] * U for _ in part]
    frows = [[[0.0] * F_TOK for _ in range(U)] for _ in part]
    far_tiles = tab.t["far_tile"].tolist()
    for b, (pi, m) in enumerate(part):
        for slot, tile, hp in m["prev"]:
            if slot < U:
                ph[b][slot] = True; pq[b][slot] = tile % mw[b]; pr[b][slot] = tile // mw[b]; php[b][slot] = hp
        feat = _unhex_tok(m["tok"]["feat"], m["tok"]["n"])
        for i, (k, v) in enumerate(m["tok"]["ref"]):
            if k == "c" and v in far_tiles[b]:
                far[b][far_tiles[b].index(v)] = feat[i]
        st_units = m["state"]["units"]
        for slot, (u, mt) in enumerate(zip(st_units, m["maptile"])):
            if u[0] == 1 and u[1] < 0 and mt >= 0 and slot < U:
                ufar[b][slot] = True; ufq[b][slot] = mt % mw[b]; ufr[b][slot] = mt // mw[b]
        for i, (k, v) in enumerate(m["tok"]["ref"]):
            if k == "u" and 0 <= v < U and ufar[b][v]:
                frows[b][v] = feat[i]
        if not m.get("war", True):
            skip[b] = skip[b] or "at peace (diplomacy: not in the simulator)"
        if [c[0] for c in m["state"]["cities"]] != [c[0] for c in plays[pi][0]["initial"]["cities"]]:
            skip[b] = skip[b] or "after a capture (the play is decided)"
        if any(u[0] == 1 and u[1] >= 0 for u in st_units[len(plays[pi][0]["scene"]["profiles"]):]) and not plays[pi][0]["scene"].get("reinforce"):
            skip[b] = skip[b] or "a unit the play created"
    s["u_far"] = mx.array(ufar); s["u_fq"] = mx.array(ufq, dtype=I32); s["u_fr"] = mx.array(ufr, dtype=I32)
    inp = {"rnd": mx.array([m["round"] for _, m in part], dtype=I32), "prev_has": mx.array(ph), "prev_tile_q": mx.array(pq, dtype=I32),
           "prev_tile_r": mx.array(pr, dtype=I32), "prev_hp": mx.array(php, dtype=I32), "far_rows": mx.array(far, dtype=F32),
           "far_unit_rows": mx.array(frows, dtype=F32)}
    return inp, skip


def unhex_u8(h):
    return [int(h[i:i + 2], 16) for i in range(0, len(h), 2)]


def check(path, limit=None, show=5, families=None, chunk=256, out=None, group=40, every=1):
    t0 = time.time()
    stats = collections.Counter()
    bad = collections.Counter()
    examples = []
    t_build = t_tok = t_cand = 0.0
    for plays in stream(path, limit=limit, families=families, group=group, every=every):
      rows = [(pi, m) for pi, (_, ms, _) in enumerate(plays) for m in ms]
      stats["plays"] += len(plays)
      # The static planes at the play's start (variant MB's map input) against Rust's.
      tb, sim, s, tab = build_rows(plays, [(pi, {"state": sc["initial"], "ids": [], "handles": []}) for pi, (sc, _, _) in enumerate(plays)])
      st = Obs(sim, tab).statics(s).tolist()
      for pi, (sc, _, _) in enumerate(plays):
          want = _unhex16(sc["orders"]["statics"], 576 * 16)
          got = [x for row in st[pi] for x in row]
          stats["static planes"] += 1
          if any(abs(a - b) > 1e-6 for a, b in zip(got, want)):
              bad["static planes"] += 1
              diff = sorted({i % 16 for i, (a, b) in enumerate(zip(got, want)) if abs(a - b) > 1e-6})
              _ex(examples, show, sc["play"], {"round": 0, "step": -1}, f"static planes differ in channels {diff}")
          else:
              stats["static planes equal"] += 1
      for c0 in range(0, len(rows), chunk):
          part = rows[c0:c0 + chunk]
          tb0 = time.time()
          tb, sim, s, tab = build_rows(plays, part)
          obs = Obs(sim, tab)
          B, U = s["u_alive"].shape
          inp, skip = moment_inputs(plays, part, s, tab)
          mx.eval(s)
          t_build += time.time() - tb0
          tt = time.time()
          tok = obs.tokens(s, **inp)
          far_tiles = tab.t["far_tile"].tolist()
          ufar = s["u_far"].tolist()
          mx.eval(tok)
          t_tok += time.time() - tt
          feat = tok["feat"].tolist(); n = tok["n"].tolist(); dist = tok["dist"].tolist(); pos = tok["pos"].tolist(); slot = tok["slot"].tolist()
          ent = tok["unit"].tolist()
          for b, (pi, m) in enumerate(part):
              if skip[b]:
                  stats["moments skipped: " + skip[b]] += 1
                  continue
              stats["moments"] += 1
              rt = m["tok"]
              rn = rt["n"]
              rfeat = _unhex_tok(rt["feat"], rn)
              ok = n[b] == rn
              if not ok:
                  bad["token count"] += 1
                  _ex(examples, show, plays[pi][0]["play"], m, f"tokens {n[b]} vs Rust {rn}")
                  continue
              rd = unhex_u8(rt["dist"])
              far_rows = {i for i, (k, v) in enumerate(rt["ref"]) if (k == "c" and v in far_tiles[b]) or (k == "u" and 0 <= v < U and ufar[b][v])}
              for i in range(rn):
                  stats["tokens"] += 1
                  if i in far_rows:
                      stats["tokens: outside the window (input rows)"] += 1
                      continue
                  diff = [j for j in range(F_TOK) if abs(feat[b][i][j] - rfeat[i][j]) > 1e-6]
                  if diff:
                      bad["token numbers"] += 1
                      for j in diff:
                          bad[f"token number {j}"] += 1
                      _ex(examples, show, plays[pi][0]["play"], m, f"token {i} {rt['ref'][i]}: " + ", ".join(f"{j}: {feat[b][i][j]:.5g} vs {rfeat[i][j]:.5g}" for j in diff[:6]))
                  else:
                      stats["tokens equal"] += 1
              stats["token rows (window)"] += rn - len(far_rows)
              if any(dist[b][i][j] != rd[i * rn + j] for i in range(rn) for j in range(rn)):
                  bad["distances"] += 1
              if pos[b][:rn] != rt["pos"]:
                  bad["positions"] += 1
                  _ex(examples, show, plays[pi][0]["play"], m, f"pos {pos[b][:rn]} vs {rt['pos']}")
              if slot[b][:rn] != rt["slot"]:
                  bad["army slots"] += 1
          # Candidates: one row per (moment, selectable unit of the dump).
          crow = []
          for b, (pi, m) in enumerate(part):
              if skip[b]:
                  continue
              for ti, cd in m["cands"].items():
                  k, v = m["tok"]["ref"][int(ti)]
                  crow.append((b, v, cd, pi, m))
          for k0 in range(0, len(crow), chunk):
              cr = crow[k0:k0 + chunk]
              bi = mx.array([b for b, *_ in cr], dtype=I32)
              sub = {k: mx.take(v, bi, axis=0) for k, v in s.items()}
              csim = _rows_sim(sim, bi)
              ctab = _rows_tab(tab, bi)
              cobs = Obs(csim, ctab)
              tc = time.time()
              me = mx.array([v for _, v, *_ in cr], dtype=I32)
              cand = cobs.candidates(sub, me)
              mx.eval(cand)
              t_cand += time.time() - tc
              cf = cand["feat"].tolist(); cn = cand["n"].tolist(); ck = cand["kind"].tolist(); cp = cand["pos"].tolist(); cpr = cand["promo"].tolist()
              for r, (b, v, cd, pi, m) in enumerate(cr):
                  stats["candidate lists"] += 1
                  acts = cd["act"]
                  rf = _unhex_f(cd["feat"], len(acts), F_ACT)
                  mine = [(ck[r][i], cp[r][i], cpr[r][i]) for i in range(cn[r])]
                  theirs = [(a[0], a[1], a[2]) for a in acts]
                  stats["candidates"] += len(theirs)
                  if any(a[1] < 0 for a in theirs):
                      # A move off the 24 x 24 window (a unit at its edge): the simulator has no tiles there.
                      # Not gated; reported: does the twin's list equal Rust's without the off-window candidates?
                      stats["candidate lists reaching outside the window"] += 1
                      inside = [(i, x) for i, x in enumerate(theirs) if x[1] >= 0]
                      mine_in = [(i, x) for i, x in enumerate(mine) if x[1] >= 0]
                      stats["candidates outside the window"] += len(theirs) - len(inside)
                      if 0 <= v < U and ufar[b][v]:
                          stats["outside lists: the unit itself beyond the window"] += 1
                      elif [x for _, x in mine_in] == [x for _, x in inside]:
                          if all(abs(cf[r][k][j] - rf[i][j]) <= 1e-6 for (k, _), (i, _) in zip(mine_in, inside) for j in range(F_ACT)):
                              stats["outside lists: the window's part equal"] += 1
                          else:
                              stats["outside lists: the window's part, numbers differ"] += 1
                              _ex(examples, show, plays[pi][0]["play"], m, f"unit {v} (outside list): window part numbers differ")
                      else:
                          stats["outside lists: the window's part, sets differ"] += 1
                          extra = [x for _, x in mine_in if x not in theirs][:4]
                          miss = [x for _, x in inside if x not in mine][:4]
                          _ex(examples, show, plays[pi][0]["play"], m, f"unit {v} (outside list): window part {len(mine_in)} vs Rust {len(inside)}; extra {extra} missing {miss}")
                      continue
                  if mine != theirs:
                      bad["candidate sets"] += 1
                      extra = [x for x in mine if x not in theirs][:4]
                      miss = [x for x in theirs if x not in mine][:4]
                      at = next((i for i, (x, y) in enumerate(zip(mine, theirs)) if x != y), None)
                      order = f"; first unequal at {at}: {mine[at:at + 3]} vs Rust {theirs[at:at + 3]}" if at is not None else ""
                      _ex(examples, show, plays[pi][0]["play"], m, f"unit {v}: candidates {len(mine)} vs Rust {len(theirs)}; extra {extra} missing {miss}{order}")
                      continue
                  stats["candidate lists equal sets"] += 1
                  allok = True
                  for i in range(len(theirs)):
                      diff = [j for j in range(F_ACT) if abs(cf[r][i][j] - rf[i][j]) > 1e-6]
                      if diff:
                          allok = False
                          for j in diff:
                              bad[f"candidate number {j}"] += 1
                          _ex(examples, show, plays[pi][0]["play"], m, f"unit {v} cand {i} {theirs[i]}: " + ", ".join(f"{j}: {cf[r][i][j]:.5g} vs {rf[i][j]:.5g}" for j in diff[:6]))
                      else:
                          stats["candidates equal"] += 1
                  if allok:
                      stats["candidate lists equal"] += 1
    res = {"stats": dict(stats), "bad": dict(bad), "dump fields missing": missing_report(), "examples": examples, "seconds": {"build": round(t_build, 1), "tokens": round(t_tok, 1), "candidates": round(t_cand, 1), "all": round(time.time() - t0, 1)}}
    print(json.dumps(res, indent=1))
    if out:
        with open(out, "w") as fh:
            json.dump(res, fh, indent=1)
    return res


def _rows_sim(sim, bi):
    """The simulator over rows `bi` of `sim`'s batch (the templates shared)."""
    cs = {k: (mx.take(v, bi, axis=0) if k not in ("tile_t", "alive_t") else v) for k, v in sim.cs.items()}
    out = Sim.__new__(Sim)
    out.__dict__.update(sim.__dict__)
    out.cs = cs
    out.tid = mx.take(sim.tid, bi, axis=0)
    out.B = out.tid.shape[0]
    out.n_pieces = mx.take(sim.n_pieces, bi, axis=0)
    return out


def _rows_tab(tab, bi):
    out = Tables.__new__(Tables)
    out.k = tab.k
    out.t = {k: mx.take(v, bi, axis=0) for k, v in tab.t.items()}
    return out


def _unhex_f(h, n, F):
    vals = [_f16_bits(int(h[4 * i: 4 * i + 4], 16)) for i in range(n * F)]
    return [vals[i * F:(i + 1) * F] for i in range(n)]


def _unhex_tok(h, n):
    return _unhex_f(h, n, F_TOK)


def _ex(examples, show, play, m, what):
    if len(examples) < show:
        examples.append(f"{play} r{m['round']} s{m['step']}: {what}")


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("dump")
    ap.add_argument("--limit", type=int)
    ap.add_argument("--show", type=int, default=8)
    ap.add_argument("--families")
    ap.add_argument("--chunk", type=int, default=256)
    ap.add_argument("--out")
    ap.add_argument("--group", type=int, default=40)
    ap.add_argument("--every", type=int, default=1, help="keep every k-th moment of a play")
    ap.add_argument("--cpu", action="store_true", help="run on the CPU (no GPU lock: development)")
    a = ap.parse_args(argv)
    if a.cpu:
        mx.set_default_device(mx.cpu)
    mx.set_memory_limit(8 << 30)
    mx.set_cache_limit(2 << 30)
    check(a.dump, limit=a.limit, show=a.show, families=a.families.split(",") if a.families else None, chunk=a.chunk, out=a.out, group=a.group, every=a.every)


if __name__ == "__main__":
    main()
