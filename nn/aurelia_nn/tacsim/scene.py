"""The converter: `ai-exam suite-dump` lines (Rust) -> a batch of scenes as MLX arrays.

A dump line holds the scene (window tiles, units with their compiled combat profile and step-cost
table, cities, the RNG state) and a scripted play (options, pick, state after each move). `load`
stacks N lines into one batch: static planes `m[...]`, the initial state `s[...]`, a pool of
step-cost tables, and (for the parity replay) the play as aligned per-step arrays.
"""

import json
import re

import mlx.core as mx

from . import exact

N_CTX = 23
CTX_FLANKING = 17
CTX_NEAR_CAPITAL = 19
VERBS = {"N": 0, "F": 1, "U": 2, "C": 3, "M": 4, "A": 5, "S": 6}
TERM_TYPES = {"none": 0, "feature": 1, "terrain": 2, "role": 3, "domain": 4, "class": 5, "always": 6, "near_improvement": 7}
FLAG_NAMES = ["move_after_attacking", "heavy_charge", "river_crossing", "amphibious", "no_defensive_bonus", "ranged_support_fire",
              "only_defensive", "city_attack_only", "must_set_up", "indirect_fire", "no_capture", "capture_defeated", "always_heal",
              "sapper", "ignore_zoc", "heal_outside_friendly", "great_general"]
STAT_NAMES = ["withdraw", "heal_on_kill", "wounded_mod", "attacks_per_turn", "xp_percent", "adjacent_damage", "enemy_aura",
              "aura_range", "heal_adjacent", "heal_friendly", "heal_neutral", "heal_enemy", "stacked_general", "general_aura"]
NEAR_IMP = re.compile(r"NearOwnImprovement \{ improvement: ImprovementId\((\d+)\), range: (\d+) \}")


class Vocab:
    """Strings of the dumps (roles, domains, classes) to small ids, shared by every batch."""

    def __init__(self):
        self.maps = {}

    def id(self, kind, value):
        m = self.maps.setdefault(kind, {"": 0})
        if value is None:
            value = ""
        return m.setdefault(str(value), len(m))


VOCAB = Vocab()


class Unsupported(Exception):
    pass


def parse_term(t):
    cond = t["cond"]
    kind = cond[0]
    if kind == "other":
        mm = NEAR_IMP.search(cond[1])
        if not mm:
            raise Unsupported(cond[1])
        return t["ctx"], TERM_TYPES["near_improvement"], int(mm.group(1)), int(mm.group(2)), t["pct"]
    if kind in ("feature", "terrain"):
        return t["ctx"], TERM_TYPES[kind], int(cond[1]), 0, t["pct"]
    if kind == "always":
        return t["ctx"], TERM_TYPES["always"], 0, 0, t["pct"]
    if kind in ("role", "domain", "class"):
        return t["ctx"], TERM_TYPES[kind], VOCAB.id(kind, cond[1]), 0, t["pct"]
    raise Unsupported(kind)


def check_supported(p):
    """Profiles the simulator does not model yet are refused (the converter says which)."""
    bad = []
    st = p["stats"]
    for k in ("withdraw", "adjacent_damage"):
        if st.get(k):
            bad.append(k)
    if p["flags"].get("capture_defeated"):
        bad.append("capture_defeated")
    if p["post_combat"] or p["situational_stats"] or p["terrain_rules"]:
        bad.append("post_combat/situational_stats/terrain_rules")
    if p["contexts"][CTX_NEAR_CAPITAL]:
        bad.append("near_capital")
    if p["suicide"] or p["domain"] == "Air":
        bad.append("air/suicide")
    if p["sight_base"] + 1 > 4:
        bad.append("sight > 3")
    return bad


def load_lines(path, limit=None, families=None):
    out = []
    with open(path) as f:
        for line in f:
            d = json.loads(line)
            if families and d["scene"]["family"] not in families:
                continue
            out.append(d)
            if limit and len(out) >= limit:
                break
    return out


def unit_row(row):
    return row  # [alive, tile, hp, moves, mmax, attacks, stance, xp, side, kind]


class Batch:
    """N scenes stacked; `u` unit slots, `c` city slots."""

    def __init__(self, dumps, u=None, c=None, k_terms=None):
        self.dumps = dumps
        B = len(dumps)
        sc0 = dumps[0]["scene"]
        self.w = sc0["window"]["w"]
        T = self.w * self.w
        self.b, self.t = B, T
        self.u = u or max(max(d["slots"], len(d["scene"]["profiles"])) for d in dumps) + 2
        self.c = c or max(1, max(len(d["scene"]["cities"]) for d in dumps))
        self.unsupported = {}
        terms_all = []
        for i, d in enumerate(dumps):
            ts = []
            for p in d["scene"]["profiles"]:
                bad = check_supported(p)
                try:
                    pt = [parse_term(t) for t in p["terms"]]
                except Unsupported as e:
                    bad.append(f"term {e}")
                    pt = []
                if bad:
                    self.unsupported[i] = bad
                ts.append(pt)
            terms_all.append(ts)
        self.k = k_terms or max(1, max((len(t) for ts in terms_all for t in ts), default=1))
        self._build(terms_all)

    def _build(self, terms_all):
        B, T, U, C, K = self.b, self.t, self.u, self.c, self.k
        tiles = [d["scene"]["tiles"] for d in self.dumps]

        def plane(name, dtype=mx.int32):
            return mx.array([[int(v) for v in t[name]] for t in tiles], dtype=dtype)

        m = {}
        for name in ("hills", "mountain", "water", "deep", "rough", "road"):
            m[name] = plane(name, mx.bool_)
        for name in ("feature", "terrain_kind", "ground_def", "improvement_def", "improvement", "sight_level"):
            m[name] = plane(name)
        m["river"] = mx.array([[[bool(x) for x in r] for r in t["river"]] for t in tiles])
        fo = []
        for d in self.dumps:
            row = []
            for side in range(2):
                bits = d["scene"]["found_ok"][side] if side < len(d["scene"].get("found_ok", [])) else ""
                row.append([c == "1" for c in bits] if bits else [False] * T)
            fo.append(row)
        m["found_ok"] = mx.array(fo)
        # Step-cost tables: one pool, deduplicated.
        pool, pool_ix = [], {}
        cost_id = []
        P = {k: [] for k in ["civilian", "is_ranged", "land", "sea", "embarks", "ranged_only_in_domain", "founder", "ignore_zoc"] + FLAG_NAMES}
        I = {k: [] for k in ["base_combat", "base_ranged", "range", "strength", "ranged_strength", "moves_land", "moves_embarked",
                             "embarked_defense", "sight", "embarked_sight", "role", "domain", "class", "capture_kind", "capture_moves", "kind"] + STAT_NAMES}
        ctx, tctx, ttype, targ, targ2, tpct = [], [], [], [], [], []
        for bi, d in enumerate(self.dumps):
            profs = d["scene"]["profiles"]
            row_cost = []
            rowP = {k: [] for k in P}
            rowI = {k: [] for k in I}
            rctx, rt = [], ([], [], [], [], [])
            for ui in range(U):
                if ui < len(profs):
                    p = profs[ui]
                    key = tuple(p["cost"])
                    if key not in pool_ix:
                        pool_ix[key] = len(pool)
                        pool.append(p["cost"])
                    row_cost.append(pool_ix[key])
                    fl = p["flags"]
                    vals = {"civilian": p["civilian"], "is_ranged": p["is_ranged"], "land": p["domain"] == "Land", "sea": p["domain"] == "Sea",
                            "embarks": p["embarks"], "ranged_only_in_domain": p["ranged_only_in_domain"], "founder": p.get("founder", False),
                            "ignore_zoc": p["ignore_zoc"]}
                    for k in FLAG_NAMES:
                        vals[k] = fl.get(k, False)
                    for k in P:
                        rowP[k].append(bool(vals[k]))
                    st = p["stats"]
                    iv = {"base_combat": p["base_combat"], "base_ranged": p["base_ranged"], "range": p["range"], "strength": p["strength"],
                          "ranged_strength": p["ranged_strength"], "moves_land": p["moves_land"], "moves_embarked": p["moves_embarked"],
                          "embarked_defense": p["embarked_defense"], "sight": p["sight_base"],
                          "embarked_sight": max(1, min(7, 1 + st.get("embarked_sight", 0))),
                          "role": VOCAB.id("role", p["role"]), "domain": VOCAB.id("domain", p["domain"]), "class": VOCAB.id("class", p["class"]) if p["class"] else 0,
                          "capture_kind": -1 if p["capture_kind"] is None else p["capture_kind"],
                          "capture_moves": p.get("capture_moves") or 0, "kind": p["kind_id"]}
                    for k in STAT_NAMES:
                        iv[k] = st.get(k, 0)
                    for k in I:
                        rowI[k].append(int(iv[k]))
                    rctx.append(p["contexts"])
                    ts = terms_all[bi][ui] + [(0, 0, 0, 0, 0)] * (K - len(terms_all[bi][ui]))
                else:
                    # A slot past the pieces holds only a captured civilian (the scene's capture kind):
                    # its profile is set here once, so a capture writes the unit's state only.
                    cap = next((q for q in profs if q["capture_kind"] is not None), None)
                    row_cost.append(0)
                    for k in P:
                        rowP[k].append(cap is not None and k in ("civilian", "land"))
                    civ = {"sight": 2, "embarked_sight": 1, "moves_land": cap["capture_moves"] or 0, "kind": cap["capture_kind"]} if cap else {}
                    for k in I:
                        rowI[k].append(civ.get(k, -1 if k in ("capture_kind", "kind") else 0))
                    rctx.append([0] * N_CTX)
                    ts = [(0, 0, 0, 0, 0)] * K
                for j, arr in enumerate(rt):
                    arr.append([t[j] if j < 4 else t[4] for t in ts])
            cost_id.append(row_cost)
            for k in P:
                P[k].append(rowP[k])
            for k in I:
                I[k].append(rowI[k])
            ctx.append(rctx)
            tctx.append(rt[0]); ttype.append(rt[1]); targ.append(rt[2]); targ2.append(rt[3]); tpct.append(rt[4])
        self.cost_pool = mx.array(pool, dtype=mx.int32).reshape(len(pool), T, 6)
        p = {k: mx.array(v) for k, v in P.items()}
        p.update({k: mx.array(v, dtype=mx.int32) for k, v in I.items()})
        p["cost_id"] = mx.array(cost_id, dtype=mx.int32)
        p["ctx"] = mx.array(ctx, dtype=mx.int32)
        p["tctx"] = mx.array(tctx, dtype=mx.int32)
        p["ttype"] = mx.array(ttype, dtype=mx.int32)
        p["targ"] = mx.array(targ, dtype=mx.int32)
        p["targ2"] = mx.array(targ2, dtype=mx.int32)
        p["tpct"] = mx.array(tpct, dtype=mx.int32)
        self.m = m
        self.p = p
        self.n_pieces = mx.array([len(d["scene"]["profiles"]) for d in self.dumps], dtype=mx.int32)
        # Cities.
        cs = {k: [] for k in ["alive", "tile", "max_hp", "str0", "pop0", "gsp", "rsp", "draws", "orig"]}
        for d in self.dumps:
            cl = d["scene"]["cities"]
            for k in cs:
                cs[k].append([])
            for ci in range(C):
                if ci < len(cl):
                    c = cl[ci]
                    vals = {"alive": True, "tile": c["tile"], "max_hp": c["max_hp"], "str0": c["strength_no_garrison"], "pop0": c["population"],
                            "gsp": c["garrison_strike_percent"], "rsp": c["ranged_strike_percent"], "draws": c["capture_draws"], "orig": c["original_owner"]}
                else:
                    vals = {"alive": False, "tile": -1, "max_hp": 1, "str0": 0, "pop0": 0, "gsp": 0, "rsp": 0, "draws": 0, "orig": -1}
                for k in cs:
                    cs[k][-1].append(vals[k])
        self.cstatic = {k: mx.array(v) if k == "alive" else mx.array(v, dtype=mx.int32) for k, v in cs.items()}
        self.cstatic["side_cities"] = mx.array([d["scene"].get("city_counts", [9, 9]) for d in self.dumps], dtype=mx.int32)

    def state_from(self, rows_per_scene):
        """A state dict from Rust state records (one per scene): units, cities, rng, owners."""
        B, U, C = self.b, self.u, self.c
        cols = {k: [] for k in ["alive", "tile", "hp", "mv", "mmax", "att", "st", "xp", "side", "kind"]}
        for st in rows_per_scene:
            units = st["units"]
            for k in cols:
                cols[k].append([])
            for ui in range(U):
                r = units[ui] if ui < len(units) else [0, -1, 0, 0, 0, 0, 0, 0, -1, -1]
                for j, k in enumerate(cols):
                    cols[k][-1].append(int(r[j]))
        s = {"u_" + k: (mx.array(v, dtype=mx.int32) if k != "alive" else mx.array(v).astype(mx.bool_)) for k, v in cols.items()}
        cc = {k: [] for k in ["owner", "dmg", "strike", "pop"]}
        for st in rows_per_scene:
            for k in cc:
                cc[k].append([])
            for ci in range(C):
                r = st["cities"][ci] if ci < len(st["cities"]) else [-1, 0, False, 0]
                cc["owner"][-1].append(int(r[0])); cc["dmg"][-1].append(int(r[1])); cc["strike"][-1].append(bool(r[2])); cc["pop"][-1].append(int(r[3]))
        s["c_owner"] = mx.array(cc["owner"], dtype=mx.int32)
        s["c_dmg"] = mx.array(cc["dmg"], dtype=mx.int32)
        s["c_strike"] = mx.array(cc["strike"])
        s["c_pop"] = mx.array(cc["pop"], dtype=mx.int32)
        s["owner"] = mx.array([[{"0": 0, "1": 1, "2": 2}.get(ch, -1) for ch in st["owner"]] for st in rows_per_scene], dtype=mx.int32)
        s["rng"] = exact.u64_from_str([st["rng"] for st in rows_per_scene])
        return s

    def initial_state(self):
        s = self.state_from([d["initial"] for d in self.dumps])
        s.update(self.profile_state())
        s["n_slots"] = mx.array([max(len(d["scene"]["profiles"]), sum(1 for r in d["initial"]["units"] if True)) for d in self.dumps], dtype=mx.int32)
        s["done"] = mx.zeros((self.b,), dtype=mx.bool_)
        return s

    def profile_state(self):
        return {"p_" + k: v for k, v in self.p.items()}


def take_scenes(batch_arrays, idx):
    """Replicate scenes: every array's first axis indexed by `idx` (for the throughput batches)."""
    return {k: mx.take(v, idx, axis=0) for k, v in batch_arrays.items()}
