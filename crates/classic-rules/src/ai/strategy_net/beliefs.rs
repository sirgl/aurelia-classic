//! The belief head: the founder, follower, enhancer and reformation beliefs of a religion (and a bonus
//! belief at the founding) by what their declared effects are worth to us, at the plan's prices (`net2`'s
//! part `Beliefs`; every other seat keeps the fixed lists of `ai::belief_lists`).
//!
//! No belief is named here. Each effect of a belief (`BeliefDefinition::effects`) is read by its kind, as
//! the policy head reads a policy's (`heads::choose_policy`):
//!
//! - **In our cities** (a following city's or its tiles' yield, growth, happiness, combat near it, tourism):
//!   our following cities are our cities, the holy city is the capital, a founder's effect is ours, a gate on
//!   a city's followers is a gate on its citizens (`in_our_frame`); the cell's run with the effects in every
//!   own city and the classifier's value of the others (`effects::Valuer`), times the share of our cities
//!   that follow (`Reach::own_share`).
//! - **Per following city, per follower** (the founder's gold, faith, culture and happiness): the cities
//!   and followers the religion reaches (`Reach`: ours, and the foreign ones as far as the rivals' religions
//!   reach now, at least `weights::belief::FOREIGN_CITIES`).
//! - **Once per event** (a city converted, a spread, a great person expended): the events of the rest of the
//!   game (`events`), at the one-off price of the yield.
//! - **A number of the religion** (its pressure, distance, missionaries' and prophets' strength, erosion,
//!   retention): the foreign following cities it adds (`weights::belief::SPREAD`) at what one pays us — the
//!   founder beliefs the religion holds, or takes with it (`per_city`); a cut of the missionaries' or the
//!   prophets' faith: the faith it saves (`MISSIONARY_FAITH`, `PROPHET_FAITH`).
//! - **Items for faith** (buildings, land units, great people): worth nothing, no executor buys them; a
//!   faith-purchase executor adds its own valuation with it.
//!
//! Every price is the plan's (`Plan::mult` over the allocator's): the intent weighs the aims — a faith or
//! a culture style, a war posture's combat — as it weighs a policy's. Pure over the model and the reach:
//! the scenario tests call it.

use super::cell::{run, CellPrices, Place, Plan as CellPlan};
use super::effects::Valuer;
use super::model::Model;
use super::weights::belief as w;
use super::weights::pd;
use super::{heads, Base, Plan};
use crate::classic::effects::{ClassicCondition, ClassicCount, ClassicModifier, ClassicSelector, ClassicTrigger, ReligionAbility, ReligionStat};
use crate::ids::BeliefId;
use crate::knowledge::observation::AIObservation;
use crate::rules::effects::{CompiledEffect, Condition, Count, EffectSet, Modifier, PerCount, Selector, Trigger};
use crate::rules::stats::Stat;
use crate::rules::CompiledRuleset;

/// How far the religion reaches over the rest of the game, as the observation shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reach {
    /// Percent of our cities that follow it.
    pub own_share: i64,
    pub own_cities: i64,
    /// Our citizens that follow it.
    pub own_followers: i64,
    pub foreign_cities: i64,
    pub foreign_followers: i64,
    /// Our cities with a specialist at work (a specialist's yield).
    pub specialist_cities: i64,
    /// Our faith a turn.
    pub faith: i64,
}

impl Reach {
    /// The reach of the observed player's religion: as it stands when one is founded (at least the
    /// priors abroad: a religion keeps spreading); before, the priors at home and the rivals' religions'
    /// mean reach abroad (at least the priors).
    pub fn of(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Reach {
        let faith = &obs.faith.overview;
        let own_cities = obs.own_cities.len() as i64;
        let citizens: i64 = obs.own_cities.iter().map(|c| i64::from(c.population)).sum();
        let specialist_cities = obs.own_cities.iter().filter(|c| has_specialist(rules, c)).count() as i64;
        let ours = faith.religion.and_then(|r| faith.religions.iter().find(|s| s.religion == r));
        let (own_share, own_followers, abroad) = match ours {
            Some(s) => {
                let home = i64::from(s.counts.cities.saturating_sub(s.counts.foreign_cities));
                let share = (100 * home / own_cities.max(1)).clamp(0, 100);
                let followers = i64::from(s.counts.followers.saturating_sub(s.counts.foreign_followers));
                (share, followers, (i64::from(s.counts.foreign_cities), i64::from(s.counts.foreign_followers)))
            }
            None => {
                let others: Vec<_> = faith.religions.iter().map(|s| (i64::from(s.counts.foreign_cities), i64::from(s.counts.foreign_followers))).collect();
                let n = others.len().max(1) as i64;
                let mean = (others.iter().map(|o| o.0).sum::<i64>() / n, others.iter().map(|o| o.1).sum::<i64>() / n);
                (w::OWN_SHARE, citizens * w::OWN_SHARE / 100, mean)
            }
        };
        let foreign_cities = abroad.0.max(w::FOREIGN_CITIES);
        let foreign_followers = abroad.1.max(foreign_cities * w::CITY_FOLLOWERS);
        Reach { own_share, own_cities, own_followers, foreign_cities, foreign_followers, specialist_cities, faith: i64::from(faith.per_turn) }
    }

    /// Our following cities.
    fn home(&self) -> i64 {
        self.own_cities * self.own_share / 100
    }
}

/// The city works a specialist of a kind that is not the default one (a citizen that works no tile, in a
/// slot of its buildings).
fn has_specialist(rules: &CompiledRuleset, city: &crate::knowledge::observation::OwnCity) -> bool {
    let default = rules.roles.default_specialist;
    let slots = crate::ai::science_levers::slots(rules, city);
    let open = slots.iter().enumerate().any(|(k, n)| *n > 0 && Some(crate::rules::content::SpecialistId(k as u16)) != default);
    open && crate::ai::science_levers::idle_citizens(city) > 0
}

/// A belief's effects read in our frame: our following cities are our cities (and their tiles), the holy
/// city is the capital, the founder is the owner, a gate on a city's followers is a gate on its citizens.
pub fn in_our_frame(set: &EffectSet) -> EffectSet {
    let frame = |e: &CompiledEffect| {
        let mut e = *e;
        e.selector = match e.selector {
            Selector::Module(ClassicSelector::FollowingCities) => Selector::OwnerCities,
            Selector::Module(ClassicSelector::FollowingCityTiles) => Selector::OwnerCityTiles,
            Selector::Module(ClassicSelector::HolyCity) => Selector::Capital,
            Selector::Module(ClassicSelector::Founder) => Selector::Owner,
            other => other,
        };
        let followers = |c: Condition| match c {
            Condition::Module(ClassicCondition::MinFollowers(n)) => Condition::MinPopulation(n),
            other => other,
        };
        e.condition = followers(e.condition);
        e.second_condition = followers(e.second_condition);
        e
    };
    let map = |v: &[CompiledEffect]| v.iter().map(frame).collect::<Vec<_>>();
    EffectSet {
        passive_player: map(&set.passive_player),
        passive_module: map(&set.passive_module),
        passive_city: map(&set.passive_city),
        passive_unit: map(&set.passive_unit),
        passive_tile: map(&set.passive_tile),
        triggered: set.triggered.iter().map(|v| map(v)).collect(),
    }
}

/// Every effect of a belief: the passive ones (the religion's own, `Scope::Module`, too) and the triggered.
fn effects_of(set: &EffectSet) -> impl Iterator<Item = &CompiledEffect> {
    super::cell::effects_of(set).chain(&set.passive_module)
}

/// The head over one decision's model, plan and reach.
pub struct Head<'a> {
    model: &'a Model,
    rules: &'a CompiledRuleset,
    base: &'a Base,
    mult: &'a [super::Milli; pd::N],
    reach: Reach,
    prices: CellPrices,
    valuer: Valuer<'a>,
    horizon: i64,
    /// The cell's run without the belief.
    v0: i64,
}

/// One belief's worth and its parts (the trace prints them).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Worth {
    /// The cell's run in our following cities, the classifier's value of the other effects there.
    pub cities: i64,
    /// Per following city and follower, per event, the religion's numbers, the items for faith.
    pub religion: i64,
}

impl Worth {
    pub fn total(&self) -> i64 {
        self.cities + self.religion
    }
}

impl<'a> Head<'a> {
    pub fn new(model: &'a Model, rules: &'a CompiledRuleset, plan: &'a Plan, base: &'a Base, reach: Reach) -> Self {
        let horizon = i64::from(plan.intent.horizon.max(1));
        let prices = CellPrices::of(base, &plan.mult).scaled(plan.intent.sharp);
        let room = i64::from(plan.city_target.saturating_sub(model.cities.len() as u32));
        let valuer = Valuer { model, rules, base, mult: &plan.mult, horizon: base.rounds_left.max(1), room, fight: heads::fight(&plan.intent) };
        let v0 = run(model, &CellPlan::new(model), &prices, horizon).value;
        Head { model, rules, base, mult: &plan.mult, reach, prices, valuer, horizon, v0 }
    }

    /// What `belief` brings us; `founders`: the founder beliefs the religion holds or takes with it (what a
    /// foreign following city pays us).
    pub fn worth(&self, belief: BeliefId, founders: &[BeliefId]) -> Worth {
        let Some(def) = self.rules.module.beliefs.get(usize::from(belief.0)) else { return Worth::default() };
        let ours = in_our_frame(&def.effects);
        let mut plan = CellPlan::new(self.model);
        plan.add(self.model, self.rules, &ours, Place::Empire, 0);
        let cell = run(self.model, &plan, &self.prices, self.horizon).value - self.v0;
        // The founder's own effects (a city-state number, a spy's) are the player's: they do not wait for
        // our cities to follow.
        let (player, cities): (Vec<&CompiledEffect>, Vec<&CompiledEffect>) = effects_of(&ours).partition(|e| matches!(e.selector, Selector::Owner));
        let at_home = (cell + cities.iter().map(|e| self.valuer.effect(e, Place::Empire)).sum::<i64>()) * self.reach.own_share / 100;
        let own = player.iter().map(|e| self.valuer.effect(e, Place::Empire)).sum::<i64>();
        let per_city = self.per_city(founders);
        let religion = effects_of(&def.effects).map(|e| self.religion(e, &self.reach, per_city)).sum::<i64>();
        Worth { cities: at_home + own, religion }
    }

    /// The best of `free` (none of `taken`), with every candidate's worth; a tie goes to the earlier of the
    /// table.
    pub fn choose(&self, free: &[BeliefId], taken: &[BeliefId], founders: &[BeliefId]) -> (Option<BeliefId>, Vec<(BeliefId, Worth)>) {
        let mut scored: Vec<(BeliefId, Worth)> = free.iter().filter(|b| !taken.contains(b)).map(|b| (*b, self.worth(*b, founders))).collect();
        scored.sort_by_key(|(b, _)| b.0);
        let best = scored.iter().fold(None::<(BeliefId, i64)>, |best, (b, v)| match best {
            Some((_, top)) if top >= v.total() => best,
            _ => Some((*b, v.total())),
        });
        (best.map(|(b, _)| b), scored)
    }

    /// What one more foreign following city pays us a turn by `founders`, at the plan's prices.
    fn per_city(&self, founders: &[BeliefId]) -> i64 {
        let one = Reach { own_share: 0, own_cities: 0, own_followers: 0, foreign_cities: 1, foreign_followers: w::CITY_FOLLOWERS, specialist_cities: 0, faith: 0 };
        founders
            .iter()
            .filter_map(|b| self.rules.module.beliefs.get(usize::from(b.0)))
            .flat_map(|d| effects_of(&d.effects))
            .filter(|e| matches!(e.modifier, Modifier::Module(ClassicModifier::AddYieldPerFollowingCity { .. } | ClassicModifier::AddYieldPerFollowers { .. } | ClassicModifier::AddHappinessPerFollowingCity { .. })))
            .map(|e| self.religion(e, &one, 0))
            .sum()
    }

    /// A yield a turn (hundredths) kept to the end, at the plan's price of its slot.
    fn yield_turn(&self, kind: crate::rules::effects::YieldKind, hundredths: i64) -> i64 {
        let slot = super::cell::slot(kind);
        self.base.slot(slot) * hundredths / 100 * super::slot_mult(self.mult, slot) / 1000
    }

    /// A yield once (hundredths), at the plan's price of its slot.
    fn yield_once(&self, kind: crate::rules::effects::YieldKind, hundredths: i64) -> i64 {
        self.yield_turn(kind, hundredths) / self.base.rounds_left.max(1)
    }

    fn happiness_turn(&self, hundredths: i64) -> i64 {
        self.base.happiness * hundredths / 100 * self.mult[pd::HAPPINESS] / 1000
    }

    /// The events of the rest of the game a triggered effect fires on (0: one the head has no count of).
    fn events(&self, trigger: Trigger, reach: &Reach) -> i64 {
        match trigger {
            // Every following city but the holy city turned once.
            Trigger::Module(ClassicTrigger::OnCityConverted) => (reach.foreign_cities + reach.own_cities * reach.own_share / 100 - 1).max(0),
            Trigger::Module(ClassicTrigger::OnReligionSpread) => reach.foreign_cities * w::SPREADS_PER_CITY,
            Trigger::Module(ClassicTrigger::OnGreatPersonExpended) => w::PEOPLE_EXPENDED,
            _ => 0,
        }
    }

    /// Percent of the rest of the game a gate of a religion's effect holds: a technology known, or to come.
    fn gate(&self, condition: Condition) -> i64 {
        match condition {
            Condition::HasTechnology(t) if !self.model.techs.contains(&t) => w::LATER,
            Condition::LacksTechnology(t) if self.model.techs.contains(&t) => 0,
            _ => 100,
        }
    }

    /// An effect the cell and the classifier have no model of: what it pays by the religion's reach, while
    /// its gates hold.
    fn religion(&self, e: &CompiledEffect, reach: &Reach, per_city: i64) -> i64 {
        self.religion_always(e, reach, per_city) * self.gate(e.condition) / 100 * self.gate(e.second_condition) / 100
    }

    fn religion_always(&self, e: &CompiledEffect, reach: &Reach, per_city: i64) -> i64 {
        let home = reach.home();
        if !matches!(e.trigger, Trigger::Passive) {
            let events = self.events(e.trigger, reach);
            return match e.modifier {
                Modifier::AddYield { yield_kind, amount } => events * self.yield_once(yield_kind, i64::from(amount) * 100),
                // Science per follower of the other creeds of the city turned (a city of `CITY_FOLLOWERS`).
                Modifier::AddYieldPerCount { yield_kind, amount, per } => events * self.yield_once(yield_kind, i64::from(per.scale(amount, w::CITY_FOLLOWERS as u32)) * 100),
                _ => 0,
            };
        }
        match e.modifier {
            Modifier::Module(ClassicModifier::AddYieldPerFollowingCity { yield_kind, amount, foreign_only }) => {
                let cities = reach.foreign_cities + if foreign_only { 0 } else { home };
                self.yield_turn(yield_kind, i64::from(amount) * 100 * cities)
            }
            Modifier::Module(ClassicModifier::AddYieldPerFollowers { yield_kind, per, foreign_only }) => {
                let followers = reach.foreign_followers + if foreign_only { 0 } else { reach.own_followers };
                self.yield_turn(yield_kind, 100 * followers / i64::from(per.max(1)))
            }
            Modifier::Module(ClassicModifier::AddHappinessPerFollowingCity { hundredths }) => self.happiness_turn(i64::from(hundredths) * (home + reach.foreign_cities)),
            Modifier::Module(ClassicModifier::AddHappinessPerCount { amount, per }) => match per.count {
                Count::Module(ClassicCount::PeacefulForeignFollowers) => self.happiness_turn(i64::from(scale(per, amount, reach.foreign_followers)) * 100),
                _ => 0,
            },
            Modifier::Module(ClassicModifier::ModifyReligion { stat, percent }) => self.religion_number(stat, percent, reach, per_city),
            // A yield per world wonder of a following city.
            Modifier::AddYieldPerCount { yield_kind, amount, per } if per.count == Count::WorldWonders => {
                let wonders: i64 = self.model.cities.iter().map(|c| i64::from(scale(per, amount, self.wonders_in(c)))).sum();
                self.yield_turn(yield_kind, wonders * 100) * reach.own_share / 100
            }
            // A percent of a yield per follower of a following city's majority (its citizens).
            Modifier::AddYieldPercentPerCount { yield_kind, percent, per } if per.count == Count::Module(ClassicCount::MajorityFollowers) => {
                let slot = super::cell::slot(yield_kind);
                let made: i64 = self.model.cities.iter().map(|c| self.city_yield(c, slot) * i64::from(scale(per, percent, c.population))).sum::<i64>() / 100;
                self.yield_turn(yield_kind, made) * reach.own_share / 100
            }
            Modifier::AddStat { stat: Stat::AnySpecialistYield(yield_kind), amount } => self.yield_turn(yield_kind, i64::from(amount) * 100 * reach.specialist_cities) * reach.own_share / 100,
            _ => 0,
        }
    }

    /// A number of the religion changed by `percent`: the foreign following cities it adds at `per_city`
    /// a turn each, or the faith its cut saves.
    fn religion_number(&self, stat: ReligionStat, percent: i32, reach: &Reach, per_city: i64) -> i64 {
        let percent = i64::from(percent);
        let saved = |share: i64| self.base.faith * reach.faith * share / 100 * (-percent).max(0) / 100 * self.mult[pd::FAITH] / 1000;
        match stat {
            ReligionStat::MissionaryCost => saved(w::MISSIONARY_FAITH),
            ReligionStat::ProphetCost => saved(w::PROPHET_FAITH),
            other => {
                let Some((_, cities)) = w::SPREAD.iter().find(|(s, _)| *s == other) else { return 0 };
                let per = if other == ReligionStat::FriendlyCityStatePressure { self.model.city_states } else { 1 };
                per_city * cities * per * percent.max(0) / 100 / 1000
            }
        }
    }

    fn wonders_in(&self, city: &super::model::CityModel) -> i64 {
        city.buildings.iter().filter(|b| self.rules.buildings.get(usize::from(b.0)).is_some_and(|d| d.wonder)).count() as i64
    }

    /// A city's yield of a slot a turn, hundredths (production as made, the empire's science and culture
    /// shared over its cities, the rest beyond its tiles).
    fn city_yield(&self, city: &super::model::CityModel, slot: usize) -> i64 {
        let cities = self.model.cities.len().max(1) as i64;
        match slot {
            1 => city.production * 100,
            3 => self.model.science * 100 / cities,
            4 => self.model.culture * 100 / cities,
            _ => city.extra[slot] * 100,
        }
    }
}

/// `per.scale` on an i64 count.
fn scale(per: PerCount, amount: i32, counted: i64) -> i32 {
    per.scale(amount, counted.clamp(0, i64::from(u32::MAX)) as u32)
}
