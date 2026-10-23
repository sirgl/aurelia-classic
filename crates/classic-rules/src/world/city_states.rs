//! City-state state (classic/docs/classic/rules.md): the
//! influence of every civilization with every city-state, allies,
//! personalities, bullying and pledges, unit gift counters and quests.
//! Authoritative: persisted, hashed while any city-state exists.
//!
//! Pairs are indexed `city_state * players + major`; every player has a
//! row (only city-state rows and civilization columns are used).

use crate::ids::{BuildingKindId, PlayerId, ResourceId, TileId};

/// The rules over this state (defined by the city-state system): status
/// queries, the bonuses other systems consume (`player_bonus` for culture,
/// faith and happiness), the formulas the interface shows.
pub use crate::systems::city_states::gifts::{
    bully_gold_amount, bully_score, gold_gift_influence, gold_gift_influence_with, pledge_problem,
};
pub use crate::systems::city_states::block::CityStateBlock;
pub use crate::systems::city_states::buyout::{buyout_cost, buyout_problem, BUYOUT_ROUNDS};
pub use crate::systems::city_states::quests::reward as quest_reward;
pub use crate::systems::city_states::{
    anchor, bonus_from, city_food_bonus, effective_influence, resting_point, stat, is_ally, is_friends, player_bonus,
    type_of, ALLIES_THRESHOLD, AT_WAR_INFLUENCE, FRIENDS_THRESHOLD,
};

/// Personality of a city-state, chosen once at the start (§4.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Personality {
    #[default]
    Friendly,
    Neutral,
    Hostile,
    Irrational,
}

impl Personality {
    pub const ALL: [Personality; 4] = [
        Personality::Friendly,
        Personality::Neutral,
        Personality::Hostile,
        Personality::Irrational,
    ];
}

/// One civilization's standing with one city-state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Standing {
    /// Base influence ×100 (never below −60 × 100).
    pub influence: i32,
    /// Was a friend once.
    pub ever_friends: bool,
    /// Round of the last bullying.
    pub last_bullied: Option<u32>,
    /// Round of the pledge to protect, while it holds.
    pub pledged: Option<u32>,
    /// Round the pledge was last broken.
    pub pledge_broken: Option<u32>,
    /// The city-state is wary of this civilization (anchor −20).
    pub wary: bool,
    /// Turns until a militaristic city-state gifts a unit (−1: unseeded).
    pub spawn_counter: i16,
    /// The first-contact gift was given.
    pub contacted: bool,
    /// Turns until the next personal quest may start.
    pub quest_countdown: i32,
}

impl Default for Standing {
    fn default() -> Self {
        Self {
            influence: 0,
            ever_friends: false,
            last_bullied: None,
            pledged: None,
            pledge_broken: None,
            wary: false,
            spawn_counter: -1,
            contacted: false,
            quest_countdown: 0,
        }
    }
}

/// A city-state's own record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Minor {
    pub ally: Option<PlayerId>,
    /// Round the current ally became the ally.
    pub allied_since: u32,
    pub personality: Personality,
    /// Turns until the next global quest may start.
    pub global_countdown: i32,
}

/// A civilization's record towards all city-states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct MajorRecord {
    /// City-states it declared war on.
    pub minors_attacked: u8,
    /// Gold it ever gave city-states.
    pub gold_gifted: u32,
    /// Turns until an allied city-state gives it a great person
    /// (Patronage finisher; 0: not counting).
    pub ally_great_person_turns: u16,
}

/// What a quest asks (§4.9.2; the payload is fixed at the start).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QuestKind {
    /// Global: clear the barbarian camp on the tile.
    KillCamp(TileId),
    /// Have the resource (own or traded).
    ConnectResource(ResourceId),
    /// Build the world wonder.
    ConstructWonder(BuildingKindId),
    /// Find the territory of the civilization (see one of its cities).
    FindPlayer(PlayerId),
    /// Discover a natural wonder: the count known at the start.
    FindNaturalWonder { known: u16 },
    /// Give gold: the gold gifted at the start.
    GiveGold { gifted: u32 },
    /// Pledge to protect the city-state.
    PledgeToProtect,
    /// Bully the target city-state.
    BullyCityState(PlayerId),
    /// Denounce the civilization.
    DenounceMajor(PlayerId),
    /// Global contest: most technologies gained (techs at the start).
    ContestTechs { start: u16 },
    /// Global: gold gifts give +50 % influence while it runs.
    Invest,
}

impl QuestKind {
    pub fn is_global(self) -> bool {
        matches!(
            self,
            QuestKind::KillCamp(_) | QuestKind::ContestTechs { .. } | QuestKind::Invest
        )
    }
}

/// A quest of a city-state for a civilization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Quest {
    pub city_state: PlayerId,
    pub major: PlayerId,
    pub kind: QuestKind,
    pub start: u32,
    /// The last round it runs (None: until done or void).
    pub end: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CityStatesState {
    players: usize,
    pub(crate) standings: Vec<Standing>,
    pub(crate) minors: Vec<Minor>,
    pub(crate) majors: Vec<MajorRecord>,
    pub(crate) quests: Vec<Quest>,
}

impl CityStatesState {
    pub fn new(players: usize) -> Self {
        Self {
            players,
            standings: vec![Standing::default(); players * players],
            minors: vec![Minor::default(); players],
            majors: vec![MajorRecord::default(); players],
            quests: Vec::new(),
        }
    }

    pub fn players(&self) -> usize {
        self.players
    }

    /// Nothing happened yet (games without city-states stay pristine).
    pub fn is_pristine(&self) -> bool {
        *self == Self::new(self.players)
    }

    fn slot(&self, city_state: PlayerId, major: PlayerId) -> Option<usize> {
        let (c, m) = (city_state.0 as usize, major.0 as usize);
        (c < self.players && m < self.players).then_some(c * self.players + m)
    }

    pub fn standing(&self, city_state: PlayerId, major: PlayerId) -> Standing {
        self.slot(city_state, major)
            .map(|i| self.standings[i])
            .unwrap_or_default()
    }

    pub(crate) fn standing_mut(
        &mut self,
        city_state: PlayerId,
        major: PlayerId,
    ) -> Option<&mut Standing> {
        let i = self.slot(city_state, major)?;
        self.standings.get_mut(i)
    }

    /// Base influence in whole points (truncated towards zero).
    pub fn influence(&self, city_state: PlayerId, major: PlayerId) -> i32 {
        self.standing(city_state, major).influence / 100
    }

    pub fn minor(&self, city_state: PlayerId) -> Minor {
        self.minors
            .get(city_state.0 as usize)
            .copied()
            .unwrap_or_default()
    }

    pub(crate) fn minor_mut(&mut self, city_state: PlayerId) -> Option<&mut Minor> {
        self.minors.get_mut(city_state.0 as usize)
    }

    pub fn ally(&self, city_state: PlayerId) -> Option<PlayerId> {
        self.minor(city_state).ally
    }

    pub fn major(&self, major: PlayerId) -> MajorRecord {
        self.majors
            .get(major.0 as usize)
            .copied()
            .unwrap_or_default()
    }

    pub(crate) fn major_mut(&mut self, major: PlayerId) -> Option<&mut MajorRecord> {
        self.majors.get_mut(major.0 as usize)
    }

    pub fn quests(&self) -> &[Quest] {
        &self.quests
    }

    pub fn standings(&self) -> &[Standing] {
        &self.standings
    }

    pub fn minors(&self) -> &[Minor] {
        &self.minors
    }

    pub fn majors(&self) -> &[MajorRecord] {
        &self.majors
    }

    pub(crate) fn from_parts(
        players: usize,
        standings: Vec<Standing>,
        minors: Vec<Minor>,
        majors: Vec<MajorRecord>,
        quests: Vec<Quest>,
    ) -> Self {
        Self {
            players,
            standings,
            minors,
            majors,
            quests,
        }
    }
}
