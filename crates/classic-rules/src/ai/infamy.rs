//! What the world's memory of an aggressor costs and brings the AI
//! (classic/docs/classic/RESOURCING.md, "Infamy of a nuclear strike"). The rules
//! keep one memory of this kind, the warmonger memory of an observer about a
//! player (`systems::diplomacy`), raised by a declaration of war, a conquest
//! and, since 23 October 2026, by every nuclear strike, and fading with the
//! rounds. The AI reads it in three places, all in the allocator's currency
//! (hundredths of a score point):
//!
//! - **a nuclear strike costs it** (`strike_cost`): every civilization that has
//!   met the user and is not at war with it remembers the strike, and the AI
//!   knows the price from how it treats others: below;
//! - **peace costs more** with a civilization we remember as an aggressor
//!   (`peace_price`): the AI gives it peace only for compensation;
//! - **war against it is worth more** (`war_bonus`): its cities are a fairer
//!   target for the world than anyone's (the memory is the world's opinion).
//!
//! Deals and friendship already read the memory (`ai::diplomacy::trusted`:
//! above `DISTRUST_WARMONGER` no friendship or pact, an open denunciation
//! above `DENOUNCE_WARMONGER`).

use crate::ai::diplomacy::DISTRUST_WARMONGER;
use crate::ai::value::Centi;
use crate::knowledge::observation::Relation;
use crate::systems::diplomacy::WARMONGER_NUKE_WEIGHT;
use crate::world::player::PlayerKind;

/// Hundredths of a score point one point of warmonger memory is worth: a
/// civilization that remembers 400 points (one strike) distrusts, denounces and
/// will not make peace but for 24 points of compensation (own design, not tuned:
/// a city of six citizens is about 100 points).
pub const CENTI_PER_POINT: Centi = 6;
/// The part of the price of peace that is a bonus for war, percent: the
/// world forgives a war on an aggressor, it does not pay for it.
pub const WAR_BONUS_PERCENT: Centi = 50;

/// The memory above the line of distrust.
fn excess(warmonger: i32) -> Centi {
    Centi::from((warmonger - DISTRUST_WARMONGER).max(0))
}

/// What a peace with an aggressor whose warmonger memory (ours) is
/// `warmonger` costs the player.
pub fn peace_price(warmonger: i32) -> Centi {
    excess(warmonger) * CENTI_PER_POINT
}

/// What a war on that aggressor is worth more to the player.
pub fn war_bonus(warmonger: i32) -> Centi {
    excess(warmonger) * CENTI_PER_POINT * WAR_BONUS_PERCENT / 100
}

/// Civilizations that would remember a strike of ours: those we have met and
/// are not at war with (one at war with us already counts us an enemy).
pub fn witnesses(relations: &[Relation]) -> usize {
    relations.iter().filter(|r| r.kind == PlayerKind::Major && !r.at_war).count()
}

/// What a nuclear strike of `level` costs its user when `witnesses`
/// civilizations remember it, each `WARMONGER_NUKE_WEIGHT * level` points at
/// the full weight (the era's scaling only lowers it).
pub fn strike_cost(witnesses: usize, level: u8) -> Centi {
    witnesses as Centi * Centi::from(WARMONGER_NUKE_WEIGHT) * Centi::from(level.max(1)) * CENTI_PER_POINT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peace_with_an_aggressor_costs_and_war_on_it_pays_above_the_line_of_distrust_only() {
        assert_eq!(peace_price(DISTRUST_WARMONGER), 0);
        assert_eq!(war_bonus(50), 0);
        assert_eq!(peace_price(DISTRUST_WARMONGER + 400), 400 * CENTI_PER_POINT);
        assert_eq!(war_bonus(DISTRUST_WARMONGER + 400), 200 * CENTI_PER_POINT);
    }

    #[test]
    fn a_strike_costs_by_its_witnesses_and_its_level() {
        let one = strike_cost(2, 1);
        assert_eq!(one, 2 * i64::from(WARMONGER_NUKE_WEIGHT) * CENTI_PER_POINT);
        assert_eq!(strike_cost(2, 2), 2 * one);
        assert_eq!(strike_cost(0, 1), 0, "nobody to remember it");
    }
}
