//! Civics screen bound to the running game (`CivicsView`): the nine policy branches and the
//! three ideologies with their real costs and states, the selected branch's tree, and the
//! selected policy's details. Same frame, panels and policy artwork as the showcase gallery;
//! every button is a typed `GameRequest` the core validates.
use super::*;
use i18n::{t, tf};
use crate::ui::model::{
    Availability, BranchCard, BranchState, CivicsView, GameRequest, PolicyCard,
};

const LIST_W: f32 = 250.0;
const SIDE_W: f32 = 300.0;
const NODE: egui::Vec2 = egui::Vec2::new(150.0, 104.0);
const CULTURE: Color32 = Color32::from_rgb(214, 124, 196);

pub(super) fn civics(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, view: &CivicsView) {
    let top = Rect::from_min_size(r.min, vec2(r.width(), 96.0));
    banner(ui, top, state, view);
    let body = Rect::from_min_max(pos2(r.left(), top.bottom() + 14.0), r.max);
    let list = Rect::from_min_size(body.min, vec2(LIST_W, body.height()));
    let side = Rect::from_min_max(pos2(body.right() - SIDE_W, body.top()), body.max);
    let tree = Rect::from_min_max(
        pos2(list.right() + 14.0, body.top()),
        pos2(side.left() - 14.0, body.bottom()),
    );
    let branch = selected_branch(state, view);
    branch_list(ui, list, state, view, branch);
    let Some(card) = view.branches.get(branch) else {
        return;
    };
    branch_tree(ui, tree, state, card);
    let policy = selected_policy(state, card);
    policy_side(ui, side, state, card, policy);
}

fn banner(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, view: &CivicsView) {
    art(state, ui, r, "civics");
    darken(ui.painter(), r, 70, 170);
    text(
        ui.painter(),
        r.min + vec2(24.0, 14.0),
        t!("classic.ui.screen-civics.title"),
        31.0,
        CREAM,
    );
    let line = match view.turns {
        Some(0) => tf!("classic.ui.screen-civics.status-ready", culture = view.culture, per_turn = view.per_turn, cost = view.next_cost),
        Some(n) => tf!("classic.ui.screen-civics.status-turns", culture = view.culture, per_turn = view.per_turn, cost = view.next_cost, n = n),
        None => tf!("classic.ui.screen-civics.status-stalled", culture = view.culture, per_turn = view.per_turn, cost = view.next_cost),
    };
    text(ui.painter(), r.min + vec2(26.0, 60.0), line, 14.0, CREAM);
    let mut free = Vec::new();
    if view.free_policies > 0 {
        free.push(tf!("classic.ui.screen-civics.free-policies", n = view.free_policies));
    }
    if view.free_tenets > 0 {
        free.push(tf!("classic.ui.screen-civics.free-tenets", n = view.free_tenets));
    }
    if !free.is_empty() {
        let chip = Rect::from_min_size(pos2(r.right() - 220.0, r.top() + 30.0), vec2(196.0, 34.0));
        ui.painter().rect_filled(chip, 17.0, TEAL);
        ui.painter().text(
            chip.center(),
            Align2::CENTER_CENTER,
            free.join(" · "),
            FontId::proportional(13.0),
            CREAM,
        );
    }
}

/// The branch the player looks at: the remembered choice, else the first open one, else the
/// first available one, else the first branch.
fn selected_branch(state: &ScreenState, view: &CivicsView) -> usize {
    if let Some(id) = state.civics_branch {
        if let Some(i) = view.branches.iter().position(|b| b.id == id) {
            return i;
        }
    }
    let by = |s: &BranchState| view.branches.iter().position(|b| b.state == *s);
    by(&BranchState::Open)
        .or_else(|| by(&BranchState::Available))
        .unwrap_or(0)
}

fn selected_policy<'a>(state: &ScreenState, card: &'a BranchCard) -> Option<&'a PolicyCard> {
    let all = || {
        card.opener
            .iter()
            .chain(card.policies.iter())
            .chain(card.finisher.iter())
    };
    if let Some(id) = state.civics_policy {
        if let Some(p) = all().find(|p| p.id == id) {
            return Some(p);
        }
    }
    card.policies
        .iter()
        .find(|p| p.availability.is_ready())
        .or(card.opener.as_ref())
        .or(card.policies.first())
}

fn branch_list(
    ui: &mut egui::Ui,
    r: Rect,
    state: &mut ScreenState,
    view: &CivicsView,
    selected: usize,
) {
    panel(ui.painter(), r);
    let mut y = r.top() + 12.0;
    let heading = |ui: &mut egui::Ui, y: &mut f32, label: &str| {
        text(ui.painter(), pos2(r.left() + 16.0, *y), label, 10.0, GOLD);
        *y += 20.0;
    };
    let row_h = ((r.height() - 64.0) / view.branches.len().max(1) as f32).min(44.0);
    let mut ideology_heading = false;
    heading(ui, &mut y, t!("classic.ui.screen-civics.branches"));
    for (i, branch) in view.branches.iter().enumerate() {
        if branch.ideology && !ideology_heading {
            ideology_heading = true;
            y += 6.0;
            heading(ui, &mut y, t!("classic.ui.screen-civics.ideologies"));
        }
        let row = Rect::from_min_size(pos2(r.left() + 8.0, y), vec2(r.width() - 16.0, row_h - 4.0));
        if branch_row(ui, row, state, branch, i == selected) {
            state.civics_branch = Some(branch.id);
            state.civics_policy = None;
        }
        y += row_h;
    }
}

fn branch_row(
    ui: &mut egui::Ui,
    row: Rect,
    state: &mut ScreenState,
    branch: &BranchCard,
    selected: bool,
) -> bool {
    let response = ui.interact(
        row,
        ui.id().with(("civics_branch", branch.id.0)),
        Sense::click(),
    );
    crate::ui::pedia_link::mark(ui, row, crate::ui::pedia::PediaRef::branch(branch.id));
    let fill = if selected {
        Color32::from_rgb(37, 65, 66)
    } else if response.hovered() {
        RAISED
    } else {
        PANEL
    };
    ui.painter().rect_filled(row, 4.0, fill);
    let badge = Rect::from_center_size(pos2(row.left() + 20.0, row.center().y), vec2(32.0, 32.0));
    emblem(
        ui,
        badge,
        state,
        &branch.art,
        branch.state == BranchState::Open,
    );
    let (status, color) = branch_status(branch);
    text(
        ui.painter(),
        pos2(row.left() + 44.0, row.top() + 3.0),
        &branch.name,
        15.0,
        CREAM,
    );
    text(
        ui.painter(),
        pos2(row.left() + 44.0, row.top() + 21.0),
        status,
        10.0,
        color,
    );
    response.clicked()
}

/// Short state line of a branch: progress when open, the reason when locked.
fn branch_status(branch: &BranchCard) -> (String, Color32) {
    let adopted = branch.policies.iter().filter(|p| p.adopted).count();
    match &branch.state {
        BranchState::Open if branch.ideology => (tf!("classic.ui.screen-civics.status-adopted-tenets", n = adopted), GOLD),
        BranchState::Open => (
            tf!("classic.ui.screen-civics.status-policies", adopted = adopted, total = branch.policies.len()),
            GOLD,
        ),
        BranchState::Available if branch.ideology => (t!("classic.ui.screen-civics.can-be-chosen").to_owned(), CULTURE),
        BranchState::Available => (t!("classic.ui.screen-civics.can-be-unlocked").to_owned(), CULTURE),
        BranchState::Locked(reason) => match &branch.era {
            Some(era) if reason == crate::game::reasons::branch(classic_rules::derived::culture::BranchBlock::EraTooEarly) => {
                (tf!("classic.ui.screen-civics.from-era", era = era.to_uppercase()), MUTED)
            }
            _ => (reason.to_uppercase(), MUTED),
        },
    }
}

fn emblem(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, art_id: &str, bright: bool) {
    let painter = ui.painter().clone();
    painter.circle_filled(r.center(), r.width() * 0.5, INK_DARK);
    match state.policy_art.texture_of(ui.ctx(), art_id) {
        Some(texture) => {
            painter.image(
                texture,
                r,
                Rect::from_min_max(pos2(0., 0.), pos2(1., 1.)),
                Color32::from_gray(if bright { 255 } else { 170 }),
            );
        }
        None => {
            painter.circle_stroke(
                r.center(),
                r.width() * 0.42,
                Stroke::new(1.0, GOLD.gamma_multiply(0.5)),
            );
        }
    }
    painter.circle_stroke(
        r.center(),
        r.width() * 0.5,
        Stroke::new(
            1.2,
            if bright {
                GOLD
            } else {
                Color32::from_rgb(70, 86, 84)
            },
        ),
    );
}

const INK_DARK: Color32 = Color32::from_rgb(12, 26, 32);

fn branch_tree(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, branch: &BranchCard) {
    panel(ui.painter(), r);
    let header = Rect::from_min_size(r.min, vec2(r.width(), 92.0));
    branch_header(ui, header, state, branch);
    let area = Rect::from_min_max(
        pos2(r.left() + 12.0, header.bottom() + 6.0),
        r.max - vec2(12.0, 12.0),
    );
    if branch.ideology {
        tenet_rows(ui, area, state, branch);
    } else {
        policy_grid(ui, area, state, branch);
    }
}

fn branch_header(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, branch: &BranchCard) {
    let badge = Rect::from_center_size(pos2(r.left() + 52.0, r.center().y), vec2(66.0, 66.0));
    let locked = matches!(branch.state, BranchState::Locked(_));
    emblem(ui, badge, state, &branch.art, !locked);
    text(
        ui.painter(),
        pos2(r.left() + 98.0, r.top() + 18.0),
        &branch.name,
        26.0,
        CREAM,
    );
    let (status, color) = branch_status(branch);
    text(
        ui.painter(),
        pos2(r.left() + 99.0, r.top() + 54.0),
        status,
        11.0,
        color,
    );
    let button_rect =
        Rect::from_min_size(pos2(r.right() - 190.0, r.top() + 26.0), vec2(170.0, 38.0));
    let (label, availability) = match (&branch.state, branch.ideology) {
        (BranchState::Open, true) => (
            t!("classic.ui.screen-civics.your-ideology").to_owned(),
            Availability::Blocked(t!("classic.ui.screen-civics.chosen").into()),
        ),
        (BranchState::Open, false) => (
            t!("classic.ui.screen-civics.branch-unlocked").to_owned(),
            Availability::Blocked(t!("classic.ui.screen-civics.unlocked").into()),
        ),
        (BranchState::Available, true) => {
            let label = if branch.free_tenets > 0 {
                tf!("classic.ui.screen-civics.choose-ideology-free", n = branch.free_tenets)
            } else {
                t!("classic.ui.screen-civics.choose-ideology").to_owned()
            };
            (label, Availability::Ready)
        }
        (BranchState::Available, false) => (t!("classic.ui.screen-civics.unlock-branch").to_owned(), Availability::Ready),
        (BranchState::Locked(reason), _) => {
            (t!("classic.ui.screen-civics.locked").to_owned(), Availability::Blocked(reason.clone()))
        }
    };
    let enabled = availability.is_ready();
    let note = match &branch.state {
        BranchState::Locked(reason) => Some(reason.as_str()),
        _ => None,
    };
    let clicked = client::shell::kit::note_button(ui, button_rect, &label, false, enabled, note);
    if let Availability::Blocked(reason) = &availability {
        ui.interact(
            button_rect,
            ui.id().with(("civics_lock", branch.id.0)),
            Sense::hover(),
        )
        .on_hover_text(reason);
    }
    if clicked {
        let request = if branch.ideology {
            GameRequest::ChooseIdeology(branch.id)
        } else {
            GameRequest::UnlockBranch(branch.id)
        };
        state.requests.push(request);
    }
}

/// Policies of a normal branch on their (column, row) grid, prerequisite lines between them,
/// the finisher as a wide card under the tree.
fn policy_grid(ui: &mut egui::Ui, area: Rect, state: &mut ScreenState, branch: &BranchCard) {
    let columns = branch
        .policies
        .iter()
        .map(|p| p.grid.0)
        .max()
        .unwrap_or(1)
        .max(1) as f32;
    let rows = branch
        .policies
        .iter()
        .map(|p| p.grid.1)
        .max()
        .unwrap_or(1)
        .max(1) as f32;
    let finisher_h = 54.0;
    let grid = Rect::from_min_max(
        area.min,
        pos2(area.right(), area.bottom() - finisher_h - 10.0),
    );
    let step_x = (grid.width() - NODE.x) / (columns - 1.0).max(1.0);
    let step_y = (grid.height() - NODE.y) / (rows - 1.0).max(1.0);
    let place = |p: &PolicyCard| {
        let x = grid.left() + (f32::from(p.grid.0.max(1)) - 1.0) * step_x;
        let y = grid.top() + (f32::from(p.grid.1.max(1)) - 1.0) * step_y.min(NODE.y + 34.0);
        Rect::from_min_size(pos2(x, y), NODE)
    };
    for p in &branch.policies {
        for parent in &p.requires {
            let Some(from) = branch.policies.get(*parent) else {
                continue;
            };
            let (a, b) = (place(from).center_bottom(), place(p).center_top());
            let mid = (a.y + b.y) * 0.5;
            let color = if from.adopted {
                GOLD
            } else {
                GOLD.gamma_multiply(0.35)
            };
            ui.painter().add(egui::Shape::line(
                vec![a, pos2(a.x, mid), pos2(b.x, mid), b],
                Stroke::new(2.0, color),
            ));
        }
    }
    for p in &branch.policies {
        policy_node(ui, place(p), state, p);
    }
    if let Some(finisher) = &branch.finisher {
        let wide = Rect::from_min_size(
            pos2(area.left(), area.bottom() - finisher_h),
            vec2(area.width(), finisher_h),
        );
        finisher_card(ui, wide, state, finisher);
    }
}

/// Ideology tenets in three tiers.
fn tenet_rows(ui: &mut egui::Ui, area: Rect, state: &mut ScreenState, branch: &BranchCard) {
    let row_h = area.height() / 3.0;
    for tier in 1..=3u8 {
        let top = area.top() + f32::from(tier - 1) * row_h;
        text(
            ui.painter(),
            pos2(area.left(), top + 2.0),
            tf!("classic.ui.screen-civics.level", tier = tier),
            10.0,
            GOLD,
        );
        let tenets: Vec<&PolicyCard> = branch.policies.iter().filter(|p| p.tier == tier).collect();
        let count = tenets.len().max(1) as f32;
        let width = ((area.width() - 8.0 * (count - 1.0)) / count).min(NODE.x);
        for (i, p) in tenets.iter().enumerate() {
            let node = Rect::from_min_size(
                pos2(area.left() + i as f32 * (width + 8.0), top + 18.0),
                vec2(width, (row_h - 26.0).min(NODE.y)),
            );
            policy_node(ui, node, state, p);
        }
    }
}

fn policy_node(ui: &mut egui::Ui, node: Rect, state: &mut ScreenState, p: &PolicyCard) {
    let response = ui.interact(
        node,
        ui.id().with(("civics_policy", p.id.0)),
        Sense::click(),
    );
    crate::ui::pedia_link::mark(ui, node, crate::ui::pedia::PediaRef::policy(p.id));
    if response.clicked() {
        state.civics_policy = Some(p.id);
    }
    let chosen = state.civics_policy == Some(p.id);
    let fill = if chosen {
        Color32::from_rgb(37, 65, 66)
    } else if response.hovered() {
        Color32::from_rgb(31, 55, 58)
    } else {
        RAISED
    };
    ui.painter().rect_filled(node, 5.0, fill);
    let edge = if p.adopted || chosen {
        GOLD
    } else {
        Color32::from_rgb(60, 78, 76)
    };
    ui.painter().rect_stroke(
        node,
        5.0,
        Stroke::new(if chosen { 1.6 } else { 1.0 }, edge),
        egui::StrokeKind::Inside,
    );
    let size = (node.height() * 0.46).min(50.0);
    let badge = Rect::from_center_size(
        pos2(node.center().x, node.top() + 6.0 + size * 0.5),
        vec2(size, size),
    );
    emblem(
        ui,
        badge,
        state,
        &p.art,
        p.adopted || p.availability.is_ready(),
    );
    let galley = ui.painter().layout(
        p.name.clone(),
        FontId::proportional(13.0),
        if p.adopted || p.availability.is_ready() {
            CREAM
        } else {
            MUTED
        },
        node.width() - 10.0,
    );
    let name_y = badge.bottom() + 4.0;
    ui.painter().galley(
        pos2(node.center().x - galley.size().x * 0.5, name_y),
        galley,
        CREAM,
    );
    let (label, color) = policy_status(p);
    ui.painter().text(
        pos2(node.center().x, node.bottom() - 10.0),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(9.0),
        color,
    );
}

fn policy_status(p: &PolicyCard) -> (&'static str, Color32) {
    if p.adopted {
        (t!("classic.ui.screen-civics.adopted"), GOLD)
    } else if p.availability.is_ready() {
        (t!("classic.ui.screen-civics.available"), CULTURE)
    } else {
        (t!("classic.ui.screen-civics.locked-caps"), MUTED)
    }
}

fn finisher_card(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, p: &PolicyCard) {
    let response = ui.interact(r, ui.id().with(("civics_finisher", p.id.0)), Sense::click());
    crate::ui::pedia_link::mark(ui, r, crate::ui::pedia::PediaRef::policy(p.id));
    if response.clicked() {
        state.civics_policy = Some(p.id);
    }
    ui.painter().rect_filled(
        r,
        5.0,
        if p.adopted {
            Color32::from_rgb(44, 58, 42)
        } else {
            RAISED
        },
    );
    ui.painter().rect_stroke(
        r,
        5.0,
        Stroke::new(
            1.0,
            if p.adopted {
                GOLD
            } else {
                Color32::from_rgb(60, 78, 76)
            },
        ),
        egui::StrokeKind::Inside,
    );
    let title = if p.adopted {
        t!("classic.ui.screen-civics.branch-completed")
    } else {
        t!("classic.ui.screen-civics.complete-branch")
    };
    text(ui.painter(), r.min + vec2(14.0, 9.0), title, 10.0, GOLD);
    let width = r.width() - 28.0;
    let summary = completion_sentence(&p.text);
    paragraph(
        ui.painter(),
        r.min + vec2(14.0, 26.0),
        &summary,
        12.0,
        CREAM,
        width,
    );
}

/// The sentence of a branch text about completing it ("Adopting all Policies ..."); a text
/// in a language whose wording differs is shown whole.
fn completion_sentence(text: &str) -> String {
    for sentence in text.split_inclusive(". ") {
        if sentence.starts_with("Adopting all") { // i18n-ok: matches the rule text
            return sentence.trim().to_owned();
        }
    }
    text.to_owned()
}

fn policy_side(
    ui: &mut egui::Ui,
    r: Rect,
    state: &mut ScreenState,
    branch: &BranchCard,
    policy: Option<&PolicyCard>,
) {
    panel(ui.painter(), r);
    let Some(p) = policy else {
        return;
    };
    let badge = Rect::from_center_size(pos2(r.center().x, r.top() + 70.0), vec2(104.0, 104.0));
    emblem(ui, badge, state, &p.art, true);
    ui.painter().text(
        pos2(r.center().x, r.top() + 134.0),
        Align2::CENTER_TOP,
        &p.name,
        FontId::proportional(24.0),
        CREAM,
    );
    let role = if p.tier > 0 {
        tf!("classic.ui.screen-civics.role-tenet", branch = branch.name.to_uppercase(), tier = p.tier)
    } else if Some(p.id) == branch.opener.as_ref().map(|o| o.id) {
        tf!("classic.ui.screen-civics.role-opener", branch = branch.name.to_uppercase())
    } else if Some(p.id) == branch.finisher.as_ref().map(|o| o.id) {
        tf!("classic.ui.screen-civics.role-finisher", branch = branch.name.to_uppercase())
    } else {
        tf!("classic.ui.screen-civics.role-policy", branch = branch.name.to_uppercase())
    };
    ui.painter().text(
        pos2(r.center().x, r.top() + 168.0),
        Align2::CENTER_TOP,
        role,
        FontId::proportional(10.0),
        GOLD,
    );
    paragraph(
        ui.painter(),
        r.min + vec2(20.0, 196.0),
        &p.text,
        13.0,
        MUTED,
        r.width() - 40.0,
    );
    let action = Rect::from_min_size(
        pos2(r.left() + 20.0, r.bottom() - 62.0),
        vec2(r.width() - 40.0, 42.0),
    );
    let opener_or_finisher = branch.opener.as_ref().map(|o| o.id) == Some(p.id)
        || branch.finisher.as_ref().map(|o| o.id) == Some(p.id);
    let label = if p.adopted {
        t!("classic.ui.screen-civics.adopted-button").to_owned()
    } else if opener_or_finisher {
        t!("classic.ui.screen-civics.comes-with-branch").to_owned()
    } else {
        t!("classic.ui.screen-civics.adopt-policy").to_owned()
    };
    let enabled = !p.adopted && !opener_or_finisher && p.availability.is_ready();
    if button(ui, action, &label, false, enabled) {
        state.requests.push(GameRequest::AdoptPolicy(p.id));
    }
    if let Availability::Blocked(reason) = &p.availability {
        if !p.adopted && !opener_or_finisher {
            paragraph(
                ui.painter(),
                pos2(action.left(), action.top() - 24.0),
                reason,
                12.0,
                GOLD,
                action.width(),
            );
        }
    }
}

/// Art ids the screen shows now (offscreen captures decode them synchronously).
pub(super) fn visible_art(view: &CivicsView, state: &ScreenState) -> Vec<String> {
    let mut ids: Vec<String> = view.branches.iter().map(|b| b.art.clone()).collect();
    let branch = selected_branch(state, view);
    if let Some(card) = view.branches.get(branch) {
        ids.extend(card.policies.iter().map(|p| p.art.clone()));
    }
    ids
}
