//! Orders judged by their effect, not by a proxy (classic/docs/ai-effects-checked.md).
//!
//! The game answers an order the rules accept with "applied" even when it
//! changes nothing: a march into a unit of another player stops before the
//! first step (`WalkOutcome::Blocked`) and clears its goal. An AI that counts
//! the answer as progress orders the same march again and again. Here a walk
//! is judged by what it did: the driver asks the unit again after every
//! applied order, from a fresh local observation, and a walk that left the
//! unit on its tile is a failed attempt of its task (`AiState::note_failure`,
//! the path of a refused order). The planner then walks another way or picks
//! another target, and the same walk is not ordered again from the same tile
//! in the same turn.
//!
//! The memory is the turn's only (the walks sent, by player and unit): the
//! driver asks a unit right after its order, and a player's turn never
//! straddles a save, so nothing of it is saved.
//!
//! The other two checks of the same class read the rules instead of a proxy:
//! a worker is wanted for the jobs the workers' own job finder offers
//! (`planner_workers::open_jobs`), not for every unimproved tile, and a job
//! is ranked and kept by the path of the unit's own movement profile
//! (`planner_workers::Reach`), not by the straight distance.

use std::sync::Mutex;

use crate::ai::planner::Decision;
use crate::ai::state::AiState;
use crate::ai::trace::{Actor, Reason};
use crate::command::Command;
use crate::ids::{EntityId, PlayerId, TileId, UnitHandle};
use crate::knowledge::observation::LocalObservation;

/// Which orders a variant judges by their effect (`Variant::effect_checks`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct EffectChecks {
    /// A walk that left the unit where it stood is a failed attempt.
    pub moves: bool,
    /// Workers are wanted for the jobs the job finder offers.
    pub demand: bool,
    /// Jobs and work-boat deposits are ranked and kept by the unit's path.
    pub jobs: bool,
}

impl EffectChecks {
    pub const NONE: EffectChecks = EffectChecks { moves: false, demand: false, jobs: false };
    pub const ALL: EffectChecks = EffectChecks { moves: true, demand: true, jobs: true };
}

/// A walk order: a march or a move to `destination`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Walk {
    march: bool,
    destination: TileId,
}

impl Walk {
    /// The walk of `unit` the command orders, if any.
    fn of(command: &Command, unit: UnitHandle) -> Option<Walk> {
        match *command {
            Command::MoveUnit { unit: walker, destination } if walker == unit => Some(Walk { march: false, destination }),
            Command::MarchTo { unit: walker, destination } if walker == unit => Some(Walk { march: true, destination }),
            _ => None,
        }
    }
}

/// A walk sent this turn: who sent it, from where.
#[derive(Debug, Clone, Copy)]
struct Sent {
    player: PlayerId,
    unit: EntityId,
    from: TileId,
    walk: Walk,
}

/// The walks sent in the current turns whose effect the unit's next
/// decision judges.
#[derive(Debug, Default)]
pub struct Walks(Mutex<Vec<Sent>>);

impl Walks {
    /// The start of `player`'s turn: the walks of its last turn are judged
    /// no more (a march goes on by itself between turns).
    pub fn begin_turn(&self, player: PlayerId) {
        self.lock().retain(|s| s.player != player);
    }

    /// The decision of the unit of `near`, its last walk of the turn judged
    /// first: a walk that left the unit on its tile counts against its task
    /// before `decide` runs, and the same walk from the same tile is not
    /// ordered again (the unit's turn ends). The walk decided is remembered.
    pub fn judged(&self, near: &LocalObservation<'_>, state: &mut AiState, dropped: &mut u64, decide: impl FnOnce(&mut AiState) -> Decision) -> Decision {
        let unit = near.unit;
        let stalled = self.take(near.player, unit.id).filter(|s| s.from == unit.tile).map(|s| s.walk);
        if stalled.is_some() && state.note_failure(unit.id) {
            *dropped += 1;
        }
        let decision = decide(state);
        // A front may decide for another of its members: only the unit's own
        // walk is judged at its next decision.
        let walk = decision.command.as_ref().and_then(|c| Walk::of(c, unit.handle));
        if walk.is_some() && walk == stalled {
            return Decision::refuse(Actor::Unit(unit.id), Reason::Blocked);
        }
        if let Some(walk) = walk {
            self.lock().push(Sent { player: near.player, unit: unit.id, from: unit.tile, walk });
        }
        decision
    }

    fn take(&self, player: PlayerId, unit: EntityId) -> Option<Sent> {
        let mut sent = self.lock();
        let at = sent.iter().position(|s| s.player == player && s.unit == unit)?;
        Some(sent.swap_remove(at))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Sent>> {
        self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}
