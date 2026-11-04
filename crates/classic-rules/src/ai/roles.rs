//! Roles of the members of a front (classic/docs/ai-tactical.md,
//! docs/ai-war-design.md 4.4.2), derived from what the rules' unit data says
//! of each unit and never from a name:
//!
//! - `Artillery` — a unit that strikes from a range (ranged strength and
//!   range): it must be screened, because it defends with the melee strength
//!   of the data, far below what it shoots with;
//! - `Anvil` — a melee unit among the strongest of the group, slow or equal
//!   in movement to the rest: holds the line on good ground and takes the
//!   city;
//! - `Hammer` — a melee unit that moves more than the median melee unit of
//!   the group: a striker that kills and may keep away from the line;
//! - `Screen` — a melee unit much weaker than the best of the group: its body
//!   and zone of control close the ways to the artillery;
//! - `Reserve` — a unit too wounded to fight (below its withdrawal health):
//!   it leaves and heals.
//!
//! The role shapes what the plan search values (terrain for the anvil,
//! exposure for the artillery, the screened share), not what the unit may
//! do: every member may strike and move.

use crate::ai::battlefield::KindStats;
use crate::ai::state::ArmyRole;
use crate::ids::UnitKindId;
use crate::knowledge::observation::OwnUnit;
use crate::rules::CompiledRuleset;

/// A melee unit with less than this share (percent) of the best melee
/// strength of the group is a screen.
pub const SCREEN_BELOW_PERCENT: i64 = 75;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Role {
    Anvil,
    Hammer,
    Artillery,
    Screen,
    Reserve,
}

impl Role {
    pub fn name(self) -> &'static str {
        match self {
            Role::Anvil => "anvil",
            Role::Hammer => "hammer",
            Role::Artillery => "artillery",
            Role::Screen => "screen",
            Role::Reserve => "reserve",
        }
    }
}

/// Health below which a melee unit leaves the line (35), and 5 more for each
/// level it has gained (up to 4), so that veterans are preserved. (The front
/// of `standard` leaves below 50: measured, a line that leaves at 50 gives
/// up chokepoints and sieges that a wounded unit could still hold; the
/// search's own death and lethal costs guard the units between 35 and 50.)
/// Own design: a third of the health keeps a unit out of one blow's reach of death and leaves it a share of its strength; measured in the battle suite of classic/docs/ai-tactical.md.
pub const WITHDRAW_BELOW: u16 = 35;

pub fn withdraw_below(level: u8) -> u16 {
    WITHDRAW_BELOW + 5 * u16::from(level.min(4))
}

/// The role of every unit of the group, in the order of `units`.
pub fn assign(rules: &CompiledRuleset, units: &[OwnUnit]) -> Vec<Role> {
    let stats: Vec<KindStats> = units.iter().map(|u| KindStats::of(rules, u.kind)).collect();
    let healthy = |i: usize| units[i].hp >= withdraw_below(units[i].level);
    let melee: Vec<usize> = (0..units.len()).filter(|&i| !stats[i].is_ranged() && healthy(i)).collect();
    let best = melee.iter().map(|&i| stats[i].melee).max().unwrap_or(0);
    let mut moves: Vec<u32> = melee.iter().map(|&i| stats[i].movement).collect();
    moves.sort();
    let median = moves.get(moves.len() / 2).copied().unwrap_or(0);
    (0..units.len())
        .map(|i| {
            if !healthy(i) {
                Role::Reserve
            } else if stats[i].is_ranged() {
                Role::Artillery
            } else if stats[i].melee * 100 < best * SCREEN_BELOW_PERCENT {
                Role::Screen
            } else if stats[i].movement > median {
                Role::Hammer
            } else {
                Role::Anvil
            }
        })
        .collect()
}

/// The army role of a kind in the operations' sense (melee, ranged, siege),
/// for the reports of the suites.
pub fn army_role(rules: &CompiledRuleset, kind: UnitKindId) -> Option<ArmyRole> {
    crate::ai::operations::army_role(rules, kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn veterans_leave_earlier() {
        assert_eq!(withdraw_below(0), 35);
        assert_eq!(withdraw_below(2), 45);
        assert_eq!(withdraw_below(9), 55);
    }
}
