//! City map orders change real citizen allocation and survive persistence.
use super::*;
use crate::ui::model::Action;
use crate::ui::screens::{game_rules, prepare_match, MapSize, MatchRequest};

fn city_session() -> Session {
    let rules = game_rules();
    let prepared = prepare_match(MatchRequest {
        civilization: "core:nation/rome".into(), seed: 20260928,
        size: MapSize::Small, players: 3, water: 25,
    }).unwrap();
    let mut session = Session::new(prepared.game, rules);
    session.request(GameRequest::Unit(Action::FoundCity));
    session
}

#[test]
fn citizen_map_assign_remove_lock_and_save_use_actual_workers() {
    let mut session = city_session();
    let initial = session.city_view().unwrap();
    assert_eq!(initial.population, 1);
    let target = initial.tiles.iter().find(|t| !t.worked && t.lock.is_ready()).unwrap().tile;
    session.request(GameRequest::LockTile(target));
    let assigned = session.city_view().unwrap();
    assert_eq!(assigned.tiles.iter().filter(|t| t.worked).count(), 1);
    let row = assigned.tiles.iter().find(|t| t.tile == target).unwrap();
    assert!(row.worked && row.locked);
    assert!(assigned.tiles.iter().filter(|t| t.tile != target).all(|t| !t.lock.is_ready()),
        "a full manual allocation must not offer a silent competing lock");
    assert!(row.lock.is_ready(), "unlock remains available");
    let mut loaded = Session::from_save(&session.save_bytes(), game_rules()).unwrap();
    loaded.request(GameRequest::SelectCity(assigned.tile));
    assert_eq!(session.world_hash(), loaded.world_hash());
    assert_eq!(loaded.city_view().unwrap(), assigned);
    for game in [&mut session, &mut loaded] {
        game.request(GameRequest::ToggleTile(target));
        let removed = game.city_view().unwrap();
        let row = removed.tiles.iter().find(|t| t.tile == target).unwrap();
        assert!(!row.worked && !row.locked);
        assert!(row.lock.is_ready(), "can reassign an explicitly excluded tile");
        game.request(GameRequest::LockTile(target));
        assert!(game.city_view().unwrap().tiles.iter().any(|t| t.tile == target && t.worked && t.locked));
        game.request(GameRequest::LockTile(target));
        assert!(game.city_view().unwrap().tiles.iter().all(|t| !t.locked));
        game.game().validate(game.rules()).unwrap();
    }
    assert_eq!(session.world_hash(), loaded.world_hash());
}

#[test]
fn citizen_map_manual_specialist_reserves_population_until_removed() {
    let mut session = city_session();
    let city_tile = session.city_view().unwrap().tile;
    let rules = game_rules();
    let (building, kind) = rules.buildings.iter().enumerate()
        .find_map(|(i, b)| b.specialist_slots.map(|(kind, _)| (i as u16, kind))).unwrap();
    let mut save = classic_rules::persistence::encode::encode(session.game(), &rules);
    save.world.cities.iter_mut().find(|c| c.tile == city_tile).unwrap().buildings.push(building);
    let mut session = Session::new(classic_rules::persistence::decode::decode(&save, &rules).unwrap(), rules);
    session.request(GameRequest::SelectCity(city_tile));
    session.request(GameRequest::SetSpecialists(kind, 1));
    let full = session.city_view().unwrap();
    assert!(full.specialists.iter().any(|s| s.kind == kind && s.working == 1 && s.forced == 1));
    assert!(full.tiles.iter().all(|t| !t.worked && !t.lock.is_ready()));
    session.request(GameRequest::SetSpecialists(kind, 0));
    assert!(session.city_view().unwrap().tiles.iter().all(|t| t.lock.is_ready()));
}
