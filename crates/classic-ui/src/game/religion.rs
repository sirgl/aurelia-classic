//! Religion screen data from the belief and religion tables, the player's faith and the
//! public facts of every founded religion, and the four faith requests (pantheon, found,
//! enhance, Reformation). A Great Prophet request uses the player's first prophet standing
//! in one of its cities (else its first prophet): the core says why it cannot act.
use classic_rules::derived::religion::{
    faith_overview, faith_per_turn, faith_purchase_status, pantheon_status, FaithOverview,
    ReligionSummary,
};
use classic_rules::ids::{BeliefId, ProductionItem, UnitHandle};
use classic_rules::rules::content::{BeliefCategory, UnitAbility};
use classic_rules::{Command, CompiledRuleset};
use i18n::{t, tf};

use super::names;
use super::reasons::{faith, faith_purchase};
use super::session::Session;
use crate::ui::model::{
    Availability, BeliefCard, FaithChoice, FaithOffer, GameRequest, NoticeKind, ReligionCard,
    ReligionView,
};

impl Session {
    pub fn religion_view(&mut self) -> ReligionView {
        let me = self.me();
        let auth = self.game().view().auth;
        let rules = self.rules();
        let per_turn = faith_per_turn(auth, rules, me);
        let overview = faith_overview(auth, rules, me, per_turn);
        let cards = |ids: &[BeliefId]| {
            ids.iter()
                .map(|b| belief_card(rules, *b))
                .collect::<Vec<_>>()
        };
        let world: Vec<ReligionCard> = overview
            .religions
            .iter()
            .map(|r| self.religion_card(r))
            .collect();
        let choice = self.faith_choice(&overview);
        ReligionView {
            faith: overview.faith,
            per_turn,
            pantheon_cost: overview.pantheon_cost,
            prophet_cost: overview.prophet_cost,
            religions_left: overview.religions_left,
            pantheon: overview.pantheon.map(|b| belief_card(rules, b)),
            religion: world.iter().find(|r| r.mine).cloned(),
            choice,
            pantheons: cards(&overview.free_pantheon),
            founders: cards(&overview.free_founder),
            followers: cards(&overview.free_follower),
            enhancers: cards(&overview.free_enhancer),
            reformations: cards(&overview.free_reformation),
            free_religions: overview
                .free_religions
                .iter()
                .map(|r| (*r, religion_name(rules, *r)))
                .collect(),
            world,
            purchases: self.faith_offers(),
        }
    }

    /// Items faith can buy (religious units, great people, buildings with a faith price):
    /// the first own city where each can be bought, else the capital's reason.
    fn faith_offers(&self) -> Vec<FaithOffer> {
        let me = self.me();
        let auth = self.game().view().auth;
        let rules = self.rules();
        let cities: Vec<_> = auth.indexes().cities_of(me).to_vec();
        let Some(first) = cities.first().copied() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for item in faith_items(rules) {
            let mut offer = None;
            for city in &cities {
                if let Ok(cost) = faith_purchase_status(auth, rules, me, *city, item) {
                    offer = Some((*city, Ok(cost)));
                    break;
                }
            }
            let (city, status) = match offer {
                Some(found) => found,
                None => (first, faith_purchase_status(auth, rules, me, first, item)),
            };
            let core = auth.cities().get(city);
            let (name, art) = match item {
                ProductionItem::Unit(k) => (
                    names::unit(rules, k),
                    classic_art(&rules.units[k.0 as usize].key),
                ),
                ProductionItem::Building(b) => (
                    names::building(rules, b),
                    classic_art(&rules.buildings[b.0 as usize].key),
                ),
                ProductionItem::Project(p) => (
                    names::item(rules, item),
                    classic_art(&rules.module.projects[p.0 as usize].key),
                ),
            };
            let (cost, availability) = match status {
                Ok(cost) => (Some(cost), Availability::Ready),
                Err(block) => (None, Availability::Blocked(faith_purchase(block).into())),
            };
            out.push(FaithOffer {
                item,
                name,
                art,
                city: core.map(|c| c.name.clone()).unwrap_or_default(),
                city_tile: core.map(|c| c.tile).unwrap_or(classic_rules::ids::TileId(0)),
                cost,
                availability,
            });
        }
        out
    }

    fn religion_card(&self, summary: &ReligionSummary) -> ReligionCard {
        let rules = self.rules();
        let auth = self.game().view().auth;
        let holy_city = auth
            .cities()
            .get(summary.holy_city)
            .map(|c| c.name.clone())
            .unwrap_or_default();
        ReligionCard {
            id: summary.religion,
            name: religion_name(rules, summary.religion),
            founder: self.civilization_name(summary.founder),
            holy_city,
            cities: summary.counts.cities,
            followers: summary.counts.followers,
            enhanced: summary.enhanced,
            beliefs: summary
                .beliefs
                .iter()
                .map(|b| belief_card(rules, *b))
                .collect(),
            mine: summary.founder == self.me(),
        }
    }

    /// The choice the screen offers: pantheon first, then what a Great Prophet can do,
    /// then Reformation, else browsing.
    fn faith_choice(&self, overview: &FaithOverview) -> FaithChoice {
        let me = self.me();
        let auth = self.game().view().auth;
        let rules = self.rules();
        if overview.pantheon.is_none() && overview.religion.is_none() {
            let availability = match overview.free_pantheon.first() {
                Some(b) => match pantheon_status(auth, rules, me, *b) {
                    Ok(_) => Availability::Ready,
                    Err(block) => Availability::Blocked(faith(block).into()),
                },
                None => Availability::Blocked(super::reasons::no_pantheon_belief().into()),
            };
            if self.prophet().is_none() {
                return FaithChoice::Pantheon(availability);
            }
        }
        if self.prophet().is_some() {
            if overview.religion.is_none() && overview.religions_left > 0 {
                return FaithChoice::Found {
                    needs_pantheon: overview.pantheon.is_none(),
                };
            }
            let enhanced = overview
                .religions
                .iter()
                .any(|r| Some(r.religion) == overview.religion && r.enhanced);
            if overview.religion.is_some() && !enhanced {
                return FaithChoice::Enhance;
            }
        }
        if overview.can_add_reformation {
            return FaithChoice::Reformation;
        }
        let hint = if overview.religion.is_none() {
            tf!("classic.game.religion.hint-prophet",
                cost = overview.prophet_cost,
                n = overview.religions_left
            )
        } else {
            t!("classic.game.religion.hint-spread").to_string()
        };
        FaithChoice::Browse(hint)
    }

    /// The player's first Great Prophet standing in one of its cities, else its first one.
    fn prophet(&self) -> Option<UnitHandle> {
        let auth = self.game().view().auth;
        let rules = self.rules();
        let mut first = None;
        for handle in auth.indexes().units_of(self.me()) {
            let Some(view) = auth.units().get(*handle) else {
                continue;
            };
            let prophet = rules
                .units
                .get(view.core.kind.0 as usize)
                .is_some_and(|u| u.module.abilities.contains(&UnitAbility::FoundReligion));
            if !prophet {
                continue;
            }
            if auth.indexes().city_on_tile(view.core.tile).is_some() {
                return Some(*handle);
            }
            first = first.or(Some(*handle));
        }
        first
    }

    pub(super) fn religion_request(&mut self, request: GameRequest) {
        let player = self.me();
        let prophet = self.prophet();
        let (command, done) = match (request, prophet) {
            (GameRequest::FoundPantheon(belief), _) => (
                Command::FoundPantheon { player, belief },
                tf!("classic.game.religion.pantheon-founded", belief = belief_name(self.rules(), belief)),
            ),
            (GameRequest::AddReformation(belief), _) => (
                Command::AddReformationBelief { player, belief },
                tf!("classic.game.religion.reformation-added", belief = belief_name(self.rules(), belief)),
            ),
            (
                GameRequest::FoundReligion {
                    religion,
                    founder,
                    follower,
                    pantheon,
                },
                Some(unit),
            ) => (
                Command::FoundReligion {
                    unit,
                    religion,
                    founder,
                    follower,
                    pantheon,
                    bonus: None,
                },
                tf!("classic.game.religion.founded", religion = religion_name(self.rules(), religion)),
            ),
            (GameRequest::BuyWithFaith { city_tile, item }, _) => {
                let Some(city) = self.own_city_at(city_tile) else {
                    return;
                };
                let name = match item {
                    ProductionItem::Unit(k) => names::unit(self.rules(), k),
                    ProductionItem::Building(b) => names::building(self.rules(), b),
                    ProductionItem::Project(_) => names::item(self.rules(), item),
                };
                (
                    Command::PurchaseWithFaith { player, city, item },
                    tf!("classic.game.religion.bought", name = name),
                )
            }
            (GameRequest::EnhanceReligion { follower, enhancer }, Some(unit)) => (
                Command::EnhanceReligion {
                    unit,
                    follower,
                    enhancer,
                },
                t!("classic.game.religion.enhanced").to_string(),
            ),
            _ => return,
        };
        if self.submit(command) {
            self.notify(NoticeKind::Faith, done);
        }
    }
}

/// Items with a faith price the screen lists: religious units and great people first,
/// then buildings bought with faith.
fn faith_items(rules: &CompiledRuleset) -> Vec<ProductionItem> {
    let unit = |i: usize| ProductionItem::Unit(classic_rules::ids::UnitKindId(i as u16));
    let religious = |u: &classic_rules::rules::compiled::UnitDefinition| {
        u.module.abilities.iter().any(|a| {
            matches!(
                a,
                UnitAbility::SpreadReligion
                    | UnitAbility::RemoveHeresy
                    | UnitAbility::FoundReligion
            )
        })
    };
    let mut out: Vec<ProductionItem> = (rules.units.iter().enumerate())
        .filter(|(_, u)| religious(u) && u.module.faith_cost.is_some())
        .map(|(i, _)| unit(i))
        .collect();
    for (i, u) in rules.units.iter().enumerate() {
        let great_person = rules.module.great_people.iter().any(|g| g.unit.0 as usize == i);
        if great_person && !religious(u) {
            out.push(unit(i));
        }
    }
    for (i, b) in rules.buildings.iter().enumerate() {
        if b.module.faith_cost.is_some() {
            out.push(ProductionItem::Building(classic_rules::ids::BuildingKindId(
                i as u16,
            )));
        }
    }
    out
}

fn belief_card(rules: &CompiledRuleset, id: BeliefId) -> BeliefCard {
    let def = &rules.module.beliefs[id.0 as usize];
    BeliefCard {
        id,
        name: names::named("belief", &def.name, &def.key),
        text: if def.key == "core:belief/pantheon/god-king" {
            t!("classic.game.religion.god-king-text").into()
        } else if def.text.is_empty() {
            crate::ui::pedia::describe::effect_summary(rules, &def.effects, &def.inert)
        } else {
            i18n::content("classic", "belief", &def.key, "desc", &def.text)
        },
        art: classic_art(&def.key),
        category: match def.category {
            Some(BeliefCategory::Pantheon) => t!("classic.game.religion.category-pantheon"),
            Some(BeliefCategory::Founder) => t!("classic.game.religion.category-founder"),
            Some(BeliefCategory::Follower) => t!("classic.game.religion.category-follower"),
            Some(BeliefCategory::Enhancer) => t!("classic.game.religion.category-enhancer"),
            Some(BeliefCategory::Reformation) => t!("classic.game.religion.category-reformation"),
            None => t!("classic.game.religion.category-belief"),
        },
    }
}

/// Unit, building and belief art is indexed by the content key under `classic:`.
fn classic_art(key: &str) -> String {
    key.replacen("core:", "classic:", 1)
}

fn belief_name(rules: &CompiledRuleset, id: BeliefId) -> String {
    rules
        .module.beliefs
        .get(id.0 as usize)
        .map(|b| names::named("belief", &b.name, &b.key))
        .unwrap_or_default()
}

fn religion_name(rules: &CompiledRuleset, id: classic_rules::rules::content::ReligionId) -> String {
    let def = &rules.module.religions[id.0 as usize];
    names::named("religion", &def.name, &def.key)
}

