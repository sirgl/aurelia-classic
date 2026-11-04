//! The forced finish of the spaceship (classic/docs/ai-science-victory.md):
//! each game is played by one AI variant on every seat to round `--at`;
//! then the nation with the most science a turn learns every technology the
//! spaceship needs (`grand_strategy::space_closure`, in the order of their
//! prerequisites) and the game goes on to `--rounds`. What remains is the
//! AI's own work: the prerequisite project (Apollo), the parts, their march
//! to the capital. Per game: the round the project was done, each part's
//! round and the city that built it (its population, production and rank
//! by production among the empire's cities), the round it was installed,
//! and how the game ended; then the share of games the leader won by
//! science by each round, with a bootstrap interval over the seeds (the
//! games of one seed share the nations: `bench::stats`). Deterministic: the
//! same options print the same report.
//!
//! `cargo run --release --offline -p aurelia-qa --bin ai-finish -- [options]`
//!
//! --variant X          the AI on every seat (default standard)
//! --at R               the round the technologies are given (default 300)
//! --rounds N           the last round, the time victory (default 500)
//! --seeds A..B         the seeds, B excluded (default 1..5); or one seed
//! --scripts a,b        the map scripts (default the five of the pace suite)
//! --size NAME          the world size (default small)
//! --jobs N             games in parallel (default: the machine's threads)

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
use classic_rules::ids::{ProductionItem, TechId, TileId, UnitHandle, UnitKindId};
use classic_rules::rules::content::ProjectId;
use classic_rules::rules::effects::{Condition, ProductionFilter};
use classic_rules::{ClassicGame, CompiledRuleset};
use kernel::ai::driver::AiDriver;
use kernel::ids::PlayerId;
use kernel::world::state::GameState;

/// The map scripts of the pace suite (`bench::suite`).
const SCRIPTS: [&str; 5] = ["continents", "pangaea", "fractal", "archipelago", "small-continents"];
/// The rounds a science victory is counted by.
const BY_ROUNDS: [u32; 3] = [350, 400, 500];

struct Options {
    variant: Variant,
    at: u32,
    rounds: u32,
    seeds: std::ops::Range<u64>,
    scripts: Vec<&'static str>,
    size: &'static str,
    jobs: usize,
}

fn options() -> Result<Options, String> {
    let mut options = Options {
        variant: Variant::Standard,
        at: 300,
        rounds: 500,
        seeds: 1..9,
        scripts: SCRIPTS.to_vec(),
        size: "small",
        jobs: std::thread::available_parallelism().map_or(1, |n| n.get()),
    };
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--variant" => {
                let name = value()?;
                options.variant = Variant::from_name(&name).ok_or_else(|| format!("no variant {name}"))?;
            }
            "--at" => options.at = value()?.parse().map_err(|e| format!("--at: {e}"))?,
            "--rounds" => options.rounds = value()?.parse().map_err(|e| format!("--rounds: {e}"))?,
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
                options.size = ["duel", "tiny", "small", "standard", "large", "huge"]
                    .into_iter()
                    .find(|s| *s == name)
                    .ok_or_else(|| format!("unknown size {name}"))?;
            }
            "--jobs" => options.jobs = value()?.parse().map_err(|e| format!("--jobs: {e}"))?,
            other => return Err(format!("unknown option {other}")),
        }
    }
    if options.at >= options.rounds {
        return Err("--at must come before --rounds".to_owned());
    }
    Ok(options)
}

/// One part as it was built and installed.
#[derive(Debug, Clone)]
struct Part {
    name: String,
    built: u32,
    /// The city that built it: population, production a turn, rank by
    /// production among the empire's cities (1: the most productive).
    population: u32,
    production: i32,
    rank: usize,
    installed: Option<u32>,
}

/// One game of the forced finish.
#[derive(Debug, Clone)]
struct Finish {
    label: String,
    seed: u64,
    leader: Option<PlayerId>,
    /// The leader's science a turn and its cities when it got the
    /// technologies.
    science: i32,
    cities: usize,
    /// The round every prerequisite project was done.
    project: Option<u32>,
    parts: Vec<Part>,
    /// The winner, the victory and its round (None: not over).
    outcome: Option<(PlayerId, &'static str, u32)>,
    rounds: u32,
    /// The leader's cities with a building that speeds the parts (a
    /// production percent toward them), at the end.
    speeding: usize,
}

impl Finish {
    fn leader_won_by_science(&self) -> Option<u32> {
        let (winner, kind, round) = self.outcome?;
        (Some(winner) == self.leader && kind == "science").then_some(round)
    }
}

fn main() {
    let options = match options() {
        Ok(options) => options,
        Err(error) => {
            eprintln!("ai-finish: {error}");
            std::process::exit(1);
        }
    };
    let games: Vec<(&'static str, u64)> = options.scripts.iter().flat_map(|s| options.seeds.clone().map(move |seed| (*s, seed))).collect();
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<Finish>>> = Mutex::new(vec![None; games.len()]);
    std::thread::scope(|scope| {
        for _ in 0..options.jobs.clamp(1, games.len().max(1)) {
            scope.spawn(|| {
                let rules = ClassicBench::rules();
                loop {
                    let at = next.fetch_add(1, Ordering::Relaxed);
                    let Some(&(script, seed)) = games.get(at) else { break };
                    let finish = play(&rules, &options, script, seed);
                    eprintln!("ai-finish: {} done", finish.label);
                    results.lock().expect("no thread panicked holding the results")[at] = Some(finish);
                }
            });
        }
    });
    let results: Vec<Finish> = results.into_inner().expect("no thread panicked").into_iter().map(|r| r.expect("every game played")).collect();
    print!("{}", report(&options, &results));
}

/// One game: played to `--at`, the leader given the spaceship's
/// technologies, played on to the end, the space work watched every round.
fn play(rules: &CompiledRuleset, options: &Options, script: &'static str, seed: u64) -> Finish {
    let map = MapSpec { script, size: options.size };
    let mut game = ClassicBench::new_game(rules, &map, seed, options.rounds);
    let mut driver = AiDriver::with_mind(ClassicBench::settings(rules), Vec::new(), ClassicMind::new(options.variant));
    driver.set_trace_capacity(0);
    let parts = classic_rules::ai::victory::part_kinds(rules);
    let prerequisites = classic_rules::ai::victory::prerequisite_projects(rules);
    let mut finish = Finish {
        label: format!("{script}/{}/{seed}", options.size),
        seed,
        leader: None,
        science: 0,
        cities: 0,
        project: None,
        parts: Vec::new(),
        outcome: None,
        rounds: 0,
        speeding: 0,
    };
    let mut seen: Vec<UnitHandle> = Vec::new();
    let mut heads: Vec<Head> = Vec::new();
    let mut installed: BTreeMap<u16, u16> = BTreeMap::new();
    while !game.is_over() && round(&game) <= options.rounds {
        let report = driver.run(&mut game, rules, u32::MAX);
        if !report.round_done && !game.is_over() {
            break;
        }
        let now = round(&game);
        if now == options.at {
            let leader = science_leader(&game, rules);
            if let Some(player) = leader {
                let obs = game.ai_observation(rules, player);
                finish.science = obs.economy.income.science;
                finish.cities = obs.own_cities.len();
                give_space_technologies(&mut game, rules, player);
            }
            finish.leader = leader;
        }
        let Some(player) = finish.leader else { continue };
        if !game.view().auth.players().is_alive(player) {
            continue;
        }
        let obs = game.ai_observation(rules, player);
        let done = |project: ProjectId| obs.economy.projects.get(project.0 as usize).copied().unwrap_or(0);
        if finish.project.is_none() && prerequisites.iter().all(|p| done(*p) > 0) {
            finish.project = Some(now);
        }
        let dims = obs.view.dimensions();
        for unit in &obs.own_units {
            if seen.contains(&unit.handle) || !parts.iter().any(|(kind, _, _)| *kind == unit.kind) {
                continue;
            }
            seen.push(unit.handle);
            let name = rules.units[unit.kind.0 as usize].name.clone();
            finish.parts.push(builder(&heads, unit.kind, |tile| dims.distance(tile, unit.tile).unwrap_or(i32::MAX), name, now));
        }
        // A part installed: the first of its kind built and not installed
        // yet (one built in the capital may be installed before it is seen).
        for (kind, project, _) in &parts {
            let count = done(*project);
            let before = installed.insert(project.0, count).unwrap_or(0);
            for _ in before..count {
                let name = &rules.units[kind.0 as usize].name;
                let at = match finish.parts.iter().position(|p| &p.name == name && p.installed.is_none()) {
                    Some(at) => at,
                    None => {
                        let capital = obs.view.own_capital().and_then(|c| obs.own_cities.iter().find(|o| o.handle == c)).map(|c| c.tile);
                        let distance = |tile| capital.and_then(|c| dims.distance(tile, c)).unwrap_or(i32::MAX);
                        finish.parts.push(builder(&heads, *kind, distance, name.clone(), now));
                        finish.parts.len() - 1
                    }
                };
                finish.parts[at].installed = Some(now);
            }
        }
        heads = space_heads(&obs, &parts);
    }
    finish.rounds = round(&game);
    if let Some(player) = finish.leader {
        let speeds = |b: &classic_rules::ids::BuildingKindId| {
            rules.buildings.get(b.0 as usize).is_some_and(|d| {
                d.effects.passive_city.iter().any(|e| matches!(e.condition, Condition::Producing(ProductionFilter::SpaceshipParts)))
            })
        };
        finish.speeding = game.ai_observation(rules, player).own_cities.iter().filter(|c| c.buildings.iter().any(speeds)).count();
    }
    finish.outcome = game.outcome().map(|o| (o.winner, victory_name(o.kind), o.round));
    finish
}

/// A city building a part at the head of its queue, as it was before the
/// round the part came out.
#[derive(Debug, Clone, Copy)]
struct Head {
    kind: UnitKindId,
    tile: TileId,
    population: u32,
    production: i32,
    rank: usize,
}

/// The cities with a part at the head of their queue, ranked by production.
fn space_heads(obs: &classic_rules::knowledge::observation::AIObservation<'_>, parts: &[(UnitKindId, ProjectId, u16)]) -> Vec<Head> {
    let mut by_production: Vec<i32> = obs.own_cities.iter().map(|c| c.income.production).collect();
    by_production.sort_unstable_by(|a, b| b.cmp(a));
    obs.own_cities
        .iter()
        .filter_map(|c| match c.queue.first() {
            Some(ProductionItem::Unit(kind)) if parts.iter().any(|(k, _, _)| k == kind) => Some(Head {
                kind: *kind,
                tile: c.tile,
                population: c.population,
                production: c.income.production,
                rank: by_production.iter().position(|p| *p == c.income.production).map_or(0, |at| at + 1),
            }),
            _ => None,
        })
        .collect()
}

/// The part of `kind` came out of the city that had it at the head of its
/// queue the round before, the nearest to where it is by `distance` (no
/// such city: unknown, zeros).
fn builder(heads: &[Head], kind: UnitKindId, distance: impl Fn(TileId) -> i32, name: String, now: u32) -> Part {
    let head = heads.iter().filter(|h| h.kind == kind).min_by_key(|h| distance(h.tile));
    Part {
        name,
        built: now,
        population: head.map_or(0, |h| h.population),
        production: head.map_or(0, |h| h.production),
        rank: head.map_or(0, |h| h.rank),
        installed: None,
    }
}

fn round(game: &GameState<Classic>) -> u32 {
    game.view().auth.turn().round
}

/// The living nation with the most science a turn (ties: the lower id).
fn science_leader(game: &GameState<Classic>, rules: &CompiledRuleset) -> Option<PlayerId> {
    let auth = game.view().auth;
    majors(game)
        .into_iter()
        .filter(|p| auth.players().is_alive(*p))
        .max_by_key(|p| (classic_rules::derived::income::player_income(auth, rules, *p).science, std::cmp::Reverse(p.0)))
}

/// Every technology of the spaceship's closure, each once its
/// prerequisites are known.
fn give_space_technologies(game: &mut GameState<Classic>, rules: &CompiledRuleset, player: PlayerId) {
    let mut closure: Vec<TechId> = classic_rules::ai::grand_strategy::space_closure(rules).into_iter().collect();
    closure.sort_by_key(|t| t.0);
    loop {
        let known = game.view().auth.players().techs_of(player).to_vec();
        let next = closure
            .iter()
            .copied()
            .find(|t| !known.contains(t) && rules.technologies[t.0 as usize].requires.iter().all(|r| known.contains(r)));
        match next {
            Some(tech) => game.debug_discover(rules, player, tech),
            None => break,
        }
    }
}

fn report(options: &Options, results: &[Finish]) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "## Forced finish: `{}`, the science leader given the spaceship's technologies at round {}, played to {}\n",
        options.variant.name(),
        options.at,
        options.rounds
    );
    let _ = writeln!(out, "| game | leader | science | cities | project | parts: built at round (city population / production / rank) → installed | end |");
    let _ = writeln!(out, "|---|---|---:|---:|---:|---|---|");
    for f in results {
        let parts: Vec<String> = f
            .parts
            .iter()
            .map(|p| {
                let installed = p.installed.map_or_else(|| "—".to_owned(), |r| r.to_string());
                format!("{} {} ({}/{}/#{}) → {}", p.name, p.built, p.population, p.production, p.rank, installed)
            })
            .collect();
        let end = match f.outcome {
            Some((winner, kind, round)) => format!("P{} {kind} {round}", winner.0),
            None => format!("not over at {}", f.rounds),
        };
        let leader = f.leader.map_or_else(|| "—".to_owned(), |p| format!("P{}", p.0));
        let project = f.project.map_or_else(|| "—".to_owned(), |r| r.to_string());
        let _ = writeln!(out, "| {} | {leader} | {} | {} | {project} | {} | {end} |", f.label, f.science, f.cities, parts.join("; "));
    }
    out.push('\n');
    let _ = writeln!(out, "| measure | value (95 % CI over seeds) |");
    let _ = writeln!(out, "|---|---|");
    for by in BY_ROUNDS.into_iter().filter(|r| *r <= options.rounds) {
        let share = blocks(results, |f| Some(f64::from(u8::from(f.leader_won_by_science().is_some_and(|r| r <= by)))));
        let _ = writeln!(out, "| leader won by science by round {by} | {} |", percent(&share));
    }
    let won = blocks(results, |f| f.leader_won_by_science().map(f64::from));
    let _ = writeln!(out, "| round of the leader's science victory (games it won) | {} |", rounds(&won));
    let project = blocks(results, |f| f.project.map(|r| f64::from(r - options.at)));
    let _ = writeln!(out, "| rounds from the technologies to the project | {} |", rounds(&project));
    let last = blocks(results, |f| f.parts.iter().map(|p| p.installed).collect::<Option<Vec<u32>>>().filter(|v| !v.is_empty()).and_then(|v| v.into_iter().max()).map(|r| f64::from(r - options.at)));
    let _ = writeln!(out, "| rounds from the technologies to the last part installed (games with every part installed) | {} |", rounds(&last));
    let parts: Vec<&Part> = results.iter().flat_map(|f| f.parts.iter()).collect();
    let weak = parts.iter().filter(|p| p.rank > 3).count();
    let _ = writeln!(out, "| parts built | {} |", parts.len());
    let _ = writeln!(out, "| parts built outside the three most productive cities | {} ({:.0} %) |", weak, 100.0 * weak as f64 / parts.len().max(1) as f64);
    let median = median(parts.iter().map(|p| p.production));
    let _ = writeln!(out, "| median production of a part's city | {median} |");
    let speeding = blocks(results, |f| f.leader.map(|_| f.speeding as f64));
    let _ = writeln!(out, "| leader's cities with a building that speeds the parts, at the end | {} |", rounds(&speeding));
    let mut ends: BTreeMap<String, u32> = BTreeMap::new();
    for f in results {
        let key = match f.outcome {
            Some((winner, kind, _)) if Some(winner) == f.leader => format!("leader: {kind}"),
            Some((_, kind, _)) => format!("another nation: {kind}"),
            None => "not over".to_owned(),
        };
        *ends.entry(key).or_default() += 1;
    }
    let ends: Vec<String> = ends.into_iter().map(|(k, n)| format!("{k} {n}")).collect();
    let _ = writeln!(out, "| ends ({} games) | {} |", results.len(), ends.join(", "));
    out
}

/// The per-game value over seed blocks (games without a value left out).
fn blocks(results: &[Finish], value: impl Fn(&Finish) -> Option<f64>) -> BlockEstimate {
    let mut sums: BTreeMap<u64, BlockSum> = BTreeMap::new();
    for f in results {
        if let Some(v) = value(f) {
            let block = sums.entry(f.seed).or_insert(BlockSum { total: 0.0, games: 0 });
            block.total += v;
            block.games += 1;
        }
    }
    BlockEstimate::of(&sums.into_values().collect::<Vec<_>>())
}

fn percent(e: &BlockEstimate) -> String {
    format!("{:.0} % ({:.0}–{:.0} %)", 100.0 * e.mean, 100.0 * e.low, 100.0 * e.high)
}

fn rounds(e: &BlockEstimate) -> String {
    if e.blocks == 0 {
        return "—".to_owned();
    }
    format!("{:.0} ({:.0}–{:.0})", e.mean, e.low, e.high)
}

fn median(values: impl Iterator<Item = i32>) -> String {
    let mut v: Vec<i32> = values.collect();
    if v.is_empty() {
        return "—".to_owned();
    }
    v.sort_unstable();
    v[v.len() / 2].to_string()
}
