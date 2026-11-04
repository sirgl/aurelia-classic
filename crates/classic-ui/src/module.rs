//! `ClassicUi`: the classic module as the app shell sees it (`ModuleUi`). It owns the match
//! (`Session`), the state of its screens and HUD panels, and the match being created;
//! everything it wants from the shell leaves as `ShellAction`s.
use std::sync::mpsc::Receiver;
use std::time::Duration;

use client::shell::module::{
    AiProgress, CameraFit, EndTurn, FrameInput, HudFeed, HudLayer, HudView, KindLabel, MapButton, MapView, MenuGroup,
    ModuleId, ModuleUi, Outcome, RouteFeed, SaveProblem, SaveSummary, Screen as ShellScreen, ScreenId, ScreenSpec, ShellAction, ShellFlags,
};
use client::shell::observer::Watch;
use client::shell::presentation::{Frame, Settings as PresentationSettings};
use egui::Rect;
use i18n::{t, tf};
use render::camera::CameraController;
use render::unit_models::UnitModelBook;
use render::world::state::RenderWorld;
use render::world_models::WorldBook;

use crate::game::{Choice, Session};
use crate::ui::shortcuts;
use crate::ui::hud::{Action, ClassicHud, PanelClicks, TopTarget};
use crate::ui::model::{GameRequest, ScreenModel};
use crate::ui::pedia::{self, PediaRef};
use crate::observer::{ClassicMatch, ClassicPowers};
use crate::ui::screens::{self, Focus, OverviewTab, PreparedMatch, Screen, ScreenState, Seat};
use crate::ui::unit_kinds::UnitKinds;

/// Camera zoom of the city screen.
const CITY_ZOOM: f32 = 3.0;

/// The classic module's user interface and its match.
pub struct ClassicUi {
    /// The running match.
    pub session: Option<Session>,
    /// The match the AI plays alone and the player watches (the observer mode).
    observer: Option<ClassicMatch>,
    /// The seed of the running match, when it was made here (a save does not keep it).
    seed: Option<u64>,
    pub screens: ScreenState,
    pub hud: ClassicHud,
    /// The shown screen's view, built once per frame (`refresh`).
    model: Option<ScreenModel>,
    hall_of_fame: Vec<String>,
    match_worker: Option<Receiver<Result<PreparedMatch, String>>>,
    /// Who plays the match being created.
    seat: Seat,
    /// Requests of the HUD panels this frame, applied after the UI.
    pending: Vec<GameRequest>,
    actions: Vec<ShellAction>,
    /// The host plays the other players' turns on a worker (`ModuleUi::set_ai_wake`).
    ai_wake: Option<client::ai_round::AiWake>,
    /// What the map shows of the air orders (the sweep range, the interceptors), per frame.
    paint: client::shell::module::MapPaint,
    /// The end of the turn waits for a research or a policy to be chosen (see `end_turn`).
    end_turn_gate: bool,
    /// The round in which the player adopted or unlocked a policy: one choice a turn is asked for.
    policy_round: Option<u32>,
}

impl ClassicUi {
    /// The game: screens show the match.
    pub fn game() -> Self {
        let mut ui = Self::showcase(UnitKinds::default());
        ui.screens.game_mode = true;
        ui
    }

    /// The showcase: screens show preview data; badges name units by `kinds`.
    pub fn showcase(kinds: UnitKinds) -> Self {
        let mut hud = ClassicHud::default();
        hud.unit_kinds = kinds;
        Self {
            session: None,
            observer: None,
            seed: None,
            screens: ScreenState::default(),
            hud,
            model: None,
            hall_of_fame: Vec::new(),
            match_worker: None,
            seat: Seat::Play,
            pending: Vec::new(),
            actions: Vec::new(),
            ai_wake: None,
            paint: Default::default(),
            end_turn_gate: true,
            policy_round: None,
        }
    }

    /// Scripts and benches that play turns without a player: the end of the turn does not wait
    /// for a research or a policy.
    pub fn without_end_turn_gate(mut self) -> Self {
        self.end_turn_gate = false;
        self
    }

    /// Tests: screens in a given state over a given view.
    #[cfg(test)]
    pub(crate) fn with_screens(screens: ScreenState, model: Option<ScreenModel>) -> Self {
        let mut ui = Self::showcase(UnitKinds::default());
        ui.screens = screens;
        ui.model = model;
        ui
    }

    /// The view of the shown screen this frame (offscreen captures preload its art).
    pub fn screen_model(&self) -> Option<&ScreenModel> {
        self.model.as_ref()
    }

    /// Start or replace the match with a prepared one; the shell shows its map.
    pub fn start(&mut self, prepared: PreparedMatch) {
        self.observer = None;
        self.policy_round = None;
        let summary = tf!("classic.module.match-summary", nation = prepared.nation.as_str(), width = prepared.world.width, height = prepared.world.height);
        match &mut self.session {
            Some(session) => session.replace_game_with(prepared.game, &prepared.options.difficulty),
            None => {
                let session = Session::with_difficulty(prepared.game, screens::game_rules(), &prepared.options.difficulty);
                self.session = Some(session);
            }
        }
        self.seed = prepared.seed;
        self.after_new_world();
        self.screens.match_ready();
        self.actions.push(ShellAction::MatchStarted { summary });
    }

    /// Start a match the AI plays alone, watched in the observer mode.
    pub fn start_observer(&mut self, prepared: PreparedMatch, watch: Watch) {
        let observed = ClassicMatch::new(prepared.game, screens::game_rules(), ClassicPowers, Some(&prepared.options.difficulty));
        let observed = match watch {
            Watch::All => observed,
            Watch::One => observed.watching(0),
        };
        let summary = client::shell::observer::Observed::summary(&observed);
        self.observer = Some(observed);
        self.session = None;
        self.screens.match_ready();
        self.actions.push(ShellAction::ObserverStarted { summary });
    }

    /// The observed match, for tests and offscreen runs.
    pub fn observer_match(&mut self) -> Option<&mut ClassicMatch> {
        self.observer.as_mut()
    }

    fn after_new_world(&mut self) {
        if let Some(session) = &self.session {
            self.hud.unit_kinds = UnitKinds::from_rules(session.rules());
        }
    }

    /// A widget's request: the shell's (saves, turns), a screen switch, or the match's.
    pub fn request(&mut self, request: GameRequest) {
        match request {
            GameRequest::Save => self.actions.push(ShellAction::Save),
            GameRequest::Load => self.actions.push(ShellAction::Load),
            GameRequest::Continue => self.actions.push(ShellAction::Continue),
            GameRequest::EndTurn => self.actions.push(ShellAction::EndTurn),
            GameRequest::SelectCity(tile) => {
                if let Some(session) = &mut self.session {
                    session.request(GameRequest::SelectCity(tile));
                    self.screens.open(Screen::City);
                }
            }
            GameRequest::ShowCityState(player) => {
                if let Some(tile) = self.session.as_mut().and_then(|session| session.show_city_state(player)) {
                    self.screens.open(Screen::Map);
                    self.actions.push(ShellAction::CenterOn(tile.0));
                }
            }
            GameRequest::Unit(Action::TradeRoute) => self.screens.open_overview(OverviewTab::Trade),
            other => {
                if let Some(session) = &mut self.session {
                    let culture = matches!(other, GameRequest::UnlockBranch(_) | GameRequest::AdoptPolicy(_) | GameRequest::ChooseIdeology(_))
                        .then(|| session.culture_choices());
                    session.request(other);
                    if culture.is_some_and(|before| session.culture_choices() > before) {
                        self.policy_round = Some(session.turn());
                    }
                }
            }
        }
    }

    /// Gives the selected unit the first of `orders` it has; false when it has none.
    fn unit_order(&mut self, orders: &[Action]) -> bool {
        let have: Vec<Action> = match self.hud.game.as_ref().and_then(|g| g.orders.as_ref()) {
            Some(orders) => orders.actions.iter().map(|a| a.action).collect(),
            None => return false,
        };
        match orders.iter().find(|a| have.contains(a)) {
            Some(&action) => {
                self.request(GameRequest::Unit(action));
                true
            }
            None => false,
        }
    }

    /// The own city after (or before) the selected one in the list of the map's cities.
    fn step_city(&mut self, forward: bool) -> bool {
        let Some(session) = &self.session else { return false };
        let world = session.view();
        let mine: Vec<u32> = world.cities.iter().filter(|c| c.owner == world.observer).map(|c| c.tile.0).collect();
        if mine.is_empty() {
            return false;
        }
        let at = session.selected_city_tile().and_then(|t| mine.iter().position(|c| *c == t));
        let next = match (at, forward) {
            (Some(i), true) => (i + 1) % mine.len(),
            (Some(i), false) => (i + mine.len() - 1) % mine.len(),
            (None, true) => 0,
            (None, false) => mine.len() - 1,
        };
        self.request(GameRequest::SelectCity(mine[next]));
        true
    }

    /// The world the module draws over: the shell's scene, else the match's view.
    fn world<'a>(&'a self, scene: Option<&'a RenderWorld>) -> &'a RenderWorld {
        world_of(scene, self.session.as_ref())
    }

    fn panel_clicks(&mut self, clicks: PanelClicks) {
        if clicks.open_research {
            self.screens.open(Screen::Research);
        }
        if clicks.open_results {
            self.screens.open(Screen::GameEnd);
        }
        if let Some(tile) = clicks.center_on {
            self.actions.push(ShellAction::CenterOn(tile));
        }
        self.pending.extend(clicks.requests);
    }

    /// "?" over a map tile: the Encyclopedia page of what is most specific there
    /// (resource, improvement, natural wonder, terrain), as the player sees it.
    fn hovered_tile_page(&self, ctx: &egui::Context, hovered: Option<u32>, shell: ShellFlags) -> Option<PediaRef> {
        let asked = ctx.input(|i| i.key_pressed(egui::Key::Questionmark));
        if !asked || shell.screen != ShellScreen::Map || ctx.egui_wants_keyboard_input() {
            return None;
        }
        let tile = hovered? as usize;
        let world = self.session.as_ref()?.view();
        let rules = screens::game_rules();
        let cell = world.terrain.get(tile)?;
        let resource = cell.resource.filter(|r| world.unlocked_resources.contains(r));
        let improvement = world.improvements.get(tile).copied().flatten();
        let wonder = world.natural_wonders.get(tile).copied().flatten();
        let key = resource
            .and_then(|r| rules.resources.get(r as usize))
            .map(|d| d.key.clone())
            .or_else(|| improvement.and_then(|i| rules.improvements.get(i as usize)).map(|d| d.key.clone()))
            .or_else(|| wonder.and_then(|w| rules.natural_wonders.get(w as usize)).map(|d| d.key.clone()))
            .or_else(|| rules.terrains.iter().find(|t| t.map_code == Some(cell.terrain)).map(|d| d.key.clone()))?;
        pedia::game_pedia().by_key(&key)
    }

    /// Centre the map on what an overview row points at.
    fn focus(&mut self, focus: Focus) {
        let tile = match focus {
            Focus::Tile(tile) => Some(tile),
            Focus::Unit(unit) => self.session.as_mut().and_then(|s| s.focus_unit(unit)),
        };
        if let Some(tile) = tile {
            self.actions.push(ShellAction::CenterOn(tile));
        }
    }

    /// The match the setup screen asked for is created off the frame.
    fn begin_requested_match(&mut self, ctx: &egui::Context) {
        let Some(order) = self.screens.take_match_order() else {
            return;
        };
        self.seat = order.seat;
        let request = order.game;
        let (send, receive) = std::sync::mpsc::channel();
        self.match_worker = Some(receive);
        let repaint = ctx.clone();
        // A browser page has no thread: the world is made here, in this frame.
        #[cfg(target_arch = "wasm32")]
        {
            let _ = send.send(screens::prepare_game(request));
            repaint.request_repaint();
        }
        #[cfg(not(target_arch = "wasm32"))]
        std::thread::spawn(move || {
            let _ = send.send(screens::prepare_game(request));
            repaint.request_repaint();
        });
    }
}

/// The data of a game screen; each screen's own view is built only while it is shown.
pub fn screen_model_of(session: &mut Session, screen: Screen, hall_of_fame: &[String]) -> ScreenModel {
    let mut model = session.screen_model();
    match screen {
        Screen::Civics => model.civics = Some(session.civics_view()),
        Screen::Religion => model.religion = Some(session.religion_view()),
        Screen::Culture => model.culture = Some(session.culture_view()),
        Screen::Overview => {
            model.overview = Some(session.overview_view());
            model.diplomacy = Some(session.diplomacy_view());
        }
        Screen::Diplomacy => model.diplomacy = Some(session.diplomacy_view()),
        Screen::CityStates => model.city_states = Some(session.city_states_view()),
        Screen::Congress => model.congress = Some(session.congress_view()),
        Screen::Espionage => model.espionage = Some(session.espionage_view()),
        Screen::Victory => model.victory = Some(session.victory_view()),
        Screen::GameEnd => model.game_end = Some(session.game_end_view(hall_of_fame)),
        _ => {}
    }
    model
}

/// The screens that show the match (their views are built every frame they are shown).
fn shows_match(screen: Screen) -> bool {
    matches!(
        screen,
        Screen::City
            | Screen::Research
            | Screen::Civics
            | Screen::Religion
            | Screen::Culture
            | Screen::Overview
            | Screen::Diplomacy
            | Screen::CityStates
            | Screen::Congress
            | Screen::Espionage
            | Screen::Victory
            | Screen::GameEnd
    )
}

/// The shell's scene, else the match's view, else an empty world.
fn world_of<'a>(scene: Option<&'a RenderWorld>, session: Option<&'a Session>) -> &'a RenderWorld {
    match (scene, session) {
        (Some(scene), _) => scene,
        (None, Some(session)) => session.view(),
        (None, None) => empty_world(),
    }
}

fn empty_world() -> &'static RenderWorld {
    static EMPTY: std::sync::OnceLock<RenderWorld> = std::sync::OnceLock::new();
    EMPTY.get_or_init(RenderWorld::default)
}

impl ModuleUi for ClassicUi {
    fn screen_spec(&self, screen: ScreenId) -> ScreenSpec {
        screens::spec(Screen::of(screen), self.screens.dev_mode)
    }

    fn menu(&self, shell: ShellFlags) -> Vec<MenuGroup> {
        screens::menu(shell)
    }

    fn phone_canvas(&self, screen: ScreenId, window: egui::Vec2) -> egui::Vec2 {
        // The tree scrolls and the new-game screen's panels stack: both read well on a short canvas.
        // Held upright the window is narrow and tall: a narrower canvas, as tall as the window allows.
        let upright = window.x < window.y;
        match (Screen::of(screen), upright) {
            (Screen::Research, false) => egui::vec2(760., 380.),
            (Screen::Research, true) => egui::vec2(430., 640.),
            // A phone held sideways (the shell's slim header leaves about 311 points): the screen is laid out at
            // the window's own size, so that its 16-point text stays 16 points (`match_setup::draw`).
            (Screen::MatchSetup, false) if window.y < 480. => window,
            (Screen::MatchSetup, false) => egui::vec2(800., 400.),
            (Screen::MatchSetup, true) => egui::vec2(430., 760.),
            (Screen::Civics, false) => egui::vec2(900., 520.),
            _ => egui::vec2(1150., 640.),
        }
    }

    fn new_game_screen(&self) -> ScreenId {
        Screen::MatchSetup.id()
    }

    fn gallery_screen(&self) -> Option<ScreenId> {
        Some(Screen::Nations.id())
    }

    fn screen_opened(&mut self, screen: ScreenId, from: ShellScreen) {
        self.screens.opened(Screen::of(screen), from.into());
    }

    fn draw_screen(&mut self, ui: &mut egui::Ui, body: Rect, _: ScreenId, shell: ShellFlags, scene: Option<&RenderWorld>) {
        self.screens.sync(shell);
        let world = world_of(scene, self.session.as_ref());
        self.screens.draw(ui, body, world, self.model.as_ref());
    }

    fn back_from(&self, screen: ScreenId) -> Option<ShellScreen> {
        match Screen::of(screen) {
            Screen::MatchSetup => Some(ShellScreen::Menu),
            Screen::Encyclopedia => Some(self.screens.pedia_return().into()),
            _ => None,
        }
    }

    fn escape_to(&self, screen: ScreenId) -> Option<ShellScreen> {
        match Screen::of(screen) {
            Screen::Encyclopedia => Some(self.screens.pedia_return().into()),
            _ => None,
        }
    }

    fn creating_match(&self) -> bool {
        self.screens.creating_match()
    }

    fn key_help(&self) -> Vec<(&'static str, &'static str)> {
        vec![
            ("Esc", t!("classic.ui.hotkeys.escape")), // i18n-ok: keycap
            (t!("classic.ui.hotkeys.key-pointer"), t!("classic.ui.hotkeys.pointer")),
        ]
    }

    fn preload_screen_art(&mut self, ctx: &egui::Context, screen: ShellScreen) {
        self.screens.screen = screen.into();
        self.screens.preload_screen_art(ctx);
    }

    fn shortcuts(&self) -> client::shell::keymap::ShortcutSet {
        shortcuts::SET
    }

    fn shortcut(&mut self, id: &str, shell: ShellFlags) -> bool {
        if self.session.is_none() {
            return false;
        }
        self.screens.sync(shell);
        if let Some(orders) = shortcuts::order_of(id) {
            return self.unit_order(orders);
        }
        if let Some(tab) = shortcuts::overview_of(id) {
            self.screens.open_overview(tab);
            return true;
        }
        match id {
            "encyclopedia" => self.screens.open_pedia(None),
            "policies" => self.screens.open(Screen::Civics),
            "technologies" => self.screens.open(Screen::Research),
            "culture" => self.screens.open(Screen::Culture),
            "next-unit" | "previous-unit" => {
                let forward = id == "next-unit";
                if let Some(tile) = self.session.as_mut().and_then(|s| s.cycle_idle_unit(forward)) {
                    self.actions.push(ShellAction::CenterOn(tile));
                }
            }
            "next-city" => return self.step_city(true),
            "previous-city" => return self.step_city(false),
            _ => return false,
        }
        true
    }

    /// A click on a top-bar value: the screen it belongs to opens the way its dock button or key opens it.
    fn top_bar_clicked(&mut self, id: &str, shell: ShellFlags) {
        let Some(target) = TopTarget::of(id) else { return };
        if self.session.is_none() {
            return;
        }
        self.screens.sync(shell);
        match target {
            TopTarget::Economy => self.screens.open_overview(OverviewTab::Cities),
            TopTarget::Resources => self.screens.open_overview(OverviewTab::Resources),
            TopTarget::Research => self.screens.open(Screen::Research),
            TopTarget::Policies => self.screens.open(Screen::Civics),
            TopTarget::Religion => self.screens.open(Screen::Religion),
        }
    }

    fn cancel(&mut self) -> bool {
        self.screens.cancel_city_buy() || self.session.as_mut().is_some_and(Session::cancel_aim)
    }

    fn map_paint(&self) -> Option<&client::shell::module::MapPaint> {
        Some(&self.paint)
    }

    fn click(&mut self, tile: Option<u32>, button: MapButton) {
        let Some(session) = &mut self.session else {
            return;
        };
        session.click(tile.map(classic_rules::ids::TileId), button);
        if button == MapButton::Primary && session.selected_city_tile().is_some() {
            self.screens.open(Screen::City);
        }
    }

    /// A city banner was clicked: an own city opens its screen, a met city-state's opens its
    /// card over the map, any other city does nothing.
    fn select_city(&mut self, tile: u32) {
        let Some(session) = &mut self.session else {
            return;
        };
        let tile_id = classic_rules::ids::TileId(tile);
        if session.open_city_state_at(tile_id) {
            return;
        }
        if session.owns_city_at(tile_id) {
            self.request(GameRequest::SelectCity(tile));
        }
    }

    fn strike_city(&mut self, tile: u32) {
        self.request(GameRequest::CityStrike(tile));
    }

    fn observed(&mut self) -> Option<&mut dyn client::shell::observer::Observed> {
        self.observer.as_mut().map(|m| m as &mut dyn client::shell::observer::Observed)
    }

    fn view(&self) -> Option<&RenderWorld> {
        self.session.as_ref().map(Session::view)
    }

    fn update(&mut self) {
        let result = match self.match_worker.as_ref().map(|worker| worker.try_recv()) {
            Some(Ok(result)) => result,
            Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) => {
                Err(t!("classic.module.world-unfinished").into())
            }
            _ => return,
        };
        self.match_worker = None;
        match (result, self.seat) {
            (Ok(prepared), Seat::Play) => self.start(prepared),
            (Ok(prepared), Seat::Observe(watch)) => self.start_observer(prepared, watch),
            (Err(error), _) => self.screens.match_failed(error),
        }
    }

    fn art_books(&self) -> Option<(UnitModelBook, WorldBook)> {
        let rules = self.session.as_ref()?.rules();
        Some((UnitModelBook::from_rules(rules), WorldBook::from_rules(rules)))
    }

    fn turn(&self) -> u32 {
        self.session.as_ref().map_or(0, Session::turn)
    }

    fn era(&self) -> u8 {
        self.session.as_ref().map_or(0, Session::era)
    }

    fn outcome(&self) -> Outcome {
        self.session.as_ref().map_or(Outcome::Playing, Session::outcome)
    }

    fn hall_of_fame_line(&self) -> String {
        self.session.as_ref().map(Session::hall_of_fame_line).unwrap_or_default()
    }

    fn set_hall_of_fame(&mut self, lines: &[String]) {
        self.hall_of_fame = lines.to_vec();
    }

    fn game_ended(&mut self) {
        self.screens.open(Screen::GameEnd);
    }

    fn module_id(&self) -> ModuleId {
        <classic_rules::classic::Classic as classic_rules::module::RulesModule>::ID
    }

    /// The classic game came before the sandbox: the shared saves are its.
    fn adopts_shared_saves(&self) -> bool {
        true
    }

    fn save_bytes(&self) -> Option<Vec<u8>> {
        self.session.as_ref().filter(|s| !s.ai_busy()).map(Session::save_bytes)
    }

    fn describe_save(&self, payload: &[u8]) -> Result<SaveSummary, SaveProblem> {
        crate::game::describe_save(payload, &screens::game_rules())
    }

    fn describe_match(&self) -> Option<SaveSummary> {
        self.session.as_ref().map(Session::summary)
    }

    fn command_journal(&mut self) -> Option<&mut dyn client::shell::saves::CommandJournal> {
        self.session.as_mut().map(Session::command_journal)
    }

    fn load_bytes(&mut self, bytes: &[u8]) -> Result<(), String> {
        match &mut self.session {
            Some(session) => session.load_bytes(bytes)?,
            None => self.session = Some(Session::from_save(bytes, screens::game_rules())?),
        }
        self.observer = None;
        self.seed = None;
        self.policy_round = None;
        self.after_new_world();
        Ok(())
    }

    fn world_seed(&self) -> Option<u64> {
        self.seed
    }

    fn turn_running(&self) -> bool {
        self.session.as_ref().is_some_and(Session::turn_running)
    }

    fn begin_end_turn(&mut self) {
        let Some(session) = &mut self.session else { return };
        // The button, Enter and every other way to end the turn arrive here: a missing choice
        // opens its screen instead (the rules would accept the end of the turn).
        match missing_choices(session, self.end_turn_gate, self.policy_round).first() {
            Some(Choice::Research) => self.screens.open(Screen::Research),
            Some(Choice::Policy) => self.screens.open(Screen::Civics),
            None => session.begin_end_turn(),
        }
    }

    fn set_ai_wake(&mut self, wake: Option<client::ai_round::AiWake>) {
        self.ai_wake = wake;
    }

    fn tick_ai(&mut self, slice: Duration) -> AiProgress {
        if let (Some(wake), Some(session)) = (&self.ai_wake, self.session.as_mut()) {
            session.use_ai_worker(wake);
        }
        self.session.as_mut().map_or(AiProgress::Idle, |s| s.tick_ai(slice))
    }

    fn finish_ai_turns(&mut self) {
        if let Some(session) = &mut self.session {
            session.finish_ai_turns();
        }
    }

    fn selected_tile(&self) -> Option<u32> {
        self.session.as_ref().and_then(Session::selected_tile)
    }

    fn selected_unit_id(&self) -> Option<u64> {
        self.session.as_ref().and_then(Session::selected_unit_id)
    }

    fn unit_selected(&self) -> bool {
        self.hud.game.as_ref().is_some_and(|g| g.orders.is_some())
    }

    fn advance_presentation(&mut self, seconds: f32, settings: PresentationSettings) -> Frame {
        let Some(session) = &mut self.session else {
            return Frame::default();
        };
        session.set_presentation_settings(settings);
        session.advance_presentation(seconds)
    }

    fn presentation_idle(&self) -> bool {
        self.session.as_ref().map_or(true, |s| s.presentation().is_idle())
    }

    fn fit_camera(&mut self, screen: ShellScreen, fit: CameraFit<'_>) -> bool {
        self.screens.screen = screen.into();
        let Some(session) = self.session.as_ref().filter(|_| self.screens.screen == Screen::City) else {
            return false;
        };
        let Some(tile) = session.selected_city_tile() else {
            return false;
        };
        let world = session.view();
        center_on(fit.camera, world, tile);
        fit.camera.zoom = CITY_ZOOM;
        if let Some(city) = self.model.as_ref().and_then(|model| model.city.as_ref()) {
            self.screens.fit_city_camera(world, fit.camera, fit.surface, fit.screen, city);
        }
        true
    }

    fn unit_kinds(&self) -> Vec<KindLabel> {
        self.hud.unit_kinds.labels()
    }

    fn refresh(&mut self, input: FrameInput) -> Option<HudFeed> {
        let Some(session) = &mut self.session else {
            self.hud.game = None;
            self.model = None;
            self.paint = Default::default();
            return None;
        };
        if Screen::from(input.shell.screen) != Screen::Map {
            session.close_city_state();
        }
        let game = session.hud_model(input.hovered_tile);
        let missing = missing_choices(session, self.end_turn_gate, self.policy_round);
        self.paint = session.air_paint();
        self.hud.extras = session.hud_extras(input.hovered_badge);
        let screen = Screen::from(input.shell.screen);
        self.model = shows_match(screen).then(|| {
            let mut model = screen_model_of(session, screen, &self.hall_of_fame);
            if let (Some(view), Some(draft)) = (model.diplomacy.as_mut(), self.screens.deal_draft()) {
                view.preview = Some((draft.clone(), session.preview_deal(draft)));
            }
            model
        });
        let feed = HudFeed {
            turn: game.turn,
            nation: format!("{} · {}", game.nation, game.era),
            top_bar: Vec::new(),
            banners: self.hud.extras.banners.clone(),
            units: self.hud.extras.units.clone(),
            route: game.path.as_ref().map(|p| RouteFeed { tiles: p.tiles.clone(), this_turn: p.this_turn, turns: p.turns }),
            end_turn: Some(end_turn(&game, &missing)),
            reference: Some((Screen::Encyclopedia.into(), t!("classic.module.reference"))),
            status: None,
        };
        self.hud.game = Some(game);
        Some(HudFeed { top_bar: self.hud.top_bar(), ..feed })
    }

    fn prepare_ui(&mut self, map: MapView<'_>, shell: ShellFlags) {
        self.screens.screen = shell.screen.into();
        let world = world_of(None, self.session.as_ref());
        let city = self.model.as_ref().and_then(|model| model.city.as_ref());
        self.screens.prepare_city_map(world, map.camera, map.surface, map.screen, city);
    }

    fn hud_reserved(&self, view: &HudView<'_>) -> Option<Rect> {
        self.hud.reserved(view.screen, self.world(view.scene), view.selected_tile)
    }

    fn draw_hud(&mut self, ctx: &egui::Context, layer: HudLayer, view: &HudView<'_>) {
        let world = world_of(view.scene, self.session.as_ref());
        let clicks = self.hud.draw(ctx, layer, view.screen, world, view.selected_tile);
        self.panel_clicks(clicks);
    }

    fn draw_tile_hover(&mut self, ctx: &egui::Context, scene: Option<&RenderWorld>, tile: Option<u32>) {
        self.hud.draw_tile_hover(ctx, self.world(scene), tile);
    }

    fn after_ui(&mut self, ctx: &egui::Context, hovered_tile: Option<u32>, shell: ShellFlags) {
        let page = crate::ui::pedia_link::take(ctx).or_else(|| self.hovered_tile_page(ctx, hovered_tile, shell));
        if let Some(page) = page {
            self.screens.open_pedia(Some(page));
        }
        if let Some(focus) = self.screens.take_focus() {
            self.focus(focus);
        }
        let mut requests = std::mem::take(&mut self.pending);
        requests.append(&mut self.screens.requests);
        for request in requests {
            self.request(request);
        }
        self.begin_requested_match(ctx);
    }

    fn take_actions(&mut self) -> Vec<ShellAction> {
        let mut actions = std::mem::take(&mut self.screens.actions);
        actions.append(&mut self.actions);
        actions
    }

    fn hud_panels(&self, view: &HudView<'_>) -> Vec<client::shell::layout::Panel> {
        self.hud.declared_panels(view.screen, self.world(view.scene), view.selected_tile)
    }
}

/// The choices the end of the turn waits for (none when the gate is off).
fn missing_choices(session: &Session, gate: bool, policy_round: Option<u32>) -> Vec<Choice> {
    if !gate {
        return Vec::new();
    }
    session.missing_choices(policy_round == Some(session.turn()))
}

/// The end-turn button: ready, naming the first missing choice, waiting while the others (or the
/// autoplay) act, or over.
fn end_turn(game: &crate::ui::model::HudModel, missing: &[Choice]) -> EndTurn {
    match (game.outcome, game.can_end_turn) {
        (crate::ui::model::Outcome::Playing, true) => match missing {
            [] => EndTurn::Ready,
            [Choice::Research, Choice::Policy, ..] => EndTurn::Decide {
                label: t!("classic.module.end-turn-research").into(),
                tip: t!("classic.module.end-turn-both-tip").into(),
            },
            [Choice::Research, ..] => EndTurn::Decide {
                label: t!("classic.module.end-turn-research").into(),
                tip: t!("classic.module.end-turn-research-tip").into(),
            },
            [Choice::Policy, ..] => EndTurn::Decide {
                label: t!("classic.module.end-turn-policy").into(),
                tip: t!("classic.module.end-turn-policy-tip").into(),
            },
        },
        (crate::ui::model::Outcome::Playing, false) => EndTurn::Waiting,
        _ => EndTurn::Unavailable,
    }
}

fn center_on(camera: &mut CameraController, world: &RenderWorld, tile: u32) {
    let width = world.width.max(1);
    let (x, z) = render::scene::layout::tile_to_world(tile % width, tile / width);
    camera.x = x;
    camera.y = z;
}
