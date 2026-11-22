//! The wonder sniper (V1, E5). STUB: to be written (see the brief).

use std::collections::BTreeMap;

use classic_rules::ids::PlayerId;
use classic_rules::{CompiledRuleset, GameState};
use serde::{Deserialize, Serialize};

use super::Cast;
use crate::stress::position::Position;
use crate::stress::scene::{Edit, Plan};
use crate::stress::seat::Seat;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sniper {
    pub seat: u8,
    pub victim: u8,
    pub rounds_before: u32,
}

pub struct Play {
    order: Sniper,
}

impl Play {
    pub fn new(order: Sniper) -> Self {
        Play { order }
    }
}

impl Seat for Play {
    fn player(&self) -> PlayerId {
        PlayerId(self.order.seat)
    }

    fn play(&mut self, _game: &mut GameState, _rules: &CompiledRuleset) {}

    fn counts(&self) -> BTreeMap<String, f64> {
        BTreeMap::new()
    }
}

pub(super) fn plan(_rules: &CompiledRuleset, _position: &Position, _instance: usize) -> Result<Plan, String> {
    Err("hv1 is not built yet".to_owned())
}

pub(super) fn edit_of(_cast: Cast, _level: usize, _rules: &CompiledRuleset, _position: &Position) -> Result<Edit, String> {
    Err("hv1 is not built yet".to_owned())
}
