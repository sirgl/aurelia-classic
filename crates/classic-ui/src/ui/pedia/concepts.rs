//! Player-facing explanations, independent of development notes and reference formulas.
//! The order is persistent navigation identity: keep all existing concept indices stable.
use i18n::key;

use super::{Category, Entry, Line, PediaRef, Table};

struct Concept {
    title: &'static str,
    part: &'static str,
    paragraphs: &'static [&'static str],
}

const ECONOMY: &str = key!("classic.pedia.concepts.part-economy");
const EMPIRE: &str = key!("classic.pedia.concepts.part-empire");
const COMBAT: &str = key!("classic.pedia.concepts.part-combat");

const CONCEPTS: [Concept; 30] = [
    Concept { title: key!("classic.pedia.concepts.turn-order.title"), part: ECONOMY, paragraphs: &[
        key!("classic.pedia.concepts.turn-order.p1"),
        key!("classic.pedia.concepts.turn-order.p2"),
        key!("classic.pedia.concepts.turn-order.p3"),
    ] },
    Concept { title: key!("classic.pedia.concepts.city-growth.title"), part: ECONOMY, paragraphs: &[
        key!("classic.pedia.concepts.city-growth.p1"),
        key!("classic.pedia.concepts.city-growth.p2"),
        key!("classic.pedia.concepts.city-growth.p3"),
    ] },
    Concept { title: key!("classic.pedia.concepts.border-growth.title"), part: ECONOMY, paragraphs: &[
        key!("classic.pedia.concepts.border-growth.p1"),
        key!("classic.pedia.concepts.border-growth.p2"),
        key!("classic.pedia.concepts.border-growth.p3"),
    ] },
    Concept { title: key!("classic.pedia.concepts.buying-tiles.title"), part: ECONOMY, paragraphs: &[
        key!("classic.pedia.concepts.buying-tiles.p1"),
        key!("classic.pedia.concepts.buying-tiles.p2"),
        key!("classic.pedia.concepts.buying-tiles.p3"),
    ] },
    Concept { title: key!("classic.pedia.concepts.production-and-purchase-costs.title"), part: ECONOMY, paragraphs: &[
        key!("classic.pedia.concepts.production-and-purchase-costs.p1"),
        key!("classic.pedia.concepts.production-and-purchase-costs.p2"),
        key!("classic.pedia.concepts.production-and-purchase-costs.p3"),
    ] },
    Concept { title: key!("classic.pedia.concepts.gold.title"), part: ECONOMY, paragraphs: &[
        key!("classic.pedia.concepts.gold.p1"),
        key!("classic.pedia.concepts.gold.p2"),
        key!("classic.pedia.concepts.gold.p3"),
    ] },
    Concept { title: key!("classic.pedia.concepts.happiness.title"), part: ECONOMY, paragraphs: &[
        key!("classic.pedia.concepts.happiness.p1"),
        key!("classic.pedia.concepts.happiness.p2"),
        key!("classic.pedia.concepts.happiness.p3"),
    ] },
    Concept { title: key!("classic.pedia.concepts.golden-ages.title"), part: ECONOMY, paragraphs: &[
        key!("classic.pedia.concepts.golden-ages.p1"),
        key!("classic.pedia.concepts.golden-ages.p2"),
        key!("classic.pedia.concepts.golden-ages.p3"),
    ] },
    Concept { title: key!("classic.pedia.concepts.social-policies.title"), part: ECONOMY, paragraphs: &[
        key!("classic.pedia.concepts.social-policies.p1"),
        key!("classic.pedia.concepts.social-policies.p2"),
        key!("classic.pedia.concepts.social-policies.p3"),
    ] },
    Concept { title: key!("classic.pedia.concepts.science.title"), part: ECONOMY, paragraphs: &[
        key!("classic.pedia.concepts.science.p1"),
        key!("classic.pedia.concepts.science.p2"),
        key!("classic.pedia.concepts.science.p3"),
    ] },
    Concept { title: key!("classic.pedia.concepts.great-people.title"), part: ECONOMY, paragraphs: &[
        key!("classic.pedia.concepts.great-people.p1"),
        key!("classic.pedia.concepts.great-people.p2"),
        key!("classic.pedia.concepts.great-people.p3"),
    ] },
    Concept { title: key!("classic.pedia.concepts.faith-and-religion.title"), part: ECONOMY, paragraphs: &[
        key!("classic.pedia.concepts.faith-and-religion.p1"),
        key!("classic.pedia.concepts.faith-and-religion.p2"),
        key!("classic.pedia.concepts.faith-and-religion.p3"),
    ] },
    Concept { title: key!("classic.pedia.concepts.score-and-the-end-of-the-game.title"), part: ECONOMY, paragraphs: &[
        key!("classic.pedia.concepts.score-and-the-end-of-the-game.p1"),
        key!("classic.pedia.concepts.score-and-the-end-of-the-game.p2"),
        key!("classic.pedia.concepts.score-and-the-end-of-the-game.p3"),
    ] },
    Concept { title: key!("classic.pedia.concepts.tourism-and-influence.title"), part: EMPIRE, paragraphs: &[
        key!("classic.pedia.concepts.tourism-and-influence.p1"),
        key!("classic.pedia.concepts.tourism-and-influence.p2"),
        key!("classic.pedia.concepts.tourism-and-influence.p3"),
    ] },
    Concept { title: key!("classic.pedia.concepts.great-works-and-archaeology.title"), part: EMPIRE, paragraphs: &[
        key!("classic.pedia.concepts.great-works-and-archaeology.p1"),
        key!("classic.pedia.concepts.great-works-and-archaeology.p2"),
        key!("classic.pedia.concepts.great-works-and-archaeology.p3"),
    ] },
    Concept { title: key!("classic.pedia.concepts.trade-routes.title"), part: EMPIRE, paragraphs: &[
        key!("classic.pedia.concepts.trade-routes.p1"),
        key!("classic.pedia.concepts.trade-routes.p2"),
        key!("classic.pedia.concepts.trade-routes.p3"),
    ] },
    Concept { title: key!("classic.pedia.concepts.city-states.title"), part: EMPIRE, paragraphs: &[
        key!("classic.pedia.concepts.city-states.p1"),
        key!("classic.pedia.concepts.city-states.p2"),
        key!("classic.pedia.concepts.city-states.p3"),
    ] },
    Concept { title: key!("classic.pedia.concepts.diplomacy.title"), part: EMPIRE, paragraphs: &[
        key!("classic.pedia.concepts.diplomacy.p1"),
        key!("classic.pedia.concepts.diplomacy.p2"),
        key!("classic.pedia.concepts.diplomacy.p3"),
    ] },
    Concept { title: key!("classic.pedia.concepts.world-congress-and-united-nations.title"), part: EMPIRE, paragraphs: &[
        key!("classic.pedia.concepts.world-congress-and-united-nations.p1"),
        key!("classic.pedia.concepts.world-congress-and-united-nations.p2"),
        key!("classic.pedia.concepts.world-congress-and-united-nations.p3"),
    ] },
    Concept { title: key!("classic.pedia.concepts.espionage.title"), part: EMPIRE, paragraphs: &[
        key!("classic.pedia.concepts.espionage.p1"),
        key!("classic.pedia.concepts.espionage.p2"),
        key!("classic.pedia.concepts.espionage.p3"),
    ] },
    Concept { title: key!("classic.pedia.concepts.victory-conditions.title"), part: EMPIRE, paragraphs: &[
        key!("classic.pedia.concepts.victory-conditions.p1"),
        key!("classic.pedia.concepts.victory-conditions.p2"),
        key!("classic.pedia.concepts.victory-conditions.p3"),
    ] },
    Concept { title: key!("classic.pedia.concepts.unit-combat-strength.title"), part: COMBAT, paragraphs: &[
        key!("classic.pedia.concepts.unit-combat-strength.p1"),
        key!("classic.pedia.concepts.unit-combat-strength.p2"),
        key!("classic.pedia.concepts.unit-combat-strength.p3"),
    ] },
    Concept { title: key!("classic.pedia.concepts.combat-damage.title"), part: COMBAT, paragraphs: &[
        key!("classic.pedia.concepts.combat-damage.p1"),
        key!("classic.pedia.concepts.combat-damage.p2"),
        key!("classic.pedia.concepts.combat-damage.p3"),
    ] },
    Concept { title: key!("classic.pedia.concepts.ranged-combat.title"), part: COMBAT, paragraphs: &[
        key!("classic.pedia.concepts.ranged-combat.p1"),
        key!("classic.pedia.concepts.ranged-combat.p2"),
        key!("classic.pedia.concepts.ranged-combat.p3"),
    ] },
    Concept { title: key!("classic.pedia.concepts.cities-in-combat.title"), part: COMBAT, paragraphs: &[
        key!("classic.pedia.concepts.cities-in-combat.p1"),
        key!("classic.pedia.concepts.cities-in-combat.p2"),
        key!("classic.pedia.concepts.cities-in-combat.p3"),
    ] },
    Concept { title: key!("classic.pedia.concepts.healing.title"), part: COMBAT, paragraphs: &[
        key!("classic.pedia.concepts.healing.p1"),
        key!("classic.pedia.concepts.healing.p2"),
        key!("classic.pedia.concepts.healing.p3"),
    ] },
    Concept { title: key!("classic.pedia.concepts.experience-and-promotions.title"), part: COMBAT, paragraphs: &[
        key!("classic.pedia.concepts.experience-and-promotions.p1"),
        key!("classic.pedia.concepts.experience-and-promotions.p2"),
        key!("classic.pedia.concepts.experience-and-promotions.p3"),
    ] },
    Concept { title: key!("classic.pedia.concepts.movement.title"), part: COMBAT, paragraphs: &[
        key!("classic.pedia.concepts.movement.p1"),
        key!("classic.pedia.concepts.movement.p2"),
        key!("classic.pedia.concepts.movement.p3"),
    ] },
    Concept { title: key!("classic.pedia.concepts.one-unit-per-tile.title"), part: COMBAT, paragraphs: &[
        key!("classic.pedia.concepts.one-unit-per-tile.p1"),
        key!("classic.pedia.concepts.one-unit-per-tile.p2"),
        key!("classic.pedia.concepts.one-unit-per-tile.p3"),
    ] },
    Concept { title: key!("classic.pedia.concepts.air-units.title"), part: COMBAT, paragraphs: &[
        key!("classic.pedia.concepts.air-units.p1"),
        key!("classic.pedia.concepts.air-units.p2"),
        key!("classic.pedia.concepts.air-units.p3"),
    ] },
];

pub fn count() -> usize {
    CONCEPTS.len()
}
pub fn title(index: usize) -> Option<&'static str> {
    CONCEPTS.get(index).map(|c| i18n::tr(c.title))
}

pub fn entries() -> Vec<Entry> {
    CONCEPTS
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let id = PediaRef::new(Table::Concept, i as u16);
            let mut entry = Entry::new(id, Category::Concepts, &format!("concept:{i}"), i18n::tr(c.title));
            entry.subtitle = i18n::tr(c.part).to_owned();
            entry.text.extend(c.paragraphs.iter().map(|key| Line::text(i18n::tr(key))));
            entry
        })
        .collect()
}
