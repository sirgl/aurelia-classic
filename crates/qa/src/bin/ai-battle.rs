//! Synthetic battles (`aurelia_qa::battle`): each seed's game is played by
//! the game's AI to its start round, two neighbouring nations go to war with
//! armies at their border, and the AI plays the battle on; per battle the
//! attacks, the losses, the captures and the share of military unit-turns
//! that took damage in the enemies' turn.
//!
//! `cargo run --release --offline -p aurelia-qa --bin ai-battle -- [options]`
//!
//! --seeds A..B         the seeds, B excluded (default 0..20); or one seed
//! --rounds N           rounds played after the war begins (default 30)
//! --size NAME          the world size (default small)
//! --start N            the round the war begins at (default: 60-160 by seed)
//! --army N             units of each army (default 12)
//! --reinforce N        lost army units come back every N rounds
//!                      (default 5; 0: never)
//! --let-peace          a peace the sides make ends the battle's war (by
//!                      default the war is forced again)
//! --out FILE           one JSON line per battle
//! --record FILE        record the battles (`aurelia_qa::record`, sights on
//!                      every round); with several seeds `-seed<N>` is
//!                      added to the file's name
//! --candidate X        head to head: each seed is played twice, X on the
//!                      battle's side A and the baseline on side B and every
//!                      other seat, then the sides swapped
//!                      (`kernel::ai::seats::Seated`); the report is X's
//! --baseline Y         the other side (default `standard`)
//! --outrangers N       N ranged slots of each army are the cheapest land
//!                      unit whose range exceeds a city's strike range
//!                      (`battle::outranger_kind`): a siege by artillery
//! --jobs N             head to head: seeds played in parallel (default 1;
//!                      the report is the same)
//!
//! Head to head the report gives per seed and in all the candidate's and
//! the baseline's military units killed in the strikes between the two
//! sides, units and hit points lost in the enemies' turns and cities
//! captured, and the candidate's share of the kills, killed / (killed +
//! lost), each kill credited to the side whose strike or defence made it
//! (`BattleStats::killed`: an attacker that dies in its own turn counts, the
//! losses to third players do not), with a bootstrap 95 % interval over the
//! seeds; the share by the losses in the enemies' turns (the measure before
//! 31 October, blind to attackers that die striking) beside it; the fight
//! audit of both sides (`battle::Tally`, classic/docs/ai-combat-diagnosis.md);
//! then the cost of the danger net's and the tactical plan's decisions in
//! the run. Without a candidate the run prints the audit of sides A and B.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

use aurelia_qa::battle::{new_battle, Battle, BattleSpec, BattleStats, FightAudit, Tally, TASK_WINDOW, TASK_WINDOWS};
use aurelia_qa::bench::modules::{BenchRules, ClassicBench};
use aurelia_qa::record::{end_record, outcome_record, GameRecord, Record, Recording};
use aurelia_qa::spec::{size_from_name, size_name};
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::variant::Variant;
use classic_rules::CompiledRuleset;
use kernel::ai::driver::AiDriver;
use kernel::ai::mind::AiMind;
use kernel::ai::seats::Seated;
use kernel::ai::trace::DecisionTrace;
use classic_rules::classic::Classic;
use serde::Serialize;

struct Options {
    seeds: std::ops::Range<u64>,
    rounds: u32,
    size: String,
    start: Option<u32>,
    army: Option<usize>,
    let_peace: bool,
    reinforce: Option<u32>,
    out: Option<PathBuf>,
    record: Option<PathBuf>,
    candidate: Option<String>,
    baseline: String,
    outrangers: usize,
    jobs: usize,
}

fn options() -> Result<Options, String> {
    let mut options = Options { seeds: 0..20, rounds: 30, size: "small".to_owned(), start: None, army: None, let_peace: false, reinforce: None, out: None, record: None, candidate: None, baseline: "standard".to_owned(), outrangers: 0, jobs: 1 };
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--seeds" => options.seeds = seeds(&value()?)?,
            "--rounds" => options.rounds = value()?.parse().map_err(|e| format!("--rounds: {e}"))?,
            "--size" => options.size = value()?,
            "--start" => options.start = Some(value()?.parse().map_err(|e| format!("--start: {e}"))?),
            "--army" => options.army = Some(value()?.parse().map_err(|e| format!("--army: {e}"))?),
            "--let-peace" => options.let_peace = true,
            "--reinforce" => options.reinforce = Some(value()?.parse().map_err(|e| format!("--reinforce: {e}"))?),
            "--out" => options.out = Some(PathBuf::from(value()?)),
            "--record" => options.record = Some(PathBuf::from(value()?)),
            "--candidate" => options.candidate = Some(value()?),
            "--baseline" => options.baseline = value()?,
            "--outrangers" => options.outrangers = value()?.parse().map_err(|e| format!("--outrangers: {e}"))?,
            "--jobs" => options.jobs = value()?.parse::<usize>().map_err(|e| format!("--jobs: {e}"))?.max(1),
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(options)
}

/// `A..B` or one seed `A`.
fn seeds(text: &str) -> Result<std::ops::Range<u64>, String> {
    let parse = |s: &str| s.trim().parse::<u64>().map_err(|e| format!("--seeds {text}: {e}"));
    match text.split_once("..") {
        Some((a, b)) => Ok(parse(a)?..parse(b)?),
        None => {
            let a = parse(text)?;
            Ok(a..a + 1)
        }
    }
}

fn main() {
    let result = options().and_then(|options| match &options.candidate {
        Some(candidate) => head_to_head(&options, candidate),
        None => run(&options),
    });
    if let Err(error) = result {
        eprintln!("ai-battle: {error}");
        std::process::exit(1);
    }
}

/// One battle's line of the `--out` file.
#[derive(Serialize)]
struct Line {
    seed: u64,
    size: &'static str,
    start_round: u32,
    sides: [u8; 2],
    army: [Vec<String>; 2],
    rounds: u32,
    attacks: [u32; 2],
    attacks_on_enemy: [u32; 2],
    lost: [u32; 2],
    captured: [u32; 2],
    peace_rounds: u32,
    unit_turns: [u32; 2],
    damaged: [u32; 2],
    damaged_share: f64,
    hash: String,
    seconds: f64,
}

fn run(options: &Options) -> Result<(), String> {
    let size = size_from_name(&options.size).ok_or_else(|| format!("unknown world size {}", options.size))?;
    let rules = ClassicBench::rules();
    let mut out = match &options.out {
        Some(path) => Some(std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?),
        None => None,
    };
    println!("| seed | start | sides | army A | attacks A / B (on enemy) | lost A / B | captured A / B | peace rounds | damaged unit-turns | share | s |");
    println!("|---|---|---|---|---|---|---|---|---|---|---|");
    let mut total = BattleStats::default();
    let mut engaged = 0;
    let mut both = 0;
    let mut battles = 0;
    let mut peace = 0;
    let started = Instant::now();
    for seed in options.seeds.clone() {
        let spec = spec_of(options, seed, size);
        let clock = Instant::now();
        let mut battle = match new_battle(&rules, &spec) {
            Ok(battle) => battle,
            Err(error) => {
                println!("| {seed} | {} | - | {error} | | | | | | | |", spec.start_round);
                continue;
            }
        };
        let stats = match &options.record {
            Some(path) => {
                let path = record_path(path, seed, options.seeds.end - options.seeds.start > 1);
                record(&rules, &mut battle, &spec, options.rounds, &path)?
            }
            None => {
                let mut driver = battle.driver(&rules, ClassicMind::new(Variant::Standard))?;
                battle.play(&rules, &mut driver, options.rounds, |_| {})?
            }
        };
        let seconds = clock.elapsed().as_secs_f64();
        let names = |i: usize| -> Vec<String> {
            battle.armies[i].kinds.iter().map(|k| rules.units[k.0 as usize].key.rsplit('/').next().unwrap_or("?").to_owned()).collect()
        };
        println!(
            "| {seed} | {} | {} v {} | {} | {} / {} ({} / {}) | {} / {} | {} / {} | {} | {} of {} | {:.1} % | {seconds:.1} |",
            spec.start_round,
            battle.sides[0].0,
            battle.sides[1].0,
            names(0).join(" "),
            stats.attacks[0],
            stats.attacks[1],
            stats.attacks_on_enemy[0],
            stats.attacks_on_enemy[1],
            stats.lost[0],
            stats.lost[1],
            stats.captured[0],
            stats.captured[1],
            stats.peace_rounds,
            stats.damaged[0] + stats.damaged[1],
            stats.unit_turns[0] + stats.unit_turns[1],
            100.0 * stats.damaged_share(),
        );
        if let Some(file) = out.as_mut() {
            let line = Line {
                seed,
                size: size_name(size),
                start_round: spec.start_round,
                sides: [battle.sides[0].0, battle.sides[1].0],
                army: [names(0), names(1)],
                rounds: stats.rounds,
                attacks: stats.attacks,
                attacks_on_enemy: stats.attacks_on_enemy,
                lost: stats.lost,
                captured: stats.captured,
                peace_rounds: stats.peace_rounds,
                unit_turns: stats.unit_turns,
                damaged: stats.damaged,
                damaged_share: stats.damaged_share(),
                hash: format!("{:016x}", stats.hash),
                seconds,
            };
            let text = serde_json::to_string(&line).map_err(|e| e.to_string())?;
            writeln!(file, "{text}").map_err(|e| e.to_string())?;
        }
        battles += 1;
        peace += stats.peace_rounds;
        engaged += u32::from(stats.attacks_on_enemy.iter().sum::<u32>() > 0);
        both += u32::from(stats.attacks_on_enemy.iter().all(|a| *a > 0));
        for i in 0..2 {
            total.attacks[i] += stats.attacks[i];
            total.attacks_on_enemy[i] += stats.attacks_on_enemy[i];
            total.lost[i] += stats.lost[i];
            total.captured[i] += stats.captured[i];
            total.unit_turns[i] += stats.unit_turns[i];
            total.damaged[i] += stats.damaged[i];
            total.killed[i] += stats.killed[i];
            total.dealt[i] += stats.dealt[i];
            total.audit[i] = total.audit[i].plus(stats.audit[i]);
        }
    }
    println!();
    println!(
        "{battles} battles, {:.0} s, {peace} rounds ended at peace: damaged unit-turns {} of {} = {:.1} %; attacks on the enemy in {engaged}, by both sides in {both}; attacks A / B {} / {} (on enemy {} / {}); lost {} / {}; captured {} / {}",
        started.elapsed().as_secs_f64(),
        total.damaged[0] + total.damaged[1],
        total.unit_turns[0] + total.unit_turns[1],
        100.0 * total.damaged_share(),
        total.attacks[0],
        total.attacks[1],
        total.attacks_on_enemy[0],
        total.attacks_on_enemy[1],
        total.lost[0],
        total.lost[1],
        total.captured[0],
        total.captured[1],
    );
    println!();
    println!("| fight audit | side A | side B |");
    println!("|---|---:|---:|");
    println!("| enemy units killed in the strikes between the sides | {} | {} |", total.killed[0], total.killed[1]);
    println!("| hit points dealt in them | {} | {} |", total.dealt[0], total.dealt[1]);
    for tally in Tally::ALL {
        println!("| {} | {} | {} |", tally.label(), total.audit[0].get(tally), total.audit[1].get(tally));
    }
    Ok(())
}

/// `FILE` for one seed; `FILE` with `-seed<N>` before its extension for many.
fn record_path(path: &Path, seed: u64, many: bool) -> PathBuf {
    if !many {
        return path.to_path_buf();
    }
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("battle");
    let name = match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => format!("{stem}-seed{seed}.{ext}"),
        None => format!("{stem}-seed{seed}"),
    };
    path.with_file_name(name)
}

/// The battle played through the recording mind: the game line, the
/// outcomes after every round, the end line.
fn record(rules: &CompiledRuleset, battle: &mut Battle, spec: &BattleSpec, rounds: u32, path: &Path) -> Result<BattleStats, String> {
    let recording = Recording::to_file(ClassicMind::new(Variant::Standard), path, 1).map_err(|e| format!("{}: {e}", path.display()))?;
    recording.write(&Record::Game(GameRecord {
        seed: spec.seed,
        map: spec.script.key().to_owned(),
        size: size_name(spec.size).to_owned(),
        rounds: battle.round + rounds,
        players: battle.game.view().auth.players().count() as u32,
        variant: Variant::Standard.name().to_owned(),
    }));
    let mut driver = battle.driver(rules, recording)?;
    driver.set_trace_capacity(usize::MAX);
    let stats = battle.play(rules, &mut driver, rounds, write_outcomes)?;
    let recording = driver.mind_mut();
    recording.write(&Record::End(end_record(&battle.game, rules)));
    recording.flush().map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(stats)
}

/// The outcome lines of the round's trace, indexed within each player's
/// turn; the trace starts afresh for the next round.
fn write_outcomes<P>(driver: &mut AiDriver<Classic, Recording<P>>)
where
    Recording<P>: AiMind<Classic>,
{
    let records = outcomes(driver.trace());
    let recording = driver.mind_mut();
    for record in &records {
        recording.write(record);
    }
    let _ = recording.flush();
    driver.set_trace_capacity(usize::MAX);
}

fn outcomes(trace: &DecisionTrace<Classic>) -> Vec<Record> {
    let mut turn = None;
    let mut index = 0;
    trace
        .entries()
        .map(|entry| {
            if turn != Some((entry.round, entry.player)) {
                turn = Some((entry.round, entry.player));
                index = 0;
            }
            let record = Record::Outcome(outcome_record(entry, index));
            index += 1;
            record
        })
        .collect()
}

/// The battle of a seed as the options set it.
fn spec_of(options: &Options, seed: u64, size: aurelia_qa::spec::WorldSize) -> BattleSpec {
    let mut spec = BattleSpec::new(seed, size).hold_war(!options.let_peace);
    if let Some(start) = options.start {
        spec = spec.start_round(start);
    }
    if let Some(army) = options.army {
        spec = spec.army(army);
    }
    if let Some(rounds) = options.reinforce {
        spec = spec.reinforce_every(rounds);
    }
    spec.outrangers(options.outrangers)
}

/// One battle with the candidate on `side` (0: A, the declarer) and the
/// baseline on the other and every other seat.
fn seat_battle(rules: &CompiledRuleset, spec: &BattleSpec, rounds: u32, candidate: Variant, baseline: Variant, side: usize) -> Result<Exchange, String> {
    let mut battle = new_battle(rules, spec)?;
    let mind = Seated::new(ClassicMind::new(baseline), vec![(battle.sides[side], ClassicMind::new(candidate))]);
    let mut driver = battle.driver(rules, mind)?;
    driver.set_trace_capacity(usize::MAX);
    // Turns whose decision budget ran out (`Reason::TurnBudgetExhausted`), per side.
    let mut exhausted = [0u32; 2];
    let sides = battle.sides;
    let stats = battle.play(rules, &mut driver, rounds, |driver| {
        for entry in driver.trace().entries() {
            if entry.reason == kernel::ai::trace::Reason::TurnBudgetExhausted {
                if let Some(at) = sides.iter().position(|p| *p == entry.player) {
                    exhausted[at] += 1;
                }
            }
        }
        driver.set_trace_capacity(usize::MAX);
    })?;
    let other = 1 - side;
    Ok(Exchange {
        lost: [stats.lost[side], stats.lost[other]],
        hp_lost: [stats.hp_lost[side], stats.hp_lost[other]],
        captured: [stats.captured[side], stats.captured[other]],
        attacks: [stats.attacks_on_enemy[side], stats.attacks_on_enemy[other]],
        city_attacks: [stats.city_attacks[side], stats.city_attacks[other]],
        assault: [stats.assault[side], stats.assault[other]],
        army_rounds: [stats.army_rounds[side], stats.army_rounds[other]],
        garrison_rounds: [stats.garrison_rounds[side], stats.garrison_rounds[other]],
        killed: [stats.killed[side], stats.killed[other]],
        dealt: [stats.dealt[side], stats.dealt[other]],
        audit: [stats.audit[side], stats.audit[other]],
        battles: 1,
        exhausted: [exhausted[side], exhausted[other]],
    })
}

/// `work` on every item, `jobs` at a time, the results in the items' order.
fn parallel<T: Sync, R: Send>(items: &[T], jobs: usize, work: impl Fn(T) -> R + Sync) -> Vec<R>
where
    T: Copy,
{
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results: std::sync::Mutex<Vec<Option<R>>> = std::sync::Mutex::new((0..items.len()).map(|_| None).collect());
    std::thread::scope(|scope| {
        for _ in 0..jobs.min(items.len()).max(1) {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some(item) = items.get(i) else { break };
                let result = work(*item);
                results.lock().expect("a battle thread panicked")[i] = Some(result);
            });
        }
    });
    results.into_inner().expect("a battle thread panicked").into_iter().map(|r| r.expect("every seed played")).collect()
}

/// One head-to-head battle or a sum of them, the candidate first.
#[derive(Debug, Clone, Copy, Default)]
struct Exchange {
    lost: [u32; 2],
    hp_lost: [u32; 2],
    captured: [u32; 2],
    attacks: [u32; 2],
    /// Strikes on the other side's cities.
    city_attacks: [u32; 2],
    /// Army units on an assault or a marching operation, summed over the rounds of each window.
    assault: [[u32; TASK_WINDOWS]; 2],
    /// Army unit-rounds alive, and of them holding a garrison.
    army_rounds: [u32; 2],
    garrison_rounds: [u32; 2],
    /// Enemy units killed and hit points dealt in the strikes between the
    /// sides (`BattleStats::killed`, `dealt`).
    killed: [u32; 2],
    dealt: [u32; 2],
    audit: [FightAudit; 2],
    /// Battles summed (each side's seat counts one).
    battles: u32,
    /// Turns whose decision budget ran out before the turn's end.
    exhausted: [u32; 2],
}

impl Exchange {
    fn plus(self, o: Exchange) -> Exchange {
        let add = |a: [u32; 2], b: [u32; 2]| [a[0] + b[0], a[1] + b[1]];
        let windows = |a: [u32; TASK_WINDOWS], b: [u32; TASK_WINDOWS]| std::array::from_fn(|i| a[i] + b[i]);
        Exchange {
            lost: add(self.lost, o.lost),
            hp_lost: add(self.hp_lost, o.hp_lost),
            captured: add(self.captured, o.captured),
            attacks: add(self.attacks, o.attacks),
            city_attacks: add(self.city_attacks, o.city_attacks),
            assault: [windows(self.assault[0], o.assault[0]), windows(self.assault[1], o.assault[1])],
            army_rounds: add(self.army_rounds, o.army_rounds),
            garrison_rounds: add(self.garrison_rounds, o.garrison_rounds),
            killed: add(self.killed, o.killed),
            dealt: add(self.dealt, o.dealt),
            audit: [self.audit[0].plus(o.audit[0]), self.audit[1].plus(o.audit[1])],
            battles: self.battles + o.battles,
            exhausted: add(self.exhausted, o.exhausted),
        }
    }
}

/// The candidate's share of the units killed in the strikes between the
/// sides: killed / (killed + lost), both credited by the strike that killed
/// (`BattleStats::killed`: an attacker that dies in its own turn counts,
/// the losses to third players do not).
fn share(e: &Exchange) -> f64 {
    ratio(e.killed[0], e.killed[1])
}

/// The share as it was measured before 31 October: the units each side
/// lost in the enemies' turns (`BattleStats::lost`), whoever killed them; an
/// attacker that died in its own turn was nobody's loss.
fn share_of_losses(e: &Exchange) -> f64 {
    ratio(e.lost[1], e.lost[0])
}

fn ratio(ours: u32, theirs: u32) -> f64 {
    if ours + theirs == 0 {
        0.5
    } else {
        f64::from(ours) / f64::from(ours + theirs)
    }
}

/// The 95 % interval of the pooled share over the seeds: 10,000 resamples
/// of the seeds with replacement (a fixed generator: the same seeds give
/// the same interval).
fn bootstrap(seeds: &[Exchange], share: fn(&Exchange) -> f64) -> (f64, f64) {
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let mut shares: Vec<f64> = (0..10_000)
        .map(|_| {
            let sum = (0..seeds.len()).fold(Exchange::default(), |acc, _| acc.plus(seeds[(next() % seeds.len() as u64) as usize]));
            share(&sum)
        })
        .collect();
    shares.sort_by(f64::total_cmp);
    (shares[249], shares[9_749])
}

fn head_to_head(options: &Options, candidate: &str) -> Result<(), String> {
    let size = size_from_name(&options.size).ok_or_else(|| format!("unknown world size {}", options.size))?;
    let names = ClassicBench::variants().join(", ");
    let variant = |name: &str| Variant::from_name(name).ok_or_else(|| format!("unknown variant {name} ({names})"));
    let (candidate_variant, baseline_variant) = (variant(candidate)?, variant(&options.baseline)?);
    let rules = ClassicBench::rules();
    let started = Instant::now();
    println!("## `{candidate}` against `{}`: {} rounds a battle, each seed with the sides swapped\n", options.baseline, options.rounds);
    println!("| seed | start | killed C / B | lost C / B | hp lost C / B | captured C / B | attacks on the enemy C / B | share C | s |");
    println!("|---|---|---|---|---|---|---|---|---|");
    let mut seeds: Vec<Exchange> = Vec::new();
    let mut skipped = Vec::new();
    let all: Vec<u64> = options.seeds.clone().collect();
    let played = parallel(&all, options.jobs, |seed| {
        let spec = spec_of(options, seed, size);
        let clock = Instant::now();
        let both = (0..2).map(|side| seat_battle(&rules, &spec, options.rounds, candidate_variant, baseline_variant, side)).collect::<Result<Vec<_>, _>>();
        (spec, both, clock.elapsed())
    });
    for (seed, (spec, both, elapsed)) in all.iter().copied().zip(played) {
        let exchange = match both {
            Ok(both) => both[0].plus(both[1]),
            Err(error) => {
                println!("| {seed} | {} | {error} | | | | | | |", spec.start_round);
                skipped.push(seed);
                continue;
            }
        };
        let e = &exchange;
        println!(
            "| {seed} | {} | {} / {} | {} / {} | {} / {} | {} / {} | {} / {} | {:.3} | {:.1} |",
            spec.start_round,
            e.killed[0],
            e.killed[1],
            e.lost[0],
            e.lost[1],
            e.hp_lost[0],
            e.hp_lost[1],
            e.captured[0],
            e.captured[1],
            e.attacks[0],
            e.attacks[1],
            share(e),
            elapsed.as_secs_f64()
        );
        seeds.push(exchange);
    }
    if seeds.is_empty() {
        return Err("no battle could be set".to_owned());
    }
    let total = seeds.iter().fold(Exchange::default(), |a, e| a.plus(*e));
    let (low, high) = bootstrap(&seeds, share);
    let (old_low, old_high) = bootstrap(&seeds, share_of_losses);
    println!();
    println!("| `{candidate}` against `{}` | candidate | baseline |", options.baseline);
    println!("|---|---:|---:|");
    println!("| enemy units killed in the strikes between the sides | {} | {} |", total.killed[0], total.killed[1]);
    println!("| hit points dealt in them | {} | {} |", total.dealt[0], total.dealt[1]);
    println!("| military units lost in the enemies' turns (to anyone) | {} | {} |", total.lost[0], total.lost[1]);
    println!("| hit points lost in the enemies' turns | {} | {} |", total.hp_lost[0], total.hp_lost[1]);
    println!("| cities captured | {} | {} |", total.captured[0], total.captured[1]);
    println!("| attacks on the enemy | {} | {} |", total.attacks[0], total.attacks[1]);
    println!("| strikes on enemy cities | {} | {} |", total.city_attacks[0], total.city_attacks[1]);
    println!("| turns whose decision budget ran out | {} | {} |", total.exhausted[0], total.exhausted[1]);
    let percent = |part: u32, whole: u32| 100.0 * f64::from(part) / f64::from(whole.max(1));
    println!("| army unit-rounds holding a garrison | {:.1} % | {:.1} % |", percent(total.garrison_rounds[0], total.army_rounds[0]), percent(total.garrison_rounds[1], total.army_rounds[1]));
    for window in 0..TASK_WINDOWS {
        let first = window as u32 * TASK_WINDOW + 1;
        let rounds = if window + 1 == TASK_WINDOWS { format!("{first}+") } else { format!("{first}–{}", first + TASK_WINDOW - 1) };
        // Units on an assault a round of the window, per battle.
        let per_round = |side: usize| f64::from(total.assault[side][window]) / f64::from(TASK_WINDOW * total.battles.max(1));
        println!("| army units on an assault a round, rounds {rounds} | {:.2} | {:.2} |", per_round(0), per_round(1));
    }
    for tally in Tally::ALL {
        println!("| {} | {} | {} |", tally.label(), total.audit[0].get(tally), total.audit[1].get(tally));
    }
    println!();
    println!(
        "{} seeds × 2 battles ({} skipped{}), {:.0} s. The candidate's share killed / (killed + lost) in the strikes between the sides: **{:.3}** (95 % CI {:.3}–{:.3}, bootstrap over seeds); equal versions 0.500. By the losses in the enemies' turns (the measure before 31 October): {:.3} ({:.3}–{:.3}).",
        seeds.len(),
        skipped.len(),
        if skipped.is_empty() { String::new() } else { format!(": {skipped:?}") },
        started.elapsed().as_secs_f64(),
        share(&total),
        low,
        high,
        share_of_losses(&total),
        old_low,
        old_high
    );
    let (builds, build_ns, rows, row_ns) = classic_rules::ai::danger::net_stats();
    let (plans, plan_ns, plan_max_ns, evaluations, _) = classic_rules::ai::tactical::plan_stats();
    println!(
        "Cost: danger-net threats built {builds} ({:.1} us each), rows read with the net {rows} ({:.2} us each); tactical plans {plans} ({:.2} ms each, longest {:.1} ms, {:.0} evaluations each).",
        build_ns as f64 / builds.max(1) as f64 / 1e3,
        row_ns as f64 / rows.max(1) as f64 / 1e3,
        plan_ns as f64 / plans.max(1) as f64 / 1e6,
        plan_max_ns as f64 / 1e6,
        evaluations as f64 / plans.max(1) as f64
    );
    Ok(())
}
