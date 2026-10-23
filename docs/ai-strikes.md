# The AI's heavy arms: missiles, nuclear weapons, carriers, landings and paratroopers

**Missiles, nuclear weapons, carriers and paratrooper jumps are part of the game's AI `standard` since 23 October 2026 (the owner's decision: "turn everything on"); the former `standard` is the control `previous`, and `strikes` and `landings` are its two thirds for attribution. A nuclear strike is a public act: every civilization that met the user remembers it (the existing warmonger memory, the human nuker pays the same), and the AI's strike value, its price of peace and its war value read that memory. The war operation counts the weapons the AI owns or can build. Paired strength against `previous` is identical on every suite (the result hashes of the games are the same wherever no weapon is used), the pins did not move, the time of a round did not change. The behaviours are fixed by scenes and occur in at most 3 of 100 bench games, because the technologies are not reached in the normal suites and wars are rare.** State of 2026-10-23, branch `publish/ai-fixes`. Labels: [measured] = measured by me in this session.

Code: `ai/strikes.rs` (weapons), `ai/carrier.rs` (carriers), `ai/operations.rs` (`muster_jump`), `ai/fleet.rs` (dispatch), `ai/planner_cities.rs` (`weapon_orders`, `carrier_orders`), `ai/variant.rs`. Tests: `tests/ai_strikes.rs` (13), `tests/ai_carriers.rs` (2), `tests/ai_landings.rs` (3), `tests/ai_war_arms.rs` (2), `tests/nuclear_infamy.rs` (4), `aurelia-qa/tests/non_interference.rs::the_heavy_arms_of_the_standard_ai_do_not_see_the_hidden_world`. Measurement tools: `aurelia-qa/examples/ai_behaviours.rs`, the bench suite `arms`.

Rules of the design (the owner's): generic and driven by ruleset data (no unit, technology or building is named in the logic), deterministic, inside the decision budget, observation only (the twin-world tests stay green), no cheat switches (no price, cap or multiplier of the AI differs).

## What the data says a weapon is

| Class | Rule (all from the unit definition) |
|---|---|
| Missile | air domain, `suicide`, `nuke_level` = 0, a ranged strength and a range |
| Nuclear weapon | air domain, `suicide`, `nuke_level` > 0, a range (the level 1 or 2 is the damage class) |
| Carrier | sea domain, `combat.carries` names a cargo kind |
| Cargo | air domain, `combat.carried_as` is that kind (strikers, fighters, missiles and nuclear weapons alike) |
| Paratrooper | a land unit whose promotions give the stat `ParadropRange` (asked of the rules: `PlayerQueries::paradrop_range`) |

The existing air arm (`ai/air.rs`) leaves one-way missiles and nuclear weapons uncommanded on purpose; the generic combat advisor fired a missile the AI happened to own at the first target with a positive preview (a lone scout included), and no city ever built one (`unit_of_role(Military)` ranks by melee strength, a missile has none). In `strikes` the advisor no longer shoots with weapons.

## Value model (hundredths of a score point, `ai/value.rs`)

A weapon is **built** when the best prospect in its reach pays its whole cost, the project it needs included, and **fired** when the best strike pays 30 % of its replacement cost (`FIRE_PERCENT`; the hammers are spent, waiting has a price too: the target moves, the war ends).

| Target | Gain of a strike |
|---|---|
| enemy unit (missile) | its cost in hammers at the production price, the share of its hit points the rules' preview promises, half counting for us (`RIVAL_PERCENT`, the war model's own rival share), twice that for a unit near our cities or an operation |
| enemy city (missile) | `SUCCESS_PERCENT` (35 %) of `city_worth` (the capture's value), times the damage share of the city's hit points; a quarter of it for a city that no army of ours stands near or no operation marches on (the damage heals) |
| blast (nuclear weapon) | in every enemy city in the radius 2: citizens killed (49 % at level 1, 69 % at level 2) of the points of the city, half counting for us; for a city an operation besieges also the better chance to take it (`NUKE_SUCCESS_PERCENT` 70 against 35); the units on the blast at the price of the damage, double near our cities |
| less | our own units in the blast at their hammers times the damage (a unit on the centre: all); a blast that reaches one of our cities is never fired |

A nuclear weapon is aimed at any tile the player knows (the rules only ask that the tile is known): a remembered enemy city is a target. The rules refuse a blast that would reach a player we are not at war with, and the planner asks them (`ai/legal.rs`), so it is never ordered. **Diplomatic cost**: the ruleset has none beyond that (the World Congress resolution against nuclear weapons stops their construction, not their use), so none is modelled; an owner's decision, see the questions.

**What is built.** `strikes::build_plan` (once a turn, in `weapon_orders`): at war with a civilization, fewer than `MAX_WEAPONS` = 2 weapons alive or queued, a kind the player may train (technology, resources, the planner's own gate), the best of (gain - weapon - project) > 0. The prospects are the cities our war operations march on, within twice the weapon's range of one of our cities (a rebase hop), and, for missiles, the enemy units standing next to our cities (the best three). A weapon is ordered only if it is ready within the life of the prospect it is built for (an operation forms for `FORM_LIMIT` and marches for `MARCH_LIMIT`; a stack at our gates is given 10 rounds), or bought at once; later it arrives after the war and is hammers lost. A nuclear weapon needs the project that `allows_nukes` first (`UnitDefinition::required_project`); the cities order the project first and the weapon once the project stands.

**The war case** (`operations::candidate`, `strikes::armory`, `strikes::struck`). The operation against a city is valued for the army alone and for each weapon the player has or could build: the weapon strikes the city before the army comes (a nuclear weapon kills the garrison and takes half the hit points, `NUKE_CITY_HIT_POINT_DAMAGE`; a package of two missiles takes the damage their strength does, at most 60 % of the hit points), the army is planned for what is left (`composition`, so a city the army alone cannot take within `MAX_ARMY` units can be taken), and the case carries the weapon's hammers, its project and, for a nuclear weapon, the world's memory of the strike. The chance of the capture is the weapon's (nuclear 70 %, two missiles +10 points). Only plans that fit in time count (a weapon to build at the empire's production must be ready within the operation's forming limit), the best value wins; an operation that was planned around a weapon makes the cities build it (`army_saved` in `build_gain`). For a running operation only the weapon on hand counts.

**Infamy.** A nuclear strike raises the warmonger memory of every civilization that met the user (rule and numbers: RESOURCING.md, "Infamy of a nuclear strike"). The AI reads it in `ai/infamy.rs`: a strike costs its user 2,400 centi-points per witness not at war with it at level 1 (twice at level 2), peace with a civilization whose memory is above the line of distrust costs 6 per point above it, a war on it is worth 3 per point more. With four witnesses a strike that gains 7,071 stays in the silo; the AI that is offered peace by a rival who has used a nuclear weapon refuses (`tests/ai_strikes.rs`).

**Basing.** A weapon waits in a city; with no enemy in range it goes to the city (within twice its range, the rules' `Rebase`) from which the most enemy value is in reach, the same rule as an aircraft (`air::rebase_target`); `ai/carrier.rs` adds a carrier as a base.

## Carriers

`carrier::wanted`: a war operation marches on a coastal city that no city of ours reaches with the longest range of the cargo we have, and no carrier is alive or queued. A coastal city orders the cheapest carrier first (before the warships and the aircraft: the cargo is useless where no city reaches), buying it when it takes more than twice `WAR_BUILD_TURNS`. A carrier waits in port until cargo is aboard (`PlayerQueries::cargo_aboard`), then sails to the water `max(3, range - 1)` tiles from the target and holds. Aircraft and weapons with nothing to strike from their base go to the carrier nearest the target (the rules' `RebaseToCarrier`: range and room are theirs).

## Landings and paratroopers

An army that must cross water already works: land units embark by themselves, the nearest tile out of the city's strike that the army can reach is the enemy's shore (two scene tests fix it: the strait is crossed and the band stands on the far shore), and how far they cross is bounded by the rules (embarking onto shallow water: with the first embarking technology a strait of two columns is crossed, of three is not found at all), not by the AI. What keeps landings rare is the war value (below). A variant that made the army stage on land only (never afloat) was tried and removed: paired score on `pace` -4.7 (-10.3...-0.1) at offset 0 and +1.1 (-3.0...+5.8) at offset 20, no landing more.

A **paratrooper** (a unit with a paradrop range) standing in an own city and belonging to a forming operation jumps (the rules' `Paradrop`, asked first) to the tile nearest to the operation's muster point that is land, out of the target's strike, at least 3 away (a shorter way is walked), within the paradrop range and with no enemy unit in sight next to it (`operations::muster_jump`). It saves the walk or the sail; across a strait or behind an enemy front it is the only quick way. The paratrooper's range is 5 in the data, a base is a city or a fort, so a jump happens when the staging tile lies within five tiles of a city.

## Measurements [measured]

**Strength**, bench, `standard` (new) against `previous` (the former `standard`), points share with the 95 % interval over 20 seed blocks, paired score difference, `--jobs 8`:

| Suite | Offset | Games | Points share | Paired score difference | Result hash |
|---|---:|---:|---|---|---|
| `pace` (6 nations, 500 rounds) | 0 | 200 | 50.0 % (50.0-50.0) | +0.0 (+0.0...+0.0) | `ffea261a19768ad7` |
| `pace` | 20 | 200 | 50.0 % (50.0-50.0) | +0.0 (+0.0...+0.0) | `f7a3219403068009` |
| `arms` (300 rounds, the era handed out at round 120) | 0 | 200 | 50.0 % (50.0-50.0) | +0.0 (+0.0...+0.0) | `e4c8f585a6d14697` |
| `arms` | 20 | 200 | 50.0 % (50.0-50.0) | +0.0 (+0.0...+0.0) | `8e46318d734b845b` |
| `standard` (tiny and small, 300 rounds) | 0 | 80 | 50.0 % (4 seed blocks: no interval) | +0.0 | `2ebbe5af458e71c5` |

The strength is not worse: it is the same, game for game, wherever no weapon is used, and the mean paired difference of the 800 games is +0.0. Earlier, on the way, a land-only staging rule for armies cost -4.7 (-10.3...-0.1) on `pace` at offset 0 and was removed (devlog).

**Behaviours per 100 games** (`examples/ai_behaviours`; Small maps unless noted, 6 nations; 5 scripts x 20 seeds, the `standard` suite 5 scripts x 4 seeds on tiny and on small maps):

| Suite | Variant | nuclear weapons | missiles | jumps | weapons built | carriers built | landings | wars | operations launched | cities captured |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| `standard`, tiny (20 games) | `previous` = `standard` | 0 | 0 | 0 | 0 | 0 | 5 | 50 | 35 | 5 |
| `standard`, small (20 games) | `previous` = `standard` | 0 | 0 | 0 | 0 | 0 | 30 | 175 | 110 | 5 |
| `arms` (100 games) | `previous` | 0 | 0 | 0 | 0 | 0 | 5 | 11 | 10 | 3 |
| `arms` (100 games) | `standard` | 0 | 3 | 0 | 6 | 0 | 5 | 11 | 10 | 3 |
| `pace` (100 games, 500 rounds) | `previous` and `standard` | 0 | 0 | 0 | 0 | 0 | 38 | 240 | 129 | 8 |

In the `standard` suites no civilization reaches the technology of a missile, of a nuclear weapon or of a carrier by round 300 (0 % of the games; in `pace`, 500 rounds: 0 % for weapons, 2 % for a carrier), so the counters are zero before and after; in `arms` every game has them from round 120 and the AI builds a weapon in 6 games and fires a missile in 3 of 100. Nuclear weapons and jumps: 0 in all, and in the forced scenarios of the earlier runs (a war of every nation with every other from round 120, cities of twelve citizens) missiles 6 and 20 of 100 (the same code before the infamy rule, under the name `heavy`). The scene tests are the proof that the behaviours work: a whole war with nothing injected (`tests/ai_war_arms.rs`) in which the AI that owns a nuclear weapon forms its operation around it and launches it.

**Time**, `examples/ai_rounds 3 8 2 representative stress`, three alternations on an idle machine (load 1.7), [measured]:

| Variant | representative: median round (ms) | stress: median round (ms) | p95 stress |
|---|---:|---:|---:|
| `unchecked` (origin/main behaviour) | 18.3-18.6 | 125.8-128.2 | 135.5-136.2 |
| `previous` | 18.4-18.9 | 128.6-128.9 | 140.8-141.8 |
| `standard` | 18.3-18.6 | 128.4-130.1 | 141.4-143.0 |

The contract is 250 ms and 1 s: the new standard stays at 7 % and 13 % of it. Per player turn in full games (bench columns, `pace` offset 0): AI p50 2.27 against 2.25 ms, p95 11.45 against 11.44 ms. The early scenarios have no weapons; the code of the arms runs only for a player that owns or can build one.

## Why the arms hardly occur in the bench games

Two reasons, both measured. (1) The technologies: in the `standard` suites (300 rounds) no civilization knows the technology of any missile, nuclear weapon or carrier at the end (0 % of the games), and the full 500-round `pace` games reach it too late for a war; `arms` hands them out at round 120. (2) War: counting `operations::start` on 30 Tiny Archipelago games of 300 rounds, 5,964 candidate targets were evaluated, 1,311 were beyond the objective range, 4,653 (78 %) of the rest had a negative value, 36 survived and 31 launched. The city the AI could take is small (1 to 4 citizens at rounds 30 to 215, gain 784 to 4,438 centi-points) and the army that takes it costs 7,500 to 34,000; a missile costs 450, a nuclear weapon 1,800 and its project 2,250, and the world remembers the strike. The weapon enters the war case (it can turn a city the army alone cannot take into a target), but for the small cities of the normal game the army was never the obstacle. Nothing in the AI's currency makes a weapon pay against a city of four citizens, and no price or cap was changed for it.

## What is tested (deterministic scenes, `tests/ai_strikes.rs` and friends)

- a missile in range of a war enemy's city that an army besieges is fired; the standard AI's generic advisor fires it at anything in sight (the control); a lone scout is not worth a missile;
- a nuclear weapon is fired at the city of a war enemy that an army besieges; never when one of our cities is in the blast; never when a unit of a player at peace stands in it; never at a player not at war;
- the cities build a missile for a war operation; build the project a nuclear weapon needs, and the weapon after it; nothing in peace;
- a bomber boards a carrier and the carrier sails for the target out of its reach; a coastal target out of reach makes the port build a carrier;
- a paratrooper jumps across a strait to the tile where its operation gathers, not next to an enemy; a band with the embarking technology crosses the strait and stands on the far shore.

## Manual check

`cargo test --release -p classic-rules --test ai_strikes --test ai_carriers --test ai_landings` (all scenes), `cargo run --release -p aurelia-qa --example ai_behaviours -- standard small all 20 300 0 late-war` (the counters), `cargo run --release -p aurelia-qa --bin ai-tournament -- --suite arms --candidate standard --baseline previous --jobs 8` (the strength). The whole story in one test: `cargo test --release -p classic-rules --test ai_war_arms -- --nocapture` (an AI with a nuclear weapon and an operation planned around it launches it); `... --test nuclear_infamy` (the rule); `... --test ai_strikes` (value, witnesses, price of peace). In the game itself: a notice \"... used a nuclear weapon\" appears for every strike (combat colour, the map centres on the blast), and the diplomacy screen shows \"You remember their warmongering (n)\" for the user.

## Not done

- Not measured: strength with the weapons really in use (they do not occur in the bench games, so the paired difference is zero by construction); a bench of games with a war forced on the nations and the era handed out, against `previous` (the earlier forced counters were taken before the war case and the infamy rule).
- No coastal defence, no counter to the weapons (nothing answers a nuclear weapon).
- Paratroopers do not make an operation of their own (an overseas target out of every walking route); that needs the operation record to say how its army gets there (a new field in the saved AI memory). The jump saves the walk only where the staging tile is within the paradrop range (5) of a city: 0 jumps in 300 games.
- The weapons' value uses the score currency alone; the stakes of a leader (a capture that wins the game) do not reach the unit-level decision.
- The infamy of a nuclear strike is a rule of the game (RESOURCING.md); its numbers (weight 400, 6 centi-points per point) are an own design and not tuned on a bench. City-states do not remember a strike.
