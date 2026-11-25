# Gate report: rl-s3 u175 vs MBf6c (rl-s3 u175 vs MBf6c)

**Verdict: FAIL**  (reference A = MBf6c, candidate B = rl-s3 u175; delta = B minus A; alpha 0.05, Holm across families)

| gate | result | what |
|---|---|---|
| harm | FAIL | no family dropped by more than 5 points with Holm-adjusted p < 0.05, over 35 families |
| required_up | pass | every required family up by more than 0 points with Holm-adjusted p < 0.05 (Holm over the required set) |
| mean | pass | mean over 35 families up: paired bootstrap 95 % CI lower bound above 0 points |
| count_at_bar | pass | families at >= 90 % of the best teacher, of 35: at least 0 more than the reference |

- Mean over 35 families: 78.9 -> 83.1, delta +4.2 points, paired bootstrap CI [+3.2, +5.3].
- Families at >= 90 % of the best teacher, of 35: 18 (CI 15-20) -> 21 (CI 19-24); difference +3, CI [+0, +7]. The bar is fixed at the point estimate of the best teacher.
- Families (harm set, 35): up 5, down 0 (of which beyond tolerance: DROP 1), unchanged 29. Benjamini-Hochberg at 0.05: 9 families differ.
- DROP: full/bait.
- Not shown to be within the tolerance (the 95 % CI of the delta reaches below -5 points; informational): 12 of 35: quick/focus, quick/prey-land, quick/promotion-choice, quick/rush, full/assault-peace, full/choke, full/general, full/harass, full/hold, full/kite, full/retreat, full/wounded.
- Required families: quick/planned-assault up, quick/assault-heal up.

## Families

b = plays A won and B lost, c = the reverse. p = exact McNemar (two-sided); Holm = adjusted across the harm set; p-worse is one-sided (H1: B worse). MDE = smallest drop the paired test finds at 80 % power at level alpha/m (Bonferroni bound of Holm), given the observed discordance; - = too few discordant plays to find any drop.

| family | n | A % | B % | delta (CI) | b / c | p | p Holm | p-worse Holm | MDE | status |
|---|---|---|---|---|---|---|---|---|---|---|
| full/bait | 127 | 96.1 | 84.3 | -11.8 [-18.7, -6.0] | 16 / 1 | <0.001 | 0.008 | 0.005 | 11.8 | DROP |
| full/assault-peace | 139 | 75.5 | 64.7 | -10.8 [-20.1, -1.2] | 31 / 16 | 0.040 | 0.920 | 0.660 | 18.9 | same |
| quick/rush | 157 | 90.4 | 82.2 | -8.3 [-14.0, -3.0] | 16 / 3 | 0.004 | 0.124 | 0.075 | 10.1 | same |
| full/rush | 157 | 90.4 | 82.2 | -8.3 [-14.0, -3.0] | 16 / 3 | 0.004 | - | - | 10.1 | same-as-quick/rush |
| full/general | 160 | 81.9 | 76.2 | -5.6 [-12.0, +0.7] | 18 / 9 | 0.122 | 1.000 | 1.000 | 12.1 | same |
| full/hold | 137 | 70.1 | 65.0 | -5.1 [-10.6, +0.4] | 11 / 4 | 0.118 | 1.000 | 1.000 | 10.2 | same |
| full/wounded | 148 | 93.9 | 89.2 | -4.7 [-10.1, +0.1] | 10 / 3 | 0.092 | 1.000 | 1.000 | 8.6 | same |
| full/retreat | 128 | 85.2 | 80.5 | -4.7 [-9.9, +0.2] | 8 / 2 | 0.109 | 1.000 | 1.000 | - | same |
| full/kite | 130 | 90.8 | 86.9 | -3.8 [-10.4, +2.5] | 11 / 6 | 0.332 | 1.000 | 1.000 | 11.5 | same |
| full/harass | 40 | 92.5 | 90.0 | -2.5 [-13.7, +7.9] | 2 / 1 | 1.000 | 1.000 | 1.000 | - | same |
| quick/focus | 160 | 95.6 | 93.8 | -1.9 [-6.2, +2.1] | 6 / 3 | 0.508 | 1.000 | 1.000 | - | same |
| full/focus | 160 | 95.6 | 93.8 | -1.9 [-6.2, +2.1] | 6 / 3 | 0.508 | - | - | - | same-as-quick/focus |
| quick/promotion-choice | 140 | 95.0 | 93.6 | -1.4 [-6.5, +3.4] | 6 / 4 | 0.754 | 1.000 | 1.000 | - | same |
| full/promotion-choice | 140 | 95.0 | 93.6 | -1.4 [-6.5, +3.4] | 6 / 4 | 0.754 | - | - | - | same-as-quick/promotion-choice |
| full/sink | 140 | 100.0 | 99.3 | -0.7 [-3.9, +2.0] | 1 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| quick/prey-land | 14 | 100.0 | 100.0 | +0.0 [-21.5, +21.5] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| quick/snipe | 140 | 100.0 | 100.0 | +0.0 [-2.7, +2.7] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/ambush-fog | 160 | 100.0 | 100.0 | +0.0 [-2.3, +2.3] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/convoy | 128 | 0.0 | 0.0 | +0.0 [-2.9, +2.9] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/prey-land | 14 | 100.0 | 100.0 | +0.0 [-21.5, +21.5] | 0 / 0 | 1.000 | - | - | - | same-as-quick/prey-land |
| full/snipe | 140 | 100.0 | 100.0 | +0.0 [-2.7, +2.7] | 0 / 0 | 1.000 | - | - | - | same-as-quick/snipe |
| full/surrounded | 160 | 96.2 | 96.2 | +0.0 [-2.2, +2.2] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/duel | 160 | 78.1 | 79.4 | +1.2 [-3.4, +6.0] | 6 / 8 | 0.791 | 1.000 | 1.000 | 8.3 | same |
| full/guard | 160 | 25.6 | 26.9 | +1.2 [-0.7, +3.3] | 0 / 2 | 0.500 | 1.000 | 1.000 | - | same |
| quick/prey-either | 146 | 96.6 | 97.9 | +1.4 [-1.9, +5.2] | 1 / 3 | 0.625 | 1.000 | 1.000 | - | same |
| full/prey-either | 146 | 96.6 | 97.9 | +1.4 [-1.9, +5.2] | 1 / 3 | 0.625 | - | - | - | same-as-quick/prey-either |
| full/choke | 128 | 75.0 | 77.3 | +2.3 [-5.0, +9.7] | 10 / 13 | 0.678 | 1.000 | 1.000 | 13.8 | same |
| quick/raid | 160 | 95.6 | 98.1 | +2.5 [-0.2, +6.2] | 0 / 4 | 0.125 | 1.000 | 1.000 | - | same |
| full/hunt-fog | 160 | 97.5 | 100.0 | +2.5 [-0.3, +6.3] | 0 / 4 | 0.125 | 1.000 | 1.000 | - | same |
| full/raid | 160 | 95.6 | 98.1 | +2.5 [-0.2, +6.2] | 0 / 4 | 0.125 | - | - | - | same-as-quick/raid |
| full/bombard | 59 | 93.2 | 98.3 | +5.1 [-1.3, +13.9] | 0 / 3 | 0.250 | 1.000 | 1.000 | - | same |
| full/flank | 160 | 93.1 | 99.4 | +6.2 [+2.8, +11.1] | 0 / 10 | 0.002 | 0.057 | 1.000 | - | same |
| full/counter | 133 | 76.7 | 85.0 | +8.3 [+1.6, +15.1] | 5 / 16 | 0.027 | 0.665 | 1.000 | 12.6 | same |
| quick/hill-archers | 140 | 66.4 | 77.9 | +11.4 [+1.6, +21.0] | 17 / 33 | 0.033 | 0.788 | 1.000 | 19.3 | same |
| full/hill-archers | 140 | 66.4 | 77.9 | +11.4 [+1.6, +21.0] | 17 / 33 | 0.033 | - | - | 19.3 | same-as-quick/hill-archers |
| quick/ram | 40 | 82.5 | 95.0 | +12.5 [+2.1, +25.5] | 0 / 5 | 0.062 | 1.000 | 1.000 | - | same |
| full/ram | 40 | 82.5 | 95.0 | +12.5 [+2.1, +25.5] | 0 / 5 | 0.062 | - | - | - | same-as-quick/ram |
| full/airstrike | 55 | 80.0 | 92.7 | +12.7 [+4.0, +23.3] | 0 / 7 | 0.016 | 0.406 | 1.000 | - | same |
| full/assault-reinforce | 138 | 60.9 | 76.1 | +15.2 [+5.1, +24.9] | 16 / 37 | 0.005 | 0.148 | 1.000 | 20.2 | same |
| quick/assault-heal | 140 | 78.6 | 94.3 | +15.7 [+8.8, +23.1] | 3 / 25 | <0.001 | <0.001 | 1.000 | 14.1 | up |
| full/assault-heal | 140 | 78.6 | 94.3 | +15.7 [+8.8, +23.1] | 3 / 25 | <0.001 | - | - | 14.1 | same-as-quick/assault-heal |
| quick/siege | 130 | 60.8 | 79.2 | +18.5 [+9.6, +27.0] | 7 / 31 | <0.001 | 0.004 | 1.000 | 18.0 | up |
| full/siege | 130 | 60.8 | 79.2 | +18.5 [+9.6, +27.0] | 7 / 31 | <0.001 | - | - | 18.0 | same-as-quick/siege |
| quick/planned-assault | 134 | 56.7 | 76.1 | +19.4 [+10.1, +28.2] | 9 / 35 | <0.001 | 0.003 | 1.000 | 18.9 | up |
| full/planned-assault | 134 | 56.7 | 76.1 | +19.4 [+10.1, +28.2] | 9 / 35 | <0.001 | - | - | 18.9 | same-as-quick/planned-assault |
| full/citadel | 120 | 35.8 | 60.8 | +25.0 [+14.0, +35.0] | 10 / 40 | <0.001 | <0.001 | 1.000 | 22.5 | up |
| quick/city-fire | 140 | 44.3 | 92.9 | +48.6 [+39.6, +56.6] | 1 / 69 | <0.001 | <0.001 | 1.000 | 22.9 | up |
| full/city-fire | 140 | 44.3 | 92.9 | +48.6 [+39.6, +56.6] | 1 / 69 | <0.001 | - | - | 22.9 | same-as-quick/city-fire |
