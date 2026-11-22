//! World Congress and United Nations on the classic ruleset (classic/docs/classic/rules.md): founding with Printing Press, host and delegates, proposals and
//! the tally, resolutions in force, the United Nations' World Leader vote
//! and the diplomatic victory, the delegates needed, save/load, the AI.

use classic_rules::ClassicGame;
use classic_content::core::core_rules;
use classic_rules::ai::congress::vote_orders;
use classic_rules::ids::{PlayerId, TechId, TileId, UnitKindId};
use classic_rules::math::HexCoord;
use classic_rules::replay::authoritative_hash;
use classic_rules::rules::content::CityStateId;
use classic_rules::world::congress::{votes_needed_for, ResolutionKind, SESSIONS, YES};
use classic_rules::world::player::PlayerKind;
use classic_rules::world::state::GameConfig;
use classic_rules::world::victory::{VictoryKind, VictoryOptions};
use classic_rules::{compile_rules, Command, CompiledRuleset, GameState};
use classic_rules::world::player::ClassicKind;

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const CS: PlayerId = PlayerId(2);

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view()
        .auth
        .map()
        .dimensions
        .to_tile(HexCoord { q, r })
        .unwrap()
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

/// Two nations and a city-state, all met.
fn world() -> (GameState, CompiledRuleset) {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(
        &rules,
        GameConfig {
            players: 3,
            width: 30,
            height: 20,
            rng_seed: 4,
        },
    );
    game.debug_set_player_kind(CS, PlayerKind::Module(ClassicKind::CityState(CityStateId(0))));
    game.debug_found_city(&rules, P0, tile(&game, 3, 3));
    game.debug_found_city(&rules, P1, tile(&game, 24, 15));
    game.debug_found_city(&rules, CS, tile(&game, 13, 9));
    game.debug_meet_all();
    (game, rules)
}

fn end_round(game: &mut GameState, rules: &CompiledRuleset) {
    let round = game.view().auth.turn().round;
    while game.view().auth.turn().round == round {
        let active = game.view().auth.active_player();
        game.execute(rules, Command::EndTurn { player: active })
            .unwrap();
    }
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

#[test]
fn delegates_needed_follow_the_classic_table() {
    let un = SESSIONS[SESSIONS.len() - 1];
    // Unciv: VictoryManager.kt, n x (67 - floor(1.1 n)) / 100 + 1 up to 28
    // votes, n x 35 / 100 above; n = 2 + 4 x nations + 1 x city-states (the United Nations seats of our
    // own design: host 2, nation 4, city-state 1).
    for (nations, cs, needed) in [
        (2.0, 4.0, 8),
        (4.0, 8.0, 11),
        (6.0, 12.0, 13),
        (8.0, 16.0, 17),
    ] {
        assert_eq!(
            votes_needed_for(nations, cs, un),
            needed,
            "{nations} nations, {cs} city-states"
        );
    }
}

#[test]
fn printing_press_founds_the_congress_with_its_discoverer_as_host() {
    let (mut game, rules) = world();
    end_round(&mut game, &rules);
    assert!(!game.view().auth.module().congress().founded());
    game.debug_learn(&rules, P1, tech(&rules, "core:tech/printing-press"));
    end_round(&mut game, &rules);
    let congress = game.view().auth.module().congress();
    assert!(congress.founded());
    assert_eq!(congress.host(), Some(P1));
    assert_eq!(congress.proposers(), &[P1, P0]);
    assert_eq!(
        classic_rules::world::congress::delegates(game.view().auth, &rules, P1),
        3,
        "2 as a nation and 1 as the host"
    );
    assert_eq!(
        classic_rules::world::congress::delegates(game.view().auth, &rules, P0),
        2
    );
}

#[test]
fn a_proposal_passes_with_more_yes_than_no() {
    let (mut game, rules) = world();
    game.debug_learn(&rules, P0, tech(&rules, "core:tech/printing-press"));
    end_round(&mut game, &rules);
    until_active(&mut game, &rules, P0);
    game.execute(
        &rules,
        Command::ProposeResolution {
            player: P0,
            kind: ResolutionKind::StandingArmyTax,
            choice: 0,
            repeal: false,
        },
    )
    .unwrap();
    for _ in 0..40 {
        if game.view().auth.module().congress().in_session() {
            break;
        }
        end_round(&mut game, &rules);
    }
    assert!(
        game.view().auth.module().congress().in_session(),
        "the session opened"
    );
    until_active(&mut game, &rules, P0);
    game.execute(
        &rules,
        Command::CastVotes {
            player: P0,
            proposal: 0,
            choice: YES,
            delegates: 2,
        },
    )
    .unwrap();
    end_round(&mut game, &rules);
    end_round(&mut game, &rules);
    let congress = game.view().auth.module().congress();
    assert!(!congress.in_session());
    assert!(congress.is_active(ResolutionKind::StandingArmyTax));
}

/// In the United Nations the World Leader vote alternates with the
/// proposals; the host with its allied city-state's delegates wins.
#[test]
fn the_world_leader_wins_a_diplomatic_victory() {
    let (mut game, rules) = world();
    for i in 0..rules.technologies.len() {
        game.debug_learn(&rules, P0, TechId(i as u16));
        game.debug_learn(&rules, P1, TechId(i as u16));
    }
    game.debug_set_influence(&rules, CS, P0, 200);
    end_round(&mut game, &rules);
    assert!(game.view().auth.module().congress().united_nations());
    for _ in 0..40 {
        if game.outcome().is_some() {
            break;
        }
        let congress = game.view().auth.module().congress();
        let leader_vote = congress.in_session()
            && congress
                .proposals()
                .iter()
                .any(|p| p.kind == ResolutionKind::DiplomaticVictory);
        if leader_vote && congress.delegates_left(P0) > 0 {
            until_active(&mut game, &rules, P0);
            let orders = vote_orders(&game.ai_observation(&rules, P0));
            for order in orders {
                game.execute(&rules, order).unwrap();
            }
        }
        end_round(&mut game, &rules);
    }
    let outcome = game.outcome().expect("a diplomatic victory");
    assert_eq!(
        (outcome.winner, outcome.kind),
        (P0, VictoryKind::Diplomatic)
    );
    assert_eq!(game.view().auth.module().victory().world_leader(), Some(P0));
}

#[test]
fn the_congress_survives_save_and_load() {
    let (mut game, rules) = world();
    game.debug_learn(&rules, P0, tech(&rules, "core:tech/printing-press"));
    end_round(&mut game, &rules);
    end_round(&mut game, &rules);
    let save = classic_rules::persistence::encode::encode(&game, &rules);
    let loaded = classic_rules::persistence::decode::decode(&save, &rules).unwrap();
    assert_eq!(authoritative_hash(&loaded), authoritative_hash(&game));
    assert_eq!(
        loaded.view().auth.module().congress(),
        game.view().auth.module().congress()
    );
}

// ---- the resolutions: their effects (own design, derived/congress) ----

/// P0 founds the congress and passes `kind` with `choice` through a session (P0 holds the delegates).
fn enact(game: &mut GameState, rules: &CompiledRuleset, kind: ResolutionKind, choice: u32) {
    enact_with(game, rules, kind, choice, |_| {});
}

/// `enact`, running `during` once the session has opened (before the votes are cast).
fn enact_with(game: &mut GameState, rules: &CompiledRuleset, kind: ResolutionKind, choice: u32, during: impl FnOnce(&mut GameState)) {
    if !game.view().auth.module().congress().founded() {
        game.debug_learn(rules, P0, tech(rules, "core:tech/printing-press"));
        end_round(game, rules);
    }
    until_active(game, rules, P0);
    game.execute(rules, Command::ProposeResolution { player: P0, kind, choice, repeal: false }).unwrap();
    for _ in 0..60 {
        if game.view().auth.module().congress().in_session() {
            break;
        }
        end_round(game, rules);
    }
    assert!(game.view().auth.module().congress().in_session(), "the session opened");
    during(game);
    until_active(game, rules, P0);
    let delegates = game.view().auth.module().congress().delegates_of(P0);
    game.execute(rules, Command::CastVotes { player: P0, proposal: 0, choice: YES, delegates }).unwrap();
    end_round(game, rules);
    end_round(game, rules);
    assert!(game.view().auth.module().congress().is_active(kind), "{kind:?} is in force");
}

#[test]
fn an_embargo_forbids_new_routes_and_ends_the_running_ones() {
    use classic_rules::derived::trade::{route_status, TradeBlock};
    use classic_rules::world::trade::RouteKind;
    let (mut game, rules) = world();
    game.debug_learn(&rules, P0, tech(&rules, "core:tech/animal-husbandry"));
    let kind = UnitKindId(rules.units.iter().position(|u| u.key == "core:unit/caravan").unwrap() as u16);
    // A neighbour of P1 close to P0's capital, and the whole map known to P0.
    let near = game.debug_found_city(&rules, P1, tile(&game, 8, 5));
    let every: Vec<TileId> = (0..600).map(TileId).collect();
    game.debug_reveal(P0, &every);
    let home = game.view().auth.indexes().cities_of(P0)[0];
    let home_tile = game.view().auth.cities().get(home).unwrap().tile;
    // The route starts while the session is open, so that it is still running when the embargo passes.
    enact_with(&mut game, &rules, ResolutionKind::PlayerEmbargo, u32::from(P1.0), |game| {
        let unit = game.debug_spawn_unit(&rules, P0, home_tile, kind);
        game.execute(&rules, Command::EstablishTradeRoute { unit, destination: near, kind: RouteKind::International }).unwrap();
        assert_eq!(game.view().auth.module().trade().routes().len(), 1, "a route runs");
    });
    assert!(game.view().auth.module().trade().routes().is_empty(), "the embargo ended the route");
    let unit = game.debug_spawn_unit(&rules, P0, home_tile, kind);
    assert_eq!(
        route_status(game.view().auth, &rules, unit, near, RouteKind::International).err(),
        Some(TradeBlock::Embargo),
        "a route to the embargoed nation is forbidden"
    );
    assert!(classic_rules::world::congress::trade_embargoed(game.view().auth, P0, P1));
    assert!(classic_rules::world::congress::trade_embargoed(game.view().auth, P1, P0), "both ways");
    assert!(!classic_rules::world::congress::trade_embargoed(game.view().auth, P0, CS), "the city-state is another matter");
    game.validate(&rules).unwrap();
}

#[test]
fn the_city_state_embargo_forbids_routes_to_city_states_only() {
    let (mut game, rules) = world();
    enact(&mut game, &rules, ResolutionKind::CityStateEmbargo, 0);
    let auth = game.view().auth;
    assert!(classic_rules::world::congress::trade_embargoed(auth, P0, CS));
    assert!(!classic_rules::world::congress::trade_embargoed(auth, P0, P1));
}

#[test]
fn banning_a_luxury_takes_its_happiness_from_everybody() {
    let (mut game, rules) = world();
    let wine = classic_rules::ids::ResourceId(rules.resources.iter().position(|r| r.key == "core:resource/wine").unwrap() as u16);
    let city_tile = game.view().auth.cities().get(game.view().auth.indexes().cities_of(P0)[0]).unwrap().tile;
    game.debug_set_resource_batch(&rules, &[(city_tile, wine)]);
    let luxuries = |g: &GameState| classic_rules::derived::happiness::player_happiness(g.view().auth, &rules, P0).luxuries;
    let with = luxuries(&game);
    assert!(with > 0, "the wine makes the empire happy");
    enact(&mut game, &rules, ResolutionKind::BanLuxury, u32::from(wine.0));
    assert_eq!(luxuries(&game), 0, "banned: no happiness");
    assert!(classic_rules::world::congress::luxury_banned(game.view().auth, wine));
}

#[test]
fn the_funding_resolutions_move_the_great_person_points_of_whole_families() {
    use classic_rules::classic::content::GreatPersonKind as K;
    use classic_rules::world::congress::{funding_percent, FUNDING_PERCENT};
    let (mut game, rules) = world();
    assert_eq!(funding_percent(game.view().auth, K::Writer), 0, "nothing in force");
    enact(&mut game, &rules, ResolutionKind::ArtsFunding, 0);
    let auth = game.view().auth;
    for arts in [K::Writer, K::Artist, K::Musician] {
        assert_eq!(funding_percent(auth, arts), FUNDING_PERCENT);
    }
    for industry in [K::Scientist, K::Engineer, K::Merchant] {
        assert_eq!(funding_percent(auth, industry), -FUNDING_PERCENT);
    }
    for other in [K::General, K::Admiral, K::Prophet] {
        assert_eq!(funding_percent(auth, other), 0);
    }
}

#[test]
fn the_sessions_grow_with_the_eras() {
    assert_eq!(SESSIONS.len(), 3);
    assert!(SESSIONS.windows(2).all(|w| w[1].era > w[0].era && w[1].turns_between < w[0].turns_between));
    assert!(SESSIONS.windows(2).all(|w| w[1].nation_delegates > w[0].nation_delegates));
    assert!(SESSIONS[..2].iter().all(|s| !s.united_nations) && SESSIONS[2].united_nations);
}

/// P0 (the host, 3 delegates) and P1 both propose, and the session opens with two proposals.
fn session_of_two_proposals() -> (GameState, CompiledRuleset) {
    let (mut game, rules) = world();
    game.debug_learn(&rules, P0, tech(&rules, "core:tech/printing-press"));
    end_round(&mut game, &rules);
    for (player, kind) in [(P0, ResolutionKind::ScholarsInResidence), (P1, ResolutionKind::StandingArmyTax)] {
        until_active(&mut game, &rules, player);
        game.execute(&rules, Command::ProposeResolution { player, kind, choice: 0, repeal: false }).unwrap();
    }
    for _ in 0..60 {
        if game.view().auth.module().congress().in_session() {
            break;
        }
        end_round(&mut game, &rules);
    }
    assert_eq!(game.view().auth.module().congress().proposals().len(), 2);
    until_active(&mut game, &rules, P0);
    (game, rules)
}

/// A second resolution in the session must not cost the AI a vote: every delegate is cast (the
/// even split leaves a remainder of one delegate when 3 delegates meet 2 proposals) and each
/// resolution gets a vote.
#[test]
fn the_ai_casts_every_delegate_over_two_resolutions() {
    let (mut game, rules) = session_of_two_proposals();
    let left = game.view().auth.module().congress().delegates_left(P0);
    assert_eq!(left, 3, "the host's seat");
    let orders = vote_orders(&game.ai_observation(&rules, P0));
    let cast: u16 = orders
        .iter()
        .map(|o| match o {
            Command::CastVotes { delegates, .. } => *delegates,
            other => panic!("{other:?}"),
        })
        .sum();
    assert_eq!(cast, left, "{orders:?}");
    assert_eq!(orders.len(), 2, "a vote on each resolution");
    for order in orders {
        game.execute(&rules, order).unwrap();
    }
    assert_eq!(game.view().auth.module().congress().delegates_left(P0), 0);
}

/// Nothing to cast: a nation with no delegates, or no session, gets no order.
#[test]
fn the_ai_without_delegates_or_session_casts_nothing() {
    let (mut game, rules) = session_of_two_proposals();
    let orders = vote_orders(&game.ai_observation(&rules, P0));
    for order in orders {
        game.execute(&rules, order).unwrap();
    }
    assert!(vote_orders(&game.ai_observation(&rules, P0)).is_empty(), "all cast: nothing left");
    for _ in 0..3 {
        end_round(&mut game, &rules);
    }
    assert!(!game.view().auth.module().congress().in_session());
    assert!(vote_orders(&game.ai_observation(&rules, P0)).is_empty(), "no session");
}

/// Two nations and a city-state as in `world()`, with domination off so that the last nation
/// standing does not end the game when the other one is eliminated.
fn world_without_domination() -> (GameState, CompiledRuleset) {
    let (mut game, rules) = world();
    game.debug_set_victory_options(VictoryOptions { disabled: VictoryKind::Domination.bit(), ..VictoryOptions::default() });
    (game, rules)
}

/// `killer` takes the only city of `victim`, next to which it spawns warriors.
fn eliminate(game: &mut GameState, rules: &CompiledRuleset, killer: PlayerId, victim: PlayerId, next_to: (i32, i32)) {
    until_active(game, rules, killer);
    let warrior = UnitKindId(rules.units.iter().position(|u| u.key == "core:unit/warrior").unwrap() as u16);
    game.execute(rules, Command::DeclareWar { player: killer, enemy: victim }).unwrap();
    for _ in 0..8 {
        let Some(city) = game.view().auth.indexes().cities_of(victim).first().copied() else {
            break;
        };
        let attacker = game.debug_spawn_unit(rules, killer, tile(game, next_to.0, next_to.1), warrior);
        let core = game.view().auth.cities().get(city).unwrap();
        let max = classic_rules::combat::city::max_hit_points(game.view().auth, rules, core);
        game.debug_set_city_damage(city, max - 1);
        let _ = game.execute(rules, Command::AttackCity { attacker, city });
    }
    assert!(!game.view().auth.players().is_alive(victim), "{victim:?} is eliminated");
}

/// The league founded by P0 (the host), whose only city P1 takes.
fn league_whose_host_is_eliminated() -> (GameState, CompiledRuleset) {
    let (mut game, rules) = world_without_domination();
    game.debug_learn(&rules, P0, tech(&rules, "core:tech/printing-press"));
    end_round(&mut game, &rules);
    assert_eq!(game.view().auth.module().congress().host(), Some(P0));
    eliminate(&mut game, &rules, P1, P0, (4, 3));
    (game, rules)
}

/// A host who is eliminated hands the league over at the league's next turn (to the living nation with
/// the most delegates), and no dead nation keeps a proposal slot or the tie-break.
#[test]
fn a_host_who_is_eliminated_gives_the_league_to_a_living_nation() {
    let (mut game, rules) = league_whose_host_is_eliminated();
    // The league's turn of the round of the elimination has run already; the next one hands over.
    end_round(&mut game, &rules);
    end_round(&mut game, &rules);
    let congress = game.view().auth.module().congress();
    assert_eq!(congress.host(), Some(P1), "proposers {:?}", congress.proposers());
    assert_eq!(congress.proposers(), &[P1]);
    assert_eq!(classic_rules::world::congress::delegates(game.view().auth, &rules, P1), 3, "the new host's seat");
    for _ in 0..60 {
        end_round(&mut game, &rules);
    }
    let congress = game.view().auth.module().congress();
    assert_eq!(congress.host(), Some(P1));
    assert!(congress.proposers().iter().all(|p| game.view().auth.players().is_alive(*p)), "{:?}", congress.proposers());
}

/// A nation elected host who dies before the tally does not become host, and the vote for a
/// dead World Leader candidate counts for nobody.
#[test]
fn an_elected_host_who_dies_before_the_tally_does_not_become_host() {
    let (mut game, rules) = world_without_domination();
    game.debug_learn(&rules, P0, tech(&rules, "core:tech/printing-press"));
    end_round(&mut game, &rules);
    for i in 0..rules.technologies.len() {
        game.debug_learn(&rules, P0, TechId(i as u16));
    }
    for _ in 0..6 {
        if game.view().auth.module().congress().in_session() {
            break;
        }
        end_round(&mut game, &rules);
    }
    let congress = game.view().auth.module().congress();
    assert_eq!(congress.proposals()[0].kind, ResolutionKind::ChangeHost, "{:?}", congress.proposals());
    until_active(&mut game, &rules, P0);
    let delegates = game.view().auth.module().congress().delegates_left(P0);
    game.execute(&rules, Command::CastVotes { player: P0, proposal: 0, choice: u32::from(P1.0), delegates }).unwrap();
    eliminate(&mut game, &rules, P0, P1, (23, 15));
    for _ in 0..3 {
        end_round(&mut game, &rules);
    }
    let congress = game.view().auth.module().congress();
    assert!(!congress.in_session());
    assert_eq!(congress.host(), Some(P0), "the dead candidate did not win; proposers {:?}", congress.proposers());
}
