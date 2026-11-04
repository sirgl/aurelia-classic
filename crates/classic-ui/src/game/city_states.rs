//! City-states the player met: influence with its resting point and thresholds, the
//! ally, quests, and the gifts, pledges and tribute demands the core accepts now.
use classic_rules::ClassicGame;
use classic_rules::ids::PlayerId;
use classic_rules::knowledge::observation::CityStateInfo;
use classic_rules::rules::content::CityStateTrait;
use classic_rules::world::city_states::{
    gold_gift_influence_at, quest_reward, type_of, QuestKind, ALLIES_THRESHOLD, FRIENDS_THRESHOLD,
};
use classic_rules::Command;

use i18n::{t, tf};

use super::session::Session;
use classic_rules::ids::TileId;
use classic_rules::world::player::ClassicPlayers;
use crate::ui::model::{CityStateCard, CityStateWindow, CityStatesView, GameRequest, GoldGift, NoticeKind};

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

    /// The living city-state the player has met whose city stands on `tile` and is known to the
    /// player (seen or remembered: a click on fog must not reveal where a city-state lives).
    pub(super) fn city_state_at(&self, tile: TileId) -> Option<PlayerId> {
        let auth = self.game().view().auth;
        let owner = auth.cities().get(auth.indexes().city_on_tile(tile)?)?.owner;
        let met = self.city_state_is_met(owner);
        (met && self.city_is_known(tile, owner)).then_some(owner)
    }

    fn city_state_is_met(&self, player: PlayerId) -> bool {
        let auth = self.game().view().auth;
        let players = auth.players();
        players.is_city_state(player) && players.is_alive(player) && classic_rules::derived::tourism::met(auth, self.me(), player)
    }

    /// The map holds a city of `owner` on `tile`, seen now or remembered.
    fn city_is_known(&self, tile: TileId, owner: PlayerId) -> bool {
        self.view().cities.iter().any(|c| c.tile == tile && c.owner == owner.0)
    }

    /// A city of the met city-state that the player has seen or remembers (its capital first).
    pub fn known_city_of(&self, city_state: PlayerId) -> Option<TileId> {
        if !self.city_state_is_met(city_state) {
            return None;
        }
        let auth = self.game().view().auth;
        let cities = auth.indexes().cities_of(city_state);
        cities.iter().filter_map(|h| auth.cities().get(*h)).map(|c| c.tile).find(|t| self.city_is_known(*t, city_state))
    }

    /// The tile holds a city of the player.
    pub fn owns_city_at(&self, tile: TileId) -> bool {
        self.own_city_at(tile).is_some()
    }

    /// A click on a met city-state's banner opens its card over the map; false for any other tile.
    pub fn open_city_state_at(&mut self, tile: TileId) -> bool {
        let Some(city_state) = self.city_state_at(tile) else {
            return false;
        };
        self.city_state_open = Some(city_state);
        self.targeting = None;
        self.city_strike = None;
        true
    }

    /// "Open on map" on the city-state's card: its card opens over the map; the city to centre on,
    /// or None (nothing opens) when none of its cities is seen or remembered.
    pub fn show_city_state(&mut self, city_state: PlayerId) -> Option<TileId> {
        let tile = self.known_city_of(city_state)?;
        self.city_state_open = Some(city_state);
        self.targeting = None;
        self.city_strike = None;
        Some(tile)
    }

    /// The city-state whose card is open over the map.
    pub fn open_city_state(&self) -> Option<PlayerId> {
        self.city_state_open
    }

    pub fn close_city_state(&mut self) {
        self.city_state_open = None;
    }

    /// The open card as the City-States screen shows it, fresh from the core (influence, gold,
    /// verdicts); it shuts by itself when the city-state is gone or no longer met.
    pub(super) fn city_state_window(&mut self) -> Option<CityStateWindow> {
        let open = self.city_state_open?;
        let view = self.city_states_view();
        let window = view.cards.into_iter().find(|c| c.player == open).map(|card| CityStateWindow { gold: view.gold, card });
        if window.is_none() {
            self.city_state_open = None;
        }
        window
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
            on_map: match self.known_city_of(city_state) {
                Some(_) => crate::ui::model::Availability::Ready,
                None => crate::ui::model::Availability::Blocked(t!("classic.game.city-states.not-on-map").into()),
            },
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
        let speed = self.game().view().auth.settings().speed(self.rules());
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
                influence: gold_gift_influence_at(&speed, round, gold, invest, 0),
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

/// A game with city-states for the tests of the card over the map.
#[cfg(test)]
pub(crate) mod fixtures {
    use classic_rules::ids::{PlayerId, TileId};
    use classic_rules::world::player::ClassicPlayers;
    use classic_rules::ClassicGame;

    use crate::game::Session;
    use crate::ui::screens::game_rules;

    /// A session with two city-states (players 2 and 3): the first met with influence 40 and 300 gold
    /// in the treasury, the second not met; the human has a city and a unit.
    pub(crate) struct World {
        pub session: Session,
        pub met: PlayerId,
        pub met_city: TileId,
        pub unmet_city: TileId,
        pub hidden: PlayerId,
        /// Met, but its city was never seen.
        pub hidden_city: TileId,
        pub own_city: TileId,
        pub rival_city: TileId,
    }

    pub(crate) fn world() -> World {
        let rules = game_rules();
        let config = classic_rules::scenario::ScenarioConfig { seed: 7, players: 2, width: 32, height: 24, cities: 1, units: 1, water_ratio: 25 };
        let options = classic_rules::scenario::NewGameOptions { city_states: 3, ..Default::default() };
        let mut game = classic_rules::scenario::generate_new_game_with(&rules, &config, Some(0), options);
        let (met, unmet, hidden) = (PlayerId(2), PlayerId(3), PlayerId(4));
        game.debug_meet(PlayerId(0), met);
        game.debug_meet(PlayerId(0), hidden);
        game.debug_set_influence(&rules, met, PlayerId(0), 40);
        game.debug_earn(PlayerId(0), 300);
        let city_of = |game: &classic_rules::GameState, player: PlayerId| {
            let auth = game.view().auth;
            let handle = *auth.indexes().cities_of(player).first().expect("a city");
            auth.cities().get(handle).expect("the city").tile
        };
        // The nations start with settlers, not cities: found one for the human and one for the rival.
        for player in [PlayerId(0), PlayerId(1)] {
            let tile = game.view().auth.units().iter_cores().find(|u| u.owner == player).expect("a start unit").tile;
            game.debug_found_city(&rules, player, tile);
        }
        let (met_city, unmet_city) = (city_of(&game, met), city_of(&game, unmet));
        let (own_city, rival_city) = (city_of(&game, PlayerId(0)), city_of(&game, PlayerId(1)));
        let hidden_city = city_of(&game, hidden);
        game.debug_reveal(PlayerId(0), &[met_city, unmet_city, rival_city]);
        assert!(game.view().auth.players().is_city_state(met));
        World { session: Session::new(game, rules), met, met_city, unmet_city, hidden, hidden_city, own_city, rival_city }
    }
}

#[cfg(test)]
mod tests {
    use client::shell::module::{MapButton, ModuleUi};

    use super::fixtures::world;
    use super::*;
    use crate::ui::screens::Screen;
    use crate::ClassicUi;

    #[test]
    fn the_card_over_the_map_is_the_world_screens_card() {
        let mut w = world();
        assert!(w.session.hud_model(None).city_state.is_none());
        assert!(w.session.open_city_state_at(w.met_city));
        let window = w.session.hud_model(None).city_state.expect("the card opens");
        assert_eq!(window.card.player, w.met);
        let screen = w.session.city_states_view();
        assert_eq!(window.card, *screen.cards.iter().find(|c| c.player == w.met).unwrap(), "the same card as on the world screen");
        assert_eq!(window.gold, screen.gold);
        assert_eq!(window.card.influence, 40);
        assert_eq!(window.card.gifts.len(), 3);
    }

    #[test]
    fn only_a_met_city_states_known_city_opens_a_card() {
        let mut w = world();
        assert!(!w.session.open_city_state_at(w.unmet_city));
        assert!(!w.session.open_city_state_at(w.hidden_city), "met, but its city was never seen: a click on fog tells nothing");
        assert!(!w.session.open_city_state_at(w.own_city), "an own city is not a city-state");
        assert_eq!(w.session.open_city_state(), None);
    }

    #[test]
    fn a_gift_through_the_card_changes_influence_and_gold_like_on_the_world_screen() {
        let mut w = world();
        assert!(w.session.open_city_state_at(w.met_city));
        let before = w.session.hud_model(None).city_state.unwrap();
        assert_eq!(before.gold, 300);
        assert!(before.card.gifts[0].availability.is_ready());
        w.session.request(GameRequest::GiftGold { city_state: w.met, gold: 250 });
        let after = w.session.hud_model(None).city_state.expect("the card stays open and refreshes");
        assert_eq!(after.gold, 50);
        assert_eq!(after.card.influence, 40 + before.card.gifts[0].influence);
        assert!(!after.card.gifts[0].availability.is_ready(), "50 gold left: the core greys the gift");
        assert_eq!(after.card, *w.session.city_states_view().cards.iter().find(|c| c.player == w.met).unwrap());
    }

    #[test]
    fn esc_the_close_request_and_the_end_of_the_turn_shut_the_card() {
        let mut w = world();
        assert!(w.session.open_city_state_at(w.met_city));
        assert!(w.session.cancel_aim(), "Esc closes the card");
        assert!(w.session.hud_model(None).city_state.is_none());
        assert!(!w.session.cancel_aim(), "nothing more to close");
        assert!(w.session.open_city_state_at(w.met_city));
        w.session.request(GameRequest::CloseCityState);
        assert!(w.session.hud_model(None).city_state.is_none());
        assert!(w.session.open_city_state_at(w.met_city));
        w.session.request(GameRequest::EndTurn);
        assert!(w.session.hud_model(None).city_state.is_none(), "a new turn starts without the card");
    }

    #[test]
    fn a_click_on_the_tile_of_a_city_state_does_not_open_the_card() {
        let mut w = world();
        assert!(w.session.selected_unit().is_some(), "the starting unit is selected");
        for tile in [w.met_city, w.unmet_city, w.hidden_city, w.rival_city] {
            w.session.click(Some(tile), MapButton::Primary);
            assert_eq!(w.session.open_city_state(), None, "{tile:?}: the tile click keeps its old meaning");
        }
        let (game, rules) = w.session.harness_game();
        game.debug_force_war(&rules, PlayerId(0), w.met);
        w.session.refresh_after_harness();
        for _ in 0..2 {
            w.session.click(Some(w.met_city), MapButton::Primary);
            assert_eq!(w.session.open_city_state(), None, "at war it is an order, not a card");
            w.session.click(None, MapButton::Primary);
        }
        assert!(w.session.hud_model(None).city_state.is_none());
    }

    #[test]
    fn open_on_map_centres_the_city_and_opens_the_window_over_the_map() {
        let w = world();
        let (met, met_city, hidden) = (w.met, w.met_city, w.hidden);
        let mut ui = ClassicUi::game();
        ui.session = Some(w.session);
        let cards = ui.session.as_mut().unwrap().city_states_view().cards;
        let card = |player: PlayerId| cards.iter().find(|c| c.player == player).unwrap().on_map.clone();
        assert!(card(met).is_ready());
        assert!(!card(hidden).is_ready(), "its city was never seen: the button is grey with a reason");
        ui.screens.open(Screen::CityStates);
        let _ = ui.take_actions();
        ui.request(GameRequest::ShowCityState(hidden));
        assert_eq!(ui.session.as_ref().unwrap().open_city_state(), None);
        assert!(ui.take_actions().is_empty(), "nothing moves for a city that is not on the map");
        ui.request(GameRequest::ShowCityState(met));
        assert_eq!(ui.session.as_ref().unwrap().open_city_state(), Some(met));
        let actions = ui.take_actions();
        assert!(actions.contains(&client::shell::module::ShellAction::CenterOn(met_city.0)), "{actions:?}");
        assert!(actions.contains(&client::shell::module::ShellAction::Open(Screen::Map.into())), "the screen closes: {actions:?}");
        assert_eq!(ui.session.as_mut().unwrap().hud_model(None).city_state.unwrap().card.player, met);
    }

    #[test]
    fn a_banner_click_routes_by_whose_city_it_is() {
        let w = world();
        let (met_city, unmet_city, own_city, rival_city) = (w.met_city, w.unmet_city, w.own_city, w.rival_city);
        let mut ui = ClassicUi::game();
        ui.session = Some(w.session);
        ui.select_city(rival_city.0);
        ui.select_city(unmet_city.0);
        assert!(ui.session.as_mut().unwrap().hud_model(None).city_state.is_none());
        assert_ne!(ui.screens.screen, Screen::City);
        ui.select_city(met_city.0);
        assert_eq!(ui.session.as_ref().unwrap().open_city_state(), Some(w.met));
        assert_ne!(ui.screens.screen, Screen::City, "no city screen for a city-state");
        ui.session.as_mut().unwrap().close_city_state();
        ui.select_city(own_city.0);
        assert_eq!(ui.session.as_ref().unwrap().selected_city_tile(), Some(own_city.0));
        assert_eq!(ui.session.as_ref().unwrap().open_city_state(), None);
    }

    #[test]
    fn the_banner_of_a_met_city_state_says_how_to_talk() {
        let mut w = world();
        // A scout beside the met city-state puts its city in sight.
        let (game, rules) = w.session.harness_game();
        let map = game.view().auth.map();
        let beside = map.dimensions.neighbors(w.met_city).into_iter().find(|t| !map.is_water(*t)).expect("land beside the city");
        game.debug_spawn_unit(&rules, PlayerId(0), beside, classic_rules::ids::UnitKindId(0));
        w.session.refresh_after_harness();
        let banners = w.session.hud_extras(None).banners;
        let tip_of = |tile: TileId| banners.iter().find(|b| b.tile == tile.0).map(|b| b.tip.clone());
        let tip = tip_of(w.met_city).flatten().expect("a tip for a met city-state");
        assert!(tip.contains("Click to talk"), "{tip}");
        assert_eq!(tip_of(w.own_city), Some(None), "an own city keeps the shell's own tip");
        for language in ["ru", "de", "es", "fr", "ja", "zh-Hans"] {
            i18n::with_language(language, || {
                let tip = tf!("classic.game.overview.city-state-tip", name = "X", strength = 7);
                assert!(!tip.contains("Click to talk") && !tip.starts_with("classic."), "{language}: {tip}");
            });
        }
    }
}
