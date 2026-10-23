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
use crate::rules::effects::{CompiledEffect, Condition, Modifier, Trigger, YieldKind};
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
        let score = ScorePoints::of_map(obs.view.tile_count());
        let rounds_left = horizon(obs, pricing);
        let t = i64::from(rounds_left);
        let science = score.tech * t / research_cost(obs, rules, rounds_left).max(1);
        let excess = obs.economy.happiness.excess();
        let growth = i64::from(growth_cost(obs, rounds_left));
        let culture = score.land * t / culture_cost(obs, rounds_left).max(1);
        let production = (wonder_rate(obs, rules, score) * t / 100).max(science * PROCESS_SHARE / 100);
        let tiles = score.land * t / crate::ai::genome::get(crate::ai::genome::Knob::TilePrice);
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
                prices.happiness = outlook.happiness(obs, &prices).max(restored).max(outlook.crisis(excess, score));
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
        ahead.truncate((self.rounds_left / crate::ai::genome::get_u32(crate::ai::genome::Knob::TechRounds)).max(1) as usize);
        if ahead.is_empty() {
            return self.science;
        }
        let opened = unlocked_value(obs, rules, self, &ahead);
        let cost: i64 = ahead.iter().map(|(c, _)| i64::from(*c)).sum();
        let value: Centi = ahead
            .iter()
            .enumerate()
            .map(|(i, (_, tech))| self.score.tech + self.later(opened.get(tech).copied().unwrap_or(0), i as u32 * crate::ai::genome::get_u32(crate::ai::genome::Knob::TechRounds)))
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
    /// world wonder adds its points of the score.
    pub fn building(&self, rules: &CompiledRuleset, city: &OwnCity, kind: BuildingKindId, cities: i32, delay: u32) -> Centi {
        let Some(def) = rules.buildings.get(kind.0 as usize) else {
            return 0;
        };
        let weights = self.weights();
        let set = &def.effects;
        let passive = set.passive_player.iter().chain(&set.passive_city).chain(&set.passive_unit).chain(&set.passive_tile);
        let effects: Centi = passive
            .chain(set.triggered.iter().flatten())
            .map(|e| self.effect(e, city, &weights, cities))
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
    pub fn new_city(&self, delay: u32) -> Centi {
        let left = i64::from(self.rounds_left.saturating_sub(delay));
        if left <= 0 || (self.model == PriceModel::Uses && delay >= self.founding_rounds) {
            return 0;
        }
        let land = self.score.land * (FIRST_RING + left / TILE_ROUNDS);
        let citizens = 1 + left / crate::ai::genome::get(crate::ai::genome::Knob::CitizenRounds);
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

impl Prices {
    /// What the unhappiness of a captured city of `population` costs the
    /// rest of the game. `Tiles`: a city and its citizens at the occupied
    /// rates of the base rules (5 + 1 per citizen). `Uses`: at the empire's
    /// own rates, as the AI keeps the city (`combat::conquest_decisions`):
    /// a puppet is a city like the empire's own; an annexed one stays
    /// occupied (`derived::happiness`: the captured-city and occupied-citizen
    /// rates over the base rates).
    pub fn captured_city_unrest(&self, population: u32) -> Centi {
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
            PriceModel::Uses => now + i64::from(self.rounds_left) / crate::ai::genome::get(crate::ai::genome::Knob::CitizenRounds),
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
/// city after 250 rounds).
pub const TILE_ROUNDS: i64 = 25;
/// Rounds a new city takes for one more citizen (bench: ~10 citizens per
/// city after 250 rounds).
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
            .map(|c| i64::from((c.income.food - c.income.food_eaten).max(0)) * MILLI / i64::from(crate::systems::growth::growth_threshold(c.population).max(1)))
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
        content * i64::from(UNMODIFIED_GOLDEN_AGE_TURNS) * hammers * prices.production / (threshold * self.rounds_left.max(1))
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
    let threshold = |pop: u32| crate::systems::growth::growth_threshold(pop).max(1);
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

/// Rounds an empire takes for a technology (bench: 34 in 300 rounds).
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
        .map(|(_, def)| i64::from(def.cost))
        .collect();
    costs.sort_unstable();
    let ahead = (rounds_left / crate::ai::genome::get_u32(crate::ai::genome::Knob::TechRounds)).max(1) as usize;
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
        if def.cost == 0 || def.unbuildable || rules.production_gate(ProductionItem::Building(kind), obs.economy.civ).is_some() {
            continue;
        }
        if def.wonder && obs.built_wonders.contains(&kind) {
            continue;
        }
        let paid = i64::from(def.cost) * hammer;
        let value = if def.wonder {
            let city = obs.own_cities.iter().max_by_key(|c| (c.income.production, std::cmp::Reverse(c.id)));
            city.map_or(0, |c| (prices.building(rules, c, kind, cities, 0) - paid).max(0) * UNLOCK_WONDER_CHANCE / 100)
        } else {
            obs.own_cities.iter().map(|c| (prices.building(rules, c, kind, cities, 0) - paid).max(0)).sum()
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
        // Unciv: Civilization.kt, ModConstants.kt (city 10, citizen 3, wonder 40).
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
