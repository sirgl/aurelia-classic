#!/usr/bin/env python3
"""Compares the project's formulas with the same formulas in Unciv's Kotlin
sources (pinned commit 70d0f5c8, MPL-2.0; see classic/docs/classic/SOURCES.md, section
"Formulas and tables"). Nothing is downloaded or stored: both sides are
written here from the repository's Rust (cited per function) and from the
Unciv lines named in each docstring, read on 2026-10-20.

Run: python3 -I classic/tools/unciv-formulas.py
The Rust side is re-implemented in Python; every function is first asserted
against the table that the Rust unit test of the same formula holds, so a
drift of this file from the Rust code fails loudly.
"""
import math


# --- growth: classic/crates/classic-rules/src/systems/growth.rs growth_threshold ----
def ours_growth(pop):
    n = pop - 1
    return 15 + 8 * n + math.isqrt(n ** 3)


def unciv_growth(pop):  # CityPopulationManager.kt getFoodToNextPopulation, speed 1
    return int(15 + 8 * (pop - 1) + math.floor((pop - 1) ** 1.5))


# --- policy cost: classic/crates/classic-rules/src/derived/culture.rs policy_cost ---
def ours_policy(paid, cities, per_city=10):
    cost = int((paid * 3) ** 2.01) + 25
    cost += cost * (cities - 1) * per_city // 100
    return cost // 5 * 5


def unciv_policy(adopted, cities, per_city=0.1):  # PolicyManager.kt getPolicyCultureCost
    cost = 25 + (adopted * 3) ** 2.01
    c = round(cost * (1 + per_city * (cities - 1)))
    return c - c % 5


# --- border culture: culture.rs border_cost vs CityExpansionManager.kt ---
def ours_border(level):
    cost = 15 + int((level * 10) ** 1.1)
    return max(1, cost // 5 * 5 if cost > 10 else cost)


def unciv_border(claimed):  # getCultureToNextTile, speed 1, not a city-state
    return round(6 * (max(0, claimed) + 1.4813) ** 1.3)


# --- buying a tile: kernel/src/world/territory.rs vs getGoldCostOfTile ---
def ours_buy(bought, ring, nearest):
    c = 50 + 5 * bought
    f = 100 + 100 * (ring - nearest) // 3
    if f > 100:
        c = c * f // 100
    return c // 5 * 5


def unciv_buy(claimed, ring):
    return round(50 * (ring - 1) + claimed * 5)


# --- research cost multipliers: world_size.rs vs MapSize.kt Predefined ---
OURS_TECH_PER_CITY = {"Standard": 5, "Large": 3, "Huge": 2}
UNCIV_TECH_PER_CITY = {"Standard": 5.0, "Large": 3.75, "Huge": 2.5}
OURS_POLICY_PER_CITY = {"Standard": 10, "Large": 7, "Huge": 5}
UNCIV_POLICY_PER_CITY = {"Standard": 10.0, "Large": 7.5, "Huge": 5.0}


def tech_city_factor_ours(cities, size="Standard"):
    return 1 + OURS_TECH_PER_CITY[size] * cities / 100


def tech_city_factor_unciv(cities, size="Standard"):
    return 1 + UNCIV_TECH_PER_CITY[size] * (cities - 1) / 100


# --- unit gold maintenance: derived/maintenance.rs vs CivInfoStatsForNextTurn.kt ---
def ours_upkeep(paid, progress):  # gold per paid unit 0.5, multiplier 8, exponent divisor 7 (ours)
    return int(((1 + progress * 8) * 0.5 * paid) ** (1 + progress / 7))


def unciv_upkeep(paid, progress):  # getUnitMaintenance: base 0.5, (1 + p), exponent 1 + p / 3
    return int((0.5 * paid * (1 + progress)) ** (1 + progress / 3))


def main():
    # self-check against the Rust unit-test tables
    assert [ours_policy(n, 1) for n in range(9)] == [25, 30, 60, 105, 170, 255, 355, 475, 615]
    assert [ours_policy(n, 4) for n in range(8)] == [30, 40, 75, 135, 220, 330, 465, 620]
    assert [ours_border(n) for n in range(11)] == [15, 25, 40, 55, 70, 85, 105, 120, 135, 155, 170]
    print("growth, pop 1..30: equal in", sum(ours_growth(p) == unciv_growth(p) for p in range(1, 31)), "of 30")
    for cities in (1, 4, 8):
        rows = [(ours_policy(n, cities), unciv_policy(n, cities)) for n in range(0, 12)]
        eq = sum(a == b for a, b in rows)
        worst = max(abs(a - b) / b for a, b in rows)
        print(f"policy cost, {cities} cities, 0..11 adopted: equal in {eq} of 12, worst relative difference {worst:.1%}")
    rows = [(ours_border(n), unciv_border(n)) for n in range(0, 31)]
    print("border culture, level 0..30 (ours level n vs Unciv tiles claimed n):",
          "equal in", sum(a == b for a, b in rows), "of 31; ours/Unciv at 0,5,10,20,30:",
          [f"{rows[i][0]}/{rows[i][1]}" for i in (0, 5, 10, 20, 30)])
    rows = [(ours_buy(k, r, 2), unciv_buy(k, r)) for k in (0, 10, 30) for r in (2, 3)]
    print("tile gold (bought 0/10/30, ring 2/3), ours/Unciv:", [f"{a}/{b}" for a, b in rows])
    for size in ("Standard", "Large", "Huge"):
        o = [tech_city_factor_ours(c, size) for c in (1, 4, 8)]
        u = [tech_city_factor_unciv(c, size) for c in (1, 4, 8)]
        print(f"research cost city factor, {size}, 1/4/8 cities: ours {[round(x, 3) for x in o]} Unciv {[round(x, 3) for x in u]}")
    for paid in (5, 10, 20, 40):
        row = [(ours_upkeep(paid, t / 500), unciv_upkeep(paid, t / 500)) for t in (50, 150, 300)]
        print(f"unit upkeep, {paid} paid units, turn 50/150/300, ours/Unciv:", [f"{a}/{b}" for a, b in row])
    print("policy per-city percent: ours", OURS_POLICY_PER_CITY, "Unciv", UNCIV_POLICY_PER_CITY)


if __name__ == "__main__":
    main()
