use super::*;
use i18n::{t, tf};
pub(super) struct Technology {
    name_key: &'static str,
    art: &'static str,
    cost: u32,
    turns: u32,
    pub prerequisites: &'static [usize],
    position: (usize, usize),
    description_key: &'static str,
    unlocks_key: &'static str,
}
impl Technology {
    pub(super) fn name(&self) -> &'static str {
        i18n::tr(self.name_key)
    }
    fn description(&self) -> &'static str {
        i18n::tr(self.description_key)
    }
    fn unlocks(&self) -> &'static str {
        i18n::tr(self.unlocks_key)
    }
}
pub(super) const TECHS: [Technology; 9] = [
    Technology {
        name_key: i18n::key!("classic.ui.screen-progress.tech.agriculture.name"),
        art: "agriculture",
        cost: 20,
        turns: 1,
        prerequisites: &[],
        position: (0, 1),
        description_key: i18n::key!("classic.ui.screen-progress.tech.agriculture.desc"),
        unlocks_key: i18n::key!("classic.ui.screen-progress.tech.agriculture.unlocks"),
    },
    Technology {
        name_key: i18n::key!("classic.ui.screen-progress.tech.pottery.name"),
        art: "pottery",
        cost: 35,
        turns: 3,
        prerequisites: &[0],
        position: (1, 0),
        description_key: i18n::key!("classic.ui.screen-progress.tech.pottery.desc"),
        unlocks_key: i18n::key!("classic.ui.screen-progress.tech.pottery.unlocks"),
    },
    Technology {
        name_key: i18n::key!("classic.ui.screen-progress.tech.mining.name"),
        art: "mining",
        cost: 55,
        turns: 4,
        prerequisites: &[0],
        position: (1, 1),
        description_key: i18n::key!("classic.ui.screen-progress.tech.mining.desc"),
        unlocks_key: i18n::key!("classic.ui.screen-progress.tech.mining.unlocks"),
    },
    Technology {
        name_key: i18n::key!("classic.ui.screen-progress.tech.animal-husbandry.name"),
        art: "husbandry",
        cost: 55,
        turns: 4,
        prerequisites: &[0],
        position: (1, 2),
        description_key: i18n::key!("classic.ui.screen-progress.tech.animal-husbandry.desc"),
        unlocks_key: i18n::key!("classic.ui.screen-progress.tech.animal-husbandry.unlocks"),
    },
    Technology {
        name_key: i18n::key!("classic.ui.screen-progress.tech.writing.name"),
        art: "writing",
        cost: 75,
        turns: 6,
        prerequisites: &[1],
        position: (2, 0),
        description_key: i18n::key!("classic.ui.screen-progress.tech.writing.desc"),
        unlocks_key: i18n::key!("classic.ui.screen-progress.tech.writing.unlocks"),
    },
    Technology {
        name_key: i18n::key!("classic.ui.screen-progress.tech.masonry.name"),
        art: "core:tech/masonry",
        cost: 75,
        turns: 6,
        prerequisites: &[2],
        position: (2, 1),
        description_key: i18n::key!("classic.ui.screen-progress.tech.masonry.desc"),
        unlocks_key: i18n::key!("classic.ui.screen-progress.tech.masonry.unlocks"),
    },
    Technology {
        name_key: i18n::key!("classic.ui.screen-progress.tech.calendar.name"),
        art: "core:tech/calendar",
        cost: 70,
        turns: 5,
        prerequisites: &[1],
        position: (2, 2),
        description_key: i18n::key!("classic.ui.screen-progress.tech.calendar.desc"),
        unlocks_key: i18n::key!("classic.ui.screen-progress.tech.calendar.unlocks"),
    },
    Technology {
        name_key: i18n::key!("classic.ui.screen-progress.tech.bronze-working.name"),
        art: "core:tech/bronze-working",
        cost: 105,
        turns: 8,
        prerequisites: &[2],
        position: (3, 1),
        description_key: i18n::key!("classic.ui.screen-progress.tech.bronze-working.desc"),
        unlocks_key: i18n::key!("classic.ui.screen-progress.tech.bronze-working.unlocks"),
    },
    Technology {
        name_key: i18n::key!("classic.ui.screen-progress.tech.philosophy.name"),
        art: "core:tech/philosophy",
        cost: 175,
        turns: 12,
        prerequisites: &[4],
        position: (3, 0),
        description_key: i18n::key!("classic.ui.screen-progress.tech.philosophy.desc"),
        unlocks_key: i18n::key!("classic.ui.screen-progress.tech.philosophy.unlocks"),
    },
];
pub(super) fn research_art_names() -> Vec<&'static str> {
    let mut names = vec!["research"];
    names.extend(TECHS.iter().map(|technology| technology.art));
    names
}
pub(super) struct Policy {
    art: &'static str,
    name_key: &'static str,
    pub prerequisite: Option<usize>,
    description_key: &'static str,
    effect_key: &'static str,
}
impl Policy {
    pub(super) fn name(&self) -> &'static str {
        i18n::tr(self.name_key)
    }
    fn description(&self) -> &'static str {
        i18n::tr(self.description_key)
    }
    fn effect(&self) -> &'static str {
        i18n::tr(self.effect_key)
    }
}
pub(super) const POLICIES: [Policy; 9] = [
    Policy {
        art: "tradition",
        name_key: i18n::key!("classic.ui.screen-progress.policy.tradition.name"),
        prerequisite: None,
        description_key: i18n::key!("classic.ui.screen-progress.policy.tradition.desc"),
        effect_key: i18n::key!("classic.ui.screen-progress.policy.tradition.effect"),
    },
    Policy {
        art: "tradition/aristocracy",
        name_key: i18n::key!("classic.ui.screen-progress.policy.aristocracy.name"),
        prerequisite: Some(0),
        description_key: i18n::key!("classic.ui.screen-progress.policy.aristocracy.desc"),
        effect_key: i18n::key!("classic.ui.screen-progress.policy.aristocracy.effect"),
    },
    Policy {
        art: "tradition/legalism",
        name_key: i18n::key!("classic.ui.screen-progress.policy.legalism.name"),
        prerequisite: Some(0),
        description_key: i18n::key!("classic.ui.screen-progress.policy.legalism.desc"),
        effect_key: i18n::key!("classic.ui.screen-progress.policy.legalism.effect"),
    },
    Policy {
        art: "liberty",
        name_key: i18n::key!("classic.ui.screen-progress.policy.liberty.name"),
        prerequisite: None,
        description_key: i18n::key!("classic.ui.screen-progress.policy.liberty.desc"),
        effect_key: i18n::key!("classic.ui.screen-progress.policy.liberty.effect"),
    },
    Policy {
        art: "liberty/republic",
        name_key: i18n::key!("classic.ui.screen-progress.policy.republic.name"),
        prerequisite: Some(3),
        description_key: i18n::key!("classic.ui.screen-progress.policy.republic.desc"),
        effect_key: i18n::key!("classic.ui.screen-progress.policy.republic.effect"),
    },
    Policy {
        art: "liberty/collective-rule",
        name_key: i18n::key!("classic.ui.screen-progress.policy.collective-rule.name"),
        prerequisite: Some(3),
        description_key: i18n::key!("classic.ui.screen-progress.policy.collective-rule.desc"),
        effect_key: i18n::key!("classic.ui.screen-progress.policy.collective-rule.effect"),
    },
    Policy {
        art: "honor",
        name_key: i18n::key!("classic.ui.screen-progress.policy.honour.name"),
        prerequisite: None,
        description_key: i18n::key!("classic.ui.screen-progress.policy.honour.desc"),
        effect_key: i18n::key!("classic.ui.screen-progress.policy.honour.effect"),
    },
    Policy {
        art: "honor/warrior-code",
        name_key: i18n::key!("classic.ui.screen-progress.policy.warrior-code.name"),
        prerequisite: Some(6),
        description_key: i18n::key!("classic.ui.screen-progress.policy.warrior-code.desc"),
        effect_key: i18n::key!("classic.ui.screen-progress.policy.warrior-code.effect"),
    },
    Policy {
        art: "honor/discipline",
        name_key: i18n::key!("classic.ui.screen-progress.policy.discipline.name"),
        prerequisite: Some(6),
        description_key: i18n::key!("classic.ui.screen-progress.policy.discipline.desc"),
        effect_key: i18n::key!("classic.ui.screen-progress.policy.discipline.effect"),
    },
];
fn banner(
    ui: &mut egui::Ui,
    r: Rect,
    state: &mut ScreenState,
    image: &'static str,
    title: &str,
    subtitle: &str,
) {
    art(state, ui, r, image);
    darken(ui.painter(), r, 65, 165);
    text(ui.painter(), r.min + vec2(24.0, 22.0), title, 33.0, CREAM);
    text(
        ui.painter(),
        r.min + vec2(26.0, 72.0),
        subtitle,
        14.0,
        CREAM,
    );
}
pub(super) fn research(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState) {
    let top = Rect::from_min_size(r.min, vec2(r.width(), 116.0));
    banner(
        ui,
        top,
        state,
        "research",
        t!("classic.ui.screen-progress.research-title"),
        t!("classic.ui.screen-progress.research-subtitle"),
    );
    let side_w = 300.0;
    let tree = Rect::from_min_max(
        pos2(r.left(), top.bottom() + 18.0),
        pos2(r.right() - side_w - 18.0, r.bottom()),
    );
    let side = Rect::from_min_max(pos2(tree.right() + 18.0, tree.top()), r.max);
    panel(ui.painter(), tree);
    let col_w = (tree.width() - 60.0) / 4.0;
    let card_w = col_w - 12.0;
    let card_h = 112.0;
    let row_gap = ((tree.height() - 162.0 - card_h * 3.0) / 2.0).clamp(16.0, 32.0);
    let first_row = 58.0 + ((tree.height() - 142.0 - card_h * 3.0 - row_gap * 2.0) * 0.5).max(0.0);
    let cards: Vec<_> = TECHS
        .iter()
        .map(|t| {
            Rect::from_min_size(
                tree.min
                    + vec2(
                        18.0 + t.position.0 as f32 * col_w,
                        first_row + t.position.1 as f32 * (card_h + row_gap),
                    ),
                vec2(card_w, card_h),
            )
        })
        .collect();
    for (i, label) in [
        t!("classic.ui.screen-progress.column-foundations"),
        t!("classic.ui.screen-progress.column-ancient"),
        t!("classic.ui.screen-progress.column-horizons"),
        t!("classic.ui.screen-progress.column-classical"),
    ]
    .into_iter()
    .enumerate()
    {
        text(
            ui.painter(),
            tree.min + vec2(20.0 + i as f32 * col_w, 20.0),
            label,
            9.0,
            GOLD,
        );
    }
    for (i, technology) in TECHS.iter().enumerate() {
        for &parent in technology.prerequisites {
            let a = cards[parent].right_center();
            let b = cards[i].left_center();
            let mid = (a.x + b.x) * 0.5;
            let color = if state.known.contains(&parent) {
                TEAL
            } else {
                Color32::from_rgb(58, 74, 76)
            };
            ui.painter().add(egui::Shape::line(
                vec![a, pos2(mid, a.y), pos2(mid, b.y), b],
                Stroke::new(2.0, color),
            ));
        }
    }
    for (i, technology) in TECHS.iter().enumerate() {
        let card = cards[i];
        let available = technology
            .prerequisites
            .iter()
            .all(|p| state.known.contains(p));
        let selected = state.research == i;
        let response = ui.interact(card, ui.id().with(("technology", i)), Sense::click());
        if response.clicked() {
            state.research = i;
        }
        ui.painter().rect_filled(
            card,
            5.0,
            if selected {
                Color32::from_rgb(34, 66, 70)
            } else {
                RAISED
            },
        );
        ui.painter().rect_stroke(
            card,
            5.0,
            Stroke::new(
                if selected { 1.7 } else { 1.0 },
                if selected {
                    GOLD
                } else {
                    Color32::from_rgb(56, 78, 79)
                },
            ),
            egui::StrokeKind::Inside,
        );
        let icon = Rect::from_min_size(card.min + vec2(8.0, 9.0), vec2(60.0, 60.0));
        art(state, ui, icon, technology.art);
        if !available {
            ui.painter()
                .rect_filled(icon, 0.0, Color32::from_black_alpha(95));
        }
        paragraph(
            ui.painter(),
            card.min + vec2(80.0, 11.0),
            technology.name(),
            16.0,
            if available { CREAM } else { MUTED },
            card.width() - 87.0,
        );
        let (status, color) = if state.known.contains(&i) {
            (t!("classic.ui.screen-progress.discovered").to_owned(), Color32::from_rgb(139, 188, 149))
        } else if state.current_research == Some(i) {
            (t!("classic.ui.screen-progress.in-progress").to_owned(), GOLD)
        } else if available {
            (
                tf!("classic.ui.screen-progress.science-turns", cost = technology.cost, n = technology.turns),
                MUTED,
            )
        } else {
            (
                tf!("classic.ui.screen-progress.requires", name = TECHS[technology.prerequisites[0]].name()),
                MUTED.gamma_multiply(0.75),
            )
        };
        paragraph(
            ui.painter(),
            pos2(card.left() + 9.0, card.bottom() - 31.0),
            &status,
            12.0,
            color,
            card.width() - 18.0,
        );
    }
    if let Some(index) = state.current_research {
        let project = &TECHS[index];
        let status = Rect::from_min_size(
            pos2(tree.left() + 18.0, tree.bottom() - 73.0),
            vec2(tree.width() - 36.0, 55.0),
        );
        ui.painter().rect_filled(status, 5.0, RAISED);
        text(
            ui.painter(),
            status.min + vec2(14.0, 9.0),
            tf!("classic.ui.screen-progress.researching", name = project.name()),
            16.0,
            CREAM,
        );
        ui.painter().text(
            status.right_top() + vec2(-14.0, 10.0),
            Align2::RIGHT_TOP,
            tf!("classic.ui.screen-progress.progress-turns", stored = state.research_progress, cost = project.cost, n = project.turns),
            FontId::proportional(13.0),
            GOLD,
        );
        let bar = Rect::from_min_size(
            status.min + vec2(14.0, 38.0),
            vec2(status.width() - 28.0, 5.0),
        );
        ui.painter().rect_filled(bar, 2.0, PANEL);
        ui.painter().rect_filled(
            Rect::from_min_size(
                bar.min,
                vec2(
                    bar.width() * state.research_progress as f32 / project.cost as f32,
                    5.0,
                ),
            ),
            2.0,
            TEAL,
        );
    }
    research_details(ui, side, state);
}
fn research_details(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState) {
    panel(ui.painter(), r);
    let t = &TECHS[state.research];
    let image = Rect::from_min_size(r.min + vec2(8.0, 8.0), vec2(r.width() - 16.0, 145.0));
    art(state, ui, image, t.art);
    darken(ui.painter(), image, 0, 90);
    text(ui.painter(), r.min + vec2(20.0, 173.0), t.name(), 26.0, CREAM);
    paragraph(
        ui.painter(),
        r.min + vec2(20.0, 216.0),
        t.description(),
        13.0,
        MUTED,
        r.width() - 40.0,
    );
    let unlock_y = (r.top() + 316.0).min(r.bottom() - 150.0);
    text(
        ui.painter(),
        pos2(r.left() + 20.0, unlock_y),
        t!("classic.ui.screen-progress.opens"),
        10.0,
        GOLD,
    );
    paragraph(
        ui.painter(),
        pos2(r.left() + 20.0, unlock_y + 24.0),
        t.unlocks(),
        13.0,
        CREAM,
        r.width() - 40.0,
    );
    let available = t.prerequisites.iter().all(|p| state.known.contains(p));
    let current = state.current_research == Some(state.research);
    let completed = state.known.contains(&state.research);
    let label = if completed {
        t!("classic.ui.screen-progress.already-discovered").to_owned()
    } else if current {
        t!("classic.ui.screen-progress.currently-researching").to_owned()
    } else if !available {
        t!("classic.ui.screen-progress.prerequisite-required").to_owned()
    } else {
        tf!("classic.ui.screen-progress.research", name = t.name())
    };
    if button(
        ui,
        Rect::from_min_size(
            pos2(r.left() + 16.0, r.bottom() - 54.0),
            vec2(r.width() - 32.0, 38.0),
        ),
        &label,
        true,
        available && !current && !completed,
    ) {
        state.start_research();
    }
}
pub(super) fn civics(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState) {
    let top = Rect::from_min_size(r.min, vec2(r.width(), 90.0));
    panel(ui.painter(), top);
    text(
        ui.painter(),
        top.min + vec2(24.0, 13.0),
        t!("classic.ui.screen-progress.policies-title"),
        32.0,
        CREAM,
    );
    text(
        ui.painter(),
        top.min + vec2(26.0, 57.0),
        t!("classic.ui.screen-progress.policies-subtitle"),
        13.0,
        MUTED,
    );
    if state.dev_mode
        && button(
            ui,
            Rect::from_min_size(top.right_top() + vec2(-196., 27.), vec2(172., 36.)),
            if state.policy_art.collection {
                t!("classic.ui.screen-progress.policy-layout")
            } else {
                t!("classic.ui.screen-progress.icon-collection")
            },
            state.policy_art.collection,
            true,
        )
    {
        state.policy_art.collection = !state.policy_art.collection;
    }
    let body = Rect::from_min_max(pos2(r.left(), top.bottom() + 18.0), r.max);
    if state.policy_art.collection {
        policy_art::draw(ui, body, &mut state.policy_art);
        return;
    }
    let main_w = body.width() - 310.0;
    let col_w = (main_w - 24.0) / 3.0;
    for (branch, (name, image, theme)) in [
        (POLICIES[0].name(), "tradition", t!("classic.ui.screen-progress.theme-tradition")),
        (POLICIES[3].name(), "liberty", t!("classic.ui.screen-progress.theme-liberty")),
        (POLICIES[6].name(), "honour", t!("classic.ui.screen-progress.theme-honour")),
    ]
    .into_iter()
    .enumerate()
    {
        let col = Rect::from_min_size(
            body.min + vec2(branch as f32 * (col_w + 12.0), 0.0),
            vec2(col_w, body.height()),
        );
        panel(ui.painter(), col);
        let portrait =
            Rect::from_center_size(pos2(col.center().x, col.top() + 64.0), vec2(92.0, 92.0));
        ui.painter()
            .circle_stroke(portrait.center(), 51.0, Stroke::new(1.5, GOLD));
        let badge = if image == "honour" { "honor" } else { image };
        if let Some(texture) = state.policy_art.texture(ui.ctx(), badge) {
            ui.painter().image(
                texture,
                portrait,
                Rect::from_min_max(pos2(0., 0.), pos2(1., 1.)),
                Color32::WHITE,
            );
        } else {
            art(state, ui, portrait, image);
        }
        ui.painter().text(
            pos2(col.center().x, col.top() + 121.0),
            Align2::CENTER_TOP,
            name,
            FontId::proportional(25.0),
            CREAM,
        );
        ui.painter().text(
            pos2(col.center().x, col.top() + 154.0),
            Align2::CENTER_TOP,
            theme,
            FontId::proportional(10.0),
            GOLD,
        );
        let node_w = (col.width() - 40.0) / 2.0;
        let nodes = [
            Rect::from_min_size(col.min + vec2(16.0, 192.0), vec2(col.width() - 32.0, 84.0)),
            Rect::from_min_size(col.min + vec2(14.0, 326.0), vec2(node_w, 143.0)),
            Rect::from_min_size(
                pos2(col.right() - 14.0 - node_w, col.top() + 326.0),
                vec2(node_w, 143.0),
            ),
        ];
        for n in 1..3 {
            let parent = POLICIES[branch * 3 + n].prerequisite.unwrap() - branch * 3;
            let a = nodes[parent].center_bottom();
            let b = nodes[n].center_top();
            ui.painter().add(egui::Shape::line(
                vec![a, pos2(a.x, a.y + 25.0), pos2(b.x, a.y + 25.0), b],
                Stroke::new(2.0, GOLD.gamma_multiply(0.55)),
            ));
        }
        for (n, node) in nodes.into_iter().enumerate() {
            let index = branch * 3 + n;
            policy_node(ui, node, index, n == 0, state);
        }
        let count = (branch * 3..branch * 3 + 3)
            .filter(|index| state.adopted.contains(index))
            .count();
        ui.painter().text(
            pos2(col.center().x, col.bottom() - 28.0),
            Align2::CENTER_CENTER,
            tf!("classic.ui.screen-progress.adopted-count", n = count),
            FontId::proportional(12.0),
            MUTED,
        );
    }
    policy_details(
        ui,
        Rect::from_min_max(pos2(body.left() + main_w + 18.0, body.top()), body.max),
        state,
    );
}

fn policy_node(ui: &mut egui::Ui, node: Rect, index: usize, opener: bool, state: &mut ScreenState) {
    let policy = &POLICIES[index];
    let unlocked = policy
        .prerequisite
        .is_none_or(|parent| state.adopted.contains(&parent));
    let adopted = state.adopted.contains(&index);
    if ui
        .interact(node, ui.id().with(("policy", index)), Sense::click())
        .clicked()
    {
        state.policy = index;
        state.branch = index / 3;
    }
    ui.painter().rect_filled(
        node,
        5.0,
        if state.policy == index {
            Color32::from_rgb(37, 65, 66)
        } else {
            RAISED
        },
    );
    ui.painter().rect_stroke(
        node,
        5.0,
        Stroke::new(
            if state.policy == index { 1.5 } else { 1.0 },
            if state.policy == index || adopted {
                GOLD
            } else {
                Color32::from_rgb(60, 78, 76)
            },
        ),
        egui::StrokeKind::Inside,
    );
    let emblem = if opener {
        node.min + vec2(39.0, 41.0)
    } else {
        pos2(node.center().x, node.top() + 35.0)
    };
    if let Some(texture) = state.policy_art.texture(ui.ctx(), policy.art) {
        ui.painter().image(
            texture,
            Rect::from_center_size(emblem, vec2(52., 52.)),
            Rect::from_min_max(pos2(0., 0.), pos2(1., 1.)),
            Color32::from_gray(if unlocked || adopted { 255 } else { 185 }),
        );
    } else {
        policy_emblem(ui.painter(), emblem, index, unlocked || adopted);
    }
    if opener {
        text(
            ui.painter(),
            node.min + vec2(75.0, 17.0),
            policy.name(),
            19.0,
            CREAM,
        );
        text(
            ui.painter(),
            node.min + vec2(75.0, 48.0),
            if adopted { t!("classic.ui.screen-progress.adopted") } else { t!("classic.ui.screen-progress.open-branch") },
            10.0,
            GOLD,
        );
    } else {
        let galley = ui.painter().layout(
            policy.name().to_owned(),
            FontId::proportional(16.0),
            if unlocked { CREAM } else { MUTED },
            node.width() - 12.0,
        );
        ui.painter().galley(
            pos2(node.center().x - galley.size().x * 0.5, node.top() + 70.0),
            galley,
            if unlocked { CREAM } else { MUTED },
        );
        ui.painter().text(
            pos2(node.center().x, node.bottom() - 16.0),
            Align2::CENTER_CENTER,
            if adopted {
                t!("classic.ui.screen-progress.adopted")
            } else if unlocked {
                t!("classic.ui.screen-progress.available")
            } else {
                t!("classic.ui.screen-progress.branch-required")
            },
            FontId::proportional(9.0),
            GOLD,
        );
    }
}

fn policy_emblem(p: &egui::Painter, center: egui::Pos2, index: usize, enabled: bool) {
    let ink = if enabled {
        GOLD
    } else {
        Color32::from_rgb(110, 108, 90)
    };
    p.circle_filled(center, 25.0, PANEL);
    p.circle_stroke(center, 25.0, Stroke::new(1.4, ink));
    let stroke = Stroke::new(2.0, ink);
    match index % 3 {
        0 => {
            // Branch seal: a laurel-like fan around a central pillar.
            p.rect_filled(Rect::from_center_size(center, vec2(5.0, 27.0)), 1.0, ink);
            for side in [-1.0, 1.0] {
                p.line_segment(
                    [
                        center + vec2(side * 5.0, 12.0),
                        center + vec2(side * 16.0, -9.0),
                    ],
                    stroke,
                );
                for y in [-7.0, 0.0, 7.0] {
                    p.line_segment(
                        [
                            center + vec2(side * 11.0, y),
                            center + vec2(side * 17.0, y - 4.0),
                        ],
                        stroke,
                    );
                }
            }
        }
        1 => {
            // Crown: aspiration, leadership and civic patronage.
            p.add(egui::Shape::closed_line(
                vec![
                    center + vec2(-15.0, -8.0),
                    center + vec2(-9.0, 3.0),
                    center + vec2(0.0, -14.0),
                    center + vec2(9.0, 3.0),
                    center + vec2(15.0, -8.0),
                    center + vec2(11.0, 11.0),
                    center + vec2(-11.0, 11.0),
                ],
                stroke,
            ));
        }
        _ => {
            // Inscribed shield: common law and collective discipline.
            p.add(egui::Shape::closed_line(
                vec![
                    center + vec2(-13.0, -12.0),
                    center + vec2(13.0, -12.0),
                    center + vec2(12.0, 6.0),
                    center + vec2(0.0, 16.0),
                    center + vec2(-12.0, 6.0),
                ],
                stroke,
            ));
            p.line_segment(
                [center + vec2(-7.0, -5.0), center + vec2(7.0, -5.0)],
                stroke,
            );
            p.line_segment([center + vec2(-7.0, 1.0), center + vec2(7.0, 1.0)], stroke);
        }
    }
}

fn policy_details(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState) {
    panel(ui.painter(), r);
    let p = &POLICIES[state.policy];
    let image = ["tradition", "liberty", "honour"][state.branch];
    let hero = Rect::from_min_size(r.min + vec2(8.0, 8.0), vec2(r.width() - 16.0, 140.0));
    art(state, ui, hero, image);
    text(ui.painter(), r.min + vec2(20.0, 170.0), p.name(), 25.0, CREAM);
    paragraph(
        ui.painter(),
        r.min + vec2(20.0, 211.0),
        p.description(),
        13.0,
        MUTED,
        r.width() - 40.0,
    );
    let effect_y = (r.top() + 310.0).min(r.bottom() - 153.0);
    text(
        ui.painter(),
        pos2(r.left() + 20.0, effect_y),
        t!("classic.ui.screen-progress.lasting-influence"),
        10.0,
        GOLD,
    );
    paragraph(
        ui.painter(),
        pos2(r.left() + 20.0, effect_y + 24.0),
        p.effect(),
        13.0,
        CREAM,
        r.width() - 40.0,
    );
    let adopted = state.adopted.contains(&state.policy);
    let unlocked = p.prerequisite.is_none_or(|pr| state.adopted.contains(&pr));
    let label = if state.game_mode {
        t!("classic.ui.screen-progress.not-in-rules").to_owned()
    } else if adopted {
        t!("classic.ui.screen-progress.policy-adopted").to_owned()
    } else if !unlocked {
        t!("classic.ui.screen-progress.adopt-branch-first").to_owned()
    } else if state.policy_points == 0 {
        t!("classic.ui.screen-progress.no-choices").to_owned()
    } else {
        tf!("classic.ui.screen-progress.adopt", name = p.name())
    };
    if button(
        ui,
        Rect::from_min_size(
            pos2(r.left() + 16.0, r.bottom() - 54.0),
            vec2(r.width() - 32.0, 38.0),
        ),
        &label,
        true,
        !state.game_mode && !adopted && unlocked && state.policy_points > 0,
    ) {
        state.adopt_policy();
    }
}
