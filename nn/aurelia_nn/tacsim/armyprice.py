"""Twin package 1: the assault judge with its deadline, the reinforcements of assault-reinforce, and the play-on past the loss cap.

Rust: `families::judge` for the seven assault families, `assault::land_reinforcements`, `ARMY_PLAY_ON_CAP=1`.

The model (written before the code):

- A scene has a loss cap (assault-heal, promotion-choice: half the army; hill-archers, city-fire, planned-assault: one unit;
  assault-peace, assault-reinforce: none) and a deadline (the "at any cost" families: N for the peace, N - 1 for the
  reinforcements; the others: none). A play ends at its round limit, at the capture, or (without play-on) when the cap breaks.
- The verdict after our script of round `rel` (the judge of `Game.judge`):
    1. the cap is broken and play-on is off          -> loss;
    2. the city is ours (first seen at round T)      -> win only if the cap never broke and T <= deadline, else loss;
    3. the round limit, or no military unit left     -> loss;
    4. otherwise the scene goes on.
  With play-on (`ARMY_PLAY_ON_CAP=1`) rule 1 is off: a broken cap is only remembered, so the play continues to the
  capture or the limit, and the row keeps the capture's round and the units lost. The verdict of a price form is then a
  function of the row: "careful" is the family's verdict as it stands (a broken cap is a loss either way), "any cost" is
  `0 <= taken <= D` with losses free (`row_verdict`).
- The reinforcements (assault-reinforce): at the enemy's turn of round `deadline` (the first enemy turn with
  `rel >= deadline`), once, the kinds of the scene's list are spawned (the list is consumed whether or not the city is
  still the enemy's) when the city is still the enemy's: the candidate tiles are the city's tile and its six neighbours
  that a unit of the kind may stand on and that hold no military unit (a neighbour with another city is out); they are
  taken in the order: the city tile first, then the farthest from our nearest piece first, then the lowest tile; one kind
  to each tile in that order, the rest (more kinds than tiles) never land. The new unit has full moves, no experience, no
  stance, its profile is the scene's spawn row of the kind for the enemy. The scene rule then plays it like any piece of the
  enemy's seat.
"""

import struct

import mlx.core as mx

from .sim import g1, gn, I32

W, L, N = 1, 0, -1
BIG = 1 << 20
ANY_COST = ("assault-peace", "assault-reinforce")
HALF_ARMY = ("assault-heal", "promotion-choice")
ONE_UNIT = ("hill-archers", "city-fire", "planned-assault")
ASSAULT = HALF_ARMY + ONE_UNIT + ANY_COST
# The play's rows the result keeps: the capture's round (-1: never), the units lost at the end, whether the cap broke.
ROW_KEYS = ("taken", "lost", "cap_broken")


class Price:
    """The package's per-row tensors and counters (`Game.pr`)."""

    def __init__(self, game, plays):
        idx = game.idx
        take = lambda v, dt=I32: mx.take(mx.array(v, dtype=dt), idx)
        scenes = [p[0] for p in plays]
        fam = [x["play"].split("/")[0] for x in scenes]
        self.is_assault = take([f in ASSAULT for f in fam], mx.bool_)
        self.is_reinforce = take([f == "assault-reinforce" for f in fam], mx.bool_)
        # The loss cap in units (the most that may be lost; `lost > cap` breaks it) and the deadline (BIG: none).
        self.cap = take([BIG if f in ANY_COST else (-1 if f in HALF_ARMY else 1) for f in fam])
        self.half = take([f in HALF_ARMY for f in fam], mx.bool_)
        self.deadline = take([x["orders"]["deadline"] if x["orders"].get("deadline") is not None else BIG for x in scenes])
        self.play_on = False
        self.cap_broken = mx.zeros((game.B,), dtype=mx.bool_)
        self.taken_round = mx.full((game.B,), -1, dtype=I32)
        self.lost_last = mx.zeros((game.B,), dtype=I32)
        self.landed = mx.zeros((game.B,), dtype=mx.bool_)
        rows = [list(x["scene"].get("reinforce", [])) for x in scenes]
        self.n_kinds = max([len(r) for r in rows] + [1])
        self.spawn_row = mx.take(mx.array([(r + [-1] * self.n_kinds)[: self.n_kinds] for r in rows], dtype=I32), idx, axis=0)


def attach(game, plays):
    return Price(game, plays)


def assault_verdict(game, v, rel, taken, lost, mil, limit):
    """The verdict of the seven assault families after round `rel` (the rule in the module's model); `v` the other
    families' verdicts, kept where the row is not an assault scene."""
    pr = game.pr
    n = game.n_mine
    cap = mx.where(pr.half, n // 2, pr.cap)
    pr.cap_broken = pr.cap_broken | (pr.is_assault & (lost > cap))
    live = game.verdict < 0
    first = taken & (pr.taken_round < 0) & live
    pr.taken_round = mx.where(first, rel, pr.taken_round)
    pr.lost_last = mx.where(live, lost, pr.lost_last)
    in_time = ~pr.cap_broken & (pr.taken_round <= pr.deadline)
    stop = pr.cap_broken & ~pr.play_on
    a = mx.where(stop, L, mx.where(taken, mx.where(in_time, W, L), mx.where(limit | ~mil, L, N)))
    return mx.where(pr.is_assault, a, v)


def row(game, b):
    """The kept row of play `b` (the first copy's): the capture's round, the units lost, whether the cap broke."""
    pr = game.pr
    return {"taken": int(pr.taken_round[b].item()), "lost": int(pr.lost_last[b].item()), "cap_broken": bool(pr.cap_broken[b].item())}


def row_verdict(taken, lost, cap_broken, win, form, deadline):
    """A play's verdict under a price form from its row: "careful" the family's own verdict (`win`), "anycost" the capture
    by `deadline` whatever was lost. The same function reads a Rust row's measures and a twin row."""
    if form == "careful":
        return bool(win)
    return 0 <= taken <= deadline


# ------------------------------------------------------------------------------------------ the price forms

def _f32(x):
    return struct.unpack("f", struct.pack("f", x))[0]


def price_set(form, family, units, rounds, deadline):
    """`army_net::price_set`: the stake [worth / 8, time pressure, 1] a form sets for a scene of `family` (`units` ours,
    `rounds` its limit, `deadline` its own or None). "careful": the loss cap (half the army, one unit, or one where the
    family has no cap), pressure 1 / rounds. "anycost[:D]": the whole army, pressure 1 / D (no D: the deadline, else the limit)."""
    form = form.strip()
    if form == "careful":
        worth = units // 2 if family in HALF_ARMY else 1
        return [_f32(max(float(worth), 0.5) / 8.0), _f32(1.0 / max(rounds, 1)), 1.0]
    if form.startswith("anycost"):
        rest = form[len("anycost"):]
        d = int(rest[1:]) if rest.startswith(":") else (deadline if deadline is not None else rounds)
        return [_f32(max(float(units), 0.5) / 8.0), _f32(1.0 / max(d, 1)), 1.0]
    raise ValueError(f"not a price form: {form}")


def price_for(form, plays):
    """[P, 3] the stake of `form` for each play's scene."""
    rows = []
    for sc, _, _ in plays:
        o = sc["orders"]
        rows.append(price_set(form, sc["play"].split("/")[0], sc["scene"]["n_mine"], o["rounds"], o.get("deadline")))
    return mx.array(rows, dtype=mx.float32)


# ------------------------------------------------------------------------------------------ the reinforcements

def land_reinforcements(game, live):
    """`assault::land_reinforcements` at the start of the enemy's turn in every live assault-reinforce scene."""
    pr = game.pr
    due = live & pr.is_reinforce & ~pr.landed & (game.rel_now >= pr.deadline)
    if not bool(mx.any(due).item()):
        return
    pr.landed = pr.landed | due
    s, sim = game.s, game.sim
    tc = mx.minimum(game.target, s["c_owner"].shape[1] - 1)
    go = due & game.has_target & (g1(s["c_owner"], tc) == 1)
    if not bool(mx.any(go).item()):
        return
    city = g1(sim.cs["tile"], tc)
    cand = mx.concatenate([city[:, None], mx.take(sim.geo.nbr, mx.maximum(city, 0), axis=0)], axis=1)  # [B, 7]
    tiles = _landing_tiles(game, cand, city)
    sp0 = mx.maximum(pr.spawn_row[:, 0], 0)
    mmax = sim.tg(sim.z["sp_moves_land"], sp0)
    for k in range(pr.n_kinds):
        sp = pr.spawn_row[:, k]
        tile = tiles[:, k]
        put = go & (sp >= 0) & (tile >= 0)
        if not bool(mx.any(put).item()):
            continue
        sd = mx.ones((game.B,), dtype=I32)
        s, slot = sim.new_unit(game.s, sd, tile, put, mx.full((game.B,), 100, dtype=I32), mmax, sim.tg(sim.z["sp_kind"], mx.maximum(sp, 0)), profile_from=mx.where(put, sp, -1))
        # A spawned unit has all its moves (the new_unit of a capture has none).
        s = sim.set_unit(s, slot, put & (slot < s["u_alive"].shape[1]), mv=mmax)
        game.s = s
        game.counts["reinforcements landed"] += int(mx.sum(put).item())
    mx.eval(game.s)


def _landing_tiles(game, cand, city):
    """[B, K] the tiles the kinds land on, in the module's order (-1: no tile for that kind)."""
    s, sim, pr = game.s, game.sim, game.pr
    B, U = s["u_alive"].shape
    sp0 = mx.maximum(pr.spawn_row[:, 0], 0)
    did = sim.tg(sim.z["sp_dest_id"], sp0)
    ok_land = mx.take(sim.dest_pool.reshape(-1), (mx.maximum(did, 0) * sim.T)[:, None] + mx.maximum(cand, 0)) & (cand >= 0)
    mil, _ = sim.occupancy(s)
    no_unit = gn(mil, mx.maximum(cand, 0)) < 0
    other_city = (sim.city_on(s, cand) >= 0) & (cand != city[:, None])
    empty = ok_land & no_unit & ~other_city
    ours = s["u_alive"] & (s["u_side"] == 0) & (s["u_tile"] >= 0) & (mx.arange(U)[None, :] < game.n_mine[:, None])
    d = sim.geo.distance(mx.broadcast_to(cand[:, :, None], (B, cand.shape[1], U)), mx.broadcast_to(s["u_tile"][:, None, :], (B, cand.shape[1], U)))
    near = mx.min(mx.where(ours[:, None, :], d, 99), axis=2)
    away = (cand != city[:, None]).astype(I32)
    key = away * (1 << 24) + (100 - near) * (1 << 12) + mx.maximum(cand, 0)
    key = mx.where(empty, key, 1 << 30)
    order = mx.argsort(key, axis=1)[:, : pr.n_kinds]
    chosen = mx.take_along_axis(cand, order, axis=1)
    return mx.where(mx.take_along_axis(key, order, axis=1) < (1 << 30), chosen, -1)


def spawn_profiles(sim, s, scenes):
    """The profile of the enemy's reinforcements in a state rebuilt from Rust's record: the units past the scene's own
    pieces on the enemy's side are those the scene lists for landing (the only units these scenes create), all of one kind."""
    rows = [x["scene"].get("reinforce") or [] for x in scenes]
    if not any(rows):
        return s
    first = mx.array([r[0] if r else -1 for r in rows], dtype=I32)
    own = mx.array([len(x["scene"]["profiles"]) for x in scenes], dtype=I32)
    U = s["u_alive"].shape[1]
    for u in range(int(mx.min(own).item()), U):
        slot = mx.full(first.shape, u, dtype=I32)
        known = (first >= 0) & (own <= u) & g1(s["u_alive"], slot) & (g1(s["u_side"], slot) == 1)
        if bool(mx.any(known).item()):
            s = sim.put_spawn_profile(s, slot, known, first)
    return s


# ------------------------------------------------------------------------------------------ step 1.8: the price in the RL reward

# The six families of the paired price data (teach7p): the deadline D of the "any cost" form (`army-net/runs/teach7p-D.json`).
PRICE_D = {"planned-assault": 8, "assault-heal": 6, "siege": 7, "city-fire": 6, "hill-archers": 7, "promotion-choice": 9}
NATIVE, CAREFUL, ANYCOST = "native", "careful", "anycost"


class Entry:
    """One RL group: a scene under one price form. `form` native (the scene's own stake; the family's own verdict), careful (the
    form's stake; the family's verdict) or anycost (the stake of `anycost:D`; the reward 1 when the capture comes at a round <= D,
    losses free, so the play goes on past the loss cap). `line` names the (family, form) the group belongs to."""

    def __init__(self, play, form=NATIVE):
        self.play = play
        self.family = play[0]["play"].split("/")[0]
        self.form = form
        self.D = PRICE_D.get(self.family) if form == ANYCOST else None
        self.line = self.family if form == NATIVE else f"{self.family}@{form}"
        o = play[0]["orders"]
        if form == NATIVE:
            self.price = [float(x) for x in o["price"]]
        else:
            name = "careful" if form == CAREFUL else f"anycost:{self.D}"
            self.price = price_set(name, self.family, play[0]["scene"]["n_mine"], o["rounds"], o.get("deadline"))


def lines_of(families, price):
    """The RL lines of a family list: with `price`, each of the six price families gets `f@careful` and `f@anycost`."""
    out = []
    for f in families:
        if price and f in PRICE_D:
            out += [(f, CAREFUL), (f, ANYCOST)]
        else:
            out.append((f, NATIVE))
    return out


def line_name(family, form):
    return family if form == NATIVE else f"{family}@{form}"


def prepare_game(game, entries):
    """Put the entries' stakes and the per-row play-on into a Game built from `[e.play for e in entries]` (rows p + P k)."""
    import mlx.core as mx
    idx = game.idx
    game.price = mx.take(mx.array([e.price for e in entries], dtype=mx.float32), idx, axis=0)
    game.pr.play_on = mx.take(mx.array([e.form == ANYCOST for e in entries]), idx)


def rewards(game, entries, copies):
    """[P * copies] the reward of each row: anycost 1 when taken at a round in 0..D, else the family's verdict == win."""
    v = game.verdict.tolist()
    taken = game.pr.taken_round.tolist()
    P = len(entries)
    out = []
    for row in range(P * copies):
        e = entries[row % P]
        out.append(1.0 if (0 <= taken[row] <= e.D if e.form == ANYCOST else v[row] == 1) else 0.0)
    return out
