//! The research pace of an AI version in whole games
//! (classic/docs/ai-science-pace.md): each game is played by one AI variant
//! on every seat of the pace suite's maps (Small, Prince, 500 rounds, the
//! time victory at the end). Per checkpoint and seat: cities, citizens,
//! technologies, the spaceship's technologies (`grand_strategy::space_closure`),
//! science a turn and where it comes from; the policy branches in the order
//! they were unlocked; the science buildings built (each building with a
//! science effect, read from the rules) and, for one with a condition on the
//! city's site, how many cities meet it; the trade routes running (internal
//! or international, land or sea, what they carry); what the great scientists
//! did (a technology or their improvement). Then the science victories by
//! rounds 300, 400 and 500. Deterministic: the same options print the same
//! report.
//!
//! `cargo run --release --offline -p aurelia-qa --bin ai-science-pace -- [options]`
//!
//! --variant X          the AI on every seat (default standard)
//! --rounds N           the last round, the time victory (default 500)
//! --seeds A..B         the seeds, B excluded (default 1..9); or one seed
//! --scripts a,b        the map scripts (default the five of the pace suite)
//! --size NAME          the world size (default small)
//! --jobs N             games in parallel (default: the machine's threads)
//! --speed NAME         the game speed (default Standard); the rounds and the
//!                      checkpoints scale with its pace (Quick: × 0.67)
//! --ideal              the science audit's ideal case (classic/docs/ai-science-audit.md):
//!                      two nations on the continents script, one per
//!                      continent where the starts allow, no barbarians,
//!                      four city-states, the rich land (`LandOptions::RICH`:
//!                      mountains, hills, forest, rivers, resources), only
//!                      the science and time victories; the scripts option
//!                      is ignored
//!
//! Science sources of a city, in this order: its citizens (the rules'
//! science per citizen), its worked tiles, its specialists, its buildings and
//! the other city effects (flat and percent: the rest of the city's rate),
//! its trade routes (added after the percents), the research process of an
//! empty queue; the empire's own percents (policies' empire effects,
//! city-state allies) are the player's science less its cities' sum.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use aurelia_qa::bench::modules::{majors, BenchRules, ClassicBench};
use aurelia_qa::bench::stats::{BlockEstimate, BlockSum};
use aurelia_qa::bench::suite::MapSpec;
use aurelia_qa::spec::victory_name;
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::variant::Variant;
use classic_rules::classic::Classic;
use classic_rules::derived::great_people::GreatPersonAction;
use classic_rules::ids::{BuildingKindId, TechId};
use classic_rules::rules::effects::{Modifier, YieldKind};
use classic_rules::rules::stats::TradeDomain;
use classic_rules::world::trade::RouteKind;
use classic_rules::world::player::ClassicPlayers;
use classic_rules::{ClassicGame, Command, CompiledRuleset};
use kernel::ai::driver::AiDriver;
use kernel::ai::Advance;
use kernel::ids::PlayerId;
use kernel::world::state::GameState;

const SCRIPTS: [&str; 5] = ["continents", "pangaea", "fractal", "archipelago", "small-continents"];
const CHECKPOINTS: [u32; 5] = [100, 200, 300, 400, 500];
const BY_ROUNDS: [u32; 3] = [300, 400, 500];

struct Options {
    variant: Variant,
    rounds: u32,
    seeds: std::ops::Range<u64>,
    scripts: Vec<&'static str>,
    size: &'static str,
    jobs: usize,
    /// The game speed's key or name (empty: Standard).
    speed: String,
    /// The speed's pace percent (Standard 100).
    pace: u32,
    ideal: bool,
}

impl Options {
    /// A Standard round at this speed.
    fn scaled(&self, round: u32) -> u32 {
        round * self.pace / 100
    }

    fn checkpoints(&self) -> Vec<u32> {
        CHECKPOINTS.iter().map(|r| self.scaled(*r)).collect()
    }

    fn by_rounds(&self) -> Vec<u32> {
        BY_ROUNDS.iter().map(|r| self.scaled(*r)).collect()
    }
}

fn options() -> Result<Options, String> {
    let mut options = Options {
        variant: Variant::Standard,
        rounds: 500,
        seeds: 1..9,
        scripts: SCRIPTS.to_vec(),
        size: "small",
        jobs: std::thread::available_parallelism().map_or(1, |n| n.get()),
        speed: String::new(),
        pace: 100,
        ideal: false,
    };
    let mut rounds_given = false;
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--variant" => {
                let name = value()?;
                options.variant = Variant::from_name(&name).ok_or_else(|| format!("no variant {name}"))?;
            }
            "--rounds" => {
                options.rounds = value()?.parse().map_err(|e| format!("--rounds: {e}"))?;
                rounds_given = true;
            }
            "--speed" => options.speed = value()?,
            "--ideal" => options.ideal = true,
            "--seeds" => {
                let text = value()?;
                options.seeds = match text.split_once("..") {
                    Some((a, b)) => a.parse().map_err(|e| format!("--seeds: {e}"))?..b.parse().map_err(|e| format!("--seeds: {e}"))?,
                    None => {
                        let seed: u64 = text.parse().map_err(|e| format!("--seeds: {e}"))?;
                        seed..seed + 1
                    }
                };
            }
            "--scripts" => {
                let text = value()?;
                options.scripts = text
                    .split(',')
                    .map(|name| SCRIPTS.into_iter().find(|s| *s == name).ok_or_else(|| format!("unknown script {name}")))
                    .collect::<Result<_, _>>()?;
            }
            "--size" => {
                let name = value()?;
                options.size = ["duel", "tiny", "small", "standard", "large", "huge"].into_iter().find(|s| *s == name).ok_or_else(|| format!("unknown size {name}"))?;
            }
            "--jobs" => options.jobs = value()?.parse().map_err(|e| format!("--jobs: {e}"))?,
            other => return Err(format!("unknown option {other}")),
        }
    }
    if !options.speed.is_empty() {
        let rules = ClassicBench::rules();
        let speed = rules.speed_named(&options.speed).ok_or_else(|| format!("unknown speed {}", options.speed))?;
        options.pace = speed.values.pace_percent;
        if !rounds_given {
            options.rounds = speed.values.max_rounds();
        }
    }
    if options.ideal {
        options.scripts = vec!["continents"];
    }
    Ok(options)
}

/// Science a turn by source (whole beakers).
#[derive(Debug, Clone, Copy, Default)]
struct Sources {
    citizens: i64,
    tiles: i64,
    specialists: i64,
    city_effects: i64,
    trade: i64,
    process: i64,
    empire: i64,
    total: i64,
}

/// The trade routes a seat runs.
#[derive(Debug, Clone, Copy, Default)]
struct Routes {
    internal_land: u32,
    internal_sea: u32,
    international_land: u32,
    international_sea: u32,
    to_city_states: u32,
    /// Hundredths a turn, both ends summed.
    food: u64,
    production: u64,
    gold: u64,
    science: u64,
}

/// One science building in a seat's cities.
#[derive(Debug, Clone, Copy, Default)]
struct BuildingCount {
    /// Cities with it.
    built: u32,
    /// Cities of a player that knows its technology.
    open: u32,
    /// Of those, cities whose site meets its placement (all when it has none).
    site: u32,
}

/// A seat at a checkpoint.
#[derive(Debug, Clone, Default)]
struct SeatAt {
    alive: bool,
    cities: u32,
    citizens: u32,
    techs: u32,
    space_techs: u32,
    excess: i32,
    sources: Sources,
    branches: Vec<u16>,
    buildings: BTreeMap<u16, BuildingCount>,
    routes: Routes,
}

/// A great scientist's end.
#[derive(Debug, Clone, Copy, Default)]
struct Scientists {
    bulbs: u32,
    academies: u32,
}

/// A seat's record: its checkpoints (round → state), the branches it
/// unlocked with their round, what its great scientists did.
type SeatRecord = (PlayerId, BTreeMap<u32, SeatAt>, Vec<(u16, u32)>, Scientists);

#[derive(Debug, Clone)]
struct GameResult {
    seed: u64,
    /// Per seat: checkpoints (round → state), the branches with their round.
    seats: Vec<SeatRecord>,
    outcome: Option<(PlayerId, &'static str, u32)>,
    /// Round each seat reached every technology of the spaceship.
    space_round: Vec<Option<u32>>,
    /// The first two seats' capitals stand on one landmass (round 1).
    shared_land: bool,
    /// Per seat, the opening at the Standard round `OPENING_ROUND`: the
    /// explored share of the map (per mille), majors met, city-states met,
    /// ruins claimed, the round of its first recon unit (0: none yet).
    opening: Vec<[u32; 5]>,
}

/// The opening's checkpoint, a Standard round (scaled by the speed).
const OPENING_ROUND: u32 = 30;

fn main() {
    let options = match options() {
        Ok(options) => options,
        Err(error) => {
            eprintln!("ai-science-pace: {error}");
            std::process::exit(1);
        }
    };
    let games: Vec<(&'static str, u64)> = options.scripts.iter().flat_map(|s| options.seeds.clone().map(move |seed| (*s, seed))).collect();
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<GameResult>>> = Mutex::new(vec![None; games.len()]);
    std::thread::scope(|scope| {
        for _ in 0..options.jobs.clamp(1, games.len().max(1)) {
            scope.spawn(|| {
                let rules = ClassicBench::rules();
                loop {
                    let at = next.fetch_add(1, Ordering::Relaxed);
                    let Some(&(script, seed)) = games.get(at) else { break };
                    let result = play(&rules, &options, script, seed);
                    eprintln!("ai-science-pace: {script}/{seed} done");
                    results.lock().expect("no thread panicked holding the results")[at] = Some(result);
                }
            });
        }
    });
    let results: Vec<GameResult> = results.into_inner().expect("no thread panicked").into_iter().map(|r| r.expect("every game played")).collect();
    let rules = ClassicBench::rules();
    print!("{}", report(&rules, &options, &results));
}

fn round(game: &GameState<Classic>) -> u32 {
    game.view().auth.turn().round
}

/// The buildings with a science effect (not world wonders), by the rules'
/// effects.
pub fn science_buildings(rules: &CompiledRuleset) -> Vec<BuildingKindId> {
    let science = |m: &Modifier| match m {
        Modifier::AddYield { yield_kind, .. } | Modifier::AddYieldPercent { yield_kind, .. } | Modifier::AddYieldPerPopulation { yield_kind, .. } => *yield_kind == YieldKind::Science,
        _ => false,
    };
    rules
        .buildings
        .iter()
        .enumerate()
        .filter(|(_, d)| !d.wonder && d.nation.is_none() && !d.unbuildable)
        .filter(|(_, d)| d.effects.passive_city.iter().chain(&d.effects.passive_tile).chain(&d.effects.passive_player).any(|e| science(&e.modifier)))
        .map(|(i, _)| BuildingKindId(i as u16))
        .collect()
}

/// Whether a city's site meets a building's placement, for the conditions
/// on the land around it (a mountain next to it or in reach, a river, the
/// coast, flat ground); the others count as met.
fn site_meets(game: &GameState<Classic>, rules: &CompiledRuleset, tile: kernel::ids::TileId, kind: BuildingKindId) -> bool {
    let p = &rules.buildings[kind.0 as usize].placement;
    let map = game.view().auth.map();
    let kinds = map.terrain_kinds();
    let mountain = |t| map.terrain(t).is_some_and(|c| kinds.is_mountains(c.0));
    let next = map.dimensions.neighbors(tile);
    if p.mountain && !next.as_slice().iter().any(|t| mountain(*t)) {
        return false;
    }
    if p.nearby_mountain && !classic_rules::math::hex_disc(map.dimensions, tile, 3).any(mountain) {
        return false;
    }
    if p.river && !map.river(tile) {
        return false;
    }
    if p.water && !next.as_slice().iter().any(|t| map.is_water(*t)) {
        return false;
    }
    !(p.flat && map.terrain(tile).is_some_and(|c| kinds.is_hills(c.0)))
}

fn seat_at(game: &GameState<Classic>, rules: &CompiledRuleset, player: PlayerId, buildings: &[BuildingKindId], space: &[TechId]) -> SeatAt {
    use classic_rules::derived::income::{city_income, player_income};
    let auth = game.view().auth;
    let mut out = SeatAt { alive: auth.players().is_alive(player), ..Default::default() };
    if !out.alive {
        return out;
    }
    let techs = auth.players().techs_of(player);
    out.techs = techs.len() as u32;
    out.space_techs = space.iter().filter(|t| techs.contains(t)).count() as u32;
    out.excess = classic_rules::derived::happiness::excess_snapshot(auth, player);
    out.branches = auth.module().culture().of(player).branches.iter().map(|b| b.0).collect();
    let mut cities_science = 0i64;
    for handle in auth.indexes().cities_of(player) {
        let Some(core) = auth.cities().get(*handle) else { continue };
        out.cities += 1;
        out.citizens += core.population;
        let breakdown = classic_rules::derived::yields::city_breakdown(auth, rules, core);
        let income = city_income(auth, rules, core);
        let trade = i64::from(classic_rules::derived::trade::city_trade_yields(auth, *handle).science);
        let citizens = i64::from(classic_rules::classic::yields::SCIENCE_PER_CITIZEN) * i64::from(core.population);
        let tiles = i64::from(breakdown.work.tile_yields.science);
        let specialists = i64::from(breakdown.work.specialist_yields.science);
        let rate = i64::from(breakdown.rates.science);
        let s = &mut out.sources;
        s.citizens += citizens;
        s.tiles += tiles;
        s.specialists += specialists;
        s.trade += trade;
        s.city_effects += rate - trade - citizens - tiles - specialists;
        s.process += i64::from(income.science) - rate;
        cities_science += i64::from(income.science);
        for kind in buildings {
            let def = &rules.buildings[kind.0 as usize];
            let open = def.required_tech.is_none_or(|t| techs.contains(&t));
            let count = out.buildings.entry(kind.0).or_default();
            if core.buildings.contains(kind) {
                count.built += 1;
            }
            if open {
                count.open += 1;
                if site_meets(game, rules, core.tile, *kind) {
                    count.site += 1;
                }
            }
        }
    }
    let total = i64::from(player_income(auth, rules, player).science);
    out.sources.empire = total - cities_science;
    out.sources.total = total;
    for route in auth.module().trade().owned_by(player) {
        let r = &mut out.routes;
        let destination = auth.cities().get(route.destination).map(|c| c.owner);
        let sea = route.domain == TradeDomain::Sea;
        match route.kind {
            RouteKind::International => {
                if sea {
                    r.international_sea += 1;
                } else {
                    r.international_land += 1;
                }
                if destination.is_some_and(|o| auth.players().is_city_state(o)) {
                    r.to_city_states += 1;
                }
            }
            _ if sea => r.internal_sea += 1,
            _ => r.internal_land += 1,
        }
        for y in [route.origin_yields, route.destination_yields] {
            r.food += u64::from(y.food);
            r.production += u64::from(y.production);
            r.gold += u64::from(y.gold);
            r.science += u64::from(y.science);
        }
    }
    out
}

fn play(rules: &CompiledRuleset, options: &Options, script: &'static str, seed: u64) -> GameResult {
    let map = MapSpec { script, size: options.size };
    let mut game = if options.ideal { ideal_game(rules, options, seed) } else { ClassicBench::new_game_at(rules, &map, seed, options.rounds, &options.speed) };
    let mut driver = AiDriver::with_mind(ClassicBench::settings(rules), Vec::new(), ClassicMind::new(options.variant));
    driver.set_trace_capacity(0);
    driver.record_journal();
    driver.configure_game(&mut game);
    let seats = majors(&game);
    let buildings = science_buildings(rules);
    let space: Vec<TechId> = classic_rules::ai::grand_strategy::space_closure(rules).into_iter().collect();
    let scientist = |kind: classic_rules::ids::UnitKindId| rules.units.get(kind.0 as usize).is_some_and(|u| u.module.economy.beakers_turns > 0);
    let mut checkpoints: Vec<BTreeMap<u32, SeatAt>> = vec![BTreeMap::new(); seats.len()];
    let mut unlocked: Vec<Vec<(u16, u32)>> = vec![Vec::new(); seats.len()];
    let mut scientists = vec![Scientists::default(); seats.len()];
    let mut space_round: Vec<Option<u32>> = vec![None; seats.len()];
    let checkpoints_at = options.checkpoints();
    let mut shared_land = false;
    let mut opening = vec![[0u32; 5]; seats.len()];
    let mut ruins = vec![0u32; seats.len()];
    let recon = |kind: classic_rules::ids::UnitKindId| rules.units.get(kind.0 as usize).and_then(|d| d.role) == Some(classic_rules::rules::content::UnitRole::Recon);
    while !game.is_over() && round(&game) <= options.rounds {
        loop {
            match driver.advance(&game, rules) {
                Advance::Worked => {}
                Advance::RoundDone | Advance::Waiting => break,
                Advance::Command(command) | Advance::EndTurn(command) => {
                    let used = match &command {
                        Command::GreatPersonAction { unit, action } => {
                            let core = game.view().auth.units().get(*unit).map(|u| (u.core.kind, u.core.owner));
                            core.filter(|(kind, _)| scientist(*kind)).and_then(|(_, owner)| seats.iter().position(|p| *p == owner)).map(|seat| (seat, *action))
                        }
                        _ => None,
                    };
                    let result = game.execute(rules, command);
                    let applied = result.is_ok();
                    driver.executed(result);
                    for done in driver.take_journal() {
                        for change in &done.changes.module_changes {
                            if let classic_rules::classic::ClassicChange::RuinsExplored { player, .. } = change {
                                if let Some(seat) = seats.iter().position(|p| p == player) {
                                    ruins[seat] += 1;
                                }
                            }
                        }
                    }
                    if let Some((seat, action)) = used.filter(|_| applied) {
                        match action {
                            GreatPersonAction::DiscoverTechnology => scientists[seat].bulbs += 1,
                            GreatPersonAction::Construct(_) => scientists[seat].academies += 1,
                            _ => {}
                        }
                    }
                }
            }
        }
        let now = round(&game);
        let auth = game.view().auth;
        if now == 1 {
            shared_land = capitals_share_land(&game, &seats);
        }
        for (index, player) in seats.iter().enumerate() {
            if opening[index][4] == 0 && auth.indexes().units_of(*player).iter().any(|h| auth.units().get(*h).is_some_and(|u| recon(u.core.kind))) {
                opening[index][4] = now;
            }
        }
        if now == options.scaled(OPENING_ROUND) {
            for (index, player) in seats.iter().enumerate() {
                opening[index] = opening_at(&game, *player, ruins[index], opening[index][4]);
            }
        }
        for (index, player) in seats.iter().enumerate() {
            for branch in &auth.module().culture().of(*player).branches {
                if !unlocked[index].iter().any(|(b, _)| *b == branch.0) {
                    unlocked[index].push((branch.0, now));
                }
            }
            if space_round[index].is_none() {
                let techs = auth.players().techs_of(*player);
                if space.iter().all(|t| techs.contains(t)) {
                    space_round[index] = Some(now);
                }
            }
        }
        if checkpoints_at.contains(&now) {
            for (index, player) in seats.iter().enumerate() {
                checkpoints[index].insert(now, seat_at(&game, rules, *player, &buildings, &space));
            }
        }
    }
    let end = round(&game).min(options.rounds);
    if !checkpoints_at.contains(&end) || game.is_over() {
        for (index, player) in seats.iter().enumerate() {
            checkpoints[index].entry(end).or_insert_with(|| seat_at(&game, rules, *player, &buildings, &space));
        }
    }
    let outcome = game.outcome().map(|o| (o.winner, victory_name(o.kind), o.round));
    GameResult {
        seed,
        seats: seats.iter().enumerate().map(|(i, p)| (*p, checkpoints[i].clone(), unlocked[i].clone(), scientists[i])).collect(),
        outcome,
        space_round,
        shared_land,
        opening,
    }
}

/// A seat's opening now (`GameResult::opening`).
fn opening_at(game: &GameState<Classic>, player: PlayerId, ruins: u32, recon_round: u32) -> [u32; 5] {
    use classic_rules::world::player::ClassicKind;
    use kernel::world::player::PlayerKind;
    let auth = game.view().auth;
    let tiles = auth.map().dimensions.tile_count() as u32;
    let k = &auth.knowledge()[player.0 as usize];
    let explored = (0..tiles).filter(|t| k.is_explored(kernel::ids::TileId(*t))).count() as u32;
    let (mut majors, mut minors) = (0, 0);
    for q in 0..auth.players().count() as u8 {
        let q = PlayerId(q);
        if q == player || k.contact(q).met_round.is_none() {
            continue;
        }
        match auth.players().kind_of(q) {
            PlayerKind::Major => majors += 1,
            PlayerKind::Module(ClassicKind::CityState(_)) => minors += 1,
            _ => {}
        }
    }
    [explored * 1000 / tiles.max(1), majors, minors, ruins, recon_round]
}

/// The ideal case's game (`--ideal`): Small continents, two nations, four
/// city-states, no barbarians, the rich land, Prince, the time victory at
/// the last round.
fn ideal_game(rules: &CompiledRuleset, options: &Options, seed: u64) -> GameState<Classic> {
    use classic_rules::scenario::{LandOptions, MapScript};
    let size = aurelia_qa::spec::size_from_name(options.size).expect("a known size");
    let mut spec = aurelia_qa::spec::GameSpec::new(MapScript::Continents, size, seed).max_rounds(options.rounds).speed(&options.speed).only_victory(classic_rules::world::victory::VictoryKind::Science);
    spec.nations = 2;
    spec.city_states = 4;
    spec.barbarians = classic_rules::world::barbarians::BarbarianMode::None;
    let (config, new_game) = spec.scenario(rules);
    classic_rules::scenario::generate_new_game_on(rules, &config, None, new_game, LandOptions::RICH)
}

/// The first two seats' first cities stand on one landmass (land tiles
/// joined by land; None of a city counts as not shared).
fn capitals_share_land(game: &GameState<Classic>, seats: &[PlayerId]) -> bool {
    let auth = game.view().auth;
    let capital = |p: PlayerId| auth.indexes().cities_of(p).first().and_then(|c| auth.cities().get(*c)).map(|c| c.tile);
    let (Some(a), Some(b)) = (seats.first().and_then(|p| capital(*p)), seats.get(1).and_then(|p| capital(*p))) else {
        return false;
    };
    let map = auth.map();
    let mut seen = std::collections::HashSet::from([a]);
    let mut stack = vec![a];
    while let Some(t) = stack.pop() {
        if t == b {
            return true;
        }
        for n in map.dimensions.neighbors(t).as_slice() {
            if !map.is_water(*n) && seen.insert(*n) {
                stack.push(*n);
            }
        }
    }
    false
}

/// The per-game value over seed blocks (games without a value left out).
fn blocks(results: &[GameResult], value: impl Fn(&GameResult) -> Option<f64>) -> BlockEstimate {
    let mut sums: BTreeMap<u64, BlockSum> = BTreeMap::new();
    for g in results {
        if let Some(v) = value(g) {
            let block = sums.entry(g.seed).or_insert(BlockSum { total: 0.0, games: 0 });
            block.total += v;
            block.games += 1;
        }
    }
    BlockEstimate::of(&sums.into_values().collect::<Vec<_>>())
}

fn ci(e: &BlockEstimate, digits: usize) -> String {
    if e.blocks == 0 {
        return "—".to_owned();
    }
    format!("{:.*} ({:.*}–{:.*})", digits, e.mean, digits, e.low, digits, e.high)
}

fn pct(e: &BlockEstimate) -> String {
    if e.blocks == 0 {
        return "—".to_owned();
    }
    format!("{:.0} % ({:.0}–{:.0})", 100.0 * e.mean, 100.0 * e.low, 100.0 * e.high)
}

/// The mean over a game's living seats at `at`.
fn seat_mean(g: &GameResult, at: u32, f: &dyn Fn(&SeatAt) -> f64) -> Option<f64> {
    let values: Vec<f64> = g.seats.iter().filter_map(|(_, c, _, _)| c.get(&at)).filter(|s| s.alive).map(f).collect();
    (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
}

/// The seat with the most technologies at `at` (ties: the most science).
fn best_seat(g: &GameResult, at: u32) -> Option<&SeatAt> {
    g.seats.iter().filter_map(|(_, c, _, _)| c.get(&at)).filter(|s| s.alive).max_by_key(|s| (s.techs, s.sources.total))
}

fn report(rules: &CompiledRuleset, options: &Options, results: &[GameResult]) -> String {
    let mut out = String::new();
    let name = |b: u16| rules.module.policy_branches.get(b as usize).map_or_else(|| format!("#{b}"), |d| d.name.clone());
    let speed = if options.speed.is_empty() { "Standard".to_owned() } else { options.speed.clone() };
    let ideal = if options.ideal { ", the ideal case (2 nations, rich land, no barbarians)" } else { "" };
    let _ = writeln!(out, "## Research pace: `{}`, every seat, {} games ({} × seeds {}..{}), {} rounds, speed {speed}{ideal}\n", options.variant.name(), results.len(), options.scripts.join(", "), options.seeds.start, options.seeds.end, options.rounds);
    if options.ideal {
        let shared = results.iter().filter(|g| g.shared_land).count();
        let _ = writeln!(out, "The two capitals share a landmass in {shared} of {} games.\n", results.len());
    }
    let space = classic_rules::ai::grand_strategy::space_closure(rules).len();
    let _ = writeln!(out, "### Victories and the spaceship's {space} technologies\n");
    let _ = writeln!(out, "| measure | value (95 % CI over seed blocks) |\n|---|---|");
    for by in options.by_rounds().into_iter().filter(|r| *r <= options.rounds) {
        let share = blocks(results, |g| Some(f64::from(u8::from(g.outcome.is_some_and(|(_, k, r)| k == "science" && r <= by)))));
        let _ = writeln!(out, "| science victory by round {by} | {} |", pct(&share));
        let all = blocks(results, |g| Some(f64::from(u8::from(g.space_round.iter().any(|r| r.is_some_and(|r| r <= by))))));
        let _ = writeln!(out, "| a seat knows all {space} by round {by} | {} |", pct(&all));
    }
    let rounds = |v: Vec<u32>| {
        let mut v = v;
        v.sort_unstable();
        match (v.first(), v.get(v.len() / 2)) {
            (Some(first), Some(median)) => format!("median {median}, earliest {first} ({} games)", v.len()),
            _ => "—".to_owned(),
        }
    };
    let first = rounds(results.iter().filter_map(|g| g.space_round.iter().flatten().min().copied()).collect());
    let _ = writeln!(out, "| round the first seat knew all {space} | {first} |");
    let won = rounds(results.iter().filter_map(|g| g.outcome.filter(|(_, k, _)| *k == "science").map(|(_, _, r)| r)).collect());
    let _ = writeln!(out, "| round of the science victory | {won} |");
    let mut ends: BTreeMap<String, u32> = BTreeMap::new();
    for g in results {
        let key = g.outcome.map_or_else(|| "not over".to_owned(), |(_, k, _)| k.to_owned());
        *ends.entry(key).or_default() += 1;
    }
    let ends: Vec<String> = ends.into_iter().map(|(k, n)| format!("{k} {n}")).collect();
    let _ = writeln!(out, "| ends | {} |\n", ends.join(", "));
    let opening = |i: usize| blocks(results, |g| (!g.opening.is_empty()).then(|| g.opening.iter().map(|o| f64::from(o[i])).sum::<f64>() / g.opening.len() as f64));
    let first_recon: Vec<u32> = results.iter().flat_map(|g| g.opening.iter().map(|o| o[4])).filter(|r| *r > 0).collect();
    let mut sorted = first_recon.clone();
    sorted.sort_unstable();
    let _ = writeln!(
        out,
        "Opening at round {} (Standard {OPENING_ROUND}), mean seat: explored {} ‰ of the map, majors met {}, city-states met {}, ruins claimed {}; first recon unit: median round {} ({} of {} seats had one by the end)\n",
        options.scaled(OPENING_ROUND),
        ci(&opening(0), 0),
        ci(&opening(1), 2),
        ci(&opening(2), 2),
        ci(&opening(3), 2),
        sorted.get(sorted.len() / 2).map_or("—".to_owned(), |r| r.to_string()),
        first_recon.len(),
        results.iter().map(|g| g.opening.len()).sum::<usize>(),
    );

    let _ = writeln!(out, "### Per checkpoint (mean of the living seats / the best seat by technologies)\n");
    let _ = writeln!(out, "| round | cities | citizens | techs | best seat techs | space techs, best | science a turn | best seat science | best seat cities | excess happiness |");
    let _ = writeln!(out, "|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|");
    for at in options.checkpoints().into_iter().filter(|r| *r <= options.rounds) {
        let m = |f: &dyn Fn(&SeatAt) -> f64| blocks(results, |g| seat_mean(g, at, f));
        let b = |f: &dyn Fn(&SeatAt) -> f64| blocks(results, |g| best_seat(g, at).map(f));
        let _ = writeln!(
            out,
            "| {at} | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            ci(&m(&|s| f64::from(s.cities)), 1),
            ci(&m(&|s| f64::from(s.citizens)), 0),
            ci(&m(&|s| f64::from(s.techs)), 1),
            ci(&b(&|s| f64::from(s.techs)), 1),
            ci(&b(&|s| f64::from(s.space_techs)), 1),
            ci(&m(&|s| s.sources.total as f64), 0),
            ci(&b(&|s| s.sources.total as f64), 0),
            ci(&b(&|s| f64::from(s.cities)), 1),
            ci(&m(&|s| f64::from(s.excess)), 1),
        );
    }
    let _ = writeln!(out, "\n### Science sources, beakers a turn (mean of the living seats)\n");
    let _ = writeln!(out, "| round | total | citizens | tiles | specialists | buildings and city effects | trade routes | research process | empire percents |");
    let _ = writeln!(out, "|---:|---:|---:|---:|---:|---:|---:|---:|---:|");
    for at in options.checkpoints().into_iter().filter(|r| *r <= options.rounds) {
        let m = |f: &dyn Fn(&Sources) -> i64| blocks(results, |g| seat_mean(g, at, &|s| f(&s.sources) as f64)).mean;
        let _ = writeln!(
            out,
            "| {at} | {:.1} | {:.1} | {:.1} | {:.1} | {:.1} | {:.1} | {:.1} | {:.1} |",
            m(&|s| s.total),
            m(&|s| s.citizens),
            m(&|s| s.tiles),
            m(&|s| s.specialists),
            m(&|s| s.city_effects),
            m(&|s| s.trade),
            m(&|s| s.process),
            m(&|s| s.empire)
        );
    }

    let _ = writeln!(out, "\n### Policy branches: share of seats unlocked by the round, and the median round\n");
    let branches: Vec<u16> = (0..rules.module.policy_branches.len() as u16).collect();
    let _ = writeln!(out, "| branch | by (Standard) 100 | by 200 | by 300 | by 400 | by 500 | median round (seats that did) | first branch, seats |");
    let _ = writeln!(out, "|---|---:|---:|---:|---:|---:|---:|---:|");
    let seats: Vec<&Vec<(u16, u32)>> = results.iter().flat_map(|g| g.seats.iter().map(|(_, _, u, _)| u)).collect();
    for b in &branches {
        let rounds: Vec<u32> = seats.iter().filter_map(|u| u.iter().find(|(x, _)| x == b).map(|(_, r)| *r)).collect();
        if rounds.is_empty() {
            continue;
        }
        let share = |by: u32| 100.0 * rounds.iter().filter(|r| **r <= by).count() as f64 / seats.len().max(1) as f64;
        let mut sorted = rounds.clone();
        sorted.sort_unstable();
        let first = seats.iter().filter(|u| u.first().is_some_and(|(x, _)| x == b)).count();
        let _ = writeln!(out, "| {} | {:.0} % | {:.0} % | {:.0} % | {:.0} % | {:.0} % | {} | {first} |", name(*b), share(options.scaled(100)), share(options.scaled(200)), share(options.scaled(300)), share(options.scaled(400)), share(options.scaled(500)), sorted[sorted.len() / 2]);
    }
    let mut orders: BTreeMap<String, u32> = BTreeMap::new();
    for u in &seats {
        let key: Vec<String> = u.iter().take(3).map(|(b, _)| name(*b)).collect();
        *orders.entry(key.join(" → ")).or_default() += 1;
    }
    let mut orders: Vec<(String, u32)> = orders.into_iter().collect();
    orders.sort_by_key(|(k, n)| (std::cmp::Reverse(*n), k.clone()));
    let _ = writeln!(out, "\nThe first three branches, most common orders ({} seats): {}\n", seats.len(), orders.iter().take(6).map(|(k, n)| format!("{k} ({n})")).collect::<Vec<_>>().join("; "));

    let _ = writeln!(out, "### Science buildings (cities with it / cities whose owner knows its technology; the site condition met)\n");
    let _ = writeln!(out, "| building | site condition | at 200: built / open | at 300 | at 400 | at 500 | cities open whose site meets it, at 400 |");
    let _ = writeln!(out, "|---|---|---:|---:|---:|---:|---:|");
    for kind in science_buildings(rules) {
        let def = &rules.buildings[kind.0 as usize];
        let p = &def.placement;
        let mut cond = Vec::new();
        for (flag, label) in [(p.mountain, "mountain next to it"), (p.nearby_mountain, "mountain in reach"), (p.river, "river"), (p.water, "coast"), (p.fresh_water, "fresh water"), (p.flat, "flat")] {
            if flag {
                cond.push(label);
            }
        }
        let share = |at: u32, f: &dyn Fn(&BuildingCount) -> (u32, u32)| {
            let (mut a, mut b) = (0u32, 0u32);
            for g in results {
                for (_, c, _, _) in &g.seats {
                    if let Some(count) = c.get(&at).and_then(|s| s.buildings.get(&kind.0)) {
                        let (x, y) = f(count);
                        a += x;
                        b += y;
                    }
                }
            }
            if b == 0 {
                "—".to_owned()
            } else {
                format!("{:.0} %", 100.0 * f64::from(a) / f64::from(b))
            }
        };
        let built = |c: &BuildingCount| (c.built, c.open);
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} | {} | {} | {} |",
            def.name,
            if cond.is_empty() { "—".to_owned() } else { cond.join(", ") },
            share(options.scaled(200), &built),
            share(options.scaled(300), &built),
            share(options.scaled(400), &built),
            share(options.scaled(500), &built),
            share(options.scaled(400), &|c: &BuildingCount| (c.site, c.open))
        );
    }

    let _ = writeln!(out, "\n### Trade routes running (mean per living seat)\n");
    let _ = writeln!(out, "| round | internal land | internal sea | international land | international sea | of them to city-states | food a turn | production | gold | science |");
    let _ = writeln!(out, "|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|");
    for at in options.checkpoints().into_iter().filter(|r| *r <= options.rounds) {
        let m = |f: &dyn Fn(&Routes) -> f64| blocks(results, |g| seat_mean(g, at, &|s| f(&s.routes))).mean;
        let _ = writeln!(
            out,
            "| {at} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} | {:.1} | {:.1} | {:.1} | {:.1} |",
            m(&|r| f64::from(r.internal_land)),
            m(&|r| f64::from(r.internal_sea)),
            m(&|r| f64::from(r.international_land)),
            m(&|r| f64::from(r.international_sea)),
            m(&|r| f64::from(r.to_city_states)),
            m(&|r| r.food as f64 / 100.0),
            m(&|r| r.production as f64 / 100.0),
            m(&|r| r.gold as f64 / 100.0),
            m(&|r| r.science as f64 / 100.0)
        );
    }
    let all: Vec<Scientists> = results.iter().flat_map(|g| g.seats.iter().map(|(_, _, _, s)| *s)).collect();
    let bulbs: u32 = all.iter().map(|s| s.bulbs).sum();
    let academies: u32 = all.iter().map(|s| s.academies).sum();
    let _ = writeln!(out, "\n### Great scientists\n\n{} seats: {bulbs} used for a technology, {academies} for their improvement ({:.2} and {:.2} a seat).", all.len(), f64::from(bulbs) / all.len().max(1) as f64, f64::from(academies) / all.len().max(1) as f64);
    out
}
