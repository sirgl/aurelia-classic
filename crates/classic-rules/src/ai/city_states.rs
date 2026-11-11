//! Minimal city-state decisions over `AIObservation` (M4 track C): gold
//! gifts towards friendship and alliance, pledges to friends, tribute from
//! city-states that are neither friends nor protected, Austria's buyout of
//! an old ally. Everything comes
//! from `AIObservation::city_states` (the city-state screen of a human).

use crate::command::Command;
use crate::knowledge::observation::{AIObservation, CityStateInfo};
use crate::world::city_states::{QuestKind, ALLIES_THRESHOLD};

/// Gold the AI keeps before it gives any away.
pub const GIFT_RESERVE: u32 = 300;
/// The gift sizes (own values, unverified).
pub const SMALL_GIFT: u32 = 250;
pub const MEDIUM_GIFT: u32 = 500;

/// How much a city-state is worth courting: a quest that a gift fulfils,
/// then the closest to the alliance; never one at war or already allied.
fn gift_value(info: &CityStateInfo, me: crate::ids::PlayerId) -> Option<i32> {
    if info.at_war || info.ally == Some(me) {
        return None;
    }
    let quest = info
        .quests
        .iter()
        .any(|q| matches!(q, QuestKind::GiveGold { .. } | QuestKind::Invest));
    Some(info.influence + if quest { 100 } else { 0 })
}

/// One gold gift a turn to the most valuable city-state while the treasury
/// is above the reserve.
pub fn gift_orders(obs: &AIObservation<'_>) -> Vec<Command> {
    let treasury = obs.economy.treasury;
    if treasury < GIFT_RESERVE + SMALL_GIFT {
        return Vec::new();
    }
    let gold = if treasury >= 2 * GIFT_RESERVE + MEDIUM_GIFT {
        MEDIUM_GIFT
    } else {
        SMALL_GIFT
    };
    let best = obs
        .city_states
        .iter()
        .filter_map(|info| gift_value(info, obs.player).map(|v| (v, info.player)))
        .filter(|(v, _)| *v < ALLIES_THRESHOLD + 100)
        .max_by_key(|(v, p)| (*v, std::cmp::Reverse(p.0)));
    best.map(|(_, city_state)| Command::GiftGold {
        player: obs.player,
        city_state,
        gold,
    })
    .into_iter()
    .collect()
}

/// Influence a plain city-state's influence falls a round above its resting point, in
/// points (the rules' `DROP_PER_TURN`; a hostile one drops 1.5, an aggressor's 2).
const DECAY_POINTS: i32 = -(crate::systems::city_states::turn::DROP_PER_TURN / 100);

/// Rounds the alliance must live on its own influence before a gift can follow the one
/// that is due: the rounds the income takes to pay the smallest gift (a negative income
/// never pays it), at least one, at most the rounds left.
pub fn gift_lead(obs: &AIObservation<'_>, rounds_left: u32) -> i32 {
    let income = obs.economy.income.gold.max(1) as u32;
    SMALL_GIFT.div_ceil(income).clamp(1, rounds_left.max(1)) as i32
}

/// The influence above which a gold gift is thrown away: the alliance's threshold and the
/// decay of the rounds `gift_lead` plus the round the gift itself falls in. Below it a gift
/// is due before the influence falls under the threshold; above it the influence it buys
/// decays before it is needed, so the gold goes to another city-state or to other uses.
pub fn gift_ceiling(allies_at: i32, lead: i32) -> i32 {
    allies_at + DECAY_POINTS * (lead + 1)
}

/// `next`: what one delegate is worth toward the World Leader vote, as the diplomatic victory is
/// priced by the grand strategy: the prize of a victory (`grand_strategy::prize`, the whole game's
/// score) times the chance one more delegate adds (`grand_strategy::delegate_chance`). 0 until
/// the congress counts the delegates the vote needs.
pub fn delegate_worth(obs: &AIObservation<'_>) -> crate::ai::value::Centi {
    use crate::knowledge::public::PlayerQueries;
    let chance = crate::ai::grand_strategy::delegate_chance(obs);
    if chance == 0 {
        return 0;
    }
    crate::ai::grand_strategy::prize(&obs.view.race(), obs) * chance / 1000
}

/// The delegates an alliance brings to the World Leader vote: the city-state's seat of the
/// session that holds it (`systems::congress::SESSIONS`).
pub(crate) fn ally_delegates() -> i64 {
    crate::systems::congress::SESSIONS.iter().filter(|s| s.united_nations).map(|s| i64::from(s.city_state_delegates)).max().unwrap_or(0)
}

/// The gold that raises an influence of `from` points to the alliance's threshold and one over
/// it (the alliance is taken by strictly more), in gifts of the smallest size at the rules' price
/// (`gold_gift_influence_at`, falling as the game goes on).
pub fn alliance_gold(obs: &AIObservation<'_>, from: i32) -> u32 {
    gold_for_points(obs, (ALLIES_THRESHOLD + 1 - from).max(0))
}

/// The gold that buys `points` of influence in gifts of the smallest size.
pub fn gold_for_points(obs: &AIObservation<'_>, points: i32) -> u32 {
    let per_gift = crate::world::city_states::gold_gift_influence_at(&obs.speed, obs.round, SMALL_GIFT, false, 0).max(1);
    (points.max(0) as u32 * SMALL_GIFT).div_ceil(per_gift as u32)
}

/// Points of influence a seat aims above its estimate of a rival's influence in its ally when it
/// takes the alliance in one lump (the winners' alliances hold 73 points at the median and 94 at the
/// third quartile at the last round of the bench, against 80 for the seat's own: `runs/m1`).
pub const LUMP_MARGIN: i32 = 10;

/// An alliance of a rival that would win the World Leader vote, as the seat can take it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GuardTarget {
    pub city_state: crate::ids::PlayerId,
    pub rival: crate::ids::PlayerId,
    /// What taking it is worth (`victory_watch::Diplomatic::flip_worth`).
    pub worth: crate::ai::value::Centi,
    /// The influence of the rival we estimate and must pass.
    pub beat: i32,
    /// Our turns before the session that holds the vote opens.
    pub rounds: u32,
    /// The gold of one gift that takes it now, aimed `LUMP_MARGIN` above the estimate.
    pub package: u32,
    /// The gold is there by the vote and the gifts can climb that far (one gift a turn).
    pub feasible: bool,
}

/// The smallest gift (in steps of 5 gold) that gives `points` of influence now.
pub fn lump_gold(obs: &AIObservation<'_>, points: i32, invest: bool) -> u32 {
    let gives = |g: u32| crate::world::city_states::gold_gift_influence_at(&obs.speed, obs.round, g, invest, 0);
    let (mut lo, mut hi) = (1u32, 400_000u32);
    if gives(hi) < points {
        return hi;
    }
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if gives(mid) >= points {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    lo.div_ceil(5) * 5
}

/// The alliances `prices.guard` makes worth buying: those of a rival whose chance to win the next
/// World Leader vote one alliance lowers (`victory_watch`), at war with none. The rival's influence is not
/// shown: it is taken to be what the seat's own alliances hold (they are held by the same economy), else
/// the ceiling a seat tops an alliance up to, never below the alliance's threshold nor what the seat
/// holds already. An alliance falls to strictly more influence.
pub fn guard_targets(obs: &AIObservation<'_>, prices: &crate::ai::value::Prices) -> Vec<GuardTarget> {
    use crate::knowledge::public::PlayerQueries;
    if prices.guard <= 0 {
        return Vec::new();
    }
    let Some(race) = crate::ai::victory_watch::diplomatic(obs) else { return Vec::new() };
    let step = ally_delegates();
    let lead = gift_lead(obs, prices.rounds_left);
    let held: Vec<i32> = obs.city_states.iter().filter(|c| c.ally == Some(obs.player)).map(|c| c.influence).collect();
    let income = i64::from(obs.economy.income.gold.max(0));
    let mut out = Vec::new();
    for info in &obs.city_states {
        let Some(rival) = info.ally.filter(|a| *a != obs.player && !info.at_war) else { continue };
        let worth = race.flip_worth(rival, prices.guard, step as i32);
        let Some(terms) = obs.view.city_state_terms(info.player).filter(|_| worth > 0) else { continue };
        let beat = if held.is_empty() { gift_ceiling(terms.allies_at, lead) } else { (held.iter().map(|v| i64::from(*v)).sum::<i64>() / held.len() as i64) as i32 }.max(terms.allies_at).max(info.influence);
        let invest = info.quests.iter().any(|q| matches!(q, QuestKind::Invest));
        let package = lump_gold(obs, (beat + LUMP_MARGIN + 1 - info.influence).max(1), invest);
        // One gift a turn (`Allocator::spend_gold`), the largest of them at most, with the influence
        // that falls a point a round until the vote.
        let hold = i64::from(race.rounds);
        let reach = i64::from(crate::world::city_states::gold_gift_influence_at(&obs.speed, obs.round, MEDIUM_GIFT, invest, 0).max(1)) * hold;
        let climb = i64::from((beat + 1 - info.influence).max(0)) + hold;
        let gold_by_then = i64::from(obs.economy.treasury) + income * (hold - 1).max(0);
        let feasible = gold_by_then >= i64::from(package) && (race.rounds <= 1 || climb <= reach);
        out.push(GuardTarget { city_state: info.player, rival, worth, beat, rounds: race.rounds, package, feasible });
    }
    out
}

/// The gold the treasury keeps for the alliance it can take most cheaply (the allocator's chest):
/// 0 when no rival is in reach or none of its alliances can be paid by the vote.
pub fn guard_chest(obs: &AIObservation<'_>, prices: &crate::ai::value::Prices) -> u32 {
    guard_targets(obs, prices).iter().filter(|t| t.feasible).map(|t| t.package).min().unwrap_or(0)
}

/// A gold gift and what it is worth (`priced_gifts`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PricedGift {
    pub city_state: crate::ids::PlayerId,
    pub gold: u32,
    pub value: crate::ai::value::Centi,
    /// The gift takes an alliance from a rival that would win the World Leader vote (`vote-guard`).
    pub contested: bool,
}

/// The gold gifts of the screen's sizes up to `gold` to city-states the
/// empire may ally (none at war with it, none allied to another
/// nation), valued at the prices (`PriceModel::Uses`). An alliance
/// is a project: the gold to raise the influence to the allies' threshold,
/// then a point a round to hold it (influence falls a point a round), for
/// the ally's and the friend's bonuses and the happiness of the ally's
/// luxuries the empire lacks each round to the end, and (`Prices::delegate`, `next`) its
/// delegates toward the World Leader vote. A gift is worth its share of the project's value per
/// gold.
pub fn priced_gifts(obs: &AIObservation<'_>, prices: &crate::ai::value::Prices, gold: u32) -> Vec<PricedGift> {
    use crate::knowledge::public::PlayerQueries;
    let mut out = Vec::new();
    if gold < SMALL_GIFT {
        return out;
    }
    let cities = obs.own_cities.len().max(1) as i64;
    let rounds = i64::from(prices.rounds_left);
    let lead = gift_lead(obs, prices.rounds_left);
    // `vote-guard`: the alliances of a rival that would win the World Leader vote, for sale at the price
    // of the influence that takes them (`guard_targets`).
    let targets = guard_targets(obs, prices);
    for info in &obs.city_states {
        let free = info.ally.is_none_or(|a| a == obs.player);
        let contest = targets.iter().find(|t| t.city_state == info.player);
        let Some(terms) = obs.view.city_state_terms(info.player).filter(|_| (free || contest.is_some()) && !info.at_war) else { continue };
        let lacking: i32 = obs
            .view
            .tradeable_luxuries(info.player)
            .iter()
            .filter(|(r, _)| obs.economy.resources.available(*r) <= 0)
            .filter_map(|(r, _)| obs.view.rules().resources.get(r.0 as usize))
            .map(|d| d.module.happiness)
            .sum();
        let round = round_value(prices, &terms.friends, 0, cities) + round_value(prices, &terms.allies, lacking, cities);
        let invest = info.quests.iter().any(|q| matches!(q, QuestKind::Invest));
        // A quest for gold pays its influence for any gift (`Invest` only makes a gift worth more).
        let quest = info.quests.iter().any(|q| matches!(q, QuestKind::GiveGold { .. }));
        if contest.is_none() && prices.gift_cap && !quest && info.influence >= gift_ceiling(terms.allies_at, lead) {
            continue;
        }
        let (target, hold, guard) = match contest {
            Some(t) => {
                if !t.feasible {
                    continue;
                }
                // The last turn before the session opens: one lump that takes the alliance whole, or
                // nothing (a gift that does not reach it is thrown away, and a rival answers a small one).
                if t.rounds <= 1 {
                    let votes = prices.delegate * ally_delegates() + t.worth;
                    out.push(PricedGift { city_state: info.player, gold: t.package, value: round * rounds + votes, contested: true });
                    continue;
                }
                (t.beat, i64::from(t.rounds), t.worth)
            }
            None => (terms.allies_at, rounds, 0),
        };
        for size in [SMALL_GIFT, MEDIUM_GIFT].into_iter().filter(|g| *g <= gold) {
            let points = i64::from(crate::world::city_states::gold_gift_influence_at(&obs.speed, obs.round, size, invest, 0).max(1));
            let climb = i64::from((target - info.influence).max(0) + 1);
            let project_gold = (climb + hold) * i64::from(size) / points;
            let votes = prices.delegate * ally_delegates() + guard;
            let value = (round * rounds + votes) * i64::from(size) / project_gold.max(1);
            out.push(PricedGift { city_state: info.player, gold: size, value, contested: guard > 0 });
        }
    }
    out
}

/// One round of a city-state level's bonuses (food in hundredths) and of
/// `luxuries` happiness, at the prices (a price is a yield a round to the
/// end).
fn round_value(prices: &crate::ai::value::Prices, bonus: &crate::knowledge::public::CityStateBonuses, luxuries: i32, cities: i64) -> crate::ai::value::Centi {
    let food = i64::from(bonus.capital_food) + i64::from(bonus.city_food) * cities;
    let happiness = i64::from(bonus.happiness + luxuries);
    let game = food * prices.food / 100 + i64::from(bonus.culture) * prices.culture + i64::from(bonus.faith) * prices.faith + happiness * prices.happiness;
    game / i64::from(prices.rounds_left.max(1))
}

/// Pledge to protect every friend not yet protected.
pub fn pledge_orders(obs: &AIObservation<'_>) -> Vec<Command> {
    obs.city_states
        .iter()
        .filter(|info| info.friends && !info.pledged && !info.at_war)
        .map(|info| Command::PledgeProtection {
            player: obs.player,
            city_state: info.player,
            pledge: true,
        })
        .collect()
}

/// Tribute from city-states that would pay and that we do not court
/// (influence at or below zero, nobody's protégé of ours).
pub fn tribute_orders(obs: &AIObservation<'_>) -> Vec<Command> {
    obs.city_states
        .iter()
        .filter(|info| info.can_bully_gold && info.influence <= 0 && !info.pledged)
        .map(|info| Command::BullyGold {
            player: obs.player,
            city_state: info.player,
        })
        .take(1)
        .collect()
}

/// Buy out the first city-state we may (Austria): its cities and units
/// are worth more than the gold.
pub fn buyout_orders(obs: &AIObservation<'_>) -> Vec<Command> {
    obs.city_states
        .iter()
        .filter(|info| info.can_buyout)
        .map(|info| Command::BuyoutCityState {
            player: obs.player,
            city_state: info.player,
        })
        .take(1)
        .collect()
}

/// The city-states system's advisor (`ai::system`): pledges and tribute at
/// the start of the turn, gifts at its end (after production spent gold).
#[derive(Debug, Clone, Copy, Default)]
pub struct CityStatesAdvisor;

impl crate::ai::system::SystemAdvisor for CityStatesAdvisor {
    fn start_of_turn(
        &self,
        input: &crate::ai::system::AdvisorInput<'_, '_>,
    ) -> Vec<crate::ai::system::Proposal> {
        let obs = input.observation;
        let mut out = crate::ai::combat::proposals(20, buyout_orders(obs));
        out.extend(crate::ai::combat::proposals(15, pledge_orders(obs)));
        out.extend(crate::ai::combat::proposals(5, tribute_orders(obs)));
        out
    }

    fn end_of_turn(
        &self,
        input: &crate::ai::system::AdvisorInput<'_, '_>,
    ) -> Vec<crate::ai::system::Proposal> {
        // Priced gifts are the allocator's purchases (`PriceModel::Uses`).
        let priced = input.brief.prices.is_some_and(|p| p.model == crate::ai::value::PriceModel::Uses);
        if priced {
            return Vec::new();
        }
        crate::ai::combat::proposals(3, gift_orders(input.observation))
    }
}
