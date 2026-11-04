//! Pages of the map tables: terrain, features, resources, improvements and
//! natural wonders. The words around the numbers are messages of `classic-pedia-entries.ftl`
//! (keys `classic.pedia.entries-world.*`, and the shared labels `classic.pedia.entries.*`).
use classic_rules::rules::compiled::CompiledRuleset;
use classic_rules::rules::content::{FreshWater, ImprovementKind, ResourceClass};
use i18n::{t, tf};

use super::describe::{self, signed, yields};
use super::entries::{one, refs};
use super::{Art, Category, Entry, Line, PediaRef, Table};
use classic_rules::classic::content::{GREAT_PERSON_IMPROVEMENT, BARBARIAN_CAMP};

pub fn all(rules: &CompiledRuleset) -> Vec<Entry> {
    let mut out = Vec::new();
    out.extend(terrains(rules));
    out.extend(features(rules));
    out.extend(resources(rules));
    out.extend(improvements(rules));
    out.extend(natural_wonders(rules));
    out
}

fn movement_text(cost: u8, impassable: bool) -> String {
    if impassable {
        t!("classic.pedia.entries-world.impassable").into()
    } else {
        format!("{cost}")
    }
}

fn terrains(rules: &CompiledRuleset) -> Vec<Entry> {
    rules
        .terrains
        .iter()
        .enumerate()
        .map(|(i, def)| {
            let id = PediaRef::new(Table::Terrain, i as u16);
            let mut e = Entry::named(id, Category::Terrain, &def.key, &def.name);
            e.subtitle = if def.water { t!("classic.pedia.entries-world.water-terrain").into() } else { t!("classic.pedia.entries-world.land-terrain").into() };
            let y = yields(&def.yields);
            e.stat(t!("classic.pedia.entries.stat-yields"), if y.is_empty() { t!("classic.pedia.entries-world.none").into() } else { y });
            let hills = yields(&def.hills_yields);
            if !hills.is_empty() && hills != yields(&def.yields) {
                e.stat(t!("classic.pedia.entries-world.stat-as-hills"), hills);
            }
            e.stat(t!("classic.pedia.entries-world.stat-movement-cost"), movement_text(def.movement_cost, def.impassable));
            if def.defense_pct != 0 {
                e.stat(t!("classic.pedia.entries-world.stat-defense"), tf!("classic.pedia.entries.value-percent", n = signed(def.defense_pct)));
            }
            e.stat(
                t!("classic.pedia.entries-world.stat-cities"),
                if def.can_found_city { t!("classic.pedia.entries-world.cities-may-be-founded") } else { t!("classic.pedia.entries-world.none") },
            );
            let found = rules.resources.iter().enumerate().filter(|(_, r)| r.valid_terrains.iter().any(|t| t.0 as usize == i));
            e.link(t!("classic.pedia.entries-world.link-resources"), refs(Table::Resource, found.map(|(r, _)| r as u16)));
            e
        })
        .collect()
}

fn features(rules: &CompiledRuleset) -> Vec<Entry> {
    rules
        .features
        .iter()
        .enumerate()
        .map(|(i, def)| {
            let id = PediaRef::new(Table::Feature, i as u16);
            let mut e = Entry::named(id, Category::Features, &def.key, &def.name);
            e.subtitle = t!("classic.pedia.entries-world.terrain-feature").into();
            let y = yields(&def.yields);
            if !y.is_empty() {
                let shown = if def.yield_not_additive { tf!("classic.pedia.entries-world.yields-replace", yields = y) } else { y };
                e.stat(t!("classic.pedia.entries.stat-yields"), shown);
            }
            e.stat(t!("classic.pedia.entries-world.stat-movement-cost"), movement_text(def.movement_cost, def.impassable));
            if def.defense_pct != 0 {
                e.stat(t!("classic.pedia.entries-world.stat-defense"), tf!("classic.pedia.entries.value-percent", n = signed(def.defense_pct)));
            }
            if let Some(removal) = def.removal {
                e.stat(t!("classic.pedia.entries-world.stat-removed-in"), tf!("classic.pedia.entries-world.worker-turns", n = removal.work_time.div_ceil(100)));
                if removal.production > 0 {
                    e.stat(t!("classic.pedia.entries-world.stat-chop-production"), removal.production);
                }
            }
            if def.rough {
                e.text.push(Line::text(t!("classic.pedia.entries-world.rough-terrain")));
            }
            if def.fresh_water {
                e.text.push(Line::text(t!("classic.pedia.entries-world.provides-fresh-water")));
            }
            if def.no_improvement {
                e.text.push(Line::text(t!("classic.pedia.entries-world.cannot-be-improved")));
            }
            if def.requires_river {
                e.text.push(Line::text(t!("classic.pedia.entries-world.only-along-rivers")));
            }
            if def.requires_flatlands {
                e.text.push(Line::text(t!("classic.pedia.entries-world.only-on-flat-land")));
            }
            e.text.extend(def.inert.iter().map(|x| Line::inactive(describe::inert_effect(rules, x))));
            e.link(t!("classic.pedia.entries-world.link-found-on"), refs(Table::Terrain, def.valid_terrains.iter().map(|t| t.0)));
            e
        })
        .collect()
}

fn resource_class(class: Option<ResourceClass>) -> &'static str {
    match class {
        Some(ResourceClass::Bonus) => t!("classic.pedia.entries-world.resource-bonus"),
        Some(ResourceClass::Strategic) => t!("classic.pedia.entries-world.resource-strategic"),
        Some(ResourceClass::Luxury) => t!("classic.pedia.entries-world.resource-luxury"),
        Some(ResourceClass::Artifact) => t!("classic.pedia.entries-world.resource-artifact"),
        None => t!("classic.pedia.entries-world.resource"),
    }
}

fn resources(rules: &CompiledRuleset) -> Vec<Entry> {
    rules
        .resources
        .iter()
        .enumerate()
        .map(|(i, def)| {
            let id = PediaRef::new(Table::Resource, i as u16);
            let mut e = Entry::named(id, Category::Resources, &def.key, &def.name);
            e.art = super::art::resource(&def.key).map(Art::Card).unwrap_or(Art::None);
            e.subtitle = resource_class(def.class).into();
            let y = yields(&def.yields);
            if !y.is_empty() {
                e.stat(t!("classic.pedia.entries.stat-yields"), y);
            }
            if def.module.happiness != 0 {
                e.stat(t!("classic.pedia.entries-world.stat-happiness-inside"), signed(def.module.happiness));
            }
            if !def.quantities.is_empty() {
                let amounts: Vec<String> = def.quantities.iter().map(u8::to_string).collect();
                e.stat(t!("classic.pedia.entries-world.stat-quantity"), describe::join_or(amounts));
            }
            if let Some(bonus) = def.wonder_production {
                let text = match bonus.before_era {
                    Some(era) => {
                        let era = &rules.eras[era.0 as usize];
                        tf!(
                            "classic.pedia.entries-world.wonder-production-before",
                            percent = signed(bonus.percent),
                            era = super::content_name(Table::Era, &era.key, &era.name)
                        )
                    }
                    None => tf!("classic.pedia.entries-world.wonder-production", percent = signed(bonus.percent)),
                };
                e.stat(t!("classic.pedia.entries-world.stat-wonders"), text);
            }
            match (def.hills, def.flatlands) {
                (true, false) => e.text.push(Line::text(t!("classic.pedia.entries-world.appears-on-hills"))),
                (false, true) => e.text.push(Line::text(t!("classic.pedia.entries-world.appears-on-flat-land"))),
                _ => {}
            }
            if def.module.only_city_states {
                e.text.push(Line::text(t!("classic.pedia.entries-world.only-city-states")));
            }
            e.text.extend(def.inert.iter().map(|x| Line::inactive(describe::inert_effect(rules, x))));
            e.link(t!("classic.pedia.entries-world.link-revealed-by"), one(Table::Technology, def.required_tech.map(|t| t.0)));
            e.link(t!("classic.pedia.entries-world.link-tradeable-with"), one(Table::Technology, def.tradeable_with_tech.map(|t| t.0)));
            e.link(t!("classic.pedia.entries-world.link-improved-by"), refs(Table::Improvement, def.improvements.iter().map(|b| b.0)));
            e.link(t!("classic.pedia.entries-world.link-found-on"), refs(Table::Terrain, def.valid_terrains.iter().map(|t| t.0)));
            e.link(t!("classic.pedia.entries-world.link-found-on"), refs(Table::Feature, def.valid_features.iter().map(|f| f.0)));
            e.link(t!("classic.pedia.entries.link-unique-to"), one(Table::Nation, def.nation.map(u16::from)));
            e
        })
        .collect()
}

fn improvement_kind(kind: ImprovementKind) -> &'static str {
    match kind {
        ImprovementKind::Regular => t!("classic.pedia.entries-world.improvement-regular"),
        GREAT_PERSON_IMPROVEMENT => t!("classic.pedia.entries-world.improvement-great-person"),
        ImprovementKind::Unique => t!("classic.pedia.entries-world.improvement-unique"),
        BARBARIAN_CAMP => t!("classic.pedia.entries-world.improvement-barbarian-camp"),
        ImprovementKind::AncientRuins => t!("classic.pedia.entries-world.improvement-ancient-ruins"),
        ImprovementKind::Module(_) => t!("classic.pedia.entries-world.improvement-regular"),
    }
}

/// The improvement's ownership and lifecycle rules in words.
fn improvement_rules(def: &classic_rules::rules::compiled::ImprovementDefinition) -> Vec<&'static str> {
    let p = def.placement;
    let mut out = Vec::new();
    if p.ignore_ownership {
        out.push(t!("classic.pedia.entries-world.rule-anybodys-land"));
    }
    if p.only_host_land {
        out.push(t!("classic.pedia.entries-world.rule-city-state-land"));
    }
    if p.requires_improvement {
        out.push(t!("classic.pedia.entries-world.rule-replaces-improvement"));
    }
    if p.prompt_when_complete {
        out.push(t!("classic.pedia.entries-world.rule-builder-chooses"));
    }
    if p.destroyed_when_pillaged {
        out.push(t!("classic.pedia.entries-world.rule-destroyed-when-pillaged"));
    }
    if def.removes_resource {
        out.push(t!("classic.pedia.entries-world.rule-removes-resource"));
    }
    out
}

fn improvements(rules: &CompiledRuleset) -> Vec<Entry> {
    rules
        .improvements
        .iter()
        .enumerate()
        .map(|(i, def)| {
            let id = PediaRef::new(Table::Improvement, i as u16);
            let mut e = Entry::named(id, Category::Improvements, &def.key, &def.name);
            e.art = super::art::improvement(&def.key).map(Art::Card).unwrap_or(Art::None);
            e.subtitle = improvement_kind(def.kind).into();
            let y = yields(&def.yields);
            if !y.is_empty() {
                e.stat(t!("classic.pedia.entries.stat-yields"), y);
            }
            if def.work_turns > 0 {
                e.stat(t!("classic.pedia.entries-world.stat-build-time"), tf!("classic.pedia.entries-world.turns", n = def.work_turns));
            }
            if def.defense_pct != 0 {
                e.stat(t!("classic.pedia.entries-world.stat-defense"), tf!("classic.pedia.entries.value-percent", n = signed(def.defense_pct)));
            }
            if def.pillage_gold > 0 {
                e.stat(t!("classic.pedia.entries-world.stat-pillage-gold"), def.pillage_gold);
            }
            if def.culture_per_era > 0 {
                e.stat(t!("classic.pedia.entries-world.stat-culture-per-era"), def.culture_per_era);
            }
            e.text.extend(improvement_rules(def).into_iter().map(|t| Line::effect(t.to_string())));
            if def.adjacent_same_culture > 0 {
                let name = super::content_name(Table::Improvement, &def.key, &def.name);
                e.text.push(Line::effect(tf!("classic.pedia.entries-world.culture-per-adjacent", n = def.adjacent_same_culture, name = name)));
            }
            if def.module.city_state_luxury_copies > 0 {
                e.text.push(Line::effect(tf!(
                    "classic.pedia.entries-world.city-state-luxury-copies",
                    n = def.module.city_state_luxury_copies
                )));
            }
            for bonus in &def.fresh_water_yields {
                let amount = signed(bonus.amount);
                let yield_name = describe::yield_name(bonus.yield_kind);
                let mut text = tf!("classic.pedia.describe.yield-amount", amount = amount, yield = yield_name);
                if let Some(t) = bonus.tech {
                    let tech = describe::name_of(rules, PediaRef::new(Table::Technology, t.0));
                    text = tf!("classic.pedia.entries-world.bonus-with-technology", text = text, name = tech);
                }
                match bonus.fresh_water {
                    FreshWater::Any => {}
                    FreshWater::Required => text = tf!("classic.pedia.entries-world.bonus-with-fresh-water", text = text),
                    FreshWater::Absent => text = tf!("classic.pedia.entries-world.bonus-without-fresh-water", text = text),
                }
                e.text.push(Line::effect(text));
            }
            e.text.extend(def.inert.iter().map(|x| Line::inactive(describe::inert_effect(rules, x))));
            let techs = def.fresh_water_yields.iter().filter_map(|b| b.tech.map(|t| t.0));
            e.link(t!("classic.pedia.entries.link-required-technology"), one(Table::Technology, def.required_tech.map(|t| t.0)));
            e.link(t!("classic.pedia.entries-world.link-improves"), refs(Table::Resource, def.resources.iter().map(|r| r.0)));
            e.link(t!("classic.pedia.entries-world.link-built-on"), refs(Table::Terrain, def.valid_terrains.iter().map(|t| t.0)));
            e.link(t!("classic.pedia.entries-world.link-built-on"), refs(Table::Feature, def.valid_features.iter().map(|f| f.0)));
            e.link(t!("classic.pedia.entries.link-unique-to"), one(Table::Nation, def.nation.map(u16::from)));
            e.link(t!("classic.pedia.entries-world.link-better-with"), refs(Table::Technology, techs));
            e
        })
        .collect()
}

fn natural_wonders(rules: &CompiledRuleset) -> Vec<Entry> {
    rules
        .natural_wonders
        .iter()
        .enumerate()
        .map(|(i, def)| {
            let id = PediaRef::new(Table::NaturalWonder, i as u16);
            let mut e = Entry::named(id, Category::NaturalWonders, &def.key, &def.name);
            e.subtitle = t!("classic.pedia.entries-world.natural-wonder").into();
            let y = yields(&def.yields);
            if !y.is_empty() {
                e.stat(t!("classic.pedia.entries.stat-yields"), y);
            }
            if def.module.happiness != 0 {
                e.stat(t!("classic.pedia.entries-world.stat-happiness-inside"), signed(def.module.happiness));
            }
            if def.first_finder_gold > 0 {
                e.stat(t!("classic.pedia.entries-world.stat-first-finder-gold"), def.first_finder_gold);
            }
            e.stat(t!("classic.pedia.entries-world.stat-movement-cost"), movement_text(def.movement_cost, def.impassable));
            if def.defense_pct != 0 {
                e.stat(t!("classic.pedia.entries-world.stat-defense"), tf!("classic.pedia.entries.value-percent", n = signed(def.defense_pct)));
            }
            if def.fresh_water {
                e.text.push(Line::text(t!("classic.pedia.entries-world.provides-fresh-water")));
            }
            if def.rough {
                e.text.push(Line::effect(t!("classic.pedia.entries-world.rough-ground-in-combat")));
            }
            if def.no_improvement {
                e.text.push(Line::effect(t!("classic.pedia.entries-world.cannot-be-improved")));
            }
            e.text.push(Line::effect(t!("classic.pedia.entries-world.happiness-for-finders")));
            e.text.extend(describe::placement_lines(rules, &def.placement));
            e.text.extend(def.inert.iter().map(|x| Line::inactive(describe::inert_effect(rules, x))));
            e.link(t!("classic.pedia.entries-world.link-found-on"), refs(Table::Terrain, def.valid_terrains.iter().map(|t| t.0)));
            e.link(t!("classic.pedia.entries-world.link-nearby-units-gain"), one(Table::Promotion, def.adjacent_unit_free_promotion.map(|p| p.0)));
            e
        })
        .collect()
}
