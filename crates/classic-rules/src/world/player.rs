//! The classic instance of `kernel::world::player`, with the classic kinds of
//! player: city-states and the barbarians.

pub use kernel::world::player::*;

use crate::classic::Classic;
use crate::ids::PlayerId;
use crate::rules::content::CityStateId;
use crate::rules::CompiledRuleset;

pub type PlayerKind = kernel::world::player::PlayerKind<Classic>;
pub type PlayerCore = kernel::world::player::PlayerCore<Classic>;
pub type PlayerStore = kernel::world::player::PlayerStore<Classic>;

/// The kinds of player besides the major civilizations. Their rules
/// differ: city-states have influence and no diplomacy screen, barbarians
/// are always at war and have no diplomacy at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClassicKind {
    CityState(CityStateId),
    Barbarian,
}

impl ModulePlayerKind<Classic> for ClassicKind {
    /// Save codes 1 (city-state, its id) and 2 (barbarians).
    fn code(self) -> (u8, u16) {
        match self {
            ClassicKind::CityState(id) => (1, id.0),
            ClassicKind::Barbarian => (2, 0),
        }
    }

    fn from_code(tag: u8, id: u16, rules: &CompiledRuleset) -> Option<Self> {
        match tag {
            1 if rules.module.city_states.get(id as usize).is_some() => Some(ClassicKind::CityState(CityStateId(id))),
            2 => Some(ClassicKind::Barbarian),
            _ => None,
        }
    }

    /// A city-state's cities take its name.
    fn city_name(self, rules: &CompiledRuleset) -> Option<String> {
        let ClassicKind::CityState(id) = self else {
            return None;
        };
        let name = rules.module.city_states.get(id.0 as usize).map(|c| c.name.trim().to_string());
        Some(name.unwrap_or_else(|| "Settlement".into()))
    }

    fn always_at_war(self) -> bool {
        self == ClassicKind::Barbarian
    }

    fn trains_founders(self) -> bool {
        !matches!(self, ClassicKind::CityState(_))
    }

    /// Feitorias stand on city-state land.
    fn hosts_improvements(self) -> bool {
        matches!(self, ClassicKind::CityState(_))
    }

    /// Every civilization's units may enter a city-state's land
    /// (classic/docs/classic/rules.md).
    fn open_territory(self) -> bool {
        matches!(self, ClassicKind::CityState(_))
    }
}

/// The kinds of the players of a game.
pub trait ClassicPlayers {
    fn is_barbarian(&self, p: PlayerId) -> bool;
    fn is_city_state(&self, p: PlayerId) -> bool;
    /// The city-state a player is (None: not a city-state).
    fn city_state_of(&self, p: PlayerId) -> Option<CityStateId>;
    /// The barbarian player, if the game has one.
    fn barbarian(&self) -> Option<PlayerId>;
}

impl ClassicPlayers for PlayerStore {
    fn is_barbarian(&self, p: PlayerId) -> bool {
        self.kind_of(p) == PlayerKind::Module(ClassicKind::Barbarian)
    }

    fn is_city_state(&self, p: PlayerId) -> bool {
        self.city_state_of(p).is_some()
    }

    fn city_state_of(&self, p: PlayerId) -> Option<CityStateId> {
        match self.kind_of(p) {
            PlayerKind::Module(ClassicKind::CityState(id)) => Some(id),
            _ => None,
        }
    }

    fn barbarian(&self) -> Option<PlayerId> {
        let index = (0..self.count()).position(|i| self.is_barbarian(PlayerId(i as u8)))?;
        Some(PlayerId(index as u8))
    }
}
