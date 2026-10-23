//! Great works, cultural influence, public opinion and archaeology
//! (authoritative: persisted and hashed). The owner is `systems::tourism`;
//! everyone else reads. Rules: classic/docs/classic/rules.md "Culture, trade,
//! diplomacy and victories", sections 1 and 2.
//!
//! ```text
//! works        every great work ever created, index = GreatWorkId
//! cities       the works in each city's building slots, index = city slot
//! players      influence of each player on every other player, the
//!              lifetime culture of the last turn (influence trend) and
//!              the public opinion of its ideology
//! archaeology  history records of tiles, antiquity sites, digs under way
//!              and dig choices waiting for their player
//! ```

use crate::ids::{BuildingKindId, CityHandle, EntityId, PlayerId, TileId};
use crate::rng::StreamRng;
use crate::rules::content::{GreatWorkSlot, PolicyBranchId};

/// Kind of a great work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GreatWorkClass {
    Art,
    Artifact,
    Literature,
    Music,
}

impl GreatWorkClass {
    /// The slot type that holds works of this class.
    pub fn slot(self) -> GreatWorkSlot {
        match self {
            GreatWorkClass::Art | GreatWorkClass::Artifact => GreatWorkSlot::ArtArtifact,
            GreatWorkClass::Literature => GreatWorkSlot::Literature,
            GreatWorkClass::Music => GreatWorkSlot::Music,
        }
    }
}

/// Index into `TourismState::works`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GreatWorkId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GreatWork {
    pub class: GreatWorkClass,
    /// Era index of its creator when it was created (theming).
    pub era: u8,
    /// Its author: the creator's civilization (theming «same player»).
    pub creator: PlayerId,
}

/// A work in one slot of a city building.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlacedWork {
    pub building: BuildingKindId,
    pub work: GreatWorkId,
}

/// Public opinion of a player's ideology.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PublicOpinion {
    /// No ideology, or the people back it.
    #[default]
    Content,
    Murmurs,
    Unrest,
    Upheaval,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlayerTourism {
    /// Influence on each player (index = PlayerId).
    pub influence: Vec<u64>,
    /// Influence on each player before the last turn (trend).
    pub influence_before: Vec<u64>,
    /// The player's lifetime culture at its last turn (others' trend).
    pub lifetime_before: u64,
    pub opinion: PublicOpinion,
    /// Pressure of the other ideologies over the player's own (0: content).
    pub dissatisfaction: u16,
    /// The ideology the people prefer when not content.
    pub preferred: Option<PolicyBranchId>,
}

impl PlayerTourism {
    pub fn influence_on(&self, other: PlayerId) -> u64 {
        self.influence.get(other.0 as usize).copied().unwrap_or(0)
    }

    pub fn influence_before_on(&self, other: PlayerId) -> u64 {
        self.influence_before
            .get(other.0 as usize)
            .copied()
            .unwrap_or(0)
    }
}

/// What a tile remembers for archaeology.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArchRecord {
    pub kind: ArtifactKind,
    pub era: u8,
    pub first: PlayerId,
    pub second: Option<PlayerId>,
}

/// History events that leave artifacts, by rank (a record is replaced only by a
/// higher one).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ArtifactKind {
    BattleRanged,
    BattleMelee,
    BarbarianCamp,
    AncientRuin,
    RazedCity,
    Writing,
}

impl ArtifactKind {
    pub const ALL: [ArtifactKind; 6] = [
        ArtifactKind::BattleRanged,
        ArtifactKind::BattleMelee,
        ArtifactKind::BarbarianCamp,
        ArtifactKind::AncientRuin,
        ArtifactKind::RazedCity,
        ArtifactKind::Writing,
    ];

    /// Rank of the kind (1..=6).
    pub fn value(self) -> u8 {
        match self {
            ArtifactKind::BattleRanged => 1,
            ArtifactKind::BattleMelee => 2,
            ArtifactKind::BarbarianCamp => 3,
            ArtifactKind::AncientRuin => 4,
            ArtifactKind::RazedCity => 5,
            ArtifactKind::Writing => 6,
        }
    }
}

/// An antiquity site on the map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DigSite {
    pub tile: TileId,
    pub record: ArchRecord,
    /// Hidden site (seen only after the Exploration finisher).
    pub hidden: bool,
}

/// An archaeologist digging a site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dig {
    pub unit: EntityId,
    pub player: PlayerId,
    pub tile: TileId,
    pub turns_left: u16,
}

/// A finished dig waiting for its player's choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingDig {
    pub player: PlayerId,
    pub site: DigSite,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Archaeology {
    /// History of tiles before the sites appear, sorted by tile.
    pub records: Vec<(TileId, ArchRecord)>,
    pub sites: Vec<DigSite>,
    pub digs: Vec<Dig>,
    pub pending: Vec<PendingDig>,
    /// Landmarks built: (tile, era of its record).
    pub landmarks: Vec<(TileId, u8)>,
    /// The sites appeared (first Archaeology in the game).
    pub triggered: bool,
    pub rng: StreamRng,
}

impl Archaeology {
    fn new(seed: u64) -> Self {
        Self {
            records: Vec::new(),
            sites: Vec::new(),
            digs: Vec::new(),
            pending: Vec::new(),
            landmarks: Vec::new(),
            triggered: false,
            rng: StreamRng::new(seed ^ 0xA2C4_E0),
        }
    }

    pub fn site_at(&self, tile: TileId) -> Option<&DigSite> {
        self.sites.iter().find(|s| s.tile == tile)
    }

    pub fn record_at(&self, tile: TileId) -> Option<&ArchRecord> {
        match self.records.binary_search_by_key(&tile, |(t, _)| *t) {
            Ok(i) => Some(&self.records[i].1),
            Err(_) => None,
        }
    }

    pub fn landmark_era(&self, tile: TileId) -> Option<u8> {
        self.landmarks
            .iter()
            .find(|(t, _)| *t == tile)
            .map(|(_, era)| *era)
    }
}

/// The works of one city, in fill order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CityWorks {
    pub works: Vec<PlacedWork>,
}

impl CityWorks {
    /// Works in the slots of `building`, in fill order.
    pub fn in_building(&self, building: BuildingKindId) -> impl Iterator<Item = GreatWorkId> + '_ {
        self.works
            .iter()
            .filter(move |w| w.building == building)
            .map(|w| w.work)
    }

    pub fn count_in(&self, building: BuildingKindId) -> usize {
        self.works.iter().filter(|w| w.building == building).count()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TourismState {
    works: Vec<GreatWork>,
    /// Index = city slot (cities are never removed); missing = no works.
    cities: Vec<CityWorks>,
    players: Vec<PlayerTourism>,
    archaeology: Archaeology,
}

/// Parts of the state for the save schema.
pub struct TourismParts {
    pub works: Vec<GreatWork>,
    pub cities: Vec<CityWorks>,
    pub players: Vec<PlayerTourism>,
    pub archaeology: Archaeology,
}

impl TourismState {
    pub fn new(players: usize, seed: u64) -> Self {
        let player = PlayerTourism {
            influence: vec![0; players],
            influence_before: vec![0; players],
            ..PlayerTourism::default()
        };
        Self {
            works: Vec::new(),
            cities: Vec::new(),
            players: vec![player; players],
            archaeology: Archaeology::new(seed),
        }
    }

    pub fn works(&self) -> &[GreatWork] {
        &self.works
    }

    pub fn work(&self, id: GreatWorkId) -> Option<&GreatWork> {
        self.works.get(id.0 as usize)
    }

    pub fn city(&self, city: CityHandle) -> &CityWorks {
        static EMPTY: CityWorks = CityWorks { works: Vec::new() };
        self.cities.get(city.index as usize).unwrap_or(&EMPTY)
    }

    pub fn city_slots(&self) -> &[CityWorks] {
        &self.cities
    }

    pub fn player(&self, p: PlayerId) -> &PlayerTourism {
        static EMPTY: PlayerTourism = PlayerTourism {
            influence: Vec::new(),
            influence_before: Vec::new(),
            lifetime_before: 0,
            opinion: PublicOpinion::Content,
            dissatisfaction: 0,
            preferred: None,
        };
        self.players.get(p.0 as usize).unwrap_or(&EMPTY)
    }

    pub fn players(&self) -> &[PlayerTourism] {
        &self.players
    }

    pub fn archaeology(&self) -> &Archaeology {
        &self.archaeology
    }

    /// Nothing happened yet (no work, no influence, no archaeology).
    pub fn is_untouched(&self) -> bool {
        let quiet = |p: &PlayerTourism| {
            p.influence.iter().all(|v| *v == 0)
                && p.lifetime_before == 0
                && p.opinion == PublicOpinion::Content
        };
        self.works.is_empty()
            && self.players.iter().all(quiet)
            && self.archaeology.records.is_empty()
            && !self.archaeology.triggered
    }

    pub(crate) fn add_work(&mut self, work: GreatWork) -> GreatWorkId {
        self.works.push(work);
        GreatWorkId(self.works.len() as u32 - 1)
    }

    pub(crate) fn city_mut(&mut self, city: CityHandle) -> &mut CityWorks {
        let index = city.index as usize;
        if self.cities.len() <= index {
            self.cities.resize(index + 1, CityWorks::default());
        }
        &mut self.cities[index]
    }

    pub(crate) fn player_mut(&mut self, p: PlayerId) -> Option<&mut PlayerTourism> {
        self.players.get_mut(p.0 as usize)
    }

    pub(crate) fn archaeology_mut(&mut self) -> &mut Archaeology {
        &mut self.archaeology
    }

    /// Restore from the save schema.
    pub(crate) fn from_parts(parts: TourismParts) -> Self {
        Self {
            works: parts.works,
            cities: parts.cities,
            players: parts.players,
            archaeology: parts.archaeology,
        }
    }
}
