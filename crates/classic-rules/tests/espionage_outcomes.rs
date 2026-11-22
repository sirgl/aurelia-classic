//! What a theft tells (§7.5, Unciv `Spy.stealTech`): a theft nobody noticed tells nothing; a
//! detected one tells the victim that it happened; an identified one also tells who; a caught
//! spy tells both nations. The victim's AI reads the record, and the record survives a save.

use classic_content::core::core_rules;
use classic_rules::classic::ClassicChange;
use classic_rules::ids::{CityHandle, PlayerId, TechId, TileId};
use classic_rules::math::HexCoord;
use classic_rules::replay::authoritative_hash;
use classic_rules::world::espionage::SpyState;
use classic_rules::world::state::GameConfig;
use classic_rules::{compile_rules, ClassicGame, Command, CompiledRuleset, GameState};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
}

fn tech(rules: &CompiledRuleset, key: &str) -> TechId {
    TechId(rules.technologies.iter().position(|t| t.key == key).unwrap() as u16)
}

/// P0 has a spy (the Renaissance) and sends it to the capital of P1.
fn world(seed: u64) -> (GameState, CompiledRuleset, CityHandle) {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(&rules, GameConfig { players: 2, width: 30, height: 20, rng_seed: seed });
    game.debug_found_city(&rules, P0, tile(&game, 3, 3));
    let theirs = game.debug_found_city(&rules, P1, tile(&game, 24, 15));
    game.debug_meet_all();
    game.debug_learn(&rules, P0, tech(&rules, "core:tech/acoustics"));
    game.debug_learn(&rules, P1, tech(&rules, "core:tech/agriculture"));
    (game, rules, theirs)
}

/// One whole round; the module changes of it.
fn end_round(game: &mut GameState, rules: &CompiledRuleset) -> Vec<ClassicChange> {
    let round = game.view().auth.turn().round;
    let mut changes = Vec::new();
    while game.view().auth.turn().round == round {
        let active = game.view().auth.active_player();
        let result = game.execute(rules, Command::EndTurn { player: active }).unwrap();
        changes.extend(result.changes().module_changes.iter().copied());
    }
    changes
}

fn send(game: &mut GameState, rules: &CompiledRuleset, city: CityHandle) {
    while game.view().auth.active_player() != P0 {
        let active = game.view().auth.active_player();
        game.execute(rules, Command::EndTurn { player: active }).unwrap();
    }
    game.execute(rules, Command::MoveSpy { player: P0, spy: 0, city: Some(city), diplomat: false }).unwrap();
}

/// What the first theft of the spy came to.
struct Heist {
    game: GameState,
    rules: CompiledRuleset,
    city: CityHandle,
    /// The theft granted a steal right (the spy was not killed).
    stole: bool,
    spy_dead: bool,
    changes: Vec<ClassicChange>,
}

fn first_heist(seed: u64) -> Option<Heist> {
    let (mut game, rules, city) = world(seed);
    end_round(&mut game, &rules);
    send(&mut game, &rules, city);
    let mut changes = Vec::new();
    for _ in 0..60 {
        changes.extend(end_round(&mut game, &rules));
        let espionage = game.view().auth.module().espionage();
        let stole = espionage.steal_rights(P0, P1) > 0;
        let spy_dead = espionage.spies_of(P0)[0].state == SpyState::Dead;
        if stole || spy_dead {
            return Some(Heist { game, rules, city, stole, spy_dead, changes });
        }
    }
    None
}

fn told(changes: &[ClassicChange]) -> Vec<ClassicChange> {
    changes
        .iter()
        .copied()
        .filter(|c| matches!(c, ClassicChange::TheftDetected { .. } | ClassicChange::SpyKilled { .. }))
        .collect()
}

/// Over many seeds every outcome comes up, and each tells exactly what it should.
#[test]
fn each_outcome_of_a_theft_tells_what_it_should() {
    let (mut quiet, mut detected, mut identified, mut killed) = (0, 0, 0, 0);
    for seed in 0..60 {
        let Some(heist) = first_heist(seed) else { continue };
        let events = told(&heist.changes);
        let record: Vec<_> = heist.game.view().auth.module().espionage().thefts_against(P1).copied().collect();
        match (heist.stole, heist.spy_dead, events.as_slice()) {
            (true, false, []) => {
                quiet += 1;
                assert!(record.is_empty(), "seed {seed}: an unnoticed theft leaves no record");
            }
            (true, false, [ClassicChange::TheftDetected { thief: P0, victim: P1, city, identified: false }]) => {
                detected += 1;
                assert_eq!(*city, heist.city);
                assert_eq!(record.len(), 1, "seed {seed}");
                assert_eq!(record[0].thief, None, "a detected theft does not name the thief");
            }
            (true, false, [ClassicChange::TheftDetected { thief: P0, victim: P1, city, identified: true }]) => {
                identified += 1;
                assert_eq!(*city, heist.city);
                assert_eq!(record.len(), 1, "seed {seed}");
                assert_eq!(record[0].thief, Some(P0), "an identified theft names the thief");
            }
            (false, true, [ClassicChange::SpyKilled { thief: P0, victim: P1, city }]) => {
                killed += 1;
                assert_eq!(*city, heist.city);
                assert!(record.is_empty(), "seed {seed}: a caught spy stole nothing");
            }
            other => panic!("seed {seed}: the outcome and what it told disagree: {other:?}"),
        }
    }
    assert!(quiet > 0 && detected > 0 && identified > 0 && killed > 0, "every outcome came up: {quiet} {detected} {identified} {killed}");
}

/// The victim's AI sees the thefts from it, and the record is saved and loaded.
#[test]
fn the_victims_ai_observes_a_found_out_theft_and_the_save_keeps_it() {
    let heist = (0..60)
        .filter_map(first_heist)
        .find(|h| h.game.view().auth.module().espionage().thefts_against(P1).next().is_some())
        .expect("some seed has a theft that is found out");
    let obs = heist.game.ai_observation(&heist.rules, P1);
    assert_eq!(obs.thefts_against_us.len(), 1);
    assert_eq!(obs.thefts_against_us[0].city, heist.city);
    assert!(heist.game.ai_observation(&heist.rules, P0).thefts_against_us.is_empty(), "the thief is not the victim");
    let save = classic_rules::persistence::encode::encode(&heist.game, &heist.rules);
    let loaded = classic_rules::persistence::decode::decode(&save, &heist.rules).unwrap();
    assert_eq!(authoritative_hash(&loaded), authoritative_hash(&heist.game));
    assert_eq!(loaded.view().auth.module().espionage(), heist.game.view().auth.module().espionage());
}

/// A save of the released game (codec 25) has no list of thefts at the end of its espionage
/// section and no list of grievances in its relations section: it loads, and the record is empty.
/// The codec-25 bytes are made from a codec-26 save of a game with one theft by cutting both lists
/// out and stamping version 25, which is exactly what the released writer produced.
#[test]
fn a_codec_25_save_loads_with_no_thefts() {
    use classic_rules::save_codec::{from_bytes, to_bytes, Field};
    let heist = (0..60)
        .filter_map(first_heist)
        .find(|h| h.game.view().auth.module().espionage().thefts_against(P1).next().is_some())
        .expect("some seed has a theft that is found out");
    let save = classic_rules::persistence::encode::encode(&heist.game, &heist.rules);
    let bytes = to_bytes(&save);
    let mut thefts = Vec::new();
    save.world.module.espionage.thefts.put(&mut thefts);
    let thefts_at = only_place(&bytes, &thefts, "the theft list");
    // The grievance list may be empty (a few bytes found anywhere): it is found in its section,
    // before the three fields that end the relations record.
    let relations = &save.world.module.relations;
    let mut section = Vec::new();
    relations.put(&mut section);
    let (mut grievances, mut tail) = (Vec::new(), Vec::new());
    relations.grievances.put(&mut grievances);
    relations.trades.put(&mut tail);
    relations.offers.put(&mut tail);
    relations.next_offer.put(&mut tail);
    let grievances_at = only_place(&bytes, &section, "the relations section") + section.len() - tail.len() - grievances.len();
    assert_eq!(bytes[grievances_at..grievances_at + grievances.len()], grievances[..]);
    assert!(grievances_at < thefts_at, "the relations section comes before the espionage section");
    let mut released = bytes[..grievances_at].to_vec();
    released.extend_from_slice(&bytes[grievances_at + grievances.len()..thefts_at]);
    released.extend_from_slice(&bytes[thefts_at + thefts.len()..]);
    released[8..12].copy_from_slice(&25u32.to_le_bytes());
    let loaded = classic_rules::persistence::decode::decode(&from_bytes(&released).expect("codec 25 reads"), &heist.rules)
        .expect("the codec-25 save loads");
    assert_eq!(loaded.view().auth.module().espionage().all_thefts(), [], "no theft is remembered");
    assert_eq!(loaded.view().auth.module().espionage().all_steal_rights(), heist.game.view().auth.module().espionage().all_steal_rights());
}

/// Where `part` is in `bytes`; it must be there once.
fn only_place(bytes: &[u8], part: &[u8], what: &str) -> usize {
    let at = bytes.windows(part.len()).position(|w| w == part).unwrap_or_else(|| panic!("{what} is in the save"));
    assert_eq!(bytes.windows(part.len()).filter(|w| *w == part).count(), 1, "{what} is in the save only once");
    at
}
