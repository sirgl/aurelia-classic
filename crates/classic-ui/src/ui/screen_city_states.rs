//! City-state panel bound to the running game (`CityStatesView`): one card per met
//! city-state with its type, status and ally, the influence bar (resting point, friends
//! and allies thresholds), the quests, and the gold gifts (with the influence they buy),
//! pledge, and tribute buttons greyed by the core's verdict.
use super::*;
use i18n::{t, tf};
use crate::ui::model::{CityStateCard, CityStatesView, GameRequest};

pub(crate) const CARD_H: f32 = 228.0;
/// The extra row of the card on the City-States screen: "Open on map".
const MAP_ROW_H: f32 = 36.0;
/// Influence shown on the bar: from war (-60) to well past the allies threshold.
const BAR_MIN: i32 = -60;
const BAR_MAX: i32 = 120;
const WAR: Color32 = Color32::from_rgb(214, 96, 84);
const FRIEND: Color32 = Color32::from_rgb(137, 188, 83);

pub(super) fn city_states(
    ui: &mut egui::Ui,
    r: Rect,
    state: &mut ScreenState,
    view: &CityStatesView,
    message: Option<&str>,
) {
    let top = Rect::from_min_size(r.min, vec2(r.width(), 92.0));
    let allies = view.cards.iter().filter(|c| c.status == "Allies").count(); // i18n-ok: model identifier
    let line = tf!("classic.ui.screen-city-states.banner-line",
        met = view.cards.len(),
        allies = allies,
        gold = view.gold
    );
    world_banner(ui, top, t!("classic.ui.screen-city-states.title"), &line, message);
    let body = Rect::from_min_max(pos2(r.left(), top.bottom() + 14.0), r.max);
    panel(ui.painter(), body);
    if view.cards.is_empty() {
        let text = t!("classic.ui.screen-city-states.none-met");
        paragraph(
            ui.painter(),
            body.min + vec2(18.0, 18.0),
            text,
            14.0,
            MUTED,
            body.width() - 36.0,
        );
        return;
    }
    let list = body.shrink(10.0);
    ui.scope_builder(egui::UiBuilder::new().max_rect(list), |ui| {
        egui::ScrollArea::vertical()
            .id_salt("city_state_cards")
            .show(ui, |ui| card_grid(ui, list, state, view));
    });
}

fn card_grid(ui: &mut egui::Ui, list: Rect, state: &mut ScreenState, view: &CityStatesView) {
    let columns = if list.width() > 900.0 { 2 } else { 1 };
    let rows = view.cards.len().div_ceil(columns);
    let width = (list.width() - 12.0 - 10.0 * (columns as f32 - 1.0)) / columns as f32;
    let height = rows as f32 * (CARD_H + MAP_ROW_H + 10.0);
    let (area, _) = ui.allocate_exact_size(vec2(list.width() - 12.0, height), Sense::hover());
    for (i, card) in view.cards.iter().enumerate() {
        let at = area.min
            + vec2(
                (i % columns) as f32 * (width + 10.0),
                (i / columns) as f32 * (CARD_H + MAP_ROW_H + 10.0),
            );
        city_state_card(
            ui,
            Rect::from_min_size(at, vec2(width, CARD_H + MAP_ROW_H)),
            &mut state.requests,
            card,
            true,
        );
    }
}

/// One city-state's card in `r`: the screen's grid and the window over the map draw it the same way;
/// the buttons push their requests to `requests`. `with_map_button`: the screen's card has the row
/// "Open on map" under the others (`CARD_H + MAP_ROW_H` tall) and its name opens the map too; the
/// window over the map (`CARD_H` tall) is already there.
pub(crate) fn city_state_card(ui: &mut egui::Ui, r: Rect, requests: &mut Vec<GameRequest>, card: &CityStateCard, with_map_button: bool) {
    ui.painter().rect_filled(r, 5.0, RAISED);
    super::world_symbols::city_state(ui.painter(),Rect::from_min_size(r.min+vec2(10.,13.),vec2(32.,32.)),card.kind,GOLD,RAISED);
    let mut name_color = CREAM;
    if with_map_button {
        let width = (r.width() - 52.0 - 120.0).clamp(60.0, 260.0);
        let name = Rect::from_min_size(r.min + vec2(52.0, 8.0), vec2(width, 28.0));
        let ready = card.on_map.is_ready();
        let response = ui.interact(name, egui::Id::new(("city_state_name", card.player.0)), if ready { Sense::click() } else { Sense::hover() });
        if ready && response.hovered() {
            name_color = GOLD;
        }
        if response.on_hover_text(t!("classic.ui.screen-city-states.on-map")).clicked() && ready {
            requests.push(GameRequest::ShowCityState(card.player));
        }
    }
    text(
        ui.painter(),
        r.min + vec2(52.0, 10.0),
        &card.name,
        20.0,
        name_color,
    );
    let kind = tf!("classic.ui.screen-city-states.kind-line", kind = kind_name(card.kind));
    text(ui.painter(), r.min + vec2(52.0, 38.0), kind, 11.0, GOLD);
    let color = status_color(card.status);
    ui.painter().text(
        pos2(r.right() - 12.0, r.top() + 12.0),
        Align2::RIGHT_TOP,
        status_name(card.status),
        FontId::proportional(15.0),
        color,
    );
    let mut ally = match &card.ally {
        Some(name) => tf!("classic.ui.screen-city-states.ally", name = name.as_str()),
        None => t!("classic.ui.screen-city-states.no-ally").to_string(),
    };
    if card.pledged {
        ally = tf!("classic.ui.screen-city-states.ally-protected", ally = ally.as_str());
    }
    ui.painter().text(
        pos2(r.right() - 12.0, r.top() + 38.0),
        Align2::RIGHT_TOP,
        ally,
        FontId::proportional(11.0),
        MUTED,
    );
    let bar = Rect::from_min_size(r.min + vec2(12.0, 62.0), vec2(r.width() - 24.0, 10.0));
    influence_bar(ui, bar, card);
    quests(
        ui,
        Rect::from_min_size(r.min + vec2(12.0, 98.0), vec2(r.width() - 24.0, 50.0)),
        card,
    );
    actions(
        ui,
        Rect::from_min_max(r.min + vec2(12.0, 152.0), r.max - vec2(12.0, 10.0)),
        requests,
        card,
        with_map_button,
    );
}

/// The model names a standing by an English identifier ("Allies", "Friends", "At war",
/// "Angry", "Neutral"); the screen words it in the active language. Any other text is shown
/// as it is.
fn status_name(status: &str) -> &str {
    match status {
        "Allies" => t!("classic.ui.screen-city-states.status-allies"), // i18n-ok: model identifier
        "Friends" => t!("classic.ui.screen-city-states.status-friends"), // i18n-ok: model identifier
        "At war" => t!("classic.ui.screen-city-states.status-war"), // i18n-ok: model identifier
        "Angry" => t!("classic.ui.screen-city-states.status-angry"), // i18n-ok: model identifier
        "Neutral" => t!("classic.ui.screen-city-states.status-neutral"), // i18n-ok: model identifier
        other => other,
    }
}

/// The kind of a city-state the same way ("Cultured", "Maritime", "Mercantile",
/// "Militaristic", "Religious").
fn kind_name(kind: &str) -> &str {
    match kind {
        "Cultured" => t!("classic.ui.screen-city-states.kind-cultured"), // i18n-ok: model identifier
        "Maritime" => t!("classic.ui.screen-city-states.kind-maritime"), // i18n-ok: model identifier
        "Mercantile" => t!("classic.ui.screen-city-states.kind-mercantile"), // i18n-ok: model identifier
        "Militaristic" => t!("classic.ui.screen-city-states.kind-militaristic"), // i18n-ok: model identifier
        "Religious" => t!("classic.ui.screen-city-states.kind-religious"), // i18n-ok: model identifier
        other => other,
    }
}

fn status_color(status: &str) -> Color32 {
    match status {
        "Allies" => GOLD, // i18n-ok: model identifier
        "Friends" => FRIEND, // i18n-ok: model identifier
        "At war" | "Angry" => WAR, // i18n-ok: model identifier
        _ => MUTED,
    }
}

/// The influence on a bar from war to past the allies line, with the resting point.
fn influence_bar(ui: &mut egui::Ui, r: Rect, card: &CityStateCard) {
    let x_of = |value: i32| {
        let share = (value.clamp(BAR_MIN, BAR_MAX) - BAR_MIN) as f32 / (BAR_MAX - BAR_MIN) as f32;
        r.left() + r.width() * share
    };
    let p = ui.painter();
    p.rect_filled(r, 3.0, Color32::from_rgb(30, 40, 44));
    let (from, to) = (
        x_of(0).min(x_of(card.influence)),
        x_of(0).max(x_of(card.influence)),
    );
    let fill = if card.influence >= 0 { FRIEND } else { WAR };
    p.rect_filled(Rect::from_x_y_ranges(from..=to, r.y_range()), 3.0, fill);
    for (value, color) in [
        (0, MUTED),
        (card.friends_at, FRIEND),
        (card.allies_at, GOLD),
    ] {
        let x = x_of(value);
        p.line_segment(
            [pos2(x, r.top() - 4.0), pos2(x, r.bottom() + 4.0)],
            Stroke::new(2.0, color),
        );
    }
    let rest = x_of(card.resting);
    let marker = vec![
        pos2(rest, r.bottom() + 1.0),
        pos2(rest - 5.0, r.bottom() + 8.0),
        pos2(rest + 5.0, r.bottom() + 8.0),
    ];
    p.add(egui::Shape::convex_polygon(marker, CREAM, Stroke::NONE));
    let caption = tf!("classic.ui.screen-city-states.influence",
        influence = card.influence,
        resting = card.resting,
        friends = card.friends_at,
        allies = card.allies_at
    );
    text(p, pos2(r.left(), r.bottom() + 9.0), caption, 11.0, CREAM);
}

fn quests(ui: &mut egui::Ui, r: Rect, card: &CityStateCard) {
    if card.quests.is_empty() {
        text(ui.painter(), r.min, t!("classic.ui.screen-city-states.no-quest"), 11.0, MUTED);
        return;
    }
    for (i, quest) in card.quests.iter().take(3).enumerate() {
        let line = tf!("classic.ui.screen-city-states.quest", quest = quest.as_str());
        text(
            ui.painter(),
            r.min + vec2(0.0, i as f32 * 16.0),
            line,
            11.0,
            CREAM,
        );
    }
}

/// Gifts on the first row; pledge and tribute on the second.
fn actions(ui: &mut egui::Ui, r: Rect, requests: &mut Vec<GameRequest>, card: &CityStateCard, with_map_button: bool) {
    let width = (r.width() - 16.0) / 3.0;
    let slot = |column: usize, row: usize| {
        Rect::from_min_size(
            r.min + vec2(column as f32 * (width + 8.0), row as f32 * 36.0),
            vec2(width, 30.0),
        )
    };
    for (i, gift) in card.gifts.iter().enumerate() {
        let label = tf!("classic.ui.screen-city-states.gift", gold = gift.gold, influence = gift.influence);
        if ready_button(ui, slot(i, 0), &label, &gift.availability) {
            let request = GameRequest::GiftGold {
                city_state: card.player,
                gold: gift.gold,
            };
            requests.push(request);
        }
    }
    let pledge = if card.pledged {
        t!("classic.ui.screen-city-states.withdraw-pledge")
    } else {
        t!("classic.ui.screen-city-states.pledge")
    };
    if ready_button(ui, slot(0, 1), pledge, &card.pledge) {
        let request = GameRequest::Pledge {
            city_state: card.player,
            pledge: !card.pledged,
        };
        requests.push(request);
    }
    let tribute = tf!("classic.ui.screen-city-states.demand-gold", gold = card.bully_gold);
    if ready_button(ui, slot(1, 1), &tribute, &card.bully_gold_state) {
        requests.push(GameRequest::BullyGold(card.player));
    }
    if ready_button(ui, slot(2, 1), t!("classic.ui.screen-city-states.demand-worker"), &card.bully_worker) {
        requests.push(GameRequest::BullyWorker(card.player));
    }
    if with_map_button && ready_button(ui, slot(0, 2), t!("classic.ui.screen-city-states.on-map"), &card.on_map) {
        requests.push(GameRequest::ShowCityState(card.player));
    }
    if let Some((cost, availability)) = &card.buyout {
        let label = tf!("classic.ui.screen-city-states.buy-out", cost = *cost);
        let buyout = Rect::from_min_size(r.min + vec2(0.0, -34.0), vec2(width, 28.0));
        if ready_button(
            ui,
            buyout.translate(vec2(2.0 * (width + 8.0), 0.0)),
            &label,
            availability,
        ) {
            requests.push(GameRequest::BuyoutCityState(card.player));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::city_states::fixtures::world;

    const AT: egui::Pos2 = egui::Pos2 { x: 100.0, y: 100.0 };

    /// The card on the screen (with its "Open on map" row), clicked at `at`: the requests.
    fn click(card: &CityStateCard, at: egui::Pos2) -> Vec<GameRequest> {
        let ctx = egui::Context::default();
        let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0));
        let mut requests = Vec::new();
        let mut frame = |events: Vec<egui::Event>| {
            let raw = egui::RawInput { screen_rect: Some(screen), events, ..Default::default() };
            let mut output = ctx.run_ui(raw, |ui| {
                city_state_card(ui, Rect::from_min_size(AT, vec2(560.0, CARD_H + MAP_ROW_H)), &mut requests, card, true);
            });
            output.textures_delta.clear();
        };
        frame(vec![]);
        frame(vec![egui::Event::PointerMoved(at)]);
        for pressed in [true, false] {
            frame(vec![egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE }]);
        }
        requests
    }

    #[test]
    fn open_on_map_is_a_button_and_the_name_of_a_card_whose_city_is_on_the_map() {
        let mut w = world();
        let cards = w.session.city_states_view().cards;
        let card = cards.iter().find(|c| c.player == w.met).unwrap();
        let want = vec![GameRequest::ShowCityState(w.met)];
        assert_eq!(click(card, pos2(AT.x + 100.0, AT.y + 152.0 + 72.0 + 15.0)), want, "the button");
        assert_eq!(click(card, pos2(AT.x + 100.0, AT.y + 20.0)), want, "the name");
    }

    #[test]
    fn open_on_map_is_grey_for_a_city_that_is_not_on_the_map() {
        let mut w = world();
        let cards = w.session.city_states_view().cards;
        let card = cards.iter().find(|c| c.player == w.hidden).unwrap();
        assert!(!card.on_map.is_ready());
        assert!(click(card, pos2(AT.x + 100.0, AT.y + 152.0 + 72.0 + 15.0)).is_empty());
        assert!(click(card, pos2(AT.x + 100.0, AT.y + 20.0)).is_empty());
    }
}
