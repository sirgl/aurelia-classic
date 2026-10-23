use classic_rules::compile_rules;

#[test]
fn all_shipped_civilizations_have_valid_name_pools() {
    let rules = compile_rules(classic_content::classic::classic_rules()).unwrap();
    assert_eq!(rules.civs.len(), 43);
    for civ in &rules.civs {
        assert!(!civ.city_names.is_empty(), "{}", civ.key);
        assert!(civ
            .city_names
            .iter()
            .all(|s| !s.is_empty() && s.trim() == s));
        let unique: std::collections::HashSet<_> =
            civ.city_names.iter().map(|s| s.to_lowercase()).collect();
        assert_eq!(unique.len(), civ.city_names.len(), "{}", civ.key);
    }
    assert_eq!(
        rules
            .civs
            .iter()
            .find(|c| c.key == "core:nation/rome")
            .unwrap()
            .city_names[0],
        "Rome"
    );
}

/// Enough real names to found a dozen cities before the numbering starts.
const MIN_CITY_NAMES: usize = 10;
/// The spies of a civilization take a few names in turn; the number is added after that.
const MIN_SPY_NAMES: usize = 3;

#[test]
fn every_civilization_has_its_own_real_city_names() {
    let rules = compile_rules(classic_content::classic::classic_rules()).unwrap();
    for civ in &rules.civs {
        assert!(civ.city_names.len() >= MIN_CITY_NAMES, "{} has {} names", civ.key, civ.city_names.len());
        let placeholder = civ.city_names.iter().find(|n| n.contains("Settlement") || n.starts_with("City "));
        assert_eq!(placeholder, None, "{} keeps a placeholder", civ.key);
    }
    let capitals: std::collections::HashSet<_> = rules.civs.iter().map(|c| c.city_names[0].to_lowercase()).collect();
    assert_eq!(capitals.len(), rules.civs.len(), "two civilizations start with the same capital");
}

#[test]
fn a_name_shared_by_two_civilizations_is_a_historical_one() {
    let rules = compile_rules(classic_content::classic::classic_rules()).unwrap();
    let mut holders: std::collections::BTreeMap<String, Vec<&str>> = Default::default();
    for civ in &rules.civs {
        for name in &civ.city_names {
            holders.entry(name.to_lowercase()).or_default().push(&civ.key);
        }
    }
    let shared: Vec<&str> = holders.iter().filter(|(_, civs)| civs.len() > 1).map(|(name, _)| name.as_str()).collect();
    // The pools are Unciv's lists (tools/import-unciv.py); each shared name is a real place of both nations.
    assert_eq!(
        shared,
        [
            "arbela", "athens", "birmingham", "brest", "caesarea", "fez", "memphis", "nantes", "nicopolis",
            "nimrud", "rennes", "richmond", "salamanca", "shiraz", "tula", "verona"
        ]
    );
}

#[test]
fn every_civilization_has_spy_names() {
    let rules = compile_rules(classic_content::classic::classic_rules()).unwrap();
    for civ in &rules.civs {
        let names = &civ.module.spy_names;
        assert!(names.len() >= MIN_SPY_NAMES, "{} has {} spy names", civ.key, names.len());
        assert!(names.iter().all(|s| !s.is_empty() && s.trim() == s), "{}", civ.key);
        let unique: std::collections::HashSet<_> = names.iter().map(|s| s.to_lowercase()).collect();
        assert_eq!(unique.len(), names.len(), "{}", civ.key);
    }
}

#[test]
fn legacy_mod_without_pool_and_explicit_pool_both_parse() {
    let legacy = r#"(civs:[(id:"mod:nation/example",name:"Example",leader:"Leader",color:(r:1,g:2,b:3),start_bias:"none")])"#;
    let source = classic_content::ron::parse_ron(legacy).unwrap();
    assert!(source.civs[0].city_names.is_empty());
    compile_rules(source).unwrap();
    let text = legacy.replace(
        "start_bias:\"none\"",
        "start_bias:\"none\",city_names:[\"First\",\"Second\"]",
    );
    let rules = compile_rules(classic_content::ron::parse_ron(&text).unwrap()).unwrap();
    assert_eq!(rules.civs[0].city_names, ["First", "Second"]);
}
