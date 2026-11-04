//! Resolution-independent, font-independent symbols for world-screen identities.
//! Small path families are drawn directly; no texture uploads or per-frame images.
use egui::{Color32, Painter, Rect, Shape, Stroke};

struct Glyph<'a> {
    p: &'a Painter,
    r: Rect,
    ink: Color32,
    ground: Color32,
}
impl Glyph<'_> {
    fn point(&self, x: f32, y: f32) -> egui::Pos2 {
        self.r.center() + egui::vec2(x, y) * self.r.width().min(self.r.height()) * 0.42
    }
    fn line(&self, points: &[(f32, f32)]) {
        self.p.add(Shape::line(
            points.iter().map(|&(x, y)| self.point(x, y)).collect(),
            Stroke::new((self.r.width() * 0.06).max(1.3), self.ink),
        ));
    }
    fn ring(&self, x: f32, y: f32, r: f32) {
        self.p.circle_stroke(
            self.point(x, y),
            r * self.r.width().min(self.r.height()) * 0.42,
            Stroke::new((self.r.width() * 0.055).max(1.2), self.ink),
        );
    }
    fn disc(&self, x: f32, y: f32, r: f32, c: Color32) {
        self.p.circle_filled(
            self.point(x, y),
            r * self.r.width().min(self.r.height()) * 0.42,
            c,
        );
    }
    fn arc(&self, x: f32, y: f32, r: f32, start: f32, end: f32) {
        let pts: Vec<_> = (0..=24)
            .map(|i| {
                let a = start + (end - start) * i as f32 / 24.;
                (x + r * a.cos(), y + r * a.sin())
            })
            .collect();
        self.line(&pts);
    }
}

#[cfg(test)]
pub(super) const RELIGIONS: [&str; 14] = [
    "Pantheon", // i18n-ok: rule-data id
    "Buddhism", // i18n-ok: rule-data id
    "Catholicism", // i18n-ok: rule-data id
    "Confucianism", // i18n-ok: rule-data id
    "Eastern Orthodoxy", // i18n-ok: rule-data id
    "Hinduism", // i18n-ok: rule-data id
    "Islam", // i18n-ok: rule-data id
    "Judaism", // i18n-ok: rule-data id
    "Protestantism", // i18n-ok: rule-data id
    "Shinto", // i18n-ok: rule-data id
    "Sikhism", // i18n-ok: rule-data id
    "Taoism", // i18n-ok: rule-data id
    "Tengriism", // i18n-ok: rule-data id
    "Zoroastrianism", // i18n-ok: rule-data id
];

pub(super) fn religion(p: &Painter, r: Rect, name: &str, ink: Color32, ground: Color32) {
    let g = Glyph { p, r, ink, ground };
    let pi = std::f32::consts::PI;
    match name {
        "Buddhism" => { // i18n-ok: rule-data id
            g.ring(0., 0., 0.76);
            g.ring(0., 0., 0.18);
            for i in 0..8 {
                let a = i as f32 * pi / 4.;
                g.line(&[
                    (0.18 * a.cos(), 0.18 * a.sin()),
                    (0.96 * a.cos(), 0.96 * a.sin()),
                ]);
            }
        }
        "Catholicism" => { // i18n-ok: rule-data id
            g.line(&[(0., -0.95), (0., 0.95)]);
            g.line(&[(-0.62, -0.36), (0.62, -0.36)]);
        }
        "Eastern Orthodoxy" => { // i18n-ok: rule-data id
            g.line(&[(0., -0.98), (0., 0.98)]);
            g.line(&[(-0.32, -0.72), (0.32, -0.72)]);
            g.line(&[(-0.65, -0.28), (0.65, -0.28)]);
            g.line(&[(-0.4, 0.35), (0.4, 0.65)]);
        }
        "Protestantism" => { // i18n-ok: rule-data id
            g.line(&[(0., -0.85), (0., 0.85)]);
            g.line(&[(-0.5, -0.28), (0.5, -0.28)]);
            for (x, y) in [(-0.8, -0.75), (0.8, -0.75), (-0.85, 0.55), (0.85, 0.55)] {
                g.line(&[(x * 0.72, y * 0.72), (x, y)]);
            }
        }
        "Judaism" => { // i18n-ok: rule-data id
            g.line(&[(0., -0.94), (0.82, 0.47), (-0.82, 0.47), (0., -0.94)]);
            g.line(&[(0., 0.94), (0.82, -0.47), (-0.82, -0.47), (0., 0.94)]);
        }
        "Islam" => { // i18n-ok: rule-data id
            g.disc(-0.12, 0., 0.86, ink);
            g.disc(0.2, -0.2, 0.73, g.ground);
            star(&g, 0.57, -0.19, 0.31, 5);
        }
        "Taoism" => { // i18n-ok: rule-data id
            g.disc(0., 0., 0.88, ink);
            let pts: Vec<_> = (0..=24)
                .map(|i| {
                    let a = -pi / 2. + pi * i as f32 / 24.;
                    g.point(0.88 * a.cos(), 0.88 * a.sin())
                })
                .collect();
            g.p.add(Shape::convex_polygon(pts, g.ground, Stroke::NONE));
            g.disc(0., -0.44, 0.44, ink);
            g.disc(0., 0.44, 0.44, g.ground);
            g.disc(0., -0.44, 0.12, g.ground);
            g.disc(0., 0.44, 0.12, ink);
            g.ring(0., 0., 0.88);
        }
        "Shinto" => { // i18n-ok: rule-data id
            g.line(&[(-0.92, -0.72), (0., -0.58), (0.92, -0.72)]);
            g.line(&[(-0.78, -0.32), (0.78, -0.32)]);
            g.line(&[(-0.5, -0.58), (-0.62, 0.95)]);
            g.line(&[(0.5, -0.58), (0.62, 0.95)]);
            g.line(&[(0., -0.55), (0., -0.32)]);
        }
        "Confucianism" => { // i18n-ok: rule-data id
            g.line(&[(0., -0.95), (0., 0.9), (-0.22, 0.82)]);
            g.line(&[(-0.78, -0.24), (-0.35, -0.24), (-0.55, 0.26), (-0.9, 0.6)]);
            g.line(&[(0.85, -0.45), (0.32, 0.), (0.55, 0.4), (0.9, 0.65)]);
        }
        "Hinduism" => { // i18n-ok: rule-data id
            g.arc(-0.34, -0.28, 0.42, -pi * 0.7, pi * 0.7);
            g.arc(-0.26, 0.38, 0.48, -pi * 0.7, pi * 0.85);
            g.line(&[(0.05, 0.3), (0.35, 0.08)]);
            g.arc(0.5, 0.26, 0.35, -pi * 0.75, pi * 0.72);
            g.arc(0.36, -0.87, 0.28, 0.15, pi - 0.15);
            g.disc(0.36, -0.96, 0.07, ink);
        }
        "Sikhism" => { // i18n-ok: rule-data id
            g.ring(0., 0.05, 0.49);
            g.line(&[
                (0., -0.98),
                (-0.14, -0.5),
                (0., 0.75),
                (0.14, -0.5),
                (0., -0.98),
            ]);
            g.arc(0., -0.04, 0.84, 0.12, pi - 0.12);
            g.line(&[(-0.83, 0.08), (-0.55, -0.65)]);
            g.line(&[(0.83, 0.08), (0.55, -0.65)]);
            g.line(&[(0., 0.65), (0., 0.97)]);
        }
        "Tengriism" => { // i18n-ok: rule-data id
            g.ring(0., 0., 0.82);
            g.line(&[(-0.97, 0.), (0.97, 0.)]);
            g.line(&[(0., -0.97), (0., 0.97)]);
            g.disc(0., 0., 0.15, ink);
        }
        "Zoroastrianism" => { // i18n-ok: rule-data id
            g.ring(0., 0.1, 0.31);
            g.disc(0., -0.66, 0.16, ink);
            g.line(&[(0., -0.48), (0., 0.05)]);
            for y in [-0.18, 0.06, 0.3] {
                g.line(&[(-0.25, y), (-0.95, y - 0.15)]);
                g.line(&[(0.25, y), (0.95, y - 0.15)]);
            }
            g.line(&[(-0.28, 0.75), (0., 0.48), (0.28, 0.75)]);
        }
        _ => {
            // Pantheon: a classical sanctuary; also a safe modded-name fallback.
            g.line(&[(-0.95, -0.42), (0., -0.95), (0.95, -0.42), (-0.95, -0.42)]);
            for x in [-0.58, 0., 0.58] {
                g.line(&[(x, -0.25), (x, 0.65)]);
            }
            g.line(&[(-0.9, 0.75), (0.9, 0.75)]);
            g.line(&[(-1., 0.95), (1., 0.95)]);
        }
    }
}
fn star(g: &Glyph<'_>, x: f32, y: f32, r: f32, n: usize) {
    let pts: Vec<_> = (0..=2 * n)
        .map(|i| {
            let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / n as f32;
            let radius = if i % 2 == 0 { r } else { r * 0.42 };
            (x + radius * a.cos(), y + radius * a.sin())
        })
        .collect();
    g.line(&pts);
}

pub(super) fn city_state(p: &Painter, r: Rect, kind: &str, ink: Color32, ground: Color32) {
    let g = Glyph { p, r, ink, ground };
    match kind {
        "Maritime" => { // i18n-ok: rule-data id
            g.ring(0., -0.72, 0.18);
            g.line(&[(0., -0.52), (0., 0.83)]);
            g.line(&[(-0.5, -0.15), (0.5, -0.15)]);
            g.arc(0., 0.12, 0.75, 0., std::f32::consts::PI);
            g.line(&[(-0.75, 0.12), (-0.9, 0.42)]);
            g.line(&[(0.75, 0.12), (0.9, 0.42)]);
        }
        "Mercantile" => { // i18n-ok: rule-data id
            g.ring(-0.28, -0.24, 0.56);
            g.ring(0.28, 0.3, 0.56);
            g.line(&[(0.28, 0.), (0.28, 0.6)]);
        }
        "Militaristic" => { // i18n-ok: rule-data id
            g.line(&[(-0.7, 0.8), (0.65, -0.85), (0.8, -0.65), (-0.55, 0.9)]);
            g.line(&[(-0.83, 0.27), (-0.2, 0.8)]);
            g.line(&[(0.7, 0.8), (-0.65, -0.85), (-0.8, -0.65), (0.55, 0.9)]);
            g.line(&[(0.83, 0.27), (0.2, 0.8)]);
        }
        "Religious" => { // i18n-ok: rule-data id
            star(&g, 0., 0., 0.95, 8);
            g.ring(0., 0., 0.25);
        }
        _ => {
            // Cultured: lyre.
            g.line(&[
                (-0.72, -0.82),
                (-0.65, 0.3),
                (-0.3, 0.85),
                (0.3, 0.85),
                (0.65, 0.3),
                (0.72, -0.82),
            ]);
            g.line(&[(-0.7, -0.55), (0.7, -0.55)]);
            for x in [-0.3, 0., 0.3] {
                g.line(&[(x, -0.55), (x, 0.65)]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_shipped_religions_have_font_independent_symbols() {
        let rules = crate::ui::screens::game_rules();
        for religion in &rules.module.religions {
            assert!(
                RELIGIONS.contains(&religion.name.as_str()),
                "{}",
                religion.name
            );
        }
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(400., 400.),
                )),
                ..Default::default()
            },
            |ui| {
                for name in RELIGIONS {
                    religion(
                        ui.painter(),
                        Rect::from_min_size(egui::pos2(20., 20.), egui::vec2(32., 32.)),
                        name,
                        Color32::GOLD,
                        Color32::BLACK,
                    );
                }
            },
        );
        output.textures_delta.clear();
        assert!(output.shapes.len() > 50, "{}", output.shapes.len());
        assert!(
            !output
                .shapes
                .iter()
                .any(|shape| matches!(shape.shape, Shape::Text(_))),
            "symbols must not depend on font glyphs"
        );
    }
}

/// A promotion's family is a visual aid; its exact name and effects remain authoritative.
/// Unrecognised/modded traits use the Special medal, never a fabricated effect symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum PromotionFamily {
    Attack,
    Ranged,
    Defense,
    Healing,
    Movement,
    Recon,
    Naval,
    Air,
    Terrain,
    Support,
    Restriction,
    Special,
}

pub(crate) fn promotion_family(key: &str) -> PromotionFamily {
    use PromotionFamily::*;
    let s = key.rsplit('/').next().unwrap_or(key);
    let contains = |words: &[&str]| words.iter().any(|word| s.contains(word));
    if contains(&[
        "penalty",
        "impassable",
        "no-capture",
        "no-defensive",
        "only-",
        "must-set-up",
        "ends-turn",
        "weak-ranged",
    ]) {
        Restriction
    } else if contains(&["heal", "repair", "medic", "march", "survivalism"]) {
        Healing
    } else if contains(&[
        "air-",
        "heli-",
        "anti-fighter",
        "anti-helicopter",
        "dogfight",
        "intercept",
        "bombard",
        "evasion",
        "flight-deck",
        "sortie",
        "payload",
    ]) {
        Air
    } else if contains(&[
        "embark",
        "naval",
        "ocean",
        "submarine",
        "wolfpack",
        "boarding",
        "coastal",
        "ship",
        "cargo",
        "canoe",
        "admiral",
        "lighthouse",
        "silent-hunter",
    ]) {
        Naval
    } else if contains(&["sight", "scouting", "sentry", "recon", "invisible"]) {
        Recon
    } else if contains(&[
        "mov",
        "mobility",
        "paradrop",
        "hover",
        "restless",
        "steam",
        "withdraw",
        "logistics",
    ]) {
        Movement
    } else if contains(&[
        "woods",
        "jungle",
        "jaguar",
        "mohawk",
        "desert",
        "terrain",
        "ski-",
        "altitude",
        "amphibious",
    ]) {
        Terrain
    } else if contains(&[
        "accuracy",
        "barrage",
        "targeting",
        "range",
        "volley",
        "fire",
        "siege",
        "sapper",
    ]) {
        Ranged
    } else if contains(&[
        "cover",
        "defen",
        "armor-plating",
        "formation",
        "guardian",
        "himeji",
        "invulner",
    ]) {
        Defense
    } else if contains(&[
        "attack",
        "anti-",
        "shock",
        "drill",
        "ambush",
        "charge",
        "flank",
        "blitz",
        "assault",
        "stronger",
        "enemy-blade",
    ]) {
        Attack
    } else if contains(&[
        "general",
        "supply",
        "experience",
        "recruit",
        "trade",
        "goods",
        "plunder",
        "pillage",
        "golden",
        "foreign",
        "rival",
        "great-",
    ]) {
        Support
    } else {
        Special
    }
}
pub(crate) fn promotion_rank(key: &str) -> u8 {
    match key.rsplit('-').next().unwrap_or("") {
        "1" | "i" => 1,
        "2" | "ii" => 2,
        "3" | "iii" => 3,
        "4" | "iv" => 4,
        _ => 0,
    }
}
pub(crate) fn promotion(p: &Painter, r: Rect, key: &str, ink: Color32, ground: Color32) {
    use PromotionFamily::*;
    let symbol = Rect::from_center_size(
        r.center() - egui::vec2(0., r.height() * 0.08),
        r.size() * 0.78,
    );
    let g = Glyph {
        p,
        r: symbol,
        ink,
        ground,
    };
    match promotion_family(key) {
        Attack => city_state(p, symbol, "Militaristic", ink, ground), // i18n-ok: rule-data id
        Naval => city_state(p, symbol, "Maritime", ink, ground), // i18n-ok: rule-data id
        Special => {
            star(&g, 0., -0.15, 0.8, 5);
            g.line(&[
                (-0.35, 0.5),
                (-0.4, 0.98),
                (0., 0.8),
                (0.4, 0.98),
                (0.35, 0.5),
            ]);
        }
        Restriction => {
            g.ring(0., 0., 0.85);
            g.line(&[(-0.6, 0.6), (0.6, -0.6)]);
        }
        Defense => {
            g.line(&[
                (-0.75, -0.75),
                (0., -0.98),
                (0.75, -0.75),
                (0.6, 0.35),
                (0., 0.98),
                (-0.6, 0.35),
                (-0.75, -0.75),
            ]);
            g.line(&[(0., -0.62), (0., 0.65)]);
        }
        Healing => {
            g.line(&[
                (-0.28, -0.9),
                (0.28, -0.9),
                (0.28, -0.28),
                (0.9, -0.28),
                (0.9, 0.28),
                (0.28, 0.28),
                (0.28, 0.9),
                (-0.28, 0.9),
                (-0.28, 0.28),
                (-0.9, 0.28),
                (-0.9, -0.28),
                (-0.28, -0.28),
                (-0.28, -0.9),
            ]);
        }
        Movement => {
            g.line(&[(-0.85, -0.65), (-0.15, 0.), (-0.85, 0.65)]);
            g.line(&[(0., -0.65), (0.75, 0.), (0., 0.65)]);
        }
        Recon => {
            g.line(&[
                (-0.98, 0.),
                (-0.5, -0.48),
                (0., -0.64),
                (0.5, -0.48),
                (0.98, 0.),
                (0.5, 0.48),
                (0., 0.64),
                (-0.5, 0.48),
                (-0.98, 0.),
            ]);
            g.ring(0., 0., 0.28);
        }
        Ranged => {
            g.ring(0., 0., 0.75);
            g.ring(0., 0., 0.36);
            g.line(&[(-0.95, 0.), (0.95, 0.)]);
            g.line(&[(0., -0.95), (0., 0.95)]);
        }
        Air => {
            g.line(&[
                (0., -0.98),
                (0.2, -0.12),
                (0.95, 0.35),
                (0.9, 0.55),
                (0.15, 0.3),
                (0.12, 0.75),
                (0.4, 0.95),
                (-0.4, 0.95),
                (-0.12, 0.75),
                (-0.15, 0.3),
                (-0.9, 0.55),
                (-0.95, 0.35),
                (-0.2, -0.12),
                (0., -0.98),
            ]);
        }
        Terrain => {
            g.line(&[(-0.95, 0.8), (-0.25, -0.82), (0.48, 0.8)]);
            g.line(&[(0.12, 0.1), (0.55, -0.45), (0.95, 0.8)]);
            g.line(&[(-0.5, -0.2), (-0.25, -0.05), (0., -0.2)]);
        }
        Support => {
            g.line(&[
                (-0.82, -0.55),
                (0., -0.95),
                (0.82, -0.55),
                (0.82, 0.55),
                (0., 0.95),
                (-0.82, 0.55),
                (-0.82, -0.55),
                (0., -0.15),
                (0.82, -0.55),
            ]);
            g.line(&[(0., -0.15), (0., 0.95)]);
        }
    }
    let rank = promotion_rank(key);
    let step = r.width() * 0.15;
    for i in 0..rank {
        let x = r.center().x + (i as f32 - (rank as f32 - 1.) / 2.) * step;
        p.circle_filled(
            egui::pos2(x, r.bottom() - r.height() * 0.075),
            r.width() * 0.035,
            ink,
        );
    }
}

#[cfg(test)]
mod promotion_tests {
    use super::*;
    #[test]
    fn shipped_promotion_symbol_coverage_and_rank() {
        let rules = crate::ui::screens::game_rules();
        let mut counts = std::collections::BTreeMap::new();
        let mut fallback = Vec::new();
        for def in &rules.promotions {
            let family = promotion_family(&def.key);
            *counts.entry(family).or_insert(0usize) += 1;
            if family == PromotionFamily::Special {
                fallback.push(def.key.as_str());
            }
        }
        assert_eq!(rules.promotions.len(), 214);
        assert_eq!(counts.values().sum::<usize>(), 214);
        assert_eq!(promotion_rank("core:promotion/accuracy-3"), 3);
        assert_eq!(promotion_rank("core:promotion/cargo-iv"), 4);
        assert_eq!(promotion_rank("core:promotion/march"), 0);
        eprintln!("Promotion symbol coverage: {counts:?}; Special category fallback: {fallback:?}");
    }
}
