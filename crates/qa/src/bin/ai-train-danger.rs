//! Trains the danger net (`classic_rules::ai::danger_net`) on danger
//! datasets (`ai-danger`), writes its integer weights as generated Rust and
//! a report of its quality on the test games beside the search's estimate.
//!
//! `cargo run --release --offline -p aurelia-qa --bin ai-train-danger -- --data FILE[,FILE..] [options]`
//!
//! --data FILE[,FILE..]   danger CSV files (`ai-danger --out`; required)
//! --out-weights FILE     the generated weights (crates/classic-rules/src/ai/danger_weights.rs)
//! --report FILE          the markdown report
//! --hidden H1,H2         hidden layers (default 32,16)
//! --epochs N             at most N epochs (default 200)
//! --patience N           stop after N epochs without a better validation loss (default 12)
//! --lr X                 Adam's learning rate (default 0.001)
//! --batch N              rows a step (default 256)
//! --huber HP             the damage loss's bend in hit points (default 100: squared error)
//! --death-weight X       the death loss's share (default 1)
//! --seed N               the trainer's random seed (default 1)
//!
//! Only rows with a threat (`threats` > 0) are read: the net is consulted
//! only there. The games and the battles (`ai-danger --battles`, the rows
//! marked `battle`) are split by their seed: `game % 20` below 14 trains,
//! 14..17 validates (early stopping, the death threshold), 17..20 tests (the
//! report, for the games and the battles apart). The same data and options
//! give the same weights, bit for bit.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::Instant;

use aurelia_qa::danger::Row;
use aurelia_qa::nn::{self, Config, Head, Loss, Net, Quantised, Samples};
use classic_rules::ai::danger::{FEATURES, NAMES};
use classic_rules::ai::danger_net::{infer, Danger};
use classic_rules::ai::net::Net as IntNet;

struct Options {
    data: Vec<PathBuf>,
    out_weights: Option<PathBuf>,
    report: Option<PathBuf>,
    config: Config,
}

fn options() -> Result<Options, String> {
    // The heads: the damage / 100 (Huber), the death (logistic).
    let heads = vec![Head { loss: Loss::Huber(1.0), weight: 1.0 }, Head { loss: Loss::Logistic, weight: 1.0 }];
    let mut options = Options { data: Vec::new(), out_weights: None, report: None, config: Config { heads, ..Config::default() } };
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{flag} needs a value"));
        let config = &mut options.config;
        if config.parse_flag(&flag, &mut value)? {
            continue;
        }
        match flag.as_str() {
            "--data" => options.data = value()?.split(',').map(PathBuf::from).collect(),
            "--out-weights" => options.out_weights = Some(PathBuf::from(value()?)),
            "--report" => options.report = Some(PathBuf::from(value()?)),
            "--huber" => config.heads[0].loss = Loss::Huber(value()?.parse::<f32>().map_err(|e| format!("--huber: {e}"))? / 100.0),
            "--death-weight" => config.heads[1].weight = value()?.parse().map_err(|e| format!("--death-weight: {e}"))?,
            other => return Err(format!("unknown option {other}")),
        }
    }
    if options.data.is_empty() {
        return Err("--data FILE is required".to_owned());
    }
    if options.config.hidden.len() != 2 {
        return Err("--hidden H1,H2: the danger net has two hidden layers (`danger_net::Weights`)".to_owned());
    }
    Ok(options)
}

/// The damage head's Huber bend in hit points.
fn huber_hp(config: &Config) -> f32 {
    match config.heads[0].loss {
        Loss::Huber(delta) => delta * 100.0,
        _ => unreachable!("the damage head is a regression"),
    }
}

fn death_weight(config: &Config) -> f32 {
    config.heads[1].weight
}

fn main() {
    if let Err(error) = options().and_then(|options| run(&options)) {
        eprintln!("ai-train-danger: {error}");
        std::process::exit(1);
    }
}

/// The rows with a threat of a danger CSV file; the count of all rows.
fn load(path: &PathBuf, rows: &mut Vec<Row>) -> Result<usize, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut lines = text.lines();
    let header: Vec<&str> = lines.next().ok_or_else(|| format!("{}: empty", path.display()))?.split(',').collect();
    let column = |name: &str| header.iter().position(|h| *h == name).ok_or_else(|| format!("{}: no column {name}", path.display()));
    let features: Vec<usize> = NAMES.iter().map(|n| column(n)).collect::<Result<_, _>>()?;
    let [game, round, player, side, unit, exposure, healed, damage, dead] =
        ["game", "round", "player", "side", "unit", "exposure", "healed", "damage", "dead"].map(column);
    let (game, round, player, side, unit) = (game?, round?, player?, side?, unit?);
    let (exposure, healed, damage, dead) = (exposure?, healed?, damage?, dead?);
    // Files written before the battles have no `battle` column: games only.
    let battle = header.iter().position(|h| *h == "battle");
    let threats = features[NAMES.iter().position(|n| *n == "threats").expect("a threats feature")];
    let mut total = 0;
    let mut cells: Vec<&str> = Vec::with_capacity(header.len());
    for (at, line) in lines.enumerate() {
        total += 1;
        cells.clear();
        cells.extend(line.split(','));
        let bad = |what: &str| format!("{}:{}: bad {what}", path.display(), at + 2);
        let int = |c: usize| cells.get(c).and_then(|s| s.parse::<i64>().ok()).ok_or_else(|| bad(header[c]));
        if int(threats)? <= 0 {
            continue;
        }
        let mut row_features = [0i32; FEATURES];
        for (slot, c) in row_features.iter_mut().zip(&features) {
            *slot = int(*c)? as i32;
        }
        rows.push(Row {
            game: int(game)? as u64,
            battle: battle.map(int).transpose()?.unwrap_or(0) as u8,
            round: int(round)? as u32,
            player: int(player)? as u8,
            side: int(side)? as u8,
            unit: int(unit)? as u64,
            features: row_features,
            exposure: int(exposure)? as i32,
            healed: int(healed)? as i32,
            damage: int(damage)? as i32,
            dead: int(dead)? as i32,
        });
    }
    Ok(total)
}

/// The part of the data a game belongs to.
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

fn samples(rows: &[&Row]) -> Samples {
    let mut out = Samples::new(FEATURES, 2);
    let mut x = [0f32; FEATURES];
    for row in rows {
        for (slot, v) in x.iter_mut().zip(&row.features) {
            *slot = *v as f32;
        }
        out.push(&x, &[row.damage as f32 / 100.0, row.dead as f32], 0);
    }
    out
}

/// A model's answers on rows: damage in hp and death probability.
struct Answers {
    damage: Vec<f64>,
    death: Vec<f64>,
}

fn f32_answers(net: &Net, rows: &[&Row]) -> Answers {
    let mut x = [0f32; FEATURES];
    let (mut damage, mut death) = (Vec::new(), Vec::new());
    for row in rows {
        for (slot, v) in x.iter_mut().zip(&row.features) {
            *slot = *v as f32;
        }
        let (damage_hp, death_p) = f32_answer(net, &x, row.hp());
        damage.push(f64::from(damage_hp));
        death.push(f64::from(death_p));
    }
    Answers { damage, death }
}

/// The f32 net's damage in hit points (clamped to `[0, hp]`) and death
/// probability for a row.
fn f32_answer(net: &Net, x: &[f32], hp: i32) -> (f32, f32) {
    let out = net.forward(x);
    ((out[0] * 100.0).clamp(0.0, (hp as f32).max(0.0)), nn::sigmoid(out[1]))
}

fn int_answers(quantised: &Quantised, rows: &[&Row]) -> Answers {
    let layers = quantised.layers();
    let net = quantised.net(&layers);
    let (mut damage, mut death) = (Vec::new(), Vec::new());
    for row in rows {
        let Danger { damage: d, death_permille } = infer(&net, &row.features);
        damage.push(f64::from(d));
        death.push(f64::from(death_permille) / 1000.0);
    }
    Answers { damage, death }
}

fn mae(rows: &[&Row], predicted: &[f64], keep: impl Fn(&Row) -> bool) -> (usize, f64) {
    let (mut n, mut sum) = (0, 0.0);
    for (row, p) in rows.iter().zip(predicted) {
        if keep(row) {
            n += 1;
            sum += (p - f64::from(row.damage)).abs();
        }
    }
    (n, sum / n.max(1) as f64)
}

fn rmse(rows: &[&Row], predicted: &[f64]) -> f64 {
    let sum: f64 = rows.iter().zip(predicted).map(|(r, p)| (p - f64::from(r.damage)).powi(2)).sum();
    (sum / rows.len().max(1) as f64).sqrt()
}

fn mean(values: &[f64]) -> f64 {
    values.iter().sum::<f64>() / values.len().max(1) as f64
}

/// Precision, recall and F1 of `predicted` (true: dies) against the rows.
fn classify(rows: &[&Row], predicted: impl Iterator<Item = bool>) -> (f64, f64, f64) {
    let (mut tp, mut fp, mut fn_) = (0f64, 0f64, 0f64);
    for (row, p) in rows.iter().zip(predicted) {
        match (p, row.dead > 0) {
            (true, true) => tp += 1.0,
            (true, false) => fp += 1.0,
            (false, true) => fn_ += 1.0,
            (false, false) => {}
        }
    }
    let precision = if tp + fp > 0.0 { tp / (tp + fp) } else { 0.0 };
    let recall = if tp + fn_ > 0.0 { tp / (tp + fn_) } else { 0.0 };
    let f1 = if precision + recall > 0.0 { 2.0 * precision * recall / (precision + recall) } else { 0.0 };
    (precision, recall, f1)
}

/// The threshold (in permille steps) that maximises F1 on the rows.
fn best_threshold(rows: &[&Row], death: &[f64]) -> f64 {
    let mut best = (0.0, 0.5);
    for permille in 1..1000 {
        let t = f64::from(permille) / 1000.0;
        let (_, _, f1) = classify(rows, death.iter().map(|p| *p >= t));
        if f1 > best.0 {
            best = (f1, t);
        }
    }
    best.1
}

fn brier(rows: &[&Row], death: &[f64]) -> f64 {
    let sum: f64 = rows.iter().zip(death).map(|(r, p)| (p - f64::from(r.dead)).powi(2)).sum();
    sum / rows.len().max(1) as f64
}

/// The tenths of the predicted probability (0–0.1, …, 0.9–1): (rows, mean predicted, observed rate).
fn deciles(rows: &[&Row], death: &[f64]) -> Vec<(usize, f64, f64)> {
    let mut bins = vec![(0usize, 0f64, 0f64); 10];
    for (row, p) in rows.iter().zip(death) {
        let bin = &mut bins[((p * 10.0) as usize).min(9)];
        bin.0 += 1;
        bin.1 += p;
        bin.2 += f64::from(row.dead);
    }
    bins.into_iter().map(|(n, p, o)| (n, p / n.max(1) as f64, o / n.max(1) as f64)).collect()
}

fn run(options: &Options) -> Result<(), String> {
    let started = Instant::now();
    let mut rows = Vec::new();
    let mut total = 0;
    for path in &options.data {
        total += load(path, &mut rows)?;
    }
    let of = |p: Part| rows.iter().filter(|r| part(r.game) == p).collect::<Vec<&Row>>();
    let (train_rows, valid_rows, test_rows) = (of(Part::Train), of(Part::Valid), of(Part::Test));
    let games = |set: &[&Row]| {
        let mut g: Vec<(u8, u64)> = set.iter().map(|r| (r.battle, r.game)).collect();
        g.dedup();
        g.sort();
        g.dedup();
        g
    };
    println!(
        "{} rows with a threat of {total}: train {} ({} games and battles), validation {} ({}), test {} ({}); loaded in {:.1} s",
        rows.len(),
        train_rows.len(),
        games(&train_rows).len(),
        valid_rows.len(),
        games(&valid_rows).len(),
        test_rows.len(),
        games(&test_rows).len(),
        started.elapsed().as_secs_f64()
    );
    if train_rows.is_empty() || valid_rows.is_empty() || test_rows.is_empty() {
        return Err("every part needs games (game % 20: 0..14 train, 14..17 validation, 17..20 test)".to_owned());
    }

    let config = &options.config;
    let trained = Instant::now();
    let (net, epochs) = nn::train(&samples(&train_rows), &samples(&valid_rows), config);
    let best_epoch = epochs.iter().enumerate().min_by(|a, b| a.1.valid_loss.total_cmp(&b.1.valid_loss)).map_or(0, |(i, _)| i + 1);
    let train_seconds = trained.elapsed().as_secs_f64();
    for (i, e) in epochs.iter().enumerate() {
        println!("epoch {:3}: train {:.5}, validation {:.5}", i + 1, e.train_loss, e.valid_loss);
    }
    println!("trained in {train_seconds:.1} s, best epoch {best_epoch}");
    let quantised = nn::quantise(&net);

    let valid_f32 = f32_answers(&net, &valid_rows);
    let valid_int = int_answers(&quantised, &valid_rows);
    let test_f32 = f32_answers(&net, &test_rows);
    let test_int = int_answers(&quantised, &test_rows);
    let nanos = time_inference(&quantised, &test_rows);
    println!("integer inference: {nanos:.0} ns a unit");

    let report = report(options, &rows, total, [&train_rows, &valid_rows, &test_rows], &epochs, best_epoch, train_seconds, &quantised, nanos, [&valid_f32, &valid_int], [&test_f32, &test_int]);
    if let Some(path) = &options.report {
        std::fs::write(path, &report).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("report: {}", path.display());
    } else {
        println!("{report}");
    }
    if let Some(path) = &options.out_weights {
        let source = weights_source(options, &quantised, &net, &test_rows, &test_f32, &rows);
        std::fs::write(path, source).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("weights: {}", path.display());
    }
    Ok(())
}

/// Nanoseconds of one integer inference, the mean over repeated passes on the rows.
fn time_inference(quantised: &Quantised, rows: &[&Row]) -> f64 {
    let layers = quantised.layers();
    let net: IntNet<'_> = quantised.net(&layers);
    let mut calls = 0u64;
    let mut sink = 0u64;
    let started = Instant::now();
    while started.elapsed().as_millis() < 500 {
        for row in rows {
            let d = infer(&net, std::hint::black_box(&row.features));
            sink = sink.wrapping_add(u64::from(d.damage) + u64::from(d.death_permille));
            calls += 1;
        }
    }
    std::hint::black_box(sink);
    started.elapsed().as_nanos() as f64 / calls as f64
}

fn command(options: &Options) -> String {
    let c = &options.config;
    let data: Vec<String> = options.data.iter().map(|p| p.display().to_string()).collect();
    format!("ai-train-danger --data {} {}", data.join(","), danger_flags(c))
}

/// The training options as the command line takes them.
fn danger_flags(c: &Config) -> String {
    format!("{} --huber {} --death-weight {} --seed {}", c.flags(), huber_hp(c), death_weight(c), c.seed)
}

#[allow(clippy::too_many_arguments)]
fn report(
    options: &Options,
    rows: &[Row],
    total: usize,
    parts: [&[&Row]; 3],
    epochs: &[nn::Epoch],
    best_epoch: usize,
    train_seconds: f64,
    quantised: &Quantised,
    nanos: f64,
    valid: [&Answers; 2],
    test: [&Answers; 2],
) -> String {
    let [train, valid_rows, test_rows] = parts;
    let c = &options.config;
    let mut out = String::new();
    let w = &mut out;
    let battles = |set: &[&Row]| set.iter().filter(|r| r.battle > 0).count();
    let _ = writeln!(w, "# Danger net: the test games and battles\n");
    let _ = writeln!(
        w,
        "Rows with a threat: {} of {total}. Train {} rows ({} of battles), validation {} ({}), test {} ({}); split by the seed of the game or the battle: \
         `game % 20` < 14, 14..17, 17..20.\n",
        rows.len(),
        train.len(),
        battles(train),
        valid_rows.len(),
        battles(valid_rows),
        test_rows.len(),
        battles(test_rows),
    );
    let _ = writeln!(
        w,
        "Net: {FEATURES} → {} → {} → 2 (ReLU; heads: damage / 100 with Huber δ = {} hp, death logit), Adam lr {}, batch {}, seed {}; \
         {} epochs, the best {best_epoch}, {train_seconds:.0} s. Integer weights: {} parameters, shifts {}/{}/{}. \
         Integer inference: {nanos:.0} ns a unit.\n",
        c.hidden[0],
        c.hidden[1],
        huber_hp(c),
        c.learning_rate,
        c.batch,
        c.seed,
        epochs.len(),
        quantised.parameters(),
        quantised.layers[0].shift,
        quantised.layers[1].shift,
        quantised.layers[2].shift
    );

    let [f32_, int_] = test;
    let thresholds = [best_threshold(valid_rows, &valid[0].death), best_threshold(valid_rows, &valid[1].death)];
    let train_rate = train.iter().filter(|r| r.dead > 0).count() as f64 / train.len() as f64;
    let of_source = |battle: u8| (0..test_rows.len()).filter(|i| test_rows[*i].battle == battle).collect::<Vec<usize>>();
    let subsets = [("the games", of_source(0)), ("the battles", of_source(1)), ("all", (0..test_rows.len()).collect())];
    for (title, indices) in &subsets {
        if indices.is_empty() {
            continue;
        }
        let rows: Vec<&Row> = indices.iter().map(|i| test_rows[*i]).collect();
        test_quality(w, title, &rows, &f32_.pick(indices), &int_.pick(indices), thresholds, train_rate);
    }

    let zero: Vec<f64> = vec![0.0; test_rows.len()];
    let exposure: Vec<f64> = test_rows.iter().map(|r| f64::from(r.exposure)).collect();
    let capped: Vec<f64> = test_rows.iter().map(|r| f64::from(r.exposure.min(r.hp()))).collect();
    let predictors: [(&str, &[f64]); 5] =
        [("0", &zero), ("exposure", &exposure), ("min(exposure, hp)", &capped), ("net f32", &f32_.damage), ("net integer", &int_.damage)];
    let groups = |w: &mut String, title: &str, cuts: &[(&str, &dyn Fn(&Row) -> bool)]| {
        let _ = writeln!(w, "\n## MAE by {title}, all test rows\n");
        let _ = writeln!(w, "| {title} | rows | 0 | exposure | min(exposure, hp) | net f32 | net integer |");
        let _ = writeln!(w, "|---|---:|---:|---:|---:|---:|---:|");
        for (name, keep) in cuts {
            let n = mae(test_rows, &zero, keep).0;
            if n == 0 {
                continue;
            }
            let cells: Vec<String> = predictors.iter().map(|(_, p)| format!("{:.2}", mae(test_rows, p, keep).1)).collect();
            let _ = writeln!(w, "| {name} | {n} | {} |", cells.join(" | "));
        }
    };
    let hp = |a: i32, b: i32| move |r: &Row| (a..=b).contains(&r.hp());
    let (h1, h2, h3, h4) = (hp(1, 25), hp(26, 50), hp(51, 75), hp(76, 100));
    groups(w, "the unit's hp", &[("1–25", &h1), ("26–50", &h2), ("51–75", &h3), ("76–100", &h4)]);
    let side = |s: u8| move |r: &Row| r.side == s;
    let (s0, s1, s2) = (side(0), side(1), side(2));
    groups(w, "side", &[("nation", &s0), ("city-state", &s1), ("barbarians", &s2)]);

    let _ = writeln!(w, "\nCalibration, all test rows: the rows by tenths of the predicted death probability.\n");
    let _ = writeln!(w, "| predicted | f32 rows | f32 predicted | f32 observed | integer rows | integer predicted | integer observed |");
    let _ = writeln!(w, "|---|---:|---:|---:|---:|---:|---:|");
    for (d, (a, b)) in deciles(test_rows, &f32_.death).iter().zip(deciles(test_rows, &int_.death)).enumerate() {
        let _ = writeln!(w, "| {:.1}–{:.1} | {} | {:.4} | {:.4} | {} | {:.4} | {:.4} |", d as f64 / 10.0, (d + 1) as f64 / 10.0, a.0, a.1, a.2, b.0, b.1, b.2);
    }

    let mut damage_gap = 0f64;
    let mut differ = 0;
    let mut death_gap = 0f64;
    for i in 0..test_rows.len() {
        damage_gap = damage_gap.max((f32_.damage[i] - int_.damage[i]).abs());
        if f32_.damage[i].round() != int_.damage[i] {
            differ += 1;
        }
        death_gap = death_gap.max((f32_.death[i] - int_.death[i]).abs() * 1000.0);
    }
    let _ = writeln!(w, "\n## Integer inference against f32\n");
    let _ = writeln!(
        w,
        "On the {} test rows: damage differs by at most {damage_gap:.3} hp (the integer one is rounded; {differ} rows differ from the rounded f32), \
         death by at most {death_gap:.2} permille.",
        test_rows.len()
    );

    let _ = writeln!(w, "\n## Reproduce\n");
    let _ = writeln!(w, "```\ncargo +1.98.1 run --release --offline -p aurelia-qa --bin ai-train-danger -- {} --out-weights crates/classic-rules/src/ai/danger_weights.rs --report REPORT.md\n```", command(options).trim_start_matches("ai-train-danger "));
    let _ = writeln!(w, "\n<details><summary>Training log</summary>\n\n| epoch | train loss | validation loss |\n|---:|---:|---:|");
    for (i, e) in epochs.iter().enumerate() {
        let _ = writeln!(w, "| {} | {:.5} | {:.5} |", i + 1, e.train_loss, e.valid_loss);
    }
    let _ = writeln!(w, "\n</details>");
    out
}

impl Answers {
    /// The answers of the rows at `indices`.
    fn pick(&self, indices: &[usize]) -> Answers {
        Answers { damage: indices.iter().map(|i| self.damage[*i]).collect(), death: indices.iter().map(|i| self.death[*i]).collect() }
    }
}

/// The quality of the nets on some test rows beside the baselines: the
/// damage (predicting 0, the search's exposure, the exposure capped at the
/// unit's health), the death (exposure ≥ hp; the nets at the thresholds
/// chosen on the validation rows and at 0.5), the Brier score.
fn test_quality(w: &mut String, title: &str, rows: &[&Row], f32_: &Answers, int_: &Answers, thresholds: [f64; 2], train_rate: f64) {
    let actual: Vec<f64> = rows.iter().map(|r| f64::from(r.damage)).collect();
    let damaged = rows.iter().filter(|r| r.damage > 0).count() as f64 / rows.len() as f64;
    let dead = rows.iter().filter(|r| r.dead > 0).count() as f64 / rows.len() as f64;
    let _ = writeln!(w, "## Test: {title}\n");
    let _ = writeln!(w, "{} rows: {:.1} % damaged, {:.2} % dead, mean damage {:.2} hp.\n", rows.len(), damaged * 100.0, dead * 100.0, mean(&actual));
    let zero: Vec<f64> = vec![0.0; rows.len()];
    let exposure: Vec<f64> = rows.iter().map(|r| f64::from(r.exposure)).collect();
    let capped: Vec<f64> = rows.iter().map(|r| f64::from(r.exposure.min(r.hp()))).collect();
    let predictors: [(&str, &[f64]); 5] =
        [("0", &zero), ("exposure", &exposure), ("min(exposure, hp)", &capped), ("net f32", &f32_.damage), ("net integer", &int_.damage)];
    let _ = writeln!(w, "| damage predictor | MAE | RMSE | mean (actual {:.2}) |", mean(&actual));
    let _ = writeln!(w, "|---|---:|---:|---:|");
    for (name, p) in predictors {
        let _ = writeln!(w, "| {name} | {:.2} | {:.2} | {:.2} |", mae(rows, p, |_| true).1, rmse(rows, p), mean(p));
    }
    let _ = writeln!(w, "\n| death predictor | threshold | precision | recall | F1 |");
    let _ = writeln!(w, "|---|---:|---:|---:|---:|");
    let (p, r, f) = classify(rows, rows.iter().map(|r| r.exposure >= r.hp()));
    let _ = writeln!(w, "| exposure ≥ hp | – | {p:.3} | {r:.3} | {f:.3} |");
    for ((name, answers), best) in [("net f32", f32_), ("net integer", int_)].into_iter().zip(thresholds) {
        for (label, t) in [(format!("{best:.3} (best F1 on validation)"), best), ("0.500".to_owned(), 0.5)] {
            let (p, r, f) = classify(rows, answers.death.iter().map(|d| *d >= t));
            let _ = writeln!(w, "| {name} | {label} | {p:.3} | {r:.3} | {f:.3} |");
        }
    }
    let _ = writeln!(
        w,
        "\nBrier score: the training rate ({train_rate:.4}) {:.4}, net f32 {:.4}, net integer {:.4}.\n",
        brier(rows, &vec![train_rate; rows.len()]),
        brier(rows, &f32_.death),
        brier(rows, &int_.death)
    );
}

/// The test rows kept as references: 24 rows spread over the f32 net's damage.
fn references(rows: &[&Row], answers: &Answers) -> Vec<usize> {
    let mut order: Vec<usize> = (0..rows.len()).collect();
    order.sort_by(|a, b| answers.damage[*a].total_cmp(&answers.damage[*b]).then(answers.death[*a].total_cmp(&answers.death[*b])).then(a.cmp(b)));
    let count = 24.min(order.len());
    (0..count).map(|k| order[(order.len() - 1) * k / (count - 1).max(1)]).collect()
}

fn weights_source(options: &Options, q: &Quantised, net: &Net, test_rows: &[&Row], test_f32: &Answers, rows: &[Row]) -> String {
    let c = &options.config;
    let span = |battle: u8| rows.iter().filter(|r| r.battle == battle).fold(None, |s: Option<(u64, u64)>, r| Some(s.map_or((r.game, r.game), |(a, b)| (a.min(r.game), b.max(r.game)))));
    let mut sources = Vec::new();
    if let Some((first, last)) = span(0) {
        sources.push(format!("games {first}..={last} (`ai-danger --size small --rounds 200`)"));
    }
    if let Some((first, last)) = span(1) {
        sources.push(format!("battles {first}..={last} (`ai-danger --battles --size small --rounds 30`)"));
    }
    let mut out = String::new();
    let w = &mut out;
    let _ = writeln!(w, "// generated by ai-train-danger, do not edit");
    let _ = writeln!(w, "//! The trained weights of the danger net (`ai::danger_net`, its fixed");
    let _ = writeln!(w, "//! point): {FEATURES} → {} → {} → 2, trained on the rows with a threat of", c.hidden[0], c.hidden[1]);
    for (i, source) in sources.iter().enumerate() {
        let _ = writeln!(w, "//! {source}{}", if i + 1 < sources.len() { " and" } else { ", and" });
    }
    let _ = writeln!(w, "//! the f32 net's answers on a few test rows. Reproduced by");
    let _ = writeln!(w, "//! `ai-train-danger` with the same data and options:");
    let _ = writeln!(w, "//! `{}`.", danger_flags(c));
    let _ = writeln!(w);
    let _ = writeln!(w, "use super::danger::FEATURES;");
    let _ = writeln!(w, "use super::danger_net::{{Reference, Weights}};");
    let _ = writeln!(w);
    let _ = writeln!(w, "const H1: usize = {};", c.hidden[0]);
    let _ = writeln!(w, "const H2: usize = {};", c.hidden[1]);
    let _ = writeln!(w);
    w.push_str(&nn::weight_arrays(q, &["FEATURES", "H1", "H2", "2"]));
    let _ = writeln!(
        w,
        "pub static WEIGHTS: Weights<'static> = Weights {{\n    low: &LOW,\n    high: &HIGH,\n    w1: &W1,\n    b1: &B1,\n    shift1: {},\n    w2: &W2,\n    b2: &B2,\n    shift2: {},\n    w3: &W3,\n    b3: &B3,\n    shift3: {},\n}};\n",
        q.layers[0].shift, q.layers[1].shift, q.layers[2].shift
    );
    let picked = references(test_rows, test_f32);
    let _ = writeln!(w, "pub static REFERENCE: [Reference; {}] = [", picked.len());
    for i in picked {
        let row = test_rows[i];
        let features: Vec<String> = row.features.iter().map(|v| v.to_string()).collect();
        let mut x = [0f32; FEATURES];
        for (slot, v) in x.iter_mut().zip(&row.features) {
            *slot = *v as f32;
        }
        let (damage_hp, death_p) = f32_answer(net, &x, row.hp());
        let damage_milli = (f64::from(damage_hp) * 1000.0).round() as i64;
        let death_ppm = (f64::from(death_p) * 1e6).round() as i64;
        let _ = writeln!(w, "    Reference {{ features: [{}], damage_milli: {damage_milli}, death_ppm: {death_ppm} }},", features.join(", "));
    }
    let _ = writeln!(w, "];");
    out
}
