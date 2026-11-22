"""The float twin of `net2` (crates/classic-rules/src/ai/net2, design §3.2).

The same layers as the integer net, read from its exported tables (`fixtures/weights.json`,
`net2::trace::export`). Two arithmetics:

* `FLOAT` -- the twin for training: real division, the clamp as `clip(0, 1000)`, the softmax by `exp`;
* `EXACT` -- the integer net's own arithmetic (division toward zero, the `e^(-k/8)` table), a check of
  the port's logic: it must give the integer net's numbers exactly.

Every value is in thousandths (1000 = 1.0), as in the integer net. A turn is the `in` part of a
trace line (`net2::trace::line`); `forward` returns the same keys as its `out` part.

`teacher=out` evaluates each neuron on the *recorded* values of the layers it reads (the parity of
§3.2 and gate G6: every neuron within 2 thousandths); without it the twin runs free on its own values.
"""

from __future__ import annotations

import json
import math
from pathlib import Path

ONE = 1000
FIXTURES = Path(__file__).resolve().parent / "fixtures"

# `e^(-k/8)` in millionths for k = 0..64 (`strategy_net/layers.rs`).
EXP = [
    1000000, 882497, 778801, 687289, 606531, 535261, 472367, 416862, 367879, 324652, 286505, 252840,
    223130, 196912, 173774, 153355, 135335, 119433, 105399, 93014, 82085, 72440, 63928, 56416, 49787,
    43937, 38774, 34218, 30197, 26649, 23518, 20754, 18316, 16163, 14264, 12588, 11109, 9804, 8652,
    7635, 6738, 5946, 5248, 4631, 4087, 3607, 3183, 2809, 2479, 2187, 1930, 1704, 1503, 1327, 1171,
    1033, 912, 805, 710, 627, 553, 488, 431, 380, 335,
]

# `sharp::w`'s temperatures of the style, the aim and the posture softmax.
TEMPERATURES = (600, 600, 1000)
KIND_ORDER = ("prey", "retake", "spoiler", "duty", "border")
BUDGETS = {"prey": 25, "retake": 20, "spoiler": 30, "duty": 15, "border": 15, "overseas": 30}
SPENT_ROUNDS = 20
PREPARE_ROUNDS = 30


class Arith:
    """The arithmetic of a forward pass."""

    def __init__(self, exact: bool):
        self.exact = exact

    def div(self, a, b):
        if not self.exact:
            return a / b
        a, b = int(a), int(b)
        q = abs(a) // abs(b)
        return q if (a >= 0) == (b >= 0) else -q

    @staticmethod
    def unit(x):
        return min(max(x, 0), ONE)

    def exp_neg(self, x):
        """`e^(x/1000)` in millionths for x <= 0."""
        if not self.exact:
            return 1_000_000 * math.exp(min(x, 0) / ONE)
        d = max(-int(x), 0)
        step = d // 125
        if step >= 64:
            return 0
        rest = d % 125
        a, b = EXP[step], EXP[step + 1]
        return a - (a - b) * rest // 125

    def softmax(self, logits):
        top = max(logits)
        weights = [self.exp_neg(l - top) for l in logits]
        total = max(sum(weights), 1)
        if not self.exact:
            return [w * ONE / total for w in weights]
        out = [w * ONE // total for w in weights]
        rest = ONE - sum(out)
        best = max(range(len(out)), key=lambda i: (out[i], -i))
        out[best] += rest
        return out


EXACT = Arith(True)
FLOAT = Arith(False)


class Weights:
    """The integer net's tables (`net2::trace::export`)."""

    def __init__(self, data: dict):
        self.neurons = data["neurons"]
        self.seat = data["seat"]
        self.rival = data["rival"]
        self.tilt = data["tilt"]
        self.gates = data["gates"]
        self.names = data["names"]
        self.q = {name: i for i, name in enumerate(self.rival)}
        self.r = {name: i for i, name in enumerate(self.names["r"])}
        self.a = {name: i for i, name in enumerate(self.names["a"])}

    @classmethod
    def load(cls, path: Path = FIXTURES / "weights.json") -> "Weights":
        return cls(json.loads(Path(path).read_text()))


# ---- one neuron ----


def _value(frame, src):
    layer, i = src
    return frame[layer][i]


def _shaped(term, frame, ar):
    x = _value(frame, term["src"])
    shape = term["shape"]
    kind = shape[0]
    if kind == "plain":
        return x
    if kind == "not":
        return ONE - x
    if kind == "above":
        return max(0, x - shape[1])
    if kind == "centered":
        return x - shape[1]
    return ar.div(x * _value(frame, shape[1]), ONE)  # times


def fire(neuron, frame, ar):
    if neuron["join"] == "max":
        best = 0
        for term in neuron["terms"]:
            best = max(best, ar.div(term["w"] * _shaped(term, frame, ar), ONE))
        return ar.unit(best)
    total = sum(term["w"] * _shaped(term, frame, ar) for term in neuron["terms"])
    out = neuron["bias"] + ar.div(total, ONE)
    return ar.unit(out) if neuron["join"] == "unit" else out


# ---- the layers ----


def seat_concepts(W, turn, ar):
    frame = {"F": turn["f"], "K": turn["k"], "G": turn["g"]}
    return [fire(W.neurons[name], frame, ar) for name in W.seat]


def eases(W, turn, ar):
    """The ease of every target (`weights::EASE`)."""
    return [fire(W.neurons["ease"], {"T": target["t"]}, ar) for target in turn.get("targets", [])]


def with_soft(W, turn, ease):
    """The turn with each rival's `soft` (R27) set to the largest ease of its targets (`levers::soft_of`)."""
    soft = W.r["soft"]
    rivals = []
    for rival in turn["rivals"]:
        best = max([e for t, e in zip(turn.get("targets", []), ease) if t["rival"] == rival["id"]], default=0)
        r = list(rival["r"])
        r[soft] = best
        rivals.append(dict(rival, r=r))
    return dict(turn, rivals=rivals)


def rival_concepts(W, turn, seat, ar, recorded=None):
    """Each rival's concepts in order; a concept reads the earlier ones (the recorded ones with `recorded`)."""
    out = []
    for at, rival in enumerate(turn["rivals"]):
        q = []
        for i, name in enumerate(W.rival):
            reads = recorded[at] if recorded is not None else q + [0] * (len(W.rival) - i)
            frame = {"K": turn["k"], "S": seat, "R": rival["r"], "Q": reads}
            q.append(fire(W.neurons[name], frame, ar))
        out.append(q)
    return out


def war_kind(W, r, q):
    best = 0
    for i, kind in enumerate(KIND_ORDER):
        if q[W.q[kind]] > q[W.q[KIND_ORDER[best]]]:
            best = i
    kind = KIND_ORDER[best]
    if kind != "retake" and r[W.r["land"]] == 0:
        return "overseas"
    return kind


def paced(rounds, pace):
    return max(1, rounds * max(pace, 1) // 100)


def _judge(W, order, rivals, qs, war_gain, rnd, pace):
    found = [(r, q) for r, q in zip(rivals, qs) if r["id"] == order["rival"]]
    if not found:
        return "dropped", None
    r, q = found[0][0]["r"], found[0][1]
    value = q[W.q["war_value"]]
    at_war = r[W.r["at_war"]] > 0
    war_since = order.get("war_since")
    if war_since is None and at_war:
        war_since = rnd
    elif war_since is not None and not at_war:
        return "dropped", None
    if war_since is None:
        in_budget = rnd <= order["since"] + paced(PREPARE_ROUNDS, pace)
    else:
        in_budget = rnd <= war_since + order["budget"]
    retaking = r[W.r["retake_case"]] > 0
    pays = r[W.r["captured_recent"]] > 0 or retaking
    kept = dict(order, value=value, war_since=war_since)
    if value >= W.gates["war_keep"] and (in_budget or pays):
        return "keep", kept
    if in_budget and r[W.r["operating"]] > 0:
        return "keep", kept
    if not in_budget and war_gain <= ONE // 2 and not retaking:
        return "spent", None
    return "dropped", None


def aimed(order, turn, ease):
    """The order's aim: its rival's easiest target, the first on a tie; none for a retake (`levers::aimed`)."""
    if order is None:
        return None
    best = None
    if order["kind"] != "retake":
        for target, e in zip(turn.get("targets", []), ease):
            if target["rival"] == order["rival"] and (best is None or e > best[1]):
                best = (target["city"], e)
    return dict(order, aim=best[0] if best else None)


def war_plan(W, turn, qs, ease):
    """The war order and the spent ones after the turn (`levers::war_plan`, `levers::aimed`)."""
    core = _war_plan(W, turn, qs)
    return {"war": aimed(core["war"], turn, ease), "spent": core["spent"]}


def _war_plan(W, turn, qs):
    rnd, pace, held = turn["round"], turn["pace"], turn["held"]
    spent = [s for s in held["spent"] if s[1] > rnd]
    order = held["war"]
    if order is not None:
        verdict, kept = _judge(W, order, turn["rivals"], qs, turn["g"][W.names["g"].index("war_gain")], rnd, pace)
        if verdict == "keep":
            return {"war": kept, "spent": spent}
        if verdict == "spent":
            spent.append([order["rival"], rnd + paced(SPENT_ROUNDS, pace)])
    best = None
    for rival, q in zip(turn["rivals"], qs):
        value = q[W.q["war_value"]]
        if value < W.gates["war_start"] or any(s[0] == rival["id"] for s in spent):
            continue
        if best is None or value > best[1]:
            best = (rival, value, war_kind(W, rival["r"], q))
    if best is None:
        return {"war": None, "spent": spent}
    kind = best[2]
    war_since = rnd if best[0]["r"][W.r["at_war"]] > 0 else None
    order = {"rival": best[0]["id"], "kind": kind, "since": rnd, "budget": paced(BUDGETS[kind], pace), "war_since": war_since, "value": best[1]}
    return {"war": order, "spent": spent}


def aggregates(W, turn, qs, core):
    out = [0] * len(W.names["a"])
    out[W.a["one"]] = ONE
    for q in qs:
        out[W.a["max_prey"]] = max(out[W.a["max_prey"]], q[W.q["prey"]])
        out[W.a["max_menace"]] = max(out[W.a["max_menace"]], q[W.q["menace"]])
    order = core["war"]
    if order is not None:
        at_war = any(r["id"] == order["rival"] and r["r"][W.r["at_war"]] > 0 for r in turn["rivals"])
        out[W.a["order_at_war" if at_war else "order_at_peace"]] = order["value"]
        out[W.a["order_value"]] = order["value"]
    elif turn["f"][W.names["at_war_feature"]] == 0:
        out[W.a["no_order"]] = ONE
    return out


def tilt(W, seat, aggs, ar):
    out = {"style": [0] * 3, "aim": [0] * 4, "posture": [0] * 4}
    frame = {"S": seat, "A": aggs}
    for row in W.tilt:
        term = row["term"]
        out[row["table"]][row["at"]] += ar.div(term["w"] * _shaped(term, frame, ar), ONE)
    return out


def shares(turn, tilts, ar):
    """The intent's 11 shares: the carried logits plus the tilt, tempered for `sharp`, softmax."""
    out = []
    for table, temperature in zip(("style", "aim", "posture"), TEMPERATURES):
        logits = [l + t for l, t in zip(turn["logits"][table], tilts[table])]
        if turn["tempered"]:
            logits = [ar.div(l * ONE, temperature) for l in logits]
        out.extend(ar.softmax(logits))
    return out


def _city_row(W, city, rivals, qs):
    by_id = {r["id"]: q for r, q in zip(rivals, qs)}
    owner = city["stack_owner"]
    menace = by_id[owner][W.q["menace"]] if owner in by_id else 0
    distrust = max([by_id[p][W.q["distrust"]] for p in city["near"] if p in by_id], default=0)
    return [menace, city["danger"], distrust, city["young"]]


def levers(W, turn, seat, qs, core, aggs, ys, ar):
    rivals = turn["rivals"]
    order = core["war"]
    peace, peace_with = [], []
    for rival, q in zip(rivals, qs):
        at_war = rival["r"][W.r["at_war"]] > 0
        score = fire(W.neurons["peace"], {"S": seat, "R": rival["r"], "Q": q}, ar) if at_war else 0
        peace.append(score)
        spent = any(s[0] == rival["id"] for s in core["spent"])
        not_order = order is None or order["rival"] != rival["id"]
        if at_war and not_order and (spent or score >= W.gates["peace"]):
            peace_with.append(rival["id"])
    guard = [fire(W.neurons["guard"], {"S": seat, "C": _city_row(W, c, rivals, qs)}, ar) for c in turn["cities"]]
    military = fire(W.neurons["military_tilt"], {"S": seat, "A": aggs, "Y": ys}, ar)
    cs = fire(W.neurons["cs_gold_share"], {"F": turn["f"], "K": turn["k"], "G": turn["g"], "Y": ys}, ar)
    deny = [r["id"] for r, q in zip(rivals, qs) if q[W.q["runaway"]] >= W.gates["deny_help"]]
    embargo, best = None, -1
    for r, q in zip(rivals, qs):
        if q[W.q["runaway"]] >= W.gates["embargo"] and q[W.q["runaway"]] > best:
            embargo, best = r["id"], q[W.q["runaway"]]
    return {"peace": peace, "peace_with": peace_with, "deny_help": deny, "embargo": embargo, "guard": guard, "military": military, "cs_gold_share": cs}


def forward(W: Weights, turn: dict, ar: Arith = FLOAT, teacher: dict | None = None) -> dict:
    """One turn's neurons (the keys of a trace line's `out`)."""
    t = teacher or {}
    ease = eases(W, turn, ar)
    turn = with_soft(W, turn, t.get("ease", ease))
    seat = seat_concepts(W, turn, ar)
    s_in = t.get("s", seat)
    qs = rival_concepts(W, turn, s_in, ar, recorded=t.get("q"))
    q_in = t.get("q", qs)
    core = war_plan(W, turn, q_in, t.get("ease", ease))
    core_in = t.get("core", core)
    aggs = aggregates(W, turn, q_in, core_in)
    a_in = t.get("a", aggs)
    tilts = tilt(W, s_in, a_in, ar)
    ys = shares(turn, t.get("tilt", tilts), ar)
    y_in = t.get("y", ys)
    out = {"s": seat, "ease": ease, "q": qs, "core": core, "a": aggs, "tilt": tilts, "y": ys}
    out.update(levers(W, turn, s_in, q_in, core_in, a_in, y_in, ar))
    return out


def deviation(a, b) -> float:
    """The largest absolute difference between two nested numbers of the same shape."""
    if isinstance(a, dict):
        return max([deviation(a[k], b[k]) for k in a] or [0.0])
    if isinstance(a, (list, tuple)):
        if len(a) != len(b):
            return math.inf
        return max([deviation(x, y) for x, y in zip(a, b)] or [0.0])
    if a is None or b is None or isinstance(a, str) or isinstance(b, str):
        return 0.0 if a == b else math.inf
    return abs(float(a) - float(b))


NEURONS = ("s", "ease", "q", "a", "tilt", "y", "peace", "guard", "military", "cs_gold_share")
DECISIONS = ("core", "peace_with", "deny_help", "embargo")


def parity(W: Weights, lines, ar: Arith = FLOAT, teacher: bool = True) -> dict:
    """The largest deviation of each output over the recorded turns, and the decisions that differ."""
    worst = {k: 0.0 for k in NEURONS}
    decisions = 0
    for line in lines:
        rec = json.loads(line) if isinstance(line, str) else line
        out = forward(W, rec["in"], ar, teacher=rec["out"] if teacher else None)
        for k in NEURONS:
            worst[k] = max(worst[k], deviation(out[k], rec["out"][k]))
        decisions += sum(deviation(out[k], rec["out"][k]) != 0 for k in DECISIONS)
    worst["decisions_differ"] = decisions
    return worst
