//! The congress lobby (W2, W3, W4) and the city-state poacher (CS2-CS5, S3, CS1). Both play through
//! the commands of a human's screens (`GiftGold`, `ProposeResolution`, `CastVotes`, `MoveSpy`,
//! `StageCoup`); the rules check each, and a refused command is skipped.
//!
//! The lobbyist buys city-states in the rounds before a session (the ones allied to the target first:
//! a flipped ally moves delegates), proposes an embargo on the target and votes all its delegates
//! against it. The poacher buys away the city-states allied to its victim, nearest first, and sends a
//! spy to each to stage a coup when the chance is fair.

use std::collections::{BTreeMap, BTreeSet};

use classic_rules::ai::espionage::GUARD_COUP_MIN_CHANCE;
use classic_rules::ids::PlayerId;
use classic_rules::knowledge::observation::{AIObservation, CityStateInfo};
use classic_rules::world::congress::{Proposal, ResolutionKind, NO, YES};
use classic_rules::world::espionage::SpyState;
use classic_rules::world::player::ClassicPlayers;
use classic_rules::{ClassicGame, Command, CompiledRuleset, GameState};
use serde::{Deserialize, Serialize};

use super::{scripted, Cast, Order, Trick};
use crate::bench::modules::majors;
use crate::stress::edit::Change;
use crate::stress::position::Position;
use crate::stress::scene::{Edit, Plan, Roles};
use crate::stress::seat::Seat;

/// The gold of one gift when the formula is not worked out (the AI's small gift).
const GIFT_CHUNK: u32 = 250;

/// Gifts one turn may make (a guard, and the gold runs out first).
const GIFTS_A_TURN: usize = 8;

/// The lobbyist buys city-states when a session is this many rounds away or nearer.
const ROUNDS_AHEAD: u32 = 3;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lobby {
    /// The lobbyist, the human.
    pub seat: u8,
    /// The nation the embargo is proposed on (the AI judged).
    pub target: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Poach {
    /// The poacher, the human.
    pub seat: u8,
    /// The nation whose city-state allies are bought away (the AI judged).
    pub victim: u8,
}

fn treasury(game: &GameState, p: PlayerId) -> u32 {
    game.view().auth.players().treasury_of(p)
}

/// What `rival` leads by in the city-state's influence over `me`, as the city-state screen shows `me`.
fn gap(game: &GameState, info: &CityStateInfo, rival: PlayerId) -> i32 {
    game.view().auth.module().city_states().influence(info.player, rival) - info.influence
}

/// The city-state to gift next, as `pick` chooses among those at peace with `me` that `me` does not hold.
fn pick_state(game: &GameState, rules: &CompiledRuleset, me: PlayerId, pick: impl Fn(&AIObservation<'_>, &[&CityStateInfo]) -> Option<PlayerId>) -> Option<PlayerId> {
    let obs = game.ai_observation(rules, me);
    let open: Vec<&CityStateInfo> = obs.city_states.iter().filter(|c| !c.at_war && c.ally != Some(me)).collect();
    pick(&obs, &open)
}

/// One gift of `GIFT_CHUNK` to `city_state`; the gold given (0 when the rules refuse).
fn gift(game: &mut GameState, rules: &CompiledRuleset, me: PlayerId, city_state: PlayerId) -> u32 {
    if treasury(game, me) < GIFT_CHUNK {
        return 0;
    }
    match game.execute(rules, Command::GiftGold { player: me, city_state, gold: GIFT_CHUNK }) {
        Ok(_) => GIFT_CHUNK,
        Err(_) => 0,
    }
}

/// Gifts while `pick` names a city-state and the gold lasts; the gold given.
fn gift_while(game: &mut GameState, rules: &CompiledRuleset, me: PlayerId, pick: impl Fn(&GameState, &[&CityStateInfo]) -> Option<PlayerId>) -> u32 {
    let mut given = 0;
    for _ in 0..GIFTS_A_TURN {
        let Some(city_state) = pick_state(game, rules, me, |_, open| pick(game, open)) else { break };
        let gold = gift(game, rules, me, city_state);
        if gold == 0 {
            break;
        }
        given += gold;
    }
    given
}

/// The nations the city-states hold as ally now.
fn allied_to(game: &GameState, rival: PlayerId) -> BTreeSet<u8> {
    let auth = game.view().auth;
    let states = auth.module().city_states();
    (0..auth.players().count() as u8).filter(|p| auth.players().is_city_state(PlayerId(*p)) && states.ally(PlayerId(*p)) == Some(rival)).collect()
}

// ---------------------------------------------------------------------------
// The lobby

pub struct LobbyPlay {
    order: Lobby,
    gifted: u32,
    proposed: u32,
    votes: u32,
}

impl LobbyPlay {
    pub fn new(order: Lobby) -> Self {
        LobbyPlay { order, gifted: 0, proposed: 0, votes: 0 }
    }

    fn me(&self) -> PlayerId {
        PlayerId(self.order.seat)
    }

    fn target(&self) -> PlayerId {
        PlayerId(self.order.target)
    }

    /// The city-states allied to the target first (the smallest gap first), then the one nearest to
    /// being won.
    fn next_to_buy(&self, game: &GameState, open: &[&CityStateInfo]) -> Option<PlayerId> {
        let target = self.target();
        let from_target = open.iter().filter(|c| c.ally == Some(target)).min_by_key(|c| (gap(game, c, target), c.player.0));
        if let Some(info) = from_target {
            return Some(info.player);
        }
        open.iter().filter(|c| c.ally != Some(target)).max_by_key(|c| (c.influence, std::cmp::Reverse(c.player.0))).map(|c| c.player)
    }

    fn propose(&mut self, game: &mut GameState, rules: &CompiledRuleset) {
        let command = Command::ProposeResolution { player: self.me(), kind: ResolutionKind::PlayerEmbargo, choice: u32::from(self.order.target), repeal: false };
        if game.execute(rules, command).is_ok() {
            self.proposed += 1;
        }
    }

    /// All the delegates left, spread over the proposals worth a vote (the first takes the rest).
    fn vote(&mut self, game: &mut GameState, rules: &CompiledRuleset, proposals: &[Proposal], delegates: u16) {
        let wanted: Vec<(u8, u32)> = proposals.iter().enumerate().filter_map(|(i, p)| choice_on(p, self.me(), self.target()).map(|c| (i as u8, c))).collect();
        if wanted.is_empty() || delegates == 0 {
            return;
        }
        let share = delegates / wanted.len() as u16;
        let rest = delegates % wanted.len() as u16;
        for (n, (proposal, choice)) in wanted.into_iter().enumerate() {
            let put = if n == 0 { share + rest } else { share };
            if put > 0 && game.execute(rules, Command::CastVotes { player: self.me(), proposal, choice, delegates: put }).is_ok() {
                self.votes += u32::from(put);
            }
        }
    }
}

/// What the lobbyist puts on a proposal: itself in an election, yes on an embargo of the target, no on
/// anything else the target proposes or on the repeal of an embargo of it; none on the rest.
fn choice_on(proposal: &Proposal, me: PlayerId, target: PlayerId) -> Option<u32> {
    if proposal.kind.elects() {
        return Some(u32::from(me.0));
    }
    let embargo = proposal.kind == ResolutionKind::PlayerEmbargo && proposal.choice == u32::from(target.0);
    match (embargo, proposal.repeal) {
        (true, false) => Some(YES),
        (true, true) => Some(NO),
        (false, _) if proposal.proposer == Some(target) => Some(NO),
        (false, _) => None,
    }
}

/// The congress as the lobbyist sees it: (in session, rounds to the next session, may propose, proposals, delegates left).
fn congress_of(game: &GameState, rules: &CompiledRuleset, me: PlayerId) -> Option<(bool, u32, bool, Vec<Proposal>, u16)> {
    let obs = game.ai_observation(rules, me);
    let c = obs.congress.as_ref()?;
    Some((c.in_session, c.turns_until_session, c.can_propose, c.proposals.clone(), c.delegates_left))
}

impl Seat for LobbyPlay {
    fn player(&self) -> PlayerId {
        self.me()
    }

    fn play(&mut self, game: &mut GameState, rules: &CompiledRuleset) {
        let Some((in_session, until, can_propose, proposals, delegates)) = congress_of(game, rules, self.me()) else { return };
        if in_session || until <= ROUNDS_AHEAD {
            let me = self.me();
            let order = LobbyPlay { order: self.order.clone(), gifted: 0, proposed: 0, votes: 0 };
            self.gifted += gift_while(game, rules, me, |g, open| order.next_to_buy(g, open));
        }
        if in_session {
            self.vote(game, rules, &proposals, delegates);
        } else if can_propose {
            self.propose(game, rules);
        }
    }

    fn counts(&self) -> BTreeMap<String, f64> {
        BTreeMap::from([("script_gifted".to_owned(), f64::from(self.gifted)), ("script_proposed".to_owned(), f64::from(self.proposed)), ("script_votes".to_owned(), f64::from(self.votes))])
    }
}

// ---------------------------------------------------------------------------
// The poacher

pub struct PoachPlay {
    order: Poach,
    gifted: u32,
    coups: u32,
    flipped: u32,
    /// The city-states the victim held at the last look (a flip is one of these going to the poacher).
    watched: BTreeSet<u8>,
}

impl PoachPlay {
    pub fn new(order: Poach) -> Self {
        PoachPlay { order, gifted: 0, coups: 0, flipped: 0, watched: BTreeSet::new() }
    }

    fn me(&self) -> PlayerId {
        PlayerId(self.order.seat)
    }

    fn victim(&self) -> PlayerId {
        PlayerId(self.order.victim)
    }

    /// Counts the city-states that went from the victim to the poacher since the last look.
    fn look(&mut self, game: &GameState) {
        let states = game.view().auth.module().city_states();
        let (me, victim) = (self.me(), self.victim());
        let mut flipped = 0;
        self.watched.retain(|p| match states.ally(PlayerId(*p)) {
            Some(a) if a == me => {
                flipped += 1;
                false
            }
            Some(a) => a == victim,
            None => false,
        });
        self.flipped += flipped;
        self.watched.extend(allied_to(game, victim));
    }

    /// The city-states allied to the victim, the smallest gap first.
    fn prey(&self, game: &GameState, rules: &CompiledRuleset) -> Vec<(PlayerId, Option<classic_rules::ids::CityHandle>)> {
        let obs = game.ai_observation(rules, self.me());
        let mut prey: Vec<(i32, u8, Option<classic_rules::ids::CityHandle>)> =
            obs.city_states.iter().filter(|c| !c.at_war && c.ally == Some(self.victim())).map(|c| (gap(game, c, self.victim()), c.player.0, c.capital_city)).collect();
        prey.sort_unstable_by_key(|(g, p, _)| (*g, *p));
        prey.into_iter().map(|(_, p, city)| (PlayerId(p), city)).collect()
    }

    /// A coup where a spy stands in a prey's capital with a fair chance; then every spy not at work on
    /// a prey goes to a prey capital no spy of ours holds.
    fn spy_work(&mut self, game: &mut GameState, rules: &CompiledRuleset) {
        let capitals: Vec<classic_rules::ids::CityHandle> = self.prey(game, rules).into_iter().filter_map(|(_, city)| city).collect();
        let spies = game.ai_observation(rules, self.me()).spies;
        let mut held: Vec<classic_rules::ids::CityHandle> = Vec::new();
        for spy in &spies {
            let Some(city) = spy.city.filter(|c| capitals.contains(c)) else { continue };
            held.push(city);
            if spy.coup_chance.is_some_and(|c| c >= GUARD_COUP_MIN_CHANCE) && game.execute(rules, Command::StageCoup { player: self.me(), spy: spy.index }).is_ok() {
                self.coups += 1;
            }
        }
        let mut free = spies.iter().filter(|s| !s.city.is_some_and(|c| held.contains(&c)) && !matches!(s.state, SpyState::Dead | SpyState::Travelling));
        for capital in capitals.into_iter().filter(|c| !held.contains(c)) {
            let Some(spy) = free.next() else { return };
            let _ = game.execute(rules, Command::MoveSpy { player: self.me(), spy: spy.index, city: Some(capital), diplomat: false });
        }
    }
}

impl Seat for PoachPlay {
    fn player(&self) -> PlayerId {
        self.me()
    }

    fn play(&mut self, game: &mut GameState, rules: &CompiledRuleset) {
        self.look(game);
        let (me, victim) = (self.me(), self.victim());
        self.gifted += gift_while(game, rules, me, |g, open| open.iter().filter(|c| c.ally == Some(victim)).min_by_key(|c| (gap(g, c, victim), c.player.0)).map(|c| c.player));
        self.spy_work(game, rules);
        self.look(game);
    }

    fn counts(&self) -> BTreeMap<String, f64> {
        BTreeMap::from([("script_gifted".to_owned(), f64::from(self.gifted)), ("script_coups".to_owned(), f64::from(self.coups)), ("script_flipped".to_owned(), f64::from(self.flipped))])
    }
}

// ---------------------------------------------------------------------------
// The scenes

/// The nations by score, the best first (the living majors).
fn by_score(game: &GameState, rules: &CompiledRuleset) -> Vec<PlayerId> {
    let auth = game.view().auth;
    let mut all: Vec<PlayerId> = majors(game).into_iter().filter(|p| auth.players().is_alive(*p)).collect();
    all.sort_by_key(|p| (std::cmp::Reverse(classic_rules::world::victory::score(auth, rules, *p)), p.0));
    all
}

/// The lobby: no congress, no trick. The target (judged) is the best by score, the lobbyist the second.
/// The poach: the victim is the nation with the most city-state allies (none: no trick), the poacher
/// the best by score among the others.
pub(super) fn plan(trick: Trick, rules: &CompiledRuleset, position: &Position, _instance: usize) -> Result<Plan, String> {
    let game = &position.game;
    let ranked = by_score(game, rules);
    let mut info = BTreeMap::new();
    let (seat, human) = match trick {
        Trick::Lobby => {
            if !game.view().auth.module().congress().founded() {
                return Err("the congress is not founded".to_owned());
            }
            let (target, lobbyist) = (ranked.first(), ranked.get(1));
            let (Some(target), Some(lobbyist)) = (target, lobbyist) else { return Err("fewer than two nations".to_owned()) };
            (*target, *lobbyist)
        }
        _ => {
            let victim = ranked.iter().copied().max_by_key(|p| (allied_to(game, *p).len(), std::cmp::Reverse(p.0))).ok_or("no nations")?;
            let allies = allied_to(game, victim).len();
            if allies == 0 {
                return Err("no nation has a city-state ally".to_owned());
            }
            let poacher = ranked.iter().copied().find(|p| *p != victim).ok_or("the victim has no rival")?;
            info.insert("victim_allies".to_owned(), allies.to_string());
            (victim, poacher)
        }
    };
    Ok(Plan { roles: Roles::Human(Cast { trick, seat: seat.0, human: human.0 }), base: Vec::new(), info })
}

/// The human gets 500, 1000 or 2000 gold and plays the script.
pub(super) fn edit_of(cast: Cast, level: usize, _rules: &CompiledRuleset, _position: &Position) -> Result<Edit, String> {
    let gold = [500, 1000, 2000][level];
    let order = match cast.trick {
        Trick::Lobby => Order::Lobby(Lobby { seat: cast.human, target: cast.seat }),
        _ => Order::Poach(Poach { seat: cast.human, victim: cast.seat }),
    };
    Ok(scripted(vec![Change::Treasury { player: cast.human, gold }], order))
}
