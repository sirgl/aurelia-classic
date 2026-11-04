//! Trade route rules (classic/docs/classic/rules.md): how many
//! routes a player may run, their range and path, the checks of a new
//! route and what it yields at both ends. Own design (Unciv has no trade
//! units); the tuning is the block of named constants and small functions
//! below, documented with its reasoning in `classic/docs/classic/BNW-DESIGN.md`.
//!
//! ```text
//! routes possible  (Σ Trade(Routes) of the player (technologies, wonders)
//!                  × (100 + Trade(RoutesPercent)) / 100) + cities / 4
//! routes used      trade units alive + routes running + trade units queued
//! range            (12 land | 18 sea + era / 2 + Trade(RangeTiles))
//!                  × (100 + city Trade(RangePercent)) / 100
//! path             cheapest path by step cost (plain 100, rough 115, road
//!                  or rail 60; sea: coastal 100, open water 120, +500 in
//!                  the water of an enemy), valid while cost ≤ range × 100
//! circuits         clamp(24 × speed / (2 L), 2, 8), speed 3 land, 5 sea
//! international    origin gold ((80 + 25 era) + 18 % √(gold of both cities)
//!                  + 25 per differing luxury or strategic resource (≤ 5)
//!                  + effect gold) × (100 + 3 min(L, 15)) % × 100 (land) |
//!                  150 (sea) %; destination (25 + 10 era of the recipient
//!                  + half the city term + effect gold) × the same percents;
//!                  science 0.3 per technology the partner knows, ≤ 4.5
//! internal         destination food or production (200 + 45 era of its
//!                  owner + 15 per citizen) × (100 + 60 sea +
//!                  Trade(InternalPercent)) / 100
//! plunder          (60 + 10 era of the route's owner) × 100 | 150 (sea)
//! ```
//!
//! All values are hundredths; the origin's science adds the cultural
//! influence bonus (track B). Note: the path is always the cheapest one (a
//! path search with an inadmissible estimate could return a longer one).

use crate::classic::effects::ClassicCount;
use crate::ids::{CityHandle, PlayerId, ResourceId, TileId, UnitHandle};
use crate::rules::content::{BuildingAbility, Domain, ResourceClass};
use crate::rules::effects::{CompiledEffect, Count, Modifier, Selector};
use crate::classic::effects::CityStateStat;
use crate::rules::stats::{Stat, TradeDomain, TradeStat};
use crate::rules::CompiledRuleset;
use crate::world::city::{CityCore, CityStatus};
use crate::world::state::AuthWorld;
use crate::world::trade::{RouteKind, RouteYields, TradeRoute};
use crate::world::player::ClassicPlayers;

// ---- tuning: the named constants of the trade route system (own design) ----

/// Range of a land route in tiles before modifiers. Own design (Unciv has no trade units).
pub const LAND_RANGE: i32 = 12;
/// Range of a sea route in tiles before modifiers. Own design (Unciv has no trade units).
pub const SEA_RANGE: i32 = 18;
/// Eras per extra tile of range: traders reach further as roads and charts improve (+1 every 2 eras).
/// Own design (no Unciv equivalent).
pub const ERAS_PER_RANGE_TILE: u8 = 2;
/// Cities per extra route slot on top of the technology slots: a larger empire fields more traders.
/// Own design (no Unciv equivalent).
pub const CITIES_PER_ROUTE_SLOT: usize = 4;
/// Tiles a caravan moves along its path in a turn. Own design (Unciv has no trade units).
pub const LAND_SPEED: u32 = 3;
/// Tiles a cargo ship moves along its path in a turn. Own design (Unciv has no trade units).
pub const SEA_SPEED: u32 = 5;
/// Target length of a route in turns; the circuits are the whole number that comes closest. Own design.
pub const TARGET_TURNS: u32 = 24;
/// Fewest circuits of one route (a very short route would turn the unit over every few turns, a very long one
/// would tie a slot up for good, hence also a most). Own design.
pub const MIN_CIRCUITS: u32 = 2;
/// Most circuits of one route. Own design, see [`MIN_CIRCUITS`].
pub const MAX_CIRCUITS: u32 = 8;
/// Path cost of an ordinary land step (also the scale of the range budget). Own design.
pub const STEP_COST: u32 = 100;
/// Path cost of a step between two tiles with a road or railroad. Own design.
pub const ROAD_STEP_COST: u32 = 60;
/// Path cost of a step into rough ground (hills, forest, jungle, ice). Own design.
pub const ROUGH_STEP_COST: u32 = 115;
/// Path cost of a sea step away from the coast. Own design.
pub const OPEN_SEA_STEP_COST: u32 = 120;
/// Extra path cost of water owned by an enemy (the path stays valid but long). Own design.
pub const ENEMY_WATER_COST: u32 = 500;
/// Base gold of an international route in the first era, hundredths. Own design: a route is worth a small
/// building of its era, so the base grows linearly with the eras.
pub const INTERNATIONAL_BASE: u32 = 80;
/// Gold added to the base of an international route per era of its sender, hundredths. Own design.
pub const INTERNATIONAL_PER_ERA: u32 = 25;
/// Base gold of the recipient end in the first era, hundredths. Own design: the recipient earns
/// about half of what the sender does, so sending is the better deal but receiving is never worthless.
pub const RECIPIENT_BASE: u32 = 25;
/// Gold added to the recipient base per era of the recipient, hundredths. Own design.
pub const RECIPIENT_PER_ERA: u32 = 10;
/// Percent of the geometric mean of the two cities' gold income (hundredths of gold per turn) a route
/// adds. Own design: a rich pair of cities pays more, a poor pair is carried by the base, and the
/// square root keeps one huge city from making a route worth more than the empire's own taxes.
pub const CITY_GOLD_PERCENT: u64 = 18;
/// Gold per luxury or strategic resource only one of the cities has, hundredths. Own design.
pub const RESOURCE_GOLD: u32 = 25;
/// Most differing resources that pay. Own design, see [`RESOURCE_GOLD`].
pub const RESOURCE_GOLD_MAX: usize = 5;
/// Percent added to the gold of a route per tile of its path: a far partner pays more, up to +45 %. Own design.
pub const DISTANCE_PERCENT_PER_TILE: u32 = 3;
/// Path length after which the distance bonus stops growing. Own design, see [`DISTANCE_PERCENT_PER_TILE`].
pub const DISTANCE_PERCENT_MAX_TILES: u32 = 15;
/// Percent of the gold a sea route yields (cargo ships cost more and need two coasts). Own design.
pub const SEA_GOLD_PERCENT: u32 = 150;
/// Science per technology the partner knows and the receiver lacks, hundredths. Own design.
pub const SCIENCE_PER_TECH: u32 = 30;
/// Most science one end of a route gets, hundredths. Own design, see [`SCIENCE_PER_TECH`].
pub const SCIENCE_MAX: u32 = 450;
/// Food or production of an internal route at the start, hundredths. Own design: a route feeds a growing
/// city a visible share of its yield.
pub const INTERNAL_BASE: u32 = 200;
/// Food or production added to an internal route per era of the destination's owner, hundredths. Own design.
pub const INTERNAL_PER_ERA: u32 = 45;
/// Food or production added to an internal route per citizen of the destination, hundredths. Own design.
pub const INTERNAL_PER_CITIZEN: u32 = 15;
/// Percent a sea route adds to the food or production of an internal route. Own design.
pub const INTERNAL_SEA_PERCENT: i32 = 60;
/// Whole gold a plunderer takes from a route in the first era (×150 % at sea). Own design.
pub const PLUNDER_BASE: u32 = 60;
/// Whole gold added to the plunder per era of the route's owner. Own design.
pub const PLUNDER_PER_ERA: u32 = 10;

/// Gold (hundredths) the two cities' own incomes (`g`: hundredths of gold per turn) add to a route:
/// `18 % × √(g_origin × g_destination)`. Own design; geometric mean, zero if either city has no gold.
pub fn city_gold_term(origin: u32, destination: u32) -> u32 {
    (u64::from(origin) * u64::from(destination)).isqrt().saturating_mul(CITY_GOLD_PERCENT) as u32 / 100
}

/// Percent of the path-length factor: `100 + 3 × min(length, 15)`. Own design.
pub fn distance_percent(length: u32) -> u32 {
    100 + DISTANCE_PERCENT_PER_TILE * length.min(DISTANCE_PERCENT_MAX_TILES)
}

/// Percent of the gold of a route of the domain (land 100, sea 150). Own design.
pub fn domain_percent(domain: TradeDomain) -> u32 {
    match domain {
        TradeDomain::Land => 100,
        TradeDomain::Sea => SEA_GOLD_PERCENT,
    }
}

/// Gold (hundredths) of `differing` luxury or strategic resources only one end has. Own design.
pub fn resource_gold_for(differing: usize) -> u32 {
    differing.min(RESOURCE_GOLD_MAX) as u32 * RESOURCE_GOLD
}

/// Science (hundredths) a route brings from a partner that knows `gap` technologies the receiver lacks.
pub fn science_for(gap: u32) -> u32 {
    (SCIENCE_PER_TECH * gap).min(SCIENCE_MAX)
}

/// Food or production (hundredths) of an internal route into a city of `citizens` in `era`, before the
/// percents. Own design.
pub fn internal_amount(era: u32, citizens: u32) -> u32 {
    INTERNAL_BASE + INTERNAL_PER_ERA * era + INTERNAL_PER_CITIZEN * citizens
}

/// Gold a plunderer takes from a route of an owner in `era`. Own design.
pub fn plunder_gold(era: u32, domain: TradeDomain) -> u32 {
    (PLUNDER_BASE + PLUNDER_PER_ERA * era) * domain_percent(domain) / 100
}

/// Why a route cannot be made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TradeBlock {
    NotATradeUnit,
    NotInOwnCity,
    UnknownCity,
    SameCity,
    Unexplored,
    OwnCityForInternational,
    ForeignCityForInternal,
    NoGranary,
    NoWorkshop,
    NotMet,
    AtWar,
    Razing,
    RouteExists,
    NotCoastal,
    OutOfRange,
    NoRouteLeft,
    NoRouteHere,
    NotAtWar,
    /// A World Congress embargo forbids the route.
    Embargo,
}

impl TradeBlock {
    pub const ALL: [TradeBlock; 19] = [
        TradeBlock::NotATradeUnit,
        TradeBlock::NotInOwnCity,
        TradeBlock::UnknownCity,
        TradeBlock::SameCity,
        TradeBlock::Unexplored,
        TradeBlock::OwnCityForInternational,
        TradeBlock::ForeignCityForInternal,
        TradeBlock::NoGranary,
        TradeBlock::NoWorkshop,
        TradeBlock::NotMet,
        TradeBlock::AtWar,
        TradeBlock::Razing,
        TradeBlock::RouteExists,
        TradeBlock::NotCoastal,
        TradeBlock::OutOfRange,
        TradeBlock::NoRouteLeft,
        TradeBlock::NoRouteHere,
        TradeBlock::NotAtWar,
        TradeBlock::Embargo,
    ];

    pub fn reason(self) -> &'static str {
        match self {
            TradeBlock::NotATradeUnit => "not a trade unit",
            TradeBlock::NotInOwnCity => "the trade unit is not in an own city",
            TradeBlock::UnknownCity => "unknown city",
            TradeBlock::SameCity => "a city does not trade with itself",
            TradeBlock::Unexplored => "the destination is not explored",
            TradeBlock::OwnCityForInternational => "an international route needs another nation's city",
            TradeBlock::ForeignCityForInternal => "food and production go to own cities",
            TradeBlock::NoGranary => "food routes need a Granary in the origin",
            TradeBlock::NoWorkshop => "production routes need a Workshop in the origin",
            TradeBlock::NotMet => "not met",
            TradeBlock::AtWar => "at war",
            TradeBlock::Razing => "a city burns",
            TradeBlock::RouteExists => "a route between these cities exists",
            TradeBlock::NotCoastal => "cargo ships need two coastal cities",
            TradeBlock::OutOfRange => "out of range",
            TradeBlock::NoRouteLeft => "no trade route left",
            TradeBlock::NoRouteHere => "no trade unit of a route here",
            TradeBlock::NotAtWar => "not at war with the route's owner",
            TradeBlock::Embargo => "the World Congress forbids this trade",
        }
    }
}

/// A route the rules allow, before it starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutePlan {
    pub origin: CityHandle,
    pub destination: CityHandle,
    pub domain: TradeDomain,
    pub path: Vec<TileId>,
    pub circuits: u16,
}

fn player_stat(auth: &AuthWorld, rules: &CompiledRuleset, p: PlayerId, stat: TradeStat) -> i32 {
    crate::effects::player::player_modifiers(auth, rules, p).stats.get(Stat::Module(crate::rules::stats::ClassicStat::Trade(stat)))
}

fn city_stat(auth: &AuthWorld, rules: &CompiledRuleset, core: &CityCore, stat: TradeStat) -> i32 {
    crate::effects::city::city_modifiers(auth, rules, core).stats.get(Stat::Module(crate::rules::stats::ClassicStat::Trade(stat)))
}

/// Routes the player may run: the technology and wonder slots with the player's percent, plus one
/// slot per [`CITIES_PER_ROUTE_SLOT`] cities. Own design.
pub fn routes_possible(auth: &AuthWorld, rules: &CompiledRuleset, p: PlayerId) -> u32 {
    let stats = &crate::effects::player::player_modifiers(auth, rules, p).stats;
    let routes = stats.get(Stat::Module(crate::rules::stats::ClassicStat::Trade(TradeStat::Routes))).max(0);
    let percent = (100 + stats.get(Stat::Module(crate::rules::stats::ClassicStat::Trade(TradeStat::RoutesPercent)))).max(0);
    let slots = (routes * percent / 100) as u32;
    // A nation that cannot field a trade unit yet (no slot from technology) gets no slot from size alone.
    if slots == 0 {
        return 0;
    }
    slots + (auth.indexes().cities_of(p).len() / CITIES_PER_ROUTE_SLOT) as u32
}

pub fn is_trade_unit(rules: &CompiledRuleset, kind: crate::ids::UnitKindId) -> bool {
    rules.units.get(kind.0 as usize).is_some_and(|u| u.module.economy.trade)
}

/// Trade units alive, routes running and (`with_queued`) trade units in
/// production.
pub fn routes_used(auth: &AuthWorld, rules: &CompiledRuleset, p: PlayerId, with_queued: bool) -> u32 {
    let units = auth
        .indexes()
        .units_of(p)
        .iter()
        .filter_map(|u| auth.units().get(*u))
        .filter(|v| is_trade_unit(rules, v.core.kind))
        .count() as u32;
    let running = auth.module().trade().owned_by(p).count() as u32;
    let queued = if with_queued { queued_trade_units(auth, rules, p) } else { 0 };
    units + running + queued
}

fn queued_trade_units(auth: &AuthWorld, rules: &CompiledRuleset, p: PlayerId) -> u32 {
    let mut n = 0;
    for city in auth.indexes().cities_of(p) {
        let Some(core) = auth.cities().get(*city) else {
            continue;
        };
        n += core
            .queue
            .iter()
            .filter(|i| matches!(i, crate::ids::ProductionItem::Unit(k) if is_trade_unit(rules, *k)))
            .count() as u32;
    }
    n
}

/// The player may start one more trade unit.
pub fn route_left(auth: &AuthWorld, rules: &CompiledRuleset, p: PlayerId) -> bool {
    routes_used(auth, rules, p, true) < routes_possible(auth, rules, p)
}

/// Range in tiles of routes from `origin`: the domain's base, one tile per two eras of the owner and
/// the owner's range effects, then the city's percent. Own design.
pub fn range(auth: &AuthWorld, rules: &CompiledRuleset, origin: &CityCore, domain: TradeDomain) -> u32 {
    let base = match domain {
        TradeDomain::Land => LAND_RANGE,
        TradeDomain::Sea => SEA_RANGE,
    };
    let era = i32::from(crate::world::player::player_era(auth, rules, origin.owner) / ERAS_PER_RANGE_TILE);
    let tiles = base + era + player_stat(auth, rules, origin.owner, TradeStat::RangeTiles(domain));
    let percent = 100 + city_stat(auth, rules, origin, TradeStat::RangePercent(domain));
    (tiles * percent / 100).max(0) as u32
}

pub fn speed(domain: TradeDomain) -> u32 {
    match domain {
        TradeDomain::Land => LAND_SPEED,
        TradeDomain::Sea => SEA_SPEED,
    }
}

/// Circuits a route of `length` steps runs: the whole number that brings its length closest to
/// [`TARGET_TURNS`], between [`MIN_CIRCUITS`] and [`MAX_CIRCUITS`]. Own design.
pub fn circuits(length: u32, domain: TradeDomain) -> u16 {
    let steps = (length * 2).max(1);
    let speed = speed(domain);
    // round(TARGET_TURNS × speed / steps) in integers
    let circuits = (2 * TARGET_TURNS * speed + steps) / (2 * steps);
    circuits.clamp(MIN_CIRCUITS, MAX_CIRCUITS) as u16
}

fn domain_of(rules: &CompiledRuleset, kind: crate::ids::UnitKindId) -> Option<TradeDomain> {
    match rules.units.get(kind.0 as usize)?.domain {
        Domain::Land => Some(TradeDomain::Land),
        Domain::Sea => Some(TradeDomain::Sea),
        Domain::Air => None,
    }
}

/// Everything a new route of `unit` to `destination` needs (rules 3.3).
pub fn route_status(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    unit: UnitHandle,
    destination: CityHandle,
    kind: RouteKind,
) -> Result<RoutePlan, TradeBlock> {
    let view = auth.units().get(unit).ok_or(TradeBlock::NotATradeUnit)?;
    if !is_trade_unit(rules, view.core.kind) {
        return Err(TradeBlock::NotATradeUnit);
    }
    let domain = domain_of(rules, view.core.kind).ok_or(TradeBlock::NotATradeUnit)?;
    let owner = view.core.owner;
    let origin = auth
        .indexes()
        .city_on_tile(view.core.tile)
        .filter(|c| auth.cities().get(*c).is_some_and(|core| core.owner == owner))
        .ok_or(TradeBlock::NotInOwnCity)?;
    pair_status(auth, rules, owner, origin, destination, kind)?;
    plan(auth, rules, origin, destination, domain)
}

/// The checks between two cities (rules 3.3, conditions 2–10).
fn pair_status(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    owner: PlayerId,
    origin: CityHandle,
    destination: CityHandle,
    kind: RouteKind,
) -> Result<(), TradeBlock> {
    let from = auth.cities().get(origin).ok_or(TradeBlock::UnknownCity)?;
    let to = auth.cities().get(destination).ok_or(TradeBlock::UnknownCity)?;
    if origin == destination {
        return Err(TradeBlock::SameCity);
    }
    let explored = auth.knowledge().get(owner.0 as usize).is_some_and(|k| k.is_explored(to.tile));
    if !explored {
        return Err(TradeBlock::Unexplored);
    }
    kind_status(auth, rules, owner, from, to, kind)?;
    if from.status == CityStatus::Razing || to.status == CityStatus::Razing {
        return Err(TradeBlock::Razing);
    }
    let exists = auth
        .module()
        .trade()
        .routes()
        .iter()
        .any(|r| r.origin == origin && r.destination == destination);
    if exists {
        return Err(TradeBlock::RouteExists);
    }
    Ok(())
}

fn kind_status(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    owner: PlayerId,
    from: &CityCore,
    to: &CityCore,
    kind: RouteKind,
) -> Result<(), TradeBlock> {
    let has = |ability: BuildingAbility| {
        from.buildings
            .iter()
            .any(|b| rules.buildings.get(b.0 as usize).is_some_and(|d| d.module.abilities.contains(&ability)))
    };
    match kind {
        RouteKind::International if to.owner == owner => Err(TradeBlock::OwnCityForInternational),
        RouteKind::International if auth.diplomacy().at_war(owner, to.owner) => Err(TradeBlock::AtWar),
        RouteKind::International if !crate::derived::tourism::met(auth, owner, to.owner) => Err(TradeBlock::NotMet),
        RouteKind::International if crate::systems::congress::trade_embargoed(auth, owner, to.owner) => Err(TradeBlock::Embargo),
        RouteKind::International => Ok(()),
        _ if to.owner != owner => Err(TradeBlock::ForeignCityForInternal),
        RouteKind::Food if !has(BuildingAbility::FoodTradeRoutes) => Err(TradeBlock::NoGranary),
        RouteKind::Production if !has(BuildingAbility::ProductionTradeRoutes) => Err(TradeBlock::NoWorkshop),
        _ => Ok(()),
    }
}

/// The path and the circuits of a route between two cities, within range.
pub fn plan(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    origin: CityHandle,
    destination: CityHandle,
    domain: TradeDomain,
) -> Result<RoutePlan, TradeBlock> {
    let from = auth.cities().get(origin).ok_or(TradeBlock::UnknownCity)?;
    let to = auth.cities().get(destination).ok_or(TradeBlock::UnknownCity)?;
    let map = auth.map();
    if domain == TradeDomain::Sea && !(map.coastal_land(from.tile) && map.coastal_land(to.tile)) {
        return Err(TradeBlock::NotCoastal);
    }
    let budget = range(auth, rules, from, domain) * STEP_COST;
    let path = crate::derived::trade_path::cheapest(auth, rules, from, to.tile, domain, budget)
        .ok_or(TradeBlock::OutOfRange)?;
    let circuits = circuits(path.len().saturating_sub(1) as u32, domain);
    Ok(RoutePlan {
        origin,
        destination,
        domain,
        path,
        circuits,
    })
}

/// Luxury and strategic resources connected on a city's own tiles.
fn local_resources(auth: &AuthWorld, rules: &CompiledRuleset, city: CityHandle) -> Vec<ResourceId> {
    let mut out: Vec<ResourceId> = crate::derived::resources::city_resources(auth, rules, city)
        .into_iter()
        .filter(|r| {
            let class = rules.resources.get(r.0 as usize).and_then(|d| d.class);
            matches!(class, Some(ResourceClass::Luxury | ResourceClass::Strategic))
        })
        .collect();
    out.sort_by_key(|r| r.0);
    out.dedup();
    out
}

/// City gold in hundredths without trade routes.
fn gold_without_trade(auth: &AuthWorld, rules: &CompiledRuleset, city: CityHandle) -> u32 {
    let Some(core) = auth.cities().get(city) else {
        return 0;
    };
    let gold = crate::derived::yields::city_breakdown(auth, rules, core).rates.gold;
    let trade = city_trade_yields(auth, city).gold as i32;
    ((gold - trade).max(0) * 100) as u32
}

/// Science (hundredths) `a` gains from `b`'s technologies it lacks ([`science_for`]).
fn science_from(auth: &AuthWorld, a: PlayerId, b: PlayerId) -> u32 {
    if !auth.players().is_major(a) || !auth.players().is_major(b) {
        return 0;
    }
    let known = auth.players().techs_of(a);
    let gap = auth.players().techs_of(b).iter().filter(|t| !known.contains(t)).count() as u32;
    science_for(gap)
}

/// What a route yields at both ends now (rules 3.6–3.8).
pub fn route_values(auth: &AuthWorld, rules: &CompiledRuleset, route: &TradeRoute) -> (RouteYields, RouteYields) {
    let (Some(from), Some(to)) = (auth.cities().get(route.origin), auth.cities().get(route.destination)) else {
        return (RouteYields::default(), RouteYields::default());
    };
    match route.kind {
        RouteKind::International => international(auth, rules, route, from, to),
        RouteKind::Food | RouteKind::Production => (RouteYields::default(), internal(auth, rules, route, to)),
    }
}

fn international(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    route: &TradeRoute,
    from: &CityCore,
    to: &CityCore,
) -> (RouteYields, RouteYields) {
    let majors = auth.players().is_major(from.owner) && auth.players().is_major(to.owner);
    let city_term = city_gold_term(
        gold_without_trade(auth, rules, route.origin),
        gold_without_trade(auth, rules, route.destination),
    );
    let sender = &crate::effects::player::player_modifiers(auth, rules, from.owner).stats;
    let own = city_stat(auth, rules, from, TradeStat::RouteGold(route.domain))
        + sender.get(Stat::Module(crate::rules::stats::ClassicStat::Trade(TradeStat::RouteGold(route.domain))));
    let target = if majors { city_stat(auth, rules, to, TradeStat::TargetGold) } else { 0 };
    let extra = policy_gold(auth, &sender, from.owner, to.owner) + incoming_gold(auth, rules, to.owner);
    let sender_era = u32::from(crate::world::player::player_era(auth, rules, from.owner));
    let recipient_era = u32::from(crate::world::player::player_era(auth, rules, to.owner));
    let scale = distance_percent(route.length()) * domain_percent(route.domain) / 100;
    let value = INTERNATIONAL_BASE
        + INTERNATIONAL_PER_ERA * sender_era
        + city_term
        + (own.max(0) + target.max(0) + extra.max(0)) as u32 * 100
        + resource_gold(auth, rules, route, &sender);
    let origin_gold = value * scale / 100;
    let recipient = if majors { city_stat(auth, rules, to, TradeStat::RecipientGold).max(0) as u32 } else { 0 };
    let destination_gold = (RECIPIENT_BASE + RECIPIENT_PER_ERA * recipient_era + city_term / 2 + recipient * 100) * scale / 100;
    // Cultural influence on a nation adds science (track B).
    let influence = if majors {
        let level = crate::derived::tourism::influence_level(auth, from.owner, to.owner);
        crate::derived::tourism::trade_route_science_bonus(level)
    } else {
        0
    };
    let origin = RouteYields {
        gold: origin_gold,
        science: science_from(auth, from.owner, to.owner) + influence,
        ..RouteYields::default()
    };
    let destination = RouteYields {
        gold: destination_gold,
        science: science_from(auth, to.owner, from.owner),
        ..RouteYields::default()
    };
    (origin, destination)
}

/// [`RESOURCE_GOLD`] per luxury or strategic resource only one of the two cities has (at most
/// [`RESOURCE_GOLD_MAX`]), times the sender's percent (Portugal).
fn resource_gold(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    route: &TradeRoute,
    sender: &crate::rules::stats::StatTotals,
) -> u32 {
    let ours = local_resources(auth, rules, route.origin);
    let theirs = local_resources(auth, rules, route.destination);
    let differ = ours.iter().filter(|r| !theirs.contains(r)).count() + theirs.iter().filter(|r| !ours.contains(r)).count();
    let percent = (100 + sender.get(Stat::Module(crate::rules::stats::ClassicStat::Trade(TradeStat::ResourceGoldPercent)))).max(0) as u32;
    resource_gold_for(differ) * percent / 100
}

/// The sender's policy gold of a route to `receiver` (rules 3.6 `Policy`):
/// Merchant Confederacy to a city-state, Economic Union to a nation
/// that follows the sender's ideology.
fn policy_gold(
    auth: &AuthWorld,
    sender: &crate::rules::stats::StatTotals,
    from: PlayerId,
    receiver: PlayerId,
) -> i32 {
    if auth.players().is_city_state(receiver) {
        return sender.get(Stat::Module(crate::rules::stats::ClassicStat::Trade(TradeStat::CityStateRouteGold)));
    }
    let ideology = auth.module().culture().of(from).ideology;
    if ideology.is_some() && ideology == auth.module().culture().of(receiver).ideology {
        return sender.get(Stat::Module(crate::rules::stats::ClassicStat::Trade(TradeStat::SharedIdeologyRouteGold)));
    }
    0
}

/// Gold the receiver's own effects give whoever sends it a route (Morocco).
fn incoming_gold(auth: &AuthWorld, rules: &CompiledRuleset, receiver: PlayerId) -> i32 {
    if !auth.players().is_major(receiver) {
        return 0;
    }
    player_stat(auth, rules, receiver, TradeStat::IncomingRouteGold)
}

fn internal(auth: &AuthWorld, rules: &CompiledRuleset, route: &TradeRoute, to: &CityCore) -> RouteYields {
    let era = u32::from(crate::world::player::player_era(auth, rules, to.owner));
    let base = internal_amount(era, to.population);
    let sea = if route.domain == TradeDomain::Sea { INTERNAL_SEA_PERCENT } else { 0 };
    let percent = (100 + sea + player_stat(auth, rules, to.owner, TradeStat::InternalPercent)).max(0) as u32;
    let amount = base * percent / 100;
    match route.kind {
        RouteKind::Food => RouteYields { food: amount, ..RouteYields::default() },
        _ => RouteYields { production: amount, ..RouteYields::default() },
    }
}

/// Whole yields the routes of a city give it now (the stored values of
/// both ends, summed in hundredths).
pub fn city_trade_yields(auth: &AuthWorld, city: CityHandle) -> RouteYields {
    let mut sum = RouteYields::default();
    for route in auth.module().trade().touching(city) {
        let y = if route.origin == city { route.origin_yields } else { route.destination_yields };
        sum.gold += y.gold;
        sum.science += y.science;
        sum.food += y.food;
        sum.production += y.production;
    }
    RouteYields {
        gold: sum.gold / 100,
        science: sum.science / 100,
        food: sum.food / 100,
        production: sum.production / 100,
    }
}

/// The other nations and city-states a trade route connects
/// `player` with, either way (routes inside the empire do not
/// count).
pub fn trade_partners(auth: &AuthWorld, player: PlayerId) -> u32 {
    let mut partners: Vec<PlayerId> = Vec::new();
    for route in auth.module().trade().routes() {
        let Some(destination) = auth.cities().get(route.destination).map(|c| c.owner) else {
            continue;
        };
        let partner = if route.owner == player {
            destination
        } else if destination == player {
            route.owner
        } else {
            continue;
        };
        if partner != player && !partners.contains(&partner) {
            partners.push(partner);
        }
    }
    partners.len() as u32
}

/// Trade routes of `player` to city-states.
pub fn city_state_routes(auth: &AuthWorld, player: PlayerId) -> u32 {
    auth.module().trade()
        .owned_by(player)
        .filter(|r| {
            let owner = auth.cities().get(r.destination).map(|c| c.owner);
            owner.is_some_and(|o| auth.players().is_city_state(o))
        })
        .count() as u32
}

/// A destination a trade unit could take now, with what it would yield.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TradeOffer {
    pub destination: CityHandle,
    pub kind: RouteKind,
    pub steps: u32,
    pub origin: RouteYields,
    pub destination_yields: RouteYields,
    /// What the route adds beyond its own yields, hundredths of yields: a
    /// new trading partner or one more route to a city-state for the
    /// owner's per-count effects (Morocco, Hanse) and the influence a turn
    /// of Treaty Organization (`side_value`).
    pub side_value: u32,
}

/// Every route `unit` may start now (the trade route panel and the AI).
pub fn trade_offers(auth: &AuthWorld, rules: &CompiledRuleset, unit: UnitHandle) -> Vec<TradeOffer> {
    let Some(view) = auth.units().get(unit) else {
        return Vec::new();
    };
    let mut offers = Offers::new(view.core.owner, view.core.kind);
    for (destination, _) in auth.cities().iter() {
        for kind in RouteKind::ALL {
            if let Ok(plan) = route_status(auth, rules, unit, destination, kind) {
                offers.add(auth, rules, kind, &plan);
            }
        }
    }
    offers.out
}

/// The routes a new trade unit of `unit_kind` in its owner's `origin`
/// could start now: what `trade_offers` would list for it, so the AI can
/// value a trade unit before it builds one. One path per destination
/// within the unit's range.
pub fn prospects(auth: &AuthWorld, rules: &CompiledRuleset, origin: CityHandle, unit_kind: crate::ids::UnitKindId) -> Vec<TradeOffer> {
    let (Some(domain), Some(from)) = (domain_of(rules, unit_kind), auth.cities().get(origin)) else {
        return Vec::new();
    };
    let reach = range(auth, rules, from, domain) as i32;
    let dims = auth.map().dimensions;
    let mut offers = Offers::new(from.owner, unit_kind);
    for (destination, to) in auth.cities().iter() {
        if dims.distance(from.tile, to.tile).is_none_or(|d| d > reach) {
            continue;
        }
        let kinds: Vec<RouteKind> = RouteKind::ALL.into_iter().filter(|k| pair_status(auth, rules, from.owner, origin, destination, *k).is_ok()).collect();
        if kinds.is_empty() {
            continue;
        }
        let Ok(plan) = plan(auth, rules, origin, destination, domain) else { continue };
        for kind in kinds {
            offers.add(auth, rules, kind, &plan);
        }
    }
    offers.out
}

/// Offers of one trade unit, with the side values per receiving player
/// (one evaluation each).
struct Offers {
    owner: PlayerId,
    unit_kind: crate::ids::UnitKindId,
    sides: Vec<(PlayerId, u32)>,
    out: Vec<TradeOffer>,
}

impl Offers {
    fn new(owner: PlayerId, unit_kind: crate::ids::UnitKindId) -> Self {
        Self { owner, unit_kind, sides: Vec::new(), out: Vec::new() }
    }

    fn add(&mut self, auth: &AuthWorld, rules: &CompiledRuleset, kind: RouteKind, plan: &RoutePlan) {
        let route = draft(self.owner, self.unit_kind, kind, plan);
        let (origin, destination_yields) = route_values(auth, rules, &route);
        let receiver = auth.cities().get(plan.destination).map(|c| c.owner).unwrap_or(self.owner);
        let side = match self.sides.iter().find(|(p, _)| *p == receiver) {
            Some((_, value)) => *value,
            None => {
                let value = side_value(auth, rules, self.owner, receiver);
                self.sides.push((receiver, value));
                value
            }
        };
        self.out.push(TradeOffer {
            destination: plan.destination,
            kind,
            steps: route.length(),
            origin,
            destination_yields,
            side_value: side,
        });
    }
}

/// Value of a route from `owner` to a city of `receiver` beyond its own
/// yields, hundredths (an AI estimate, see `TradeOffer::side_value`): the
/// owner's per-count effects whose count the route raises, at one step
/// (a flat yield counts in full, a percent as a tenth of a yield per
/// point), and Treaty Organization's influence with a city-state not yet
/// connected.
pub fn side_value(auth: &AuthWorld, rules: &CompiledRuleset, owner: PlayerId, receiver: PlayerId) -> u32 {
    if receiver == owner {
        return 0;
    }
    let to_city_state = auth.players().is_city_state(receiver);
    let connected = crate::derived::tourism::trade_connected(auth, owner, receiver);
    let raises = |count: Count| match count {
        Count::Module(ClassicCount::TradePartners) => !connected,
        Count::Module(ClassicCount::CityStateRoutes) => to_city_state,
        Count::Population
        | Count::Module(ClassicCount::Policies)
        | Count::WorldWonders
        | Count::ConnectedCities
        | Count::Module(ClassicCount::MajorityFollowers)
        | Count::Module(ClassicCount::ReligionsPresent)
        | Count::Module(ClassicCount::PeacefulForeignFollowers)
        | Count::Module(ClassicCount::OtherReligionFollowers) => false,
    };
    let mut value = 0u32;
    let mut add = |effect: &CompiledEffect, times: u32| {
        value += count_step_value(effect, &raises) * times;
    };
    crate::effects::visit_owner_sources(auth, rules, owner, |s| &s.passive_city, &mut add);
    for city in auth.indexes().cities_of(owner) {
        let Some(core) = auth.cities().get(*city) else {
            continue;
        };
        for building in &core.buildings {
            let Some(def) = rules.buildings.get(building.0 as usize) else {
                continue;
            };
            for effect in def.effects.passive_city.iter().filter(|e| e.selector == Selector::SourceCity) {
                add(effect, 1);
            }
        }
    }
    if to_city_state && !connected {
        let influence = crate::systems::city_states::stat(auth, rules, owner, CityStateStat::TradeRouteInfluence);
        value += influence.max(0) as u32 * 100;
    }
    value
}

/// One step of a per-count effect whose count `raises`, hundredths.
fn count_step_value(effect: &CompiledEffect, raises: &dyn Fn(Count) -> bool) -> u32 {
    match effect.modifier {
        Modifier::AddYieldPerCount { amount, per, .. } if raises(per.count) => {
            amount.max(0) as u32 * 100 / u32::from(per.every.max(1))
        }
        Modifier::AddYieldPercentPerCount { percent, per, .. } if raises(per.count) => {
            percent.max(0) as u32 * 10 / u32::from(per.every.max(1))
        }
        _ => 0,
    }
}

/// The route a plan starts (values filled by the caller).
pub fn draft(owner: PlayerId, unit_kind: crate::ids::UnitKindId, kind: RouteKind, plan: &RoutePlan) -> TradeRoute {
    TradeRoute {
        owner,
        origin: plan.origin,
        destination: plan.destination,
        domain: plan.domain,
        kind,
        unit_kind,
        path: plan.path.clone(),
        position: 0,
        forward: true,
        circuits_done: 0,
        circuits_total: plan.circuits,
        origin_yields: RouteYields::default(),
        destination_yields: RouteYields::default(),
    }
}

/// The tile a trade unit moves to when it changes its home city (rules
/// 3.2): an own city other than where it is; coastal for a cargo ship.
pub fn home_status(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    unit: UnitHandle,
    city: CityHandle,
) -> Result<TileId, TradeBlock> {
    let view = auth.units().get(unit).ok_or(TradeBlock::NotATradeUnit)?;
    if !is_trade_unit(rules, view.core.kind) {
        return Err(TradeBlock::NotATradeUnit);
    }
    let core = auth.cities().get(city).ok_or(TradeBlock::UnknownCity)?;
    if core.owner != view.core.owner {
        return Err(TradeBlock::NotInOwnCity);
    }
    if core.tile == view.core.tile {
        return Err(TradeBlock::SameCity);
    }
    if domain_of(rules, view.core.kind) == Some(TradeDomain::Sea) && !auth.map().coastal_land(core.tile) {
        return Err(TradeBlock::NotCoastal);
    }
    Ok(core.tile)
}

/// A combat unit of a player at war with `owner` on `tile` (its owner:
/// the plunderer; not embarked).
pub fn enemy_on(auth: &AuthWorld, rules: &CompiledRuleset, tile: TileId, owner: PlayerId) -> Option<PlayerId> {
    auth.indexes().units_on(tile).iter().find_map(|u| {
        let v = auth.units().get(*u)?;
        let combat = !rules.is_civilian(v.core.kind);
        let war = auth.diplomacy().at_war(v.core.owner, owner);
        let embarked = auth.map().is_water(tile) && rules.units.get(v.core.kind.0 as usize).is_some_and(|d| d.domain == Domain::Land);
        (combat && war && !embarked).then_some(v.core.owner)
    })
}

/// The route `unit` may plunder: the first one whose trade unit stands on
/// its tile, owned by a player it is at war with.
pub fn plunder_target(auth: &AuthWorld, rules: &CompiledRuleset, unit: UnitHandle) -> Result<usize, TradeBlock> {
    let view = auth.units().get(unit).ok_or(TradeBlock::NotATradeUnit)?;
    let routes = auth.module().trade().routes();
    let index = routes
        .iter()
        .position(|r| r.unit_tile() == view.core.tile)
        .ok_or(TradeBlock::NoRouteHere)?;
    let owner = routes[index].owner;
    match enemy_on(auth, rules, view.core.tile, owner) {
        Some(p) if p == view.core.owner => Ok(index),
        _ => Err(TradeBlock::NotAtWar),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reference economy per era (hundredths of gold per turn of one developed city), taken from the
    /// headless census of the AI games (`examples/expansion_census.rs`: 2.3 / 4.2 / 7.5 / 11.6 gold
    /// per city in the eras the games reach) and extended with the same growth for the later eras.
    const CITY_GOLD: [u32; 8] = [300, 500, 800, 1200, 1800, 2600, 3500, 4600];
    /// Reference food of such a city per turn (hundredths), flat in the census (7.6 - 8.7).
    const CITY_FOOD: [u32; 8] = [800, 850, 900, 1000, 1100, 1250, 1400, 1550];

    fn international(era: u32, gold: u32, length: u32, domain: TradeDomain, resources: usize) -> u32 {
        let value = INTERNATIONAL_BASE
            + INTERNATIONAL_PER_ERA * era
            + city_gold_term(gold, gold)
            + resource_gold_for(resources);
        value * distance_percent(length) * domain_percent(domain) / 10_000
    }

    #[test]
    fn a_route_is_worth_a_fraction_of_the_city_that_sends_it_in_every_era() {
        for (era, gold) in CITY_GOLD.iter().enumerate() {
            let land = international(era as u32, *gold, 8, TradeDomain::Land, 1);
            let sea = international(era as u32, *gold, 8, TradeDomain::Sea, 1);
            let share = land * 100 / gold;
            assert!((15..=70).contains(&share), "era {era}: a land route is {share} % of the city's gold ({land})");
            assert!(sea > land && sea <= land * 16 / 10, "era {era}: the sea route pays half as much again");
        }
    }

    #[test]
    fn the_international_gold_grows_with_the_era_and_stays_below_the_cities_own_taxes() {
        let mut previous = 0;
        for (era, gold) in CITY_GOLD.iter().enumerate() {
            let route = international(era as u32, *gold, 10, TradeDomain::Land, 2);
            assert!(route > previous, "era {era}: {route} must exceed {previous}");
            assert!(route < *gold, "era {era}: a route never beats the city's whole income");
            previous = route;
        }
    }

    #[test]
    fn the_city_term_is_a_geometric_mean_of_the_two_incomes() {
        assert_eq!(city_gold_term(0, 5000), 0, "a city without gold adds nothing");
        assert_eq!(city_gold_term(400, 400), 72, "18 % of 4 gold");
        assert_eq!(city_gold_term(400, 900), 108, "the mean of 4 and 9 gold is 6: 18 % is 1.08");
        // Doubling both cities doubles the term (it is linear in the common scale) ...
        assert_eq!(city_gold_term(800, 800), 2 * city_gold_term(400, 400));
        // ... but one city growing tenfold only multiplies it by about three (the square root of ten).
        let one_big = city_gold_term(400, 4000);
        assert!(one_big > 3 * city_gold_term(400, 400) && one_big < 4 * city_gold_term(400, 400), "{one_big}");
    }

    #[test]
    fn distance_pays_up_to_a_cap_and_the_sea_pays_more() {
        assert_eq!(distance_percent(0), 100);
        assert_eq!(distance_percent(8), 124);
        assert_eq!(distance_percent(15), 145);
        assert_eq!(distance_percent(40), 145, "no bonus beyond 15 tiles");
        assert_eq!(domain_percent(TradeDomain::Land), 100);
        assert_eq!(domain_percent(TradeDomain::Sea), 150);
    }

    #[test]
    fn resource_gold_and_science_have_caps() {
        assert_eq!(resource_gold_for(0), 0);
        assert_eq!(resource_gold_for(3), 75);
        assert_eq!(resource_gold_for(9), 125, "at most five resources count");
        assert_eq!(science_for(0), 0);
        assert_eq!(science_for(4), 120);
        assert_eq!(science_for(40), 450, "at most 4.5 science a turn");
    }

    #[test]
    fn internal_routes_feed_a_city_a_visible_share_of_its_food() {
        for (era, food) in CITY_FOOD.iter().enumerate() {
            let small = internal_amount(era as u32, 3);
            let big = internal_amount(era as u32, 15);
            assert!(big > small);
            assert!(small * 100 / food >= 10, "era {era}: even a small city gets a tenth: {small} of {food}");
            assert!(big * 100 / food <= 60, "era {era}: a big city never gets more than 60 %: {big} of {food}");
        }
    }

    #[test]
    fn plunder_scales_with_the_era_and_the_sea() {
        assert_eq!(plunder_gold(0, TradeDomain::Land), 60);
        assert_eq!(plunder_gold(4, TradeDomain::Land), 100);
        assert_eq!(plunder_gold(0, TradeDomain::Sea), 90);
        assert!(plunder_gold(7, TradeDomain::Land) > plunder_gold(3, TradeDomain::Land));
    }

    #[test]
    fn circuits_fit_the_target_length_for_every_route_length() {
        for length in 1..=30u32 {
            for domain in [TradeDomain::Land, TradeDomain::Sea] {
                let c = u32::from(circuits(length, domain));
                assert!((MIN_CIRCUITS..=MAX_CIRCUITS).contains(&c));
                // The route lasts 2 L c / speed turns: close to the target unless a bound applies.
                let turns = 2 * length * c / speed(domain);
                if c > MIN_CIRCUITS && c < MAX_CIRCUITS {
                    assert!(turns.abs_diff(TARGET_TURNS) <= 2 * length / speed(domain) + 1, "{length} {domain:?}: {turns} turns");
                }
            }
        }
    }
}
