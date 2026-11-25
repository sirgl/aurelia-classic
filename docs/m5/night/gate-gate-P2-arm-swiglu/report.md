# Gate report: arm-swiglu vs arm-base (arm-swiglu vs arm-base)

**Verdict: PASS**  (reference A = arm-base, candidate B = arm-swiglu; delta = B minus A; alpha 0.05, Holm across families)

| gate | result | what |
|---|---|---|
| harm | pass | no family dropped by more than 5 points with Holm-adjusted p < 0.05, over 10 families |
| mean | pass | mean over 10 families up: paired bootstrap 95 % CI lower bound above 0 points |

- Mean over 10 families: 81.6 -> 86.2, delta +4.6 points, paired bootstrap CI [+2.4, +6.8].
- Families (harm set, 10): up 0, down 0 (of which beyond tolerance: DROP 0), unchanged 10. Benjamini-Hochberg at 0.05: 0 families differ.
- Not shown to be within the tolerance (the 95 % CI of the delta reaches below -5 points; informational): 3 of 10: twin/hill-archers, twin/ram, twin/siege.

## Families

b = plays A won and B lost, c = the reverse. p = exact McNemar (two-sided); Holm = adjusted across the harm set; p-worse is one-sided (H1: B worse). MDE = smallest drop the paired test finds at 80 % power at level alpha/m (Bonferroni bound of Holm), given the observed discordance; - = too few discordant plays to find any drop.

| family | n | A % | B % | delta (CI) | b / c | p | p Holm | p-worse Holm | MDE | status |
|---|---|---|---|---|---|---|---|---|---|---|
| twin/hill-archers | 140 | 80.0 | 79.3 | -0.7 [-8.3, +6.9] | 15 / 14 | 1.000 | 1.000 | 1.000 | 13.2 | same |
| twin/ram | 40 | 95.0 | 95.0 | +0.0 [-10.3, +10.3] | 1 / 1 | 1.000 | 1.000 | 1.000 | - | same |
| twin/promotion-choice | 140 | 92.9 | 93.6 | +0.7 [-4.7, +6.2] | 6 / 7 | 1.000 | 1.000 | 1.000 | 8.5 | same |
| twin/prey-either | 160 | 90.6 | 93.1 | +2.5 [-3.9, +9.0] | 11 / 15 | 0.557 | 1.000 | 1.000 | 10.9 | same |
| twin/raid | 160 | 90.6 | 93.1 | +2.5 [-3.2, +8.3] | 8 / 12 | 0.503 | 1.000 | 1.000 | 9.5 | same |
| twin/siege | 130 | 60.0 | 63.1 | +3.1 [-6.6, +12.7] | 19 / 23 | 0.644 | 1.000 | 1.000 | 17.1 | same |
| twin/city-fire | 140 | 92.1 | 95.7 | +3.6 [-1.7, +9.2] | 4 / 9 | 0.267 | 1.000 | 1.000 | 8.5 | same |
| twin/assault-heal | 140 | 78.6 | 87.9 | +9.3 [+2.4, +16.4] | 6 / 19 | 0.015 | 0.132 | 1.000 | 12.2 | same |
| twin/bombard | 59 | 84.7 | 96.6 | +11.9 [+3.8, +22.3] | 0 / 7 | 0.016 | 0.132 | 1.000 | - | same |
| twin/planned-assault | 134 | 51.5 | 64.9 | +13.4 [+3.9, +22.5] | 13 / 31 | 0.010 | 0.096 | 1.000 | 17.0 | same |
