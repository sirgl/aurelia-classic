//! Income taken by a kill or by force, told to the human player: the yields an effect paid for a
//! kill (Honor's culture for a barbarian, gold for every kill, a belief's faith) and the gold of
//! a pillaged improvement, a captured city or a plundered trade route. The gold of a barbarian
//! camp is `camps.rs`'s. The notice of a natural wonder the player found, with the finder gold and
//! the happiness it adds, is here too.
use classic_rules::classic::effects::{ClassicSource, ClassicYield};
use classic_rules::classic::ClassicChange;
use classic_rules::ids::TileId;
use classic_rules::rules::content::NaturalWonderId;
use classic_rules::rules::effects::{SourceRef, YieldKind};
use classic_rules::transaction::journal::AppliedCommand;
use classic_rules::PlunderSource;
use i18n::tf;

use super::names;
use super::session::Session;
use crate::ui::model::NoticeKind;

/// "+11 culture": an amount of a yield in a short phrase.
fn part(kind: YieldKind, amount: u32) -> String {
    match kind {
        YieldKind::Gold => tf!("classic.game.income.gold", n = amount),
        YieldKind::Culture => tf!("classic.game.income.culture", n = amount),
        YieldKind::Science => tf!("classic.game.income.science", n = amount),
        YieldKind::Food => tf!("classic.game.income.food", n = amount),
        YieldKind::Production => tf!("classic.game.income.production", n = amount),
        YieldKind::Module(ClassicYield::Faith) => tf!("classic.game.income.faith", n = amount),
        YieldKind::Output => String::new(),
    }
}

/// The notice text of the yields of one command: `parts` (one phrase per yield), the name of the
/// source when all of them come from one.
fn kill_text(source: Option<&str>, parts: &[String], barbarian: bool) -> String {
    let list = parts.iter().skip(1).fold(parts.first().cloned().unwrap_or_default(), |all, next| tf!("classic.game.income.and", a = all, b = next.as_str()));
    let barbarian = if barbarian { "yes" } else { "no" };
    match source {
        Some(source) => tf!("classic.game.kill.notice", source = source, parts = list, barbarian = barbarian),
        None => tf!("classic.game.kill.notice-plain", parts = list, barbarian = barbarian),
    }
}

/// The notice text of gold taken by force.
fn plunder_text(source: PlunderSource, gold: u32) -> String {
    match source {
        PlunderSource::Improvement => tf!("classic.game.plunder.improvement", gold = gold),
        PlunderSource::City => tf!("classic.game.plunder.city", gold = gold),
        PlunderSource::TradeRoute => tf!("classic.game.plunder.route", gold = gold),
    }
}

/// The notice text of a natural wonder found: its `name`, the finder gold (the first finder's), what
/// effects paid and the happiness one found wonder adds; only what is more than nothing.
fn wonder_text(name: &str, first_gold: u32, yields: &[(YieldKind, u32)], happiness: i32) -> String {
    let mut parts: Vec<String> = Vec::new();
    if first_gold > 0 {
        parts.push(tf!("classic.game.wonder.gold-first", n = first_gold));
    }
    parts.extend(yields.iter().map(|(kind, amount)| part(*kind, *amount)));
    if happiness > 0 {
        parts.push(tf!("classic.game.wonder.happiness", n = happiness));
    }
    match parts.split_first() {
        None => tf!("classic.game.wonder.found", name = name),
        Some((first, rest)) => {
            let list = rest.iter().fold(first.clone(), |all, next| tf!("classic.game.income.and", a = all, b = next.as_str()));
            tf!("classic.game.wonder.found-with", name = name, parts = list)
        }
    }
}

impl Session {
    /// The name of a natural wonder in the active language.
    fn natural_wonder_name(&self, wonder: NaturalWonderId) -> String {
        match self.rules().natural_wonders.get(wonder.0 as usize) {
            Some(def) => names::named("natural-wonder", &def.name, &def.key),
            None => String::new(),
        }
    }

    /// The name an effect's source goes by in a notice: a policy by its branch ("Honor"), a belief,
    /// a nation, a technology or a building by its name; none for the others.
    fn income_source_name(&self, source: SourceRef) -> Option<String> {
        let rules = self.rules();
        match source {
            SourceRef::Nation(id) => Some(names::nation(rules, id)),
            SourceRef::Technology(tech) => Some(names::tech(rules, tech)),
            SourceRef::Building(building) => Some(names::building(rules, building)),
            SourceRef::Module(ClassicSource::Policy(policy)) => {
                let def = rules.module.policies.get(policy.0 as usize)?;
                match def.branch.and_then(|b| rules.module.policy_branches.get(b.0 as usize)) {
                    Some(branch) => Some(names::named("policy", &branch.name, &branch.key)),
                    None => Some(names::named("policy", &def.name, &def.key)),
                }
            }
            SourceRef::Module(ClassicSource::Belief(belief)) => {
                let def = rules.module.beliefs.get(belief.0 as usize)?;
                Some(names::named("belief", &def.name, &def.key))
            }
            SourceRef::Unit(_) | SourceRef::Promotion(_) => None,
        }
    }

    /// A notice for the income the human player's kills paid (one line per command, the yields of
    /// one kind added up) and for the gold their units took by force (a line each), centred on the
    /// tile where it happened.
    pub(super) fn notify_income(&mut self, records: &[AppliedCommand]) {
        let me = self.me();
        if self.game().view().auth.settings().is_ai(me) {
            return;
        }
        for record in records {
            let mut yields: Vec<(YieldKind, u32)> = Vec::new();
            let mut sources: Vec<SourceRef> = Vec::new();
            let mut spot: Option<TileId> = None;
            let mut barbarians = true;
            // What effects paid for a find comes before the find in the journal.
            let mut wonder_yields: Vec<(YieldKind, u32)> = Vec::new();
            for change in &record.changes.module_changes {
                match *change {
                    ClassicChange::WonderYield { player, kind, amount, .. } if player == me => {
                        match wonder_yields.iter_mut().find(|(k, _)| *k == kind) {
                            Some((_, sum)) => *sum = sum.saturating_add(amount),
                            None => wonder_yields.push((kind, amount)),
                        }
                    }
                    ClassicChange::NaturalWonderFound { player, tile, wonder, gold, happiness, .. } if player == me => {
                        let text = wonder_text(&self.natural_wonder_name(wonder), gold, &wonder_yields, happiness);
                        wonder_yields.clear();
                        self.notify_at(NoticeKind::Culture, text, Some(tile.0));
                    }
                    ClassicChange::KillYield { player, tile, kind, amount, source, barbarian } if player == me => {
                        match yields.iter_mut().find(|(k, _)| *k == kind) {
                            Some((_, sum)) => *sum = sum.saturating_add(amount),
                            None => yields.push((kind, amount)),
                        }
                        if !sources.contains(&source) {
                            sources.push(source);
                        }
                        spot.get_or_insert(tile);
                        barbarians &= barbarian;
                    }
                    ClassicChange::Plundered { player, tile, source, gold } if player == me => {
                        self.notify_at(NoticeKind::Combat, plunder_text(source, gold), Some(tile.0));
                    }
                    _ => {}
                }
            }
            if let Some(tile) = spot {
                let parts: Vec<String> = yields.iter().map(|(kind, amount)| part(*kind, *amount)).collect();
                let name = match sources[..] {
                    [one] => self.income_source_name(one),
                    _ => None,
                };
                self.notify_at(NoticeKind::Combat, kill_text(name.as_deref(), &parts, barbarians), Some(tile.0));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use classic_rules::classic::effects::ClassicSource;
    use classic_rules::ids::{PlayerId, PolicyId, UnitKindId};
    use classic_rules::transaction::journal::ChangeJournal;
    use classic_rules::{ClassicGame, Command};

    use super::*;
    use crate::ui::screens::{game_rules, prepare_match, MapSize, MatchRequest};

    fn new_match() -> Session {
        let request = MatchRequest { nation: "core:nation/rome".into(), seed: 4, size: MapSize::Small, players: 3, water: 25 };
        Session::new(prepare_match(request).expect("world").game, game_rules())
    }

    /// A record whose journal holds `changes`.
    fn record(changes: Vec<ClassicChange>) -> AppliedCommand {
        let mut journal = ChangeJournal::default();
        journal.module_changes = changes;
        AppliedCommand { command: Command::EndTurn { player: PlayerId(0) }, changes: journal }
    }

    fn texts(session: &mut Session) -> Vec<String> {
        session.hud_model(None).notices.into_iter().rev().map(|n| n.text).collect()
    }

    fn kill_yield(player: PlayerId, tile: TileId, kind: YieldKind, amount: u32, source: SourceRef, barbarian: bool) -> ClassicChange {
        ClassicChange::KillYield { player, tile, kind, amount, source, barbarian }
    }

    /// Honor's finisher pays gold for every kill: P0 (or P1) kills a wounded warrior of the other in
    /// a real game; the records of the kill and the gold the killer got.
    fn honor_kill(session: &Session, killer: PlayerId, victim: PlayerId) -> (Vec<AppliedCommand>, TileId, u32) {
        let rules = game_rules();
        let mut game = session.game().clone();
        let finisher = PolicyId(rules.module.policies.iter().position(|p| p.key == "core:policy/honor/finisher").unwrap() as u16);
        game.debug_adopt_policy(&rules, killer, finisher);
        while game.view().auth.active_player() != killer {
            let player = game.view().auth.active_player();
            game.execute(&rules, Command::EndTurn { player }).unwrap();
        }
        let start = game.view().auth.indexes().units_of(PlayerId(0))[0];
        let start = game.view().auth.units().get(start).unwrap().core.tile;
        let dims = game.view().auth.map().dimensions;
        let free = |game: &classic_rules::GameState, t: TileId| {
            let auth = game.view().auth;
            classic_rules::navigation::cost::enter_cost_on_map(auth.map(), t).is_some()
                && auth.indexes().units_on(t).is_empty()
                && auth.indexes().city_on_tile(t).is_none()
        };
        let (from, at) = classic_rules::math::tiles_in_hex_radius(&dims, start, 2)
            .into_iter()
            .filter(|t| free(&game, *t))
            .find_map(|a| {
                let b = classic_rules::math::tiles_in_hex_radius(&dims, a, 1).into_iter().find(|b| *b != a && free(&game, *b))?;
                Some((a, b))
            })
            .expect("two free land tiles next to each other near the start");
        let warrior = UnitKindId(rules.units.iter().position(|u| u.key == "core:unit/warrior").unwrap() as u16);
        let attacker = game.debug_spawn_unit(&rules, killer, from, warrior);
        let defender = game.debug_spawn_unit(&rules, victim, at, warrior);
        game.debug_set_unit_hp(defender, 1);
        game.debug_meet(killer, victim);
        game.debug_force_war(&rules, killer, victim);
        let before = game.view().auth.players().treasury_of(killer);
        let command = Command::AttackUnit { attacker, defender };
        let result = game.execute(&rules, command.clone()).unwrap();
        assert!(game.view().auth.units().get(defender).is_none(), "the victim dies");
        let gold = game.view().auth.players().treasury_of(killer) - before;
        (vec![AppliedCommand { command, changes: result.into_journal() }], at, gold)
    }

    #[test]
    fn a_kill_that_paid_leaves_one_notice_with_the_source_the_sum_and_the_place() {
        let mut session = new_match();
        let me = session.me();
        let (records, at, gold) = honor_kill(&session, me, PlayerId(1));
        assert!(gold > 0);
        session.notify_income(&records);
        assert_eq!(texts(&mut session), [format!("Honor: +{gold} gold for a kill")]);
        let notice = session.hud_model(None).notices.into_iter().next().unwrap();
        assert_eq!((notice.tile, notice.kind), (Some(at.0), NoticeKind::Combat));
    }

    #[test]
    fn another_players_kill_is_not_told() {
        let mut session = new_match();
        let (records, _, gold) = honor_kill(&session, PlayerId(1), session.me());
        assert!(gold > 0, "P1 was paid");
        session.notify_income(&records);
        assert!(texts(&mut session).is_empty(), "{:?}", texts(&mut session));
    }

    #[test]
    fn an_ai_player_is_told_nothing() {
        let mut session = new_match();
        let me = session.me();
        let (records, _, _) = honor_kill(&session, me, PlayerId(1));
        let (game, _) = session.harness_game();
        game.configure_players("", &[me]);
        session.notify_income(&records);
        assert!(texts(&mut session).is_empty());
    }

    #[test]
    fn several_yields_of_one_command_are_one_line_and_gold_taken_by_force_a_line_each() {
        let mut session = new_match();
        let me = session.me();
        let (a, b) = (TileId(10), TileId(11));
        let honor = SourceRef::Module(ClassicSource::Policy(PolicyId(0)));
        let nation = SourceRef::Nation(0);
        session.notify_income(&[record(vec![
            kill_yield(me, a, YieldKind::Culture, 11, honor, true),
            kill_yield(me, a, YieldKind::Gold, 7, nation, true),
            kill_yield(me, a, YieldKind::Gold, 3, honor, true),
            ClassicChange::Plundered { player: me, tile: b, source: PlunderSource::City, gold: 90 },
            ClassicChange::Plundered { player: PlayerId(1), tile: b, source: PlunderSource::City, gold: 50 },
            ClassicChange::CampCleared { player: me, tile: b, gold: 25 },
        ])]);
        assert_eq!(
            texts(&mut session),
            ["City plundered: +90 gold", "+11 culture, +10 gold for a slain barbarian"],
            "two sources: no name; a camp's gold is camps.rs's; the other player's plunder is not told"
        );
        let tiles: Vec<_> = session.hud_model(None).notices.iter().map(|n| n.tile).collect();
        assert_eq!(tiles, [Some(a.0), Some(b.0)]);
    }

    /// A game of the session where `site` holds El Dorado, and the records of the end of turns:
    /// P0 and P1 both see the wonder; P1's turn comes first when `p1_first`.
    fn el_dorado_finds(session: &Session, p1_first: bool, speed: &str) -> (TileId, Vec<AppliedCommand>, Vec<AppliedCommand>) {
        let rules = game_rules();
        let mut game = session.game().clone();
        if !speed.is_empty() {
            game.set_speed(&rules, speed);
        }
        let start = game.view().auth.indexes().units_of(PlayerId(0))[0];
        let start = game.view().auth.units().get(start).unwrap().core.tile;
        let site = classic_rules::math::tiles_in_hex_radius(&game.view().auth.map().dimensions, start, 3)
            .into_iter()
            .find(|t| *t != start && game.view().auth.indexes().city_on_tile(*t).is_none())
            .expect("a tile near the start");
        let el_dorado = rules.natural_wonders.iter().position(|w| w.key == "core:natural-wonder/el-dorado").unwrap();
        game.debug_set_natural_wonder_batch(&rules, &[(site, classic_rules::rules::content::NaturalWonderId(el_dorado as u16))]);
        let (mut mine, mut theirs) = (Vec::new(), Vec::new());
        let mut end_turn = |game: &mut classic_rules::GameState| {
            let player = game.view().auth.active_player();
            let command = Command::EndTurn { player };
            let result = game.execute(&rules, command.clone()).unwrap();
            let record = AppliedCommand { command, changes: result.into_journal() };
            if player == PlayerId(0) { mine.push(record) } else if player == PlayerId(1) { theirs.push(record) }
        };
        if p1_first {
            game.debug_reveal(PlayerId(1), &[site]);
            end_turn(&mut game); // P0: nothing to find yet
            end_turn(&mut game); // P1: the first finder
            game.debug_reveal(PlayerId(0), &[site]);
        } else {
            game.debug_reveal(PlayerId(0), &[site]);
            game.debug_reveal(PlayerId(1), &[site]);
        }
        while game.view().auth.active_player() != PlayerId(0) {
            end_turn(&mut game);
        }
        end_turn(&mut game); // P0
        while game.view().auth.active_player() != PlayerId(1) {
            end_turn(&mut game);
        }
        end_turn(&mut game); // P1
        (site, mine, theirs)
    }

    #[test]
    fn the_first_finder_of_a_wonder_is_told_the_gold_and_the_happiness_at_its_place() {
        let mut session = new_match();
        let (site, mine, _) = el_dorado_finds(&session, false, "");
        let happiness = classic_rules::derived::happiness::natural_wonder_find_happiness(session.game().view().auth, &game_rules(), session.me());
        session.notify_income(&mine);
        assert_eq!(texts(&mut session), [format!("Natural wonder found: El Dorado: +500 gold (first to find it), +{happiness} happiness")]);
        let notice = session.hud_model(None).notices.into_iter().next().unwrap();
        assert_eq!(notice.tile, Some(site.0));
    }

    #[test]
    fn the_notice_shows_the_gold_scaled_by_the_game_speed() {
        let mut session = new_match();
        let rules = game_rules();
        let quick = rules.speeds.iter().find(|s| s.key.ends_with("quick")).expect("a Quick speed");
        let expected = classic_rules::rules::speeds::SpeedValues::scale(500, quick.values.gold_percent);
        assert!(expected < 500);
        let (_, mine, _) = el_dorado_finds(&session, false, &quick.key);
        session.notify_income(&mine);
        let lines = texts(&mut session);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with(&format!("Natural wonder found: El Dorado: +{expected} gold (first to find it)")), "{}", lines[0]);
    }

    #[test]
    fn a_later_finder_is_told_the_wonder_and_the_happiness_but_no_gold() {
        let mut session = new_match();
        let (_, mine, _) = el_dorado_finds(&session, true, "");
        let happiness = classic_rules::derived::happiness::natural_wonder_find_happiness(session.game().view().auth, &game_rules(), session.me());
        session.notify_income(&mine);
        assert_eq!(texts(&mut session), [format!("Natural wonder found: El Dorado: +{happiness} happiness")]);
    }

    #[test]
    fn another_players_find_and_an_ai_players_find_are_not_told() {
        let mut session = new_match();
        let (_, mine, theirs) = el_dorado_finds(&session, false, "");
        session.notify_income(&theirs);
        assert!(texts(&mut session).is_empty(), "P1 found it: not P0's business");
        let me = session.me();
        let (game, _) = session.harness_game();
        game.configure_players("", &[me]);
        session.notify_income(&mine);
        assert!(texts(&mut session).is_empty(), "an AI player is told nothing");
    }

    #[test]
    fn a_wonder_text_adds_what_effects_paid_and_leaves_out_what_is_nothing() {
        assert_eq!(wonder_text("Mt. Fuji", 0, &[], 0), "Natural wonder found: Mt. Fuji");
        assert_eq!(wonder_text("Mt. Fuji", 0, &[(YieldKind::Gold, 100)], 2), "Natural wonder found: Mt. Fuji: +100 gold, +2 happiness");
        assert_eq!(
            wonder_text("El Dorado", 500, &[(YieldKind::Gold, 500), (YieldKind::Culture, 3)], 0),
            "Natural wonder found: El Dorado: +500 gold (first to find it), +500 gold, +3 culture"
        );
        let mut session = new_match();
        let me = session.me();
        let tile = TileId(7);
        let wonder = classic_rules::rules::content::NaturalWonderId(0);
        let spain = SourceRef::Nation(0);
        session.notify_income(&[record(vec![
            ClassicChange::WonderYield { player: me, tile, kind: YieldKind::Gold, amount: 100, source: spain },
            ClassicChange::NaturalWonderFound { player: me, tile, wonder, first: false, gold: 0, happiness: 1 },
            ClassicChange::WonderYield { player: PlayerId(1), tile, kind: YieldKind::Gold, amount: 500, source: spain },
        ])]);
        let lines = texts(&mut session);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].ends_with(": +100 gold, +1 happiness"), "{}", lines[0]);
    }

    #[test]
    fn the_texts_name_every_income_in_both_languages() {
        let parts = [YieldKind::Gold, YieldKind::Culture, YieldKind::Science, YieldKind::Food, YieldKind::Production, YieldKind::Module(ClassicYield::Faith)]
            .map(|kind| part(kind, 12));
        assert_eq!(parts, ["+12 gold", "+12 culture", "+12 science", "+12 food", "+12 production", "+12 faith"]);
        assert_eq!(kill_text(Some("Honor"), &parts[..2], false), "Honor: +12 gold, +12 culture for a kill");
        assert_eq!(kill_text(None, &parts[..1], true), "+12 gold for a slain barbarian");
        assert_eq!(plunder_text(PlunderSource::Improvement, 5), "Improvement pillaged: +5 gold");
        assert_eq!(plunder_text(PlunderSource::TradeRoute, 60), "Trade route plundered: +60 gold");
        for language in ["ru", "de", "es", "fr", "ja", "zh-Hans"] {
            i18n::with_language(language, || {
                let all = [
                    kill_text(Some("X"), &parts[..2], true),
                    kill_text(None, &parts[..1], false),
                    plunder_text(PlunderSource::Improvement, 5),
                    plunder_text(PlunderSource::City, 5),
                    plunder_text(PlunderSource::TradeRoute, 5),
                ];
                let wonder = [wonder_text("X", 0, &[], 0), wonder_text("X", 5, &[(YieldKind::Gold, 5)], 1)];
                for text in all.iter().chain(parts.iter()).chain(wonder.iter()) {
                    assert!(!text.contains("classic.game") && !text.contains('{'), "{language}: {text}");
                }
                assert!(!kill_text(None, &parts[..1], true).contains("slain barbarian"), "{language} is translated");
            });
        }
        i18n::with_language("ru", || {
            let parts = [YieldKind::Gold, YieldKind::Culture].map(|kind| part(kind, 12));
            crate::game::reasons::only_cyrillic(&kill_text(Some("\u{427}"), &parts, true).replace(char::is_numeric, "").replace('+', ""));
            crate::game::reasons::only_cyrillic(&plunder_text(PlunderSource::City, 5).replace(char::is_numeric, "").replace('+', ""));
        });
    }
}
