//! Golden ages (classic/docs/classic/rules.md): each turn before the cities
//! resolve, a running golden age counts down;
//! otherwise the excess happiness of the turn fills the meter (a negative
//! excess drains it, never below 0) and a full meter starts a golden age.
//! One started during another only lengthens it. Journal:
//! ClassicChange::GoldenAgeChanged (tile and city yields depend on whether
//! a golden age is on).
//!
//! ```text
//! threshold = 500 + 250 × golden ages so far
//!           + ⌊threshold × (cities − 1) × 1 / 100⌋, rounded down to 5
//! length    = 10 × (100 + Stat::Module(crate::rules::stats::ClassicStat::GoldenAgeLengthPercent)) / 100
//! ```

use crate::classic::ClassicChange;
use crate::ids::PlayerId;
use crate::rules::stats::Stat;
use crate::rules::CompiledRuleset;
use crate::transaction::transaction::WorldTransaction;

impl<'a> WorldTransaction<'a> {
    /// One turn of the golden age: the counter goes down (its end is a
    /// change of the player's yields), or the meter fills.
    pub(crate) fn process_golden_age(&mut self, player: PlayerId, rules: &CompiledRuleset) {
        let threshold = golden_age_threshold(self.world, rules, player);
        let length = golden_age_length(self.world, rules, player, UNMODIFIED_GOLDEN_AGE_TURNS);
        let Some(economy) = self.world.players.economy_mut(player) else {
            return;
        };
        if economy.golden_age_turns == 0 {
            let meter = i64::from(economy.golden_age_meter) + i64::from(economy.excess_happiness);
            economy.golden_age_meter = meter.clamp(0, i64::from(u32::MAX)) as u32;
            if economy.golden_age_meter >= threshold {
                economy.golden_age_meter -= threshold;
                self.start_golden_age(player, length);
            }
            return;
        }
        economy.golden_age_turns -= 1;
        if economy.golden_age_turns == 0 {
            self.journal
                .module_changes
                .push(ClassicChange::GoldenAgeChanged { player });
        }
    }

    /// Points toward the next golden age (a kill of a unit with Golden Age
    /// Points); none while a golden age runs.
    pub(crate) fn add_golden_age_points(&mut self, player: PlayerId, points: u32) {
        if let Some(economy) = self.world.players.economy_mut(player) {
            if economy.golden_age_turns == 0 {
                economy.golden_age_meter = economy.golden_age_meter.saturating_add(points);
            }
        }
    }

    /// Start (or lengthen) a golden age of `turns` (already lengthened by
    /// the player's modifiers); a new one counts.
    pub(crate) fn start_golden_age(&mut self, player: PlayerId, turns: u16) {
        let Some(economy) = self.world.players.economy_mut(player) else {
            return;
        };
        if economy.golden_age_turns == 0 {
            economy.golden_ages = economy.golden_ages.saturating_add(1);
        }
        economy.golden_age_turns = economy.golden_age_turns.saturating_add(turns);
        self.journal
            .module_changes
            .push(ClassicChange::GoldenAgeChanged { player });
    }
}

/// Rounds of a golden age at Standard speed, before modifiers (the game
/// speed's `golden_age_percent` scales it in `golden_age_length`). Unciv:
/// GoldenAgeManager.enterGoldenAge, 10; equal.
pub const UNMODIFIED_GOLDEN_AGE_TURNS: u16 = 10;
/// Happiness the first golden age needs, and the increase per golden age
/// already had. Unciv: GoldenAgeManager.happinessRequiredForNextGoldenAge,
/// 500 and 250; equal.
pub const GOLDEN_AGE_BASE_THRESHOLD: u32 = 500;
/// Unciv: GoldenAgeManager.happinessRequiredForNextGoldenAge (250), equal.
pub const GOLDEN_AGE_THRESHOLD_INCREASE: u32 = 250;
/// Percent added to the threshold per city, the first included; the result
/// is truncated, not rounded to 5. Unciv:
/// GoldenAgeManager.happinessRequiredForNextGoldenAge, equal.
pub const GOLDEN_AGE_THRESHOLD_CITY_PERCENT: u32 = 1;

/// Happiness the meter needs for the player's next golden age, × the game
/// speed's pace (Unciv: happinessRequiredForNextGoldenAge, `modifier`; equal).
pub fn golden_age_threshold(auth: &crate::world::state::AuthWorld, rules: &CompiledRuleset, player: PlayerId) -> u32 {
    let ages = auth
        .players()
        .economy_of(player)
        .map(|e| u32::from(e.golden_ages))
        .unwrap_or(0);
    let cities = auth.indexes().cities_of(player).len() as u32;
    let mut threshold = GOLDEN_AGE_BASE_THRESHOLD + ages * GOLDEN_AGE_THRESHOLD_INCREASE;
    threshold += threshold * cities * GOLDEN_AGE_THRESHOLD_CITY_PERCENT / 100;
    kernel::rules::speeds::SpeedValues::scale(threshold, auth.settings().speed(rules).pace_percent)
}

/// A golden age of `base` turns lengthened by the player's modifiers
/// (only a positive modifier counts), then × the game speed's golden age
/// percent, rounded down, at least 1 (Unciv: calculateGoldenAgeLength,
/// `goldenAgeLengthModifier`; equal).
pub fn golden_age_length(
    auth: &crate::world::state::AuthWorld,
    rules: &CompiledRuleset,
    player: PlayerId,
    base: u16,
) -> u16 {
    let percent = crate::effects::player::player_modifiers(auth, rules, player)
        .stats
        .get(Stat::Module(crate::rules::stats::ClassicStat::GoldenAgeLengthPercent));
    let speed = auth.settings().speed(rules).golden_age_percent;
    let turns = if percent <= 0 { u32::from(base) } else { u32::from(base) * (100 + percent as u32) / 100 };
    if percent <= 0 && speed == 100 {
        return base;
    }
    let turns = kernel::rules::speeds::SpeedValues::scale(turns, speed);
    turns.clamp(1, u32::from(u16::MAX)) as u16
}
