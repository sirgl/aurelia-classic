//! Frozen pre-classic content: `classic/assets/rules/core.ron` and `nations.ron` as they
//! were at the M3.0 merge (values written before the classic catalogs existed).
//!
//! Test fixture only. `classic-rules/tests/baseline_hashes.rs` pins world hashes
//! recorded on this content, and the M3.0 effect tests check its numbers.
//! The game itself loads the classic ruleset (`crate::classic`). Never edit these
//! files: a change here silently re-pins the behaviour baseline.

use classic_rules::rules::source::SourceRules;

/// Frozen `core.ron` text.
pub const CORE_RON: &str = include_str!("../fixtures/legacy/core.ron");
/// Frozen `nations.ron` text.
pub const CIVS_RON: &str = include_str!("../fixtures/legacy/nations.ron");

/// The frozen ruleset (core + nations), uncompiled. The frozen files
/// have no terrain table (v1 terrain yields were a code table, removed with
/// the classic tile yields): the classic terrain table and its grounds are added,
/// so the legacy games work their tiles by the classic terrain values.
pub fn legacy_rules() -> SourceRules {
    let mut rules = crate::ron::parse_ron(CORE_RON).expect("legacy core.ron must parse");
    rules.nations = legacy_nations();
    add_legacy_world(&mut rules);
    rules
}

/// What the frozen files lack for today's kernel: the classic terrain table
/// with its grounds, and the roles of the rows they name (the forest the
/// generator plants, the warrior and worker players start with).
pub fn add_legacy_world(rules: &mut SourceRules) {
    use classic_rules::rules::roles::{FeatureRole, StartingUnit};
    use classic_rules::rules::StableContentKey;
    crate::classic::add_classic_terrains(rules);
    rules.roles.features = vec![(FeatureRole::Forest, StableContentKey::new("core:feature/forest"))];
    rules.roles.starting_units = vec![
        (StartingUnit::Defense, StableContentKey::new("core:unit/warrior")),
        (StartingUnit::Worker, StableContentKey::new("core:unit/worker")),
    ];
}

/// The frozen nations.
pub fn legacy_nations() -> Vec<classic_rules::rules::source::SourceNation> {
    crate::ron::parse_nations(CIVS_RON).expect("legacy nations.ron must parse")
}
