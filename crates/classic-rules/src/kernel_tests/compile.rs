//! Compiler tests: tech tree, nations.

use crate::rules::SourceRules;
use crate::CompiledRuleset;
use crate::rules::compile::*;

/// A piece of RON for a technology (invalid inputs as strings, like content errors
/// from files; no hardcoded tables here either).
fn tech(key: &str, cost: u32, requires: &[&str]) -> String {
    let reqs = requires
        .iter()
        .map(|k| format!("\"{}\"", k))
        .collect::<Vec<_>>()
        .join(", ");
    format!("(id: \"{}\", cost: {}, requires: [{}])", key, cost, reqs)
}

fn compile_techs(techs: Vec<String>) -> Result<CompiledRuleset, RulesError> {
    let body = techs.join(",\n");
    let source = crate::fixtures::parse_ron_text(&format!("(technologies: [{}])", body));
    compile_rules(source)
}

#[test]
fn tech_tree_resolves_eras_and_rejects_garbage() {
    // A chain: eras 0/1/2 by DAG depth.
    let rules = compile_techs(vec![
        tech("core:tech/writing", 60, &["core:tech/pottery"]),
        tech("core:tech/pottery", 40, &["core:tech/agriculture"]),
        tech("core:tech/agriculture", 20, &[]),
    ])
    .unwrap();
    let era = |k: &str| rules.technologies.iter().find(|t| t.key == k).unwrap().era;
    assert_eq!(
        (
            era("core:tech/agriculture"),
            era("core:tech/pottery"),
            era("core:tech/writing")
        ),
        (0, 1, 2)
    );
    // An unknown prerequisite: UnknownReference, not ignored.
    assert!(matches!(
        compile_techs(vec![tech("core:tech/a", 10, &["core:tech/nope"])]),
        Err(RulesError::UnknownReference(_))
    ));
    // A cycle a<->b: ForbiddenCycle.
    assert!(matches!(
        compile_techs(vec![
            tech("core:tech/a", 10, &["core:tech/b"]),
            tech("core:tech/b", 10, &["core:tech/a"]),
        ]),
        Err(RulesError::ForbiddenCycle(_))
    ));
    // A self-cycle is also a cycle.
    assert!(matches!(
        compile_techs(vec![tech("core:tech/a", 10, &["core:tech/a"])]),
        Err(RulesError::ForbiddenCycle(_))
    ));
    // A zero price: ZeroCost.
    assert!(matches!(
        compile_techs(vec![tech("core:tech/a", 0, &[])]),
        Err(RulesError::ZeroCost(_))
    ));
}

#[test]
fn nations_reject_duplicate_keys_and_empty_names() {
    use crate::rules::source::{SourceNation, StableContentKey};
    let mk = |id: &str, name: &str| SourceNation {
        id: Some(StableContentKey::new(id)),
        name: name.into(),
        leader: "Leader".into(),
        color: (10, 20, 30),
        start_bias: Default::default(),
        effects: vec![],
        ..Default::default()
    };
    let with_nations = |nations: Vec<SourceNation>| {
        let mut source = crate::fixtures::parse_ron_text("(technologies: [])");
        source.nations = nations;
        compile_rules(source)
    };
    // A valid input compiles, the key sorts stably.
    let ok = with_nations(vec![mk("core:nation/b", "B"), mk("core:nation/a", "A")]).unwrap();
    assert_eq!(ok.nations[0].key, "core:nation/a");
    assert_eq!(ok.nations[1].key, "core:nation/b");
    // A duplicate key: DuplicateId.
    assert!(matches!(
        with_nations(vec![mk("core:nation/a", "A"), mk("core:nation/a", "B")]),
        Err(RulesError::DuplicateId(_))
    ));
    // An empty name: EmptyName (cannot be shown in the menu).
    assert!(matches!(
        with_nations(vec![mk("core:nation/a", "   ")]),
        Err(RulesError::EmptyName(_))
    ));
}

// ---- Content tables: consistency checks ----

use crate::rules::content::{InertAmount, SourceInertEffect};
use crate::rules::source::{
    SourceBuilding, SourceNation, SourcePolicy, SourceTechnology, SourceUnit, StableContentKey,
};

fn k(s: &str) -> StableContentKey {
    StableContentKey::new(s)
}

fn classic_tech(id: &str, unlocks_units: &[&str]) -> SourceTechnology {
    SourceTechnology {
        id: Some(k(id)),
        cost: 10,
        unlocks_units: unlocks_units.iter().map(|u| k(u)).collect(),
        ..Default::default()
    }
}

fn classic_unit(id: &str, tech: Option<&str>) -> SourceUnit {
    SourceUnit {
        id: Some(k(id)),
        movement: 2,
        strength: 8,
        cost: 40,
        required_tech: tech.map(k),
        ..Default::default()
    }
}

#[test]
fn unlock_lists_must_mirror_required_tech() {
    let mut source = SourceRules {
        technologies: vec![classic_tech("core:tech/a", &["core:unit/x"])],
        units: vec![classic_unit("core:unit/x", Some("core:tech/a"))],
        ..Default::default()
    };
    assert!(compile_rules(source.clone()).is_ok());
    source.units[0].required_tech = None;
    assert!(matches!(
        compile_rules(source),
        Err(RulesError::Inconsistent(_))
    ));
}

#[test]
fn unique_units_mirror_their_nation_and_class() {
    let nation = SourceNation {
        id: Some(k("core:nation/c")),
        name: "C".into(),
        leader: "L".into(),
        unique_units: vec![k("core:unit/uu")],
        ..Default::default()
    };
    let mut uu = classic_unit("core:unit/uu", None);
    uu.nation = Some(k("core:nation/c"));
    uu.replaces = Some(k("core:unit/x"));
    uu.class = Some(k("core:unit-class/x"));
    let mut regular = classic_unit("core:unit/x", None);
    regular.class = Some(k("core:unit-class/x"));
    let source = SourceRules {
        nations: vec![nation],
        units: vec![regular, uu],
        ..Default::default()
    };
    assert!(compile_rules(source.clone()).is_ok());
    let mut other_class = source.clone();
    other_class.units[1].class = Some(k("core:unit-class/y"));
    assert!(matches!(
        compile_rules(other_class),
        Err(RulesError::Inconsistent(_))
    ));
    let mut unlisted = source;
    unlisted.nations[0].unique_units.clear();
    assert!(matches!(
        compile_rules(unlisted),
        Err(RulesError::Inconsistent(_))
    ));
}

#[test]
fn policy_prerequisite_cycle_is_rejected() {
    let policy = |id: &str, pre: &str| SourcePolicy {
        id: Some(k(id)),
        prerequisites: vec![k(pre)],
        ..Default::default()
    };
    let source = SourceRules {
        module: crate::classic::tables::ClassicSourceTables {
            policies: vec![
                policy("core:policy/a", "core:policy/b"),
                policy("core:policy/b", "core:policy/a"),
            ],
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(matches!(
        compile_rules(source),
        Err(RulesError::ForbiddenCycle(_))
    ));
}

#[test]
fn inert_effects_resolve_their_references() {
    let inert = |r: &str| SourceInertEffect {
        needs_mechanic: "city_defense".into(),
        modifier: "city.defense.flat".into(),
        scope: "city".into(),
        selector: "this_city".into(),
        trigger: "passive".into(),
        amount: InertAmount::Hundredths(250),
        refs: vec![k(r)],
    };
    let building = |r: &str| SourceBuilding {
        id: Some(k("core:building/walls")),
        cost: 75,
        inert: vec![inert(r)],
        ..Default::default()
    };
    let ok = SourceRules {
        buildings: vec![building("core:unit-combat/melee")],
        ..Default::default()
    };
    let rules = compile_rules(ok).unwrap();
    assert_eq!(rules.buildings[0].inert.len(), 1);
    let bad = SourceRules {
        buildings: vec![building("core:unit/nope")],
        ..Default::default()
    };
    assert!(matches!(
        compile_rules(bad),
        Err(RulesError::UnknownReference(_))
    ));
}

#[test]
fn unbuildable_items_have_no_cost_and_buildable_ones_need_one() {
    let mut gp = classic_unit("core:unit/great-x", None);
    gp.cost = 0;
    gp.unbuildable = true;
    let source = SourceRules {
        units: vec![gp],
        ..Default::default()
    };
    assert!(compile_rules(source.clone()).is_ok());
    let mut priced = source.clone();
    priced.units[0].cost = 5;
    assert!(matches!(
        compile_rules(priced),
        Err(RulesError::Inconsistent(_))
    ));
    let mut free = source;
    free.units[0].unbuildable = false;
    assert!(matches!(compile_rules(free), Err(RulesError::ZeroCost(_))));
}

/// A World Congress project rewarding a building, and an ordinary project
/// that allows nuclear weapons, with a nuclear unit requiring it.
fn league_source() -> SourceRules {
    use crate::rules::content::ProjectKind;
    use crate::rules::source::{SourceLeagueProject, SourceLeagueReward, SourceProject};
    let mut bomb = classic_unit("core:unit/bomb", None);
    bomb.combat.nuke_level = 1;
    bomb.required_project = Some(k("core:project/manhattan"));
    SourceRules {
        units: vec![bomb],
        buildings: vec![SourceBuilding {
            id: Some(k("core:building/station")),
            unbuildable: true,
            ..Default::default()
        }],
        module: crate::classic::tables::ClassicSourceTables {
            projects: vec![
                SourceProject {
                    id: Some(k("core:project/fair")),
                    kind: Some(ProjectKind::LeagueProject),
                    league: Some(SourceLeagueProject {
                        cost_per_player: 350,
                        tech_any_member: None,
                        tiers: vec![
                            vec![SourceLeagueReward::GoldenAgePoints(500)],
                            vec![SourceLeagueReward::FreePolicies(1)],
                            vec![SourceLeagueReward::Building(k("core:building/station"))],
                        ],
                    }),
                    ..Default::default()
                },
                SourceProject {
                    id: Some(k("core:project/manhattan")),
                    kind: Some(ProjectKind::Project),
                    cost: 750,
                    allows_nukes: true,
                    ..Default::default()
                },
            ],
            ..Default::default()
        },
        ..Default::default()
    }
}

#[test]
fn league_projects_and_nuclear_weapons_are_checked() {
    let source = league_source();
    let rules = compile_rules(source.clone()).unwrap();
    let fair = rules.module.projects.iter().find(|p| p.key == "core:project/fair").unwrap();
    assert_eq!(fair.league.as_ref().unwrap().cost_per_player, 350);
    let inconsistent = |s: SourceRules| matches!(compile_rules(s), Err(RulesError::Inconsistent(_)));
    let mut two_tiers = source.clone();
    two_tiers.module.projects[0].league.as_mut().unwrap().tiers.pop();
    assert!(inconsistent(two_tiers), "three tiers");
    let mut no_data = source.clone();
    no_data.module.projects[0].league = None;
    assert!(inconsistent(no_data), "a World Congress project needs its data");
    let mut on_project = source.clone();
    on_project.module.projects[1].league = source.module.projects[0].league.clone();
    assert!(inconsistent(on_project), "an ordinary project has none");
    let mut buildable = source.clone();
    buildable.buildings[0].unbuildable = false;
    buildable.buildings[0].cost = 100;
    assert!(inconsistent(buildable), "a rewarded building is not built");
    let mut harmless = source;
    harmless.module.projects[1].allows_nukes = false;
    assert!(inconsistent(harmless), "a nuclear weapon needs a project allowing it");
}
