//! Role bindings: the compiler resolves them, rejects a carrier that does
//! not fit, and the fingerprint covers them without disturbing the hash of
//! rules written before roles existed.

use crate::rules::compile::RulesError;
use crate::rules::roles::{Climate, Ground, TerrainPalette};
use crate::rules::{compile_rules, SourceRules};

const CONTENT: &str = r#"(
    units: [(id: "core:unit/settler", movement: 2, strength: 1, founder: true, cost: 40)],
)"#;

fn source() -> SourceRules {
    crate::fixtures::parse_ron_text(CONTENT)
}

fn rebind(source: &mut SourceRules, ground: Ground, slug: &str) {
    let key = crate::rules::source::StableContentKey::new(format!("core:terrain/{slug}"));
    for (g, k) in &mut source.roles.grounds {
        if *g == ground {
            *k = key.clone();
        }
    }
}

fn inconsistent(source: SourceRules) -> bool {
    matches!(compile_rules(source), Err(RulesError::Inconsistent(_)))
}

#[test]
fn grounds_resolve_to_the_palette_and_terrain_kinds() {
    let rules = compile_rules(source()).unwrap();
    let palette = rules.terrain_kinds.palette();
    assert_eq!(palette, TerrainPalette::LEGACY);
    for ground in Ground::ALL {
        assert_eq!(palette.ground(palette.code(ground)), Some(ground));
    }
    let hills = palette.code(Ground::Hills);
    assert!(rules.terrain_kinds.is_hills(hills) && !rules.terrain_kinds.is_mountains(hills));
    assert!(rules.terrain_kinds.is_mountains(palette.code(Ground::Mountains)));
    assert!(rules.terrain_kinds.is_water(palette.code(Ground::Water)));
}

#[test]
fn rules_without_terrains_need_no_grounds() {
    let rules = compile_rules(SourceRules::default()).unwrap();
    assert_eq!(rules.terrain_kinds, crate::navigation::cost::TerrainKinds::LEGACY);
}

#[test]
fn a_carrier_must_fit_its_ground() {
    let mut missing = source();
    missing.roles.grounds.retain(|(g, _)| *g != Ground::Hills);
    assert!(inconsistent(missing), "every ground needs a terrain");

    let mut twice = source();
    let extra = twice.roles.grounds[0].clone();
    twice.roles.grounds.push(extra);
    assert!(inconsistent(twice), "a role has one carrier");

    let mut dry_water = source();
    rebind(&mut dry_water, Ground::Water, "plains");
    assert!(inconsistent(dry_water), "water is played by a water terrain");

    let mut shared = source();
    rebind(&mut shared, Ground::Flat(Climate::Cold), "snow");
    assert!(inconsistent(shared), "two grounds, one terrain");

    let mut unknown = source();
    rebind(&mut unknown, Ground::Hills, "hillock");
    assert!(matches!(compile_rules(unknown), Err(RulesError::UnknownReference(_))));
}

#[test]
fn bindings_are_part_of_the_current_fingerprint_only() {
    let base = compile_rules(source()).unwrap();
    let mut swapped = source();
    rebind(&mut swapped, Ground::Flat(Climate::Humid), "plains");
    rebind(&mut swapped, Ground::Flat(Climate::Temperate), "grassland");
    let swapped = compile_rules(swapped).unwrap();
    assert_ne!(base.hash(), swapped.hash(), "a changed binding is changed content");
    assert_eq!(base.hash_before_roles(), swapped.hash_before_roles());
    assert_eq!(base.hash_before_city_names(), swapped.hash_before_city_names());
}

#[test]
fn snapshot_of_rules_before_roles_still_loads() {
    use crate::persistence::{decode, encode::encode, DecodeError};
    let rules = compile_rules(source()).unwrap();
    let config = crate::world::state::GameConfig { players: 2, width: 8, height: 8, rng_seed: 1 };
    let mut save = encode(&crate::GameState::new(&rules, config), &rules);
    save.header.ruleset_hash = rules.hash_before_roles();
    assert!(decode::decode_snapshot(&save, &rules).is_ok(), "a save written before roles loads");
    assert!(matches!(decode::decode(&save, &rules), Err(DecodeError::RulesetMismatch)), "replay stays strict");
}

#[test]
fn a_feature_plays_one_role() {
    use crate::rules::roles::FeatureRole;
    let text = r#"(features: [(id: "core:feature/forest", chop_turns: 2, prod_bonus: 1, chop_grant: 8)])"#;
    let rules = compile_rules(crate::fixtures::parse_ron_text(text)).unwrap();
    let forest = rules.roles.feature(FeatureRole::Forest).expect("the fixture binds its forest");
    assert_eq!(rules.roles.feature_role(forest), Some(FeatureRole::Forest));
    assert_eq!(rules.roles.feature(FeatureRole::Jungle), None);

    let mut twice = crate::fixtures::parse_ron_text(text);
    let forest = twice.roles.features[0].1.clone();
    twice.roles.features.push((FeatureRole::Jungle, forest));
    assert!(inconsistent(twice));
}

#[test]
fn specialists_and_handicaps_need_their_default_carriers() {
    use crate::rules::source::{SourceHandicap, SourceSpecialist, StableContentKey};
    let mut specialists = source();
    let citizen = StableContentKey::new("core:specialist/citizen");
    specialists.specialists.push(SourceSpecialist { id: Some(citizen.clone()), ..Default::default() });
    assert!(inconsistent(specialists.clone()), "a default specialist is required");
    specialists.roles.default_specialist = Some(citizen);
    assert!(compile_rules(specialists).unwrap().roles.default_specialist.is_some());

    let mut handicaps = source();
    let level = StableContentKey::new("core:handicap/prince");
    handicaps.handicaps.push(SourceHandicap { id: Some(level.clone()), ..Default::default() });
    handicaps.roles.default_difficulty = Some(level.clone());
    assert!(inconsistent(handicaps.clone()), "the AI handicap is required");
    handicaps.roles.ai_handicap = Some(level);
    let rules = compile_rules(handicaps).unwrap();
    assert_eq!(rules.default_difficulty(), "core:handicap/prince");
    assert_eq!(compile_rules(source()).unwrap().default_difficulty(), "", "no handicaps, no level");
}
