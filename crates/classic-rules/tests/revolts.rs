//! Rebels at very high unhappiness (classic/docs/classic/rules.md, Happiness;
//! Unciv 4.22.10 `TurnManager.kt` `updateRevolts` / `doRevoltSpawn`): below
//! -20 happiness a nation's rebels rise for the barbarians after a countdown
//! of 4-6 of its turns; at -20 nothing rises; no barbarians, no rebels; the
//! rebels are melee land units the nation could build now; the same seed
//! gives the same rising; the countdown survives save and load.

use classic_content::core::core_rules;
use classic_rules::classic::ClassicChange;
use classic_rules::derived::happiness::{player_happiness, REBELLION_THRESHOLD};
use classic_rules::ids::{CityHandle, PlayerId, TileId, UnitKindId};
use classic_rules::math::HexCoord;
use classic_rules::replay::authoritative_hash;
use classic_rules::rules::content::Domain;
use classic_rules::world::barbarians::{rebel_kinds, BarbarianMode, REVOLT_DELAY_BASE, REVOLT_DELAY_RANDOM};
use classic_rules::world::player::{ClassicKind, PlayerKind};
use classic_rules::world::state::GameConfig;
use classic_rules::{compile_rules, ClassicGame, Command, CompiledRuleset, GameState};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const BARBARIANS: PlayerId = PlayerId(2);

/// The longest countdown at Standard speed: 4 + 2 turns.
const LONGEST_DELAY: u32 = REVOLT_DELAY_BASE + REVOLT_DELAY_RANDOM as u32 - 1;

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
}

/// Two nations with a city each in opposite corners of a 30 × 20 map and
/// the barbarian player (P2) in `mode`; P0's city has the population that
/// puts P0's excess happiness at `excess`.
fn world(seed: u64, mode: BarbarianMode, excess: i32) -> (GameState, CompiledRuleset, CityHandle, u32) {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(&rules, GameConfig { players: 3, width: 30, height: 20, rng_seed: seed });
    let city = game.debug_found_city(&rules, P0, tile(&game, 8, 8));
    game.debug_found_city(&rules, P1, tile(&game, 26, 16));
    game.debug_set_player_kind(BARBARIANS, PlayerKind::Module(ClassicKind::Barbarian));
    game.debug_set_barbarian_mode(mode);
    let population = (1..80)
        .find(|p| {
            game.debug_set_population(&rules, city, *p);
            player_happiness(game.view().auth, &rules, P0).excess() == excess
        })
        .unwrap_or_else(|| panic!("a population with excess happiness {excess}"));
    (game, rules, city, population)
}

/// One round (every player's end turn); P0's population is held where the
/// world put it (growth and starvation do not move the happiness). The
/// rebellions the round's journal tells.
fn round(game: &mut GameState, rules: &CompiledRuleset, city: CityHandle, population: u32) -> Vec<ClassicChange> {
    let mut rose = Vec::new();
    for _ in 0..3 {
        let active = game.view().auth.active_player();
        if active == P0 {
            game.debug_set_population(rules, city, population);
        }
        let result = game.execute(rules, Command::EndTurn { player: active }).unwrap();
        rose.extend(result.changes().module_changes.iter().filter(|c| matches!(c, ClassicChange::RebelsRose { .. })).copied());
    }
    rose
}

/// The rebellions of `rounds` rounds, with the round of each.
fn play(game: &mut GameState, rules: &CompiledRuleset, city: CityHandle, population: u32, rounds: u32) -> Vec<(u32, ClassicChange)> {
    let mut out = Vec::new();
    for r in 0..rounds {
        out.extend(round(game, rules, city, population).into_iter().map(|c| (r, c)));
    }
    out
}

/// Unciv: below [-20] happiness rebels rise after `baseTurnsUntilRevolt`
/// (4) + rand(3) turns, for the barbarians, on a land tile near a city of
/// the unhappy nation, of a melee land kind it could build; the excess
/// is -21 all along, so they rise again after the next countdown.
#[test]
fn rebels_rise_at_minus_21_after_the_countdown() {
    let (mut game, rules, city, population) = world(5, BarbarianMode::Normal, REBELLION_THRESHOLD);
    assert_eq!(REBELLION_THRESHOLD, -21);
    let risings = play(&mut game, &rules, city, population, 2 * (LONGEST_DELAY + 1) + 1);
    assert!(risings.len() >= 2, "a rising, then another after the next countdown: {risings:?}");
    let (first, _) = risings[0];
    assert!((REVOLT_DELAY_BASE - 1..=LONGEST_DELAY).contains(&first), "the first rising after 4-6 turns below -20, round {first}");
    let auth = game.view().auth;
    let kinds = rebel_kinds(auth, &rules, P0);
    let city_tile = auth.cities().get(city).unwrap().tile;
    for (_, change) in &risings {
        let ClassicChange::RebelsRose { player, city: at, tile, kind, count } = *change else { unreachable!() };
        assert_eq!((player, at), (P0, city));
        assert!((1..=1).contains(&count), "one city: one rebel ({count})");
        assert!(kinds.contains(&kind), "a kind P0 could build now");
        let def = &rules.units[kind.0 as usize];
        assert!(def.domain == Domain::Land && def.strength > 0 && def.ranged_strength == 0 && def.nation.is_none());
        assert_eq!(auth.map().owning_city(tile), Some(city), "the rising tile is the city's");
        assert_ne!(tile, city_tile, "never on the city centre");
    }
    let rebels = auth.indexes().units_of(BARBARIANS).iter().filter(|u| {
        let v = auth.units().get(**u).unwrap();
        auth.map().dimensions.distance(v.core.tile, city_tile).unwrap() <= 5 && kinds.contains(&v.core.kind)
    });
    assert!(rebels.count() >= 1, "the rebels stand in P0's land for the barbarians");
}

/// At -20 the empire is very unhappy but not rebellious: nothing rises and
/// no countdown runs.
#[test]
fn no_rebels_at_minus_20() {
    let (mut game, rules, city, population) = world(5, BarbarianMode::Normal, REBELLION_THRESHOLD + 1);
    let risings = play(&mut game, &rules, city, population, 3 * (LONGEST_DELAY + 1));
    assert!(risings.is_empty(), "{risings:?}");
    assert!(game.view().auth.module().barbarians().revolts().is_empty());
}

/// Unciv: no barbarian civilization, no revolts.
#[test]
fn no_rebels_without_barbarians() {
    let (mut game, rules, city, population) = world(5, BarbarianMode::None, REBELLION_THRESHOLD - 5);
    let risings = play(&mut game, &rules, city, population, 3 * (LONGEST_DELAY + 1));
    assert!(risings.is_empty(), "{risings:?}");
}

/// A countdown stops when the happiness comes back to -20 before it ends.
#[test]
fn a_countdown_ends_when_the_happiness_recovers() {
    let (mut game, rules, city, population) = world(5, BarbarianMode::Normal, REBELLION_THRESHOLD);
    assert!(round(&mut game, &rules, city, population).is_empty());
    assert!(game.view().auth.module().barbarians().revolt_countdown(P0).is_some(), "the countdown started");
    let calmer = population - 1;
    let risings = play(&mut game, &rules, city, calmer, 3 * (LONGEST_DELAY + 1));
    assert!(risings.is_empty(), "{risings:?}");
    assert_eq!(game.view().auth.module().barbarians().revolt_countdown(P0), None);
}

/// The same seed gives the same rising (tile, kind, count) and the same
/// world; another seed is its own game.
#[test]
fn rebels_are_deterministic_by_seed() {
    let run = |seed: u64| {
        let (mut game, rules, city, population) = world(seed, BarbarianMode::Normal, REBELLION_THRESHOLD - 4);
        let risings = play(&mut game, &rules, city, population, 2 * (LONGEST_DELAY + 1));
        (risings, authoritative_hash(&game))
    };
    let (a, hash_a) = run(7);
    let (b, hash_b) = run(7);
    assert!(!a.is_empty(), "rebels rose");
    assert_eq!(a, b);
    assert_eq!(hash_a, hash_b);
}

/// More cities, more rebels: one city of eleven or more gives one to three
/// (Unciv: `1 + rand(100 + 20 (cities - 1)) / 100`); every rebel stands on a
/// tile of its own.
#[test]
fn a_wide_empire_raises_more_rebels() {
    let (mut game, rules, city, _) = world(11, BarbarianMode::Normal, 0);
    let mut cities = vec![city];
    for (q, r) in [(3, 3), (13, 3), (3, 13), (13, 13), (18, 8), (8, 17), (18, 2), (23, 8), (1, 8), (13, 8)] {
        cities.push(game.debug_found_city(&rules, P0, tile(&game, q, r)));
    }
    let population = (1..40)
        .find(|p| {
            for c in &cities {
                game.debug_set_population(&rules, *c, *p);
            }
            player_happiness(game.view().auth, &rules, P0).excess() <= REBELLION_THRESHOLD
        })
        .expect("eleven cities fall below -20");
    let mut counts = Vec::new();
    for _ in 0..40 {
        for change in round(&mut game, &rules, city, population) {
            let ClassicChange::RebelsRose { count, .. } = change else { unreachable!() };
            counts.push(count);
        }
        for c in &cities {
            game.debug_set_population(&rules, *c, population);
        }
    }
    assert!(counts.len() >= 4, "{counts:?}");
    assert!(counts.iter().all(|c| (1..=3).contains(c)), "{counts:?}");
    assert!(counts.iter().any(|c| *c > 1), "eleven cities: two rebels or more some of the time: {counts:?}");
    let auth = game.view().auth;
    let mut tiles: Vec<TileId> = auth.indexes().units_of(BARBARIANS).iter().map(|u| auth.units().get(*u).unwrap().core.tile).collect();
    let all = tiles.len();
    tiles.sort();
    tiles.dedup();
    assert_eq!(tiles.len(), all, "one rebel per tile");
}

/// The rebels are of the nation's era: with a later melee unit known, it
/// joins the kinds; the kinds its own technology made obsolete leave.
#[test]
fn rebels_follow_the_era_of_the_nation() {
    let (mut game, rules, _, _) = world(5, BarbarianMode::Normal, 0);
    let start = rebel_kinds(game.view().auth, &rules, P0);
    assert!(!start.is_empty(), "a nation at the start can raise rebels");
    let later = rules
        .units
        .iter()
        .enumerate()
        .find(|(_, d)| {
            d.domain == Domain::Land && d.strength > 0 && d.ranged_strength == 0 && d.nation.is_none() && !d.unbuildable && d.required_resources.is_empty() && d.required_tech.is_some() && d.required_project.is_none()
        })
        .map(|(i, d)| (UnitKindId(i as u16), d.required_tech.unwrap()))
        .expect("a melee unit that needs a technology");
    assert!(!start.contains(&later.0), "not before its technology");
    let mut prerequisites = vec![later.1];
    while let Some(tech) = prerequisites.pop() {
        if game.view().auth.players().techs_of(P0).contains(&tech) {
            continue;
        }
        let pending: Vec<_> = rules.technologies[tech.0 as usize].requires.iter().copied().filter(|t| !game.view().auth.players().techs_of(P0).contains(t)).collect();
        if pending.is_empty() {
            game.debug_discover(&rules, P0, tech);
        } else {
            prerequisites.push(tech);
            prerequisites.extend(pending);
        }
    }
    let now = rebel_kinds(game.view().auth, &rules, P0);
    assert!(now.contains(&later.0), "the new melee unit can rise");
    for kind in &now {
        let def = &rules.units[kind.0 as usize];
        assert!(def.obsolete_tech.is_none_or(|t| !game.view().auth.players().techs_of(P0).contains(&t)), "nothing obsolete");
    }
}

/// A running countdown is saved and loaded; the loaded game rises as the
/// original does.
#[test]
fn a_rebellion_countdown_survives_save_and_load() {
    let (mut game, rules, city, population) = world(5, BarbarianMode::Normal, REBELLION_THRESHOLD);
    assert!(round(&mut game, &rules, city, population).is_empty());
    let countdown = game.view().auth.module().barbarians().revolt_countdown(P0);
    assert!(countdown.is_some());
    let save = classic_rules::persistence::encode::encode(&game, &rules);
    let bytes = classic_rules::save_codec::to_bytes(&save);
    let mut loaded = classic_rules::persistence::decode::decode(&classic_rules::save_codec::from_bytes(&bytes).unwrap(), &rules).unwrap();
    assert_eq!(loaded.view().auth.module().barbarians().revolt_countdown(P0), countdown);
    assert_eq!(authoritative_hash(&loaded), authoritative_hash(&game));
    let a = play(&mut game, &rules, city, population, LONGEST_DELAY + 1);
    let b = play(&mut loaded, &rules, city, population, LONGEST_DELAY + 1);
    assert!(!a.is_empty());
    assert_eq!(a, b);
}
