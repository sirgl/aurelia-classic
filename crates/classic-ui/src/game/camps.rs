//! Barbarian camps in the game: the notice of a camp the player's unit cleared, with the gold it gave
//! and, when a barbarian of the camp joined (Furor Teutonicus), what that brought; the notice of
//! rebels that very high unhappiness raised in the player's land (`ClassicChange::RebelsRose`).
use classic_rules::classic::ClassicChange;
use classic_rules::ids::TileId;
use classic_rules::transaction::journal::AppliedCommand;
use i18n::tf;

use super::session::Session;
use crate::ui::model::NoticeKind;

/// The notice text: the camp's gold, `joined` the gold of a barbarian that joined with the kill.
fn camp_text(gold: u32, joined: Option<u32>) -> String {
    match joined {
        Some(joined) => tf!("classic.game.camp.cleared-joined", gold = gold, joined = joined),
        None => tf!("classic.game.camp.cleared", gold = gold),
    }
}

/// The notice text of rebels: the city they rose near, how many and of which unit.
fn rebels_text(city: &str, count: u8, unit: &str) -> String {
    tf!("classic.game.rebels.rose", city = city, count = count, unit = unit)
}

impl Session {
    /// A notice, centred on the rising tile, for every rising of rebels against the human player
    /// in `records` (`ClassicChange::RebelsRose`: its happiness is below the rebellion threshold).
    pub(super) fn notify_rebels(&mut self, records: &[AppliedCommand]) {
        let me = self.me();
        if self.game().view().auth.settings().is_ai(me) {
            return;
        }
        for record in records {
            for change in &record.changes.module_changes {
                if let ClassicChange::RebelsRose { player, city, tile, kind, count } = *change {
                    if player != me {
                        continue;
                    }
                    let unit = super::names::unit(self.rules(), kind);
                    let text = rebels_text(&self.city_name(city), count, &unit);
                    self.notify_at(NoticeKind::Combat, text, Some(tile.0));
                }
            }
        }
    }

    /// A notice for every barbarian camp the human player's units cleared in `records`, centred on
    /// the camp. A barbarian that joined where the camp was cleared is told in the same line; one
    /// that joined while the camp stays (more defenders are left) gets a line of its own.
    pub(super) fn notify_camp(&mut self, records: &[AppliedCommand]) {
        let me = self.me();
        if self.game().view().auth.settings().is_ai(me) {
            return;
        }
        for record in records {
            let changes = &record.changes.module_changes;
            let mut told: Vec<TileId> = Vec::new();
            for change in changes {
                if let ClassicChange::CampCleared { player, tile, gold } = *change {
                    if player != me {
                        continue;
                    }
                    let joined = changes.iter().find_map(|c| match *c {
                        ClassicChange::CampUnitConverted { player, tile: at, gold } if player == me && at == tile => Some(gold),
                        _ => None,
                    });
                    told.push(tile);
                    self.notify_at(NoticeKind::Combat, camp_text(gold, joined), Some(tile.0));
                }
            }
            for change in changes {
                if let ClassicChange::CampUnitConverted { player, tile, gold } = *change {
                    if player == me && !told.contains(&tile) {
                        self.notify_at(NoticeKind::Combat, tf!("classic.game.camp.joined", gold = gold), Some(tile.0));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use classic_rules::ids::TileId;
    use classic_rules::world::barbarians::CAMP_GOLD;
    use classic_rules::ClassicGame;

    use super::*;
    use crate::game::MapButton;
    use crate::ui::screens::{game_rules, prepare_match, MapSize, MatchRequest};

    /// A session whose selected unit stands next to an empty barbarian camp; the tile of the camp.
    fn next_to_camp() -> (Session, TileId) {
        let request = MatchRequest { nation: "core:nation/rome".into(), seed: 4, size: MapSize::Small, players: 3, water: 25 };
        let mut session = Session::new(prepare_match(request).expect("world").game, game_rules());
        let (_, unit) = session.selected_unit().expect("the starting unit is selected");
        let from = unit.tile;
        let (game, rules) = session.harness_game();
        let map = game.view().auth.map();
        let site = map
            .dimensions
            .neighbors(from)
            .into_iter()
            .find(|t| !map.is_water(*t) && map.improvement(*t).flatten().is_none())
            .expect("a free land tile next to the start");
        game.debug_place_camp(&rules, site);
        session.refresh_after_harness();
        (session, site)
    }

    fn treasury(session: &Session) -> u32 {
        session.game().view().auth.players().treasury_of(session.me())
    }

    #[test]
    fn walking_into_a_camp_leaves_a_notice_with_the_gold_and_the_place() {
        let (mut session, site) = next_to_camp();
        assert!(session.hud_model(None).notices.is_empty(), "no notice before the step");
        let before = treasury(&session);
        session.click(Some(site), MapButton::Primary);
        assert_eq!(treasury(&session) - before, CAMP_GOLD, "the camp paid");
        let notice = session.hud_model(None).notices.into_iter().next().expect("a notice");
        assert_eq!(notice.text, format!("Barbarian camp cleared: +{CAMP_GOLD} gold"));
        assert_eq!(notice.tile, Some(site.0));
        assert_eq!(notice.kind, NoticeKind::Combat);
        assert_eq!(session.hud_model(None).notices.len(), 1, "one notice for one camp");
    }

    #[test]
    fn a_camp_is_told_about_once() {
        let (mut session, site) = next_to_camp();
        session.click(Some(site), MapButton::Primary);
        session.click(Some(site), MapButton::Primary);
        assert_eq!(session.hud_model(None).notices.len(), 1, "stepping on the tile again clears nothing");
    }

    #[test]
    fn the_texts_name_the_gold_and_the_unit_that_joined_in_both_languages() {
        assert_eq!(camp_text(75, None), "Barbarian camp cleared: +75 gold");
        assert_eq!(camp_text(25, Some(25)), "Barbarian camp cleared: +25 gold; a barbarian joined you (+25 gold)");
        assert_eq!(tf!("classic.game.camp.joined", gold = 25), "A barbarian from the camp joined you: +25 gold");
        i18n::with_language("ru", || {
            assert_eq!(camp_text(75, None), "\u{41b}\u{430}\u{433}\u{435}\u{440}\u{44c} \u{432}\u{430}\u{440}\u{432}\u{430}\u{440}\u{43e}\u{432} \u{440}\u{430}\u{437}\u{433}\u{440}\u{43e}\u{43c}\u{43b}\u{435}\u{43d}: +75 \u{437}\u{43e}\u{43b}\u{43e}\u{442}\u{430}"); // Russian: "Barbarian camp cleared: +75 gold"
            crate::game::reasons::only_cyrillic(&camp_text(25, Some(25)).replace(char::is_numeric, "").replace('+', ""));
            crate::game::reasons::only_cyrillic(&tf!("classic.game.camp.joined", gold = 25).replace(char::is_numeric, "").replace('+', ""));
        });
    }

    /// A rising against the player is told, centred on the rising tile, with the city, the count
    /// and the unit, in every language of the game; a rising against another nation is not.
    #[test]
    fn rebels_are_told_in_every_language() {
        use classic_rules::ids::UnitKindId;
        use classic_rules::transaction::journal::ChangeJournal;
        let (mut session, site) = next_to_camp();
        let me = session.me();
        let (_, unit) = session.selected_unit().expect("the starting unit is selected");
        let at = unit.tile;
        let (game, rules) = session.harness_game();
        let city = game.debug_found_city(&rules, me, at);
        session.refresh_after_harness();
        let name = session.city_name(city);
        assert!(!name.is_empty());
        let rose = |player| {
            let mut journal = ChangeJournal::default();
            journal.module_changes.push(ClassicChange::RebelsRose { player, city, tile: site, kind: UnitKindId(0), count: 2 });
            AppliedCommand { command: classic_rules::Command::EndTurn { player }, changes: journal }
        };
        let records = vec![rose(me), rose(classic_rules::ids::PlayerId(me.0 + 1))];
        for lang in ["en", "ru", "de", "fr", "es", "ja", "zh-Hans"] {
            let told = i18n::with_language(lang, || {
                session.notices.clear();
                session.notify_rebels(&records);
                session.hud_model(None).notices
            });
            assert_eq!(told.len(), 1, "{lang}: ours only");
            assert!(told[0].text.contains('2') && told[0].text.contains(&name) && !told[0].text.contains("classic."), "{lang}: {}", told[0].text);
            assert_eq!(told[0].tile, Some(site.0), "{lang}");
        }
        assert_eq!(rebels_text("Roma", 2, "Warrior"), "Very high unhappiness: rebels rose near Roma (2 × Warrior)");
    }

    /// The whole path without a window: the player's empire is held below -20 happiness, turns
    /// end, rebels of the barbarians rise in its land and the notice tells it at the rising tile;
    /// the top bar warned of the threshold all along.
    #[test]
    fn an_empire_below_minus_20_sees_its_rebels_rise() {
        use classic_rules::derived::happiness::{player_happiness, REBELLION_THRESHOLD};
        use classic_rules::world::barbarians::BarbarianMode;
        use classic_rules::world::player::ClassicPlayers;
        use crate::ui::model::GameRequest;
        let request = MatchRequest { nation: "core:nation/rome".into(), seed: 4, size: MapSize::Small, players: 3, water: 25 };
        // Barbarians on, as the setup screen starts every match.
        let options = crate::ui::screens::GameOptions { barbarians: true, ..Default::default() };
        let prepared = crate::ui::screens::prepare_game(crate::ui::screens::NewGame { request, options }).expect("world");
        let mut session = Session::new(prepared.game, game_rules());
        let me = session.me();
        let (_, unit) = session.selected_unit().expect("the starting unit is selected");
        let at = unit.tile;
        let (game, rules) = session.harness_game();
        assert_eq!(game.view().auth.module().barbarians().mode, BarbarianMode::Normal);
        let city = game.debug_found_city(&rules, me, at);
        let population = (1..80)
            .find(|p| {
                game.debug_set_population(&rules, city, *p);
                player_happiness(game.view().auth, &rules, me).excess() <= REBELLION_THRESHOLD
            })
            .expect("a population below -20");
        session.refresh_after_harness();
        let warning = session.economy();
        let tip = crate::ui::hud::game_widgets::economy_values(&warning).into_iter().find_map(|v| v.progress.filter(|p| p.contains("rebels")));
        assert_eq!(tip.as_deref(), Some("Below -20 happiness: rebels rise in your land every few turns"));
        let mut told = None;
        for _ in 0..12 {
            let (game, rules) = session.harness_game();
            game.debug_set_population(&rules, city, population);
            session.refresh_after_harness();
            session.notices.clear();
            session.request(GameRequest::EndTurn);
            told = session.hud_model(None).notices.into_iter().find(|n| n.text.starts_with("Very high unhappiness: rebels rose near"));
            if told.is_some() {
                break;
            }
        }
        let told = told.expect("rebels rose within the countdown");
        let tile = TileId(told.tile.expect("the notice points at the rising"));
        let auth = session.game().view().auth;
        assert_eq!(auth.map().owning_city(tile), Some(city), "{}", told.text);
        let barbarian = auth.players().barbarian().expect("barbarians");
        let near = auth.indexes().units_of(barbarian).iter().filter(|u| auth.map().dimensions.distance(auth.units().get(**u).unwrap().core.tile, tile).unwrap() <= 2).count();
        assert!(near >= 1, "the rebels stand there for the barbarians");
    }


    /// A match without barbarians has no rebels: however unhappy the empire, the top bar does
    /// not promise them.
    #[test]
    fn no_rebel_warning_in_a_match_without_barbarians() {
        use classic_rules::derived::happiness::{player_happiness, REBELLION_THRESHOLD};
        let request = MatchRequest { nation: "core:nation/rome".into(), seed: 4, size: MapSize::Small, players: 3, water: 25 };
        let options = crate::ui::screens::GameOptions { barbarians: false, ..Default::default() };
        let prepared = crate::ui::screens::prepare_game(crate::ui::screens::NewGame { request, options }).expect("world");
        let mut session = Session::new(prepared.game, game_rules());
        let me = session.me();
        let (_, unit) = session.selected_unit().expect("the starting unit is selected");
        let at = unit.tile;
        let (game, rules) = session.harness_game();
        let city = game.debug_found_city(&rules, me, at);
        (1..80)
            .find(|p| {
                game.debug_set_population(&rules, city, *p);
                player_happiness(game.view().auth, &rules, me).excess() <= REBELLION_THRESHOLD
            })
            .expect("a population below -20");
        session.refresh_after_harness();
        let economy = session.economy();
        assert!(economy.happiness <= REBELLION_THRESHOLD && !economy.rebels_possible);
        let tip = crate::ui::hud::game_widgets::economy_values(&economy).into_iter().find_map(|v| v.progress.filter(|p| p.contains("rebels")));
        assert_eq!(tip, None);
    }

}
