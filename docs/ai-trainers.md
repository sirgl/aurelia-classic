# The trainers: a library of combat scenes (eras × unit types × tasks, and the 91 tricks)

`ai-exam suite` (`crates/qa/src/trainers.rs`, `trainers/`): short combat scenes, each a function of its index, that an
AI variant (or a script) plays on the learner's seat. Purpose: bring the AI's combat to a reasonable level before the
dreams, and measure every kind of fighting against a reference that plays the task well. A measurement only: it
changes no rule and no AI (`ai-tournament --solo standard --suite standard --speed quick` = `380c45a20e751d31`,
`--solo pre-nets-x` = `3743c16568ce4435`, unchanged).

## Commands

| what | command | time |
|---|---|---|
| everything (proof, controls, players) | `ai-exam suite --out DIR --instances 100 --players standard,unciv --jobs 8` | 190 s on 8 threads (25 families, 177 family × era cells, 17,700 instances) |
| one family, every era | `ai-exam suite --out DIR --families duel --instances 100 --players standard --jobs 4` | 17 s |
| another build on the same scenes | `ai-exam suite --out DIR2 --kept DIR/kept.jsonl --players tac2 --jobs 8` | 44 s for all (no proof) |
| one scene, a trace a round | `ai-exam suite-one --family hold --era medieval --instance 3 --attempt 0 --player standard` | < 0.1 s |
| what a scene is | `ai-exam suite-show --family hold --era medieval --instance 3` | |
| families, eras, solvers, tricks | `ai-exam suite-list`; the era's lab and kinds: `ai-exam suite-lab --era medieval` | |

`DIR/rows.jsonl` holds every play (`info.stage`: `solver`, `passive`, `random`, `player`), `DIR/kept.jsonl` the
attempt kept per instance (or `attempt: null`: unsolved). A run resumes: what DIR holds is not played again. The
tables and Roman's page: the lab's `tools/summary.py` and `page.py`.

## The scene

- **The lab of an era** (`lab.rs`): a generated two-nation game (Small, Pangaea, no city-states, no barbarians, Quick),
  repainted to flat open land, every unit removed, a home city for each nation in a far corner (west: the learner,
  east: the enemy; the enemy's is its capital), the two nations at war, every technology of the era and the eras
  before known to both, 300 gold each. Ground kinds come from the rules' data (open land: the land terrain with no
  defence and the most food; hills: the rules' high ground; impassable land; shallow water; the cover feature with a
  defence bonus).
- **Kinds by era and arm**: melee, mounted, ranged, mounted ranged (archers with 4+ moves), hit-and-run (moves after
  attacking, 4+ moves), siege, city assault (attacks only cities), naval melee and ranged, fighter, bomber, Great
  General; from each kind's role, domain, movement and free promotions. An era's kinds of an arm are the common kinds
  the era brings; an arm it adds nothing to keeps the latest earlier kind it does not make obsolete. Unique kinds are
  used only where a trick names them (the keshik `core:unit/keshik`, the battering ram and siege tower as the era's
  unique city-assault unit, a unique ranged ship 20 % of bombard draws), with a fallback by arm.
- **A family claims an era** when the kinds it needs exist there (`Fam::claims`): kite needs a hit-and-run kind (not
  ancient), siege a siege kind, ram a city-assault kind (ancient: battering ram, classical: siege tower), bombard an era
  that brings its own ranged ship, sink a ship and the embarking technology, airstrike a bomber.
- **An instance** is drawn from `families::seed(family, era, instance, attempt)` alone: positions, compositions,
  clutter (hills and cover), sizes. The same index builds the same scene in every build (checked for the tac2 build).
- **The closed arena**: a unit of either seat that is not one of the scene's pieces (a home city trains or buys one) is
  taken off at every round end. **The map is known to both sides** (in the pilot an unexplored map sent the AI's units
  off to scout).
- **Who plays**: the learner's seat is an AI variant, or a script on top of `standard` (which plays the rest of the
  seat). The enemy's seat is `standard`; in the defence scenes its scene units are a scripted human (`enemy_turn`); in
  duel, focus, hold, kite, retreat, general, wounded the enemy's units are a scripted HUNTER (strike at even odds or
  better, else close on the nearest of ours; ranged units keep at range behind a melee unit; in hold the assault
  commits): the enemy's AI with hunt tasks wandered off the field in the pilot. The learner's AI gets the war objective
  of the target city in the city scenes, and no operation in the field scenes (as the exam's T1).

## The proof that an instance is winnable, and the controls

Fixed before the final run. Attempt 0, 1, ... of an instance (12 at most) is drawn until one is **kept**: it builds,
one of the family's solvers (scripted references, `Fam::solvers`, every one of them plays every attempt it is tried on)
wins it, and the **passive** control (every unit fortifies, or does nothing) does not. An instance with no kept
attempt is **unsolved** and leaves the suite (counted per cell). On the kept attempt the **random** control (each unit
does one thing drawn among standing, fortifying, a reachable move and an accepted strike) and the players play.
Exception by design: the opportunity contexts where either call is right (`prey`, contexts `sea` and `guarded`) are not
filtered by the passive control (holding back wins them).

**Reasonable level** (per family and era, on the kept attempts only, set before the final run after the pilots): a
player is at the reasonable level when it wins at least 70 % of what the best solver wins there and loses at most
1.3 × the best solver's units (+ 0.25). Win rates carry a Wilson 95 % interval on the page (N = the kept attempts, up
to 100).

Guards (`tests/ai_trainers.rs`, slow tier, `.config/nextest.toml`): a scene is a function of its index (the same index
builds and plays the same, another attempt is another scene); the win rules decide what they say; every family builds
in every era it claims; a sample of every family stays solvable (2 of instances 0-3 in its middle era kept).

## The families

Win rules are fixed in the header of `trainers/families.rs` (quoted here). "Lost" counts our scene units dead or
captured, "killed" the enemy's combat units dead.

| family | task | eras | rounds | WIN | solvers | tricks (side) |
|---|---|---|---|---|---|---|
| `duel` | skirmish N vs M on open ground with clutter | ancient, classical, medieval, renaissance, industrial, modern, atomic, information | 12 | every enemy unit dead and one of ours alive within 12 rounds (N of ours, 2-4, against N or, from N = 3, N - 1). | ref-fight, ref-hold, ref-careful | MT4 U |
| `focus` | kill the wounded first: 3 of 4 foes in 4 rounds | ancient, classical, medieval, renaissance, industrial, modern, atomic, information | 4 | 3 enemy units killed within 4 rounds with at most 1 of ours lost (2 of the 4 foes start wounded to 25-70 %). | ref-focus, ref-fight | MT7 U, MT4 U |
| `hold` | hold a mountain pass on hills against 1.5 times the force | ancient, classical, medieval, renaissance, industrial, modern, atomic, information | 10 | (a wall of mountains across the map, a pass three tiles wide on hills) no enemy unit has stood on the pass's middle tile or west of the wall at a round end, and one of ours is alive when 10 rounds end (or every attacker is dead first); the attackers are melee and mounted, one and a half times the defenders (at least 3). | ref-hold, ref-hold-even | MT1 U, MT15 U |
| `choke` | break a human holding a pass (melee on hills, ranged behind) | ancient, classical, medieval, renaissance, industrial, modern, atomic, information | 14 | every holder dead within 14 rounds with at least half of ours alive (the holders are a scripted human: they hold the pass on hills, fortify and shoot, never advance). | ref-choke, ref-fight | MT1 D |
| `bait` | a civilian bait in front of our post, an ambush behind it | ancient, classical, medieval, renaissance, industrial, modern, atomic, information | 12 | 2 ambushers killed with at most 1 of ours lost within 12 rounds (a scripted human leaves a civilian 2 tiles in front of our post, an ambush behind it; from round 4 the ambush comes on). | ref-post, ref-fight | MT2 D, MT3 D |
| `kite` | hit-and-run units (keshik, horseman, cavalry, tanks) against slow melee, no loss | classical, medieval, renaissance, industrial, modern, atomic, information | 14 | every enemy unit dead within 14 rounds with none of ours lost (2 hit-and-run units of the era against 2-3 slow melee of the era before). | ref-kite, ref-kite-careful | MT7 U |
| `guard` | keep civilians (worker, Great General) from raiders | ancient, classical, medieval, renaissance, industrial, modern, atomic, information | 12 | every civilian of ours alive and ours when 12 rounds end (or every raider dead); the raiders are a scripted human that hunts civilians. | ref-guard, ref-guard-strike | MO7 D, E1 D, MT8 D, CS4 D |
| `convoy` | walk a settler past a raider and found a city | ancient, classical, medieval, renaissance, industrial, modern, atomic, information | 14 | a city of ours founded within 14 rounds by the settler (its start is too close to our city to found on); a scripted raider hunts it. | ref-convoy | M2 D, M4 D, MO7 D |
| `raid` | pillage 3 improvements around an enemy city | ancient, classical, medieval, renaissance, industrial, modern, atomic, information | 10 | 3 enemy improvements pillaged within 10 rounds with at most 1 of ours lost. | ref-pillage | MO6 U, MO5 U |
| `counter` | stop raiders pillaging our land | ancient, classical, medieval, renaissance, industrial, modern, atomic, information | 10 | fewer than 2 of our improvements pillaged when 10 rounds end (or every raider dead); the raiders are a scripted human that pillages. | ref-hunt, ref-fight | MO6 D, MO5 D, D3 D |
| `siege` | take a walled city with siege units | classical, medieval, renaissance, industrial, modern, atomic, information | 20 | the city (walls, and castle from the medieval era on, by the rules' buildings) taken within 20 rounds. Measures: rounds to take, units and production lost (quick vs cheap). | ref-siege, ref-press | MO9 U, MO10 U, V2 U |
| `ram` | take the capital with a battering ram or siege tower and a capturer | ancient, classical | 14 | the enemy capital taken within 14 rounds by an army with a city-assault unit (battering ram / siege tower by rules id; fallback the era's siege unit) and a capturer. | ref-siege, ref-press | MO1 U |
| `rush` | hold the capital against an early rush | ancient, classical, medieval, renaissance, industrial, modern, atomic, information | 14 | our capital still ours when 14 rounds end (or every rusher dead); a scripted human rushes it (ranged shoot from 2 tiles, melee takes it). | ref-defend, ref-fight | MO1 D, MO8 D, MO12 D |
| `retreat` | save the army from a force twice its size | ancient, classical, medieval, renaissance, industrial, modern, atomic, information | 10 | at least 75 % of our army alive when 10 rounds end against a force twice its size. | ref-flee |  |
| `flank` | surround and kill a strong defender in 3 rounds | ancient, classical, medieval, renaissance, industrial, modern, atomic, information | 3 | the strong defender killed within 3 rounds with at most 1 of ours lost. | ref-flank, ref-press | MT4 U |
| `general` | fight with a Great General's aura and keep him alive | ancient, classical, medieval, renaissance, industrial, modern, atomic, information | 12 | every enemy unit dead within 12 rounds and our Great General alive. | ref-fight, ref-careful | MT12 U, MT8 U |
| `citadel` | break a holder on a citadel (damage to every adjacent unit) | ancient, classical, medieval, renaissance, industrial, modern, atomic, information | 14 | every holder dead within 14 rounds with at least half of ours alive; the holder stands on a citadel (the rules' improvement that damages adjacent enemies). | ref-choke (melee go in only for a kill below 30 %), ref-fight | MT10 D, MO2 D, MO4 D |
| `bombard` | bombard and take a coastal city with ships | ancient, medieval, renaissance, modern, information | 12 | (3 ranged ships and a melee ship against a city of 2-4 without walls) the coastal city taken, or at 10 % of its hit points or less at a round end, within 12 rounds, by ships. | ref-bombard | N3 U, N4 U |
| `sink` | sink embarked units | classical, medieval, renaissance, industrial, modern, atomic, information | 6 | every embarked enemy unit dead within 6 rounds. | ref-fight | MT5 U, MT6 U, N1 U, N2 U |
| `airstrike` | strike a stack with bombers | modern, atomic, information | 6 | 2 enemy units killed within 6 rounds (bombers based in our city, a melee spotter in front: an air strike needs its target in sight). | ref-air | A1 U, A3 U |
| `prey` | a weak fresh city in reach: take it when the context says so | ancient, classical, medieval, renaissance, industrial, modern, atomic, information | 14 |  | ref-siege, ref-hold | MO8 U, MO1 U, MO11 U |
| `wounded` | a human focus-fires our wounded units: save them | ancient, classical, medieval, renaissance, industrial, modern, atomic, information | 6 | at most 1 of ours lost when 6 rounds end (2 of our 3-4 units start at 30-50 %; the enemy is the hunter, which strikes the weakest it reaches). | ref-flee-wounded, ref-fight | MT7 D |
| `harass` | slow melee and a ranged unit against a human's hit-and-run horse (keshik) | ancient, classical, medieval | 12 | both raiders dead within 12 rounds with at most 1 of ours lost (2 slow melee, a mounted and a ranged unit of ours against 2 horse archers of a scripted human, the keshik half the time, that shoot and step back out of reach). | ref-screen, ref-fight | MO12 D |
| `snipe` | catch an escorted civilian (worker, Great General) of the enemy | classical, medieval, renaissance, industrial, modern, atomic, information | 8 | an enemy civilian (worker, Great General) captured or killed within 8 rounds (it walks to its city with an escort). | ref-snipe | MO7 U, MT8 U, E1 U, M4 U, CS4 U |
| `surrounded` | a strong unit of ours about to be surrounded: break out or survive | ancient, classical, medieval, renaissance, industrial, modern, atomic, information | 4 | our strong unit alive when 4 rounds end, or 2 attackers killed (3-4 melee of a scripted human close round it, then strike). | ref-break, ref-flee | MT4 D |
| `assault-heal` | a worn army takes a city, rotating the wounded out of its fire (home city 10 tiles back) | classical, medieval, renaissance, industrial, modern, atomic, information | 30 | the enemy city (all its defences, 1-2 garrison) taken within 30 rounds with at most half of ours lost; 3 of ours start at 45-70 %. Measures: rounds, lost, damage taken (and the part in the enemy's turn), turns rested. | ref-rotate, ref-planned | MT7 D, MO9 U |
| `promotion-choice` | promotions earned in a fight with a field force, then the assault | classical, medieval, renaissance, industrial, modern, atomic, information | 20 | the city taken within 20 rounds with at most half of ours lost; every unit holds two promotions of its role's ladder and is a few experience points short of a third. Measures: as above and the picks (`picks`). | ref-promo, ref-planned | MT7 U, MT4 U |
| `hill-archers` | ranged units take hills to reach a city whose neighbours shut the view | classical, medieval, renaissance, industrial, modern, atomic, information | 16 | the city taken within 16 rounds with at most 1 of ours lost; its neighbours on our side are hills (and some tiles two away), only a shooter on a hill sees it. Measures: city damage dealt per round, damage taken, rounds. | ref-hills, ref-planned | MO9 U |
| `city-fire` | invest a strong city without feeding its strike | classical, medieval, renaissance, industrial, modern, atomic, information | 14 | the city taken within 14 rounds with at most 1 of ours lost; open ground, all defences, one garrison. Measures: damage taken in the enemy's turn before the capture, lost, rounds. | ref-fire, ref-planned | MO9 U |
| `planned-assault` | the composite: hills, a worn army, a home city, promotions | classical, medieval, renaissance, industrial, modern, atomic, information | 30 | the city taken within 30 rounds with at most 1 of ours lost. Measures: all of the above. | ref-planned, ref-rotate | MO9 U, MT7 U |
| `assault-reinforce` | take the city before the enemy's reinforcements land, at any cost in units | classical, medieval, renaissance, industrial, modern, atomic, information | 6-8 | the assault-heal ground and forces, the army at the edge of the city's strike; at round N (6-8, drawn from the instance alone: the scene's `deadline` is N - 1) 2-3 of the era's strongest melee of the enemy land in and next to the city. Win: the city taken by round N - 1; NO loss cap; the play ends at N. | ref-burst, ref-planned | MO9 U, MO10 U |
| `assault-peace` | take the city, the war's goal, before the peace, at any cost in units | classical, medieval, renaissance, industrial, modern, atomic, information | 8-10 | the planned-assault composite with a fourth melee unit, the army at the edge of the city's strike; the peace comes at round N (7-9, drawn from the instance alone; `deadline` = N). Win: the city taken by round N; NO loss cap; the play ends at N. | ref-burst, ref-planned | MO9 U, MO10 U |

The references (`families::reference_turn`, built on `act::unit_turn`): **ref-fight** (ranged keep at range behind a
melee unit and strike the best blow by the rules' previews: a kill first, then the most wounded, the units that capture
before the others; melee strike at even odds or better, move to strike when a blow lands from a reachable tile, else
close in), **ref-careful** (the same at 2:1 odds), **ref-hold** (hold the best defensive tile, fortify), **ref-focus**,
**ref-choke** (ranged soften the pass, melee wait two tiles off until a holder is below 60 % or the pass is empty),
**ref-post** (never leave the post for the bait), **ref-kite** (shoot from the edge of range and step out of every
foe's reach), **ref-guard** (the civilian walks to its city, the escort stands on its tile), **ref-convoy**,
**ref-pillage** (the nearest unpillaged improvement outside the city's strike), **ref-hunt**, **ref-siege** (siege and
ranged on the city and the units by it, melee wait 3 tiles off until the city is at half), **ref-press** (everything
at the city at any odds), **ref-defend**, **ref-flee**, **ref-flank** (surround, then strike at any odds),
**ref-bombard**, **ref-air** (bombers strike the weakest in range; a melee spotter stands in front, an air strike
needs its target in sight), **ref-flee-wounded**, **ref-screen** (melee guard the ranged unit, our horse runs the
horse archers down), **ref-snipe**, **ref-break**. The assault references (`assault.rs`, one planner with switches: stage, tiles, rotate, promote,
burst): ref-fire, ref-hills, ref-rotate, ref-promo, ref-planned (all but burst) and **ref-burst** (stage for at most 2
turns, then every unit strikes the city every turn and none leaves to heal: the shooters from where they stand or the
nearest firing tile they can fire from this turn, not next to the city; every melee unit the best blow it survives by
the preview; the last melee unit alive waits two tiles off, next to the city once it is down to a third, and takes it at
the first blow that can). ref-burst pays units for time: the reference of the "at any cost" families, where ref-planned
is the careful control.

## The 91 tricks

`Fam::tricks` maps each family to the tricks it stands for, with the side: U (the learner may use it), D (a scripted
human uses it against the learner). Combat tricks with a scene on both sides: 12; on one side: 21; no scene: 58 (the
non-combat ones stay in ai-stress, the rest are not applicable to the rules or not built, with the reason).

| trick | what | U: scenes | D: scenes | why no scene |
|---|---|---|---|---|
| MT1 | Hold a chokepoint (hill, fort, citadel, ZOC pinch) with a few melee units and ra | hold | choke |  |
| MT2 | Poke an AI city or unit with a cheap bait so the AI sends its army into your pre | – | bait | U not built: a lure scene (the AI baits a human) |
| MT3 | Worker (or other civilian) as bait: move a worker next to the AI's fortified uni | – | bait | U not applicable (triage: the rule is in the lure) |
| MT4 | Kill the AI melee units first; the AI wave then stops. | duel, focus, flank | surrounded |  |
| MT5 | Shoot units that the AI embarks in the middle of a battle or on coastal routes;  | sink | – | D not built: the AI embarked under a human fleet (next step) |
| MT6 | Ranged fire across lakes or water at units that cannot reply. | sink | – | D not built (as MT5) |
| MT7 | Focus-fire wounded or retreating units; wound a unit and leave it alive so the A | focus, kite | wounded |  |
| MT8 | Snipe unescorted Great Generals and other lone high-value units. | general, snipe | guard |  |
| MT9 | Peacekeeper blockade: under open borders, park workers or weak units around a ci | – | – | needs open borders at peace: ai-stress P3 (sneak-stack) |
| MT10 | Fort/citadel damage zone: put a citadel two tiles from an AI city; every unit ne | – | citadel | U not built: the AI building a citadel with its general |
| MT11 | Use civilians as spotters for siege units. | – | – | not applicable: joke source, no mechanism (triage) |
| MT12 | Great General stacking plus instant heal by finishing a city with near-promotion | general | – | D: a human general stack against the AI (next step) |
| MT13 | Deny healing: stand next to a wounded AI unit so it cannot heal in place. | – | – | not applicable: an Unciv-code fact (triage) |
| MT14 | Over-commit trigger: with 5 or more units near an AI target city the AI drops it | – | – | not applicable: an Unciv-code fact (triage) |
| MT15 | Strong high-damage defenders deter AI melee: the AI will not attack a city if th | hold | – | D: as MT1 D (choke) |
| MO1 | Early capital rush: 2-4 archers and a couple of warriors, march to the AI capita | ram, prey | rush |  |
| MO2 | Forward settle next to the AI border, buy tiles, drop a Great General citadel on | – | citadel | U not built (citadel by the AI) |
| MO3 | Sneak attack: move an army in under open borders, surround a city, then declare  | – | – | non-combat: stays in ai-stress (whole-game positions) |
| MO4 | Take land by citadel after peace; the AI treats it as weak or no casus belli. | – | citadel | U deliberately not built (rules gap, CHAIN.md) |
| MO5 | Cut an AI luxury or strategic resource with a fort or citadel after a turn of wo | raid | counter |  |
| MO6 | Pillage raids with fast units. | raid | counter |  |
| MO7 | Steal or snipe unescorted settlers and workers; hover on a hill near a contested | snipe | guard, convoy |  |
| MO8 | Wait and take: let the AI settle and grow new cities, then conquer them quickly. | prey | rush |  |
| MO9 | Take good cities from the AI instead of founding your own (Deity advice). | siege | – | D: as rush (the AI holds its city) |
| MO10 | Attack before the AI modernises: outdated garrisons fall to upgraded units; or w | siege | – | D: as rush with a later-era rusher (next step) |
| MO11 | Turtle with a minimal army; the AI does not attack a weak target. | prey | – | D: whole-game (ai-stress A5) |
| MO12 | Pre-built offensive army and a lopsided army composition that the AI cannot coun | – | rush, harass |  |
| MO13 | Punish Piety-opening AIs: many AIs choose Piety (and Honor) early and neglect th | – | – | non-combat: stays in ai-stress (whole-game positions) |
| D1 | Sell a luxury for a lump sum, then declare war and keep the gold. **On list (var | – | – | non-combat: stays in ai-stress (whole-game positions) (diplomacy, deals) |
| D2 | Gold-per-turn deal then a war that breaks it; or drain the AI treasury with GPT/ | – | – | non-combat: stays in ai-stress (whole-game positions) (diplomacy, deals) |
| D3 | Pillage-and-repair the sold resource, or sell a resource just before barbarians  | – | counter |  |
| D4 | Bribe AI A to declare war on civ B, then declare war on A (two-front war at no c | – | – | non-combat: stays in ai-stress (whole-game positions) (diplomacy, deals) |
| D5 | Pay an AI to attack a one-city civ, then take the cities and liberate the one yo | – | – | non-combat: stays in ai-stress (whole-game positions) (diplomacy, deals) |
| D6 | Short war / peace cycles to re-sell resources sooner than waiting out the 30-tur | – | – | non-combat: stays in ai-stress (whole-game positions) (diplomacy, deals) |
| D7 | Peace shopping: peace with one AI of a team brings peace with all; the AI does n | – | – | not applicable: no teams in the rules |
| D8 | White-peace fishing: remove the cities the AI "demands" from the screen; the AI  | – | – | non-combat: stays in ai-stress (whole-game positions) (diplomacy, deals) |
| D9 | Wear down the AI army so it proposes peace; refuse the first offers so it offers | – | – | non-combat: stays in ai-stress (whole-game positions) (diplomacy, deals) |
| D10 | Attack right after a peace treaty expires. | – | – | non-combat: stays in ai-stress (whole-game positions) (diplomacy, deals) |
| D11 | Tech blocking with cheap research agreements; sign research agreements with many | – | – | non-combat: stays in ai-stress (whole-game positions) (diplomacy, deals) |
| D12 | Sell open borders for gold, or buy them cheap from a friendly AI (Unciv sells at | – | – | non-combat: stays in ai-stress (whole-game positions) (diplomacy, deals) |
| D13 | Declare war once to "meet" an AI and unlock bribes against it. | – | – | not applicable: war needs contact |
| D14 | Deny a strategic resource (iron) before war so the AI cannot rebuild swordsmen. | – | – | non-combat: stays in ai-stress (whole-game positions) (diplomacy, deals) |
| D15 | Keep the AI busy: war against others lowers its appetite for a new war (Unciv -2 | – | – | not applicable: Unciv MotivationToAttack |
| D16 | Break an Unciv demand once, then repeat it freely for 100 turns. | – | – | not applicable: Unciv popup flag |
| D17 | Unciv AI war timing is predictable: it needs at least as many units as cities, p | – | – | not applicable: Unciv deterministic rolls |
| D18 | Deterrence: keep military strength high vs. AIs so fewer declare war at once. | – | – | non-combat: stays in ai-stress (whole-game positions) (diplomacy, deals) |
| E1 | Steal workers from city-states ("worker shops") or other civs instead of buildin | snipe | guard |  |
| E2 | Luxury arbitrage: buy luxury tiles early and resell the resources repeatedly; Un | – | – | non-combat: stays in ai-stress (whole-game positions) |
| E3 | Cheap late tech from the AI: Unciv prices tech by square root of cost. | – | – | not applicable: no tech item in deals |
| E4 | Early National College and Academies as a human science lead the AI does not mat | – | – | non-combat: stays in ai-stress (whole-game positions) |
| E5 | Build wonders the AI cannot (Circus Maximum in a secondary city, Pyramids for wo | – | – | non-combat: stays in ai-stress (whole-game positions) |
| R1 | Convert AI cities with prophets or missionaries; an early prophet into a freshly | – | – | non-combat: stays in ai-stress (whole-game positions) |
| R2 | Capture the holy city, then purge the rest with missionary and inquisitor. | – | – | non-combat: stays in ai-stress (whole-game positions) |
| R3 | Re-convert city-states the AI flipped, with a prophet. | – | – | non-combat: stays in ai-stress (whole-game positions) |
| R4 | Convert AI cities away so the AI loses the shared-religion tourism bonus on you. | – | – | non-combat: stays in ai-stress (whole-game positions) |
| C1 | Block the AI's tourism: steal its dig sites, withhold open borders and trade rou | – | – | non-combat: stays in ai-stress (whole-game positions) |
| C2 | Conquer a neighbour to erase its accumulated culture for a cultural win. | – | – | non-combat: stays in ai-stress (whole-game positions) |
| C3 | Kill the culture leader early, before it becomes influential. | – | – | non-combat: stays in ai-stress (whole-game positions) |
| S1 | Park a spy early in an AI capital and steal repeatedly. | – | – | non-combat: stays in ai-stress (whole-game positions) |
| S2 | Let AI spies in and kill them (Double Agents) to level your own spies; AI spies  | – | – | non-combat: stays in ai-stress (whole-game positions) |
| S3 | Coup a city-state held by an AI ally; spend gold to shrink the influence gap fir | – | – | non-combat: stays in ai-stress (whole-game positions) |
| S4 | Use spies as diplomats to swing World Congress votes. | – | – | not applicable: diplomats carry no votes |
| CS1 | Pledge to protect plus consulates: near-permanent friendship for little effort;  | – | – | non-combat: stays in ai-stress (whole-game positions) |
| CS2 | Feed advanced units to a city-state ally the AI attacks. | – | – | non-combat: stays in ai-stress (whole-game positions) |
| CS3 | Bribe the AI team to war on city-states (spreads city-state hatred toward the AI | – | – | non-combat: stays in ai-stress (whole-game positions) |
| CS4 | Steal a city-state worker by parking next to the tile it wants to improve; repea | snipe | guard |  |
| CS5 | War-peace cycles with city-states, shooting in between; recapture city-state wor | – | – | non-combat: stays in ai-stress (whole-game positions) |
| CS6 | Early influence lead snowballs; final alliance sweep wins a diplomatic or cultur | – | – | non-combat: stays in ai-stress (whole-game positions) |
| CS7 | Finish AI units to complete city-state missions for a free alliance. | – | – | non-combat: stays in ai-stress (whole-game positions) |
| CS8 | Unciv: AI pledges to every city-state, bullies only at Peace and only those it v | – | – | not applicable: Unciv AI fact |
| W1 | Sell your "core" delegates to an AI on its proposal, then vote the rest against  | – | – | not applicable: no delegate item in deals |
| W2 | Buy city-state loyalty right before a vote; embargo the leader, ban its luxuries | – | – | non-combat: stays in ai-stress (whole-game positions) |
| W3 | Take the host seat and push your own agenda: world religion, ideology, ban resol | – | – | non-combat: stays in ai-stress (whole-game positions) |
| W4 | In the World Leader vote the AIs split their votes for themselves, so a human ge | – | – | non-combat: stays in ai-stress (whole-game positions) |
| N1 | Sink embarked units with ships. | sink | – | D not built (as MT5) |
| N2 | Keep ships at sea and let AI armies "jump in the water"; they lose full stacks i | sink | – | D only by design (not a trick to use) |
| N3 | Bombard AI cities with destroyers; subs ignored by destroyers. | bombard | – | D not built: a human fleet bombards an AI city |
| N4 | Block AI harbours with cheap units, put ranged units in the path of its fleet. | – | – | not built: harbour block (next step: a coastal city and a blockading ship) |
| N5 | Sink destroyers with land artillery before the navy can act. | – | – | not built: artillery against ships (next step) |
| A1 | AI builds almost no air units unless a unique unit; fighters and bombers face li | airstrike | – | D not built: human bombers against the AI (next step) |
| A2 | Nuke or destroy the final spaceship part as it nears the capital; AI does not ai | – | – | non-combat: stays in ai-stress (whole-game positions) |
| A3 | Use nukes against the AI as a routine tool. | airstrike | – | not built: nukes (a deterrence scene needs the nuclear AI and a city to lose; next step) |
| V1 | Snipe a wonder 1-2 turns before the AI completes it. | – | – | non-combat: stays in ai-stress (whole-game positions) |
| V2 | Take the AI city that holds the wonder instead of racing. | siege | – | D: as rush |
| V3 | Stop an AI space race by destroying parts en route, or by taking all production  | – | – | non-combat: stays in ai-stress (whole-game positions) |
| V4 | Win diplomatic or cultural before the AI launches (AI launch timing is the main  | – | – | not applicable: Civ IV timing advice |
| M1 | Settle the gaps between AI cities (filler cities, tile buying). | – | – | non-combat: stays in ai-stress (whole-game positions) |
| M2 | Ninja-settle: body-block AI settlers near your border; keep a unit on a hill for | – | convoy | U not applicable (triage) |
| M3 | Delay accepting embassies and friendship so the AI cannot forward-settle into yo | – | – | not applicable: no embassy-to-settling link |
| M4 | Intercept AI settlers heading to distant poor spots, or kill border cities the A | snipe | convoy |  |
| M5 | Stop city-spamming AIs early to cap their expansion. | – | – | non-combat: stays in ai-stress (whole-game positions) |

The assault references (`trainers/assault.rs`, one planner with four switches; they play only by the rules' previews,
data and the public state): **stage** (no unit enters the city's strike, range 2, until every attacker can reach its
firing tile or an adjacent tile in one move; they go on the same turn, so the city strikes once with one target at a
time at most); **tiles** (a shooter takes, of the tiles it reaches with a clear line and in range, one outside the
strike, else the best defence, else the least threatened); **rotate** (a unit under 70 hit points leaves for the
nearest friendly land out of the city's reach, rests without moving, and returns at 90); **promote** (the promotion
with the highest plan value from the rules' effects: city attack for the blows, defence against ranged for the line,
healing for an army with no friendly land). ref-fire = stage; ref-hills = stage + tiles; ref-rotate = stage + tiles +
rotate; ref-promo = stage + tiles + promote; ref-planned = all four. What makes them good by construction: the city's
strike is once a turn at the weakest-defence tile within 2, so staging and going together bounds its damage to one
strike a turn, and the burst on the city (it heals 20 a turn) is kept shorter than its healing.

Part 2 adds these measures to every row of the five families: `dmg_city` (damage taken from the city's strike),
`dmg_own`, `dmg_total`, `city_dealt`, `dealt_per_round`, `rested`, `picks` (the promotion picks, in `info`).

## First measurement (22 November 2026, `runs/final`, 100 instances a cell)

Means over the family's cells with 10 or more kept instances; win rates on the kept attempts; tac2 is the build of
`work/tac` 48dc5cf (its own `standard` as the enemy; that build's `standard` on the learner's seat equals the main
build's in every cell).

| family | cells | kept / drawn | passive % | random % | best solver % | standard % | tac2 % | unciv % |
|---|---|---|---|---|---|---|---|---|
| `duel` | 8 | 800/800 | 0 | 0 | 90 | 9 | 9 | 52 |
| `focus` | 8 | 800/800 | 0 | 0 | 100 | 6 | 6 | 91 |
| `hold` | 8 | 589/800 | 0 | 1 | 83 | 0 | 0 | 33 |
| `choke` | 7 | 612/700 | 0 | 0 | 84 | 0 | 0 | 78 |
| `bait` | 8 | 768/800 | 0 | 7 | 96 | 0 | 0 | 84 |
| `kite` | 7 | 660/700 | 0 | 0 | 100 | 0 | 0 | 46 |
| `guard` | 8 | 800/800 | 0 | 0 | 100 | 31 | 34 | 15 |
| `convoy` | 8 | 791/800 | 0 | 16 | 100 | 76 | 77 | 86 |
| `raid` | 8 | 800/800 | 0 | 0 | 100 | 0 | 0 | 0 |
| `counter` | 8 | 655/800 | 0 | 0 | 92 | 0 | 0 | 34 |
| `siege` | 7 | 643/700 | 0 | 0 | 87 | 42 | 22 | 54 |
| `ram` | 2 | 200/200 | 0 | 0 | 100 | 30 | 16 | 54 |
| `rush` | 8 | 714/800 | 0 | 20 | 97 | 72 | 60 | 11 |
| `retreat` | 8 | 641/800 | 0 | 1 | 100 | 20 | 28 | 1 |
| `flank` | 8 | 749/800 | 0 | 0 | 95 | 0 | 0 | 57 |
| `general` | 8 | 800/800 | 0 | 0 | 94 | 0 | 0 | 62 |
| `citadel` | 8 | 714/800 | 0 | 0 | 88 | 0 | 0 | 24 |
| `bombard` | 3 | 292/300 | 0 | 0 | 100 | 0 | 0 | 0 |
| `sink` | 7 | 700/700 | 0 | 2 | 100 | 40 | 40 | 98 |
| `airstrike` | 3 | 140/300 | 0 | 0 | 100 | 14 | 14 | 45 |
| `wounded` | 8 | 800/800 | 0 | 26 | 100 | 87 | 87 | 20 |
| `harass` | 2 | 200/200 | 0 | 0 | 100 | 0 | 0 | 48 |
| `snipe` | 7 | 700/700 | 0 | 0 | 100 | 0 | 2 | 60 |
| `surrounded` | 8 | 800/800 | 0 | 15 | 98 | 61 | 61 | 0 |
| `prey-either` | 8 | 730/730 | 100 | 95 | 100 | 43 | 93 | 8 |
| `prey-land` | 4 | 60/60 | 0 | 0 | 100 | 68 | 59 | 0 |

Cells with no solver for most instances (generator or solver to improve, the instances are out of the suite): choke
modern 14/100 and atomic 0/100, bombard modern and information 0/100 (the era's ships do not scratch its cities),
retreat information 12/100, airstrike atomic 25/100, harass medieval 1/100 (no script catches keshiks with foot and
one horse).

Roman's page: a local page outside the repository.

## Part 2 measurement: the planned assault (23 November 2026, `runs/t2`, `runs/t2-tac2`, 100 instances a cell)

Seven eras a family (classical to information: the ancient era has no siege unit). Win rates on the kept attempts, all
cells together; "best solver" is on all drawn attempts (the proof keeps the attempts the solver wins, so the kept
rate is 100 % by construction); tac2 is work/tac a2a1b36 plus this module (`tac2-src-b`, `target-tac`).

| family | kept / drawn | passive % | random % | best solver % (drawn) | standard % | tac2 % | unciv % |
|---|---|---|---|---|---|---|---|
| `assault-heal` | 700/700 | 0 | 0 | 99 | 40 | 46 | 59 |
| `promotion-choice` | 700/700 | 0 | 0 | 81 | 26 | 27 | 62 |
| `hill-archers` | 700/700 | 0 | 0 | 66 | 33 | 25 | 20 |
| `city-fire` | 700/700 | 0 | 0 | 95 | 42 | 30 | 31 |
| `planned-assault` | 690/700 | 0 | 0 | 43 | 28 | 13 | 21 |

Findings: standard and tac2 lose assault-heal and promotion-choice mainly by not resting the wounded and not staging
the approach (see the traces on the page); in hill-archers and planned-assault the references are still weaker than
the build's `standard` on the drawn attempts (the scenes are solvable only by redraw): these two references need
more work (the hills' line of sight and the burst against the city's 20 a turn healing). Row measures: `dmg_city`,
`dmg_own`, `city_dealt`, `dealt_per_round`, `rested`, `picks`. Traces: `runs/t2-traces/` (`tools/trace.sh`).

## Fast mode (23 November 2026, `runs/fast-*`, 30 instances a cell, every family, standard and unciv)

The trainers had no whole-game prefix to cut: the lab is built synthetically (a repainted Small world, a home city
each, round 0), and the profile (`TRAINERS_PROF=1`, `sample`) shows 90 % of a play inside `driver.step`, 73 % of that
in the AI's own siege planner (`ai::siege_walls::plan_turn` -> `evaluate` -> `find_path`, an A* that already carries
a node limit and prices one step with `PlayerView::tile`), not in `validate_move`. Switches (all default to the
old behaviour; set by `ai-exam suite`/`suite-one` options):

| option | what | whole suite (6 threads, proof included) | plays/s | agreement with the default (Wilson 95 %, 60 cells: 30 families x standard, unciv) |
|---|---|---|---|---|
| default (`--world small --fog off --ai full`) | the whole map shown to both sides, the whole AI | 129 s | 450 | - |
| `--world duel` | a 40 x 24 lab (960 tiles) instead of 66 x 42 | 92 s | 622 | 6 of 60 differ: convoy, guard, surrounded (standard); convoy, flank, sink (unciv): the home cities' distance is part of those scenes |
| `--ai combat` | wrapper `CombatOnly`: only the decisions of war and the army (moves, attacks, fortify, strikes, promotions, declarations) are kept; research, production, city management, policies, religion, trade and peace are dropped | 126 s | 442 | 1 of 60 differs (unciv, ram); no saving: the cost is in the planning (`begin_turn`, siege planner), not in the decisions dropped; a saving needs a change inside the AI, outside this measurement-only branch |
| `--world duel --ai combat` | both | 91 s | 616 | 12 of 60 differ |
| `--fog on` | each side sees what its units and cities see (the game's visibility), no `reveal` | 95 s | 595 | 43 of 60 differ: the AI blind plays another game (the references and the scenes' aims use the true map) |
| `--world duel --fog on` | both | 89 s | 631 | 38 of 60 differ |

One family (city-fire medieval, 100 instances, one thread): 12.5 s default, 5.0 s fog, 6.7 s duel, 4.8 s duel + fog,
12.6 s combat (the first, stricter filter of `combat` changed the outcome: standard 35 % to 0 %, because it dropped the
operations' orders; the kept filter gives 35 % as the default). Early stop was already there: `judge` ends a play at
the round the outcome is decided (city taken, one side gone, the convoy through).

Verdict: the defaults stay (open map, full AI, Small lab): fog is a different measurement (use it to measure the AI's
scouting and its blindness, not combat tactics), `duel` is safe for the families whose win rate agrees (everything but
convoy, guard, surrounded, flank, sink) at 1.4 x, `combat` is equivalent but not faster until the AI's planner is
bounded (the siege planner's path searches allocate a scratch of the map's size every call: an exact fix, with hashes
to check, in `ai::siege_walls`).

## Citadel correction (23 November 2026, `runs/cit5`)

Found by the MLX parity lane: the citadel stood on neutral land and damaged nobody (the zone hurts only units next to a
citadel on land owned by an enemy at war, `citadel_damage`; `debug_raise_citadel` gives the land only to an owner's
city within 5 tiles, `CITADEL_CITY_REACH`). The scene now founds a small town of the holders 5 tiles behind the pass,
raises the citadel from it (`debug_raise_citadel`), and checks the pass tile is the holders'. The attackers are mostly
shooters; ref-choke sends melee in only for a kill. Test `the_citadel_stands_on_the_holders_land`. Re-proved and
re-measured (100 instances a cell, seven eras; the ancient era also builds): kept 679/800 (was 714/800); passive 0 %,
random 0 %, standard 0 % (was 0 %), tac2 0 %, unciv 26 % (was 25 %); ref-choke 8 % and ref-fight 9 % on the drawn
attempts (the proof keeps those they win). The first-measurement row of `citadel` above is superseded.

## Targets for the hill-archers and planned-assault references (written 23 November 2026, before the runs)

The reference must win at least 70 % of all drawn attempts in the family, or at least 1.5 times the win rate of
`standard` on the kept attempts (all cells together, 100 instances a cell). Measured with `runs/t3` after the
improvement; the result is written in the section below.

## Improvement of the references (23 November 2026, `runs/t3`, `runs/t3-tac2`, 100 instances a cell)

Target (written before the runs, above): the reference wins at least 70 % of all drawn attempts or at least 1.5 times
`standard` on the kept attempts. Causes found by trace and fixed in the one planner `assault::turn`:
1. The readiness deadlock: with fewer firing tiles (hills round the city) than shooters, a surplus shooter was never
   "ready", so the army waited the full 12 rounds and went after the city had bought its defences (hill-archers
   medieval: the assault began at round 13). Now the army goes when as many shooters can arrive as there are tiles
   (the rest are a reserve), and waits at most 4 rounds for a resting unit and 5 for the staging.
2. The rotation took shooters out of the burst at 70 hit points, and the city heals 20 a turn: a shooter in the burst
   now leaves below 45 (`ROTATE_BURST`; the city's strongest strike takes about 90), melee waiting still at 70.

| family | reference (all drawn attempts) before | after | standard (kept) | ratio |
|---|---|---|---|---|
| `hill-archers` | ref-hills 22 %, ref-planned 11 % | ref-hills 57 %, ref-planned 66 % | 33 % | 2.0 |
| `planned-assault` | ref-planned 25 % | ref-planned 43 % | 28 % | 1.5 |

Both meet the second clause (planned-assault only just). tac2 13-25 %, unciv 20-21 % on the two. The other three
families were re-proved with the same planner (assault-heal 99 %, promotion-choice 81 %, city-fire 95 % on the drawn
attempts); the table above is the new one. The first part-2 run is `runs/t2`.

## The "at any cost" families (23 November 2026, army-net clone `runs/fam-anycost`, 100 instances × 7 eras a family)

The proof as asked before the runs: `ref-burst` wins >= 70 %, `ref-planned` (the careful rotation) <= 30 %, the passive
control loses. Kept attempts (kept file `data/kept-anycost.jsonl` of the army-net clone), win / capture / units lost a play:

| family | kept | ref-burst | ref-planned | passive | held out 80-99: ref-burst | ref-planned | standard | unciv |
|---|---|---|---|---|---|---|---|---|
| `assault-reinforce` | 696/700 | 99.4 % / 99.4 % / 2.10 | 9.9 % / 23.3 % / 0.44 | 0 % | 98.6 % / 2.15 (N 139) | 12.2 % / 30.2 % / 0.51 | 51.8 % / 54.7 % / 2.01 | 48.2 % / 53.2 % / 2.97 |
| `assault-peace` | 677/700 | 98.8 % / 98.8 % / 3.18 | 12.0 % / 12.0 % / 0.72 | 0 % | 97.9 % / 3.21 (N 140) | 18.6 % / 18.6 % / 0.72 | 65.0 % / 65.0 % / 1.64 | 40.0 % / 40.0 % / 3.36 |

The kept attempts are selected (a solver won them). Unselected, on attempt 0 of every instance: assault-reinforce
ref-burst 80.7 %, ref-planned 9.3 %; assault-peace ref-burst 57.0 %, ref-planned 7.1 %. The scene was tuned in 7
quick runs (attempt 0, 15 instances) and 2 full ones: the army starts at the edge of the city's strike, the burst stages for at most
2 turns, its shooters fire from where they stand, the last melee unit keeps itself for the capture; assault-peace has
a fourth melee unit, five worn units and N 7-9 (with N 8-10 ref-planned won 38 % held out, 52 % at N 10). The older
families play bit-identically (1493 rows of 8 families, instances 0-2, solvers and standard, against the build before).

## Limits

- Scripts are simple and competent, not optimal: the best solver's win rate is a floor of what is winnable. A
  search solver (`search.rs`) on these scenes is the next step for the cells where the references are weak.
- The scenes are short and closed (no reinforcements, no economy): they test tactics, not the operations that put
  units in the field. In the field scenes the learner's AI has no operation.
- Not built: nukes, the AI's own citadels, harbour blocks, a human fleet or bombers against the AI, the AI's lure.
