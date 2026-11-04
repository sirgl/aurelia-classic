//! Diplomatic Marriage (§4.15, Austria): a
//! nation allied for five rounds buys out the city-state. Its cities
//! wait for the buyer's decision (annex or puppet) without resistance, its
//! units become the buyer's, and the city-state is gone.

use super::block::CityStateBlock;
use super::is_ally;
use crate::command::CommandError;
use crate::ids::{PlayerId, UnitHandle};
use crate::classic::effects::CityStateStat;
use crate::rules::CompiledRuleset;
use crate::transaction::transaction::WorldTransaction;
use crate::world::city::CityStatus;
use crate::world::state::AuthWorld;

/// Gold base price of buying out a city-state at Standard speed. Unciv:
/// logic/nation/diplomacy/CityStateFunctions.kt
/// (`getDiplomaticMarriageCost`, 500); equal.
pub const BUYOUT_COST: u32 = 500;
/// Rounds of alliance before the buyout is possible. Unciv: Nations.json
/// (Gods & Kings, Austria: allied for 5 turns); equal.
pub const BUYOUT_ROUNDS: u32 = 5;
/// Scrap gold of a unit: its gold price `(30 × cost)^0.75` divided by
/// this. Unciv: models/ruleset/unit/BaseUnit.kt (`getDisbandGold`, / 20)
/// and IConstruction.kt (`getBaseGoldCost`); equal.
pub const SCRAP_GOLD_DIVISOR: u32 = 20;

/// Scrap gold of a unit of `cost` production: `floor((30 cost)^0.75) / 20`,
/// in integers.
pub fn scrap_gold(cost: u32) -> u32 {
    let x = u64::from(cost) * 30;
    let price = x.saturating_mul(x).saturating_mul(x).isqrt().isqrt();
    (price / u64::from(SCRAP_GOLD_DIVISOR)) as u32
}

/// Buyout price: 500 plus the scrap gold of every unit of the
/// city-state, rounded down to 5. At another game speed the 500 is × its
/// gold percent and a unit's cost × its production percent (Unciv:
/// CityStateFunctions.getDiplomaticMarriageCost, `goldCostModifier`, and the
/// unit's production cost; equal).
pub fn buyout_cost(auth: &AuthWorld, rules: &CompiledRuleset, city_state: PlayerId) -> u32 {
    use kernel::rules::speeds::SpeedValues;
    let speed = auth.settings().speed(rules);
    let scrap: u32 = auth
        .indexes()
        .units_of(city_state)
        .iter()
        .filter_map(|u| auth.units().get(*u))
        .filter_map(|v| rules.units.get(v.core.kind.0 as usize))
        .map(|d| scrap_gold(SpeedValues::scale(d.cost, speed.production_percent)))
        .sum();
    (SpeedValues::scale(BUYOUT_COST, speed.gold_percent) + scrap) / 5 * 5
}

/// Why `major` may not buy out `city_state` now (None:
/// it may). Contact, life and peace are checked by the validator.
pub fn buyout_problem(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    city_state: PlayerId,
    major: PlayerId,
) -> Option<CityStateBlock> {
    // Cheap checks first: the observation asks for every city-state.
    if !is_ally(auth, city_state, major) {
        return Some(CityStateBlock::NotTheAlly);
    }
    if super::stat(auth, rules, major, CityStateStat::AnnexCityStates) <= 0 {
        return Some(CityStateBlock::CannotAnnex);
    }
    let since = auth.module().city_states().minor(city_state).allied_since;
    if auth.turn().round < since + BUYOUT_ROUNDS {
        return Some(CityStateBlock::AlliedTooRecently);
    }
    if auth.players().treasury_of(major) < buyout_cost(auth, rules, city_state) {
        return Some(CityStateBlock::NotEnoughGold);
    }
    None
}

impl<'a> WorldTransaction<'a> {
    /// The buyout: gold, cities, units; the alliance ends.
    pub(crate) fn buyout_city_state(
        &mut self,
        major: PlayerId,
        city_state: PlayerId,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        let cost = buyout_cost(self.world, rules, city_state);
        if !self.world.players.spend(major, cost) {
            return Err(CityStateBlock::NotEnoughGold.into());
        }
        self.acquire_city_state(major, city_state, CityStatus::AwaitingDecision, rules)
    }

    /// The takeover: the city-state's cities (in `status`: the buyer's
    /// decision, or a puppet for the Merchant of Venice) and units become
    /// the major's; the alliance ends.
    pub(crate) fn acquire_city_state(
        &mut self,
        major: PlayerId,
        city_state: PlayerId,
        status: CityStatus,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        let cities = self.world.indexes.cities_of(city_state).to_vec();
        for city in cities {
            self.transfer_city(city, city_state, major, rules)?;
            if let Some(core) = self.world.cities.get_mut(city) {
                core.status = status;
                core.resistance_turns = 0;
                core.queue.clear();
                core.production = 0;
            }
            self.journal
                .city_changes
                .push(crate::transaction::journal::CityChange::StatusChanged { city });
        }
        let mut units: Vec<UnitHandle> = self.world.indexes.units_of(city_state).to_vec();
        units.sort_by_key(|u| self.world.units.get(*u).map(|v| v.core.id));
        for unit in units {
            let Some(core) = self.world.units.get(unit).map(|v| *v.core) else {
                continue;
            };
            self.destroy_unit(unit)?;
            let tile = self.free_tile_near(major, core.tile, core.kind, rules);
            let gift = self.spawn_unit(major, tile, core.kind, rules);
            if let Some(new) = self.world.units.get_mut_core(gift) {
                new.moves_left = 0;
                new.hp = core.hp;
                new.experience = core.experience;
                new.promotions = core.promotions;
            }
        }
        let ally = self.world.module.city_states.ally(city_state);
        self.set_ally(city_state, ally, None, rules);
        Ok(())
    }
}
