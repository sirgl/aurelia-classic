# Gate report: arm-L6 vs arm-base (arm-L6 vs arm-base)

**Verdict: FAIL**  (reference A = arm-base, candidate B = arm-L6; delta = B minus A; alpha 0.05, Holm across families)

| gate | result | what |
|---|---|---|
| harm | pass | no family dropped by more than 5 points with Holm-adjusted p < 0.05, over 10 families |
| mean | FAIL | mean over 10 families up: paired bootstrap 95 % CI lower bound above 0 points |

- Mean over 10 families: 81.6 -> 83.8, delta +2.2 points, paired bootstrap CI [-0.1, +4.6].
- Families (harm set, 10): up 0, down 0 (of which beyond tolerance: DROP 0), unchanged 10. Benjamini-Hochberg at 0.05: 0 families differ.
- Not shown to be within the tolerance (the 95 % CI of the delta reaches below -5 points; informational): 3 of 10: twin/promotion-choice, twin/raid, twin/ram.

## Families

b = plays A won and B lost, c = the reverse. p = exact McNemar (two-sided); Holm = adjusted across the harm set; p-worse is one-sided (H1: B worse). MDE = smallest drop the paired test finds at 80 % power at level alpha/m (Bonferroni bound of Holm), given the observed discordance; - = too few discordant plays to find any drop.

| family | n | A % | B % | delta (CI) | b / c | p | p Holm | p-worse Holm | MDE | status |
|---|---|---|---|---|---|---|---|---|---|---|
| twin/ram | 40 | 95.0 | 85.0 | -10.0 [-22.5, -0.2] | 4 / 0 | 0.125 | 0.923 | 0.577 | - | same |
| twin/promotion-choice | 140 | 92.9 | 87.1 | -5.7 [-12.3, +0.6] | 14 / 6 | 0.115 | 0.923 | 0.577 | 10.8 | same |
| twin/raid | 160 | 90.6 | 85.6 | -5.0 [-11.5, +1.3] | 17 / 9 | 0.169 | 1.000 | 0.675 | 10.9 | same |
| twin/city-fire | 140 | 92.1 | 92.9 | +0.7 [-4.7, +6.2] | 6 / 7 | 1.000 | 1.000 | 1.000 | 8.5 | same |
| twin/prey-either | 160 | 90.6 | 93.1 | +2.5 [-3.7, +8.8] | 10 / 14 | 0.541 | 1.000 | 1.000 | 10.4 | same |
| twin/hill-archers | 140 | 80.0 | 85.0 | +5.0 [-1.7, +11.9] | 8 / 15 | 0.210 | 1.000 | 1.000 | 11.7 | same |
| twin/assault-heal | 140 | 78.6 | 84.3 | +5.7 [-1.7, +13.2] | 10 / 18 | 0.185 | 1.000 | 1.000 | 12.9 | same |
| twin/siege | 130 | 60.0 | 66.2 | +6.2 [-3.3, +15.4] | 16 / 24 | 0.268 | 1.000 | 1.000 | 16.7 | same |
| twin/bombard | 59 | 84.7 | 94.9 | +10.2 [+2.4, +20.0] | 0 / 6 | 0.031 | 0.281 | 1.000 | - | same |
| twin/planned-assault | 134 | 51.5 | 64.2 | +12.7 [+3.7, +21.3] | 11 / 28 | 0.009 | 0.095 | 1.000 | 16.0 | same |
