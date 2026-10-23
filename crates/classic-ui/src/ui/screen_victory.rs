//! Victory progress (`VictoryView`: domination, science, culture, diplomacy and time
//! with the scores of the civilizations met) and the results at the end of the game
//! (`GameEndView`: who won, how and when, "One more turn", "Main menu", Hall of Fame).
use super::*;
use i18n::{t, tf};
use crate::ui::model::{GameEndView, GameRequest, ScoreRow, VictoryRow, VictoryView};
use client::shell::kit::Availability;
use client::shell::widgets::game_end::{self, Choice, HallOfFame, Results, Verdict};

const SCORES_W: f32 = 320.0;
const ROW_H: f32 = 86.0;
const PROGRESS: Color32 = Color32::from_rgb(100, 198, 239);

pub(super) fn victory(ui: &mut egui::Ui, r: Rect, view: &VictoryView) {
    let top = Rect::from_min_size(r.min, vec2(r.width(), 92.0));
    let line = tf!("classic.ui.screen-victory.banner-line",
        turn = view.turn,
        max = view.max_turns,
        n = view.rows.iter().filter(|row| row.enabled).count()
    );
    world_banner(ui, top, t!("classic.ui.screen-victory.title"), &line, None);
    let body = Rect::from_min_max(pos2(r.left(), top.bottom() + 14.0), r.max);
    let right = Rect::from_min_max(pos2(body.right() - SCORES_W, body.top()), body.max);
    let left = Rect::from_min_max(body.min, pos2(right.left() - 14.0, body.bottom()));
    panel(ui.painter(), left);
    heading(ui, left, t!("classic.ui.screen-victory.progress"));
    let rows = Rect::from_min_max(left.min + vec2(10., 36.), left.max - vec2(10., 10.));
    ui.scope_builder(egui::UiBuilder::new().max_rect(rows), |ui| {
        ui.set_clip_rect(rows);
        egui::ScrollArea::vertical().id_salt("victory_conditions")
            .max_height(rows.height()).auto_shrink([false, false]).show(ui, |ui| {
                for row in &view.rows { victory_row(ui, row); ui.add_space(8.); }
            });
    });
    scores(ui, right, &view.scores);
}

fn victory_row(ui: &mut egui::Ui, row: &VictoryRow) {
    let width = ui.available_width();
    let ink = if row.enabled { CREAM } else { MUTED };
    let state = if row.enabled { row.progress.as_str() } else { t!("classic.ui.screen-victory.disabled") };
    let progress = ui.painter().layout(state.to_owned(), FontId::proportional(14.), ink, width - 24.);
    let goal = ui.painter().layout(row.goal.clone(), FontId::proportional(12.), MUTED, width - 24.);
    let height = (62. + progress.size().y + goal.size().y).max(ROW_H);
    let (r, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    ui.painter().rect_filled(r, 5., RAISED);
    text(ui.painter(), r.min + vec2(12., 8.), row.kind, 20., if row.enabled { GOLD } else { MUTED });
    let at = r.min + vec2(12., 34.);
    let progress_height = progress.size().y;
    ui.painter().galley(at, progress, ink);
    ui.painter().galley(at + vec2(0., progress_height + 4.), goal, MUTED);
    let bar = Rect::from_min_size(r.left_bottom() + vec2(12., -14.), vec2(width - 24., 5.));
    ui.painter().rect_filled(bar, 2., BG);
    if row.enabled && row.show_progress {
        let share = row.have.min(row.need) as f32 / row.need.max(1) as f32;
        ui.painter().rect_filled(Rect::from_min_size(bar.min, vec2(bar.width() * share, bar.height())), 2., PROGRESS);
    }
}

fn scores(ui: &mut egui::Ui, r: Rect, rows: &[ScoreRow]) {
    panel(ui.painter(), r);
    heading(ui, r, t!("classic.ui.screen-victory.scores"));
    let body = Rect::from_min_max(r.min + vec2(16., 38.), r.max - vec2(16., 12.));
    ui.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
        ui.set_clip_rect(body);
        egui::ScrollArea::vertical().id_salt("victory_scores")
            .max_height(body.height()).auto_shrink([false, false]).show(ui, |ui| {
                for (i, row) in rows.iter().enumerate() {
                    let width = ui.available_width();
                    let ink = if row.mine { GOLD } else { CREAM };
                    let score = ui.painter().layout_no_wrap(row.score.to_string(), FontId::proportional(14.), ink);
                    let name = ui.painter().layout(tf!("classic.ui.screen-victory.score-row", rank = i + 1, name = row.name.as_str()), FontId::proportional(14.), ink, (width - score.size().x - 12.).max(40.));
                    let height = name.size().y.max(score.size().y) + 10.;
                    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
                    ui.painter().galley(rect.min, name, ink);
                    ui.painter().galley(pos2(rect.right() - score.size().x, rect.top()), score, ink);
                }
            });
    });
}

/// The results: title, who won how and when, the score, One more turn / Main menu, and
/// the Hall of Fame of earlier games (the shell's `game_end` screen).
pub(super) fn game_end(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, view: &GameEndView) {
    match game_end::show(ui, r, &results(view)) {
        Some(Choice::KeepPlaying) => {
            state.requests.push(GameRequest::ContinueAfterVictory);
            state.navigate(Screen::Map);
        }
        Some(Choice::Menu) => state.navigate(Screen::Menu),
        None => {}
    }
}

fn results(view: &GameEndView) -> Results {
    Results {
        verdict: if view.won { Verdict::Won } else { Verdict::Lost },
        title: view.title.into(),
        verdict_line: if view.won { t!("classic.ui.screen-victory.you-won") } else { t!("classic.ui.screen-victory.you-lost") }.into(),
        line: view.line.clone(),
        score: tf!("classic.ui.screen-victory.your-score", score = view.score),
        keep_playing_label: t!("classic.ui.screen-victory.one-more-turn").into(),
        keep_playing: if view.can_continue {
            Availability::Ready
        } else {
            Availability::Blocked(t!("classic.ui.screen-victory.fallen").into())
        },
        menu_label: t!("classic.ui.screen-victory.main-menu").into(),
        hall_of_fame: HallOfFame {
            heading: t!("classic.ui.screen-victory.hall-of-fame").into(),
            empty: t!("classic.ui.screen-victory.hall-empty").into(),
            games: view.hall_of_fame.clone(),
        },
    }
}
