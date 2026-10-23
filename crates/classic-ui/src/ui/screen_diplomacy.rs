//! Diplomacy screen bound to the running game (`DiplomacyView`): the civilizations met
//! with their emblems, the leader portrait, war or peace, treaties, the war, peace,
//! denunciation and open-borders buttons, a trade panel that drafts a deal (`DealDraft`)
//! and the offers waiting for the player's answer. Every button is a typed
//! `GameRequest` the core validates.
use super::civilizations::{Asset, CivilizationArt};
use super::scroll::scroll_panel;
use super::*;
use i18n::{t, tf};
use crate::ui::model::{CivCard, DealDraft, DealOption, DiplomacyView, GameRequest, OfferCard};
use classic_rules::ids::PlayerId;
use classic_rules::world::relations::DealItem;

const LIST_W: f32 = 290.0;
const WAR: Color32 = Color32::from_rgb(214, 96, 84);
const PEACE: Color32 = Color32::from_rgb(137, 188, 83);

/// The civilization looked at, the deal drafted with it, and the leader artwork.
#[derive(Default)]
pub(super) struct DiplomacyState {
    pick: Option<PlayerId>,
    draft: Option<DealDraft>,
    leaders: CivilizationArt,
}

impl DiplomacyState {
    /// Offscreen captures: the emblems of the list and the selected leader's portrait.
    pub(super) fn preload(&mut self, ctx: &egui::Context, view: &DiplomacyView) {
        let slugs: Vec<&str> = view.civs.iter().map(|c| c.art.as_str()).collect();
        if let Some(selected) = self.selected(view) {
            self.leaders.preload(ctx, &slugs, &selected.art);
        }
    }

    fn selected<'v>(&self, view: &'v DiplomacyView) -> Option<&'v CivCard> {
        let picked = view.civs.iter().find(|c| Some(c.player) == self.pick);
        picked.or(view.civs.first())
    }

    /// The draft with `partner`; another partner starts a new one.
    fn draft_for(&mut self, partner: PlayerId) -> &mut DealDraft {
        let same = self.draft.as_ref().map(|d| d.partner) == Some(partner);
        if !same {
            self.draft = Some(DealDraft::new(partner));
        }
        self.draft.get_or_insert_with(|| DealDraft::new(partner))
    }
}

impl ScreenState {
    pub fn deal_draft(&self) -> Option<&DealDraft> {
        self.diplomacy.draft.as_ref()
    }
}

pub(super) fn diplomacy(
    ui: &mut egui::Ui,
    r: Rect,
    state: &mut ScreenState,
    view: &DiplomacyView,
    message: Option<&str>,
) {
    let top = Rect::from_min_size(r.min, vec2(r.width(), 92.0));
    let line = tf!("classic.ui.screen-diplomacy.banner-line", met = view.civs.len(), gold = view.gold, offers = view.offers.len());
    world_banner(ui, top, t!("classic.ui.screen-diplomacy.title"), &line, message);
    let body = Rect::from_min_max(pos2(r.left(), top.bottom() + 14.0), r.max);
    diplomacy_body(ui, body, state, view);
}

/// The civilizations met, the one looked at with its deal table, and the
/// offers (the Diplomacy screen below its banner, the overview's F4 tab).
pub(super) fn diplomacy_body(
    ui: &mut egui::Ui,
    body: Rect,
    state: &mut ScreenState,
    view: &DiplomacyView,
) {
    let left = Rect::from_min_size(body.min, vec2(LIST_W, body.height()));
    let right = Rect::from_min_max(pos2(body.right() - LIST_W, body.top()), body.max);
    let center = Rect::from_min_max(
        pos2(left.right() + 14.0, body.top()),
        pos2(right.left() - 14.0, body.bottom()),
    );
    civ_list(ui, left, state, view);
    offers(ui, right, state, view);
    match state.diplomacy.selected(view) {
        Some(card) => civ_detail(ui, center, state, card, view),
        None => nobody_met(ui, center),
    }
}

fn nobody_met(ui: &mut egui::Ui, r: Rect) {
    panel(ui.painter(), r);
    paragraph(
        ui.painter(),
        r.min + vec2(18.0, 18.0),
        t!("classic.ui.screen-diplomacy.none-met"),
        14.0,
        MUTED,
        r.width() - 36.0,
    );
}

fn civ_list(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, view: &DiplomacyView) {
    panel(ui.painter(), r);
    heading(ui, r, t!("classic.ui.screen-diplomacy.nations-met"));
    let picked = state.diplomacy.selected(view).map(|c| c.player);
    let brightness = state.brightness;
    let list = Rect::from_min_max(pos2(r.left(), r.top() + 36.0), r.max - vec2(0.0, 8.0));
    scroll_panel(
        ui,
        list,
        "civ_list_screen_diplomacy.rs",
        view.civs.len() as f32 * 70.0,
        |ui, r| {
            let mut y = r.top();
            for card in &view.civs {
                let row =
                    Rect::from_min_size(pos2(r.left() + 10.0, y), vec2(r.width() - 20.0, 64.0));
                let leaders = &mut state.diplomacy.leaders;
                if civ_row(
                    ui,
                    row,
                    leaders,
                    card,
                    picked == Some(card.player),
                    brightness,
                ) {
                    state.diplomacy.pick = Some(card.player);
                }
                y += 70.0;
            }
        },
    );
}

fn civ_row(
    ui: &mut egui::Ui,
    r: Rect,
    leaders: &mut CivilizationArt,
    card: &CivCard,
    picked: bool,
    brightness: f32,
) -> bool {
    if !ui.is_rect_visible(r) {
        return false;
    }
    let response = ui.interact(r, ui.id().with(("civ_row", card.player.0)), Sense::click());
    let fill = if picked {
        Color32::from_rgb(37, 65, 66)
    } else if response.hovered() {
        Color32::from_rgb(31, 55, 58)
    } else {
        RAISED
    };
    ui.painter().rect_filled(r, 5.0, fill);
    if picked {
        let stroke = Stroke::new(1.3, GOLD);
        ui.painter()
            .rect_stroke(r, 5.0, stroke, egui::StrokeKind::Inside);
    }
    let emblem = Rect::from_min_size(r.min + vec2(4.0, 4.0), vec2(56.0, 56.0));
    leaders.paint(ui, emblem, &card.art, Asset::Emblem, brightness);
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
        pos2(x, r.top() + 27.0),
        &card.leader,
        11.0,
        MUTED,
    );
    let color = if card.at_war { WAR } else { PEACE };
    text(
        ui.painter(),
        pos2(x, r.top() + 44.0),
        &card.state,
        11.0,
        color,
    );
    response.clicked()
}

fn civ_detail(
    ui: &mut egui::Ui,
    r: Rect,
    state: &mut ScreenState,
    card: &CivCard,
    view: &DiplomacyView,
) {
    panel(ui.painter(), r);
    portrait(ui, r, state, card);
    let compact = r.width() < 620.;
    let actions_y = if compact { 128. } else { 196. };
    let actions = Rect::from_min_size(
        pos2(r.left() + 14.0, r.top() + actions_y),
        vec2(r.width() - 28.0, 34.0),
    );
    relation_buttons(ui, actions, state, card);
    let trade = Rect::from_min_max(
        pos2(r.left() + 14.0, r.top() + actions_y + 50.),
        r.max - vec2(14.0, 14.0),
    );
    trade_panel(ui, trade, state, card, view);
}

/// The leader's portrait with the name, the state of war or peace and the treaties.
fn portrait(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, card: &CivCard) {
    if r.width() < 620. {
        let frame = Rect::from_min_size(r.min + vec2(14., 14.), vec2(100., 100.));
        state.diplomacy.leaders.paint(
            ui,
            frame,
            &card.art,
            Asset::Portrait,
            state.brightness,
        );
        let x = frame.right() + 14.;
        text(ui.painter(), pos2(x, r.top() + 14.), &card.name, 21., CREAM);
        text(
            ui.painter(),
            pos2(x, r.top() + 42.),
            &card.leader,
            13.,
            GOLD,
        );
        text(
            ui.painter(),
            pos2(x, r.top() + 62.),
            &card.state,
            12.,
            if card.at_war { WAR } else { PEACE },
        );
        text(
            ui.painter(),
            pos2(x, r.top() + 82.),
            tf!("classic.ui.screen-diplomacy.score-gold", score = card.score, gold = card.gold),
            11.,
            MUTED,
        );
        let treaties = if card.treaties.is_empty() {
            t!("classic.ui.screen-diplomacy.no-treaties").to_owned()
        } else {
            tf!("classic.ui.screen-diplomacy.active-treaties", n = card.treaties.len())
        };
        text(ui.painter(), pos2(x, r.top() + 101.), treaties, 11., CREAM);
        ui.interact(
            Rect::from_min_max(
                pos2(x, r.top() + 98.),
                pos2(r.right() - 14., r.top() + 120.),
            ),
            ui.id().with("compact_treaties"),
            Sense::hover(),
        )
        .on_hover_text(card.treaties.join("\n"));
        return;
    }
    let frame = Rect::from_min_size(r.min + vec2(14.0, 14.0), vec2(168.0, 168.0));
    ui.painter().rect_filled(frame.expand(3.0), 3.0, BG);
    let stroke = Stroke::new(1.5, GOLD);
    ui.painter()
        .rect_stroke(frame.expand(3.0), 3.0, stroke, egui::StrokeKind::Inside);
    let brightness = state.brightness;
    let leaders = &mut state.diplomacy.leaders;
    leaders.paint(ui, frame, &card.art, Asset::Portrait, brightness);
    let x = frame.right() + 20.0;
    text(
        ui.painter(),
        pos2(x, r.top() + 14.0),
        &card.name,
        28.0,
        CREAM,
    );
    text(
        ui.painter(),
        pos2(x, r.top() + 50.0),
        &card.leader,
        15.0,
        GOLD,
    );
    let color = if card.at_war { WAR } else { PEACE };
    text(
        ui.painter(),
        pos2(x, r.top() + 74.0),
        &card.state,
        14.0,
        color,
    );
    let facts = tf!("classic.ui.screen-diplomacy.score-gold", score = card.score, gold = card.gold);
    text(ui.painter(), pos2(x, r.top() + 96.0), facts, 12.0, MUTED);
    let mut lines: Vec<String> = card.treaties.iter().take(4).cloned().collect();
    if lines.is_empty() {
        lines.push(t!("classic.ui.screen-diplomacy.no-treaties").into());
    }
    if card.treaties.len() > 4 {
        lines[3] = tf!("classic.ui.screen-diplomacy.more-treaties", n = card.treaties.len() - 3);
    }
    for (i, line) in lines.iter().enumerate() {
        let at = pos2(x, r.top() + 120.0 + i as f32 * 16.0);
        text(ui.painter(), at, line, 12.0, CREAM);
    }
}

fn relation_buttons(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, card: &CivCard) {
    let width = (r.width() - 16.0) / 3.0;
    let slot = |i: usize| {
        Rect::from_min_size(
            r.min + vec2(i as f32 * (width + 8.0), 0.0),
            vec2(width, r.height()),
        )
    };
    if card.at_war {
        if ready_button(ui, slot(0), t!("classic.ui.screen-diplomacy.make-peace"), &card.make_peace) {
            state.requests.push(GameRequest::MakePeace(card.player.0));
        }
    } else if ready_button(ui, slot(0), t!("classic.ui.screen-diplomacy.declare-war"), &card.declare_war) {
        state.requests.push(GameRequest::DeclareWar(card.player));
    }
    if ready_button(ui, slot(1), t!("classic.ui.screen-diplomacy.denounce"), &card.denounce) {
        state.requests.push(GameRequest::Denounce(card.player));
    }
    if ready_button(ui, slot(2), t!("classic.ui.screen-diplomacy.open-borders"), &card.open_borders) {
        state.requests.push(GameRequest::OpenBorders(card.player.0));
    }
}

/// Both sides edit their own quantities; the complete draft is checked by the core.
fn trade_panel(
    ui: &mut egui::Ui,
    r: Rect,
    state: &mut ScreenState,
    card: &CivCard,
    view: &DiplomacyView,
) {
    text(ui.painter(), r.min, t!("classic.ui.screen-diplomacy.negotiate"), 11.0, GOLD);
    let draft = state.diplomacy.draft_for(card.player);
    let summary_height = if draft.items.is_empty() { 38. } else { 96. };
    let columns = Rect::from_min_max(
        r.min + vec2(0., 22.),
        pos2(
            r.right(),
            (r.bottom() - 76. - summary_height).max(r.top() + 66.),
        ),
    );
    let width = (columns.width() - 16.) / 3.;
    let column = |i| {
        Rect::from_min_size(
            columns.min + vec2(i as f32 * (width + 8.), 0.),
            vec2(width, columns.height()),
        )
    };
    deal_column(ui, column(0), t!("classic.ui.screen-diplomacy.you-give"), &card.ours, view.me, draft);
    deal_column(
        ui,
        column(1),
        t!("classic.ui.screen-diplomacy.you-receive"),
        &card.theirs,
        card.player,
        draft,
    );
    deal_column(ui, column(2), t!("classic.ui.screen-diplomacy.agreements"), &card.mutual, view.me, draft);
    let summary = Rect::from_min_max(
        pos2(r.left(), columns.bottom() + 8.),
        pos2(r.right(), r.bottom() - 68.),
    );
    ui.painter().rect_filled(summary, 4., BG);
    super::scroll::scroll_body(ui, summary.shrink(8.), "deal_selected_terms", |ui| {
        if draft.items.is_empty() {
            ui.label(egui::RichText::new(t!("classic.ui.screen-diplomacy.select-items")).color(MUTED));
        }
        let items = draft.items.clone();
        for (giver, item) in items {
            let side = if item.is_mutual() {
                t!("classic.ui.screen-diplomacy.side-both")
            } else if giver == view.me {
                t!("classic.ui.screen-diplomacy.side-give")
            } else {
                t!("classic.ui.screen-diplomacy.side-receive")
            };
            let options = if item.is_mutual() {
                &card.mutual
            } else if giver == view.me {
                &card.ours
            } else {
                &card.theirs
            };
            let label = term_label(item, options);
            ui.horizontal_wrapped(|ui| {
                if ui.small_button(t!("classic.ui.screen-diplomacy.remove")).clicked() {
                    draft.toggle(giver, item);
                }
                ui.label(
                    egui::RichText::new(tf!("classic.ui.screen-diplomacy.term", side = side, label = label.as_str()))
                        .color(CREAM)
                        .size(12.),
                );
            });
        }
    });
    let checked = view
        .preview
        .as_ref()
        .filter(|(old, _)| old == draft)
        .map(|(_, availability)| availability);
    let pending = view
        .outgoing
        .iter()
        .any(|offer| offer.partner == draft.partner && offer.items == draft.items);
    let reason = if pending {
        t!("classic.ui.screen-diplomacy.offer-sent")
    } else if draft.items.is_empty() {
        t!("classic.ui.screen-diplomacy.select-one")
    } else {
        match checked {
            Some(crate::ui::model::Availability::Ready) => {
                t!("classic.ui.screen-diplomacy.ready-to-propose")
            }
            Some(crate::ui::model::Availability::Blocked(reason)) => reason.as_str(),
            None => {
                ui.ctx().request_repaint();
                t!("classic.ui.screen-diplomacy.checking")
            }
        }
    };
    paragraph(
        ui.painter(),
        pos2(r.left(), r.bottom() - 62.),
        reason,
        11.,
        MUTED,
        r.width(),
    );
    let footer = Rect::from_min_max(pos2(r.left(), r.bottom() - 32.), r.max);
    let clear = Rect::from_min_size(footer.min, vec2(100., 32.));
    let propose = Rect::from_min_max(pos2(clear.right() + 8., footer.top()), footer.max);
    if button(ui, clear, t!("classic.ui.screen-diplomacy.clear-offer"), false, !draft.items.is_empty()) {
        draft.items.clear();
    }
    let ready = checked.is_some_and(|availability| availability.is_ready())
        && !draft.items.is_empty()
        && !pending;
    if button(ui, propose, t!("classic.ui.screen-diplomacy.propose-deal"), false, ready) {
        if let Some(request) = draft.request() {
            state.requests.push(request);
        }
        // Retain terms on rejection; a proposal is not an accepted treaty.
    }
}

fn term_label(item: DealItem, options: &[DealOption]) -> String {
    match item {
        DealItem::Gold(n) => tf!("classic.ui.screen-diplomacy.gold-now", n = n),
        DealItem::GoldPerTurn(n) => tf!("classic.ui.screen-diplomacy.gold-per-turn", n = n),
        DealItem::Resource { resource, amount } => {
            let name = options
                .iter()
                .find(|o| matches!(o.item,DealItem::Resource { resource: r, .. } if r == resource))
                .map(|o| o.label.as_str())
                .unwrap_or(t!("classic.ui.screen-diplomacy.resource"));
            tf!("classic.ui.screen-diplomacy.resource-amount", amount = amount, name = name)
        }
        _ => options
            .iter()
            .find(|o| o.item == item)
            .map(|o| o.label.clone())
            .unwrap_or_else(|| t!("classic.ui.screen-diplomacy.agreement").into()),
    }
}

fn deal_column(
    ui: &mut egui::Ui,
    r: Rect,
    title: &'static str,
    options: &[DealOption],
    giver: PlayerId,
    draft: &mut DealDraft,
) {
    text(ui.painter(), r.min, title, 10., GOLD);
    let list = Rect::from_min_max(r.min + vec2(0., 18.), r.max);
    super::scroll::scroll_body(ui, list, title, |ui| {
        for option in options {
            let picked = draft.contains(giver, option.item);
            let peace = draft.items.iter().any(|(_, item)| *item == DealItem::Peace);
            let peace_item = peace
                && matches!(
                    option.item,
                    DealItem::Gold(_) | DealItem::GoldPerTurn(_) | DealItem::Resource { .. }
                );
            let enabled = option.availability.is_ready()
                || (peace_item && option.max_amount.is_some_and(|n| n > 0));
            let width = ui.available_width();
            let (row, _) = ui.allocate_exact_size(vec2(width, 30.), Sense::hover());
            let label = if picked {
                format!("✓ {}", option.label)
            } else {
                option.label.clone()
            };
            let note = match &option.availability {
                crate::ui::model::Availability::Blocked(reason) if !enabled && !picked => Some(reason.as_str()),
                _ => None,
            };
            if client::shell::kit::note_button(ui, row, &label, picked, picked || enabled, note) {
                draft.toggle(giver, option.item);
            }
            if let Some(reason) = note {
                ui.interact(
                    row,
                    ui.id().with(("reason", giver.0, option.label.as_str())),
                    Sense::hover(),
                )
                .on_hover_text(reason);
            }
            if let Some(max) = option.max_amount {
                let mut amount = draft.amount(giver, option.item);
                ui.push_id(("quantity", giver.0, option.label.as_str()), |ui| {
                    ui.horizontal(|ui| {
                        let response = ui.add_enabled(
                            max > 0,
                            egui::DragValue::new(&mut amount)
                                .range(1..=max.max(1))
                                .speed(1.)
                                .min_decimals(0)
                                .max_decimals(0),
                        );
                        if response.changed() {
                            draft.set_amount(giver, option.item, amount);
                        }
                        ui.label(
                            egui::RichText::new(tf!("classic.ui.screen-diplomacy.of-max", max = max))
                                .size(10.)
                                .color(MUTED),
                        );
                    });
                });
            }
            ui.add_space(5.);
        }
    });
}

fn offers(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, view: &DiplomacyView) {
    panel(ui.painter(), r);
    heading(ui, r, t!("classic.ui.screen-diplomacy.offers-heading"));
    if view.offers.is_empty() {
        let at = r.min + vec2(16.0, 36.0);
        paragraph(
            ui.painter(),
            at,
            t!("classic.ui.screen-diplomacy.no-offers"),
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
        "offers_screen_diplomacy.rs",
        view.offers
            .iter()
            .map(|c| 92.0 + c.lines.len() as f32 * 17.0)
            .sum(),
        |ui, r| {
            let mut y = r.top();
            for offer in &view.offers {
                let h = 84.0 + offer.lines.len() as f32 * 17.0;
                let card = Rect::from_min_size(pos2(r.left() + 10.0, y), vec2(r.width() - 20.0, h));
                offer_card(ui, card, state, offer);
                y += h + 8.0;
            }
        },
    );
}

fn offer_card(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, offer: &OfferCard) {
    if !ui.is_rect_visible(r) {
        return;
    }
    ui.painter().rect_filled(r, 5.0, RAISED);
    text(
        ui.painter(),
        r.min + vec2(10.0, 8.0),
        &offer.from,
        15.0,
        CREAM,
    );
    let expires = tf!("classic.ui.screen-diplomacy.answer-by", turn = offer.expires);
    text(ui.painter(), r.min + vec2(10.0, 28.0), expires, 10.0, MUTED);
    for (i, line) in offer.lines.iter().enumerate() {
        let at = r.min + vec2(10.0, 46.0 + i as f32 * 17.0);
        text(ui.painter(), at, line, 12.0, CREAM);
    }
    let half = (r.width() - 28.0) / 2.0;
    let accept = Rect::from_min_size(pos2(r.left() + 10.0, r.bottom() - 36.0), vec2(half, 28.0));
    let refuse = accept.translate(vec2(half + 8.0, 0.0));
    if button(ui, accept, t!("classic.ui.screen-diplomacy.accept"), false, true) {
        let request = GameRequest::RespondDeal {
            offer: offer.id,
            accept: true,
        };
        state.requests.push(request);
    }
    if button(ui, refuse, t!("classic.ui.screen-diplomacy.refuse"), false, true) {
        let request = GameRequest::RespondDeal {
            offer: offer.id,
            accept: false,
        };
        state.requests.push(request);
    }
}

#[cfg(test)]
mod overflow_tests {
    use super::*;
    #[test]
    fn proposal_pointer_uses_exact_checked_terms_and_blocks_stale_or_pending_drafts() {
        use crate::ui::model::Availability;
        for width in [490., 780.] {
            let ctx = egui::Context::default();
            let mut state = ScreenState::default();
            let partner = PlayerId(1);
            let mut draft = DealDraft::new(partner);
            draft.set_amount(PlayerId(0), DealItem::Gold(1), 37);
            draft.toggle(PlayerId(0), DealItem::Gold(1));
            state.diplomacy.draft = Some(draft.clone());
            let card = CivCard {
                player: partner,
                name: "Partner".into(),
                leader: String::new(),
                art: String::new(),
                at_war: false,
                state: String::new(),
                treaties: vec![],
                score: 0,
                gold: 100,
                declare_war: Availability::Ready,
                make_peace: Availability::Ready,
                denounce: Availability::Ready,
                open_borders: Availability::Ready,
                ours: vec![],
                theirs: vec![],
                mutual: vec![],
            };
            let mut view = DiplomacyView {
                me: PlayerId(0),
                gold: 100,
                civs: vec![],
                offers: vec![],
                outgoing: vec![],
                preview: None,
            };
            let bounds = Rect::from_min_size(pos2(20., 20.), vec2(width, 340.));
            let point = pos2(bounds.right() - 60., bounds.bottom() - 16.);
            let frame = |state: &mut ScreenState, view: &DiplomacyView, events| {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(
                            egui::Pos2::ZERO,
                            vec2(width + 40., 400.),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ui| trade_panel(ui, bounds, state, &card, view),
                );
                output.textures_delta.clear();
            };
            let click = |state: &mut ScreenState, view: &DiplomacyView| {
                frame(state, view, vec![egui::Event::PointerMoved(point)]);
                for pressed in [true, false] {
                    frame(
                        state,
                        view,
                        vec![egui::Event::PointerButton {
                            pos: point,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: Default::default(),
                        }],
                    );
                }
            };
            frame(&mut state, &view, vec![]);
            click(&mut state, &view);
            assert!(
                state.requests.is_empty(),
                "unvalidated drafts cannot be sent"
            );
            view.preview = Some((draft.clone(), Availability::Ready));
            click(&mut state, &view);
            assert_eq!(state.requests, vec![draft.request().unwrap()]);
            state.requests.clear();
            view.outgoing.push(draft.clone());
            click(&mut state, &view);
            assert!(
                state.requests.is_empty(),
                "pending exact offers cannot be resent"
            );
            view.outgoing.clear();
            state
                .diplomacy
                .draft
                .as_mut()
                .unwrap()
                .set_amount(PlayerId(0), DealItem::Gold(1), 38);
            click(&mut state, &view);
            assert!(
                state.requests.is_empty(),
                "editing invalidates the old validation"
            );
        }
    }

    fn frame(
        ctx: &egui::Context,
        state: &mut ScreenState,
        view: &DiplomacyView,
        events: Vec<egui::Event>,
    ) -> Vec<egui::epaint::ClippedShape> {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(400., 320.))),
                events,
                ..Default::default()
            },
            |ui| {
                offers(
                    ui,
                    Rect::from_min_max(pos2(20., 20.), pos2(320., 280.)),
                    state,
                    view,
                )
            },
        );
        output.textures_delta.clear();
        output.shapes
    }
    #[test]
    fn final_offer_after_scroll_keeps_its_typed_accept_action() {
        let ctx = egui::Context::default();
        let mut state = ScreenState::default();
        let view = DiplomacyView {
            outgoing: Vec::new(),
            preview: None,
            me: PlayerId(0),
            gold: 10,
            civs: vec![],
            offers: (0..20)
                .map(|id| OfferCard {
                    id,
                    from: format!("Nation {id}"),
                    expires: 50,
                    lines: vec!["They give: 10 gold".into()],
                })
                .collect(),
        };
        frame(&ctx, &mut state, &view, vec![]);
        frame(
            &ctx,
            &mut state,
            &view,
            vec![egui::Event::PointerMoved(pos2(150., 160.))],
        );
        frame(
            &ctx,
            &mut state,
            &view,
            vec![egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                phase: egui::TouchPhase::Move,
                delta: vec2(0., -9000.),
                modifiers: Default::default(),
            }],
        );
        let mut shapes = Vec::new();
        for _ in 0..24 {
            shapes = frame(&ctx, &mut state, &view, vec![]);
        }
        let point = shapes
            .into_iter()
            .filter_map(|s| match s.shape {
                egui::Shape::Text(t)
                    if t.galley.text() == "Accept" && s.clip_rect.contains(t.pos) =>
                {
                    Some(t.pos + vec2(4., 4.))
                }
                _ => None,
            })
            .last()
            .expect("final offer has a visible Accept button");
        frame(
            &ctx,
            &mut state,
            &view,
            vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ],
        );
        frame(
            &ctx,
            &mut state,
            &view,
            vec![egui::Event::PointerButton {
                pos: point,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
        );
        assert!(
            state.requests.iter().any(|r| matches!(
                r,
                GameRequest::RespondDeal {
                    offer: 19,
                    accept: true
                }
            )),
            "last offer must retain its real command target"
        );
    }
}
