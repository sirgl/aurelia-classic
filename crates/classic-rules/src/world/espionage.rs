//! Espionage state (classic/docs/classic/rules.md): every civilization's spies
//! (rank, state, city, progress), the technologies it may steal from each
//! other civilization, which eras already granted spies. Authoritative:
//! persisted, hashed once a spy exists.

use crate::ids::{CityHandle, PlayerId};

/// The rules over this state (defined by the espionage system).
pub use crate::systems::espionage::{
    coup_chance, detection, enemy_steal_percent, intel_rate, move_problem, player_stat, rigged_votes,
    starting_rank, stealable, Detection, SpyBlock, ELECTION_ROUNDS,
};

/// The name of the `index`-th spy a civilization has had: its list in turn, numbered when the
/// list has gone round ("Nathan Hale", ..., "Nathan Hale 2"). A civilization without a list
/// numbers its spies.
pub fn spy_name(rules: &crate::CompiledRuleset, civ: u8, index: u8) -> String {
    let list = rules.civs.get(usize::from(civ)).map_or(&[][..], |c| c.module.spy_names.as_slice());
    if list.is_empty() {
        return format!("Spy {}", u32::from(index) + 1);
    }
    let base = &list[usize::from(index) % list.len()];
    match usize::from(index) / list.len() {
        0 => base.clone(),
        round => format!("{base} {}", round + 1),
    }
}

/// The name index a spy new to a civilization takes: one past the highest its spies carry.
pub(crate) fn next_name_in(spies: &[Spy]) -> u8 {
    spies.iter().map(|s| s.name).max().map_or(0, |highest| highest.wrapping_add(1))
}

/// The state of a spy (rules.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SpyState {
    #[default]
    Unassigned,
    Travelling,
    Surveillance,
    GatheringIntel,
    RigElection,
    CounterIntel,
    MakingIntroductions,
    Schmooze,
    Dead,
}

impl SpyState {
    pub const ALL: [SpyState; 9] = [
        SpyState::Unassigned,
        SpyState::Travelling,
        SpyState::Surveillance,
        SpyState::GatheringIntel,
        SpyState::RigElection,
        SpyState::CounterIntel,
        SpyState::MakingIntroductions,
        SpyState::Schmooze,
        SpyState::Dead,
    ];

    pub fn tag(self) -> u8 {
        SpyState::ALL.iter().position(|s| *s == self).unwrap_or(0) as u8
    }

    /// Surveillance is established once the
    /// state moved past it.
    pub fn established(self) -> bool {
        matches!(
            self,
            SpyState::GatheringIntel | SpyState::RigElection | SpyState::Schmooze
        )
    }
}

/// Highest spy rank (0 = recruit .. 2 = special agent).
/// Unciv: models/ModConstants.kt (`maxSpyRank` 3: three ranks), equal.
pub const MAX_RANK: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Spy {
    /// Index into the civilization's spy names (cycling).
    pub name: u8,
    pub rank: u8,
    pub state: SpyState,
    pub city: Option<CityHandle>,
    /// Progress of the state: amount, per turn, goal (×100 for intel).
    pub amount: i64,
    pub rate: i64,
    pub goal: i64,
    /// Placed as a diplomat (in a capital, at peace).
    pub diplomat: bool,
    /// Turns dead (replacement after 5).
    pub dead_turns: u8,
}

impl Spy {
    /// Surveillance established in its city.
    pub fn has_surveillance(&self) -> bool {
        self.state.established()
            || (self.state == SpyState::Surveillance && self.amount >= self.goal)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EspionageState {
    players: usize,
    pub(crate) spies: Vec<Vec<Spy>>,
    /// Steal rights per (thief × players + victim).
    pub(crate) steal_rights: Vec<u8>,
    /// The Renaissance spy for everyone was granted.
    pub(crate) renaissance_granted: bool,
    /// The latest era index that granted the player spies.
    pub(crate) era_granted: Vec<u8>,
}

impl EspionageState {
    pub fn new(players: usize) -> Self {
        Self {
            players,
            spies: vec![Vec::new(); players],
            steal_rights: vec![0; players * players],
            renaissance_granted: false,
            era_granted: vec![0; players],
        }
    }

    pub fn is_pristine(&self) -> bool {
        *self == Self::new(self.players)
    }

    pub fn spies_of(&self, player: PlayerId) -> &[Spy] {
        self.spies
            .get(player.0 as usize)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    pub(crate) fn spy_mut(&mut self, player: PlayerId, index: usize) -> Option<&mut Spy> {
        self.spies.get_mut(player.0 as usize)?.get_mut(index)
    }

    pub fn steal_rights(&self, thief: PlayerId, victim: PlayerId) -> u8 {
        let i = thief.0 as usize * self.players + victim.0 as usize;
        self.steal_rights.get(i).copied().unwrap_or(0)
    }

    pub(crate) fn set_steal_rights(&mut self, thief: PlayerId, victim: PlayerId, n: u8) {
        let i = thief.0 as usize * self.players + victim.0 as usize;
        if let Some(r) = self.steal_rights.get_mut(i) {
            *r = n;
        }
    }

    /// The spy of `player` in `city`, with its index.
    pub fn spy_in(&self, player: PlayerId, city: CityHandle) -> Option<(usize, &Spy)> {
        self.spies_of(player)
            .iter()
            .enumerate()
            .find(|(_, s)| s.city == Some(city) && s.state != SpyState::Dead)
    }

    /// The name index a spy new to `player` takes: one past the highest in use, so no two living
    /// spies share a name until the list has gone round.
    pub fn next_name(&self, player: PlayerId) -> u8 {
        next_name_in(self.spies_of(player))
    }

    pub fn renaissance_granted(&self) -> bool {
        self.renaissance_granted
    }

    pub fn era_granted(&self) -> &[u8] {
        &self.era_granted
    }

    pub fn all_spies(&self) -> &[Vec<Spy>] {
        &self.spies
    }

    pub fn all_steal_rights(&self) -> &[u8] {
        &self.steal_rights
    }

    pub(crate) fn from_parts(
        players: usize,
        spies: Vec<Vec<Spy>>,
        steal_rights: Vec<u8>,
        renaissance_granted: bool,
        era_granted: Vec<u8>,
    ) -> Self {
        Self {
            players,
            spies,
            steal_rights,
            renaissance_granted,
            era_granted,
        }
    }
}
