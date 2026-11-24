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
pub mod history_families;
pub mod lab;
pub mod memory;
pub mod scene_rule;
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
    /// The round the learner's losses first passed the family's loss cap (the assault families).
    pub cap_broken: Option<u32>,
    pub founded: Option<u32>,
    pub crossed: bool,
    pub city_low: bool,
    pub hold_lost: Option<u32>,
    pub first_kill: Option<u32>,
    pub kills_by: BTreeMap<u32, u32>,
    /// What the assault families keep between rounds (damage, resting, promotions seen).
    pub track: assault::Track,
    /// The second army's track (twin-siege: two armies at two cities).
    pub track2: assault::Track,
    /// The "at any cost" assault families: the last round a capture wins (None: the round limit) and the enemy's
    /// reinforcements that land in the round after it (kinds; spawned once, then scene pieces).
    pub deadline: Option<u32>,
    pub reinforce: Vec<UnitKindId>,
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
    /// The DAgger probe's guard (`army_net::seat::probe_and_play`): a reference plays, probed each turn.
    Probe(String),
}

impl Seat {
    pub fn parse(name: &str) -> Seat {
        match name {
            "passive" => Seat::Passive,
            "random" => Seat::Random,
            "army-net" => Seat::Net,
            n if n.starts_with("probe:") => Seat::Probe(n["probe:".len()..].to_owned()),
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
    if scene_rule::on() {
        // The scene rule's seats: the enemy's, and the learner's unless an AI variant plays it.
        let mut ruled = vec![scene.enemy];
        if !matches!(seat, Seat::Mind(_)) {
            ruled.push(scene.learner);
        }
        scene_rule::set_ruled(&ruled);
    }
    let played = play_seated(rules, game, scene, player, trace, seat, settings, seated);
    scene_rule::set_ruled(&[]);
    crate::army_net::record::close(played.as_ref().ok());
    played
}

#[allow(clippy::too_many_arguments)]
fn play_seated(rules: &CompiledRuleset, game: GameState, scene: Scene, player: &str, trace: bool, seat: Seat, settings: kernel::ai::plan::AiSettings, seated: kernel::ai::seats::Seated<ClassicMind>) -> Result<Row, String> {
    type S = kernel::ai::seats::Seated<ClassicMind>;
    if scene_rule::on() {
        // The scene rule (the trainers' default; ARMY_SCENE_RULE=standard: off): the minds give no command to the ruled seats' units and cities.
        return if lab::mode::combat_only() {
            let driver = AiDriver::<Classic, scene_rule::Ruled<CombatOnly<S>>>::with_mind(settings, Vec::new(), scene_rule::Ruled(CombatOnly(seated)));
            play_on(rules, game, scene, player, trace, seat, driver)
        } else {
            let driver = AiDriver::<Classic, scene_rule::Ruled<S>>::with_mind(settings, Vec::new(), scene_rule::Ruled(seated));
            play_on(rules, game, scene, player, trace, seat, driver)
        };
    }
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

/// One army unit's row of a learner round in the unit log (`unit_log`).
struct UnitRow {
    slot: usize,
    hp: u16,
    rest_gov: bool,
    rest_end: bool,
    struck_city: bool,
    struck_any: bool,
    dist_start: i32,
    dist_end: i32,
}

/// The loss log (`loss_log`, info only): for each army unit that died in a round of an assault play, its slot, what struck it in the enemy's
/// turn (the causes in order, the last one killed it: "city", the enemy unit's kind, "script" for the scripted field force, "other"; "own"
/// for a unit that died in our own turn, by its own attack) and its tile at the start and at the end of our turn (-1: died in it).
fn log_losses(army: &[EntityId], round: u32, pre: &BTreeMap<EntityId, (TileId, u16)>, post: Option<&BTreeMap<EntityId, (TileId, u16)>>, next: &BTreeMap<EntityId, (TileId, u16)>, killers: &BTreeMap<EntityId, Vec<String>>, out: &mut Vec<String>) {
    for (id, (t0, _)) in pre {
        let Some(slot) = army.iter().position(|a| a == id) else { continue };
        let line = |cause: String, end: i64| format!("{round},{slot},{cause},{},{end}", t0.0);
        match post.and_then(|p| p.get(id)) {
            None if post.is_some() || !next.contains_key(id) => out.push(line("own".into(), -1)),
            Some((t1, _)) if !next.contains_key(id) => out.push(line(killers.get(id).map_or_else(|| "other".to_owned(), |c| c.join("+")), i64::from(t1.0))),
            _ => {}
        }
    }
}

/// `AiDriver::step` for the enemy's side in an assault play with the damage to our army attributed (info only): the same calls as
/// `AiDriver::step`; around a command that strikes (a unit's attack or the city's strike) our army's hp is read before and after.
fn watched_step<P: kernel::ai::mind::AiMind<Classic>>(driver: &mut AiDriver<Classic, P>, game: &mut GameState, rules: &CompiledRuleset, scene: &Scene, killers: &mut BTreeMap<EntityId, Vec<String>>) -> kernel::ai::StepOutcome {
    use classic_rules::command::Command as C;
    use kernel::ai::{Advance, StepOutcome};
    driver.configure_game(game);
    match driver.advance(game, rules) {
        Advance::Worked => StepOutcome::Worked,
        Advance::RoundDone => StepOutcome::RoundDone,
        Advance::Waiting => StepOutcome::Waiting,
        Advance::Command(command) | Advance::EndTurn(command) => {
            let name_of = |game: &GameState, h: UnitHandle| game.view().auth.units().get(h).and_then(|u| rules.units.get(u.core.kind.0 as usize)).map_or_else(|| "?".to_owned(), |d| d.name.clone());
            let cause = match &command {
                C::CityStrike { .. } => Some("city".to_owned()),
                C::AttackUnit { attacker, .. } => Some(name_of(game, *attacker)),
                C::RangedAttack { unit, .. } => Some(name_of(game, *unit)),
                _ => None,
            };
            let before: Option<BTreeMap<EntityId, u16>> = cause.as_ref().map(|_| scene.mine_alive(game).iter().map(|(p, c)| (p.id, c.hp)).collect());
            let result = game.execute(rules, command);
            if let (Some(cause), Some(before)) = (cause, before) {
                let after: BTreeMap<EntityId, u16> = scene.mine_alive(game).iter().map(|(p, c)| (p.id, c.hp)).collect();
                for (id, hp) in before {
                    if after.get(&id).is_none_or(|h| *h < hp) {
                        killers.entry(id).or_default().push(cause.clone());
                    }
                }
            }
            driver.executed(result)
        }
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
        if matches!(seat, Seat::Net | Seat::Probe(_)) {
            crate::army_net::record::mute();
        }
    }
    crate::army_net::seat::reset_expert();
    crate::army_net::observe::reset_history();
    crate::army_net::observe::set_history_orders(&orders);
    if seat == Seat::Net {
        crate::army_net::obsdump::start(rules, &game, &scene, &orders);
    }
    let mut net_time = crate::army_net::seat::Timing::default();
    // The mind's time on the learner's turns (the AI's own turn time, for the army net's budget).
    let mut learner_us = 0u64;
    let mut rows_trace = Vec::new();
    if trace {
        rows_trace.push(families::line(rules, &game, &scene, 0));
    }
    let rnd = Draw::new(lab::mix(0xa11d ^ (scene.instance as u64) << 8 ^ u64::from(scene.attempt) << 40 ^ (scene.fam as u64) << 48 ^ u64::from(scene.era) << 56));
    let mut verdict: Option<bool> = None;
    // The round log of an assault play (`round_log`): per learner round its stage label (the staged teacher's own
    // state; the net's plan readout after its turn end), the army units that ended our turn nearer the city than they
    // began it, the army alive, and our units hit within the city's strike range in the enemy turn that followed.
    let logged = assault::is_assault(scene.fam);
    let city_t = scene.target.unwrap_or(scene.centre);
    let snap = |game: &GameState, scene: &Scene| -> BTreeMap<EntityId, (TileId, u16)> { scene.mine_alive(game).iter().map(|(p, c)| (p.id, (c.tile, c.hp))).collect() };
    let mut rlog: BTreeMap<u32, (String, usize, usize, usize)> = BTreeMap::new();
    let mut r_pre: Option<(u32, BTreeMap<EntityId, (TileId, u16)>)> = None;
    let mut r_post: Option<(u32, BTreeMap<EntityId, (TileId, u16)>)> = None;
    // The unit log (`unit_log`, info only): per learner round and army unit (slot, hp at the turn's start, resting by the seat's own
    // state in force for the turn's decisions and at its end, struck a city, struck anything, distance to the city at the turn's start and end).
    let mut ulog: BTreeMap<u32, BTreeMap<usize, UnitRow>> = BTreeMap::new();
    let mut rest_prev: BTreeSet<usize> = BTreeSet::new();
    let slot_of = |id: &EntityId| orders.army.iter().position(|a| a == id);
    let stage_label = |s: usize, e: bool| format!("{}{}", crate::army_net::model::STAGE_NAMES.get(s).copied().unwrap_or("?"), if e { "+e" } else { "" });
    // The loss log and the position log (info only): who struck which army unit in the enemy's turn, the army's tiles at the end of our turn.
    let mut killers: BTreeMap<EntityId, Vec<String>> = BTreeMap::new();
    let mut losses: Vec<String> = Vec::new();
    let mut poslog: BTreeMap<u32, Vec<(usize, u32)>> = BTreeMap::new();
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
                if scene_rule::on() {
                    // The scene rule closes the arena at each seat's turn start too: a unit a home city finished at the
                    // turn's start is gone before the seat looks (the net saw it as a far unit's token).
                    close_arena(rules, &mut game, &scene);
                }
                // The memory's persistence guard: save and load here (the game, and the net's memory beside it).
                if let Some(at) = crate::army_net::seat::save_at().filter(|a| now.saturating_sub(round0) == a.round && matches!(seat, Seat::Net | Seat::Script(_))) {
                    game = crate::army_net::seat::save_and_load(rules, &game, &orders, &mut scene, &at)?;
                }
                if logged {
                    let pre = snap(&game, &scene);
                    if let Some((r, post)) = r_post.take() {
                        let hit = post.iter().filter(|(id, (t, hp))| lab::dist(&game, *t, city_t) <= classic_rules::combat::city::CITY_ATTACK_RANGE && pre.get(*id).is_none_or(|x| x.1 < *hp)).count();
                        rlog.entry(r).or_default().3 = hit;
                        if let Some((_, pre_prev)) = r_pre.as_ref() {
                            log_losses(&orders.army, r, pre_prev, Some(&post), &pre, &killers, &mut losses);
                        }
                        killers.clear();
                    }
                    if let (Seat::Net, Some((r, _))) = (&seat, r_pre.as_ref()) {
                        if let Some((s, e)) = crate::army_net::seat::take_stage() {
                            rlog.entry(*r).or_default().0 = stage_label(s, e);
                        }
                        if let Some(rest) = crate::army_net::seat::take_rest() {
                            rest_prev = rest.iter().enumerate().filter(|(_, b)| **b).map(|(s, _)| s).collect();
                            for row in ulog.entry(*r).or_default().values_mut() {
                                row.rest_end = row.slot < rest.len() && rest[row.slot];
                            }
                        }
                    }
                    let rnow = now.saturating_sub(round0);
                    // The net decides with the resting set it wrote at the last turn's end; a staged teacher's is set below, after its turn.
                    let gov = |slot: usize| matches!(seat, Seat::Net) && rest_prev.contains(&slot);
                    let rows = ulog.entry(rnow).or_default();
                    for (id, (tile, hp)) in &pre {
                        if let Some(slot) = slot_of(id) {
                            rows.insert(slot, UnitRow { slot, hp: *hp, rest_gov: gov(slot), rest_end: false, struck_city: false, struck_any: false, dist_start: lab::dist(&game, *tile, city_t), dist_end: -1 });
                        }
                    }
                    r_pre = Some((rnow, pre));
                }
                match &seat {
                    Seat::Mind(_) => {}
                    Seat::Script(name) => {
                        families::reference_turn(rules, &mut game, &mut scene, name);
                        if logged && assault::flags(name).is_some() {
                            let ts = crate::army_net::seat::teacher_state(&game, &scene, &orders, &scene.track);
                            rlog.entry(now.saturating_sub(round0)).or_default().0 = stage_label(ts[2].max(0) as usize, ts[0] > 0);
                            for row in ulog.entry(now.saturating_sub(round0)).or_default().values_mut() {
                                let rest = orders.army.get(row.slot).is_some_and(|id| scene.track.resting.contains(id));
                                row.rest_gov = rest;
                                row.rest_end = rest;
                            }
                        }
                        {
                            let l = scene.learner;
                            assault::sample(rules, &game, &mut scene, l);
                        }
                        // The staged planner's own state, for the memory state of this turn's end (record v3).
                        if crate::army_net::record::active() && assault::flags(name).is_some() {
                            crate::army_net::record::set_track(crate::army_net::seat::teacher_state(&game, &scene, &orders, &scene.track));
                        }
                    }
                    Seat::Passive => {
                        act::passive_turn(&mut game, rules, scene.learner, &scene.mine);
                    }
                    Seat::Random => {
                        act::random_turn(&mut game, rules, scene.learner, &scene.mine, &rnd);
                    }
                    Seat::Probe(name) => {
                        crate::army_net::seat::probe_and_play(rules, &mut game, &mut scene, name);
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
                if scene_rule::on() && !matches!(seat, Seat::Mind(_)) {
                    scene_rule::turn(rules, &mut game, &scene, scene.learner);
                }
            }
            if active == scene.enemy && scripted_theirs != Some(now) && !game.is_over() {
                scripted_theirs = Some(now);
                if scene_rule::on() {
                    close_arena(rules, &mut game, &scene);
                }
                if logged {
                    if let Some((r, pre)) = r_pre.as_ref() {
                        let post = snap(&game, &scene);
                        let fwd = post.iter().filter(|(id, (t, _))| pre.get(*id).is_some_and(|(t0, _)| lab::dist(&game, *t, city_t) < lab::dist(&game, *t0, city_t))).count();
                        let e = rlog.entry(*r).or_default();
                        e.1 = fwd;
                        e.2 = post.len();
                        let (city_slots, any_slots) = crate::army_net::observe::struck_slots_of_round(now);
                        for row in ulog.entry(*r).or_default().values_mut() {
                            row.struck_city = city_slots.contains(&(row.slot as i16));
                            row.struck_any = any_slots.contains(&(row.slot as i16));
                            if let Some((t, _)) = orders.army.get(row.slot).and_then(|id| post.get(id)) {
                                row.dist_end = lab::dist(&game, *t, city_t);
                            }
                        }
                        poslog.insert(*r, post.iter().filter_map(|(id, (t, _))| slot_of(id).map(|s| (s, t.0))).collect());
                        r_post = Some((*r, post));
                    }
                }
                let scripted_before = logged.then(|| snap(&game, &scene));
                families::enemy_turn(rules, &mut game, &mut scene);
                if let Some(before) = scripted_before {
                    let after = snap(&game, &scene);
                    for (id, (_, hp)) in &before {
                        if after.get(id).is_none_or(|x| x.1 < *hp) {
                            killers.entry(*id).or_default().push("script".to_owned());
                        }
                    }
                }
                if scene_rule::on() {
                    // The scene rule's part of the enemy's turn (city strikes, shots, stand); the loss log names it "rule".
                    let ruled_before = logged.then(|| snap(&game, &scene));
                    scene_rule::turn(rules, &mut game, &scene, scene.enemy);
                    if let Some(before) = ruled_before {
                        let after = snap(&game, &scene);
                        for (id, (_, hp)) in &before {
                            if after.get(id).is_none_or(|x| x.1 < *hp) {
                                killers.entry(*id).or_default().push("rule".to_owned());
                            }
                        }
                    }
                }
            }
            let t_step = std::time::Instant::now();
            // The recorder and the army net's memory (its turn-end hook) see the learner's commands through `record::step`
            // (`AiDriver::step` with hooks; the same commands).
            let stepped = if active == scene.learner && (crate::army_net::record::active() || seat == Seat::Net) {
                crate::army_net::record::step(&mut driver, &mut game, rules)
            } else if crate::army_net::obsdump::events_wanted() {
                // The event dump (MLX parity) wins over the loss log's watch of the enemy's commands: both only read the step.
                crate::army_net::obsdump::step(&mut driver, &mut game, rules)
            } else if logged && active == scene.enemy {
                watched_step(&mut driver, &mut game, rules, &scene, &mut killers)
            } else {
                driver.step(&mut game, rules)
            };
            PROF[0].fetch_add(t_step.elapsed().as_micros() as u64, std::sync::atomic::Ordering::Relaxed);
            if active == scene.learner {
                learner_us += t_step.elapsed().as_micros() as u64;
            }
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
        crate::army_net::obsdump::event(rules, &game, scene.enemy, "arena", None);
        PROF[2].fetch_add(t_c.elapsed().as_micros() as u64, std::sync::atomic::Ordering::Relaxed);
        let rel = game.view().auth.turn().round.saturating_sub(round0);
        verdict = families::judge(rules, &game, &mut scene, rel);
        if trace {
            let mut l = families::line(rules, &game, &scene, rel);
            let notes = crate::army_net::seat::take_notes();
            if !notes.is_empty() {
                l.push_str(&format!(" | notes: {}", notes.join(" ; ")));
            }
            rows_trace.push(l);
        }
        if verdict.is_none() && scene.hunt {
            hunt(&mut driver, &game, &scene);
        }
    }
    let rel = game.view().auth.turn().round.saturating_sub(round0);
    let win = verdict.unwrap_or(false);
    crate::army_net::obsdump::finish(win, rel);
    if assault::is_assault(scene.fam) {
        scene.info.insert("picks".into(), scene.track.picks.join("; "));
        scene.info.insert("city_strikes".into(), crate::army_net::observe::city_strikes(round0));
        if let (Seat::Net, Some((r, _))) = (&seat, r_pre.as_ref()) {
            if let Some((s, e)) = crate::army_net::seat::take_stage() {
                rlog.entry(*r).or_default().0 = stage_label(s, e);
            }
        }
        if let (Seat::Net, Some((r, _))) = (&seat, r_pre.as_ref()) {
            if let Some(rest) = crate::army_net::seat::take_rest() {
                for row in ulog.entry(*r).or_default().values_mut() {
                    row.rest_end = row.slot < rest.len() && rest[row.slot];
                }
            }
        }
        let last = snap(&game, &scene);
        match (r_post.take(), r_pre.as_ref()) {
            (Some((r, post)), Some((_, pre))) => log_losses(&orders.army, r, pre, Some(&post), &last, &killers, &mut losses),
            (None, Some((r, pre))) => log_losses(&orders.army, *r, pre, None, &last, &killers, &mut losses),
            _ => {}
        }
        scene.info.insert("loss_log".into(), losses.join(";"));
        scene.info.insert("pos_log".into(), poslog.iter().map(|(r, v)| format!("{r}:{}", v.iter().map(|(s, t)| format!("{s},{t}")).collect::<Vec<_>>().join("|"))).collect::<Vec<_>>().join(";"));
        let unit_log = ulog
            .iter()
            .map(|(r, rows)| format!("{r}:{}", rows.values().map(|u| format!("{},{},{}{},{}{},{},{}", u.slot, u.hp, u8::from(u.rest_gov), u8::from(u.rest_end), u8::from(u.struck_city), u8::from(u.struck_any), u.dist_start, u.dist_end)).collect::<Vec<_>>().join("|")))
            .collect::<Vec<_>>()
            .join(";");
        scene.info.insert("unit_log".into(), unit_log);
        scene.info.insert("round_log".into(), rlog.iter().map(|(r, (l, f, a, h))| format!("{r}:{l}:{f}/{a}:{h}")).collect::<Vec<_>>().join(";"));
    }
    let mut measures = families::measures(rules, &game, &scene, rel);
    // Times are not measures of the play (they differ from run to run): they go to `info`.
    let mut info = scene.info.clone();
    info.insert("learner_mind_us".into(), learner_us.to_string());
    if seat == Seat::Net {
        measures.insert("net_decisions".into(), net_time.decisions as f64);
        measures.insert("net_updates".into(), net_time.updates as f64);
        measures.insert("net_turns".into(), net_time.turns as f64);
        info.insert("net_observe_us".into(), (net_time.observe_ns / 1000).to_string());
        info.insert("net_forward_us".into(), (net_time.forward_ns / 1000).to_string());
        info.insert("net_memory_us".into(), (crate::army_net::seat::memory_ns() / 1000).to_string());
        info.insert("net_encode_us".into(), (net_time.encode_ns / 1000).to_string());
        info.insert("net_update_us".into(), (net_time.update_ns / 1000).to_string());
    }
    Ok(Row { family: scene.fam.name().to_owned(), era: lab::era_names(rules).get(scene.era as usize).cloned().unwrap_or_default(), instance: scene.instance, attempt: scene.attempt, player: player.to_owned(), win, rounds: rel, measures, info, trace: rows_trace })
}
