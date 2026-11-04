//! Trains the choice net (`classic_rules::ai::choice_net`) on choice
//! datasets (`ai-choice`), writes its integer weights as generated Rust and
//! a report: whether the net would choose better than the allocator's own
//! order on the explored choices of the test games, how both order those
//! choices' outcomes, and which features the net needs.
//!
//! `cargo run --release --offline -p aurelia-qa --bin ai-train-choice -- --production FILE --research FILE [options]`
//!
//! --production FILE      production CSV (`ai-choice`'s PREFIX.production.csv)
//! --research FILE        research CSV (PREFIX.research.csv); at least one of the two
//! --target-production T  the outcome the production net estimates (default city_yields:30)
//! --target-research T    the outcome the research net estimates (default techs:60)
//!                        T: score:H, techs:H, beakers:H, citizens:H, cities:H, production:H (the
//!                        empire's gain over H rounds), city_population:H, city_food:H,
//!                        city_production:H, city_science:H, city_gold:H, city_culture:H,
//!                        city_yields:H (the city's), win (1: the chooser won the game); H in 30, 60
//! --baseline             fit the gain less a state-only net's estimate (the advantage)
//! --margin-production X, --margin-research X
//!                        the net's best is taken only when it leads the allocator's best by more
//!                        than X (standard units of the outcome); default: the best of `MARGINS`
//!                        on the validation sets
//! --min-set N            sets of at least N candidates (default 2)
//! --drop GROUP[,GROUP..] leave out feature groups (`groups`)
//! --audit                permutation importance and the retraining without each group
//! --out-weights FILE     the generated weights (crates/classic-rules/src/ai/choice_weights.rs)
//! --report FILE          the markdown report
//! --jobs J               trainings at once for --audit (default: the machine's threads)
//! --hidden H1,H2,..  --epochs N  --patience N  --lr X  --batch N  --seed N   (`nn::Config`;
//!                        defaults 32,16, 200, 12, 0.001, 256, 1)
//!
//! Rows: the explored choice sets (`ChoiceMode::Explore`: the candidate
//! taken was a uniformly random one of the allocator's top), each the taken
//! candidate's row and the outcome; a set whose outcome is unknown (the game
//! ended, the city was lost) has none. Only explored sets: the taken
//! candidate does not depend on the state there, so the net learns what a
//! candidate leads to and not where the allocator meets it; no propensity
//! weights are needed (every candidate of a set had the same chance). The
//! split is by game seed: `game % 20` below 14 trains, 14..17 validates
//! (early stopping), 17..20 tests (the report). The same data and options
//! give the same weights, bit for bit.
//!
//! The test: on the explored sets of the test games, the difference of the
//! outcome a policy would get, the net's best against the allocator's best
//! (rank 0), estimated by inverse propensity: a set counts k × (taken is the
//! net's − taken is the allocator's) × (outcome − b), where k is the set's
//! size and b the mean of the net's answers over the set (a control variate
//! that does not depend on the candidate taken: the estimate stays
//! unbiased); the 95 % interval by resampling the games.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::Instant;

use aurelia_qa::choice::{feature_names, CITY_STATE_NAMES, EMPIRE_STATE_NAMES, HORIZONS};
use aurelia_qa::nn::{self, Config, Head, Loss, Net, Quantised, Rng, Samples};
use aurelia_qa::site::Agreement;
use classic_rules::ai::choice::{CITY_FEATURES, EMPIRE_FEATURES};
use classic_rules::ai::choice_net::Kind;
use classic_rules::ai::net::forward;

/// Rounds of a window of the ordering pairs.
const WINDOW: u32 = 20;
/// Label columns a line: per horizon (now, `HORIZONS`) the city's then the
/// empire's states.
const LABELS: usize = 3 * 12;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Base {
    Empire(usize),
    City(usize),
    CityYields,
    Win,
}

/// An outcome: the sum of the gains of its parts over `horizon` rounds.
#[derive(Debug, Clone, PartialEq)]
struct Target {
    parts: Vec<Base>,
    horizon: u32,
}

impl Target {
    /// `part+part+..:H` or `win`.
    fn parse(text: &str) -> Option<Self> {
        if text == "win" {
            return Some(Target { parts: vec![Base::Win], horizon: 0 });
        }
        let (names, horizon) = text.split_once(':')?;
        let horizon: u32 = horizon.parse().ok()?;
        if !HORIZONS.contains(&horizon) {
            return None;
        }
        let mut parts = Vec::new();
        for name in names.split('+') {
            parts.push(if name == "city_yields" {
                Base::CityYields
            } else if let Some(city) = name.strip_prefix("city_") {
                Base::City(CITY_STATE_NAMES.iter().position(|n| *n == city)?)
            } else {
                Base::Empire(EMPIRE_STATE_NAMES.iter().position(|n| *n == name)?)
            });
        }
        Some(Target { parts, horizon })
    }

    fn name(&self) -> String {
        let names: Vec<String> = self
            .parts
            .iter()
            .map(|b| match b {
                Base::Win => "win".to_owned(),
                Base::CityYields => "city_yields".to_owned(),
                Base::City(i) => format!("city_{}", CITY_STATE_NAMES[*i]),
                Base::Empire(i) => EMPIRE_STATE_NAMES[*i].to_owned(),
            })
            .collect();
        if self.parts == [Base::Win] { "win".to_owned() } else { format!("{}:{}", names.join("+"), self.horizon) }
    }

    /// The outcome of a set; None: unknown.
    fn of(&self, set: &Set) -> Option<f64> {
        let h = 1 + HORIZONS.iter().position(|x| *x == self.horizon).unwrap_or(0);
        let gain = |column: usize| {
            let (now, then) = (set.labels[column], set.labels[h * 12 + column]);
            (now.is_finite() && then.is_finite()).then(|| f64::from(then - now))
        };
        self.parts
            .iter()
            .map(|base| match base {
                Base::Win => Some(f64::from(u8::from(set.winner == Some(set.player)))),
                Base::Empire(i) => gain(6 + i),
                Base::City(i) => gain(*i),
                Base::CityYields => (1..6).map(gain).sum(),
            })
            .sum()
    }
}

#[derive(Debug, Clone)]
struct Options {
    data: [Option<PathBuf>; 2],
    targets: [Target; 2],
    config: Config,
    baseline: bool,
    /// The margins of the nets' choice (standard units); None: the best on
    /// the validation sets of `MARGINS`.
    margins: [Option<f64>; 2],
    min_set: usize,
    drop: Vec<String>,
    audit: bool,
    jobs: usize,
    out_weights: Option<PathBuf>,
    report: Option<PathBuf>,
}

fn options() -> Result<Options, String> {
    let mut options = Options {
        data: [None, None],
        targets: [Target::parse("city_yields:30").expect("a target"), Target::parse("techs:60").expect("a target")],
        config: Config { hidden: vec![32, 16], ..Config::default() },
        baseline: false,
        margins: [None, None],
        min_set: 2,
        drop: Vec::new(),
        audit: false,
        jobs: std::thread::available_parallelism().map_or(1, |n| n.get()),
        out_weights: None,
        report: None,
    };
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{flag} needs a value"));
        if options.config.parse_flag(&flag, &mut value)? {
            continue;
        }
        match flag.as_str() {
            "--production" => options.data[0] = Some(PathBuf::from(value()?)),
            "--research" => options.data[1] = Some(PathBuf::from(value()?)),
            "--target-production" | "--target-research" => {
                let text = value()?;
                let target = Target::parse(&text).ok_or_else(|| format!("{flag} {text}: unknown target"))?;
                options.targets[usize::from(flag == "--target-research")] = target;
            }
            "--baseline" => options.baseline = true,
            "--margin-production" | "--margin-research" => {
                let text = value()?;
                options.margins[usize::from(flag == "--margin-research")] = Some(text.parse().map_err(|e| format!("{flag} {text}: {e}"))?);
            }
            "--min-set" => options.min_set = value()?.parse().map_err(|e| format!("--min-set: {e}"))?,
            "--drop" => options.drop = value()?.split(',').map(str::to_owned).collect(),
            "--audit" => options.audit = true,
            "--jobs" => options.jobs = value()?.parse().map_err(|e| format!("--jobs: {e}"))?,
            "--out-weights" => options.out_weights = Some(PathBuf::from(value()?)),
            "--report" => options.report = Some(PathBuf::from(value()?)),
            other => return Err(format!("unknown option {other}")),
        }
    }
    if options.data.iter().all(Option::is_none) {
        return Err("--production FILE or --research FILE is required".to_owned());
    }
    options.config.heads = vec![Head { loss: Loss::Huber(1.0), weight: 1.0 }];
    Ok(options)
}

fn main() {
    if let Err(error) = options().and_then(|options| run(&options)) {
        eprintln!("ai-train-choice: {error}");
        std::process::exit(1);
    }
}

/// One explored choice set.
#[derive(Debug, Clone)]
struct Set {
    id: u64,
    game: u64,
    player: u8,
    round: u32,
    city: i64,
    /// The candidates' rows, best first by the allocator, and items.
    rows: Vec<Vec<i32>>,
    items: Vec<u32>,
    taken: usize,
    labels: [f32; LABELS],
    winner: Option<u8>,
}

impl Set {
    /// The ordering window: the game, the player, the city, `WINDOW` rounds.
    fn window(&self) -> u64 {
        (self.game << 28) ^ (u64::from(self.player) << 24) ^ ((self.city as u64 & 0xFFF) << 12) ^ u64::from(self.round / WINDOW)
    }
}

/// Counts of a loaded file.
#[derive(Debug, Clone, Copy, Default)]
struct Loaded {
    lines: usize,
    sets: usize,
    explored: usize,
}

/// The explored sets of a choice CSV file (`choice::header`).
fn load(path: &PathBuf, kind: Kind) -> Result<(Vec<Set>, Loaded), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut lines = text.lines();
    let header: Vec<&str> = lines.next().ok_or_else(|| format!("{}: empty", path.display()))?.split(',').collect();
    let column = |name: &str| header.iter().position(|h| *h == name).ok_or_else(|| format!("{}: no column {name}", path.display()));
    let features: Vec<usize> = feature_names(kind).iter().map(|n| column(n)).collect::<Result<_, _>>()?;
    let mut labels = Vec::new();
    for offset in [0].into_iter().chain(HORIZONS) {
        for n in CITY_STATE_NAMES {
            labels.push(column(&format!("city_{n}_{offset}"))?);
        }
        for n in EMPIRE_STATE_NAMES {
            labels.push(column(&format!("empire_{n}_{offset}"))?);
        }
    }
    let [set, game, player, round, city, explored, taken, item, winner] = ["set", "game", "player", "round", "city", "explored", "taken", "item", "winner"].map(column);
    let (set, game, player, round, city, explored, taken, item, winner) = (set?, game?, player?, round?, city?, explored?, taken?, item?, winner?);
    let mut out: Vec<Set> = Vec::new();
    let mut counts = Loaded::default();
    let mut current: Option<u64> = None;
    for (n, line) in lines.enumerate() {
        counts.lines += 1;
        let fields: Vec<&str> = line.split(',').collect();
        let int = |c: usize| fields[c].parse::<i64>().map_err(|e| format!("{} line {}: column {}: {e}", path.display(), n + 2, header[c]));
        let id = int(set)? as u64;
        if current != Some(id) {
            current = Some(id);
            counts.sets += 1;
        }
        if int(explored)? == 0 {
            continue;
        }
        let row: Vec<i32> = features.iter().map(|c| int(*c).map(|v| v as i32)).collect::<Result<_, _>>()?;
        if out.last().is_none_or(|s: &Set| s.id != id) {
            counts.explored += 1;
            let mut values = [f32::NAN; LABELS];
            for (slot, c) in values.iter_mut().zip(&labels) {
                if !fields[*c].is_empty() {
                    *slot = fields[*c].parse::<f32>().map_err(|e| format!("{} line {}: {e}", path.display(), n + 2))?;
                }
            }
            out.push(Set {
                id,
                game: int(game)? as u64,
                player: int(player)? as u8,
                round: int(round)? as u32,
                city: int(city)?,
                rows: Vec::new(),
                items: Vec::new(),
                taken: usize::MAX,
                labels: values,
                winner: if fields[winner].is_empty() { None } else { Some(int(winner)? as u8) },
            });
        }
        let last = out.last_mut().expect("a set");
        if int(taken)? == 1 {
            last.taken = last.rows.len();
        }
        last.rows.push(row);
        last.items.push(int(item)? as u32);
    }
    Ok((out, counts))
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

/// The feature groups of a kind: a name and its features' names.
fn groups(kind: Kind) -> Vec<(&'static str, Vec<&'static str>)> {
    let names = feature_names(kind);
    let prices = |n: &&str| n.starts_with("price_") || *n == "rounds_left";
    let empire: Vec<&str> = names[..EMPIRE_FEATURES].to_vec();
    let mut out = vec![("empire", empire.iter().copied().filter(|n| !prices(n)).collect()), ("prices", empire.iter().copied().filter(prices).collect())];
    let candidate_start = match kind {
        Kind::Production => {
            out.push(("city", names[EMPIRE_FEATURES..EMPIRE_FEATURES + CITY_FEATURES].to_vec()));
            EMPIRE_FEATURES + CITY_FEATURES
        }
        Kind::Research => EMPIRE_FEATURES,
    };
    let candidate = &names[candidate_start..];
    let amounts = |n: &&str| n.starts_with("flat_") || n.starts_with("percent_") || n.starts_with("per_citizen_");
    let pick = |f: &dyn Fn(&&str) -> bool| candidate.iter().copied().filter(|n| f(n)).collect::<Vec<&str>>();
    match kind {
        Kind::Production => {
            out.push(("kind", pick(&|n| n.starts_with("is_"))));
            out.push(("allocator", pick(&|n| ["value", "value_per_turn", "permille_of_best", "rank"].contains(n))));
            out.push(("cost", pick(&|n| ["turns", "cost", "upkeep"].contains(n))));
            out.push(("effect values", pick(&|n| n.starts_with("value_") && *n != "value_per_turn")));
            out.push(("amounts", pick(&amounts)));
        }
        Kind::Research => {
            out.push(("cost", pick(&|n| ["tech_cost", "tech_turns"].contains(n))));
            out.push(("allocator", pick(&|n| ["value_per_beaker", "permille_of_best", "rank", "own_value", "follow_up", "points", "sea_value"].contains(n))));
            out.push(("unlocks", pick(&|n| n.starts_with("unlocks_") || ["tech_era", "strength_gain", "embarking", "roads", "map"].contains(n))));
            out.push(("unlock values", pick(&|n| ["value_buildings", "value_units", "value_improvements", "value_effects"].contains(n))));
            out.push(("amounts", pick(&amounts)));
        }
    }
    out
}

/// The columns kept without the groups `drop`.
fn kept(kind: Kind, drop: &[&str]) -> Vec<usize> {
    let names = feature_names(kind);
    let dropped: Vec<&str> = groups(kind).into_iter().filter(|(g, _)| drop.contains(g)).flat_map(|(_, f)| f).collect();
    (0..names.len()).filter(|i| !dropped.contains(&names[*i])).collect()
}

fn inputs(row: &[i32], kept: &[usize]) -> Vec<f32> {
    kept.iter().map(|i| row[*i] as f32).collect()
}

/// One kind's sets with their outcomes, by part.
struct Data<'s> {
    kind: Kind,
    parts: [Vec<(&'s Set, f64)>; 3],
    /// The training outcomes' mean and spread (the net fits the outcome
    /// in these units).
    mean: f64,
    scale: f64,
}

/// The samples of the taken candidates; `less` subtracts a baseline's
/// estimate (in the standard units).
fn samples(data: &Data<'_>, part: usize, kept: &[usize], less: Option<&[f64]>) -> Samples {
    let mut out = Samples::new(kept.len(), 1);
    for (i, (set, y)) in data.parts[part].iter().enumerate() {
        let mut target = (y - data.mean) / data.scale;
        if let Some(less) = less {
            target -= less[i];
        }
        out.push(&inputs(&set.rows[set.taken], kept), &[target as f32], set.window());
    }
    out
}

/// A trained net, its columns, and the state-only baseline it fits
/// against (if any).
struct Trained {
    net: Net,
    kept: Vec<usize>,
    epochs: Vec<nn::Epoch>,
    seconds: f64,
}

/// The state columns of a kind (the baseline reads them).
fn state_columns(kind: Kind) -> Vec<usize> {
    (0..match kind {
        Kind::Production => EMPIRE_FEATURES + CITY_FEATURES,
        Kind::Research => EMPIRE_FEATURES,
    })
        .collect()
}

fn train(options: &Options, config: &Config, data: &Data<'_>, drop: &[&str]) -> Trained {
    let kept = kept(data.kind, drop);
    let started = Instant::now();
    let (train_less, valid_less) = if options.baseline {
        let state = state_columns(data.kind);
        let (base, _) = nn::train(&samples(data, 0, &state, None), &samples(data, 1, &state, None), config);
        let estimate = |p: usize| data.parts[p].iter().map(|(s, _)| f64::from(base.forward(&inputs(&s.rows[s.taken], &state))[0])).collect::<Vec<f64>>();
        (Some(estimate(0)), Some(estimate(1)))
    } else {
        (None, None)
    };
    let (net, epochs) = nn::train(&samples(data, 0, &kept, train_less.as_deref()), &samples(data, 1, &kept, valid_less.as_deref()), config);
    Trained { net, kept, epochs, seconds: started.elapsed().as_secs_f64() }
}

/// A predictor's answers for every candidate of every set.
type Answers = Vec<Vec<f64>>;

fn f32_answers(net: &Net, sets: &[(&Set, f64)], kept: &[usize]) -> Answers {
    sets.iter().map(|(s, _)| s.rows.iter().map(|r| f64::from(net.forward(&inputs(r, kept))[0])).collect()).collect()
}

fn int_answers(q: &Quantised, sets: &[(&Set, f64)], kept: &[usize]) -> Answers {
    let layers = q.layers();
    let net = q.net(&layers);
    let mut out = [0i64; 1];
    sets.iter()
        .map(|(s, _)| {
            s.rows
                .iter()
                .map(|r| {
                    let row: Vec<i32> = kept.iter().map(|i| r[*i]).collect();
                    forward(&net, &row, &mut out);
                    out[0] as f64
                })
                .collect()
        })
        .collect()
}

/// The allocator's answers: its value per unit per mille of the set's best
/// (the rank where ties).
fn allocator_answers(kind: Kind, sets: &[(&Set, f64)]) -> Answers {
    let names = feature_names(kind);
    let at = names.iter().position(|n| *n == "permille_of_best").expect("a permille column");
    sets.iter().map(|(s, _)| s.rows.iter().enumerate().map(|(r, row)| f64::from(row[at]) - r as f64 * 1e-3).collect()).collect()
}

/// The control variate of each set: the mean of the net's answers over
/// its candidates, in the outcome's units.
fn controls(data: &Data<'_>, sets: &[(&Set, f64)], net: &Answers) -> Vec<f64> {
    sets.iter().zip(net).map(|((s, _), a)| data.mean + data.scale * a.iter().sum::<f64>() / s.rows.len() as f64).collect()
}

/// What the net chooses on the test sets against the allocator's best: the
/// rank it takes and, for production, the kind of order.
fn choices_table(w: &mut String, kind: Kind, sets: &[(&Set, f64)], net: &Answers) {
    let mut ranks = [0usize; 4];
    for a in net {
        ranks[argmax(a).min(3)] += 1;
    }
    let n = sets.len().max(1) as f64;
    let shares: Vec<String> = ranks.iter().map(|r| format!("{:.1} %", 100.0 * *r as f64 / n)).collect();
    let _ = writeln!(w, "The net takes rank 0, 1, 2, 3 of the allocator's top in {} of the test sets.\n", shares.join(", "));
    if kind != Kind::Production {
        return;
    }
    let names = feature_names(kind);
    let kinds: Vec<(usize, &str)> = names.iter().enumerate().filter(|(_, n)| n.starts_with("is_")).map(|(i, n)| (i, *n)).collect();
    let _ = writeln!(w, "| order | in the sets | the allocator's best | the net's choice |");
    let _ = writeln!(w, "|---|---:|---:|---:|");
    for (column, name) in kinds {
        let present = sets.iter().filter(|(s, _)| s.rows.iter().any(|r| r[column] == 1)).count();
        let best = sets.iter().filter(|(s, _)| s.rows[0][column] == 1).count();
        let chosen = sets.iter().zip(net).filter(|((s, _), a)| s.rows[argmax(a)][column] == 1).count();
        if present == 0 {
            continue;
        }
        let _ = writeln!(w, "| {} | {:.1} % | {:.1} % | {:.1} % |", &name[3..], 100.0 * present as f64 / n, 100.0 * best as f64 / n, 100.0 * chosen as f64 / n);
    }
    let _ = writeln!(w);
}

/// The candidate a net with `margin` takes: its best when that leads the
/// allocator's best (rank 0) by more than the margin.
fn choice(answers: &[f64], margin: f64) -> usize {
    let best = argmax(answers);
    if answers[best] - answers[0] > margin { best } else { 0 }
}

/// The first best.
fn argmax(values: &[f64]) -> usize {
    let mut at = 0;
    for (i, v) in values.iter().enumerate() {
        if *v > values[at] {
            at = i;
        }
    }
    at
}

/// The difference of the outcome of choosing by `answers` and by the
/// allocator (rank 0), inverse propensity with the set mean of `control`
/// as the control variate: the estimate, its 95 % interval over games, and
/// the share of sets the two choose differently.
#[derive(Debug, Clone, Copy, Default)]
struct Policy {
    sets: usize,
    delta: f64,
    low: f64,
    high: f64,
    differ: f64,
}

fn policy(sets: &[(&Set, f64)], answers: &Answers, control: &[f64], margin: f64) -> Policy {
    let mut by_game: BTreeMap<u64, (f64, usize)> = BTreeMap::new();
    let mut differ = 0;
    for ((set, y), (a, b)) in sets.iter().zip(answers.iter().zip(control)) {
        let k = set.rows.len() as f64;
        let chosen = choice(a, margin);
        differ += usize::from(chosen != 0);
        let d = k * (f64::from(u8::from(set.taken == chosen)) - f64::from(u8::from(set.taken == 0))) * (y - b);
        let entry = by_game.entry(set.game).or_default();
        entry.0 += d;
        entry.1 += 1;
    }
    let games: Vec<(f64, usize)> = by_game.into_values().collect();
    let total = |g: &[(f64, usize)]| {
        let (s, n) = g.iter().fold((0.0, 0usize), |(s, n), x| (s + x.0, n + x.1));
        s / n.max(1) as f64
    };
    let delta = total(&games);
    let mut rng = Rng::new(0xB007);
    let mut draws: Vec<f64> = (0..1000)
        .map(|_| {
            let sample: Vec<(f64, usize)> = (0..games.len()).map(|_| games[rng.below(games.len())]).collect();
            total(&sample)
        })
        .collect();
    draws.sort_by(|a, b| a.total_cmp(b));
    Policy { sets: sets.len(), delta, low: draws[25], high: draws[974], differ: differ as f64 / sets.len().max(1) as f64 }
}

/// How a predictor orders the outcomes of the taken candidates within
/// the windows (`Set::window`): its answer for the taken candidate less
/// the set's mean answer.
fn ordering(sets: &[(&Set, f64)], answers: &Answers) -> Agreement {
    let points: Vec<(u64, f64, f64)> = sets
        .iter()
        .zip(answers)
        .map(|((s, y), a)| (s.window(), a[s.taken] - a.iter().sum::<f64>() / a.len() as f64, *y))
        .collect();
    Agreement::of(&points)
}

/// The mean outcome by the rank taken, sets of `size` candidates, with the
/// 95 % half-interval.
fn by_rank(sets: &[(&Set, f64)], size: usize) -> Vec<(usize, f64, f64)> {
    (0..size)
        .map(|r| {
            let ys: Vec<f64> = sets.iter().filter(|(s, _)| s.rows.len() == size && s.taken == r).map(|(_, y)| *y).collect();
            let n = ys.len().max(1) as f64;
            let mean = ys.iter().sum::<f64>() / n;
            let var = ys.iter().map(|y| (y - mean).powi(2)).sum::<f64>() / (n - 1.0).max(1.0);
            (ys.len(), mean, 1.96 * (var / n).sqrt())
        })
        .collect()
}

fn run(options: &Options) -> Result<(), String> {
    let mut report = String::new();
    let mut nets: Vec<Net4> = Vec::new();
    let _ = writeln!(report, "# Choice net: the test games\n");
    for (k, kind) in [Kind::Production, Kind::Research].into_iter().enumerate() {
        let Some(path) = &options.data[k] else { continue };
        let started = Instant::now();
        let (sets, loaded) = load(path, kind)?;
        let target = &options.targets[k];
        let mut parts: [Vec<(&Set, f64)>; 3] = [Vec::new(), Vec::new(), Vec::new()];
        let mut unknown = 0;
        let mut small = 0;
        for set in &sets {
            if set.rows.len() < options.min_set || set.taken >= set.rows.len() {
                small += 1;
                continue;
            }
            let Some(y) = target.of(set) else {
                unknown += 1;
                continue;
            };
            parts[match part(set.game) {
                Part::Train => 0,
                Part::Valid => 1,
                Part::Test => 2,
            }]
            .push((set, y));
        }
        let ys: Vec<f64> = parts[0].iter().map(|(_, y)| *y).collect();
        let mean = ys.iter().sum::<f64>() / ys.len().max(1) as f64;
        let scale = (ys.iter().map(|y| (y - mean).powi(2)).sum::<f64>() / ys.len().max(1) as f64).sqrt().max(1e-6);
        let data = Data { kind, parts, mean, scale };
        let counts = format!(
            "{}: {} lines, {} sets, {} explored; {small} with fewer than {} candidates, {unknown} without an outcome; train {}, validation {}, test {} sets; \
             the outcome {}: mean {mean:.2}, sd {scale:.2}",
            kind.name(),
            loaded.lines,
            loaded.sets,
            loaded.explored,
            options.min_set,
            data.parts[0].len(),
            data.parts[1].len(),
            data.parts[2].len(),
            target.name()
        );
        println!("{counts}; loaded in {:.1} s", started.elapsed().as_secs_f64());
        if data.parts.iter().any(Vec::is_empty) {
            return Err(format!("{}: every part needs sets (game % 20: 0..14 train, 14..17 validation, 17..20 test)", kind.name()));
        }
        let drop: Vec<&str> = options.drop.iter().map(String::as_str).collect();
        let trained = train(options, &options.config, &data, &drop);
        let best_epoch = trained.epochs.iter().enumerate().min_by(|a, b| a.1.valid_loss.total_cmp(&b.1.valid_loss)).map_or(0, |(i, _)| i + 1);
        println!("{}: trained in {:.1} s, best epoch {best_epoch} of {}", kind.name(), trained.seconds, trained.epochs.len());
        let quantised = nn::quantise(&trained.net);

        let _ = writeln!(report, "## {}\n", kind.name());
        let _ = writeln!(report, "{counts}.\n");
        let hidden: Vec<String> = options.config.hidden.iter().map(|h| h.to_string()).collect();
        let _ = writeln!(
            report,
            "Net: {} → {} → 1 (Huber on the standardised outcome{}), best epoch {best_epoch} of {}, {:.1} s; {} parameters.\n",
            trained.kept.len(),
            hidden.join(" → "),
            if options.baseline { ", less a state-only net's estimate" } else { "" },
            trained.epochs.len(),
            trained.seconds,
            quantised.parameters()
        );
        for (p, title) in [(2, "Test"), (1, "Validation")] {
            let sets = &data.parts[p];
            let net = f32_answers(&trained.net, sets, &trained.kept);
            let int = int_answers(&quantised, sets, &trained.kept);
            let allocator = allocator_answers(kind, sets);
            let _ = writeln!(report, "### {title}: choosing by the net against the allocator's best\n");
            let _ = writeln!(report, "| predictor | sets | chooses another than rank 0 | Δ outcome a choice | 95 % CI | pairs | ordered right | within Spearman |");
            let _ = writeln!(report, "|---|---:|---:|---:|---|---:|---:|---:|");
            let mut rng = Rng::new(0xC401 + p as u64);
            let random: Answers = sets.iter().map(|(s, _)| s.rows.iter().map(|_| rng.unit() as f64).collect()).collect();
            let control = controls(&data, sets, &net);
            for (name, answers) in [("allocator (permille of best)", &allocator), ("net f32", &net), ("net integer", &int), ("random of the top", &random)] {
                let pol = policy(sets, answers, &control, 0.0);
                let order = ordering(sets, answers);
                let _ = writeln!(
                    report,
                    "| {name} | {} | {:.1} % | {:+.3} | {:+.3}…{:+.3} | {} | {:.1} % | {:.3} |",
                    pol.sets,
                    100.0 * pol.differ,
                    pol.delta,
                    pol.low,
                    pol.high,
                    order.pairs,
                    100.0 * order.concordant,
                    order.within_spearman
                );
                if p == 2 {
                    println!("{} test {name}: Δ {:+.3} ({:+.3}…{:+.3}), differs {:.1} %, pairs right {:.1} %", kind.name(), pol.delta, pol.low, pol.high, 100.0 * pol.differ, 100.0 * order.concordant);
                }
            }
            let _ = writeln!(report);
            if p == 2 {
                choices_table(&mut report, kind, sets, &net);
                let gap = net.iter().flatten().zip(int.iter().flatten()).fold(0f64, |m, (a, b)| m.max((a - b / 65536.0).abs()));
                let same = net.iter().zip(&int).filter(|(a, b)| argmax(a) == argmax(b)).count();
                let _ = writeln!(
                    report,
                    "Integer inference against f32 on the test candidates: at most {gap:.5} apart (standard units), the same choice in {same} of {} sets.\n",
                    sets.len()
                );
            }
        }
        // The margin: the best Δ on the validation sets, or the option's.
        let answers: Vec<Answers> = (1..3).map(|p| f32_answers(&trained.net, &data.parts[p], &trained.kept)).collect();
        let controls_of: Vec<Vec<f64>> = (1..3).map(|p| controls(&data, &data.parts[p], &answers[p - 1])).collect();
        let _ = writeln!(report, "### The margin: the net's best taken only when it leads rank 0 by more (standard units)\n");
        let _ = writeln!(report, "| margin | validation: differs | Δ | 95 % CI | test: differs | Δ | 95 % CI |");
        let _ = writeln!(report, "|---:|---:|---:|---|---:|---:|---|");
        let mut best_margin = (f64::NEG_INFINITY, 0.0);
        for margin in MARGINS {
            let v = policy(&data.parts[1], &answers[0], &controls_of[0], margin);
            let t = policy(&data.parts[2], &answers[1], &controls_of[1], margin);
            if v.delta > best_margin.0 {
                best_margin = (v.delta, margin);
            }
            let _ = writeln!(
                report,
                "| {margin} | {:.1} % | {:+.3} | {:+.3}…{:+.3} | {:.1} % | {:+.3} | {:+.3}…{:+.3} |",
                100.0 * v.differ,
                v.delta,
                v.low,
                v.high,
                100.0 * t.differ,
                t.delta,
                t.low,
                t.high
            );
            println!("{} margin {margin}: validation Δ {:+.3} ({:+.3}…{:+.3}) differs {:.1} %; test Δ {:+.3} ({:+.3}…{:+.3}) differs {:.1} %", kind.name(), v.delta, v.low, v.high, 100.0 * v.differ, t.delta, t.low, t.high, 100.0 * t.differ);
        }
        let margin = options.margins[k].unwrap_or(best_margin.1);
        let _ = writeln!(report, "\nThe game's margin: {margin}{}.\n", if options.margins[k].is_some() { " (given)" } else { " (the best on validation)" });
        let _ = writeln!(report, "### The outcome by the rank taken (explored sets of {} candidates, all parts)\n", 4);
        let _ = writeln!(report, "| rank taken | sets | mean {} | ± 95 % |", target.name());
        let _ = writeln!(report, "|---:|---:|---:|---:|");
        let all: Vec<(&Set, f64)> = data.parts.iter().flatten().copied().collect();
        for (r, (n, m, h)) in by_rank(&all, 4).into_iter().enumerate() {
            let _ = writeln!(report, "| {r} | {n} | {m:.3} | {h:.3} |");
        }
        let _ = writeln!(report);
        if options.audit {
            audit(&mut report, options, &data, &trained);
        }
        let nanos = time_inference(&quantised, &data.parts[2], &trained.kept);
        let _ = writeln!(report, "Integer inference: {nanos:.0} ns a candidate.\n");
        if trained.kept.len() == feature_names(kind).len() {
            nets.push((kind, quantised.clone(), references(&quantised, &data.parts[2]), (margin * 65536.0).round() as i64));
        }
    }
    let _ = writeln!(report, "## Reproduce\n\n```\ncargo +1.98.1 run --release --offline -p aurelia-qa --bin ai-train-choice -- {}\n```", command(options));
    if let Some(path) = &options.report {
        std::fs::write(path, &report).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("report: {}", path.display());
    } else {
        println!("{report}");
    }
    if let Some(path) = &options.out_weights {
        if !options.drop.is_empty() {
            return Err("--out-weights: the game's nets read every feature (no --drop)".to_owned());
        }
        std::fs::write(path, weights_source(options, &nets)).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("weights: {}", path.display());
    }
    Ok(())
}

/// The training options as the command line takes them.
fn command(options: &Options) -> String {
    let mut out = String::new();
    for (flag, path) in ["--production", "--research"].iter().zip(&options.data) {
        if let Some(path) = path {
            let _ = write!(out, "{flag} {} ", path.display());
        }
    }
    let _ = write!(out, "--target-production {} --target-research {} ", options.targets[0].name(), options.targets[1].name());
    if options.baseline {
        out.push_str("--baseline ");
    }
    for (flag, margin) in ["--margin-production", "--margin-research"].iter().zip(options.margins) {
        if let Some(margin) = margin {
            let _ = write!(out, "{flag} {margin} ");
        }
    }
    let _ = write!(out, "--min-set {} {} --seed {}", options.min_set, options.config.flags(), options.config.seed);
    out
}

/// Nanoseconds of one integer inference of a candidate.
fn time_inference(q: &Quantised, sets: &[(&Set, f64)], kept: &[usize]) -> f64 {
    let layers = q.layers();
    let net = q.net(&layers);
    let rows: Vec<Vec<i32>> = sets.iter().take(2000).flat_map(|(s, _)| s.rows.iter().map(|r| kept.iter().map(|i| r[*i]).collect::<Vec<i32>>())).collect();
    let mut out = [0i64; 1];
    let (mut calls, mut sink) = (0u64, 0i64);
    let started = Instant::now();
    while started.elapsed().as_millis() < 300 {
        for row in &rows {
            forward(&net, std::hint::black_box(row), &mut out);
            sink = sink.wrapping_add(out[0]);
            calls += 1;
        }
    }
    std::hint::black_box(sink);
    started.elapsed().as_nanos() as f64 / calls.max(1) as f64
}

/// Seeds of the retrained nets of the audit: their mean is compared.
const AUDIT_SEEDS: u64 = 3;

/// The quality of a net on the test sets: the policy difference, the
/// ordering, the test loss.
#[derive(Debug, Clone, Copy, Default)]
struct Quality {
    delta: f64,
    right: f64,
    loss: f64,
}

fn quality(data: &Data<'_>, answers: &Answers) -> Quality {
    let sets = &data.parts[2];
    let pol = policy(sets, answers, &controls(data, sets, answers), 0.0);
    let order = ordering(sets, answers);
    let loss = sets.iter().zip(answers).map(|((s, y), a)| ((y - data.mean) / data.scale - a[s.taken]).powi(2)).sum::<f64>() / sets.len().max(1) as f64;
    Quality { delta: pol.delta, right: order.concordant, loss }
}

/// Permutation importance on the test sets and the retraining without
/// each group.
fn audit(w: &mut String, options: &Options, data: &Data<'_>, trained: &Trained) {
    let kind = data.kind;
    let names = feature_names(kind);
    let test = &data.parts[2];
    let base = quality(data, &f32_answers(&trained.net, test, &trained.kept));
    let groups = groups(kind);
    let mut dropped: Vec<Vec<&str>> = vec![Vec::new()];
    dropped.extend(groups.iter().map(|g| vec![g.0]));
    let jobs: Vec<(usize, u64)> = (0..dropped.len()).flat_map(|d| (0..AUDIT_SEEDS).map(move |s| (d, s))).collect();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results: std::sync::Mutex<Vec<Quality>> = std::sync::Mutex::new(vec![Quality::default(); jobs.len()]);
    std::thread::scope(|scope| {
        for _ in 0..options.jobs.clamp(1, jobs.len()) {
            scope.spawn(|| loop {
                let at = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some((d, seed)) = jobs.get(at) else { break };
                let mut drop: Vec<&str> = options.drop.iter().map(String::as_str).collect();
                drop.extend(dropped[*d].iter());
                let config = Config { seed: options.config.seed + seed, ..options.config.clone() };
                let retrained = train(options, &config, data, &drop);
                let q = quality(data, &f32_answers(&retrained.net, test, &retrained.kept));
                results.lock().expect("no thread panicked holding the results")[at] = q;
            });
        }
    });
    let results = results.into_inner().expect("no thread panicked holding the results");
    let mean = |d: usize| {
        let of = &results[d * AUDIT_SEEDS as usize..(d + 1) * AUDIT_SEEDS as usize];
        let n = of.len() as f64;
        Quality { delta: of.iter().map(|q| q.delta).sum::<f64>() / n, right: of.iter().map(|q| q.right).sum::<f64>() / n, loss: of.iter().map(|q| q.loss).sum::<f64>() / n }
    };
    let full = mean(0);
    let _ = writeln!(w, "### Feature audit\n");
    let _ = writeln!(
        w,
        "On the test sets: Δ outcome (the net's choice against rank 0, as above), ordered right (pairs of taken candidates within the windows), \
         test loss (squared error of the taken candidate's answer, standard units). **Permutation**: the group's columns shuffled together \
         across the test candidates (3 shuffles, the mean), against this net (Δ {:+.3}, {:.1} %, loss {:.4}). **Ablation**: retrained \
         without the group at {AUDIT_SEEDS} seeds, the mean, against the full net at the same seeds (Δ {:+.3}, {:.1} %, loss {:.4}).\n",
        base.delta,
        100.0 * base.right,
        base.loss,
        full.delta,
        100.0 * full.right,
        full.loss
    );
    let _ = writeln!(w, "| group | features | permuted: Δ outcome | ordered right, pp | loss | retrained without: Δ outcome | ordered right, pp | loss |");
    let _ = writeln!(w, "|---|---:|---:|---:|---:|---:|---:|---:|");
    for (g, (group, features)) in groups.iter().enumerate() {
        let columns: Vec<usize> = features.iter().filter_map(|f| names.iter().position(|n| n == f)).filter_map(|c| trained.kept.iter().position(|k| *k == c)).collect();
        let mut rng = Rng::new(0x5175 + g as u64);
        let flat: Vec<(usize, usize)> = test.iter().enumerate().flat_map(|(i, (s, _))| (0..s.rows.len()).map(move |r| (i, r))).collect();
        let mut q = Quality::default();
        let repeats = 3;
        for _ in 0..repeats {
            let mut order: Vec<usize> = (0..flat.len()).collect();
            rng.shuffle(&mut order);
            let mut answers: Answers = test.iter().map(|(s, _)| vec![0.0; s.rows.len()]).collect();
            for (at, &(i, r)) in flat.iter().enumerate() {
                let mut x = inputs(&test[i].0.rows[r], &trained.kept);
                let (di, dr) = flat[order[at]];
                let donor = inputs(&test[di].0.rows[dr], &trained.kept);
                for c in &columns {
                    x[*c] = donor[*c];
                }
                answers[i][r] = f64::from(trained.net.forward(&x)[0]);
            }
            let one = quality(data, &answers);
            q.delta += one.delta / repeats as f64;
            q.right += one.right / repeats as f64;
            q.loss += one.loss / repeats as f64;
        }
        let without = mean(g + 1);
        let _ = writeln!(
            w,
            "| {group} | {} | {:+.3} | {:+.1} | {:+.4} | {:+.3} | {:+.1} | {:+.4} |",
            columns.len(),
            q.delta - base.delta,
            100.0 * (q.right - base.right),
            q.loss - base.loss,
            without.delta - full.delta,
            100.0 * (without.right - full.right),
            without.loss - full.loss
        );
    }
    let _ = writeln!(w);
}

/// A few test candidates and the integer net's answers for them.
fn references(q: &Quantised, sets: &[(&Set, f64)]) -> Vec<(Vec<i32>, i64)> {
    let layers = q.layers();
    let net = q.net(&layers);
    let step = (sets.len() / 8).max(1);
    let mut out = [0i64; 1];
    sets.iter()
        .step_by(step)
        .take(8)
        .map(|(s, _)| {
            let row = s.rows[s.taken].clone();
            forward(&net, &row, &mut out);
            (row, out[0])
        })
        .collect()
}

/// A kind's integer net, its references and its margin (Q16).
type Net4 = (Kind, Quantised, Vec<(Vec<i32>, i64)>, i64);

/// Margins of the nets' choice tried on the validation sets.
const MARGINS: [f64; 6] = [0.0, 0.05, 0.1, 0.2, 0.3, 0.5];

fn weights_source(options: &Options, nets: &[Net4]) -> String {
    let mut out = String::new();
    let w = &mut out;
    let hidden: Vec<String> = options.config.hidden.iter().map(|h| h.to_string()).collect();
    let _ = writeln!(w, "// generated by ai-train-choice, do not edit");
    let _ = writeln!(w, "//! The trained weights of the choice net (`ai::choice_net`, run by `ai::net`):");
    for (kind, _, _, _) in nets {
        let _ = writeln!(w, "//! {}: → {} → 1, the outcome {};", kind.name(), hidden.join(" → "), options.targets[usize::from(*kind == Kind::Research)].name());
    }
    let _ = writeln!(w, "//! trained on the explored choices of `ai-choice` games, with the");
    let _ = writeln!(w, "//! integer answers on a few test rows. Reproduced by `ai-train-choice` with the");
    let _ = writeln!(w, "//! same data and options:");
    let flags = command(options);
    let _ = writeln!(w, "//! `{}`.", flags[flags.find("--target-production").unwrap_or(0)..].trim());
    let _ = writeln!(w);
    let _ = writeln!(w, "use super::choice_net::{{Kind, Reference}};");
    let _ = writeln!(w, "use super::net::Net;");
    for kind in [Kind::Production, Kind::Research] {
        let Some((_, q, _, _)) = nets.iter().find(|n| n.0 == kind) else { continue };
        let inputs = match kind {
            Kind::Production => "PRODUCTION_INPUTS",
            Kind::Research => "RESEARCH_INPUTS",
        };
        let _ = writeln!(w, "\nmod {} {{", kind.name());
        let _ = writeln!(w, "use super::super::choice::{inputs};");
        let _ = writeln!(w, "use super::super::net::{{Activation, Layer, Net}};\n");
        let mut widths = vec![inputs.to_owned()];
        for (k, h) in options.config.hidden.iter().enumerate() {
            let _ = writeln!(w, "const H{}: usize = {h};", k + 1);
            widths.push(format!("H{}", k + 1));
        }
        let _ = writeln!(w, "const HEADS: usize = 1;\n");
        widths.push("HEADS".to_owned());
        let widths: Vec<&str> = widths.iter().map(String::as_str).collect();
        w.push_str(&nn::weight_arrays(q, &widths));
        w.push_str(&nn::net_statics(q, "NET"));
        let _ = writeln!(w, "}}");
    }
    let _ = writeln!(w);
    for kind in [Kind::Production, Kind::Research] {
        let upper = kind.name().to_uppercase();
        if nets.iter().any(|n| n.0 == kind) {
            let _ = writeln!(w, "pub static {upper}: Option<&Net<'static>> = Some(&{}::NET);", kind.name());
        } else {
            let _ = writeln!(w, "pub static {upper}: Option<&Net<'static>> = None;");
        }
    }
    for kind in [Kind::Production, Kind::Research] {
        let margin = nets.iter().find(|n| n.0 == kind).map_or(0, |n| n.3);
        let _ = writeln!(w, "pub const {}_MARGIN: i64 = {margin};", kind.name().to_uppercase());
    }
    let _ = writeln!(w, "\npub static REFERENCE: &[Reference] = &[");
    for (kind, _, references, _) in nets {
        for (row, output) in references {
            let values: Vec<String> = row.iter().map(|v| v.to_string()).collect();
            let _ = writeln!(w, "    Reference {{ kind: Kind::{kind:?}, row: &[{}], output: {output} }},", values.join(", "));
        }
    }
    let _ = writeln!(w, "];");
    out
}
