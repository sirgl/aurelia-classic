//! The rival-model probe (rival/REPORT.md, step 1): what a seat loses in wonder races, how old its
//! knowledge of the other nations is, and how long before a first blow it saw the blow's army.
//!
//! Probe only: it reads the authoritative world (the truth) and the seats' own knowledge, and
//! changes nothing, so the result hash of a match does not move. It is on when `AURELIA_RIVAL_PROBE`
//! names a file; every finished match appends one JSON line to it.

use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::sync::{Mutex, OnceLock};

use kernel::ids::{BuildingKindId, CityHandle, PlayerId, ProductionItem};
use kernel::knowledge::view::{is_visible, unit_seen_by, PlayerView};
use kernel::rules::CompiledRuleset;
use kernel::transaction::journal::{AppliedCommand, CityChange};
use kernel::world::state::GameState;
use serde::Serialize;

use crate::bench::modules::{BenchRules, CityBlow};
use crate::bench::suite::Side;

/// The environment variable that turns the probe on (a file the lines are appended to).
pub const PROBE_ENV: &str = "AURELIA_RIVAL_PROBE";

/// A hostile unit this close to a city is a threat (`ai::defence::THREAT_RADIUS`).
const THREAT_RADIUS: i32 = classic_rules::ai::defence::THREAT_RADIUS;
/// Rounds of a window of the hammers and burned hammers by time (a fifth of a pace game).
const WINDOW: u32 = 66;

fn add_window(windows: &mut Vec<u64>, round: u32, amount: u64) {
    let at = (round.saturating_sub(1) / WINDOW) as usize;
    if windows.len() <= at {
        windows.resize(at + 1, 0);
    }
    windows[at] += amount;
}

/// Every this many rounds the seat's production is sampled (the sum of the samples × this is its total).
const SAMPLE_EVERY: u32 = 10;

#[derive(Debug, Clone, Copy)]
struct Order {
    owner: PlayerId,
    since: u32,
    /// The wonder was at the head of the queue when the turn's orders were last seen.
    head: bool,
    /// The cost of the order at the head, after modifiers (last seen).
    cost: u32,
    lost: Option<Loss>,
    /// Index of its record in `RivalProbe::order_log`.
    rec: usize,
}

/// One order of a world wonder, with what its seat knew when it was put (the calibration data of
/// the race model: `outcome` is what became of it).
#[derive(Debug, Clone, Serialize)]
pub struct OrderRec {
    pub seat: u8,
    pub round: u32,
    pub wonder: String,
    /// Cost after modifiers (0: not at the head, unknown), the city's stock and its production a turn.
    pub cost: u32,
    pub stock: u32,
    pub rate: u32,
    /// Turns to finish at that rate (0: unknown), and the speed's pace percent.
    pub turns: u32,
    pub pace: u32,
    /// Living major rivals, those met, those met whose era reaches the era of the wonder's technology,
    /// and those not met.
    pub rivals: u32,
    pub met: u32,
    pub able_met: u32,
    /// Largest population among the sighted cities of the able met rivals (0: none sighted), and how
    /// many rounds ago that sighting was.
    pub best_pop: u32,
    pub best_pop_age: u32,
    /// Rank by score of the seat among the living majors when the order was put (1 = best).
    pub rank_score: u32,
    /// The same among the seat and the met rivals only (the victory screen's score list).
    pub rank_met: u32,
    /// "won", "lost" (another city completed the wonder first), "cancelled", "open".
    pub outcome: &'static str,
    pub closed: u32,
}

#[derive(Debug, Clone, Copy)]
struct Loss {
    round: u32,
    /// The city's stock at the moment the other city completed the wonder.
    stock: u32,
    head: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct WonderTally {
    /// Orders of a world wonder put in a city's queue.
    pub ordered: u32,
    /// Wonders completed.
    pub completed: u32,
    /// Hammers (cost at the head) of the completed ones.
    pub completed_hammers: u64,
    /// Orders another city completed the wonder under (own: another city of the same owner).
    pub lost: u32,
    pub lost_own: u32,
    pub lost_head: u32,
    /// The city's stock when the wonder went (summed over the lost orders at the head).
    pub invested_at_loss: u64,
    /// Lost orders the rules burned: the head was consumed without a reward, the cost destroyed.
    pub burned: u32,
    pub burned_hammers: u64,
    /// Lost orders the seat dropped before the head was consumed: the stock stays in the city.
    pub redirected: u32,
    pub redirected_stock: u64,
    /// Orders dropped for another reason (nobody completed the wonder).
    pub cancelled: u32,
    /// Orders still open at the end.
    pub open: u32,
    /// Rounds the lost orders had been open, summed.
    pub lost_rounds: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Completion {
    pub round: u32,
    pub wonder: String,
    pub builder: u8,
    /// 1 = best of the living majors.
    pub rank_score: u32,
    pub rank_techs: u32,
    pub majors: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgeRow {
    pub round: u32,
    pub seat: u8,
    pub nation: u8,
    /// Cities the nation has, and those the seat has ever seen (its sightings or in sight now).
    pub cities: u32,
    pub seen: u32,
    /// The sum over the seen of the rounds since each was last in sight.
    pub age_sum: u64,
    /// Rounds since any military unit of the nation was in sight (None: never).
    pub unit_age: Option<u32>,
    /// Rounds since any unit or city of the nation was in sight (`Contact::last_seen_round`).
    pub contact_age: Option<u32>,
    /// Military units of the nation alive.
    pub units: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct Blow {
    pub round: u32,
    pub victim: u8,
    pub attacker: u8,
    /// Rounds from the first sighting of the attacker's units within the threat radius of the city
    /// (an unbroken run in the victim's own turn-start views) to the blow; 0: not seen before.
    pub warning: u32,
    /// The attacker's first blow on any city of the victim.
    pub first_on_seat: bool,
    /// Units of the attacker within the radius at the victim's last look (truth), seen or not.
    pub truth_near: u32,
}

#[derive(Debug, Clone, Serialize)]
struct SeatLine {
    player: u8,
    side: &'static str,
    wonders: WonderTally,
    /// Sum of the sampled production a turn × `SAMPLE_EVERY`: the seat's hammers over the match.
    hammers: u64,
    /// The same by windows of `WINDOW` rounds, and the burned hammers by the window of the burn.
    hammers_by: Vec<u64>,
    burned_by: Vec<u64>,
}

#[derive(Debug, Clone, Serialize)]
struct Line<'a> {
    label: &'a str,
    seed: u64,
    candidate: &'a str,
    baseline: &'a str,
    rounds: u32,
    seats: Vec<SeatLine>,
    completions: Vec<Completion>,
    ages: Vec<AgeRow>,
    blows: Vec<Blow>,
    orders: Vec<OrderRec>,
}

/// The probe of one match.
pub struct RivalProbe {
    seats: Vec<(PlayerId, Side)>,
    tally: BTreeMap<u8, WonderTally>,
    hammers: BTreeMap<u8, u64>,
    hammers_by: BTreeMap<u8, Vec<u64>>,
    burned_by: BTreeMap<u8, Vec<u64>>,
    orders: HashMap<(CityHandle, BuildingKindId), Order>,
    completions: Vec<Completion>,
    ages: Vec<AgeRow>,
    blows: Vec<Blow>,
    order_log: Vec<OrderRec>,
    /// Last round a military unit of the nation was in sight of the seat: (seat, nation).
    unit_seen: HashMap<(u8, u8), u32>,
    /// Since when units of `nation` have been in sight within the radius of the seat's city.
    watch: HashMap<(u8, CityHandle, u8), u32>,
    /// Units of the nation within the radius of the city at the seat's last look (truth).
    near_truth: HashMap<(u8, CityHandle, u8), u32>,
    /// Owner and tile of every city at the start of the active player's turn.
    cities_then: HashMap<CityHandle, (PlayerId, kernel::ids::TileId)>,
    struck: HashMap<(u8, CityHandle), ()>,
    struck_seat: HashMap<(u8, u8), ()>,
}

fn sink() -> Option<&'static Mutex<std::fs::File>> {
    static SINK: OnceLock<Option<Mutex<std::fs::File>>> = OnceLock::new();
    SINK.get_or_init(|| {
        let path = std::env::var(PROBE_ENV).ok().filter(|p| !p.is_empty())?;
        let file = std::fs::OpenOptions::new().create(true).append(true).open(path).ok()?;
        Some(Mutex::new(file))
    })
    .as_ref()
}

/// Whether the probe is on.
pub fn enabled() -> bool {
    sink().is_some()
}

impl RivalProbe {
    pub fn new(seats: &[(PlayerId, Side)]) -> Self {
        Self {
            seats: seats.to_vec(),
            tally: seats.iter().map(|(p, _)| (p.0, WonderTally::default())).collect(),
            hammers: BTreeMap::new(),
            hammers_by: BTreeMap::new(),
            burned_by: BTreeMap::new(),
            orders: HashMap::new(),
            completions: Vec::new(),
            ages: Vec::new(),
            blows: Vec::new(),
            order_log: Vec::new(),
            unit_seen: HashMap::new(),
            watch: HashMap::new(),
            near_truth: HashMap::new(),
            cities_then: HashMap::new(),
            struck: HashMap::new(),
            struck_seat: HashMap::new(),
        }
    }

    fn is_seat(&self, p: PlayerId) -> bool {
        self.seats.iter().any(|(s, _)| *s == p)
    }

    /// The start of `active`'s turn, before its AI acts: what it sees of the others' armies near its
    /// cities, and the cities' owners for the blows of this round.
    pub fn turn_start<R: BenchRules>(&mut self, game: &GameState<R::M>, rules: &CompiledRuleset<R::M>, active: PlayerId) {
        let auth = game.view().auth;
        self.cities_then = auth.cities().iter().map(|(h, c)| (h, (c.owner, c.tile))).collect();
        if !self.is_seat(active) {
            return;
        }
        let now = auth.turn().round;
        let view = PlayerView::new(auth, game.derived(), rules, active);
        let dims = view.dimensions();
        let mine: Vec<(CityHandle, kernel::ids::TileId)> = auth.cities().iter().filter(|(_, c)| c.owner == active).map(|(h, c)| (h, c.tile)).collect();
        let mut present: HashMap<(CityHandle, u8), u32> = HashMap::new();
        let mut truth: HashMap<(CityHandle, u8), u32> = HashMap::new();
        let mut military_seen: HashMap<u8, ()> = HashMap::new();
        for core in auth.units().iter_cores() {
            if core.owner == active || !auth.players().is_major(core.owner) {
                continue;
            }
            if kernel::ai::plan::unit_power(rules, core.kind, core.hp) == 0 {
                continue;
            }
            let seen = unit_seen_by(auth, game.derived(), rules, active, core);
            if seen {
                military_seen.insert(core.owner.0, ());
            }
            for (city, tile) in &mine {
                if dims.distance(*tile, core.tile).is_some_and(|d| d <= THREAT_RADIUS) {
                    *truth.entry((*city, core.owner.0)).or_default() += 1;
                    if seen {
                        *present.entry((*city, core.owner.0)).or_default() += 1;
                    }
                }
            }
        }
        for nation in military_seen.keys() {
            self.unit_seen.insert((active.0, *nation), now);
        }
        self.watch.retain(|(seat, city, nation), _| *seat != active.0 || present.contains_key(&(*city, *nation)));
        for (city, nation) in present.keys() {
            self.watch.entry((active.0, *city, *nation)).or_insert(now);
        }
        self.near_truth.retain(|(seat, _, _), _| *seat != active.0);
        for ((city, nation), n) in truth {
            self.near_truth.insert((active.0, city, nation), n);
        }
    }

    /// `owner`'s orders before the end of its turn: the queue as the AI left it.
    pub fn before_end<R: BenchRules>(&mut self, game: &GameState<R::M>, rules: &CompiledRuleset<R::M>, owner: PlayerId) {
        if !self.is_seat(owner) {
            return;
        }
        let auth = game.view().auth;
        let now = auth.turn().round;
        let view = PlayerView::new(auth, game.derived(), rules, owner);
        let mut queued: HashMap<(CityHandle, BuildingKindId), bool> = HashMap::new();
        for (h, core) in auth.cities().iter().filter(|(_, c)| c.owner == owner) {
            for (i, item) in core.queue.iter().enumerate() {
                if let ProductionItem::Building(kind) = *item {
                    if rules.buildings.get(kind.0 as usize).is_some_and(|d| d.wonder) {
                        queued.insert((h, kind), i == 0);
                    }
                }
            }
        }
        // Orders gone from the queue before the end of the turn: the seat dropped them.
        let gone: Vec<(CityHandle, BuildingKindId)> = self.orders.iter().filter(|(k, o)| o.owner == owner && !queued.contains_key(k)).map(|(k, _)| *k).collect();
        for key in gone {
            let order = self.orders.remove(&key).expect("listed above");
            let stock = auth.cities().get(key.0).map_or(0, |c| c.production);
            let tally = self.tally.get_mut(&owner.0).expect("seat");
            self.order_log[order.rec].outcome = if order.lost.is_some() { "lost" } else { "cancelled" };
            self.order_log[order.rec].closed = now;
            match order.lost {
                Some(loss) => {
                    tally.redirected += 1;
                    tally.redirected_stock += u64::from(stock);
                    tally.lost_rounds += u64::from(loss.round.saturating_sub(order.since));
                }
                None => tally.cancelled += 1,
            }
        }
        for (key, head) in queued {
            let cost = if head { view.own_production_cost(key.0, ProductionItem::Building(key.1)).unwrap_or(0) } else { 0 };
            match self.orders.get_mut(&key) {
                Some(order) => {
                    order.head = head;
                    if head {
                        order.cost = cost;
                    }
                }
                None => {
                    self.tally.get_mut(&owner.0).expect("seat").ordered += 1;
                    let rec = self.order_log.len();
                    let record = self.record_order::<R>(game, rules, owner, key, now, head, cost);
                    self.order_log.push(record);
                    self.orders.insert(key, Order { owner, since: now, head, cost, lost: None, rec });
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn record_order<R: BenchRules>(&self, game: &GameState<R::M>, rules: &CompiledRuleset<R::M>, owner: PlayerId, key: (CityHandle, BuildingKindId), now: u32, head: bool, cost: u32) -> OrderRec {
        let auth = game.view().auth;
        let players = auth.players();
        let (pace, rates) = R::order_inputs(game, rules, owner);
        let city = auth.cities().get(key.0);
        let stock = city.map_or(0, |c| c.production);
        let rate = rates.iter().find(|(h, _)| *h == key.0).map_or(0, |(_, r)| *r);
        let def = &rules.buildings[key.1 .0 as usize];
        let wonder_era = def
            .required_tech
            .and_then(|t| rules.technologies.get(t.0 as usize))
            .and_then(|t| t.content_era)
            .and_then(|e| rules.eras.get(e.0 as usize))
            .map_or(0, |e| e.index);
        let view = PlayerView::new(auth, game.derived(), rules, owner);
        let (mut rivals, mut met, mut able_met, mut best_pop, mut best_pop_age) = (0, 0, 0, 0, 0);
        let knowledge = auth.knowledge().get(owner.0 as usize);
        for rival in (0..players.count() as u8).map(PlayerId).filter(|p| *p != owner && players.is_major(*p) && players.is_alive(*p)) {
            rivals += 1;
            if view.contact(rival).met_round.is_none() {
                continue;
            }
            met += 1;
            if kernel::world::player::player_era(auth, rules, rival) < wonder_era {
                continue;
            }
            able_met += 1;
            for (_, c) in auth.cities().iter().filter(|(_, c)| c.owner == rival) {
                let (pop, age) = if is_visible(game.derived(), owner, c.tile) {
                    (c.population, 0)
                } else if let Some(seen) = knowledge.and_then(|k| k.sightings().find(|(t, _)| *t == c.tile)).and_then(|(_, s)| s.seen.as_ref()) {
                    (seen.population, now.saturating_sub(knowledge.and_then(|k| k.memory(c.tile)).map_or(now, |m| m.seen_round())))
                } else {
                    continue;
                };
                if pop > best_pop {
                    (best_pop, best_pop_age) = (pop, age);
                }
            }
        }
        let mine = R::score(game, rules, owner);
        let rank_score = 1 + (0..players.count() as u8).map(PlayerId).filter(|p| *p != owner && players.is_major(*p) && players.is_alive(*p) && R::score(game, rules, *p) > mine).count() as u32;
        let rank_met = 1
            + (0..players.count() as u8)
                .map(PlayerId)
                .filter(|p| *p != owner && players.is_major(*p) && players.is_alive(*p) && view.contact(*p).met_round.is_some() && R::score(game, rules, *p) > mine)
                .count() as u32;
        let remaining = cost.saturating_sub(stock);
        OrderRec {
            seat: owner.0,
            round: now,
            wonder: def.key.clone(),
            cost: if head { cost } else { 0 },
            stock,
            rate,
            turns: if head && rate > 0 { remaining.div_ceil(rate) } else { 0 },
            pace,
            rivals,
            met,
            able_met,
            best_pop,
            best_pop_age,
            rank_score,
            rank_met,
            outcome: "open",
            closed: 0,
        }
    }

    /// After `player`'s turn, its end included: completions, lost races, burned orders and the blows.
    pub fn player_done<R: BenchRules>(&mut self, game: &GameState<R::M>, rules: &CompiledRuleset<R::M>, player: PlayerId, applied: &[AppliedCommand<R::M>]) {
        let auth = game.view().auth;
        let now = auth.turn().round;
        for command in applied {
            for change in &command.changes.city_changes {
                if let CityChange::BuildingAdded { city, building } = *change {
                    if rules.buildings.get(building.0 as usize).is_some_and(|d| d.wonder) {
                        self.completed::<R>(game, rules, city, building);
                    }
                }
            }
        }
        // Orders of the player that left the queue in the end of the turn.
        if self.is_seat(player) {
            let still: Vec<(CityHandle, BuildingKindId)> = self
                .orders
                .iter()
                .filter(|(k, o)| o.owner == player && auth.cities().get(k.0).is_none_or(|c| !c.queue.contains(&ProductionItem::Building(k.1))))
                .map(|(k, _)| *k)
                .collect();
            for key in still {
                let order = self.orders.remove(&key).expect("listed above");
                let city = auth.cities().get(key.0);
                let tally = self.tally.get_mut(&player.0).expect("seat");
                let built_here = city.is_some_and(|c| c.buildings.contains(&key.1));
                self.order_log[order.rec].outcome = if built_here { "won" } else if order.lost.is_some() { "lost" } else { "cancelled" };
                self.order_log[order.rec].closed = now;
                if built_here {
                    tally.completed += 1;
                    tally.completed_hammers += u64::from(order.cost);
                } else if let Some(loss) = order.lost {
                    tally.lost_rounds += u64::from(loss.round.saturating_sub(order.since));
                    if order.head {
                        tally.burned += 1;
                        tally.burned_hammers += u64::from(order.cost);
                        add_window(self.burned_by.entry(player.0).or_default(), now, u64::from(order.cost));
                    } else {
                        tally.redirected += 1;
                    }
                } else {
                    tally.cancelled += 1;
                }
            }
        }
        // Blows of this player's turn on other majors' cities.
        for command in applied {
            let target_city = |tile: kernel::ids::TileId| self.cities_then.iter().find(|(_, (_, t))| *t == tile).map(|(h, _)| *h);
            let city = match R::city_blow(&command.command) {
                Some(CityBlow::City(city)) => Some(city),
                Some(CityBlow::Tile(tile)) => target_city(tile),
                None => None,
            };
            let Some(city) = city else { continue };
            let Some(&(victim, _)) = self.cities_then.get(&city) else { continue };
            if victim == player || !self.is_seat(victim) || !auth.players().is_major(player) {
                continue;
            }
            if self.struck.insert((player.0, city), ()).is_some() {
                continue;
            }
            let first_on_seat = self.struck_seat.insert((victim.0, player.0), ()).is_none();
            let warning = self.watch.get(&(victim.0, city, player.0)).map_or(0, |since| now.saturating_sub(*since));
            let truth_near = self.near_truth.get(&(victim.0, city, player.0)).copied().unwrap_or(0);
            self.blows.push(Blow { round: now, victim: victim.0, attacker: player.0, warning, first_on_seat, truth_near });
        }
    }

    /// A world wonder stands in `city` now: its builder's order closes (counted in `player_done`),
    /// and every other open order of the wonder is lost.
    fn completed<R: BenchRules>(&mut self, game: &GameState<R::M>, rules: &CompiledRuleset<R::M>, city: CityHandle, wonder: BuildingKindId) {
        let auth = game.view().auth;
        let now = auth.turn().round;
        let Some(builder) = auth.cities().get(city).map(|c| c.owner) else { return };
        let players = auth.players();
        let living: Vec<PlayerId> = (0..players.count() as u8).map(PlayerId).filter(|p| players.is_major(*p) && players.is_alive(*p)).collect();
        let score = |p: PlayerId| R::score(game, rules, p);
        let mine = score(builder);
        let techs = players.techs_of(builder).len();
        self.completions.push(Completion {
            round: now,
            wonder: rules.buildings[wonder.0 as usize].key.clone(),
            builder: builder.0,
            rank_score: 1 + living.iter().filter(|p| **p != builder && score(**p) > mine).count() as u32,
            rank_techs: 1 + living.iter().filter(|p| **p != builder && players.techs_of(**p).len() > techs).count() as u32,
            majors: living.len() as u32,
        });
        let keys: Vec<(CityHandle, BuildingKindId)> = self.orders.keys().filter(|k| k.1 == wonder && k.0 != city).copied().collect();
        for key in keys {
            let Some(core) = auth.cities().get(key.0) else { continue };
            let order = self.orders.get_mut(&key).expect("listed above");
            if order.lost.is_some() {
                continue;
            }
            let own = core.owner == builder;
            let loss = Loss { round: now, stock: core.production, head: order.head };
            order.lost = Some(loss);
            let tally = self.tally.get_mut(&order.owner.0).expect("seat");
            tally.lost += 1;
            tally.lost_own += u32::from(own);
            if loss.head {
                tally.lost_head += 1;
                tally.invested_at_loss += u64::from(loss.stock);
            }
        }
    }

    /// At the end of a round: production samples, and the age of knowledge at the checkpoints.
    pub fn after_round<R: BenchRules>(&mut self, game: &GameState<R::M>, rules: &CompiledRuleset<R::M>, checkpoints: &[u32], last: bool) {
        let auth = game.view().auth;
        let now = auth.turn().round;
        if now % SAMPLE_EVERY == 0 {
            for (seat, _) in self.seats.clone() {
                if auth.players().is_alive(seat) {
                    let production = R::economy(game, rules, seat).production.max(0) as u64;
                    *self.hammers.entry(seat.0).or_default() += production * u64::from(SAMPLE_EVERY);
                    add_window(self.hammers_by.entry(seat.0).or_default(), now, production * u64::from(SAMPLE_EVERY));
                }
            }
        }
        if checkpoints.contains(&now) || last {
            self.ages_at::<R>(game, rules, now);
        }
    }

    fn ages_at<R: BenchRules>(&mut self, game: &GameState<R::M>, rules: &CompiledRuleset<R::M>, now: u32) {
        let auth = game.view().auth;
        let players = auth.players();
        for (seat, _) in self.seats.clone() {
            if !players.is_alive(seat) {
                continue;
            }
            let view = PlayerView::new(auth, game.derived(), rules, seat);
            let Some(knowledge) = auth.knowledge().get(seat.0 as usize) else { continue };
            for (nation, _) in self.seats.clone() {
                if nation == seat || !players.is_alive(nation) {
                    continue;
                }
                let contact = view.contact(nation);
                if contact.met_round.is_none() {
                    continue;
                }
                let mut row = AgeRow {
                    round: now,
                    seat: seat.0,
                    nation: nation.0,
                    cities: 0,
                    seen: 0,
                    age_sum: 0,
                    unit_age: self.unit_seen.get(&(seat.0, nation.0)).map(|r| now.saturating_sub(*r)),
                    contact_age: contact.last_seen_round.map(|r| now.saturating_sub(r)),
                    units: auth.units().iter_cores().filter(|u| u.owner == nation && kernel::ai::plan::unit_power(rules, u.kind, u.hp) > 0).count() as u32,
                };
                for (_, city) in auth.cities().iter().filter(|(_, c)| c.owner == nation) {
                    row.cities += 1;
                    if is_visible(game.derived(), seat, city.tile) {
                        row.seen += 1;
                    } else if knowledge.sightings().any(|(t, _)| t == city.tile) {
                        row.seen += 1;
                        let seen = knowledge.memory(city.tile).map_or(now, |m| m.seen_round());
                        row.age_sum += u64::from(now.saturating_sub(seen));
                    }
                }
                self.ages.push(row);
            }
        }
    }

    /// The match is over: one line to the file.
    pub fn finish(mut self, label: &str, seed: u64, candidate: &str, baseline: &str, rounds: u32) {
        for order in self.orders.values() {
            self.tally.get_mut(&order.owner.0).expect("seat").open += 1;
        }
        let seats = self
            .seats
            .iter()
            .map(|(p, side)| SeatLine {
                player: p.0,
                side: if *side == Side::Candidate { "candidate" } else { "baseline" },
                wonders: self.tally.get(&p.0).cloned().unwrap_or_default(),
                hammers: self.hammers.get(&p.0).copied().unwrap_or(0),
                hammers_by: self.hammers_by.get(&p.0).cloned().unwrap_or_default(),
                burned_by: self.burned_by.get(&p.0).cloned().unwrap_or_default(),
            })
            .collect();
        let line = Line { label, seed, candidate, baseline, rounds, seats, completions: self.completions, ages: self.ages, blows: self.blows, orders: self.order_log };
        if let (Some(sink), Ok(text)) = (sink(), serde_json::to_string(&line)) {
            let mut file = sink.lock().expect("probe sink");
            let _ = writeln!(file, "{text}");
        }
    }
}
