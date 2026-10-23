//! World Congress and United Nations state (classic/docs/classic/rules.md): the
//! league once founded, its host, the special session in force, the
//! session timer, proposal rights, proposals of the next session, the
//! votes of an open session, the resolutions in force and the extra
//! delegates won by failed World Leader votes. Authoritative: persisted,
//! hashed once founded.

use crate::ids::PlayerId;
use crate::rules::content::ProjectId;

/// The rules over this state (defined by the congress system).
pub use crate::systems::league_projects::{
    cost_times100, project_problem, share_times100, tiers, BOOST_PERCENT,
};
pub use crate::systems::congress::{
    delegates, funding_percent, game_era, luxury_banned, proposal_problem, session_for_era, trade_embargoed,
    vote_problem, votes_needed, votes_needed_for, SpecialSession, ARMY_TAX_PERCENT, FUNDING_PERCENT,
    HERITAGE_CULTURE_PER_WONDER, NO, SCHOLARS_PERCENT, SESSIONS, YES,
};

/// The resolutions the congress knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ResolutionKind {
    /// Elect the World Leader (United Nations only, automatic).
    DiplomaticVictory,
    /// Choose the host (special sessions, automatic).
    ChangeHost,
    /// Unit maintenance rises (`ARMY_TAX_PERCENT`).
    StandingArmyTax,
    /// More research on technologies a member knows (`SCHOLARS_PERCENT`).
    ScholarsInResidence,
    /// No Atomic Bombs or Nuclear Missiles may be built.
    NuclearNonProliferation,
    /// Culture for every world wonder (`HERITAGE_CULTURE_PER_WONDER`).
    CulturalHeritageSites,
    /// No trade routes with city-states.
    CityStateEmbargo,
    /// No trade routes of the chosen civilization, none to it.
    PlayerEmbargo,
    /// The chosen luxury gives no happiness.
    BanLuxury,
    /// More great person points for writers, artists, musicians, fewer for
    /// scientists, engineers, merchants (`FUNDING_PERCENT`).
    ArtsFunding,
    /// The reverse of arts funding.
    SciencesFunding,
    /// Start a World Congress project (World's Fair, International Games,
    /// International Space Station; choice: the project id). Not kept in
    /// force: passing it starts the project.
    InternationalProject,
}

impl ResolutionKind {
    pub const ALL: [ResolutionKind; 12] = [
        ResolutionKind::DiplomaticVictory,
        ResolutionKind::ChangeHost,
        ResolutionKind::StandingArmyTax,
        ResolutionKind::ScholarsInResidence,
        ResolutionKind::NuclearNonProliferation,
        ResolutionKind::CulturalHeritageSites,
        ResolutionKind::CityStateEmbargo,
        ResolutionKind::PlayerEmbargo,
        ResolutionKind::BanLuxury,
        ResolutionKind::ArtsFunding,
        ResolutionKind::SciencesFunding,
        ResolutionKind::InternationalProject,
    ];

    /// Decided by a vote for a civilization (not yes/no).
    pub fn elects(self) -> bool {
        matches!(
            self,
            ResolutionKind::DiplomaticVictory | ResolutionKind::ChangeHost
        )
    }

    /// Proposed only by the league itself.
    pub fn automatic(self) -> bool {
        self.elects()
    }

    pub fn tag(self) -> u8 {
        ResolutionKind::ALL
            .iter()
            .position(|k| *k == self)
            .unwrap_or(0) as u8
    }
}

/// A proposal of the next (or open) session: enact a resolution (with its
/// choice: a player, a resource) or repeal one in force.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Proposal {
    pub kind: ResolutionKind,
    /// Player id or resource id the proposer chose (0 when none).
    pub choice: u32,
    pub proposer: Option<PlayerId>,
    pub repeal: bool,
}

/// Delegates a member put on a proposal: yes/no (choice 1/0), or the
/// player voted for (elections).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Vote {
    pub voter: PlayerId,
    pub proposal: u8,
    pub choice: u32,
    pub delegates: u16,
}

/// A World Congress project under way (§6.8): the production each member
/// contributed through the project's city process, in hundredths.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProjectProgress {
    pub project: ProjectId,
    /// Index = player id.
    pub contributions: Vec<u64>,
}

impl ProjectProgress {
    pub fn total(&self) -> u64 {
        self.contributions.iter().sum()
    }

    pub fn of(&self, player: PlayerId) -> u64 {
        self.contributions.get(player.0 as usize).copied().unwrap_or(0)
    }
}

/// What World Congress projects left a member for good or for a while.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct LeagueRewards {
    /// Permanent happiness (International Games).
    pub happiness: u16,
    /// Turns of boosted culture left (World's Fair, `BOOST_PERCENT`).
    pub culture_boost_turns: u16,
    /// Turns of boosted tourism left (International Games, `BOOST_PERCENT`).
    pub tourism_boost_turns: u16,
}

impl LeagueRewards {
    pub fn is_empty(&self) -> bool {
        *self == LeagueRewards::default()
    }
}

/// A resolution in force.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Active {
    pub kind: ResolutionKind,
    pub choice: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CongressState {
    pub(crate) founded: bool,
    pub(crate) host: Option<PlayerId>,
    /// Index into the special sessions (`systems::congress::SESSIONS`).
    pub(crate) special: u8,
    pub(crate) united_nations: bool,
    pub(crate) turns_until_session: u32,
    /// The round the open session started (None: not in session).
    pub(crate) session_round: Option<u32>,
    /// The World Leader vote is due next session (UN alternation).
    pub(crate) leader_vote_due: bool,
    /// Players who may propose before the next session.
    pub(crate) proposers: Vec<PlayerId>,
    pub(crate) proposals: Vec<Proposal>,
    /// Delegates each member holds in the open session (by player).
    pub(crate) delegates: Vec<u16>,
    pub(crate) votes: Vec<Vote>,
    pub(crate) active: Vec<Active>,
    /// Permanent extra delegates (failed World Leader votes).
    pub(crate) extra: Vec<u16>,
    /// The last round the league's turn ran.
    pub(crate) last_round: Option<u32>,
    /// World Congress projects under way.
    pub(crate) projects: Vec<ProjectProgress>,
    /// World Congress projects completed (never proposed again).
    pub(crate) completed: Vec<ProjectId>,
    /// Rewards of completed projects by player.
    pub(crate) rewards: Vec<LeagueRewards>,
}

impl CongressState {
    pub fn new(players: usize) -> Self {
        Self {
            extra: vec![0; players],
            delegates: vec![0; players],
            rewards: vec![LeagueRewards::default(); players],
            ..Default::default()
        }
    }

    pub fn is_pristine(&self) -> bool {
        !self.founded && self.last_round.is_none()
    }

    pub fn founded(&self) -> bool {
        self.founded
    }

    pub fn host(&self) -> Option<PlayerId> {
        self.host
    }

    pub fn special_session(&self) -> u8 {
        self.special
    }

    pub fn united_nations(&self) -> bool {
        self.united_nations
    }

    pub fn turns_until_session(&self) -> u32 {
        self.turns_until_session
    }

    pub fn in_session(&self) -> bool {
        self.session_round.is_some()
    }

    pub fn session_round(&self) -> Option<u32> {
        self.session_round
    }

    pub fn leader_vote_due(&self) -> bool {
        self.leader_vote_due
    }

    pub fn proposers(&self) -> &[PlayerId] {
        &self.proposers
    }

    pub fn proposals(&self) -> &[Proposal] {
        &self.proposals
    }

    pub fn delegates_of(&self, player: PlayerId) -> u16 {
        self.delegates.get(player.0 as usize).copied().unwrap_or(0)
    }

    pub fn delegates(&self) -> &[u16] {
        &self.delegates
    }

    pub fn votes(&self) -> &[Vote] {
        &self.votes
    }

    /// Delegates `player` has not cast yet in the open session.
    pub fn delegates_left(&self, player: PlayerId) -> u16 {
        let cast: u16 = self
            .votes
            .iter()
            .filter(|v| v.voter == player)
            .map(|v| v.delegates)
            .sum();
        self.delegates_of(player).saturating_sub(cast)
    }

    pub fn active(&self) -> &[Active] {
        &self.active
    }

    pub fn is_active(&self, kind: ResolutionKind) -> bool {
        self.active.iter().any(|a| a.kind == kind)
    }

    pub fn extra(&self) -> &[u16] {
        &self.extra
    }

    pub fn last_round(&self) -> Option<u32> {
        self.last_round
    }

    pub fn projects(&self) -> &[ProjectProgress] {
        &self.projects
    }

    /// The project under way, if it is.
    pub fn project(&self, project: ProjectId) -> Option<&ProjectProgress> {
        self.projects.iter().find(|p| p.project == project)
    }

    pub fn completed(&self) -> &[ProjectId] {
        &self.completed
    }

    pub fn rewards(&self) -> &[LeagueRewards] {
        &self.rewards
    }

    pub fn rewards_of(&self, player: PlayerId) -> LeagueRewards {
        self.rewards.get(player.0 as usize).copied().unwrap_or_default()
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn from_parts(
        founded: bool,
        host: Option<PlayerId>,
        special: u8,
        united_nations: bool,
        turns_until_session: u32,
        session_round: Option<u32>,
        leader_vote_due: bool,
        lists: CongressLists,
    ) -> Self {
        Self {
            founded,
            host,
            special,
            united_nations,
            turns_until_session,
            session_round,
            leader_vote_due,
            proposers: lists.proposers,
            proposals: lists.proposals,
            delegates: lists.delegates,
            votes: lists.votes,
            active: lists.active,
            extra: lists.extra,
            last_round: lists.last_round,
            projects: lists.projects,
            completed: lists.completed,
            rewards: lists.rewards,
        }
    }
}

/// The list parts of a restored congress.
pub(crate) struct CongressLists {
    pub proposers: Vec<PlayerId>,
    pub proposals: Vec<Proposal>,
    pub delegates: Vec<u16>,
    pub votes: Vec<Vote>,
    pub active: Vec<Active>,
    pub extra: Vec<u16>,
    pub last_round: Option<u32>,
    pub projects: Vec<ProjectProgress>,
    pub completed: Vec<ProjectId>,
    pub rewards: Vec<LeagueRewards>,
}
