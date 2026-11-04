//! World Congress screen bound to the running game (`CongressView`): host, session timer,
//! the delegates of every member, the World Leader requirement, resolutions in force,
//! the proposals of the session with vote buttons (one delegate or all that are left),
//! and the resolutions the player may propose or repeal.
use super::scroll::scroll_panel;
use super::*;
use i18n::{t, tf};
use crate::ui::model::{ChoiceButton, CongressView, GameRequest, ProposalCard, ProposeRow};

const SIDE_W: f32 = 290.0;
const PROPOSE_W: f32 = 360.0;

pub(super) fn congress(
    ui: &mut egui::Ui,
    r: Rect,
    state: &mut ScreenState,
    view: &CongressView,
    message: Option<&str>,
) {
    let top = Rect::from_min_size(r.min, vec2(r.width(), 92.0));
    let body = Rect::from_min_max(pos2(r.left(), top.bottom() + 14.0), r.max);
    let Some(title) = view.title else {
        world_banner(ui, top, t!("classic.ui.screen-congress.title"), t!("classic.ui.screen-congress.not-founded"), message);
        unfounded(ui, body, view);
        return;
    };
    let line = tf!("classic.ui.screen-congress.banner-line",
        host = view.host.as_str(),
        session = view.session.as_str(),
        left = view.delegates_left,
        needed = view.needed
    );
    world_banner(ui, top, title, &line, message);
    let left = Rect::from_min_size(body.min, vec2(SIDE_W, body.height()));
    let right = Rect::from_min_max(pos2(body.right() - PROPOSE_W, body.top()), body.max);
    let center = Rect::from_min_max(
        pos2(left.right() + 14.0, body.top()),
        pos2(right.left() - 14.0, body.bottom()),
    );
    members(ui, left, view);
    proposals(ui, center, state, view);
    propose(ui, right, state, view);
}

fn unfounded(ui: &mut egui::Ui, r: Rect, view: &CongressView) {
    panel(ui.painter(), r);
    let at = r.min + vec2(18.0, 18.0);
    paragraph(
        ui.painter(),
        at,
        &view.session,
        14.0,
        CREAM,
        r.width() - 36.0,
    );
    let leader = tf!("classic.ui.screen-congress.leader-needs", needed = view.needed);
    paragraph(
        ui.painter(),
        at + vec2(0.0, 32.0),
        &leader,
        13.0,
        MUTED,
        r.width() - 36.0,
    );
}

/// Delegates of every member, the World Leader line, and the resolutions in force.
fn members(ui: &mut egui::Ui, r: Rect, view: &CongressView) {
    panel(ui.painter(), r);
    heading(ui, r, t!("classic.ui.screen-congress.delegates"));
    let list = Rect::from_min_max(r.min + vec2(16.0, 36.0), r.max - vec2(16.0, 8.0));
    super::scroll::scroll_body(ui, list, "congress_members", |ui| {
        for (name, count) in &view.delegates {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(name).size(14.0).color(CREAM));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(count.to_string())
                            .size(14.0)
                            .color(GOLD),
                    );
                });
            });
        }
        ui.add_space(12.0);
        ui.add(
            egui::Label::new(
                egui::RichText::new(&view.world_leader)
                    .size(13.0)
                    .color(GOLD),
            )
            .wrap(),
        );
        ui.add_space(12.0);
        ui.label(egui::RichText::new(t!("classic.ui.screen-congress.in-force")).size(10.0).color(GOLD));
        if view.active.is_empty() {
            ui.label(
                egui::RichText::new(t!("classic.ui.screen-congress.no-resolution"))
                    .size(12.0)
                    .color(MUTED),
            );
        }
        for line in &view.active {
            ui.add(egui::Label::new(egui::RichText::new(line).size(12.0).color(CREAM)).wrap());
        }
    });
}

fn proposals(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, view: &CongressView) {
    panel(ui.painter(), r);
    let title = if view.in_session {
        t!("classic.ui.screen-congress.vote")
    } else {
        t!("classic.ui.screen-congress.proposals-next")
    };
    heading(ui, r, title);
    if view.proposals.is_empty() {
        let at = r.min + vec2(16.0, 36.0);
        paragraph(
            ui.painter(),
            at,
            t!("classic.ui.screen-congress.no-proposals"),
            13.0,
            MUTED,
            r.width() - 32.0,
        );
        return;
    }
    let list = Rect::from_min_max(pos2(r.left(), r.top() + 36.0), r.max - vec2(0.0, 8.0));
    scroll_panel(
        ui,
        list,
        "proposals_screen_congress.rs",
        view.proposals
            .iter()
            .map(|c| 60.0 + c.choices.len() as f32 * 34.0)
            .sum(),
        |ui, r| {
            let mut y = r.top();
            for card in &view.proposals {
                let h = 52.0 + card.choices.len() as f32 * 34.0;
                let rect = Rect::from_min_size(pos2(r.left() + 10.0, y), vec2(r.width() - 20.0, h));
                proposal(ui, rect, state, card, view.delegates_left);
                y += h + 8.0;
            }
        },
    );
}

fn proposal(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, card: &ProposalCard, left: u16) {
    if !ui.is_rect_visible(r) {
        return;
    }
    ui.painter().rect_filled(r, 5.0, RAISED);
    text(
        ui.painter(),
        r.min + vec2(10.0, 8.0),
        &card.title,
        15.0,
        CREAM,
    );
    let by = tf!("classic.ui.screen-congress.proposed-by", name = card.proposer.as_str());
    text(ui.painter(), r.min + vec2(10.0, 28.0), by, 11.0, MUTED);
    for (i, choice) in card.choices.iter().enumerate() {
        let row = Rect::from_min_size(
            r.min + vec2(10.0, 50.0 + i as f32 * 34.0),
            vec2(r.width() - 20.0, 30.0),
        );
        if let Some(delegates) = vote_row(ui, row, choice, left) {
            state.requests.push(GameRequest::CastVotes {
                proposal: card.index,
                choice: choice.choice,
                delegates,
            });
        }
    }
}

/// A choice with "+1" and "All" buttons; returns the delegates to cast.
fn vote_row(ui: &mut egui::Ui, r: Rect, choice: &ChoiceButton, left: u16) -> Option<u16> {
    text(
        ui.painter(),
        r.min + vec2(0.0, 7.0),
        &choice.label,
        13.0,
        CREAM,
    );
    let all = Rect::from_min_size(pos2(r.right() - 90.0, r.top()), vec2(90.0, r.height()));
    let one = all.translate(vec2(-98.0, 0.0));
    if ready_button(ui, one, "+1", &choice.availability) {
        return Some(1);
    }
    let label = tf!("classic.ui.screen-congress.all-votes", left = left);
    if ready_button(ui, all, &label, &choice.availability) {
        return Some(left);
    }
    None
}

fn propose(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, view: &CongressView) {
    panel(ui.painter(), r);
    let title = if view.can_propose {
        t!("classic.ui.screen-congress.propose")
    } else {
        t!("classic.ui.screen-congress.resolutions")
    };
    heading(ui, r, title);
    let list = Rect::from_min_max(r.min + vec2(10.0, 34.0), r.max - vec2(10.0, 10.0));
    ui.scope_builder(egui::UiBuilder::new().max_rect(list), |ui| {
        egui::ScrollArea::vertical()
            .id_salt("congress_propose")
            .show(ui, |ui| {
                let heights: Vec<f32> = view.propose.iter().map(row_height).collect();
                let total: f32 = heights.iter().map(|h| h + 8.0).sum();
                let (area, _) =
                    ui.allocate_exact_size(vec2(list.width() - 12.0, total), Sense::hover());
                let mut y = area.top();
                for (row, h) in view.propose.iter().zip(heights) {
                    let rect = Rect::from_min_size(pos2(area.left(), y), vec2(area.width(), h));
                    propose_row(ui, rect, state, row);
                    y += h + 8.0;
                }
            });
    });
}

fn row_height(row: &ProposeRow) -> f32 {
    48.0 + row.choices.len().div_ceil(2) as f32 * 32.0
}

fn propose_row(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, row: &ProposeRow) {
    ui.painter().rect_filled(r, 5.0, RAISED);
    text(
        ui.painter(),
        r.min + vec2(10.0, 7.0),
        &row.title,
        13.0,
        CREAM,
    );
    text(
        ui.painter(),
        r.min + vec2(10.0, 26.0),
        row.text,
        10.0,
        MUTED,
    );
    let width = (r.width() - 28.0) / 2.0;
    for (i, choice) in row.choices.iter().enumerate() {
        let at = r.min
            + vec2(
                10.0 + (i % 2) as f32 * (width + 8.0),
                44.0 + (i / 2) as f32 * 32.0,
            );
        let button_rect = Rect::from_min_size(at, vec2(width, 28.0));
        if ready_button(ui, button_rect, &choice.label, &choice.availability) {
            state.requests.push(GameRequest::ProposeResolution {
                kind: row.kind,
                choice: choice.choice,
                repeal: row.repeal,
            });
        }
    }
}
