//! Disbanding a unit and selling a building through the session the window uses (no GPU, no
//! window): the order or the Sell button opens a card that tells the gold; nothing happens
//! before the yes; no keeps everything; yes does it and the notice tells it; the card reads in
//! every language of the game.
use classic_rules::ids::{BuildingKindId, PlayerId, UnitHandle};
use classic_rules::rules::content::BuildingKind;

use super::Session;
use crate::ui::model::{Action, Availability, GameRequest};
use crate::ui::screens::{game_rules, prepare_match, MapSize, MatchRequest};

const LANGUAGES: [&str; 7] = ["en", "ru", "de", "fr", "es", "ja", "zh-Hans"];

fn new_match(seed: u64) -> Session {
    let request = MatchRequest { nation: "core:nation/rome".into(), seed, size: MapSize::Small, players: 3, water: 25 };
    let prepared = prepare_match(request).expect("world");
    Session::new(prepared.game, game_rules())
}

/// The player's capital founded by its settler; the city tile.
fn capital(session: &mut Session) -> classic_rules::ids::TileId {
    session.request(GameRequest::Unit(Action::FoundCity));
    let me = session.me();
    let auth = session.game().view().auth;
    let city = auth.indexes().cities_of(me)[0];
    auth.cities().get(city).unwrap().tile
}

fn treasury(session: &Session) -> u32 {
    session.game().view().auth.players().treasury_of(session.me())
}

fn notices(session: &mut Session) -> Vec<String> {
    session.hud_model(None).notices.into_iter().map(|n| n.text).collect()
}

/// A warrior of the player in its capital, selected.
fn warrior_at_home(session: &mut Session) -> UnitHandle {
    let at = capital(session);
    let me: PlayerId = session.me();
    let unit = {
        let (game, rules) = session.harness_game();
        let kind = classic_rules::ids::UnitKindId(rules.units.iter().position(|u| u.key == "core:unit/warrior").unwrap() as u16);
        game.debug_spawn_unit(&rules, me, at, kind)
    };
    session.refresh_after_harness();
    session.select_unit(unit);
    unit
}

#[test]
fn the_disband_order_waits_for_the_yes_and_tells_the_gold() {
    let mut session = new_match(7);
    let unit = warrior_at_home(&mut session);
    let alive = |session: &Session| session.game().view().auth.units().get(unit).is_some();
    let orders = session.unit_orders().expect("the warrior's orders");
    let disband = orders.actions.iter().find(|a| a.action == Action::Disband).expect("every unit may be disbanded");
    assert_eq!(disband.availability, Availability::Ready);
    assert_eq!(disband.detail.as_deref(), Some("Disband for 10 gold"));

    let before = session.world_hash();
    session.request(GameRequest::Unit(Action::Disband));
    assert_eq!(session.world_hash(), before, "nothing happens before the yes");
    let card = session.hud_model(None).confirm.expect("the card asks first");
    assert_eq!(card.title, "Disband Warrior?");
    assert!(card.line.contains("10 gold"), "{}", card.line);
    assert_eq!((card.confirm.as_str(), card.cancel.as_str()), ("Disband", "Cancel"));
    for lang in LANGUAGES {
        let card = i18n::with_language(lang, || session.hud_model(None).confirm.unwrap());
        for words in [&card.title, &card.line, &card.confirm, &card.cancel] {
            assert!(!words.is_empty() && !words.contains("classic."), "{lang}: {card:?}");
        }
        assert!(card.line.contains("10"), "{lang}: the gold in {}", card.line);
    }
    session.request(GameRequest::Confirm(false));
    assert!(alive(&session) && session.hud_model(None).confirm.is_none(), "no: the unit stays");
    assert_eq!(session.world_hash(), before);

    let gold = treasury(&session);
    session.request(GameRequest::Unit(Action::Disband));
    session.request(GameRequest::Confirm(true));
    assert!(!alive(&session), "yes: the unit is gone");
    assert_eq!(treasury(&session), gold + 10);
    assert!(notices(&mut session).contains(&"Warrior disbanded for 10 gold".to_string()));
}

/// A unit with no moves left keeps the order greyed, with the rules' reason in the player's
/// language.
#[test]
fn a_unit_without_moves_cannot_be_disbanded() {
    let mut session = new_match(7);
    let unit = warrior_at_home(&mut session);
    {
        let (game, rules) = session.harness_game();
        let auth = game.view().auth;
        let from = auth.units().get(unit).unwrap().core.tile;
        let dims = auth.map().dimensions;
        let steps: Vec<_> = classic_rules::math::tiles_in_hex_radius(&dims, from, 1).into_iter().filter(|t| *t != from).collect();
        for step in steps {
            if game.view().auth.units().get(unit).unwrap().core.moves_left == 0 {
                break;
            }
            let _ = game.execute(&rules, classic_rules::Command::MoveUnit { unit, destination: step });
        }
        assert_eq!(game.view().auth.units().get(unit).unwrap().core.moves_left, 0);
    }
    session.refresh_after_harness();
    session.select_unit(unit);
    let orders = session.unit_orders().expect("orders");
    let disband = orders.actions.iter().find(|a| a.action == Action::Disband).unwrap();
    assert_eq!(disband.availability, Availability::Blocked("A unit disbands only with moves left".into()));
    session.request(GameRequest::Unit(Action::Disband));
    assert!(session.hud_model(None).confirm.is_none(), "no card for an order the rules refuse");
}

#[test]
fn a_building_sells_after_the_yes_once_a_turn() {
    let mut session = new_match(7);
    let at = capital(&mut session);
    let regular: Vec<BuildingKindId> = {
        let (game, rules) = session.harness_game();
        let city = game.view().auth.indexes().cities_of(PlayerId(0))[0];
        let picks: Vec<BuildingKindId> = (0..rules.buildings.len())
            .filter(|i| {
                let b = &rules.buildings[*i];
                b.kind == BuildingKind::Regular && b.cost >= 40 && b.module.great_work_slots.is_none()
            })
            .take(2)
            .map(|i| BuildingKindId(i as u16))
            .collect();
        for b in &picks {
            game.debug_add_building(&rules, city, *b);
        }
        picks
    };
    session.refresh_after_harness();
    session.select_city_at(at);
    let rules = session.harness_game().1;
    let gold = rules.buildings[regular[0].0 as usize].cost / 10;
    let city = session.city_view().expect("the city screen");
    let palace = city.buildings.iter().find(|b| rules.buildings[b.kind.0 as usize].kind == BuildingKind::Palace).expect("the palace");
    assert!(palace.sale.is_none(), "the palace has no Sell button");
    let entry = city.buildings.iter().find(|b| b.kind == regular[0]).unwrap();
    let (label, availability) = entry.sale.clone().expect("a Sell button");
    assert_eq!(label, format!("Sell for {gold} gold"));
    assert_eq!(availability, Availability::Ready);

    let before = session.world_hash();
    session.request(GameRequest::SellBuilding(regular[0]));
    assert_eq!(session.world_hash(), before, "nothing happens before the yes");
    let card = session.city_view().unwrap().confirm.expect("the card over the city screen");
    assert_eq!(Some(card.clone()), session.hud_model(None).confirm, "the same card on the map");
    assert!(card.title.starts_with("Sell ") && card.line.contains(&format!("{gold} gold")), "{card:?}");
    for lang in LANGUAGES {
        let card = i18n::with_language(lang, || session.city_view().unwrap().confirm.unwrap());
        for words in [&card.title, &card.line, &card.confirm, &card.cancel] {
            assert!(!words.is_empty() && !words.contains("classic."), "{lang}: {card:?}");
        }
    }
    session.request(GameRequest::Confirm(false));
    assert_eq!(session.world_hash(), before, "no: the building stays");

    let treasury_before = treasury(&session);
    session.request(GameRequest::SellBuilding(regular[0]));
    session.request(GameRequest::Confirm(true));
    let city = session.city_view().unwrap();
    assert!(city.buildings.iter().all(|b| b.kind != regular[0]), "yes: the building is gone");
    assert_eq!(treasury(&session), treasury_before + gold);
    assert!(notices(&mut session).iter().any(|n| n.ends_with(&format!("sold for {gold} gold"))));
    let other = city.buildings.iter().find(|b| b.kind == regular[1]).unwrap();
    assert_eq!(
        other.sale.as_ref().map(|s| s.1.clone()),
        Some(Availability::Blocked("The city has already sold a building this turn".into())),
        "one sale a turn"
    );
    session.request(GameRequest::SellBuilding(regular[1]));
    assert!(session.city_view().unwrap().confirm.is_none(), "no card for a sale the rules refuse");
}
