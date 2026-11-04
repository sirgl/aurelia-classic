//! Diagnostic counters of the war logic (classic/docs/ai-war-audit-current.md): what the
//! war estimates were, why operations ended, how many troops were free for a march, how
//! many raids formed and what they did. A tool switches them on for the current thread
//! (`enable`), plays games, and reads them (`take`). Off by default: a game that nobody
//! watches does not even look at them, and no decision ever reads them (the probe only
//! writes), so the twins of the non-interference tests stay equal.

use crate::ids::PlayerId;
use std::cell::RefCell;

/// Why an operation ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum EndCause {
    /// The target is not the enemy's any more and is ours: the city was taken.
    Captured,
    /// The target is not the enemy's any more (lost to a third party, or out of sight).
    TargetGone,
    /// Forming took longer than its limit.
    FormTimeout,
    /// Marching took longer than `MARCH_LIMIT`.
    MarchTimeout,
    /// The war ended while the army marched.
    Peace,
    /// The defenders seen at the target are half as strong again as the army.
    Outmatched,
    /// Its army was worth more at a city in danger (`ai::defence`).
    Recalled,
}

/// One evaluation of a candidate target under `WarGate::Value`.
#[derive(Debug, Clone, Copy)]
pub struct Estimate {
    pub round: u32,
    pub player: PlayerId,
    pub enemy: PlayerId,
    pub population: u32,
    /// Hundredths of score points: the value, the gain after the chance and the leader's weight,
    /// the cost, the city's unrest as priced, the city's worth.
    pub value: i64,
    pub gain: i64,
    pub cost: i64,
    pub unrest: i64,
    pub worth: i64,
    pub missing: u8,
    pub need: u8,
    pub free: u8,
    /// The empire's excess happiness and the allocator's price of a point (hundredths of points).
    pub excess: i32,
    pub happiness_price: i64,
    /// The score of one citizen at the map's scale.
    pub citizen: i64,
}

/// A marching operation at the start of its player's turn (`operations::plan`): how
/// near its army came and whether the player sees its target.
#[derive(Debug, Clone, Copy)]
pub struct MarchSample {
    pub round: u32,
    pub player: PlayerId,
    pub enemy: PlayerId,
    pub target: u32,
    /// The round it launched (its march's first round): a sample belongs to the
    /// operation of the same player, target and launch.
    pub launched: u32,
    /// Its members, and the nearest one's distance to the target (-1: no member).
    pub members: u32,
    pub nearest: i32,
    /// The target's tile is in the player's sight now.
    pub sighted: bool,
}

/// One look for an operation by a player that could start one.
#[derive(Debug, Clone, Copy)]
pub struct StartCheck {
    pub round: u32,
    pub player: PlayerId,
    /// Army units free for a march (`operations::free_units`).
    pub free: u8,
    /// Army units the player has, and how they are held: in an operation, as the
    /// garrison a city wants, as a guard of a city in danger, healing, with another task.
    pub army: u8,
    pub in_operation: u8,
    pub garrison: u8,
    pub guard: u8,
    pub hurt: u8,
    pub other: u8,
    /// Cities of the player, and the garrison each wants.
    pub cities: u8,
    /// Candidates that passed value and time; whether one started.
    pub candidates: u8,
    pub started: bool,
}

/// An operation that ended.
#[derive(Debug, Clone, Copy)]
pub struct OperationEnd {
    pub round: u32,
    pub player: PlayerId,
    pub enemy: PlayerId,
    pub cause: EndCause,
    pub marching: bool,
    pub age: u32,
    /// The target's tile: an end belongs to the start of the same player and target.
    pub target: u32,
}

/// An operation admitted (`operations::start`).
#[derive(Debug, Clone, Copy)]
pub struct StartNote {
    pub round: u32,
    pub player: PlayerId,
    pub enemy: PlayerId,
    pub target: u32,
    /// The value it was admitted at, and the rounds that value counted (assemble + march + siege).
    pub value: i64,
    pub rounds: u32,
    /// Units still to build (the need less the free units), the need, the free units.
    pub missing: u32,
    pub need: u32,
    pub free: u32,
    /// What the capture chance is learned from (`operations::chance_features`), and the chance
    /// the operation was admitted with, percent (None: the constant's).
    pub features: [i64; 12],
    pub chance: Option<i64>,
}

/// An operation the defence recalled and what stood behind it.
#[derive(Debug, Clone, Copy, Default)]
pub struct Recall {
    pub round: u32,
    pub player: u8,
    pub target: u32,
    /// The running value (`operations::running_value`), the same case over the rounds the
    /// peace's `running_case` counts (the march and the siege only), and the value at the start.
    pub value: i64,
    pub value_case: i64,
    /// The value over the rounds still to take, and the saving of the members that reach, net of their worth.
    pub value_expected: i64,
    pub net_saved: i64,
    pub start_value: Option<i64>,
    /// The running case: gain, cost, units missing, units needed; the same at the start.
    pub gain: i64,
    pub cost: i64,
    pub missing: u32,
    pub need: u32,
    pub start_missing: u32,
    pub start_need: u32,
    pub start_free: u32,
    /// What the army saves at the city that saves most.
    pub saved: i64,
    pub age: u32,
    /// Rounds since the operation started.
    pub life: u32,
    pub marching: bool,
    pub members: u32,
    pub army_power: u32,
    /// The endangered city: threat, guard, planned guard, stake, and the chance to lose it.
    pub threat: u32,
    pub guard: u32,
    pub planned: u32,
    pub stake: i64,
    /// The strongest attacker is: 0 the operation's enemy at war, 1 another at war, 2 at peace.
    pub attacker: u8,
    /// The endangered city is the muster city; the nearest member's distance to it.
    pub at_muster: bool,
    pub nearest_member: i32,
    /// The saving the same city would give without the army being able to arrive in time.
    pub reaches: bool,
}

/// A raid that formed, and what it did (`ai::raid`).
#[derive(Debug, Clone, Copy, Default)]
pub struct RaidCounters {
    pub formed: u64,
    pub declared: u64,
    pub ended: u64,
    pub ended_group_gone: u64,
    pub ended_limit: u64,
    pub ended_peace: u64,
    pub pillages: u64,
    pub pillage_gold: u64,
    pub plunders: u64,
    pub kills: u64,
    pub retreats: u64,
    /// Evaluations: chosen with a negative conquest, and all.
    pub chosen_negative_conquest: u64,
    pub evaluated: u64,
}

#[derive(Debug, Clone, Default)]
pub struct WarProbe {
    pub enabled: bool,
    pub estimates: Vec<Estimate>,
    pub checks: Vec<StartCheck>,
    pub ends: Vec<OperationEnd>,
    /// (round, declarer, enemy, for a raid).
    pub declarations: Vec<(u32, PlayerId, PlayerId, bool)>,
    pub launched: Vec<(u32, PlayerId, PlayerId)>,
    /// Recalls by the defence, with what decided them.
    pub recalls: Vec<Recall>,
    /// Operations started: the value they were admitted at.
    pub starts: Vec<StartNote>,
    pub raids: RaidCounters,
    /// Cities the player took, as its end of turn decides them: (round, player, the city's tile,
    /// the owner the player remembered there).
    pub captures: Vec<(u32, PlayerId, u32, Option<PlayerId>)>,
    /// Captured cities priced by `ai::conquest::priced_case`: (round, player, the city's
    /// tile, its citizens, the empire's excess happiness before it, the case).
    pub conquests: Vec<(u32, PlayerId, u32, u32, i32, crate::ai::conquest::Case)>,
    /// The marching operations, every turn of their march.
    pub marches: Vec<MarchSample>,
}

thread_local! {
    static PROBE: RefCell<WarProbe> = RefCell::new(WarProbe::default());
}

/// Switch the probe of this thread on (and clear it) or off.
pub fn enable(on: bool) {
    PROBE.with(|p| *p.borrow_mut() = WarProbe { enabled: on, ..WarProbe::default() });
}

/// The probe of this thread, which is cleared and keeps recording.
pub fn take() -> WarProbe {
    PROBE.with(|p| {
        let mut p = p.borrow_mut();
        let enabled = p.enabled;
        std::mem::replace(&mut *p, WarProbe { enabled, ..WarProbe::default() })
    })
}

/// Whether the probe of this thread records.
pub fn on() -> bool {
    PROBE.with(|p| p.borrow().enabled)
}

/// Record something when the probe is on.
pub(crate) fn note(f: impl FnOnce(&mut WarProbe)) {
    PROBE.with(|p| {
        let mut p = p.borrow_mut();
        if p.enabled {
            f(&mut p);
        }
    });
}

