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
