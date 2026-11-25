# Gate report: arm-gate vs arm-base (arm-gate vs arm-base)

**Verdict: PASS**  (reference A = arm-base, candidate B = arm-gate; delta = B minus A; alpha 0.05, Holm across families)

| gate | result | what |
|---|---|---|
| harm | pass | no family dropped by more than 5 points with Holm-adjusted p < 0.05, over 10 families |
| mean | pass | mean over 10 families up: paired bootstrap 95 % CI lower bound above 0 points |

- Mean over 10 families: 81.6 -> 85.1, delta +3.5 points, paired bootstrap CI [+1.3, +5.7].
- Families (harm set, 10): up 2, down 0 (of which beyond tolerance: DROP 0), unchanged 8. Benjamini-Hochberg at 0.05: 2 families differ.
- Not shown to be within the tolerance (the 95 % CI of the delta reaches below -5 points; informational): 5 of 10: twin/hill-archers, twin/promotion-choice, twin/raid, twin/ram, twin/siege.

## Families

b = plays A won and B lost, c = the reverse. p = exact McNemar (two-sided); Holm = adjusted across the harm set; p-worse is one-sided (H1: B worse). MDE = smallest drop the paired test finds at 80 % power at level alpha/m (Bonferroni bound of Holm), given the observed discordance; - = too few discordant plays to find any drop.

| family | n | A % | B % | delta (CI) | b / c | p | p Holm | p-worse Holm | MDE | status |
|---|---|---|---|---|---|---|---|---|---|---|
| twin/promotion-choice | 140 | 92.9 | 87.9 | -5.0 [-10.9, +0.5] | 11 / 4 | 0.118 | 0.711 | 0.592 | 9.2 | same |
| twin/ram | 40 | 95.0 | 92.5 | -2.5 [-12.2, +5.6] | 1 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| twin/hill-archers | 140 | 80.0 | 77.9 | -2.1 [-10.0, +5.7] | 17 / 14 | 0.720 | 1.000 | 1.000 | 13.6 | same |
| twin/raid | 160 | 90.6 | 89.4 | -1.2 [-7.0, +4.4] | 11 / 9 | 0.824 | 1.000 | 1.000 | 9.5 | same |
| twin/siege | 130 | 60.0 | 60.0 | +0.0 [-9.9, +9.9] | 22 / 22 | 1.000 | 1.000 | 1.000 | 17.5 | same |
| twin/city-fire | 140 | 92.1 | 95.0 | +2.9 [-2.3, +8.3] | 4 / 8 | 0.388 | 1.000 | 1.000 | 8.0 | same |
| twin/prey-either | 160 | 90.6 | 97.5 | +6.9 [+1.5, +12.7] | 4 / 15 | 0.019 | 0.154 | 1.000 | 9.2 | same |
| twin/assault-heal | 140 | 78.6 | 88.6 | +10.0 [+3.6, +16.7] | 4 / 18 | 0.004 | 0.039 | 1.000 | 11.4 | up |
| twin/planned-assault | 134 | 51.5 | 61.9 | +10.4 [+1.8, +18.8] | 11 / 25 | 0.029 | 0.202 | 1.000 | 15.4 | same |
| twin/bombard | 59 | 84.7 | 100.0 | +15.3 [+5.9, +26.5] | 0 / 9 | 0.004 | 0.039 | 1.000 | - | up |
