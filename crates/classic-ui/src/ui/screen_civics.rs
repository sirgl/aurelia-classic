//! Civics screen bound to the running game (`CivicsView`). Three parts: the branches with what
//! each is for and how far it is, the selected branch's tree with every policy's effect and its
//! own adopt button, and the "Available now" list: everything that may be taken this turn, from
//! every branch, with its effect and one button. A phone-sized canvas shows the list and the
//! branches as two tabs. Every button is a typed `GameRequest` the core validates; a policy is
//! adopted with one click on the button that names its price.
use super::*;
use super::civics_focus::Zone;
use crate::ui::model::{
    BranchCard, BranchState, CivicsView, Cost, GameRequest, Offer, OfferKind, PolicyCard,
    PolicyState,
};
use i18n::{t, tf};

/// A canvas at least this wide shows the branches, the tree and the offers side by side.
const WIDE: f32 = 1150.0;
const CULTURE: Color32 = Color32::from_rgb(214, 124, 196);
const INK_DARK: Color32 = Color32::from_rgb(12, 26, 32);
const ADOPTED_FILL: Color32 = Color32::from_rgb(31, 58, 52);
const LOCKED_FILL: Color32 = Color32::from_rgb(19, 38, 44);
const EDGE: Color32 = Color32::from_rgb(60, 78, 76);
const NODE_MAX: egui::Vec2 = egui::Vec2::new(190.0, 120.0);
const GAP: f32 = 8.0;

/// The rectangles of one frame.
struct Frame {
    banner: Rect,
    tabs: Option<Rect>,
    list: Option<Rect>,
    tree: Option<Rect>,
    offers: Option<Rect>,
    /// Offer cards side by side.
    columns: usize,
}

fn frame(r: Rect, zone: Zone) -> Frame {
    let wide = r.width() >= WIDE;
    let banner_h = if r.height() >= 700.0 { 96.0 } else if wide { 72.0 } else { 58.0 };
    let banner = Rect::from_min_size(r.min, vec2(r.width(), banner_h));
    let mut body = Rect::from_min_max(pos2(r.left(), banner.bottom() + 10.0), r.max);
    if !wide {
        let tabs = Rect::from_min_size(body.min, vec2(body.width(), 28.0));
        body.min.y = tabs.bottom() + 6.0;
        return narrow(banner, tabs, body, zone);
    }
    let list_w = if r.width() >= 1400.0 { 300.0 } else { 272.0 };
    let side_w = if r.width() >= 1400.0 { 340.0 } else { 316.0 };
    let list = Rect::from_min_size(body.min, vec2(list_w, body.height()));
    let side = Rect::from_min_max(pos2(body.right() - side_w, body.top()), body.max);
    let tree = Rect::from_min_max(pos2(list.right() + 12.0, body.top()), pos2(side.left() - 12.0, body.bottom()));
    Frame { banner, tabs: None, list: Some(list), tree: Some(tree), offers: Some(side), columns: 1 }
}

/// The phone layout: one tab at a time.
fn narrow(banner: Rect, tabs: Rect, body: Rect, zone: Zone) -> Frame {
    if zone == Zone::Offers {
        let columns = ((body.width() / 330.0).floor() as usize).max(1);
        return Frame { banner, tabs: Some(tabs), list: None, tree: None, offers: Some(body), columns };
    }
    let list = Rect::from_min_size(body.min, vec2(206.0, body.height()));
    let tree = Rect::from_min_max(pos2(list.right() + 8.0, body.top()), body.max);
    Frame { banner, tabs: Some(tabs), list: Some(list), tree: Some(tree), offers: None, columns: 1 }
}

pub(super) fn civics(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, view: &CivicsView) {
    state.civics.now = ui.input(|i| i.time);
    let zone = state.civics.zone_in(view);
    let frame = frame(r, zone);
    banner(ui, frame.banner, state, view);
    if let Some(tabs) = frame.tabs {
        tab_row(ui, tabs, state, view, zone);
    }
    let selected = state.civics.branch_index(view);
    if let Some(list) = frame.list {
        branch_list(ui, list, state, view, selected, zone == Zone::Branches);
    }
    if let (Some(tree), Some(card)) = (frame.tree, view.branches.get(selected)) {
        branch_tree(ui, tree, state, view, card, zone == Zone::Branches);
    }
    if let Some(side) = frame.offers {
        offers_panel(ui, side, state, view, frame.columns, zone == Zone::Offers);
    }
}

/// Asks the core for `request` unless an adoption went through a moment ago.
fn take(state: &mut ScreenState, request: GameRequest) {
    if state.civics.may_take() {
        state.requests.push(request);
        state.civics.taken();
    }
}

fn banner(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, view: &CivicsView) {
    art(state, ui, r, "civics");
    darken(ui.painter(), r, 70, 170);
    let compact = r.height() < 90.0;
    let (title_y, line_y, title_size) = if compact { (4.0, 36.0, 24.0) } else { (14.0, 60.0, 31.0) };
    text(ui.painter(), r.min + vec2(24.0, title_y), t!("classic.ui.screen-civics.title"), title_size, CREAM);
    text(ui.painter(), r.min + vec2(26.0, line_y), status_line(view), 14.0, CREAM);
    let free = free_chip_text(view);
    if free.is_empty() {
        return;
    }
    let chip = Rect::from_min_size(pos2(r.right() - 230.0, r.center().y - 17.0), vec2(206.0, 34.0));
    ui.painter().rect_filled(chip, 17.0, TEAL);
    ui.painter().text(chip.center(), Align2::CENTER_CENTER, free, FontId::proportional(13.0), CREAM);
}

fn status_line(view: &CivicsView) -> String {
    match view.turns {
        Some(0) => tf!("classic.ui.screen-civics.status-ready", culture = view.culture, per_turn = view.per_turn, cost = view.next_cost),
        Some(n) => tf!("classic.ui.screen-civics.status-turns", culture = view.culture, per_turn = view.per_turn, cost = view.next_cost, n = n),
        None => tf!("classic.ui.screen-civics.status-stalled", culture = view.culture, per_turn = view.per_turn, cost = view.next_cost),
    }
}

fn free_chip_text(view: &CivicsView) -> String {
    let mut free = Vec::new();
    if view.free_policies > 0 {
        free.push(tf!("classic.ui.screen-civics.free-policies", n = view.free_policies));
    }
    if view.free_tenets > 0 {
        free.push(tf!("classic.ui.screen-civics.free-tenets", n = view.free_tenets));
    }
    free.join(" · ")
}

/// The phone's two tabs.
fn tab_row(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, view: &CivicsView, zone: Zone) {
    let half = (r.width() - GAP) * 0.5;
    let offers = Rect::from_min_size(r.min, vec2(half, r.height()));
    let branches = Rect::from_min_size(pos2(offers.right() + GAP, r.top()), vec2(half, r.height()));
    let label = tf!("classic.ui.screen-civics.tab-available", n = view.offers.len());
    if button(ui, offers, &label, zone == Zone::Offers, !view.offers.is_empty()) {
        state.civics.zone = Zone::Offers;
    }
    if button(ui, branches, t!("classic.ui.screen-civics.branches-tab"), zone == Zone::Branches, true) {
        state.civics.zone = Zone::Branches;
    }
}

// ---------------------------------------------------------------------------------------------
// Text helpers

/// `value` in at most `rows` rows of `width`: lines break at spaces, and when the text does not
/// fit the last line ends with "..." at a word boundary (a word wider than the line is cut). Returns the height.
fn clipped(p: &egui::Painter, at: egui::Pos2, value: &str, size: f32, color: Color32, width: f32, rows: usize) -> f32 {
    let lines = wrap_lines(p, value, size, width.max(8.0), rows.max(1));
    let font = FontId::proportional(size);
    let mut y = at.y;
    for line in &lines {
        let galley = p.layout_no_wrap(line.clone(), font.clone(), color);
        let height = galley.size().y;
        p.galley(pos2(at.x, y), galley, color);
        y += height;
    }
    y - at.y
}

fn text_width(p: &egui::Painter, value: &str, size: f32) -> f32 {
    p.layout_no_wrap(value.to_owned(), FontId::proportional(size), Color32::WHITE).size().x
}

/// The lines of `value` for `clipped`.
fn wrap_lines(p: &egui::Painter, value: &str, size: f32, width: f32, rows: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut words = value.split_whitespace();
    while let Some(word) = words.next() {
        let last_row = lines.len() == rows;
        let current = lines.last().cloned().unwrap_or_default();
        let joined = if current.is_empty() { word.to_owned() } else { format!("{current} {word}") };
        if text_width(p, &joined, size) <= width {
            match lines.last_mut() {
                Some(line) => *line = joined,
                None => lines.push(joined),
            }
            continue;
        }
        if lines.is_empty() || !last_row {
            // A new row; a first word wider than the row is cut.
            lines.push(word.to_owned());
            if text_width(p, word, size) > width {
                let cut = ellipsize(p, word, size, width);
                lines.pop();
                lines.push(cut);
            }
            continue;
        }
        let line = lines.pop().unwrap_or_default();
        lines.push(ellipsize(p, &format!("{line} ..."), size, width));
        break;
    }
    lines
}

/// `value` cut to `width` with a closing "...": whole words dropped first, then letters.
fn ellipsize(p: &egui::Painter, value: &str, size: f32, width: f32) -> String {
    let body = value.trim_end_matches('.').trim_end();
    if text_width(p, value, size) <= width && !value.ends_with("...") {
        return value.to_owned();
    }
    let mut words: Vec<&str> = body.split_whitespace().collect();
    while words.len() > 1 {
        let shown = format!("{} ...", words.join(" "));
        if text_width(p, &shown, size) <= width {
            return shown;
        }
        words.pop();
    }
    let mut cut = words.first().copied().unwrap_or("").to_owned();
    while !cut.is_empty() && text_width(p, &format!("{cut}..."), size) > width {
        cut.pop();
    }
    format!("{cut}...")
}

/// The first clause of an effect text: what it does in one line ("-25% culture ...").
pub(super) fn short_effect(value: &str) -> String {
    let cut = value.find(';').or_else(|| value.find(". ")).unwrap_or(value.len());
    value[..cut].trim().trim_end_matches('.').to_owned()
}

/// What a branch is for: its opener's effect, then its finisher's, one line each clause.
pub(super) fn purpose(card: &BranchCard) -> String {
    let parts: Vec<String> = [&card.opener, &card.finisher]
        .into_iter()
        .flatten()
        .map(|p| short_effect(&p.text))
        .filter(|s| !s.is_empty())
        .collect();
    parts.join(" · ")
}

/// The whole of `purpose`, for the hover.
fn purpose_full(card: &BranchCard) -> String {
    let mut lines = Vec::new();
    if let Some(opener) = &card.opener {
        lines.push(format!("{}: {}", t!("classic.ui.screen-civics.opener"), opener.text));
    }
    if let Some(finisher) = &card.finisher {
        lines.push(format!("{}: {}", t!("classic.ui.screen-civics.finisher"), finisher.text));
    }
    lines.join("\n")
}

/// Why `lock` holds, with the numbers the player needs.
pub(super) fn lock_text(lock: crate::ui::model::Lock, view: &CivicsView) -> String {
    if lock.is_culture() {
        return tf!("classic.ui.screen-civics.needs-culture", cost = view.next_cost, have = view.culture);
    }
    lock.reason().to_owned()
}

/// The reason a policy is locked: a missing earlier policy is named.
fn policy_reason(p: &PolicyCard, branch: &BranchCard, view: &CivicsView) -> Option<String> {
    let PolicyState::Locked(lock) = p.state else { return None };
    if lock == crate::ui::model::Lock::Policy(classic_rules::derived::culture::PolicyBlock::MissingPrerequisite) {
        let missing = p.requires.iter().filter_map(|i| branch.policies.get(*i)).find(|q| !q.adopted());
        if let Some(q) = missing {
            return Some(tf!("classic.ui.screen-civics.needs-policy", name = q.name.as_str()));
        }
    }
    Some(lock_text(lock, view))
}

fn cost_label(verb: Verb, cost: Cost) -> String {
    match (verb, cost) {
        (Verb::Adopt, Cost::Culture(n)) => tf!("classic.ui.screen-civics.adopt-culture", n = n),
        (Verb::Unlock, Cost::Culture(n)) => tf!("classic.ui.screen-civics.unlock-culture", n = n),
        (Verb::Adopt, _) => t!("classic.ui.screen-civics.adopt-free").to_owned(),
        (Verb::Unlock, _) => t!("classic.ui.screen-civics.unlock-free").to_owned(),
        (Verb::Choose, _) => t!("classic.ui.screen-civics.choose").to_owned(),
    }
}

#[derive(Clone, Copy)]
enum Verb {
    Adopt,
    Unlock,
    Choose,
}

impl Verb {
    fn of(kind: OfferKind) -> Verb {
        match kind {
            OfferKind::Policy(_) => Verb::Adopt,
            OfferKind::Unlock(_) => Verb::Unlock,
            OfferKind::Ideology(_) => Verb::Choose,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The branch list

fn branch_list(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, view: &CivicsView, selected: usize, focused: bool) {
    panel(ui.painter(), r);
    let headings = if view.branches.iter().any(|b| b.ideology) { 2.0 } else { 1.0 };
    let rows = view.branches.len().max(1) as f32;
    let row_h = ((r.height() - 18.0 * headings - 14.0) / rows).clamp(34.0, 56.0);
    let mut y = r.top() + 8.0;
    let mut ideology_heading = false;
    text(ui.painter(), pos2(r.left() + 16.0, y), t!("classic.ui.screen-civics.branches"), 10.0, GOLD);
    y += 18.0;
    for (i, branch) in view.branches.iter().enumerate() {
        if branch.ideology && !ideology_heading {
            ideology_heading = true;
            y += 4.0;
            text(ui.painter(), pos2(r.left() + 16.0, y), t!("classic.ui.screen-civics.ideologies"), 10.0, GOLD);
            y += 18.0;
        }
        let row = Rect::from_min_size(pos2(r.left() + 6.0, y), vec2(r.width() - 12.0, row_h - 3.0));
        if branch_row(ui, row, state, view, branch, i == selected && focused, i == selected) {
            state.civics.pick_branch(branch.id);
        }
        y += row_h;
    }
}

fn branch_row(ui: &mut egui::Ui, row: Rect, state: &mut ScreenState, view: &CivicsView, branch: &BranchCard, focused: bool, selected: bool) -> bool {
    let response = ui.interact(row, ui.id().with(("civics_branch", branch.id.0)), Sense::click());
    crate::ui::pedia_link::mark(ui, row, crate::ui::pedia::PediaRef::branch(branch.id));
    let fill = if selected { Color32::from_rgb(37, 65, 66) } else if response.hovered() { RAISED } else { PANEL };
    ui.painter().rect_filled(row, 4.0, fill);
    if focused {
        ui.painter().rect_stroke(row, 4.0, Stroke::new(1.4, GOLD), egui::StrokeKind::Inside);
    }
    let badge = Rect::from_center_size(pos2(row.left() + 20.0, row.center().y), vec2(30.0, 30.0));
    emblem(ui, badge, state, &branch.art, !matches!(branch.state, BranchState::Locked(_)));
    let ready = branch.adoptable().count() + usize::from(matches!(branch.state, BranchState::Available(_)));
    if ready > 0 {
        count_dot(ui, pos2(badge.right() - 1.0, badge.top() + 3.0), ready);
    }
    let left = row.left() + 46.0;
    let name_color = if matches!(branch.state, BranchState::Locked(_)) { MUTED } else { CREAM };
    text(ui.painter(), pos2(left, row.top() + 3.0), &branch.name, 14.0, name_color);
    let (tag, tag_color) = branch_tag(branch, view);
    let name_w = text_width(ui.painter(), &branch.name, 14.0);
    let tag = ellipsize(ui.painter(), &tag, 10.5, (row.right() - 8.0 - row.left() - 46.0 - name_w - 8.0).max(40.0));
    let tag_galley = ui.painter().layout_no_wrap(tag, FontId::proportional(10.5), tag_color);
    let tag_x = row.right() - 8.0 - tag_galley.size().x;
    ui.painter().galley(pos2(tag_x, row.top() + 5.0), tag_galley, tag_color);
    let rows = if row.height() >= 48.0 { 2 } else { 1 };
    let width = row.right() - 8.0 - left;
    clipped(ui.painter(), pos2(left, row.top() + 21.0), &purpose(branch), 10.0, MUTED, width, rows);
    response.clone().on_hover_text(format!("{}\n{}", branch_state_line(branch, view), purpose_full(branch)));
    response.clicked()
}

/// A small badge with a number: how many things the branch has to take now.
fn count_dot(ui: &egui::Ui, at: egui::Pos2, n: usize) {
    ui.painter().circle_filled(at, 7.5, CULTURE);
    ui.painter().text(at, Align2::CENTER_CENTER, n.to_string(), FontId::proportional(10.0), INK_DARK);
}

/// The short tag at the right of a branch row: progress, price, or what is missing.
fn branch_tag(branch: &BranchCard, view: &CivicsView) -> (String, Color32) {
    match branch.state {
        BranchState::Open if branch.ideology => (tf!("classic.ui.screen-civics.tag-tenets", n = branch.adopted_count()), GOLD),
        BranchState::Open => (tf!("classic.ui.screen-civics.tag-progress", adopted = branch.adopted_count(), total = branch.policies.len()), GOLD),
        BranchState::Available(Cost::Culture(n)) => (tf!("classic.ui.screen-civics.tag-price", n = n), CULTURE),
        BranchState::Available(_) => (t!("classic.ui.screen-civics.tag-free").to_owned(), CULTURE),
        BranchState::Locked(lock) if lock.is_era() => (branch.era.clone().unwrap_or_default(), MUTED),
        BranchState::Locked(lock) if lock.is_culture() => (tf!("classic.ui.screen-civics.tag-price", n = view.next_cost), MUTED),
        BranchState::Locked(lock) => (lock.reason().to_owned(), MUTED),
    }
}

/// The state of a branch in words (hover, and the tree's header).
fn branch_state_line(branch: &BranchCard, view: &CivicsView) -> String {
    match branch.state {
        BranchState::Open if branch.ideology => tf!("classic.ui.screen-civics.status-adopted-tenets", n = branch.adopted_count()),
        BranchState::Open => tf!("classic.ui.screen-civics.status-policies", adopted = branch.adopted_count(), total = branch.policies.len()),
        BranchState::Available(_) if branch.ideology => t!("classic.ui.screen-civics.can-be-chosen").to_owned(),
        BranchState::Available(_) => t!("classic.ui.screen-civics.can-be-unlocked").to_owned(),
        BranchState::Locked(lock) if lock.is_era() => match &branch.era {
            Some(era) => tf!("classic.ui.screen-civics.from-era", era = era.to_uppercase()),
            None => lock.reason().to_uppercase(),
        },
        BranchState::Locked(lock) => lock_text(lock, view),
    }
}

fn emblem(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, art_id: &str, bright: bool) {
    let painter = ui.painter().clone();
    painter.circle_filled(r.center(), r.width() * 0.5, INK_DARK);
    match state.policy_art.texture_of(ui.ctx(), art_id) {
        Some(texture) => {
            let tint = Color32::from_gray(if bright { 255 } else { 170 });
            painter.image(texture, r, Rect::from_min_max(pos2(0., 0.), pos2(1., 1.)), tint);
        }
        None => {
            painter.circle_stroke(r.center(), r.width() * 0.42, Stroke::new(1.0, GOLD.gamma_multiply(0.5)));
        }
    }
    let ring = if bright { GOLD } else { Color32::from_rgb(70, 86, 84) };
    painter.circle_stroke(r.center(), r.width() * 0.5, Stroke::new(1.2, ring));
}

// ---------------------------------------------------------------------------------------------
// The tree

fn branch_tree(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, view: &CivicsView, branch: &BranchCard, focused: bool) {
    panel(ui.painter(), r);
    let header_h = if r.height() >= 420.0 { 84.0 } else { 62.0 };
    let header = Rect::from_min_size(r.min, vec2(r.width(), header_h));
    branch_header(ui, header, state, view, branch);
    let area = Rect::from_min_max(pos2(r.left() + 10.0, header.bottom() + 2.0), r.max - vec2(10.0, 10.0));
    let focus = if focused { state.civics.policy_in(branch).map(|p| p.id) } else { None };
    let nodes = Nodes { view, branch, focus };
    if branch.ideology {
        tenet_rows(ui, area, state, &nodes);
    } else {
        policy_grid(ui, area, state, &nodes);
    }
}

/// What a node needs to know about its branch.
struct Nodes<'a> {
    view: &'a CivicsView,
    branch: &'a BranchCard,
    /// The policy with the keyboard focus (None: the branch list has it).
    focus: Option<classic_rules::ids::PolicyId>,
}

fn branch_header(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, view: &CivicsView, branch: &BranchCard) {
    let size = (r.height() - 20.0).min(56.0);
    let badge = Rect::from_center_size(pos2(r.left() + 14.0 + size * 0.5, r.center().y), vec2(size, size));
    emblem(ui, badge, state, &branch.art, !matches!(branch.state, BranchState::Locked(_)));
    let left = badge.right() + 14.0;
    let button_w = 196.0;
    text(ui.painter(), pos2(left, r.top() + 10.0), &branch.name, 22.0, CREAM);
    let (line, color) = header_state(branch, view);
    text(ui.painter(), pos2(left + 1.0, r.top() + 38.0), line, 11.0, color);
    let text_left = left + 150.0_f32.min(r.width() * 0.2);
    let reserved = if matches!(branch.state, BranchState::Available(_)) { button_w + 24.0 } else { 14.0 };
    let width = r.right() - reserved - text_left;
    if let (Some(opener), true) = (&branch.opener, width > 120.0) {
        let label = t!("classic.ui.screen-civics.opener");
        text(ui.painter(), pos2(text_left, r.top() + 12.0), label, 10.0, GOLD);
        clipped(ui.painter(), pos2(text_left, r.top() + 26.0), &opener.text, 11.5, CREAM, width, 3);
        ui.interact(Rect::from_min_size(pos2(text_left, r.top() + 8.0), vec2(width, r.height() - 16.0)), ui.id().with(("civics_opener", branch.id.0)), Sense::hover())
            .on_hover_text(&opener.text);
    }
    header_action(ui, r, state, branch, button_w);
}

/// The state of the branch as a plain line: gold when open, pink when it may be taken, grey and
/// the reason when locked.
fn header_state(branch: &BranchCard, view: &CivicsView) -> (String, Color32) {
    let line = branch_state_line(branch, view);
    match branch.state {
        BranchState::Open => (line, GOLD),
        BranchState::Available(_) => (line, CULTURE),
        BranchState::Locked(_) => (line, MUTED),
    }
}

/// The one button of a header: unlock the branch or choose the ideology, only when that can be done.
fn header_action(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, branch: &BranchCard, width: f32) {
    let BranchState::Available(cost) = branch.state else { return };
    let rect = Rect::from_min_size(pos2(r.right() - width - 12.0, r.center().y - 17.0), vec2(width, 34.0));
    let label = if branch.ideology {
        match branch.free_tenets {
            0 => t!("classic.ui.screen-civics.choose-ideology").to_owned(),
            n => tf!("classic.ui.screen-civics.choose-ideology-free", n = n),
        }
    } else {
        cost_label(Verb::Unlock, cost)
    };
    if button(ui, rect, &label, true, true) {
        let kind = if branch.ideology { OfferKind::Ideology(branch.id) } else { OfferKind::Unlock(branch.id) };
        take(state, kind.request());
    }
}

/// Policies of a normal branch on their rows, the prerequisite lines between them, the finisher
/// as a wide card under the tree. Columns are the distinct columns of the branch (no empty ones).
fn policy_grid(ui: &mut egui::Ui, area: Rect, state: &mut ScreenState, nodes: &Nodes) {
    let branch = nodes.branch;
    let finisher_h = if area.height() >= 330.0 { 54.0 } else { 42.0 };
    let grid = Rect::from_min_max(area.min, pos2(area.right(), area.bottom() - finisher_h - 8.0));
    let columns = distinct(branch.policies.iter().map(|p| p.grid.0));
    let rows = distinct(branch.policies.iter().map(|p| p.grid.1));
    let line_gap = 14.0;
    let node_w = ((grid.width() - GAP * (columns.len().max(1) as f32 - 1.0)) / columns.len().max(1) as f32).min(NODE_MAX.x);
    let node_h = ((grid.height() - line_gap * (rows.len().max(1) as f32 - 1.0)) / rows.len().max(1) as f32).min(NODE_MAX.y);
    let used_w = node_w * columns.len() as f32 + GAP * (columns.len().max(1) as f32 - 1.0);
    let left = grid.left() + (grid.width() - used_w) * 0.5;
    let place = |p: &PolicyCard| {
        let c = columns.iter().position(|v| *v == p.grid.0).unwrap_or(0) as f32;
        let row = rows.iter().position(|v| *v == p.grid.1).unwrap_or(0) as f32;
        Rect::from_min_size(pos2(left + c * (node_w + GAP), grid.top() + row * (node_h + line_gap)), vec2(node_w, node_h))
    };
    draw_links(ui, branch, &place);
    for p in &branch.policies {
        policy_node(ui, place(p), state, nodes, p);
    }
    if let Some(finisher) = &branch.finisher {
        let wide = Rect::from_min_size(pos2(area.left(), area.bottom() - finisher_h), vec2(area.width(), finisher_h));
        finisher_card(ui, wide, state, finisher);
    }
}

/// The distinct values, sorted.
fn distinct(values: impl Iterator<Item = u8>) -> Vec<u8> {
    let mut all: Vec<u8> = values.collect();
    all.sort_unstable();
    all.dedup();
    all
}

fn draw_links(ui: &egui::Ui, branch: &BranchCard, place: &dyn Fn(&PolicyCard) -> Rect) {
    for p in &branch.policies {
        for parent in &p.requires {
            let Some(from) = branch.policies.get(*parent) else { continue };
            let (a, b) = (place(from).center_bottom(), place(p).center_top());
            let mid = (a.y + b.y) * 0.5;
            let color = if from.adopted() { GOLD } else { GOLD.gamma_multiply(0.35) };
            ui.painter().add(egui::Shape::line(vec![a, pos2(a.x, mid), pos2(b.x, mid), b], Stroke::new(2.0, color)));
        }
    }
}

/// Ideology tenets in three tiers.
fn tenet_rows(ui: &mut egui::Ui, area: Rect, state: &mut ScreenState, nodes: &Nodes) {
    let row_h = area.height() / 3.0;
    for tier in 1..=3u8 {
        let top = area.top() + f32::from(tier - 1) * row_h;
        text(ui.painter(), pos2(area.left(), top + 2.0), tf!("classic.ui.screen-civics.level", tier = tier), 10.0, GOLD);
        let tenets: Vec<&PolicyCard> = nodes.branch.policies.iter().filter(|p| p.tier == tier).collect();
        let count = tenets.len().max(1) as f32;
        let width = ((area.width() - GAP * (count - 1.0)) / count).min(NODE_MAX.x + 20.0);
        for (i, p) in tenets.iter().enumerate() {
            let at = pos2(area.left() + i as f32 * (width + GAP), top + 17.0);
            policy_node(ui, Rect::from_min_size(at, vec2(width, (row_h - 22.0).min(NODE_MAX.y))), state, nodes, p);
        }
    }
}

/// How a node is laid out for its size.
#[derive(Clone, Copy, PartialEq, Eq)]
enum NodeShape {
    /// Emblem and name side by side, the effect under them, the state at the foot.
    Full,
    /// Narrow (many tenets in a row): the name under the emblem.
    Stacked,
    /// Short (a phone): one line of name with the button or nothing, one line of text.
    Compact,
}

impl NodeShape {
    fn of(node: Rect) -> NodeShape {
        if node.height() < 78.0 {
            NodeShape::Compact
        } else if node.width() < 120.0 {
            NodeShape::Stacked
        } else {
            NodeShape::Full
        }
    }
}

/// One policy: emblem, name, its effect (the first lines of it; all of it on hover), and at the
/// foot its state: adopted, the adopt button with the price, or why it is locked.
fn policy_node(ui: &mut egui::Ui, node: Rect, state: &mut ScreenState, nodes: &Nodes, p: &PolicyCard) {
    let response = ui.interact(node, ui.id().with(("civics_policy", p.id.0)), Sense::click());
    crate::ui::pedia_link::mark(ui, node, crate::ui::pedia::PediaRef::policy(p.id));
    if response.clicked() {
        state.civics.pick_policy(p.id);
    }
    paint_node_frame(ui, node, p.state, nodes.focus == Some(p.id), response.hovered());
    let shape = NodeShape::of(node);
    let lit = matches!(p.state, PolicyState::Adopted | PolicyState::Adoptable(_));
    let badge_size = if shape == NodeShape::Full { 30.0 } else { 22.0 };
    let badge = Rect::from_min_size(node.min + vec2(7.0, if shape == NodeShape::Compact { 5.0 } else { 7.0 }), vec2(badge_size, badge_size));
    emblem(ui, badge, state, &p.art, lit);
    let name_color = if lit { CREAM } else { MUTED };
    let body_top = match shape {
        NodeShape::Full => {
            let left = badge.right() + 7.0;
            clipped(ui.painter(), pos2(left, node.top() + 8.0), &p.name, 13.0, name_color, node.right() - 6.0 - left, 2);
            badge.bottom() + 6.0
        }
        NodeShape::Stacked => {
            clipped(ui.painter(), pos2(node.left() + 8.0, badge.bottom() + 4.0), &p.name, 12.0, name_color, node.width() - 12.0, 1);
            badge.bottom() + 22.0
        }
        NodeShape::Compact => {
            let right = if p.price().is_some() { node.right() - 96.0 } else { node.right() - 6.0 };
            let left = badge.right() + 6.0;
            clipped(ui.painter(), pos2(left, node.top() + 7.0), &p.name, 12.0, name_color, right - left, 1);
            node.top() + 30.0
        }
    };
    let ink = if lit { Color32::from_rgb(214, 220, 206) } else { MUTED };
    if shape == NodeShape::Compact {
        compact_body(ui, node, state, nodes, p, body_top, ink);
    } else {
        let foot_h = 28.0;
        let rows = (((node.bottom() - foot_h - 4.0 - body_top) / 13.0).floor() as usize).clamp(1, 4);
        clipped(ui.painter(), pos2(node.left() + 8.0, body_top), &p.text, 10.5, ink, node.width() - 16.0, rows);
        let foot = Rect::from_min_max(pos2(node.left() + 6.0, node.bottom() - foot_h), node.max - vec2(6.0, 4.0));
        node_foot(ui, foot, state, nodes, p);
    }
    response.on_hover_text(node_hover(nodes, p));
}

/// The short node: the price button at the top right, one line below: why it is locked, else its effect.
fn compact_body(ui: &mut egui::Ui, node: Rect, state: &mut ScreenState, nodes: &Nodes, p: &PolicyCard, top: f32, ink: Color32) {
    if let PolicyState::Adoptable(cost) = p.state {
        let button_rect = Rect::from_min_size(pos2(node.right() - 90.0, node.top() + 4.0), vec2(84.0, 22.0));
        if button(ui, button_rect, &cost_label(Verb::Adopt, cost), true, true) {
            take(state, GameRequest::AdoptPolicy(p.id));
        }
    }
    let width = node.width() - 16.0;
    match policy_reason(p, nodes.branch, nodes.view) {
        Some(reason) => clipped(ui.painter(), pos2(node.left() + 8.0, top), &reason, 10.0, GOLD.gamma_multiply(0.9), width, 1),
        None => clipped(ui.painter(), pos2(node.left() + 8.0, top), &p.text, 10.0, ink, width, 1),
    };
}

fn paint_node_frame(ui: &egui::Ui, node: Rect, state: PolicyState, focused: bool, hovered: bool) {
    let fill = match state {
        PolicyState::Adopted => ADOPTED_FILL,
        PolicyState::Adoptable(_) if hovered => Color32::from_rgb(34, 60, 62),
        PolicyState::Adoptable(_) => RAISED,
        PolicyState::Included | PolicyState::Locked(_) if hovered => Color32::from_rgb(25, 47, 52),
        PolicyState::Included | PolicyState::Locked(_) => LOCKED_FILL,
    };
    let edge = match state {
        PolicyState::Adopted => GOLD,
        PolicyState::Adoptable(_) => CULTURE,
        _ => EDGE,
    };
    ui.painter().rect_filled(node, 5.0, fill);
    ui.painter().rect_stroke(node, 5.0, Stroke::new(1.0, edge), egui::StrokeKind::Inside);
    if focused {
        ui.painter().rect_stroke(node.expand(1.5), 6.0, Stroke::new(1.8, GOLD), egui::StrokeKind::Outside);
    }
}

fn node_foot(ui: &mut egui::Ui, foot: Rect, state: &mut ScreenState, nodes: &Nodes, p: &PolicyCard) {
    match p.state {
        PolicyState::Adopted => {
            text(ui.painter(), foot.left_center() + vec2(2.0, -6.0), t!("classic.ui.screen-civics.adopted"), 10.0, GOLD);
        }
        PolicyState::Adoptable(cost) => {
            if button(ui, foot, &cost_label(Verb::Adopt, cost), true, true) {
                take(state, GameRequest::AdoptPolicy(p.id));
            }
        }
        PolicyState::Included => {
            text(ui.painter(), foot.left_center() + vec2(2.0, -6.0), t!("classic.ui.screen-civics.included"), 10.0, MUTED);
        }
        PolicyState::Locked(lock) => {
            let reason = policy_reason(p, nodes.branch, nodes.view).unwrap_or_default();
            let color = if lock.is_culture() { GOLD } else { MUTED };
            clipped(ui.painter(), foot.min + vec2(2.0, 1.0), &reason, 10.0, color, foot.width() - 4.0, 2);
        }
    }
}

fn node_hover(nodes: &Nodes, p: &PolicyCard) -> String {
    let state = match p.state {
        PolicyState::Adopted => t!("classic.ui.screen-civics.adopted").to_owned(),
        PolicyState::Adoptable(_) => t!("classic.ui.screen-civics.available").to_owned(),
        PolicyState::Included => t!("classic.ui.screen-civics.included").to_owned(),
        PolicyState::Locked(_) => policy_reason(p, nodes.branch, nodes.view).unwrap_or_default(),
    };
    format!("{}\n{}\n{}", p.name, p.text, state)
}

fn finisher_card(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, p: &PolicyCard) {
    let response = ui.interact(r, ui.id().with(("civics_finisher", p.id.0)), Sense::click());
    crate::ui::pedia_link::mark(ui, r, crate::ui::pedia::PediaRef::policy(p.id));
    if response.clicked() {
        state.civics.pick_policy(p.id);
    }
    let done = p.adopted();
    ui.painter().rect_filled(r, 5.0, if done { Color32::from_rgb(44, 58, 42) } else { LOCKED_FILL });
    ui.painter().rect_stroke(r, 5.0, Stroke::new(1.0, if done { GOLD } else { EDGE }), egui::StrokeKind::Inside);
    let title = if done { t!("classic.ui.screen-civics.branch-completed") } else { t!("classic.ui.screen-civics.complete-branch") };
    text(ui.painter(), r.min + vec2(12.0, 6.0), title, 10.0, GOLD);
    let rows = if r.height() >= 50.0 { 2 } else { 1 };
    clipped(ui.painter(), r.min + vec2(12.0, 21.0), &p.text, 11.5, CREAM, r.width() - 24.0, rows);
    response.on_hover_text(&p.text);
}

// ---------------------------------------------------------------------------------------------
// Available now

fn offers_panel(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, view: &CivicsView, columns: usize, focused: bool) {
    panel(ui.painter(), r);
    let title = r.min + vec2(16.0, 12.0);
    text(ui.painter(), title, t!("classic.ui.screen-civics.available-title"), 11.0, GOLD);
    if !view.offers.is_empty() {
        count_dot(ui, pos2(r.left() + 140.0, r.top() + 19.0), view.offers.len());
    }
    let hint_h = 20.0;
    let area = Rect::from_min_max(pos2(r.left() + 10.0, r.top() + 36.0), pos2(r.right() - 10.0, r.bottom() - hint_h - 8.0));
    if view.offers.is_empty() {
        nothing_to_take(ui, area, view);
    } else {
        offer_cards(ui, area, state, view, columns, focused);
    }
    if !client::shell::theme::touch_wanted() {
        let hint = ui.painter().layout_no_wrap(t!("classic.ui.screen-civics.keys-hint").to_owned(), FontId::proportional(10.0), MUTED);
        ui.painter().galley(pos2(r.left() + 16.0, r.bottom() - hint_h), hint, MUTED);
    }
}

/// The empty list: why, and how far the next policy is.
fn nothing_to_take(ui: &mut egui::Ui, area: Rect, view: &CivicsView) {
    let width = area.width() - 12.0;
    clipped(ui.painter(), area.min + vec2(6.0, 4.0), t!("classic.ui.screen-civics.nothing-now"), 15.0, CREAM, width, 2);
    let bar = Rect::from_min_size(area.min + vec2(6.0, 56.0), vec2(width, 10.0));
    ui.painter().rect_filled(bar, 5.0, INK_DARK);
    let share = if view.next_cost == 0 { 1.0 } else { (view.culture as f32 / view.next_cost as f32).clamp(0.0, 1.0) };
    ui.painter().rect_filled(Rect::from_min_size(bar.min, vec2(bar.width() * share, bar.height())), 5.0, CULTURE);
    let line = tf!("classic.ui.screen-civics.culture-bar", culture = view.culture, cost = view.next_cost);
    clipped(ui.painter(), area.min + vec2(6.0, 72.0), &line, 12.0, MUTED, width, 2);
}

fn offer_cards(ui: &mut egui::Ui, area: Rect, state: &mut ScreenState, view: &CivicsView, columns: usize, focused: bool) {
    let columns = columns.max(1);
    let line_h = 6.0;
    let rows_total = view.offers.len().div_ceil(columns);
    let card_h = ((area.height() / rows_total as f32) - line_h).clamp(80.0, 100.0);
    let fit = ((area.height() + line_h) / (card_h + line_h)).floor().max(1.0) as usize * columns;
    let shown = view.offers.len().min(fit);
    let card_w = (area.width() - GAP * (columns as f32 - 1.0)) / columns as f32;
    let current = state.civics.offer.min(view.offers.len() - 1);
    for (i, offer) in view.offers.iter().take(shown).enumerate() {
        let (c, row) = (i % columns, i / columns);
        let at = pos2(area.left() + c as f32 * (card_w + GAP), area.top() + row as f32 * (card_h + line_h));
        let card = Rect::from_min_size(at, vec2(card_w, card_h));
        offer_card(ui, card, state, offer, i, focused && i == current);
    }
    if shown < view.offers.len() {
        let rest = tf!("classic.ui.screen-civics.more-offers", n = view.offers.len() - shown);
        let y = area.top() + (shown.div_ceil(columns)) as f32 * (card_h + line_h);
        text(ui.painter(), pos2(area.left() + 6.0, y), rest, 11.0, MUTED);
    }
}

fn offer_card(ui: &mut egui::Ui, card: Rect, state: &mut ScreenState, offer: &Offer, index: usize, focused: bool) {
    let response = ui.interact(card, ui.id().with(("civics_offer", index)), Sense::click());
    if response.clicked() {
        state.civics.pick_offer(index);
    }
    ui.painter().rect_filled(card, 5.0, if response.hovered() { Color32::from_rgb(34, 60, 62) } else { RAISED });
    ui.painter().rect_stroke(card, 5.0, Stroke::new(1.0, CULTURE), egui::StrokeKind::Inside);
    if focused {
        ui.painter().rect_stroke(card.expand(1.5), 6.0, Stroke::new(1.8, GOLD), egui::StrokeKind::Outside);
    }
    let badge = Rect::from_min_size(card.min + vec2(8.0, 8.0), vec2(38.0, 38.0));
    emblem(ui, badge, state, &offer.art, true);
    let button_rect = Rect::from_min_size(pos2(card.right() - 112.0, card.top() + 8.0), vec2(104.0, 28.0));
    let left = badge.right() + 8.0;
    clipped(ui.painter(), pos2(left, card.top() + 8.0), &offer.name, 14.0, CREAM, button_rect.left() - 6.0 - left, 1);
    text(ui.painter(), pos2(left, card.top() + 27.0), offer_role(offer), 10.0, GOLD);
    let top = badge.bottom() + 6.0;
    let rows = (((card.bottom() - top - 12.0) / 14.0).floor() as usize).clamp(1, 4);
    clipped(ui.painter(), pos2(card.left() + 10.0, top), &offer.text, 11.5, Color32::from_rgb(214, 220, 206), card.width() - 20.0, rows);
    if button(ui, button_rect, &cost_label(Verb::of(offer.kind), offer.cost), true, true) {
        take(state, offer.kind.request());
    }
    response.on_hover_text(format!("{}\n{}", offer.name, offer.text));
}

fn offer_role(offer: &Offer) -> String {
    let branch = offer.branch.to_uppercase();
    match offer.kind {
        OfferKind::Policy(_) => tf!("classic.ui.screen-civics.role-policy", branch = branch),
        OfferKind::Unlock(_) => tf!("classic.ui.screen-civics.role-opener", branch = branch),
        OfferKind::Ideology(_) => tf!("classic.ui.screen-civics.role-ideology", branch = branch),
    }
}

/// Art ids the screen shows now (offscreen captures decode them synchronously).
pub(super) fn visible_art(view: &CivicsView, state: &ScreenState) -> Vec<String> {
    let mut ids: Vec<String> = view.branches.iter().map(|b| b.art.clone()).collect();
    if let Some(card) = view.branches.get(state.civics.branch_index(view)) {
        ids.extend(card.policies.iter().map(|p| p.art.clone()));
    }
    ids.extend(view.offers.iter().map(|o| o.art.clone()));
    ids
}

#[cfg(test)]
#[path = "screen_civics_tests.rs"]
mod tests;
