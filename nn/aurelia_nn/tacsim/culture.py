"""The economy on the GPU (MLX): the player's culture and social policies, exact to the Rust rules
(`derived::culture::policy_cost`, `systems::policies`, the Culture step of the turn).

    python -m aurelia_nn.tacsim.culture DUMP.jsonl|DIR   # parity: the policy price of every world, every adoption

    price     25 + (3 × paid)^2.01 (the kernel's deterministic power) × (100 + Policies cost %, ≥ −75) %
              × the difficulty's policy % × the speed's culture % × (1 + per-city share × (cities − 1)),
              the per-city share × (100 + PolicyCityPenalty %) %; to the nearest whole, down to a multiple of 5
    adoption  a branch's opener (the branch unlocked: one policy's payment), a policy, a tenet (a free
              tenet first): a free policy first, else the price from the pool; a free one does not raise
              the next price; the last policy of a branch brings its finisher free
    the turn  the cities' culture (Borders) + city-states + Cultural Heritage Sites (2 a world wonder)
              + the excess happiness × HappinessToCulturePercent; × (100 + the league's boost) %;
              into the pool and the lifetime, the last 8 turns kept; the most cities ever

The adoptions are the player's commands (its AI's choice): MLX replays the choice the dump records
(the policies the next line holds) with its own prices and payments. Inputs from the subsystems not in
MLX: the city-states' culture bonus, the World Congress (heritage sites, the culture boost).
"""

import argparse
import json
import math

from . import city as C
from . import player as P
from . import rdebug as R
from . import turn as TN

BASE, PER_POLICY, EXPONENT, MAX_DISCOUNT, ROUND_TO = 25.0, 3.0, 2.01, -75, 5
RECENT_TURNS = 8
POLICIES = P.key("Module(Policies)")
CITY_PENALTY = P.key("Module(PolicyCityPenalty)")
HAPPY_CULTURE = P.key("Module(Society(HappinessToCulturePercent))")


def price(paid, cities, per_city_percent, per_city_discount, cost_percent, difficulty, speed):
    """`derived::culture::policy_cost` (IEEE doubles in the kernel's order)."""
    cost = BASE + TN.kpow(float(paid) * PER_POLICY, EXPONENT)
    per_city = per_city_percent / 10000.0
    if per_city_discount != 0:
        per_city *= float(100 + per_city_discount) / 100.0
    city_modifier = per_city * (float(cities) - 1.0)
    cost *= float(100 + max(cost_percent, MAX_DISCOUNT)) / 100.0
    cost *= float(max(difficulty, 1)) / 100.0
    if speed != 100:
        cost *= float(max(speed, 1)) / 100.0
    c = math.floor(cost * (1.0 + city_modifier) + 0.5)
    c -= int(math.fmod(c, ROUND_TO))
    return max(0, min(c, 2 ** 32 - 1))


def policy_cost(model, prow, game, t, state, policies, side="pre"):
    """The next policy's price for culture state `state` and the policy set `policies` (the player's
    cost percents read the policies)."""
    st = t[side]["state"]
    tt = dict(t)
    tt[side] = dict(t[side], state=dict(st, policies=policies))
    _, costs = P.stats_and_costs(prow, tt, side)
    hc = R.body(R.parse(st.get("handicap", game["rules"]["players"][t["player"]]["handicap"])))
    paid = len(policies) - state["free_used"]
    return price(paid, st["effective_cities"], game["rules"]["policy_per_city"], costs.get(CITY_PENALTY, 0), costs.get(POLICIES, 0),
                 R.body(hc["module"])["policy_percent"], model.speed["culture_percent"])


class Defs:
    def __init__(self, er):
        self.policies = er["policies"]
        self.branches = er["branches"]
        self.role = [R.tag(R.parse(p["role"])) for p in self.policies]
        self.opener_of = {b["opener"]: i for i, b in enumerate(self.branches) if b["opener"] is not None}
        self.finisher_of = {b["finisher"]: i for i, b in enumerate(self.branches) if b["finisher"] is not None}

    def complete(self, policies, branch):
        own = [i for i, p in enumerate(self.policies) if p["branch"] == branch and self.role[i] == "Policy"]
        return bool(own) and all(i in policies for i in own)


def defs(model):
    if not hasattr(model, "_culture_defs"):
        model._culture_defs = Defs(model.er)
    return model._culture_defs


def adopt(model, prow, game, t, before, policies_before, policies_after, branches_after):
    """Replay the adoptions from `before` (a culture state with its policies) to the policy list
    `policies_after`: per new policy its payment. Returns the culture state after them."""
    d = defs(model)
    s = dict(before)
    pol = list(policies_before)
    branches = list(before["branches"])
    for x in policies_after[len(policies_before):]:
        role = d.role[x]
        if role == "Finisher":
            pol.append(x)
            s["free_used"] += 1
            continue
        if role == "Opener":
            b = d.opener_of.get(x)
            if b is not None and d.branches[b]["kind"] == "Ideology":
                pol.append(x)
                s["free_used"] += 1
                continue
        free_tenet = role != "Policy" and role != "Opener" and s["free_tenets"] > 0
        if free_tenet:
            s["free_tenets"] -= 1
            pol.append(x)
            s["free_used"] += 1
        elif s["free_policies"] > 0:
            s["free_policies"] -= 1
            pol.append(x)
            s["free_used"] += 1
        else:
            cost = policy_cost(model, prow, game, t, s, pol, side="pre")
            s["pool"] -= cost
            pol.append(x)
        if role == "Opener":
            b = d.opener_of.get(x)
            if b is not None and b not in branches:
                branches.append(b)
    for b in branches_after:
        if b not in branches:
            branches.append(b)
    s["branches"] = branches
    return s, pol


def adopt_inferred(model, prow, game, t, ps, st):
    """`adopt` from the previous line's world to this one's, with the free policies and tenets granted
    on the way outside the player's end turn (the World Congress, an era entered by a great
    scientist, an ideology's free tenets): the fewest that make the line's culture state follow (an
    input; without one, the plain replay)."""
    best = None
    want = st["culture"]
    for total in range(7):
        for gp_ in range(total + 1):
            gt_ = total - gp_
            before = dict(ps["culture"], free_policies=ps["culture"]["free_policies"] + gp_, free_tenets=ps["culture"]["free_tenets"] + gt_)
            after, pol = adopt(model, prow, game, t, before, ps["policies"], st["policies"], st["culture"]["branches"])
            if best is None:
                best = (after, pol, 0)
            if all(after[k] == want[k] for k in ("pool", "free_policies", "free_tenets", "free_used")):
                return after, pol, total
    return best


def collect(model, prow, game, t, culture_sum, excess, wonders, cities):
    """The Culture step: the culture of the turn into the pool."""
    st = t["pre"]["state"]
    c = dict(st["culture"])
    stats, _ = P.stats_and_costs(prow, t, "pre")
    happy = excess * max(stats.get(HAPPY_CULTURE, 0), 0) // 100 if excess > 0 else 0
    heritage = game["rules"].get("heritage_per_wonder", 2) * wonders if st.get("heritage") else 0
    total = culture_sum + max(c["cs"], 0) + heritage + happy
    boost = game["rules"].get("boost_percent", 75) if c["boost_turns"] > 0 else 0
    total = min(total * (100 + boost) // 100, 2 ** 32 - 1)
    c["pool"] = min(c["pool"] + total, 2 ** 32 - 1)
    c["lifetime"] += total
    c["recent"] = (c["recent"] + [total])[-RECENT_TURNS:]
    c["max_cities"] = max(c["max_cities"], cities)
    return c


FIELDS = ("pool", "lifetime", "recent", "free_policies", "free_tenets", "free_used", "max_cities", "branches")


def run(path, limit_games=None, show=6):
    """The price on every side of every line; the adoptions between a player's lines."""
    tot = {"prices": 0, "prices_ok": 0, "adoptions": 0, "adoptions_ok": 0, "policies_adopted": 0}
    ex = []
    model = prow = None
    for g, game in C.each_game(path, limit_games):
        if model is None:
            model = C.Model(game["tables"], game["rules"])
            prow = P.Rows(model.rows, model.er)
        last = {}
        for t in P.prepare(game, game["turns"]):
            for side in ("pre", "post"):
                st = t[side]["state"]
                if "culture" not in st:
                    continue
                c = st["culture"]
                mine = policy_cost(model, prow, game, t, c, st["policies"], side)
                tot["prices"] += 1
                tot["prices_ok"] += int(mine == c["policy_cost"])
                if mine != c["policy_cost"] and len(ex) < show:
                    ex.append({"game": g, "round": t["round"], "player": t["player"], "side": side, "price": [c["policy_cost"], mine],
                               "paid": len(st["policies"]) - c["free_used"], "cities": st["effective_cities"]})
            p = t["player"]
            st = t["pre"]["state"]
            if "culture" in st and p in last:
                prev = last[p]
                ps = prev["post"]["state"]
                if ps["policies"] == st["policies"] and ps["culture"]["pool"] != st["culture"]["pool"]:
                    # Culture the player's commands brought (ruins, a great writer, a natural wonder): not in MLX.
                    tot["command_culture"] = tot.get("command_culture", 0) + 1
                elif ps["policies"] != st["policies"]:
                    tot["adoptions"] += 1
                    tot["policies_adopted"] += len(st["policies"]) - len(ps["policies"])
                    after, pol, grants = adopt_inferred(model, prow, game, t, ps, st)
                    tot["command_grants"] = tot.get("command_grants", 0) + grants
                    want = st["culture"]
                    ok = pol == st["policies"] and all(after[k] == want[k] for k in ("pool", "free_policies", "free_tenets", "free_used"))
                    tot["adoptions_ok"] += int(ok)
                    if not ok and len(ex) < show:
                        ex.append({"game": g, "round": t["round"], "player": p, "adopted": st["policies"][len(ps["policies"]):],
                                   "want": {k: want[k] for k in ("pool", "free_policies", "free_tenets", "free_used")},
                                   "mlx": {k: after[k] for k in ("pool", "free_policies", "free_tenets", "free_used")},
                                   "before": {k: ps["culture"][k] for k in ("pool", "free_policies", "free_tenets", "free_used", "policy_cost")}})
            last[p] = t
    return {**tot, "examples": ex}


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("dump")
    ap.add_argument("--games", type=int)
    ap.add_argument("--show", type=int, default=6)
    o = ap.parse_args(argv)
    rep = run(o.dump, o.games, o.show)
    print(f"culture: policy price {rep['prices_ok']}/{rep['prices']} worlds; adoptions between a player's turns {rep['adoptions_ok']}/{rep['adoptions']} "
          f"({rep['policies_adopted']} policies) equal on the pool, the free policies and tenets; free policies or tenets the commands granted "
          f"{rep.get('command_grants', 0)}; lines whose pool the commands changed without an adoption {rep.get('command_culture', 0)}")
    for e in rep["examples"]:
        print("  ex:", json.dumps(e))


if __name__ == "__main__":
    main()
