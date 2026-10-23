//! Content loader from disk (§28/§29): manifest -> RON files -> SourceRules.
//! Semantics are in classic-rules::rules::compile; here only IO + parsing.
//! The built-in classic set goes through `crate::classic` (without the FS); this module is
//! for MODS and overrides (the path from the manifest).

use crate::error::ContentError;
use classic_rules::rules::SourceRules;

/// Read a RON file by path and parse it (mods, content hot reload).
pub fn load_source_rules(path: &str) -> Result<SourceRules, ContentError> {
    let text = std::fs::read_to_string(path).map_err(|e| ContentError::Io(e.to_string()))?;
    crate::ron::parse_ron(&text)
}

/// The built-in default rule set (classic), the same as core_rules().
pub fn load_core_rules() -> SourceRules {
    crate::core::core_rules()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classic_files_on_disk_match_the_embedded_ruleset() {
        // The same files as embedded: file/binary drift is caught here.
        for (name, embedded) in crate::classic::FILES {
            let path = format!("../../assets/rules/classic/{name}");
            let from_disk = load_source_rules(&path).unwrap();
            let parsed = crate::ron::parse_ron(embedded).unwrap();
            assert_eq!(from_disk.units.len(), parsed.units.len(), "{name}");
            assert_eq!(std::fs::read_to_string(&path).unwrap(), *embedded, "{name}");
        }
        let compiled = classic_rules::rules::compile_rules(load_core_rules()).unwrap();
        assert_eq!(compiled.resources.len(), 42);
        assert!(compiled
            .resources
            .iter()
            .any(|r| r.key == "core:resource/gold"));
    }

    #[test]
    fn missing_file_errors_loudly() {
        assert!(matches!(
            load_source_rules("assets/rules/does-not-exist.ron"),
            Err(ContentError::Io(_))
        ));
    }
}
