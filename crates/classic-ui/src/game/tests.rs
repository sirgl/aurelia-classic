//! Headless scripted games through the session API the window uses (no GPU, no window).
use classic_rules::ClassicGame;
use client::selection::Selection;
use classic_rules::ids::{PlayerId, ProductionItem, TileId};

use super::{MapButton, Session};
use crate::ui::model::{Action, Availability, GameRequest, NoticeKind, Outcome, TechStatus};
use crate::ui::screens::{game_rules, prepare_match, MapSize, MatchRequest};

fn new_match(seed: u64) -> Session {
    let request = MatchRequest {
        nation: "core:nation/rome".into(),
        seed,
        size: MapSize::Small,
        players: 3,
        water: 25,
    };
    let prepared = prepare_match(request).expect("world");
    Session::new(prepared.game, game_rules())
}

fn action_state(session: &mut Session, action: Action) -> Availability {
    let orders = session.unit_orders().expect("a selected unit");
    let state = orders.actions.iter().find(|a| a.action == action);
    state.expect("action offered").availability.clone()
}

fn found_capital(session: &mut Session) {
    assert_eq!(
        action_state(session, Action::FoundCity),
        Availability::Ready
    );
    session.request(GameRequest::Unit(Action::FoundCity));
    assert!(
        matches!(session.selection(), Selection::City(_)),
        "new city is selected"
    );
}

fn enqueue(session: &mut Session, name: &str) {
    let city = session.city_view().expect("city screen data");
    let option = city
        .options
        .iter()
        .find(|o| o.name == name)
        .expect("option");
    assert_eq!(option.availability, Availability::Ready, "{name}");
    session.request(GameRequest::Enqueue(option.item));
    let city = session.city_view().unwrap();
    assert_eq!(city.queue.last().map(|q| q.name.as_str()), Some(name));
}

fn research_first_available(session: &mut Session) -> String {
    let research = session.research_view();
    let card = research
        .techs
        .iter()
        .find(|t| t.status == TechStatus::Available)
        .expect("an available technology")
        .clone();
    session.request(GameRequest::Research(card.id));
    let status = session.research_view().techs[card.id.0 as usize]
        .status
        .clone();
    assert_eq!(status, TechStatus::Researching);
    card.name
}

#[test]
fn scripted_match_founds_builds_researches_and_survives_save_and_load() {
    let mut session = new_match(20260928);
    assert!(
        session.selection().selected_unit().is_some(),
        "settler selected at start"
    );
    found_capital(&mut session);
    enqueue(&mut session, "Warrior");
    let tech = research_first_available(&mut session);
    let start_turn = session.turn();
    let mut seen = Vec::new();
    for _ in 0..30 {
        session.request(GameRequest::EndTurn);
        session.game().validate(session.rules()).unwrap();
        for notice in session.hud_model(None).notices {
            if notice.turn == session.turn() {
                seen.push(notice.text);
            }
        }
    }
    assert_eq!(session.turn(), start_turn + 30, "one round per end turn");
    assert!(seen.iter().any(|t| t == "Warrior trained"), "{seen:?}");
    assert!(
        seen.iter().any(|t| *t == format!("Researched {tech}")),
        "{seen:?}"
    );
    let bytes = session.save_bytes();
    let mut loaded = new_match(1);
    loaded.load_bytes(&bytes).unwrap();
    assert_eq!(loaded.world_hash(), session.world_hash());
    assert_eq!(loaded.turn(), session.turn());
    for _ in 0..3 {
        session.request(GameRequest::EndTurn);
        loaded.request(GameRequest::EndTurn);
    }
    assert_eq!(
        loaded.world_hash(),
        session.world_hash(),
        "continuation is deterministic"
    );
}

/// The same rounds with the AI turns on a worker thread (the desktop's way): orders wait
/// meanwhile, the game, its save (AI memory included) and the notices are the sliced
/// path's, and the host's wake is called.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn ai_turns_on_a_worker_play_the_sliced_game_and_hold_orders_back() {
    use super::AiProgress;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;
    let mut sliced = new_match(20260928);
    let mut worker = new_match(20260928);
    let wakes = Arc::new(AtomicUsize::new(0));
    let counter = wakes.clone();
    let wake: client::ai_round::AiWake = Arc::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    });
    worker.use_ai_worker(&wake);
    found_capital(&mut sliced);
    found_capital(&mut worker);
    for round in 0..8 {
        let turn = worker.turn();
        sliced.begin_end_turn();
        sliced.finish_ai_turns();
        worker.begin_end_turn();
        let mut frames = 0;
        loop {
            assert!(worker.ai_busy());
            assert!(!worker.hud_model(None).can_end_turn);
            worker.request(GameRequest::Unit(Action::Fortify));
            assert_eq!(worker.hud_model(None).message.as_deref(), Some(super::ai_turns_message()));
            frames += 1;
            match worker.tick_ai(Duration::from_millis(3)) {
                AiProgress::Busy => std::thread::sleep(Duration::from_micros(200)),
                AiProgress::Finished => break,
                AiProgress::Idle => panic!("the AI turns ended without a report"),
            }
        }
        assert!(!worker.ai_busy());
        assert_eq!(worker.tick_ai(Duration::ZERO), AiProgress::Idle);
        assert!(frames > 1, "the worker took several frames");
        assert_eq!(worker.turn(), turn + 1);
        assert_eq!(worker.world_hash(), sliced.world_hash(), "round {round}");
        assert!(worker.save_bytes() == sliced.save_bytes(), "round {round}: the saves differ");
    }
    assert!(wakes.load(Ordering::SeqCst) >= 1);
    assert!(worker.hud_model(None).can_end_turn);
}

/// PLAN M6 stage C in the game: End Turn starts the AI turns, every frame advances them
/// by a small slice, orders wait meanwhile (selection still works), and the round ends
/// exactly as when it runs at once.
#[test]
fn ai_turns_run_in_frame_slices_and_hold_orders_back() {
    use super::AiProgress;
    use std::time::Duration;
    let mut sliced = new_match(20260928);
    let mut whole = new_match(20260928);
    found_capital(&mut sliced);
    found_capital(&mut whole);
    for _ in 0..8 {
        let turn = sliced.turn();
        sliced.begin_end_turn();
        let mut frames = 0;
        loop {
            assert!(sliced.ai_busy());
            assert!(!sliced.hud_model(None).can_end_turn);
            sliced.request(GameRequest::Unit(Action::Fortify));
            assert_eq!(
                sliced.hud_model(None).message.as_deref(),
                Some(super::ai_turns_message())
            );
            frames += 1;
            match sliced.tick_ai(Duration::ZERO) {
                AiProgress::Busy => {}
                AiProgress::Finished => break,
                AiProgress::Idle => panic!("the AI turns ended without a report"),
            }
        }
        assert!(!sliced.ai_busy());
        assert_eq!(sliced.tick_ai(Duration::ZERO), AiProgress::Idle);
        assert!(frames > 1, "the AI turns took several frames");
        assert_eq!(sliced.turn(), turn + 1);
        whole.request(GameRequest::EndTurn);
        assert_eq!(sliced.world_hash(), whole.world_hash());
    }
    assert!(sliced.hud_model(None).can_end_turn);
}

#[test]
fn blocked_orders_explain_why() {
    let mut session = new_match(7);
    let blocked = action_state(&mut session, Action::Move);
    assert_eq!(blocked, Availability::Ready);
    found_capital(&mut session);
    let city = session.city_view().unwrap();
    let locked = city.options.iter().find(|o| o.name == "Pikeman").unwrap();
    assert!(matches!(&locked.availability, Availability::Blocked(r) if r.starts_with("Needs technology: ")));
    // Rome's Legion replaces the Swordsman: the regular unit is on the list, refused with its reason.
    let replaced = city.options.iter().find(|o| o.name == "Swordsman").unwrap();
    assert_eq!(replaced.availability, Availability::Blocked("Replaced by your unique unit".into()));
    assert_eq!(city.hurry.1, Availability::Blocked("The queue is empty".into()));
    let research = session.research_view();
    let locked = research.techs.iter().find(|t| t.name == "Writing").unwrap();
    assert!(matches!(&locked.status, TechStatus::Locked(text) if text == "Requires Pottery"));
    assert_eq!(locked.availability, Availability::Ready);
    assert_eq!(locked.plan.last(), Some(&(locked.id.0 as usize)));
}

#[test]
fn rejected_move_shows_a_message_and_changes_nothing() {
    let mut session = new_match(7);
    let hash = session.world_hash();
    let revision = session.view().revision();
    session.click(Some(TileId(u32::MAX - 1)), MapButton::Primary);
    assert_eq!(session.world_hash(), hash);
    assert_eq!(session.view().revision(), revision);
    assert!(session.hud_model(None).message.is_some());
}

#[test]
fn commands_that_change_no_picture_keep_the_render_revision() {
    let mut session = new_match(7);
    found_capital(&mut session);
    let revision = session.view().revision();
    enqueue(&mut session, "Warrior");
    research_first_available(&mut session);
    assert_eq!(
        session.view().revision(),
        revision,
        "no terrain rebuild for menus"
    );
}

#[test]
fn route_preview_needs_explored_land_and_unit_moves_along_it() {
    let mut session = new_match(20260928);
    let (unit, from) = selected(&session);
    let view = session.view().clone();
    let unknown = (0..view.explored.len() as u32).find(|t| view.explored[*t as usize] == 0);
    assert!(
        session.path_preview(unknown).is_none(),
        "no route into unknown land"
    );
    let target = reachable_neighbor(&mut session, from).expect("a reachable tile");
    let route = session.path_preview(Some(target)).expect("route");
    assert_eq!(route.tiles.first(), Some(&from));
    session.click(Some(TileId(target)), MapButton::Primary);
    let now = session
        .game()
        .view()
        .auth
        .units()
        .get(unit)
        .unwrap()
        .core
        .tile
        .0;
    assert_eq!(now, target);
}

#[test]
fn skip_and_sleep_hand_selection_to_nothing_when_all_units_rest() {
    let mut session = new_match(7);
    // A start has a settler and a warrior; each sleep hands selection on.
    let units = session.idle_units().len();
    assert!(units >= 2, "BNW starts with a settler and a warrior");
    for _ in 0..units {
        session.request(GameRequest::Unit(Action::Sleep));
    }
    assert_eq!(session.selection(), Selection::None);
    assert!(session.idle_units().is_empty());
    session.request(GameRequest::EndTurn);
    assert!(
        session.idle_units().is_empty(),
        "sleep lasts until the unit is selected"
    );
}

#[test]
fn defeat_is_reported_when_the_player_has_nothing_left() {
    let mut session = new_match(7);
    assert_eq!(session.outcome(), Outcome::Playing);
    let rules = game_rules();
    let mut game = session.game().clone();
    let own = game.view().auth.indexes().units_of(PlayerId(0)).to_vec();
    let tile = game.view().auth.units().get(own[0]).unwrap().core.tile;
    // P1 acts on its own turn (sequential turns).
    while game.view().auth.active_player() != PlayerId(1) {
        let player = game.view().auth.active_player();
        game.execute(&rules, classic_rules::Command::EndTurn { player })
            .unwrap();
    }
    let war = classic_rules::Command::DeclareWar {
        player: PlayerId(1),
        enemy: PlayerId(0),
    };
    game.debug_meet(PlayerId(1), PlayerId(0));
    game.execute(&rules, war).unwrap();
    let dims = game.view().auth.map().dimensions;
    let beside = classic_rules::math::tiles_in_hex_radius(&dims, tile, 1)
        .into_iter()
        .find(|t| {
            *t != tile
                && classic_rules::navigation::cost::enter_cost_on_map(game.view().auth.map(), *t)
                    .is_some()
        })
        .expect("land next to the starting units");
    let warrior = rules
        .units
        .iter()
        .position(|u| u.key == "core:unit/warrior")
        .expect("warrior in the ruleset");
    let kind = classic_rules::ids::UnitKindId(warrior as u16);
    for target in own.iter().copied() {
        for _ in 0..40 {
            if game.view().auth.units().get(target).is_none() {
                break;
            }
            let attacker = game.debug_spawn_unit(&rules, PlayerId(1), beside, kind);
            let strike = classic_rules::Command::AttackUnit {
                attacker,
                defender: target,
            };
            game.execute(&rules, strike).unwrap();
        }
        assert!(
            game.view().auth.units().get(target).is_none(),
            "starting unit destroyed"
        );
    }
    session.replace_game(game);
    assert_eq!(session.outcome(), Outcome::Defeat);
    session.request(GameRequest::EndTurn);
    assert_eq!(session.hud_model(None).can_end_turn, false);
}

fn selected(session: &Session) -> (classic_rules::ids::UnitHandle, u32) {
    let unit = session.selection().selected_unit().unwrap();
    let tile = session
        .game()
        .view()
        .auth
        .units()
        .get(unit)
        .unwrap()
        .core
        .tile
        .0;
    (unit, tile)
}

fn reachable_neighbor(session: &mut Session, from: u32) -> Option<u32> {
    let width = session.view().width;
    let candidates = [
        from + 1,
        from.wrapping_sub(1),
        from + width,
        from.wrapping_sub(width),
    ];
    candidates.into_iter().find(|t| {
        let route = session.path_preview(Some(*t));
        matches!(route, Some(r) if r.this_turn == r.tiles.len())
    })
}

#[test]
fn enqueue_requests_reach_the_core_queue() {
    let mut session = new_match(7);
    found_capital(&mut session);
    let Selection::City(city) = session.selection() else {
        unreachable!()
    };
    let warrior = session
        .city_view()
        .unwrap()
        .options
        .iter()
        .find(|o| o.name == "Warrior")
        .unwrap()
        .item;
    session.request(GameRequest::Enqueue(warrior));
    let queue = session
        .game()
        .view()
        .auth
        .cities()
        .get(city)
        .unwrap()
        .queue
        .clone();
    assert!(matches!(queue.as_slice(), [ProductionItem::Unit(_)]));
    session.request(GameRequest::ClearQueue);
    assert!(session
        .game()
        .view()
        .auth
        .cities()
        .get(city)
        .unwrap()
        .queue
        .is_empty());
}

#[test]
fn diplomacy_lists_only_known_rivals_and_makes_peace() {
    let mut session = new_match(7);
    let unseen: Vec<u8> = session.rivals().iter().map(|r| r.player).collect();
    assert!(unseen.is_empty(), "no contact at the start: {unseen:?}");
    let mut game = session.game().clone();
    // The human is the active player: the war is declared on its turn.
    let war = classic_rules::Command::DeclareWar {
        player: PlayerId(0),
        enemy: PlayerId(2),
    };
    game.debug_meet(PlayerId(0), PlayerId(2));
    game.execute(&game_rules(), war).unwrap();
    session.replace_game(game);
    let rivals = session.rivals();
    assert_eq!(rivals.len(), 1);
    assert!(rivals[0].at_war && matches!(rivals[0].peace, Availability::Blocked(_)), "a young war cannot end");
    let mut game = session.game().clone();
    for _ in 0..classic_rules::MIN_WAR_ROUNDS_BEFORE_PEACE {
        for player in 0..3 {
            game.execute(&game_rules(), classic_rules::Command::EndTurn { player: PlayerId(player) }).unwrap();
        }
    }
    session.replace_game(game);
    let rivals = session.rivals();
    assert!(rivals[0].at_war && rivals[0].peace == Availability::Ready);
    session.request(GameRequest::MakePeace(2));
    let auth = session.game().view().auth;
    assert!(!auth.diplomacy().at_war(PlayerId(0), PlayerId(2)));
}

/// The Civics screen shows the real branches and costs; unlocking a branch through the
/// screen's request spends culture and opens its tree.
#[test]
fn civics_screen_unlocks_a_branch_with_the_culture_pool() {
    let request = MatchRequest {
        nation: "core:nation/rome".into(),
        seed: 20260928,
        size: MapSize::Small,
        players: 3,
        water: 25,
    };
    let mut prepared = prepare_match(request).expect("world");
    prepared.game.debug_grant_culture(PlayerId(0), 30);
    let mut session = Session::new(prepared.game, game_rules());
    found_capital(&mut session);
    let view = session.civics_view();
    assert_eq!(
        view.branches.len(),
        12,
        "nine branches and three ideologies"
    );
    assert_eq!(view.branches[0].name, "Tradition", "rule set order");
    assert_eq!(view.next_cost, 25);
    let tradition = &view.branches[0];
    assert_eq!(tradition.state, crate::ui::model::BranchState::Available);
    let freedom = view.branches.iter().find(|b| b.name == "Freedom").unwrap();
    assert!(matches!(
        freedom.state,
        crate::ui::model::BranchState::Locked(_)
    ));
    session.request(GameRequest::UnlockBranch(tradition.id));
    let view = session.civics_view();
    assert_eq!(view.branches[0].state, crate::ui::model::BranchState::Open);
    assert_eq!(view.culture, 5);
    assert_eq!(view.next_cost, 30, "the opener counts");
    let aristocracy = view.branches[0]
        .policies
        .iter()
        .find(|p| p.name == "Aristocracy")
        .unwrap();
    assert!(
        matches!(aristocracy.availability, Availability::Blocked(_)),
        "5 < 30 culture"
    );
}

/// The Religion screen offers the pantheon pickers with the real price; founding one through
/// the screen's request spends faith and shows the pantheon.
#[test]
fn religion_screen_founds_a_pantheon_with_the_faith_pool() {
    let request = MatchRequest {
        nation: "core:nation/rome".into(),
        seed: 20260928,
        size: MapSize::Small,
        players: 3,
        water: 25,
    };
    let mut prepared = prepare_match(request).expect("world");
    prepared.game.debug_grant_faith(PlayerId(0), 12);
    let mut session = Session::new(prepared.game, game_rules());
    found_capital(&mut session);
    let view = session.religion_view();
    assert_eq!(view.pantheon_cost, 10);
    assert_eq!(
        view.choice,
        crate::ui::model::FaithChoice::Pantheon(Availability::Ready)
    );
    assert!(view.pantheons.len() > 20, "every pantheon belief is free");
    let belief = view.pantheons[0].id;
    session.request(GameRequest::FoundPantheon(belief));
    let view = session.religion_view();
    assert_eq!(view.faith, 2);
    assert_eq!(view.pantheon.map(|p| p.id), Some(belief));
    assert!(view.pantheons.iter().all(|p| p.id != belief), "taken");
}

#[test]
fn culture_screen_shows_a_writers_work_and_the_great_person_action() {
    let request = MatchRequest {
        nation: "core:nation/rome".into(),
        seed: 20260928,
        size: MapSize::Small,
        players: 3,
        water: 25,
    };
    let prepared = prepare_match(request).expect("world");
    let mut session = Session::new(prepared.game, game_rules());
    found_capital(&mut session);
    let (game, rules) = session.harness_game();
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
    let unit = game.debug_spawn_unit(
        &rules,
        PlayerId(0),
        tile,
        classic_rules::ids::UnitKindId(writer as u16),
    );
    session.refresh_after_harness();
    session.client.selection = client::selection::Selection::Unit(unit);
    let orders = session.unit_orders().expect("the writer's orders");
    let great_work = orders
        .actions
        .iter()
        .find(|a| a.action == crate::ui::model::Action::GreatWork);
    assert_eq!(
        great_work.map(|a| a.availability.clone()),
        Some(Availability::Ready)
    );
    session.order(crate::ui::model::Action::GreatWork);
    let view = session.culture_view();
    let amphitheater = view
        .buildings
        .iter()
        .find(|b| b.building == "Amphitheater")
        .expect("listed");
    let work = amphitheater.works[0].as_ref().expect("real work in its slot");
    assert_eq!(work.class, "Writing");
    assert!(work.art.as_ref().is_some_and(|art| art.starts_with("art:great-work/")));
    assert!(!work.provenance.is_empty());
    let saved_work = work.clone();
    let mut restored = Session::from_save(&session.save_bytes(), game_rules()).expect("round trip");
    let restored_view = restored.culture_view();
    let restored_work = restored_view.buildings.iter().flat_map(|b| b.works.iter().flatten())
        .find(|work| work.id == saved_work.id).expect("same work after load");
    assert_eq!(restored_work, &saved_work, "art and provenance follow stable work identity");
    assert_eq!(view.tourism, 2);
}

fn small_match(seed: u64, players: u8) -> classic_rules::world::state::GameState {
    let request = MatchRequest {
        nation: "core:nation/rome".into(),
        seed,
        size: MapSize::Small,
        players,
        water: 25,
    };
    prepare_match(request).expect("world").game
}

#[test]
fn a_new_match_plays_at_its_difficulty_from_the_first_turn() {
    let mut session = Session::with_difficulty(small_match(3, 3), game_rules(), "core:handicap/deity");
    let settings = session.game().view().auth.settings().clone();
    assert_eq!(settings.difficulty, "core:handicap/deity", "before any AI turn");
    assert_eq!(settings.ai, vec![false, true, true], "the human is player 0");
    session.replace_game_with(small_match(4, 2), "core:handicap/settler");
    let settings = session.game().view().auth.settings();
    assert_eq!(settings.difficulty, "core:handicap/settler");
    assert_eq!(settings.ai, vec![false, true]);
}

#[test]
fn the_production_list_shows_only_own_unique_units_and_buildings() {
    let mut session = new_match(5);
    found_capital(&mut session);
    let city = session.city_view().expect("city screen data");
    let names: Vec<&str> = city.options.iter().map(|o| o.name.as_str()).collect();
    assert!(names.contains(&"Legion"), "Rome's own unique unit: {names:?}");
    assert!(!names.contains(&"Hoplite"), "Greece's unique unit is not Rome's");
}

fn key_index(keys: impl Iterator<Item = String>, key: &str) -> u16 {
    keys.into_iter().position(|k| k == key).unwrap() as u16
}

#[test]
fn resources_show_in_the_top_bar_the_overview_and_the_notices() {
    let mut session = new_match(6);
    found_capital(&mut session);
    let me = session.me();
    let (game, rules) = session.harness_game();
    let auth = game.view().auth;
    let capital = auth.cities().iter().find(|(_, c)| c.owner == me).map(|(_, c)| c.tile).unwrap();
    let near = auth
        .map()
        .dimensions
        .neighbors(capital)
        .into_iter()
        .find(|t| auth.map().owner(*t).flatten() == Some(me))
        .unwrap();
    let iron = classic_rules::ids::ResourceId(key_index(rules.resources.iter().map(|r| r.key.clone()), "core:resource/iron"));
    let mine = classic_rules::ids::ImprovementId(key_index(
        rules.improvements.iter().map(|r| r.key.clone()),
        "core:improvement/mine",
    ));
    let bronze = classic_rules::ids::TechId(key_index(
        rules.technologies.iter().map(|r| r.key.clone()),
        "core:tech/iron-working",
    ));
    game.debug_set_resource_batch(&rules, &[(near, iron)]);
    game.debug_set_improvement_batch(&rules, &[(near, mine)]);
    game.debug_learn(&rules, me, bronze);
    session.refresh_after_harness();
    session.report_resources(&[]);
    assert!(session.economy().resources.ends_with("Iron"), "{}", session.economy().resources);
    let row = session.overview_view().resources.into_iter().find(|r| r.name == "Iron").unwrap();
    assert_eq!((row.class, row.used), ("Strategic", 0));
    assert!(row.available > 0);
    let before = session.resource_marks();
    let (game, rules) = session.harness_game();
    let sword = classic_rules::ids::UnitKindId(key_index(rules.units.iter().map(|u| u.key.clone()), "core:unit/swordsman"));
    for _ in 0..=row.total {
        game.debug_spawn_unit(&rules, me, near, sword);
    }
    session.refresh_after_harness();
    session.report_resources(&before);
    let texts: Vec<String> = session.overview_view().notices.iter().map(|n| n.text.clone()).collect();
    assert!(texts.iter().any(|t| t.starts_with("Iron connected")), "{texts:?}");
    assert!(texts.iter().any(|t| t.starts_with("Not enough Iron")), "{texts:?}");
}

/// The trade panel's draft reaches the core as one offer to the partner.
#[test]
fn deal_draft_becomes_an_offer_of_the_core() {
    let mut session = new_match(7);
    found_capital(&mut session);
    let (game, _) = session.harness_game();
    game.debug_meet_all();
    game.debug_earn(PlayerId(0), 300);
    session.refresh_after_harness();
    let view = session.diplomacy_view();
    let card = view.nations.first().expect("a met nation");
    let ready = card.ours.iter().chain(&card.mutual).find(|o| o.availability.is_ready());
    let item = ready.expect("an item the rules allow now").item;
    let mut draft = crate::ui::model::DealDraft::new(card.player);
    draft.toggle(PlayerId(0), item);
    let hash = session.world_hash();
    assert!(session.preview_deal(&draft).is_ready());
    assert_eq!(session.world_hash(), hash, "preview never changes the world");
    let mut invalid = draft.clone();
    invalid.toggle(PlayerId(0), classic_rules::world::relations::DealItem::Gold(u32::MAX));
    assert!(!session.preview_deal(&invalid).is_ready());
    assert_eq!(session.world_hash(), hash, "invalid preview is also read-only");
    session.request(draft.request().expect("a proposal"));
    let offers = session.game().view().auth.module().relations().offers().to_vec();
    assert_eq!(offers.len(), 1, "{:?}", session.hud_model(None).message);
    assert_eq!(offers[0].to, card.player);
    assert_eq!(offers[0].items, draft.items, "gold items carry the draft's amounts");
}

/// The city-state panel lists met city-states with influence and three gold gifts.
#[test]
fn city_state_panel_shows_influence_and_gifts() {
    let rules = game_rules();
    let config = classic_rules::scenario::ScenarioConfig {
        seed: 7,
        players: 2,
        width: 32,
        height: 24,
        cities: 0,
        units: 0,
        water_ratio: 25,
    };
    let options = classic_rules::scenario::NewGameOptions {
        city_states: 2,
        ..Default::default()
    };
    let mut game = classic_rules::scenario::generate_new_game_with(&rules, &config, Some(0), options);
    let minor = PlayerId(2);
    game.debug_meet(PlayerId(0), minor);
    game.debug_set_influence(&rules, minor, PlayerId(0), 40);
    game.debug_earn(PlayerId(0), 300);
    let mut session = Session::new(game, rules);
    let view = session.city_states_view();
    let card = view.cards.iter().find(|c| c.player == minor).expect("met city-state");
    assert_eq!(card.influence, 40);
    assert_eq!(card.status, "Friends");
    let gifts: Vec<u32> = card.gifts.iter().map(|g| g.gold).collect();
    assert_eq!(gifts, vec![250, 500, 1000]);
    assert_eq!(card.gifts[0].availability, Availability::Ready);
    assert!(!card.gifts[1].availability.is_ready(), "300 gold only");
    assert!(card.gifts[0].influence > 0);
}

/// Victory progress has the five kinds; a time victory ends the game, writes a Hall
/// of Fame line, and "one more turn" lets the game go on.
#[test]
fn time_victory_ends_the_game_and_one_more_turn_continues() {
    let mut session = new_match(7);
    found_capital(&mut session);
    let kinds: Vec<&str> = session.victory_view().rows.iter().map(|r| r.kind).collect();
    assert_eq!(kinds, vec!["Domination", "Science", "Culture", "Diplomacy", "Time"]);
    let (game, _) = session.harness_game();
    let options = classic_rules::world::victory::VictoryOptions {
        disabled: 0,
        max_rounds: 11,
    };
    game.debug_set_victory_options(options);
    for _ in 0..12 {
        session.request(GameRequest::EndTurn);
    }
    assert!(session.game().is_over());
    assert_ne!(session.outcome(), Outcome::Playing);
    let end = session.game_end_view(&[]);
    assert!(end.can_continue);
    assert!(end.line.contains("Time victory"), "{}", end.line);
    assert!(session.hall_of_fame_line().contains("turn"));
    session.request(GameRequest::ContinueAfterVictory);
    assert_eq!(session.outcome(), Outcome::Playing);
    // The window advances AI turns every frame; here at once.
    session.finish_ai_turns();
    let turn = session.turn();
    session.request(GameRequest::EndTurn);
    assert_eq!(session.hud_model(None).message, None);
    assert_eq!(session.turn(), turn + 1, "the rounds go on");
}

/// A game from the setup with city-states and barbarians plays its AI turns.
#[test]
fn setup_game_with_city_states_and_barbarians_plays_on() {
    use crate::ui::screens::{prepare_game, GameOptions, NewGame};
    let new_game = NewGame {
        request: MatchRequest {
            nation: "core:nation/rome".into(),
            seed: 11,
            size: MapSize::Small,
            players: 3,
            water: 25,
        },
        options: GameOptions {
            city_states: 4,
            barbarians: true,
            ..GameOptions::default()
        },
    };
    let prepared = prepare_game(new_game).expect("world");
    let players = prepared.game.view().auth.players();
    assert_eq!(players.count(), 3 + 4 + 1, "nations, city-states, barbarians");
    let mut session = Session::new(prepared.game, game_rules());
    found_capital(&mut session);
    for _ in 0..30 {
        session.request(GameRequest::EndTurn);
    }
    assert!(session.turn() >= 30, "turn {}", session.turn());
}

#[test]
fn the_trade_panel_lists_offers_and_starts_a_route() {
    let mut session = new_match(8);
    found_capital(&mut session);
    let me = session.me();
    let (game, rules) = session.harness_game();
    let auth = game.view().auth;
    let (capital, tile) = auth.cities().iter().find(|(_, c)| c.owner == me).map(|(h, c)| (h, c.tile)).unwrap();
    let dims = auth.map().dimensions;
    let site = classic_rules::math::tiles_in_hex_radius(&dims, tile, 7)
        .into_iter()
        .find(|t| {
            dims.distance(tile, *t).unwrap_or(0) >= 5
                && classic_rules::navigation::cost::enter_cost_on_map(auth.map(), *t).is_some()
                && auth.map().owner(*t).flatten().is_none()
        })
        .unwrap();
    let other = game.debug_found_city(&rules, me, site);
    let granary = key_index(rules.buildings.iter().map(|b| b.key.clone()), "core:building/granary");
    game.debug_add_building(&rules, capital, classic_rules::ids::BuildingKindId(granary));
    let husbandry = key_index(rules.technologies.iter().map(|t| t.key.clone()), "core:tech/animal-husbandry");
    game.debug_learn(&rules, me, classic_rules::ids::TechId(husbandry));
    let caravan = key_index(rules.units.iter().map(|u| u.key.clone()), "core:unit/caravan");
    game.debug_spawn_unit(&rules, me, tile, classic_rules::ids::UnitKindId(caravan));
    session.refresh_after_harness();
    let view = session.overview_view();
    let unit = view.trade_units.first().expect("the caravan is listed");
    let offer = unit.offers.iter().find(|o| o.destination == other).expect("a food route to the new city");
    assert!(offer.yields.contains("food"), "{}", offer.yields);
    assert!(offer.other_end.is_empty(), "internal benefit belongs to the destination");
    assert!(view.trade_summary.contains("0 active routes · 1 idle trader"), "{}", view.trade_summary);
    session.request(GameRequest::EstablishTradeRoute {
        unit: unit.unit,
        destination: offer.destination,
        kind: offer.kind,
    });
    let view = session.overview_view();
    assert_eq!(view.trade_routes.len(), 1);
    assert!(view.trade_units.is_empty(), "the caravan became the route");
    assert!(view.trade_summary.contains("1 active route · 0 idle traders"), "{}", view.trade_summary);
    assert!(view.notices.iter().any(|n| n.text.starts_with("Trade route to")));
}

#[path = "ui_journey_tests.rs"]
mod ui_journey_tests;

/// Saves the shell keeps (`client::shell::saves`) load back into the same match.
mod save_files {
    use client::shell::module::ModuleId;
    use client::shell::saves::{Autosaves, ModuleSaves, SaveError, SaveSlot};
    use classic_rules::module::RulesModule;

    pub(crate) const CLASSIC: ModuleId = classic_rules::classic::Classic::ID;

    #[test]
    fn an_autosaved_game_loads_back_to_the_same_world() {
        use crate::ui::screens::{game_rules, prepare_match, MapSize, MatchRequest};
        let dir = std::env::temp_dir().join(format!("aurelia-autosave-load-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let ring = Autosaves::in_dir(dir.clone(), CLASSIC);
        let request = MatchRequest {
            nation: "core:nation/greece".into(),
            seed: 7,
            size: MapSize::Small,
            players: 2,
            water: 25,
        };
        let prepared = prepare_match(request).unwrap();
        let mut session = crate::game::Session::with_difficulty(prepared.game, game_rules(), "core:handicap/king");
        session.end_turn();
        session.end_turn();
        for _ in 0..3 {
            ring.write(&session.save_bytes(), session.turn(), 2).unwrap();
        }
        assert_eq!(ring.list().len(), 2);
        let bytes = ring.latest().unwrap().read().unwrap();
        let loaded = crate::game::Session::from_save(&bytes, game_rules()).unwrap();
        assert_eq!(loaded.world_hash(), session.world_hash());
        assert_eq!(loaded.turn(), session.turn());
        assert_eq!(loaded.difficulty(), "core:handicap/king", "the difficulty comes back with the save");
        std::fs::remove_dir_all(dir).unwrap();
    }


    #[test]
    fn a_damaged_save_file_is_an_error() {
        let dir = std::env::temp_dir().join(format!("aurelia-save-damaged-{}", std::process::id()));
        let slot = SaveSlot::at(dir.join("quicksave.aurelia"), CLASSIC);
        slot.write(b"AURELIA\0payload").unwrap();
        let rules = crate::ui::screens::game_rules();
        assert!(crate::game::decode_game(&slot.read().unwrap(), &rules).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A classic save of an earlier build, where it was before saves were kept per module:
    /// `$AURELIA_SAVE_DIR/quicksave.aurelia` and the autosave ring, untagged.
    #[test]
    fn a_save_of_the_shared_layout_moves_to_classic_and_loads() {
        const OLD: &[u8] = include_bytes!("../../../classic-rules/tests/fixtures/city-names-before-v1.aurelia");
        const SANDBOX: ModuleId = ModuleId { name: "sandbox", version: 1 };
        let root = std::env::temp_dir().join(format!("aurelia-shared-layout-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("autosave")).unwrap();
        std::fs::write(root.join("quicksave.aurelia"), OLD).unwrap();
        std::fs::write(root.join("autosave").join("autosave-000003-t0001.aurelia"), OLD).unwrap();
        std::fs::write(root.join("hall_of_fame.txt"), "Rome · Science victory · turn 300 · score 900\n").unwrap();

        let classic = ModuleSaves::under(&root, CLASSIC);
        assert_eq!(classic.adopt_shared_saves(&root).unwrap(), 3);
        let rules = std::sync::Arc::new(classic_rules::compile_rules(classic_content::classic::classic_rules()).unwrap());
        for slot in [classic.quick_save(), classic.autosaves().latest().unwrap()] {
            let loaded = crate::game::Session::from_save(&slot.read().unwrap(), rules.clone()).unwrap();
            assert_eq!(loaded.world_hash(), 156481080629868776, "{:?}", slot.path());
        }
        assert!(classic.latest().is_some(), "Continue finds the adopted game");
        assert_eq!(classic.hall_of_fame().lines().len(), 1);

        let sandbox = ModuleSaves::under(&root, SANDBOX);
        assert!(sandbox.latest().is_none() && sandbox.hall_of_fame().lines().is_empty(), "other modules see none of it");
        let foreign = SaveSlot::at(classic.quick_save().path().to_path_buf(), SANDBOX).read();
        assert_eq!(foreign, Err(SaveError::OtherModule { saved: "classic".into(), running: "sandbox" }));
        std::fs::remove_dir_all(root).unwrap();
    }
}

/// A technology of a later era announces the era and the top bar names it.
#[test]
fn entering_a_new_era_is_announced_and_named_in_the_hud() {
    let mut session = new_match(20260928);
    assert_eq!(session.hud_model(None).era, "Ancient Era");
    let rules = session.rules().clone();
    let classical = rules
        .technologies
        .iter()
        .position(|t| t.content_era.is_some_and(|e| rules.eras[e.0 as usize].index == 1))
        .expect("a classical technology");
    let before = session.snapshot();
    let me = session.me();
    session.client.game.debug_learn(&rules, me, classic_rules::ids::TechId(classical as u16));
    session.report_research(&before);
    let notices = session.hud_model(None).notices;
    assert!(notices.iter().any(|n| n.text == "A new era begins: the Classical Era"), "{notices:?}");
    assert_eq!(session.hud_model(None).era, "Classical Era");
    let again = session.snapshot();
    session.report_research(&again);
    let count = session.hud_model(None).notices.iter().filter(|n| n.text.starts_with("A new era")).count();
    assert_eq!(count, 1, "announced once");
}

/// The player's air orders: an aircraft rebases with a move click on an own
/// city, a paratrooper jumps with a right click on free known ground.
#[test]
fn aircraft_rebase_and_paratroopers_jump_by_clicks() {
    use classic_rules::ids::UnitKindId;
    use classic_rules::math::tiles_in_hex_radius;
    let mut session = new_match(7);
    found_capital(&mut session);
    let (rules, capital) = {
        let auth = session.game().view().auth;
        let city = auth.indexes().cities_of(PlayerId(0))[0];
        (session.rules().clone(), auth.cities().get(city).unwrap().tile)
    };
    let kind = |key: &str| UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap() as u16);
    let (second, free) = {
        let auth = session.game().view().auth;
        let dims = auth.map().dimensions;
        let land: Vec<TileId> = tiles_in_hex_radius(&dims, capital, 6)
            .into_iter()
            .filter(|t| classic_rules::navigation::cost::enter_cost_on_map(auth.map(), *t).is_some())
            .collect();
        let at = |d: i32| land.iter().copied().find(|t| dims.distance(capital, *t) == Some(d)).expect("land at distance");
        (at(5), at(3))
    };
    let (fighter, trooper) = {
        let (game, rules) = session.harness_game();
        game.debug_found_city(&rules, PlayerId(0), second);
        let dims = game.view().auth.map().dimensions;
        game.debug_reveal(PlayerId(0), &tiles_in_hex_radius(&dims, capital, 8));
        let fighter = game.debug_spawn_unit(&rules, PlayerId(0), capital, kind("core:unit/fighter"));
        let trooper = game.debug_spawn_unit(&rules, PlayerId(0), capital, kind("core:unit/paratrooper"));
        (fighter, trooper)
    };
    session.refresh_after_harness();
    let tile_of = |session: &Session, unit| session.game().view().auth.units().get(unit).unwrap().core.tile;
    // Rebase: Move mode, click the other city.
    session.select_unit(fighter);
    session.request(GameRequest::Unit(Action::Move));
    session.click(Some(second), MapButton::Primary);
    assert_eq!(tile_of(&session, fighter), second, "the fighter rebased to the other city");
    // Paradrop: right click on free ground.
    session.select_unit(trooper);
    session.click(Some(free), MapButton::Secondary);
    assert_eq!(tile_of(&session, trooper), free, "the paratrooper jumped");
    // A second jump from the open field is refused with a worded reason.
    let before = session.world_hash();
    session.click(Some(capital), MapButton::Secondary);
    assert_eq!(session.world_hash(), before);
    assert!(session.hud_model(None).message.is_some());
}

/// The player's air orders by buttons and clicks: a fighter waits to intercept (the
/// button goes, the status and a map mark say so), sweeps a tile in range (the range is
/// lit while the mode waits; out of range is refused with words; Esc leaves the mode),
/// and units that cannot do either are not offered the buttons.
#[test]
fn a_fighter_intercepts_and_sweeps_by_buttons_and_clicks() {
    use classic_rules::ids::UnitKindId;
    use classic_rules::math::tiles_in_hex_radius;
    use classic_rules::world::unit::Stance;
    let mut session = new_match(7);
    found_capital(&mut session);
    let (rules, capital) = {
        let auth = session.game().view().auth;
        let city = auth.indexes().cities_of(PlayerId(0))[0];
        (session.rules().clone(), auth.cities().get(city).unwrap().tile)
    };
    let kind = |key: &str| UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap() as u16);
    let range = i32::from(rules.units[kind("core:unit/fighter").0 as usize].range);
    assert!(range > 1, "a fighter reaches beyond its neighbours");
    let (fighter, spare, bomber, warrior) = {
        let (game, rules) = session.harness_game();
        let dims = game.view().auth.map().dimensions;
        game.debug_reveal(PlayerId(0), &tiles_in_hex_radius(&dims, capital, range + 2));
        let fighter = game.debug_spawn_unit(&rules, PlayerId(0), capital, kind("core:unit/fighter"));
        let spare = game.debug_spawn_unit(&rules, PlayerId(0), capital, kind("core:unit/fighter"));
        let bomber = game.debug_spawn_unit(&rules, PlayerId(0), capital, kind("core:unit/bomber"));
        let warrior = game.debug_spawn_unit(&rules, PlayerId(0), capital, kind("core:unit/warrior"));
        (fighter, spare, bomber, warrior)
    };
    session.refresh_after_harness();
    let (near, far) = {
        let dims = session.game().view().auth.map().dimensions;
        let ring = |d: i32| {
            let tiles = tiles_in_hex_radius(&dims, capital, d);
            tiles.into_iter().find(|t| dims.distance(capital, *t) == Some(d)).expect("a tile at the distance")
        };
        (ring(range - 1), ring(range + 1))
    };
    let offered = |session: &mut Session, unit| {
        session.select_unit(unit);
        let orders = session.unit_orders().expect("orders");
        orders.actions.iter().map(|a| a.action).collect::<Vec<_>>()
    };
    // Who is offered what.
    let fighter_orders = offered(&mut session, fighter);
    assert!(fighter_orders.contains(&Action::Sweep) && fighter_orders.contains(&Action::Intercept));
    assert!(!offered(&mut session, bomber).contains(&Action::Sweep), "a bomber does not sweep");
    let walker = offered(&mut session, warrior);
    assert!(!walker.contains(&Action::Sweep) && !walker.contains(&Action::Intercept));
    // The sweep mode: the button toggles it, the range is lit, the capital itself is not.
    session.select_unit(fighter);
    session.request(GameRequest::Unit(Action::Sweep));
    assert_eq!(session.hud_model(None).orders.unwrap().targeting, Some(Action::Sweep));
    let paint = session.air_paint();
    let dims = session.game().view().auth.map().dimensions;
    assert!(paint.tints.len() > 6, "the tiles in reach are lit");
    assert!(paint.tints.iter().all(|t| dims.distance(capital, TileId(t.tile)).is_some_and(|d| d >= 1 && d <= range)));
    assert!(paint.tints.iter().any(|t| t.tile == near.0) && paint.tints.iter().all(|t| t.tile != far.0));
    // Out of range: refused in words, nothing changes, the mode is over.
    let before = session.world_hash();
    session.click(Some(far), MapButton::Primary);
    assert_eq!(session.world_hash(), before);
    assert_eq!(session.hud_model(None).message.as_deref(), Some("The tile is beyond the sweep range"));
    assert_eq!(session.hud_model(None).orders.unwrap().targeting, None);
    assert!(session.air_paint().tints.is_empty(), "the range goes with the mode");
    // In range: the fighter flies the sweep and has spent its turn.
    session.request(GameRequest::Unit(Action::Sweep));
    session.click(Some(near), MapButton::Primary);
    let core = *session.game().view().auth.units().get(fighter).unwrap().core;
    assert_eq!((core.moves_left, core.attacks_made, core.tile), (0, 1, capital), "it swept and came home");
    let orders = session.unit_orders().unwrap();
    let sweep = orders.actions.iter().find(|a| a.action == Action::Sweep).expect("the button stays");
    assert_eq!(sweep.availability, Availability::Blocked("The unit has no attack left this turn".into()));
    // Esc leaves the mode.
    session.select_unit(spare);
    session.request(GameRequest::Unit(Action::Sweep));
    assert!(session.cancel_aim() && session.hud_model(None).orders.unwrap().targeting.is_none());
    assert!(!session.cancel_aim(), "nothing more to cancel");
    // Interception duty: an immediate order; the button goes, the stance shows.
    assert!(session.air_paint().icons.is_empty());
    session.request(GameRequest::Unit(Action::Intercept));
    assert_eq!(session.game().view().auth.units().get(spare).unwrap().core.stance, Stance::Intercept);
    let orders = session.unit_orders().unwrap();
    assert!(!orders.actions.iter().any(|a| a.action == Action::Intercept), "already waiting");
    assert!(orders.status.ends_with("Intercepting"), "{}", orders.status);
    let marks = session.air_paint().icons;
    assert_eq!(marks.len(), 1, "one mark per tile");
    assert_eq!((marks[0].tile, marks[0].label.as_deref()), (capital.0, Some("Intercepting")));
}

/// A nuclear strike is a public act: the player is told of it, whoever used the weapon and
/// wherever it fell (classic/docs/classic/RESOURCING.md, "Infamy of a nuclear strike").
#[test]
fn a_nuclear_strike_is_announced_to_the_player() {
    use classic_rules::transaction::journal::{AppliedCommand, ChangeJournal, UnitChange};
    let mut session = new_match(7);
    let game = session.game().clone();
    let unit = game.view().auth.indexes().units_of(PlayerId(1)).first().copied().expect("a unit of the other player");
    let tile = game.view().auth.units().get(unit).unwrap().core.tile;
    let mut changes = ChangeJournal::default();
    changes.unit_changes.push(UnitChange::Destroyed {
        unit,
        id: classic_rules::ids::EntityId(1),
        owner: PlayerId(1),
        tile,
        kind: classic_rules::ids::UnitKindId(0),
        promotions: Default::default(),
    });
    let before = session.notices.len();
    session.notify_nuclear(&[AppliedCommand { command: classic_rules::Command::Nuke { unit, target: tile }, changes }]);
    assert_eq!(session.notices.len(), before + 1);
    let notice = &session.notices[0];
    assert_eq!(notice.kind, NoticeKind::Combat);
    assert_eq!(notice.tile, Some(tile.0));
    assert!(notice.text.contains(&session.player_name(PlayerId(1))), "{}", notice.text);
}

/// A match with 3 nations, 2 city-states (players 3 and 4) and the barbarians (player 5), seen
/// from the human (player 0).
fn session_with_every_kind_of_player() -> Session {
    use crate::ui::screens::{prepare_game, GameOptions, NewGame};
    let new_game = NewGame {
        request: MatchRequest { nation: "core:nation/rome".into(), seed: 11, size: MapSize::Small, players: 3, water: 25 },
        options: GameOptions { city_states: 2, barbarians: true, ..GameOptions::default() },
    };
    Session::new(prepare_game(new_game).expect("world").game, game_rules())
}

/// The barbarians have the nation index 0 (America) in the players' table: the name of a player
/// goes by its kind, so they must not be called by the nation of that index.
#[test]
fn a_player_is_named_by_its_kind() {
    use classic_rules::world::player::ClassicPlayers;
    let session = session_with_every_kind_of_player();
    let auth = session.game().view().auth;
    let barbarian = auth.players().barbarian().expect("barbarians");
    let (city_state, nation) = (PlayerId(3), PlayerId(0));
    assert!(auth.players().is_city_state(city_state));
    let rules = game_rules();
    assert_eq!(session.player_name(barbarian), "Barbarians");
    let def = &rules.module.city_states[auth.players().city_state_of(city_state).unwrap().0 as usize];
    assert_eq!(session.player_name(city_state), crate::game::names::named("city-state", &def.name, &def.key));
    assert_eq!(session.player_name(nation), "Rome");
    // No player but the nation with the index shares its name.
    assert_ne!(session.player_name(barbarian), crate::game::names::nation(&rules, auth.players().nation_of(barbarian)));
    let names: Vec<String> = (0..auth.players().count()).map(|p| session.player_name(PlayerId(p as u8))).collect();
    let mut unique = names.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), names.len(), "every player has its own name: {names:?}");
}

/// The map's diplomacy panel and every other list of rivals leave the barbarians out (always at
/// war, nothing to make peace or trade), while a met city-state stays, under its own name.
#[test]
fn the_rival_list_has_no_barbarians() {
    use classic_rules::world::player::ClassicPlayers;
    let mut session = session_with_every_kind_of_player();
    let barbarian = session.game().view().auth.players().barbarian().expect("barbarians");
    assert!(session.game().view().auth.diplomacy().at_war(session.me(), barbarian), "always at war");
    assert!(session.rivals().iter().all(|r| r.player != barbarian.0), "turn 1: {:?}", session.rivals());
    assert!(session.hud_model(None).rivals.is_empty(), "nobody is met on turn 1");
    let mut game = session.game().clone();
    game.debug_meet(PlayerId(0), PlayerId(3));
    game.debug_meet(PlayerId(0), PlayerId(1));
    session.replace_game(game);
    let rivals = session.rivals();
    let listed: Vec<u8> = rivals.iter().map(|r| r.player).collect();
    assert_eq!(listed, vec![1, 3], "the met nation and the met city-state only");
    assert_eq!(rivals[1].name, session.player_name(PlayerId(3)));
    assert_ne!(rivals[1].name, session.player_name(PlayerId(0)));
}
