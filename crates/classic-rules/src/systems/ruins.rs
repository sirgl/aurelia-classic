//! Ancient Ruins (reduced): the first unit
//! of a civilization that enters a tile with the ruins takes their reward
//! and the ruins are gone. Rewards are mechanics the module already has: a
//! citizen in the nearest city, culture, faith, gold, experience for the
//! unit, the map around the ruins. No reward harms (every unit is as if it
//! had `NoBadGoodies`).
//!
//! ```text
//! AI       the ruins offer three different rewards from the combat stream when
//!          the unit enters; it takes the first, or the best of the three for a
//!          unit that chooses (`ChoosesRuinsReward`, the Native Tongue promotion)
//! human    entering gives nothing: the ruins stay, and the player claims one of
//!          the three rewards the place offers (`derived::ruins`) with
//!          `Command::ClaimRuins`, a unit standing on them
//! no city  the citizen is gold instead
//! ```
//!
//! The reward table is small and unweighted (Unciv's `Ruins.json` has more
//! kinds: free units, a technology, an upgrade, camp reveals); each number
//! below says how it compares with Unciv. The generator lays the
//! ruins (`scenario::ruins`).

use crate::classic::ClassicChange;
use crate::ids::{PlayerId, TileId, UnitHandle};
use crate::rules::content::ImprovementKind;
use crate::rules::CompiledRuleset;
use crate::transaction::journal::{PlayerChange, TileChange};
use crate::transaction::transaction::WorldTransaction;

/// Gold of the gold reward (and of a citizen without a city). Own value
/// (unverified): Unciv `Ruins.json` gives a random 50..100; a fixed amount is
/// kept because the claim dialog shows the exact number.
pub const GOLD: u32 = 60;
/// Culture of the culture reward. Unciv: `Ruins.json` ("discover cultural
/// artifacts", 20 at standard speed); equal.
pub const CULTURE: u32 = 20;
/// Faith of the faith reward. Own value (unverified): Unciv `Ruins.json` gives
/// the faith of a pantheon (and a third of a prophet after one), which
/// depends on the game; a fixed amount is kept for the claim dialog.
pub const FAITH: u32 = 30;
/// Experience of the unit. Unciv: `Ruins.json` ("your exploring unit receives
/// training", 10 XP); equal (Unciv offers it only from turn 10 and not to
/// civilians).
pub const EXPERIENCE: u16 = 10;
/// Tiles around the ruins the map reward shows. Unciv: `Ruins.json` ("find a
/// crudely-drawn map", radius 4); equal in radius (Unciv centres the disc on a
/// random tile 4 away and reveals each tile with 80 % chance).
pub const MAP_RADIUS: u8 = 4;

/// A reward of the ruins, best first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Reward {
    Citizen,
    Culture,
    Faith,
    Gold,
    Experience,
    Map,
}

/// Every reward, best first (the order the chooser prefers).
pub const REWARDS: [Reward; 6] =
    [Reward::Citizen, Reward::Culture, Reward::Faith, Reward::Gold, Reward::Experience, Reward::Map];

/// How many rewards the ruins offer.
pub const OFFERS: usize = 3;

impl<'a> WorldTransaction<'a> {
    /// A unit of `player` entered `tile`: ruins there give their reward.
    pub(crate) fn enter_ruins(&mut self, player: PlayerId, unit: UnitHandle, tile: TileId, rules: &CompiledRuleset) {
        if !self.world.players.is_major(player) {
            return;
        }
        let Some(ruins) = self.world.map.improvement(tile).flatten() else {
            return;
        };
        if rules.improvements.get(ruins.0 as usize).is_none_or(|d| d.kind != ImprovementKind::AncientRuins) {
            return;
        }
        // A human player picks the reward (`claim_ruins`); the ruins wait for the claim.
        if !self.world.settings.is_ai(player) {
            return;
        }
        self.world.map.set_improvement(tile, None);
        self.journal.tile_changes.push(TileChange::ImprovementBuilt { tile });
        let offers = self.offers();
        let chooses = self.world.units.get(unit).is_some_and(|v| {
            crate::effects::unit::own_ability(
                rules,
                v.core,
                crate::rules::effects::UnitAbility::Module(crate::classic::effects::ClassicUnitAbility::ChoosesRuinsReward),
            )
        });
        let best = offers.iter().copied().min_by_key(|r| REWARDS.iter().position(|x| x == r));
        let reward = if chooses { best } else { offers.first().copied() };
        if let Some(reward) = reward {
            let given = self.give_reward(player, unit, tile, reward);
            self.journal.module_changes.push(ClassicChange::RuinsExplored { player, tile, reward: given });
        }
    }

    /// A unit of a human player claims `reward` from the ruins it stands on (validated:
    /// `derived::ruins::claim_status`). The ruins are gone and the journal names the reward given.
    pub(crate) fn claim_ruins(&mut self, unit: UnitHandle, tile: TileId, reward: Reward) -> Result<(), crate::command::CommandError> {
        let player = self.world.units.get(unit).map(|u| u.core.owner).ok_or(crate::command::CommandError::UnknownUnit)?;
        self.world.map.set_improvement(tile, None);
        self.journal.tile_changes.push(TileChange::ImprovementBuilt { tile });
        let given = self.give_reward(player, unit, tile, reward);
        self.journal.module_changes.push(ClassicChange::RuinsExplored { player, tile, reward: given });
        Ok(())
    }

    /// Three different rewards, drawn from the combat stream.
    fn offers(&mut self) -> Vec<Reward> {
        let mut left = REWARDS.to_vec();
        let mut offers = Vec::with_capacity(OFFERS);
        while offers.len() < OFFERS {
            let at = self.world.rng.combat.below(left.len() as u64) as usize;
            offers.push(left.remove(at));
        }
        offers
    }

    /// Give `reward`; the reward actually given (a citizen without a city is gold).
    fn give_reward(&mut self, player: PlayerId, unit: UnitHandle, tile: TileId, reward: Reward) -> Reward {
        match reward {
            Reward::Citizen => match self.nearest_city(player, tile) {
                Some(city) => self.grow_by(city, 1),
                None => {
                    self.world.players.earn(player, GOLD);
                    return Reward::Gold;
                }
            },
            Reward::Culture => {
                if let Some(state) = self.world.module.culture.player_mut(player) {
                    state.culture = state.culture.saturating_add(CULTURE);
                    state.lifetime = state.lifetime.saturating_add(u64::from(CULTURE));
                }
            }
            Reward::Faith => {
                if let Some(state) = self.world.module.religion.player_mut(player) {
                    state.faith = state.faith.saturating_add(FAITH);
                }
            }
            Reward::Gold => self.world.players.earn(player, GOLD),
            Reward::Experience => self.grant_experience(unit, EXPERIENCE),
            Reward::Map => self.journal.player_changes.push(PlayerChange::AreaRevealed { player, tile, radius: MAP_RADIUS }),
        }
        reward
    }
}
