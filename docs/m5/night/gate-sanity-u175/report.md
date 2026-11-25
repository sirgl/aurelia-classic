# Gate report: rl2-u175 (this machine) vs u0175 (M4) (rl2-u175 (this machine) vs u0175 (M4))

**Verdict: PASS**  (reference A = u0175 (M4), candidate B = rl2-u175 (this machine); delta = B minus A; alpha 0.05, Holm across families)

| gate | result | what |
|---|---|---|
| harm | pass | no family dropped by more than 5 points with Holm-adjusted p < 0.05, over 35 families |

- Families (harm set, 35): up 0, down 0 (of which beyond tolerance: DROP 0), unchanged 35. Benjamini-Hochberg at 0.05: 0 families differ.
- Not shown to be within the tolerance (the 95 % CI of the delta reaches below -5 points; informational): 5 of 35: quick/prey-land, quick/ram, full/airstrike, full/bombard, full/harass.

## Families

b = plays A won and B lost, c = the reverse. p = exact McNemar (two-sided); Holm = adjusted across the harm set; p-worse is one-sided (H1: B worse). MDE = smallest drop the paired test finds at 80 % power at level alpha/m (Bonferroni bound of Holm), given the observed discordance; - = too few discordant plays to find any drop.

| family | n | A % | B % | delta (CI) | b / c | p | p Holm | p-worse Holm | MDE | status |
|---|---|---|---|---|---|---|---|---|---|---|
| quick/assault-heal | 140 | 95.7 | 95.7 | +0.0 [-2.4, +2.4] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| quick/city-fire | 140 | 98.6 | 98.6 | +0.0 [-2.6, +2.6] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| quick/focus | 160 | 96.2 | 96.2 | +0.0 [-2.2, +2.2] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| quick/hill-archers | 140 | 82.9 | 82.9 | +0.0 [-1.8, +1.8] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| quick/planned-assault | 134 | 77.6 | 77.6 | +0.0 [-1.5, +1.5] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| quick/prey-either | 146 | 99.3 | 99.3 | +0.0 [-2.5, +2.5] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| quick/prey-land | 14 | 100.0 | 100.0 | +0.0 [-21.5, +21.5] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| quick/promotion-choice | 140 | 99.3 | 99.3 | +0.0 [-2.6, +2.6] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| quick/raid | 160 | 98.1 | 98.1 | +0.0 [-2.3, +2.3] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| quick/ram | 40 | 95.0 | 95.0 | +0.0 [-7.9, +7.9] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| quick/rush | 157 | 90.4 | 90.4 | +0.0 [-1.9, +1.9] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| quick/siege | 130 | 92.3 | 92.3 | +0.0 [-2.4, +2.4] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| quick/snipe | 140 | 100.0 | 100.0 | +0.0 [-2.7, +2.7] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/airstrike | 55 | 89.1 | 89.1 | +0.0 [-5.1, +5.1] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/ambush-fog | 160 | 97.5 | 97.5 | +0.0 [-2.2, +2.2] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/assault-heal | 140 | 95.7 | 95.7 | +0.0 [-2.4, +2.4] | 0 / 0 | 1.000 | - | - | - | same-as-quick/assault-heal |
| full/assault-peace | 139 | 56.1 | 56.1 | +0.0 [-0.3, +0.3] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/assault-reinforce | 138 | 71.7 | 71.7 | +0.0 [-1.2, +1.2] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/bait | 127 | 92.1 | 92.1 | +0.0 [-2.5, +2.5] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/bombard | 59 | 94.9 | 94.9 | +0.0 [-5.5, +5.5] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/choke | 128 | 68.0 | 68.0 | +0.0 [-1.0, +1.0] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/citadel | 120 | 59.2 | 59.2 | +0.0 [-0.6, +0.6] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/city-fire | 140 | 98.6 | 98.6 | +0.0 [-2.6, +2.6] | 0 / 0 | 1.000 | - | - | - | same-as-quick/city-fire |
| full/convoy | 128 | 0.0 | 0.0 | +0.0 [-2.9, +2.9] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/counter | 133 | 82.0 | 82.0 | +0.0 [-1.8, +1.8] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/duel | 160 | 75.0 | 75.0 | +0.0 [-1.2, +1.2] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/flank | 160 | 96.2 | 96.2 | +0.0 [-2.2, +2.2] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/focus | 160 | 96.2 | 96.2 | +0.0 [-2.2, +2.2] | 0 / 0 | 1.000 | - | - | - | same-as-quick/focus |
| full/general | 160 | 77.5 | 77.5 | +0.0 [-1.3, +1.3] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/guard | 160 | 28.1 | 28.1 | +0.0 [-1.0, +1.0] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/harass | 40 | 92.5 | 92.5 | +0.0 [-7.4, +7.4] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/hill-archers | 140 | 82.9 | 82.9 | +0.0 [-1.8, +1.8] | 0 / 0 | 1.000 | - | - | - | same-as-quick/hill-archers |
| full/hold | 137 | 67.2 | 67.2 | +0.0 [-0.9, +0.9] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/hunt-fog | 160 | 96.2 | 96.2 | +0.0 [-2.2, +2.2] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/kite | 130 | 90.0 | 90.0 | +0.0 [-2.3, +2.3] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/planned-assault | 134 | 77.6 | 77.6 | +0.0 [-1.5, +1.5] | 0 / 0 | 1.000 | - | - | - | same-as-quick/planned-assault |
| full/prey-either | 146 | 99.3 | 99.3 | +0.0 [-2.5, +2.5] | 0 / 0 | 1.000 | - | - | - | same-as-quick/prey-either |
| full/prey-land | 14 | 100.0 | 100.0 | +0.0 [-21.5, +21.5] | 0 / 0 | 1.000 | - | - | - | same-as-quick/prey-land |
| full/promotion-choice | 140 | 99.3 | 99.3 | +0.0 [-2.6, +2.6] | 0 / 0 | 1.000 | - | - | - | same-as-quick/promotion-choice |
| full/raid | 160 | 98.1 | 98.1 | +0.0 [-2.3, +2.3] | 0 / 0 | 1.000 | - | - | - | same-as-quick/raid |
| full/ram | 40 | 95.0 | 95.0 | +0.0 [-7.9, +7.9] | 0 / 0 | 1.000 | - | - | - | same-as-quick/ram |
| full/retreat | 128 | 80.5 | 80.5 | +0.0 [-1.8, +1.8] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/rush | 157 | 90.4 | 90.4 | +0.0 [-1.9, +1.9] | 0 / 0 | 1.000 | - | - | - | same-as-quick/rush |
| full/siege | 130 | 92.3 | 92.3 | +0.0 [-2.4, +2.4] | 0 / 0 | 1.000 | - | - | - | same-as-quick/siege |
| full/sink | 140 | 100.0 | 100.0 | +0.0 [-2.7, +2.7] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/snipe | 140 | 100.0 | 100.0 | +0.0 [-2.7, +2.7] | 0 / 0 | 1.000 | - | - | - | same-as-quick/snipe |
| full/surrounded | 160 | 96.2 | 96.2 | +0.0 [-2.2, +2.2] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/wounded | 148 | 85.8 | 85.8 | +0.0 [-1.8, +1.8] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
