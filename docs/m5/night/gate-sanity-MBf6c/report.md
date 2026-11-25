# Gate report: MBf6c (this machine) vs MBf6c (M4) (MBf6c (this machine) vs MBf6c (M4))

**Verdict: PASS**  (reference A = MBf6c (M4), candidate B = MBf6c (this machine); delta = B minus A; alpha 0.05, Holm across families)

| gate | result | what |
|---|---|---|
| harm | pass | no family dropped by more than 5 points with Holm-adjusted p < 0.05, over 35 families |

- Families (harm set, 35): up 0, down 0 (of which beyond tolerance: DROP 0), unchanged 35. Benjamini-Hochberg at 0.05: 0 families differ.
- Not shown to be within the tolerance (the 95 % CI of the delta reaches below -5 points; informational): 4 of 35: quick/prey-land, quick/ram, full/bombard, full/harass.

## Families

b = plays A won and B lost, c = the reverse. p = exact McNemar (two-sided); Holm = adjusted across the harm set; p-worse is one-sided (H1: B worse). MDE = smallest drop the paired test finds at 80 % power at level alpha/m (Bonferroni bound of Holm), given the observed discordance; - = too few discordant plays to find any drop.

| family | n | A % | B % | delta (CI) | b / c | p | p Holm | p-worse Holm | MDE | status |
|---|---|---|---|---|---|---|---|---|---|---|
| quick/assault-heal | 140 | 78.6 | 78.6 | +0.0 [-1.5, +1.5] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| quick/city-fire | 140 | 44.3 | 44.3 | +0.0 [-0.3, +0.3] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| quick/focus | 160 | 95.6 | 95.6 | +0.0 [-2.1, +2.1] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| quick/hill-archers | 140 | 66.4 | 66.4 | +0.0 [-0.9, +0.9] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| quick/planned-assault | 134 | 56.7 | 56.7 | +0.0 [-0.4, +0.4] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| quick/prey-either | 146 | 96.6 | 96.6 | +0.0 [-2.4, +2.4] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| quick/prey-land | 14 | 100.0 | 100.0 | +0.0 [-21.5, +21.5] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| quick/promotion-choice | 140 | 95.0 | 95.0 | +0.0 [-2.4, +2.4] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| quick/raid | 160 | 95.6 | 95.6 | +0.0 [-2.1, +2.1] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| quick/ram | 40 | 82.5 | 82.5 | +0.0 [-5.7, +5.7] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| quick/rush | 157 | 90.4 | 90.4 | +0.0 [-1.9, +1.9] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| quick/siege | 130 | 60.8 | 60.8 | +0.0 [-0.6, +0.6] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| quick/snipe | 140 | 100.0 | 100.0 | +0.0 [-2.7, +2.7] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/airstrike | 55 | 80.0 | 80.0 | +0.0 [-3.9, +3.9] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/ambush-fog | 160 | 100.0 | 100.0 | +0.0 [-2.3, +2.3] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/assault-heal | 140 | 78.6 | 78.6 | +0.0 [-1.5, +1.5] | 0 / 0 | 1.000 | - | - | - | same-as-quick/assault-heal |
| full/assault-peace | 139 | 75.5 | 75.5 | +0.0 [-1.4, +1.4] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/assault-reinforce | 138 | 60.9 | 60.9 | +0.0 [-0.6, +0.6] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/bait | 127 | 96.1 | 96.1 | +0.0 [-2.7, +2.7] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/bombard | 59 | 93.2 | 93.2 | +0.0 [-5.3, +5.3] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/choke | 128 | 75.0 | 75.0 | +0.0 [-1.5, +1.5] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/citadel | 120 | 35.8 | 35.8 | +0.0 [-0.9, +0.9] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/city-fire | 140 | 44.3 | 44.3 | +0.0 [-0.3, +0.3] | 0 / 0 | 1.000 | - | - | - | same-as-quick/city-fire |
| full/convoy | 128 | 0.0 | 0.0 | +0.0 [-2.9, +2.9] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/counter | 133 | 76.7 | 76.7 | +0.0 [-1.5, +1.5] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/duel | 160 | 78.1 | 78.1 | +0.0 [-1.3, +1.3] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/flank | 160 | 93.1 | 93.1 | +0.0 [-2.0, +2.0] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/focus | 160 | 95.6 | 95.6 | +0.0 [-2.1, +2.1] | 0 / 0 | 1.000 | - | - | - | same-as-quick/focus |
| full/general | 160 | 81.9 | 81.9 | +0.0 [-1.5, +1.5] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/guard | 160 | 25.6 | 25.6 | +0.0 [-1.1, +1.1] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/harass | 40 | 92.5 | 92.5 | +0.0 [-7.4, +7.4] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/hill-archers | 140 | 66.4 | 66.4 | +0.0 [-0.9, +0.9] | 0 / 0 | 1.000 | - | - | - | same-as-quick/hill-archers |
| full/hold | 137 | 70.1 | 70.1 | +0.0 [-1.1, +1.1] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/hunt-fog | 160 | 97.5 | 97.5 | +0.0 [-2.2, +2.2] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/kite | 130 | 90.8 | 90.8 | +0.0 [-2.3, +2.3] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/planned-assault | 134 | 56.7 | 56.7 | +0.0 [-0.4, +0.4] | 0 / 0 | 1.000 | - | - | - | same-as-quick/planned-assault |
| full/prey-either | 146 | 96.6 | 96.6 | +0.0 [-2.4, +2.4] | 0 / 0 | 1.000 | - | - | - | same-as-quick/prey-either |
| full/prey-land | 14 | 100.0 | 100.0 | +0.0 [-21.5, +21.5] | 0 / 0 | 1.000 | - | - | - | same-as-quick/prey-land |
| full/promotion-choice | 140 | 95.0 | 95.0 | +0.0 [-2.4, +2.4] | 0 / 0 | 1.000 | - | - | - | same-as-quick/promotion-choice |
| full/raid | 160 | 95.6 | 95.6 | +0.0 [-2.1, +2.1] | 0 / 0 | 1.000 | - | - | - | same-as-quick/raid |
| full/ram | 40 | 82.5 | 82.5 | +0.0 [-5.7, +5.7] | 0 / 0 | 1.000 | - | - | - | same-as-quick/ram |
| full/retreat | 128 | 85.2 | 85.2 | +0.0 [-2.0, +2.0] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/rush | 157 | 90.4 | 90.4 | +0.0 [-1.9, +1.9] | 0 / 0 | 1.000 | - | - | - | same-as-quick/rush |
| full/siege | 130 | 60.8 | 60.8 | +0.0 [-0.6, +0.6] | 0 / 0 | 1.000 | - | - | - | same-as-quick/siege |
| full/sink | 140 | 100.0 | 100.0 | +0.0 [-2.7, +2.7] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/snipe | 140 | 100.0 | 100.0 | +0.0 [-2.7, +2.7] | 0 / 0 | 1.000 | - | - | - | same-as-quick/snipe |
| full/surrounded | 160 | 96.2 | 96.2 | +0.0 [-2.2, +2.2] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
| full/wounded | 148 | 93.9 | 93.9 | +0.0 [-2.2, +2.2] | 0 / 0 | 1.000 | 1.000 | 1.000 | - | same |
