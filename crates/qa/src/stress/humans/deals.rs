//! The deal tricks: the swindle (D1, D3, D6, E2), the drain before a war (D2) and white-peace fishing
//! (D8). Every offer goes through `Command::ProposeDeal` (the rules check the items as they check a
//! human's trade screen) and the seat's AI answers it on its own turn; the script reads the answer
//! from the world at its next turn (a new running trade to the seat, the war's end, a friendship).

use std::collections::BTreeMap;

use classic_rules::derived::resources::resource_ledger;
use classic_rules::ids::{PlayerId, ResourceId};
use classic_rules::world::relations::DealItem;
use classic_rules::{Command, CompiledRuleset, GameState};
use kernel::rules::content::ResourceClass;
use serde::{Deserialize, Serialize};

use super::army;
use super::{scripted, Cast, Order, Trick};
use crate::stress::edit::{Accord, Change};
use crate::stress::position::Position;
use crate::stress::scene::{nearest_pair, Edit, Plan, Roles};
use crate::stress::seat::Seat;

/// A price below this is not asked (the trick is not worth a war).
const LEAST_PRICE: u32 = 10;

/// Friendship asked this many times after a peace before the swindler gives up.
const BEFRIEND_TRIES: u32 = 5;

/// The rounds of the window the drain's war starts at (both arms: the base).
const DRAIN_WAR_AFTER: u32 = 3;

/// The army that stands at the seat's border in both arms of the drain.
const DRAIN_ARMY: (usize, usize) = (3, 3);

fn treasury(game: &GameState, p: PlayerId) -> u32 {
    game.view().auth.players().treasury_of(p)
}

/// The running trades from `from` to `to`.
fn trades(game: &GameState, from: PlayerId, to: PlayerId) -> usize {
    let round = game.view().auth.turn().round;
    game.view().auth.module().relations().trades().iter().filter(|t| t.from == from && t.to == to && t.until > round).count()
}

/// What the human sells for the seat's gold: a luxury it can spare that the seat lacks, else any it
/// can spare, else a strategic one, else its gold a turn (half its income).
fn ware(game: &GameState, rules: &CompiledRuleset, human: PlayerId, seat: PlayerId) -> Option<DealItem> {
    let auth = game.view().auth;
    let (mine, theirs) = (resource_ledger(auth, rules, human), resource_ledger(auth, rules, seat));
    let spare = |class: ResourceClass| -> Vec<ResourceId> {
        (0..rules.resources.len() as u16).map(ResourceId).filter(|r| rules.resources[r.0 as usize].class == Some(class) && mine.tradeable(*r) > 0).collect()
    };
    let luxuries = spare(ResourceClass::Luxury);
    let pick = luxuries.iter().copied().find(|r| theirs.available(*r) <= 0).or_else(|| luxuries.first().copied()).or_else(|| spare(ResourceClass::Strategic).first().copied());
    if let Some(resource) = pick {
        return Some(DealItem::Resource { resource, amount: 1 });
    }
    let income = classic_rules::world::relations::gold_income(auth, rules, human);
    (income >= 2).then(|| DealItem::GoldPerTurn((income / 2) as u32))
}

/// Offers `ware` for `price` of the seat's gold; true when the rules let the offer stand.
fn offer(game: &mut GameState, rules: &CompiledRuleset, human: PlayerId, seat: PlayerId, ware: DealItem, price: u32) -> bool {
    let items = vec![(human, ware), (seat, DealItem::Gold(price))];
    game.execute(rules, Command::ProposeDeal { from: human, to: seat, items }).is_ok()
}

/// A mutual item (peace, friendship) offered to the seat.
fn ask(game: &mut GameState, rules: &CompiledRuleset, human: PlayerId, seat: PlayerId, item: DealItem) -> bool {
    game.execute(rules, Command::ProposeDeal { from: human, to: seat, items: vec![(human, item)] }).is_ok()
}

// ---------------------------------------------------------------------------
// The swindle

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Swindle {
    pub seat: u8,
    pub victim: u8,
    /// The first price, a percent of the victim's treasury (it halves at each refusal).
    pub percent: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SwindleStage {
    /// The next offer; `refusals` so far in this cycle.
    Offer { refusals: u32 },
    /// An offer of `price` stands; `trades` running from the human to the seat before it.
    Offered { refusals: u32, price: u32, trades: usize },
    /// At war: peace asked every round (the rules refuse it before the war's tenth round).
    War,
    /// At peace again: friendship asked, `tries` so far.
    Befriend { tries: u32 },
    /// The seat refuses friendship: no more cycles.
    Done,
}

pub struct SwindlePlay {
    order: Swindle,
    stage: SwindleStage,
    offers: u32,
    paid: u32,
    gold: u32,
}

impl SwindlePlay {
    pub fn new(order: Swindle) -> Self {
        SwindlePlay { order, stage: SwindleStage::Offer { refusals: 0 }, offers: 0, paid: 0, gold: 0 }
    }

    fn sell(&mut self, game: &mut GameState, rules: &CompiledRuleset, refusals: u32) -> SwindleStage {
        let (human, seat) = (PlayerId(self.order.seat), PlayerId(self.order.victim));
        let price = (u64::from(treasury(game, seat)) * u64::from(self.order.percent) / 100 >> refusals.min(6)) as u32;
        let Some(ware) = ware(game, rules, human, seat).filter(|_| price >= LEAST_PRICE) else { return SwindleStage::Offer { refusals } };
        let before = trades(game, human, seat);
        if !offer(game, rules, human, seat, ware, price) {
            return SwindleStage::Offer { refusals: refusals + 1 };
        }
        self.offers += 1;
        SwindleStage::Offered { refusals, price, trades: before }
    }
}

impl Seat for SwindlePlay {
    fn player(&self) -> PlayerId {
        PlayerId(self.order.seat)
    }

    fn play(&mut self, game: &mut GameState, rules: &CompiledRuleset) {
        let (human, seat) = (PlayerId(self.order.seat), PlayerId(self.order.victim));
        let round = game.view().auth.turn().round;
        let at_war = game.view().auth.diplomacy().at_war(human, seat);
        self.stage = match self.stage {
            SwindleStage::Offer { refusals } => self.sell(game, rules, refusals),
            SwindleStage::Offered { refusals, price, trades: before } => {
                if trades(game, human, seat) > before {
                    self.paid += 1;
                    self.gold += price;
                    if army::war(game, rules, human, seat) { SwindleStage::War } else { SwindleStage::Done }
                } else {
                    self.sell(game, rules, refusals + 1)
                }
            }
            SwindleStage::War if !at_war => SwindleStage::Befriend { tries: 0 },
            SwindleStage::War => {
                ask(game, rules, human, seat, DealItem::Peace);
                SwindleStage::War
            }
            SwindleStage::Befriend { .. } if game.view().auth.module().relations().are_friends(human, seat, round) => self.sell(game, rules, 0),
            SwindleStage::Befriend { tries } if tries >= BEFRIEND_TRIES => SwindleStage::Done,
            SwindleStage::Befriend { tries } => {
                ask(game, rules, human, seat, DealItem::Friendship);
                SwindleStage::Befriend { tries: tries + 1 }
            }
            SwindleStage::Done => SwindleStage::Done,
        };
    }

    fn counts(&self) -> BTreeMap<String, f64> {
        let flag = |b: bool| f64::from(u8::from(b));
        BTreeMap::from([
            ("script_offers".to_owned(), f64::from(self.offers)),
            ("script_paid".to_owned(), flag(self.paid >= 1)),
            ("script_paid_again".to_owned(), flag(self.paid >= 2)),
            ("script_cycles".to_owned(), f64::from(self.paid)),
            ("script_gold_taken".to_owned(), f64::from(self.gold)),
        ])
    }
}

// ---------------------------------------------------------------------------
// The drain

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Drain {
    pub seat: u8,
    pub victim: u8,
    pub percent: u32,
}

pub struct DrainPlay {
    order: Drain,
    turn: u32,
    /// The standing offer's price and the trades before it.
    standing: Option<(u32, usize)>,
    offers: u32,
    paid: bool,
    gold: u32,
}

impl DrainPlay {
    pub fn new(order: Drain) -> Self {
        DrainPlay { order, turn: 0, standing: None, offers: 0, paid: false, gold: 0 }
    }
}

impl Seat for DrainPlay {
    fn player(&self) -> PlayerId {
        PlayerId(self.order.seat)
    }

    /// Before the war: the human's gold a turn (what its income carries, at most a twentieth of the
    /// price) for a lump sum of the seat's gold; the price halves after a refusal.
    fn play(&mut self, game: &mut GameState, rules: &CompiledRuleset) {
        let (human, seat) = (PlayerId(self.order.seat), PlayerId(self.order.victim));
        self.turn += 1;
        if let Some((price, before)) = self.standing.take() {
            if trades(game, human, seat) > before {
                self.paid = true;
                self.gold += price;
            }
        }
        if self.paid || self.turn >= DRAIN_WAR_AFTER {
            return;
        }
        let price = (u64::from(treasury(game, seat)) * u64::from(self.order.percent) / 100 >> (self.turn - 1)) as u32;
        let income = classic_rules::world::relations::gold_income(game.view().auth, rules, human).max(0) as u32;
        let per_turn = (price / 20).clamp(1, income.max(1));
        if price < LEAST_PRICE || income == 0 {
            return;
        }
        let before = trades(game, human, seat);
        if offer(game, rules, human, seat, DealItem::GoldPerTurn(per_turn), price) {
            self.offers += 1;
            self.standing = Some((price, before));
        }
    }

    fn counts(&self) -> BTreeMap<String, f64> {
        BTreeMap::from([
            ("script_offers".to_owned(), f64::from(self.offers)),
            ("script_paid".to_owned(), f64::from(u8::from(self.paid))),
            ("script_gold_taken".to_owned(), f64::from(self.gold)),
        ])
    }
}

// ---------------------------------------------------------------------------
// White-peace fishing

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Beg {
    /// The losing side, the human.
    pub seat: u8,
    /// The winning side, the AI judged.
    pub victim: u8,
}

pub struct BegPlay {
    order: Beg,
    asks: u32,
    white_peace: bool,
    /// The peace came with the human's gold or gold a turn (the human seat's own AI offered it): not white.
    paid_peace: bool,
    /// The human's treasury and its running trades to the winner at its last turn of the war.
    last: Option<(u32, usize)>,
}

impl BegPlay {
    pub fn new(order: Beg) -> Self {
        BegPlay { order, asks: 0, white_peace: false, paid_peace: false, last: None }
    }
}

impl Seat for BegPlay {
    fn player(&self) -> PlayerId {
        PlayerId(self.order.seat)
    }

    /// Every round of the war: white peace asked (the rules refuse it before the war's tenth round).
    /// A peace that came after the human's last ask is a white peace, unless the human paid for it: its treasury fell
    /// by `LEAST_PRICE` or more since its last turn of the war, or a new trade of it to the winner runs (the human
    /// seat's own AI offers peace with gold, and the winner may take that one; CHAIN.md D8, casebook E1).
    fn play(&mut self, game: &mut GameState, rules: &CompiledRuleset) {
        let (human, winner) = (PlayerId(self.order.seat), PlayerId(self.order.victim));
        if !game.view().auth.diplomacy().at_war(human, winner) {
            if self.asks > 0 && !self.white_peace && !self.paid_peace {
                let paid = self.last.is_some_and(|(gold, deals)| treasury(game, human) + LEAST_PRICE <= gold || trades(game, human, winner) > deals);
                if paid {
                    self.paid_peace = true;
                } else {
                    self.white_peace = true;
                }
            }
            return;
        }
        if ask(game, rules, human, winner, DealItem::Peace) {
            self.asks += 1;
        }
        self.last = Some((treasury(game, human), trades(game, human, winner)));
    }

    fn counts(&self) -> BTreeMap<String, f64> {
        BTreeMap::from([
            ("script_asks".to_owned(), f64::from(self.asks)),
            ("script_white_peace".to_owned(), f64::from(u8::from(self.white_peace))),
            ("script_paid_peace".to_owned(), f64::from(u8::from(self.paid_peace))),
        ])
    }
}

// ---------------------------------------------------------------------------
// The scenes

/// The nearest pair: the seat (judged) and the human by the instance. The swindle and the drain need
/// them at peace and friends (the base: gold needs friendship); the drain adds the human's army at the
/// border and its war three rounds in (both arms); white-peace fishing puts them at war, the human
/// losing (two cities to the seat, three quarters of its army gone: both arms).
pub(super) fn plan(trick: Trick, rules: &CompiledRuleset, position: &Position, instance: usize) -> Result<Plan, String> {
    let game = &position.game;
    let (a, b, d) = nearest_pair(game).ok_or("no two nations share a landmass")?;
    let (seat, human) = if instance % 2 == 1 { (b, a) } else { (a, b) };
    if game.view().auth.diplomacy().at_war(seat, human) {
        return Err("the nearest pair is at war already".to_owned());
    }
    let mut base = vec![Change::Meet { a: seat.0, b: human.0 }];
    match trick {
        Trick::Swindle => base.push(Change::Accord { giver: seat.0, receiver: human.0, accord: Accord::Friendship }),
        Trick::Drain => {
            base.push(Change::Accord { giver: seat.0, receiver: human.0, accord: Accord::Friendship });
            let target = crate::battle::front_city(game, human, seat).ok_or("the seat has no city on the human's landmass")?;
            let (kinds, _) = army::army_kinds(game, rules, human)?;
            base.extend(super::campaign::raise(game, rules, human, target, kinds, DRAIN_ARMY)?);
            base.push(Change::Later { after: DRAIN_WAR_AFTER, then: Box::new(Change::War { a: human.0, b: seat.0 }) });
        }
        _ => {
            base.push(Change::War { a: seat.0, b: human.0 });
            base.extend(losing(game, rules, human, seat)?);
        }
    }
    let info = BTreeMap::from([("pair_distance".to_owned(), d.to_string())]);
    Ok(Plan { roles: Roles::Human(Cast { trick, seat: seat.0, human: human.0 }), base, info })
}

/// The loser's two cities nearest to the winner (never its capital; one when it holds two) and three
/// quarters of its military units (every fourth kept, by id).
fn losing(game: &GameState, rules: &CompiledRuleset, loser: PlayerId, winner: PlayerId) -> Result<Vec<Change>, String> {
    let auth = game.view().auth;
    let capital = classic_rules::combat::city::capital_of(auth, rules, loser);
    let mut cities: Vec<(i32, u64)> = auth
        .indexes()
        .cities_of(loser)
        .iter()
        .filter(|h| Some(**h) != capital)
        .filter_map(|h| auth.cities().get(*h))
        .map(|c| (crate::stress::scene::nearest_on_land(game, winner, c.tile).unwrap_or(i32::MAX), c.id.0))
        .collect();
    if cities.is_empty() {
        return Err("the loser holds only its capital".to_owned());
    }
    cities.sort_unstable();
    let mut changes: Vec<Change> = cities.into_iter().take(2).map(|(_, city)| Change::CaptureCity { city, conqueror: winner.0 }).collect();
    let ids = crate::stress::edit::military_units(game, rules, loser).into_iter().enumerate().filter(|(i, _)| i % 4 != 0).map(|(_, id)| id).collect();
    changes.push(Change::RemoveUnits { ids });
    Ok(changes)
}

/// The swindle and the drain ask 30, 60 or 90 % of the seat's treasury; white peace has one level.
pub(super) fn edit_of(cast: Cast, level: usize, _rules: &CompiledRuleset, _position: &Position) -> Result<Edit, String> {
    let percent = [30, 60, 90][level];
    let order = match cast.trick {
        Trick::Swindle => Order::Swindle(Swindle { seat: cast.human, victim: cast.seat, percent }),
        Trick::Drain => Order::Drain(Drain { seat: cast.human, victim: cast.seat, percent }),
        _ => Order::Beg(Beg { seat: cast.human, victim: cast.seat }),
    };
    Ok(scripted(Vec::new(), order))
}
