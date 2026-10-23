//! Research cost of a technology (classic/docs/classic/rules.md): one function
//! for the turn's research, espionage, the Korean boost, the interface and
//! the AI. Nothing is stored.
//!
//! ```text
//! base   = Cost × difficulty research % × map research %
//! known  = 1 + 0.3 × (met living civilizations that know it) / (living civilizations)
//! cost   = ⌊base / known × (1 + per-city % × (non-puppet cities − 1))⌋
//! ```
//!
//! then the player's ModifyCost(Technologies) effects, at least 1. Unciv:
//! TechManager.costOfTech, equal in shape: the difficulty percent applies to
//! humans only (the AI default level carries 100), the map's 100–130 % and
//! the per-city percent counted from the second non-puppet city (Unciv
//! `MapSize.kt`), the discount for known technologies (Unciv's
//! `TechManager.getScienceModifier`), the result truncated. Standard speed
//! and an Ancient start: their percents are 100.

use crate::effects::player::player_modifiers;
use crate::ids::{PlayerId, TechId};
use crate::rules::modifiers::Caps;
use crate::rules::CompiledRuleset;
use crate::world::size::WorldSize;
use crate::world::state::AuthWorld;

/// The discount in percent when every living civilization knows the
/// technology. Unciv: TechManager.getScienceModifier, equal (0.3).
pub const KNOWN_DISCOUNT_PERCENT: i64 = 30;

/// The numbers of the research cost formula.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TechCostInputs {
    /// The technology's content cost.
    pub base: u32,
    /// The player's difficulty research percent (an AI: 100, Unciv applies it to humans only).
    pub difficulty_percent: i32,
    /// The map size's research percent (`world_size::research_percent`).
    pub world_percent: i32,
    /// Hundredths of a percent added per city beyond the first
    /// (`world_size::tech_cost_per_city`: 500 = 5 %).
    pub per_city_percent: i64,
    /// The player's non-puppet cities now (may be 0: Unciv's `cities - 1`
    /// then lowers the cost).
    pub cities: u32,
    /// Met living civilizations that know the technology.
    pub known: u32,
    /// Living civilizations, the player included.
    pub living: u32,
}

/// Science for one technology: Unciv's TechManager.costOfTech as an exact
/// fraction, truncated at the end (Unciv: `toInt()`).
pub fn tech_cost(input: TechCostInputs) -> u32 {
    let living = i128::from(input.living.max(1));
    let mut num = i128::from(input.base);
    num *= i128::from(input.difficulty_percent.max(1));
    num *= i128::from(input.world_percent.max(1));
    // ÷ (1 + 0.3 known / living)
    num *= 100 * living;
    let mut den: i128 = 100 * 100 * (100 * living + i128::from(KNOWN_DISCOUNT_PERCENT) * i128::from(input.known));
    // × (1 + per-city × (cities − 1)), per-city in hundredths of a percent.
    num *= (10_000 + i128::from(input.per_city_percent) * (i128::from(input.cities) - 1)).max(0);
    den *= 10_000;
    (num / den).clamp(1, i128::from(u32::MAX)) as u32
}

/// The inputs for `player` researching `tech` now.
pub fn tech_cost_inputs(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId, tech: TechId) -> TechCostInputs {
    let base = rules.technologies.get(tech.0 as usize).map_or(0, |t| t.cost);
    let size = WorldSize::of(auth.map().dimensions);
    let (known, living) = known_by_others(auth, player, tech);
    TechCostInputs {
        base,
        difficulty_percent: auth.settings().handicap(rules, player).research_percent,
        world_percent: crate::classic::world_size::research_percent(size),
        per_city_percent: crate::classic::world_size::tech_cost_per_city(size),
        cities: crate::derived::culture::effective_cities(auth, player),
        known,
        living,
    }
}

/// (met living civilizations that know `tech`, living civilizations).
fn known_by_others(auth: &AuthWorld, player: PlayerId, tech: TechId) -> (u32, u32) {
    let players = auth.players();
    let (mut known, mut living) = (0, 0);
    for index in 0..players.count() {
        let other = PlayerId(index as u8);
        if !players.is_major(other) || !players.is_alive(other) {
            continue;
        }
        living += 1;
        let knows = other != player && players.techs_of(other).contains(&tech);
        if knows && crate::derived::tourism::met(auth, player, other) {
            known += 1;
        }
    }
    (known, living)
}

/// Science `player` needs for `tech` now (rules.md), after the
/// player's ModifyCost(Technologies) effects; at least 1.
pub fn technology_cost(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId, tech: TechId) -> u32 {
    let cost = tech_cost(tech_cost_inputs(auth, rules, player, tech));
    let mods = player_modifiers(auth, rules, player);
    mods.tech_cost.evaluate(cost.min(i32::MAX as u32) as i32, Caps::at_least(1)) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn writing(cities: u32) -> TechCostInputs {
        TechCostInputs {
            base: 55,
            difficulty_percent: 100,
            world_percent: 110,
            per_city_percent: 500,
            cities,
            known: 0,
            living: 8,
        }
    }

    /// A human at the default difficulty on a Standard map (Unciv:
    /// TechManager.costOfTech, MapSize.kt): Writing 55 × 1.1 = 60.5 → 60, ×
    /// 1.15 → 69, × 1.35 → 81 for 1 / 4 / 8 cities; Philosophy 192 / 221 /
    /// 259; Education 533 / 613 / 720.
    #[test]
    fn reference_costs_by_city_count() {
        let cost = |base, cities| tech_cost(TechCostInputs { base, ..writing(cities) });
        assert_eq!([cost(55, 1), cost(55, 4), cost(55, 8)], [60, 69, 81]);
        assert_eq!([cost(175, 1), cost(175, 4), cost(175, 8)], [192, 221, 259]);
        assert_eq!([cost(485, 1), cost(485, 4), cost(485, 8)], [533, 613, 720]);
    }

    /// Writing, one city, known by 3 of 8 civilizations → 60.5 / 1.1125 =
    /// 54.4 → 54; known by all: 60.5 / 1.3 = 46.5 → 46.
    #[test]
    fn known_technologies_are_cheaper() {
        assert_eq!(tech_cost(TechCostInputs { known: 3, ..writing(1) }), 54);
        assert_eq!(tech_cost(TechCostInputs { known: 8, ..writing(1) }), 46);
    }

    /// A difficulty of 85 %; a Huge map 130 % and 2.5 % a city.
    #[test]
    fn difficulty_and_map_size_scale_the_cost() {
        // 55 × 85 % × 110 % = 51.4 → 51.
        assert_eq!(tech_cost(TechCostInputs { difficulty_percent: 85, ..writing(1) }), 51);
        let huge = TechCostInputs {
            world_percent: crate::classic::world_size::research_percent(WorldSize::Huge),
            per_city_percent: crate::classic::world_size::tech_cost_per_city(WorldSize::Huge),
            ..writing(10)
        };
        // 55 × 130 % = 71.5, × (1 + 9 × 2.5 %) = 87.6 → 87.
        assert_eq!(tech_cost(huge), 87);
        // No city yet: Unciv's (0 − 1) × 5 % lowers the cost: 60.5 × 0.95 → 57.
        assert_eq!(tech_cost(writing(0)), 57);
    }
}
