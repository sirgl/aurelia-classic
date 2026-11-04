//! The allocator of the game's AI (`allocator`, `standard`, classic/docs/ai-allocator.md): one
//! turn-level owner of the empire's production and gold that values every
//! use in points of the final score (`ai::value`) and picks the best per
//! unit of what it spends.
//!
//! Production: a city with room in its queue takes, after the needs the
//! baseline meets first (a garrison, a threatened city's defender, the units
//! a war operation lacks, the space race, a scout), the order with the most
//! value per turn of its production — a settler, a worker, a building or a
//! world wonder. Queued orders are commitments: nothing valued replaces
//! them. Gold: above the emergency reserve and the war chest of the running
//! operations, the purchase with the most value per gold — a queue head
//! (the production it frees and the turns it saves) or a tile (its land
//! points and what it yields); under `PriceModel::Uses` also a gold gift
//! toward a city-state's alliance, and a trade unit among the orders
//! (classic/docs/ai-allocator.md, "Gold and happiness").
//!
//! Deterministic: candidates in table and city order, ties to the lower id.

use crate::ai::choice::{self, ChoiceMode, ProductionOption};
use crate::ai::planner::{role_of, AiRole, Decision, TurnBrief};
use crate::knowledge::public::PlayerQueries;
use crate::ai::planner_cities::{building_available, emergency_reserve, order, turns, unit_of_role, Empire, Slot, SETTLER_MIN_POPULATION};
use crate::ai::state::{AiState, OperationPhase};
use crate::ai::taste::{self, Sides};
use crate::ai::trace::{Actor, Reason};
use crate::ai::value::{Centi, PriceModel, Prices};
use crate::command::Command;
use crate::derived::income::unit_upkeep_at;
use crate::ids::{ProductionItem, TileId};
use crate::knowledge::observation::{AIObservation, OwnCity};
use crate::rules::content::Personality;
use crate::rules::CompiledRuleset;

/// Settlers on the way or queued at once (2 gave 0.4 fewer cities at round
/// 100 and the same win share).
pub const MAX_SETTLERS: usize = 3;
/// Rounds a settler walks to its site.
pub const SETTLE_WALK_ROUNDS: u32 = 6;
/// Unimproved worked tiles one worker serves.
pub const JOBS_PER_WORKER: u32 = 4;
/// Yields an improvement adds, percent (a farm or a mine one, a resource's
/// improvement two or three).
pub const IMPROVEMENT_YIELDS: i64 = 150;
/// Rounds a worker takes for one improvement, the walk included (Standard
/// speed; × the speed's pace: `SpeedValues::pace_rounds`).
pub const IMPROVE_ROUNDS: u32 = 8;
/// The empire's first worker is worth this many times the yields of the
/// jobs it takes, once a settler is on the way (or the second city stands):
/// its first improvements, a luxury among them, come a settler's cycle
/// sooner (600 seats at round 60: excess happiness +7.0 instead of +3.9;
/// bench `pace`: cities at round 100 3.6 instead of 3.1; platform/docs/ai-bench.md,
/// "Happiness and the first worker").
pub const FIRST_WORKER_FACTOR: i64 = 3;
/// A building taking longer than this is not ordered (production turns at
/// Standard speed; × the speed's pace).
pub const BUILDING_TURNS: u32 = 40;
/// A world wonder taking longer than this is not ordered (production turns
/// at Standard speed; × the speed's pace).
pub const WONDER_TURNS: u32 = 60;
/// A wonder's chance to be ours falls by this many percent per turn it
/// takes (another nation may finish it first); a turn at Standard speed (a
/// turn of another speed counts 100 / its pace).
pub const WONDER_RISK_PER_TURN: i64 = 2;
/// The least chance a long wonder keeps, percent.
pub const WONDER_MIN_CHANCE: i64 = 30;
/// Tiles bought in one turn.
pub const TILES_PER_TURN: usize = 2;

/// The allocator of one player's turn.
#[derive(Debug, Clone)]
pub struct Allocator {
    prices: Prices,
    cities: i32,
    /// Worked land tiles without an improvement.
    jobs: u32,
    /// A new city keeps the empire content (or the empire is young).
    may_settle: bool,
    /// Gold the running operations keep for their army's upkeep.
    war_chest: u32,
    personality: Personality,
    /// A trade unit worth building, priced (`PriceModel::Uses`).
    trade: Option<TradeProspect>,
    /// The water deposits a work boat could improve, by value
    /// (`ai::sea_work`).
    sea: Vec<Centi>,
    /// How the choice among the best orders is made (`ai::choice`), and the
    /// empire's features for it (None: the allocator's choice, unrecorded).
    choice: ChoiceMode,
    empire: Option<[i32; choice::EMPIRE_FEATURES]>,
    /// The cities a tall empire keeps and whether it founds up to them
    /// whatever the price (`ai::tall`; None: no cap).
    tall: Option<(u32, bool)>,
    /// `trade-fit` (`TurnBrief::trade_fit`): what its routes keep away from;
    /// every choosing city prices its own trade unit (`trade::fit_prospect`)
    /// instead of `trade`'s one city. None: off.
    fit: Option<crate::ai::trade::Hazards>,
    /// Trade units ordered this turn (`trade-fit`: they take route slots).
    trade_ordered: std::cell::Cell<u32>,
    /// The work a worker more would find (`WorkersFit::demand`; None: the
    /// jobs on the worked tiles alone decide).
    load: Option<crate::ai::planner_workers::WorkLoad>,
    /// The strategy net's plan (`ai::strategy_net`): the choice among the best orders
    /// is its production head's. None: the allocator's own.
    strategy: Option<std::sync::Arc<crate::ai::strategy_net::Plan>>,
    /// The unit net's turn (`ai::unit_net`): its commander's orders are offered. None: off.
    #[cfg(feature = "experimental-ai")]
    unit_net: Option<std::sync::Arc<crate::ai::unit_net::UnitNetTurn>>,
}

/// A trade unit the most productive city could build while routes are
/// left: the value of its best route now.
#[derive(Debug, Clone, Copy)]
struct TradeProspect {
    city: crate::ids::EntityId,
    kind: crate::ids::UnitKindId,
    value: Centi,
}

impl Allocator {
    pub fn new(obs: &AIObservation<'_>, brief: &TurnBrief, state: &AiState, rules: &CompiledRuleset) -> Self {
        // The turn's prices (from its start) unless the brief has none.
        let prices = brief.prices.unwrap_or_else(|| Prices::of(obs, rules, Default::default()));
        let empire = (brief.choice != ChoiceMode::Off || choice::log::enabled()).then(|| choice::empire_features(brief, rules, Some(&prices)));
        Self {
            choice: brief.choice,
            empire,
            prices,
            cities: obs.own_cities.len() as i32,
            jobs: if brief.effects.demand { finder_jobs(obs, brief, state, rules) } else { open_jobs(obs) },
            may_settle: may_settle(obs, brief, state, prices.model),
            war_chest: war_chest(obs, state) + state.war_plan.map_or(0, |plan| plan.chest),
            personality: brief.personality,
            sea: brief.sea_jobs.clone(),
            trade: match prices.model {
                PriceModel::Uses if brief.trade_fit => None,
                PriceModel::Uses if brief.tall.plays() => crate::ai::tall::trade_prospect(obs, rules, &prices, brief.tall).map(|(city, kind, value)| TradeProspect { city, kind, value }),
                PriceModel::Uses => trade_prospect(obs, rules, &prices),
                PriceModel::Tiles => None,
            },
            tall: brief.tall_cities.map(|cap| (cap, brief.tall.book_cities())),
            fit: (brief.trade_fit && prices.model == PriceModel::Uses && obs.economy.trade_routes_left > 0).then(|| crate::ai::trade::Hazards::of(obs, rules)),
            trade_ordered: std::cell::Cell::new(0),
            load: brief.workers.demand.then(|| crate::ai::planner_workers::WorkLoad::of(&obs.view, rules, brief, state.strategy.current)),
            strategy: brief.strategy.clone(),
            #[cfg(feature = "experimental-ai")]
            unit_net: brief.unit_net.clone(),
        }
    }

    pub fn prices(&self) -> &Prices {
        &self.prices
    }

    /// The empire's demands the allocator owns: settlers while a city keeps
    /// the empire content; workers by their value (`choose`).
    pub(crate) fn adjust(&self, empire: &mut Empire) {
        empire.settler_target = if self.may_settle { empire.sites.min(crate::ai::genome::get_usize(crate::ai::genome::Knob::MaxSettlers)) } else { 0 };
        // A tall empire founds no city beyond its cap; the book founds up to it.
        if let Some((cap, floor)) = self.tall {
            let left = (cap as usize).saturating_sub(self.cities.max(0) as usize);
            let most = empire.sites.min(crate::ai::genome::get_usize(crate::ai::genome::Knob::MaxSettlers));
            empire.settler_target = if floor { most.min(left) } else { empire.settler_target.min(left) };
        }
    }

    /// The most valuable order per turn of the city's production, or of
    /// the near-equal ones the one the leader loves (`ai::taste`; None:
    /// nothing is worth building). Under a choice mode (`ai::choice`) the
    /// choice among the best ones may be another.
    pub(crate) fn choose(&self, obs: &AIObservation<'_>, empire: &mut Empire, rules: &CompiledRuleset, slot: &Slot<'_>) -> Option<Decision> {
        self.choose_within(obs, empire, rules, slot, true)
    }

    /// `choose` among the buildings and wonders beyond the build-time limits too
    /// (`BUILDING_TURNS`, `WONDER_TURNS`): a city that would otherwise end its turn
    /// idle (`Variant::no_idle_cities`).
    pub(crate) fn choose_unlimited(&self, obs: &AIObservation<'_>, empire: &mut Empire, rules: &CompiledRuleset, slot: &Slot<'_>) -> Option<Decision> {
        self.choose_within(obs, empire, rules, slot, false)
    }

    fn choose_within(&self, obs: &AIObservation<'_>, empire: &mut Empire, rules: &CompiledRuleset, slot: &Slot<'_>, limited: bool) -> Option<Decision> {
        let mut options: Vec<(i64, ProductionItem, Reason)> = Vec::new();
        let mut valued: Vec<ProductionOption> = Vec::new();
        let mut seen = probe::enabled().then(Vec::new);
        for (item, value, reason) in self.candidates(obs, empire, rules, slot, limited) {
            let time = turns(obs, rules, slot.city, item);
            if let Some(seen) = seen.as_mut() {
                seen.push(probe::Candidate { item, value, turns: time });
            }
            if value > 0 && time != u32::MAX {
                let ratio = value / i64::from(time.max(1));
                options.push((ratio, item, reason));
                if self.empire.is_some() || self.strategy.is_some() {
                    valued.push(ProductionOption { item, value, turns: time, ratio });
                }
            }
        }
        let picked = taste::pick(&self.personality, &options, |o| o.0, |o| Sides::of_item(rules, o.1));
        let picked = match (&self.empire, &self.strategy) {
            (_, Some(plan)) => {
                // The strategy net's production head (`ai::strategy_net::heads::production`).
                let at = picked.and_then(|p| options.iter().position(|o| std::ptr::eq(o, p)));
                let at = crate::ai::strategy_net::heads::production(obs, rules, &self.prices, plan, slot.city, self.cities, &valued, at);
                at.map(|i| (options[i].1, options[i].2))
            }
            (Some(features), None) => {
                let at = picked.and_then(|p| options.iter().position(|o| std::ptr::eq(o, p)));
                let at = choice::choose_production(self.choice, features, obs, rules, &self.prices, slot.city, self.cities, &valued, at);
                at.map(|i| (options[i].1, options[i].2))
            }
            (None, None) => picked.map(|&(_, item, reason)| (item, reason)),
        };
        if let Some(seen) = seen {
            probe::record(obs, slot.city, &self.prices, seen, picked.map(|p| p.0));
        }
        let (item, reason) = picked?;
        self.take(empire, rules, item);
        Some(order(empire, slot.city, item, reason))
    }

    /// Count an order in the empire's totals.
    fn take(&self, empire: &mut Empire, rules: &CompiledRuleset, item: ProductionItem) {
        match item {
            ProductionItem::Unit(kind) if self.fit.is_some() && crate::derived::trade::is_trade_unit(rules, kind) => self.trade_ordered.set(self.trade_ordered.get() + 1),
            ProductionItem::Unit(kind) => match role_of(rules, kind) {
                AiRole::Settler => empire.settlers += 1,
                AiRole::Worker if crate::ai::sea_work::is_sea_builder(rules, kind) => empire.sea_builders += 1,
                AiRole::Worker => empire.workers += 1,
                AiRole::Civilian => {}
                _ => empire.military += 1,
            },
            ProductionItem::Building(kind) => {
                let def = &rules.buildings[kind.0 as usize];
                if def.wonder || def.kind == crate::rules::content::BuildingKind::NationalWonder {
                    empire.wonders_taken.push(kind);
                }
            }
            ProductionItem::Project(_) => {}
        }
    }

    /// Every order the city may take now with its value once done.
    fn candidates(&self, obs: &AIObservation<'_>, empire: &Empire, rules: &CompiledRuleset, slot: &Slot<'_>, limited: bool) -> Vec<(ProductionItem, Centi, Reason)> {
        let mut out = Vec::new();
        let city = slot.city;
        if empire.settlers < empire.settler_target && city.population >= SETTLER_MIN_POPULATION {
            if let Some(kind) = unit_of_role(obs, empire, rules, AiRole::Settler) {
                let item = ProductionItem::Unit(kind);
                let delay = turns(obs, rules, city, item).saturating_add(SETTLE_WALK_ROUNDS);
                out.push((item, self.prices.new_city(delay), Reason::ProduceSettler));
            }
        }
        if let Some(kind) = unit_of_role(obs, empire, rules, AiRole::Worker) {
            let item = ProductionItem::Unit(kind);
            let value = self.worker(obs, empire, empire.workers, turns(obs, rules, city, item));
            out.push((item, value, Reason::ProduceWorker));
        }
        if let Some(value) = self.sea_value(obs, empire, rules, city) {
            if let Some(kind) = crate::ai::planner_cities::sea_builder(obs, empire, rules) {
                let item = ProductionItem::Unit(kind);
                let time = turns(obs, rules, city, item);
                if time != u32::MAX {
                    out.push((item, self.prices.later(value, time + crate::ai::sea_work::SEA_JOB_ROUNDS), Reason::ProduceWorker));
                }
            }
        }
        if let Some(trade) = self.trade.filter(|t| t.city == city.id) {
            let item = ProductionItem::Unit(trade.kind);
            let value = self.prices.later(trade.value, turns(obs, rules, city, item));
            out.push((item, value, Reason::ProduceTradeUnit));
        }
        if let Some(hazards) = &self.fit {
            if let Some((kind, value)) = crate::ai::trade::fit_prospect(obs, rules, &self.prices, hazards, city, self.trade_ordered.get()) {
                let item = ProductionItem::Unit(kind);
                out.push((item, self.prices.later(value, turns(obs, rules, city, item)), Reason::ProduceTradeUnit));
            }
        }
        if let Some(plan) = &self.strategy {
            // A military unit when the strategy net's plan wants an army (`heads::military_value`).
            if let Some(kind) = unit_of_role(obs, empire, rules, AiRole::Military) {
                let item = ProductionItem::Unit(kind);
                let value = crate::ai::strategy_net::heads::military_value(plan, &self.prices, rules.units[kind.0 as usize].cost);
                if value > 0 {
                    out.push((item, value, Reason::ProduceArmy));
                }
            }
        }
        #[cfg(feature = "experimental-ai")]
        if let Some(turn) = self.unit_net.as_ref().filter(|t| (empire.military as usize) < t.orders.len()) {
            // The unit net's orders (`unit_net::commander::orders`): a defender near a threatened city.
            let value = crate::ai::unit_net::order_value(turn, city.tile, obs.view.dimensions());
            if value > 0 {
                if let Some(kind) = unit_of_role(obs, empire, rules, AiRole::Military) {
                    out.push((ProductionItem::Unit(kind), value, Reason::ProduceDefender));
                }
            }
        }
        for index in 0..rules.buildings.len() {
            let kind = crate::ids::BuildingKindId(index as u16);
            if !building_available(obs, empire, rules, slot, kind) {
                continue;
            }
            let item = ProductionItem::Building(kind);
            let time = turns(obs, rules, city, item);
            let wonder = rules.buildings[index].wonder;
            // The limits are Standard production turns: production costs scale with the game speed.
            // A goal of the strategy net's plan is offered beyond them (`ai::strategy_net`).
            let goal = self.strategy.as_ref().is_some_and(|p| p.goal_of_building(kind).is_some());
            if time == u32::MAX || (limited && !goal && time > obs.speed.pace_rounds(if wonder { WONDER_TURNS } else { BUILDING_TURNS })) {
                continue;
            }
            let mut value = self.prices.building(obs, rules, city, kind, self.cities, time);
            if wonder {
                let standard_time = i64::from(time) * 100 / i64::from(obs.speed.pace_percent.max(1));
                let chance = (100 - WONDER_RISK_PER_TURN * standard_time).max(crate::ai::genome::get(crate::ai::genome::Knob::WonderMinChance));
                value -= self.prices.score.wonder * (100 - chance) / 100;
            }
            out.push((item, value, Reason::ProduceBuilding));
        }
        out
    }

    /// The value of the first water deposit no work boat, living or queued,
    /// is meant for; None without one.
    fn sea_value(&self, obs: &AIObservation<'_>, empire: &Empire, rules: &CompiledRuleset, city: &OwnCity) -> Option<Centi> {
        if self.sea.is_empty() || !crate::ai::fleet::coastal(&obs.view, city.tile) {
            return None;
        }
        let boats = empire.sea_builders;
        self.sea.get(boats).copied().filter(|_| crate::ai::planner_cities::sea_builder(obs, empire, rules).is_some())
    }

    /// A worker more, with `workers` living or queued, ready in `delay`
    /// rounds: the jobs the others leave, each a yield of food or
    /// production from when it is done, less the worker's upkeep. The first
    /// one counts `FIRST_WORKER_FACTOR` times once a settler is on the way
    /// (before that the settler comes first: the second city is not
    /// delayed).
    fn worker(&self, obs: &AIObservation<'_>, empire: &Empire, workers: usize, delay: u32) -> Centi {
        let per_worker = crate::ai::genome::get_u32(crate::ai::genome::Knob::JobsPerWorker);
        // `WorkersFit::demand`: no worker more without its share of the work
        // the rules still offer (a worker cannot be disbanded: its upkeep runs
        // to the end of the game).
        if self.load.is_some_and(|load| workers > 0 && !load.wants(workers, empire.settlers, delay, per_worker)) {
            return 0;
        }
        let served = per_worker * workers as u32;
        let jobs = self.jobs.saturating_sub(served).min(per_worker);
        let yield_price = crate::ai::genome::get(crate::ai::genome::Knob::ImprovementYields) * self.prices.food.max(self.prices.production) / 100;
        let mut improvements: Centi = (1..=jobs).map(|k| self.prices.later(yield_price, delay.saturating_add(k * obs.speed.pace_rounds(IMPROVE_ROUNDS)))).sum();
        if workers == 0 && (self.cities >= 2 || empire.settlers > 0) {
            improvements *= crate::ai::genome::get(crate::ai::genome::Knob::FirstWorkerFactor);
        }
        let units = obs.own_units.len();
        let upkeep = i64::from(unit_upkeep_at(units + 1, obs.round, obs.speed.max_rounds()) - unit_upkeep_at(units, obs.round, obs.speed.max_rounds()));
        improvements - self.prices.later(upkeep * self.prices.gold, delay)
    }

    /// Purchases with the gold above the emergency reserve and the war
    /// chest, best value per gold first: queue heads (at most one city in
    /// three; never below zero income from the bought item's upkeep),
    /// tiles, and under `PriceModel::Uses` a gold gift to a city-state (one
    /// a turn).
    pub(crate) fn spend_gold(&self, obs: &AIObservation<'_>, brief: &TurnBrief, empire: &Empire, rules: &CompiledRuleset) -> Vec<Decision> {
        let reserve = self.closing(emergency_reserve(obs, brief) + self.war_chest);
        let mut spendable = obs.economy.treasury.saturating_sub(reserve);
        let mut offers = self.hurry_offers(obs, empire, rules);
        offers.extend(self.tile_offers(obs, spendable));
        if self.prices.model == PriceModel::Uses {
            offers.extend(self.gift_offers(obs, spendable));
        }
        offers.sort_by_key(|o| (std::cmp::Reverse(o.ratio()), o.price, o.tie));
        let hurry_limit = obs.own_cities.len().div_ceil(3);
        let (mut hurries, mut tiles, mut gifts) = (0, 0, 0);
        let mut income = obs.economy.income.gold;
        let mut out = Vec::new();
        for offer in offers {
            let (room, reason) = match offer.command {
                Command::PurchaseTile { .. } => (tiles < TILES_PER_TURN, Reason::BuyTile),
                Command::GiftGold { .. } => (gifts < 1, Reason::GiftGold),
                _ => (hurries < hurry_limit, Reason::Hurry),
            };
            if !room || offer.price > spendable || income - offer.upkeep < 0 || offer.ratio() <= self.least_ratio() {
                continue;
            }
            let bought = |d: &Decision| matches!(d.command, Some(Command::PurchaseTile { tile: t, .. }) if Some(t) == offer.tile());
            if out.iter().any(bought) {
                continue;
            }
            // The rules' own check, before the gold is counted as spent.
            if !obs.view.accepts(&offer.command) {
                continue;
            }
            income -= offer.upkeep;
            spendable -= offer.price;
            match reason {
                Reason::BuyTile => tiles += 1,
                Reason::GiftGold => gifts += 1,
                _ => hurries += 1,
            }
            out.push(Decision::act(offer.actor, reason, offer.command));
        }
        out
    }

    /// Gold gifts to city-states the gold above the reserve pays
    /// (`ai::city_states::priced_gifts`).
    fn gift_offers(&self, obs: &AIObservation<'_>, spendable: u32) -> Vec<Offer> {
        crate::ai::city_states::priced_gifts(obs, &self.prices, spendable)
            .into_iter()
            .map(|gift| {
                let command = Command::GiftGold { player: obs.player, city_state: gift.city_state, gold: gift.gold };
                Offer { actor: Actor::Player, tie: u64::from(gift.city_state.0), price: gift.gold, value: gift.value, upkeep: 0, command }
            })
            .collect()
    }

    /// The value a gold must buy at least: half a tile's land per average
    /// tile price.
    fn least_ratio(&self) -> i64 {
        i64::from(self.closing((self.prices.score.land * RATIO_SCALE / (2 * crate::ai::genome::get(crate::ai::genome::Knob::TilePrice))) as u32))
    }

    /// What gold kept for later is still for near the end of the game
    /// (`PriceModel::Uses`): gold left at the last round buys nothing, so
    /// in the last `DEFICIT_RESERVE_TURNS` rounds the reserve and the least
    /// a purchase must be worth shrink with the rounds left.
    fn closing(&self, amount: u32) -> u32 {
        let full = crate::ai::planner_cities::DEFICIT_RESERVE_TURNS;
        match self.prices.model {
            PriceModel::Uses => amount * self.prices.rounds_left.min(full) / full,
            PriceModel::Tiles => amount,
        }
    }

    /// Buying a queue head frees the production it still needs and brings
    /// its value the turns earlier it saves. Wonders are not for sale.
    fn hurry_offers(&self, obs: &AIObservation<'_>, empire: &Empire, rules: &CompiledRuleset) -> Vec<Offer> {
        let mut out = Vec::new();
        let hammer = self.prices.production / i64::from(self.prices.rounds_left);
        for city in &obs.own_cities {
            let Some(&item) = city.queue.first() else { continue };
            let Some(price) = obs.view.own_hurry_price(city.handle) else { continue };
            let Some(cost) = obs.view.own_production_cost(city.handle, item) else { continue };
            let time = turns(obs, rules, city, item);
            let saved = self.prices.later(self.item_value(obs, empire, rules, city, item), 0) * i64::from(time.min(self.prices.rounds_left)) / i64::from(self.prices.rounds_left);
            let freed = i64::from(cost.saturating_sub(city.production)) * hammer;
            let upkeep = crate::ai::planner_cities::item_upkeep(rules, item);
            let command = Command::HurryProduction { player: obs.player, city: city.handle };
            out.push(Offer { actor: Actor::City(city.id), tie: city.id.0, price, value: saved + freed, upkeep, command });
        }
        out
    }

    /// The value of an item once done now (units other than settlers and
    /// workers: none here — their needs come first anyway).
    fn item_value(&self, obs: &AIObservation<'_>, empire: &Empire, rules: &CompiledRuleset, city: &OwnCity, item: ProductionItem) -> Centi {
        match item {
            ProductionItem::Building(kind) => self.prices.building(obs, rules, city, kind, self.cities, 0),
            ProductionItem::Unit(kind) => match role_of(rules, kind) {
                AiRole::Settler => self.prices.new_city(SETTLE_WALK_ROUNDS),
                AiRole::Worker => self.worker(obs, empire, empire.workers.saturating_sub(1), 0),
                _ => 0,
            },
            ProductionItem::Project(_) => 0,
        }
    }

    /// Tiles the cities may buy: land points, and a resource's yields and
    /// happiness. Looked at only when the gold buys a tile.
    fn tile_offers(&self, obs: &AIObservation<'_>, spendable: u32) -> Vec<Offer> {
        let mut out = Vec::new();
        if spendable < crate::world::territory::PLOT_BASE_COST {
            return out;
        }
        for city in &obs.own_cities {
            for (tile, price) in obs.view.own_tile_offers(city.handle) {
                if price > spendable {
                    continue;
                }
                let value = self.tile_value(obs, tile);
                let command = Command::PurchaseTile { player: obs.player, city: city.handle, tile };
                out.push(Offer { actor: Actor::City(city.id), tie: city.id.0, price, value, upkeep: 0, command });
            }
        }
        out
    }

    /// Land points of a land tile, with a third of its resource's improved
    /// yields (the city may work it) and a luxury's happiness.
    fn tile_value(&self, obs: &AIObservation<'_>, tile: TileId) -> Centi {
        let Some(snapshot) = obs.view.tile(tile).snapshot().copied() else { return 0 };
        if obs.view.terrain_kinds().is_water(snapshot.terrain.0) {
            return self.prices.score.land / 4;
        }
        let rules = obs.view.rules();
        let resource = snapshot.resource.and_then(|r| rules.resources.get(r.0 as usize));
        let extra = resource.map_or(0, |def| {
            let improved = def.improved_yields.first().map(|(_, y)| *y).unwrap_or_default();
            let yields = self.prices.food * i64::from(def.yields.food + improved.food)
                + self.prices.production * i64::from(def.yields.production + improved.production)
                + self.prices.gold * i64::from(def.yields.gold + improved.gold);
            yields / 3 + self.prices.happiness * i64::from(luxury_happiness(obs, self.prices.model, snapshot.resource, def.module.happiness)) / 2
        });
        self.prices.score.land + extra
    }
}

/// A settler may be valued: always for the first cities; then, under
/// `PriceModel::Tiles`, while one more city keeps the empire content;
/// under `PriceModel::Uses` whenever the rules let a city be founded (not
/// very unhappy), its unhappiness priced in its value (`Prices::new_city`).
fn may_settle(obs: &AIObservation<'_>, brief: &TurnBrief, state: &AiState, model: PriceModel) -> bool {
    use crate::ai::planner_strategy::{expansion_open, expansion_paused, FIRST_CITIES};
    use crate::derived::happiness::{mood, Mood};
    if obs.own_cities.len() < FIRST_CITIES {
        return true;
    }
    match model {
        PriceModel::Tiles => expansion_open(brief, state),
        PriceModel::Uses => !expansion_paused(brief, state) && mood(obs.economy.turn_happiness) != Mood::VeryUnhappy,
    }
}

/// Happiness a tile's resource adds: a luxury's own (under
/// `PriceModel::Uses` only when the empire has no copy: a second copy of a
/// kind adds none).
fn luxury_happiness(obs: &AIObservation<'_>, model: PriceModel, resource: Option<crate::ids::ResourceId>, happiness: i32) -> i32 {
    let had = resource.is_some_and(|r| obs.economy.resources.available(r) > 0);
    match model {
        PriceModel::Uses if had => 0,
        _ => happiness,
    }
}

/// The trade unit the most productive city with room in its queue could
/// build while the empire may start a route and has no trade unit idle or
/// queued, valued by its best route now: the origin's gold and science
/// and, on a route home, the destination's food and production, each a
/// turn to the end (route yields are hundredths); the unit has no upkeep.
fn trade_prospect(obs: &AIObservation<'_>, rules: &CompiledRuleset, prices: &Prices) -> Option<TradeProspect> {
    use crate::knowledge::public::PlayerQueries;
    let is_trade = |item: &ProductionItem| matches!(item, ProductionItem::Unit(k) if crate::derived::trade::is_trade_unit(rules, *k));
    let queued = obs.own_cities.iter().any(|c| c.queue.iter().any(is_trade));
    if obs.economy.trade_routes_left == 0 || !obs.economy.trade_offers.is_empty() || queued {
        return None;
    }
    let kind = crate::ai::trade::trade_unit(obs, rules)?;
    let city = obs
        .own_cities
        .iter()
        .filter(|c| c.queue.len() < crate::world::city::MAX_QUEUE)
        .max_by_key(|c| (c.income.production, std::cmp::Reverse(c.id)))?;
    let worth = |o: &crate::derived::trade::TradeOffer| {
        let home = o.kind != crate::world::trade::RouteKind::International;
        let mut value = i64::from(o.origin.gold) * prices.gold + i64::from(o.origin.science) * prices.science;
        if home {
            value += i64::from(o.destination_yields.food) * prices.food + i64::from(o.destination_yields.production) * prices.production;
        }
        value / 100
    };
    let value = obs.view.trade_prospects(city.handle, kind).iter().map(worth).max()?;
    (value > 0).then_some(TradeProspect { city: city.id, kind, value })
}

/// Fixed-point scale of value per gold.
const RATIO_SCALE: i64 = 1000;

/// One purchase the gold could make.
struct Offer {
    actor: Actor,
    /// Ties go to the lower: a city's id, a city-state's player.
    tie: u64,
    price: u32,
    value: Centi,
    /// Gold a turn it costs once bought.
    upkeep: i32,
    command: Command,
}

impl Offer {
    fn ratio(&self) -> i64 {
        self.value * RATIO_SCALE / i64::from(self.price.max(1))
    }

    fn tile(&self) -> Option<TileId> {
        match self.command {
            Command::PurchaseTile { tile, .. } => Some(tile),
            _ => None,
        }
    }
}

/// Worked land tiles of the empire without an improvement (city centres
/// aside).
fn open_jobs(obs: &AIObservation<'_>) -> u32 {
    let mut jobs = 0;
    for city in &obs.own_cities {
        for &tile in &city.worked {
            let open = obs.view.tile(tile).snapshot().is_some_and(|s| {
                s.improvement.is_none() && !s.city && !obs.view.terrain_kinds().is_water(s.terrain.0)
            });
            if open {
                jobs += 1;
            }
        }
    }
    jobs
}

/// Worked tiles of the empire where a worker could build now: the workers'
/// own job finder (`planner_workers::open_jobs`) over the tiles `open_jobs`
/// counts, so a worker is wanted only for work the rules let it do.
fn finder_jobs(obs: &AIObservation<'_>, brief: &TurnBrief, state: &AiState, rules: &CompiledRuleset) -> u32 {
    let mut tiles: Vec<TileId> = obs.own_cities.iter().flat_map(|c| c.worked.iter().copied()).collect();
    tiles.sort();
    tiles.dedup();
    crate::ai::planner_workers::open_jobs(&obs.view, rules, brief, state.strategy.current, tiles.into_iter())
}

/// The war chest: what the treasury keeps for the upkeep of the forming
/// operations' armies (`operations::WAR_CHEST_ROUNDS` of their deficit);
/// a war plan's chest comes on top.
fn war_chest(obs: &AIObservation<'_>, state: &AiState) -> u32 {
    let army: u32 = state
        .operations()
        .iter()
        .map(|o| if o.phase == OperationPhase::Forming { o.need.total() } else { 0 })
        .sum();
    if army == 0 {
        return 0;
    }
    let units = obs.own_units.len();
    let extra = unit_upkeep_at(units + army as usize, obs.round, obs.speed.max_rounds()) - unit_upkeep_at(units, obs.round, obs.speed.max_rounds());
    let deficit = (extra - obs.economy.income.gold).max(0) as u32;
    deficit * crate::ai::operations::WAR_CHEST_ROUNDS
}

/// A recorder of the allocator's valuations for diagnosis (the `science_econ`
/// example): off unless enabled on the thread, never read by a decision.
pub mod probe {
    use super::*;
    use std::cell::RefCell;

    /// One candidate order of a city: its value once done and the turns it
    /// takes.
    #[derive(Debug, Clone, Copy)]
    pub struct Candidate {
        pub item: ProductionItem,
        pub value: Centi,
        pub turns: u32,
    }

    /// The allocator's choice for one city.
    #[derive(Debug, Clone)]
    pub struct Choice {
        pub player: crate::ids::PlayerId,
        pub round: u32,
        pub city: crate::ids::EntityId,
        pub population: u32,
        pub production: i32,
        pub prices: Prices,
        pub options: Vec<Candidate>,
        pub picked: Option<ProductionItem>,
    }

    thread_local! {
        static LOG: RefCell<Option<Vec<Choice>>> = const { RefCell::new(None) };
    }

    /// Start (or stop) recording on this thread.
    pub fn enable(on: bool) {
        LOG.with(|log| *log.borrow_mut() = on.then(Vec::new));
    }

    pub fn enabled() -> bool {
        LOG.with(|log| log.borrow().is_some())
    }

    /// The choices recorded since the last call.
    pub fn take() -> Vec<Choice> {
        LOG.with(|log| log.borrow_mut().as_mut().map(std::mem::take).unwrap_or_default())
    }

    pub(super) fn record(obs: &AIObservation<'_>, city: &OwnCity, prices: &Prices, options: Vec<Candidate>, picked: Option<ProductionItem>) {
        let choice = Choice {
            player: obs.player,
            round: obs.round,
            city: city.id,
            population: city.population,
            production: city.income.production,
            prices: *prices,
            options,
            picked,
        };
        LOG.with(|log| {
            if let Some(log) = log.borrow_mut().as_mut() {
                log.push(choice);
            }
        });
    }
}
