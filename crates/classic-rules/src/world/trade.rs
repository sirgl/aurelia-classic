//! Trade routes (classic/docs/classic/rules.md): caravans and cargo
//! ships that run between two cities for a number of circuits.
//! Authoritative: persisted (economy section) and hashed. The rules that
//! create, value and move routes are in `derived::trade` and
//! `systems::trade`.

use crate::ids::{CityHandle, PlayerId, TileId, UnitKindId};
pub use crate::rules::stats::TradeDomain;

/// What a route carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RouteKind {
    /// Gold and science for both ends; the cities belong to different
    /// players.
    International,
    /// Food for the destination city of the same player (Granary).
    Food,
    /// Production for the destination city of the same player (Workshop).
    Production,
}

impl RouteKind {
    pub const ALL: [RouteKind; 3] = [RouteKind::International, RouteKind::Food, RouteKind::Production];

    pub fn code(self) -> u8 {
        match self {
            RouteKind::International => 0,
            RouteKind::Food => 1,
            RouteKind::Production => 2,
        }
    }

    pub fn from_code(code: u8) -> Option<Self> {
        RouteKind::ALL.into_iter().find(|k| k.code() == code)
    }
}

/// Yields of a route at one end, in hundredths (gold, science, food,
/// production), once for each end of the route.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct RouteYields {
    pub gold: u32,
    pub science: u32,
    pub food: u32,
    pub production: u32,
}

/// One running route.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TradeRoute {
    pub owner: PlayerId,
    pub origin: CityHandle,
    pub destination: CityHandle,
    pub domain: TradeDomain,
    pub kind: RouteKind,
    /// The trade unit that comes back when the route ends.
    pub unit_kind: UnitKindId,
    /// Origin to destination, both city tiles included.
    pub path: Vec<TileId>,
    /// Where the trade unit is on the path, and which way it goes.
    pub position: u16,
    pub forward: bool,
    pub circuits_done: u16,
    pub circuits_total: u16,
    /// Values of the last update (owner's or destination owner's turn).
    pub origin_yields: RouteYields,
    pub destination_yields: RouteYields,
}

impl TradeRoute {
    /// The tile the trade unit stands on.
    pub fn unit_tile(&self) -> TileId {
        self.path[(self.position as usize).min(self.path.len() - 1)]
    }

    /// Steps of the path (`L` of the rules).
    pub fn length(&self) -> u32 {
        self.path.len().saturating_sub(1) as u32
    }

    pub fn finished(&self) -> bool {
        self.circuits_done >= self.circuits_total
    }
}

/// Every running route, in creation order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TradeState {
    routes: Vec<TradeRoute>,
}

impl TradeState {
    pub fn routes(&self) -> &[TradeRoute] {
        &self.routes
    }

    pub fn owned_by(&self, player: PlayerId) -> impl Iterator<Item = &TradeRoute> {
        self.routes.iter().filter(move |r| r.owner == player)
    }

    /// Routes that start or end at `city`.
    pub fn touching(&self, city: CityHandle) -> impl Iterator<Item = &TradeRoute> {
        self.routes.iter().filter(move |r| r.origin == city || r.destination == city)
    }

    pub(crate) fn push(&mut self, route: TradeRoute) {
        self.routes.push(route);
    }

    pub(crate) fn routes_mut(&mut self) -> &mut Vec<TradeRoute> {
        &mut self.routes
    }

    pub(crate) fn from_routes(routes: Vec<TradeRoute>) -> Self {
        Self { routes }
    }
}
