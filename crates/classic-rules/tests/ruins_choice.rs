//! Ancient Ruins and the human player (`systems::ruins`, `derived::ruins`): a unit that walks
//! onto the ruins takes nothing by itself; the player claims one of the three rewards the place
//! offers with `Command::ClaimRuins`. An AI player is still paid when its unit enters.

use classic_content::core::core_rules;
use classic_rules::classic::ClassicChange;
use classic_rules::command::CommandError;
use classic_rules::derived::ruins::offers_at;
use classic_rules::ids::{ImprovementId, PlayerId, TileId, UnitHandle};
use classic_rules::ruins::{Reward, CULTURE, FAITH, GOLD, REWARDS};
use classic_rules::world::state::GameConfig;
use classic_rules::{compile_rules, Command, CompiledRuleset, GameState};

const P0: PlayerId = PlayerId(0);

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(classic_rules::math::HexCoord { q, r }).unwrap()
}

/// A world with a city, a warrior one step from ruins and the ruins' tile.
fn world(seed: u64, ai: bool) -> (GameState, CompiledRuleset, UnitHandle, TileId) {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(&rules, GameConfig { players: 2, width: 24, height: 16, rng_seed: seed });
    if ai {
        game.configure_players(rules.default_difficulty(), &[P0]);
    }
    game.debug_found_city(&rules, P0, tile(&game, 5, 5));
    let ruins = rules.improvements.iter().position(|i| i.key == "core:improvement/ancient-ruins").unwrap();
    let site = tile(&game, 12, 5);
    game.debug_set_improvement_batch(&rules, &[(site, ImprovementId(ruins as u16))]);
    let kind = classic_rules::ids::UnitKindId(rules.units.iter().position(|u| u.key == "core:unit/warrior").unwrap() as u16);
    let warrior = game.debug_spawn_unit(&rules, P0, tile(&game, 11, 5), kind);
    (game, rules, warrior, site)
}

fn ruins_stand(game: &GameState, site: TileId) -> bool {
    game.view().auth.map().improvement(site).flatten().is_some()
}

fn culture_of(game: &GameState) -> u32 {
    game.view().auth.module().culture().player(P0).map_or(0, |c| c.culture)
}

fn explored(result: &classic_rules::transaction::journal::ChangeJournal) -> Vec<Reward> {
    result
        .module_changes
        .iter()
        .filter_map(|c| match c {
            ClassicChange::RuinsExplored { reward, .. } => Some(*reward),
            _ => None,
        })
        .collect()
}

#[test]
fn a_human_unit_on_ruins_takes_nothing_until_the_player_claims() {
    let (mut game, rules, warrior, site) = world(3, false);
    let treasury = game.view().auth.players().treasury_of(P0);
    let result = game.execute(&rules, Command::MoveUnit { unit: warrior, destination: site }).unwrap();
    assert!(explored(result.changes()).is_empty(), "no automatic reward for a human");
    assert!(ruins_stand(&game, site), "the ruins wait for the claim");
    assert_eq!(game.view().auth.players().treasury_of(P0), treasury);
    game.validate(&rules).unwrap();
}

#[test]
fn a_claim_gives_the_chosen_reward_and_removes_the_ruins() {
    for seed in 0..6 {
        let (mut game, rules, warrior, site) = world(seed, false);
        game.execute(&rules, Command::MoveUnit { unit: warrior, destination: site }).unwrap();
        let offers = offers_at(game.view().auth, site);
        for reward in offers {
            let (mut game, rules, warrior, site) = world(seed, false);
            game.execute(&rules, Command::MoveUnit { unit: warrior, destination: site }).unwrap();
            let before = (
                culture_of(&game),
                game.view().auth.players().treasury_of(P0),
                game.view().auth.module().religion().player(P0).faith,
                game.view().auth.units().get(warrior).unwrap().core.experience,
            );
            let result = game.execute(&rules, Command::ClaimRuins { unit: warrior, reward }).unwrap();
            assert_eq!(explored(result.changes()), vec![reward], "seed {seed}: the journal names the reward");
            assert!(!ruins_stand(&game, site), "seed {seed}: the ruins are gone");
            let auth = game.view().auth;
            let after = (
                culture_of(&game),
                auth.players().treasury_of(P0),
                auth.module().religion().player(P0).faith,
                auth.units().get(warrior).unwrap().core.experience,
            );
            match reward {
                Reward::Culture => assert_eq!(after.0 - before.0, CULTURE),
                Reward::Gold => assert_eq!(after.1 - before.1, GOLD),
                Reward::Faith => assert_eq!(after.2 - before.2, FAITH),
                Reward::Experience => assert!(after.3 > before.3),
                Reward::Citizen | Reward::Map => {}
            }
            game.validate(&rules).unwrap();
        }
    }
}

#[test]
fn a_reward_the_ruins_do_not_offer_is_refused() {
    let (mut game, rules, warrior, site) = world(3, false);
    game.execute(&rules, Command::MoveUnit { unit: warrior, destination: site }).unwrap();
    let offers = offers_at(game.view().auth, site);
    let missing = REWARDS.into_iter().find(|r| !offers.contains(r)).expect("six rewards, three offers");
    let refused = game.execute(&rules, Command::ClaimRuins { unit: warrior, reward: missing }).unwrap_err();
    assert!(matches!(refused, CommandError::RuleViolation(ref why) if why == "the ruins do not offer that"), "{refused:?}");
    assert!(ruins_stand(&game, site), "a refused claim leaves the ruins");
}

#[test]
fn a_unit_away_from_the_ruins_cannot_claim() {
    let (mut game, rules, warrior, site) = world(3, false);
    let reward = offers_at(game.view().auth, site)[0];
    let refused = game.execute(&rules, Command::ClaimRuins { unit: warrior, reward }).unwrap_err();
    assert!(matches!(refused, CommandError::RuleViolation(ref why) if why == "no ancient ruins here"), "{refused:?}");
}

#[test]
fn the_offers_are_three_different_rewards_that_belong_to_the_place() {
    let (game, _rules, _warrior, _site) = world(3, false);
    let auth = game.view().auth;
    let mut sets = std::collections::HashSet::new();
    for i in 0..60u32 {
        let offers = offers_at(auth, TileId(i * 5));
        assert_eq!(offers, offers_at(auth, TileId(i * 5)), "the same place, the same offers");
        assert!(offers[0] != offers[1] && offers[1] != offers[2] && offers[0] != offers[2], "{offers:?}");
        let rank = |r: Reward| REWARDS.iter().position(|x| *x == r).unwrap();
        assert!(rank(offers[0]) < rank(offers[1]) && rank(offers[1]) < rank(offers[2]), "best first: {offers:?}");
        sets.insert(offers);
    }
    assert!(sets.len() >= 8, "the places differ: {} distinct offers in 60 places", sets.len());
}

#[test]
fn an_ai_unit_is_paid_when_it_enters() {
    let (mut game, rules, warrior, site) = world(3, true);
    let result = game.execute(&rules, Command::MoveUnit { unit: warrior, destination: site }).unwrap();
    assert_eq!(explored(result.changes()).len(), 1, "the AI takes its reward on entering");
    assert!(!ruins_stand(&game, site));
    game.validate(&rules).unwrap();
}
