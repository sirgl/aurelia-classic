//! Culture overview bound to the running game (`CultureView`): tourism and the cultural
//! victory count, the ideology's public opinion with the switch, influence on every
//! nation met, the great works in their buildings' slots with themes, and the
//! finished digs with their choices. Same frame and panels as the other game screens;
//! every button is a typed `GameRequest` the core validates.
use super::scroll::{scroll_body, scroll_panel};
use super::*;
use i18n::{t, tf};
use crate::ui::model::{
    CultureView, DigCard, GameRequest, InfluenceRow, WorkBuildingCard,
};

const SIDE_W: f32 = 360.0;
const TOURISM: Color32 = Color32::from_rgb(214, 196, 240);

pub(super) fn culture(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, view: &CultureView) {
    let top = Rect::from_min_size(r.min, vec2(r.width(), 92.0));
    banner(ui, top, state, view);
    let body = Rect::from_min_max(pos2(r.left(), top.bottom() + 14.0), r.max);
    let left = Rect::from_min_size(body.min, vec2(SIDE_W, body.height()));
    let right = Rect::from_min_max(pos2(body.right() - SIDE_W, body.top()), body.max);
    let center = Rect::from_min_max(
        pos2(left.right() + 14.0, body.top()),
        pos2(right.left() - 14.0, body.bottom()),
    );
    influence(ui, left, view);
    works(ui, center, state, view);
    digs(ui, right, state, view);
}

fn banner(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, view: &CultureView) {
    panel(ui.painter(), r);
    text(
        ui.painter(),
        r.min + vec2(24.0, 12.0),
        t!("classic.ui.screen-culture.title"),
        31.0,
        CREAM,
    );
    let line = tf!("classic.ui.screen-culture.banner-line",
        tourism = view.tourism,
        culture = view.culture_per_turn,
        influential = view.influential_on,
        needed = view.needed_for_victory
    );
    text(ui.painter(), r.min + vec2(26.0, 58.0), line, 14.0, TOURISM);
    let ideology = Rect::from_min_size(pos2(r.right() - 470.0, r.top() + 14.0), vec2(250.0, 64.0));
    text(ui.painter(), ideology.min, t!("classic.ui.screen-culture.ideology"), 10.0, GOLD);
    paragraph(
        ui.painter(),
        ideology.min + vec2(0.0, 18.0),
        &view.ideology,
        13.0,
        CREAM,
        ideology.width(),
    );
    let label = match &view.preferred {
        Some(name) => tf!("classic.ui.screen-culture.switch-to", name = name.as_str()),
        None => t!("classic.ui.screen-culture.switch").to_string(),
    };
    let button_rect =
        Rect::from_min_size(pos2(r.right() - 206.0, r.top() + 26.0), vec2(186.0, 38.0));
    if ready_button(ui, button_rect, &label, &view.switch) {
        state.requests.push(GameRequest::SwitchIdeology);
    }
}

fn influence(ui: &mut egui::Ui, r: Rect, view: &CultureView) {
    panel(ui.painter(), r);
    text(
        ui.painter(),
        r.min + vec2(16.0, 14.0),
        t!("classic.ui.screen-culture.influence"),
        10.0,
        GOLD,
    );
    if view.influence.is_empty() {
        paragraph(
            ui.painter(),
            r.min + vec2(16.0, 36.0),
            t!("classic.ui.screen-culture.no-nation-met"),
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
        "influence_screen_culture.rs",
        view.influence.len() as f32 * 64.0,
        |ui, r| {
            let mut y = r.top();
            for row in &view.influence {
                influence_row(
                    ui,
                    Rect::from_min_size(pos2(r.left() + 10.0, y), vec2(r.width() - 20.0, 58.0)),
                    row,
                );
                y += 64.0;
            }
        },
    );
}

fn influence_row(ui: &mut egui::Ui, r: Rect, row: &InfluenceRow) {
    if !ui.is_rect_visible(r) {
        return;
    }
    ui.painter().rect_filled(r, 5.0, RAISED);
    text(
        ui.painter(),
        r.min + vec2(10.0, 6.0),
        &row.nation,
        15.0,
        CREAM,
    );
    ui.painter().text(
        pos2(r.right() - 10.0, r.top() + 8.0),
        Align2::RIGHT_TOP,
        row.level,
        FontId::proportional(14.0),
        TOURISM,
    );
    let facts = tf!("classic.ui.screen-culture.influence-facts",
        percent = row.percent,
        per_turn = row.per_turn,
        trend = row.trend,
        theirs = row.their_level
    );
    text(ui.painter(), r.min + vec2(10.0, 32.0), facts, 11.0, MUTED);
}

fn works(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, view: &CultureView) {
    panel(ui.painter(), r);
    text(
        ui.painter(),
        r.min + vec2(18.0, 14.0),
        t!("classic.ui.screen-culture.great-works"),
        10.0,
        GOLD,
    );
    if view.buildings.is_empty() {
        paragraph(
            ui.painter(),
            r.min + vec2(18.0, 38.0),
            t!("classic.ui.screen-culture.no-slots"),
            13.0,
            MUTED,
            r.width() - 36.0,
        );
        return;
    }
    let list = Rect::from_min_max(r.min + vec2(10.0, 34.0), r.max - vec2(10.0, 10.0));
    scroll_body(ui, list, "culture_works", |ui| {
        let width = ui.available_width().max(1.);
        let columns = work_columns(width);
        for card in &view.buildings {
            let rows = card.works.len().max(1).div_ceil(columns);
            let height = 70. + rows as f32 * 152.;
            let (row, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
            if ui.is_rect_visible(row) {
                building_row(ui, row, state, card, columns);
            }
            ui.add_space(8.);
        }
    });
}
fn work_columns(width: f32) -> usize {
    ((width - 20.) / 132.).floor().max(1.) as usize
}
fn building_row(
    ui: &mut egui::Ui,
    r: Rect,
    state: &mut ScreenState,
    card: &WorkBuildingCard,
    columns: usize,
) {
    ui.painter().rect_filled(r, 5., RAISED);
    let emblem = Rect::from_min_size(r.min + vec2(8., 8.), vec2(48., 48.));
    art(state, ui, emblem, &card.art);
    let header = Rect::from_min_max(
        pos2(emblem.right() + 10., r.top() + 8.),
        pos2(r.right() - 10., r.top() + 62.),
    );
    clipped_work_text(ui, header.min, &card.building, 15., CREAM, header);
    let mut caption = tf!("classic.ui.screen-culture.work-caption", city = card.city.as_str(), slot = card.slot);
    if let Some(bonus) = card.theme {
        caption = tf!("classic.ui.screen-culture.work-caption-themed", caption = caption.as_str(), bonus = bonus);
    }
    clipped_work_text(
        ui,
        header.min + vec2(0., 25.),
        &caption,
        11.,
        if card.theme.is_some() { GOLD } else { MUTED },
        header,
    );
    ui.interact(
        header,
        ui.id().with(("work_building", &card.city, &card.building)),
        Sense::hover(),
    )
    .on_hover_text(format!("{}\n{caption}", card.building));
    let width = (r.width() - 20.) / columns as f32;
    for (i, work) in card.works.iter().enumerate() {
        let slot = Rect::from_min_size(
            r.min
                + vec2(
                    10. + (i % columns) as f32 * width,
                    66. + (i / columns) as f32 * 152.,
                ),
            vec2(width - 7., 144.),
        );
        if !ui.is_rect_visible(slot) {
            continue;
        }
        ui.painter()
            .rect_filled(slot, 5., Color32::from_rgb(19, 36, 41));
        let image = Rect::from_min_size(slot.min + vec2(5., 5.), vec2(slot.width() - 10., 98.));
        if let Some(work) = work {
            if let Some(key) = &work.art {
                art(state, ui, image, key);
            }
            clipped_work_text(ui, slot.min + vec2(7., 107.), work.class, 12., CREAM, slot);
            clipped_work_text(
                ui,
                slot.min + vec2(7., 125.),
                &work.provenance,
                10.,
                MUTED,
                slot,
            );
            ui.interact(slot, ui.id().with(("great_work", work.id)), Sense::hover())
                .on_hover_text(format!("{}\n{}", work.class, work.provenance));
        } else {
            ui.painter().rect_stroke(
                image,
                4.,
                Stroke::new(1., Color32::from_rgb(66, 80, 81)),
                egui::StrokeKind::Inside,
            );
            ui.painter().text(
                image.center(),
                Align2::CENTER_CENTER,
                t!("classic.ui.screen-culture.empty-slot"),
                FontId::proportional(12.),
                MUTED,
            );
            clipped_work_text(ui, slot.min + vec2(7., 110.), card.slot, 11., MUTED, slot);
        }
    }
}
fn clipped_work_text(
    ui: &egui::Ui,
    at: egui::Pos2,
    value: &str,
    size: f32,
    color: Color32,
    clip: Rect,
) {
    let width = (clip.right() - at.x - 4.).max(1.);
    let mut job = egui::text::LayoutJob::simple_singleline(
        value.to_owned(),
        FontId::proportional(size),
        color,
    );
    job.wrap.max_width = width;
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    let galley = ui.painter().layout_job(job);
    ui.painter()
        .with_clip_rect(ui.clip_rect().intersect(clip))
        .galley(at, galley, color);
}

fn digs(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, view: &CultureView) {
    panel(ui.painter(), r);
    text(
        ui.painter(),
        r.min + vec2(16.0, 14.0),
        t!("classic.ui.screen-culture.archaeology"),
        10.0,
        GOLD,
    );
    let sites = match view.sites {
        0 => t!("classic.ui.screen-culture.no-sites").to_string(),
        n => tf!("classic.ui.screen-culture.sites", n = n),
    };
    paragraph(
        ui.painter(),
        r.min + vec2(16.0, 34.0),
        &sites,
        12.0,
        MUTED,
        r.width() - 32.0,
    );
    let list = Rect::from_min_max(pos2(r.left(), r.top() + 62.0), r.max - vec2(0.0, 8.0));
    scroll_panel(
        ui,
        list,
        "digs_screen_culture.rs",
        (if view.free_great_people.is_empty() {
            0.0
        } else {
            48.0 + view.free_great_people.len() as f32 * 34.0
        }) + view
            .digs
            .iter()
            .map(|c| 48.0 + c.choices.len() as f32 * 34.0)
            .sum::<f32>(),
        |ui, r| {
            let mut y = r.top();
            if !view.free_great_people.is_empty() {
                let h = 40.0 + 34.0 * view.free_great_people.len() as f32;
                let card = Rect::from_min_size(pos2(r.left() + 10.0, y), vec2(r.width() - 20.0, h));
                free_great_person_card(ui, card, state, view);
                y += h + 8.0;
            }
            for card in &view.digs {
                let h = 40.0 + 34.0 * card.choices.len() as f32;
                dig_card(
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

/// The great person kinds a free great person may be, one button each.
fn free_great_person_card(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, view: &CultureView) {
    if !ui.is_rect_visible(r) {
        return;
    }
    ui.painter().rect_filled(r, 5.0, RAISED);
    text(
        ui.painter(),
        r.min + vec2(10.0, 8.0),
        t!("classic.ui.screen-culture.free-great-person"),
        14.0,
        CREAM,
    );
    for (i, (kind, label)) in view.free_great_people.iter().enumerate() {
        let b = Rect::from_min_size(
            pos2(r.left() + 10.0, r.top() + 36.0 + i as f32 * 34.0),
            vec2(r.width() - 20.0, 30.0),
        );
        if button(ui, b, label, false, true) {
            state
                .requests
                .push(GameRequest::ChooseFreeGreatPerson(*kind));
        }
    }
}

fn dig_card(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, card: &DigCard) {
    if !ui.is_rect_visible(r) {
        return;
    }
    ui.painter().rect_filled(r, 5.0, RAISED);
    text(
        ui.painter(),
        r.min + vec2(10.0, 8.0),
        &card.label,
        14.0,
        CREAM,
    );
    for (i, (choice, label)) in card.choices.iter().enumerate() {
        let b = Rect::from_min_size(
            pos2(r.left() + 10.0, r.top() + 36.0 + i as f32 * 34.0),
            vec2(r.width() - 20.0, 30.0),
        );
        if button(ui, b, label, false, true) {
            state.requests.push(GameRequest::ChooseArchaeology {
                tile: card.tile,
                choice: *choice,
            });
        }
    }
}

/// Building art the screen shows now (offscreen captures decode it synchronously).
pub(super) fn visible_art(view: &CultureView) -> Vec<String> {
    view.buildings
        .iter()
        .take(12)
        .flat_map(|b| {
            std::iter::once(b.art.clone())
                .chain(b.works.iter().flatten().filter_map(|w| w.art.clone()))
        })
        .collect()
}
