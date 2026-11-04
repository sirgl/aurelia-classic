//! The value measure of the allocator (classic/docs/ai-allocator.md): points of the
//! score that decides the time victory, in hundredths ("centi-points").
//!
//! The score counts cities, citizens and land (× the map's score modifier),
//! world wonders and technologies (`systems::victory::score`). Everything
//! else the empire makes is worth what it adds to those counts by the last
//! round: a food a turn is citizens, a beaker a turn is technologies, a
//! culture a turn is border tiles, a gold a turn is bought tiles, a hammer a
//! turn is wonders. These are the shadow prices of the yields; the allocator
//! compares every use of production and gold in this one currency.
//!
//! A planning model, not a simulation: thresholds and costs come from the
//! rules' public formulas over what the player observes.
//!
//! Two models of gold and happiness (`PriceModel`): `allocator` prices
//! gold by the tiles it buys and happiness by a comfort margin; the game's
//! AI, `standard`, prices gold by its best use now (a tile, a hurried queue
//! head or a city-state's alliance) and happiness by what it changes in
//! the rest of the game: the growth an unhappy empire loses, the production and gold
//! penalty, the very-unhappy line where growth stops and no city is
//! founded, and golden ages (classic/docs/ai-allocator.md, "Gold and happiness").

use crate::ai::planner_value::Weights;
use crate::ids::{BuildingKindId, TechId};
use crate::knowledge::observation::{AIObservation, OwnCity};
use crate::rules::effects::{CompiledEffect, Condition, Modifier, Selector, Trigger, YieldKind};
use crate::rules::CompiledRuleset;
use crate::systems::victory::{SCORE_CITY, SCORE_LAND, SCORE_POPULATION, SCORE_TECH, SCORE_WONDER, STANDARD_MAP_PLOTS};

/// Hundredths of a score point.
pub type Centi = i64;

/// Rounds the player still plans for. With a time victory, the rounds to
/// it. Without one (the game ends by another victory, whenever it comes)
/// the game's AI (`SciencePricing::Reach` and `Aim`) judges its investments
/// over an open horizon: an open-ended game is expected to last as long
/// again as it has lasted, never less than the rounds to the nominal length
/// (`max_rounds`). The earlier variants and the control `Closed` keep the
/// nominal length, which is one round from the nominal end on.
pub fn horizon(obs: &AIObservation<'_>, pricing: SciencePricing) -> u32 {
    let open = matches!(pricing, SciencePricing::Reach | SciencePricing::Aim) && !obs.victories.enabled(crate::world::victory::VictoryKind::Time);
    rounds_planned(obs.victories.max_rounds, obs.round, open)
}

fn rounds_planned(max_rounds: u32, round: u32, open: bool) -> u32 {
    let nominal = max_rounds.saturating_sub(round);
    (if open { nominal.max(round) } else { nominal }).max(1)
}

/// Points of the score per counted thing, in hundredths: the game's
/// multipliers with the map's modifier (smaller maps count cities, citizens
/// and land more).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScorePoints {
    pub city: Centi,
    pub citizen: Centi,
    pub land: Centi,
    pub wonder: Centi,
    pub tech: Centi,
}

impl ScorePoints {
    /// The score of a map of `tiles` plots (`systems::victory::map_score_mod`).
    pub fn of_map(tiles: usize) -> Self {
        let plots = (tiles as i64).max(1);
        let m = 100 * STANDARD_MAP_PLOTS / plots;
        let map = if m > 100 { (m - 100) / 3 + 100 } else { m };
        Self {
            city: SCORE_CITY * map,
            citizen: SCORE_POPULATION * map,
            land: SCORE_LAND * map,
            wonder: SCORE_WONDER * 100,
            tech: SCORE_TECH * 100,
        }
    }
}

/// How gold and happiness are priced.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PriceModel {
    /// Step 4 (`allocator`): a gold buys a share of a tile at the average
    /// plot price; a point of happiness is a citizen while the excess is
    /// below `HAPPINESS_COMFORT`.
    #[default]
    Tiles,
    /// `standard` (and `allocator-econ2`): a gold is worth its best purchase now; a point of
    /// happiness is worth its effects over the rounds left
    /// (`Outlook::happiness`), and the empire's choices that make or cost
    /// happiness follow that price.
    Uses,
}

/// How a beaker is priced.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SciencePricing {
    /// By the score a technology adds itself (`ScorePoints::tech`).
    #[default]
    Direct,
    /// By what the technologies researched in the rounds left unlock too:
    /// the buildings and wonders they open, at the other prices of the turn
    /// (`unlocked_value`).
    Reach,
    /// `Reach` that keeps the nominal length of the game when the time
    /// victory is off (the control `Variant::Closed`: the game's AI before
    /// the open horizon).
    ReachClosed,
    /// `Reach` for a planner that invests toward its aim (classic/docs/ai-investment.md):
    /// a game without a time victory has no deadline at `max_rounds`, so the
    /// rounds left are the open horizon (`horizon`); a beaker is also worth
    /// what the aim's progress is (`grand_strategy::aim_science`).
    Aim,
}

/// What one more of each yield a turn, kept to the last round, adds to the
/// final score (centi-points), and one more point of excess happiness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Prices {
    pub model: PriceModel,
    pub score: ScorePoints,
    /// Rounds to the time victory (at least 1).
    pub rounds_left: u32,
    pub food: Centi,
    pub production: Centi,
    pub gold: Centi,
    pub science: Centi,
    pub culture: Centi,
    pub faith: Centi,
    pub happiness: Centi,
    /// Unhappiness a city and a citizen bring, hundredths of a point (the
    /// empire's own rates under the rules' modifiers).
    pub unhappiness: Unhappiness,
    /// Rounds until the empire reaches the very-unhappy line, where the
    /// rules found no city and train no settler (`PriceModel::Uses`;
    /// `rounds_left` under `Tiles`).
    pub founding_rounds: u32,
    /// Percent of the rounds left the empire will spend at the
    /// very-unhappy line, where no citizen grows (`PriceModel::Uses`; 0
    /// under `Tiles`).
    pub capped: i64,
    /// The empire's cities now.
    pub cities: u32,
    /// The empire's excess happiness now, and what the moods cost it (`PriceModel::Uses`;
    /// zeros under `Tiles`): the price of a point the outlook gives without the crisis
    /// floor, the growth and the production a step down a mood costs
    /// (content to unhappy, unhappy to very unhappy). They price the unrest of a captured
    /// city by the steps it crosses (`captured_city_unrest_steps`).
    pub excess: i32,
    pub drift: Centi,
    pub step_unhappy: Centi,
    pub step_line: Centi,
    /// The unrest of a captured city is priced by the steps it crosses
    /// (`captured_city_unrest_steps`, `WarTuning::unrest`).
    pub unrest_steps: bool,
    /// A city beyond the opening is worth founding only while the empire
    /// stays content after it (`affords_city`); off only in the control
    /// `Variant::EagerFounding` (classic/docs/ai-growth.md).
    pub content_founding: bool,
    /// A building's effects are priced by what they change in the city:
    /// tile conditions over its territory, gates by its state, one-off
    /// grants once (`Prices::effect_here`; variant `true-effects`,
    /// classic/docs/ai-effects.md). Off: conditional and triggered effects
    /// at the planner's values.
    pub true_effects: bool,
    /// The spaceship parts still to build, by which a production percent
    /// toward some items (`Condition::Producing`) is priced by what it
    /// speeds in the city (`Prices::producing`; variant `space-finish`,
    /// classic/docs/ai-science-victory.md). None: the planner's flat value.
    pub space: Option<SpaceDemand>,
    /// The game speed's pace percent (`SpeedValues::pace_percent`): the
    /// rounds a tile, a citizen or a technology takes are Standard rounds
    /// × it (`Prices::paced`).
    pub pace: u32,
    /// The cities a tall empire keeps (`ai::tall`, `Variant::tall`): a new
    /// city beyond them is worth nothing. None: no cap.
    pub city_cap: Option<u32>,
    /// A gold gift is bought only while the alliance needs it: none to a city-state whose
    /// influence is above the alliance plus the rounds the next gift is away
    /// (`city_states::gift_ceiling`). Off only in the control `Variant::PreCsGifts`.
    pub gift_cap: bool,
}

/// The spaceship parts an empire still has to build: the hammers of those
/// whose technology it knows that are neither installed, walking nor done,
/// and the cities that will build them — the most productive ones, one a
/// part (`victory::space_plan` gives each part to the city that finishes
/// it soonest).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SpaceDemand {
    /// Base hammers of the parts left (their rules' cost).
    pub hammers: i64,
    /// The production of the cities that will build them, summed.
    pub builders: i64,
    /// The least production among those cities: a city below it builds no
    /// part.
    pub least: i32,
}

impl SpaceDemand {
    pub fn of(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Self {
        let known = |tech: Option<TechId>| tech.is_none_or(|t| obs.economy.techs.contains(&t));
        let mut parts = 0usize;
        let mut hammers = 0i64;
        for (kind, project, needed) in crate::ai::victory::part_kinds(rules) {
            let Some(def) = rules.units.get(kind.0 as usize) else { continue };
            if !known(def.required_tech) {
                continue;
            }
            let done = usize::from(obs.economy.projects.get(project.0 as usize).copied().unwrap_or(0));
            let walking = obs.own_units.iter().filter(|u| u.kind == kind).count();
            let left = usize::from(needed).saturating_sub(done + walking);
            parts += left;
            hammers += left as i64 * i64::from(def.cost);
        }
        let mut production: Vec<i32> = obs.own_cities.iter().map(|c| c.income.production.max(0)).collect();
        production.sort_unstable_by(|a, b| b.cmp(a));
        production.truncate(parts);
        Self {
            hammers,
            builders: production.iter().map(|p| i64::from(*p)).sum(),
            least: production.last().copied().unwrap_or(i32::MAX),
        }
    }
}

/// Unhappiness of one more city and one more citizen, in hundredths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unhappiness {
    pub city: i64,
    pub citizen: i64,
}

impl Unhappiness {
    /// The rules' base rates (`derived::happiness`: unhappiness per city and
    /// per citizen), before the empire's modifiers.
    pub const BASE: Unhappiness = Unhappiness {
        city: crate::derived::happiness::UNHAPPINESS_FROM_CITY_TIMES100,
        citizen: crate::derived::happiness::UNHAPPINESS_FROM_CITIZENS_TIMES100,
    };

    /// The empire's rates now: its unhappiness from cities and population
    /// over its cities and citizens (the base rates without either).
    pub fn of(obs: &AIObservation<'_>) -> Self {
        let report = &obs.economy.happiness;
        let cities = obs.own_cities.len() as i64;
        let population: i64 = obs.own_cities.iter().map(|c| i64::from(c.population)).sum();
        let rate = |total: i32, count: i64, base: i64| if count > 0 && total > 0 { (i64::from(total) * 100 / count).max(1) } else { base };
        Self {
            city: rate(report.from_cities, cities, Self::BASE.city),
            citizen: rate(report.from_population, population, Self::BASE.citizen),
        }
    }
}

/// Excess happiness at which one more point stops being worth a citizen:
/// below it growth stalls soon (−75 % food while unhappy).
pub const HAPPINESS_COMFORT: i32 = 6;
/// Gold a bought tile costs on average: an own estimate (Unciv's tile price,
/// `CityExpansionManager.getGoldCostOfTile`, is 50 per ring beyond the first plus 5
/// per tile the city already claimed, scaled by game speed; ours is a flat mean).
pub const TILE_PRICE: i64 = 75;
/// Share of production the Wealth and Research processes convert, percent.
pub const PROCESS_SHARE: i64 = 25;

impl Prices {
    pub fn of(obs: &AIObservation<'_>, rules: &CompiledRuleset, model: PriceModel) -> Self {
        Self::of_with(obs, rules, model, SciencePricing::Direct)
    }

    pub fn of_with(obs: &AIObservation<'_>, rules: &CompiledRuleset, model: PriceModel, pricing: SciencePricing) -> Self {
        Self::of_effects(obs, rules, model, pricing, false)
    }

    /// `of_with`, the buildings the beaker price counts priced by their
    /// true effects when `true_effects` (`Prices::true_effects`).
    pub fn of_effects(obs: &AIObservation<'_>, rules: &CompiledRuleset, model: PriceModel, pricing: SciencePricing, true_effects: bool) -> Self {
        Self::of_switches(obs, rules, model, pricing, true_effects, true)
    }

    /// `of_effects` with the switch of the gold gifts' cap (`Prices::gift_cap`).
    pub fn of_switches(obs: &AIObservation<'_>, rules: &CompiledRuleset, model: PriceModel, pricing: SciencePricing, true_effects: bool, gift_cap: bool) -> Self {
        let score = ScorePoints::of_map(obs.view.tile_count());
        let rounds_left = horizon(obs, pricing);
        let t = i64::from(rounds_left);
        let science = score.tech * t / research_cost(obs, rules, rounds_left).max(1);
        let excess = obs.economy.happiness.excess();
        let growth = i64::from(growth_cost(obs, rounds_left));
        let culture = score.land * t / culture_cost(obs, rounds_left).max(1);
        let production = (wonder_rate(obs, rules, score) * t / 100).max(science * PROCESS_SHARE / 100);
        // A tile's gold at the game speed (× its gold percent).
        let tile_price = crate::ai::genome::get(crate::ai::genome::Knob::TilePrice) * i64::from(obs.speed.gold_percent) / 100;
        let tiles = score.land * t / tile_price.max(1);
        let mut prices = Self {
            model,
            score,
            rounds_left,
            food: score.citizen * t / growth * growth_room(excess) / 100,
            production,
            gold: tiles,
            science,
            culture,
            faith: tiles / 2,
            happiness: 0,
            unhappiness: Unhappiness::BASE,
            founding_rounds: rounds_left,
            capped: 0,
            cities: obs.own_cities.len() as u32,
            excess,
            drift: 0,
            step_unhappy: 0,
            step_line: 0,
            unrest_steps: false,
            content_founding: false,
            true_effects,
            space: None,
            pace: obs.speed.pace_percent,
            city_cap: None,
            gift_cap,
        };
        match model {
            PriceModel::Tiles => {
                let stalled = stalled_food(obs, excess) * t / growth;
                prices.happiness = happiness_price(score, excess, stalled);
            }
            PriceModel::Uses => {
                prices.gold = tiles.max(production * hurry_rate(obs) / RATE_SCALE);
                prices.faith = prices.gold / 2;
                prices.unhappiness = Unhappiness::of(obs);
                let outlook = Outlook::of(obs, prices.unhappiness.citizen, rounds_left);
                let restored = restored_citizens(obs, excess, t, growth) * score.citizen;
                let drift = outlook.happiness(obs, &prices);
                prices.happiness = drift.max(restored).max(outlook.crisis(excess, score));
                prices.drift = drift;
                prices.step_unhappy = mood_step(obs, &prices, crate::derived::happiness::Mood::Content, crate::derived::happiness::Mood::Unhappy, t, growth);
                prices.step_line = mood_step(obs, &prices, crate::derived::happiness::Mood::Unhappy, crate::derived::happiness::Mood::VeryUnhappy, t, growth);
                prices.founding_rounds = outlook.very_unhappy_in.clamp(0, t) as u32;
                prices.capped = outlook.capped;
                // A food a turn grows citizens: those the very-unhappy line
                // would stop anyway are worth nothing.
                prices.food = prices.food * (100 - outlook.capped) / 100;
                // A gold is worth its best use: a city-state's alliance may
                // pay more than tiles and purchases.
                let gifts = crate::ai::city_states::priced_gifts(obs, &prices, crate::ai::city_states::MEDIUM_GIFT);
                let best = gifts.iter().map(|g| g.value * t / i64::from(g.gold)).max().unwrap_or(0);
                prices.gold = prices.gold.max(best);
                prices.faith = prices.gold / 2;
            }
        }
        if matches!(pricing, SciencePricing::Reach | SciencePricing::ReachClosed | SciencePricing::Aim) {
            prices.science = prices.reach_science(obs, rules);
            prices.production = prices.production.max(prices.science * PROCESS_SHARE / 100);
        }
        prices
    }

    /// The price of a beaker a turn kept to the last round when the
    /// technologies it buys are worth their score and what they open: the
    /// cheapest unknown ones, as many as the empire researches in the rounds
    /// left, each valued by `unlocked_value` from the round it is learned.
    fn reach_science(&self, obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Centi {
        let known = |t: TechId| obs.economy.techs.contains(&t);
        let mut ahead: Vec<(u32, TechId)> = rules
            .technologies
            .iter()
            .enumerate()
            .filter(|(i, _)| !known(TechId(*i as u16)))
            .map(|(i, def)| (def.cost, TechId(i as u16)))
            .collect();
        ahead.sort_unstable_by_key(|(c, t)| (*c, t.0));
        let tech_rounds = obs.speed.pace_rounds(crate::ai::genome::get_u32(crate::ai::genome::Knob::TechRounds));
        ahead.truncate((self.rounds_left / tech_rounds).max(1) as usize);
        if ahead.is_empty() {
            return self.science;
        }
        let opened = unlocked_value(obs, rules, self, &ahead);
        // The rules' cost at the game speed (× its science percent).
        let cost: i64 = ahead.iter().map(|(c, _)| i64::from(*c)).sum::<i64>() * i64::from(obs.speed.science_percent) / 100;
        let value: Centi = ahead
            .iter()
            .enumerate()
            .map(|(i, (_, tech))| self.score.tech + self.later(opened.get(tech).copied().unwrap_or(0), i as u32 * tech_rounds))
            .sum();
        value * i64::from(self.rounds_left) / cost.max(1)
    }

    /// A total `value` over the remaining rounds, of a source that starts
    /// `delay` rounds from now.
    pub fn later(&self, value: Centi, delay: u32) -> Centi {
        let left = i64::from(self.rounds_left);
        value * (left - i64::from(delay)).max(0) / left
    }

    pub fn of_yield(&self, kind: YieldKind) -> Centi {
        match kind {
            YieldKind::Output => self.production + self.gold,
            YieldKind::Food => self.food,
            YieldKind::Production => self.production,
            YieldKind::Gold => self.gold,
            YieldKind::Science => self.science,
            YieldKind::Culture => self.culture,
            YieldKind::Module(crate::classic::effects::ClassicYield::Faith) => self.faith,
        }
    }

    /// The prices as the planner's weights (`planner_value`, whose value
    /// of a yield a turn is ten times its weight): effects without a model
    /// of their own are valued by the same function as the planners'.
    pub fn weights(&self) -> Weights {
        let w = |c: Centi| (c / 10).clamp(0, i64::from(i16::MAX)) as i32;
        Weights {
            food: w(self.food),
            production: w(self.production),
            gold: w(self.gold),
            science: w(self.science),
            culture: w(self.culture),
            faith: w(self.faith),
            growth: w(self.food),
            military: w(self.gold),
            happiness: w(self.happiness),
        }
    }

    /// The value of a building in a city once built `delay` rounds from
    /// now: its effects for the rest of the game, its upkeep subtracted; a
    /// world wonder adds its points of the score. With `true_effects` each
    /// effect is priced by what it changes in the city (`effect_here`).
    pub fn building(&self, obs: &AIObservation<'_>, rules: &CompiledRuleset, city: &OwnCity, kind: BuildingKindId, cities: i32, delay: u32) -> Centi {
        if self.true_effects {
            return self.building_here(obs, rules, city, kind, cities, delay, 0);
        }
        let Some(def) = rules.buildings.get(kind.0 as usize) else {
            return 0;
        };
        let weights = self.weights();
        let set = &def.effects;
        let passive = set.passive_player.iter().chain(&set.passive_city).chain(&set.passive_unit).chain(&set.passive_tile);
        let effects: Centi = passive
            .chain(set.triggered.iter().flatten())
            .map(|e| self.producing(obs, rules, city, &def.consumes_resources, e).unwrap_or_else(|| self.effect(e, city, &weights, cities)))
            .sum();
        let upkeep = i64::from(def.maintenance) * self.gold;
        let wonder = if def.wonder { self.score.wonder } else { 0 };
        self.later(effects - upkeep, delay) + wonder
    }

    /// One effect: yields of the city's own size modelled here, the rest by
    /// the planner's effect values at these prices.
    fn effect(&self, effect: &CompiledEffect, city: &OwnCity, weights: &Weights, cities: i32) -> Centi {
        let plain = matches!(effect.condition, Condition::Always) && matches!(effect.trigger, Trigger::Passive);
        let reach = if effect.selector.is_owner_wide() { i64::from(cities.max(1)) } else { 1 };
        let own = match effect.modifier {
            _ if !plain => None,
            Modifier::AddYield { yield_kind, amount } => Some(self.of_yield(yield_kind) * i64::from(amount)),
            Modifier::AddYieldPercent { yield_kind, percent } => {
                Some(self.of_yield(yield_kind) * i64::from(base_yield(city, yield_kind)) * i64::from(percent) / 100)
            }
            Modifier::Module(crate::classic::effects::ClassicModifier::AddHappiness { amount }) => {
                Some(self.happiness * i64::from(amount).min(self.local_cap(city)))
            }
            // A yield per citizen (a Library's half a science each): the
            // citizens the city will have, as its local happiness counts.
            Modifier::AddYieldPerPopulation { yield_kind, hundredths } => {
                Some(self.of_yield(yield_kind) * i64::from(hundredths) * self.local_cap(city) / 100)
            }
            _ => None,
        };
        match own {
            Some(value) => value * reach,
            None => i64::from(crate::ai::planner_value::effect_value(effect, weights, cities)),
        }
    }

    /// What a new city founded `delay` rounds from now adds by the last
    /// round: its city points, its land (the first ring, then a tile every
    /// `TILE_ROUNDS`), its citizens (one, then one every `CITIZEN_ROUNDS`
    /// while happiness allows) and its yields, less the happiness it costs
    /// the empire. Under `PriceModel::Uses` nothing once the empire will be
    /// very unhappy by then: the rules found no city; and for the empire's
    /// first `OPENING_CITIES` cities their citizens are valued as the
    /// empire's growth is (the food price) — only the city's own
    /// unhappiness is their price.
    /// Standard rounds at the game's pace (themselves at Standard), at least 1.
    pub fn paced(&self, rounds: i64) -> i64 {
        if self.pace == 100 {
            return rounds;
        }
        (rounds * i64::from(self.pace) / 100).max(1)
    }

    pub fn new_city(&self, delay: u32) -> Centi {
        let left = i64::from(self.rounds_left.saturating_sub(delay));
        if left <= 0 || (self.model == PriceModel::Uses && delay >= self.founding_rounds) || self.city_cap.is_some_and(|cap| self.cities >= cap) {
            return 0;
        }
        let opening = self.cities < crate::ai::genome::get_u32(crate::ai::genome::Knob::OpeningCities);
        if self.content_founding && self.model == PriceModel::Uses && !opening && !self.affords_city() {
            return 0;
        }
        let land = self.score.land * (FIRST_RING + left / self.paced(TILE_ROUNDS));
        let citizens = 1 + left / self.paced(crate::ai::genome::get(crate::ai::genome::Knob::CitizenRounds));
        let yields = self.later(NEW_CITY_PRODUCTION * self.production + NEW_CITY_SCIENCE * self.science + self.gold, delay);
        let people = match self.model {
            PriceModel::Tiles => self.score.citizen * citizens * growth_share(self) / 100 - NEW_CITY_UNHAPPINESS * self.happiness,
            // The opening: its citizens are the empire's growth.
            PriceModel::Uses if self.cities < crate::ai::genome::get_u32(crate::ai::genome::Knob::OpeningCities) => {
                self.score.citizen * citizens * (100 - self.capped) / 100 - self.unhappiness.city * self.happiness / 100
            }
            PriceModel::Uses => {
                let unhappiness = self.unhappiness.city + citizens * self.unhappiness.citizen;
                self.score.citizen * citizens - unhappiness * self.happiness / 100
            }
        };
        self.score.city + land + people + yields
    }
}

/// Free buildings a free building's value may follow (a wonder's free
/// building is priced by `building_here`; one that gives another stops there).
const GRANT_DEPTH: u8 = 1;

/// The true effects of a building (`Prices::true_effects`,
/// classic/docs/ai-effects.md): each effect priced by what the rules make
/// of it in this city, in the allocator's measure. An effect whose answer
/// is not decided here (a condition this code does not read, a modifier
/// without a model) keeps the planner's value (`effect`).
impl Prices {
    #[allow(clippy::too_many_arguments)]
    fn building_here(&self, obs: &AIObservation<'_>, rules: &CompiledRuleset, city: &OwnCity, kind: BuildingKindId, cities: i32, delay: u32, depth: u8) -> Centi {
        let Some(def) = rules.buildings.get(kind.0 as usize) else {
            return 0;
        };
        let weights = self.weights();
        let set = &def.effects;
        let passive = set.passive_player.iter().chain(&set.passive_city).chain(&set.passive_unit).chain(&set.passive_tile);
        let effects: Centi = passive
            .chain(set.triggered.iter().flatten())
            .map(|e| self.producing(obs, rules, city, &def.consumes_resources, e).unwrap_or_else(|| self.effect_here(obs, rules, city, e, &weights, cities, depth)))
            .sum();
        let upkeep = i64::from(def.maintenance) * self.gold;
        let wonder = if def.wonder { self.score.wonder } else { 0 };
        self.later(effects - upkeep, delay) + wonder
    }

    /// A production percent toward some items (`Condition::Producing`)
    /// while the space demand is known (`space`), by what it speeds in the
    /// cities it reaches. Toward spaceship parts: the base hammers it saves
    /// on the parts those cities will build — each of the builders
    /// (`SpaceDemand`) its share of the parts by its production — once; none
    /// when the building takes a resource the parts need and leaves less
    /// than a part needs. Toward other items: the city's production times
    /// the share such items take of it (`planner_value::production_share`),
    /// for the rest of the game. None: another effect, or no demand known.
    fn producing(&self, obs: &AIObservation<'_>, rules: &CompiledRuleset, city: &OwnCity, consumes: &[(crate::ids::ResourceId, u16)], effect: &CompiledEffect) -> Option<Centi> {
        let demand = self.space?;
        let Condition::Producing(filter) = effect.condition else {
            return None;
        };
        let Modifier::AddYieldPercent { yield_kind: YieldKind::Production, percent } = effect.modifier else {
            return None;
        };
        if !matches!(effect.second_condition, Condition::Always) || !matches!(effect.trigger, Trigger::Passive) {
            return None;
        }
        let reached = self.reached(obs, city, effect.selector)?;
        let percent = i64::from(percent.max(0));
        let value = match filter {
            crate::rules::effects::ProductionFilter::SpaceshipParts => {
                if starves_parts(obs, rules, consumes) {
                    return Some(0);
                }
                let sped: i64 = reached
                    .iter()
                    .filter(|c| c.income.production >= demand.least)
                    .map(|c| demand.hammers * i64::from(c.income.production.max(0)) / demand.builders.max(1))
                    .sum();
                sped * percent / (100 + percent) * self.production / i64::from(self.rounds_left.max(1))
            }
            other => {
                let share = i64::from(crate::ai::planner_value::production_share(other));
                reached.iter().map(|c| self.production * i64::from(base_yield(c, YieldKind::Production)) * percent / 100 * share / 100).sum()
            }
        };
        Some(value)
    }

    /// One effect of a building of `city` for the rest of the game, as
    /// `building` prices it (`effect_here` with `true_effects`, else the
    /// planner's value where the allocator has no model).
    pub fn effect_in(&self, obs: &AIObservation<'_>, rules: &CompiledRuleset, city: &OwnCity, effect: &CompiledEffect, cities: i32) -> Centi {
        let weights = self.weights();
        if self.true_effects {
            self.effect_here(obs, rules, city, effect, &weights, cities, 0)
        } else {
            self.effect(effect, city, &weights, cities)
        }
    }

    /// One effect of a building of `city` by what it changes: a passive one
    /// in the tiles, cities or empire it reaches while its gates hold there,
    /// a grant on completion once, a grant to the units the city trains per
    /// unit it will train.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn effect_here(&self, obs: &AIObservation<'_>, rules: &CompiledRuleset, city: &OwnCity, effect: &CompiledEffect, weights: &Weights, cities: i32, depth: u8) -> Centi {
        use crate::rules::effects::Scope;
        let priced = match effect.trigger {
            Trigger::Passive => match effect.scope {
                Scope::Tile => self.tiles_effect(obs, rules, city, effect, weights),
                Scope::City => self.cities_effect(obs, rules, city, effect, weights),
                Scope::Player => self.empire_effect(obs, rules, city, effect),
                _ => None,
            },
            Trigger::OnBuilt => self.grant_once(obs, rules, city, effect, cities, depth),
            Trigger::OnUnitTrained => self.trained_grant(obs, rules, effect),
            _ => None,
        };
        priced.unwrap_or_else(|| self.effect(effect, city, weights, cities))
    }

    /// The own cities an effect of a building of `city` reaches (None: a
    /// selector without cities, or the event's city).
    fn reached<'o>(&self, obs: &'o AIObservation<'_>, city: &'o OwnCity, selector: Selector) -> Option<Vec<&'o OwnCity>> {
        match selector {
            Selector::SourceCity | Selector::SourceCityTiles => Some(vec![city]),
            Selector::OwnerCities | Selector::OwnerCityTiles => Some(obs.own_cities.iter().collect()),
            Selector::Capital => {
                let capital = obs.view.own_capital();
                Some(obs.own_cities.iter().filter(|c| Some(c.handle) == capital).collect())
            }
            _ => None,
        }
    }

    /// A tile effect: its value on one tile times the tiles of the reached
    /// cities that meet its conditions (`tiles_met`).
    fn tiles_effect(&self, obs: &AIObservation<'_>, rules: &CompiledRuleset, city: &OwnCity, effect: &CompiledEffect, weights: &Weights) -> Option<Centi> {
        let per_tile = match effect.modifier {
            Modifier::AddYield { yield_kind, amount } => self.of_yield(yield_kind) * i64::from(amount),
            _ => i64::from(crate::ai::planner_value::effect_value(effect, weights, 1)),
        };
        let mut hundredths = 0;
        for reached in self.reached(obs, city, effect.selector)? {
            hundredths += self.tiles_met(obs, rules, reached, effect)?;
        }
        Some(per_tile * hundredths / 100)
    }

    /// Tiles of a city's territory (`territory::territory`, the work radius)
    /// that meet both conditions of a tile effect, in hundredths of a tile:
    /// a worked tile counts in full, any other at the share of the open
    /// tiles the city's spare citizens can work — the citizens it has on
    /// average over the rounds left (`citizens_ahead`) less those working
    /// tiles now. None: a condition not decided on some tile.
    fn tiles_met(&self, obs: &AIObservation<'_>, rules: &CompiledRuleset, city: &OwnCity, effect: &CompiledEffect) -> Option<i64> {
        use crate::ai::territory::{effect_meets, territory};
        let ctx = territory_context(obs, rules, city);
        let land = territory(&obs.view, city.tile, crate::world::city::CITY_WORK_RADIUS, obs.player);
        let (mut worked, mut other, mut open) = (0i64, 0i64, 0i64);
        for tile in &land {
            let works = city.worked.contains(tile);
            if !works {
                open += 1;
            }
            match effect_meets(&obs.view, *tile, effect, &ctx)? {
                true if works => worked += 1,
                true => other += 1,
                false => {}
            }
        }
        // The centre is among the worked tiles and needs no citizen.
        let working = (city.worked.len() as i64 - 1).max(0);
        let spare = (self.citizens_ahead(city) - working).max(0);
        let share = if open > 0 { (spare * 100 / open).min(100) } else { 0 };
        Some(worked * 100 + other * share)
    }

    /// Citizens `city` has on average over the rounds left: now and at the
    /// end (`local_cap`), halfway.
    fn citizens_ahead(&self, city: &OwnCity) -> i64 {
        (i64::from(city.population) + self.local_cap(city)) / 2
    }

    /// A city effect: in each reached city whose state meets its
    /// conditions, what it changes there (`city_change`).
    fn cities_effect(&self, obs: &AIObservation<'_>, rules: &CompiledRuleset, city: &OwnCity, effect: &CompiledEffect, weights: &Weights) -> Option<Centi> {
        let mut total = 0;
        for reached in self.reached(obs, city, effect.selector)? {
            if !city_meets(obs, rules, reached, effect)? {
                continue;
            }
            total += self.city_change(obs, rules, reached, effect, weights);
        }
        Some(total)
    }

    /// What a city effect whose conditions hold changes in `city`: its
    /// yields at the city's own size; local happiness up to the citizens
    /// the city's other buildings leave; nothing for the defense of the
    /// city, which the danger watch prices while the city is threatened
    /// (`defence::building_net`); an exemption from the unhappiness of
    /// occupation what the city's occupation costs, nothing in a city not
    /// occupied; a trade route stat per route the city may run
    /// (`route_change`); anything else the planner's value in one city.
    fn city_change(&self, obs: &AIObservation<'_>, rules: &CompiledRuleset, city: &OwnCity, effect: &CompiledEffect, weights: &Weights) -> Centi {
        use crate::classic::effects::ClassicModifier;
        use crate::rules::effects::CityCombatStat;
        use crate::rules::stats::{ClassicStat, SocietyStat, Stat};
        match effect.modifier {
            Modifier::AddYield { yield_kind, amount } => self.of_yield(yield_kind) * i64::from(amount),
            Modifier::AddYieldPercent { yield_kind, percent } => self.of_yield(yield_kind) * i64::from(base_yield(city, yield_kind)) * i64::from(percent) / 100,
            Modifier::AddYieldPerPopulation { yield_kind, hundredths } => self.of_yield(yield_kind) * i64::from(hundredths) * self.local_cap(city) / 100,
            Modifier::Module(ClassicModifier::AddHappiness { amount }) => {
                let room = (self.local_cap(city) - local_happiness(rules, city)).max(0);
                self.happiness * i64::from(amount).min(room)
            }
            Modifier::ModifyCityCombat { stat: CityCombatStat::Defense | CityCombatStat::MaxHitPoints, .. } => 0,
            Modifier::AddStat { stat: Stat::Module(ClassicStat::Society(SocietyStat::NoOccupiedUnhappiness)), .. } => self.occupation(city),
            Modifier::AddStat { stat: Stat::Module(ClassicStat::Trade(stat)), amount } => self.route_change(obs, rules, effect, stat, amount, weights),
            _ => i64::from(crate::ai::planner_value::effect_value(effect, weights, 1)),
        }
    }

    /// What the occupation of `city` costs the empire's happiness: an
    /// occupied city's unhappiness over an own city's of its size
    /// (`derived::happiness`: the annexed-city and occupied-citizen rates
    /// and the flat occupation term over the base rates, at the empire's
    /// rates). Nothing for a city that is not occupied.
    fn occupation(&self, city: &OwnCity) -> Centi {
        use crate::derived::happiness::{
            OCCUPIED_CITY_UNHAPPINESS_TIMES100 as FLAT, UNHAPPINESS_FROM_ANNEXED_CITIZENS_TIMES100 as OCCUPIED,
            UNHAPPINESS_FROM_ANNEXED_CITY_TIMES100 as CAPTURED, UNHAPPINESS_FROM_CITIZENS_TIMES100 as CITIZEN, UNHAPPINESS_FROM_CITY_TIMES100 as CITY,
        };
        if city.status != crate::world::city::CityStatus::Occupied {
            return 0;
        }
        let citizens = i64::from(city.population.max(1));
        let rates = self.unhappiness;
        let occupied = rates.city * CAPTURED / CITY + FLAT + citizens * rates.citizen * OCCUPIED / CITIZEN;
        let own = rates.city + citizens * rates.citizen;
        (occupied - own).max(0) * self.happiness / 100
    }

    /// A trade route stat of a city per route it touches: the gold of the
    /// routes from it or to it at a gold a turn per route (`derived::trade`:
    /// the route's gold adds the stat in whole gold), the range or the
    /// share of its internal routes at the planner's value while it has a
    /// route; nothing for the gold the sender of a route to it gets (the
    /// sender's, not ours). Routes the city touches: the empire's route
    /// slots (`route_slots`) shared by its cities, as many coming in as
    /// going out.
    fn route_change(&self, obs: &AIObservation<'_>, rules: &CompiledRuleset, effect: &CompiledEffect, stat: crate::rules::stats::TradeStat, amount: i32, weights: &Weights) -> Centi {
        use crate::rules::stats::TradeStat;
        let cities = obs.own_cities.len().max(1) as i64;
        let routes = i64::from(route_slots(obs, rules)) * 100 / cities;
        match stat {
            TradeStat::TargetGold => 0,
            TradeStat::RouteGold(_) | TradeStat::RecipientGold => self.gold * i64::from(amount) * routes / 100,
            _ if routes == 0 => 0,
            _ => i64::from(crate::ai::planner_value::effect_value(effect, weights, 1)),
        }
    }

    /// A player effect while its conditions hold for the empire: happiness
    /// is the empire's once (not a city's, not capped by citizens), a
    /// happiness per city once per city. None: another modifier, or a
    /// condition not decided here.
    fn empire_effect(&self, obs: &AIObservation<'_>, rules: &CompiledRuleset, city: &OwnCity, effect: &CompiledEffect) -> Option<Centi> {
        use crate::classic::effects::ClassicModifier;
        use crate::rules::stats::{ClassicStat, Stat};
        if !city_meets(obs, rules, city, effect)? {
            return Some(0);
        }
        match effect.modifier {
            Modifier::Module(ClassicModifier::AddHappiness { amount }) => Some(self.happiness * i64::from(amount)),
            Modifier::AddStat { stat: Stat::Module(ClassicStat::HappinessPerCity), amount } => Some(self.happiness * i64::from(amount) * obs.own_cities.len().max(1) as i64),
            _ => None,
        }
    }

    /// A grant on completion, priced once by what it gives in this measure:
    /// a free building its value in the city (`building_here`), free
    /// buildings in the empire's cities theirs where they lack it, a free
    /// technology a technology's worth at the beaker price (`technology`),
    /// a citizen its points less its unhappiness (`citizen`), a great
    /// person or a golden age the yields they bring (`great_person`,
    /// `golden_age`), a free policy the culture it saves, a free unit its
    /// hammers (a great person's: what it brings). None: no model.
    fn grant_once(&self, obs: &AIObservation<'_>, rules: &CompiledRuleset, city: &OwnCity, effect: &CompiledEffect, cities: i32, depth: u8) -> Option<Centi> {
        use crate::classic::effects::{ClassicGrant, ClassicModifier};
        use crate::rules::effects::Grant;
        let t = i64::from(self.rounds_left);
        let value = match effect.modifier {
            Modifier::FreeBuilding(kind) if depth < GRANT_DEPTH => {
                if city.buildings.contains(&kind) {
                    0
                } else {
                    self.building_here(obs, rules, city, kind, cities, 0, depth + 1)
                }
            }
            Modifier::FreeBuildingInCities { building, count } if depth < GRANT_DEPTH => {
                let mut values: Vec<Centi> = obs
                    .own_cities
                    .iter()
                    .filter(|c| !c.buildings.contains(&building))
                    .map(|c| self.building_here(obs, rules, c, building, cities, 0, depth + 1))
                    .collect();
                values.sort_unstable_by_key(|v| std::cmp::Reverse(*v));
                values.into_iter().take(usize::from(count)).sum()
            }
            Modifier::Grant(Grant::FreeTechnologies(n)) => i64::from(n) * self.technology(obs, rules),
            Modifier::Grant(Grant::Population(n)) => {
                let reached = self.reached(obs, city, effect.selector)?.len() as i64;
                reached * i64::from(n) * self.citizen()
            }
            Modifier::Grant(Grant::Module(ClassicGrant::FreeGreatPeople(n))) => {
                let best = (0..rules.units.len()).filter_map(|k| self.great_person(obs, rules, city, crate::ids::UnitKindId(k as u16))).max()?;
                i64::from(n) * best
            }
            Modifier::Grant(Grant::Module(ClassicGrant::GoldenAge { turns })) => self.golden_age(obs, turns),
            Modifier::Grant(Grant::MilitaryUnitCopies) => {
                let mut kinds: Vec<crate::ids::UnitKindId> = obs.own_units.iter().map(|u| u.kind).filter(|k| crate::ai::planner::role_of(rules, *k) == crate::ai::planner::AiRole::Military).collect();
                kinds.sort_unstable_by_key(|k| k.0);
                kinds.dedup();
                kinds.iter().map(|k| i64::from(rules.units[k.0 as usize].cost) * self.production / t).sum()
            }
            Modifier::Module(ClassicModifier::FreePolicies { count }) => i64::from(count) * i64::from(obs.culture.next_policy_cost) * self.culture / t,
            Modifier::FreeUnit { unit, count } => {
                let one = self.great_person(obs, rules, city, unit).or_else(|| {
                    let cost = rules.units.get(unit.0 as usize).map_or(0, |u| i64::from(u.cost));
                    (cost > 0).then(|| cost * self.production / t)
                })?;
                i64::from(count) * one
            }
            _ => return None,
        };
        Some(value)
    }

    /// A technology's worth at the beaker price: the beakers of an average
    /// technology ahead (`research_cost`) once.
    pub fn technology(&self, obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Centi {
        let cost = research_cost(obs, rules, self.rounds_left);
        if cost == i64::MAX {
            return 0;
        }
        self.science * cost / i64::from(self.rounds_left)
    }

    /// One more citizen now: its points, less what its unhappiness costs.
    pub(crate) fn citizen(&self) -> Centi {
        self.score.citizen - self.unhappiness.citizen * self.happiness / 100
    }

    /// What a great person of `kind` brings once by its action
    /// (`UnitAbilities`): a scientist the empire's science of its turns, an
    /// engineer its hurry in `city`, a merchant its trade mission's gold in
    /// this era, an artist its golden age; the best of them. None: no such
    /// action.
    fn great_person(&self, obs: &AIObservation<'_>, rules: &CompiledRuleset, city: &OwnCity, kind: crate::ids::UnitKindId) -> Option<Centi> {
        let a = &rules.units.get(kind.0 as usize)?.module.economy;
        let t = i64::from(self.rounds_left);
        let science = i64::from(obs.economy.income.science.max(0)) * i64::from(a.beakers_turns) * self.science / t;
        let hurry = (i64::from(a.base_hurry) + i64::from(a.hurry_multiplier) * i64::from(city.population)) * self.production / t;
        let gold = if a.base_gold > 0 { (i64::from(a.base_gold) + i64::from(a.gold_per_era) * i64::from(obs.view.own_era())) * self.gold / t } else { 0 };
        let golden = if a.golden_age_turns > 0 { self.golden_age(obs, a.golden_age_turns) } else { 0 };
        let best = science.max(hurry).max(gold).max(golden);
        (best > 0).then_some(best)
    }

    /// A golden age of `turns`: the empire's production of those turns
    /// times the rules' golden-age production percent.
    fn golden_age(&self, obs: &AIObservation<'_>, turns: u16) -> Centi {
        use crate::classic::yields::GOLDEN_AGE_PRODUCTION_PERCENT;
        let production: i64 = obs.own_cities.iter().map(|c| i64::from(c.income.production.max(0))).sum();
        production * i64::from(GOLDEN_AGE_PRODUCTION_PERCENT) / 100 * i64::from(turns) * self.production / i64::from(self.rounds_left)
    }

    /// A grant to each unit the city trains that meets its condition
    /// (experience, a promotion): per unit the share of its hammers the
    /// promotions are worth (`promotion_percent` of the strength each),
    /// times the units the city will train in the rounds left — as many as
    /// the empire's units of the kind per city per round played so far.
    /// None: another grant, or a condition on more than the unit's kind.
    fn trained_grant(&self, obs: &AIObservation<'_>, rules: &CompiledRuleset, effect: &CompiledEffect) -> Option<Centi> {
        let promotions = match effect.modifier {
            Modifier::GrantExperience(xp) => promotions_of(u32::from(xp)),
            Modifier::AddPromotion(_) => 100,
            _ => return None,
        };
        let fits = |kind: crate::ids::UnitKindId| -> Option<bool> {
            let def = rules.units.get(kind.0 as usize)?;
            let fights = !rules.is_civilian(kind) && (def.strength > 0 || def.ranged_strength > 0);
            Some(fights && match effect.condition {
                Condition::Always => true,
                Condition::UnitDomain(domain) => def.domain == domain,
                Condition::UnitRole(role) => def.role == Some(role),
                _ => return None,
            })
        };
        let mut matching = Vec::new();
        for unit in &obs.own_units {
            if fits(unit.kind)? {
                matching.push(i64::from(rules.units[unit.kind.0 as usize].cost));
            }
        }
        if matching.is_empty() {
            return Some(0);
        }
        let cost = matching.iter().sum::<i64>() / matching.len() as i64;
        let cities = obs.own_cities.len().max(1) as i64;
        let trained = matching.len() as i64 * i64::from(self.rounds_left) * 100 / (cities * i64::from(obs.round.max(1)));
        let per_unit = cost * self.production / i64::from(self.rounds_left) * promotions / 100 * promotion_percent(rules) / 100;
        Some(per_unit * trained / 100)
    }
}

/// The territory context of an own city (`territory::TerritoryContext`).
fn territory_context<'c>(obs: &'c AIObservation<'_>, rules: &'c CompiledRuleset, city: &'c OwnCity) -> crate::ai::territory::TerritoryContext<'c> {
    crate::ai::territory::TerritoryContext {
        rules,
        player: obs.player,
        techs: &obs.economy.techs,
        centre: city.tile,
        population: city.population,
        buildings: &city.buildings,
    }
}

/// Do both conditions of a city or player effect hold for `city` now? The
/// city conditions of `territory::tile_meets` on its centre, and its state:
/// at peace with every major nation, connected to the capital, garrisoned.
/// None: a condition not decided here (the queue head's: the planner's
/// model of a production percent).
fn city_meets(obs: &AIObservation<'_>, rules: &CompiledRuleset, city: &OwnCity, effect: &CompiledEffect) -> Option<bool> {
    let ctx = territory_context(obs, rules, city);
    let one = |condition: Condition| match condition {
        Condition::Producing(_) => None,
        Condition::AtPeace => Some(!obs.relations.iter().any(|r| r.at_war && r.kind == crate::world::player::PlayerKind::Major)),
        Condition::ConnectedToCapital => Some(obs.view.own_capital() == Some(city.handle) || obs.view.own_connected_cities().contains(&city.handle)),
        Condition::CityGarrisoned => Some(obs.own_units.iter().any(|u| u.tile == city.tile && crate::ai::planner::role_of(rules, u.kind) == crate::ai::planner::AiRole::Military)),
        _ => crate::ai::territory::tile_meets(&obs.view, city.tile, condition, &ctx),
    };
    match (one(effect.condition), one(effect.second_condition)) {
        (Some(false), _) | (_, Some(false)) => Some(false),
        (Some(true), Some(true)) => Some(true),
        _ => None,
    }
}

/// A building that consumes `consumes` leaves the empire less of a
/// resource than a spaceship part needs (the rules let no part start or
/// complete without it: `derived::resources::production_resources`).
fn starves_parts(obs: &AIObservation<'_>, rules: &CompiledRuleset, consumes: &[(crate::ids::ResourceId, u16)]) -> bool {
    let needs: Vec<(crate::ids::ResourceId, u16)> = crate::ai::victory::part_kinds(rules)
        .into_iter()
        .filter_map(|(kind, _, _)| rules.units.get(kind.0 as usize))
        .flat_map(|d| d.required_resources.iter().copied())
        .collect();
    consumes.iter().any(|(resource, taken)| {
        let left = obs.economy.resources.available(*resource) - i32::from(*taken);
        needs.iter().any(|(r, n)| r == resource && left < i32::from(*n))
    })
}

/// Local happiness the city's buildings give it already (their own
/// unconditional happiness).
fn local_happiness(rules: &CompiledRuleset, city: &OwnCity) -> i64 {
    use crate::classic::effects::ClassicModifier;
    city.buildings
        .iter()
        .filter_map(|b| rules.buildings.get(b.0 as usize))
        .flat_map(|def| &def.effects.passive_city)
        .filter(|e| e.selector == Selector::SourceCity && matches!(e.condition, Condition::Always))
        .map(|e| match e.modifier {
            Modifier::Module(ClassicModifier::AddHappiness { amount }) => i64::from(amount),
            _ => 0,
        })
        .sum()
}

/// Trade routes the empire may run (`derived::trade::routes_possible` on
/// what the player knows): the route stats of its technologies, adopted
/// policies and buildings, then one slot per `CITIES_PER_ROUTE_SLOT` cities
/// once it has one.
fn route_slots(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> u32 {
    use crate::rules::stats::{ClassicStat, Stat, TradeStat};
    let techs = obs.economy.techs.iter().filter_map(|t| rules.technologies.get(t.0 as usize)).map(|d| &d.effects);
    let policies = obs.culture.policies.iter().filter_map(|p| rules.module.policies.get(p.0 as usize)).map(|d| &d.effects);
    let buildings = obs.own_cities.iter().flat_map(|c| &c.buildings).filter_map(|b| rules.buildings.get(b.0 as usize)).map(|d| &d.effects);
    let (mut routes, mut percent) = (0i32, 100i32);
    for e in techs.chain(policies).chain(buildings).flat_map(|s| &s.passive_player) {
        match e.modifier {
            Modifier::AddStat { stat: Stat::Module(ClassicStat::Trade(TradeStat::Routes)), amount } => routes += amount,
            Modifier::AddStat { stat: Stat::Module(ClassicStat::Trade(TradeStat::RoutesPercent)), amount } => percent += amount,
            _ => {}
        }
    }
    let slots = (routes.max(0) * percent.max(0) / 100) as u32;
    if slots == 0 {
        return 0;
    }
    slots + (obs.own_cities.len() / crate::derived::trade::CITIES_PER_ROUTE_SLOT) as u32
}

/// Promotions `xp` experience buys a new unit, hundredths: the levels of
/// the rules' ladder it passes (`combat::experience::experience_needed`)
/// and the share of the next.
fn promotions_of(xp: u32) -> i64 {
    use crate::combat::experience::experience_needed;
    let mut level = 1u8;
    let mut below = 0;
    while experience_needed(level) <= xp && level < u8::MAX {
        below = experience_needed(level);
        level += 1;
    }
    let next = experience_needed(level);
    i64::from(level - 1) * 100 + i64::from(xp - below) * 100 / i64::from((next - below).max(1))
}

/// Strength percent a promotion gives on average: the combat percents of
/// the rules' first promotions (choosable, no prerequisite).
fn promotion_percent(rules: &CompiledRuleset) -> i64 {
    let firsts = rules.promotions.iter().filter(|p| p.choosable && p.requires.is_empty() && p.requires_any_of.is_empty());
    let (mut sum, mut count) = (0i64, 0i64);
    for p in firsts {
        sum += p.effects.passive_unit.iter().map(|e| match e.modifier {
            Modifier::ModifyCombat { percent, .. } => i64::from(percent.max(0)),
            _ => 0,
        }).sum::<i64>();
        count += 1;
    }
    if count == 0 { 0 } else { sum / count }
}

impl Prices {
    /// The empire stays content with one more city and its first citizen,
    /// with `FOUNDING_MARGIN` points to spare: an unhappy empire grows at a
    /// quarter, so a city founded into a deficit holds back every other
    /// city's growth. Measured against founding up to the very-unhappy
    /// line: 64.5 % of the points, fewer cities and 20 % more science.
    fn affords_city(&self) -> bool {
        i64::from(self.excess) * 100 >= self.unhappiness.city + self.unhappiness.citizen + FOUNDING_MARGIN * 100
    }
}

/// Points of excess happiness a new city must leave over (`Prices::affords_city`).
pub const FOUNDING_MARGIN: i64 = 2;

/// What the empire loses for the rest of the game when its mood goes from `from` down to `to`:
/// the food that no longer becomes citizens (`growth_room`), and, below the very-unhappy
/// line, half the production (`VERY_UNHAPPY_PRODUCTION_PERCENT`). The rules' own mood effects.
fn mood_step(obs: &AIObservation<'_>, prices: &Prices, from: crate::derived::happiness::Mood, to: crate::derived::happiness::Mood, rounds_left: i64, growth: i64) -> Centi {
    use crate::derived::happiness::{Mood, VERY_UNHAPPY_PRODUCTION_PERCENT};
    let room = |mood: Mood| match mood {
        Mood::Content => 100,
        Mood::Unhappy => 25,
        Mood::VeryUnhappy => 0,
    };
    let gross: i64 = obs.own_cities.iter().map(|c| i64::from((c.income.food - c.income.food_eaten).max(0))).sum();
    let food = gross * (room(from) - room(to)) / 100;
    let citizens = food * rounds_left / growth.max(1);
    let production = if to == Mood::VeryUnhappy {
        let total: i64 = obs.own_cities.iter().map(|c| i64::from(c.income.production.max(0))).sum();
        total * i64::from(-VERY_UNHAPPY_PRODUCTION_PERCENT) / 100 * prices.production
    } else {
        0
    };
    citizens * prices.score.citizen + production
}

impl Prices {
    /// The unrest of a captured city priced by the steps it makes the empire take
    /// (`PriceModel::Uses`; `Tiles` keeps `captured_city_unrest`). The rules' moods are steps: an
    /// unhappy empire (-1 to -10) grows at a quarter and a very unhappy one (-11 and
    /// below) not at all and makes half the production, so a point of unhappiness costs
    /// the empire little between the steps and a step when it crosses one. The city's points of
    /// unrest (`captured_city_unrest` before the happiness price) take the empire from its excess
    /// to the excess less them: each point costs `drift` (the outlook's price of using the
    /// margin up), and a step down a mood costs what that mood costs once. The price of a
    /// point `happiness` is a floor for choices at the margin (a building, a luxury) and
    /// counts every point of the city at the dearest mood the empire is in: it made the unrest
    /// of a city of two citizens 97 points at the median against a worth of 60
    /// (classic/docs/ai-war-audit-current.md).
    pub fn captured_city_unrest_steps(&self, population: u32) -> Centi {
        self.kept_city_unrest_steps(population, population >= crate::ai::combat::ANNEX_POPULATION)
    }

    /// `captured_city_unrest_steps` of a captured city kept `annexed` or as a puppet
    /// (`operations::captured_parts`); `Tiles` does not tell them apart.
    pub fn kept_city_unrest_steps(&self, population: u32, annexed: bool) -> Centi {
        if self.model != PriceModel::Uses {
            return self.captured_city_unrest_flat(population);
        }
        self.unrest_by_steps(self.kept_city_unrest_hundredths(population, annexed))
    }

    /// What `hundredths` of a point of new unhappiness cost the empire by the moods it
    /// crosses (`captured_city_unrest_steps` of any new city: `PriceModel::Uses`).
    pub fn unrest_by_steps(&self, hundredths: i64) -> Centi {
        use crate::derived::happiness::VERY_UNHAPPY_THRESHOLD;
        let points = (hundredths + 99) / 100;
        let after = i64::from(self.excess) - points;
        let mut cost = self.drift * hundredths / 100;
        if self.excess >= 0 && after < 0 {
            cost += self.step_unhappy;
        }
        if i64::from(self.excess) > i64::from(VERY_UNHAPPY_THRESHOLD) && after <= i64::from(VERY_UNHAPPY_THRESHOLD) {
            cost += self.step_line;
        }
        cost
    }

    /// The unhappiness a captured city brings, hundredths of a point of excess happiness
    /// (`captured_city_unrest` before it is priced).
    pub fn captured_city_unrest_hundredths(&self, population: u32) -> i64 {
        self.kept_city_unrest_hundredths(population, population >= crate::ai::combat::ANNEX_POPULATION)
    }

    /// `captured_city_unrest_hundredths` of a captured city kept `annexed` (occupied) or as a
    /// puppet (a city like the empire's own).
    pub fn kept_city_unrest_hundredths(&self, population: u32, annexed: bool) -> i64 {
        use crate::derived::happiness::{
            UNHAPPINESS_FROM_ANNEXED_CITY_TIMES100 as CAPTURED, UNHAPPINESS_FROM_CITY_TIMES100 as CITY,
            UNHAPPINESS_FROM_ANNEXED_CITIZENS_TIMES100 as OCCUPIED, UNHAPPINESS_FROM_CITIZENS_TIMES100 as CITIZEN,
        };
        let citizens = i64::from(population.max(1));
        let rates = self.unhappiness;
        if annexed {
            rates.city * CAPTURED / CITY + citizens * rates.citizen * OCCUPIED / CITIZEN
        } else {
            rates.city + citizens * rates.citizen
        }
    }

    /// What the unhappiness of a captured city of `population` costs the
    /// rest of the game. `Tiles`: a city and its citizens at the occupied
    /// rates of the base rules (5 + 1 per citizen). `Uses`: at the empire's
    /// own rates, as the AI keeps the city (`combat::conquest_decisions`):
    /// a puppet is a city like the empire's own; an annexed one stays
    /// occupied (`derived::happiness`: the captured-city and occupied-citizen
    /// rates over the base rates).
    pub fn captured_city_unrest(&self, population: u32) -> Centi {
        if self.unrest_steps {
            return self.captured_city_unrest_steps(population);
        }
        self.captured_city_unrest_flat(population)
    }

    /// `captured_city_unrest` with every point at the price of a point now.
    pub fn captured_city_unrest_flat(&self, population: u32) -> Centi {
        use crate::derived::happiness::{
            UNHAPPINESS_FROM_ANNEXED_CITY_TIMES100 as CAPTURED, UNHAPPINESS_FROM_CITY_TIMES100 as CITY,
            UNHAPPINESS_FROM_ANNEXED_CITIZENS_TIMES100 as OCCUPIED, UNHAPPINESS_FROM_CITIZENS_TIMES100 as CITIZEN,
        };
        let citizens = i64::from(population.max(1));
        match self.model {
            PriceModel::Tiles => (5 + citizens) * self.happiness,
            PriceModel::Uses => {
                let rates = self.unhappiness;
                let hundredths = if population >= crate::ai::combat::ANNEX_POPULATION {
                    rates.city * CAPTURED / CITY + citizens * rates.citizen * OCCUPIED / CITIZEN
                } else {
                    rates.city + citizens * rates.citizen
                };
                hundredths * self.happiness / 100
            }
        }
    }

    /// The citizens a city's local happiness can reach over the rest of the
    /// game (local happiness counts up to the population): `Uses` counts
    /// the citizens the city grows by then (one every `CITIZEN_ROUNDS`),
    /// `Tiles` its population now.
    fn local_cap(&self, city: &OwnCity) -> i64 {
        let now = i64::from(city.population.max(1));
        match self.model {
            PriceModel::Tiles => now,
            PriceModel::Uses => now + i64::from(self.rounds_left) / self.paced(crate::ai::genome::get(crate::ai::genome::Knob::CitizenRounds)),
        }
    }
}

/// Citizens a point of happiness is worth at least at the very-unhappy line
/// (`Outlook::crisis`): enough that a happiness building or a luxury beats
/// sitting at the line unless the other choice gains a lot of score (bench,
/// offsets 20–21: 4 left 32 % of the seats at or below the line at the
/// end, 6 with `Prices::local_cap` 26 %, 8 28 %, 20 no fewer and less
/// strength; classic/docs/ai-allocator.md).
pub const CRISIS_CITIZENS: i64 = 6;
/// The empire's first cities found the opening: their citizens count as the
/// empire's growth (the food price), not against its happiness: a player
/// founds them for their land.
pub const OPENING_CITIES: u32 = 3;

/// Land tiles of a new city: its first ring.
pub const FIRST_RING: i64 = 7;
/// Rounds a city's borders take for one more tile (bench: ~18 tiles per
/// city after 250 rounds; Standard speed, × the speed's pace).
pub const TILE_ROUNDS: i64 = 25;
/// Rounds a new city takes for one more citizen (bench: ~10 citizens per
/// city after 250 rounds; Standard speed, × the speed's pace).
pub const CITIZEN_ROUNDS: i64 = 25;
/// Yields of a young city a turn.
pub const NEW_CITY_PRODUCTION: i64 = 2;
pub const NEW_CITY_SCIENCE: i64 = 2;
/// A city and its first citizen.
pub const NEW_CITY_UNHAPPINESS: i64 = 4;

/// Fixed-point scale of hammers per gold.
const RATE_SCALE: i64 = 1000;

/// Hammers a gold buys at best now, × `RATE_SCALE`: the production a
/// hurried queue head still needs over its gold price, the best city's
/// (wonders are not for sale; 0 with nothing to buy).
fn hurry_rate(obs: &AIObservation<'_>) -> i64 {
    let rate = |city: &OwnCity| {
        let item = *city.queue.first()?;
        let price = obs.view.own_hurry_price(city.handle)?;
        let cost = obs.view.own_production_cost(city.handle, item)?;
        Some(i64::from(cost.saturating_sub(city.production)) * RATE_SCALE / i64::from(price.max(1)))
    };
    obs.own_cities.iter().filter_map(rate).max().unwrap_or(0)
}

/// Where the empire's excess happiness goes in the rounds left at its
/// growth now (`PriceModel::Uses`): each citizen grown costs its
/// unhappiness, so a content empire turns unhappy, grows at a quarter,
/// and stops at the very-unhappy line.
struct Outlook {
    excess: i32,
    /// Rounds until the empire turns unhappy (0: it is), and until it
    /// reaches the very-unhappy line (0: it is there).
    unhappy_in: i64,
    very_unhappy_in: i64,
    rounds_left: i64,
    /// Percent of the rounds left spent at the very-unhappy line: more
    /// food there only reaches the line sooner.
    capped: i64,
}

/// Fixed-point scale of rounds per point (`Outlook`).
const MILLI: i64 = 1000;

impl Outlook {
    fn of(obs: &AIObservation<'_>, per_citizen: i64, rounds_left: u32) -> Self {
        use crate::derived::happiness::{UNHAPPY_GROWTH_PERCENT, VERY_UNHAPPY_THRESHOLD};
        let excess = obs.economy.happiness.excess();
        let t = i64::from(rounds_left);
        // Citizens a thousand rounds at full growth, and the unhappiness
        // they bring a round (hundredths × MILLI).
        let growth: i64 = obs
            .own_cities
            .iter()
            .map(|c| i64::from((c.income.food - c.income.food_eaten).max(0)) * MILLI / i64::from(crate::systems::growth::speed_growth_threshold(&obs.speed, c.population).max(1)))
            .sum();
        let drift = growth * per_citizen;
        let rounds = |points: i64, percent: i64| match (points, drift) {
            (0, _) => 0,
            (_, 0) => t,
            _ => points * 100 * MILLI * 100 / (drift * percent),
        };
        let slow = 100 + i64::from(UNHAPPY_GROWTH_PERCENT);
        let unhappy_in = rounds(i64::from(excess.max(0)), 100);
        let span = i64::from(excess.min(0) - VERY_UNHAPPY_THRESHOLD).max(0);
        let very_unhappy_in = unhappy_in + rounds(span, slow);
        let mut outlook = Self { excess, unhappy_in, very_unhappy_in, rounds_left: t, capped: 0 };
        outlook.capped = outlook.share(very_unhappy_in);
        outlook
    }

    /// Percent of the rounds left after `rounds`.
    fn share(&self, rounds: i64) -> i64 {
        (self.rounds_left - rounds.clamp(0, self.rounds_left)) * 100 / self.rounds_left.max(1)
    }

    /// One more point of excess happiness for the rest of the game: every
    /// later turn of the outlook comes `1 / drift` rounds later, so the
    /// empire grows `1 / unhappiness per citizen` more citizens by the line
    /// (three quarters of them while content, a quarter while unhappy);
    /// content rounds fill the golden age meter.
    fn happiness(&self, obs: &AIObservation<'_>, prices: &Prices) -> Centi {
        use crate::derived::happiness::UNHAPPY_GROWTH_PERCENT;
        // The citizens one point holds, counted for `share` percent of the
        // rounds left.
        let held = |share: i64| prices.score.citizen * 100 / prices.unhappiness.citizen.max(1) * share / 100;
        let lost = -i64::from(UNHAPPY_GROWTH_PERCENT);
        let growth = if self.excess >= 0 {
            (lost * held(self.share(self.unhappy_in)) + (100 - lost) * held(self.share(self.very_unhappy_in))) / 100
        } else {
            held(self.capped)
        };
        let production: i64 = obs.own_cities.iter().map(|c| i64::from(c.income.production.max(0))).sum();
        // No per-point production or gold penalty (the very-unhappy line is
        // valued by `crisis`).
        growth + self.golden_age(obs, prices, production)
    }

    /// The least a point is worth near the very-unhappy line: the line is a
    /// crisis — no city grows or is founded, production, gold and combat
    /// suffer — that a player gets out of first, whatever the score says
    /// late in the game, when the outlook's time-weighted prices fade. A
    /// point is worth `CRISIS_CITIZENS` citizens times the share of the way
    /// to the line the empire has gone: its deficit over the line's ten
    /// points. (Not the share of the game the outlook expects at the line:
    /// that made every captured city's unrest dearer than the city, and
    /// the conquest matrix lost its capture at 8 tiles.)
    fn crisis(&self, excess: i32, score: ScorePoints) -> Centi {
        use crate::derived::happiness::VERY_UNHAPPY_THRESHOLD;
        let depth = i64::from((-excess).clamp(0, -VERY_UNHAPPY_THRESHOLD)) * 100 / i64::from(-VERY_UNHAPPY_THRESHOLD);
        score.citizen * CRISIS_CITIZENS * depth / 100
    }

    /// The golden ages a point adds while the empire is content: a round's
    /// excess fills the meter, a full meter gives `UNMODIFIED_GOLDEN_AGE_TURNS`
    /// rounds of more production.
    fn golden_age(&self, obs: &AIObservation<'_>, prices: &Prices, production: i64) -> Centi {
        use crate::classic::yields::GOLDEN_AGE_PRODUCTION_PERCENT;
        use crate::systems::golden_age::UNMODIFIED_GOLDEN_AGE_TURNS;
        if self.excess < 0 {
            return 0;
        }
        let content = self.unhappy_in.min(self.rounds_left);
        let threshold = i64::from(obs.economy.golden_age_threshold.max(1));
        let hammers = production * i64::from(GOLDEN_AGE_PRODUCTION_PERCENT) / 100;
        // The golden age's rounds at the game speed (× its golden age percent).
        let turns = i64::from(UNMODIFIED_GOLDEN_AGE_TURNS) * i64::from(obs.speed.golden_age_percent) / 100;
        content * turns * hammers * prices.production / (threshold * self.rounds_left.max(1))
    }
}

/// Share of a new city's citizens that are new to the empire, percent: when
/// happiness binds, its citizens are citizens another city does not grow.
fn growth_share(prices: &Prices) -> i64 {
    100 - prices.happiness * 100 / prices.score.citizen.max(1) / 2
}

/// The value of one more point of excess happiness. Unhappy, the empire
/// grows at a quarter (very unhappy: not at all): each point of the deficit
/// holds back its share of the `stalled` citizens, at least a citizen.
/// Content, a point is a citizen while growth would soon use it up, less
/// with room to spare.
fn happiness_price(score: ScorePoints, excess: i32, stalled: i64) -> Centi {
    if excess < 0 {
        let per_point = (stalled / i64::from(-excess)).clamp(1, MAX_CITIZENS_PER_HAPPINESS);
        return score.citizen * per_point;
    }
    let comfort = crate::ai::genome::get_i32(crate::ai::genome::Knob::HappinessComfort);
    let room = (comfort - excess).clamp(1, comfort);
    score.citizen * i64::from(room) / i64::from(comfort)
}

/// Most citizens one point of happiness is worth.
pub const MAX_CITIZENS_PER_HAPPINESS: i64 = 10;

/// Citizens one point of excess happiness gives back while the empire is
/// in a deficit (`PriceModel::Uses`): the growth the next mood up restores
/// — a quarter of the food surplus out of the very-unhappy state, the rest
/// out of the unhappy one — kept to the end, shared by the points it takes
/// to get there (at most `MAX_CITIZENS_PER_HAPPINESS`). The outlook's
/// linear price misses this step: one point out of a deficit restores the
/// growth of every city. 0 while content.
fn restored_citizens(obs: &AIObservation<'_>, excess: i32, rounds_left: i64, growth: i64) -> i64 {
    use crate::derived::happiness::VERY_UNHAPPY_THRESHOLD;
    if excess >= 0 {
        return 0;
    }
    let gross: i64 = obs.own_cities.iter().map(|c| i64::from((c.income.food - c.income.food_eaten).max(0))).sum();
    let (next, points) = if excess <= VERY_UNHAPPY_THRESHOLD { (VERY_UNHAPPY_THRESHOLD + 1, VERY_UNHAPPY_THRESHOLD + 1 - excess) } else { (0, -excess) };
    let food = gross * (growth_room(next) - growth_room(excess)) / 100;
    (food * rounds_left / growth.max(1) / i64::from(points)).min(MAX_CITIZENS_PER_HAPPINESS)
}

/// Food a turn the empire's unhappiness keeps from growth: the cities'
/// surplus before the growth penalty, times the penalty.
fn stalled_food(obs: &AIObservation<'_>, excess: i32) -> i64 {
    let gross: i32 = obs.own_cities.iter().map(|c| (c.income.food - c.income.food_eaten).max(0)).sum();
    i64::from(gross) * (100 - growth_room(excess)) / 100
}

/// Percent of food that becomes citizens: unhappy cities grow at a quarter.
fn growth_room(excess: i32) -> i64 {
    match crate::derived::happiness::mood(excess) {
        crate::derived::happiness::Mood::Content => 100,
        crate::derived::happiness::Mood::Unhappy => 25,
        crate::derived::happiness::Mood::VeryUnhappy => 0,
    }
}

/// Food of the citizen an average city would grow last by the final round:
/// its surplus now, kept to the end, buys citizens at the growth thresholds
/// (a food more buys a share of the last one).
fn growth_cost(obs: &AIObservation<'_>, rounds_left: u32) -> u32 {
    let cities = obs.own_cities.len().max(1) as u32;
    let population: u32 = obs.own_cities.iter().map(|c| c.population).sum();
    let surplus: i32 = obs.own_cities.iter().map(|c| c.income.food_surplus.max(0)).sum();
    let budget = (surplus as u32 / cities) * rounds_left;
    let threshold = |pop: u32| crate::systems::growth::speed_growth_threshold(&obs.speed, pop).max(1);
    last_step(population / cities + 1, budget, threshold)
}

/// Culture of the border tile an average city would take last by the final
/// round with its culture now.
fn culture_cost(obs: &AIObservation<'_>, rounds_left: u32) -> i64 {
    let cities = obs.own_cities.len().max(1) as u32;
    let owned = |i: usize| obs.view.tile(crate::ids::TileId(i as u32)).snapshot().is_some_and(|s| s.owner == Some(obs.player));
    let tiles = (0..obs.view.tile_count()).filter(|i| owned(*i)).count() as u32;
    let acquired = (tiles / cities).saturating_sub(FIRST_RING as u32);
    let culture: u32 = obs.own_cities.iter().map(|c| c.income.culture).sum();
    let budget = (culture / cities) * rounds_left;
    i64::from(last_step(acquired, budget, crate::world::city::culture_cost))
}

/// The cost of the step a budget spent on rising steps from `first` ends
/// on (at most `MAX_STEPS` steps).
fn last_step(first: u32, mut budget: u32, cost: impl Fn(u32) -> u32) -> u32 {
    let mut step = first;
    for _ in 0..MAX_STEPS {
        let price = cost(step);
        if price > budget {
            return price;
        }
        budget -= price;
        step += 1;
    }
    cost(step)
}

/// Steps `last_step` walks at most.
const MAX_STEPS: u32 = 64;

/// Rounds an empire takes for a technology (bench: 34 in 300 rounds; Standard
/// speed, × the speed's pace).
pub const TECH_ROUNDS: u32 = 9;

/// Beakers of an average technology of the rest of the game: the mean rules
/// cost of the cheapest unknown ones, as many as the empire researches in
/// the rounds left (later technologies cost more).
fn research_cost(obs: &AIObservation<'_>, rules: &CompiledRuleset, rounds_left: u32) -> i64 {
    let known = |t: TechId| obs.economy.techs.contains(&t);
    let mut costs: Vec<i64> = rules
        .technologies
        .iter()
        .enumerate()
        .filter(|(i, _)| !known(TechId(*i as u16)))
        // The rules' cost at the game speed (× its science percent).
        .map(|(_, def)| i64::from(def.cost) * i64::from(obs.speed.science_percent) / 100)
        .collect();
    costs.sort_unstable();
    let ahead = (rounds_left / obs.speed.pace_rounds(crate::ai::genome::get_u32(crate::ai::genome::Knob::TechRounds))).max(1) as usize;
    costs.truncate(ahead);
    if costs.is_empty() {
        return i64::MAX;
    }
    costs.iter().sum::<i64>() / costs.len() as i64
}

/// What each of the `techs` opens: for every building it makes available,
/// the surplus of the building's value (`Prices::building`, at the prices
/// given, with the science already priced) over the hammers it costs, in
/// each city of the empire that would take it, or once for a wonder at a
/// `UNLOCK_WONDER_CHANCE` of being ours. Only gains count: an opening is an
/// option, never a burden.
fn unlocked_value(obs: &AIObservation<'_>, rules: &CompiledRuleset, prices: &Prices, techs: &[(u32, TechId)]) -> kernel::FxHashMap<TechId, Centi> {
    use crate::ids::ProductionItem;
    let hammer = prices.production / i64::from(prices.rounds_left);
    let cities = obs.own_cities.len().max(1) as i32;
    let mut out: kernel::FxHashMap<TechId, Centi> = kernel::FxHashMap::default();
    for (index, def) in rules.buildings.iter().enumerate() {
        let Some(tech) = def.required_tech.filter(|t| techs.iter().any(|(_, x)| x == t)) else { continue };
        let kind = BuildingKindId(index as u16);
        if def.cost == 0 || def.unbuildable || rules.production_gate(ProductionItem::Building(kind), obs.economy.nation).is_some() {
            continue;
        }
        if def.wonder && obs.built_wonders.contains(&kind) {
            continue;
        }
        let paid = i64::from(def.cost) * hammer;
        let value = if def.wonder {
            let city = obs.own_cities.iter().max_by_key(|c| (c.income.production, std::cmp::Reverse(c.id)));
            city.map_or(0, |c| (prices.building(obs, rules, c, kind, cities, 0) - paid).max(0) * UNLOCK_WONDER_CHANCE / 100)
        } else {
            obs.own_cities.iter().map(|c| (prices.building(obs, rules, c, kind, cities, 0) - paid).max(0)).sum()
        };
        *out.entry(tech).or_insert(0) += value;
    }
    out
}

/// Percent of a wonder's surplus that counts as ours when the technology
/// that opens it is learned: the others build wonders too.
pub const UNLOCK_WONDER_CHANCE: i64 = 50;

/// Score a hammer buys in the cheapest world wonder the player may build
/// (hundredths of centi-points per hammer); 0 when none is left.
fn wonder_rate(obs: &AIObservation<'_>, rules: &CompiledRuleset, score: ScorePoints) -> Centi {
    let known = |t: Option<TechId>| t.is_none_or(|t| obs.economy.techs.contains(&t));
    let cheapest = rules
        .buildings
        .iter()
        .enumerate()
        .filter(|(i, def)| def.wonder && def.cost > 0 && known(def.required_tech) && !obs.built_wonders.contains(&BuildingKindId(*i as u16)))
        .map(|(_, def)| i64::from(def.cost))
        .min();
    cheapest.map_or(0, |cost| score.wonder * 100 / (2 * cost))
}

/// The yield a percent of the city applies to.
fn base_yield(city: &OwnCity, kind: YieldKind) -> i32 {
    let income = &city.income;
    match kind {
        YieldKind::Food => income.food,
        YieldKind::Production | YieldKind::Output => income.production,
        YieldKind::Gold => income.gold,
        YieldKind::Science => income.science,
        YieldKind::Culture => income.culture as i32,
        YieldKind::Module(crate::classic::effects::ClassicYield::Faith) => income.faith as i32,
    }
    .max(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rules' map modifier: 100 on a Standard map, 211 on a Duel map
    /// (classic/docs/classic/rules.md); wonders and technologies do not scale.
    #[test]
    fn score_points_follow_the_map_size() {
        let standard = ScorePoints::of_map(4160);
        // Unciv: Nation.kt, ModConstants.kt (city 10, citizen 3, wonder 40).
        assert_eq!((standard.city, standard.citizen, standard.land), (1000, 300, 100));
        let duel = ScorePoints::of_map(960);
        assert_eq!((duel.city, duel.citizen, duel.land), (10 * 211, 3 * 211, 211));
        assert_eq!((duel.wonder, duel.tech), (4000, 400));
    }

    /// The share of the rounds left after a turn of the outlook: all of
    /// them now, none at or after the end.
    #[test]
    fn the_outlook_shares_the_rounds_left() {
        let outlook = Outlook { excess: 3, unhappy_in: 50, very_unhappy_in: 250, rounds_left: 200, capped: 0 };
        assert_eq!((outlook.share(0), outlook.share(50), outlook.share(250), outlook.share(-5)), (100, 75, 0, 100));
    }

    /// The unrest of a captured city by the steps of the moods: each point costs the drift, and a
    /// mood step costs once when the city crosses it. A city of two citizens brings 5 points
    /// (`Unhappiness::BASE`: 3 for the city, 1 a citizen).
    #[test]
    fn captured_unrest_is_priced_by_the_steps_it_crosses() {
        let mut prices = Prices {
            model: PriceModel::Uses,
            score: ScorePoints::of_map(4160),
            rounds_left: 200,
            food: 1_000,
            production: 300,
            gold: 80,
            science: 250,
            culture: 200,
            faith: 40,
            happiness: 2_700,
            unhappiness: Unhappiness::BASE,
            founding_rounds: 200,
            capped: 0,
            cities: 4,
            excess: 8,
            drift: 100,
            step_unhappy: 1_200,
            step_line: 2_500,
            unrest_steps: true,
            content_founding: false,
            true_effects: false,
            space: None,
            pace: 100,
            city_cap: None,
            gift_cap: true,
        };
        let flat = prices.captured_city_unrest_flat(2);
        assert_eq!(flat, 5 * 2_700, "the flat price: five points at the price of a point");
        // Content with room to spare: five points of drift, no step.
        assert_eq!(prices.captured_city_unrest(2), 5 * 100);
        // Content near the line of unhappiness: it crosses into the unhappy mood.
        prices.excess = 3;
        assert_eq!(prices.captured_city_unrest(2), 5 * 100 + 1_200);
        // Unhappy in the middle of the band: no step at all.
        prices.excess = -4;
        assert_eq!(prices.captured_city_unrest(2), 5 * 100);
        // Unhappy next to the very-unhappy line: it crosses it.
        prices.excess = -9;
        assert_eq!(prices.captured_city_unrest(2), 5 * 100 + 2_500);
        // Already very unhappy: the steps are behind it.
        prices.excess = -15;
        assert_eq!(prices.captured_city_unrest(2), 5 * 100);
        // The flat price is the same everywhere; the switch off keeps it.
        prices.unrest_steps = false;
        assert_eq!(prices.captured_city_unrest(2), flat);
    }

    /// Without an empire, a city and a citizen bring the rules' base
    /// unhappiness.
    #[test]
    fn unhappiness_base_rates_are_the_rules() {
        assert_eq!((Unhappiness::BASE.city, Unhappiness::BASE.citizen), (300, 100));
    }

    /// A synthetic ruleset of two technologies in a row; the second opens a
    /// building whose only effect is `effect` (RON of one building effect, or
    /// none).
    fn two_techs(effect: &str) -> crate::rules::CompiledRuleset {
        crate::fixtures::compile_ron(&format!(
            r#"(
    units: [
        (id: "core:unit/settler", movement: 2, strength: 1, founder: true, cost: 40),
        (id: "core:unit/worker", movement: 2, strength: 1, builder: true, cost: 30),
    ],
    buildings: [
        (id: "core:building/hall", cost: 20, effects: [{effect}], required_tech: Some("core:tech/second")),
    ],
    technologies: [
        (id: "core:tech/first", cost: 60),
        (id: "core:tech/second", unlocks_buildings: ["core:building/hall"], cost: 60, requires: ["core:tech/first"]),
    ],
)"#
        ))
    }

    fn prices_of(rules: &crate::rules::CompiledRuleset, pricing: SciencePricing) -> Prices {
        use crate::game::ClassicGame;
        let mut game = crate::GameState::new(
            rules,
            crate::world::state::GameConfig { players: 2, width: 16, height: 16, rng_seed: 1 },
        );
        let tile = game.view().auth.map().dimensions.to_tile(crate::math::HexCoord { q: 8, r: 8 }).unwrap();
        game.debug_found_city(rules, crate::ids::PlayerId(0), tile);
        let obs = game.ai_observation(rules, crate::ids::PlayerId(0));
        Prices::of_with(&obs, rules, PriceModel::Uses, pricing)
    }

    /// A beaker is worth what the technologies it buys open, read from the
    /// declared effects of what they unlock: a building that adds science or
    /// gold makes it dearer, one that adds nothing leaves it at the score of
    /// the technologies. The rules' names and ids play no part (the same
    /// rule holds for the shipped rules in `tests/ai_science.rs`).
    #[test]
    fn a_beaker_is_priced_by_what_the_technologies_open() {
        let gold = "(scope: City, selector: SourceCity, modifier: AddYield(yield_kind: Gold, amount: 3))";
        let science = "(scope: City, selector: SourceCity, modifier: AddYield(yield_kind: Science, amount: 3))";
        let (none, gold, science) = (two_techs(""), two_techs(gold), two_techs(science));
        let direct = prices_of(&none, SciencePricing::Direct).science;
        assert_eq!(prices_of(&none, SciencePricing::Reach).science, direct, "nothing opened: the technologies' own score");
        let (with_gold, with_science) = (prices_of(&gold, SciencePricing::Reach).science, prices_of(&science, SciencePricing::Reach).science);
        assert!(with_gold > direct, "a gold building opened: {with_gold} > {direct}");
        assert!(with_science > direct, "a science building opened: {with_science} > {direct}");
        assert_eq!(prices_of(&science, SciencePricing::Direct).science, direct, "the direct price ignores what is opened");
    }

    /// An open game is planned as long again as it has lasted, never less
    /// than the rounds to its nominal end; a game with a deadline by it.
    #[test]
    fn an_open_game_has_a_horizon() {
        assert_eq!(rounds_planned(500, 100, true), 400, "early: the nominal length");
        assert_eq!(rounds_planned(500, 250, true), 250);
        assert_eq!(rounds_planned(500, 400, true), 400, "late: as long again as played");
        assert_eq!(rounds_planned(500, 650, true), 650, "past the nominal end");
        assert_eq!(rounds_planned(500, 400, false), 100);
        assert_eq!(rounds_planned(500, 650, false), 1);
    }

    /// A budget walks rising steps and ends on the step it cannot pay.
    #[test]
    fn the_last_step_a_budget_reaches() {
        let cost = |step: u32| 10 * (step + 1);
        assert_eq!(last_step(0, 0, cost), 10);
        assert_eq!(last_step(0, 30, cost), 30, "10 + 20 paid, the third step not");
        assert_eq!(last_step(2, 30, cost), 40);
    }
}
