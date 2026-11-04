//! The games of stage E2: the learner net samples its choices on its seats
//! against opponents from a list, each game kept as its command log and a
//! sidecar of the net's decisions and the outcome (`aurelia_qa::selfplay`,
//! classic/docs/ai-net-data.md, «E2: the net's own games»). Shards are made
//! later from the logs: `ai-features --e2 DIR --out SHARDS`.
//!
//! `cargo run --release -p aurelia-qa --bin ai-selfplay -- --net W.bin --out DIR [options]`
//!
//! --net W[+roles]      the learner's weights (`dummy`: the linear stand-in),
//!                      optionally `+founder,builder` as on the bench (required
//!                      unless --learner-mind)
//! --learner-mind NAME  a bench mind (`pre-war-fix`, `standard` …) on the
//!                      learner's seats instead of the net: a control, no
//!                      decision recorded
//! --out DIR            where the games go (required)
//! --net-seats N        learner seats a game (default 1), consecutive nations
//!                      from the group's index on
//! --opponents LIST     the other seats' minds by the bench's names, dealt in
//!                      turn (default `standard-scripted`, the scripted expert; e.g. `standard,pre-war-fix,net:old.bin`)
//! --suite NAME         the bench suite whose maps and seeds are played
//!                      (default standard)
//! --sizes LIST         keep the suite's maps of these sizes (default all)
//! --rounds R           the last round, the time victory (default: the
//!                      suite's scaled to the speed, 330 for a suite of 500
//!                      at Quick; the speed's last round with --from-logs); a
//!                      victory ends a game before it
//! --speed NAME         the game speed of the games from round 0: quick,
//!                      standard (default), epic or marathon (a mid-game
//!                      start keeps its log's speed)
//! --from-logs DIR      mid-game starts: group g continues the log
//!                      (`*.aurlog` + `.json`, sorted by name, those of
//!                      --sizes) number (seed offset + g) mod their count
//! --from-round R       the round the log is replayed to (its start; default 300)
//! --from-round-jitter J  R ± J, drawn from the group (default 0)
//! --learner-seat S     the nation the learner takes over at R:
//!                      `best-science` (default), `random`, or a player id
//! --give-space-techs   the learner of a share of the groups learns every
//!                      technology of the spaceship (`space_closure`) at R,
//!                      as `ai-finish` gives them (`gave_space_techs` in
//!                      `.e2.json`)
//! --give-space-techs-share P  that share, decided per group (default 0.25)
//! --battle KIND        training ground: battles of KIND (field, siege,
//!                      defence, rush-defence, skirmish, clash) on the suite's seeds: the suite
//!                      game played by `standard-scripted` (the scripted expert) to the war's
//!                      round, then the learner's side against the opponents (`grounds`)
//! --battle-rounds A-B  the war's length in rounds, drawn per group (default 20-40)
//! --battle-army A-B    the main army's units, drawn per group (default 6-10)
//! --battle-difficulty D  the kind's difficulty knob (default 1, the battle
//!                      eval's setting; `grounds::BattleRecord::difficulty`)
//! --battle-stage S     the start stage (the reverse curriculum's knob;
//!                      default none: the documented placement): 0 the
//!                      attackers at the walls (field: the armies in
//!                      contact), S rounds of march farther back, at most
//!                      home (`grounds::BattleRecord::stage`)
//! --battle-minimal     the battle's world as small as the scene allows:
//!                      the smallest map (duel), two nations, no city-states,
//!                      no barbarians (`grounds::MINIMAL_SIZE`; scripts by
//!                      default `grounds::MINIMAL_SCRIPTS`, pangaea): the
//!                      scripted prefix to the war's round a fraction of
//!                      the suite's
//! --battle-vary        siege, skirmish and clash: vary the scenes per group
//!                      (`grounds::Variety`): the war's round within 30-170,
//!                      the target among the enemy's cities, the units
//!                      among the kinds of each role, the side the army
//!                      comes from, every map script by default; a world
//!                      without a war drawn again instead of failing
//! --battle-spawn-order O  siege ladder: the order its scenes create the
//!                      learner's units in (and so their turn's order):
//!                      shooters-first (default; the capture kind last) or
//!                      random (each scene draws shooters first, capturers
//!                      first or shuffled; `grounds::SpawnOrder`)
//! --battle-rung R      siege only: the siege ladder's rung (`grounds::LADDER`,
//!                      in place of --battle-stage; 41 rungs, the scenes
//!                      cleared): the ring of ranged units at the walls and
//!                      fewer of them, the clutter back one element a rung,
//!                      melee only, the one-turn rungs (move, step aside,
//!                      shoot, ride in, then strike), farther back; rung
//!                      41 + S is --battle-stage S. Skirmish: the
//!                      skirmish ladder's rung (`grounds::SKIRMISH`; default
//!                      the last, six a side even): from a 1-HP enemy unit
//!                      next to the learner's down to an even fight, the
//!                      advantage shrinking a step a rung. Clash: the battle
//!                      window's advantage ladder (`grounds::CLASH`: the
//!                      enemy's health and count, the approach stage)
//!                      Clash (a battle window): --battle-army units a side
//!                      (the enemy × the knob), --battle-stage S rounds of
//!                      approach apart (0: in contact, the default), played S
//!                      + --battle-rounds rounds (`grounds::clash_phi`)
//! --battle-scripts L   the map scripts the groups take in turn (default
//!                      pangaea,continents,fractal)
//! --window A-B         training ground: economy windows (with --from-logs):
//!                      play W rounds from the start, W drawn per group in
//!                      A-B, then stop (the game's last round unchanged)
//! --from-rounds LIST   economy windows: the start round drawn per group
//!                      from LIST (e.g. 0,0,50,100,150,200,250; a value
//!                      repeated weighs more), ± the jitter; default
//!                      --from-round. 0: an opening window, a new game of
//!                      the source log's spec from its start
//! --opening-window W0  an opening window's rounds (default 90 Standard
//!                      rounds at the logs' speed: 60 at Quick)
//! --opening W          training ground: openings, the suite's games from
//!                      round 0 played W rounds (0: 135 Standard rounds at
//!                      --speed, 89 at Quick) and stopped, the learner net
//!                      commanding its units only (--net-heads unit) against
//!                      the opponents (`grounds::OpeningScene`)
//! --net-heads LIST     the decision heads the learner net plays (`unit`,
//!                      `research`, `production`, `policy`, `religion`,
//!                      `citizens`); the others stay with `standard`
//!                      (default: every head; `unit` with --opening)
//! --decision-values K,D,R[,S[,L]]  battles: value each of the net's unit
//!                      decisions by the search teacher's rollouts (`search::Values`):
//!                      the net's choice and the K candidates the previews
//!                      keep, R rollouts of D round ends each (the same combat
//!                      seeds for all; the first seed tying them all ends it), a
//!                      share S of the learner's first turn's decisions (default 1)
//!                      and L of the later turns' (default S);
//!                      kept in `.e2.json` `decision_values`, exported as
//!                      `e2_dvalues.npy` (the trainer's `--decision-advantage`)
//! --ladder-solvable    siege ladder: a scene the shallow search teacher
//!                      (`search:depth=0,rollouts=1`) does not take in the
//!                      learner's first turn on one of two fixed combat seeds
//!                      is drawn again (its scene seed with the attempt), up to
//!                      `selfplay::LADDER_DRAWS` draws; none solvable: the first
//!                      (`.e2.json` `ladder_draw`; a summary line at the end)
//! --ladder-pool FILE    siege ladder: on a rung the pool has (JSON,
//!                      `selfplay::LadderPool`, made by tools/ladder-pool.py)
//!                      every group plays a scene of the pool, solvable by the
//!                      shallow search teacher (first turn where the scene
//!                      allows one, else within the war); other rungs as
//!                      without it (with --ladder-solvable: probed)
//! --games N            games to play (default 1): game i is member i % K of
//!                      group i / K
//! --group K            games of one start differing only in the sampling
//!                      seed (default 1; GRPO groups)
//! --seed-offset S      the suite's seed offset of the first group (default
//!                      0); with --from-logs the first group's log
//! --temperature T      sampling temperature on the Q8 logits (default 1; 0:
//!                      argmax)
//! --sample-seed X      salt of the sampling seeds (default 0)
//! --jobs J             games played at once (default 1)
//!
//! A game whose `<stem>.e2.json` exists in DIR is skipped: a run resumes
//! where it stopped.

use std::path::PathBuf;
use std::time::Instant;

use aurelia_qa::bench::modules::{BenchRules, ClassicBench};
use aurelia_qa::bench::suite::Suite;
use aurelia_qa::export::parallel;
use aurelia_qa::grounds::{self, BattleKind, Grounds};
use aurelia_qa::selfplay::{done, jobs, play, write, Config, FromLogs, LearnerSeat};

struct Options {
    out: Option<PathBuf>,
    net: Option<String>,
    net_seats: usize,
    opponents: Vec<String>,
    suite: String,
    sizes: Vec<String>,
    rounds: Option<u32>,
    games: usize,
    group: usize,
    offset: u64,
    temperature: f64,
    sample_seed: u64,
    jobs: usize,
    learner_mind: Option<String>,
    from_logs: Option<PathBuf>,
    from_round: u32,
    from_jitter: u32,
    learner_seat: LearnerSeat,
    speed: Option<String>,
    give_space_techs: bool,
    give_space_techs_share: f64,
    battle: Option<String>,
    battle_rounds: (u32, u32),
    battle_army: (u32, u32),
    battle_scripts: Vec<String>,
    battle_difficulty: f64,
    battle_stage: Option<u32>,
    battle_rung: Option<u32>,
    battle_minimal: bool,
    battle_spawn_random: bool,
    battle_vary: bool,
    battle_scripts_set: bool,
    window: Option<(u32, u32)>,
    from_rounds: Vec<u32>,
    opening_window: Option<u32>,
    opening: Option<u32>,
    net_heads: Option<String>,
    decision_values: Option<aurelia_qa::search::ValuesConfig>,
    ladder_solvable: bool,
    ladder_pool: Option<PathBuf>,
}

fn list(value: &str) -> Vec<String> {
    value.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned).collect()
}

fn options() -> Result<Options, String> {
    let mut o = Options {
        out: None,
        net: None,
        net_seats: 1,
        opponents: vec![aurelia_qa::bench::modules::SCRIPTED_EXPERT.to_owned()],
        suite: "standard".to_owned(),
        sizes: Vec::new(),
        rounds: None,
        games: 1,
        group: 1,
        offset: 0,
        temperature: 1.0,
        sample_seed: 0,
        jobs: 1,
        learner_mind: None,
        from_logs: None,
        from_round: 300,
        from_jitter: 0,
        learner_seat: LearnerSeat::BestScience,
        give_space_techs: false,
        give_space_techs_share: 0.25,
        speed: None,
        battle: None,
        battle_rounds: (20, 40),
        battle_army: (6, 10),
        battle_scripts: aurelia_qa::grounds::BATTLE_SCRIPTS.iter().map(|s| (*s).to_owned()).collect(),
        battle_difficulty: 1.0,
        battle_stage: None,
        battle_rung: None,
        battle_minimal: false,
        battle_spawn_random: false,
        battle_vary: false,
        battle_scripts_set: false,
        window: None,
        from_rounds: Vec::new(),
        opening_window: None,
        opening: None,
        net_heads: None,
        decision_values: None,
        ladder_solvable: false,
        ladder_pool: None,
    };
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{flag} needs a value"));
        let number = |v: String| v.parse::<u64>().map_err(|e| format!("{flag}: {e}"));
        match flag.as_str() {
            "--out" => o.out = Some(PathBuf::from(value()?)),
            "--net" => o.net = Some(value()?),
            "--net-seats" => o.net_seats = number(value()?)? as usize,
            "--opponents" => o.opponents = list(&value()?),
            "--suite" => o.suite = value()?,
            "--sizes" => o.sizes = list(&value()?),
            "--rounds" => o.rounds = Some(number(value()?)? as u32),
            "--games" => o.games = number(value()?)? as usize,
            "--group" => o.group = number(value()?)? as usize,
            "--seed-offset" => o.offset = number(value()?)?,
            "--temperature" => o.temperature = value()?.parse().map_err(|e| format!("--temperature: {e}"))?,
            "--sample-seed" => o.sample_seed = number(value()?)?,
            "--jobs" => o.jobs = number(value()?)? as usize,
            "--learner-mind" => o.learner_mind = Some(value()?),
            "--from-logs" => o.from_logs = Some(PathBuf::from(value()?)),
            "--from-round" => o.from_round = number(value()?)? as u32,
            "--from-round-jitter" => o.from_jitter = number(value()?)? as u32,
            "--learner-seat" => o.learner_seat = LearnerSeat::parse(&value()?)?,
            "--give-space-techs" => o.give_space_techs = true,
            "--give-space-techs-share" => {
                o.give_space_techs_share = value()?.parse().ok().filter(|p: &f64| (0.0..=1.0).contains(p)).ok_or("--give-space-techs-share needs a share in 0 … 1")?;
            }
            "--speed" => o.speed = Some(value()?),
            "--battle" => o.battle = Some(value()?),
            "--battle-rounds" => o.battle_rounds = grounds::parse_range(&value()?, "--battle-rounds")?,
            "--battle-army" => o.battle_army = grounds::parse_range(&value()?, "--battle-army")?,
            "--battle-scripts" => {
                o.battle_scripts = list(&value()?);
                o.battle_scripts_set = true;
            }
            "--battle-difficulty" => o.battle_difficulty = value()?.parse().ok().filter(|d: &f64| d.is_finite() && *d > 0.0).ok_or("--battle-difficulty needs a number > 0")?,
            "--battle-stage" => o.battle_stage = Some(number(value()?)? as u32),
            "--battle-rung" => o.battle_rung = Some(number(value()?)? as u32),
            "--battle-minimal" => o.battle_minimal = true,
            "--battle-spawn-order" => {
                o.battle_spawn_random = match value()?.as_str() {
                    "random" => true,
                    "shooters-first" => false,
                    other => return Err(format!("--battle-spawn-order: {other} (shooters-first, random)")),
                }
            }
            "--battle-vary" => o.battle_vary = true,
            "--window" => o.window = Some(grounds::parse_range(&value()?, "--window")?),
            "--from-rounds" => o.from_rounds = list(&value()?).iter().map(|v| v.parse::<u32>().map_err(|e| format!("--from-rounds: {e}"))).collect::<Result<_, _>>()?,
            "--opening-window" => o.opening_window = Some(number(value()?)? as u32).filter(|w| *w > 0).ok_or("--opening-window needs rounds > 0").map(Some)?,
            "--opening" => o.opening = Some(number(value()?)? as u32),
            "--net-heads" => o.net_heads = Some(value()?),
            "--decision-values" => o.decision_values = Some(aurelia_qa::search::ValuesConfig::parse(&value()?)?),
            "--ladder-solvable" => o.ladder_solvable = true,
            "--ladder-pool" => o.ladder_pool = Some(PathBuf::from(value()?)),
            other => return Err(format!("unknown option {other}")),
        }
    }
    if o.net_seats == 0 || o.group == 0 || o.opponents.is_empty() {
        return Err("--net-seats, --group and --opponents need at least one".to_owned());
    }
    if !(o.temperature >= 0.0 && o.temperature.is_finite()) {
        return Err("--temperature needs a number ≥ 0".to_owned());
    }
    Ok(o)
}

/// An opening window's rounds: `--opening-window`, else
/// `OPENING_STANDARD_ROUNDS` at the logs' speed (one speed for all).
fn opening_window(o: &Options, rules: &classic_rules::CompiledRuleset, from: &FromLogs) -> Result<u32, String> {
    if let Some(w) = o.opening_window {
        return Ok(w);
    }
    let speeds: std::collections::BTreeSet<&str> = from.logs.iter().map(|(_, spec)| spec.speed.as_str()).collect();
    if speeds.len() > 1 {
        return Err(format!("the logs are of several speeds ({speeds:?}): give --opening-window"));
    }
    let key = speeds.into_iter().next().unwrap_or("");
    let values = if key.is_empty() { kernel::rules::speeds::SpeedValues::STANDARD } else { rules.speed(key).ok_or_else(|| format!("unknown speed {key} in the logs"))?.values };
    Ok(values.game_round(grounds::OPENING_STANDARD_ROUNDS).max(1))
}

fn main() {
    if let Err(error) = options().and_then(|o| run(&o)) {
        eprintln!("ai-selfplay: {error}");
        std::process::exit(1);
    }
}

fn run(o: &Options) -> Result<(), String> {
    let out = o.out.as_ref().ok_or("--out DIR is required")?;
    let net = match (&o.net, &o.learner_mind) {
        (Some(net), _) => net.clone(),
        (None, Some(mind)) => format!("mind:{mind}"),
        (None, None) => return Err("--net W.bin (or --learner-mind NAME) is required".to_owned()),
    };
    let suite = Suite::named(&o.suite).ok_or_else(|| format!("unknown suite {} ({})", o.suite, Suite::NAMES.join(", ")))?;
    let from = match &o.from_logs {
        Some(dir) => {
            let share = if o.give_space_techs { o.give_space_techs_share } else { 0.0 };
            Some(FromLogs { give_space_techs: share, ..FromLogs::read(dir, &o.sizes, o.from_round, o.from_jitter, o.learner_seat)? })
        }
        None => None,
    };
    let rules = ClassicBench::rules();
    let speed = match &o.speed {
        None => None,
        Some(name) => Some(rules.speed_named(name).ok_or_else(|| {
            let known: Vec<_> = rules.speeds.iter().map(|s| s.name.to_ascii_lowercase()).collect();
            format!("unknown speed {name} ({})", known.join(", "))
        })?),
    };
    let values = speed.map_or(kernel::rules::speeds::SpeedValues::STANDARD, |s| s.values);
    let rounds = o.rounds.unwrap_or(if from.is_some() { values.max_rounds() } else { values.game_round(suite.rounds) });
    let latest = o.from_rounds.iter().copied().max().unwrap_or(o.from_round);
    if from.is_some() && rounds <= latest {
        return Err(format!("--rounds {rounds} must come after the start round {latest}"));
    }
    let opening = match o.opening {
        None => None,
        Some(_) if o.battle.is_some() || o.window.is_some() || from.is_some() => return Err("--opening is a ground of its own, from the suite's games: no --battle, --window or --from-logs".to_owned()),
        Some(0) => Some(Grounds::Opening { window: values.game_round(grounds::OPENING_GROUND_STANDARD_ROUNDS).max(1) }),
        Some(w) => Some(Grounds::Opening { window: w }),
    };
    grounds::set_random_spawn_order(o.battle_spawn_random);
    let grounds = match (&o.battle, o.window) {
        _ if opening.is_some() => opening,
        (Some(_), Some(_)) => return Err("--battle and --window are two grounds: one at a time".to_owned()),
        (Some(_), None) if from.is_some() => return Err("--battle starts from the suite's games, not --from-logs".to_owned()),
        (Some(kind), None) if o.battle_vary && !["siege", "skirmish", "clash"].contains(&kind.as_str()) => return Err("--battle-vary is the siege's, the skirmish's and the clash's".to_owned()),
        (Some(kind), None) if o.battle_rung.is_some() && (!["siege", "skirmish", "clash"].contains(&kind.as_str()) || o.battle_stage.is_some()) => {
            return Err("--battle-rung is the siege's, the skirmish's and the clash's, in place of --battle-stage".to_owned())
        }
        (Some(kind), None) => Some(Grounds::Battle { kind: BattleKind::parse(kind)?, rounds: o.battle_rounds, army: o.battle_army, scripts: match (o.battle_scripts_set, o.battle_vary, o.battle_minimal) {
                (false, true, _) => grounds::vary_scripts(),
                (false, false, true) => grounds::MINIMAL_SCRIPTS.iter().map(|s| (*s).to_owned()).collect(),
                _ => o.battle_scripts.clone(),
            },
            difficulty: o.battle_difficulty,
            stage: o.battle_stage,
            rung: o.battle_rung,
            minimal: o.battle_minimal,
            vary: o.battle_vary,
        }),
        (None, Some(window)) if from.is_some() => Some(Grounds::Economy { rounds: o.from_rounds.clone(), window, opening: opening_window(o, &rules, from.as_ref().expect("the logs"))? }),
        (None, Some(_)) => return Err("--window needs --from-logs".to_owned()),
        (None, None) => None,
    };
    let config = Config {
        suite,
        sizes: o.sizes.clone(),
        rounds,
        net,
        net_seats: o.net_seats,
        opponents: o.opponents.clone(),
        temperature: o.temperature,
        group: o.group,
        seed_offset: o.offset,
        sample_seed: o.sample_seed,
        from,
        learner_mind: o.learner_mind.clone(),
        speed: speed.filter(|s| !s.values.is_standard()).map_or_else(String::new, |s| s.key.clone()),
        heads: o.net_heads.clone().or_else(|| o.opening.map(|_| "unit".to_owned())),
        decision_values: o.decision_values.clone(),
        ladder_solvable: o.ladder_solvable,
        grounds,
    };
    if let Some(speed) = speed {
        println!("speed {}: the last round {rounds}", speed.name);
    }
    if let Some(path) = &o.ladder_pool {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        aurelia_qa::selfplay::set_ladder_pool(serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))?);
    }
    let mut all = jobs(&config, o.games);
    let pooled = aurelia_qa::selfplay::pool_jobs(&config, &mut all)?;
    if pooled > 0 {
        println!("ladder pool: {pooled} of {} games play a pooled scene", all.len());
    }
    if all.len() < o.games {
        eprintln!("ai-selfplay: the suite's maps give {} games, not {}", all.len(), o.games);
    }
    let todo: Vec<_> = all.into_iter().filter(|j| !done(out, j)).collect();
    println!("{} games to play ({} done already)", todo.len(), o.games.saturating_sub(todo.len()));
    let started = Instant::now();
    let results = parallel(&todo, o.jobs, |job| {
        let played = play(&rules, &config, job)?;
        write(out, &played).map_err(|e| format!("{}: {e}", played.meta.stem))
    });
    let (mut failed, mut log_bytes, mut sidecar_bytes, mut decisions) = (0, 0, 0, 0);
    // `--ladder-solvable`: scenes, scenes redrawn, draws probed, scenes none of whose draws was solvable.
    let (mut scenes, mut redrawn, mut draws, mut unsolved) = (0u32, 0u32, 0u32, 0u32);
    for (job, result) in todo.iter().zip(results) {
        match result {
            Ok(meta) => {
                if let Some(d) = meta.ladder_draw {
                    scenes += 1;
                    redrawn += u32::from(d.attempt > 0);
                    draws += d.draws;
                    unsolved += u32::from(!d.solved);
                }
                log_bytes += meta.log_bytes;
                sidecar_bytes += meta.sidecar_bytes;
                decisions += meta.decisions;
                let learners: Vec<String> = meta
                    .seats
                    .iter()
                    .filter(|s| s.learner)
                    .map(|s| {
                        let won = s.victory.as_ref().map_or_else(String::new, |v| format!(" won {v} {}", s.victory_round.unwrap_or(0)));
                        let x = &s.space;
                        format!(
                            "p{} rank {} share {:.2} score {}{won}; space techs {}/{}, projects {}/{}, parts {} ({} installed)/{}, science {}",
                            s.player, s.rank, s.points_share, s.score, x.space_techs, x.space_techs_needed, x.projects, x.projects_needed, x.parts, x.parts_installed, x.parts_needed, x.science
                        )
                    })
                    .collect();
                let given = if meta.gave_space_techs { " with the space techs" } else { "" };
                let from = meta.from_round.map_or_else(String::new, |r| format!(" from {r}{given}"));
                let ground = match &meta.ground {
                    Some(g) if g.opening.is_some() => {
                        let o = g.opening.as_ref().expect("an opening");
                        let last = o.round.len().saturating_sub(1);
                        let mine: Vec<String> = o
                            .seats
                            .iter()
                            .filter(|s| s.learner)
                            .map(|s| {
                                format!(
                                    "p{} cities {} improved {} worked {} explored {}/{} met {}/{} cs {}/{} rival cities {} capitals {} camps {} lost {} ({}, civilians {}) potential {:.0}",
                                    s.player,
                                    s.cities[last],
                                    s.improved.get(last).copied().unwrap_or(0),
                                    s.improved_worked.get(last).copied().unwrap_or(0),
                                    s.explored[last],
                                    s.tiles,
                                    s.majors_met[last],
                                    s.rivals,
                                    s.city_states_met[last],
                                    s.city_states,
                                    s.rival_cities_seen[last],
                                    s.rival_capitals_known[last],
                                    s.camps_cleared[last],
                                    s.units_lost[last],
                                    s.lost_value[last],
                                    s.civilians_lost.get(last).copied().unwrap_or(0),
                                    s.potential[last]
                                )
                            })
                            .collect();
                        format!("; opening to {}, reward {:.3}: {}", o.end_round, o.reward, mine.join("; "))
                    }
                    Some(g) => match (&g.battle, &g.economy) {
                        (Some(b), _) => format!(
                            "; {} difficulty {:.2}{} rounds {}-{}, reward {:.3}, lost {:.2} / enemy {:.2}, captured {:?}, held {}, target damage {:.2}, peace rounds {}{}",
                            g.kind,
                            b.difficulty,
                            b.rung.map_or_else(String::new, |r| format!(" rung {r} need {:?}", b.need)) + &b.stage.map_or_else(|| b.rung.map_or_else(String::new, |_| format!(" gap {}", b.start_gap.unwrap_or(-1))), |s| format!(" stage {s} gap {}", b.start_gap.unwrap_or(-1))),
                            b.start_round,
                            b.end_round,
                            b.reward,
                            b.learner_army.lost_share(),
                            b.enemy_army.lost_share(),
                            b.captured_round,
                            b.held,
                            b.target_damage,
                            b.peace_rounds,
                            if g.kind == "skirmish" { format!(", won {:?}", b.won_round) } else { String::new() }
                        ),
                        (_, Some(e)) => {
                            let ratio = |net: f64, expert: Option<f64>| expert.filter(|x| *x > 0.0).map_or_else(|| "-".to_owned(), |x| format!("{:.3}", net / x));
                            format!(
                                "; window {}-{}, science {:.0} vs {:.0}, share {:.3}; rate {:.0} / expert {} = {}; potential {:.0} / expert {} = {}",
                                e.from_round,
                                e.end_round,
                                e.learner_science,
                                e.opponents_mean,
                                e.science_share,
                                e.learner_rate,
                                e.expert_rate.map_or_else(|| "-".to_owned(), |x| format!("{x:.0}")),
                                ratio(e.learner_rate, e.expert_rate),
                                e.learner_potential,
                                e.expert_potential.map_or_else(|| "-".to_owned(), |x| format!("{x:.0}")),
                                ratio(e.learner_potential, e.expert_potential)
                            )
                        }
                        _ => String::new(),
                    },
                    None => String::new(),
                };
                println!(
                    "{}:{from} {} rounds, {} decisions, log {:.1} KB, sidecar {:.1} KB, {:.1} s; {}{ground}",
                    meta.stem,
                    meta.final_round,
                    meta.decisions,
                    meta.log_bytes as f64 / 1024.0,
                    meta.sidecar_bytes as f64 / 1024.0,
                    meta.seconds,
                    learners.join("; ")
                );
            }
            Err(error) => {
                failed += 1;
                eprintln!("{}: {error}", job.stem());
            }
        }
    }
    let played = todo.len() - failed;
    let secs = started.elapsed().as_secs_f64();
    println!(
        "{played} games in {secs:.1} s ({:.1} games/hour), {decisions} decisions, {:.1} KB log and {:.1} KB sidecar per game",
        played as f64 * 3600.0 / secs.max(1e-9),
        log_bytes as f64 / 1024.0 / played.max(1) as f64,
        sidecar_bytes as f64 / 1024.0 / played.max(1) as f64
    );
    if scenes > 0 {
        println!("ladder filter: {scenes} scenes, {redrawn} redrawn ({:.3}), {unsolved} unsolvable in {} draws ({:.3}), {draws} draws probed", f64::from(redrawn) / f64::from(scenes), aurelia_qa::selfplay::LADDER_DRAWS, f64::from(unsolved) / f64::from(scenes));
    }
    if failed > 0 {
        return Err(format!("{failed} games failed"));
    }
    Ok(())
}
