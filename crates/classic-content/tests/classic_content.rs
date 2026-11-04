//! The classic ruleset against its source catalogs (classic/docs/classic): every catalog
//! entity compiles, the tech tree is a DAG with every technology reachable,
//! every unlock is reachable, and spot numbers match the catalogs.

use std::collections::{HashSet, VecDeque};

use classic_content::core::core_rules;
use classic_rules::ids::TechId;
use classic_rules::rules::content::{BuildingKind, UnitRole};
use classic_rules::rules::effects::{Condition, Modifier, Trigger, YieldKind};
use classic_rules::{compile_rules, CompiledRuleset};
use serde_json::Value;

fn rules() -> CompiledRuleset {
    compile_rules(core_rules()).expect("BNW ruleset compiles")
}

fn catalog(name: &str) -> Value {
    let path = format!("../../docs/classic/{name}");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    serde_json::from_str(&text).unwrap()
}

fn ids(name: &str, list: &str) -> Vec<String> {
    catalog(name)[list]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["id"].as_str().unwrap().to_string())
        .collect()
}

fn tech(rules: &CompiledRuleset, key: &str) -> TechId {
    let index = rules.technologies.iter().position(|t| t.key == key);
    TechId(index.unwrap_or_else(|| panic!("{key} must exist")) as u16)
}

#[test]
fn every_catalog_entity_compiles() {
    let r = rules();
    let keys: HashSet<&str> = r
        .units
        .iter()
        .map(|d| d.key.as_str())
        .chain(r.buildings.iter().map(|d| d.key.as_str()))
        .chain(r.technologies.iter().map(|d| d.key.as_str()))
        .chain(r.improvements.iter().map(|d| d.key.as_str()))
        .chain(r.features.iter().map(|d| d.key.as_str()))
        .chain(r.resources.iter().map(|d| d.key.as_str()))
        .chain(r.nations.iter().map(|d| d.key.as_str()))
        .chain(r.promotions.iter().map(|d| d.key.as_str()))
        .chain(r.module.policies.iter().map(|d| d.key.as_str()))
        .chain(r.module.beliefs.iter().map(|d| d.key.as_str()))
        .chain(r.eras.iter().map(|d| d.key.as_str()))
        .chain(r.terrains.iter().map(|d| d.key.as_str()))
        .chain(r.natural_wonders.iter().map(|d| d.key.as_str()))
        .chain(r.module.projects.iter().map(|d| d.key.as_str()))
        .chain(r.module.policy_branches.iter().map(|d| d.key.as_str()))
        .chain(r.module.religions.iter().map(|d| d.key.as_str()))
        .chain(r.module.city_state_types.iter().map(|d| d.key.as_str()))
        .chain(r.module.city_states.iter().map(|d| d.key.as_str()))
        .chain(r.specialists.iter().map(|d| d.key.as_str()))
        .collect();
    let catalogs = [
        ("techs.json", "items", 81),
        ("units.json", "items", 82),
        ("unique_units.json", "items", 59),
        ("buildings.json", "items", 62),
        ("unique_buildings.json", "items", 20),
        ("wonders.json", "items", 67),
        ("policies.json", "items", 111),
        ("policies.json", "branches", 12),
        ("beliefs.json", "items", 69),
        ("beliefs.json", "religions", 13),
        ("resources.json", "items", 42),
        ("improvements.json", "items", 29),
        ("terrain.json", "items", 9),
        ("features.json", "items", 8),
        ("natural_wonders.json", "items", 17),
        ("nations.json", "items", 43),
        ("city_states.json", "items", 58),
        ("city_states.json", "types", 5),
        ("promotions.json", "items", 214),
        ("great_people.json", "specialists", 7),
        ("eras.json", "items", 8),
    ];
    for (name, list, count) in catalogs {
        let ids = ids(name, list);
        assert_eq!(ids.len(), count, "{name} {list}");
        for id in ids {
            assert!(
                keys.contains(id.as_str()),
                "{id} from {name} did not compile"
            );
        }
    }
    // Great people are keyed core:great-person/<kind>; their unit is the catalog id.
    let great_units: HashSet<&str> = r
        .module.great_people
        .iter()
        .map(|g| r.units[g.unit.0 as usize].key.as_str())
        .collect();
    for id in ids("great_people.json", "items") {
        assert!(great_units.contains(id.as_str()), "great person {id}");
    }
}

/// Technologies reachable from the roots by learning only techs whose
/// prerequisites are all known.
fn reachable_techs(r: &CompiledRuleset) -> HashSet<TechId> {
    let mut known = HashSet::new();
    let mut queue: VecDeque<TechId> = VecDeque::new();
    for (i, t) in r.technologies.iter().enumerate() {
        if t.requires.is_empty() {
            queue.push_back(TechId(i as u16));
        }
    }
    while let Some(t) = queue.pop_front() {
        if !known.insert(t) {
            continue;
        }
        for (i, next) in r.technologies.iter().enumerate() {
            let id = TechId(i as u16);
            if !known.contains(&id) && next.requires.iter().all(|q| known.contains(q)) {
                queue.push_back(id);
            }
        }
    }
    known
}

#[test]
fn tech_tree_is_acyclic_and_every_technology_is_reachable() {
    let r = rules();
    assert_eq!(r.technologies.len(), 81);
    let roots: Vec<&str> = r
        .technologies
        .iter()
        .filter(|t| t.requires.is_empty())
        .map(|t| t.key.as_str())
        .collect();
    assert_eq!(roots, ["core:tech/agriculture"], "BNW has one root");
    let reachable = reachable_techs(&r);
    assert_eq!(reachable.len(), 81, "every technology is reachable");
    // Eras of the DAG grow along prerequisites (the compiler rejects cycles).
    for t in &r.technologies {
        for q in &t.requires {
            assert!(r.technologies[q.0 as usize].era < t.era, "{}", t.key);
        }
    }
}

#[test]
fn every_unlock_is_reachable() {
    let r = rules();
    let reachable = reachable_techs(&r);
    let check = |what: &str, key: &str, tech: Option<TechId>| {
        if let Some(t) = tech {
            assert!(
                reachable.contains(&t),
                "{what} {key} needs an unreachable tech"
            );
        }
    };
    for u in &r.units {
        check("unit", &u.key, u.required_tech);
    }
    for b in &r.buildings {
        check("building", &b.key, b.required_tech);
    }
    for i in &r.improvements {
        check("improvement", &i.key, i.required_tech);
    }
    for res in &r.resources {
        check("resource", &res.key, res.required_tech);
    }
    for p in &r.module.projects {
        check("project", &p.key, p.required_tech);
    }
    // Everything buildable without a technology is a deliberate root item.
    let free_units: Vec<&str> = r
        .units
        .iter()
        .filter(|u| u.required_tech.is_none() && !u.unbuildable && u.nation.is_none())
        .map(|u| u.key.as_str())
        .collect();
    assert_eq!(
        free_units,
        [
            "core:unit/scout",
            "core:unit/settler",
            "core:unit/warrior",
            "core:unit/worker"
        ]
    );
}

#[test]
fn spot_numbers_match_classic() {
    let r = rules();
    let unit = |k: &str| r.units.iter().find(|u| u.key == k).unwrap();
    let building = |k: &str| r.buildings.iter().find(|b| b.key == k).unwrap();
    let tech_def = |k: &str| &r.technologies[tech(&r, k).0 as usize];

    let warrior = unit("core:unit/warrior");
    assert_eq!(
        (warrior.strength, warrior.cost, warrior.movement),
        (8, 40, 2)
    );
    assert_eq!(warrior.required_tech, None);
    assert_eq!(warrior.role, Some(UnitRole::Melee));
    let swordsman = unit("core:unit/swordsman");
    assert_eq!((swordsman.strength, swordsman.cost), (14, 75));
    assert_eq!(
        swordsman.required_tech,
        Some(tech(&r, "core:tech/iron-working"))
    );
    let settler = unit("core:unit/settler");
    assert_eq!((settler.strength, settler.cost), (0, 106));
    assert!(settler.founder);
    let archer = unit("core:unit/archer");
    assert_eq!(
        (archer.strength, archer.ranged_strength, archer.range),
        (5, 7, 2)
    );

    let granary = building("core:building/granary");
    assert_eq!((granary.cost, granary.maintenance), (60, 1));
    assert_eq!(granary.required_tech, Some(tech(&r, "core:tech/pottery")));
    let food = granary.effects.passive_city.iter().any(|e| {
        e.modifier
            == Modifier::AddYield {
                yield_kind: YieldKind::Food,
                amount: 2,
            }
    });
    assert!(food, "granary +2 food");

    let writing = tech_def("core:tech/writing");
    assert_eq!(writing.cost, 55);
    assert_eq!(writing.requires, vec![tech(&r, "core:tech/pottery")]);

    let library = r
        .buildings
        .iter()
        .position(|b| b.key == "core:building/library")
        .unwrap();
    let great_library = building("core:building/great_library");
    assert_eq!(great_library.kind, BuildingKind::WorldWonder);
    assert!(great_library.wonder);
    assert_eq!(great_library.cost, 185);
    assert_eq!(
        great_library.required_tech,
        Some(tech(&r, "core:tech/writing"))
    );
    let free_library = great_library
        .effects
        .triggered(Trigger::OnBuilt)
        .iter()
        .any(|e| {
            e.modifier == Modifier::FreeBuilding(classic_rules::ids::BuildingKindId(library as u16))
        });
    assert!(free_library, "Great Library builds a free Library");
    let science = great_library.effects.passive_city.iter().any(|e| {
        e.modifier
            == Modifier::AddYield {
                yield_kind: YieldKind::Science,
                amount: 3,
            }
    });
    assert!(science, "Great Library +3 science");
    let free_tech = great_library
        .effects
        .triggered(Trigger::OnBuilt)
        .iter()
        .any(|e| e.modifier == Modifier::Grant(classic_rules::rules::effects::Grant::FreeTechnologies(1)));
    assert!(free_tech, "Great Library owes a free technology");

    let pyramids = building("core:building/pyramids");
    assert_eq!(
        (pyramids.cost, pyramids.kind),
        (185, BuildingKind::WorldWonder)
    );
    assert_eq!(pyramids.required_tech, Some(tech(&r, "core:tech/masonry")));
    let worker = r
        .units
        .iter()
        .position(|u| u.key == "core:unit/worker")
        .unwrap();
    let workers = pyramids
        .effects
        .triggered(Trigger::OnBuilt)
        .iter()
        .any(|e| {
            e.modifier
                == Modifier::FreeUnit {
                    unit: classic_rules::ids::UnitKindId(worker as u16),
                    count: 2,
                }
        });
    assert!(workers, "Pyramids: two free workers");
    let speed = pyramids.effects.passive_player.iter().any(|e| {
        e.modifier
            == Modifier::AddStat {
                stat: classic_rules::rules::stats::Stat::WorkerSpeedPercent,
                amount: 25,
            }
    });
    assert!(speed, "Pyramids: +25% worker speed");

    // Unconditional improvement yields of a technology are its Tile effects.
    let chemistry = tech_def("core:tech/chemistry");
    let mine = r
        .improvements
        .iter()
        .position(|i| i.key == "core:improvement/mine")
        .unwrap();
    let mine_bonus = chemistry.effects.passive_tile.iter().any(|e| {
        e.condition == Condition::TileImprovement(classic_rules::ids::ImprovementId(mine as u16))
    });
    assert!(mine_bonus, "Chemistry: +1 production on mines");
}

#[test]
fn unique_units_replace_their_class_for_their_nation() {
    let r = rules();
    let nation = r
        .nations
        .iter()
        .position(|c| c.key == "core:nation/aztecs")
        .unwrap() as u8;
    let jaguar = r.nation_variant(nation, "core:unit/warrior").unwrap();
    assert_eq!(r.units[jaguar.0 as usize].key, "core:unit/jaguar");
    let america = r
        .nations
        .iter()
        .position(|c| c.key == "core:nation/america")
        .unwrap() as u8;
    let warrior = r.nation_variant(america, "core:unit/warrior").unwrap();
    assert_eq!(r.units[warrior.0 as usize].key, "core:unit/warrior");
}
