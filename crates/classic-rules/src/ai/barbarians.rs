//! Minimal barbarian behaviour over `AIObservation` (M4 track C), as the
//! barbarian rules describe it (rules.md): attack what can be beaten,
//! pillage foreign improvements, otherwise march on the nearest visible
//! city or unit within `HUNT_RANGE` that can be reached over known land;
//! with nothing in sight stay at camp.

use crate::ai::planner_map::{
    find_path, Blockers, MapScratch, PathOutcome, SearchLimit, SearchStats,
};
use crate::command::Command;
use crate::ids::{TileId, UnitHandle};
use crate::knowledge::observation::{AIObservation, OwnUnit};
use crate::rules::CompiledRuleset;
use crate::classic::content::BARBARIAN_CAMP;

/// Barbarians go after targets no farther than this.
pub const HUNT_RANGE: i32 = 8;
/// Nodes of one reachability search (the hunting disc has 217 tiles).
const SEARCH_NODES: u32 = 400;

/// The barbarian player's orders for one turn: shots, charges (one per
/// target: the orders are planned together), pillage, marches — each unit
/// at most one order.
pub fn barbarian_orders(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Vec<Command> {
    let mut attacks = crate::ai::combat::ranged_orders(obs, rules);
    attacks.extend(crate::ai::combat::melee_orders(obs, rules));
    let mut targets: Vec<TileId> = Vec::new();
    let mut out = Vec::new();
    for command in attacks {
        let target = attack_target(obs, &command);
        if target.is_some_and(|t| !targets.contains(&t)) {
            targets.extend(target);
            out.push(command);
        }
    }
    let busy: Vec<UnitHandle> = out.iter().filter_map(ordered_unit).collect();
    let mut scratch = MapScratch::default();
    for unit in obs
        .own_units
        .iter()
        .filter(|u| u.moves_left > 0 && !busy.contains(&u.handle))
    {
        let order =
            pillage_order(obs, rules, unit).or_else(|| march_order(obs, rules, unit, &mut scratch));
        out.extend(order);
    }
    out
}

fn ordered_unit(command: &Command) -> Option<UnitHandle> {
    match command {
        Command::RangedAttack { unit, .. } => Some(*unit),
        Command::AttackUnit { attacker, .. } | Command::AttackCity { attacker, .. } => {
            Some(*attacker)
        }
        _ => None,
    }
}

fn attack_target(obs: &AIObservation<'_>, command: &Command) -> Option<TileId> {
    match command {
        Command::RangedAttack { target, .. } => Some(*target),
        Command::AttackUnit { defender, .. } => obs
            .visible_foreign_units
            .iter()
            .find(|u| u.handle == *defender)
            .map(|u| u.tile),
        Command::AttackCity { city, .. } => obs
            .visible_foreign_cities
            .iter()
            .find(|c| c.handle == *city)
            .map(|c| c.tile),
        _ => None,
    }
}

/// Pillage when standing on an improvement of somebody else (units that
/// can pillage, not on a camp or ruins).
pub fn pillage_order(
    obs: &AIObservation<'_>,
    rules: &CompiledRuleset,
    unit: &OwnUnit,
) -> Option<Command> {
    let snapshot = *obs.view.tile(unit.tile).snapshot()?;
    let foreign = snapshot.owner.is_some_and(|o| o != obs.player) && !snapshot.city;
    let can = rules
        .units
        .get(unit.kind.0 as usize)
        .is_some_and(|d| d.combat.pillage);
    let intact = !obs.view.known_pillaged(unit.tile);
    let worth = intact && snapshot.improvement.is_some_and(|i| {
        rules.improvements.get(i.0 as usize).is_some_and(|d| {
            !matches!(
                d.kind,
                crate::rules::content::ImprovementKind::AncientRuins
                    | BARBARIAN_CAMP
            )
        })
    });
    (foreign && can && worth).then_some(Command::Pillage { unit: unit.handle })
}

/// March towards the nearest visible foreign city or unit in range that
/// the unit can reach over the land it knows.
pub fn march_order(
    obs: &AIObservation<'_>,
    rules: &CompiledRuleset,
    unit: &OwnUnit,
    scratch: &mut MapScratch,
) -> Option<Command> {
    if rules.is_civilian(unit.kind) {
        return None;
    }
    let destination = targets_by_distance(obs, unit.tile)
        .into_iter()
        .filter_map(|target| approach(obs, unit.tile, target))
        .find(|tile| reachable(obs, unit.handle, unit.tile, *tile, scratch))?;
    Some(Command::MarchTo {
        unit: unit.handle,
        destination,
    })
}

/// A path over the land the unit itself may enter (its profile: ground it
/// cannot cross is no way, the core refuses a march onto it).
fn reachable(obs: &AIObservation<'_>, unit: UnitHandle, from: TileId, to: TileId, scratch: &mut MapScratch) -> bool {
    let limit = SearchLimit {
        max_nodes: SEARCH_NODES,
    };
    let mut stats = SearchStats::default();
    let blockers = Blockers::default();
    let outcome = find_path(&crate::ai::planner_map::KnownMap::of_unit(&obs.view, unit), from, to, &blockers, scratch, limit, &mut stats);
    matches!(outcome, PathOutcome::Found { .. })
}

fn dimensions(obs: &AIObservation<'_>) -> crate::math::MapDimensions {
    crate::math::MapDimensions {
        width: obs.map_width,
        height: obs.map_height,
    }
}

/// The free neighbour of `target` nearest to `from` (the unit stops next
/// to its prey and attacks next turn); None when already there.
fn approach(obs: &AIObservation<'_>, from: TileId, target: TileId) -> Option<TileId> {
    let dims = dimensions(obs);
    let occupied = |t: TileId| {
        obs.visible_foreign_units.iter().any(|u| u.tile == t)
            || obs.visible_foreign_cities.iter().any(|c| c.tile == t)
            || obs.own_units.iter().any(|u| u.tile == t)
    };
    dims.neighbors(target)
        .into_iter()
        .filter(|t| !occupied(*t))
        .filter_map(|t| dims.distance(from, t).map(|d| (d, t)))
        .min_by_key(|(d, t)| (*d, t.0))
        .filter(|(d, _)| *d > 0)
        .map(|(_, t)| t)
}

/// Visible foreign cities and units within HUNT_RANGE, nearest first.
fn targets_by_distance(obs: &AIObservation<'_>, from: TileId) -> Vec<TileId> {
    let dims = dimensions(obs);
    let mut near: Vec<(i32, TileId)> = obs
        .visible_foreign_cities
        .iter()
        .map(|c| c.tile)
        .chain(obs.visible_foreign_units.iter().map(|u| u.tile))
        .filter_map(|t| dims.distance(from, t).map(|d| (d, t)))
        .filter(|(d, _)| *d > 0 && *d <= HUNT_RANGE)
        .collect();
    near.sort_by_key(|(d, t)| (*d, t.0));
    near.dedup();
    near.into_iter().map(|(_, t)| t).collect()
}
