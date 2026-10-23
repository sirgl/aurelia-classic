//! Read-only hover presentation of the observer's projected map.
use super::{GOLD, LIGHT, MUTED, panel, text};
use crate::ui::model::{CombatOdds, Outlook};
use render::gpu::buffers::terrain_icon_food_prod;
use render::world::state::{FeatureKind, RenderWorld};
use client::shell::observer::{Card, Line, Row, Section, Tone};
use egui::{Align2, Pos2, Rect, pos2, vec2};
use i18n::{t, tf};
use std::sync::Mutex;

#[derive(Debug, PartialEq, Eq)]
struct TileInfo {
    terrain: &'static str,
    remembered: bool,
    forest: bool,
    river: bool,
    resource: Option<&'static str>,
    /// Food, production, gold, science, culture, faith the observer knows.
    yields: [i32; 6],
    /// Feature other than forest (jungle, marsh, ...), improvement and natural wonder.
    feature: Option<&'static str>,
    improvement: Option<&'static str>,
    wonder: Option<&'static str>,
    owner: Option<u8>,
}

/// Display names of the game's compiled ruleset, by table index (one source of
/// ids for the whole game: the projection carries these indexes).
struct Names {
    resources: Vec<String>,
    improvements: Vec<String>,
    wonders: Vec<String>,
    features: Vec<(FeatureKind, String)>,
    /// (map code, name) of every terrain.
    terrains: Vec<(u16, String)>,
}

/// The names in the active language, built once per language (and per catalogue change) and
/// kept for the life of the process, like the text of a message.
fn names() -> &'static Names {
    static NAMES: Mutex<Vec<((String, u64), &'static Names)>> = Mutex::new(Vec::new());
    let (language, _scope, generation) = i18n::context();
    let mut cache = NAMES.lock().unwrap();
    if let Some((_, names)) = cache.iter().find(|((l, g), _)| *l == language && *g == generation) {
        return names;
    }
    let rules = crate::ui::screens::game_rules();
    let name = |kind: &str, key: &str, fallback: &str| i18n::content("classic", kind, key, "name", fallback);
    let names: &'static Names = Box::leak(Box::new(Names {
        resources: rules.resources.iter().map(|r| name("resource", &r.key, &r.name)).collect(),
        improvements: rules.improvements.iter().map(|i| name("improvement", &i.key, &i.name)).collect(),
        wonders: rules.natural_wonders.iter().map(|w| name("natural-wonder", &w.key, &w.name)).collect(),
        features: rules.features.iter().enumerate()
            .map(|(i, f)| (FeatureKind::of(&rules, classic_rules::ids::FeatureId(i as u16)), name("feature", &f.key, &f.name)))
            .collect(),
        terrains: rules.terrains.iter().filter_map(|t| Some((t.map_code?, name("terrain", &t.key, &t.name)))).collect(),
    }));
    cache.push(((language, generation), names));
    names
}

fn resource_names() -> &'static [String] {
    &names().resources
}

fn terrain_name(id: u16) -> &'static str {
    let found = names().terrains.iter().find(|(code, _)| *code == id);
    found.map(|(_, name)| name.as_str()).unwrap_or(t!("classic.ui.tile-tooltip.unknown-terrain"))
}

/// The ruleset name of a render feature kind (forest is shown on its own line).
fn feature_name(kind: FeatureKind) -> Option<&'static str> {
    if matches!(kind, FeatureKind::None | FeatureKind::Forest | FeatureKind::Other) {
        return None;
    }
    let found = names().features.iter().find(|(feature, _)| *feature == kind);
    found.map(|(_, name)| name.as_str())
}
fn tile_info(world: &RenderWorld, tile: u32) -> Option<TileInfo> {
    if tile >= world.width.saturating_mul(world.height) {
        return None;
    }
    let index = tile as usize;
    let visible = world.visible.get(index).copied().unwrap_or(0) != 0;
    if !visible && world.explored.get(index).copied().unwrap_or(0) == 0 {
        return None;
    }
    // Row-major fast path; sparse projections must match the requested tile ID too.
    let cell = world
        .terrain
        .get(index)
        .filter(|c| c.tile.0 == tile)
        .or_else(|| world.terrain.iter().find(|c| c.tile.0 == tile))?;
    let feature = world.feature.get(index).copied().filter(|_| visible).and_then(feature_name);
    let yields = match world.yields.get(index) {
        Some(y) => y.map(i32::from),
        None => {
            let (food, production) = terrain_icon_food_prod(cell.terrain);
            [food, production, 0, 0, 0, 0]
        }
    };
    // Explored land out of sight shows the live deposit its technology unlocks (R355).
    let resource = cell
        .resource
        .filter(|id| world.unlocked_resources.contains(id))
        .and_then(|id| resource_names().get(id as usize))
        .map(String::as_str);
    Some(TileInfo {
        terrain: terrain_name(cell.terrain),
        remembered: !visible,
        // Trees of a jungle are not a forest: the feature line names the jungle.
        forest: visible && feature.is_none() && world.forest.get(index).copied().unwrap_or(0) != 0,
        river: visible && world.river.get(index).copied().unwrap_or(0) != 0,
        resource,
        yields,
        feature,
        improvement: world
            .improvements
            .get(index)
            .copied()
            .flatten()
            .and_then(|i| names().improvements.get(i as usize))
            .map(String::as_str),
        wonder: world
            .natural_wonders
            .get(index)
            .copied()
            .flatten()
            .and_then(|w| names().wonders.get(w as usize))
            .map(String::as_str),
        owner: cell.owner,
    })
}
fn bounds(screen: Rect, cursor: Pos2, height: f32) -> Rect {
    let width = 258.0_f32.min((screen.width() - 16.).max(1.));
    let height = height.min((screen.height() - 16.).max(1.));
    let mut x = cursor.x + 18.;
    let mut y = cursor.y + 22.;
    if x + width > screen.right() - 8. {
        x = cursor.x - width - 18.;
    }
    if y + height > screen.bottom() - 8. {
        y = cursor.y - height - 18.;
    }
    Rect::from_min_size(
        pos2(
            x.clamp(
                screen.left() + 8.,
                (screen.right() - width - 8.).max(screen.left() + 8.),
            ),
            y.clamp(
                screen.top() + 8.,
                (screen.bottom() - height - 8.).max(screen.top() + 8.),
            ),
        ),
        vec2(width, height),
    )
}
/// `owners`: nation name by player index (tile owners).
pub fn draw(ctx: &egui::Context, world: &RenderWorld, tile: Option<u32>, owners: &[String]) {
    let Some(cursor) = ctx.pointer_hover_pos() else {
        return;
    };
    let Some(info) = tile.and_then(|tile| tile_info(world, tile)) else {
        return;
    };
    let screen = ctx.content_rect();
    let details = detail_lines(&info, owners);
    let base = match (info.remembered, info.resource) {
        (true, None) => 112.,
        (true, Some(_)) => 134.,
        (false, _) => 155.,
    };
    let r = bounds(screen, cursor, base + details.len() as f32 * 20.);
    // Painter-only overlay registers no widget, so it never captures the pointer or selects tiles.
    let painter = ctx
        .layer_painter(egui::LayerId::new(
            egui::Order::Tooltip,
            "tile_hover".into(),
        ))
        .with_clip_rect(screen);
    panel(&painter, r, 6.);
    let at = r.min + vec2(15., 12.);
    text(&painter, at, Align2::LEFT_TOP, info.terrain, 19., LIGHT);
    text(
        &painter,
        at + vec2(0., 27.),
        Align2::LEFT_TOP,
        status_line(&info),
        11.,
        MUTED,
    );
    let mut y = 51.;
    if !info.remembered {
        text(
            &painter,
            at + vec2(0., y),
            Align2::LEFT_TOP,
            forest_river_line(&info),
            13.,
            LIGHT,
        );
        y += 22.;
        text(
            &painter,
            at + vec2(0., y),
            Align2::LEFT_TOP,
            resource_line(&info),
            13.,
            GOLD,
        );
        y += 26.;
    }
    if info.remembered {
        if let Some(resource) = info.resource {
            text(&painter, at + vec2(0., y), Align2::LEFT_TOP, resource, 13., GOLD);
            y += 22.;
        }
    }
    for line in &details {
        text(&painter, at + vec2(0., y), Align2::LEFT_TOP, line, 13., LIGHT);
        y += 20.;
    }
    text(
        &painter,
        at + vec2(0., y),
        Align2::LEFT_TOP,
        t!("classic.ui.tile-tooltip.yields"),
        11.,
        MUTED,
    );
    text(
        &painter,
        at + vec2(0., y + 18.),
        Align2::LEFT_TOP,
        yields_line(info.yields),
        13.,
        LIGHT,
    );
}
/// What the tooltip's second line says: whether the tile is in sight or only remembered.
fn status_line(info: &TileInfo) -> &'static str {
    if info.remembered {
        t!("classic.ui.tile-tooltip.last-seen")
    } else {
        t!("classic.ui.tile-tooltip.visible-terrain")
    }
}

/// Forest and river of a tile in sight.
fn forest_river_line(info: &TileInfo) -> &'static str {
    match (info.forest, info.river) {
        (true, true) => t!("classic.ui.tile-tooltip.forest-river"),
        (true, false) => t!("classic.ui.tile-tooltip.forest"),
        (false, true) => t!("classic.ui.tile-tooltip.river"),
        _ => t!("classic.ui.tile-tooltip.no-forest-river"),
    }
}

/// The resource of a tile in sight, or the note that none is revealed.
fn resource_line(info: &TileInfo) -> &'static str {
    info.resource.unwrap_or(t!("classic.ui.tile-tooltip.no-resource"))
}

/// The observer's card of a tile: what `draw` shows, as data (title, status, forest and
/// river, resource, wonder, feature, improvement, owner, yields), read from the same
/// `TileInfo`. A tile the viewer has not explored has none; a remembered one shows what
/// was last seen. `owners`: nation name by player index.
pub fn card(world: &RenderWorld, tile: u32, owners: &[String]) -> Option<Card> {
    let info = tile_info(world, tile)?;
    let mut facts = Vec::new();
    if !info.remembered {
        facts.push(Row::line(forest_river_line(&info)));
        facts.push(Row::Line(Line::new(resource_line(&info)).tone(Tone::Accent)));
    } else if let Some(resource) = info.resource {
        facts.push(Row::Line(Line::new(resource).tone(Tone::Accent)));
    }
    facts.extend(detail_lines(&info, owners).into_iter().map(Row::line));
    let mut card = Card::new(info.terrain).subtitle(status_line(&info));
    if !facts.is_empty() {
        card = card.section(Section::plain(facts));
    }
    Some(card.section(Section::new(t!("classic.ui.tile-tooltip.yields"), vec![Row::line(yields_line(info.yields))])))
}

/// "3 Food · 2 Culture": the yields above zero, food and production always.
fn yields_line(yields: [i32; 6]) -> String {
    let parts: Vec<String> = yields
        .iter()
        .zip([
            i18n::key!("classic.ui.tile-tooltip.yield-food"),
            i18n::key!("classic.ui.tile-tooltip.yield-production"),
            i18n::key!("classic.ui.tile-tooltip.yield-gold"),
            i18n::key!("classic.ui.tile-tooltip.yield-science"),
            i18n::key!("classic.ui.tile-tooltip.yield-culture"),
            i18n::key!("classic.ui.tile-tooltip.yield-faith"),
        ])
        .enumerate()
        .filter(|(i, (amount, _))| *i < 2 || **amount > 0)
        .map(|(_, (amount, key))| i18n::tr_args(key, &[("amount", (*amount).into())]))
        .collect();
    parts.join("  ·  ")
}
const FAVORABLE: egui::Color32 = egui::Color32::from_rgb(137, 188, 83);
const UNFAVORABLE: egui::Color32 = egui::Color32::from_rgb(214, 96, 84);

/// Attack odds of the selected unit on the hovered enemy: strengths, expected damage
/// both ways (lowest–highest) and the verdict.
pub fn draw_odds(ctx: &egui::Context, odds: &CombatOdds) {
    let Some(cursor) = ctx.pointer_hover_pos() else {
        return;
    };
    let screen = ctx.content_rect();
    let r = bounds(screen, cursor, 196.);
    let painter = ctx
        .layer_painter(egui::LayerId::new(egui::Order::Tooltip, "combat_odds".into()))
        .with_clip_rect(screen);
    panel(&painter, r, 6.);
    let at = r.min + vec2(15., 12.);
    let kind = if odds.ranged { t!("classic.ui.tile-tooltip.ranged-attack") } else { t!("classic.ui.tile-tooltip.melee-attack") };
    text(&painter, at, Align2::LEFT_TOP, kind, 10., GOLD);
    let color = match odds.outlook {
        Outlook::Favorable => FAVORABLE,
        Outlook::Even => GOLD,
        Outlook::Unfavorable => UNFAVORABLE,
    };
    text(&painter, at + vec2(0., 16.), Align2::LEFT_TOP, odds.verdict, 20., color);
    let half = (r.width() - 30.) / 2.;
    let ours = Side {
        name: &odds.attacker,
        strength: &odds.attacker_strength,
        hp: odds.attacker_hp,
        damage: odds.to_attacker,
    };
    let theirs = Side {
        name: &odds.defender,
        strength: &odds.defender_strength,
        hp: odds.defender_hp,
        damage: odds.to_defender,
    };
    odds_column(&painter, at + vec2(0., 50.), half, ours);
    odds_column(&painter, at + vec2(half + 4., 50.), half, theirs);
}

/// One side of the attack: its name, strength, hit points and the damage it takes
/// (lowest, expected, highest).
struct Side<'a> {
    name: &'a str,
    strength: &'a str,
    hp: u16,
    damage: (u16, u16, u16),
}

fn odds_column(painter: &egui::Painter, at: Pos2, width: f32, side: Side<'_>) {
    let Side {
        name,
        strength,
        hp,
        damage,
    } = side;
    let name = painter.layout_no_wrap(name.to_owned(), egui::FontId::proportional(14.), LIGHT);
    let clipped = painter.with_clip_rect(Rect::from_min_size(at, vec2(width - 4., 20.)));
    clipped.galley(at, name, LIGHT);
    let lines = [
        (tf!("classic.ui.tile-tooltip.strength", strength = strength), GOLD),
        (tf!("classic.ui.tile-tooltip.hp", hp = hp), LIGHT),
        (tf!("classic.ui.tile-tooltip.takes-damage", n = damage.1), LIGHT),
        (tf!("classic.ui.tile-tooltip.possible", low = damage.0, high = damage.2), MUTED),
    ];
    for (i, (line, color)) in lines.into_iter().enumerate() {
        text(painter, at + vec2(0., 24. + i as f32 * 20.), Align2::LEFT_TOP, line, 12., color);
    }
    let left = hp.saturating_sub(damage.1);
    let bar = Rect::from_min_size(at + vec2(0., 110.), vec2(width - 10., 5.));
    painter.rect_filled(bar, 2., egui::Color32::from_rgb(48, 67, 77));
    let share = f32::from(left) / f32::from(hp.max(1));
    let filled = Rect::from_min_size(bar.min, vec2(bar.width() * share, 5.));
    let tint = if share > 0.5 { FAVORABLE } else { UNFAVORABLE };
    painter.rect_filled(filled, 2., tint);
}

/// Feature, improvement, natural wonder and owner lines of a tile.
fn detail_lines(info: &TileInfo, owners: &[String]) -> Vec<String> {
    let mut lines = Vec::new();
    lines.extend(info.wonder.map(|w| tf!("classic.ui.tile-tooltip.natural-wonder", name = w)));
    lines.extend(info.feature.map(str::to_owned));
    lines.extend(info.improvement.map(|i| tf!("classic.ui.tile-tooltip.improvement", name = i)));
    if let Some(owner) = info.owner.and_then(|o| owners.get(o as usize)) {
        lines.push(tf!("classic.ui.tile-tooltip.territory", owner = owner.as_str()));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use classic_rules::ids::TileId;
    use render::world::state::TerrainCell;
    fn world() -> RenderWorld {
        RenderWorld {
            width: 1,
            height: 1,
            terrain: vec![TerrainCell {
                tile: TileId(0),
                terrain: 2,
                owner: None,
                resource: Some(0),
            }],
            visible: vec![1],
            explored: vec![1],
            forest: vec![1],
            river: vec![1],
            unlocked_resources: vec![0],
            ..Default::default()
        }
    }
    #[test]
    fn visible_resource_uses_compiled_catalog_and_unlock_filter() {
        let mut w = world();
        let info = tile_info(&w, 0).unwrap();
        assert_eq!(info.resource, Some(resource_names()[0].as_str()));
        assert_eq!(info.resource, Some("Aluminum"), "first BNW resource by key");
        let tundra = classic_rules::rules::roles::Ground::Flat(classic_rules::rules::roles::Climate::Cold);
        assert_eq!(terrain_name(w.terrain_kinds.palette().code(tundra)), "Tundra", "every map terrain has its ruleset name");
        assert!(info.forest && info.river);
        let (food, production) = terrain_icon_food_prod(2);
        assert_eq!(info.yields, [food, production, 0, 0, 0, 0], "no yield layer: the bare terrain");
        assert_eq!(yields_line([3, 0, 0, 0, 2, 0]), "3 Food  ·  0 Production  ·  2 Culture");
        w.unlocked_resources.clear();
        assert_eq!(tile_info(&w, 0).unwrap().resource, None);
        w.terrain[0].resource = Some(u16::MAX);
        w.unlocked_resources.push(u16::MAX);
        assert_eq!(tile_info(&w, 0).unwrap().resource, None);
    }
    #[test]
    fn remembered_hides_forest_and_river_but_shows_the_unlocked_deposit_unknown_and_invalid_tiles_are_absent() {
        let mut w = world();
        w.visible[0] = 0;
        let info = tile_info(&w, 0).unwrap();
        assert!(info.remembered);
        assert!(!info.forest && !info.river);
        assert_eq!(info.resource, Some("Aluminum"), "R355: the live deposit of explored land, its technology known");
        w.unlocked_resources.clear();
        assert_eq!(tile_info(&w, 0).unwrap().resource, None, "locked: nothing, as in sight");
        w.unlocked_resources.push(0);
        w.forest[0] = 0;
        w.river[0] = 0;
        assert_eq!(tile_info(&w, 0), Some(info));
        w.explored[0] = 0;
        assert!(tile_info(&w, 0).is_none());
        assert!(tile_info(&w, 1).is_none());
    }
    #[test]
    fn the_observer_card_says_what_the_tooltip_says() {
        let mut w = world();
        w.terrain[0].owner = Some(1);
        let owners = ["Rome".to_owned(), "Egypt".to_owned()];
        let card = super::card(&w, 0, &owners).unwrap();
        let info = tile_info(&w, 0).unwrap();
        assert_eq!(card.title, info.terrain);
        assert_eq!(card.subtitle.as_deref(), Some("Visible terrain"));
        let lines: Vec<&Row> = card.rows().collect();
        assert_eq!(lines[0], &Row::line("Forest · River"));
        assert_eq!(lines[1], &Row::Line(Line::new("Aluminum").tone(Tone::Accent)));
        assert_eq!(lines[2], &Row::line("Territory of Egypt"));
        assert_eq!(lines[3], &Row::line(yields_line(info.yields)));
        w.visible[0] = 0;
        let card = super::card(&w, 0, &owners).unwrap();
        assert_eq!(card.subtitle.as_deref(), Some("Last seen"));
        assert!(!card.rows().any(|row| matches!(row, Row::Line(line) if line.text.contains("orest"))));
        assert!(card.rows().any(|row| row == &Row::Line(Line::new("Aluminum").tone(Tone::Accent))), "the deposit of a remembered tile");
        assert!(!card.rows().any(|row| matches!(row, Row::Line(line) if line.text.contains("No revealed resource"))));
        w.explored[0] = 0;
        assert!(super::card(&w, 0, &owners).is_none());
    }

    #[test]
    fn native_pointer_hover_draws_without_capture_or_mutation_and_leaving_hides() {
        let ctx = egui::Context::default();
        let w = world();
        let before = w.terrain.clone();
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(640., 480.));
        let run = |events| {
            ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events,
                    ..Default::default()
                },
                |ui| draw(ui.ctx(), &w, Some(0), &[]),
            )
        };
        let mut shown = run(vec![egui::Event::PointerMoved(pos2(630., 470.))]);
        shown.textures_delta.clear();
        assert!(!shown.shapes.is_empty());
        assert!(!ctx.egui_wants_pointer_input());
        assert_eq!(w.terrain, before);
        let mut hidden = run(vec![egui::Event::PointerGone]);
        hidden.textures_delta.clear();
        assert!(hidden.shapes.len() < shown.shapes.len());
        assert!(
            screen
                .shrink(7.)
                .contains_rect(bounds(screen, pos2(630., 470.), 155.))
        );
    }
}
