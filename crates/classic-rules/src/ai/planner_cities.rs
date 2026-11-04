//! City production by needs (R-07 AI stage E).
//!
//! Every city with an empty queue gets one order, chosen in this order of
//! needs: a garrison for an empty city, a settler while good sites are
//! known and the strategy expands, a worker per city with work to do, the
//! army the strategy wants, the most valuable building per cost. The army
//! is capped by the difficulty's unit supply (a difficulty column) and by
//! income: a unit costs its upkeep (`derived::income::unit_upkeep`), and no
//! new unit is ordered when the gold income would drop below zero (no
//! unbounded army at zero income). A threatened city without a garrison
//! switches to a defender at once. Costs come from the core
//! (`PlayerView::own_production_cost`), never from a copied formula.

use crate::knowledge::public::PlayerQueries;
use crate::ai::planner::{role_of, AiRole, Decision, PlanTools, TurnBrief};
use crate::ai::planner_settle::{count_sites, SiteContext};
use crate::ai::allocator::{Allocator, SETTLE_WALK_ROUNDS};
use crate::ai::planner_value::{building_value, Weights};
use crate::ai::state::{AiState, ArmyRole, Composition, Reservation, Strategy};
use crate::ai::trace::{Actor, Reason};
use crate::ai::value::PriceModel;
use crate::command::Command;
use crate::derived::income::unit_upkeep_at;
use crate::ids::{BuildingKindId, PlayerId, ProductionItem, TileId, UnitKindId};
use crate::knowledge::observation::{AIObservation, OwnCity};
use crate::rules::content::Domain;
use crate::rules::CompiledRuleset;
use crate::world::unit::MOVE_DENOMINATOR;

/// An order taking longer than this many turns is not chosen while
/// something faster is wanted (production turns at Standard speed; × the
/// speed's pace, as its costs).
pub const SLOW_ORDER_TURNS: u32 = 40;
/// Settlers come from cities of at least this population.
pub const SETTLER_MIN_POPULATION: u32 = 2;
/// Gold kept in the treasury before hurrying.
pub const HURRY_RESERVE: u32 = 30;
/// Turns of gold deficit the treasury keeps in reserve before buying.
pub const DEFICIT_RESERVE_TURNS: u32 = 30;

/// The stops of its own the AI replaces by the price or the risk the rules
/// put on the option (`Variant::priced_stops`, classic/docs/ai-own-stops.md).
/// The rules punish only a treasury below zero (units are disbanded) and let a
/// unit that enters rival territory pass closed borders; each field drops the
/// stop the AI kept on top of that.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PricedStops {
    /// New upkeep (a building's maintenance, a unit beyond the cap of a
    /// deficit) while the treasury carries the deficit it causes for
    /// `citizens::GOLD_FOCUS_TURNS` (`citizens::carries`), instead of only
    /// while the income pays it; the allocator prices the upkeep as before.
    pub upkeep: bool,
    /// The gold kept before buying is the deficit for
    /// `DEFICIT_RESERVE_TURNS` (the war chest comes on top), nothing at a
    /// positive income, instead of `HURRY_RESERVE` and 25 a city, twice in war.
    pub reserve: bool,
    /// Missionaries and prophets go to any city they can reach by their own
    /// way of moving (rival territory included) and stop next to it, and one
    /// is bought only while such a city exists (`ai::religion`).
    pub missionaries: bool,
}

impl PricedStops {
    /// The stops of the game's AI.
    pub const NONE: PricedStops = PricedStops { upkeep: false, reserve: false, missionaries: false };
    /// Every stop priced.
    pub const ALL: PricedStops = PricedStops { upkeep: true, reserve: true, missionaries: true };
}

/// What the empire has and wants, updated as orders are given.
#[derive(Clone)]
pub(crate) struct Empire {
    pub(crate) player: PlayerId,
    nation: u8,
    /// Only orders the rules accept (`TurnBrief::sound_orders`).
    sound_orders: bool,
    /// The own stops priced instead (`TurnBrief::priced_stops`).
    stops: PricedStops,
    strategy: Strategy,
    weights: Weights,
    pub(crate) military: usize,
    military_cap: usize,
    /// The difficulty's unit supply: no unit beyond it, not even a defender.
    supply: usize,
    pub(crate) settlers: usize,
    pub(crate) settler_target: usize,
    /// Good city sites known now, at most `allocator::MAX_SETTLERS`.
    pub(crate) sites: usize,
    pub(crate) workers: usize,
    /// Builders of the sea (work boats) living or queued.
    pub(crate) sea_builders: usize,
    pub(crate) worker_target: usize,
    pub(crate) wonders_taken: Vec<BuildingKindId>,
    /// Fewer recon units live or are queued than the unknown part of the
    /// map calls for (`ai::scouting::wants_recon`); for a leader looking
    /// for a capital: none lives (`ai::capital_search`: a scout, not a
    /// warrior, crosses the map).
    wants_explorer: bool,
    /// `TurnBrief::no_idle_cities`: a city the allocator leaves without an order takes
    /// the best process or the best order beyond the limits (`allocated_order`).
    no_idle: bool,
    /// Space orders given this turn (Apollo, spaceship parts).
    space_planned: Vec<ProductionItem>,
    /// The cities' new queues of the space plan (`victory::space_plan`;
    /// empty unless `space_finish`).
    space_queues: Vec<crate::ai::victory::SpaceQueue>,
    /// The space plan orders the space items (`TurnBrief::space_finish`):
    /// a city's own pass orders none.
    space_finish: bool,
    /// The book's settlers come first (`ai::tall`, `Tall::Book`).
    book: bool,
    /// The book's second edition orders a scout before its settlers when the
    /// shared rule of the first recon unit wants one (`tall::BookPlan::scout_first`,
    /// `scouting::recon_worth`: the ruins it can expect are worth its hammers).
    book_scout: bool,
    /// The science buildings a city orders first (`tall::BuildingOrder`).
    building_order: crate::ai::tall::BuildingOrder,
    /// Units the forming war operations still lack (`ai::operations`).
    war_demand: Composition,
    /// The army cap for those units: the whole share of a war.
    war_cap: usize,
    /// Cities that may still switch to a war order this turn.
    war_cities: usize,
    /// Warships the operations against coastal cities still lack
    /// (`ai::fleet`).
    fleet_demand: usize,
    /// Strikers and fighters the war operations and the enemy's air force
    /// still lack (`ai::air`); zero for a variant without an air arm.
    striker_demand: usize,
    fighter_demand: usize,
    /// Carriers the war calls for (`ai::carrier`).
    carrier_demand: usize,
    /// The weapon (and the project it needs first) the war calls for
    /// (`ai::strikes`), until a city has ordered it this turn.
    weapon_plan: Option<crate::ai::strikes::BuildPlan>,
    /// The weapon plan was made this turn.
    weapon_planned: bool,
    /// Items the rules refused for the city being planned: no picker offers
    /// them again (`ai::legal`).
    pub(crate) refused: Vec<ProductionItem>,
    /// The unit kinds are chosen by what they are for (`ai::army_fit`,
    /// `TurnBrief::army_fit`); None: the strongest.
    fit: Option<FitContext>,
    /// Barbarian camps want a clearing group the free units do not make (`ai::camps`).
    pub(crate) camp_demand: usize,
}

/// What `ai::army_fit` rates the kinds against this turn.
#[derive(Clone)]
struct FitContext {
    foes: crate::ai::army_fit::Foes,
    /// The strength (hundredths) of the city a forming operation is sent against.
    city: Option<i64>,
    prices: Option<crate::ai::value::Prices>,
    /// One more unit's upkeep to the end, at the allocator's price of gold.
    upkeep: crate::ai::value::Centi,
}

impl FitContext {
    fn of(obs: &AIObservation<'_>, brief: &TurnBrief, state: &AiState, rules: &CompiledRuleset) -> Self {
        // The operation that forms first, else the war the treasury saves for.
        let forming = state.operations().iter().find(|o| o.phase == crate::ai::state::OperationPhase::Forming).map(|o| (o.target, o.enemy));
        let target = forming.or_else(|| state.war_plan.map(|p| (p.target, p.enemy)));
        let city = target
            .and_then(|(tile, _)| state.seen_cities().iter().find(|c| c.tile == tile))
            .map(|c| crate::ai::operations::estimate_target(brief, rules, c).strength);
        let units = obs.own_units.len();
        let upkeep = brief.prices.as_ref().map_or(0, |p| {
            i64::from(unit_upkeep_at(units + 1, brief.round, brief.speed.max_rounds()) - unit_upkeep_at(units, brief.round, brief.speed.max_rounds())) * p.gold
        });
        Self { foes: crate::ai::army_fit::Foes::seen(obs, rules, target.map(|(_, e)| e)), city, prices: brief.prices, upkeep }
    }
}

/// Orders for every city with an empty queue (and threatened cities).
pub fn plan_cities(
    obs: &AIObservation<'_>,
    brief: &TurnBrief,
    state: &AiState,
    tools: &mut PlanTools<'_>,
) -> Vec<Decision> {
    plan_cities_with(obs, brief, state, tools, None)
}

/// `plan_cities` with the allocator of the game's AI (`allocator`, `standard`)
/// (`ai::allocator`) choosing the valued orders and spending the gold.
pub(crate) fn plan_cities_with(
    obs: &AIObservation<'_>,
    brief: &TurnBrief,
    state: &AiState,
    tools: &mut PlanTools<'_>,
    allocator: Option<&Allocator>,
) -> Vec<Decision> {
    let mut empire = Empire::new(obs, brief, state, tools);
    if let Some(allocator) = allocator {
        allocator.adjust(&mut empire);
    }
    if brief.unciv.settle {
        // Unciv's `trainSettler` decides the settlers (`ai::unciv::parts`).
        empire.settler_target = 0;
    }
    let mut cities: Vec<&OwnCity> = obs.own_cities.iter().collect();
    cities.sort_by_key(|c| (std::cmp::Reverse(c.income.production), c.id));
    let mut out = Vec::new();
    let hurried = match allocator {
        // Unciv's `useGold` spends the gold (`ai::unciv::parts`).
        _ if brief.unciv.gold => Vec::new(),
        Some(allocator) => allocator.spend_gold(obs, brief, &empire, tools.rules),
        None => spend_gold(obs, brief, &empire, tools.rules),
    };
    out.extend(hurried.iter().filter(|d| d.actor == Actor::Player).cloned());
    for city in cities {
        // A city plans again, with the refused item banned for it, when the
        // rules refuse its choice; the empire's totals are those from before.
        let before = empire.clone();
        empire.refused.clear();
        let mut orders = city_orders(obs, brief, state, tools, &mut empire, city, &hurried, allocator);
        let asks_rules = brief.checks_orders;
        for _ in 0..if asks_rules { crate::ai::legal::MAX_REPLANS } else { 0 } {
            let refused = crate::ai::legal::refused_items(&obs.view, &orders);
            if refused.is_empty() {
                break;
            }
            tools.counters.orders_retried += refused.len() as u64;
            let mut banned = std::mem::take(&mut empire.refused);
            banned.extend(refused);
            empire = before.clone();
            empire.refused = banned;
            orders = city_orders(obs, brief, state, tools, &mut empire, city, &hurried, allocator);
        }
        if asks_rules {
            orders = crate::ai::legal::filter(obs, orders, Some(&mut *tools.counters));
        }
        out.extend(orders);
        empire.refused.clear();
    }
    out
}

/// The orders of one city this turn, by need: the first path that has one.
#[allow(clippy::too_many_arguments)]
fn city_orders(
    obs: &AIObservation<'_>,
    brief: &TurnBrief,
    state: &AiState,
    tools: &mut PlanTools<'_>,
    empire: &mut Empire,
    city: &OwnCity,
    hurried: &[Decision],
    allocator: Option<&Allocator>,
) -> Vec<Decision> {
    if let Some(orders) = defend_now(obs, brief, empire, tools.rules, city) {
        return orders;
    }
    if let Some(orders) = defence_orders(obs, brief, empire, tools.rules, city) {
        return orders;
    }
    // The carrier before the warships: the aircraft and weapons it takes to
    // the coast are of no use where no city reaches.
    if let Some(orders) = carrier_orders(obs, empire, tools.rules, city) {
        return orders;
    }
    if let Some(orders) = fleet_orders(obs, empire, tools.rules, city) {
        return orders;
    }
    if let Some(orders) = air_orders(obs, empire, tools.rules, city) {
        return orders;
    }
    if let Some(orders) = weapon_orders(obs, brief, state, empire, tools.rules, city) {
        return orders;
    }
    if let Some(orders) = war_orders(obs, empire, tools.rules, city) {
        return orders;
    }
    if let Some(orders) = camp_orders(obs, empire, tools.rules, city) {
        return orders;
    }
    if let Some(orders) = recon_orders(obs, empire, tools.rules, city) {
        return orders;
    }
    if allocator.is_some() {
        if let Some(orders) = escort_order(obs, brief, state, empire, tools.rules, city) {
            return orders;
        }
    }
    if let Some(allocator) = allocator {
        if let Some(orders) = settler_ahead(obs, empire, tools.rules, city, allocator) {
            return orders;
        }
    }
    // The city's purchases: a hurried queue head (one at most) and the
    // allocator's tiles, which leave the queue as it is.
    let mut out = Vec::new();
    let purchases = hurried.iter().filter(|d| d.actor == Actor::City(city.id));
    let hurried_head = purchases.clone().any(|d| d.reason == Reason::Hurry);
    out.extend(purchases.cloned());
    let mut queued = city.queue.len() - usize::from(hurried_head);
    if !hurried_head {
        if let Some((orders, length)) = space_queue(empire, city) {
            out.extend(orders);
            queued = length;
        }
    }
    out.extend(fill_queue(obs, empire, tools.rules, city, queued, allocator));
    out
}

/// The city's queue as the space plan rewrites it (`victory::space_plan`):
/// the orders (the queue cleared first unless it is empty) and the new
/// queue's length. Items the rules refused for the city are left out. None:
/// the plan leaves the queue as it is.
fn space_queue(empire: &Empire, city: &OwnCity) -> Option<(Vec<Decision>, usize)> {
    let plan = empire.space_queues.iter().find(|q| q.city == city.handle)?;
    let queue: Vec<ProductionItem> = plan.queue.iter().copied().filter(|i| !empire.refused.contains(i)).collect();
    if queue == city.queue {
        return None;
    }
    let mut orders = Vec::new();
    if !city.queue.is_empty() {
        let clear = Command::ClearProduction { player: empire.player, city: city.handle };
        orders.push(Decision::act(Actor::City(city.id), Reason::ProduceProject, clear));
    }
    orders.extend(queue.iter().map(|item| order(empire, city, *item, Reason::ProduceProject)));
    Some((orders, queue.len()))
}

/// Orders until the city has `QUEUE_TARGET` items (`queued` now); an army
/// cap that holds a garrison back is traced.
fn fill_queue(
    obs: &AIObservation<'_>,
    empire: &mut Empire,
    rules: &CompiledRuleset,
    city: &OwnCity,
    mut queued: usize,
    allocator: Option<&Allocator>,
) -> Vec<Decision> {
    let mut out = Vec::new();
    let capped = empire.military >= empire.military_cap;
    if queued == 0 && capped && needs_garrison(obs, empire, rules, city) {
        out.push(Decision::refuse(
            Actor::City(city.id),
            Reason::ArmyCapReached,
        ));
    }
    let mut planned = Vec::new();
    while queued < QUEUE_TARGET {
        let decision = choose_order(obs, empire, rules, city, &planned, allocator);
        let item = ordered_item(&decision);
        out.push(decision);
        let Some(item) = item else {
            break;
        };
        planned.push(item);
        queued += 1;
    }
    out
}

/// Orders a city keeps queued. One: the next order is
/// chosen when the city needs it, from the empire as it is then (a second
/// order planned at size 1 kept the first settler waiting behind a worker
/// and a shrine for 20 rounds; the bench's pace table). The AI refills the
/// queue at the start of every turn, before the city produces.
pub const QUEUE_TARGET: usize = 1;

fn ordered_item(decision: &Decision) -> Option<ProductionItem> {
    match &decision.command {
        Some(Command::EnqueueProduction { item, .. }) => Some(*item),
        _ => None,
    }
}

/// Gold the empire keeps for emergencies (more in war).
fn gold_reserve(obs: &AIObservation<'_>, brief: &TurnBrief) -> u32 {
    emergency_reserve(obs, brief) + gift_reserve(obs) * reserve_factor(brief)
}

/// The reserve without the savings for city-state gifts. With the reserve
/// priced (`PricedStops::reserve`) only the deficit's (`deficit_reserve`):
/// the rules punish a treasury below zero, not a small one, and a war's
/// needs are its chest (`allocator::war_chest`).
pub fn emergency_reserve(obs: &AIObservation<'_>, brief: &TurnBrief) -> u32 {
    let deficit = deficit_reserve(obs);
    if brief.priced_stops.reserve {
        return deficit;
    }
    (HURRY_RESERVE + 25 * obs.own_cities.len() as u32 + deficit) * reserve_factor(brief)
}

/// The gold that pays the empire's deficit for `DEFICIT_RESERVE_TURNS`
/// (none at a positive income).
fn deficit_reserve(obs: &AIObservation<'_>) -> u32 {
    (-obs.economy.income.gold).max(0) as u32 * DEFICIT_RESERVE_TURNS
}

/// Twice the reserve in war.
fn reserve_factor(brief: &TurnBrief) -> u32 {
    if brief.relations.iter().any(|r| r.at_war) {
        2
    } else {
        1
    }
}

/// Met city-states: save for a gold gift (ai::city_states gifts above
/// GIFT_RESERVE + SMALL_GIFT, so the AI saves for city-state gifts).
fn gift_reserve(obs: &AIObservation<'_>) -> u32 {
    if obs.city_states.is_empty() {
        0
    } else {
        crate::ai::city_states::GIFT_RESERVE + crate::ai::city_states::SMALL_GIFT
    }
}

/// What buying the head of a queue is worth, lower first: settlers while
/// expanding, then workers, military in war, buildings.
fn hurry_rank(empire: &Empire, rules: &CompiledRuleset, item: ProductionItem) -> u8 {
    match item {
        ProductionItem::Unit(kind) => match role_of(rules, kind) {
            AiRole::Settler if empire.strategy == Strategy::Expand => 0,
            AiRole::Worker => 1,
            AiRole::Military if matches!(empire.strategy, Strategy::Defend | Strategy::Attack) => 2,
            _ => 4,
        },
        ProductionItem::Building(kind) => {
            let wonder = rules
                .buildings
                .get(kind.0 as usize)
                .is_some_and(|d| d.wonder);
            if wonder {
                4
            } else {
                3
            }
        }
        ProductionItem::Project(_) => 4,
    }
}

/// (rank, price, city) of every queue head worth buying, best first.
fn hurry_offers<'o>(
    obs: &'o AIObservation<'_>,
    empire: &Empire,
    rules: &CompiledRuleset,
) -> Vec<(u8, u32, &'o OwnCity)> {
    let mut offers = Vec::new();
    for city in &obs.own_cities {
        let Some(head) = city.queue.first() else {
            continue;
        };
        let rank = hurry_rank(empire, rules, *head);
        if rank > 3 {
            continue;
        }
        if let Some(price) = obs.view.own_hurry_price(city.handle) {
            offers.push((rank, price, city));
        }
    }
    offers.sort_by_key(|(rank, price, city)| (*rank, *price, city.id));
    offers
}

/// Gold a turn the item costs once built: a building's maintenance, about
/// one for a unit that is not free (the ruleset's unit maintenance).
pub(crate) fn item_upkeep(rules: &CompiledRuleset, item: ProductionItem) -> i32 {
    match item {
        ProductionItem::Unit(kind) => {
            i32::from(rules.units.get(kind.0 as usize).is_some_and(|u| !u.module.economy.no_maintenance))
        }
        ProductionItem::Building(kind) => rules.buildings.get(kind.0 as usize).map_or(0, |b| b.maintenance as i32),
        ProductionItem::Project(_) => 0,
    }
}

/// Buy queue heads with the gold above the reserve (the AI's purchases):
/// at most one city in three per turn, best rank then cheapest first.
fn spend_gold(
    obs: &AIObservation<'_>,
    brief: &TurnBrief,
    empire: &Empire,
    rules: &CompiledRuleset,
) -> Vec<Decision> {
    let reserve = gold_reserve(obs, brief);
    let mut spendable = obs.economy.treasury.saturating_sub(reserve);
    let offers = hurry_offers(obs, empire, rules);
    let limit = obs.own_cities.len().div_ceil(3);
    let mut out = Vec::new();
    // Nothing bought may push the income below zero (M4-A):
    // a bought unit or building pays upkeep from the next turn.
    let mut income = obs.economy.income.gold;
    for (_, price, city) in offers {
        let upkeep = city.queue.first().map_or(0, |item| item_upkeep(rules, *item));
        if out.len() >= limit || price > spendable || income - upkeep < 0 {
            continue;
        }
        let command = Command::HurryProduction {
            player: empire.player,
            city: city.handle,
        };
        if !obs.view.accepts(&command) {
            continue;
        }
        income -= upkeep;
        spendable -= price;
        out.push(Decision::act(Actor::City(city.id), Reason::Hurry, command));
    }
    out
}

impl Empire {
    fn new(
        obs: &AIObservation<'_>,
        brief: &TurnBrief,
        state: &AiState,
        tools: &mut PlanTools<'_>,
    ) -> Self {
        let rules = tools.rules;
        let count = |role: AiRole| {
            let units = obs
                .own_units
                .iter()
                .filter(|u| role_of(rules, u.kind) == role && !crate::ai::sea_work::is_sea_builder(rules, u.kind))
                .count();
            units + queued(obs, rules, role)
        };
        let boats = obs.own_units.iter().filter(|u| crate::ai::sea_work::is_sea_builder(rules, u.kind)).count() + queued_boats(obs, rules);
        let military = count(AiRole::Military);
        let cities = obs.own_cities.len();
        let wants_explorer = match brief.search {
            None if brief.scout_early => crate::ai::scouting::recon_worth(obs, brief, rules, recon_on_hand(obs, rules)),
            None => crate::ai::scouting::wants_recon(brief.speed.standard_round(brief.round), brief.known_percent, recon_on_hand(obs, rules)),
            Some(_) => recon_on_hand(obs, rules) == 0,
        };
        let sites = known_sites(obs, state, tools);
        let expanding = matches!(state.strategy.current, Strategy::Expand) || cities < 2;
        // A very unhappy empire may found no city (the core's rule): a
        // settler then only waits and costs its upkeep.
        let may_found = crate::derived::happiness::mood(obs.economy.turn_happiness)
            != crate::derived::happiness::Mood::VeryUnhappy;
        let settler_target = if expanding && sites > 0 && (may_found || cities < 2) {
            sites.min(2)
        } else {
            0
        };
        Self {
            player: obs.player,
            nation: obs.economy.nation,
            sound_orders: brief.sound_orders,
            stops: brief.priced_stops,
            strategy: state.strategy.current,
            weights: Weights::for_empire(state.strategy.current, obs.economy.income.gold, obs.economy.turn_happiness),
            military,
            military_cap: military_cap(obs, brief, state.strategy.current, tools, military),
            supply: tools
                .settings
                .supply
                .supplied(obs.own_cities.len() as u32, brief.total_population)
                as usize,
            settlers: count(AiRole::Settler),
            settler_target,
            sites,
            workers: count(AiRole::Worker),
            sea_builders: boats,
            worker_target: worker_target(obs, state, rules),
            wonders_taken: wonders_taken(obs, rules),
            wants_explorer,
            no_idle: brief.no_idle_cities,
            space_planned: Vec::new(),
            space_queues: if brief.space_finish { crate::ai::victory::space_plan(obs, rules) } else { Vec::new() },
            space_finish: brief.space_finish,
            book: brief.tall.book_cities(),
            book_scout: brief.tall.plan().scout_first && crate::ai::scouting::recon_worth(obs, brief, rules, recon_on_hand(obs, rules)),
            building_order: brief.building_order,
            war_demand: crate::ai::operations::demand(obs, state, rules),
            war_cap: war_cap(obs, brief, tools, military),
            war_cities: cities.div_ceil(2),
            fleet_demand: crate::ai::fleet::shortfall(obs, state, rules),
            striker_demand: if brief.air { crate::ai::air::shortfall(obs, state, rules, crate::ai::air::Wing::Striker) } else { 0 },
            fighter_demand: if brief.air { crate::ai::air::shortfall(obs, state, rules, crate::ai::air::Wing::Fighter) } else { 0 },
            carrier_demand: if brief.carriers { crate::ai::carrier::wanted(obs, brief, state, rules) } else { 0 },
            weapon_plan: None,
            weapon_planned: false,
            refused: Vec::new(),
            fit: brief.army_fit.then(|| FitContext::of(obs, brief, state, rules)),
            camp_demand: if brief.camps { crate::ai::camps::demand(obs, brief, state, rules) } else { 0 },
        }
    }
}

/// One worker per city, but none more while a worker has no job (an idle
/// worker only costs its upkeep: maintenance ran AI treasuries dry).
fn worker_target(obs: &AIObservation<'_>, state: &AiState, rules: &CompiledRuleset) -> usize {
    let idle = obs.own_units.iter().any(|u| {
        role_of(rules, u.kind) == AiRole::Worker
            && !crate::ai::sea_work::is_sea_builder(rules, u.kind)
            && !matches!(state.task_of(u.id).map(|t| t.task), Some(crate::ai::state::UnitTask::Improve { .. }))
    });
    if idle {
        0
    } else {
        obs.own_cities.len().max(1)
    }
}

/// Recon units alive or queued.
fn recon_on_hand(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> usize {
    let alive = obs.own_units.iter().filter(|u| is_recon(rules, u.kind)).count();
    let queued = obs
        .own_cities
        .iter()
        .flat_map(|c| &c.queue)
        .filter(|item| matches!(item, ProductionItem::Unit(kind) if is_recon(rules, *kind)))
        .count();
    alive + queued
}

fn is_recon(rules: &CompiledRuleset, kind: UnitKindId) -> bool {
    rules.units.get(kind.0 as usize).and_then(|d| d.role)
        == Some(crate::rules::content::UnitRole::Recon)
}

/// Units of a role already in production queues.
fn queued(obs: &AIObservation<'_>, rules: &CompiledRuleset, role: AiRole) -> usize {
    let mut count = 0;
    for city in &obs.own_cities {
        for item in &city.queue {
            if let ProductionItem::Unit(kind) = item {
                if role_of(rules, *kind) == role && !crate::ai::sea_work::is_sea_builder(rules, *kind) {
                    count += 1;
                }
            }
        }
    }
    count
}

/// The army the empire may keep: the difficulty's unit supply share of the
/// strategy, and no new unit when its upkeep would turn income negative
/// (with the upkeep priced, `PricedStops::upkeep`: a deficit the treasury
/// cannot carry, `citizens::carries`).
fn military_cap(
    obs: &AIObservation<'_>,
    brief: &TurnBrief,
    strategy: Strategy,
    tools: &PlanTools<'_>,
    military: usize,
) -> usize {
    let cities = obs.own_cities.len() as u32;
    let supplied = tools
        .settings
        .supply
        .supplied(cities, brief.total_population) as usize;
    let share = match strategy {
        Strategy::Attack | Strategy::Defend => supplied,
        Strategy::Recover => supplied * 3 / 4,
        Strategy::Expand | Strategy::Stabilize => (supplied / 2).max(cities as usize + 1),
    };
    let units = obs.own_units.len();
    let upkeep_of_one = unit_upkeep_at(units + 1, obs.round, obs.speed.max_rounds()) - unit_upkeep_at(units, obs.round, obs.speed.max_rounds());
    let after = obs.economy.income.gold - upkeep_of_one;
    let unpaid = if brief.priced_stops.upkeep { !crate::ai::citizens::carries(after, obs.economy.treasury) } else { after < 0 };
    if unpaid {
        // Even at a deficit every city keeps one defender (barbarians take
        // undefended cities and settlers cannot leave).
        return military.min(share).max((cities as usize).min(share));
    }
    share
}

/// The army a forming operation may grow to: the whole supply share of a
/// war, while the treasury pays the deficit its upkeep causes for the
/// war (`operations::war_chest_covers`).
fn war_cap(obs: &AIObservation<'_>, brief: &TurnBrief, tools: &PlanTools<'_>, military: usize) -> usize {
    let share = military_cap(obs, brief, Strategy::Attack, tools, military).max(
        tools.settings.supply.supplied(obs.own_cities.len() as u32, brief.total_population) as usize,
    );
    let units = obs.own_units.len();
    let mut cap = military;
    while cap < share {
        let extra = unit_upkeep_at(units + cap - military + 1, obs.round, obs.speed.max_rounds()) - unit_upkeep_at(units, obs.round, obs.speed.max_rounds());
        if !crate::ai::operations::war_chest_covers(obs.economy.income.gold, obs.economy.treasury, extra) {
            break;
        }
        cap += 1;
    }
    cap
}

/// Good city sites known around the most productive city (0 when none).
fn known_sites(obs: &AIObservation<'_>, state: &AiState, tools: &mut PlanTools<'_>) -> usize {
    let Some(origin) = obs
        .own_cities
        .iter()
        .max_by_key(|c| (c.income.production, std::cmp::Reverse(c.id)))
    else {
        return 0;
    };
    let own: Vec<TileId> = obs.own_cities.iter().map(|c| c.tile).collect();
    let foreign: Vec<TileId> = state.seen_cities().iter().map(|c| c.tile).collect();
    let pick = |r: Reservation| match r {
        Reservation::Site(t) => Some(t),
        _ => None,
    };
    let reserved = state.reserved_tiles(pick, None);
    let danger: Vec<TileId> = obs
        .visible_foreign_units
        .iter()
        .filter(|u| obs.relation(u.owner).is_some_and(|r| r.at_war))
        .filter(|u| role_of(tools.rules, u.kind) == AiRole::Military)
        .map(|u| u.tile)
        .collect();
    let settler = crate::scenario::settler_kind(tools.rules);
    let ctx = SiteContext {
        view: &obs.view,
        profile: crate::ai::planner_map::KnownMap::of_kind(&obs.view, settler).profile,
        rules: tools.rules,
        player: obs.player,
        own_cities: &own,
        foreign_cities: &foreign,
        reserved: &reserved,
        danger: &danger,
        tile_memo: Default::default(),
    };
    let moves = settler_moves(tools.rules);
    let limit = tools.settings.budget.candidates;
    // Every reader of the count asks for at most `MAX_SETTLERS`.
    count_sites(&ctx, origin.tile, moves, tools, limit, crate::ai::allocator::MAX_SETTLERS)
}

fn settler_moves(rules: &CompiledRuleset) -> u16 {
    let movement = rules
        .units
        .iter()
        .find(|u| u.founder)
        .map(|u| u.movement)
        .unwrap_or(2);
    movement.max(1) * MOVE_DENOMINATOR
}

/// Wonders built anywhere or already in one of our queues.
fn wonders_taken(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Vec<BuildingKindId> {
    let mut taken = obs.built_wonders.clone();
    for city in &obs.own_cities {
        for item in &city.queue {
            if let ProductionItem::Building(b) = item {
                if rules.buildings.get(b.0 as usize).is_some_and(|d| d.wonder) {
                    taken.push(*b);
                }
            }
        }
    }
    taken
}

fn knows(obs: &AIObservation<'_>, tech: Option<crate::ids::TechId>) -> bool {
    match tech {
        Some(tech) => obs.economy.techs.contains(&tech),
        None => true,
    }
}

/// Can the player train this unit now (technology, not obsolete, own nation,
/// land, its strategic resources available)?
fn unit_available(
    obs: &AIObservation<'_>,
    empire: &Empire,
    rules: &CompiledRuleset,
    kind: UnitKindId,
) -> bool {
    unit_available_in(obs, empire, rules, kind, Domain::Land)
}

/// `unit_available` for units of another domain (the fleet's ships).
fn unit_available_in(
    obs: &AIObservation<'_>,
    empire: &Empire,
    rules: &CompiledRuleset,
    kind: UnitKindId,
    domain: Domain,
) -> bool {
    let Some(def) = rules.units.get(kind.0 as usize) else {
        return false;
    };
    let gate = rules
        .production_gate(ProductionItem::Unit(kind), empire.nation)
        .is_none();
    let obsolete = def
        .obsolete_tech
        .is_some_and(|t| obs.economy.techs.contains(&t));
    let resources = def
        .required_resources
        .iter()
        .all(|(r, n)| obs.economy.resources.available(*r) >= i32::from(*n));
    let replaced = has_own_replacement(rules, empire.nation, kind);
    let banned = empire.refused.contains(&ProductionItem::Unit(kind));
    // A unit of a policy (Landsknecht: Mercenary Army) is refused without it.
    let policy = !empire.sound_orders || def.module.required_policy.is_none_or(|p| obs.culture.policies.contains(&p));
    gate && policy && !banned && !def.unbuildable
        && def.cost > 0
        && def.domain == domain
        && knows(obs, def.required_tech)
        && !obsolete
        && resources
        && def.requires_buildings.is_empty()
        && !replaced
}

/// The nation has a unique unit replacing this regular one.
fn has_own_replacement(rules: &CompiledRuleset, nation: u8, kind: UnitKindId) -> bool {
    rules
        .units
        .iter()
        .any(|u| u.nation == Some(nation) && u.replaces == Some(kind))
}

/// The work boat the player may build now (None without one).
pub(crate) fn sea_builder(obs: &AIObservation<'_>, empire: &Empire, rules: &CompiledRuleset) -> Option<UnitKindId> {
    (0..rules.units.len())
        .map(|i| UnitKindId(i as u16))
        .filter(|k| crate::ai::sea_work::is_sea_builder(rules, *k) && unit_available_in(obs, empire, rules, *k, Domain::Sea))
        .min_by_key(|k| (rules.units[k.0 as usize].cost, k.0))
}

/// Work boats waiting in the cities' queues.
fn queued_boats(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> usize {
    obs.own_cities
        .iter()
        .flat_map(|c| &c.queue)
        .filter(|item| matches!(item, ProductionItem::Unit(k) if crate::ai::sea_work::is_sea_builder(rules, *k)))
        .count()
}

/// The first available unit of a role, the strongest for military.
pub(crate) fn unit_of_role(
    obs: &AIObservation<'_>,
    empire: &Empire,
    rules: &CompiledRuleset,
    role: AiRole,
) -> Option<UnitKindId> {
    let mut best: Option<(u16, u32, UnitKindId)> = None;
    for index in 0..rules.units.len() {
        let kind = UnitKindId(index as u16);
        if role_of(rules, kind) != role || !unit_available(obs, empire, rules, kind) {
            continue;
        }
        let def = &rules.units[index];
        let strength = if role == AiRole::Military {
            def.strength
        } else {
            0
        };
        let better = match best {
            None => true,
            Some((s, c, _)) => strength > s || (strength == s && def.cost < c),
        };
        if better {
            best = Some((strength, def.cost, kind));
        }
    }
    best.map(|(_, _, kind)| kind)
}

/// The land military kind to order for `purpose`: with `TurnBrief::army_fit` the one that
/// fits it (`ai::army_fit::pick`), else the strongest (`unit_of_role`).
fn military_kind(obs: &AIObservation<'_>, empire: &Empire, rules: &CompiledRuleset, purpose: crate::ai::army_fit::Purpose) -> Option<UnitKindId> {
    let strongest = || unit_of_role(obs, empire, rules, AiRole::Military);
    let Some(fit) = &empire.fit else { return strongest() };
    let candidates: Vec<UnitKindId> = (0..rules.units.len())
        .map(|i| UnitKindId(i as u16))
        .filter(|k| role_of(rules, *k) == AiRole::Military && !is_recon(rules, *k) && unit_available(obs, empire, rules, *k))
        .collect();
    crate::ai::army_fit::pick(rules, &candidates, purpose, &fit.foes, fit.city, fit.prices.as_ref(), fit.upkeep).or_else(strongest)
}

/// The purpose of a unit of the Defend strategy's army (`army_order`) with `army_fit`: a
/// ranged unit while they are fewer than half the empire's melee units (the field fights
/// of the training grounds are won by armies about half ranged, classic/docs/ai-army.md),
/// else a defender.
fn field_purpose(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> crate::ai::army_fit::Purpose {
    use crate::ai::army_fit::Purpose;
    let (mut ranged, mut melee) = (0usize, 0usize);
    let kinds = obs.own_units.iter().map(|u| u.kind).chain(obs.own_cities.iter().flat_map(|c| &c.queue).filter_map(|i| match i {
        ProductionItem::Unit(k) => Some(*k),
        _ => None,
    }));
    for kind in kinds {
        match crate::ai::operations::army_role(rules, kind) {
            Some(ArmyRole::Ranged) => ranged += 1,
            Some(ArmyRole::Melee) => melee += 1,
            _ => {}
        }
    }
    if ranged * 2 < melee { Purpose::Strike } else { Purpose::Hold }
}

pub(crate) fn order(empire: &Empire, city: &OwnCity, item: ProductionItem, reason: Reason) -> Decision {
    let command = Command::EnqueueProduction {
        player: empire.player,
        city: city.handle,
        item,
    };
    Decision::act(Actor::City(city.id), reason, command)
}

/// Turns the city needs for an item (u32::MAX when it produces nothing):
/// the city's production with the item's own percents (a Forge for land
/// units, Aristocracy for wonders). A food unit (settler) also gets the
/// city's food surplus as production while it is built (systems::growth,
/// the food surplus counted as production).
pub(crate) fn turns(obs: &AIObservation<'_>, rules: &CompiledRuleset, city: &OwnCity, item: ProductionItem) -> u32 {
    turns_from(obs, rules, city, item, city.production)
}

/// `turns` with `stock` of the city's production put into the item (0: the
/// item waits behind another one, which takes the stock).
pub(crate) fn turns_from(obs: &AIObservation<'_>, rules: &CompiledRuleset, city: &OwnCity, item: ProductionItem, stock: u32) -> u32 {
    let Some(cost) = obs.view.own_production_cost(city.handle, item) else {
        return u32::MAX;
    };
    let stock = stock.min(cost);
    let food_unit = match item {
        ProductionItem::Unit(kind) => rules.units.get(kind.0 as usize).is_some_and(|u| u.module.economy.food),
        ProductionItem::Building(_) | ProductionItem::Project(_) => false,
    };
    let percent = obs.view.own_item_production_percent(city.handle, item);
    let mut per_turn = i64::from(city.income.production_rate.toward(percent));
    if food_unit {
        let surplus = i64::from(city.income.food_surplus_times100);
        per_turn += crate::systems::growth::food_production_times100(surplus) / 100;
    }
    match per_turn.max(0) as u32 {
        0 => u32::MAX,
        per_turn => (cost - stock).div_ceil(per_turn),
    }
}

/// A unit a forming operation lacks, at the head of this city's queue
/// (classic/docs/ai-operations.md "Production for an operation"): the city is one of
/// the half of the empire's cities asked, most productive first; its queue
/// holds no military unit and does not start with a settler, a worker or a
/// world wonder; it builds the unit within `WAR_BUILD_TURNS`; the army stays
/// within the war's cap. Siege first, then ranged, then melee. The queue is
/// cleared, the unit goes first and the old head after it.
fn war_orders(
    obs: &AIObservation<'_>,
    empire: &mut Empire,
    rules: &CompiledRuleset,
    city: &OwnCity,
) -> Option<Vec<Decision>> {
    if empire.war_demand.total() == 0 || empire.war_cities == 0 || empire.military >= empire.war_cap {
        return None;
    }
    let military = |item: &ProductionItem| matches!(item, ProductionItem::Unit(k) if role_of(rules, *k) == AiRole::Military);
    let kept_head = match city.queue.first() {
        Some(ProductionItem::Unit(kind)) => matches!(role_of(rules, *kind), AiRole::Settler | AiRole::Worker),
        Some(ProductionItem::Building(kind)) => rules.buildings.get(kind.0 as usize).is_some_and(|b| b.wonder),
        Some(ProductionItem::Project(_)) => true,
        None => false,
    };
    if kept_head || city.queue.iter().any(military) {
        return None;
    }
    let (role, kind) = war_unit(obs, empire, rules)?;
    let item = ProductionItem::Unit(kind);
    if turns(obs, rules, city, item) > obs.speed.pace_rounds(crate::ai::operations::WAR_BUILD_TURNS) {
        return None;
    }
    take_demand(&mut empire.war_demand, role);
    empire.military += 1;
    empire.war_cities -= 1;
    let mut orders = Vec::new();
    if !city.queue.is_empty() {
        let clear = Command::ClearProduction { player: empire.player, city: city.handle };
        orders.push(Decision::act(Actor::City(city.id), Reason::ProduceArmy, clear));
    }
    orders.push(order(empire, city, item, Reason::ProduceArmy));
    if let Some(head) = city.queue.first() {
        orders.push(order(empire, city, *head, Reason::ProduceArmy));
    }
    Some(orders)
}

/// A warship the operations against coastal cities lack, at the head of
/// this city's queue (`ai::fleet`): the same rules as `war_orders` — one of
/// the half of the cities asked, a queue without a military unit (a worker
/// goes after the ship, a settler and a world wonder do not), built
/// within `WAR_BUILD_TURNS`; the two ships are outside the army cap (they are not
/// counted in it). A city that cannot
/// build ships (no coast) is not asked.
fn fleet_orders(obs: &AIObservation<'_>, empire: &mut Empire, rules: &CompiledRuleset, city: &OwnCity) -> Option<Vec<Decision>> {
    if empire.fleet_demand == 0 || empire.war_cities == 0 || !crate::ai::fleet::coastal(&obs.view, city.tile) {
        return None;
    }
    let military = |item: &ProductionItem| matches!(item, ProductionItem::Unit(k) if matches!(role_of(rules, *k), AiRole::Military | AiRole::Naval));
    let kept_head = match city.queue.first() {
        Some(ProductionItem::Unit(kind)) => role_of(rules, *kind) == AiRole::Settler,
        Some(ProductionItem::Building(kind)) => rules.buildings.get(kind.0 as usize).is_some_and(|b| b.wonder),
        Some(ProductionItem::Project(_)) => true,
        None => false,
    };
    if kept_head || city.queue.iter().any(military) {
        return None;
    }
    let kind = crate::ai::fleet::best_warship(rules, |kind| unit_available_in(obs, empire, rules, kind, Domain::Sea))?;
    let item = ProductionItem::Unit(kind);
    // A ship the city cannot finish in time is bought when the treasury
    // allows, else not ordered.
    let slow = turns(obs, rules, city, item) > obs.speed.pace_rounds(crate::ai::operations::WAR_BUILD_TURNS);
    let purchase = if slow { hurry_defender(obs, empire, city, item) } else { None };
    if slow && purchase.is_none() {
        return None;
    }
    empire.fleet_demand -= 1;
    empire.war_cities -= 1;
    let mut orders = Vec::new();
    if !city.queue.is_empty() {
        let clear = Command::ClearProduction { player: empire.player, city: city.handle };
        orders.push(Decision::act(Actor::City(city.id), Reason::ProduceArmy, clear));
    }
    orders.push(order(empire, city, item, Reason::ProduceArmy));
    if let Some(head) = city.queue.first() {
        orders.push(order(empire, city, *head, Reason::ProduceArmy));
    }
    orders.extend(purchase);
    Some(orders)
}

/// The carrier the war calls for (`ai::carrier`), at the head of a coastal
/// city's queue: the rules of `fleet_orders`.
fn carrier_orders(obs: &AIObservation<'_>, empire: &mut Empire, rules: &CompiledRuleset, city: &OwnCity) -> Option<Vec<Decision>> {
    if empire.carrier_demand == 0 || empire.war_cities == 0 || !crate::ai::fleet::coastal(&obs.view, city.tile) {
        return None;
    }
    let military = |item: &ProductionItem| matches!(item, ProductionItem::Unit(k) if matches!(role_of(rules, *k), AiRole::Military | AiRole::Naval));
    let kept_head = match city.queue.first() {
        Some(ProductionItem::Unit(kind)) => role_of(rules, *kind) == AiRole::Settler,
        Some(ProductionItem::Building(kind)) => rules.buildings.get(kind.0 as usize).is_some_and(|b| b.wonder),
        Some(ProductionItem::Project(_)) => true,
        None => false,
    };
    if kept_head || city.queue.iter().any(military) {
        return None;
    }
    let kind = crate::ai::carrier::best_carrier(rules, |kind| unit_available_in(obs, empire, rules, kind, Domain::Sea))?;
    let item = ProductionItem::Unit(kind);
    // A ship the city cannot finish in `2 * WAR_BUILD_TURNS` is bought when the
    // treasury allows, else not ordered (`fleet_orders`).
    let slow = turns(obs, rules, city, item) > obs.speed.pace_rounds(2 * crate::ai::operations::WAR_BUILD_TURNS);
    let purchase = if slow { hurry_defender(obs, empire, city, item) } else { None };
    if (slow && purchase.is_none()) || empire.refused.contains(&item) {
        return None;
    }
    empire.carrier_demand -= 1;
    empire.war_cities -= 1;
    let mut orders = Vec::new();
    if !city.queue.is_empty() {
        let clear = Command::ClearProduction { player: empire.player, city: city.handle };
        orders.push(Decision::act(Actor::City(city.id), Reason::ProduceArmy, clear));
    }
    orders.push(order(empire, city, item, Reason::ProduceArmy));
    if let Some(head) = city.queue.first() {
        orders.push(order(empire, city, *head, Reason::ProduceArmy));
    }
    orders.extend(purchase);
    Some(orders)
}

/// An aircraft the war operations lack (strikers) or the enemy's air force
/// calls for (fighters, `ai::air`), at the head of this city's queue: the
/// rules of `fleet_orders` (one of the half of the cities asked, no
/// military unit queued, bought when too slow and the treasury allows),
/// and only while the treasury pays its upkeep. Aircraft are outside the
/// army cap.
fn air_orders(obs: &AIObservation<'_>, empire: &mut Empire, rules: &CompiledRuleset, city: &OwnCity) -> Option<Vec<Decision>> {
    use crate::ai::air::{best_of_wing, Wing};
    if (empire.striker_demand == 0 && empire.fighter_demand == 0) || empire.war_cities == 0 {
        return None;
    }
    let military = |item: &ProductionItem| matches!(item, ProductionItem::Unit(k) if matches!(role_of(rules, *k), AiRole::Military | AiRole::Naval));
    let kept_head = match city.queue.first() {
        Some(ProductionItem::Unit(kind)) => role_of(rules, *kind) == AiRole::Settler,
        Some(ProductionItem::Building(kind)) => rules.buildings.get(kind.0 as usize).is_some_and(|b| b.wonder),
        Some(ProductionItem::Project(_)) => true,
        None => false,
    };
    if kept_head || city.queue.iter().any(military) {
        return None;
    }
    let units = obs.own_units.len();
    let extra = unit_upkeep_at(units + 1, obs.round, obs.speed.max_rounds()) - unit_upkeep_at(units, obs.round, obs.speed.max_rounds());
    if !crate::ai::operations::war_chest_covers(obs.economy.income.gold, obs.economy.treasury, extra) {
        return None;
    }
    let wing = if empire.striker_demand > 0 { Wing::Striker } else { Wing::Fighter };
    let kind = best_of_wing(rules, wing, |kind| unit_available_in(obs, empire, rules, kind, Domain::Air))?;
    let item = ProductionItem::Unit(kind);
    let slow = turns(obs, rules, city, item) > obs.speed.pace_rounds(crate::ai::operations::WAR_BUILD_TURNS);
    let purchase = if slow { hurry_defender(obs, empire, city, item) } else { None };
    if slow && purchase.is_none() {
        return None;
    }
    match wing {
        Wing::Striker => empire.striker_demand -= 1,
        Wing::Fighter => empire.fighter_demand -= 1,
    }
    empire.war_cities -= 1;
    let mut orders = Vec::new();
    if !city.queue.is_empty() {
        let clear = Command::ClearProduction { player: empire.player, city: city.handle };
        orders.push(Decision::act(Actor::City(city.id), Reason::ProduceArmy, clear));
    }
    orders.push(order(empire, city, item, Reason::ProduceArmy));
    if let Some(head) = city.queue.first() {
        orders.push(order(empire, city, *head, Reason::ProduceArmy));
    }
    orders.extend(purchase);
    Some(orders)
}

/// The one-shot weapon (or the project it needs) the war calls for, at the
/// head of this city's queue: the rules of `air_orders`, for the city that
/// builds it soonest; chosen once a turn (`strikes::build_plan`).
fn weapon_orders(obs: &AIObservation<'_>, brief: &TurnBrief, state: &AiState, empire: &mut Empire, rules: &CompiledRuleset, city: &OwnCity) -> Option<Vec<Decision>> {
    if !brief.strikes {
        return None;
    }
    if empire.weapon_plan.is_none() && !empire.weapon_planned {
        empire.weapon_planned = true;
        empire.weapon_plan = crate::ai::strikes::build_plan(obs, brief, state, rules, |kind| unit_available_in(obs, empire, rules, kind, Domain::Air));
    }
    let plan = empire.weapon_plan?;
    let kept_head = match city.queue.first() {
        Some(ProductionItem::Unit(kind)) => role_of(rules, *kind) == AiRole::Settler,
        Some(ProductionItem::Building(kind)) => rules.buildings.get(kind.0 as usize).is_some_and(|b| b.wonder),
        Some(ProductionItem::Project(_)) => true,
        None => false,
    };
    let military = |item: &ProductionItem| matches!(item, ProductionItem::Unit(k) if matches!(role_of(rules, *k), AiRole::Military | AiRole::Naval));
    if kept_head || city.queue.iter().any(military) || empire.refused.contains(&plan.item()) {
        return None;
    }
    let item = plan.item();
    // Ready within the life of the prospect it is built for, or bought.
    let slow = turns(obs, rules, city, item) > plan.life;
    let purchase = if slow && matches!(item, ProductionItem::Unit(_)) { hurry_defender(obs, empire, city, item) } else { None };
    if slow && purchase.is_none() {
        return None;
    }
    empire.weapon_plan = None;
    let mut orders = Vec::new();
    if !city.queue.is_empty() {
        let clear = Command::ClearProduction { player: empire.player, city: city.handle };
        orders.push(Decision::act(Actor::City(city.id), Reason::ProduceArmy, clear));
    }
    orders.push(order(empire, city, item, Reason::ProduceArmy));
    if let Some(head) = city.queue.first() {
        orders.push(order(empire, city, *head, Reason::ProduceArmy));
    }
    orders.extend(purchase);
    Some(orders)
}

/// The unit a camp's clearing group lacks (`ai::camps::demand`), at the head of the most
/// productive city's queue that holds no military unit and does not start with a settler, a
/// worker or a world wonder, built within `WAR_BUILD_TURNS`: the kind that holds against the
/// foes seen (`army_fit::Purpose::Hold`, the barbarians among them), within the army cap.
fn camp_orders(obs: &AIObservation<'_>, empire: &mut Empire, rules: &CompiledRuleset, city: &OwnCity) -> Option<Vec<Decision>> {
    if empire.camp_demand == 0 || empire.military >= empire.military_cap || empire.military >= empire.supply {
        return None;
    }
    let military = |item: &ProductionItem| matches!(item, ProductionItem::Unit(k) if role_of(rules, *k) == AiRole::Military);
    let kept_head = match city.queue.first() {
        Some(ProductionItem::Unit(kind)) => matches!(role_of(rules, *kind), AiRole::Settler | AiRole::Worker),
        Some(ProductionItem::Building(kind)) => rules.buildings.get(kind.0 as usize).is_some_and(|b| b.wonder),
        Some(ProductionItem::Project(_)) => true,
        None => false,
    };
    if kept_head || city.queue.iter().any(military) {
        return None;
    }
    let kind = military_kind(obs, empire, rules, crate::ai::army_fit::Purpose::Hold)?;
    let item = ProductionItem::Unit(kind);
    if turns(obs, rules, city, item) > obs.speed.pace_rounds(crate::ai::operations::WAR_BUILD_TURNS) {
        return None;
    }
    empire.camp_demand -= 1;
    empire.military += 1;
    let mut orders = Vec::new();
    if !city.queue.is_empty() {
        let clear = Command::ClearProduction { player: empire.player, city: city.handle };
        orders.push(Decision::act(Actor::City(city.id), Reason::ProduceArmy, clear));
    }
    orders.push(order(empire, city, item, Reason::ProduceArmy));
    if let Some(head) = city.queue.first() {
        orders.push(order(empire, city, *head, Reason::ProduceArmy));
    }
    Some(orders)
}

/// The role and kind of the next war order: siege, ranged, melee — a
/// siege place takes a ranged unit when no siege unit is available.
fn war_unit(obs: &AIObservation<'_>, empire: &Empire, rules: &CompiledRuleset) -> Option<(ArmyRole, UnitKindId)> {
    let demand = empire.war_demand;
    let distance = demand.ranged + demand.siege > 0;
    let wanted = [
        (demand.siege > 0, ArmyRole::Siege),
        (distance, ArmyRole::Ranged),
        (demand.melee > 0, ArmyRole::Melee),
    ];
    wanted
        .into_iter()
        .filter(|(want, _)| *want)
        .find_map(|(_, role)| war_kind(obs, empire, rules, role).map(|kind| (role, kind)))
}

/// The kind of an army role: with `army_fit` the melee and ranged ones by their fit
/// (`Purpose::Take`, `Purpose::Strike`), the siege one and the others the strongest.
fn war_kind(obs: &AIObservation<'_>, empire: &Empire, rules: &CompiledRuleset, role: ArmyRole) -> Option<UnitKindId> {
    use crate::ai::army_fit::Purpose;
    let fitted = match role {
        _ if empire.fit.is_none() => None,
        ArmyRole::Melee => military_kind(obs, empire, rules, Purpose::Take).filter(|k| crate::ai::operations::army_role(rules, *k) == Some(ArmyRole::Melee)),
        ArmyRole::Ranged => military_kind(obs, empire, rules, Purpose::Strike).filter(|k| crate::ai::operations::army_role(rules, *k) == Some(ArmyRole::Ranged)),
        ArmyRole::Siege => None,
    };
    fitted.or_else(|| unit_of_army_role(obs, empire, rules, role))
}

/// Count one ordered unit of `role` off the demand (a ranged unit fills a
/// siege place when no ranged one is wanted).
fn take_demand(demand: &mut Composition, role: ArmyRole) {
    let count = match role {
        ArmyRole::Melee => &mut demand.melee,
        ArmyRole::Siege => &mut demand.siege,
        ArmyRole::Ranged if demand.ranged > 0 => &mut demand.ranged,
        ArmyRole::Ranged => &mut demand.siege,
    };
    *count = count.saturating_sub(1);
}

/// The strongest available unit of an army role (melee strength, else
/// ranged), the cheaper on a tie.
fn unit_of_army_role(
    obs: &AIObservation<'_>,
    empire: &Empire,
    rules: &CompiledRuleset,
    role: ArmyRole,
) -> Option<UnitKindId> {
    let mut best: Option<(u16, u32, UnitKindId)> = None;
    for index in 0..rules.units.len() {
        let kind = UnitKindId(index as u16);
        if crate::ai::operations::army_role(rules, kind) != Some(role) || !unit_available(obs, empire, rules, kind) {
            continue;
        }
        let def = &rules.units[index];
        let strength = if role == ArmyRole::Melee { def.strength } else { def.ranged_strength };
        if best.is_none_or(|(s, c, _)| strength > s || (strength == s && def.cost < c)) {
            best = Some((strength, def.cost, kind));
        }
    }
    best.map(|(_, _, kind)| kind)
}

fn garrisoned(obs: &AIObservation<'_>, rules: &CompiledRuleset, city: &OwnCity) -> bool {
    obs.own_units
        .iter()
        .any(|u| u.tile == city.tile && role_of(rules, u.kind) == AiRole::Military)
}

/// A city with an enemy army in sight and no garrison switches to a
/// defender now (and buys it when the treasury allows).
fn defend_now(
    obs: &AIObservation<'_>,
    brief: &TurnBrief,
    empire: &mut Empire,
    rules: &CompiledRuleset,
    city: &OwnCity,
) -> Option<Vec<Decision>> {
    let head_military = matches!(city.queue.first(), Some(ProductionItem::Unit(k)) if role_of(rules, *k) == AiRole::Military);
    let at_supply = empire.military >= empire.supply;
    if !enemy_near(obs, brief, rules, city)
        || garrisoned(obs, rules, city)
        || head_military
        || at_supply
    {
        return None;
    }
    let defender = military_kind(obs, empire, rules, crate::ai::army_fit::Purpose::Hold)?;
    empire.military += 1;
    let clear = Command::ClearProduction {
        player: empire.player,
        city: city.handle,
    };
    let item = ProductionItem::Unit(defender);
    let mut orders = vec![
        Decision::act(Actor::City(city.id), Reason::ProduceDefender, clear),
        order(empire, city, item, Reason::ProduceDefender),
    ];
    orders.extend(hurry_defender(obs, empire, city, item));
    Some(orders)
}

/// A city in danger (`ai::defence`, the game's AI `standard`: the variants
/// before it have no watch) builds what saves it now when that pays: a
/// defender of the best kind (`defence::defender_net`) or a building that
/// strengthens the city (walls, `defence::building_net`) that it finishes within
/// `defence::RESPONSE_ROUNDS` or can buy, the better net value first. The
/// queue is cleared, the order goes first and the old head after it; it is
/// bought when the treasury allows.
fn defence_orders(obs: &AIObservation<'_>, brief: &TurnBrief, empire: &mut Empire, rules: &CompiledRuleset, city: &OwnCity) -> Option<Vec<Decision>> {
    let watch = brief.defence.as_ref()?;
    let danger = watch.danger(city.tile)?;
    let item = defence_item(obs, brief, empire, rules, city, watch, danger)?;
    if let ProductionItem::Unit(_) = item {
        empire.military += 1;
    }
    let mut orders = Vec::new();
    if !city.queue.is_empty() {
        let clear = Command::ClearProduction { player: empire.player, city: city.handle };
        orders.push(Decision::act(Actor::City(city.id), Reason::ProduceDefender, clear));
    }
    orders.push(order(empire, city, item, Reason::ProduceDefender));
    if let Some(head) = city.queue.first() {
        orders.push(order(empire, city, *head, Reason::ProduceDefender));
    }
    orders.extend(hurry_defender(obs, empire, city, item));
    Some(orders)
}

/// The defender or the city's defense worth most at `danger`, unless the
/// queue already starts with a military unit or a defense.
#[allow(clippy::too_many_arguments)]
fn defence_item(
    obs: &AIObservation<'_>,
    brief: &TurnBrief,
    empire: &Empire,
    rules: &CompiledRuleset,
    city: &OwnCity,
    watch: &crate::ai::defence::Watch,
    danger: &crate::ai::defence::Danger,
) -> Option<ProductionItem> {
    let head_military = matches!(city.queue.first(), Some(ProductionItem::Unit(k)) if role_of(rules, *k) == AiRole::Military);
    let head_defense = matches!(city.queue.first(), Some(ProductionItem::Building(b)) if crate::ai::defence::building_net(brief, rules, danger, city, *b).is_some());
    if head_military || head_defense {
        return None;
    }
    let unit = (empire.military < empire.supply)
        .then(|| military_kind(obs, empire, rules, crate::ai::army_fit::Purpose::Hold))
        .flatten()
        .and_then(|kind| crate::ai::defence::defender_net(brief, rules, watch, danger).map(|net| (net, ProductionItem::Unit(kind))));
    let slot = Slot { city, planned: &[] };
    let in_time = |item: ProductionItem| turns(obs, rules, city, item) <= crate::ai::defence::RESPONSE_ROUNDS || affordable_now(obs, empire, city, item);
    let building = (0..rules.buildings.len())
        .map(|i| BuildingKindId(i as u16))
        .filter(|b| crate::ai::defence::city_defense(rules, *b) != (0, 0))
        .filter(|b| building_available(obs, empire, rules, &slot, *b) && in_time(ProductionItem::Building(*b)))
        .filter_map(|b| crate::ai::defence::building_net(brief, rules, danger, city, b).map(|net| (net, b)))
        .max_by_key(|(net, b)| (*net, std::cmp::Reverse(b.0)))
        .map(|(net, b)| (net, ProductionItem::Building(b)));
    match (unit, building) {
        (Some(u), Some(b)) if b.0 > u.0 => Some(b.1),
        (Some(u), _) => Some(u.1),
        (None, b) => b.map(|b| b.1),
    }
}

/// The treasury buys `item` in this city now (its price from the core's
/// hurry formula on an empty stock) and keeps `HURRY_RESERVE` (with the
/// reserve priced, `PricedStops::reserve`: the deficit's, `deficit_reserve`).
fn affordable_now(obs: &AIObservation<'_>, empire: &Empire, city: &OwnCity, item: ProductionItem) -> bool {
    let price = obs.view.own_production_cost(city.handle, item).and_then(|cost| crate::world::city::hurry_price(cost, 0));
    let kept = if empire.stops.reserve { deficit_reserve(obs) } else { HURRY_RESERVE };
    price.is_some_and(|price| obs.economy.treasury >= price + kept)
}

/// An enemy (at war) military unit the player sees within 3 tiles.
fn enemy_near(
    obs: &AIObservation<'_>,
    brief: &TurnBrief,
    rules: &CompiledRuleset,
    city: &OwnCity,
) -> bool {
    let dims = obs.view.dimensions();
    obs.visible_foreign_units.iter().any(|u| {
        let near = dims.distance(u.tile, city.tile).is_some_and(|d| d <= 3);
        brief.at_war(u.owner) && near && role_of(rules, u.kind) == AiRole::Military
    })
}

/// Buy the new defender at once when the treasury allows (its price from
/// the core's hurry formula on an empty stock).
fn hurry_defender(
    obs: &AIObservation<'_>,
    empire: &Empire,
    city: &OwnCity,
    item: ProductionItem,
) -> Option<Decision> {
    if !affordable_now(obs, empire, city, item) {
        return None;
    }
    let hurry = Command::HurryProduction {
        player: empire.player,
        city: city.handle,
    };
    Some(Decision::act(Actor::City(city.id), Reason::Hurry, hurry))
}

/// A city and the orders planned for it in this pass.
pub(crate) struct Slot<'c> {
    pub(crate) city: &'c OwnCity,
    pub(crate) planned: &'c [ProductionItem],
}

impl Slot<'_> {
    fn has(&self, item: ProductionItem) -> bool {
        self.city.queue.contains(&item) || self.planned.contains(&item)
    }

    fn plans_military(&self, rules: &CompiledRuleset) -> bool {
        let military = |item: &ProductionItem| match item {
            ProductionItem::Unit(kind) => role_of(rules, *kind) == AiRole::Military,
            ProductionItem::Building(_) | ProductionItem::Project(_) => false,
        };
        self.city.queue.iter().chain(self.planned).any(military)
    }
}

/// The next order of one city, by need.
fn choose_order(
    obs: &AIObservation<'_>,
    empire: &mut Empire,
    rules: &CompiledRuleset,
    city: &OwnCity,
    planned: &[ProductionItem],
    allocator: Option<&Allocator>,
) -> Decision {
    let slot = Slot { city, planned };
    if let Some(decision) = garrison_order(obs, empire, rules, &slot) {
        return decision;
    }
    // The World Congress project takes this city's production (its queue
    // runs empty; `ai::congress` sets the process).
    if crate::ai::congress::contributing_city(obs) == Some(city.handle) {
        return Decision::refuse(Actor::City(city.id), Reason::ContributeToProject);
    }
    if let Some(decision) = space_order(obs, empire, rules, &slot) {
        return decision;
    }
    if empire.book || empire.building_order != crate::ai::tall::BuildingOrder::Off {
        if let Some(decision) = book_order(obs, empire, rules, &slot) {
            return decision;
        }
    }
    if let Some(allocator) = allocator {
        return allocated_order(obs, empire, rules, &slot, allocator);
    }
    if let Some(decision) = settler_order(obs, empire, rules, &slot) {
        return decision;
    }
    if let Some(decision) = worker_order(obs, empire, rules, &slot) {
        return decision;
    }
    if let Some(decision) = explorer_order(obs, empire, rules, &slot) {
        return decision;
    }
    if let Some(decision) = army_order(obs, empire, rules, &slot) {
        return decision;
    }
    if let Some(decision) = building_order(obs, empire, rules, &slot) {
        return decision;
    }
    fallback_order(obs, empire, rules, &slot)
}

/// The allocator's order (`ai::allocator`): a scout or the army a defence
/// strategy wants first, then the most valuable order per turn of the
/// city's production, else a process.
fn allocated_order(
    obs: &AIObservation<'_>,
    empire: &mut Empire,
    rules: &CompiledRuleset,
    slot: &Slot<'_>,
    allocator: &Allocator,
) -> Decision {
    if let Some(decision) = explorer_order(obs, empire, rules, slot) {
        return decision;
    }
    if matches!(empire.strategy, Strategy::Defend | Strategy::Recover) {
        if let Some(decision) = army_order(obs, empire, rules, slot) {
            return decision;
        }
    }
    if let Some(decision) = allocator.choose(obs, empire, rules, slot) {
        return decision;
    }
    if empire.no_idle {
        if let Some(decision) = priced_process(obs, allocator.prices(), slot) {
            return decision;
        }
        if let Some(decision) = allocator.choose_unlimited(obs, empire, rules, slot) {
            return decision;
        }
    }
    process_order(obs, slot).unwrap_or_else(|| Decision::refuse(Actor::City(slot.city.id), Reason::NothingToBuild))
}

fn needs_garrison(
    obs: &AIObservation<'_>,
    empire: &Empire,
    rules: &CompiledRuleset,
    city: &OwnCity,
) -> bool {
    let wanted =
        crate::ai::planner_units::garrison_wanted(obs.speed.standard_round(obs.round), &obs.relations, empire.strategy);
    wanted > 0 && !garrisoned(obs, rules, city)
}

fn garrison_order(
    obs: &AIObservation<'_>,
    empire: &mut Empire,
    rules: &CompiledRuleset,
    slot: &Slot<'_>,
) -> Option<Decision> {
    let needed = needs_garrison(obs, empire, rules, slot.city) && !slot.plans_military(rules);
    if !needed || empire.military >= empire.military_cap {
        return None;
    }
    let kind = military_kind(obs, empire, rules, crate::ai::army_fit::Purpose::Hold)?;
    empire.military += 1;
    Some(order(
        empire,
        slot.city,
        ProductionItem::Unit(kind),
        Reason::ProduceDefender,
    ))
}

/// Apollo and the spaceship parts the ship still lacks (`ai::victory`),
/// before anything but a garrison: cities are asked most productive first.
fn space_order(
    obs: &AIObservation<'_>,
    empire: &mut Empire,
    rules: &CompiledRuleset,
    slot: &Slot<'_>,
) -> Option<Decision> {
    if empire.space_finish {
        return None;
    }
    let item = crate::ai::victory::next_space_item(obs, rules, slot.city, &empire.space_planned)?;
    if empire.refused.contains(&item) {
        return None;
    }
    empire.space_planned.push(item);
    Some(order(empire, slot.city, item, Reason::ProduceProject))
}

/// The book's order (`Tall::Book`, a league opponent, content named in
/// `ai::tall`): a settler while the empire has fewer cities than the book's
/// and a settler is wanted; then, with a building order
/// (`tall::BuildingOrder`), the next science building of the book's list or
/// the one the rules' data rank first, while no settler is wanted.
fn book_order(obs: &AIObservation<'_>, empire: &mut Empire, rules: &CompiledRuleset, slot: &Slot<'_>) -> Option<Decision> {
    use crate::ai::tall::BuildingOrder;
    if empire.book_scout {
        let kind = (0..rules.units.len())
            .map(|i| UnitKindId(i as u16))
            .filter(|k| is_recon(rules, *k) && unit_available(obs, empire, rules, *k))
            .min_by_key(|k| (rules.units[k.0 as usize].cost, k.0));
        if let Some(kind) = kind {
            empire.book_scout = false;
            return Some(order(empire, slot.city, ProductionItem::Unit(kind), Reason::ProduceArmy));
        }
    }
    if empire.book {
        if let Some(decision) = settler_order(obs, empire, rules, slot) {
            return Some(decision);
        }
    } else if empire.settlers < empire.settler_target {
        return None;
    }
    let may = |kind: BuildingKindId| building_available(obs, empire, rules, slot, kind);
    let kind = match empire.building_order {
        BuildingOrder::Off => None,
        BuildingOrder::Book => crate::ai::tall::book_next_building(obs, rules, slot.city, may),
        BuildingOrder::Science => crate::ai::tall::science_building(obs, rules, slot.city, may),
    }?;
    Some(order(empire, slot.city, ProductionItem::Building(kind), Reason::ProduceBuilding))
}

fn settler_order(
    obs: &AIObservation<'_>,
    empire: &mut Empire,
    rules: &CompiledRuleset,
    slot: &Slot<'_>,
) -> Option<Decision> {
    if empire.settlers >= empire.settler_target || slot.city.population < SETTLER_MIN_POPULATION {
        return None;
    }
    let kind = unit_of_role(obs, empire, rules, AiRole::Settler)?;
    if turns(obs, rules, slot.city, ProductionItem::Unit(kind)) > obs.speed.pace_rounds(SLOW_ORDER_TURNS) {
        return None;
    }
    empire.settlers += 1;
    Some(order(
        empire,
        slot.city,
        ProductionItem::Unit(kind),
        Reason::ProduceSettler,
    ))
}

fn worker_order(
    obs: &AIObservation<'_>,
    empire: &mut Empire,
    rules: &CompiledRuleset,
    slot: &Slot<'_>,
) -> Option<Decision> {
    if empire.workers >= empire.worker_target {
        return None;
    }
    let kind = unit_of_role(obs, empire, rules, AiRole::Worker)?;
    empire.workers += 1;
    Some(order(
        empire,
        slot.city,
        ProductionItem::Unit(kind),
        Reason::ProduceWorker,
    ))
}

fn army_order(
    obs: &AIObservation<'_>,
    empire: &mut Empire,
    rules: &CompiledRuleset,
    slot: &Slot<'_>,
) -> Option<Decision> {
    let wants_army = matches!(
        empire.strategy,
        Strategy::Attack | Strategy::Defend | Strategy::Recover
    );
    if !wants_army || empire.military >= empire.military_cap {
        return None;
    }
    let purpose = if empire.fit.is_some() { field_purpose(obs, rules) } else { crate::ai::army_fit::Purpose::Hold };
    let kind = military_kind(obs, empire, rules, purpose)?;
    empire.military += 1;
    Some(order(
        empire,
        slot.city,
        ProductionItem::Unit(kind),
        Reason::ProduceArmy,
    ))
}

/// A city whose order is done builds the wanted recon unit next, ahead of
/// the settlers it would queue again at once.
fn recon_orders(
    obs: &AIObservation<'_>,
    empire: &mut Empire,
    rules: &CompiledRuleset,
    city: &OwnCity,
) -> Option<Vec<Decision>> {
    if !city.queue.is_empty() {
        return None;
    }
    explorer_order(obs, empire, rules, &Slot { city, planned: &[] }).map(|order| vec![order])
}

/// The cheapest recon unit when the unknown part of the map calls for one
/// or the capital its leader looks for.
fn explorer_order(
    obs: &AIObservation<'_>,
    empire: &mut Empire,
    rules: &CompiledRuleset,
    slot: &Slot<'_>,
) -> Option<Decision> {
    if !empire.wants_explorer || empire.military >= empire.military_cap {
        return None;
    }
    let kind = (0..rules.units.len())
        .map(|i| UnitKindId(i as u16))
        .filter(|k| is_recon(rules, *k) && unit_available(obs, empire, rules, *k))
        .min_by_key(|k| (rules.units[k.0 as usize].cost, k.0))?;
    empire.wants_explorer = false;
    empire.military += 1;
    Some(order(
        empire,
        slot.city,
        ProductionItem::Unit(kind),
        Reason::ProduceArmy,
    ))
}

/// Can the city build this building now (and is it not ordered yet)?
pub(crate) fn building_available(
    obs: &AIObservation<'_>,
    empire: &Empire,
    rules: &CompiledRuleset,
    slot: &Slot<'_>,
    kind: BuildingKindId,
) -> bool {
    let Some(def) = rules.buildings.get(kind.0 as usize) else {
        return false;
    };
    let item = ProductionItem::Building(kind);
    let gate = rules.production_gate(item, empire.nation).is_none();
    let has = slot.city.buildings.contains(&kind) || slot.has(item) || empire.refused.contains(&item);
    let requires = def
        .requires_buildings
        .iter()
        .all(|b| slot.city.buildings.contains(b));
    // A building that consumes a strategic resource (a Factory: Coal) is
    // refused without it; chosen again every round it left the city idle.
    let resources = !empire.sound_orders
        || def.consumes_resources.iter().all(|(r, n)| obs.economy.resources.available(*r) >= i32::from(*n));
    let unique = def.wonder || def.kind == crate::rules::content::BuildingKind::NationalWonder;
    let wonder_free = !unique || !empire.wonders_taken.contains(&kind);
    // No new maintenance at a gold deficit (M4-A), unless
    // the building earns gold itself; with the upkeep priced
    // (`PricedStops::upkeep`) no deficit the treasury cannot carry.
    let after = obs.economy.income.gold - def.maintenance as i32;
    let paid = if empire.stops.upkeep { crate::ai::citizens::carries(after, obs.economy.treasury) } else { after >= 0 };
    let affordable = def.maintenance == 0 || paid || crate::ai::planner_value::building_gold(rules, kind) > 0;
    gate && !def.unbuildable
        && resources
        && def.cost > 0
        && !has
        && requires
        && wonder_free
        && knows(obs, def.required_tech)
        && affordable
        && obs.view.own_building_site(slot.city.handle, kind).is_ok()
        && crate::ai::policies::culture_allows(obs, rules, slot.city.handle, kind)
}

/// The most valuable building per cost that the city finishes in time.
fn building_order(
    obs: &AIObservation<'_>,
    empire: &mut Empire,
    rules: &CompiledRuleset,
    slot: &Slot<'_>,
) -> Option<Decision> {
    let cities = obs.own_cities.len() as i32;
    let mut best: Option<(i64, BuildingKindId)> = None;
    for index in 0..rules.buildings.len() {
        let kind = BuildingKindId(index as u16);
        if !building_available(obs, empire, rules, slot, kind) {
            continue;
        }
        let value = building_value(rules, kind, &empire.weights, cities)
            + empire.weights.science * crate::ai::espionage::counter_espionage_value(obs, rules, slot.city, kind);
        let item = ProductionItem::Building(kind);
        if value <= 0 || turns(obs, rules, slot.city, item) > obs.speed.pace_rounds(SLOW_ORDER_TURNS) {
            continue;
        }
        let score = i64::from(value) * 1000 / i64::from(rules.buildings[index].cost.max(1));
        if best.is_none_or(|(s, b)| score > s || (score == s && kind.0 < b.0)) {
            best = Some((score, kind));
        }
    }
    let (_, kind) = best?;
    let def = &rules.buildings[kind.0 as usize];
    if def.wonder || def.kind == crate::rules::content::BuildingKind::NationalWonder {
        empire.wonders_taken.push(kind);
    }
    Some(order(
        empire,
        slot.city,
        ProductionItem::Building(kind),
        Reason::ProduceBuilding,
    ))
}

/// Nothing is needed: army up to the cap, one spare worker, the cheapest
/// building; or a refusal (the city then produces nothing).
fn fallback_order(
    obs: &AIObservation<'_>,
    empire: &mut Empire,
    rules: &CompiledRuleset,
    slot: &Slot<'_>,
) -> Decision {
    if empire.military < empire.military_cap {
        if let Some(decision) = unit_order(obs, empire, rules, slot, AiRole::Military) {
            return decision;
        }
    }
    if empire.workers < empire.worker_target + 1 {
        if let Some(decision) = unit_order(obs, empire, rules, slot, AiRole::Worker) {
            return decision;
        }
    }
    let cheapest = (0..rules.buildings.len())
        .map(|i| BuildingKindId(i as u16))
        .filter(|b| building_available(obs, empire, rules, slot, *b))
        .min_by_key(|b| (rules.buildings[b.0 as usize].cost, b.0));
    if let Some(kind) = cheapest {
        let item = ProductionItem::Building(kind);
        return order(empire, slot.city, item, Reason::ProduceBuilding);
    }
    // Nothing worth building: the queue may run empty and the city turns
    // its production into gold or science (the wealth / research processes; filler
    // units only crowded the cities).
    if let Some(decision) = process_order(obs, slot) {
        return decision;
    }
    Decision::refuse(Actor::City(slot.city.id), Reason::NothingToBuild)
}

/// Wealth while the empire earns little gold, else Research, for a city
/// whose queue runs empty (once: the process stays until changed).
/// `no-idle-cities`: of the processes the rules open to the city, the one whose
/// yield (`systems::processes::process_yield` of its production) the prices value
/// most, when worth anything (no command when the city runs it already); None when
/// none is open.
fn priced_process(obs: &AIObservation<'_>, prices: &crate::ai::value::Prices, slot: &Slot<'_>) -> Option<Decision> {
    use crate::world::city_economy::Process;
    let runs_empty = slot.city.queue.len() <= 1 && slot.planned.is_empty();
    if !runs_empty {
        return None;
    }
    let worth = |process: Process| {
        let (gold, science) = crate::systems::processes::process_yield(Some(process), 100);
        i64::from(gold) * prices.gold + i64::from(science) * prices.science
    };
    let command = |process: Process| Command::SetProcess { player: obs.player, city: slot.city.handle, process: Some(process) };
    let best = [Process::Wealth, Process::Research]
        .into_iter()
        .filter(|p| worth(*p) > 0 && (slot.city.process == Some(*p) || obs.view.accepts(&command(*p))))
        .max_by_key(|p| worth(*p))?;
    Some(if slot.city.process == Some(best) { Decision::refuse(Actor::City(slot.city.id), Reason::NothingToBuild) } else { Decision::act(Actor::City(slot.city.id), Reason::NothingToBuild, command(best)) })
}

fn process_order(obs: &AIObservation<'_>, slot: &Slot<'_>) -> Option<Decision> {
    use crate::world::city_economy::Process;
    let runs_empty = slot.city.queue.len() <= 1 && slot.planned.is_empty();
    if !runs_empty {
        return None;
    }
    let process = if obs.economy.income.gold < crate::ai::citizens::GOLD_COMFORT {
        Process::Wealth
    } else {
        Process::Research
    };
    if slot.city.process == Some(process) {
        return None;
    }
    let command = Command::SetProcess {
        player: obs.player,
        city: slot.city.handle,
        process: Some(process),
    };
    obs.view
        .accepts(&command)
        .then(|| Decision::act(Actor::City(slot.city.id), Reason::NothingToBuild, command))
}

/// The best available unit of a role, counted in the empire's totals.
fn unit_order(
    obs: &AIObservation<'_>,
    empire: &mut Empire,
    rules: &CompiledRuleset,
    slot: &Slot<'_>,
    role: AiRole,
) -> Option<Decision> {
    let kind = unit_of_role(obs, empire, rules, role)?;
    let reason = match role {
        AiRole::Worker => {
            empire.workers += 1;
            Reason::ProduceWorker
        }
        _ => {
            empire.military += 1;
            Reason::ProduceArmy
        }
    };
    Some(order(empire, slot.city, ProductionItem::Unit(kind), reason))
}

/// A settler that met danger and has no escort calls for one: the nearest
/// city builds the cheapest soldier at once. The garrison stays in its city,
/// so no other unit came and the settler retreated and stepped out again
/// for 30 rounds (`inspect standard continents 3 0`: the second city at
/// round 56 instead of 30).
fn escort_order(
    obs: &AIObservation<'_>,
    brief: &TurnBrief,
    state: &AiState,
    empire: &mut Empire,
    rules: &CompiledRuleset,
    city: &OwnCity,
) -> Option<Vec<Decision>> {
    use crate::ai::state::UnitTask;
    let dims = obs.view.dimensions();
    let is_military = |item: &ProductionItem| matches!(item, ProductionItem::Unit(k) if role_of(rules, *k) == AiRole::Military);
    if city.queue.first().is_some_and(is_military) || empire.military >= empire.military_cap {
        return None;
    }
    let ward = brief.units.iter().filter(|u| u.role == AiRole::Settler).find(|u| {
        let task = state.task_of(u.id);
        let waits = task.is_some_and(|t| matches!(t.task, UnitTask::Settle { .. }) && t.failures > 0);
        let escorted = state.tasks().iter().any(|t| t.task == UnitTask::Escort { ward: u.id });
        waits && !escorted
    })?;
    // The city nearest to the settler builds it.
    let nearest = brief.nearest_city(dims, ward.tile)?;
    if nearest.tile != city.tile {
        return None;
    }
    let kind = cheapest_soldier(obs, empire, rules)?;
    let item = ProductionItem::Unit(kind);
    if turns(obs, rules, city, item) > obs.speed.pace_rounds(ESCORT_BUILD_TURNS) {
        return None;
    }
    empire.military += 1;
    let mut orders = Vec::new();
    if let Some(head) = city.queue.first() {
        orders.push(Decision::act(Actor::City(city.id), Reason::ProduceArmy, Command::ClearProduction { player: empire.player, city: city.handle }));
        orders.push(order(empire, city, item, Reason::ProduceArmy));
        orders.push(order(empire, city, *head, Reason::ProduceArmy));
    } else {
        orders.push(order(empire, city, item, Reason::ProduceArmy));
    }
    Some(orders)
}

/// While the empire has fewer cities than this (only the second city), a city
/// that can train a settler puts it ahead of a building in its queue: the stock
/// the building gathered (a Granary of the size-1 capital) goes into the
/// settler. Pace suite: the second city about 1.5 rounds earlier. For the
/// third city (3) the first worker comes later and the luxuries stay
/// unconnected (`happiness_pace`); for the fourth (4) it is more cities at
/// round 100, but 59 % of the points against `allocator` instead of 72 %
/// (platform/docs/ai-bench.md, "Second city: the settler ahead of the queue").
pub const SETTLER_AHEAD_CITIES: usize = 2;
/// A settler taking longer than this is not put ahead (Standard speed; × the speed's pace).
pub const SETTLER_AHEAD_TURNS: u32 = 20;

/// Put the settler ahead of the head of the queue when the head is a
/// building (a wonder keeps its place) and the settler is worth its time
/// (`Prices::new_city`), under `PriceModel::Uses`.
fn settler_ahead(
    obs: &AIObservation<'_>,
    empire: &mut Empire,
    rules: &CompiledRuleset,
    city: &OwnCity,
    allocator: &Allocator,
) -> Option<Vec<Decision>> {
    let prices = allocator.prices();
    let cities = obs.own_cities.len();
    let wanted = empire.settlers < empire.settler_target && city.population >= SETTLER_MIN_POPULATION;
    if prices.model != PriceModel::Uses || cities >= SETTLER_AHEAD_CITIES || !wanted {
        return None;
    }
    let head = *city.queue.first()?;
    let ProductionItem::Building(building) = head else { return None };
    if rules.buildings[building.0 as usize].wonder {
        return None;
    }
    let item = ProductionItem::Unit(unit_of_role(obs, empire, rules, AiRole::Settler)?);
    let time = turns(obs, rules, city, item);
    if time > obs.speed.pace_rounds(SETTLER_AHEAD_TURNS) || prices.new_city(time.saturating_add(SETTLE_WALK_ROUNDS)) <= 0 {
        return None;
    }
    empire.settlers += 1;
    Some(vec![
        Decision::act(Actor::City(city.id), Reason::ProduceSettler, Command::ClearProduction { player: empire.player, city: city.handle }),
        order(empire, city, item, Reason::ProduceSettler),
        order(empire, city, head, Reason::ProduceSettler),
    ])
}

/// An escort that takes longer than this is not ordered (Standard speed; × the speed's pace).
const ESCORT_BUILD_TURNS: u32 = 12;

fn cheapest_soldier(obs: &AIObservation<'_>, empire: &Empire, rules: &CompiledRuleset) -> Option<UnitKindId> {
    (0..rules.units.len())
        .map(|i| UnitKindId(i as u16))
        .filter(|k| crate::ai::operations::army_role(rules, *k) == Some(ArmyRole::Melee) && unit_available(obs, empire, rules, *k))
        .min_by_key(|k| (rules.units[k.0 as usize].cost, k.0))
}
