//! The choice at Ancient Ruins (read-only): which three rewards a ruin offers a player and
//! whether a unit may claim one now. A human player picks (`Command::ClaimRuins`); the offers
//! are a property of the place, so a reloaded game shows the same three. An AI player takes its
//! reward when it enters (`systems::ruins`), from the combat stream.
//!
//! ```text
//! offers   three different rewards, drawn from a hash of the ruin's tile and the map size
//! claim    a unit of a nation stands on the tile and the reward is one of the offers
//! ```

use crate::ids::{PlayerId, TileId, UnitHandle};
use crate::rules::content::ImprovementKind;
use crate::rules::CompiledRuleset;
use crate::systems::ruins::{Reward, OFFERS, REWARDS};
use crate::world::state::AuthWorld;

/// Why a claim is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuinsBlock {
    UnknownUnit,
    /// Only nations explore ruins.
    NotANation,
    /// The unit does not stand on Ancient Ruins.
    NoRuinsHere,
    /// The reward is not one of the three the ruins offer.
    NotAnOffer,
}

impl RuinsBlock {
    pub const ALL: [RuinsBlock; 4] =
        [RuinsBlock::UnknownUnit, RuinsBlock::NotANation, RuinsBlock::NoRuinsHere, RuinsBlock::NotAnOffer];

    pub fn reason(self) -> &'static str {
        match self {
            RuinsBlock::UnknownUnit => "unknown unit",
            RuinsBlock::NotANation => "only a nation explores ruins",
            RuinsBlock::NoRuinsHere => "no ancient ruins here",
            RuinsBlock::NotAnOffer => "the ruins do not offer that",
        }
    }
}

impl From<RuinsBlock> for crate::command::CommandError {
    fn from(block: RuinsBlock) -> Self {
        crate::command::CommandError::RuleViolation(block.reason().into())
    }
}

/// Ancient Ruins stand on `tile`.
pub fn ruins_on(auth: &AuthWorld, rules: &CompiledRuleset, tile: TileId) -> bool {
    auth.map()
        .improvement(tile)
        .flatten()
        .and_then(|i| rules.improvements.get(i.0 as usize))
        .is_some_and(|d| d.kind == ImprovementKind::AncientRuins)
}

/// The three rewards the ruins on `tile` offer a player, best first.
pub fn offers_at(auth: &AuthWorld, tile: TileId) -> [Reward; OFFERS] {
    let map = auth.map();
    let mut state = (u64::from(tile.0) << 20) ^ map.tile_count() as u64 ^ 0x5EED_0F_A5_u64;
    let mut left = REWARDS.to_vec();
    let mut picked = Vec::with_capacity(OFFERS);
    while picked.len() < OFFERS {
        let at = (mix(&mut state) % left.len() as u64) as usize;
        picked.push(left.remove(at));
    }
    picked.sort_by_key(|r| REWARDS.iter().position(|x| x == r));
    [picked[0], picked[1], picked[2]]
}

/// splitmix64.
fn mix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// The ruins a unit of `player` stands on (None: none, or the unit is not the player's).
pub fn site_of(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId, unit: UnitHandle) -> Option<TileId> {
    let core = auth.units().get(unit)?.core;
    (core.owner == player && auth.players().is_major(player) && ruins_on(auth, rules, core.tile)).then_some(core.tile)
}

/// May `unit` claim `reward` now? Ok(the ruins' tile).
pub fn claim_status(auth: &AuthWorld, rules: &CompiledRuleset, unit: UnitHandle, reward: Reward) -> Result<TileId, RuinsBlock> {
    let core = auth.units().get(unit).ok_or(RuinsBlock::UnknownUnit)?.core;
    if !auth.players().is_major(core.owner) {
        return Err(RuinsBlock::NotANation);
    }
    if !ruins_on(auth, rules, core.tile) {
        return Err(RuinsBlock::NoRuinsHere);
    }
    if !offers_at(auth, core.tile).contains(&reward) {
        return Err(RuinsBlock::NotAnOffer);
    }
    Ok(core.tile)
}

/// The first unit of `player` that stands on ruins, with the tile.
pub fn first_site(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId) -> Option<(UnitHandle, TileId)> {
    auth.indexes()
        .units_of(player)
        .iter()
        .find_map(|u| site_of(auth, rules, player, *u).map(|t| (*u, t)))
}
