# Gate report: arm-rms-qk vs arm-base (arm-rms-qk vs arm-base)

**Verdict: PASS**  (reference A = arm-base, candidate B = arm-rms-qk; delta = B minus A; alpha 0.05, Holm across families)

| gate | result | what |
|---|---|---|
| harm | pass | no family dropped by more than 5 points with Holm-adjusted p < 0.05, over 10 families |
| mean | pass | mean over 10 families up: paired bootstrap 95 % CI lower bound above 0 points |

- Mean over 10 families: 83.4 -> 87.5, delta +4.1 points, paired bootstrap CI [+2.6, +5.7].
- Families (harm set, 10): up 2, down 0 (of which beyond tolerance: DROP 0), unchanged 8. Benjamini-Hochberg at 0.05: 3 families differ.
- Seeds disagree on: twin/assault-heal.
- Not shown to be within the tolerance (the 95 % CI of the delta reaches below -5 points; informational): 1 of 10: twin/ram.

## Families

b = plays A won and B lost, c = the reverse. p = exact McNemar (two-sided); Holm = adjusted across the harm set; p-worse is one-sided (H1: B worse). MDE = smallest drop the paired test finds at 80 % power at level alpha/m (Bonferroni bound of Holm), given the observed discordance; - = too few discordant plays to find any drop.

| family | n | A % | B % | delta (CI) | b / c | p | p Holm | p-worse Holm | MDE | status | seeds (delta per seed) |
|---|---|---|---|---|---|---|---|---|---|---|---|
| twin/ram | 80 | 96.2 | 93.8 | -2.5 [-9.2, +3.3] | 3 / 1 | 0.625 | 1.000 | 1.000 | - | same | -2.5, -2.5 |
| twin/promotion-choice | 280 | 92.1 | 91.8 | -0.4 [-4.4, +3.7] | 16 / 15 | 1.000 | 1.000 | 1.000 | 6.8 | same | +2.1, -2.9 |
| twin/assault-heal | 280 | 84.3 | 85.7 | +1.4 [-3.3, +6.1] | 20 / 24 | 0.652 | 1.000 | 1.000 | 8.2 | same (seeds disagree) | +10.0, -7.1 |
| twin/hill-archers | 280 | 80.7 | 83.9 | +3.2 [-1.8, +8.3] | 21 / 30 | 0.262 | 1.000 | 1.000 | 8.8 | same | +4.3, +2.1 |
| twin/prey-either | 320 | 91.9 | 96.2 | +4.4 [+0.6, +8.3] | 12 / 26 | 0.034 | 0.201 | 1.000 | 6.6 | same | +8.1, +0.6 |
| twin/raid | 320 | 90.9 | 95.3 | +4.4 [+0.9, +8.0] | 9 / 23 | 0.020 | 0.140 | 1.000 | 6.1 | same | +3.1, +5.6 |
| twin/city-fire | 280 | 92.1 | 97.1 | +5.0 [+2.2, +8.4] | 2 / 16 | 0.001 | 0.013 | 1.000 | 5.1 | up | +5.0, +5.0 |
| twin/siege | 260 | 61.5 | 68.5 | +6.9 [+0.1, +13.6] | 32 / 50 | 0.060 | 0.299 | 1.000 | 12.1 | same | +5.4, +8.5 |
| twin/bombard | 118 | 88.1 | 96.6 | +8.5 [+3.0, +15.1] | 1 / 11 | 0.006 | 0.051 | 1.000 | 9.5 | same | +10.2, +6.8 |
| twin/planned-assault | 268 | 56.3 | 66.4 | +10.1 [+3.5, +16.5] | 28 / 55 | 0.004 | 0.036 | 1.000 | 11.8 | up | +16.4, +3.7 |
