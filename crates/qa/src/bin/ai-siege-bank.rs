//! A bank of real siege positions and its replays (the `tactic` lane, `tactician-fit`;
//! classic/docs/ai-tactician-fit.md). A measurement tool: it changes no rule and no AI.
//!
//! `cargo run --release -p aurelia-qa --bin ai-siege-bank -- record|replay [options]`
//!
//! **record** plays solo games of a variant (every seat the same) round by round and, for every
//! war operation the AI memory holds, writes
//! - `DIR/ops-K.csv`: one row per operation (the probe's start and end, `war_probe::EndCause`),
//! - `DIR/turns-K.csv`: one row per operation and end of its player's turn while it marches: the
//!   members (distance to the target, role, health), the target city's damage and owner, the
//!   defenders near it, the war,
//! - `DIR/pos/*.save` and `DIR/index-K.csv`: positions, a save of the game with the AI memory at the
//!   start of a round, at two moments of an operation: `out` (the first round start after its army
//!   set out) and `arrive` (the first round start after that at which half of its living members
//!   stand within `ARRIVE` tiles of the target; often the same round: the muster point lies on the
//!   staging ring, and one save serves both).
//!
//! --variant X     every seat plays X (default war-pays)
//! --suite NAME    default pace; --speed NAME default quick
//! --offsets A..B  the suite's seed offsets, B excluded (default 200..201)
//! --jobs N        games in parallel (default 4)
//! --dir DIR       output directory
//! --limit N       the first N matches of each offset only
//!
//! **replay** plays every position of a bank for N rounds under each layer for the attacker
//! (the player of the operation) and `--others` for every other seat, R times with the dice of
//! every random stream restarted (`ClassicGame::reseed_streams`) from the position and the reseed
//! number alone: common random numbers across the layers. One row per position, layer and reseed:
//! taken (round), the members lost, the attacker's army lost, the defender's army lost, the damage
//! dealt to the city in the attacker's turns, the operation still running.
//!
//! --dir DIR            the bank
//! --kind out|arrive    the positions (default arrive)
//! --offsets A..B       the positions of these offsets
//! --layers A,B,…       the attacker's variants (default war-pays)
//! --others NAME        every other seat (default standard)
//! --reseeds R          (default 2; 0: the position's own dice, one play)
//! --rounds N           rounds played (default 15)
//! --set K=V,…          `unit_net::fit` overrides for every layer (dev only)
//! --jobs N             (default 4)
//! --out FILE           the rows (CSV)
//!
//! The tactics exam's C2 references (an exam addition; they change no AI): the layer `ref-press` plays
//! `war-pays` with its operation members pressed by a script every turn (a shooter fires at the city, a melee
//! unit attacks it when next to it, else a foe next to it, else the unit walks at the city); `ref-holdN` is
//! `war-pays` with the attacker out of play (its turns ended unplayed) for the first N rounds, then as war-pays.

use std::collections::BTreeMap;
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use aurelia_qa::bench::modules::{majors, BenchRules, ClassicBench};
use aurelia_qa::bench::suite::{Mode, Suite};
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::operations::army_role;
use classic_rules::ai::state::{ArmyRole, OperationPhase, UnitTask};
use classic_rules::ai::war_probe;
use classic_rules::ids::{EntityId, PlayerId, TileId};
use classic_rules::persistence::{decode::decode, encode::encode};
use classic_rules::save_codec::{from_bytes, to_bytes};
use classic_rules::{ClassicGame, CompiledRuleset, GameState};
use kernel::ai::driver::AiDriver;
use kernel::ai::seats::Seated;
use kernel::ai::StepOutcome;

type Driver = AiDriver<classic_rules::classic::Classic, Seated<ClassicMind>>;

/// An army arrived when half of its living members stand this close to the target (the
/// staging ring, `operations::STAGING_DISTANCE`; the muster point lies 3–5 tiles from it).
const ARRIVE: i32 = 3;
/// Defenders this close to the target defend it (`operations::DEFENCE_RADIUS`).
const NEAR: i32 = 3;

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("ai-siege-bank record|replay [options]");
        std::process::exit(2);
    }
    let mode = args.remove(0);
    let result = match mode.as_str() {
        "record" => record(&args),
        "replay" => replay(&args),
        other => Err(format!("unknown mode {other}")),
    };
    if let Err(e) = result {
        eprintln!("ai-siege-bank: {e}");
        std::process::exit(1);
    }
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn range(text: &str) -> Result<std::ops::Range<u64>, String> {
    let (a, b) = text.split_once("..").ok_or("A..B")?;
    Ok(a.parse().map_err(|e| format!("{e}"))?..b.parse().map_err(|e| format!("{e}"))?)
}

fn suite_of(args: &[String]) -> Result<Suite, String> {
    let suite = Suite::named(&flag(args, "--suite").unwrap_or_else(|| "pace".into())).ok_or("unknown suite")?;
    let speed = flag(args, "--speed").unwrap_or_else(|| "quick".into());
    let rules = ClassicBench::rules();
    let def = rules.speed_named(&speed).ok_or_else(|| format!("unknown speed {speed}"))?;
    Ok(suite.at_speed(&def.key, &def.values))
}

fn mind(name: &str) -> Result<ClassicMind, String> {
    ClassicBench::mind(name).ok_or_else(|| format!("no AI variant {name}"))
}

fn round(game: &GameState) -> u32 {
    game.view().auth.turn().round
}

/// `f` over `items` on `jobs` threads, results in order.
fn parallel<T: Sync, R: Send>(jobs: usize, items: &[T], f: impl Fn(&CompiledRuleset, &T) -> R + Sync) -> Vec<R> {
    let next = AtomicUsize::new(0);
    let out: Mutex<Vec<(usize, R)>> = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..jobs.clamp(1, items.len().max(1)) {
            scope.spawn(|| {
                let rules = ClassicBench::rules();
                loop {
                    let at = next.fetch_add(1, Ordering::Relaxed);
                    let Some(item) = items.get(at) else { break };
                    let r = f(&rules, item);
                    out.lock().expect("results").push((at, r));
                }
            });
        }
    });
    let mut out = out.into_inner().expect("results");
    out.sort_by_key(|(at, _)| *at);
    out.into_iter().map(|(_, r)| r).collect()
}

/// SplitMix64's finaliser.
fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

fn role_code(rules: &CompiledRuleset, kind: classic_rules::ids::UnitKindId) -> u8 {
    match army_role(rules, kind) {
        Some(ArmyRole::Melee) => 0,
        Some(ArmyRole::Ranged) => 1,
        Some(ArmyRole::Siege) => 2,
        None => 9,
    }
}

/// The ids of `player`'s units on the operation against `target`, by the AI memory.
fn members_of(driver: &Driver, player: PlayerId, target: TileId) -> Vec<EntityId> {
    let Some(state) = driver.states().get(player) else { return Vec::new() };
    let mut ids: Vec<EntityId> = state.tasks().iter().filter(|t| t.task == UnitTask::Operation { target }).map(|t| t.unit).collect();
    ids.sort();
    ids
}

/// The city on `tile`: owner, damage, max health, population.
fn city_at(game: &GameState, rules: &CompiledRuleset, tile: TileId) -> Option<(PlayerId, u16, u16, u32)> {
    let auth = &game.view().auth;
    let city = auth.indexes().city_on_tile(tile).and_then(|c| auth.cities().get(c))?;
    Some((city.owner, city.damage, classic_rules::combat::city::max_hit_points(auth, rules, city), city.population))
}

/// `owner`'s military units within `radius` of `tile`: count and power (strength × health / 100).
fn defenders(game: &GameState, rules: &CompiledRuleset, owner: PlayerId, tile: TileId, radius: i32) -> (u32, u32) {
    let auth = &game.view().auth;
    let dims = auth.map().dimensions;
    let mut n = 0;
    let mut power = 0;
    for u in auth.units().iter_cores().filter(|u| u.owner == owner && army_role(rules, u.kind).is_some()) {
        if dims.distance(u.tile, tile).is_some_and(|d| d <= radius) {
            n += 1;
            let def = &rules.units[u.kind.0 as usize];
            power += u32::from(def.strength.max(def.ranged_strength)) * u32::from(u.hp) / 100;
        }
    }
    (n, power)
}

// ---- record ----

/// An operation the memory holds, as the tool follows it.
#[derive(Debug, Clone, Default)]
struct Track {
    player: u8,
    enemy: u8,
    target: u32,
    start: u32,
    launch: Option<u32>,
    arrive: Option<u32>,
    members: Vec<EntityId>,
    open: bool,
}

struct GameRecord {
    ops: String,
    turns: String,
    index: String,
    /// The world's hash at every round start (a replay's fidelity check).
    hashes: String,
}

fn record(args: &[String]) -> Result<(), String> {
    let variant = flag(args, "--variant").unwrap_or_else(|| "war-pays".into());
    let suite = suite_of(args)?;
    let offsets = range(&flag(args, "--offsets").unwrap_or_else(|| "200..201".into()))?;
    let jobs: usize = flag(args, "--jobs").map_or(Ok(4), |j| j.parse().map_err(|e| format!("{e}")))?;
    let dir = flag(args, "--dir").ok_or("--dir")?;
    let limit: Option<usize> = flag(args, "--limit").map(|l| l.parse().map_err(|e| format!("{e}"))).transpose()?;
    std::fs::create_dir_all(format!("{dir}/pos")).map_err(|e| e.to_string())?;
    for offset in offsets {
        let mut matches = suite.matches(Mode::Solo, offset);
        if let Some(l) = limit {
            matches.truncate(l);
        }
        let started = std::time::Instant::now();
        let records = parallel(jobs, &matches, |rules, m| {
            let r = record_game(rules, &suite, m, offset, &variant, &dir);
            eprintln!("{} {:.0} s", m.label(), started.elapsed().as_secs_f64());
            r
        });
        let mut ops = String::from("offset,map,seed,player,enemy,target,start,cause,end,marching,age,captured_target,war_captures,launch,arrive,members_at_launch,value,need,free,population,form_turns,form_assembled,form_outmatched,form_no_declare,form_blocked_by_declare,form_blocked_by_outmatched,form_near,form_near_healthy,form_wounded,form_distance,form_need,muster_to_target\n");
        let mut turns = String::from("offset,map,seed,player,target,start,round,at_war,owner,damage,max_hp,population,def_n,def_power,def_n_wide,members,alive,near,near_melee,near_distance,adjacent_melee,hp_sum,dist_list\n");
        let mut index = String::from("file,kind,offset,map,seed,round,player,enemy,target,start,launch,members\n");
        let mut hashes = String::from("offset,map,seed,round,hash\n");
        for r in records {
            let r = r?;
            ops.push_str(&r.ops);
            turns.push_str(&r.turns);
            index.push_str(&r.index);
            hashes.push_str(&r.hashes);
        }
        std::fs::write(format!("{dir}/hashes-{offset}.csv"), hashes).map_err(|e| e.to_string())?;
        std::fs::write(format!("{dir}/ops-{offset}.csv"), ops).map_err(|e| e.to_string())?;
        std::fs::write(format!("{dir}/turns-{offset}.csv"), turns).map_err(|e| e.to_string())?;
        std::fs::write(format!("{dir}/index-{offset}.csv"), index).map_err(|e| e.to_string())?;
        println!("offset {offset}: {} games, {:.0} s", matches.len(), started.elapsed().as_secs_f64());
    }
    Ok(())
}

fn record_game(rules: &CompiledRuleset, suite: &Suite, m: &aurelia_qa::bench::suite::Match, offset: u64, variant: &str, dir: &str) -> Result<GameRecord, String> {
    let mut game = ClassicBench::new_game_at(rules, &m.map, m.seed, suite.rounds, &suite.speed);
    let mut driver: Driver = AiDriver::with_mind(ClassicBench::settings(rules), Vec::new(), Seated::new(mind(variant)?, Vec::new()));
    driver.set_trace_capacity(0);
    let tiles = game.view().auth.map().tile_count();
    let map = m.map.script;
    let players = majors(&game);
    let mut tracks: Vec<Track> = Vec::new();
    let mut turns = String::new();
    let mut index = String::new();
    let mut hashes = String::new();
    war_probe::enable(true);
    while !game.is_over() && round(&game) <= suite.rounds {
        let now = round(&game);
        hashes.push_str(&format!("{offset},{map},{},{now},{:016x}\n", m.seed, classic_rules::replay::hash::authoritative_hash(&game)));
        let dims = game.view().auth.map().dimensions;
        // The round's start: the operations the memory holds.
        for &p in &players {
            let Some(state) = driver.states().get(p) else { continue };
            let operations = state.operations().to_vec();
            for t in tracks.iter_mut().filter(|t| t.open && t.player == p.0) {
                if !operations.iter().any(|o| o.target.0 == t.target && o.since_round == t.start) {
                    t.open = false;
                }
            }
            for o in &operations {
                let at = match tracks.iter().position(|t| t.open && t.player == p.0 && t.target == o.target.0 && t.start == o.since_round) {
                    Some(at) => at,
                    None => {
                        tracks.push(Track { player: p.0, enemy: o.enemy.0, target: o.target.0, start: o.since_round, open: true, ..Track::default() });
                        tracks.len() - 1
                    }
                };
                let members = members_of(&driver, p, o.target);
                let t = &mut tracks[at];
                t.members = members.clone();
                if o.phase != OperationPhase::Marching {
                    continue;
                }
                let mut kinds: Vec<&'static str> = Vec::new();
                if t.launch.is_none() {
                    t.launch = Some(o.phase_round);
                    kinds.push("out");
                }
                let distances: Vec<i32> = members
                    .iter()
                    .filter_map(|id| game.view().auth.units().iter_cores().find(|u| u.id == *id))
                    .filter_map(|u| dims.distance(u.tile, o.target))
                    .collect();
                // Arrived: at least half of the living members stand within `ARRIVE` of the target.
                let close = distances.iter().filter(|d| **d <= ARRIVE).count();
                if t.arrive.is_none() && !distances.is_empty() && 2 * close >= distances.len() {
                    t.arrive = Some(now);
                    kinds.push("arrive");
                }
                let mut written: Option<String> = None;
                for kind in kinds {
                    if now + 25 > suite.rounds {
                        continue;
                    }
                    let file = match &written {
                        Some(file) => file.clone(),
                        None => {
                            let file = format!("{offset}-{map}-{}-{now}-{}-{}.save", m.seed, p.0, o.target.0);
                            let mut save = encode(&game, rules);
                            save.ai = driver.save();
                            std::fs::write(format!("{dir}/pos/{file}"), to_bytes(&save)).map_err(|e| e.to_string())?;
                            written = Some(file.clone());
                            file
                        }
                    };
                    let ids: Vec<String> = members.iter().map(|i| i.0.to_string()).collect();
                    index.push_str(&format!("{file},{kind},{offset},{map},{},{now},{},{},{},{},{},{}\n", m.seed, p.0, o.enemy.0, o.target.0, t.start, o.phase_round, ids.join(";")));
                }
            }
        }
        // The round, turn by turn: a row per marching operation at the end of its player's turn.
        loop {
            match driver.step(&mut game, rules) {
                StepOutcome::Worked => {}
                StepOutcome::PlayerDone(p) => {
                    for t in tracks.iter().filter(|t| t.player == p.0 && t.launch.is_some() && t.open) {
                        turns.push_str(&turn_row(&game, rules, &driver, offset, map, m.seed, t, now));
                    }
                }
                StepOutcome::RoundDone => break,
                StepOutcome::Waiting => break,
            }
        }
        if game.is_over() {
            break;
        }
    }
    let probe = war_probe::take();
    war_probe::enable(false);
    let mut ops = String::new();
    for start in &probe.starts {
        let end = probe.ends.iter().find(|e| e.player == start.player && e.target == start.target && e.round >= start.round);
        let next = probe.starts.iter().filter(|s| s.player == start.player && s.round > start.round).map(|s| s.round).min().unwrap_or(u32::MAX);
        let until = end.map_or(suite.rounds, |e| e.round + 20).min(next);
        let war_captures = probe.captures.iter().filter(|(r, p, _, from)| *p == start.player && *from == Some(start.enemy) && (start.round..until).contains(r)).count();
        let track = tracks.iter().find(|t| t.player == start.player.0 && t.target == start.target && t.start >= start.round && t.start <= start.round + 1);
        let (launch, arrive, launched_members) = track.map_or((String::new(), String::new(), 0), |t| (t.launch.map_or(String::new(), |r| r.to_string()), t.arrive.map_or(String::new(), |r| r.to_string()), t.members.len()));
        let last = end.map_or(u32::MAX, |e| e.round);
        let forming: Vec<&war_probe::FormingSample> = probe.forming.iter().filter(|f| f.player == start.player && f.target == start.target && (start.round..=last).contains(&f.round)).collect();
        let form = (
            forming.len(),
            forming.iter().filter(|f| f.assembled).count(),
            forming.iter().filter(|f| f.outmatched).count(),
            forming.iter().filter(|f| !f.may_declare).count(),
            forming.iter().filter(|f| f.assembled && !f.outmatched && !f.may_declare).count(),
            forming.iter().filter(|f| f.assembled && f.outmatched).count(),
        );
        // The forming turns' means (tenths): members near the muster, of them healthy, wounded, mean distance.
        let tenths = |x: u64| if forming.is_empty() { 0 } else { 10 * x / forming.len() as u64 };
        let fnear = tenths(forming.iter().map(|f| u64::from(f.near)).sum());
        let fhealthy = tenths(forming.iter().map(|f| u64::from(f.near_healthy)).sum());
        let fwounded = tenths(forming.iter().map(|f| u64::from(f.wounded)).sum());
        let fdist = if forming.is_empty() { 0 } else { 10 * forming.iter().map(|f| u64::from(f.distance_sum)).sum::<u64>() / forming.iter().map(|f| u64::from(f.members.max(1))).sum::<u64>() };
        let fneed = forming.last().map_or(0, |f| f.need);
        let fmt = forming.last().map_or(-1, |f| f.muster_to_target);
        ops.push_str(&format!(
            "{offset},{map},{},{},{},{},{},{},{},{},{},{},{war_captures},{launch},{arrive},{launched_members},{},{},{},{},{},{},{},{},{},{},{fnear},{fhealthy},{fwounded},{fdist},{fneed},{fmt}\n",
            m.seed,
            start.player.0,
            start.enemy.0,
            start.target,
            start.round,
            end.map_or("Running".to_owned(), |e| format!("{:?}", e.cause)),
            end.map_or(suite.rounds, |e| e.round),
            end.is_some_and(|e| e.marching),
            end.map_or(0, |e| e.age),
            end.is_some_and(|e| matches!(e.cause, war_probe::EndCause::Captured)),
            start.value,
            start.need,
            start.free,
            start.features.get(3).copied().unwrap_or(0),
            form.0,
            form.1,
            form.2,
            form.3,
            form.4,
            form.5,
        ));
    }
    let _ = tiles;
    Ok(GameRecord { ops, turns, index, hashes })
}

fn turn_row(game: &GameState, rules: &CompiledRuleset, driver: &Driver, offset: u64, map: &str, seed: u64, t: &Track, now: u32) -> String {
    let auth = &game.view().auth;
    let dims = auth.map().dimensions;
    let target = TileId(t.target);
    let player = PlayerId(t.player);
    let enemy = PlayerId(t.enemy);
    let (owner, damage, max_hp, population) = city_at(game, rules, target).map_or((255, 0, 0, 0), |(o, d, m, p)| (o.0, d, m, p));
    let at_war = auth.diplomacy().at_war(player, enemy);
    let (def_n, def_power) = defenders(game, rules, enemy, target, NEAR);
    let (def_wide, _) = defenders(game, rules, enemy, target, 6);
    let members = {
        let now_members = members_of(driver, player, target);
        if now_members.is_empty() { t.members.clone() } else { now_members }
    };
    let alive: Vec<&kernel::world::unit::UnitCore> = members.iter().filter_map(|id| auth.units().iter_cores().find(|u| u.id == *id)).collect();
    let dist = |u: &kernel::world::unit::UnitCore| dims.distance(u.tile, target).unwrap_or(99);
    let near: Vec<&&kernel::world::unit::UnitCore> = alive.iter().filter(|u| dist(u) <= ARRIVE).collect();
    let near_melee = near.iter().filter(|u| role_code(rules, u.kind) == 0).count();
    let near_distance = near.iter().filter(|u| matches!(role_code(rules, u.kind), 1 | 2)).count();
    let adjacent_melee = alive.iter().filter(|u| dist(u) <= 1 && role_code(rules, u.kind) == 0 && u.hp > 0).count();
    let hp_sum: u32 = alive.iter().map(|u| u32::from(u.hp)).sum();
    let list: Vec<String> = alive.iter().map(|u| format!("{}{}:{}", ["m", "r", "s", "x", "x", "x", "x", "x", "x", "o"][role_code(rules, u.kind) as usize], dist(u), u.hp)).collect();
    format!(
        "{offset},{map},{seed},{},{},{},{now},{},{owner},{damage},{max_hp},{population},{def_n},{def_power},{def_wide},{},{},{},{near_melee},{near_distance},{adjacent_melee},{hp_sum},{}\n",
        t.player,
        t.target,
        t.start,
        u8::from(at_war),
        members.len(),
        alive.len(),
        near.len(),
        list.join(" ")
    )
}

// ---- replay ----

#[derive(Debug, Clone)]
struct Position {
    file: String,
    offset: u64,
    seed: u64,
    round: u32,
    player: PlayerId,
    enemy: PlayerId,
    target: TileId,
    members: Vec<EntityId>,
}

fn read_index(dir: &str, kind: &str, offsets: &std::ops::Range<u64>) -> Result<Vec<Position>, String> {
    let mut out = Vec::new();
    for offset in offsets.clone() {
        let path = format!("{dir}/index-{offset}.csv");
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        for line in text.lines().skip(1) {
            let c: Vec<&str> = line.split(',').collect();
            if c.len() < 12 || c[1] != kind {
                continue;
            }
            let num = |i: usize| c[i].parse::<u64>().unwrap_or(0);
            out.push(Position {
                file: c[0].to_owned(),
                offset,
                seed: num(4),
                round: num(5) as u32,
                player: PlayerId(num(6) as u8),
                enemy: PlayerId(num(7) as u8),
                target: TileId(num(8) as u32),
                members: c[11].split(';').filter(|s| !s.is_empty()).filter_map(|s| s.parse().ok()).map(EntityId).collect(),
            });
        }
    }
    Ok(out)
}

/// What one replay of a position gave.
#[derive(Debug, Clone, Default)]
struct Outcome {
    taken: Option<u32>,
    members_lost: u32,
    army_lost: u32,
    defender_lost: u32,
    damage: u32,
    running: bool,
    at_war: bool,
    rounds: u32,
    hash: u64,
}

fn military_ids(game: &GameState, rules: &CompiledRuleset, owner: PlayerId) -> Vec<EntityId> {
    game.view().auth.units().iter_cores().filter(|u| u.owner == owner && army_role(rules, u.kind).is_some()).map(|u| u.id).collect()
}

/// `ref-press` for the operation's members at the attacker's turn start (see the header).
fn press_members(game: &mut GameState, rules: &CompiledRuleset, pos: &Position) -> u64 {
    use classic_rules::Command;
    let mut made = 0;
    let dist = |g: &GameState, a: TileId, b: TileId| g.view().auth.map().dimensions.distance(a, b).unwrap_or(i32::MAX);
    for _ in 0..3 {
        let members: Vec<(classic_rules::ids::UnitHandle, classic_rules::ids::UnitKindId, TileId)> = {
            let auth = game.view().auth;
            auth.indexes().units_of(pos.player).iter().filter_map(|h| auth.units().get(*h).map(|u| (*h, u.core.kind, u.core.tile, u.core.id))).filter(|(_, _, _, id)| pos.members.contains(id)).map(|(h, k, t, _)| (h, k, t)).collect()
        };
        for (h, kind, tile) in members {
            let ranged = classic_rules::ai::combat::ranged_range(rules, kind).is_some();
            let city_h = game.view().auth.indexes().city_on_tile(pos.target);
            let mut cmds: Vec<Command> = Vec::new();
            if ranged {
                cmds.push(Command::RangedAttack { unit: h, target: pos.target });
            } else if let Some(city) = city_h {
                cmds.push(Command::AttackCity { attacker: h, city });
            }
            let auth = game.view().auth;
            let mut foes: Vec<(classic_rules::ids::UnitHandle, TileId)> = auth.indexes().units_of(pos.enemy).iter().filter_map(|u| auth.units().get(*u).map(|x| (*u, x.core.tile))).filter(|(_, t)| dist(game, tile, *t) <= if ranged { 3 } else { 1 }).collect();
            foes.sort_by_key(|(_, t)| (dist(game, tile, *t), t.0));
            for (fh, ft) in foes {
                cmds.push(if ranged { Command::RangedAttack { unit: h, target: ft } } else { Command::AttackUnit { attacker: h, defender: fh } });
            }
            let mut acted = false;
            for cmd in cmds {
                if game.accepts_order(rules, pos.player, &cmd) && game.execute(rules, cmd).is_ok() {
                    made += 1;
                    acted = true;
                    break;
                }
            }
            if acted || dist(game, tile, pos.target) <= 1 {
                continue;
            }
            let steps = rules.units.get(kind.0 as usize).map_or(1, |d| i32::from(d.movement.max(1)));
            let auth = game.view().auth;
            let mut tiles: Vec<TileId> = (0..auth.map().tile_count() as u32)
                .map(TileId)
                .filter(|t| *t != tile && dist(game, tile, *t) <= steps && auth.indexes().units_on(*t).is_empty() && auth.indexes().city_on_tile(*t).is_none() && dist(game, *t, pos.target) < dist(game, tile, pos.target))
                .collect();
            tiles.sort_by_key(|&t| (dist(game, t, pos.target), t.0));
            tiles.truncate(10);
            for t in tiles {
                let cmd = Command::MoveUnit { unit: h, destination: t };
                if game.accepts_order(rules, pos.player, &cmd) && game.execute(rules, cmd).is_ok() {
                    made += 1;
                    break;
                }
            }
        }
    }
    made
}

/// A layer named `tactician-fit@NAME` plays the candidate NAME of `--candidates`.
fn play_layer(rules: &CompiledRuleset, dir: &str, pos: &Position, layer: &str, others: &str, dice: Option<u64>, rounds: u32, candidates: &[(String, &'static classic_rules::ai::unit_net::fit::Numbers)]) -> Result<Outcome, String> {
    match layer.split_once('@') {
        Some((variant, name)) => {
            let n = candidates.iter().find(|(c, _)| c == name).map(|(_, n)| *n).ok_or_else(|| format!("no candidate {name}"))?;
            let _scope = classic_rules::ai::unit_net::fit::enter(n);
            play_position(rules, dir, pos, variant, others, dice, rounds)
        }
        None => play_position(rules, dir, pos, layer, others, dice, rounds),
    }
}

fn play_position(rules: &CompiledRuleset, dir: &str, pos: &Position, layer: &str, others: &str, dice: Option<u64>, rounds: u32) -> Result<Outcome, String> {
    let bytes = std::fs::read(format!("{dir}/pos/{}", pos.file)).map_err(|e| format!("{}: {e}", pos.file))?;
    let save = from_bytes(&bytes).map_err(|e| format!("{}: {e:?}", pos.file))?;
    let mut game = decode(&save, rules).map_err(|e| format!("{}: {e:?}", pos.file))?;
    if let Some(dice) = dice {
        game.reseed_streams(dice);
    }
    // The exam's references play `war-pays` (the bank's own attacker) with the script.
    let press = layer == "ref-press";
    let hold: u32 = layer.strip_prefix("ref-hold").and_then(|n| n.parse().ok()).unwrap_or(0);
    let layer = if press || hold > 0 { "war-pays" } else { layer };
    let mut driver: Driver = AiDriver::with_mind(ClassicBench::settings(rules), if hold > 0 { vec![pos.player] } else { Vec::new() }, Seated::new(mind(others)?, vec![(pos.player, mind(layer)?)]));
    driver.set_trace_capacity(0);
    driver.load(&save.ai, game.view().auth.map().tile_count(), rules).map_err(|e| format!("{}: memory {e:?}", pos.file))?;
    let army = military_ids(&game, rules, pos.player);
    let defence = military_ids(&game, rules, pos.enemy);
    let mut out = Outcome::default();
    let mut damage_before = city_at(&game, rules, pos.target).map_or(0, |c| c.1);
    let stop = pos.round + rounds;
    let mut pressed: Option<u32> = None;
    let mut released = hold == 0;
    'rounds: while !game.is_over() && round(&game) < stop {
        loop {
            if !released && round(&game) >= pos.round + hold {
                driver.set_humans(Vec::new());
                released = true;
            }
            if press && game.view().auth.active_player() == pos.player && pressed != Some(round(&game)) {
                pressed = Some(round(&game));
                press_members(&mut game, rules, pos);
            }
            match driver.step(&mut game, rules) {
                StepOutcome::Worked => {}
                StepOutcome::Waiting if !released && !game.is_over() => {
                    // The attacker is out of play: its turn ends unplayed.
                    let end = classic_rules::Command::EndTurn { player: game.view().auth.active_player() };
                    if game.execute(rules, end).is_err() {
                        break 'rounds;
                    }
                }
                StepOutcome::PlayerDone(p) => {
                    let city = city_at(&game, rules, pos.target);
                    let damage = city.map_or(0, |c| c.1);
                    if p == pos.player {
                        out.damage += u32::from(damage.saturating_sub(damage_before));
                        if city.is_some_and(|c| c.0 == pos.player) {
                            out.taken = Some(round(&game) - pos.round);
                            out.damage += u32::from(city.map_or(0, |c| c.2).saturating_sub(damage_before));
                            break 'rounds;
                        }
                    }
                    damage_before = damage;
                }
                StepOutcome::RoundDone | StepOutcome::Waiting => break,
            }
        }
    }
    let alive = |id: &EntityId| game.view().auth.units().iter_cores().any(|u| u.id == *id);
    out.members_lost = pos.members.iter().filter(|id| !alive(id)).count() as u32;
    out.army_lost = army.iter().filter(|id| !alive(id)).count() as u32;
    out.defender_lost = defence.iter().filter(|id| !alive(id)).count() as u32;
    out.running = driver.states().get(pos.player).is_some_and(|s| s.operations().iter().any(|o| o.target == pos.target));
    out.at_war = game.view().auth.diplomacy().at_war(pos.player, pos.enemy);
    out.rounds = round(&game) - pos.round;
    out.hash = classic_rules::replay::hash::authoritative_hash(&game);
    Ok(out)
}

fn replay(args: &[String]) -> Result<(), String> {
    let dir = flag(args, "--dir").ok_or("--dir")?;
    let kind = flag(args, "--kind").unwrap_or_else(|| "arrive".into());
    let offsets = range(&flag(args, "--offsets").unwrap_or_else(|| "200..207".into()))?;
    let layers: Vec<String> = flag(args, "--layers").unwrap_or_else(|| "war-pays".into()).split(',').map(str::to_owned).collect();
    let others = flag(args, "--others").unwrap_or_else(|| "standard".into());
    let reseeds: u32 = flag(args, "--reseeds").map_or(Ok(2), |r| r.parse().map_err(|e| format!("{e}")))?;
    let rounds: u32 = flag(args, "--rounds").map_or(Ok(15), |r| r.parse().map_err(|e| format!("{e}")))?;
    let jobs: usize = flag(args, "--jobs").map_or(Ok(4), |j| j.parse().map_err(|e| format!("{e}")))?;
    let out_path = flag(args, "--out").ok_or("--out")?;
    let salt: u64 = flag(args, "--salt").map_or(Ok(0), |r| r.parse().map_err(|e| format!("{e}")))?;
    if let Some(set) = flag(args, "--set") {
        classic_rules::ai::unit_net::fit::set_process_overrides(&set)?;
    }
    // `--candidates FILE`: a candidate a line, `NAME<TAB>pairs` (`fit::parse`); each adds the layer
    // `tactician-fit@NAME`.
    let mut candidates: Vec<(String, &'static classic_rules::ai::unit_net::fit::Numbers)> = Vec::new();
    let mut layers = layers;
    if let Some(path) = flag(args, "--candidates") {
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))?;
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            let (name, pairs) = line.split_once('\t').unwrap_or((line, ""));
            let n = classic_rules::ai::unit_net::fit::parse(pairs)?;
            candidates.push((name.to_owned(), Box::leak(Box::new(n))));
            layers.push(format!("tactician-fit@{name}"));
        }
    }
    // `--sample K/N`: only the positions whose number modulo N is K (successive halving's subsets).
    let positions = read_index(&dir, &kind, &offsets)?;
    let positions: Vec<Position> = match flag(args, "--sample") {
        Some(sample) => {
            let (k, n) = sample.split_once('/').ok_or("--sample K/N")?;
            let (k, n): (u64, u64) = (k.parse().map_err(|e| format!("{e}"))?, n.parse().map_err(|e| format!("{e}"))?);
            positions.into_iter().filter(|p| mix(p.seed ^ u64::from(p.round) << 20 ^ u64::from(p.target.0) << 40) % n == k).collect()
        }
        None => positions,
    };
    let mut jobs_list: Vec<(usize, String, u32)> = Vec::new();
    for (i, _) in positions.iter().enumerate() {
        for layer in &layers {
            for r in 0..reseeds.max(1) {
                jobs_list.push((i, layer.clone(), r));
            }
        }
    }
    let started = std::time::Instant::now();
    let done = AtomicUsize::new(0);
    let results = parallel(jobs, &jobs_list, |rules, (i, layer, r)| {
        let pos = &positions[*i];
        let dice = (reseeds > 0).then(|| mix(pos.seed ^ mix(u64::from(pos.round) << 32 ^ u64::from(pos.player.0) << 24 ^ u64::from(pos.target.0) ^ mix(u64::from(*r) ^ salt << 16 ^ 0x5ee9e))));
        let o = play_layer(rules, &dir, pos, layer, &others, dice, rounds, &candidates);
        let n = done.fetch_add(1, Ordering::Relaxed) + 1;
        if n % 50 == 0 {
            eprintln!("{n}/{} {:.0} s", jobs_list.len(), started.elapsed().as_secs_f64());
        }
        o
    });
    let mut text = String::from("file,offset,layer,reseed,taken,taken_round,members,members_lost,army_lost,defender_lost,damage,running,at_war,rounds,hash\n");
    let mut sums: BTreeMap<String, (u32, u32, u32, u32, u32, u64)> = BTreeMap::new();
    for ((i, layer, r), o) in jobs_list.iter().zip(results) {
        let pos = &positions[*i];
        let o = o?;
        text.push_str(&format!(
            "{},{},{layer},{r},{},{},{},{},{},{},{},{},{},{},{:016x}\n",
            pos.file,
            pos.offset,
            u8::from(o.taken.is_some()),
            o.taken.map_or(-1, i64::from),
            pos.members.len(),
            o.members_lost,
            o.army_lost,
            o.defender_lost,
            o.damage,
            u8::from(o.running),
            u8::from(o.at_war),
            o.rounds,
            o.hash
        ));
        let s = sums.entry(layer.clone()).or_default();
        s.0 += 1;
        s.1 += u32::from(o.taken.is_some());
        s.2 += o.members_lost;
        s.3 += o.army_lost;
        s.4 += o.defender_lost;
        s.5 += u64::from(o.damage);
    }
    std::fs::File::create(&out_path).and_then(|mut f| f.write_all(text.as_bytes())).map_err(|e| format!("{out_path}: {e}"))?;
    println!("{} positions ({kind}, offsets {offsets:?}), {} plays, {:.0} s", positions.len(), jobs_list.len(), started.elapsed().as_secs_f64());
    for (layer, (n, taken, ml, al, dl, dmg)) in sums {
        let n = n.max(1) as f64;
        println!(
            "{layer}: taken {:.1} %, members lost {:.2}, army lost {:.2}, defender lost {:.2}, damage {:.0} per play",
            100.0 * f64::from(taken) / n,
            f64::from(ml) / n,
            f64::from(al) / n,
            f64::from(dl) / n,
            dmg as f64 / n
        );
    }
    println!("wrote {out_path}");
    Ok(())
}
