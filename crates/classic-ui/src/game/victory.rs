//! Victory progress (domination, science, culture, diplomacy, time and the scores of the
//! nations met), the end of the game with its Hall of Fame line, and "one more
//! turn" after a victory.
use classic_rules::ClassicGame;
use classic_rules::world::victory::{
    capitals_held, foreign_capitals_held, score, spaceship_parts, VictoryKind, VictoryOptions,
};

use i18n::{t, tf};

use super::session::Session;
use crate::ui::model::{GameEndView, NoticeKind, Outcome, ScoreRow, VictoryRow, VictoryView};

const APOLLO: &str = "core:project/apollo_program";

impl Session {
    pub fn victory_view(&self) -> VictoryView {
        let auth = self.game().view().auth;
        let options = auth.module().victory().options();
        let rows = vec![
            self.domination_row(options),
            self.science_row(options),
            self.culture_row(options),
            self.diplomatic_row(options),
            self.time_row(options),
        ];
        VictoryView {
            turn: self.turn(),
            max_turns: options.max_rounds + 1,
            rows,
            scores: self.scores(),
        }
    }

    fn domination_row(&self, options: VictoryOptions) -> VictoryRow {
        let auth = self.game().view().auth;
        let (total, held) = capitals_held(auth, self.me());
        let foreign = foreign_capitals_held(auth, self.me());
        VictoryRow {
            kind: t!("classic.game.victory.row-domination"),
            enabled: options.enabled(VictoryKind::Domination),
            goal: t!("classic.game.victory.goal-domination").into(),
            progress: if foreign == 0 && held == total {
                tf!("classic.game.victory.progress-domination-lone", held = held, total = total)
            } else {
                tf!("classic.game.victory.progress-domination", held = held, total = total)
            },
            show_progress: total > held || foreign > 0,
            have: held as u32,
            need: total as u32,
        }
    }

    fn science_row(&self, options: VictoryOptions) -> VictoryRow {
        let auth = self.game().view().auth;
        let rules = self.rules();
        let me = self.me();
        let (mut have, mut need) = (0u32, 0u32);
        for (project, count) in spaceship_parts(rules) {
            have += u32::from(auth.module().victory().project_count(me, project).min(count));
            need += u32::from(count);
        }
        let apollo = rules.module.projects.iter().position(|p| p.key == APOLLO);
        let built = apollo.is_some_and(|i| {
            let id = classic_rules::rules::content::ProjectId(i as u16);
            auth.module().victory().project_count(me, id) > 0
        });
        let progress = if built {
            tf!("classic.game.victory.progress-science-built", have = have, need = need)
        } else {
            tf!("classic.game.victory.progress-science", have = have, need = need)
        };
        VictoryRow {
            kind: t!("classic.game.victory.row-science"),
            enabled: options.enabled(VictoryKind::Science),
            goal: t!("classic.game.victory.goal-science").into(),
            progress,
            show_progress: true,
            have: have + u32::from(built),
            need: need + 1,
        }
    }

    fn culture_row(&self, options: VictoryOptions) -> VictoryRow {
        let auth = self.game().view().auth;
        let have = classic_rules::derived::tourism::influential_on(auth, self.me());
        let need = classic_rules::derived::tourism::nations_needed_for_cultural_victory(auth);
        VictoryRow {
            kind: t!("classic.game.victory.row-culture"),
            enabled: options.enabled(VictoryKind::Cultural),
            goal: t!("classic.game.victory.goal-culture").into(),
            progress: tf!("classic.game.victory.progress-culture", have = have, need = need),
            show_progress: true,
            have,
            need,
        }
    }

    fn diplomatic_row(&self, options: VictoryOptions) -> VictoryRow {
        let auth = self.game().view().auth;
        let congress = auth.module().congress();
        let have = classic_rules::world::congress::delegates(auth, self.rules(), self.me());
        let need = classic_rules::world::congress::votes_needed(auth);
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
            enabled: options.enabled(VictoryKind::Diplomatic),
            goal: t!("classic.game.victory.goal-diplomacy").into(),
            progress,
            show_progress: congress.united_nations() || auth.module().victory().world_leader().is_some(),
            have: u32::from(have),
            need: u32::from(need),
        }
    }

    /// The time victory: its last turn, and the game's speed, which sets it, beside the row's name.
    fn time_row(&self, options: VictoryOptions) -> VictoryRow {
        let last = options.max_rounds + 1;
        let speed = crate::ui::screens::GameSpeed::of_key(&self.game().view().auth.settings().speed);
        VictoryRow {
            kind: t!("classic.game.victory.row-time"),
            enabled: options.enabled(VictoryKind::Time),
            goal: format!("{} · {}", tf!("classic.game.victory.goal-time", last = last), speed.label()),
            progress: tf!("classic.game.victory.progress-time", turn = self.turn(), last = last),
            show_progress: true,
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
}
