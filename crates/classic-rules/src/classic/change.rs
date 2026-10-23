//! Changes of the classic module's state in the change journal
//! (`ChangeJournal::module_changes`): the mechanic systems write them;
//! derived yields, knowledge and the client's presentation read them.

use crate::ids::{CityHandle, PlayerId};
use crate::world::module::{ModuleChange, StaleYields};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassicChange {
    /// The city's religious followers or its religion's beliefs changed
    /// (belief effects on its yields may change).
    ReligionChanged { city: CityHandle },
    /// A great work went into one of the city's slots (its culture
    /// changes).
    GreatWorksChanged { city: CityHandle },
    /// A trade route of the city started, ended or changed its values (its
    /// yields change).
    TradeChanged { city: CityHandle },
    /// A golden age started or ended (tile gold and city production of the
    /// player change).
    GoldenAgeChanged { player: PlayerId },
    /// The player earned a Great General (Carthage's units cross mountains
    /// from then on; the unit sources of the player changed).
    GeneralBorn { player: PlayerId },
    /// A unit of the player entered Ancient Ruins on the tile and took the
    /// reward (the ruins are gone).
    RuinsExplored { player: PlayerId, tile: crate::ids::TileId, reward: crate::systems::ruins::Reward },
    /// The unhappiness penalty of the player's cities changed (production
    /// and gold of every city of the player).
    HappinessChanged { player: PlayerId },
    /// The player adopted a policy or unlocked a branch (policy effects of
    /// its cities may change).
    PolicyAdopted { player: PlayerId },
    /// Treaties, embassies, friendships, deals or offers of the player
    /// changed (no derived system reads them).
    RelationsChanged { player: PlayerId },
    /// The city took `damage` hit points in a fight (melee, a ranged or air
    /// attack, a nuke). No derived system reads city damage; the
    /// presentation shows the number on the city.
    CityDamaged { city: CityHandle, damage: u16 },
}

impl ModuleChange for ClassicChange {
    fn stale_yields(&self) -> StaleYields {
        match *self {
            ClassicChange::ReligionChanged { city }
            | ClassicChange::GreatWorksChanged { city }
            | ClassicChange::TradeChanged { city } => StaleYields::City(city),
            ClassicChange::GoldenAgeChanged { player }
            | ClassicChange::HappinessChanged { player }
            | ClassicChange::PolicyAdopted { player } => StaleYields::Player(player),
            ClassicChange::GeneralBorn { .. } | ClassicChange::RuinsExplored { .. } | ClassicChange::RelationsChanged { .. } | ClassicChange::CityDamaged { .. } => {
                StaleYields::Nothing
            }
        }
    }

    fn changes_unit_sources(&self) -> Option<PlayerId> {
        match *self {
            ClassicChange::PolicyAdopted { player }
            | ClassicChange::GoldenAgeChanged { player }
            | ClassicChange::GeneralBorn { player } => Some(player),
            _ => None,
        }
    }
}
