//! The track-B system advisors in the AI driver (`ai::system` registry:
//! `ai::policies::PolicyAdvisor`, `ai::religion::ReligionAdvisor`): in a
//! headless classic game of AI nations, the AI adopts policies, founds
//! pantheons and a religion, and the core refuses almost none of their
//! proposals.

use classic_content::core::core_rules;
use classic_rules::classic::ClassicSystem;
use classic_rules::ai::trace::{Outcome, Reason};
use classic_rules::ai::{AiDriver, AiSettings};
use classic_rules::compile_rules;
use classic_rules::ids::PlayerId;
use classic_rules::scenario::{generate_new_game, ScenarioConfig};

const PLAYERS: u8 = 4;
/// 200: after the merge with QA (no default warriors) the first religion
/// of this seed comes at round 160–200 (was by 160).
const ROUNDS: u32 = 200;

/// Seeds of the headless games. Few AI nations reach a Great
/// Prophet in 160 rounds (2 games of 8 on this ruleset), so the religion
/// is asserted over the set, not over one game: a single seed made the
/// assertion hang on the butterfly effects of any rule change.
const SEEDS: [u64; 9] = [0x7_B0B, 1, 2, 3, 4, 5, 6, 7, 8];

#[test]
fn ai_adopts_policies_and_founds_a_religion_in_a_classic_game() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut religions = 0;
    for seed in SEEDS {
        religions += one_game(&rules, seed);
    }
    assert!(religions >= 1, "a religion is founded");
}

/// One game: every nation adopts policies, pantheons are founded,
/// the core refuses few advisor commands. Returns the religions founded.
fn one_game(rules: &classic_rules::CompiledRuleset, seed: u64) -> usize {
    let config = ScenarioConfig {
        seed,
        players: PLAYERS,
        width: 48,
        height: 32,
        cities: 0,
        units: 0,
        water_ratio: 25,
    };
    let mut game = generate_new_game(rules, &config);
    let mut driver = AiDriver::new(AiSettings::for_rules(rules, rules.default_difficulty()), Vec::new());
    driver.set_trace_capacity(50_000);
    let systems = driver.mind_mut().advisors_mut().systems();
    assert!(systems.contains(&ClassicSystem::Policies) && systems.contains(&ClassicSystem::Religion));
    while game.view().auth.turn().round < ROUNDS && game.winner().is_none() {
        driver.run(&mut game, rules, u32::MAX);
    }
    game.validate(rules).unwrap();
    let auth = game.view().auth;
    let policies: usize = (0..PLAYERS).map(|p| auth.module().culture().of(PlayerId(p)).policies.len()).sum();
    let pantheons = auth.module().religion().pantheons();
    let religions = auth.module().religion().religions().len();
    let (applied, rejected) = advisor_outcomes(&driver);
    eprintln!(
        "seed {seed}, {ROUNDS} rounds: policies {policies}, pantheons {pantheons}, religions {religions}, \
         advisor commands {applied} applied / {rejected} rejected"
    );
    assert!(policies >= 2 * PLAYERS as usize, "seed {seed}: every nation adopts policies");
    // A nation founding a religion without a pantheon picks the
    // pantheon belief with it: pantheons and religions together count
    // the nations that used their faith.
    assert!(pantheons >= 1 && usize::from(pantheons) + religions >= 2, "seed {seed}: pantheons are founded");
    assert!(rejected * 20 <= applied, "seed {seed}: the core refuses few advisor commands");
    religions
}

/// Applied and rejected commands the system advisors proposed.
fn advisor_outcomes(driver: &AiDriver) -> (usize, usize) {
    let mut applied = 0;
    let mut rejected = 0;
    for entry in driver.trace().entries() {
        if entry.reason != Reason::SystemAdvisor {
            continue;
        }
        match entry.outcome {
            Outcome::Applied => applied += 1,
            Outcome::Rejected(_) => {
                eprintln!("rejected: {entry}");
                rejected += 1
            }
            Outcome::NoCommand => {}
        }
    }
    (applied, rejected)
}

/// The culture advisor (`ai::tourism::TourismAdvisor`): a Great Writer next
/// to a city with a free literature slot creates its work in the AI turn.
#[test]
fn ai_puts_a_great_writer_into_a_free_slot() {
    let rules = compile_rules(core_rules()).unwrap();
    let config = ScenarioConfig {
        seed: 0x7_B0C,
        players: 2,
        width: 32,
        height: 24,
        cities: 0,
        units: 0,
        water_ratio: 20,
    };
    let mut game = generate_new_game(&rules, &config);
    let mut driver = AiDriver::new(
        AiSettings::for_rules(&rules, rules.default_difficulty()),
        Vec::new(),
    );
    while game.view().auth.indexes().cities_of(PlayerId(0)).is_empty() {
        driver.run(&mut game, &rules, u32::MAX);
    }
    let city = game.view().auth.indexes().cities_of(PlayerId(0))[0];
    let tile = game.view().auth.cities().get(city).unwrap().tile;
    let amphitheater = rules
        .buildings
        .iter()
        .position(|b| b.key == "core:building/amphitheater");
    game.debug_add_building(
        &rules,
        city,
        classic_rules::ids::BuildingKindId(amphitheater.unwrap() as u16),
    );
    let writer = rules
        .units
        .iter()
        .position(|u| u.key == "core:unit/great-writer")
        .unwrap();
    game.debug_spawn_unit(
        &rules,
        PlayerId(0),
        tile,
        classic_rules::ids::UnitKindId(writer as u16),
    );
    let round = game.view().auth.turn().round;
    while game.view().auth.turn().round < round + 2 {
        driver.run(&mut game, &rules, u32::MAX);
    }
    assert_eq!(game.view().auth.module().tourism().city(city).works.len(), 1);
    game.validate(&rules).unwrap();
}

/// The culture advisor digs: an archaeologist next to an open site digs it
/// and the AI takes what the dig offers (an artifact for the Palace slot).
#[test]
fn ai_digs_a_site_and_takes_its_find() {
    let rules = compile_rules(core_rules()).unwrap();
    let config = ScenarioConfig {
        seed: 0x7_B0D,
        players: 2,
        width: 32,
        height: 24,
        cities: 0,
        units: 0,
        water_ratio: 20,
    };
    let mut game = generate_new_game(&rules, &config);
    let mut driver = AiDriver::new(
        AiSettings::for_rules(&rules, rules.default_difficulty()),
        Vec::new(),
    );
    while game.view().auth.indexes().cities_of(PlayerId(0)).is_empty() {
        driver.run(&mut game, &rules, u32::MAX);
    }
    let archaeology = rules
        .technologies
        .iter()
        .position(|t| t.key == "core:tech/archaeology")
        .unwrap();
    game.debug_learn(
        &rules,
        PlayerId(0),
        classic_rules::ids::TechId(archaeology as u16),
    );
    while !game.view().auth.module().tourism().archaeology().triggered {
        driver.run(&mut game, &rules, u32::MAX);
    }
    let city = game.view().auth.indexes().cities_of(PlayerId(0))[0];
    let home = game.view().auth.cities().get(city).unwrap().tile;
    let dims = game.view().auth.map().dimensions;
    let site = game
        .view()
        .auth
        .module()
        .tourism()
        .archaeology()
        .sites
        .iter()
        .filter(|s| !s.hidden)
        .min_by_key(|s| dims.distance(s.tile, home).unwrap_or(i32::MAX))
        .copied()
        .expect("a visible site");
    let digger = rules
        .units
        .iter()
        .position(|u| u.key == "core:unit/archaeologist")
        .unwrap();
    game.debug_spawn_unit(
        &rules,
        PlayerId(0),
        site.tile,
        classic_rules::ids::UnitKindId(digger as u16),
    );
    let round = game.view().auth.turn().round;
    while game.view().auth.turn().round < round + 10 {
        driver.run(&mut game, &rules, u32::MAX);
    }
    let auth = game.view().auth;
    assert!(
        auth.module().tourism().archaeology().pending.is_empty(),
        "the AI chose"
    );
    let found = auth
        .module()
        .tourism()
        .works()
        .iter()
        .any(|w| w.class == classic_rules::world::tourism::GreatWorkClass::Artifact)
        || auth
            .module()
            .tourism()
            .archaeology()
            .landmark_era(site.tile)
            .is_some();
    assert!(found, "an artifact or a Landmark");
    game.validate(&rules).unwrap();
}
