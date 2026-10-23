//! One running match: the single owner of the game client and all player-side UI state.
//!
//! Input (clicks, widget requests) -> typed `Command` -> `GameClient::submit` -> observer view.
//! The session never mutates the world except through commands, and everything it hands to
//! the UI comes from the observer's projection or the observer's own entities.
use classic_rules::ClassicGame;
use std::sync::Arc;

use client::selection::Selection;
use client::simulation_driver::{DriverConfig, SimulationDriver};
use client::GameClient;
use classic_rules::classic::Classic;
use classic_rules::command::{validate, CommandError, Refusal};
use classic_rules::ids::{CityHandle, PlayerId, TileId, UnitHandle};
use classic_rules::navigation::PathScratch;
use classic_rules::transaction::journal::{AppliedCommand, UnitChange};
use classic_rules::world::unit::UnitCore;
use classic_rules::{Command, CompiledRuleset, GameState};
use render::renderer::Renderer;
use render::world::state::RenderWorld;

use i18n::{t, tf};

use super::names;
use super::MapButton;
use crate::game::cues::{derive, Observer};
use client::shell::presentation::motion::Sound;
use client::shell::presentation::Presentation;
use crate::ui::model::{
    Action, Availability, GameRequest, HudModel, Notice, NoticeKind, Outcome, ScreenModel,
};

/// Notices kept for the notification log (the HUD lists the newest).
const NOTICE_LIMIT: usize = 200;

pub struct Session {
    pub(super) client: GameClient<Classic>,
    pub(super) driver: SimulationDriver<Classic>,
    pub(super) scratch: PathScratch,
    pub(super) targeting: Option<Action>,
    /// An own city aiming its ranged strike: the next map click picks the target.
    pub(super) city_strike: Option<CityHandle>,
    /// Units told to wait this turn (Skip) or until selected again (Sleep). UI-only:
    /// the core has no such state; it only removes the unit from "needs orders".
    pub(super) skipped: Vec<UnitHandle>,
    pub(super) sleeping: Vec<UnitHandle>,
    pub(super) notices: Vec<Notice>,
    pub(super) message: Option<String>,
    pub(super) path_cache: Option<super::path::PathCache>,
    /// The player's empire before the AI turns that run now (the notices of
    /// the round compare against it when they end).
    pub(super) pending_report: Option<super::turn::EmpireSnapshot>,
    /// What the player sees and hears of the commands that ran (client state only).
    pub(super) presentation: Presentation,
    /// Bumped by every change of the game the session makes or sees, so
    /// views of the whole empire are rebuilt only when it changed.
    pub(super) revision: u64,
    pub(super) cache: super::overview::ViewCache,
    /// Ancient Ruins whose card the player closed ("later"): the card stays shut while a unit
    /// stands there. UI-only; the ruins themselves stay on the map.
    pub(super) ruins_later: Option<TileId>,
}

impl Session {
    pub fn new(game: GameState, rules: Arc<CompiledRuleset>) -> Self {
        let difficulty = rules.default_difficulty().to_owned();
        Self::with_difficulty(game, rules, &difficulty)
    }

    /// Every player, the observed one included, is played by the AI from
    /// now on (offscreen QA runs: the observer stays alive and the
    /// presentation shows a whole game). The AI memory starts afresh.
    pub fn autoplay(&mut self) {
        let difficulty = self.driver.difficulty().to_owned();
        let config = DriverConfig { human_players: 0 };
        let mut driver = SimulationDriver::with_difficulty(config, &self.client.rules, &difficulty);
        driver.configure_game(&mut self.client.game);
        driver.record_journal();
        self.driver = driver;
    }

    /// The observed player is the human again (the end of an autoplayed stretch: the
    /// frame benches measure a game that waits for its player).
    pub fn end_autoplay(&mut self) {
        let difficulty = self.driver.difficulty().to_owned();
        let mut driver = SimulationDriver::with_difficulty(DriverConfig::default(), &self.client.rules, &difficulty);
        driver.configure_game(&mut self.client.game);
        driver.record_journal();
        self.driver = driver;
    }

    /// A session whose AI players play at `difficulty` (a handicap key).
    pub fn with_difficulty(game: GameState, rules: Arc<CompiledRuleset>, difficulty: &str) -> Self {
        Self::starting(game, rules, difficulty, None)
    }

    /// A session over `game` seen through `observer`'s eyes (`None`: the first human's).
    pub(super) fn starting(
        game: GameState,
        rules: Arc<CompiledRuleset>,
        difficulty: &str,
        observer: Option<PlayerId>,
    ) -> Self {
        let mut game = game;
        let driver = SimulationDriver::with_difficulty(DriverConfig::default(), &rules, difficulty);
        driver.configure_game(&mut game);
        let human = observer.unwrap_or_else(|| driver.config.first_human());
        let mut client = GameClient::new(game, rules, Renderer::headless(), human);
        client.render_world.visual_surface = true;
        client.record_journal();
        let mut driver = driver;
        driver.record_journal();
        let mut presentation = Presentation::default();
        presentation.reset(client.render_world.width);
        let mut session = Self {
            client,
            driver,
            scratch: PathScratch::default(),
            targeting: None,
            city_strike: None,
            skipped: Vec::new(),
            sleeping: Vec::new(),
            notices: Vec::new(),
            message: None,
            path_cache: None,
            pending_report: None,
            presentation,
            revision: 0,
            cache: Default::default(),
            ruins_later: None,
        };
        if observer.is_none() {
            session.select_next_idle_unit();
        }
        session
    }

    /// Cues of the commands applied since the last call join the presentation.
    pub(super) fn present_journal(&mut self) {
        let records = self.client.take_journal();
        if records.is_empty() {
            return;
        }
        self.notify_ruins(&records);
        self.notify_nuclear(&records);
        let observer = Observer::new(self.client.game.view(), &self.client.rules, self.me());
        let cues = derive(&observer, &records);
        self.presentation.push(cues);
    }

    /// A notice for every nuclear weapon used in `records`, whoever used it and wherever it fell: a
    /// nuclear strike is a public act (the civilizations that met its user remember it,
    /// classic/docs/classic/RESOURCING.md, "Infamy of a nuclear strike"), so the player is told.
    pub(super) fn notify_nuclear(&mut self, records: &[AppliedCommand]) {
        for record in records {
            let Command::Nuke { unit, target } = record.command else { continue };
            let user = record.changes.unit_changes.iter().find_map(|change| match *change {
                UnitChange::Destroyed { unit: gone, owner, .. } if gone == unit => Some(owner),
                _ => None,
            });
            if let Some(user) = user {
                let nation = self.player_name(user);
                self.notify_at(NoticeKind::Combat, tf!("classic.game.turn.nuclear-strike", nation = nation), Some(target.0));
            }
        }
    }

    /// One frame of the presentation: `seconds` of wall time pass.
    pub fn advance_presentation(&mut self, seconds: f32) -> client::shell::presentation::Frame {
        self.presentation.advance(seconds, &self.client.render_world)
    }

    pub fn presentation(&self) -> &Presentation {
        &self.presentation
    }

    pub fn set_presentation_settings(&mut self, settings: client::shell::presentation::Settings) {
        self.presentation.set_settings(settings);
    }

    pub fn me(&self) -> PlayerId {
        self.client.observer
    }

    pub fn game(&self) -> &GameState {
        &self.client.game
    }

    pub fn rules(&self) -> &CompiledRuleset {
        &self.client.rules
    }

    /// Observer view for the visual surface: the client's projection, whose layer
    /// revisions change only when that layer's picture does.
    pub fn view(&self) -> &RenderWorld {
        &self.client.render_world
    }

    pub fn selection(&self) -> Selection {
        self.client.selection
    }

    /// Tile of the selected unit, city or tile (for highlights and the camera).
    pub fn selected_tile(&self) -> Option<u32> {
        let auth = self.client.game.view().auth;
        match self.client.selection {
            Selection::Unit(unit) => auth.units().get(unit).map(|u| u.core.tile.0),
            Selection::City(city) => auth.cities().get(city).map(|c| c.tile.0),
            Selection::Tile(tile) => Some(tile.0),
            Selection::None => None,
        }
    }

    /// Persistent id of the selected own unit (the presentation may be moving it).
    pub fn selected_unit_id(&self) -> Option<u64> {
        self.selected_unit().map(|(_, core)| core.id.0)
    }

    /// The player's era (0 ancient .. 7 information): the music follows it.
    pub fn era(&self) -> u8 {
        self.client.game.view().auth.players().civ_era_of(self.me(), &self.client.rules)
    }

    pub fn selected_city_tile(&self) -> Option<u32> {
        match self.client.selection {
            Selection::City(_) => self.selected_tile(),
            _ => None,
        }
    }

    /// Content key of the game's difficulty.
    pub fn difficulty(&self) -> &str {
        self.driver.difficulty()
    }

    pub fn world_hash(&self) -> u64 {
        classic_rules::replay::hash::authoritative_hash(&self.client.game)
    }

    /// Display turn: the core's global round, counted from 1.
    pub fn turn(&self) -> u32 {
        self.client.game.view().auth.turn().round + 1
    }

    /// Playing until somebody won (or the player fell); "one more turn" plays on.
    pub fn outcome(&self) -> Outcome {
        let game = &self.client.game;
        if !game.view().auth.players().is_alive(self.me()) {
            return Outcome::Defeat;
        }
        if game.outcome().is_some() && !game.is_over() {
            return Outcome::Playing;
        }
        match game.winner() {
            Some(winner) if winner == self.me() => Outcome::Victory,
            Some(_) => Outcome::Defeat,
            None => Outcome::Playing,
        }
    }

    pub fn hud_model(&mut self, hover: Option<u32>) -> HudModel {
        let combat = self.combat_odds(hover);
        let capture = self.capture_choice();
        let ruins = self.ruins_choice();
        HudModel {
            combat,
            capture,
            ruins,
            turn: self.turn(),
            civilization: self.civilization_name(self.me()),
            era: names::era(self.rules(), self.era()),
            economy: self.economy(),
            research: self.research_summary(),
            orders: self.unit_orders(),
            path: self.path_preview(hover),
            notices: self.notices.clone(),
            message: self.hud_message(),
            outcome: self.outcome(),
            can_end_turn: self.outcome() == Outcome::Playing && !self.ai_busy(),
            rivals: self.rivals(),
        }
    }

    pub fn screen_model(&mut self) -> ScreenModel {
        ScreenModel {
            city: self.city_view(),
            research: self.research_view(),
            civics: None,
            religion: None,
            culture: None,
            diplomacy: None,
            city_states: None,
            congress: None,
            espionage: None,
            victory: None,
            game_end: None,
            message: self.message.clone(),
            overview: None,
            turn: self.turn(),
            save_status: None,
        }
    }

    pub fn civilization_name(&self, player: PlayerId) -> String {
        let civ = self.client.game.view().auth.players().civ_of(player);
        match self.client.rules.civs.get(civ as usize) {
            Some(def) => names::named("nation", &def.name, &def.key),
            None => tf!("classic.game.session.player-n", n = player.0 as u32 + 1),
        }
    }

    /// The HUD line: the AI turns while they run, else the last message.
    fn hud_message(&self) -> Option<String> {
        if self.ai_busy() {
            return Some(ai_turns_message().into());
        }
        self.message.clone()
    }

    pub fn request(&mut self, request: GameRequest) {
        if self.ai_busy() && !matches!(request, GameRequest::SelectCity(_)) {
            self.message = Some(ai_turns_message().into());
            return;
        }
        match request {
            GameRequest::Unit(action) => self.order(action),
            GameRequest::EndTurn => self.end_turn(),
            GameRequest::SelectCity(tile) => self.select_city_at(TileId(tile)),
            GameRequest::CityStrike(tile) => self.aim_city_strike(TileId(tile)),
            GameRequest::Research(tech) => self.choose_research(tech),
            GameRequest::Save | GameRequest::Load | GameRequest::Continue => {}
            GameRequest::MakePeace(player) => self.diplomacy(Command::MakePeace {
                player: self.me(),
                enemy: PlayerId(player),
            }),
            GameRequest::OpenBorders(player) => self.diplomacy(Command::OpenBorders {
                host: self.me(),
                guest: PlayerId(player),
            }),
            GameRequest::UnlockBranch(_)
            | GameRequest::AdoptPolicy(_)
            | GameRequest::ChooseIdeology(_) => self.civics_request(request),
            GameRequest::FoundPantheon(_)
            | GameRequest::FoundReligion { .. }
            | GameRequest::EnhanceReligion { .. }
            | GameRequest::AddReformation(_)
            | GameRequest::BuyWithFaith { .. } => self.religion_request(request),
            GameRequest::SwitchIdeology
            | GameRequest::ChooseArchaeology { .. }
            | GameRequest::ChooseFreeGreatPerson(_) => {
                self.culture_request(request)
            }
            GameRequest::ClaimRuins(_) | GameRequest::PostponeRuins => self.ruins_request(request),
            GameRequest::Promote(_) | GameRequest::DecideCity { .. } => {
                self.combat_request(request)
            }
            GameRequest::DeclareWar(_)
            | GameRequest::Denounce(_)
            | GameRequest::ProposeDeal { .. }
            | GameRequest::RespondDeal { .. } => self.deal_request(request),
            GameRequest::GiftGold { .. }
            | GameRequest::BullyGold(_)
            | GameRequest::BullyWorker(_)
            | GameRequest::BuyoutCityState(_)
            | GameRequest::Pledge { .. } => self.city_state_request(request),
            GameRequest::ProposeResolution { .. } | GameRequest::CastVotes { .. } => {
                self.congress_request(request)
            }
            GameRequest::MoveSpy { .. }
            | GameRequest::StealTechnology { .. }
            | GameRequest::StageCoup(_) => self.espionage_request(request),
            GameRequest::ContinueAfterVictory => self.continue_after_victory(),
            GameRequest::EstablishTradeRoute { unit, destination, kind } => {
                self.establish_route(unit, destination, kind)
            }
            other => self.city_request(other),
        }
    }

    pub fn click(&mut self, tile: Option<TileId>, button: MapButton) {
        let Some(tile) = tile else {
            self.client.selection = Selection::None;
            self.targeting = None;
            self.city_strike = None;
            return;
        };
        if self.ai_busy() {
            // Other players act: selection only, no orders until the turn is back.
            self.targeting = None;
            self.city_strike = None;
            if button == MapButton::Primary {
                self.select_without_orders(tile);
            }
            return;
        }
        if let Some(city) = self.city_strike.take() {
            // A city aiming its strike: either button fires at the tile.
            return self.city_strike_at(city, tile);
        }
        match button {
            MapButton::Primary => self.primary_click(tile),
            MapButton::Secondary => self.attack(tile),
        }
    }

    fn select_without_orders(&mut self, tile: TileId) {
        if let Some(unit) = self.next_own_unit_at(tile) {
            return self.select_unit(unit);
        }
        if let Some(city) = self.own_city_at(tile) {
            self.client.selection = Selection::City(city);
            return;
        }
        self.client.selection = Selection::Tile(tile);
    }

    fn primary_click(&mut self, tile: TileId) {
        match self.targeting.take() {
            Some(Action::Attack) => return self.attack(tile),
            Some(Action::Move) => return self.move_selected(tile),
            Some(Action::Sweep) => return self.sweep(tile),
            _ => {}
        }
        if let Some(unit) = self.next_own_unit_at(tile) {
            self.presentation.play_sound(Sound::Select);
            return self.select_unit(unit);
        }
        if let Some(city) = self.own_city_at(tile) {
            self.client.selection = Selection::City(city);
            return;
        }
        if self.selected_unit().is_some() {
            return self.move_selected(tile);
        }
        self.client.selection = Selection::Tile(tile);
    }

    /// Esc: leave the target mode (move, attack, sweep) or the city strike aim; true
    /// when there was one to leave.
    pub fn cancel_aim(&mut self) -> bool {
        let aiming = self.targeting.is_some() || self.city_strike.is_some();
        self.targeting = None;
        self.city_strike = None;
        aiming
    }

    pub fn select_city_at(&mut self, tile: TileId) {
        if let Some(city) = self.own_city_at(tile) {
            self.client.selection = Selection::City(city);
            self.targeting = None;
            self.city_strike = None;
        }
    }

    /// The own city on `tile` aims its ranged strike (the banner's bow);
    /// the next map click is the target.
    fn aim_city_strike(&mut self, tile: TileId) {
        let Some(city) = self.own_city_at(tile) else {
            return;
        };
        self.targeting = None;
        self.city_strike = Some(city);
        self.message = Some(t!("classic.game.session.city-strike").into());
    }

    /// The aiming city strikes the unit on `tile`.
    fn city_strike_at(&mut self, city: CityHandle, tile: TileId) {
        let command = Command::CityStrike { city, target: tile };
        if self.submit(command) {
            self.presentation.play_sound(Sound::Select);
        }
    }

    /// The city aiming its strike, if any (the HUD shows the odds of the
    /// hovered target).
    pub fn aiming_city(&self) -> Option<CityHandle> {
        self.city_strike
    }

    pub fn select_unit(&mut self, unit: UnitHandle) {
        self.client.selection = Selection::Unit(unit);
        self.targeting = None;
        self.sleeping.retain(|u| *u != unit);
    }

    /// Own unit on the tile; repeated clicks cycle through a stack.
    fn next_own_unit_at(&self, tile: TileId) -> Option<UnitHandle> {
        let auth = self.client.game.view().auth;
        let here: Vec<UnitHandle> = auth.indexes().units_on(tile).to_vec();
        let mine: Vec<UnitHandle> = here
            .into_iter()
            .filter(|h| auth.units().get(*h).map(|u| u.core.owner) == Some(self.me()))
            .collect();
        let current = self.client.selection.selected_unit();
        let position = mine.iter().position(|h| Some(*h) == current);
        match position {
            Some(i) if mine.len() > 1 => Some(mine[(i + 1) % mine.len()]),
            Some(_) => None,
            None => mine.first().copied(),
        }
    }

    pub(super) fn own_city_at(&self, tile: TileId) -> Option<CityHandle> {
        let auth = self.client.game.view().auth;
        let city = auth.indexes().city_on_tile(tile)?;
        let owner = auth.cities().get(city)?.owner;
        if owner == self.me() {
            Some(city)
        } else {
            None
        }
    }

    pub(super) fn selected_unit(&self) -> Option<(UnitHandle, &UnitCore)> {
        let handle = self.client.selection.selected_unit()?;
        let unit = self.client.game.view().auth.units().get(handle)?;
        if unit.core.owner != self.me() {
            return None;
        }
        Some((handle, unit.core))
    }

    /// Result of `validate` for a command, without executing it.
    pub(super) fn check(&mut self, command: &Command) -> Result<(), CommandError> {
        let view = self.client.game.view();
        validate(view, &self.client.rules, command.clone(), &mut self.scratch).map(|_| ())
    }

    pub(super) fn availability(&mut self, command: &Command) -> Availability {
        match self.check(command) {
            Ok(()) => Availability::Ready,
            Err(error) => Availability::Blocked(names::reason(self.rules(), &error)),
        }
    }

    /// Execute one interactive command; failures become the short HUD message.
    pub(super) fn submit(&mut self, command: Command) -> bool {
        match self.client.submit(command) {
            Ok(()) => {
                self.message = None;
                // Moves left may change without any picture change.
                self.path_cache = None;
                self.present_journal();
                self.revision += 1;
                true
            }
            Err(error) => {
                self.message = Some(names::reason(self.rules(), &error));
                false
            }
        }
    }

    pub(super) fn notify(&mut self, kind: NoticeKind, text: String) {
        self.notify_at(kind, text, None);
    }

    /// A notice about something on the map: clicking it centres the camera there.
    pub(super) fn notify_at(&mut self, kind: NoticeKind, text: String, tile: Option<u32>) {
        let turn = self.turn();
        self.notices.insert(0, Notice { turn, kind, text, tile });
        self.notices.truncate(NOTICE_LIMIT);
    }

    fn move_selected(&mut self, tile: TileId) {
        let Some((unit, core)) = self.selected_unit() else {
            return;
        };
        if core.moves_left == 0 {
            self.message = Some(super::reasons::no_moves().into());
            return;
        }
        if let Some(order) = self.air_move_order(unit, tile) {
            match self.check(&order) {
                Ok(()) => {
                    self.submit(order);
                }
                Err(error) => self.message = Some(names::reason(self.rules(), &error)),
            }
            return;
        }
        let direct = Command::MoveUnit {
            unit,
            destination: tile,
        };
        match self.check(&direct) {
            Ok(()) => {
                self.submit(direct);
            }
            Err(CommandError::Refused(Refusal::NoMovesLeft)) => self.march(unit, tile),
            Err(error) => self.message = Some(names::reason(self.rules(), &error)),
        }
    }

    /// A target beyond this turn's moves: the core walks the unit as far as
    /// its moves go (one MarchTo command).
    fn march(&mut self, unit: UnitHandle, tile: TileId) {
        let order = Command::MarchTo {
            unit,
            destination: tile,
        };
        self.submit(order);
    }

    /// Harness-only (offscreen benches): the game for classic-rules's debug setters. Call
    /// `refresh_after_harness` afterwards so the projection follows.
    pub fn harness_game(&mut self) -> (&mut GameState, Arc<CompiledRuleset>) {
        (&mut self.client.game, self.client.rules.clone())
    }

    pub fn refresh_after_harness(&mut self) {
        self.client.refresh_projection();
        self.path_cache = None;
        self.present_journal();
        self.revision += 1;
    }

    /// A session for a saved game, with its AI players' memory.
    pub fn from_save(bytes: &[u8], rules: Arc<CompiledRuleset>) -> Result<Self, String> {
        let (game, ai) = decode_save(bytes, &rules)?;
        let mut session = Self::new(game, rules);
        session
            .driver
            .load_ai(&ai, &session.client)
            .map_err(|e| tf!("classic.game.session.save-cannot-load", error = e.to_string()))?;
        Ok(session)
    }

    /// The match's command journal (`client::shell::saves::journal`).
    pub fn command_journal(&mut self) -> &mut dyn client::shell::saves::CommandJournal {
        &mut self.client
    }

    /// The game and the AI players' memory (call between turns: not `ai_busy`).
    pub fn save_bytes(&self) -> Vec<u8> {
        let mut save = classic_rules::persistence::encode::encode(&self.client.game, &self.client.rules);
        save.ai = self.driver.save_ai();
        classic_rules::save_codec::to_bytes(&save)
    }

    pub fn load_bytes(&mut self, bytes: &[u8]) -> Result<(), String> {
        let (game, ai) = decode_save(bytes, &self.client.rules)?;
        self.replace_game(game);
        self.driver
            .load_ai(&ai, &self.client)
            .map_err(|e| tf!("classic.game.session.save-cannot-load", error = e.to_string()))
    }

    /// New match or loaded save: same rules, fresh UI state and AI memory. The projection
    /// refresh keeps layer revisions monotonic, so the renderer rebuilds exactly what differs.
    pub fn replace_game(&mut self, game: GameState) {
        // A save plays at its own difficulty.
        let saved = game.view().auth.settings().difficulty.clone();
        let difficulty = if saved.is_empty() {
            self.driver.difficulty().to_owned()
        } else {
            saved
        };
        self.replace_game_with(game, &difficulty);
    }

    /// A new match at `difficulty`: same rules, fresh UI state and AI memory.
    pub fn replace_game_with(&mut self, game: GameState, difficulty: &str) {
        let mut game = game;
        self.driver.reset_with_difficulty(&self.client.rules, difficulty);
        self.driver.configure_game(&mut game);
        self.pending_report = None;
        self.client.replace_game(game);
        self.presentation.reset(self.client.render_world.width);
        self.targeting = None;
        self.skipped.clear();
        self.sleeping.clear();
        self.notices.clear();
        self.message = None;
        self.path_cache = None;
        self.revision += 1;
        self.select_next_idle_unit();
    }
}

/// The rules' answer on a deal item ("why not", None: it may) as a button state.
pub(super) fn availability_of(problem: Option<classic_rules::world::relations::DealBlock>) -> Availability {
    match problem {
        None => Availability::Ready,
        Some(block) => Availability::Blocked(super::reasons::deal(block).into()),
    }
}

/// Shown while the other civilizations take their turns.
pub fn ai_turns_message() -> &'static str {
    t!("classic.game.session.ai-turns")
}

pub fn decode_game(bytes: &[u8], rules: &CompiledRuleset) -> Result<GameState, String> {
    decode_save(bytes, rules).map(|(game, _)| game)
}

/// The game of a save and the AI memory it carries.
pub fn decode_save(
    bytes: &[u8],
    rules: &CompiledRuleset,
) -> Result<(GameState, classic_rules::persistence::ai::SavedAiStates), String> {
    let save = classic_rules::save_codec::from_bytes(bytes).map_err(|e| e.to_string())?;
    let game = classic_rules::persistence::decode::decode_snapshot(&save, rules)
        .map_err(|e| tf!("classic.game.session.save-cannot-load", error = e.to_string()))?;
    game.validate(rules)
        .map_err(|e| tf!("classic.game.session.save-inconsistent", error = e.to_string()))?;
    Ok((game, save.ai))
}

#[cfg(test)]
mod city_name_snapshot_tests {
    #[test]
    fn desktop_loads_pre_name_catalog_snapshot_without_renaming_or_losing_ai() {
        let bytes = include_bytes!("../../../../crates/classic-rules/tests/fixtures/city-names-before-v1.aurelia");
        let rules = classic_rules::compile_rules(classic_content::classic::classic_rules()).unwrap();
        let original = classic_rules::save_codec::from_bytes(bytes).unwrap();
        let (game, ai) = super::decode_save(bytes,&rules).unwrap();
        assert_eq!(classic_rules::replay::authoritative_hash(&game),156481080629868776);
        assert_eq!(ai, original.ai);
        assert_eq!(game.view().auth.cities().iter().next().unwrap().1.name,"My Custom Capital");
    }
}
