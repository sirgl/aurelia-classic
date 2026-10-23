//! Saved trade routes (part of the economy section; `world::trade`).
//! Cities are written as their position in the save's city order.

use crate::ids::{CityHandle, PlayerId, TileId, UnitKindId};
use crate::rules::CompiledRuleset;
use crate::world::state::AuthWorld;
use crate::world::trade::{RouteKind, RouteYields, TradeDomain, TradeRoute, TradeState};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SavedTradeRoute {
    pub owner: u8,
    pub origin: u32,
    pub destination: u32,
    /// 0 land, 1 sea.
    pub domain: u8,
    /// `RouteKind::code`.
    pub kind: u8,
    pub unit_kind: u16,
    pub path: Vec<u32>,
    pub position: u16,
    pub forward: bool,
    pub circuits_done: u16,
    pub circuits_total: u16,
    /// Origin then destination: gold, science, food, production.
    pub yields: Vec<u32>,
}

fn yields_of(y: RouteYields) -> [u32; 4] {
    [y.gold, y.science, y.food, y.production]
}

pub fn encode_routes(auth: &AuthWorld) -> Vec<SavedTradeRoute> {
    let order: Vec<CityHandle> = auth.cities().iter().map(|(h, _)| h).collect();
    let ordinal = |c: CityHandle| order.iter().position(|h| *h == c).unwrap_or(usize::MAX) as u32;
    auth.module().trade()
        .routes()
        .iter()
        .map(|r| SavedTradeRoute {
            owner: r.owner.0,
            origin: ordinal(r.origin),
            destination: ordinal(r.destination),
            domain: u8::from(r.domain == TradeDomain::Sea),
            kind: r.kind.code(),
            unit_kind: r.unit_kind.0,
            path: r.path.iter().map(|t| t.0).collect(),
            position: r.position,
            forward: r.forward,
            circuits_done: r.circuits_done,
            circuits_total: r.circuits_total,
            yields: [yields_of(r.origin_yields), yields_of(r.destination_yields)].concat(),
        })
        .collect()
}

/// The routes of a save (cities already restored, in save order).
pub fn decode_routes(
    auth: &AuthWorld,
    saved: &[SavedTradeRoute],
    rules: &CompiledRuleset,
) -> Result<TradeState, &'static str> {
    let order: Vec<CityHandle> = auth.cities().iter().map(|(h, _)| h).collect();
    let routes = saved
        .iter()
        .map(|s| decode_route(s, &order, auth, rules))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(TradeState::from_routes(routes))
}

fn decode_route(
    s: &SavedTradeRoute,
    order: &[CityHandle],
    auth: &AuthWorld,
    rules: &CompiledRuleset,
) -> Result<TradeRoute, &'static str> {
    let city = |i: u32| order.get(i as usize).copied().ok_or("route city");
    let tiles = auth.map().tile_count() as u32;
    if s.path.len() < 2 || s.path.iter().any(|t| *t >= tiles) || s.position as usize >= s.path.len() {
        return Err("route path");
    }
    if usize::from(s.owner) >= auth.players().count() || usize::from(s.unit_kind) >= rules.units.len() {
        return Err("route owner or unit");
    }
    if s.yields.len() != 8 || s.domain > 1 {
        return Err("route values");
    }
    let y = |i: usize| RouteYields {
        gold: s.yields[i],
        science: s.yields[i + 1],
        food: s.yields[i + 2],
        production: s.yields[i + 3],
    };
    Ok(TradeRoute {
        owner: PlayerId(s.owner),
        origin: city(s.origin)?,
        destination: city(s.destination)?,
        domain: if s.domain == 1 { TradeDomain::Sea } else { TradeDomain::Land },
        kind: RouteKind::from_code(s.kind).ok_or("route kind")?,
        unit_kind: UnitKindId(s.unit_kind),
        path: s.path.iter().map(|t| TileId(*t)).collect(),
        position: s.position,
        forward: s.forward,
        circuits_done: s.circuits_done,
        circuits_total: s.circuits_total,
        origin_yields: y(0),
        destination_yields: y(4),
    })
}

/// Hash words of the routes (canonical saved form).
pub fn hash_words(saved: &[SavedTradeRoute]) -> Vec<u64> {
    let mut words = Vec::new();
    for r in saved {
        words.push(u64::from(r.owner) << 40 | u64::from(r.origin) << 20 | u64::from(r.destination));
        words.push(u64::from(r.domain) << 32 | u64::from(r.kind) << 24 | u64::from(r.unit_kind));
        words.extend(r.path.iter().map(|t| u64::from(*t)));
        words.push(u64::from(r.position) << 32 | u64::from(r.forward) << 31 | u64::from(r.circuits_done) << 16 | u64::from(r.circuits_total));
        words.extend(r.yields.iter().map(|v| u64::from(*v)));
        words.push(0x7472_6164_65);
    }
    words
}
