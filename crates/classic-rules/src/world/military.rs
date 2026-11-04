//! Per-player military state of the combat system: great general and great
//! admiral points from combat experience (classic/docs/classic/rules.md).
//! Authoritative: persisted and hashed.

use crate::ids::PlayerId;
pub use crate::systems::airlift::AirliftBlock;
pub use crate::systems::combat::air::{can_air_sweep, AirOrderBlock};
pub use crate::systems::paradrop::{paradrop_range, paradrop_status, ParadropBlock};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PlayerMilitary {
    /// Points towards the next great general (land combat).
    pub general_points: u32,
    /// Points towards the next great admiral (naval combat).
    pub admiral_points: u32,
    pub generals_born: u16,
    pub admirals_born: u16,
}

impl PlayerMilitary {
    pub fn is_default(&self) -> bool {
        *self == PlayerMilitary::default()
    }
}

#[derive(Debug, Clone, Default)]
pub struct MilitaryState {
    players: Vec<PlayerMilitary>,
}

impl MilitaryState {
    pub fn new(players: usize) -> Self {
        Self {
            players: vec![PlayerMilitary::default(); players],
        }
    }

    pub fn of(&self, player: PlayerId) -> PlayerMilitary {
        self.players
            .get(player.0 as usize)
            .copied()
            .unwrap_or_default()
    }

    pub fn all(&self) -> &[PlayerMilitary] {
        &self.players
    }

    pub(crate) fn get_mut(&mut self, player: PlayerId) -> Option<&mut PlayerMilitary> {
        self.players.get_mut(player.0 as usize)
    }

    pub(crate) fn from_parts(players: Vec<PlayerMilitary>) -> Self {
        Self { players }
    }
}
