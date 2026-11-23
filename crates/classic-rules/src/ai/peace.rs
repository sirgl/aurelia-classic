//! Peace by value (M6.2 step 6, classic/docs/ai-leaders.md "Peace by value"): every AI
//! ends a war with a nation only by a peace deal, and values the deal in the
//! allocator's measure — hundredths of points of the score, a capital's
//! share of a victory included (`Stakes`). It answers every offer addressed to
//! it so (`answer_offers`), a human's too.
//!
//! Why: peace between nations is a deal the other side accepts
//! (classic/docs/classic/rules.md; the rules refuse `Command::MakePeace` between
//! two nations since 12 November). Before, the AI without leaders (`allocator`)
//! walked out of any war it did not fight 15 rounds after it began, so a victim
//! ended a siege before it could succeed, and nobody paid or asked anything for
//! peace. Here each side takes peace when going on with the war is worth less to
//! it than the terms.
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
use crate::world::relations::{DealItem, DealOffer, PeaceRefusal};

/// What a foreign city of `population` citizens brings when it becomes ours without the fight: its worth less the
/// unrest of a conquered city (the deal's price of a city, and the war order's, `ordered_case`).
fn seen_city_worth(prices: &Prices, stakes: &Stakes, population: u32) -> Centi {
    let unrest = (5 + i64::from(population.max(1))) * prices.happiness * stakes.unrest / 100;
    city_worth(prices, population, 0) - unrest
}

/// What a `net2` war order on `enemy` promises (`net2::orders`, CHAIN.md D8): the capture of the order rival's easiest
/// city it still holds (`Net2Plan::order_eases`, the aim first) at its ease, with no army cost (the order's army is the
/// war machine's to form). The war machine's price of the capture and the deal's price of the city each cross 0 at some
/// small sizes (HD8 #1: a city of 1 citizen priced 2 947 by the one and below 0 by the other, of 2 citizens the other way
/// round): the larger of the two is the city's worth to a war that is to take it. None without an order on it or a
/// city of it we have seen.
fn ordered_case(brief: &TurnBrief, state: &AiState, prices: &Prices, stakes: &Stakes, enemy: PlayerId) -> Option<WarCase> {
    let eases = crate::ai::operations::with_net2(brief, crate::ai::net2::plan::Part::War, |p| p.war_on(enemy).map(|_| p.order_eases.clone()))??;
    let mut best: Option<WarCase> = None;
    for (tile, ease) in eases {
        let Some(city) = state.seen_cities().iter().find(|c| c.tile == tile && c.owner == enemy) else { continue };
        let puppet = city.population < crate::ai::combat::ANNEX_POPULATION;
        let captured = crate::ai::operations::captured_gain(prices, stakes, city.population, prices.rounds_left, puppet);
        let worth = captured.max(seen_city_worth(prices, stakes, city.population));
        let case = WarCase { gain: (worth * ease / 1000).max(0), cost: 0 };
        if best.is_none_or(|b| case.gain > b.gain) {
            best = Some(case);
        }
    }
    best
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
        let mut attack = running_case(brief, state, rules, prices, stakes, enemy)
            .or_else(|| view.and_then(|v| crate::ai::raid::running_case(brief, state, rules, prices, v, enemy)))
            .unwrap_or_default();
        // THE ONE place a war's going on is priced (CHAIN.md D8, tac/runs/C1.md). With `TurnBrief::war_at_peace`
        // (`Variant::values_war_at_peace`): an assault with no operation is the operation it is (`assault_case`), and a
        // `net2` war order is worth at least what it promises (`ordered_case`: the order decides the war, the case only
        // ranks, `operations::order_decides`), with no operation running or with one whose case is below it.
        if brief.war_at_peace {
            let assault = crate::ai::operations::assault_case(brief, state, rules, prices, stakes, enemy);
            let ordered = ordered_case(brief, state, prices, stakes, enemy).filter(|_| crate::ai::net2::orders::PRICES_PEACE);
            for case in [assault, ordered].into_iter().flatten() {
                if case.value(stakes) > attack.value(stakes) {
                    attack = case;
                }
            }
        }
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

/// The rounds from now in which the rules keep a flow from `from` running: no declaration of war passes while a
/// peace treaty runs (`validate_declare_war`), and a war is what ends a running deal (no command cancels one,
/// `systems/diplomacy`). The peace treaty is the one running, or the one the deal makes (`PEACE_DURATION` at the
/// speed) when it holds the peace item.
pub fn guaranteed_rounds(obs: &AIObservation<'_>, from: PlayerId, items: &[(PlayerId, DealItem)]) -> u32 {
    let running = obs.relation(from).map_or(0, |r| r.peace_treaty_until.saturating_sub(obs.round));
    let made = if items.iter().any(|(_, item)| *item == DealItem::Peace) { obs.speed.peace_rounds } else { 0 };
    running.max(made)
}

/// A deal in which we pay now: our gold or one of our cities (what a break of the deal does not give back).
fn pays_now(me: PlayerId, items: &[(PlayerId, DealItem)]) -> bool {
    items.iter().any(|(giver, item)| *giver == me && matches!(item, DealItem::Gold(g) if *g > 0) || *giver == me && matches!(item, DealItem::City(_)))
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

    /// The war with `enemy` as the player weighs it. A war `net2` wants peace in (`Net2Plan::peace_with`: no order
    /// on it and no reason left) promises no attack: ending it costs nothing of ours, only the loss it still threatens.
    pub fn prospect(&self, enemy: PlayerId) -> WarProspect {
        let input = self.input;
        let mut prospect = WarProspect::of(input.brief, input.state, input.rules, &self.prices, &self.stakes, enemy, Some(&input.observation.view));
        if input.brief.net2_for(crate::ai::net2::plan::Part::Peace).is_some_and(|p| p.makes_peace_with(enemy)) {
            prospect.attack = WarCase::default();
        }
        prospect
    }

    /// `value` of an item that flows to us later (gold a turn, a resource, a neighbour's open borders), read by a
    /// `net2` plan at the trust in the giver (§4.6, P4: the buyer who paid a sale that the seller then broke by war):
    /// `trust`/1000 of it, nothing below `TRUST_MIN`. As today without a plan.
    fn flows_later(&self, from: PlayerId, value: Centi) -> Centi {
        match self.input.brief.net2_for(crate::ai::net2::plan::Part::Deals) {
            Some(plan) if plan.trust(from) < crate::ai::net2::plan::TRUST_MIN => 0,
            Some(plan) => value * plan.trust(from) / 1000,
            None => value,
        }
    }

    /// `flows_later` of a flow we pay for now (`prepaid`: our gold or a city in the same deal), read by a `net2` plan:
    /// only the rounds the rules guarantee count (`guaranteed_rounds`), the rest at the plan's credit, which is 0
    /// on these rules (design §4.6 completed, CHAIN.md D1/D2: the swindler of the first sale is a friend, trust 1000,
    /// and declares war the next round; the rules end the flow at the war and keep the gold, `diplomacy/mod.rs:124`,
    /// with no backstab memory). A flow exchanged for a flow (a luxury swap) is not prepaid: a break ends both.
    /// As today without a plan.
    fn prepaid_flow(&self, from: PlayerId, value: Centi, items: &[(PlayerId, DealItem)]) -> Centi {
        let Some(plan) = self.input.brief.net2_for(crate::ai::net2::plan::Part::Deals) else { return value };
        let rounds = self.input.observation.speed.deal_rounds.min(self.prices.rounds_left).max(1);
        let sure = guaranteed_rounds(self.obs(), from, items).min(rounds);
        let rest = rounds - sure;
        let trusted = self.flows_later(from, value);
        trusted * i64::from(sure) / i64::from(rounds) + trusted * i64::from(rest) / i64::from(rounds) * plan.credit(from) / 1000
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
    /// price of gold; a city by what it brings or costs; a resource by what its copies
    /// give for the deal's rounds (`resource`); peace by giving up
    /// the war's prospect; an embassy we receive by the capital it reveals
    /// (`found`) on top of the advisor's scale; the rest by the advisor's
    /// gold scale (`diplomacy::item_value_with`) at the price of gold. Peace
    /// counts once, however many sides give it.
    pub fn value(&self, other: PlayerId, items: &[(PlayerId, DealItem)]) -> Centi {
        let me = self.obs().player;
        // Peace with an aggressor (a nuclear strike, a conquest) costs more: `ai::infamy`.
        let aggressor = self.obs().relation(other).map_or(0, |r| crate::ai::infamy::peace_price(r.warmonger));
        let peace = if items.iter().any(|(_, item)| *item == DealItem::Peace) { -self.prospect(other).value(&self.stakes) - aggressor } else { 0 };
        let prepaid = pays_now(me, items);
        let later = |worth: Centi| if prepaid { self.prepaid_flow(other, worth, items) } else { self.flows_later(other, worth) };
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
                        let worth = self.gold(i64::from(gold) * i64::from(rounds));
                        if receiving {
                            later(worth)
                        } else {
                            -worth
                        }
                    }
                    // A city we cannot see is worth nothing we can tell; ours we always see.
                    DealItem::City(city) => sign * self.city(city).unwrap_or(0),
                    DealItem::Resource { resource, amount } => {
                        let worth = self.resource(other, resource, amount, receiving);
                        if receiving {
                            later(worth)
                        } else {
                            worth
                        }
                    }
                    DealItem::Embassy if receiving => self.found(other) + self.scale(other, *item, receiving),
                    _ => self.scale(other, *item, receiving),
                }
            })
            .sum();
        peace + terms
    }

    /// An item by the advisor's gold scale (`diplomacy::item_value_with`) at
    /// the price of gold.
    fn scale(&self, other: PlayerId, item: DealItem, receiving: bool) -> Centi {
        let agreements = crate::ai::diplomacy::agreement_prices(self.input);
        // `strategist-x`: no friendship, research agreement, pact or open borders for the rival running away.
        if crate::ai::diplomacy::helps(item) && crate::ai::diplomacy::denied_item(self.input.brief, other, item) {
            return self.gold(crate::ai::diplomacy::REFUSED);
        }
        let value = crate::ai::diplomacy::item_value_with(self.obs(), self.input.rules, agreements, self.input.brief.net2_for(crate::ai::net2::plan::Part::Deals), other, item, receiving);
        self.gold(value)
    }

    /// What `amount` copies of `resource` received (`receiving`) from `other` or given to it for
    /// the speed's deal are worth to the player, by its prices of the turn and the rules' data:
    /// - a luxury: the happiness of its kind (`module.happiness`) for the deal's rounds, at the
    ///   price of happiness, while the empire lacks the kind; further copies add nothing;
    /// - a strategic resource: per copy, the production of the dearest unit the player can
    ///   build that needs it (cost over the copies it needs), at the price of production, while
    ///   the empire has no free copy;
    /// - the rival's side counts `RivalPercent` of the same worth, as `city_worth` counts a
    ///   rival's gain: what it gains by our copies (a luxury only when it has none to trade of
    ///   the kind) is our loss, what it loses by giving its last tradeable copies of a luxury is
    ///   our gain. A swap of last copies is even; a gift of a spare copy is not free.
    fn resource(&self, other: PlayerId, resource: crate::ids::ResourceId, amount: u16, receiving: bool) -> Centi {
        let obs = self.obs();
        let ours = &obs.economy.resources;
        let rounds = i64::from(obs.speed.deal_rounds.min(self.prices.rounds_left));
        let rounds_left = i64::from(self.prices.rounds_left.max(1));
        let Some(def) = self.input.rules.resources.get(resource.0 as usize) else { return 0 };
        let luxury = def.class == Some(crate::rules::content::ResourceClass::Luxury);
        let amount = i64::from(amount);
        // The worth of `copies` copies to one who has none free.
        let worth = |copies: i64| -> Centi {
            match def.class {
                Some(crate::rules::content::ResourceClass::Luxury) => {
                    i64::from(def.module.happiness.max(0)) * self.prices.happiness * rounds / rounds_left * copies.min(1)
                }
                Some(crate::rules::content::ResourceClass::Strategic) => {
                    copies * self.copy_production(resource) * self.prices.production / rounds_left
                }
                _ => 0,
            }
        };
        let rival = |copies: i64| worth(copies) * crate::ai::genome::get(crate::ai::genome::Knob::RivalPercent) / 100;
        let theirs = obs.view.tradeable_luxuries(other).iter().find(|(r, _)| *r == resource).map_or(0, |(_, n)| i64::from(*n));
        let free = i64::from(ours.available(resource).max(0));
        if receiving {
            let ours = if free > 0 { 0 } else { worth(amount) };
            let their_loss = if luxury && theirs <= amount { rival(amount) } else { 0 };
            return ours + their_loss;
        }
        // `strategist-x`: no strategic copy for the rival running away (a part of its spaceship may
        // wait for it; Vox Populi's rule of no aluminium to a nation close to the spaceship, by class).
        if !luxury && crate::ai::diplomacy::denied(self.input.brief, other) {
            return self.gold(crate::ai::diplomacy::REFUSED);
        }
        let lost = (amount - (free - 1).max(0)).clamp(0, amount);
        let their_gain = if !luxury || theirs == 0 { rival(amount) } else { 0 };
        -(worth(lost) + their_gain)
    }

    /// The production one copy of a strategic `resource` stands for: the dearest unit the
    /// player's technologies allow (not obsolete, not a nation's own) that needs it, its cost
    /// over the copies it needs. With no such unit yet, the unit the rules open next: the one
    /// of the least advanced technology the player lacks (the earlier era, then the cheaper), priced the
    /// same way; 0 only for a resource no unit of the rules needs.
    fn copy_production(&self, resource: crate::ids::ResourceId) -> i64 {
        let techs = &self.obs().economy.techs;
        let known = |t: Option<crate::ids::TechId>| t.is_some_and(|t| techs.contains(&t));
        let per_copy = |u: &crate::rules::compiled::UnitDefinition| {
            let (_, copies) = u.required_resources.iter().find(|(r, _)| *r == resource)?;
            Some(i64::from(u.cost) / i64::from((*copies).max(1)))
        };
        let needing = || {
            self.input
                .rules
                .units
                .iter()
                .filter(|u| !u.unbuildable && u.nation.is_none() && !known(u.obsolete_tech))
                .filter(|u| u.required_resources.iter().any(|(r, _)| *r == resource))
        };
        let now = needing()
            .filter(|u| u.required_tech.is_none_or(|t| techs.contains(&t)))
            .filter_map(per_copy)
            .max();
        if let Some(now) = now {
            return now;
        }
        needing()
            .filter_map(|u| {
                let tech = u.required_tech?;
                let def = self.input.rules.technologies.get(usize::from(tech.0))?;
                Some(((def.era, def.cost, u.cost), per_copy(u)?))
            })
            .min_by_key(|(key, _)| *key)
            .map_or(0, |(_, worth)| worth)
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
        Some(seen_city_worth(&self.prices, &self.stakes, seen.population))
    }



    pub fn accepts(&self, offer: &DealOffer) -> bool {
        self.value(offer.from, &offer.items) >= 0
    }

    /// The answer to `offer` and, for a refused offer that makes peace, why and what the peace
    /// itself costs in gold. All of it comes from the value the decision is made by (`value`):
    /// the war's prospect, the aggressor's price, the terms. The price is the gold that makes
    /// the white peace a yes (`gold_for` of what it is short), whatever else the offer holds;
    /// the reason is the larger of the war's prospect and the aggressor's price; when the white
    /// peace would pass, the terms asked besides it are what is refused.
    pub fn answer(&self, offer: &DealOffer) -> (bool, PeaceRefusal) {
        // What the rules would refuse is refused: gold a turn beyond what the income carries
        // (`deals::gold_income`), copies or gold the player lacks.
        if self.obs().view.offer_problem(offer.from, &offer.items).is_some() {
            return (false, PeaceRefusal::Unsaid);
        }
        let value = self.value(offer.from, &offer.items);
        let makes_peace = offer.items.iter().any(|(_, item)| *item == DealItem::Peace);
        if value >= 0 || !makes_peace {
            return (value >= 0, PeaceRefusal::Unsaid);
        }
        let white = self.value(offer.from, &[(offer.from, DealItem::Peace)]);
        if white >= 0 {
            return (false, PeaceRefusal::Terms);
        }
        let price = self.gold_for(-white);
        let going_on = self.prospect(offer.from).value(&self.stakes).max(0);
        let aggressor = self.obs().relation(offer.from).map_or(0, |r| crate::ai::infamy::peace_price(r.warmonger));
        let why = if aggressor > going_on { PeaceRefusal::Aggressor { price } } else { PeaceRefusal::Winning { price } };
        (false, why)
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
        .map(|offer| {
            let (accept, why) = negotiation.answer(offer);
            crate::ai::tricks_trace::line(|| {
                let prospect = negotiation.prospect(offer.from);
                let order = input.brief.net2.as_ref().and_then(|p| p.war_on(offer.from)).map(|o| format!("{{\"kind\":\"{}\",\"value\":{},\"fighting\":{}}}", o.kind.name(), o.value, o.war_since().is_some()));
                let trust = input.brief.net2.as_ref().map(|p| p.trust(offer.from));
                let eases = input.brief.net2.as_ref().map(|p| format!("{:?}", p.order_eases)).unwrap_or_default();
                let seen: Vec<(u32, u32)> = input.state.seen_cities().iter().filter(|c| c.owner == offer.from).map(|c| (c.tile.0, c.population)).collect();
                format!(
                    "{{\"ev\":\"answer\",\"r\":{},\"p\":{},\"from\":{},\"items\":\"{:?}\",\"accept\":{accept},\"value\":{},\"attack_gain\":{},\"attack_cost\":{},\"defence\":{},\"order\":{},\"trust\":{:?},\"peace_with\":{},\"eases\":\"{eases}\",\"seen\":\"{seen:?}\"}}",
                    obs.round,
                    obs.player.0,
                    offer.from.0,
                    offer.items,
                    negotiation.value(offer.from, &offer.items),
                    prospect.attack.gain,
                    prospect.attack.cost,
                    prospect.defence,
                    order.unwrap_or_else(|| "null".to_owned()),
                    trust,
                    input.brief.net2.as_ref().is_some_and(|p| p.makes_peace_with(offer.from))
                )
            });
            Command::RespondDeal { player: obs.player, offer: offer.id, accept, why }
        })
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
        .filter(|r| !obs.offers_to_us.iter().any(|o| o.from == r.other) && !obs.waiting_on(r.other))
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
        .filter(|r| !obs.offers_to_us.iter().any(|o| o.from == r.other) && !obs.waiting_on(r.other))
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
