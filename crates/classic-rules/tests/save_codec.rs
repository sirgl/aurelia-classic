//! Bytes of a save round-trip: the world hash and the bytes stay the same.

use classic_rules::save_codec::*;
use classic_rules::ids::PlayerId;
use classic_rules::persistence::{decode::decode, encode::encode};
use classic_rules::replay::hash::authoritative_hash;
use classic_rules::scenario::{generate_player_game, ScenarioConfig};
use classic_rules::{compile_rules, Command};

fn played_game() -> (classic_rules::GameState, classic_rules::CompiledRuleset) {
    let rules = compile_rules(classic_content::core::core_rules()).unwrap();
    let mut game = generate_player_game(&rules, &ScenarioConfig::quick_game(), 0);
    let settler = game.view().auth.indexes().units_of(PlayerId(0))[0];
    game.execute(&rules, Command::FoundCity { settler })
        .unwrap();
    // Three full rounds: every alive player ends its turn in order.
    let players = game.view().auth.players().count();
    for _ in 0..3 * players {
        let player = game.view().auth.active_player();
        game.execute(&rules, Command::EndTurn { player }).unwrap();
    }
    (game, rules)
}

#[test]
fn bytes_round_trip_keeps_the_world_hash() {
    let (game, rules) = played_game();
    let bytes = to_bytes(&encode(&game, &rules));
    let loaded = decode(&from_bytes(&bytes).unwrap(), &rules).unwrap();
    assert_eq!(authoritative_hash(&loaded), authoritative_hash(&game));
    assert_eq!(to_bytes(&encode(&loaded, &rules)), bytes);
}

#[test]
fn river_mouths_keep_the_hash_across_core_save_and_load() {
    // Regression: decode marked water ends of river-mouth edges as river tiles.
    for seed in [1, 7, 20260928] {
        let rules = compile_rules(classic_content::core::core_rules()).unwrap();
        let config = ScenarioConfig {
            seed,
            ..ScenarioConfig::quick_game()
        };
        let game = generate_player_game(&rules, &config, 0);
        let loaded = decode(&encode(&game, &rules), &rules).unwrap();
        assert_eq!(authoritative_hash(&loaded), authoritative_hash(&game));
    }
}

#[test]
fn damaged_bytes_are_errors_not_panics() {
    let (game, rules) = played_game();
    let bytes = to_bytes(&encode(&game, &rules));
    assert_eq!(from_bytes(b"hello").unwrap_err(), CodecError::NotASave);
    for cut in [9, 20, bytes.len() / 2, bytes.len() - 1] {
        assert!(from_bytes(&bytes[..cut]).is_err(), "cut at {cut}");
    }
    let mut extra = bytes.clone();
    extra.push(0);
    assert!(from_bytes(&extra).is_err());
    let mut future = bytes;
    future[8] = 99;
    assert_eq!(
        from_bytes(&future).unwrap_err(),
        CodecError::UnsupportedCodec(99)
    );
}

/// The classic module reads codec versions 19 to 23 (platform/docs/saves.md): a save of version
/// 19 reads, and is written back as version 23 (every unit on no carrier, the game at Standard speed,
/// every religious unit with its full strength), which reads back to the
/// same bytes; other versions are refused.
#[test]
fn the_codec_reads_classic_saves_byte_for_byte() {
    let before: &[u8] = include_bytes!("fixtures/city-names-before-v1.aurelia");
    assert_eq!(u32::from_le_bytes(before[8..12].try_into().unwrap()), 19);
    let upgraded = to_bytes(&from_bytes(before).unwrap());
    assert_eq!(u32::from_le_bytes(upgraded[8..12].try_into().unwrap()), 23);
    assert_eq!(to_bytes(&from_bytes(&upgraded).unwrap()), upgraded);
    for version in [18, 24] {
        let mut other = before.to_vec();
        other[8..12].copy_from_slice(&u32::to_le_bytes(version));
        assert_eq!(from_bytes(&other).unwrap_err(), CodecError::UnsupportedCodec(version));
    }
}

/// The record of a religious unit as codec 20 wrote it (unit, religion,
/// spreads left; spelled out byte by byte) still reads, with no strength
/// lost; codec 21 adds the lost percent.
#[test]
fn a_religious_unit_of_codec_20_reads_with_no_strength_lost() {
    use classic_rules::persistence::religion::SavedReligiousUnit;
    let v20: [u8; 11] = [7, 0, 0, 0, 0, 0, 0, 0, 2, 0, 1];
    let mut reader = Reader::new(&v20, 20);
    let old = <SavedReligiousUnit as Field>::take(&mut reader).unwrap();
    assert_eq!(old, SavedReligiousUnit { unit: 7, religion: 2, spreads_left: 1, strength_lost: 0 });
    assert!(reader.is_at_end());
    let mut v21 = v20.to_vec();
    v21.push(25);
    let mut reader = Reader::new(&v21, 21);
    let new = <SavedReligiousUnit as Field>::take(&mut reader).unwrap();
    assert_eq!(new, SavedReligiousUnit { unit: 7, religion: 2, spreads_left: 1, strength_lost: 25 });
    let mut written = Vec::new();
    new.put(&mut written);
    assert_eq!(written, v21);
}
