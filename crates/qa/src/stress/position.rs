//! Positions: a recorded game replayed to a round (`ai-play --stop-round`, a log of the first rounds
//! of a whole game). A position's round is drawn from its index, so every situation of one instance
//! starts from the same world.

use std::path::{Path, PathBuf};

use classic_rules::ai::variant::Variant;
use classic_rules::{CompiledRuleset, GameState};

use crate::bench::modules::majors;
use crate::export::{read_log, start};

/// A recorded game at the start of a round.
pub struct Position {
    /// The log's file stem.
    pub name: String,
    pub log: PathBuf,
    pub round: u32,
    pub game: GameState,
    /// The AI variant that played the log, and plays on.
    pub variant: Variant,
}

/// The logs of `dir` (`*.aurlog` with a `.json` beside), sorted by name.
pub fn logs(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(|e| Some(e.ok()?.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "aurlog") && p.with_extension("json").exists())
        .collect();
    paths.sort();
    if paths.is_empty() {
        return Err(format!("no logs in {}", dir.display()));
    }
    Ok(paths)
}

/// The round of instance `index`: `base` plus a draw of `0..=jitter` from the index.
pub fn round_of(index: usize, base: u32, jitter: u32) -> u32 {
    base + (mix(index as u64 ^ 0x57_2e55) % u64::from(jitter + 1)) as u32
}

/// The log replayed to the start of `round`.
pub fn load(rules: &CompiledRuleset, log: &Path, round: u32) -> Result<Position, String> {
    let (spec, _) = read_log(log)?;
    let variant = Variant::from_name(&spec.variant).ok_or_else(|| format!("{}: no AI variant {}", log.display(), spec.variant))?;
    let mut game = start(rules, &spec.game()?);
    let nations = majors(&game);
    crate::selfplay::replay_to(rules, &mut game, log, round, &nations, &mut vec![None; nations.len()], false)?;
    let name = log.file_stem().map_or_else(String::new, |s| s.to_string_lossy().into_owned());
    Ok(Position { name, log: log.to_path_buf(), round, game, variant })
}

/// A splitmix64 step: the draws of an instance.
pub fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}
