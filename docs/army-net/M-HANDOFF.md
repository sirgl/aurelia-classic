# Variant M (learned memory) — design, gates (written before any M measurement), handoff

State at handoff (23 Nov, ~14:00): clone `the M4 lab's army-net` (`classic` branch `nn/army-net`,
head see `git log`), DESIGN.md (A, B), tables `runs/table-*.json`, label shares `runs/label-shares.json`,
`tools/label_shares.py`. M is NOT built yet; this file is its brief. Read DESIGN.md and
`crates/qa/src/army_net/{observe,record,seat,model,parity}.rs`, `nn/aurelia_nn/army/{data,model,train}.py` first.

## Why M
The references keep commitments between turns that the net cannot see: the staged assault's `Scene::track`
(`engaged`, `waited`, `resting`), the rotation of the wounded. BC/DAgger nets without memory shuffle wounded units
every turn (they never heal) and burst piecemeal (the city heals back): traces `runs/traces/pa-classical-*`.
Hand history numbers (AH0: hp gained since our last turn, left its tile; tokens 46, 47) gave 14 families vs A0's
11 but did not fix assault-heal (52 %) / planned-assault (21 %).

## Design
- **Per-unit memory** u_s ∈ R^16 for each army slot s (slot = index in `Orders::army`, ≤ 16), zero at play start.
  The token of a unit adds `W_um · u_s`. At the side's turn end (the memory-state decision, below) each army
  token's final state H_i updates its slot: `u_s ← GRU16(H_i, u_s)`.
  - Named slots, hand-initialised gates (Roman's method: look, hand-edit, optimise last): dim 0 "turns waited for
    the stage" (+1 each turn the army did not strike the objective city; why: ref-planned goes after `waited` ≥ 5),
    dim 1 "resting since" (+1 while the unit stays wounded and still; why: ref-rotate rests to 90 hp), dim 2
    "engaged" (latches to 1 at the first strike on the city; why: ref-planned's `engaged`), dim 3 "hp at last turn"
    (copy of hp; why: healing vs being shot). Their update gate biased to keep (forget-gate bias + 2), weights
    hand-set from the token numbers that carry the fact, then trained. Dims 4–15 free.
- **Plan tokens**: 4 vectors P_k ∈ R^64 kept between turns, learned initial values; appended to the token sequence
  (attention bias for any pair with a plan token: its own learned scalar per head, not a distance bucket); at turn end
  `x_k = attention(q = P_k, K, V = H of the army tokens)`, `P_k ← GRU64(x_k, P_k)`. Not selectable.
- **Memory-state decision**: the record gets a per-decision flags field (the reserved u16; bit 0 = "our turn ends
  here: update the memory") and per-token slot ids (T × i16, −1 none). Teacher plays: the END decision is the
  memory state. DAgger plays: `seat::turn` records, at the net's own END, an unlabelled decision (pointer −2,
  flags 1) — the memory follows the net's trajectory. Rust carries `MemoryState {slots, plan}` per play in the seat
  (thread-local, reset in `play_on` next to `reset_expert` / `reset_history`; one play per thread from start to end).
- **Training** (`army/train_m.py`): batches of plays. Unroll over turns r = 0..R−1: memory-state decisions of turn r
  (batched over the plays, masked where a play has ended) → body with M_r → M_{r+1}. Then every decision of the batch
  is scored with its turn's M_r (gathered), losses as now. Truncated BPTT over whole plays (≤ 31 turns), curriculum on
  length 4 → 8 → full; in 10 % of sequences memory reset or dropped at a random turn; grad clip 1.0.
- **Privileged distillation** (training only): a head reads u_s / P_k and predicts the teacher's own state recorded
  beside the decision — for assault references `track.engaged`, `track.waited`, `resting ∋ unit`; loss weight 0.3
  (knob). Requires the recorder to write the teacher's track per memory-state decision (assault families; −1 else).
- **DAgger** with the net's own memory carried along its trajectory (as `seat` does), the turn-level probe of the
  siege set (expert memory carried: `EXPERT_TRACK`), per-decision probes elsewhere.
- **Rust**: `model.rs` gains `MemoryState`, the GRU cells, plan tokens in `body`; `parity.rs` unrolls each play of a
  record file carrying the memory and checks every logit against MLX's unroll (≤ 1e-4) over multi-turn plays.

## Gates (fixed here, before any M run)
1. Held-out trainers (Rust, `--from 80 --instances 100`): families at ≥ 90 % of the best teacher ≥ 17 (A2's count),
   and no family below A2 by more than its 95 % noise (two-proportion, the held-out N of that family).
2. assault-heal and planned-assault reported with A0 / A2 / AH0 / M side by side; M must beat A0 in both.
3. Training throughput ≥ 15 000 decisions/s (A2: ~45 000; the unroll costs) and the net's time per turn on one core
   ≤ 2 × A2's (2.3 ms per turn of 6 decisions).
4. Memory used: (a) the same eval with the memory zeroed every turn loses ≥ 5 points in assault-heal or
   planned-assault; (b) a linear probe from u_s / P_k predicts the teacher's `engaged` on held-out plays with AUC ≥ 0.8;
   (c) 2–3 memory trainers (the right act depends on something seen earlier and hidden now: an enemy that walked into
   fog; a stage that waited k turns), proven like the others (a reference wins, the passive control loses): M ≥ 70 % of
   the reference there, A2 clearly below.

## Order
1. Record format v3 (flags, slot ids, teacher track) + Rust reader + `data.py`; re-record teacher data (3 min).
2. `model.rs` / `model.py` memory, `train_m.py`, parity over multi-turn plays. Commit.
3. M0 (BC) on GPU (keep each GPU job ≤ 20 min: the mlx-sim lane shares the lock), eval, table `runs/table-M0.json`.
4. DAgger M1 (memory carried), train, eval, `runs/table-M1.json`; proofs (a), (b).
5. Memory trainers (c) in `trainers/families.rs` (append to `Fam::ALL`: older seeds stay), proof, measure.

## Addition 2 (coordinator, Roman: the net's own inner state, "the plan was such-and-such")
- **Named plan slots** in the 4 plan tokens, hand-initialised: target (a pointer to the city / unit token), stage
  (gather / march / siege / assault / heal-rotate / withdraw), turns in the stage, strength ratio at commitment,
  expected turns to capture; the rest free. **Hysteresis** as in V3: the plan changes only when a new one beats it by
  > 15 % or on an event (a unit lost, the city's strike much stronger than expected); the threshold is a learned gate
  initialised to that rule (why: without it a per-turn argmax flips the plan, which is the oscillation the traces show).
- **Supervise the plan tokens** from the teacher's plan state (ref-planned's stage and target; the staged assault's
  track) with the auxiliary head; proof (b) adds a probe of the stage from the plan tokens.
- **net2's memory as context** (`net2/memory.rs`, `plan::Memory2` → `CoreMemory`: war order, aim, phase, kind, spent):
  feed the war aim / target / phase as tokens; in the trainers set them from the scene. NOTE: net2 reached main after
  this clone's base (main 64df245 here; net2 is d10a5db+): this needs `main` merged into `nn/army-net` first (hashes
  then 5d461f2a421f2151 / 380c45a20e751d31).
- **Persist the memory in the save** as net2 does (its own section; an undecodable body = start fresh), with a test that
  save/load mid-siege plays the same as no save/load. NOTE: the brief keeps the army-net player in the qa tools only
  ("no change to the game's rules or AI on main code paths"); a save section is a main-path change, so until the net
  moves into the AI the memory is saved by the qa player beside the game (a sidecar keyed by the game's id) with the
  same test — the coordinator decides when it moves into the save body.

## Addition 3 (coordinator, Roman: "should the memory hold the last turns, and how do they link to the plan?") — variant MH

Written 23 Nov ~15:00, before any MH run (M0 and M1 measured, MH not built).

### Design
- **History tokens** of the current turn and the last K = 3 turns (most recent first, at most 32): each OWN command of
  an army unit (kind: move, strike a unit, strike the city, ranged, fortify / set up, promote / pillage / other; the
  subject tile relative to the objective; the outcome: damage dealt, damage taken, kill, capture, moved), and each
  ENEMY event at our turn start (our unit hit in the enemy's turn: damage, died, in a city's fire; a foe that came into
  / went out of sight), with its age in turns and the unit's army slot. 20 numbers (`observe::F_HIST`), recorded
  per decision (record v4). Logged by one hook that sees every command of the learner's army (`act::execute`,
  `record::step`), paused on the DAgger probe's copies.
- **In the body**: the history enters as keys / values only (the tokens and the plan tokens attend to it; it is not
  updated: O(T * H), not (T + H)^2), with its own learned bias per layer and head and a bias for the pair (history
  token, the current token of the same unit) — the pointer to the unit.
- **The link to the plan**: each history token's embedding adds `W_hp * P_0(turn it was taken)`, the operation plan
  token in force then (its stage and target live in its named dimensions); in training the plan of that turn is
  gathered from the unroll, in play the last 4 plan states are kept. (Which plan token it "served" is not modelled:
  only P_0 carries the operation; named here as a gap.)
- **Progress into the plan**: the plan update reads `progress` = (the objective city's hit-point share now, damage we
  dealt to the city over the window, damage we took over the window, our units lost over the window), all from the
  tokens and the history. Named plan dimension P_CITY0 = the city's share when the plan was taken (held by the
  hysteresis). The hysteresis gate adds `w_p * sigmoid(3 P_ENGAGED) * relu(theta_p - (P_CITY0 - city share now))`
  (theta_p 0.3, w_p 5 at the start). Why: an assault that has not taken 30 % of the city's hit points since it
  committed is the stall the traces show (the city heals back); then the plan may change (rotate, wait, withdraw).
- Everything else as M (slots, plan tokens, aux head, TBPTT, DAgger with the net's own memory and history).

### Gates for MH (fixed here)
1. Held-out trainers: families at >= 90 % of the best teacher >= M's count (the M net it is compared with: the last
   M of this lane, M1 or M2), no family below A2 by more than its 95 % noise.
2. planned-assault and assault-heal: MH above M by more than the two-proportion 95 % noise of the family's held-out N,
   or a diagnosis (from the traces) of why not.
3. The net's time per turn on one core <= 2 x A2's.
4. Proofs: (a) ablation: the history masked every decision loses >= 5 points in planned-assault or assault-heal;
   (b) probe: the teacher's stage from history + plan tokens (held-out plays; reported beside the plan-only probe);
   (c) the piecemeal strikes: strikes on the city per turn on planned-assault, MH vs A2 vs ref-planned (the trace
   measure: rounds with 1-2 strikes vs rounds with >= 3), held-out.

## Addition 4 (coordinator, Roman: memory like an LLM's tokens: "we moved so", "the plan is such", "the plan changed,
## we retreat", "this archer needs healing, pulls back") — variant MN on top of MH

Written 23 Nov ~15:05, before any MN run (order M1 -> MH -> MN; if MH already fixes planned-assault, MN is still built
for readability, and the report says so).

### Design (to be refined in code; the refinements go here before the run)
- A memory STREAM of record tokens with three writers: FACTS (MH's history, written by the game, not learned);
  STRATEGY (net2's `Memory2`: war target, phase, kind; in the trainers set from the scene: the objective, "war",
  the family's aim); NOTES (a learned note head at the end of each of our turns).
- A note = a kind from the vocabulary PLAN(target, stage), PLAN_CHANGE(reason: city stronger than expected / losses /
  reinforcements / target lost), WAIT_STAGE(shooters still to arrive), PULL_BACK_HEAL(unit), RETURN(unit),
  CAPTURER(unit), FOCUS(target), HOLD(unit, tile), THREAT(enemy stack), with pointer arguments (a token) and a free
  8-dim vector; it lives until replaced (PLAN) or for its span (a unit's note until RETURN); each record carries its age.
- Training: the note head is supervised from the teacher's own state each turn (ref-planned's stage / target, its
  rest and return decisions, the staged assault's track; ref-recall's seen stack for THREAT); the action head learns
  with the notes in context; DAgger with the net's own notes carried; note dropout 10 %.
- Hand-set thresholds: PULL_BACK_HEAL starts at the teacher's rule (hp < 70, < 45 for a shooter in the burst;
  why: `assault::ROTATE_BELOW`, `ROTATE_BURST`), RETURN at hp >= 90 (`ROTATE_BACK`), PLAN_CHANGE at MH's stall rule.
- Readability: the notes printed in the traces (`runs/traces/*`).

### Gates for MN (fixed here)
1. Held-out families at >= 90 % of the best teacher >= MH's count.
2. planned-assault and assault-heal not below MH by more than the family's 95 % noise.
3. Note agreement with the teacher's state on held-out plays >= 80 % (per note kind and pooled).
4. Time per turn on one core <= 2 x A2's.
5. Proofs: notes masked (ablation), the agreement above, planned-assault traces with notes beside the teacher's.

## Roman's decision (coordinator, 23 Nov ~15:40): the line is B (map) + memory + notes — MB -> MBH -> MBN

Written before any MB run. Measured on B1 (eval rows): per turn of 6.4 decisions, map encode 10.9 ms, incremental
updates 28.5 ms (a move reaches most of the grid through 4 dilated blocks), forward 1.4 ms. A2: 2.43 ms per turn
of 12.2 decisions on one core (this lane's measure, `runs/speed1-*`); M1b 3.00 ms (with the memory's update), MH0
4.49 ms.

### MB design
- M (slots, plan tokens, aux head) plus B's hex-conv encoder, encoded ONCE per turn from the turn's first moment
  (no updates within the turn: the unit tokens carry exact positions and hit points; what is lost is measured: MB
  against M on the same data).
- Lighter: 32 channels; the grid cropped to 12 x 12 around the army: centre = floor mean of the window cells of the
  army's tokens and the objective's (integer arithmetic, the same in Rust and MLX), origin clamped into the 24 x 24
  window; a token or candidate outside the crop reads no map. The encoder's output enters as in B (m_tok, m_cand,
  m_pool); the memory-state decision reads the same turn map.
- The Rust convolution profiled (one core); target: the encoder <= 1 ms per turn.

### MB gates (fixed here)
1. Time per turn on one core <= 2 x A2's (<= 4.9 ms on this lane's measure; the coordinator's figure: <= 4.6 ms).
2. B's field families kept: raid >= 85 %, prey-land >= 85 % (held-out).
3. M's siege families (siege, ram, assault-heal, city-fire, hill-archers, planned-assault, promotion-choice, rush)
   not lower than the M net of the same recipe beyond the family's 95 % noise.
4. Families at >= 90 % of the best teacher reported beside M's and A2's (no family below A2 beyond noise).

## The B line (coordinator, Roman's decisions, 23 Nov ~15:50) — order, init, gates (written before the runs)

Postponed: every further A-based run (M1r / M1rb were already trained and are reported; no AH, no ladder on M1,
no MH / MN on the A base). A2 and M1 stay the fixed baselines. MH's code (91c6dee) and MN's (5c8a445) are the
bases of the history and note parts. Order: **MB -> MBH -> MBN**, then the capacity ladder and the growth
(additions 6, 7) on the B line. The DAgger fixes are tested on the MB line.

### Function-preserving init at every step (never from scratch)
- MB = M1's trunk and memory + B1's trained map encoder (64 channels: the encoder B1 learned), its projections into
  the tokens, the candidates and the objective (m_tok, m_cand, m_pool) ZERO: MB plays exactly as M1 before
  training (checked on the quick eval). The map once per turn (B1 was trained with per-move updates; the
  map-encoder lane decides per-move vs once-per-turn by its kernels' speed).
- MBH = MB + the history tokens with zero-initialised output (the history's value path zero: it starts equal to MB).
- MBN = MBH + notes likewise.

### The quick eval (~10 min; Rust; held-out instances 80-99)
The 8 siege families (siege, ram, assault-heal, city-fire, hill-archers, planned-assault, promotion-choice, rush) +
raid + prey + 2 sentinels at teacher level (focus, snipe): 12 families. The full 32-family table only at a
variant's gate.

### Gates
- MB: >= 18 families (M1's count) at >= 90 % of the best teacher; raid and prey-land >= 85 %; time per turn on
  one core <= 4.6 ms (pending the map-encoder lane's kernels: B1's encode is 10.9 ms a turn today).
- MBH: assault-heal >= 80 % and planned-assault >= 50 % (held-out); no family below MB beyond its 95 % noise.
- MBN: planned-assault >= 70 %; the notes agree with the teacher's state >= 80 % (held-out).
- The line's goal: >= 22 of 32 families at >= 90 % of the best teacher.
- If after behaviour cloning a siege family is below its target: one fixed DAgger round (labels up to the first
  divergence; beta-mixing 0.5 -> 0.25), the quick eval again; two rounds without movement -> a diagnosis with
  traces (strikes per turn on the city against the teacher, who rested), and stop.
- Per step reported: the 12-family quick table, the memory / history ablations, time per turn, training
  decisions/s, GPU minutes.

### What the A line taught (for the B line)
- M0 (BC) assault-heal 71 % (A0 60); zeroing its memory: 46 %. DAgger lowers assault-heal in both lines (A0 60 ->
  A2 40; M0 71 -> M1 42). Measured cause so far: from the net's states the expert keeps relabelling wounded
  units "move away to the healing ground" (42 % of their labels out of fire vs the teacher's own 18 %; resting
  30 % vs 47 %), and its carried memory said "engaged" in 50 % of engaged turns while the city was untouched
  (teacher 2 %). Reconciling the memory with the real turn (M1r) and beta 0.5 (M1rb) did not restore it
  (40 %, 49 %). The divergence-truncated labels are not built yet.
- The memory trainers v2 (a corridor): no-memory control AHc2 43 % / 50 % (chance), MH0 98 % / 86 %; MH0 with the
  memory zeroed 13 % on ambush-fog, with the facts masked 53 % on hunt-fog: the slots / plan carry the ambush, the
  facts carry the hunt.

## State at 16:10 and the backlog (for the next agent: this one hands off — its context is full)

Done on the B line: MB's code (`model_mb.py`, `train_m.py --map --map-ch`, Rust once-per-turn map in seat / parity),
the function-preserving init `runs/init-MB` (M1 + B1's encoder, projections zero: 1 619 / 1 619 quick-eval plays
equal to M1), the quick eval `runs/m-quick.sh NET OUT [env]`, MB0 training (`runs/train-MB0.sh`, log
`logs/netMB0.log`; 2.8 k decisions/s: the 64-channel encoder over 576 tiles a turn), the divergence-truncated
DAgger labels (`ARMY_DAGGER_TRUNCATE=1`, seat.rs `follow`; built, not yet run) and beta-mixing (`ARMY_DAGGER_BETA`).
Correction: in `table-M-final.json` of 15:50 the column M0z was M0's rows by a typo in the command; the real M0z
(re-run 16:05, identical to the first) is assault-heal 64 / 140 = 46 % (M0 100 / 140 = 71 %): the memory proof stands.

Backlog in the coordinator's order (none started unless said):
1. MB0 -> its quick eval and full table at the gate; the DAgger round on MB with ARMY_DAGGER_TRUNCATE=1 and beta 0.5
   -> 0.25, the quick eval; then MBH, MBN as written above.
2. Aux heads in MB (Roman approved; write their gates before the run: accuracy reported, no family below MB without
   them beyond noise): (a) per-tile danger (`danger_net`'s) on the encoder's cells / unit tokens after layer 1,
   (b) reach (ours / theirs reach the tile this turn) after layer 1, (c) the chosen strike's expected dealt / taken
   on the candidate's final representation, (d) net2's war target / phase on the plan tokens, (e) a value head (win;
   the city taken within N rounds) on the pooled state + plan tokens; small weights 0.1-0.3, each decaying to 0
   over the second half of training; one placement ablation for danger (early vs last layer). Then: linear probes
   per layer per concept (R^2 / accuracy, rank at 90 % R^2) with and without the heads; the causal check
   (project the concept's subspace out per layer: change of choices and quick-eval wins); late fusion of the hand
   value as an input vs the head. Use of (e): advantage-weighted imitation on assault-heal / planned-assault vs
   plain; later a one-turn lookahead.
3. History (MBH) to addition 5's spec: STRUCK_US (attacker type, attack kind melee / ranged / air / city / bombard
   / nuke, origin tile or UNSEEN + bearing / distance class, damage, hp after, intercepted), UNIT_DIED (both sides,
   killer), CITY events, ENEMY_SEEN / LOST, a TURN_END summary per turn (= the plan's progress); a cap of 128 by
   priority (deaths > strikes on us > city > our strikes > seen / lost > plain moves); K by ablation (1, 2, 3, 5);
   pointers to live tokens by unit id (DEAD slot), the tile into the distance bias, age and phase embeddings;
   per-unit aggregates in Rust (damage taken last turn, unseen strikes within 6, air strikes within 8); one more
   memory trainer: an unseen / air attacker hits us each turn from out of sight (the right act: leave its range).
   Report the K ablation and the attention mass per event kind.
   With the cache constraints (Roman, long histories): records encoded once when written (a record encoder that
   sees only the record), keys / values cached per layer, append-only; age as an ALiBi-style query-time bias (+ a
   learned bias per kind); grouped / multi-query attention for the history; per-unit bias to its own records plus
   global ones; consolidation beyond the window into notes / GRU slots; training parallel over a play with a
   causal mask by write time (mx.fast.scaled_dot_product_attention); measure K = 3 / 10 / 30 turns (~100 / 400 /
   1 200 records), time per turn with the cache vs recomputing, Rust parity with the cache.
   With the generic record (V3 later): kind from an extensible vocabulary with a domain (army / city / economy /
   diplomacy / strategy), actor and target pointers, place, a few numbers, age, writer (game fact / net note /
   net2 record), a "salient" flag never evicted (war declared on us, a city lost, a unit type first seen, an ally
   lost). Written into OBSERVATION-SCHEMA.md; only the army domain is implemented.
4. Forecast (after MBH): per enemy token a distribution over its reach tiles after 1 turn, heatmaps after 2 and 3
   (a small decoder on the encoder's grid), "whom it strikes" at 1; free labels from the recordings; log-loss and
   top-1 / top-5 against "stays" and "moves toward our nearest"; if it beats them: expected presence / damage planes
   into a second pass (imagine, then decide), the decision effect on the quick eval (siege + retreat + kite +
   wounded + raid); accuracy against unciv-played enemies separately. Intent per enemy group (attack city X,
   attack our army, raid, escort / settle, hold, retreat / heal, passing by) from hindsight labels (the next 5
   turns), its accuracy per class and its lead time; the MN note THREAT(stack, target) written from it; later an
   input of net2's threat (no change to net2 now).
5. MBN as written (notes), then the capacity ladder (d 48 / 64 / 96 / 128 at 3 layers, 128 at 4 and 6) and the
   function-preserving growth (widen / deepen, check identical play, then train; vs the same size from scratch),
   on the B line. RAM: the .bin files are copied into RAM then onto the device (now the host copy is freed after
   the device copy); reserve with `res run --ram`; past ~32 GB split the data or stream it.
6. Report where the teacher itself is far below 100 % (siege 87, hill-archers 88, citadel 62, choke 80, hold 84):
   only learning from outcomes can pass imitation there.
7. MBS (after MBH; Roman: Mamba): attention across the tokens within a turn + a selective state-space recurrence
   (Mamba-2 / SSD, input-dependent gates) along time per entity slot (each unit's slot, each plan token, one global
   slot) replacing the GRU memory; the history window kept short. MLX: the SSD form as chunked matmuls over a play's
   turns; Rust: one recurrent step per slot per turn. Gates (fixed here, before any MBS code): on the quick eval no
   family below MBH beyond its 95 % noise; the memory ablation loses >= 5 points in assault-heal or planned-assault;
   time per turn <= 2 x A2's; fact survival reported: linear probes of "turns waited" and of "an enemy seen k turns
   ago" from the state at k = 3, 10, 30 (AUC / R^2), MBS against MBH.

## HANDOFF (16:20, for the fresh agent) — read this section first

### Where things stand
- Clone `classic` on `nn/army-net` (platform on main e26f946); commits since 0fff6e6: 00a1a65 merge main 6b810d5,
  326a705 probe guard, c9e9134 M, 6c8c89d memory trainers, 777c118 memory sidecar, 91c6dee MH + trainers v2,
  5c8a445 MN + MB + reconcile + beta, e0364aa divergence-truncated labels, and the handoff commit after it.
- Hashes on this base: `--solo standard` 5d461f2a421f2151, `--solo pre-net2` 380c45a20e751d31 (match).
- M0z answer: the real number is assault-heal 64 / 140 = 46 % (M0 71 %), re-run at 16:05 identical to the first
  run (`runs/eval-M0-zero` = `runs/evalall-M0-zero`). The 71 % in `table-M-final.json` of 15:50 came from a typo:
  the M0z column was given M0's rows file. Rebuilt correctly since.
- MB0 (16:15; init M1 + B1 encoder, 18 min BC on teach4 + dagM1, map once per turn, 64 channels): quick eval
  `runs/quick-MB0.json`: raid 64 -> 98 (beyond noise), hill-archers 75 -> 80, assault-heal 42 -> 43, planned-assault
  9 -> 8, the others within noise of M1. Time per turn on one core 15.0 ms (encode 11.3 ms: the scalar kernel;
  the map-encoder lane owns the kernels) — MB's speed gate fails until that lane lands. Full table:
  `runs/evalall-MB0` and `runs/evalall-MB0-zero` (running at handoff; build the table with tools/m_table.py).

### Commands
- Build: `res run --cpu 2-6 --name army-net -- sh -c 'cd classic && CARGO_TARGET_DIR=<clone>/target-own cargo build --release -p aurelia-qa --bin ai-exam -j $RES_THREADS'`;
  copy the binary to `runs/ai-exam-hN` before long CPU runs (a rebuild must not change a running job).
- Quick eval (12 families, ~2.5 min for MB, 40 s for M): `BIN=runs/ai-exam-h9 sh runs/m-quick.sh NET OUT [ENV=..]`.
- Full eval (32 families): `KEPT=data/kept-m.jsonl BIN=... sh runs/m-eval.sh NET OUT [ENV=..]` (ARMY_NET_ZERO_MEMORY=1,
  ARMY_NET_NO_FACTS=1, ARMY_NET_NO_HISTORY=1 for A nets).
- Table: `python3 tools/m_table.py runs/table-X.json NAME=runs/evalall-NAME/rows.jsonl ...` (A0 / A2 / AH0 / AH1
  added by itself; teachers from runs/eval-base + runs/teach4-hold; "*" pass, "v" below A2 beyond noise).
- Train: `classic/nn` `python -m aurelia_nn.army.train_m [--history | --map --map-ch 64] ...`, MN: `train_mn`;
  GPU through `res run --gpu --cpu 2 --ram 32`; each job `--max-minutes <= 18`.
- Parity: `PLAYS=teach4-hold.bin sh runs/m-parity-smoke.sh NET` (Rust vs MLX over 150 multi-turn plays).
- Probes: `python -m aurelia_nn.army.probe_m --plays runs/teach4-hold.bin --weights runs/NET/weights.safetensors --out ...`.
- DAgger: `runs/m-dag1.sh` pattern (ARMY_NET, ARMY_DAGGER=teachers.json, --record); switches ARMY_DAGGER_TRUNCATE=1,
  ARMY_DAGGER_BETA=0.5, ARMY_DAGGER_NO_RECONCILE=1.

### Table files
- `table-M-final.json`: the A and M line on 32 families (A0, A2, AH0, AH1, M0, M0z, M1, M1z, M1r, M1rb, M1b, AHc2,
  MH0, MH0nf, MH0z). M0 / M1 rows of ambush-fog / hunt-fog are of the trainers' v1 (invalid: v2 replaced them).
- `table-M0.json`, `table-M1.json`, `table-M1b.json`, `table-MH0.json`, `table-M1fix.json`: the step tables.
- `quick-MB0.json`: MB0 against M1 on the quick eval. `probe-M1.json`: proof (b). `page-extra.json`: the page's text.

### Data (disk ~29 GB free at handoff; keep >= 25)
- `teach4-train.bin` / `teach4-hold.bin`: record v4 (history, track, memory states), 32 families incl. memory v2.
- `dagM1.bin`: v3 DAgger of M0 (no history). `teach-train-h.bin`, `teach-hold-h.bin`, `teach-*.bin` (v2), `dag*` of
  the A line: the previous agent's (superseded; delete only by the coordinator's word).

### Open, in order (the gates are written above, before their runs)
MB0 full table -> MB DAgger round (ARMY_DAGGER_TRUNCATE=1, beta 0.5 -> 0.25) -> MBH (zero-init history; addition 5's
events; the cache design) -> MBN (zero-init notes) -> MBS -> aux heads (placement, decay, per-layer probes, causal
check, late fusion; value head and advantage weighting) -> forecast and intent -> the ladder and growth on the B line.
Backlog items 1-7 above hold the specs. The page: work/army-net-2026-11-23.html (tools/m_page.py, page-extra.json).
Not covered by the sidecar: MH's past plan states and MN's notes (only M's memory is saved beside the game).
- MB0 full table (16:30, `runs/table-MB0.json`): 18 of the 30 old families at >= 90 % of the best teacher (A2 17,
  M1 16), 18 of 32 (the memory trainers: ambush-fog 64 %, hunt-fog 47 % — below; MH0 had 98 / 86); none below A2
  beyond noise; raid 98, prey-land 78 (N 9), assault-heal 43 (memory zeroed: 28), planned-assault 8. MB gate:
  families >= 18 met; raid >= 85 met, prey-land >= 85 not (7 of 9); time per turn 15 ms not met (kernel lane).

### Corrections (coordinator, 16:35)
- **MBS (the Mamba variant, backlog item 7) is POSTPONED** (Roman: «MBS пока не надо, потом мб»): kept as a later
  option with its description and gates; it is NOT part of the B-line order. The order is MB -> (DAgger round on MB)
  -> MBH -> MBN, then the aux heads, the forecast / intent, the ladder and growth.
- **MB0 is unfinished**: it stopped at the 18-min GPU-job limit at step 499 of 980 (held-out joint accuracy 0.810;
  `logs/netMB0.log`). The numbers above (18 of 30, raid 98, ...) are of this half-trained net. The next agent
  resumes it from `runs/netMB0/last.safetensors` (`train_m --map --map-ch 64 --init runs/netMB0/last.safetensors`,
  the same data, the remaining ~1 epoch, lr continuing low, e.g. 1.5e-4 cosine) before the quick eval and the MB gate.

## Addition 8 (coordinator, Roman, 23 Nov ~16:40-17:00): the price of a capture, outcomes, checkpoints — written before any run of it

Roman: «иногда хорошо взять город любой ценой… мы не должны стараться закрепить это поведение». The siege trainers'
win rules carry a loss cap (assault-heal, promotion-choice: <= half lost; hill-archers, city-fire, planned-assault:
<= 1 lost) and the teachers rotate the wounded, so imitation and the gates reward unit preservation everywhere.

### Reporting (from now on, every table)
- Beside the win (capture within the rounds AND within the loss cap, unchanged): the CAPTURE rate alone and the
  losses per play (mean `lost`), for the nets and the teachers. To see a capture after the cap broke, the quick
  eval runs with `ARMY_PLAY_ON_CAP=1`: an assault scene whose cap broke plays on to its capture or its round limit;
  the verdict stays a loss (the measure `cap_broken` = the round it broke, -1 never). The win rules do not change.

### The price as an input (not a fixed style)
- A price vector of 3 (the capture's worth in units / 8: how many of our units the capture is worth; time
  pressure: 1 / rounds until the deadline that matters (reinforcements, peace, a rival), 0 none; "price given" 1)
  enters through its own projection added to the OBJECTIVE token's embedding (`h_0 += W_price * price`): the
  objective is "given from above", the price too. Not as token fields: every one of the 48 token columns is used
  by some token kind (22-24 are arms of a unit), so a shared column would change the units' embedding. It is per
  play (the play's json `price`, recorded; Rust: `Orders::price`). In the trainers the scene sets it (the careful
  families: worth = the loss cap, pressure = 1 / the round limit; the "any cost" families below: worth = the whole
  army, pressure = 1 / the deadline); in the game from net2's war target / phase (later; no change to net2 now).
- Function-preserving: `W_price` zero at the init, so the net plays exactly as before until trained (checked on
  the quick eval: every play identical).

### "At any cost" trainers (2-3, each proven before it counts)
- `assault-reinforce`: the assault scene; at round N (6-8) enemy reinforcements (2-3 of the era's best melee)
  enter the city; win = the city taken before round N, no loss cap.
- `assault-peace`: the city is the war's goal; peace at round N (8-10): win = the city taken by round N, no loss cap.
- `capital-race` (proposed, later): a rival's army marches on the same city; whoever takes it first.
- Proof (each): a reference that pays units to take it in time (`ref-burst`: every unit strikes the city every
  turn, the capturer goes in at the first chance, no rotation) wins >= 70 % (held-out); the careful rotation
  (`ref-planned`) <= 30 %; the passive control loses. Reported: A2 / M1 / MB0 / the teachers there.

### Gates (new; the old gates stay as written and are reported too)
- P1: the price fields zero-initialised: every quick-eval play identical to the net without them.
- P2: on the "any cost" families the net with the price >= 70 % of `ref-burst`, the old families not below the
  net without the price beyond the family's 95 % noise.
- P3 (the trade-off is conditional): the same net on the same held-out scenes with the price set careful vs "any
  cost": with "any cost" the capture rate is higher and the losses per play higher (paired, both beyond noise).
- V1 (value head, outcome learning): the value head predicts (a) the win, (b) the capture, and (c) the change of
  the potential Phi over the next 3 turns (below); held-out AUC (a, b) and R^2 (c) reported. Advantage-weighted
  imitation (weight exp(A / beta), A = shaped return - V) vs plain on the quick eval: no family below plain beyond
  noise, and above in assault-heal or planned-assault, or a diagnosis.

### Potential-based shaping only (coordinator ~16:50)
- reward += Phi(s') - Phi(s) per turn (the best policy unchanged: a retreat to heal that leads to the capture is not
  punished in the long run). Phi = the city's hit points taken off (share) + per unit: closeness to a firing tile
  if hp >= 60 %, closeness to a healing tile weighted by its missing hp if below + the army's expected damage per
  turn on the city against the city's heal (20) - units lost x the price (field 22). Never a plain "distance to
  the city" reward (it teaches marching the wounded into the city's fire). The best-of pilot ranks attempts with no
  win by the same Phi.

### Checkpoints (coordinator ~16:55)
- Every B-line training saves `runs/<run>/ckpt/e<epoch, 2 decimals>.safetensors` + its Rust export `.bin` at each
  half-epoch and at each job's end, a line {"epoch", "path"} in `ckpt/index.jsonl`; the last 8 kept
  (`train_m.save_checkpoint`). Another lane evaluates them; this lane does not.

### Done for it at 17:00
- `train_m`: the recordings streamed (indexed, a step reads only its plays from the mapped files: RAM ~ the index),
  resumable jobs (`--resume`: OUT/state.json, OUT/opt.safetensors; without the optimizer state a 20-step lr ramp),
  `runs/train-chain.sh` (<= 18-min jobs until done), the checkpoints. `ARMY_PLAY_ON_CAP` in the trainers.
- Merged `perf/map-encoder` 54d2d64 (the conflict: MB's recompute replaced by `map_conv.rs`; the token / candidate
  map reads take `Encoder::channels`).

## Paper plan (coordinator, Roman 23 Nov ~18:00: MBN may become a paper — "a small LLM-like net that writes and reads its own structured thoughts"; no change to the order before MBN)
When MBN is reached, its experiments are designed for publication:
1. The ablation ladder, >= 3 independent training seeds each, intervals over seeds: no memory / memory only (GRU
   slots) / + history / + thoughts (MBN).
2. The intervention test: force a thought into the stream (PLAN_CHANGE, PULL_BACK_HEAL(unit), WAIT_STAGE(k),
   PLAN(other target)) at chosen turns of held-out plays; measure whether the behaviour follows (the unit pulls
   back, the strike waits, the army re-targets) against a control with a neutral thought — the causal evidence that
   the net acts on its thoughts.
3. Thought accuracy against the teacher's state, and its correlation with winning.
4. Sample efficiency: held-out wins against the amount of training data, with and without thoughts.
5. Readable traces of 3 plays.

## State 19:25 (the B-line agent of the evening)
- Step 1 done: MB0 to 2 epochs (resumed at step 499; streamed data; `runs/train-chain.sh`), quick eval after the
  main merge `runs/quick-merge.json` (A2 / M1 / MB0 on the new base; moved <= 2 points). MB gate: families 18 of 32
  (16:30 table), raid 96, prey-land 9 / 9, time per turn 3.25 ms (kernels merged) — all met.
- "Any cost" trainers (8e1529a, kept `data/kept-anycost.jsonl`): held-out ref-burst 99 / 98, ref-planned 12 / 19,
  standard 52 / 65, unciv 48 / 40; A2 32 / 2, M1 13 / 3, MB0 12 / 1 (reinforce / peace win %): the nets learned
  carefulness as a constant, as Roman feared.
- Step 2 running: MBc (control: MB0 + 1 epoch), MBa (heads + value + price), MBv (MBa + AWI beta 0.5); quick
  `runs/quick-step2.json` (tools/quick_table.py), probes `runs/probe-<NET>.json` (probe_layers, slow on CPU: ~35
  min a net). MBa: 9 of 13 quick rows (MBc 7, MB0 8), value head win accuracy 0.848 (base 0.649), Phi-change R^2
  ~0.30 (mse 0.085, var 0.122); parity MBa Rust vs MLX 2.9e-5.
- Step 3 started: DAgger round on MBa (`runs/m-dag-mb.sh netMBa dagMBa1`, truncated labels, beta 0.5) -> train
  MBd1 from MBa on teach4 + dagM1 + dagMBa1, 1 epoch -> quick eval.

## MBH0 spec (written 19:45, before any MBH code or run)
- MBH0 = MBA + the history of facts that record v4 already holds (`F_HIST` 20, H <= 32, this turn and the last 3;
  `teach4-*.bin`, `dagM1.bin` carry it), so no re-recording for the first measurement. Addition 5's richer event
  spec (STRUCK_US with attack kind / unseen origin, UNIT_DIED both sides, CITY, ENEMY_SEEN / LOST, TURN_END, priority
  cap 128), the generic record with the salient flag, and the cache measurement (K = 3 / 10 / 30) are MBH1, after.
- In the body: per layer a SEPARATE cross-attention of the tokens and plan tokens to the history (its own softmax,
  one always-present null key so an empty history is defined), q / k / v the layer's own (on norm1 of the history
  embedding), bias per head for the pair (token, a fact of the same unit), age as an ALiBi-style query-time bias
  (-slope_head x age; age is not in the keys, so keys / values can be cached when written), and its output through
  a per-layer projection `ho_l` initialised to ZERO and added to the residual. Why separate: a joint softmax with the
  tokens (MH's) would change MB's attention the moment history keys exist, so the start would not equal MB.
- The history embedding: `h2(relu(h1(fact))) + hp(P_0 of the fact's turn)` (MH's link to the plan). MB's memory
  update (no MH progress gate in MBH0: its input size would break the init).
- Function-preserving check: MBH0 at the init plays every quick-eval play exactly as MBa. Parity Rust vs MLX <= 1e-4.
- Gates (as written at 15:40): assault-heal >= 80 % and planned-assault >= 50 % (held-out); no family below MB
  beyond its 95 % noise; plus the ablation (history masked) and time per turn <= 4.6 ms.
