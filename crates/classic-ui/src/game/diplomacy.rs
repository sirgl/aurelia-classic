//! Diplomacy with the nations the player knows about: the core's contact memory
//! (met at least once) or a war. Unmet nations are never listed. The HUD shows a
//! short list with peace and open borders; the diplomacy screen shows treaties, war and
//! denunciation, the trade items with each nation and the offers waiting for an
//! answer.
use crate::ui::model::Availability;
use classic_rules::PlayerQueries;
use classic_rules::ClassicGame;
use classic_rules::ids::PlayerId;
use classic_rules::knowledge::observation::Relation;
use classic_rules::world::relations::{GrievanceKind, GRIEVANCE_POINT};
use classic_rules::rules::content::ResourceClass;
use classic_rules::world::player::PlayerKind;
use classic_rules::world::relations::{DealItem, DealOffer, OfferLapse, PeaceRefusal};
use classic_rules::Command;
use i18n::{t, tf};

use super::session::{availability_of, Session};
use crate::ui::model::{
    NationCard, PeaceAsk, DealOption, DiplomacyView, GameRequest, NoticeKind, OfferCard, Rival,
};
use classic_rules::world::player::ClassicPlayers;

/// Items both sides sign once (the giver listed is the proposer).
const MUTUAL: [DealItem; 4] = [
    DealItem::DefensivePact,
    DealItem::ResearchAgreement,
    DealItem::Friendship,
    DealItem::Peace,
];

impl Session {
    pub fn rivals(&mut self) -> Vec<Rival> {
        let count = self.game().view().auth.players().count() as u8;
        let known: Vec<PlayerId> = (0..count)
            .map(PlayerId)
            .filter(|p| *p != self.me() && self.has_diplomacy(*p) && self.is_known(*p))
            .collect();
        known.into_iter().map(|p| self.rival(p)).collect()
    }

    /// Whether the player takes part in diplomacy at all: the barbarians never do (always at
    /// war, no peace, no borders, no deals), so no diplomacy list shows them.
    fn has_diplomacy(&self, player: PlayerId) -> bool {
        !self.game().view().auth.players().is_barbarian(player)
    }

    fn is_known(&self, player: PlayerId) -> bool {
        let auth = self.game().view().auth;
        if !auth.players().is_alive(player) {
            return false;
        }
        let met = self
            .game()
            .player_view(self.rules(), self.me())
            .contact(player)
            .met_round
            .is_some();
        met || auth.diplomacy().at_war(self.me(), player)
    }

    /// Whether `host` may open its borders to `guest`; a refusal is worded as in the deal screen.
    fn open_borders_availability(&self, host: PlayerId, guest: PlayerId) -> Availability {
        let view = self.game().player_view(self.rules(), self.me());
        availability_of(view.deal_item_problem(host, guest, DealItem::OpenBorders))
    }

    fn rival(&mut self, player: PlayerId) -> Rival {
        let me = self.me();
        let diplomacy = self.game().view().auth.diplomacy();
        let (at_war, borders_open) = (
            diplomacy.at_war(me, player),
            diplomacy.grants_entry(me, player),
        );
        Rival {
            player: player.0,
            name: self.player_name(player),
            at_war,
            borders_open,
            peace: self.peace_availability(player),
            peace_by_offer: self.peace_by_offer(player),
            city_state: !self.peace_by_offer(player),
            peace_ask: self.peace_ask(player),
            open_borders: self.open_borders_availability(me, player),
        }
    }

    /// Peace with `other` is a deal it answers (a nation), not the player's act (a city-state).
    fn peace_by_offer(&self, other: PlayerId) -> bool {
        classic_rules::world::relations::peace_needs_a_deal(self.game().view().auth, self.me(), other)
    }

    /// What the peace button sends: an offer of white peace to a nation (it makes peace when the
    /// nation accepts), peace itself with a city-state.
    fn peace_command(&self, other: PlayerId) -> Command {
        let me = self.me();
        if self.peace_by_offer(other) {
            Command::ProposeDeal { from: me, to: other, items: vec![(me, DealItem::Peace)] }
        } else {
            Command::MakePeace { player: me, enemy: other }
        }
    }

    /// An offer of peace of the player's waits for `other`'s answer.
    fn peace_offered(&self, other: PlayerId) -> bool {
        let me = self.me();
        let offers = self.game().view().auth.module().relations().offers();
        offers.iter().any(|o| o.from == me && o.to == other && o.items.iter().any(|(_, i)| *i == DealItem::Peace))
    }

    /// What `other` said when it refused the player's peace, if it is still at war with the
    /// player and asked gold for it.
    pub(super) fn peace_ask(&mut self, other: PlayerId) -> Option<PeaceAsk> {
        let (price, reason) = match self.peace_asks.iter().find(|(p, _)| *p == other)?.1 {
            PeaceRefusal::Aggressor { price } => (price, tf!("classic.game.diplomacy.ask-aggressor", price = price)),
            PeaceRefusal::Winning { price } => (price, tf!("classic.game.diplomacy.ask-winning", price = price)),
            PeaceRefusal::Unsaid | PeaceRefusal::Terms => return None,
        };
        let me = self.me();
        if !self.game().view().auth.diplomacy().at_war(me, other) {
            return None;
        }
        let offer = if self.peace_offered(other) {
            Availability::Blocked(t!("classic.game.diplomacy.peace-waiting").into())
        } else {
            self.availability(&Self::peace_for_gold(me, other, price))
        };
        Some(PeaceAsk { price, reason, offer })
    }

    /// The white peace with `gold` the player gives for it.
    fn peace_for_gold(me: PlayerId, other: PlayerId, gold: u32) -> Command {
        Command::ProposeDeal { from: me, to: other, items: vec![(me, DealItem::Peace), (me, DealItem::Gold(gold))] }
    }

    /// The button "peace for N gold": the offer of the peace with the gold the nation asked.
    pub(super) fn peace_for_gold_request(&mut self, other: PlayerId) {
        let Some(ask) = self.peace_ask(other) else { return };
        if let Availability::Blocked(reason) = ask.offer {
            self.message = Some(reason);
            return;
        }
        let command = Self::peace_for_gold(self.me(), other, ask.price);
        let text = tf!("classic.game.diplomacy.notice-peace-offered-gold", name = self.player_name(other), price = ask.price);
        if self.submit(command) {
            self.notify(NoticeKind::Diplomacy, text);
        }
    }

    fn peace_availability(&mut self, other: PlayerId) -> Availability {
        if self.peace_offered(other) {
            return Availability::Blocked(t!("classic.game.diplomacy.peace-waiting").into());
        }
        let command = self.peace_command(other);
        self.availability(&command)
    }

    /// The peace button: the offer to a nation (the notice says it waits for the answer) or the
    /// peace with a city-state.
    pub(super) fn peace_request(&mut self, other: PlayerId) {
        if let Availability::Blocked(reason) = self.peace_availability(other) {
            self.message = Some(reason);
            return;
        }
        let name = self.player_name(other);
        let command = self.peace_command(other);
        let text = match command {
            Command::ProposeDeal { .. } => tf!("classic.game.diplomacy.notice-peace-offered", name = name),
            _ => tf!("classic.game.diplomacy.notice-peace", name = name),
        };
        if self.submit(command) {
            self.notify(NoticeKind::Diplomacy, text);
        }
    }

    /// Notices of the deals in `records` that concern the player: the answer to its offer
    /// (`ClassicChange::OfferAnswered`), an offer of peace addressed to it, which waits on
    /// the diplomacy screen, and a deal broken by an unpaid gold per turn, either way
    /// (`ClassicChange::DealBroken`).
    pub(super) fn notify_offers(&mut self, records: &[classic_rules::transaction::journal::AppliedCommand]) {
        let me = self.me();
        if self.game().view().auth.settings().is_ai(me) {
            return;
        }
        for record in records {
            if let Command::ProposeDeal { from, to, items } = &record.command {
                if *to == me && items.iter().any(|(_, i)| *i == DealItem::Peace) {
                    let text = tf!("classic.game.diplomacy.notice-peace-proposed", name = self.player_name(*from));
                    self.notify(NoticeKind::Diplomacy, text);
                }
            }
            for change in &record.changes.module_changes {
                if let classic_rules::classic::ClassicChange::DealBroken { debtor, creditor, gold } = *change {
                    // An unpaid gold per turn broke a deal (classic/docs/classic/rules.md, Deals).
                    let text = if creditor == me {
                        tf!("classic.game.diplomacy.notice-deal-broken-theirs", name = self.player_name(debtor), gold = gold)
                    } else if debtor == me {
                        tf!("classic.game.diplomacy.notice-deal-broken-ours", name = self.player_name(creditor), gold = gold)
                    } else {
                        continue;
                    };
                    self.notify(NoticeKind::Diplomacy, text);
                    continue;
                }
                if let classic_rules::classic::ClassicChange::OfferLapsed { from, to, reason, .. } = *change {
                    if to == me {
                        let text = offer_lapsed_notice(&self.player_name(from), reason);
                        self.notify(NoticeKind::Diplomacy, text);
                    }
                    continue;
                }
                let classic_rules::classic::ClassicChange::OfferAnswered { from, to, accepted, peace } = *change else { continue };
                if from != me {
                    continue;
                }
                let name = self.player_name(to);
                let why = match &record.command {
                    Command::RespondDeal { why, .. } => *why,
                    _ => PeaceRefusal::Unsaid,
                };
                self.peace_asks.retain(|(p, _)| *p != to);
                if peace && !accepted && why != PeaceRefusal::Unsaid {
                    self.peace_asks.push((to, why));
                }
                let text = match (peace, accepted) {
                    (true, true) => tf!("classic.game.diplomacy.notice-peace-accepted", name = name),
                    (true, false) => match why {
                        PeaceRefusal::Unsaid => tf!("classic.game.diplomacy.notice-peace-refused", name = name),
                        PeaceRefusal::Aggressor { price } => {
                            let reason = tf!("classic.game.diplomacy.ask-aggressor", price = price);
                            tf!("classic.game.diplomacy.notice-peace-refused-why", name = name, reason = reason)
                        }
                        PeaceRefusal::Winning { price } => {
                            let reason = tf!("classic.game.diplomacy.ask-winning", price = price);
                            tf!("classic.game.diplomacy.notice-peace-refused-why", name = name, reason = reason)
                        }
                        PeaceRefusal::Terms => {
                            let reason = t!("classic.game.diplomacy.ask-terms");
                            tf!("classic.game.diplomacy.notice-peace-refused-why", name = name, reason = reason)
                        }
                    },
                    (false, true) => tf!("classic.game.diplomacy.notice-offer-accepted", name = name),
                    (false, false) => tf!("classic.game.diplomacy.notice-offer-refused", name = name),
                };
                self.notify(NoticeKind::Diplomacy, text);
            }
        }
    }

    /// Nations the player met (not city-states), as its observation knows them.
    pub(super) fn met_nations(&self) -> Vec<Relation> {
        let observation = self.game().ai_observation(self.rules(), self.me());
        observation
            .relations
            .iter()
            .filter(|r| r.kind == PlayerKind::Major)
            .copied()
            .collect()
    }

    pub fn diplomacy_view(&mut self) -> DiplomacyView {
        let observation = self.game().ai_observation(self.rules(), self.me());
        let offers: Vec<DealOffer> = observation.offers_to_us.clone();
        let gold = observation.economy.treasury;
        let nations = self
            .met_nations()
            .iter()
            .map(|r| self.nation_card(r))
            .collect();
        let offers = offers.iter().map(|o| self.offer_card(o)).collect();
        let outgoing = self
            .game()
            .view()
            .auth
            .module()
            .relations()
            .offers()
            .iter()
            .filter(|offer| offer.from == self.me())
            .map(|offer| crate::ui::model::DealDraft {
                partner: offer.to,
                items: offer.items.clone(),
                amounts: Vec::new(),
            })
            .collect();
        DiplomacyView {
            confirm: self.confirm_card(),
            outgoing,
            preview: None,
            me: self.me(),
            gold,
            nations,
            offers,
        }
    }

    /// Validates the complete draft without issuing a command or changing the game.
    pub fn preview_deal(
        &mut self,
        draft: &crate::ui::model::DealDraft,
    ) -> crate::ui::model::Availability {
        self.availability(&Command::ProposeDeal {
            from: self.me(),
            to: draft.partner,
            items: draft.items.clone(),
        })
    }

    fn nation_card(&mut self, relation: &Relation) -> NationCard {
        let (me, other) = (self.me(), relation.other);
        let auth = self.game().view().auth;
        let nation = self.rules().nations.get(auth.players().nation_of(other) as usize);
        let leader = nation.map(|c| c.leader.clone()).unwrap_or_default();
        let art = nation.map(|c| art_key(&c.key)).unwrap_or_default();
        NationCard {
            player: other,
            name: self.player_name(other),
            leader,
            art,
            at_war: relation.at_war,
            state: war_state(relation, auth.turn().round),
            treaties: treaties(relation),
            score: classic_rules::world::victory::score(auth, self.rules(), other),
            gold: auth.players().treasury_of(other),
            declare_war: self.availability(&Command::DeclareWar {
                player: me,
                enemy: other,
            }),
            make_peace: self.peace_availability(other),
            peace_ask: self.peace_ask(other),
            denounce: self.availability(&Command::Denounce {
                player: me,
                target: other,
            }),
            open_borders: self.open_borders_availability(me, other),
            ours: self.deal_options(me, other),
            theirs: self.deal_options(other, me),
            mutual: self.mutual_options(other),
        }
    }

    /// What `giver` may hand to `receiver`: gold, gold per turn, an embassy, open
    /// borders (blocked ones with the reason), and the tradable resources it has.
    fn deal_options(&self, giver: PlayerId, receiver: PlayerId) -> Vec<DealOption> {
        let mut items = vec![
            DealItem::Gold(1),
            DealItem::GoldPerTurn(1),
            DealItem::Embassy,
            DealItem::OpenBorders,
        ];
        let fixed = items.len();
        items.extend(self.resource_items());
        let view = self.game().player_view(self.rules(), self.me());
        let auth = self.game().view().auth;
        // The gold a turn the giver's income still carries with its running deals: the rules'
        // own limit (`DealBlock::NotEnoughIncome`).
        let income = classic_rules::world::relations::gold_income(auth, self.rules(), giver).max(0) as u32;
        let resources = classic_rules::derived::resources::resource_ledger(auth, self.rules(), giver);
        let mut out = Vec::new();
        for (index, item) in items.into_iter().enumerate() {
            let problem = view.deal_item_problem(giver, receiver, item);
            let max_amount = match item {
                DealItem::Gold(_) => Some(auth.players().treasury_of(giver)),
                DealItem::GoldPerTurn(_) => Some(income),
                DealItem::Resource { resource, .. } => {
                    Some(resources.tradeable(resource).max(0).min(u16::MAX as i32) as u32)
                }
                _ => None,
            };
            if index >= fixed && max_amount == Some(0) {
                continue;
            }
            out.push(DealOption {
                max_amount,
                item,
                label: self.item_label(item),
                availability: availability_of(problem),
            });
        }
        out
    }

    /// One copy of every luxury and strategic resource of the rules.
    fn resource_items(&self) -> Vec<DealItem> {
        let mut out = Vec::new();
        for (index, def) in self.rules().resources.iter().enumerate() {
            let tradable = matches!(
                def.class,
                Some(ResourceClass::Luxury | ResourceClass::Strategic)
            );
            if tradable {
                out.push(DealItem::Resource {
                    resource: classic_rules::ids::ResourceId(index as u16),
                    amount: 1,
                });
            }
        }
        out
    }

    fn mutual_options(&self, other: PlayerId) -> Vec<DealOption> {
        let me = self.me();
        let view = self.game().player_view(self.rules(), me);
        MUTUAL
            .iter()
            .map(|item| DealOption {
                max_amount: None,
                item: *item,
                label: self.item_label(*item),
                availability: availability_of(view.deal_item_problem(me, other, *item)),
            })
            .collect()
    }

    fn offer_card(&self, offer: &DealOffer) -> OfferCard {
        let lines = offer
            .items
            .iter()
            .map(|(giver, item)| {
                let name = match item {
                    DealItem::Resource { resource, .. } => self.resource_name(*resource),
                    _ => self.item_label(*item),
                };
                let what = offer_quantity_label(*item, &name);
                if item.is_mutual() {
                    tf!("classic.game.diplomacy.offer-both", what = what)
                } else if *giver == self.me() {
                    tf!("classic.game.diplomacy.offer-give", what = what)
                } else {
                    tf!("classic.game.diplomacy.offer-receive", what = what)
                }
            })
            .collect();
        OfferCard {
            id: offer.id,
            from: self.player_name(offer.from),
            expires: offer.expires + 1,
            lines,
        }
    }

    pub(super) fn item_label(&self, item: DealItem) -> String {
        match item {
            DealItem::Gold(1) => t!("classic.game.diplomacy.item-gold").into(),
            DealItem::Gold(n) => tf!("classic.game.diplomacy.item-gold-amount", n = n),
            DealItem::GoldPerTurn(1) => t!("classic.game.diplomacy.item-gold-per-turn").into(),
            DealItem::GoldPerTurn(n) => tf!("classic.game.diplomacy.item-gold-per-turn-amount", n = n),
            DealItem::Resource { resource, amount } => {
                let name = self.resource_name(resource);
                if amount == 1 {
                    name
                } else {
                    tf!("classic.game.diplomacy.item-resource-amount", n = amount, name = name)
                }
            }
            DealItem::City(city) => self.city_name(city),
            DealItem::Embassy => t!("classic.game.diplomacy.item-embassy").into(),
            DealItem::OpenBorders => t!("classic.game.diplomacy.item-open-borders").into(),
            DealItem::DefensivePact => t!("classic.game.diplomacy.item-defensive-pact").into(),
            DealItem::ResearchAgreement => t!("classic.game.diplomacy.item-research-agreement").into(),
            DealItem::Friendship => t!("classic.game.diplomacy.item-friendship").into(),
            DealItem::Peace => t!("classic.game.diplomacy.item-peace").into(),
            DealItem::ThirdPartyWar(p) => tf!("classic.game.diplomacy.item-war-on", name = self.player_name(p)),
            DealItem::ThirdPartyPeace(p) => tf!("classic.game.diplomacy.item-peace-with", name = self.player_name(p)),
        }
    }

    pub(super) fn resource_name(&self, resource: classic_rules::ids::ResourceId) -> String {
        match self.rules().resources.get(resource.0 as usize) {
            Some(def) => super::names::named("resource", &def.name, &def.key),
            None => t!("classic.game.diplomacy.resource").into(),
        }
    }

    pub(super) fn city_name(&self, city: classic_rules::ids::CityHandle) -> String {
        let auth = self.game().view().auth;
        auth.cities()
            .get(city)
            .map(|c| c.name.clone())
            .unwrap_or_default()
    }

    pub(super) fn diplomacy(&mut self, command: Command) {
        let text = match &command {
            Command::OpenBorders { guest, .. } => {
                tf!("classic.game.diplomacy.notice-borders", name = self.player_name(*guest))
            }
            _ => return,
        };
        if self.submit(command) {
            self.notify(NoticeKind::Diplomacy, text);
        }
    }

    /// War and denunciation (once confirmed: `super::confirm`), deals and answers to offers
    /// from the diplomacy screen.
    pub(super) fn deal_request(&mut self, request: GameRequest) {
        let me = self.me();
        let (command, text) = match request {
            // War and denunciation wait for the player's yes (`super::confirm`).
            GameRequest::DeclareWar(enemy) => return self.ask_first(super::confirm::Confirmation::War(enemy)),
            GameRequest::Denounce(target) => return self.ask_first(super::confirm::Confirmation::Denounce(target)),
            GameRequest::ProposeDeal { to, items } => (
                Command::ProposeDeal {
                    from: me,
                    to,
                    items,
                },
                tf!("classic.game.diplomacy.notice-offered", name = self.player_name(to)),
            ),
            GameRequest::RespondDeal { offer, accept } => (
                Command::RespondDeal {
                    player: me,
                    offer,
                    accept,
                    why: classic_rules::world::relations::PeaceRefusal::Unsaid,
                },
                if accept {
                    t!("classic.game.diplomacy.notice-accepted")
                } else {
                    t!("classic.game.diplomacy.notice-refused")
                }
                .to_string(),
            ),
            _ => return,
        };
        if self.submit(command) {
            self.notify(NoticeKind::Diplomacy, text);
        }
    }
}

impl Session {
    /// War on `enemy` or the denunciation of `target`, which the player confirmed.
    pub(super) fn declare_war(&mut self, enemy: PlayerId) {
        let text = tf!("classic.game.diplomacy.notice-war", name = self.player_name(enemy));
        if self.submit(Command::DeclareWar { player: self.me(), enemy }) {
            self.notify(NoticeKind::Diplomacy, text);
        }
    }

    pub(super) fn denounce(&mut self, target: PlayerId) {
        let text = tf!("classic.game.diplomacy.notice-denounce", name = self.player_name(target));
        if self.submit(Command::Denounce { player: self.me(), target }) {
            self.notify(NoticeKind::Diplomacy, text);
        }
    }
}

/// Artwork key of a nation ("core:nation/rome" -> "art:rome").
fn art_key(nation_key: &str) -> String {
    let slug = nation_key.rsplit('/').next().unwrap_or(nation_key);
    format!("art:{slug}")
}

fn war_state(relation: &Relation, round: u32) -> String {
    if relation.at_war {
        return tf!("classic.game.diplomacy.state-war", turn = relation.war_since + 1);
    }
    if relation.peace_treaty_until > round {
        return tf!("classic.game.diplomacy.state-treaty", turn = relation.peace_treaty_until + 1);
    }
    t!("classic.game.diplomacy.state-peace").into()
}

fn treaties(relation: &Relation) -> Vec<String> {
    let facts = [
        (relation.our_embassy, t!("classic.game.diplomacy.fact-our-embassy")),
        (relation.their_embassy, t!("classic.game.diplomacy.fact-their-embassy")),
        (relation.we_grant_entry, t!("classic.game.diplomacy.fact-we-grant-entry")),
        (relation.they_grant_entry, t!("classic.game.diplomacy.fact-they-grant-entry")),
        (relation.defensive_pact, t!("classic.game.diplomacy.item-defensive-pact")),
        (relation.research_agreement, t!("classic.game.diplomacy.item-research-agreement")),
        (relation.friends, t!("classic.game.diplomacy.item-friendship")),
        (relation.we_denounced, t!("classic.game.diplomacy.fact-we-denounced")),
        (relation.they_denounced, t!("classic.game.diplomacy.fact-they-denounced")),
    ];
    let mut out: Vec<String> = facts
        .iter()
        .filter(|(holds, _)| *holds)
        .map(|(_, text)| text.to_string())
        .collect();
    if relation.warmonger > 0 {
        out.push(tf!("classic.game.diplomacy.fact-warmonger", n = relation.warmonger));
    }
    for kind in GrievanceKind::all() {
        let amount = relation.grievances[kind.index()];
        if amount > 0 {
            out.push(grievance_fact(kind, amount));
        }
    }
    out
}

/// What the player holds against a nation, for one reason: `amount` in opinion points, rounded up so
/// that a grievance that has not faded yet never reads as 0.
fn grievance_fact(kind: GrievanceKind, amount: i32) -> String {
    let points = (amount + GRIEVANCE_POINT - 1) / GRIEVANCE_POINT;
    match kind {
        GrievanceKind::StolenLand => tf!("classic.game.diplomacy.fact-grievance-stolen-land", n = points),
        GrievanceKind::BetrayedFriendship => tf!("classic.game.diplomacy.fact-grievance-betrayed-friendship", n = points),
        GrievanceKind::BetrayedPact => tf!("classic.game.diplomacy.fact-grievance-betrayed-pact", n = points),
    }
}


/// Offer text always states the actual quantity, including one. Option labels stay short.
fn offer_quantity_label(item: DealItem, name: &str) -> String {
    match item {
        DealItem::Gold(amount) => tf!("classic.game.diplomacy.quantity-gold-now", n = amount),
        DealItem::GoldPerTurn(amount) => tf!("classic.game.diplomacy.quantity-gold-per-turn", n = amount),
        DealItem::Resource { amount, .. } => tf!("classic.game.diplomacy.quantity-resource", n = amount, name = name),
        _ => name.to_owned(),
    }
}

#[cfg(test)]
mod offer_label_tests {
    use super::*;
    #[test]
    fn incoming_offer_labels_always_state_exact_quantities() {
        assert_eq!(offer_quantity_label(DealItem::Gold(1), "Gold"), "1 gold now");
        assert_eq!(offer_quantity_label(DealItem::GoldPerTurn(1), "Gold per turn"), "1 gold per turn");
        assert_eq!(offer_quantity_label(DealItem::Gold(37), "Gold"), "37 gold now");
        for amount in [1, 3] {
            assert_eq!(offer_quantity_label(DealItem::Resource { resource: classic_rules::ids::ResourceId(3), amount }, "Iron"), format!("{amount} × Iron"));
        }
        assert_eq!(offer_quantity_label(DealItem::OpenBorders, "Open borders"), "Open borders");
    }
}

#[cfg(test)]
mod peace_tests {
    use classic_rules::classic::ClassicChange;
    use classic_rules::transaction::journal::{AppliedCommand, ChangeJournal};

    use super::*;
    use crate::ui::screens::{game_rules, prepare_match, MapSize, MatchRequest};

    fn session_at_war() -> Session {
        let request = MatchRequest { nation: "core:nation/rome".into(), seed: 7, size: MapSize::Small, players: 3, water: 25 };
        let mut session = Session::new(prepare_match(request).expect("world").game, game_rules());
        let (game, rules) = session.harness_game();
        game.debug_meet(PlayerId(0), PlayerId(2));
        game.execute(&rules, Command::DeclareWar { player: PlayerId(0), enemy: PlayerId(2) }).unwrap();
        session.refresh_after_harness();
        session
    }

    fn texts(session: &mut Session) -> Vec<String> {
        session.hud_model(None).notices.into_iter().map(|n| n.text).collect()
    }

    /// The answer to the player's offer and an offer of peace to it become notices, each in
    /// its own words, in every language of the game (no key shows instead of a text); the
    /// answers between other players are not told.
    #[test]
    fn answers_and_offers_of_peace_are_told_in_every_language() {
        let mut session = session_at_war();
        let (me, rival) = (session.me(), PlayerId(2));
        let answered = |from: PlayerId, to: PlayerId, accepted: bool, peace: bool| {
            let mut journal = ChangeJournal::default();
            journal.module_changes.push(ClassicChange::OfferAnswered { from, to, accepted, peace });
            AppliedCommand { command: Command::RespondDeal { player: to, offer: 0, accept: accepted, why: classic_rules::world::relations::PeaceRefusal::Unsaid }, changes: journal }
        };
        let proposed = AppliedCommand { command: Command::ProposeDeal { from: rival, to: me, items: vec![(rival, DealItem::Peace)] }, changes: ChangeJournal::default() };
        let records = vec![
            answered(me, rival, true, true),
            answered(me, rival, false, true),
            answered(me, rival, true, false),
            answered(me, rival, false, false),
            answered(rival, me, true, true),
            answered(PlayerId(1), rival, false, true),
            proposed,
        ];
        for lang in ["en", "ru", "de", "fr", "es", "ja", "zh-Hans"] {
            let told = i18n::with_language(lang, || {
                session.notices.clear();
                session.notify_offers(&records);
                texts(&mut session)
            });
            assert_eq!(told.len(), 5, "{lang}: four answers to our offers and one offer: {told:?}");
            let mut distinct = told.clone();
            distinct.sort();
            distinct.dedup();
            assert_eq!(distinct.len(), 5, "{lang}: {told:?}");
            assert!(told.iter().all(|t| !t.contains("classic.")), "{lang}: {told:?}");
        }
        session.notices.clear();
        session.notify_offers(&records);
        let name = session.player_name(rival);
        let told = texts(&mut session);
        assert!(told.contains(&format!("{name} refused peace: the war goes on")), "{told:?}");
        assert!(told.contains(&format!("{name} offers peace: answer on the Diplomacy screen")), "{told:?}");
    }

    /// A refusal of peace tells why and the price in every language of the game (no key shows
    /// instead of a text, the price is in the text), keeps the price for the button "peace for
    /// N gold", and an acceptance, a plain refusal and a refusal of the terms keep none.
    #[test]
    fn a_refusal_of_peace_tells_why_and_names_the_price_in_every_language() {
        let mut session = session_at_war();
        let (me, rival) = (session.me(), PlayerId(2));
        let refused = |why: PeaceRefusal| {
            let mut journal = ChangeJournal::default();
            journal.module_changes.push(ClassicChange::OfferAnswered { from: me, to: rival, accepted: false, peace: true });
            AppliedCommand { command: Command::RespondDeal { player: rival, offer: 0, accept: false, why }, changes: journal }
        };
        let cases = [
            (PeaceRefusal::Aggressor { price: 37 }, Some("37")),
            (PeaceRefusal::Winning { price: 41 }, Some("41")),
            (PeaceRefusal::Terms, None),
            (PeaceRefusal::Unsaid, None),
        ];
        for lang in ["en", "ru", "de", "fr", "es", "ja", "zh-Hans"] {
            let mut told = Vec::new();
            i18n::with_language(lang, || {
                for (why, price) in cases {
                    session.notices.clear();
                    session.notify_offers(&[refused(why)]);
                    let text = texts(&mut session).pop().expect("a notice");
                    assert!(!text.contains("classic."), "{lang}: {text}");
                    if let Some(price) = price {
                        assert!(text.contains(price), "{lang}: the price in {text}");
                        let ask = session.peace_ask(rival).expect("the price is kept");
                        assert!(ask.reason.contains(price) && !ask.reason.contains("classic."), "{lang}: {}", ask.reason);
                        let label = tf!("classic.ui.screen-diplomacy.offer-peace-gold", price = ask.price);
                        let small = tf!("classic.ui.hud-game.offer-peace-gold", price = ask.price);
                        assert!(label.contains(price) && small.contains(price) && !label.contains("classic.") && !small.contains("classic."), "{lang}: {label} / {small}");
                    } else {
                        assert!(session.peace_ask(rival).is_none(), "{lang}: no price to keep");
                    }
                    told.push(text);
                }
            });
            told.sort();
            told.dedup();
            assert_eq!(told.len(), 4, "{lang}: four different words: {told:?}");
        }
        session.notices.clear();
        session.notify_offers(&[refused(PeaceRefusal::Winning { price: 41 })]);
        let name = session.player_name(rival);
        assert_eq!(texts(&mut session), vec![format!("{name} refuses peace. They are winning this war: peace costs 41 gold.")]);
    }

    /// `MakePeace` with a nation is refused with the consent reason, worded in every language;
    /// the button sends the offer instead.
    #[test]
    fn peace_alone_with_a_nation_is_refused_with_its_reason() {
        let mut session = session_at_war();
        let command = Command::MakePeace { player: session.me(), enemy: PlayerId(2) };
        for lang in ["en", "ru", "de", "fr", "es", "ja", "zh-Hans"] {
            let blocked = i18n::with_language(lang, || session.availability(&command));
            let Availability::Blocked(reason) = blocked else { panic!("{lang}: allowed") };
            assert!(!reason.contains("classic.") && !reason.contains("needs its consent"), "{lang}: {reason}");
        }
        let Availability::Blocked(reason) = session.availability(&command) else { unreachable!() };
        assert_eq!(reason, "Peace with a nation is a deal: offer it, and it makes peace if it accepts");
        assert!(matches!(session.peace_command(PlayerId(2)), Command::ProposeDeal { .. }));
    }
}

/// The notice of an offer that left the inbox unanswered.
fn offer_lapsed_notice(name: &str, reason: OfferLapse) -> String {
    let why = match reason {
        OfferLapse::Expired => t!("classic.game.diplomacy.offer-lapse-expired"),
        OfferLapse::NationGone => t!("classic.game.diplomacy.offer-lapse-gone"),
        OfferLapse::WarBegan => t!("classic.game.diplomacy.offer-lapse-war"),
        OfferLapse::NoLongerPossible => t!("classic.game.diplomacy.offer-lapse-impossible"),
    };
    tf!("classic.game.diplomacy.notice-offer-lapsed", name = name, reason = why)
}
