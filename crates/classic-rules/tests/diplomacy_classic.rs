//! Diplomacy between nations on the classic ruleset (classic/docs/classic/rules.md): contact, peace treaties, defensive pacts, deals and
//! their durations, research agreements, friendship, denunciation,
//! warmonger memory, the AI's answers, save/load.

use classic_rules::ClassicGame;
use classic_content::core::core_rules;
use classic_rules::ids::{PlayerId, TechId, TileId, UnitKindId};
use classic_rules::math::HexCoord;
use classic_rules::replay::authoritative_hash;
use classic_rules::world::relations::DealItem;
use classic_rules::world::state::GameConfig;
use classic_rules::{compile_rules, Command, CompiledRuleset, GameState};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const P2: PlayerId = PlayerId(2);

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

fn tech(rules: &CompiledRuleset, key: &str) -> TechId {
    TechId(
        rules
            .technologies
            .iter()
            .position(|t| t.key == key)
            .unwrap() as u16,
    )
}

/// Three nations that met (their scouts stand together), each with a
/// city far apart, all knowing the diplomacy technologies.
fn three_nations() -> (GameState, CompiledRuleset) {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(
        &rules,
        GameConfig {
            players: 3,
            width: 24,
            height: 16,
            rng_seed: 3,
        },
    );
    let scout = unit(&rules, "core:unit/scout");
    for (i, p) in [P0, P1, P2].into_iter().enumerate() {
        game.debug_spawn_unit(&rules, p, tile(&game, 10 + i as i32, 8), scout);
        game.debug_found_city(&rules, p, tile(&game, 2 + 9 * i as i32, 2));
        for key in [
            "core:tech/writing",
            "core:tech/civil-service",
            "core:tech/chivalry",
            "core:tech/education",
        ] {
            game.debug_learn(&rules, p, tech(&rules, key));
        }
    }
    (game, rules)
}

fn until_active(game: &mut GameState, rules: &CompiledRuleset, player: PlayerId) {
    for _ in 0..8 {
        let active = game.view().auth.active_player();
        if active == player {
            return;
        }
        game.execute(rules, Command::EndTurn { player: active })
            .unwrap();
    }
    panic!("never active");
}

fn rounds(game: &mut GameState, rules: &CompiledRuleset, n: u32) {
    for _ in 0..n {
        let start = game.view().auth.turn().round;
        while game.view().auth.turn().round == start {
            let active = game.view().auth.active_player();
            game.execute(rules, Command::EndTurn { player: active })
                .unwrap();
        }
    }
    until_active(game, rules, P0);
}

/// Propose from the active player and accept as the receiver.
fn deal(
    game: &mut GameState,
    rules: &CompiledRuleset,
    from: PlayerId,
    to: PlayerId,
    items: Vec<(PlayerId, DealItem)>,
) {
    until_active(game, rules, from);
    let id = game.view().auth.module().relations().next_offer_id();
    game.execute(rules, Command::ProposeDeal { from, to, items })
        .unwrap();
    game.execute(
        rules,
        Command::RespondDeal {
            player: to,
            offer: id,
            accept: true,
        },
    )
    .unwrap();
}

#[test]
fn war_needs_contact_and_peace_blocks_war_for_ten_rounds() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(
        &rules,
        GameConfig {
            players: 2,
            width: 24,
            height: 16,
            rng_seed: 1,
        },
    );
    let warrior = unit(&rules, "core:unit/warrior");
    game.debug_spawn_unit(&rules, P0, tile(&game, 2, 2), warrior);
    game.debug_spawn_unit(&rules, P1, tile(&game, 20, 12), warrior);
    let war = Command::DeclareWar {
        player: P0,
        enemy: P1,
    };
    assert!(game.execute(&rules, war.clone()).is_err(), "not met");
    let (mut game, rules) = three_nations();
    game.execute(&rules, war.clone()).unwrap();
    let peace = Command::MakePeace {
        player: P0,
        enemy: P1,
    };
    assert!(game.execute(&rules, peace.clone()).is_err(), "a young war cannot end");
    rounds(&mut game, &rules, 10);
    game.execute(&rules, peace).unwrap();
    assert!(game.execute(&rules, war.clone()).is_err(), "peace treaty");
    rounds(&mut game, &rules, 10);
    assert!(
        game.execute(&rules, war.clone()).is_err(),
        "still the treaty in round 10"
    );
    rounds(&mut game, &rules, 1);
    game.execute(&rules, war).expect("the treaty is over");
}

#[test]
fn defensive_pact_drags_the_partner_into_war() {
    let (mut game, rules) = three_nations();
    deal(
        &mut game,
        &rules,
        P1,
        P2,
        vec![(P1, DealItem::Embassy), (P2, DealItem::Embassy)],
    );
    deal(
        &mut game,
        &rules,
        P1,
        P2,
        vec![(P1, DealItem::DefensivePact)],
    );
    let round = game.view().auth.turn().round;
    assert!(game
        .view()
        .auth
        .module()
        .relations()
        .has_pact(P1, P2, round));
    until_active(&mut game, &rules, P0);
    game.execute(
        &rules,
        Command::DeclareWar {
            player: P0,
            enemy: P1,
        },
    )
    .unwrap();
    let d = game.view().auth.diplomacy();
    assert!(d.at_war(P0, P1));
    assert!(d.at_war(P2, P0), "the pact partner joins");
    assert!(
        !game.view().auth.module().relations().has_pact(P1, P2, round),
        "joining a war ends the joiner's pacts"
    );
}

#[test]
fn open_borders_need_an_embassy_and_expire() {
    let (mut game, rules) = three_nations();
    until_active(&mut game, &rules, P0);
    let open = Command::ProposeDeal {
        from: P0,
        to: P1,
        items: vec![(P0, DealItem::OpenBorders)],
    };
    assert!(game.execute(&rules, open).is_err(), "no embassy at P1 yet");
    deal(&mut game, &rules, P0, P1, vec![(P1, DealItem::Embassy)]);
    deal(&mut game, &rules, P0, P1, vec![(P0, DealItem::OpenBorders)]);
    assert!(game.view().auth.diplomacy().grants_entry(P0, P1));
    rounds(&mut game, &rules, 31);
    assert!(
        !game.view().auth.diplomacy().grants_entry(P0, P1),
        "30 rounds"
    );
}

#[test]
fn research_agreement_pays_science_at_the_end() {
    let (mut game, rules) = three_nations();
    game.debug_earn(P0, 500);
    game.debug_earn(P1, 500);
    deal(
        &mut game,
        &rules,
        P0,
        P1,
        vec![(P0, DealItem::Embassy), (P1, DealItem::Embassy)],
    );
    let gold_only = Command::ProposeDeal {
        from: P0,
        to: P1,
        items: vec![(P0, DealItem::ResearchAgreement)],
    };
    until_active(&mut game, &rules, P0);
    assert!(game.execute(&rules, gold_only).is_err(), "needs friendship");
    deal(&mut game, &rules, P0, P1, vec![(P0, DealItem::Friendship)]);
    let gold = game.view().auth.players().treasury_of(P0);
    deal(
        &mut game,
        &rules,
        P0,
        P1,
        vec![(P0, DealItem::ResearchAgreement)],
    );
    assert!(
        game.view().auth.players().treasury_of(P0) < gold,
        "both sides pay"
    );
    let science_before = game.view().auth.players().science_of(P1);
    rounds(&mut game, &rules, 29);
    let r = game.view().auth.module().relations();
    assert!(r.research_points(P0, P1) > 0 && r.research_points(P1, P0) > 0);
    let _ = science_before;
    rounds(&mut game, &rules, 2);
    let round = game.view().auth.turn().round;
    assert!(!game
        .view()
        .auth
        .module()
        .relations()
        .has_research_agreement(P0, P1, round));
}

#[test]
fn denouncing_closes_embassies_and_breaks_friendship() {
    let (mut game, rules) = three_nations();
    deal(
        &mut game,
        &rules,
        P0,
        P1,
        vec![(P0, DealItem::Embassy), (P1, DealItem::Embassy)],
    );
    deal(&mut game, &rules, P0, P1, vec![(P0, DealItem::Friendship)]);
    until_active(&mut game, &rules, P0);
    game.execute(
        &rules,
        Command::Denounce {
            player: P0,
            target: P1,
        },
    )
    .unwrap();
    let round = game.view().auth.turn().round;
    let r = game.view().auth.module().relations();
    assert!(r.denounced(P0, P1, round));
    assert!(!r.has_embassy(P0, P1) && !r.has_embassy(P1, P0));
    assert!(!r.are_friends(P0, P1, round));
    assert!(game
        .execute(
            &rules,
            Command::Denounce {
                player: P0,
                target: P1
            }
        )
        .is_err());
}

#[test]
fn declaring_war_is_remembered_and_fades() {
    let (mut game, rules) = three_nations();
    game.execute(
        &rules,
        Command::DeclareWar {
            player: P0,
            enemy: P1,
        },
    )
    .unwrap();
    let r = game.view().auth.module().relations();
    // Medieval era (Chivalry, Education known): 250 × 70 %.
    assert_eq!(r.warmonger(P2, P0), 175);
    assert_eq!(r.warmonger(P1, P0), 175, "the victim remembers too");
    rounds(&mut game, &rules, 3);
    let after = game.view().auth.module().relations().warmonger(P2, P0);
    assert_eq!(after, 175 - 3 * 3, "−5 × 70 % = −3 per turn");
}

#[test]
fn gold_per_turn_flows_every_turn() {
    let (mut game, rules) = three_nations();
    game.debug_earn(P0, 300);
    let income = classic_rules::derived::income::player_income(game.view().auth, &rules, P0).gold;
    assert!(income > 0, "a city with a palace earns gold");
    deal(
        &mut game,
        &rules,
        P0,
        P1,
        vec![(P0, DealItem::GoldPerTurn(1)), (P1, DealItem::Embassy)],
    );
    let before = game.view().auth.players().treasury_of(P1);
    rounds(&mut game, &rules, 2);
    let after = game.view().auth.players().treasury_of(P1);
    let p1_income = classic_rules::derived::income::player_income(game.view().auth, &rules, P1).gold;
    assert!(
        after as i64 >= before as i64 + 2 * (p1_income as i64 + 1) - 4,
        "{before} -> {after}"
    );
}

#[test]
fn ai_exchanges_embassies_and_answers_offers() {
    let (mut game, rules) = three_nations();
    let obs = game.ai_observation(&rules, P0);
    let offers = classic_rules::ai::diplomacy::proposals(&obs, &rules);
    assert!(offers.iter().any(|c| matches!(c, Command::ProposeDeal { items, .. } if items.iter().any(|(_, i)| *i == DealItem::Embassy))));
    for cmd in offers {
        game.execute(&rules, cmd).unwrap();
    }
    for p in [P1, P2] {
        let answers =
            classic_rules::ai::diplomacy::answer_offers(&game.ai_observation(&rules, p), &rules);
        assert!(
            answers
                .iter()
                .all(|c| matches!(c, Command::RespondDeal { accept: true, .. })),
            "{answers:?}"
        );
        for cmd in answers {
            game.execute(&rules, cmd).unwrap();
        }
    }
    let r = game.view().auth.module().relations();
    assert!(r.has_embassy(P0, P1) && r.has_embassy(P1, P0));
}

/// An embassy shows its owner the host's capital and the tiles within two
/// of it as they are now, without keeping them in sight; the host learns
/// nothing of the guest (classic/docs/classic/rules.md).
#[test]
fn an_embassy_reveals_the_hosts_capital_and_its_surroundings() {
    use classic_rules::knowledge::public::PlayerQueries;
    let (mut game, rules) = three_nations();
    let dims = game.view().auth.map().dimensions;
    let capital = tile(&game, 11, 2);
    let home = tile(&game, 2, 2);
    let disc = |center: TileId, radius: i32| classic_rules::math::hex_disc(dims, center, radius).collect::<Vec<_>>();
    let known = |game: &GameState, p: PlayerId, t: TileId| game.player_view(&rules, p).is_known(t);
    let ring3: Vec<TileId> = disc(capital, 3).into_iter().filter(|t| dims.distance(capital, *t) == Some(3)).collect();
    let unknown_ring3: Vec<TileId> = ring3.into_iter().filter(|t| !known(&game, P0, *t)).collect();
    assert!(!known(&game, P0, capital) && !unknown_ring3.is_empty());
    assert!(!known(&game, P1, home));
    let race = |game: &GameState| game.player_view(&rules, P0).race().capitals.iter().find(|c| c.holder == P1).and_then(|c| c.tile);
    assert_eq!(race(&game), None);

    deal(&mut game, &rules, P0, P1, vec![(P1, DealItem::Embassy)]);

    for t in disc(capital, 2) {
        assert!(known(&game, P0, t), "{t:?} within two of the capital");
    }
    for t in unknown_ring3 {
        assert!(!known(&game, P0, t), "{t:?} is farther");
    }
    let view = game.player_view(&rules, P0);
    let seen = view.tile(capital);
    assert!(!seen.is_visible(), "remembered, not in sight");
    let snapshot = seen.snapshot().unwrap();
    assert!(snapshot.city && snapshot.owner == Some(P1));
    assert_eq!(race(&game), Some(capital), "the victory screen's capital is found");
    assert!(!known(&game, P1, home), "the host learns nothing");
}

/// The revealed tiles are the player's memory of the map: a save keeps
/// them, and the save format is the one before embassies revealed tiles.
#[test]
fn revealed_tiles_survive_save_and_load() {
    let (mut game, rules) = three_nations();
    deal(&mut game, &rules, P0, P1, vec![(P1, DealItem::Embassy)]);
    let capital = tile(&game, 11, 2);
    let save = classic_rules::persistence::encode::encode(&game, &rules);
    let bytes = classic_rules::save_codec::to_bytes(&save);
    let loaded = classic_rules::persistence::decode::decode(&classic_rules::save_codec::from_bytes(&bytes).unwrap(), &rules).unwrap();
    assert_eq!(authoritative_hash(&loaded), authoritative_hash(&game));
    assert!(loaded.player_view(&rules, P0).is_known(capital));
    assert_eq!(classic_rules::save_codec::CODEC_VERSION, 23, "the game's speed is the last codec change");
}

/// The leaders ask a nation whose capital they value and have not
/// found for its embassy even when they distrust it (the advisor's own
/// exchange waits for trust): in a duel the rival's capital is the last
/// one, its capture wins the game. So does the game's AI (`standard`, a
/// leader); the AI before the leaders (`allocator`) does not.
#[test]
fn leaders_ask_for_the_embassy_that_finds_the_last_rival_capital() {
    use classic_rules::ai::driver::ClassicMind;
    use classic_rules::ai::trace::Outcome;
    use classic_rules::ai::variant::Variant;
    use classic_rules::ai::{AiDriver, AiSettings};
    let asks = |variant: Variant| {
        let rules = compile_rules(core_rules()).unwrap();
        let mut game = GameState::new(&rules, GameConfig { players: 2, width: 40, height: 16, rng_seed: 3 });
        let grassland = classic_rules::ids::TerrainId(1);
        let land: Vec<_> = (0..game.view().auth.map().tile_count() as u32).map(|t| (TileId(t), grassland)).collect();
        game.debug_set_terrain_batch(&rules, &land);
        game.debug_found_city(&rules, P0, tile(&game, 3, 8));
        let far = tile(&game, 34, 8);
        game.debug_found_city(&rules, P1, far);
        game.debug_meet(P0, P1);
        for p in [P0, P1] {
            game.debug_learn(&rules, p, tech(&rules, "core:tech/writing"));
        }
        until_active(&mut game, &rules, P1);
        game.execute(&rules, Command::Denounce { player: P1, target: P0 }).unwrap();
        assert!(!game.player_view(&rules, P0).is_known(far));
        let settings = AiSettings::for_rules(&rules, rules.default_difficulty());
        let mut driver = AiDriver::with_mind(settings, vec![P1], ClassicMind::new(variant));
        driver.set_trace_capacity(100_000);
        while game.view().auth.turn().round < 12 {
            let active = game.view().auth.active_player();
            if active == P0 {
                driver.run(&mut game, &rules, u32::MAX);
            } else {
                game.execute(&rules, Command::EndTurn { player: active }).unwrap();
            }
        }
        let asked = driver.trace().entries().filter(|e| e.outcome == Outcome::Applied).any(|e| {
            matches!(&e.command, Some(Command::ProposeDeal { from: P0, to: P1, items }) if items.contains(&(P1, DealItem::Embassy)))
        });
        asked
    };
    assert!(asks(Variant::Leaders), "the leader asks for the embassy");
    assert!(asks(Variant::Standard), "the game's AI is a leader");
    assert!(!asks(Variant::Allocator), "the allocator waits for trust");
}

#[test]
fn relations_survive_save_and_load() {
    let (mut game, rules) = three_nations();
    deal(
        &mut game,
        &rules,
        P0,
        P1,
        vec![(P0, DealItem::Embassy), (P1, DealItem::Embassy)],
    );
    until_active(&mut game, &rules, P0);
    game.execute(
        &rules,
        Command::DeclareWar {
            player: P0,
            enemy: P2,
        },
    )
    .unwrap();
    game.execute(
        &rules,
        Command::ProposeDeal {
            from: P0,
            to: P1,
            items: vec![(P0, DealItem::Friendship)],
        },
    )
    .unwrap();
    let save = classic_rules::persistence::encode::encode(&game, &rules);
    let loaded = classic_rules::persistence::decode::decode(&save, &rules).unwrap();
    assert_eq!(authoritative_hash(&loaded), authoritative_hash(&game));
    assert_eq!(loaded.view().auth.module().relations().offers().len(), 1);
}

/// Regression: luxury happiness counted only the luxuries on the player's
/// own tiles; the rules count what the player has available, imports
/// included and exports excluded.
#[test]
fn a_traded_luxury_moves_its_happiness_to_the_receiver() {
    let (mut game, rules) = three_nations();
    let wine = classic_rules::ids::ResourceId(rules.resources.iter().position(|r| r.key == "core:resource/wine").unwrap() as u16);
    let p1_city = tile(&game, 11, 2);
    game.debug_set_resource_batch(&rules, &[(p1_city, wine)]);
    for p in [P0, P1] {
        game.debug_learn(&rules, p, tech(&rules, "core:tech/calendar"));
    }
    let luxuries = |game: &GameState, p| classic_rules::derived::happiness::player_happiness(game.view().auth, &rules, p).luxuries;
    let (p0, p1) = (luxuries(&game, P0), luxuries(&game, P1));
    assert!(p1 > p0, "P1 has wine");
    deal(&mut game, &rules, P1, P0, vec![(P1, DealItem::Resource { resource: wine, amount: 1 })]);
    assert_eq!(luxuries(&game, P0), p1, "the importer gets the wine's happiness");
    assert_eq!(luxuries(&game, P1), p0, "the exporter of its only copy loses it");
    game.validate(&rules).unwrap();
}
