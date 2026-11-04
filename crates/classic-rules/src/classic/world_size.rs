//! The classic numbers of a world size (`world::size::WorldSize`).

use crate::world::size::WorldSize;

/// Hundredths of a percent added to the next policy's cost per non-puppet
/// city beyond the first: 10 % up to Standard, Large 7.5 %, Huge 5 %.
/// Unciv: MapSize.kt (`policyCostPerCityModifier`), equal.
pub fn policy_cost_per_city(size: WorldSize) -> i64 {
    match size {
        WorldSize::Duel | WorldSize::Tiny | WorldSize::Small | WorldSize::Standard => 1000,
        WorldSize::Large => 750,
        WorldSize::Huge => 500,
    }
}

/// Research cost percent of the map size (rules.md): Duel–Small 100, Standard 110,
/// Large 120, Huge 130. Unciv: MapSize.kt (`techCostMultiplier`), equal.
pub fn research_percent(size: WorldSize) -> i32 {
    match size {
        WorldSize::Duel | WorldSize::Tiny | WorldSize::Small => 100,
        WorldSize::Standard => 110,
        WorldSize::Large => 120,
        WorldSize::Huge => 130,
    }
}

/// Hundredths of a percent added to the research cost per non-puppet city
/// beyond the first: 5 % up to Standard, Large 3.75 %, Huge 2.5 %. Unciv:
/// MapSize.kt (`techCostPerCityModifier`), equal.
pub fn tech_cost_per_city(size: WorldSize) -> i64 {
    match size {
        WorldSize::Duel | WorldSize::Tiny | WorldSize::Small | WorldSize::Standard => 500,
        WorldSize::Large => 375,
        WorldSize::Huge => 250,
    }
}

/// Percent of the city count's unhappiness by map size: 100 on every
/// size. Unciv: CityStats.kt, equal (no map-size term; was Large 80, Huge 60).
pub fn city_count_unhappiness_percent(_size: WorldSize) -> i64 {
    100
}

/// Religions that can be founded in the game: 1 + half the major
/// nations (rounded down), whatever the map size. Unciv:
/// ReligionManager.maxNumberOfReligions, ModConstants.kt
/// (`religionLimitBase` 1, `religionLimitMultiplier` 0.5), equal (the cap at
/// the ruleset's religion count is not applied here).
pub fn max_religions(majors: u32) -> u8 {
    (1 + majors / 2).min(u32::from(u8::MAX)) as u8
}

/// Major nations of the game, alive or not (the religion limit's input).
pub fn major_count(auth: &crate::world::state::AuthWorld) -> u32 {
    let players = auth.players();
    (0..players.count()).filter(|i| players.is_major(crate::ids::PlayerId(*i as u8))).count() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_of_a_size() {
        assert_eq!((max_religions(8), max_religions(6), max_religions(2)), (5, 4, 2));
        assert_eq!(policy_cost_per_city(WorldSize::Large), 750);
        assert_eq!((research_percent(WorldSize::Small), research_percent(WorldSize::Standard)), (100, 110));
        assert_eq!((tech_cost_per_city(WorldSize::Large), tech_cost_per_city(WorldSize::Huge)), (375, 250));
        assert_eq!(city_count_unhappiness_percent(WorldSize::Large), 100);
    }
}
