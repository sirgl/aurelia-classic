//! Peace by value (M6.2 step 6, classic/docs/ai-leaders.md "Peace by value"): the
//! leaders (`leaders` and the game's AI, `standard`) end a war with a
//! nation only by a peace deal, and value the deal in the
//! allocator's measure — hundredths of points of the score, a capital's
//! share of a victory included (`Stakes`).
//!
//! Why: the AI before it (`allocator`) walks out of any war it does not
//! fight 15 rounds after it began (`Command::MakePeace` needs nobody's
//! consent in these rules), so a victim ends a siege before it can
//! succeed, and nobody pays or asks anything for peace. Peace between
//! nations is a deal both sides accept (classic/docs/classic/rules.md); here
//! each side takes peace when going on with the war is worth less to it than
//! the terms.
//!
//! What a war promises (`WarProspect`): our operation against the enemy —
//! its chance times what its capture brings, less the army it still costs
//! (`operations::running_case`) — and the enemy's army seen near our
//! cities, which takes the city it threatens with the chance of an
//! operation. The leader's appetite weighs the gain and the loss
//! (`Stakes::weigh`, its aggression); nothing else of the personality.
//! The enemy's side is estimated from the same facts the other way round
//! (its army and personality are not known). Everything reads the
//! player's observation, its memory and the trade screen's checks.

use crate::ai::grand_strategy::Stakes;
use crate::ai::operations::{city_worth, running_case, WarCase, SIEGE_ROUNDS_ESTIMATE};
use crate::ai::planner::{unit_power, AiRole, TurnBrief};
use crate::ai::planner_strategy::THREAT_RADIUS;
use crate::ai::state::AiState;
use crate::ai::system::AdvisorInput;
use crate::ai::value::{Centi, Prices};
use crate::command::Command;
use crate::ids::{PlayerId, TileId};
use crate::knowledge::observation::AIObservation;
use crate::knowledge::public::PlayerQueries;
use crate::rules::CompiledRuleset;
use crate::world::player::PlayerKind;
use crate::world::relations::{DealItem, DealOffer};

/// How an AI ends its wars with nations.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PeaceMaking {
    /// By its own `MakePeace` when its strategy says so (the game's AI:
    /// a war no operation fights ends after `planner_strategy::MIN_WAR_ROUNDS`);
    /// it accepts any peace offered after `diplomacy::ACCEPT_PEACE_AFTER_ROUNDS`.
    #[default]
    Alone,
    /// By a peace deal both sides value (this module); `MakePeace` only
    /// with city-states.
    ByDeal,
}

/// What going on with a war against one enemy promises the player.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WarProspect {
    /// Our running operation against the enemy (zero without one).
    pub attack: WarCase,
    /// What the enemy's army seen near our cities is expected to take.
    pub defence: Centi,
}

impl WarProspect {
    pub fn of(brief: &TurnBrief, state: &AiState, rules: &CompiledRuleset, prices: &Prices, stakes: &Stakes, enemy: PlayerId, view: Option<&crate::knowledge::view::PlayerView<'_>>) -> Self {
        // An operation against the enemy, else the raid on its land that still pays.
        let attack = running_case(brief, state, rules, prices, stakes, enemy)
            .or_else(|| view.and_then(|v| crate::ai::raid::running_case(brief, state, rules, prices, v, enemy)))
            .unwrap_or_default();
        Self {
            attack,
            defence: expected_loss(brief, state, rules, prices, enemy),
        }
    }

    /// Going on as the leader weighs it: its appetite weighs the gain, the
    /// rest of the scale the loss.
    pub fn value(&self, stakes: &Stakes) -> Centi {
        self.attack.value(stakes) + stakes.weigh(-self.defence)
    }

    /// The enemy's side as the player can tell it: what we expect to lose
    /// is its gain, what we expect to take its loss.
    pub fn enemy_value(&self) -> Centi {
        self.defence - self.attack.gain
    }
}

/// The enemy's army seen near us (its strongest power in the memory) takes
/// the city it threatens with the chance of an operation, scaled down when
/// our units near that city are stronger; we lose what the city is worth.
fn expected_loss(brief: &TurnBrief, state: &AiState, rules: &CompiledRuleset, prices: &Prices, enemy: PlayerId) -> Centi {
    let threat = state.relation(enemy).map_or(0, |m| m.threat);
    let Some(city) = threatened(brief, state, enemy).filter(|_| threat > 0) else {
        return 0;
    };
    let near = |tile: TileId| brief.dims.distance(tile, city.tile).is_some_and(|d| d <= THREAT_RADIUS);
    let defenders: u32 = brief.units.iter().filter(|u| u.role == AiRole::Military && near(u.tile)).map(|u| unit_power(rules, u.kind, u.hp)).sum();
    let chance = crate::ai::genome::get(crate::ai::genome::Knob::SuccessPercent) * i64::from(threat) / i64::from(threat.max(defenders));
    chance * city_worth(prices, city.population, SIEGE_ROUNDS_ESTIMATE) / 100
}

/// Our city nearest to the enemy's military units in sight, else to its
/// cities we know.
fn threatened<'b>(brief: &'b TurnBrief, state: &AiState, enemy: PlayerId) -> Option<&'b crate::ai::planner::CityBrief> {
    let units = brief.sightings.iter().filter(|s| s.owner == enemy && s.military).map(|s| s.tile);
    let cities = state.seen_cities().iter().filter(|c| c.owner == enemy).map(|c| c.tile);
    let nearest = |tiles: Vec<TileId>| {
        let distance = |c: &crate::ai::planner::CityBrief| tiles.iter().filter_map(|t| brief.dims.distance(c.tile, *t)).min();
        brief.cities.iter().filter_map(|c| Some((distance(c)?, c.id, c))).min_by_key(|(d, id, _)| (*d, *id)).map(|(_, _, c)| c)
    };
    nearest(units.collect()).or_else(|| nearest(cities.collect()))
}

/// A leader's side of the turn's deals: its prices and stakes, by which it
/// values every deal.
pub struct Negotiation<'i, 'o, 'a> {
    input: &'i AdvisorInput<'o, 'a>,
    prices: Prices,
    stakes: Stakes,
}

impl<'i, 'o, 'a> Negotiation<'i, 'o, 'a> {
    /// The leader's prices of the turn and its stakes (its personality and
    /// the capitals that bring a victory closer).
    pub fn new(input: &'i AdvisorInput<'o, 'a>) -> Self {
        let obs = input.observation;
        let prices = input.brief.prices.unwrap_or_else(|| Prices::of(obs, input.rules, Default::default()));
        let personality = input.rules.module.personality(input.brief.nation);
        let stakes = Stakes::of(obs, input.brief, input.state, &personality);
        Self { input, prices, stakes }
    }

    fn obs(&self) -> &'o AIObservation<'a> {
        self.input.observation
    }

    pub fn stakes(&self) -> &Stakes {
        &self.stakes
    }

    pub fn prospect(&self, enemy: PlayerId) -> WarProspect {
        let input = self.input;
        WarProspect::of(input.brief, input.state, input.rules, &self.prices, &self.stakes, enemy, Some(&input.observation.view))
    }

    /// `gold` in hundredths of score points: a gold now is the price of a
    /// gold a turn spread over the rounds left.
    fn gold(&self, gold: i64) -> Centi {
        gold * self.prices.gold / i64::from(self.prices.rounds_left)
    }

    /// Gold worth at least `value`.
    fn gold_for(&self, value: Centi) -> u32 {
        let value = (value * i64::from(self.prices.rounds_left)).max(0) as u64;
        value.div_ceil(self.prices.gold.max(1) as u64).min(u64::from(u32::MAX)) as u32
    }

    /// What `items` with `other` are worth to the player: gold by the
    /// price of gold; a city by what it brings or costs; peace by giving up
    /// the war's prospect; an embassy we receive by the capital it reveals
    /// (`found`) on top of the advisor's scale; the rest by the advisor's
    /// gold scale (`diplomacy::item_value`) at the price of gold. Peace
    /// counts once, however many sides give it.
    pub fn value(&self, other: PlayerId, items: &[(PlayerId, DealItem)]) -> Centi {
        let me = self.obs().player;
        // Peace with an aggressor (a nuclear strike, a conquest) costs more: `ai::infamy`.
        let aggressor = self.obs().relation(other).map_or(0, |r| crate::ai::infamy::peace_price(r.warmonger));
        let peace = if items.iter().any(|(_, item)| *item == DealItem::Peace) { -self.prospect(other).value(&self.stakes) - aggressor } else { 0 };
        let terms: Centi = items
            .iter()
            .map(|(giver, item)| {
                let receiving = *giver != me || item.is_mutual();
                let sign = if receiving { 1 } else { -1 };
                match *item {
                    DealItem::Peace => 0,
                    DealItem::Gold(gold) => sign * self.gold(i64::from(gold)),
                    DealItem::GoldPerTurn(gold) => {
                        // The game speed's deal (`DEAL_DURATION` at Standard).
                        let rounds = self.input.observation.speed.deal_rounds.min(self.prices.rounds_left);
                        sign * self.gold(i64::from(gold) * i64::from(rounds))
                    }
                    DealItem::City(city) => sign * self.city(city).unwrap_or_else(|| self.gold(600)),
                    DealItem::Embassy if receiving => self.found(other) + self.scale(other, *item, receiving),
                    _ => self.scale(other, *item, receiving),
                }
            })
            .sum();
        peace + terms
    }

    /// An item by the advisor's gold scale (`diplomacy::item_value`) at
    /// the price of gold.
    fn scale(&self, other: PlayerId, item: DealItem, receiving: bool) -> Centi {
        let agreements = crate::ai::diplomacy::agreement_prices(self.input);
        self.gold(crate::ai::diplomacy::item_value_priced(self.obs(), self.input.rules, agreements, other, item, receiving))
    }

    /// What an embassy in `host`'s capital reveals that the leader values
    /// (`Stakes::finding`).
    pub fn found(&self, host: PlayerId) -> Centi {
        self.stakes.finding(host)
    }

    /// What a city in the deal is worth: one of ours its worth, one we see
    /// what its capture would bring without the fight.
    fn city(&self, city: crate::ids::CityHandle) -> Option<Centi> {
        let obs = self.obs();
        if let Some(own) = obs.own_cities.iter().find(|c| c.handle == city) {
            return Some(city_worth(&self.prices, own.population, 0));
        }
        let seen = obs.visible_foreign_cities.iter().find(|c| c.handle == city)?;
        let unrest = (5 + i64::from(seen.population.max(1))) * self.prices.happiness * self.stakes.unrest / 100;
        Some(city_worth(&self.prices, seen.population, 0) - unrest)
    }

    pub fn accepts(&self, offer: &DealOffer) -> bool {
        self.value(offer.from, &offer.items) >= 0
    }

    /// The peace the player offers `enemy`: it asks the most the enemy
    /// would give by its estimate, never less than going on is worth to
    /// itself — gold one way or the other, white peace at zero. Gold the
    /// enemy lacks halves until the deal passes or stops paying for the
    /// war; gold we lack is what we have. None: no such deal passes.
    pub fn peace_offer(&self, enemy: PlayerId) -> Option<Vec<(PlayerId, DealItem)>> {
        let me = self.obs().player;
        let prospect = self.prospect(enemy);
        let ours = prospect.value(&self.stakes);
        let ask = ours.max(-prospect.enemy_value());
        let deal = |giver: PlayerId, gold: u32| {
            let mut items = vec![(me, DealItem::Peace)];
            if gold > 0 {
                items.push((giver, DealItem::Gold(gold)));
            }
            items
        };
        let passes = |items: &[(PlayerId, DealItem)]| self.obs().view.offer_problem(enemy, items).is_none();
        if ask <= 0 {
            let gold = self.gold_for(-ask).min(self.obs().economy.treasury);
            return Some(deal(me, gold)).filter(|d| passes(d));
        }
        let mut gold = self.gold_for(ask);
        loop {
            let items = deal(enemy, gold);
            if self.gold(i64::from(gold)) < ours {
                return None;
            }
            if passes(&items) {
                return Some(items);
            }
            if gold == 0 {
                return None;
            }
            gold /= 2;
        }
    }
}

/// Answers to every offer addressed to the leader, by value.
pub fn answer_offers(input: &AdvisorInput<'_, '_>) -> Vec<Command> {
    let obs = input.observation;
    if obs.offers_to_us.is_empty() {
        return Vec::new();
    }
    let negotiation = Negotiation::new(input);
    obs.offers_to_us
        .iter()
        .map(|offer| Command::RespondDeal { player: obs.player, offer: offer.id, accept: negotiation.accepts(offer) })
        .collect()
}

/// Embassies the leader asks of the nations whose capital it values
/// and has not found (`Negotiation::found`): theirs, with ours when they
/// lack one, when the deal is worth it to the leader and possible now.
/// `skip`: partners that already have an offer of ours this turn.
pub fn embassy_offers(input: &AdvisorInput<'_, '_>, skip: &[PlayerId]) -> Vec<Command> {
    let obs = input.observation;
    let me = obs.player;
    let candidates: Vec<&crate::knowledge::observation::Relation> = obs
        .relations
        .iter()
        .filter(|r| r.kind == PlayerKind::Major && !r.at_war && !r.our_embassy && !skip.contains(&r.other))
        .filter(|r| !obs.offers_to_us.iter().any(|o| o.from == r.other))
        .collect();
    if candidates.is_empty() {
        return Vec::new();
    }
    let negotiation = Negotiation::new(input);
    candidates
        .into_iter()
        .filter(|r| negotiation.found(r.other) > 0)
        .filter_map(|r| {
            let mut items = vec![(r.other, DealItem::Embassy)];
            if !r.their_embassy {
                items.push((me, DealItem::Embassy));
            }
            let worth = negotiation.value(r.other, &items) >= 0;
            (worth && obs.view.offer_problem(r.other, &items).is_none()).then_some(Command::ProposeDeal { from: me, to: r.other, items })
        })
        .collect()
}

/// A peace offer to every nation at war with the leader that has no
/// offer of its own waiting for our answer this turn.
pub fn peace_offers(input: &AdvisorInput<'_, '_>) -> Vec<Command> {
    let obs = input.observation;
    let enemies: Vec<PlayerId> = obs
        .relations
        .iter()
        .filter(|r| r.at_war && r.kind == PlayerKind::Major)
        .filter(|r| !obs.offers_to_us.iter().any(|o| o.from == r.other))
        .map(|r| r.other)
        .collect();
    if enemies.is_empty() {
        return Vec::new();
    }
    let negotiation = Negotiation::new(input);
    enemies
        .into_iter()
        .filter_map(|enemy| Some(Command::ProposeDeal { from: obs.player, to: enemy, items: negotiation.peace_offer(enemy)? }))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Going on is the attack's weighed gain less its army and the weighed
    /// loss; the enemy's side is the same facts the other way round.
    #[test]
    fn a_war_is_worth_its_gain_less_its_loss() {
        let prospect = WarProspect { attack: WarCase { gain: 1_000, cost: 200 }, defence: 300 };
        assert_eq!(prospect.value(&Stakes::neutral()), 500);
        let eager = Stakes { appetite: 160, ..Stakes::neutral() };
        assert_eq!(prospect.value(&eager), 1_600 - 200 - 120);
        let reluctant = Stakes { appetite: 40, ..Stakes::neutral() };
        assert_eq!(prospect.value(&reluctant), 400 - 200 - 480);
        assert_eq!(prospect.enemy_value(), -700);
        assert_eq!(WarProspect::default().value(&eager), 0, "an idle war is worth nothing: white peace ends it");
    }
}
