//! Pages of the nation, technology, unit, building, policy, faith and
//! people tables. Every number and sentence comes from the compiled rules; the words around
//! them are messages of `classic-pedia-entries.ftl` (keys `classic.pedia.entries.*`).
use classic_rules::rules::compiled::{BuildingDefinition, NationDefinition, CompiledRuleset, TechnologyDefinition, UnitDefinition};
use classic_rules::classic::tables::PolicyDefinition;
use classic_rules::rules::content::{
    BeliefCategory, BuildingKind, Domain, EraId, PolicyBranchKind, PolicyRole, UnitRole,
};
use i18n::{t, tf};

use super::describe::{self, effect_lines, effect_refs, great_person, yields};
use super::{art, Art, Category, Entry, Line, PediaRef, Table};
use classic_rules::classic::content::{GREAT_PERSON_ROLE, RELIGIOUS_ROLE, RELIGIOUS_BUILDING, GUILD};

pub fn all(rules: &CompiledRuleset) -> Vec<Entry> {
    let mut out = Vec::new();
    out.extend(rules.nations.iter().enumerate().map(|(i, d)| nation(rules, i, d)));
    out.extend(city_states(rules));
    out.extend(rules.technologies.iter().enumerate().map(|(i, d)| technology(rules, i, d)));
    out.extend(rules.units.iter().enumerate().map(|(i, d)| unit(rules, i, d)));
    out.extend(promotions(rules));
    out.extend(rules.buildings.iter().enumerate().map(|(i, d)| building(rules, i, d)));
    out.extend(projects(rules));
    out.extend(rules.module.policies.iter().enumerate().map(|(i, d)| policy(rules, i, d)));
    out.extend(policy_branches(rules));
    out.extend(beliefs(rules));
    out.extend(religions(rules));
    out.extend(great_people(rules));
    out.extend(specialists(rules));
    out.extend(eras(rules));
    out.extend(handicaps(rules));
    out
}

pub(super) fn refs(table: Table, ids: impl IntoIterator<Item = u16>) -> Vec<PediaRef> {
    ids.into_iter().map(|i| PediaRef::new(table, i)).collect()
}

pub(super) fn one(table: Table, id: Option<u16>) -> Vec<PediaRef> {
    refs(table, id)
}

/// The name of an era in the active language.
fn era_name(rules: &CompiledRuleset, era: Option<EraId>) -> Option<String> {
    let era = rules.eras.get(era?.0 as usize)?;
    Some(super::content_name(Table::Era, &era.key, &era.name))
}

/// "Ancient Era" for a page's subtitle.
fn era_label(rules: &CompiledRuleset, era: Option<EraId>) -> Option<String> {
    let def = rules.eras.get(era?.0 as usize)?;
    Some(era_text(&def.name, super::content_name(Table::Era, &def.key, &def.name)))
}

/// An era's words as the subtitle shows them: the catalog's era name may lack the word "era"
/// ("Ancient"); that is decided on the catalog's name, so a translated name is never touched.
fn era_text(catalog_name: &str, shown: String) -> String {
    if catalog_name.to_lowercase().ends_with("era") {
        shown
    } else {
        tf!("classic.pedia.entries.era-label", name = shown)
    }
}

fn nation_name(rules: &CompiledRuleset, nation: u8) -> String {
    rules.nations.get(nation as usize).map(|c| super::content_name(Table::Nation, &c.key, &c.name)).unwrap_or_default()
}

/// The parts of a subtitle that exist, joined the way the language wants (`subtitle-join`).
fn join_subtitle(parts: &[Option<String>]) -> String {
    let mut parts = parts.iter().flatten().filter(|p| !p.is_empty()).cloned();
    let Some(mut text) = parts.next() else { return String::new() };
    for next in parts {
        text = tf!("classic.pedia.entries.subtitle-join", a = text, b = next);
    }
    text
}

fn nation(rules: &CompiledRuleset, i: usize, def: &NationDefinition) -> Entry {
    let id = PediaRef::new(Table::Nation, i as u16);
    let mut e = Entry::named(id, Category::Nations, &def.key, &def.name);
    e.subtitle = tf!("classic.pedia.entries.led-by", leader = def.leader.as_str());
    e.art = Art::Nation(def.key.rsplit('/').next().unwrap_or("").to_owned());
    e.stat(t!("classic.pedia.entries.stat-leader"), &def.leader);
    e.stat(t!("classic.pedia.entries.stat-adjective"), &def.adjective);
    if !def.ability_name.is_empty() {
        e.text.push(Line::text(tf!("classic.pedia.entries.unique-ability", name = def.ability_name.as_str())));
    }
    e.text.extend(effect_lines(rules, &def.effects, &def.inert));
    for &(unit, count) in &def.free_units {
        let name = describe::name_of(rules, PediaRef::new(Table::Unit, unit.0));
        e.text.push(Line::text(tf!("classic.pedia.entries.starts-with", count = count, name = name)));
    }
    e.link(t!("classic.pedia.entries.link-unique-units"), refs(Table::Unit, def.unique_units.iter().map(|u| u.0)));
    e.link(t!("classic.pedia.entries.link-unique-buildings"), refs(Table::Building, def.unique_buildings.iter().map(|b| b.0)));
    e.link(t!("classic.pedia.entries.link-unique-improvements"), refs(Table::Improvement, def.unique_improvements.iter().map(|b| b.0)));
    e.link(t!("classic.pedia.entries.link-free-technologies"), refs(Table::Technology, def.free_techs.iter().map(|t| t.0)));
    e.link(t!("classic.pedia.entries.link-starting-units"), refs(Table::Unit, def.free_units.iter().map(|u| u.0 .0)));
    e.link(t!("classic.pedia.entries.link-preferred-religions"), refs(Table::Religion, def.module.preferred_religions.iter().map(|r| r.0)));
    e.link(t!("classic.pedia.entries.link-related"), effect_refs(rules, &def.effects, &def.inert));
    e
}

fn city_states(rules: &CompiledRuleset) -> Vec<Entry> {
    let mut out = Vec::new();
    for (i, def) in rules.module.city_states.iter().enumerate() {
        let id = PediaRef::new(Table::CityState, i as u16);
        let mut e = Entry::named(id, Category::CityStates, &def.key, &def.name);
        let kind = rules.module.city_state_types.get(def.city_state_type.0 as usize);
        e.subtitle = kind
            .map(|k| tf!("classic.pedia.entries.city-state-of-type", kind = super::content_name(Table::CityStateType, &k.key, &k.name)))
            .unwrap_or_default();
        e.link(t!("classic.pedia.entries.link-type"), one(Table::CityStateType, Some(def.city_state_type.0)));
        out.push(e);
    }
    for (i, def) in rules.module.city_state_types.iter().enumerate() {
        let id = PediaRef::new(Table::CityStateType, i as u16);
        let mut e = Entry::named(id, Category::CityStates, &def.key, &def.name);
        e.subtitle = t!("classic.pedia.entries.city-state-type").into();
        e.stat(t!("classic.pedia.entries.stat-friends-at-influence"), def.friends_threshold);
        e.stat(t!("classic.pedia.entries.stat-allies-at-influence"), def.allies_threshold);
        e.text.extend(def.inert.iter().map(|x| Line::inactive(describe::inert_effect(rules, x))));
        out.push(e);
    }
    out
}

fn technology(rules: &CompiledRuleset, i: usize, def: &TechnologyDefinition) -> Entry {
    let id = PediaRef::new(Table::Technology, i as u16);
    let mut e = Entry::named(id, Category::Technologies, &def.key, &def.name);
    let era = era_label(rules, def.content_era).or_else(|| def.era_name.as_ref().map(|name| era_text(name, name.clone())));
    e.subtitle = join_subtitle(&[era, Some(t!("classic.pedia.entries.technology").into())]);
    e.art = art::technology(&def.key).map(Art::Card).unwrap_or(Art::None);
    e.stat(t!("classic.pedia.entries.stat-cost"), tf!("classic.pedia.entries.value-science", n = def.cost));
    if def.repeatable {
        e.text.push(Line::text(t!("classic.pedia.entries.repeatable")));
    }
    for action in &def.unlocks_actions {
        e.text.push(Line::text(tf!("classic.pedia.entries.allows-action", action = describe::pretty(action))));
    }
    if def.module.triggers_archaeological_sites {
        e.text.push(Line::effect(t!("classic.pedia.entries.tech-antiquity-sites")));
    }
    if def.map_visible {
        e.text.push(Line::effect(t!("classic.pedia.entries.tech-reveals-map")));
    }
    if def.bridge_building {
        e.text.push(Line::effect(t!("classic.pedia.entries.tech-bridges")));
    }
    if def.road_move_change != 0 {
        let key = if def.road_move_change < 0 {
            t!("classic.pedia.entries.tech-roads-faster")
        } else {
            t!("classic.pedia.entries.tech-roads-slower")
        };
        e.text.push(Line::effect(key));
    }
    e.text.extend(effect_lines(rules, &def.effects, &def.inert));
    e.link(t!("classic.pedia.entries.link-requires"), refs(Table::Technology, def.requires.iter().map(|t| t.0)));
    e.link(t!("classic.pedia.entries.link-unlocks"), refs(Table::Unit, def.unlocks_units.iter().map(|u| u.0)));
    e.link(t!("classic.pedia.entries.link-unlocks"), refs(Table::Building, def.unlocks_buildings.iter().map(|b| b.0)));
    e.link(t!("classic.pedia.entries.link-unlocks"), refs(Table::Improvement, def.unlocks_improvements.iter().map(|b| b.0)));
    e.link(t!("classic.pedia.entries.link-unlocks"), refs(Table::Project, def.unlocks_projects.iter().map(|b| b.0)));
    e.link(t!("classic.pedia.entries.link-era"), one(Table::Era, def.content_era.map(|e| e.0)));
    e.link(t!("classic.pedia.entries.link-related"), effect_refs(rules, &def.effects, &def.inert));
    e
}

fn role_name(role: UnitRole) -> &'static str {
    match role {
        UnitRole::Civilian => t!("classic.pedia.entries.role-civilian"),
        GREAT_PERSON_ROLE => t!("classic.pedia.entries.role-great-person"),
        RELIGIOUS_ROLE => t!("classic.pedia.entries.role-religious"),
        UnitRole::SpaceshipPart => t!("classic.pedia.entries.role-spaceship-part"),
        UnitRole::Recon => t!("classic.pedia.entries.role-recon"),
        UnitRole::Melee => t!("classic.pedia.entries.role-melee"),
        UnitRole::Archer => t!("classic.pedia.entries.role-archer"),
        UnitRole::Mounted => t!("classic.pedia.entries.role-mounted"),
        UnitRole::Siege => t!("classic.pedia.entries.role-siege"),
        UnitRole::Gun => t!("classic.pedia.entries.role-gun"),
        UnitRole::Armor => t!("classic.pedia.entries.role-armor"),
        UnitRole::NavalMelee => t!("classic.pedia.entries.role-naval-melee"),
        UnitRole::NavalRanged => t!("classic.pedia.entries.role-naval-ranged"),
        UnitRole::Submarine => t!("classic.pedia.entries.role-submarine"),
        UnitRole::Carrier => t!("classic.pedia.entries.role-carrier"),
        UnitRole::Fighter => t!("classic.pedia.entries.role-fighter"),
        UnitRole::Bomber => t!("classic.pedia.entries.role-bomber"),
        UnitRole::Helicopter => t!("classic.pedia.entries.role-helicopter"),
        UnitRole::Missile => t!("classic.pedia.entries.role-missile"),
        UnitRole::Module(_) => t!("classic.pedia.entries.role-civilian"),
    }
}

fn domain_name(domain: Domain) -> &'static str {
    match domain {
        Domain::Land => t!("classic.pedia.entries.domain-land"),
        Domain::Sea => t!("classic.pedia.entries.domain-sea"),
        Domain::Air => t!("classic.pedia.entries.domain-air"),
    }
}

fn unit(rules: &CompiledRuleset, i: usize, def: &UnitDefinition) -> Entry {
    let id = PediaRef::new(Table::Unit, i as u16);
    let category = if def.nation.is_some() { Category::UniqueUnits } else { Category::Units };
    let mut e = Entry::named(id, category, &def.key, &def.name);
    e.subtitle = join_subtitle(&[
        era_label(rules, def.era),
        def.role.map(|r| tf!("classic.pedia.entries.unit-of-role", role = role_name(r))),
        def.nation.map(|c| tf!("classic.pedia.entries.unique-to", nation = nation_name(rules, c))),
    ]);
    e.art = art::unit(&def.key).map(Art::Card).unwrap_or(Art::None);
    unit_stats(&mut e, def);
    for (resource, amount) in &def.required_resources {
        let name = describe::name_of(rules, PediaRef::new(Table::Resource, resource.0));
        e.stat(t!("classic.pedia.entries.stat-needs"), tf!("classic.pedia.entries.value-count-name", count = amount, name = name));
    }
    e.text.extend(effect_lines(rules, &def.effects, &def.inert));
    for ability in &def.module.abilities {
        e.text.push(Line::effect(describe::unit_ability(*ability)));
    }
    e.link(t!("classic.pedia.entries.link-required-technology"), one(Table::Technology, def.required_tech.map(|t| t.0)));
    e.link(t!("classic.pedia.entries.link-obsolete-with"), one(Table::Technology, def.obsolete_tech.map(|t| t.0)));
    e.link(t!("classic.pedia.entries.link-upgrades-to"), refs(Table::Unit, def.upgrades_to.iter().map(|u| u.0)));
    e.link(t!("classic.pedia.entries.link-replaces"), one(Table::Unit, def.replaces.map(|u| u.0)));
    e.link(t!("classic.pedia.entries.link-unique-to"), one(Table::Nation, def.nation.map(u16::from)));
    e.link(t!("classic.pedia.entries.link-requires-resources"), refs(Table::Resource, def.required_resources.iter().map(|r| r.0 .0)));
    e.link(t!("classic.pedia.entries.link-requires-buildings"), refs(Table::Building, def.requires_buildings.iter().map(|b| b.0)));
    e.link(t!("classic.pedia.entries.link-free-promotions"), refs(Table::Promotion, def.free_promotions.iter().map(|p| p.0)));
    e.link(t!("classic.pedia.entries.link-era"), one(Table::Era, def.era.map(|e| e.0)));
    e.link(t!("classic.pedia.entries.link-related"), effect_refs(rules, &def.effects, &def.inert));
    e
}

fn unit_stats(e: &mut Entry, def: &UnitDefinition) {
    if def.strength > 0 {
        e.stat(t!("classic.pedia.entries.stat-combat-strength"), def.strength);
    }
    if def.ranged_strength > 0 {
        e.stat(t!("classic.pedia.entries.stat-ranged-strength"), def.ranged_strength);
        e.stat(t!("classic.pedia.entries.stat-range"), def.range);
    }
    e.stat(t!("classic.pedia.entries.stat-movement"), def.movement);
    if def.sight > 0 {
        e.stat(t!("classic.pedia.entries.stat-sight"), def.sight);
    }
    if !def.unbuildable {
        e.stat(t!("classic.pedia.entries.stat-cost"), tf!("classic.pedia.entries.value-production", n = def.cost));
    }
    if let Some(faith) = def.module.faith_cost {
        e.stat(t!("classic.pedia.entries.stat-faith-cost"), faith);
    }
    e.stat(t!("classic.pedia.entries.stat-domain"), domain_name(def.domain));
}

fn building_kind(kind: BuildingKind) -> &'static str {
    match kind {
        BuildingKind::Regular => t!("classic.pedia.entries.building-regular"),
        RELIGIOUS_BUILDING => t!("classic.pedia.entries.building-religious"),
        GUILD => t!("classic.pedia.entries.building-guild"),
        BuildingKind::Palace => t!("classic.pedia.entries.building-palace"),
        BuildingKind::Unique => t!("classic.pedia.entries.building-unique"),
        BuildingKind::WorldWonder => t!("classic.pedia.entries.building-world-wonder"),
        BuildingKind::NationalWonder => t!("classic.pedia.entries.building-national-wonder"),
        BuildingKind::Module(_) => t!("classic.pedia.entries.building-regular"),
    }
}

fn building(rules: &CompiledRuleset, i: usize, def: &BuildingDefinition) -> Entry {
    let id = PediaRef::new(Table::Building, i as u16);
    let wonder = matches!(def.kind, BuildingKind::WorldWonder | BuildingKind::NationalWonder) || def.wonder;
    let category = if wonder { Category::Wonders } else { Category::Buildings };
    let mut e = Entry::named(id, category, &def.key, &def.name);
    e.subtitle = join_subtitle(&[
        era_label(rules, def.era),
        Some(building_kind(def.kind).into()),
        def.nation.map(|c| tf!("classic.pedia.entries.unique-to", nation = nation_name(rules, c))),
    ]);
    e.art = art::building(&def.key, def.kind).map(Art::Card).unwrap_or(Art::None);
    building_stats(rules, &mut e, def);
    e.text.extend(effect_lines(rules, &def.effects, &def.inert));
    for ability in &def.module.abilities {
        e.text.push(Line::effect(describe::building_ability(*ability)));
    }
    if def.module.requires_holy_city {
        e.text.push(Line::text(t!("classic.pedia.entries.only-in-holy-city")));
    }
    for project in rules.module.projects.iter().filter(|p| rewards_building(p, i)) {
        let name = super::content_name(Table::Project, &project.key, &project.name);
        e.text.push(Line::text(tf!("classic.pedia.entries.only-through-project", name = name)));
    }
    if def.module.faith_purchase_needs_belief {
        e.text.push(Line::text(t!("classic.pedia.entries.faith-needs-belief")));
    }
    e.link(t!("classic.pedia.entries.link-required-technology"), one(Table::Technology, def.required_tech.map(|t| t.0)));
    e.link(t!("classic.pedia.entries.link-obsolete-with"), one(Table::Technology, def.obsolete_tech.map(|t| t.0)));
    e.link(t!("classic.pedia.entries.link-replaces"), one(Table::Building, def.replaces.map(|b| b.0)));
    e.link(t!("classic.pedia.entries.link-unique-to"), one(Table::Nation, def.nation.map(u16::from)));
    e.link(t!("classic.pedia.entries.link-requires-buildings"), refs(Table::Building, def.requires_buildings.iter().map(|b| b.0)));
    e.link(t!("classic.pedia.entries.link-requires-buildings"), refs(Table::Building, def.requires_buildings_in_empire.iter().map(|b| b.0)));
    e.link(t!("classic.pedia.entries.link-requires-resources"), refs(Table::Resource, def.consumes_resources.iter().map(|r| r.0 .0)));
    e.link(t!("classic.pedia.entries.link-requires-policy-branch"), one(Table::PolicyBranch, def.module.requires_policy_branch.map(|b| b.0)));
    e.link(t!("classic.pedia.entries.link-specialists"), one(Table::Specialist, def.specialist_slots.map(|s| s.0 .0)));
    e.link(t!("classic.pedia.entries.link-era"), one(Table::Era, def.era.map(|e| e.0)));
    e.link(t!("classic.pedia.entries.link-related"), effect_refs(rules, &def.effects, &def.inert));
    e
}

/// A World Congress project's rewards include building `index`.
fn rewards_building(project: &classic_rules::classic::tables::ProjectDefinition, index: usize) -> bool {
    let Some(league) = &project.league else {
        return false;
    };
    let wanted = classic_rules::classic::tables::LeagueReward::Building(classic_rules::ids::BuildingKindId(index as u16));
    league.tiers.iter().flatten().any(|r| *r == wanted)
}

fn building_stats(rules: &CompiledRuleset, e: &mut Entry, def: &BuildingDefinition) {
    if !def.unbuildable {
        e.stat(t!("classic.pedia.entries.stat-cost"), tf!("classic.pedia.entries.value-production", n = def.cost));
    }
    if let Some(faith) = def.module.faith_cost {
        e.stat(t!("classic.pedia.entries.stat-faith-cost"), faith);
    }
    if def.maintenance > 0 {
        e.stat(t!("classic.pedia.entries.stat-maintenance"), tf!("classic.pedia.entries.value-gold", n = def.maintenance));
    }
    if let Some((specialist, count)) = def.specialist_slots {
        let name = describe::name_of(rules, PediaRef::new(Table::Specialist, specialist.0));
        e.stat(t!("classic.pedia.entries.stat-specialist-slots"), tf!("classic.pedia.entries.value-count-name", count = count, name = name));
    }
    if let Some((slot, count)) = def.module.great_work_slots {
        e.stat(
            t!("classic.pedia.entries.stat-great-work-slots"),
            tf!("classic.pedia.entries.value-count-name", count = count, name = describe::pretty(&format!("{slot:?}"))), // i18n-ok: the slot kind's own identifier
        );
    }
    if def.module.theming.iter().any(|t| t.bonus > 0) {
        let best = def.module.theming.iter().map(|t| t.bonus).max().unwrap_or(0);
        e.stat(t!("classic.pedia.entries.stat-theming-bonus"), tf!("classic.pedia.entries.value-up-to", n = best));
    }
}

/// Who earns each reward tier of a World Congress project (rules.md).
const LEAGUE_TIERS: [&str; 3] = [
    i18n::key!("classic.pedia.entries.league-tier-half"),
    i18n::key!("classic.pedia.entries.league-tier-full"),
    i18n::key!("classic.pedia.entries.league-tier-largest"),
];

fn projects(rules: &CompiledRuleset) -> Vec<Entry> {
    rules
        .module.projects
        .iter()
        .enumerate()
        .map(|(i, def)| {
            let id = PediaRef::new(Table::Project, i as u16);
            let mut e = Entry::named(id, Category::Projects, &def.key, &def.name);
            e.subtitle = match def.kind {
                classic_rules::rules::content::ProjectKind::Project => t!("classic.pedia.entries.project").into(),
                classic_rules::rules::content::ProjectKind::LeagueProject => t!("classic.pedia.entries.league-project").into(),
            };
            e.art = art::project(&def.key).map(Art::Card).unwrap_or(Art::None);
            if !def.unbuildable {
                e.stat(t!("classic.pedia.entries.stat-cost"), tf!("classic.pedia.entries.value-production", n = def.cost));
            }
            if let Some(victory) = &def.victory {
                e.stat(t!("classic.pedia.entries.stat-victory"), describe::capitalize(victory)); // i18n-ok: the victory's own identifier
            }
            if def.max_global_instances > 0 {
                e.stat(t!("classic.pedia.entries.stat-built-once"), def.max_global_instances);
            }
            if def.allows_nukes {
                e.text.push(Line::effect(t!("classic.pedia.entries.project-allows-nukes")));
            }
            if let Some(league) = &def.league {
                e.stat(t!("classic.pedia.entries.stat-cost-per-nation"), tf!("classic.pedia.entries.value-production", n = league.cost_per_player));
                for (tier, rewards) in league.tiers.iter().enumerate() {
                    let phrases: Vec<String> = rewards.iter().map(|r| describe::league_reward(rules, r)).collect();
                    e.text.push(Line::effect(tf!(
                        "classic.pedia.entries.league-tier",
                        tier = i18n::tr(LEAGUE_TIERS[tier]),
                        rewards = describe::join_list(phrases)
                    )));
                }
                e.link(t!("classic.pedia.entries.link-proposed-once"), one(Table::Technology, league.tech_any_member.map(|t| t.0)));
            }
            e.text.extend(def.inert.iter().map(|x| Line::inactive(describe::inert_effect(rules, x))));
            e.link(t!("classic.pedia.entries.link-required-technology"), one(Table::Technology, def.required_tech.map(|t| t.0)));
            e.link(t!("classic.pedia.entries.link-requires-projects"), refs(Table::Project, def.prerequisite_projects.iter().map(|p| p.0 .0)));
            e
        })
        .collect()
}

fn promotions(rules: &CompiledRuleset) -> Vec<Entry> {
    rules
        .promotions
        .iter()
        .enumerate()
        .map(|(i, def)| {
            let id = PediaRef::new(Table::Promotion, i as u16);
            let mut e = Entry::named(id, Category::Promotions, &def.key, &def.name);
            let roles: Vec<String> = def.unit_roles.iter().map(|r| role_name(*r).to_owned()).collect();
            e.subtitle = if roles.is_empty() {
                t!("classic.pedia.entries.promotion").into()
            } else {
                tf!("classic.pedia.entries.promotion-for-roles", roles = describe::join_list(roles))
            };
            if !def.choosable {
                e.text.push(Line::text(t!("classic.pedia.entries.promotion-granted")));
            }
            if def.lost_with_upgrade {
                e.text.push(Line::text(t!("classic.pedia.entries.promotion-lost-on-upgrade")));
            }
            e.text.extend(effect_lines(rules, &def.effects, &def.inert));
            e.link(t!("classic.pedia.entries.link-requires"), refs(Table::Promotion, def.requires.iter().map(|p| p.0)));
            e.link(t!("classic.pedia.entries.link-requires-one-of"), refs(Table::Promotion, def.requires_any_of.iter().map(|p| p.0)));
            e.link(t!("classic.pedia.entries.link-required-technology"), one(Table::Technology, def.required_tech.map(|t| t.0)));
            e.link(t!("classic.pedia.entries.link-for-units"), refs(Table::Unit, def.civilian_units.iter().map(|u| u.0)));
            e.link(t!("classic.pedia.entries.link-related"), effect_refs(rules, &def.effects, &def.inert));
            e
        })
        .collect()
}

/// The catalog text repeats the name as a title ("Legalism Provides ..."): drop it. The
/// title may be the catalog's or the translated name.
fn help_text(names: &[&str], text: &str) -> String {
    for name in names {
        match text.strip_prefix(name) {
            Some(rest) if !name.is_empty() && rest.trim_start().chars().next().is_some_and(char::is_uppercase) => {
                return rest.trim_start().to_owned();
            }
            _ => {}
        }
    }
    text.to_owned()
}

fn policy(rules: &CompiledRuleset, i: usize, def: &PolicyDefinition) -> Entry {
    let id = PediaRef::new(Table::Policy, i as u16);
    let mut e = Entry::named(id, Category::Policies, &def.key, &def.name);
    let branch = def.branch.and_then(|b| rules.module.policy_branches.get(b.0 as usize));
    let role = match def.role {
        PolicyRole::Policy => t!("classic.pedia.entries.policy").to_owned(),
        PolicyRole::Tenet(tier) => tf!("classic.pedia.entries.policy-tenet", level = tier),
        PolicyRole::Opener => t!("classic.pedia.entries.policy-opener").to_owned(),
        PolicyRole::Finisher => t!("classic.pedia.entries.policy-finisher").to_owned(),
    };
    e.subtitle = join_subtitle(&[branch.map(|b| super::content_name(Table::PolicyBranch, &b.key, &b.name)), Some(role)]);
    let slug = def.key.strip_prefix("core:policy/").unwrap_or(&def.key);
    let art_id = match (def.role, branch) {
        (PolicyRole::Opener | PolicyRole::Finisher, Some(b)) => b.key.strip_prefix("core:policy/").unwrap_or(&b.key),
        _ => slug,
    };
    e.art = Art::Policy(art_id.to_owned());
    if !def.text.is_empty() {
        let text = i18n::content("classic", "policy", &def.key, "desc", &def.text);
        e.text.push(Line::text(help_text(&[&e.name, &def.name], &text)));
    }
    e.text.extend(effect_lines(rules, &def.effects, &def.inert));
    e.link(t!("classic.pedia.entries.link-branch"), one(Table::PolicyBranch, def.branch.map(|b| b.0)));
    e.link(t!("classic.pedia.entries.link-requires"), refs(Table::Policy, def.prerequisites.iter().map(|p| p.0)));
    e.link(t!("classic.pedia.entries.link-related"), effect_refs(rules, &def.effects, &def.inert));
    e
}

fn policy_branches(rules: &CompiledRuleset) -> Vec<Entry> {
    rules
        .module.policy_branches
        .iter()
        .enumerate()
        .map(|(i, def)| {
            let id = PediaRef::new(Table::PolicyBranch, i as u16);
            let mut e = Entry::named(id, Category::Policies, &def.key, &def.name);
            let kind = match def.kind {
                PolicyBranchKind::Normal => t!("classic.pedia.entries.branch-normal"),
                PolicyBranchKind::Ideology => t!("classic.pedia.entries.branch-ideology"),
            };
            e.subtitle = join_subtitle(&[
                Some(kind.into()),
                era_name(rules, def.era_prerequisite).map(|era| tf!("classic.pedia.entries.branch-from-era", era = era)),
            ]);
            e.art = Art::Policy(def.key.strip_prefix("core:policy/").unwrap_or(&def.key).to_owned());
            if def.locked_without_religion {
                e.text.push(Line::text(t!("classic.pedia.entries.branch-needs-religion")));
            }
            if def.first_adopter_free_policies > 0 {
                e.text.push(Line::text(tf!(
                    "classic.pedia.entries.branch-first-adopter",
                    n = def.first_adopter_free_policies
                )));
            }
            let members = (0..rules.module.policies.len() as u16)
                .filter(|p| rules.module.policies[*p as usize].branch.map(|b| b.0) == Some(i as u16));
            e.link(t!("classic.pedia.entries.link-opener"), one(Table::Policy, def.opener.map(|p| p.0)));
            e.link(t!("classic.pedia.entries.link-policies"), refs(Table::Policy, members));
            e.link(t!("classic.pedia.entries.link-finisher"), one(Table::Policy, def.finisher.map(|p| p.0)));
            e.link(t!("classic.pedia.entries.link-excludes"), refs(Table::PolicyBranch, def.mutually_exclusive_with.iter().map(|b| b.0)));
            e.link(t!("classic.pedia.entries.link-era"), one(Table::Era, def.era_prerequisite.map(|e| e.0)));
            e
        })
        .collect()
}

fn belief_category(category: Option<BeliefCategory>) -> &'static str {
    match category {
        Some(BeliefCategory::Pantheon) => t!("classic.pedia.entries.belief-pantheon"),
        Some(BeliefCategory::Founder) => t!("classic.pedia.entries.belief-founder"),
        Some(BeliefCategory::Follower) => t!("classic.pedia.entries.belief-follower"),
        Some(BeliefCategory::Enhancer) => t!("classic.pedia.entries.belief-enhancer"),
        Some(BeliefCategory::Reformation) => t!("classic.pedia.entries.belief-reformation"),
        None => t!("classic.pedia.entries.belief"),
    }
}

fn beliefs(rules: &CompiledRuleset) -> Vec<Entry> {
    rules
        .module.beliefs
        .iter()
        .enumerate()
        .map(|(i, def)| {
            let id = PediaRef::new(Table::Belief, i as u16);
            let mut e = Entry::named(id, Category::Beliefs, &def.key, &def.name);
            e.subtitle = belief_category(def.category).into();
            e.art = art::belief(&def.key).map(Art::Card).unwrap_or(Art::None);
            if !def.text.is_empty() {
                let text = i18n::content("classic", "belief", &def.key, "desc", &def.text);
                e.text.push(Line::text(help_text(&[&e.name, &def.name], &text)));
            }
            e.text.extend(effect_lines(rules, &def.effects, &def.inert));
            e.link(t!("classic.pedia.entries.link-related"), effect_refs(rules, &def.effects, &def.inert));
            e
        })
        .collect()
}

fn religions(rules: &CompiledRuleset) -> Vec<Entry> {
    rules
        .module.religions
        .iter()
        .enumerate()
        .map(|(i, def)| {
            let id = PediaRef::new(Table::Religion, i as u16);
            let mut e = Entry::named(id, Category::Religions, &def.key, &def.name);
            e.subtitle = t!("classic.pedia.entries.religion").into();
            let preferred = rules
                .nations
                .iter()
                .enumerate()
                .filter(|(_, c)| c.module.preferred_religions.iter().any(|r| r.0 as usize == i))
                .map(|(c, _)| c as u16);
            e.link(t!("classic.pedia.entries.link-preferred-by"), refs(Table::Nation, preferred));
            e
        })
        .collect()
}

fn great_people(rules: &CompiledRuleset) -> Vec<Entry> {
    let mut out = Vec::new();
    for (i, def) in rules.module.great_people.iter().enumerate() {
        let id = PediaRef::new(Table::GreatPerson, i as u16);
        let Some(unit) = rules.units.get(def.unit.0 as usize) else {
            continue;
        };
        let name = super::content_name(Table::Unit, &unit.key, &unit.name);
        let mut e = Entry::new(id, Category::GreatPeople, &def.key, &name);
        e.subtitle = great_person(def.kind).into();
        e.art = art::unit(&unit.key).map(Art::Card).unwrap_or(Art::None);
        let source = match def.points_source {
            classic_rules::rules::content::PointsSource::Specialists => t!("classic.pedia.entries.points-specialists"),
            classic_rules::rules::content::PointsSource::CombatExperience => t!("classic.pedia.entries.points-combat"),
            classic_rules::rules::content::PointsSource::Faith => t!("classic.pedia.entries.points-faith"),
        };
        e.stat(t!("classic.pedia.entries.stat-points-from"), source);
        e.text.extend(def.inert.iter().map(|x| Line::inactive(describe::inert_effect(rules, x))));
        e.link(t!("classic.pedia.entries.link-unit"), one(Table::Unit, Some(def.unit.0)));
        e.link(t!("classic.pedia.entries.link-born-from"), one(Table::Specialist, def.specialist.map(|s| s.0)));
        e.link(t!("classic.pedia.entries.link-builds"), refs(Table::Improvement, def.improvements.iter().map(|b| b.0)));
        out.push(e);
    }
    out
}

fn specialists(rules: &CompiledRuleset) -> Vec<Entry> {
    rules
        .specialists
        .iter()
        .enumerate()
        .map(|(i, def)| {
            let id = PediaRef::new(Table::Specialist, i as u16);
            let mut e = Entry::named(id, Category::Specialists, &def.key, &def.name);
            e.subtitle = t!("classic.pedia.entries.specialist").into();
            let y = yields(&def.yields);
            if !y.is_empty() {
                e.stat(t!("classic.pedia.entries.stat-yields"), y);
            }
            if def.module.great_person_points > 0 {
                e.stat(t!("classic.pedia.entries.stat-great-person-points"), def.module.great_person_points);
            }
            e.link(t!("classic.pedia.entries.link-great-person"), one(Table::GreatPerson, def.module.great_person.map(|g| g.0)));
            e
        })
        .collect()
}

fn eras(rules: &CompiledRuleset) -> Vec<Entry> {
    rules
        .eras
        .iter()
        .enumerate()
        .map(|(i, def)| {
            let id = PediaRef::new(Table::Era, i as u16);
            let mut e = Entry::named(id, Category::Eras, &def.key, &def.name);
            e.subtitle = tf!("classic.pedia.entries.era-number", n = def.index + 1);
            e.stat(t!("classic.pedia.entries.stat-starting-gold"), def.starting_gold);
            e.stat(t!("classic.pedia.entries.stat-starting-military"), def.starting_defense_units);
            e.stat(t!("classic.pedia.entries.stat-starting-workers"), def.starting_worker_units);
            e.stat(t!("classic.pedia.entries.stat-starting-scouts"), def.starting_explore_units);
            e.stat(t!("classic.pedia.entries.stat-research-agreement-cost"), def.research_agreement_cost);
            let techs = (0..rules.technologies.len() as u16)
                .filter(|t| rules.technologies[*t as usize].content_era.map(|e| e.0 as usize) == Some(i));
            e.link(t!("classic.pedia.entries.link-technologies"), refs(Table::Technology, techs));
            e
        })
        .collect()
}

fn handicaps(rules: &CompiledRuleset) -> Vec<Entry> {
    rules
        .handicaps
        .iter()
        .enumerate()
        .map(|(i, def)| {
            let id = PediaRef::new(Table::Handicap, i as u16);
            let mut e = Entry::named(id, Category::Difficulty, &def.key, &def.name);
            e.subtitle = if Some(def) == rules.ai_handicap() {
                t!("classic.pedia.entries.handicap-ai").into()
            } else {
                tf!("classic.pedia.entries.handicap-level", n = def.index + 1)
            };
            let v = &def.values;
            let percent = |n: i32| tf!("classic.pedia.entries.value-percent", n = n);
            e.stat(t!("classic.pedia.entries.stat-base-happiness"), v.module.happiness_default);
            e.stat(t!("classic.pedia.entries.stat-happiness-per-luxury"), v.module.extra_happiness_per_luxury);
            e.stat(t!("classic.pedia.entries.stat-research-cost"), percent(v.research_percent as i32));
            e.stat(t!("classic.pedia.entries.stat-policy-cost"), percent(v.module.policy_percent as i32));
            e.stat(t!("classic.pedia.entries.stat-unit-cost"), percent(v.unit_cost_percent as i32));
            e.stat(t!("classic.pedia.entries.stat-building-cost"), percent(v.building_cost_percent as i32));
            e.stat(t!("classic.pedia.entries.stat-free-unit-supply"), v.production_free_units);
            e
        })
        .collect()
}
