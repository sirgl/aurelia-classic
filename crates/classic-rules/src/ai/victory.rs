//! Decisions towards a science victory (M4 track C):
//!
//! - production (asked by the city planner before buildings): the Apollo
//!   Program once Rocketry is known, then exactly the spaceship parts the
//!   ship still lacks — parts installed, parts walking and parts queued
//!   count — in the most productive cities;
//! - units: parts walk to the capital and are installed there; an own
//!   civilian resting in the capital steps aside for them (one civilian
//!   per tile).
//!
//! Whether an order is possible is asked through `PlayerView::accepts`
//! (the same check the interface greys buttons with).

use crate::knowledge::public::PlayerQueries;
use crate::ai::planner::{role_of, AiRole};
use crate::command::Command;
use crate::ids::{ProductionItem, TileId, UnitKindId};
use crate::knowledge::observation::{AIObservation, OwnCity};
use crate::rules::content::{BuildingKind, ProjectId};
use crate::rules::CompiledRuleset;
use crate::world::victory::VictoryKind;

/// The capital: the city with the palace (parts are installed there),
/// else the oldest own city.
fn capital<'o>(obs: &'o AIObservation<'_>, rules: &CompiledRuleset) -> Option<&'o OwnCity> {
    let palace = |city: &&OwnCity| {
        city.buildings
            .iter()
            .any(|b| rules.buildings.get(b.0 as usize).is_some_and(|d| d.kind == BuildingKind::Palace))
    };
    obs.own_cities.iter().find(palace).or(obs.own_cities.first())
}

fn apollo(rules: &CompiledRuleset) -> Option<ProjectId> {
    rules
        .module.projects
        .iter()
        .position(|p| p.key.ends_with("apollo_program"))
        .map(|i| ProjectId(i as u16))
}

/// Spaceship part units with the project each one installs and how many
/// of it the ship needs.
fn part_kinds(rules: &CompiledRuleset) -> Vec<(UnitKindId, ProjectId, u16)> {
    let parts = crate::world::victory::spaceship_parts(rules);
    let mut out = Vec::new();
    for (index, def) in rules.units.iter().enumerate() {
        let Some(project) = def.spaceship_project else {
            continue;
        };
        if let Some((_, needed)) = parts.iter().find(|(p, _)| *p == project) {
            out.push((UnitKindId(index as u16), project, *needed));
        }
    }
    out
}

fn completed(obs: &AIObservation<'_>, project: ProjectId) -> u16 {
    obs.economy.projects.get(project.0 as usize).copied().unwrap_or(0)
}

/// Queued in any own city or already planned this turn.
fn ordered(obs: &AIObservation<'_>, planned: &[ProductionItem], item: ProductionItem) -> usize {
    let queued = obs.own_cities.iter().flat_map(|c| c.queue.iter()).filter(|i| **i == item).count();
    queued + planned.iter().filter(|i| **i == item).count()
}

/// The next space order of `city` (None: nothing towards the spaceship is
/// wanted or possible here). `planned` holds the orders already given this
/// turn in other cities.
pub fn next_space_item(
    obs: &AIObservation<'_>,
    rules: &CompiledRuleset,
    city: &OwnCity,
    planned: &[ProductionItem],
) -> Option<ProductionItem> {
    if !obs.victories.enabled(VictoryKind::Science) {
        return None;
    }
    let apollo = apollo(rules)?;
    let accepted = |item: ProductionItem| {
        obs.view.accepts(&Command::EnqueueProduction {
            player: obs.player,
            city: city.handle,
            item,
        })
    };
    if completed(obs, apollo) == 0 {
        let item = ProductionItem::Project(apollo);
        let wanted = ordered(obs, planned, item) == 0 && accepted(item);
        return wanted.then_some(item);
    }
    for (kind, project, needed) in part_kinds(rules) {
        let item = ProductionItem::Unit(kind);
        let walking = obs.own_units.iter().filter(|u| u.kind == kind).count();
        let have = usize::from(completed(obs, project)) + walking + ordered(obs, planned, item);
        if have < usize::from(needed) && accepted(item) {
            return Some(item);
        }
    }
    None
}

fn is_part(rules: &CompiledRuleset, kind: UnitKindId) -> bool {
    rules.units.get(kind.0 as usize).is_some_and(|d| d.spaceship_project.is_some())
}

/// Parts in the capital are installed; the others march there.
pub fn spaceship_orders(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Vec<Command> {
    let Some(capital) = capital(obs, rules).map(|c| c.tile) else {
        return Vec::new();
    };
    let waiting = obs.own_units.iter().any(|u| is_part(rules, u.kind) && u.tile != capital);
    let mut out: Vec<Command> = Vec::new();
    if waiting {
        out.extend(clear_capital(obs, rules, capital));
    }
    let parts = obs
        .own_units
        .iter()
        .filter(|u| is_part(rules, u.kind))
        .map(|u| {
            if u.tile == capital {
                Command::BuildSpaceshipPart { unit: u.handle }
            } else {
                Command::MarchTo {
                    unit: u.handle,
                    destination: capital,
                }
            }
        })
        .filter(|c| obs.view.accepts(c));
    out.extend(parts);
    out
}

/// Own civilians (not parts) standing in the capital march to free land
/// nearby (own or unclaimed), so a part can enter: one civilian per tile, and cities
/// training units over a full tile stack several there.
fn clear_capital(obs: &AIObservation<'_>, rules: &CompiledRuleset, capital: TileId) -> Vec<Command> {
    let civilian = |kind: UnitKindId| !matches!(role_of(rules, kind), AiRole::Military | AiRole::Naval);
    let blockers = obs
        .own_units
        .iter()
        .filter(|u| u.tile == capital && civilian(u.kind) && !is_part(rules, u.kind) && u.moves_left > 0);
    let kinds = obs.view.terrain_kinds();
    let free = |tile: &TileId| {
        let own = obs.view.tile(*tile).snapshot().is_some_and(|s| {
            s.owner.is_none_or(|o| o == obs.player) && !s.city && kinds.enter_cost(s.terrain).is_some()
        });
        own && !obs.own_units.iter().any(|u| u.tile == *tile && civilian(u.kind))
            && !obs.visible_foreign_units.iter().any(|u| u.tile == *tile)
    };
    let dims = obs.view.dimensions();
    let mut tiles: Vec<TileId> = crate::math::hex_disc(dims, capital, CLEARING_RADIUS).filter(free).collect();
    tiles.sort_by_key(|t| (dims.distance(capital, *t).unwrap_or(i32::MAX), t.0));
    let mut tiles = tiles.into_iter();
    let mut out = Vec::new();
    for blocker in blockers {
        let command = tiles
            .by_ref()
            .map(|tile| Command::MarchTo { unit: blocker.handle, destination: tile })
            .find(|command| obs.view.accepts(command));
        out.extend(command);
    }
    out
}

/// Civilians leaving the capital go at most this far.
const CLEARING_RADIUS: i32 = 3;

/// The victory system's advisor (`ai::system`): parts move and install at
/// the start of the turn (their production is the city planner's).
#[derive(Debug, Clone, Copy, Default)]
pub struct VictoryAdvisor;

impl crate::ai::system::SystemAdvisor for VictoryAdvisor {
    fn start_of_turn(
        &self,
        input: &crate::ai::system::AdvisorInput<'_, '_>,
    ) -> Vec<crate::ai::system::Proposal> {
        crate::ai::combat::proposals(12, spaceship_orders(input.observation, input.rules))
    }
}
