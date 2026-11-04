//! Promotions priced by their effect (`Variant::PromoFit`, `FightFix::promo`,
//! classic/docs/ai-combat-fix.md «Promotions»).
//!
//! The game's AI takes the first promotion in content order at the end of
//! the turn, and the instant heal only below half health
//! (`combat::promotion_choices`). Here every promotion the unit may take is
//! priced by what it changes in the fights the unit has in view, read from
//! the rules' own previews with the promotion applied
//! (`PlayerQueries::promoted_attack_preview`, `promoted_defense_preview`):
//! no content is named, every effect (a bonus against cities, against a kind,
//! on a terrain, a longer range, more health) counts only through the
//! numbers the rules give for this unit, this target, this ground.
//!
//! * A strike: the unit's best strike on a target at war it can reach this
//!   turn (the moment of its decision) or next turn (the end of the turn),
//!   valued as `combat::attack_score` (percent of the defender's health
//!   taken less 0.6 of the percent of its own lost, +100 for a sure kill),
//!   plus 100 for every target the expected roll takes or kills that it
//!   would not take without the promotion.
//! * A threat: the expected damage of every foe at war that can strike the
//!   unit on its tile next turn, all of them striking; the promotion is
//!   worth the health it saves, plus 100 when the unit survives them with
//!   it and not without.
//!
//! The instant heal is priced the same way, by the health it gives the
//! unit in those fights: it wins when the healed unit takes a city or kills
//! a unit it does not take without it, or survives the foes in reach. Each
//! unit takes the option priced highest above nothing, at its decision
//! before it moves and strikes (its strikes this turn) and at the end of the
//! turn (its strikes next turn); a tie keeps the lasting promotion.
//!
//! With no fight in view, or none worth anything, the end of the turn takes
//! the heal when none of it is lost (the unit misses at least its health),
//! else the first lasting promotion in content order. Keeping the experience
//! in reserve instead (the rules keep it: a level needs total experience,
//! `experience::experience_needed`, nothing is spent) was measured and lost
//! points (classic/docs/ai-combat-fix.md «Promotions»).
//!
//! `Promotions::HealFirst` keeps the game's AI's choices and adds only the
//! heal before the unit moves and strikes when it decides a fight.
//!
//! `PROMO_TRACE` prints every promotion choice and, for every variant, each
//! unit decision where the heal would decide a fight (the bench's counts).

use crate::ai::combat::{attack_score, Promotions};
use crate::ai::planner::{Decision, UnitView};
use crate::ai::trace::{Actor, Reason};
use crate::command::Command;
use crate::ids::{PlayerId, PromotionId, TileId, UnitHandle};
use crate::knowledge::observation::{AIObservation, ObservedCity, ObservedUnit, OwnUnit};
use crate::knowledge::public::PlayerQueries;
use crate::knowledge::view::PlayerView;
use crate::math::MapDimensions;
use crate::rules::CompiledRuleset;

/// `PROMO_TRACE` is set: the choices and the deciding heals go to stderr.
pub fn tracing() -> bool {
    static TRACING: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *TRACING.get_or_init(|| std::env::var_os("PROMO_TRACE").is_some())
}

/// Value of a target the promotion makes the expected roll take or kill
/// (and of the unit's survival it decides): `attack_score`'s sure kill.
const DECIDES: i32 = 100;

/// Strikes tried per target (the tiles the unit strikes it from).
const TILES_PER_TARGET: usize = 2;

/// The fights one unit has in view: its strikes (from, target) and the
/// foes that can strike it where it stands (foe, from).
struct Scene<'v, 'a> {
    view: &'v PlayerView<'a>,
    unit: OwnUnit,
    strikes: Vec<(TileId, TileId)>,
    threats: Vec<(UnitHandle, TileId)>,
}

/// What one option (a promotion, or none) does in the scene.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Effect {
    /// The best strike's `attack_score` (0 without a strike).
    strike: i32,
    /// Targets the expected roll takes or kills.
    wins: u32,
    /// Expected damage of every threat striking.
    taken: i32,
    /// The unit's health after the option (the heal adds to it).
    hp: i32,
}

impl Effect {
    fn survives(&self) -> bool {
        self.taken < self.hp
    }
}

impl Scene<'_, '_> {
    fn effect(&self, option: Option<PromotionId>) -> Effect {
        let mut out = Effect { hp: i32::from(self.unit.hp), ..Effect::default() };
        if let Some(p) = option {
            let rules = self.view.rules();
            if rules.promotions.get(p.0 as usize).is_some_and(|d| d.heals_instantly()) {
                out.hp = (out.hp + i32::from(crate::combat::experience::INSTANT_HEAL_HIT_POINTS)).min(i32::from(crate::world::unit::UNIT_MAX_HP));
            }
        }
        let mut won: Vec<TileId> = Vec::new();
        for &(from, target) in &self.strikes {
            let Some(p) = self.view.promoted_attack_preview(self.unit.handle, option, from, target) else { continue };
            out.strike = out.strike.max(attack_score(&p));
            if p.to_defender.1 >= p.defender_hp && p.to_attacker.1 < p.attacker_hp && !won.contains(&target) {
                won.push(target);
            }
        }
        out.wins = won.len() as u32;
        for &(foe, from) in &self.threats {
            if let Some(p) = self.view.promoted_defense_preview(self.unit.handle, option, foe, from) {
                out.taken += i32::from(p.to_defender.1);
            }
        }
        out
    }

    /// The price of `with` against `base` on the scale of `attack_score`.
    fn price(base: &Effect, with: &Effect) -> i32 {
        let strike = with.strike - base.strike + DECIDES * (with.wins as i32 - base.wins as i32);
        let guard = (base.taken - with.taken) + if with.survives() && !base.survives() { DECIDES } else { 0 };
        strike + if base.taken > 0 { guard } else { 0 }
    }
}

/// The choice for one unit: Some(promotion) to take now, None to wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Choice {
    pick: Option<PromotionId>,
    /// The heal would take or kill a target this turn (unit moment) or
    /// save the unit from the foes in reach (end of the turn); traced.
    heal_decides: bool,
    value: i32,
}

fn choose(scene: &Scene<'_, '_>, available: &[PromotionId], now: bool) -> Choice {
    let rules = scene.view.rules();
    let heals = |p: &PromotionId| rules.promotions.get(p.0 as usize).is_some_and(|d| d.heals_instantly());
    let base = scene.effect(None);
    let mut best: Option<(i32, PromotionId)> = None;
    let mut heal_decides = false;
    // Lasting promotions first, the heal last: a tie keeps the lasting one
    // (the first in content order, deterministic).
    let order = available.iter().filter(|p| !heals(p)).chain(available.iter().filter(|p| heals(p)));
    for p in order {
        let with = scene.effect(Some(*p));
        if heals(p) {
            heal_decides = if now { with.wins > base.wins } else { with.survives() && !base.survives() };
        }
        let value = Scene::price(&base, &with);
        if best.is_none_or(|(v, _)| value > v) {
            best = Some((value, *p));
        }
    }
    match best {
        Some((value, p)) if value > 0 => Choice { pick: Some(p), heal_decides, value },
        _ => Choice { pick: None, heal_decides, value: best.map_or(0, |(v, _)| v) },
    }
}

/// The heal when none of it is lost (the unit misses at least the health it
/// gives): the turns of rest it saves are worth it whatever comes.
fn whole_heal(rules: &CompiledRuleset, unit: &OwnUnit, available: &[PromotionId]) -> Option<PromotionId> {
    let lost = crate::world::unit::UNIT_MAX_HP.saturating_sub(unit.hp);
    let heals = |p: &&PromotionId| rules.promotions.get(p.0 as usize).is_some_and(|d| d.heals_instantly());
    available.iter().find(heals).copied().filter(|_| lost >= crate::combat::experience::INSTANT_HEAL_HIT_POINTS)
}

/// The first lasting promotion in content order (the game's AI's choice),
/// when no fight prices one.
fn first_lasting(rules: &CompiledRuleset, available: &[PromotionId]) -> Option<PromotionId> {
    available.iter().copied().find(|p| rules.promotions.get(p.0 as usize).is_some_and(|d| !d.heals_instantly()))
}

fn distance(dims: MapDimensions, a: TileId, b: TileId) -> i32 {
    dims.distance(a, b).unwrap_or(i32::MAX)
}

/// The unit's strike reach: its range, 1 for a melee unit.
fn reach(rules: &CompiledRuleset, unit: &OwnUnit) -> i32 {
    rules.units.get(unit.kind.0 as usize).map_or(1, |d| if d.ranged_strength > 0 { i32::from(d.range).max(1) } else { 1 })
}

/// Tiles the unit may stand on (its domain's ground).
fn ground_fits(view: &PlayerView<'_>, unit: &OwnUnit, tile: TileId) -> bool {
    let rules = view.rules();
    let kinds = view.terrain_kinds();
    let Some(water) = view.tile(tile).snapshot().map(|s| kinds.is_water(s.terrain.0)) else { return false };
    let sea = rules.units.get(unit.kind.0 as usize).is_some_and(|d| d.domain == crate::rules::content::Domain::Sea);
    water == sea
}

#[allow(clippy::too_many_arguments)]
fn scene<'v, 'a>(
    view: &'v PlayerView<'a>,
    dims: MapDimensions,
    unit: &OwnUnit,
    foes: &[ObservedUnit],
    cities: &[ObservedCity],
    at_war: &dyn Fn(PlayerId) -> bool,
    now: bool,
) -> Scene<'v, 'a> {
    let rules = view.rules();
    let r = reach(rules, unit);
    let move_tiles = i32::from(unit.moves_max / crate::world::unit::MOVE_DENOMINATOR).max(1);
    // Where the unit may strike from: this turn the tiles it reaches with
    // moves left, next turn the ground within its moves.
    let stands: Vec<TileId> = if now {
        let mut t: Vec<TileId> = view.own_reachable(unit.handle).into_iter().filter(|(_, left)| *left > 0).map(|(t, _)| t).collect();
        if unit.moves_left > 0 && !t.contains(&unit.tile) {
            t.push(unit.tile);
        }
        t
    } else {
        crate::math::hex_disc(dims, unit.tile, move_tiles).filter(|t| *t == unit.tile || ground_fits(view, unit, *t)).collect()
    };
    let foes: Vec<&ObservedUnit> = foes.iter().filter(|f| at_war(f.owner) && !rules.is_civilian(f.kind)).collect();
    let cities: Vec<&ObservedCity> = cities.iter().filter(|c| at_war(c.owner)).collect();
    let mut targets: Vec<TileId> = foes.iter().map(|f| f.tile).chain(cities.iter().map(|c| c.tile)).collect();
    targets.sort_by_key(|t| t.0);
    targets.dedup();
    let mut strikes = Vec::new();
    if !rules.is_civilian(unit.kind) {
        for target in targets {
            if distance(dims, unit.tile, target) > move_tiles + r {
                continue;
            }
            let mut from: Vec<TileId> = stands.iter().copied().filter(|t| (1..=r).contains(&distance(dims, *t, target))).collect();
            from.sort_by_key(|t| (*t != unit.tile, distance(dims, unit.tile, *t), t.0));
            strikes.extend(from.into_iter().take(TILES_PER_TARGET).map(|f| (f, target)));
        }
    }
    let mut threats = Vec::new();
    for foe in foes {
        let def = rules.units.get(foe.kind.0 as usize);
        let fr = def.map_or(1, |d| if d.ranged_strength > 0 { i32::from(d.range).max(1) } else { 1 });
        let moves = def.map_or(2, |d| i32::from(d.movement));
        let d = distance(dims, foe.tile, unit.tile);
        if d > moves + fr {
            continue;
        }
        // It strikes from where it stands when in reach, else from the
        // tile next to the unit nearest to it.
        let from = if d <= fr {
            Some(foe.tile)
        } else {
            let mut next: Vec<TileId> = dims.neighbors(unit.tile).into_iter().collect();
            next.sort_by_key(|t| (distance(dims, *t, foe.tile), t.0));
            next.into_iter().find(|t| view.promoted_defense_preview(unit.handle, None, foe.handle, *t).is_some())
        };
        if let Some(from) = from {
            threats.push((foe.handle, from));
        }
    }
    Scene { view, unit: *unit, strikes, threats }
}

fn name(rules: &CompiledRuleset, p: Option<PromotionId>) -> String {
    p.and_then(|p| rules.promotions.get(p.0 as usize)).map_or_else(|| "-".to_string(), |d| d.key.trim_start_matches("core:promotion/").to_string())
}

fn kind_name(rules: &CompiledRuleset, unit: &OwnUnit) -> String {
    rules.units.get(unit.kind.0 as usize).map_or_else(|| "?".to_string(), |d| d.key.trim_start_matches("core:unit/").to_string())
}

/// The unit's promotion at its decision (`ClassicMind::decide_unit`), before
/// it moves and strikes: under `Priced` the option its strikes this turn
/// price highest; under `HealFirst` the instant heal when it takes or kills
/// a target this turn. Under `Ordered`
/// (the other variants) nothing is chosen; `PROMO_TRACE` still prints the
/// decisions where the heal would decide a fight.
pub fn before_unit(view: &UnitView<'_, '_>, rule: Promotions) -> Option<Decision> {
    let fit = rule != Promotions::Ordered;
    if !fit && !tracing() {
        return None;
    }
    let local = view.local;
    let unit = &local.unit;
    let rules = local.view.rules();
    if unit.moves_left == 0 || rules.is_civilian(unit.kind) {
        return None;
    }
    let available = local.view.promotions_available(unit.handle);
    if available.is_empty() {
        return None;
    }
    let heals = available.iter().any(|p| rules.promotions.get(p.0 as usize).is_some_and(|d| d.heals_instantly()));
    if rule != Promotions::Priced && !heals {
        return None;
    }
    let at_war = |p: PlayerId| local.relation(p).is_some_and(|r| r.at_war);
    let scene = scene(&local.view, view.brief.dims, unit, &local.foreign_units, &local.foreign_cities, &at_war, true);
    if scene.strikes.is_empty() {
        return None;
    }
    let choice = choose(&scene, &available, true);
    if tracing() && (fit || choice.heal_decides) {
        eprintln!(
            "promo-now p{} r{} u{} kind={} hp={} lvl={} heal_decides={} pick={} value={} fit={}",
            local.player.0, local.round, unit.id.0, kind_name(rules, unit), unit.hp, unit.level,
            u8::from(choice.heal_decides), name(rules, choice.pick), choice.value, rule as u8
        );
    }
    let promotion = match rule {
        Promotions::Ordered => return None,
        Promotions::HealFirst => available.iter().copied().find(|p| rules.promotions.get(p.0 as usize).is_some_and(|d| d.heals_instantly())).filter(|_| choice.heal_decides)?,
        Promotions::Priced => choice.pick?,
    };
    Some(Decision::act(Actor::Unit(unit.id), Reason::SystemAdvisor, Command::Promote { unit: unit.handle, promotion }))
}

/// The end of the turn's promotions of `PromoFit`: each unit that may be
/// promoted takes the option the fights in view (its strikes next turn, the
/// foes that can strike it) price highest above nothing; else the heal when
/// none of it is lost (`whole_heal`); else the first lasting promotion.
pub fn end_of_turn(obs: &AIObservation<'_>, rules: &CompiledRuleset, dims: MapDimensions) -> Vec<Command> {
    let at_war = |p: PlayerId| obs.relation(p).is_some_and(|r| r.at_war);
    let mut out = Vec::new();
    for unit in &obs.own_units {
        let available = obs.view.promotions_available(unit.handle);
        if available.is_empty() || rules.is_civilian(unit.kind) {
            continue;
        }
        let scene = scene(&obs.view, dims, unit, &obs.visible_foreign_units, &obs.visible_foreign_cities, &at_war, false);
        let mut choice = if scene.strikes.is_empty() && scene.threats.is_empty() {
            Choice { pick: None, heal_decides: false, value: 0 }
        } else {
            choose(&scene, &available, false)
        };
        if choice.pick.is_none() {
            choice.pick = whole_heal(rules, unit, &available).or_else(|| first_lasting(rules, &available));
        }
        if tracing() {
            eprintln!(
                "promo-eot p{} r{} u{} kind={} hp={} lvl={} avail={} strikes={} threats={} heal_decides={} pick={} value={} fit=1",
                obs.player.0, obs.round, unit.id.0, kind_name(rules, unit), unit.hp, unit.level, available.len(),
                scene.strikes.len(), scene.threats.len(), u8::from(choice.heal_decides), name(rules, choice.pick), choice.value
            );
        }
        if let Some(promotion) = choice.pick {
            out.push(Command::Promote { unit: unit.handle, promotion });
        }
    }
    out
}

/// `PROMO_TRACE` of the game's AI's end-of-turn choices
/// (`combat::promotion_choices`): what it takes, on which unit, when.
pub fn trace_choices(obs: &AIObservation<'_>, rules: &CompiledRuleset, commands: &[Command]) {
    if !tracing() {
        return;
    }
    for command in commands {
        let Command::Promote { unit, promotion } = *command else { continue };
        let Some(own) = obs.own_units.iter().find(|u| u.handle == unit) else { continue };
        let available = obs.view.promotions_available(unit);
        eprintln!(
            "promo-eot p{} r{} u{} kind={} hp={} lvl={} avail={} pick={} fit=0",
            obs.player.0, obs.round, own.id.0, kind_name(rules, own), own.hp, own.level, available.len(), name(rules, Some(promotion))
        );
    }
}
