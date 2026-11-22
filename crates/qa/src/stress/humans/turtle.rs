//! The turtle (MO11, D18): the score leader keeps almost no army; at the start of each of its turns
//! the script disbands its military units beyond a cap (the garrisons of its cities are kept first).
//! The other nations are judged: does anyone take what a defenceless leader holds.

use std::collections::BTreeMap;

use classic_rules::ids::PlayerId;
use classic_rules::{Command, CompiledRuleset, GameState};
use kernel::ai::plan::{role_of, AiRole};
use serde::{Deserialize, Serialize};

use super::{scripted, Cast, Order, Trick};
use crate::bench::modules::majors;
use crate::stress::position::Position;
use crate::stress::scene::{city_tiles, nearest_on_land, Edit, Plan, Roles};
use crate::stress::seat::Seat;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Turtle {
    pub seat: u8,
    /// Military units kept, as a percent of the turtle's cities.
    pub keep_percent: u32,
}

pub struct Play {
    order: Turtle,
    disbanded: u32,
}

impl Play {
    pub fn new(order: Turtle) -> Self {
        Play { order, disbanded: 0 }
    }
}

impl Seat for Play {
    fn player(&self) -> PlayerId {
        PlayerId(self.order.seat)
    }

    fn play(&mut self, game: &mut GameState, rules: &CompiledRuleset) {
        let turtle = PlayerId(self.order.seat);
        let auth = game.view().auth;
        let keep = auth.indexes().cities_of(turtle).len() * self.order.keep_percent as usize / 100;
        // The garrisons first (a unit in a city), then by id: the units past `keep` go.
        let mut army: Vec<(bool, u64, classic_rules::ids::UnitHandle)> = auth
            .indexes()
            .units_of(turtle)
            .iter()
            .filter_map(|h| auth.units().get(*h).map(|u| (*h, u)))
            .filter(|(_, u)| matches!(role_of(rules, u.core.kind), AiRole::Military | AiRole::Naval))
            .map(|(h, u)| (auth.indexes().city_on_tile(u.core.tile).is_none(), u.core.id.0, h))
            .collect();
        army.sort_unstable_by_key(|(outside, id, _)| (*outside, *id));
        for (_, _, unit) in army.into_iter().skip(keep) {
            if game.execute(rules, Command::DisbandUnit { unit }).is_ok() {
                self.disbanded += 1;
            }
        }
    }

    fn counts(&self) -> BTreeMap<String, f64> {
        BTreeMap::from([("script_disbanded".to_owned(), f64::from(self.disbanded))])
    }
}

/// The turtle is the nation with the most score; `seat` (for the record) is its nearest neighbour.
pub(super) fn plan(rules: &CompiledRuleset, position: &Position, _instance: usize) -> Result<Plan, String> {
    let game = &position.game;
    let auth = game.view().auth;
    let all = majors(game);
    let turtle = all.iter().copied().max_by_key(|p| (classic_rules::world::victory::score(auth, rules, *p), std::cmp::Reverse(p.0))).ok_or("no nations")?;
    let near = all
        .iter()
        .copied()
        .filter(|p| *p != turtle)
        .min_by_key(|p| (city_tiles(game, turtle).into_iter().filter_map(|t| nearest_on_land(game, *p, t)).min().unwrap_or(i32::MAX), p.0))
        .ok_or("the turtle has no rival")?;
    Ok(Plan { roles: Roles::Human(Cast { trick: Trick::Turtle, seat: near.0, human: turtle.0 }), base: Vec::new(), info: BTreeMap::new() })
}

/// Mild: one unit per city kept; medium: one per two cities; strong: none.
pub(super) fn edit_of(cast: Cast, level: usize, _rules: &CompiledRuleset, _position: &Position) -> Result<Edit, String> {
    Ok(scripted(Vec::new(), Order::Turtle(Turtle { seat: cast.human, keep_percent: [100, 50, 0][level] })))
}
