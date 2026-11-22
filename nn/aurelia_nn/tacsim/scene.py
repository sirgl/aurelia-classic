"""The converter: `ai-exam suite-dump` lines (Rust) -> a batch of scenes as MLX arrays.

A dump line holds the scene (window tiles, units with their compiled combat profile, step-cost table
and promotion changes, cities, the RNG state) and a scripted play (options, pick, state after each
move). `Batch` stacks N lines: static planes `m[...]`, the initial state, pools of step-cost and
stopping tables, the promotion tables, the profiles of units a capture brings, and (for the parity
replay) the play as aligned per-step arrays.

The data-oriented form of promotions: a unit's profile is its piece's profile plus, for each promotion
it takes, that promotion's change (contexts, stats, flags, moves, sight, range: additive in the rules'
`UnitModifiers::apply`; the dump checks every pair for additivity). Situational terms of a promotion
are rows of the unit's term table gated by the promotion's bit. The movement tables (step costs,
stopping, withdrawal) are not additive: a table per set of the promotions that change them (one or two).
"""

import json
import re

import mlx.core as mx

from . import exact

N_CTX = 23
CTX_FLANKING = 17
CTX_NEAR_CAPITAL = 19
CTX_EMBARKED_DEF = 20
CTX_INTERCEPTION = 21
VERBS = {"N": 0, "F": 1, "U": 2, "C": 3, "M": 4, "A": 5, "S": 6, "P": 7}
TERM_TYPES = {"none": 0, "feature": 1, "terrain": 2, "role": 3, "domain": 4, "class": 5, "always": 6, "near_improvement": 7}
FLAG_NAMES = ["move_after_attacking", "heavy_charge", "river_crossing", "amphibious", "no_defensive_bonus", "ranged_support_fire",
              "only_defensive", "city_attack_only", "must_set_up", "indirect_fire", "no_capture", "capture_defeated", "always_heal",
              "sapper", "ignore_zoc", "heal_outside_friendly", "great_general", "heal_on_kill_not_barbarians", "air_sweep",
              "cross_mountains", "hovering"]
STAT_NAMES = ["withdraw", "heal_on_kill", "wounded_mod", "attacks_per_turn", "xp_percent", "adjacent_damage", "enemy_aura",
              "aura_range", "heal_adjacent", "heal_friendly", "heal_neutral", "heal_enemy", "stacked_general", "general_aura",
              "intercept_chance", "interceptions_per_turn", "evasion", "interception_taken", "xp_needed_pct", "adjacent_damage_chance"]
# Per-unit scalars of the profile (the stats above join them).
INT_NAMES = ["base_combat", "base_ranged", "range", "strength", "ranged_strength", "moves_land", "moves_embarked",
             "embarked_defense", "sight", "embarked_sight", "role", "domain", "class", "capture_kind", "capture_moves", "kind",
             "air_intercept_range", "civ_spawn"]
BOOL_NAMES = ["civilian", "is_ranged", "land", "sea", "air", "embarks", "ranged_only_in_domain", "founder", "suicide"] + FLAG_NAMES
# Profile fields a promotion may change, and where they go (profile field -> per-unit array).
DELTA_INT = {"base_ranged": "base_ranged", "range": "range", "moves_land": "moves_land", "moves_embarked": "moves_embarked",
             "embarked_defense": "embarked_defense", "sight_base": "sight", "sight_embarked": "embarked_sight"}
# Changes the simulator reads elsewhere or that do not change play (the movement tables come per set).
DELTA_IGNORED = {"terrain_rules", "cost", "dest_ok", "withdraw_ok", "embarks", "ignore_zoc"}
NEAR_IMP = re.compile(r"NearOwnImprovement \{ improvement: ImprovementId\((\d+)\), range: (\d+) \}")
MAX_MOVING = 3  # promotions that change a unit's movement tables, at most (2^3 sets)
TERM_COLS = ("tctx", "ttype", "targ", "targ2", "tpct", "tprom")
# Bools a promotion may set: the flags and `embarks` (Embarkation).
FLAG_DELTA = FLAG_NAMES + ["embarks"]


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
    if st.get("adjacent_damage"):
        bad.append("adjacent_damage")
    # Terrain rules (double move, impassable) are read by navigation only: the step-cost table holds them.
    if p["post_combat"] or p["situational_stats"]:
        bad.append("post_combat/situational_stats")
    if p["contexts"][CTX_NEAR_CAPITAL]:
        bad.append("near_capital")
    if p["flags"].get("air_sweep"):
        bad.append("air_sweep")
    pr = p.get("promo")
    if pr and pr.get("nonadditive"):
        bad.append(f"nonadditive promotions {pr['nonadditive'][:3]}")
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


def profile_values(p):
    """The per-unit scalars (bools, ints) of one dumped profile."""
    fl, st = p["flags"], p["stats"]
    b = {"civilian": p["civilian"], "is_ranged": p["is_ranged"], "land": p["domain"] == "Land", "sea": p["domain"] == "Sea",
         "air": p["domain"] == "Air", "embarks": p["embarks"], "ranged_only_in_domain": p["ranged_only_in_domain"],
         "founder": p.get("founder", False), "suicide": p.get("suicide", False)}
    for k in FLAG_NAMES:
        b[k] = fl.get(k, False)
    b["ignore_zoc"] = p["ignore_zoc"]
    i = {"base_combat": p["base_combat"], "base_ranged": p["base_ranged"], "range": p["range"], "strength": p["strength"],
         "ranged_strength": p["ranged_strength"], "moves_land": p["moves_land"], "moves_embarked": p["moves_embarked"],
         "embarked_defense": p["embarked_defense"], "sight": p["sight_base"],
         "embarked_sight": p.get("sight_embarked", max(1, min(7, 1 + st.get("embarked_sight", 0)))),
         "role": VOCAB.id("role", p["role"]), "domain": VOCAB.id("domain", p["domain"]), "class": VOCAB.id("class", p["class"]) if p["class"] else 0,
         "capture_kind": -1 if p["capture_kind"] is None else p["capture_kind"],
         "capture_moves": p.get("capture_moves") or 0, "kind": p["kind_id"], "air_intercept_range": p.get("air_intercept_range", 0),
         "civ_spawn": p.get("civ_spawn", -1)}
    for k in STAT_NAMES:
        i[k] = st.get(k, 0)
    return {k: bool(v) for k, v in b.items()}, {k: int(v) for k, v in i.items()}


def bits(s, n):
    return [c == "1" for c in s] if s else [False] * n


class Batch:
    """N scenes stacked; `u` unit slots, `c` city slots, `k` term rows, `np` promotions, `ns` spawn profiles."""

    def __init__(self, dumps, u=None, c=None, k_terms=None):
        self.dumps = dumps
        B = len(dumps)
        sc0 = dumps[0]["scene"]
        self.w = sc0["window"]["w"]
        T = self.w * self.w
        self.b, self.t = B, T
        self.u = u or max(max(d["slots"], len(d["scene"]["profiles"])) for d in dumps) + 2
        self.c = c or max(1, max(len(d["scene"]["cities"]) for d in dumps))
        self.np = max(1, max(len(d["scene"].get("promotions", [])) for d in dumps))
        self.ns = max(1, max(len(d["scene"].get("spawn", [])) for d in dumps))
        self.unsupported = {}
        self.odd_promos = {}
        self.pool, self.pool_ix = [], {}
        self.dpool, self.dpool_ix = [], {}
        # Term rows of each profile: its own terms, then each choosable promotion's added terms (gated).
        terms_all, spawn_terms = [], []
        for i, d in enumerate(dumps):
            ts = []
            for p in d["scene"]["profiles"]:
                bad = check_supported(p)
                rows = self._terms(p, bad)
                if bad:
                    self.unsupported.setdefault(i, []).extend(bad)
                ts.append(rows)
            terms_all.append(ts)
            spawn_terms.append([self._terms(p, []) for p in d["scene"].get("spawn", [])])
        self.k = k_terms or max(1, max((len(t) for ts in terms_all + spawn_terms for t in ts), default=1))
        self._build(terms_all, spawn_terms)

    def _terms(self, p, bad):
        rows = []
        try:
            rows = [parse_term(t) + (-1,) for t in p["terms"]]
            pr = p.get("promo") or {}
            for key, dl in (pr.get("delta") or {}).items():
                if not dl:
                    continue
                td = dl.get("terms")
                if td:
                    if td.get("removed"):
                        bad.append("promotion removes a term")
                    rows += [parse_term(t) + (int(key),) for t in td["added"]]
        except Unsupported as e:
            bad.append(f"term {e}")
        return rows

    def _pool(self, table):
        key = tuple(table)
        if key not in self.pool_ix:
            self.pool_ix[key] = len(self.pool)
            self.pool.append(list(table))
        return self.pool_ix[key]

    def _dpool(self, dest, withdraw):
        key = (dest, withdraw)
        if key not in self.dpool_ix:
            self.dpool_ix[key] = len(self.dpool)
            self.dpool.append([bits(dest, self.t), bits(withdraw, self.t)])
        return self.dpool_ix[key]

    def _moving_tables(self, p, bad):
        """The movement tables of each set of the promotions that change them: ([moving local ids],
        {set mask: (cost id, dest id)})."""
        pr = p.get("promo") or {}
        delta = pr.get("delta") or {}
        moving = sorted(int(k) for k, dl in delta.items() if dl and any(x in dl for x in ("cost", "dest_ok", "withdraw_ok", "embarks", "ignore_zoc")))
        if len(moving) > MAX_MOVING:
            bad.append(f"{len(moving)} promotions change movement")
            moving = moving[:MAX_MOVING]

        def tables(dl):
            cost = list(p["cost"])
            for i, ch in (dl or {}).get("cost", []):
                cost[i] += ch
            dest = (dl or {}).get("dest_ok", p.get("dest_ok", ""))
            wd = (dl or {}).get("withdraw_ok", p.get("withdraw_ok", ""))
            return self._pool(cost), self._dpool(dest, wd)

        combos = {0: tables(None)}
        for j, m in enumerate(moving):
            combos[1 << j] = tables(delta[str(m)])
        for pair in pr.get("pairs", []):
            a, b2 = pair["set"]
            if a in moving and b2 in moving:
                combos[(1 << moving.index(a)) | (1 << moving.index(b2))] = tables(pair["delta"])
        return moving, combos

    def _build(self, terms_all, spawn_terms):
        B, T, U, C, K, NP, NS = self.b, self.t, self.u, self.c, self.k, self.np, self.ns
        M = 1 << MAX_MOVING
        tiles = [d["scene"]["tiles"] for d in self.dumps]

        def plane(name, dtype=mx.int32, default=0):
            return mx.array([[int(v) for v in t[name]] if name in t else [default] * T for t in tiles], dtype=dtype)

        m = {}
        for name in ("hills", "mountain", "water", "deep", "rough", "road"):
            m[name] = plane(name, mx.bool_)
        for name in ("feature", "terrain_kind", "ground_def", "improvement_def", "improvement", "sight_level", "citadel_dmg"):
            m[name] = plane(name)
        m["river"] = mx.array([[[bool(x) for x in r] for r in t["river"]] for t in tiles])
        fo = []
        for d in self.dumps:
            row = []
            for side in range(2):
                fbits = d["scene"]["found_ok"][side] if side < len(d["scene"].get("found_ok", [])) else ""
                row.append(bits(fbits, T))
            fo.append(row)
        m["found_ok"] = mx.array(fo)

        P = {k: [] for k in BOOL_NAMES}
        I = {k: [] for k in INT_NAMES + STAT_NAMES}
        X = {k: [] for k in ("cost_id", "dest_id", "ctx", "capture_spawn") + TERM_COLS}
        dints = list(DELTA_INT.values()) + STAT_NAMES
        # Promotion tables (per scene and unit): fit, the change of each, the moving set and its tables.
        Z = {k: [] for k in ("fit", "odd", "d_ctx", "cmov", "combo_cost", "combo_dest") + tuple("d_" + k for k in dints) + tuple("f_" + k for k in FLAG_DELTA)}
        S = {k: [] for k in BOOL_NAMES + INT_NAMES + STAT_NAMES + ["cost_id", "dest_id", "ctx", "prom", "level"] + list(TERM_COLS)}

        def term_cols(ts):
            ts = ts + [(0, 0, 0, 0, 0, -1)] * (K - len(ts))
            return [[t[j] for t in ts] for j in range(6)]

        for bi, d in enumerate(self.dumps):
            profs = d["scene"]["profiles"]
            bad = self.unsupported.setdefault(bi, [])
            rowsP = {k: [] for k in P}
            rowsI = {k: [] for k in I}
            rowsX = {k: [] for k in X}
            rowsZ = {k: [] for k in Z}
            for ui in range(U):
                if ui < len(profs):
                    p = profs[ui]
                    bv, iv = profile_values(p)
                    moving, combos = self._moving_tables(p, bad)
                    cid, did = combos[0]
                    tc = term_cols(terms_all[bi][ui])
                    cap = p.get("capture_spawn") or []
                    rowsX["capture_spawn"].append(list(cap) + [-1] * (U - len(cap)))
                    pr = p.get("promo") or {}
                    delta = pr.get("delta") or {}
                    fit = set(pr.get("fit") or [])
                    rowsZ["fit"].append([j in fit for j in range(NP)])
                    dctx, dI, dF, cmov, dodd = [], {k: [] for k in dints}, {k: [] for k in FLAG_DELTA}, [], []
                    for j in range(NP):
                        dl = delta.get(str(j)) or {}
                        row = [0] * N_CTX
                        for ci, ch in dl.get("contexts", []):
                            row[ci] += ch
                        dctx.append(row)
                        for src, dst in DELTA_INT.items():
                            dI[dst].append(int(dl.get(src, 0)))
                        sd = dl.get("stats", {})
                        for k in STAT_NAMES:
                            dI[k].append(int(sd.get(k, 0)))
                        fd = dl.get("flags", {})
                        for k in FLAG_NAMES:
                            dF[k].append(bool(fd.get(k, False)))
                        dF["embarks"].append(bool(dl.get("embarks", False)))
                        # A change the simulator does not model marks the promotion (taking it is counted
                        # in the state's `n_unmodelled`).
                        odd = [k for k in dl if k not in DELTA_INT and k not in DELTA_IGNORED and k not in ("contexts", "stats", "flags", "terms")]
                        odd += [k for k in sd if k not in STAT_NAMES and k not in ("range", "sight", "embarked_sight", "gg_points_percent", "capital_falloff")]
                        odd += [k for k in fd if k not in FLAG_NAMES and k != "insta_heal"]
                        if odd:
                            self.odd_promos.setdefault(bi, set()).update(odd)
                        dodd.append(bool(odd))
                        cmov.append(moving.index(j) if j in moving else -1)
                    rowsZ["d_ctx"].append(dctx)
                    for k, v in dI.items():
                        rowsZ["d_" + k].append(v)
                    for k, v in dF.items():
                        rowsZ["f_" + k].append(v)
                    rowsZ["cmov"].append(cmov)
                    rowsZ["odd"].append(dodd)
                    rowsZ["combo_cost"].append([combos.get(mm, (-1, -1))[0] for mm in range(M)])
                    rowsZ["combo_dest"].append([combos.get(mm, (-1, -1))[1] for mm in range(M)])
                    rowsX["ctx"].append(p["contexts"])
                else:
                    # A slot past the pieces holds a unit the play creates: a captured civilian (the
                    # scene's capture kind) unless a capture writes another profile (`spawn`).
                    cap = next((q for q in profs if q["capture_kind"] is not None), None)
                    bv = {k: cap is not None and k in ("civilian", "land") for k in P}
                    civ = {"sight": 2, "embarked_sight": 1, "moves_land": cap["capture_moves"] or 0, "kind": cap["capture_kind"]} if cap else {}
                    iv = {k: civ.get(k, -1 if k in ("capture_kind", "kind", "civ_spawn") else 0) for k in I}
                    cid, did = 0, 0
                    tc = term_cols([])
                    rowsX["capture_spawn"].append([-1] * U)
                    rowsZ["fit"].append([False] * NP)
                    rowsZ["d_ctx"].append([[0] * N_CTX] * NP)
                    for k in dints:
                        rowsZ["d_" + k].append([0] * NP)
                    for k in FLAG_DELTA:
                        rowsZ["f_" + k].append([False] * NP)
                    rowsZ["cmov"].append([-1] * NP)
                    rowsZ["odd"].append([False] * NP)
                    rowsZ["combo_cost"].append([-1] * M)
                    rowsZ["combo_dest"].append([-1] * M)
                    rowsX["ctx"].append([0] * N_CTX)
                for k in P:
                    rowsP[k].append(bv[k])
                for k in I:
                    rowsI[k].append(iv[k])
                rowsX["cost_id"].append(cid)
                rowsX["dest_id"].append(did)
                for j, name in enumerate(TERM_COLS):
                    rowsX[name].append(tc[j])
            for k in P:
                P[k].append(rowsP[k])
            for k in I:
                I[k].append(rowsI[k])
            for k in X:
                X[k].append(rowsX[k])
            for k in Z:
                Z[k].append(rowsZ[k])
            # Spawn profiles (units a capture brings).
            sp = d["scene"].get("spawn", [])
            rowsS = {k: [] for k in S}
            for si in range(NS):
                if si < len(sp):
                    q = sp[si]
                    bv, iv = profile_values(q)
                    cid, did = self._pool(q["cost"]), self._dpool(q.get("dest_ok", ""), q.get("withdraw_ok", ""))
                    ctx = q["contexts"]
                    prom = bits(q.get("prom_bits", ""), NP)
                    prom = prom + [False] * (NP - len(prom))
                    level = q.get("level", 1)
                    tc = term_cols(spawn_terms[bi][si])
                    bad.extend(f"spawn: {x}" for x in check_supported(q))
                else:
                    bv, iv = {k: False for k in P}, {k: 0 for k in I}
                    cid, did, ctx, tc, prom, level = 0, 0, [0] * N_CTX, term_cols([]), [False] * NP, 1
                for k in P:
                    rowsS[k].append(bv[k])
                for k in I:
                    rowsS[k].append(iv[k])
                rowsS["cost_id"].append(cid)
                rowsS["dest_id"].append(did)
                rowsS["ctx"].append(ctx)
                rowsS["prom"].append(prom)
                rowsS["level"].append(level)
                for j, name in enumerate(TERM_COLS):
                    rowsS[name].append(tc[j])
            for k in S:
                S[k].append(rowsS[k])
        if not self.pool:
            self.pool.append([-2] * (T * 6))
        if not self.dpool:
            self.dpool.append([[False] * T, [False] * T])
        self.cost_pool = mx.array(self.pool, dtype=mx.int32).reshape(len(self.pool), T, 6)
        self.dest_pool = mx.array([x[0] for x in self.dpool])
        self.withdraw_pool = mx.array([x[1] for x in self.dpool])
        p = {k: mx.array(v) for k, v in P.items()}
        p.update({k: mx.array(v, dtype=mx.int32) for k, v in I.items()})
        p.update({k: mx.array(v, dtype=mx.int32) for k, v in X.items()})
        self.m = m
        self.p = p
        self.z = {k: (mx.array(v) if k in ("fit", "odd") or k.startswith("f_") else mx.array(v, dtype=mx.int32)) for k, v in Z.items()}
        self.sp = {k: (mx.array(v) if k in P or k == "prom" else mx.array(v, dtype=mx.int32)) for k, v in S.items()}
        # The scene's promotion table: prerequisites (all of, any of) and the instant heal.
        req = [[[False] * NP for _ in range(NP)] for _ in range(B)]
        anyof = [[[False] * NP for _ in range(NP)] for _ in range(B)]
        has_any = [[False] * NP for _ in range(B)]
        insta = [[False] * NP for _ in range(B)]
        for bi, d in enumerate(self.dumps):
            for j, pr in enumerate(d["scene"].get("promotions", [])):
                insta[bi][j] = pr["insta_heal"]
                for q in pr["requires"]:
                    if q >= 0:
                        req[bi][j][q] = True
                for q in pr["requires_any"]:
                    if q >= 0:
                        anyof[bi][j][q] = True
                has_any[bi][j] = len(pr["requires_any"]) > 0
        self.z.update({"req": mx.array(req), "anyof": mx.array(anyof), "has_any": mx.array(has_any), "insta": mx.array(insta)})
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
        self.unsupported = {k: v for k, v in self.unsupported.items() if v}

    def extra(self):
        """The static tables besides the tiles and the cities (promotions `z_*`, spawn profiles `sp_*`)."""
        out = {"z_" + k: v for k, v in self.z.items()}
        out.update({"sp_" + k: v for k, v in self.sp.items()})
        return out

    UNIT_COLS = ["alive", "tile", "hp", "mv", "mmax", "att", "st", "xp", "side", "kind", "level", "prom", "icpt"]

    def state_from(self, rows_per_scene):
        """A state dict from Rust state records (one per scene): units, cities, rng, owners."""
        B, U, C, NP = self.b, self.u, self.c, self.np
        cols = {k: [] for k in self.UNIT_COLS}
        empty = [0, -1, 0, 0, 0, 0, 0, 0, -1, -1, 0, "", 0]
        for st in rows_per_scene:
            units = st["units"]
            for k in cols:
                cols[k].append([])
            for ui in range(U):
                r = units[ui] if ui < len(units) else empty
                r = list(r) + empty[len(r):]
                for j, k in enumerate(self.UNIT_COLS):
                    if k == "prom":
                        b = bits(r[j], NP) if isinstance(r[j], str) else []
                        cols[k][-1].append(b + [False] * (NP - len(b)))
                    else:
                        cols[k][-1].append(int(r[j]))
        s = {}
        for k, v in cols.items():
            if k in ("alive", "prom"):
                s["u_" + k] = mx.array(v).astype(mx.bool_)
            else:
                s["u_" + k] = mx.array(v, dtype=mx.int32)
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
        s["n_slots"] = mx.array([max(len(d["scene"]["profiles"]), len(d["initial"]["units"])) for d in self.dumps], dtype=mx.int32)
        s["done"] = mx.zeros((self.b,), dtype=mx.bool_)
        # The movement-table set of each unit (bits over its moving promotions held).
        s["u_mset"] = mx.zeros((self.b, self.u), dtype=mx.int32)
        return s

    def profile_state(self):
        return {"p_" + k: v for k, v in self.p.items()}


def take_scenes(batch_arrays, idx):
    """Replicate scenes: every array's first axis indexed by `idx` (for the throughput batches)."""
    return {k: mx.take(v, idx, axis=0) for k, v in batch_arrays.items()}
