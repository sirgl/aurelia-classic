use super::*;

const TABLES: [Table; 22] = [
    Table::Concept,
    Table::Era,
    Table::Civilization,
    Table::CityState,
    Table::CityStateType,
    Table::Technology,
    Table::Unit,
    Table::Promotion,
    Table::Building,
    Table::Project,
    Table::Policy,
    Table::PolicyBranch,
    Table::Belief,
    Table::Religion,
    Table::Resource,
    Table::Improvement,
    Table::Terrain,
    Table::Feature,
    Table::NaturalWonder,
    Table::GreatPerson,
    Table::Specialist,
    Table::Handicap,
];

#[test]
fn every_entity_of_the_ruleset_has_a_page() {
    let rules = crate::ui::screens::game_rules();
    let pedia = game_pedia();
    let mut pages = 0;
    for table in TABLES {
        let rows = table_len(&rules, table).unwrap();
        for index in 0..rows {
            let id = PediaRef::new(table, index as u16);
            let entry = pedia.entry(id).unwrap_or_else(|| panic!("no page for {id:?}"));
            assert!(!entry.name.is_empty() && entry.name != "Unknown", "{id:?} unnamed");
            assert_eq!(Some(entry.name.clone()), entity_name(&rules, id));
            pages += 1;
        }
    }
    assert_eq!(pages, pedia.entries().len(), "no page without an entity");
    assert!(pages > 1000, "{pages} pages");
    for category in Category::ALL {
        assert!(pedia.category(category).next().is_some(), "{category:?} is empty");
    }
    assert_eq!(pedia.category(Category::Civilizations).count(), 43);
}

#[test]
fn every_cross_link_resolves_and_keys_are_unique() {
    let pedia = game_pedia();
    let mut links = 0;
    for entry in pedia.entries() {
        assert_eq!(pedia.by_key(&entry.key), Some(entry.id), "key {} is ambiguous", entry.key);
        for target in entry.targets() {
            assert!(pedia.entry(target).is_some(), "{} links to missing {target:?}", entry.key);
            assert_ne!(target, entry.id, "{} links to itself", entry.key);
            links += 1;
        }
    }
    assert!(links > 3000, "{links} links");
}

#[test]
fn relations_are_navigable_from_both_ends() {
    let pedia = game_pedia();
    let page = |key: &str| pedia.entry(pedia.by_key(key).unwrap()).unwrap();
    let group = |entry: &Entry, label: &str| -> Vec<PediaRef> {
        entry.links.iter().find(|g| g.label == label).map(|g| g.targets.clone()).unwrap_or_default()
    };
    let swordsman = page("core:unit/swordsman");
    let iron = page("core:tech/iron-working");
    assert!(group(swordsman, "Required technology").contains(&iron.id));
    assert!(group(iron, "Unlocks").contains(&swordsman.id));
    let warrior = page("core:unit/warrior");
    assert!(group(warrior, "Upgrades to").iter().any(|u| group(pedia.entry(*u).unwrap(), "Upgrades from").contains(&warrior.id)));
    let pottery = page("core:tech/pottery");
    let leads_to = group(pottery, "Leads to");
    assert!(!leads_to.is_empty(), "Pottery leads somewhere");
    for next in leads_to {
        assert!(group(pedia.entry(next).unwrap(), "Requires").contains(&pottery.id));
    }
    let america = page("core:nation/america");
    assert!(!group(america, "Unique units").is_empty());
    assert!(america.text.iter().any(|l| l.text.contains("Manifest Destiny")));
}

#[test]
fn search_finds_every_page_by_its_name() {
    let pedia = game_pedia();
    for entry in pedia.entries() {
        let found = pedia.search(&entry.name);
        assert!(found.contains(&entry.id), "search for {:?} misses it", entry.name);
    }
    let warriors = pedia.search("warr");
    let first = pedia.entry(warriors[0]).unwrap();
    assert!(first.name.to_lowercase().starts_with("warr"), "{}", first.name);
    assert!(pedia.search("   ").is_empty());
    assert!(pedia.search("zzzz-no-such-thing").is_empty());
}

#[test]
fn pages_carry_stats_and_effects_from_the_data() {
    let rules = crate::ui::screens::game_rules();
    let pedia = game_pedia();
    let page = |key: &str| pedia.entry(pedia.by_key(key).unwrap()).unwrap();
    let warrior = page("core:unit/warrior");
    let def = rules.units.iter().find(|u| u.key == "core:unit/warrior").unwrap();
    let strength = warrior.stats.iter().find(|s| s.label == "Combat strength").unwrap();
    assert_eq!(strength.value, def.strength.to_string());
    let monument = page("core:building/monument");
    assert!(monument.text.iter().any(|l| l.kind == LineKind::Effect && l.text.contains("Culture")), "{:?}", monument.text);
    let growth = pedia.entries().iter().find(|e| e.name == "City Growth").unwrap();
    assert!(growth.text.iter().all(|l| l.kind == LineKind::Text));
}

#[test]
fn concepts_keep_navigation_identity_and_read_as_player_guidance() {
    let titles = [
        "Turn Order", "City Growth", "Border Growth", "Buying Tiles",
        "Production and Purchase Costs", "Gold", "Happiness", "Golden Ages",
        "Social Policies", "Science", "Great People", "Faith and Religion",
        "Score and the End of the Game", "Tourism and Influence",
        "Great Works and Archaeology", "Trade Routes", "City-States", "Diplomacy",
        "World Congress and United Nations", "Espionage", "Victory Conditions",
        "Unit Combat Strength", "Combat Damage", "Ranged Combat", "Cities in Combat",
        "Healing", "Experience and Promotions", "Movement", "One Unit per Tile", "Air Units",
    ];
    let pedia = game_pedia();
    assert_eq!(pedia.category(Category::Concepts).count(), titles.len());
    for (index, title) in titles.into_iter().enumerate() {
        let key = format!("concept:{index}");
        let id = PediaRef::new(Table::Concept, index as u16);
        assert_eq!(pedia.by_key(&key), Some(id));
        let page = pedia.entry(id).expect("stable concept index");
        assert_eq!(page.name, title);
        assert!(pedia.search(title).contains(&id));
        assert!(page.text.len() >= 2, "{title}: explanation and player guidance");
        assert!(!page.subtitle.contains("rules reference"));
        for line in &page.text {
            assert_eq!(line.kind, LineKind::Text, "{title}: no formula rendering");
            assert!(line.text.len() > 50 && line.text.ends_with('.'), "{title}: readable paragraph");
            for raw in ["```", "core:", "getGrowthThreshold", "BASE_CITY_", "m_k", " = ", "::", "§"] {
                assert!(!line.text.contains(raw), "{title}: leaked development notation {raw}");
            }
        }
    }
    let source = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/ui/pedia/concepts.rs")).unwrap();
    assert!(!source.contains("include_str!"), "player concepts must not embed the reference document");
    assert!(!source.contains("docs/classic/rules.md"));
    assert!(!source.contains("fn parse("), "no reference-markdown parser in player content");
}

/// `cargo test -p classic-ui --lib dump_pages -- --ignored --nocapture`: read pages as text.
#[test]
#[ignore]
fn dump_pages() {
    let pedia = game_pedia();
    let keys = std::env::var("AURELIA_PEDIA_KEYS").unwrap_or_else(|_| "core:nation/america,core:unit/swordsman".into());
    for key in keys.split(',') {
        let Some(id) = pedia.by_key(key) else {
            println!("MISSING {key}");
            continue;
        };
        let e = pedia.entry(id).unwrap();
        println!("=== {} [{:?}] {} | {:?}", e.name, e.category, e.subtitle, e.art);
        for s in &e.stats {
            println!("  {}: {}", s.label, s.value);
        }
        for l in &e.text {
            println!("  {:?}: {}", l.kind, l.text);
        }
        for g in &e.links {
            let names: Vec<String> = g.targets.iter().map(|t| pedia.entry(*t).unwrap().name.clone()).collect();
            println!("  -> {}: {}", g.label, names.join(", "));
        }
    }
}

#[test]
fn scientist_prophet_and_airport_pages_present_gameplay_not_import_metadata() {
    let pedia=game_pedia();
    let page=|key:&str| pedia.entry(pedia.by_key(key).unwrap()).unwrap();
    let scientist=page("core:unit/great-scientist");
    assert!(scientist.text.iter().any(|line|line.text=="Grants a burst of Science in proportion to your recent output"));
    let prophet=page("core:unit/great-prophet");
    for expected in ["Can found or enhance a religion","Can spread religion 4 times","Religious strength: 1000","Can spread its religion to a city"] {
        assert!(prophet.text.iter().any(|line|line.text==expected),"Prophet lacks {expected}");
    }
    let airport=page("core:building/airport");
    assert!(airport.text.iter().any(|line|line.text.contains("+4 aircraft capacity")));
    assert!(airport.text.iter().any(|line|line.text.contains("airlifted to or from this city")));
    for entry in pedia.entries() {
        for line in &entry.text {
            for internal in ["CIVILIAN ATTACK PRIORITY", "SPECIALUNIT", "civilian attack priority", "Unit ability work rate", "BeakersTurns(", "FoundReligion", "ReligionSpreads(", "ReligiousStrength(", "Modify city combat stat:", "Modify unit stat", "Grant ability(", "Modify city state", "Passive promotion(", "Post combat promotion(", "Grant experience("] {
                assert!(!line.text.contains(internal),"{} leaks {internal}: {}",entry.key,line.text);
            }
        }
    }
}

/// `AURELIA_PEDIA_DUMP=/path/file cargo test -p classic-ui dump_all_pages -- --ignored`: every page as
/// text, to compare the output of two builds (or two languages) line by line.
#[test]
#[ignore]
fn dump_all_pages() {
    let Ok(path) = std::env::var("AURELIA_PEDIA_DUMP") else { return };
    let pedia = Pedia::build(&crate::ui::screens::game_rules());
    let mut out = String::new();
    for e in pedia.entries() {
        out.push_str(&format!("=== {} | {} | {:?} | {}\n", e.key, e.name, e.category, e.subtitle));
        for s in &e.stats {
            out.push_str(&format!("  S {}: {}\n", s.label, s.value));
        }
        for l in &e.text {
            out.push_str(&format!("  {:?}: {}\n", l.kind, l.text));
        }
        for g in &e.links {
            let names: Vec<String> = g.targets.iter().map(|t| pedia.entry(*t).unwrap().name.clone()).collect();
            out.push_str(&format!("  -> {}: {}\n", g.label, names.join(", ")));
        }
    }
    for c in Category::ALL {
        out.push_str(&format!("category {c:?}: {}\n", c.label()));
    }
    std::fs::write(path, out).unwrap();
}
