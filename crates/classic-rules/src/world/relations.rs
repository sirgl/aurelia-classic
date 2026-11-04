//! Diplomatic relations between players beyond war and borders
//! (classic/docs/classic/rules.md): peace treaties, embassies,
//! defensive pacts, research agreements, declarations of friendship,
//! denunciations, warmonger memory, ongoing trades and pending deal
//! offers. Authoritative: persisted and hashed.
//!
//! Durations are stored as the round until which a relation holds
//! (exclusive): a relation is active while `round < until`, so nothing has
//! to tick it. Pair matrices are row-major `players × players`; symmetric
//! relations are written on both halves.

use crate::ids::{CityHandle, PlayerId, ResourceId};
pub use crate::systems::diplomacy::deals::DealBlock;

/// One item of a deal and the side that gives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DealItem {
    /// Gold now.
    Gold(u32),
    /// Gold every turn of the giver for `DEAL_DURATION` rounds.
    GoldPerTurn(u32),
    /// Copies of a luxury or strategic resource for `DEAL_DURATION` rounds.
    Resource { resource: ResourceId, amount: u16 },
    /// A city (not the capital).
    City(CityHandle),
    /// The receiver gets an embassy in the giver's capital.
    Embassy,
    /// The giver opens its borders to the receiver.
    OpenBorders,
    /// Both sides: a defensive pact.
    DefensivePact,
    /// Both sides: a research agreement (each pays the era's cost).
    ResearchAgreement,
    /// Both sides: a declaration of friendship.
    Friendship,
    /// Both sides: peace (and a peace treaty).
    Peace,
    /// The giver declares war on a third player.
    ThirdPartyWar(PlayerId),
    /// The giver makes peace with a third player.
    ThirdPartyPeace(PlayerId),
}

impl DealItem {
    /// Items that bind both sides (one entry stands for both).
    pub fn is_mutual(self) -> bool {
        matches!(
            self,
            DealItem::DefensivePact
                | DealItem::ResearchAgreement
                | DealItem::Friendship
                | DealItem::Peace
        )
    }
}

/// A deal offered by `from` to `to`, waiting for `to`'s answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DealOffer {
    pub id: u32,
    pub from: PlayerId,
    pub to: PlayerId,
    /// (giver, item); a mutual item is listed once.
    pub items: Vec<(PlayerId, DealItem)>,
    /// The offer lapses at the end of this round.
    pub expires: u32,
}

/// A running trade item of an accepted deal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OngoingTrade {
    pub from: PlayerId,
    pub to: PlayerId,
    pub flow: TradeFlow,
    /// The trade ends when the round reaches this value.
    pub until: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TradeFlow {
    GoldPerTurn(u32),
    Resource { resource: ResourceId, amount: u16 },
}

/// Pair data of relations (see the module docs).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RelationsState {
    pub(crate) players: usize,
    /// Peace treaty: no war before this round (symmetric).
    pub(crate) peace_until: Vec<u32>,
    /// Round the war began (symmetric; meaningful while at war).
    pub(crate) war_since: Vec<u32>,
    /// a has an embassy in b's capital (directed).
    pub(crate) embassy: Vec<u8>,
    /// Defensive pact until (symmetric).
    pub(crate) pact_until: Vec<u32>,
    /// Research agreement until (directed: each side is paid at its turn).
    pub(crate) research_until: Vec<u32>,
    /// Science a has made during its agreement with b (directed).
    pub(crate) research_points: Vec<u32>,
    /// Declaration of friendship until (symmetric).
    pub(crate) friends_until: Vec<u32>,
    /// a denounced b, until (directed).
    pub(crate) denounced_until: Vec<u32>,
    /// Open borders a → b expire at this round (directed; 0 = no expiry).
    pub(crate) open_until: Vec<u32>,
    /// Warmonger memory of observer a about player b (directed, ≥ 0).
    pub(crate) warmonger: Vec<i32>,
    pub(crate) trades: Vec<OngoingTrade>,
    pub(crate) offers: Vec<DealOffer>,
    pub(crate) next_offer: u32,
}

impl RelationsState {
    pub fn new(players: usize) -> Self {
        let n = players * players;
        Self {
            players,
            peace_until: vec![0; n],
            war_since: vec![0; n],
            embassy: vec![0; n],
            pact_until: vec![0; n],
            research_until: vec![0; n],
            research_points: vec![0; n],
            friends_until: vec![0; n],
            denounced_until: vec![0; n],
            open_until: vec![0; n],
            warmonger: vec![0; n],
            trades: Vec::new(),
            offers: Vec::new(),
            next_offer: 0,
        }
    }

    pub(crate) fn slot(&self, a: PlayerId, b: PlayerId) -> Option<usize> {
        let (a, b) = (a.0 as usize, b.0 as usize);
        (a < self.players && b < self.players).then_some(a * self.players + b)
    }

    fn get(v: &[u32], i: Option<usize>) -> u32 {
        i.and_then(|i| v.get(i).copied()).unwrap_or(0)
    }

    pub(crate) fn set_pair(v: &mut [u32], players: usize, a: PlayerId, b: PlayerId, value: u32) {
        let (a, b) = (a.0 as usize, b.0 as usize);
        if a < players && b < players {
            v[a * players + b] = value;
            v[b * players + a] = value;
        }
    }

    pub fn peace_treaty_until(&self, a: PlayerId, b: PlayerId) -> u32 {
        Self::get(&self.peace_until, self.slot(a, b))
    }

    pub fn war_since(&self, a: PlayerId, b: PlayerId) -> u32 {
        Self::get(&self.war_since, self.slot(a, b))
    }

    /// `a` has an embassy in `b`'s capital.
    pub fn has_embassy(&self, a: PlayerId, b: PlayerId) -> bool {
        self.slot(a, b).is_some_and(|i| self.embassy[i] == 1)
    }

    pub fn has_pact(&self, a: PlayerId, b: PlayerId, round: u32) -> bool {
        round < Self::get(&self.pact_until, self.slot(a, b))
    }

    pub fn has_research_agreement(&self, a: PlayerId, b: PlayerId, round: u32) -> bool {
        round < Self::get(&self.research_until, self.slot(a, b))
    }

    pub fn research_agreement_until(&self, a: PlayerId, b: PlayerId) -> u32 {
        Self::get(&self.research_until, self.slot(a, b))
    }

    pub fn research_points(&self, a: PlayerId, b: PlayerId) -> u32 {
        Self::get(&self.research_points, self.slot(a, b))
    }

    pub fn are_friends(&self, a: PlayerId, b: PlayerId, round: u32) -> bool {
        round < Self::get(&self.friends_until, self.slot(a, b))
    }

    /// `a` denounced `b` and the denunciation still holds.
    pub fn denounced(&self, a: PlayerId, b: PlayerId, round: u32) -> bool {
        round < Self::get(&self.denounced_until, self.slot(a, b))
    }

    pub fn open_borders_until(&self, a: PlayerId, b: PlayerId) -> u32 {
        Self::get(&self.open_until, self.slot(a, b))
    }

    /// Observer `a`'s warmonger memory of `b`.
    pub fn warmonger(&self, a: PlayerId, b: PlayerId) -> i32 {
        self.slot(a, b).map(|i| self.warmonger[i]).unwrap_or(0)
    }

    pub fn trades(&self) -> &[OngoingTrade] {
        &self.trades
    }

    pub fn offers(&self) -> &[DealOffer] {
        &self.offers
    }

    pub fn offer(&self, id: u32) -> Option<&DealOffer> {
        self.offers.iter().find(|o| o.id == id)
    }

    pub fn next_offer_id(&self) -> u32 {
        self.next_offer
    }

    /// Nothing differs from a new game (the hash mixes the state only then).
    pub fn is_pristine(&self) -> bool {
        *self == RelationsState::new(self.players)
    }

    /// Copies of `resource` `player` exports minus imports by deals.
    pub fn resource_balance(&self, player: PlayerId, resource: ResourceId, round: u32) -> i32 {
        let mut balance = 0;
        for t in &self.trades {
            let TradeFlow::Resource {
                resource: r,
                amount,
            } = t.flow
            else {
                continue;
            };
            if r != resource || round >= t.until {
                continue;
            }
            if t.to == player {
                balance += i32::from(amount);
            }
            if t.from == player {
                balance -= i32::from(amount);
            }
        }
        balance
    }

    /// Gold per turn `player` pays (negative) or receives by deals.
    pub fn gold_per_turn(&self, player: PlayerId, round: u32) -> i32 {
        let mut total = 0;
        for t in &self.trades {
            let TradeFlow::GoldPerTurn(gold) = t.flow else {
                continue;
            };
            if round >= t.until {
                continue;
            }
            if t.to == player {
                total += gold as i32;
            }
            if t.from == player {
                total -= gold as i32;
            }
        }
        total
    }
}
