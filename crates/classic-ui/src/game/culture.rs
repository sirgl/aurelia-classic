//! Culture overview from the player's tourism observation (`derived::tourism`): tourism and
//! influence on every nation met, the great works in each building's slots with its
//! theme, public opinion with the ideology switch, and the finished digs with their
//! choices. Two requests: SwitchIdeology and ChooseArchaeology.
use classic_rules::derived::archaeology::{pending_choices, ArchaeologyChoice};
use classic_rules::derived::tourism::{
    theme_of, tourism_observation, works_in, InfluenceLevel, InfluenceTrend, InfluenceView,
};
use classic_rules::rules::content::GreatWorkSlot;
use classic_rules::world::tourism::{GreatWork, GreatWorkClass, GreatWorkId, PublicOpinion};
use classic_rules::Command;
use i18n::{t, tf};

use super::names;
use super::session::Session;
use crate::ui::model::{
    CultureView, DigCard, GameRequest, GreatWorkCard, InfluenceRow, NoticeKind, WorkBuildingCard,
};

impl Session {
    pub fn culture_view(&mut self) -> CultureView {
        let me = self.me();
        let switch = self.availability(&Command::SwitchIdeology { player: me });
        let auth = self.game().view().auth;
        let rules = self.rules();
        let obs = tourism_observation(auth, rules, me);
        let state = auth.module().culture().of(me);
        let branch = |b: classic_rules::rules::content::PolicyBranchId| {
            let def = &rules.module.policy_branches[b.0 as usize];
            names::named("policy", &def.name, &def.key)
        };
        let ideology = match state.ideology {
            Some(b) => tf!("classic.game.culture.ideology",
                name = branch(b),
                opinion = opinion_text(obs.opinion, obs.dissatisfaction)
            ),
            None => t!("classic.game.culture.no-ideology").to_string(),
        };
        CultureView {
            tourism: obs.tourism,
            culture_per_turn: classic_rules::derived::culture::culture_per_turn(auth, rules, me),
            influential_on: obs.influential_on,
            needed_for_victory: obs.needed_for_victory,
            ideology,
            preferred: obs.preferred.map(branch),
            switch,
            influence: obs
                .influence
                .iter()
                .map(|v| self.influence_row(v))
                .collect(),
            buildings: self.work_buildings(),
            digs: self.dig_cards(),
            free_great_people: classic_rules::derived::grants::next_great_person_options(
                auth, rules, me,
            )
            .into_iter()
            .map(|k| (k, crate::ui::pedia::describe::great_person(k)))
            .collect(),
            sites: obs.sites.len() as u32,
        }
    }

    fn influence_row(&self, v: &InfluenceView) -> InfluenceRow {
        InfluenceRow {
            nation: self.player_name(v.other),
            level: level_name(v.level),
            percent: v.permille / 10,
            per_turn: v.per_turn,
            trend: match v.trend {
                InfluenceTrend::Rising => t!("classic.game.culture.trend-rising"),
                InfluenceTrend::Falling => t!("classic.game.culture.trend-falling"),
                InfluenceTrend::Static => t!("classic.game.culture.trend-static"),
            },
            their_level: level_name(v.their_level),
        }
    }

    /// Every building with great work slots in the player's cities, city order.
    fn work_buildings(&self) -> Vec<WorkBuildingCard> {
        let auth = self.game().view().auth;
        let rules = self.rules();
        let mut out = Vec::new();
        for city in auth.indexes().cities_of(self.me()) {
            let Some(core) = auth.cities().get(*city) else {
                continue;
            };
            for building in &core.buildings {
                let def = &rules.buildings[building.0 as usize];
                let Some((slot, count)) = def.module.great_work_slots else {
                    continue;
                };
                let works = works_in(auth, *city, *building);
                let mut slots: Vec<Option<GreatWorkCard>> = auth
                    .module()
                    .tourism()
                    .city(*city)
                    .in_building(*building)
                    .filter_map(|id| {
                        auth.module()
                            .tourism()
                            .work(id)
                            .map(|work| Some(self.work_card(id, work)))
                    })
                    .collect();
                slots.resize(usize::from(count), None);
                out.push(WorkBuildingCard {
                    city: core.name.clone(),
                    building: names::building(rules, *building),
                    art: names::art(&def.key),
                    slot: slot_name(slot),
                    works: slots,
                    theme: theme_of(rules, *building, &works, core.owner).map(|t| t.bonus),
                });
            }
        }
        out
    }

    fn work_card(&self, id: GreatWorkId, work: &GreatWork) -> GreatWorkCard {
        let class = match work.class {
            GreatWorkClass::Art => t!("classic.game.culture.class-art"),
            GreatWorkClass::Artifact => t!("classic.game.culture.class-artifact"),
            GreatWorkClass::Literature => t!("classic.game.culture.class-writing"),
            GreatWorkClass::Music => t!("classic.game.culture.class-music"),
        };
        let era = self
            .rules()
            .eras
            .iter()
            .find(|e| e.index == work.era)
            .map(|e| names::named("era", &e.name, &e.key))
            .unwrap_or_default();
        GreatWorkCard {
            id: id.0,
            class,
            provenance: tf!("classic.game.culture.provenance", nation = self.player_name(work.creator), era = era),
            art: crate::ui::pedia::art::great_work(work.class, id.0 as usize),
        }
    }

    fn dig_cards(&self) -> Vec<DigCard> {
        let auth = self.game().view().auth;
        pending_choices(auth, self.rules(), self.me())
            .into_iter()
            .map(|(tile, open)| DigCard {
                tile,
                label: dig_label(auth, tile),
                choices: open.into_iter().map(|c| (c, choice_name(c))).collect(),
            })
            .collect()
    }

    pub(super) fn culture_request(&mut self, request: GameRequest) {
        let player = self.me();
        let (command, done) = match request {
            GameRequest::SwitchIdeology => (
                Command::SwitchIdeology { player },
                t!("classic.game.culture.ideology-switched").to_string(),
            ),
            GameRequest::ChooseArchaeology { tile, choice } => (
                Command::ChooseArchaeology {
                    player,
                    tile,
                    choice,
                },
                tf!("classic.game.culture.dig-chosen", choice = choice_name(choice)),
            ),
            GameRequest::ChooseFreeGreatPerson(kind) => (
                Command::ChooseFreeGreatPerson { player, kind },
                tf!("classic.game.culture.free-great-person",
                    name = crate::ui::pedia::describe::great_person(kind)
                ),
            ),
            _ => return,
        };
        if self.submit(command) {
            self.notify(NoticeKind::Culture, done);
        }
    }
}

fn opinion_text(opinion: PublicOpinion, dissatisfaction: u16) -> String {
    match opinion {
        PublicOpinion::Content => t!("classic.game.culture.opinion-content").to_string(),
        PublicOpinion::Murmurs => tf!("classic.game.culture.opinion-murmurs", n = dissatisfaction),
        PublicOpinion::Unrest => tf!("classic.game.culture.opinion-unrest", n = dissatisfaction),
        PublicOpinion::Upheaval => tf!("classic.game.culture.opinion-upheaval", n = dissatisfaction),
    }
}

fn slot_name(slot: GreatWorkSlot) -> &'static str {
    match slot {
        GreatWorkSlot::ArtArtifact => t!("classic.game.culture.slot-art-artifact"),
        GreatWorkSlot::Literature => t!("classic.game.culture.slot-writing"),
        GreatWorkSlot::Music => t!("classic.game.culture.slot-music"),
    }
}

fn choice_name(choice: ArchaeologyChoice) -> &'static str {
    match choice {
        ArchaeologyChoice::Landmark => t!("classic.game.culture.choice-landmark"),
        ArchaeologyChoice::ArtifactFirst => t!("classic.game.culture.class-artifact"),
        ArchaeologyChoice::ArtifactSecond => t!("classic.game.culture.choice-artifact-second"),
        ArchaeologyChoice::Writing => t!("classic.game.culture.class-writing"),
        ArchaeologyChoice::CultureBoost => t!("classic.game.culture.choice-culture"),
    }
}

fn dig_label(auth: &classic_rules::world::state::AuthWorld, tile: classic_rules::ids::TileId) -> String {
    let dims = auth.map().dimensions;
    match dims.to_coord(tile) {
        Some(c) => tf!("classic.game.culture.dig-at", q = c.q, r = c.r),
        None => t!("classic.game.culture.dig").to_string(),
    }
}

fn level_name(level: InfluenceLevel) -> &'static str {
    match level {
        InfluenceLevel::Unknown => t!("classic.game.culture.level-unknown"),
        InfluenceLevel::Noticed => t!("classic.game.culture.level-noticed"),
        InfluenceLevel::Admired => t!("classic.game.culture.level-admired"),
        InfluenceLevel::Imitated => t!("classic.game.culture.level-imitated"),
        InfluenceLevel::Embraced => t!("classic.game.culture.level-embraced"),
        InfluenceLevel::Prevailing => t!("classic.game.culture.level-prevailing"),
    }
}
