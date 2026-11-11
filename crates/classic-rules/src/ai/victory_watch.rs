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

/// Alliances a rival can add in `rounds`: what our own economy would buy (`flips_in`: a rival's gold
/// is not shown), and at least one alliance for every session interval the rounds span. The bench's
/// mirror games show the leaders taking about one alliance a session interval whatever the economy
/// of the seat that reads them (`runs/m1.md`), so a rival one alliance short is never out of reach.
pub fn rival_flips(obs: &AIObservation<'_>, rounds: u32) -> i64 {
    let interval = i64::from(kernel::rules::speeds::SpeedValues::scale(last_session().turns_between, obs.speed.pace_percent).max(1));
    let floor = if rounds == 0 { 0 } else { (i64::from(rounds) + interval - 1) / interval };
    flips_in(obs, rounds).max(floor)
}

/// The diplomatic race (None before the United Nations sits, with the vote open, or without
/// the delegates' table).
pub fn diplomatic(obs: &AIObservation<'_>) -> Option<Diplomatic> {
    let rounds = rounds_to_vote(obs)?;
    let c = obs.congress.as_ref()?;
    // Alliances a rival can still take: the city-states we know, not its own.
    let known = obs.city_states.len() as i64;
    let reach = rival_flips(obs, rounds).min(known);
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

/// Dev switches of the vote guard's parts while they are tuned (`VG_STEP`, bits: 1 the alliances of
/// a rival, 2 the host election, 4 the coups); all on when unset. Gone with the freeze.
pub fn part(bit: u32) -> bool {
    use std::sync::OnceLock;
    static BITS: OnceLock<u32> = OnceLock::new();
    *BITS.get_or_init(|| std::env::var("VG_STEP").ok().and_then(|v| v.parse().ok()).unwrap_or(7)) & bit != 0
}

/// Whom the seat votes for as host of the league (a `ChangeHost` election at a special session),
/// when the nation with the most delegates it knows could win the World Leader vote with the host's
/// seats and the seat is not that nation: the strongest of the others, itself included. The host holds
/// the seats of `SpecialSession::host_delegates` for good (until the next election), so denying them to
/// the front-runner costs it as many alliances; every guard reads the same table, so they vote
/// for the same nation and carry the plurality. None: vote for itself, as the plain AI does.
pub fn host_choice(obs: &AIObservation<'_>) -> Option<PlayerId> {
    let c = obs.congress.as_ref().filter(|c| c.united_nations && c.needed > 0 && c.delegates.len() > 1)?;
    if !obs.victories.enabled(crate::world::victory::VictoryKind::Diplomatic) {
        return None;
    }
    let front = c.delegates.iter().max_by_key(|(p, d)| (*d, std::cmp::Reverse(p.0)))?.0;
    if front == obs.player {
        return None;
    }
    // With the host's seats (it holds them already, or the election gives them) and the alliances it could
    // still take before the leader vote (two session intervals: the vote may be the second session on),
    // could it win? The ramp of the diplomatic race.
    let interval = kernel::rules::speeds::SpeedValues::scale(last_session().turns_between, obs.speed.pace_percent).max(1);
    let reach = rival_flips(obs, 2 * interval).min(obs.city_states.len() as i64);
    let seats = last_session().host_delegates;
    let delegates = c.delegates.iter().find(|(p, _)| *p == front)?.1;
    let base = if c.host == Some(front) { delegates.saturating_sub(seats) } else { delegates };
    if chance_for(i32::from(c.needed) - i32::from(base + seats), reach) == 0 {
        return None;
    }
    c.delegates.iter().filter(|(p, _)| *p != front).max_by_key(|(p, d)| (*d, std::cmp::Reverse(p.0))).map(|(p, _)| *p)
}

/// A city-state allied to a rival whose alliance one fewer would lower its chance to win the next vote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Contested {
    pub city_state: PlayerId,
    pub rival: PlayerId,
    /// What taking it is worth (`Diplomatic::flip_worth`).
    pub worth: Centi,
}

/// The alliances of the rivals that can win the next vote, dearest first, and our turns before the
/// session that holds it opens. Empty while no rival can win (nothing else is computed then).
pub fn contested(obs: &AIObservation<'_>) -> (Vec<Contested>, u32) {
    let prize = prize_of_race(obs);
    if prize <= 0 {
        return (Vec::new(), 0);
    }
    let Some(race) = diplomatic(obs) else { return (Vec::new(), 0) };
    let step = crate::ai::city_states::ally_delegates() as i32;
    let mut out: Vec<Contested> = obs
        .city_states
        .iter()
        .filter_map(|info| {
            let rival = info.ally.filter(|a| *a != obs.player && !info.at_war)?;
            let worth = race.flip_worth(rival, prize, step);
            (worth > 0).then_some(Contested { city_state: info.player, rival, worth })
        })
        .collect();
    out.sort_by_key(|c| (std::cmp::Reverse(c.worth), c.city_state.0));
    (out, race.rounds)
}

/// What losing the game to a rival's vote costs the seat, in the measure of the allocator
/// (`grand_strategy::prize`): 0 while no rival can win the next vote (no race, or every rival out
/// of reach), so that nothing else is computed then.
pub fn guard_prize(obs: &AIObservation<'_>) -> Centi {
    if !part(1) {
        return 0;
    }
    prize_of_race(obs)
}

fn prize_of_race(obs: &AIObservation<'_>) -> Centi {
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
