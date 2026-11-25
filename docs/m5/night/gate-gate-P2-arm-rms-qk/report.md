# Gate report: arm-rms-qk vs arm-base (arm-rms-qk vs arm-base)

**Verdict: PASS**  (reference A = arm-base, candidate B = arm-rms-qk; delta = B minus A; alpha 0.05, Holm across families)

| gate | result | what |
|---|---|---|
| harm | pass | no family dropped by more than 5 points with Holm-adjusted p < 0.05, over 10 families |
| mean | pass | mean over 10 families up: paired bootstrap 95 % CI lower bound above 0 points |

- Mean over 10 families: 81.6 -> 87.8, delta +6.2 points, paired bootstrap CI [+4.0, +8.5].
- Families (harm set, 10): up 2, down 0 (of which beyond tolerance: DROP 0), unchanged 8. Benjamini-Hochberg at 0.05: 3 families differ.
- Not shown to be within the tolerance (the 95 % CI of the delta reaches below -5 points; informational): 1 of 10: twin/ram.

## Families

b = plays A won and B lost, c = the reverse. p = exact McNemar (two-sided); Holm = adjusted across the harm set; p-worse is one-sided (H1: B worse). MDE = smallest drop the paired test finds at 80 % power at level alpha/m (Bonferroni bound of Holm), given the observed discordance; - = too few discordant plays to find any drop.

| family | n | A % | B % | delta (CI) | b / c | p | p Holm | p-worse Holm | MDE | status |
|---|---|---|---|---|---|---|---|---|---|---|
| twin/ram | 40 | 95.0 | 92.5 | -2.5 [-14.0, +8.2] | 2 / 1 | 1.000 | 1.000 | 1.000 | - | same |
| twin/promotion-choice | 140 | 92.9 | 95.0 | +2.1 [-3.2, +7.8] | 5 / 8 | 0.581 | 1.000 | 1.000 | 8.5 | same |
| twin/raid | 160 | 90.6 | 93.8 | +3.1 [-1.8, +8.3] | 5 / 10 | 0.302 | 1.000 | 1.000 | 8.1 | same |
| twin/hill-archers | 140 | 80.0 | 84.3 | +4.3 [-2.6, +11.3] | 9 / 15 | 0.307 | 1.000 | 1.000 | 11.9 | same |
| twin/city-fire | 140 | 92.1 | 97.1 | +5.0 [+0.8, +10.2] | 1 / 8 | 0.039 | 0.234 | 1.000 | - | same |
| twin/siege | 130 | 60.0 | 65.4 | +5.4 [-4.4, +15.0] | 18 / 25 | 0.360 | 1.000 | 1.000 | 17.3 | same |
| twin/prey-either | 160 | 90.6 | 98.8 | +8.1 [+3.2, +13.7] | 2 / 15 | 0.002 | 0.021 | 1.000 | 8.6 | up |
| twin/assault-heal | 140 | 78.6 | 88.6 | +10.0 [+3.3, +16.9] | 5 / 19 | 0.007 | 0.053 | 1.000 | 11.9 | same |
| twin/bombard | 59 | 84.7 | 94.9 | +10.2 [+2.4, +20.0] | 0 / 6 | 0.031 | 0.219 | 1.000 | - | same |
| twin/planned-assault | 134 | 51.5 | 67.9 | +16.4 [+7.0, +25.4] | 11 / 33 | 0.001 | 0.013 | 1.000 | 17.0 | up |
