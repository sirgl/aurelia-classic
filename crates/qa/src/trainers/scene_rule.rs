//! The scene rule (`ARMY_SCENE_RULE=simple`; default OFF, and then nothing here runs): a small explicit rule
//! that plays what the AI `standard` plays of the scene's pieces besides the seat's own script, so that the
//! world of a trainer scene is owned by the trainer and is the same in the MLX twin
//! (`nn/aurelia_nn/tacsim/armyrule.py`). With the rule on, at the end of a seat's scripted part (the net, a
//! reference or a control on the learner's seat; the family script on the enemy's seat), for that seat:
//!
//! 1. Promotions: each of its pieces with a promotion to take takes one (`ai::combat::promotion_choices`:
//!    an instant heal below 50 HP, else the first promotion in content order that does not heal).
//! 2. City strikes: each of its cities (by handle) strikes the foe piece the strike leaves with the fewest hit
//!    points (a kill leaves 0; ties: the lowest tile).
//! 3. Shots: each of its ranged pieces with moves left (piece order) shoots from where it stands, by the same
//!    key, when the rules accept the shot (no set-up, no move first).
//! 4. Stand and fortify: each of its pieces ends its turn (`act::hold_unit`) and fortifies where the rules
//!    accept it, unless it is set up for a ranged attack.
//!
//! 0. (before the seat's script) The arena closes at the turn's start too: a unit a home city finished then is
//!    taken off before the seat looks (`trainers::close_arena`; else the net sees it as a far unit's token).
//!
//! The seat's mind then plays only the rest of the seat (its economy): `Ruled` drops every command it would
//! give a unit or a city's strike of a ruled seat, its deal proposals (no peace between the scene's seats) and its
//! purchases (no unit appears inside a turn). The learner's seat is ruled unless an AI variant plays it
//! (`Seat::Mind`: the AI under measurement plays its whole seat).
//!
//! Targets are the other seat's live pieces. Every choice is a function of the game (ties by tile and
//! content order), so the twin can play it the same way.

use std::cell::RefCell;
use std::collections::BTreeSet;

use classic_rules::combat::damage::Roll;
use classic_rules::combat::experience::{available_promotions, experience_needed_by};
use classic_rules::ids::{CityHandle, PlayerId, PromotionId, TileId, UnitHandle};
use classic_rules::{ClassicGame, Command, CompiledRuleset, GameState};
use kernel::world::unit::{Stance, UnitCore};

use super::act;
use super::Scene;

/// The rule's switch: `ARMY_SCENE_RULE=simple` (anything else, or unset: off).
pub fn on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("ARMY_SCENE_RULE").is_ok_and(|v| v == "simple"))
}

thread_local! {
    /// The seats of the play the rule plays (their minds give no unit or city-strike command).
    static RULED: RefCell<BTreeSet<PlayerId>> = const { RefCell::new(BTreeSet::new()) };
}

pub fn set_ruled(players: &[PlayerId]) {
    RULED.with(|r| *r.borrow_mut() = players.iter().copied().collect());
}

pub fn ruled(p: PlayerId) -> bool {
    RULED.with(|r| r.borrow().contains(&p))
}

/// The promotion rule (1) for a unit: None when it has none to take.
pub fn promotion_pick(game: &GameState, rules: &CompiledRuleset, c: &UnitCore) -> Option<PromotionId> {
    let auth = game.view().auth;
    if c.experience < experience_needed_by(auth, rules, c) {
        return None;
    }
    let avail = available_promotions(auth, rules, c);
    if avail.is_empty() {
        return None;
    }
    let heals = |p: &PromotionId| rules.promotions[p.0 as usize].heals_instantly();
    match avail.iter().copied().find(heals) {
        Some(p) if c.hp < 50 => Some(p),
        _ => Some(*avail.iter().find(|p| !heals(p)).unwrap_or(&avail[0])),
    }
}

/// The key of a strike: the hit points it leaves (0 a kill), then the tile.
fn key(hp: i64, dealt: i64, t: TileId) -> (i64, u32) {
    ((hp - dealt).max(0), t.0)
}

/// The city strike rule (2): the target tile among `foes` (None: nothing the city may strike).
pub fn city_target(game: &GameState, rules: &CompiledRuleset, side: PlayerId, city: CityHandle, foes: &[TileId]) -> Option<TileId> {
    city_target_by(game, rules, side, city, foes, false)
}

/// `most`: the alternative key measured beside it (the most damage, `ai::combat::city_strikes`).
pub fn city_target_by(game: &GameState, rules: &CompiledRuleset, side: PlayerId, city: CityHandle, foes: &[TileId], most: bool) -> Option<TileId> {
    let mut best: Option<((i64, u32), TileId)> = None;
    for &t in foes {
        let cmd = Command::CityStrike { city, target: t };
        if !game.accepts_order(rules, side, &cmd) {
            continue;
        }
        let Ok(plan) = classic_rules::combat::exchange::plan_city_strike(game.view(), rules, city, t) else { continue };
        let dealt = i64::from(plan.damage(Roll::Expected));
        let k = if most { (-dealt, t.0) } else { key(i64::from(plan.defender_hp), dealt, t) };
        if best.is_none_or(|b| k < b.0) {
            best = Some((k, t));
        }
    }
    best.map(|b| b.1)
}

/// The shot rule (3): the target tile among `foes` of a ranged unit from where it stands.
pub fn shot_target(game: &GameState, rules: &CompiledRuleset, side: PlayerId, unit: UnitHandle, foes: &[TileId]) -> Option<TileId> {
    let me = act::core_of(game, unit)?;
    if me.moves_left == 0 {
        return None;
    }
    act::range_of(rules, me.kind)?;
    let mut best: Option<((i64, u32), TileId)> = None;
    for &t in foes {
        if !game.accepts_order(rules, side, &Command::RangedAttack { unit, target: t }) {
            continue;
        }
        let Some((dealt, _, hp)) = act::preview(game, rules, unit, None, t) else { continue };
        let k = key(hp, dealt, t);
        if best.is_none_or(|b| k < b.0) {
            best = Some((k, t));
        }
    }
    best.map(|b| b.1)
}

/// The fortify rule (4): whether the unit (its turn over) fortifies.
pub fn fortifies(game: &GameState, rules: &CompiledRuleset, side: PlayerId, unit: UnitHandle) -> bool {
    let Some(me) = act::core_of(game, unit) else { return false };
    !matches!(me.stance, Stance::SetUp) && game.accepts_order(rules, side, &Command::Fortify { unit })
}

/// The live pieces of the seat other than `side`: their tiles (sorted, deduplicated).
pub fn foe_tiles(game: &GameState, scene: &Scene, side: PlayerId) -> Vec<TileId> {
    let foes = if side == scene.learner { scene.theirs_alive(game) } else { scene.mine_alive(game) };
    let mut v: Vec<TileId> = foes.iter().map(|(_, c)| c.tile).collect();
    v.sort_by_key(|t| t.0);
    v.dedup();
    v
}

/// The rule's part of `side`'s turn (after the seat's script, before its mind plays the rest of the seat).
pub fn turn(rules: &CompiledRuleset, game: &mut GameState, scene: &Scene, side: PlayerId) {
    let pieces = if side == scene.learner { &scene.mine } else { &scene.theirs };
    crate::army_net::obsdump::set_act_src("rule");
    // 1. Promotions.
    for p in pieces {
        let Some(c) = act::alive(game, p, side) else { continue };
        if let Some(promotion) = promotion_pick(game, rules, &c) {
            act::try_cmd(game, rules, side, Command::Promote { unit: p.h, promotion });
        }
    }
    // 2. City strikes.
    let mut cities: Vec<CityHandle> = game.view().auth.indexes().cities_of(side).to_vec();
    cities.sort_by_key(|c| c.index);
    for city in cities {
        let foes = foe_tiles(game, scene, side);
        if let Some(target) = city_target(game, rules, side, city, &foes) {
            act::try_cmd(game, rules, side, Command::CityStrike { city, target });
        }
    }
    // 3. Shots.
    for p in pieces {
        if act::alive(game, p, side).is_none() {
            continue;
        }
        let foes = foe_tiles(game, scene, side);
        if let Some(target) = shot_target(game, rules, side, p.h, &foes) {
            act::try_cmd(game, rules, side, Command::RangedAttack { unit: p.h, target });
        }
    }
    // 4. Stand and fortify.
    for p in pieces {
        let Some(c) = act::alive(game, p, side) else { continue };
        if c.moves_left > 0 {
            act::hold_unit(game, rules, p.h);
        }
        if fortifies(game, rules, side, p.h) {
            act::try_cmd(game, rules, side, Command::Fortify { unit: p.h });
        }
    }
    crate::army_net::obsdump::set_act_src("act");
}

/// A command the mind of a ruled seat may not give: one for a unit, a city's strike, a deal (a peace the two minds
/// agree on expels the units from the borders and stops the war the scene is about; measured: the turns where our
/// units jumped a row in the forced checks), or a purchase (a unit bought in the turn is a token the net sees at its
/// turn's end; the arena's close at the turn's start takes the units a city finished).
pub fn ruled_out(c: &Command) -> bool {
    matches!(c, Command::CityStrike { .. } | Command::ProposeDeal { .. } | Command::HurryProduction { .. }) || classic_rules::knowledge::actions::acting_unit(c).is_some()
}

fn keep(player: PlayerId, d: &kernel::ai::plan::Decision<classic_rules::classic::Classic>) -> bool {
    !ruled(player) || d.command.as_ref().is_none_or(|c| !ruled_out(c))
}

/// The mind of the seats with the rule on: the inner mind without the commands of the ruled seats' units and
/// cities' strikes (the rule plays them) and without their deal proposals; the rest of the seat (its economy) as
/// the inner mind plays it.
#[derive(Debug)]
pub struct Ruled<P>(pub P);

type Cl = classic_rules::classic::Classic;

impl<P: kernel::ai::mind::AiMind<Cl>> kernel::ai::mind::AiMind<Cl> for Ruled<P> {
    type Brief = P::Brief;
    type Sight<'g> = P::Sight<'g>;
    type Near<'g> = P::Near<'g>;

    fn sense<'g>(&self, game: &'g GameState, rules: &'g CompiledRuleset, player: PlayerId) -> Self::Sight<'g> {
        self.0.sense(game, rules, player)
    }
    fn sense_unit<'g>(&self, game: &'g GameState, rules: &'g CompiledRuleset, settings: &kernel::ai::plan::AiSettings, player: PlayerId, unit: UnitHandle) -> Option<Self::Near<'g>> {
        if ruled(player) {
            return None;
        }
        self.0.sense_unit(game, rules, settings, player, unit)
    }
    fn note_humans(&mut self, humans: &[PlayerId]) {
        self.0.note_humans(humans);
    }
    fn start_turn(&self, sight: &Self::Sight<'_>, rules: &CompiledRuleset, player: PlayerId, state: &mut kernel::ai::state::AiState) -> kernel::ai::mind::TurnStart<Cl, Self::Brief> {
        let mut start = self.0.start_turn(sight, rules, player, state);
        start.opening.retain(|d| keep(player, d));
        start
    }
    fn order_units(&self, brief: &Self::Brief, rules: &CompiledRuleset, order: &mut Vec<(classic_rules::ids::EntityId, UnitHandle)>) {
        self.0.order_units(brief, rules, order);
    }
    fn picks_next(&self, brief: &Self::Brief) -> bool {
        self.0.picks_next(brief)
    }
    fn next_unit(&self, near: &Self::Near<'_>, brief: &Self::Brief, state: &kernel::ai::state::AiState, remaining: &[(classic_rules::ids::EntityId, UnitHandle)]) -> Option<classic_rules::ids::EntityId> {
        self.0.next_unit(near, brief, state, remaining)
    }
    fn plan_strategic(&self, brief: &Self::Brief, state: &mut kernel::ai::state::AiState, rules: &CompiledRuleset) -> Vec<kernel::ai::plan::Decision<Cl>> {
        // No player here: a unit or strike command of a ruled seat is refused by `permits`.
        self.0.plan_strategic(brief, state, rules)
    }
    fn decide_unit(&self, near: &Self::Near<'_>, player: PlayerId, unit: UnitHandle, brief: &Self::Brief, state: &mut kernel::ai::state::AiState, tools: &mut kernel::ai::plan::PlanTools<'_, Cl>) -> Option<kernel::ai::plan::Decision<Cl>> {
        if ruled(player) {
            return None;
        }
        self.0.decide_unit(near, player, unit, brief, state, tools)
    }
    fn permits(&self, game: &GameState, rules: &CompiledRuleset, player: PlayerId, command: &Command) -> bool {
        !(ruled(player) && ruled_out(command)) && self.0.permits(game, rules, player, command)
    }
    fn plan_end_of_turn(&self, sight: &Self::Sight<'_>, player: PlayerId, brief: &Self::Brief, state: &kernel::ai::state::AiState, tools: &mut kernel::ai::plan::PlanTools<'_, Cl>) -> Vec<kernel::ai::plan::Decision<Cl>> {
        let mut out = self.0.plan_end_of_turn(sight, player, brief, state, tools);
        out.retain(|d| keep(player, d));
        out
    }
}
