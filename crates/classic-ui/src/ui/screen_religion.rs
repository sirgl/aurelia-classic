//! Religion screen bound to the running game (`ReligionView`): the player's faith, pantheon
//! and religion, the belief pickers of the choice it has now (pantheon, founding,
//! enhancing, Reformation) with the generated belief artwork, and the religions of the
//! world. Same frame and panels as the other game screens; every button is a typed
//! `GameRequest` the core validates.
use super::scroll::scroll_panel;
use super::*;
use i18n::{t, tf};
use crate::ui::model::{
    Availability, BeliefCard, FaithChoice, GameRequest, ReligionCard, ReligionView,
};
use classic_rules::ids::BeliefId;
use classic_rules::rules::content::ReligionId;

const SIDE_W: f32 = 290.0;
const CARD: egui::Vec2 = egui::Vec2::new(246.0, 118.0);
const FAITH: Color32 = Color32::from_rgb(236, 226, 170);

/// The player's picks on the religion screen (UI state only).
#[derive(Debug, Clone, Default)]
pub(super) struct FaithPicks {
    step: usize,
    religion: Option<ReligionId>,
    founder: Option<BeliefId>,
    follower: Option<BeliefId>,
    pantheon: Option<BeliefId>,
    enhancer: Option<BeliefId>,
    reformation: Option<BeliefId>,
}

pub(super) fn religion(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, view: &ReligionView) {
    let top = Rect::from_min_size(r.min, vec2(r.width(), 92.0));
    banner(ui, top, state, view);
    let body = Rect::from_min_max(pos2(r.left(), top.bottom() + 14.0), r.max);
    let left = Rect::from_min_size(body.min, vec2(SIDE_W, body.height()));
    let right = Rect::from_min_max(pos2(body.right() - SIDE_W, body.top()), body.max);
    let center = Rect::from_min_max(
        pos2(left.right() + 14.0, body.top()),
        pos2(right.left() - 14.0, body.bottom()),
    );
    own_faith(ui, left, state, view);
    choice(ui, center, state, view);
    world(ui, right, state, view);
}

fn banner(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, view: &ReligionView) {
    panel(ui.painter(), r);
    let art_name = view
        .religion
        .as_ref()
        .and_then(|rel| rel.beliefs.first())
        .or(view.pantheon.as_ref())
        .map(|b| b.art.clone());
    if let Some(name) = art_name {
        let emblem = Rect::from_min_size(pos2(r.right() - 108.0, r.top() + 6.0), vec2(80.0, 80.0));
        art(state, ui, emblem, &name);
    }
    text(
        ui.painter(),
        r.min + vec2(24.0, 12.0),
        t!("classic.ui.screen-religion.title"),
        31.0,
        CREAM,
    );
    let line = if view.pantheon.is_none() && view.religion.is_none() {
        tf!("classic.ui.screen-religion.banner-pantheon", faith = view.faith, per_turn = view.per_turn, pantheon = view.pantheon_cost, prophet = view.prophet_cost, left = view.religions_left)
    } else {
        tf!("classic.ui.screen-religion.banner", faith = view.faith, per_turn = view.per_turn, prophet = view.prophet_cost, left = view.religions_left)
    };
    text(ui.painter(), r.min + vec2(26.0, 58.0), line, 14.0, FAITH);
}

fn own_faith(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, view: &ReligionView) {
    panel(ui.painter(), r);
    text(
        ui.painter(),
        r.min + vec2(42.0, 14.0),
        t!("classic.ui.screen-religion.your-pantheon"),
        10.0,
        GOLD,
    );
    super::world_symbols::religion(ui.painter(),Rect::from_min_size(r.min+vec2(14.,10.),vec2(22.,22.)),"Pantheon",GOLD,BG); // i18n-ok: symbol id
    let list = Rect::from_min_max(pos2(r.left(), r.top() + 34.0), r.max - vec2(0.0, 8.0));
    scroll_panel(
        ui,
        list,
        "own_faith_screen_religion.rs",
        (if view.pantheon.is_some() { 100.0 } else { 30.0 })
            + 26.0
            + view
                .religion
                .as_ref()
                .map(|r| 56.0 + r.beliefs.len() as f32 * 44.0)
                .unwrap_or(40.0),
        |ui, r| {
            let mut y = r.top();
            match &view.pantheon {
                Some(card) => {
                    belief_row(
                        ui,
                        Rect::from_min_size(pos2(r.left() + 10.0, y), vec2(r.width() - 20.0, 92.0)),
                        state,
                        card,
                    );
                    y += 100.0;
                }
                None => {
                    paragraph(
                        ui.painter(),
                        pos2(r.left() + 16.0, y),
                        t!("classic.ui.screen-religion.no-pantheon"),
                        13.0,
                        MUTED,
                        r.width() - 32.0,
                    );
                    y += 30.0;
                }
            }
            text(
                ui.painter(),
                pos2(r.left() + 16.0, y + 6.0),
                t!("classic.ui.screen-religion.your-religion"),
                10.0,
                GOLD,
            );
            y += 26.0;
            let Some(religion) = &view.religion else {
                paragraph(
                    ui.painter(),
                    pos2(r.left() + 16.0, y),
                    t!("classic.ui.screen-religion.no-religion"),
                    13.0,
                    MUTED,
                    r.width() - 32.0,
                );
                return;
            };
            text(
                ui.painter(),
                pos2(r.left() + 50.0, y),
                &religion.name,
                22.0,
                CREAM,
            );
            super::world_symbols::religion(ui.painter(),Rect::from_min_size(pos2(r.left()+14.,y-2.),vec2(28.,28.)),&religion.name,GOLD,BG);
            let facts = if religion.enhanced {
                tf!("classic.ui.screen-religion.facts-enhanced", city = religion.holy_city.as_str(), cities = religion.cities, followers = religion.followers)
            } else {
                tf!("classic.ui.screen-religion.facts", city = religion.holy_city.as_str(), cities = religion.cities, followers = religion.followers)
            };
            paragraph(
                ui.painter(),
                pos2(r.left() + 16.0, y + 30.0),
                &facts,
                11.0,
                MUTED,
                r.width() - 32.0,
            );
            y += 56.0;
            for card in &religion.beliefs {
                small_belief(
                    ui,
                    Rect::from_min_size(pos2(r.left() + 10.0, y), vec2(r.width() - 20.0, 40.0)),
                    state,
                    card,
                );
                y += 44.0;
            }
        },
    );
}

/// A belief with its art, name and text (own pantheon, details).
fn belief_row(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, card: &BeliefCard) {
    if !ui.is_rect_visible(r) {
        return;
    }
    ui.painter().rect_filled(r, 5.0, RAISED);
    let emblem = Rect::from_min_size(
        r.min + vec2(6.0, 6.0),
        vec2(r.height() - 12.0, r.height() - 12.0),
    );
    art(state, ui, emblem, &card.art);
    let x = emblem.right() + 10.0;
    text(
        ui.painter(),
        pos2(x, r.top() + 8.0),
        &card.name,
        15.0,
        CREAM,
    );
    paragraph(
        ui.painter(),
        pos2(x, r.top() + 30.0),
        &card.text,
        11.0,
        MUTED,
        r.right() - x - 8.0,
    );
}

/// A belief in a list: small art, category and name; the text on hover.
fn small_belief(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, card: &BeliefCard) {
    if !ui.is_rect_visible(r) {
        return;
    }
    ui.painter().rect_filled(r, 4.0, RAISED);
    let emblem = Rect::from_min_size(
        r.min + vec2(4.0, 4.0),
        vec2(r.height() - 8.0, r.height() - 8.0),
    );
    art(state, ui, emblem, &card.art);
    text(
        ui.painter(),
        pos2(emblem.right() + 8.0, r.top() + 4.0),
        card.category,
        9.0,
        GOLD,
    );
    text(
        ui.painter(),
        pos2(emblem.right() + 8.0, r.top() + 17.0),
        &card.name,
        13.0,
        CREAM,
    );
    ui.interact(
        r,
        ui.id().with(("belief_hover", card.id.0, r.min.y.to_bits())),
        Sense::hover(),
    )
    .on_hover_text(&card.text);
}

fn choice(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, view: &ReligionView) {
    panel(ui.painter(), r);
    match &view.choice {
        FaithChoice::Pantheon(availability) => pantheon_choice(ui, r, state, view, availability),
        FaithChoice::Found { needs_pantheon } => found_choice(ui, r, state, view, *needs_pantheon),
        FaithChoice::Enhance => enhance_choice(ui, r, state, view),
        FaithChoice::Reformation => reformation_choice(ui, r, state, view),
        FaithChoice::Browse(hint) => {
            paragraph(
                ui.painter(),
                r.min + vec2(18.0, 14.0),
                hint,
                13.0,
                MUTED,
                r.width() - 36.0,
            );
            purchases(
                ui,
                Rect::from_min_max(r.min + vec2(10.0, 46.0), r.max - vec2(10.0, 10.0)),
                state,
                view,
            );
        }
    }
}

fn pantheon_choice(
    ui: &mut egui::Ui,
    r: Rect,
    state: &mut ScreenState,
    view: &ReligionView,
    availability: &Availability,
) {
    text(
        ui.painter(),
        r.min + vec2(18.0, 14.0),
        t!("classic.ui.screen-religion.found-pantheon-heading"),
        10.0,
        GOLD,
    );
    let grid = Rect::from_min_max(
        r.min + vec2(10.0, 36.0),
        pos2(r.right() - 10.0, r.bottom() - 64.0),
    );
    let picked = state.faith_picks.pantheon;
    if let Some(id) = belief_grid(ui, grid, state, &view.pantheons, picked, "pantheon") {
        state.faith_picks.pantheon = Some(id);
    }
    let label = tf!("classic.ui.screen-religion.found-pantheon", faith = view.pantheon_cost);
    let action = action_rect(r);
    let ready = availability.is_ready() && state.faith_picks.pantheon.is_some();
    if button(ui, action, &label, false, ready) {
        if let Some(belief) = state.faith_picks.pantheon {
            state.requests.push(GameRequest::FoundPantheon(belief));
        }
    }
    blocked_line(
        ui,
        action,
        availability,
        state.faith_picks.pantheon.is_none(),
        t!("classic.ui.screen-religion.choose-belief"),
    );
}

fn found_choice(
    ui: &mut egui::Ui,
    r: Rect,
    state: &mut ScreenState,
    view: &ReligionView,
    needs_pantheon: bool,
) {
    text(
        ui.painter(),
        r.min + vec2(18.0, 14.0),
        t!("classic.ui.screen-religion.found-religion-heading"),
        10.0,
        GOLD,
    );
    let mut steps = vec![
        t!("classic.ui.screen-religion.step-religion"),
        t!("classic.ui.screen-religion.step-founder"),
        t!("classic.ui.screen-religion.step-follower"),
    ];
    if needs_pantheon {
        steps.push(t!("classic.ui.screen-religion.step-pantheon"));
    }
    let step = state.faith_picks.step.min(steps.len() - 1);
    step_tabs(ui, r, state, &steps, step);
    let grid = Rect::from_min_max(
        r.min + vec2(10.0, 76.0),
        pos2(r.right() - 10.0, r.bottom() - 64.0),
    );
    match step {
        0 => {
            if let Some(id) =
                religion_names(ui, grid, &view.free_religions, state.faith_picks.religion)
            {
                state.faith_picks.religion = Some(id);
            }
        }
        1 => {
            if let Some(id) = belief_grid(
                ui,
                grid,
                state,
                &view.founders,
                state.faith_picks.founder,
                "founder",
            ) {
                state.faith_picks.founder = Some(id);
            }
        }
        2 => {
            if let Some(id) = belief_grid(
                ui,
                grid,
                state,
                &view.followers,
                state.faith_picks.follower,
                "follower",
            ) {
                state.faith_picks.follower = Some(id);
            }
        }
        _ => {
            if let Some(id) = belief_grid(
                ui,
                grid,
                state,
                &view.pantheons,
                state.faith_picks.pantheon,
                "pantheon",
            ) {
                state.faith_picks.pantheon = Some(id);
            }
        }
    }
    let p = &state.faith_picks;
    let complete = p.religion.is_some()
        && p.founder.is_some()
        && p.follower.is_some()
        && (!needs_pantheon || p.pantheon.is_some());
    let action = action_rect(r);
    if button(ui, action, t!("classic.ui.screen-religion.found-religion"), false, complete) {
        if let (Some(religion), Some(founder), Some(follower)) = (p.religion, p.founder, p.follower)
        {
            let pantheon = if needs_pantheon { p.pantheon } else { None };
            state.requests.push(GameRequest::FoundReligion {
                religion,
                founder,
                follower,
                pantheon,
            });
        }
    }
    if !complete {
        blocked_line(
            ui,
            action,
            &Availability::Ready,
            true,
            t!("classic.ui.screen-religion.choose-all"),
        );
    }
}

fn enhance_choice(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, view: &ReligionView) {
    text(
        ui.painter(),
        r.min + vec2(18.0, 14.0),
        t!("classic.ui.screen-religion.enhance-heading"),
        10.0,
        GOLD,
    );
    let steps = [t!("classic.ui.screen-religion.step-follower"), t!("classic.ui.screen-religion.step-enhancer")];
    let step = state.faith_picks.step.min(1);
    step_tabs(ui, r, state, &steps, step);
    let grid = Rect::from_min_max(
        r.min + vec2(10.0, 76.0),
        pos2(r.right() - 10.0, r.bottom() - 64.0),
    );
    if step == 0 {
        if let Some(id) = belief_grid(
            ui,
            grid,
            state,
            &view.followers,
            state.faith_picks.follower,
            "follower",
        ) {
            state.faith_picks.follower = Some(id);
        }
    } else if let Some(id) = belief_grid(
        ui,
        grid,
        state,
        &view.enhancers,
        state.faith_picks.enhancer,
        "enhancer",
    ) {
        state.faith_picks.enhancer = Some(id);
    }
    let action = action_rect(r);
    let picks = (state.faith_picks.follower, state.faith_picks.enhancer);
    let complete = picks.0.is_some() && picks.1.is_some();
    if button(ui, action, t!("classic.ui.screen-religion.enhance"), false, complete) {
        if let (Some(follower), Some(enhancer)) = picks {
            state
                .requests
                .push(GameRequest::EnhanceReligion { follower, enhancer });
        }
    }
}

fn reformation_choice(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, view: &ReligionView) {
    text(
        ui.painter(),
        r.min + vec2(18.0, 14.0),
        t!("classic.ui.screen-religion.reformation-heading"),
        10.0,
        GOLD,
    );
    let grid = Rect::from_min_max(
        r.min + vec2(10.0, 36.0),
        pos2(r.right() - 10.0, r.bottom() - 64.0),
    );
    if let Some(id) = belief_grid(
        ui,
        grid,
        state,
        &view.reformations,
        state.faith_picks.reformation,
        "reformation",
    ) {
        state.faith_picks.reformation = Some(id);
    }
    let action = action_rect(r);
    let pick = state.faith_picks.reformation;
    if button(ui, action, t!("classic.ui.screen-religion.reformation"), false, pick.is_some()) {
        if let Some(belief) = pick {
            state.requests.push(GameRequest::AddReformation(belief));
        }
    }
}

/// What faith can buy: one row per item with its city, price and Buy button.
fn purchases(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, view: &ReligionView) {
    text(
        ui.painter(),
        r.min + vec2(8.0, 0.0),
        t!("classic.ui.screen-religion.buy-heading"),
        10.0,
        GOLD,
    );
    let list = Rect::from_min_max(r.min + vec2(0.0, 20.0), r.max);
    ui.scope_builder(egui::UiBuilder::new().max_rect(list), |ui| {
        egui::ScrollArea::vertical()
            .id_salt("faith_offers")
            .show(ui, |ui| {
                let row_h = 46.0;
                let (area, _) = ui.allocate_exact_size(
                    vec2(
                        list.width() - 12.0,
                        view.purchases.len() as f32 * (row_h + 6.0),
                    ),
                    Sense::hover(),
                );
                for (i, offer) in view.purchases.iter().enumerate() {
                    let row = Rect::from_min_size(
                        area.min + vec2(0.0, i as f32 * (row_h + 6.0)),
                        vec2(area.width(), row_h),
                    );
                    ui.painter().rect_filled(row, 4.0, RAISED);
                    let emblem = Rect::from_min_size(
                        row.min + vec2(4.0, 4.0),
                        vec2(row_h - 8.0, row_h - 8.0),
                    );
                    art(state, ui, emblem, &offer.art);
                    text(
                        ui.painter(),
                        pos2(emblem.right() + 10.0, row.top() + 6.0),
                        &offer.name,
                        14.0,
                        CREAM,
                    );
                    let detail = match (&offer.availability, offer.cost) {
                        (Availability::Ready, Some(cost)) => {
                            tf!("classic.ui.screen-religion.offer-detail", cost = cost, city = offer.city.as_str())
                        }
                        (Availability::Blocked(reason), _) => reason.clone(),
                        (Availability::Ready, None) => String::new(),
                    };
                    let color = if offer.availability.is_ready() {
                        FAITH
                    } else {
                        MUTED
                    };
                    text(
                        ui.painter(),
                        pos2(emblem.right() + 10.0, row.top() + 25.0),
                        detail,
                        11.0,
                        color,
                    );
                    let buy = Rect::from_min_size(
                        pos2(row.right() - 96.0, row.top() + 8.0),
                        vec2(88.0, 30.0),
                    );
                    if button(ui, buy, t!("classic.ui.screen-religion.buy"), false, offer.availability.is_ready()) {
                        state.requests.push(GameRequest::BuyWithFaith {
                            city_tile: offer.city_tile,
                            item: offer.item,
                        });
                    }
                }
            });
    });
}

fn action_rect(r: Rect) -> Rect {
    Rect::from_min_size(
        pos2(r.right() - 270.0, r.bottom() - 52.0),
        vec2(250.0, 38.0),
    )
}

fn blocked_line(
    ui: &mut egui::Ui,
    action: Rect,
    availability: &Availability,
    unpicked: bool,
    hint: &str,
) {
    let reason = match availability {
        Availability::Blocked(reason) => reason.clone(),
        Availability::Ready if unpicked => hint.to_string(),
        Availability::Ready => return,
    };
    ui.painter().text(
        pos2(action.left() - 14.0, action.center().y),
        Align2::RIGHT_CENTER,
        reason,
        FontId::proportional(12.0),
        GOLD,
    );
}

fn step_tabs(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, steps: &[&str], current: usize) {
    let width = ((r.width() - 20.0) / steps.len() as f32 - 8.0).min(170.0);
    for (i, label) in steps.iter().enumerate() {
        let tab = Rect::from_min_size(
            r.min + vec2(10.0 + i as f32 * (width + 8.0), 34.0),
            vec2(width, 32.0),
        );
        if button(ui, tab, label, i == current, true) {
            state.faith_picks.step = i;
        }
    }
}

/// Belief cards in a scrolling grid; returns the clicked belief.
fn belief_grid(
    ui: &mut egui::Ui,
    r: Rect,
    state: &mut ScreenState,
    cards: &[BeliefCard],
    picked: Option<BeliefId>,
    salt: &'static str,
) -> Option<BeliefId> {
    if cards.is_empty() {
        paragraph(
            ui.painter(),
            r.min + vec2(8.0, 8.0),
            t!("classic.ui.screen-religion.no-belief-left"),
            13.0,
            MUTED,
            r.width(),
        );
        return None;
    }
    let columns = ((r.width() - 4.0) / (CARD.x + 8.0)).floor().max(1.0) as usize;
    let rows = cards.len().div_ceil(columns);
    let card_w = (r.width() - 12.0 - 8.0 * (columns as f32 - 1.0)) / columns as f32;
    let size = vec2(card_w, CARD.y);
    let mut clicked = None;
    ui.scope_builder(egui::UiBuilder::new().max_rect(r), |ui| {
        egui::ScrollArea::vertical().id_salt(salt).show(ui, |ui| {
            let (area, _) = ui.allocate_exact_size(
                vec2(r.width() - 12.0, rows as f32 * (CARD.y + 8.0)),
                Sense::hover(),
            );
            for (i, card) in cards.iter().enumerate() {
                let cell = Rect::from_min_size(
                    area.min
                        + vec2(
                            (i % columns) as f32 * (card_w + 8.0),
                            (i / columns) as f32 * (CARD.y + 8.0),
                        ),
                    size,
                );
                if belief_card(ui, cell, state, card, picked == Some(card.id)) {
                    clicked = Some(card.id);
                }
            }
        });
    });
    clicked
}

fn belief_card(
    ui: &mut egui::Ui,
    r: Rect,
    state: &mut ScreenState,
    card: &BeliefCard,
    picked: bool,
) -> bool {
    let response = ui.interact(r, ui.id().with(("belief_card", card.id.0)), Sense::click());
    crate::ui::pedia_link::mark(ui, r, crate::ui::pedia::PediaRef::belief(card.id));
    let fill = if picked {
        Color32::from_rgb(37, 65, 66)
    } else if response.hovered() {
        Color32::from_rgb(31, 55, 58)
    } else {
        RAISED
    };
    ui.painter().rect_filled(r, 5.0, fill);
    ui.painter().rect_stroke(
        r,
        5.0,
        Stroke::new(
            if picked { 1.6 } else { 1.0 },
            if picked {
                GOLD
            } else {
                Color32::from_rgb(60, 78, 76)
            },
        ),
        egui::StrokeKind::Inside,
    );
    let emblem = Rect::from_min_size(r.min + vec2(8.0, 8.0), vec2(64.0, 64.0));
    art(state, ui, emblem, &card.art);
    let x = emblem.right() + 8.0;
    text(
        ui.painter(),
        pos2(x, r.top() + 8.0),
        card.category,
        9.0,
        GOLD,
    );
    let name = ui.painter().layout(
        card.name.clone(),
        FontId::proportional(15.0),
        CREAM,
        r.right() - x - 6.0,
    );
    ui.painter().galley(pos2(x, r.top() + 22.0), name, CREAM);
    paragraph(
        ui.painter(),
        pos2(r.left() + 8.0, r.top() + 78.0),
        &card.text,
        11.0,
        MUTED,
        r.width() - 16.0,
    );
    response.clicked()
}

fn religion_names(
    ui: &mut egui::Ui,
    r: Rect,
    names: &[(ReligionId, String)],
    picked: Option<ReligionId>,
) -> Option<ReligionId> {
    let mut clicked = None;
    let columns = 3usize;
    let width = (r.width() - 16.0) / columns as f32;
    for (i, (id, name)) in names.iter().enumerate() {
        let cell = Rect::from_min_size(
            r.min
                + vec2(
                    (i % columns) as f32 * (width + 8.0),
                    (i / columns) as f32 * 50.0,
                ),
            vec2(width - 8.0, 42.0),
        );
        if button(ui, cell, "", picked == Some(*id), true) {
            clicked = Some(*id);
        }
        let symbol=Rect::from_min_size(cell.min+vec2(6.,7.),vec2(28.,28.));
        let ground=if picked==Some(*id) { TEAL } else { RAISED };
        super::world_symbols::religion(ui.painter(),symbol,name,GOLD,ground);
        let label=Rect::from_min_max(cell.min+vec2(40.,4.),cell.max-vec2(4.,4.));
        paragraph(ui.painter(),label.min,name,12.,CREAM,label.width());
    }
    clicked
}

fn world(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, view: &ReligionView) {
    panel(ui.painter(), r);
    text(
        ui.painter(),
        r.min + vec2(16.0, 14.0),
        t!("classic.ui.screen-religion.world-heading"),
        10.0,
        GOLD,
    );
    if view.world.is_empty() {
        paragraph(
            ui.painter(),
            r.min + vec2(16.0, 36.0),
            t!("classic.ui.screen-religion.no-world-religion"),
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
        "world_screen_religion.rs",
        view.world.len() as f32 * 82.0,
        |ui, r| {
            let mut y = r.top();
            for card in &view.world {
                let h = 74.0;
                religion_row(
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

fn religion_row(ui: &mut egui::Ui, r: Rect, _state: &mut ScreenState, card: &ReligionCard) {
    if !ui.is_rect_visible(r) {
        return;
    }
    ui.painter().rect_filled(r, 5.0, RAISED);
    if card.mine {
        ui.painter()
            .rect_stroke(r, 5.0, Stroke::new(1.0, GOLD), egui::StrokeKind::Inside);
    }
    let emblem = Rect::from_min_size(
        r.min + vec2(6.0, 6.0),
        vec2(r.height() - 12.0, r.height() - 12.0),
    );
    super::world_symbols::religion(ui.painter(),emblem.shrink(8.),&card.name,GOLD,RAISED);
    let x = emblem.right() + 10.0;
    text(
        ui.painter(),
        pos2(x, r.top() + 6.0),
        &card.name,
        16.0,
        CREAM,
    );
    text(
        ui.painter(),
        pos2(x, r.top() + 28.0),
        &card.founder,
        11.0,
        GOLD,
    );
    let facts = tf!("classic.ui.screen-religion.world-facts", city = card.holy_city.as_str(), cities = card.cities, followers = card.followers);
    text(ui.painter(), pos2(x, r.top() + 46.0), facts, 10.0, MUTED);
    let beliefs: Vec<&str> = card.beliefs.iter().map(|b| b.name.as_str()).collect();
    ui.interact(r, ui.id().with(("religion_row", card.id.0)), Sense::hover())
        .on_hover_text(beliefs.join(", "));
}

/// Belief art the screen shows now (offscreen captures decode it synchronously).
pub(super) fn visible_art(view: &ReligionView) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    out.extend(view.pantheon.iter().map(|b| b.art.clone()));
    if let Some(r) = &view.religion {
        out.extend(r.beliefs.iter().map(|b| b.art.clone()));
    }
    let list = match &view.choice {
        FaithChoice::Pantheon(_) => &view.pantheons,
        FaithChoice::Found { .. } => &view.founders,
        FaithChoice::Enhance => &view.followers,
        FaithChoice::Reformation => &view.reformations,
        FaithChoice::Browse(_) => &view.pantheons,
    };
    out.extend(list.iter().take(12).map(|b| b.art.clone()));
    if matches!(view.choice, FaithChoice::Browse(_)) {
        out.extend(view.purchases.iter().take(12).map(|o| o.art.clone()));
    }
    for r in &view.world {
        out.extend(r.beliefs.iter().take(2).map(|b| b.art.clone()));
    }
    out
}

#[cfg(test)]
mod symbol_tests {
    use super::*;
    #[test]
    fn religion_symbol_cards_keep_their_selection_action() {
        let ctx=egui::Context::default();
        let names=vec![(ReligionId(12),"Zoroastrianism".to_owned())];
        let mut selected=None;
        let point=pos2(85.,40.); // Label area, not just the card's empty border.
        for events in [vec![],vec![egui::Event::PointerMoved(point)],vec![egui::Event::PointerButton{pos:point,button:egui::PointerButton::Primary,pressed:true,modifiers:Default::default()}],vec![egui::Event::PointerButton{pos:point,button:egui::PointerButton::Primary,pressed:false,modifiers:Default::default()}]] {
            let mut output=ctx.run_ui(egui::RawInput {screen_rect:Some(Rect::from_min_size(pos2(0.,0.),vec2(600.,400.))),events,..Default::default()},|ui| {
                if let Some(id)=religion_names(ui,Rect::from_min_size(pos2(20.,20.),vec2(530.,300.)),&names,selected) {selected=Some(id);}
            });
            output.textures_delta.clear();
        }
        assert_eq!(selected,Some(ReligionId(12)));
    }
}
