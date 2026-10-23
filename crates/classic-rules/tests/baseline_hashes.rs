//! Behaviour baseline (M3.0): world hashes of fixed games recorded BEFORE the
//! effect system and the systems split. Every later refactoring must keep these
//! numbers: same content semantics + same commands = same authoritative hash.
//!
//! Two games:
//! - `core_game`: the frozen legacy content (`classic_content::legacy`, a copy of
//!   classic/assets/rules/core.ron + civs.ron at the M3.0 merge) driven by the greedy AI
//!   plus scripted production orders (monument, granary, wonders, ...), so
//!   building effects, tech bonuses, growth, culture, combat and research all
//!   feed the hash. The command log is replayed on a fresh world.
//! - `fuzz`: the same 40 random worlds x 150 random commands as fuzz_sim.rs
//!   (see `fuzz_world_hash_baseline` there).
//!
//! Run with `--nocapture` to print the hashes.

// Frozen legacy content: the pins below were recorded on it.
use classic_rules::ClassicGame;
use classic_content::legacy::legacy_rules as core_rules;
use classic_rules::ai::{war_declarations, GreedyPlanner, Planner};
use classic_rules::ids::{BuildingKindId, CityHandle, PlayerId, ProductionItem, UnitKindId};
use classic_rules::replay::authoritative_hash;
use classic_rules::scenario::{generate, ScenarioConfig};
use classic_rules::{compile_rules, Command, CompiledRuleset, GameState};

// Re-pin log: every deliberate behaviour change that moves these numbers
// adds one line here in the commit that makes the change (no silent re-pins).
// The same log covers FUZZ_WORLDS_HASH in fuzz_sim.rs.
// - M3.0 baseline: core d14e65e83a663c1a / rounds 99b286830ce82fb3 / fuzz 04efe4aa398df193.
// - M3 hex topology: A* walks six hex directions, melee/siege reach hex
//   neighbours only, sight is a hex disc with a hex-line LOS (was a square
//   with 4/8 square neighbours): paths, fights and visibility change.
// - M3 one city per tile: FoundCity checks the site (no city within
//   MIN_CITY_RANGE, no foreign land) and the synthetic generator drops a
//   second city drawn on an occupied tile: fuzz worlds change, core_game not.
// - M3 rounds: TurnState holds the global round and the active player
//   (sequential turns, only the active player acts). The hash mixes the
//   round instead of the per-EndTurn counter; core_game plays the same 711
//   commands, the fuzz script now targets the active player.
// - M3 attacks spend moves: an attack needs moves left and zeroes them,
//   civilians do not attack. Units no longer strike several times a turn
//   (core_game 711 -> 672 accepted commands); fuzz unchanged.
// - M3 owning city per tile: founding takes only unowned ring tiles,
//   culture and purchase extend the city's own border, capture transfers
//   only the city's tiles, cities work only their tiles; the hash mixes
//   the owning city of owned tiles.
// - M3 entity identity: units and cities carry persistent ids, iterate in
//   id order (not slot order) and keep per-player indexes in that order;
//   the hash mixes the ids and the id counter. Same 672 commands.
// - M3 save checks + effective active player: a player with nothing never
//   holds the round (fuzz worlds whose player 0 got nothing at generation
//   were stuck on its turn); core_game unchanged.
// - M3 closed borders on the whole path: A* never steps on foreign land
//   without war or a grant (fuzz moves route around); core_game unchanged.
// - M3 movement: move points (60 per point), terrain/feature/hills
//   costs, roads between road tiles, river crossings and zones of control
//   take all moves, the partial-move rule, turn-aware A*, one military and
//   one civilian unit per tile, trained units step out of a full city,
//   moves walk step by step (core_game 672 -> 569 accepted commands).
// - M3 healing: resting units heal 25 in a city, 20 in friendly land,
//   10 elsewhere (was 10, fortified 15): round hashes change, final not.
// - M3 move orders: MarchTo keeps a goal (hashed) and units walk on at the
//   start of their owner's turn; an enemy in sight or a blocked way cancels
//   the order. AI marches now continue between plans.
// - M3 player knowledge: the hash mixes tile memory (owner, improvement,
//   road, feature, city, last seen round) and contacts. Same 569 commands.
// - M1c classic-like land (scenario/landscape.rs): plot types on ridges, climate
//   bands with tundra (code 7), downhill rivers from hills, ice, jungle,
//   marsh, oasis, flood plains, atolls, natural wonder sites; the hash mixes
//   the sites. Seed 0x5EED_0003 no longer reached a second wonder in 120
//   rounds on its new map; 0x5EED_0008 does (646 commands, 9 building kinds).
// - M1c sight over water: water no longer blocks line of sight, mountains
//   do; a shore unit sees across a bay. Same 646 commands, visibility and
//   tile memory change.
// - M4 culture (track B): cities make no culture per citizen (culture
//   comes from buildings, tiles and great works), the border grows at most
//   one level per turn, a purchased tile does not raise the culture level,
//   and the player's culture pool (with its last turns) is hashed. Same 569
//   commands; borders grow slower (only Monument cities grow them).
//   Fuzz worlds: the culture pool history of every player is hashed.
// - merge of M1c land (main) and M4 culture (track B): both changes above
//   apply together; the pins below are the merged game (646 commands).
// - M4-A tile yields (a behaviour fix: v1 yields did not follow the classic
//   rules): the frozen content gets the classic terrain table (its v1 code
//   table is gone),
//   cities work 3 rings by tile yields per channel, legacy `Output`
//   modifiers count as production, gold comes from tiles, culture from
//   buildings only, resources yield once revealed, improvements obey their
//   placement; the hash mixes the economy section (citizen orders, golden
//   ages, great people counters). On main's classic-like land and sight
//   rules
//   (merge of M1c): natural wonder tiles yield their own yields, lakes are
//   fresh water; the fuzz fixtures get the classic terrain table as well.
//   core_game 646 -> 660 accepted commands.
// - M4-A growth and great people (a behaviour fix: v1 growth cost 10+4·pop
//   did not follow the classic rules): growth threshold 15 + 8(p−1) +
//   (p−1)^1.5 with the food store in hundredths, starvation, kept food
//   (aqueduct), settlers
//   need size 2 and take the food surplus as production; great person
//   points from specialists, births and the golden age countdown step; the
//   hash mixes the city food store, kept food and great person points.
//   core_game 660 -> 638 accepted commands; fuzz worlds re-pinned.
// - M4-A happiness and golden ages: a happiness snapshot step and the
//   golden age meter run every turn; the economy section hashes the
//   difficulty, AI players, the snapshot and how cities are annexed (the
//   legacy content has no handicap table: no unhappiness, same 638
//   commands); fuzz worlds re-pinned.
// - M4-A gold (a behaviour fix: v1 tile prices by ring and turn did not
//   follow the classic rules): classic tile prices (50 + 5 per tile bought,
//   × distance, −100 % for a visible resource, rounded to 5) and the count
//   of tiles bought in
//   the economy section; the legacy content has no handicap table, so no
//   maintenance (its v1 upkeep of 1 gold per unit is gone) and no
//   deficit. Same 638 commands; fuzz worlds re-pinned.
// - merge of M4 track B (culture) with M4-A (yields, growth, happiness,
//   gold): both apply; the world hash mixes both sections. Same 638
//   commands; fuzz worlds re-pinned.

// - M4 combat (required by the classic rules): the damage formula (2400 + rand 1200
//   × strength ratio, wounded strikers weaker, mutual-kill rule), city
//   strength 800 + 40/citizen + technology share and 200 HP that heal 20 a
//   turn, terrain defense, 20 %/turn fortification, flanking, experience;
//   civilians are captured instead of fought; attack counters, stance and
//   city damage/status are hashed. core_game 646 -> 637 accepted commands,
//   fuzz worlds change with the formula.
// - M4 diplomacy: war needs contact and a peace treaty forbids war for 10
//   rounds (fuzz scripts lose their wars between strangers); the hash mixes
//   relations (war start, warmonger memory) once they exist: core_game
//   round hashes change, same 637 commands and final state.
// - M4 victories: the outcome is part of the truth; one of the 40 fuzz
//   worlds ends with its last civilization standing (domination, round 11)
//   and now records it: fuzz 131f5f5197958fc5. core_game unchanged.
// - merge of M4 track B (culture, faith, tourism) and track C (combat,
//   diplomacy, barbarians, city-states, victories): both apply; same 637
//   commands. fuzz ced4a207b88b452c.
// - merge of M4 track C with M4-A (yields, growth, happiness, gold,
//   resources): both apply; 634 commands. fuzz 51d6db0618f95a9b.
// - QA no default order (merged with M4-A, whose processes keep): a city
//   with an empty queue and no process builds nothing and keeps its
//   production (there is no default order; the free warriors crowded long
//   AI games). Players that lose their last city are eliminated with their
//   units. core_game 634 -> 635 commands; fuzz 6a53c4569bf30a31.
// - cities keep the round they were founded or acquired (capture gold):
//   the field is hashed, nothing else
//   changes (same 635 commands; without the hash line the old pins hold).
//   core_game 49dc5977f36b832e / rounds 0d20fb0a9a11f848, fuzz
//   6204054233816a1c; classic_game (after the merge with workstreams B and E1)
//   88ff2be81f53785a / rounds 93cf3b2585f8dc28.
// - Sight (a behaviour fix): a city sees 3 rings, every owned tile
//   shows its neighbours, a unit sees its kind's sight plus its promotions
//   (+1 on hills), forest and jungle hide what lies behind them from flat
//   land. Accepting a deal that is no longer possible is refused before the
//   offer leaves the list. Same 635 commands; core_game c64aec04cf049a3c /
//   rounds b928771387e4f42c, fuzz 59519b0a7a7be76c; classic_game
//   a3cbc4be4c89e581 / rounds a1a96d17b9897515 (786 commands).

// - City names (Codex UI pass): a founded city takes the next free name of its
//   civilization's list, else "<civ> Settlement" numbered, unique across the game
//   (world::city::next_city_name); names are hashed. Same 635 commands;
//   core_game 393e0b90a56116cc / rounds f63deb61ee1f189e, fuzz 7dc75f880815ed79;
//   classic_game b223b5a52a955f33 / rounds b71f100b00604e7b (786 commands).

// - 17 October, cost formulas with every game-setup modifier (classic/docs/classic/rules.md; `derived::research`): a technology costs base ×
//   difficulty % × map size % ÷ the known discount × (1 + per-city % × cities ever),
//   an AI's institutes cost 75 %, buildings and national wonders grow with the
//   owner's cities. The AI plays other commands on other costs (core_game 635 -> 598, classic_game 786 -> 784); core_game
//   0x393e0b90a56116cc -> 0x69b79c9c66e1d3fd / rounds 0xf63deb61ee1f189e ->
//   0xb78e04536accd606, fuzz 0x7dc75f880815ed79 -> 0x7b4e7b5a0576c963; classic_game
//   0xb223b5a52a955f33 -> 0xc1d063dedfd81f3d / rounds 0xb71f100b00604e7b ->
//   0xd7c843401139c2d5 (784 commands).
// - 18 October, per-unit sight with embarked sight: a
//   land unit on water is embarked and sees EMBARKED_SIGHT (1) tile, not 2.
//   `core_game` and the fuzz worlds start with generator units standing on
//   water (the synthetic generator places units on any tile): their sight
//   shrinks, knowledge and the greedy AI's choices follow. Same 598 commands;
//   core_game 0x69b79c9c66e1d3fd -> 0xa963432a39f66979 / rounds
//   0xb78e04536accd606 -> 0x3a42fe5ad0adc702, fuzz 0x7b4e7b5a0576c963 ->
//   0x14e4e2f7247547e4. classic_game unchanged.

// - 19 October, observation memory (R352): the state hash folds in the last seen deposit of
//   every remembered tile and each remembered city (owner, name, population, era,
//   buildings). The game is unchanged: with those two lines out of the hash every pin above
//   holds. The final hashes are re-pinned once, for this reason only.

// - 19 October, observation memory (R355): a deposit on an explored hex is read from the map, filtered by
//   the player's technologies, not from the memory (the packed field stays in the save and the hash). Static
//   Static maps never change a deposit, so no AI choice and no pin moves.

// - 19 October, the AI's pantheon by the score of its effects (`ai::pantheon`) and a prophet's
//   Holy Site only on a suitable terrain: `core_game` and the fuzz worlds are played by the
//   greedy AI, which founds no pantheon, so they are unchanged. classic_game (new pantheons)
//   and the smoke suite of the AI bench (tests/ai_bench.rs) are re-pinned.

// - 21 October, the rule data re-sourced from Unciv (docs/classic/RESOURCING.md): the terrain
//   table now has Unciv's coast and ocean gold (1), the maintenance, cost and technology fields
//   of the entries Unciv has are Unciv's (buildings, units, wonders, eras), name pools are
//   Unciv's. core_game 0x81a7e0c501063420 -> 0x2545ac3987c541de / rounds 0x88a0e1b78b29920f ->
//   0x044cf2c9b935b878 (598 commands), fuzz 0x9819f83a34822c56 -> 0x39e0f81700c93496;
//   classic_game 0xc634056b89b0656e -> 0xfddd23c9bab27d14 / rounds 0x9e7bdac3e5693d86 ->
//   0x90d8bd975657d687 (784 commands).

/// Hash of the final state of `core_game`.
const CORE_GAME_FINAL_HASH: u64 = 0x6f51419bd31828cb;
/// Fold of the per-round hashes of `core_game`.
const CORE_GAME_ROUNDS_HASH: u64 = 0xef4db1f4d90537c;

const ROUNDS: usize = 120;

/// Buildings tried in this order when a city queue is empty.
const BUILD_ORDER: [&str; 10] = [
    "core:building/monument",
    "core:building/granary",
    "core:building/pyramids",
    "core:building/great_library",
    "core:building/library",
    "core:building/shrine",
    "core:building/barracks",
    "core:building/oracle",
    "core:building/market",
    "core:building/temple",
];

fn scenario() -> ScenarioConfig {
    ScenarioConfig {
        seed: 0x5EED_0008,
        players: 3,
        width: 28,
        height: 28,
        cities: 6,
        units: 18,
        water_ratio: 10,
    }
}

fn building_id(rules: &CompiledRuleset, key: &str) -> BuildingKindId {
    let index = rules.buildings.iter().position(|b| b.key == key);
    BuildingKindId(index.expect("core building must exist") as u16)
}

fn unit_id(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    let index = rules.units.iter().position(|u| u.key == key);
    UnitKindId(index.expect("core unit must exist") as u16)
}

fn own_cities(game: &GameState, player: PlayerId) -> Vec<CityHandle> {
    let auth = game.view().auth;
    let mut out = Vec::new();
    for (handle, core) in auth.cities().iter() {
        if core.owner == player {
            out.push(handle);
        }
    }
    out
}

/// Production orders for every city with an empty queue: first building of
/// BUILD_ORDER that validates, otherwise a warrior.
fn production_orders(game: &GameState, rules: &CompiledRuleset, player: PlayerId) -> Vec<Command> {
    let mut out = Vec::new();
    for city in own_cities(game, player) {
        let queue_empty = game
            .view()
            .auth
            .cities()
            .get(city)
            .unwrap()
            .queue
            .is_empty();
        if !queue_empty {
            continue;
        }
        for key in BUILD_ORDER {
            out.push(Command::EnqueueProduction {
                player,
                city,
                item: ProductionItem::Building(building_id(rules, key)),
            });
        }
        out.push(Command::EnqueueProduction {
            player,
            city,
            item: ProductionItem::Unit(unit_id(rules, "core:unit/warrior")),
        });
    }
    out
}

fn player_commands(game: &GameState, rules: &CompiledRuleset, player: PlayerId) -> Vec<Command> {
    let mut out = war_declarations(&game.ai_observation(rules, player));
    out.extend(GreedyPlanner.plan(game.ai_observation(rules, player), rules));
    out.extend(production_orders(game, rules, player));
    out.push(Command::EndTurn { player });
    out
}

/// Executes one command; successful ones go to the log.
fn execute_logged(
    game: &mut GameState,
    rules: &CompiledRuleset,
    cmd: Command,
    log: &mut Vec<Command>,
) {
    if game.execute(rules, cmd.clone()).is_ok() {
        log.push(cmd);
    }
}

fn mix(acc: u64, value: u64) -> u64 {
    (acc ^ value).wrapping_mul(0x100000001b3)
}

struct CoreGame {
    final_hash: u64,
    rounds_hash: u64,
    log: Vec<Command>,
}

fn play_core_game(rules: &CompiledRuleset) -> CoreGame {
    let mut game = generate(&scenario(), rules);
    let mut log = Vec::new();
    let mut rounds_hash = 0xcbf29ce484222325u64;
    let players = game.view().auth.players().count();
    for _ in 0..ROUNDS {
        for p in 0..players {
            let player = PlayerId(p as u8);
            // Commands are planned on the state before the first of them runs
            // (stale handles are rejected and not logged).
            for cmd in player_commands(&game, rules, player) {
                execute_logged(&mut game, rules, cmd, &mut log);
            }
        }
        game.validate(rules).expect("world must stay valid");
        rounds_hash = mix(rounds_hash, authoritative_hash(&game));
    }
    CoreGame {
        final_hash: authoritative_hash(&game),
        rounds_hash,
        log,
    }
}

fn replay(rules: &CompiledRuleset, log: &[Command]) -> u64 {
    let mut game = generate(&scenario(), rules);
    for cmd in log {
        game.execute(rules, cmd.clone())
            .expect("replayed command must apply");
    }
    authoritative_hash(&game)
}

#[test]
fn core_game_hash_baseline() {
    let rules = compile_rules(core_rules()).unwrap();
    let run = play_core_game(&rules);
    let replayed = replay(&rules, &run.log);
    eprintln!(
        "core_game: final={:#018x} rounds={:#018x} commands={} replay={:#018x}",
        run.final_hash,
        run.rounds_hash,
        run.log.len(),
        replayed
    );
    assert_eq!(
        run.final_hash, replayed,
        "replay diverged from the live game"
    );
    assert_eq!(
        run.final_hash, CORE_GAME_FINAL_HASH,
        "core game final hash changed"
    );
    assert_eq!(
        run.rounds_hash, CORE_GAME_ROUNDS_HASH,
        "core game round hashes changed"
    );
}

/// The baseline game must actually exercise the mechanics it protects.
#[test]
fn core_game_covers_buildings_and_research() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = generate(&scenario(), &rules);
    let mut log = Vec::new();
    let players = game.view().auth.players().count();
    for _ in 0..ROUNDS {
        for p in 0..players {
            let player = PlayerId(p as u8);
            for cmd in player_commands(&game, &rules, player) {
                execute_logged(&mut game, &rules, cmd, &mut log);
            }
        }
    }
    let auth = game.view().auth;
    let mut built = Vec::new();
    for (_, core) in auth.cities().iter() {
        built.extend(core.buildings.iter().copied());
    }
    let techs: usize = (0..players)
        .map(|p| auth.players().techs_of(PlayerId(p as u8)).len())
        .sum();
    let has = |key: &str| built.contains(&building_id(&rules, key));
    let mut keys: Vec<&str> = built
        .iter()
        .map(|b| rules.buildings[b.0 as usize].key.as_str())
        .collect();
    keys.sort();
    keys.dedup();
    eprintln!(
        "core_game: buildings={} techs={} kinds={:?}",
        built.len(),
        techs,
        keys
    );
    assert!(has("core:building/monument"), "monument must be built");
    assert!(has("core:building/granary"), "granary must be built");
    assert!(has("core:building/pyramids"), "a wonder must be built");
    assert!(
        has("core:building/great_library"),
        "a second wonder must be built"
    );
    assert!(techs >= 6, "research must progress");
}
