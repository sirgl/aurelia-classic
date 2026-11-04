//! The Maya Long Count of the game calendar (`turn::calendar`): its cycles
//! fire OnLongCountCycle.

use crate::turn::calendar::speed_months_at;
use kernel::rules::speeds::SpeedValues;

/// Days of a baktun and the Long Count epoch year (3114 BC).
const BAKTUN_DAYS: i64 = 144_000;
const LONG_COUNT_EPOCH_YEAR: i64 = -3114;
/// Ten-thousandths of a day per year (365.2422).
const YEAR_DAYS_TIMES10000: i64 = 3_652_422;

/// The Long Count cycle (baktun) the date of `round` falls in at a game
/// speed (its calendar); cycles before the epoch are negative.
pub fn baktun(speed: &SpeedValues, round: u32) -> i64 {
    let months = speed_months_at(speed, round) + (i64::from(speed.start_year) - LONG_COUNT_EPOCH_YEAR) * 12;
    let days = (months * YEAR_DAYS_TIMES10000).div_euclid(12 * 10_000);
    days.div_euclid(BAKTUN_DAYS)
}

/// A Long Count cycle ended between the previous round and `round`.
pub fn long_count_cycle_ends(speed: &SpeedValues, round: u32) -> bool {
    round > 0 && baktun(speed, round) > baktun(speed, round - 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::turn::calendar::year;

    #[test]
    fn long_count_cycles_end_about_every_394_years() {
        let standard = SpeedValues::STANDARD;
        let ends: Vec<i64> = (1..500).filter(|r| long_count_cycle_ends(&standard, *r)).map(year).collect();
        // From 3902 BC to 2012 AD.
        assert_eq!(ends.len(), 16, "{ends:?}");
        // The cycle that ended in 2012 AD (13.0.0.0.0).
        assert!(ends.contains(&2012), "{ends:?}");
        assert!(ends.contains(&1618) || ends.contains(&1620), "{ends:?}");
    }
}
