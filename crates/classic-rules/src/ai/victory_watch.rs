//! What a seat knows of its rivals' road to a victory (the vote guard, `Variant::VoteGuard`).
//!
//! The tracker estimates, for every nation the seat has met, how far it is from each victory, from
//! what the rules show a player and nothing else:
//!
//! - **Diplomatic** (acted on): the Congress screen lists the delegates of the nation and of every
//!   nation it met (`CongressInfo::delegates`), the delegates the World Leader needs, the rounds to
//!   the next session and whether it elects the leader; the city-state screen names the ally of every
//!   city-state met. A rival's rounds to the vote are the rounds to the session that opens it
//!   (delegates are fixed when a session opens, `systems::congress::open_session`). Not shown, so not
//!   used: a rival's gold, its influence with a city-state, a nation not met.
//! - **Domination, science** (readings only): the original capitals each nation holds as far as the
//!   seat has seen them, and the era of each nation met against the spaceship's era.
//!
//! The chance that a rival wins the vote is a ramp over its gap to the threshold: a rival at the
//! threshold wins (every AI votes for itself), one short by `g` delegates wins if it can buy `g`
//! alliances before the session opens. How many it can buy is the delegates the rounds left allow
//! at the pace the seat's own economy pays an alliance (`flips_in`): an estimate from the rules'
//! price of an alliance (`gold_gift_influence_at`) and the seat's income, since a rival's gold is
//! not shown. Everything is a read of the observation: no state is kept between turns.

use crate::ai::grand_strategy::Permille;
use crate::ai::value::Centi;
use crate::ids::PlayerId;
use crate::knowledge::observation::AIObservation;

/// A rival's standing in the diplomatic race.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reading {
    pub rival: PlayerId,
    /// Its delegates now (the Congress screen).
    pub delegates: u16,
    /// Delegates short of the threshold (zero or negative: it has enough).
    pub gap: i32,
    /// Its chance to win the next World Leader vote.
    pub chance: Permille,
}

/// The diplomatic race as the seat sees it before the next World Leader vote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diplomatic {
    /// Delegates the World Leader needs.
    pub needed: u16,
    /// Our turns before the session that holds the vote opens (the last one is the turn before
    /// the opening round); 0: the session is open, the delegates are fixed.
    pub rounds: u32,
    /// Alliances a rival can buy in those rounds at the pace of our economy.
    pub reach: i64,
    /// Our own standing (not a rival: `gap` is ours).
    pub own: Reading,
    /// The rivals we have met, strongest first.
    pub rivals: Vec<Reading>,
}

/// The chance of a rival `gap` delegates short with `reach` alliances in reach (permille): 1000 at
/// or over the threshold, falling evenly to 0 at `reach + 1` short.
pub fn chance_for(gap: i32, reach: i64) -> Permille {
    if gap <= 0 {
        return 1000;
    }
    (1000 * (reach + 1 - i64::from(gap)) / (reach + 1)).clamp(0, 1000)
}

/// Rounds of our turns before the next World Leader vote opens: the session timer when the next
/// session elects, one more interval when it does not. None: the vote cannot come (no United
/// Nations, victory off), or the session is open.
pub fn rounds_to_vote(obs: &AIObservation<'_>) -> Option<u32> {
    let c = obs.congress.as_ref().filter(|c| c.united_nations && c.needed > 0)?;
    if !obs.victories.enabled(crate::world::victory::VictoryKind::Diplomatic) || c.in_session {
        return None;
    }
    let interval = kernel::rules::speeds::SpeedValues::scale(last_session().turns_between, obs.speed.pace_percent).max(1);
    Some(if c.leader_vote_next { c.turns_until_session } else { c.turns_until_session + interval })
}

fn last_session() -> crate::systems::congress::SpecialSession {
    crate::systems::congress::SESSIONS[crate::systems::congress::SESSIONS.len() - 1]
}

/// Alliances bought in `rounds` by an economy of `income` a round: the gold of the rounds over the
/// gold of one alliance, climbed from nothing to the alliance's threshold at the gift the rules
/// price (`city_states::alliance_gold`).
pub fn flips_in(obs: &AIObservation<'_>, rounds: u32) -> i64 {
    let income = i64::from(obs.economy.income.gold.max(0));
    let price = i64::from(crate::ai::city_states::alliance_gold(obs, 0)).max(1);
    income * i64::from(rounds) / price
}

/// The diplomatic race (None before the United Nations sits, with the vote open, or without
/// the delegates' table).
pub fn diplomatic(obs: &AIObservation<'_>) -> Option<Diplomatic> {
    let rounds = rounds_to_vote(obs)?;
    let c = obs.congress.as_ref()?;
    // Alliances a rival can still take: the city-states we know, not its own.
    let known = obs.city_states.len() as i64;
    let reach = flips_in(obs, rounds).min(known);
    let reading = |(rival, delegates): (PlayerId, u16)| {
        let gap = i32::from(c.needed) - i32::from(delegates);
        Reading { rival, delegates, gap, chance: chance_for(gap, reach) }
    };
    let own = c.delegates.iter().copied().find(|(p, _)| *p == obs.player).map(reading)?;
    let mut rivals: Vec<Reading> = c.delegates.iter().copied().filter(|(p, _)| *p != obs.player).map(reading).collect();
    rivals.sort_by_key(|r| (std::cmp::Reverse(r.chance), std::cmp::Reverse(r.delegates), r.rival.0));
    Some(Diplomatic { needed: c.needed, rounds, reach, own, rivals })
}

impl Diplomatic {
    /// The rival's reading.
    pub fn of(&self, rival: PlayerId) -> Option<&Reading> {
        self.rivals.iter().find(|r| r.rival == rival)
    }

    /// What taking one alliance from `rival` is worth, in the measure of `prize` (what losing the
    /// game costs): the fall of its chance to win the vote, the more so the less we are ahead
    /// ourselves. 0 for a rival far from the threshold (beyond the ramp) and for one so far over it
    /// that one alliance does not bring it under. `step`: the delegates an alliance brings.
    pub fn flip_worth(&self, rival: PlayerId, prize: Centi, step: i32) -> Centi {
        let Some(r) = self.of(rival) else { return 0 };
        let fall = chance_for(r.gap, self.reach) - chance_for(r.gap + step, self.reach);
        // The more we are ahead ourselves, the less its fall matters to us.
        prize * fall / 1000 * (1000 - self.own.chance) / 1000
    }
}

/// What losing the game to a rival's vote costs the seat, in the measure of the allocator
/// (`grand_strategy::prize`): 0 while no rival can win the next vote (no race, or every rival out
/// of reach), so that nothing else is computed then.
pub fn guard_prize(obs: &AIObservation<'_>) -> Centi {
    use crate::knowledge::public::PlayerQueries;
    let Some(race) = diplomatic(obs) else { return 0 };
    if race.rivals.iter().all(|r| r.chance == 0) {
        return 0;
    }
    crate::ai::grand_strategy::prize(&obs.view.race(), obs)
}

/// How far a rival is from the other victories, as far as the seat knows (readings, not acted on).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Other {
    pub rival: PlayerId,
    /// Original capitals it holds, of all the game counts.
    pub capitals_held: u8,
    pub capitals_all: u8,
    /// Its era against the spaceship's era (negative: eras to go).
    pub era_vs_space: i32,
}

/// The readings for the victories not acted on: domination (the capitals each nation holds, as the
/// seat has seen them) and science (the era each nation met has reached against the spaceship's).
pub fn others(obs: &AIObservation<'_>, space_era: usize) -> Vec<Other> {
    use crate::knowledge::public::PlayerQueries;
    let capitals = obs.view.original_capitals();
    let all = capitals.len().min(255) as u8;
    obs.view
        .met_eras()
        .into_iter()
        .map(|(rival, era)| Other {
            rival,
            capitals_held: capitals.iter().filter(|c| c.holder == rival).count().min(255) as u8,
            capitals_all: all,
            era_vs_space: i32::from(era) - space_era as i32,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ramp_is_one_at_the_threshold_and_zero_beyond_reach() {
        assert_eq!(chance_for(-3, 2), 1000);
        assert_eq!(chance_for(0, 2), 1000);
        assert_eq!(chance_for(1, 2), 666);
        assert_eq!(chance_for(2, 2), 333);
        assert_eq!(chance_for(3, 2), 0);
        assert_eq!(chance_for(5, 2), 0);
        assert_eq!(chance_for(1, 0), 0);
    }
}
