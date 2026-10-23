//! Deals (classic/docs/classic/rules.md): which items may be
//! traded now (`item_problem`, pure) and what an accepted deal does.

use crate::command::CommandError;
use crate::transaction::journal::PlayerChange;
use crate::ids::{PlayerId, ResourceId};
use crate::rules::content::ResourceClass;
use crate::rules::CompiledRuleset;
use crate::systems::diplomacy::{has_met, DEAL_DURATION, RELATIONSHIP_DURATION};
use crate::transaction::transaction::WorldTransaction;
use crate::world::relations::{DealItem, DealOffer, OngoingTrade, RelationsState, TradeFlow};
use crate::world::state::AuthWorld;

/// At most this many items in one deal. Own value (no Unciv equivalent).
pub const MAX_DEAL_ITEMS: usize = 12;
/// An offer waits this many rounds for its answer. Own value (no Unciv
/// equivalent).
pub const OFFER_ROUNDS: u32 = 1;
/// An embassy reveals the tiles this close to the host's capital (our rule:
/// public descriptions name the capital and the tiles around it, no
/// number; classic/docs/classic/rules.md).
pub const EMBASSY_REVEAL_RADIUS: u8 = 2;

/// Does some technology of the rules carry `flag`, and does `p` or `q`
/// know one? Rules without the flag (the frozen legacy content) need no technology.
fn tech_allows(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    players: &[PlayerId],
    flag: fn(&crate::rules::compiled::TechnologyDefinition) -> bool,
) -> bool {
    if !rules.technologies.iter().any(flag) {
        return true;
    }
    players.iter().any(|p| {
        auth.players()
            .techs_of(*p)
            .iter()
            .any(|t| rules.technologies.get(t.0 as usize).is_some_and(flag))
    })
}

/// Research agreement cost: the higher current era's cost (Standard).
pub fn research_agreement_cost(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    a: PlayerId,
    b: PlayerId,
) -> u32 {
    let era = |p| crate::combat::city::current_era(auth, rules, p);
    let costs = [era(a), era(b)].map(|e| e.map(|e| e.research_agreement_cost).unwrap_or(0));
    costs[0].max(costs[1])
}

/// Copies of `resource` `player` can give: what its resource ledger has
/// free to trade (`derived::resources`: connected copies and imports minus units,
/// buildings, exports and the untradeable Feitoria copies).
pub fn resource_available(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    player: PlayerId,
    resource: ResourceId,
) -> i32 {
    crate::derived::resources::resource_ledger(auth, rules, player).tradeable(resource)
}

/// Why an item cannot pass in a deal. `reason` is the rule text of logs and tests; a screen
/// words each reason once from the type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DealBlock {
    /// The deal is between two other players.
    NotOurDeal,
    GoldNeedsFriendship,
    NotEnoughGold,
    AtWar,
    NotEnoughIncome,
    NotTradeable,
    NotEnoughResource,
    NoSuchCity,
    NotACityToGive,
    EmbassyExists,
    NoCityForEmbassy,
    EmbassyNeedsTechnology,
    BordersOpen,
    BordersNeedTechnology,
    BordersNeedEmbassy,
    NoPact,
    PactNeedsTechnology,
    PactNeedsEmbassies,
    NoAgreement,
    AgreementNeedsTechnology,
    AgreementNeedsEmbassies,
    AgreementNeedsFriendship,
    NoFriendship,
    NotAtWar,
    CannotDeclareWar,
    CannotMakePeace,
}

impl DealBlock {
    pub const ALL: [DealBlock; 26] = [
        DealBlock::NotOurDeal,
        DealBlock::GoldNeedsFriendship,
        DealBlock::NotEnoughGold,
        DealBlock::AtWar,
        DealBlock::NotEnoughIncome,
        DealBlock::NotTradeable,
        DealBlock::NotEnoughResource,
        DealBlock::NoSuchCity,
        DealBlock::NotACityToGive,
        DealBlock::EmbassyExists,
        DealBlock::NoCityForEmbassy,
        DealBlock::EmbassyNeedsTechnology,
        DealBlock::BordersOpen,
        DealBlock::BordersNeedTechnology,
        DealBlock::BordersNeedEmbassy,
        DealBlock::NoPact,
        DealBlock::PactNeedsTechnology,
        DealBlock::PactNeedsEmbassies,
        DealBlock::NoAgreement,
        DealBlock::AgreementNeedsTechnology,
        DealBlock::AgreementNeedsEmbassies,
        DealBlock::AgreementNeedsFriendship,
        DealBlock::NoFriendship,
        DealBlock::NotAtWar,
        DealBlock::CannotDeclareWar,
        DealBlock::CannotMakePeace,
    ];

    pub fn reason(self) -> &'static str {
        match self {
            DealBlock::NotOurDeal => "not our deal",
            DealBlock::GoldNeedsFriendship => "gold needs friendship",
            DealBlock::NotEnoughGold => "not enough gold",
            DealBlock::AtWar => "at war",
            DealBlock::NotEnoughIncome => "not enough income",
            DealBlock::NotTradeable => "not a tradeable resource",
            DealBlock::NotEnoughResource => "not enough of the resource",
            DealBlock::NoSuchCity => "no such city",
            DealBlock::NotACityToGive => "not a city to give",
            DealBlock::EmbassyExists => "embassy exists",
            DealBlock::NoCityForEmbassy => "no city for an embassy",
            DealBlock::EmbassyNeedsTechnology => "embassies need Writing",
            DealBlock::BordersOpen => "borders already open",
            DealBlock::BordersNeedTechnology => "open borders need Civil Service",
            DealBlock::BordersNeedEmbassy => "open borders need an embassy",
            DealBlock::NoPact => "no pact possible",
            DealBlock::PactNeedsTechnology => "pacts need Chivalry",
            DealBlock::PactNeedsEmbassies => "pacts need embassies",
            DealBlock::NoAgreement => "no agreement possible",
            DealBlock::AgreementNeedsTechnology => "agreements need Education",
            DealBlock::AgreementNeedsEmbassies => "agreements need embassies",
            DealBlock::AgreementNeedsFriendship => "agreements need friendship",
            DealBlock::NoFriendship => "no friendship possible",
            DealBlock::NotAtWar => "not at war",
            DealBlock::CannotDeclareWar => "cannot declare that war",
            DealBlock::CannotMakePeace => "cannot make that peace",
        }
    }
}

/// Why `giver` cannot give `item` to `receiver` now (None: it can).
/// `peace_in_deal`: the deal also makes peace (gold may then be given
/// without friendship).
pub fn item_problem(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    giver: PlayerId,
    receiver: PlayerId,
    item: DealItem,
    peace_in_deal: bool,
) -> Option<DealBlock> {
    let d = auth.diplomacy();
    let r = auth.module().relations();
    let round = auth.turn().round;
    let at_war = d.at_war(giver, receiver);
    let both = [giver, receiver];
    match item {
        DealItem::Gold(gold) => {
            if !peace_in_deal && !r.are_friends(giver, receiver, round) {
                return Some(DealBlock::GoldNeedsFriendship);
            }
            if gold == 0 || auth.players().treasury_of(giver) < gold {
                return Some(DealBlock::NotEnoughGold);
            }
        }
        DealItem::GoldPerTurn(gold) => {
            if at_war && !peace_in_deal {
                return Some(DealBlock::AtWar);
            }
            let income = crate::derived::income::player_income(auth, rules, giver).gold;
            if gold == 0 || gold as i32 > income.max(0) {
                return Some(DealBlock::NotEnoughIncome);
            }
        }
        DealItem::Resource { resource, amount } => {
            if at_war && !peace_in_deal {
                return Some(DealBlock::AtWar);
            }
            let class = rules
                .resources
                .get(resource.0 as usize)
                .and_then(|d| d.class);
            if !matches!(
                class,
                Some(ResourceClass::Luxury | ResourceClass::Strategic)
            ) {
                return Some(DealBlock::NotTradeable);
            }
            if amount == 0 || resource_available(auth, rules, giver, resource) < i32::from(amount) {
                return Some(DealBlock::NotEnoughResource);
            }
        }
        DealItem::City(city) => {
            let Some(core) = auth.cities().get(city) else {
                return Some(DealBlock::NoSuchCity);
            };
            let capital = crate::combat::city::capital_of(auth, rules, giver) == Some(city);
            if core.owner != giver || capital {
                return Some(DealBlock::NotACityToGive);
            }
        }
        DealItem::Embassy => {
            if r.has_embassy(receiver, giver) {
                return Some(DealBlock::EmbassyExists);
            }
            if auth.indexes().cities_of(receiver).is_empty() {
                return Some(DealBlock::NoCityForEmbassy);
            }
            if !tech_allows(auth, rules, &[receiver], |t| t.allows_embassy) {
                return Some(DealBlock::EmbassyNeedsTechnology);
            }
        }
        DealItem::OpenBorders => {
            if at_war || d.grants_entry(giver, receiver) {
                return Some(DealBlock::BordersOpen);
            }
            if !tech_allows(auth, rules, &both, |t| t.allows_open_borders) {
                return Some(DealBlock::BordersNeedTechnology);
            }
            if !r.has_embassy(giver, receiver)
                && rules.technologies.iter().any(|t| t.allows_embassy)
            {
                return Some(DealBlock::BordersNeedEmbassy);
            }
        }
        DealItem::DefensivePact => {
            if at_war || r.has_pact(giver, receiver, round) {
                return Some(DealBlock::NoPact);
            }
            if !tech_allows(auth, rules, &both, |t| t.allows_defensive_pact) {
                return Some(DealBlock::PactNeedsTechnology);
            }
            if !(r.has_embassy(giver, receiver) && r.has_embassy(receiver, giver)) {
                return Some(DealBlock::PactNeedsEmbassies);
            }
        }
        DealItem::ResearchAgreement => {
            if at_war || r.has_research_agreement(giver, receiver, round) {
                return Some(DealBlock::NoAgreement);
            }
            if !tech_allows(auth, rules, &both, |t| t.allows_research_agreement) {
                return Some(DealBlock::AgreementNeedsTechnology);
            }
            if !(r.has_embassy(giver, receiver) && r.has_embassy(receiver, giver)) {
                return Some(DealBlock::AgreementNeedsEmbassies);
            }
            if !r.are_friends(giver, receiver, round) {
                return Some(DealBlock::AgreementNeedsFriendship);
            }
            let cost = research_agreement_cost(auth, rules, giver, receiver);
            if auth.players().treasury_of(giver) < cost
                || auth.players().treasury_of(receiver) < cost
            {
                return Some(DealBlock::NotEnoughGold);
            }
        }
        DealItem::Friendship => {
            if at_war || r.are_friends(giver, receiver, round) {
                return Some(DealBlock::NoFriendship);
            }
        }
        DealItem::Peace => {
            if !at_war {
                return Some(DealBlock::NotAtWar);
            }
        }
        DealItem::ThirdPartyWar(third) => {
            let ok = third != giver
                && third != receiver
                && auth.players().is_alive(third)
                && has_met(auth, giver, third)
                && has_met(auth, receiver, third)
                && !d.at_war(giver, third)
                && round >= r.peace_treaty_until(giver, third);
            if !ok {
                return Some(DealBlock::CannotDeclareWar);
            }
        }
        DealItem::ThirdPartyPeace(third) => {
            let ok = third != giver
                && third != receiver
                && d.at_war(giver, third)
                && has_met(auth, receiver, third);
            if !ok {
                return Some(DealBlock::CannotMakePeace);
            }
        }
    }
    None
}

/// Problems of a whole offer (None: every item is possible).
pub fn offer_problem(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    from: PlayerId,
    to: PlayerId,
    items: &[(PlayerId, DealItem)],
) -> Option<&'static str> {
    if items.is_empty() || items.len() > MAX_DEAL_ITEMS {
        return Some("empty or too large deal");
    }
    let peace = items.iter().any(|(_, i)| *i == DealItem::Peace);
    for (giver, item) in items {
        if *giver != from && *giver != to {
            return Some("a third player cannot give");
        }
        let receiver = if *giver == from { to } else { from };
        if let Some(problem) = item_problem(auth, rules, *giver, receiver, *item, peace) {
            return Some(problem.reason());
        }
    }
    None
}

impl<'a> WorldTransaction<'a> {
    pub(crate) fn propose_deal(
        &mut self,
        from: PlayerId,
        to: PlayerId,
        items: Vec<(PlayerId, DealItem)>,
    ) -> Result<(), CommandError> {
        let round = self.world.turn.round;
        let relations = &mut self.world.module.relations;
        let id = relations.next_offer;
        relations.next_offer += 1;
        relations.offers.push(DealOffer {
            id,
            from,
            to,
            items,
            expires: round + OFFER_ROUNDS,
        });
        self.relations_changed(to);
        Ok(())
    }

    /// The answer to an offer: accepted deals run item by item (peace
    /// last); the offer leaves the list either way.
    pub(crate) fn respond_deal(
        &mut self,
        offer: u32,
        accept: bool,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        let position = self
            .world
            .module
            .relations
            .offers
            .iter()
            .position(|o| o.id == offer)
            .ok_or(CommandError::RuleViolation("no such offer".into()))?;
        // Checked before anything changes: a refused command leaves the world as it was.
        let pending = &self.world.module.relations.offers[position];
        if accept && offer_problem(self.world, rules, pending.from, pending.to, &pending.items).is_some() {
            return Err(CommandError::RuleViolation(
                "the deal is no longer possible".into(),
            ));
        }
        let offer = self.world.module.relations.offers.remove(position);
        self.relations_changed(offer.from);
        if !accept {
            return Ok(());
        }
        let mut items = offer.items.clone();
        items.sort_by_key(|(_, i)| *i == DealItem::Peace);
        for (giver, item) in items {
            let receiver = if giver == offer.from {
                offer.to
            } else {
                offer.from
            };
            self.run_item(giver, receiver, item, rules)?;
        }
        Ok(())
    }

    fn run_item(
        &mut self,
        giver: PlayerId,
        receiver: PlayerId,
        item: DealItem,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        let round = self.world.turn.round;
        match item {
            DealItem::Gold(gold) => {
                if self.world.players.spend(giver, gold) {
                    self.world.players.earn(receiver, gold);
                }
            }
            DealItem::GoldPerTurn(gold) => {
                self.add_trade(giver, receiver, TradeFlow::GoldPerTurn(gold))
            }
            DealItem::Resource { resource, amount } => {
                self.add_trade(giver, receiver, TradeFlow::Resource { resource, amount })
            }
            DealItem::City(city) => {
                self.transfer_city(city, giver, receiver, rules)?;
                if let Some(core) = self.world.cities.get_mut(city) {
                    core.status = crate::world::city::CityStatus::Own;
                }
            }
            DealItem::Embassy => self.open_embassy(receiver, giver, rules),
            DealItem::OpenBorders => self.open_borders(giver, receiver, rules)?,
            DealItem::DefensivePact => {
                let relations = &mut self.world.module.relations;
                let players = relations.players;
                RelationsState::set_pair(
                    &mut relations.pact_until,
                    players,
                    giver,
                    receiver,
                    round + DEAL_DURATION,
                );
            }
            DealItem::ResearchAgreement => {
                let cost = research_agreement_cost(self.world, rules, giver, receiver);
                self.world.players.spend(giver, cost);
                self.world.players.spend(receiver, cost);
                let relations = &mut self.world.module.relations;
                let players = relations.players;
                RelationsState::set_pair(
                    &mut relations.research_until,
                    players,
                    giver,
                    receiver,
                    round + DEAL_DURATION,
                );
                if let (Some(i), Some(j)) = (
                    relations.slot(giver, receiver),
                    relations.slot(receiver, giver),
                ) {
                    relations.research_points[i] = 0;
                    relations.research_points[j] = 0;
                }
            }
            DealItem::Friendship => {
                let relations = &mut self.world.module.relations;
                let players = relations.players;
                RelationsState::set_pair(
                    &mut relations.friends_until,
                    players,
                    giver,
                    receiver,
                    round + RELATIONSHIP_DURATION,
                );
            }
            DealItem::Peace => self.make_peace(giver, receiver, rules)?,
            DealItem::ThirdPartyWar(third) => self.declare_war(giver, third, rules)?,
            DealItem::ThirdPartyPeace(third) => self.make_peace(giver, third, rules)?,
        }
        self.relations_changed(giver);
        self.relations_changed(receiver);
        Ok(())
    }

    /// `guest` opens an embassy in `host`'s capital and learns the capital
    /// and the tiles around it as they are now (classic/docs/classic/rules.md).
    fn open_embassy(&mut self, guest: PlayerId, host: PlayerId, rules: &CompiledRuleset) {
        let relations = &mut self.world.module.relations;
        if let Some(i) = relations.slot(guest, host) {
            relations.embassy[i] = 1;
        }
        let capital = crate::combat::city::capital_of(self.world, rules, host).and_then(|c| self.world.cities.get(c));
        if let Some(tile) = capital.map(|c| c.tile) {
            self.journal.player_changes.push(PlayerChange::AreaRevealed { player: guest, tile, radius: EMBASSY_REVEAL_RADIUS });
        }
    }

    fn add_trade(&mut self, from: PlayerId, to: PlayerId, flow: TradeFlow) {
        let until = self.world.turn.round + DEAL_DURATION;
        self.world.module.relations.trades.push(OngoingTrade {
            from,
            to,
            flow,
            until,
        });
    }
}
