//! The real game's technology tree on the shell's graph widget (`widgets::dag`):
//! read-only cards in, typed requests out. The words and tooltips are the classic module's.
use super::*;
use i18n::{t, tf};
use client::shell::art::ArtPen;
use client::shell::kit::art_tint;
use client::shell::widgets::cards::{CardLine, ImageCard, Picture};
use client::shell::widgets::dag::{self, Cell, Corner, Graph, Node, Progress};

/// The tree's scroll state salt.
const TREE: &str = "real_research_tree";

fn progress(status: &TechStatus) -> Progress {
    match status {
        TechStatus::Known => Progress::Done,
        TechStatus::Researching => Progress::Queued(1),
        TechStatus::Queued(n) => Progress::Queued(*n as u32 + 1),
        TechStatus::Available | TechStatus::Locked(_) => Progress::Open,
    }
}

fn status_line(view: &ResearchView, tech: &TechCard) -> CardLine {
    let progress = progress(&tech.status);
    let text = if view.free_choice && tech.availability.is_ready() {
        t!("classic.ui.screen-research.free-technology").into()
    } else if progress == Progress::Done {
        t!("classic.ui.screen-research.discovered").into()
    } else if progress == Progress::Queued(1) {
        tf!("classic.ui.screen-research.science-progress", stored = view.stored, cost = tech.cost)
    } else {
        tf!("classic.ui.screen-research.science-cost", cost = tech.cost)
    };
    CardLine::new(text, if matches!(progress, Progress::Queued(_)) { GOLD } else { MUTED })
}

fn node(view: &ResearchView, tech: &TechCard) -> Node {
    let corner = tech.unlocks.first().map(|unlock| Corner {
        art: unlock.art.clone(),
        initial: unlock.name.chars().next().unwrap_or('+'),
        more: tech.unlocks.len() - 1,
    });
    Node {
        cell: Cell { column: tech.column, row: tech.row },
        requires: tech.requires.clone(),
        band: tech.era.clone(),
        progress: progress(&tech.status),
        ready: tech.availability.is_ready(),
        card: ImageCard {
            picture: Picture::Art(tech.art.clone()),
            lines: vec![CardLine::new(tech.name.clone(), CREAM), status_line(view, tech)],
            bar: None,
        },
        corner,
    }
}

/// The tree as the shell's graph.
pub(super) fn graph(view: &ResearchView) -> Graph {
    Graph { id: TREE, nodes: view.techs.iter().map(|tech| node(view, tech)).collect() }
}

pub(super) fn show(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, view: &ResearchView) {
    panel(ui.painter(), r);
    let title = if view.free_choice { t!("classic.ui.screen-research.free-title") } else { t!("classic.ui.screen-research.title") };
    text(ui.painter(), r.min + vec2(16., 10.), title, 22., CREAM);
    let subtitle = if view.free_choice { t!("classic.ui.screen-research.free-subtitle").into() }
        else { tf!("classic.ui.screen-research.subtitle", stored = view.stored, per_turn = view.per_turn) };
    text(ui.painter(), r.min + vec2(17., 39.), subtitle, 12., MUTED);
    let area = Rect::from_min_max(r.min + vec2(8., 62.), r.max - vec2(8., 8.));
    let mut pen = ArtPen { art: &mut state.art, tint: art_tint(state.brightness) };
    let response = dag::show(ui, area, &graph(view), &mut state.research_tree, &mut pen);
    if let Some(tech) = response.clicked.and_then(|i| view.techs.get(i)) {
        if tech.availability.is_ready() {
            state.requests.push(GameRequest::Research(tech.id));
        }
    }
    for card in response.cards {
        let tech = &view.techs[card.node];
        crate::ui::pedia_link::mark(ui, card.response.interact_rect, crate::ui::pedia::PediaRef::tech(tech.id));
        card.response.on_hover_ui(|ui| tooltip(ui, view, tech));
    }
}

fn tooltip(ui: &mut egui::Ui, view: &ResearchView, card: &TechCard) {
    ui.set_max_width(360.);
    let max_height = (ui.ctx().content_rect().height() - 80.).clamp(120., 400.);
    egui::ScrollArea::vertical().id_salt(("tech_information", card.id.0)).max_height(max_height).show(ui, |ui| {
        ui.heading(&card.name);
        ui.label(tf!("classic.ui.screen-research.tip-era-cost", era = card.era.as_str(), cost = card.cost));
        if let Availability::Blocked(reason) = &card.availability { ui.label(reason); }
        if !card.unlocks.is_empty() {
            ui.separator();
            ui.label(t!("classic.ui.screen-research.unlocks"));
            for unlock in &card.unlocks { ui.label(&unlock.name); }
        }
        if view.free_choice {
            ui.separator();
            ui.label(t!("classic.ui.screen-research.free-note"));
        } else if !card.plan.is_empty() {
            ui.separator();
            ui.label(tf!("classic.ui.screen-research.plan-steps", n = card.plan.len()));
            ui.label(t!("classic.ui.screen-research.plan-note"));
            for (order, &id) in card.plan.iter().enumerate() {
                if let Some(step) = view.techs.get(id) { ui.label(tf!("classic.ui.screen-research.plan-step", n = order + 1, name = step.name.as_str())); }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    fn session() -> crate::game::Session {
        let rules = crate::ui::screens::game_rules();
        let game = classic_rules::scenario::generate_player_game(&rules, &classic_rules::scenario::ScenarioConfig::quick_game(), 0);
        crate::game::Session::new(game, rules)
    }
    fn cell(tech: &TechCard) -> Cell {
        Cell { column: tech.column, row: tech.row }
    }

    #[test]
    fn every_dependency_routes_forward_outside_card_interiors() {
        let view = session().research_view();
        let mut count = 0;
        for target in &view.techs { for (order, &parent) in target.requires.iter().enumerate() {
            let source = &view.techs[parent];
            assert!(cell(source).rect().right() < cell(target).rect().left());
            let points = dag::route(cell(source), cell(target), order);
            count += 1;
            for segment in points.windows(2) { let (a, b) = (segment[0], segment[1]);
                for card in &view.techs { let r = cell(card).rect().shrink(0.01);
                    let cross_x = a.x == b.x && a.x > r.left() && a.x < r.right() && a.y.min(b.y) < r.bottom() && a.y.max(b.y) > r.top();
                    let cross_y = a.y == b.y && a.y > r.top() && a.y < r.bottom() && a.x.min(b.x) < r.right() && a.x.max(b.x) > r.left();
                    assert!(!cross_x && !cross_y, "{} -> {} crosses {}", source.name, target.name, card.name);
                }
            }
        }}
        assert_eq!(view.techs.len(), 81);
        // 134 prerequisite edges: Unciv drops one of Ballistics' two prerequisites (Railroad).
        assert_eq!(count, 134);
    }

    /// Focus the farthest technology, click its card where the widget drew it.
    #[test]
    fn far_card_pointer_click_submits_a_real_session_plan() {
        let mut session = session();
        let view = session.research_view();
        let index = view.techs.iter().position(|t| t.name == "Future Tech").unwrap();
        let target = &view.techs[index];
        assert!(target.plan.len() > 16);
        let mut state = ScreenState::default();
        state.research_tree.focus(index);
        state.research_tree.selected = None;
        let ctx = egui::Context::default();
        let screen = Rect::from_min_size(egui::Pos2::ZERO, vec2(960., 1000.));
        let frame = |state: &mut ScreenState, events: Vec<egui::Event>| {
            let mut card = None;
            let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ui| {
                let mut pen = ArtPen { art: &mut state.art, tint: Color32::WHITE };
                let area = screen.shrink(8.);
                let response = dag::show(ui, area, &graph(&view), &mut state.research_tree, &mut pen);
                card = response.cards.iter().find(|c| c.node == index).map(|c| c.response.interact_rect);
                if let Some(i) = response.clicked { state.requests.push(GameRequest::Research(view.techs[i].id)); }
            });
            output.textures_delta.clear();
            card
        };
        frame(&mut state, vec![]);
        let at = frame(&mut state, vec![]).expect("the focused far card is drawn").center();
        frame(&mut state, vec![egui::Event::PointerMoved(at)]);
        for pressed in [true, false] {
            frame(&mut state, vec![egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE }]);
        }
        let request = state.requests.pop().expect("far card emitted typed request");
        assert!(matches!(request, GameRequest::Research(id) if id == target.id));
        session.request(request);
        let queue = session.game().view().auth.players().research_queue_of(classic_rules::ids::PlayerId(0));
        assert_eq!(queue.iter().map(|id| id.0 as usize).collect::<Vec<_>>(), target.plan);
        let after = session.research_view();
        for (n, &id) in target.plan.iter().enumerate() { assert_eq!(progress(&after.techs[id].status), Progress::Queued(n as u32 + 1)); }
        let saved = session.save_bytes();
        session.load_bytes(&saved).unwrap();
        assert_eq!(session.research_view().techs[index].status, after.techs[index].status);
    }

    #[test]
    fn visible_art_working_set_fits_the_existing_bounded_cache() {
        let view = session().research_view();
        for column in 0..view.techs.iter().map(|t| t.column).max().unwrap() {
            let viewport = Rect::from_min_size(pos2(column as f32 * dag::COLUMN, 0.), vec2(1366., 1000.));
            let mut names = std::collections::HashSet::new();
            for card in &view.techs { if cell(card).rect().intersects(viewport) {
                names.insert(&card.art);
                if let Some(image) = card.unlocks.first().and_then(|u| u.art.as_ref()) { names.insert(image); }
            }}
            assert!(names.len() <= 64, "column {column}: {} visible textures", names.len());
        }
    }
}
