//! The classic instance of `kernel::rules::stats` (generic over the rules
//! module), with the classic stats (`classic::stats`).


pub use crate::classic::content::{GreatPersonKind, GREAT_PERSON_KINDS};
pub use crate::classic::stats::{ClassicStat, EspionageStat, LeagueStat, PlayerRule, SocietyStat, TradeDomain, TradeStat};

use crate::classic::Classic;

pub type Stat = kernel::rules::stats::Stat<Classic>;
pub type SourceStat = kernel::rules::stats::SourceStat<Classic>;
pub type StatTotals = kernel::rules::stats::StatTotals<Classic>;

/// The player has the rule exception (`ClassicStat::Rule`).
pub fn has_rule(stats: &StatTotals, rule: PlayerRule) -> bool {
    stats.get(Stat::Module(ClassicStat::Rule(rule))) > 0
}
