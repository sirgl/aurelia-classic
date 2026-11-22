//! The situation exam (`ai-stress`; classic/docs/ai-stress.md): a real mid-game position, ONE change,
//! the same position played on without it (the control), and the difference between the two
//! (`edited - control`) against a gate written before the first run. A measurement: it changes no
//! rule and no AI.
//!
//! A position is a recorded game replayed to round R (`position`); a situation (`Situation`) turns it
//! into a `Scene` (`scene`): the position with its base (what both arms share: a forced war, a met
//! nation), the edit (only the edited arm) and the roles the metrics read. `play` plays both arms on
//! with the game's own AI on every seat and a `Watch` writes down what happened; `metrics` turns the
//! watch into numbers; `gates` holds the gates and the table of the report.
//!
//! Everything is a function of (log, round, situation): the same instance plays the same.

pub mod edit;
pub mod gates;
pub mod humans;
pub mod metrics;
pub mod play;
pub mod position;
pub mod random;
pub mod scene;
pub mod seat;
pub mod suite;
pub mod tricks;
pub mod watch;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The situations built (the catalogue's names, work/ai-scenarios.html; `P*` are the adversary
/// groups of work/human-playbook-triage.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Situation {
    /// A weak neighbour: its army and its city defenders gone.
    A5,
    /// An invasion: ten enemy units inside the territory, in a war.
    B1,
    /// Runaway science: one rival's science doubled.
    A1,
    /// The capital lost to the nearest rival.
    B4,
    /// Near science victory: a rival holds the spaceship but one part.
    B9,
    /// Two AIs at war with one common enemy.
    D1,
    /// The world congress: the host dies.
    C5,
    /// Sneak stack: an army inside the borders at peace (open borders), the war four rounds later.
    P3,
    /// Early rush: an enemy army next to the capital in rounds 30-50.
    P2,
    /// Wonder snipe: the wonder a city builds is completed by another nation.
    P7,
    /// Border grab: a forward city at the border, citadels next to it.
    P5,
    /// Raid-hunt: scripted raiders capture civilians and pillage.
    P1,
    /// Deal cycle: a friend sells open borders for gold, then declares war (scripted).
    P4,
    /// Alliance sweep: the leader, rich, gifts gold to the city-states every round (scripted).
    P8,
    /// Military x2: one rival's army doubled.
    A2,
    /// Culture x2: one rival's culture doubled.
    A3,
    /// Gold x3: one rival's gold multiplied.
    A4,
    /// A weak neighbour over the sea.
    A6,
    /// We become weak: our army halved.
    A7,
    /// Surprise war: an army at the border at peace, war at once.
    B2,
    /// A friend's betrayal: a friend's army at the border, war at once.
    B5,
    /// A strategic resource lost.
    B6,
    /// Bankrupt and unhappy: the treasury empty, the luxuries gone.
    B7,
    /// An amphibious target: a weak enemy at war reachable only by sea.
    C2,
    /// A defensive pact: the ally's partner is attacked.
    D2,
    /// A losing war: cities and army lost to the enemy.
    D3,
    /// Defence against humans: a seat plays a strong human's trick (`humans`).
    Human(humans::Trick),
}

impl Situation {
    pub const ALL: [Situation; 26] = [
        Situation::A5,
        Situation::B1,
        Situation::A1,
        Situation::B4,
        Situation::B9,
        Situation::D1,
        Situation::C5,
        Situation::P3,
        Situation::P2,
        Situation::P7,
        Situation::P5,
        Situation::P1,
        Situation::P4,
        Situation::P8,
        Situation::A2,
        Situation::A3,
        Situation::A4,
        Situation::A6,
        Situation::A7,
        Situation::B2,
        Situation::B5,
        Situation::B6,
        Situation::B7,
        Situation::C2,
        Situation::D2,
        Situation::D3,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Situation::A5 => "a5",
            Situation::B1 => "b1",
            Situation::A1 => "a1",
            Situation::B4 => "b4",
            Situation::B9 => "b9",
            Situation::D1 => "d1",
            Situation::C5 => "c5",
            Situation::P3 => "p3",
            Situation::P2 => "p2",
            Situation::P7 => "p7",
            Situation::P5 => "p5",
            Situation::P1 => "p1",
            Situation::P4 => "p4",
            Situation::P8 => "p8",
            Situation::A2 => "a2",
            Situation::A3 => "a3",
            Situation::A4 => "a4",
            Situation::A6 => "a6",
            Situation::A7 => "a7",
            Situation::B2 => "b2",
            Situation::B5 => "b5",
            Situation::B6 => "b6",
            Situation::B7 => "b7",
            Situation::C2 => "c2",
            Situation::D2 => "d2",
            Situation::D3 => "d3",
            Situation::Human(t) => t.name(),
        }
    }

    /// Every situation: the catalogue's (`ALL`) and the human tricks.
    pub fn all() -> Vec<Situation> {
        Self::ALL.into_iter().chain(humans::Trick::ALL.into_iter().map(Situation::Human)).collect()
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let all = Self::all();
        let names: Vec<&str> = all.iter().map(|s| s.name()).collect();
        all.iter().copied().find(|s| s.name() == text.to_ascii_lowercase()).ok_or_else(|| format!("no situation {text} ({})", names.join(" ")))
    }

    /// 1: built in wave 1 (`scene`), 2: in wave 2 (`tricks`).
    pub fn wave(self) -> u8 {
        match self {
            Situation::A5 | Situation::B1 | Situation::A1 | Situation::B4 | Situation::B9 | Situation::D1 | Situation::C5 => 1,
            _ => 2,
        }
    }

    /// The round an instance starts at, from the run's base round and jitter: the early rush starts
    /// at rounds 30 to 50 whatever the run asks.
    pub fn start(self, index: usize, round: u32, jitter: u32) -> u32 {
        match self {
            Situation::P2 => position::round_of(index, 30, 20),
            Situation::Human(t) => t.start(index, round, jitter),
            _ => position::round_of(index, round, jitter),
        }
    }

    /// The rounds both arms are played on.
    pub fn window(self) -> u32 {
        match self {
            Situation::A5 => 40,
            Situation::B1 => 15,
            Situation::A1 => 60,
            Situation::B4 => 30,
            Situation::B9 => 40,
            Situation::D1 => 30,
            Situation::C5 => 40,
            Situation::P3 => 20,
            Situation::P2 => 25,
            Situation::P7 => 15,
            Situation::P5 => 30,
            Situation::P1 => 20,
            Situation::P4 => 20,
            Situation::P8 => 40,
            Situation::A2 => 40,
            Situation::A3 => 60,
            Situation::A4 => 40,
            Situation::A6 => 50,
            Situation::A7 => 30,
            Situation::B2 => 15,
            Situation::B5 => 15,
            Situation::B6 => 30,
            Situation::B7 => 30,
            Situation::C2 => 40,
            Situation::D2 => 25,
            Situation::D3 => 30,
            Situation::Human(t) => t.window(),
        }
    }
}

/// One instance of one situation, both arms.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Row {
    pub situation: String,
    pub instance: usize,
    /// The strength of the edit: 0 mild, 1 medium, 2 strong (`Situation::levels`).
    pub level: u8,
    pub position: String,
    pub round: u32,
    pub window: u32,
    pub edited: BTreeMap<String, f64>,
    pub control: BTreeMap<String, f64>,
    pub info: BTreeMap<String, String>,
}

/// A scene saved with a run: enough to build the edited position again exactly (`ai-stress rebuild`)
/// and to play forks from it: the log and the round (the replay prefix), the base and the edit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Saved {
    pub log: String,
    pub round: u32,
    pub situation: String,
    pub level: u8,
    pub instance: usize,
    pub roles: scene::Roles,
    pub base: Vec<edit::Change>,
    pub edit: Vec<edit::Change>,
    pub gift: Option<edit::Gift>,
    /// The scripted adversary of the edited arm (none in wave 1).
    #[serde(default)]
    pub script: Option<seat::Script>,
    /// The world hash of the edited arm at its start (16 hex digits): a rebuild must match it.
    pub world: String,
}

/// What an instance gave: the rows, and the situations that could not be built with the reason.
pub struct Done {
    pub rows: Vec<Row>,
    pub unbuilt: Vec<(String, String)>,
}

/// Plays `situations` at every level on one position: the control once, the edited arm per level.
/// `save` is the directory that keeps each edited start (`Saved`).
pub fn run_instance(rules: &classic_rules::CompiledRuleset, position: &position::Position, instance: usize, situations: &[Situation], save: Option<&std::path::Path>, to_end: bool) -> Done {
    let mut done = Done { rows: Vec::new(), unbuilt: Vec::new() };
    for &situation in situations {
        if let Err(e) = run_situation(rules, position, instance, situation, save, to_end, &mut done.rows) {
            done.unbuilt.push((situation.name().to_owned(), e));
        }
    }
    done
}

/// Plays `situations` on the log of instance `index`: the log replayed to each situation's start
/// round (`Situation::start`, one replay per round), played on by `variant` on every seat (the log's
/// own when none).
#[allow(clippy::too_many_arguments)]
pub fn run_log(rules: &classic_rules::CompiledRuleset, log: &std::path::Path, index: usize, situations: &[Situation], (round, jitter): (u32, u32), variant: Option<classic_rules::ai::variant::Variant>, save: Option<&std::path::Path>, to_end: bool) -> Done {
    let mut by_round: BTreeMap<u32, Vec<Situation>> = BTreeMap::new();
    for s in situations {
        by_round.entry(s.start(index, round, jitter)).or_default().push(*s);
    }
    let mut done = Done { rows: Vec::new(), unbuilt: Vec::new() };
    for (start, todo) in by_round {
        match position::load(rules, log, start) {
            Ok(mut p) => {
                if let Some(v) = variant {
                    p.variant = v;
                }
                let d = run_instance(rules, &p, index, &todo, save, to_end);
                done.rows.extend(d.rows);
                done.unbuilt.extend(d.unbuilt);
            }
            Err(e) => done.unbuilt.extend(todo.iter().map(|s| (s.name().to_owned(), e.clone()))),
        }
    }
    done
}

fn run_situation(rules: &classic_rules::CompiledRuleset, position: &position::Position, instance: usize, situation: Situation, save: Option<&std::path::Path>, to_end: bool, rows: &mut Vec<Row>) -> Result<(), String> {
    let plan = scene::plan(situation, rules, position, instance)?;
    let window = situation.window();
    let start = position.round;
    let edits: Vec<scene::Edit> = (0..situation.levels()).map(|l| scene::edit_of(situation, l, rules, position, &plan)).collect::<Result<_, _>>()?;
    let (control_arm, _) = scene::arms(rules, position, &plan, &edits[0])?;
    let control = measure_arm(rules, &plan, play::play_arm(rules, control_arm, position.variant, window, to_end)?, start, window);
    for (level, edit) in edits.iter().enumerate() {
        let (_, edited_arm) = scene::arms(rules, position, &plan, edit)?;
        let world = kernel::replay::hash::authoritative_hash(&edited_arm.game);
        if let Some(dir) = save {
            let saved = Saved {
                log: position.log.display().to_string(),
                round: start,
                situation: situation.name().to_owned(),
                level: level as u8,
                instance,
                roles: plan.roles,
                base: plan.base.clone(),
                edit: edit.changes.clone(),
                gift: edit.gift,
                script: edit.script.clone(),
                world: format!("{world:016x}"),
            };
            save_scene(dir, &saved)?;
        }
        let edited = measure_arm(rules, &plan, play::play_arm(rules, edited_arm, position.variant, window, to_end)?, start, window);
        let mut info = plan.info.clone();
        info.insert("roles".to_owned(), format!("{:?}", plan.roles));
        rows.push(Row { situation: situation.name().to_owned(), instance, level: level as u8, position: position.name.clone(), round: start, window, edited, control: control.clone(), info });
    }
    Ok(())
}

/// The measures of a played arm: after the window, and at the end of the game when it was played out.
fn measure_arm(rules: &classic_rules::CompiledRuleset, plan: &scene::Plan, played: play::Played, start: u32, window: u32) -> metrics::Measures {
    let mut m = metrics::measure(&plan.roles, rules, &played.window, &played.watch, start, window);
    m.extend(played.script.clone());
    if let Some(end) = &played.end {
        m.extend(metrics::end_outcome(&plan.roles, rules, end));
    }
    m
}

/// `<dir>/<situation>-<instance>-L<level>.json`.
pub fn save_scene(dir: &std::path::Path, saved: &Saved) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{}-{:03}-L{}.json", saved.situation, saved.instance, saved.level));
    let json = serde_json::to_vec_pretty(saved).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| format!("{}: {e}", path.display()))
}

/// The edited world of a saved scene, built again: the log replayed, the base and the edit made.
/// Errs when the world's hash differs from the saved one (the saved scene no longer rebuilds).
pub fn rebuild(rules: &classic_rules::CompiledRuleset, saved: &Saved) -> Result<classic_rules::GameState, String> {
    let position = position::load(rules, std::path::Path::new(&saved.log), saved.round)?;
    let mut game = position.game;
    edit::apply_all(&mut game, rules, &saved.base)?;
    edit::apply_all(&mut game, rules, &saved.edit)?;
    let world = format!("{:016x}", kernel::replay::hash::authoritative_hash(&game));
    if world != saved.world {
        return Err(format!("the rebuilt world is {world}, the saved one {}", saved.world));
    }
    Ok(game)
}

/// One random sample on a position (`random::sample`): the control and the edited arm played, the
/// row's `situation` is "random", its `instance` the sample's number and `info.recipe` the edit.
pub fn run_random(rules: &classic_rules::CompiledRuleset, position: &position::Position, sample: usize, seed: u64, to_end: bool) -> Result<Row, String> {
    let (plan, edit, recipe) = random::sample(rules, position, position::mix(seed ^ sample as u64))?;
    let window = 40;
    let start = position.round;
    let (control_arm, edited_arm) = scene::arms(rules, position, &plan, &edit)?;
    let control = measure_arm(rules, &plan, play::play_arm(rules, control_arm, position.variant, window, to_end)?, start, window);
    let edited = measure_arm(rules, &plan, play::play_arm(rules, edited_arm, position.variant, window, to_end)?, start, window);
    let mut info = plan.info.clone();
    info.insert("recipe".to_owned(), recipe);
    info.insert("changes".to_owned(), serde_json::to_string(&edit.changes).map_err(|e| e.to_string())?);
    info.insert("gift".to_owned(), serde_json::to_string(&edit.gift).map_err(|e| e.to_string())?);
    Ok(Row { situation: "random".to_owned(), instance: sample, level: 0, position: position.name.clone(), round: start, window, edited, control, info })
}
