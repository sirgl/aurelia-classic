//! Victory progress (domination, science, culture, diplomacy, time and the scores of the
//! nations met), the end of the game with its Hall of Fame line, and "one more
//! turn" after a victory.
use classic_rules::ClassicGame;
use classic_rules::derived::standing::VictoryProgress;
use classic_rules::world::victory::{score, VictoryKind};

use i18n::{t, tf};

use super::session::Session;
use crate::ui::model::{GameEndView, NoticeKind, Outcome, ScoreRow, VictoryRow, VictoryView};

impl Session {
    pub fn victory_view(&self) -> VictoryView {
        let auth = self.game().view().auth;
        let options = auth.module().victory().options();
        let progress = classic_rules::derived::standing::victory_progress(auth, self.rules(), self.me());
        let of = |kind: VictoryKind| *progress.iter().find(|p| p.kind == kind).expect("every kind has a row");
        let rows = vec![
            self.domination_row(of(VictoryKind::Domination)),
            self.science_row(of(VictoryKind::Science)),
            self.culture_row(of(VictoryKind::Cultural)),
            self.diplomatic_row(of(VictoryKind::Diplomatic)),
            self.time_row(of(VictoryKind::Time)),
        ];
        VictoryView {
            turn: self.turn(),
            max_turns: options.max_rounds + 1,
            rows,
            scores: self.scores(),
        }
    }

    fn domination_row(&self, p: VictoryProgress) -> VictoryRow {
        let (held, total, foreign) = (p.have, p.need, p.extra);
        VictoryRow {
            kind: t!("classic.game.victory.row-domination"),
            enabled: p.enabled,
            goal: t!("classic.game.victory.goal-domination").into(),
            progress: if foreign == 0 && held == total {
                tf!("classic.game.victory.progress-domination-lone", held = held, total = total)
            } else {
                tf!("classic.game.victory.progress-domination", held = held, total = total)
            },
            show_progress: p.show_progress,
            have: held,
            need: total,
        }
    }

    fn science_row(&self, p: VictoryProgress) -> VictoryRow {
        let (have, need) = (p.have - p.extra, p.need - 1);
        let progress = if p.extra > 0 {
            tf!("classic.game.victory.progress-science-built", have = have, need = need)
        } else {
            tf!("classic.game.victory.progress-science", have = have, need = need)
        };
        VictoryRow {
            kind: t!("classic.game.victory.row-science"),
            enabled: p.enabled,
            goal: t!("classic.game.victory.goal-science").into(),
            progress,
            show_progress: p.show_progress,
            have: p.have,
            need: p.need,
        }
    }

    fn culture_row(&self, p: VictoryProgress) -> VictoryRow {
        VictoryRow {
            kind: t!("classic.game.victory.row-culture"),
            enabled: p.enabled,
            goal: t!("classic.game.victory.goal-culture").into(),
            progress: tf!("classic.game.victory.progress-culture", have = p.have, need = p.need),
            show_progress: p.show_progress,
            have: p.have,
            need: p.need,
        }
    }

    fn diplomatic_row(&self, p: VictoryProgress) -> VictoryRow {
        let auth = self.game().view().auth;
        let congress = auth.module().congress();
        let (have, need) = (p.have, p.need);
        let progress = match auth.module().victory().world_leader() {
            Some(leader) => tf!("classic.game.victory.progress-diplomacy-leader", name = self.player_name(leader)),
            None if !congress.founded() => t!("classic.game.victory.progress-diplomacy-no-congress").into(),
            None if !congress.united_nations() => {
                tf!("classic.game.victory.progress-diplomacy-no-un", have = have, need = need)
            }
            None => tf!("classic.game.victory.progress-diplomacy", have = have, need = need),
        };
        VictoryRow {
            kind: t!("classic.game.victory.row-diplomacy"),
            enabled: p.enabled,
            goal: t!("classic.game.victory.goal-diplomacy").into(),
            progress,
            show_progress: p.show_progress,
            have,
            need,
        }
    }

    /// The time victory: its last turn, and the game's speed, which sets it, beside the row's name.
    fn time_row(&self, p: VictoryProgress) -> VictoryRow {
        let last = p.need;
        let speed = crate::ui::screens::GameSpeed::of_key(&self.game().view().auth.settings().speed);
        VictoryRow {
            kind: t!("classic.game.victory.row-time"),
            enabled: p.enabled,
            goal: format!("{} · {}", tf!("classic.game.victory.goal-time", last = last), speed.label()),
            progress: tf!("classic.game.victory.progress-time", turn = self.turn(), last = last),
            show_progress: p.show_progress,
            have: self.turn(),
            need: last,
        }
    }

    /// The player and every nation it met, best score first.
    fn scores(&self) -> Vec<ScoreRow> {
        let auth = self.game().view().auth;
        let mut players = vec![self.me()];
        players.extend(self.met_nations().iter().map(|r| r.other));
        let mut rows: Vec<ScoreRow> = players
            .into_iter()
            .map(|p| ScoreRow {
                name: self.player_name(p),
                score: score(auth, self.rules(), p),
                mine: p == self.me(),
            })
            .collect();
        rows.sort_by_key(|r| std::cmp::Reverse(r.score));
        rows
    }

    /// The results screen; `hall_of_fame` holds the lines of earlier games.
    pub fn game_end_view(&self, hall_of_fame: &[String]) -> GameEndView {
        let game = self.game();
        let won = self.outcome() == Outcome::Victory;
        let alive = game.view().auth.players().is_alive(self.me());
        GameEndView {
            won,
            title: if won { t!("classic.game.victory.title-victory") } else { t!("classic.game.victory.title-defeat") },
            line: self.end_line(),
            score: score(game.view().auth, self.rules(), self.me()),
            can_continue: alive && game.is_over(),
            hall_of_fame: hall_of_fame.to_vec(),
        }
    }

    fn end_line(&self) -> String {
        let Some(outcome) = self.game().outcome() else {
            return t!("classic.game.victory.end-fallen").into();
        };
        let kind = kind_name(outcome.kind);
        let turn = outcome.round + 1;
        if outcome.winner == self.me() {
            return tf!("classic.game.victory.end-won", victory = kind, turn = turn);
        }
        let winner = self.player_name(outcome.winner);
        tf!("classic.game.victory.end-lost", winner = winner, victory = kind, turn = turn)
    }

    /// One date-free line for the Hall of Fame: nation, result, turn, score.
    pub fn hall_of_fame_line(&self) -> String {
        let me = self.me();
        let nation = self.player_name(me);
        let score = score(self.game().view().auth, self.rules(), me);
        let turn = self.turn();
        let result = match self.game().outcome() {
            Some(o) if o.winner == me => kind_name(o.kind).to_string(),
            Some(o) => tf!("classic.game.victory.hall-defeat",
                winner = self.player_name(o.winner),
                victory = kind_name(o.kind)
            ),
            None => t!("classic.game.victory.hall-eliminated").to_string(),
        };
        tf!("classic.game.victory.hall-line", nation = nation, result = result, turn = turn, score = score)
    }

    /// "One more turn": the game goes on after a victory.
    pub(super) fn continue_after_victory(&mut self) {
        let alive = self.game().view().auth.players().is_alive(self.me());
        if !self.game().is_over() || !alive {
            return;
        }
        self.client.game.continue_after_victory();
        self.notify(NoticeKind::Diplomacy, t!("classic.game.victory.one-more-turn").into());
    }
}

fn kind_name(kind: VictoryKind) -> &'static str {
    match kind {
        VictoryKind::Time => t!("classic.game.victory.kind-time"),
        VictoryKind::Science => t!("classic.game.victory.kind-science"),
        VictoryKind::Domination => t!("classic.game.victory.kind-domination"),
        VictoryKind::Cultural => t!("classic.game.victory.kind-cultural"),
        VictoryKind::Diplomatic => t!("classic.game.victory.kind-diplomatic"),
    }
}

#[cfg(test)]
mod projection_tests {
    use super::*;
    use crate::ui::model::{Action, GameRequest};
    use crate::ui::screens::{game_rules, prepare_match, MapSize, MatchRequest};

    #[test]
    fn early_victory_counts_do_not_imply_an_active_or_completed_path() {
        let prepared = prepare_match(MatchRequest {
            nation: "core:nation/rome".into(), seed: 7, size: MapSize::Small,
            players: 3, water: 25,
        }).unwrap();
        let mut session = Session::new(prepared.game, game_rules());
        session.request(GameRequest::Unit(Action::FoundCity));
        assert!(session.city_view().is_some());
        let view = session.victory_view();
        let domination = view.rows.iter().find(|row| row.kind == "Domination").unwrap();
        assert_eq!((domination.have, domination.need), (1, 1));
        assert!(!domination.show_progress, "a lone home capital is not domination");
        assert!(domination.progress.contains("No foreign capital captured"));
        let diplomacy = view.rows.iter().find(|row| row.kind == "Diplomacy").unwrap();
        assert!(diplomacy.have > 0, "potential delegates exist even before Congress");
        assert!(!diplomacy.show_progress, "potential delegates are not active votes");
        assert!(diplomacy.progress.contains("not founded"));
        assert_eq!(session.overview_view().victory.as_ref(), Some(&view));
        assert!(!session.game().is_over());
    }

    /// The AI's observation holds about the world what the player's own screens show: the same
    /// demographics rows, victory rows and score list, and only the nations the player has met.
    #[test]
    fn the_ai_observation_equals_the_screens_of_the_same_player() {
        use classic_rules::ids::PlayerId;
        use classic_rules::knowledge::observation::AIObservation;
        let prepared = prepare_match(MatchRequest {
            nation: "core:nation/rome".into(), seed: 7, size: MapSize::Small,
            players: 4, water: 25,
        }).unwrap();
        let mut session = Session::new(prepared.game, game_rules());
        session.request(GameRequest::Unit(Action::FoundCity));
        let (game, rules) = session.harness_game();
        game.debug_meet(PlayerId(0), PlayerId(1));
        session.refresh_after_harness();
        for _ in 0..40 {
            session.request(GameRequest::EndTurn);
        }
        assert!(session.game().view().auth.turn().round >= 20, "a game under way");
        let overview = session.overview_view();
        let victory = session.victory_view();
        let view = session.game().view();
        let (obs, _) = AIObservation::for_player(view.auth, view.derived, session.rules(), session.me());
        let screen: Vec<_> = overview.demographics.iter().map(|d| (d.value, d.rank, d.best, d.average, d.worst)).collect();
        let seen: Vec<_> = obs.public.demographics.iter().map(|d| (d.value, d.rank, d.best, d.average, d.worst)).collect();
        assert_eq!(seen, screen, "demographics");
        assert!(seen.iter().any(|d| d.0 > 0), "a world with something in it");
        let rows: Vec<_> = victory.rows.iter().map(|r| (r.enabled, r.show_progress, r.have, r.need)).collect();
        let order = [VictoryKind::Domination, VictoryKind::Science, VictoryKind::Cultural, VictoryKind::Diplomatic, VictoryKind::Time];
        let held: Vec<_> = order.iter().map(|k| obs.public.progress(*k).map(|p| (p.enabled, p.show_progress, if *k == VictoryKind::Time { session.turn() } else { p.have }, p.need)).unwrap()).collect();
        assert_eq!(held, rows, "victory rows");
        let mut scores: Vec<i64> = obs.public.scores.iter().map(|(_, s)| *s).collect();
        scores.sort_by_key(|s| std::cmp::Reverse(*s));
        assert_eq!(scores, victory.scores.iter().map(|s| s.score).collect::<Vec<_>>(), "score list");
        assert_eq!(obs.public.scores[0].0, session.me());
        let met: Vec<PlayerId> = obs.relations.iter().filter(|r| view.auth.players().is_major(r.other)).map(|r| r.other).collect();
        for (p, _) in obs.public.scores.iter().skip(1) {
            assert!(met.contains(p), "{p:?} is on the score list but not met");
        }
        assert!(obs.public.scores.len() <= 1 + met.len(), "a nation not met is not on the list");
    }
}
