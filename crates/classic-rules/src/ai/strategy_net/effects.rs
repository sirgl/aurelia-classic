//! Layer 3, the effects of a candidate for us: what an item's effects are
//! worth within the horizon H at the net's prices, beyond the yields and the
//! growth the economy cell counts (`cell`). Each effect goes to one price
//! dimension (`weights::pd`) by its kind in the rules: happiness, a free unit
//! (a settler is expansion, a worker improvements, a great person a
//! technology's points, a trade unit a route), a grant, a cost cut, trade
//! routes, great person points, tourism, city-state favour, combat. The
//! constants are `weights::effect`.

use super::cell::{effects_of, Place};
use super::model::Model;
use super::weights::{effect as w, pd};
use super::{Base, Milli};
use crate::classic::effects::{ClassicCost, ClassicGrant, ClassicModifier};
use crate::rules::effects::{CompiledEffect, CostTarget, EffectSet, Grant, Modifier, Scope};
use crate::rules::stats::{ClassicStat, Stat};
use crate::rules::CompiledRuleset;

/// What the classifier reads.
pub struct Valuer<'a> {
    pub model: &'a Model,
    pub rules: &'a CompiledRuleset,
    pub base: &'a Base,
    pub mult: &'a [Milli; pd::N],
    /// Rounds the value is counted over (the rounds left: the allocator's
    /// measure, a change kept to the last round).
    pub horizon: i64,
    /// New cities the plan still wants (a free settler beyond them is worth
    /// nothing, as the allocator prices a city beyond its cap).
    pub room: i64,
    /// How much the plan fights (thousandths): danger, or the shares of the
    /// postures "prepare" and "war". A combat bonus pays only in fights.
    pub fight: Milli,
}

impl Valuer<'_> {
    /// A one-off unit of a yield slot at its net price (centi-points).
    fn unit(&self, slot: usize) -> i64 {
        self.base.slot(slot) * super::slot_mult(self.mult, slot) / 1000 / self.base.rounds_left.max(1)
    }

    /// A per-turn amount kept for H rounds, valued at `price` a turn kept to
    /// the end (the allocator's measure), times a dimension's multiplier.
    fn within(&self, price: i64, amount: i64, dim: usize) -> i64 {
        price * amount / 100 * self.horizon / self.base.rounds_left.max(1) * self.mult[dim] / 1000
    }

    /// A great person point at the net's price: a technology's points over
    /// `POINTS_PER_PERSON`.
    fn point(&self) -> i64 {
        self.base.tech * self.mult[pd::GREAT_PEOPLE] / 1000 / w::POINTS_PER_PERSON
    }

    fn cities_reached(&self, e: &CompiledEffect, place: Place) -> i64 {
        match (e.scope, e.selector, place) {
            (Scope::Player, _, _) => 1,
            (_, crate::rules::effects::Selector::OwnerCities | crate::rules::effects::Selector::OwnerCityTiles, _) => self.model.cities.len().max(1) as i64,
            _ => 1,
        }
    }

    /// A unit given free, at its kind's price.
    fn free_unit(&self, unit: crate::ids::UnitKindId, count: i64) -> i64 {
        let Some(def) = self.rules.units.get(usize::from(unit.0)) else { return 0 };
        let hammers = i64::from(def.cost) * self.unit(1);
        if def.founder {
            return count.min(self.room) * self.base.new_city * self.mult[pd::EXPANSION] / 1000;
        }
        if let Some(g) = self.rules.module.great_people.iter().find(|g| g.unit == unit) {
            use crate::classic::content::GreatPersonKind;
            // A general or an admiral fights: the army's price of its hammers.
            if matches!(g.kind, GreatPersonKind::General | GreatPersonKind::Admiral) {
                return count * hammers * self.mult[pd::MILITARY] / 1000;
            }
            return count * self.base.tech * self.mult[pd::GREAT_PEOPLE] / 1000;
        }
        if def.module.economy.trade {
            return count * (hammers + self.route()) * self.mult[pd::TRADE] / 1000;
        }
        let dim = match crate::ai::planner::role_of(self.rules, unit) {
            crate::ai::planner::AiRole::Worker => pd::WORKER,
            crate::ai::planner::AiRole::Military | crate::ai::planner::AiRole::Naval => pd::MILITARY,
            _ => pd::PRODUCTION,
        };
        count * hammers * self.mult[dim] / 1000
    }

    /// An internal food route within H: the rules' food of a route at our
    /// era and size, at the cell's food price.
    fn route(&self) -> i64 {
        let citizens = (self.model.citizens() / self.model.cities.len().max(1) as i64).max(3) as u32;
        let food = i64::from(crate::derived::trade::internal_amount(self.model.era as u32, citizens));
        self.within(self.base.food * self.mult[pd::FOOD] / 1000, food, pd::TRADE)
    }

    /// The share (percent) of an effect's conditions holding where it acts:
    /// the mean over the cities it reaches (a player's effect: the capital).
    fn holds(&self, e: &CompiledEffect, place: Place) -> i64 {
        let model = self.model;
        if model.cities.is_empty() {
            return 0;
        }
        let plan = super::cell::Plan::new(model);
        let cities: Vec<usize> = match (e.selector, place) {
            (crate::rules::effects::Selector::OwnerCities | crate::rules::effects::Selector::OwnerCityTiles, _) => (0..model.cities.len()).collect(),
            (crate::rules::effects::Selector::SourceCity | crate::rules::effects::Selector::SourceCityTiles, Place::City(i)) => vec![i],
            _ => vec![model.capital()],
        };
        cities.iter().map(|i| plan.holds(model, *i, e)).sum::<i64>() / cities.len().max(1) as i64
    }

    /// One effect of an item at `place`, within H (the cell's yields not
    /// counted), as much as its conditions hold.
    pub fn effect(&self, e: &CompiledEffect, place: Place) -> i64 {
        let share = match e.condition {
            // A combat or production gate is the situation's, not the city's.
            c if c.is_situational() || c.is_opponent() || matches!(c, crate::rules::effects::Condition::Producing(_)) => 100,
            _ => self.holds(e, place),
        };
        if share == 0 {
            return 0;
        }
        self.effect_always(e, place) * share / 100
    }

    fn effect_always(&self, e: &CompiledEffect, place: Place) -> i64 {
        let model = self.model;
        let cities = model.cities.len().max(1) as i64;
        let reach = self.cities_reached(e, place);
        let happiness = |amount: i64| self.within(self.base.happiness, amount * 100, pd::HAPPINESS);
        match e.modifier {
            Modifier::Module(ClassicModifier::AddHappiness { amount }) => happiness(i64::from(amount) * reach),
            Modifier::AddStat { stat: Stat::Module(ClassicStat::HappinessPerCity), amount } => happiness(i64::from(amount) * cities),
            Modifier::Module(ClassicModifier::AddHappinessPerCount { amount, per }) => {
                let counted = match per.count {
                    crate::rules::effects::Count::Population => model.citizens() / reach.max(1),
                    crate::rules::effects::Count::ConnectedCities => cities - 1,
                    _ => 0,
                };
                happiness(i64::from(per.scale(amount, counted.clamp(0, 10_000) as u32)) * reach)
            }
            Modifier::AddStat { stat: Stat::Module(ClassicStat::PopulationUnhappinessPercent), amount } => {
                let citizens = if reach > 1 { model.citizens() } else { model.cities.get(model.capital()).map_or(1, |c| c.population) };
                happiness(-i64::from(amount) * citizens * model.citizen_unhappiness / 100 / 100)
            }
            Modifier::FreeUnit { unit, count } => self.free_unit(unit, i64::from(count)),
            Modifier::Grant(Grant::FreeTechnologies(n)) => i64::from(n) * self.base.tech * self.mult[pd::SCIENCE] / 1000 * 2,
            Modifier::Grant(Grant::Population(n)) => i64::from(n) * reach * self.base.citizen * self.mult[pd::CITIZEN] / 1000,
            Modifier::Grant(Grant::Module(ClassicGrant::FreeGreatPeople(n))) => i64::from(n) * self.base.tech * self.mult[pd::GREAT_PEOPLE] / 1000,
            Modifier::Grant(Grant::Module(ClassicGrant::GoldenAge { turns })) => {
                let made: i64 = model.cities.iter().map(|c| c.production * self.unit(1) + c.extra[2] * self.unit(2)).sum();
                made * i64::from(turns) * w::GOLDEN_AGE_PERCENT / 100
            }
            Modifier::Module(ClassicModifier::FreePolicies { count }) => i64::from(count) * model.policy_cost * self.unit(4),
            Modifier::AddStat { stat: Stat::Module(ClassicStat::GreatPeopleRate | ClassicStat::GreatPersonRate(_)), amount } => {
                // The points the cities make: about two a city (wonders, specialists).
                let points = 2 * cities;
                points * i64::from(amount) / 100 * self.point() * self.horizon
            }
            Modifier::Module(ClassicModifier::AddGreatPersonPoints { amount, .. }) => i64::from(amount) * reach * self.point() * self.horizon,
            Modifier::AddStat { stat: Stat::Module(ClassicStat::Trade(crate::rules::stats::TradeStat::Routes)), amount } => i64::from(amount) * self.route(),
            Modifier::AddStat { stat: Stat::Module(ClassicStat::Trade(_)), amount } => self.within(self.base.gold, i64::from(amount.max(0)) * 100, pd::TRADE),
            Modifier::ModifyCost { target, percent } => {
                let cut = i64::from(-percent).max(0);
                match target {
                    CostTarget::BorderGrowth => self.within(self.base.culture, model.culture * cut, pd::CULTURE),
                    CostTarget::TilePurchase => self.within(self.base.gold, cities * cut * w::TILE_GOLD_SHARE / 100, pd::GOLD),
                    CostTarget::Module(ClassicCost::PolicyCityPenalty) => self.within(self.base.culture, model.culture * cut * cities / w::POLICY_PENALTY_CITIES, pd::CULTURE),
                    CostTarget::Module(ClassicCost::Policies) => self.within(self.base.culture, model.culture * cut, pd::CULTURE),
                    CostTarget::Buildings | CostTarget::Units => self.within(self.base.production, cities * 2 * cut, pd::PRODUCTION),
                    CostTarget::UnitUpgrade => cut * w::COMBAT_HAMMERS * self.unit(1) * self.mult[pd::MILITARY] / 1000 / 10,
                    _ => 0,
                }
            }
            Modifier::Module(ClassicModifier::TourismToward { percent, .. }) => self.within(self.base.culture, model.culture * i64::from(percent.max(0)) * w::TOURISM_ON_CULTURE / 100, pd::TOURISM),
            Modifier::Module(ClassicModifier::ModifyTourism { amount, .. }) => self.within(self.base.culture, model.culture * i64::from(amount.max(0)), pd::TOURISM),
            Modifier::Module(ClassicModifier::ModifyCityState { .. }) => self.within(self.base.gold, model.city_states * w::CITY_STATE_GOLD * 100, pd::GOLD),
            Modifier::PassivePromotion(_)
            | Modifier::ModifyCombat { .. }
            | Modifier::ModifyUnitStat { .. }
            | Modifier::ModifyCityCombat { .. }
            | Modifier::YieldFromKill { .. }
            | Modifier::AddPromotion(_)
            | Modifier::GrantAbility(_) => w::COMBAT_HAMMERS * cities * self.unit(1) * self.mult[pd::MILITARY] / 1000 * self.fight / 1000,
            _ => w::UNKNOWN,
        }
    }

    /// Every effect of a set at `place` (free buildings are the cell's).
    pub fn set(&self, set: &EffectSet, place: Place) -> i64 {
        effects_of(set).map(|e| self.effect(e, place)).sum()
    }

    /// A building at `place`: its effects, its upkeep within H, and a world
    /// wonder's points of the score.
    pub fn building(&self, building: crate::ids::BuildingKindId, place: Place) -> i64 {
        let Some(def) = self.rules.buildings.get(usize::from(building.0)) else { return 0 };
        let upkeep = self.within(self.base.gold, i64::from(def.maintenance) * 100, pd::GOLD);
        let wonder = if def.wonder { self.base.wonder * self.mult[pd::WONDER] / 1000 } else { 0 };
        self.set(&def.effects, place) - upkeep + wonder
    }
}
