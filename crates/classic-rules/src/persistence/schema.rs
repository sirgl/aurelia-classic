//! The classic instance of `kernel::persistence::schema` (generic over the rules
//! module) and the classic save sections kept beside it.

pub use kernel::persistence::schema::*;

use crate::classic::Classic;

pub type SavedWorldV1 = kernel::persistence::schema::SavedWorldV1<Classic>;
pub type SaveGameV1 = kernel::persistence::schema::SaveGameV1<Classic>;

/// A spy: ((name, rank, state, city ordinal + 1), (amount, rate, goal),
/// (diplomat, dead turns)).
pub type SavedSpy = ((u8, u8, u8, u32), (u64, u64, u64), (bool, u8));

/// Espionage: spies per player, steal rights (thief × players + victim),
/// the Renaissance spy granted, the latest era that granted spies.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SavedEspionage {
    pub spies: Vec<Vec<SavedSpy>>,
    pub steal_rights: Vec<u8>,
    pub renaissance_granted: bool,
    pub era_granted: Vec<u8>,
}

/// World Congress: founded, host, special session, United Nations, timer,
/// open session round, World Leader vote due, proposers, proposals (kind,
/// choice, proposer, repeal), delegates per player, votes (voter, proposal,
/// choice, delegates), resolutions in force (kind, choice), extra delegates
/// per player, the last round of the league's turn, projects under way
/// (project, contributions per player), completed projects, project rewards
/// per player (happiness, culture boost turns, tourism boost turns).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[allow(clippy::type_complexity)]
pub struct SavedCongress {
    pub founded: bool,
    pub host: Option<u8>,
    pub special: u8,
    pub united_nations: bool,
    pub turns_until_session: u32,
    pub session_round: Option<u32>,
    pub leader_vote_due: bool,
    pub proposers: Vec<u8>,
    pub proposals: Vec<(u8, u32, Option<u8>, bool)>,
    pub delegates: Vec<u16>,
    pub votes: Vec<(u8, u8, u32, u16)>,
    pub active: Vec<(u8, u32)>,
    pub extra: Vec<u16>,
    pub last_round: Option<u32>,
    pub projects: Vec<(u16, Vec<u64>)>,
    pub completed: Vec<u16>,
    pub rewards: Vec<(u16, u16, u16)>,
}

/// Victory state: disabled victories (bits), the last round, completed
/// projects (players × projects), the World Leader, the outcome (kind,
/// winner, round), one more turn chosen.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SavedVictory {
    pub disabled: u8,
    pub max_rounds: u32,
    pub project_counts: Vec<u16>,
    pub world_leader: Option<u8>,
    pub outcome: Option<(u8, u8, u32)>,
    pub continued: bool,
}

/// City-state state: standings (influence ×100 as u32, flags, last
/// bullied + 1, pledged + 1, pledge broken + 1) and their counters (unit
/// gift counter as u16, quest countdown as u32), pair-indexed; minors
/// (ally + 1, allied since, personality, global countdown); majors (city-
/// states attacked, gold gifted, turns to an allied great person); quests (city-state, major, (tag, payload),
/// start, end + 1).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[allow(clippy::type_complexity)]
pub struct SavedCityStates {
    pub standings: Vec<(u32, u8, u32, u32, u32)>,
    pub counters: Vec<(u16, u32)>,
    pub minors: Vec<(u8, u32, u8, u32)>,
    pub majors: Vec<(u8, u32, u16)>,
    pub quests: Vec<(u8, u8, (u8, u32), u32, u32)>,
}

/// Diplomatic relations: pair matrices (players × players, row-major),
/// trades (from, to, (tag, value, extra), until) and offers (id, from, to,
/// expires, [(giver, (tag, value, extra))]); see `persistence::relations`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SavedRelations {
    pub peace_until: Vec<u32>,
    pub war_since: Vec<u32>,
    pub embassy: Vec<u8>,
    pub pact_until: Vec<u32>,
    pub research_until: Vec<u32>,
    pub research_points: Vec<u32>,
    pub friends_until: Vec<u32>,
    pub denounced_until: Vec<u32>,
    pub open_until: Vec<u32>,
    /// Warmonger memory (non-negative).
    pub warmonger: Vec<u32>,
    pub trades: Vec<(u8, u8, (u8, u32, u16), u32)>,
    #[allow(clippy::type_complexity)]
    pub offers: Vec<(u32, u8, u8, u32, Vec<(u8, (u8, u32, u16))>)>,
    pub next_offer: u32,
}

