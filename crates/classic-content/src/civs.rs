//! Civilizations of the default rule set (43 civilizations from
//! classic/assets/rules/classic/civilizations.ron): identity is name, leader, colour,
//! start preferences, unique abilities, units and buildings.
//!
//! The selection menu takes them from here; core_rules() contains the same records.

use classic_rules::rules::source::SourceCiv;

/// The set of civilizations: selection menu, HUD labels, start biases.
pub fn civs() -> Vec<SourceCiv> {
    let text = crate::classic::FILES
        .iter()
        .find(|(name, _)| *name == "civilizations.ron")
        .map(|(_, text)| *text)
        .expect("BNW ruleset has civilizations.ron");
    crate::ron::parse_ron(text)
        .expect("civilizations.ron must parse")
        .civs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civs_parse_with_unique_keys_and_names() {
        let civs = civs();
        assert_eq!(civs.len(), 43, "BNW has 43 civilizations");
        let mut keys: Vec<&str> = civs
            .iter()
            .map(|c| c.id.as_ref().expect("civ id").0.as_str())
            .collect();
        keys.sort();
        let before = keys.len();
        keys.dedup();
        assert_eq!(keys.len(), before, "civ keys must be unique");
        for c in &civs {
            assert!(!c.name.trim().is_empty(), "civ name empty");
            assert!(!c.leader.trim().is_empty(), "leader name empty");
        }
        let rome = civs
            .iter()
            .find(|c| c.id.as_ref().unwrap().0 == "core:nation/rome")
            .unwrap();
        assert_eq!(rome.leader, "Augustus Caesar", "BNW leader of Rome");
    }

    #[test]
    fn unknown_start_bias_fails_loudly() {
        let bad = "civs: [(id: \"core:nation/x\", name: \"X\", leader: \"Y\", \
                    color: (r: 1, g: 2, b: 3), start_bias: \"jungle\")]";
        assert!(crate::ron::parse_civs(bad).is_err());
    }

    #[test]
    fn civilizations_file_on_disk_matches_embedded_text() {
        let text = std::fs::read_to_string("../../assets/rules/classic/civilizations.ron")
            .expect("assets/rules/classic/civilizations.ron must exist");
        let embedded = crate::classic::FILES
            .iter()
            .find(|(name, _)| *name == "civilizations.ron")
            .unwrap()
            .1;
        assert_eq!(
            text, embedded,
            "civilizations.ron on disk drifted from embedded"
        );
    }
}
