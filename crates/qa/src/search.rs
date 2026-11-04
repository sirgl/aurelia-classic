//! The search teacher (expert iteration) of the training grounds
//! (classic/docs/ai-search-teacher.md): `--learner-mind search[:k=…,…]`.
//!
//! At each learner turn of a battle ground the search decides the scene's
//! units one decision at a time, in the driver's order (by id). For the
//! front unit it lists the net's own candidates (`encode::unit_candidates`:
//! moves, attacks, marches, the rest, waiting) and the defer choice
//! (format 9: the unit goes to the end of the queue, once a turn), keeps a
//! few by the rules' previews ([`prune`]), and plays each kept candidate
//! out in copies of the game ([`rollout`]): the candidate, the rest of the
//! turn by the fast policy (the greedy one-blow-ahead [`greedy`] over the
//! units still in the queue, the scene's strikes, then the base mind), the
//! next `depth` round ends by the base mind for the learner and the scripted
//! expert (`SCRIPTED_EXPERT`, the standard before the strategy net) for everyone else; scored by the ground's own reward on the copy
//! ([`score`]). The best is played in the real game, then the next
//! decision. Combat damage is drawn from the game's combat stream, so each
//! rollout reseeds the copy's stream (`rollouts` seeds, the same for every
//! candidate of a decision, averaged): the search never sees the real
//! game's dice. Every choice is a function of the game: the same scene
//! plays the same commands.
//!
//! The commands go to the log on the learner's seat like any scripted
//! learner's (`BattleScene::strike`), so `ai-features --minds learner`
//! exports them as the learner's labels.

use std::collections::BTreeMap;
use std::time::Instant;

use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::net_e2e::encode::{self, tag, Turn};
use classic_rules::classic::Classic;
use classic_rules::combat::damage::Roll;
use classic_rules::combat::exchange::{plan_attack, AttackKind, Defender};
use classic_rules::ids::{PlayerId, TileId, UnitHandle};
use classic_rules::{ClassicGame, Command, CompiledRuleset, GameState};
use kernel::ai::driver::AiDriver;
use kernel::ai::mind::AiMind;
use kernel::ai::seats::Seated;
use kernel::ai::StepOutcome;

use serde::{Deserialize, Serialize};

use crate::bench::modules::{BenchRules, ClassicBench};
use crate::grounds::{BattleScene, Scene};

/// The mind name's prefix (`--learner-mind search:k=6,depth=2`).
pub const NAME: &str = "search";

/// The search's budget and policies (`search:key=value,...`).
#[derive(Debug, Clone, PartialEq)]
pub struct SearchConfig {
    /// Command candidates kept a decision (attacks and moves by the
    /// previews, half each; besides: one march, fortify, wait, defer).
    pub k: usize,
    /// Round ends a rollout plays (the current round's counts); 0: to the
    /// end of the learner's own turn only (the fast policy finishes it, no
    /// mind plays, `turn_score`).
    pub depth: u32,
    /// Rollouts a candidate (combat seeds), averaged.
    pub rollouts: u32,
    /// The base mind: the learner's seat in the driver (the rest of every
    /// turn) and the rollouts' learner.
    pub base: String,
    /// Offer the defer choice (once a unit a turn).
    pub defer: bool,
    /// Decisions a unit a turn at most.
    pub per_unit: u32,
    /// A siege capture's speed decay γ in the score (`score`; 0: the
    /// reference reward, linear in the war's length).
    pub speed_decay: f64,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self { k: 6, depth: 2, rollouts: 2, base: crate::bench::modules::SCRIPTED_EXPERT.to_owned(), defer: true, per_unit: 6, speed_decay: 0.5 }
    }
}

impl SearchConfig {
    /// `search` or `search:k=6,depth=2,rollouts=2,base=siege-join,defer=0,speed-decay=0.5`;
    /// None for another mind's name.
    pub fn parse(name: &str) -> Option<Result<Self, String>> {
        let rest = name.strip_prefix(NAME)?;
        if !rest.is_empty() && !rest.starts_with(':') {
            return None;
        }
        let mut c = SearchConfig::default();
        for item in rest.trim_start_matches(':').split(',').filter(|s| !s.is_empty()) {
            let Some((key, value)) = item.split_once('=') else { return Some(Err(format!("{name}: {item} is no key=value"))) };
            let number = || value.parse::<u32>().map_err(|e| format!("{name}: {key}: {e}"));
            let set = match key {
                "k" => number().map(|n| c.k = n as usize),
                "depth" => number().map(|n| c.depth = n),
                "rollouts" => number().map(|n| c.rollouts = n.max(1)),
                "per-unit" => number().map(|n| c.per_unit = n.max(1)),
                "defer" => number().map(|n| c.defer = n != 0),
                "speed-decay" => value.parse::<f64>().ok().filter(|g| (0.0..=1.0).contains(g)).map(|g| c.speed_decay = g).ok_or(format!("{name}: speed-decay in [0, 1]")),
                "base" => match ClassicBench::mind(value) {
                    Some(_) => {
                        c.base = value.to_owned();
                        Ok(())
                    }
                    None => Err(format!("{name}: no AI variant {value}")),
                },
                _ => Err(format!("{name}: unknown key {key} (k, depth, rollouts, per-unit, defer, base, speed-decay)")),
            };
            if let Err(e) = set {
                return Some(Err(e));
            }
        }
        Some(Ok(c))
    }
}

/// What the search did in a game (`.e2.json` `search`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SearchStats {
    pub turns: u32,
    pub decisions: u64,
    pub rollouts: u64,
    pub commands: u64,
    pub seconds: f64,
    /// The defer choice taken: `[position, unit]`, the unit's persistent
    /// id, deferred before the log's command at that position (several at
    /// one position in the order taken). The exporter labels each as a unit
    /// decision whose choice is the defer candidate (format 9).
    #[serde(default)]
    pub deferrals: Vec<(u64, u64)>,
    /// The unit commands chosen while the defer choice was offered:
    /// `[position, unit]` of the command. The exporter appends the defer
    /// candidate to those decisions (not chosen), as the net's own games
    /// offer it (`Trace::defer_offered`).
    #[serde(default)]
    pub offered: Vec<(u64, u64)>,
}

/// The search of one game: its config, the minds of the seats (to build
/// the rollouts' drivers) and its counters.
#[derive(Debug)]
pub struct Search {
    pub config: SearchConfig,
    /// The seats' minds by name (the learner's: the base), the others' mind.
    seats: Vec<(PlayerId, String)>,
    others: String,
    searched: Option<u32>,
    /// The round whose start the scene has judged (`scripted_turn`).
    judged: Option<u32>,
    pub stats: SearchStats,
}

impl Search {
    pub fn new(config: SearchConfig, seats: Vec<(PlayerId, String)>, others: String) -> Self {
        Self { config, seats, others, searched: None, judged: None, stats: SearchStats::default() }
    }
}

/// A scripted learner's commands before its mind plays the turn: the
/// search's decisions (once a learner turn) and the scene's strikes
/// (`Scene::strike`); without a search the strikes alone. Returns the
/// commands made (the caller counts them in the positions).
/// `position`: the log's commands so far (the next command's position);
/// `round_start`: called at a round's start, after the scene judged the
/// round before (`Scene::after_round`). A round the scene has not judged
/// yet (the last player's turn ended, the next round begun) is not
/// searched: the scene may end with the round before.
pub fn scripted_turn(search: Option<&mut Search>, rules: &CompiledRuleset, game: &mut GameState, scene: &mut Scene, driver: &AiDriver<Classic, Seated<ClassicMind>>, position: u64, round_start: bool) -> u64 {
    let mut made = 0;
    if let (Some(search), Scene::Battle(battle)) = (search, &mut *scene) {
        let round = game.view().auth.turn().round;
        if round_start {
            search.judged = Some(round);
        }
        if game.view().auth.active_player() == battle.learner() && search.judged == Some(round) && search.searched != Some(round) && !game.is_over() && !decided(rules, game, battle) {
            search.searched = Some(round);
            let clock = Instant::now();
            made += search_turn(search, rules, game, battle, driver, position);
            search.stats.turns += 1;
            search.stats.commands += made;
            search.stats.seconds += clock.elapsed().as_secs_f64();
        }
    }
    made + scene.strike(rules, game)
}

/// The scene is decided before the learner's turn: the city taken, the
/// skirmish won, a one-turn rung past its turn, the war's length played
/// (the scene ends at this round's end; the scene's strikes run the same).
fn decided(rules: &CompiledRuleset, game: &GameState, scene: &BattleScene) -> bool {
    let Some(b) = scene.clone().finish(game, rules).battle else { return true };
    let round = game.view().auth.turn().round;
    let one_turn = scene.kind() == crate::grounds::BattleKind::Siege && b.rung.and_then(|r| crate::grounds::LADDER.get(r as usize)).is_some_and(|r| r.one_turn());
    b.captured_round.is_some() || b.won_round.is_some() || (one_turn && round > b.start_round) || round >= b.start_round + b.length
}

/// One choice of a decision.
#[derive(Debug, Clone)]
enum Choice {
    Command(Command),
    /// No command: the unit leaves the queue (the base mind may still move it).
    Wait,
    /// The unit goes to the end of the queue and decides again there.
    Defer,
}

/// A listed candidate with its prior (higher first within its group).
#[derive(Debug, Clone)]
struct Listed {
    choice: Choice,
    group: Group,
    prior: i64,
    /// Its index in the net's candidate list (`encode::unit_candidates`,
    /// the defer candidate last).
    index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Group {
    Attack,
    Move,
    March,
    Fortify,
    Other,
    Wait,
    Defer,
}

fn alive(game: &GameState, unit: UnitHandle, owner: PlayerId) -> Option<kernel::world::unit::UnitCore> {
    game.view().auth.units().get(unit).filter(|u| u.core.owner == owner).map(|u| *u.core)
}

/// A unit's reach: its range, 1 for a unit without a ranged attack.
fn reach(rules: &CompiledRuleset, kind: classic_rules::ids::UnitKindId) -> i32 {
    classic_rules::ai::combat::ranged_range(rules, kind).unwrap_or(1)
}

/// The blow an attack command strikes: how and on which tile.
fn blow_of(rules: &CompiledRuleset, game: &GameState, command: &Command) -> Option<(UnitHandle, AttackKind, TileId)> {
    let auth = game.view().auth;
    let how = |unit: UnitHandle| auth.units().get(unit).map(|u| if classic_rules::ai::combat::ranged_range(rules, u.core.kind).is_some() { AttackKind::Ranged } else { AttackKind::Melee });
    match *command {
        Command::RangedAttack { unit, target } => Some((unit, AttackKind::Ranged, target)),
        Command::AttackCity { attacker, city } => Some((attacker, how(attacker)?, auth.cities().get(city)?.tile)),
        Command::AttackUnit { attacker, defender } => Some((attacker, how(attacker)?, auth.units().get(defender)?.core.tile)),
        _ => None,
    }
}

/// A blow's value by the rules' preview (the expected roll), the greedy
/// oracle's order: a capture first, a kill (the healthiest killed), a shot
/// at a city, then the exchange (twice the damage dealt less taken less the
/// target's health left: the most worn first). A melee blow the unit is
/// expected not to survive, or a melee blow on a city that does not take
/// it, is below `GREEDY_FLOOR`.
fn blow_value(rules: &CompiledRuleset, game: &GameState, unit: UnitHandle, how: AttackKind, tile: TileId) -> Option<i64> {
    blow_value_from(rules, game, unit, None, how, tile)
}

/// `blow_value` of the blow struck from `from` after a move there
/// (`plan_attack_from`: reach, line of sight and strength from that tile).
fn blow_value_from(rules: &CompiledRuleset, game: &GameState, unit: UnitHandle, from: Option<TileId>, how: AttackKind, tile: TileId) -> Option<i64> {
    let me = game.view().auth.units().get(unit).map(|u| *u.core)?;
    let plan = match from {
        None => plan_attack(game.view(), rules, unit, tile, how).ok()?,
        Some(from) => classic_rules::combat::exchange::plan_attack_from(game.view(), rules, unit, from, tile, how).ok()?,
    };
    let dealt = i64::from(plan.cap_to_defender(plan.damage_to_defender(Roll::Expected)));
    let taken = i64::from(plan.damage_to_attacker(Roll::Expected));
    let hp = i64::from(plan.defender_hp);
    let kill = dealt >= hp;
    Some(match plan.defender {
        Defender::City(_) if how == AttackKind::Melee && kill => 5000,
        Defender::City(_) if how == AttackKind::Melee => -2000 + 2 * dealt - taken,
        Defender::City(_) => 600 + dealt,
        _ if kill => 1000 + hp,
        _ if how == AttackKind::Melee && taken >= i64::from(me.hp) => -3000,
        _ => 2 * (dealt - taken) - (hp - dealt),
    })
}

/// The greedy policy strikes a blow worth at least this.
const GREEDY_FLOOR: i64 = -1000;
/// Blows a greedy pass makes at most.
const GREEDY_BLOWS: usize = 64;

/// A move to strike from is worth it from this value up (a kill, a
/// capture, a shot at a city: `blow_value`).
const MOVE_TO_STRIKE: i64 = 600;

/// The fast policy's blows: while one is worth it, the best blow of any of
/// `units` (alive, the learner's) on the enemy's units and the scene's
/// city (`target`; a blow on another city takes nothing) by the
/// previews (`blow_value`), the order free: from where the unit stands, or
/// after a move this turn to a tile it reaches with moves left when the
/// blow from there kills, captures or shoots a city (`MOVE_TO_STRIKE`; a
/// blow from the spot wins a tie). No other move: the base mind and the
/// scene's strikes walk. Returns the commands made.
fn greedy(rules: &CompiledRuleset, game: &mut GameState, learner: PlayerId, enemy: PlayerId, target: Option<TileId>, units: &[UnitHandle]) -> u64 {
    let mut made = 0;
    for _ in 0..GREEDY_BLOWS {
        let mut best: Option<(i64, Command)> = None;
        let mut step: Option<(i64, TileId, Command)> = None;
        {
            let auth = game.view().auth;
            let dims = auth.map().dimensions;
            let foes: Vec<(UnitHandle, TileId)> = auth.indexes().units_of(enemy).iter().filter_map(|h| auth.units().get(*h).map(|u| (*h, u.core.tile))).filter(|(_, t)| auth.indexes().city_on_tile(*t).is_none()).collect();
            // The scene's city only: a shot at another city takes nothing.
            let cities: Vec<(classic_rules::ids::CityHandle, TileId)> = auth.indexes().cities_of(enemy).iter().filter_map(|c| auth.cities().get(*c).map(|city| (*c, city.tile))).filter(|(_, t)| Some(*t) == target).collect();
            for h in units {
                let Some(me) = alive(game, *h, learner) else { continue };
                if me.moves_left == 0 {
                    continue;
                }
                let ranged = classic_rules::ai::combat::ranged_range(rules, me.kind).is_some();
                let span = rules.units.get(me.kind.0 as usize).map_or(1, |d| i32::from(d.movement.max(1))) + reach(rules, me.kind);
                let near = |t: TileId| dims.distance(me.tile, t).is_some_and(|d| d <= span);
                let mut tries: Vec<Command> = Vec::new();
                for (fh, t) in foes.iter().filter(|(_, t)| near(*t)) {
                    tries.push(if ranged { Command::RangedAttack { unit: *h, target: *t } } else { Command::AttackUnit { attacker: *h, defender: *fh } });
                }
                for (c, t) in cities.iter().filter(|(_, t)| near(*t)) {
                    tries.push(if ranged { Command::RangedAttack { unit: *h, target: *t } } else { Command::AttackCity { attacker: *h, city: *c } });
                }
                for cmd in &tries {
                    let Some((u, how, tile)) = blow_of(rules, game, cmd) else { continue };
                    if game.accepts_order(rules, learner, cmd) {
                        if let Some(v) = blow_value(rules, game, u, how, tile).filter(|v| *v >= GREEDY_FLOOR && best.as_ref().is_none_or(|b| *v > b.0)) {
                            best = Some((v, cmd.clone()));
                        }
                    }
                    // The same blow after a move this turn, from a tile reached with moves left.
                    let within = if ranged { reach(rules, me.kind) } else { 1 };
                    for (from, left) in game.player_view(rules, learner).own_reachable(*h) {
                        if left == 0 || from == me.tile || dims.distance(from, tile).is_none_or(|d| d > within) || !auth.indexes().units_on(from).is_empty() {
                            continue;
                        }
                        if let Some(v) = blow_value_from(rules, game, u, Some(from), how, tile).filter(|v| *v >= MOVE_TO_STRIKE && step.as_ref().is_none_or(|s| *v > s.0)) {
                            step = Some((v, from, cmd.clone()));
                        }
                    }
                }
            }
        }
        if let Some((_, from, cmd)) = step.filter(|s| best.as_ref().is_none_or(|b| s.0 > b.0)) {
            let unit = blow_of(rules, game, &cmd).map(|(u, _, _)| u).expect("an attack");
            if game.execute(rules, Command::MoveUnit { unit, destination: from }).is_err() {
                break;
            }
            made += 1;
            best = game.accepts_order(rules, learner, &cmd).then_some((0, cmd));
            if best.is_none() {
                continue;
            }
        }
        if std::env::var("SEARCH_TRACE").is_ok_and(|v| v == "2") {
            eprintln!("  greedy blow {:?}", best.as_ref().map(|b| (b.0, format!("{:?}", b.1))));
        }
        match best {
            Some((_, cmd)) => {
                if game.execute(rules, cmd).is_err() {
                    break;
                }
                made += 1;
            }
            None => break,
        }
    }
    made
}

/// The listed candidates of `unit`'s decision: the net's list
/// (`encode::unit_candidates`) with priors, and the defer choice.
fn listed(rules: &CompiledRuleset, game: &GameState, base: &ClassicMind, learner: PlayerId, unit: UnitHandle, objective: &[TileId], defer: bool) -> Vec<Listed> {
    candidates_of(rules, game, base, learner, unit, objective, defer).map_or_else(Vec::new, |(_, list)| list)
}

/// `listed`, and the net's own candidate list it was made from (the defer
/// candidate last when `defer`): the rows a net decision chose from.
fn candidates_of(rules: &CompiledRuleset, game: &GameState, base: &ClassicMind, learner: PlayerId, unit: UnitHandle, objective: &[TileId], defer: bool) -> Option<(Vec<encode::Candidate>, Vec<Listed>)> {
    let settings = ClassicBench::settings(rules);
    let obs = game.ai_observation(rules, learner);
    let mut turn = Turn::encode(&obs, rules);
    let near = base.sense_unit(game, rules, &settings, learner, unit)?;
    let mut candidates = encode::unit_candidates(&mut turn, &near, rules);
    let reachable: BTreeMap<u32, u16> = near.view.own_reachable(unit).into_iter().map(|(t, m)| (t.0, m)).collect();
    let dims = game.view().auth.map().dimensions;
    let me = near.unit.kind;
    let want = reach(rules, me);
    let to_objective = |t: TileId| objective.iter().filter_map(|o| dims.distance(t, *o)).min().unwrap_or(32);
    let mut out = Vec::new();
    for (index, c) in candidates.iter().enumerate() {
        let Some(command) = c.command.clone() else {
            if c.meta[0] == tag::WAIT {
                out.push(Listed { choice: Choice::Wait, group: Group::Wait, prior: 0, index });
            }
            continue;
        };
        let (group, prior) = match command {
            Command::MoveUnit { destination, .. } => {
                let d = to_objective(destination);
                let left = i64::from(reachable.get(&destination.0).copied().unwrap_or(0));
                // At the reach of the objective, with moves left to strike from there.
                let strike = i64::from(d == want && left > 0) * 200;
                (Group::Move, 1000 - 100 * i64::from((d - want).abs()) + strike + left / 6)
            }
            Command::MarchTo { destination, .. } => (Group::March, -i64::from(to_objective(destination))),
            Command::Fortify { .. } => (Group::Fortify, 0),
            _ => match blow_of(rules, game, &command) {
                Some((u, how, tile)) => (Group::Attack, blow_value(rules, game, u, how, tile).unwrap_or(GREEDY_FLOOR - 1)),
                None => (Group::Other, 0),
            },
        };
        out.push(Listed { choice: Choice::Command(command), group, prior, index });
    }
    if defer {
        out.push(Listed { choice: Choice::Defer, group: Group::Defer, prior: 0, index: candidates.len() });
        candidates.push(encode::defer_candidate());
    }
    Some((candidates, out))
}

/// The candidates kept for rollouts: the attacks and the moves by their
/// previews (half of `k` each, a group short of its half leaving the rest
/// to the other), the march nearest the objective, fortify, waiting and the
/// defer choice. The order breaks ties: attacks, defer, wait, moves, the
/// march, fortify — a unit that would act as well after the others defers
/// rather than leaving its turn to the base mind (the deferral is the
/// label the net learns the order from), and a move or a fortification
/// worth no more than waiting is not made (no label of a step that
/// changes nothing).
fn prune(mut list: Vec<Listed>, k: usize) -> Vec<Listed> {
    list.sort_by_key(|l| std::cmp::Reverse(l.prior));
    let count = |g: Group| list.iter().filter(|l| l.group == g).count();
    let half = k.div_ceil(2);
    let attacks = count(Group::Attack).min(half.max(k.saturating_sub(count(Group::Move))));
    let moves = count(Group::Move).min(k.saturating_sub(attacks));
    let mut out = Vec::new();
    for (g, n) in [(Group::Attack, attacks), (Group::Defer, 1), (Group::Wait, 1), (Group::Move, moves), (Group::March, 1), (Group::Fortify, 1)] {
        out.extend(list.iter().filter(|l| l.group == g).take(n).cloned());
    }
    out
}

/// A copy's combat stream for rollout `r` of a decision: a stream of its
/// own, seeded from the real stream's state (no draw of the real game is
/// seen) and `r`; the same for every candidate (common random numbers).
fn reseed(game: &mut GameState, base: u64, r: u32) {
    let seed = mix(base ^ mix(u64::from(r) + 0x5ea2c4));
    game.world_mut().rng.combat = kernel::rng::StreamRng::new(seed);
}

/// SplitMix64's finaliser.
fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// The rollouts' world: what every rollout of a decision starts from.
struct Root<'a> {
    rules: &'a CompiledRuleset,
    search: &'a Search,
    saved: &'a kernel::persistence::ai::SavedAiStates,
    tiles: usize,
    learner: PlayerId,
    enemy: PlayerId,
    /// The scene's units of the learner.
    army: Vec<UnitHandle>,
    /// The city fought over.
    target: Option<TileId>,
    seed: u64,
    /// Waiting and fortifying end the unit's turn in the copy (its moves
    /// spent, out of the fast policy's queue: the net's unit, after either,
    /// does not strike again this turn, `Values`); only the defer choice lets
    /// it act later. False: the search's own wait, the unit left to the base
    /// mind, and a fortified unit still in the queue.
    hold_wait: bool,
}

impl Root<'_> {
    fn driver(&self) -> AiDriver<Classic, Seated<ClassicMind>> {
        let mind = |name: &str| ClassicBench::mind(name).expect("a known mind");
        let seats = self.search.seats.iter().map(|(p, n)| (*p, mind(n))).collect();
        let mut driver = AiDriver::<Classic, Seated<ClassicMind>>::with_mind(ClassicBench::settings(self.rules), Vec::new(), Seated::new(mind(&self.search.others), seats));
        driver.set_trace_capacity(0);
        driver.load(self.saved, self.tiles, self.rules).expect("the AI memory of the same game");
        driver
    }

    /// The learner's turn by the fast policy from here: the greedy blows of
    /// `units`, the scene's strikes; the base mind plays the rest in the driver.
    fn fast_turn(&self, game: &mut GameState, scene: &mut BattleScene, units: &[UnitHandle]) {
        greedy(self.rules, game, self.learner, self.enemy, self.target, units);
        scene.strike(self.rules, game);
    }

    /// One rollout of `choice` for the front unit of `queue` (rollout `r`).
    fn rollout(&self, game: &GameState, scene: &BattleScene, queue: &[UnitHandle], choice: &Choice, r: u32) -> f64 {
        let rules = self.rules;
        let mut g = game.clone();
        reseed(&mut g, self.seed, r);
        let mut sc = scene.clone();
        let mut rest: Vec<UnitHandle> = queue.to_vec();
        match choice {
            Choice::Command(cmd) => {
                if g.execute(rules, cmd.clone()).is_err() {
                    return f64::NEG_INFINITY;
                }
                // The net's fortify ends the unit's turn too (`hold_wait`): the fast policy
                // never uses it again this turn, as the net's own unit does not.
                if self.hold_wait && matches!(cmd, Command::Fortify { .. }) {
                    if let Some(core) = g.world_mut().units.get_mut_core(rest[0]) {
                        core.moves_left = 0;
                    }
                    rest.remove(0);
                }
            }
            Choice::Wait => {
                if self.hold_wait {
                    if let Some(core) = g.world_mut().units.get_mut_core(rest[0]) {
                        core.moves_left = 0;
                    }
                }
                rest.remove(0);
            }
            Choice::Defer => rest.rotate_left(1),
        }
        self.fast_turn(&mut g, &mut sc, &rest);
        if self.search.config.depth == 0 {
            // To the end of the learner's own turn: the fast policy finished it; no mind plays on.
            return turn_score(rules, &g, &sc, self.search.config.speed_decay);
        }
        let mut driver = self.driver();
        let mut rounds = 0;
        for _ in 0..100_000 {
            match driver.step(&mut g, rules) {
                StepOutcome::Worked => {}
                StepOutcome::PlayerDone(p) => {
                    sc.after_player(&g, p);
                    if g.view().auth.active_player() == self.learner && !g.is_over() {
                        let army = self.army.clone();
                        self.fast_turn(&mut g, &mut sc, &army);
                    }
                }
                StepOutcome::RoundDone => {
                    rounds += 1;
                    drop(g.take_journal());
                    let over = sc.after_round(rules, &mut g, &mut driver, 0);
                    if over || rounds >= self.search.config.depth || g.is_over() {
                        break;
                    }
                    if g.view().auth.active_player() == self.learner {
                        let army = self.army.clone();
                        self.fast_turn(&mut g, &mut sc, &army);
                    }
                }
                StepOutcome::Waiting => break,
            }
        }
        score(rules, &g, &sc, self.search.config.speed_decay)
    }
}

/// A rollout's value: the ground's reward if the battle ended now
/// (`grounds::battle_reward`: a capture with its speed, a skirmish's win
/// and exchange, the clash's potential) plus a twentieth of the clash's
/// potential Φ (the enemy's value destroyed and the learner's ready force,
/// a share of its army's value): between two outcomes the reward ties,
/// the one that keeps the force and hurts the enemy more. With
/// `speed_decay` γ > 0 a capture is worth 0.5 + 0.5 γ^(rounds after the
/// war's first), the siege ladder's reward (`train_e2.siege_capture`,
/// `--siege-speed-decay`): 1 in the first turn, 0.75 a turn later at 0.5.
pub fn score(rules: &CompiledRuleset, game: &GameState, scene: &BattleScene, speed_decay: f64) -> f64 {
    let Some(b) = scene.clone().finish(game, rules).battle else { return 0.0 };
    score_record(rules, game, scene, &b, speed_decay)
}

/// The end-of-own-turn score (`depth` 0): `score` of the battle as if it
/// were judged now, within the learner's turn (`BattleScene::record_now`: a
/// capture this turn, the target's damage now, a skirmish won now), with the
/// enemy's units killed, the damage dealt and the learner's losses as the
/// ground's reward counts them.
pub fn turn_score(rules: &CompiledRuleset, game: &GameState, scene: &BattleScene, speed_decay: f64) -> f64 {
    score_record(rules, game, scene, &scene.record_now(game, rules), speed_decay)
}

fn score_record(rules: &CompiledRuleset, game: &GameState, scene: &BattleScene, b: &crate::grounds::BattleRecord, speed_decay: f64) -> f64 {
    let reward = match b.captured_round {
        Some(round) if speed_decay > 0.0 && scene.kind() == crate::grounds::BattleKind::Siege => 0.5 + 0.5 * speed_decay.min(1.0).powi(round.saturating_sub(b.start_round) as i32),
        _ => crate::grounds::battle_reward(scene.kind(), b),
    };
    let full = f64::from(kernel::world::unit::UNIT_MAX_HP) * crate::grounds::READY_SHARE;
    let (side, units) = &scene.armies()[0];
    let ready: f64 = units.iter().filter_map(|(h, k)| alive(game, *h, *side).map(|u| f64::from(rules.units[k.0 as usize].cost) * (f64::from(u.hp) / full).min(1.0))).sum();
    reward + 0.05 * (b.enemy_army.lost_value + ready) / f64::from(b.learner_army.value.max(1))
}

fn trace_on() -> bool {
    std::env::var_os("SEARCH_TRACE").is_some_and(|v| !v.is_empty() && v != "0")
}

/// The search's decisions of the learner's turn in the real game. Returns
/// the commands made.
fn search_turn(search: &mut Search, rules: &CompiledRuleset, game: &mut GameState, scene: &mut BattleScene, driver: &AiDriver<Classic, Seated<ClassicMind>>, position: u64) -> u64 {
    let learner = scene.learner();
    let enemy = scene.armies()[1].0;
    let base = ClassicBench::mind(&search.config.base).expect("a known base mind");
    let saved = driver.save();
    let tiles = game.view().auth.map().tile_count();
    let army: Vec<UnitHandle> = {
        let mut units: Vec<(kernel::ids::EntityId, UnitHandle)> = scene.armies()[0].1.iter().filter_map(|(h, _)| alive(game, *h, learner).map(|u| (u.id, *h))).collect();
        units.sort_by_key(|(id, _)| *id);
        units.into_iter().map(|(_, h)| h).collect()
    };
    let mut queue: Vec<UnitHandle> = army.iter().copied().filter(|h| alive(game, *h, learner).is_some_and(|u| u.moves_left > 0)).collect();
    let mut deferred: Vec<UnitHandle> = Vec::new();
    let mut decided: std::collections::HashMap<UnitHandle, u32> = std::collections::HashMap::new();
    let mut made = 0;
    let config = search.config.clone();
    for _ in 0..(config.per_unit as usize + 1) * army.len().max(1) {
        // A capture may end the game (the last city of a duel): nothing is left to decide.
        if game.is_over() {
            break;
        }
        let Some(&unit) = queue.first() else { break };
        let Some(me) = alive(game, unit, learner) else {
            queue.remove(0);
            continue;
        };
        let count = decided.entry(unit).or_insert(0);
        if me.moves_left == 0 || *count >= config.per_unit {
            queue.remove(0);
            continue;
        }
        *count += 1;
        // The objective: the target city (a siege), else the enemy's units.
        let objective: Vec<TileId> = match scene.target() {
            Some(t) => vec![t],
            None => scene.armies()[1].1.iter().filter_map(|(h, _)| alive(game, *h, enemy).map(|u| u.tile)).collect(),
        };
        let can_defer = config.defer && !deferred.contains(&unit) && queue.len() > 1;
        let list = listed(rules, game, &base, learner, unit, &objective, can_defer);
        let kept = prune(list, config.k);
        let choice = if kept.iter().all(|l| matches!(l.choice, Choice::Wait)) {
            Choice::Wait
        } else {
            let root = Root { rules, search, saved: &saved, tiles, learner, enemy, army: army.clone(), target: scene.target(), seed: game.view().auth.rng().combat.state(), hold_wait: false };
            let mut best: Option<(f64, &Listed)> = None;
            let mut values = Vec::new();
            for l in &kept {
                let v = (0..config.rollouts).map(|r| root.rollout(game, scene, &queue, &l.choice, r)).sum::<f64>() / f64::from(config.rollouts);
                values.push(v);
                if best.is_none_or(|b| v > b.0 + 1e-9) {
                    best = Some((v, l));
                }
            }
            search.stats.decisions += 1;
            search.stats.rollouts += (kept.len() as u64) * u64::from(config.rollouts);
            if trace_on() {
                let shown: Vec<String> = kept.iter().zip(&values).map(|(l, v)| format!("{:?}{}={v:.3}", l.group, short(&l.choice)).replace("FortifyFortify", "Fortify")).collect();
                eprintln!("search r{} unit {:?} hp{} moves{}: {}", game.view().auth.turn().round, unit, me.hp, me.moves_left, shown.join(" "));
            }
            best.map_or(Choice::Wait, |(_, l)| l.choice.clone())
        };
        let mark = (position + made, me.id.0);
        match choice {
            Choice::Command(cmd) => {
                if game.execute(rules, cmd).is_ok() {
                    if can_defer {
                        search.stats.offered.push(mark);
                    }
                    made += 1;
                } else {
                    queue.remove(0);
                }
            }
            Choice::Wait => {
                queue.remove(0);
            }
            Choice::Defer => {
                search.stats.deferrals.push(mark);
                deferred.push(unit);
                queue.rotate_left(1);
            }
        }
    }
    made
}

// ---------------------------------------------------------------------------
// Solvable ladder scenes (`ai-selfplay --ladder-solvable`)

/// The fixed combat seeds of the solvability probe: a scene is solvable when
/// the shallow search takes the city in the learner's first turn on one of
/// them (a capture that needs good dice on one seed of two stays).
pub const PROBE_SEEDS: [u64; 2] = [0x501a_b1e0, 0x501a_b1e1];

/// Whether the shallow search teacher (`search:depth=0,rollouts=1`, base
/// `standard`) takes the scene's city in the learner's first turn, played
/// on a copy of `game` (the scene just set) with its combat stream seeded by
/// `seed`: the drivers start as the game's own does (fresh AI memory, the
/// scene's aims), the rounds and players in order until the learner's first
/// turn ends. `seats`: every seat's mind by name (the learner's: `standard`),
/// `others` the driver's mind of the rest.
pub fn first_turn_capture(rules: &CompiledRuleset, game: &GameState, scene: &BattleScene, seats: Vec<(PlayerId, String)>, others: &str, seed: u64) -> bool {
    let learner = scene.learner();
    let mut g = game.clone();
    g.world_mut().rng.combat = kernel::rng::StreamRng::new(seed);
    let mut sc = Scene::Battle(Box::new(scene.clone()));
    let mind = |name: &str| ClassicBench::mind(name).expect("a known mind");
    let seated = seats.iter().map(|(p, n)| (*p, mind(n))).collect();
    let mut driver = AiDriver::<Classic, Seated<ClassicMind>>::with_mind(ClassicBench::settings(rules), Vec::new(), Seated::new(mind(others), seated));
    driver.set_trace_capacity(0);
    sc.aim(&mut driver, &g);
    let config = SearchConfig { depth: 0, rollouts: 1, ..SearchConfig::default() };
    let mut search = Search::new(config, seats, others.to_owned());
    let taken = |g: &GameState| {
        let auth = g.view().auth;
        scene.target().and_then(|t| auth.indexes().city_on_tile(t)).and_then(|c| auth.cities().get(c)).is_some_and(|c| c.owner == learner)
    };
    for _ in 0..3 {
        scripted_turn(Some(&mut search), rules, &mut g, &mut sc, &driver, 0, true);
        for _ in 0..200_000 {
            match driver.step(&mut g, rules) {
                StepOutcome::Worked => {}
                StepOutcome::PlayerDone(p) => {
                    if p == learner || g.is_over() {
                        return taken(&g);
                    }
                    sc.after_player(&g, rules, p);
                    scripted_turn(Some(&mut search), rules, &mut g, &mut sc, &driver, 0, false);
                }
                StepOutcome::RoundDone | StepOutcome::Waiting => break,
            }
        }
        if g.is_over() || sc.after_round(rules, &mut g, &mut driver, 0) {
            return taken(&g);
        }
    }
    taken(&g)
}

// ---------------------------------------------------------------------------
// Per-decision values of the net's own games (`ai-selfplay --decision-values`)

/// `--decision-values k,depth,rollouts[,share[,later]]`: the search's
/// budget for valuing the net's unit decisions in its own battles, and the
/// share of the decisions valued (drawn per decision): `share` of the
/// learner's first turn of the scene (the siege ladder's first-turn rule
/// judges it), `later` of the turns after it (default `share`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValuesConfig {
    /// Candidates kept by the previews (`prune`); the net's choice is valued too.
    pub k: usize,
    /// Round ends a rollout plays (0: the end of the learner's own turn).
    pub depth: u32,
    /// Rollouts a candidate (combat seeds, the same for every candidate).
    pub rollouts: u32,
    /// The share of the first turn's decisions valued (1: all).
    pub share: f64,
    /// The share of the later turns' decisions valued.
    pub later: f64,
}

impl ValuesConfig {
    pub fn parse(text: &str) -> Result<Self, String> {
        let parts: Vec<&str> = text.split(',').map(str::trim).collect();
        if !(3..=5).contains(&parts.len()) {
            return Err(format!("--decision-values {text}: k,depth,rollouts[,share[,later]]"));
        }
        let number = |s: &str, name: &str| s.parse::<u32>().ok().filter(|n| *n > 0).ok_or(format!("--decision-values: {name} {s} is no count > 0"));
        let fraction = |at: usize, name: &str, default: f64| match parts.get(at) {
            Some(s) => s.parse::<f64>().ok().filter(|p| (0.0..=1.0).contains(p)).ok_or(format!("--decision-values: {name} {s} is not in [0, 1]")),
            None => Ok(default),
        };
        let share = fraction(3, "share", 1.0)?;
        let later = fraction(4, "later", share)?;
        let depth = parts[1].parse::<u32>().map_err(|_| format!("--decision-values: depth {} is no count (0: the end of the learner's own turn)", parts[1]))?;
        Ok(Self { k: number(parts[0], "k")? as usize, depth, rollouts: number(parts[2], "rollouts")?, share, later })
    }
}

/// One valued decision of the net (`.e2.json` `decision_values.values`):
/// its key in the trace (the learner, the round of its turn, the ordinal in
/// the turn's decisions), the chosen candidate's index in the net's list,
/// and the mean score of its rollouts (`score`, the ground's reward) beside
/// those of the candidates the previews keep. `q_mean` is the mean over
/// every candidate valued, the net's choice among them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionValue {
    pub player: u8,
    pub round: u32,
    pub ordinal: u32,
    pub chosen: u16,
    pub q_chosen: f32,
    pub q_mean: f32,
    pub q_best: f32,
    /// The candidates valued (indices in the net's list) and their values.
    pub candidates: Vec<u16>,
    pub q: Vec<f32>,
}

/// What the valuing did in a game.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ValuesStats {
    /// The learner's unit decisions in the scene's war (the scene's units).
    pub decisions: u64,
    pub valued: u64,
    /// Left out by the share's draw.
    pub sampled_out: u64,
    /// Not valued: the scene decided already (the city taken, a one-turn
    /// rung past its turn), a unit outside the scene's army, no choice.
    pub skipped: u64,
    /// The rebuilt candidates differ from the decision's (count or the
    /// chosen's tag, tile, parameter), or the choice failed in a copy: not
    /// valued (0 is the invariant).
    pub mismatched: u64,
    pub rollouts: u64,
    pub seconds: f64,
}

/// `.e2.json` `decision_values`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValuesRecord {
    pub config: ValuesConfig,
    pub stats: ValuesStats,
    pub values: Vec<DecisionValue>,
}

/// The valuing of one game's net decisions: the search's rollouts
/// (`Root::rollout`, its fast policy and base mind) from the world of each
/// decision, before its command, for the net's choice and the candidates
/// the previews keep. The net's wait and fortify end the unit's turn in the
/// copies (after either the net's unit does not strike again that turn);
/// its defer moves the unit to the end of the queue.
/// The seeds are played one at a time over every candidate: when the first
/// ties them all, the rest are not played (the decision changes nothing
/// the rollouts see).
#[derive(Debug)]
pub struct Values {
    search: Search,
    seed: u64,
    /// The round of the learner's first turn with a decision of the scene's units.
    first: Option<u32>,
    pub record: ValuesRecord,
}

impl Values {
    /// `seats`: every seat's mind by name (the learner's: the rollouts' base,
    /// `standard`); `others`: the driver's mind of the rest; `seed`: the
    /// share's draws.
    pub fn new(config: ValuesConfig, seats: Vec<(PlayerId, String)>, others: String, seed: u64) -> Self {
        let search = SearchConfig { k: config.k, depth: config.depth, rollouts: config.rollouts, ..SearchConfig::default() };
        Self { search: Search::new(search, seats, others), seed, first: None, record: ValuesRecord { config, stats: ValuesStats::default(), values: Vec::new() } }
    }

    /// Values the decision `record` (the net's, ordinal `ordinal` of the
    /// learner's turn in `round`) on `game` as it was made (the driver
    /// decides a unit in a step of its own, the command in the next).
    #[allow(clippy::too_many_arguments)]
    pub fn value(&mut self, rules: &CompiledRuleset, game: &GameState, scene: &BattleScene, driver: &AiDriver<Classic, Seated<ClassicMind>>, round: u32, ordinal: u32, record: &classic_rules::ai::net_e2e::sample::DecisionRecord, defer_offered: bool) {
        use classic_rules::ai::net_e2e::sample::Actor;
        let learner = scene.learner();
        let Actor::Unit { index, generation } = record.actor else { return };
        let unit = UnitHandle { index, generation };
        if !scene.armies()[0].1.iter().any(|(h, _)| *h == unit) {
            return;
        }
        let stats = &mut self.record.stats;
        stats.decisions += 1;
        let Some(chosen) = record.chosen.map(usize::from) else {
            stats.skipped += 1;
            return;
        };
        if game.is_over() || decided(rules, game, scene) || alive(game, unit, learner).is_none() {
            stats.skipped += 1;
            return;
        }
        let draw = (mix(self.seed ^ (u64::from(round) << 40) ^ (u64::from(ordinal) << 20) ^ u64::from(index)) >> 11) as f64 / (1u64 << 53) as f64;
        let first = *self.first.get_or_insert(round);
        let share = if round == first { self.record.config.share } else { self.record.config.later };
        if draw >= share {
            stats.sampled_out += 1;
            return;
        }
        let clock = Instant::now();
        let valued = self.value_unit(rules, game, scene, driver, unit, chosen, record, defer_offered);
        let stats = &mut self.record.stats;
        stats.seconds += clock.elapsed().as_secs_f64();
        match valued {
            Some((candidates, q, seeds)) => {
                stats.valued += 1;
                stats.rollouts += (candidates.len() as u64) * u64::from(seeds);
                let at = candidates.iter().position(|c| usize::from(*c) == chosen).expect("the choice is valued");
                let q_mean = q.iter().sum::<f64>() / q.len() as f64;
                let q_best = q.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                self.record.values.push(DecisionValue {
                    player: learner.0,
                    round,
                    ordinal,
                    chosen: chosen as u16,
                    q_chosen: q[at] as f32,
                    q_mean: q_mean as f32,
                    q_best: q_best as f32,
                    candidates,
                    q: q.iter().map(|v| *v as f32).collect(),
                });
            }
            None => stats.mismatched += 1,
        }
    }

    /// The candidates valued (the previews' kept and the net's choice, in
    /// the net's order), their values and the seeds played; None when the
    /// rebuilt list does not fit the decision or the choice fails in a copy.
    #[allow(clippy::too_many_arguments)]
    fn value_unit(&self, rules: &CompiledRuleset, game: &GameState, scene: &BattleScene, driver: &AiDriver<Classic, Seated<ClassicMind>>, unit: UnitHandle, chosen: usize, record: &classic_rules::ai::net_e2e::sample::DecisionRecord, defer_offered: bool) -> Option<(Vec<u16>, Vec<f64>, u32)> {
        let search = &self.search;
        let learner = scene.learner();
        let enemy = scene.armies()[1].0;
        let base = ClassicBench::mind(&search.config.base).expect("a known base mind");
        let objective: Vec<TileId> = match scene.target() {
            Some(t) => vec![t],
            None => scene.armies()[1].1.iter().filter_map(|(h, _)| alive(game, *h, enemy).map(|u| u.tile)).collect(),
        };
        let (candidates, list) = candidates_of(rules, game, &base, learner, unit, &objective, defer_offered)?;
        let meta = candidates.get(chosen).map(|c| [c.meta[0], c.meta[1], c.meta[3]]);
        if candidates.len() != usize::from(record.candidates) || meta != Some(record.chosen_meta) {
            return None;
        }
        let mine = list.iter().find(|l| l.index == chosen)?.clone();
        let mut valued = prune(list, search.config.k);
        if !valued.iter().any(|l| l.index == chosen) {
            valued.push(mine);
        }
        valued.sort_by_key(|l| l.index);
        // The scene's units in the driver's order (by id), the deciding unit first.
        let army: Vec<UnitHandle> = {
            let mut units: Vec<(kernel::ids::EntityId, UnitHandle)> = scene.armies()[0].1.iter().filter_map(|(h, _)| alive(game, *h, learner).map(|u| (u.id, *h))).collect();
            units.sort_by_key(|(id, _)| *id);
            units.into_iter().map(|(_, h)| h).collect()
        };
        let mut queue: Vec<UnitHandle> = vec![unit];
        queue.extend(army.iter().copied().filter(|h| *h != unit && alive(game, *h, learner).is_some_and(|u| u.moves_left > 0)));
        let saved = driver.save();
        let root = Root { rules, search, saved: &saved, tiles: game.view().auth.map().tile_count(), learner, enemy, army, target: scene.target(), seed: game.view().auth.rng().combat.state(), hold_wait: true };
        // Seed by seed over every candidate (common random numbers); when the first seed ties them all
        // (a decision that changes nothing the rollouts see: most of a won scene's), the other seeds are
        // not played.
        let mut sums = vec![0.0f64; valued.len()];
        let mut played = 0u32;
        for r in 0..search.config.rollouts {
            for (l, sum) in valued.iter().zip(sums.iter_mut()) {
                if sum.is_finite() {
                    *sum += root.rollout(game, scene, &queue, &l.choice, r);
                }
            }
            played += 1;
            let finite: Vec<f64> = sums.iter().copied().filter(|v| v.is_finite()).collect();
            if r == 0 && finite.iter().copied().fold(f64::NEG_INFINITY, f64::max) - finite.iter().copied().fold(f64::INFINITY, f64::min) <= 1e-9 {
                break;
            }
        }
        let (mut indices, mut q) = (Vec::with_capacity(valued.len()), Vec::with_capacity(valued.len()));
        for (l, sum) in valued.iter().zip(&sums) {
            // A command the rules refuse in the copy: not a candidate (the net's own choice: no value).
            if !sum.is_finite() {
                if l.index == chosen {
                    return None;
                }
                continue;
            }
            indices.push(l.index as u16);
            q.push(sum / f64::from(played));
        }
        Some((indices, q, played))
    }
}

fn short(choice: &Choice) -> String {
    match choice {
        Choice::Command(Command::MoveUnit { destination, .. }) => format!("→{}", destination.0),
        Choice::Command(Command::MarchTo { destination, .. }) => format!("⇒{}", destination.0),
        Choice::Command(Command::RangedAttack { target, .. }) => format!("*{}", target.0),
        Choice::Command(Command::AttackCity { .. }) => "!city".to_owned(),
        Choice::Command(Command::AttackUnit { .. }) => "!unit".to_owned(),
        Choice::Command(c) => format!("{c:?}").split([' ', '{']).next().unwrap_or("").to_owned(),
        Choice::Wait | Choice::Defer => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_mind_name() {
        assert_eq!(SearchConfig::parse("standard"), None);
        assert_eq!(SearchConfig::parse("searchlight"), None);
        assert_eq!(SearchConfig::parse("search"), Some(Ok(SearchConfig::default())));
        let c = SearchConfig::parse("search:k=4,depth=3,rollouts=1,base=siege-join,defer=0").unwrap().unwrap();
        let v = ValuesConfig::parse("4,1,2").unwrap();
        assert_eq!((v.k, v.depth, v.rollouts, v.share, v.later), (4, 1, 2, 1.0, 1.0));
        let v = ValuesConfig::parse("4, 1, 2, 0.25").unwrap();
        assert_eq!((v.share, v.later), (0.25, 0.25), "later: the first turn's share by default");
        assert_eq!(ValuesConfig::parse("6,2,2,0.5,0.05").unwrap().later, 0.05);
        assert_eq!(ValuesConfig::parse("4,0,1,1,0").unwrap().depth, 0, "the end of the own turn");
        assert_eq!(SearchConfig::parse("search:depth=0,rollouts=1").unwrap().unwrap().depth, 0);
        for bad in ["4,1", "0,1,2", "4,1,2,1.5", "4,1,2,0.5,x", "4,1,2,1,1,1", "4,x,1"] {
            assert!(ValuesConfig::parse(bad).is_err(), "{bad}");
        }
        assert_eq!((c.k, c.depth, c.rollouts, c.base.as_str(), c.defer), (4, 3, 1, "siege-join", false));
        assert!(SearchConfig::parse("search:x=1").unwrap().is_err());
        assert!(SearchConfig::parse("search:base=nobody").unwrap().is_err());
    }

    #[test]
    fn prune_keeps_both_groups_and_the_choices_without_a_command() {
        let l = |group: Group, prior: i64| Listed { choice: if group == Group::Wait { Choice::Wait } else if group == Group::Defer { Choice::Defer } else { Choice::Command(Command::EndTurn { player: PlayerId(0) }) }, group, prior, index: 0 };
        let list = vec![l(Group::Move, 5), l(Group::Move, 9), l(Group::Move, 1), l(Group::Move, 7), l(Group::Attack, 3), l(Group::March, 0), l(Group::Wait, 0), l(Group::Defer, 0), l(Group::Other, 100)];
        let kept = prune(list, 4);
        let groups: Vec<(Group, i64)> = kept.iter().map(|l| (l.group, l.prior)).collect();
        // One attack (all there is), defer, wait, three moves (the rest of k, best first), the march.
        assert_eq!(groups, vec![(Group::Attack, 3), (Group::Defer, 0), (Group::Wait, 0), (Group::Move, 9), (Group::Move, 7), (Group::Move, 5), (Group::March, 0)]);
    }
}
