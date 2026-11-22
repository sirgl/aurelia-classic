//! The deal tricks of wave 3 beyond P4 (group P4 of work/human-playbook-triage.md): a friend sells a
//! luxury for a lump sum and declares war (P4L, trick D1); a friend pays the victim to declare war on
//! a third nation and then declares war on the victim (P4B, trick D4). Both are one state machine:
//! offer the bait, wait for the victim to take it, betray.

use std::collections::BTreeMap;

use classic_rules::ids::{PlayerId, ResourceId, TileId};
use classic_rules::world::relations::{DealItem, TradeFlow};
use classic_rules::{Command, CompiledRuleset, GameState};
use kernel::rules::content::ResourceClass;

use super::Seat;

/// What the trickster offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Bait {
    /// A luxury the victim lacks, for `percent` of its treasury at once.
    Luxury { percent: u32 },
    /// `gold` for the victim's war on `third`.
    Bribe { third: PlayerId, gold: u32 },
}

/// Where the trick stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    /// Offering (each round; a lump-sum price halves after each refusal).
    Offer,
    /// The victim took the bait: war at the seat's next turn.
    Taken,
    /// War declared: the script is done.
    Betrayed,
}

/// A luxury `seller` holds on its land and `buyer` holds on none of its tiles (the lowest id).
pub(crate) fn luxury_to_sell(game: &GameState, rules: &CompiledRuleset, seller: PlayerId, buyer: PlayerId) -> Option<ResourceId> {
    let map = game.view().auth.map();
    let held_by = |player: PlayerId, resource: ResourceId| (0..map.tile_count() as u32).map(TileId).any(|t| map.owner(t).flatten() == Some(player) && map.resource(t).flatten() == Some(resource));
    (0..rules.resources.len() as u16)
        .map(ResourceId)
        .filter(|r| rules.resources[r.0 as usize].class == Some(ResourceClass::Luxury))
        .find(|r| held_by(seller, *r) && !held_by(buyer, *r))
}

pub(super) struct Betrayal {
    seat: PlayerId,
    victim: PlayerId,
    bait: Bait,
    stage: Stage,
    offers: u32,
    /// The victim's treasury before the offer (a lump sum is read as the fall of it).
    treasury_before: u32,
    gold_taken: u32,
}

impl Betrayal {
    pub(super) fn new(seat: PlayerId, victim: PlayerId, bait: Bait) -> Self {
        Betrayal { seat, victim, bait, stage: Stage::Offer, offers: 0, treasury_before: 0, gold_taken: 0 }
    }

    /// The offer of this round; false when there is nothing to offer.
    fn offer(&mut self, game: &mut GameState, rules: &CompiledRuleset) -> bool {
        let treasury = game.view().auth.players().treasury_of(self.victim);
        let items = match self.bait {
            Bait::Luxury { percent } => {
                let price = (u64::from(treasury) * u64::from(percent) / 100 >> self.offers.min(6)) as u32;
                let Some(resource) = luxury_to_sell(game, rules, self.seat, self.victim) else { return false };
                if price < 10 {
                    return false;
                }
                vec![(self.seat, DealItem::Resource { resource, amount: 1 }), (self.victim, DealItem::Gold(price))]
            }
            Bait::Bribe { third, gold } => {
                // What is left of the bribe after the seat's AI spent from the treasury.
                let purse = gold.min(game.view().auth.players().treasury_of(self.seat));
                if purse < 50 {
                    return false;
                }
                vec![(self.seat, DealItem::Gold(purse)), (self.victim, DealItem::ThirdPartyWar(third))]
            }
        };
        self.offers += 1;
        self.treasury_before = treasury;
        let _ = game.execute(rules, Command::ProposeDeal { from: self.seat, to: self.victim, items });
        true
    }

    /// Whether the victim took the bait since the offer.
    fn taken(&self, game: &GameState) -> bool {
        let auth = game.view().auth;
        match self.bait {
            Bait::Luxury { .. } => auth.module().relations().trades().iter().any(|t| t.from == self.seat && t.to == self.victim && matches!(t.flow, TradeFlow::Resource { .. })),
            Bait::Bribe { third, .. } => auth.diplomacy().at_war(self.victim, third),
        }
    }
}

impl Seat for Betrayal {
    fn player(&self) -> PlayerId {
        self.seat
    }

    fn play(&mut self, game: &mut GameState, rules: &CompiledRuleset) {
        match self.stage {
            Stage::Offer => {
                if self.offers > 0 && self.taken(game) {
                    let now = game.view().auth.players().treasury_of(self.victim);
                    self.gold_taken = match self.bait {
                        Bait::Luxury { .. } => self.treasury_before.saturating_sub(now),
                        Bait::Bribe { gold, .. } => gold,
                    };
                    self.stage = Stage::Taken;
                    self.play(game, rules);
                    return;
                }
                self.offer(game, rules);
            }
            Stage::Taken => {
                if game.execute(rules, Command::DeclareWar { player: self.seat, enemy: self.victim }).is_ok() {
                    self.stage = Stage::Betrayed;
                }
            }
            Stage::Betrayed => {}
        }
    }

    fn counts(&self) -> BTreeMap<String, f64> {
        let betrayed = matches!(self.stage, Stage::Betrayed);
        BTreeMap::from([
            ("script_offers".to_owned(), f64::from(self.offers)),
            ("script_bait_taken".to_owned(), f64::from(u8::from(self.stage != Stage::Offer))),
            ("script_betrayed".to_owned(), f64::from(u8::from(betrayed))),
            ("script_gold_moved".to_owned(), f64::from(self.gold_taken)),
        ])
    }
}
