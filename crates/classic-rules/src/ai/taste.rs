//! A leader's taste (classic/docs/ai-leaders.md): among options of nearly equal
//! value, the one its personality loves.
//!
//! The game's AI takes the single best option by value. Shifting a
//! leader's prices instead (step 5 of M6.2) cost points and still did not
//! show: a wonder twice as dear lost to buildings by value per turn. A
//! leader takes, of the options within `BAND_PERCENT` of the best value,
//! the one whose side of the game its personality values most; value
//! decides between options of equal affinity and everywhere outside the
//! band. So a personality never buys a clearly worse option, and a neutral
//! one (5 everywhere) takes what the game's AI takes.

use crate::ids::{BuildingKindId, ProductionItem, TechId};
use crate::rules::content::Personality;
use crate::rules::effects::{Modifier, YieldKind};
use crate::rules::CompiledRuleset;

/// Width of the band of near-equal options, percent of the best value.
pub const BAND_PERCENT: i64 = 10;

/// The sides of the game an option serves (a set).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Sides(u8);

impl Sides {
    pub const NONE: Sides = Sides(0);
    pub const EXPANSION: Sides = Sides(1);
    pub const WONDERS: Sides = Sides(1 << 1);
    pub const SCIENCE: Sides = Sides(1 << 2);
    pub const CULTURE: Sides = Sides(1 << 3);
    pub const GOLD: Sides = Sides(1 << 4);
    pub const FAITH: Sides = Sides(1 << 5);

    pub fn with(self, other: Sides) -> Sides {
        Sides(self.0 | other.0)
    }

    pub fn has(self, side: Sides) -> bool {
        self.0 & side.0 != 0
    }

    fn of_yield(kind: YieldKind) -> Sides {
        match kind {
            YieldKind::Science => Sides::SCIENCE,
            YieldKind::Culture => Sides::CULTURE,
            YieldKind::Gold => Sides::GOLD,
            YieldKind::Module(crate::classic::effects::ClassicYield::Faith) => Sides::FAITH,
            YieldKind::Output | YieldKind::Food | YieldKind::Production => Sides::NONE,
        }
    }

    /// A world wonder serves the wonders; another building, the yields its
    /// effects add.
    pub fn of_building(rules: &CompiledRuleset, kind: BuildingKindId) -> Sides {
        let Some(def) = rules.buildings.get(usize::from(kind.0)) else { return Sides::NONE };
        if def.wonder {
            return Sides::WONDERS;
        }
        let set = &def.effects;
        let effects = set.passive_player.iter().chain(&set.passive_city).chain(&set.passive_tile).chain(set.triggered.iter().flatten());
        effects
            .map(|effect| match effect.modifier {
                Modifier::AddYield { yield_kind, .. }
                | Modifier::AddYieldPercent { yield_kind, .. }
                | Modifier::AddYieldPerPopulation { yield_kind, .. }
                | Modifier::AddYieldPerCount { yield_kind, .. }
                | Modifier::AddYieldPercentPerCount { yield_kind, .. } => Sides::of_yield(yield_kind),
                _ => Sides::NONE,
            })
            .fold(Sides::NONE, Sides::with)
    }

    /// A settler serves expansion; a building, its sides.
    pub fn of_item(rules: &CompiledRuleset, item: ProductionItem) -> Sides {
        match item {
            ProductionItem::Unit(kind) if matches!(crate::ai::planner::role_of(rules, kind), crate::ai::planner::AiRole::Settler) => Sides::EXPANSION,
            ProductionItem::Building(kind) => Sides::of_building(rules, kind),
            ProductionItem::Unit(_) | ProductionItem::Project(_) => Sides::NONE,
        }
    }

    /// A technology serves the sides of the buildings it unlocks (a world
    /// wonder only while nobody has built it).
    pub fn of_tech(rules: &CompiledRuleset, tech: TechId, built_wonders: &[BuildingKindId]) -> Sides {
        let Some(def) = rules.technologies.get(usize::from(tech.0)) else { return Sides::NONE };
        def.unlocks_buildings
            .iter()
            .filter(|b| !built_wonders.contains(b))
            .map(|b| Sides::of_building(rules, *b))
            .fold(Sides::NONE, Sides::with)
    }
}

/// How much a personality loves an option: its highest value among the
/// option's sides; an option of no side is neutral.
pub fn affinity(personality: &Personality, sides: Sides) -> u8 {
    let values = [
        (Sides::EXPANSION, personality.expansion),
        (Sides::WONDERS, personality.wonders),
        (Sides::SCIENCE, personality.science),
        (Sides::CULTURE, personality.culture),
        (Sides::GOLD, personality.gold),
        (Sides::FAITH, personality.religion),
    ];
    values.iter().filter(|(side, _)| sides.has(*side)).map(|(_, v)| *v).max().unwrap_or(Personality::MIDDLE)
}

/// The option a leader of `personality` takes: the first of the best
/// value, or — with a leaning — of those within the band of the best, the
/// highest affinity, then the higher value, then the earlier. `sides` is
/// asked only of options inside the band.
pub fn pick<'o, T>(personality: &Personality, options: &'o [T], value: impl Fn(&T) -> i64, sides: impl Fn(&T) -> Sides) -> Option<&'o T> {
    let best = options.iter().reduce(|best, o| if value(o) > value(best) { o } else { best })?;
    if *personality == Personality::NEUTRAL {
        return Some(best);
    }
    let top = value(best);
    let floor = top - top.abs() * crate::ai::genome::get(crate::ai::genome::Knob::BandPercent) / 100;
    let key = |o: &T| (affinity(personality, sides(o)), value(o));
    options.iter().filter(|o| value(o) >= floor).reduce(|chosen, o| if key(o) > key(chosen) { o } else { chosen })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// (value, sides) options.
    fn options() -> Vec<(i64, Sides)> {
        vec![(1_000, Sides::GOLD), (950, Sides::WONDERS), (920, Sides::SCIENCE), (800, Sides::WONDERS)]
    }

    fn picked(personality: &Personality) -> (i64, Sides) {
        *pick(personality, &options(), |o| o.0, |o| o.1).expect("an option")
    }

    /// A neutral leader takes the best value; a leaning one its side inside
    /// the band and never outside it.
    #[test]
    fn the_band_ranks_near_equal_options_by_affinity() {
        assert_eq!(picked(&Personality::NEUTRAL), (1_000, Sides::GOLD));
        let builder = Personality { wonders: 9, ..Personality::NEUTRAL };
        assert_eq!(picked(&builder), (950, Sides::WONDERS), "the better of its wonders in the band");
        let scholar = Personality { science: 7, wonders: 3, ..Personality::NEUTRAL };
        assert_eq!(picked(&scholar), (920, Sides::SCIENCE));
        let miser = Personality { gold: 2, ..Personality::NEUTRAL };
        assert_eq!(picked(&miser), (950, Sides::WONDERS), "the disliked best gives way to a neutral option");
        assert!(pick(&builder, &[] as &[(i64, Sides)], |o| o.0, |o| o.1).is_none());
    }

    /// An option of several sides counts its most loved one.
    #[test]
    fn affinity_is_the_most_loved_side() {
        let p = Personality { science: 8, gold: 3, ..Personality::NEUTRAL };
        assert_eq!(affinity(&p, Sides::SCIENCE.with(Sides::GOLD)), 8);
        assert_eq!(affinity(&p, Sides::GOLD), 3);
        assert_eq!(affinity(&p, Sides::NONE), Personality::MIDDLE);
    }
}
