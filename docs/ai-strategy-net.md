# The strategy net: a hand-written network of concepts for research, policies and production

**`strategy-net` is `standard` whose research, policies and choice among the allocator's best orders are made by a network written by hand: 46 situation features, 13 concept neurons, an intent (style locked once, victory aim, posture), 15 shadow prices, an economy cell that runs the empire forward by the rules' growth, a goal pointer that picks 3–5 goals by their marginal value in the cell, a policy-branch character and fit read from the rules' effects, and three decision heads. No weight is trained; every one has a reason (`ai/strategy_net/weights.rs`; the Russian page `work/strategy-net-weights.html` prints them all). Against `standard` at Quick on fresh offsets 130–134 (not tuned on; after the style lock and the policy fix): 61.0 % of the points on the standard suite (95 % CI 58.0–64.0, stronger), 58.6 % in duels (54.8–62.5, stronger), 59.0 % over 330 rounds (54.5–63.5, stronger; before the fix 53.0 / 57.8 / 36.0 % on offsets 110–114): science level with `standard` (545 against 600 beakers a turn at the end of the long game, 163–169 against 162–168 at the end of the short ones; before 127 against 164 and 365 against 593), 6 science victories in 200 long games against 2. Not promoted.** State of 3 November 2026.

Code: `crates/classic-rules/src/ai/strategy_net/` (`features` layer 0, `weights` every hand weight, `layers` integer neurons and softmax, `mod` intent, prices, memory and the turn, `model` and `cell` the economy cell, `effects` the effect classifier, `goals` the pointer, `heads` the decisions, `trace`); hooks in `driver.rs` (the plan at the start of a turn), `planner_research.rs`, `policies.rs`, `allocator.rs`; the variant `Variant::StrategyNet` (`strategy-net`). Scenario tests `crates/classic-rules/tests/ai_strategy_net.rs`; traced games `ai-strategy-trace` (crates/qa).

## The layers

| layer | what | weights |
|---|---|---:|
| 0. situation | 46 numbers 0…1: the land of the capital (food and production of its 12 best tiles, luxuries), room (free good sites within 10 tiles), the nearest rival city, barbarians (camps, raiders), ruins, the food internal routes would carry, city-states met and near, era, progress, cities, citizens, science, production, gold, war, force against what is in sight, threatened cities, happiness, faith, religion, tourism pressure, eras and score behind or ahead of the rivals, wonders taken, the leader's nine personality traits from the rules, early unique units. The slow part (land, rivals, ruins, caravans, gaps) is surveyed every 10 Standard rounds. | — |
| 1. concepts | good land, room to expand, tall growth = AND(good land, NOT room), danger, warlike, early, strike window = AND(warlike, rival near, early), wonder chance, science lead, culture pressure, happiness crisis, city-state play. A neuron is bias + Σ w·x clamped to 0…1; AND is relu(a + b − 1). | 54 |
| 1. intent | three softmaxes: style (tall, wide, conquest), victory aim (science, culture, domination, diplomacy), posture (peace, defence, prepare, war); the aim and the posture are held until another leads by 15 % (`AiState::mind_memory`); the style is locked once (below). More cities: by style and free sites, less in a crisis or danger, at least 4 while sites are free, at most the rules' city model (`tall::city_target`, the count with the most science per research cost) + 1 for wide and + 0.5 for conquest. Horizon H: 60 Standard rounds ± the intent, 20…150. | 66 |
| 2. prices | 15 multipliers of the allocator's prices (science, production, gold, culture, faith, happiness, military, wonder, expansion, trade, great people, tourism, worker, citizen, food): 1 + ½ Σ row × deviation over the 11 shares and two gates (danger, happiness crisis). Food is the cell's. | 59 |
| cell | the empire run forward in 10 steps over H, city by city: the food store and citizens by the rules' growth threshold at the game's speed (starvation too), the tiles the citizens work (food first until the city is fed with a surplus of 2, then hammers and beakers), the city's yields and the plan's percents. A plan's effect is the rules' effect evaluated on our own tiles (`territory::tile_meets`: terrain, feature, resource, improvement, river, coast). V(plan) = what it makes within H at the net's prices + the last step's rate to the end + the citizens it gained. The food price is the mean over the cities of V(+1 food) − V. | 10 |
| pointer | candidates: world wonders nobody built (in the capital or the two most productive cities, the rules' placement checked), key buildings (a percent, a yield per citizen, a tile yield, growth), policy branches, technologies that open a project (with the science aim). Rate = (V(plan + goal from its round) − V(plan) + its other effects) × chance / (rounds + 2). The research, each city's hammers and the culture are queues: a second goal of a city starts when the first is done. The best is taken into the plan, the rest rated again: up to 5 goals (and 2 key buildings beside them), at least 3, stop below 20 % of the first. | 10 |
| heads | research: own value at the net's prices + each goal's worth shared over the missing techs of its path by cost (×5: a tech's own value counts every unlocked item in every city to the end, a goal one item), ×1.3 on a shared prerequisite, the target's path held unless another leads by 15 %. Policies: a branch is scored by its fit to the intent (below) × (1 + ½ × its step's value in the cell over the best candidate's); a started branch by its rest per policy left, a new one by all it brings per step, opened only when it leads by 50 %; a goal branch +50 %; the ideology the rules name for the plan; switched at unrest when the culture pressure is above 0.6. Production: the allocator's top 4 plus goal items (offered beyond the allocator's build-time limits) and an army unit while preparing or waging a war; the allocator's value re-weighted by the net's prices by each effect's slot (a percent by the city's growth over H), the style's correction of the kind, half the goal's worth for its item. Faith and great people stay `standard`'s. | 15 |
| classifier | what an effect is worth beyond the cell, at the net's prices: happiness, free units by kind (a settler only while the plan wants a city), grants, cost cuts, trade routes, great person points, tourism, city-state favour, combat effects (only as much as the plan fights). | 8 |

266 hand weights and parameters in all (`weights::count`).

## The style lock and the policy branches

The owner's play, taken as given (3 November): a player chooses the style once; Tradition is few strong cities, defence and science; Liberty is more cities, flexible, conquest; Honor only at war; Rationalism almost a default, Commerce rare, Piety by the nation and its religion; Freedom follows Tradition, Order Liberty, Autocracy when a war is needed; the science aim goes Tradition → Rationalism and never Honor in peace.

- **The lock** (`mod.rs`, `weights::LOCK_*`): the style is locked when the first branch may be opened after the first survey (no earlier than 20 and no later than 40 Standard rounds; Quick 13–26); the first branch waits for it. A locked style is computed from the situation at the lock with only the events taken from the turn (`STYLE_EVENTS`: war, threatened cities, the force in sight, unhappiness), so the empire's own growth (its cities filling the room, its happiness spent on them) never moves it; another style replaces it only when it leads by 50 %, or by the ordinary 15 % in a war for survival (at war with a threatened city or danger ≥ 0.6). Before the fix the traced game went tall → wide at round 25 and back at 176: the room it had settled stopped counting as room.
- **A branch's character** (`branches.rs`): each effect of its policies goes to one character by its kind in the rules (a yield or growth in the capital, growth, free buildings in the first cities, border growth: tall; settlers, workers, flat per-city yields, the policy cost of a city, happiness per city: wide; combat, promotions, experience, kills, upgrades, generals: war; garrisons and city combat: defence; science, gold, faith, culture and city-state numbers their own; a production bonus by what it builds), each policy once. No branch is named. Shares in the classic rules: Tradition tall 0.79, Liberty wide 0.92, Honor war 0.86, Rationalism science 0.86, Commerce gold 0.51, Piety faith 0.79, Aesthetics culture 1.0, Patronage diplomacy 0.86 (scenario 23).
- **The fit** (`weights::branch::FIT`): 1 + Σ share × Σ input × weight over the held style, the aim and posture shares and two concepts (danger, the new `religious_play`). A branch whose largest character is war is shut outside the war posture (in the policy head and in the goal pointer). The fit is the key; the cell's value of a step adds at most half (`head::VALUE_WEIGHT`), because the cell sees little of the growth and science branches (border growth, free buildings, a science percent while content, free technologies): before, the cell valued Liberty three times Tradition for a tall empire.
- **The wait**: with the science aim, once a branch is open, the culture waits for a science branch whose era is ahead (no filler branch; the book's measurement: a filler cost 7 % of the science, `ai-science-audit.md`).
- **The ideology**: the one the rules name (`roles::ai_ideology`) for the war posture (prepare or war), else for the tall style (peace), else for the wide one; the tenets by the cell.
- **Eras in game order**: the net read an era's id as its order, but ids are the table's rows (alphabetical: the Renaissance is 7, the Medieval 5); the era feature, the rival gaps, the branches' and wonders' eras now use the rules' `index`.

## What the development showed

Each fix came from a measurement (dev bench: 80 games of the standard suite at Quick on offsets 120–128, never the measured offsets):

| change | why | share |
|---|---|---:|
| first version | — | 21.2 % |
| the cell's and the classifier's values to the end of the game | the cell counted H rounds, the classifier and the allocator the whole game: free settlers beat every growth policy | — |
| army only in the prepare and war postures; danger from a neighbour alone lowered | 3.3× the baseline's army at the end, half its gold | 45.0–47.5 % |
| a city ceiling from the rules' city model | 8.5 cities against 5.2, science −43 % | — |
| goals queued in their city; goal wonders offered beyond the allocator's limit | 263 goal wonders in two traced games the allocator did not offer | 43.8 % |
| the intent tilts prices by half; generals priced as an army; finish a branch (50 %) | science buildings over food and gold; three branches open at once | 50.0 % |
| the cell's mood keeps the empire content (the allocator buys happiness) | attribution: without the policy head 56.2 %, with it 50.0 %: the cell capped growth after a few citizens, so no growth policy paid | 55.0 % |
| combat effects only as much as the plan fights | Honor was taken in peace | 62.5 % |

**Why the desert wonder's marginal value was negative** (the worked example on the weights page): the toy capital among desert and hills (a) could starve — the cell kept its citizens on an empty store, so the plan without the wonder kept working hills (0/2) and looked rich; (b) the citizens took tiles by a fixed weight (3 food + 2 hammers), so with the wonder they left the hills for desert (1/1), trading hammers for food the mood did not let the city use, and the tail of the run carried that loss to the end. With starvation, a tile choice that feeds the city first and then works hammers, and a content mood, the wonder is worth 6 593 (second of the goals after the Great Library) and the pointer takes it with its path through Mathematics to Currency; after a wonder that feeds the city it is worth more than alone (scenario 15).

## Scenarios

`tests/ai_strategy_net.rs`, all 28 green: 

| # | scenario | the net's choice |
|---:|---|---|
| 1 | great land, no room | tall; at most one city more |
| 2 | room, no threat | wide; 4+ cities more; a city is priced higher than in 1 |
| 3 | a rival capital next door, a warlike nation, early | the strike window, conquest, posture "prepare", army price > 1.4 |
| 4 | the same without a rival in reach | no window, peace |
| 5 | a happiness crisis | happiness the highest price, no new city |
| 6 | camps and a threatened city | defence; army price up, wonders down |
| 7 | a war under way | posture "war" |
| 8 | scientific leader ahead / cultural leader with wonders / diplomatic leader among city-states | aims science / culture / diplomacy |
| 9 | a close call between tall and wide | the held style stays (hysteresis 15 %) |
| 10 | tall science against an early strike | a longer horizon |
| 11 | great land, no room, the culture to open a branch | Tradition (from the policies' effects in the cell) |
| 12 | open land | Liberty |
| 13 | Tradition started, Liberty open | the next Tradition policy |
| 14 | the desert-bonus wonder on desert and on grassland | worth its desert tiles; the rules' placement refuses it on grassland |
| 15 | the desert wonder alone and after a wonder that feeds the city | worth more after (the cell's compounding) |
| 16 | a desert-and-hills capital | the desert wonder is a goal; as the target the research takes its path |
| 17 | the food price at H = 20 and 120 | higher at 120 |
| 18 | a goal behind a tech worth nothing itself | the bridge tech first; without the goal the other |
| 19 | a prerequisite of two goals | its goal value ×1.3 |
| 20 | the target's path 10 % / 30 % behind another tech | kept / left |
| 21 | the memory through `mind_memory` | round trip; another mind's bytes read as empty |
| 22 | the weight count | 266 |
| 23 | the branches' characters from their effects | Tradition tall, Liberty wide, Honor war, Rationalism science, Commerce gold, Piety faith, Aesthetics culture, Patronage diplomacy |
| 24 | a conquest plan preparing a strike; tall and wide in peace; a war | Liberty; never Honor (shut); at war Honor's fit > 1 |
| 25 | wide locked on open land, the room then filled by its own cities | still wide (unlocked: tall) |
| 26 | at war with a threatened city | a war for survival: the 15 % margin |
| 27 | science aim, Tradition whole, Rationalism's era ahead / reached | the culture waits / Rationalism |
| 28 | the ideology for tall, wide, at war | Freedom, Order, Autocracy (the rules' roles) |

## The bench

`ai-tournament --candidate strategy-net --speed quick --seed-offset 130 --repeat 5` (pace: offset 130 only), built from `4077901` (git archive), 3 threads; fresh offsets, nothing tuned on them (the fix was built from the owner's play and checked on traced games only).

| suite | games | share | 95 % CI | score difference a seed block | verdict | before the fix (110–114) |
|---|---:|---:|---|---:|---|---|
| standard (198 rounds) | 400 | 61.0 % | 58.0–64.0 % | +56.4 (+40.7…+71.7) | stronger | 53.0 %, no difference |
| duel (198 rounds) | 400 | 58.6 % | 54.8–62.5 % | +98.5 (+55.2…+140.8) | stronger | 57.8 %, stronger |
| pace (330 rounds) | 200 | 59.0 % | 54.5–63.5 % | −14.5 (−46.1…+17.2) | stronger | 36.0 %, weaker |

Runs of the standard suite: 61.3 / 60.0 / 60.0 / 61.3 / 62.5 %; duels 52.5 / 56.2 / 60.0 / 54.4 / 70.0 %.

Means of the candidate's seats against the baseline's (pace, 200 games):

| round | score | cities | citizens | techs | science | wonders |
|---|---:|---:|---:|---:|---:|---:|
| 66 | 191.4 / 167.5 | 2.9 / 2.3 | 12.4 / 10.9 | 14.9 / 15.3 | 20.1 / 17.7 | 0.6 / 0.3 |
| 132 | 456.5 / 398.3 | 3.4 / 3.4 | 28.8 / 30.9 | 27.5 / 26.9 | 57.5 / 58.0 | 3.8 / 2.2 |
| 198 | — | — | — | 39.1 / 39.3 | 154.5 / 165.9 | — |
| 264 | — | — | — | 51.4 / 51.6 | 317.7 / 337.2 | — |
| end (330) | — | — | — | 63.4 / 64.1 | 544.5 / 599.7 | — |

Standard suite at the end: techs 38.9 / 38.9, science 168.8 / 167.9, cities 5.1 / 5.4. Duel at the end: techs 37.8 / 36.0, science 130.7 / 110.6, world wonders 14.3 / 8.6.

Victories (pace, 200 games): the net 118 wins (105 by time, 7 diplomatic, 6 science, round ~321), `standard` 82 (78 by time, 2 diplomatic, 2 science, round ~306). The spaceship project is done by 21 % of the net's seats (13 % of `standard`'s). The science victory is still rare for both (3.0 % against 1.0 % of the games); the net's science a turn is 9 % below `standard`'s at the end of the long game (544 against 600), the gap that remains.

What fixed the pace: the policy head no longer goes Liberty → Honor for a tall empire. The cell valued Liberty about three times Tradition (it does not see the growth branch's border growth and free buildings), and Honor's combat effects pay in any fight; the fit of a branch's character to the held style, the war gate and the wait for Rationalism put the owner's order in front, and the style no longer flips. The 3 November era fix (an era's id read as its order) also moved the era feature, the rival gaps and the branches' eras. Not separated: which of the three changes (lock, fit, eras) gave how much; no ablation was run.

## Traced games

`ai-strategy-trace --suite pace --seed 1` (pangaea, small, the net on the even seats, build `4077901`; the file `work/strategy-fix-bench/trace-pace-seed1.txt`). The style flips twice before the lock (round 2–7: the first survey), is locked at round 15 on all three seats (Quick; about 22 Standard rounds) and never changes again:

| seat | style (locked r15) | branches in order | ideology |
|---|---|---|---|
| Ottomans (rank 2, 72 techs) | wide 0.74 | Liberty (15), Rationalism (125) | Order (161) |
| Rome (rank 6) | tall 0.46 (wide 0.27, conquest 0.27) | Tradition (15), Patronage (150), Rationalism (165) | Freedom (207) |
| Arabia (rank 3, 63 techs) | wide 0.76 | Liberty (15), Piety (173), Rationalism (178) | Order (245) |

Honor is never adopted. Before the fix the Ottomans went tall → wide at 25 and back to tall at 176, and Liberty → Honor → Aesthetics. The aim drifted to diplomacy on the tall seat between rounds 150 and 166, which delayed its Rationalism by 15 rounds (the aim is not locked, as asked); Rome ends sixth in this one game, the Ottomans second: one seed is a trace, not a measure.

## Standard unchanged

The hooks run only with `TurnBrief::strategy`, which only `strategy-net` sets. `ai-tournament --solo standard --speed quick --seed-offset 110` gives the same result hash built from HEAD and from HEAD with this work, also after the 3 November fix (build `4077901`): `a01c38790cfd7d1c` (standard suite), `9e51cc4d1ca9df8e` (duel).

## Promoted (the twelfth promotion, 3 November)

`standard` plays the strategy net (`Variant::strategy_net`: `standard`, its alias `strategy-net`, `hybrid`, and Unciv's
modules inside `standard`). The net is in every build: `ai::strategy_net`, `TurnBrief::strategy` and the heads are no
longer behind `experimental-ai` (266 hand weights, no file, no download); the unit net, the end-to-end nets and the
site and choice tables still are. `standard-scripted` is the standard before it (every switch of `standard` but the
net) and exists only with `experimental-ai`, for the bench. The other candidates on `standard` (`trade-fit`,
`fog-march`, `unit-net` …) stay as measured, without the net.

Checks: `ai-tournament --candidate standard-scripted --suite pace --speed quick --seed-offset 130` built from the
promoted tree plays the benched games exactly: `standard` 59.0 % (54.5–63.5 %), 118 wins of 200 (105 by time,
7 diplomatic, 6 science), the mirror of the bench above. The smoke pin moves to `6eb9f185cee7f36e`, the hash of the
benched `strategy-net` (`4077901`) against itself; `standard-scripted` against itself keeps `0df05201ff04d810`.
Time of a turn in that run (one machine, 3 threads), `standard-scripted` → `standard`: AI ms p50 1.42 → 1.78, p95
5.34 → 5.65 with the cached tile weights of `cell::run` and the screening reused by `goals::choose` (before them: 1.46 →
2.20 and 5.48 → 7.82); the games are the same (result hash `52e7019852be0774` both times). Solo on the standard suite
(Quick, offset 110): `standard` and `strategy-net` `5b72ac75c806c0b5`, `standard-scripted` `a01c38790cfd7d1c` (the
old `standard`'s). The release also settles a natural wonder find inside the move that sees it, which changes
the games: the same pace bench then gives `standard` 57.0 % (51.0–62.5 %), 114 wins of 200, AI ms p50 1.45 → 1.78,
p95 5.41 → 5.72 (result hash `b8753f43f6382409`).

The net tooling (selfplay, the training grounds, the search teacher; `aurelia_qa::bench::modules::SCRIPTED_EXPERT`)
plays the scripted standard, so that its scenes, teacher and data stay what they were measured on; moving it to the
strategy net is a separate step (the grounds' teacher tables would be measured again). A save of `standard` keeps the
net's memory (AI state version `MIND_MEMORY_VERSION`); `ai_acceptance::leaders_aims_survive_save_and_load` plays on
after a load hash for hash.

## Not done

- No training: the next step is fine-tuning by outcomes with a penalty for moving from the hand weights (the design page).
- Faith and great people are `standard`'s; the posture changes the prices, not the war logic's declarations.
- The cell has one mood for the empire (content unless unhappy now) and no buildings beyond the plan; trade routes and improvements are not in it.
