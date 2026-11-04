//! Siege tactics (classic/docs/ai-siege-tactics.md): where a unit fights
//! from. Three switches a variant carries (`SiegeTactics`, the turn's
//! `TurnBrief::siege`), each read from the rules' data — the ground's
//! defence percent, the unit's range with its promotions, the city's strike
//! range — never from a unit's name:
//!
//! - `ground`: the front ranks the tiles a member strikes from, gathers on
//!   and closes in on by the battlefield's danger there (the enemy's blows by
//!   the rules' damage formulas on the ground's defence), then by the
//!   ground's defence, instead of the count of the enemies next to them.
//! - `firing`: a ranged unit shoots from the tile it reaches this turn
//!   where it ends least exposed (moving first: the rules end its turn with
//!   the shot), not from wherever the turn found it; the military advisor
//!   leaves a shot to the unit's own decision when such a tile is better than
//!   where the unit stands.
//! - `outrange`: a unit's range counts its promotions; a ranged unit whose
//!   range exceeds a city's strike range bombards the city from the tiles
//!   the city cannot strike and waits for the siege there, not on the
//!   common gathering ring.
//! - `fortify`: a military land unit whose decisions end (no command)
//!   within an enemy's reach, out of an own city, fortifies: the rules give a
//!   fortified unit a defence percent, the order takes no moves (a unit that
//!   moved or shot may give it) and does not stop healing.
//!
//! The rules let a unit move after its attack only with the ability
//! `MoveAfterAttacking` (`systems::combat::finish_attack`): "shoot, then step
//! back" is "step to the firing tile, then shoot" for everyone else.

use crate::combat::city::CITY_ATTACK_RANGE;
use crate::ids::{PlayerId, TileId, UnitKindId};
use crate::knowledge::observation::OwnUnit;
use crate::knowledge::view::PlayerView;
use crate::rules::effects::{Modifier, UnitStat};
use crate::rules::CompiledRuleset;
use crate::world::unit::PromotionSet;

/// The siege-tactics switches a variant carries (`Variant::siege_tactics`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct SiegeTactics {
    /// End tiles ranked by the danger with the ground's defence.
    pub ground: bool,
    /// A ranged unit steps to its least exposed firing tile, then shoots.
    pub firing: bool,
    /// The range with promotions; an outranging unit bombards from beyond
    /// the city's strike.
    pub outrange: bool,
    /// A unit that ends its decisions within an enemy's reach fortifies
    /// (the rules' fortification: a percent of defence a turn, `fortify`
    /// takes no moves and stops no healing).
    pub fortify: bool,
}

impl SiegeTactics {
    pub const NONE: SiegeTactics = SiegeTactics { ground: false, firing: false, outrange: false, fortify: false };
    pub const ALL: SiegeTactics = SiegeTactics { ground: true, firing: true, outrange: true, fortify: true };

    /// Some switch is on: the turn keeps the sight the danger reads
    /// (`TurnBrief::danger`).
    pub fn any(self) -> bool {
        self.ground || self.firing || self.outrange || self.fortify
    }
}

/// The ranged range of a unit of `kind` with `promotions`: the kind's range
/// and every unit-stat change of range its promotions give (the rules'
/// `UnitStat::Range`, `combat::strength`). None: it fights in melee.
pub fn unit_range(rules: &CompiledRuleset, kind: UnitKindId, promotions: &PromotionSet) -> Option<i32> {
    let def = rules.units.get(kind.0 as usize)?;
    if def.ranged_strength == 0 || def.range == 0 {
        return None;
    }
    let bonus: i32 = promotions
        .iter()
        .filter_map(|p| rules.promotions.get(p.0 as usize))
        .flat_map(|p| p.effects.passive_unit.iter())
        .filter_map(|e| match &e.modifier {
            Modifier::ModifyUnitStat { stat: UnitStat::Range, amount } => Some(*amount),
            _ => None,
        })
        .sum();
    Some((i32::from(def.range) + bonus).max(1))
}

/// The range a decision counts for `unit`: with its promotions under
/// `outrange`, the kind's alone otherwise (`combat::ranged_range`, the game's
/// AI).
pub fn range_of(rules: &CompiledRuleset, unit: &OwnUnit, siege: SiegeTactics) -> Option<i32> {
    if siege.outrange {
        unit_range(rules, unit.kind, &unit.promotions)
    } else {
        crate::ai::combat::ranged_range(rules, unit.kind)
    }
}

/// The unit outranges a city: its range is beyond the city's strike range
/// (`combat::city::CITY_ATTACK_RANGE`), so some tiles it strikes a city from
/// are tiles that city cannot strike.
pub fn outranges(range: Option<i32>) -> bool {
    range.is_some_and(|r| r > CITY_ATTACK_RANGE)
}

/// The defence of a known tile for a unit of `owner` standing on it, in
/// percent: the ground (hills, forest, jungle, marsh by the rules' data) and
/// its improvement where the rules count it (unowned or own land), as
/// `combat::terrain::tile_defense_percent` reads the world.
pub fn ground_percent(view: &PlayerView<'_>, rules: &CompiledRuleset, tile: TileId, owner: PlayerId) -> i64 {
    let Some(snapshot) = view.tile(tile).snapshot().copied() else { return 0 };
    let kinds = view.terrain_kinds();
    let high = kinds.is_hills(snapshot.terrain.0) || kinds.is_mountains(snapshot.terrain.0);
    let mut percent = crate::combat::terrain::ground_defense_percent(rules, high, snapshot.feature);
    if snapshot.owner.is_none() || snapshot.owner == Some(owner) {
        percent += snapshot.improvement.and_then(|i| rules.improvements.get(i.0 as usize)).map_or(0, |d| i64::from(d.defense_pct));
    }
    percent
}

/// One tile a ranged unit may shoot a target from, as `firing_tile` weighs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FiringPlace {
    pub tile: TileId,
    /// Enemy cities whose strike reaches the tile.
    pub city_strikes: i32,
    /// Enemy melee units next to the tile after the shot (the target counted
    /// gone when the shot is expected to kill it).
    pub melee_next: i32,
    /// The ground's defence there, percent.
    pub ground: i64,
}

/// The tile to shoot from among `places` (the unit's own tile `here` among
/// them when it may shoot from there): with `outrange` or `firing` first out
/// of the enemy cities' strike, with `firing` then away from enemy melee
/// units (a ranged unit has no answer to a charge) and on the strongest
/// ground; staying wins every tie (the move costs the unit its
/// fortification), then the lower tile.
pub fn firing_tile(here: TileId, places: &[FiringPlace], siege: SiegeTactics) -> Option<TileId> {
    places
        .iter()
        .min_by_key(|p| {
            let strikes = if siege.outrange || siege.firing { p.city_strikes } else { 0 };
            let (melee, ground) = if siege.firing { (p.melee_next, -p.ground) } else { (0, 0) };
            (strikes, melee, ground, p.tile != here, p.tile)
        })
        .map(|p| p.tile)
}

/// Enemy cities (at war, in `cities`) whose strike reaches `tile`.
pub fn city_strikes(view: &PlayerView<'_>, cities: &[TileId], tile: TileId) -> i32 {
    let dims = view.dimensions();
    cities.iter().filter(|c| dims.distance(**c, tile).is_some_and(|d| d <= CITY_ATTACK_RANGE)).count() as i32
}

/// Where a ranged `unit` of the player shoots `target` from this turn:
/// its own tile when the target is in reach there, or a tile of `stops`
/// (tiles it may stop on, with the moves left there) it reaches with moves
/// left; a unit that must set up first shoots only from where it stands (a
/// move undoes the set-up), so does a unit that has moved this turn while
/// the target is in reach where it stands, and so does a unit in an own city
/// (the city is its strongest ground, and its garrison).
/// Weighed by `firing_tile`. `foe_hp` is the target's health; `cities`
/// the enemy cities at war, `melee` the tiles of the enemy melee units in
/// sight.
#[allow(clippy::too_many_arguments)]
pub fn choose_firing_tile(
    view: &PlayerView<'_>,
    rules: &CompiledRuleset,
    siege: SiegeTactics,
    unit: &OwnUnit,
    range: i32,
    target: TileId,
    foe_hp: u16,
    stops: &[(TileId, u16)],
    cities: &[TileId],
    melee: &[TileId],
) -> Option<TileId> {
    use crate::knowledge::public::PlayerQueries;
    let dims = view.dimensions();
    let fits = |t: TileId| dims.distance(t, target).is_some_and(|d| (1..=range).contains(&d));
    // A siege unit set up already would undo its set-up by a step.
    let set_up = view.must_set_up_first(unit.handle) || unit.stance == crate::world::unit::Stance::SetUp;
    // A unit that has moved this turn shoots from where it stands: it chose
    // its firing tile before it moved (no second step on a fresh estimate).
    let fresh = unit.moves_left >= unit.moves_max;
    // An own city is the strongest ground there is: its garrison shoots from it.
    let home = view.tile(unit.tile).snapshot().is_some_and(|s| s.city && s.owner == Some(view.player()));
    if home && fits(unit.tile) {
        return view.attack_preview_from(unit.handle, unit.tile, target).map(|_| unit.tile);
    }
    let mut tiles: Vec<TileId> = vec![unit.tile];
    if !set_up && (fresh || !fits(unit.tile)) {
        tiles.extend(stops.iter().filter(|(t, left)| *left > 0 && *t != unit.tile).map(|(t, _)| *t));
    }
    let places: Vec<FiringPlace> = tiles
        .into_iter()
        .filter(|t| fits(*t))
        .filter_map(|t| {
            let preview = view.attack_preview_from(unit.handle, t, target)?;
            let killed = preview.to_defender.1 >= foe_hp;
            let melee_next = melee.iter().filter(|m| !(killed && **m == target) && dims.adjacent(**m, t)).count() as i32;
            Some(FiringPlace { tile: t, city_strikes: city_strikes(view, cities, t), melee_next, ground: ground_percent(view, rules, t, unit_owner(view)) })
        })
        .collect();
    firing_tile(unit.tile, &places, siege)
}

fn unit_owner(view: &PlayerView<'_>) -> PlayerId {
    view.player()
}

/// The enemy cities at war a unit decision knows: in its sight, in the
/// turn's sight beyond it, and remembered out of sight (`danger::Sight`).
pub fn enemy_cities(view: &crate::ai::planner::UnitView<'_, '_>) -> Vec<TileId> {
    let local = view.local;
    let at_war = |owner: PlayerId| local.relation(owner).is_some_and(|r| r.at_war);
    let mut cities: Vec<TileId> = local.foreign_cities.iter().filter(|c| at_war(c.owner)).map(|c| c.tile).collect();
    if let Some(sight) = view.brief.danger.as_ref() {
        cities.extend(sight.cities.iter().filter(|c| at_war(c.owner)).map(|c| c.tile));
        cities.extend(sight.remembered.iter().filter(|c| at_war(c.owner)).map(|c| c.tile));
    }
    cities.sort();
    cities.dedup();
    cities
}

/// The tiles of the enemy melee units at war a unit decision sees: military
/// units that do not shoot (`combat::ranged_range`).
pub fn enemy_melee(view: &crate::ai::planner::UnitView<'_, '_>) -> Vec<TileId> {
    let local = view.local;
    let rules = local.view.rules();
    let at_war = |owner: PlayerId| local.relation(owner).is_some_and(|r| r.at_war);
    let melee = |kind: UnitKindId| crate::ai::planner::role_of(rules, kind) == crate::ai::planner::AiRole::Military && crate::ai::combat::ranged_range(rules, kind).is_none();
    local.foreign_units.iter().filter(|u| at_war(u.owner) && melee(u.kind)).map(|u| u.tile).collect()
}

/// Tiles the unit of a decision may stop on this turn, with the moves left
/// there (its own first).
pub fn unit_stops(view: &crate::ai::planner::UnitView<'_, '_>) -> Vec<(TileId, u16)> {
    let rules = view.local.view.rules();
    let unit = view.local.unit;
    let mut stops: Vec<(TileId, u16)> =
        view.local.view.own_reachable(unit.handle).into_iter().filter(|(t, _)| *t == unit.tile || crate::ai::planner_units::can_stop(view, rules, *t)).collect();
    stops.sort_by_key(|(t, _)| (*t != unit.tile, *t));
    stops
}

/// A unit decision's shot (`decision`, a `RangedAttack`) with `firing` or
/// `outrange`: the unit first steps to its firing tile when that is not
/// where it stands (`choose_firing_tile`); the shot is made from there at the
/// next decision. Any other decision is returned as it is.
pub fn step_to_fire(view: &crate::ai::planner::UnitView<'_, '_>, decision: crate::ai::planner::Decision) -> crate::ai::planner::Decision {
    use crate::command::Command;
    let siege = view.brief.siege;
    let target = match &decision.command {
        Some(Command::RangedAttack { target, .. }) => *target,
        _ => return decision,
    };
    if !(siege.firing || siege.outrange) {
        return decision;
    }
    let rules = view.local.view.rules();
    let unit = view.local.unit;
    let Some(range) = range_of(rules, &unit, siege) else { return decision };
    let foe_hp = view
        .local
        .foreign_cities
        .iter()
        .find(|c| c.tile == target)
        .map(|c| c.hp)
        .or_else(|| view.local.foreign_units.iter().filter(|u| u.tile == target).map(|u| u.hp).max())
        .unwrap_or(0);
    let stops = unit_stops(view);
    let cities = enemy_cities(view);
    let best = choose_firing_tile(&view.local.view, rules, siege, &unit, range, target, foe_hp, &stops, &cities, &enemy_melee(view));
    match best {
        Some(tile) if tile != unit.tile => crate::ai::planner::Decision::act(
            crate::ai::trace::Actor::Unit(unit.id),
            crate::ai::trace::Reason::Advance,
            Command::MoveUnit { unit: unit.handle, destination: tile },
        ),
        _ => decision,
    }
}

/// The military advisor's start-of-turn shots with `firing` or `outrange`:
/// a land unit that would shoot its target better from another tile it
/// reaches (`choose_firing_tile`) steps there first — the advisor's move,
/// then its shot, in the order of its plan; no two units are sent to one
/// tile. A unit that must set up, or has no better tile, shoots from where it
/// stands.
pub fn keep_shots(input: &crate::ai::system::AdvisorInput<'_, '_>, shots: Vec<crate::command::Command>) -> Vec<crate::command::Command> {
    use crate::command::Command;
    use crate::knowledge::public::PlayerQueries;
    let siege = input.brief.siege;
    if !(siege.firing || siege.outrange) || shots.is_empty() {
        return shots;
    }
    let obs = input.observation;
    let rules = input.rules;
    let at_war = |owner: PlayerId| obs.relation(owner).is_some_and(|r| r.at_war);
    let mut cities: Vec<TileId> = obs.visible_foreign_cities.iter().filter(|c| at_war(c.owner)).map(|c| c.tile).collect();
    cities.extend(input.state.seen_cities().iter().filter(|c| at_war(c.owner)).map(|c| c.tile));
    cities.sort();
    cities.dedup();
    let shooter = |command: &Command| match command {
        Command::RangedAttack { unit, target } => obs.own_units.iter().find(|u| u.handle == *unit).map(|u| (*u, *target)),
        _ => None,
    };
    let melee_kind = |kind: UnitKindId| crate::ai::planner::role_of(rules, kind) == crate::ai::planner::AiRole::Military && crate::ai::combat::ranged_range(rules, kind).is_none();
    let melee: Vec<TileId> = obs.visible_foreign_units.iter().filter(|u| at_war(u.owner) && melee_kind(u.kind)).map(|u| u.tile).collect();
    let mut claimed: Vec<TileId> = Vec::new();
    let mut out = Vec::with_capacity(shots.len());
    for command in shots {
        let Some((unit, target)) = shooter(&command) else {
            out.push(command);
            continue;
        };
        let land = rules.units.get(unit.kind.0 as usize).is_some_and(|d| d.domain == crate::rules::content::Domain::Land);
        let range = range_of(rules, &unit, siege);
        let set_up = obs.view.must_set_up_first(unit.handle) || unit.stance == crate::world::unit::Stance::SetUp;
        if !land || unit.moves_left == 0 || set_up || range.is_none() {
            out.push(command);
            continue;
        }
        let foe_hp = obs
            .visible_foreign_cities
            .iter()
            .find(|c| c.tile == target)
            .map(|c| c.hp)
            .or_else(|| obs.visible_foreign_units.iter().filter(|u| u.tile == target).map(|u| u.hp).max())
            .unwrap_or(0);
        let stops: Vec<(TileId, u16)> = advisor_stops(obs, rules, &unit).into_iter().filter(|(t, _)| !claimed.contains(t)).collect();
        let best = choose_firing_tile(&obs.view, rules, siege, &unit, range.unwrap_or(1), target, foe_hp, &stops, &cities, &melee);
        if let Some(tile) = best.filter(|t| *t != unit.tile) {
            claimed.push(tile);
            out.push(Command::MoveUnit { unit: unit.handle, destination: tile });
        }
        out.push(command);
    }
    out
}

/// Tiles a unit may stop on this turn as the turn starts, with the moves
/// left there: land, no foreign unit, no own unit of its stack class.
fn advisor_stops(obs: &crate::knowledge::observation::AIObservation<'_>, rules: &CompiledRuleset, unit: &OwnUnit) -> Vec<(TileId, u16)> {
    let class = crate::world::stacking::stack_class(rules, unit.kind);
    let kinds = obs.view.terrain_kinds();
    let free = |t: TileId| {
        let water = obs.view.tile(t).snapshot().is_none_or(|s| kinds.is_water(s.terrain.0));
        let own = obs.own_units.iter().any(|u| u.id != unit.id && u.tile == t && crate::world::stacking::stack_class(rules, u.kind) == class);
        !water && !own && !obs.visible_foreign_units.iter().any(|u| u.tile == t)
    };
    let mut stops: Vec<(TileId, u16)> = obs.view.own_reachable(unit.handle).into_iter().filter(|(t, _)| *t == unit.tile || free(*t)).collect();
    stops.sort_by_key(|(t, _)| (*t != unit.tile, *t));
    stops
}

/// The unit of a decision is within an enemy's reach where it stands: an
/// enemy military unit in sight that walks and strikes to it in a turn (its
/// kind's movement and range, the rules' data), or an enemy city whose strike
/// reaches it.
pub fn within_reach(view: &crate::ai::planner::UnitView<'_, '_>) -> bool {
    let local = view.local;
    let rules = local.view.rules();
    let dims = local.view.dimensions();
    let tile = local.unit.tile;
    let at_war = |owner: PlayerId| local.relation(owner).is_some_and(|r| r.at_war);
    let units = local.foreign_units.iter().filter(|u| at_war(u.owner) && crate::ai::planner::role_of(rules, u.kind) == crate::ai::planner::AiRole::Military).any(|u| {
        let def = rules.units.get(u.kind.0 as usize);
        let reach = def.map_or(1, |d| i32::from(d.movement.max(1)) + i32::from(d.range.max(1)));
        dims.distance(u.tile, tile).is_some_and(|d| d <= reach)
    });
    units || city_strikes(&local.view, &enemy_cities(view), tile) > 0
}

/// The tile a unit's task holds it to (None: a task without one, or none):
/// the city it guards or besieges, the tile it heals at, the city its raid or
/// operation aims at.
fn task_tile(state: &crate::ai::state::AiState, unit: &OwnUnit) -> Option<TileId> {
    use crate::ai::state::UnitTask;
    match state.task_of(unit.id).map(|t| t.task)? {
        UnitTask::Defend { city } => Some(city),
        UnitTask::Assault { target } | UnitTask::Operation { target } | UnitTask::Raid { target } => Some(target),
        UnitTask::Heal { at } => Some(at),
        UnitTask::Scout { target } => Some(target),
        UnitTask::Settle { site } => Some(site),
        UnitTask::Improve { tile, .. } => Some(tile),
        UnitTask::Escort { .. } => None,
    }
}

/// With `SiegeTactics::ground`, a healthy unit about to hold (`decision`: a
/// fortification, or no command) within an enemy's reach, not fortified yet,
/// out of an own city and with moves left, steps first onto the next tile
/// with stronger ground (`ground_percent`) that is no more exposed — no more
/// enemy cities strike it, no more enemy melee units stand next to it — and
/// no farther from the tile its task holds it to; the strongest such ground,
/// then the fewest enemies next to it. Every such step gains ground, so a
/// unit cannot step back and forth. A wounded unit rests where it is (a move
/// costs the turn's healing). Any other decision is returned as it is.
pub fn settle(view: &crate::ai::planner::UnitView<'_, '_>, state: &crate::ai::state::AiState, decision: crate::ai::planner::Decision) -> crate::ai::planner::Decision {
    use crate::command::Command;
    let unit = view.local.unit;
    let holds = match &decision.command {
        None => true,
        Some(Command::Fortify { unit: u }) => *u == unit.handle,
        Some(_) => false,
    };
    if !view.brief.siege.ground || !holds || unit.fortified || unit.moves_left == 0 || unit.hp < crate::ai::planner_units::HEAL_BELOW_HP {
        return decision;
    }
    let rules = view.local.view.rules();
    if crate::ai::planner::role_of(rules, unit.kind) != crate::ai::planner::AiRole::Military || !unit.can_fortify {
        return decision;
    }
    if view.local.own_cities.iter().any(|c| c.tile == unit.tile) || !within_reach(view) {
        return decision;
    }
    let dims = view.local.view.dimensions();
    let owner = view.local.player;
    let cities = enemy_cities(view);
    let melee = enemy_melee(view);
    let exposure = |t: TileId| (city_strikes(&view.local.view, &cities, t), melee.iter().filter(|m| dims.adjacent(**m, t)).count() as i32);
    let anchor = task_tile(state, &unit);
    let far = |t: TileId| anchor.and_then(|a| dims.distance(a, t)).unwrap_or(0);
    let here = (ground_percent(&view.local.view, rules, unit.tile, owner), exposure(unit.tile), far(unit.tile));
    let best = unit_stops(view)
        .into_iter()
        .map(|(t, _)| t)
        .filter(|t| dims.adjacent(*t, unit.tile))
        .map(|t| (t, ground_percent(&view.local.view, rules, t, owner), exposure(t), far(t)))
        .filter(|(_, ground, (strikes, next), far)| *ground > here.0 && *strikes <= here.1 .0 && *next <= here.1 .1 && *far <= here.2)
        .max_by_key(|(t, ground, (strikes, next), _)| (*ground, -strikes, -next, std::cmp::Reverse(*t)));
    match best {
        Some((tile, ..)) => crate::ai::planner::Decision::act(crate::ai::trace::Actor::Unit(unit.id), decision.reason, Command::MoveUnit { unit: unit.handle, destination: tile }),
        None => decision,
    }
}

/// A decision that ends the unit's turn without a command (`decision`) with
/// `SiegeTactics::fortify`: a military land unit within an enemy's reach,
/// out of an own city, not fortified and able to fortify, fortifies first.
pub fn dig_in(view: &crate::ai::planner::UnitView<'_, '_>, decision: crate::ai::planner::Decision) -> crate::ai::planner::Decision {
    let unit = view.local.unit;
    if !view.brief.siege.fortify || decision.command.is_some() || unit.fortified || !unit.can_fortify {
        return decision;
    }
    let rules = view.local.view.rules();
    if crate::ai::planner::role_of(rules, unit.kind) != crate::ai::planner::AiRole::Military {
        return decision;
    }
    if view.local.own_cities.iter().any(|c| c.tile == unit.tile) || !within_reach(view) {
        return decision;
    }
    crate::ai::planner::Decision::act(crate::ai::trace::Actor::Unit(unit.id), decision.reason, crate::command::Command::Fortify { unit: unit.handle })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place(tile: u32, city_strikes: i32, melee_next: i32, ground: i64) -> FiringPlace {
        FiringPlace { tile: TileId(tile), city_strikes, melee_next, ground }
    }

    /// The firing tile: out of the city's strike first (`outrange`, `firing`),
    /// then away from enemy melee and on the strongest ground (`firing`); the
    /// own tile wins ties.
    #[test]
    fn the_firing_tile_is_the_least_exposed_one() {
        let here = TileId(1);
        let places = [place(1, 1, 2, 0), place(2, 0, 1, 0), place(3, 0, 0, 25), place(4, 0, 0, 0), place(5, 1, 0, 50)];
        let firing = SiegeTactics { firing: true, ..SiegeTactics::NONE };
        let outrange = SiegeTactics { outrange: true, ..SiegeTactics::NONE };
        assert_eq!(firing_tile(here, &places, firing), Some(TileId(3)), "out of the strike, no melee next to it, the hill");
        assert_eq!(firing_tile(here, &places, outrange), Some(TileId(2)), "out of the city's strike, the lowest tile");
        assert_eq!(firing_tile(here, &places, SiegeTactics::NONE), Some(here), "nothing weighed: the unit stays");
        let even = [place(6, 0, 0, 0), place(1, 0, 0, 0)];
        assert_eq!(firing_tile(here, &even, SiegeTactics::ALL), Some(here), "a tie keeps the unit where it stands");
    }

    /// A unit outranges a city only with a range beyond the city's strike.
    #[test]
    fn outranging_is_a_range_beyond_the_city_strike() {
        assert!(!outranges(None));
        assert!(!outranges(Some(CITY_ATTACK_RANGE)));
        assert!(outranges(Some(CITY_ATTACK_RANGE + 1)));
    }
}
