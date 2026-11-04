//! The economy cell: a small recurrent cell shaped like the growth rules.
//! It runs the empire forward over the horizon H in `STEPS` coarse steps,
//! city by city: the food store and the citizens (the rules' growth
//! threshold at the game's speed; an empty store starves a citizen away),
//! the tiles the citizens work (chosen from
//! the city's own tiles by their yields), the city's yields, then the plan's
//! multipliers. A plan is a set of wonders, buildings and policies, each
//! entering at the round it is expected (`Plan::add`): their effects are the
//! rules' effects evaluated on our tiles, so the compounding of two goals —
//! a wonder that feeds growth, then more worked tiles of the kind another
//! wonder improves — comes out of the run by itself.
//!
//! The run's value is what the empire makes within H at the net's prices
//! (production, gold, beakers, culture, faith; food turns into citizens),
//! plus the citizens it gained at the citizen price. The marginal value of
//! a goal is `run(plan + goal) − run(plan)` (`goals`).

use super::model::{Model, TileModel};
use super::weights::cell::*;
use super::Milli;
use crate::ids::{BuildingKindId, TechId};
use crate::rules::effects::{CompiledEffect, Condition, EffectSet, Modifier, Scope, Selector, Trigger};
use crate::rules::CompiledRuleset;

/// The slot of a yield (food, production, gold, science, culture, faith;
/// `Output` counts as production).
pub fn slot(kind: crate::rules::effects::YieldKind) -> usize {
    use crate::rules::effects::YieldKind;
    match kind {
        YieldKind::Food => 0,
        YieldKind::Production | YieldKind::Output => 1,
        YieldKind::Gold => 2,
        YieldKind::Science => 3,
        YieldKind::Culture => 4,
        YieldKind::Module(_) => 5,
    }
}

/// What one plan item changes in one city from a round on (amounts in
/// hundredths of a yield a turn).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Delta {
    pub from: i64,
    /// The city must have this many citizens.
    pub min_pop: i64,
    /// Per tile (its index in `CityModel::tiles`; `usize::MAX` the centre).
    pub tiles: Vec<(usize, [i64; 6])>,
    pub flat: [i64; 6],
    /// Hundredths of a yield per citizen.
    pub per_pop: [i64; 6],
    pub percent: [i64; 6],
    pub growth: i64,
    /// Happiness of the empire, hundredths.
    pub happy: i64,
}

/// Where a plan item stands: a building of one city, or a policy of the
/// empire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    City(usize),
    Empire,
}

/// A plan: the deltas of its items per city, the buildings and
/// technologies it adds (the conditions of later items read them).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    pub deltas: Vec<Vec<Delta>>,
    pub buildings: Vec<(usize, BuildingKindId)>,
    pub techs: Vec<TechId>,
    /// The plan's queues: the round each city's hammers, the research and
    /// the culture are free again (a second goal of a city waits for the
    /// first: `goals`).
    pub city_free: Vec<i64>,
    pub research_free: i64,
    pub culture_free: i64,
}

/// Every effect of a set (the passive ones and the triggered ones).
pub fn effects_of(set: &EffectSet) -> impl Iterator<Item = &CompiledEffect> {
    set.passive_player.iter().chain(&set.passive_city).chain(&set.passive_unit).chain(&set.passive_tile).chain(set.triggered.iter().flatten())
}

/// A free building's effects are followed this deep.
const FREE_DEPTH: u8 = 1;

impl Plan {
    pub fn new(model: &Model) -> Self {
        Plan { deltas: vec![Vec::new(); model.cities.len()], buildings: Vec::new(), techs: Vec::new(), city_free: vec![0; model.cities.len()], research_free: 0, culture_free: 0 }
    }

    /// The cities an effect of an item at `place` reaches.
    fn reached(model: &Model, place: Place, selector: Selector) -> Vec<usize> {
        let capital = model.capital();
        match (selector, place) {
            (Selector::SourceCity | Selector::SourceCityTiles, Place::City(i)) => vec![i],
            (Selector::SourceCity | Selector::SourceCityTiles | Selector::Capital, _) => vec![capital],
            (Selector::OwnerCities | Selector::OwnerCityTiles | Selector::Owner, _) => (0..model.cities.len()).collect(),
            _ => Vec::new(),
        }
    }

    /// The share (percent) of a city effect's conditions holding in city
    /// `i` under the plan (both gates; a population gate by the city's
    /// citizens now).
    pub fn holds(&self, model: &Model, i: usize, effect: &CompiledEffect) -> i64 {
        let mut min_pop = 0;
        let a = self.share(model, i, None, effect.condition, &mut min_pop);
        let b = self.share(model, i, None, effect.second_condition, &mut min_pop);
        if model.cities[i].population < min_pop {
            return 0;
        }
        a * b / 100
    }

    /// The share (percent) of the time a condition holds in city `i` (on
    /// `tile`, else its centre); `min_pop` takes a population gate.
    fn share(&self, model: &Model, i: usize, tile: Option<&TileModel>, condition: Condition, min_pop: &mut i64) -> i64 {
        let city = &model.cities[i];
        match condition {
            Condition::Always | Condition::ConnectedToCapital => 100,
            c if super::model::is_static(c) => {
                let t = tile.unwrap_or(&city.centre);
                if model.bit(c).is_some_and(|b| t.meets(b)) {
                    100
                } else {
                    0
                }
            }
            Condition::HasBuilding(b) => {
                if city.buildings.contains(&b) || self.buildings.contains(&(i, b)) {
                    100
                } else {
                    0
                }
            }
            Condition::HasTechnology(t) => {
                if model.techs.contains(&t) || self.techs.contains(&t) {
                    100
                } else {
                    0
                }
            }
            Condition::LacksTechnology(t) => {
                if model.techs.contains(&t) {
                    0
                } else {
                    100
                }
            }
            Condition::MinPopulation(n) => {
                *min_pop = (*min_pop).max(i64::from(n));
                100
            }
            Condition::CityGarrisoned => GARRISON_SHARE,
            Condition::AtPeace => {
                if model.at_war {
                    0
                } else {
                    100
                }
            }
            _ => 0,
        }
    }

    /// Adds an item's effects (a building's at `Place::City`, a policy's at
    /// `Place::Empire`) from round `from` on.
    pub fn add(&mut self, model: &Model, rules: &CompiledRuleset, set: &EffectSet, place: Place, from: i64) {
        self.add_depth(model, rules, set, place, from, 0);
    }

    /// Adds building `b` in city `i` (its effects and the building itself).
    pub fn add_building(&mut self, model: &Model, rules: &CompiledRuleset, i: usize, b: BuildingKindId, from: i64) {
        if let Some(def) = rules.buildings.get(usize::from(b.0)) {
            self.buildings.push((i, b));
            self.add(model, rules, &def.effects, Place::City(i), from);
        }
    }

    fn add_depth(&mut self, model: &Model, rules: &CompiledRuleset, set: &EffectSet, place: Place, from: i64, depth: u8) {
        for effect in effects_of(set) {
            let passive = matches!(effect.trigger, Trigger::Passive);
            match effect.modifier {
                Modifier::FreeBuilding(b) if !passive && depth < FREE_DEPTH => {
                    for i in Self::reached(model, place, Selector::SourceCity) {
                        if !model.cities[i].buildings.contains(&b) {
                            self.buildings.push((i, b));
                            if let Some(def) = rules.buildings.get(usize::from(b.0)) {
                                self.add_depth(model, rules, &def.effects, Place::City(i), from, depth + 1);
                            }
                        }
                    }
                    continue;
                }
                Modifier::FreeBuildingInCities { building, count } if depth < FREE_DEPTH => {
                    let lacking: Vec<usize> = (0..model.cities.len()).filter(|i| !model.cities[*i].buildings.contains(&building)).take(usize::from(count)).collect();
                    for i in lacking {
                        self.buildings.push((i, building));
                        if let Some(def) = rules.buildings.get(usize::from(building.0)) {
                            self.add_depth(model, rules, &def.effects, Place::City(i), from, depth + 1);
                        }
                    }
                    continue;
                }
                _ if !passive => continue,
                _ => {}
            }
            for i in Self::reached(model, place, effect.selector) {
                if let Some(delta) = self.delta(model, i, effect, from) {
                    self.deltas[i].push(delta);
                }
            }
        }
    }

    /// One passive effect in city `i`, as a delta (None: nothing the cell
    /// models, or a condition that does not hold).
    fn delta(&self, model: &Model, i: usize, effect: &CompiledEffect, from: i64) -> Option<Delta> {
        use crate::classic::effects::ClassicModifier;
        use crate::rules::stats::{ClassicStat, Stat};
        let city = &model.cities[i];
        let mut d = Delta { from, ..Delta::default() };
        let gate = |tile: Option<&TileModel>, d: &mut Delta| -> i64 {
            let a = self.share(model, i, tile, effect.condition, &mut d.min_pop);
            let b = self.share(model, i, tile, effect.second_condition, &mut d.min_pop);
            a * b / 100
        };
        match (effect.scope, effect.modifier) {
            (Scope::Tile, Modifier::AddYield { yield_kind, amount }) => {
                let s = slot(yield_kind);
                for (at, t) in city.tiles.iter().enumerate().chain(std::iter::once((usize::MAX, &city.centre))) {
                    let share = gate(Some(t), &mut d);
                    if share > 0 {
                        let mut y = [0; 6];
                        y[s] = i64::from(amount) * share;
                        d.tiles.push((at, y));
                    }
                }
                (!d.tiles.is_empty()).then_some(d)
            }
            (_, Modifier::AddYield { yield_kind, amount }) => {
                let share = gate(None, &mut d);
                d.flat[slot(yield_kind)] = i64::from(amount) * share;
                (share > 0).then_some(d)
            }
            (_, Modifier::AddYieldPercent { yield_kind, percent }) => {
                let share = match effect.condition {
                    Condition::Producing(filter) if slot(yield_kind) == 1 => i64::from(crate::ai::planner_value::production_share(filter)),
                    Condition::Producing(_) => 0,
                    _ => gate(None, &mut d),
                };
                d.percent[slot(yield_kind)] = i64::from(percent) * share / 100;
                (share > 0).then_some(d)
            }
            (_, Modifier::AddYieldPerPopulation { yield_kind, hundredths }) => {
                let share = gate(None, &mut d);
                d.per_pop[slot(yield_kind)] = i64::from(hundredths) * share / 100;
                (share > 0).then_some(d)
            }
            (_, Modifier::ModifyGrowth { percent }) => {
                let share = gate(None, &mut d);
                d.growth = i64::from(percent) * share / 100;
                (share > 0).then_some(d)
            }
            (_, Modifier::Module(ClassicModifier::AddHappiness { amount })) => {
                let share = gate(None, &mut d);
                // A city's happiness counts once per city reached; the
                // player's once (in the capital's deltas).
                let once = effect.scope == Scope::Player && i != model.capital();
                d.happy = if once { 0 } else { i64::from(amount) * share };
                (d.happy != 0).then_some(d)
            }
            (_, Modifier::AddStat { stat: Stat::Module(ClassicStat::HappinessPerCity), amount }) if i == model.capital() => {
                d.happy = i64::from(amount) * 100 * model.cities.len() as i64;
                Some(d)
            }
            (_, Modifier::Module(ClassicModifier::AddHappinessPerCount { amount, per })) => {
                let counted = match per.count {
                    crate::rules::effects::Count::Population => city.population,
                    crate::rules::effects::Count::ConnectedCities => (model.cities.len() as i64 - 1).max(0),
                    _ => 0,
                };
                let once = effect.scope == Scope::Player && i != model.capital();
                if once {
                    return None;
                }
                let n = i64::from(per.scale(amount, counted.clamp(0, i64::from(u32::MAX)) as u32));
                d.happy = n * gate(None, &mut d);
                (d.happy != 0).then_some(d)
            }
            (_, Modifier::AddStat { stat: Stat::Module(ClassicStat::PopulationUnhappinessPercent), amount }) => {
                d.happy = -i64::from(amount) * city.population * model.citizen_unhappiness / 100 * gate(None, &mut d) / 100;
                (d.happy != 0).then_some(d)
            }
            _ => None,
        }
    }
}

/// The prices the cell values its yields at: per unit of each slot, in
/// thousandths of a centi-point (food: 0, it turns into citizens), and a
/// citizen gained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellPrices {
    pub unit: [i64; 6],
    pub citizen: i64,
    /// Rounds left in the game: after H the run's last step goes on at its
    /// rate to the end (the allocator's measure: a change kept to the end).
    pub rounds_left: i64,
}

impl CellPrices {
    pub fn of(base: &super::Base, mult: &[Milli; super::weights::pd::N]) -> Self {
        let mut unit = [0; 6];
        for (s, u) in unit.iter_mut().enumerate().skip(1) {
            *u = base.slot(s) * super::slot_mult(mult, s) / base.rounds_left.max(1);
        }
        CellPrices { unit, citizen: base.citizen * mult[super::weights::pd::CITIZEN] / 1000, rounds_left: base.rounds_left.max(1) }
    }
}

/// What a run gives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// Centi-points: within the horizon, then the last step's rate to the
    /// end of the game.
    pub value: i64,
    pub citizens: i64,
    /// Per city: the yields a turn at the first step and their mean over
    /// the run (hundredths).
    pub first: Vec<[i64; 6]>,
    pub mean: Vec<[i64; 6]>,
}

/// The growth threshold of a city of `pop` citizens at the game's pace,
/// hundredths of food.
fn threshold(pop: i64, pace: i64) -> i64 {
    i64::from(crate::systems::growth::growth_threshold(pop.clamp(1, 200) as u32)) * pace
}

/// The weight a citizen takes a tile by (yields in hundredths; `food`: the
/// weight of food).
fn weight(y: &[i64; 6], food: i64) -> i64 {
    y[0] * food + (1..6).map(|s| y[s] * TILE_WEIGHTS[s] * 100).sum::<i64>()
}

/// A city's tiles in the order its citizens take them at one food weight:
/// the best first, the lower index on a tie.
struct Ranking {
    weights: Vec<i64>,
    order: Vec<usize>,
}

impl Ranking {
    fn of(tiles: &[[i64; 6]], food: i64) -> Self {
        let weights: Vec<i64> = tiles.iter().map(|y| weight(y, food)).collect();
        let mut order: Vec<usize> = (0..tiles.len()).collect();
        order.sort_unstable_by_key(|k| (std::cmp::Reverse(weights[*k]), *k));
        Ranking { weights, order }
    }
}

/// What a city's citizens choose from at a step: its tiles (hundredths)
/// with the plan's deltas `on_tiles` (indices in the city's deltas), ranked
/// while the city is hungry (food at `food_weight`) and once it is fed.
struct Choice {
    food_weight: i64,
    on_tiles: Vec<usize>,
    tiles: Vec<[i64; 6]>,
    hungry: Ranking,
    fed: Ranking,
}

impl Choice {
    fn of(city: &super::model::CityModel, deltas: &[Delta], on_tiles: &[usize], food_weight: i64) -> Self {
        let mut tiles: Vec<[i64; 6]> = city.tiles.iter().map(|t| t.yields.map(|v| v * 100)).collect();
        for &k in on_tiles {
            for (at, y) in deltas[k].tiles.iter().filter(|(at, _)| *at != usize::MAX) {
                for s in 0..6 {
                    tiles[*at][s] += y[s];
                }
            }
        }
        Choice { food_weight, on_tiles: on_tiles.to_vec(), hungry: Ranking::of(&tiles, food_weight), fed: Ranking::of(&tiles, 100), tiles }
    }
}

/// The run of `model` under `plan` over `horizon` rounds.
pub fn run(model: &Model, plan: &Plan, prices: &CellPrices, horizon: i64) -> Outcome {
    let steps = STEPS;
    let dt = (horizon / steps).max(1);
    let n = model.cities.len();
    let mut pop: Vec<i64> = model.cities.iter().map(|c| c.population).collect();
    let mut store: Vec<i64> = model.cities.iter().map(|c| c.food * 100).collect();
    let start: i64 = pop.iter().sum();
    let mut produced = [0i64; 6];
    let mut last = [0i64; 6];
    let mut first = vec![[0i64; 6]; n];
    let mut sums = vec![[0i64; 6]; n];
    let food_per_citizen = i64::from(crate::derived::citizens::FOOD_PER_CITIZEN) * 100;
    let spec_weight: i64 = (1..6).map(|y| model.specialist[y] * 100 * TILE_WEIGHTS[y] * 100).sum::<i64>() + model.specialist[0] * 100 * 100;
    let each = super::model::per_citizen();
    // A city's choice is ranked anew only when its food weight or the plan's
    // deltas on its tiles change: most steps of most runs reuse it.
    let mut choices: Vec<Option<Choice>> = (0..n).map(|_| None).collect();
    let mut active: Vec<usize> = Vec::new();
    let mut on_tiles: Vec<usize> = Vec::new();
    let mut taken: Vec<bool> = Vec::new();
    for step in 0..steps {
        let t = step * dt;
        // The empire's mood: today's and the plan's happiness. The citizens
        // it gains do not lower it: the allocator buys happiness at its
        // price and keeps the empire content (bench: +4 on average to the
        // end); a cell that let them lower it capped growth after a few
        // citizens and the net bought no growth (dev bench: 21 % fewer
        // citizens at the end).
        let happy: i64 = (0..n).flat_map(|i| plan.deltas[i].iter().filter(|d| d.from <= t).map(|d| d.happy)).sum();
        let mood = model.excess * 100 + happy;
        let _ = start;
        let mood_growth = if mood >= 0 {
            100
        } else if mood > VERY_UNHAPPY * 100 {
            UNHAPPY_GROWTH
        } else {
            0
        };
        // Food is worth working as much as the mood lets the city grow:
        // its full weight while content, a third when growth stops.
        let food_weight = 100 + (TILE_WEIGHTS[0] * 100 - 100) * mood_growth / 100;
        for i in 0..n {
            let city = &model.cities[i];
            let deltas = &plan.deltas[i];
            active.clear();
            active.extend((0..deltas.len()).filter(|k| deltas[*k].from <= t && pop[i] >= deltas[*k].min_pop));
            // The centre's yields this step (hundredths); the other tiles'
            // are the choice's.
            let mut centre = city.centre.yields.map(|v| v * 100);
            on_tiles.clear();
            for &k in &active {
                for (at, y) in &deltas[k].tiles {
                    if *at == usize::MAX {
                        for s in 0..6 {
                            centre[s] += y[s];
                        }
                    } else if on_tiles.last() != Some(&k) {
                        on_tiles.push(k);
                    }
                }
            }
            if choices[i].as_ref().is_none_or(|c| c.food_weight != food_weight || c.on_tiles != on_tiles) {
                choices[i] = Some(Choice::of(city, deltas, &on_tiles, food_weight));
            }
            let choice = choices[i].as_ref().expect("chosen above");
            // The citizens take tiles one by one: food at its weight until the
            // city eats and keeps a surplus of `FED_SURPLUS`, then at a third
            // (a governor that grows, then works hammers and beakers). Each
            // takes the first tile of its ranking not taken yet.
            taken.clear();
            taken.resize(choice.tiles.len(), false);
            let (mut hungry_at, mut fed_at) = (0, 0);
            let mut free = choice.tiles.len();
            let mut total = centre;
            let mut citizens = pop[i];
            let fed = food_per_citizen * pop[i] + FED_SURPLUS * 100 - city.extra[0] * 100;
            while citizens > 0 && free > 0 {
                let (ranking, at) = if total[0] < fed { (&choice.hungry, &mut hungry_at) } else { (&choice.fed, &mut fed_at) };
                while taken[ranking.order[*at]] {
                    *at += 1;
                }
                let k = ranking.order[*at];
                if ranking.weights[k] <= spec_weight {
                    break;
                }
                taken[k] = true;
                free -= 1;
                for s in 0..6 {
                    total[s] += choice.tiles[k][s];
                }
                citizens -= 1;
            }
            for s in 0..6 {
                total[s] += citizens * model.specialist[s] * 100 + city.extra[s] * 100 + pop[i] * each[s] * 100;
            }
            let mut percent = [0i64; 6];
            let mut growth = 0;
            for d in active.iter().map(|k| &deltas[*k]) {
                for s in 0..6 {
                    total[s] += d.flat[s] + d.per_pop[s] * pop[i];
                    percent[s] += d.percent[s];
                }
                growth += d.growth;
            }
            for s in 0..6 {
                total[s] = total[s].max(0) * (100 + percent[s]).max(0) / 100;
            }
            if step == 0 {
                first[i] = total;
            }
            for s in 0..6 {
                sums[i][s] += total[s];
            }
            // Growth by the rules: the surplus over what the citizens eat,
            // its percent, the mood; the threshold at the game's pace.
            let mut surplus = total[0] - food_per_citizen * pop[i];
            if surplus > 0 {
                surplus = surplus * (100 + growth) / 100 * mood_growth / 100;
            }
            store[i] += surplus * dt;
            while store[i] >= threshold(pop[i], model.pace) && pop[i] < 200 {
                store[i] -= threshold(pop[i], model.pace);
                pop[i] += 1;
            }
            // Starvation by the rules: an empty store loses a citizen.
            if store[i] < 0 {
                store[i] = 0;
                if pop[i] > 1 {
                    pop[i] -= 1;
                }
            }
            for s in 1..6 {
                produced[s] += total[s] * dt;
                if step == steps - 1 {
                    last[s] += total[s];
                }
            }
        }
    }
    let gained = pop.iter().sum::<i64>() - start;
    let tail = (prices.rounds_left - steps * dt).max(0);
    let value = (1..6).map(|s| (produced[s] + last[s] * tail) / 100 * prices.unit[s] / 1000).sum::<i64>() + gained * prices.citizen;
    let mean = sums.iter().map(|s| s.map(|v| v / steps)).collect();
    Outcome { value, citizens: gained, first, mean }
}

/// The rounds a run over `horizon` covers (`STEPS` whole steps).
pub fn span(horizon: i64) -> i64 {
    (horizon / STEPS).max(1) * STEPS
}

/// The food price of the cell: the mean over the cities of what one more
/// food a turn from now adds to the run (centi-points of a food a turn kept
/// to the end, the allocator's measure). The growth formula over H: the food
/// brings citizens earlier, their yields count at the prices, and the last
/// step's rate goes on to the end.
pub fn food_price(model: &Model, plan: &Plan, prices: &CellPrices, horizon: i64) -> i64 {
    if model.cities.is_empty() {
        return 0;
    }
    let base = run(model, plan, prices, horizon).value;
    let mut total = 0;
    for i in 0..model.cities.len() {
        let mut fed = plan.clone();
        let mut flat = [0; 6];
        flat[0] = 100;
        fed.deltas[i].push(Delta { flat, ..Delta::default() });
        total += run(model, &fed, prices, horizon).value - base;
    }
    total / model.cities.len() as i64
}
