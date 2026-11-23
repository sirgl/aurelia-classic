"""The economy on the GPU (MLX), first subsystem: the yields of every map tile, exact to the Rust rules
(`kernel/src/derived/tile_yields.rs`, `plot_yields` + `finish_tile`).

    python -m aurelia_nn.tacsim.econ DUMP.jsonl      # parity against Rust, every tile of every round

Data-oriented: per tile ids (terrain kind, feature, resource, improvement, natural wonder) index the
rules' yield tables; per player a technology bit-mask gates the resource and the fresh-water
technology yields; neighbourhood terms (fresh water, adjacent mountains, twin improvements) are sums
over the six neighbours. One batch row is one round of one game (the whole map), all rounds of all
games at once.

The parity source is `AURELIA_ECON_DUMP=FILE ai-tournament ...` (`crates/qa/src/bench/econ_dump.rs`):
the tables once per game, then per round the dynamic planes and Rust's `tile_yields_for` of every tile.
"""

import argparse
import json
import time

import mlx.core as mx

I32 = mx.int32
NY = 6  # food, production, gold, science, culture, faith
CITY_MIN = [2, 1, 0, 0, 0, 0]  # `finish_tile`: the city centre's minimum (Classic)
LAKE_FOOD = 1
DIRS = [(1, 0), (1, -1), (0, -1), (-1, 0), (-1, 1), (0, 1)]


def load(path, limit_games=None):
    """(tables per game, rounds) from an econ dump."""
    games, rounds = {}, []
    with open(path) as f:
        for line in f:
            d = json.loads(line)
            if d["t"] == "tables":
                if limit_games and len(games) >= limit_games and d["game"] not in games:
                    continue
                games[d["game"]] = d
            elif d["t"] == "round" and d["game"] in games:
                rounds.append(d)
    return games, rounds


class Tables:
    """The yield tables of the rules as dense arrays (the same rules in every game of a dump)."""

    def __init__(self, t):
        self.ntk = len(t["terrain_kinds"])
        self.terrain = mx.array([k["yields"] for k in t["terrain_kinds"]] + [[0] * NY], dtype=I32)  # [NTK + 1, 6] (last: none)
        F = t["features"]
        self.feature = mx.array([f["yields"] for f in F] + [[0] * NY], dtype=I32)
        self.f_replace = mx.array([f["not_additive"] for f in F] + [False])
        self.f_fresh = mx.array([f["fresh_water"] for f in F] + [False])
        W = t["wonders"]
        self.wonder = mx.array([w["yields"] for w in W] + [[0] * NY], dtype=I32)
        self.w_fresh = mx.array([w["fresh_water"] for w in W] + [False])
        R, I = t["resources"], t["improvements"]
        self.resource = mx.array([r["yields"] for r in R] + [[0] * NY], dtype=I32)
        self.r_tech = mx.array([r["tech"] for r in R] + [-1], dtype=I32)
        improved = [[[0] * NY for _ in range(len(I) + 1)] for _ in range(len(R) + 1)]
        for ri, r in enumerate(R):
            for imp, y in r["improved"]:
                improved[ri][imp] = [a + b for a, b in zip(improved[ri][imp], y)]
        self.improved = mx.array(improved, dtype=I32)  # [NR + 1, NI + 1, 6]
        self.imp = mx.array([i["yields"] for i in I] + [[0] * NY], dtype=I32)
        self.imp_mountain = mx.array([i["mountain"] for i in I] + [[0] * NY], dtype=I32)
        self.imp_twin = mx.array([i["twin_culture"] for i in I] + [0], dtype=I32)
        E = max(1, max(len(i["fresh_water"]) for i in I))
        fw = [[[-2, 0, 0, 0]] * E for _ in range(len(I) + 1)]
        for ii, i in enumerate(I):
            for e, y in enumerate(i["fresh_water"]):
                fw[ii][e] = [y["tech"], y["kind"], y["amount"], y["fresh"]]
        self.fw = mx.array(fw, dtype=I32)  # [NI + 1, E, 4]: tech (-1 none, -2 no entry), channel, amount, condition
        self.nf, self.nw, self.nr, self.ni = len(F), len(W), len(R), len(I)


def neighbours(w, h):
    nb = []
    for i in range(w * h):
        q, r = i % w, i // w
        nb.append([(r + dr) * w + q + dq if 0 <= q + dq < w and 0 <= r + dr < h else -1 for dq, dr in DIRS])
    return mx.array(nb, dtype=I32)


def gather_rows(table, idx):
    """table[idx] with idx [B, T] (−1 → the table's last row: none)."""
    n = table.shape[0] - 1
    return mx.take(table, mx.where(idx < 0, n, idx), axis=0)


def plot_yields(tb, nb, st, finish=True):
    """[B, T, 6]: `finish_tile(plot_yields(...))` of every tile for its owner (the viewer on unowned
    land); `finish=False`: the plot yields before `finish_tile` (the city's tile effects come between). `st`: kind, mountain, water, lake, river, wonder (static, [B, T]); feature, resource,
    improvement, pillaged, owner, city ([B, T]); techs [B, P, NT] bool; viewer [B]."""
    B, T = st["kind"].shape
    nbm = nb if nb.ndim == 3 else mx.broadcast_to(nb[None], (B, T, 6))  # [T, 6] shared, or [B, T, 6] per row (local layouts)
    on = (nbm >= 0)

    def nbr(a):
        return mx.where(on, mx.take_along_axis(a, mx.maximum(nbm, 0).reshape(B, -1), axis=1).reshape(B, T, 6), 0)

    # Nature: the terrain, a feature (replacing or adding), a natural wonder (replacing), a lake.
    y = gather_rows(tb.terrain, st["kind"])
    f = st["feature"]
    fy = gather_rows(tb.feature, f)
    repl = mx.take(tb.f_replace, mx.where(f < 0, tb.nf, f))[..., None]
    y = mx.where(f[..., None] >= 0, mx.where(repl, fy, y + fy), y)
    w = st["wonder"]
    y = mx.where(w[..., None] >= 0, gather_rows(tb.wonder, w), y)
    y = y + mx.where(st["lake"][..., None], mx.array([LAKE_FOOD, 0, 0, 0, 0, 0], dtype=I32), 0)
    # The viewer's technologies: the tile's owner, else the dump's viewer.
    viewer = mx.where(st["owner"] >= 0, st["owner"], st["viewer"][:, None])  # [B, T]
    techs = st["techs"]  # [B, P, NT]
    NT = techs.shape[2]
    flat = techs.reshape(B, -1)

    def known(tech):
        """Does the viewer of each tile know `tech` [B, T, ...] (−1: no requirement)?"""
        idx = viewer.reshape(viewer.shape + (1,) * (tech.ndim - 2)) * NT + mx.maximum(tech, 0)
        got = mx.take_along_axis(flat, idx.reshape(B, -1), axis=1).reshape(tech.shape)
        return (tech < 0) | got

    # The resource, once the viewer knows its revealing technology.
    r = st["resource"]
    r_vis = mx.where((r >= 0) & known(mx.take(tb.r_tech, mx.where(r < 0, tb.nr, r))), r, -1)
    y = y + gather_rows(tb.resource, r_vis)
    # The improvement (nothing while pillaged): its yields, the improved resource, fresh water
    # technology yields, adjacent mountains, twins.
    i = mx.where(st["pillaged"], -1, st["improvement"])
    ii = mx.where(i < 0, tb.ni, i)
    has = (i >= 0)[..., None]
    iy = gather_rows(tb.imp, i)
    rr = mx.where(r_vis < 0, tb.nr, r_vis)
    iy = iy + mx.take(tb.improved.reshape(-1, NY), rr * (tb.ni + 1) + ii, axis=0)
    fresh_feature = mx.take(tb.f_fresh, mx.where(st["feature"] < 0, tb.nf, st["feature"]))
    fresh_wonder = mx.take(tb.w_fresh, mx.where(st["wonder"] < 0, tb.nw, st["wonder"]))
    near_fresh = mx.any(on & ((nbr(st["lake"].astype(I32)) > 0) | (nbr(fresh_feature.astype(I32)) > 0) | (nbr(fresh_wonder.astype(I32)) > 0)), axis=2)
    fresh = ~st["water"] & (st["river"] | near_fresh)
    fw = mx.take(tb.fw, ii, axis=0)  # [B, T, E, 4]
    cond = (fw[..., 3] == 0) | ((fw[..., 3] == 1) & fresh[..., None]) | ((fw[..., 3] == 2) & ~fresh[..., None])
    ok = (fw[..., 0] >= -1) & cond & known(fw[..., 0])
    amount = mx.where(ok, fw[..., 2], 0)
    chan = mx.arange(NY)[None, None, None, :] == fw[..., 1:2]
    iy = iy + mx.sum(mx.where(chan, amount[..., None], 0), axis=2)
    mountains = mx.sum(on & (nbr(st["mountain"].astype(I32)) > 0), axis=2)
    iy = iy + mountains[..., None] * gather_rows(tb.imp_mountain, i)
    twins = mx.sum(on & (nbr(i + 1) == (i + 1)[..., None]) & (i >= 0)[..., None], axis=2)
    twin = twins * mx.take(tb.imp_twin, ii)
    iy = iy + mx.where(mx.arange(NY)[None, None, :] == 4, twin[..., None], 0)
    y = y + mx.where(has, iy, 0)
    if not finish:
        return y
    # `finish_tile`: the city centre's minimum, then every channel at least 0.
    y = mx.where(st["city"][..., None], mx.maximum(y, mx.array(CITY_MIN, dtype=I32)), y)
    return mx.maximum(y, 0)


def batch(games, rounds):
    """The rounds as one batch (all of one map size)."""
    g0 = games[rounds[0]["game"]]
    static = {k: [] for k in ("kind", "mountain", "water", "lake", "river", "wonder")}
    dyn = {k: [] for k in ("feature", "resource", "improvement", "pillaged", "owner", "city")}
    techs, viewer, want = [], [], []
    P = max(len(r["techs"]) for r in rounds)
    NT = len(rounds[0]["techs"][0])
    for r in rounds:
        g = games[r["game"]]
        for k in static:
            static[k].append(g["tiles"][k])
        for k in dyn:
            dyn[k].append(r[k])
        rows = [[c == "1" for c in t] for t in r["techs"]] + [[False] * NT] * (P - len(r["techs"]))
        techs.append(rows)
        viewer.append(r["viewer"])
        want.append(r["yields"])
    st = {}
    for k, v in list(static.items()) + list(dyn.items()):
        st[k] = mx.array(v) if k in ("mountain", "water", "lake", "river", "pillaged", "city") else mx.array(v, dtype=I32)
    st["techs"] = mx.array(techs)
    st["viewer"] = mx.array(viewer, dtype=I32)
    return st, mx.array(want, dtype=I32), (g0["map_w"], g0["map_h"])


def run(path, limit_games=None):
    t0 = time.time()
    games, rounds = load(path, limit_games)
    tb = Tables(next(iter(games.values())))
    by_size = {}
    for r in rounds:
        g = games[r["game"]]
        by_size.setdefault((g["map_w"], g["map_h"]), []).append(r)
    t_load = time.time() - t0
    tiles = ok = rounds_ok = 0
    bad = []
    t_gpu = 0.0
    for (w, h), rs in by_size.items():
        nb = neighbours(w, h)
        for k in range(0, len(rs), 64):
            chunk = rs[k: k + 64]
            st, want, _ = batch(games, chunk)
            t1 = time.time()
            got = plot_yields(tb, nb, st)
            mx.eval(got)
            t_gpu += time.time() - t1
            eq = mx.all(got == want, axis=2)
            tiles += eq.size
            ok += int(mx.sum(eq).item())
            row_ok = mx.all(eq, axis=1).tolist()
            rounds_ok += sum(row_ok)
            if not all(row_ok) and len(bad) < 6:
                e = eq.tolist()
                gl, wl = got.tolist(), want.tolist()
                for b, rok in enumerate(row_ok):
                    if not rok and len(bad) < 6:
                        t = e[b].index(False)
                        bad.append({"game": chunk[b]["game"], "round": chunk[b]["round"], "tile": t, "rust": wl[b][t], "mlx": gl[b][t],
                                    "feature": chunk[b]["feature"][t], "resource": chunk[b]["resource"][t], "improvement": chunk[b]["improvement"][t],
                                    "owner": chunk[b]["owner"][t], "kind": games[chunk[b]["game"]]["tiles"]["kind"][t]})
    return {"games": len(games), "rounds": len(rounds), "rounds_ok": rounds_ok, "tiles": tiles, "tiles_ok": ok, "examples": bad,
            "load_s": round(t_load, 2), "gpu_s": round(t_gpu, 3)}


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("dump")
    ap.add_argument("--games", type=int)
    o = ap.parse_args(argv)
    rep = run(o.dump, o.games)
    print(f"tile yields: {rep['rounds_ok']}/{rep['rounds']} rounds of {rep['games']} games equal on every tile "
          f"({rep['tiles_ok']}/{rep['tiles']} tiles); load {rep['load_s']} s, GPU {rep['gpu_s']} s")
    for e in rep["examples"]:
        print("  ex:", e)


if __name__ == "__main__":
    main()
