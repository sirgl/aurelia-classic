//! The supply of the strategist's goal (`ship-supply`, `Prices::supply`): the strategic copies the
//! items of the goal need, counted from the rules' data, and what a missing copy is worth.
//!
//! The goal is the science victory where it fits the game (`tall_real::science_game`); its items
//! are the spaceship parts (`victory::part_kinds`) and the buildings with a production percent
//! toward them (`Condition::Producing(SpaceshipParts)`). A part holds its copies from its
//! completion until it is installed, a building as long as it stands
//! (`derived::resources`), and the rules let no part start or complete without a free copy
//! (`derived::resources::production_resources`):
//!
//! | quantity | from the rules' data |
//! |---|---|
//! | builders | the most productive own cities, one a part still missing (installed, walking: not missing) |
//! | need | each builder: the copies of its part, and of the speeder it lacks |
//! | short | need − the copies free now (the empire's ledger) |
//! | worth of a copy | the hammers of the part it lets start (its cost over its copies) × the strategist's supply multiplier |
//!
//! The need opens with the technology that reveals the resource (`ResourceDefinition::required_tech`):
//! before it the player sees no deposit and trades no copy. It holds from then while the rest of the
//! goal is built, so the copies are in hand when the parts open. Every channel converts the worth at
//! its own price of production: the tile a city buys (`allocator`), the improvement a worker builds
//! (`planner_workers`), a deal (`peace`), a gift to a city-state (`city_states`), and a unit that holds
//! a copy a part waits for is disbanded (`release_orders`). No resource and no item is named.

use crate::ai::strategy_net::Milli;
use crate::command::Command;
use crate::ids::{PlayerId, ResourceId, UnitHandle};
use crate::knowledge::observation::AIObservation;
use crate::knowledge::public::PlayerQueries;
use crate::rules::content::ResourceClass;
use crate::rules::effects::{Condition, ProductionFilter};
use crate::rules::CompiledRuleset;

/// Resources one goal may be short of at once (the classic parts need one; the slots beyond the
/// rules' resources stay empty).
pub const SUPPLY_SLOTS: usize = 4;

/// Copies of one resource the goal lacks. `copies` 0 is an empty slot (`EMPTY`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shortage {
    pub resource: ResourceId,
    pub copies: i32,
    /// The hammers one missing copy holds back: the dearest part that needs it, its cost over the copies it needs.
    pub hammers: i64,
}

const EMPTY: Shortage = Shortage { resource: ResourceId(0), copies: 0, hammers: 0 };

/// The goal's shortages and the strategist's multiplier of their worth (thousandths,
/// `strategy_net::supply_mult`; 1000 before the strategist thinks).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SupplyNeed {
    pub slots: [Shortage; SUPPLY_SLOTS],
    pub mult: Milli,
}

impl Default for SupplyNeed {
    fn default() -> Self {
        Self { slots: [EMPTY; SUPPLY_SLOTS], mult: crate::ai::strategy_net::layers::ONE }
    }
}

impl SupplyNeed {
    /// The shortages of the observing empire's goal now.
    pub fn of(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Self {
        let mut need = Self::default();
        if !crate::ai::tall_real::science_game(obs, rules) {
            return need;
        }
        let parts = parts_left(obs, rules);
        if parts.is_empty() {
            return need;
        }
        let builders = builders(obs, parts.len());
        let mut at = 0;
        for resource in goal_resources(rules) {
            if at == SUPPLY_SLOTS || !revealed(obs, rules, resource) {
                continue;
            }
            let wanted = copies_wanted(rules, &parts, &builders, resource);
            let short = wanted - obs.economy.resources.available(resource).max(0);
            if short > 0 {
                need.slots[at] = Shortage { resource, copies: short, hammers: hammers_per_copy(rules, &parts, resource) };
                at += 1;
            }
        }
        need
    }

    /// The need with the strategist's multiplier.
    pub fn priced(self, mult: Milli) -> Self {
        Self { mult, ..self }
    }

    /// Copies of `resource` the goal lacks (0: none).
    pub fn short(&self, resource: ResourceId) -> i32 {
        self.slots.iter().find(|s| s.copies > 0 && s.resource == resource).map_or(0, |s| s.copies)
    }

    pub fn any(&self) -> bool {
        self.slots.iter().any(|s| s.copies > 0)
    }

    /// The hammers `copies` more copies of `resource` let the goal build (none beyond the shortage),
    /// at the strategist's multiplier: one-off production each channel prices at its own rate.
    pub fn hammers(&self, resource: ResourceId, copies: i32) -> i64 {
        let Some(slot) = self.slots.iter().find(|s| s.copies > 0 && s.resource == resource) else { return 0 };
        i64::from(copies.clamp(0, slot.copies)) * slot.hammers * self.mult / crate::ai::strategy_net::layers::ONE
    }

    /// `hammers` in centi-points at the turn's prices: production now over the rounds left (the
    /// price of production is a hammer a round kept to the end).
    pub fn worth(&self, resource: ResourceId, copies: i32, prices: &crate::ai::value::Prices) -> crate::ai::value::Centi {
        self.hammers(resource, copies) * prices.production / i64::from(prices.rounds_left.max(1))
    }

    /// The resources short whose copies `other` could give in a deal now (the trade screen's).
    pub fn offered_by(&self, obs: &AIObservation<'_>, other: PlayerId) -> Vec<(ResourceId, i32)> {
        self.slots
            .iter()
            .filter(|s| s.copies > 0)
            .map(|s| (s.resource, obs.view.tradeable_copies(other, s.resource).min(s.copies)))
            .filter(|(_, n)| *n > 0)
            .collect()
    }
}

/// The need as a settler weighs it (`planner_settle::SiteContext::supply`): the shortages and the
/// rounds left of the turn's prices, over which a copy's hammers count a round.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SiteSupply {
    pub need: SupplyNeed,
    pub rounds_left: u32,
}

impl SiteSupply {
    pub fn of(prices: &crate::ai::value::Prices) -> Self {
        Self { need: prices.supply, rounds_left: prices.rounds_left }
    }
}

/// One part still missing: its kind's definition.
struct Part<'r> {
    def: &'r crate::rules::compiled::UnitDefinition,
}

/// Every part the ship still lacks (installed and walking parts are not missing; a queued one is: it
/// takes its copies when done), whatever its technology: the goal's horizon.
fn parts_left<'r>(obs: &AIObservation<'_>, rules: &'r CompiledRuleset) -> Vec<Part<'r>> {
    let mut out = Vec::new();
    for (kind, project, needed) in crate::ai::victory::part_kinds(rules) {
        let Some(def) = rules.units.get(kind.0 as usize) else { continue };
        let done = usize::from(obs.economy.projects.get(project.0 as usize).copied().unwrap_or(0));
        let walking = obs.own_units.iter().filter(|u| u.kind == kind).count();
        let left = usize::from(needed).saturating_sub(done + walking);
        out.extend(std::iter::repeat_n((), left).map(|_| Part { def }));
    }
    // The dearest first: they go to the strongest builders (`victory::space_plan`).
    out.sort_by_key(|p| std::cmp::Reverse(p.def.cost));
    out
}

/// The cities that will build the parts: the most productive, one a part.
fn builders<'o>(obs: &'o AIObservation<'_>, parts: usize) -> Vec<&'o crate::knowledge::observation::OwnCity> {
    let mut cities: Vec<_> = obs.own_cities.iter().collect();
    cities.sort_by_key(|c| (std::cmp::Reverse(c.income.production), c.id));
    cities.truncate(parts);
    cities
}

/// The non-wonder buildings with a production percent toward the parts.
fn speeders(rules: &CompiledRuleset) -> impl Iterator<Item = (usize, &crate::rules::compiled::BuildingDefinition)> {
    rules.buildings.iter().enumerate().filter(|(_, b)| !b.wonder).filter(|(_, b)| {
        b.effects.passive_city.iter().any(|e| matches!(e.condition, Condition::Producing(ProductionFilter::SpaceshipParts)))
    })
}

/// The strategic resources the goal's items need, ascending, once each.
fn goal_resources(rules: &CompiledRuleset) -> Vec<ResourceId> {
    let parts = crate::ai::victory::part_kinds(rules).into_iter().filter_map(|(k, _, _)| rules.units.get(k.0 as usize));
    let mut out: Vec<ResourceId> = parts.flat_map(|d| d.required_resources.iter().map(|(r, _)| *r)).collect();
    out.extend(speeders(rules).flat_map(|(_, b)| b.consumes_resources.iter().map(|(r, _)| *r)));
    out.retain(|r| rules.resources.get(r.0 as usize).is_some_and(|d| d.class == Some(ResourceClass::Strategic)));
    out.sort_by_key(|r| r.0);
    out.dedup();
    out
}

/// The player knows the technology that reveals `resource` (it sees the deposits and may trade copies).
fn revealed(obs: &AIObservation<'_>, rules: &CompiledRuleset, resource: ResourceId) -> bool {
    let Some(def) = rules.resources.get(resource.0 as usize) else { return false };
    def.required_tech.is_none_or(|t| obs.economy.techs.contains(&t))
}

/// Copies of `resource` the builders hold at once: each its part's, and its speeder's where it has none.
fn copies_wanted(rules: &CompiledRuleset, parts: &[Part<'_>], builders: &[&crate::knowledge::observation::OwnCity], resource: ResourceId) -> i32 {
    let of = |list: &[(ResourceId, u16)]| list.iter().find(|(r, _)| *r == resource).map_or(0, |(_, n)| i32::from(*n));
    let mut wanted = 0;
    for (part, city) in parts.iter().zip(builders) {
        wanted += of(&part.def.required_resources);
        let has_speeder = speeders(rules).any(|(index, _)| city.buildings.iter().any(|b| usize::from(b.0) == index));
        if !has_speeder {
            wanted += speeders(rules).map(|(_, b)| of(&b.consumes_resources)).max().unwrap_or(0);
        }
    }
    wanted
}

/// The hammers one copy of `resource` holds back: the dearest missing part that needs it, its cost
/// over its copies; a resource only the speeders need is worth their cost over theirs.
fn hammers_per_copy(rules: &CompiledRuleset, parts: &[Part<'_>], resource: ResourceId) -> i64 {
    let per_copy = |cost: u32, list: &[(ResourceId, u16)]| {
        list.iter().find(|(r, _)| *r == resource).map(|(_, n)| i64::from(cost) / i64::from((*n).max(1)))
    };
    let part = parts.iter().filter_map(|p| per_copy(p.def.cost, &p.def.required_resources)).max();
    part.or_else(|| speeders(rules).filter_map(|(_, b)| per_copy(b.cost, &b.consumes_resources)).max()).unwrap_or(0)
}

/// Copies a deposit of `resource` gives (the rules' least quantity; the view shows no amount).
pub fn deposit_copies(rules: &CompiledRuleset, resource: ResourceId) -> i32 {
    rules.resources.get(resource.0 as usize).and_then(|d| d.quantities.iter().min()).map_or(1, |q| i32::from(*q).max(1))
}

/// The release of copies a part waits for: while every prerequisite project of the parts is done (a
/// part may start now), no part can start for want of free copies and the empire is at war with no
/// nation, own units that are no part, stand outside a city (a garrison stays) and hold such a copy are
/// disbanded, the cheapest first, until one part can start.
pub fn release_orders(obs: &AIObservation<'_>, rules: &CompiledRuleset, need: &SupplyNeed) -> Vec<Command> {
    let projects = crate::ai::victory::prerequisite_projects(rules);
    let open = !projects.is_empty() && projects.iter().all(|p| obs.economy.projects.get(p.0 as usize).copied().unwrap_or(0) > 0);
    let at_war = obs.relations.iter().any(|r| r.at_war && r.kind == crate::world::player::PlayerKind::Major);
    if !need.any() || !open || at_war {
        return Vec::new();
    }
    let mut out = Vec::new();
    for slot in need.slots.iter().filter(|s| s.copies > 0) {
        let part_copies = crate::ai::victory::part_kinds(rules)
            .into_iter()
            .filter_map(|(kind, _, _)| rules.units.get(kind.0 as usize))
            .filter_map(|d| d.required_resources.iter().find(|(r, _)| *r == slot.resource).map(|(_, n)| i32::from(*n)))
            .min();
        let Some(part_copies) = part_copies else { continue };
        let blocked = part_copies - obs.economy.resources.available(slot.resource);
        let mut holders: Vec<(u32, crate::ids::EntityId, UnitHandle, i32)> = obs
            .own_units
            .iter()
            .filter(|u| u.moves_left > 0 && !obs.own_cities.iter().any(|c| c.tile == u.tile))
            .filter_map(|u| {
                let def = rules.units.get(u.kind.0 as usize)?;
                let (_, held) = def.required_resources.iter().find(|(r, _)| *r == slot.resource)?;
                (def.spaceship_project.is_none()).then_some((def.cost, u.id, u.handle, i32::from(*held)))
            })
            .collect();
        holders.sort_by_key(|(cost, id, _, _)| (*cost, *id));
        let mut freed = 0;
        for (_, _, unit, held) in holders {
            if freed >= blocked {
                break;
            }
            let order = Command::DisbandUnit { unit };
            if obs.view.accepts(&order) {
                out.push(order);
                freed += held;
            }
        }
    }
    out
}
