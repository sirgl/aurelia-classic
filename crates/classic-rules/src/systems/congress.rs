//! World Congress and United Nations (classic/docs/classic/rules.md): founding by
//! the first civilization with Printing Press that met every other,
//! special sessions by the game's era (host elections, the United
//! Nations), sessions every 28/21/14 rounds, proposals by the host and
//! the two members with the most delegates, delegates, the tally,
//! resolutions in force, the World Leader vote and the delegates it needs.
//! Own design (the tuning is the block of named constants at the top).
//!
//! The league's turn runs once a round (the first `Congress` step of the
//! round). A session opens when its timer runs out and closes one round
//! later: members cast their delegates in their turns;
//! delegates not cast abstain.

use crate::command::CommandError;
use crate::ids::PlayerId;
use crate::rules::stats::{LeagueStat, Stat};
use crate::rules::CompiledRuleset;
use crate::transaction::transaction::WorldTransaction;
use crate::world::congress::{Active, Proposal, ResolutionKind, Vote};
use crate::world::state::AuthWorld;
use crate::world::player::ClassicPlayers;

/// One special session of the league (Standard speed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpecialSession {
    /// Era index that triggers it.
    pub era: u8,
    pub turns_between: u32,
    pub civ_delegates: u16,
    pub host_delegates: u16,
    pub city_state_delegates: u16,
    pub united_nations: bool,
}

/// The three special sessions: the Assembly (Renaissance), the Council (Industrial era, the city-states
/// get a seat) and the United Nations (Atomic era, the World Leader vote). Own design (Unciv has no World
/// Congress, only the United Nations vote, where its owner has 2 votes like the host here): the seats and
/// the pace grow with the eras, 28 / 21 / 14 rounds between sessions.
pub const SESSIONS: [SpecialSession; 3] = [
    SpecialSession {
        era: 3,
        turns_between: 28,
        civ_delegates: 2,
        host_delegates: 1,
        city_state_delegates: 0,
        united_nations: false,
    },
    SpecialSession {
        era: 4,
        turns_between: 21,
        civ_delegates: 3,
        host_delegates: 2,
        city_state_delegates: 1,
        united_nations: false,
    },
    SpecialSession {
        era: 6,
        turns_between: 14,
        civ_delegates: 4,
        host_delegates: 2,
        city_state_delegates: 1,
        united_nations: true,
    },
];
/// Extra delegates each of the two leading candidates keeps for good when the World Leader vote fails.
/// Own design (Unciv has no equivalent).
pub const LEADERS_BONUS_ON_FAIL: u16 = 1;
/// Percent a nation must have met of the other civilizations (rounded up, at least one) to found the
/// World Congress with the technology that allows it. Own design.
pub const FOUNDING_MET_PERCENT: usize = 50;
/// Percent added to unit maintenance while the Standing Army Tax is in force. Own design.
pub const ARMY_TAX_PERCENT: u64 = 20;
/// Percent added to the science of a nation researching a technology another civilization knows while
/// Scholars in Residence is in force. Own design.
pub const SCHOLARS_PERCENT: i32 = 12;
/// Culture per turn each world wonder gives while the Cultural Heritage Sites resolution is in force. Own design.
pub const HERITAGE_CULTURE_PER_WONDER: u32 = 2;
/// Percent the arts (writers, artists, musicians) gain and the industries (scientists, engineers, merchants)
/// lose in great person points under Arts Funding, and the reverse under Sciences Funding. Own design.
pub const FUNDING_PERCENT: i32 = 25;
/// Share of the available votes the World Leader needs: with `n` votes,
/// `n × (67 - floor(1.1 n)) / 100 + 1` up to 28 votes, `n × 35 / 100`
/// above. Unciv: logic/civilization/managers/VictoryManager.kt
/// (`votesNeededForDiplomaticVictory`, applied, as there, to the count of
/// available votes: here the delegates of the United Nations session).
pub const VICTORY_SHARE_PERCENT: i64 = 67;
/// Unciv: VictoryManager.kt (`votesNeededForDiplomaticVictory`), equal.
pub const VICTORY_SHARE_DROP_TIMES10: i64 = 11;
/// Unciv: VictoryManager.kt (`votesNeededForDiplomaticVictory`), equal.
pub const VICTORY_LARGE_VOTES: i64 = 28;
/// Unciv: VictoryManager.kt (`votesNeededForDiplomaticVictory`), equal.
pub const VICTORY_LARGE_PERCENT: i64 = 35;
/// Proposals per session: the host and the two other members with the most delegates. Own design (Unciv has no equivalent).
pub const PROPOSERS: usize = 3;
/// Yes and no of a yes/no proposal.
pub const YES: u32 = 1;
pub const NO: u32 = 0;

/// The era that triggers special sessions: the top era, if at least half
/// the civilizations reached it, else the one before.
pub fn game_era(auth: &AuthWorld, rules: &CompiledRuleset) -> u8 {
    let majors = crate::systems::city_states::majors(auth);
    let eras: Vec<u8> = majors
        .iter()
        .map(|p| {
            crate::combat::city::current_era(auth, rules, *p)
                .map(|e| e.index)
                .unwrap_or(0)
        })
        .collect();
    let top = eras.iter().copied().max().unwrap_or(0);
    let at_top = eras.iter().filter(|e| **e == top).count();
    if at_top >= majors.len() / 2 {
        top
    } else {
        top.saturating_sub(1)
    }
}

/// The special session the game era calls for (index into SESSIONS).
pub fn session_for_era(era: u8) -> usize {
    SESSIONS.iter().rposition(|s| s.era <= era).unwrap_or(0)
}

/// Delegates `player` holds at the opening of a session (§6.5): the
/// session's per civilization, host and city-state ally delegates, those
/// won by failed World Leader votes, `League(Delegates)` (Forbidden Palace)
/// and `League(DelegatesPerDiplomat)` per diplomat in the capital of a
/// living civilization (Globalization).
pub fn delegates(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId) -> u16 {
    if !auth.players().is_major(player) || !auth.players().is_alive(player) {
        return 0;
    }
    let congress = auth.module().congress();
    let session = SESSIONS[congress.special_session() as usize];
    let host = if congress.host() == Some(player) {
        session.host_delegates
    } else {
        0
    };
    let allies = crate::systems::city_states::allies_of(auth, player).len() as u16;
    let extra = congress
        .extra()
        .get(player.0 as usize)
        .copied()
        .unwrap_or(0);
    let stats = &crate::effects::player::player_modifiers(auth, rules, player).stats;
    let own = stats.get(Stat::Module(crate::rules::stats::ClassicStat::League(LeagueStat::Delegates)));
    let per_diplomat = stats.get(Stat::Module(crate::rules::stats::ClassicStat::League(LeagueStat::DelegatesPerDiplomat)));
    let diplomats = if per_diplomat > 0 { visiting_diplomats(auth, player) } else { 0 };
    let effects = (own + per_diplomat * diplomats as i32).clamp(0, i32::from(u16::MAX)) as u16;
    session.civ_delegates + host + session.city_state_delegates * allies + extra + effects
}

/// Diplomats of `player` at work (`SCHMOOZE`) in the capital of a living
/// civilization (§7.11).
pub fn visiting_diplomats(auth: &AuthWorld, player: PlayerId) -> u32 {
    diplomats_in(auth, player, |host| auth.players().is_major(host) && auth.players().is_alive(host))
}

/// A diplomat of `player` is at work in the capital of `host`.
pub fn diplomat_visiting(auth: &AuthWorld, player: PlayerId, host: PlayerId) -> bool {
    diplomats_in(auth, player, |h| h == host) > 0
}

fn diplomats_in(auth: &AuthWorld, player: PlayerId, hosts: impl Fn(PlayerId) -> bool) -> u32 {
    let mut count = 0;
    for spy in auth.module().espionage().spies_of(player) {
        if !spy.diplomat || spy.state != crate::world::espionage::SpyState::Schmooze {
            continue;
        }
        let host = spy.city.and_then(|c| auth.cities().get(c)).map(|c| c.owner);
        if host.is_some_and(&hosts) {
            count += 1;
        }
    }
    count
}

/// Delegates the World Leader needs (§6.9).
pub fn votes_needed(auth: &AuthWorld) -> u16 {
    let un = SESSIONS[SESSIONS.len() - 1];
    // Only living voters count (Unciv: VictoryManager.kt, `getVotingCivs`).
    let weight = |p: PlayerId| if auth.players().is_alive(p) { 1.0 } else { 0.0 };
    let players: Vec<PlayerId> = (0..auth.players().count())
        .map(|p| PlayerId(p as u8))
        .collect();
    let civs: f64 = players
        .iter()
        .filter(|p| auth.players().is_major(**p))
        .map(|p| weight(*p))
        .sum();
    let city_states: f64 = players
        .iter()
        .filter(|p| auth.players().is_city_state(**p))
        .map(|p| weight(*p))
        .sum();
    votes_needed_for(civs, city_states, un)
}

/// The threshold for `civs` living civilizations and `city_states` living
/// city-states: Unciv's share of the available votes, the votes being the
/// delegates of the session (host, per civilization, per city-state).
pub fn votes_needed_for(civs: f64, city_states: f64, un: SpecialSession) -> u16 {
    let n = i64::from(un.host_delegates)
        + i64::from(un.civ_delegates) * civs as i64
        + i64::from(un.city_state_delegates) * city_states as i64;
    let need = if n > VICTORY_LARGE_VOTES {
        n * VICTORY_LARGE_PERCENT / 100
    } else {
        n * (VICTORY_SHARE_PERCENT - VICTORY_SHARE_DROP_TIMES10 * n / 10) / 100 + 1
    };
    need.clamp(0, i64::from(u16::MAX)) as u16
}

/// Scholars in Residence: the technology `player` researches is known by
/// another civilization (+20 % science toward it).
pub fn scholars_bonus(auth: &AuthWorld, player: PlayerId) -> bool {
    if !auth
        .module()
        .congress()
        .is_active(ResolutionKind::ScholarsInResidence)
    {
        return false;
    }
    let Some(head) = auth.players().research_queue_of(player).first().copied() else {
        return false;
    };
    crate::systems::city_states::majors(auth)
        .into_iter()
        .any(|p| p != player && auth.players().techs_of(p).contains(&head))
}

/// Cultural Heritage Sites: [`HERITAGE_CULTURE_PER_WONDER`] culture a turn for every world wonder.
pub fn heritage_culture(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId) -> u32 {
    if !auth
        .module()
        .congress()
        .is_active(ResolutionKind::CulturalHeritageSites)
    {
        return 0;
    }
    let wonders = auth
        .indexes()
        .cities_of(player)
        .iter()
        .filter_map(|c| auth.cities().get(*c))
        .flat_map(|c| c.buildings.iter())
        .filter(|b| {
            rules
                .buildings
                .get(b.0 as usize)
                .is_some_and(|d| d.kind == crate::rules::content::BuildingKind::WorldWonder)
        })
        .count() as u32;
    HERITAGE_CULTURE_PER_WONDER * wonders
}

/// Percent the funding resolutions in force change the great person points of `kind` by: Arts Funding adds
/// [`FUNDING_PERCENT`] to writers, artists and musicians and takes it from scientists, engineers and
/// merchants; Sciences Funding does the reverse; both in force cancel out. Own design.
pub fn funding_percent(auth: &AuthWorld, kind: crate::classic::content::GreatPersonKind) -> i32 {
    use crate::classic::content::GreatPersonKind as K;
    let side = match kind {
        K::Writer | K::Artist | K::Musician => 1,
        K::Scientist | K::Engineer | K::Merchant => -1,
        K::General | K::Admiral | K::Prophet => return 0,
    };
    let congress = auth.module().congress();
    let shift = i32::from(congress.is_active(ResolutionKind::ArtsFunding))
        - i32::from(congress.is_active(ResolutionKind::SciencesFunding));
    side * shift * FUNDING_PERCENT
}

/// A World Congress embargo forbids trade routes from `from` to `to`: the City-State Embargo forbids every
/// route to a city-state, a Player Embargo every route of the chosen civilization and every route to it.
pub fn trade_embargoed(auth: &AuthWorld, from: PlayerId, to: PlayerId) -> bool {
    let congress = auth.module().congress();
    if !congress.founded() {
        return false;
    }
    (congress.is_active(ResolutionKind::CityStateEmbargo) && auth.players().is_city_state(to))
        || congress
            .active()
            .iter()
            .any(|a| a.kind == ResolutionKind::PlayerEmbargo && (a.choice == u32::from(from.0) || a.choice == u32::from(to.0)))
}

/// The luxury resource a Ban Luxury resolution in force strips of its happiness for everybody.
pub fn luxury_banned(auth: &AuthWorld, resource: crate::ids::ResourceId) -> bool {
    auth.module()
        .congress()
        .active()
        .iter()
        .any(|a| a.kind == ResolutionKind::BanLuxury && a.choice == u32::from(resource.0))
}

/// Why `player` may not propose `kind` now (None: it may).
pub fn proposal_problem(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    player: PlayerId,
    kind: ResolutionKind,
    choice: u32,
    repeal: bool,
) -> Option<&'static str> {
    let congress = auth.module().congress();
    if !congress.founded() || congress.in_session() {
        return Some("no proposals now");
    }
    if !congress.proposers().contains(&player) {
        return Some("no right to propose");
    }
    if congress
        .proposals()
        .iter()
        .any(|p| p.proposer == Some(player))
    {
        return Some("already proposed");
    }
    if kind.automatic() {
        return Some("proposed by the league itself");
    }
    if repeal
        != congress
            .active()
            .iter()
            .any(|a| a.kind == kind && a.choice == choice)
    {
        return Some(if repeal {
            "not in force"
        } else {
            "already in force"
        });
    }
    if congress
        .proposals()
        .iter()
        .any(|p| p.kind == kind && p.choice == choice)
    {
        return Some("already proposed");
    }
    if kind == ResolutionKind::InternationalProject {
        if repeal {
            return Some("a project is not in force");
        }
        let project = crate::rules::content::ProjectId(choice.min(u32::from(u16::MAX)) as u16);
        return crate::systems::league_projects::project_problem(auth, rules, project);
    }
    if kind == ResolutionKind::PlayerEmbargo {
        let target = PlayerId(choice as u8);
        let alive = auth.players().is_major(target) && auth.players().is_alive(target);
        if !alive || auth.indexes().cities_of(target).is_empty() {
            return Some("no such nation");
        }
    }
    None
}

/// Why `player` may not put `delegates` on proposal `index` with `choice`.
pub fn vote_problem(
    auth: &AuthWorld,
    player: PlayerId,
    index: u8,
    choice: u32,
    delegates: u16,
) -> Option<&'static str> {
    let congress = auth.module().congress();
    if !congress.in_session() {
        return Some("no session");
    }
    let Some(proposal) = congress.proposals().get(index as usize) else {
        return Some("no such proposal");
    };
    if delegates == 0 || delegates > congress.delegates_left(player) {
        return Some("not enough delegates");
    }
    let valid = if proposal.kind.elects() {
        let candidate = PlayerId(choice as u8);
        auth.players().is_major(candidate) && auth.players().is_alive(candidate)
    } else {
        choice == YES || choice == NO
    };
    (!valid).then_some("no such choice")
}

/// The winning choice of a proposal and its votes; a tie between
/// candidates goes to the host if among them, else to the lowest id;
/// a yes/no tie is a no (§6.6).
fn decision(
    votes: &[Vote],
    index: u8,
    host: Option<PlayerId>,
    elects: bool,
) -> Option<(u32, u16, bool)> {
    let mut tally: std::collections::BTreeMap<u32, u16> = Default::default();
    for v in votes.iter().filter(|v| v.proposal == index) {
        *tally.entry(v.choice).or_default() += v.delegates;
    }
    let top = tally.values().copied().max()?;
    let leaders: Vec<u32> = tally
        .iter()
        .filter(|(_, n)| **n == top)
        .map(|(c, _)| *c)
        .collect();
    let tie = leaders.len() > 1;
    if !elects {
        return Some((if tie { NO } else { leaders[0] }, top, tie));
    }
    let host_choice = host.map(|h| u32::from(h.0)).filter(|h| leaders.contains(h));
    Some((host_choice.unwrap_or(leaders[0]), top, tie))
}

impl<'a> WorldTransaction<'a> {
    /// The league's turn, once a round (§6.1–6.3).
    pub(crate) fn congress_turn(&mut self, rules: &CompiledRuleset) {
        // Founding is checked at every end of turn (a game without a
        // league keeps no league state at all).
        if !self.world.module.congress.founded {
            self.try_found_league(rules);
            return;
        }
        let round = self.world.turn.round;
        if self.world.module.congress.last_round == Some(round) {
            return;
        }
        self.world.module.congress.last_round = Some(round);
        self.check_projects(rules);
        if self.world.module.congress.session_round.is_some_and(|r| r < round) {
            self.finish_session(rules);
        }
        if !self.world.module.congress.in_session() && self.try_special_session(rules) {
            return;
        }
        self.tick_session_timer(rules);
    }

    fn try_found_league(&mut self, rules: &CompiledRuleset) {
        let majors = crate::systems::city_states::majors(self.world);
        if majors.len() < 2 {
            return;
        }
        // Own design: the first nation with the technology that has met at least half of the others.
        let needed = ((majors.len() - 1) * FOUNDING_MET_PERCENT).div_ceil(100).max(1);
        let founder = majors.iter().copied().find(|p| {
            let printing = self.world.players.techs_of(*p).iter().any(|t| {
                rules
                    .technologies
                    .get(t.0 as usize)
                    .is_some_and(|d| d.module.allows_world_congress)
            });
            printing
                && majors
                    .iter()
                    .filter(|o| *o != p && crate::systems::diplomacy::has_met(self.world, *p, **o))
                    .count()
                    >= needed
        });
        let Some(founder) = founder else {
            return;
        };
        let special = session_for_era(game_era(self.world, rules));
        let round = self.world.turn.round;
        let congress = &mut self.world.module.congress;
        congress.founded = true;
        congress.host = Some(founder);
        congress.special = special as u8;
        congress.united_nations = SESSIONS[special].united_nations;
        congress.turns_until_session = SESSIONS[special].turns_between;
        congress.last_round = Some(round);
        self.assign_proposers(rules);
        self.relations_changed(founder);
    }

    /// A special session of a later era: a host election (and the United
    /// Nations) opens at once.
    fn try_special_session(&mut self, rules: &CompiledRuleset) -> bool {
        let next = session_for_era(game_era(self.world, rules));
        if next <= self.world.module.congress.special as usize {
            return false;
        }
        let congress = &mut self.world.module.congress;
        congress.special = next as u8;
        congress.united_nations |= SESSIONS[next].united_nations;
        congress.proposals.insert(
            0,
            Proposal {
                kind: ResolutionKind::ChangeHost,
                choice: 0,
                proposer: None,
                repeal: false,
            },
        );
        self.open_session(rules);
        true
    }

    fn tick_session_timer(&mut self, rules: &CompiledRuleset) {
        let congress = &mut self.world.module.congress;
        congress.turns_until_session = congress.turns_until_session.saturating_sub(1);
        if congress.turns_until_session > 0 {
            return;
        }
        if congress.leader_vote_due && congress.united_nations {
            congress.proposals.retain(|p| p.proposer.is_none());
            congress.proposals.push(Proposal {
                kind: ResolutionKind::DiplomaticVictory,
                choice: 0,
                proposer: None,
                repeal: false,
            });
        }
        self.open_session(rules);
    }

    fn open_session(&mut self, rules: &CompiledRuleset) {
        let delegates: Vec<u16> = (0..self.world.players.count())
            .map(|p| delegates(self.world, rules, PlayerId(p as u8)))
            .collect();
        let round = self.world.turn.round;
        let congress = &mut self.world.module.congress;
        congress.delegates = delegates;
        congress.votes.clear();
        congress.session_round = Some(round);
    }

    /// The tally: every proposal is decided, the timer restarts, new
    /// proposal rights (in the United Nations the World Leader vote
    /// alternates with them).
    fn finish_session(&mut self, rules: &CompiledRuleset) {
        let proposals = std::mem::take(&mut self.world.module.congress.proposals);
        let votes = std::mem::take(&mut self.world.module.congress.votes);
        let host = self.world.module.congress.host;
        let mut leader_vote = false;
        for (i, proposal) in proposals.iter().enumerate() {
            let decided = decision(&votes, i as u8, host, proposal.kind.elects());
            leader_vote |= proposal.kind == ResolutionKind::DiplomaticVictory;
            self.resolve(*proposal, decided, &votes, i as u8, rules);
        }
        let congress = &mut self.world.module.congress;
        congress.session_round = None;
        congress.turns_until_session = SESSIONS[congress.special as usize].turns_between;
        congress.leader_vote_due = congress.united_nations && !leader_vote;
        self.assign_proposers(rules);
    }

    fn resolve(
        &mut self,
        proposal: Proposal,
        decided: Option<(u32, u16, bool)>,
        votes: &[Vote],
        index: u8,
        rules: &CompiledRuleset,
    ) {
        match proposal.kind {
            ResolutionKind::ChangeHost => {
                if let Some((winner, _, _)) = decided {
                    self.world.module.congress.host = Some(PlayerId(winner as u8));
                }
            }
            ResolutionKind::DiplomaticVictory => self.leader_vote(decided, votes, index, rules),
            // Starting a project has no lasting effect of its own.
            ResolutionKind::InternationalProject if decided.is_some_and(|d| d.0 == YES) => {
                let project = crate::rules::content::ProjectId(proposal.choice as u16);
                if crate::systems::league_projects::project_problem(self.world, rules, project).is_none() {
                    self.start_project(project);
                }
            }
            ResolutionKind::InternationalProject => {}
            kind if decided.is_some_and(|d| d.0 == YES) => {
                let active = &mut self.world.module.congress.active;
                if proposal.repeal {
                    active.retain(|a| !(a.kind == kind && a.choice == proposal.choice));
                } else {
                    active.push(Active {
                        kind,
                        choice: proposal.choice,
                    });
                }
                if matches!(kind, ResolutionKind::CityStateEmbargo | ResolutionKind::PlayerEmbargo) && !proposal.repeal {
                    self.end_embargoed_routes(rules);
                }
            }
            _ => {}
        }
    }

    /// The World Leader: elected with enough delegates and no tie; else
    /// the two leading candidates gain two delegates for good.
    fn leader_vote(
        &mut self,
        decided: Option<(u32, u16, bool)>,
        votes: &[Vote],
        index: u8,
        rules: &CompiledRuleset,
    ) {
        let needed = votes_needed(self.world);
        if let Some((winner, count, false)) = decided {
            if count >= needed {
                self.world.module.victory.world_leader = Some(PlayerId(winner as u8));
                self.test_diplomatic_victory(rules);
                return;
            }
        }
        let mut tally: std::collections::BTreeMap<u32, u16> = Default::default();
        for v in votes.iter().filter(|v| v.proposal == index) {
            *tally.entry(v.choice).or_default() += v.delegates;
        }
        let mut ranked: Vec<(u16, u32)> = tally.into_iter().map(|(c, n)| (n, c)).collect();
        ranked.sort_by_key(|(n, c)| (std::cmp::Reverse(*n), *c));
        for (_, candidate) in ranked.into_iter().take(2) {
            if let Some(extra) = self.world.module.congress.extra.get_mut(candidate as usize) {
                *extra += LEADERS_BONUS_ON_FAIL;
            }
        }
    }

    /// The host and the member with the most delegates propose (§6.4).
    fn assign_proposers(&mut self, rules: &CompiledRuleset) {
        let host = self.world.module.congress.host;
        let mut ranked: Vec<(u16, PlayerId)> = crate::systems::city_states::majors(self.world)
            .into_iter()
            .filter(|p| Some(*p) != host)
            .map(|p| (delegates(self.world, rules, p), p))
            .collect();
        ranked.sort_by_key(|(d, p)| (std::cmp::Reverse(*d), p.0));
        let mut proposers: Vec<PlayerId> = host.into_iter().collect();
        proposers.extend(ranked.into_iter().map(|(_, p)| p));
        proposers.truncate(PROPOSERS);
        self.world.module.congress.proposers = proposers;
    }

    pub(crate) fn propose_resolution(
        &mut self,
        player: PlayerId,
        kind: ResolutionKind,
        choice: u32,
        repeal: bool,
    ) -> Result<(), CommandError> {
        self.world.module.congress.proposals.push(Proposal {
            kind,
            choice,
            proposer: Some(player),
            repeal,
        });
        self.relations_changed(player);
        Ok(())
    }

    pub(crate) fn cast_votes(
        &mut self,
        player: PlayerId,
        proposal: u8,
        choice: u32,
        delegates: u16,
    ) -> Result<(), CommandError> {
        self.world.module.congress.votes.push(Vote {
            voter: player,
            proposal,
            choice,
            delegates,
        });
        self.relations_changed(player);
        Ok(())
    }
}
