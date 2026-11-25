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

from . import armycamp
from . import armyparty
from . import armyhist as H
from . import armyobs as A
from . import armyprice
from . import armythird
from . import exact
from .armyloop import Brain, greedy_pointer, load_net, PER_UNIT
from .sim import g1, gn, put1, I32, V_NONE, V_FORT, V_SETUP, V_MOVE, V_ATTACK, V_PROMOTE

PER_TURN = 96
# The foes a candidate list's blow previews look at (the nearest to the unit; see `Obs.candidates`).
NFO = None  # counted per call: the per-batch count is small, a fixed 6 doubled the step (measured)
ASSAULT = {"assault-heal", "promotion-choice", "hill-archers", "city-fire", "planned-assault"}


def trim_tokens(tok):
    """The tokens without the padding columns past the row with the most tokens (they are masked: no weight in the body,
    never selectable). One host read."""
    T = max(1, int(mx.max(tok["n"]).item()))
    if T >= tok["mask"].shape[1]:
        return tok
    out = dict(tok)
    for k in ("feat", "mask", "pos", "slot", "unit", "ent", "selectable"):
        out[k] = tok[k][:, :T]
    out["dist"] = tok["dist"][:, :T, :T]
    return out


class Game:
    """A batch of scene copies played on the device."""

    def __init__(self, plays, copies, brain, seed=1, forced=None, salt=True, facts=None):
        """`forced`: [(play index, moment-like row)] instead of each play's initial state (the forced checks: the
        Rust state at a turn's start, one row each); then `copies` is ignored. `salt` False: every copy of a play keeps
        the play's combat stream (the GRPO groups: the copies differ only by the sampled decisions)."""
        self.plays = plays
        P = len(plays)
        self.brain = brain
        rows = []
        for pi, (sc, ms, _) in enumerate(plays):
            m0 = ms[0] if ms else {"ids": [], "handles": []}
            row = {"state": sc["initial"], "ids": m0.get("ids", []), "handles": m0.get("handles", []), "pillageable": sc["orders"]["pillageable"],
                   "ruins": sc["orders"].get("ruins")}
            if ms:
                row.update({k: m0[k] for k in ("wm", "cstr", "upos", "kx") if k in m0})
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
        # The kind contexts (`kx`: the player-level combat percents, the dumps since a832467) the profiles hold now, per play
        # [U][N_CTX] (own play: every copy of a play follows its schedule); None: forced rows or a dump without them.
        self._kx_ap = None
        if forced is None and any("kx" in r for _, r in rows):
            U0 = s["p_ctx"].shape[1]
            self._kx_ap = []
            for pi, r in rows:
                base = A.kx_base(plays[pi][0], plays[pi][1])
                kx = r.get("kx") or []
                self._kx_ap.append([(kx[k] if k < len(kx) and kx[k] else base.get(k)) or [0] * A.N_CTX for k in range(U0)])
        if forced is None and salt:
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
                by_round.setdefault((m["round"], m["step"]), m)
            self.sched.append(by_round)
        self.far_rows = mx.take(mx.array(far, dtype=mx.float32), self.idx, axis=0)
        self.far_static = far
        # A city's most hit points once it is ours (the conqueror's techs, the buildings that survived: exogenous, from the
        # first Rust moment that has the city ours; -1 unknown): what the history's fact of the capturing strike reads.
        C = self.sim.cs["tile"].shape[1]
        cap = []
        for scl, ms, _ in plays:
            row = [-1] * C
            for m in ms:
                for k, c in enumerate(m["state"]["cities"][:C]):
                    if c[0] == 0 and row[k] < 0 and k < len(m.get("cstr", [])):
                        row[k] = m["cstr"][k][1]
            cap.append(row)
        self.cap_maxhp = mx.take(mx.array(cap, dtype=I32), self.idx, axis=0)
        self.um, self.pm, self.alive = brain.m.initial(self.B)
        # MBN's notes (written at our turn's end, read in the next turn) and the history of facts (MBH reads it; on with
        # either stream, `facts`); both live in the state dict, so the compacted steps carry them.
        self.facts = (brain.notes or brain.hist) if facts is None else (facts or brain.hist)
        if self.facts:
            H.init(self.s, self.B, U, plan0=self.pm[0, 0])
        if brain.notes:
            for k, v in brain.initial_notes(self.B).items():
                self.s["ns_" + k] = v
        self.hist_cmp = collections.Counter()
        self.hist_ex = []
        self.M = mx.zeros((self.B, 576, brain.m.config["map_ch"])) if brain.has_map else None
        self.statics = self.obs.statics(self.s) if brain.has_map else None
        self.price = mx.take(mx.array([x["orders"]["price"] for x in sc], dtype=mx.float32), self.idx, axis=0)
        self.this = None
        self.prev = None
        # GRPO (`armygrpo`): at this round the net's picks and actions are sampled (temperature 1) and recorded; None: greedy.
        self.sample_rel = None
        self.sample_key = None
        # Multi-turn GRPO (`armyrl`): {round: [B] bool} the rows that sample their picks and actions at that round (the
        # copies of a play share it); the map of the turn (MB nets) is kept per sampled round for the gradient pass.
        self.sample_plan = None
        self.turn_M = {}
        # The inputs of the memory's step at the end of some turns (`armymemgrad.Tape`, the RL gradient through the memory); None: not kept.
        self.mem_tape = None
        self.records = []
        self.row_ids = None
        self.verdict = mx.full((self.B,), -1, dtype=I32)
        self.city_low = mx.zeros((self.B,), dtype=mx.bool_)
        self.pr = armyprice.attach(self, plays)
        self.counts = collections.Counter()
        self.cand_fn = self.obs.candidates
        self.legacy = False
        self.body_fn = brain.body
        self.enemy_rule = "std"
        # The trainers' end of a unit's turn heals an unmoved unit (`act::hold_unit`, nn/army-net ed9e453); False: the
        # harness before it (moves spent by hand, no heal), for the dumps made before.
        self.hold_heals = True
        self.third_played = False  # forced runs of `armyenemy`: the third seat's turn is already in the state
        # The judge where Rust's looks (after our script; see `learner_turn`); False: after the enemy's turn (G1).
        self.judge_after_script = True
        self.turn_tile = None
        self.fort_rule = "fit"
        # The scene rule (`armyrule`, the trainers' default): both seats' pieces besides the net and the family script;
        # False: the port of standard (Rust ARMY_SCENE_RULE=standard).
        self.scene_rule = True
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
            body = mx.compile(lambda tok, um, pm, price, x: b.body(tok, um, pm, None, price, x))
            self.body_fn = lambda tok, um, pm, M, price, x=None: body(tok, um, pm, price, x)
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
        self._apply_economy(last)

    def schedule_step(self, rel, step):
        """The economy at Rust's moment (round, step) of each play where it differs from the one in force: Rust's
        economy can change inside our turn (a far city of ours grows between two decisions), the round's first moment
        alone left the city's token behind for the rest of the turn (G7 2a's drift of the notes). `step` None: our turn's
        end (the memory's step reads that moment's tokens)."""
        key = (1 << 32) - 1 if step is None else step
        last = getattr(self, "_last", None)
        if last is None:
            return
        # The far cities (outside the window, only their token rows in the dump) grow inside a turn too: their rows count.
        econ = lambda pi, m: (m["state"]["owner"], [c[3] for c in m["state"]["cities"]], m.get("cstr"), m.get("wm"), self._far_hex(pi, m))
        moved = False
        for pi in range(len(self.plays)):
            m = self.sched[pi].get((self.plays[pi][0]["orders"]["start_round"] + rel, key))
            if m is None or m is last[pi]:
                continue
            if last[pi] is None or econ(pi, m) != econ(pi, last[pi]):
                moved = True
                self.counts["economy moved inside a turn"] += 1
            last[pi] = m
        if moved:
            self._apply_economy(last)

    def _kx_move(self, new, h):
        """The profiles' combat percents follow each play's kind contexts `new` [P] (per slot a list, [] unknown; None: no
        change) where `h` [B]: the change since the last applied one is added to `p_ctx` (the twin's own promotions stay)."""
        U = len(self._kx_ap[0]) if self._kx_ap else 0
        delta, any_ = [], False
        for pi, kx in enumerate(new):
            ap = self._kx_ap[pi]
            row = [[0] * A.N_CTX for _ in range(U)]
            if kx is not None:
                for k in range(min(U, len(kx))):
                    if kx[k] and kx[k] != ap[k]:
                        row[k] = [a - b for a, b in zip(kx[k], ap[k])]
                        ap[k] = list(kx[k])
                        any_ = True
            delta.append(row)
        if not any_:
            return
        self.counts["kind contexts moved (a player's combat percents)"] += 1
        d = mx.take(mx.array(delta, dtype=self.s["p_ctx"].dtype), self.idx, axis=0)
        self.s["p_ctx"] = self.s["p_ctx"] + mx.where(h[:, None, None], d, 0)

    def _far_hex(self, pi, m):
        """The far cities' token rows of moment `m` (hex, as the dump has them)."""
        ftiles = {f[0] for f in self.plays[pi][0]["orders"]["far_cities"]}
        if not ftiles:
            return ()
        W = 4 * A.F_TOK
        h = m["tok"]["feat"]
        return tuple((v, h[i * W:(i + 1) * W]) for i, (k, v) in enumerate(m["tok"]["ref"]) if k == "c" and v in ftiles)

    def turn_end_economy(self, ended, rows=None):
        """The seats `ended` have ended their turn since the round's moment: their minds ran after it (a tenet, a tech, a
        city's growth). Their units' wounded modifiers and their cities' strengths from the next round's first Rust
        moment (no seat changes them before then), in `rows` [B] (all by default), where the play has such a moment; a
        unit dead by then takes its seat's change (`armyenemy.wm_after`). The forced checks set this in their rows."""
        rel = getattr(self, "_rel", None)
        if rel is None:
            return
        from .armyenemy import wm_after
        U, C = self.U, self.s["c_pop"].shape[1]
        last = getattr(self, "_last", [None] * len(self.plays))
        nw, cs, has = [], [], []
        for pi in range(len(self.plays)):
            m = self.sched[pi].get(self.plays[pi][0]["orders"]["start_round"] + rel + 1)
            cur = last[pi]
            ok = m is not None and cur is not None and "wm" in m and "wm" in cur
            has.append(ok)
            if not ok:
                nw.append([0] * U); cs.append([0] * C)
                continue
            units = cur["state"]["units"]
            w = wm_after((cur["wm"] + [0] * U)[:U], m, units, ended)
            nw.append((w + [0] * U)[:U])
            cc = m.get("cstr") or cur.get("cstr") or []
            cs.append(([c[0] for c in cc] + [0] * C)[:C])
        if not any(has):
            return
        s = self.s
        tk = lambda v, dt=I32: mx.take(mx.array(v, dtype=dt), self.idx, axis=0)
        h = tk(has, mx.bool_)
        if rows is not None:
            h = h & rows
        side_in = lambda a: mx.any(mx.stack([a == e for e in sorted(ended)]), axis=0)
        s["p_wounded_mod"] = mx.where(h[:, None] & side_in(s["u_side"]), tk(nw), s["p_wounded_mod"])
        if "c_str0" in s and C:
            s["c_str0"] = mx.where(h[:, None] & side_in(s["c_owner"]), tk(cs), s["c_str0"])
        if self._kx_ap is not None:
            # The kind contexts of the ended seats' units likewise (`armyenemy.kx_after`), as a change of the profiles'.
            from .armyenemy import kx_after
            sel = [False] * len(self.plays)
            for b, (pi, ok) in enumerate(zip(self.idx.tolist(), h.tolist())):
                sel[pi] = sel[pi] or ok
            new = []
            for pi in range(len(self.plays)):
                m = self.sched[pi].get(self.plays[pi][0]["orders"]["start_round"] + rel + 1)
                cur = last[pi]
                ok = sel[pi] and m is not None and cur is not None and "kx" in m and "kx" in cur
                new.append(kx_after(cur["kx"], m, cur["state"]["units"], ended) if ok else None)
            self._kx_move(new, h)

    def _apply_economy(self, last):
        s = self.s
        own, pop, cs0, cmax, wm, have = [], [], [], [], [], []
        T, C, U = self.sim.T, s["c_pop"].shape[1], self.U
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
        if self._kx_ap is not None:
            self._kx_move([m.get("kx") if m is not None else None for m in last], h)
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

    def xin(self, rel):
        """The streams the body reads besides the tokens and the memory (None: the net reads neither)."""
        b, s = self.brain, self.s
        if b.notes:
            return {"ns": {k: s["ns_" + k] for k in b.m.NOTE_STATE}}
        if b.hist:
            return {"hist": H.tokens(s, mx.full((self.B,), rel, dtype=I32), self.pm)}
        return None

    def learner_turn(self, rel):
        brain, s = self.brain, self.s
        B, U = self.B, self.U
        self.prev, self.this = self.this, self.snapshot()
        self.turn_tile = s["u_tile"]
        self.cur_rel = rel
        active = ~s["done"] & (self.verdict < 0)
        if self.facts:
            # Our turn's first look: the enemy's events since our last turn's end become facts of this round.
            H.turn_start(self, active, mx.full((B,), rel, dtype=I32))
        if self.compare:
            self.compare_state(rel)
        trace = [[] for _ in self.plays]
        made = mx.zeros((B, U), dtype=I32)
        for step in range(PER_TURN):
            if step > 0:
                self.schedule_step(rel, step)
            # Only the rows still deciding are computed (step 0: every live row; later: the active ones, compacted).
            n0 = int(mx.sum(active).item())
            if n0 == 0:
                break
            if step > 0 and n0 < B and self.compact:
                ids, pos = self._compact(active, n0)
                self.row_ids = ids
                saved = self._enter_rows(ids)
                made_v, act_v = mx.take(made, ids, axis=0), mx.ones((n0,), dtype=mx.bool_)
                pick, ai, act_v, made_v = self.decide(rel, step, act_v, made_v, trace)
                has = active
                self.row_ids = None
                self._leave_rows(saved, has, pos)
                made = self._merge(made, made_v, has, pos)
                active = mx.where(has, mx.take(act_v, pos, axis=0), False)
                if self.compare:
                    pick = mx.where(has, mx.take(pick, pos, axis=0), -1)
                    ai = mx.where(has, mx.take(ai, pos, axis=0), 0)
            else:
                pick, ai, active, made = self.decide(rel, step, active, made, trace)
            if self.compare:
                P = len(self.plays)
                al_ = active[:P].tolist(); pk_ = pick[:P].tolist(); ai_ = ai[:P].tolist()
                for b in range(P):
                    if al_[b]:
                        trace[b].append((pk_[b], ai_[b]))
        s = self.s
        self.counts["net turns"] += int(mx.sum(~s["done"] & (self.verdict < 0)).item())
        if self.compare:
            self.compare_turn(rel, trace)
        # Every army unit stands, the seat's mind ends our turn; the memory steps on the turn's last tokens.
        self.schedule_step(rel, None)
        s = self.s
        live = ~s["done"] & (self.verdict < 0)
        self.our_turn_end(live)
        if self.judge_after_script and rel >= 1:
            # Rust's round boundary (`trainers::play_on`): the driver reports the round done at the learner's first step,
            # after the seat's script (and the scene rule's part): the arena closes and the judge looks there, before
            # the mind's part of our turn and the enemy's turn (a city low after our strikes, before its heal).
            self.judge(rel)
        s = self.s
        tok = self.tokens(rel)
        x = self.xin(rel)
        if self.compare and self.facts:
            self.compare_moment(rel, None, mx.arange(B, dtype=I32), x, trace, tok=tok)
        if self.mem_tape is not None:
            self.mem_tape.note(self, rel, tok, x, live)
        h = self.body_fn(tok, self.um, self.pm, self.M, self.price, x)
        um2, pm2, al2 = brain.step(h, tok, self.um, self.pm, self.alive)
        if brain.notes:
            ns = {k: s["ns_" + k] for k in brain.m.NOTE_STATE}
            for k, v in brain.write_notes(h, tok, ns, self.alive, um2, pm2, al2, live).items():
                s["ns_" + k] = v
        if self.facts:
            H.push_plan(s, self.pm, live)
            H.turn_end(self, live)
        self.um = mx.where(live[:, None, None], um2, self.um)
        self.pm = mx.where(live[:, None, None], pm2, self.pm)
        self.alive = mx.where(live[:, None], al2, self.alive)
        self.s = self.sim.end_turn(s, mx.zeros((B,), dtype=I32), live)

    compact = True

    def decide(self, rel, step, active, made, trace=None):
        """One decision step over the current rows (all of them, or the compacted active ones): the tokens, the body, the
        pointer, the unit's candidates, the greedy action, applied. Returns (pick, action index, still active, made)."""
        brain, s = self.brain, self.s
        tok = trim_tokens(self.tokens(rel))
        unit = tok["unit"]
        stopped = mx.where(unit >= 0, gn(made, mx.maximum(unit, 0)) >= PER_UNIT, False)
        sel = tok["selectable"] & ~stopped & active[:, None]
        if step == 0 and brain.has_map:
            self.M = mx.where(active[:, None, None], brain.encode(self.statics, tok), self.M)
        x = self.xin(rel)
        if self.compare and self.facts:
            self.compare_moment(rel, step, self.row_ids if self.row_ids is not None else mx.arange(self.B, dtype=I32), x, trace, active, tok=tok)
        h = self.body_fn(tok, self.um, self.pm, self.M, self.price, x)
        lg = brain.pointer(h, sel)
        smask = None
        if self.sample_plan is not None:
            if rel in self.sample_plan:
                smask = self.sample_plan[rel]
                if self.row_ids is not None:
                    smask = mx.take(smask, self.row_ids)
                if step == 0 and brain.has_map:
                    self.turn_M[rel] = self.M
        elif self.sample_rel is not None and self.sample_rel == rel:
            smask = mx.ones(active.shape, dtype=mx.bool_)
        sampling = smask is not None
        if sampling:
            active_in = active & smask
            self.sample_key, k1, k2 = mx.random.split(self.sample_key, 3)
            pidx = mx.random.categorical(lg, axis=-1, key=k1).astype(I32)
            pick = mx.where(smask, mx.where(pidx >= sel.shape[1], -1, pidx), greedy_pointer(lg, sel.shape[1]))
        else:
            pick = greedy_pointer(lg, sel.shape[1])
        active = active & (pick >= 0)
        n_act = int(mx.sum(active).item())
        self.counts["net decisions"] += n_act
        if n_act == 0:
            if sampling:
                self._record(active_in, tok, sel, pidx, pick, None, None, rel, x)
            return pick, mx.zeros_like(pick), active, made
        pk = mx.maximum(pick, 0)
        me = mx.where(active, mx.take_along_axis(unit, pk[:, None], axis=1)[:, 0], 0)
        cand = self.cand_fn(s, me)
        al = brain.actions(h, pk, cand, self.M)
        if sampling:
            ai = mx.where(smask, mx.random.categorical(al, axis=-1, key=k2).astype(I32), A.first_index_eq(al, mx.max(al, axis=1)))
        else:
            ai = A.first_index_eq(al, mx.max(al, axis=1))
        kind = mx.take_along_axis(cand["kind"], ai[:, None], axis=1)[:, 0]
        tgt = mx.take_along_axis(cand["tgt"], ai[:, None], axis=1)[:, 0]
        if sampling:
            self._record(active_in, tok, sel, pidx, pick, cand, ai, rel, x)
        self.apply(me, kind, tgt, active, side=0, mk=None if self.legacy else cand["_mk"])
        made = put1(made, me, g1(made, me) + 1, active)
        return pick, ai, active, made

    def _record(self, active_in, tok, sel, pidx, pick, cand, ai, rel=None, x=None):
        """A sampled decision step for the gradient pass (`armygrpo`): the rows that were deciding, with their inputs (the
        trimmed tokens, the memory, the candidates' fields the action head reads), the sampled indices and the global row."""
        n = int(mx.sum(active_in).item())
        if n == 0:
            return
        ids, _ = self._compact(active_in, n)
        take = lambda a: mx.take(a, ids, axis=0)
        gid = take(self.row_ids if self.row_ids is not None else mx.arange(active_in.shape[0], dtype=I32))
        tk = ("feat", "dist", "mask", "slot") + (("pos",) if self.brain.has_map else ())
        rec = {"gid": gid, "tok": {k: take(tok[k]) for k in tk}, "sel": take(sel), "rel": rel,
               "um": take(self.um), "pm": take(self.pm), "price": take(self.price), "pidx": take(pidx), "pick": take(pick)}
        if x is not None:
            # The streams as the decision read them (inputs of the gradient pass: no gradient through the writing).
            rec["x"] = {k: {kk: take(vv) for kk, vv in v.items()} for k, v in x.items()}
        if cand is not None:
            ck = ("feat", "promo", "mask") + (("pos",) if self.brain.has_map else ())
            rec["cand"] = {k: take(cand[k]) for k in ck}
            rec["aidx"] = take(ai)
        self.records.append(rec)

    # The compacted step: the rows `ids` of every per-row array, the simulator and the tables (exact: rows are independent).
    ROW_ATTRS = ("B", "s", "tab", "far_rows", "start", "price", "um", "pm", "alive", "M", "pillaged", "prev", "cap_maxhp")

    def _compact(self, active, n):
        B = self.B
        ids = mx.argsort(mx.where(active, 0, B) + mx.arange(B, dtype=I32))[:n].astype(I32)
        pos = mx.maximum(mx.cumsum(active.astype(I32)) - 1, 0)
        return ids, pos

    def _enter_rows(self, ids):
        obs = self.obs
        saved = {k: getattr(self, k) for k in self.ROW_ATTRS}
        sim = self.sim
        saved["obs"] = (obs.tab, obs.t)
        saved["simrows"] = (sim.cs, sim.tid, sim.n_pieces, sim.B)
        take = lambda a: None if a is None else mx.take(a, ids, axis=0)
        self.B = ids.shape[0]
        self.s = {k: take(v) for k, v in saved["s"].items()}
        self._entered = dict(self.s)
        # The simulator and the observer stay the same objects (callers hold their methods): their row arrays swap.
        sub = A._rows_sim(sim, ids)
        sim.cs, sim.tid, sim.n_pieces, sim.B = sub.cs, sub.tid, sub.n_pieces, sub.B
        self.tab = A._rows_tab(saved["tab"], ids)
        obs.tab, obs.t = self.tab, self.tab.t
        for k in ("far_rows", "start", "price", "um", "pm", "alive", "M", "pillaged", "cap_maxhp"):
            setattr(self, k, take(saved[k]))
        self.prev = None if saved["prev"] is None else tuple(take(x) for x in saved["prev"])
        return saved

    def _merge(self, full, sub, has, pos):
        if full is None:
            return None
        sel = has.reshape((-1,) + (1,) * (full.ndim - 1))
        return mx.where(sel, mx.take(sub, pos, axis=0), full)

    def _leave_rows(self, saved, has, pos):
        sub_s, entered = self.s, self._entered
        full = dict(saved["s"])
        for k, v in sub_s.items():
            if k not in entered or v is not entered[k]:
                full[k] = self._merge(full[k], v, has, pos) if k in full else v
        pillaged = self._merge(saved["pillaged"], self.pillaged, has, pos)
        obs = self.obs
        obs.tab, obs.t = saved["obs"]
        sim = self.sim
        sim.cs, sim.tid, sim.n_pieces, sim.B = saved["simrows"]
        for k in self.ROW_ATTRS:
            setattr(self, k, saved[k])
        self.s, self.pillaged = full, pillaged
        self._entered = None

    def our_turn_end(self, live):
        """The end of the net's turn: every army unit stands (`act::stand`: no moves left), then the seat's AI mind
        fortifies some of them (`planner_units::hold` through its decision tree)."""
        if self.scene_rule:
            # Rule version 2 (twin package 7, `armyciv`): the rule plays our civilians after the net's turn.
            from . import armyciv
            armyciv.learner_pass(self, live)
        s = self.s
        army = self.tab.t["army"] >= 0
        s = self.sim.hold_units(s, army & (s["u_side"] == 0) & live[:, None] & (s["u_mv"] > 0), heal=self.hold_heals)
        if self.scene_rule:
            # The scene rule (`armyrule`, Rust `ARMY_SCENE_RULE=simple`) plays the rest of our seat's pieces.
            from . import armyrule
            self.s = s
            armyrule.seat_turn(self, 0, live)
            armycamp.mark(self)
            armyparty.mark(self)
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

    def compare_moment(self, rel, step, gids, x, trace, active=None, tok=None):
        """Gate G7's parity of the streams (the first copy against the Rust dump): at a decision moment (`step`) or our
        turn's end (`step` None), for each play still equal to Rust's (its state at every compared moment and its
        decisions so far), the history tokens and the note stream against the dump's moment."""
        P = len(self.plays)
        if not hasattr(self, "mok"):
            self.mok = [True] * P
        s = self.s
        gl = gids.tolist()
        act = active.tolist() if active is not None else [True] * len(gl)
        vd = self.verdict.tolist()
        rows = [(j, b) for j, b in enumerate(gl) if b < P and act[j] and self.mok[b] and vd[b] < 0]
        if not rows:
            return
        n = s["u_alive"].shape[0]
        ht = H.tokens(s, mx.full((n,), rel, dtype=I32))
        al, tl, hp, dmg = s["u_alive"].tolist(), s["u_tile"].tolist(), s["u_hp"].tolist(), s["c_dmg"].tolist()
        hf, hm, hs, ha = ht["hist"].tolist(), ht["hmask"].tolist(), ht["hslot"].tolist(), ht["hage"].tolist()
        notes = None
        if self.brain.notes:
            notes = {k: s["ns_" + k].tolist() for k in self.brain.m.NOTE_STATE}
        tfeat = tok["feat"].tolist() if tok is not None else None
        tn = tok["n"].tolist() if tok is not None else None
        c = self.hist_cmp
        for j, pi in rows:
            r = self.plays[pi][0]["orders"]["start_round"] + rel
            ms = [m for m in self.plays[pi][1] if m["round"] == r]
            dec = [m for m in ms if m["step"] < (1 << 32) - 1]
            if step is None:
                m = next((m for m in ms if m["step"] == (1 << 32) - 1), None)
                want = [(mm["pick"], mm["applied"]) for mm in dec]
                got = trace[pi]
                if not (got == want[:len(got)] and len(got) == len([w for w in want if w[0] >= 0])):
                    self.mok[pi] = False; c["stopped: decisions"] += 1
                    continue
            else:
                m = next((mm for mm in dec if mm["step"] == step), None)
                want = [(mm["pick"], mm["applied"]) for mm in dec if mm["step"] < step]
                if trace[pi][:step] != want:
                    self.mok[pi] = False; c["stopped: decisions"] += 1
                    continue
            if m is None:
                # Rust's turn ended before this look (no selectable unit: its loop breaks before the moment; or the play
                # was decided before our turn's end): nothing to compare here; the decisions are checked at the turn's end.
                c["no Rust moment (turn or play over)"] += 1
                continue
            st = m["state"]
            same = len(st["units"]) <= self.U and all((u[0] == 1) == bool(al[j][k]) and (u[0] != 1 or (u[1] == tl[j][k] and u[2] == hp[j][k]))
                                                     for k, u in enumerate(st["units"]))
            same = same and all(cc[1] == dmg[j][k] for k, cc in enumerate(st["cities"]))
            if not same:
                self.mok[pi] = False; c["stopped: state"] += 1
                continue
            c["moments compared"] += 1
            c["moments compared (turn ends)" if step is None else "moments compared (decisions)"] += 1
            if tfeat is not None:
                self.compare_tokens(c, pi, rel, step, m, tfeat[j], tn[j])
            rt = H.of_moment(m)
            if rt is not None:
                fs, sl, ag = rt
                cnt = sum(hm[j])
                ok = cnt == len(sl) and all(hs[j][i] == sl[i] and ha[j][i] == ag[i] and all(abs(a - b) <= 1e-6 for a, b in zip(hf[j][i], fs[i])) for i in range(min(cnt, len(sl))))
                c["history equal" if ok else "history differs"] += 1
                c["history tokens compared"] += len(sl)
                if not ok and sum(1 for e in self.hist_ex if not e.startswith("NOTES")) < 40:
                    first = next((i for i in range(min(cnt, len(sl))) if not (hs[j][i] == sl[i] and ha[j][i] == ag[i] and all(abs(a - b) <= 1e-6 for a, b in zip(hf[j][i], fs[i])))), None)
                    det = "" if first is None else f" token {first}: mlx {[round(v, 4) for v in hf[j][first]]} s{hs[j][first]} a{ha[j][first]} | rust {[round(v, 4) for v in fs[first]]} s{sl[first]} a{ag[first]}"
                    self.hist_ex.append(f"{self.plays[pi][0]['play']} rel {rel} step {step}: count mlx {cnt} rust {len(sl)}{det}")
            rn = m.get("notes")
            if notes is not None and rn is not None:
                ok = True
                why = ""
                for k, tol in (("stage", 0), ("stage_age", 0), ("engaged", 0), ("change_age", 0), ("reason", 0), ("rest", 0), ("rest_age", 0), ("waited", 1e-4), ("free", 1e-4)):
                    a, b = notes[k][j], rn[k]
                    a = a if isinstance(a, list) else [a]
                    b = b if isinstance(b, list) else [b]
                    if len(a) != len(b) or any(abs(x - y) > tol + 1e-6 for x, y in zip(a, b)):
                        ok = False
                        c[f"notes differ in {k}"] += 1
                        c["notes: largest difference x 1e6 in " + k] = max(c["notes: largest difference x 1e6 in " + k], int(1e6 * max(abs(x - y) for x, y in zip(a, b)))) if len(a) == len(b) else 10 ** 9
                        why = why or f"{k}: mlx {[round(v, 4) for v in a]} rust {[round(v, 4) for v in b]}"
                c["notes equal" if ok else "notes differ"] += 1
                if not ok and sum(1 for e in self.hist_ex if e.startswith("NOTES")) < 20:
                    self.hist_ex.append(f"NOTES {self.plays[pi][0]['play']} rel {rel} step {step}: {why}")

    def compare_tokens(self, c, pi, rel, step, m, feat, n):
        """The tokens of a compared moment against the dump's (G7 2a's diagnosis: the state check covers the units'
        alive / tile / hp and the cities' damage only; a token number outside it moves the memory)."""
        rf = A._unhex_tok(m["tok"]["feat"], m["tok"]["n"])
        if n != len(rf):
            c["tokens differ: count"] += 1
            return
        bad = [(i, k, feat[i][k], rf[i][k]) for i in range(n) for k in range(len(rf[i])) if abs(feat[i][k] - rf[i][k]) > 1e-6]
        if not bad:
            c["tokens equal"] += 1
            return
        c["tokens differ"] += 1
        for k in sorted({b[1] for b in bad}):
            c[f"tokens differ in number {k}"] += 1
        if sum(1 for e in self.hist_ex if e.startswith("TOK")) < 30:
            self.hist_ex.append(f"TOK {self.plays[pi][0]['play']} rel {rel} step {step}: " + "; ".join(f"t{i} #{k} mlx {a:.5f} rust {b:.5f}" for i, k, a, b in bad[:6]))

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
        """The candidate's action in the simulator (`mk`: the unit's masks on this state, when the caller has them); our
        army unit's command becomes a fact of the history (`armyhist`, when it is kept)."""
        if side == 0 and self.facts:
            H.log_cmd(self, me, H.fact_kind(kind), tgt, take, mx.full((self.B,), self.cur_rel, dtype=I32),
                      lambda: self._apply(me, kind, tgt, take, side, mk))
        else:
            self._apply(me, kind, tgt, take, side, mk)

    def _apply(self, me, kind, tgt, take, side, mk=None):
        owner0 = self.s["c_owner"]
        self._apply_raw(me, kind, tgt, take, side, mk)
        armycamp.check(self)
        s = self.s
        if side == 0 and "c_maxhp" in s:
            won = (owner0 != 0) & (s["c_owner"] == 0) & (self.cap_maxhp >= 0)
            s["c_maxhp"] = mx.where(won, self.cap_maxhp, s["c_maxhp"])

    def _apply_raw(self, me, kind, tgt, take, side, mk=None):
        s, sim = self.s, self.sim
        sd = mx.full((self.B,), side, dtype=I32)
        verb = mx.where(kind == A.K_MOVE, V_MOVE, mx.where((kind == A.K_UNIT) | (kind == A.K_CITY) | (kind == A.K_RANGED), V_ATTACK,
                        mx.where(kind == A.K_FORT, V_FORT, mx.where(kind == A.K_SETUP, V_SETUP, mx.where(kind == A.K_PROMO, V_PROMOTE, V_NONE)))))
        sim_take = take & (verb != V_NONE)
        if bool(mx.any(sim_take).item()):
            mk = mk if mk is not None else sim.masks(s, sd, me, rows=sim_take)
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
            # The pillager acts: its fortification ends (Rust: stance Ready after the pillage; the apply check's raid lines).
            s = sim.set_unit(s, me, pil, mv=mx.maximum(g1(s["u_mv"], me) - 60, 0), st=mx.zeros(me.shape, dtype=I32))
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
                mk = sim.masks(self.s, sd, me, rows=hit)
                self.s = sim.apply_unit(self.s, sd, me, mx.full((B,), V_ATTACK, dtype=I32), t, mk, hit)
            fld = mine & ~hit
            if bool(mx.any(fld).item()):
                s = self.s
                mk = sim.masks(s, sd, me, rows=fld)
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
                    mk = sim.masks(self.s, sd, me, rows=hit2)
                    self.s = sim.apply_unit(self.s, sd, me, mx.full((B,), V_ATTACK, dtype=I32), t2, mk, hit2)
            # The script's unit stands (`act::stand`).
            self.s = sim.hold_units(self.s, (mx.arange(U)[None, :] == u) & mine[:, None], heal=self.hold_heals)

    def third_turn(self, live):
        """The third seat's turn. Camp families (twin package 4): after the learner's, before the enemy's; the third party
        families (package 8; `armyparty`): after the enemy's (the dumps' order: learner, enemy, third). At its end its units'
        moves are back and the next mover's fortification counts one more turn."""
        party = armyparty.is_party(self) if "war" in self.s else mx.zeros((self.B,), dtype=mx.bool_)
        armycamp.neutral_turn(self, live & ~party)
        armycamp.neutral_rule(self, live & ~party)
        armyparty.third_turn(self, live)
        if "war" in self.s:
            self.s = self.sim.end_turn(self.s, mx.full((self.B,), 2, dtype=I32), live)

    def enemy_turn(self):
        live = ~self.s["done"] & (self.verdict < 0)
        party = armyparty.is_party(self) if "war" in self.s else mx.zeros((self.B,), dtype=mx.bool_)
        if not self.third_played:  # `armyenemy` forces the state after the third seat's turn
            self.third_camp(live & ~party)
            if "war" in self.s:
                self.turn_end_economy({2}, live & ~party)
        # The family script's first act (`assault::enemy_turn`): the reinforcements of assault-reinforce land.
        armyprice.land_reinforcements(self, ~self.s["done"] & (self.verdict < 0))
        if self.scene_rule:
            from . import armyrule
            armyrule.enemy_turn(self)
        elif self.enemy_rule == "std":
            from . import armystd
            armystd.enemy_turn(self, charge_on=self.charge)
        else:
            self.enemy_turn_approx()
        if not self.third_played and armyparty.has_family(self) and bool(mx.any(live & party).item()):
            self.turn_end_economy({1}, live & party)
            self.third_turn(~self.s["done"] & (self.verdict < 0) & party)

    def third_camp(self, live):
        """The camp families' third seat before the enemy (their dumps' order)."""
        armycamp.neutral_turn(self, live)
        armycamp.neutral_rule(self, live)
        if "war" in self.s:
            self.s = self.sim.end_turn(self.s, mx.full((self.B,), 2, dtype=I32), live)

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
                mk = sim.masks(self.s, sd, me, rows=hit)
                self.s = sim.apply_unit(self.s, sd, me, mx.full((B,), V_ATTACK, dtype=I32), t, mk, hit)
            # The field units of the assault families close in on the nearest of ours, then strike.
            fld = mine & ~hit & g1(self.field, me) & mx.array([f in ASSAULT for f in self.fam])
            if bool(mx.any(fld).item()):
                s = self.s
                mk = sim.masks(s, sd, me, rows=fld)
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
                    mk = sim.masks(self.s, sd, me, rows=hit2)
                    self.s = sim.apply_unit(self.s, sd, me, mx.full((B,), V_ATTACK, dtype=I32), t2, mk, hit2)
            # The rest hold: fortify where the rules let them.
            s = self.s
            rest = mine & ~hit & ~fld
            if bool(mx.any(rest).item()):
                mk = sim.masks(s, sd, me, rows=rest)
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
        # siege, ram
        g = is_f({"siege", "ram"})
        v = mx.where(g, mx.where(taken, W, mx.where(limit | ~mil, L, N)), v)
        # The seven assault families: the loss cap, the deadline, the play-on past the cap (`armyprice`).
        v = armyprice.assault_verdict(self, v, rel, taken, lost, mil, limit)
        g = is_f({"raid"})
        v = mx.where(g, mx.where((pill >= 3) & (lost <= 1), W, mx.where(limit | (lost >= 2), L, N)), v)
        g = is_f({"prey"})
        lose = self.either & (n_alive * 4 < n * 3)
        v = mx.where(g, mx.where(taken, W, mx.where(lose, L, mx.where(limit, mx.where(self.either & (n_alive * 4 >= n * 3), W, L), N))), v)
        g = is_f({"bombard"})
        v = mx.where(g, mx.where(taken | self.city_low, W, mx.where(limit | (n_alive == 0), L, N)), v)
        # The fog families (twin package 3; `armyfog`).
        from . import armyfog
        v = armyfog.judge(self, rel, v)
        v = armycamp.judge(self, rel, v)
        v = armyparty.judge(self, rel, v)
        # Guard, convoy, snipe (twin package 7; `armyciv`).
        from . import armyciv
        v = armyciv.judge(self, rel, v)
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
            armyparty.keep_world(self, rel)
            ta = time.time()
            self.learner_turn(rel)
            mx.eval(self.s)
            tb_ = time.time()
            self._rel = rel
            self.turn_end_economy({0})
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


# ------------------------------------------------------------------------------------------------ the family groups
# Every family module's pass (`armyrule.field_turn`, `armyfog`, `armyciv`, `armycamp`, `armyparty`, `armyprice`) runs over the
# whole batch of a game and is masked to its family's rows. In a game of one family group the other groups' passes are skipped
# (their state is absent); in a game that mixes groups every pass runs on every row, so a mix costs the SUM of the groups'
# passes at the full batch (line 7: five packages mixed ran at 595 scene-rounds/s, the packages alone at 1 957 - 4 949). Rows are
# independent, so a mixed batch is played as one game per group (`Batch`, `run`): the same plays, the same results.

GROUPS = (
    ("party", armythird.THIRD),
    ("camp", ("camp", "camp-race")),
    ("civ", ("guard", "convoy", "snipe")),
    ("fog", ("ambush-fog", "hunt-fog")),
    ("assault", tuple(sorted(ASSAULT | set(armyprice.ASSAULT)))),
)
# Scenes per group below which splitting a batch does not pay: a small batch is bound by kernel launches (about 2 us each), and
# every game repeats the net's and the simulator's own launches; a large one by the passes' reads of [B, ~100] arrays.
SPLIT_MIN_SCENES = 4000


def family_group(family):
    """The name of the group of passes the family's scene needs ("plain": none of the family modules)."""
    for name, fams in GROUPS:
        if family in fams:
            return name
    return "plain"


def split_plays(plays):
    """[(group name, [play indices])] in the order of the groups' first play; one entry when the plays are of one group."""
    out = collections.OrderedDict()
    for i, p in enumerate(plays):
        out.setdefault(family_group(p[0]["play"].split("/")[0]), []).append(i)
    return list(out.items())


def should_split(plays, copies, split="auto"):
    """Whether `plays` x `copies` are played as one game per group: `split` True / False, or "auto" (more than one group and
    at least `SPLIT_MIN_SCENES` scenes)."""
    if split is True or split == "on":
        return len(split_plays(plays)) > 1
    if split is False or split == "off":
        return False
    return len(split_plays(plays)) > 1 and len(plays) * copies >= SPLIT_MIN_SCENES


class Batch:
    """A batch of plays x copies played as one `Game` per family group (see above), the rows in the order of a single game
    (row = copy * len(plays) + play). `setup(game)` runs on each game before it plays. Games are built, played and released one
    after the other, so the peak memory is the largest group's."""

    def __init__(self, plays, copies, brain, split="auto", setup=None, **kw):
        self.plays, self.copies, self.brain, self.setup, self.kw = plays, copies, brain, setup, kw
        self.groups = split_plays(plays) if should_split(plays, copies, split) else [("all", list(range(len(plays))))]
        self.B = len(plays) * copies
        self.counts = collections.Counter()
        self.seconds = 0.0
        self.hist_cmp = collections.Counter()
        self.verdict = None
        self.peak = {}

    def play(self, limit_rounds=None):
        P = len(self.plays)
        out = [-1] * self.B
        for name, ids in self.groups:
            g = Game([self.plays[i] for i in ids], self.copies, self.brain, **self.kw)
            if self.setup:
                self.setup(g)
            v = g.play(limit_rounds).tolist()
            for r, x in enumerate(v):
                out[(r // len(ids)) * P + ids[r % len(ids)]] = x
            self.counts.update(g.counts)
            self.hist_cmp.update(g.hist_cmp)
            self.seconds += g.seconds
            self.peak[name] = mx.get_peak_memory()
            del g
            mx.clear_cache()
        self.verdict = mx.array(out, dtype=I32)
        return self.verdict


def run(path, weights, copies=1, families=None, limit=None, rust=None, out=None, group=None, rounds=None, setup=None, split="auto"):
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
    if not group and should_split(plays, copies, split):
        groups = [[plays[i] for i in ids] for _, ids in split_plays(plays)]  # one game per family group (`Batch`)
    per = collections.defaultdict(lambda: [0, 0, 0, 0, 0, 0])  # MLX wins, n, Rust wins, same verdict, plays, MLX wins (first copy)
    for gp in groups:
        g = Game(gp, copies, brain)
        if setup:
            setup(g)
        v = g.play(rounds).tolist()
        res["counts"].update(g.counts)
        res.setdefault("examples", []).extend(getattr(g, "ex", []))
        res.setdefault("examples_stance", []).extend(getattr(g, "ex_st", []))
        res.setdefault("streams", collections.Counter()).update(g.hist_cmp)
        res.setdefault("examples_streams", []).extend(g.hist_ex)
        res["dump_moments"] = res.get("dump_moments", 0) + sum(len(p[1]) for p in gp)
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
                res.setdefault("rows", {})[sc["play"]] = armyprice.row(g, b)
                per[key][2] += bool(rw)
                per[key][3] += (rw == win)
                per[key][4] += 1
                per[key][5] += win
    for f, (w, n, rw, same, plays_n, w1) in sorted(per.items()):
        res["families"][f] = {"plays": plays_n, "rust_win": round(100 * rw / max(plays_n, 1), 1), "mlx_win": round(100 * w1 / max(plays_n, 1), 1),
                              "same_verdict": same, "scenes": n, "mlx_win_all_copies": round(100 * w / max(n, 1), 1)}
    res["counts"] = dict(res["counts"])
    res["dump fields missing"] = A.missing_report()
    if "streams" in res:
        res["streams"] = dict(res["streams"])
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
    ap.add_argument("--split", choices=["auto", "on", "off"], default="auto", help="one game per family group (auto: more than one group and >= %d scenes)" % SPLIT_MIN_SCENES)
    ap.add_argument("--out")
    ap.add_argument("--rounds", type=int, help="stop after this many rounds (the throughput runs)")
    ap.add_argument("--cpu", action="store_true")
    ap.add_argument("--enemy", default="std", choices=["std", "approx"])
    ap.add_argument("--old-heal", action="store_true", help="the harness before act::hold_unit (no heal at a stand)")
    ap.add_argument("--fort", default="fit", choices=["fit", "old", "none"])
    ap.add_argument("--charge", action="store_true")
    ap.add_argument("--compile", action="store_true")
    ap.add_argument("--scene-rule", dest="scene_rule", action="store_true", default=True, help="the scene rule on both seats (the default, as in Rust)")
    ap.add_argument("--standard", dest="scene_rule", action="store_false", help="the port of standard instead of the rule (Rust ARMY_SCENE_RULE=standard)")
    ap.add_argument("--old-judge", action="store_true", help="judge after the enemy's turn (the G1 harness)")
    ap.add_argument("--play-on-cap", action="store_true", help="a broken loss cap does not end the play (Rust ARMY_PLAY_ON_CAP=1)")
    ap.add_argument("--rust-memory", action="store_true", help="the memory's step in Rust's arithmetic order (armymem)")
    a = ap.parse_args(argv)
    if a.cpu:
        mx.set_default_device(mx.cpu)
    mx.set_memory_limit(16 << 30)
    mx.set_cache_limit(2 << 30)
    def setup(g):
        g.enemy_rule, g.hold_heals, g.fort_rule, g.charge = a.enemy, not a.old_heal, a.fort, a.charge
        g.scene_rule = a.scene_rule
        g.pr.play_on = a.play_on_cap
        g.judge_after_script = not a.old_judge
        if a.rust_memory:
            g.brain.use_rust_memory()
        if a.compile:
            g.enable_compile()
    res = run(a.dump, a.weights, copies=a.copies, families=a.families.split(",") if a.families else None, limit=a.limit, out=a.out, group=a.group, rounds=a.rounds, setup=setup, split=a.split)
    res["setup"] = {"play_on_cap": a.play_on_cap, "enemy": a.enemy, "old_heal": a.old_heal, "fort": a.fort, "charge": a.charge, "scene_rule": a.scene_rule, "old_judge": a.old_judge}
    if a.out:
        with open(a.out, "w") as fh:
            json.dump(res, fh, indent=1)


if __name__ == "__main__":
    main()
