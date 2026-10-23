//! End of turn: the human EndTurn, then the AI players' turns through `SimulationDriver`
//! in frame slices (the window stays responsive), then notices for what changed in the
//! player's empire.
use classic_rules::ClassicGame;
use std::collections::HashMap;
use std::time::Duration;

use client::selection::Selection;
use i18n::{t, tf};
use client::simulation_driver::TickOutcome;
use classic_rules::ids::{CityHandle, TechId, UnitHandle, UnitKindId};

use super::names;
use super::AiProgress;
use super::session::Session;
use crate::ui::model::{NoticeKind, Outcome};


/// The player's own empire before a round, compared after it.
pub(super) struct EmpireSnapshot {
    cities: HashMap<CityHandle, CitySnapshot>,
    units: HashMap<UnitHandle, (UnitKindId, u16, u32)>,
    techs: Vec<TechId>,
    /// The player's era (the latest era of its technologies).
    era: u8,
    /// Golden age turns and the happiness snapshot of the empire.
    society: (u16, i32),
    /// Finished digs waiting for a choice, and the public opinion.
    digs: usize,
    opinion: classic_rules::world::tourism::PublicOpinion,
    /// Totals and free copies of the empire's resources.
    resources: Vec<super::resources::ResourceMark>,
    /// Own trade routes.
    routes: Vec<super::trade::RouteMark>,
    /// Own spies: name, rank, state.
    spies: Vec<(u8, u8, classic_rules::world::espionage::SpyState)>,
}

struct CitySnapshot {
    name: String,
    tile: u32,
    population: u32,
    buildings: usize,
    damage: u16,
}

impl Session {
    /// End the turn and run every AI turn at once (tests, offscreen runs).
    pub fn end_turn(&mut self) {
        self.begin_end_turn();
        self.finish_ai_turns();
    }

    /// End the human's turn; the AI turns follow frame by frame (`tick_ai`).
    pub fn begin_end_turn(&mut self) {
        if self.outcome() != Outcome::Playing || self.ai_busy() {
            return;
        }
        let before = self.snapshot();
        if let Err(error) = self.driver.end_human_turn(&mut self.client) {
            self.message = Some(names::reason(self.rules(), &error));
            return;
        }
        self.present_journal();
        self.pending_report = Some(before);
        self.path_cache = None;
        self.targeting = None;
    }

    /// Let a worker thread play the other players' turns (native hosts); `wake` repaints the
    /// window when it has an answer. Idempotent: a new match asks again.
    pub fn use_ai_worker(&mut self, wake: &client::ai_round::AiWake) {
        #[cfg(not(target_arch = "wasm32"))]
        if !self.driver.offloaded() {
            self.driver.offload_on_thread(self.client.rules.clone(), wake.clone());
        }
        #[cfg(target_arch = "wasm32")]
        let _ = wake;
    }

    /// Other players act now: gameplay input waits, the camera does not.
    pub fn ai_busy(&self) -> bool {
        self.driver.busy(&self.client) || self.ai_turn_after_victory()
    }

    /// The end of the human's turn is still running: AI turns, or their report that the
    /// next `tick_ai` writes.
    pub fn turn_running(&self) -> bool {
        self.ai_busy() || self.pending_report.is_some()
    }

    /// After "one more turn" an AI player is to move. The driver's own check stops at
    /// any winner, so the session asks for the AI turns itself until the round ends.
    fn ai_turn_after_victory(&self) -> bool {
        let game = &self.client.game;
        let auth = game.view().auth;
        let active = auth.active_player();
        game.outcome().is_some()
            && !game.is_over()
            && active != self.me()
            && auth.players().is_alive(active)
    }

    /// Advance the AI turns for one frame slice of wall time.
    pub fn tick_ai(&mut self, slice: Duration) -> AiProgress {
        if self.ai_busy() {
            let outcome = self.driver.tick(&mut self.client, slice);
            self.present_journal();
            if let Some(why) = self.driver.take_failure() {
                self.message = Some(tf!("classic.game.turn.ai-stopped", why = why));
            }
            if outcome == TickOutcome::Busy {
                return AiProgress::Busy;
            }
        }
        let Some(before) = self.pending_report.take() else {
            return AiProgress::Idle;
        };
        self.finish_end_turn(&before);
        AiProgress::Finished
    }

    /// Run the AI turns that are left to their end now.
    pub fn finish_ai_turns(&mut self) {
        while self.tick_ai(Duration::from_secs(3600)) == AiProgress::Busy {}
    }

    fn finish_end_turn(&mut self, before: &EmpireSnapshot) {
        self.path_cache = None;
        self.revision += 1;
        self.skipped.clear();
        self.targeting = None;
        self.message = None;
        self.report_changes(before);
        self.report_outcome();
        self.keep_or_advance_selection();
    }

    pub(super) fn snapshot(&self) -> EmpireSnapshot {
        let auth = self.game().view().auth;
        let mut cities = HashMap::new();
        for (handle, core) in auth.cities().iter() {
            if core.owner == self.me() {
                let city = CitySnapshot {
                    name: core.name.clone(),
                    tile: core.tile.0,
                    population: core.population,
                    buildings: core.buildings.len(),
                    damage: core.damage,
                };
                cities.insert(handle, city);
            }
        }
        let mut units = HashMap::new();
        for (handle, core) in auth.units().iter_handles() {
            if core.owner == self.me() {
                units.insert(handle, (core.kind, core.hp, core.tile.0));
            }
        }
        let techs = auth.players().techs_of(self.me()).to_vec();
        let society = auth
            .players()
            .economy_of(self.me())
            .map(|e| (e.golden_age_turns, e.excess_happiness))
            .unwrap_or_default();
        let me = self.me();
        let digs = auth
            .module()
            .tourism()
            .archaeology()
            .pending
            .iter()
            .filter(|p| p.player == me)
            .count();
        EmpireSnapshot {
            cities,
            units,
            techs,
            era: self.era(),
            society,
            digs,
            opinion: auth.module().tourism().player(me).opinion,
            resources: self.resource_marks(),
            routes: self.route_marks(),
            spies: self.spy_marks(),
        }
    }

    fn report_changes(&mut self, before: &EmpireSnapshot) {
        self.report_cities(before);
        self.report_units(before);
        self.report_research(before);
        self.report_society(before);
        self.report_culture(before);
        self.report_resources(&before.resources);
        self.report_trade(&before.routes);
        self.report_spies(&before.spies);
    }

    /// Golden ages starting and ending, the empire turning unhappy.
    fn report_society(&mut self, before: &EmpireSnapshot) {
        use classic_rules::derived::happiness::{mood, Mood};
        let Some(economy) = self.game().view().auth.players().economy_of(self.me()).cloned() else {
            return;
        };
        let (turns_before, excess_before) = before.society;
        if turns_before == 0 && economy.golden_age_turns > 0 {
            let text = tf!("classic.game.turn.golden-age-begins", n = economy.golden_age_turns);
            self.notify(NoticeKind::Growth, text);
        } else if turns_before > 0 && economy.golden_age_turns == 0 {
            self.notify(NoticeKind::Growth, t!("classic.game.turn.golden-age-ended").into());
        }
        let now = mood(economy.excess_happiness);
        if now != mood(excess_before) {
            let text = match now {
                Mood::Content => t!("classic.game.turn.mood-content").to_string(),
                Mood::Unhappy => tf!("classic.game.turn.mood-unhappy", value = format!("{:+}", economy.excess_happiness)),
                Mood::VeryUnhappy => tf!("classic.game.turn.mood-very-unhappy", value = format!("{:+}", economy.excess_happiness)),
            };
            self.notify(NoticeKind::Growth, text);
        }
    }

    /// A dig finished (its choice waits on the Culture screen); the public
    /// opinion of the ideology changed.
    fn report_culture(&mut self, before: &EmpireSnapshot) {
        let now = self.snapshot();
        if now.digs > before.digs {
            self.notify(
                NoticeKind::Culture,
                t!("classic.game.turn.dig-finished").into(),
            );
        }
        if now.opinion != before.opinion {
            let text = match now.opinion {
                classic_rules::world::tourism::PublicOpinion::Content => {
                    t!("classic.game.turn.opinion-content")
                }
                classic_rules::world::tourism::PublicOpinion::Murmurs => {
                    t!("classic.game.turn.opinion-murmurs")
                }
                classic_rules::world::tourism::PublicOpinion::Unrest => {
                    t!("classic.game.turn.opinion-unrest")
                }
                classic_rules::world::tourism::PublicOpinion::Upheaval => {
                    t!("classic.game.turn.opinion-upheaval")
                }
            };
            self.notify(NoticeKind::Culture, text.into());
        }
    }

    fn report_cities(&mut self, before: &EmpireSnapshot) {
        let rules = self.client.rules.clone();
        let mut notes = Vec::new();
        for (handle, old) in &before.cities {
            let at = Some(old.tile);
            let Some(city) = self.game().view().auth.cities().get(*handle) else {
                notes.push((NoticeKind::Combat, tf!("classic.game.turn.city-lost", city = old.name.as_str()), at));
                continue;
            };
            if city.owner != self.me() {
                notes.push((NoticeKind::Combat, tf!("classic.game.turn.city-captured", city = old.name.as_str()), at));
                continue;
            }
            if city.population > old.population {
                let text = tf!("classic.game.turn.city-grew", city = city.name.as_str(), n = city.population);
                notes.push((NoticeKind::Growth, text, at));
            }
            for building in city.buildings.iter().skip(old.buildings) {
                let text = tf!("classic.game.turn.city-built", city = city.name.as_str(), building = names::building(&rules, *building));
                notes.push((NoticeKind::Production, text, at));
            }
            if city.damage > old.damage {
                notes.push((NoticeKind::Combat, tf!("classic.game.turn.city-attacked", city = city.name.as_str()), at));
            }
        }
        for (kind, text, tile) in notes {
            self.notify_at(kind, text, tile);
        }
    }

    fn report_units(&mut self, before: &EmpireSnapshot) {
        let mut notes = Vec::new();
        let auth = self.game().view().auth;
        for (handle, (kind, hp, tile)) in &before.units {
            let name = names::unit(self.rules(), *kind);
            match auth.units().get(*handle) {
                None => notes.push((NoticeKind::Combat, tf!("classic.game.turn.unit-destroyed", name = name), *tile)),
                Some(u) if u.core.hp < *hp => {
                    let text = tf!("classic.game.turn.unit-attacked", name = name, hp = u.core.hp);
                    notes.push((NoticeKind::Combat, text, u.core.tile.0));
                }
                Some(_) => {}
            }
        }
        for (handle, core) in auth.units().iter_handles() {
            if core.owner == self.me() && !before.units.contains_key(&handle) {
                let name = names::unit(self.rules(), core.kind);
                notes.push((NoticeKind::Production, tf!("classic.game.turn.unit-trained", name = name), core.tile.0));
            }
        }
        for (kind, text, tile) in notes {
            self.notify_at(kind, text, Some(tile));
        }
    }

    pub(super) fn report_research(&mut self, before: &EmpireSnapshot) {
        let learned: Vec<TechId> = self
            .game()
            .view()
            .auth
            .players()
            .techs_of(self.me())
            .iter()
            .filter(|t| !before.techs.contains(t))
            .copied()
            .collect();
        for tech in learned {
            let text = tf!("classic.game.turn.researched", tech = names::tech(self.rules(), tech));
            self.notify(NoticeKind::Research, text);
        }
        let era = self.era();
        if era > before.era {
            let text = tf!("classic.game.turn.new-era", era = names::era(self.rules(), era));
            self.notify(NoticeKind::Research, text);
        }
    }

    fn report_outcome(&mut self) {
        let alive = self.game().view().auth.players().is_alive(self.me());
        match self.outcome() {
            Outcome::Victory => self.notify(NoticeKind::Diplomacy, t!("classic.game.turn.won").into()),
            Outcome::Defeat if alive => {
                self.notify(NoticeKind::Diplomacy, t!("classic.game.turn.lost-to-another").into())
            }
            Outcome::Defeat => self.notify(NoticeKind::Combat, t!("classic.game.turn.fell").into()),
            Outcome::Playing => {}
        }
    }

    /// A selected city stays selected; a unit selection moves to the next unit needing orders.
    fn keep_or_advance_selection(&mut self) {
        if let Selection::City(city) = self.selection() {
            let owner = self.game().view().auth.cities().get(city).map(|c| c.owner);
            if owner == Some(self.me()) {
                return;
            }
        }
        self.select_next_idle_unit();
    }
}
