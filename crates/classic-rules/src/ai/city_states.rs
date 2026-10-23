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

/// A gold gift and what it is worth (`priced_gifts`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PricedGift {
    pub city_state: crate::ids::PlayerId,
    pub gold: u32,
    pub value: crate::ai::value::Centi,
}

/// The gold gifts of the screen's sizes up to `gold` to city-states the
/// empire may ally (none at war with it, none allied to another
/// civilization), valued at the prices (`PriceModel::Uses`). An alliance
/// is a project: the gold to raise the influence to the allies' threshold,
/// then a point a round to hold it (influence falls a point a round), for
/// the ally's and the friend's bonuses and the happiness of the ally's
/// luxuries the empire lacks each round to the end. A gift is worth its
/// share of the project's value per gold.
pub fn priced_gifts(obs: &AIObservation<'_>, prices: &crate::ai::value::Prices, gold: u32) -> Vec<PricedGift> {
    use crate::knowledge::public::PlayerQueries;
    let mut out = Vec::new();
    if gold < SMALL_GIFT {
        return out;
    }
    let cities = obs.own_cities.len().max(1) as i64;
    let rounds = i64::from(prices.rounds_left);
    for info in &obs.city_states {
        let free = info.ally.is_none_or(|a| a == obs.player);
        let Some(terms) = obs.view.city_state_terms(info.player).filter(|_| free && !info.at_war) else { continue };
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
        for size in [SMALL_GIFT, MEDIUM_GIFT].into_iter().filter(|g| *g <= gold) {
            let points = i64::from(crate::world::city_states::gold_gift_influence(obs.round, size, invest).max(1));
            let climb = i64::from((terms.allies_at - info.influence).max(0) + 1);
            let project_gold = (climb + rounds) * i64::from(size) / points;
            let value = round * rounds * i64::from(size) / project_gold.max(1);
            out.push(PricedGift { city_state: info.player, gold: size, value });
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
