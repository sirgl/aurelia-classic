# Gate report: rl-s3 u175 vs rl2 u175 (seed 1) (rl-s3 u175 vs rl2 u175 (seed 1))

**Verdict: FAIL**  (reference A = rl2 u175 (seed 1), candidate B = rl-s3 u175; delta = B minus A; alpha 0.05, Holm across families)

| gate | result | what |
|---|---|---|
| harm | FAIL | no family dropped by more than 5 points with Holm-adjusted p < 0.05, over 35 families |
| mean | FAIL | mean over 35 families up: paired bootstrap 95 % CI lower bound above 0 points |
| count_at_bar | pass | families at >= 90 % of the best teacher, of 35: at least 0 more than the reference |

- Mean over 35 families: 83.5 -> 83.1, delta -0.3 points, paired bootstrap CI [-1.2, +0.5].
- Families at >= 90 % of the best teacher, of 35: 21 (CI 19-23) -> 21 (CI 19-24); difference +0, CI [-3, +3]. The bar is fixed at the point estimate of the best teacher.
- Families (harm set, 35): up 0, down 0 (of which beyond tolerance: DROP 2), unchanged 33. Benjamini-Hochberg at 0.05: 2 families differ.
- DROP: quick/rush, quick/siege.
- Not shown to be within the tolerance (the 95 % CI of the delta reaches below -5 points; informational): 15 of 35: quick/assault-heal, quick/city-fire, quick/focus, quick/hill-archers, quick/planned-assault, quick/prey-either, quick/prey-land, quick/promotion-choice, quick/ram, full/bait, full/citadel, full/general, full/harass, full/hold, full/kite.

## Families

b = plays A won and B lost, c = the reverse. p = exact McNemar (two-sided); Holm = adjusted across the harm set; p-worse is one-sided (H1: B worse). MDE = smallest drop the paired test finds at 80 % power at level alpha/m (Bonferroni bound of Holm), given the observed discordance; - = too few discordant plays to find any drop.

| family | n | A % | B % | delta (CI) | b / c | p | p Holm | p-worse Holm | MDE | status |
|---|---|---|---|---|---|---|---|---|---|---|
| quick/siege | 130 | 92.3 | 79.2 | -13.1 [-20.6, -5.9] | 21 / 4 | <0.001 | 0.032 | 0.016 | 14.2 | DROP |
| full/siege | 130 | 92.3 | 79.2 | -13.1 [-20.6, -5.9] | 21 / 4 | <0.001 | - | - | 14.2 | same-as-quick/siege |
| quick/rush | 157 | 90.4 | 82.2 | -8.3 [-13.8, -3.3] | 15 / 2 | 0.002 | 0.080 | 0.040 | 9.5 | DROP |
| full/rush | 157 | 90.4 | 82.2 | -8.3 [-13.8, -3.3] | 15 / 2 | 0.002 | - | - | 9.5 | same-as-quick/rush |
| full/bait | 127 | 92.1 | 84.3 | -7.9 [-13.8, -2.7] | 11 / 1 | 0.006 | 0.209 | 0.105 | - | same |
| quick/city-fire | 140 | 98.6 | 92.9 | -5.7 [-11.4, -0.8] | 10 / 2 | 0.039 | 1.000 | 0.598 | - | same |
| quick/promotion-choice | 140 | 99.3 | 93.6 | -5.7 [-10.9, -2.1] | 8 / 0 | 0.008 | 0.250 | 0.125 | - | same |
| full/city-fire | 140 | 98.6 | 92.9 | -5.7 [-11.4, -0.8] | 10 / 2 | 0.039 | - | - | - | same-as-quick/city-fire |
| full/promotion-choice | 140 | 99.3 | 93.6 | -5.7 [-10.9, -2.1] | 8 / 0 | 0.008 | - | - | - | same-as-quick/promotion-choice |
| quick/hill-archers | 140 | 82.9 | 77.9 | -5.0 [-12.3, +2.3] | 17 / 10 | 0.248 | 1.000 | 1.000 | 13.8 | same |
| full/hill-archers | 140 | 82.9 | 77.9 | -5.0 [-12.3, +2.3] | 17 / 10 | 0.248 | - | - | 13.8 | same-as-quick/hill-archers |
| full/kite | 130 | 90.0 | 86.9 | -3.1 [-9.5, +3.1] | 10 / 6 | 0.454 | 1.000 | 1.000 | 11.1 | same |
| quick/focus | 160 | 96.2 | 93.8 | -2.5 [-6.4, +0.8] | 5 / 1 | 0.219 | 1.000 | 1.000 | - | same |
| full/focus | 160 | 96.2 | 93.8 | -2.5 [-6.4, +0.8] | 5 / 1 | 0.219 | - | - | - | same-as-quick/focus |
| full/harass | 40 | 92.5 | 90.0 | -2.5 [-11.7, +5.4] | 1 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/hold | 137 | 67.2 | 65.0 | -2.2 [-7.3, +3.0] | 8 / 5 | 0.581 | 1.000 | 1.000 | 9.3 | same |
| quick/planned-assault | 134 | 77.6 | 76.1 | -1.5 [-9.0, +6.0] | 14 / 12 | 0.845 | 1.000 | 1.000 | 14.1 | same |
| full/planned-assault | 134 | 77.6 | 76.1 | -1.5 [-9.0, +6.0] | 14 / 12 | 0.845 | - | - | 14.1 | same-as-quick/planned-assault |
| quick/assault-heal | 140 | 95.7 | 94.3 | -1.4 [-6.2, +3.0] | 5 / 3 | 0.727 | 1.000 | 1.000 | - | same |
| full/assault-heal | 140 | 95.7 | 94.3 | -1.4 [-6.2, +3.0] | 5 / 3 | 0.727 | - | - | - | same-as-quick/assault-heal |
| quick/prey-either | 146 | 99.3 | 97.9 | -1.4 [-5.2, +2.0] | 3 / 1 | 0.625 | 1.000 | 1.000 | - | same |
| full/prey-either | 146 | 99.3 | 97.9 | -1.4 [-5.2, +2.0] | 3 / 1 | 0.625 | - | - | - | same-as-quick/prey-either |
| full/general | 160 | 77.5 | 76.2 | -1.2 [-7.6, +5.0] | 14 / 12 | 0.845 | 1.000 | 1.000 | 11.8 | same |
| full/guard | 160 | 28.1 | 26.9 | -1.2 [-3.3, +0.7] | 2 / 0 | 0.500 | 1.000 | 1.000 | - | same |
| full/sink | 140 | 100.0 | 99.3 | -0.7 [-3.9, +2.0] | 1 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| quick/prey-land | 14 | 100.0 | 100.0 | +0.0 [-21.5, +21.5] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| quick/raid | 160 | 98.1 | 98.1 | +0.0 [-2.3, +2.3] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| quick/ram | 40 | 95.0 | 95.0 | +0.0 [-7.9, +7.9] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| quick/snipe | 140 | 100.0 | 100.0 | +0.0 [-2.7, +2.7] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/convoy | 128 | 0.0 | 0.0 | +0.0 [-2.9, +2.9] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/prey-land | 14 | 100.0 | 100.0 | +0.0 [-21.5, +21.5] | 0 / 0 | 1.000 | - | - | - | same-as-quick/prey-land |
| full/raid | 160 | 98.1 | 98.1 | +0.0 [-2.3, +2.3] | 0 / 0 | 1.000 | - | - | - | same-as-quick/raid |
| full/ram | 40 | 95.0 | 95.0 | +0.0 [-7.9, +7.9] | 0 / 0 | 1.000 | - | - | - | same-as-quick/ram |
| full/retreat | 128 | 80.5 | 80.5 | +0.0 [-4.6, +4.6] | 4 / 4 | 1.000 | 1.000 | 1.000 | - | same |
| full/snipe | 140 | 100.0 | 100.0 | +0.0 [-2.7, +2.7] | 0 / 0 | 1.000 | - | - | - | same-as-quick/snipe |
| full/surrounded | 160 | 96.2 | 96.2 | +0.0 [-2.2, +2.2] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/citadel | 120 | 59.2 | 60.8 | +1.7 [-5.2, +8.5] | 8 / 10 | 0.815 | 1.000 | 1.000 | 12.9 | same |
| full/ambush-fog | 160 | 97.5 | 100.0 | +2.5 [-0.3, +6.3] | 0 / 4 | 0.125 | 1.000 | 1.000 | - | same |
| full/counter | 133 | 82.0 | 85.0 | +3.0 [-2.2, +8.5] | 4 / 8 | 0.388 | 1.000 | 1.000 | - | same |
| full/flank | 160 | 96.2 | 99.4 | +3.1 [+0.4, +7.1] | 0 / 5 | 0.062 | 1.000 | 1.000 | - | same |
| full/wounded | 148 | 85.8 | 89.2 | +3.4 [-1.9, +8.9] | 5 / 10 | 0.302 | 1.000 | 1.000 | 9.4 | same |
| full/bombard | 59 | 94.9 | 98.3 | +3.4 [-2.7, +11.5] | 0 / 2 | 0.500 | 1.000 | 1.000 | - | same |
| full/airstrike | 55 | 89.1 | 92.7 | +3.6 [-4.5, +12.8] | 1 / 3 | 0.625 | 1.000 | 1.000 | - | same |
| full/hunt-fog | 160 | 96.2 | 100.0 | +3.8 [+0.7, +7.9] | 0 / 6 | 0.031 | 0.969 | 1.000 | - | same |
| full/assault-reinforce | 138 | 71.7 | 76.1 | +4.3 [-2.9, +11.6] | 10 / 16 | 0.327 | 1.000 | 1.000 | 13.7 | same |
| full/duel | 160 | 75.0 | 79.4 | +4.4 [-0.0, +8.9] | 3 / 10 | 0.092 | 1.000 | 1.000 | 8.0 | same |
| full/assault-peace | 139 | 56.1 | 64.7 | +8.6 [+0.3, +16.8] | 12 / 24 | 0.065 | 1.000 | 1.000 | 16.4 | same |
| full/choke | 128 | 68.0 | 77.3 | +9.4 [+1.1, +17.5] | 9 / 21 | 0.043 | 1.000 | 1.000 | 16.1 | same |
