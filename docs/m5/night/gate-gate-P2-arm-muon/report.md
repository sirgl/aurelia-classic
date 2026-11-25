# Gate report: arm-muon vs arm-base (arm-muon vs arm-base)

**Verdict: FAIL**  (reference A = arm-base, candidate B = arm-muon; delta = B minus A; alpha 0.05, Holm across families)

| gate | result | what |
|---|---|---|
| harm | pass | no family dropped by more than 5 points with Holm-adjusted p < 0.05, over 10 families |
| mean | FAIL | mean over 10 families up: paired bootstrap 95 % CI lower bound above 0 points |

- Mean over 10 families: 81.6 -> 83.5, delta +1.9 points, paired bootstrap CI [-0.4, +4.2].
- Families (harm set, 10): up 0, down 0 (of which beyond tolerance: DROP 0), unchanged 10. Benjamini-Hochberg at 0.05: 0 families differ.
- Not shown to be within the tolerance (the 95 % CI of the delta reaches below -5 points; informational): 4 of 10: twin/city-fire, twin/promotion-choice, twin/raid, twin/ram.

## Families

b = plays A won and B lost, c = the reverse. p = exact McNemar (two-sided); Holm = adjusted across the harm set; p-worse is one-sided (H1: B worse). MDE = smallest drop the paired test finds at 80 % power at level alpha/m (Bonferroni bound of Holm), given the observed discordance; - = too few discordant plays to find any drop.

| family | n | A % | B % | delta (CI) | b / c | p | p Holm | p-worse Holm | MDE | status |
|---|---|---|---|---|---|---|---|---|---|---|
| twin/promotion-choice | 140 | 92.9 | 83.6 | -9.3 [-16.2, -2.7] | 18 / 5 | 0.011 | 0.106 | 0.053 | 11.7 | same |
| twin/raid | 160 | 90.6 | 86.9 | -3.8 [-9.8, +2.1] | 14 / 8 | 0.286 | 1.000 | 1.000 | 10.0 | same |
| twin/city-fire | 140 | 92.1 | 89.3 | -2.9 [-9.5, +3.6] | 12 / 8 | 0.503 | 1.000 | 1.000 | 10.8 | same |
| twin/ram | 40 | 95.0 | 92.5 | -2.5 [-12.2, +5.6] | 1 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| twin/hill-archers | 140 | 80.0 | 82.1 | +2.1 [-4.9, +9.3] | 11 / 14 | 0.690 | 1.000 | 1.000 | 12.2 | same |
| twin/prey-either | 160 | 90.6 | 93.1 | +2.5 [-3.4, +8.6] | 9 / 13 | 0.523 | 1.000 | 1.000 | 10.0 | same |
| twin/assault-heal | 140 | 78.6 | 83.6 | +5.0 [-2.8, +12.8] | 12 / 19 | 0.281 | 1.000 | 1.000 | 13.6 | same |
| twin/bombard | 59 | 84.7 | 93.2 | +8.5 [-0.4, +18.6] | 1 / 6 | 0.125 | 0.875 | 1.000 | - | same |
| twin/planned-assault | 134 | 51.5 | 61.2 | +9.7 [+0.2, +18.9] | 15 / 28 | 0.066 | 0.528 | 1.000 | 16.8 | same |
| twin/siege | 130 | 60.0 | 70.0 | +10.0 [+0.7, +19.0] | 13 / 26 | 0.053 | 0.479 | 1.000 | 16.5 | same |
