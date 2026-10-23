//! Classic catalog importer: classic/docs/classic/*.json → classic/assets/rules/classic/*.ron, then
//! the written files are parsed and compiled like the game does, and
//! classic/docs/classic/implemented.md reports what compiled and which effects are
//! inert (the M4 work list).
//!
//! Deterministic: records sorted by key, fields in schema order, no clock,
//! no hash-map iteration in the output. Regenerate with
//! `cargo run --release --offline -p classic-content --features importer --bin classic-import`.

mod catalog;
mod city_states;
mod effects;
mod espionage;
mod military;
mod personalities;
mod production;
mod report;
mod roles;
mod ron_out;
mod society;
mod tables;

use std::path::{Path, PathBuf};

use catalog::Catalogs;
use ron_out::{file, record_file, Record};

/// What one run wrote.
pub struct ImportSummary {
    pub files: Vec<PathBuf>,
    pub report: PathBuf,
}

const HEADER: &str = "Civ 5 inspired rule set (Unciv Gods & Kings ruleset plus own extensions); generated, do not edit.
Derived from Unciv (https://github.com/yairm210/Unciv), MPL-2.0, where the entry says `unciv:`
in classic/docs/classic/*.json (see classic/docs/classic/UNCIV-ATTRIBUTION.md); other values are own, unverified.
Source: classic/docs/classic/*.json (made by classic/tools/import-unciv.py). Regenerate:
  cargo run --release --offline -p classic-content --features importer --bin classic-import
Inert effects need a mechanic the engine does not have yet: classic/docs/classic/implemented.md.";

const PERSONALITY_HEADER: &str = "Leader personalities of the Civ V AI (generated, do not edit): this
project's own judgement from public descriptions of the leaders and their
civilizations' abilities (docs/ai-leaders.md), not the game's files.
0 to 10, 5 neutral. Source: crates/classic-content/src/import/personalities.rs. Regenerate:
  cargo run --release --offline -p classic-content --features importer --bin classic-import";

/// A ruleset file: its name and its named tables.
type RulesetFile = (&'static str, Vec<(&'static str, Vec<Record>)>);

/// Every ruleset file and its tables (`curated`: the hand-written city and spy
/// names of each civilization, `tables::curated_names`).
fn ruleset_files(
    cat: &Catalogs,
    legacy: &[(String, (u8, u8, u8))],
    curated: &tables::CuratedNames,
) -> Vec<RulesetFile> {
    vec![
        ("eras.ron", vec![("eras", tables::eras(cat))]),
        ("terrain.ron", vec![("terrains", tables::terrains(cat))]),
        ("features.ron", vec![("features", tables::features(cat))]),
        (
            "natural_wonders.ron",
            vec![("natural_wonders", tables::natural_wonders(cat))],
        ),
        (
            "techs.ron",
            vec![("technologies", tables::technologies(cat))],
        ),
        ("units.ron", vec![("units", tables::units(cat))]),
        (
            "unique_units.ron",
            vec![("units", tables::unique_units(cat))],
        ),
        (
            "promotions.ron",
            vec![("promotions", tables::promotions(cat))],
        ),
        ("buildings.ron", vec![("buildings", tables::buildings(cat))]),
        (
            "unique_buildings.ron",
            vec![("buildings", tables::unique_buildings(cat))],
        ),
        ("wonders.ron", vec![("buildings", tables::wonders(cat))]),
        ("projects.ron", vec![("projects", tables::projects(cat))]),
        (
            "improvements.ron",
            vec![("improvements", tables::improvements(cat))],
        ),
        ("resources.ron", vec![("resources", tables::resources(cat))]),
        (
            "policies.ron",
            vec![
                ("policy_branches", tables::policy_branches(cat)),
                ("policies", tables::policies(cat)),
            ],
        ),
        (
            "beliefs.ron",
            vec![
                ("religions", tables::religions(cat)),
                ("beliefs", tables::beliefs(cat)),
            ],
        ),
        (
            "civilizations.ron",
            vec![("civs", tables::civilizations(cat, legacy, curated))],
        ),
        (
            "city_states.ron",
            vec![
                ("city_state_types", tables::city_state_types(cat)),
                ("city_states", tables::city_states(cat)),
            ],
        ),
        (
            "great_people.ron",
            vec![
                ("specialists", tables::specialists(cat)),
                ("great_people", tables::great_people(cat)),
            ],
        ),
    ]
}

/// Refuse to write through a symlink (worktrees may link assets to main).
fn check_not_symlink(path: &Path) -> Result<(), String> {
    let mut current = PathBuf::new();
    for part in path.components() {
        current.push(part);
        if let Ok(meta) = std::fs::symlink_metadata(&current) {
            if meta.file_type().is_symlink() {
                return Err(format!(
                    "{} is a symlink: refusing to write through it",
                    current.display()
                ));
            }
        }
    }
    Ok(())
}

/// Run the importer on the repository at `root`.
pub fn run(root: &Path) -> Result<ImportSummary, String> {
    let cat = Catalogs::load(&root.join("docs/classic"))?;
    let legacy: Vec<(String, (u8, u8, u8))> = crate::legacy::legacy_civs()
        .into_iter()
        .map(|c| (c.id.map(|k| k.0).unwrap_or_default(), c.color))
        .collect();
    let out_dir = root.join("assets/rules/classic");
    check_not_symlink(&root.join("assets/rules"))?;
    std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
    check_not_symlink(&out_dir)?;
    let written = std::fs::read_to_string(out_dir.join("civilizations.ron")).unwrap_or_default();
    let curated = tables::curated_names(&written);
    let mut texts = Vec::new();
    let mut files = Vec::new();
    for (name, tables) in ruleset_files(&cat, &legacy, &curated) {
        let refs: Vec<(&str, &[Record])> = tables.iter().map(|(t, r)| (*t, r.as_slice())).collect();
        let text = file(HEADER, &refs);
        let path = out_dir.join(name);
        std::fs::write(&path, &text).map_err(|e| format!("{}: {e}", path.display()))?;
        texts.push((name, text));
        files.push(path);
    }
    let path = out_dir.join("roles.ron");
    let text = record_file(HEADER, "roles", &roles::roles());
    std::fs::write(&path, &text).map_err(|e| format!("{}: {e}", path.display()))?;
    texts.push(("roles.ron", text));
    files.push(path);
    let path = out_dir.join("personalities.ron");
    let text = file(PERSONALITY_HEADER, &[("personalities", personalities::personalities().as_slice())]);
    std::fs::write(&path, &text).map_err(|e| format!("{}: {e}", path.display()))?;
    texts.push(("personalities.ron", text));
    files.push(path);
    let source = crate::classic::parse_files(texts.iter().map(|(n, t)| (*n, t.as_str())))
        .map_err(|e| format!("generated RON does not parse: {e}"))?;
    let rules = classic_rules::compile_rules(source)
        .map_err(|e| format!("generated ruleset does not compile: {e}"))?;
    let report_path = root.join("docs/classic/implemented.md");
    check_not_symlink(&report_path)?;
    std::fs::write(&report_path, report::render(&cat, &rules)).map_err(|e| e.to_string())?;
    Ok(ImportSummary {
        files,
        report: report_path,
    })
}
