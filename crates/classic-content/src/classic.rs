//! The classic ruleset (classic/assets/rules/classic/*.ron), embedded at
//! build time: deterministic, no file system (web and tests). The files
//! are generated from classic/docs/classic by the importer (`classic-import`); each holds
//! some tables of one ruleset.

use classic_rules::rules::source::SourceRules;

use crate::error::ContentError;

macro_rules! classic_files {
    ($($name:literal),* $(,)?) => {
        /// (file name, text) of every ruleset file, in load order.
        pub const FILES: &[(&str, &str)] = &[
            $(($name, include_str!(concat!("../../../assets/rules/classic/", $name)))),*
        ];
    };
}

classic_files!(
    "eras.ron",
    "terrain.ron",
    "features.ron",
    "natural_wonders.ron",
    "techs.ron",
    "units.ron",
    "unique_units.ron",
    "promotions.ron",
    "buildings.ron",
    "unique_buildings.ron",
    "wonders.ron",
    "projects.ron",
    "improvements.ron",
    "resources.ron",
    "policies.ron",
    "beliefs.ron",
    "civilizations.ron",
    "city_states.ron",
    "great_people.ron",
    "handicaps.ron",
    "roles.ron",
    "personalities.ron",
);

/// Parse ruleset files and merge their tables (a parse error names the file).
pub fn parse_files<'a>(
    files: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Result<SourceRules, ContentError> {
    let mut rules = SourceRules::default();
    for (name, text) in files {
        let part = crate::ron::parse_ron(text).map_err(|e| match e {
            ContentError::Parse(msg) => ContentError::Parse(format!("{name}: {msg}")),
            other => other,
        })?;
        rules.merge(part);
    }
    Ok(rules)
}

/// Give content that names no terrains (frozen and fixture rulesets) the
/// classic terrain table and the grounds its terrains play.
pub fn add_classic_terrains(rules: &mut SourceRules) {
    let classic = classic_rules();
    rules.terrains = classic.terrains;
    rules.roles.grounds = classic.roles.grounds;
    rules.roles.deep_water = classic.roles.deep_water;
}

/// The embedded classic ruleset, uncompiled.
pub fn classic_rules() -> SourceRules {
    parse_files(FILES.iter().copied()).expect("embedded BNW ruleset must parse")
}

#[cfg(test)]
mod tests {
    use classic_rules::rules::roles::TerrainPalette;

    /// The classic ruleset keeps the legacy map codes, so test maps painted in legacy codes
    /// mean the same terrain under both.
    #[test]
    fn rules_use_the_legacy_palette() {
        let rules = classic_rules::compile_rules(super::classic_rules()).unwrap();
        assert_eq!(rules.terrain_kinds.palette(), TerrainPalette::LEGACY);
        assert!(rules.roles.deep_water.is_some());
    }

    /// Every role the kernel names has its classic carrier.
    #[test]
    fn every_role_is_bound() {
        use classic_rules::rules::roles::{FeatureRole, ProcessRole, StartingUnit};
        let rules = classic_rules::compile_rules(super::classic_rules()).unwrap();
        let roles = &rules.roles;
        assert!(FeatureRole::ALL.into_iter().all(|r| roles.feature(r).is_some()));
        let unit = |r| rules.units[roles.starting_unit(r).unwrap().0 as usize].key.as_str();
        assert_eq!(unit(StartingUnit::Defense), "core:unit/warrior");
        assert_eq!(unit(StartingUnit::Explore), "core:unit/scout");
        let tech = |t: Option<classic_rules::ids::TechId>| rules.technologies[t.unwrap().0 as usize].key.as_str();
        assert_eq!(tech(roles.road_tech), "core:tech/the-wheel");
        assert_eq!(tech(roles.process_tech(ProcessRole::Research)), "core:tech/education");
        let specialist = roles.default_specialist.unwrap();
        assert_eq!(rules.specialists[specialist.0 as usize].key, "core:specialist/citizen");
        assert_eq!(rules.default_difficulty(), "core:handicap/prince");
        assert_eq!(rules.ai_handicap().unwrap().key, "core:handicap/ai-default");
        use classic_rules::classic::effects::GreatPersonKind;
        use classic_rules::rules::roles::IdeologyCase;
        let branch = |b: Option<classic_rules::rules::content::PolicyBranchId>| {
            rules.module.policy_branches[b.unwrap().0 as usize].key.as_str()
        };
        assert_eq!(branch(roles.module.ai_branches().first().copied()), "core:policy/tradition");
        assert_eq!(branch(roles.module.ai_ideology(IdeologyCase::AtWar)), "core:policy/autocracy");
        assert_eq!(branch(roles.module.faith_branch(GreatPersonKind::Writer)), "core:policy/aesthetics");
        assert_eq!(roles.module.faith_branch(GreatPersonKind::Prophet), None);
        let freedom = roles.module.ai_ideology(IdeologyCase::AtPeace).unwrap();
        assert_eq!(branch(roles.module.ideology_tie(freedom)), "core:policy/order");
    }
}
