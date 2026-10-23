//! New game setup (classic style): civilization and leader with their unique
//! ability, units and buildings; map type, size, sea level, difficulty,
//! speed, rivals, victory conditions and seed; then a loading screen while
//! the world is made off the UI thread. Every choice is a real generator or
//! game input; choices the core cannot honour yet are shown disabled with
//! the reason.
use super::*;
use crate::ui::pedia::{self, LineKind};
use classic_rules::{CompiledRuleset, GameState, ids::PlayerId, scenario::{MapScript, ScenarioConfig}};
use client::shell::observer::Watch;
use crate::game::names;
use i18n::{t, tf};
use std::sync::{Arc, OnceLock};

pub fn rules() -> Arc<CompiledRuleset> {
    static RULES: OnceLock<Arc<CompiledRuleset>> = OnceLock::new();
    RULES
        .get_or_init(|| {
            Arc::new(classic_rules::compile_rules(classic_content::core::core_rules()).expect("core rules"))
        })
        .clone()
}

/// World sizes: dimensions, civilizations and city-states (own values, unverified).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WorldSize {
    Duel,
    Tiny,
    Small,
    Standard,
    Large,
    Huge,
}

impl WorldSize {
    pub const ALL: [WorldSize; 6] = [
        WorldSize::Duel,
        WorldSize::Tiny,
        WorldSize::Small,
        WorldSize::Standard,
        WorldSize::Large,
        WorldSize::Huge,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Duel => t!("classic.ui.match-setup.size-duel"),
            Self::Tiny => t!("classic.ui.match-setup.size-tiny"),
            Self::Small => t!("classic.ui.match-setup.size-small"),
            Self::Standard => t!("classic.ui.match-setup.size-standard"),
            Self::Large => t!("classic.ui.match-setup.size-large"),
            Self::Huge => t!("classic.ui.match-setup.size-huge"),
        }
    }

    pub fn dimensions(self) -> (u32, u32) {
        match self {
            Self::Duel => (40, 24),
            Self::Tiny => (56, 36),
            Self::Small => (66, 42),
            Self::Standard => (80, 52),
            Self::Large => (104, 64),
            Self::Huge => (128, 80),
        }
    }

    /// Civilizations of the size, the player included.
    pub fn civilizations(self) -> u8 {
        match self {
            Self::Duel => 2,
            Self::Tiny => 4,
            Self::Small => 6,
            Self::Standard => 8,
            Self::Large => 10,
            Self::Huge => 12,
        }
    }

    pub fn city_states(self) -> u8 {
        match self {
            Self::Duel => 4,
            Self::Tiny => 8,
            Self::Small => 12,
            Self::Standard => 16,
            Self::Large => 20,
            Self::Huge => 24,
        }
    }
}

/// Map size of a request: a world size, or one of the compact worlds the
/// tests and benches use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapSize {
    Small,
    Medium,
    Large,
    World(WorldSize),
}
impl MapSize {
    pub fn dimensions(self) -> (u32, u32) {
        match self {
            Self::Small => (24, 20),
            Self::Medium => (32, 24),
            Self::Large => (40, 30),
            Self::World(size) => size.dimensions(),
        }
    }
}

/// Sea level of the setup: the water share the Fractal script keeps (other
/// scripts add their ocean, `MapScript::water_ratio`).
const SEA_LEVELS: [u8; 3] = [10, 25, 40];

/// The name of a sea level (10, 25 or 40) in the active language.
fn sea_label(level: u8) -> &'static str {
    match level {
        10 => t!("classic.ui.match-setup.sea-low"),
        25 => t!("classic.ui.match-setup.sea-medium"),
        _ => t!("classic.ui.match-setup.sea-high"),
    }
}
/// Civilizations of a game, the player included (the core holds up to 255
/// players; more than this would only slow the AI turns down).
const MAX_CIVILIZATIONS: u8 = 16;
/// City-states the setup offers at most (the Huge world's city-states: 24).
const MAX_CITY_STATES: u8 = 24;

#[derive(Clone, Debug)]
pub struct MatchRequest {
    pub civilization: String,
    pub seed: u64,
    pub size: MapSize,
    /// Civilizations, the player included.
    pub players: u8,
    /// Sea level (10, 25 or 40).
    pub water: u8,
}

/// Victory conditions of the setup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Victories {
    pub domination: bool,
    pub science: bool,
    pub culture: bool,
    pub diplomatic: bool,
    pub time: bool,
}

impl Default for Victories {
    fn default() -> Self {
        Self { domination: true, science: true, culture: true, diplomatic: true, time: true }
    }
}

/// Game speed. The rules core has no speed scaling yet, so only Standard runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum GameSpeed {
    Quick,
    #[default]
    Standard,
    Epic,
    Marathon,
}

impl GameSpeed {
    pub const ALL: [GameSpeed; 4] = [GameSpeed::Quick, GameSpeed::Standard, GameSpeed::Epic, GameSpeed::Marathon];

    pub fn label(self) -> &'static str {
        match self {
            Self::Quick => t!("classic.ui.match-setup.speed-quick"),
            Self::Standard => t!("classic.ui.match-setup.speed-standard"),
            Self::Epic => t!("classic.ui.match-setup.speed-epic"),
            Self::Marathon => t!("classic.ui.match-setup.speed-marathon"),
        }
    }

    /// Whether the core plays this speed (only Standard: no speed scaling yet).
    pub fn available(self) -> bool {
        self == GameSpeed::Standard
    }
}

/// Choices of the game beyond the world itself; the session keeps them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameOptions {
    /// Difficulty level key (`core:handicap/prince`).
    pub difficulty: String,
    pub map: MapScript,
    pub speed: GameSpeed,
    pub victories: Victories,
    /// City-states placed on the map.
    pub city_states: u8,
    /// Barbarian camps and their units.
    pub barbarians: bool,
}

impl Default for GameOptions {
    fn default() -> Self {
        Self {
            difficulty: rules().default_difficulty().into(),
            map: MapScript::Fractal,
            speed: GameSpeed::Standard,
            victories: Victories::default(),
            city_states: 0,
            barbarians: false,
        }
    }
}

impl Victories {
    /// The core's victory options (Domination is always on in this build).
    pub fn options(self) -> classic_rules::world::victory::VictoryOptions {
        use classic_rules::world::victory::{VictoryKind, VictoryOptions};
        VictoryOptions::default()
            .with(VictoryKind::Science, self.science)
            .with(VictoryKind::Cultural, self.culture)
            .with(VictoryKind::Diplomatic, self.diplomatic)
            .with(VictoryKind::Time, self.time)
    }
}

/// Everything the setup screen asks for.
#[derive(Clone, Debug)]
pub struct NewGame {
    pub request: MatchRequest,
    pub options: GameOptions,
}

impl From<MatchRequest> for NewGame {
    fn from(request: MatchRequest) -> Self {
        Self { request, options: GameOptions::default() }
    }
}

/// Who plays the new game's first civilization.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Seat {
    /// The player does.
    #[default]
    Play,
    /// The AI does, like every other civilization; the player only watches, on the grid
    /// of every civilization or on the first one alone.
    Observe(Watch),
}

/// What the setup screen asked for.
#[derive(Clone, Debug)]
pub struct MatchOrder {
    pub game: NewGame,
    pub seat: Seat,
}

pub struct PreparedMatch {
    pub game: GameState,
    pub world: RenderWorld,
    pub civilization: String,
    pub options: GameOptions,
}

/// A world of the Fractal script at the default difficulty (tests, benches).
pub fn prepare(request: MatchRequest) -> Result<PreparedMatch, String> {
    prepare_game(NewGame::from(request))
}

pub fn prepare_game(new_game: NewGame) -> Result<PreparedMatch, String> {
    let NewGame { request, options } = new_game;
    let rules = rules();
    let civ = rules
        .civs
        .iter()
        .position(|c| c.key == request.civilization)
        .ok_or(t!("classic.ui.match-setup.error-nation"))?;
    let sea_level = SEA_LEVELS.contains(&request.water);
    if !(2..=MAX_CIVILIZATIONS).contains(&request.players) || !sea_level {
        return Err(t!("classic.ui.match-setup.error-world-settings").into());
    }
    if rules.handicap(&options.difficulty).is_none() {
        return Err(t!("classic.ui.match-setup.error-difficulty").into());
    }
    if !options.speed.available() {
        return Err(t!("classic.ui.match-setup.error-speed").into());
    }
    let (width, height) = request.size.dimensions();
    let config = ScenarioConfig {
        seed: request.seed,
        players: request.players,
        width,
        height,
        cities: 0,
        units: 0,
        water_ratio: options.map.water_ratio(request.water),
    };
    let new_game = classic_rules::scenario::NewGameOptions {
        barbarians: if options.barbarians {
            classic_rules::world::barbarians::BarbarianMode::Normal
        } else {
            classic_rules::world::barbarians::BarbarianMode::None
        },
        city_states: options.city_states,
        victories: options.victories.options(),
        script: options.map,
    };
    let game = classic_rules::scenario::generate_new_game_with(&rules, &config, Some(civ as u8), new_game);
    game.validate(&rules)
        .map_err(|e| tf!("classic.ui.match-setup.error-create", error = format!("{e:?}")))?;
    // The projection is the observer's view: unknown geography is already masked.
    let mut world =
        render::world::projection::rebuild_render_world(game.view(), PlayerId(0), &rules);
    world.visual_surface = true;
    Ok(PreparedMatch {
        game,
        world,
        civilization: names::nation(&rules, civ as u8),
        options,
    })
}

/// Difficulty levels a player may choose (Settler … Deity), by index.
fn difficulties(rules: &CompiledRuleset) -> Vec<(String, String)> {
    let mut levels: Vec<_> = rules
        .handicaps
        .iter()
        .filter(|h| Some(h.key.as_str()) != rules.ai_handicap().map(|ai| ai.key.as_str()))
        .collect();
    levels.sort_by_key(|h| h.index);
    levels.into_iter().map(|h| (h.key.clone(), names::named("handicap", &h.name, &h.key))).collect()
}

pub(super) struct MatchSetup {
    civilization: String,
    seed: String,
    size: WorldSize,
    map: MapScript,
    ai_civilizations: u8,
    city_states: u8,
    water: u8,
    difficulty: String,
    speed: GameSpeed,
    victories: Victories,
    pub loading: bool,
    pub error: Option<String>,
    pending: Option<NewGame>,
    seat: Seat,
    /// What is being created (the loading screen shows it).
    creating: Option<NewGame>,
    artwork: civilizations::CivilizationArt,
    selection_scroll_frames: u8,
    pub(super) rules_tab: bool,
}
impl Default for MatchSetup {
    fn default() -> Self {
        let size = WorldSize::Small;
        Self {
            civilization: "core:nation/rome".into(),
            seed: "20260928".into(),
            size,
            map: MapScript::Continents,
            ai_civilizations: size.civilizations() - 1,
            city_states: size.city_states(),
            water: 25,
            difficulty: rules().default_difficulty().into(),
            speed: GameSpeed::Standard,
            victories: Victories::default(),
            loading: false,
            error: None,
            pending: None,
            seat: Seat::Play,
            creating: None,
            artwork: Default::default(),
            selection_scroll_frames: 3,
            rules_tab: false,
        }
    }
}
impl MatchSetup {
    fn request(&self) -> Result<NewGame, String> {
        let seed = self
            .seed
            .trim()
            .parse::<u64>()
            .map_err(|_| t!("classic.ui.match-setup.error-seed"))?;
        let request = MatchRequest {
            civilization: self.civilization.clone(),
            seed,
            size: MapSize::World(self.size),
            players: self.ai_civilizations + 1,
            water: self.water,
        };
        let options = GameOptions {
            difficulty: self.difficulty.clone(),
            map: self.map,
            speed: self.speed,
            victories: self.victories,
            city_states: self.city_states,
            barbarians: true,
        };
        Ok(NewGame { request, options })
    }
    pub fn preload(&mut self, ctx: &egui::Context) {
        let rules = rules();
        let slugs: Vec<_> = rules
            .civs
            .iter()
            .map(|c| civilizations::art_key(c.key.rsplit('/').next().unwrap_or("")))
            .collect();
        let slugs: Vec<&str> = slugs.iter().map(String::as_str).collect();
        let selected = civilizations::art_key(self.civilization.rsplit('/').next().unwrap_or(""));
        self.artwork.preload(ctx, &slugs, &selected);
    }
    pub fn begin(&mut self, seat: Seat) -> bool {
        if self.loading {
            return false;
        }
        match self.request() {
            Ok(request) => {
                self.creating = Some(request.clone());
                self.pending = Some(request);
                self.seat = seat;
                self.loading = true;
                self.error = None;
                true
            }
            Err(error) => {
                self.error = Some(error);
                false
            }
        }
    }
    pub fn take_request(&mut self) -> Option<NewGame> {
        self.pending.take()
    }
    pub fn take_order(&mut self) -> Option<MatchOrder> {
        let seat = self.seat;
        self.take_request().map(|game| MatchOrder { game, seat })
    }
    /// A new world size brings its number of rivals.
    fn set_size(&mut self, size: WorldSize) {
        self.size = size;
        self.ai_civilizations = size.civilizations() - 1;
        self.city_states = size.city_states();
    }
}
pub(super) fn draw(ui: &mut egui::Ui, r: Rect, state: &mut MatchSetup, brightness: f32) {
    let rules = rules();
    if state.loading {
        draw_loading(ui, r, state, &rules, brightness);
        return;
    }
    if r.width() < 560. && r.height() > r.width() {
        // A phone held upright: the nations above, the world's form below, each at the full width
        // (three columns would leave every text at a third of its size).
        let split = (r.height() * 0.34).max(190.);
        let roster = Rect::from_min_max(r.min, pos2(r.right(), r.top() + split));
        let settings = Rect::from_min_max(pos2(r.left(), roster.bottom() + 8.), r.max);
        draw_roster(ui, roster, state, &rules, brightness);
        draw_world_settings(ui, settings, state);
        return;
    }
    let roster_width = (r.width() * 0.2).clamp(170., 240.);
    let settings_width = (r.width() * 0.3).clamp(290., 380.);
    let roster = Rect::from_min_max(r.min, pos2(r.left() + roster_width, r.bottom()));
    let settings = Rect::from_min_max(pos2(r.right() - settings_width, r.top()), r.max);
    let portrait = Rect::from_min_max(
        pos2(roster.right() + 18., r.top()),
        pos2(settings.left() - 18., r.bottom()),
    );
    draw_roster(ui, roster, state, &rules, brightness);
    draw_leader(ui, portrait, state, &rules, brightness);
    draw_world_settings(ui, settings, state);
}
fn draw_roster(
    ui: &mut egui::Ui,
    r: Rect,
    state: &mut MatchSetup,
    rules: &CompiledRuleset,
    brightness: f32,
) {
    panel(ui.painter(), r);
    text(
        ui.painter(),
        r.min + vec2(15., 17.),
        t!("classic.ui.match-setup.roster-heading"),
        13.,
        GOLD,
    );
    ui.painter().text(r.right_top() + vec2(-15., 17.), Align2::RIGHT_TOP, rules.civs.len(), FontId::proportional(13.), MUTED);
    let list = Rect::from_min_max(r.min + vec2(8., 46.), r.max - vec2(8., 12.));
    ui.scope_builder(egui::UiBuilder::new().max_rect(list), |ui| {
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt("match_civilizations")
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
            .max_height(list.height());
        if state.selection_scroll_frames > 0 {
            let selected = rules
                .civs
                .iter()
                .position(|c| c.key == state.civilization)
                .unwrap_or(0);
            scroll = scroll.vertical_scroll_offset(
                (selected as f32 * 72. - list.height() * 0.5 + 33.).max(0.),
            );
        }
        scroll.show(ui, |ui| {
            for civ in &rules.civs {
                let (row, response) =
                    ui.allocate_exact_size(vec2(list.width() - 12., 67.), Sense::click());
                let selected = state.civilization == civ.key;
                if ui.is_rect_visible(row) {
                    roster_row(ui, row, state, civ, selected, brightness);
                }
                if let Some(page) = pedia::game_pedia().by_key(&civ.key) {
                    crate::ui::pedia_link::mark(ui, row, page);
                }
                if response.clicked() && !state.loading {
                    state.civilization = civ.key.clone();
                }
                ui.add_space(5.);
            }
        });
    });
    state.selection_scroll_frames = state.selection_scroll_frames.saturating_sub(1);
}
fn roster_row(
    ui: &egui::Ui,
    row: Rect,
    state: &mut MatchSetup,
    civ: &classic_rules::rules::compiled::CivDefinition,
    selected: bool,
    brightness: f32,
) {
    ui.painter()
        .rect_filled(row, 4., if selected { TEAL } else { RAISED });
    if selected {
        ui.painter().rect_stroke(
            row,
            4.,
            Stroke::new(1., GOLD),
            egui::StrokeKind::Inside,
        );
    }
    let slug = civilizations::art_key(civ.key.rsplit('/').next().unwrap_or(""));
    let slug = slug.as_str();
    state.artwork.paint(
        ui,
        Rect::from_min_size(row.min + vec2(3., 7.), vec2(52., 52.)),
        slug,
        civilizations::Asset::Emblem,
        brightness,
    );
    text(ui.painter(), row.min + vec2(60., 14.), names::named("nation", &civ.name, &civ.key), 15., CREAM);
    paragraph(ui.painter(), row.min + vec2(60., 37.), &civ.leader, 10., MUTED, row.width() - 66.);
}
fn draw_leader(
    ui: &mut egui::Ui,
    r: Rect,
    state: &mut MatchSetup,
    rules: &CompiledRuleset,
    brightness: f32,
) {
    let Some(civ) = rules.civs.iter().find(|c| c.key == state.civilization) else {
        return;
    };
    let slug = civilizations::art_key(civ.key.rsplit('/').next().unwrap_or(""));
    let slug = slug.as_str();
    panel(ui.painter(), r);
    state.artwork.paint(
        ui,
        r.shrink(1.),
        slug,
        civilizations::Asset::Background,
        brightness,
    );
    darken(ui.painter(), r, 15, 175);
    let plaque_height = (r.height() * 0.42).clamp(220., 300.);
    let side = (r.width() * 0.62).min(r.height() - plaque_height - 40.).max(80.);
    let portrait = Rect::from_center_size(
        pos2(r.center().x, r.top() + 20. + side * 0.5),
        vec2(side, side),
    );
    ui.painter().rect_filled(portrait.expand(4.), 3., BG);
    ui.painter().rect_stroke(
        portrait.expand(4.),
        3.,
        Stroke::new(1.2, GOLD),
        egui::StrokeKind::Inside,
    );
    state.artwork.paint(
        ui,
        portrait,
        slug,
        civilizations::Asset::Portrait,
        brightness,
    );
    let plaque = Rect::from_min_max(
        pos2(r.left() + 14., r.bottom() - plaque_height),
        r.max - vec2(14., 14.),
    );
    draw_plaque(ui, plaque, state, civ, brightness);
}
/// Name, leader, unique ability and the civilization's unique units, buildings
/// and improvements, as the Encyclopedia derives them from the ruleset.
fn draw_plaque(
    ui: &mut egui::Ui,
    plaque: Rect,
    state: &mut MatchSetup,
    civ: &classic_rules::rules::compiled::CivDefinition,
    brightness: f32,
) {
    let slug = civilizations::art_key(civ.key.rsplit('/').next().unwrap_or(""));
    let slug = slug.as_str();
    ui.painter().rect_filled(plaque, 4., Color32::from_black_alpha(205));
    state.artwork.paint(
        ui,
        Rect::from_min_size(plaque.min + vec2(10., 12.), vec2(58., 58.)),
        slug,
        civilizations::Asset::Emblem,
        brightness,
    );
    text(ui.painter(), plaque.min + vec2(80., 12.), names::named("nation", &civ.name, &civ.key), 24., CREAM);
    text(ui.painter(), plaque.min + vec2(81., 44.), &civ.leader, 15., GOLD);
    let Some(entry) = pedia::game_pedia().by_key(&civ.key).and_then(|p| pedia::game_pedia().entry(p).cloned()) else {
        return;
    };
    let body = Rect::from_min_max(plaque.min + vec2(14., 80.), plaque.max - vec2(14., 10.));
    ui.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
        egui::ScrollArea::vertical()
            .id_salt(("civ_uniques", slug))
            .max_height(body.height())
            .auto_shrink([false, false])
            .show(ui, |ui| plaque_text(ui, &entry));
    });
}
fn plaque_text(ui: &mut egui::Ui, entry: &pedia::Entry) {
    let ability = civ_lines(entry);
    ui.label(egui::RichText::new(ability.0).size(14.).color(GOLD));
    for (line, active) in ability.1 {
        let color = if active { CREAM } else { MUTED };
        ui.label(egui::RichText::new(line).size(12.).color(color));
    }
    let pedia = pedia::game_pedia();
    for group in &entry.links {
        // The group labels are the Encyclopedia's own link names, not shown here.
        let kind = match group.label {
            "Unique units" => Unique::Unit,         // i18n-ok: pedia link group name
            "Unique buildings" => Unique::Building, // i18n-ok: pedia link group name
            "Unique improvements" => Unique::Improvement, // i18n-ok: pedia link group name
            _ => continue,
        };
        for target in &group.targets {
            let Some(unique) = pedia.entry(*target) else {
                continue;
            };
            ui.add_space(4.);
            let replaces = unique
                .links
                .iter()
                .find(|g| g.label == "Replaces") // i18n-ok: pedia link group name
                .and_then(|g| g.targets.first())
                .and_then(|t| pedia.entry(*t))
                .map(|e| e.name.as_str());
            let response = ui.label(
                egui::RichText::new(unique_line(kind, &unique.name, replaces)).size(12.).color(CREAM),
            );
            crate::ui::pedia_link::mark_response(ui, &response, unique.id);
            let stats: Vec<String> = unique.stats.iter().take(3).map(|s| format!("{} {}", s.label, s.value)).collect();
            if !stats.is_empty() {
                ui.label(egui::RichText::new(stats.join(" · ")).size(11.).color(MUTED));
            }
        }
    }
}
/// What a nation's unique entry on the card is.
#[derive(Clone, Copy)]
enum Unique {
    Unit,
    Building,
    Improvement,
}

/// "UNIQUE UNIT: Name" with the unit it replaces, if any, as one message.
fn unique_line(kind: Unique, name: &str, replaces: Option<&str>) -> String {
    match (kind, replaces) {
        (Unique::Unit, None) => tf!("classic.ui.match-setup.unique-unit", name = name),
        (Unique::Unit, Some(old)) => tf!("classic.ui.match-setup.unique-unit-replaces", name = name, old = old),
        (Unique::Building, None) => tf!("classic.ui.match-setup.unique-building", name = name),
        (Unique::Building, Some(old)) => tf!("classic.ui.match-setup.unique-building-replaces", name = name, old = old),
        (Unique::Improvement, None) => tf!("classic.ui.match-setup.unique-improvement", name = name),
        (Unique::Improvement, Some(old)) => {
            tf!("classic.ui.match-setup.unique-improvement-replaces", name = name, old = old)
        }
    }
}

/// The unique ability heading and its effect lines (inactive ones marked).
fn civ_lines(entry: &pedia::Entry) -> (String, Vec<(String, bool)>) {
    let mut heading = t!("classic.ui.match-setup.ability-heading").to_owned();
    let mut lines = Vec::new();
    for line in &entry.text {
        match line.kind {
            // The Encyclopedia entry's own heading line (rule data, not screen text).
            LineKind::Text if line.text.starts_with("Unique ability: ") => { // i18n-ok: pedia entry text
                heading = line.text.trim_start_matches("Unique ability: ").to_owned(); // i18n-ok: pedia entry text
            }
            LineKind::Effect => lines.push((line.text.clone(), true)),
            LineKind::Inactive => lines.push((tf!("classic.ui.match-setup.ability-inactive", line = line.text.as_str()), false)),
            _ => {}
        }
    }
    (heading, lines)
}
fn draw_world_settings(ui: &mut egui::Ui, r: Rect, state: &mut MatchSetup) {
    panel(ui.painter(), r);
    for (i, (label, selected)) in [(t!("classic.ui.match-setup.tab-world"), !state.rules_tab), (t!("classic.ui.match-setup.tab-rules"), state.rules_tab)].into_iter().enumerate() {
        let tab = Rect::from_min_size(r.min + vec2(14. + i as f32 * (r.width() - 28.) * 0.5, 14.), vec2((r.width() - 34.) * 0.5, 34.));
        if button(ui, tab, label, selected, true) { state.rules_tab = i == 1; }
    }
    let body = Rect::from_min_max(r.min + vec2(18., 64.), r.max - vec2(18., 100.));
    ui.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
        egui::ScrollArea::vertical()
            .id_salt("match_settings")
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
            .max_height(body.height())
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let width = ui.available_width();
                let (form, _) = ui.allocate_exact_size(vec2(width, 414.), Sense::hover());
                draw_setup_page(ui, form, state);
                if let Err(error) = state.request() {
                    ui.label(egui::RichText::new(error).color(GOLD).size(11.));
                }
                if let Some(error) = &state.error {
                    ui.label(egui::RichText::new(error).color(GOLD).size(11.));
                }
            });
    });
    let footer = create_button_rect(r);
    paragraph(
        ui.painter(),
        pos2(footer.left(), footer.top() - 34.),
        t!("classic.ui.match-setup.footer-hint"),
        11.,
        MUTED,
        footer.width(),
    );
    let [start, all, one] = footer_buttons(footer);
    let ready = !state.loading && state.request().is_ok();
    let words = &client::shell::words::current().observer;
    if game_control(ui, start, "create_world", t!("classic.ui.match-setup.start"), None, true, ready, false) {
        state.begin(Seat::Play);
    }
    if game_control(ui, all, "observe_world", words.watch_all, None, false, ready, false) {
        state.begin(Seat::Observe(Watch::All));
    }
    if game_control(ui, one, "observe_one", words.watch_one, None, false, ready, false) {
        state.begin(Seat::Observe(Watch::One));
    }
}
/// The footer's three buttons: start the game, or watch it played by the AI alone (every
/// civilization, or the first one).
fn footer_buttons(footer: Rect) -> [Rect; 3] {
    let (first, second) = (footer.left() + footer.width() * 0.4, footer.left() + footer.width() * 0.7);
    let part = |from: f32, to: f32| Rect::from_min_max(pos2(from, footer.top()), pos2(to, footer.bottom()));
    [part(footer.left(), first - 3.), part(first + 3., second - 3.), part(second + 3., footer.right())]
}
fn create_button_rect(r: Rect) -> Rect {
    Rect::from_min_size(
        pos2(r.left() + 18., r.bottom() - 59.),
        vec2(r.width() - 36., 41.),
    )
}

/// Rows of the world form (offsets from its top).
struct FormLayout {
    map: Vec<Rect>,
    size: Vec<Rect>,
    difficulty: Rect,
    speed: Vec<Rect>,
    ai: Rect,
    city_states: Rect,
    water: Vec<Rect>,
    victories: Vec<Rect>,
    seed: Rect,
    random: Rect,
}
#[cfg(test)]
const FORM_HEIGHT: f32 = 700.;

impl FormLayout {
    fn new(r: Rect) -> Self {
        let grid = |top: f32, count: usize, columns: usize, height: f32| -> Vec<Rect> {
            let width = (r.width() - 6. * (columns as f32 - 1.)) / columns as f32;
            (0..count)
                .map(|i| {
                    let (col, row) = ((i % columns) as f32, (i / columns) as f32);
                    Rect::from_min_size(
                        r.min + vec2(col * (width + 6.), top + row * (height + 6.)),
                        vec2(width, height),
                    )
                })
                .collect()
        };
        let stepper = |top: f32| Rect::from_min_size(pos2(r.right() - 176., r.top() + top), vec2(176., 34.));
        let seed = Rect::from_min_size(r.min + vec2(0., 640.), vec2(r.width() - 94., 38.));
        Self {
            map: grid(47., 5, 3, 34.),
            size: grid(147., 6, 3, 46.),
            difficulty: stepper(255.),
            speed: grid(325., 4, 4, 30.),
            ai: stepper(377.),
            city_states: stepper(419.),
            water: grid(481., 3, 3, 30.),
            victories: grid(545., 5, 3, 28.),
            seed,
            random: Rect::from_min_size(pos2(seed.right() + 6., seed.top()), vec2(88., 38.)),
        }
    }
}

fn map_label(map: MapScript) -> &'static str {
    match map {
        MapScript::Continents => t!("classic.ui.match-setup.map-continents"),
        MapScript::Pangaea => t!("classic.ui.match-setup.map-pangaea"),
        MapScript::Archipelago => t!("classic.ui.match-setup.map-archipelago"),
        MapScript::Fractal => t!("classic.ui.match-setup.map-fractal"),
        MapScript::SmallContinents => t!("classic.ui.match-setup.map-small-continents"),
    }
}

fn map_hint(map: MapScript) -> String {
    match map {
        MapScript::Continents => t!("classic.ui.match-setup.map-hint-continents").into(),
        MapScript::Pangaea => t!("classic.ui.match-setup.map-hint-pangaea").into(),
        MapScript::Archipelago => tf!(
            "classic.ui.match-setup.map-hint-archipelago",
            coast = names::named("tech", "", "core:tech/optics"),
            deep = names::named("tech", "", "core:tech/astronomy"),
        ),
        MapScript::Fractal => t!("classic.ui.match-setup.map-hint-fractal").into(),
        MapScript::SmallContinents => t!("classic.ui.match-setup.map-hint-small-continents").into(),
    }
}

// Flat control harness for focused keyboard and input contract tests.
#[cfg(test)]
fn draw_world_form(ui: &mut egui::Ui, r: Rect, state: &mut MatchSetup) {
    let layout = FormLayout::new(r);
    text(ui.painter(), r.min, t!("classic.ui.match-setup.world-title"), 21., CREAM);
    section_label(ui, r.min + vec2(0., 31.), t!("classic.ui.match-setup.section-map-type"));
    draw_map_types(ui, &layout, state);
    section_label(ui, r.min + vec2(0., 131.), t!("classic.ui.match-setup.section-map-size"));
    draw_sizes(ui, &layout, state);
    section_label(ui, r.min + vec2(0., 263.), t!("classic.ui.match-setup.section-difficulty"));
    draw_difficulty(ui, layout.difficulty, state);
    section_label(ui, r.min + vec2(0., 309.), t!("classic.ui.match-setup.section-speed"));
    draw_speeds(ui, &layout, state);
    section_label(ui, r.min + vec2(0., 387.), t!("classic.ui.match-setup.section-rival-nations"));
    section_label(ui, r.min + vec2(0., 429.), t!("classic.ui.match-setup.section-city-states"));
    draw_rivals(ui, &layout, state);
    section_label(ui, r.min + vec2(0., 465.), t!("classic.ui.match-setup.section-sea-level"));
    let enabled = !state.loading;
    for (i, value) in SEA_LEVELS.into_iter().enumerate() {
        if game_control(ui, layout.water[i], ("water", value), sea_label(value), None, state.water == value, enabled, true) {
            state.water = value;
        }
    }
    section_label(ui, r.min + vec2(0., 529.), t!("classic.ui.match-setup.section-victories"));
    draw_victories(ui, &layout.victories, state);
    section_label(ui, r.min + vec2(0., 624.), t!("classic.ui.match-setup.section-seed"));
    draw_seed(ui, layout.seed, state);
    if game_control(ui, layout.random, "random_seed", t!("classic.ui.match-setup.randomize"), None, false, enabled, false) {
        state.randomize_seed();
    }
}

/// Two short pages keep important settings visible without a long hidden form.
fn draw_setup_page(ui: &mut egui::Ui, r: Rect, state: &mut MatchSetup) {
    let mut layout = FormLayout::new(r);
    if !state.rules_tab {
        section_label(ui, r.min + vec2(0., 4.), t!("classic.ui.match-setup.section-map-type"));
        for cell in &mut layout.map { *cell = cell.translate(vec2(0., -22.)); }
        draw_map_types(ui, &layout, state);
        section_label(ui, r.min + vec2(0., 104.), t!("classic.ui.match-setup.section-map-size"));
        for cell in &mut layout.size { *cell = cell.translate(vec2(0., -22.)); }
        draw_sizes(ui, &layout, state);
        section_label(ui, r.min + vec2(0., 250.), t!("classic.ui.match-setup.section-difficulty"));
        layout.difficulty = layout.difficulty.translate(vec2(0., -13.));
        draw_difficulty(ui, layout.difficulty, state);
        section_label(ui, r.min + vec2(0., 296.), t!("classic.ui.match-setup.section-rivals"));
        section_label(ui, r.min + vec2(0., 338.), t!("classic.ui.match-setup.section-city-states"));
        layout.ai = layout.ai.translate(vec2(0., -89.));
        layout.city_states = layout.city_states.translate(vec2(0., -89.));
        draw_rivals(ui, &layout, state);
        paragraph(ui.painter(), r.min + vec2(0., 380.), t!("classic.ui.match-setup.world-hint"), 11., MUTED, r.width());
    } else {
        section_label(ui, r.min + vec2(0., 4.), t!("classic.ui.match-setup.section-speed"));
        for cell in &mut layout.speed { *cell = cell.translate(vec2(0., -300.)); }
        draw_speeds(ui, &layout, state);
        section_label(ui, r.min + vec2(0., 76.), t!("classic.ui.match-setup.section-sea-level"));
        for (i, value) in SEA_LEVELS.into_iter().enumerate() {
            let cell = layout.water[i].translate(vec2(0., -384.));
            if game_control(ui, cell, ("water", value), sea_label(value), None, state.water == value, true, true) { state.water = value; }
        }
        section_label(ui, r.min + vec2(0., 150.), t!("classic.ui.match-setup.section-victories"));
        for cell in &mut layout.victories { *cell = cell.translate(vec2(0., -374.)); }
        draw_victories(ui, &layout.victories, state);
        section_label(ui, r.min + vec2(0., 256.), t!("classic.ui.match-setup.section-seed"));
        draw_seed(ui, layout.seed.translate(vec2(0., -362.)), state);
        if game_control(ui, layout.random.translate(vec2(0., -362.)), "random_seed", t!("classic.ui.match-setup.randomize"), None, false, true, false) { state.randomize_seed(); }
        paragraph(ui.painter(), r.min + vec2(0., 332.), t!("classic.ui.match-setup.rules-hint"), 12., MUTED, r.width());
    }
}

fn draw_map_types(ui: &mut egui::Ui, layout: &FormLayout, state: &mut MatchSetup) {
    let enabled = !state.loading;
    for (i, map) in MapScript::ALL.into_iter().enumerate() {
        let clicked = game_control(ui, layout.map[i], ("map_type", i), map_label(map), None, state.map == map, enabled, true);
        hint(ui, layout.map[i], &map_hint(map));
        if clicked {
            state.map = map;
        }
    }
}

fn draw_sizes(ui: &mut egui::Ui, layout: &FormLayout, state: &mut MatchSetup) {
    let enabled = !state.loading;
    for (i, size) in WorldSize::ALL.into_iter().enumerate() {
        let (width, height) = size.dimensions();
        let detail = format!("{width} × {height}");
        if game_control(ui, layout.size[i], ("map_size", i), size.label(), Some(&detail), state.size == size, enabled, true) {
            state.set_size(size);
        }
    }
}

fn draw_speeds(ui: &mut egui::Ui, layout: &FormLayout, state: &mut MatchSetup) {
    let enabled = !state.loading;
    for (i, speed) in GameSpeed::ALL.into_iter().enumerate() {
        let available = enabled && speed.available();
        if game_control(ui, layout.speed[i], ("speed", i), speed.label(), None, state.speed == speed, available, true) {
            state.speed = speed;
        }
        if !speed.available() {
            hint(ui, layout.speed[i], t!("classic.ui.match-setup.speed-unavailable"));
        }
    }
}

/// AI civilizations and city-states.
fn draw_rivals(ui: &mut egui::Ui, layout: &FormLayout, state: &mut MatchSetup) {
    let enabled = !state.loading;
    let max_ai = MAX_CIVILIZATIONS - 1;
    let ai = state.ai_civilizations;
    if let Some(value) = stepper(ui, layout.ai, "ai_civilizations", &ai.to_string(), enabled && ai > 1, enabled && ai < max_ai) {
        state.ai_civilizations = (ai as i16 + value).clamp(1, max_ai as i16) as u8;
    }
    let cs = state.city_states;
    if let Some(value) = stepper(ui, layout.city_states, "city_states", &cs.to_string(), enabled && cs > 0, enabled && cs < MAX_CITY_STATES) {
        state.city_states = (cs as i16 + value).clamp(0, MAX_CITY_STATES as i16) as u8;
    }
    hint(ui, layout.city_states, t!("classic.ui.match-setup.city-states-hint"));
}

fn hint(ui: &egui::Ui, r: Rect, text: &str) {
    ui.interact(r, ui.id().with(("hint", r.min.x as i32, r.min.y as i32)), Sense::hover())
        .on_hover_text(text);
}

fn draw_difficulty(ui: &mut egui::Ui, r: Rect, state: &mut MatchSetup) {
    let rules = rules();
    let levels = difficulties(&rules);
    let at = levels.iter().position(|(k, _)| *k == state.difficulty).unwrap_or(0);
    let fallback = names::named("handicap", "", "core:handicap/prince");
    let name = levels.get(at).map(|(_, n)| n.as_str()).unwrap_or(fallback.as_str());
    let enabled = !state.loading;
    if let Some(step) = stepper(ui, r, "difficulty", name, enabled && at > 0, enabled && at + 1 < levels.len()) {
        let next = (at as i16 + step).clamp(0, levels.len() as i16 - 1) as usize;
        if let Some((key, _)) = levels.get(next) {
            state.difficulty = key.clone();
        }
    }
    if let Some(page) = pedia::game_pedia().by_key(&state.difficulty) {
        crate::ui::pedia_link::mark(ui, r, page);
        let entry = pedia::game_pedia().entry(page).cloned();
        if let Some(entry) = entry {
            let lines: Vec<String> = entry.stats.iter().map(|s| format!("{}: {}", s.label, s.value)).collect();
            hint(ui, r, &lines.join("\n"));
        }
    }
}

fn draw_victories(ui: &mut egui::Ui, cells: &[Rect], state: &mut MatchSetup) {
    let enabled = !state.loading;
    let v = &mut state.victories;
    let toggles: [(&str, &mut bool, String); 5] = [
        (t!("classic.ui.match-setup.victory-domination"), &mut v.domination, t!("classic.ui.match-setup.victory-domination-tip").into()),
        (
            t!("classic.ui.match-setup.victory-science"),
            &mut v.science,
            tf!("classic.ui.match-setup.victory-science-tip", project = names::named("project", "", "core:project/apollo_program")),
        ),
        (t!("classic.ui.match-setup.victory-culture"), &mut v.culture, t!("classic.ui.match-setup.victory-culture-tip").into()),
        (t!("classic.ui.match-setup.victory-diplomacy"), &mut v.diplomatic, t!("classic.ui.match-setup.victory-diplomacy-tip").into()),
        (t!("classic.ui.match-setup.victory-time"), &mut v.time, t!("classic.ui.match-setup.victory-time-tip").into()),
    ];
    for (i, (label, value, tip)) in toggles.into_iter().enumerate() {
        let locked = i == 0;
        if game_control(ui, cells[i], ("victory", i), label, None, *value, enabled && !locked, true) {
            *value = !*value;
        }
        hint(ui, cells[i], &tip);
    }
}

fn section_label(ui: &egui::Ui, at: egui::Pos2, label: &str) {
    text(ui.painter(), at, label, 10., GOLD);
}

/// A − value + stepper; returns -1 or +1 when a side was pressed.
fn stepper(ui: &mut egui::Ui, r: Rect, id: &'static str, value: &str, less: bool, more: bool) -> Option<i16> {
    let minus = Rect::from_min_size(r.min, vec2(40., r.height()));
    let plus = Rect::from_min_size(r.right_top() - vec2(40., 0.), vec2(40., r.height()));
    let center = Rect::from_min_max(minus.right_top() + vec2(6., 0.), plus.left_bottom() - vec2(6., 0.));
    ui.painter().rect_filled(center, 4., BG);
    ui.painter().rect_stroke(center, 4., Stroke::new(1., Color32::from_rgb(52, 72, 73)), egui::StrokeKind::Inside);
    ui.painter().text(center.center(), Align2::CENTER_CENTER, value, FontId::proportional(15.), CREAM);
    let mut step = None;
    if game_control(ui, minus, (id, "less"), "−", None, false, less, false) {
        step = Some(-1);
    }
    if game_control(ui, plus, (id, "more"), "+", None, false, more, false) {
        step = Some(1);
    }
    step
}
fn draw_seed(ui: &mut egui::Ui, r: Rect, state: &mut MatchSetup) {
    ui.painter().rect_filled(r, 4., RAISED);
    let response = ui.put(
        r.shrink2(vec2(9., 9.)),
        egui::TextEdit::singleline(&mut state.seed)
            .id(egui::Id::new("world_seed"))
            .font(FontId::monospace(13.))
            .text_color(CREAM)
            .frame(egui::Frame::NONE)
            .char_limit(20)
            .clip_text(true)
            .interactive(!state.loading),
    );
    if response.changed() {
        state.seed.retain(|c| c.is_ascii_digit());
    }
    let accent = response.has_focus() || response.hovered() || state.request().is_err();
    ui.painter().rect_stroke(
        r,
        4.,
        Stroke::new(
            if response.has_focus() { 2. } else { 1. },
            if accent {
                GOLD
            } else {
                Color32::from_rgb(62, 88, 89)
            },
        ),
        egui::StrokeKind::Inside,
    );
    response
        .on_hover_text(t!("classic.ui.match-setup.seed-tip"));
}

/// The loading screen: the leader, the ability and the world being made.
fn draw_loading(ui: &mut egui::Ui, r: Rect, state: &mut MatchSetup, rules: &CompiledRuleset, brightness: f32) {
    let Some(creating) = state.creating.clone() else {
        return;
    };
    let Some(civ) = rules.civs.iter().find(|c| c.key == creating.request.civilization) else {
        return;
    };
    let slug = civilizations::art_key(civ.key.rsplit('/').next().unwrap_or(""));
    let slug = slug.as_str();
    panel(ui.painter(), r);
    state.artwork.paint(ui, r.shrink(1.), slug, civilizations::Asset::Background, brightness);
    darken(ui.painter(), r, 40, 200);
    let side = (r.height() * 0.62).min(r.width() * 0.4);
    let portrait = Rect::from_min_size(r.min + vec2(48., 48.), vec2(side, side));
    ui.painter().rect_stroke(portrait.expand(4.), 3., Stroke::new(1.2, GOLD), egui::StrokeKind::Inside);
    state.artwork.paint(ui, portrait, slug, civilizations::Asset::Portrait, brightness);
    let card = Rect::from_min_max(pos2(portrait.right() + 28., portrait.top()), pos2(r.right() - 48., portrait.bottom()));
    ui.painter().rect_filled(card, 6., Color32::from_black_alpha(228));
    let x = card.left() + 24.;
    text(ui.painter(), pos2(x, card.top() + 18.), names::named("nation", &civ.name, &civ.key), 34., CREAM);
    text(ui.painter(), pos2(x, card.top() + 62.), &civ.leader, 18., GOLD);
    let body = Rect::from_min_max(pos2(x, card.top() + 100.), card.max - vec2(24., 16.));
    if let Some(entry) = pedia::game_pedia().by_key(&civ.key).and_then(|p| pedia::game_pedia().entry(p).cloned()) {
        ui.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| plaque_text(ui, &entry));
    }
    let (w, h) = creating.request.size.dimensions();
    let summary = tf!(
        "classic.ui.match-setup.summary",
        map = map_label(creating.options.map),
        size = match creating.request.size {
            MapSize::World(size) => size.label(),
            _ => t!("classic.ui.match-setup.size-custom"),
        },
        width = w,
        height = h,
        players = creating.request.players as u32,
        difficulty = difficulties(rules)
            .into_iter()
            .find(|(k, _)| *k == creating.options.difficulty)
            .map(|(_, n)| n)
            .unwrap_or_default(),
    );
    let bottom = r.bottom() - 60.;
    let strip = Rect::from_min_max(pos2(r.left() + 30., bottom - 44.), pos2(r.right() - 30., bottom + 52.));
    ui.painter().rect_filled(strip, 6., Color32::from_black_alpha(170));
    text(ui.painter(), pos2(r.left() + 48., bottom - 30.), summary, 14., MUTED);
    let time = ui.input(|i| i.time) as f32;
    let dots = ".".repeat(1 + (time * 2.) as usize % 3);
    text(ui.painter(), pos2(r.left() + 48., bottom), tf!("classic.ui.match-setup.creating", dots = dots), 20., CREAM);
    let bar = Rect::from_min_size(pos2(r.left() + 48., bottom + 34.), vec2(r.width() - 96., 4.));
    ui.painter().rect_filled(bar, 2., Color32::from_rgb(48, 67, 77));
    let sweep = (time * 0.6).fract();
    let lit = Rect::from_min_size(pos2(bar.left() + bar.width() * sweep * 0.8, bar.top()), vec2(bar.width() * 0.2, 4.));
    ui.painter().rect_filled(lit, 2., GOLD);
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(30));
}
/// Painted game styling with native egui focus, accessibility and keyboard activation.
#[allow(clippy::too_many_arguments)]
fn game_control(
    ui: &mut egui::Ui,
    r: Rect,
    id: impl std::hash::Hash + std::fmt::Debug,
    label: &str,
    detail: Option<&str>,
    selected: bool,
    enabled: bool,
    choice: bool,
) -> bool {
    let response = ui.interact(
        r,
        ui.id().with(id),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    response.widget_info(|| {
        if choice {
            egui::WidgetInfo::selected(egui::WidgetType::RadioButton, enabled, selected, label)
        } else {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                enabled,
                match label {
                    "+" => t!("classic.ui.match-setup.more"),
                    "−" => t!("classic.ui.match-setup.fewer"),
                    other => other,
                },
            )
        }
    });
    let focused = enabled && response.has_focus();
    let pressed = enabled && response.is_pointer_button_down_on();
    let hovered = enabled && response.hovered();
    let fill = if !enabled {
        PANEL
    } else if pressed {
        Color32::from_rgb(30, 79, 79)
    } else if selected {
        TEAL
    } else if hovered {
        Color32::from_rgb(39, 67, 70)
    } else {
        RAISED
    };
    ui.painter().rect_filled(r, 4., fill);
    ui.painter().rect_stroke(
        r,
        4.,
        Stroke::new(
            if focused || selected { 1.5 } else { 1. },
            if focused || selected || hovered {
                GOLD
            } else {
                Color32::from_rgb(57, 84, 86)
            },
        ),
        egui::StrokeKind::Inside,
    );
    let ink = if enabled || selected {
        CREAM
    } else {
        MUTED.gamma_multiply(0.65)
    };
    let y = if detail.is_some() {
        r.top() + 16.
    } else {
        r.center().y
    };
    ui.painter().text(
        pos2(r.center().x, y),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(if label == "−" || label == "+" {
            22.
        } else {
            13.
        }),
        ink,
    );
    if let Some(detail) = detail {
        ui.painter().text(
            pos2(r.center().x, r.bottom() - 12.),
            Align2::CENTER_CENTER,
            detail,
            FontId::proportional(10.5),
            if selected { CREAM } else { MUTED },
        );
    }
    if choice && selected {
        let at = r.right_top() + vec2(-9., 8.);
        ui.painter().add(egui::Shape::line(
            vec![at + vec2(-3., 0.), at + vec2(-1., 2.5), at + vec2(3.5, -3.)],
            Stroke::new(1.5, GOLD),
        ));
    }
    let keyboard =
        focused && ui.input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Space));
    enabled && (response.clicked() || keyboard)
}
impl MatchSetup {
    fn randomize_seed(&mut self) {
        use std::hash::{BuildHasher, Hasher};
        let mut source = std::collections::hash_map::RandomState::new().build_hasher();
        source.write(self.seed.as_bytes());
        let old = self.seed.parse::<u64>().ok();
        let mut seed = source.finish();
        if old == Some(seed) {
            seed = seed.wrapping_add(1);
        }
        self.seed = seed.to_string();
    }
}

#[cfg(test)]
#[path = "match_setup_tests.rs"]
mod tests;
