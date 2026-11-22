//! The trainers (`ai-exam suite`, classic/docs/ai-trainers.md): a library of short combat scenes,
//! task × era × composition × ground, each a function of its index, played by an AI variant (or a
//! script) on the learner's seat against `standard` or a scripted human on the other. A measurement:
//! it changes no rule and no AI.
//!
//! - `lab`: the era's world (repainted flat, the nations at war, the era's technologies, a home city
//!   each in a far corner) and the kinds of each era and arm, from the rules' data.
//! - `families`: the tasks: how a scene is drawn, its win rule (fixed in `families.rs` before any
//!   run), its references (the solvers) and the scripted human of a defence scene.
//! - `act`: what scripted seats do (the references, the scripted humans, passive, random).
//! - `suite`: draws, the proof that an instance is winnable (a solver wins it and the passive
//!   control does not; else the instance is drawn again, attempt by attempt), the players, the rows.
//!
//! The arena is CLOSED: a unit of either seat that is not one of the scene's pieces (a home city
//! trains or buys one) is taken off the map at every round end, so the scene's units are the only
//! units that fight.

pub mod act;
pub mod families;
pub mod lab;
pub mod suite;

use std::collections::{BTreeMap, BTreeSet};

use classic_rules::ai::driver::ClassicMind;
use classic_rules::classic::Classic;
use classic_rules::ids::{PlayerId, TileId, UnitHandle, UnitKindId};
use classic_rules::{CompiledRuleset, GameState};
use kernel::ai::driver::AiDriver;
use kernel::ai::state::{ObjectiveKind, SeenCity, UnitTask, WarObjective};
use kernel::ids::EntityId;
use serde::{Deserialize, Serialize};

use crate::bench::modules::{majors, BenchRules, ClassicBench};
use families::Fam;
use lab::Draw;

/// A scene unit.
#[derive(Debug, Clone)]
pub struct Piece {
    pub h: UnitHandle,
    pub id: EntityId,
    pub kind: UnitKindId,
    pub role: &'static str,
}

/// A built scene: the game to play and what it asks.
#[derive(Clone)]
pub struct Scene {
    pub fam: Fam,
    pub era: u8,
    pub instance: usize,
    pub attempt: u32,
    pub learner: PlayerId,
    pub enemy: PlayerId,
    pub mine: Vec<Piece>,
    pub theirs: Vec<Piece>,
    /// The city to take (or hold), the tile to hold, the learner's refuge.
    pub target: Option<TileId>,
    pub hold: Option<TileId>,
    pub refuge: Option<TileId>,
    /// Improved tiles of the raid scenes.
    pub improved: Vec<TileId>,
    /// The arena centre.
    pub centre: TileId,
    pub rounds: u32,
    /// The enemy's AI units are given the hunt of the nearest learner unit every round.
    pub hunt: bool,
    /// The enemy's scene units are played by the scripted hunter (`families::enemy_turn`).
    pub hunter: bool,
    /// War objectives set before the first round (the learner's aim, the enemy's).
    pub aim_learner: Option<TileId>,
    pub aim_enemy: Option<TileId>,
    /// The scene's context (opportunity scenes) and what it expects.
    pub context: Option<String>,
    pub info: BTreeMap<String, String>,
    /// The learner's city count at the start (a founding scene).
    pub cities0: usize,
    pub start_round: u32,
    // State.
    pub pillaged: BTreeSet<TileId>,
    pub taken: Option<u32>,
    pub founded: Option<u32>,
    pub crossed: bool,
    pub city_low: bool,
    pub hold_lost: Option<u32>,
    pub first_kill: Option<u32>,
    pub kills_by: BTreeMap<u32, u32>,
}

impl Scene {
    pub fn mine_alive(&self, game: &GameState) -> Vec<(Piece, kernel::world::unit::UnitCore)> {
        self.mine.iter().filter_map(|p| act::alive(game, p, self.learner).map(|c| (p.clone(), c))).collect()
    }
    pub fn theirs_alive(&self, game: &GameState) -> Vec<(Piece, kernel::world::unit::UnitCore)> {
        self.theirs.iter().filter_map(|p| act::alive(game, p, self.enemy).map(|c| (p.clone(), c))).collect()
    }
    pub fn label(&self, rules: &CompiledRuleset) -> String {
        format!("{} {} #{}.{}", self.fam.name(), lab::era_names(rules).get(self.era as usize).cloned().unwrap_or_default(), self.instance, self.attempt)
    }
}

/// One played instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Row {
    pub family: String,
    pub era: String,
    pub instance: usize,
    pub attempt: u32,
    pub player: String,
    pub win: bool,
    pub rounds: u32,
    pub measures: BTreeMap<String, f64>,
    pub info: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trace: Vec<String>,
}

/// Who plays the learner's seat: an AI variant, or a script on top of `standard` (which plays the rest
/// of the seat: the home city).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Seat {
    Mind(String),
    /// A family reference (`ref-<name>`).
    Script(String),
    Passive,
    Random,
}

impl Seat {
    pub fn parse(name: &str) -> Seat {
        match name {
            "passive" => Seat::Passive,
            "random" => Seat::Random,
            n if n.starts_with("ref-") => Seat::Script(n.to_owned()),
            n => Seat::Mind(n.to_owned()),
        }
    }
}

fn mind(n: &str) -> Result<ClassicMind, String> {
    ClassicBench::mind(n).ok_or_else(|| format!("no AI variant {n}"))
}

/// The tiles within `reach` of the scene's centre, shown to both sides.
fn reveal(game: &mut GameState, scene: &Scene) {
    let tiles = lab::tiles_within(game, scene.centre, lab::ARENA + 4);
    game.debug_reveal(scene.learner, &tiles);
    game.debug_reveal(scene.enemy, &tiles);
}

/// The war memory a side's AI has before the first round: the objective city (a declaration of war
/// leaves it, `battle::aim`).
fn aim<P: kernel::ai::mind::AiMind<Classic>>(driver: &mut AiDriver<Classic, P>, game: &GameState, side: PlayerId, target: TileId, round: u32) {
    let auth = game.view().auth;
    let Some(city) = auth.indexes().city_on_tile(target).and_then(|c| auth.cities().get(c)) else { return };
    let state = driver.state_mut(side, round);
    state.see_city(SeenCity { tile: target, owner: city.owner, population: city.population, last_seen_round: round });
    state.relation_mut(city.owner).objective = Some(WarObjective { tile: target, kind: ObjectiveKind::City, chosen_round: round });
}

/// The enemy's AI units hunt the nearest learner unit (the exam's T1 foes).
fn hunt<P: kernel::ai::mind::AiMind<Classic>>(driver: &mut AiDriver<Classic, P>, game: &GameState, scene: &Scene) {
    let round = game.view().auth.turn().round;
    let mine = scene.mine_alive(game);
    for (r, rc) in scene.theirs_alive(game) {
        if let Some((_, c)) = mine.iter().min_by_key(|(_, c)| (lab::dist(game, c.tile, rc.tile), c.tile.0)) {
            let state = driver.state_mut(scene.enemy, round);
            state.relation_mut(scene.learner).objective = Some(WarObjective { tile: c.tile, kind: ObjectiveKind::Territory, chosen_round: round });
            state.set_task(r.id, UnitTask::Assault { target: c.tile }, round);
        }
    }
}

/// The closed arena: every unit of the two seats that is not a scene piece is taken off.
fn close_arena(rules: &CompiledRuleset, game: &mut GameState, scene: &Scene) {
    let ids: BTreeSet<EntityId> = scene.mine.iter().chain(&scene.theirs).map(|p| p.id).collect();
    let gone: Vec<UnitHandle> = {
        let auth = game.view().auth;
        auth.units().iter_handles().filter(|(_, c)| (c.owner == scene.learner || c.owner == scene.enemy) && !ids.contains(&c.id)).map(|(h, _)| h).collect()
    };
    for h in gone {
        game.debug_remove_unit(rules, h);
    }
}

/// Plays `scene` on `game` with `player` on the learner's seat. `trace`: one line a round in the row.
pub fn play(rules: &CompiledRuleset, mut game: GameState, mut scene: Scene, player: &str, trace: bool) -> Result<Row, String> {
    let seat = Seat::parse(player);
    let base = match &seat {
        Seat::Mind(n) => n.clone(),
        _ => "standard".to_owned(),
    };
    let mut seats = Vec::new();
    for p in majors(&game) {
        seats.push((p, if p == scene.learner { mind(&base)? } else { mind("standard")? }));
    }
    let mut driver = AiDriver::<Classic, kernel::ai::seats::Seated<ClassicMind>>::with_mind(ClassicBench::settings(rules), Vec::new(), kernel::ai::seats::Seated::new(mind("standard")?, seats));
    driver.set_trace_capacity(0);
    reveal(&mut game, &scene);
    let round0 = game.view().auth.turn().round;
    scene.start_round = round0;
    if let Some(t) = scene.aim_learner {
        aim(&mut driver, &game, scene.learner, t, round0);
    }
    if let Some(t) = scene.aim_enemy {
        aim(&mut driver, &game, scene.enemy, t, round0);
    }
    if scene.hunt {
        hunt(&mut driver, &game, &scene);
    }
    let mut rows_trace = Vec::new();
    if trace {
        rows_trace.push(families::line(rules, &game, &scene, 0));
    }
    let rnd = Draw::new(lab::mix(0xa11d ^ (scene.instance as u64) << 8 ^ u64::from(scene.attempt) << 40 ^ (scene.fam as u64) << 48 ^ u64::from(scene.era) << 56));
    let mut verdict: Option<bool> = None;
    let mut scripted_mine: Option<u32> = None;
    let mut scripted_theirs: Option<u32> = None;
    let mut guard = 0;
    while verdict.is_none() && !game.is_over() && guard < 120 {
        guard += 1;
        loop {
            let now = game.view().auth.turn().round;
            let active = game.view().auth.active_player();
            if active == scene.learner && scripted_mine != Some(now) && !game.is_over() {
                scripted_mine = Some(now);
                match &seat {
                    Seat::Mind(_) => {}
                    Seat::Script(name) => {
                        families::reference_turn(rules, &mut game, &mut scene, name);
                    }
                    Seat::Passive => {
                        act::passive_turn(&mut game, rules, scene.learner, &scene.mine);
                    }
                    Seat::Random => {
                        act::random_turn(&mut game, rules, scene.learner, &scene.mine, &rnd);
                    }
                }
            }
            if active == scene.enemy && scripted_theirs != Some(now) && !game.is_over() {
                scripted_theirs = Some(now);
                families::enemy_turn(rules, &mut game, &mut scene);
            }
            match driver.step(&mut game, rules) {
                kernel::ai::StepOutcome::Worked | kernel::ai::StepOutcome::PlayerDone(_) => {}
                kernel::ai::StepOutcome::RoundDone => break,
                kernel::ai::StepOutcome::Waiting if game.is_over() => break,
                kernel::ai::StepOutcome::Waiting => return Err(format!("round {} waits for a human", game.view().auth.turn().round)),
            }
        }
        close_arena(rules, &mut game, &scene);
        let rel = game.view().auth.turn().round.saturating_sub(round0);
        verdict = families::judge(rules, &game, &mut scene, rel);
        if trace {
            rows_trace.push(families::line(rules, &game, &scene, rel));
        }
        if verdict.is_none() && scene.hunt {
            hunt(&mut driver, &game, &scene);
        }
    }
    let rel = game.view().auth.turn().round.saturating_sub(round0);
    let win = verdict.unwrap_or(false);
    let measures = families::measures(rules, &game, &scene, rel);
    Ok(Row { family: scene.fam.name().to_owned(), era: lab::era_names(rules).get(scene.era as usize).cloned().unwrap_or_default(), instance: scene.instance, attempt: scene.attempt, player: player.to_owned(), win, rounds: rel, measures, info: scene.info.clone(), trace: rows_trace })
}
