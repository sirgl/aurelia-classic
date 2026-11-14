//! News for the human player about what the other players did and what hurts its empire, told
//! after their turns: a war declared on it, an offer of its own that lapsed unanswered, a
//! city that starves, a rival one step from a victory (`derived::standing::victory_warnings`).
//!
//! Each notice is born from one event of the game, never from a poll of the screen: the war and
//! the lapse come from the kernel journal (`PlayerChange::WarDeclared`,
//! `ClassicChange::OfferLapsed`), the starving and the victory steps from the difference of two
//! states, the one before the round and the one after (a notice at the step that is new, so a
//! threshold is told once). Peace, answers to offers, broken deals, lost and captured cities and
//! the orders removed by a wonder built elsewhere are told by `diplomacy.rs`, `turn.rs` and
//! `lost_orders.rs`.
use classic_rules::classic::ClassicChange;
use classic_rules::derived::standing::{victory_warnings, VictoryWarning};
use classic_rules::transaction::journal::{AppliedCommand, PlayerChange};
use classic_rules::world::city::CityCore;
use i18n::{t, tf};

use super::session::Session;
use super::turn::CitySnapshot;
use crate::ui::model::NoticeKind;

impl Session {
    /// A notice for every war a nation declared on the player and every offer of the player that
    /// lapsed unanswered, in `records`.
    pub(super) fn notify_news(&mut self, records: &[AppliedCommand]) {
        let me = self.me();
        if self.game().view().auth.settings().is_ai(me) {
            return;
        }
        for record in records {
            for change in &record.changes.player_changes {
                let PlayerChange::WarDeclared { player, enemy } = *change else { continue };
                if enemy == me && player != me {
                    let text = tf!("classic.game.news.war-declared", name = self.player_name(player));
                    self.notify(NoticeKind::Diplomacy, text);
                }
            }
            for change in &record.changes.module_changes {
                let ClassicChange::OfferLapsed { from, to, peace } = *change else { continue };
                if from != me {
                    continue;
                }
                let name = self.player_name(to);
                let text = if peace {
                    tf!("classic.game.news.peace-lapsed", name = name)
                } else {
                    tf!("classic.game.news.offer-lapsed", name = name)
                };
                self.notify(NoticeKind::Diplomacy, text);
            }
        }
    }

    /// The victory steps of rivals that are true now.
    pub(super) fn victory_steps(&self) -> Vec<VictoryWarning> {
        victory_warnings(self.game().view().auth, self.rules(), self.me())
    }

    /// A notice for every victory step that was not true before the round.
    pub(super) fn report_victory_steps(&mut self, before: &[VictoryWarning]) {
        let new: Vec<VictoryWarning> = self.victory_steps().into_iter().filter(|step| !before.contains(step)).collect();
        self.tell_victory_steps(&new);
    }

    /// A notice for each of the victory `steps`.
    pub(super) fn tell_victory_steps(&mut self, steps: &[VictoryWarning]) {
        for step in steps {
            let text = match *step {
                VictoryWarning::ShipHalf(rival) => tf!("classic.game.news.ship-half", name = self.player_name(rival)),
                VictoryWarning::ShipLastPart(rival) => tf!("classic.game.news.ship-last-part", name = self.player_name(rival)),
                VictoryWarning::LastCapital(rival) => tf!("classic.game.news.last-capital", name = self.player_name(rival)),
                VictoryWarning::LeaderVote => t!("classic.game.news.leader-vote").to_string(),
            };
            self.notify(NoticeKind::Diplomacy, text);
        }
    }

    /// Does the city eat more than it grows now (its food surplus is below zero)?
    pub(super) fn is_starving(&self, core: &CityCore) -> bool {
        classic_rules::derived::income::city_income(self.game().view().auth, self.rules(), core).food_surplus < 0
    }

    /// The notices of a city of the player that starves: it began to starve in the round, or it
    /// lost a citizen to hunger (a fight that took a citizen also took the city's hit points).
    pub(super) fn starvation_notes(&self, city: &CityCore, old: &CitySnapshot) -> Vec<(NoticeKind, String, Option<u32>)> {
        let at = Some(old.tile);
        let mut notes = Vec::new();
        if city.population < old.population && city.damage <= old.damage {
            notes.push((NoticeKind::Growth, tf!("classic.game.news.starved", city = city.name.as_str(), n = city.population), at));
        }
        if !old.starving && self.is_starving(city) {
            notes.push((NoticeKind::Growth, tf!("classic.game.news.starving", city = city.name.as_str()), at));
        }
        notes
    }
}

#[cfg(test)]
mod tests {
    use classic_rules::ids::{CityHandle, PlayerId};
    use classic_rules::transaction::journal::ChangeJournal;
    use classic_rules::{ClassicGame, Command};

    use super::*;
    use crate::ui::screens::{game_rules, prepare_match, MapSize, MatchRequest};

    const LANGUAGES: [&str; 7] = ["en", "ru", "de", "fr", "es", "ja", "zh-Hans"];

    fn new_match() -> Session {
        let request = MatchRequest { nation: "core:nation/rome".into(), seed: 4, size: MapSize::Small, players: 3, water: 25 };
        Session::new(prepare_match(request).expect("world").game, game_rules())
    }

    fn texts(session: &mut Session) -> Vec<String> {
        session.hud_model(None).notices.into_iter().rev().map(|n| n.text).collect()
    }

    fn told_in(lang: &str, session: &mut Session, tell: impl FnOnce(&mut Session)) -> Vec<String> {
        i18n::with_language(lang, || {
            session.notices.clear();
            tell(session);
            texts(session)
        })
    }

    fn record(player: PlayerId, change: impl FnOnce(&mut ChangeJournal)) -> AppliedCommand {
        let mut changes = ChangeJournal::default();
        change(&mut changes);
        AppliedCommand { command: Command::EndTurn { player }, changes }
    }

    fn war(player: PlayerId, enemy: PlayerId) -> AppliedCommand {
        record(player, |j| j.player_changes.push(PlayerChange::WarDeclared { player, enemy }))
    }

    fn lapsed(from: PlayerId, to: PlayerId, peace: bool) -> AppliedCommand {
        record(to, |j| j.module_changes.push(ClassicChange::OfferLapsed { from, to, peace }))
    }

    fn found_city(session: &mut Session, player: PlayerId) -> CityHandle {
        let (game, rules) = session.harness_game();
        let unit = game.view().auth.indexes().units_of(player)[0];
        let tile = game.view().auth.units().get(unit).unwrap().core.tile;
        game.debug_found_city(&rules, player, tile)
    }

    fn set_population(session: &mut Session, city: CityHandle, population: u32) {
        let (game, rules) = session.harness_game();
        game.debug_set_population(&rules, city, population);
        session.refresh_after_harness();
    }

    fn name_of(session: &Session, city: CityHandle) -> String {
        session.game().view().auth.cities().get(city).unwrap().name.clone()
    }

    /// A war declared on the player is told to it, with the name of the declarer, in every
    /// language; a war it declared itself and a war between others are not.
    #[test]
    fn a_war_declared_on_the_player_is_told_to_it_and_not_a_war_of_others() {
        let mut session = new_match();
        let (me, a, b) = (session.me(), PlayerId(1), PlayerId(2));
        let records = vec![war(a, me), war(me, a), war(a, b)];
        for lang in LANGUAGES {
            let told = told_in(lang, &mut session, |s| s.notify_news(&records));
            assert_eq!(told.len(), 1, "{lang}: {told:?}");
            assert!(told[0].contains(&i18n::with_language(lang, || session.player_name(a))) && !told[0].contains("classic."), "{lang}: {told:?}");
        }
        let told = told_in("en", &mut session, |s| s.notify_news(&records));
        assert_eq!(told, vec![format!("{} declared war on you", session.player_name(a))]);
    }

    /// An offer of the player that lapsed is told to it; one of another nation is not.
    #[test]
    fn a_lapsed_offer_is_told_to_its_proposer_only() {
        let mut session = new_match();
        let (me, a, b) = (session.me(), PlayerId(1), PlayerId(2));
        let records = vec![lapsed(me, a, true), lapsed(me, b, false), lapsed(a, me, false), lapsed(a, b, true)];
        for lang in LANGUAGES {
            let told = told_in(lang, &mut session, |s| s.notify_news(&records));
            assert_eq!(told.len(), 2, "{lang}: the peace and the deal of the player: {told:?}");
            assert!(told[0] != told[1] && told.iter().all(|t| !t.contains("classic.")), "{lang}: {told:?}");
        }
        let told = told_in("en", &mut session, |s| s.notify_news(&records));
        assert_eq!(told[0], format!("{} did not answer your offer of peace: it has lapsed", session.player_name(a)));
        assert_eq!(told[1], format!("{} did not answer your offer: it has lapsed", session.player_name(b)));
    }

    /// A city of the player that begins to starve is told once, with the city's tile; a citizen
    /// lost to hunger is told; a city of a rival that starves is not.
    #[test]
    fn a_starving_city_is_told_once_and_a_citizen_lost_is_told() {
        let mut session = new_match();
        let me = session.me();
        let (mine, theirs) = (found_city(&mut session, me), found_city(&mut session, PlayerId(1)));
        session.refresh_after_harness();
        let before = session.snapshot();
        set_population(&mut session, mine, 12);
        set_population(&mut session, theirs, 12);
        let core = session.game().view().auth.cities().get(mine).unwrap().clone();
        assert!(session.is_starving(&core), "twelve citizens eat more than a new city grows");
        session.notices.clear();
        session.report_changes(&before);
        let (mine_name, their_name) = (name_of(&session, mine), name_of(&session, theirs));
        let starving = format!("{mine_name} is starving: it eats more than it grows");
        assert!(texts(&mut session).contains(&starving), "{:?}", texts(&mut session));
        assert!(texts(&mut session).iter().all(|t| !t.contains(&their_name)), "a rival's city is not told");
        let tile = session.hud_model(None).notices.iter().find(|n| n.text == starving).and_then(|n| n.tile);
        assert_eq!(tile, Some(core.tile.0), "the notice leads to the city");

        let now = session.snapshot();
        session.notices.clear();
        session.report_changes(&now);
        assert!(texts(&mut session).iter().all(|t| !t.contains("starving")), "told once: {:?}", texts(&mut session));

        set_population(&mut session, mine, 11);
        session.notices.clear();
        session.report_changes(&now);
        assert!(texts(&mut session).contains(&format!("{mine_name} starved and shrank to 11")), "{:?}", texts(&mut session));
        for lang in LANGUAGES {
            let told = told_in(lang, &mut session, |s| s.report_changes(&now));
            assert!(told.iter().all(|t| !t.contains("classic.")), "{lang}: {told:?}");
        }
    }

    /// The ship of a rival at its half is told once, with the rival's name; the ship of the
    /// player itself is not told to it.
    #[test]
    fn a_rival_with_half_the_ship_is_told_once_and_the_players_own_ship_is_not() {
        let mut session = new_match();
        let rival = PlayerId(1);
        let rules = game_rules();
        let project = |key: &str| classic_rules::rules::content::ProjectId(rules.module.projects.iter().position(|p| p.key == key).unwrap() as u16);
        let apollo = project("core:project/apollo_program");
        let boosters = classic_rules::world::victory::spaceship_parts(&rules)[0].0;
        let before = session.victory_steps();
        assert!(before.is_empty());
        let me = session.me();
        {
            let (game, _) = session.harness_game();
            for player in [me, rival] {
                game.debug_add_project(player, apollo);
                for _ in 0..3 {
                    game.debug_add_project(player, boosters);
                }
            }
        }
        for lang in LANGUAGES {
            let told = told_in(lang, &mut session, |s| s.report_victory_steps(&before));
            assert_eq!(told.len(), 1, "{lang}: the rival's ship only: {told:?}");
            assert!(told[0].contains(&i18n::with_language(lang, || session.player_name(rival))) && !told[0].contains("classic."), "{lang}: {told:?}");
        }
        let now = session.victory_steps();
        let told = told_in("en", &mut session, |s| s.report_victory_steps(&now));
        assert!(told.is_empty(), "a step is told once: {told:?}");
    }

    /// The other victory steps read in every language (their events are tested in the core).
    #[test]
    fn every_victory_step_has_a_text_in_every_language() {
        let mut session = new_match();
        let rival = PlayerId(1);
        for lang in LANGUAGES {
            let name = i18n::with_language(lang, || session.player_name(rival));
            let steps = [VictoryWarning::ShipHalf(rival), VictoryWarning::ShipLastPart(rival), VictoryWarning::LastCapital(rival), VictoryWarning::LeaderVote];
            let told = told_in(lang, &mut session, |s| s.tell_victory_steps(&steps));
            assert_eq!(told.len(), 4, "{lang}");
            assert!(told.iter().all(|t| !t.contains("classic.")), "{lang}: {told:?}");
            assert_eq!(told.iter().filter(|t| t.contains(&name)).count(), 3, "{lang}: {told:?}");
        }
    }

    /// `by` takes `city` by force: it declares war on the owner and storms the city with a
    /// melee unit beside it at one hit point.
    fn take_city(session: &mut Session, city: CityHandle, by: PlayerId) {
        use classic_rules::ids::UnitKindId;
        let (game, rules) = session.harness_game();
        game.debug_meet_all();
        let owner = game.view().auth.cities().get(city).unwrap().owner;
        while game.view().auth.active_player() != by {
            let active = game.view().auth.active_player();
            game.execute(&rules, Command::EndTurn { player: active }).unwrap();
        }
        game.execute(&rules, Command::DeclareWar { player: by, enemy: owner }).unwrap();
        let at = game.view().auth.cities().get(city).unwrap().tile;
        let dims = game.view().auth.map().dimensions;
        let free = |t: &classic_rules::ids::TileId| {
            classic_rules::navigation::cost::enter_cost_on_map(game.view().auth.map(), *t).is_some() && game.view().auth.indexes().units_on(*t).is_empty()
        };
        let beside = classic_rules::math::tiles_in_hex_radius(&dims, at, 1).into_iter().find(|t| *t != at && free(t)).expect("land beside the city");
        let fighter = rules
            .units
            .iter()
            .enumerate()
            .filter(|(_, u)| !u.unbuildable && u.cost > 0 && u.strength > 0 && u.ranged_strength == 0 && u.required_resources.is_empty() && u.nation.is_none())
            .min_by_key(|(_, u)| u.cost)
            .map(|(i, _)| UnitKindId(i as u16))
            .expect("a melee unit in the rules");
        let attacker = game.debug_spawn_unit(&rules, by, beside, fighter);
        let max = classic_rules::combat::city::max_hit_points(game.view().auth, &rules, game.view().auth.cities().get(city).unwrap());
        game.debug_set_city_damage(city, max - 1);
        game.execute(&rules, Command::AttackCity { attacker, city }).unwrap();
        assert_eq!(game.view().auth.cities().get(city).map(|c| c.owner), Some(by), "the city is taken");
    }

    /// A city of the player that a rival took in the round is told with its tile, in every
    /// language; a city the rival took from another nation is not.
    #[test]
    fn a_city_taken_from_the_player_is_told_with_its_tile() {
        let mut session = new_match();
        let me = session.me();
        let (rival, other) = (PlayerId(1), PlayerId(2));
        let (mine, others) = (found_city(&mut session, me), found_city(&mut session, other));
        session.refresh_after_harness();
        let before = session.snapshot();
        let (mine_name, their_name) = (name_of(&session, mine), name_of(&session, others));
        let tile = session.game().view().auth.cities().get(mine).unwrap().tile.0;
        take_city(&mut session, mine, rival);
        take_city(&mut session, others, rival);
        session.refresh_after_harness();
        session.notices.clear();
        session.report_changes(&before);
        let told = session.hud_model(None).notices;
        let captured = told.iter().find(|n| n.text == format!("{mine_name} was captured")).expect("the capture is told");
        assert_eq!(captured.tile, Some(tile), "the notice leads to the city");
        assert!(told.iter().all(|n| !n.text.contains(&their_name)), "the other nation's city is not told");
        for lang in LANGUAGES {
            let told = told_in(lang, &mut session, |s| s.report_changes(&before));
            assert!(told.iter().any(|t| t.contains(&mine_name)) && told.iter().all(|t| !t.contains("classic.") && !t.contains(&their_name)), "{lang}: {told:?}");
        }
    }
}
