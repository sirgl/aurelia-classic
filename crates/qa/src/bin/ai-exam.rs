//! The tactics exam (`aurelia_qa::grounds::exam`; /Users/roman.ivanov/civ/work/nets/exam/REPORT.md).
//!
//! `ai-exam run --out DIR [--scenes t1a,t2,...] [--players standard,unit-net,hybrid,war-pays] [--instances 80]
//!   [--offset 0] [--jobs 4] [--control]`
//!   plays every (scene, instance, player) not yet in DIR/rows.jsonl; the references of a scene
//!   (`ref-pillage`, `ref-press`) are played too unless --no-refs. Unbuildable instances go to DIR/unbuilt.jsonl.
//! `ai-exam one --scene t2 --instance 17 --player unit-net [--offset 0] [--control]`
//!   replays one instance with the trace on stderr, one line a round.
//!
//! A measurement: it changes no rule and no AI.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use aurelia_qa::bench::modules::{BenchRules, ClassicBench};
use aurelia_qa::grounds::exam::{build, build_world, jobs, play, Row, Sc};

fn main() {
    if let Err(e) = real() {
        eprintln!("ai-exam: {e}");
        std::process::exit(1);
    }
}

struct Opts {
    cmd: String,
    out: PathBuf,
    scenes: Vec<Sc>,
    players: Vec<String>,
    instances: usize,
    offset: u64,
    jobs: usize,
    control: bool,
    refs: bool,
    scene: Option<Sc>,
    instance: usize,
    player: String,
}

fn opts() -> Result<Opts, String> {
    let mut args = std::env::args().skip(1);
    let cmd = args.next().ok_or("run | one")?;
    let mut o = Opts { cmd, out: PathBuf::from("rows"), scenes: Sc::ALL.to_vec(), players: ["standard", "unit-net", "hybrid", "war-pays"].iter().map(|s| (*s).to_owned()).collect(), instances: 80, offset: 0, jobs: 4, control: false, refs: true, scene: None, instance: 0, player: "standard".to_owned() };
    while let Some(a) = args.next() {
        let mut value = || args.next().ok_or(format!("{a} needs a value"));
        match a.as_str() {
            "--out" => o.out = PathBuf::from(value()?),
            "--scenes" => o.scenes = value()?.split(',').map(Sc::parse).collect::<Result<_, _>>()?,
            "--players" => o.players = value()?.split(',').map(str::to_owned).collect(),
            "--instances" => o.instances = value()?.parse().map_err(|e| format!("{e}"))?,
            "--offset" => o.offset = value()?.parse().map_err(|e| format!("{e}"))?,
            "--jobs" => o.jobs = value()?.parse().map_err(|e| format!("{e}"))?,
            "--control" => o.control = true,
            "--no-refs" => o.refs = false,
            "--scene" => o.scene = Some(Sc::parse(&value()?)?),
            "--instance" => o.instance = value()?.parse().map_err(|e| format!("{e}"))?,
            "--player" => o.player = value()?,
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(o)
}

fn real() -> Result<(), String> {
    let o = opts()?;
    let rules = ClassicBench::rules();
    match o.cmd.as_str() {
        "one" => {
            let sc = o.scene.ok_or("--scene")?;
            let all = jobs(&rules, o.instance + 1, o.offset)?;
            let world = build_world(&rules, &all[o.instance])?;
            let (game, run) = build(sc, &rules, &world, o.control)?;
            eprintln!("scene {} instance {} {}: {:?}", sc.name(), o.instance, world.label, run.info);
            let row = play(&rules, game, run, o.instance, &o.player, true)?;
            println!("{}", serde_json::to_string(&row).map_err(|e| e.to_string())?);
            Ok(())
        }
        "run" => {
            std::fs::create_dir_all(&o.out).map_err(|e| e.to_string())?;
            let rows_path = o.out.join("rows.jsonl");
            let mut done = std::collections::BTreeSet::new();
            if let Ok(text) = std::fs::read_to_string(&rows_path) {
                for line in text.lines() {
                    if let Ok(r) = serde_json::from_str::<Row>(line) {
                        done.insert((r.scene, r.instance, r.player));
                    }
                }
            }
            let suffix = if o.control { "-ctl" } else { "" };
            let rows = Mutex::new(std::fs::OpenOptions::new().create(true).append(true).open(&rows_path).map_err(|e| e.to_string())?);
            let unbuilt = Mutex::new(std::fs::OpenOptions::new().create(true).append(true).open(o.out.join("unbuilt.jsonl")).map_err(|e| e.to_string())?);
            let all = jobs(&rules, o.instances, o.offset)?;
            let indexes: Vec<usize> = (0..all.len()).collect();
            let started = std::time::Instant::now();
            let count = std::sync::atomic::AtomicUsize::new(0);
            aurelia_qa::export::parallel(&indexes, o.jobs, |&i| {
                let players_of = |sc: Sc| -> Vec<String> {
                    let mut v = o.players.clone();
                    if o.refs {
                        v.extend(sc.references().iter().map(|s| (*s).to_owned()));
                    }
                    v.into_iter().filter(|p| !done.contains(&(format!("{}{suffix}", sc.name()), i, p.clone()))).collect()
                };
                let scenes: Vec<Sc> = o.scenes.iter().copied().filter(|sc| (!o.control || sc.has_raiders()) && !players_of(*sc).is_empty()).collect();
                if scenes.is_empty() {
                    return;
                }
                let log_unbuilt = |sc: Sc, e: &str| {
                    let line = serde_json::json!({"scene": format!("{}{suffix}", sc.name()), "instance": i, "error": e});
                    let _ = writeln!(unbuilt.lock().unwrap(), "{line}");
                };
                let world = match build_world(&rules, &all[i]) {
                    Ok(w) => w,
                    Err(e) => {
                        for sc in scenes {
                            log_unbuilt(sc, &format!("world: {e}"));
                        }
                        return;
                    }
                };
                for sc in scenes {
                    let (game, run) = match build(sc, &rules, &world, o.control) {
                        Ok(x) => x,
                        Err(e) => {
                            log_unbuilt(sc, &e);
                            continue;
                        }
                    };
                    for p in players_of(sc) {
                        match play(&rules, game.clone(), run.clone(), i, &p, false) {
                            Ok(mut row) => {
                                row.scene = format!("{}{suffix}", row.scene);
                                let line = serde_json::to_string(&row).unwrap();
                                let _ = writeln!(rows.lock().unwrap(), "{line}");
                                let n = count.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                                if n % 25 == 0 {
                                    println!("{n} rows, {:.0} s", started.elapsed().as_secs_f64());
                                }
                            }
                            Err(e) => log_unbuilt(sc, &format!("{p}: {e}")),
                        }
                    }
                }
            });
            println!("done: {} rows in {:.0} s", count.load(std::sync::atomic::Ordering::Relaxed), started.elapsed().as_secs_f64());
            Ok(())
        }
        other => Err(format!("unknown command {other}")),
    }
}
