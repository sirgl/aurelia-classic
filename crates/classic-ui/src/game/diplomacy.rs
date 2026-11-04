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
use classic_rules::rules::content::ResourceClass;
use classic_rules::world::player::PlayerKind;
use classic_rules::world::relations::{DealItem, DealOffer};
use classic_rules::Command;
use i18n::{t, tf};

use super::session::{availability_of, Session};
use crate::ui::model::{
    NationCard, DealOption, DiplomacyView, GameRequest, NoticeKind, OfferCard, Rival,
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
        let peace = Command::MakePeace {
            player: me,
            enemy: player,
        };
        Rival {
            player: player.0,
            name: self.player_name(player),
            at_war,
            borders_open,
            peace: self.availability(&peace),
            open_borders: self.open_borders_availability(me, player),
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
            make_peace: self.availability(&Command::MakePeace {
                player: me,
                enemy: other,
            }),
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
        let income = classic_rules::derived::income::player_income(auth, self.rules(), giver)
            .gold
            .max(0) as u32;
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
            Command::MakePeace { enemy, .. } => {
                tf!("classic.game.diplomacy.notice-peace", name = self.player_name(*enemy))
            }
            Command::OpenBorders { guest, .. } => {
                tf!("classic.game.diplomacy.notice-borders", name = self.player_name(*guest))
            }
            _ => return,
        };
        if self.submit(command) {
            self.notify(NoticeKind::Diplomacy, text);
        }
    }

    /// War, denunciation, deals and answers to offers from the diplomacy screen.
    pub(super) fn deal_request(&mut self, request: GameRequest) {
        let me = self.me();
        let (command, text) = match request {
            GameRequest::DeclareWar(enemy) => (
                Command::DeclareWar { player: me, enemy },
                tf!("classic.game.diplomacy.notice-war", name = self.player_name(enemy)),
            ),
            GameRequest::Denounce(target) => (
                Command::Denounce { player: me, target },
                tf!("classic.game.diplomacy.notice-denounce", name = self.player_name(target)),
            ),
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
    out
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
