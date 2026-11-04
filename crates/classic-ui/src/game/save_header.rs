//! What the list of saves shows of a classic save: the turn, the player's nation and era.
//! They are read from the decoded save (`save_codec::from_bytes`, the schema only): no game is
//! built, no AI memory loaded, the rules are not checked. The player is seat 0, as in every
//! match (`DriverConfig::first_human`).
use classic_rules::save_codec::{from_bytes, CodecError, CODEC_VERSION};
use classic_rules::CompiledRuleset;
use client::shell::module::{SaveProblem, SaveSummary};

use super::names;

/// The summary of the save in `payload` (the bytes after the shell's tag).
pub fn describe(payload: &[u8], rules: &CompiledRuleset) -> Result<SaveSummary, SaveProblem> {
    let save = from_bytes(payload).map_err(problem)?;
    let world = &save.world;
    let era = world
        .player_techs
        .first()
        .into_iter()
        .flatten()
        .filter_map(|tech| rules.technologies.get(usize::from(*tech)))
        .filter_map(|tech| tech.content_era)
        .filter_map(|era| rules.eras.get(usize::from(era.0)))
        .map(|era| era.index)
        .max()
        .unwrap_or(0);
    Ok(SaveSummary {
        // The round is counted from 0; the player sees it from 1 (`Session::turn`).
        turn: Some(world.turn_round + 1),
        nation: Some(names::nation(rules, world.player_nations.first().copied().unwrap_or(0))),
        era: Some(names::era(rules, era)),
    })
}

fn problem(error: CodecError) -> SaveProblem {
    match error {
        CodecError::UnsupportedCodec(version) if version > CODEC_VERSION => SaveProblem::Newer(version),
        CodecError::UnsupportedCodec(version) => SaveProblem::Older(version),
        other => SaveProblem::Damaged(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules() -> std::sync::Arc<CompiledRuleset> {
        crate::ui::screens::game_rules()
    }

    #[test]
    fn a_format_the_build_does_not_read_is_newer_or_older_and_the_rest_is_damaged() {
        assert_eq!(problem(CodecError::UnsupportedCodec(CODEC_VERSION + 1)), SaveProblem::Newer(CODEC_VERSION + 1));
        assert_eq!(problem(CodecError::UnsupportedCodec(3)), SaveProblem::Older(3));
        assert_eq!(problem(CodecError::Truncated), SaveProblem::Damaged("save file is truncated".into()));
        assert_eq!(problem(CodecError::NotASave), SaveProblem::Damaged("not a save file".into()));
    }

    #[test]
    fn a_real_save_gives_its_turn_nation_and_era_without_building_the_game() {
        let bytes = include_bytes!("../../../../crates/classic-rules/tests/fixtures/city-names-before-v1.aurelia");
        let rules = rules();
        let summary = describe(bytes, &rules).expect("the fixture is a classic save");
        let (game, _) = super::super::decode_save(bytes, &rules).unwrap();
        assert_eq!(summary.turn, Some(game.view().auth.turn().round + 1), "the turn the player sees");
        let nation = game.view().auth.players().nation_of(classic_rules::ids::PlayerId(0));
        assert_eq!(summary.nation, Some(names::nation(&rules, nation)));
        let era = game.view().auth.players().nation_era_of(classic_rules::ids::PlayerId(0), &rules);
        assert_eq!(summary.era, Some(names::era(&rules, era)), "the era the game itself reads");
        assert!(summary.nation.as_deref().is_some_and(|n| !n.is_empty()));
        // Cut anywhere after the header, the save is refused, never a panic.
        for cut in [bytes.len() - 1, bytes.len() / 2, 40] {
            assert!(matches!(describe(&bytes[..cut], &rules), Err(SaveProblem::Damaged(_))), "cut at {cut}");
        }
    }

    #[test]
    fn garbage_and_cut_saves_are_refused_without_a_panic() {
        let rules = rules();
        assert!(matches!(describe(b"", &rules), Err(SaveProblem::Damaged(_))));
        assert!(matches!(describe(b"AURELIA\0", &rules), Err(SaveProblem::Damaged(_))));
        let mut newer = b"AURELIA\0".to_vec();
        newer.extend_from_slice(&(CODEC_VERSION + 1).to_le_bytes());
        assert_eq!(describe(&newer, &rules), Err(SaveProblem::Newer(CODEC_VERSION + 1)));
    }
}
