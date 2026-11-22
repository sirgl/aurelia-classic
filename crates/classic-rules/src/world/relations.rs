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
pub use crate::systems::diplomacy::deals::{gold_income, peace_needs_a_deal, DealBlock};

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
    /// The offer lapses when the round reaches this value (`systems::diplomacy::deals::offer_lifetime`).
    pub expires: u32,
}

/// Why an observer holds a grievance against another player: a wrong the observer remembers,
/// in opinion points that fade by themselves. The warmonger memory (`RelationsState::warmonger`)
/// is the world's memory of an aggressor; a grievance is the memory of the one who was wronged.
/// Each kind names its fading (Unciv `DiplomaticModifiers`: the `revertToZero` rate of
/// `DiplomacyTurnManager.nextTurnDiplomaticModifiers`); its trigger names the points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GrievanceKind {
    /// A tile of the observer's land was taken by a culture bomb (the citadel). Unciv:
    /// `StealingTerritory`, -10 per tile (`UniqueTriggerActivation.kt`, `OneTimeTakeOverTilesInRadius`),
    /// fading by 1/4 a turn (`DiplomacyTurnManager.kt`).
    StolenLand,
    /// The offender declared war on a nation it had declared friendship with, or on its own
    /// allied city-state. Unciv: `BetrayedDeclarationOfFriendship` (DeclareWar.kt
    /// `breakTreaties`, `handleCityStateDirectAttack`), fading by 1/8 a turn.
    BetrayedFriendship,
    /// The offender declared war on its defensive pact partner. Unciv: `BetrayedDefensivePact`
    /// (DeclareWar.kt `breakTreaties`; it adds to the friendship's), fading by 1/16 a turn.
    BetrayedPact,
}

/// How many kinds of grievance there are.
pub const GRIEVANCE_KINDS: usize = 3;

/// Opinion points a taken tile costs its former owner. Unciv: UniqueTriggerActivation.kt
/// (`OneTimeTakeOverTilesInRadius`, `StealingTerritory` -10); equal.
pub const STOLEN_TILE_POINTS: i32 = 10;

/// Opinion points the victim of a betrayed friendship holds. Unciv: DeclareWar.kt
/// (`breakTreaties`, -40 for the target, times the Loyal focus of the default personality, 1); equal.
pub const BETRAYED_FRIEND_POINTS: i32 = 40;

/// Opinion points every other nation that knows the betrayer of a friendship holds. Unciv:
/// DeclareWar.kt (`breakTreaties`, -20); equal.
pub const BETRAYED_FRIEND_WITNESS_POINTS: i32 = 20;

/// Opinion points the victim of a betrayed defensive pact holds. Unciv: DeclareWar.kt
/// (`breakTreaties`, -20 for the target); equal.
pub const BETRAYED_PACT_POINTS: i32 = 20;

/// Opinion points every other nation that knows the betrayer of a defensive pact holds. Unciv:
/// DeclareWar.kt (`breakTreaties`, -10); equal.
pub const BETRAYED_PACT_WITNESS_POINTS: i32 = 10;

/// Opinion points every nation that knows it holds against a nation that declared war on its own
/// allied city-state. Unciv: DeclareWar.kt (`handleCityStateDirectAttack`,
/// `BetrayedDeclarationOfFriendship` -10); equal.
pub const BETRAYED_ALLY_POINTS: i32 = 10;

/// Units one opinion point is kept in for a grievance, so that a fading of 0.67 x 1/4 a turn is exact.
/// Own value (storage only: the opinion modifiers of Unciv are floats).
pub const GRIEVANCE_POINT: i32 = 10_000;

/// Percent of its rate a grievance fades by, divided by the speed's pace. Unciv:
/// DiplomacyTurnManager.kt (`SPEED_ADJUSTMENT_NORMALIZATION_FACTOR` 0.67, `revertToZero`); equal.
pub const GRIEVANCE_FADING_PERCENT: i32 = 67;

impl GrievanceKind {
    pub fn all() -> [GrievanceKind; GRIEVANCE_KINDS] {
        [GrievanceKind::StolenLand, GrievanceKind::BetrayedFriendship, GrievanceKind::BetrayedPact]
    }

    pub fn index(self) -> usize {
        match self {
            GrievanceKind::StolenLand => 0,
            GrievanceKind::BetrayedFriendship => 1,
            GrievanceKind::BetrayedPact => 2,
        }
    }

    /// What the grievance fades by in a round at Standard pace, in [`GRIEVANCE_POINT`]s.
    pub const fn fading(self) -> i32 {
        match self {
            GrievanceKind::StolenLand => GRIEVANCE_POINT / 4,
            GrievanceKind::BetrayedFriendship => GRIEVANCE_POINT / 8,
            GrievanceKind::BetrayedPact => GRIEVANCE_POINT / 16,
        }
    }

    /// What the grievance fades by in a round at the pace `pace_percent` (Standard 100).
    pub fn fading_at(self, pace_percent: u32) -> i32 {
        let pace = i64::from(pace_percent.max(1));
        (i64::from(self.fading()) * i64::from(GRIEVANCE_FADING_PERCENT) / pace) as i32
    }

    /// The saved tag; `from_tag` is its inverse.
    pub fn tag(self) -> u8 {
        self.index() as u8
    }

    pub fn from_tag(tag: u8) -> Option<GrievanceKind> {
        Self::all().get(usize::from(tag)).copied()
    }
}

/// What `observer` holds against `offender` for one reason, in [`GRIEVANCE_POINT`]s (always > 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grievance {
    pub observer: PlayerId,
    pub offender: PlayerId,
    pub kind: GrievanceKind,
    pub amount: i32,
}

/// Why an offer left the list of its receiver without an answer. Not stored: the journal tells it
/// (`ClassicChange::OfferLapsed`) and the screen of the receiver says it once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OfferLapse {
    /// Nobody answered before the round it expires.
    Expired,
    /// The proposer or the receiver is gone from the game.
    NationGone,
    /// The two nations are at war now, and the offer is not a peace.
    WarBegan,
    /// The terms cannot be met any more (a city changed hands, the gold is spent, the peace is made).
    NoLongerPossible,
}

/// Why an offer that makes peace was refused, and what would turn the refusal into a yes.
/// The one who answers sets it (the AI states the very estimate it decided by, `ai::peace`),
/// the proposer is told. The rules do not read it: it never changes an answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PeaceRefusal {
    /// Nothing said: an acceptance, a human's answer, an offer without peace.
    #[default]
    Unsaid,
    /// The answerer remembers the proposer as an aggressor; the white peace is for `price` gold.
    Aggressor { price: u32 },
    /// The answerer is winning the war; the white peace is for `price` gold.
    Winning { price: u32 },
    /// The peace alone would pass; what the offer asks of the answerer besides it is too much.
    Terms,
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
    /// Grievances, sorted by (observer, offender, kind), one entry for each, all above 0.
    pub(crate) grievances: Vec<Grievance>,
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
            grievances: Vec::new(),
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

    /// What observer `a` holds against `b` for `kind`, in [`GRIEVANCE_POINT`]s.
    pub fn grievance(&self, a: PlayerId, b: PlayerId, kind: GrievanceKind) -> i32 {
        self.grievances
            .iter()
            .find(|g| g.observer == a && g.offender == b && g.kind == kind)
            .map_or(0, |g| g.amount)
    }

    /// What observer `a` holds against `b` by reason (index = `GrievanceKind::index`).
    pub fn grievances_about(&self, a: PlayerId, b: PlayerId) -> [i32; GRIEVANCE_KINDS] {
        GrievanceKind::all().map(|kind| self.grievance(a, b, kind))
    }

    pub fn grievances(&self) -> &[Grievance] {
        &self.grievances
    }

    /// Adds `delta` (any sign) to what `observer` holds against `offender` for `kind`; at 0 the
    /// grievance is gone.
    pub(crate) fn add_grievance(&mut self, observer: PlayerId, offender: PlayerId, kind: GrievanceKind, delta: i32) {
        let key = |g: &Grievance| (g.observer.0, g.offender.0, g.kind);
        match self.grievances.binary_search_by_key(&(observer.0, offender.0, kind), key) {
            Ok(i) => {
                let amount = self.grievances[i].amount.saturating_add(delta).max(0);
                if amount == 0 {
                    self.grievances.remove(i);
                } else {
                    self.grievances[i].amount = amount;
                }
            }
            Err(i) if delta > 0 => self.grievances.insert(i, Grievance { observer, offender, kind, amount: delta }),
            Err(_) => {}
        }
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

    /// Gold per turn `player` pays (negative) or receives by deals (a dead nation has none: its
    /// deals end at its death, `systems::elimination`).
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
