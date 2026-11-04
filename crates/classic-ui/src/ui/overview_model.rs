//! Read-only view models of the advisor screens (overviews, demographics,
//! notification log), the city banners and the HUD tooltips. `game::Session`
//! builds them from the observer's knowledge and the core's own queries.
use classic_rules::ids::{UnitHandle, UnitKindId};

use crate::ui::model::{Notice, Rival};
pub use client::shell::module::{CityBanner, UnitTip, YieldTip};

/// One own city on the cities overview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CityRow {
    pub tile: u32,
    pub name: String,
    pub capital: bool,
    pub population: u32,
    pub food: u32,
    pub production: u32,
    pub gold: i32,
    pub science: i32,
    pub culture: u32,
    pub faith: u32,
    /// Combat strength of the city (defense).
    pub strength: i32,
    /// "Warrior (3)" or "Nothing".
    pub producing: String,
    /// Turns until the next citizen (None: no food surplus).
    pub grows_in: Option<u32>,
}

/// One own unit on the military overview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitRow {
    pub unit: UnitHandle,
    pub tile: u32,
    pub kind: UnitKindId,
    pub name: String,
    pub military: bool,
    pub hp: u16,
    pub strength: i32,
    pub ranged: u16,
    pub moves: String,
    /// "Fortified", "Working · 3 turns", "Ready".
    pub status: String,
}

/// A classic demographics row: the player's value, rank and the rivals' range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DemographicRow {
    pub label: &'static str,
    pub unit: &'static str,
    pub value: i64,
    pub rank: usize,
    pub best: i64,
    pub average: i64,
    pub worst: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OverviewView {
    pub turn: u32,
    pub cities: Vec<CityRow>,
    pub units: Vec<UnitRow>,
    pub demographics: Vec<DemographicRow>,
    pub notices: Vec<Notice>,
    /// The same authoritative projection used by the full victory screen.
    pub victory: Option<crate::ui::model::VictoryView>,
    /// Nations the player knows (the diplomacy overview).
    pub rivals: Vec<Rival>,
    /// Gold per turn and its breakdown (the economy panel).
    pub gold: Option<YieldTip>,
    /// Strategic and luxury resources of the empire (the resources tab).
    pub resources: Vec<ResourceRow>,
    /// Trade routes run or received (the trade tab).
    pub trade_routes: Vec<TradeRouteRow>,
    /// Own trade units with the routes they may start.
    pub trade_units: Vec<TradeUnitRow>,
    /// "1 of 2 trade routes in use".
    pub trade_summary: String,
}

/// A running route as the trade panel lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TradeRouteRow {
    pub from: String,
    pub to: String,
    pub kind: &'static str,
    /// What our end gets; what the other end gets.
    pub yields: String,
    pub other_end: String,
    pub turns_left: u32,
    /// Our route (else a route to one of our cities).
    pub own: bool,
}

/// An own trade unit and the routes it may start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TradeUnitRow {
    pub unit: classic_rules::ids::UnitHandle,
    pub at: String,
    pub offers: Vec<TradeOfferRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TradeOfferRow {
    pub destination: classic_rules::ids::CityHandle,
    pub kind: classic_rules::world::trade::RouteKind,
    pub label: String,
    pub yields: String,
    pub other_end: String,
}

/// One resource of the empire: copies connected, held by units and
/// buildings, and free (negative: a deficit).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceRow {
    pub name: String,
    pub class: &'static str,
    pub total: i32,
    pub used: i32,
    pub available: i32,
}

/// Tooltips of the top bar, in the order of its values.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TopBarTips {
    pub yields: Vec<YieldTip>,
}

/// Everything the HUD tooltips and banners read in one frame.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HudExtras {
    pub top_bar: TopBarTips,
    pub banners: Vec<CityBanner>,
    /// Units on the badge hovered in the previous frame.
    pub units: Option<(u32, Vec<UnitTip>)>,
    /// Nation name by player index (owners of tiles and units).
    pub player_names: Vec<String>,
}
