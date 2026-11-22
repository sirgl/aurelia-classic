# The economy nets on the current `standard`

The site net (`ai::site_net`, where to found a city) and the choice net (`ai::choice_net`, what a city
builds and what a player researches) were fitted on an older `standard` and measured then as giving no
gain. `standard` has changed since: the strategist (`ai::strategy_net`), `next-w`, `next-x`. This page
asks one question: does `standard` get stronger with one or both nets switched on, as they are or with
hand edits; and only if a hand edit shows that the idea works, with weights fitted again on fresh games.

The order of the work (the owner, 22 November): first read the cases where a net chooses differently
from `standard` and why, then edit the net by hand (its inputs, its structure, its formula, a few
weights with a why), and only then fit.

The work is two commits: the site net and its hand edits (`econ-site`, `econ-site-tall`,
`econ-site-own-land`, the shadow tool), and on top the choice net over the strategist (`econ-choice`,
`econ`, `econ-choice-production`), which failed and stays a bench variant only.

## The model

### Who decides today

| Decision | `standard` | The economy net, where it plugs in |
|---|---|---|
| Where a settler founds | `planner_settle::choose_site`: the land score (`SiteContext::score`) plus the distance terms minus 3 a turn of travel; on a tall seat the science buildings' site bonus (`SiteQuality::Science`) | `SiteQuality::Net`: the net's population at +50 rounds mapped to score points (`site_net::planner_score`) plus the same distance terms; it replaces the land score and also the tall seats' science bonus |
| What a city builds | the allocator's candidates (`allocator::choose_within`), chosen by the strategist's production head (`strategy_net::heads::production`: the allocator's top 4, a goal's item, the plan's military unit, re-priced by the plan) | `choice::choose_production`, but only in the arm without a strategist; for `standard` it never runs |
| What the player researches | the strategist's research head (`strategy_net::heads::research`: own value plus the goals' paths) | `choice::choose_research` inside `planner_research::research_by`, which `standard` never reaches (the strategist returns first) |

So the choice net is dead in `standard` by construction: "standard plus the choice net" needs a rule
for who wins when the strategist and the net disagree. The rule here: **the net over the strategist** —
the strategist's choice stands unless the net's best of the allocator's top leads the strategist's
choice by the net's margin (`choice_net::best_over`); a strategist's choice outside the allocator's top
(a goal's wonder, the plan's military unit) always stands, because the net never saw such a row. This
keeps the strategist's goals and prices in force and lets the net correct only the choices it was
trained on.

### The candidates

Each is `standard` (the mind of `next-x`, `ClassicMind::new`) with one switch and nothing else; the
names are the bench's (`experimental-ai` only):

| Candidate | Switch |
|---|---|
| `econ-site` | `TurnBrief::site_net` on (`Variant::site_net`) |
| `econ-choice` | `TurnBrief::choice` = `ChoiceMode::NET`, read over the strategist (`choice::over_production`, `choice::over_research`) |
| `econ` | both |

`standard` itself is untouched: `ai-tournament --solo standard --suite standard --speed quick` keeps
the result hash `d2d736c1273a82bb`.

### Known risks before any run

- **Stale data.** Both nets were fitted on games of an older `standard` at Standard speed (`ai-site`,
  `ai-choice` have no speed option); the bench plays Quick. The site net's `round` feature and its
  50-round horizon mean another phase of the game at Quick (50 Quick rounds are about 75 Standard
  rounds).
- **The site net's target is myopic.** It predicts the population of the new city alone at +50; it
  does not see what the city takes from its neighbours (shared tiles), what it denies to a rival, or
  the border. A gap site between own cities and a forward site at a rival's border both score low on
  that target, so the net is expected to fill fewer gaps and to settle forward less, not more.
- **The choice dataset cannot be made on `standard` as it is.** `ai-choice` records a choice only in
  `choice::choose_production` / `choose_research`; with the strategist on, neither runs, so the tool
  writes no production rows and no research rows for `standard`, and its exploration does nothing.
  Fitting the choice net again needs the exploration and the record moved into the strategist's arm
  first.

## Gates (written before any run)

Whole games, the pace suite, Quick speed, head to head against `standard`, 400 games a candidate:
`ai-tournament --candidate C --suite pace --speed quick --repeat 2 --seed-offset 520` (offsets 520 and
521; the same games for every candidate, so the three compare paired). A hand-edited or refitted net is
measured on fresh offsets: 530–531, then 540–541. Offsets 300–301 and 400–447 belong to another
experiment and are not used.

| Gate | Pass |
|---|---|
| Victory-weighted share (a real victory 1, a victory by time ¼) | ≥ 50.5 % |
| Real victories | the candidate's ≥ `standard`'s in the same games |
| Turn time, median ms a turn | ≤ +10 % of `standard`'s in the same games |
| Economy diagnostic (reported, not a gate on its own): cities and population of each side at round 198 (the pace checkpoint of Standard round 300), cities founded a seat | reported |

`econ` (both nets) runs only if neither single net is a clear loss (victory-weighted share ≥ 49 % on
its 400 games); otherwise its result follows from the parts and the compute goes to the diagnosis.

## Results

### Phase 1: the nets as fitted (offsets 520–521)

| Candidate | Games | Victory-weighted share | Real victories (candidate / `standard`) | Turn ms p50 (candidate / `standard`, each offset) | r198: cities; population; science a turn (candidate / `standard`) | Gate |
|---|---:|---:|---:|---|---|---|
| `econ-site` | 400 | **53.7 %** (CI 50.0–57.4); 54.9 and 52.4 by offset | **87 / 75** (science 54 / 42) | 2.62 / 2.58; 2.57 / 2.53 (+1.6 %) | 5.0 / 5.4; 52.5 / 51.0; 193 / 189 | **pass** |
| `econ-choice` | 400 | **30.3 %** (CI 24.9–35.6); 31.6 and 28.9 | 47 / 73 | 2.47 / 2.58; 2.50 / 2.62 | 4.6 / 5.6; 55.0 / 50.8; 151 / 186 | **fail** |
| `econ` | — | not run (`econ-choice` below 49 %) | | | | |

`standard` unchanged: `--solo standard --suite standard --speed quick` → `d2d736c1273a82bb` on the
build of the variants.

### Where the nets choose otherwise, and why

`ai-econ-shadow --variant econ --games 20` plays solo games with both nets on and records each choice of
a net beside the one `standard` makes in the same state (`ai::econ_shadow`; never read by a decision).
`ai-econ-shadow --probe` reads the site net on its own reference rows with one input moved.

**The site net.** It takes another tile in 88 % of the choices (1559 of 1772). Where they differ, its
sites are farther from the own cities (4.4 against 3.3 tiles), half as often a gap within 3 tiles of an
own city (369 against 733), more often within 5 of a foreign city (369 against 231), on land the
planner scores lower (432 against 582). The probe: its answer rises with the distance to the own city
(3.05 citizens at 3 tiles, 3.24 at 4, 3.50 at 6, flat from 7), does not move with the distance to a
foreign city (3.08 from 3 to 10 tiles, 3.14 with none known), barely with the round (3.14 at round 20,
3.04 at 200: the Standard-speed data do not hurt at Quick), and rises with the foreign tiles in the
radius (3.09 with none, 3.30 with ten), tiles a city cannot work. The games: fewer and bigger cities,
more science, more science victories. `standard`'s packing at three tiles is the weaker play.

**The choice net.** Over the strategist it overrides 49 % of the research choices (3649 of 7444) and
10 % of the production choices (1698 of 16940), nearly always moving off the allocator's best (rank 0).
The research overrides are one pattern: the strategist and the allocator want Writing and the net takes
anything else, from Mining to Steel, so the library line waits most of the game; science a turn at r198
falls by 19 %. The production overrides give up the allocator's best for its 2nd–4th: stadium → hotel,
settler → hotel, workshop → caravansary, forge or colosseum, worker → an ancient wonder.

`ai-choice` records nothing on `standard`: its record and its exploration are inside
`choice::choose_*`, which the strategist's arm never calls. Fitting the choice net on fresh `standard`
games needs the exploration moved into that arm first.

### Hand edits

| Variant | Edit (`site_net::SiteNetPlay`, `choice_mode`) | Why |
|---|---|---|
| `econ-site-tall` | `Tall`: the net's score plus the tall seat's science site bonus (`planner_settle::NetRead::Science`) | `standard`'s tall seats choose sites by that bonus; the fitted net replaces it and never saw it |
| `econ-site-own-land` | `OwnLand`: the input `foreign_tiles` read as 0 | a foreign tile cannot be worked; the fitted weight on it is a trace of where such sites lay, and pulls sites toward rivals |
| `econ-choice-production` | the choice net over the production only | its research half blocks Writing |

Not edited: the round (no effect), the spacing (the net's preference is what wins), the research net
(dropped, not patched: no input or weight explains the Writing aversion from outside).

### Phase 2: offsets 530–531

The same 400 games for each candidate (`--repeat 2 --seed-offset 530`).

| Candidate | Victory-weighted share (by offset) | Real victories (candidate / `standard`) | Turn ms p50, each offset | r198: cities; population; science a turn | Capitals lost | Gate |
|---|---|---:|---|---|---:|---|
| `econ-site` | **59.0 %** (CI 52.5–65.7; 57.9, 60.0) | **92 / 60** | 2.72 / 2.68; 2.64 / 2.63 (+0.9 %) | 4.9 / 5.2; 51.9 / 50.0; 193 / 185 | 3 / 2 | **pass** |
| `econ-site-tall` | 56.1 % (CI 50.4–61.7; 54.0, 58.0) | 91 / 68 | 2.73 / 2.68; 2.68 / 2.67 | 5.0 / 5.2; 51.1 / 50.2; 192 / 186 | 4 / 2 | pass, below `econ-site` |
| `econ-site-own-land` | 52.4 % (CI 47.5–57.1; 50.0, 54.4) | 82 / 73 | 2.70 / 2.66; 2.65 / 2.61 | 5.0 / 5.2; 52.1 / 50.3; 194 / 186 | 4 / 2 | pass, below `econ-site` |
| `econ-choice-production` | 50.1 % (CI 45.2–54.9; 46.6, 53.3) | 71 / 68 | 2.66 / 2.70; 2.61 / 2.67 | 5.2 / 5.3; 49.7 / 50.8; 179 / 186 | 4 / 5 | **fail** (below 50.5 %; science −4 %) |

Neither hand edit beats the net as fitted on the same games: the tall seats' science bonus on top of
the net costs 3 points, reading the foreign tiles as 0 costs 7. So the weight on the foreign
tiles is not spurious for the outcome; a likely reading, not measured: a site whose radius reaches into
a rival's land contests that land, and the weight pays for land taken, not for tiles worked. The fitted net (`SiteNetPlay::Fitted`, `econ-site`) is the site part for
the joint candidate. The production half of the choice net alone is no gain either: the choice net
stays out, and it is not fitted again (its fit needs the strategist's arm to explore first; nothing here
says the idea works).

### Phase 3: `econ-site` on offsets 540–541

| Candidate | Victory-weighted share (by offset) | Real victories | Turn ms p50, each offset | r198: cities; population; science a turn | Capitals lost | Gate |
|---|---|---:|---|---|---:|---|
| `econ-site` | **52.8 %** (CI 47.9–57.4; 51.4, 54.3) | **79 / 71** | 2.77 / 2.70; 2.79 / 2.77 (+1.6 %) | 5.0 / 5.2; 52.7 / 50.6; 194 / 186 | 9 / 3 | **pass** |

Over the three pairs of offsets (1200 games): victory-weighted share 53.7, 59.0 and 52.8 %, real
victories 258 against 206, turn time +1 to +2 %. The cost that grows with the games is the capitals
lost: 17 against 7 in the 1200 games (4 of `econ-site`'s seats eliminated on offset 540, none of
`standard`'s), the price of the forward sites (below).

### The human tricks: gaps (M1) and far settlers (M4)

`ai-econ-shadow --founded standard,econ-site --games 40`: 40 solo games of the pace suite at Quick to
r198, every city founded beside the nearest own and foreign city at its founding, the settlers that
never founded a city, and the gaps left at r198: unowned land tiles three or more tiles from every city
and within five of two cities of one major, where another player can still put a city between them.

| Variant | Cities a seat at r198 | Foundings | Own distance, mean | At 3 from an own city | Within 5 of a foreign city | Settlers | Never founded | Gaps a seat |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| `standard` | 5.71 | 1126 | 3.27 | 80 % | 18 % | 1440 | 74 (5.1 %) | 11.3 |
| `econ-site` | 5.35 | 1038 | 3.98 | 45 % | 35 % | 1444 | 166 (11.5 %) | 11.5 |

- **M1, gaps between the AI's cities: no change.** The net spaces its cities wider, yet the land left
  between two of its cities for a filler city is the same (11.5 against 11.3 tiles a seat). The trick
  stays as open as it is in `standard`; the net neither closes nor widens it.
- **M4, distant settlers: a real cost.** Twice as many settlers never found a city (11.5 % against
  5.1 %: killed, captured, or still walking at r198), and twice as many cities are founded within five
  tiles of a rival (35 % against 18 %). The net wins in spite of it; the settlers' escort and travel
  (`planner_settle` escort wait, the travel cost a turn) are where to look next, not the net.

### Commands

```sh
cd classic
CARGO_TARGET_DIR=../target-own cargo +1.98.1 build --release -p aurelia-qa --bin ai-tournament --bin ai-econ-shadow
B=../target-own/release
$B/ai-tournament --solo standard --suite standard --speed quick --jobs 4                     # d2d736c1273a82bb
$B/ai-tournament --candidate econ-site --suite pace --speed quick --repeat 2 --seed-offset 520 --jobs 4
$B/ai-tournament --candidate econ-choice --suite pace --speed quick --repeat 2 --seed-offset 520 --jobs 4
$B/ai-econ-shadow --variant econ --games 20 --jobs 4
$B/ai-econ-shadow --probe
for c in econ-site econ-site-tall econ-site-own-land econ-choice-production; do
  $B/ai-tournament --candidate $c --suite pace --speed quick --repeat 2 --seed-offset 530 --jobs 4
done
$B/ai-econ-shadow --founded standard,econ-site --games 40 --jobs 4
$B/ai-tournament --candidate econ-site --suite pace --speed quick --repeat 2 --seed-offset 540 --jobs 4
```
