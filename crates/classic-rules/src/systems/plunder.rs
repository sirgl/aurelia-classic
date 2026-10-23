//! Gold taken by force: the gold a conqueror takes from a captured city
//! and the plunder percents of effects on it and on barbarian camp gold
//! (`ClassicStat::PlunderGoldPercent` of the taker: Songhai;
//! `ClassicStat::CapturedGoldPercent` of the captured city: Burial Tomb).
//!
//! ```text
//! gold = PLUNDER_BASE_GOLD + population × PLUNDER_GOLD_PER_POPULATION
//!        + rand(CITY_PLUNDER_ROLL)                                   (combat stream)
//! gold = gold × min(rounds held by the old owner, 50) / 50
//! gold = gold × (100 + city percent) / 100 × (100 + taker percent) / 100
//! ```
//!
//! The shape and every constant follow Unciv
//! (`city/managers/CityConquestFunctions.kt`, `getGoldForCapturingCity`): the
//! percents multiply by the exact fraction, rounded down at the end of each
//! step. classic/docs/classic/rules.md has no capture gold section yet.

use crate::effects::city::city_modifiers;
use crate::effects::player::player_modifiers;
use crate::ids::{CityHandle, PlayerId};
use crate::rules::stats::Stat;
use crate::rules::CompiledRuleset;
use crate::transaction::transaction::WorldTransaction;
use crate::world::state::AuthWorld;

/// Flat part of the gold taken from a captured city. Unciv:
/// CityConquestFunctions.getGoldForCapturingCity, 20; equal.
pub const PLUNDER_BASE_GOLD: u32 = 20;
/// Gold taken per citizen of the captured city. Unciv:
/// CityConquestFunctions.getGoldForCapturingCity, 10; equal.
pub const PLUNDER_GOLD_PER_POPULATION: u32 = 10;
/// Bound of the one random roll added to the gold taken (the roll is below
/// it). Unciv: CityConquestFunctions.getGoldForCapturingCity, one roll below
/// 40; equal.
pub const CITY_PLUNDER_ROLL: u64 = 40;
/// Rounds of holding after which a city gives full gold (less before).
/// Unciv: CityConquestFunctions.getGoldForCapturingCity, 50; equal.
pub const CITY_PLUNDER_FULL_AFTER_ROUNDS: u32 = 50;

impl<'a> WorldTransaction<'a> {
    /// The conqueror takes the city's gold. Called before the city changes
    /// hands: its population, buildings and holding time are the old
    /// owner's.
    pub(crate) fn take_capture_gold(&mut self, city: CityHandle, conqueror: PlayerId, rules: &CompiledRuleset) {
        let Some(core) = self.world.cities.get(city) else {
            return;
        };
        let population = core.population;
        let held = self.world.turn.round.saturating_sub(core.acquired_round);
        let city_percent = city_modifiers(self.world, rules, core).stats.get(Stat::Module(crate::rules::stats::ClassicStat::CapturedGoldPercent));
        let roll = self.world.rng.combat.below(CITY_PLUNDER_ROLL);
        let mut gold = PLUNDER_BASE_GOLD + population * PLUNDER_GOLD_PER_POPULATION + roll as u32;
        gold = gold * held.min(CITY_PLUNDER_FULL_AFTER_ROUNDS) / CITY_PLUNDER_FULL_AFTER_ROUNDS;
        gold = scaled_by_percent(gold, city_percent);
        gold = scaled_by_percent(gold, plunder_percent(self.world, rules, conqueror));
        self.world.players.earn(conqueror, gold);
    }
}

/// The taker's plunder percent (Songhai +200).
pub fn plunder_percent(auth: &AuthWorld, rules: &CompiledRuleset, taker: PlayerId) -> i32 {
    player_modifiers(auth, rules, taker).stats.get(Stat::Module(crate::rules::stats::ClassicStat::PlunderGoldPercent))
}

/// `gold × (100 + percent) / 100`, rounded down (Unciv multiplies by the
/// exact fraction, `toPercent`).
pub fn scaled_by_percent(gold: u32, percent: i32) -> u32 {
    let factor = u64::from((100 + percent).max(0) as u32);
    (u64::from(gold) * factor / 100).min(u64::from(u32::MAX)) as u32
}
