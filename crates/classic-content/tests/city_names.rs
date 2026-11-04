use classic_rules::compile_rules;

#[test]
fn all_shipped_nations_have_valid_name_pools() {
    let rules = compile_rules(classic_content::classic::classic_rules()).unwrap();
    assert_eq!(rules.nations.len(), 43);
    for nation in &rules.nations {
        assert!(!nation.city_names.is_empty(), "{}", nation.key);
        assert!(nation
            .city_names
            .iter()
            .all(|s| !s.is_empty() && s.trim() == s));
        let unique: std::collections::HashSet<_> =
            nation.city_names.iter().map(|s| s.to_lowercase()).collect();
        assert_eq!(unique.len(), nation.city_names.len(), "{}", nation.key);
    }
    assert_eq!(
        rules
            .nations
            .iter()
            .find(|c| c.key == "core:nation/rome")
            .unwrap()
            .city_names[0],
        "Rome"
    );
}

/// Enough real names to found a dozen cities before the numbering starts.
const MIN_CITY_NAMES: usize = 10;
/// The spies of a nation take a few names in turn; the number is added after that.
const MIN_SPY_NAMES: usize = 3;

#[test]
fn every_nation_has_its_own_real_city_names() {
    let rules = compile_rules(classic_content::classic::classic_rules()).unwrap();
    for nation in &rules.nations {
        assert!(nation.city_names.len() >= MIN_CITY_NAMES, "{} has {} names", nation.key, nation.city_names.len());
        let placeholder = nation.city_names.iter().find(|n| n.contains("Settlement") || n.starts_with("City "));
        assert_eq!(placeholder, None, "{} keeps a placeholder", nation.key);
    }
    let capitals: std::collections::HashSet<_> = rules.nations.iter().map(|c| c.city_names[0].to_lowercase()).collect();
    assert_eq!(capitals.len(), rules.nations.len(), "two nations start with the same capital");
}

#[test]
fn a_name_shared_by_two_nations_is_a_historical_one() {
    let rules = compile_rules(classic_content::classic::classic_rules()).unwrap();
    let mut holders: std::collections::BTreeMap<String, Vec<&str>> = Default::default();
    for nation in &rules.nations {
        for name in &nation.city_names {
            holders.entry(name.to_lowercase()).or_default().push(&nation.key);
        }
    }
    let shared: Vec<&str> = holders.iter().filter(|(_, nations)| nations.len() > 1).map(|(name, _)| name.as_str()).collect();
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
fn every_nation_has_spy_names() {
    let rules = compile_rules(classic_content::classic::classic_rules()).unwrap();
    for nation in &rules.nations {
        let names = &nation.module.spy_names;
        assert!(names.len() >= MIN_SPY_NAMES, "{} has {} spy names", nation.key, names.len());
        assert!(names.iter().all(|s| !s.is_empty() && s.trim() == s), "{}", nation.key);
        let unique: std::collections::HashSet<_> = names.iter().map(|s| s.to_lowercase()).collect();
        assert_eq!(unique.len(), names.len(), "{}", nation.key);
    }
}

#[test]
fn legacy_mod_without_pool_and_explicit_pool_both_parse() {
    let legacy = r#"(nations:[(id:"mod:nation/example",name:"Example",leader:"Leader",color:(r:1,g:2,b:3),start_bias:"none")])"#;
    let source = classic_content::ron::parse_ron(legacy).unwrap();
    assert!(source.nations[0].city_names.is_empty());
    compile_rules(source).unwrap();
    let text = legacy.replace(
        "start_bias:\"none\"",
        "start_bias:\"none\",city_names:[\"First\",\"Second\"]",
    );
    let rules = compile_rules(classic_content::ron::parse_ron(&text).unwrap()).unwrap();
    assert_eq!(rules.nations[0].city_names, ["First", "Second"]);
}
