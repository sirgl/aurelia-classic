//! A diplomat at work in a rival's capital softens the ideological tourism penalty
//! (classic/docs/classic/rules.md):
//! +25 % toward a nation with another ideology, nothing otherwise.

use classic_content::core::core_rules;
use classic_rules::derived::tourism::{tourism_multiplier, DIFFERENT_IDEOLOGY_TOURISM_PERCENT, DIPLOMAT_TOURISM_PERCENT};
use classic_rules::ids::{CityHandle, PlayerId, TechId, TileId};
use classic_rules::math::HexCoord;
use classic_rules::rules::content::PolicyBranchId;
use classic_rules::world::espionage::SpyState;
use classic_rules::world::state::GameConfig;
use classic_rules::{compile_rules, ClassicGame, Command, CompiledRuleset, GameState};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
}

fn tech(rules: &CompiledRuleset, key: &str) -> TechId {
    TechId(rules.technologies.iter().position(|t| t.key == key).unwrap() as u16)
}

fn branch(rules: &CompiledRuleset, key: &str) -> PolicyBranchId {
    PolicyBranchId(rules.module.policy_branches.iter().position(|b| b.key == key).unwrap() as u16)
}

fn round(game: &mut GameState, rules: &CompiledRuleset) {
    for _ in 0..game.view().auth.players().count() {
        let player = game.view().auth.active_player();
        game.execute(rules, Command::EndTurn { player }).unwrap();
    }
}

/// Two nations with their capitals apart, P0 with a spy (Renaissance), each with the
/// ideology of the keys given (None: no ideology yet).
fn world(ideologies: [Option<&str>; 2]) -> (GameState, CompiledRuleset, CityHandle) {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(&rules, GameConfig { players: 2, width: 40, height: 24, rng_seed: 7 });
    game.debug_found_city(&rules, P0, tile(&game, 8, 10));
    let theirs = game.debug_found_city(&rules, P1, tile(&game, 14, 10));
    game.debug_meet(P0, P1);
    game.debug_meet(P1, P0);
    for p in [P0, P1] {
        game.debug_learn(&rules, p, tech(&rules, "core:tech/radio"));
    }
    game.debug_learn(&rules, P0, tech(&rules, "core:tech/acoustics"));
    for (player, key) in [P0, P1].into_iter().zip(ideologies) {
        if let Some(key) = key {
            let active = game.view().auth.active_player();
            assert_eq!(active, player);
            game.execute(&rules, Command::ChooseIdeology { player, ideology: branch(&rules, key) }).unwrap();
            game.execute(&rules, Command::EndTurn { player }).unwrap();
        }
    }
    round(&mut game, &rules);
    (game, rules, theirs)
}

fn multiplier(game: &GameState, rules: &CompiledRuleset) -> i32 {
    tourism_multiplier(game.view().auth, rules, P0, P1)
}

/// Send the first spy to `city` as a diplomat.
fn order_diplomat(game: &mut GameState, rules: &CompiledRuleset, city: CityHandle) {
    while game.view().auth.active_player() != P0 {
        let active = game.view().auth.active_player();
        game.execute(rules, Command::EndTurn { player: active }).unwrap();
    }
    game.execute(rules, Command::MoveSpy { player: P0, spy: 0, city: Some(city), diplomat: true }).unwrap();
}

/// Play until the diplomat is at work.
fn wait_at_work(game: &mut GameState, rules: &CompiledRuleset) {
    for _ in 0..12 {
        if game.view().auth.module().espionage().spies_of(P0)[0].state == SpyState::Schmooze {
            return;
        }
        round(game, rules);
    }
    panic!("the diplomat never got to work: {:?}", game.view().auth.module().espionage().spies_of(P0)[0]);
}

fn send_diplomat(game: &mut GameState, rules: &CompiledRuleset, city: CityHandle) {
    order_diplomat(game, rules, city);
    wait_at_work(game, rules);
}

#[test]
fn a_diplomat_in_the_capital_of_another_ideology_softens_the_penalty() {
    let (mut game, rules, theirs) = world([Some("core:policy/freedom"), Some("core:policy/order")]);
    assert_eq!(multiplier(&game, &rules), DIFFERENT_IDEOLOGY_TOURISM_PERCENT, "the penalty alone");
    send_diplomat(&mut game, &rules, theirs);
    assert_eq!(
        multiplier(&game, &rules),
        DIFFERENT_IDEOLOGY_TOURISM_PERCENT + DIPLOMAT_TOURISM_PERCENT,
        "-25 and +15 with a diplomat at work"
    );
    assert_eq!(DIPLOMAT_TOURISM_PERCENT, 15);
    assert_eq!(tourism_multiplier(game.view().auth, &rules, P1, P0), DIFFERENT_IDEOLOGY_TOURISM_PERCENT, "only the sender gains");
    game.validate(&rules).unwrap();
}

#[test]
fn the_diplomat_adds_nothing_while_the_ideologies_agree_or_are_missing() {
    for ideologies in [[Some("core:policy/freedom"), Some("core:policy/freedom")], [Some("core:policy/freedom"), None], [None, None]] {
        let (mut game, rules, theirs) = world(ideologies);
        let before = multiplier(&game, &rules);
        send_diplomat(&mut game, &rules, theirs);
        assert_eq!(multiplier(&game, &rules), before, "{ideologies:?}");
    }
}

#[test]
fn a_diplomat_still_on_its_way_or_sent_home_by_war_adds_nothing() {
    let (mut game, rules, theirs) = world([Some("core:policy/freedom"), Some("core:policy/order")]);
    order_diplomat(&mut game, &rules, theirs);
    assert_eq!(multiplier(&game, &rules), DIFFERENT_IDEOLOGY_TOURISM_PERCENT, "travelling");
    wait_at_work(&mut game, &rules);
    assert!(multiplier(&game, &rules) > DIFFERENT_IDEOLOGY_TOURISM_PERCENT);
    game.execute(&rules, Command::DeclareWar { player: P0, enemy: P1 }).unwrap();
    assert_eq!(multiplier(&game, &rules), DIFFERENT_IDEOLOGY_TOURISM_PERCENT, "war evacuates the diplomats");
}
