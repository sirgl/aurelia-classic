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
    /// A unit of the player entered a barbarian camp on the tile and cleared it
    /// (the camp is gone). `gold` is what the treasury received: the camp gold
    /// at the game's speed and the player's plunder percent, none for a
    /// city-state.
    CampCleared { player: PlayerId, tile: crate::ids::TileId, gold: u32 },
    /// A barbarian unit killed inside its camp on the tile joined the player (Furor
    /// Teutonicus) and brought `gold`. Written by the kill, so before the
    /// `CampCleared` of the same camp, in the same command or an earlier one.
    CampUnitConverted { player: PlayerId, tile: crate::ids::TileId, gold: u32 },
    /// A kill of the player's unit on the tile paid it `amount` of `kind` (an effect of a policy,
    /// belief, nation, technology or building: Honor's culture for a barbarian, gold for every
    /// kill). `amount` is what the stock received; `barbarian` tells that the victim was one.
    KillYield {
        player: PlayerId,
        tile: crate::ids::TileId,
        kind: crate::rules::effects::YieldKind,
        amount: u32,
        source: crate::rules::effects::SourceRef,
        barbarian: bool,
    },
    /// The player found the natural wonder on the tile (its find is settled at the end of the
    /// player's turn). `gold` is what the first finder got (0 for a later finder) and `happiness`
    /// the happiness one found wonder adds (with the player's stat percent); what effects paid
    /// for the find (Spain) follows as `WonderYield` changes before this one.
    NaturalWonderFound {
        player: PlayerId,
        tile: crate::ids::TileId,
        wonder: crate::rules::content::NaturalWonderId,
        first: bool,
        gold: u32,
        happiness: i32,
    },
    /// A yield an effect paid for finding the natural wonder on the tile (Spain's gold).
    WonderYield {
        player: PlayerId,
        tile: crate::ids::TileId,
        kind: crate::rules::effects::YieldKind,
        amount: u32,
        source: crate::rules::effects::SourceRef,
    },
    /// The player took `gold` by force on the tile: from a pillaged improvement, a captured city
    /// (after the plunder percents of effects) or a plundered trade route. The gold of a barbarian
    /// camp is `CampCleared`'s, never this.
    Plundered { player: PlayerId, tile: crate::ids::TileId, source: crate::PlunderSource, gold: u32 },
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
            ClassicChange::GeneralBorn { .. }
            | ClassicChange::RuinsExplored { .. }
            | ClassicChange::CampCleared { .. }
            | ClassicChange::CampUnitConverted { .. }
            | ClassicChange::KillYield { .. }
            | ClassicChange::Plundered { .. }
            | ClassicChange::NaturalWonderFound { .. }
            | ClassicChange::WonderYield { .. }
            | ClassicChange::RelationsChanged { .. }
            | ClassicChange::CityDamaged { .. } => {
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
