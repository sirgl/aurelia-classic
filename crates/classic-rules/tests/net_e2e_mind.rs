//! The net player (`Variant::Net`, `ai::net_e2e::mind`,
//! classic/docs/ai-neural-architecture.md, Playing) with the deterministic
//! stand-in scorer (`LinearScorer`): a game plays on without its orders being
//! refused, every candidate it may choose is an order the rules accept, and
//! the same seed gives the same world.
//!
//! The real weights: `AURELIA_NET_WEIGHTS=<weights.bin> cargo +1.98.1 test
//! --release -p classic-rules --test net_e2e_mind -- --ignored --nocapture`
//! plays the same game with them and prints the speed.

use std::time::Instant;

use classic_content::core::core_rules;
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::net_e2e::encode;
use classic_rules::ai::net_e2e::mind::{layout_text, NetPlayer, NetStats};
use classic_rules::ai::variant::{Variant, NET_BASE};
use classic_rules::ai::AiSettings;
use classic_rules::classic::Classic;
use classic_rules::knowledge::actions::acting_unit;
use classic_rules::replay::authoritative_hash;
use classic_rules::scenario::{generate_new_game, ScenarioConfig};
use classic_rules::{compile_rules, ClassicGame, Command, CompiledRuleset, GameState};
use kernel::ai::driver::AiDriver;
use kernel::ai::mind::AiMind;
use kernel::ai::trace::{Outcome, Reason};
use kernel::ai::StepOutcome;

type Driver = AiDriver<Classic, ClassicMind>;

/// Four nations on a small map, each with a city to start.
fn scenario(seed: u64) -> ScenarioConfig {
    ScenarioConfig { seed, players: 4, width: 32, height: 24, cities: 1, units: 2, water_ratio: 10 }
}

fn round(game: &GameState) -> u32 {
    game.view().auth.turn().round
}

fn driver(rules: &CompiledRuleset, mind: ClassicMind) -> Driver {
    let settings = AiSettings::for_rules(rules, rules.default_difficulty());
    let mut driver = AiDriver::with_mind(settings, Vec::new(), mind);
    driver.set_trace_capacity(1_000_000);
    driver
}

fn net_mind() -> ClassicMind {
    ClassicMind::with_net(Variant::Net, NetPlayer::dummy())
}

/// A command of a kind the net decides (unit commands, research, production,
/// policies).
fn nets_kind(command: &Command) -> bool {
    acting_unit(command).is_some()
        || matches!(
            command,
            Command::ChooseTechnology { .. } | Command::EnqueueProduction { .. } | Command::ClearProduction { .. } | Command::AdoptPolicy { .. } | Command::UnlockPolicyBranch { .. } | Command::ChooseIdeology { .. }
        )
}

/// What the driver's trace says of the net's orders: sent and applied,
/// withheld by the last look (`Reason::Forbidden`: the rules would refuse
/// it), rejected by the game.
#[derive(Debug, Default)]
struct Refusals {
    applied: usize,
    withheld: usize,
    rejected: usize,
}

fn refusals(driver: &Driver) -> Refusals {
    let mut out = Refusals::default();
    for entry in driver.trace().entries() {
        if !entry.command.as_ref().is_some_and(nets_kind) {
            continue;
        }
        match (&entry.outcome, entry.reason) {
            (Outcome::Applied, _) => out.applied += 1,
            (Outcome::Rejected(_), _) => out.rejected += 1,
            (_, Reason::Forbidden) => out.withheld += 1,
            _ => {}
        }
    }
    out
}

fn stats(driver: &mut Driver) -> NetStats {
    driver.mind_mut().net().expect("the net's mind").stats()
}

/// Plays `rounds` rounds; returns the world hash after each.
fn play(rules: &CompiledRuleset, game: &mut GameState, driver: &mut Driver, rounds: u32) -> Vec<u64> {
    let mut hashes = Vec::new();
    while round(game) < rounds && !game.is_over() {
        driver.run(game, rules, u32::MAX);
        hashes.push(authoritative_hash(game));
    }
    hashes
}

/// A game of four nets runs to round 30: the net decides every head, and
/// the rules refuse almost none of its orders (the last look withholds a
/// refused one; the game itself refuses none).
#[test]
fn a_game_of_nets_runs_to_round_30_without_refusals() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = generate_new_game(&rules, &scenario(0x0_9E7A));
    let mut driver = driver(&rules, net_mind());
    play(&rules, &mut game, &mut driver, 30);
    assert!(round(&game) >= 30, "the game reached round 30");
    let refused = refusals(&driver);
    let stats = stats(&mut driver);
    eprintln!("refusals {refused:?}\nnet {stats:?}\ncounters {:?}", driver.counters());
    assert_eq!(refused.rejected, 0, "the game refused a net order");
    assert!(refused.applied > 500, "{refused:?}");
    assert!(refused.withheld * 100 <= refused.applied, "withheld net orders pile up: {refused:?}");
    assert!(stats.decisions.iter().take(3).all(|n| *n > 0), "unit, research and production decided by the net: {stats:?}");
    assert_eq!(stats.memory_resets, 0, "every saved memory fits");
}

/// Every candidate of every unit, research, production and policy decision
/// of the first player of each sampled turn is an order the rules accept
/// (production: or refuse only as queued already, which the net never
/// sends); a long march the rules refuse is passed over at play.
#[test]
fn every_candidate_converts_to_an_order_the_rules_accept() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = generate_new_game(&rules, &scenario(0x0_5EED));
    let mut driver = driver(&rules, net_mind());
    let settings = AiSettings::for_rules(&rules, rules.default_difficulty());
    let probe = ClassicMind::new(NET_BASE);
    let (mut checked, mut marches, mut refused_marches) = (0usize, 0usize, 0usize);
    let mut refused: Vec<String> = Vec::new();
    let mut sampled = 0;
    while round(&game) < 25 {
        let player = game.view().auth.active_player();
        if round(&game) % 4 == 1 && game.view().auth.players().is_major(player) {
            sampled += 1;
            let sight = game.ai_observation(&rules, player);
            let mut turn = encode::Turn::encode(&sight, &rules);
            let mut all: Vec<encode::Candidate> = encode::research_candidates(&sight, &rules);
            all.extend(encode::policy_candidates(&sight, &rules));
            for city in &sight.own_cities {
                let production = encode::production_candidates(&mut turn, &sight, city, &rules);
                all.extend(production.into_iter().filter(|c| c.meta[0] != encode::tag::PRODUCE_UNIT || !city.queue.iter().any(|q| matches!(c.command, Some(Command::EnqueueProduction { item, .. }) if item == *q))));
            }
            for unit in &sight.own_units {
                let Some(near) = probe.sense_unit(&game, &rules, &settings, player, unit.handle) else { continue };
                all.extend(encode::unit_candidates(&mut turn, &near, &rules));
            }
            for c in &all {
                let Some(command) = &c.command else { continue };
                if matches!(command, Command::EnqueueProduction { city, item, .. } if sight.own_cities.iter().any(|o| o.handle == *city && o.queue.contains(item))) {
                    continue;
                }
                checked += 1;
                let accepted = game.accepts_order(&rules, player, command);
                if c.meta[0] == encode::tag::MARCH {
                    marches += 1;
                    refused_marches += usize::from(!accepted);
                } else if !accepted {
                    refused.push(format!("round {} player {}: {command:?}", round(&game), player.0));
                }
            }
        }
        // One player's turn.
        loop {
            match driver.step(&mut game, &rules) {
                StepOutcome::PlayerDone(_) | StepOutcome::RoundDone => break,
                StepOutcome::Waiting => break,
                StepOutcome::Worked => {}
            }
        }
    }
    eprintln!("sampled {sampled} turns, {checked} candidates, {marches} marches ({refused_marches} refused now), {} others refused", refused.len());
    assert!(sampled >= 8 && checked > 200, "a sample: {sampled} turns, {checked} candidates");
    assert!(refused.is_empty(), "candidates the rules refuse: {refused:#?}");
}

/// The same seed gives the same game: the world hash after every round is
/// equal in two runs of four nets.
#[test]
fn a_game_of_nets_is_deterministic() {
    let rules = compile_rules(core_rules()).unwrap();
    let run = || {
        let mut game = generate_new_game(&rules, &scenario(0x0_0D1E));
        let mut driver = driver(&rules, net_mind());
        play(&rules, &mut game, &mut driver, 15)
    };
    let (first, second) = (run(), run());
    assert!(first.len() >= 14, "{} rounds", first.len());
    assert_eq!(first, second);
}

/// The canonical layout text the weights file's hash is taken of
/// (classic/docs/ai-net-model.md): a line per table, integer scales as
/// integers, a tenth as `0.1`.
#[test]
fn the_layout_text_has_a_line_per_table() {
    let rules = compile_rules(core_rules()).unwrap();
    let text = layout_text(&rules);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 5);
    assert!(lines[0].starts_with("tile:known/1;visible/1;"), "{}", lines[0]);
    assert!(lines[1].contains(";treasury/0.1;"), "{}", lines[1]);
    assert!(lines[2].contains(";moves_left/10;"), "{}", lines[2]);
    assert!(lines[3].starts_with("candidate:act_move/1;"), "{}", lines[3]);
    assert!(lines[4].starts_with("event:event_kind_id_plus_1/1;event_param_id_plus_1/1;age/1;"), "{}", lines[4]);
    assert!(lines[4].contains(";item_cost/0.1;"), "{}", lines[4]);
    assert!(text.ends_with('\n'));
}

/// The pinned weights (`tests/fixtures/net_mind/random-small.bin`: the
/// small preset, random weights, this build's layout; `make_weights.py`
/// beside it regenerates it).
fn pinned_weights() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/net_mind/random-small.bin")
}

fn hash_rows(hash: &mut u64, values: impl IntoIterator<Item = i64>) {
    for v in values {
        for b in v.to_le_bytes() {
            *hash ^= u64::from(b);
            *hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
        }
    }
}

/// GOLDEN ENCODING: the rows the net reads of a fixed state — two nations
/// of a generated map at its first round (two cities, three units each) —
/// hashed: tiles, global, entities and their meta, the candidates and
/// their meta of every head (research, policy, production per city, every
/// unit). A change of a feature's meaning, scale or order changes the hash
/// even when the names stay; re-pin only on purpose, with the exporter's
/// format (`encode::FORMAT`) and the trainer's data, and say so in
/// classic/docs/ai-neural-architecture.md (Playing).
///
/// Format 7 adds the events of a played game: the same map played by the
/// net's base for 40 rounds, each nation's tracker observing it at every
/// round's start, the event rows and meta of every observation.
///
/// Re-pin log:
/// - 31 October 2026, format 5 (religion head): first pin.
/// - 1 November 2026, format 6: columns appended (global: the empire's
///   demand signals, the game's speed and length; entity: a city's
///   religion; candidate: the item queued in other own cities, the target
///   claimed by another own unit's march); every format-5 column unchanged
///   (their hash alone is still the format-5 pin).
/// - 1 November 2026, format 7: the event rows (a table of their own,
///   `events.rs`) join the hash; the four tables of the fixed state are
///   unchanged (their hash alone is still the format-6 pin, checked here).
/// - 2 November 2026, format 8: four candidate columns appended (the blow's
///   outcome: `blow_kills`, `blow_captures`, the uncapped damage, the city's
///   floor), zero here (no war at the first round, no attack candidate);
///   the four tables over the format-7 columns (the candidate rows cut to
///   their first 145) are still the format-6 pin, checked here.
/// - 2 November 2026, format 10: fourteen candidate columns appended (the
///   strike from a move's destination this turn and the next, the nearest
///   hostile units: `encode::MoveStrike`), zero here (no war); the four
///   tables over the format-9 columns (the rows cut to
///   `FORMAT_9_CANDIDATE_COLUMNS`) are still the format-8 pin, checked here.
#[test]
fn the_encoding_of_a_fixed_state_is_pinned() {
    const FORMAT_6: u64 = 0x783d_457b_2f27_62aa;
    const FORMAT_7_CANDIDATE_COLUMNS: usize = 145;
    const FORMAT_8: u64 = 0xd7af_4e39_2e8e_e6aa;
    const FOUR_TABLES: u64 = 0x4369_793b_b106_6baa;
    const PINNED: u64 = 0xfda3_375f_032c_1c7c;
    let rules = compile_rules(core_rules()).unwrap();
    let config = ScenarioConfig { seed: 0x0_601D, players: 2, width: 24, height: 18, cities: 2, units: 3, water_ratio: 10 };
    let game = generate_new_game(&rules, &config);
    let settings = AiSettings::for_rules(&rules, rules.default_difficulty());
    let probe = ClassicMind::new(NET_BASE);
    let mut hash = 0xCBF2_9CE4_8422_2325u64;
    let mut format_7 = hash;
    let mut format_9 = hash;
    let mut blocks = 0;
    for player in [classic_rules::ids::PlayerId(0), classic_rules::ids::PlayerId(1)] {
        let sight = game.ai_observation(&rules, player);
        let mut turn = encode::Turn::encode(&sight, &rules);
        let mut candidates = vec![encode::research_candidates(&sight, &rules), encode::policy_candidates(&sight, &rules)];
        for city in &sight.own_cities {
            candidates.push(encode::production_candidates(&mut turn, &sight, city, &rules));
        }
        for unit in &sight.own_units {
            let near = probe.sense_unit(&game, &rules, &settings, player, unit.handle).expect("an own unit");
            turn.own_unit_row(&near.unit, &rules);
            candidates.push(encode::unit_candidates(&mut turn, &near, &rules));
        }
        for (h, columns) in [(&mut hash, usize::MAX), (&mut format_7, FORMAT_7_CANDIDATE_COLUMNS), (&mut format_9, encode::FORMAT_9_CANDIDATE_COLUMNS)] {
            hash_rows(h, turn.tiles.iter().map(|v| i64::from(*v)));
            hash_rows(h, turn.global.iter().map(|v| i64::from(*v)));
            hash_rows(h, turn.entities.iter().map(|v| i64::from(*v)));
            hash_rows(h, turn.entity_meta.iter().flatten().map(|v| i64::from(*v)));
            for block in &candidates {
                hash_rows(h, [block.len() as i64]);
                for c in block {
                    hash_rows(h, c.row.iter().take(columns).map(|v| i64::from(*v)));
                    hash_rows(h, c.meta.iter().map(|v| i64::from(*v)));
                }
            }
        }
        blocks += candidates.len();
    }
    assert!(blocks >= 8, "{blocks} decisions");
    assert_eq!(format_7, FORMAT_6, "the four tables over the format-7 columns changed: {format_7:#018x}");
    assert_eq!(format_9, FORMAT_8, "the four tables over the format-9 columns changed: {format_9:#018x}");
    assert_eq!(hash, FOUR_TABLES, "the four tables changed: {hash:#018x}");
    // Format 7: the events of the same map played on.
    let mut game = game;
    let mut driver = driver(&rules, ClassicMind::new(NET_BASE));
    let mut trackers = [classic_rules::ai::net_e2e::events::Tracker::new(), classic_rules::ai::net_e2e::events::Tracker::new()];
    let mut kinds = std::collections::BTreeSet::new();
    let mut events = 0;
    for _ in 0..40 {
        for (p, tracker) in trackers.iter_mut().enumerate() {
            let sight = game.ai_observation(&rules, classic_rules::ids::PlayerId(p as u8));
            let turn = encode::Turn::encode_tracked(&sight, &rules, tracker);
            hash_rows(&mut hash, [turn.event_count() as i64]);
            hash_rows(&mut hash, turn.events.iter().map(|v| i64::from(*v)));
            hash_rows(&mut hash, turn.event_meta.iter().flatten().map(|v| i64::from(*v)));
            events += turn.event_count();
            kinds.extend(turn.event_meta.iter().map(|m| m[0]));
        }
        one_round(&rules, &mut game, &mut driver);
    }
    assert!(events >= 20 && kinds.len() >= 3, "{events} event rows of kinds {kinds:?}");
    assert_eq!(hash, PINNED, "the encoding changed: {hash:#018x} (format {})", encode::FORMAT);
}

/// PINNED GAME: four nets with the pinned weights play 20 rounds of the test
/// map; the world hash is pinned (as the bench pins `standard`'s smoke
/// game). A change of the encoder, the model's arithmetic, the net player
/// or the rules shows here; re-pin only on purpose, with a note in
/// classic/docs/ai-neural-architecture.md (Playing).
///
/// Re-pin log:
/// - 31 October 2026, format 5: first pin.
/// - 1 November 2026, the net's base `pre-war-fix` → `standard` (`NET_BASE`): not moved, on
///   purpose. Here the net decides every unit, research, production and policy, and what its
///   base still decides (diplomacy, citizens, purchases) plays alike in both bases: the same
///   world every round for 60 rounds, measured with `ClassicMind::with_net(Variant::PreWarFix, …)`.
///   The base itself is guarded by `a_net_without_heads_plays_standard`.
/// - 1 November 2026, the seventh promotion: `standard`, and so the net's base, besieges walled
///   cities (`ai::siege_walls`, its plans and press under the net's own unit decisions): not
///   moved; the nets decide every unit here and no siege group forms in the 20 rounds.
/// - 2 November 2026, the eighth promotion: `standard` puppets every captured city; the net's
///   base stays the standard before it, `pre-puppet` (`NET_BASE`), byte-identical: not moved.
/// - 2 November 2026, the ninth promotion: `standard` besieges as `siege-join`; the net's base,
///   `pre-puppet`, carries none of its switches (`Variant::siege_join`): not moved.
/// - 2 November 2026, the tenth promotion: `standard` fits its workers as `workers-fit`; the net's
///   base, `pre-puppet`, carries none of its switches (`Variant::workers_fit`): not moved.
/// - 2 November 2026, the eleventh promotion: `standard` plays as `army-camps`; the net's base,
///   `pre-puppet`, carries neither switch (`Variant::army_fit`, `Variant::camps`): not moved.
/// - 1 November 2026, format 7 (the events): not moved, on purpose. `random-small.bin` was
///   regenerated in the format-7 layout with the events on (weights version 4) from the same
///   format-5 random net, its event layers random and `xattn.out` zero, as `train_imitation
///   --init` carries a net without events: the nets read their events (`NetStats::events`) and
///   play the game they played without them.
#[test]
fn a_game_of_nets_with_pinned_weights_is_pinned() {
    const PINNED: u64 = 0x5905_0a2a_eb7b_5657;
    let rules = compile_rules(core_rules()).unwrap();
    let path = pinned_weights();
    let net = NetPlayer::from_path(path.to_str().unwrap()).expect("the pinned weights");
    let mut game = generate_new_game(&rules, &scenario(0x0_9E7A));
    let mut driver = driver(&rules, ClassicMind::with_net(Variant::Net, net));
    play(&rules, &mut game, &mut driver, 20);
    let stats = stats(&mut driver);
    assert!(stats.decisions[0] > 100, "{stats:?}");
    assert!(stats.events >= 5, "the nets read their events: {stats:?}");
    assert_eq!(refusals(&driver).rejected, 0);
    let hash = authoritative_hash(&game);
    assert_eq!(hash, PINNED, "the game of nets changed: {hash:#018x}");
}

/// Format 10: the pinned weights carried into the format-10 layout
/// (`random-small-f10.bin`: `python -m aurelia_nn.move_strike random-small.bin
/// random-small-f10.bin --layout <a format-10 shard>`, zero weights on the
/// move-strike columns) play the pinned game of the format-9 file, which this
/// build plays as its prefix (`mind::layout_format`).
#[test]
fn the_pinned_weights_carried_into_format_10_play_the_pinned_game() {
    const PINNED: u64 = 0x5905_0a2a_eb7b_5657;
    let rules = compile_rules(core_rules()).unwrap();
    let path = pinned_weights().with_file_name("random-small-f10.bin");
    let net = NetPlayer::from_path(path.to_str().unwrap()).expect("the carried weights");
    let mut game = generate_new_game(&rules, &scenario(0x0_9E7A));
    let mut driver = driver(&rules, ClassicMind::with_net(Variant::Net, net));
    play(&rules, &mut game, &mut driver, 20);
    assert!(stats(&mut driver).decisions[0] > 100);
    assert_eq!(authoritative_hash(&game), PINNED, "the carried net's game");
}

/// The pinned weights with the defer choice on (format 9) at `bias` (Q8).
fn deferring_net(bias: i32) -> ClassicMind {
    use classic_rules::ai::net_e2e::mind::ModelScorer;
    use classic_rules::ai::net_e2e::model::{Model, MAX_DEFER_BIAS};
    let bytes = std::fs::read(pinned_weights()).expect("the pinned weights");
    let model = Model::from_bytes(&bytes).expect("the pinned weights").with_defer(Some(bias.clamp(-MAX_DEFER_BIAS, MAX_DEFER_BIAS)));
    let scorer = ModelScorer::new(std::sync::Arc::new(model), "random-small+defer");
    ClassicMind::with_net(Variant::Net, NetPlayer::new(std::sync::Arc::new(scorer)))
}

/// DEFER (format 9): weights that do not know the defer choice
/// (`random-small.bin`, version 4) never offer it — the pinned game above is
/// theirs. The same net with the choice on and a bias that keeps the defer
/// candidate below every other offers it on every unit decision and never
/// takes it: the same pinned game.
#[test]
fn a_net_that_never_defers_plays_the_pinned_game() {
    const PINNED: u64 = 0x5905_0a2a_eb7b_5657;
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = generate_new_game(&rules, &scenario(0x0_9E7A));
    let mut driver = driver(&rules, deferring_net(i32::MIN));
    play(&rules, &mut game, &mut driver, 20);
    let stats = stats(&mut driver);
    assert!(stats.defer_offered > 100 && stats.defers == 0, "{stats:?}");
    assert_eq!(authoritative_hash(&game), PINNED, "the game of nets changed");
}

/// A net whose bias puts the defer candidate above every other takes it at
/// each unit's first decision of a turn: the driver moves the unit to the
/// end of the queue and asks again without it (one deferral a turn), so
/// every unit decides on the same world in the same order as without the
/// choice — the pinned game again, with a deferral per unit and turn.
#[test]
fn a_net_that_defers_every_unit_once_plays_the_pinned_game() {
    const PINNED: u64 = 0x5905_0a2a_eb7b_5657;
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = generate_new_game(&rules, &scenario(0x0_9E7A));
    let mut driver = driver(&rules, deferring_net(i32::MAX));
    play(&rules, &mut game, &mut driver, 20);
    let stats = stats(&mut driver);
    let deferred = driver.trace().entries().filter(|e| e.reason == Reason::Deferred).count() as u64;
    eprintln!("net {stats:?}");
    assert!(stats.defers > 100 && stats.defers == stats.defer_offered, "every offer taken: {stats:?}");
    assert_eq!(deferred, stats.defers, "each deferral traced");
    assert_eq!(refusals(&driver).rejected, 0);
    assert_eq!(authoritative_hash(&game), PINNED, "the game of nets changed");
}

fn pinned_net() -> ClassicMind {
    ClassicMind::with_net(Variant::Net, NetPlayer::from_path(pinned_weights().to_str().unwrap()).expect("the pinned weights"))
}

/// The game and its AI memory through the save's bytes, and a driver with
/// a new mind that loads the memory (as a loaded game, and as the game
/// window's AI worker every round).
fn reload(rules: &CompiledRuleset, game: &GameState, driver: &Driver, mind: ClassicMind) -> (GameState, Driver) {
    use classic_rules::persistence::{decode::decode, encode::encode};
    let mut save = encode(game, rules);
    save.ai = driver.save();
    let read = classic_rules::save_codec::from_bytes(&classic_rules::save_codec::to_bytes(&save)).unwrap();
    let loaded = decode(&read, rules).unwrap();
    let mut restored = self::driver(rules, mind);
    restored.load(&read.ai, loaded.view().auth.map().tile_count(), rules).unwrap();
    (loaded, restored)
}

/// Plays one round (from the start of a round to the next).
fn one_round(rules: &CompiledRuleset, game: &mut GameState, driver: &mut Driver) {
    let start = round(game);
    while round(game) == start && !game.is_over() {
        driver.run(game, rules, u32::MAX);
    }
}

/// SAVE AND LOAD: four nets with the pinned weights saved mid-game load
/// with the net's memory (the AI state's `mind_memory`) and play on as the
/// game that was not saved; the same load without the memory plays another
/// game (the memory decides).
#[test]
fn the_nets_memory_survives_save_and_load() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = generate_new_game(&rules, &scenario(0x0_9E7A));
    let mut live = driver(&rules, pinned_net());
    play(&rules, &mut game, &mut live, 8);
    let sizes: Vec<usize> = live.states().states().iter().map(|s| s.mind_memory.len()).collect();
    assert!(sizes.len() == 4 && sizes.iter().all(|n| *n > 256), "every nation has its memory: {sizes:?}");
    let (mut loaded, mut restored) = reload(&rules, &game, &live, pinned_net());
    assert_eq!(restored.states(), live.states());
    let (mut forgetful_game, mut forgetful) = reload(&rules, &game, &live, pinned_net());
    for p in 0..4 {
        forgetful.state_mut(classic_rules::ids::PlayerId(p), round(&game)).mind_memory.clear();
    }
    let mut diverged = false;
    for _ in 0..8 {
        one_round(&rules, &mut game, &mut live);
        one_round(&rules, &mut loaded, &mut restored);
        one_round(&rules, &mut forgetful_game, &mut forgetful);
        assert_eq!(authoritative_hash(&loaded), authoritative_hash(&game), "round {}", round(&game));
        diverged |= authoritative_hash(&forgetful_game) != authoritative_hash(&game);
    }
    assert_eq!(restored.states(), live.states());
    assert!(diverged, "a game that forgot the net's memory plays the same: the memory decides nothing");
    assert_eq!(stats(&mut restored).memory_resets, 0);
}

/// THE GAME WINDOW'S WORKER: the game and the AI memory go through the
/// save's bytes and a new mind every round; the game is the one a single
/// mind plays through the whole match.
#[test]
fn a_mind_rebuilt_every_round_plays_the_same_game() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = generate_new_game(&rules, &scenario(0x0_9E7A));
    let mut one = driver(&rules, pinned_net());
    // The game the window starts is set up for its AI players already (a
    // save of a game nobody set up loads as another game, whatever the AI).
    one.configure_game(&mut game);
    let mut rebuilt_game = game.clone();
    let mut rebuilt = driver(&rules, pinned_net());
    for _ in 0..12 {
        one_round(&rules, &mut game, &mut one);
        let (mut next_game, mut next) = reload(&rules, &rebuilt_game, &rebuilt, pinned_net());
        one_round(&rules, &mut next_game, &mut next);
        (rebuilt_game, rebuilt) = (next_game, next);
        assert_eq!(authoritative_hash(&rebuilt_game), authoritative_hash(&game), "round {}", round(&game));
    }
    assert_eq!(rebuilt.states(), one.states());
}

/// The saved memory round-trips through its bytes and is refused (started
/// afresh) for another scorer or damage.
#[test]
fn the_memory_bytes_round_trip() {
    use classic_rules::ai::net_e2e::mind::{decode_memory, encode_memory, LinearScorer, ModelScorer, Scorer};
    use classic_rules::ai::net_e2e::model::Model;
    let model = Model::from_bytes(&std::fs::read(pinned_weights()).unwrap()).unwrap();
    let scorer = ModelScorer::new(std::sync::Arc::new(model), "pinned");
    let [wp, wn, wu] = scorer.memory_widths();
    let mut memory = scorer.memory();
    memory.player = (0..wp as i16).collect();
    memory.nation_ids = vec![2, 5];
    memory.nations = (0..2 * wn as i16).map(|v| -v).collect();
    memory.unit_ids = vec![7, 9, 40];
    memory.units = (0..3 * wu as i16).collect();
    // The player's event tracker (format 7): observed twice on a game, so it
    // holds a snapshot.
    let rules = compile_rules(core_rules()).unwrap();
    let game = generate_new_game(&rules, &scenario(0x0_9E7A));
    let mut tracker = classic_rules::ai::net_e2e::events::Tracker::new();
    let sight = game.ai_observation(&rules, classic_rules::ids::PlayerId(0));
    tracker.observe(&sight, &rules);
    tracker.observe(&sight, &rules);
    assert!(tracker.to_bytes().len() > 40);
    let bytes = encode_memory(&memory, &tracker, &scorer);
    assert_eq!(decode_memory(&bytes, &scorer), (Some(memory.clone()), tracker.clone()));
    assert_eq!(encode_memory(&decode_memory(&bytes, &scorer).0.unwrap(), &tracker, &scorer), bytes, "deterministic bytes");
    assert_eq!(decode_memory(&bytes, &LinearScorer), (None, tracker.clone()), "another scorer starts its slots afresh, the events stay");
    assert_eq!(decode_memory(&bytes[..bytes.len() - 1], &scorer).0, None, "truncated");
    assert_eq!(decode_memory(&[], &scorer), (None, Default::default()));
    // A format-1 memory (before the events) loads its slots, with no events.
    let mut v1 = bytes.clone();
    let events = u32::from_le_bytes(v1[8..12].try_into().unwrap()) as usize;
    v1.drain(8..12 + events);
    v1[4..8].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(decode_memory(&v1, &scorer), (Some(memory.clone()), Default::default()));
}

/// A weights file of another feature layout is refused before the net
/// plays a turn, naming both hashes.
#[test]
fn a_weights_file_of_another_layout_is_refused() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut bytes = std::fs::read(pinned_weights()).unwrap();
    // The layout hash follows the magic, the version and the config.
    let count = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    let at = 12 + 4 * count;
    bytes[at] ^= 0xFF;
    let model = classic_rules::ai::net_e2e::model::Model::from_bytes(&bytes).unwrap();
    let scorer = classic_rules::ai::net_e2e::mind::ModelScorer::new(std::sync::Arc::new(model), "changed");
    let mind = ClassicMind::with_net(Variant::Net, NetPlayer::new(std::sync::Arc::new(scorer)));
    let mut game = generate_new_game(&rules, &scenario(0x0_9E7A));
    let mut driver = driver(&rules, mind);
    let refused = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| driver.run(&mut game, &rules, u32::MAX)));
    let message = refused.expect_err("the net does not play").downcast::<String>().map(|m| *m).unwrap_or_default();
    assert!(message.contains("layout hash") && message.contains(&format!("{:#018x}", classic_rules::ai::net_e2e::mind::layout_hash(&rules))), "{message}");
    assert!(message.contains(&format!("encoder format {}", encode::FORMAT)), "{message}");
}

/// The measured map: `NET_SPEED_MAP=<width>x<height>x<players>` (default
/// the test map, 32×24 with four nations), `NET_SPEED_ROUNDS` (60).
fn speed_scenario() -> (ScenarioConfig, u32) {
    let mut config = scenario(0x0_9E7A);
    if let Ok(map) = std::env::var("NET_SPEED_MAP") {
        let parts: Vec<u32> = map.split('x').map(|p| p.parse().expect("NET_SPEED_MAP=<w>x<h>x<players>")).collect();
        (config.width, config.height, config.players) = (parts[0], parts[1], parts[2] as u8);
    }
    let rounds = std::env::var("NET_SPEED_ROUNDS").ok().and_then(|r| r.parse().ok()).unwrap_or(60);
    (config, rounds)
}

/// Rounds a second of a game of `mind` on every seat, and the refusals
/// and the net's counts.
fn speed(rules: &CompiledRuleset, mind: impl Fn() -> ClassicMind) -> (f64, String) {
    let (config, rounds) = speed_scenario();
    let mut game = generate_new_game(rules, &config);
    let mut driver = driver(rules, mind());
    let started = Instant::now();
    play(rules, &mut game, &mut driver, rounds);
    let per_second = f64::from(round(&game).saturating_sub(1)) / started.elapsed().as_secs_f64();
    let net = driver.mind_mut().net().map(|n| format!("{:?}", n.stats())).unwrap_or_default();
    (per_second, format!("{:?} {net}", refusals(&driver)))
}

/// The speed of a game of nets against `standard`; with
/// `AURELIA_NET_WEIGHTS` also the real model.
#[test]
#[ignore = "a measurement"]
fn net_speed() {
    let rules = compile_rules(core_rules()).unwrap();
    eprintln!("map and rounds: {:?}", speed_scenario());
    let (standard, _) = speed(&rules, || ClassicMind::new(Variant::Standard));
    eprintln!("standard: {standard:.2} rounds/s");
    let (dummy, counts) = speed(&rules, net_mind);
    eprintln!("net (dummy scorer): {dummy:.2} rounds/s; {counts}");
    if std::env::var("AURELIA_NET_WEIGHTS").is_ok() {
        let (weights, counts) = speed(&rules, || ClassicMind::new(Variant::Net));
        eprintln!("net (weights): {weights:.2} rounds/s; {counts}");
    }
}


/// With no head of its own (`AURELIA_NET_HEADS=` empty) the net mind plays
/// exactly the scripted mind: the same world as `NET_BASE` (`pre-puppet`), its base, every round. With
/// a subset, the net decides only those heads.
///
/// 20 rounds (12 before the base moved to `standard`, 1 November 2026): on this map `standard`
/// and `pre-war-fix` play the same world for 15 rounds and part in the 16th, so the game is long
/// enough to tell the base from the old one, and the test checks that it does.
#[test]
fn a_net_without_heads_plays_standard() {
    use classic_rules::ai::net_e2e::mind::parse_heads;
    let rules = compile_rules(core_rules()).unwrap();
    let run = |mind: ClassicMind| {
        let mut game = generate_new_game(&rules, &scenario(0x0_9E7A));
        let mut driver = driver(&rules, mind);
        let hashes = play(&rules, &mut game, &mut driver, 20);
        let stats = driver.mind_mut().net().map(|n| n.stats());
        (hashes, stats)
    };
    let (standard, _) = run(ClassicMind::new(NET_BASE));
    let (headless, stats) = run(ClassicMind::with_net(Variant::Net, NetPlayer::dummy().with_heads(parse_heads("").unwrap())));
    assert_eq!(headless, standard);
    let (old_base, _) = run(ClassicMind::new(Variant::PreWarFix));
    assert_ne!(headless, old_base, "the game is long enough to tell `standard` from `pre-war-fix`");
    assert_eq!(stats.unwrap().decisions, [0; 4]);
    let (_, stats) = run(ClassicMind::with_net(Variant::Net, NetPlayer::dummy().with_heads(parse_heads("research,policy").unwrap())));
    let d = stats.unwrap().decisions;
    assert!(d[0] == 0 && d[2] == 0 && d[1] > 0, "{d:?}");
    assert!(parse_heads("unit,nonsense").is_err());
}
