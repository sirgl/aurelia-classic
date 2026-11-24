"""The army net playing whole trainer scenes inside the MLX simulator on the GPU (greedy), both sides moving.

    python -m aurelia_nn.tacsim.armyplay DUMP.jsonl --weights RUN/weights.safetensors [--copies 1] [--rust ROWS.jsonl]

The scenes come from the observation dump's scene lines (`ARMY_OBS_DUMP`), so the plays are the same instances as
the Rust evaluation. The learner's side is the net: per decision the twin's tokens (`armyobs.Obs.tokens`), the body
with the memory of the turn's start (and, for MB, the map encoded at the turn's first moment), the greedy pointer,
the twin's candidates and the greedy action, applied by the simulator (`Sim.apply_unit`; pillage and intercept here);
a unit is stopped after 6 decisions of a turn, a turn after 96; then every army unit stands, the memory steps on the
turn's last tokens, the turn ends. The judge is `families::judge` of the ten families.

The enemy is the simulator's own script, NOT the Rust enemy: in Rust the trainers' enemy is the family script
(`assault::enemy_turn`: the field units' `act::unit_turn` Close / Support at even odds) and then the AI `standard`'s
mind for the rest of its seat (the garrison, the city's strike). Here: every enemy military unit strikes at even
odds or better from where it stands (the best blow by `act::best_blow`'s score), the field units of the assault
families close in on the nearest of ours first, the others hold (fortify); each enemy city strikes the unit of ours
it hurts most (`ai::combat::city_strikes`). The economy (borders, city strength, the wounded modifier of a policy) follows the
Rust play's schedule of the same instance, round by round, as the tacsim parity does.
"""

import argparse
import collections
import json
import time

import mlx.core as mx

from . import armyobs as A
from . import exact
from .armyloop import Brain, greedy_pointer, load_net, PER_UNIT
from .sim import g1, gn, put1, I32, V_NONE, V_FORT, V_SETUP, V_MOVE, V_ATTACK, V_PROMOTE

PER_TURN = 96
# The foes a candidate list's blow previews look at (the nearest to the unit; see `Obs.candidates`).
NFO = None  # counted per call: the per-batch count is small, a fixed 6 doubled the step (measured)
ASSAULT = {"assault-heal", "promotion-choice", "hill-archers", "city-fire", "planned-assault"}


class Game:
    """A batch of scene copies played on the device."""

    def __init__(self, plays, copies, brain, seed=1, forced=None):
        """`forced`: [(play index, moment-like row)] instead of each play's initial state (the forced checks: the
        Rust state at a turn's start, one row each); then `copies` is ignored."""
        self.plays = plays
        P = len(plays)
        self.brain = brain
        rows = []
        for pi, (sc, ms, _) in enumerate(plays):
            m0 = ms[0] if ms else {"ids": [], "handles": []}
            row = {"state": sc["initial"], "ids": m0.get("ids", []), "handles": m0.get("handles", []), "pillageable": sc["orders"]["pillageable"]}
            if ms:
                row.update({k: m0[k] for k in ("wm", "cstr") if k in m0})
            rows.append((pi, row))
        self.templates = A.build_templates(plays)
        if forced is None:
            idx = [i % P for i in range(P * copies)]
            picked = [rows[i] for i in idx]
        else:
            idx = [pi for pi, _ in forced]
            picked = list(forced)
        self.B = len(idx)
        _, self.sim, s, self.tab = A.build_rows(plays, picked, self.templates)
        self.idx = mx.array(idx, dtype=I32)
        if forced is None:
            # Each copy its own combat stream (the first copy keeps Rust's).
            salt = (mx.arange(self.B).astype(mx.uint64) // P) * 0x2545F4914F6CDD1D
            s["rng"] = s["rng"] ^ salt
        U = s["u_alive"].shape[1]
        s["u_far"] = mx.zeros((self.B, U), dtype=mx.bool_)
        s["u_fq"] = mx.zeros((self.B, U), dtype=I32)
        s["u_fr"] = mx.zeros((self.B, U), dtype=I32)
        s["n_slots"] = mx.take(self.templates[0].s0_all["n_slots"], self.idx)
        s["done"] = mx.zeros((self.B,), dtype=mx.bool_)
        s["n_unmodelled"] = mx.zeros((self.B,), dtype=I32)
        self.s = s
        self.obs = A.Obs(self.sim, self.tab)
        # The foe slots of the candidates' previews: the most enemy units a scene starts with, two more for the new ones.
        self.obs.nfo = NFO
        self.compare = forced is None and copies == 1
        self.U = U
        sc = [p[0] for p in plays]
        take = lambda v, dt=I32: mx.take(mx.array(v, dtype=dt), self.idx)
        self.n_mine = take([x["orders"].get("n_mine", x["scene"]["n_mine"]) for x in sc])
        self.rounds = take([x["orders"]["rounds"] for x in sc])
        self.start = take([x["orders"]["start_round"] for x in sc])
        fam = [x["play"].split("/")[0] for x in sc]
        self.fam = [fam[i] for i in idx]
        self.either = take([x["orders"].get("context") != "land" for x in sc], mx.bool_)
        roles = [x["orders"].get("roles", []) for x in sc]
        self.field = mx.take(mx.array([[r == "field" for r in (rl + [""] * U)[:U]] for rl in roles]), self.idx, axis=0)
        imp = [[False] * self.sim.T for _ in sc]
        for i, x in enumerate(sc):
            for t in x["orders"].get("improved", []):
                if t >= 0:
                    imp[i][t] = True
        self.improved = mx.take(mx.array(imp), self.idx, axis=0)
        self.pillaged = mx.zeros((self.B, self.sim.T), dtype=mx.bool_)
        # The target city (the objective's tile).
        ct = self.sim.cs["tile"]
        self.target = A.first_index_eq(ct, self.tab.t["obj_w"])
        self.has_target = self.target < ct.shape[1]
        # Far cities' rows (static: the first moment's), the schedules of the economy by round.
        NF = self.tab.t["far_tile"].shape[1]
        far = []
        self.sched = []
        for pi, (scl, ms, _) in enumerate(plays):
            rows_f = [[0.0] * A.F_TOK for _ in range(NF)]
            ftiles = scl["orders"]["far_cities"]
            if ms:
                feat = A._unhex_tok(ms[0]["tok"]["feat"], ms[0]["tok"]["n"])
                for i, (k, v) in enumerate(ms[0]["tok"]["ref"]):
                    for j, f in enumerate(ftiles):
                        if k == "c" and v == f[0]:
                            rows_f[j] = feat[i]
            far.append(rows_f)
            by_round = {}
            for m in ms:
                by_round.setdefault(m["round"], m)
            self.sched.append(by_round)
        self.far_rows = mx.take(mx.array(far, dtype=mx.float32), self.idx, axis=0)
        self.far_static = far
        self.um, self.pm, self.alive = brain.m.initial(self.B)
        self.M = mx.zeros((self.B, 576, brain.m.config["map_ch"])) if brain.has_map else None
        self.statics = self.obs.statics(self.s) if brain.has_map else None
        self.price = mx.take(mx.array([x["orders"]["price"] for x in sc], dtype=mx.float32), self.idx, axis=0)
        self.this = None
        self.prev = None
        self.verdict = mx.full((self.B,), -1, dtype=I32)
        self.city_low = mx.zeros((self.B,), dtype=mx.bool_)
        self.cap_broken = mx.zeros((self.B,), dtype=mx.bool_)
        self.counts = collections.Counter()
        self.cand_fn = self.obs.candidates
        self.legacy = False
        self.body_fn = brain.body
        self.enemy_rule = "std"
        # The trainers' end of a unit's turn heals an unmoved unit (`act::hold_unit`, nn/army-net ed9e453); False: the
        # harness before it (moves spent by hand, no heal), for the dumps made before.
        self.hold_heals = True
        # The judge where Rust's looks (after our script; see `learner_turn`); False: after the enemy's turn (G1).
        self.judge_after_script = True
        self.turn_tile = None
        self.fort_rule = "fit"
        # The scene rule (`armyrule`): both seats' pieces besides the net and the family script; off: the port of standard.
        self.scene_rule = False
        self.charge = False
        self.build_walks(plays, idx, U)
        # The slots that may hold an enemy ranged unit (the advisor's shooters): any scene's enemy ranged unit at the start.
        rk = self.tab.k["range"].tolist()
        self.shooter_slots = sorted({k for p in plays for k, u in enumerate(p[0]["initial"]["units"][:U]) if u[0] and u[8] == 1 and rk[u[9]] >= 0})

    def enable_compile(self):
        """`mx.compile` the decision step's two heavy parts: the candidates (the masks and the blow previews inside)
        and the net's body. The batch's shapes are fixed for the whole play, so each compiles once."""
        b = self.brain
        if self.M is None:
            body = mx.compile(lambda tok, um, pm, price: b.body(tok, um, pm, None, price))
            self.body_fn = lambda tok, um, pm, M, price: body(tok, um, pm, price)
        else:
            self.body_fn = mx.compile(b.body)

    # -------------------------------------------------------------- the economy's schedule

    def schedule(self, rel):
        """The Rust play's state of the economy at this round's learner turn (borders, population, city strength, the
        wounded modifier), where the play had a moment then."""
        s = self.s
        own, pop, cs0, cmax, wm, have = [], [], [], [], [], []
        T, C, U = self.sim.T, s["c_pop"].shape[1], self.U
        last = getattr(self, "_last", [None] * len(self.plays))
        for pi in range(len(self.plays)):
            m = self.sched[pi].get(self.plays[pi][0]["orders"]["start_round"] + rel) or last[pi]
            last[pi] = m
        self._last = last
        if not any(last):
            return
        for pi, m in enumerate(last):
            ok = m is not None
            have.append(ok)
            st = m["state"] if ok else None
            own.append([{"0": 0, "1": 1, "2": 2}.get(ch, -1) for ch in st["owner"]] if ok else [-1] * T)
            pop.append([c[3] for c in st["cities"]] + [0] * (C - len(st["cities"])) if ok else [0] * C)
            cc = (m.get("cstr", []) + [[0, 1]] * C)[:C] if ok else [[0, 1]] * C
            cs0.append([c[0] for c in cc]); cmax.append([c[1] for c in cc])
            wm.append((m.get("wm", []) + [0] * U)[:U] if ok else [0] * U)
        h = mx.take(mx.array(have), self.idx)
        tk = lambda v: mx.take(mx.array(v, dtype=I32), self.idx, axis=0)
        s["owner"] = mx.where(h[:, None], tk(own), s["owner"])
        s["c_pop"] = mx.where(h[:, None], tk(pop), s["c_pop"])
        if "c_str0" in s:
            s["c_str0"] = mx.where(h[:, None], tk(cs0), s["c_str0"])
            s["c_maxhp"] = mx.where(h[:, None], tk(cmax), s["c_maxhp"])
        s["p_wounded_mod"] = mx.where(h[:, None], tk(wm), s["p_wounded_mod"])
        # The far cities' rows (outside the window: their population and strength grow) from the same moment.
        NF = self.tab.t["far_tile"].shape[1]
        far = []
        for pi, m in enumerate(last):
            rows_f = [list(r) for r in self.far_static[pi]]
            if m is not None and NF:
                ftiles = self.plays[pi][0]["orders"]["far_cities"]
                feat = None
                for i, (k, v) in enumerate(m["tok"]["ref"]):
                    for j, f in enumerate(ftiles):
                        if k == "c" and v == f[0]:
                            feat = feat or A._unhex_tok(m["tok"]["feat"], m["tok"]["n"])
                            rows_f[j] = feat[i]
            far.append(rows_f)
        self.far_rows = mx.take(mx.array(far, dtype=mx.float32), self.idx, axis=0)

    # -------------------------------------------------------------- the learner: the net

    def snapshot(self):
        s = self.s
        uq, ur = self.obs.mapq(s["u_tile"])
        return (s["u_alive"] & (s["u_tile"] >= 0), uq, ur, s["u_hp"])

    def tokens(self, rnd):
        has, pq, pr, php = self.prev if self.prev is not None else (mx.zeros((self.B, self.U), dtype=mx.bool_),) + (mx.zeros((self.B, self.U), dtype=I32),) * 3
        return self.obs.tokens(self.s, mx.full((self.B,), rnd, dtype=I32) + self.start, has, pq, pr, php, self.far_rows)

    def learner_turn(self, rel):
        brain, s = self.brain, self.s
        B, U = self.B, self.U
        self.prev, self.this = self.this, self.snapshot()
        self.turn_tile = s["u_tile"]
        active = ~s["done"] & (self.verdict < 0)
        if self.compare:
            self.compare_state(rel)
        trace = [[] for _ in self.plays]
        made = mx.zeros((B, U), dtype=I32)
        army = self.tab.t["army"] >= 0
        for step in range(PER_TURN):
            tok = self.tokens(rel)
            unit = tok["unit"]
            stopped = mx.where(unit >= 0, gn(made, mx.maximum(unit, 0)) >= PER_UNIT, False)
            sel = tok["selectable"] & ~stopped & active[:, None]
            if step == 0 and brain.has_map:
                self.M = mx.where(active[:, None, None], brain.encode(self.statics, tok), self.M)
            h = self.body_fn(tok, self.um, self.pm, self.M, self.price)
            pick = greedy_pointer(brain.pointer(h, sel), sel.shape[1])
            active = active & (pick >= 0)
            n_act = int(mx.sum(active).item())
            if n_act == 0:
                break
            pk = mx.maximum(pick, 0)
            me = mx.where(active, mx.take_along_axis(unit, pk[:, None], axis=1)[:, 0], 0)
            cand = self.cand_fn(s, me)
            al = brain.actions(h, pk, cand, self.M)
            ai = A.first_index_eq(al, mx.max(al, axis=1))
            kind = mx.take_along_axis(cand["kind"], ai[:, None], axis=1)[:, 0]
            tgt = mx.take_along_axis(cand["tgt"], ai[:, None], axis=1)[:, 0]
            if self.compare:
                P = len(self.plays)
                al_ = active[:P].tolist(); pk_ = pick[:P].tolist(); ai_ = ai[:P].tolist()
                for b in range(P):
                    if al_[b]:
                        trace[b].append((pk_[b], ai_[b]))
            self.apply(me, kind, tgt, active, side=0, mk=None if self.legacy else cand["_mk"])
            made = put1(made, me, g1(made, me) + 1, active)
            self.counts["net decisions"] += n_act
        self.counts["net turns"] += int(mx.sum(~s["done"] & (self.verdict < 0)).item())
        if self.compare:
            self.compare_turn(rel, trace)
        # Every army unit stands, the seat's mind ends our turn; the memory steps on the turn's last tokens.
        live = ~s["done"] & (self.verdict < 0)
        self.our_turn_end(live)
        if self.judge_after_script and rel >= 1:
            # Rust's round boundary (`trainers::play_on`): the driver reports the round done at the learner's first step,
            # after the seat's script (and the scene rule's part): the arena closes and the judge looks there, before
            # the mind's part of our turn and the enemy's turn (a city low after our strikes, before its heal).
            self.judge(rel)
        s = self.s
        tok = self.tokens(rel)
        h = self.body_fn(tok, self.um, self.pm, self.M, self.price)
        um2, pm2, al2 = brain.step(h, tok, self.um, self.pm, self.alive)
        self.um = mx.where(live[:, None, None], um2, self.um)
        self.pm = mx.where(live[:, None, None], pm2, self.pm)
        self.alive = mx.where(live[:, None], al2, self.alive)
        self.s = self.sim.end_turn(s, mx.zeros((B,), dtype=I32), live)

    def our_turn_end(self, live):
        """The end of the net's turn: every army unit stands (`act::stand`: no moves left), then the seat's AI mind
        fortifies some of them (`planner_units::hold` through its decision tree)."""
        s = self.s
        army = self.tab.t["army"] >= 0
        s = self.sim.hold_units(s, army & (s["u_side"] == 0) & live[:, None] & (s["u_mv"] > 0), heal=self.hold_heals)
        if self.scene_rule:
            # The scene rule (`armyrule`, Rust `ARMY_SCENE_RULE=simple`) plays the rest of our seat's pieces.
            from . import armyrule
            self.s = s
            armyrule.seat_turn(self, 0, live)
            return
        # Which of them the mind fortifies hangs on its tasks and operations (`decide_military` -> `follow_task` ->
        # `hold`), which are not ported. The rule FITTED on the event dump (not a port; 72 % of the army unit-turns
        # right, never fortifying 69 %, the earlier rule 64 %): a melee unit that moved this turn and ends within 2 of
        # an enemy city, a ranged one that moved and struck; with `Sim.masks`'s fortify condition.
        water = self.sim.mg("water", s["u_tile"]) > 0
        tile = s["u_tile"]
        ct = self.sim.cs["tile"]
        foe_city = self.sim.cs["alive"] & (s["c_owner"] == 1) & (ct >= 0)
        dc = self.sim.geo.distance(mx.broadcast_to(tile[:, :, None], tile.shape + (ct.shape[1],)), mx.broadcast_to(ct[:, None, :], tile.shape + (ct.shape[1],)))
        dcity = mx.min(mx.where(foe_city[:, None, :], dc, 99), axis=2)
        moved = tile != (self.turn_tile if self.turn_tile is not None else tile)
        ranged = self.obs.kind(s, "range") >= 0
        fits = moved & mx.where(ranged, s["u_att"] > 0, dcity <= 2)
        if self.fort_rule == "old":
            fd = self.obs.field(s, 0)
            d = A.hexd(fd["uq"][:, :, None] - fd["uq"][:, None, :], fd["ur"][:, :, None] - fd["ur"][:, None, :])
            near = A.min_or(fd["foes"][:, None, :], d, 2, 99)
            fits = ~s["p_is_ranged"] | (near <= 1)
        elif self.fort_rule == "none":
            fits = mx.zeros_like(fits)
        can_fort = s["u_alive"] & (tile >= 0) & (s["u_side"] == 0) & army & live[:, None] & ~((s["u_st"] == 1) | (s["u_st"] == 2)) \
            & ~s["p_civilian"] & ~s["p_no_defensive_bonus"] & s["p_land"] & ~water & fits
        s["u_st"] = mx.where(can_fort, 1, s["u_st"])
        self.s = s

    def rust_turn(self, pi, rel):
        r = self.plays[pi][0]["orders"]["start_round"] + rel
        return [m for m in self.plays[pi][1] if m["round"] == r and m["step"] < (1 << 32) - 1]

    def compare_state(self, rel):
        """The first copy's state at our turn's start against the Rust play's at the same round (the enemy's fidelity)."""
        s = self.s
        P = len(self.plays)
        al, tl, hp = s["u_alive"][:P].tolist(), s["u_tile"][:P].tolist(), s["u_hp"][:P].tolist()
        xp, stv, lv = s["u_xp"][:P].tolist(), s["u_st"][:P].tolist(), s["u_level"][:P].tolist()
        dmg = s["c_dmg"][:P].tolist()
        self.state_ok = getattr(self, "state_ok", [True] * P)
        for pi in range(P):
            ms = self.rust_turn(pi, rel)
            if not ms or not self.state_ok[pi] or self.verdict[pi].item() >= 0:
                continue
            st = ms[0]["state"]
            npc = len(self.plays[pi][0]["orders"]["roles"])
            same = all((u[0] == 1) == bool(al[pi][k]) and (not u[0] or (u[1] == tl[pi][k] and u[2] == hp[pi][k] and u[7] == xp[pi][k] and u[6] == stv[pi][k] and u[10] == lv[pi][k]))
                       for k, u in enumerate(st["units"]) if k < min(self.U, npc))
            same = same and all(c[1] == dmg[pi][k] for k, c in enumerate(st["cities"]))
            self.counts["round starts compared"] += 1
            if same and exact.u64_to_ints(s["rng"][pi:pi + 1])[0] != int(st["rng"]):
                self.counts["round starts: units and cities equal, the combat stream not"] += 1
                fam = self.plays[pi][0]["play"].split("/")[0]
                self.counts[f"stream differs first: {fam}"] += 1
                same = False
            if same:
                self.counts["round starts equal to Rust"] += 1
            else:
                self.state_ok[pi] = False
                self.counts["plays diverged from Rust (state)"] += 1
                # What differs first: an enemy unit's place or life, our unit's hit points or life, the city.
                for k, u in enumerate(st["units"]):
                    if k >= self.U:
                        break
                    side = u[8] if u[0] else (s["u_side"][pi, k].item() if al[pi][k] else -1)
                    who = "enemy" if side == 1 else "our"
                    if (u[0] == 1) != bool(al[pi][k]):
                        self.counts[f"first divergence: {who} unit alive/dead"] += 1
                    elif u[0] and u[1] != tl[pi][k]:
                        self.counts[f"first divergence: {who} unit's tile"] += 1
                    elif u[0] and u[2] != hp[pi][k]:
                        self.counts[f"first divergence: {who} unit's hp"] += 1
                    elif u[0] and k < npc and u[7] != xp[pi][k]:
                        self.counts[f"first divergence: {who} unit's xp"] += 1
                    elif u[0] and k < npc and u[10] != lv[pi][k]:
                        self.counts[f"first divergence: {who} unit's level"] += 1
                    elif u[0] and k < npc and u[6] != stv[pi][k]:
                        self.counts[f"first divergence: {who} unit's stance"] += 1
                        if len(getattr(self, "ex_st", [])) < 6:
                            self.ex_st = getattr(self, "ex_st", []) + [f"{self.plays[pi][0]['play']} rel {rel} slot {k} rust {u[:11]} mlx st {stv[pi][k]}"]
                if any(c[1] != dmg[pi][k] for k, c in enumerate(st["cities"])):
                    self.counts["first divergence: city damage"] += 1

    def compare_turn(self, rel, trace):
        P = len(self.plays)
        for pi in range(P):
            if not self.state_ok[pi]:
                continue
            ms = self.rust_turn(pi, rel)
            if not ms:
                continue
            want = [(m["pick"], m["applied"]) for m in ms]
            got = trace[pi][:len(want)]
            self.counts["turns compared"] += 1
            if got == want[:len(got)] and len(got) == len([w for w in want if w[0] >= 0]):
                self.counts["turns with Rust's decisions"] += 1
            else:
                self.counts["turns with other decisions"] += 1
                if not hasattr(self, "ex"):
                    self.ex = []
                if len(self.ex) < 6:
                    self.ex.append(f"{self.plays[pi][0]['play']} rel {rel}: MLX {got[:8]} Rust {want[:8]}")

    def apply(self, me, kind, tgt, take, side, mk=None):
        """The candidate's action in the simulator (`mk`: the unit's masks on this state, when the caller has them)."""
        s, sim = self.s, self.sim
        sd = mx.full((self.B,), side, dtype=I32)
        verb = mx.where(kind == A.K_MOVE, V_MOVE, mx.where((kind == A.K_UNIT) | (kind == A.K_CITY) | (kind == A.K_RANGED), V_ATTACK,
                        mx.where(kind == A.K_FORT, V_FORT, mx.where(kind == A.K_SETUP, V_SETUP, mx.where(kind == A.K_PROMO, V_PROMOTE, V_NONE)))))
        sim_take = take & (verb != V_NONE)
        if bool(mx.any(sim_take).item()):
            mk = mk if mk is not None else sim.masks(s, sd, me)
            s = sim.apply_unit(s, sd, me, verb, tgt, mk, sim_take)
        # Done: the unit's turn is over (`act::stand`).
        done = take & (kind == A.K_DONE)
        s = sim.hold_units(s, (mx.arange(self.U)[None, :] == me[:, None]) & done[:, None], heal=self.hold_heals)
        # Intercept: on duty.
        s = sim.set_unit(s, me, take & (kind == A.K_OTHER), st=mx.full(me.shape, 4, dtype=I32))
        # Pillage (`Transaction::pillage`): the improvement (else the road) goes, 25 HP from an improvement, a move.
        pil = take & (kind == A.K_PILLAGE)
        if bool(mx.any(pil).item()):
            tile = g1(s["u_tile"], me)
            had = sim.mg("improvement", tile[:, None])[:, 0] >= 0
            T = sim.T
            hot = (mx.arange(T)[None, :] == tile[:, None]) & pil[:, None]
            self.pillaged = self.pillaged | (hot & had[:, None])
            s["pillageable"] = mx.where(hot, 0, s["pillageable"])
            s = sim.set_unit(s, me, pil & had, hp=mx.minimum(g1(s["u_hp"], me) + 25, 100))
            s = sim.set_unit(s, me, pil, mv=mx.maximum(g1(s["u_mv"], me) - 60, 0))
            self.counts["pillages"] += int(mx.sum(pil).item())
        self.s = s

    # -------------------------------------------------------------- the enemy's marches (a schedule)

    def build_walks(self, plays, idx, U):
        """The enemy's units that stand outside its cities at the scene's start and are not the family script's: the
        seat's AI marches them off (operations, tasks of the empire; they never strike in the dumps). Their tile after
        each enemy turn comes from the Rust play's events (`ARMY_OBS_EVENTS`), by the round from the scene's start."""
        R = 1 + max(p[0]["orders"]["rounds"] for p in plays) + 2
        walk, walker = [], []
        for sc, _, _ in plays:
            roles = sc["orders"]["roles"]
            ctiles = {c["tile"] for c in sc["scene"]["cities"]}
            init = sc["initial"]["units"]
            w = [k < len(init) and init[k][0] == 1 and init[k][8] == 1 and init[k][1] not in ctiles and (roles[k] if k < len(roles) else "") != "field" for k in range(U)]
            walker.append(w)
            sched = [[-1] * U for _ in range(R)]
            units = [list(u) for u in init]
            r0 = sc["orders"]["start_round"]
            for e in sc.get("events", []):
                for k, u in e["u"]:
                    while len(units) <= k:
                        units.append([0, -1])
                    units[k] = u
                if e["side"] == 1 and e["src"] == "end":
                    rel = e["round"] - r0 - 1
                    if 0 <= rel < R:
                        sched[rel] = [units[k][1] if k < len(units) and w[k] and units[k][0] == 1 else -1 for k in range(U)]
            walk.append(sched)
        self.walker = mx.take(mx.array(walker), self.idx, axis=0)
        self.walks = mx.take(mx.array(walk, dtype=I32), self.idx, axis=0)  # [B, R, U]
        self.has_walks = any(any(w) for w in walker) and all("events" in p[0] for p in plays)
        self.rel_now = mx.zeros((self.B,), dtype=I32)

    def walk_to(self, me):
        """The marching unit's tile after this enemy turn in the Rust play (-1: unknown, it stays)."""
        R = self.walks.shape[1]
        r = mx.clip(self.rel_now, 0, R - 1)
        row = mx.take_along_axis(self.walks, mx.broadcast_to(r[:, None, None], (self.B, 1, self.U)), axis=1)[:, 0]
        return mx.where(self.has_walks, g1(row, me), -1)

    def field_script(self, live):
        """The family script of the assault families (`assault::enemy_turn`): each field unit's `act::unit_turn`,
        Close (melee) or Support (ranged) at even odds: strike from where it stands, else close in and strike."""
        s, sim = self.s, self.sim
        B, U, side = self.B, self.U, 1
        sd = mx.full((B,), side, dtype=I32)
        is_assault = mx.array([f in ASSAULT for f in self.fam])
        for u in range(U):
            me = mx.full((B,), u, dtype=I32)
            s = self.s
            mine = live & is_assault & g1(self.field, me) & g1(s["u_alive"], me) & (g1(s["u_side"], me) == side) & (g1(s["u_tile"], me) >= 0)
            if not bool(mx.any(mine).item()):
                continue
            ok, t = self.best_blow(me, side)
            hit = mine & ok
            if bool(mx.any(hit).item()):
                mk = sim.masks(self.s, sd, me)
                self.s = sim.apply_unit(self.s, sd, me, mx.full((B,), V_ATTACK, dtype=I32), t, mk, hit)
            fld = mine & ~hit
            if bool(mx.any(fld).item()):
                s = self.s
                mk = sim.masks(s, sd, me)
                fd = self.obs.field(s, side)
                lq, lr = self.obs.mapq(mk["lt"])
                d = A.hexd(lq[:, :, None] - fd["uq"][:, None, :], lr[:, :, None] - fd["ur"][:, None, :])
                near = A.min_or(fd["foes"][:, None, :], d, 2, 99)
                rng = g1(self.obs.kind(s, "range"), me)
                want = mx.where((rng >= 0)[:, None], mx.abs(near - rng[:, None]), mx.maximum(near - 1, 0))
                key = mx.where(mk["move_l"], want * 1000 - self.obs.defence(s, mk["lt"], 1), 1 << 30)
                j = A.first_index_eq(key, mx.min(key, axis=1))
                go = fld & (mx.min(key, axis=1) < (1 << 30))
                tile = mx.take_along_axis(mk["lt"], j[:, None], axis=1)[:, 0]
                self.s = sim.apply_unit(s, sd, me, mx.full((B,), V_MOVE, dtype=I32), tile, mk, go)
                ok2, t2 = self.best_blow(me, side)
                hit2 = go & ok2
                if bool(mx.any(hit2).item()):
                    mk = sim.masks(self.s, sd, me)
                    self.s = sim.apply_unit(self.s, sd, me, mx.full((B,), V_ATTACK, dtype=I32), t2, mk, hit2)
            # The script's unit stands (`act::stand`).
            self.s = sim.hold_units(self.s, (mx.arange(U)[None, :] == u) & mine[:, None], heal=self.hold_heals)

    def enemy_turn(self):
        if self.scene_rule:
            from . import armyrule
            armyrule.enemy_turn(self)
        elif self.enemy_rule == "std":
            from . import armystd
            armystd.enemy_turn(self, charge_on=self.charge)
        else:
            self.enemy_turn_approx()

    # -------------------------------------------------------------- the enemy: the simulator's script

    def best_blow(self, me, side):
        """The enemy unit's best even blow from where it stands on the learner's units and cities (best_blow's score):
        (has one, the target tile)."""
        s, sim, geo = self.s, self.sim, self.geo_()
        tile = g1(s["u_tile"], me)
        tgt = mx.take(geo.att_tile, mx.maximum(tile, 0), axis=0)
        tgt = mx.where((tile >= 0)[:, None], tgt, -1)
        mil, civ = sim.occupancy(s)
        city = sim.city_on(s, tgt)
        m_sl, c_sl = gn(mil, mx.maximum(tgt, 0)), gn(civ, mx.maximum(tgt, 0))
        m_side = mx.where((tgt >= 0) & (m_sl >= 0), gn(s["u_side"], mx.maximum(m_sl, 0)), -1)
        c_side = mx.where((tgt >= 0) & (c_sl >= 0), gn(s["u_side"], mx.maximum(c_sl, 0)), -1)
        c_own = mx.where(city >= 0, gn(s["c_owner"], mx.maximum(city, 0)), -1)
        foe = 1 - side
        target = (m_side == foe) | (c_side == foe) | (c_own == foe)
        dealt, taken, dhp, ok = self.obs.preview(s, me, mx.broadcast_to(tile[:, None], tgt.shape), tgt, target, True)
        hp = g1(s["u_hp"], me)[:, None]
        kills = dealt >= dhp
        fine = ok & (kills | ((dealt >= taken) & (taken < hp)))
        capt = (m_side == foe) & (mx.take(self.tab.k["range"], mx.maximum(gn(s["u_kind"], mx.maximum(m_sl, 0)), 0)) < 0)
        score = mx.where(kills, 10000, 0) + (100 - mx.maximum(dhp - dealt, 0)) * 10 + mx.where(capt, 50, 0) + mx.where(city >= 0, 20, 0) - taken
        score = mx.where(fine, score, -(1 << 30))
        best = mx.max(score, axis=1)
        j = A.first_index_eq(score, best)
        return best > -(1 << 30), mx.take_along_axis(tgt, j[:, None], axis=1)[:, 0]

    def geo_(self):
        return self.sim.geo

    def enemy_turn_approx(self):
        s, sim = self.s, self.sim
        B, U = self.B, self.U
        side = 1
        sd = mx.full((B,), side, dtype=I32)
        live = ~s["done"] & (self.verdict < 0)
        for u in range(U):
            me = mx.full((B,), u, dtype=I32)
            mine = live & g1(s["u_alive"], me) & (g1(s["u_side"], me) == side) & (g1(s["u_tile"], me) >= 0) & ~g1(self.obs.kind(s, "civ"), me)
            if not bool(mx.any(mine).item()):
                continue
            ok, t = self.best_blow(me, side)
            hit = mine & ok
            if bool(mx.any(hit).item()):
                mk = sim.masks(self.s, sd, me)
                self.s = sim.apply_unit(self.s, sd, me, mx.full((B,), V_ATTACK, dtype=I32), t, mk, hit)
            # The field units of the assault families close in on the nearest of ours, then strike.
            fld = mine & ~hit & g1(self.field, me) & mx.array([f in ASSAULT for f in self.fam])
            if bool(mx.any(fld).item()):
                s = self.s
                mk = sim.masks(s, sd, me)
                fd = self.obs.field(s, side)
                lq, lr = self.obs.mapq(mk["lt"])
                d = A.hexd(lq[:, :, None] - fd["uq"][:, None, :], lr[:, :, None] - fd["ur"][:, None, :])
                near = A.min_or(fd["foes"][:, None, :], d, 2, 99)
                rng = g1(self.obs.kind(s, "range"), me)
                want = mx.where((rng >= 0)[:, None], mx.abs(near - rng[:, None]), mx.maximum(near - 1, 0))
                key = mx.where(mk["move_l"], want * 1000 - self.obs.defence(s, mk["lt"], 1), 1 << 30)
                j = A.first_index_eq(key, mx.min(key, axis=1))
                go = fld & (mx.min(key, axis=1) < (1 << 30))
                tile = mx.take_along_axis(mk["lt"], j[:, None], axis=1)[:, 0]
                self.s = sim.apply_unit(s, sd, me, mx.full((B,), V_MOVE, dtype=I32), tile, mk, go)
                ok2, t2 = self.best_blow(me, side)
                hit2 = go & ok2
                if bool(mx.any(hit2).item()):
                    mk = sim.masks(self.s, sd, me)
                    self.s = sim.apply_unit(self.s, sd, me, mx.full((B,), V_ATTACK, dtype=I32), t2, mk, hit2)
            # The rest hold: fortify where the rules let them.
            s = self.s
            rest = mine & ~hit & ~fld
            if bool(mx.any(rest).item()):
                mk = sim.masks(s, sd, me)
                self.s = sim.apply_unit(s, sd, me, mx.full((B,), V_FORT, dtype=I32), mx.zeros((B,), dtype=I32), mk, rest & mk["fort"])
        # Each enemy city strikes the unit of ours it hurts most (a kill first).
        C = self.sim.cs["tile"].shape[1]
        for c in range(C):
            s = self.s
            cs = mx.full((B,), c, dtype=I32)
            ok = sim.city_mask(s, sd, cs) & live[:, None]
            if not bool(mx.any(ok).item()):
                continue
            mil, civ = sim.occupancy(s)
            T = sim.T
            tiles = mx.broadcast_to(mx.arange(T, dtype=I32)[None], (B, T))
            dsl = mx.where(gn(mil, tiles) >= 0, gn(mil, tiles), gn(civ, tiles))
            hp = mx.where(dsl >= 0, gn(s["u_hp"], mx.maximum(dsl, 0)), 0)
            _, c_rstr = sim.city_strength(s, cs, mil)
            dfn = self.obs.defense_vs_ranged(s, self.obs.fighter_n_slots(s, dsl, dsl >= 0), self.obs.fighter_n_slots(s, mx.full((B, T), -1, dtype=I32), mx.zeros((B, T), dtype=mx.bool_)))
            dmg = A.expected_damage(mx.broadcast_to(c_rstr[:, None], (B, T)), dfn, mx.zeros((B, T), dtype=I32), mx.zeros((B, T), dtype=I32))
            # `ai::combat::city_strikes`: the visible enemy it hurts most (the expected damage up to its hit points),
            # the lowest tile on a tie.
            score = mx.minimum(dmg, hp)
            score = mx.where(ok, score, -1)
            best = mx.max(score, axis=1)
            t = A.first_index_eq(score, best)
            take = best >= 0
            self.s = sim.city_strike(s, sd, cs, t, take)
            self.counts["enemy city strikes"] += int(mx.sum(take).item())
        self.s = sim.end_turn(self.s, sd, live)

    # -------------------------------------------------------------- the judge

    def judge(self, rel):
        s = self.s
        B = self.B
        U = self.U
        piece = mx.arange(U)[None, :] < self.n_mine[:, None]
        mine = s["u_alive"] & piece & (s["u_side"] == 0)
        n_alive = mx.sum(mine, axis=1)
        lost = self.n_mine - n_alive
        mil = mx.any(mine & ~self.obs.kind(s, "civ"), axis=1)
        tc = mx.minimum(self.target, s["c_owner"].shape[1] - 1)
        taken = self.has_target & (g1(s["c_owner"], tc) == 0)
        hpc = g1(s.get("c_maxhp", self.sim.cs["max_hp"]), tc) - g1(s["c_dmg"], tc)
        self.city_low = self.city_low | (self.has_target & (hpc * 10 <= g1(s.get("c_maxhp", self.sim.cs["max_hp"]), tc)))
        limit = (rel >= self.rounds)
        pill = mx.sum(self.pillaged & self.improved, axis=1)
        n = self.n_mine
        fam = self.fam
        W, L, N = 1, 0, -1
        v = mx.full((B,), N, dtype=I32)
        is_f = lambda names: mx.array([x in names for x in fam])
        cap = mx.where(is_f({"assault-heal", "promotion-choice"}), n // 2, 1)
        self.cap_broken = self.cap_broken | (is_f(ASSAULT) & (lost > cap))
        # siege, ram
        g = is_f({"siege", "ram"})
        v = mx.where(g, mx.where(taken, W, mx.where(limit | ~mil, L, N)), v)
        g = is_f(ASSAULT)
        v = mx.where(g, mx.where(self.cap_broken, L, mx.where(taken, W, mx.where(limit | ~mil, L, N))), v)
        g = is_f({"raid"})
        v = mx.where(g, mx.where((pill >= 3) & (lost <= 1), W, mx.where(limit | (lost >= 2), L, N)), v)
        g = is_f({"prey"})
        lose = self.either & (n_alive * 4 < n * 3)
        v = mx.where(g, mx.where(taken, W, mx.where(lose, L, mx.where(limit, mx.where(self.either & (n_alive * 4 >= n * 3), W, L), N))), v)
        g = is_f({"bombard"})
        v = mx.where(g, mx.where(taken | self.city_low, W, mx.where(limit | (n_alive == 0), L, N)), v)
        self.verdict = mx.where(self.verdict < 0, v, self.verdict)

    def play(self, limit_rounds=None):
        rel = 0
        max_rounds = int(mx.max(self.rounds).item())
        if limit_rounds:
            max_rounds = min(max_rounds, limit_rounds - 1)
        t0 = time.time()
        while rel < max_rounds + 1:
            if not bool(mx.any((self.verdict < 0) & ~self.s["done"]).item()):
                break
            self.counts["scene-rounds"] += int(mx.sum((self.verdict < 0) & ~self.s["done"]).item())
            self.schedule(rel)
            self.rel_now = mx.full((self.B,), rel, dtype=I32)
            ta = time.time()
            self.learner_turn(rel)
            mx.eval(self.s)
            tb_ = time.time()
            self.enemy_turn()
            mx.eval(self.s)
            self.counts["ms learner turns"] += int(1000 * (tb_ - ta))
            self.counts["ms enemy turns"] += int(1000 * (time.time() - tb_))
            self.s = self.sim.round_end(self.s, ~self.s["done"])
            rel += 1
            if not self.judge_after_script:
                self.judge(rel)
            mx.eval(self.s, self.verdict)
        # A capture ends the scene (the simulator's `done`): judge it.
        self.judge(rel)
        self.seconds = time.time() - t0
        self.rounds_played = rel
        return self.verdict


def run(path, weights, copies=1, families=None, limit=None, rust=None, out=None, group=None, rounds=None, setup=None):
    brain = Brain(load_net(weights))
    plays = A.load(path, limit=limit, families=families)
    res = {"families": {}, "counts": collections.Counter(), "seconds": 0.0}
    rust_w = {}
    if rust:
        for l in open(rust):
            r = json.loads(l)
            era = r["era"]
            rust_w[(r["family"], r["instance"], r["attempt"], era)] = r["win"]
    groups = [plays] if not group else [plays[i:i + group] for i in range(0, len(plays), group)]
    per = collections.defaultdict(lambda: [0, 0, 0, 0, 0, 0])  # MLX wins, n, Rust wins, same verdict, plays, MLX wins (first copy)
    for gp in groups:
        g = Game(gp, copies, brain)
        if setup:
            setup(g)
        v = g.play(rounds).tolist()
        res["counts"].update(g.counts)
        res.setdefault("examples", []).extend(getattr(g, "ex", []))
        res.setdefault("examples_stance", []).extend(getattr(g, "ex_st", []))
        res["seconds"] += g.seconds
        for b, verdict in enumerate(v):
            sc = gp[b % len(gp)][0]
            fam, era, inst, att = sc["play"].split("/")
            win = verdict == 1
            key = fam
            if fam == "prey" and sc["orders"].get("context") == "land":
                key = "prey-land"
            per[key][0] += win
            per[key][1] += 1
            if b < len(gp):
                rw = gp[b][2]["win"] if gp[b][2] else None
                res.setdefault("plays", {})[sc["play"]] = [int(win), int(bool(rw))]
                per[key][2] += bool(rw)
                per[key][3] += (rw == win)
                per[key][4] += 1
                per[key][5] += win
    for f, (w, n, rw, same, plays_n, w1) in sorted(per.items()):
        res["families"][f] = {"plays": plays_n, "rust_win": round(100 * rw / max(plays_n, 1), 1), "mlx_win": round(100 * w1 / max(plays_n, 1), 1),
                              "same_verdict": same, "scenes": n, "mlx_win_all_copies": round(100 * w / max(n, 1), 1)}
    res["counts"] = dict(res["counts"])
    res["seconds"] = round(res["seconds"], 1)
    c = res["counts"]
    res["throughput"] = {"scenes": sum(v["scenes"] for v in res["families"].values()), "scene_rounds_per_s": round(c.get("scene-rounds", 0) / max(res["seconds"], 1e-9), 1),
                         "net_decisions_per_s": round(c.get("net decisions", 0) / max(res["seconds"], 1e-9), 1)}
    print(json.dumps(res, indent=1))
    if out:
        with open(out, "w") as fh:
            json.dump(res, fh, indent=1)
    return res


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("dump")
    ap.add_argument("--weights", required=True)
    ap.add_argument("--copies", type=int, default=1)
    ap.add_argument("--families")
    ap.add_argument("--limit", type=int)
    ap.add_argument("--group", type=int)
    ap.add_argument("--out")
    ap.add_argument("--rounds", type=int, help="stop after this many rounds (the throughput runs)")
    ap.add_argument("--cpu", action="store_true")
    ap.add_argument("--enemy", default="std", choices=["std", "approx"])
    ap.add_argument("--old-heal", action="store_true", help="the harness before act::hold_unit (no heal at a stand)")
    ap.add_argument("--fort", default="fit", choices=["fit", "old", "none"])
    ap.add_argument("--charge", action="store_true")
    ap.add_argument("--compile", action="store_true")
    ap.add_argument("--scene-rule", action="store_true", help="the scene rule (Rust ARMY_SCENE_RULE=simple) on both seats")
    ap.add_argument("--old-judge", action="store_true", help="judge after the enemy's turn (the G1 harness)")
    a = ap.parse_args(argv)
    if a.cpu:
        mx.set_default_device(mx.cpu)
    mx.set_memory_limit(16 << 30)
    mx.set_cache_limit(2 << 30)
    def setup(g):
        g.enemy_rule, g.hold_heals, g.fort_rule, g.charge = a.enemy, not a.old_heal, a.fort, a.charge
        g.scene_rule = a.scene_rule
        g.judge_after_script = not a.old_judge
        if a.compile:
            g.enable_compile()
    res = run(a.dump, a.weights, copies=a.copies, families=a.families.split(",") if a.families else None, limit=a.limit, out=a.out, group=a.group, rounds=a.rounds, setup=setup)
    res["setup"] = {"enemy": a.enemy, "old_heal": a.old_heal, "fort": a.fort, "charge": a.charge, "scene_rule": a.scene_rule, "old_judge": a.old_judge}
    if a.out:
        with open(a.out, "w") as fh:
            json.dump(res, fh, indent=1)


if __name__ == "__main__":
    main()
