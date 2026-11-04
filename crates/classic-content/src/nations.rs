//! Nations of the default rule set (43 nations from
//! classic/assets/rules/classic/nations.ron): identity is name, leader, colour,
//! start preferences, unique abilities, units and buildings.
//!
//! The selection menu takes them from here; core_rules() contains the same records.

use classic_rules::rules::source::SourceNation;

/// The set of nations: selection menu, HUD labels, start biases.
pub fn nations() -> Vec<SourceNation> {
    let text = crate::classic::FILES
        .iter()
        .find(|(name, _)| *name == "nations.ron")
        .map(|(_, text)| *text)
        .expect("BNW ruleset has nations.ron");
    crate::ron::parse_ron(text)
        .expect("nations.ron must parse")
        .nations
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nations_parse_with_unique_keys_and_names() {
        let nations = nations();
        assert_eq!(nations.len(), 43, "BNW has 43 nations");
        let mut keys: Vec<&str> = nations
            .iter()
            .map(|c| c.id.as_ref().expect("nation id").0.as_str())
            .collect();
        keys.sort();
        let before = keys.len();
        keys.dedup();
        assert_eq!(keys.len(), before, "nation keys must be unique");
        for c in &nations {
            assert!(!c.name.trim().is_empty(), "nation name empty");
            assert!(!c.leader.trim().is_empty(), "leader name empty");
        }
        let rome = nations
            .iter()
            .find(|c| c.id.as_ref().unwrap().0 == "core:nation/rome")
            .unwrap();
        assert_eq!(rome.leader, "Augustus Caesar", "BNW leader of Rome");
    }

    #[test]
    fn unknown_start_bias_fails_loudly() {
        let bad = "nations: [(id: \"core:nation/x\", name: \"X\", leader: \"Y\", \
                    color: (r: 1, g: 2, b: 3), start_bias: \"jungle\")]";
        assert!(crate::ron::parse_nations(bad).is_err());
    }

    #[test]
    fn nations_file_on_disk_matches_embedded_text() {
        let text = std::fs::read_to_string("../../assets/rules/classic/nations.ron")
            .expect("assets/rules/classic/nations.ron must exist");
        let embedded = crate::classic::FILES
            .iter()
            .find(|(name, _)| *name == "nations.ron")
            .unwrap()
            .1;
        assert_eq!(
            text, embedded,
            "nations.ron on disk drifted from embedded"
        );
    }
}
