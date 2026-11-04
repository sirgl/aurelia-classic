//! Sampling play for E2 (`ai::net_e2e::sample`,
//! classic/docs/ai-neural-architecture.md, «The E2 recipe»): at temperature
//! 0 the sampling net plays the argmax game (the pinned game of
//! `net_e2e_mind.rs`, whose hash must not move); at temperature 1 the same
//! seeds play the same game, another sampling seed another one, and the
//! trace holds what policy-gradient training needs.

use classic_content::core::core_rules;
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::net_e2e::mind::NetPlayer;
use classic_rules::ai::net_e2e::sample::{Sampling, Trace};
use classic_rules::ai::variant::Variant;
use classic_rules::ai::AiSettings;
use classic_rules::classic::Classic;
use classic_rules::replay::authoritative_hash;
use classic_rules::scenario::{generate_new_game, ScenarioConfig};
use classic_rules::{compile_rules, CompiledRuleset};
use kernel::ai::driver::AiDriver;

/// The pinned game of `net_e2e_mind.rs` (`a_game_of_nets_with_pinned_weights_is_pinned`). Not
/// moved when the net's base became `standard` (1 November 2026): see its re-pin log.
const PINNED: u64 = 0x5905_0a2a_eb7b_5657;

fn scenario() -> ScenarioConfig {
    ScenarioConfig { seed: 0x0_9E7A, players: 4, width: 32, height: 24, cities: 1, units: 2, water_ratio: 10 }
}

/// Four nets of the pinned weights, sampling, to round 20: the world hash
/// and each player's trace (by the shared mind).
fn play(rules: &CompiledRuleset, sampling: Option<Sampling>) -> (u64, Option<Trace>) {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/net_mind/random-small.bin");
    let net = NetPlayer::from_path(path.to_str().unwrap()).expect("the pinned weights");
    if let Some(s) = sampling {
        net.set_sampling(s);
    }
    let mut game = generate_new_game(rules, &scenario());
    let settings = AiSettings::for_rules(rules, rules.default_difficulty());
    let mut driver = AiDriver::<Classic, ClassicMind>::with_mind(settings, Vec::new(), ClassicMind::with_net(Variant::Net, net));
    driver.set_trace_capacity(0);
    while game.view().auth.turn().round < 20 && !game.is_over() {
        let position = driver.counters().commands_applied as u32;
        driver.step(&mut game, rules);
        driver.mind_mut().net().unwrap().stamp_trace(position);
    }
    let trace = driver.mind_mut().net().unwrap().take_trace();
    (authoritative_hash(&game), trace)
}

#[test]
fn at_temperature_zero_the_sampling_net_plays_the_argmax_game() {
    let rules = compile_rules(core_rules()).unwrap();
    let (hash, trace) = play(&rules, Some(Sampling { temperature: 0.0, seed: 99 }));
    assert_eq!(hash, PINNED, "the argmax game: {hash:#018x}");
    let trace = trace.expect("a trace");
    assert!(trace.decisions() > 100, "{}", trace.decisions());
    assert!(trace.turns.iter().flat_map(|t| &t.decisions).all(|d| d.logp == 0.0));
    let (plain, none) = play(&rules, None);
    assert_eq!(plain, PINNED);
    assert!(none.is_none());
}

#[test]
fn a_sampled_game_is_a_function_of_its_seeds() {
    let rules = compile_rules(core_rules()).unwrap();
    let sampling = Sampling { temperature: 1.0, seed: 7 };
    let (first, trace) = play(&rules, Some(sampling));
    let (second, again) = play(&rules, Some(sampling));
    assert_eq!(first, second, "the same seeds, the same game");
    assert_eq!(trace, again, "and the same trace");
    assert_ne!(first, PINNED, "sampling played another game than the argmax");
    let (other, _) = play(&rules, Some(Sampling { temperature: 1.0, seed: 8 }));
    assert_ne!(first, other, "another sampling seed, another game");
    let trace = trace.expect("a trace");
    let decisions: Vec<_> = trace.turns.iter().flat_map(|t| &t.decisions).collect();
    assert!(decisions.len() > 100);
    let mut sampled = 0;
    for d in &decisions {
        assert_ne!(d.position, u32::MAX, "every record stamped");
        assert_eq!(usize::from(d.accepted) + d.refused.len(), usize::from(d.candidates));
        if let Some(c) = d.chosen {
            assert!(c < d.candidates && !d.refused.contains(&c));
            assert!(d.logp <= 0.0 && d.logp.is_finite(), "{d:?}");
            sampled += usize::from(d.logp < -1e-3);
        }
    }
    assert!(sampled * 10 > decisions.len(), "{sampled} of {} choices had another candidate in reach", decisions.len());
    assert!(trace.turns.windows(2).all(|w| w[0].position <= w[1].position));
    assert!(trace.turns.iter().all(|t| t.value.is_some()), "the model's value per turn");
    assert_eq!(Trace::decode(&trace.encode()).unwrap(), trace);
}
