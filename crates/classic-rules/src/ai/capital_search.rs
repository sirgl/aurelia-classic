//! The leaders' search for the rival capitals they have not found
//! (classic/docs/ai-leaders.md "Capital search").
//!
//! Why: the domination victory takes every original capital, and a war
//! operation aims only at a city the player has seen. On the duel suite
//! the leaders knew the rival's capital in 12 % of their evaluations of
//! the capture that wins the game: their scouts fill the land next to home,
//! die to barbarians or walk home to heal, and scouting ends at round 150.
//! A leader that values a capital it has not found searches for it:
//!
//! - its scouts head away from home and along the known borders of the
//!   capital's holder, where the city must lie, and never onto the ground
//!   of a known barbarian camp (`CapitalSearch::pull`);
//! - scouting goes on after the early game, a recon unit is built whenever
//!   none lives (a warrior on a scout's task does not count), and a hurt
//!   scout heals where it stands when no enemy is near (`planner_units`,
//!   `planner_cities`).
//!
//! Only the player's knowledge: the victory progress screen says which
//! capitals exist and who holds them, the known map shows the holder's
//! borders where the player saw them; no capital's tile is read before the
//! player explored it.

use crate::ai::grand_strategy::{path_of, VictoryPath};
use crate::ai::planner::TurnBrief;
use crate::ai::state::AiState;
use crate::ids::{PlayerId, TileId};
use crate::knowledge::observation::AIObservation;
use crate::knowledge::public::{CapitalStanding, PlayerQueries};
use crate::math::MapDimensions;
use crate::rules::content::Personality;
use crate::world::victory::VictoryKind;

/// Score of a frontier tile per tile it lies farther from our capital (a
/// revealed tile scores 10): the scouts head away from home.
pub const OUTWARD_PER_TILE: i32 = 10;
/// Score of a frontier tile this close to a known tile of a holder of a
/// capital we look for: the scouts walk its borders.
pub const LEAD_BONUS: i32 = 150;
pub const LEAD_RADIUS: i32 = 3;
/// A frontier tile this close to a known barbarian camp is no target: the
/// scouts that chased one died there or walked home to heal again and
/// again.
pub const CAMP_RADIUS: i32 = 2;

/// Where a leader looks for the capitals it values and has not found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapitalSearch {
    /// Our capital: the search heads away from it.
    home: TileId,
    /// Known tiles of the capitals' holders next to unknown ones: the
    /// unseen land behind them holds the capitals.
    leads: Vec<TileId>,
    /// Known barbarian camps.
    camps: Vec<TileId>,
}

impl CapitalSearch {
    /// The search of a leader this turn; None when it values no capital it
    /// has not found (or has no city yet).
    pub fn of(obs: &AIObservation<'_>, brief: &TurnBrief, state: &AiState, personality: &Personality) -> Option<Self> {
        if !obs.victories.enabled(VictoryKind::Domination) {
            return None;
        }
        let capitals = obs.view.original_capitals();
        let holders = sought_holders(&capitals, brief, state, personality);
        if holders.is_empty() {
            return None;
        }
        let home = obs.view.own_capital().and_then(|c| obs.own_cities.iter().find(|o| o.handle == c)).map(|c| c.tile)?;
        let (leads, camps) = known_marks(obs, &holders);
        Some(Self { home, leads, camps })
    }

    /// What a frontier tile is worth to the search beyond what it reveals;
    /// None: no target (a camp's ground).
    pub fn pull(&self, dims: MapDimensions, tile: TileId) -> Option<i32> {
        let near = |marks: &[TileId], radius: i32| marks.iter().any(|m| dims.distance(*m, tile).is_some_and(|d| d <= radius));
        if near(&self.camps, CAMP_RADIUS) {
            return None;
        }
        let outward = dims.distance(self.home, tile).unwrap_or(0) * OUTWARD_PER_TILE;
        Some(outward + if near(&self.leads, LEAD_RADIUS) { LEAD_BONUS } else { 0 })
    }
}

/// The holders of the capitals the leader values and has not found: the
/// last rival capital always (its capture wins the game), every one on the
/// domination path, and a leader leaning to war wants the capitals of its
/// enemies.
fn sought_holders(capitals: &[CapitalStanding], brief: &TurnBrief, state: &AiState, personality: &Personality) -> Vec<PlayerId> {
    let rivals: Vec<&CapitalStanding> = capitals.iter().filter(|c| c.holder != brief.player).collect();
    let all_valued = rivals.len() == 1 || path_of(state) == VictoryPath::Domination;
    let warlike = personality.aggression > Personality::MIDDLE;
    let mut holders: Vec<PlayerId> = rivals
        .into_iter()
        .filter(|c| c.tile.is_none() && (all_valued || (warlike && brief.at_war(c.holder))))
        .map(|c| c.holder)
        .collect();
    holders.sort_unstable();
    holders.dedup();
    holders
}

/// One pass over the known map: the known tiles of the `holders` next to
/// unknown ones, and the known barbarian camps.
fn known_marks(obs: &AIObservation<'_>, holders: &[PlayerId]) -> (Vec<TileId>, Vec<TileId>) {
    let view = &obs.view;
    let camp = crate::systems::barbarians::camp_improvement(view.rules());
    let (mut leads, mut camps) = (Vec::new(), Vec::new());
    for tile in (0..view.tile_count() as u32).map(TileId) {
        let Some(known) = view.tile(tile).snapshot().copied() else { continue };
        if known.improvement.is_some() && known.improvement == camp {
            camps.push(tile);
        } else if known.owner.is_some_and(|o| holders.contains(&o)) && crate::ai::planner_map::is_frontier(view, tile) {
            leads.push(tile);
        }
    }
    (leads, camps)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pull grows with the distance from home, a lead adds its bonus
    /// within its radius, a camp's ground is left alone.
    #[test]
    fn the_search_pulls_away_from_home_and_to_the_leads() {
        let dims = MapDimensions { width: 20, height: 20 };
        let tile = |x: u32, y: u32| TileId(y * dims.width + x);
        let search = CapitalSearch { home: tile(2, 2), leads: vec![tile(12, 2)], camps: vec![tile(2, 10)] };
        assert_eq!(search.pull(dims, tile(2, 2)), Some(0));
        assert_eq!(search.pull(dims, tile(5, 2)), Some(3 * OUTWARD_PER_TILE));
        assert_eq!(search.pull(dims, tile(9, 2)), Some(7 * OUTWARD_PER_TILE + LEAD_BONUS));
        assert_eq!(search.pull(dims, tile(8, 2)), Some(6 * OUTWARD_PER_TILE));
        assert_eq!(search.pull(dims, tile(3, 9)), None, "a camp's ground is no target");
    }
}
