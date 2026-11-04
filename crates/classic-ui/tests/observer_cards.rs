//! The observer's tooltips and city cards over the classic rules: they say what the ordinary game's
//! tooltip and city screen say, from each nation's own knowledge, and reading them
//! never reaches the world.
use std::time::Duration;

use classic_ui::observer::{ClassicMatch, ClassicPowers};
use classic_ui::ui::screens::{game_rules, prepare_game, GameOptions, MapSize, MatchRequest, NewGame};
use client::shell::observer::{Card, Observed, Row, Tone};

fn observed(rounds: u32) -> ClassicMatch {
    observed_on(MapSize::Medium, rounds)
}

fn observed_on(size: MapSize, rounds: u32) -> ClassicMatch {
    let request = MatchRequest { nation: "core:nation/rome".into(), seed: 7, size, players: 4, water: 25 };
    let prepared = prepare_game(NewGame { request, options: GameOptions::default() }).expect("a world");
    let mut obs = ClassicMatch::new(prepared.game, game_rules(), ClassicPowers, None);
    while obs.game().view().auth.turn().round < rounds {
        obs.advance(Duration::from_secs(3600));
    }
    for power in 0..obs.powers().len() {
        obs.refresh(power);
    }
    obs
}

fn texts(card: &Card) -> Vec<String> {
    card.rows()
        .map(|row| match row {
            Row::Line(line) => line.text.clone(),
            Row::Pair { label, value, .. } => format!("{label} {value}"),
            Row::Bar { label, value, .. } => format!("{label} {value}"),
        })
        .collect()
}

fn titles(card: &Card) -> Vec<&str> {
    card.sections.iter().filter_map(|s| s.title.as_deref()).collect()
}

fn has_yields(card: &Card) -> bool {
    texts(card).iter().any(|t| t.contains("Food") && t.contains("Production"))
}

#[test]
fn a_tile_has_a_tooltip_only_as_far_as_the_nation_knows_it() {
    let obs = observed(20);
    let (mut seen, mut remembered) = (0, 0);
    for power in 0..obs.powers().len() {
        let world = obs.world(power);
        for tile in 0..world.width * world.height {
            let card = obs.tile_card(power, tile);
            let index = tile as usize;
            if !world.is_known(index) {
                assert!(card.is_none(), "power {power}: a card for the unexplored tile {tile}");
                continue;
            }
            let card = card.unwrap_or_else(|| panic!("power {power}: no card for the known tile {tile}"));
            assert!(!card.title.is_empty() && card.title != "Unknown terrain", "tile {tile}: {:?}", card.title);
            assert!(has_yields(&card), "tile {tile}: {:?}", texts(&card));
            let lines = texts(&card);
            if world.visible[index] == 1 {
                seen += 1;
                assert_eq!(card.subtitle.as_deref(), Some("Visible terrain"));
                // A tile with a river or a forest never says it has neither.
                let plain = lines.iter().any(|l| l == "No forest or river");
                assert!(!plain || world.river[index] != 1, "tile {tile}: {lines:?}");
                assert!(lines.iter().any(|l| l == "No revealed resource") || card.rows().any(|r| matches!(r, Row::Line(l) if l.tone == Tone::Accent)));
            } else {
                remembered += 1;
                assert_eq!(card.subtitle.as_deref(), Some("Last seen"));
                assert!(!lines.iter().any(|l| l.contains("orest") || l.contains("River") || l.contains("resource")), "tile {tile}: {lines:?}");
                // R355: the deposit of explored land is the live one, shown once its technology
                // unlocks it; no "No revealed resource" line out of sight.
                let unlocked = world.terrain[index].resource.is_some_and(|r| world.unlocked_resources.contains(&r));
                let shown = card.rows().any(|r| matches!(r, Row::Line(l) if l.tone == Tone::Accent));
                assert_eq!(shown, unlocked, "tile {tile}: the deposit line of a remembered tile");
            }
        }
    }
    assert!(seen > 0 && remembered > 0, "visible {seen}, remembered {remembered}");
}

#[test]
fn an_own_city_has_the_card_of_the_city_screen_and_a_foreign_one_only_the_public_one() {
    // A small map: the neighbours meet early.
    let obs = observed_on(MapSize::Small, 20);
    let mut foreign = 0;
    for power in 0..obs.powers().len() {
        let own = obs.cities_of(power);
        assert!(!own.is_empty(), "power {power} has a city");
        let nation = obs.powers()[power].name.clone();
        for city in &own {
            let card = obs.city_card(power, city.tile).expect("a card of an own city");
            assert_eq!(card.title, city.name);
            assert!(card.subtitle.as_deref().unwrap().starts_with(&nation), "{:?}", card.subtitle);
            for title in ["Citizens", "Yields", "Production", "Buildings", "Defense"] {
                assert!(titles(&card).contains(&title), "{}: {:?}", city.name, titles(&card));
            }
            let lines = texts(&card);
            assert!(lines.iter().any(|l| l.starts_with("Population")), "{lines:?}");
            assert!(lines.iter().any(|l| l.starts_with("Food ")) && lines.iter().any(|l| l.starts_with("Hit points")));
            assert!(card.sections.iter().all(|s| s.rows.len() <= 12), "a short card: {:?}", card);
        }
        for marker in &obs.world(power).cities {
            let Some(entry) = obs.city_at(power, marker.tile.0) else { continue };
            if entry.own {
                continue;
            }
            foreign += 1;
            let card = obs.city_card(power, marker.tile.0).expect("a card of a known foreign city");
            assert_eq!(card.title, marker.name);
            assert_eq!(card.subtitle, Some(format!("Nation: {}", entry.owner)));
            assert!(!titles(&card).iter().any(|t| ["Production", "Buildings", "Yields", "Citizens"].contains(t)), "{:?}", titles(&card));
            assert!(card.rows().count() <= 2);
        }
    }
    assert!(foreign > 0, "some nation knows a foreign city after twenty rounds");
}

#[test]
fn with_the_whole_map_open_every_tile_has_a_card_and_every_city_a_full_one() {
    let mut obs = observed(20);
    obs.set_revealed(true);
    for power in 0..obs.powers().len() {
        obs.refresh(power);
    }
    for power in 0..obs.powers().len() {
        let world = obs.world(power);
        for tile in 0..world.width * world.height {
            let card = obs.tile_card(power, tile).unwrap_or_else(|| panic!("power {power}: no card for tile {tile}"));
            assert_eq!(card.subtitle.as_deref(), Some("Visible terrain"));
        }
        let auth = obs.game().view().auth;
        let cities = auth.cities().iter().map(|(_, c)| (c.tile.0, c.name.clone(), c.owner)).collect::<Vec<_>>();
        assert!(cities.len() > obs.cities_of(power).len(), "the map holds foreign cities");
        for (tile, name, owner) in cities {
            if !auth.players().is_major(owner) {
                continue;
            }
            let card = obs.city_card(power, tile).unwrap_or_else(|| panic!("power {power}: no card for {name}"));
            assert_eq!(card.title, name);
            assert!(titles(&card).contains(&"Production"), "{name}: {:?}", titles(&card));
            if owner.0 != obs.powers()[power].player {
                let subtitle = card.subtitle.unwrap();
                assert!(subtitle.starts_with("Nation: "), "{subtitle}");
            }
        }
    }
}

#[test]
fn reading_the_cards_does_not_reach_the_world() {
    let mut obs = observed(20);
    let before = obs.state_hash();
    for revealed in [false, true] {
        obs.set_revealed(revealed);
        for power in 0..obs.powers().len() {
            obs.refresh(power);
            let world = obs.world(power);
            for tile in 0..world.width * world.height {
                obs.tile_card(power, tile);
                obs.city_card(power, tile);
            }
        }
    }
    assert_eq!(obs.state_hash(), before);
}
