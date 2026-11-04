//! Trains the site net (`classic_rules::ai::site_net`) on site datasets
//! (`ai-site`), writes its integer weights as generated Rust and a report:
//! how the net and the settling planner's own score (`heuristic_score`)
//! order the cities of the test games, and which features the net needs.
//!
//! `cargo run --release --offline -p aurelia-qa --bin ai-train-site -- --data FILE[,FILE..] [options]`
//!
//! --data FILE[,FILE..]   site CSV files (`ai-site --out`; required)
//! --out-weights FILE     the generated weights (crates/classic-rules/src/ai/site_weights.rs)
//! --report FILE          the markdown report
//! --heads SPEC           the heads, TARGET:LOSS[:WEIGHT] by commas (default logpop:huber);
//!                        targets: pop, logpop (ln(1 + population)), value, logvalue (the
//!                        yields at `improvements::yield_value`'s prices); losses: huber, rank
//! --huber X              the Huber loss's bend, target units (default 1)
//! --horizon N            the rounds after the founding of the outcome (25, 50, 100; default 50)
//! --window W             rounds of a choice window: the groups ranked (default 20)
//! --drop GROUP[,GROUP..] leave out feature groups (`GROUPS`)
//! --audit                permutation importance and the retraining without each group
//! --jobs J               trainings at once for --audit (default: the machine's threads)
//! --hidden H1,H2,..  --epochs N  --patience N  --lr X  --batch N  --seed N   (`nn::Config`;
//!                        defaults 64,32, 200, 12, 0.001, 256, 1)
//!
//! Rows: the cities the AI founded beyond its first (the capital is not the
//! settling planner's choice), with a state at the horizon; a city lost
//! before it counts as nothing (population 0, no yields: the site was
//! worth nothing to its owner); a city the game ended before the horizon
//! has no row (its outcome is unknown). The split is by game seed, never by
//! row: `game % 20` below 14 trains, 14..17 validates (early stopping),
//! 17..20 tests (the report). The same data and options give the same
//! weights, bit for bit.
//!
//! The settling planner's score of a site (`planner_settle`) is the
//! quality of its land plus distance terms, minus the travel; the net's
//! answer replaces the land part (`site_net::planner_score`). Its points a
//! citizen and its points at no citizen are fitted on the training rows
//! where the planner's score is legal: the linear map that gives the net's
//! answers the mean and the spread of the planner's land part
//! (`heuristic_score` less `planner_settle::distance_terms` of the row's
//! distances), so a citizen weighs against a turn of travel and a tile of
//! distance as much as the land's spread did (`ScoreMap`).

use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::Instant;

use aurelia_qa::nn::{self, Config, Head, Loss, Net, Quantised, Rng, Samples};
use aurelia_qa::site::{group_of, spearman, Agreement, CityState, Row, HORIZONS};
use classic_rules::ai::improvements::yield_value;
use classic_rules::ai::net::{forward, ACT};
use classic_rules::ai::planner_settle::distance_terms;
use classic_rules::ai::site::{FAR, SITE_FEATURES, SITE_NAMES};
use classic_rules::ai::site_net;
use classic_rules::rules::content::Yields;

/// The feature groups of the audit: a name and the prefixes or names of
/// its features (`ai::site`'s table).
const GROUPS: [(&str, &[&str]); 12] = [
    ("nature", &["nature_"]),
    ("potential", &["potential_"]),
    ("bonus_tiles", &["bonus_tiles_"]),
    ("bonus_city", &["bonus_city_"]),
    ("resources", &["resources_"]),
    ("terrain", &["terrain_"]),
    ("feature", &["feature_"]),
    ("centre", &["centre_"]),
    ("tiles", &["territory_tiles", "unknown_tiles", "foreign_tiles", "river_tiles", "lake_tiles", "natural_wonders"]),
    ("distances", &["own_city_distance", "foreign_city_distance"]),
    ("moment", &["own_cities", "round", "excess_happiness"]),
    ("heuristic", &["heuristic_score", "heuristic_legal"]),
];

/// The features of a group, by index.
fn group_features(group: &str) -> Vec<usize> {
    let (_, members) = GROUPS.iter().find(|(name, _)| *name == group).expect("a known group");
    (0..SITE_FEATURES).filter(|i| members.iter().any(|m| if m.ends_with('_') { SITE_NAMES[*i].starts_with(m) } else { SITE_NAMES[*i] == *m })).collect()
}

/// What a head fits, from the city's state at the horizon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Population,
    LogPopulation,
    Value,
    LogValue,
}

impl Target {
    fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "pop" => Target::Population,
            "logpop" => Target::LogPopulation,
            "value" => Target::Value,
            "logvalue" => Target::LogValue,
            _ => return None,
        })
    }

    fn name(self) -> &'static str {
        match self {
            Target::Population => "pop",
            Target::LogPopulation => "logpop",
            Target::Value => "value",
            Target::LogValue => "logvalue",
        }
    }

    /// The target of an outcome: population; its log; the yields' value / 100; its log.
    fn of(self, outcome: &Outcome) -> f32 {
        (match self {
            Target::Population => outcome.population,
            Target::LogPopulation => outcome.population.ln_1p(),
            Target::Value => outcome.value / 100.0,
            Target::LogValue => outcome.value.max(0.0).ln_1p(),
        }) as f32
    }
}

#[derive(Clone)]
struct Options {
    data: Vec<PathBuf>,
    out_weights: Option<PathBuf>,
    report: Option<PathBuf>,
    config: Config,
    /// The target of each head of `config.heads`.
    targets: Vec<Target>,
    horizon: u32,
    window: u32,
    drop: Vec<String>,
    audit: bool,
    jobs: usize,
}

fn heads(text: &str, huber: f32) -> Result<(Vec<Target>, Vec<Head>), String> {
    let mut targets = Vec::new();
    let mut heads = Vec::new();
    for spec in text.split(',') {
        let parts: Vec<&str> = spec.split(':').collect();
        let (target, loss, weight) = match parts[..] {
            [t, l] => (t, l, "1"),
            [t, l, w] => (t, l, w),
            _ => return Err(format!("--heads {spec}: TARGET:LOSS[:WEIGHT]")),
        };
        targets.push(Target::parse(target).ok_or_else(|| format!("--heads {spec}: unknown target {target}"))?);
        let loss = match loss {
            "huber" => Loss::Huber(huber),
            "rank" => Loss::Rank,
            _ => return Err(format!("--heads {spec}: unknown loss {loss}")),
        };
        heads.push(Head { loss, weight: weight.parse().map_err(|e| format!("--heads {spec}: {e}"))? });
    }
    Ok((targets, heads))
}

fn options() -> Result<Options, String> {
    let mut options = Options {
        data: Vec::new(),
        out_weights: None,
        report: None,
        config: Config { hidden: vec![64, 32], ..Config::default() },
        targets: Vec::new(),
        horizon: 50,
        window: 20,
        drop: Vec::new(),
        audit: false,
        jobs: std::thread::available_parallelism().map_or(1, |n| n.get()),
    };
    let (mut head_spec, mut huber) = ("logpop:huber".to_owned(), 1f32);
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{flag} needs a value"));
        if options.config.parse_flag(&flag, &mut value)? {
            continue;
        }
        match flag.as_str() {
            "--data" => options.data = value()?.split(',').map(PathBuf::from).collect(),
            "--out-weights" => options.out_weights = Some(PathBuf::from(value()?)),
            "--report" => options.report = Some(PathBuf::from(value()?)),
            "--heads" => head_spec = value()?,
            "--huber" => huber = value()?.parse().map_err(|e| format!("--huber: {e}"))?,
            "--horizon" => options.horizon = value()?.parse().map_err(|e| format!("--horizon: {e}"))?,
            "--window" => options.window = value()?.parse().map_err(|e| format!("--window: {e}"))?,
            "--drop" => options.drop = value()?.split(',').map(str::to_owned).collect(),
            "--audit" => options.audit = true,
            "--jobs" => options.jobs = value()?.parse().map_err(|e| format!("--jobs: {e}"))?,
            other => return Err(format!("unknown option {other}")),
        }
    }
    if options.data.is_empty() {
        return Err("--data FILE is required".to_owned());
    }
    if !HORIZONS.contains(&options.horizon) {
        return Err(format!("--horizon {}: one of {HORIZONS:?}", options.horizon));
    }
    if let Some(unknown) = options.drop.iter().find(|d| !GROUPS.iter().any(|(g, _)| g == d)) {
        return Err(format!("--drop {unknown}: groups are {}", GROUPS.map(|g| g.0).join(", ")));
    }
    (options.targets, options.config.heads) = heads(&head_spec, huber)?;
    Ok(options)
}

fn main() {
    if let Err(error) = options().and_then(|options| run(&options)) {
        eprintln!("ai-train-site: {error}");
        std::process::exit(1);
    }
}

/// A city's outcome at the horizon.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Outcome {
    population: f64,
    /// The gross yields at `improvements::yield_value`'s prices (food 12,
    /// production 8, gold 5, science 6, culture 6).
    value: f64,
}

impl Outcome {
    fn of(state: &CityState) -> Self {
        let yields = Yields { food: state.food, production: state.production, gold: state.gold, science: state.science, culture: state.culture as i32, module: 0 };
        Outcome { population: f64::from(state.population), value: f64::from(yield_value(&yields)) }
    }
}

/// A row of the training data: the founded city and its outcome.
struct Site {
    row: Row,
    outcome: Outcome,
    /// The choice window (`site::group_of`).
    group: u64,
}

/// The rows of a site CSV file (`site::header`); the count of all rows.
fn load(path: &PathBuf, rows: &mut Vec<Row>) -> Result<usize, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut lines = text.lines();
    let header: Vec<&str> = lines.next().ok_or_else(|| format!("{}: empty", path.display()))?.split(',').collect();
    let column = |name: &str| header.iter().position(|h| *h == name).ok_or_else(|| format!("{}: no column {name}", path.display()));
    let features: Vec<usize> = SITE_NAMES.iter().map(|n| column(n)).collect::<Result<_, _>>()?;
    let [game, player, tile, round, lost] = ["game", "player", "tile", "round", "lost_round"].map(column);
    let (game, player, tile, round, lost) = (game?, player?, tile?, round?, lost?);
    let state_names = ["population", "food", "production", "science", "gold", "culture", "worked", "buildings"];
    let states: Vec<Vec<usize>> =
        HORIZONS.iter().map(|h| state_names.iter().map(|n| column(&format!("{n}_{h}"))).collect::<Result<Vec<usize>, String>>()).collect::<Result<_, _>>()?;
    let mut total = 0;
    for (at, line) in lines.enumerate() {
        total += 1;
        let cells: Vec<&str> = line.split(',').collect();
        let bad = |c: usize| format!("{}:{}: bad {}", path.display(), at + 2, header[c]);
        let int = |c: usize| cells.get(c).and_then(|s| s.parse::<i64>().ok()).ok_or_else(|| bad(c));
        let maybe = |c: usize| -> Result<Option<i64>, String> {
            match cells.get(c) {
                Some(&"") => Ok(None),
                _ => int(c).map(Some),
            }
        };
        let mut row_features = [0i32; SITE_FEATURES];
        for (slot, c) in row_features.iter_mut().zip(&features) {
            *slot = int(*c)? as i32;
        }
        let mut row_states = [None; HORIZONS.len()];
        for (slot, columns) in row_states.iter_mut().zip(&states) {
            let values: Vec<Option<i64>> = columns.iter().map(|c| maybe(*c)).collect::<Result<_, _>>()?;
            if values.iter().all(Option::is_some) {
                let v: Vec<i64> = values.into_iter().flatten().collect();
                *slot = Some(CityState {
                    population: v[0] as u32,
                    food: v[1] as i32,
                    production: v[2] as i32,
                    science: v[3] as i32,
                    gold: v[4] as i32,
                    culture: v[5] as u32,
                    worked: v[6] as u32,
                    buildings: v[7] as u32,
                });
            }
        }
        rows.push(Row {
            game: int(game)? as u64,
            player: int(player)? as u8,
            tile: int(tile)? as u32,
            round: int(round)? as u32,
            features: row_features,
            states: row_states,
            lost_round: maybe(lost)?.map(|r| r as u32),
        });
    }
    Ok(total)
}

/// The rows the net learns from (the module docs): not a capital, an
/// outcome at the horizon (a city lost before it: nothing).
fn sites(rows: Vec<Row>, horizon: u32, window: u32) -> (Vec<Site>, [usize; 3]) {
    let (mut capitals, mut lost, mut unknown) = (0, 0, 0);
    let mut out = Vec::new();
    for row in rows {
        if row.feature("own_cities") == Some(0) {
            capitals += 1;
            continue;
        }
        let outcome = match (row.state_at(horizon), row.lost_round) {
            (Some(state), _) => Outcome::of(&state),
            (None, Some(l)) if l <= row.round + horizon => {
                lost += 1;
                Outcome { population: 0.0, value: 0.0 }
            }
            _ => {
                unknown += 1;
                continue;
            }
        };
        let group = group_of(&row, window);
        out.push(Site { row, outcome, group });
    }
    (out, [capitals, lost, unknown])
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    Train,
    Valid,
    Test,
}

fn part(game: u64) -> Part {
    match game % 20 {
        0..=13 => Part::Train,
        14..=16 => Part::Valid,
        _ => Part::Test,
    }
}

/// The net's inputs of a row: the kept features as f32.
fn inputs(site: &Site, kept: &[usize]) -> Vec<f32> {
    kept.iter().map(|i| site.row.features[*i] as f32).collect()
}

fn samples(sites: &[&Site], kept: &[usize], targets: &[Target]) -> Samples {
    let mut out = Samples::new(kept.len(), targets.len());
    for site in sites {
        let y: Vec<f32> = targets.iter().map(|t| t.of(&site.outcome)).collect();
        out.push(&inputs(site, kept), &y, site.group);
    }
    out
}

/// A predictor's agreement with the outcomes, ordered within the choice windows.
#[derive(Debug, Clone, Copy)]
struct Quality {
    population: Agreement,
    value: Agreement,
}

fn quality(sites: &[&Site], scores: &[f64]) -> Quality {
    let points = |outcome: fn(&Outcome) -> f64| -> Vec<(u64, f64, f64)> { sites.iter().zip(scores).map(|(s, x)| (s.group, *x, outcome(&s.outcome))).collect() };
    Quality { population: Agreement::of(&points(|o| o.population)), value: Agreement::of(&points(|o| o.value)) }
}

fn heuristic(sites: &[&Site]) -> Vec<f64> {
    sites.iter().map(|s| f64::from(s.row.feature("heuristic_score").unwrap_or(0))).collect()
}

/// The f32 net's head `k` on the rows.
fn f32_scores(net: &Net, sites: &[&Site], kept: &[usize], k: usize) -> Vec<f64> {
    sites.iter().map(|s| f64::from(net.forward(&inputs(s, kept))[k])).collect()
}

/// The f32 net's head `k` with the moment of the choice (`moment` group:
/// own cities, round, excess happiness) of each window's first row for
/// every row of the window: the candidates of one decision share it.
fn f32_scores_at_one_moment(net: &Net, sites: &[&Site], kept: &[usize], k: usize) -> Vec<f64> {
    let moment = group_features("moment");
    let mut first: std::collections::HashMap<u64, &Site> = std::collections::HashMap::new();
    for site in sites {
        let slot = first.entry(site.group).or_insert(site);
        if (site.row.round, site.row.tile) < (slot.row.round, slot.row.tile) {
            *slot = site;
        }
    }
    sites
        .iter()
        .map(|s| {
            let mut features = s.row.features;
            for i in &moment {
                features[*i] = first[&s.group].row.features[*i];
            }
            let x: Vec<f32> = kept.iter().map(|i| features[*i] as f32).collect();
            f64::from(net.forward(&x)[k])
        })
        .collect()
}

/// The integer net's head `k` on the rows (Q16 as f64).
fn int_scores(quantised: &Quantised, sites: &[&Site], kept: &[usize], k: usize) -> Vec<f64> {
    let layers = quantised.layers();
    let net = quantised.net(&layers);
    let mut out = vec![0i64; quantised.layers.last().map_or(0, |l| l.b.len())];
    sites
        .iter()
        .map(|s| {
            let features: Vec<i32> = kept.iter().map(|i| s.row.features[*i]).collect();
            forward(&net, &features, &mut out);
            out[k] as f64 / f64::from(1u32 << ACT)
        })
        .collect()
}

/// The linear map from the net's answer (citizens) to the planner's score
/// points (the module docs): points = `at_zero` + `per_citizen` × answer.
#[derive(Debug, Clone, Copy)]
struct ScoreMap {
    at_zero: i32,
    per_citizen: i32,
    rows: usize,
    /// The least-squares slope of the land part on the answer and their
    /// correlation, reported beside the map.
    regression: f64,
    correlation: f64,
}

/// The planner's land part of a row's `heuristic_score` (its distance
/// terms taken out).
fn land_part(site: &Site) -> f64 {
    let distance = |name: &str| site.row.feature(name).filter(|d| *d != FAR);
    let terms = distance_terms(distance("own_city_distance"), distance("foreign_city_distance"));
    f64::from(site.row.feature("heuristic_score").unwrap_or(0) - terms)
}

fn score_map(quantised: &Quantised, train: &[&Site], kept: &[usize]) -> ScoreMap {
    let legal: Vec<&Site> = train.iter().copied().filter(|s| s.row.feature("heuristic_legal") == Some(1)).collect();
    let answers = int_scores(quantised, &legal, kept, 0);
    let land: Vec<f64> = legal.iter().map(|s| land_part(s)).collect();
    let n = legal.len().max(1) as f64;
    let mean = |v: &[f64]| v.iter().sum::<f64>() / n;
    let (mx, my) = (mean(&answers), mean(&land));
    let sxx = answers.iter().map(|x| (x - mx).powi(2)).sum::<f64>();
    let syy = land.iter().map(|y| (y - my).powi(2)).sum::<f64>();
    let sxy = answers.iter().zip(&land).map(|(x, y)| (x - mx) * (y - my)).sum::<f64>();
    let per_citizen = (syy / sxx.max(f64::MIN_POSITIVE)).sqrt();
    ScoreMap {
        at_zero: (my - per_citizen * mx).round() as i32,
        per_citizen: per_citizen.round() as i32,
        rows: legal.len(),
        regression: sxy / sxx.max(f64::MIN_POSITIVE),
        correlation: sxy / (sxx * syy).sqrt().max(f64::MIN_POSITIVE),
    }
}

/// A trained net and what it scores on the validation and test rows.
struct Trained {
    net: Net,
    epochs: Vec<nn::Epoch>,
    seconds: f64,
    kept: Vec<usize>,
}

fn train_without(options: &Options, parts: [&[&Site]; 3], drop: &[&str]) -> Trained {
    let dropped: Vec<usize> = drop.iter().flat_map(|g| group_features(g)).collect();
    let kept: Vec<usize> = (0..SITE_FEATURES).filter(|i| !dropped.contains(i)).collect();
    let started = Instant::now();
    let (net, epochs) = nn::train(&samples(parts[0], &kept, &options.targets), &samples(parts[1], &kept, &options.targets), &options.config);
    Trained { net, epochs, seconds: started.elapsed().as_secs_f64(), kept }
}

fn run(options: &Options) -> Result<(), String> {
    let started = Instant::now();
    let mut rows = Vec::new();
    let mut total = 0;
    for path in &options.data {
        total += load(path, &mut rows)?;
    }
    let (sites, [capitals, lost, unknown]) = sites(rows, options.horizon, options.window);
    let of = |p: Part| sites.iter().filter(|s| part(s.row.game) == p).collect::<Vec<&Site>>();
    let (train_rows, valid_rows, test_rows) = (of(Part::Train), of(Part::Valid), of(Part::Test));
    let games = |set: &[&Site]| {
        let mut g: Vec<u64> = set.iter().map(|s| s.row.game).collect();
        g.dedup();
        g.sort_unstable();
        g.dedup();
        g.len()
    };
    let counts = format!(
        "{total} rows: {capitals} capitals left out, {lost} cities lost before +{} (outcome 0), {unknown} without an outcome (the game ended); \
         {} rows: train {} ({} games), validation {} ({}), test {} ({})",
        options.horizon,
        sites.len(),
        train_rows.len(),
        games(&train_rows),
        valid_rows.len(),
        games(&valid_rows),
        test_rows.len(),
        games(&test_rows)
    );
    println!("{counts}; loaded in {:.1} s", started.elapsed().as_secs_f64());
    if train_rows.is_empty() || valid_rows.is_empty() || test_rows.is_empty() {
        return Err("every part needs games (game % 20: 0..14 train, 14..17 validation, 17..20 test)".to_owned());
    }
    let parts = [&train_rows[..], &valid_rows[..], &test_rows[..]];
    let drop: Vec<&str> = options.drop.iter().map(String::as_str).collect();
    let trained = train_without(options, parts, &drop);
    let best_epoch = trained.epochs.iter().enumerate().min_by(|a, b| a.1.valid_loss.total_cmp(&b.1.valid_loss)).map_or(0, |(i, _)| i + 1);
    for (i, e) in trained.epochs.iter().enumerate() {
        println!("epoch {:3}: train {:.5}, validation {:.5}", i + 1, e.train_loss, e.valid_loss);
    }
    println!("trained in {:.1} s, best epoch {best_epoch}", trained.seconds);
    let quantised = nn::quantise(&trained.net);
    let kept = &trained.kept;

    let mut w = String::new();
    let _ = writeln!(w, "# Site net: the test games\n");
    let _ = writeln!(w, "{counts}. Split by the game's seed: `game % 20` < 14, 14..17, 17..20.\n");
    let c = &options.config;
    let head_names: Vec<String> = options.targets.iter().zip(&c.heads).map(|(t, h)| format!("{} ({:?}, ×{})", t.name(), h.loss, h.weight)).collect();
    let hidden: Vec<String> = c.hidden.iter().map(|h| h.to_string()).collect();
    let _ = writeln!(
        w,
        "Net: {} → {} → {} (ReLU; heads: {}), Adam lr {}, batch {}, seed {}; {} epochs, the best {best_epoch}, {:.0} s. \
         Integer weights: {} parameters, shifts {}.{}\n",
        kept.len(),
        hidden.join(" → "),
        c.heads.len(),
        head_names.join(", "),
        c.learning_rate,
        c.batch,
        c.seed,
        trained.epochs.len(),
        trained.seconds,
        quantised.parameters(),
        quantised.layers.iter().map(|l| l.shift.to_string()).collect::<Vec<_>>().join("/"),
        if drop.is_empty() { String::new() } else { format!(" Left out: {}.", drop.join(", ")) }
    );

    let horizon = options.horizon;
    let table = |w: &mut String, title: &str, set: &[&Site]| {
        let _ = writeln!(w, "## {title}: ordering within the choice windows (game, player, {} rounds)\n", options.window);
        let _ = writeln!(w, "| predictor | population +{horizon}: pairs | right | within Spearman | Spearman, all rows | yields' value +{horizon}: pairs | right | within Spearman |");
        let _ = writeln!(w, "|---|---:|---:|---:|---:|---:|---:|---:|");
        let mut predictors = vec![("heuristic_score".to_owned(), heuristic(set))];
        for (k, target) in options.targets.iter().enumerate() {
            predictors.push((format!("net f32, head {} ({})", k, target.name()), f32_scores(&trained.net, set, kept, k)));
            predictors.push((format!("net integer, head {} ({})", k, target.name()), int_scores(&quantised, set, kept, k)));
            predictors.push((format!("net f32, head {} ({}), one moment a window", k, target.name()), f32_scores_at_one_moment(&trained.net, set, kept, k)));
        }
        for (name, scores) in &predictors {
            let q = quality(set, scores);
            let (p, v) = (q.population, q.value);
            let _ = writeln!(
                w,
                "| {name} | {} | {:.1} % | {:.3} | {:.3} | {} | {:.1} % | {:.3} |",
                p.pairs,
                100.0 * p.concordant,
                p.within_spearman,
                p.spearman,
                v.pairs,
                100.0 * v.concordant,
                v.within_spearman
            );
        }
        let _ = writeln!(w);
    };
    table(&mut w, "Test", &test_rows);
    table(&mut w, "Validation", &valid_rows);

    // The integer net against the f32 one.
    let (f32_test, int_test) = (f32_scores(&trained.net, &test_rows, kept, 0), int_scores(&quantised, &test_rows, kept, 0));
    let gap = f32_test.iter().zip(&int_test).fold(0f64, |m, (a, b)| m.max((a - b).abs()));
    let order_kept = spearman(&f32_test, &int_test);
    let nanos = time_inference(&quantised, &test_rows, kept);
    let _ = writeln!(
        w,
        "Integer inference against f32, head 0 on the {} test rows: at most {gap:.5} apart (target units), Spearman {order_kept:.6}. \
         Integer inference: {nanos:.0} ns a site.\n",
        test_rows.len()
    );
    println!("integer inference: {nanos:.0} ns a site, at most {gap:.5} from f32");
    let map = score_map(&quantised, &train_rows, kept);
    let _ = writeln!(
        w,
        "The planner's score of the net's answer (`site_net::planner_score`): {} + {} points a citizen, fitted on the {} training rows with a \
         legal `heuristic_score` (the spread of the planner's land part over the spread of the answers; the least-squares slope would be {:.1}, \
         correlation {:.3}).\n",
        map.at_zero, map.per_citizen, map.rows, map.regression, map.correlation
    );
    println!("planner score: {} + {} a citizen ({} rows; least squares {:.1}, r {:.3})", map.at_zero, map.per_citizen, map.rows, map.regression, map.correlation);

    if options.audit {
        audit(&mut w, options, parts, &trained);
    }

    let _ = writeln!(w, "## Reproduce\n");
    let _ = writeln!(
        w,
        "```\ncargo +1.98.1 run --release --offline -p aurelia-qa --bin ai-train-site -- {} --out-weights crates/classic-rules/src/ai/site_weights.rs --report REPORT.md\n```",
        command(options)
    );
    let _ = writeln!(w, "\n<details><summary>Training log</summary>\n\n| epoch | train loss | validation loss |\n|---:|---:|---:|");
    for (i, e) in trained.epochs.iter().enumerate() {
        let _ = writeln!(w, "| {} | {:.5} | {:.5} |", i + 1, e.train_loss, e.valid_loss);
    }
    let _ = writeln!(w, "\n</details>");

    if let Some(path) = &options.report {
        std::fs::write(path, &w).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("report: {}", path.display());
    } else {
        println!("{w}");
    }
    if let Some(path) = &options.out_weights {
        if kept.len() != SITE_FEATURES {
            return Err("--out-weights: the game's net reads every feature (no --drop)".to_owned());
        }
        let source = weights_source(options, &quantised, &trained.net, &test_rows, &sites, &map);
        std::fs::write(path, source).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("weights: {}", path.display());
    }
    Ok(())
}

/// The training options as the command line takes them.
fn command(options: &Options) -> String {
    let c = &options.config;
    let data: Vec<String> = options.data.iter().map(|p| p.display().to_string()).collect();
    let heads: Vec<String> = options
        .targets
        .iter()
        .zip(&c.heads)
        .map(|(t, h)| {
            let loss = if h.loss == Loss::Rank { "rank" } else { "huber" };
            if h.weight == 1.0 { format!("{}:{loss}", t.name()) } else { format!("{}:{loss}:{}", t.name(), h.weight) }
        })
        .collect();
    let huber = c.heads.iter().find_map(|h| if let Loss::Huber(d) = h.loss { Some(d) } else { None }).unwrap_or(1.0);
    format!(
        "--data {} --heads {} --huber {huber} --horizon {} --window {} {} --seed {}",
        data.join(","),
        heads.join(","),
        options.horizon,
        options.window,
        c.flags(),
        c.seed
    )
}

/// Nanoseconds of one integer inference of the game's form
/// (`site_net::infer`), the mean over repeated passes on the rows.
fn time_inference(quantised: &Quantised, sites: &[&Site], kept: &[usize]) -> f64 {
    let layers = quantised.layers();
    let net = quantised.net(&layers);
    let rows: Vec<Vec<i32>> = sites.iter().map(|s| kept.iter().map(|i| s.row.features[*i]).collect()).collect();
    let mut calls = 0u64;
    let mut sink = 0i64;
    let started = Instant::now();
    while started.elapsed().as_millis() < 500 {
        for row in &rows {
            sink = sink.wrapping_add(i64::from(site_net::infer(&net, std::hint::black_box(row))));
            calls += 1;
        }
    }
    std::hint::black_box(sink);
    started.elapsed().as_nanos() as f64 / calls as f64
}

/// The blocks of groups the audit also retrains without (`-`) or with
/// alone (`only`).
const BLOCKS: [(&str, bool, &[&str]); 7] = [
    ("− nature, potential", false, &["nature", "potential"]),
    ("− the land: nature, potential, bonus_tiles, resources, terrain, feature, centre", false, &["nature", "potential", "bonus_tiles", "resources", "terrain", "feature", "centre"]),
    ("− the context: distances, moment, heuristic", false, &["distances", "moment", "heuristic"]),
    ("only nature", true, &["nature"]),
    ("only potential", true, &["potential"]),
    ("only terrain, feature", true, &["terrain", "feature"]),
    ("only heuristic", true, &["heuristic"]),
];

/// The groups a block leaves out.
fn block_drop(block: &(&str, bool, &[&str])) -> Vec<&'static str> {
    let (_, only, groups) = block;
    GROUPS.iter().map(|g| g.0).filter(|g| groups.contains(g) != *only).collect()
}

/// Seeds of the retrained nets of the audit: their mean is compared.
const AUDIT_SEEDS: u64 = 5;

/// Permutation importance on the test rows and the retraining without each
/// group, `AUDIT_SEEDS` seeds each (the module docs).
fn audit(w: &mut String, options: &Options, parts: [&[&Site]; 3], trained: &Trained) {
    let test = parts[2];
    let base = quality(test, &f32_scores(&trained.net, test, &trained.kept, 0)).population;
    let repeats = 5;
    // The retrained nets, in parallel (each deterministic): the full one and
    // one without each group, at every seed.
    let mut dropped: Vec<Vec<&str>> = vec![Vec::new()];
    dropped.extend(GROUPS.iter().map(|g| vec![g.0]));
    dropped.extend(BLOCKS.iter().map(block_drop));
    let jobs: Vec<(&[&str], u64)> = dropped.iter().flat_map(|d| (0..AUDIT_SEEDS).map(move |s| (&d[..], s))).collect();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results: std::sync::Mutex<Vec<Option<Agreement>>> = std::sync::Mutex::new(vec![None; jobs.len()]);
    std::thread::scope(|scope| {
        for _ in 0..options.jobs.clamp(1, jobs.len()) {
            scope.spawn(|| loop {
                let at = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some((group, seed)) = jobs.get(at) else { break };
                let mut drop: Vec<&str> = options.drop.iter().map(String::as_str).collect();
                drop.extend(group.iter());
                let seeded = Options { config: Config { seed: options.config.seed + seed, ..options.config.clone() }, ..options.clone() };
                let retrained = train_without(&seeded, parts, &drop);
                let agreement = quality(test, &f32_scores(&retrained.net, test, &retrained.kept, 0)).population;
                results.lock().expect("no thread panicked holding the results")[at] = Some(agreement);
            });
        }
    });
    let results: Vec<Agreement> = results.into_inner().expect("no thread panicked holding the results").into_iter().map(Option::unwrap_or_default).collect();
    let mean = |g: usize| {
        let of = &results[g * AUDIT_SEEDS as usize..(g + 1) * AUDIT_SEEDS as usize];
        let n = of.len() as f64;
        (of.iter().map(|a| a.concordant).sum::<f64>() / n, of.iter().map(|a| a.within_spearman).sum::<f64>() / n)
    };
    let (full_right, full_rho) = mean(0);
    let _ = writeln!(w, "## Feature audit\n");
    let _ = writeln!(
        w,
        "Head 0 against the population at +{} on the test rows, pairs within the choice windows. **Permutation**: the group's columns \
         shuffled together across the test rows ({repeats} shuffles, the mean), the change of the share ordered right and of the within \
         Spearman against this net ({:.1} %, {:.3}). **Ablation**: the net retrained without the group with the same options at {AUDIT_SEEDS} \
         seeds from {}, the mean, against the full net at the same seeds ({:.1} %, {:.3}).\n",
        options.horizon,
        100.0 * base.concordant,
        base.within_spearman,
        options.config.seed,
        100.0 * full_right,
        full_rho
    );
    let _ = writeln!(w, "| group | features | permuted: Δ right, pp | Δ within Spearman | retrained without: right | Δ right, pp | Δ within Spearman |");
    let _ = writeln!(w, "|---|---:|---:|---:|---:|---:|---:|");
    for (g, (group, _)) in GROUPS.iter().enumerate() {
        let columns: Vec<usize> = group_features(group).into_iter().filter(|i| trained.kept.contains(i)).collect();
        let at: Vec<usize> = columns.iter().map(|c| trained.kept.iter().position(|k| k == c).expect("a kept column")).collect();
        let mut rng = Rng::new(0x5175 + g as u64);
        let (mut right, mut rho) = (0f64, 0f64);
        for _ in 0..repeats {
            let mut order: Vec<usize> = (0..test.len()).collect();
            rng.shuffle(&mut order);
            let scores: Vec<f64> = test
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    let mut x = inputs(s, &trained.kept);
                    let donor = inputs(test[order[i]], &trained.kept);
                    for a in &at {
                        x[*a] = donor[*a];
                    }
                    f64::from(trained.net.forward(&x)[0])
                })
                .collect();
            let q = quality(test, &scores).population;
            right += q.concordant / repeats as f64;
            rho += q.within_spearman / repeats as f64;
        }
        let (without_right, without_rho) = mean(g + 1);
        let _ = writeln!(
            w,
            "| {group} | {} | {:+.1} | {:+.3} | {:.1} % | {:+.1} | {:+.3} |",
            columns.len(),
            100.0 * (right - base.concordant),
            rho - base.within_spearman,
            100.0 * without_right,
            100.0 * (without_right - full_right),
            without_rho - full_rho
        );
    }
    let _ = writeln!(w, "\n| retrained | features | right | Δ right, pp | within Spearman |");
    let _ = writeln!(w, "|---|---:|---:|---:|---:|");
    for (b, block) in BLOCKS.iter().enumerate() {
        let features = SITE_FEATURES - block_drop(block).iter().map(|g| group_features(g).len()).sum::<usize>();
        let (right, rho) = mean(1 + GROUPS.len() + b);
        let _ = writeln!(w, "| {} | {features} | {:.1} % | {:+.1} | {rho:.3} |", block.0, 100.0 * right, 100.0 * (right - full_right));
    }
    let _ = writeln!(w);
}

/// The test rows kept as references: 24 rows spread over the f32 net's answer.
fn references(rows: &[&Site], net: &Net) -> Vec<(usize, f32)> {
    let mut scored: Vec<(usize, f32)> = rows.iter().enumerate().map(|(i, s)| (i, net.forward(&inputs(s, &(0..SITE_FEATURES).collect::<Vec<_>>()))[0])).collect();
    scored.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
    let count = 24.min(scored.len());
    (0..count).map(|k| scored[(scored.len() - 1) * k / (count - 1).max(1)]).collect()
}

fn weights_source(options: &Options, q: &Quantised, net: &Net, test_rows: &[&Site], sites: &[Site], map: &ScoreMap) -> String {
    let c = &options.config;
    let (first, last) = sites.iter().fold((u64::MAX, 0), |(a, b), s| (a.min(s.row.game), b.max(s.row.game)));
    let mut out = String::new();
    let w = &mut out;
    let hidden: Vec<String> = c.hidden.iter().map(|h| h.to_string()).collect();
    let _ = writeln!(w, "// generated by ai-train-site, do not edit");
    let _ = writeln!(w, "//! The trained weights of the site net (`ai::site_net`, run by `ai::net`):");
    let _ = writeln!(w, "//! {SITE_FEATURES} → {} → {}, trained on the cities beyond the capital of", hidden.join(" → "), c.heads.len());
    let _ = writeln!(w, "//! games {first}..={last} (`ai-site --size small --rounds 300`), and the f32");
    let _ = writeln!(w, "//! net's answers on a few test rows. Reproduced by `ai-train-site` with the");
    let _ = writeln!(w, "//! same data and options:");
    let flags = command(options);
    let _ = writeln!(w, "//! `{}`.", flags[flags.find(" --heads").map_or(0, |i| i + 1)..].trim());
    let _ = writeln!(w);
    let _ = writeln!(w, "use super::net::{{Activation, Layer, Net}};");
    let _ = writeln!(w, "use super::site::SITE_FEATURES;");
    let _ = writeln!(w, "use super::site_net::Reference;");
    let _ = writeln!(w);
    let mut widths = vec!["SITE_FEATURES".to_owned()];
    for (k, h) in c.hidden.iter().enumerate() {
        let _ = writeln!(w, "const H{}: usize = {h};", k + 1);
        widths.push(format!("H{}", k + 1));
    }
    let _ = writeln!(w, "const HEADS: usize = {};", c.heads.len());
    widths.push("HEADS".to_owned());
    let _ = writeln!(w);
    let widths: Vec<&str> = widths.iter().map(String::as_str).collect();
    w.push_str(&nn::weight_arrays(q, &widths));
    w.push_str(&nn::net_statics(q, "NET"));
    let _ = writeln!(w);
    let _ = writeln!(w, "/// The settling planner's score points of an answer of 0 citizens and of");
    let _ = writeln!(w, "/// each citizen more (`site_net::planner_score`; fitted by `ai-train-site`).");
    let _ = writeln!(w, "pub const SCORE_AT_ZERO: i32 = {};", map.at_zero);
    let _ = writeln!(w, "pub const SCORE_PER_CITIZEN: i32 = {};", map.per_citizen);
    let _ = writeln!(w);
    let picked = references(test_rows, net);
    let _ = writeln!(w, "pub static REFERENCE: [Reference; {}] = [", picked.len());
    for (i, value) in picked {
        let features: Vec<String> = test_rows[i].row.features.iter().map(|v| v.to_string()).collect();
        let _ = writeln!(w, "    Reference {{ features: [{}], output_milli: {} }},", features.join(", "), (f64::from(value) * 1000.0).round() as i64);
    }
    let _ = writeln!(w, "];");
    out
}
