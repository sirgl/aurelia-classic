//! The classic screens: their state between frames, the ids the shell knows them by, the
//! Empire menu, and the dispatch from a shown screen to its drawing. With a
//! `ScreenModel` (the game) city and research show the real match and emit typed
//! `GameRequest`s; without one (the showcase) they show local preview data.
//!
//! The shell owns which screen is shown (`client::shell::screens`); `ScreenState::screen`
//! is its value for this frame, and `navigate` asks the shell to switch.
use crate::ui::model::{GameRequest, ScreenModel};
use client::shell::art::{ArtKinds, ScreenArt};
use client::shell::widgets::dag;
pub(crate) use client::shell::kit::{
    button, darken, heading, panel, paragraph, ready_button, text, world_banner, BG, CREAM, GOLD,
    MUTED, PANEL, RAISED, TEAL,
};
use client::shell::module::{self as shell, MenuGroup, ScreenId, ScreenMusic, ScreenSpec, ShellAction, ShellFlags};
pub(crate) use client::shell::scroll;
use egui::{pos2, vec2, Align2, Color32, FontId, Rect, Sense, Stroke};
use i18n::t;
use render::world::state::RenderWorld;
use std::collections::BTreeSet;

#[path = "match_setup.rs"]
mod match_setup;
pub use match_setup::{
    prepare as prepare_match, prepare_game, rules as game_rules, GameOptions, GameSpeed, MapSize, MatchRequest,
    MatchOrder, NewGame, PreparedMatch, Seat, Victories, WorldSize,
};
#[path = "screen_city.rs"]
mod city;
#[path = "screen_civilizations.rs"]
mod civilizations;
#[path = "policy_art.rs"]
mod policy_art;
#[path = "screen_progress.rs"]
mod progress;
#[path = "screen_game.rs"]
mod game_screens;
#[path = "screen_civics.rs"]
mod civics_screen;
#[path = "world_symbols.rs"]
pub(crate) mod world_symbols;
#[path = "screen_religion.rs"]
mod religion_screen;
#[path = "screen_culture.rs"]
mod culture_screen;
#[path = "screen_diplomacy.rs"]
mod diplomacy_screen;
#[path = "screen_city_states.rs"]
mod city_states_screen;
#[path = "screen_congress.rs"]
mod congress_screen;
#[path = "screen_espionage.rs"]
mod espionage_screen;
#[path = "screen_victory.rs"]
mod victory_screen;
#[path = "screen_pedia.rs"]
mod pedia_screen;
#[path = "screen_overview.rs"]
mod overview_screen;
pub use overview_screen::{sort_cities, CitySort, Focus, OverviewTab};

/// A screen as the classic screens name it: the shell's own (map, menu, settings) and the
/// module's, numbered for the shell by `ScreenId`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum Screen {
    #[default]
    Map,
    Menu,
    City,
    Research,
    Civics,
    Religion,
    Culture,
    Diplomacy,
    CityStates,
    Congress,
    Espionage,
    Victory,
    /// The results when the game ends (victory or defeat, Hall of Fame).
    GameEnd,
    Civilizations,
    MatchSetup,
    Settings,
    Encyclopedia,
    Overview,
}

/// The module's screens, in `ScreenId` order.
const MODULE_SCREENS: [Screen; 15] = [
    Screen::City,
    Screen::Research,
    Screen::Civics,
    Screen::Religion,
    Screen::Culture,
    Screen::Diplomacy,
    Screen::CityStates,
    Screen::Congress,
    Screen::Espionage,
    Screen::Victory,
    Screen::GameEnd,
    Screen::Civilizations,
    Screen::MatchSetup,
    Screen::Encyclopedia,
    Screen::Overview,
];

/// The screens about the other players and the end of the game (the game binary only).
const WORLD_SCREENS: [Screen; 5] = [Screen::Diplomacy, Screen::CityStates, Screen::Congress, Screen::Espionage, Screen::Victory];

impl From<Screen> for shell::Screen {
    fn from(screen: Screen) -> Self {
        match screen {
            Screen::Map => shell::Screen::Map,
            Screen::Menu => shell::Screen::Menu,
            Screen::Settings => shell::Screen::Settings,
            module => shell::Screen::Module(module.id()),
        }
    }
}

impl From<shell::Screen> for Screen {
    fn from(screen: shell::Screen) -> Self {
        match screen {
            shell::Screen::Map => Screen::Map,
            shell::Screen::Menu => Screen::Menu,
            shell::Screen::Settings => Screen::Settings,
            shell::Screen::Module(id) => Screen::of(id),
        }
    }
}

impl Screen {
    /// The shell's id of a module screen.
    pub fn id(self) -> ScreenId {
        let index = MODULE_SCREENS.iter().position(|s| *s == self).expect("a module screen");
        ScreenId(index as u8)
    }
    pub fn of(id: ScreenId) -> Self {
        MODULE_SCREENS[id.0 as usize]
    }
    /// Screens that exist only with a running game.
    fn needs_game(self) -> bool {
        matches!(self, Self::Religion | Self::Culture | Self::Overview | Self::GameEnd) || WORLD_SCREENS.contains(&self)
    }
    pub fn wants_world(self) -> bool {
        matches!(self, Self::Map | Self::City)
    }
    fn music(self) -> ScreenMusic {
        match self {
            Self::Menu | Self::MatchSetup => ScreenMusic::Menu,
            Self::Settings | Self::Civilizations | Self::Encyclopedia => ScreenMusic::Keep,
            _ => ScreenMusic::Game,
        }
    }
    /// The shortcut that opens the screen (an action id of `ui::shortcuts`), for button
    /// tooltips; the keymap says the key.
    fn hotkey(self) -> Option<&'static str> {
        match self {
            Self::Encyclopedia => Some("encyclopedia"),
            Self::Overview => Some("overview-economy"),
            Self::Civics => Some("policies"),
            Self::Research => Some("technologies"),
            Self::Culture => Some("culture"),
            Self::Menu => Some("Esc"), // i18n-ok: keycap
            _ => None,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Map => t!("classic.ui.screens.world-map"),
            Self::Menu => t!("classic.ui.screens.menu"),
            Self::City => t!("classic.ui.screens.city"),
            Self::Research => t!("classic.ui.screens.research"),
            Self::Civics => t!("classic.ui.screens.policies"),
            Self::Religion => t!("classic.ui.screens.religion"),
            Self::Culture => t!("classic.ui.screens.culture"),
            Self::Diplomacy => t!("classic.ui.screens.diplomacy"),
            Self::CityStates => t!("classic.ui.screens.city-states"),
            Self::Congress => t!("classic.ui.screens.congress"),
            Self::Espionage => t!("classic.ui.screens.espionage"),
            Self::Victory => t!("classic.ui.screens.victory"),
            Self::GameEnd => t!("classic.ui.screens.results"),
            Self::Civilizations => t!("classic.ui.screens.nations"),
            Self::MatchSetup => t!("classic.ui.screens.new-game"),
            Self::Settings => t!("classic.ui.screens.settings"),
            Self::Encyclopedia => t!("classic.ui.screens.encyclopedia"),
            Self::Overview => t!("classic.ui.screens.overview"),
        }
    }
    /// The showcase's footer under the screen.
    fn footer(self, dev_mode: bool) -> &'static str {
        match self {
            Self::City | Self::Research | Self::Civics => t!("classic.ui.screens.preview-footer"),
            Self::Civilizations if dev_mode => t!("classic.ui.screens.nations-footer"),
            _ => "",
        }
    }
}

/// How the shell frames a classic screen.
pub(crate) fn spec(screen: Screen, dev_mode: bool) -> ScreenSpec {
    ScreenSpec {
        title: screen.label(),
        hotkey: screen.hotkey(),
        needs_game: screen.needs_game(),
        shows_world: screen == Screen::City,
        music: screen.music(),
        footer: screen.footer(dev_mode),
    }
}

/// The Empire menu: the empire's screens, the world's (in the game) and the game's.
pub(crate) fn menu(flags: ShellFlags) -> Vec<MenuGroup> {
    let mut groups = vec![(t!("classic.ui.screens.group-empire"), vec![Screen::City, Screen::Research, Screen::Civics])];
    if flags.game_mode {
        groups[0].1.extend([Screen::Religion, Screen::Culture, Screen::Overview]);
        groups.push((t!("classic.ui.screens.group-world"), WORLD_SCREENS.to_vec()));
    }
    if flags.game_mode && !flags.has_session {
        groups.clear();
    }
    let mut game = vec![Screen::Menu, Screen::Encyclopedia, Screen::Settings];
    if flags.dev_mode {
        game.push(Screen::Civilizations);
    }
    groups.push((t!("classic.ui.screens.group-game"), game));
    groups.into_iter().map(|(title, screens)| MenuGroup { title, screens: screens.into_iter().map(Into::into).collect() }).collect()
}

pub struct ScreenState {
    /// The screen the shell shows this frame.
    pub screen: Screen,
    pub dev_mode: bool,
    /// The game binary: screens need a `ScreenModel`.
    pub game_mode: bool,
    /// Art brightness of the player's settings.
    pub brightness: f32,
    /// Requests from this frame's clicks, applied by the module to its match.
    pub requests: Vec<GameRequest>,
    /// Screen switches asked this frame, for the shell.
    pub actions: Vec<ShellAction>,
    /// The technology tree's selection and scroll.
    research_tree: dag::GraphState,
    match_setup: match_setup::MatchSetup,
    art: ScreenArt,
    civilizations: civilizations::CivilizationsState,
    policy_art: policy_art::PolicyArt,
    city_ui: game_screens::city_screen::CityUiState,
    city_tab: usize,
    production: usize,
    queue: Vec<usize>,
    focus: usize,
    research: usize,
    current_research: Option<usize>,
    research_progress: u32,
    known: BTreeSet<usize>,
    branch: usize,
    policy: usize,
    adopted: BTreeSet<usize>,
    policy_points: u32,
    /// Civics screen of the game: the branch and the policy looked at.
    civics_branch: Option<classic_rules::rules::content::PolicyBranchId>,
    civics_policy: Option<classic_rules::ids::PolicyId>,
    /// Religion screen of the game: the beliefs and religion picked.
    faith_picks: religion_screen::FaithPicks,
    /// Diplomacy screen: the civilization looked at, the deal drafted, leader art.
    diplomacy: diplomacy_screen::DiplomacyState,
    /// Espionage screen: the spy whose destinations are listed.
    spy: u8,
    pedia: pedia_screen::PediaState,
    overview: overview_screen::OverviewState,
    /// Map position an overview row asked for (drained by the module).
    map_focus: Option<Focus>,
}

impl Default for ScreenState {
    fn default() -> Self {
        Self {
            screen: Screen::Map,
            dev_mode: std::env::var("AURELIA_VISUAL_DEV").as_deref() == Ok("1"),
            game_mode: false,
            brightness: 1.0,
            requests: Vec::new(),
            actions: Vec::new(),
            research_tree: Default::default(),
            match_setup: Default::default(),
            art: ScreenArt::new(CLASSIC_ART),
            civilizations: Default::default(),
            policy_art: Default::default(),
            city_ui: Default::default(),
            city_tab: 0,
            production: 0,
            queue: vec![0],
            focus: 0,
            research: 2,
            current_research: Some(2),
            research_progress: 26,
            known: [0, 1].into_iter().collect(),
            branch: 0,
            policy: 0,
            adopted: BTreeSet::new(),
            policy_points: 2,
            civics_branch: None,
            civics_policy: None,
            faith_picks: Default::default(),
            diplomacy: Default::default(),
            spy: 0,
            pedia: Default::default(),
            overview: Default::default(),
            map_focus: None,
        }
    }
}

impl ScreenState {
    /// The shell's state for this frame.
    pub fn sync(&mut self, flags: ShellFlags) {
        self.screen = flags.screen.into();
        self.dev_mode = flags.dev_mode;
        self.game_mode = flags.game_mode;
        self.brightness = flags.brightness;
    }
    /// Offscreen capture setup; the same state as pressing Buy a tile in the city.
    pub fn capture_city_buy(&mut self, city: &crate::ui::model::CityView) {
        self.city_ui.begin_buy(city);
    }
    /// Offscreen capture setup: the same state as pressing Rename in the city.
    pub fn capture_city_rename(&mut self, city: &crate::ui::model::CityView) {
        self.city_ui.begin_rename(city);
    }
    pub fn city_buy_mode(&self) -> bool {
        self.screen == Screen::City && self.city_ui.buy_mode
    }
    pub fn cancel_city_buy(&mut self) -> bool {
        self.screen == Screen::City && self.city_ui.cancel_buy()
    }
    pub fn fit_city_buy_camera(
        &self,
        world: &RenderWorld,
        camera: &mut render::camera::CameraController,
        surface: Option<&render::scene::terrain::surface::SurfaceMesh>,
        screen: Rect,
        city: &crate::ui::model::CityView,
    ) {
        if self.city_buy_mode() {
            game_screens::city_screen::fit_camera(world, camera, surface, screen, city, true);
        }
    }
    /// Fit the selected city's working land (or purchase offers) beside its panels.
    pub fn fit_city_camera(
        &self,
        world: &RenderWorld,
        camera: &mut render::camera::CameraController,
        surface: Option<&render::scene::terrain::surface::SurfaceMesh>,
        screen: Rect,
        city: &crate::ui::model::CityView,
    ) {
        if self.screen == Screen::City {
            game_screens::city_screen::fit_camera(world, camera, surface, screen, city, self.city_buy_mode());
        }
    }
    pub fn prepare_city_map(
        &mut self,
        world: &RenderWorld,
        camera: &render::camera::CameraController,
        surface: Option<&render::scene::terrain::surface::SurfaceMesh>,
        screen: Rect,
        city: Option<&crate::ui::model::CityView>,
    ) {
        if self.screen != Screen::City {
            self.city_ui.cancel_buy();
            return;
        }
        game_screens::city_screen::prepare(&mut self.city_ui, world, camera, surface, screen, city);
    }

    /// Offscreen captures: show the rules tab of the match setup.
    pub fn capture_setup_rules(&mut self) {
        self.match_setup.rules_tab = true;
    }

    /// Screenshot harness only: warm the visible working set and optionally focus a node.
    /// Live screens keep using asynchronous artwork loading.
    pub fn capture_research(&mut self, ctx: &egui::Context, view: &crate::ui::model::ResearchView, focus: Option<usize>, viewport: egui::Vec2) {
        // Mirror the management canvas and clamp to the actual scrollable extent.
        // A far-end focus shows preceding columns too, not just the final node.
        let body = viewport - vec2(48., 117.);
        let scale = (body.x / 1150.).min(body.y / 640.).min(1.);
        let visible_width = body.x / scale - 16.;
        let content_width = dag::canvas_width(view.techs.iter().map(|t| t.column).max().unwrap_or(0));
        let requested = focus.and_then(|i| view.techs.get(i)).map(|t| t.column as f32 * dag::COLUMN).unwrap_or(0.);
        let start = requested.min((content_width - visible_width).max(0.));
        if let Some(index) = focus {
            self.research_tree.focus(index);
        }
        let mut names = Vec::new();
        for tech in view.techs.iter().filter(|t| {
            let card = dag::Cell { column: t.column, row: 0 }.rect();
            card.right() >= start && card.left() <= start + visible_width
        }) {
            names.push(tech.art.as_str());
            if let Some(art) = tech.unlocks.first().and_then(|u| u.art.as_deref()) {
                names.push(art);
            }
        }
        names.sort_unstable();
        names.dedup();
        self.art.preload(ctx, &names);
    }

    /// Decode the art of the shown screen now (offscreen captures).
    pub fn preload_screen_art(&mut self, ctx: &egui::Context) {
        let names: Vec<&str> = match self.screen {
            Screen::City => {
                vec!["city", "granary", "warrior", "core:building/monument", "watermill", "agriculture", "mining", "writing"]
            }
            Screen::Research => progress::research_art_names(),
            Screen::Civics => vec!["civics", "tradition", "liberty", "honour"],
            _ => vec![],
        };
        self.art.preload(ctx, &names);
        if self.screen == Screen::MatchSetup {
            self.match_setup.preload(ctx);
        }
        if self.screen == Screen::Civilizations {
            self.civilizations.preload(ctx);
        }
    }

    /// What the screen art painted last (`--bench-art`).
    pub(crate) fn art_frame(&self) -> client::shell::art::ArtFrame {
        self.art.frame_stats()
    }
    /// The city screen lists every production option (`--bench-art`).
    pub(crate) fn list_all_production(&mut self, city: &crate::ui::model::CityView) {
        self.city_ui.list_all_production(city);
    }
    /// Forget every decoded picture, as a fresh start would (`--bench-art`).
    pub(crate) fn forget_art(&mut self) {
        self.art = ScreenArt::new(CLASSIC_ART);
    }

    /// Synchronous art warmup for headless captures of game screens.
    pub fn preload_art(&mut self, ctx: &egui::Context, names: &[&str]) {
        self.art.preload(ctx, names);
    }

    pub fn request_new_match(&mut self) -> bool {
        self.match_setup.begin(Seat::Play)
    }

    pub fn take_match_request(&mut self) -> Option<NewGame> {
        self.match_setup.take_request()
    }
    /// The same request, with who plays the first civilization.
    pub fn take_match_order(&mut self) -> Option<MatchOrder> {
        self.match_setup.take_order()
    }
    /// The match the setup asked for runs.
    pub fn match_ready(&mut self) {
        self.match_setup.loading = false;
    }
    pub fn match_failed(&mut self, error: String) {
        self.match_setup.loading = false;
        self.match_setup.error = Some(error);
    }
    pub fn creating_match(&self) -> bool {
        self.match_setup.loading
    }

    pub fn select_policy_art(&mut self, group: &str) -> bool {
        if !self.policy_art.select_group(group) {
            return false;
        }
        self.navigate(Screen::Civics);
        true
    }
    /// Offscreen captures: decode the building art the game's Culture screen shows now.
    pub fn preload_culture_art(&mut self, ctx: &egui::Context, model: &ScreenModel) {
        if let Some(view) = &model.culture {
            let names = culture_screen::visible_art(view);
            let names: Vec<&str> = names.iter().map(String::as_str).collect();
            self.preload_art(ctx, &names);
        }
    }
    /// Offscreen captures: decode the leader art the game's Diplomacy screen shows now.
    pub fn preload_leader_art(&mut self, ctx: &egui::Context, model: &ScreenModel) {
        if let Some(view) = &model.diplomacy {
            self.diplomacy.preload(ctx, view);
        }
    }
    /// Offscreen captures: decode the belief art the game's Religion screen shows now.
    pub fn preload_religion_art(&mut self, ctx: &egui::Context, model: &ScreenModel) {
        if let Some(view) = &model.religion {
            let names = religion_screen::visible_art(view);
            let names: Vec<&str> = names.iter().map(String::as_str).collect();
            self.preload_art(ctx, &names);
        }
    }
    /// Offscreen captures: decode the art the game's Civics screen shows now.
    pub fn preload_civics_art(&mut self, ctx: &egui::Context, model: &ScreenModel) {
        if let Some(view) = &model.civics {
            let ids = civics_screen::visible_art(view, self);
            self.policy_art.preload_ids(ctx, &ids);
        }
    }
    pub fn preload_policy_art(&mut self, ctx: &egui::Context) {
        if self.policy_art.collection {
            self.policy_art.preload(ctx);
        } else if self.screen == Screen::Civics {
            for group in ["honor", "liberty", "tradition"] {
                self.policy_art.select_group(group);
                self.policy_art.preload(ctx);
            }
            self.policy_art.collection = false;
        }
    }

    #[cfg(test)]
    pub fn selected_civilization(&self) -> &str {
        self.civilizations.selected_slug()
    }
    pub fn select_civilization(&mut self, slug: &str) -> bool {
        self.civilizations.select(slug)
    }
    /// Show another screen (the shell switches after this frame).
    fn navigate(&mut self, screen: Screen) {
        self.screen = screen;
        self.actions.push(ShellAction::Open(screen.into()));
    }
    pub fn open(&mut self, screen: Screen) {
        self.navigate(screen);
    }
    /// The shell switched to `screen` from `from`.
    pub fn opened(&mut self, screen: Screen, from: Screen) {
        if screen == Screen::Encyclopedia {
            self.pedia.return_to = from;
        }
    }
    /// Draw the shown screen (`screen` was synced this frame).
    pub fn draw(&mut self, ui: &mut egui::Ui, r: Rect, world: &RenderWorld, game: Option<&ScreenModel>) {
        draw_body(ui, r, self.screen, self, world, game);
    }
    fn start_research(&mut self) -> bool {
        let technology = &progress::TECHS[self.research];
        if self.known.contains(&self.research) || !technology.prerequisites.iter().all(|p| self.known.contains(p)) {
            return false;
        }
        self.current_research = Some(self.research);
        self.research_progress = 0;
        true
    }
    fn adopt_policy(&mut self) -> bool {
        if self.policy_points == 0
            || self.adopted.contains(&self.policy)
            || progress::POLICIES[self.policy].prerequisite.is_some_and(|p| !self.adopted.contains(&p))
        {
            return false;
        }
        self.adopted.insert(self.policy);
        self.policy_points -= 1;
        true
    }
}

/// The classic module's pictures that are not ordinary cards.
const CLASSIC_ART: ArtKinds = ArtKinds {
    heroes: &["city", "research", "civics"],
    items: &["warrior", "granary", "watermill", "agriculture", "pottery", "mining", "husbandry", "writing"],
    // The civic figure group stays whole in wide branch headers.
    groups: &["liberty"],
};

fn art(state: &mut ScreenState, ui: &egui::Ui, r: Rect, name: &str) {
    let tint = Color32::from_gray((state.brightness * 255.0).min(255.0) as u8);
    state.art.paint(ui.ctx(), ui.painter(), r, name, tint);
}

fn draw_body(
    ui: &mut egui::Ui,
    r: Rect,
    screen: Screen,
    state: &mut ScreenState,
    world: &RenderWorld,
    game: Option<&ScreenModel>,
) {
    match (screen, game) {
        (Screen::MatchSetup, _) => match_setup::draw(ui, r, &mut state.match_setup, state.brightness),
        (Screen::Encyclopedia, _) => pedia_screen::draw(ui, r, state),
        (Screen::Overview, Some(model @ ScreenModel { overview: Some(view), .. })) => {
            overview_screen::draw(ui, r, state, view, model.diplomacy.as_ref())
        }
        (Screen::City, Some(game)) => game_screens::city(ui, r, state, game),
        (Screen::City, None) => city::draw(ui, r, state, world),
        (Screen::Research, Some(game)) => game_screens::research(ui, r, state, &game.research),
        (Screen::Research, None) => progress::research(ui, r, state),
        (Screen::Civics, Some(ScreenModel { civics: Some(view), .. })) => civics_screen::civics(ui, r, state, view),
        (Screen::Civics, _) => progress::civics(ui, r, state),
        (Screen::Religion, Some(ScreenModel { religion: Some(view), .. })) => {
            religion_screen::religion(ui, r, state, view)
        }
        (Screen::Culture, Some(ScreenModel { culture: Some(view), .. })) => culture_screen::culture(ui, r, state, view),
        (Screen::Diplomacy, Some(model @ ScreenModel { diplomacy: Some(view), .. })) => {
            diplomacy_screen::diplomacy(ui, r, state, view, model.message.as_deref())
        }
        (Screen::CityStates, Some(model @ ScreenModel { city_states: Some(view), .. })) => {
            city_states_screen::city_states(ui, r, state, view, model.message.as_deref())
        }
        (Screen::Congress, Some(model @ ScreenModel { congress: Some(view), .. })) => {
            congress_screen::congress(ui, r, state, view, model.message.as_deref())
        }
        (Screen::Espionage, Some(model @ ScreenModel { espionage: Some(view), .. })) => {
            espionage_screen::espionage(ui, r, state, view, model.message.as_deref())
        }
        (Screen::Victory, Some(ScreenModel { victory: Some(view), .. })) => victory_screen::victory(ui, r, view),
        (Screen::GameEnd, Some(ScreenModel { game_end: Some(view), .. })) => {
            victory_screen::game_end(ui, r, state, view)
        }
        (Screen::Civilizations, _) => civilizations::draw(ui, r, &mut state.civilizations, state.brightness),
        _ => {}
    }
}

/// Tests: one frame of the shell's screen stack over these screens. `state.screen` is the
/// shown screen; a switch a screen asks applies after the frame, as in the app.
#[cfg(test)]
pub(crate) fn draw(ctx: &egui::Context, state: &mut ScreenState, world: &RenderWorld, game: Option<&ScreenModel>) -> bool {
    let mut shell = client::shell::screens::ScreenState::default();
    shell.screen = state.screen.into();
    shell.dev_mode = state.dev_mode;
    shell.game_mode = state.game_mode;
    shell.has_session = game.is_some();
    let mut module = crate::ClassicUi::with_screens(std::mem::take(state), game.cloned());
    let modal = client::shell::screens::draw(ctx, &mut shell, &mut module, Some(world));
    let actions = shell::ModuleUi::take_actions(&mut module);
    *state = std::mem::take(&mut module.screens);
    for action in actions {
        if let ShellAction::Open(screen) = action {
            state.screen = screen.into();
        }
    }
    modal
}

#[cfg(test)]
#[path = "screens_tests.rs"]
mod tests;
