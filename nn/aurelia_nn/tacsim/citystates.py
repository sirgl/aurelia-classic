"""The economy on the GPU (MLX), part 5: city-states, exact to the Rust rules (`systems::city_states`).

    python -m aurelia_nn.tacsim.citystates DUMP.jsonl|DIR    # parity: bonuses, influence, allies

The state (the dump's `world.cs`): per (city-state, nation) the influence ×100, the pledge, the wary
flag, the unit gift counter, the contact; per city-state its ally and personality; per nation the
city-states it attacked. The rules: each city-state type's thresholds and its friend and ally bonuses
by era.

- the bonuses a nation gets (`bonus_from`, `player_bonus`, `city_food_bonus`): per city-state its
  friend and ally amounts of the latest era the nation reached, Siam's percent on food, culture and
  faith — the city-state food of every city, the culture, faith and happiness of the nation (they
  replace the dump's inputs);
- a city-state's own turn (`city_state_turn`), for every living nation in id order: the first contact;
  the drift toward the resting point (decay from above: hostile 1.5, aggressor 2, else 1 a turn, times
  Patronage's percent less half of Greece's rate, a quarter less when its capital follows the nation's
  religion; recovery from below, Greece's rate, half again for the same religion), Treaty
  Organization's and Gunboat Diplomacy's shifts, the decay stopping at the resting point; the
  intrusion of a non-friend's combat units on its land; the ally after every change (the current one
  keeps it unless another has strictly more; the most influence, the lower id on a tie).

Not in MLX (counted apart by the parity): quests and their rewards, militaristic unit gifts' new
counters (the events stream), elections rigged by spies.
"""

import argparse
import json
import time

LEVELS = ("Friends", "Allies")
KINDS = ("CapitalFood", "AllCitiesFood", "Culture", "Faith", "Happiness", "UnitGiftTurns", "UnitGiftRandom")
SCALED = {"CapitalFood", "AllCitiesFood", "Culture", "Faith"}
STATS = ("RestingPoint", "DecayPercent", "GoldGiftPercent", "BullyScorePercent", "UnitGiftInfluence", "FriendBonusPercent",
         "FriendshipRate", "AnnexCityStates", "RestingPointSameReligion", "AfraidInfluence", "TradeRouteInfluence",
         "UnitGiftRatePercent", "AllyResourceBonus", "AllyGreatPeople", "AllyScience", "GreatPersonGiftInfluence", "TradeMissionGoldPercent")
FRIENDS_THRESHOLD, ALLIES_THRESHOLD, AT_WAR_INFLUENCE = 30, 60, -60
ANCHOR_PROTECTED, ANCHOR_WARY = 10, -20
AGGRESSOR_THRESHOLD = 2
DROP, DROP_HOSTILE, DROP_AGGRESSOR, RECOVERY = -100, -150, -200, 100
INTRUSION = -1000
SAME_RELIGION_DECAY, SAME_RELIGION_RECOVERY = -25, 50


def stat(stats, name):
    return stats[STATS.index(name)]


def tdiv(a, b):
    """Rust's integer division (toward zero)."""
    q = abs(a) // abs(b)
    return q if (a >= 0) == (b > 0) else -q


class Rules:
    def __init__(self, er):
        self.types = []
        for t in er.get("cs_types", []):
            bonuses = [(lv, kind, max(era, 0), amount) for lv, kind, era, amount in t["bonuses"]]
            self.types.append({"friends": t["friends"], "allies": t["allies"], "bonuses": bonuses})
        self.seat_type = er.get("cs_seats", [])

    def thresholds(self, cs):
        t = self.seat_type[cs] if cs < len(self.seat_type) else -1
        if t < 0:
            return FRIENDS_THRESHOLD, ALLIES_THRESHOLD
        return self.types[t]["friends"], self.types[t]["allies"]

    def amount(self, cs, level, kind, era):
        """`bonus_amount`: the entry of the latest era reached."""
        t = self.seat_type[cs]
        if t < 0:
            return 0
        best = None
        for lv, k, e, a in self.types[t]["bonuses"]:
            if lv == level and k == kind and e <= era and (best is None or e >= best[0]):
                best = (e, a)
        return best[1] if best else 0


class World:
    """The city-states' state of a side (`world.cs`, `world.seats`)."""

    def __init__(self, st):
        w = st["world"]
        self.seats = w["seats"]
        self.standing = {}
        for s in w["cs"]["standings"]:
            cs, m, infl, ever, pledged, wary, spawn, contacted, qc, rest, friends, war = s
            self.standing[(cs, m)] = {"influence": infl, "ever": ever, "pledged": pledged, "wary": wary, "spawn": spawn,
                                      "contacted": contacted, "rest": rest, "friends": friends, "war": war}
        self.minor = {m[0]: {"ally": m[1], "since": m[2], "personality": m[3], "alive": m[5]} for m in w["cs"]["minors"]}
        self.major = {m[0]: {"attacked": m[1]} for m in w["cs"]["majors"]}
        self.quests = w["cs"]["quests"]

    def majors(self):
        return [p for p, s in enumerate(self.seats) if s[5] and s[7]]

    def influence(self, cs, m):
        return tdiv(self.standing[(cs, m)]["influence"], 100)

    def effective(self, cs, m):
        s = self.standing[(cs, m)]
        return AT_WAR_INFLUENCE if s["war"] else tdiv(s["influence"], 100)


def is_friends(rules, w, cs, m):
    return w.effective(cs, m) >= rules.thresholds(cs)[0]


def bonus_from(rules, w, cs, m, kind):
    if rules.seat_type[cs] < 0 or not w.minor[cs]["alive"]:
        return 0
    era = w.seats[m][1]
    total = 0
    if is_friends(rules, w, cs, m):
        total += rules.amount(cs, "Friends", kind, era)
    if w.minor[cs]["ally"] == m:
        total += rules.amount(cs, "Allies", kind, era)
    if kind in SCALED and total != 0:
        total = tdiv(total * (100 + stat(w.seats[m][4], "FriendBonusPercent")), 100)
    return total


def player_bonus(rules, w, m, kind):
    return sum(bonus_from(rules, w, cs, m, kind) for cs in w.minor)


def city_food(rules, w, m, capital):
    """`city_food_bonus` of a city of nation `m` (whole food)."""
    if not w.minor or not w.seats[m][5]:
        return 0
    t = 0
    for cs in w.minor:
        t += bonus_from(rules, w, cs, m, "AllCitiesFood")
        if capital:
            t += bonus_from(rules, w, cs, m, "CapitalFood")
    return tdiv(t, 100)


def follows_religion(wr, cs, major):
    """The city-state's capital (its Palace city, else its first city) follows `major`'s religion."""
    rid = wr["players"][major]["religion"]
    if rid < 0:
        return False
    own = [c for c in wr["cities"] if c["owner"] == cs]
    cap = next((c for c in own if c["capital"]), own[0] if own else None)
    return cap is not None and cap["majority"] == 2 + rid


def update_ally(rules, w, cs):
    allies = rules.thresholds(cs)[1]
    cur = w.minor[cs]["ally"]
    best = None
    for p in w.majors():
        s = w.effective(cs, p)
        if s >= allies and (best is None or s > w.effective(cs, best)):
            best = p
    nxt = best
    if cur >= 0 and best is not None and w.effective(cs, cur) >= allies and w.effective(cs, best) <= w.effective(cs, cur):
        nxt = cur
    nxt = -1 if nxt is None else nxt
    if nxt != cur:
        w.minor[cs]["ally"] = nxt
        return True
    return False


def set_influence(rules, w, cs, m, value):
    s = w.standing[(cs, m)]
    s["influence"] = max(value, AT_WAR_INFLUENCE * 100)
    update_ally(rules, w, cs)


def city_state_turn(rules, w, wr, cs, met, intruders, afraid, connected):
    """The influence part of a city-state's turn on `w` (changed in place). `met`: the city-state's met
    list; `intruders`, `afraid`, `connected`: {major: ...} of the dump's extras."""
    for m in w.majors():
        st = w.standing[(cs, m)]
        if met[m] and not st["contacted"]:
            st["contacted"] = True
        stats = w.seats[m][4]
        base = w.influence(cs, m)
        rest = st["rest"]
        same = follows_religion(wr, cs, m)
        if base == rest:
            drift = 0
        elif base > rest:
            if w.minor[cs]["personality"] == "Hostile":
                b = DROP_HOSTILE
            elif w.major.get(m, {}).get("attacked", 0) >= AGGRESSOR_THRESHOLD:
                b = DROP_AGGRESSOR
            else:
                b = DROP
            pct = max(100 + stat(stats, "DecayPercent") - tdiv(stat(stats, "FriendshipRate"), 2) + (SAME_RELIGION_DECAY if same else 0), 0)
            drift = tdiv(b * pct, 100)
        else:
            drift = tdiv(RECOVERY * max(100 + stat(stats, "FriendshipRate") + (SAME_RELIGION_RECOVERY if same else 0), 0), 100)
        shift = 0
        if met[m]:
            tr = stat(stats, "TradeRouteInfluence")
            if tr != 0 and connected.get(m, False):
                shift += tr * 100
            af = stat(stats, "AfraidInfluence")
            if af != 0 and afraid.get(m, False):
                shift += af * 100
        change = drift + shift
        if change != 0:
            nxt = base + tdiv(change, 100)
            if base >= rest and nxt < rest:
                set_influence(rules, w, cs, m, rest * 100)
            else:
                set_influence(rules, w, cs, m, st["influence"] + change)
        # Intrusion (after the unit gifts, which do not move the influence).
        if not st["war"] and not is_friends(rules, w, cs, m) and intruders.get(m, 0) > 0:
            set_influence(rules, w, cs, m, st["influence"] + INTRUSION)
    update_ally(rules, w, cs)


# ---- parity ----

def run(path, limit_games=None, show=8):
    from . import city as C
    from . import religion as RL
    t0 = time.time()
    tot = {"lines": 0, "bonus": 0, "bonus_ok": 0, "food": 0, "food_ok": 0, "cs_turns": 0, "cs_turns_ok": 0, "pairs": 0, "pairs_ok": 0,
           "quest_pairs": 0, "allies_ok": 0, "stats_ok": 0}
    examples = []
    rules = None
    for g, game in C.each_game(path, limit_games):
        er = game["rules"]
        rules = Rules(er)  # per game: the seats and the map size are the game's
        for t in game["turns"]:
            pre = t["pre"]["state"]
            if "world" not in pre:
                continue
            p = t["player"]
            tot["lines"] += 1
            w = World(pre)
            ex = pre["extra"]
            tot["stats_ok"] += int(ex["cs_stats"] == w.seats[p][4])
            if w.seats[p][5]:
                mine = [player_bonus(rules, w, p, k) for k in ("CapitalFood", "AllCitiesFood", "Culture", "Faith", "Happiness")]
                tot["bonus"] += 1
                ok = mine == ex["cs_bonus"]
                tot["bonus_ok"] += int(ok)
                if not ok and len(examples) < show:
                    examples.append({"game": g, "round": t["round"], "player": p, "bonus": [mine, ex["cs_bonus"]]})
                for c in pre["cities"]:
                    tot["food"] += 1
                    tot["food_ok"] += int(city_food(rules, w, p, c["capital"]) == c["inputs"]["base"][0])
            elif w.seats[p][6] and p in w.minor and w.minor[p]["alive"]:
                wr = RL.world(pre)
                intr = {m: n for m, n, a, cn in ex["intruders"]}
                afraid = {m: a for m, n, a, cn in ex["intruders"]}
                conn = {m: cn for m, n, a, cn in ex["intruders"]}
                city_state_turn(rules, w, wr, p, pre["met"], intr, afraid, conn)
                post = World(t["post"]["state"])
                tot["cs_turns"] += 1
                all_ok = True
                quests_pre = {(q[0], q[1], q[2]) for q in pre["world"]["cs"]["quests"] if q[0] == p}
                quests_post = {(q[0], q[1], q[2]) for q in t["post"]["state"]["world"]["cs"]["quests"] if q[0] == p}
                for m in w.majors():
                    a, b = w.standing[(p, m)]["influence"], post.standing[(p, m)]["influence"]
                    quest = any(q[1] == m for q in quests_pre - quests_post)
                    if quest and a != b:
                        tot["quest_pairs"] += 1
                        continue
                    tot["pairs"] += 1
                    tot["pairs_ok"] += int(a == b)
                    all_ok &= a == b
                    if a != b and len(examples) < show:
                        examples.append({"game": g, "round": t["round"], "cs": p, "major": m, "influence": [pre["world"]["cs"]["standings"], a, b][1:],
                                         "pre": [s for s in pre["world"]["cs"]["standings"] if s[0] == p and s[1] == m], "stats": w.seats[m][4],
                                         "intr": intr.get(m), "personality": w.minor[p]["personality"]})
                ally_ok = w.minor[p]["ally"] == post.minor[p]["ally"]
                tot["allies_ok"] += int(ally_ok)
                tot["cs_turns_ok"] += int(all_ok and ally_ok)
    return {**tot, "examples": examples, "s": round(time.time() - t0, 1)}


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("dump")
    ap.add_argument("--games", type=int)
    ap.add_argument("--show", type=int, default=8)
    o = ap.parse_args(argv)
    r = run(o.dump, o.games, o.show)
    print(f"city-states: stats {r['stats_ok']}/{r['lines']}; bonuses of a nation {r['bonus_ok']}/{r['bonus']} (food, culture, faith, happiness), "
          f"city-state food of a city {r['food_ok']}/{r['food']}; a city-state's turn {r['cs_turns_ok']}/{r['cs_turns']} (influence pairs "
          f"{r['pairs_ok']}/{r['pairs']}, {r['quest_pairs']} pairs a quest reward moved, ally {r['allies_ok']}/{r['cs_turns']}); {r['s']} s")
    for e in r["examples"]:
        print("  ex:", json.dumps(e)[:1500])


if __name__ == "__main__":
    main()
