//! Research (the queue, science and learning are the kernel's,
//! `kernel::systems::research`) and the science boost of a capital.

pub use kernel::systems::research::{plan_research, ResearchPlanError};
pub use crate::derived::research::technology_cost;

use crate::rules::stats::{SocietyStat, Stat};
use crate::rules::CompiledRuleset;
use crate::transaction::transaction::WorldTransaction;

/// Percent of the median research cost that a science boost gives, for a
/// player without modifiers. Own value, not compared with a public source
/// (unverified).
pub const MEDIAN_TECH_PERCENT: u32 = 50;

/// A building whose own effects add science to its city (science
/// buildings and wonders: Library, University, Great Library, ...).
pub fn is_science_building(rules: &CompiledRuleset, building: crate::ids::BuildingKindId) -> bool {
    use crate::rules::effects::{Modifier, Selector, YieldKind};
    let Some(def) = rules.buildings.get(building.0 as usize) else {
        return false;
    };
    def.effects.passive_city.iter().any(|e| {
        let science = match e.modifier {
            Modifier::AddYield { yield_kind, amount } => yield_kind == YieldKind::Science && amount > 0,
            Modifier::AddYieldPercent { yield_kind, percent } => yield_kind == YieldKind::Science && percent > 0,
            Modifier::AddYieldPerPopulation { yield_kind, hundredths } => {
                yield_kind == YieldKind::Science && hundredths > 0
            }
            _ => false,
        };
        science && e.selector == Selector::SourceCity
    })
}

impl<'a> WorldTransaction<'a> {
    /// Korea: a science building completed in the capital adds science to
    /// the stock — half the median research cost of the technologies the
    /// player may research now (50 % of the median, unverified).
    pub(crate) fn capital_science_boost(
        &mut self,
        city: crate::ids::CityHandle,
        building: crate::ids::BuildingKindId,
        rules: &CompiledRuleset,
    ) {
        let Some(core) = self.world.cities.get(city) else {
            return;
        };
        let owner = core.owner;
        if !crate::world::city::is_capital(rules, core) || !is_science_building(rules, building) {
            return;
        }
        let mods = crate::effects::player::player_modifiers(self.world, rules, owner);
        if mods.stats.get(Stat::Module(crate::rules::stats::ClassicStat::Society(SocietyStat::CapitalScienceBuildingBoost))) <= 0 {
            return;
        }
        let mut costs: Vec<u32> = crate::derived::grants::researchable(self.world, rules, owner)
            .iter()
            .filter(|t| rules.technologies.get(t.0 as usize).is_some())
            .map(|t| technology_cost(self.world, rules, owner, *t))
            .collect();
        if costs.is_empty() {
            return;
        }
        costs.sort_unstable();
        let median = costs[costs.len() / 2];
        let boost = median * MEDIAN_TECH_PERCENT / 100;
        self.world.players.earn_science(owner, boost);
    }
}
