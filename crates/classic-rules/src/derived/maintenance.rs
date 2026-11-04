//! Gold of an empire beyond its cities' yields (classic/docs/classic/rules.md):
//! maintenance of units, buildings and routes, and the gold of cities
//! connected to the capital.
//!
//! ```text
//! units      free = 3 + the difficulty's free units + units with NoMaintenance
//!            + garrisons with GarrisonFreeMaintenance (Oligarchy)
//!            + FreeMaintenanceUnits (Volunteer Army);
//!            paid = units − free (no cities: nothing)
//!            support = paid × 50 (hundredths)
//!            p = min(turn / 500, 1); cost = (support/100 × (1 + p))^(1 + p/3)
//!            × the difficulty's unit cost percent (AI: its AI unit cost percent) / 100
//! buildings  Σ maintenance × the difficulty's building cost percent (AI: its AI
//!            building cost percent)
//! routes     road 1, railroad 2 per own tile (city tiles free; hills
//!            free with FreeHillsRouteMaintenance, Inca)
//!            × (100 + route maintenance percent) / 100 × the route cost percent / 100
//! connection per city reached from the capital by routes (or over water
//!            between two harbors): −100 + 15 × capital size + 110 × size
//!            (hundredths), × (100 + Stat::Module(crate::rules::stats::ClassicStat::ConnectionGoldPercent)) / 100 per
//!            city (Machu Picchu), summed before rounding down
//! ```
//!
//! The power of the unit cost is computed in integers (fixed point), the
//! same on every platform; it rounds down like an integer cast of `pow(...)`.

use crate::world::city::MAX_TERRITORY_RADIUS;
use crate::ids::{CityHandle, PlayerId, TileId};
use crate::math::hex_disc;
use crate::rules::stats::{PlayerRule, Stat};
use crate::rules::CompiledRuleset;
use crate::world::map::Route;
use crate::world::state::AuthWorld;

/// Gold per paid unit in hundredths. Unciv: CivInfoStatsForNextTurn.getUnitMaintenance,
/// equal (0.5).
pub const GOLD_PER_UNIT_TIMES100: u64 = 50;
/// How fast the unit cost grows with the turn: the base is multiplied by
/// `1 + MULTIPLIER × p` and raised to `1 + p / EXPONENT_DIVISOR`, where p is the turn
/// over the game's last round (the speed's; at most 1). Unciv:
/// CivInfoStatsForNextTurn.getUnitMaintenance, equal (1 and 3).
pub const UNIT_COST_GROWTH_FACTOR: u64 = 1;
/// Unciv: CivInfoStatsForNextTurn.getUnitMaintenance, equal.
pub const UNIT_COST_EXPONENT_DIVISOR: u64 = 3;
/// The end turn of the Standard game (another speed: its
/// `SpeedValues::max_rounds`). Unciv: Speeds.json (Standard, 500 turns), equal.
pub const ESTIMATED_END_TURN: u64 = 500;
/// Units every player keeps without gold upkeep, before the difficulty's and
/// the policies' free units. Unciv: CivInfoStatsForNextTurn.getUnitMaintenance,
/// equal (3).
pub const FREE_UNITS: u64 = 3;
/// Gold per turn of a road tile and of a railroad tile. Unciv: TileImprovements.json
/// (Road, Railroad), equal.
pub const ROAD_MAINTENANCE: i64 = 1;
/// Unciv: TileImprovements.json (Railroad), equal.
pub const RAILROAD_MAINTENANCE: i64 = 2;
/// Gold of a city connected to the capital: a base, plus a share per citizen of the
/// capital and per citizen of the connected city (hundredths). Unciv:
/// CityStats.kt (trade route gold `0.15 capital + 1.1 size - 1`), equal.
pub const CONNECTION_BASE_GOLD_TIMES100: i64 = -100;
/// Unciv: CityStats.kt (connection gold), equal.
pub const CONNECTION_CAPITAL_POP_TIMES100: i64 = 15;
/// Unciv: CityStats.kt (connection gold), equal.
pub const CONNECTION_CITY_POP_TIMES100: i64 = 110;

/// The empire's gold beyond city yields, whole gold per turn.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Upkeep {
    pub units: i32,
    pub buildings: i32,
    pub routes: i32,
    /// Gold of the cities connected to the capital.
    pub connections: i32,
}

impl Upkeep {
    /// Costs minus the connection gold.
    pub fn net_cost(&self) -> i32 {
        self.units + self.buildings + self.routes - self.connections
    }
}

pub fn upkeep(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId) -> Upkeep {
    Upkeep {
        units: unit_maintenance(auth, rules, player),
        buildings: building_maintenance(auth, rules, player),
        routes: route_maintenance(auth, rules, player),
        connections: (connection_gold_times100(auth, rules, player) / 100) as i32,
    }
}

/// Unit maintenance (see the module docs).
pub fn unit_maintenance(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId) -> i32 {
    if auth.indexes().cities_of(player).is_empty() {
        return 0;
    }
    let handicap = auth.settings().handicap(rules, player);
    let garrisons = free_garrisons(auth, rules, player);
    let policy_free = crate::effects::player::player_modifiers(auth, rules, player)
        .stats
        .get(Stat::Module(crate::rules::stats::ClassicStat::FreeMaintenanceUnits));
    let free = FREE_UNITS + handicap.gold_free_units.max(0) as u64 + garrisons + policy_free.max(0) as u64;
    let paid = unit_weights_times100(auth, rules, player).saturating_sub(free * 100);
    let turn = u64::from(auth.turn().round);
    let end = u64::from(auth.settings().speed(rules).max_rounds());
    let cost = unit_cost_at(paid * GOLD_PER_UNIT_TIMES100 / 100, turn, end);
    let percent = if auth.settings().is_ai(player) {
        handicap.ai_unit_cost_percent
    } else {
        handicap.unit_cost_percent
    };
    let mut cost = cost * percent.max(0) as u64 / 100;
    // Standing Army Tax (World Congress): unit maintenance rises by ARMY_TAX_PERCENT.
    if auth.module().congress().is_active(crate::world::congress::ResolutionKind::StandingArmyTax) {
        cost = cost * (100 + crate::systems::congress::ARMY_TAX_PERCENT) / 100;
    }
    cost.min(i32::MAX as u64) as i32
}

/// The player's units that pay maintenance, in hundredths of a unit: each
/// weighs 100 + its MaintenancePercent (Germany's land units, the Ottomans'
/// ships, the Zulu's melee, Nationalism); kinds with NoMaintenance weigh
/// nothing. (Unverified whether the support of the discounted units is
/// scaled; the free units come off the weighted sum here.)
fn unit_weights_times100(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId) -> u64 {
    use crate::classic::effects::ClassicUnitStat;
    use crate::rules::effects::UnitStat;
    let stat = UnitStat::Module(ClassicUnitStat::MaintenancePercent);
    let owner_part = crate::effects::unit::owner_unit_modifiers(auth, rules, player);
    let mut by_kind: Vec<(crate::ids::UnitKindId, u64)> = Vec::new();
    let mut total = 0u64;
    for unit in auth.indexes().units_of(player) {
        let Some(view) = auth.units().get(*unit) else {
            continue;
        };
        let kind = view.core.kind;
        let weight = match by_kind.iter().find(|(k, _)| *k == kind) {
            Some((_, w)) => *w,
            None => {
                let free = rules.units.get(kind.0 as usize).is_some_and(|d| d.module.economy.no_maintenance);
                let percent =
                    crate::effects::unit::owner_stat_for_kind(auth, rules, &owner_part, player, kind, stat);
                let w = if free { 0 } else { (100 + percent).max(0) as u64 };
                by_kind.push((kind, w));
                w
            }
        };
        total += weight;
    }
    total
}

/// Garrisons that cost nothing (Oligarchy): one per own city with a garrison
/// (`combat::city::garrison`), 0 without the rule.
fn free_garrisons(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId) -> u64 {
    let stats = &crate::effects::player::player_modifiers(auth, rules, player).stats;
    if !crate::rules::stats::has_rule(&stats, PlayerRule::GarrisonFreeMaintenance) {
        return 0;
    }
    let mut count = 0;
    for city in auth.indexes().cities_of(player) {
        let Some(core) = auth.cities().get(*city) else {
            continue;
        };
        if crate::world::city::garrison(auth, rules, core).is_some() {
            count += 1;
        }
    }
    count
}

/// ⌊(support/100 × (1 + t/500))^(1 + t/1500)⌋ in fixed point (a Standard game).
pub fn unit_cost(support_times100: u64, turn: u64) -> u64 {
    unit_cost_at(support_times100, turn, ESTIMATED_END_TURN)
}

/// `unit_cost` in a game of `end` rounds (the game speed's last round,
/// `SpeedValues::max_rounds`): the share of the game played is `t / end`.
/// Unciv: CivInfoStatsForNextTurn.getUnitMaintenance (`speed.numTotalTurns()`), equal.
pub fn unit_cost_at(support_times100: u64, turn: u64, end: u64) -> u64 {
    // base = support × (E + t) / (100 E); exponent = (3E + t) / 3E.
    let end = end.max(1);
    let turn = turn.min(end);
    let num = u128::from(support_times100) * u128::from(end + UNIT_COST_GROWTH_FACTOR * turn);
    let den = u128::from(end) * 100;
    let exp_den = end * UNIT_COST_EXPONENT_DIVISOR;
    fixed::pow_floor(num, den, exp_den + turn, exp_den)
}

/// Building maintenance of the player's cities (see the module docs).
pub fn building_maintenance(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId) -> i32 {
    let handicap = auth.settings().handicap(rules, player);
    let total: i64 = auth
        .indexes()
        .cities_of(player)
        .iter()
        .filter_map(|c| auth.cities().get(*c))
        .flat_map(|c| c.buildings.iter())
        .filter_map(|b| rules.buildings.get(b.0 as usize))
        .map(|d| i64::from(d.maintenance))
        .sum();
    let percent = if auth.settings().is_ai(player) {
        handicap.ai_building_cost_percent
    } else {
        handicap.building_cost_percent
    };
    (total * i64::from(percent.max(0)) / 100) as i32
}

/// Every tile of the player's cities' territory (each once).
pub fn territory(auth: &AuthWorld, player: PlayerId) -> Vec<(CityHandle, TileId)> {
    let map = auth.map();
    let mut out = Vec::new();
    for handle in auth.indexes().cities_of(player) {
        let Some(core) = auth.cities().get(*handle) else {
            continue;
        };
        for tile in hex_disc(map.dimensions, core.tile, MAX_TERRITORY_RADIUS) {
            if map.owning_city(tile) == Some(*handle) {
                out.push((*handle, tile));
            }
        }
    }
    out
}

/// Road and railroad maintenance of the player's territory (rules.md).
/// The player's percents are summed (Wagon Trains and the Inca's −50 %
/// each: an Inca with Wagon Trains pays nothing, unverified); never below 0.
pub fn route_maintenance(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId) -> i32 {
    let map = auth.map();
    let stats = &crate::effects::player::player_modifiers(auth, rules, player).stats;
    let free_hills = crate::rules::stats::has_rule(&stats, PlayerRule::FreeHillsRouteMaintenance);
    let total: i64 = territory(auth, player)
        .into_iter()
        .filter(|(_, t)| auth.indexes().city_on_tile(*t).is_none())
        .filter(|(_, t)| !(free_hills && is_hills(auth, *t)))
        .map(|(_, t)| match map.route(t) {
            Route::None => 0,
            Route::Road => ROAD_MAINTENANCE,
            Route::Railroad => RAILROAD_MAINTENANCE,
        })
        .sum();
    let modifier = 100 + i64::from(stats.get(Stat::RouteMaintenancePercent));
    let total = total * modifier.max(0) / 100;
    let percent = auth.settings().handicap(rules, player).route_cost_percent;
    (total * i64::from(percent.max(0)) / 100) as i32
}

fn is_hills(auth: &AuthWorld, tile: TileId) -> bool {
    auth.map().terrain(tile).is_some_and(|t| auth.map().terrain_kinds().is_hills(t.0))
}


/// Gold of the cities connected to the capital, hundredths.
pub fn connection_gold_times100(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    player: PlayerId,
) -> i64 {
    let Some(capital) = crate::world::city::capital_of(auth, rules, player) else {
        return 0;
    };
    let capital_pop = auth
        .cities()
        .get(capital)
        .map(|c| i64::from(c.population))
        .unwrap_or(0);
    let cities: Vec<CityHandle> = crate::derived::connections::connected_cities(auth, rules, player, capital)
        .iter()
        .copied()
        .filter(|c| *c != capital)
        .collect();
    if cities.is_empty() {
        return 0;
    }
    let stats = &crate::effects::player::player_modifiers(auth, rules, player).stats;
    let percent = i64::from(stats.get(crate::rules::stats::Stat::Module(crate::rules::stats::ClassicStat::ConnectionGoldPercent)));
    cities
        .into_iter()
        .filter_map(|c| auth.cities().get(c))
        .map(|core| {
            let gold = CONNECTION_BASE_GOLD_TIMES100
                + capital_pop * CONNECTION_CAPITAL_POP_TIMES100
                + i64::from(core.population) * CONNECTION_CITY_POP_TIMES100;
            if percent == 0 {
                gold
            } else {
                gold * (100 + percent) / 100
            }
        })
        .sum()
}

/// Fixed-point powers (integers only: the same result everywhere).
mod fixed {
    /// Fraction bits.
    const BITS: u32 = 40;
    const ONE: i128 = 1 << BITS;
    /// ln 2 × 2^40.
    const LN2: i128 = 762_123_384_786;

    /// ⌊(num/den)^(exp_num/exp_den)⌋ for num, den > 0 (0 when num = 0).
    pub fn pow_floor(num: u128, den: u128, exp_num: u64, exp_den: u64) -> u64 {
        if num == 0 || den == 0 {
            return 0;
        }
        if exp_num == exp_den {
            return (num / den).min(u128::from(u64::MAX)) as u64;
        }
        let x = ((num << BITS) / den) as i128;
        let y = ln(x) * i128::from(exp_num) / i128::from(exp_den);
        (exp(y) >> BITS).clamp(0, i128::from(u64::MAX)) as u64
    }

    /// ln(x / 2^40) × 2^40 for x > 0.
    fn ln(x: i128) -> i128 {
        let k = (127 - x.leading_zeros() as i32) - BITS as i32;
        let m = if k >= 0 { x >> k } else { x << -k };
        let z = (m - ONE) * ONE / (m + ONE);
        let z2 = z * z / ONE;
        let mut term = z;
        let mut sum = 0;
        let mut n = 1;
        while term != 0 {
            sum += term / n;
            term = term * z2 / ONE;
            n += 2;
        }
        i128::from(k) * LN2 + 2 * sum
    }

    /// e^(y / 2^40) × 2^40.
    fn exp(y: i128) -> i128 {
        let n = y.div_euclid(LN2);
        let r = y.rem_euclid(LN2);
        let mut term = ONE;
        let mut sum = ONE;
        let mut i = 1;
        while term != 0 {
            term = term * r / ONE / i;
            sum += term;
            i += 1;
        }
        if n >= 0 {
            sum << n.min(80)
        } else {
            sum >> (-n).min(127)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::pow_floor;

        #[test]
        fn powers_match_floating_point_away_from_integers() {
            assert_eq!(pow_floor(13, 1, 3600, 3500), 13); // 13.98
            assert_eq!(pow_floor(26, 1, 3700, 3500), 31); // 31.36
            assert_eq!(pow_floor(1, 1, 3700, 3500), 1);
            assert_eq!(pow_floor(5, 2, 3500, 3500), 2);
            assert_eq!(pow_floor(1200, 1, 3, 4), 203); // 203.88
        }
    }
}

#[cfg(test)]
mod tests {
    use super::unit_cost;

    #[test]
    fn unit_maintenance_follows_the_classic_table() {
        // Unciv: CivInfoStatsForNextTurn.getUnitMaintenance, Standard,
        // default difficulty, paid units: (0.5 n (1 + p))^(1 + p/3), p = t/500.
        for (units, row) in [
            (5, [2, 3, 4, 5]),
            (10, [5, 6, 9, 12]),
            (20, [10, 14, 19, 27]),
        ] {
            let got: Vec<u64> = [0, 100, 200, 300]
                .iter()
                .map(|t| unit_cost(units * 50, *t))
                .collect();
            assert_eq!(got, row, "{units} units");
        }
    }
}
