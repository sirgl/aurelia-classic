//! Resources a player has (resource counts: total, used, available).
//!
//! ```text
//! total      copies on the tiles of the player's cities that the player
//!            knows (reveal technology) and has connected: the city stands
//!            on the tile or the tile has one of the resource's
//!            improvements. A strategic deposit gives its amount (map
//!            generation: one of the resource's quantities), any other
//!            resource one copy; the amount × (100 + ResourceQuantityPercent
//!            of the resource) %, then a strategic one × (100 +
//!            StrategicQuantityPercent) % (truncated per tile); a luxury tile
//!            of a city with ExtraLuxuryCopies (Bazaar) gives that many more
//!            + ResourceCopies of the player's buildings (Recycling Center)
//!            + one copy of each luxury a city-state has connected, for the
//!              player whose nation owns a siphoning improvement on
//!              its land (Feitoria); these copies cannot be traded
//!            + copies other players give by deals (imports)
//!            + the luxuries and strategics of allied city-states (§4.7;
//!            twice as many with `AllyResourceBonus`, Cultural Diplomacy)
//! used       units alive that require it
//!            + buildings standing that consume it
//!            + copies given to other players by deals (exports)
//!            + a city-state's luxuries and strategics its ally holds
//! available  total - used; below zero is a deficit
//! tradeable  available - the untradeable copies (Feitoria)
//! ```
//!
//! Production (`production_resources`): starting or buying an item needs
//! `available >= required`; a city completes it only while that holds
//! (the order is dropped otherwise). A unit that
//! requires a resource in deficit fights worse (`strategic_combat_penalty`,
//! rules.md). Luxuries count once each for happiness while
//! available (`derived::happiness`).

use crate::world::city::MAX_TERRITORY_RADIUS;
use crate::derived::tile_yields::{resource_revealed, TechSet};
use crate::ids::{CityHandle, PlayerId, ProductionItem, ResourceId, TileId, UnitKindId};
use crate::math::hex_disc;
use crate::rules::content::ResourceClass;
use crate::rules::stats::{Stat, StatTotals};
use crate::rules::CompiledRuleset;
use crate::world::state::AuthWorld;
use crate::world::player::ClassicPlayers;

/// Combat penalty (percent) of a unit whose owner is short of a strategic
/// resource the unit requires (flat, whatever the shortfall). Unciv:
/// `battle/BattleConstants.kt` (`MISSING_RESOURCES_MALUS`, -25), applied in
/// `BattleDamage.kt`; equal.
pub const MISSING_RESOURCES_MALUS: i32 = -25;

/// Totals and uses of every resource of one player (index = `ResourceId`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResourceLedger {
    total: Vec<i32>,
    used: Vec<i32>,
    /// Copies in `total` the player may not give away (Feitoria).
    untradeable: Vec<i32>,
}

impl ResourceLedger {
    pub fn total(&self, r: ResourceId) -> i32 {
        self.total.get(r.0 as usize).copied().unwrap_or(0)
    }

    pub fn used(&self, r: ResourceId) -> i32 {
        self.used.get(r.0 as usize).copied().unwrap_or(0)
    }

    /// Copies free for new units and buildings (negative: a deficit).
    pub fn available(&self, r: ResourceId) -> i32 {
        self.total(r) - self.used(r)
    }

    /// Copies the player may give by a deal.
    pub fn tradeable(&self, r: ResourceId) -> i32 {
        self.available(r) - self.untradeable.get(r.0 as usize).copied().unwrap_or(0)
    }

    /// Resources with a total or a use, ascending.
    pub fn known(&self) -> impl Iterator<Item = ResourceId> + '_ {
        (0..self.total.len())
            .map(|i| ResourceId(i as u16))
            .filter(|r| self.total(*r) != 0 || self.used(*r) != 0)
    }

    /// Resources in deficit, ascending.
    pub fn deficits(&self) -> impl Iterator<Item = ResourceId> + '_ {
        self.known().filter(|r| self.available(*r) < 0)
    }
}

/// The player's ledger now.
pub fn resource_ledger(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId) -> ResourceLedger {
    let stats = &crate::effects::player::player_modifiers(auth, rules, player).stats;
    let mut total = connected_resources(auth, rules, player, &stats);
    add_building_copies(rules, &stats, &mut total);
    let untradeable = city_state_luxuries(auth, rules, player);
    for (t, extra) in total.iter_mut().zip(&untradeable) {
        *t += extra;
    }
    let mut ledger = ResourceLedger {
        total,
        used: used_resources(auth, rules, player),
        untradeable,
    };
    add_deals(auth, player, &mut ledger);
    add_city_state_alliances(auth, rules, player, &mut ledger);
    ledger
}

/// Percent of a city-state's resources that an ally with Cultural Diplomacy
/// receives. Own value, unverified.
pub const ALLY_RESOURCE_BONUS_PERCENT: u32 = 200;

/// Luxuries and strategics of allied city-states (§4.7): the ally gets the
/// city-state's own copies (×2 with `AllyResourceBonus`), the city-state
/// holds them as exported.
fn add_city_state_alliances(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    player: PlayerId,
    ledger: &mut ResourceLedger,
) {
    if auth.players().is_city_state(player) {
        if auth.module().city_states().ally(player).is_some() {
            for (slot, n) in shared_resources(auth, rules, player).iter().enumerate() {
                ledger.used[slot] += i32::from(*n);
            }
        }
        return;
    }
    for (slot, n) in from_city_states(auth, rules, player).iter().enumerate() {
        ledger.total[slot] += i32::from(*n);
    }
}

/// Copies of each resource (index = `ResourceId`) `major`'s allied
/// city-states give it.
pub fn from_city_states(auth: &AuthWorld, rules: &CompiledRuleset, major: PlayerId) -> Vec<u16> {
    let mut counts = vec![0u16; rules.resources.len()];
    let allies = crate::systems::city_states::allies_of(auth, major);
    if allies.is_empty() {
        return counts;
    }
    let bonus = crate::systems::city_states::stat(
        auth,
        rules,
        major,
        crate::classic::effects::CityStateStat::AllyResourceBonus,
    ) > 0;
    let percent = if bonus { ALLY_RESOURCE_BONUS_PERCENT } else { 100 };
    for ally in allies {
        for (slot, n) in shared_resources(auth, rules, ally).iter().enumerate() {
            let given = u32::from(*n) * percent / 100;
            counts[slot] = counts[slot].saturating_add(given.min(u32::from(u16::MAX)) as u16);
        }
    }
    counts
}

/// A city-state's own luxuries and strategics (what its ally receives).
fn shared_resources(auth: &AuthWorld, rules: &CompiledRuleset, city_state: PlayerId) -> Vec<u16> {
    let stats = &crate::effects::player::player_modifiers(auth, rules, city_state).stats;
    let counts = connected_resources(auth, rules, city_state, &stats);
    let mut out = vec![0u16; counts.len()];
    for ((slot, count), def) in out.iter_mut().zip(&counts).zip(&rules.resources) {
        let shared = matches!(def.class, Some(ResourceClass::Luxury | ResourceClass::Strategic));
        if shared {
            *slot = (*count).clamp(0, i32::from(u16::MAX)) as u16;
        }
    }
    out
}

/// Resources traded by deals now (`world::relations`): imports count as
/// the receiver's copies, exports are held by the giver.
fn add_deals(auth: &AuthWorld, player: PlayerId, ledger: &mut ResourceLedger) {
    let round = auth.turn().round;
    for trade in auth.module().relations().trades() {
        let crate::world::relations::TradeFlow::Resource { resource, amount } = trade.flow else {
            continue;
        };
        if round >= trade.until {
            continue;
        }
        let slot = resource.0 as usize;
        if trade.to == player {
            if let Some(t) = ledger.total.get_mut(slot) {
                *t += i32::from(amount);
            }
        }
        if trade.from == player {
            if let Some(u) = ledger.used.get_mut(slot) {
                *u += i32::from(amount);
            }
        }
    }
}

/// `ResourceCopies` of the player's buildings.
fn add_building_copies(rules: &CompiledRuleset, stats: &StatTotals, total: &mut [i32]) {
    for (stat, amount) in stats.iter() {
        if let Stat::ResourceCopies(r) = stat {
            if let Some(t) = total.get_mut(r.0 as usize) {
                *t += amount;
            }
        }
    }
    debug_assert_eq!(total.len(), rules.resources.len());
}

/// Copies of each resource on the player's connected tiles, with the
/// quantity stats of `stats` (index = `ResourceId`).
fn connected_resources(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    player: PlayerId,
    stats: &StatTotals,
) -> Vec<i32> {
    let mut counts = vec![0i32; rules.resources.len()];
    let techs = TechSet::of_player(auth, player);
    for handle in auth.indexes().cities_of(player) {
        let Some(core) = auth.cities().get(*handle) else {
            continue;
        };
        let mut extra_luxury: Option<i32> = None;
        for (tile, resource) in city_connected(auth, rules, &techs, *handle) {
            let def = &rules.resources[resource.0 as usize];
            let mut copies = deposit_copies(auth.map().resource_amount(tile), resource, def.class, stats);
            if def.class == Some(ResourceClass::Luxury) {
                // Evaluated once per city, and only for cities with a luxury.
                let extra = *extra_luxury.get_or_insert_with(|| {
                    crate::effects::city::city_stat(auth, rules, core, Stat::ExtraLuxuryCopies)
                });
                copies += extra.max(0);
            }
            counts[resource.0 as usize] += copies;
        }
    }
    counts
}

/// Copies one connected tile gives (see the module docs).
fn deposit_copies(amount: u8, resource: ResourceId, class: Option<ResourceClass>, stats: &StatTotals) -> i32 {
    let mut copies = i32::from(amount);
    copies = copies * (100 + stats.get(Stat::ResourceQuantityPercent(resource))).max(0) / 100;
    if class == Some(ResourceClass::Strategic) {
        copies = copies * (100 + stats.get(Stat::StrategicQuantityPercent)).max(0) / 100;
    }
    copies
}

/// The connected resource tiles of one city.
fn city_connected(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    techs: &TechSet,
    city: CityHandle,
) -> Vec<(TileId, ResourceId)> {
    let Some(core) = auth.cities().get(city) else {
        return Vec::new();
    };
    let map = auth.map();
    hex_disc(map.dimensions, core.tile, MAX_TERRITORY_RADIUS)
        .into_iter()
        .filter(|t| map.owning_city(*t) == Some(city))
        .filter_map(|t| connected_on(auth, rules, techs, t, t == core.tile).map(|r| (t, r)))
        .collect()
}

/// Resources connected on the tiles of one city (trade
/// routes compare them).
pub fn city_resources(auth: &AuthWorld, rules: &CompiledRuleset, city: CityHandle) -> Vec<ResourceId> {
    let Some(core) = auth.cities().get(city) else {
        return Vec::new();
    };
    let techs = TechSet::of_player(auth, core.owner);
    city_connected(auth, rules, &techs, city).into_iter().map(|(_, r)| r).collect()
}

/// One copy of each luxury connected by a city-state on whose land an
/// improvement of `player`'s nation siphons luxuries (Feitoria: the
/// improvement is unique to one nation, so its nation is its
/// builder's). Index = `ResourceId`; empty of copies for everybody else.
fn city_state_luxuries(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId) -> Vec<i32> {
    let mut copies = vec![0i32; rules.resources.len()];
    let nation = auth.players().nation_of(player);
    let siphons: Vec<(crate::ids::ImprovementId, i32)> = rules
        .improvements
        .iter()
        .enumerate()
        .filter(|(_, d)| d.module.city_state_luxury_copies > 0 && d.nation == Some(nation))
        .map(|(i, d)| (crate::ids::ImprovementId(i as u16), i32::from(d.module.city_state_luxury_copies)))
        .collect();
    if siphons.is_empty() || !auth.players().is_major(player) {
        return copies;
    }
    for p in 0..auth.players().count() {
        let host = PlayerId(p as u8);
        if !auth.players().is_city_state(host) {
            continue;
        }
        let per_luxury = siphon_on_land(auth, host, &siphons);
        if per_luxury == 0 {
            continue;
        }
        let host_copies = connected_resources(auth, rules, host, &StatTotals::default());
        for (i, def) in rules.resources.iter().enumerate() {
            if def.class == Some(ResourceClass::Luxury) && host_copies[i] > 0 {
                copies[i] += per_luxury;
            }
        }
    }
    copies
}

/// The largest siphon of the improvements in `siphons` standing on
/// `host`'s land (0: none).
fn siphon_on_land(auth: &AuthWorld, host: PlayerId, siphons: &[(crate::ids::ImprovementId, i32)]) -> i32 {
    let map = auth.map();
    let mut best = 0;
    for handle in auth.indexes().cities_of(host) {
        let Some(core) = auth.cities().get(*handle) else {
            continue;
        };
        for tile in hex_disc(map.dimensions, core.tile, MAX_TERRITORY_RADIUS) {
            if map.owning_city(tile) != Some(*handle) {
                continue;
            }
            let here = map.improvement(tile).flatten();
            for (improvement, amount) in siphons {
                if here == Some(*improvement) {
                    best = best.max(*amount);
                }
            }
        }
    }
    best
}

/// The resource a tile gives its owner, if known and connected.
fn connected_on(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    techs: &TechSet,
    tile: TileId,
    city_tile: bool,
) -> Option<ResourceId> {
    let resource = auth.map().resource(tile).flatten()?;
    if !resource_revealed(rules, resource, techs) {
        return None;
    }
    let def = rules.resources.get(resource.0 as usize)?;
    let improvement = auth.map().working_improvement(tile);
    let improved = improvement.is_some_and(|i| def.improvements.contains(&i));
    (city_tile || improved).then_some(resource)
}

/// Copies the player's units and buildings hold.
fn used_resources(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId) -> Vec<i32> {
    let mut used = vec![0i32; rules.resources.len()];
    let mut add = |needs: &[(ResourceId, u16)]| {
        for (r, n) in needs {
            if let Some(slot) = used.get_mut(r.0 as usize) {
                *slot += i32::from(*n);
            }
        }
    };
    for unit in auth.indexes().units_of(player) {
        if let Some(view) = auth.units().get(*unit) {
            add(unit_requirements(rules, view.core.kind));
        }
    }
    for city in auth.indexes().cities_of(player) {
        let Some(core) = auth.cities().get(*city) else {
            continue;
        };
        for b in &core.buildings {
            if let Some(def) = rules.buildings.get(b.0 as usize) {
                add(&def.consumes_resources);
            }
        }
    }
    used
}

fn unit_requirements(rules: &CompiledRuleset, kind: UnitKindId) -> &[(ResourceId, u16)] {
    rules
        .units
        .get(kind.0 as usize)
        .map(|u| u.required_resources.as_slice())
        .unwrap_or(&[])
}

/// What producing `item` takes (copies per resource).
pub fn item_requirements(rules: &CompiledRuleset, item: ProductionItem) -> &[(ResourceId, u16)] {
    match item {
        ProductionItem::Unit(kind) => unit_requirements(rules, kind),
        ProductionItem::Building(b) => rules
            .buildings
            .get(b.0 as usize)
            .map(|d| d.consumes_resources.as_slice())
            .unwrap_or(&[]),
        // Projects need no resources in the compiled rules.
        ProductionItem::Project(_) => &[],
    }
}

/// Err(the first resource short): the player cannot start, buy or
/// complete `item` now.
pub fn production_resources(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    player: PlayerId,
    item: ProductionItem,
) -> Result<(), ResourceId> {
    let needs = item_requirements(rules, item);
    if needs.is_empty() {
        return Ok(());
    }
    let ledger = resource_ledger(auth, rules, player);
    match needs.iter().find(|(r, n)| ledger.available(*r) < i32::from(*n)) {
        Some((r, _)) => Err(*r),
        None => Ok(()),
    }
}

/// Combat percent of a unit of `kind` whose owner lacks a resource it
/// requires: `MISSING_RESOURCES_MALUS` once if any required resource is in
/// deficit, else 0 (Unciv's flat penalty).
pub fn strategic_combat_penalty(ledger: &ResourceLedger, rules: &CompiledRuleset, kind: UnitKindId) -> i32 {
    let short = unit_requirements(rules, kind)
        .iter()
        .any(|(r, _)| ledger.used(*r) > ledger.total(*r));
    if short {
        MISSING_RESOURCES_MALUS
    } else {
        0
    }
}

/// The penalty of one unit of the player (0 for a unit that requires
/// nothing: the ledger is not built; 0 for barbarians, as Unciv).
pub fn unit_resource_penalty(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId, kind: UnitKindId) -> i32 {
    if unit_requirements(rules, kind).is_empty() || auth.players().is_barbarian(player) {
        return 0;
    }
    strategic_combat_penalty(&resource_ledger(auth, rules, player), rules, kind)
}

pub use kernel::scenario::deposit_amount;
