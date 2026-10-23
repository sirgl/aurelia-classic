//! Sweeps over the classic ruleset for the mechanic families brought to zero
//! inert effects (classic/docs/classic/implemented.md, «Inert effects by mechanic
//! family»): no inert effect may name a finished family, and every catalog
//! entity of a family compiles to a typed (active) effect of the expected
//! modifier. A family that becomes typed adds its prefix to `DONE` and one
//! `every_…` sweep below.

use classic_rules::classic::effects::{ClassicCondition, ClassicCount, ClassicGrant, ClassicModifier, ClassicTrigger, ClassicYield};
use classic_content::core::core_rules;
use classic_rules::rules::content::InertEffect;
use classic_rules::rules::content::ResourceClass;
use classic_rules::rules::effects::{CompiledEffect, Condition, CostTarget, Count, Duration, EffectSet, Forbidden, Grant, Modifier, PlotFilter, ProductionFilter, Scope, Trigger, YieldKind};
use classic_rules::classic::effects::{CityRelation, CityStateStat};
use classic_rules::rules::stats::{
    PlayerRule, Stat, SocietyStat, EspionageStat, LeagueStat, TradeDomain, TradeStat,
};
use classic_rules::rules::content::BuildingAbility;
use classic_rules::{compile_rules, CompiledRuleset};
use serde_json::Value;

fn rules() -> CompiledRuleset {
    compile_rules(core_rules()).expect("BNW ruleset compiles")
}

/// `needs_mechanic` prefixes that no inert effect may carry any more.
const DONE: &[&str] = &[
    "modifier:happiness.per_x_pop_in_city",
    "modifier:happiness.per_x_policies",
    "modifier:world_wonder.",
    "modifier:espionage.",
    "espionage_building",
    "espionage_rank_now",
    "spy_pressure",
    "modifier:league.",
    "source_effects:league_reward",
    "league_project_contribution",
    "allows_nukes",
    "building_requirement.unlocked_by_league_project",
    "modifier:culture.on_kill.",
    "modifier:gold.on_kill.",
    "modifier:faith.on_kill.",
    "condition:near_friendly_city_following_religion",
    "condition:near_enemy_city_following_religion",
    "modifier:heal.flat",
    "trigger:on_adopt",
    "modifier:gold.plunder.",
    "modifier:capture.plunder_gold.",
    "border_obstacle",
    "airlift",
    // Tile conditions and the Tile-scope percent (E2).
    "selector:river_tiles_of_city",
    "selector:water_tiles_of_city",
    "selector:water_resource_tiles_of_city",
    "selector:lake_tiles_of_city",
    "selector:natural_wonder_tiles_of_following_cities",
    "modifier:tile.",
    "tile_filter:",
    "faith_from_unimproved_forest",
    // Resources and improvements (E2).
    "modifier:resource.",
    "resource.revealed_by_policy",
    "hardcoded_policy_exploration_finisher",
    "improvement_placement",
    "culture_adjacent_same_type",
    "removes_resource",
    "luxury_copies_siphoned_from_minor",
    "feature.requires_river",
    "feature.requires_flatlands",
    "great_person_points_source",
    // Technologies and the Honor opener (E2).
    "triggers_archaeological_sites",
    "map_visible",
    "bridge_building",
    "modifier:route.movement_cost.flat",
    "always_see_barb_camps",
    "natural_wonder",
    "trigger:on_discover_natural_wonder",
    "modifier:production.",
    "resource.wonder_production",
    "capital_building_modifier",
    "modifier:purchase.",
    "selector:all_cities",
    "selector:this_city",
    "enables_ss_part_",
    "modifier:worker.",
    "modifier:route.maintenance",
    "no_hills_improvement_maintenance",
    "garrison_free_maintenance",
    "modifier:grant.tech",
    "modifier:grant.great_person_choice",
    "modifier:grant.unit_per_military_unit",
    "modifier:golden_age.",
    "modifier:city.population",
    "condition:city_garrisoned",
    "selector:coastal_cities",
    "modifier:happiness.per_city_connection",
    "happiness_to_",
    "half_specialist_",
    "no_occupied_unhappiness",
    "modifier:wonder.",
    "modifier:policy.free_building_on_conquest",
    "modifier:tourism.pct",
    "modifier:trait.free_unit",
    "modifier:trait.free_building",
    "modifier:forbid.",
    "free_social_policies_per_era",
    "tech_boost_from_capital_science_buildings",
    "maya_calendar_bonuses",
    "tech_from_city_conquer",
    "raze_speed_modifier",
    "extra_founded_city_territory_claim_range",
    "luxury_happiness_retention",
    "unique_luxury_",
    "no_annexing",
    // Religion (workstream D).
    "condition:min_followers",
    "condition:city_on_river",
    "condition:requires_peace",
    "trigger:on_great_person_expended",
    "trigger:on_city_converted",
    "trigger:on_religion_spread",
    "modifier:religion.pressure_to_friendly_city_states",
    "modifier:city_state.influence_resting_point",
    "modifier:happiness.per_x_peaceful_foreign_followers",
    "modifier:yield.production.pct_per_follower_max",
    "modifier:yield.faith.per_religion_present",
    "modifier:trade.route.yield.",
    "selector:specialists_of_following_cities",
    "other_religion_pressure_erosion",
    "converts_barbarians",
    "faith_purchase_all_great_people",
    "inquisitor_pressure_retention",
    "building_requirement.faith_purchase_unlocked_by_belief",
    "bonus_religious_belief",
    "second_religion_pantheon",
    // Trade routes, city-states, diplomacy, tourism (workstream F).
    "modifier:trade.land_range.",
    "modifier:trade.religious_pressure.",
    "modifier:trade.resource_diversity_gold.",
    "modifier:trade.routes.",
    "modifier:trade.incoming_route.",
    "modifier:trade.route.gold.",
    "modifier:trade.city_state_route.",
    "modifier:yield.gold.per_trade_partner",
    "modifier:yield.culture.per_trade_partner",
    "modifier:yield.production.pct_per_city_state_route",
    "modifier:city_connection.",
    "modifier:city_state.influence_per_turn",
    "modifier:city_state.unit_gift_rate.",
    "minor_resource_bonus",
    "minor_great_people_allies",
    "minor_science_allies",
    "great_person_gift_influence",
    "dof_great_person_modifier",
    // `modifier:trade_mission.gold.` stays: the promotion's own percent
    // (the trade mission gold modifier) belongs to the unit work.
    "modifier:research_agreement.",
    "modifier:great_scientist.",
    "trigger:on_unit_created",
    // Movement and wounded-fighting traits (gameplay wave, item 1).
    "crosses_mountains_after_great_general",
    "faster_in_hills",
    "move_friendly_woods_as_road",
    "fight_well_damaged",
    // Polynesia: +10 % strength within two tiles of an own Moai.
    "condition:near_improvement",
    "nearby_improvement_bonus_range",
    "modifier:trait.combat_bonus_improvement",
    // Promotions that the engine code carries (gameplay wave, item 1).
    "great_general_receives_movement",
    "hardcoded_promotion_anti_helicopter",
    "land_barbarian_conversion_percent",
    "modifier:religion.strength_loss_rival_land",
    "hardcoded_promotion_goody_hut_picker",
];

/// Every inert effect of the ruleset with the key of its owner.
fn all_inert(r: &CompiledRuleset) -> Vec<(String, InertEffect)> {
    let mut out = Vec::new();
    let mut add = |key: &str, list: &[InertEffect]| {
        for e in list {
            out.push((key.to_string(), e.clone()));
        }
    };
    r.units.iter().for_each(|d| add(&d.key, &d.inert));
    r.buildings.iter().for_each(|d| add(&d.key, &d.inert));
    r.technologies.iter().for_each(|d| add(&d.key, &d.inert));
    r.improvements.iter().for_each(|d| add(&d.key, &d.inert));
    r.features.iter().for_each(|d| add(&d.key, &d.inert));
    r.resources.iter().for_each(|d| add(&d.key, &d.inert));
    r.civs.iter().for_each(|d| add(&d.key, &d.inert));
    r.promotions.iter().for_each(|d| add(&d.key, &d.inert));
    r.module.policies.iter().for_each(|d| add(&d.key, &d.inert));
    r.module.beliefs.iter().for_each(|d| add(&d.key, &d.inert));
    r.natural_wonders.iter().for_each(|d| add(&d.key, &d.inert));
    r.module.projects.iter().for_each(|d| add(&d.key, &d.inert));
    r.module.city_state_types.iter().for_each(|d| add(&d.key, &d.inert));
    r.module.great_people.iter().for_each(|d| add(&d.key, &d.inert));
    out
}

#[test]
fn finished_families_have_no_inert_effect() {
    let r = rules();
    let left: Vec<String> = all_inert(&r)
        .into_iter()
        .filter(|(_, e)| DONE.iter().any(|p| e.needs_mechanic.starts_with(p)))
        .map(|(key, e)| format!("{key}: {} ({})", e.needs_mechanic, e.modifier))
        .collect();
    assert!(left.is_empty(), "inert effects of finished families:\n{}", left.join("\n"));
}

// ---- catalog helpers ----

fn catalog(name: &str) -> Value {
    let path = format!("../../docs/classic/{name}");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    serde_json::from_str(&text).unwrap()
}

const CATALOGS: [&str; 12] = [
    "beliefs.json",
    "buildings.json",
    "civilizations.json",
    "features.json",
    "improvements.json",
    "natural_wonders.json",
    "policies.json",
    "resources.json",
    "techs.json",
    "unique_buildings.json",
    "wonders.json",
    "great_people.json",
];

/// Ids of the catalog entities with an effect whose `modifier` is `name`
/// (civilization abilities included).
pub fn catalog_entities_with(name: &str) -> Vec<String> {
    let mut out = Vec::new();
    for file in CATALOGS {
        let cat = catalog(file);
        for item in cat["items"].as_array().into_iter().flatten() {
            let mut effects: Vec<&Value> = item["effects"].as_array().into_iter().flatten().collect();
            for ability in item["ability"].as_array().into_iter().flatten() {
                effects.extend(ability["effects"].as_array().into_iter().flatten());
            }
            if effects.iter().any(|e| e["modifier"].as_str() == Some(name)) {
                out.push(item["id"].as_str().unwrap().to_string());
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Ids of the catalog entities with an effect whose selector kind is `kind`.
pub fn catalog_entities_with_selector(kind: &str) -> Vec<String> {
    let mut out = Vec::new();
    for file in CATALOGS {
        let cat = catalog(file);
        for item in cat["items"].as_array().into_iter().flatten() {
            let effects = item["effects"].as_array().into_iter().flatten();
            if effects.into_iter().any(|e| e["selector"]["kind"].as_str() == Some(kind)) {
                out.push(item["id"].as_str().unwrap().to_string());
            }
        }
    }
    out.sort();
    out
}

/// The typed effects of the entity with this key (any table with effects).
fn effects_of<'a>(r: &'a CompiledRuleset, key: &str) -> &'a EffectSet {
    let found = r
        .civs
        .iter()
        .find(|d| d.key == key)
        .map(|d| &d.effects)
        .or_else(|| r.module.policies.iter().find(|d| d.key == key).map(|d| &d.effects))
        .or_else(|| r.module.beliefs.iter().find(|d| d.key == key).map(|d| &d.effects))
        .or_else(|| r.technologies.iter().find(|d| d.key == key).map(|d| &d.effects))
        .or_else(|| r.buildings.iter().find(|d| d.key == key).map(|d| &d.effects))
        .or_else(|| r.units.iter().find(|d| d.key == key).map(|d| &d.effects))
        .or_else(|| r.promotions.iter().find(|d| d.key == key).map(|d| &d.effects));
    found.unwrap_or_else(|| panic!("{key} has no effect table"))
}

fn all_effects(set: &EffectSet) -> impl Iterator<Item = &CompiledEffect> {
    let passive = [
        &set.passive_player,
        &set.passive_module,
        &set.passive_city,
        &set.passive_unit,
        &set.passive_tile,
    ];
    passive.into_iter().chain(set.triggered.iter()).flatten()
}

/// Every catalog entity with `catalog_modifier` has a typed effect that
/// `expected` accepts; returns how many there are (at least one).
pub fn assert_every_typed(r: &CompiledRuleset, catalog_modifier: &str, expected: impl Fn(&CompiledEffect) -> bool) -> usize {
    let ids = catalog_entities_with(catalog_modifier);
    assert!(!ids.is_empty(), "no catalog entity has {catalog_modifier}");
    for id in &ids {
        let set = effects_of(r, id);
        assert!(all_effects(set).any(&expected), "{id}: {catalog_modifier} is not typed as expected");
    }
    ids.len()
}

// ---- per-count modifiers ----

#[test]
fn every_per_count_effect_is_typed() {
    let r = rules();
    let per = |count: Count| {
        move |e: &CompiledEffect| match e.modifier {
            Modifier::Module(ClassicModifier::AddHappinessPerCount { per, .. }) | Modifier::AddYieldPerCount { per, .. } => per.count == count,
            _ => false,
        }
    };
    assert_eq!(assert_every_typed(&r, "happiness.per_x_pop_in_city", per(Count::Population)), 1);
    assert_eq!(assert_every_typed(&r, "happiness.per_x_policies", per(Count::Module(ClassicCount::Policies))), 1);
    assert_eq!(assert_every_typed(&r, "world_wonder.yield.faith.flat", per(Count::WorldWonders)), 1);
}

// ---- espionage and World Congress delegates ----

fn adds(stat: Stat) -> impl Fn(&CompiledEffect) -> bool {
    move |e: &CompiledEffect| matches!(e.modifier, Modifier::AddStat { stat: s, .. } if s == stat)
}

#[test]
fn every_espionage_effect_is_typed() {
    let r = rules();
    let espionage = |s| adds(Stat::Module(classic_rules::rules::stats::ClassicStat::Espionage(s)));
    assert_eq!(assert_every_typed(&r, "espionage.enemy_steal_rate.pct", espionage(EspionageStat::EnemyStealPercent)), 4);
    assert_eq!(assert_every_typed(&r, "espionage.steal_rate.pct", espionage(EspionageStat::StealPercent)), 1);
    assert_eq!(assert_every_typed(&r, "espionage.catch_chance.pct", espionage(EspionageStat::CatchPercent)), 1);
    assert_eq!(assert_every_typed(&r, "espionage.rig_election.pct", espionage(EspionageStat::RigElectionPercent)), 1);
    assert_eq!(assert_every_typed(&r, "espionage.new_spy_rank.flat", espionage(EspionageStat::StartingRank)), 1);
    assert_eq!(assert_every_typed(&r, "belief.spy_pressure", espionage(EspionageStat::SpyPressure)), 1);
    let spies = |e: &CompiledEffect| {
        matches!(e.modifier, Modifier::Module(ClassicModifier::GrantSpies { .. })) || adds(Stat::Module(classic_rules::rules::stats::ClassicStat::Espionage(EspionageStat::ExtraSpies)))(e)
    };
    assert_eq!(assert_every_typed(&r, "espionage.spies.flat", spies), 2);
    let ranks = |e: &CompiledEffect| matches!(e.modifier, Modifier::Module(ClassicModifier::PromoteSpies { .. }));
    assert_eq!(assert_every_typed(&r, "espionage.spy_rank.flat", ranks), 1);
}

#[test]
fn every_counter_espionage_building_has_its_ability() {
    let r = rules();
    let has = |modifier: &str, ability: BuildingAbility| {
        let ids = catalog_entities_with(modifier);
        for id in &ids {
            let def = r.buildings.iter().find(|b| &b.key == id).unwrap();
            assert!(def.module.abilities.contains(&ability), "{id}: {modifier} is not {ability:?}");
        }
        ids.len()
    };
    assert_eq!(has("building.espionage", BuildingAbility::CounterEspionage), 4);
    assert_eq!(has("building.affect_spies_now", BuildingAbility::AffectSpiesNow), 1);
}

#[test]
fn every_delegate_effect_is_typed() {
    let r = rules();
    assert_eq!(assert_every_typed(&r, "league.delegates.flat", adds(Stat::Module(classic_rules::rules::stats::ClassicStat::League(LeagueStat::Delegates)))), 1);
    let per_diplomat = adds(Stat::Module(classic_rules::rules::stats::ClassicStat::League(LeagueStat::DelegatesPerDiplomat)));
    assert_eq!(assert_every_typed(&r, "league.delegates_per_diplomat.flat", per_diplomat), 1);
}

// ---- World Congress projects and the Manhattan Project ----

#[test]
fn every_league_project_has_its_cost_and_typed_rewards() {
    let r = rules();
    let cat = catalog("wonders.json");
    let mut projects = 0;
    for item in cat["items"].as_array().unwrap() {
        let key = item["id"].as_str().unwrap();
        let def = r.module.projects.iter().find(|p| p.key == key);
        match item["kind"].as_str() {
            Some("league_project") => {
                let league = def.unwrap().league.as_ref().unwrap_or_else(|| panic!("{key}: no league data"));
                assert_eq!(i64::from(league.cost_per_player), item["cost_per_player"].as_i64().unwrap(), "{key}");
                let tiers = item["reward_tiers"].as_array().unwrap();
                for (tier, rewards) in tiers.iter().zip(&league.tiers) {
                    assert_eq!(tier["effects"].as_array().unwrap().len(), rewards.len(), "{key}: every reward typed");
                }
                projects += 1;
            }
            Some("project") => {
                let nukes = item["effects"].as_array().unwrap().iter().any(|e| e["modifier"] == "project.allows_nukes");
                assert_eq!(def.unwrap().allows_nukes, nukes, "{key}");
                assert!(def.unwrap().league.is_none());
            }
            _ => {}
        }
    }
    assert_eq!(projects, 3);
}

// ---- yields from kills ----

#[test]
fn every_kill_yield_is_typed() {
    let r = rules();
    let from_kill = |kind: YieldKind| {
        move |e: &CompiledEffect| matches!(e.modifier, Modifier::YieldFromKill { yield_kind, .. } if yield_kind == kind)
    };
    // Honor opener, the Aztecs; Honor finisher; God of War.
    assert_eq!(assert_every_typed(&r, "culture.on_kill.pct_of_strength", from_kill(YieldKind::Culture)), 2);
    assert_eq!(assert_every_typed(&r, "gold.on_kill.pct_of_strength", from_kill(YieldKind::Gold)), 1);
    assert_eq!(assert_every_typed(&r, "faith.on_kill.pct_of_strength", from_kill(YieldKind::Module(ClassicYield::Faith))), 1);
}

// ---- beliefs near following cities ----

#[test]
fn every_belief_near_a_following_city_is_typed() {
    let r = rules();
    let near = |relation: CityRelation| move |e: &CompiledEffect| e.condition == Condition::Module(ClassicCondition::NearFollowingCity(relation));
    assert_eq!(assert_every_typed(&r, "heal.flat", near(CityRelation::Own)), 1);
    for (id, relation) in [
        ("core:belief/enhancer/defender-of-the-faith", CityRelation::Own),
        ("core:belief/enhancer/just-war", CityRelation::Enemy),
    ] {
        assert!(all_effects(effects_of(&r, id)).any(near(relation)), "{id}");
    }
}

// ---- timed bonuses on all units ----

#[test]
fn every_attack_bonus_for_turns_is_timed_on_all_units() {
    let r = rules();
    let timed = |e: &CompiledEffect| {
        e.scope == Scope::Unit
            && e.trigger == Trigger::Module(ClassicTrigger::OnAdopt)
            && matches!(e.duration, Duration::Turns(_))
            && matches!(e.modifier, Modifier::ModifyCombat { .. })
    };
    assert_eq!(assert_every_typed(&r, "combat.attack.pct", timed), 1);
}

// ---- plunder ----

#[test]
fn every_plunder_percent_is_a_stat() {
    let r = rules();
    let stat = |wanted: Stat| move |e: &CompiledEffect| matches!(e.modifier, Modifier::AddStat { stat, .. } if stat == wanted);
    assert_eq!(assert_every_typed(&r, "gold.plunder.pct", stat(Stat::Module(classic_rules::rules::stats::ClassicStat::PlunderGoldPercent))), 1);
    assert_eq!(assert_every_typed(&r, "capture.plunder_gold.pct", stat(Stat::Module(classic_rules::rules::stats::ClassicStat::CapturedGoldPercent))), 1);
}

// ---- building abilities of movement ----

#[test]
fn border_obstacles_and_airlifts_are_building_abilities() {
    use classic_rules::rules::content::BuildingAbility;
    let r = rules();
    for (modifier, ability) in [
        ("building.border_obstacle", BuildingAbility::BorderObstacle),
        ("building.airlift", BuildingAbility::Airlift),
    ] {
        let ids = catalog_entities_with(modifier);
        assert_eq!(ids.len(), 1, "{modifier}");
        for id in &ids {
            let def = r.buildings.iter().find(|b| &b.key == id).unwrap();
            assert!(def.module.abilities.contains(&ability), "{id}");
        }
    }
    assert_eq!(r.effect_index.border_obstacle_buildings.len(), 1);
}

// ---- tile conditions (E2) ----

#[test]
fn every_plot_selector_is_a_tile_condition() {
    let r = rules();
    let cases = [
        ("river_tiles_of_city", PlotFilter::River),
        ("water_tiles_of_city", PlotFilter::Sea),
        ("water_resource_tiles_of_city", PlotFilter::SeaResource),
        ("lake_tiles_of_city", PlotFilter::Lake),
        ("natural_wonder_tiles_of_following_cities", PlotFilter::NaturalWonder),
    ];
    for (kind, plot) in cases {
        let ids = catalog_entities_with_selector(kind);
        assert!(!ids.is_empty(), "{kind}");
        for id in ids {
            let typed = all_effects(effects_of(&r, &id))
                .any(|e| e.scope == Scope::Tile && e.condition == Condition::TilePlot(plot));
            assert!(typed, "{id}: {kind} is not a Tile effect with {plot:?}");
        }
    }
}

#[test]
fn every_trait_tile_yield_is_typed() {
    let r = rules();
    let tile_percent = |e: &CompiledEffect| {
        e.scope == Scope::Tile
            && e.condition == Condition::TilePlot(PlotFilter::NaturalWonder)
            && matches!(e.modifier, Modifier::AddYieldPercent { percent: 100, .. })
    };
    assert_eq!(assert_every_typed(&r, "tile.yield.any.pct", tile_percent), 1);
    let unimproved = |e: &CompiledEffect| matches!(e.condition, Condition::TileFeatureUnimproved(_));
    assert_eq!(assert_every_typed(&r, "tile.yield.culture.flat_if_unimproved", unimproved), 1);
    let forest = |e: &CompiledEffect| matches!(e.condition, Condition::CityAdjacentUnimprovedFeature { .. });
    assert_eq!(assert_every_typed(&r, "trait.faith_from_unimproved_forest", forest), 1);
    let russia = effects_of(&r, "core:nation/russia");
    let strategic = all_effects(russia).any(|e| e.condition == Condition::TileResourceClass(ResourceClass::Strategic));
    assert!(strategic, "Russia: production on strategic resources");
}

// ---- resources and improvements (E2) ----

#[test]
fn every_resource_number_is_a_stat() {
    use classic_rules::rules::stats::Stat;
    let r = rules();
    let stat = |want: fn(Stat) -> bool| move |e: &CompiledEffect| matches!(e.modifier, Modifier::AddStat { stat, .. } if want(stat));
    assert_eq!(assert_every_typed(&r, "resource.provide", stat(|s| matches!(s, Stat::ResourceCopies(_)))), 1);
    assert_eq!(assert_every_typed(&r, "resource.quantity.pct", stat(|s| matches!(s, Stat::ResourceQuantityPercent(_)))), 2);
    assert_eq!(assert_every_typed(&r, "resource.strategic_quantity.pct", stat(|s| s == Stat::StrategicQuantityPercent)), 1);
    assert_eq!(assert_every_typed(&r, "resource.extra_luxury_copy", stat(|s| s == Stat::ExtraLuxuryCopies)), 1);
}

#[test]
fn every_improvement_rule_is_a_typed_field() {
    let r = rules();
    let cat = catalog("improvements.json");
    for item in cat["items"].as_array().unwrap() {
        let key = item["id"].as_str().unwrap();
        let def = r.improvements.iter().find(|d| d.key == key).unwrap();
        let flag = |name: &str| item["placement"][name].as_bool() == Some(true);
        let p = def.placement;
        assert_eq!(p.ignore_ownership, flag("ignore_ownership"), "{key}");
        assert_eq!(p.only_host_land, flag("only_city_state_territory"), "{key}");
        assert_eq!(p.requires_improvement, flag("requires_improvement"), "{key}");
        assert_eq!(p.prompt_when_complete, flag("prompt_when_complete"), "{key}");
        assert_eq!(p.destroyed_when_pillaged, flag("destroyed_when_pillaged"), "{key}");
        let effect = |name: &str| {
            item["effects"].as_array().into_iter().flatten().any(|e| e["modifier"].as_str() == Some(name))
        };
        assert_eq!(def.removes_resource, effect("improvement.removes_resource"), "{key}");
        assert_eq!(def.adjacent_same_culture > 0, effect("improvement.culture_adjacent_same_type"), "{key}");
        assert_eq!(def.module.city_state_luxury_copies > 0, effect("improvement.luxury_copies_siphoned_from_minor"), "{key}");
    }
    let hidden = r.resources.iter().find(|d| d.key == "core:resource/hidden_antiquity_site").unwrap();
    assert!(hidden.module.revealed_by_policy.is_some());
}

#[test]
fn every_great_person_names_its_points_source() {
    use classic_rules::rules::content::PointsSource;
    use classic_rules::classic::effects::GreatPersonKind;
    let r = rules();
    for def in &r.module.great_people {
        let expected = match def.kind {
            GreatPersonKind::General | GreatPersonKind::Admiral => PointsSource::CombatExperience,
            GreatPersonKind::Prophet => PointsSource::Faith,
            _ => PointsSource::Specialists,
        };
        assert_eq!(def.points_source, expected, "{}", def.key);
    }
}

#[test]
fn every_technology_rule_is_a_typed_field() {
    let r = rules();
    let cat = catalog("techs.json");
    for item in cat["items"].as_array().unwrap() {
        let key = item["id"].as_str().unwrap();
        let def = r.technologies.iter().find(|d| d.key == key).unwrap();
        let has = |name: &str| item["effects"].as_array().into_iter().flatten().any(|e| e["modifier"].as_str() == Some(name));
        assert_eq!(def.module.triggers_archaeological_sites, has("tech.triggers_archaeological_sites"), "{key}");
        assert_eq!(def.map_visible, has("tech.map_visible"), "{key}");
        assert_eq!(def.bridge_building, has("tech.bridge_building"), "{key}");
        assert_eq!(def.road_move_change != 0, has("route.movement_cost.flat"), "{key}");
    }
    let ability = |e: &CompiledEffect| matches!(e.modifier, Modifier::AddStat { stat: Stat::Module(classic_rules::rules::stats::ClassicStat::Rule(PlayerRule::SeeBarbarianCamps)), .. });
    assert_eq!(assert_every_typed(&r, "policy.always_see_barb_camps", ability), 1);
}

// ---- natural wonders ----

#[test]
fn every_natural_wonder_has_typed_placement_and_tile_rules() {
    use classic_rules::rules::natural_wonders::PlacementMethod;
    let r = rules();
    let cat = catalog("natural_wonders.json");
    let items = cat["items"].as_array().unwrap();
    assert_eq!(items.len(), r.natural_wonders.len());
    for item in items {
        let id = item["id"].as_str().unwrap();
        let def = r.natural_wonders.iter().find(|w| w.key == id).unwrap();
        assert!(def.inert.is_empty(), "{id}: {:?}", def.inert);
        assert_eq!(def.rough, item["rough"].as_bool().unwrap(), "{id} rough");
        assert_eq!(def.no_improvement, item["no_improvement"].as_bool().unwrap(), "{id} no_improvement");
        let placement = item["placement"].as_object().unwrap();
        let custom = placement.contains_key("eligibility_method_number");
        let rules_given = !def.placement.adjacent.is_empty()
            || !def.placement.core_terrains.is_empty()
            || def.placement.avoid_ocean_coast
            || def.placement.method != PlacementMethod::Land;
        assert!(custom || rules_given, "{id}: placement rules lost");
        let restricted = placement.get("core_tile_can_be_any_terrain_type").and_then(Value::as_bool) == Some(false);
        assert_eq!(restricted, !def.placement.core_terrains.is_empty(), "{id}: core terrains");
    }
}

#[test]
fn spain_finds_are_typed_triggers() {
    use classic_rules::rules::effects::Trigger;
    let r = rules();
    let first = |e: &CompiledEffect| e.trigger == Trigger::Module(ClassicTrigger::OnNaturalWonderFoundFirst);
    let later = |e: &CompiledEffect| e.trigger == Trigger::Module(ClassicTrigger::OnNaturalWonderFound);
    assert_eq!(assert_every_typed(&r, "gold.on_discover_natural_wonder_first", first), 1);
    assert_eq!(assert_every_typed(&r, "gold.on_discover_natural_wonder", later), 1);
}
// ---- production, purchase and maintenance ----

fn producing(e: &CompiledEffect) -> bool {
    let percent = matches!(e.modifier, Modifier::AddYieldPercent { yield_kind: YieldKind::Production, .. });
    percent && matches!(e.condition, Condition::Producing(_))
}

#[test]
fn every_production_percent_is_typed() {
    let r = rules();
    let mut entities = 0;
    for name in [
        "production.units.pct",
        "production.buildings.pct",
        "production.spaceship.pct",
        "production.military_units.pct",
        "production.wonders.pct",
        "production.building_class.pct",
    ] {
        entities += assert_every_typed(&r, name, producing);
    }
    // 30 catalog effects on 23 entities (Cultural Centers, Professional
    // Army and the Piety opener carry several classes).
    assert_eq!(entities, 23);
    let rome = |e: &CompiledEffect| e.condition == Condition::Producing(ProductionFilter::BuiltInCapital) && producing(e);
    assert_eq!(assert_every_typed(&r, "trait.capital_building_modifier", rome), 1);
    let marble = r.resources.iter().find(|d| d.key == "core:resource/marble").unwrap();
    let bonus = marble.wonder_production.expect("Marble speeds wonders");
    assert_eq!(bonus.percent, 15);
    assert!(bonus.before_era.is_some());
}

#[test]
fn every_purchase_and_maintenance_effect_is_typed() {
    let r = rules();
    let cost = |target: CostTarget| move |e: &CompiledEffect| matches!(e.modifier, Modifier::ModifyCost { target: t, .. } if t == target);
    assert_eq!(assert_every_typed(&r, "purchase.units.cost.pct", cost(CostTarget::UnitPurchase)), 1);
    assert_eq!(assert_every_typed(&r, "purchase.buildings.cost.pct", cost(CostTarget::BuildingPurchase)), 1);
    // Mercantilism and Big Ben.
    assert_eq!(assert_every_typed(&r, "purchase.cost.pct", cost(CostTarget::Hurry)), 2);
    let stat = |want: Stat| move |e: &CompiledEffect| matches!(e.modifier, Modifier::AddStat { stat, .. } if stat == want);
    assert_eq!(assert_every_typed(&r, "worker.speed.pct", stat(Stat::WorkerSpeedPercent)), 2);
    assert_eq!(assert_every_typed(&r, "route.maintenance.pct", stat(Stat::RouteMaintenancePercent)), 2);
    let rules_of = [
        ("policy.garrison_free_maintenance", PlayerRule::GarrisonFreeMaintenance),
        ("trait.no_hills_improvement_maintenance", PlayerRule::FreeHillsRouteMaintenance),
        ("policy.enables_ss_part_purchase", PlayerRule::SpaceshipPartPurchase),
        ("policy.enables_ss_part_hurry", PlayerRule::SpaceshipPartHurry),
    ];
    for (name, rule) in rules_of {
        assert_eq!(assert_every_typed(&r, name, stat(Stat::Module(classic_rules::rules::stats::ClassicStat::Rule(rule)))), 1);
    }
}

// ---- society: grants, conditions, stats, traits ----

fn society(stat: SocietyStat) -> impl Fn(&CompiledEffect) -> bool {
    move |e: &CompiledEffect| matches!(e.modifier, Modifier::AddStat { stat: Stat::Module(classic_rules::rules::stats::ClassicStat::Society(s)), .. } if s == stat)
}

#[test]
fn every_grant_is_typed() {
    let r = rules();
    let grant = |test: fn(Grant) -> bool| move |e: &CompiledEffect| matches!(e.modifier, Modifier::Grant(g) if test(g));
    assert_eq!(assert_every_typed(&r, "grant.tech", grant(|g| matches!(g, Grant::FreeTechnologies(1 | 2)))), 3);
    assert_eq!(assert_every_typed(&r, "grant.great_person_choice", grant(|g| matches!(g, Grant::Module(ClassicGrant::FreeGreatPeople(1))))), 2);
    assert_eq!(assert_every_typed(&r, "grant.unit_per_military_unit", grant(|g| g == Grant::MilitaryUnitCopies)), 1);
    assert_eq!(assert_every_typed(&r, "golden_age.start", grant(|g| g == Grant::Module(ClassicGrant::GoldenAge { turns: 10 }))), 1);
    assert_eq!(assert_every_typed(&r, "golden_age.start_turns", grant(|g| matches!(g, Grant::Module(ClassicGrant::GoldenAge { .. })))), 2);
    assert_eq!(assert_every_typed(&r, "city.population.flat", grant(|g| matches!(g, Grant::Population(_)))), 2);
    let conquest = |e: &CompiledEffect| e.trigger == Trigger::OnCityConquered;
    assert_eq!(assert_every_typed(&r, "policy.free_building_on_conquest", conquest), 1);
}

#[test]
fn every_society_condition_is_typed() {
    let r = rules();
    let garrison = |e: &CompiledEffect| e.condition == Condition::CityGarrisoned;
    let oligarchy = effects_of(&r, "core:policy/tradition/oligarchy");
    assert!(all_effects(oligarchy).any(garrison));
    let coastal = |e: &CompiledEffect| e.condition == Condition::CityCoastal;
    assert_eq!(assert_every_typed(&r, "trait.free_building", coastal), 1);
    let golden = |e: &CompiledEffect| e.condition == Condition::Module(ClassicCondition::GoldenAge);
    let brazil = effects_of(&r, "core:nation/brazil");
    assert_eq!(all_effects(brazil).filter(|e| golden(e)).count(), 4, "tourism and three great people rates");
    let caste = effects_of(&r, "core:policy/honor/military-caste");
    assert_eq!(all_effects(caste).filter(|e| garrison(e)).count(), 2);
    let maritime = effects_of(&r, "core:policy/exploration/maritime-infrastructure");
    assert!(all_effects(maritime).any(coastal));
}

#[test]
fn every_society_stat_is_typed() {
    let r = rules();
    let per_connection = |e: &CompiledEffect| {
        matches!(e.modifier, Modifier::Module(ClassicModifier::AddHappinessPerCount { per, .. }) if per.count == Count::ConnectedCities)
    };
    assert_eq!(assert_every_typed(&r, "happiness.per_city_connection", per_connection), 1);
    assert_eq!(assert_every_typed(&r, "policy.happiness_to_culture", society(SocietyStat::HappinessToCulturePercent)), 1);
    assert_eq!(assert_every_typed(&r, "policy.happiness_to_science", society(SocietyStat::ScienceWhileHappyPercent)), 1);
    assert_eq!(assert_every_typed(&r, "policy.half_specialist_food", society(SocietyStat::SpecialistFoodPercent)), 1);
    assert_eq!(
        assert_every_typed(&r, "policy.half_specialist_unhappiness", society(SocietyStat::SpecialistUnhappinessPercent)),
        1
    );
    assert_eq!(
        assert_every_typed(&r, "building.no_occupied_unhappiness", society(SocietyStat::NoOccupiedUnhappiness)),
        1
    );
    assert_eq!(assert_every_typed(&r, "wonder.culture.pct", society(SocietyStat::WonderCulturePercent)), 1);
}

#[test]
fn every_society_trait_is_typed() {
    let r = rules();
    let free_unit = |e: &CompiledEffect| {
        matches!(e.modifier, Modifier::FreeUnit { .. }) && matches!(e.condition, Condition::LearnedTechnology(_))
    };
    assert_eq!(assert_every_typed(&r, "trait.free_unit", free_unit), 2);
    assert_eq!(assert_every_typed(&r, "trait.free_unit_prereq_tech", free_unit), 2);
    let era = |e: &CompiledEffect| e.trigger == Trigger::OnEraEntered;
    assert_eq!(assert_every_typed(&r, "trait.free_social_policies_per_era", era), 1);
    let long_count = |e: &CompiledEffect| e.modifier == Modifier::Grant(Grant::Module(ClassicGrant::LongCountGreatPerson));
    assert_eq!(assert_every_typed(&r, "trait.maya_calendar_bonuses", long_count), 1);
    let conquest = |e: &CompiledEffect| e.modifier == Modifier::Grant(Grant::ConquestTechnology);
    assert_eq!(assert_every_typed(&r, "trait.tech_from_city_conquer", conquest), 1);
    let stats = [
        ("trait.tech_boost_from_capital_science_buildings", SocietyStat::CapitalScienceBuildingBoost),
        ("trait.raze_speed_modifier", SocietyStat::RazeSpeedPercent),
        ("trait.extra_founded_city_territory_claim_range", SocietyStat::FoundingTerritoryTiles),
        ("trait.luxury_happiness_retention", SocietyStat::LuxuryRetentionPercent),
        ("trait.unique_luxury_cities", SocietyStat::UniqueLuxuryCities),
        ("trait.unique_luxury_quantity", SocietyStat::UniqueLuxuryQuantity),
        ("trait.unique_luxury_requires_new_area", SocietyStat::UniqueLuxuryNewLandmass),
    ];
    for (name, stat) in stats {
        assert_eq!(assert_every_typed(&r, name, society(stat)), 1, "{name}");
    }
    let annex = |e: &CompiledEffect| e.modifier == Modifier::Forbid(Forbidden::Annexing);
    assert_eq!(assert_every_typed(&r, "trait.no_annexing", annex), 1);
    let settlers = |e: &CompiledEffect| matches!(e.modifier, Modifier::Forbid(Forbidden::UnitClassOf(_)));
    assert_eq!(assert_every_typed(&r, "forbid.train_unit_class", settlers), 1);
}

// ---- religion ----

#[test]
fn every_religion_effect_is_typed() {
    use classic_rules::rules::effects::{Condition, Trigger};
    use classic_rules::classic::effects::{ReligionAbility, ReligionStat};
    let r = rules();
    // Catalog names shared with other entities (building happiness, growth,
    // resting point, specialist yields) are checked per belief in
    // `religion_beliefs_carry_their_conditions`.
    assert_eq!(
        assert_every_typed(&r, "trade.route.yield.science.flat", |e| e.condition == Condition::ConnectedToCapital),
        1
    );
    let expended = |e: &CompiledEffect| e.trigger == Trigger::Module(ClassicTrigger::OnGreatPersonExpended);
    assert_eq!(assert_every_typed(&r, "faith.on_great_person_expended", expended), 1);
    assert_eq!(assert_every_typed(&r, "great_person_expended.gold.flat", expended), 1);
    assert_eq!(assert_every_typed(&r, "gold.on_city_first_converted", |e| e.trigger == Trigger::Module(ClassicTrigger::OnCityConverted)), 1);
    let spread = |e: &CompiledEffect| {
        e.trigger == Trigger::Module(ClassicTrigger::OnReligionSpread)
            && matches!(e.modifier, Modifier::AddYieldPerCount { per, .. } if per.count == Count::Module(ClassicCount::OtherReligionFollowers))
    };
    assert_eq!(assert_every_typed(&r, "science.on_spread_per_other_religion_follower", spread), 1);
    let religion = |stat: ReligionStat| move |e: &CompiledEffect| matches!(e.modifier, Modifier::Module(ClassicModifier::ModifyReligion { stat: s, .. }) if s == stat);
    assert_eq!(
        assert_every_typed(&r, "religion.pressure_to_friendly_city_states.pct", religion(ReligionStat::FriendlyCityStatePressure)),
        1
    );
    assert_eq!(assert_every_typed(&r, "belief.other_religion_pressure_erosion", religion(ReligionStat::OtherReligionErosion)), 1);
    assert_eq!(assert_every_typed(&r, "belief.inquisitor_pressure_retention", religion(ReligionStat::PressureRetention)), 1);
    let ability = |a: ReligionAbility| move |e: &CompiledEffect| e.modifier == Modifier::Module(ClassicModifier::GrantReligionAbility(a));
    assert_eq!(assert_every_typed(&r, "belief.converts_barbarians", ability(ReligionAbility::ConvertsBarbarians)), 1);
    assert_eq!(
        assert_every_typed(&r, "belief.faith_purchase_all_great_people", ability(ReligionAbility::FaithPurchaseAllGreatPeople)),
        1
    );
    let count = |c: Count| {
        move |e: &CompiledEffect| match e.modifier {
            Modifier::Module(ClassicModifier::AddHappinessPerCount { per, .. })
            | Modifier::AddYieldPerCount { per, .. }
            | Modifier::AddYieldPercentPerCount { per, .. } => per.count == c,
            _ => false,
        }
    };
    assert_eq!(assert_every_typed(&r, "happiness.per_x_peaceful_foreign_followers", count(Count::Module(ClassicCount::PeacefulForeignFollowers))), 1);
    assert_eq!(assert_every_typed(&r, "yield.production.pct_per_follower_max", count(Count::Module(ClassicCount::MajorityFollowers))), 1);
    assert_eq!(assert_every_typed(&r, "yield.faith.per_religion_present", count(Count::Module(ClassicCount::ReligionsPresent))), 1);
    assert_eq!(assert_every_typed(&r, "trait.bonus_religious_belief", |e| e.modifier == Modifier::Module(ClassicModifier::ExtraFoundingBelief)), 1);
    assert_eq!(
        assert_every_typed(&r, "policy.second_religion_pantheon", |e| e.modifier == Modifier::Module(ClassicModifier::SecondReligionPantheon)),
        1
    );
}

/// The precise religion sweeps the catalog modifier names above cannot
/// tell apart (shared names): the four follower beliefs with a follower
/// threshold, Papal Primacy, Guruship, Swords into Plowshares.
#[test]
fn religion_beliefs_carry_their_conditions() {
    use classic_rules::rules::effects::Condition;
    use classic_rules::classic::effects::CityStateStat;
    use classic_rules::rules::stats::Stat;
    let r = rules();
    for key in [
        "core:belief/follower/asceticism",
        "core:belief/follower/choral-music",
        "core:belief/follower/liturgical-drama",
        "core:belief/follower/religious-center",
    ] {
        let set = effects_of(&r, key);
        let gated = all_effects(set)
            .any(|e| matches!(e.condition, Condition::HasBuilding(_)) && matches!(e.second_condition, Condition::Module(ClassicCondition::MinFollowers(_))));
        assert!(gated, "{key}: building and followers");
    }
    let waters = effects_of(&r, "core:belief/pantheon/sacred-waters");
    assert!(all_effects(waters).any(|e| e.condition == Condition::CityOnRiver));
    let swords = effects_of(&r, "core:belief/follower/swords-into-plowshares");
    assert!(all_effects(swords).any(|e| e.condition == Condition::AtPeace && matches!(e.modifier, Modifier::ModifyGrowth { .. })));
    let papal = effects_of(&r, "core:belief/founder/papal-primacy");
    assert!(all_effects(papal).any(|e| matches!(
        e.modifier,
        Modifier::Module(ClassicModifier::ModifyCityState { stat: CityStateStat::RestingPointSameReligion, amount: 15 })
    )));
    let guruship = effects_of(&r, "core:belief/follower/guruship");
    assert!(all_effects(guruship).any(|e| matches!(e.modifier, Modifier::AddStat { stat: Stat::AnySpecialistYield(_), amount: 2 })));
}

/// Every building the catalog lets a belief unlock for faith carries the
/// typed rule, and only those.
#[test]
fn faith_purchase_unlock_rule_is_typed() {
    let r = rules();
    let mut expected = Vec::new();
    for file in ["buildings.json", "unique_buildings.json", "wonders.json"] {
        for item in catalog(file)["items"].as_array().into_iter().flatten() {
            if item["requirements"]["faith_purchase_unlocked_by_belief"].as_bool() == Some(true) {
                expected.push(item["id"].as_str().unwrap().to_string());
            }
        }
    }
    expected.sort();
    let mut typed: Vec<String> = r.buildings.iter().filter(|b| b.module.faith_purchase_needs_belief).map(|b| b.key.clone()).collect();
    typed.sort();
    assert_eq!(expected.len(), 8);
    assert_eq!(typed, expected);
}

// ---- trade routes, city-states, diplomacy, tourism ----

fn city_state(stat: CityStateStat) -> impl Fn(&CompiledEffect) -> bool {
    move |e: &CompiledEffect| matches!(e.modifier, Modifier::Module(ClassicModifier::ModifyCityState { stat: s, .. }) if s == stat)
}

fn counts(count: Count) -> impl Fn(&CompiledEffect) -> bool {
    move |e: &CompiledEffect| match e.modifier {
        Modifier::AddYieldPerCount { per, .. } | Modifier::AddYieldPercentPerCount { per, .. } => per.count == count,
        _ => false,
    }
}

#[test]
fn every_trade_effect_is_typed() {
    let r = rules();
    let trade = |t: TradeStat| adds(Stat::Module(classic_rules::rules::stats::ClassicStat::Trade(t)));
    assert_eq!(assert_every_typed(&r, "trade.land_range.flat_tiles", trade(TradeStat::RangeTiles(TradeDomain::Land))), 1);
    assert_eq!(assert_every_typed(&r, "trade.religious_pressure.pct", trade(TradeStat::ReligiousPressurePercent)), 1);
    assert_eq!(assert_every_typed(&r, "trade.resource_diversity_gold.pct", trade(TradeStat::ResourceGoldPercent)), 1);
    assert_eq!(assert_every_typed(&r, "trade.routes.pct", trade(TradeStat::RoutesPercent)), 1);
    assert_eq!(assert_every_typed(&r, "trade.incoming_route.yield.gold.flat", trade(TradeStat::IncomingRouteGold)), 1);
    assert_eq!(assert_every_typed(&r, "trade.route.gold.flat", trade(TradeStat::SharedIdeologyRouteGold)), 1);
    assert_eq!(assert_every_typed(&r, "trade.city_state_route.gold.flat", trade(TradeStat::CityStateRouteGold)), 1);
    assert_eq!(assert_every_typed(&r, "yield.gold.per_trade_partner", counts(Count::Module(ClassicCount::TradePartners))), 1);
    assert_eq!(assert_every_typed(&r, "yield.culture.per_trade_partner", counts(Count::Module(ClassicCount::TradePartners))), 1);
    assert_eq!(assert_every_typed(&r, "yield.production.pct_per_city_state_route", counts(Count::Module(ClassicCount::CityStateRoutes))), 1);
    assert_eq!(assert_every_typed(&r, "city_connection.gold.pct", adds(Stat::Module(classic_rules::rules::stats::ClassicStat::ConnectionGoldPercent))), 1);
}

#[test]
fn every_city_state_and_diplomacy_effect_is_typed() {
    let r = rules();
    let influence = |e: &CompiledEffect| {
        city_state(CityStateStat::AfraidInfluence)(e) || city_state(CityStateStat::TradeRouteInfluence)(e)
    };
    assert_eq!(assert_every_typed(&r, "city_state.influence_per_turn", influence), 2);
    assert_eq!(assert_every_typed(&r, "city_state.unit_gift_rate.pct", city_state(CityStateStat::UnitGiftRatePercent)), 1);
    assert_eq!(assert_every_typed(&r, "policy.minor_resource_bonus", city_state(CityStateStat::AllyResourceBonus)), 1);
    assert_eq!(assert_every_typed(&r, "policy.minor_great_people_allies", city_state(CityStateStat::AllyGreatPeople)), 1);
    assert_eq!(assert_every_typed(&r, "policy.minor_science_allies", city_state(CityStateStat::AllyScience)), 1);
    assert_eq!(assert_every_typed(&r, "trait.great_person_gift_influence", city_state(CityStateStat::GreatPersonGiftInfluence)), 1);
    assert_eq!(assert_every_typed(&r, "trade_mission.gold.pct", city_state(CityStateStat::TradeMissionGoldPercent)), 1);
    assert_eq!(assert_every_typed(&r, "trait.dof_great_person_modifier", adds(Stat::Module(classic_rules::rules::stats::ClassicStat::GreatPeopleRatePerFriend))), 1);
    assert_eq!(assert_every_typed(&r, "research_agreement.science.pct", adds(Stat::Module(classic_rules::rules::stats::ClassicStat::ResearchAgreementPercent))), 2);
    assert_eq!(assert_every_typed(&r, "great_scientist.science.pct", adds(Stat::Module(classic_rules::rules::stats::ClassicStat::GreatScientistSciencePercent))), 1);
}

#[test]
fn every_great_person_tourism_effect_is_typed() {
    let r = rules();
    let burst = |e: &CompiledEffect| {
        e.trigger == Trigger::Module(ClassicTrigger::OnGreatPersonBorn)
            && matches!(e.condition, Condition::Module(ClassicCondition::GreatPersonIs(_)))
            && matches!(e.modifier, Modifier::Module(ClassicModifier::InstantTourism { amount: 200 }))
    };
    assert_eq!(assert_every_typed(&r, "tourism.on_unit_created.pct_of_recent", burst), 1);
    let futurism = r.module.policies.iter().find(|p| p.key == "core:policy/autocracy/futurism").unwrap();
    assert_eq!(futurism.effects.triggered(Trigger::Module(ClassicTrigger::OnGreatPersonBorn)).len(), 3, "writer, artist, musician");
}

// ---- civilization traits of movement and wounded fighting ----

#[test]
fn every_movement_trait_is_typed() {
    use classic_rules::rules::effects::{UnitAbility, UnitStat};
    let r = rules();
    let ability = |a: UnitAbility| {
        move |e: &CompiledEffect| e.scope == Scope::Unit && matches!(e.modifier, Modifier::GrantAbility(g) if g == a)
    };
    assert_eq!(assert_every_typed(&r, "trait.faster_in_hills", ability(UnitAbility::HillsIgnoreTerrainCost)), 1);
    assert_eq!(assert_every_typed(&r, "trait.move_friendly_woods_as_road", ability(UnitAbility::RoadInFriendlyLand)), 1);
    let carthage = |e: &CompiledEffect| {
        ability(UnitAbility::CrossMountains)(e) && e.condition == Condition::Module(ClassicCondition::GreatGeneralEarned)
    };
    assert_eq!(assert_every_typed(&r, "trait.crosses_mountains_after_great_general", carthage), 1);
    let japan = |e: &CompiledEffect| {
        matches!(e.modifier, Modifier::ModifyUnitStat { stat: UnitStat::WoundedDamagePercent, amount: -33 })
    };
    assert_eq!(assert_every_typed(&r, "trait.fight_well_damaged", japan), 1);
    let moai = |e: &CompiledEffect| {
        e.scope == Scope::Unit
            && matches!(e.condition, Condition::NearOwnImprovement { range: 2, .. })
            && matches!(e.modifier, Modifier::ModifyCombat { percent: 10, .. })
    };
    assert!(all_effects(effects_of(&r, "core:nation/polynesia")).any(moai), "Polynesia: the Moai bonus");
    let germany = adds(Stat::Module(classic_rules::rules::stats::ClassicStat::LandBarbarianConversionPercent));
    assert_eq!(assert_every_typed(&r, "trait.land_barbarian_conversion_percent", germany), 1);
}

#[test]
fn every_hardcoded_promotion_is_typed() {
    use classic_rules::classic::effects::ClassicUnitAbility;
    use classic_rules::rules::effects::UnitAbility;
    let r = rules();
    let lends = |e: &CompiledEffect| {
        matches!(e.modifier, Modifier::GrantAbility(UnitAbility::Module(ClassicUnitAbility::LendsMovementToGeneral)))
    };
    assert_eq!(assert_every_typed_promotion(&r, "core:promotion/movement-to-general", lends), 1);
    let helicopters = |e: &CompiledEffect| {
        matches!(e.condition, Condition::OpponentRole(classic_rules::rules::content::UnitRole::Helicopter))
            && matches!(e.modifier, Modifier::ModifyCombat { percent: 100, .. })
    };
    assert_eq!(assert_every_typed_promotion(&r, "core:promotion/anti-helicopter", helicopters), 1);
    let evangelist = |e: &CompiledEffect| {
        matches!(
            e.modifier,
            Modifier::ModifyUnitStat {
                stat: classic_rules::rules::effects::UnitStat::Module(classic_rules::classic::effects::ClassicUnitStat::RivalLandStrengthLossPercent),
                amount: 25
            }
        )
    };
    assert_eq!(assert_every_typed_promotion(&r, "core:promotion/unwelcome-evangelist", evangelist), 1);
    let picker = |e: &CompiledEffect| {
        matches!(e.modifier, Modifier::GrantAbility(UnitAbility::Module(ClassicUnitAbility::ChoosesRuinsReward)))
    };
    assert_eq!(assert_every_typed_promotion(&r, "core:promotion/goody-hut-picker", picker), 1);
}

/// The promotion has a typed effect `expected` accepts; returns 1.
fn assert_every_typed_promotion(r: &CompiledRuleset, key: &str, expected: impl Fn(&CompiledEffect) -> bool) -> usize {
    assert!(all_effects(effects_of(r, key)).any(expected), "{key} is not typed as expected");
    1
}
