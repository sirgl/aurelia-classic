//! The observer's tooltips and cards of the classic game, as the shell's plain data
//! (`Card`): a tile's tooltip is the ordinary game's (`ui::hud::tile_tooltip`), a city's
//! card is the ordinary city screen's view model (`CityView`) with no orders. Nothing here
//! works an economy out: every figure is what the game's own screens show.
use std::sync::Arc;

use classic_rules::ids::{PlayerId, TileId};
use classic_rules::{CompiledRuleset, GameState};
use client::shell::observer::{Card, Line, Row, Section, Tone};
use render::world::state::RenderWorld;

use i18n::{t, tf};

use crate::game::{ObservedCity, Session};
use crate::ui::hud::tile_tooltip;
use crate::ui::model::CityView;
use crate::ui::screens::game_rules;

/// Queue entries a city card lists before «and N more».
const QUEUE_ROWS: usize = 6;

/// The names of the game's nations by player index (the owners of the tiles).
fn owner_names(game: &GameState, name: impl Fn(PlayerId) -> String) -> Vec<String> {
    let count = game.view().auth.players().count();
    (0..count).map(|player| name(PlayerId(player as u8))).collect()
}

/// The tooltip of a tile the viewer knows (see `tile_tooltip::card`).
pub fn tile_card(game: &GameState, world: &RenderWorld, tile: TileId, name: impl Fn(PlayerId) -> String) -> Option<Card> {
    tile_tooltip::card(world, tile.0, &owner_names(game, name))
}

/// The card of the city on `tile`: the whole of it for `full`, else what the viewer's
/// marker of the city says.
pub fn city_card(
    game: &GameState,
    rules: &CompiledRuleset,
    world: &RenderWorld,
    viewer: PlayerId,
    tile: TileId,
    full: bool,
    name: impl Fn(PlayerId) -> String,
) -> Option<Card> {
    if !full {
        return public_card(world, tile, name);
    }
    match Session::observed_city(game.clone(), ruleset(rules), tile) {
        Some(city) => Some(full_card(&city, viewer)),
        // A city of a power that has no city screen (a city-state): what its marker says.
        None => public_card(world, tile, name),
    }
}

/// The game's ruleset as the shared handle a session takes: the cached one when the match
/// plays by it, else a copy.
fn ruleset(rules: &CompiledRuleset) -> Arc<CompiledRuleset> {
    let shared = game_rules();
    if std::ptr::eq(rules, Arc::as_ptr(&shared)) {
        shared
    } else {
        Arc::new(rules.clone())
    }
}

/// A foreign city the viewer knows: its name, its nation and the figures its marker
/// carries; no economy, no queue.
fn public_card(world: &RenderWorld, tile: TileId, name: impl Fn(PlayerId) -> String) -> Option<Card> {
    let marker = world.cities.iter().find(|c| c.tile == tile)?;
    let mut rows = Vec::new();
    if let Some(population) = marker.population {
        rows.push(Row::pair(t!("classic.observer-cards.population"), population));
    }
    if !marker.buildings.is_empty() {
        rows.push(Row::pair(t!("classic.observer-cards.known-buildings"), marker.buildings.len()));
    }
    if rows.is_empty() {
        rows.push(Row::note(t!("classic.observer-cards.nothing-known")));
    }
    let card = Card::new(marker.name.clone()).subtitle(tf!("classic.observer-cards.nation", name = name(PlayerId(marker.owner))));
    Some(card.section(Section::plain(rows)))
}

/// The city screen's facts, read-only. A city of another nation than the viewer's
/// (the whole map is open) names its nation.
fn full_card(city: &ObservedCity, viewer: PlayerId) -> Card {
    let view = &city.view;
    let mut subtitle = if city.owner == viewer.0 { city.nation.clone() } else { tf!("classic.observer-cards.nation", name = city.nation.as_str()) };
    if city.capital {
        subtitle = tf!("classic.observer-cards.with-capital", subtitle = subtitle);
    }
    let mut card = Card::new(view.name.clone()).subtitle(subtitle);
    let sections = [citizens(view), yields(view), production(view), buildings(view), specialists(view), great_people(view), defense(view)];
    // A section with nothing to say (no specialists, no great people) is left out.
    card.sections = sections.into_iter().filter(|section| !section.rows.is_empty()).collect();
    card
}

fn percent(stored: u32, needed: u32) -> u8 {
    (u64::from(stored) * 100 / u64::from(needed.max(1))).min(100) as u8
}

fn citizens(view: &CityView) -> Section {
    // The first yield of the city screen is the food surplus, signed: a negative one starves
    // the city, a zero one does not grow it (the words are the screen's, in any language).
    let food = view.yields.first().map_or("+0", |(_, value)| value.as_str());
    let growth_tone = if food.starts_with('-') {
        Tone::Bad
    } else if food == "+0" {
        Tone::Muted
    } else {
        Tone::Plain
    };
    Section::new(
        t!("classic.observer-cards.citizens"),
        vec![
            Row::pair(t!("classic.observer-cards.population"), view.population),
            Row::line(view.citizens.clone()),
            Row::Line(Line::new(view.growth.clone()).tone(growth_tone)),
            Row::bar(t!("classic.observer-cards.food"), format!("{} / {}", view.food_stored, view.food_needed), percent(view.food_stored, view.food_needed)), // i18n-ok: two numbers
        ],
    )
}

/// The defence rows share the screen's list of yields; the card shows them apart.
fn defense_rows() -> [&'static str; 2] {
    [t!("classic.game.city.row-defence"), t!("classic.game.city.row-hit-points")]
}

fn yields(view: &CityView) -> Section {
    let defense = defense_rows();
    let rows = view.yields.iter().filter(|(label, _)| !defense.contains(label));
    let rows = rows.map(|(label, value)| {
        let tone = if value.starts_with('-') && value != "-0" { Tone::Bad } else { Tone::Plain };
        Row::toned(*label, value, tone)
    });
    Section::new(t!("classic.observer-cards.yields"), rows.collect())
}

fn production(view: &CityView) -> Section {
    let mut rows = Vec::new();
    if view.queue.is_empty() {
        rows.push(Row::note(view.idle_production.clone()));
    }
    rows.push(Row::pair(t!("classic.observer-cards.stored"), view.production_stored));
    rows.push(Row::pair(t!("classic.observer-cards.per-turn"), view.production_per_turn));
    let rate = view.production_per_turn.max(0) as u32;
    for (i, entry) in view.queue.iter().take(QUEUE_ROWS).enumerate() {
        // The head waits for what is left of its cost; the others for their whole cost.
        let turns = if i == 0 {
            (rate > 0).then(|| entry.cost.saturating_sub(view.production_stored).div_ceil(rate).max(1))
        } else {
            view.options.iter().find(|o| o.name == entry.name).and_then(|o| o.turns)
        };
        let value = match turns {
            Some(turns) => tf!("classic.observer-cards.queue-turns", n = turns, cost = entry.cost),
            None => entry.cost.to_string(),
        };
        rows.push(Row::pair(entry.name.clone(), value));
    }
    if view.queue.len() > QUEUE_ROWS {
        rows.push(Row::note(tf!("classic.observer-cards.and-more", n = view.queue.len() - QUEUE_ROWS)));
    }
    Section::new(t!("classic.observer-cards.production"), rows)
}

fn buildings(view: &CityView) -> Section {
    if view.buildings.is_empty() {
        return Section::new(t!("classic.observer-cards.buildings"), vec![Row::note(t!("classic.observer-cards.none-yet"))]);
    }
    let names: Vec<&str> = view.buildings.iter().map(|b| b.name.as_str()).collect();
    Section::new(t!("classic.observer-cards.buildings"), vec![Row::line(names.join(", "))])
}

/// The specialists at work (none: no section).
fn specialists(view: &CityView) -> Section {
    let rows = view.specialists.iter().filter(|s| s.working > 0);
    let rows = rows.map(|s| Row::pair(s.name.clone(), tf!("classic.observer-cards.of", have = s.working, total = s.slots)));
    Section::new(t!("classic.observer-cards.specialists"), rows.collect())
}

/// The great person points of the city (none: no section).
fn great_people(view: &CityView) -> Section {
    let rows = view.great_people.iter().map(|g| {
        let label = if g.per_turn > 0 { tf!("classic.observer-cards.per-turn-gain", name = g.name.as_str(), n = g.per_turn) } else { g.name.clone() };
        Row::bar(label, format!("{} / {}", g.points, g.threshold), percent(g.points, g.threshold)) // i18n-ok: two numbers
    });
    Section::new(t!("classic.observer-cards.great-people"), rows.collect())
}

fn defense(view: &CityView) -> Section {
    let defense = defense_rows();
    let rows = view.yields.iter().filter(|(label, _)| defense.contains(label));
    Section::new(t!("classic.observer-cards.defense"), rows.map(|(label, value)| Row::pair(*label, value)).collect())
}
