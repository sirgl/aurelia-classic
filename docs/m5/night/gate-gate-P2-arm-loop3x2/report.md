# Gate report: arm-loop3x2 vs arm-base (arm-loop3x2 vs arm-base)

**Verdict: FAIL**  (reference A = arm-base, candidate B = arm-loop3x2; delta = B minus A; alpha 0.05, Holm across families)

| gate | result | what |
|---|---|---|
| harm | pass | no family dropped by more than 5 points with Holm-adjusted p < 0.05, over 10 families |
| mean | FAIL | mean over 10 families up: paired bootstrap 95 % CI lower bound above 0 points |

- Mean over 10 families: 81.6 -> 83.4, delta +1.8 points, paired bootstrap CI [-0.5, +3.9].
- Families (harm set, 10): up 0, down 0 (of which beyond tolerance: DROP 0), unchanged 10. Benjamini-Hochberg at 0.05: 0 families differ.
- Not shown to be within the tolerance (the 95 % CI of the delta reaches below -5 points; informational): 6 of 10: twin/hill-archers, twin/planned-assault, twin/promotion-choice, twin/raid, twin/ram, twin/siege.

## Families

b = plays A won and B lost, c = the reverse. p = exact McNemar (two-sided); Holm = adjusted across the harm set; p-worse is one-sided (H1: B worse). MDE = smallest drop the paired test finds at 80 % power at level alpha/m (Bonferroni bound of Holm), given the observed discordance; - = too few discordant plays to find any drop.

| family | n | A % | B % | delta (CI) | b / c | p | p Holm | p-worse Holm | MDE | status |
|---|---|---|---|---|---|---|---|---|---|---|
| twin/raid | 160 | 90.6 | 88.1 | -2.5 [-8.5, +3.4] | 13 / 9 | 0.523 | 1.000 | 1.000 | 10.0 | same |
| twin/hill-archers | 140 | 80.0 | 78.6 | -1.4 [-8.9, +6.0] | 15 / 13 | 0.851 | 1.000 | 1.000 | 12.9 | same |
| twin/promotion-choice | 140 | 92.9 | 92.1 | -0.7 [-5.9, +4.3] | 6 / 5 | 1.000 | 1.000 | 1.000 | 7.6 | same |
| twin/ram | 40 | 95.0 | 95.0 | +0.0 [-7.9, +7.9] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| twin/siege | 130 | 60.0 | 60.8 | +0.8 [-8.5, +10.1] | 19 / 20 | 1.000 | 1.000 | 1.000 | 16.5 | same |
| twin/city-fire | 140 | 92.1 | 94.3 | +2.1 [-3.2, +7.7] | 5 / 8 | 0.581 | 1.000 | 1.000 | 8.5 | same |
| twin/planned-assault | 134 | 51.5 | 53.7 | +2.2 [-6.8, +11.2] | 18 / 21 | 0.749 | 1.000 | 1.000 | 16.0 | same |
| twin/prey-either | 160 | 90.6 | 95.6 | +5.0 [-0.6, +10.9] | 6 / 14 | 0.115 | 1.000 | 1.000 | 9.5 | same |
| twin/assault-heal | 140 | 78.6 | 84.3 | +5.7 [-1.4, +12.9] | 9 / 17 | 0.169 | 1.000 | 1.000 | 12.4 | same |
| twin/bombard | 59 | 84.7 | 91.5 | +6.8 [-2.9, +17.2] | 2 / 6 | 0.289 | 1.000 | 1.000 | - | same |
