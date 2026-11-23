# The army net's observation — one schema (Rust builds it; MLX reads it)

Built in ONE place: `classic/crates/qa/src/army_net/observe.rs` (`tokens`, `candidates`, the history), recorded by
`record.rs`, read by `nn/aurelia_nn/army/data.py`. Every number is rounded through f16 (as stored). The builder is
meant to be a pure function of flat state tables (units, tiles, cities, the event log) and the memory it is given;
the gaps against that are listed at the end.

## Inputs (flat tables)
- units: id, owner, kind, tile, hp (0-100), moves left / max (1/MOVE_DENOMINATOR), attacks made, stance, level, xp,
  promotions available; cities: tile, owner, hit points / max, strength, strike made, population; tiles: terrain
  (hills, mountain, water, rough), feature, river sides, road, owner, improvement (pillaged), citadel; the orders
  (side, foe, aim, objective tile, refuge, rounds, start round, army ids, mean strength, window, fog); the event log
  (below); the visibility of `side` (fog families only).

## Tokens: `T <= 40` rows of `F_TOK = 48` f32 (order: the objective; our units by id; the foe's units by id (fog
families: the ones `side` sees); our cities by tile; the foe's cities by tile)
| idx | objective | unit (ours / foe) | city |
|---|---|---|---|
| 0-3 | kind one-hot: objective, own, foe, city | | |
| 4, 5 | — | (col, row) - objective's, / 12 | same |
| 6 | — | hex distance to the objective / 12 | same |
| 7 | aim one-hot 7..14 (destroy, hold, take city, protect, pillage, survive, catch, escort) | hp / 100 | hp share |
| 8 | | moves left / max | |
| 9 | | max moves / denominator / 5 | |
| 10 | | can attack (no attack made) | |
| 11 | | strength / mean strength | strength / 100 / mean |
| 12 | | ranged strength / mean | can strike |
| 13 | | range / 3 | |
| 14-26 | 15: rounds left / 30; 16: rounds / 30; 17: optional; 18: own army count / 8; 19: foe military / 8; 20: own hp sum / 800; 21: foe hp / 800 | arm one-hot (12 arms of `lab::arms_of`, civilian) | |
| 27, 28 | | fortified, set up | |
| 29 | | embarked | |
| 30, 31, 32 | | level / 5, xp / 60, promotable | |
| 33, 34, 35 | | tile defence / 100, hills, rough | |
| 36 | | ours: in a foe city's fire | |
| 37, 38 | | ours: adjacent foes / 3, friends / 3; foe: our units adjacent / 3 | |
| 39 | | ours: threat (foes reaching it next turn) / 4; foe: our units reaching it / 4 | |
| 40, 41 | | nearest foe / 12, refuge distance / 12 (foe: nearest of ours / 12) | |
| 42 | | selectable | |
| 43, 44, 45 | | | is the objective, population / 10, ours |
| 46, 47 | | hp gained since our previous turn began / 100, left its tile since then | |
- `dist`: `T x T` u8, hex distance clipped at 15 (the attention bias); `selectable`: `T` u8; `pos`: `T` i16 window
  index (-1 outside); `slot`: `T` i16, the unit's index in the army (-1: not an army unit; < 16).

## Candidates of the acting unit: `C` rows of `F_ACT = 40` f32 ("done" first, then the rules' legal list)
0-9 kind one-hot (done, move, attack unit, attack city, ranged, fortify, set up, promote, pillage, other); 10-26 the
subject tile against the unit's own (dq, dr / 12, max / 5, defence / 100, hills, rough, city fire, adjacent foes / 3,
friends / 3, threat / 4, nearest foe / 12 and its change / 5, objective distance / 12 and its change / 5, refuge / 12,
on the objective, own land); move: 27 moves left after / max, 28 best blow from there, 29 foes it reaches / 3;
strike: 30 dealt / 100, 31 taken / 100, 32 defender hp / 100, 33 kill, 34 striker dies, 35 target ranged, 36 target
civilian, 37 target city, 38 target strength / mean, 39 no preview. `promo`: u16 (promotion id + 1); `pos`: i16.

## Map (variants B, MB): the 24 x 24 window (axial q, r from the arena's centre, clamped)
16 static planes per tile (on the map, hills, mountain, water, rough, our defence / 100, river side, road, owned by us,
by the foe, improvement, citadel, objective, refuge, our city, the foe's city), then the 33 local numbers of every
token on the tile, summed (`model.rs::LOCAL`): 49 input planes.

## History of facts (variants MH, MBH): `H <= 32` rows of `F_HIST = 20` (this turn and the last 3; most recent first)
0-6 our command (move, strike a unit, strike a city, ranged, fortify / set up, promote / pillage / other); 7 our unit
hit in the enemy's turn; 8 a foe came into sight; 9 a foe went out of sight; 10 age / 3; 11, 12 the subject tile
against the objective / 12; 13 its distance to the objective / 12; 14 dealt / 100; 15 taken / 100; 16 kill; 17 died or
captured; 18 moved / 5; 19 on a city (a strike at a city; a hit in a city's fire). Per row: `slot` i16 (the unit;
-1: a foe's event), `age` u8. Addition 5's richer event spec (attacker type, attack kind, unseen origin's bearing,
TURN_END summary, priority cap 128) is NOT in this layout yet.

## Memory (variants M*): given to the builder's consumer, not built
- per army slot (16) a vector of 16: 0 turns waited, 1 resting since, 2 engaged, 3 hp at last turn end, 4-15 free;
- 4 plan tokens of d: P_0 named dims 0-5 stage logits, 6 turns in the stage, 7 balance at commitment, 8 eta, 9
  rounds left, 10 engaged, 11 waited, 12 the city's share at commitment (MH); the rest free;
- alive mask (16), MH: P_0 of each past turn; MN: the note stream (stage one-hot 6, waited, free 8, change age,
  reason one-hot 4, per slot PULL_BACK_HEAL active and its age).
- Note tokens (MN): `F_NOTE = 25`: kind one-hot 5 (PLAN, PLAN_CHANGE, WAIT_STAGE, PULL_BACK_HEAL, RETURN), stage 6,
  reason 4 (losses, city stronger, advance, other), waited / 10, age / 3, free 8; slot i16.

## Record (v4, `"ARM4"`): per play json + statics (W*W*16 f16) + decisions; per decision the tokens, dist, selectable,
danger (T f16), candidates, promotions, positions, flags (bit 0: memory state), slots, the teacher's track (engaged,
waited, stage, resting bits), the history (H, rows, slots, ages). Byte layout: `record.rs` module doc.

## Gaps against "a pure function of flat tables" (to close before an MLX twin)
- observe.rs keeps per-play thread-local state: the hit points / tiles at our last two turn starts (numbers 46, 47),
  the event log and the last turn end's snapshot (the history). They should become an explicit input (a
  `PlayLog` table: per round per unit tile and hp at our turn start and end, and the event rows) that the caller
  owns and passes in, as the memory is.
- The candidate list comes from the rules' legal actions (`knowledge::actions::unit_actions`) and the strike
  previews (`act::preview`): a twin needs those as tables (legal moves with moves left, previews) or the rules in MLX.

## The history record, generic (for V3's whole-game history; only the army domain is implemented)
A record: `kind` (an extensible vocabulary, u16) with `domain` (army / city / economy / diplomacy / strategy),
`writer` (game fact / net note / net2 record), `actor` and `target` (pointers: a unit / city / player id, resolved to a
current token when it is alive, else a DEAD slot), `place` (a tile, or none), `numbers` (a few f16, the kind's own:
damage, hp after, hp delta, gold, ...), `age` (turns; a query-time bias, not in the cached keys), `salient` (never
evicted from the window: war declared on us, a city lost, a unit type first seen, an ally lost). Today's army facts
(F_HIST above) map onto it: kinds 0-9 of domain army, writer = game fact, actor = the slot's unit, place = the subject
tile, numbers = dealt, taken, kill, died, moved, on a city.
