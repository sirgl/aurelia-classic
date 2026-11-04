//! The owner's hypothesis of a science victory, the standard Civ5 play
//! (classic/docs/ai-science-pace.md): few cities, the growth branch and then
//! the science branch, cities where the science buildings' site conditions
//! hold, internal trade routes. Two variants play it:
//!
//! - `tall-science` (`Tall::Science`) derives every part from the rules'
//!   data and the observation, no content name:
//!   the number of cities that keeps the most science per research cost
//!   (`CityModel`: the per-city technology cost of the map, the happiness a
//!   city costs and brings, the size a city reaches); a site's value adds the
//!   science of the buildings whose site condition it meets (`site_bonuses`);
//!   the policy branches are priced by their science (`science_lens`: food
//!   and happiness as the citizens they keep, beakers as beakers, the rest
//!   nothing) on the started-against-new scale of `policy_branches::priced`;
//!   trade routes and the trade unit are priced by what they bring
//!   (`route_value`), so an internal route that feeds a city and a sea route
//!   that pays more win where they do.
//! - `tall-book` (`Tall::Book`) is a **league opponent and an upper-bound
//!   probe, not a candidate for `standard`**: it follows the plan literally
//!   and rigidly and is the one place in the AI where content is named (the
//!   `BOOK_*` keys below): exactly `BOOK_CITIES` cities, the whole growth
//!   branch then the whole science branch (then the science lens of
//!   `tall-science`), sites next to a mountain where one is in reach, the
//!   science buildings in the book's order before anything but the settlers,
//!   internal food routes, sea ones first.
//!
//! - `tall-book-2` (`Tall::Book2`, `BookPlan::SECOND`) is the book's second
//!   edition (classic/docs/ai-science-audit.md): a scout before the first
//!   settler, and once the science branch is whole the chosen ideology's
//!   tenets before any new branch. `tall-book-2-filler`
//!   (`Tall::Book2Filler`) also takes a filler branch by the science lens
//!   while the science branch waits for its era, instead of saving the
//!   culture (a probe: the filler against the waiting).
//!
//! Both are `Standard` in everything else. Deterministic. Four probes
//! attribute the book's gain: `tall-book-no-order` (the book without its
//! building order), `tall-book-cities` (`Tall::BookCities`: its four cities
//! alone), `tall-book-policies` (`Tall::BookPolicies`: its policies alone),
//! `science-first` (`BuildingOrder::Science`: the science buildings first,
//! ranked from the rules' data by beakers per hammer).

use crate::ai::value::{Centi, Prices};
use crate::command::Command;
use crate::derived::trade::TradeOffer;
use crate::ids::{BuildingKindId, ProductionItem, TileId, UnitKindId};
use crate::knowledge::observation::{AIObservation, OwnCity};
use crate::knowledge::public::PlayerQueries;
use crate::knowledge::view::PlayerView;
use crate::rules::compiled::BuildingPlacement;
use crate::rules::content::PolicyBranchId;
use crate::rules::effects::{Modifier, YieldKind};
use crate::rules::CompiledRuleset;
use crate::world::trade::RouteKind;

/// Which tall play a variant carries (`Variant::tall`, `TurnBrief::tall`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Tall {
    #[default]
    Off,
    /// `tall-science`: the hypothesis from the rules' data.
    Science,
    /// `tall-book`: the hypothesis by the book, content named (a league
    /// opponent and an upper-bound probe, never `standard`).
    Book,
    /// `tall-book-cities`: the book's number of cities alone, `Standard` in
    /// the rest (an attribution probe).
    BookCities,
    /// `tall-book-policies`: the book's policies alone (its two branches,
    /// then the science lens), `Standard` in the rest (an attribution probe).
    BookPolicies,
    /// `tall-book-2`: the book's second edition (`BookPlan::SECOND`).
    Book2,
    /// `tall-book-2-filler`: the second edition that fills the wait for the
    /// science branch with another branch (`BookPlan::FILLER`, a probe).
    Book2Filler,
}

/// What an edition of the book changes in its plan.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BookPlan {
    /// A scout before the settlers when the shared rule of the first recon
    /// unit wants one (`scouting::recon_worth`, the rule of `scout-early`).
    pub scout_first: bool,
    /// Once the science branch is whole, an adoptable tenet of the chosen
    /// ideology before any new branch.
    pub tenets_first: bool,
    /// While the science branch waits for its era, the science lens may
    /// take another branch instead of saving the culture.
    pub filler: bool,
}

impl BookPlan {
    /// The first edition (`tall-book`): no scout, the culture waits, the
    /// science lens after the science branch.
    pub const FIRST: BookPlan = BookPlan { scout_first: false, tenets_first: false, filler: false };
    pub const SECOND: BookPlan = BookPlan { scout_first: true, tenets_first: true, filler: false };
    pub const FILLER: BookPlan = BookPlan { scout_first: true, tenets_first: true, filler: true };
}

impl Tall {
    pub fn on(self) -> bool {
        self != Tall::Off
    }

    /// It chooses its policies, trade routes and sites as the play says
    /// (`tall-science`, `tall-book`).
    pub fn plays(self) -> bool {
        matches!(self, Tall::Science | Tall::Book | Tall::Book2 | Tall::Book2Filler)
    }

    /// It takes the book's policies (`book_policy_command`).
    pub fn book_policies(self) -> bool {
        matches!(self, Tall::Book | Tall::BookPolicies | Tall::Book2 | Tall::Book2Filler)
    }

    /// It keeps exactly the book's cities (`BOOK_CITIES`): founds up to them
    /// first, never beyond.
    pub fn book_cities(self) -> bool {
        matches!(self, Tall::Book | Tall::BookCities | Tall::Book2 | Tall::Book2Filler)
    }

    /// The whole book (sites, routes, cities, policies): either edition.
    pub fn book(self) -> bool {
        matches!(self, Tall::Book | Tall::Book2 | Tall::Book2Filler)
    }

    /// The edition's plan (`BookPlan::FIRST` for every other play).
    pub fn plan(self) -> BookPlan {
        match self {
            Tall::Book2 => BookPlan::SECOND,
            Tall::Book2Filler => BookPlan::FILLER,
            _ => BookPlan::FIRST,
        }
    }
}

/// Which science buildings a city orders before the allocator's choice
/// (`Variant::building_order`, `TurnBrief::building_order`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BuildingOrder {
    #[default]
    Off,
    /// The book's list (`BOOK_BUILDINGS`, `tall-book`).
    Book,
    /// From the rules' data: the building with a science effect that adds
    /// the most beakers in the city per hammer (`science_building`,
    /// `science-first`).
    Science,
}

// ---- the number of cities ----

/// Most cities the model looks at.
pub const MAX_CITIES: u32 = 40;

/// What the city model reads of an empire (hundredths where noted).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CityModel {
    /// Happiness that does not grow with the cities: the handicap, the
    /// luxuries (a new city is not counted on for a new kind), the empire's
    /// buildings and wonders, natural wonders, the Congress, less the
    /// opinion's and the occupied cities' unhappiness.
    pub fixed: i64,
    /// Happiness a city brings: the cities' own (local, capped at their
    /// size, and per city) over the cities, hundredths.
    pub per_city: i64,
    /// Unhappiness of a city and of a citizen, hundredths
    /// (`value::Unhappiness::of`).
    pub city_unhappiness: i64,
    pub citizen_unhappiness: i64,
    /// The size a city reaches in the rounds left (`reachable_size`).
    pub size: i64,
    /// Hundredths of a percent the research cost grows per city beyond the
    /// first (`world_size::tech_cost_per_city`).
    pub cost_per_city: i64,
}

impl CityModel {
    pub fn of(obs: &AIObservation<'_>, rounds_left: u32) -> Self {
        let report = &obs.economy.happiness;
        let cities = obs.own_cities.len().max(1) as i64;
        let rates = crate::ai::value::Unhappiness::of(obs);
        let surplus = obs.own_cities.iter().map(|c| i64::from(c.income.food_surplus.max(0))).sum::<i64>() / cities;
        let largest = obs.own_cities.iter().map(|c| i64::from(c.population)).max().unwrap_or(1);
        let size = WorldSizeOf::of(&obs.view);
        Self {
            fixed: i64::from(report.handicap + report.luxuries + report.buildings + report.natural_wonders + report.league - report.from_opinion - report.from_occupied),
            per_city: i64::from(report.local + report.per_city) * 100 / cities,
            city_unhappiness: rates.city,
            citizen_unhappiness: rates.citizen.max(1),
            size: reachable_size(surplus, rounds_left).max(largest),
            cost_per_city: crate::classic::world_size::tech_cost_per_city(size.0),
        }
    }

    /// Citizens `n` cities keep while the empire stays content, hundredths:
    /// the happiness left after the cities' own unhappiness over a
    /// citizen's, at most `size` a city.
    pub fn citizens(&self, n: u32) -> i64 {
        let n = i64::from(n);
        let budget = (self.fixed * 100 + n * self.per_city - n * self.city_unhappiness).max(0) * 100 / self.citizen_unhappiness;
        budget.min(n * self.size * 100)
    }

    /// Science per research cost with `n` cities, in the units of
    /// `citizens` (the beakers a citizen makes are the same for every `n`).
    pub fn science_per_cost(&self, n: u32) -> i64 {
        let cost = 10_000 + self.cost_per_city * (i64::from(n) - 1);
        self.citizens(n) * 10_000 / cost.max(1)
    }

    /// The number of cities with the most science per research cost (the
    /// fewer on a tie).
    pub fn best(&self) -> u32 {
        (1..=MAX_CITIES).max_by_key(|n| (self.science_per_cost(*n), std::cmp::Reverse(*n))).unwrap_or(1)
    }
}

/// The world size of the map the player sees (map sizes are public).
struct WorldSizeOf(crate::world::size::WorldSize);

impl WorldSizeOf {
    fn of(view: &PlayerView<'_>) -> Self {
        Self(crate::world::size::WorldSize::of(view.dimensions()))
    }
}

/// The size a city of size 1 reaches in `rounds` at `surplus` food a round
/// (the rules' growth thresholds, `systems::growth::growth_threshold`).
pub fn reachable_size(surplus: i64, rounds: u32) -> i64 {
    if surplus <= 0 {
        return 1;
    }
    let mut food = surplus * i64::from(rounds);
    let mut size = 1u32;
    while size < 100 {
        let need = i64::from(crate::systems::growth::growth_threshold(size));
        if food < need {
            break;
        }
        food -= need;
        size += 1;
    }
    i64::from(size)
}

/// The cities `tall-science` keeps: the model's best, never below the
/// cities it has.
pub fn city_target(obs: &AIObservation<'_>, rounds_left: u32) -> u32 {
    CityModel::of(obs, rounds_left).best().max(obs.own_cities.len() as u32)
}

// ---- city sites ----

/// Site points of a food a turn on a worked tile (`planner_settle`'s land
/// tile score: 3 a food; a tile of the worked rings counts 3 times).
pub const SITE_POINTS_PER_FOOD: i32 = 3 * 3;

/// Site points of a beaker a turn: a citizen eats `FOOD_PER_CITIZEN` food
/// and makes `SCIENCE_PER_CITIZEN` beakers, so a beaker is worth the food
/// that feeds the citizen that makes it.
pub fn site_points_per_beaker() -> i32 {
    SITE_POINTS_PER_FOOD * crate::derived::income::FOOD_PER_CITIZEN / crate::classic::yields::SCIENCE_PER_CITIZEN.max(1)
}

/// What each science building with a site condition adds to a site that
/// meets it, in site points: its science in a city of `size` citizens that
/// makes `per_citizen` hundredths of a beaker each (flat beakers, a
/// percent of the city's, beakers per citizen).
pub fn site_bonuses(rules: &CompiledRuleset, size: i64, per_citizen: i64) -> Vec<(BuildingKindId, i32)> {
    let mut out = Vec::new();
    for (index, def) in rules.buildings.iter().enumerate() {
        if def.unbuildable || def.wonder || !has_site_condition(&def.placement) {
            continue;
        }
        let mut hundredths = 0i64;
        for effect in def.effects.passive_city.iter().chain(&def.effects.passive_tile) {
            hundredths += match effect.modifier {
                Modifier::AddYield { yield_kind: YieldKind::Science, amount } => i64::from(amount) * 100,
                Modifier::AddYieldPercent { yield_kind: YieldKind::Science, percent } => size * per_citizen * i64::from(percent) / 100,
                Modifier::AddYieldPerPopulation { yield_kind: YieldKind::Science, hundredths } => size * i64::from(hundredths),
                _ => 0,
            };
        }
        if hundredths > 0 {
            let points = hundredths * i64::from(site_points_per_beaker()) / 100;
            out.push((crate::ids::BuildingKindId(index as u16), points.min(i64::from(i32::MAX)) as i32));
        }
    }
    out
}

/// A placement this module judges on the map: a mountain next to the city
/// or in its reach, a river, the coast, flat ground.
fn has_site_condition(p: &BuildingPlacement) -> bool {
    p.mountain || p.nearby_mountain || p.river || p.water || p.flat
}

/// The radius a city's territory may reach (a mountain "in reach").
const REACH: i32 = crate::world::city::CITY_WORK_RADIUS;

/// Whether `tile` meets the site conditions of `p` as the player knows the
/// map (an unknown tile meets none).
pub fn site_meets(view: &PlayerView<'_>, tile: TileId, p: &BuildingPlacement) -> bool {
    let kinds = view.terrain_kinds();
    let dims = view.dimensions();
    let snap = |t: TileId| view.tile(t).snapshot().copied();
    let mountain = |t: TileId| snap(t).is_some_and(|s| kinds.is_mountains(s.terrain.0));
    let water = |t: TileId| snap(t).is_some_and(|s| kinds.is_water(s.terrain.0));
    let Some(here) = snap(tile) else { return false };
    let next = dims.neighbors(tile);
    if p.mountain && !next.as_slice().iter().any(|t| mountain(*t)) {
        return false;
    }
    if p.nearby_mountain && !crate::math::hex_disc(dims, tile, REACH).any(mountain) {
        return false;
    }
    if p.river && !here.river {
        return false;
    }
    if p.water && !next.as_slice().iter().any(|t| water(*t)) {
        return false;
    }
    !(p.flat && kinds.is_hills(here.terrain.0))
}

/// The bonus of a site: the buildings whose conditions it meets.
pub fn site_bonus(view: &PlayerView<'_>, rules: &CompiledRuleset, tile: TileId, bonuses: &[(BuildingKindId, i32)]) -> i32 {
    bonuses
        .iter()
        .filter(|(kind, _)| rules.buildings.get(kind.0 as usize).is_some_and(|d| site_meets(view, tile, &d.placement)))
        .map(|(_, points)| *points)
        .sum()
}

/// The site bonuses of the turn: `tall-science` prices each science
/// building with a site condition in a city of the size the model expects;
/// `tall-book` gives a site next to a mountain `BOOK_MOUNTAIN_POINTS`.
pub fn turn_site_bonuses(obs: &AIObservation<'_>, rules: &CompiledRuleset, tall: Tall, rounds_left: u32) -> Vec<(BuildingKindId, i32)> {
    match tall {
        Tall::Off | Tall::BookCities | Tall::BookPolicies => Vec::new(),
        Tall::Science => {
            let population: i64 = obs.own_cities.iter().map(|c| i64::from(c.population)).sum();
            let per_citizen = (i64::from(obs.economy.income.science) * 100 / population.max(1)).max(i64::from(crate::classic::yields::SCIENCE_PER_CITIZEN) * 100);
            site_bonuses(rules, CityModel::of(obs, rounds_left).size, per_citizen)
        }
        Tall::Book | Tall::Book2 | Tall::Book2Filler => book_building(rules, BOOK_OBSERVATORY).map(|kind| vec![(kind, BOOK_MOUNTAIN_POINTS)]).unwrap_or_default(),
    }
}

// ---- policies ----

/// The turn's prices seen as beakers (`tall-science`'s policy choice): a
/// beaker at its price; a food and a point of happiness at the citizens
/// they keep (the allocator's price over a citizen's score) times the
/// beakers a citizen makes; production, gold, culture and faith nothing; no
/// new city (`city_cap` 0).
pub fn science_lens(obs: &AIObservation<'_>, prices: &Prices) -> Prices {
    let population: i64 = obs.own_cities.iter().map(|c| i64::from(c.population)).sum();
    let per_citizen = (i64::from(obs.economy.income.science) * 100 / population.max(1)).max(100);
    let citizen = prices.score.citizen.max(1);
    let beakers = |value: Centi| value * per_citizen / 100 * prices.science / citizen;
    let mut lens = *prices;
    lens.food = beakers(prices.food);
    lens.happiness = beakers(prices.happiness);
    lens.production = 0;
    lens.gold = 0;
    lens.culture = 0;
    lens.faith = 0;
    lens.city_cap = Some(0);
    lens
}

/// The culture decision of `tall-science`: the priced choice of
/// `policies::priced_policy_command` through the science lens.
pub fn science_policy_command(obs: &AIObservation<'_>, rules: &CompiledRuleset, memory: &crate::ai::state::AiState, prices: &Prices) -> Option<Command> {
    let lens = science_lens(obs, prices);
    let levers = crate::ai::science_levers::ScienceLevers { policies: true, ..crate::ai::science_levers::ScienceLevers::NONE };
    crate::ai::policies::priced_policy_command(obs, rules, memory, &lens, levers)
}

// ---- trade ----

/// What a route brings at the turn's prices: the origin's gold and
/// science and, on an internal route, the destination's food and
/// production (route yields are hundredths).
pub fn route_value(offer: &TradeOffer, prices: &Prices) -> Centi {
    let mut value = i64::from(offer.origin.gold) * prices.gold + i64::from(offer.origin.science) * prices.science;
    if offer.kind != RouteKind::International {
        value += i64::from(offer.destination_yields.food) * prices.food + i64::from(offer.destination_yields.production) * prices.production;
    }
    value / 100
}

/// The book's key of an offer: an internal food route first, then an
/// internal production one, into the largest city; then the rest by the
/// standard's score.
fn book_route_key(obs: &AIObservation<'_>, offer: &TradeOffer) -> (u8, u32, u32) {
    let size = obs.own_cities.iter().find(|c| c.handle == offer.destination).map_or(0, |c| c.population);
    let kind = match offer.kind {
        RouteKind::Food => 2,
        RouteKind::Production => 1,
        RouteKind::International => 0,
    };
    (kind, size, crate::ai::trade::offer_score(offer))
}

/// `EstablishTradeRoute` for every own trade unit with an offer, the best
/// by `route_value` (`tall-science`) or the book's order (`tall-book`).
pub fn route_commands(obs: &AIObservation<'_>, prices: &Prices, tall: Tall) -> Vec<Command> {
    obs.economy
        .trade_offers
        .iter()
        .filter_map(|(unit, offers)| {
            let tie = |o: &TradeOffer| std::cmp::Reverse((o.destination.index, o.kind.code()));
            let best = match tall {
                Tall::Book | Tall::Book2 | Tall::Book2Filler => offers.iter().max_by_key(|o| (book_route_key(obs, o), tie(o)))?,
                _ => offers.iter().max_by_key(|o| (route_value(o, prices), tie(o)))?,
            };
            Some(Command::EstablishTradeRoute { unit: *unit, destination: best.destination, kind: best.kind })
        })
        .collect()
}

/// The trade units the player may build now, by id.
fn trade_kinds(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Vec<UnitKindId> {
    let known = |t: Option<crate::ids::TechId>| t.is_none_or(|t| obs.economy.techs.contains(&t));
    (0..rules.units.len())
        .map(|i| UnitKindId(i as u16))
        .filter(|k| {
            let def = &rules.units[k.0 as usize];
            def.module.economy.trade && known(def.required_tech) && rules.production_gate(ProductionItem::Unit(*k), obs.economy.nation).is_none()
        })
        .collect()
}

/// The trade unit to build and where, valued by its best route now: each
/// trade unit kind in the most productive city with room in its queue that
/// may build it, by `route_value` (`tall-science`); `tall-book` takes a sea
/// unit wherever one can be built and has an internal route, else a land
/// one. None while no route is left, a trade unit is idle or queued.
pub fn trade_prospect(obs: &AIObservation<'_>, rules: &CompiledRuleset, prices: &Prices, tall: Tall) -> Option<(crate::ids::EntityId, UnitKindId, Centi)> {
    let is_trade = |item: &ProductionItem| matches!(item, ProductionItem::Unit(k) if crate::derived::trade::is_trade_unit(rules, *k));
    let queued = obs.own_cities.iter().any(|c| c.queue.iter().any(is_trade));
    if obs.economy.trade_routes_left == 0 || !obs.economy.trade_offers.is_empty() || queued {
        return None;
    }
    let mut best: Option<(i64, Centi, crate::ids::EntityId, UnitKindId)> = None;
    for kind in trade_kinds(obs, rules) {
        let enqueue = |c: &OwnCity| Command::EnqueueProduction { player: obs.player, city: c.handle, item: ProductionItem::Unit(kind) };
        let Some(city) = obs
            .own_cities
            .iter()
            .filter(|c| c.queue.len() < crate::world::city::MAX_QUEUE && obs.view.accepts(&enqueue(c)))
            .max_by_key(|c| (c.income.production, std::cmp::Reverse(c.id)))
        else {
            continue;
        };
        let offers = obs.view.trade_prospects(city.handle, kind);
        let value = offers.iter().map(|o| route_value(o, prices)).max().unwrap_or(0);
        let rank = match tall {
            Tall::Book | Tall::Book2 | Tall::Book2Filler => {
                let internal = offers.iter().any(|o| o.kind != RouteKind::International);
                let sea = rules.units[kind.0 as usize].domain != crate::rules::content::Domain::Land;
                i64::from(internal) * 2 + i64::from(sea && internal)
            }
            _ => value,
        };
        if value > 0 && best.is_none_or(|(r, v, _, k)| (rank, value) > (r, v) || ((rank, value) == (r, v) && kind.0 < k.0)) {
            best = Some((rank, value, city.id, kind));
        }
    }
    best.map(|(_, value, city, kind)| (city, kind, value))
}

// ---- science buildings first (`science-first`) ----

/// Hundredths of a beaker a turn `kind` adds in `city` now: flat beakers, a
/// percent of the city's science, beakers per citizen.
pub fn beakers_in(rules: &CompiledRuleset, city: &OwnCity, kind: BuildingKindId) -> i64 {
    let Some(def) = rules.buildings.get(kind.0 as usize) else { return 0 };
    def.effects
        .passive_city
        .iter()
        .filter(|e| matches!(e.condition, crate::rules::effects::Condition::Always))
        .map(|e| match e.modifier {
            Modifier::AddYield { yield_kind: YieldKind::Science, amount } => i64::from(amount) * 100,
            Modifier::AddYieldPercent { yield_kind: YieldKind::Science, percent } => i64::from(city.income.science.max(0)) * i64::from(percent),
            Modifier::AddYieldPerPopulation { yield_kind: YieldKind::Science, hundredths } => i64::from(city.population) * i64::from(hundredths),
            _ => 0,
        })
        .sum()
}

/// The building with a science effect (no world wonder) that adds the most
/// beakers in `city` per hammer of its cost, among those `may` lets it take
/// and the rules accept now (ties: the lower id).
pub fn science_building(obs: &AIObservation<'_>, rules: &CompiledRuleset, city: &OwnCity, may: impl Fn(BuildingKindId) -> bool) -> Option<BuildingKindId> {
    let mut best: Option<(i64, BuildingKindId)> = None;
    for (index, def) in rules.buildings.iter().enumerate() {
        let kind = BuildingKindId(index as u16);
        if def.wonder || def.unbuildable || city.buildings.contains(&kind) || city.queue.contains(&ProductionItem::Building(kind)) {
            continue;
        }
        let beakers = beakers_in(rules, city, kind);
        if beakers <= 0 || !may(kind) {
            continue;
        }
        let order = Command::EnqueueProduction { player: obs.player, city: city.handle, item: ProductionItem::Building(kind) };
        if !obs.view.accepts(&order) {
            continue;
        }
        let per_hammer = beakers * 1000 / i64::from(def.cost.max(1));
        if best.is_none_or(|(b, k)| per_hammer > b || (per_hammer == b && kind.0 < k.0)) {
            best = Some((per_hammer, kind));
        }
    }
    best.map(|(_, kind)| kind)
}

// ---- the book (`tall-book` only: content named) ----

/// The book's cities: exactly four.
pub const BOOK_CITIES: u32 = 4;
/// The book's growth branch, completed first.
pub const BOOK_GROWTH_BRANCH: &str = "core:policy/tradition";
/// The book's science branch, completed second.
pub const BOOK_SCIENCE_BRANCH: &str = "core:policy/rationalism";
/// The building whose site the book's cities look for (a mountain next to
/// the city).
pub const BOOK_OBSERVATORY: &str = "core:building/observatory";
/// The book's science buildings, in the order each city builds them.
pub const BOOK_BUILDINGS: [&str; 5] = ["core:building/library", "core:building/university", "core:building/observatory", "core:building/public_school", "core:building/research_lab"];
/// Site points of a site next to a mountain for the book: about ten turns
/// of a settler's walk (`planner_settle::TRAVEL_COST`).
pub const BOOK_MOUNTAIN_POINTS: i32 = 300;

pub fn book_building(rules: &CompiledRuleset, key: &str) -> Option<BuildingKindId> {
    rules.buildings.iter().position(|b| b.key == key).map(|i| BuildingKindId(i as u16))
}

fn book_branch(rules: &CompiledRuleset, key: &str) -> Option<PolicyBranchId> {
    rules.module.policy_branches.iter().position(|b| b.key == key).map(|i| PolicyBranchId(i as u16))
}

/// Every ordinary policy and the finisher of `branch` adopted.
fn complete(obs: &AIObservation<'_>, rules: &CompiledRuleset, branch: PolicyBranchId) -> bool {
    rules
        .module
        .policies
        .iter()
        .enumerate()
        .filter(|(_, p)| p.branch == Some(branch) && p.role != crate::rules::content::PolicyRole::Opener)
        .all(|(i, _)| obs.culture.policies.contains(&crate::ids::PolicyId(i as u16)))
}

/// The book's step in `branch`: unlock it, else adopt its adoptable policy
/// of the lowest row; None when it has nothing to take now.
fn book_step(obs: &AIObservation<'_>, rules: &CompiledRuleset, branch: PolicyBranchId) -> Option<Command> {
    let player = obs.player;
    if !obs.culture.branches.contains(&branch) {
        return obs.culture.unlockable.contains(&branch).then_some(Command::UnlockPolicyBranch { player, branch });
    }
    obs.culture
        .adoptable
        .iter()
        .copied()
        .filter(|p| rules.module.policies.get(p.0 as usize).and_then(|d| d.branch) == Some(branch))
        .min_by_key(|p| (rules.module.policies[p.0 as usize].grid.1, rules.module.policies[p.0 as usize].grid.0, p.0))
        .map(|policy| Command::AdoptPolicy { player, policy })
}

/// The book's culture decision: the growth branch whole, then the science
/// branch whole (the first edition's culture waits for it; `plan.filler`
/// takes the science lens's choice meanwhile), then (`plan.tenets_first`)
/// the best adoptable tenet of the chosen ideology by the science lens, then
/// the science lens.
pub fn book_policy_command(obs: &AIObservation<'_>, rules: &CompiledRuleset, memory: &crate::ai::state::AiState, prices: &Prices, plan: BookPlan) -> Option<Command> {
    if obs.culture.ideologies.is_empty() {
        for key in [BOOK_GROWTH_BRANCH, BOOK_SCIENCE_BRANCH] {
            let Some(branch) = book_branch(rules, key) else { continue };
            if complete(obs, rules, branch) {
                continue;
            }
            let step = book_step(obs, rules, branch);
            // The science branch not yet open: the filler edition spends the
            // culture by the science lens on what is open now.
            if step.is_none() && plan.filler && key == BOOK_SCIENCE_BRANCH && !obs.culture.branches.contains(&branch) {
                return science_policy_command(obs, rules, memory, prices);
            }
            return step;
        }
        if plan.tenets_first {
            if let Some(tenet) = book_tenet(obs, rules, memory, prices) {
                return Some(tenet);
            }
        }
    }
    science_policy_command(obs, rules, memory, prices)
}

/// The adoptable tenet of an ideology the science lens values most (ties:
/// the lower policy), None when no tenet is adoptable.
fn book_tenet(obs: &AIObservation<'_>, rules: &CompiledRuleset, memory: &crate::ai::state::AiState, prices: &Prices) -> Option<Command> {
    let ideology = |p: &crate::ids::PolicyId| {
        let branch = rules.module.policies.get(p.0 as usize).and_then(|d| d.branch);
        branch.and_then(|b| rules.module.policy_branches.get(b.0 as usize)).is_some_and(|b| b.kind == crate::classic::content::PolicyBranchKind::Ideology)
    };
    let tenets: Vec<crate::ids::PolicyId> = obs.culture.adoptable.iter().copied().filter(ideology).collect();
    if tenets.is_empty() {
        return None;
    }
    let lens = science_lens(obs, prices);
    let levers = crate::ai::science_levers::ScienceLevers { policies: true, ..crate::ai::science_levers::ScienceLevers::NONE };
    let market = crate::ai::policy_branches::priced::PolicyMarket::new(obs, rules, memory, &lens, levers);
    let policy = tenets.into_iter().max_by_key(|p| (market.policy(*p), std::cmp::Reverse(p.0)))?;
    Some(Command::AdoptPolicy { player: obs.player, policy })
}

/// The book's next science building for `city`: the first of
/// `BOOK_BUILDINGS` it lacks, when `may` (the planner's availability) lets
/// it and the rules accept it now; the next one waits for it.
pub fn book_next_building(obs: &AIObservation<'_>, rules: &CompiledRuleset, city: &OwnCity, may: impl Fn(BuildingKindId) -> bool) -> Option<BuildingKindId> {
    for key in BOOK_BUILDINGS {
        let Some(kind) = book_building(rules, key) else { continue };
        if city.buildings.contains(&kind) || city.queue.contains(&ProductionItem::Building(kind)) {
            continue;
        }
        let order = Command::EnqueueProduction { player: obs.player, city: city.handle, item: ProductionItem::Building(kind) };
        if may(kind) && obs.view.accepts(&order) {
            return Some(kind);
        }
        // A building the city cannot take (no technology, no mountain)
        // does not hold the next one back only when its site never will.
        let site_never = rules.buildings[kind.0 as usize].placement.mountain && !site_meets(&obs.view, city.tile, &rules.buildings[kind.0 as usize].placement);
        if !site_never {
            return None;
        }
    }
    None
}
