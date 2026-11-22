//! What a war on a friend or a defensive pact partner costs the declarer in the opinion of the
//! nations that know it, as Unciv 4.22.10 has it (DeclareWar.kt `breakTreaties`): the betrayed
//! friend holds -40 (`BetrayedDeclarationOfFriendship`), every other nation that knows the
//! declarer -20; a betrayed pact partner -20 (`BetrayedDefensivePact`), the others -10, on top of
//! the friendship's. A war on one's own allied city-state costs -10 with everyone
//! (`handleCityStateDirectAttack`). They fade by 0.67 x 1/8 and 1/16 a turn at Standard pace
//! (DiplomacyTurnManager.kt `revertToZero`). A war on a stranger costs none of them.

use classic_content::core::core_rules;
use classic_rules::ai::diplomacy::trusted;
use classic_rules::ids::{PlayerId, TileId};
use classic_rules::math::HexCoord;
use classic_rules::rules::content::CityStateId;
use classic_rules::world::player::{ClassicKind, PlayerKind};
use classic_rules::world::relations::{DealItem, GrievanceKind, GRIEVANCE_POINT};
use classic_rules::{compile_rules, ClassicGame, Command, CompiledRuleset, GameConfig, GameState};

const BETRAYER: PlayerId = PlayerId(0);
const VICTIM: PlayerId = PlayerId(1);
const WITNESS: PlayerId = PlayerId(2);
const STRANGER: PlayerId = PlayerId(3);
const CS: PlayerId = PlayerId(4);

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
}

/// Four nations and a city-state with a city each; the stranger has met nobody, the others
/// have met each other and the city-state.
fn world() -> (GameState, CompiledRuleset) {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(&rules, GameConfig { players: 5, width: 40, height: 20, rng_seed: 7 });
    game.debug_set_player_kind(CS, PlayerKind::Module(ClassicKind::CityState(CityStateId(0))));
    for (p, q, r) in [(BETRAYER, 3, 3), (VICTIM, 15, 10), (WITNESS, 27, 3), (STRANGER, 37, 17), (CS, 10, 16)] {
        game.debug_found_city(&rules, p, tile(&game, q, r));
    }
    for (a, b) in [(BETRAYER, VICTIM), (BETRAYER, WITNESS), (VICTIM, WITNESS), (BETRAYER, CS), (VICTIM, CS), (WITNESS, CS)] {
        game.debug_meet(a, b);
    }
    (game, rules)
}

fn held(game: &GameState, observer: PlayerId, kind: GrievanceKind) -> i32 {
    game.view().auth.module().relations().grievance(observer, BETRAYER, kind)
}

fn points(n: i32) -> i32 {
    n * GRIEVANCE_POINT
}

/// The betrayed friend holds 40, a nation that knows the betrayer 20, one that does not nothing.
#[test]
fn a_war_on_a_friend_costs_forty_with_it_and_twenty_with_the_others() {
    let (mut game, rules) = world();
    game.debug_deal_item(&rules, BETRAYER, VICTIM, DealItem::Friendship).unwrap();
    game.execute(&rules, Command::DeclareWar { player: BETRAYER, enemy: VICTIM }).unwrap();
    let kind = GrievanceKind::BetrayedFriendship;
    assert_eq!((held(&game, VICTIM, kind), held(&game, WITNESS, kind), held(&game, STRANGER, kind)), (points(40), points(20), 0));
    assert_eq!(held(&game, VICTIM, GrievanceKind::BetrayedPact), 0);
}

/// A betrayed pact costs 20 with the partner and 10 with the others; with a friendship besides,
/// both are held.
#[test]
fn a_war_on_a_pact_partner_costs_twenty_with_it_and_ten_with_the_others() {
    let (mut game, rules) = world();
    game.debug_deal_item(&rules, BETRAYER, VICTIM, DealItem::DefensivePact).unwrap();
    game.debug_deal_item(&rules, BETRAYER, VICTIM, DealItem::Friendship).unwrap();
    game.execute(&rules, Command::DeclareWar { player: BETRAYER, enemy: VICTIM }).unwrap();
    let pact = GrievanceKind::BetrayedPact;
    assert_eq!((held(&game, VICTIM, pact), held(&game, WITNESS, pact)), (points(20), points(10)));
    assert_eq!(held(&game, VICTIM, GrievanceKind::BetrayedFriendship), points(40), "the friendship's too");
}

/// A war on a nation the declarer had no treaty with costs no grievance.
#[test]
fn a_war_on_a_stranger_costs_no_grievance() {
    let (mut game, rules) = world();
    game.execute(&rules, Command::DeclareWar { player: BETRAYER, enemy: VICTIM }).unwrap();
    assert!(game.view().auth.module().relations().grievances().is_empty());
}

/// A war on one's own allied city-state costs 10 with every nation that knows the declarer.
#[test]
fn a_war_on_its_own_allied_city_state_costs_ten_with_everyone() {
    let (mut game, rules) = world();
    game.debug_set_influence(&rules, CS, BETRAYER, 100);
    assert_eq!(game.view().auth.module().city_states().ally(CS), Some(BETRAYER));
    game.execute(&rules, Command::DeclareWar { player: BETRAYER, enemy: CS }).unwrap();
    let kind = GrievanceKind::BetrayedFriendship;
    assert_eq!((held(&game, VICTIM, kind), held(&game, WITNESS, kind), held(&game, STRANGER, kind)), (points(10), points(10), 0));
}

/// The betrayal fades as Unciv's modifier does, at the holder's turn end: 0.67 x 1/8 of a point a
/// round for a friendship, 0.67 x 1/16 for a pact, at Standard pace.
#[test]
fn a_betrayal_fades_as_the_modifier_of_unciv_does() {
    let (mut game, rules) = world();
    game.debug_deal_item(&rules, BETRAYER, VICTIM, DealItem::DefensivePact).unwrap();
    game.debug_deal_item(&rules, BETRAYER, VICTIM, DealItem::Friendship).unwrap();
    game.execute(&rules, Command::DeclareWar { player: BETRAYER, enemy: VICTIM }).unwrap();
    let round = game.view().auth.turn().round;
    while game.view().auth.turn().round == round {
        let active = game.view().auth.active_player();
        game.execute(&rules, Command::EndTurn { player: active }).unwrap();
    }
    assert_eq!(GrievanceKind::BetrayedFriendship.fading_at(100), 837);
    assert_eq!(GrievanceKind::BetrayedPact.fading_at(100), 418);
    assert_eq!(held(&game, VICTIM, GrievanceKind::BetrayedFriendship), points(40) - 837);
    assert_eq!(held(&game, VICTIM, GrievanceKind::BetrayedPact), points(20) - 418);
}

/// The AI reads it: the betrayed friend no longer trusts the betrayer (40 points are 400 on the
/// scale of the warmonger memory), and neither does a witness (200; distrust begins at 100).
#[test]
fn the_ai_distrusts_a_betrayer() {
    let (mut game, rules) = world();
    game.debug_deal_item(&rules, BETRAYER, VICTIM, DealItem::Friendship).unwrap();
    game.execute(&rules, Command::DeclareWar { player: BETRAYER, enemy: VICTIM }).unwrap();
    for (observer, remembered) in [(VICTIM, 400), (WITNESS, 200)] {
        let obs = game.ai_observation(&rules, observer);
        let relation = *obs.relation(BETRAYER).unwrap();
        assert!(relation.remembered_against() >= remembered, "{observer:?}: {}", relation.remembered_against());
        assert!(!trusted(obs.round, &relation), "{observer:?}");
    }
}
