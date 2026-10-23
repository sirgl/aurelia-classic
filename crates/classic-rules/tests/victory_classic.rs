//! Victories on the classic ruleset (classic/docs/classic/rules.md): domination at
//! the capture of the last original capital, the spaceship (Apollo, parts
//! installed in the capital), time and score, disabled victories, one more
//! turn after the victory, save/load of the outcome.

use classic_rules::ClassicGame;
use classic_content::core::core_rules;
use classic_rules::ids::{CityHandle, PlayerId, ProductionItem, TechId, TileId, UnitKindId};
use classic_rules::math::HexCoord;
use classic_rules::replay::authoritative_hash;
use classic_rules::rules::content::ProjectId;
use classic_rules::world::state::GameConfig;
use classic_rules::world::victory::{VictoryKind, VictoryOptions};
use classic_rules::{compile_rules, Command, CompiledRuleset, GameState};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view()
        .auth
        .map()
        .dimensions
        .to_tile(HexCoord { q, r })
        .unwrap()
}

fn unit(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap() as u16)
}

fn project(rules: &CompiledRuleset, key: &str) -> ProjectId {
    ProjectId(rules.module.projects.iter().position(|p| p.key == key).unwrap() as u16)
}

fn tech(rules: &CompiledRuleset, key: &str) -> TechId {
    TechId(
        rules
            .technologies
            .iter()
            .position(|t| t.key == key)
            .unwrap() as u16,
    )
}

/// Two civilizations: P0 with its capital, P1 with its capital and a
/// second city (so it survives the loss of its capital).
fn world() -> (GameState, CompiledRuleset, CityHandle, CityHandle) {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(
        &rules,
        GameConfig {
            players: 2,
            width: 20,
            height: 16,
            rng_seed: 3,
        },
    );
    let mine = game.debug_found_city(&rules, P0, tile(&game, 3, 3));
    let theirs = game.debug_found_city(&rules, P1, tile(&game, 9, 8));
    game.debug_found_city(&rules, P1, tile(&game, 16, 13));
    game.debug_meet(P0, P1);
    (game, rules, mine, theirs)
}

fn until_active(game: &mut GameState, rules: &CompiledRuleset, player: PlayerId) {
    for _ in 0..4 {
        let active = game.view().auth.active_player();
        if active == player {
            return;
        }
        game.execute(rules, Command::EndTurn { player: active })
            .unwrap();
    }
    panic!("{player:?} never became active");
}

fn capture(game: &mut GameState, rules: &CompiledRuleset, city: CityHandle) {
    let attacker = game.debug_spawn_unit(
        rules,
        P0,
        tile(game, 8, 8),
        unit(rules, "core:unit/warrior"),
    );
    game.execute(
        rules,
        Command::DeclareWar {
            player: P0,
            enemy: P1,
        },
    )
    .unwrap();
    let core = game.view().auth.cities().get(city).unwrap();
    let max = classic_rules::combat::city::max_hit_points(game.view().auth, rules, core);
    game.debug_set_city_damage(city, max - 1);
    game.execute(rules, Command::AttackCity { attacker, city })
        .unwrap();
}

#[test]
fn holding_every_original_capital_is_domination() {
    let (mut game, rules, _, theirs) = world();
    capture(&mut game, &rules, theirs);
    let outcome = game.outcome().expect("domination at the capture");
    assert_eq!(
        (outcome.winner, outcome.kind),
        (P0, VictoryKind::Domination)
    );
    assert!(
        game.view().auth.players().is_alive(P1),
        "the loser still has a city"
    );
    assert!(game.is_over());
    game.continue_after_victory();
    assert!(!game.is_over(), "one more turn");
    until_active(&mut game, &rules, P1);
    assert_eq!(game.winner(), Some(P0), "the outcome stays");
}

#[test]
fn a_disabled_victory_is_not_awarded() {
    let (mut game, rules, _, theirs) = world();
    game.debug_set_victory_options(VictoryOptions {
        disabled: VictoryKind::Domination.bit(),
        ..VictoryOptions::default()
    });
    capture(&mut game, &rules, theirs);
    assert_eq!(game.outcome(), None);
}

#[test]
fn the_spaceship_needs_apollo_and_every_part_in_the_capital() {
    let (mut game, rules, mine, _) = world();
    let apollo = project(&rules, "core:project/apollo_program");
    let enqueue = |item| Command::EnqueueProduction {
        player: P0,
        city: mine,
        item,
    };
    assert!(
        game.execute(&rules, enqueue(ProductionItem::Project(apollo)))
            .is_err(),
        "Rocketry first"
    );
    game.debug_learn(&rules, P0, tech(&rules, "core:tech/rocketry"));
    game.execute(&rules, enqueue(ProductionItem::Project(apollo)))
        .unwrap();
    let cockpit = unit(&rules, "core:unit/ss-cockpit");
    game.debug_learn(&rules, P0, tech(&rules, "core:tech/satellites"));
    assert!(
        game.execute(&rules, enqueue(ProductionItem::Unit(cockpit)))
            .is_err(),
        "parts need the Apollo Program"
    );
    game.debug_add_project(P0, apollo);
    let capital = tile(&game, 3, 3);
    let parts = [
        "ss-booster",
        "ss-booster",
        "ss-booster",
        "ss-cockpit",
        "ss-engine",
        "ss-stasis-chamber",
    ];
    for (i, key) in parts.iter().enumerate() {
        let part = game.debug_spawn_unit(
            &rules,
            P0,
            capital,
            unit(&rules, &format!("core:unit/{key}")),
        );
        game.execute(&rules, Command::BuildSpaceshipPart { unit: part })
            .unwrap();
        let done = i + 1 == parts.len();
        assert_eq!(game.outcome().is_some(), done, "after {key} #{i}");
    }
    let outcome = game.outcome().unwrap();
    assert_eq!((outcome.winner, outcome.kind), (P0, VictoryKind::Science));
}

#[test]
fn the_best_score_wins_at_the_last_round() {
    let (mut game, rules, _, _) = world();
    game.debug_set_victory_options(VictoryOptions {
        max_rounds: 12,
        ..VictoryOptions::default()
    });
    let score = |g: &GameState, p| classic_rules::world::victory::score(g.view().auth, &rules, p);
    assert!(
        score(&game, P1) > score(&game, P0),
        "two cities score more than one"
    );
    while game.outcome().is_none() && game.view().auth.turn().round < 14 {
        let active = game.view().auth.active_player();
        game.execute(&rules, Command::EndTurn { player: active })
            .unwrap();
    }
    let outcome = game.outcome().expect("a time victory");
    assert_eq!(
        (outcome.winner, outcome.kind, outcome.round),
        (P1, VictoryKind::Time, 12)
    );
}

#[test]
fn the_outcome_survives_save_and_load() {
    let (mut game, rules, _, theirs) = world();
    capture(&mut game, &rules, theirs);
    game.continue_after_victory();
    let save = classic_rules::persistence::encode::encode(&game, &rules);
    let loaded = classic_rules::persistence::decode::decode(&save, &rules).unwrap();
    assert_eq!(authoritative_hash(&loaded), authoritative_hash(&game));
    assert_eq!(loaded.outcome(), game.outcome());
    assert!(!loaded.is_over());
}

/// Round of the first victory test (`MIN_ROUNDS_TO_TEST` = 10 in `systems::victory`).
const AFTER_THE_GRACE_ROUNDS: u32 = 13;

/// P0's great musician performs a concert next to P1's capital: its tourism blast makes P0
/// Embraced on a young civilization.
fn concert_on_rival(game: &mut GameState, rules: &CompiledRuleset) {
    // A civilization with no culture at all has no influence level: let one round pass.
    let round = game.view().auth.turn().round;
    play_to_round(game, rules, round + 1);
    // One concert makes the audience Noticed (its floor is one level); twenty make a young civilization Prevailing.
    for _ in 0..20 {
        let musician = game.debug_spawn_unit(rules, P0, tile(game, 10, 8), unit(rules, "core:unit/great-musician"));
        game.execute(rules, Command::PerformConcert { unit: musician }).unwrap();
    }
    let auth = game.view().auth;
    assert!(
        classic_rules::derived::tourism::influence_level(auth, P0, P1) >= Some(classic_rules::derived::tourism::InfluenceLevel::Embraced),
        "the concert made P0 influential on P1"
    );
}

fn play_to_round(game: &mut GameState, rules: &CompiledRuleset, round: u32) {
    while game.outcome().is_none() && game.view().auth.turn().round < round {
        let active = game.view().auth.active_player();
        game.execute(rules, Command::EndTurn { player: active }).unwrap();
    }
}

/// Being Embraced on every other civilization wins the game (Outcome::Cultural), through the
/// turn step that tests the victories, and only after the first rounds.
#[test]
fn influence_over_every_rival_is_a_cultural_victory() {
    let (mut game, rules, _, _) = world();
    concert_on_rival(&mut game, &rules);
    assert_eq!(game.outcome(), None, "the victories are tested at the turn step, not at the concert");
    play_to_round(&mut game, &rules, AFTER_THE_GRACE_ROUNDS);
    let outcome = game.outcome().expect("a cultural victory");
    assert_eq!((outcome.winner, outcome.kind), (P0, VictoryKind::Cultural));
    assert!(outcome.round > 10, "not within the first ten rounds: round {}", outcome.round);
    assert!(game.is_over());
    assert_eq!(game.winner(), Some(P0));
    game.validate(&rules).unwrap();
    // The outcome survives a save.
    let save = classic_rules::persistence::encode::encode(&game, &rules);
    let loaded = classic_rules::persistence::decode::decode(&save, &rules).unwrap();
    assert_eq!(loaded.outcome(), game.outcome());
}

#[test]
fn without_influence_or_with_the_victory_off_nobody_wins_by_culture() {
    let (mut game, rules, _, _) = world();
    play_to_round(&mut game, &rules, AFTER_THE_GRACE_ROUNDS);
    assert_eq!(game.outcome(), None, "no influence, no victory");
    let (mut game, rules, _, _) = world();
    game.debug_set_victory_options(VictoryOptions { disabled: VictoryKind::Cultural.bit(), ..VictoryOptions::default() });
    concert_on_rival(&mut game, &rules);
    play_to_round(&mut game, &rules, AFTER_THE_GRACE_ROUNDS);
    assert_eq!(game.outcome(), None, "a disabled cultural victory is not awarded");
}
