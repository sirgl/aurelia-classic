"""The economy on the GPU (MLX): a player's resource ledger, exact to the Rust rules
(`derived::resources::resource_ledger`), and its accounting inside the turn.

    python -m aurelia_nn.tacsim.ledger DUMP.jsonl|DIR    # parity: total and used of every resource of every end of turn

    total  copies on the tiles of the player's cities it knows (reveal technology) and has connected
           (the city tile, or one of the resource's improvements, not pillaged): the deposit's amount
           × (100 + ResourceQuantityPercent of the resource) %, a strategic one × (100 +
           StrategicQuantityPercent) % (truncated per tile), a luxury + the city's ExtraLuxuryCopies;
           + ResourceCopies of the player; + the inputs (Feitoria copies, imports by deals, allied
           city-states)
    used   units alive × their requirements + buildings standing × what they consume; + the inputs
           (exports, a city-state's copies its ally holds)

One batch row is one turn line (the whole map); the tiles' copies are one scatter-add into
[turn, resource]. Inside the turn (`Running`): a unit trained or a building completed takes its copies
at once, and an order whose resources are short is dropped (`production_resources`): the cities
later in the turn's order see it.

Inputs from the subsystems not in MLX (the dump's `resources` lines): Feitoria copies, deals,
city-state alliances.
"""

import argparse
import json
import time

import mlx.core as mx

from . import city as C
from . import player as P
from . import rdebug as R

I32 = mx.int32
RQP = "ResourceQuantityPercent"
SQP = P.key("StrategicQuantityPercent")


class Tables:
    """The resource tables of the rules (once per model)."""

    def __init__(self, er):
        self.er = er
        res = er["resources"]
        self.nr = len(res)
        self.nu, self.nb = len(er["units"]), len(er["buildings"])
        cls = [R.tag(R.arg(R.parse(r["class"]))) if R.arg(R.parse(r["class"])) is not None else None for r in res]
        self.strategic = [c == "Strategic" for c in cls]
        self.luxury = [c == "Luxury" for c in cls]
        self.r_tech = [-1 if r.get("tech") is None else r["tech"] for r in res]
        ni = 1 + max([i for r in res for i in r.get("improvements", [])] + [0])
        self.ni = ni
        conn = [[False] * (ni + 1) for _ in range(self.nr + 1)]
        for ri, r in enumerate(res):
            for i in r.get("improvements", []):
                conn[ri][i] = True
        self.conn = mx.array(conn)  # [NR + 1, NI + 1] (last row/column: none)
        req = [[0] * self.nr for _ in range(self.nu)]
        for k, u in enumerate(er["units"]):
            for r, n in u["resources"]:
                req[k][r] += n
        cons = [[0] * self.nr for _ in range(self.nb)]
        for k, b in enumerate(er["buildings"]):
            for r, n in b["consumes"]:
                cons[k][r] += n
        self.req, self.cons = req, cons
        self.req_m, self.cons_m = mx.array(req, dtype=I32), mx.array(cons, dtype=I32)
        self.rqp_keys = [P.key(f"{RQP}(ResourceId({r}))") for r in range(self.nr)]
        self.copies_keys = [P.key(f"ResourceCopies(ResourceId({r}))") for r in range(self.nr)]

    def needs(self, item):
        """[(resource, copies)] an item index takes (units first, then buildings; a project nothing)."""
        if item < 0:
            return []
        if item < self.nu:
            row = self.req[item]
        else:
            row = self.cons[item - self.nu]
        return [(r, n) for r, n in enumerate(row) if n]


def tables(model):
    if not hasattr(model, "_res_tables"):
        model._res_tables = Tables(model.er)
    return model._res_tables


def base(model, prow, game, turns, side="pre", extra_lux=None):
    """Per turn line: (total[NR], used[NR]) of the player's ledger on the side's world, in MLX.
    `extra_lux`: {(turn index, city): ExtraLuxuryCopies} (the city table's stat)."""
    tb = tables(model)
    B = len(turns)
    if B == 0:
        return []
    T = game["tables"]["map_w"] * game["tables"]["map_h"]
    planes = [t[side + "_planes"] for t in turns]
    resource = mx.array([p[1] for p in planes], dtype=I32)
    improvement = mx.array([p[2] for p in planes], dtype=I32)
    pillaged = mx.array([p[3] for p in planes]) > 0
    ocity = mx.array([p[5] for p in planes], dtype=I32)
    cityh = mx.array([p[6] for p in planes], dtype=I32)
    amount = mx.array([p[7] if len(p) > 7 else [0] * T for p in planes], dtype=I32)
    states = [t[side]["state"] for t in turns]
    H = 1 + max([c["h"] for s in states for c in s["cities"]] + [0])
    hm = [[False] * (H + 1) for _ in range(B)]
    elx = [[0] * (H + 1) for _ in range(B)]
    for b, s in enumerate(states):
        for c in s["cities"]:
            hm[b][c["h"]] = True
            if extra_lux is not None:
                elx[b][c["h"]] = extra_lux.get((b, c["h"]), 0)
    hm, elx = mx.array(hm), mx.array(elx, dtype=I32)
    oc = mx.where(ocity < 0, H, ocity)
    owned = mx.take_along_axis(hm, oc, axis=1)
    techs = mx.array([[ch == "1" for ch in s["techs"]] for s in states])
    r_tech = mx.array(tb.r_tech + [-1], dtype=I32)
    rr = mx.where(resource < 0, tb.nr, resource)
    need = mx.take(r_tech, rr)
    revealed = (need < 0) | mx.take_along_axis(techs, mx.maximum(need, 0), axis=1)
    working = mx.where(pillaged | (improvement < 0), tb.ni, mx.minimum(improvement, tb.ni))
    improved = mx.take(tb.conn.reshape(-1), rr * (tb.ni + 1) + working)
    city_tile = (cityh >= 0) & (cityh == ocity)
    connected = owned & (resource >= 0) & revealed & (city_tile | improved)
    # The player's quantity stats and copies.
    rqp, sqp, copies_stat = [], [], []
    for t in turns:
        stats, _ = P.stats_and_costs(prow, t, side)
        rqp.append([stats.get(k, 0) for k in tb.rqp_keys] + [0])
        sqp.append(stats.get(SQP, 0))
        copies_stat.append([stats.get(k, 0) for k in tb.copies_keys])
    rqp_m = mx.array(rqp, dtype=I32)
    pct = mx.maximum(100 + mx.take_along_axis(rqp_m, rr, axis=1), 0)
    copies = amount * pct // 100
    strat = mx.array(tb.strategic + [False])
    lux = mx.array(tb.luxury + [False])
    is_s = mx.take(strat, rr)
    copies = mx.where(is_s, copies * mx.maximum(100 + mx.array(sqp, dtype=I32)[:, None], 0) // 100, copies)
    copies = copies + mx.where(mx.take(lux, rr), mx.maximum(mx.take_along_axis(elx, oc, axis=1), 0), 0)
    copies = mx.where(connected, copies, 0)
    # Scatter-add into [turn, resource] (the last column: no resource).
    flat = (mx.arange(B)[:, None] * (tb.nr + 1) + rr).reshape(-1)
    total = mx.zeros((B * (tb.nr + 1),), dtype=I32).at[flat].add(copies.reshape(-1)).reshape(B, tb.nr + 1)[:, :tb.nr]
    total = total + mx.array(copies_stat, dtype=I32)
    units = [[0] * tb.nu for _ in range(B)]
    blds = [[0] * tb.nb for _ in range(B)]
    extra_t = [[0] * tb.nr for _ in range(B)]
    extra_u = [[0] * tb.nr for _ in range(B)]
    for b, s in enumerate(states):
        for k, n in s["units"]:
            units[b][k] += n
        for c in s["cities"]:
            for x in c["buildings"]:
                blds[b][x] += 1
        for r, _, _, untradeable, imports, from_cs, used_extra in s.get("resources", []):
            extra_t[b][r] += untradeable + imports + from_cs
            extra_u[b][r] += used_extra
    used = (mx.sum(mx.array(units, dtype=I32)[:, :, None] * tb.req_m[None], axis=1) + mx.sum(mx.array(blds, dtype=I32)[:, :, None] * tb.cons_m[None], axis=1)
            + mx.array(extra_u, dtype=I32))
    total = total + mx.array(extra_t, dtype=I32)
    mx.eval(total, used)
    return list(zip(total.tolist(), used.tolist()))


class Running:
    """A turn's ledger as its cities resolve in order: available = total − used, updated by every
    unit trained and building completed (its consumed copies; a building's ResourceCopies)."""

    def __init__(self, tb, total, used, copies_of=None):
        self.tb = tb
        self.total, self.used = list(total), list(used)
        self.copies_of = copies_of or {}

    def blocked(self, item):
        return any(self.total[r] - self.used[r] < n for r, n in self.tb.needs(item))

    def take(self, item):
        for r, n in self.tb.needs(item):
            self.used[r] += n
        if item >= self.tb.nu:
            for r, n in self.copies_of.get(item - self.tb.nu, ()):
                self.total[r] += n

    def copy(self):
        r = Running(self.tb, self.total, self.used, self.copies_of)
        if hasattr(self, "born"):
            r.born = dict(self.born)  # the great people born so far (`great.py`)
        return r


def building_copies(model, prow):
    """{building: [(resource, copies)]}: ResourceCopies rows of owner-wide buildings (the player table)."""
    if not hasattr(model, "_bld_copies"):
        tb = tables(model)
        out = {}
        inv = {k: r for r, k in enumerate(tb.copies_keys)}
        for row in prow.static:
            if row["src"][0] != "wide":
                continue
            pm = P._parsed(row)
            if pm and pm[0] == "stat" and pm[1] in inv and all(c[0] == C.C_ALWAYS for c in row["c"]):
                out.setdefault(row["src"][1], []).append((inv[pm[1]], pm[2]))
        model._bld_copies = out
    return model._bld_copies


def compare(tb, turns, got, side="pre"):
    """Per turn line: MLX's (total, used) against Rust's ledger, and the queue's blocked orders."""
    s = {"ledgers": 0, "ledgers_ok": 0, "orders": 0, "orders_ok": 0}
    ex = []
    for t, (total, used) in zip(turns, got):
        st = t[side]["state"]
        if "resources" not in st:
            continue
        want_t, want_u = [0] * tb.nr, [0] * tb.nr
        for r, tot, us, *_ in st["resources"]:
            want_t[r], want_u[r] = tot, us
        s["ledgers"] += 1
        ok = want_t == total and want_u == used
        s["ledgers_ok"] += int(ok)
        if not ok and len(ex) < 6:
            ex.append({"round": t["round"], "player": t["player"], "bad": [[r, want_t[r], total[r], want_u[r], used[r]] for r in range(tb.nr)
                                                                            if (want_t[r], want_u[r]) != (total[r], used[r])]})
        run = Running(tb, total, used)
        for c in st["cities"]:
            for q, b in zip(c["queue"], c.get("blocked", [])):
                item = C.item_of(q, tb.nu)
                s["orders"] += 1
                s["orders_ok"] += int(run.blocked(item) == b)
    return s, ex


def run(path, limit_games=None, chunk=64, show=6):
    tot, examples = {}, []
    t_run = 0.0
    model = prow = None
    n_games = 0
    for g, game in C.each_game(path, limit_games):
        n_games += 1
        if model is None:
            model = C.Model(game["tables"], game["rules"])
            prow = P.Rows(model.rows, model.er)
        turns = P.prepare(game, [t for t in game["turns"]])
        for k in range(0, len(turns), chunk):
            part = turns[k:k + chunk]
            t1 = time.time()
            elx = extra_lux_of(model, game, part, "pre")
            got = base(model, prow, game, part, "pre", elx)
            t_run += time.time() - t1
            s, ex = compare(tables(model), part, got)
            for key_, v in s.items():
                tot[key_] = tot.get(key_, 0) + v
            for e in ex:
                if len(examples) < show:
                    e["game"] = g
                    examples.append(e)
    return {"games": n_games, **tot, "examples": examples, "run_s": round(t_run, 2)}


def extra_lux_of(model, game, turns, side):
    """{(turn index, city): ExtraLuxuryCopies} from the city table (cities with a luxury only matter)."""
    b = C.Batch(model, game, turns, side=side)
    if b.n == 0:
        return {}
    got = C.evaluate(model, game, b)
    v = got["extra_lux"].tolist()
    return {(ti, c["h"]): v[i] for i, (ti, c) in enumerate(b.cities) if v[i]}


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("dump")
    ap.add_argument("--games", type=int)
    ap.add_argument("--show", type=int, default=6)
    o = ap.parse_args(argv)
    rep = run(o.dump, o.games, show=o.show)
    print(f"resource ledger: {rep.get('ledgers_ok', 0)}/{rep.get('ledgers', 0)} end-turns equal on every resource (total, used); "
          f"orders blocked by resources {rep.get('orders_ok', 0)}/{rep.get('orders', 0)} as Rust; run {rep['run_s']} s")
    for e in rep["examples"]:
        print("  ex:", json.dumps(e))


if __name__ == "__main__":
    main()
