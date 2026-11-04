//! Headless 100-round game on the classic ruleset (the game's default content):
//! a classic new game (nations, free techs, starting units, start biases)
//! driven by the greedy AI plus scripted production of classic buildings and
//! wonders. The world is validated every round, the command log is replayed
//! on a fresh world, and the hashes are pinned: a change of classic content
//! semantics or of the classic start shows up here, next to the frozen legacy
//! pins of baseline_hashes.rs.
//!
//! Run with `--nocapture` to print the hashes.

use classic_rules::ClassicGame;
use classic_content::core::core_rules;
use classic_rules::ai::{war_declarations, AiState, GreedyPlanner, Planner};
use classic_rules::ids::{BuildingKindId, CityHandle, PlayerId, ProductionItem, UnitKindId};
use classic_rules::replay::authoritative_hash;
use classic_rules::scenario::{generate_new_game, ScenarioConfig};
use classic_rules::{compile_rules, Command, CompiledRuleset, GameState};

// Re-pin log (one line per deliberate change, as in baseline_hashes.rs):
// - first classic import: final 9a07f112a0a626fe / rounds 9cca58265af470a9.
// - merge of M3 (rules core) into the classic content: the authoritative hash now
//   mixes the global round, the active player, entity ids and the id counter,
//   the owning city of owned tiles, unit move goals, tile memory and contacts
//   (see baseline_hashes.rs re-pin log); hex topology, sequential turns and
//   the movement rules (feature costs from the classic features table) apply. The game
//   itself plays the same: 758 accepted commands, 6 cities, 44 techs and the
//   same classic buildings as before the merge.
// - M1c classic-like land (scenario/landscape.rs): climate bands with tundra,
//   ridges, downhill rivers, all classic features, natural wonder sites (hashed);
//   terrain flags from data (hill and mountain are land, a mountain is
//   impassable); starts pick the best land among the far-enough tiles
//   instead of the farthest tile (which was polar on these maps). 746
//   accepted commands, 6 cities, 49 techs, Pyramids, Stonehenge, Temple of
//   Artemis built. With the world-art merge the sites live in main's
//   per-tile natural wonder layer and hash there (same game, same commands).
// - M1c sight over water (water does not block line of sight, mountains do):
//   same 746 commands, visibility and tile memory change.
// - M4 culture and policies (track B): city culture is classic (buildings and
//   tiles only, one border level per turn), every first city gets the
//   Palace, culture fills the player's pool and the AI adopts policies
//   (`ai::policies::policy_command`): 843 accepted commands, Tradition
//   completed (its finisher's Aqueducts appear).
// - M4 faith and religion (track B): city faith fills the player's pool
//   (Faith step), the AI founds pantheons and acts on religion
//   (`ai::religion::religion_commands`), pantheon beliefs reach cities,
//   religious pressure runs each round: 871 accepted commands, 5 pantheons,
//   no religion yet in 100 rounds.
// - merge of M1c land and sight (main) with M4 culture, policies, faith and
//   religion (track B): both apply; the pins below are the merged game (837
//   commands, 30 policies, 6 pantheons, 1 religion).
// - M4 tourism (track B): the world hash mixes the tourism state (great
//   works, influence, the lifetime culture of each player's last turn,
//   public opinion); the Tourism step runs after Culture. Same game: 837
//   commands, 30 policies, 6 pantheons, 1 religion.
// - M4 wonders (track B): classic wonders need their policy branch (the
//   Pyramids need Liberty, which this game's Tradition player never opens):
//   the scripted Pyramids are refused, the city builds on; 849 commands,
//   no Pyramids, 30 policies, 6 pantheons, 1 religion.
// - M4-A tile yields: cities work 3 rings with classic tile yields (terrain,
//   features, revealed resources, improvements, fresh water, city centre
//   minimum) per channel — no v1 combined output, no culture per citizen,
//   gold from tiles only; citizens go to tiles, specialists or unemployed
//   by focus; the hash mixes the economy section; natural wonder tiles yield
//   their yields (merged with M1c land). 746 -> 706 accepted commands,
//   6 cities, 62 techs.
// - M4-A growth and great people: classic growth thresholds and food store in
//   hundredths, starvation, kept food, settlers from size 2 with the food
//   surplus as production, great person points and births, specialist
//   yields from buildings, the golden age countdown. 706 -> 697 accepted
//   commands, 6 cities, 55 techs.
// - M4-A after the M6 AI merge: the first city founded gets the palace
//   (free capital building: +3 production, gold, science, +1
//   culture); the economy advisor (great people, city focus) plugs into
//   the AI registry; AI workers use the core's site rule. 697 -> 755
//   accepted commands, 6 cities, 81 techs, Pyramids and Stonehenge built.
// - M4-A happiness: default-difficulty happiness and unhappiness (cities, citizens,
//   luxuries, buildings, natural wonders), unhappy growth, production,
//   gold and combat penalties, the golden age meter; the economy section
//   hashes the snapshot and settings. 755 -> 754 accepted commands, 80
//   techs.
// - M4-A gold: classic maintenance of units, buildings and roads, city
//   connection gold, deficits (empty treasury, science loss, disbanding),
//   classic purchase and tile prices, building placement rules, processes.
//   754 -> 752 accepted commands, 78 techs.
// - merge of M4 track B (culture, policies, faith, religion, tourism,
//   wonders) with M4-A (yields, growth, happiness, gold): both apply; the
//   Palace comes from track B's `place_palace` on both founding paths;
//   founder beliefs refresh their founder's city yields when a city
//   changes faith; great works journal their city. 779 accepted commands,
//   6 cities, 83 techs, 30 policies, 5 pantheons, Stonehenge and the
//   Temple of Artemis.
// - M4-A strategic resources: map deposits hold several copies (one of the
//   resource's quantities; the world hash mixes deposits of more than
//   one), units and buildings need their resources to be built. Same 779
//   commands.

// - M4 combat: classic combat formulas and city strength/HP; new units get their
//   free promotions (they were created without them); the first city of a
//   major nation gets the Palace (+3 science, gold, production — the
//   game now reaches 80 techs instead of 49); 796 accepted commands.
// - merge of M4 track B and track C: classic combat with track B's culture,
//   faith and tourism; 849 commands, 81 techs, 30 policies, 6 pantheons,
//   1 religion.
// - M4 espionage: the game reaches the Renaissance, every nation
//   gets its spy and the hash mixes the spies (they stay unassigned: the
//   scripted players do not use them). Same 849 commands.
// - merge of M4 track C (combat, diplomacy, barbarians, city-states,
//   congress, espionage, victories) with M4-A (yields, growth, happiness,
//   gold, resources): both apply; conquest status replaces M4-A's
//   annexation, the resource ledger counts deal imports and exports,
//   maritime city-state food reaches the classic yields. 779 commands.
// - typed natural wonder placement: the generator applied no
//   `core_tile_can_be_*` restriction (the catalog's `false` columns were
//   read as 1 from inert effects), so 8 wonders may now stand on fewer
//   tiles and this map's wonders moved. 781 commands.
// - production, purchase and maintenance effects: production percents
//   toward the item at the head of the queue (the game builds the Temple
//   of Artemis; its policies' production, purchase and maintenance effects
//   apply), the AI estimates turns with the item's own percents, a
//   great engineer hurries buildings only, worker jobs count hundredths
//   of worker-turns. 789 commands, 83 techs, 30 policies, 5 pantheons.
// - inert society effects typed (workstream E1): the scripted players'
//   policies now apply their grants and society stats (golden ages of
//   Representation, owed free great people and technologies kept in the
//   economy section, Meritocracy's happiness, policy effects of other
//   triggers than OnAdopt), luxuries follow deals in happiness. Same 779
//   commands.
// - religion (workstream D): cities keep their capital connection (the
//   classic ruleset reads it: Messenger of the Gods) and the hash mixes it;
//   the game itself is unchanged (same 779 commands; without that mix the
//   old hashes come back).
// - cities keep the round they were founded or acquired (hashed; same
//   commands, the log in tests/baseline_hashes.rs).
// - E2 tile conditions: player 0 plays the Celts, whose Druidic Lore is now
//   typed (+1 faith in a city with an unimproved forest next to its centre,
//   +2 with three): more faith from round 1. 786 commands, 81 techs,
//   30 policies, 3 pantheons, 1 religion.
// - Sight (city 3 rings, owned tiles, unit sight and promotions, hills,
//   forest and jungle hide; see the re-pin log of tests/baseline_hashes.rs).
//   Same 786 commands, 81 techs, 30 policies, 3 pantheons, 1 religion.

// - City names from the nation's list (see the re-pin log of
//   tests/baseline_hashes.rs). Same 786 commands.

// - classic cost formulas (see the re-pin log of tests/baseline_hashes.rs). 784 commands.
// - 18 October, pacing AI (one queued order, no gold focus on a full treasury,
//   a shrine after the second city): the final world is the same, the rounds'
//   hashes differ: 0xd7c843401139c2d5 -> 0x268c2c883fa35ea7.
// - 19 October, the pantheon by the score of its effects on the land and the
//   situation (`ai::pantheon`) instead of the fixed preference list, and a
//   prophet builds its Holy Site only where the terrain suits it: the AI founds
//   other pantheons, so the final world differs:
//   0xf1536870200b6919 -> 0xe59a7e40b095d7ee (rounds 0x83322371227e2048 ->
//   0x0281bab5ba280e5d).
// - 20 October, resources at sea (fish, whales, pearls, crab and sea oil laid
//   by the map generator, a fish for a coastal start; oil and uranium counts
//   balanced on land): the map of the game has other deposits, so the game
//   differs: 0xe59a7e40b095d7ee -> 0xc634056b89b0656e (rounds
//   0x0281bab5ba280e5d -> 0x9e7bdac3e5693d86).

// - 21 October, the rule data re-sourced from Unciv (see the re-pin log of
//   tests/baseline_hashes.rs): 0xc634056b89b0656e -> 0xfddd23c9bab27d14 (rounds
//   0x9e7bdac3e5693d86 -> 0x90d8bd975657d687), 784 commands.

/// Hash of the final state of the classic game.
const CLASSIC_GAME_FINAL_HASH: u64 = 0x1f920735c9735b24;
/// Fold of the per-round hashes of the classic game.
const CLASSIC_GAME_ROUNDS_HASH: u64 = 0x7d888bdc2e8e01d;

const ROUNDS: usize = 100;

/// classic buildings tried in this order when a city queue is empty.
const BUILD_ORDER: [&str; 10] = [
    "core:building/monument",
    "core:building/shrine",
    "core:building/granary",
    "core:building/pyramids",
    "core:building/library",
    "core:building/great_library",
    "core:building/temple_of_artemis",
    "core:building/barracks",
    "core:building/walls",
    "core:building/stonehenge",
];

fn scenario() -> ScenarioConfig {
    ScenarioConfig {
        seed: 0x5EED_0B5E,
        players: 6,
        width: 48,
        height: 40,
        cities: 0,
        units: 0,
        water_ratio: 20,
    }
}

fn building_id(rules: &CompiledRuleset, key: &str) -> BuildingKindId {
    let index = rules.buildings.iter().position(|b| b.key == key);
    BuildingKindId(index.unwrap_or_else(|| panic!("{key} must exist")) as u16)
}

fn unit_id(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    let index = rules.units.iter().position(|u| u.key == key);
    UnitKindId(index.unwrap_or_else(|| panic!("{key} must exist")) as u16)
}

fn own_cities(game: &GameState, player: PlayerId) -> Vec<CityHandle> {
    let mut out = Vec::new();
    for (handle, core) in game.view().auth.cities().iter() {
        if core.owner == player {
            out.push(handle);
        }
    }
    out
}

/// For every city with an empty queue: every building of BUILD_ORDER
/// (validation keeps the allowed ones), then a warrior.
fn production_orders(game: &GameState, rules: &CompiledRuleset, player: PlayerId) -> Vec<Command> {
    let mut out = Vec::new();
    for city in own_cities(game, player) {
        let core = game.view().auth.cities().get(city).unwrap();
        if !core.queue.is_empty() {
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
    out.extend(classic_rules::ai::policies::policy_command(
        &game.ai_observation(rules, player),
        rules,
        &AiState::new(player, 0),
    ));
    out.extend(classic_rules::ai::religion::religion_commands(
        &game.ai_observation(rules, player),
        rules,
    ));
    out.push(Command::EndTurn { player });
    out
}

fn mix(acc: u64, value: u64) -> u64 {
    (acc ^ value).wrapping_mul(0x100000001b3)
}

struct ClassicRun {
    game: GameState,
    rounds_hash: u64,
    log: Vec<Command>,
}

fn play(rules: &CompiledRuleset) -> ClassicRun {
    let mut game = generate_new_game(rules, &scenario());
    let mut log = Vec::new();
    let mut rounds_hash = 0xcbf29ce484222325u64;
    let players = game.view().auth.players().count();
    for _ in 0..ROUNDS {
        for p in 0..players {
            let player = PlayerId(p as u8);
            for cmd in player_commands(&game, rules, player) {
                if game.execute(rules, cmd.clone()).is_ok() {
                    log.push(cmd);
                }
            }
        }
        game.validate(rules).expect("world must stay valid");
        rounds_hash = mix(rounds_hash, authoritative_hash(&game));
    }
    ClassicRun {
        game,
        rounds_hash,
        log,
    }
}

fn replay(rules: &CompiledRuleset, log: &[Command]) -> u64 {
    let mut game = generate_new_game(rules, &scenario());
    for cmd in log {
        game.execute(rules, cmd.clone())
            .expect("replayed command must apply");
    }
    authoritative_hash(&game)
}

#[test]
fn classic_game_runs_100_rounds_and_replays_to_the_pinned_hash() {
    let rules = compile_rules(core_rules()).unwrap();
    let run = play(&rules);
    let final_hash = authoritative_hash(&run.game);
    let replayed = replay(&rules, &run.log);
    let auth = run.game.view().auth;
    let players = auth.players().count();
    let techs: usize = (0..players)
        .map(|p| auth.players().techs_of(PlayerId(p as u8)).len())
        .sum();
    let mut built: Vec<&str> = Vec::new();
    for (_, core) in auth.cities().iter() {
        for b in &core.buildings {
            built.push(rules.buildings[b.0 as usize].key.as_str());
        }
    }
    built.sort();
    built.dedup();
    eprintln!(
        "classic_game: final={final_hash:#018x} rounds={:#018x} commands={} replay={replayed:#018x} cities={} techs={techs} buildings={built:?}",
        run.rounds_hash,
        run.log.len(),
        auth.cities().len(),
    );
    assert_eq!(final_hash, replayed, "replay diverged from the live game");
    assert!(auth.cities().len() >= players / 2, "players found cities");
    assert!(techs >= players * 6, "research progresses on BNW costs");
    assert!(
        built.contains(&"core:building/monument"),
        "a BNW building is built"
    );
    let policies: usize = (0..players)
        .map(|p| auth.module().culture().of(PlayerId(p as u8)).policies.len())
        .sum();
    let pantheons = auth.module().religion().pantheons();
    let religions = auth.module().religion().religions().len();
    eprintln!("classic_game: policies={policies} pantheons={pantheons} religions={religions}");
    assert!(policies >= players * 3, "the AI adopts policies with its culture");
    assert!(pantheons >= 1, "the AI founds pantheons with its faith");
    assert_eq!(
        final_hash, CLASSIC_GAME_FINAL_HASH,
        "BNW game final hash changed"
    );
    assert_eq!(
        run.rounds_hash, CLASSIC_GAME_ROUNDS_HASH,
        "BNW game round hashes changed"
    );
}

/// A new classic game starts every player with Agriculture known, a
/// settler (first, so the AI founds with it) and the nation's warrior.
#[test]
fn classic_new_game_starts_with_free_tech_settler_and_warrior() {
    let rules = compile_rules(core_rules()).unwrap();
    let game = generate_new_game(&rules, &scenario());
    let auth = game.view().auth;
    let agriculture = rules
        .technologies
        .iter()
        .position(|t| t.key == "core:tech/agriculture")
        .unwrap();
    for p in 0..auth.players().count() {
        let player = PlayerId(p as u8);
        let nation = auth.players().nation_of(player);
        let techs = auth.players().techs_of(player);
        assert!(techs.iter().any(|t| t.0 as usize == agriculture));
        let units = auth.indexes().units_of(player);
        assert_eq!(units.len(), 2, "settler and warrior");
        let kinds: Vec<UnitKindId> = units
            .iter()
            .map(|h| auth.units().get(*h).unwrap().core.kind)
            .collect();
        assert!(rules.units[kinds[0].0 as usize].founder, "founder first");
        let defense = rules.nation_variant(nation, "core:unit/warrior").unwrap();
        assert_eq!(kinds[1], defense, "the nation's warrior");
    }
}
