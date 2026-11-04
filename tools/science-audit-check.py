#!/usr/bin/env python3
"""The science audit's independent calculation (classic/docs/ai-science-audit.md).

Reads the JSON lines of `ai-science-audit` and computes every city's science
from a hand-written table of the reference values (Unciv 4.22.10's Gods & Kings
files and the BNW additions of the rules), not from the rules files or the
engine's code; prints every city whose science differs from the engine's and
every empire's total next to the engine's.

    ai-science-audit LOG.aurlog --players 0,1,2 --rounds 100,150,197 > seats.jsonl
    python3 tools/science-audit-check.py seats.jsonl
"""
import json
import sys

# Beakers per 2 citizens, flat beakers, percents, by building key (the last part).
PER_TWO_CITIZENS = {"library", "paper_maker", "royal_library", "public_school"}
FLAT = {"palace": 3, "public_school": 3, "research_lab": 4, "national_college": 3,
        "great_library": 3, "oxford_university": 3, "pyramid": 2}
PERCENT = {"university": 33, "wat": 33, "observatory": 50, "research_lab": 50, "national_college": 50}
SCIENTIST = 3          # a Scientist's beakers
SPECIALIST_BONUS = 2   # Secularism, and Korea's trait, each a specialist
RATIONALISM = 10       # the opener: percent of the cities' sum while not unhappy (BNW)
UNCIV_RATIONALISM = 15 # Unciv G&K: a city percent while happy


def policies(seat):
    return {p.split(" / ")[0] for p in seat["policies"]}


def city_science(seat, city, unciv_stacking=False):
    held = policies(seat)
    built = {b.split("/")[-1] for b in city["buildings"]}
    pop = city["population"]
    korea = seat.get("nation") == "core:nation/korea"
    secularism = "core:policy/rationalism/secularism" in held
    specialists = 0
    for kind, count in city["specialists"].items():
        kind = kind.split("/")[-1]
        if kind == "citizen":
            continue
        each = (SCIENTIST if kind == "scientist" else 0) + SPECIALIST_BONUS * (secularism + korea)
        specialists += each * count
    flat = sum(v for k, v in FLAT.items() if k in built) + sum(pop // 2 for k in PER_TWO_CITIZENS if k in built)
    if "core:policy/order/party-leadership" in held:
        flat += 1
    if "core:policy/commerce/mercantilism" in held:
        flat += len(built & {"mint", "market", "bank", "stock_exchange"})
    if "core:policy/autocracy/third-alternative" in held and city["capital"]:
        flat += 4
    percent = sum(v for k, v in PERCENT.items() if k in built)
    if "core:policy/rationalism/free-thought" in held and "university" in built:
        percent += 17
    if "core:policy/order/workers-faculties" in held and "factory" in built:
        percent += 20
    if unciv_stacking and "core:policy/rationalism/opener" in held and seat["excess_happiness"] >= 0:
        percent += UNCIV_RATIONALISM
    base = pop + city["tile_science"] + specialists
    return {"specialists": specialists, "flat": flat, "percent": percent, "rate": (base + flat) * (100 + percent) // 100}


def main(path):
    cities = differ = 0
    for line in open(path):
        seat = json.loads(line)
        held = policies(seat)
        total = unciv = 0
        for city in seat["cities"]:
            cities += 1
            want = city_science(seat, city)
            engine = (city["specialist_science"], city["flat"], city["percent"], city["rate"] - city["trade_science"])
            if (want["specialists"], want["flat"], want["percent"], want["rate"]) != engine:
                differ += 1
                print("city differs:", seat["player"], seat["round"], city["name"], want, engine)
            after = city["trade_science"] + city["process_science"]
            total += want["rate"] + after
            unciv += city_science(seat, city, True)["rate"] + after
        allied = seat["allied_cs_science"] if "core:policy/patronage/scholasticism" in held else 0
        happy = "core:policy/rationalism/opener" in held and seat["excess_happiness"] >= 0
        expected = total + (total * RATIONALISM // 100 if happy else 0) + allied
        mark = "" if expected == seat["player_science"] else "   <-- differs"
        print(f"player {seat['player']} round {seat['round']}: engine {seat['player_science']}, expected {expected}, "
              f"with Unciv's Rationalism {unciv + allied}{mark}")
    print(f"cities checked {cities}, differing {differ}")


if __name__ == "__main__":
    main(sys.argv[1])
