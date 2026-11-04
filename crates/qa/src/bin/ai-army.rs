//! What armies the AI builds and what they meet, and the barbarian camps
//! (classic/docs/ai-army.md). A measurement tool only: it changes no rule and no AI.
//!
//! `cargo run --release -p aurelia-qa --bin ai-army -- [options]`
//!
//! --candidate X / --baseline Y   the sides (default `standard` both); --solo X: every seat X
//! --suite NAME                   a bench suite (default duel)
//! --speed NAME                   a game speed (default quick)
//! --offset K                     the suite's seeds + 1000·K (default 0)
//! --jobs N                       games in parallel (default 4)
//! --limit N                      only the suite's first N matches
//! --out PREFIX                   the CSV files below (default `army`)
//!
//! PREFIX-units.csv     every land military unit a major got: the round, the owner's era, the
//!                      kind, its rules' role, the reason of the city order that queued it
//!                      (`ProduceArmy`: the war's and the escorts' orders, `ProduceDefender`: a
//!                      garrison or a city in danger; `start`, `other`: none), its death and kills
//! PREFIX-kills.csv     every land military unit destroyed in a fight: the winner's and the
//!                      loser's kind, owner kind and era, who attacked
//! PREFIX-wars.csv      each major's first war with another major: the round, the era, the
//!                      military technologies known, the strategic resources (total/free)
//! PREFIX-faced.csv     per major, the land military units by rules' role of the majors it is
//!                      at war with, summed over the rounds of war, and its own
//! PREFIX-camps.csv     every barbarian camp: where, when it appeared and went, who cleared it,
//!                      the nearest major's city distance at its appearance
//! PREFIX-civilians.csv every civilian unit a major lost: the round, the kind, to whom
//! PREFIX-seats.csv     per major at the end: score, camps cleared and their gold, civilians lost
//!                      to barbarians, military units got and lost

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use aurelia_qa::bench::modules::{majors, standing, BenchRules, ClassicBench};
use aurelia_qa::bench::suite::{Match, Mode, Side, Suite};
use classic_rules::world::player::ClassicPlayers;
use classic_rules::Command;
use kernel::ai::driver::AiDriver;
use kernel::ai::seats::Seated;
use kernel::ai::trace::Outcome;
use kernel::ai::{DriverPhase, StepOutcome};
use kernel::ids::{PlayerId, ResourceId, TechId, TileId, UnitHandle, UnitKindId};
use kernel::rules::content::Domain;
use kernel::transaction::journal::{PlayerChange, UnitChange};

type R = ClassicBench;
type M = <ClassicBench as BenchRules>::M;
type Game = kernel::world::state::GameState<M>;
type Rules = kernel::rules::CompiledRuleset<M>;

/// The trace kept between two reads (a player's turn).
const TRACE: usize = 50_000;
/// Camps within this many tiles of a major's city are its own business (Unciv's 6).
const CAMP_NEAR: i32 = 6;
/// A new unit seen this far from the city that queued its kind came from it.
const FIRST_WALK: i32 = 4;

#[derive(Clone)]
struct Options {
    candidate: String,
    baseline: String,
    solo: bool,
    suite: String,
    speed: String,
    offset: u64,
    jobs: usize,
    out: String,
    limit: usize,
}

fn options() -> Result<Options, String> {
    let mut o = Options {
        candidate: "standard".into(),
        baseline: "standard".into(),
        solo: false,
        suite: "duel".into(),
        speed: "quick".into(),
        offset: 0,
        jobs: 4,
        out: "army".into(),
        limit: usize::MAX,
    };
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--candidate" => o.candidate = value()?,
            "--baseline" => o.baseline = value()?,
            "--solo" => {
                let v = value()?;
                o.candidate = v.clone();
                o.baseline = v;
                o.solo = true;
            }
            "--suite" => o.suite = value()?,
            "--speed" => o.speed = value()?,
            "--offset" => o.offset = value()?.parse().map_err(|e| format!("{e}"))?,
            "--jobs" => o.jobs = value()?.parse().map_err(|e| format!("{e}"))?,
            "--out" => o.out = value()?,
            "--limit" => o.limit = value()?.parse().map_err(|e| format!("{e}"))?,
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(o)
}

fn main() {
    if let Err(e) = options().and_then(|o| run(&o)) {
        eprintln!("ai-army: {e}");
        std::process::exit(1);
    }
}

#[derive(Default)]
struct Out {
    units: String,
    kills: String,
    wars: String,
    faced: String,
    camps: String,
    civilians: String,
    seats: String,
}

const FILES: [(&str, &str); 7] = [
    ("units", "game,seed,player,side,variant,round,era,kind,role,army_role,cost,purpose,died,killed_by,kills\n"),
    ("kills", "game,seed,round,winner,winner_side,winner_owner,winner_era,winner_kind,winner_role,loser,loser_side,loser_owner,loser_era,loser_kind,loser_role,attacker_won\n"),
    ("wars", "game,seed,player,side,round,era,techs,military_techs,strategic\n"),
    ("faced", "game,seed,player,side,war_rounds,role,enemy_units,own_units\n"),
    ("camps", "game,seed,tile,appeared,gone,cleared_by,cleared_side,nearest_major,nearest_distance\n"),
    ("civilians", "game,seed,player,side,round,kind,by\n"),
    ("seats", "game,seed,player,side,variant,score,cities,techs,camps_cleared,camp_gold,camps_near_rounds,civilians_to_barbarians,civilians_to_others,military_got,military_lost,military_lost_to_barbarians,barbarians_killed\n"),
];

fn run(o: &Options) -> Result<(), String> {
    let rules = R::rules();
    let suite = Suite::named(&o.suite).ok_or("unknown suite")?;
    let def = rules.speed_named(&o.speed).ok_or_else(|| format!("unknown speed {}", o.speed))?;
    let suite = suite.at_speed(&def.key, &def.values);
    let mode = if o.solo { Mode::Solo } else { Mode::Versus };
    let matches: Vec<Match> = suite.matches(mode, o.offset).into_iter().take(o.limit).collect();
    let next = AtomicUsize::new(0);
    let outs: Mutex<Vec<(usize, Out)>> = Mutex::new(Vec::new());
    let started = std::time::Instant::now();
    std::thread::scope(|scope| {
        for _ in 0..o.jobs.clamp(1, matches.len().max(1)) {
            scope.spawn(|| {
                let rules = R::rules();
                loop {
                    let at = next.fetch_add(1, Ordering::Relaxed);
                    let Some(m) = matches.get(at) else { break };
                    match play(o, &rules, &suite, m) {
                        Ok(out) => {
                            eprintln!("{} done, {:.0} s", m.label(), started.elapsed().as_secs_f64());
                            outs.lock().expect("outs").push((at, out));
                        }
                        Err(e) => eprintln!("{}: {e}", m.label()),
                    }
                }
            });
        }
    });
    let mut outs = outs.into_inner().map_err(|e| e.to_string())?;
    outs.sort_by_key(|(at, _)| *at);
    for (name, header) in FILES {
        let mut text = String::from(header);
        for (_, out) in &outs {
            text.push_str(match name {
                "units" => &out.units,
                "kills" => &out.kills,
                "wars" => &out.wars,
                "faced" => &out.faced,
                "camps" => &out.camps,
                "civilians" => &out.civilians,
                _ => &out.seats,
            });
        }
        let path = format!("{}-{name}.csv", o.out);
        std::fs::File::create(&path).and_then(|mut f| f.write_all(text.as_bytes())).map_err(|e| format!("{path}: {e}"))?;
        println!("wrote {path}");
    }
    println!("{} games, {:.0} s", matches.len(), started.elapsed().as_secs_f64());
    Ok(())
}

/// A land military unit of a major, as first seen.
struct Got {
    player: PlayerId,
    round: u32,
    era: u8,
    kind: UnitKindId,
    purpose: String,
    died: Option<u32>,
    killed_by: String,
    kills: u32,
}

#[derive(Default, Clone, Copy)]
struct SeatTally {
    camps_cleared: u32,
    camps_near_rounds: u32,
    civilians_to_barbarians: u32,
    civilians_to_others: u32,
    military_got: u32,
    military_lost: u32,
    military_lost_to_barbarians: u32,
    barbarians_killed: u32,
}

fn land_military(rules: &Rules, kind: UnitKindId) -> bool {
    kernel::ai::plan::role_of(rules, kind) == kernel::ai::plan::AiRole::Military
}

fn kind_name(rules: &Rules, kind: UnitKindId) -> String {
    rules.units.get(kind.0 as usize).map_or_else(|| "?".to_owned(), |d| d.name.clone())
}

fn rules_role(rules: &Rules, kind: UnitKindId) -> String {
    rules.units.get(kind.0 as usize).and_then(|d| d.role).map_or_else(|| "none".to_owned(), |r| format!("{r:?}"))
}

fn army_role(rules: &Rules, kind: UnitKindId) -> String {
    classic_rules::ai::operations::army_role(rules, kind).map_or_else(|| "none".to_owned(), |r| format!("{r:?}"))
}

/// Who a player is, for the tables: `major`, `barbarian`, `city-state`.
fn owner_kind(game: &Game, p: PlayerId) -> &'static str {
    let players = game.view().auth.players();
    if players.is_barbarian(p) {
        "barbarian"
    } else if players.is_city_state(p) {
        "city-state"
    } else {
        "major"
    }
}

fn play(o: &Options, rules: &Rules, suite: &Suite, m: &Match) -> Result<Out, String> {
    let mut game = R::new_game_at(rules, &m.map, m.seed, suite.rounds, &suite.speed);
    let all = majors(&game);
    let seats: Vec<(PlayerId, Side)> = all.iter().enumerate().map(|(i, p)| (*p, m.seating.side(i))).collect();
    let mind = |name: &str| R::mind(name).ok_or_else(|| format!("no variant {name}"));
    let mut candidates = Vec::new();
    for (p, side) in &seats {
        if *side == Side::Candidate {
            candidates.push((*p, mind(&o.candidate)?));
        }
    }
    let mut driver = AiDriver::with_mind(R::settings(rules), Vec::new(), Seated::new(mind(&o.baseline)?, candidates));
    driver.set_trace_capacity(TRACE);
    driver.record_journal();
    let label = m.label();
    let side_name = |p: PlayerId| seats.iter().find(|(q, _)| *q == p).map_or("other", |(_, s)| if *s == Side::Candidate { "candidate" } else { "baseline" });
    let variant_of = |p: PlayerId| if side_name(p) == "candidate" { o.candidate.as_str() } else { o.baseline.as_str() };
    let mut out = Out::default();

    // The rules' data the tables group by.
    let strategic: Vec<ResourceId> = {
        let mut v: Vec<ResourceId> = rules.units.iter().flat_map(|u| u.required_resources.iter().map(|(r, _)| *r)).collect();
        v.sort();
        v.dedup();
        v
    };
    let military_techs: HashSet<TechId> = (0..rules.units.len())
        .map(|i| UnitKindId(i as u16))
        .filter(|k| land_military(rules, *k) && rules.units[k.0 as usize].domain == Domain::Land)
        .filter_map(|k| rules.units[k.0 as usize].required_tech)
        .collect();

    let mut got: Vec<Got> = Vec::new();
    let mut by_id: HashMap<u64, usize> = HashMap::new();
    let mut known: HashSet<u64> = HashSet::new();
    // (owner, city tile, kind) -> the reason of the order that queued it.
    let mut queued: HashMap<(u8, u32, u16), String> = HashMap::new();
    let mut tally: HashMap<PlayerId, SeatTally> = HashMap::new();
    let mut wars: HashSet<(u8, u8)> = HashSet::new();
    let mut first_war: HashSet<PlayerId> = HashSet::new();
    // (player, role) -> (enemy unit-rounds, own unit-rounds); player -> war rounds.
    let mut faced: BTreeMap<(u8, String), (u64, u64)> = BTreeMap::new();
    let mut war_rounds: HashMap<PlayerId, u32> = HashMap::new();
    // Camp tile -> (appeared, nearest major, distance).
    let mut camps: BTreeMap<u32, (u32, Option<PlayerId>, i32)> = BTreeMap::new();
    let mut cleared_seen: HashSet<(u32, u32)> = HashSet::new();
    let gold_percent = game.view().auth.settings().speed_values().gold_percent;
    let camp_gold = kernel::rules::speeds::SpeedValues::scale(classic_rules::world::barbarians::CAMP_GOLD, gold_percent);

    // A unit made by an upgrade (new id) -> the kind it was.
    let mut upgraded: HashMap<u64, UnitKindId> = HashMap::new();
    let note_units = |game: &Game, round: u32, known: &mut HashSet<u64>, got: &mut Vec<Got>, by_id: &mut HashMap<u64, usize>, queued: &HashMap<(u8, u32, u16), String>, upgraded: &HashMap<u64, UnitKindId>, tally: &mut HashMap<PlayerId, SeatTally>| {
        let auth = game.view().auth;
        for core in auth.units().iter_cores() {
            if !known.insert(core.id.0) {
                continue;
            }
            if !all.contains(&core.owner) || !land_military(rules, core.kind) {
                continue;
            }
            // The unit may have walked out of its city in its first turn: the nearest own city
            // within `FIRST_WALK` tiles that queued its kind.
            let dims = auth.map().dimensions;
            let origin = auth
                .cities()
                .iter()
                .filter(|(_, c)| c.owner == core.owner && queued.contains_key(&(core.owner.0, c.tile.0, core.kind.0)))
                .filter_map(|(_, c)| dims.distance(c.tile, core.tile).filter(|d| *d <= FIRST_WALK).map(|d| (d, c.tile.0)))
                .min();
            let in_city = origin.is_some();
            let purpose = if upgraded.contains_key(&core.id.0) {
                "upgrade".to_owned()
            } else if round == 0 {
                "start".to_owned()
            } else if in_city {
                origin.and_then(|(_, tile)| queued.get(&(core.owner.0, tile, core.kind.0))).cloned().unwrap_or_else(|| "unqueued".to_owned())
            } else {
                "other".to_owned()
            };
            let era = auth.players().nation_era_of(core.owner, rules);
            by_id.insert(core.id.0, got.len());
            got.push(Got { player: core.owner, round, era, kind: core.kind, purpose, died: None, killed_by: String::new(), kills: 0 });
            if !upgraded.contains_key(&core.id.0) {
                tally.entry(core.owner).or_default().military_got += 1;
            }
        }
    };
    note_units(&game, 0, &mut known, &mut got, &mut by_id, &queued, &upgraded, &mut tally);

    while !game.is_over() && game.view().auth.turn().round <= suite.rounds {
        let round = game.view().auth.turn().round;
        loop {
            // Units each turn's start (a unit built in the player's own turn start).
            if driver.phase() == DriverPhase::Observe {
                note_units(&game, round, &mut known, &mut got, &mut by_id, &queued, &upgraded, &mut tally);
            }
            match driver.step(&mut game, rules) {
                StepOutcome::Worked => {}
                StepOutcome::PlayerDone(player) => {
                    for entry in driver.trace().entries() {
                        if entry.outcome != Outcome::Applied || entry.player != player {
                            continue;
                        }
                        let (city, item) = match &entry.command {
                            Some(Command::EnqueueProduction { city, item, .. }) | Some(Command::PurchaseWithFaith { city, item, .. }) => (*city, *item),
                            _ => continue,
                        };
                        if let kernel::ids::ProductionItem::Unit(kind) = item {
                            if let Some(core) = game.view().auth.cities().get(city) {
                                queued.insert((player.0, core.tile.0, kind.0), format!("{:?}", entry.reason));
                            }
                        }
                    }
                    driver.set_trace_capacity(TRACE);
                    let journal = driver.take_journal();
                    for applied in &journal {
                        if let Command::UpgradeUnit { .. } = applied.command {
                            // The old unit goes, a new one of the next kind comes on its tile.
                            let old = applied.changes.unit_changes.iter().find_map(|c| match c {
                                UnitChange::Destroyed { id, kind, .. } => Some((id.0, *kind)),
                                _ => None,
                            });
                            let new = applied.changes.unit_changes.iter().find_map(|c| match c {
                                UnitChange::Created(h) => game.view().auth.units().get(*h).map(|v| v.core.id.0),
                                _ => None,
                            });
                            if let Some((old_id, old_kind)) = old {
                                if let Some(&i) = by_id.get(&old_id) {
                                    got[i].died = Some(round);
                                    got[i].killed_by = "upgraded".to_owned();
                                }
                                if let Some(new_id) = new {
                                    upgraded.insert(new_id, old_kind);
                                }
                            }
                            continue;
                        }
                        let (attacker, defender) = match applied.command {
                            Command::AttackUnit { attacker, defender } => (Some(attacker), Some(defender)),
                            Command::AttackCity { attacker, .. } => (Some(attacker), None),
                            Command::RangedAttack { unit, .. } => (Some(unit), None),
                            _ => (None, None),
                        };
                        let destroyed: Vec<(UnitHandle, u64, PlayerId, UnitKindId)> = applied
                            .changes
                            .unit_changes
                            .iter()
                            .filter_map(|c| match c {
                                UnitChange::Destroyed { unit, id, owner, kind, .. } => Some((*unit, id.0, *owner, *kind)),
                                _ => None,
                            })
                            .collect();
                        let ident = |h: UnitHandle| -> Option<(u64, PlayerId, UnitKindId)> {
                            if let Some(v) = game.view().auth.units().get(h) {
                                return Some((v.core.id.0, v.core.owner, v.core.kind));
                            }
                            destroyed.iter().find(|d| d.0 == h).map(|d| (d.1, d.2, d.3))
                        };
                        for &(_, id, owner, kind) in &destroyed {
                            // Who won: the attacker, unless the attacker itself died.
                            let att = attacker.and_then(ident);
                            let def = defender.and_then(ident);
                            let (winner, attacker_won) = match (att, def) {
                                (Some(a), _) if a.0 != id => (Some(a), true),
                                (Some(_), d) => (d, false),
                                (None, _) => (None, false),
                            };
                            if rules.is_civilian(kind) || !land_military(rules, kind) {
                                // A civilian lost (killed or captured).
                                if all.contains(&owner) && kernel::ai::plan::role_of(rules, kind) != kernel::ai::plan::AiRole::Naval && player != owner {
                                    let by = owner_kind(&game, player);
                                    let t = tally.entry(owner).or_default();
                                    if by == "barbarian" {
                                        t.civilians_to_barbarians += 1;
                                    } else {
                                        t.civilians_to_others += 1;
                                    }
                                    out.civilians.push_str(&format!("{label},{},{},{},{round},{},{by}\n", m.seed, owner.0, side_name(owner), kind_name(rules, kind)));
                                }
                                continue;
                            }
                            let killer = winner.map(|w| w.1).unwrap_or(player);
                            if let Some(&i) = by_id.get(&id) {
                                got[i].died = Some(round);
                                got[i].killed_by = owner_kind(&game, killer).to_owned();
                            }
                            if all.contains(&owner) {
                                let t = tally.entry(owner).or_default();
                                t.military_lost += 1;
                                if owner_kind(&game, killer) == "barbarian" {
                                    t.military_lost_to_barbarians += 1;
                                }
                            }
                            if all.contains(&killer) && owner_kind(&game, owner) == "barbarian" {
                                tally.entry(killer).or_default().barbarians_killed += 1;
                            }
                            if let Some((wid, wowner, wkind)) = winner {
                                if let Some(&i) = by_id.get(&wid) {
                                    got[i].kills += 1;
                                }
                                let era = |p: PlayerId| game.view().auth.players().nation_era_of(p, rules);
                                out.kills.push_str(&format!(
                                    "{label},{},{round},{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
                                    m.seed,
                                    wowner.0,
                                    side_name(wowner),
                                    owner_kind(&game, wowner),
                                    era(wowner),
                                    kind_name(rules, wkind),
                                    rules_role(rules, wkind),
                                    owner.0,
                                    side_name(owner),
                                    owner_kind(&game, owner),
                                    era(owner),
                                    kind_name(rules, kind),
                                    rules_role(rules, kind),
                                    attacker_won
                                ));
                            }
                        }
                        for change in &applied.changes.player_changes {
                            match *change {
                                PlayerChange::WarDeclared { player: a, enemy: b } if all.contains(&a) && all.contains(&b) => {
                                    wars.insert((a.0.min(b.0), a.0.max(b.0)));
                                    for p in [a, b] {
                                        if first_war.insert(p) {
                                            out.wars.push_str(&war_row(&game, rules, &label, m.seed, p, side_name(p), round, &military_techs, &strategic));
                                        }
                                    }
                                }
                                PlayerChange::PeaceMade { player: a, enemy: b } => {
                                    wars.remove(&(a.0.min(b.0), a.0.max(b.0)));
                                }
                                _ => {}
                            }
                        }
                    }
                }
                StepOutcome::RoundDone => break,
                StepOutcome::Waiting if game.is_over() => break,
                StepOutcome::Waiting => return Err(format!("round {round} waits for a human")),
            }
        }
        note_units(&game, round + 1, &mut known, &mut got, &mut by_id, &queued, &upgraded, &mut tally);
        let auth = game.view().auth;
        // The armies at war, by the rules' role.
        for &p in &all {
            let enemies: Vec<PlayerId> = all.iter().copied().filter(|q| *q != p && wars.contains(&(p.0.min(q.0), p.0.max(q.0)))).collect();
            if enemies.is_empty() {
                continue;
            }
            *war_rounds.entry(p).or_default() += 1;
            for core in auth.units().iter_cores() {
                if !land_military(rules, core.kind) {
                    continue;
                }
                let role = rules_role(rules, core.kind);
                if enemies.contains(&core.owner) {
                    faced.entry((p.0, role)).or_default().0 += 1;
                } else if core.owner == p {
                    faced.entry((p.0, role)).or_default().1 += 1;
                }
            }
        }
        // The camps.
        let dims = auth.map().dimensions;
        let live: Vec<TileId> = auth.module().barbarians().camps().iter().map(|c| c.tile).collect();
        for &tile in &live {
            camps.entry(tile.0).or_insert_with(|| {
                let nearest = auth
                    .cities()
                    .iter()
                    .filter(|(_, c)| all.contains(&c.owner))
                    .filter_map(|(_, c)| dims.distance(c.tile, tile).map(|d| (d, c.owner)))
                    .min();
                (round, nearest.map(|(_, p)| p), nearest.map_or(-1, |(d, _)| d))
            });
            for (_, c) in auth.cities().iter() {
                if all.contains(&c.owner) && dims.distance(c.tile, tile).is_some_and(|d| d <= CAMP_NEAR) {
                    tally.entry(c.owner).or_default().camps_near_rounds += 1;
                }
            }
        }
        let gone: Vec<u32> = camps.iter().filter(|(t, (_, _, _))| !live.iter().any(|l| l.0 == **t)).map(|(t, _)| *t).collect();
        for tile in gone {
            let (appeared, nearest, distance) = camps.remove(&tile).expect("a camp");
            let cleared = auth.module().barbarians().cleared().iter().rev().find(|c| c.tile.0 == tile && !cleared_seen.contains(&(tile, c.round))).copied();
            let (by, by_side) = match cleared {
                Some(c) => {
                    cleared_seen.insert((tile, c.round));
                    if all.contains(&c.player) {
                        tally.entry(c.player).or_default().camps_cleared += 1;
                    }
                    (format!("{}:{}", owner_kind(&game, c.player), c.player.0), side_name(c.player))
                }
                None => ("none".to_owned(), "none"),
            };
            out.camps.push_str(&format!("{label},{},{tile},{appeared},{round},{by},{by_side},{},{distance}\n", m.seed, nearest.map_or(-1, |p| i32::from(p.0))));
        }
    }
    let last = game.view().auth.turn().round;
    for (tile, (appeared, nearest, distance)) in &camps {
        out.camps.push_str(&format!("{label},{},{tile},{appeared},,alive,none,{},{distance}\n", m.seed, nearest.map_or(-1, |p| i32::from(p.0))));
    }
    for g in &got {
        out.units.push_str(&format!(
            "{label},{},{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
            m.seed,
            g.player.0,
            side_name(g.player),
            variant_of(g.player),
            g.round,
            g.era,
            kind_name(rules, g.kind),
            rules_role(rules, g.kind),
            army_role(rules, g.kind),
            rules.units.get(g.kind.0 as usize).map_or(0, |d| d.cost),
            g.purpose,
            g.died.map_or(String::new(), |r| r.to_string()),
            g.killed_by,
            g.kills
        ));
    }
    for ((p, role), (enemy, own)) in &faced {
        let pid = PlayerId(*p);
        out.faced.push_str(&format!("{label},{},{p},{},{},{role},{enemy},{own}\n", m.seed, side_name(pid), war_rounds.get(&pid).copied().unwrap_or(0)));
    }
    for &p in &all {
        let s = standing::<R>(&game, rules, p);
        let t = tally.get(&p).copied().unwrap_or_default();
        out.seats.push_str(&format!(
            "{label},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
            m.seed,
            p.0,
            side_name(p),
            variant_of(p),
            s.score,
            s.cities,
            s.techs,
            t.camps_cleared,
            t.camps_cleared * camp_gold,
            t.camps_near_rounds,
            t.civilians_to_barbarians,
            t.civilians_to_others,
            t.military_got,
            t.military_lost,
            t.military_lost_to_barbarians,
            t.barbarians_killed
        ));
    }
    let _ = last;
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
fn war_row(game: &Game, rules: &Rules, label: &str, seed: u64, p: PlayerId, side: &str, round: u32, military_techs: &HashSet<TechId>, strategic: &[ResourceId]) -> String {
    let auth = game.view().auth;
    let techs = auth.players().techs_of(p);
    let mut known: Vec<String> = techs.iter().filter(|t| military_techs.contains(t)).filter_map(|t| rules.technologies.get(t.0 as usize)).map(|t| t.name.clone()).collect();
    known.sort();
    let ledger = classic_rules::derived::resources::resource_ledger(auth, rules, p);
    let res: Vec<String> = strategic
        .iter()
        .map(|r| format!("{}={}/{}", rules.resources.get(r.0 as usize).map_or("?", |d| d.name.as_str()), ledger.total(*r), ledger.available(*r)))
        .collect();
    format!(
        "{label},{seed},{},{side},{round},{},{},{},{}\n",
        p.0,
        auth.players().nation_era_of(p, rules),
        techs.len(),
        known.join(";"),
        res.join(";")
    )
}
