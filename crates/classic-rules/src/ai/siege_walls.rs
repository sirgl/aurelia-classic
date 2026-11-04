//! Siege walls (classic/docs/ai-siege-walls.md): a walled city taken the way a
//! player takes it — the army gathers out of the city's reach and strikes
//! together, not one unit after another.
//!
//! Behind the `siege-walls` switch (`Variant::siege_walls`) and in the books
//! (`ai::books`); `standard` never reads it. At the start of a full turn
//! (`SiegeWalls::plan_turn`) every city the player's army besieges — the
//! target of a marching operation at war, or of an assault — gets a plan:
//!
//! - the **ring**: posts just out of the city's reach (its strike range from
//!   the rules, or the garrison's range when it shoots farther), on the
//!   strongest ground, away from the tiles the enemy's field units reach;
//! - the **choice** of the turn, by expected value (`simulate`): strike now
//!   (from the ring, or again with the units in place) against waiting k
//!   rounds on the ring (the units heal, the reinforcements arrive, the city
//!   heals back). The value is the capture's (`front::city_damage_worth` of the
//!   whole city: the AI's own price of a city) times its chance by round, less
//!   the time it takes (a share of the city's value a round,
//!   `TIME_ROUNDS`), less the expected losses (each unit's worth,
//!   `battlefield::worth`, times its chance to die under the city's and the
//!   garrison's strikes and in its own assault). Every number is the rules'
//!   data: the previews of the strikes (`attack_preview_from`), the city's
//!   strike and healing, the units' healing, the wounded's reduced damage;
//! - a wounded unit takes part in the strike when the strike is worth more
//!   with it (its shot or blow brings the capture forward more than its
//!   expected loss costs), else it heals on the ring.
//!
//! On the turn of a strike every ranged unit of the group moves to its firing
//! tile and shoots the city before any melee unit acts, the strongest shot
//! first (the decision of whichever member the driver asks carries the
//! shooters' commands, as the front's does); a siege unit that must set up
//! steps in and sets up the turn before, with the others (the strike then
//! takes two turns). A melee unit assaults when its blow's expected value —
//! the city's chance to fall times its value less its own chance to die times
//! its worth — is positive, else it closes in next to the city between it and
//! the shooters (a screen). When the group cannot take the city, it goes back
//! to the ring and heals.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use crate::ai::battlefield::{worth, KindStats};
use crate::ai::operations::{army_role, estimate_target, estimated_strike};
use crate::ai::planner::{role_of, AiRole, Decision, PlanTools, TurnBrief, UnitView};
use crate::ai::planner_units::{can_stop, hold, move_along, HEAL_BELOW_HP};
use crate::ai::state::{AiState, ArmyRole, OperationPhase, SeenCity, UnitTask};
use crate::ai::trace::{Actor, Reason};
use crate::combat::city::{CITY_ATTACK_RANGE, CITY_HEAL_PER_TURN, CITY_RANGED_STRENGTH_PERCENT};
use crate::combat::damage::{city_strike_damage, Roll, RANGED_EQUAL_STRENGTH_RANDOM_DAMAGE, WOUNDED_DAMAGE_PERCENT};
use crate::command::Command;
use crate::ids::{EntityId, PlayerId, TileId, UnitHandle};
use crate::knowledge::observation::{AIObservation, ObservedCity, OwnUnit};
use crate::knowledge::public::PlayerQueries;
use crate::rules::CompiledRuleset;
use crate::world::city::CITY_BASE_HIT_POINTS;
use crate::world::unit::UNIT_MAX_HP;

/// The switches of `siege-join` (classic/docs/ai-siege-walls.md, "Why
/// `standard` fails the training grounds' siege"); `standard` carries them since the
/// ninth promotion, `pre-join` none.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SiegeJoin {
    /// A military unit at the walls of the war's objective, without a task or
    /// drafted into a forming operation, is a member (`besieged`).
    pub join: bool,
    /// A melee member of a strike that cannot reach the walls this turn steps
    /// as close as it gets, to strike the next (the model counts its blow from
    /// the next round), instead of going back to its post.
    pub close: bool,
    /// A shooter's firing tile is one its preview shoots the city from (the
    /// rules' line of sight), not any tile within its range.
    pub sight: bool,
}

impl SiegeJoin {
    pub const NONE: SiegeJoin = SiegeJoin { join: false, close: false, sight: false };
    pub const ALL: SiegeJoin = SiegeJoin { join: true, close: true, sight: true };

    pub fn any(self) -> bool {
        self.join || self.close || self.sight
    }
}

/// Members this many tiles beyond the ring make the city's siege group; the
/// others are still on their way (they keep marching, `books::press`).
pub const GROUP_MARGIN: i32 = 3;
/// Rounds a strike is valued over: a siege unit that sets up fires in the
/// second round, a city that survives a volley is struck again, a group whose
/// volleys beat the healing wears the city down over several rounds (its
/// losses counted every round).
pub const STRIKE_WINDOW: u32 = 8;
/// The longest wait on the ring that is weighed against striking now.
pub const MAX_WAIT: u32 = 8;
/// The capture loses its whole value over this many Standard rounds of
/// delay (the speed scales it): the time a capture saves.
pub const TIME_ROUNDS: u32 = 30;
/// A fortified city (its hit points above a city's base: walls) is besieged
/// by an army without a shooter that outranges its strike only when its value
/// a round beats the best other city's this many times: such a city is taken by
/// units that bombard it without being hit, or not at all.
pub const CLEAR_MARGIN: f64 = 1.5;
/// A strike whose capture chance within `SOON_ROUNDS` is at least this is
/// made now, whatever a wait promises.
pub const SURE_CAPTURE: f64 = 0.9;
/// The rounds of a strike's start that count as "now": the step in, the
/// volley, the blow.
pub const SOON_ROUNDS: u32 = 3;
/// The least chance of a capture within the window that makes the group
/// muster for it (below it the group fights as the front does).
pub const MIN_CAPTURE: f64 = 0.25;
/// Health a unit regains a round outside its own land (`systems::combat::heal`).
const FIELD_HEAL: u16 = crate::systems::combat::heal::HEALING_NEUTRAL_TERRITORY as u16;

/// The choice of a siege group's turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Choice {
    /// Not engaged; the strike waits (the force is incomplete, wounded, or
    /// waiting pays more): gather on the ring, fortify, heal.
    Muster,
    /// From the ring: every shooter to its firing tile, all in one turn.
    Strike,
    /// Engaged (the group stood within the city's reach, or struck last
    /// round): strike again with the units in place.
    Press,
    /// Engaged, but waiting pays more: back to the ring to heal.
    Withdraw,
    /// No capture in reach, now or after any wait weighed: the group fights
    /// as `standard`'s front does (no muster that only waits under the
    /// enemy's sorties).
    Front,
}

impl Choice {
    pub fn name(self) -> &'static str {
        match self {
            Choice::Muster => "muster",
            Choice::Strike => "strike",
            Choice::Press => "press",
            Choice::Withdraw => "withdraw",
            Choice::Front => "front",
        }
    }

    fn strikes(self) -> bool {
        matches!(self, Choice::Strike | Choice::Press)
    }
}

/// What the siege did, counted over the process (`tally`; the bench reads it
/// with `SIEGE_WALLS_TRACE`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tally {
    pub muster: u64,
    pub strike: u64,
    pub press: u64,
    pub withdraw: u64,
    pub front: u64,
    /// Steps of a member to bring a city out of sight into view.
    pub spots: u64,
    /// Shots at the city, and those of them made by a wounded unit.
    pub shots: u64,
    pub wounded_shots: u64,
    /// Shots at a unit that threatened a shooter.
    pub threat_shots: u64,
    pub set_ups: u64,
    /// Melee strikes on the city, those by a wounded unit, and the screens.
    pub assaults: u64,
    pub wounded_assaults: u64,
    pub screens: u64,
    /// Wounded members left out of a strike to heal.
    pub rested: u64,
}

static COUNTS: [AtomicU64; 14] = [const { AtomicU64::new(0) }; 14];

/// The names of the counts (`Tally`'s fields in `COUNTS`' order), for the trace.
const COUNT_NAMES: [&str; 14] =
    ["muster", "strike", "press", "withdraw", "shot", "wounded-shot", "threat-shot", "set-up", "assault", "wounded-assault", "screen", "rested", "front", "spot"];

fn count(at: usize) {
    COUNTS[at].fetch_add(1, Ordering::Relaxed);
    if tracing() && at >= 4 && at != 12 {
        eprintln!("siege-walls act={}", COUNT_NAMES[at]);
    }
}

/// The process's counts so far.
pub fn tally() -> Tally {
    let c = |i: usize| COUNTS[i].load(Ordering::Relaxed);
    Tally {
        muster: c(0),
        strike: c(1),
        press: c(2),
        withdraw: c(3),
        shots: c(4),
        wounded_shots: c(5),
        threat_shots: c(6),
        set_ups: c(7),
        assaults: c(8),
        wounded_assaults: c(9),
        screens: c(10),
        rested: c(11),
        front: c(12),
        spots: c(13),
    }
}

/// `SIEGE_WALLS_TRACE` is set: each choice and act is printed to stderr (the
/// bench's count of the options chosen).
fn tracing() -> bool {
    static TRACING: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *TRACING.get_or_init(|| std::env::var_os("SIEGE_WALLS_TRACE").is_some())
}

fn trace(player: PlayerId, round: u32, target: TileId, what: &str) {
    if tracing() {
        eprintln!("siege-walls p{} r{round} t{} {what}", player.0, target.0);
    }
}

// ---------------------------------------------------------------------------
// The measured arrival of a member on its way.

/// A member of the operation not in place to strike within a round — on its
/// way to the ring, or held where it fights — as the arrival model sees it
/// (`arrival`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Awaited {
    /// Turns its known path to the ring takes at its movement (the path's move
    /// points over its moves a turn); by the hex distance when no path is known.
    pub turns: u32,
    /// An enemy's military unit stands within its strike (its range, or next to
    /// a melee unit), or an enemy's city next to it: it fights where it stands.
    pub engaged: bool,
    /// It is wounded (below `HEAL_BELOW_HP`): it heals before it walks on.
    pub wounded: bool,
    /// The share of its path's tiles within the reach of an enemy's unit
    /// (its movement and range) or city (its strike).
    pub threat: f64,
}

/// The arrival model's weights on a logit-like scale (`arrival`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArrivalWeights {
    pub base: f64,
    /// Per round given beyond the path's turns.
    pub early: f64,
    /// Per round given short of the path's turns (a negative slack).
    pub late: f64,
    /// Per turn of the path (a long way is a way on which more happens).
    pub turns: f64,
    pub engaged: f64,
    pub wounded: f64,
    pub threat: f64,
}

/// Fitted on bench games (classic/docs/ai-siege-walls.md "Arrival"): 36 504
/// predictions for members not in place in 400 Quick land duels of the four
/// rushes and `siege-walls` against `standard` (seed offset 0), each against
/// the first later round the member stood in place (it strikes that round or
/// the next), if it did; checked on 36 715 of another 400 (offset 40): mean
/// predicted 0.221, observed 0.227, Brier 0.128 against 0.175 for the base
/// rate. A member short of its path's turns all but never arrives (`late`).
pub const ARRIVAL: ArrivalWeights = ArrivalWeights { base: 0.095, early: 0.223, late: 11.268, turns: -0.504, engaged: -0.903, wounded: -1.593, threat: -0.467 };

/// The slack counts up to this many rounds either way (beyond, a member is as
/// late or as early as it gets).
const SLACK_CAP: f64 = 8.0;

/// The chance that an awaited member stands with the group `by` rounds from
/// now. The link is `½ (1 + z / (1 + |z|))`: monotone like the logistic and
/// made of exact arithmetic only (the game's hashes hold on every platform).
pub fn arrival(a: &Awaited, by: u32) -> f64 {
    arrival_with(&ARRIVAL, a, by)
}

/// `arrival` with given weights (the calibration).
pub fn arrival_with(w: &ArrivalWeights, a: &Awaited, by: u32) -> f64 {
    let slack = (f64::from(by) - f64::from(a.turns)).clamp(-SLACK_CAP, SLACK_CAP);
    let z = w.base + w.early * slack.max(0.0) + w.late * slack.min(0.0) + w.turns * f64::from(a.turns) + w.engaged * f64::from(u8::from(a.engaged)) + w.wounded * f64::from(u8::from(a.wounded)) + w.threat * a.threat;
    0.5 * (1.0 + z / (1.0 + z.abs()))
}

// ---------------------------------------------------------------------------
// The value model.

/// One member of a siege group as the value model sees it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fighter {
    pub ranged: bool,
    pub hp: u16,
    /// Its worth to the AI (`battlefield::worth`), in hit points.
    pub worth: i32,
    /// Damage it deals the city at `hp`: (min, expected, max).
    pub dealt: (u16, u16, u16),
    /// Damage its melee strike on the city costs it at `hp` (0 for a shooter).
    pub taken: (u16, u16, u16),
    /// Damage one strike of the city does to it where it fights from.
    pub city_hit: (u16, u16, u16),
    /// The round (0: this one) it first strikes.
    pub ready: u32,
    /// The round it first stands within the city's reach (a siege unit sets
    /// up there a round before it fires).
    pub exposed: u32,
    /// It strikes from beyond the city's reach (an outranging shooter).
    pub covered: bool,
    /// The chance it is there to strike (1 for a member of the group; for one
    /// on its way, its measured arrival by the round it is needed).
    pub arrive: f64,
}

/// The besieged city as the value model sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CityModel {
    pub hp: u16,
    /// The most it heals back to.
    pub cap: u16,
    pub heal: u16,
    /// The capture's worth, in our hit points.
    pub value: i32,
    /// The value a round of delay costs.
    pub time_cost: i32,
    /// Strikes on our units a round besides the city's own (its ranged
    /// garrison and neighbours).
    pub extra_hits: u8,
}

/// The expected value of a plan: the capture's value by its chance and
/// round, less the expected losses.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Outcome {
    pub value: f64,
    /// The chance the city falls within the window.
    pub capture: f64,
    /// The worth expected lost.
    pub loss: f64,
    /// The chance it falls within `SOON_ROUNDS` of the strike's start.
    pub soon: f64,
    /// The rounds from the strike's start to the capture, given a capture.
    pub eta: f64,
}

impl Outcome {
    /// The value a round of war: the plan's value over the rounds to the capture
    /// (the wait before the strike included).
    pub fn rate(&self, delay: u32) -> f64 {
        self.value / (f64::from(delay) + self.eta.max(1.0))
    }
}

/// The share of a striker's damage it keeps at `hp` (`damage::wounded_factor`).
fn wounded(hp: f64) -> f64 {
    let damage = (f64::from(UNIT_MAX_HP) - hp).max(0.0);
    (100.0 - damage * WOUNDED_DAMAGE_PERCENT as f64 / 100.0).max(1.0)
}

/// The chance a strike of damage `range` (min, expected, max) does at least
/// `need`, the need itself spread by `spread` either way (a uniform guess
/// between the bounds).
fn chance_at_least(range: (f64, f64, f64), need: f64, spread: f64) -> f64 {
    let lo = range.0 - spread;
    let hi = range.2 + spread;
    if need <= lo {
        return 1.0;
    }
    if need > hi {
        return 0.0;
    }
    ((hi - need + 1.0) / (hi - lo + 1.0)).clamp(0.0, 1.0)
}

fn scaled(range: (u16, u16, u16), k: f64) -> (f64, f64, f64) {
    (f64::from(range.0) * k, f64::from(range.1) * k, f64::from(range.2) * k)
}

/// The value of a strike by `fighters` on `city` that begins `delay` rounds
/// from now, over `window` rounds: every round the ready shooters fire (the
/// city keeps 1 HP against them), the best one or two ready melee units
/// strike when the city may fall, the city (and its ranged garrison) strikes
/// the exposed unit it hurts most, the city heals.
pub fn simulate(fighters: &[Fighter], city: &CityModel, delay: u32, window: u32) -> Outcome {
    let mut hp: Vec<f64> = fighters.iter().map(|f| f64::from(f.hp)).collect();
    let mut alive: Vec<f64> = fighters.iter().map(|f| f.arrive.clamp(0.0, 1.0)).collect();
    let mut city_hp = f64::from(city.hp);
    let mut open = 1.0;
    let mut out = Outcome::default();
    let mut eta = 0.0;
    for t in 0..window {
        let ks: Vec<f64> = (0..fighters.len()).map(|i| wounded(hp[i]) / wounded(f64::from(fighters[i].hp))).collect();
        let k = |i: usize| ks[i];
        let (mut lo, mut mid, mut hi) = (0.0, 0.0, 0.0);
        for (i, f) in fighters.iter().enumerate() {
            if f.ranged && f.ready <= t {
                let d = scaled(f.dealt, k(i) * alive[i]);
                lo += d.0;
                mid += d.1;
                hi += d.2;
            }
        }
        let after = (city_hp - mid).max(1.0);
        let spread = ((city_hp - lo).max(1.0) - (city_hp - hi).max(1.0)) / 2.0;
        // The melee blows, the strongest first: a blow that may take the city, or
        // one that wears it down and leaves its striker `front::STRIKE_KEEPS_HP`.
        let mut melee: Vec<usize> = (0..fighters.len()).filter(|i| !fighters[*i].ranged && fighters[*i].ready <= t && alive[*i] > 0.05).collect();
        melee.sort_by(|a, b| scaled(fighters[*b].dealt, k(*b)).1.total_cmp(&scaled(fighters[*a].dealt, k(*a)).1).then(a.cmp(b)));
        let mut p = 0.0;
        let mut left = after;
        let mut reach = 1.0;
        for &i in &melee {
            let d = scaled(fighters[i].dealt, k(i));
            let falls = chance_at_least(d, left, spread) * alive[i];
            let taken = fighters[i].taken;
            let keeps = hp[i] - f64::from(taken.2) >= f64::from(crate::ai::front::STRIKE_KEEPS_HP);
            if falls <= 0.0 && !keeps {
                continue;
            }
            let dies = chance_at_least((f64::from(taken.0), f64::from(taken.1), f64::from(taken.2)), hp[i], 0.0);
            out.loss += open * reach * alive[i] * dies * f64::from(fighters[i].worth);
            p += reach * falls;
            reach *= 1.0 - falls;
            hp[i] = (hp[i] - f64::from(taken.1)).max(1.0);
            alive[i] *= 1.0 - dies;
            left = (left - d.1).max(1.0);
            if reach <= 1e-6 {
                break;
            }
        }
        let p = p.clamp(0.0, 1.0);
        out.capture += open * p;
        eta += open * p * f64::from(t + 1);
        if t < SOON_ROUNDS {
            out.soon += open * p;
        }
        out.value += open * p * f64::from(city.value - city.time_cost * (delay + t) as i32).max(0.0);
        open *= 1.0 - p;
        if open <= 1e-6 {
            break;
        }
        // The city's answer and its garrison's: the exposed unit it hurts most.
        for _ in 0..=city.extra_hits {
            let exposed = (0..fighters.len()).filter(|i| !fighters[*i].covered && fighters[*i].exposed <= t && alive[*i] > 0.05);
            let Some(i) = exposed.max_by(|a, b| fighters[*a].city_hit.1.cmp(&fighters[*b].city_hit.1).then(hp[*b].total_cmp(&hp[*a])).then(b.cmp(a))) else { break };
            let hit = fighters[i].city_hit;
            let dies = chance_at_least((f64::from(hit.0), f64::from(hit.1), f64::from(hit.2)), hp[i], 0.0);
            out.loss += open * alive[i] * dies * f64::from(fighters[i].worth);
            // A wound that does not kill costs its share of the unit's worth, halved:
            // it heals, at the price of the rounds out of the fight.
            let wound = f64::from(hit.1).min(hp[i]) / f64::from(UNIT_MAX_HP) / 2.0;
            out.loss += open * alive[i] * (1.0 - dies) * wound * f64::from(fighters[i].worth);
            alive[i] *= 1.0 - dies;
            hp[i] = (hp[i] - f64::from(hit.1)).max(1.0);
        }
        city_hp = (left + f64::from(city.heal)).min(f64::from(city.cap.max(city.hp)));
    }
    out.value -= out.loss;
    out.eta = if out.capture > 0.0 { eta / out.capture } else { f64::from(window) };
    out
}

/// The same fighters `wait` rounds later on the ring: healed, the ones on
/// their way there by their measured arrival (`awaited`: None for a member of
/// the group), every shooter firing the round it walks in, or the round after
/// when `step_in` (a siege unit sets up, a step into range takes a shooter's
/// moves); the city healed back.
pub fn waited(fighters: &[Fighter], step_in: &[bool], awaited: &[Option<Awaited>], city: &CityModel, wait: u32) -> (Vec<Fighter>, CityModel) {
    // Rounds counted from the strike's start, `wait` rounds from now (`simulate`'s `delay`).
    let later: Vec<Fighter> = fighters
        .iter()
        .zip(step_in)
        .zip(awaited.iter().chain(std::iter::repeat(&None)))
        .map(|((f, set), a)| {
            let ready = u32::from(*set).max(f.ready.saturating_sub(wait));
            let arrive = a.as_ref().map_or(f.arrive, |a| arrival(a, wait + ready));
            Fighter { hp: f.hp.saturating_add(FIELD_HEAL.saturating_mul(wait as u16)).min(UNIT_MAX_HP), ready, exposed: ready.saturating_sub(u32::from(*set)), arrive, ..*f }
        })
        .collect();
    let hp = (u32::from(city.hp) + wait * u32::from(city.heal)).min(u32::from(city.cap.max(city.hp))) as u16;
    (later, CityModel { hp, ..*city })
}

// ---------------------------------------------------------------------------
// The turn's plans.

/// One shooter's part in a strike: its firing tile and its steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Shot {
    unit: EntityId,
    handle: UnitHandle,
    start: TileId,
    from: TileId,
    /// It sets up before it fires (a siege unit).
    set_up: bool,
    /// It fires this turn (else it only steps in and sets up).
    fire: bool,
    wounded: bool,
    /// 0 move, 1 set up, 2 fire, 3 done.
    stage: u8,
}

/// A member's step next to a city out of sight, before the volley.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Spot {
    unit: EntityId,
    handle: UnitHandle,
    to: TileId,
    done: bool,
}

/// A siege group's plan of the turn.
#[derive(Debug, Clone)]
struct Plan {
    /// The city besieged: the operation's target, or a city on the way that
    /// is worth more a round of war (`plan_operation`).
    target: TileId,
    choice: Choice,
    /// The member that brings the city into sight before the volley.
    spot: Option<Spot>,
    /// Members of the group (the plan decides for them).
    members: Vec<EntityId>,
    /// The shots of a strike, the strongest first.
    shots: Vec<Shot>,
    /// Melee members that take part in a strike.
    assault: Vec<EntityId>,
    /// Where each member waits.
    posts: Vec<(EntityId, TileId)>,
    /// The city's reach (its strike, or its ranged garrison's).
    reach: i32,
    /// The members that screened this turn (counted once).
    screened: Vec<EntityId>,
    /// The chance of a capture this round (`simulate`'s first round).
    capture_now: f64,
    /// The chance of a capture within the strike's window.
    capture: f64,
    /// `SiegeJoin::close`.
    close: bool,
}

#[derive(Debug, Default)]
struct Memory {
    round: u32,
    plans: Vec<Plan>,
    /// The targets struck last turn.
    engaged: Vec<TileId>,
    /// The most hit points each foreign city was seen with (every city in
    /// sight, every turn): the last seen state of a city out of sight.
    max_hp: BTreeMap<TileId, u16>,
    /// The city's strength the previews last showed (walls and garrison
    /// included): the estimate out of sight.
    strength: BTreeMap<TileId, i64>,
    /// The city each operation's group besieged last turn (`plan_operation`).
    chosen: BTreeMap<TileId, TileId>,
}

/// The start-of-turn orders the siege plans take over.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stripped {
    /// Every shot and set-up of these units (their group strikes).
    pub all: Vec<UnitHandle>,
    /// Their shots at this city (their group waits on the ring).
    pub at: Vec<(UnitHandle, TileId)>,
}

impl Stripped {
    /// The order is the plans' to give.
    pub fn strips(&self, command: &Command) -> bool {
        match *command {
            Command::RangedAttack { unit, target } => self.all.contains(&unit) || self.at.contains(&(unit, target)),
            Command::SetUpRangedAttack { unit } => self.all.contains(&unit) || self.at.iter().any(|(u, _)| *u == unit),
            _ => false,
        }
    }
}

/// The siege of one mind, with each player's plans.
#[derive(Debug)]
pub struct SiegeWalls {
    /// The mind's number in the process (the trace tells the games apart).
    id: u64,
    memory: Mutex<BTreeMap<PlayerId, Memory>>,
    /// `siege-join`'s switches (`SiegeJoin`).
    fix: SiegeJoin,
}

impl Default for SiegeWalls {
    fn default() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        Self { id: NEXT.fetch_add(1, Ordering::Relaxed), memory: Mutex::new(BTreeMap::new()), fix: SiegeJoin::NONE }
    }
}

/// The operation a member besieges for: the target of its marching operation
/// at war, or of its assault, held by a player at war with us. With `join`
/// (`siege-join`) also a military unit at the walls of the city its war aims at
/// (`at_walls`) without a task or drafted into a forming operation: the
/// planners give a task only when a unit is asked, after the turn's siege
/// plans are made, so a unit that came to the walls without one fought its
/// first turn alone (its blow before the volley, its shot at a unit) or took
/// the first task free (a garrison at home); and a forming operation walks its
/// members to its muster point, away from the walls they stand at.
fn besieged(brief: &TurnBrief, state: &AiState, unit: EntityId, join: bool) -> Option<TileId> {
    match state.task_of(unit).map(|t| t.task) {
        Some(UnitTask::Operation { target }) => {
            let op = state.operation(target)?;
            if op.phase == OperationPhase::Marching {
                return brief.at_war(op.enemy).then_some(target);
            }
            // A forming operation musters at home: a member already at the walls stays.
            if join { at_walls(brief, state, unit) } else { None }
        }
        Some(UnitTask::Assault { target }) => Some(target),
        None if join => at_walls(brief, state, unit),
        _ => None,
    }
}

/// `siege-join`: the city the player's war aims at (`WarObjective`) when the
/// military `unit` stands, out of the player's cities, within the widest ring
/// and its margin of it.
fn at_walls(brief: &TurnBrief, state: &AiState, unit: EntityId) -> Option<TileId> {
    let u = brief.units.iter().find(|u| u.id == unit)?;
    if u.role != AiRole::Military || brief.cities.iter().any(|c| c.tile == u.tile) {
        return None;
    }
    let aim = state
        .relations()
        .iter()
        .filter(|m| brief.at_war(m.other))
        .find_map(|m| m.objective.filter(|o| o.kind == crate::ai::state::ObjectiveKind::City))?;
    (brief.dims.distance(u.tile, aim.tile)? <= CITY_ATTACK_RANGE + 1 + GROUP_MARGIN).then_some(aim.tile)
}

/// A shooter's range with its promotions.
fn range(rules: &CompiledRuleset, unit: &OwnUnit) -> Option<i32> {
    crate::ai::siege_tactics::unit_range(rules, unit.kind, &unit.promotions)
}

/// The kind must set up before it shoots.
fn sets_up(sight: &AIObservation<'_>, unit: &OwnUnit) -> bool {
    unit.stance == crate::world::unit::Stance::SetUp || sight.view.must_set_up_first(unit.handle)
}

/// Damage (min, expected, max) of the city's strike of `strength` (hundredths)
/// on a unit of defence `defence` (hundredths).
fn city_hit(strength: i64, defence: i64) -> (u16, u16, u16) {
    let max = RANGED_EQUAL_STRENGTH_RANDOM_DAMAGE - 1;
    (city_strike_damage(strength, defence, Roll::Drawn(0)), city_strike_damage(strength, defence, Roll::Expected), city_strike_damage(strength, defence, Roll::Drawn(max)))
}

/// The defence the rules give a city the player has never seen with its hit
/// points: the cheapest building of the city's own defence (extra hit points or
/// strength) whose technology the player knows — its estimate of the owner's
/// era, as `operations::estimate_city` takes its technologies — in a city at
/// least as large as the player's largest (a city with the production for it);
/// none in a smaller one. (extra strength in hundredths, extra hit points.)
pub fn default_defence(brief: &TurnBrief, rules: &CompiledRuleset, population: u32) -> (i64, u16) {
    use crate::rules::content::BuildingKind;
    use crate::rules::effects::{CityCombatStat, Modifier, Selector};
    let largest = brief.cities.iter().map(|c| c.population).max().unwrap_or(1);
    if population < largest {
        return (0, 0);
    }
    let mut best: Option<(u32, i64, i64)> = None;
    for def in &rules.buildings {
        if def.unbuildable || def.cost == 0 || !matches!(def.kind, BuildingKind::Regular) || !def.required_tech.is_none_or(|t| brief.knows(t)) {
            continue;
        }
        let (mut strength, mut hp) = (0i64, 0i64);
        for effect in def.effects.passive_city.iter().filter(|e| e.selector == Selector::SourceCity) {
            match effect.modifier {
                Modifier::ModifyCityCombat { stat: CityCombatStat::Defense, amount } => strength += i64::from(amount),
                Modifier::ModifyCityCombat { stat: CityCombatStat::MaxHitPoints, amount } => hp += i64::from(amount),
                _ => {}
            }
        }
        if (strength > 0 || hp > 0) && best.is_none_or(|(cost, _, _)| def.cost < cost) {
            best = Some((def.cost, strength, hp));
        }
    }
    best.map_or((0, 0), |(_, s, h)| (s, h.clamp(0, i64::from(u16::MAX)) as u16))
}

/// What the player knows of a city's defence: its hit points (current and the
/// most seen), its strength, and whether it is fortified (hit points above a
/// city's base: walls).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Defence {
    hp: u16,
    cap: u16,
    estimate: crate::ai::operations::CityEstimate,
}

/// The defence of `seen` as the player knows it: in sight, what it shows (the
/// previews' strength once one was made); out of sight, the last seen state,
/// else the rules' default for its era and size (`default_defence`).
fn defence_of(brief: &TurnBrief, rules: &CompiledRuleset, m: &Memory, seen: &SeenCity, observed: Option<ObservedCity>) -> Defence {
    let mut estimate = estimate_target(brief, rules, seen);
    let remembered = m.max_hp.get(&seen.tile).copied();
    let cap = match remembered {
        Some(hp) => hp.max(observed.map_or(0, |c| c.hp)),
        None => {
            let (strength, hp) = default_defence(brief, rules, seen.population);
            estimate.strength += strength;
            CITY_BASE_HIT_POINTS.saturating_add(hp).max(observed.map_or(0, |c| c.hp))
        }
    };
    if let Some(strength) = m.strength.get(&seen.tile) {
        estimate.strength = *strength;
    }
    Defence { hp: observed.map_or(cap, |c| c.hp), cap, estimate }
}

impl SiegeWalls {
    pub fn new() -> Self {
        Self::default()
    }

    /// The siege with `siege-join`'s switches.
    pub fn joined(self, fix: SiegeJoin) -> Self {
        Self { fix, ..self }
    }

    /// The plans of the player's turn; returns the start-of-turn shots they
    /// take over (`Stripped`: the advisor's shots of a striking group, and the
    /// piecemeal shots at a city whose group waits).
    pub fn plan_turn(&self, sight: &AIObservation<'_>, brief: &TurnBrief, state: &AiState, rules: &CompiledRuleset) -> Stripped {
        let mut memory = self.memory.lock().expect("no siege thread panicked");
        let m = memory.entry(brief.player).or_default();
        if brief.round < m.round {
            *m = Memory::default();
        }
        m.round = brief.round;
        // The last seen state of every foreign city in sight.
        for city in &sight.visible_foreign_cities {
            let hp = m.max_hp.entry(city.tile).or_insert(city.hp);
            *hp = (*hp).max(city.hp);
        }
        let mut targets: Vec<TileId> = brief.units.iter().filter_map(|u| besieged(brief, state, u.id, self.fix.join)).collect();
        targets.sort();
        targets.dedup();
        let mut plans: Vec<Plan> = Vec::new();
        for target in targets {
            if let Some(plan) = plan_operation(self.id, sight, brief, state, rules, m, target, self.fix) {
                if !plans.iter().any(|p| p.target == plan.target || p.members.iter().any(|u| plan.members.contains(u))) {
                    plans.push(plan);
                }
            }
        }
        m.engaged = plans.iter().filter(|p| p.choice.strikes()).map(|p| p.target).collect();
        let handle = |id: &EntityId| sight.own_units.iter().find(|u| u.id == *id).map(|u| u.handle);
        let mut stripped = Stripped::default();
        for plan in &plans {
            if plan.choice == Choice::Front {
                continue;
            }
            if plan.choice.strikes() {
                stripped.all.extend(plan.members.iter().filter_map(handle));
            } else {
                stripped.at.extend(plan.members.iter().filter_map(handle).map(|h| (h, plan.target)));
            }
        }
        m.plans = plans;
        stripped
    }

    /// The decision of a member of a siege group (None: not one; the
    /// marching and the front decide).
    pub fn decide(&self, view: &UnitView<'_, '_>, state: &AiState, tools: &mut PlanTools<'_>) -> Option<Decision> {
        let brief = view.brief;
        let unit = view.local.unit;
        besieged(brief, state, unit.id, self.fix.join)?;
        let mut memory = self.memory.lock().expect("no siege thread panicked");
        let m = memory.get_mut(&brief.player)?;
        if m.round != brief.round {
            return None;
        }
        let plan = m.plans.iter_mut().find(|p| p.members.contains(&unit.id))?;
        if plan.choice == Choice::Front {
            return None;
        }
        let target = plan.target;
        // The city gone (taken): the operation's end is `standard`'s.
        let city = view.local.foreign_cities.iter().find(|c| c.tile == target).copied();
        let near = view.local.view.dimensions().distance(unit.tile, target).is_some_and(|d| d <= view.local.radius);
        if city.is_none() && near && view.local.view.is_visible(target) {
            return None;
        }
        if let Some(spot) = plan.spot.as_mut().filter(|s| !s.done) {
            spot.done = true;
            let step = Command::MoveUnit { unit: spot.handle, destination: spot.to };
            if !view.local.view.is_visible(target) && view.local.view.accepts(&step) {
                count(13);
                return Some(Decision::act(Actor::Unit(spot.unit), Reason::Advance, step));
            }
        }
        // The spotter keeps the city in sight for the next turn's plan while the group
        // waits (back on the ring it would not see it: the ring lies beyond its sight).
        if !plan.choice.strikes() && plan.spot.is_some_and(|s| s.done && s.unit == unit.id) && unit.tile == plan.spot.map_or(unit.tile, |s| s.to) {
            return Some(hold(view, Reason::Rally));
        }
        if plan.choice.strikes() {
            if let Some(decision) = next_shot(view, plan) {
                return Some(decision);
            }
            return Some(own_part(view, tools, plan, city));
        }
        Some(to_post(view, tools, plan))
    }
}

/// One city weighed for an operation's group (`evaluate`).
struct Weighed {
    plan: Plan,
    /// The best option's value a round of war (`Outcome::rate`).
    rate: f64,
    /// A capture is in reach: now, or after a wait the group survives to strike.
    in_reach: bool,
    /// Hit points above a city's base (walls), and no member outranges its strike.
    fortified_unanswered: bool,
    /// Wounded members left out of a strike to heal.
    rested: usize,
    /// The trace's line.
    line: String,
}

/// The plan of the operation against `op`: its target and every city at war
/// one of its members stands near are weighed (`evaluate`), and the group
/// besieges the one worth most a round of war (`Outcome::rate`: a weak city on
/// the way before a walled capital); a fortified city without a member that
/// outranges its strike only when it beats the others by `CLEAR_MARGIN`. None:
/// no member is near any of them.
#[allow(clippy::too_many_arguments)]
fn plan_operation(mind: u64, sight: &AIObservation<'_>, brief: &TurnBrief, state: &AiState, rules: &CompiledRuleset, m: &mut Memory, op: TileId, fix: SiegeJoin) -> Option<Plan> {
    let dims = brief.dims;
    let distance = |a: TileId, b: TileId| dims.distance(a, b).unwrap_or(i32::MAX);
    let members: Vec<OwnUnit> = brief
        .units
        .iter()
        .filter(|u| besieged(brief, state, u.id, fix.join) == Some(op))
        .filter_map(|u| sight.own_units.iter().find(|o| o.id == u.id).copied())
        .filter(|u| army_role(rules, u.kind).is_some() && rules.units[u.kind.0 as usize].domain == crate::rules::content::Domain::Land)
        .collect();
    // The cities weighed: the target, and every city at war that a member stands
    // near (within the widest ring and its margin).
    let near = CITY_ATTACK_RANGE + 1 + GROUP_MARGIN + 1;
    let mut cities: Vec<TileId> = vec![op];
    let foreign = sight.visible_foreign_cities.iter().map(|c| (c.tile, c.owner)).chain(state.seen_cities().iter().map(|c| (c.tile, c.owner)));
    for (tile, owner) in foreign {
        if tile != op && brief.at_war(owner) && !cities.contains(&tile) && members.iter().any(|u| distance(u.tile, tile) <= near) {
            cities.push(tile);
        }
    }
    let weighed: Vec<Weighed> = cities.iter().filter_map(|c| evaluate(mind, sight, brief, state, rules, m, op, *c, &members, fix)).collect();
    let best_other = |i: usize| weighed.iter().enumerate().filter(|(j, w)| *j != i && w.in_reach).map(|(_, w)| w.rate).fold(0.0f64, f64::max);
    let worth = |i: usize, w: &Weighed| w.in_reach && w.rate > 0.0 && (!w.fortified_unanswered || w.rate >= CLEAR_MARGIN * best_other(i));
    let best = weighed.iter().enumerate().filter(|(i, w)| worth(*i, w)).max_by(|(i, a), (j, b)| a.rate.total_cmp(&b.rate).then(j.cmp(i))).map(|(i, _)| i);
    // The city besieged last turn is kept unless another is clearly worth more: a group
    // that turns between two cities takes neither.
    let kept = m.chosen.get(&op).and_then(|c| weighed.iter().position(|w| w.plan.target == *c)).filter(|i| worth(*i, &weighed[*i]));
    let chosen = match (kept, best) {
        (Some(k), Some(b)) if weighed[b].rate < CLEAR_MARGIN * weighed[k].rate => Some(k),
        (_, b) => b,
    };
    match chosen {
        Some(i) => m.chosen.insert(op, weighed[i].plan.target),
        None => m.chosen.remove(&op),
    };
    // No capture worth it in reach: the operation's own city, as the front fights it.
    let at = chosen.or_else(|| weighed.iter().position(|w| w.plan.target == op))?;
    let mut weighed = weighed;
    let mut pick = weighed.swap_remove(at);
    if chosen.is_none() {
        pick.plan.choice = Choice::Front;
        pick.plan.shots.clear();
        pick.plan.assault.clear();
        pick.plan.spot = None;
    }
    count(match pick.plan.choice {
        Choice::Muster => 0,
        Choice::Strike => 1,
        Choice::Press => 2,
        Choice::Withdraw => 3,
        Choice::Front => 12,
    });
    if pick.plan.choice.strikes() {
        for _ in 0..pick.rested {
            count(11);
        }
    }
    trace(brief.player, brief.round, pick.plan.target, &format!("choice={} op={} cities={} {}", pick.plan.choice.name(), op.0, cities.len(), pick.line));
    Some(pick.plan)
}

/// The features of a member's way to `target`'s ring (`Awaited`): its known
/// path's turns at its movement, the enemy next to it, its wounds, the share
/// of the path within an enemy's reach.
fn awaited(sight: &AIObservation<'_>, brief: &TurnBrief, rules: &CompiledRuleset, unit: &OwnUnit, target: TileId, ring: i32) -> Awaited {
    use crate::ai::planner_map::{find_path, turns_for, Blockers, KnownMap, MapScratch, PathOutcome, SearchStats};
    let dims = brief.dims;
    let distance = |a: TileId, b: TileId| dims.distance(a, b).unwrap_or(i32::MAX);
    let military = |k: crate::ids::UnitKindId| role_of(rules, k) == AiRole::Military;
    let foes: Vec<(TileId, i32)> = sight
        .visible_foreign_units
        .iter()
        .filter(|u| brief.at_war(u.owner) && military(u.kind))
        .map(|u| {
            let def = &rules.units[u.kind.0 as usize];
            (u.tile, i32::from(def.movement.max(1)) + i32::from(def.range.max(1)))
        })
        .collect();
    let forts: Vec<TileId> = sight.visible_foreign_cities.iter().filter(|c| c.tile != target && brief.at_war(c.owner)).map(|c| c.tile).collect();
    // Engaged: an enemy's unit within its own strike (its range, or next to a melee
    // unit), or an enemy's city next to it: it fights where it stands.
    let strike = range(rules, unit).unwrap_or(1);
    let engaged = foes.iter().any(|(t, _)| distance(*t, unit.tile) <= strike) || forts.iter().any(|t| distance(*t, unit.tile) <= 1);
    let reached = |t: TileId| foes.iter().any(|(f, r)| distance(*f, t) <= *r) || forts.iter().any(|c| distance(*c, t) <= CITY_ATTACK_RANGE);
    let movement = i32::from(rules.units[unit.kind.0 as usize].movement.max(1));
    let by_hex = |d: i32| ((d - ring).max(0) + movement - 1) / movement;
    let map = KnownMap::of_kind(&sight.view, unit.kind);
    let limit = crate::ai::planner::DecisionBudget::default().search();
    let found = find_path(&map, unit.tile, target, &Blockers::new(Vec::new()), &mut MapScratch::default(), limit, &mut SearchStats::default());
    let (turns, threat) = match found {
        PathOutcome::Found { tiles, points } if tiles.len() > 1 => {
            let at_ring = tiles.iter().position(|t| distance(*t, target) <= ring).unwrap_or(tiles.len() - 1);
            let way = &tiles[..=at_ring];
            let share = points as u64 * at_ring as u64 / (tiles.len() - 1) as u64;
            let threat = way.iter().filter(|t| reached(**t)).count() as f64 / way.len() as f64;
            (turns_for(share as u32, unit.moves_max), threat)
        }
        _ => (by_hex(distance(unit.tile, target)).max(0) as u32, f64::from(u8::from(reached(unit.tile)))),
    };
    Awaited { turns, engaged, wounded: unit.hp < HEAL_BELOW_HP, threat }
}

/// The plan of the siege of `target` by the members of the operation against
/// `op`, weighed, or None when no member is near it. A member on its way
/// counts by its measured arrival (`arrival`) when `target` is the operation's
/// own: its march goes there; one marching to another city does not come.
#[allow(clippy::too_many_arguments)]
fn evaluate(mind: u64, sight: &AIObservation<'_>, brief: &TurnBrief, state: &AiState, rules: &CompiledRuleset, m: &mut Memory, op: TileId, target: TileId, members: &[OwnUnit], fix: SiegeJoin) -> Option<Weighed> {
    let dims = brief.dims;
    let distance = |a: TileId, b: TileId| dims.distance(a, b).unwrap_or(i32::MAX);
    let observed: Option<ObservedCity> = sight.visible_foreign_cities.iter().find(|c| c.tile == target).copied();
    let seen: SeenCity = match (observed, state.seen_cities().iter().find(|c| c.tile == target)) {
        (Some(c), _) => SeenCity { tile: c.tile, owner: c.owner, population: c.population, last_seen_round: brief.round },
        (None, Some(s)) => *s,
        (None, None) => return None,
    };
    if !brief.at_war(seen.owner) {
        return None;
    }
    let at_war = |p: PlayerId| brief.at_war(p);
    let military = |k: crate::ids::UnitKindId| role_of(rules, k) == AiRole::Military;
    // The city's reach: its strike, or its ranged garrison's when longer.
    let garrison: Vec<_> = sight.visible_foreign_units.iter().filter(|u| u.tile == target && at_war(u.owner) && military(u.kind)).copied().collect();
    let garrison_range = garrison.iter().filter(|u| rules.units[u.kind.0 as usize].ranged_strength > 0).map(|u| i32::from(rules.units[u.kind.0 as usize].range)).max().unwrap_or(0);
    let reach = CITY_ATTACK_RANGE.max(garrison_range);
    let ring = reach + 1;
    // The enemy's field units and how far they strike this turn.
    let field: Vec<(TileId, i32)> = sight
        .visible_foreign_units
        .iter()
        .filter(|u| u.tile != target && at_war(u.owner) && military(u.kind) && distance(u.tile, target) <= ring + GROUP_MARGIN + 4)
        .map(|u| {
            let def = &rules.units[u.kind.0 as usize];
            (u.tile, i32::from(def.movement.max(1)) + i32::from(def.range.max(1)))
        })
        .collect();
    let extra_hits = sight
        .visible_foreign_units
        .iter()
        .filter(|u| at_war(u.owner) && military(u.kind) && rules.units[u.kind.0 as usize].ranged_strength > 0 && distance(u.tile, target) <= 1)
        .count()
        .min(3) as u8;
    let own_way = target == op;
    // A member marching to another city does not come here: only those near count.
    let members: Vec<OwnUnit> = members.iter().filter(|u| own_way || distance(u.tile, target) <= ring + GROUP_MARGIN).copied().collect();
    let group: Vec<OwnUnit> = members.iter().filter(|u| distance(u.tile, target) <= ring + GROUP_MARGIN).copied().collect();
    if group.is_empty() {
        return None;
    }
    let Defence { hp: city_hp, cap, estimate } = defence_of(brief, rules, m, &seen, observed);
    let value = crate::ai::front::city_damage_worth(CITY_BASE_HIT_POINTS, seen.population);
    let time_cost = value / brief.speed.pace_rounds(TIME_ROUNDS).max(1) as i32;
    let kinds = sight.view.terrain_kinds();
    let owner = brief.player;
    let ground = |t: TileId| crate::ai::siege_tactics::ground_percent(&sight.view, rules, t, owner);
    let foreign_on = |t: TileId| sight.visible_foreign_units.iter().any(|u| u.tile == t);
    let own_military_on = |t: TileId, except: EntityId| sight.own_units.iter().any(|u| u.id != except && u.tile == t && military(u.kind));
    let land = |t: TileId| {
        sight.view.tile(t).snapshot().is_some_and(|s| !kinds.is_water(s.terrain.0) && !kinds.is_mountains(s.terrain.0) && !s.city)
    };
    let threatened = |t: TileId| field.iter().filter(|(f, r)| distance(*f, t) <= *r).count() as i32;
    let melee_next = |t: TileId| {
        sight
            .visible_foreign_units
            .iter()
            .filter(|u| at_war(u.owner) && military(u.kind) && rules.units[u.kind.0 as usize].ranged_strength == 0 && dims.adjacent(u.tile, t))
            .count() as i32
    };

    // The posts on the ring: away from the enemy's field units, on strong ground.
    let candidates: Vec<TileId> = crate::math::hex_disc(dims, target, ring + 1).filter(|t| distance(*t, target) >= ring && land(*t) && !foreign_on(*t)).collect();
    let mut order: Vec<&OwnUnit> = group.iter().collect();
    // The shooters and the most valuable first.
    order.sort_by_key(|u| (army_role(rules, u.kind) == Some(ArmyRole::Melee), std::cmp::Reverse(worth(&KindStats::of(rules, u.kind), u.level)), u.id));
    let mut taken: Vec<TileId> = Vec::new();
    let mut posts = Vec::new();
    for u in &order {
        let best = candidates
            .iter()
            .filter(|t| !taken.contains(t) && !own_military_on(**t, u.id))
            .min_by_key(|t| (threatened(**t), distance(**t, target) != ring, distance(**t, u.tile) / 2, -ground(**t), distance(**t, u.tile), t.0))
            .copied();
        if let Some(tile) = best {
            taken.push(tile);
            posts.push((u.id, tile));
        }
    }

    // The fighters of a strike now, with their firing tiles.
    let city_strength = |preview: Option<crate::combat::exchange::CombatPreview>| preview.map_or(estimate.strength, |p| p.defender_strength) * CITY_RANGED_STRENGTH_PERCENT / 100;
    let mut strength_seen: Option<i64> = None;
    let mut fighters: Vec<Fighter> = Vec::new();
    let mut setups: Vec<bool> = Vec::new();
    let mut ways: Vec<Option<Awaited>> = Vec::new();
    let mut shots: Vec<(Shot, u16)> = Vec::new();
    let mut ids: Vec<(EntityId, bool)> = Vec::new();
    let mut claimed: Vec<TileId> = Vec::new();
    let mut outranged = false;
    for u in &members {
        let def = &rules.units[u.kind.0 as usize];
        let movement = i32::from(def.movement.max(1));
        let in_group = group.iter().any(|g| g.id == u.id);
        let unit_worth = worth(&KindStats::of(rules, u.kind), u.level);
        let estimated = estimated_strike(rules, u.kind, u.hp, &estimate);
        // A member not in place to strike within a round (on its way, or held where it
        // fights): its measured arrival.
        let way_of = |ready: u32| (ready > 1).then(|| awaited(sight, brief, rules, u, target, ring));
        match range(rules, u) {
            Some(r) => {
                outranged |= r > reach;
                let set_up = sets_up(sight, u);
                // With `sight`, only a tile the preview shoots the city from (its line of sight).
                let fits = |t: TileId| (1..=r).contains(&distance(t, target)) && (!fix.sight || observed.is_none() || sight.view.attack_preview_from(u.handle, t, target).is_some());
                // Where it fires from: this turn (a reachable tile with moves left; a siege
                // unit where it stands), else next turn from a tile it steps onto now (a
                // siege unit sets up there; a shooter whose step in takes its moves), else
                // later.
                let mut from = None;
                let mut ready = 0;
                let mut step_in = set_up;
                if in_group {
                    if set_up && fits(u.tile) {
                        from = Some(u.tile);
                    } else {
                        let stops: Vec<(TileId, u16)> = sight.view.own_reachable(u.handle);
                        let free = |t: TileId| fits(t) && !claimed.contains(&t) && (t == u.tile || (land(t) && !foreign_on(t) && !own_military_on(t, u.id)));
                        let rank = |t: TileId| (distance(t, target) <= reach, melee_next(t), -ground(t), t != u.tile, t.0);
                        let now = stops.iter().filter(|(t, left)| !set_up && *left > 0 && free(*t)).min_by_key(|(t, _)| rank(*t)).map(|(t, _)| *t);
                        let next = || stops.iter().filter(|(t, _)| free(*t)).min_by_key(|(t, left)| (*left == 0, rank(*t))).map(|(t, _)| *t);
                        from = now.or_else(next);
                        if now.is_none() {
                            step_in = true;
                            ready = 1;
                        }
                    }
                }
                let ready = match from {
                    Some(_) => ready,
                    None => (((distance(u.tile, target) - r).max(1) + movement - 1) / movement) as u32 + u32::from(set_up),
                };
                let way = way_of(ready);
                let preview = from.filter(|_| observed.is_some()).and_then(|t| sight.view.attack_preview_from(u.handle, t, target));
                if preview.is_some() && strength_seen.is_none() {
                    strength_seen = Some(city_strength(preview));
                }
                if let Some(p) = preview {
                    m.strength.insert(target, p.defender_strength);
                }
                let dealt = match (preview, estimated) {
                    (Some(p), _) => p.to_defender,
                    (None, Some(e)) => (e.dealt * 3 / 4, e.dealt, e.dealt * 5 / 4),
                    (None, None) => continue,
                };
                let at = from.unwrap_or(u.tile);
                if let Some(tile) = from {
                    claimed.push(tile);
                    shots.push((
                        Shot { unit: u.id, handle: u.handle, start: u.tile, from: tile, set_up, fire: ready == 0, wounded: u.hp < HEAL_BELOW_HP, stage: 0 },
                        dealt.1,
                    ));
                }
                fighters.push(Fighter {
                    ranged: true,
                    hp: u.hp,
                    worth: unit_worth,
                    dealt,
                    taken: (0, 0, 0),
                    city_hit: (0, 0, 0),
                    ready,
                    exposed: ready.saturating_sub(u32::from(step_in)),
                    covered: distance(at, target) > reach,
                    arrive: way.as_ref().map_or(1.0, |a| arrival(a, ready)),
                });
                // From its post on the ring: a siege unit sets up first, a shooter whose step
                // into range takes its moves fires the round after.
                setups.push(set_up || ring - r >= movement);
                ways.push(way);
                ids.push((u.id, in_group));
            }
            None => {
                // Next to the city this turn (a free tile it reaches with moves left), else later.
                let adjacent: Option<TileId> = if !in_group {
                    None
                } else if distance(u.tile, target) == 1 {
                    Some(u.tile)
                } else {
                    sight
                        .view
                        .own_reachable(u.handle)
                        .into_iter()
                        .filter(|(t, left)| *left > 0 && distance(*t, target) == 1 && land(*t) && !foreign_on(*t) && !own_military_on(*t, u.id))
                        .min_by_key(|(t, _)| (-ground(*t), t.0))
                        .map(|(t, _)| t)
                };
                let ready = match adjacent {
                    Some(_) => 0,
                    None => (((distance(u.tile, target) - 1).max(1) + movement - 1) / movement) as u32,
                };
                let way = way_of(ready);
                let preview = adjacent.filter(|_| observed.is_some()).and_then(|t| sight.view.attack_preview_from(u.handle, t, target));
                let (dealt, taken) = match (preview, estimated) {
                    (Some(p), _) => (p.to_defender, p.to_attacker),
                    (None, Some(e)) => ((e.dealt * 3 / 4, e.dealt, e.dealt * 5 / 4), (e.taken * 3 / 4, e.taken, e.taken * 5 / 4)),
                    (None, None) => continue,
                };
                let arrive = way.as_ref().map_or(1.0, |a| arrival(a, ready));
                fighters.push(Fighter { ranged: false, hp: u.hp, worth: unit_worth, dealt, taken, city_hit: (0, 0, 0), ready, exposed: ready, covered: false, arrive });
                setups.push(ring > movement);
                ways.push(way);
                ids.push((u.id, in_group));
            }
        }
    }
    // The city's strike on each fighter where it stands to fight.
    let strength = strength_seen.unwrap_or(estimate.strength * CITY_RANGED_STRENGTH_PERCENT / 100);
    for (f, (id, _)) in fighters.iter_mut().zip(&ids) {
        let Some(u) = members.iter().find(|u| u.id == *id) else { continue };
        let def = &rules.units[u.kind.0 as usize];
        let at = shots.iter().find(|(s, _)| s.unit == *id).map_or(u.tile, |(s, _)| s.from);
        let defence = i64::from(def.strength.max(1)) * 100 * (100 + ground(at)) / 100;
        f.city_hit = city_hit(strength, defence);
    }
    let city = CityModel { hp: city_hp, cap, heal: CITY_HEAL_PER_TURN as u16, value, time_cost, extra_hits };

    // Strike now: the shooters that fire this turn (or step in and set up for the
    // next), every melee unit the round it reaches the walls (a blow comes when the
    // city may fall, and a late one finishes a worn city; one on its way by its
    // arrival); the other shooters join a later strike (waiting is the other option:
    // no stream of shooters under the walls' strike). Each wounded member in or out
    // as it pays.
    let now_ok: Vec<bool> = fighters
        .iter()
        .zip(&ids)
        .map(|(f, (id, in_group))| if f.ranged { *in_group && shots.iter().any(|(s, _)| s.unit == *id) } else { true })
        .collect();
    let striking = |rested: &[EntityId], skip: Option<usize>| -> Vec<Fighter> {
        (0..fighters.len()).filter(|j| now_ok[*j] && Some(*j) != skip && !rested.contains(&ids[*j].0)).map(|j| fighters[j]).collect()
    };
    let mut now = simulate(&striking(&[], None), &city, 0, STRIKE_WINDOW);
    let mut rested: Vec<EntityId> = Vec::new();
    for i in 0..fighters.len() {
        if !now_ok[i] || fighters[i].hp >= HEAL_BELOW_HP {
            continue;
        }
        let out = simulate(&striking(&rested, Some(i)), &city, 0, STRIKE_WINDOW);
        if out.value > now.value {
            now = out;
            rested.push(ids[i].0);
        }
    }
    // Waiting k rounds on the ring: the members on their way by their measured
    // arrival, the group healed, the city healed back.
    let mut later = Outcome { value: f64::MIN, ..Outcome::default() };
    let mut wait = 0;
    for k in 1..=MAX_WAIT {
        let (healed, city_later) = waited(&fighters, &setups, &ways, &city, k);
        let out = simulate(&healed, &city_later, k, STRIKE_WINDOW);
        if out.value > later.value {
            later = out;
            wait = k;
        }
    }
    let engaged = m.engaged.contains(&target) || group.iter().any(|u| distance(u.tile, target) <= reach);
    // A strike needs a shot to make, or a blow that may take the city now.
    // A melee member steps next to the walls this turn and strikes the next: a group of
    // melee units alone strikes from the ring too.
    let acts = shots.iter().any(|(s, _)| !rested.contains(&s.unit)) || now.capture > 0.0 && fighters.iter().zip(&ids).any(|(f, (_, in_group))| !f.ranged && *in_group && f.ready <= 1);
    // A capture all but sure now is not traded for a better one later.
    // A city out of sight is valued by the player's estimate (no walls seen): it is
    // looked at first, then struck.
    let strike = observed.is_some() && acts && now.value > 0.0 && (now.value >= later.value || now.soon >= SURE_CAPTURE);
    // A capture in reach: by the strike now, or after a wait the group survives to strike.
    let in_reach = now.capture >= MIN_CAPTURE || later.capture >= MIN_CAPTURE && later.value > 0.0;
    let choice = match (strike, engaged) {
        _ if !in_reach => Choice::Front,
        (true, false) => Choice::Strike,
        (true, true) => Choice::Press,
        (false, false) => Choice::Muster,
        (false, true) => Choice::Withdraw,
    };
    let rate = if strike || !in_reach { now.rate(0) } else { later.rate(wait).max(now.rate(0)) };
    let group_ids: Vec<EntityId> = group.iter().map(|u| u.id).collect();
    let mut shots: Vec<(Shot, u16)> = shots.into_iter().filter(|(s, _)| group_ids.contains(&s.unit) && !rested.contains(&s.unit)).collect();
    shots.sort_by_key(|(s, dealt)| (std::cmp::Reverse(*dealt), s.unit));
    let fortified_unanswered = cap > CITY_BASE_HIT_POINTS && !outranged;
    let awaited_n = ways.iter().filter(|w| w.is_some()).count();
    let line = format!(
        "city_hp={city_hp} cap={cap} seen={} group={} members={} awaited={awaited_n} shots={} rested={} now={:.0} (capture {:.2}, loss {:.0}) wait={wait} later={:.0} (capture {:.2}) rate={rate:.1} fortified={} outranged={outranged}",
        observed.is_some(),
        group.len(),
        members.len(),
        shots.len(),
        rested.len(),
        now.value,
        now.capture,
        now.loss,
        later.value,
        later.capture,
        cap > CITY_BASE_HIT_POINTS,
    ) + &if std::env::var_os("SIEGE_WALLS_TRACE_FIGHTERS").is_some() {
        format!(
            " fighters={:?}",
            fighters.iter().zip(&ids).map(|(f, (id, _))| (f.ranged, f.hp, f.dealt.1, f.city_hit.1, f.ready, (f.arrive * 100.0) as u8, members.iter().find(|u| u.id == *id).map(|u| distance(u.tile, target)))).collect::<Vec<_>>()
        )
    } else {
        String::new()
    };
    // The calibration's record (`SIEGE_WALLS_TRACE`): the group of the operation's
    // own city, and each member on its way with its features and predictions.
    if tracing() && own_way {
        // Each member of the group with the round it strikes from (0 or 1: in place).
        let ids_text: Vec<String> = ids.iter().zip(&fighters).filter(|((_, in_group), _)| *in_group).map(|((id, _), f)| format!("{}:{}", id.0, f.ready)).collect();
        eprintln!("siege-walls group m{mind} p{} r{} t{} ids={}", brief.player.0, brief.round, target.0, ids_text.join(","));
        for ((id, _), way) in ids.iter().zip(&ways) {
            let Some(a) = way else { continue };
            let pred: Vec<String> = (1..=MAX_WAIT).map(|k| format!("{:.3}", arrival(a, k))).collect();
            eprintln!(
                "siege-walls await m{mind} p{} r{} t{} u{} turns={} eng={} wnd={} thr={:.3} pred={}",
                brief.player.0,
                brief.round,
                target.0,
                id.0,
                a.turns,
                u8::from(a.engaged),
                u8::from(a.wounded),
                a.threat,
                pred.join(",")
            );
        }
    }
    let first_round = simulate(&striking(&rested, None).into_iter().filter(|f| f.ready == 0).collect::<Vec<_>>(), &city, 0, 1);
    // A city out of sight is neither valued by previews nor shot: the cheapest healthy
    // member (a melee one first) steps where it sees the city — as far from it as its
    // sight allows — before anything else of the turn.
    let spot = if choice != Choice::Front && observed.is_none() {
        let mut spotters: Vec<&OwnUnit> = group.iter().filter(|u| !rested.contains(&u.id) && u.hp >= HEAL_BELOW_HP).collect();
        spotters.sort_by_key(|u| (range(rules, u).is_some(), worth(&KindStats::of(rules, u.kind), u.level), u.id));
        spotters.into_iter().find_map(|u| {
            let eyes = i32::from(rules.units[u.kind.0 as usize].sight.max(1));
            sight
                .view
                .own_reachable(u.handle)
                .into_iter()
                .filter(|(t, _)| distance(*t, target) <= eyes && land(*t) && !foreign_on(*t) && !own_military_on(*t, u.id))
                .min_by_key(|(t, _)| (std::cmp::Reverse(distance(*t, target)), threatened(*t), -ground(*t), t.0))
                .map(|(t, _)| Spot { unit: u.id, handle: u.handle, to: t, done: false })
        })
    } else {
        None
    };
    let plan = Plan {
        target,
        choice,
        spot,
        members: group_ids,
        shots: if choice.strikes() { shots.into_iter().map(|(s, _)| s).collect() } else { Vec::new() },
        assault: if choice.strikes() {
            group.iter().filter(|u| range(rules, u).is_none() && !rested.contains(&u.id)).map(|u| u.id).collect()
        } else {
            Vec::new()
        },
        posts,
        screened: Vec::new(),
        reach,
        capture_now: first_round.capture,
        capture: now.capture,
        close: fix.close,
    };
    Some(Weighed { plan, rate, in_reach, fortified_unanswered, rested: rested.len(), line })
}

// ---------------------------------------------------------------------------
// The choice of a target before the march (the books).

/// The value a round of war of besieging `city` with a fresh army of `army`
/// (one entry a unit) that marches `march` rounds to it: `simulate` over the
/// rules' estimate of the city (its defence as `defence_of` knows it: the last
/// seen state, else the rules' default for its era and size), every unit
/// striking from the first round, each by its arrival after the march's
/// turns; and whether the city is fortified with no unit of the army that
/// outranges its strike.
pub fn rate_target(brief: &TurnBrief, rules: &CompiledRuleset, walls: Option<&SiegeWalls>, city: &SeenCity, army: &[crate::ids::UnitKindId], march: u32) -> (f64, bool) {
    let empty = Memory::default();
    let memory = walls.map(|w| w.memory.lock().expect("no siege thread panicked"));
    let m = memory.as_ref().and_then(|mm| mm.get(&brief.player)).unwrap_or(&empty);
    let Defence { cap, estimate, .. } = defence_of(brief, rules, m, city, None);
    let way = Awaited { turns: march, engaged: false, wounded: false, threat: 0.0 };
    let strength = estimate.strength * CITY_RANGED_STRENGTH_PERCENT / 100;
    let mut outranged = false;
    let fighters: Vec<Fighter> = army
        .iter()
        .filter_map(|kind| {
            let def = &rules.units[kind.0 as usize];
            let e = estimated_strike(rules, *kind, UNIT_MAX_HP, &estimate)?;
            let range = crate::ai::combat::ranged_range(rules, *kind);
            outranged |= range.is_some_and(|r| r > CITY_ATTACK_RANGE);
            let ranged = range.is_some();
            Some(Fighter {
                ranged,
                hp: UNIT_MAX_HP,
                worth: worth(&KindStats::of(rules, *kind), 0),
                dealt: (e.dealt * 3 / 4, e.dealt, e.dealt * 5 / 4),
                taken: if ranged { (0, 0, 0) } else { (e.taken * 3 / 4, e.taken, e.taken * 5 / 4) },
                city_hit: city_hit(strength, i64::from(def.strength.max(1)) * 100),
                ready: 0,
                exposed: 0,
                covered: range.is_some_and(|r| r > CITY_ATTACK_RANGE),
                arrive: arrival(&way, march),
            })
        })
        .collect();
    let value = crate::ai::front::city_damage_worth(CITY_BASE_HIT_POINTS, city.population);
    let time_cost = value / brief.speed.pace_rounds(TIME_ROUNDS).max(1) as i32;
    let model = CityModel { hp: cap, cap, heal: CITY_HEAL_PER_TURN as u16, value, time_cost, extra_hits: 0 };
    let out = simulate(&fighters, &model, march, STRIKE_WINDOW);
    (out.rate(march), cap > CITY_BASE_HIT_POINTS && !outranged)
}

/// The next step of the strike's shots, the strongest shooter first: its
/// move to its firing tile, its set-up, its shot (at the city; at an enemy
/// unit next to one of our shooters when the shot kills it and the city does
/// not fall this round). None when every shot is made (or refused).
fn next_shot(view: &UnitView<'_, '_>, plan: &mut Plan) -> Option<Decision> {
    let local = view.local;
    let pv = &local.view;
    let rules = pv.rules();
    let target = plan.target;
    let shooters: Vec<TileId> = plan.shots.iter().map(|s| s.from).collect();
    let capture_now = plan.capture_now;
    for shot in plan.shots.iter_mut() {
        while shot.stage < 3 {
            let actor = Actor::Unit(shot.unit);
            let stage = shot.stage;
            shot.stage += 1;
            let command = match stage {
                0 if shot.from != shot.start => Command::MoveUnit { unit: shot.handle, destination: shot.from },
                1 if shot.set_up && pv.must_set_up_first(shot.handle) => {
                    count(7);
                    Command::SetUpRangedAttack { unit: shot.handle }
                }
                2 if shot.fire => {
                    // A kill of an enemy unit next to one of our shooters first, while
                    // the city does not fall this round.
                    let threat = local
                        .foreign_units
                        .iter()
                        .filter(|u| local.relation(u.owner).is_some_and(|r| r.at_war) && u.tile != target)
                        .filter(|u| role_of(rules, u.kind) == AiRole::Military && shooters.iter().any(|s| pv.dimensions().adjacent(*s, u.tile)))
                        .filter_map(|u| pv.attack_preview(shot.handle, u.tile).filter(|p| p.to_defender.1 >= u.hp).map(|p| (p.to_defender.1, u.tile)))
                        .max_by_key(|(d, t)| (*d, std::cmp::Reverse(t.0)));
                    match threat {
                        Some((_, tile)) if capture_now < 0.5 => {
                            count(6);
                            Command::RangedAttack { unit: shot.handle, target: tile }
                        }
                        _ => {
                            // The preview reads the city wherever the asked member stands.
                            let hurts = pv.attack_preview(shot.handle, target).is_some_and(|p| p.defender_hp > 1 && p.to_defender.1 > 0);
                            if !hurts {
                                shot.stage = 3;
                                continue;
                            }
                            count(4);
                            if shot.wounded {
                                count(5);
                            }
                            Command::RangedAttack { unit: shot.handle, target }
                        }
                    }
                }
                _ => continue,
            };
            if !pv.accepts(&command) {
                // Blocked or out of reach: this shooter is done.
                shot.stage = 3;
                continue;
            }
            let reason = match command {
                Command::MoveUnit { .. } => Reason::Advance,
                _ => Reason::AttackCity,
            };
            return Some(Decision::act(actor, reason, command));
        }
    }
    None
}

/// The asked member's own part in a strike once the shots are made: a melee
/// member assaults when its blow is worth its risk, else screens next to the
/// city; a shooter that fired holds; the rest go to their posts.
fn own_part(view: &UnitView<'_, '_>, tools: &mut PlanTools<'_>, plan: &mut Plan, city: Option<ObservedCity>) -> Decision {
    let local = view.local;
    let unit = local.unit;
    let pv = &local.view;
    let rules = pv.rules();
    let actor = Actor::Unit(unit.id);
    if plan.shots.iter().any(|s| s.unit == unit.id) {
        return hold(view, Reason::AttackCity);
    }
    if !plan.assault.contains(&unit.id) {
        return to_post(view, tools, plan);
    }
    let dims = pv.dimensions();
    let Some(city) = city else { return to_post(view, tools, plan) };
    if unit.can_attack && unit.moves_left > 0 {
        let value = f64::from(crate::ai::front::city_damage_worth(CITY_BASE_HIT_POINTS, city.population));
        let unit_worth = f64::from(worth(&KindStats::of(rules, unit.kind), unit.level));
        let mut tiles: Vec<TileId> = Vec::new();
        if dims.distance(unit.tile, city.tile) == Some(1) {
            tiles.push(unit.tile);
        } else {
            tiles.extend(pv.own_reachable(unit.handle).into_iter().filter(|(t, left)| *left > 0 && dims.distance(*t, city.tile) == Some(1) && can_stop(view, rules, *t)).map(|(t, _)| t));
        }
        let best = tiles
            .iter()
            .filter_map(|t| {
                let p = pv.attack_preview_from(unit.handle, *t, city.tile)?;
                let d = p.to_defender;
                let falls = chance_at_least((f64::from(d.0), f64::from(d.1), f64::from(d.2)), f64::from(city.hp), 0.0);
                let a = p.to_attacker;
                let dies = chance_at_least((f64::from(a.0), f64::from(a.1), f64::from(a.2)), f64::from(unit.hp), 0.0);
                let ev = falls * value - dies * unit_worth;
                Some((ev, *t))
            })
            .max_by(|a, b| a.0.total_cmp(&b.0).then(b.1 .0.cmp(&a.1 .0)));
        // A blow worth its risk by the capture it may make, or one that wears the city
        // down for the next blow of the strike and leaves its striker able to fight on.
        let wears = |t: TileId| {
            plan.capture > 0.0
                && pv.attack_preview_from(unit.handle, t, city.tile).is_some_and(|p| p.to_defender.1 > 0 && unit.hp.saturating_sub(p.to_attacker.2) >= crate::ai::front::STRIKE_KEEPS_HP)
        };
        if let Some((_, tile)) = best.filter(|(ev, t)| *ev > 0.0 || wears(*t)) {
            if tile != unit.tile {
                return Decision::act(actor, Reason::Advance, Command::MoveUnit { unit: unit.handle, destination: tile });
            }
            let strike = Command::AttackCity { attacker: unit.handle, city: city.handle };
            if pv.accepts(&strike) {
                count(8);
                if unit.hp < HEAL_BELOW_HP {
                    count(9);
                }
                trace(local.player, local.round, plan.target, &format!("assault hp={} city_hp={}", unit.hp, city.hp));
                return Decision::act(actor, Reason::AttackCity, strike);
            }
        }
    }
    // No blow worth it: a healthy member closes in between the city and the shooters.
    if unit.hp >= HEAL_BELOW_HP && unit.moves_left > 0 && dims.distance(unit.tile, city.tile) != Some(1) {
        let shooters: Vec<TileId> = plan.shots.iter().map(|s| s.from).collect();
        let near = |t: TileId| shooters.iter().map(|s| dims.distance(*s, t).unwrap_or(i32::MAX)).min().unwrap_or(0);
        let screen = pv
            .own_reachable(unit.handle)
            .into_iter()
            .filter(|(t, _)| dims.distance(*t, city.tile) == Some(1) && can_stop(view, rules, *t))
            .min_by_key(|(t, left)| (near(*t), -crate::ai::siege_tactics::ground_percent(pv, rules, *t, local.player), std::cmp::Reverse(*left), t.0))
            .map(|(t, _)| t);
        if let Some(tile) = screen {
            if !plan.screened.contains(&unit.id) {
                plan.screened.push(unit.id);
                count(10);
            }
            return Decision::act(actor, Reason::Advance, Command::MoveUnit { unit: unit.handle, destination: tile });
        }
    }
    if dims.distance(unit.tile, city.tile) == Some(1) {
        return hold(view, Reason::Rally);
    }
    if plan.close && unit.moves_left > 0 && plan.capture > 0.0 {
        // `close`: the walls out of reach this turn, as close as it gets, to strike the next.
        let here = dims.distance(unit.tile, city.tile).unwrap_or(i32::MAX);
        let step = pv
            .own_reachable(unit.handle)
            .into_iter()
            .filter(|(t, _)| *t != unit.tile && can_stop(view, rules, *t) && dims.distance(*t, city.tile).is_some_and(|d| d > 1 && d < here))
            .min_by_key(|(t, left)| (dims.distance(*t, city.tile), -crate::ai::siege_tactics::ground_percent(pv, rules, *t, local.player), std::cmp::Reverse(*left), t.0))
            .map(|(t, _)| t);
        if let Some(tile) = step {
            return Decision::act(actor, Reason::Advance, Command::MoveUnit { unit: unit.handle, destination: tile });
        }
    }
    to_post(view, tools, plan)
}

/// To the member's post on the ring, then fortified there (healing).
fn to_post(view: &UnitView<'_, '_>, tools: &mut PlanTools<'_>, plan: &Plan) -> Decision {
    let unit = view.local.unit;
    let reason = if unit.hp < HEAL_BELOW_HP { Reason::Heal } else { Reason::Rally };
    if let Some(strike) = skirmish(view, plan.target, plan.reach) {
        return strike;
    }
    let Some(&(_, post)) = plan.posts.iter().find(|(id, _)| *id == unit.id) else { return hold(view, reason) };
    if unit.tile == post || unit.moves_left == 0 {
        return hold(view, reason);
    }
    // The turn's walk ends out of the city's reach: at the post, or at the tile nearest
    // to it that is out of reach (a walk around the walls ends no turn within them).
    let pv = &view.local.view;
    let dims = pv.dimensions();
    let rules = pv.rules();
    let off = |t: TileId| dims.distance(t, plan.target).is_some_and(|d| d > plan.reach);
    let far = |t: TileId| dims.distance(t, post).unwrap_or(i32::MAX);
    let stop = pv
        .own_reachable(unit.handle)
        .into_iter()
        .filter(|(t, _)| *t == unit.tile || off(*t) && can_stop(view, rules, *t))
        .min_by_key(|(t, _)| (!off(*t), far(*t), *t != unit.tile, t.0))
        .map(|(t, _)| t);
    match stop {
        Some(tile) if tile != unit.tile && far(tile) < far(unit.tile) => {
            return Decision::act(Actor::Unit(unit.id), reason, Command::MoveUnit { unit: unit.handle, destination: tile });
        }
        Some(_) if off(unit.tile) => return hold(view, reason),
        _ => {}
    }
    let step = move_along(view, tools, post, reason);
    if step.command.is_some() {
        step
    } else {
        hold(view, reason)
    }
}

/// A member that waits still fights the enemy's field units that come to it,
/// from where it stands (no step into the city's reach): a shooter its best
/// shot (a kill first), a melee unit a blow that kills, or one that deals half
/// as much again as it takes and leaves it healthy (the front's rule).
fn skirmish(view: &UnitView<'_, '_>, city: TileId, reach: i32) -> Option<Decision> {
    let local = view.local;
    let unit = local.unit;
    if !unit.can_attack || unit.moves_left == 0 {
        return None;
    }
    let pv = &local.view;
    let rules = pv.rules();
    let ranged = range(rules, &unit).is_some();
    let dims = pv.dimensions();
    let foes = local.foreign_units.iter().filter(|u| u.tile != city && local.relation(u.owner).is_some_and(|r| r.at_war) && role_of(rules, u.kind) == AiRole::Military);
    // A melee blow that kills moves its striker onto the foe's tile: never into the city's reach.
    let out_of_reach = |t: TileId| dims.distance(t, city).is_some_and(|d| d > reach);
    let best = foes
        .filter(|u| if ranged { true } else { dims.adjacent(u.tile, unit.tile) && out_of_reach(u.tile) })
        .filter_map(|u| {
            let p = pv.attack_preview(unit.handle, u.tile)?;
            let (dealt, taken) = (p.to_defender.1, p.to_attacker.1);
            let kills = dealt >= u.hp;
            let worth_it = ranged && dealt > 0 || kills && taken < unit.hp || 2 * dealt >= 3 * taken && unit.hp.saturating_sub(taken) >= HEAL_BELOW_HP;
            worth_it.then_some((kills, dealt, u.tile, u.handle))
        })
        .max_by_key(|(kills, dealt, tile, _)| (*kills, *dealt, std::cmp::Reverse(tile.0)))?;
    let (_, _, tile, handle) = best;
    let command = if ranged { Command::RangedAttack { unit: unit.handle, target: tile } } else { Command::AttackUnit { attacker: unit.handle, defender: handle } };
    pv.accepts(&command).then(|| Decision::act(Actor::Unit(unit.id), Reason::AttackUnit, command))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shooter(dealt: u16, hp: u16, ready: u32) -> Fighter {
        Fighter { ranged: true, hp, worth: 120, dealt: (dealt - 4, dealt, dealt + 4), taken: (0, 0, 0), city_hit: (18, 22, 26), ready, exposed: ready, covered: false, arrive: 1.0 }
    }

    fn swordsman(dealt: u16, ready: u32) -> Fighter {
        Fighter { ranged: false, hp: 100, worth: 120, dealt: (dealt - 5, dealt, dealt + 5), taken: (20, 25, 30), city_hit: (12, 15, 18), ready, exposed: ready, covered: false, arrive: 1.0 }
    }

    fn city(hp: u16) -> CityModel {
        CityModel { hp, cap: 200, heal: 20, value: 1080, time_cost: 36, extra_hits: 0 }
    }

    /// Five archers and a swordsman take a full city in a strike: the volley
    /// of the first round brings it within the blow; one archer alone does not
    /// dent it, and the model sends it to wait.
    #[test]
    fn a_complete_force_takes_the_city_and_a_lone_shooter_waits() {
        let mut force: Vec<Fighter> = (0..5).map(|_| shooter(36, 100, 0)).collect();
        force.push(swordsman(40, 0));
        let out = simulate(&force, &city(200), 0, STRIKE_WINDOW);
        assert!(out.capture > 0.9, "{out:?}");
        assert!(out.value > 500.0, "{out:?}");
        let lone = simulate(&[shooter(36, 100, 0), swordsman(40, 0)], &city(200), 0, STRIKE_WINDOW);
        assert!(lone.capture < 0.05, "{lone:?}");
        assert!(lone.value < 0.0, "a stream of units only loses them: {lone:?}");
    }

    /// A worn city falls to the units in place: a second strike is worth more
    /// than going back to heal while the city heals out of reach.
    #[test]
    fn a_second_strike_in_place_beats_going_back() {
        let force: Vec<Fighter> = vec![shooter(30, 60, 0), shooter(30, 70, 0), shooter(30, 100, 0), swordsman(40, 0)];
        let worn = city(70);
        let now = simulate(&force, &worn, 0, STRIKE_WINDOW);
        let (healed, later_city) = waited(&force, &[false; 4], &[None; 4], &worn, 4);
        let later = simulate(&healed, &later_city, 4, STRIKE_WINDOW);
        assert!(now.value > later.value, "press {now:?} against heal {later:?}");
    }

    /// A wounded shooter fires when its shot brings the capture forward.
    #[test]
    fn a_wounded_shot_that_brings_the_capture_pays() {
        let mut force: Vec<Fighter> = vec![shooter(36, 100, 0), shooter(36, 100, 0), swordsman(40, 0)];
        let with = {
            let mut f = force.clone();
            f.push(shooter(36, 30, 0));
            simulate(&f, &city(140), 0, STRIKE_WINDOW)
        };
        let without = simulate(&force, &city(140), 0, STRIKE_WINDOW);
        assert!(with.value > without.value, "with {with:?} without {without:?}");
        force.clear();
    }

    #[test]
    fn the_chance_of_a_range_is_its_share_above_the_need() {
        assert_eq!(chance_at_least((10.0, 15.0, 20.0), 5.0, 0.0), 1.0);
        assert_eq!(chance_at_least((10.0, 15.0, 20.0), 25.0, 0.0), 0.0);
        let half = chance_at_least((10.0, 15.0, 20.0), 15.5, 0.0);
        assert!((0.4..0.6).contains(&half), "{half}");
    }
}
