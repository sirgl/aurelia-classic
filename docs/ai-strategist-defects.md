# Three strategy defects fixed through the strategist (`strategist-x`)

The model, the gates and the protocol of lane A (22 November 2026), written before any run. The
candidate `strategist-x` is `standard` with the changes below; `standard` does not change (hash
`d2d736c1273a82bb` for `ai-tournament --solo standard --suite standard --speed quick`). Every new
weight is written by hand with its reason in `ai/strategy_net/weights.rs`; no number is fitted.

## 1. The spaceship's aluminium

**Root cause.** No planner knows how many copies of a strategic resource the strategist's goal needs.
A copy is priced by two other models: "a copy is the army's units" (workers `planner_value::resource_value`,
deals `peace::Negotiation::resource`: a second copy is worth 0) and "a tile is its yield" (tile purchase,
city site). The science victory needs one copy for each part while the part is alive (from its
completion to its installation) and one for each part factory (`derived::resources::production_resources`:
no part starts or completes without a free copy). The strategist has no input about the copies of its
goal and no output for them. Diagnosis, channel by channel: `work/nets/exam/SHIP-LOG.md` (ref
`exam/spaceship-aluminium`).

**Class.** A resource an item of the strategist's goal needs (`required_resources`,
`consumes_resources`): aluminium for the parts and their factory in the classic rules; the same class
holds for any goal whose items need copies. Closed here for the science goal; another goal joins by
giving `SupplyNeed` its items.

**Net change.**

| part | what | reader |
|---|---|---|
| input `fe::SUPPLY_SHORT` | the copies the goal lacks over the copies it needs (0: no need), from the rules' data (`ai::supply::SupplyNeed::of`: builders = the most productive cities, one a missing part; need = each builder's part and the factory it lacks; short = need − free copies) | the trace; the supply price |
| output `Plan::supply` | the goal's shortages (resource, copies, hammers a copy holds back) and the strategist's multiplier `supply_mult` = 1 + Σ row × `SUPPLY_ROWS` (aim science +1.5: the copy is the part's hammers; tall +0.3: a tall empire has few deposits; conquest −0.5: it takes the copies with cities; danger −0.5: defence first) | `Prices::supply`, read by the channels below |
| tech | the goal's path holds the technologies that reveal the resources its items need (`goals`: a Tech goal's path closes over the reveal techs) | the research head |

Channels that read `Prices::supply` (each converts the hammers at its own price of production; no other
multiplier): the tile a city buys (`allocator::tile_offers`), the improvement a worker builds
(`planner_workers::supply_gain`), the site of a new city (`planner_settle::SiteContext::supply_score`), a
building that gives or consumes copies (`value::Prices::building_supply`), the answer to a deal and an
offer of copies for gold a turn (`peace::Negotiation::resource`, `peace::supply_offers`), the gift to a
city-state whose alliance gives the copies (`city_states::supply_worth`), and a non-part unit outside a
city that holds a copy a part waits for is disbanded (`supply::release_orders`). The code of the
channels is the WIP of `exam/spaceship-aluminium`; the need is now the strategist's output.

## 2. City-states

**Root cause.** The strategist reads the city-states (`fe::CITY_STATES`, `fe::CS_NEAR`, the concept
`city_state_play`, the aim diplomacy) but emits nothing about them. The scripted planners know a
city-state only as a target of gifts: the war machine never targets one (`operations::targets` walks
`brief.relations`, which hold nations only: 0 wars on city-states in 1200 seats,
`work/cs-audit/REPORT.md`), the pledge goes to friends only (`city_states::pledge_orders`), and no quest
is read but the gold ones.

**Class.** A city-state as a prey or as a client: a decision of the strategist about each one that no
planner makes.

**Net change.** A stance per known city-state, two neurons (thousandths, 0…1), inputs from the
city-state screen and the seen cities only:

| neuron | inputs (weight, why) | reader |
|---|---|---|
| `cs_prey` | bias −600 (no city-state is prey by default); style conquest +1200 (a conquest plan takes cities, a city-state's are the nearest weak ones); `warlike` +600 (an aggressive leader); `near` +800 (6 tiles from our nearest city → 1, 12 → 0: an army reaches it within the plan); `city_state_play` −1000 (an empire that lives on its city-states keeps them); our ally or friend −2000 (never the own client); pledged by us −2000 | `operations::targets`: the seen cities of a city-state whose prey ≥ 0.5 are candidates of the war machine, at peace (no cooldown with a city-state), the rival share of the loss 0 (a city-state's score is nobody's); the war case decides whether the war pays; the posture still gates an operation (`posture_permits`) |
| `cs_client` | bias −300; aim diplomacy +1500 (the votes are the city-states'); `city_state_play` +800; `near` +300 (its land and quests are at hand) | `city_states::pledge_orders`: a pledge to a client at influence ≥ 0 not at war (its resting point +10, the pledge quest done), besides the friends |

## 3. A rival running away

**Root cause.** The strategist reads a model of every rival (`ai::rival_model`, features 54–67) with
weight 0 and has no output about a rival. The scripted war machine values a capture by the score only
(every rival's loss counts 50 % under `war-pays`, `operations::rival_percent`), and it never aims at a
city of more than 8 citizens or at a rival already at war with us (`operations::odds_are_good`); a new
operation starts only in the postures prepare and war (`posture_permits`), which nothing about a rival
moves; a denunciation follows the warmonger memory alone (`diplomacy::denunciations`); friendship and
research agreements are offered to any trusted nation, the leader included. The exam shows it
(`stress/runs/wave1`, `standard` on every seat): B9 (a rival with all the space technologies, 1–3 parts
missing) wins by science in 38–44 % of the positions against 6 % in the control, and no city of it is
taken; A1 (science ×1.25…×2): 0 wars on the leader and 0 denunciations at every strength.

**Class.** A rival's victory as a cost to us. Today only the World Leader vote is watched (`vote-guard`,
scripted); the science race and the score are not.

**Net change.** A threat per met rival `r` from its public screens (`rival_model::Rival`):

| input | scale |
|---|---|
| `tree_r` | the technologies guessed of `r` over the rules' technologies (the road to the spaceship) |
| `lead_r` | (its technologies − ours) × 10 over the rules' technologies, 0…1 (a tenth of the tree ahead → 1) |
| `pace_r` | its science over its and ours, 0.5 when unknown (the lead grows) |

`threat_r` = unit(−1000 + 1200 tree_r + 1000 lead_r + 1200 (pace_r − 0.5)): −1000, a rival level with us
is no threat; +1200 a share of the tree (a rival with the space technologies, 0.75 of the tree, gives
+900); +1000 a tenth of the tree ahead (it reaches the spaceship first); +1200 × (pace − ½) (twice our
science gives +400: the lead grows). The plan's threat is the largest, its rival named, when it is at
least 0.3 (below it the rival is in the race).

| output | reader |
|---|---|
| input `fe::THREAT` (the plan's threat) into the posture: prepare +2000 (the army that stops the leader), peace −1200 | `operations::posture_permits`: an operation may start |
| `Plan::threat` (rival, level) | `grand_strategy::Stakes::of`: each seen city of the rival carries a stake = the victory's prize × threat × its citizens over the rival's guessed citizens (a capture takes its share of the rival's race); `operations::odds_are_good` admits a staked city (the case decides); the peace answers read the same stakes (`peace`) |
| `Plan::threat` at ≥ 0.5 | `diplomacy::denunciations`: the rival is denounced when not a friend; `diplomacy::proposals`: no friendship, research agreement or open borders offered to it; `diplomacy::item_value_priced`: a research agreement with it is refused |


## 4. The weak neighbour (S-4 of `work/small-nets-design-2026-11-22.md`), gate written before its first run

Added 22 Nov 02:30 by the coordinator's decision. The mirror of the city-state prey on nations:
`prey_r` = unit(−0.8 + 1.5 weak_r + 0.8 near_r − 1.0 threat_to_us_r), `weak_r` = 1 − their soldiers over
ours (the demographics' soldiers row of the rival model), `near_r` from the distance to its nearest seen city;
outputs: the posture term prepare +1.5 × prey at prey ≥ 0.5, and stakes on its cities as for the threat.

| gate | metric (exam A5, odd instances 1…59, mean over the three levels; `ai-stress run --situations a5`) | pass |
|---|---|---|
| P1 | `nobody_attacked`, edited | candidate ≤ `standard`'s − 0.15 |
| P2 | `seat_cities`, edited − control | candidate ≥ `standard`'s |
| P3 | `end_seat_win_weight`, edited − control | candidate ≥ `standard`'s − 0.03 |
| P4 | A7 (we are the weak one) `wars_declared` delta, if the exam has A7 by the run | candidate ≤ `standard`'s |
| P5 | whole games W1–W3 with S-4 in | as W1–W3 |

## 5. Tall first (S-5, the coordinator's step-0 finding), gate written before its first run

22 Nov 02:50. Dreams step 0 (pace Quick, offsets 300–301): a seat held to tall + science + peace took 65.2 %
victory-weighted against `standard`, held to wide + science 34.5 %, with 0.6 cities fewer at round 198; the
strategist locks wide on about 60 % of the seats (89 of 144 locks in the lane's traces). Edit: the wide style's
logit −1.2 (`weights::threat::WIDE`, strategist-x only).

| gate | metric | pass |
|---|---|---|
| Y1 | whole games, the candidate with the edit against the candidate without it (`AURELIA_X_PARTS` without `style`), tuning offset 1202 | weighted share of the one with the edit ≥ the one without |
| Y2 | the accepted candidate | W1–W3 on 1201 and 1203 (never on 300–301) |

## Gates (written before the first run, not changed after)

**Whole games.** `ai-tournament --candidate strategist-x --baseline standard --suite pace --speed quick`,
offsets 1201 and 1203 (400 games, never tuned on); tuning on 1200 and 1202. The baseline of the defect
metrics: the mirror `--candidate standard --baseline standard` on 1201 and 1203, and `standard`'s seats in
the candidate's games. The defect metrics come from the read-only probe `AURELIA_DIPLO_PROBE`
(`crates/qa/src/bench/diplo_probe.rs`, one JSON line per seat; summary `tools/diplo-summary.py`).

| gate | metric | pass |
|---|---|---|
| W1 | victory-weighted share (real victory 1, time ¼) | ≥ 50.0 % |
| W2 | real victories, candidate against baseline | not fewer |
| W3 | AI ms p50 a turn, candidate over baseline, same run | ≤ 1.10 |
| W4 | `--solo standard --suite standard --speed quick` hash | `d2d736c1273a82bb` |
| S1 | seats that reach the parts (every prerequisite project done): share without the copies of one part free at that round | candidate ≤ ½ of `standard`'s seats' in the same games, or ≤ 5 % |
| S2 | rounds a part could not start for want of copies, per seat that reached the parts | candidate ≤ `standard`'s |
| C1 | wars declared on city-states per seat-game | candidate ≥ 0.10 |
| C2 | city-state cities held at the end per seat | candidate ≥ 0.05 |
| C3 | city-state allies at the end per seat | candidate ≥ 0.9 × `standard`'s |
| C4 | quests completed over quests given | candidate ≥ `standard`'s + 3 points |

If `standard`'s share in S1 is below 10 % in the mirror, the defect does not show at this scale: S1 is
reported and S2 decides.

**The situation exam.** `ai-stress run` on the positions `work/nets/stress/runs/positions` (200 pace
Quick logs of `standard` to round 112), situations A1 and B9 at three strengths, every arm played to the
end; the variant of every seat is set by `--variant` (`standard`, then `strategist-x`, the same
instances). Accept on the odd instances 1…59 (30 positions × 3 levels); tune on the even ones. Means over
the three levels:

| gate | metric | pass |
|---|---|---|
| T1 | B9 `rival_wins_science`, edited | candidate ≤ `standard`'s − 0.10 |
| T2 | B9 `rival_cities_lost`, edited − control | candidate ≥ `standard`'s + 0.30 |
| T3 | A1 `wars_on_leader`, edited − control | candidate ≥ `standard`'s + 0.20 |
| T4 | A1 `denouncements_of_leader`, edited − control | candidate ≥ `standard`'s + 0.50 |
| T5 | A1 and B9 `end_seat_win_weight`, edited − control | candidate ≥ `standard`'s − 0.03, each |

A defect is fixed when its gates pass (1: S1, S2; 2: C1–C4; 3: T1–T5); the candidate is accepted when
W1–W4 pass. **Stop rule**: two tuning rounds without movement of a defect's metrics → its diagnosis is
written here and the defect stops.

## Results

### Baseline (`standard`, measured 22 Nov 01:20, build `d59d286` + the exam's harness)

W4: `--solo standard --suite standard --speed quick` hash `d2d736c1273a82bb` (40 games). Mirror
`--candidate standard --baseline standard`, pace Quick, offsets 1201 and 1203 (400 games, 2400 seats):
real victories 67 of 400 games (diplomatic 9 + science 22 on 1201), AI ms p50 2.33.

| metric | `standard` |
|---|---:|
| C1 wars on city-states / seat | 0.000 |
| C2 city-state cities held at the end / seat | 0.031 |
| tributes / seat | 2.83 |
| C4 quests done / given | 12.6 % |
| C3 allies at the end / seat | 1.35 (r132: 0.11) |
| seats that reached the parts | 890 |
| S1 lacking a part's copies then | 26.1 % (232): the defect shows, S1 is a gate |
| S2 rounds a part waited for copies / such seat | 4.53 |

The probe counts a quest by its city-state, nation, kind and start; 48.6 quests a seat are counted
(the audit counted 24.8 by another method); the share done (12.6 %) is the audit's (12 %).

### Tuning (even seeds and instances only: offsets 1200, 1202; exam instances 0, 2, …, 58)

Diagnosis first (`work/nets/lane-strategy/CONCLUSIONS.md` §4–7, traces `runs/trace-*`): in B9 the seats that met
the leader read its technologies from the literacy row shared by score (32–50 of 81 while it knows ~70); half the
seats never meet it; no met seat changes posture. In A5 the disarmed neighbour reads as an average army (the
soldiers row, 10 [8…14]), so S-4 as designed would not fire: not written.

| build (tuning) | weighted share vs `standard` | real wins | what it showed |
|---|---:|---|---|
| x1 all three, offset 1200 | 42.0 % | 20 / 34 | worse; diplomatic victories 5 / 19 |
| threat alone, 1200 | 56.3 % | 40 / 26 | the threat pays |
| supply alone, 1200 | 48.9 % | 35 / 38 | S1 16.3 % vs 25.7 % (not half); allies 1.08 vs 1.56 |
| prey alone, 1200 | 51.0 % | 36 / 32 | 0.010 wars on city-states a seat |
| client alone, 1200 | 55.9 % | 41 / 27 | tributes 0.95 vs 2.8 |
| threat, 1202 | 53.0 % | 34 / 29 | |
| threat + client, 1202 | 50.9 % | 32 / 30 | Y1's control |
| threat + client + tall first, 1202 | **62.2 %** | **58 / 27** (science 35 / 17) | Y1 passes |

Hand edits made between the rounds (each with its why in the code): the supply wish opens with the ship's progress
(`SUPPLY_OPEN`); the client pledge at influence above 0 (not at 0, a tribute source); the threat's cities reached
from the decisive range and formed for the decisive limit. The exam's wars on the leader did not move in three
rounds (x1, x3, x4: `rival_cities_lost` 0, `wars_on_leader` Δ ≈ 0, `forming` 0 in the trace of B9 #6 with the
posture at prepare): **stopped by the stop rule**. Not found: which condition of the war machine leaves the
leader's cities without a candidate (seen cities, the route to a staging tile, the case, the chest); the next step
is the war probe's start checks on B9 #6.

Accepted parts (`wishes::Parts::ACCEPTED`): threat, client, tall first. The supply and the prey stay in the code
behind `AURELIA_X_PARTS` (their gates failed on tuning).

### Acceptance (odd seeds, never tuned on): `strategist-x` (`8c33559`) against `standard`, pace Quick, offsets 1201 and 1203

| gate | result | pass |
|---|---|---|
| W1 weighted share | 61.3 % (CI 54.8–67.3) on 1201, 59.1 % (51.5–67.3) on 1203; 400 games | yes |
| W2 real victories | 102 against 52 (science 59 / 18, diplomatic 43 / 34) | yes |
| W3 AI ms p50 | 2.37 / 2.46 and 2.24 / 2.33: 0.96 | yes |
| W4 standard's hash | `d2d736c1273a82bb` (bin of 8c33559, 40 games) | yes |
| S1 lacking a part's copies | 28.7 % against 29.6 % (the supply is not played) | no |
| S2 waiting rounds | 5.37 against 4.89 | no |
| C1 wars on city-states | 0.000 | no |
| C2 city-state cities at the end | 0.079 (standard 0.036) | yes |
| C3 allies at the end | 1.65 against 1.16 | yes |
| C4 quests done | 12.9 % against 12.8 % (+0.1 point) | no |

Defects 1 and 2 are not fixed by the accepted candidate (their parts failed on tuning and are not played); the
candidate is accepted on W1–W3. Its gain comes from the threat and tall first (tuning table above).

Exam, odd instances 1…59 (90 rows a situation, three levels), `strategist-x` on every seat against `standard` on
every seat (`runs/stress-final`, `runs/stress-standard`; `tools/stress-gates.py`):

| gate | standard | strategist-x | pass |
|---|---:|---:|---|
| T1 B9 rival wins by science (edited) | 0.311 | 0.256 | no (−0.055, needs −0.10) |
| T2 B9 rival cities lost Δ | 0.000 | 0.000 | no |
| T3 A1 wars on the leader Δ | −0.011 | 0.033 | no |
| T4 A1 denunciations of the leader Δ | −0.022 | 0.478 | yes |
| T5 A1 / B9 end win weight Δ | 0.061 / −0.011 | −0.050 / 0.058 | no / yes |

Defect 3 is moved (denunciations, no help to the leader, the leader wins less often) but not fixed: no war on it.

## Review conditions (22 Nov, after the independent review `work/nets/lane-strategy/REVIEW.md`)

Written before the runs below; the gates are not changed after them.

**Finding 1 (fixed).** `operations::candidate` let any city with a capital stake above 0 past the odds gate
(`odds_are_good`: at peace and at most 8 citizens) and gave it `DECISIVE_RANGE` (32) and the decisive form
limit. A stake of the domination path (`Stakes::of_leanings`: every remaining capital of a seat on the
domination path, and the last capital of anyone) is above 0 too, so `strategist-x` sent ordinary domination
operations against large capitals and into running wars, where `war-pays` measured 9 % and 3 % captures. Now
only a city of the rival running away is `CapitalStake::runaway`, set by one rule, `Stakes::with_plan` (the
driver's turn stakes and `Stakes::of` both come through it; applied twice it adds nothing).

**Ablations (step 2).** Purpose: the replication's gain is +61 science and +53 diplomatic victories, and
the pledge adds nothing (59.9 % without it); which of the two remaining parts gives which victories, and
which one costs the wars on the candidate (1225 against 820), the capitals lost and the rebels (118 turns
below −20 happiness against 2). Each 400 pace games, Quick, against `standard`, offset 1550 (`--seed-offset
1550 --repeat 2`), build with finding 1 fixed, probe on:

| run | parts | question |
|---|---|---|
| A-style | `AURELIA_X_PARTS=style` (tall first alone) | does tall first give the science victories, and the happiness collapse? |
| A-threat | `AURELIA_X_PARTS=threat` (the threat alone) | does the threat give the diplomatic victories (denunciations of the leader), and the wars on the candidate? |

Reported for each: victory-weighted share and CI, victories by kind, wars declared and on it, capitals lost,
turns below −20 happiness and risings. No pass mark: they explain, they do not accept.

**Gate of a hand edit (step 4), if the trace of step 3 indicates one.** Offset 1555, 400 pace games Quick
against `standard` (`--repeat 2`), never run before:

| gate | metric | pass |
|---|---|---|
| R1 | victory-weighted share, pooled | ≥ 58 % |
| R2 | risings of rebels, candidate | ≤ 2 × `standard`'s in the same games |
| R3 | capitals lost, candidate | ≤ 1.5 × `standard`'s in the same games |
| R4 | `--solo standard --suite standard --speed quick` hash | `d2d736c1273a82bb` |

**Finding 1, the case.** The rule is latent in the measured games: a domination stake exists for a seat on
the domination path or for the last capital left, and no seat of the 800 replication games ends with the
domination goal. An instrumented build (printing every non-threat stake that reached `candidate`) printed
none in 40 `--solo strategist-x --suite standard` games and none in 30 pace games of offset 1550. So the
fix does not explain the wars or the capitals lost; it closes the rule for a domination seat. Pinned by
`grand_strategy::tests::only_the_runaway_rivals_cities_are_runaway`.

### Ablations, offset 1550 (400 pace games Quick each, build `b3bf402` + probe)

| | tall first alone (A-style) | | the threat alone (A-threat) | |
|---|---:|---:|---:|---:|
| | `strategist-x` | `standard` | `strategist-x` | `standard` |
| victory-weighted share (95 % CI) | **61.1 % (55.8–66.3)** | | 49.7 % (46.6–52.8) | |
| real victories | **121** | 64 | 67 | 66 |
| diplomatic / science / time | 52 / 69 / 99 | 29 / 35 / 115 | 37 / 29 / 129 | 40 / 26 / 138 |
| wars declared / wars on it | 238 / 208 | 251 / 281 | 528 / **627** | 457 / 358 |
| cities captured / capitals lost | 97 / 2 | 125 / 4 | 237 / 3 | 160 / 1 |
| seats eliminated | 1 | 2 | 0 | 0 |
| turns below −20 happiness / risings (rebels) | 9 / 1 (1) | 5 / 0 | **228 / 40 (79)** | 45 / 7 (11) |

**What explains the gain: tall first, both kinds.** Tall first alone gives the whole gain of the replicated
candidate (61.1 % against 60.0–60.6 %), science +34 and diplomatic +23. The threat alone gives nothing
(49.7 %, real 67 against 66) and every cost.

Why tall first gives the diplomatic victories too (the probe's seats, A-style): in `standard`'s own seats a
seat that holds the tall style wins a real victory 13 times as often as a wide one (0.106 a seat against
0.008: tall 558 seats, 25 diplomatic + 34 science; wide 597 seats, 4 + 1), and allies twice the city-states
at the end (1.70 against 0.81). The diplomatic winners are tall (25 of 29 for `standard`, 45 of 52 for the
candidate) with 6 allies and 7 friends: a tall seat of 3–6 cities has the gold for the city-states that a
wide one spends on settlers and upkeep. Tall first moves 512 seats from wide to tall (tall 921, wide 85,
conquest 194 of 1200 against 558 / 597 / 45), and each of them now plays the style that wins both races.
Case 1550001 (continents, even): p2 `strategist-x`, tall, 3 cities, 6 allies, wins by diplomacy; the three
`standard` seats are wide with 9–14 cities and 0–3 allies. Case 1550002 (odd): p5 `strategist-x`, tall,
3 cities, 6 allies, 7 friends, wins by diplomacy. The denunciations of the leader do not move the votes:
the threat alone has 37 diplomatic victories against 40.

A side effect of the same weight: the wide logit's −1.2 also moves seats to conquest (194 against 45),
which win little (0.031 a seat). Not changed tonight (it is the next tuning question, not a defect).

**What causes the rebels and the wars on the candidate: the threat.** With the threat alone 917 of 1200
candidate seats denounce a leader at some round; those seats were at war 16.9 rounds a game against 6.5
for the candidate's seats that denounce nobody, and 17 of the 18 unhappy candidate seats are among them.
The unhappy seats fall late (first round 223–317 of ~330), at war with 1.2 nations on average when it
starts (8 of 18 at war), with no captured city (captured 0 at the first unhappy round), and most are
denounced by every other nation. Cases: 1551003 (pangaea, even) p2, conquest, 11 cities, at war with all 5
nations from its first unhappy round (285), 35 rounds below −20, least excess −42, denounced by all 5;
1551017 (pangaea, odd) p1, tall, 9 cities, 67 rounds at war, 30 rounds unhappy from 261, denounced by all
5. Tall first alone has 1 unhappy candidate seat in 1200. The threat's posture term (prepare +2.0, peace
−1.2) opens wars in general, not on the leader (no operation forms on it, the stop rule above), and its
denunciation joins the wars on it.

Where the happiness goes (the probe's breakdown, `strategist-x-threat` on 60 games of offset 1551,
`happiness::player_happiness` at the first unhappy round and 10–20 rounds before): 1551003 p2 again (the
threat alone is the same game) falls from −3 at round 270 to −41 at round 285 with the luxuries unchanged
(24, 6 kinds), no occupied city and the buildings +3: the unhappiness of its citizens rose 113 → 144 and
public opinion (a foreign ideology's pressure) added 11. 1551005 (odd) p5, tall: +3 at round 210 to −28 at
226, citizens' unhappiness 53 → 76, the cities' count unhappiness 10 → 13, opinion +9, luxuries unchanged. So the collapse is not
captured cities and not lost luxuries: it is a late empire at war that grows and meets the ideologies'
pressure without the happiness to match. Which planner lets the citizens outgrow the happiness at war is
not traced tonight; the cause in the candidate is the threat's wars, and tall first does not show it.

**Hand edit (step 4): `strategist-x` plays tall first alone.** Why: the threat adds no victory (alone 49.7 %;
with tall first 59.9–60.6 % against 61.1 % without it) and it is the part that costs the wars on the
candidate, the capitals lost and the rebels. The threat stays as the bench-only variant
`strategist-x-threat` for the commander's lane (its next step: why no operation forms on the leader).
Measured by the gate R1–R4 above on offset 1555.

### Gate of the hand edit, offset 1555 (400 pace games Quick, `strategist-x` = tall first alone, build `1433d1f`)

| gate | result | pass |
|---|---|---|
| R1 victory-weighted share, pooled | 56.7 % (95 % CI 52.0–61.4); 56.5 % on 1555, 56.8 % on 1556; real victories 105 against 64 (diplomatic 38 / 29, science 67 / 35, time 94 / 137) | **no** (needs ≥ 58 %) |
| R2 risings of rebels | 0 against `standard`'s 3 (turns below −20 happiness 3 against 20) | yes |
| R3 capitals lost | 3 against 3 (1.0×) | yes |
| R4 `standard`'s hash | `d2d736c1273a82bb` | yes |

Wars declared 285 against 318, wars on it 321 against 282, cities captured 135 against 183, seats eliminated
3 against 1. Points share 49.8 % (45.5–54.0): the score is lower (the paired final-score difference −62 and
−74 a seat block), as for every tall-first build.

Control on the same seeds (measured after the gate, it does not change it): the candidate before the edit
(threat + tall first, build `4269615`) takes 55.8 % (50.1–61.6) on 1555–1556, real 94 against 60, wars on
it 628 against 397, risings 4 against 3. So offset 1555 is a harder draw for every tall-first build, and
the edit loses no share (56.7 % against 55.8 %) while it halves the wars on the candidate (321 against 628).
The gate R1 stays failed as written.

## Promoted (22 November, the fifteenth promotion)

Tall first (§5, `strategist-x`'s parts) is part of the game's `standard` since the fifteenth promotion: `standard` plays as the bench's `nets-x-site`
(tall first of `strategist-x` with the guarded site net of `econ-site`; classic/docs/ai-strategy-net.md "Promoted:
nets-x-site"). The results on this page were measured against the standard before it, the bench's `pre-nets-x` since
(its hash `d2d736c1273a82bb`, as quoted here). The variants of this page keep playing on that standard, as measured.
