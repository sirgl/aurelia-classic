//! Growth (classic/docs/classic/rules.md): the formulas of the food
//! difference, the threshold and the food kept; the store itself is the
//! kernel's (`WorldTransaction::grow_city`): at the threshold the city grows
//! by one citizen and keeps its kept food; below zero it starves (one
//! citizen lost, never below 1). While the head of the queue is a food unit
//! (settler) the surplus goes to production instead and the store does not
//! move.
//!
//! ```text
//! threshold(pop) = 15 + 8·(pop−1) + ⌊(pop−1)^1.5⌋          (Standard speed)
//! difference     = food − 2·pop; a positive one × (100 + growth %) / 100
//! store          += difference; kept += difference, kept ≤ threshold·FoodKept %
//! store ≥ threshold → store −= threshold − kept, pop + 1
//! store < 0         → store = 0, pop − 1 (if pop > 1)
//! ```

use crate::world::player::player_era;
use crate::derived::citizens::FOOD_PER_CITIZEN;
use crate::derived::yields::CityBreakdown;
use crate::rules::stats::{SocietyStat, Stat};
use crate::rules::CompiledRuleset;

/// Food a city of size 1 needs to grow (the constant term of
/// `growth_threshold`). Unciv: CityPopulationManager.getFoodToNextPopulation;
/// equal.
pub const FOOD_TO_NEXT_POPULATION_BASE: u32 = 15;
/// Food added to the threshold per citizen beyond the first (the linear
/// term of `growth_threshold`). Unciv: same function; equal.
pub const FOOD_TO_NEXT_POPULATION_PER_POP: u32 = 8;
/// Least city size at which a settler can be trained. Unciv: Units.json
/// (Settler, `Requires at least [2] population`), equal.
pub const SETTLER_MIN_CITY_SIZE: u32 = 2;

/// Food a city of `pop` citizens needs to grow: `15 + 8 (pop−1) + floor((pop−1)^1.5)`,
/// Standard speed, start in the Ancient era (Unciv `CityPopulationManager.kt`,
/// its food-to-next-population function: the same formula, before its speed and city-state
/// factors; checked 2026-10-20). `(pop−1)^1.5` truncated is
/// `isqrt((pop−1)^3)`: exact in integers, the same on every platform.
pub fn growth_threshold(pop: u32) -> u32 {
    let n = u64::from(pop.max(1) - 1);
    let power = (n * n * n).isqrt();
    let threshold = u64::from(FOOD_TO_NEXT_POPULATION_BASE) + n * u64::from(FOOD_TO_NEXT_POPULATION_PER_POP) + power;
    threshold.min(u64::from(u32::MAX)) as u32
}

/// `growth_threshold` at the game's speed: × the speed's pace percent,
/// rounded down, at least 1 (Unciv: CityPopulationManager's food to the next
/// population, × `modifier`; equal). Standard: `growth_threshold` itself.
pub fn speed_growth_threshold(speed: &kernel::rules::speeds::SpeedValues, pop: u32) -> u32 {
    let base = growth_threshold(pop);
    if speed.pace_percent == 100 {
        return base;
    }
    kernel::rules::speeds::SpeedValues::scale(base, speed.pace_percent).max(1)
}

/// The threshold of a player's city at the game's speed
/// (`speed_growth_threshold`): an AI player's grows by the game
/// level's AI growth percent and its per-era modifier per era (rules.md), at least 1.
pub fn player_growth_threshold(
    auth: &crate::world::state::AuthWorld,
    rules: &CompiledRuleset,
    player: crate::ids::PlayerId,
    pop: u32,
) -> u32 {
    let base = speed_growth_threshold(&auth.settings().speed(rules), pop);
    if !auth.settings().is_ai(player) {
        return base;
    }
    let handicap = auth.settings().handicap(rules, player);
    let era = i64::from(player_era(auth, rules, player));
    let t = i64::from(base) * i64::from(handicap.ai_growth_percent) / 100;
    let t = t * (i64::from(handicap.ai_per_era_modifier) * era + 100).max(0) / 100;
    t.clamp(1, i64::from(u32::MAX)) as u32
}


/// What a food surplus (hundredths) adds to production while a food unit
/// is built (hundredths): in full up to 2 food, half of the next 2, a
/// quarter beyond. Unciv: CityStats.getProductionFromExcessiveFood
/// (`2 + food / 4` from 4 food, 2 from 2, 1 from 1), equal for whole food.
pub fn food_production_times100(surplus_times100: i64) -> i64 {
    if surplus_times100 <= 0 {
        0
    } else if surplus_times100 <= 200 {
        surplus_times100
    } else if surplus_times100 <= 400 {
        200 + (surplus_times100 - 200) / 2
    } else {
        300 + (surplus_times100 - 400) / 4
    }
}

pub use kernel::systems::turn::producing_food;

/// Food the citizens eat, hundredths: each specialist eats
/// `SpecialistFoodPercent` more (Civil Society −50: half).
pub fn consumption_times100(yields: &CityBreakdown, population: u32) -> i64 {
    let percent = i64::from(yields.modifiers.stats.get(Stat::Module(crate::rules::stats::ClassicStat::Society(SocietyStat::SpecialistFoodPercent))));
    let specialists = i64::from(yields.work.specialist_count().min(population));
    let eaters = (i64::from(population) * 100 + specialists * percent).max(0);
    i64::from(FOOD_PER_CITIZEN) * eaters
}

/// The food difference of one turn, hundredths: 0 while producing food; a positive
/// difference grows by the city's growth percent and `empire_percent`
/// (unhappiness, We Love the King Day).
pub fn food_difference_times100(
    yields: &CityBreakdown,
    population: u32,
    producing_food: bool,
    empire_percent: i32,
) -> i64 {
    if producing_food {
        return 0;
    }
    let difference = i64::from(yields.rates.food) * 100 - consumption_times100(yields, population);
    if difference <= 0 {
        return difference;
    }
    let percent = 100 + i64::from(yields.modifiers.growth_percent) + i64::from(empire_percent);
    difference * percent.max(0) / 100
}

/// Production a food unit gets from the surplus this turn (whole points).
pub fn food_production(yields: &CityBreakdown, population: u32) -> i32 {
    let surplus = i64::from(yields.rates.food) * 100 - consumption_times100(yields, population);
    (food_production_times100(surplus) / 100) as i32
}

/// Share of the threshold kept on growth (percent, 0 to 100).
pub fn food_kept_percent(yields: &CityBreakdown) -> u32 {
    yields.modifiers.stats.get(Stat::FoodKept).clamp(0, 100) as u32
}
