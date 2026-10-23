//! Blocked controls say why, in words, from the rules' typed reasons; a city is renamed from its
//! civilization's list; the spies carry their civilization's names.
use client::selection::Selection;

use super::Session;
use crate::ui::model::{Action, Availability, GameRequest};
use crate::ui::screens::{game_rules, prepare_match, MapSize, MatchRequest};

fn session(seed: u64) -> Session {
    let request = MatchRequest {
        civilization: "core:nation/rome".into(),
        seed,
        size: MapSize::Small,
        players: 3,
        water: 25,
    };
    Session::new(prepare_match(request).expect("world").game, game_rules())
}

fn blocked(availability: &Availability) -> &str {
    match availability {
        Availability::Blocked(reason) => reason,
        Availability::Ready => panic!("expected a blocked control"),
    }
}

fn found_capital(session: &mut Session) {
    session.request(GameRequest::Unit(Action::FoundCity));
    assert!(matches!(session.selection(), Selection::City(_)));
}

fn state(session: &mut Session, action: Action) -> Availability {
    let orders = session.unit_orders().expect("a selected unit");
    orders.actions.iter().find(|a| a.action == action).expect("offered").availability.clone()
}

#[test]
fn a_fortified_unit_and_an_empty_queue_and_an_empty_treasury_say_why() {
    let mut session = session(20260928);
    found_capital(&mut session);
    let warrior = session.idle_units().into_iter().next().expect("a unit that can fortify");
    session.select_unit(warrior);
    assert_eq!(state(&mut session, Action::Fortify), Availability::Ready);
    session.request(GameRequest::Unit(Action::Fortify));
    session.select_unit(warrior);
    assert_eq!(blocked(&state(&mut session, Action::Fortify)), "Already fortified");
    let city = session.game().view().auth.cities().iter().next().unwrap().1.tile;
    session.select_city_at(city);
    let view = session.city_view().unwrap();
    assert_eq!(blocked(&view.hurry.1), "The queue is empty");
    assert_eq!(blocked(&view.clear), "The queue is empty");
}

#[test]
fn a_locked_unit_names_the_technology_it_needs() {
    let mut session = session(20260928);
    found_capital(&mut session);
    let view = session.city_view().unwrap();
    let archer = view.options.iter().find(|o| o.name == "Archer").expect("archer is listed");
    let reason = blocked(&archer.availability);
    assert!(reason.starts_with("Needs technology: "), "{reason}");
    assert!(reason.contains("Archery"), "{reason}");
}

#[test]
fn a_city_is_renamed_to_a_free_name_of_its_civilization_and_the_name_becomes_free_again() {
    let mut session = session(20260928);
    found_capital(&mut session);
    let before = session.city_view().unwrap();
    assert_eq!(before.name, "Rome");
    assert!(before.names.len() >= 9, "Rome's list has more names: {:?}", before.names);
    assert!(before.names.iter().all(|(_, name)| name != "Rome"), "the name it holds is not a choice");
    let (place, chosen) = before.names[2].clone();
    session.request(GameRequest::RenameCity(place));
    let after = session.city_view().unwrap();
    assert_eq!(after.name, chosen);
    assert!(after.names.iter().any(|(_, name)| name == "Rome"), "the name given up is free again");
    assert!(after.names.iter().all(|(_, name)| *name != chosen));
    let notice = session.hud_model(None).notices.into_iter().next().expect("a notice");
    assert_eq!(notice.text, format!("Rome is now called {chosen}"));
    // A place that is no longer free is refused with a reason and the name stays.
    session.request(GameRequest::RenameCity(place));
    assert_eq!(session.city_view().unwrap().name, chosen);
    assert_eq!(session.hud_model(None).message.as_deref(), Some("That name is not available"));
}

#[test]
fn a_spy_has_its_civilizations_name_in_the_screen_the_notice_and_the_city_banner() {
    let mut session = session(20260928);
    found_capital(&mut session);
    let city_tile = session.game().view().auth.cities().iter().next().unwrap().1.tile.0;
    let (game, rules) = session.harness_game();
    let tech = rules.technologies.iter().position(|t| t.key == "core:tech/acoustics").unwrap();
    game.debug_learn(&rules, classic_rules::ids::PlayerId(0), classic_rules::ids::TechId(tech as u16));
    session.refresh_after_harness();
    session.request(GameRequest::EndTurn);
    let civ = session.game().view().auth.players().civ_of(classic_rules::ids::PlayerId(0));
    let first = session.rules().civs[civ as usize].module.spy_names[0].clone();
    let view = session.espionage_view();
    assert_eq!(view.spies.len(), 1);
    assert_eq!(view.spies[0].name, first);
    let home = view.targets.iter().find(|t| t.role == "Your city").expect("the own city").city;
    session.request(GameRequest::MoveSpy { spy: 0, city: Some(home), diplomat: false });
    let notice = session.hud_model(None).notices.into_iter().next().expect("a notice");
    assert!(notice.text.starts_with(&format!("{first} travels to ")), "{}", notice.text);
    let banner = session.hud_extras(None).banners.into_iter().find(|b| b.tile == city_tile).expect("banner");
    assert_eq!(banner.visitors, [first]);
}
