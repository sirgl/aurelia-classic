# Audit of the expansion-only systems

Scope: every rule, constant and formula of a system that exists only in the expansion of the game Classic imitates
(trade units, cultural influence and tourism, great works, ideologies, the World Congress, archaeology) and the parts of
the neighbouring systems (espionage, city-state quests, great people, religion) that the expansion changed. Unciv has no
rule set for these systems (checked on its master, commit `1b5aad5`: no tourism, great work, ideology, congress or
archaeology file; "trade routes" there means the gold of a city connection to the capital).

Classes, applied to the state at the start of this work (`origin/main` 02d56162, column "Before") and to the state now
(column "Now"):

- **a** from Unciv, with file and line of the rule;
- **b** own design with a reason written down (this audit, [BNW-DESIGN.md](BNW-DESIGN.md) and the doc comment of the
  constant), inside a sanity test;
- **c** suspected memory of the original or a number with no explanation;
- **d** missing, stub or never applied.

Method: every `pub const` and every literal of the system files read by hand, every typed effect of the catalogs whose
modifier belongs to a system listed (`apply-own-design.py` tables), every `ResolutionKind`, public function and stat
checked for a caller (three functions and five resolutions had none). Unciv was read from a shallow clone of its master.

## Counts

Entries of sections 1 - 7 (100 rules, constants and formulas):

| Class | Before | Now |
|---|---:|---:|
| a from Unciv | 9 | 11 |
| b own design with a reason | 18 | 89 |
| c suspected memory or unexplained | 66 | 0 |
| d missing, stub, never applied | 7 | 0 |

The seven (d) entries: the missing growth of range with the era (T02), the embargoes in trade (T22), the public-opinion
unhappiness function that nothing called (U14), the conquest relief that conquest never read (U18), and the three
congress resolution groups that had no effect (W10 embargoes, W11 Ban Luxury, W12 fundings). Section 8 counts catalog rows, not rules: of 240 rows (class c before) 14 are now class b and 226 remain class c as a
**known gap that needs the owner's approval** (the report asks for it).

## 1. Trade routes

| ID | Item | Where | Before | Now | Source / reason |
|---|---|---|---|---|---|
| T01 | land range 10, sea range 20 | `derived/trade.rs` | c | b | 12 / 18, plus one tile per two eras |
| T02 | range grows with the era | `derived/trade.rs` | d | b | new `ERAS_PER_RANGE_TILE` |
| T03 | speed 2 on land, 4 at sea | `derived/trade.rs` | c | b | 3 / 5 |
| T04 | target length 30 turns, circuits `max(30 / (2 L / speed), 2)` | `derived/trade.rs` | c | b | 24 turns, circuits rounded between 2 and 8 |
| T05 | route slots = sum of slots x the percent | `derived/trade.rs` | b | b | plus one slot per 4 cities, none without a technology slot |
| T06 | which technologies and wonders give a slot (8 + 2) | catalogs | c | b | one slot per era (Animal Husbandry, Sailing, Currency, Guilds, Economics, Industrialization, Railroad, Computers) |
| T07 | step costs 100 / 50 / enemy water 1000 | `derived/trade_path.rs` | c | b | 100 / 60 (road) / 115 (rough) / 120 (open sea) / +500 |
| T08 | rough ground +1 and oasis +1 tie-break, budget `+99` | `derived/trade_path.rs` | c | b | removed; budget exactly range x 100 |
| T09 | international base 90 gold | `derived/trade.rs` | c | b | 80 + 25 per era |
| T10 | city gold term `300 g / (g + 4000)` | `derived/trade.rs` | b | b | replaced: 18 % of the geometric mean of both cities' gold |
| T11 | resource gold 45 per differing resource, no cap | `derived/trade.rs` | c | b | 25, at most 5 resources |
| T12 | sea +100 %, river +25 % | `derived/trade.rs` | c | b | distance +3 % per tile up to +45 %, sea 150 % |
| T13 | recipient base 90 | `derived/trade.rs` | c | b | 25 + 10 per era + half the city term |
| T14 | science `ceil(difference / 2)` | `derived/trade.rs` | c | b | 0.3 per missing technology, at most 4.5 |
| T15 | internal route 270 + 60 per era | `derived/trade.rs` | c | b | 200 + 45 per era + 15 per citizen |
| T16 | plunder 90 gold, x2 at sea | `derived/trade.rs` | c | b | 60 + 10 per era, x1.5 at sea |
| T17 | flat route gold of Market, Bank, Bazaar, Satrap's Court, Hanse, Colossus, East India Company | catalogs | c | b | rule: +1 per ordinary building, +2 per wonder (rows kept, rule written) |
| T18 | range percent and gold of Compass, Refrigeration, Combustion, Caravansary, Harbor | catalogs | c | b | +15 % (techs), +40 % (buildings), +1 gold |
| T19 | route gold of Wagon Trains, Merchant Confederacy, Treasure Fleets, Economic Union, Iron Curtain | catalogs | c | b | about half of a base route of the era |
| T20 | cost of Caravan 75, Cargo Ship 100 | catalogs | c | b | a Worker's cost (Unciv 70) and 90 |
| T21 | war, capture and razing end a route; combat units plunder | `systems/trade.rs` | b | b | design choice |
| T22 | embargo resolutions | `systems/congress.rs` | d | b | implemented (new `TradeBlock::Embargo`) |
| T23 | a route carries religious pressure at any distance | `systems/religion.rs` | b | b | pressure of an adjacent city (Unciv `Speeds.json`) at any distance |
| T24 | internal routes need Granary / Workshop / Longhouse | catalogs | b | b | design choice |
| T25 | AI: first trade unit at 8 citizens | `ai/trade.rs` | b | b | AI own |

## 2. Tourism, cultural influence and public opinion

| ID | Item | Where | Before | Now | Source / reason |
|---|---|---|---|---|---|
| U01 | culture of a work 2 | `derived/tourism.rs` | c | b | 1 + era / 3 |
| U02 | tourism of a work 2 | `derived/tourism.rs` | c | b | 2 + era / 2 |
| U03 | each tie 20 % | `derived/tourism.rs` | c | b | table 0 / 12 / 21 / 27 |
| U04 | different ideology -30 % | `derived/tourism.rs` | c | b | -25 % |
| U05 | diplomat +20 % | `derived/tourism.rs` | c | b | +15 % |
| U06 | smallest concert 90 | `derived/tourism.rs` | c | b | one level of the audience |
| U07 | levels as percent of lifetime culture (9 / 30 / 63 / 108 / 216) | `derived/tourism.rs` | c | b | quadratic per mille of the culture of the last 8 turns |
| U08 | influence only grows | `systems/tourism.rs` | c | b | forgets 2 % a turn |
| U09 | trend by influence over lifetime culture | `derived/tourism.rs` | b | b | stock against the stock before the last turn |
| U10 | concert 10 turns of tourism, 20 % for the others | units catalog | c | b | 7 turns, 12 % |
| U11 | Writer's treatise: culture of the last 8 turns | `derived/tourism.rs` | a | a | Unciv `PolicyManager.kt` (`cultureOfLast8Turns`) |
| U12 | pressure = difference of level indexes | `derived/tourism.rs` | c | b | continuous difference of influence, cap 4 points a nation |
| U13 | opinion steps 0-2 / 3-4 / 5+ | `derived/tourism.rs` | c | b | 1 / 3 / 6 |
| U14 | opinion causes unhappiness | `derived/tourism.rs` | d | b | function existed, never called; now `from_opinion` of the happiness report |
| U15 | unhappiness table (1, 10), (2, 5), (4, 3) | `derived/tourism.rs` | c | b | (3 + 3 d) % of the citizens, at most 30 % |
| U16 | switch loses 2 tenets | `systems/tourism.rs` | c | b | a quarter, at least one |
| U17 | route science by influence level `level - 1` | `derived/tourism.rs` | c | b | 0.5 per level from Admired |
| U18 | influence spares citizens of a conquered city | `derived/tourism.rs`, `systems/combat/city.rs` | d | b | table existed, conquest used 0; now 10 % per level from Admired |
| U19 | culture victory: Embraced on every living nation | `derived/tourism.rs` | b | b | design choice kept |
| U20 | AI estimate of the culture victory (lifetime culture) | `ai/grand_strategy.rs` | c | b | follows the new model (`turns_to_reach`) |
| U21 | tourism rows: Hotel / Airport 50 %, Visitor Center 100 %, Media Culture 34 %, Internet 100 %, Cultural Exchange 15 %, Futurism 250 %, Religious Art 5, Eiffel Tower 12 | catalogs | c | b | 40 / 40 / 80 / 25 / 80 / 12 / 200 / 4 / 10 |
| U22 | landmarks and wonders turn culture into tourism | `derived/tourism.rs` | b | b | design choice |

## 3. Great works and theming

| ID | Item | Where | Before | Now | Source / reason |
|---|---|---|---|---|---|
| G01 | slots of genre buildings (1) and the Museum (2) | catalogs | c | b | rule: one per genre building, two for the top art building |
| G02 | slots of wonders | catalogs | c | b | by era 1 / 2 / 3 / 4; five wonders changed |
| G03 | theming bonuses and their names | catalogs | c | b | one scheme: diverse n + 1, one age n, one hand n - 1 |
| G04 | guild points 1 / 2 / 3 | catalogs | c | b | 2 each |
| G05 | Writer and Musician specialists: 2 culture, 3 points | catalogs | a | a | equal to Unciv's Artist (`Specialists.json`) |
| G06 | Great Artist golden age 8 turns | units catalog | a | a | Unciv `Units.json` |
| G07 | least golden age 3 turns | `derived/great_people.rs` | c | b | guard value, documented |
| G08 | classes of works, records by rank (battle < camp < ruin < razed city < writing) | `world/tourism.rs` | b | b | design choice |

## 4. Ideologies

| ID | Item | Where | Before | Now | Source / reason |
|---|---|---|---|---|---|
| I01 | choice at the Industrial era or three Factories | catalogs | c | b | two Factories |
| I02 | free tenets for the first and second adopter 2 / 1 | catalogs | c | b | 3 / 1 |
| I03 | tier rule: more tenets of the tier below than of this tier | `derived/culture.rs` | c | b | three tenets of the tier below |
| I04 | numbers of the 48 tenets | catalogs | c | b | budget per tier (see BNW-DESIGN.md section 4) |
| I05 | tenets per tier 7 / 6 / 3 | catalogs | b | b | structure kept |
| I06 | tie-break between ideologies | `roles.ron` | b | b | design choice |
| I07 | tenet names and roles | catalogs | b | b | names are facts, roles kept (art ids depend on them) |

## 5. World Congress

| ID | Item | Where | Before | Now | Source / reason |
|---|---|---|---|---|---|
| W01 | founding: first nation with the technology that met everybody | `systems/congress.rs` | c | b | met at least half of the others |
| W02 | four sessions (eras 3 / 4 / 5 / 6, every 30 / 25 / 20 / 10, 1 / 2 / 3 / 4 delegates) | `systems/congress.rs` | c | b | three sessions, 28 / 21 / 14, 2 / 3 / 4 |
| W03 | two proposers | `systems/congress.rs` | c | b | three |
| W04 | two delegates for the leaders of a failed vote | `systems/congress.rs` | c | b | one |
| W05 | delegates needed for the World Leader | `systems/congress.rs` | a | a | Unciv `VictoryManager.kt` (`votesNeededForDiplomaticVictory`) |
| W06 | tally: most delegates wins, a tie of yes / no is no, host wins election ties | `systems/congress.rs` | b | b | design choice |
| W07 | Standing Army Tax +25 % | `derived/maintenance.rs` | c | b | +20 % (`ARMY_TAX_PERCENT`) |
| W08 | Scholars in Residence +20 % | `classic/systems.rs` | c | b | +12 % (`SCHOLARS_PERCENT`) |
| W09 | Cultural Heritage Sites +3 culture | `systems/congress.rs` | c | b | +2 |
| W10 | City-State Embargo, Player Embargo | - | d | b | no effect before; now forbid and end routes |
| W11 | Ban Luxury | - | d | b | no effect before; now strips the happiness |
| W12 | Arts Funding, Sciences Funding | - | d | b | no effect before; now +25 % / -25 % great person points |
| W13 | Nuclear Non-Proliferation | `classic/systems.rs` | b | b | bans nuclear weapons |
| W14 | project costs 350 / 720 / 850 | catalogs | c | b | 150 x (era - 1): 300 / 600 / 900 |
| W15 | tiers at 50 % and 100 % of the share | `classic/tables.rs` | c | b | 40 % and 90 % |
| W16 | project rewards | catalogs | c | b | see BNW-DESIGN.md section 5 |
| W17 | boost of culture and tourism +100 % | `systems/league_projects.rs` | c | b | +75 % |
| W18 | delegates from allied city-states, diplomats, Forbidden Palace | `systems/congress.rs`, catalogs | c | b | the base is the session seat; Forbidden Palace +2 is one Assembly seat, Globalization +1 per diplomat at work is one more for each (rows unchanged, rule written) |

## 6. Archaeology

| ID | Item | Where | Before | Now | Source / reason |
|---|---|---|---|---|---|
| A01 | sites per nation (5, 8) | `derived/archaeology.rs` | c | b | (4, 6) |
| A02 | hidden 30 %, writing in 30 % of the hidden, at most 15 writings | `derived/archaeology.rs` | c | b | 25 %, 40 %, 10 |
| A03 | weight multipliers 9/8, 11/12, 7/8, 1/5, x3 desert, x3 jungle, x2, x2, "area size x2" | `derived/archaeology.rs` | c | b | 150 % desert and jungle, 85 % owned, 60 % improved, 10 % city |
| A04 | weight of history `(kind + 1)(10 - era)` | `derived/archaeology.rs` | c | b | 6 + 5 per rank + 2 per era back |
| A05 | crowding +1 within 3, +1 within 2, +1 within 1 | `systems/archaeology.rs` | c | b | `5 - distance` within 4 |
| A06 | invented records, eras by weight | `derived/archaeology.rs` | c | b | table kept, reason written |
| A07 | culture boost counts 8 turns | `derived/archaeology.rs` | b | a | equals Unciv's recent-culture window |
| A08 | dig takes 6 turns | `improvements.ron` | c | b | 6 turns: the low end of the ordinary tile improvements of Unciv (`TileImprovements.json`, `turnsToBuild`: Plantation 6, Farm and Mine 7, Pasture and Quarry 8) |
| A09 | hidden sites revealed by a policy | catalogs | b | b | design choice |
| A10 | Archaeologist cost 200 | units catalog | c | b | 175 |

## 7. Neighbouring systems the expansion changed

| ID | Item | Where | Before | Now | Source / reason |
|---|---|---|---|---|---|
| N01 | spy travel 1, revival 5, network 3, intel 125 %, rank rate 25 %, skill 30 %, steal ranks, election gain 20 / loss 5 / neutral 20, coup 50 / 85 / 20 / 10 / 20 | `systems/espionage.rs` | a | a | Unciv `Spy.kt`, `ModConstants.kt`, `CityStateFunctions.kt` |
| N02 | spy for every nation at the Renaissance | `systems/espionage.rs` | c | a | Unciv `Eras.json` (Renaissance era) |
| N03 | one more spy per era from the Industrial era | `systems/espionage.rs` | c | b | own, written down |
| N04 | diplomat introduction 5 turns | `systems/espionage.rs` | c | b | 4 |
| N05 | espionage policy rows (rig election, catch, steal rate) | catalogs | c | b | +75 % each |
| N06 | city-state quests: timings, ranges, weights, rewards | `systems/city_states/quests.rs` | a | a | Unciv `QuestManager.kt`, `Quests.json`, `QuestTargetHelpers.kt` |
| N07 | camp quest range 8 | `systems/city_states/quests.rs` | b | b | Unciv picks a random camp; ours the nearest within 8 |
| N08 | great person points, golden age, trade mission 30 influence | `derived/great_people.rs`, `systems/great_people.rs` | a | a | Unciv `GreatPersonManager.kt`, `Units.json` |
| N09 | prophets stop in the Industrial era | `derived/religion.rs` | a | a | Unciv `Eras.json` |
| N10 | religion pressure, founding, pantheon, prophet costs | `derived/religion.rs` | a | a | Unciv (13 constants, listed in rules.md) |

## 8. Known gap: content tables of the expansion

These are catalog rows (units, buildings, nations, beliefs, promotions), not systems. They were hand-checked in an
earlier pass ("own value") but their origin cannot be shown, so they stay class **c** until the owner decides (see the
open question in the report).

| ID | Rows | Count | Before | Now | What was done |
|---|---|---:|---|---|---|
| K01 | costs of the 7 buildings and 7 world wonders without an Unciv entry | 14 | c | b | each within 80 - 125 % of the median cost of the Unciv buildings / wonders of its era (measured); Hotel 300 -> 500 |
| K02 | policy rows other than tenets (Exploration branch and the expansion-changed policies) | 11 | c | c | not redone |
| K03 | unique units without an Unciv entry (stats, abilities) | 12 | c | c | not redone |
| K04 | units without an Unciv entry (Work Boat, Great Writer, Great Musician, Bazooka, XCOM Squad) | 5 | c | c | not redone (Caravan, Cargo Ship, Archaeologist are in the trade and archaeology tables) |
| K05 | beliefs without an Unciv entry | 14 | c | c | not redone |
| K06 | nations (8) and their abilities, city-states (16) | 24 | c | c | not redone |
| K07 | promotions without an Unciv entry | 135 | c | c | not redone |
| K08 | unique buildings and national wonders without an Unciv entry (Ikanda, Ducal Stable, Royal Library, Candi, Hanse, East India Company, Visitor Center); resources, improvements, natural wonders | 25 | c | c | not redone |
