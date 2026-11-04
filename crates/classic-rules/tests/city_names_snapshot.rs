//! Frozen pre-change classic bytes prove compatibility independently of the new hash code.
use classic_rules::{
    compile_rules,
    persistence::{
        decode::{decode, decode_snapshot},
        encode::encode,
        DecodeError,
    },
    replay::authoritative_hash,
};

const BEFORE: &[u8] = include_bytes!("fixtures/city-names-before-v1.aurelia");
const BEFORE_RULES: u64 = 2736398163960058391;
const BEFORE_WORLD: u64 = 156481080629868776;

fn rules() -> classic_rules::rules::CompiledRuleset {
    compile_rules(classic_content::classic::classic_rules()).unwrap()
}

#[test]
fn frozen_snapshot_migrates_names_unchanged_but_strict_replay_rejects_it() {
    let rules = rules();
    let before = classic_rules::save_codec::from_bytes(BEFORE).unwrap();
    assert_eq!(before.header.ruleset_hash, BEFORE_RULES);
    assert_eq!(before.header.save_schema_version, 44);
    assert_eq!(before.header.simulation_version, 1);
    assert_eq!(rules.nations.len(), 43);
    assert_ne!(rules.hash(), BEFORE_RULES);
    assert!(matches!(
        decode(&before, &rules),
        Err(DecodeError::RulesetMismatch)
    ));
    let game = decode_snapshot(&before, &rules).unwrap();
    game.validate(&rules).unwrap();
    assert_eq!(authoritative_hash(&game), BEFORE_WORLD);
    let saved = encode(&game, &rules);
    assert_eq!(saved.header.ruleset_hash, rules.hash());
    assert_eq!(
        saved
            .world
            .cities
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        ["My Custom Capital", "City 2", "City 1", "City 2"]
    );
    assert_eq!(
        authoritative_hash(&decode(&saved, &rules).unwrap()),
        BEFORE_WORLD
    );
    // The codec writes the current version; the snapshot itself was not
    // touched by the load.
    let fresh = classic_rules::save_codec::from_bytes(BEFORE).unwrap();
    assert_eq!(
        classic_rules::save_codec::to_bytes(&before),
        classic_rules::save_codec::to_bytes(&fresh),
        "input snapshot is immutable"
    );
}

#[test]
fn legacy_snapshot_does_not_admit_other_content_changes() {
    let before = classic_rules::save_codec::from_bytes(BEFORE).unwrap();
    let original = rules();
    let mut variants = Vec::new();
    let mut units = original.clone();
    units.units[0].cost += 1;
    variants.push(units);
    let mut techs = original.clone();
    techs
        .technologies
        .iter_mut()
        .find(|t| !t.requires.is_empty())
        .unwrap()
        .requires
        .clear();
    variants.push(techs);
    let mut effects = original.clone();
    let nation = effects
        .nations
        .iter_mut()
        .find(|c| c.effects != Default::default())
        .unwrap();
    nation.effects = Default::default();
    variants.push(effects);
    let mut inserted = original.clone();
    let mut extra = inserted.units[0].clone();
    extra.key = "fixture:unit/new-id".into();
    inserted.units.insert(0, extra);
    variants.push(inserted);
    for changed in variants {
        assert!(matches!(
            decode_snapshot(&before, &changed),
            Err(DecodeError::RulesetMismatch)
        ));
    }
}

#[test]
fn legacy_snapshot_still_checks_versions_and_world_ranges() {
    let rules = rules();
    let before = classic_rules::save_codec::from_bytes(BEFORE).unwrap();
    let mut corrupt = before.clone();
    corrupt.world.cities[0].owner = u8::MAX;
    assert!(matches!(
        decode_snapshot(&corrupt, &rules),
        Err(DecodeError::Corrupt(_))
    ));
    let mut schema = before.clone();
    schema.header.save_schema_version += 1;
    assert!(matches!(
        decode_snapshot(&schema, &rules),
        Err(DecodeError::SchemaMismatch(_))
    ));
    let mut sim = before.clone();
    sim.header.simulation_version += 1;
    assert!(matches!(
        decode_snapshot(&sim, &rules),
        Err(DecodeError::SimMismatch(_))
    ));
    let mut unknown = before;
    unknown.header.ruleset_hash ^= 1;
    assert!(matches!(
        decode_snapshot(&unknown, &rules),
        Err(DecodeError::RulesetMismatch)
    ));
}
