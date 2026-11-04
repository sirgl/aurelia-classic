//! What the AI makes of a city it captured (`DecideCity`), by the rule its variant
//! carries (`Variant::conquest`): the game's AI before the eighth promotion
//! (`pre-puppet`, and every variant but the ones below) annexes a city of
//! `combat::ANNEX_POPULATION` citizens and more and puppets the rest
//! (`combat::conquest_decisions`); `standard` since the eighth promotion (2 November),
//! its alias `puppet-all` and `puppet-war` puppet every city; `puppet-price`
//! takes what is worth most over the rest of the game at the allocator's prices
//! (`priced_case`, classic/docs/ai-conquest-payoff.md "The priced decision").
//!
//! The priced decision compares three keepings of the same city from the rules'
//! data, each in points of the final score (`Centi`) over the rounds left `R`:
//!
//! - **puppet**: the city's worth — its score (a city, its first ring and its
//!   citizens: `Prices::score`) and its yields now to the end at the allocator's
//!   prices — less a normal city's unrest (the empire's rates, `Unhappiness::of`)
//!   priced by the moods it crosses (`Prices::unrest_by_steps`). A puppet adds
//!   nothing to the research or policy cost (`derived::culture::effective_cities`),
//!   buys nothing (`classic::systems::hurry_problem`) and takes any production
//!   order here, so the AI's production is the same as in an annexed city.
//! - **annex** (only where the nation may annex): the same worth, less the occupied
//!   city's unrest (the annexed-city and occupied-citizen rates and the flat
//!   occupation term of `derived::happiness`) until a building of the rules that
//!   exempts a city from it (`SocietyStat::NoOccupiedUnhappiness`: a courthouse)
//!   is built at the city's production, then a normal city's unrest; less that
//!   building's hammers and its maintenance to the end; or, when that costs more or
//!   no such building is open, the occupied unrest to the end. Less the research
//!   and policy cost one more non-puppet city adds (`world_size::tech_cost_per_city`,
//!   `policy_cost_per_city`): the share of the empire's beakers and culture it eats,
//!   at the allocator's prices. Resistance stops no yield under these rules (it only
//!   stops the city's strike): no term. The purchases an annexed city allows add
//!   nothing: gold is already priced at its best use in the empire.
//! - **raze** (only where the rules accept it: no original capital, no unique
//!   luxury): no worth and no unrest after it burns; while it burns (a citizen a
//!   round) it counts as occupied. The rival's loss is the same as for a kept city.
//!
//! Under these rules no term favours the annex, so the rule is "puppet, or raze a
//! city whose unrest tips the empire into a worse mood"; measured, those razes lose
//! (the step is priced as if it lasted to the end, the empire climbs back), and
//! `puppet-all` is the better variant (classic/docs/ai-conquest-payoff.md).
//!
//! The highest wins, the puppet on a tie. Not priced: the chance to lose the city
//! again, warmonger memory of a raze, the buildings a national wonder asks in every
//! non-puppet city. Several captures in one turn are decided in order, each with
//! the excess happiness the earlier ones leave (the empire's excess now, without the
//! cities awaiting the decision, which the rules count as normal cities meanwhile).

use crate::ai::value::{Centi, Prices};
use crate::command::{CityDecision, Command};
use crate::ids::{BuildingKindId, ProductionItem};
use crate::knowledge::observation::{AIObservation, OwnCity};
use crate::knowledge::public::PlayerQueries;
use crate::rules::CompiledRuleset;
use crate::world::city::CityStatus;

/// How a variant decides a captured city.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Conquest {
    /// The game's AI before the eighth promotion: annexed at `combat::ANNEX_POPULATION`
    /// citizens, else a puppet.
    #[default]
    Threshold,
    /// Every city a puppet (Unciv's AI at the capture): the game's AI since the eighth
    /// promotion.
    PuppetAll,
    /// The keeping worth most at the allocator's prices (`priced_case`).
    Priced,
}

/// The decisions of this turn's captured cities by `conquest`.
pub fn decisions(obs: &AIObservation<'_>, rules: &CompiledRuleset, prices: Option<&Prices>, conquest: Conquest) -> Vec<Command> {
    let decide = |city: &OwnCity, decision| Command::DecideCity { player: obs.player, city: city.handle, decision };
    let waiting = obs.own_cities.iter().filter(|c| c.status == CityStatus::AwaitingDecision);
    match (conquest, prices) {
        (Conquest::Threshold, _) => crate::ai::combat::conquest_decisions(obs),
        (Conquest::PuppetAll, _) | (Conquest::Priced, None) => waiting.map(|c| decide(c, CityDecision::Puppet)).collect(),
        (Conquest::Priced, Some(prices)) => {
            // The empire's excess now, less what the waiting cities weigh in it: the rules
            // count a city awaiting the decision as a normal one.
            let counted: i64 = waiting.clone().map(|c| (unrest_hundredths(prices, c.population).0 + 99) / 100).sum();
            let mut excess = obs.economy.happiness.excess() + counted as i32;
            waiting
                .map(|city| {
                    let case = priced_case(obs, rules, prices, excess, city);
                    crate::ai::war_probe::note(|p| p.conquests.push((obs.round, obs.player, city.tile.0, city.population, excess, case)));
                    excess -= case.unrest;
                    decide(city, case.decision)
                })
                .collect()
        }
    }
}

/// The three keepings of one captured city, points of the final score (raze: None
/// where the rules refuse it; annex: None where the nation may not annex).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Case {
    pub puppet: Centi,
    pub annex: Option<Centi>,
    pub raze: Option<Centi>,
    /// The highest of them (the puppet on a tie).
    pub decision: CityDecision,
    /// The points of unhappiness the decision brings the empire now.
    pub unrest: i32,
}

/// `Case` of `city` with the empire at `excess` happiness before it.
pub fn priced_case(obs: &AIObservation<'_>, rules: &CompiledRuleset, prices: &Prices, excess: i32, city: &OwnCity) -> Case {
    let mut prices = *prices;
    prices.excess = excess;
    let rounds = i64::from(prices.rounds_left.max(1));
    let worth = worth(&prices, city);
    let (normal, occupied) = unrest_hundredths(&prices, city.population);
    let normal_cost = mood_cost(&prices, normal);
    let occupied_cost = mood_cost(&prices, occupied);
    let puppet = worth - normal_cost;
    let annex = obs.economy.may_annex.then(|| {
        let never = occupied_cost;
        let with = exemption(obs, rules, &prices, city).map(|(built, cost)| {
            let built = i64::from(built).min(rounds);
            (occupied_cost * built + normal_cost * (rounds - built)) / rounds + cost
        });
        worth - with.map_or(never, |w| w.min(never)) - added_costs(obs, &prices)
    });
    let raze_command = Command::DecideCity { player: obs.player, city: city.handle, decision: CityDecision::Raze };
    let raze = obs.view.accepts(&raze_command).then(|| {
        let burning = i64::from(city.population.max(1)).min(rounds);
        -occupied_cost * burning / rounds
    });
    let mut decision = CityDecision::Puppet;
    let mut best = puppet;
    if let Some(a) = annex.filter(|a| *a > best) {
        (decision, best) = (CityDecision::Annex, a);
    }
    if raze.is_some_and(|r| r > best) {
        decision = CityDecision::Raze;
    }
    let hundredths = if decision == CityDecision::Puppet { normal } else { occupied };
    Case { puppet, annex, raze, decision, unrest: ((hundredths + 99) / 100) as i32 }
}

/// The city's score and its yields now to the end at the allocator's prices
/// (`Prices`: a yield a round kept to the last round).
fn worth(prices: &Prices, city: &OwnCity) -> Centi {
    let score = prices.score;
    let citizens = i64::from(city.population.max(1));
    let points = score.city + score.land * (crate::ai::value::FIRST_RING + citizens) + score.citizen * citizens;
    let income = &city.income;
    let yields = prices.food * i64::from(income.food_surplus.max(0))
        + prices.production * i64::from(income.production.max(0))
        + prices.gold * i64::from(income.gold.max(0))
        + prices.science * i64::from(income.science.max(0))
        + prices.culture * i64::from(income.culture)
        + prices.faith * i64::from(income.faith);
    points + yields
}

/// The unhappiness of the city kept as a normal city and as an occupied one,
/// hundredths of a point (`derived::happiness`, at the empire's rates).
fn unrest_hundredths(prices: &Prices, population: u32) -> (i64, i64) {
    use crate::derived::happiness::{
        OCCUPIED_CITY_UNHAPPINESS_TIMES100 as FLAT, UNHAPPINESS_FROM_ANNEXED_CITIZENS_TIMES100 as OCCUPIED,
        UNHAPPINESS_FROM_ANNEXED_CITY_TIMES100 as CAPTURED, UNHAPPINESS_FROM_CITIZENS_TIMES100 as CITIZEN, UNHAPPINESS_FROM_CITY_TIMES100 as CITY,
    };
    let citizens = i64::from(population.max(1));
    let rates = prices.unhappiness;
    let normal = rates.city + citizens * rates.citizen;
    let occupied = rates.city * CAPTURED / CITY + FLAT + citizens * rates.citizen * OCCUPIED / CITIZEN;
    (normal, occupied)
}

/// What `hundredths` of unhappiness to the end cost: by the moods the empire
/// crosses (`Prices::unrest_by_steps`) under `PriceModel::Uses`, else at the price
/// of a point.
fn mood_cost(prices: &Prices, hundredths: i64) -> Centi {
    match prices.model {
        crate::ai::value::PriceModel::Uses => prices.unrest_by_steps(hundredths),
        crate::ai::value::PriceModel::Tiles => hundredths * prices.happiness / 100,
    }
}

/// The cheapest building of the rules that exempts the city from occupied unrest
/// and the player may build now: the rounds it takes at the city's production and
/// its price (its hammers once, its maintenance from then on). None: no such
/// building is open.
fn exemption(obs: &AIObservation<'_>, rules: &CompiledRuleset, prices: &Prices, city: &OwnCity) -> Option<(u32, Centi)> {
    let rounds = i64::from(prices.rounds_left.max(1));
    let hammer = prices.production / rounds;
    let gold = prices.gold;
    let production = i64::from(city.income.production.max(1));
    exempting_buildings(rules)
        .filter(|kind| open_to(obs, rules, *kind))
        .map(|kind| {
            let def = &rules.buildings[kind.0 as usize];
            let cost = obs.view.own_production_cost(city.handle, ProductionItem::Building(kind)).unwrap_or(def.cost * obs.speed.production_percent / 100);
            let built = (i64::from(cost) + production - 1) / production;
            let upkeep = gold * i64::from(def.maintenance) * (rounds - built.min(rounds)) / rounds;
            (built as u32, i64::from(cost) * hammer + upkeep)
        })
        .min_by_key(|(built, price)| (*price, *built))
}

/// The buildings whose city effects exempt their city from occupied unrest.
pub fn exempting_buildings(rules: &CompiledRuleset) -> impl Iterator<Item = BuildingKindId> + '_ {
    use crate::rules::effects::Modifier;
    use crate::rules::stats::{ClassicStat, SocietyStat, Stat};
    rules.buildings.iter().enumerate().filter_map(|(i, def)| {
        let exempts = def.effects.passive_city.iter().any(|e| {
            matches!(e.modifier, Modifier::AddStat { stat: Stat::Module(ClassicStat::Society(SocietyStat::NoOccupiedUnhappiness)), amount } if amount > 0)
        });
        (exempts && !def.wonder && !def.unbuildable).then_some(BuildingKindId(i as u16))
    })
}

/// The player knows the building's technology, and the building is not another
/// nation's own nor replaced by its nation's own.
fn open_to(obs: &AIObservation<'_>, rules: &CompiledRuleset, kind: BuildingKindId) -> bool {
    let def = &rules.buildings[kind.0 as usize];
    let nation = obs.economy.nation;
    let known = def.required_tech.is_none_or(|t| obs.economy.techs.contains(&t));
    let own = def.nation.is_none_or(|n| n == nation);
    let replaced = rules.buildings.iter().any(|b| b.nation == Some(nation) && b.replaces == Some(kind));
    known && own && !replaced
}

/// What one more non-puppet city adds to the research and policy cost: the share
/// of the empire's beakers and culture a round it eats (`derived::research`: the
/// cost × (1 + per-city percent × (non-puppet cities − 1))), at the allocator's
/// prices.
fn added_costs(obs: &AIObservation<'_>, prices: &Prices) -> Centi {
    let size = crate::world::size::WorldSize::of(obs.view.dimensions());
    let counted = obs.own_cities.iter().filter(|c| !matches!(c.status, CityStatus::Puppet | CityStatus::AwaitingDecision)).count() as i64;
    // Hundredths of a percent: the cost now is 10 000 + p × (n − 1), one more city adds p.
    let eaten = |per_city: i64, amount: i64, price: Centi| amount.max(0) * price * per_city / (10_000 + per_city * counted);
    let science = eaten(crate::classic::world_size::tech_cost_per_city(size), i64::from(obs.economy.income.science), prices.science);
    let culture = eaten(crate::classic::world_size::policy_cost_per_city(size), i64::from(obs.economy.income.culture), prices.culture);
    science + culture
}
