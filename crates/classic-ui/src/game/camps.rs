//! Barbarian camps in the game: the notice of a camp the player's unit cleared, with the gold it gave
//! and, when a barbarian of the camp joined (Furor Teutonicus), what that brought.
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

impl Session {
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
}
