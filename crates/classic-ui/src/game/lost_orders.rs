//! Orders the rules took out of a city's queue (`CityChange::OrderRemoved`), told to the human
//! player: which order, in which city, why, and that the accumulated production is kept.
//!
//! The kernel journal is the full truth: it names the removals of every player's cities. The
//! filter is here, at the edge that feeds the player's notices: only a removal whose `owner` is
//! the session's player becomes a notice. Nothing else reads the entry (a rival never learns what
//! a city was building).
use classic_rules::transaction::journal::{AppliedCommand, CityChange, OrderRemoval};
use i18n::tf;

use super::names;
use super::session::Session;
use crate::ui::model::NoticeKind;

impl Session {
    /// A notice, centred on the city, for every order removed from a city of the human player.
    pub(super) fn notify_lost_orders(&mut self, records: &[AppliedCommand]) {
        let me = self.me();
        if self.game().view().auth.settings().is_ai(me) {
            return;
        }
        for record in records {
            for change in &record.changes.city_changes {
                let CityChange::OrderRemoved { city, owner, item, reason } = *change else { continue };
                if owner != me {
                    continue;
                }
                let Some(core) = self.game().view().auth.cities().get(city) else { continue };
                let (name, tile) = (core.name.clone(), core.tile.0);
                let item = names::item(self.rules(), item);
                let text = match reason {
                    OrderRemoval::WonderBuilt => tf!("classic.game.turn.order-removed-wonder-built", city = name, item = item),
                    OrderRemoval::NationalWonderBuilt => tf!("classic.game.turn.order-removed-national-wonder-built", city = name, item = item),
                    OrderRemoval::AlreadyBuilt => tf!("classic.game.turn.order-removed-already-built", city = name, item = item),
                    OrderRemoval::NeedsBuilding => tf!("classic.game.turn.order-removed-needs-building", city = name, item = item),
                    OrderRemoval::Blocked => tf!("classic.game.turn.order-removed-blocked", city = name, item = item),
                };
                self.notify_at(NoticeKind::Production, text, Some(tile));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use classic_rules::ids::{BuildingKindId, PlayerId, ProductionItem};
    use classic_rules::transaction::journal::ChangeJournal;
    use classic_rules::ids::CityHandle;
    use classic_rules::{Command, GameState};

    use super::*;
    use crate::ui::screens::{game_rules, prepare_match, MapSize, MatchRequest};

    fn new_match() -> Session {
        let request = MatchRequest { nation: "core:nation/rome".into(), seed: 4, size: MapSize::Small, players: 3, water: 25 };
        Session::new(prepare_match(request).expect("world").game, game_rules())
    }

    fn texts(session: &mut Session) -> Vec<String> {
        session.hud_model(None).notices.into_iter().rev().map(|n| n.text).collect()
    }

    /// A world wonder with no placement or building condition, its technology learned by `players`.
    fn plain_wonder(game: &mut GameState, players: &[PlayerId]) -> BuildingKindId {
        let rules = game_rules();
        let (index, def) = rules
            .buildings
            .iter()
            .enumerate()
            .find(|(_, b)| {
                b.wonder
                    && !b.unbuildable
                    && b.required_tech.is_some()
                    && b.nation.is_none()
                    && b.replaces.is_none()
                    && b.requires_buildings.is_empty()
                    && b.requires_buildings_in_empire.is_empty()
                    && b.placement == Default::default()
                    && b.consumes_resources.is_empty()
            })
            .expect("a plain world wonder in the rules");
        for p in players {
            game.debug_learn(&rules, *p, def.required_tech.unwrap());
        }
        BuildingKindId(index as u16)
    }

    fn city_of(game: &mut GameState, player: PlayerId) -> CityHandle {
        let rules = game_rules();
        let auth = game.view().auth;
        let unit = auth.indexes().units_of(player)[0];
        let tile = auth.units().get(unit).unwrap().core.tile;
        game.debug_found_city(&rules, player, tile)
    }

    fn end_turns_until(game: &mut GameState, player: PlayerId) {
        let rules = game_rules();
        while game.view().auth.active_player() != player {
            let active = game.view().auth.active_player();
            game.execute(&rules, Command::EndTurn { player: active }).unwrap();
        }
    }

    /// `owner` queues a wonder that already stands in another player's city without the sweep (the
    /// harness grant, the state an old save holds); the record of the end of the owner's turn, where
    /// the kernel drops the stale order.
    fn stale_order(session: &mut Session, owner: PlayerId, holder: PlayerId, reload: bool) -> (Vec<AppliedCommand>, BuildingKindId, CityHandle) {
        let rules = game_rules();
        let (game, _) = session.harness_game();
        let wonder = plain_wonder(game, &[owner, holder]);
        let (mine, theirs) = (city_of(game, owner), city_of(game, holder));
        end_turns_until(game, owner);
        game.execute(&rules, Command::EnqueueProduction { player: owner, city: mine, item: ProductionItem::Building(wonder) }).expect("enqueue the wonder");
        game.debug_add_building(&rules, theirs, wonder);
        if reload {
            // Saved with the stale order in the queue, loaded again (a save made before the change).
            let bytes = classic_rules::persistence::encode::encode(game, &rules);
            *game = classic_rules::persistence::decode::decode(&bytes, &rules).expect("the save loads");
            assert_eq!(game.view().auth.cities().get(mine).unwrap().queue.len(), 1, "the stale order survived the save");
        }
        let before = game.view().auth.cities().get(mine).unwrap().production;
        let command = Command::EndTurn { player: owner };
        let result = game.execute(&rules, command.clone()).expect("end of turn");
        let record = AppliedCommand { command, changes: result.into_journal() };
        let core = game.view().auth.cities().get(mine).unwrap();
        assert!(core.queue.is_empty(), "the kernel took the stale order out");
        assert!(core.production >= before, "nothing was spent: {} -> {}", before, core.production);
        (vec![record], wonder, mine)
    }

    #[test]
    fn the_owner_of_a_lost_order_is_told_what_where_why_and_that_the_production_is_kept() {
        let mut session = new_match();
        let me = session.me();
        let (records, wonder, lost) = stale_order(&mut session, me, PlayerId(1), false);
        session.notify_lost_orders(&records);
        let city = session.game().view().auth.cities().get(lost).unwrap().clone();
        let name = names::item(session.rules(), ProductionItem::Building(wonder));
        let expected = format!("{}: {} was finished in another city. The order was removed; the production you had stored is kept.", city.name, name);
        assert_eq!(texts(&mut session), [expected]);
        let notice = session.hud_model(None).notices.into_iter().next().unwrap();
        assert_eq!((notice.tile, notice.kind), (Some(city.tile.0), NoticeKind::Production));
        let ru = i18n::with_language("ru", || {
            session.notices.clear();
            session.notify_lost_orders(&records);
            texts(&mut session)
        });
        assert!(ru[0].contains("Заказ снят, накопленное производство сохранено."), "{ru:?}");
    }

    #[test]
    fn another_players_lost_order_is_not_told() {
        let mut session = new_match();
        let me = session.me();
        let (records, _, lost) = stale_order(&mut session, PlayerId(1), me, false);
        let owner = session.game().view().auth.cities().get(lost).unwrap().owner;
        assert_eq!(owner, PlayerId(1));
        assert!(
            records.iter().flat_map(|r| &r.changes.city_changes).any(|c| matches!(c, CityChange::OrderRemoved { owner, .. } if *owner == PlayerId(1))),
            "the journal holds the rival's removal"
        );
        session.notify_lost_orders(&records);
        assert!(texts(&mut session).is_empty(), "{:?}", texts(&mut session));
    }

    #[test]
    fn a_save_made_before_the_change_loads_and_drops_the_stale_order_with_the_message() {
        let mut session = new_match();
        let me = session.me();
        let (records, wonder, lost) = stale_order(&mut session, me, PlayerId(1), true);
        let removed = records.iter().flat_map(|r| &r.changes.city_changes).any(|c| {
            matches!(c, CityChange::OrderRemoved { city, owner, item, reason }
                if *city == lost && *owner == me && *item == ProductionItem::Building(wonder) && *reason == OrderRemoval::WonderBuilt)
        });
        assert!(removed, "the drain journals the removal");
        session.notify_lost_orders(&records);
        assert_eq!(texts(&mut session).len(), 1);
    }

    #[test]
    fn every_reason_has_its_own_text() {
        let mut session = new_match();
        let me = session.me();
        let (records, wonder, lost) = stale_order(&mut session, me, PlayerId(1), false);
        let item = ProductionItem::Building(wonder);
        let mut seen = Vec::new();
        for reason in [OrderRemoval::WonderBuilt, OrderRemoval::NationalWonderBuilt, OrderRemoval::AlreadyBuilt, OrderRemoval::NeedsBuilding, OrderRemoval::Blocked] {
            let mut journal = ChangeJournal::default();
            journal.city_changes.push(CityChange::OrderRemoved { city: lost, owner: me, item, reason });
            session.notices.clear();
            session.notify_lost_orders(&[AppliedCommand { command: records[0].command.clone(), changes: journal }]);
            let text = texts(&mut session).remove(0);
            assert!(text.contains("production you had stored is kept"), "{text}");
            assert!(!seen.contains(&text));
            seen.push(text);
        }
    }

    #[test]
    fn a_hurry_of_a_lost_head_is_refused_with_a_reason_and_costs_nothing() {
        let mut session = new_match();
        let me = session.me();
        let rules = game_rules();
        let (game, _) = session.harness_game();
        let wonder = plain_wonder(game, &[me, PlayerId(1)]);
        let city = city_of(game, me);
        game.execute(&rules, Command::EnqueueProduction { player: me, city, item: ProductionItem::Building(wonder) }).expect("enqueue");
        // The wonder stands elsewhere without a sweep (the harness grant, as an old save holds it).
        let other = city_of(game, PlayerId(1));
        game.debug_add_building(&rules, other, wonder);
        game.debug_earn(me, 1000);
        let gold = game.view().auth.players().treasury_of(me);
        let error = game.execute(&rules, Command::HurryProduction { player: me, city }).unwrap_err();
        assert_eq!(game.view().auth.players().treasury_of(me), gold);
        assert!(!names::reason(&rules, &error).is_empty());
    }
}
