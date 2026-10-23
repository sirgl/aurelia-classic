//! Victory state (classic/docs/classic/rules.md): the victory options of the game,
//! completed projects (Apollo, spaceship parts) per player, the World
//! Leader chosen by the United Nations, the outcome and whether the game
//! goes on after it. Authoritative: persisted, hashed once not pristine.

use crate::ids::PlayerId;
use crate::rules::content::ProjectId;

/// The rules over this state (defined by the victory system): the score,
/// the spaceship, the domination count, what may be built.
pub use crate::systems::victory::{
    nukes_allowed,
    capitals_held, foreign_capitals_held, influential_on_all, map_score_mod, project_problem, score, spaceship_parts,
    spaceship_problem, MIN_ROUNDS_TO_TEST,
};

/// The kinds of victory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum VictoryKind {
    /// Highest score when the last round is reached.
    Time,
    /// Every spaceship part installed in the capital.
    Science,
    /// Every original capital held.
    Domination,
    /// Embraced on every other civilization (tourism; track B).
    Cultural,
    /// Elected World Leader by the United Nations.
    Diplomatic,
}

impl VictoryKind {
    pub const ALL: [VictoryKind; 5] = [
        VictoryKind::Time,
        VictoryKind::Science,
        VictoryKind::Domination,
        VictoryKind::Cultural,
        VictoryKind::Diplomatic,
    ];

    pub fn bit(self) -> u8 {
        1 << VictoryKind::ALL
            .iter()
            .position(|k| *k == self)
            .unwrap_or(0)
    }
}

/// The last round of a game at Standard speed.
/// Unciv: Speeds.json (Standard, 500 turns), equal.
pub const STANDARD_MAX_ROUNDS: u32 = 500;

/// The game's victory options (the setup screen).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VictoryOptions {
    /// Bit per `VictoryKind::ALL` position: the victory is off.
    pub disabled: u8,
    /// The time victory round.
    pub max_rounds: u32,
}

impl Default for VictoryOptions {
    fn default() -> Self {
        Self {
            disabled: 0,
            max_rounds: STANDARD_MAX_ROUNDS,
        }
    }
}

impl VictoryOptions {
    pub fn enabled(&self, kind: VictoryKind) -> bool {
        self.disabled & kind.bit() == 0
    }

    /// The options with `kind` switched on or off (the setup screen).
    pub fn with(mut self, kind: VictoryKind, on: bool) -> Self {
        if on {
            self.disabled &= !kind.bit();
        } else {
            self.disabled |= kind.bit();
        }
        self
    }
}

/// Who won, how and when.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Outcome {
    pub winner: PlayerId,
    pub kind: VictoryKind,
    pub round: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VictoryState {
    players: usize,
    projects: usize,
    pub(crate) options: VictoryOptions,
    /// Completed instances per (player, project), row-major.
    pub(crate) project_counts: Vec<u16>,
    pub(crate) world_leader: Option<PlayerId>,
    pub(crate) outcome: Option<Outcome>,
    /// The players chose one more turn after the outcome.
    pub(crate) continued: bool,
}

impl VictoryState {
    pub fn new(players: usize, projects: usize) -> Self {
        Self {
            players,
            projects,
            options: VictoryOptions::default(),
            project_counts: vec![0; players * projects],
            world_leader: None,
            outcome: None,
            continued: false,
        }
    }

    pub fn is_pristine(&self) -> bool {
        *self == Self::new(self.players, self.projects)
    }

    pub fn options(&self) -> VictoryOptions {
        self.options
    }

    pub fn outcome(&self) -> Option<Outcome> {
        self.outcome
    }

    pub fn continued(&self) -> bool {
        self.continued
    }

    pub fn world_leader(&self) -> Option<PlayerId> {
        self.world_leader
    }

    pub fn project_count(&self, player: PlayerId, project: ProjectId) -> u16 {
        let i = player.0 as usize * self.projects + project.0 as usize;
        if player.0 as usize >= self.players || project.0 as usize >= self.projects {
            return 0;
        }
        self.project_counts.get(i).copied().unwrap_or(0)
    }

    pub(crate) fn add_project(&mut self, player: PlayerId, project: ProjectId) {
        if player.0 as usize >= self.players || project.0 as usize >= self.projects {
            return;
        }
        let i = player.0 as usize * self.projects + project.0 as usize;
        self.project_counts[i] = self.project_counts[i].saturating_add(1);
    }

    pub fn project_counts(&self) -> &[u16] {
        &self.project_counts
    }

    pub(crate) fn from_parts(
        players: usize,
        projects: usize,
        options: VictoryOptions,
        project_counts: Vec<u16>,
        world_leader: Option<PlayerId>,
        outcome: Option<Outcome>,
        continued: bool,
    ) -> Self {
        Self {
            players,
            projects,
            options,
            project_counts,
            world_leader,
            outcome,
            continued,
        }
    }
}
