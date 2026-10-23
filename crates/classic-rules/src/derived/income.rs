//! Per-turn income: the one set of formulas the end-turn resolution
//! applies (systems::turn) and the HUD and the AI read (§36 platform/structure.md:
//! the client never re-derives game numbers).
//!
//! A city's income comes from one snapshot of its yields (`city_breakdown`)
//! taken before its turn steps. Player income applies the player-level
//! modifiers to the city sums once.

use crate::classic::effects::ClassicYield;
use crate::derived::yields::{city_breakdown, CityBreakdown};
use crate::ids::PlayerId;
use crate::rules::effects::YieldKind;
use crate::rules::modifiers::Caps;
use crate::rules::stats::{SocietyStat, Stat};
use crate::rules::CompiledRuleset;
use crate::systems::growth;
pub use crate::systems::growth::{growth_threshold, player_growth_threshold};
use crate::world::city::CityCore;
use crate::world::state::AuthWorld;

pub use crate::derived::citizens::FOOD_PER_CITIZEN;

/// What one city adds on its owner's next end turn.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CityIncome {
    /// Gross food of the city.
    pub food: i32,
    /// Food its citizens eat.
    pub food_eaten: i32,
    /// Food into the stock after growth modifiers, whole food (negative:
    /// the city starves; 0 while a settler takes the surplus).
    pub food_surplus: i32,
    /// The same in hundredths (what the store gets).
    pub food_surplus_times100: i32,
    /// Production, the food a settler takes included.
    pub production: i32,
    /// What the production toward the head is made of: the rate toward
    /// another item (without food) is `production_rate.toward(its percent)`.
    pub production_rate: crate::derived::yields::ProductionRate,
    /// City gold before the player's Gold modifiers.
    pub gold: i32,
    /// City science before the player's Science modifiers.
    pub science: i32,
    pub culture: u32,
    /// City faith before the player's Faith modifiers.
    pub faith: u32,
}

/// What a player gains on its next end turn.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PlayerIncome {
    /// Gold after the player's modifiers, minus unit upkeep.
    pub gold: i32,
    pub science: i32,
    /// Culture into the player's pool (the sum of the cities' culture).
    pub culture: u32,
    /// Faith into the player's pool (cities, then the player's modifiers).
    pub faith: u32,
}

/// Income of one city now.
pub fn city_income(auth: &AuthWorld, rules: &CompiledRuleset, core: &CityCore) -> CityIncome {
    let yields = city_breakdown(auth, rules, core);
    let excess = crate::derived::happiness::excess_snapshot(auth, core.owner);
    let growth_percent = crate::derived::happiness::growth_percent(excess);
    let mut income = income_of(&yields, core.population, growth::producing_food(rules, core), growth_percent);
    if core.queue.is_empty() {
        let (gold, science) = crate::systems::processes::process_yield(core.economy.process, income.production);
        income.gold += gold;
        income.science += science;
    }
    income
}

/// Income from a city's yield snapshot; `growth_percent` is the empire's
/// growth modifier (unhappiness).
pub fn income_of(yields: &CityBreakdown, population: u32, producing_food: bool, growth_percent: i32) -> CityIncome {
    let rates = yields.rates;
    let eaten = (growth::consumption_times100(yields, population) / 100) as i32;
    let difference = growth::food_difference_times100(yields, population, producing_food, growth_percent);
    let mut production = rates.production;
    if producing_food {
        production += growth::food_production(yields, population);
    }
    CityIncome {
        food: rates.food,
        food_eaten: eaten,
        food_surplus: (difference / 100) as i32,
        food_surplus_times100: difference as i32,
        production,
        production_rate: yields.production,
        gold: rates.gold,
        science: rates.science,
        culture: rates.culture.max(0) as u32,
        faith: rates.module.max(0) as u32,
    }
}

/// Income of a player now: city sums through the player's modifiers, then
/// maintenance and city connection gold (`derived::maintenance`).
pub fn player_income(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId) -> PlayerIncome {
    let cities = auth
        .indexes()
        .cities_of(player)
        .iter()
        .filter_map(|c| auth.cities().get(*c));
    let incomes: Vec<CityIncome> = cities.map(|core| city_income(auth, rules, core)).collect();
    player_income_from(auth, rules, player, &incomes)
}

/// Player income from its cities' incomes (already computed by the caller).
pub fn player_income_from(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    player: PlayerId,
    cities: &[CityIncome],
) -> PlayerIncome {
    let gold = cities.iter().map(|c| c.gold).sum();
    let science = cities.iter().map(|c| c.science).sum();
    let culture: u32 = cities.iter().map(|c| c.culture).sum();
    let faith: u32 = cities.iter().map(|c| c.faith).sum();
    let upkeep = crate::derived::maintenance::upkeep(auth, rules, player);
    PlayerIncome {
        gold: player_gold(auth, rules, player, gold) - upkeep.net_cost(),
        science: player_science(auth, rules, player, science),
        culture: culture.saturating_add(culture_from_happiness(auth, rules, player)),
        faith: player_faith(auth, rules, player, faith),
    }
}

/// Culture the excess happiness of the turn's snapshot adds to the pool
/// (Fine Arts: `HappinessToCulturePercent` of a positive excess).
pub fn culture_from_happiness(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId) -> u32 {
    let excess = crate::derived::happiness::excess_snapshot(auth, player);
    if excess <= 0 {
        return 0;
    }
    let mods = crate::effects::player::player_modifiers(auth, rules, player);
    let percent = mods.stats.get(Stat::Module(crate::rules::stats::ClassicStat::Society(SocietyStat::HappinessToCulturePercent)));
    (excess * percent.max(0) / 100) as u32
}

/// Gold maintenance per turn of `units` paid units in game turn `turn`
/// before the difficulty percent (unit maintenance, rules.md): what the AI
/// weighs one more unit with.
pub fn unit_upkeep(units: usize, turn: u32) -> i32 {
    let support = units as u64 * crate::derived::maintenance::GOLD_PER_UNIT_TIMES100;
    crate::derived::maintenance::unit_cost(support, u64::from(turn)).min(i32::MAX as u64) as i32
}

/// The player's Faith modifiers on the sum of its cities' faith.
pub(crate) fn player_faith(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId, cities: u32) -> u32 {
    let mods = crate::effects::player::player_modifiers(auth, rules, player);
    let cities = cities.min(i32::MAX as u32) as i32;
    let own = mods.yields[YieldKind::Module(ClassicYield::Faith).index()].evaluate(cities, Caps::at_least(0)) as u32;
    // Religious city-states give their friends and allies faith (§4.6).
    let minors = crate::systems::city_states::player_bonus(
        auth,
        rules,
        player,
        crate::rules::content::CityStateBonusKind::Faith,
    );
    own.saturating_add(minors.max(0) as u32)
}

/// The player's Gold modifiers on the sum of its cities' gold.
pub(crate) fn player_gold(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    player: PlayerId,
    cities: i32,
) -> i32 {
    let mods = crate::effects::player::player_modifiers(auth, rules, player);
    mods.yields[YieldKind::Gold.index()].evaluate(cities, Caps::at_least(0))
}

/// The player's Science modifiers on the sum of its cities' science, plus
/// a share of its allied city-states' science (Scholasticism).
pub(crate) fn player_science(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    player: PlayerId,
    cities: i32,
) -> i32 {
    let mods = crate::effects::player::player_modifiers(auth, rules, player);
    let science = mods.yields[YieldKind::Science.index()].evaluate(cities, Caps::at_least(0));
    let allies = mods.module.city_state[crate::classic::effects::CityStateStat::AllyScience.index()] > 0;
    let allied = if allies { allied_city_state_science(auth, player) } else { 0 };
    science + science_while_happy(auth, player, &mods, cities) + allied
}

/// Rationalism (`ScienceWhileHappyPercent`): a percent of the cities' science
/// while the empire is not unhappy (excess of the turn's snapshot ≥ 0;
/// rules.md).
fn science_while_happy(
    auth: &AuthWorld,
    player: PlayerId,
    mods: &crate::effects::player::PlayerModifiers,
    cities: i32,
) -> i32 {
    let percent = mods.stats.get(Stat::Module(crate::rules::stats::ClassicStat::Society(SocietyStat::ScienceWhileHappyPercent)));
    if percent <= 0 || cities <= 0 {
        return 0;
    }
    if crate::derived::happiness::excess_snapshot(auth, player) < 0 {
        return 0;
    }
    cities * percent / 100
}

/// Percent of the science of a major's allied city-states that the major gets. Own
/// value, unverified.
pub const ALLY_SCIENCE_PERCENT: i32 = 25;

/// A quarter of the science of `major`'s allied city-states (rules
/// "City-states" §4.7; their science of their last turn).
pub fn allied_city_state_science(auth: &AuthWorld, major: PlayerId) -> i32 {
    let mut science = 0i64;
    for ally in crate::systems::city_states::allies_of(auth, major) {
        let last = auth
            .players()
            .economy_of(ally)
            .and_then(|e| e.science_history.first().copied())
            .unwrap_or(0);
        science += i64::from(last);
    }
    (science * i64::from(ALLY_SCIENCE_PERCENT) / 100) as i32
}
