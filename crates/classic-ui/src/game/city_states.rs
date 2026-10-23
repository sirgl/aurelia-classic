//! City-states the player met: influence with its resting point and thresholds, the
//! ally, quests, and the gifts, pledges and tribute demands the core accepts now.
use classic_rules::ClassicGame;
use classic_rules::ids::PlayerId;
use classic_rules::knowledge::observation::CityStateInfo;
use classic_rules::rules::content::CityStateTrait;
use classic_rules::world::city_states::{
    gold_gift_influence, quest_reward, type_of, QuestKind, ALLIES_THRESHOLD, FRIENDS_THRESHOLD,
};
use classic_rules::Command;

use i18n::{t, tf};

use super::session::Session;
use crate::ui::model::{CityStateCard, CityStatesView, GameRequest, GoldGift, NoticeKind};

/// The gold gifts the panel offers.
pub const GIFTS: [u32; 3] = [250, 500, 1000];

impl Session {
    pub fn city_states_view(&mut self) -> CityStatesView {
        let observation = self.game().ai_observation(self.rules(), self.me());
        let infos: Vec<CityStateInfo> = observation.city_states.clone();
        let gold = observation.economy.treasury;
        let cards = infos.iter().map(|i| self.city_state_card(i)).collect();
        CityStatesView { gold, cards }
    }

    fn city_state_card(&mut self, info: &CityStateInfo) -> CityStateCard {
        let (me, city_state) = (self.me(), info.player);
        let auth = self.game().view().auth;
        let (friends_at, allies_at) = match type_of(auth, self.rules(), city_state) {
            Some(kind) => (kind.friends_threshold, kind.allies_threshold),
            None => (FRIENDS_THRESHOLD, ALLIES_THRESHOLD),
        };
        let ally = info.ally.map(|a| self.ally_name(a));
        CityStateCard {
            player: city_state,
            name: self.player_name(city_state),
            kind: trait_name(info.minor_trait),
            influence: info.influence,
            resting: info.resting,
            friends_at,
            allies_at,
            status: status(info, me),
            ally,
            pledged: info.pledged,
            quests: info.quests.iter().map(|q| self.quest_text(*q)).collect(),
            gifts: self.gold_gifts(info),
            pledge: self.availability(&Command::PledgeProtection {
                player: me,
                city_state,
                pledge: !info.pledged,
            }),
            bully_gold: info.bully_gold,
            bully_gold_state: self.availability(&Command::BullyGold {
                player: me,
                city_state,
            }),
            bully_worker: self.availability(&Command::BullyWorker {
                player: me,
                city_state,
            }),
            buyout: self.buyout(info),
        }
    }

    /// The buyout price and the core's verdict, when the player may annex city-states.
    fn buyout(&mut self, info: &CityStateInfo) -> Option<(u32, crate::ui::model::Availability)> {
        if !info.can_buyout {
            return None;
        }
        let auth = self.game().view().auth;
        let cost = classic_rules::world::city_states::buyout_cost(auth, self.rules(), info.player);
        let command = Command::BuyoutCityState {
            player: self.me(),
            city_state: info.player,
        };
        Some((cost, self.availability(&command)))
    }

    fn ally_name(&self, ally: PlayerId) -> String {
        if ally == self.me() {
            return t!("classic.game.city-states.you").into();
        }
        self.player_name(ally)
    }

    /// The gifts and the influence each buys now (more while an Invest quest runs).
    fn gold_gifts(&mut self, info: &CityStateInfo) -> Vec<GoldGift> {
        let round = self.game().view().auth.turn().round;
        let invest = info.quests.contains(&QuestKind::Invest);
        let mut out = Vec::new();
        for gold in GIFTS {
            let command = Command::GiftGold {
                player: self.me(),
                city_state: info.player,
                gold,
            };
            out.push(GoldGift {
                gold,
                influence: gold_gift_influence(round, gold, invest),
                availability: self.availability(&command),
            });
        }
        out
    }

    fn quest_text(&self, quest: QuestKind) -> String {
        let task = match quest {
            QuestKind::KillCamp(_) => t!("classic.game.city-states.quest-kill-camp").to_string(),
            QuestKind::ConnectResource(r) => tf!("classic.game.city-states.quest-connect", name = self.resource_name(r)),
            QuestKind::ConstructWonder(b) => {
                tf!("classic.game.city-states.quest-build", name = super::names::building(self.rules(), b))
            }
            QuestKind::FindPlayer(p) => tf!("classic.game.city-states.quest-find", name = self.player_name(p)),
            QuestKind::FindNaturalWonder { .. } => t!("classic.game.city-states.quest-natural-wonder").to_string(),
            QuestKind::GiveGold { .. } => t!("classic.game.city-states.quest-give-gold").to_string(),
            QuestKind::PledgeToProtect => t!("classic.game.city-states.quest-pledge").to_string(),
            QuestKind::BullyCityState(p) => tf!("classic.game.city-states.quest-bully", name = self.player_name(p)),
            QuestKind::DenounceMajor(p) => tf!("classic.game.city-states.quest-denounce", name = self.player_name(p)),
            QuestKind::ContestTechs { .. } => t!("classic.game.city-states.quest-contest").to_string(),
            QuestKind::Invest => t!("classic.game.city-states.quest-invest").to_string(),
        };
        match quest_reward(quest) {
            0 => task,
            reward => tf!("classic.game.city-states.quest-reward", task = task, n = reward),
        }
    }

    pub(super) fn city_state_request(&mut self, request: GameRequest) {
        let player = self.me();
        let request_gold = match request {
            GameRequest::GiftGold { gold, .. } => Some(gold),
            _ => None,
        };
        let (command, city_state, done) = match request {
            GameRequest::GiftGold { city_state, gold } => (
                Command::GiftGold {
                    player,
                    city_state,
                    gold,
                },
                city_state,
                i18n::key!("classic.game.city-states.done-gift"),
            ),
            GameRequest::BullyGold(city_state) => (
                Command::BullyGold { player, city_state },
                city_state,
                i18n::key!("classic.game.city-states.done-bully-gold"),
            ),
            GameRequest::BuyoutCityState(city_state) => (
                Command::BuyoutCityState { player, city_state },
                city_state,
                i18n::key!("classic.game.city-states.done-buyout"),
            ),
            GameRequest::BullyWorker(city_state) => (
                Command::BullyWorker { player, city_state },
                city_state,
                i18n::key!("classic.game.city-states.done-bully-worker"),
            ),
            GameRequest::Pledge { city_state, pledge } => (
                Command::PledgeProtection {
                    player,
                    city_state,
                    pledge,
                },
                city_state,
                if pledge {
                    i18n::key!("classic.game.city-states.done-pledge")
                } else {
                    i18n::key!("classic.game.city-states.done-unpledge")
                },
            ),
            _ => return,
        };
        let gold = request_gold.unwrap_or(0);
        let text = i18n::tr_args(done, &[("name", self.player_name(city_state).into()), ("gold", gold.into())]);
        if self.submit(command) {
            self.notify(NoticeKind::Diplomacy, text);
        }
    }
}

fn trait_name(minor_trait: Option<CityStateTrait>) -> &'static str {
    match minor_trait {
        Some(CityStateTrait::Cultured) => t!("classic.game.city-states.trait-cultured"),
        Some(CityStateTrait::Militaristic) => t!("classic.game.city-states.trait-militaristic"),
        Some(CityStateTrait::Maritime) => t!("classic.game.city-states.trait-maritime"),
        Some(CityStateTrait::Mercantile) => t!("classic.game.city-states.trait-mercantile"),
        Some(CityStateTrait::Religious) => t!("classic.game.city-states.trait-religious"),
        None => t!("classic.game.city-states.trait-none"),
    }
}

fn status(info: &CityStateInfo, me: PlayerId) -> &'static str {
    if info.at_war {
        return t!("classic.game.city-states.status-war");
    }
    if info.ally == Some(me) {
        return t!("classic.game.city-states.status-allies");
    }
    if info.friends {
        return t!("classic.game.city-states.status-friends");
    }
    if info.influence < 0 {
        return t!("classic.game.city-states.status-angry");
    }
    t!("classic.game.city-states.status-neutral")
}
