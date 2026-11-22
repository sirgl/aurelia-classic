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
use classic_rules::ClassicGame;
use classic_rules::derived::standing::{victory_warnings, VictoryWarning};
use classic_rules::transaction::journal::{AppliedCommand, PlayerChange};
use classic_rules::ids::PlayerId;
use classic_rules::world::city::{CityCore, CityStatus};
use classic_rules::world::relations::OfferLapse;
use i18n::{t, tf};

/// A foreign unit this close to a city of the player is at its border.
const BORDER_DISTANCE: i32 = 2;
/// The rounds a nation's sighting at the border stays told.
const BORDER_QUIET_ROUNDS: u32 = 5;

use super::session::Session;
use super::turn::CitySnapshot;
use crate::ui::model::NoticeKind;

impl Session {
    /// A notice for every event of `records` that concerns the player: a war declared on it, an
    /// offer of its own that lapsed, a theft, a dropped proposal, a first contact, a quest, a
    /// wonder, a nation destroyed. A war of a stranger (a city-state joins the war of its ally) is
    /// not told: a notice names only whom the player knows.
    pub(super) fn notify_news(&mut self, records: &[AppliedCommand]) {
        let me = self.me();
        if self.game().view().auth.settings().is_ai(me) {
            return;
        }
        for record in records {
            for change in &record.changes.player_changes {
                self.tell_player_change(change);
            }
            for change in &record.changes.module_changes {
                self.tell_change(change);
            }
        }
    }

    /// The notice of one kernel event. Every kind of `PlayerChange` is named: told, or not a
    /// matter for a notice (no `_` arm, so a new kind is decided here).
    fn tell_player_change(&mut self, change: &PlayerChange) {
        let me = self.me();
        match *change {
            PlayerChange::WarDeclared { player, enemy } => {
                if enemy == me && player != me && self.is_met(player) {
                    let text = tf!("classic.game.news.war-declared", name = self.player_name(player));
                    self.notify(NoticeKind::Diplomacy, text);
                }
            }
            PlayerChange::Eliminated { player } => {
                if player != me && self.is_met(player) {
                    let text = tf!("classic.game.news.nation-destroyed", name = self.player_name(player));
                    self.notify(NoticeKind::Diplomacy, text);
                }
            }
            // Silent: bookkeeping of the world, or told by another notice (peace by the answer
            // to the offer, a tech chosen by the research report).
            PlayerChange::TechnologyChosen { .. }
            | PlayerChange::TurnEnded { .. }
            | PlayerChange::PeaceMade { .. }
            | PlayerChange::BordersOpened { .. }
            | PlayerChange::EffectsChanged { .. }
            | PlayerChange::MapRevealed { .. }
            | PlayerChange::AreaRevealed { .. }
            | PlayerChange::TileNoticed { .. } => {}
        }
    }

    /// Has the player met `other`?
    fn is_met(&self, other: PlayerId) -> bool {
        classic_rules::derived::tourism::met(self.game().view().auth, self.me(), other)
    }

    /// The notice of one event of the classic rules, to the player it concerns
    /// (`ClassicChange::is_told_to`). Every kind is named: told here, told by another notice
    /// (`Elsewhere`), or silent (`Silent`: the screens show it). There is no `_` arm, so a new
    /// kind does not compile until it is decided.
    fn tell_change(&mut self, change: &ClassicChange) {
        let me = self.me();
        if !change.is_told_to(me) {
            return;
        }
        match *change {
            ClassicChange::OfferLapsed { from, to, peace, .. } => {
                if from == me {
                    let name = self.player_name(to);
                    let text = if peace {
                        tf!("classic.game.news.peace-lapsed", name = name)
                    } else {
                        tf!("classic.game.news.offer-lapsed", name = name)
                    };
                    self.notify(NoticeKind::Diplomacy, text);
                }
                // The receiver's notice, with the reason: `diplomacy.rs`.
            }
            ClassicChange::TheftDetected { thief, victim, city, identified } => {
                let text = self.theft_notice(thief, victim, city, identified);
                self.notify(NoticeKind::Diplomacy, text);
            }
            ClassicChange::SpyKilled { thief, victim, city } => {
                let text = if victim == me {
                    tf!("classic.game.news.spy-caught-ours", name = self.player_name(thief), city = self.city_name(city))
                } else {
                    tf!("classic.game.news.spy-lost", name = self.player_name(victim), city = self.city_name(city))
                };
                self.notify(NoticeKind::Diplomacy, text);
            }
            ClassicChange::ProposalDropped { kind, .. } => {
                let text = tf!("classic.game.news.proposal-dropped", name = super::congress::resolution_name(kind));
                self.notify(NoticeKind::Diplomacy, text);
            }
            ClassicChange::CityStateMet { city_state, gold, .. } => {
                let name = self.player_name(city_state);
                let text = if gold > 0 {
                    tf!("classic.game.news.city-state-met-gold", name = name, gold = gold)
                } else {
                    tf!("classic.game.news.city-state-met", name = name)
                };
                self.notify(NoticeKind::Diplomacy, text);
            }
            ClassicChange::QuestGiven { city_state, kind, .. } => {
                let text = tf!("classic.game.news.quest-given", name = self.player_name(city_state), task = self.quest_text(kind));
                self.notify(NoticeKind::Diplomacy, text);
            }
            ClassicChange::QuestDone { city_state, kind, .. } => {
                let text = tf!("classic.game.news.quest-done", name = self.player_name(city_state), task = self.quest_text(kind));
                self.notify(NoticeKind::Diplomacy, text);
            }
            ClassicChange::WonderBuilt { player, city, building } => self.tell_wonder(player, city, building),
            // Told by another notice.
            ClassicChange::OfferAnswered { .. } | ClassicChange::DealBroken { .. } => {} // diplomacy.rs
            ClassicChange::RuinsExplored { .. } => {}                                      // notify_ruins
            ClassicChange::CampCleared { .. } | ClassicChange::CampUnitConverted { .. } => {} // notify_camp
            ClassicChange::RebelsRose { .. } => {}                                         // notify_rebels
            ClassicChange::Plundered { .. } | ClassicChange::KillYield { .. } | ClassicChange::WonderYield { .. } => {} // notify_income
            ClassicChange::GoldenAgeChanged { .. } | ClassicChange::HappinessChanged { .. } => {} // report_society
            // Silent: a recomputation signal or a number the screens show.
            ClassicChange::ReligionChanged { .. }
            | ClassicChange::GreatWorksChanged { .. }
            | ClassicChange::TradeChanged { .. }
            | ClassicChange::GeneralBorn { .. }
            | ClassicChange::NaturalWonderFound { .. }
            | ClassicChange::PolicyAdopted { .. }
            | ClassicChange::RelationsChanged { .. }
            | ClassicChange::CityDamaged { .. }
            | ClassicChange::BuildingSold { .. } => {}
        }
    }

    /// The text of a found-out theft for its victim or its thief.
    fn theft_notice(&self, thief: PlayerId, victim: PlayerId, city: classic_rules::ids::CityHandle, identified: bool) -> String {
        let city = self.city_name(city);
        let victim_side = victim == self.me();
        match (victim_side, identified) {
            (true, true) => tf!("classic.game.news.theft-identified", name = self.player_name(thief), city = city),
            (true, false) => tf!("classic.game.news.theft-unknown", city = city),
            (false, true) => tf!("classic.game.news.theft-identified-thief", name = self.player_name(victim), city = city),
            (false, false) => tf!("classic.game.news.theft-noticed", city = city),
        }
    }

    /// A rival's world wonder is told by its name when the rival is met; the player's own wonder
    /// is told by its city's notices.
    fn tell_wonder(&mut self, builder: PlayerId, city: classic_rules::ids::CityHandle, building: classic_rules::ids::BuildingKindId) {
        if builder == self.me() {
            return;
        }
        let wonder = super::names::building(self.rules(), building);
        let text = if self.is_met(builder) {
            tf!("classic.game.news.wonder-built", name = self.player_name(builder), wonder = wonder, city = self.city_name(city))
        } else {
            tf!("classic.game.news.wonder-built-unmet", wonder = wonder)
        };
        self.notify(NoticeKind::Diplomacy, text);
    }

    /// A notice for each nation met since `before`.
    pub(super) fn report_contacts(&mut self, before: &[PlayerId]) {
        let met: Vec<PlayerId> = self.met_nations().iter().map(|r| r.other).collect();
        for other in met.into_iter().filter(|p| !before.contains(p)) {
            let text = tf!("classic.game.news.nation-met", name = self.player_name(other));
            self.notify(NoticeKind::Diplomacy, text);
        }
    }

    /// A notice for a nation (barbarians and city-states included) whose units stand within
    /// `BORDER_DISTANCE` of a city of the player, at most once in `BORDER_QUIET_ROUNDS` rounds per
    /// nation: a derived event of the round (nothing in the rules records a sighting).
    pub(super) fn report_border(&mut self) {
        let me = self.me();
        let round = self.game().view().auth.turn().round;
        let observation = self.game().ai_observation(self.rules(), me);
        let dims = self.game().view().auth.map().dimensions;
        let mut near: Vec<(PlayerId, classic_rules::ids::CityHandle)> = Vec::new();
        for unit in &observation.visible_foreign_units {
            let city = observation
                .own_cities
                .iter()
                .find(|c| dims.distance(c.tile, unit.tile).is_some_and(|d| d <= BORDER_DISTANCE));
            if let Some(city) = city {
                if !near.iter().any(|(owner, _)| *owner == unit.owner) {
                    near.push((unit.owner, city.handle));
                }
            }
        }
        for (owner, city) in near {
            let quiet = self.border_told.iter().any(|(p, r)| *p == owner && round < r + BORDER_QUIET_ROUNDS);
            if quiet {
                continue;
            }
            self.border_told.retain(|(p, _)| *p != owner);
            self.border_told.push((owner, round));
            let text = tf!("classic.game.news.border-sighting", name = self.player_name(owner), city = self.city_name(city));
            self.notify(NoticeKind::Diplomacy, text);
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
    /// lost a citizen to hunger (a fight that took a citizen also took the city's hit points, and a
    /// city the player razes loses a citizen every turn: that is the player's order, not hunger).
    pub(super) fn starvation_notes(&self, city: &CityCore, old: &CitySnapshot) -> Vec<(NoticeKind, String, Option<u32>)> {
        let at = Some(old.tile);
        let mut notes = Vec::new();
        if city.population < old.population && city.damage <= old.damage && city.status != CityStatus::Razing {
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
        record(to, |j| j.module_changes.push(ClassicChange::OfferLapsed { from, to, peace, reason: OfferLapse::Expired }))
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
        session.harness_game().0.debug_meet(me, a);
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
            game.debug_meet(me, rival);
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

    /// A rival the player has never met is not named in the notice of its ship, in any language.
    #[test]
    fn a_rival_the_player_has_not_met_is_not_named_in_its_ship_notice() {
        let mut session = new_match();
        let (me, rival) = (session.me(), PlayerId(1));
        assert!(!classic_rules::derived::tourism::met(session.game().view().auth, me, rival), "the rival is a stranger");
        let rules = game_rules();
        let project = |key: &str| classic_rules::rules::content::ProjectId(rules.module.projects.iter().position(|p| p.key == key).unwrap() as u16);
        let apollo = project("core:project/apollo_program");
        let boosters = classic_rules::world::victory::spaceship_parts(&rules)[0].0;
        let before = session.victory_steps();
        {
            let (game, _) = session.harness_game();
            game.debug_add_project(rival, apollo);
            for _ in 0..3 {
                game.debug_add_project(rival, boosters);
            }
        }
        for lang in LANGUAGES {
            let told = told_in(lang, &mut session, |s| s.report_victory_steps(&before));
            assert!(told.is_empty(), "{lang}: told about a nation never met: {told:?}");
        }
    }

    /// A war declared on the player by a nation it has never met (an allied city-state joins the
    /// war of its ally) is not told, so the notice does not name a stranger.
    #[test]
    fn a_war_declared_by_a_nation_the_player_has_not_met_is_not_told() {
        let mut session = new_match();
        let (me, stranger) = (session.me(), PlayerId(1));
        assert!(!classic_rules::derived::tourism::met(session.game().view().auth, me, stranger), "the declarer is a stranger");
        for lang in LANGUAGES {
            let told = told_in(lang, &mut session, |s| s.notify_news(&[war(stranger, me)]));
            assert!(told.is_empty(), "{lang}: a stranger is named: {told:?}");
        }
    }

    /// A land city of `player` four tiles from its first unit.
    fn found_city_near(session: &mut Session, player: PlayerId) -> CityHandle {
        let (game, rules) = session.harness_game();
        let unit = game.view().auth.indexes().units_of(player)[0];
        let at = game.view().auth.units().get(unit).unwrap().core.tile;
        let dims = game.view().auth.map().dimensions;
        let near = classic_rules::math::tiles_in_hex_radius(&dims, at, 3);
        let far = classic_rules::math::tiles_in_hex_radius(&dims, at, 4)
            .into_iter()
            .find(|t| !near.contains(t) && classic_rules::navigation::cost::enter_cost_on_map(game.view().auth.map(), *t).is_some() && game.view().auth.indexes().units_on(*t).is_empty())
            .expect("land four tiles off");
        game.debug_found_city(&rules, player, far)
    }

    /// A city the player razes loses a citizen every turn: that is not hunger and is not told so.
    #[test]
    fn a_city_the_player_razes_is_not_told_as_starved() {
        let mut session = new_match();
        let (me, rival) = (session.me(), PlayerId(1));
        found_city(&mut session, rival);
        let theirs = found_city_near(&mut session, rival);
        session.refresh_after_harness();
        set_population(&mut session, theirs, 10);
        take_city(&mut session, theirs, me);
        {
            let (game, rules) = session.harness_game();
            game.execute(&rules, Command::DecideCity { player: me, city: theirs, decision: classic_rules::command::CityDecision::Raze }).unwrap();
        }
        session.refresh_after_harness();
        let population = |s: &Session| s.game().view().auth.cities().get(theirs).map(|c| c.population);
        let before_round = population(&session);
        let before = session.snapshot();
        {
            let (game, rules) = session.harness_game();
            let round = game.view().auth.turn().round;
            while game.view().auth.turn().round == round {
                let active = game.view().auth.active_player();
                game.execute(&rules, Command::EndTurn { player: active }).unwrap();
            }
        }
        session.refresh_after_harness();
        assert!(population(&session) < before_round, "the razed city shrinks, else the scene proves nothing");
        for lang in LANGUAGES {
            let told = told_in(lang, &mut session, |s| s.report_changes(&before));
            let name = name_of(&session, theirs);
            assert!(told.iter().all(|t| !t.contains(&name)), "{lang}: a razed city is told as hunger: {told:?}");
        }
    }

    fn changes(player: PlayerId, change: ClassicChange) -> AppliedCommand {
        record(player, |j| j.module_changes.push(change))
    }

    /// Each new notice in every language: the text names what happened, has no missing key, and
    /// reaches only the nation it concerns.
    #[test]
    fn the_events_of_the_news_stream_are_told_in_every_language_to_those_they_concern() {
        use classic_rules::world::city_states::QuestKind;
        use classic_rules::world::congress::ResolutionKind;
        let mut session = new_match();
        let (me, a, b) = (session.me(), PlayerId(1), PlayerId(2));
        let mine = found_city(&mut session, me);
        let theirs = found_city(&mut session, a);
        session.refresh_after_harness();
        session.harness_game().0.debug_meet(me, a);
        let wonder = classic_rules::ids::BuildingKindId(0);
        let cases: Vec<(&str, Vec<AppliedCommand>, usize)> = vec![
            ("a theft identified, told to its victim", vec![changes(a, ClassicChange::TheftDetected { thief: a, victim: me, city: mine, identified: true })], 1),
            ("a theft detected, told to its victim", vec![changes(a, ClassicChange::TheftDetected { thief: a, victim: me, city: mine, identified: false })], 1),
            ("a theft of the player, told to the thief", vec![changes(me, ClassicChange::TheftDetected { thief: me, victim: a, city: theirs, identified: true })], 1),
            ("a spy killed in a city of the player", vec![changes(a, ClassicChange::SpyKilled { thief: a, victim: me, city: mine })], 1),
            ("a spy of the player killed", vec![changes(me, ClassicChange::SpyKilled { thief: me, victim: a, city: theirs })], 1),
            ("a theft of others is not told", vec![changes(a, ClassicChange::TheftDetected { thief: a, victim: b, city: theirs, identified: true })], 0),
            ("a proposal dropped", vec![changes(me, ClassicChange::ProposalDropped { proposer: me, kind: ResolutionKind::StandingArmyTax })], 1),
            ("a proposal of another dropped is not told", vec![changes(a, ClassicChange::ProposalDropped { proposer: a, kind: ResolutionKind::StandingArmyTax })], 0),
            ("a city-state met with a gift", vec![changes(me, ClassicChange::CityStateMet { city_state: b, player: me, gold: 30 })], 1),
            ("a city-state met without a gift", vec![changes(me, ClassicChange::CityStateMet { city_state: b, player: me, gold: 0 })], 1),
            ("a quest given", vec![changes(me, ClassicChange::QuestGiven { city_state: b, player: me, kind: QuestKind::Invest })], 1),
            ("a quest done", vec![changes(me, ClassicChange::QuestDone { city_state: b, player: me, kind: QuestKind::Invest })], 1),
            ("a quest of another nation is not told", vec![changes(a, ClassicChange::QuestDone { city_state: b, player: a, kind: QuestKind::Invest })], 0),
            ("a wonder of a met rival", vec![changes(a, ClassicChange::WonderBuilt { player: a, city: theirs, building: wonder })], 1),
            ("a wonder of an unmet rival", vec![changes(b, ClassicChange::WonderBuilt { player: b, city: theirs, building: wonder })], 1),
            ("an own wonder is told by its city", vec![changes(me, ClassicChange::WonderBuilt { player: me, city: mine, building: wonder })], 0),
            ("a nation destroyed that was met", vec![record(a, |j| j.player_changes.push(PlayerChange::Eliminated { player: a }))], 1),
            ("a nation destroyed that was not met", vec![record(b, |j| j.player_changes.push(PlayerChange::Eliminated { player: b }))], 0),
        ];
        for (what, records, count) in cases {
            for lang in LANGUAGES {
                let told = told_in(lang, &mut session, |s| s.notify_news(&records));
                assert_eq!(told.len(), count, "{what} in {lang}: {told:?}");
                assert!(told.iter().all(|t| !t.contains("classic.") && !t.is_empty()), "{what} in {lang}: {told:?}");
            }
        }
        let theft = vec![changes(a, ClassicChange::TheftDetected { thief: a, victim: me, city: mine, identified: true })];
        let told = told_in("en", &mut session, |s| s.notify_news(&theft));
        assert_eq!(told, vec![format!("{} stole a technology in {}", session.player_name(a), name_of(&session, mine))]);
    }

    /// A nation met since the round began is told once.
    #[test]
    fn a_new_contact_is_told_once() {
        let mut session = new_match();
        let (me, a) = (session.me(), PlayerId(1));
        let before = session.snapshot();
        session.harness_game().0.debug_meet(me, a);
        session.notices.clear();
        session.report_contacts(&before.contacts);
        assert_eq!(texts(&mut session), vec![format!("You met a nation: {}", session.player_name(a))]);
        session.notices.clear();
        session.report_contacts(&session.snapshot().contacts);
        assert!(texts(&mut session).is_empty(), "told once");
    }

    /// A unit next to a city of the player is told as at the border, once in five rounds per nation.
    #[test]
    fn a_unit_at_the_border_is_told_once_in_five_rounds() {
        let mut session = new_match();
        let (me, a) = (session.me(), PlayerId(1));
        let mine = found_city(&mut session, me);
        let at = session.game().view().auth.cities().get(mine).unwrap().tile;
        let next_to = classic_rules::ids::TileId(at.0 + 1);
        {
            let (game, rules) = session.harness_game();
            let warrior = classic_rules::ids::UnitKindId(rules.units.iter().position(|u| u.key == "core:unit/warrior").unwrap() as u16);
            game.debug_spawn_unit(&rules, a, next_to, warrior);
        }
        session.refresh_after_harness();
        session.notices.clear();
        session.report_border();
        let told = texts(&mut session);
        assert_eq!(told.len(), 1, "{told:?}");
        assert!(told[0].contains(&session.player_name(a)) && told[0].contains(&name_of(&session, mine)), "{told:?}");
        session.report_border();
        assert_eq!(texts(&mut session).len(), 1, "the second sighting within five rounds is quiet");
    }

    /// The audience of an event: both nations of a theft, every nation for a wonder.
    #[test]
    fn the_audience_of_a_theft_and_of_a_wonder() {
        let me = PlayerId(0);
        let city = CityHandle { index: 0, generation: 0 };
        let all = [
            ClassicChange::TheftDetected { thief: me, victim: PlayerId(1), city, identified: false },
            ClassicChange::WonderBuilt { player: me, city, building: classic_rules::ids::BuildingKindId(0) },
        ];
        assert!(all[0].is_told_to(me) && all[0].is_told_to(PlayerId(1)) && !all[0].is_told_to(PlayerId(2)));
        assert!(all[1].is_told_to(PlayerId(2)), "a wonder is announced to every nation");
    }
}
