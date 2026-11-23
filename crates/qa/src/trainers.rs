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
pub mod assault;
pub mod dump;
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
    /// What the assault families keep between rounds (damage, resting, promotions seen).
    pub track: assault::Track,
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
    /// The army net (`army_net::seat`), weights from `ARMY_NET`.
    Net,
}

impl Seat {
    pub fn parse(name: &str) -> Seat {
        match name {
            "passive" => Seat::Passive,
            "random" => Seat::Random,
            "army-net" => Seat::Net,
            n if n.starts_with("ref-") => Seat::Script(n.to_owned()),
            n => Seat::Mind(n.to_owned()),
        }
    }
}

fn mind(n: &str) -> Result<ClassicMind, String> {
    ClassicBench::mind(n).ok_or_else(|| format!("no AI variant {n}"))
}

/// The whole map, shown to both sides: in the pilot an unexplored map beyond the arena sent the AI's
/// scene units off to scout (a straight walk to the map's edge), which is no part of any task.
fn reveal(game: &mut GameState, scene: &Scene) {
    let tiles: Vec<TileId> = (0..game.view().auth.map().tile_count() as u32).map(TileId).collect();
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

/// Profile counters (microseconds): driver steps, samples, arena closing, whole plays.
pub static PROF: [std::sync::atomic::AtomicU64; 4] = [std::sync::atomic::AtomicU64::new(0), std::sync::atomic::AtomicU64::new(0), std::sync::atomic::AtomicU64::new(0), std::sync::atomic::AtomicU64::new(0)];

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
    let settings = ClassicBench::settings(rules);
    let seated = kernel::ai::seats::Seated::new(mind("standard")?, seats);
    let played = play_seated(rules, game, scene, player, trace, seat, settings, seated);
    crate::army_net::record::close(played.as_ref().ok());
    played
}

#[allow(clippy::too_many_arguments)]
fn play_seated(rules: &CompiledRuleset, game: GameState, scene: Scene, player: &str, trace: bool, seat: Seat, settings: kernel::ai::plan::AiSettings, seated: kernel::ai::seats::Seated<ClassicMind>) -> Result<Row, String> {
    if lab::mode::combat_only() {
        let driver = AiDriver::<Classic, CombatOnly<kernel::ai::seats::Seated<ClassicMind>>>::with_mind(settings, Vec::new(), CombatOnly(seated));
        play_on(rules, game, scene, player, trace, seat, driver)
    } else {
        let driver = AiDriver::<Classic, kernel::ai::seats::Seated<ClassicMind>>::with_mind(settings, Vec::new(), seated);
        play_on(rules, game, scene, player, trace, seat, driver)
    }
}

/// The combat layer only (`--ai combat`): the decisions of war and of the army stay (operations, tactics,
/// defence, the cities' strikes); research, production, city management, policies, religion, trade and
/// peace are dropped from every list the mind returns. The closed arena takes away what a city would
/// build, so what is lost is only the AI's own peace and economy.
#[derive(Debug)]
pub struct CombatOnly<P>(pub P);

impl<P: kernel::ai::mind::AiMind<Classic>> kernel::ai::mind::AiMind<Classic> for CombatOnly<P> {
    type Brief = P::Brief;
    type Sight<'g> = P::Sight<'g>;
    type Near<'g> = P::Near<'g>;

    fn sense<'g>(&self, game: &'g GameState, rules: &'g CompiledRuleset, player: PlayerId) -> Self::Sight<'g> {
        self.0.sense(game, rules, player)
    }
    fn sense_unit<'g>(&self, game: &'g GameState, rules: &'g CompiledRuleset, settings: &kernel::ai::plan::AiSettings, player: PlayerId, unit: UnitHandle) -> Option<Self::Near<'g>> {
        self.0.sense_unit(game, rules, settings, player, unit)
    }
    fn note_humans(&mut self, humans: &[PlayerId]) {
        self.0.note_humans(humans);
    }
    fn start_turn(&self, sight: &Self::Sight<'_>, rules: &CompiledRuleset, player: PlayerId, state: &mut kernel::ai::state::AiState) -> kernel::ai::mind::TurnStart<Classic, Self::Brief> {
        let mut start = self.0.start_turn(sight, rules, player, state);
        start.opening.retain(combat_decision);
        start
    }
    fn order_units(&self, brief: &Self::Brief, rules: &CompiledRuleset, order: &mut Vec<(classic_rules::ids::EntityId, UnitHandle)>) {
        self.0.order_units(brief, rules, order);
    }
    fn picks_next(&self, brief: &Self::Brief) -> bool {
        self.0.picks_next(brief)
    }
    fn next_unit(&self, near: &Self::Near<'_>, brief: &Self::Brief, state: &kernel::ai::state::AiState, remaining: &[(classic_rules::ids::EntityId, UnitHandle)]) -> Option<classic_rules::ids::EntityId> {
        self.0.next_unit(near, brief, state, remaining)
    }
    fn plan_strategic(&self, brief: &Self::Brief, state: &mut kernel::ai::state::AiState, rules: &CompiledRuleset) -> Vec<kernel::ai::plan::Decision<Classic>> {
        let mut out = self.0.plan_strategic(brief, state, rules);
        out.retain(combat_decision);
        out
    }
    fn decide_unit(&self, near: &Self::Near<'_>, player: PlayerId, unit: UnitHandle, brief: &Self::Brief, state: &mut kernel::ai::state::AiState, tools: &mut kernel::ai::plan::PlanTools<'_, Classic>) -> Option<kernel::ai::plan::Decision<Classic>> {
        self.0.decide_unit(near, player, unit, brief, state, tools)
    }
    fn permits(&self, game: &GameState, rules: &CompiledRuleset, player: PlayerId, command: &classic_rules::command::Command) -> bool {
        self.0.permits(game, rules, player, command)
    }
    fn plan_end_of_turn(&self, sight: &Self::Sight<'_>, player: PlayerId, brief: &Self::Brief, state: &kernel::ai::state::AiState, tools: &mut kernel::ai::plan::PlanTools<'_, Classic>) -> Vec<kernel::ai::plan::Decision<Classic>> {
        let mut out = self.0.plan_end_of_turn(sight, player, brief, state, tools);
        out.retain(combat_decision);
        out
    }
}

/// A decision of the combat layer: war and the army's commands; not research, production, the cities'
/// management, policies, religion, trade or peace (a decision with no command is a refusal and stays).
fn combat_decision(d: &kernel::ai::plan::Decision<Classic>) -> bool {
    use classic_rules::command::Command as C;
    match &d.command {
        None => true,
        Some(c) => matches!(
            c,
            C::MoveUnit { .. }
                | C::MarchTo { .. }
                | C::AttackUnit { .. }
                | C::AttackCity { .. }
                | C::Fortify { .. }
                | C::RangedAttack { .. }
                | C::CityStrike { .. }
                | C::SetUpRangedAttack { .. }
                | C::Promote { .. }
                | C::UpgradeUnit { .. }
                | C::Pillage { .. }
                | C::DeclareWar { .. }
                | C::Rebase { .. }
                | C::RebaseToCarrier { .. }
                | C::AirSweep { .. }
                | C::Intercept { .. }
                | C::BuildCitadel { .. }
                | C::EndTurn { .. }
                | C::Paradrop { .. }
        ),
    }
}

fn play_on<P: kernel::ai::mind::AiMind<Classic>>(rules: &CompiledRuleset, mut game: GameState, mut scene: Scene, player: &str, trace: bool, seat: Seat, mut driver: AiDriver<Classic, P>) -> Result<Row, String> {
    driver.set_trace_capacity(0);
    if !lab::mode::fog() {
        reveal(&mut game, &scene);
    }
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
    {
        let l = scene.learner;
        assault::sample(rules, &game, &mut scene, l);
    }
    let orders = crate::army_net::Orders::of_scene(rules, &game, &scene);
    if crate::army_net::record::wanted() {
        crate::army_net::record::start(orders.clone());
        if seat == Seat::Net {
            crate::army_net::record::mute();
        }
    }
    let mut net_time = crate::army_net::seat::Timing::default();
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
            assault::keep_war(rules, &mut game, &scene);
            if active == scene.learner && scripted_mine != Some(now) && !game.is_over() {
                scripted_mine = Some(now);
                match &seat {
                    Seat::Mind(_) => {}
                    Seat::Script(name) => {
                        families::reference_turn(rules, &mut game, &mut scene, name);
                        {
        let l = scene.learner;
        assault::sample(rules, &game, &mut scene, l);
    }
                    }
                    Seat::Passive => {
                        act::passive_turn(&mut game, rules, scene.learner, &scene.mine);
                    }
                    Seat::Random => {
                        act::random_turn(&mut game, rules, scene.learner, &scene.mine, &rnd);
                    }
                    Seat::Net => {
                        let t = crate::army_net::seat::turn(rules, &mut game, &orders, &scene)?;
                        net_time.decisions += t.decisions;
                        net_time.observe_ns += t.observe_ns;
                        net_time.forward_ns += t.forward_ns;
                        net_time.encode_ns += t.encode_ns;
                        net_time.update_ns += t.update_ns;
                        net_time.updates += t.updates;
                        net_time.turns += 1;
                    }
                }
            }
            if active == scene.enemy && scripted_theirs != Some(now) && !game.is_over() {
                scripted_theirs = Some(now);
                families::enemy_turn(rules, &mut game, &mut scene);
            }
            let t_step = std::time::Instant::now();
            let stepped = if active == scene.learner && crate::army_net::record::active() { crate::army_net::record::step(&mut driver, &mut game, rules) } else { driver.step(&mut game, rules) };
            PROF[0].fetch_add(t_step.elapsed().as_micros() as u64, std::sync::atomic::Ordering::Relaxed);
            let t_s = std::time::Instant::now();
            assault::sample(rules, &game, &mut scene, active);
            PROF[1].fetch_add(t_s.elapsed().as_micros() as u64, std::sync::atomic::Ordering::Relaxed);
            match stepped {
                kernel::ai::StepOutcome::Worked | kernel::ai::StepOutcome::PlayerDone(_) => {}
                kernel::ai::StepOutcome::RoundDone => break,
                kernel::ai::StepOutcome::Waiting if game.is_over() => break,
                kernel::ai::StepOutcome::Waiting => return Err(format!("round {} waits for a human", game.view().auth.turn().round)),
            }
        }
        let t_c = std::time::Instant::now();
        close_arena(rules, &mut game, &scene);
        PROF[2].fetch_add(t_c.elapsed().as_micros() as u64, std::sync::atomic::Ordering::Relaxed);
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
    if assault::is_assault(scene.fam) {
        scene.info.insert("picks".into(), scene.track.picks.join("; "));
    }
    let mut measures = families::measures(rules, &game, &scene, rel);
    if seat == Seat::Net {
        measures.insert("net_decisions".into(), net_time.decisions as f64);
        measures.insert("net_observe_us".into(), net_time.observe_ns as f64 / 1000.0);
        measures.insert("net_forward_us".into(), net_time.forward_ns as f64 / 1000.0);
        measures.insert("net_encode_us".into(), net_time.encode_ns as f64 / 1000.0);
        measures.insert("net_update_us".into(), net_time.update_ns as f64 / 1000.0);
        measures.insert("net_updates".into(), net_time.updates as f64);
        measures.insert("net_turns".into(), net_time.turns as f64);
    }
    Ok(Row { family: scene.fam.name().to_owned(), era: lab::era_names(rules).get(scene.era as usize).cloned().unwrap_or_default(), instance: scene.instance, attempt: scene.attempt, player: player.to_owned(), win, rounds: rel, measures, info: scene.info.clone(), trace: rows_trace })
}
