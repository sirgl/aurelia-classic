//! Read-only models of the world screens (diplomacy, city-states, World Congress,
//! espionage, victory, end of the game) and of the combat overlays on the map (attack
//! odds, promotions, captured cities). `game::Session` builds them from the player's
//! knowledge; widgets draw them and answer with `GameRequest`s.
use classic_rules::command::CityDecision;
use classic_rules::ids::{CityHandle, PlayerId, PromotionId, TechId, TileId};
use classic_rules::world::congress::ResolutionKind;
use classic_rules::world::relations::DealItem;

use super::model::{Availability, GameRequest};

/// How an attack is likely to end, for the colour of the odds panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outlook {
    Favorable,
    Even,
    Unfavorable,
}

/// Expected result of the selected unit's attack on the hovered tile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CombatOdds {
    pub attacker: String,
    pub defender: String,
    /// Combat strengths with one decimal ("12.5").
    pub attacker_strength: String,
    pub defender_strength: String,
    /// (lowest, expected, highest) damage.
    pub to_defender: (u16, u16, u16),
    pub to_attacker: (u16, u16, u16),
    pub attacker_hp: u16,
    pub defender_hp: u16,
    pub verdict: &'static str,
    pub outlook: Outlook,
    pub ranged: bool,
}

/// A promotion the selected unit may take now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromotionChoice {
    pub id: PromotionId,
    pub key: String,
    pub name: String,
}

/// One of the player's cities waiting for the conqueror's decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureChoice {
    pub tile: TileId,
    pub city: String,
    pub founder: String,
    pub options: Vec<CaptureOption>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureOption {
    pub decision: CityDecision,
    pub label: &'static str,
    pub hint: &'static str,
    pub availability: Availability,
}

/// Ancient Ruins under one of the player's units: the three rewards to pick from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuinsChoice {
    pub tile: TileId,
    pub options: Vec<RuinsOption>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuinsOption {
    pub reward: classic_rules::ruins::Reward,
    pub label: &'static str,
    pub hint: String,
    pub availability: Availability,
}

/// The diplomacy screen: every nation met, the trade items with each, and the
/// offers waiting for the player's answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiplomacyView {
    pub outgoing: Vec<DealDraft>,
    /// Exact last-frame draft checked by the real command validator.
    pub preview: Option<(DealDraft, Availability)>,
    /// The player (the giver of its own items and of the items both sides sign).
    pub me: PlayerId,
    pub gold: u32,
    pub nations: Vec<NationCard>,
    pub offers: Vec<OfferCard>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NationCard {
    pub player: PlayerId,
    pub name: String,
    pub leader: String,
    /// Artwork key of the nation ("art:rome").
    pub art: String,
    pub at_war: bool,
    /// "At war since turn 12", "Peace treaty until turn 40", "At peace".
    pub state: String,
    pub treaties: Vec<String>,
    pub score: i64,
    pub gold: u32,
    pub declare_war: Availability,
    pub make_peace: Availability,
    pub denounce: Availability,
    pub open_borders: Availability,
    /// What the player may give, what the other side may give, what binds both.
    pub ours: Vec<DealOption>,
    pub theirs: Vec<DealOption>,
    pub mutual: Vec<DealOption>,
}

/// One item of the trade panel. Gold items carry the amount the panel offers by
/// default; the panel may change it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DealOption {
    /// Available quantity from the core ledger/income; None for treaties.
    pub max_amount: Option<u32>,
    pub item: DealItem,
    pub label: String,
    pub availability: Availability,
}

/// An offer of another nation waiting for the player's answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfferCard {
    pub id: u32,
    pub from: String,
    pub expires: u32,
    /// "They give: 100 gold", "You give: Iron", "Both: Open borders".
    pub lines: Vec<String>,
}

/// A deal being put together on the diplomacy screen (UI state).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DealDraft {
    pub partner: PlayerId,
    pub items: Vec<(PlayerId, DealItem)>,
    /// Independent amounts per giver and item; UI state only.
    pub amounts: Vec<(PlayerId, DealItem)>,
}

impl DealDraft {
    pub fn new(partner: PlayerId) -> Self {
        Self {
            partner,
            items: Vec::new(),
            amounts: Vec::new(),
        }
    }

    pub fn contains(&self, giver: PlayerId, item: DealItem) -> bool {
        self.items
            .iter()
            .any(|(g, listed)| *g == giver && same_slot(*listed, item))
    }

    pub fn amount(&self, giver: PlayerId, item: DealItem) -> u32 {
        let selected = self
            .amounts
            .iter()
            .find(|(g, i)| *g == giver && same_slot(*i, item))
            .map(|(_, i)| *i)
            .unwrap_or(item);
        match selected {
            DealItem::Gold(n) | DealItem::GoldPerTurn(n) => n.max(1),
            DealItem::Resource { amount, .. } => u32::from(amount.max(1)),
            _ => 1,
        }
    }

    pub fn with_amount(item: DealItem, amount: u32) -> DealItem {
        match item {
            DealItem::Gold(_) => DealItem::Gold(amount),
            DealItem::GoldPerTurn(_) => DealItem::GoldPerTurn(amount),
            DealItem::Resource { resource, .. } => DealItem::Resource {
                resource,
                amount: amount.min(u16::MAX as u32) as u16,
            },
            other => other,
        }
    }

    pub fn set_amount(&mut self, giver: PlayerId, item: DealItem, amount: u32) {
        let changed = Self::with_amount(item, amount);
        self.amounts
            .retain(|(g, i)| !(*g == giver && same_slot(*i, item)));
        self.amounts.push((giver, changed));
        for (g, i) in &mut self.items {
            if *g == giver && same_slot(*i, item) {
                *i = changed;
            }
        }
    }

    pub fn toggle(&mut self, giver: PlayerId, item: DealItem) {
        if self.contains(giver, item) {
            self.items
                .retain(|(g, listed)| !(*g == giver && same_slot(*listed, item)));
        } else {
            self.items
                .push((giver, Self::with_amount(item, self.amount(giver, item))));
        }
    }

    /// The proposal to send, once the deal holds an item.
    pub fn request(&self) -> Option<GameRequest> {
        if self.items.is_empty() {
            return None;
        }
        Some(GameRequest::ProposeDeal {
            to: self.partner,
            items: self.items.clone(),
        })
    }
}

/// Two items fill the same place in a deal: gold by kind, the rest exactly.
fn same_slot(a: DealItem, b: DealItem) -> bool {
    match (a, b) {
        (DealItem::Gold(_), DealItem::Gold(_)) => true,
        (DealItem::GoldPerTurn(_), DealItem::GoldPerTurn(_)) => true,
        (DealItem::Resource { resource: a, .. }, DealItem::Resource { resource: b, .. }) => a == b,
        _ => a == b,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CityStatesView {
    pub gold: u32,
    pub cards: Vec<CityStateCard>,
}

/// A met city-state as the player's city-state panel shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CityStateCard {
    pub player: PlayerId,
    pub name: String,
    /// "Maritime", "Cultured", ...
    pub kind: &'static str,
    pub influence: i32,
    pub resting: i32,
    pub friends_at: i32,
    pub allies_at: i32,
    /// "Allies", "Friends", "Neutral", "At war".
    pub status: &'static str,
    pub ally: Option<String>,
    pub pledged: bool,
    pub quests: Vec<String>,
    pub gifts: Vec<GoldGift>,
    pub pledge: Availability,
    pub bully_gold: u32,
    pub bully_gold_state: Availability,
    pub bully_worker: Availability,
    /// Gold price of annexing the city-state (Austria's buyout), when offered.
    pub buyout: Option<(u32, Availability)>,
    /// "Open on map": ready when one of its cities is seen or remembered, else why not.
    pub on_map: Availability,
}

/// The city-state card opened over the map (a click on a city-state's city): the same card as the
/// City-States screen, with the player's gold beside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CityStateWindow {
    pub gold: u32,
    pub card: CityStateCard,
}

/// A gold gift and the influence it buys now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoldGift {
    pub gold: u32,
    pub influence: i32,
    pub availability: Availability,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CongressView {
    /// "World Congress" or "United Nations"; None before the league is founded.
    pub title: Option<&'static str>,
    pub host: String,
    pub session: String,
    pub in_session: bool,
    /// Delegates of every member, the player's own first.
    pub delegates: Vec<(String, u16)>,
    pub delegates_left: u16,
    pub needed: u16,
    pub world_leader: String,
    pub proposals: Vec<ProposalCard>,
    pub active: Vec<String>,
    pub can_propose: bool,
    pub propose: Vec<ProposeRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposalCard {
    pub index: u8,
    pub title: String,
    pub proposer: String,
    pub choices: Vec<ChoiceButton>,
}

/// A yes/no, a candidate or a proposal choice with whether it can be taken now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoiceButton {
    pub choice: u32,
    pub label: String,
    pub availability: Availability,
}

/// A resolution the player may propose (or repeal), with its choices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposeRow {
    pub kind: ResolutionKind,
    pub title: String,
    pub text: &'static str,
    pub repeal: bool,
    pub choices: Vec<ChoiceButton>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EspionageView {
    pub spies: Vec<SpyCard>,
    pub targets: Vec<SpyTarget>,
    pub steal: Vec<StealCard>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpyCard {
    pub index: u8,
    pub name: String,
    pub rank: &'static str,
    pub state: &'static str,
    pub city: Option<String>,
    pub diplomat: bool,
    pub coup_chance: Option<i64>,
    pub coup: Availability,
    pub home: Availability,
    /// One entry per target, same order as `EspionageView::targets`.
    pub moves: Vec<SpyMove>,
}

/// A city a spy may be sent to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpyTarget {
    pub city: CityHandle,
    pub name: String,
    pub owner: String,
    /// "Your city", "City-state", "Capital", "City".
    pub role: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpyMove {
    pub spy: Availability,
    /// Foreign capitals only.
    pub diplomat: Option<Availability>,
}

/// Technologies a steal right allows from one nation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StealCard {
    pub from: PlayerId,
    pub nation: String,
    pub techs: Vec<(TechId, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VictoryView {
    /// Display turn and the last turn of the game.
    pub turn: u32,
    pub max_turns: u32,
    pub rows: Vec<VictoryRow>,
    pub scores: Vec<ScoreRow>,
}

/// Progress towards one kind of victory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VictoryRow {
    pub kind: &'static str,
    pub enabled: bool,
    pub goal: String,
    pub progress: String,
    /// Counts can exist before this victory path is active; do not imply readiness.
    pub show_progress: bool,
    pub have: u32,
    pub need: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScoreRow {
    pub name: String,
    pub score: i64,
    pub mine: bool,
}

/// The end of the game: who won, how, and the Hall of Fame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameEndView {
    pub won: bool,
    pub title: &'static str,
    pub line: String,
    pub score: i64,
    pub can_continue: bool,
    pub hall_of_fame: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use classic_rules::ids::ResourceId;

    #[test]
    fn deal_quantities_are_independent_and_remembered_after_removal() {
        let (me, them) = (PlayerId(0), PlayerId(1));
        let mut draft = DealDraft::new(them);
        let gold = DealItem::Gold(1);
        let iron = DealItem::Resource {
            resource: ResourceId(3),
            amount: 1,
        };
        draft.set_amount(me, gold, 37);
        draft.set_amount(them, gold, 9);
        draft.toggle(me, gold);
        draft.toggle(them, gold);
        draft.toggle(them, iron);
        draft.set_amount(them, iron, 3);
        assert_eq!(
            draft.items,
            vec![
                (me, DealItem::Gold(37)),
                (them, DealItem::Gold(9)),
                (
                    them,
                    DealItem::Resource {
                        resource: ResourceId(3),
                        amount: 3
                    }
                )
            ]
        );
        draft.toggle(them, iron);
        assert!(!draft.contains(them, iron));
        draft.toggle(them, iron);
        assert_eq!(draft.amount(them, iron), 3);
        assert_eq!(draft.amount(me, gold), 37);
    }

    #[test]
    fn deal_draft_toggles_items_and_builds_the_proposal() {
        let (me, them) = (PlayerId(0), PlayerId(1));
        let mut draft = DealDraft::new(them);
        assert_eq!(draft.request(), None, "an empty deal is not sent");
        draft.set_amount(me, DealItem::Gold(1), 120);
        draft.toggle(me, DealItem::Gold(0));
        draft.toggle(them, DealItem::OpenBorders);
        let iron = DealItem::Resource {
            resource: ResourceId(3),
            amount: 1,
        };
        draft.toggle(them, iron);
        draft.toggle(them, iron);
        assert!(draft.contains(me, DealItem::Gold(5)), "gold by kind");
        assert!(!draft.contains(them, DealItem::Gold(5)));
        assert_eq!(
            draft.request(),
            Some(GameRequest::ProposeDeal {
                to: them,
                items: vec![(me, DealItem::Gold(120)), (them, DealItem::OpenBorders)],
            })
        );
        draft.toggle(me, DealItem::Gold(0));
        assert_eq!(draft.items, vec![(them, DealItem::OpenBorders)]);
    }
}
