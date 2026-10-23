#!/usr/bin/env python3
"""Sample outputs of the own functions of the Brave New World-like systems (tourism, trade routes).

Prints old (the values before the own functions) and new (the code) for five inputs each.
Usage: python3 -I classic/tools/calibration/classic-tourism-trade.py"""


def level_old(k):
    return [10, 30, 60, 100, 200][k - 1]


def level_new(k):
    l = min(k, 4)
    base = 6 * l * l + 3 * l
    return 2 * base if k > 4 else base


def gold_term_old(g):  # hundredths: g / 20
    return g // 20


def gold_term_new(g):
    return 300 * g // (g + 4000)


def internal_old(era):
    return 300 + 50 * era


def internal_new(era):
    return 270 + 60 * era


print("influence level thresholds, percent of the target's lifetime culture")
print("level  old  new  new/old")
for k in range(1, 6):
    print(f"{k:5d} {level_old(k):4d} {level_new(k):4d}  {level_new(k) / level_old(k):.2f}")
print("\nrouted gold term of one city, hundredths of gold (g = the city's gold per turn x 100)")
print("gold/turn  old  new  new/old")
for gold in (5, 10, 20, 40, 60):
    g = gold * 100
    print(f"{gold:9d} {gold_term_old(g):4d} {gold_term_new(g):4d}  {gold_term_new(g) / gold_term_old(g):.2f}")
print("\ninternal route, hundredths of food or production by the destination owner's era")
print("era  old  new  new/old")
for era in (0, 1, 2, 4, 6):
    print(f"{era:3d} {internal_old(era):4d} {internal_new(era):4d}  {internal_new(era) / internal_old(era):.2f}")
