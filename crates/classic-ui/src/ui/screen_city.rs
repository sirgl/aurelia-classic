use super::*;
use i18n::{t, tf};
/// A sample production item of the preview city; the text fields hold message keys.
struct Production {
    name: &'static str,
    art: &'static str,
    kind: &'static str,
    turns: u32,
    cost: u32,
    description: &'static str,
}
const PRODUCTION: [Production; 4] = [
    Production {
        name: i18n::key!("classic.ui.screen-city.granary"),
        art: "granary",
        kind: i18n::key!("classic.ui.screen-city.kind-building"),
        turns: 4,
        cost: 60,
        description: i18n::key!("classic.ui.screen-city.granary-desc"),
    },
    Production {
        name: i18n::key!("classic.ui.screen-city.warrior"),
        art: "warrior",
        kind: i18n::key!("classic.ui.screen-city.kind-melee-unit"),
        turns: 3,
        cost: 40,
        description: i18n::key!("classic.ui.screen-city.warrior-desc"),
    },
    Production {
        name: i18n::key!("classic.ui.screen-city.monument"),
        art: "core:building/monument",
        kind: i18n::key!("classic.ui.screen-city.kind-building"),
        turns: 3,
        cost: 40,
        description: i18n::key!("classic.ui.screen-city.monument-desc"),
    },
    Production {
        name: i18n::key!("classic.ui.screen-city.water-mill"),
        art: "watermill",
        kind: i18n::key!("classic.ui.screen-city.kind-building"),
        turns: 5,
        cost: 75,
        description: i18n::key!("classic.ui.screen-city.water-mill-desc"),
    },
];
pub(super) fn draw(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, world: &RenderWorld) {
    let city = world.cities.first();
    let name = city.map_or(t!("classic.ui.screen-city.default-name"), |city| city.name.as_str());
    let population = city.and_then(|city| city.population).unwrap_or(6);
    let left = Rect::from_min_size(r.min, vec2(278.0, r.height()));
    let right = Rect::from_min_max(pos2(r.right() - 320.0, r.top()), r.max);
    let title = Rect::from_center_size(
        pos2((left.right() + right.left()) * 0.5, r.top() + 28.0),
        vec2(300.0, 56.0),
    );
    panel(ui.painter(), title);
    ui.painter().text(
        title.center() - vec2(0.0, 8.0),
        Align2::CENTER_CENTER,
        name,
        FontId::proportional(25.0),
        CREAM,
    );
    ui.painter().text(
        title.center() + vec2(0.0, 15.0),
        Align2::CENTER_CENTER,
        t!("classic.ui.screen-city.subtitle"),
        FontId::proportional(10.0),
        GOLD,
    );
    overview(ui, left, state, population);
    production(ui, right, state);
}

fn overview(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, population: u32) {
    panel(ui.painter(), r);
    text(
        ui.painter(),
        r.min + vec2(18.0, 17.0),
        t!("classic.ui.screen-city.overview"),
        11.0,
        GOLD,
    );
    let badge = r.min + vec2(45.0, 75.0);
    ui.painter().circle_filled(badge, 27.0, RAISED);
    ui.painter()
        .circle_stroke(badge, 27.0, Stroke::new(1.5, GOLD));
    ui.painter().text(
        badge,
        Align2::CENTER_CENTER,
        population,
        FontId::proportional(28.0),
        CREAM,
    );
    text(
        ui.painter(),
        r.min + vec2(87.0, 53.0),
        t!("classic.ui.screen-city.citizens"),
        21.0,
        CREAM,
    );
    text(
        ui.painter(),
        r.min + vec2(87.0, 83.0),
        t!("classic.ui.screen-city.growing"),
        12.0,
        MUTED,
    );
    text(
        ui.painter(),
        r.min + vec2(18.0, 121.0),
        t!("classic.ui.screen-city.food-stored"),
        13.0,
        CREAM,
    );
    let bar = Rect::from_min_size(r.min + vec2(18.0, 145.0), vec2(r.width() - 36.0, 6.0));
    ui.painter().rect_filled(bar, 2.0, RAISED);
    ui.painter().rect_filled(
        Rect::from_min_size(bar.min, vec2(bar.width() * 28.0 / 45.0, 6.0)),
        2.0,
        Color32::from_rgb(151, 191, 115),
    );
    for (i, (label, value)) in [
        (t!("classic.ui.screen-city.food"), "+7"),
        (t!("classic.ui.screen-city.production"), "9"),
        (t!("classic.ui.screen-city.science"), "+8"),
        (t!("classic.ui.screen-city.gold"), "+4"),
        (t!("classic.ui.screen-city.culture"), "+3"),
        (t!("classic.ui.screen-city.defence"), "12"),
    ]
    .into_iter()
    .enumerate()
    {
        let at = r.min + vec2(18.0 + (i % 2) as f32 * 126.0, 174.0 + (i / 2) as f32 * 39.0);
        text(ui.painter(), at, label, 12.0, MUTED);
        text(ui.painter(), at + vec2(78.0, -2.0), value, 18.0, GOLD);
    }
    for (index, label) in [t!("classic.ui.screen-city.citizens"), t!("classic.ui.screen-city.buildings")].into_iter().enumerate() {
        if button(
            ui,
            Rect::from_min_size(
                r.min + vec2(14.0 + index as f32 * 128.0, 300.0),
                vec2(122.0, 32.0),
            ),
            label,
            state.city_tab == index,
            true,
        ) {
            state.city_tab = index;
        }
    }
    let body = Rect::from_min_max(r.min + vec2(16.0, 352.0), r.max - vec2(16.0, 16.0));
    if state.city_tab == 1 {
        buildings(ui, body, state);
    } else {
        citizens(ui, body, state);
    }
}

fn citizens(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState) {
    text(ui.painter(), r.min, t!("classic.ui.screen-city.citizen-focus"), 10.0, GOLD);
    for (i, label) in [t!("classic.ui.screen-city.focus-balanced"), t!("classic.ui.screen-city.focus-growth"), t!("classic.ui.screen-city.focus-industry")].into_iter().enumerate() {
        if button(
            ui,
            Rect::from_min_size(r.min + vec2(i as f32 * 84.0, 24.0), vec2(78.0, 31.0)),
            label,
            state.focus == i,
            true,
        ) {
            state.focus = i;
        }
    }
    let jobs = match state.focus {
        1 => [4, 1, 1],
        2 => [1, 4, 1],
        _ => [3, 2, 1],
    };
    text(
        ui.painter(),
        r.min + vec2(0.0, 74.0),
        t!("classic.ui.screen-city.assignments"),
        10.0,
        GOLD,
    );
    for (i, (name, image)) in [
        (t!("classic.ui.screen-city.fields"), "agriculture"),
        (t!("classic.ui.screen-city.workshops"), "mining"),
        (t!("classic.ui.screen-city.scholars"), "writing"),
    ]
    .into_iter()
    .enumerate()
    {
        let row = Rect::from_min_size(
            r.min + vec2(0.0, 96.0 + i as f32 * 53.0),
            vec2(r.width(), 46.0),
        );
        art(
            state,
            ui,
            Rect::from_min_size(row.min, vec2(42.0, 42.0)),
            image,
        );
        text(ui.painter(), row.min + vec2(52.0, 7.0), name, 15.0, CREAM);
        ui.painter().text(
            row.right_center() - vec2(6.0, 0.0),
            Align2::RIGHT_CENTER,
            jobs[i],
            FontId::proportional(18.0),
            GOLD,
        );
    }
}

fn buildings(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState) {
    text(ui.painter(), r.min, t!("classic.ui.screen-city.established"), 10.0, GOLD);
    for (i, (name, image, effect)) in [
        (t!("classic.ui.screen-city.palace"), "city", t!("classic.ui.screen-city.palace-effect")),
        (t!("classic.ui.screen-city.granary"), "granary", t!("classic.ui.screen-city.granary-effect")),
        (t!("classic.ui.screen-city.monument"), "core:building/monument", t!("classic.ui.screen-city.monument-effect")),
    ]
    .into_iter()
    .enumerate()
    {
        let row = Rect::from_min_size(
            r.min + vec2(0.0, 27.0 + i as f32 * 69.0),
            vec2(r.width(), 62.0),
        );
        art(
            state,
            ui,
            Rect::from_min_size(row.min, vec2(52.0, 52.0)),
            image,
        );
        text(ui.painter(), row.min + vec2(62.0, 3.0), name, 16.0, CREAM);
        text(
            ui.painter(),
            row.min + vec2(62.0, 29.0),
            effect,
            11.0,
            MUTED,
        );
    }
}

fn production(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState) {
    panel(ui.painter(), r);
    text(
        ui.painter(),
        r.min + vec2(16.0, 17.0),
        t!("classic.ui.screen-city.choose-production"),
        11.0,
        GOLD,
    );
    for (i, item) in PRODUCTION.iter().enumerate() {
        let row = Rect::from_min_size(
            r.min + vec2(12.0, 44.0 + i as f32 * 68.0),
            vec2(r.width() - 24.0, 60.0),
        );
        if ui
            .interact(row, ui.id().with(("production", i)), Sense::click())
            .clicked()
        {
            state.production = i;
        }
        ui.painter().rect_filled(row, 4.0, RAISED);
        if state.production == i {
            ui.painter()
                .rect_stroke(row, 4.0, Stroke::new(1.3, GOLD), egui::StrokeKind::Inside);
        }
        art(
            state,
            ui,
            Rect::from_min_size(row.min + vec2(4.0, 4.0), vec2(52.0, 52.0)),
            item.art,
        );
        text(
            ui.painter(),
            row.min + vec2(66.0, 8.0),
            i18n::tr(item.name),
            17.0,
            CREAM,
        );
        text(
            ui.painter(),
            row.min + vec2(66.0, 34.0),
            tf!("classic.ui.screen-city.cost-turns", cost = item.cost, n = item.turns),
            11.0,
            MUTED,
        );
    }
    let selected = &PRODUCTION[state.production];
    text(
        ui.painter(),
        r.min + vec2(17.0, 330.0),
        i18n::tr(selected.kind),
        10.0,
        GOLD,
    );
    paragraph(
        ui.painter(),
        r.min + vec2(17.0, 353.0),
        i18n::tr(selected.description),
        13.0,
        CREAM,
        r.width() - 34.0,
    );
    if button(
        ui,
        Rect::from_min_size(r.min + vec2(16.0, 413.0), vec2(r.width() - 32.0, 36.0)),
        t!("classic.ui.screen-city.add-to-queue"),
        true,
        state.queue.len() < 4,
    ) {
        state.queue.push(state.production);
    }
    text(
        ui.painter(),
        r.min + vec2(17.0, 473.0),
        tf!("classic.ui.screen-city.queue", n = state.queue.len()),
        10.0,
        GOLD,
    );
    let mut remove = None;
    for (i, &item) in state.queue.iter().enumerate() {
        let at = r.min + vec2(17.0, 502.0 + i as f32 * 33.0);
        text(
            ui.painter(),
            at,
            format!("{}  {}", i + 1, i18n::tr(PRODUCTION[item].name)),
            13.0,
            CREAM,
        );
        if button(
            ui,
            Rect::from_min_size(pos2(r.right() - 46.0, at.y - 4.0), vec2(28.0, 26.0)),
            "-",
            false,
            true,
        ) {
            remove = Some(i);
        }
    }
    if let Some(index) = remove {
        state.queue.remove(index);
    }
}
