# Gate report: arm-swiglu vs arm-base (arm-swiglu vs arm-base)

**Verdict: PASS**  (reference A = arm-base, candidate B = arm-swiglu; delta = B minus A; alpha 0.05, Holm across families)

| gate | result | what |
|---|---|---|
| harm | pass | no family dropped by more than 5 points with Holm-adjusted p < 0.05, over 10 families |
| mean | pass | mean over 10 families up: paired bootstrap 95 % CI lower bound above 0 points |

- Mean over 10 families: 83.4 -> 86.3, delta +2.9 points, paired bootstrap CI [+1.3, +4.5].
- Families (harm set, 10): up 1, down 0 (of which beyond tolerance: DROP 0), unchanged 9. Benjamini-Hochberg at 0.05: 2 families differ.
- Seeds disagree on: twin/assault-heal.
- Not shown to be within the tolerance (the 95 % CI of the delta reaches below -5 points; informational): 2 of 10: twin/hill-archers, twin/ram.

## Families

b = plays A won and B lost, c = the reverse. p = exact McNemar (two-sided); Holm = adjusted across the harm set; p-worse is one-sided (H1: B worse). MDE = smallest drop the paired test finds at 80 % power at level alpha/m (Bonferroni bound of Holm), given the observed discordance; - = too few discordant plays to find any drop.

| family | n | A % | B % | delta (CI) | b / c | p | p Holm | p-worse Holm | MDE | status | seeds (delta per seed) |
|---|---|---|---|---|---|---|---|---|---|---|---|
| twin/ram | 80 | 96.2 | 92.5 | -3.8 [-10.9, +2.3] | 4 / 1 | 0.375 | 1.000 | 1.000 | - | same | +0.0, -7.5 |
| twin/hill-archers | 280 | 80.7 | 78.6 | -2.1 [-7.8, +3.5] | 35 / 29 | 0.532 | 1.000 | 1.000 | 9.9 | same | -0.7, -3.6 |
| twin/promotion-choice | 280 | 92.1 | 92.5 | +0.4 [-3.7, +4.4] | 15 / 16 | 1.000 | 1.000 | 1.000 | 6.8 | same | +0.7, +0.0 |
| twin/siege | 260 | 61.5 | 63.1 | +1.5 [-4.7, +7.8] | 33 / 37 | 0.720 | 1.000 | 1.000 | 11.1 | same | +3.1, +0.0 |
| twin/raid | 320 | 90.9 | 93.8 | +2.8 [-0.9, +6.6] | 13 / 22 | 0.175 | 0.877 | 1.000 | 6.4 | same | +2.5, +3.1 |
| twin/city-fire | 280 | 92.1 | 95.7 | +3.6 [+0.4, +7.0] | 5 / 15 | 0.041 | 0.331 | 1.000 | 5.4 | same | +3.6, +3.6 |
| twin/prey-either | 320 | 91.9 | 95.6 | +3.8 [-0.2, +7.8] | 14 / 26 | 0.081 | 0.565 | 1.000 | 6.8 | same | +2.5, +5.0 |
| twin/assault-heal | 280 | 84.3 | 88.6 | +4.3 [-0.3, +8.9] | 15 / 27 | 0.088 | 0.565 | 1.000 | 8.0 | same (seeds disagree) | +9.3, -0.7 |
| twin/bombard | 118 | 88.1 | 97.5 | +9.3 [+3.7, +16.1] | 1 / 12 | 0.003 | 0.034 | 1.000 | 10.0 | up | +11.9, +6.8 |
| twin/planned-assault | 268 | 56.3 | 65.7 | +9.3 [+2.9, +15.6] | 27 / 52 | 0.007 | 0.059 | 1.000 | 11.5 | same | +13.4, +5.2 |
