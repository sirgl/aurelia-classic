//! The ruleset hash is a content fingerprint: same content → same hash
//! (whatever the source order), any changed number → another hash, and a
//! save written under other rules does not load.

use crate::rules::{compile_rules, CompiledRuleset};

const CONTENT: &str = r#"(
    units: [
        (id: "core:unit/settler", movement: 2, strength: 1, founder: true, cost: 40),
        (id: "core:unit/warrior", movement: 2, strength: 10, cost: 30),
    ],
    buildings: [
        (id: "core:building/granary", cost: 60, effects: [(scope: City, selector: SourceCity, modifier: AddYield(yield_kind: Output, amount: 2))], required_tech: Some("core:tech/pottery")),
        (id: "core:building/monument", cost: 50),
    ],
    technologies: [
        (id: "core:tech/agriculture", cost: 20),
        (id: "core:tech/pottery", unlocks_buildings: ["core:building/granary"], cost: 40, requires: ["core:tech/agriculture"]),
    ],
    improvements: [(id: "core:improvement/farm", work_turns: 3, food_bonus: 2)],
)"#;

fn compiled(text: &str) -> CompiledRuleset {
    compile_rules(crate::fixtures::parse_ron_text(text)).unwrap()
}

/// CONTENT with one substring replaced (the substring must exist).
fn changed(from: &str, to: &str) -> u64 {
    assert!(CONTENT.contains(from), "fixture lacks {from}");
    compiled(&CONTENT.replacen(from, to, 1)).hash()
}

#[test]
fn same_content_same_hash_regardless_of_source_order() {
    let base = compiled(CONTENT).hash();
    assert_eq!(base, compiled(CONTENT).hash());
    let swapped = CONTENT.replacen(
        r#"(id: "core:unit/settler", movement: 2, strength: 1, founder: true, cost: 40),
        (id: "core:unit/warrior", movement: 2, strength: 10, cost: 30),"#,
        r#"(id: "core:unit/warrior", movement: 2, strength: 10, cost: 30),
        (id: "core:unit/settler", movement: 2, strength: 1, founder: true, cost: 40),"#,
        1,
    );
    assert_ne!(swapped, CONTENT);
    assert_eq!(
        base,
        compiled(&swapped).hash(),
        "source order is not content"
    );
}

#[test]
fn changing_one_number_changes_the_hash() {
    let base = compiled(CONTENT).hash();
    let variants = [
        ("cost: 30", "cost: 31"),                   // unit cost
        ("strength: 10", "strength: 11"),           // unit strength
        ("cost: 60", "cost: 61"),                   // building cost
        ("amount: 2))]", "amount: 3))]"),           // effect amount
        ("yield_kind: Output", "yield_kind: Food"), // effect target
        ("cost: 20", "cost: 21"),                   // tech cost
        ("food_bonus: 2", "food_bonus: 3"),         // improvement bonus
        ("work_turns: 3", "work_turns: 4"),         // improvement turns
    ];
    for (from, to) in variants {
        assert_ne!(
            changed(from, to),
            base,
            "{from} -> {to} must change the hash"
        );
    }
}

#[test]
fn table_sizes_alone_do_not_define_the_hash() {
    // The v1 hash summed table sizes: two different buildings with the same
    // count collided. A renamed key is different content.
    let base = compiled(CONTENT).hash();
    let renamed = changed("core:building/monument", "core:building/obelisk");
    assert_ne!(renamed, base);
}

#[test]
fn save_from_other_rules_is_rejected() {
    use crate::persistence::{decode::decode, encode::encode, DecodeError};
    let rules = compiled(CONTENT);
    let config = crate::world::state::GameConfig {
        players: 2,
        width: 8,
        height: 8,
        rng_seed: 1,
    };
    let game = crate::GameState::new(&rules, config);
    let save = encode(&game, &rules);
    assert_eq!(save.header.ruleset_hash, rules.hash());
    assert!(decode(&save, &rules).is_ok());
    let other = compiled(&CONTENT.replacen("cost: 60", "cost: 61", 1));
    assert!(matches!(
        decode(&save, &other),
        Err(DecodeError::RulesetMismatch)
    ));
}
