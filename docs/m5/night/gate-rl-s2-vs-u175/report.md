# Gate report: rl-s2 u150 vs rl2 u175 (seed 1) (rl-s2 u150 vs rl2 u175 (seed 1))

**Verdict: PASS**  (reference A = rl2 u175 (seed 1), candidate B = rl-s2 u150; delta = B minus A; alpha 0.05, Holm across families)

| gate | result | what |
|---|---|---|
| harm | pass | no family dropped by more than 5 points with Holm-adjusted p < 0.05, over 35 families |
| mean | pass | mean over 35 families up: paired bootstrap 95 % CI lower bound above 0 points |
| count_at_bar | pass | families at >= 90 % of the best teacher, of 35: at least 0 more than the reference |

- Mean over 35 families: 83.5 -> 84.4, delta +0.9 points, paired bootstrap CI [+0.0, +1.8].
- Families at >= 90 % of the best teacher, of 35: 21 (CI 19-23) -> 25 (CI 21-27); difference +4, CI [+0, +6]. The bar is fixed at the point estimate of the best teacher.
- Families (harm set, 35): up 0, down 0 (of which beyond tolerance: DROP 0), unchanged 35. Benjamini-Hochberg at 0.05: 0 families differ.
- Not shown to be within the tolerance (the 95 % CI of the delta reaches below -5 points; informational): 13 of 35: quick/city-fire, quick/prey-either, quick/prey-land, quick/ram, quick/rush, quick/siege, full/airstrike, full/bombard, full/citadel, full/general, full/hold, full/kite, full/retreat.

## Families

b = plays A won and B lost, c = the reverse. p = exact McNemar (two-sided); Holm = adjusted across the harm set; p-worse is one-sided (H1: B worse). MDE = smallest drop the paired test finds at 80 % power at level alpha/m (Bonferroni bound of Holm), given the observed discordance; - = too few discordant plays to find any drop.

| family | n | A % | B % | delta (CI) | b / c | p | p Holm | p-worse Holm | MDE | status |
|---|---|---|---|---|---|---|---|---|---|---|
| quick/prey-land | 14 | 100.0 | 92.9 | -7.1 [-31.5, +15.2] | 1 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/prey-land | 14 | 100.0 | 92.9 | -7.1 [-31.5, +15.2] | 1 / 0 | 1.000 | - | - | - | same-as-quick/prey-land |
| quick/siege | 130 | 92.3 | 86.2 | -6.2 [-13.8, +1.3] | 16 / 8 | 0.152 | 1.000 | 1.000 | 13.9 | same |
| full/siege | 130 | 92.3 | 86.2 | -6.2 [-13.8, +1.3] | 16 / 8 | 0.152 | - | - | 13.9 | same-as-quick/siege |
| full/general | 160 | 77.5 | 73.8 | -3.8 [-10.2, +2.7] | 17 / 11 | 0.345 | 1.000 | 1.000 | 12.4 | same |
| quick/city-fire | 140 | 98.6 | 95.0 | -3.6 [-8.7, +0.9] | 7 / 2 | 0.180 | 1.000 | 1.000 | - | same |
| full/city-fire | 140 | 98.6 | 95.0 | -3.6 [-8.7, +0.9] | 7 / 2 | 0.180 | - | - | - | same-as-quick/city-fire |
| quick/prey-either | 146 | 99.3 | 97.3 | -2.1 [-6.2, +1.5] | 4 / 1 | 0.375 | 1.000 | 1.000 | - | same |
| full/prey-either | 146 | 99.3 | 97.3 | -2.1 [-6.2, +1.5] | 4 / 1 | 0.375 | - | - | - | same-as-quick/prey-either |
| quick/rush | 157 | 90.4 | 88.5 | -1.9 [-6.5, +2.5] | 7 / 4 | 0.549 | 1.000 | 1.000 | - | same |
| full/rush | 157 | 90.4 | 88.5 | -1.9 [-6.5, +2.5] | 7 / 4 | 0.549 | - | - | - | same-as-quick/rush |
| full/citadel | 120 | 59.2 | 57.5 | -1.7 [-7.7, +4.4] | 8 / 6 | 0.791 | 1.000 | 1.000 | 11.1 | same |
| full/hold | 137 | 67.2 | 65.7 | -1.5 [-6.0, +3.1] | 6 / 4 | 0.754 | 1.000 | 1.000 | - | same |
| full/bait | 127 | 92.1 | 91.3 | -0.8 [-3.8, +1.9] | 1 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/kite | 130 | 90.0 | 89.2 | -0.8 [-7.0, +5.4] | 8 / 7 | 1.000 | 1.000 | 1.000 | 10.7 | same |
| quick/promotion-choice | 140 | 99.3 | 98.6 | -0.7 [-4.4, +2.7] | 2 / 1 | 1.000 | 1.000 | 1.000 | - | same |
| quick/snipe | 140 | 100.0 | 99.3 | -0.7 [-3.9, +2.0] | 1 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/promotion-choice | 140 | 99.3 | 98.6 | -0.7 [-4.4, +2.7] | 2 / 1 | 1.000 | - | - | - | same-as-quick/promotion-choice |
| full/snipe | 140 | 100.0 | 99.3 | -0.7 [-3.9, +2.0] | 1 / 0 | 1.000 | - | - | - | same-as-quick/snipe |
| quick/focus | 160 | 96.2 | 95.6 | -0.6 [-4.2, +2.7] | 3 / 2 | 1.000 | 1.000 | 1.000 | - | same |
| quick/raid | 160 | 98.1 | 97.5 | -0.6 [-3.4, +1.7] | 1 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/flank | 160 | 96.2 | 95.6 | -0.6 [-3.3, +1.7] | 1 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/focus | 160 | 96.2 | 95.6 | -0.6 [-4.2, +2.7] | 3 / 2 | 1.000 | - | - | - | same-as-quick/focus |
| full/raid | 160 | 98.1 | 97.5 | -0.6 [-3.4, +1.7] | 1 / 0 | 1.000 | - | - | - | same-as-quick/raid |
| quick/assault-heal | 140 | 95.7 | 95.7 | +0.0 [-3.7, +3.7] | 2 / 2 | 1.000 | 1.000 | 1.000 | - | same |
| full/airstrike | 55 | 89.1 | 89.1 | +0.0 [-8.6, +8.6] | 2 / 2 | 1.000 | 1.000 | 1.000 | - | same |
| full/assault-heal | 140 | 95.7 | 95.7 | +0.0 [-3.7, +3.7] | 2 / 2 | 1.000 | - | - | - | same-as-quick/assault-heal |
| full/bombard | 59 | 94.9 | 94.9 | +0.0 [-5.5, +5.5] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/convoy | 128 | 0.0 | 0.0 | +0.0 [-2.9, +2.9] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/guard | 160 | 28.1 | 28.1 | +0.0 [-1.0, +1.0] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/retreat | 128 | 80.5 | 80.5 | +0.0 [-5.9, +5.9] | 7 / 7 | 1.000 | 1.000 | 1.000 | 10.4 | same |
| full/sink | 140 | 100.0 | 100.0 | +0.0 [-2.7, +2.7] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/surrounded | 160 | 96.2 | 96.2 | +0.0 [-2.2, +2.2] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/hunt-fog | 160 | 96.2 | 96.9 | +0.6 [-2.3, +3.8] | 1 / 2 | 1.000 | 1.000 | 1.000 | - | same |
| quick/ram | 40 | 95.0 | 97.5 | +2.5 [-5.8, +12.7] | 0 / 1 | 1.000 | 1.000 | 1.000 | - | same |
| full/ambush-fog | 160 | 97.5 | 100.0 | +2.5 [-0.3, +6.3] | 0 / 4 | 0.125 | 1.000 | 1.000 | - | same |
| full/ram | 40 | 95.0 | 97.5 | +2.5 [-5.8, +12.7] | 0 / 1 | 1.000 | - | - | - | same-as-quick/ram |
| quick/hill-archers | 140 | 82.9 | 86.4 | +3.6 [-3.8, +11.0] | 11 / 16 | 0.442 | 1.000 | 1.000 | 13.8 | same |
| full/hill-archers | 140 | 82.9 | 86.4 | +3.6 [-3.8, +11.0] | 11 / 16 | 0.442 | - | - | 13.8 | same-as-quick/hill-archers |
| full/counter | 133 | 82.0 | 85.7 | +3.8 [-1.2, +9.0] | 3 / 8 | 0.227 | 1.000 | 1.000 | - | same |
| full/assault-reinforce | 138 | 71.7 | 76.1 | +4.3 [-3.2, +11.8] | 11 / 17 | 0.345 | 1.000 | 1.000 | 14.3 | same |
| full/harass | 40 | 92.5 | 97.5 | +5.0 [-3.6, +16.3] | 0 / 2 | 0.500 | 1.000 | 1.000 | - | same |
| full/wounded | 148 | 85.8 | 91.2 | +5.4 [+0.8, +10.5] | 2 / 10 | 0.039 | 1.000 | 1.000 | - | same |
| quick/planned-assault | 134 | 77.6 | 85.1 | +7.5 [+0.4, +14.7] | 7 / 17 | 0.064 | 1.000 | 1.000 | 13.5 | same |
| full/planned-assault | 134 | 77.6 | 85.1 | +7.5 [+0.4, +14.7] | 7 / 17 | 0.064 | - | - | 13.5 | same-as-quick/planned-assault |
| full/duel | 160 | 75.0 | 82.5 | +7.5 [+2.7, +12.5] | 2 / 14 | 0.004 | 0.142 | 1.000 | 9.0 | same |
| full/choke | 128 | 68.0 | 76.6 | +8.6 [+0.7, +16.4] | 8 / 19 | 0.052 | 1.000 | 1.000 | 15.1 | same |
| full/assault-peace | 139 | 56.1 | 69.8 | +13.7 [+5.1, +21.9] | 10 / 29 | 0.003 | 0.118 | 1.000 | 17.1 | same |
