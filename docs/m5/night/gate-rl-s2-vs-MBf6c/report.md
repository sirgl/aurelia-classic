# Gate report: rl-s2 u150 vs MBf6c (rl-s2 u150 vs MBf6c)

**Verdict: PASS**  (reference A = MBf6c, candidate B = rl-s2 u150; delta = B minus A; alpha 0.05, Holm across families)

| gate | result | what |
|---|---|---|
| harm | pass | no family dropped by more than 5 points with Holm-adjusted p < 0.05, over 35 families |
| required_up | pass | every required family up by more than 0 points with Holm-adjusted p < 0.05 (Holm over the required set) |
| mean | pass | mean over 35 families up: paired bootstrap 95 % CI lower bound above 0 points |
| count_at_bar | pass | families at >= 90 % of the best teacher, of 35: at least 0 more than the reference |

- Mean over 35 families: 78.9 -> 84.4, delta +5.5 points, paired bootstrap CI [+4.4, +6.6].
- Families at >= 90 % of the best teacher, of 35: 18 (CI 15-20) -> 25 (CI 21-27); difference +7, CI [+3, +10]. The bar is fixed at the point estimate of the best teacher.
- Families (harm set, 35): up 6, down 0 (of which beyond tolerance: DROP 0), unchanged 29. Benjamini-Hochberg at 0.05: 7 families differ.
- Not shown to be within the tolerance (the 95 % CI of the delta reaches below -5 points; informational): 11 of 35: quick/prey-land, quick/rush, full/assault-peace, full/bait, full/choke, full/general, full/harass, full/hold, full/kite, full/retreat, full/wounded.
- Required families: quick/planned-assault up, quick/assault-heal up.

## Families

b = plays A won and B lost, c = the reverse. p = exact McNemar (two-sided); Holm = adjusted across the harm set; p-worse is one-sided (H1: B worse). MDE = smallest drop the paired test finds at 80 % power at level alpha/m (Bonferroni bound of Holm), given the observed discordance; - = too few discordant plays to find any drop.

| family | n | A % | B % | delta (CI) | b / c | p | p Holm | p-worse Holm | MDE | status |
|---|---|---|---|---|---|---|---|---|---|---|
| full/general | 160 | 81.9 | 73.8 | -8.1 [-14.7, -1.6] | 21 / 8 | 0.024 | 0.651 | 0.422 | 12.6 | same |
| quick/prey-land | 14 | 100.0 | 92.9 | -7.1 [-31.5, +15.2] | 1 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/prey-land | 14 | 100.0 | 92.9 | -7.1 [-31.5, +15.2] | 1 / 0 | 1.000 | - | - | - | same-as-quick/prey-land |
| full/assault-peace | 139 | 75.5 | 69.8 | -5.8 [-14.5, +3.1] | 24 / 16 | 0.268 | 1.000 | 1.000 | 17.3 | same |
| full/bait | 127 | 96.1 | 91.3 | -4.7 [-9.7, -1.0] | 6 / 0 | 0.031 | 0.813 | 0.531 | - | same |
| full/retreat | 128 | 85.2 | 80.5 | -4.7 [-9.9, +0.2] | 8 / 2 | 0.109 | 1.000 | 1.000 | - | same |
| full/hold | 137 | 70.1 | 65.7 | -4.4 [-9.7, +0.9] | 10 / 4 | 0.180 | 1.000 | 1.000 | 9.7 | same |
| full/wounded | 148 | 93.9 | 91.2 | -2.7 [-8.2, +2.5] | 9 / 5 | 0.424 | 1.000 | 1.000 | 9.0 | same |
| quick/rush | 157 | 90.4 | 88.5 | -1.9 [-7.1, +3.1] | 9 / 6 | 0.607 | 1.000 | 1.000 | 8.9 | same |
| full/rush | 157 | 90.4 | 88.5 | -1.9 [-7.1, +3.1] | 9 / 6 | 0.607 | - | - | 8.9 | same-as-quick/rush |
| full/kite | 130 | 90.8 | 89.2 | -1.5 [-7.6, +4.4] | 8 / 6 | 0.791 | 1.000 | 1.000 | 10.3 | same |
| quick/snipe | 140 | 100.0 | 99.3 | -0.7 [-3.9, +2.0] | 1 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/snipe | 140 | 100.0 | 99.3 | -0.7 [-3.9, +2.0] | 1 / 0 | 1.000 | - | - | - | same-as-quick/snipe |
| full/hunt-fog | 160 | 97.5 | 96.9 | -0.6 [-4.2, +2.8] | 3 / 2 | 1.000 | 1.000 | 1.000 | - | same |
| quick/focus | 160 | 95.6 | 95.6 | +0.0 [-4.0, +4.0] | 4 / 4 | 1.000 | 1.000 | 1.000 | - | same |
| full/ambush-fog | 160 | 100.0 | 100.0 | +0.0 [-2.3, +2.3] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/convoy | 128 | 0.0 | 0.0 | +0.0 [-2.9, +2.9] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/focus | 160 | 95.6 | 95.6 | +0.0 [-4.0, +4.0] | 4 / 4 | 1.000 | - | - | - | same-as-quick/focus |
| full/sink | 140 | 100.0 | 100.0 | +0.0 [-2.7, +2.7] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/surrounded | 160 | 96.2 | 96.2 | +0.0 [-2.2, +2.2] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| quick/prey-either | 146 | 96.6 | 97.3 | +0.7 [-3.0, +4.6] | 2 / 3 | 1.000 | 1.000 | 1.000 | - | same |
| full/prey-either | 146 | 96.6 | 97.3 | +0.7 [-3.0, +4.6] | 2 / 3 | 1.000 | - | - | - | same-as-quick/prey-either |
| full/choke | 128 | 75.0 | 76.6 | +1.6 [-6.3, +9.4] | 12 / 14 | 0.845 | 1.000 | 1.000 | 14.8 | same |
| full/bombard | 59 | 93.2 | 94.9 | +1.7 [-4.0, +8.4] | 0 / 1 | 1.000 | 1.000 | 1.000 | - | same |
| quick/raid | 160 | 95.6 | 97.5 | +1.9 [-1.3, +5.6] | 1 / 4 | 0.375 | 1.000 | 1.000 | - | same |
| full/raid | 160 | 95.6 | 97.5 | +1.9 [-1.3, +5.6] | 1 / 4 | 0.375 | - | - | - | same-as-quick/raid |
| full/flank | 160 | 93.1 | 95.6 | +2.5 [-0.2, +6.0] | 0 / 4 | 0.125 | 1.000 | 1.000 | - | same |
| full/guard | 160 | 25.6 | 28.1 | +2.5 [-0.1, +5.2] | 0 / 4 | 0.125 | 1.000 | 1.000 | - | same |
| quick/promotion-choice | 140 | 95.0 | 98.6 | +3.6 [-0.9, +8.7] | 2 / 7 | 0.180 | 1.000 | 1.000 | - | same |
| full/promotion-choice | 140 | 95.0 | 98.6 | +3.6 [-0.9, +8.7] | 2 / 7 | 0.180 | - | - | - | same-as-quick/promotion-choice |
| full/duel | 160 | 78.1 | 82.5 | +4.4 [+0.3, +8.7] | 2 / 9 | 0.065 | 1.000 | 1.000 | - | same |
| full/harass | 40 | 92.5 | 97.5 | +5.0 [-6.7, +17.6] | 1 / 3 | 0.625 | 1.000 | 1.000 | - | same |
| full/counter | 133 | 76.7 | 85.7 | +9.0 [+2.6, +15.7] | 4 / 16 | 0.012 | 0.331 | 1.000 | 12.3 | same |
| full/airstrike | 55 | 80.0 | 89.1 | +9.1 [-0.3, +19.4] | 1 / 6 | 0.125 | 1.000 | 1.000 | - | same |
| quick/ram | 40 | 82.5 | 97.5 | +15.0 [+4.0, +28.9] | 0 / 6 | 0.031 | 0.813 | 1.000 | - | same |
| full/ram | 40 | 82.5 | 97.5 | +15.0 [+4.0, +28.9] | 0 / 6 | 0.031 | - | - | - | same-as-quick/ram |
| full/assault-reinforce | 138 | 60.9 | 76.1 | +15.2 [+5.5, +24.6] | 14 / 35 | 0.004 | 0.110 | 1.000 | 19.4 | same |
| quick/assault-heal | 140 | 78.6 | 95.7 | +17.1 [+10.4, +24.5] | 2 / 26 | <0.001 | <0.001 | 1.000 | 14.1 | up |
| full/assault-heal | 140 | 78.6 | 95.7 | +17.1 [+10.4, +24.5] | 2 / 26 | <0.001 | - | - | 14.1 | same-as-quick/assault-heal |
| quick/hill-archers | 140 | 66.4 | 86.4 | +20.0 [+11.2, +28.6] | 8 / 36 | <0.001 | <0.001 | 1.000 | 18.1 | up |
| full/hill-archers | 140 | 66.4 | 86.4 | +20.0 [+11.2, +28.6] | 8 / 36 | <0.001 | - | - | 18.1 | same-as-quick/hill-archers |
| full/citadel | 120 | 35.8 | 57.5 | +21.7 [+10.3, +32.2] | 13 / 39 | <0.001 | 0.012 | 1.000 | 22.9 | up |
| quick/siege | 130 | 60.8 | 86.2 | +25.4 [+16.6, +33.8] | 4 / 37 | <0.001 | <0.001 | 1.000 | 18.8 | up |
| full/siege | 130 | 60.8 | 86.2 | +25.4 [+16.6, +33.8] | 4 / 37 | <0.001 | - | - | 18.8 | same-as-quick/siege |
| quick/planned-assault | 134 | 56.7 | 85.1 | +28.4 [+20.0, +36.3] | 2 / 40 | <0.001 | <0.001 | 1.000 | 18.5 | up |
| full/planned-assault | 134 | 56.7 | 85.1 | +28.4 [+20.0, +36.3] | 2 / 40 | <0.001 | - | - | 18.5 | same-as-quick/planned-assault |
| quick/city-fire | 140 | 44.3 | 95.0 | +50.7 [+42.0, +58.6] | 0 / 71 | <0.001 | <0.001 | 1.000 | 23.1 | up |
| full/city-fire | 140 | 44.3 | 95.0 | +50.7 [+42.0, +58.6] | 0 / 71 | <0.001 | - | - | 23.1 | same-as-quick/city-fire |
