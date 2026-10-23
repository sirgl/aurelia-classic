//! HUD widgets that exist only with a running game: economy, research, notices, end
//! turn and the end-of-game panel (the route is map feedback: `map_feedback`). All of them read `HudModel` and return clicks.
//! The research strip, the notices and the end-of-game modal are the shell's widgets.
use super::layout::{self, place, Anchor, Panel};
use super::{panel, text, Icon, GOLD, INK, LIGHT, MUTED};
use crate::ui::model::{Availability, Economy, GameRequest, HudModel, NoticeKind, Outcome, Rival};
use client::shell::widgets::cards::{Bar, CardLine, ImageCard, Picture};
use i18n::{t, tf};
use client::shell::widgets::research_strip;
use client::shell::widgets::game_end::{self, Announcement};
use client::shell::widgets::notices::{self, Action, Marker, Notice};
use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};

const FOOD: Color32 = Color32::from_rgb(137, 188, 83);
const SCIENCE: Color32 = Color32::from_rgb(100, 198, 239);
const CULTURE: Color32 = Color32::from_rgb(214, 124, 196);
const FAITH: Color32 = Color32::from_rgb(236, 226, 170);
const HAPPY: Color32 = Color32::from_rgb(232, 196, 92);
const UNHAPPY: Color32 = Color32::from_rgb(214, 96, 78);

/// One value of the top bar: the full text (icon label included), the compact text (numbers
/// only; the full text stays on hover), and which of the breakdown tips belongs to it
/// (`HudExtras::top_bar` lists them in the order gold, food, production, science, culture,
/// faith).
pub(super) struct EconomyValue {
    pub icon: Icon,
    pub text: String,
    pub compact: String,
    pub tint: Color32,
    pub tip: Option<usize>,
}

fn signed(n: i32) -> String {
    format!("{n:+}")
}

pub(super) fn economy_values(economy: &Economy) -> Vec<EconomyValue> {
    let value = |icon, text: String, compact: String, tint, tip| EconomyValue { icon, text, compact, tint, tip };
    let gold_delta = signed(economy.gold_per_turn);
    let science_delta = signed(economy.science_per_turn as i32);
    let culture_delta = signed(economy.culture_per_turn as i32);
    let faith_delta = signed(economy.faith_per_turn as i32);
    let happiness = signed(economy.happiness);
    let mut values = vec![
        value(
            Icon::Coin,
            tf!("classic.ui.hud-game.top-gold", gold = economy.gold, delta = gold_delta.as_str()),
            format!("{} ({gold_delta})", economy.gold),
            GOLD,
            Some(0),
        ),
        value(
            Icon::Apple,
            tf!("classic.ui.hud-game.top-food", food = economy.food),
            economy.food.to_string(),
            FOOD,
            Some(1),
        ),
        value(
            Icon::Hammer,
            tf!("classic.ui.hud-game.top-production", production = economy.production),
            economy.production.to_string(),
            GOLD,
            Some(2),
        ),
        value(
            Icon::Flask,
            tf!("classic.ui.hud-game.top-science", science = economy.science, delta = science_delta.as_str()),
            format!("{} ({science_delta})", economy.science),
            SCIENCE,
            Some(3),
        ),
        value(
            Icon::Note,
            tf!("classic.ui.hud-game.top-happiness", happiness = happiness.as_str()),
            happiness.clone(),
            if economy.happiness < 0 { UNHAPPY } else { HAPPY },
            None,
        ),
        value(Icon::Sun, economy.golden_age.clone(), compact_golden_age(&economy.golden_age), GOLD, None),
        value(
            Icon::Note,
            tf!("classic.ui.hud-game.top-culture", culture = economy.culture, delta = culture_delta.as_str()),
            format!("{} ({culture_delta})", economy.culture),
            CULTURE,
            Some(4),
        ),
        value(
            Icon::Sun,
            tf!("classic.ui.hud-game.top-faith", faith = economy.faith, delta = faith_delta.as_str()),
            format!("{} ({faith_delta})", economy.faith),
            FAITH,
            Some(5),
        ),
    ];
    if !economy.resources.is_empty() {
        values.push(value(Icon::Shield, economy.resources.clone(), economy.resources.clone(), LIGHT, None));
    }
    values
}

/// The golden-age text without its label: from the first digit on ("Golden age 3/10" ->
/// "3/10"), whatever the language puts before the numbers.
fn compact_golden_age(text: &str) -> String {
    match text.find(|c: char| c.is_ascii_digit()) {
        Some(at) => text[at..].to_owned(),
        None => text.to_owned(),
    }
}

/// Current research under the top bar; a click opens the research screen.
pub(super) fn research(ctx: &egui::Context, game: &HudModel) -> bool {
    let mut clicked = false;
    let home = Rect::from_min_size(pos2(0.0, 59.0), vec2(249.0, 87.0));
    let Some(bounds) = place(ctx, Panel::at("game_research", Anchor::TopLeft, home).priority(60).collapsible()) else { return false };
    egui::Area::new("game_research".into())
        .fixed_pos(bounds.min)
        .show(ctx, |ui| {
            clicked = research_strip::research_strip(ui, bounds, &research_card(game), t!("classic.ui.hud-game.open-research"), false);
        });
    clicked
}

fn research_card(game: &HudModel) -> ImageCard {
    let (name, detail, share) = research_lines(game);
    ImageCard {
        picture: Picture::Icon(Icon::Flask, SCIENCE),
        lines: vec![CardLine::new(t!("classic.ui.hud-game.research-kicker"), MUTED), CardLine::new(name, LIGHT), CardLine::new(detail, MUTED)],
        bar: Some(Bar { share, color: SCIENCE }),
    }
}

fn research_lines(game: &HudModel) -> (String, String, f32) {
    let Some(research) = &game.research else {
        return (
            t!("classic.ui.hud-game.research-choose").into(),
            t!("classic.ui.hud-game.research-idle").into(),
            0.0,
        );
    };
    let share = research.stored as f32 / research.cost.max(1) as f32;
    let detail = match research.turns {
        Some(n) => tf!("classic.ui.hud-game.research-detail", stored = research.stored, cost = research.cost, n = n),
        None => tf!("classic.ui.hud-game.research-detail-stalled", stored = research.stored, cost = research.cost),
    };
    (research.name.clone(), detail, share.min(1.0))
}

/// Known nations with peace and open-borders buttons (under the notices).
pub(super) fn diplomacy(ctx: &egui::Context, screen: Rect, game: &HudModel) -> Vec<GameRequest> {
    let mut requests = Vec::new();
    if game.rivals.is_empty() || screen.width() < 800. {
        return requests;
    }
    // The frame sizes itself to the rivals: its size is the last frame's.
    let size = layout::last_size(ctx, "game_diplomacy").unwrap_or(vec2(302.0, 22.0 + game.rivals.len() as f32 * 22.0));
    let home = Rect::from_min_size(pos2(screen.right() - 316.0, 96.0), size);
    let Some(r) = place(ctx, Panel::at("game_diplomacy", Anchor::TopRight, home).priority(50).collapsible()) else { return requests };
    egui::Area::new("game_diplomacy".into())
        .fixed_pos(r.min)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(INK)
                .stroke(Stroke::new(1.0, GOLD.gamma_multiply(0.8)))
                .inner_margin(egui::Margin::same(10))
                .show(ui, |ui| {
                    ui.set_width(280.0);
                    for rival in &game.rivals {
                        rival_row(ui, rival, &mut requests);
                    }
                });
        });
    requests
}

fn rival_row(ui: &mut egui::Ui, rival: &Rival, requests: &mut Vec<GameRequest>) {
    let mut reasons = Vec::new();
    ui.horizontal(|ui| {
        let color = if rival.at_war {
            Color32::from_rgb(214, 96, 84)
        } else {
            LIGHT
        };
        let line = if rival.at_war {
            tf!("classic.ui.hud-game.rival-war", name = rival.name.as_str())
        } else {
            tf!("classic.ui.hud-game.rival-peace", name = rival.name.as_str())
        };
        ui.label(egui::RichText::new(line).color(color).size(12.0));
        if rival.at_war && small_button(ui, t!("classic.ui.hud-game.make-peace"), &rival.peace, &mut reasons) {
            requests.push(GameRequest::MakePeace(rival.player));
        }
        if !rival.borders_open && small_button(ui, t!("classic.ui.hud-game.open-borders"), &rival.open_borders, &mut reasons) {
            requests.push(GameRequest::OpenBorders(rival.player));
        }
    });
    for reason in reasons {
        ui.label(egui::RichText::new(reason).color(Color32::from_rgb(225, 151, 128)).size(10.0));
    }
}

/// A small button; when the rules block it, its reason is collected to stand under the row.
fn small_button(ui: &mut egui::Ui, label: &str, availability: &Availability, reasons: &mut Vec<String>) -> bool {
    let response = ui.add_enabled(availability.is_ready(), egui::Button::new(label).small());
    match availability {
        Availability::Blocked(reason) => {
            reasons.push(tf!("classic.ui.hud-game.blocked-reason", action = label, reason = reason.as_str()));
            response.on_disabled_hover_text(reason).clicked()
        }
        Availability::Ready => response.clicked(),
    }
}

/// Notices of the last turns; returns the map tile of a clicked notice.
pub(super) fn notices(ctx: &egui::Context, screen: Rect, game: &HudModel) -> Option<u32> {
    if let Some(message) = &game.message {
        let size = layout::last_size(ctx, "game_message").unwrap_or(vec2(300.0, 34.0));
        let home = Rect::from_min_size(pos2(screen.center().x - size.x / 2.0, screen.top() + 52.0), size);
        let r = place(ctx, Panel::at("game_message", Anchor::Top, home).priority(95));
        egui::Area::new("game_message".into())
            .order(egui::Order::Foreground)
            .fixed_pos(r.map_or(home.min, |r| r.min))
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(INK)
                    .stroke(Stroke::new(1.0, GOLD))
                    .inner_margin(egui::Margin::symmetric(14, 8))
                    .show(ui, |ui| ui.label(egui::RichText::new(message).color(LIGHT)));
            });
    }
    if game.notices.is_empty() || screen.width() < 800. {
        return None;
    }
    let rows: Vec<Notice> = game.notices.iter().take(6).map(|notice| Notice {
        marker: Marker::Dot(notice_color(notice.kind)),
        text: tf!("classic.ui.hud-game.notice-line", turn = notice.turn, text = notice.text.as_str()),
        action: match notice.tile {
            Some(_) => Action::Row { hint: t!("classic.ui.hud-game.notice-hint").into() },
            None => Action::None,
        },
    }).collect();
    let mut clicked = None;
    let home = Rect::from_min_size(pos2(screen.right() - 316.0, 96.0), vec2(300.0, notices::height(rows.len())));
    let r = place(ctx, Panel::at("game_notices", Anchor::TopRight, home).priority(70).collapsible())?;
    egui::Area::new("game_notices".into())
        .fixed_pos(r.min)
        .show(ctx, |ui| clicked = notices::show(ui, 300.0, &rows).and_then(|i| game.notices[i].tile));
    clicked
}

fn notice_color(kind: NoticeKind) -> Color32 {
    match kind {
        NoticeKind::Growth => FOOD,
        NoticeKind::Production => GOLD,
        NoticeKind::Research => SCIENCE,
        NoticeKind::Combat => Color32::from_rgb(214, 96, 84),
        NoticeKind::Diplomacy => Color32::from_rgb(186, 136, 216),
        NoticeKind::Culture => CULTURE,
        NoticeKind::Faith => FAITH,
    }
}

/// The captured city's fate: a modal with Annex, Puppet, Raze and Liberate.
pub(super) fn capture(ctx: &egui::Context, screen: Rect, game: &HudModel) -> Vec<GameRequest> {
    let mut requests = Vec::new();
    let Some(choice) = &game.capture else {
        return requests;
    };
    game_end::backdrop(ctx, screen, "capture_backdrop");
    let size = vec2(520.0_f32.min(screen.width() - 24.), 104.0 + choice.options.len() as f32 * 52.0);
    // Above the city labels and unit badges (foreground layers).
    egui::Area::new("game_capture".into())
        .order(egui::Order::Tooltip)
        .fixed_pos(screen.center() - size / 2.0)
        .show(ctx, |ui| {
            let (r, _) = ui.allocate_exact_size(size, Sense::hover());
            ui.painter().rect_filled(r, 8., Color32::from_rgb(10, 20, 29));
            panel(ui.painter(), r, 8.0);
            let title = tf!("classic.ui.hud-game.capture-title", city = choice.city.as_str());
            text(ui.painter(), r.min + vec2(22.0, 18.0), Align2::LEFT_TOP, title, 24.0, GOLD);
            let line = tf!("classic.ui.hud-game.capture-line", founder = choice.founder.as_str());
            text(ui.painter(), r.min + vec2(22.0, 56.0), Align2::LEFT_TOP, line, 13.0, LIGHT);
            for (i, option) in choice.options.iter().enumerate() {
                let row = Rect::from_min_size(
                    r.min + vec2(18.0, 90.0 + i as f32 * 52.0),
                    vec2(r.width() - 36.0, 44.0),
                );
                if capture_row(ui, row, option) {
                    requests.push(GameRequest::DecideCity {
                        tile: choice.tile,
                        decision: option.decision,
                    });
                }
            }
        });
    requests
}

fn capture_row(ui: &mut egui::Ui, r: Rect, option: &crate::ui::model::CaptureOption) -> bool {
    choice_row(ui, r, option.label, option.hint, &option.availability, t!("classic.ui.hud-game.unavailable"))
}

/// A button on the left, its explanation (or why it is unavailable) on the right; true on a click.
fn choice_row(ui: &mut egui::Ui, r: Rect, label: &'static str, hint: &str, availability: &Availability, unavailable: &str) -> bool {
    let ready = availability.is_ready();
    let button = Rect::from_min_size(r.min, vec2(118.0, r.height()));
    let sense = if ready { Sense::click() } else { Sense::hover() };
    let response = ui.interact(button, egui::Id::new(("capture", label)), sense);
    let fill = if ready && response.hovered() {
        Color32::from_rgb(40, 77, 97)
    } else {
        Color32::from_rgb(18, 34, 44)
    };
    ui.painter().rect_filled(button, 4.0, fill);
    let stroke = Stroke::new(1.0, if ready { GOLD } else { MUTED.gamma_multiply(0.4) });
    ui.painter().rect_stroke(button, 4.0, stroke, egui::StrokeKind::Inside);
    let ink = if ready { LIGHT } else { MUTED.gamma_multiply(0.6) };
    text(ui.painter(), button.center(), Align2::CENTER_CENTER, label, 15.0, ink);
    let hint = match availability {
        Availability::Ready => hint.to_string(),
        Availability::Blocked(reason) => format!("{unavailable}: {reason}"),
    };
    let galley = ui.painter().layout(
        hint,
        egui::FontId::proportional(12.0),
        MUTED,
        r.width() - button.width() - 16.0,
    );
    let at = pos2(button.right() + 14.0, r.center().y - galley.size().y / 2.0);
    ui.painter().galley(at, galley, MUTED);
    response.clicked() && ready
}

/// Ancient Ruins under the player's unit: the three rewards and "later". No backdrop: the
/// player may keep playing with the card open.
pub(super) fn ruins(ctx: &egui::Context, screen: Rect, game: &HudModel) -> Vec<GameRequest> {
    let mut requests = Vec::new();
    let Some(choice) = &game.ruins else {
        return requests;
    };
    let rows = choice.options.len() as f32;
    let size = vec2(520.0_f32.min(screen.width() - 24.), 128.0 + rows * 52.0);
    egui::Area::new("game_ruins".into())
        .order(egui::Order::Tooltip)
        // Left of the centre, where the camera puts the unit: the player sees it while choosing.
        .fixed_pos(pos2(screen.left() + 24.0, screen.top() + 170.0))
        .show(ctx, |ui| {
            let (r, _) = ui.allocate_exact_size(size, Sense::hover());
            ui.painter().rect_filled(r, 8., Color32::from_rgb(10, 20, 29));
            panel(ui.painter(), r, 8.0);
            text(ui.painter(), r.min + vec2(22.0, 18.0), Align2::LEFT_TOP, t!("classic.ui.hud-game.ruins-title"), 24.0, GOLD);
            let line = t!("classic.ui.hud-game.ruins-line");
            text(ui.painter(), r.min + vec2(22.0, 56.0), Align2::LEFT_TOP, line, 13.0, LIGHT);
            for (i, option) in choice.options.iter().enumerate() {
                let row = Rect::from_min_size(r.min + vec2(18.0, 90.0 + i as f32 * 52.0), vec2(r.width() - 36.0, 44.0));
                if choice_row(ui, row, option.label, &option.hint, &option.availability, t!("classic.ui.hud-game.unavailable")) {
                    requests.push(GameRequest::ClaimRuins(option.reward));
                }
            }
            let later = Rect::from_min_size(
                r.min + vec2(18.0, 98.0 + rows * 52.0),
                vec2(r.width() - 36.0, 22.0),
            );
            let (later_label, later_hint) = (t!("classic.ui.hud-game.ruins-later"), t!("classic.ui.hud-game.ruins-later-hint"));
            if choice_row(ui, later, later_label, later_hint, &Availability::Ready, t!("classic.ui.hud-game.unavailable")) {
                requests.push(GameRequest::PostponeRuins);
            }
        });
    requests
}

/// Victory or defeat panel; returns true when the player asks for the results.
pub(super) fn outcome(ctx: &egui::Context, screen: Rect, game: &HudModel) -> bool {
    let (title, line) = match game.outcome {
        Outcome::Playing => return false,
        Outcome::Victory => (t!("classic.ui.hud-game.victory"), t!("classic.ui.hud-game.victory-line")),
        Outcome::Defeat => (t!("classic.ui.hud-game.defeat"), t!("classic.ui.hud-game.defeat-line")),
    };
    let announcement = Announcement { title: title.into(), line: line.into(), button: t!("classic.ui.hud-game.see-results").into() };
    game_end::announce(ctx, screen, &announcement)
}
