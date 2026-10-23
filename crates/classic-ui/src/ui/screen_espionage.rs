//! Espionage overview bound to the running game (`EspionageView`): the player's spies
//! with rank, state and city (bring home, stage a coup with its chance), the cities the
//! selected spy may be sent to (own cities for counter-intelligence, foreign capitals
//! also as a diplomat), and the technologies the steal rights allow.
use super::scroll::scroll_panel;
use super::*;
use i18n::{t, tf};
use crate::ui::model::{EspionageView, GameRequest, SpyCard, SpyTarget, StealCard};

const SIDE_W: f32 = 320.0;
const STEAL_W: f32 = 300.0;
const SPY_H: f32 = 132.0;

pub(super) fn espionage(
    ui: &mut egui::Ui,
    r: Rect,
    state: &mut ScreenState,
    view: &EspionageView,
    message: Option<&str>,
) {
    let top = Rect::from_min_size(r.min, vec2(r.width(), 92.0));
    let line = tf!("classic.ui.screen-espionage.banner-line",
        spies = view.spies.len(),
        cities = view.targets.len(),
        nations = view.steal.len()
    );
    world_banner(ui, top, t!("classic.ui.screen-espionage.title"), &line, message);
    let body = Rect::from_min_max(pos2(r.left(), top.bottom() + 14.0), r.max);
    let left = Rect::from_min_size(body.min, vec2(SIDE_W, body.height()));
    let right = Rect::from_min_max(pos2(body.right() - STEAL_W, body.top()), body.max);
    let center = Rect::from_min_max(
        pos2(left.right() + 14.0, body.top()),
        pos2(right.left() - 14.0, body.bottom()),
    );
    spies(ui, left, state, view);
    let picked = view
        .spies
        .iter()
        .find(|s| s.index == state.spy)
        .or(view.spies.first());
    destinations(ui, center, state, view, picked);
    steal(ui, right, state, view);
}

fn spies(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, view: &EspionageView) {
    panel(ui.painter(), r);
    heading(ui, r, t!("classic.ui.screen-espionage.your-spies"));
    if view.spies.is_empty() {
        let hint = t!("classic.ui.screen-espionage.no-spies");
        paragraph(
            ui.painter(),
            r.min + vec2(16.0, 36.0),
            hint,
            13.0,
            MUTED,
            r.width() - 32.0,
        );
        return;
    }
    let picked = view.spies.iter().any(|s| s.index == state.spy);
    let list = Rect::from_min_max(pos2(r.left(), r.top() + 36.0), r.max - vec2(0.0, 8.0));
    scroll_panel(
        ui,
        list,
        "spies_screen_espionage.rs",
        view.spies.len() as f32 * (SPY_H + 8.0),
        |ui, r| {
            let mut y = r.top();
            for (i, spy) in view.spies.iter().enumerate() {
                let card =
                    Rect::from_min_size(pos2(r.left() + 10.0, y), vec2(r.width() - 20.0, SPY_H));
                let selected = spy.index == state.spy || (!picked && i == 0);
                spy_card(ui, card, state, spy, selected);
                y += SPY_H + 8.0;
            }
        },
    );
}

fn spy_card(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, spy: &SpyCard, selected: bool) {
    if !ui.is_rect_visible(r) {
        return;
    }
    let response = ui.interact(r, ui.id().with(("spy_card", spy.index)), Sense::click());
    if response.clicked() {
        state.spy = spy.index;
    }
    let fill = if selected {
        Color32::from_rgb(37, 65, 66)
    } else {
        RAISED
    };
    ui.painter().rect_filled(r, 5.0, fill);
    if selected {
        let stroke = Stroke::new(1.3, GOLD);
        ui.painter()
            .rect_stroke(r, 5.0, stroke, egui::StrokeKind::Inside);
    }
    let title = format!("{}  ·  {}", spy.name, spy.rank);
    text(ui.painter(), r.min + vec2(10.0, 8.0), title, 15.0, CREAM);
    text(
        ui.painter(),
        r.min + vec2(10.0, 30.0),
        spy.state,
        12.0,
        GOLD,
    );
    let place = match (&spy.city, spy.diplomat) {
        (Some(city), true) => tf!("classic.ui.screen-espionage.diplomat-in", city = city.as_str()),
        (Some(city), false) => tf!("classic.ui.screen-espionage.in-city", city = city.as_str()),
        (None, _) => t!("classic.ui.screen-espionage.at-home").to_string(),
    };
    text(ui.painter(), r.min + vec2(10.0, 50.0), place, 12.0, MUTED);
    let half = (r.width() - 28.0) / 2.0;
    let home = Rect::from_min_size(pos2(r.left() + 10.0, r.bottom() - 40.0), vec2(half, 30.0));
    let coup = home.translate(vec2(half + 8.0, 0.0));
    if ready_button(ui, home, t!("classic.ui.screen-espionage.bring-home"), &spy.home) {
        state.requests.push(moved(spy.index, None, false));
    }
    let label = match spy.coup_chance {
        Some(chance) => tf!("classic.ui.screen-espionage.coup-chance", chance = chance),
        None => t!("classic.ui.screen-espionage.stage-coup").to_string(),
    };
    if ready_button(ui, coup, &label, &spy.coup) {
        state.requests.push(GameRequest::StageCoup(spy.index));
    }
}

fn moved(spy: u8, city: Option<classic_rules::ids::CityHandle>, diplomat: bool) -> GameRequest {
    GameRequest::MoveSpy {
        spy,
        city,
        diplomat,
    }
}

/// Where the selected spy may go, with Send and (foreign capitals) As diplomat.
fn destinations(
    ui: &mut egui::Ui,
    r: Rect,
    state: &mut ScreenState,
    view: &EspionageView,
    spy: Option<&SpyCard>,
) {
    panel(ui.painter(), r);
    let Some(spy) = spy else {
        heading(ui, r, t!("classic.ui.screen-espionage.destinations"));
        return;
    };
    heading(ui, r, &tf!("classic.ui.screen-espionage.send-to", name = spy.name.to_uppercase()));
    let list = Rect::from_min_max(r.min + vec2(10.0, 34.0), r.max - vec2(10.0, 10.0));
    ui.scope_builder(egui::UiBuilder::new().max_rect(list), |ui| {
        egui::ScrollArea::vertical()
            .id_salt("spy_destinations")
            .show(ui, |ui| {
                let row_h = 40.0;
                let height = view.targets.len() as f32 * (row_h + 6.0);
                let (area, _) =
                    ui.allocate_exact_size(vec2(list.width() - 12.0, height), Sense::hover());
                for (i, target) in view.targets.iter().enumerate() {
                    let row = Rect::from_min_size(
                        area.min + vec2(0.0, i as f32 * (row_h + 6.0)),
                        vec2(area.width(), row_h),
                    );
                    destination(ui, row, state, spy, target, i);
                }
            });
    });
}

fn destination(
    ui: &mut egui::Ui,
    r: Rect,
    state: &mut ScreenState,
    spy: &SpyCard,
    target: &SpyTarget,
    index: usize,
) {
    ui.painter().rect_filled(r, 4.0, RAISED);
    text(
        ui.painter(),
        r.min + vec2(10.0, 4.0),
        &target.name,
        14.0,
        CREAM,
    );
    let owner = format!("{}  ·  {}", target.owner, target.role);
    text(ui.painter(), r.min + vec2(10.0, 22.0), owner, 10.0, MUTED);
    let Some(moves) = spy.moves.get(index) else {
        return;
    };
    let send = Rect::from_min_size(pos2(r.right() - 306.0, r.top() + 5.0), vec2(176.0, 30.0));
    if ready_button(ui, send, t!("classic.ui.screen-espionage.send"), &moves.spy) {
        state
            .requests
            .push(moved(spy.index, Some(target.city), false));
    }
    if let Some(diplomat) = &moves.diplomat {
        let as_diplomat =
            Rect::from_min_size(pos2(r.right() - 122.0, r.top() + 5.0), vec2(116.0, 30.0));
        if ready_button(ui, as_diplomat, t!("classic.ui.screen-espionage.as-diplomat"), diplomat) {
            state
                .requests
                .push(moved(spy.index, Some(target.city), true));
        }
    }
}

fn steal(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, view: &EspionageView) {
    panel(ui.painter(), r);
    heading(ui, r, t!("classic.ui.screen-espionage.steal"));
    if view.steal.is_empty() {
        let hint = t!("classic.ui.screen-espionage.no-steal");
        paragraph(
            ui.painter(),
            r.min + vec2(16.0, 36.0),
            hint,
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
        "steal_screen_espionage.rs",
        view.steal
            .iter()
            .map(|c| 38.0 + c.techs.len() as f32 * 34.0)
            .sum(),
        |ui, r| {
            let mut y = r.top();
            for card in &view.steal {
                let h = 30.0 + card.techs.len() as f32 * 34.0;
                steal_card(
                    ui,
                    Rect::from_min_size(pos2(r.left() + 10.0, y), vec2(r.width() - 20.0, h)),
                    state,
                    card,
                );
                y += h + 8.0;
            }
        },
    );
}

fn steal_card(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, card: &StealCard) {
    if !ui.is_rect_visible(r) {
        return;
    }
    text(
        ui.painter(),
        r.min + vec2(4.0, 4.0),
        &card.civilization,
        14.0,
        CREAM,
    );
    for (i, (tech, name)) in card.techs.iter().enumerate() {
        let row = Rect::from_min_size(
            r.min + vec2(0.0, 28.0 + i as f32 * 34.0),
            vec2(r.width(), 30.0),
        );
        if button(ui, row, name, false, true) {
            state.requests.push(GameRequest::StealTechnology {
                from: card.from,
                tech: *tech,
            });
        }
    }
}
