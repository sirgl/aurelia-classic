//! Culture and social policies of every player (authoritative: persisted
//! and hashed). The owner is `systems::policies`; everyone else reads.
//!
//! ```text
//! culture        the pool that pays for policies (culture points)
//! lifetime       culture generated over the whole game (the denominator of
//!                tourism influence)
//! recent         culture of the last RECENT_TURNS turns (Great Writer, digs)
//! policies       adopted policies in adoption order (openers, finishers and
//!                tenets included)
//! branches       unlocked branches in unlock order (the ideology included)
//! ideology       the chosen ideology branch
//! free_policies  policies the player may still adopt for free
//! free_tenets    tenets the player may still adopt for free
//! free_used      policies the player received for free (they do not raise
//!                the cost of the next one)
//! max_cities     the most cities the player ever had (policy cost)
//! free_buildings buildings still owed to the next cities founded
//! ```

use crate::ids::{BuildingKindId, PlayerId, PolicyId};
use crate::rules::content::PolicyBranchId;

/// Turns of culture history kept per player (Great Writer, archaeology culture
/// boost). Unciv: `PolicyManager.kt` (`cultureOfLast8Turns`, 8); equal.
pub const RECENT_TURNS: usize = 8;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlayerCulture {
    pub culture: u32,
    pub lifetime: u64,
    /// Culture gained in each of the last turns, newest last (at most
    /// RECENT_TURNS entries).
    pub recent: Vec<u32>,
    pub policies: Vec<PolicyId>,
    pub branches: Vec<PolicyBranchId>,
    pub ideology: Option<PolicyBranchId>,
    pub free_policies: u16,
    pub free_tenets: u16,
    pub free_used: u16,
    pub max_cities: u16,
    /// (building, cities still owed), in grant order.
    pub free_buildings: Vec<(BuildingKindId, u8)>,
}

impl PlayerCulture {
    pub fn has_policy(&self, policy: PolicyId) -> bool {
        self.policies.contains(&policy)
    }

    pub fn has_branch(&self, branch: PolicyBranchId) -> bool {
        self.branches.contains(&branch)
    }

    /// Policies that raise the cost of the next one: all owned minus the free
    /// ones received.
    pub fn paid_policies(&self) -> u32 {
        (self.policies.len() as u32).saturating_sub(u32::from(self.free_used))
    }

    /// Culture of the last `turns` turns (at most RECENT_TURNS).
    pub fn recent_culture(&self, turns: usize) -> u32 {
        let skip = self.recent.len().saturating_sub(turns);
        self.recent.iter().skip(skip).sum()
    }
}

/// Culture state of all players, index = PlayerId.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CultureState {
    players: Vec<PlayerCulture>,
    /// How many players ever adopted each ideology (first and second
    /// adopters get free tenets), in first-adoption order.
    ideology_adopters: Vec<(PolicyBranchId, u8)>,
}

impl CultureState {
    pub fn new(players: usize) -> Self {
        Self {
            players: vec![PlayerCulture::default(); players],
            ideology_adopters: Vec::new(),
        }
    }

    pub fn player(&self, p: PlayerId) -> Option<&PlayerCulture> {
        self.players.get(p.0 as usize)
    }

    /// Culture state of `p`, or the empty state for an unknown player.
    pub fn of(&self, p: PlayerId) -> &PlayerCulture {
        static EMPTY: PlayerCulture = PlayerCulture {
            culture: 0,
            lifetime: 0,
            recent: Vec::new(),
            policies: Vec::new(),
            branches: Vec::new(),
            ideology: None,
            free_policies: 0,
            free_tenets: 0,
            free_used: 0,
            max_cities: 0,
            free_buildings: Vec::new(),
        };
        self.players.get(p.0 as usize).unwrap_or(&EMPTY)
    }

    pub fn players(&self) -> &[PlayerCulture] {
        &self.players
    }

    pub fn ideology_adopters(&self) -> &[(PolicyBranchId, u8)] {
        &self.ideology_adopters
    }

    /// Players who adopted `ideology` before (0 for the first adopter).
    pub fn adopters_of(&self, ideology: PolicyBranchId) -> u8 {
        for (branch, count) in &self.ideology_adopters {
            if *branch == ideology {
                return *count;
            }
        }
        0
    }

    pub(crate) fn player_mut(&mut self, p: PlayerId) -> Option<&mut PlayerCulture> {
        self.players.get_mut(p.0 as usize)
    }

    pub(crate) fn count_adopter(&mut self, ideology: PolicyBranchId) {
        for (branch, count) in self.ideology_adopters.iter_mut() {
            if *branch == ideology {
                *count = count.saturating_add(1);
                return;
            }
        }
        self.ideology_adopters.push((ideology, 1));
    }

    /// Restore from the save schema.
    pub(crate) fn from_parts(
        players: Vec<PlayerCulture>,
        ideology_adopters: Vec<(PolicyBranchId, u8)>,
    ) -> Self {
        Self {
            players,
            ideology_adopters,
        }
    }
}
