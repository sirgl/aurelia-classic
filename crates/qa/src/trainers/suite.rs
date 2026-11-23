//! The suite: every (family, era, instance) proven winnable, then played by the players.
//!
//! THE PROOF AND THE REDRAW RULE (fixed before the first run): instance i of a family in an era is
//! drawn as attempt 0, 1, ... (`families::seed(family, era, i, attempt)`) up to `--attempts` (12). An
//! attempt is KEPT when it builds, one of the family's solvers wins it (`Fam::solvers`: the family's
//! reference first; every solver plays every attempt it is tried on) and the passive control
//! (`passive`: every unit fortifies or does nothing) does NOT win it; else the next attempt is drawn.
//! An instance no attempt is kept for is UNSOLVED and leaves the suite (counted). On a kept attempt
//! the random control and the players are played. The "reasonable level" of a family is read on the
//! kept attempts only, as a share of the best solver's win rate there.
//!
//! `ai-exam suite --out DIR [--families duel,hold] [--eras ancient,medieval] [--instances 100]
//!   [--players standard,unciv] [--jobs 4] [--attempts 6] [--kept DIR/kept.jsonl]`
//!   writes DIR/rows.jsonl (every play; info.stage = solver | passive | random | player),
//!   DIR/kept.jsonl (the attempt kept per instance, or unsolved) and DIR/unbuilt.jsonl. Resumes:
//!   what DIR holds is not played again. `--kept FILE`: no proof, the players play the attempts that
//!   FILE kept (a second binary, e.g. the `tac2` build, plays the same scenes).
//! `ai-exam suite-one --family hold --era medieval --instance 3 [--attempt 0] --player standard`:
//!   one play with the trace on stderr.
//! `ai-exam suite-show --family hold --era medieval --instance 3 [--attempt 0]`: what the scene is.
//! `ai-exam suite-list`: the families, the eras each claims, the win rules' rounds, the solvers.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use classic_rules::CompiledRuleset;
use crate::bench::modules::BenchRules;
use serde::{Deserialize, Serialize};

use super::families::{self, Fam};
use super::{lab, play, Row};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Kept {
    pub family: String,
    pub era: String,
    pub instance: usize,
    /// None: unsolved.
    pub attempt: Option<u32>,
    pub solver: Option<String>,
    pub tried: u32,
    /// Attempts dropped because the passive control won them.
    pub passive_won: u32,
    pub unbuilt: u32,
}

pub const ATTEMPTS: u32 = 12;

struct Opts {
    out: PathBuf,
    families: Vec<Fam>,
    eras: Option<Vec<String>>,
    instances: usize,
    from: usize,
    players: Vec<String>,
    jobs: usize,
    attempts: u32,
    kept: Option<PathBuf>,
    family: Option<Fam>,
    era: Option<String>,
    instance: usize,
    attempt: u32,
    player: String,
    /// Record the players' decisions for the army net into this file (`army_net::record`).
    record: Option<PathBuf>,
}

fn opts(args: &[String]) -> Result<Opts, String> {
    let mut o = Opts { out: PathBuf::from("trainers"), families: Fam::ALL.to_vec(), eras: None, instances: 100, from: 0, players: vec!["standard".to_owned()], jobs: 4, attempts: ATTEMPTS, kept: None, family: None, era: None, instance: 0, attempt: 0, player: "standard".to_owned(), record: None };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut v = || it.next().cloned().ok_or(format!("{a} needs a value"));
        match a.as_str() {
            "--out" => o.out = PathBuf::from(v()?),
            "--families" => o.families = v()?.split(',').map(Fam::parse).collect::<Result<_, _>>()?,
            "--eras" => o.eras = Some(v()?.split(',').map(str::to_owned).collect()),
            "--instances" => o.instances = v()?.parse().map_err(|e| format!("{e}"))?,
            "--from" => o.from = v()?.parse().map_err(|e| format!("{e}"))?,
            "--players" => o.players = v()?.split(',').filter(|s| !s.is_empty()).map(str::to_owned).collect(),
            "--jobs" => o.jobs = v()?.parse().map_err(|e| format!("{e}"))?,
            "--attempts" => o.attempts = v()?.parse().map_err(|e| format!("{e}"))?,
            "--kept" => o.kept = Some(PathBuf::from(v()?)),
            "--family" => o.family = Some(Fam::parse(&v()?)?),
            "--era" => o.era = Some(v()?),
            "--instance" => o.instance = v()?.parse().map_err(|e| format!("{e}"))?,
            "--attempt" => o.attempt = v()?.parse().map_err(|e| format!("{e}"))?,
            "--player" => o.player = v()?,
            "--record" => o.record = Some(PathBuf::from(v()?)),
            "--world" => lab::mode::WORLD.store(match v()?.as_str() { "small" => 0, "tiny" => 1, "duel" => 2, w => return Err(format!("--world small|tiny|duel, not {w}")) }, std::sync::atomic::Ordering::Relaxed),
            "--fog" => lab::mode::FOG.store(match v()?.as_str() { "on" => 1, "off" => 0, w => return Err(format!("--fog on|off, not {w}")) }, std::sync::atomic::Ordering::Relaxed),
            "--ai" => lab::mode::AI.store(match v()?.as_str() { "full" => 0, "combat" => 1, w => return Err(format!("--ai full|combat, not {w}")) }, std::sync::atomic::Ordering::Relaxed),
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(o)
}

fn tag(mut row: Row, stage: &str) -> Row {
    row.info.insert("stage".into(), stage.to_owned());
    row
}

/// The proof of one instance: attempts until one is kept. Returns the kept record and every row played.
pub fn prove(rules: &CompiledRuleset, fam: Fam, era: u8, instance: usize, attempts: u32) -> (Kept, Vec<Row>) {
    let names = lab::era_names(rules);
    let mut k = Kept { family: fam.name().to_owned(), era: names.get(era as usize).cloned().unwrap_or_default(), instance, attempt: None, solver: None, tried: 0, passive_won: 0, unbuilt: 0 };
    let mut rows = Vec::new();
    for a in 0..attempts {
        k.tried = a + 1;
        let Ok((game, scene)) = families::build(rules, fam, era, instance, a) else {
            k.unbuilt += 1;
            continue;
        };
        let mut solver = None;
        for s in fam.solvers() {
            if let Ok(r) = play(rules, game.clone(), scene.clone(), s, false) {
                if r.win && solver.is_none() {
                    solver = Some((*s).to_owned());
                }
                rows.push(tag(r, "solver"));
            }
        }
        let Some(solver) = solver else { continue };
        let Ok(pr) = play(rules, game.clone(), scene.clone(), "passive", false) else { continue };
        // An opportunity scene whose context lets either call stand is won by holding back by design:
        // the passive filter does not apply to it (the doc says so).
        let either = scene.info.get("expected").is_some_and(|e| e == "either");
        let passive_won = pr.win && !either;
        rows.push(tag(pr, "passive"));
        if passive_won {
            k.passive_won += 1;
            continue;
        }
        if let Ok(r) = play(rules, game.clone(), scene.clone(), "random", false) {
            rows.push(tag(r, "random"));
        }
        k.attempt = Some(a);
        k.solver = Some(solver);
        break;
    }
    (k, rows)
}

pub fn main(cmd: &str, args: &[String]) -> Result<(), String> {
    if cmd == "suite-dump" {
        let rules = crate::bench::modules::ClassicBench::rules();
        return super::dump::main(&rules, args);
    }
    let o = opts(args)?;
    let rules = crate::bench::modules::ClassicBench::rules();
    let rules = &rules;
    let names = lab::era_names(rules);
    let era_of = |n: &str| lab::era_index(rules, n).ok_or_else(|| format!("unknown era {n} ({})", names.join(", ")));
    match cmd {
        "suite-list" => {
            for f in Fam::ALL {
                let eras: Vec<String> = (0..names.len() as u8).filter(|e| lab::lab(rules, *e).is_ok_and(|l| f.claims(rules, &l.kinds))).map(|e| names[e as usize].clone()).collect();
                println!("{:10} rounds {:2}  eras {}  solvers {}  tricks {}  | {}", f.name(), f.rounds(), eras.join(","), f.solvers().join(","), f.tricks().iter().map(|(t, s)| format!("{t}:{s}")).collect::<Vec<_>>().join(","), f.task());
            }
            Ok(())
        }
        "suite-show" | "suite-one" => {
            let fam = o.family.ok_or("--family")?;
            let era = era_of(o.era.as_deref().ok_or("--era")?)?;
            let (game, scene) = families::build(rules, fam, era, o.instance, o.attempt)?;
            eprintln!("{}: {:?}", scene.label(rules), scene.info);
            eprintln!("{}", families::line(rules, &game, &scene, 0));
            if cmd == "suite-one" {
                let row = play(rules, game, scene, &o.player, true)?;
                for l in &row.trace {
                    eprintln!("{l}");
                }
                println!("{}", serde_json::to_string(&Row { trace: Vec::new(), ..row }).map_err(|e| e.to_string())?);
            }
            Ok(())
        }
        "suite-lab" => {
            let era = era_of(o.era.as_deref().unwrap_or("ancient"))?;
            let l = lab::lab(rules, era)?;
            let g = &l.game;
            let map = g.view().auth.map();
            let dims = map.dimensions;
            let n = map.tile_count() as u32;
            let water = (0..n).filter(|t| map.is_water(classic_rules::ids::TileId(*t))).count();
            let stand = (0..n).filter(|t| crate::battle::standable(rules, g, classic_rules::ids::TileId(*t))).count();
            println!("dims {}x{} tiles {n} water {water} standable {stand} centre {} homes {:?} grounds {:?}", dims.width, dims.height, l.centre.0, l.homes, l.grounds);
            println!("terrain at centre {:?} cost {:?}", map.terrain(l.centre), kernel::navigation::cost::enter_cost_on_map(map, l.centre));
            for (a, v) in &l.kinds.by_arm {
                println!("{:?}: {}", a, v.iter().map(|k| rules.units[k.0 as usize].name.clone()).collect::<Vec<_>>().join(", "));
            }
            for (a, v) in &l.kinds.uniques {
                println!("unique {:?}: {}", a, v.iter().map(|k| rules.units[k.0 as usize].name.clone()).collect::<Vec<_>>().join(", "));
            }
            Ok(())
        }
        "suite" => run(rules, &o),
        other => Err(format!("unknown command {other}")),
    }
}

fn run(rules: &CompiledRuleset, o: &Opts) -> Result<(), String> {
    let names = lab::era_names(rules);
    std::fs::create_dir_all(&o.out).map_err(|e| e.to_string())?;
    let rows_path = o.out.join("rows.jsonl");
    let kept_path = o.out.join("kept.jsonl");
    // What is done already.
    let mut done_rows: BTreeSet<(String, String, usize, u32, String)> = BTreeSet::new();
    if let Ok(text) = std::fs::read_to_string(&rows_path) {
        for l in text.lines() {
            if let Ok(r) = serde_json::from_str::<Row>(l) {
                done_rows.insert((r.family, r.era, r.instance, r.attempt, r.player));
            }
        }
    }
    let mut kept: BTreeMap<(String, String, usize), Kept> = BTreeMap::new();
    let source = o.kept.clone().unwrap_or_else(|| kept_path.clone());
    if let Ok(text) = std::fs::read_to_string(&source) {
        for l in text.lines() {
            if let Ok(k) = serde_json::from_str::<Kept>(l) {
                kept.insert((k.family.clone(), k.era.clone(), k.instance), k);
            }
        }
    }
    let prove_here = o.kept.is_none();
    let mut cells: Vec<(Fam, u8, usize)> = Vec::new();
    for &f in &o.families {
        for e in 0..names.len() as u8 {
            if o.eras.as_ref().is_some_and(|v| !v.contains(&names[e as usize])) {
                continue;
            }
            let Ok(l) = lab::lab(rules, e) else { continue };
            if !f.claims(rules, &l.kinds) {
                continue;
            }
            for i in o.from..o.instances {
                cells.push((f, e, i));
            }
        }
    }
    let rows_out = Mutex::new(std::fs::OpenOptions::new().create(true).append(true).open(&rows_path).map_err(|e| e.to_string())?);
    let kept_out = Mutex::new(std::fs::OpenOptions::new().create(true).append(true).open(&kept_path).map_err(|e| e.to_string())?);
    let record_out = match &o.record {
        Some(path) => Some(Mutex::new(std::fs::OpenOptions::new().create(true).append(true).open(path).map_err(|e| e.to_string())?)),
        None => None,
    };
    let started = std::time::Instant::now();
    let count = std::sync::atomic::AtomicUsize::new(0);
    let write_row = |r: &Row| {
        let line = serde_json::to_string(r).unwrap();
        let _ = writeln!(rows_out.lock().unwrap(), "{line}");
    };
    let indexes: Vec<usize> = (0..cells.len()).collect();
    crate::export::parallel(&indexes, o.jobs, |&ix| {
        let (f, e, i) = cells[ix];
        let key = (f.name().to_owned(), names[e as usize].clone(), i);
        let k = match kept.get(&key) {
            Some(k) => k.clone(),
            None if prove_here => {
                let (k, rows) = prove(rules, f, e, i, o.attempts);
                for r in &rows {
                    write_row(r);
                }
                let _ = writeln!(kept_out.lock().unwrap(), "{}", serde_json::to_string(&k).unwrap());
                k
            }
            None => return,
        };
        let Some(a) = k.attempt else { return };
        // `solvers` stands for the family's references.
        let players: Vec<String> = o.players.iter().flat_map(|p| if p == "solvers" { f.solvers().iter().map(|s| (*s).to_owned()).collect() } else { vec![p.clone()] }).collect();
        let todo: Vec<&String> = players.iter().filter(|p| !done_rows.contains(&(key.0.clone(), key.1.clone(), i, a, (*p).clone()))).collect();
        if todo.is_empty() {
            return;
        }
        let Ok((game, scene)) = families::build(rules, f, e, i, a) else { return };
        for p in todo {
            crate::army_net::record::want(record_out.is_some());
            match play(rules, game.clone(), scene.clone(), p, false) {
                Ok(r) => write_row(&tag(r, "player")),
                Err(err) => eprintln!("{} {p}: {err}", scene.label(rules)),
            }
            crate::army_net::record::want(false);
            if let (Some(out), Some((bytes, _))) = (record_out.as_ref(), crate::army_net::record::take_last()) {
                let _ = out.lock().unwrap().write_all(&bytes);
            }
        }
        let n = count.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        if n % 50 == 0 {
            println!("{n} instances, {:.0} s", started.elapsed().as_secs_f64());
        }
    });
    if std::env::var_os("TRAINERS_PROF").is_some() {
        let p: Vec<u64> = crate::trainers::PROF.iter().map(|a| a.load(std::sync::atomic::Ordering::Relaxed) / 1000).collect();
        eprintln!("prof ms: driver.step {} sample {} close_arena {}", p[0], p[1], p[2]);
    }
    println!("done: {} instances in {:.0} s", count.load(std::sync::atomic::Ordering::Relaxed), started.elapsed().as_secs_f64());
    Ok(())
}
