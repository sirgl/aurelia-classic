//! The congress lobby (W2, W3, W4) and the city-state poacher (CS2-CS5, S3, CS1). STUB: to be written
//! (see the brief).

use std::collections::BTreeMap;

use classic_rules::ids::PlayerId;
use classic_rules::{CompiledRuleset, GameState};
use serde::{Deserialize, Serialize};

use super::{Cast, Trick};
use crate::stress::position::Position;
use crate::stress::scene::{Edit, Plan};
use crate::stress::seat::Seat;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lobby {
    pub seat: u8,
    pub target: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Poach {
    pub seat: u8,
    pub victim: u8,
}

pub struct LobbyPlay {
    order: Lobby,
}

impl LobbyPlay {
    pub fn new(order: Lobby) -> Self {
        LobbyPlay { order }
    }
}

impl Seat for LobbyPlay {
    fn player(&self) -> PlayerId {
        PlayerId(self.order.seat)
    }

    fn play(&mut self, _game: &mut GameState, _rules: &CompiledRuleset) {}

    fn counts(&self) -> BTreeMap<String, f64> {
        BTreeMap::new()
    }
}

pub struct PoachPlay {
    order: Poach,
}

impl PoachPlay {
    pub fn new(order: Poach) -> Self {
        PoachPlay { order }
    }
}

impl Seat for PoachPlay {
    fn player(&self) -> PlayerId {
        PlayerId(self.order.seat)
    }

    fn play(&mut self, _game: &mut GameState, _rules: &CompiledRuleset) {}

    fn counts(&self) -> BTreeMap<String, f64> {
        BTreeMap::new()
    }
}

pub(super) fn plan(trick: Trick, _rules: &CompiledRuleset, _position: &Position, _instance: usize) -> Result<Plan, String> {
    Err(format!("{} is not built yet", trick.name()))
}

pub(super) fn edit_of(cast: Cast, _level: usize, _rules: &CompiledRuleset, _position: &Position) -> Result<Edit, String> {
    Err(format!("{} is not built yet", cast.trick.name()))
}
