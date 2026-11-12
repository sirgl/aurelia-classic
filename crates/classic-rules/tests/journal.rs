//! The command journal (platform/docs/saves.md §3) on classic games: ten rounds of an AI
//! game recorded after a save replay from that save to the same world, and
//! a journal with a wrong command names the round it diverged in.

use classic_content::core::core_rules;
use classic_rules::ai::{AiDriver, AiSettings};
use classic_rules::persistence::encode::encode;
use classic_rules::replay::authoritative_hash;
use classic_rules::scenario::{generate_new_game, ScenarioConfig};
use classic_rules::{compile_rules, Command, CompiledRuleset, GameState};
use kernel::persistence::codec::{to_bytes, Reader};
use kernel::replay::codec::CommandField;
use kernel::replay::journal::{replay, Divergence, ReplayError};

/// The round the save is made in (cities and armies are out by then).
const SAVED: u32 = 20;
/// The round whose first command the wrong journal replaces.
const WRONG: u32 = 25;
const RECORDED: u32 = 30;

fn scenario() -> ScenarioConfig {
    ScenarioConfig { seed: 0xA1_5EED, players: 4, width: 40, height: 32, cities: 0, units: 0, water_ratio: 15 }
}

/// A recorded game: the save, the journal's header and records, the records
/// cut where round `WRONG` begins, the world there, and the final world.
struct Recorded {
    rules: CompiledRuleset,
    save: Vec<u8>,
    head: Vec<u8>,
    tail: Vec<u8>,
    at_wrong: GameState,
    game: GameState,
}

fn play(game: &mut GameState, rules: &CompiledRuleset, driver: &mut AiDriver, until: u32) {
    while game.view().auth.turn().round < until {
        let report = driver.run(game, rules, u32::MAX);
        assert!(!report.waiting, "an all-AI game never waits");
    }
}

fn recorded() -> Recorded {
    let rules = compile_rules(core_rules()).expect("core rules compile");
    let mut game = generate_new_game(&rules, &scenario());
    let mut driver = AiDriver::new(AiSettings::for_rules(&rules, rules.default_difficulty()), Vec::new());
    play(&mut game, &rules, &mut driver, SAVED);
    let save = to_bytes(&encode(&game, &rules));
    let mut head = game.begin_journal(&rules);
    play(&mut game, &rules, &mut driver, WRONG);
    head.extend(game.take_journal());
    let at_wrong = game.clone();
    play(&mut game, &rules, &mut driver, RECORDED);
    let tail = game.take_journal();
    Recorded { rules, save, head, tail, at_wrong, game }
}

#[test]
fn ten_recorded_rounds_replay_from_the_save_to_the_same_world() {
    let r = recorded();
    let journal = [r.head.clone(), r.tail.clone()].concat();
    let replayed = replay(&r.save, &journal, &r.rules).expect("the journal replays");
    assert_eq!(replayed.rounds, (RECORDED - SAVED) as usize, "every round's hash is checked");
    assert!(replayed.commands > 0 && !replayed.cut);
    assert_eq!(authoritative_hash(&replayed.game), authoritative_hash(&r.game));
    let per_round = journal.len() / replayed.rounds;
    eprintln!("{} commands, {} bytes, {per_round} bytes a round", replayed.commands, journal.len());
}

#[test]
fn a_wrong_command_names_the_round_it_diverged_in() {
    let r = recorded();
    let world = r.at_wrong.view().auth;
    let first = Command::take(&mut Reader::new(&r.tail[1..], 0), world).expect("round 25 starts with a command");
    let active = world.active_player();
    let wrong = Command::EndTurn { player: active };
    assert!(!matches!(first, Command::EndTurn { .. }), "the AI acts before it ends its turn");
    let encoded = |command: &Command| {
        let mut out = Vec::new();
        command.put(world, &mut out);
        out
    };
    let rest = &r.tail[1 + encoded(&first).len()..];
    let journal = [r.head.clone(), vec![0], encoded(&wrong), rest.to_vec()].concat();
    let Err(ReplayError::Diverged(Divergence { round, .. })) = replay(&r.save, &journal, &r.rules) else {
        panic!("a wrong command diverges");
    };
    assert_eq!(round, WRONG);
}

/// A journal written before peace between nations became a deal both accept (command layout 1)
/// may hold a `MakePeace` between two nations that the rules now refuse. It is not played to
/// a silent divergence in the middle: the replay names its layout at once, in the words the
/// command line and a bug report show.
#[test]
fn a_journal_of_the_old_command_layout_is_refused_by_name() {
    use classic_rules::classic::Classic;
    use kernel::replay::codec::CommandCodec;
    use kernel::replay::journal::JournalHeader;

    let r = recorded();
    let journal = [r.head.clone(), r.tail.clone()].concat();
    let (header, _) = JournalHeader::read(&journal).expect("a journal");
    assert_eq!(header.command_codec, Classic::COMMAND_CODEC_VERSION);
    assert!(Classic::COMMAND_CODEC_VERSION >= 2, "version 1 is the layout before the consent rule");
    let old = JournalHeader { command_codec: 1, ..header.clone() };
    let records = &journal[header.to_bytes().len()..];
    let old_journal = [old.to_bytes(), records.to_vec()].concat();
    let refused = replay(&r.save, &old_journal, &r.rules).err().expect("an old journal is refused");
    assert_eq!(refused, ReplayError::OtherCommands { journal: 1, current: Classic::COMMAND_CODEC_VERSION });
    assert_eq!(refused.to_string(), format!("the journal has command layout 1, this build reads {}", Classic::COMMAND_CODEC_VERSION));
    replay(&r.save, &journal, &r.rules).expect("the current layout replays");
}
