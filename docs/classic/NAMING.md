# Naming of the rule constants

The project owner's rule: do not invent names mechanically; take the name of the corresponding concept from Unciv (open source, named here as allowed) or from a public analogue, and only as a last resort keep a descriptive name of our own.

Only the NAME of a concept was learned from Unciv (Kotlin property, json field, local variable or function name converted to Rust style); no code or text was copied. Where Unciv has the number inline, the name comes from the surrounding function or variable (status `analogue`). Where Unciv has no counterpart the earlier descriptive name stays (status `own`).

Status: `matched` = the same quantity under the Unciv name; `analogue` = the same concept, name taken from the surrounding Unciv function or variable (value or form may differ, see the note); `own` = no Unciv counterpart.

None of these constants is serialized, hashed or read from a rules data file (all are compile-time Rust constants), so no hash or save format changes. No data-file key was renamed; nothing here needs a format version.

Counts: matched 15, analogue 26, own 39, total 80.

| our name | Unciv name (file / property) or note | status |
|---|---|---|
| `CITY_STRENGTH_FROM_GARRISON_DIVISOR` | ModConstants.kt cityStrengthFromGarrison (0.2) (ours is the reciprocal form of 0.2, x100) | analogue |
| `CULTURE_TO_NEXT_TILE_EXPONENT` | CityExpansionManager.kt getCultureToNextTile (formulas differ) | analogue |
| `CULTURE_TO_NEXT_TILE_FIRST` | CityExpansionManager.kt getCultureToNextTile (formulas differ) | analogue |
| `CULTURE_TO_NEXT_TILE_LEVEL_FACTOR` | CityExpansionManager.kt getCultureToNextTile (formulas differ) | analogue |
| `CULTURE_TO_NEXT_TILE_MAX_DISCOUNT_PERCENT` | CityExpansionManager.kt getCultureToNextTile (formulas differ) | analogue |
| `CULTURE_TO_NEXT_TILE_ROUND_TO` | CityExpansionManager.kt getCultureToNextTile (formulas differ) | analogue |
| `FOOD_TO_NEXT_POPULATION_BASE` | CityPopulationManager.kt getFoodToNextPopulation (15) | analogue |
| `FOOD_TO_NEXT_POPULATION_PER_POP` | CityPopulationManager.kt getFoodToNextPopulation (8) | analogue |
| `GREAT_GENERAL_BONUS_PERCENT` | Units.json Great General unique "[+15]% Strength bonus for ... units within [2] tiles" | analogue |
| `GREAT_GENERAL_BONUS_RANGE` | Units.json Great General unique (same line, 2 tiles) | analogue |
| `HEALING_ENEMY_TERRITORY` | MapUnit.kt rankTileForHealing (10) | analogue |
| `HEALING_FRIENDLY_TERRITORY` | MapUnit.kt rankTileForHealing (20) | analogue |
| `HEALING_IN_CITY_CENTER` | MapUnit.kt rankTileForHealing, local "healing" (25 on a city centre) | analogue |
| `HEALING_NEUTRAL_TERRITORY` | MapUnit.kt rankTileForHealing (10) | analogue |
| `PLUNDER_BASE_GOLD` | CityConquestFunctions.kt getGoldForCapturingCity baseGold (20) | analogue |
| `PLUNDER_GOLD_PER_POPULATION` | CityConquestFunctions.kt getGoldForCapturingCity (ours 15, Unciv 10) | analogue |
| `POINTS_FOR_NEXT_GREAT_GENERAL_INCREASE` | GreatPersonManager.kt getNewGreatPerson (the +50 is an inline number in Unciv) | analogue |
| `POINTS_FOR_NEXT_GREAT_PERSON_INCREASE` | GreatPersonManager.kt getNewGreatPerson (Unciv doubles the threshold, ours adds a fixed step) | analogue |
| `POLICY_CULTURE_COST_BASE` | PolicyManager.kt getPolicyCultureCost (25) | analogue |
| `POLICY_CULTURE_COST_EXPONENT` | PolicyManager.kt getPolicyCultureCost (2.01) | analogue |
| `POLICY_CULTURE_COST_PER_POLICY` | PolicyManager.kt getPolicyCultureCost (x3) | analogue |
| `POLICY_CULTURE_COST_ROUND_TO` | PolicyManager.kt getPolicyCultureCost (cost - cost % 5) | analogue |
| `UNHAPPINESS_FROM_ANNEXED_CITIZENS_TIMES100` | CityStats.kt unhappinessFromCitizens *= 2 (ours 1.34x, Unciv 2x) | analogue |
| `UNHAPPINESS_FROM_ANNEXED_CITY_TIMES100` | CityStats.kt hasExtraAnnexUnhappiness (ours 5, Unciv 3+2 extra annex) | analogue |
| `UNHAPPINESS_FROM_CITIZENS_TIMES100` | CityStats.kt unhappinessFromCitizens | analogue |
| `UNHAPPINESS_FROM_CITY_TIMES100` | CityStats.kt unhappinessFromCity (-3) | analogue |
| `ATTACKING_ACROSS_RIVER_MALUS` | BattleConstants.kt ATTACKING_ACROSS_RIVER_MALUS (-20) | matched |
| `BASE_FLANKING_BONUS` | BattleConstants.kt BASE_FLANKING_BONUS (10) | matched |
| `CITY_AIR_UNIT_CAPACITY` | ModConstants.kt cityAirUnitCapacity (6) | matched |
| `CITY_STRENGTH_BASE` | ModConstants.kt cityStrengthBase (8.0) (ours scaled x100) | matched |
| `CITY_STRENGTH_PER_POP` | ModConstants.kt cityStrengthPerPop (0.4) (ours scaled x100) | matched |
| `DAMAGE_TO_CIVILIAN_UNIT` | BattleConstants.kt DAMAGE_TO_CIVILIAN_UNIT (40) | matched |
| `FORTIFICATION_BONUS` | BattleConstants.kt FORTIFICATION_BONUS (20) | matched |
| `HILLS_DEFENCE_BONUS` | Terrains.json Hill "defenceBonus" (0.25) | matched |
| `LANDING_MALUS` | BattleConstants.kt LANDING_MALUS (-50) | matched |
| `MAX_XP_FROM_BARBARIANS` | ModConstants.kt maxXPfromBarbarians (30) | matched |
| `MISSING_RESOURCES_MALUS` | BattleConstants.kt MISSING_RESOURCES_MALUS (value differs: ours -50, Unciv -25) | matched |
| `POINTS_FOR_NEXT_GREAT_GENERAL` | GreatPersonManager.kt pointsForNextGreatGeneral (200) (initial value) | matched |
| `POINTS_FOR_NEXT_GREAT_PERSON` | GreatPersonManager.kt pointsForNextGreatPersonCounter (100) (initial value) | matched |
| `UNMODIFIED_GOLDEN_AGE_TURNS` | GoldenAgeManager.kt enterGoldenAge(unmodifiedNumberOfTurns = 10) (parameter default) | matched |
| `UNMODIFIED_GOLDEN_AGE_TURNS` | GoldenAgeManager.kt enterGoldenAge(unmodifiedNumberOfTurns = 10) (parameter default) | matched |
| `ADOPT_ATTACK_BONUS_PERCENT` | none in Unciv (named after the effect, not the policy) | own |
| `BARBARIAN_CAPTURE_RANSOM` | no named Unciv counterpart | own |
| `CAPTURED_CITY_POPULATION_KEPT_PERCENT` | no named Unciv counterpart (CityConquestFunctions.kt halves nothing by a constant) | own |
| `CAPTURED_UNIT_HP` | no Unciv counterpart | own |
| `CITY_ATTACK_DAMAGE_PERCENT` | no Unciv counterpart | own |
| `CITY_HEAL_PER_TURN` | no named Unciv counterpart | own |
| `CITY_HILL_STRENGTH` | no named Unciv counterpart (CityCombatant.kt has the garrison/tech terms only) | own |
| `CITY_PLUNDER_FULL_AFTER_ROUNDS` | CityConquestFunctions.kt turnModifier (inline) | own |
| `CITY_PLUNDER_ROLL_A` | CityConquestFunctions.kt getGoldForCapturingCity uses `nextInt(40)` inline; ours is two rolls | own |
| `CITY_PLUNDER_ROLL_B` | as above | own |
| `CITY_RANGED_STRENGTH_PERCENT` | no Unciv counterpart | own |
| `DIFFERENT_IDEOLOGY_TOURISM_PERCENT` | no Unciv counterpart (no tourism) | own |
| `DIPLOMAT_TOURISM_PERCENT` | no Unciv counterpart (no tourism) | own |
| `EXPECTED_CITY_COUNT` | no Unciv counterpart (AI constant) | own |
| `FRIENDSHIP_MIN_CONTACT_ROUNDS` | no Unciv counterpart | own |
| `INSTANT_HEAL_HIT_POINTS` | no Unciv counterpart | own |
| `MELEE_EQUAL_STRENGTH_BASE_DAMAGE` | BattleDamage.kt damageModifier: `24 + 12 * randomnessFactor` (24 hit points; ours is in hundredths) | own |
| `MELEE_EQUAL_STRENGTH_RANDOM_DAMAGE` | BattleDamage.kt damageModifier (the 12 of `24 + 12 * randomnessFactor`) | own |
| `MELEE_VS_CITY_DAMAGE_PERCENT` | no Unciv counterpart | own |
| `MIN_GREAT_PERSON_GOLDEN_AGE_TURNS` | no Unciv counterpart | own |
| `NATURAL_WONDER_HAPPINESS` | no named Unciv counterpart (a unique there) | own |
| `PILLAGE_HEAL_HP` | no named Unciv counterpart | own |
| `POLICY_COST_MAX_DISCOUNT_PERCENT` | no Unciv counterpart (no cap there) | own |
| `RANGED_EQUAL_STRENGTH_BASE_DAMAGE` | BattleDamage.kt damageModifier (same formula for ranged) | own |
| `RANGED_EQUAL_STRENGTH_RANDOM_DAMAGE` | BattleDamage.kt damageModifier (same formula for ranged) | own |
| `REBASE_RANGE_PERCENT` | no named Unciv counterpart | own |
| `RESEARCH_AGREEMENT_POOL_DIVISOR` | no Unciv counterpart (Unciv has the treaty, not this split) | own |
| `SAPPED_WALLS_ATTACK_PERCENT` | no Unciv counterpart (a unique there) | own |
| `SAPPER_EFFECT_RANGE` | no Unciv counterpart (a unique there) | own |
| `SCIENCE_PER_CITIZEN` | no named Unciv counterpart (a unique/stat there) | own |
| `SETTLER_MIN_CITY_SIZE` | no Unciv counterpart | own |
| `SHARED_TIES_TOURISM_PERCENT` | no Unciv counterpart (no tourism) | own |
| `UNIT_CAPTURE_MAX_CHANCE` | no Unciv counterpart | own |
| `UNIT_CAPTURE_MIN_CHANCE` | no Unciv counterpart (capture is a unique there) | own |
| `UNIT_CAPTURE_RATIO_PERCENT` | no Unciv counterpart | own |
| `UNIT_COST_EXPONENT_DIVISOR` | no Unciv counterpart | own |
| `UNIT_COST_GROWTH_FACTOR` | no Unciv counterpart (Unciv uses unit supply, ModConstants.unitSupplyPerPopulation) | own |
| `WARMONGER_CITY_STATE_PERCENT` | no Unciv counterpart | own |
| `WOUNDED_DAMAGE_PERCENT` | BattleConstants.kt DAMAGE_REDUCTION_WOUNDED_UNIT_RATIO_PERCENTAGE (300) is the reciprocal form, not the same quantity | own |
