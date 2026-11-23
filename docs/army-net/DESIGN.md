# The army net — final design (23 Nov 2026, written before the code)

Clone: `the M4 lab's army-net` (`classic` on `nn/army-net` = `nn/mlx-sim` 7474473 + `qa/trainers`
beefede merged; `platform` main 15d5b3d). Rust: `classic/crates/qa/src/army_net.rs` and `army_net/`; Python/MLX:
`classic/nn/aurelia_nn/army/`. A measurement: no rule and no AI on main code paths changes; the player lives only in
`ai-exam suite`.

## Task

Conduct one side's army for a turn: which unit acts next, what it does (move, strike, fortify, set up, promote,
pillage, done), until the net ends the turn. Given from above: an objective token. Civilians (settlers, workers) and
the home city stay with the seat's mind; the Great General is a piece of the army and is commanded.

## What changed against the sketch, and why

| sketch | final | why |
|---|---|---|
| 24×24 hex-conv map encoder, 4 blocks × 64 ch | none in v1; every map fact the decisions read enters as exact per-tile fields of the candidate's tile and the unit's tile | the gate's 0.3 ms per decision on one core: a 4×64 hex conv over 576 tiles is ≈ 130 M MAC (≈ 10–30 ms in f32 on a core); the references decide by a handful of tile facts (defence, threat, distance to foe/objective, city fire, the blow from there), which are cheaper and exact as fields |
| fields from the MLX simulator | the same fields computed by the Rust rules (`own_reachable`, `plan_attack[_from]` previews, city range, threat, ZOC through the reach set), in one builder shared by the recorder and the player | the authoritative number is the Rust trainers: one source of features means no train/serve skew; tacsim stays the fast environment for the search teacher (step 6), which then needs a Python port of the builder with a parity test |
| 1–3 M parameters | ≈ 150 k (d 64, 3 layers, 4 heads) | latency; the data (≈ 1 M decisions) does not ask for more |
| teachers: references, Unciv, tac2 | references (every solver of the family) and Unciv; not tac2 | tac2 lives in another build (`tac2-src-b`), and is below the best reference in every family of the table; its decisions would teach a weaker play |

## Two variants (Roman's question, 23 Nov: why drop the map encoder?)

- **A** (no map): as above.
- **B** (map): A plus the 24×24 hex-conv encoder of the sketch: input = 16 static planes of the window (on the
  map, hills, mountain, water, rough, our defence, river, road, owned by us / the foe, improvement, citadel damage,
  objective, refuge, our / the foe's city; `army_net::statics_of`, once per play) + the 33 local numbers of every
  token summed on its tile (kind, hp, moves, strengths, range, arm, stances, level, xp, promotable, selectable,
  city facts); 1×1 projection to 64, then 4 residual blocks of two hex convolutions (7 taps, 64 ch, dilations
  (1,2), (2,4), (2,1), (1,1): a tile sees 14 hexes). The head reads it three ways: the token embedding adds
  `m_tok(M[tile of the token])`, the candidate embedding adds `m_cand(M[candidate's tile])`, the objective token
  adds `m_pool(mean M)`. The window is 24×24 around the arena's centre (clamped to the map).
- **B's budget** is per turn, not per decision: the map is encoded whole once at the start of the side's turn
  (≈ 130 M MAC), then after every decision only the tiles whose input planes changed are recomputed, stage by
  stage, over the tiles each convolution's taps reach from them (`Net::map_update`; checked equal to the whole
  encode). With a 14-hex field a single change still reaches most of the 24×24 window by the last block, so the
  saving is bounded (measured, reported). Reported: whole-encode ms per turn and update ms per decision on one core,
  beside the AI's own turn time on the same scenes.
- Both are trained on the same recorded data with the same recipe and evaluated on the same held-out instances;
  the table shows both, with the geometry families (choke, hold, flank, kite, harass, hill-archers, surrounded,
  convoy) called out.

## Inputs (`army_net/observe.rs`; all rounded through f16, as stored)

- **Tokens** (≤ 40): the objective; every unit of both sides (ours first, by id); every city of both sides. 48
  numbers: kind (objective / own / foe / city); offset and distance to the objective tile; hp, moves left / max,
  can attack, base strength and ranged strength over the scene's mean, range, arm (12 arms of `lab::arms_of` +
  civilian), fortified, set up, embarked, level, xp, promotable; its tile's defence, hills, rough; for ours: in a
  foe city's fire, adjacent foes / friends, threat (foes that reach it next turn), nearest foe, distance to the
  refuge; for foes: our units next to it and our units that reach it; selectable. City: hp share, strength over
  the mean, can strike, is the objective, population, ours. Objective: aim (8: destroy, hold, take city, protect,
  pillage, survive, catch, escort — from the family, what a commander would say), rounds left / total, optional
  (prey "either"), counts and hp of both armies.
- **Distances**: hex distance between the tokens' tiles, clipped at 15: the attention bias (a learned scalar per
  head and distance).
- **Candidates** of the acting unit: `knowledge::actions::unit_actions` (the rules' legal list) filtered to the
  army's kinds, plus *done*. 40 numbers: kind (10); the subject tile (destination, target, or its own tile):
  offset, distance, defence, hills, rough, city fire, adjacent foes / friends, threat, nearest foe and its change,
  distance to the objective and its change, to the refuge, on the objective, own land; for a move: moves left
  after it, the best blow from there (preview dealt / hp) and how many foes it reaches there; for a strike: the
  preview (dealt, taken, defender hp, kill, the striker dies), the target's kind (ranged, civilian, city) and
  strength. A promotion candidate also carries its id (an embedding).

## Body (`army/model.py` = `army_net/model.rs`)

Token MLP (48 → 64 → 64, ReLU) → 3 pre-norm transformer layers (d 64, 4 heads, FF 128, ReLU, the distance bias) →
LayerNorm → H. Heads:
- **pointer**: a logit per selectable token (`H_i · w`), the end's logit from the objective token; softmax over
  them.
- **action scorer** for the picked unit u: candidate MLP (40 → 64 + promotion embedding, ReLU → 64) = c;
  `[H_u, H_0, c, H_u ⊙ c]` → 64 (ReLU) → 1; softmax over the unit's candidates.
- **value** (H_0 → 64 → 2): the objective's win (the play's outcome) and the share of the army lost.
- **danger** (auxiliary, H_i → 64 → 1): the hit points each of our units loses in the enemy's next turn.

Sequential decisions: pick a unit → act → the game applies it → tokens rebuilt → next. The player stops a unit after
6 decisions in a turn and a turn after 96; greedy (argmax). At the end every army unit stands.

## Teacher data (`army_net/record.rs`)

`ai-exam suite --kept data/kept.jsonl --players solvers,unciv --record FILE` records every command the learner's
side gives to an army unit, with the tokens and candidates before it, labelled by the legal action it amounts to (a
move by the tile the unit ended on: a march or a longer move counts where it stopped), and the end of the turn.
`data/kept.jsonl` = `runs/final` (21 families) + `runs/cit5` (citadel, after the fix) + `runs/t3` (the five assault
families at beefede). **Train: instances 0–79 of every cell; held out: 80–99** (never trained on; only the evaluation
and the validation loss read them).

Weights: a decision of the play's teacher weighs 1 when the play is won, 0.1 when lost; the family's best teacher
(the highest win rate on the training instances) × 2.

## Training (`army/train.py`)

MLX on the GPU (under the broker's GPU lock), AdamW, cross-entropy of the pointer and the action, + 0.5 × value BCE,
+ 0.5 × danger MSE; batches gathered on the GPU from the flat arrays by offsets. Curves: loss and accuracy (pointer,
action, both) on the held-out decisions per epoch; throughput in decisions trained per second.

## Evaluation and the gate (written before the evaluation run)

The authoritative number: `ai-exam suite --kept data/kept.jsonl --from 80 --instances 100 --players
army-net,standard,unciv,solvers` on this build (standard = `380c45a20e751d31` on this base: main 64df245; the
`5d461f2a421f2151` of net2 is a later main and does not apply here); tac2 from the existing `runs/*-tac2` rows on the
same instances.

**Gate.** (1) On the held-out instances, in at least half of the families with ≥ 10 held-out kept instances, the
net's win rate is ≥ 90 % of the best teacher's win rate there (best teacher = the highest of the family's
references and Unciv on the same held-out instances). (2) The net's forward time per unit decision ≤ 0.3 ms on one
core (measured in the player: the body + pointer + scorer, mean over every decision of the run; feature building is
reported beside it and not part of (2)). For variant B (2) reads: the net's time per turn (whole encode + updates +
forward) ≤ the AI `standard`'s own learner-turn time on the same scenes (measured beside it). The gate is judged for A
and B separately.

## Parity

`army/parity.py` runs the MLX model on recorded decisions and writes its logits; `ai-exam army-parity` runs the
Rust forward on the same decisions; max |Δ| ≤ 1e-4 on every logit.
