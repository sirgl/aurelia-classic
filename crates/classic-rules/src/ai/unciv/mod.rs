// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Derived from Unciv (https://github.com/yairm210/Unciv), tag 4.22.10, MPL-2.0:
// core/src/com/unciv/logic/automation/ (its AI). Each file of this module names the Unciv
// files it ports; classic/docs/ai-unciv.md maps them and lists what is not ported.

//! The AI of Unciv 4.22.10 ported to the classic rules: a separate AI mind,
//! `unciv`, and the hybrid `unciv-military` (Unciv's unit automation for the
//! military units, everything else `standard`). It plays from the player's own
//! knowledge (the observation, fog of war kept): where Unciv reads the truth
//! (an enemy's every city and its health) it reads what the player saw.
//!
//! Unciv decides a unit's whole turn in one call; the classic driver asks for
//! one order at a time and asks the unit again after each. The port runs
//! Unciv's chain from the top at every ask on the fresh observation, keeps
//! the second half of a move-and-attack for the next ask (`Memo::pending`) and
//! skips an order that would repeat one that changed nothing (`Memo::tried`).
//! Its units move in Unciv's priority order (`AiMind::order_units`).
//!
//! The hybrids `std-unciv-*` (`UncivPlay::Parts`, `parts.rs`) put one decision
//! family of Unciv's AI (research, policies, settling, production, workers,
//! gold, religion) inside `standard`, which plays the rest unchanged.

pub mod air;
pub mod battle_helper;
pub mod civ;
pub mod civilian;
pub mod construction;
pub mod diplomacy;
pub mod espionage;
pub mod gold;
pub mod head_towards;
pub mod next_turn;
pub mod parts;
pub mod personality;
pub mod religion;
pub mod sight;
pub mod unit_automation;
pub mod worker;

use std::sync::Mutex;

use crate::ai::planner::{Decision, PlanTools, UnitView};
use crate::ai::state::AiState;
use crate::ids::{EntityId, PlayerId, TileId, UnitHandle};
use crate::knowledge::observation::{AIObservation, OwnUnit};
use crate::rules::CompiledRuleset;
use crate::knowledge::public::PlayerQueries;

use sight::{Kind, TurnSight, Unit};

/// How much of the turn Unciv's AI plays (`Variant::unciv`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum UncivPlay {
    /// Not at all: the classic mind's own AI.
    #[default]
    Off,
    /// Unciv's automation of the military units (land and sea; the air arm and
    /// the nuclear weapons stay `standard`'s), its city strikes and promotions;
    /// everything else `standard` (`unciv-military`).
    Military,
    /// Unciv's AI plays the whole turn (`unciv`): every unit, the cities'
    /// production, research, policies, religion, diplomacy and gold; the Brave
    /// New World mechanics Unciv's AI does not know stay `standard`'s
    /// (classic/docs/ai-unciv.md, "To be completed").
    Full,
    /// `standard` with some of Unciv's decision families in place of its own
    /// (`std-unciv-*`, `parts.rs`): its units' order, sight, city strikes and
    /// promotions stay `standard`'s.
    Parts(UncivParts),
}

impl UncivPlay {
    /// Unciv plays the turn or its military units (`unciv`, `unciv-military`):
    /// its units' order, the wider sight, the city strikes and promotions.
    pub fn on(self) -> bool {
        matches!(self, UncivPlay::Military | UncivPlay::Full)
    }

    /// The mind carries any part of Unciv's AI (`on`, or some of its families).
    pub fn any(self) -> bool {
        self.on() || self.parts().any()
    }

    /// The decision families Unciv plays inside `standard` (none but for `Parts`).
    pub fn parts(self) -> UncivParts {
        match self {
            UncivPlay::Parts(parts) => parts,
            _ => UncivParts::NONE,
        }
    }
}

pub use parts::UncivParts;

/// The local observation's radius of an Unciv decision: Unciv's threat queries
/// look 4-6 tiles around the tiles a unit reaches.
pub const UNCIV_RADIUS: i32 = 9;

/// Candidate tiles a search tries before it gives up (each may cost a path
/// search; Unciv walks lazy sequences with the same effect).
pub const CANDIDATE_LIMIT: usize = 24;

/// One step of Unciv's chain.
pub enum Step {
    /// An order (the unit is asked again after it).
    Act(Decision),
    /// The unit's turn ends here.
    Done(crate::ai::trace::Reason),
    /// This step does not apply: the next one.
    Next,
}

/// What one unit's decisions of the turn remember.
#[derive(Debug, Clone)]
pub struct Memo {
    pub unit: EntityId,
    /// The blow after a walk (`Battle.moveAndAttack`): from, target.
    pub pending: Option<(TileId, TileId)>,
    /// A pillage after a walk: the tile and the round.
    pub pillage: Option<(TileId, u32)>,
    /// The orders sent this turn with the tile and moves they were sent from.
    pub tried: Vec<(TileId, u16, String)>,
    pub conquer_done: bool,
    pub escort_done: bool,
}

impl Memo {
    pub fn new(unit: EntityId) -> Self {
        Self { unit, pending: None, pillage: None, tried: Vec::new(), conquer_done: false, escort_done: false }
    }

    /// An order that repeats one sent from the same tile with the same moves
    /// changed nothing: the chain goes on (`Step::Next`).
    pub fn guard(&mut self, u: &Unit<'_, '_>, step: Step) -> Step {
        let Step::Act(decision) = step else {
            return step;
        };
        let Some(command) = &decision.command else {
            return Step::Done(decision.reason);
        };
        let key = (u.tile(), u.me.moves_left, format!("{command:?}"));
        if self.tried.contains(&key) {
            return Step::Next;
        }
        self.tried.push(key);
        Step::Act(decision)
    }
}

/// The per-player state of the turn.
#[derive(Debug)]
struct Turn {
    player: PlayerId,
    sight: std::sync::Arc<TurnSight>,
    memos: Vec<Memo>,
}

/// Unciv's AI as a part of the classic mind: its turn's sight and its units'
/// memos (interior: the mind decides through `&self`).
#[derive(Debug, Default)]
pub struct UncivMind {
    play: UncivPlay,
    turns: Mutex<Vec<Turn>>,
    /// What each player's diplomacy keeps between its turns (not saved: a
    /// game reloaded forgets the forces seen and the offers made).
    civs: Mutex<Vec<CivMemory>>,
    /// Dev-only (`parts::DRY_ENV`): the families inside `standard` decide but
    /// their decisions are dropped and `standard` plays everything.
    dry: bool,
}

/// A player's diplomatic memory between turns.
#[derive(Debug, Clone)]
struct CivMemory {
    player: PlayerId,
    /// The force of each other player's units seen (the most at once, fading 2 % a turn).
    forces: Vec<(PlayerId, f32)>,
    /// Our last offers: to whom, which kind (`diplomacy::offer_kind`), the round.
    offers: Vec<(PlayerId, u8, u32)>,
}

impl UncivMind {
    pub fn new(play: UncivPlay) -> Self {
        let dry = play.parts().any() && std::env::var(parts::DRY_ENV).is_ok_and(|v| !v.is_empty() && v != "0");
        Self { play, turns: Mutex::new(Vec::new()), civs: Mutex::new(Vec::new()), dry }
    }

    pub fn full(&self) -> bool {
        self.play == UncivPlay::Full
    }

    /// The opening of a turn of `unciv` (`NextTurnAutomation.automateCivMoves`
    /// before the units): trades and diplomacy, faith, policies, research, city
    /// strikes, gold, city-states, promotions, beliefs, spies, the free great
    /// person; and the Brave New World mechanics' `standard` parts (trade routes,
    /// the World Congress).
    pub fn full_opening(&self, obs: &AIObservation<'_>, rules: &CompiledRuleset, state: &mut AiState, brief: &crate::ai::planner::TurnBrief) -> Vec<Decision> {
        let personality = personality::UncivPersonality::of(rules, obs.economy.nation);
        let mut out = Vec::new();
        let major = obs.view.own_kind() == crate::world::player::PlayerKind::Major;
        if major {
            out.extend(self.diplomacy(obs, rules, state, &personality));
            out.extend(religion::spend_faith(obs, rules));
            out.extend(civ::adopt_policy(obs, rules, state, &personality));
        }
        out.extend(civ::research(obs, rules));
        out.extend(next_turn::city_bombardment(obs, rules));
        out.extend(gold::use_gold(obs, rules, &personality));
        if major {
            out.extend(civ::city_states(obs, &personality));
        }
        out.extend(next_turn::promotions(obs, rules, |_| true));
        if major {
            out.extend(religion::choose_beliefs(obs, rules));
            out.extend(espionage::automate_spies(obs, rules));
            out.extend(civ::choose_great_person(obs));
        }
        out.extend(civ::conquest_decisions(obs, state));
        // Brave New World, `standard`'s: trade routes and the World Congress.
        out.extend(crate::ai::trade::route_commands(obs).into_iter().map(|c| Decision::act(crate::ai::trace::Actor::Player, crate::ai::trace::Reason::SystemAdvisor, c)));
        out.extend(fallback(obs, rules, state, brief, crate::ai::system::Moment::StartOfTurn));
        out
    }

    /// `unciv`'s cities at the end of its turn (`automateCities`, `trainSettler`)
    /// and the Brave New World fallbacks (great works, archaeology, ideology
    /// switches, trade units).
    pub fn full_end_of_turn(&self, obs: &AIObservation<'_>, rules: &CompiledRuleset, state: &AiState, brief: &crate::ai::planner::TurnBrief) -> Vec<Decision> {
        let personality = personality::UncivPersonality::of(rules, obs.economy.nation);
        let afraid = afraid_of_barbarians(obs);
        let empire = construction::Empire::new(obs, rules, personality, afraid);
        let own_force = diplomacy::own_force(obs, rules);
        let enemies = self.enemy_force(obs);
        let mut out = construction::automate_cities(&empire, enemies, own_force);
        out.extend(construction::train_settler(&empire));
        out.extend(civ::conquest_decisions(obs, state));
        out.extend(crate::ai::trade::production_commands(obs, rules).into_iter().map(|c| Decision::act(crate::ai::trace::Actor::Player, crate::ai::trace::Reason::ProduceTradeUnit, c)));
        out.extend(fallback(obs, rules, state, brief, crate::ai::system::Moment::EndOfTurn));
        out
    }

    /// The diplomacy of the turn with the player's memory (forces seen, offers
    /// made, war plans).
    fn diplomacy(&self, obs: &AIObservation<'_>, rules: &CompiledRuleset, state: &mut AiState, personality: &personality::UncivPersonality) -> Vec<Decision> {
        let (forces, offers) = self.note_forces(obs, rules);
        let memory = Memory::read(state).unwrap_or_default();
        let race = obs.view.race();
        let d = diplomacy::Diplomat {
            obs,
            rules,
            state,
            personality: *personality,
            own_force: diplomacy::own_force(obs, rules),
            forces,
            scores: race.scores.clone(),
            plans: memory.plans.clone(),
        };
        let last = |p: PlayerId, kind: u8| offers.iter().filter(|(q, k, _)| *q == p && *k == kind).map(|(_, _, r)| *r).max();
        let turn = diplomacy::turn(&d, &last);
        // Remember the offers made and the plans begun or ended.
        {
            let mut civs = self.civs.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(memory) = civs.iter_mut().find(|c| c.player == obs.player) {
                for decision in &turn.decisions {
                    if let Some(crate::command::Command::ProposeDeal { to, items, .. }) = &decision.command {
                        if let Some(kind) = diplomacy::offer_kind(items) {
                            memory.offers.retain(|(p, k, _)| !(p == to && *k == kind));
                            memory.offers.push((*to, kind, obs.round));
                        }
                    }
                }
            }
        }
        let mut memory = memory;
        let before = memory.plans.clone();
        memory.plans.retain(|(p, _)| !turn.end_plans.contains(p));
        if let Some(p) = turn.begin_plan {
            if !memory.plans.iter().any(|(q, _)| *q == p) {
                memory.plans.push((p, obs.round));
            }
        }
        if memory.plans != before && Memory::read(state).is_some() {
            memory.write(state);
        }
        turn.decisions
    }

    /// The forces of the other players' units seen this turn kept in the
    /// player's memory (the most at once, fading 2 % a turn): the forces and
    /// our last offers.
    fn note_forces(&self, obs: &AIObservation<'_>, rules: &CompiledRuleset) -> (Vec<(PlayerId, f32)>, Vec<(PlayerId, u8, u32)>) {
        let seen = diplomacy::seen_forces(obs, rules);
        let mut civs = self.civs.lock().unwrap_or_else(|e| e.into_inner());
        let at = match civs.iter().position(|c| c.player == obs.player) {
            Some(at) => at,
            None => {
                civs.push(CivMemory { player: obs.player, forces: Vec::new(), offers: Vec::new() });
                civs.len() - 1
            }
        };
        let memory = &mut civs[at];
        for f in &mut memory.forces {
            f.1 *= 0.98;
        }
        for (p, force) in &seen {
            match memory.forces.iter_mut().find(|(q, _)| q == p) {
                Some(entry) => entry.1 = entry.1.max(*force),
                None => memory.forces.push((*p, *force)),
            }
        }
        (memory.forces.clone(), memory.offers.clone())
    }

    /// The force of the nations at war with the player, as its memory keeps them.
    fn enemy_force(&self, obs: &AIObservation<'_>) -> f32 {
        let civs = self.civs.lock().unwrap_or_else(|e| e.into_inner());
        let memory = civs.iter().find(|c| c.player == obs.player);
        obs.relations
            .iter()
            .filter(|r| r.at_war && r.kind != crate::world::player::PlayerKind::Module(crate::world::player::ClassicKind::Barbarian))
            .map(|r| memory.and_then(|m| m.forces.iter().find(|(p, _)| *p == r.other)).map_or(0.0, |(_, f)| *f))
            .sum()
    }

    /// A unit's decision in `unciv`: Unciv's automation for the units it knows;
    /// None for the Brave New World units (`standard` decides them).
    pub fn decide_full(&self, view: &UnitView<'_, '_>, state: &AiState, tools: &mut PlanTools<'_>) -> Option<Decision> {
        let rules = tools.rules;
        let sight = self.sight_of(view.local.player)?;
        let id = view.local.unit.id;
        let kind = Kind { rules, kind: view.local.unit.kind };
        let def = rules.units.get(view.local.unit.kind.0 as usize)?;
        let mut memo = {
            let mut turns = self.lock();
            let turn = turns.iter_mut().find(|t| t.player == view.local.player)?;
            match turn.memos.iter().position(|m| m.unit == id) {
                Some(at) => turn.memos.swap_remove(at),
                None => Memo::new(id),
            }
        };
        let unit = Unit::new(view, &sight);
        let bnw_great_person = civilian::great_person_kind(&unit).is_some_and(|k| matches!(k, crate::classic::effects::GreatPersonKind::Writer | crate::classic::effects::GreatPersonKind::Artist | crate::classic::effects::GreatPersonKind::Musician));
        let decision = if kind.is_air() || kind.is_nuclear() && !kind.is_land() {
            Some(air::automate_air(&unit, &mut memo, tools))
        } else if kind.is_military() {
            Some(unit_automation::automate_military(&unit, state, &mut memo, tools))
        } else if def.builder {
            Some(worker::automate_worker(&unit, tools))
        } else if let Some(d) = religion::automate_religious_unit(&unit, tools) {
            Some(d)
        } else if bnw_great_person {
            None
        } else {
            civilian::automate_civilian(&unit, &mut memo, tools)
        };
        if let Some(turn) = self.lock().iter_mut().find(|t| t.player == view.local.player) {
            turn.memos.push(memo);
        }
        decision
    }

    pub fn play(&self) -> UncivPlay {
        self.play
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Turn>> {
        self.turns.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The start of `obs.player`'s turn: the sight of the turn, fresh memos,
    /// the cities it founded remembered.
    pub fn begin_turn(&self, obs: &AIObservation<'_>, state: &mut AiState) {
        remember_founded(obs, state);
        let sight = std::sync::Arc::new(TurnSight::take(obs, state));
        let mut turns = self.lock();
        turns.retain(|t| t.player != obs.player);
        turns.push(Turn { player: obs.player, sight, memos: Vec::new() });
    }

    fn sight_of(&self, player: PlayerId) -> Option<std::sync::Arc<TurnSight>> {
        self.lock().iter().find(|t| t.player == player).map(|t| t.sight.clone())
    }

    /// Does Unciv decide this unit (`unciv-military`: a land or sea military
    /// unit, no nuclear weapon)?
    pub fn commands(&self, rules: &CompiledRuleset, unit: &OwnUnit) -> bool {
        let kind = Kind { rules, kind: unit.kind };
        self.play.on() && kind.is_military() && !kind.is_air() && !kind.is_nuclear()
    }

    /// The unit's decision by Unciv's automation; None when Unciv does not
    /// decide it.
    pub fn decide(&self, view: &UnitView<'_, '_>, state: &AiState, tools: &mut PlanTools<'_>) -> Option<Decision> {
        let rules = tools.rules;
        if !self.commands(rules, &view.local.unit) {
            return None;
        }
        let sight = self.sight_of(view.local.player)?;
        let id = view.local.unit.id;
        let mut memo = {
            let mut turns = self.lock();
            let turn = turns.iter_mut().find(|t| t.player == view.local.player)?;
            match turn.memos.iter().position(|m| m.unit == id) {
                Some(at) => turn.memos.swap_remove(at),
                None => Memo::new(id),
            }
        };
        let unit = Unit::new(view, &sight);
        let decision = unit_automation::automate_military(&unit, state, &mut memo, tools);
        if let Some(turn) = self.lock().iter_mut().find(|t| t.player == view.local.player) {
            turn.memos.push(memo);
        }
        Some(decision)
    }

    /// Unciv's order of the units (`getUnitPriority`), from the turn's sight.
    pub fn order_units(&self, rules: &CompiledRuleset, player: PlayerId, order: &mut [(EntityId, UnitHandle)]) {
        if !self.play.on() {
            return;
        }
        let Some(sight) = self.sight_of(player) else {
            return;
        };
        let at_war = sight.at_war.iter().any(|p| sight.foreign_kind(*p) != Some(crate::world::player::PlayerKind::Module(crate::world::player::ClassicKind::Barbarian)));
        next_turn::order_units(rules, &sight, at_war, order);
    }

    /// The opening of the turn Unciv adds: its promotions and its cities' strikes.
    pub fn opening(&self, obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Vec<Decision> {
        if !self.play.on() {
            return Vec::new();
        }
        let mut out = next_turn::promotions(obs, rules, |u| self.commands(rules, u));
        out.extend(next_turn::city_bombardment(obs, rules));
        out
    }

    /// A decision of the classic mind that Unciv's part replaces: an order of a
    /// unit Unciv decides, a city strike, a promotion of such a unit.
    pub fn replaces(&self, rules: &CompiledRuleset, own: &[OwnUnit], decision: &Decision) -> bool {
        if !self.play.on() {
            return false;
        }
        let Some(command) = &decision.command else {
            return false;
        };
        if matches!(command, crate::command::Command::CityStrike { .. }) {
            return true;
        }
        if matches!(command, crate::command::Command::UpgradeUnit { .. }) {
            return false; // Gold: `standard`'s.
        }
        let Some(unit) = crate::knowledge::actions::acting_unit(command) else {
            return false;
        };
        own.iter().find(|u| u.handle == unit).is_some_and(|u| self.commands(rules, u))
    }
}

/// The `standard` advisors of the Brave New World mechanics Unciv's AI does
/// not play: great works, archaeology and ideology switches (`ai::tourism`)
/// and the World Congress (`ai::congress`).
fn fallback(obs: &AIObservation<'_>, rules: &CompiledRuleset, state: &AiState, brief: &crate::ai::planner::TurnBrief, moment: crate::ai::system::Moment) -> Vec<Decision> {
    use crate::ai::system::{AdvisorInput, Moment, SystemAdvisor};
    let input = AdvisorInput { observation: obs, brief, state, rules };
    let mut proposals = Vec::new();
    match moment {
        Moment::StartOfTurn => {
            proposals.extend(crate::ai::tourism::TourismAdvisor.start_of_turn(&input));
            proposals.extend(crate::ai::congress::CongressAdvisor.start_of_turn(&input));
        }
        Moment::EndOfTurn => {
            proposals.extend(crate::ai::tourism::TourismAdvisor.end_of_turn(&input));
            proposals.extend(crate::ai::congress::CongressAdvisor.end_of_turn(&input));
        }
    }
    proposals.sort_by_key(|p| std::cmp::Reverse(p.priority));
    proposals.into_iter().map(|p| Decision::act(crate::ai::trace::Actor::Player, crate::ai::trace::Reason::SystemAdvisor, p.command)).collect()
}

/// `Automation.afraidOfBarbarians` from the observation (the continent's size
/// read as the land the player explored).
fn afraid_of_barbarians(obs: &AIObservation<'_>) -> bool {
    if obs.view.own_kind() != crate::world::player::PlayerKind::Major {
        return false;
    }
    if obs.round > obs.speed.game_round(200) {
        return false;
    }
    let mut multiplier = 1.0f32;
    if obs.round > obs.speed.game_round(120) {
        multiplier /= 2.0;
    }
    if obs.own_cities.is_empty() || obs.own_units.len() as f32 >= 4.0 * multiplier {
        return false;
    }
    let kinds = obs.view.terrain_kinds();
    let (mut land, mut visible) = (0u32, 0u32);
    for t in 0..obs.view.tile_count() as u32 {
        let tile = TileId(t);
        if obs.view.is_visible(tile) {
            visible += 1;
        }
        if obs.view.tile(tile).snapshot().is_some_and(|s| !kinds.is_water(s.terrain.0)) {
            land += 1;
        }
    }
    (land as f32) >= visible as f32 * multiplier
}

/// The mind's own memory in `AiState::mind_memory` (saved with the game):
/// the tiles of the cities the player has owned (Unciv's `foundingCiv`, read as
/// the cities it held) and the war plans in preparation (Unciv's negative
/// `WaryOf` flag: the target and the round the plan began). Layout: the tag,
/// u16 count and little-endian u32 tiles, u8 count and (u8 player, u32 round).
const UNCIV_MEMORY_TAG: u8 = 0xC6;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Memory {
    pub founded: Vec<TileId>,
    pub plans: Vec<(PlayerId, u32)>,
}

impl Memory {
    pub fn read(state: &AiState) -> Option<Memory> {
        let m = &state.mind_memory;
        if m.is_empty() {
            return Some(Memory::default());
        }
        if m[0] != UNCIV_MEMORY_TAG || m.len() < 3 {
            return None; // Another mind's memory (the net's): not ours to read.
        }
        let n = usize::from(u16::from_le_bytes([m[1], m[2]]));
        let mut at = 3;
        let mut founded = Vec::with_capacity(n);
        for _ in 0..n {
            let c = m.get(at..at + 4)?;
            founded.push(TileId(u32::from_le_bytes([c[0], c[1], c[2], c[3]])));
            at += 4;
        }
        let k = usize::from(*m.get(at)?);
        at += 1;
        let mut plans = Vec::with_capacity(k);
        for _ in 0..k {
            let c = m.get(at..at + 5)?;
            plans.push((PlayerId(c[0]), u32::from_le_bytes([c[1], c[2], c[3], c[4]])));
            at += 5;
        }
        Some(Memory { founded, plans })
    }

    pub fn write(&self, state: &mut AiState) {
        let mut m = vec![UNCIV_MEMORY_TAG];
        m.extend_from_slice(&(self.founded.len() as u16).to_le_bytes());
        for t in &self.founded {
            m.extend_from_slice(&t.0.to_le_bytes());
        }
        m.push(self.plans.len().min(255) as u8);
        for (p, r) in self.plans.iter().take(255) {
            m.push(p.0);
            m.extend_from_slice(&r.to_le_bytes());
        }
        state.mind_memory = m;
    }
}

pub fn founded_tiles(state: &AiState) -> Vec<TileId> {
    Memory::read(state).map(|m| m.founded).unwrap_or_default()
}

fn remember_founded(obs: &AIObservation<'_>, state: &mut AiState) {
    let Some(mut memory) = Memory::read(state) else {
        return;
    };
    let before = memory.founded.len();
    for city in &obs.own_cities {
        if !memory.founded.contains(&city.tile) {
            memory.founded.push(city.tile);
        }
    }
    if memory.founded.len() == before && !state.mind_memory.is_empty() {
        return;
    }
    memory.founded.sort();
    memory.write(state);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The mind's memory survives its bytes (the save keeps them), and another
    /// mind's memory is left alone.
    #[test]
    fn the_memory_round_trips_and_leaves_other_minds_alone() {
        let mut state = AiState::new(PlayerId(2), 0);
        assert_eq!(Memory::read(&state), Some(Memory::default()));
        let memory = Memory { founded: vec![TileId(7), TileId(300_000)], plans: vec![(PlayerId(3), 41), (PlayerId(1), 99)] };
        memory.write(&mut state);
        assert_eq!(Memory::read(&state), Some(memory));
        assert_eq!(founded_tiles(&state), vec![TileId(7), TileId(300_000)]);
        state.mind_memory = vec![1, 2, 3];
        assert_eq!(Memory::read(&state), None, "the net's memory is not ours");
        assert!(founded_tiles(&state).is_empty());
    }
}
