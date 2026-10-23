//! Germany (Furor Teutonicus): a land barbarian unit defeated inside its
//! camp joins the victor with a 67 % chance and brings 25 gold. Each trial
//! is a game of its own with a seed of its own; the rule draws its roll
//! only where it applies, so the count over many seeds shows the chance.

use classic_content::core::core_rules;
use classic_rules::ids::{PlayerId, UnitKindId};
use classic_rules::world::player::{ClassicKind, PlayerKind};
use classic_rules::{compile_rules, ClassicGame, Command, CompiledRuleset, GameConfig, GameState};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const BARBARIANS: PlayerId = PlayerId(2);
const TRIALS: u64 = 40;
/// Trials of the cases that must never convert.
const CONTROLS: u64 = 8;

fn unit(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

/// One fight: P0's warrior beats the barbarian warrior at `1 hp` standing on
/// (`camp`) or beside the camp. Returns (units P0 has afterwards, gold gained).
fn fight(civ_key: &str, seed: u64, camp: bool) -> (usize, u32) {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(&rules, GameConfig { players: 3, width: 30, height: 20, rng_seed: seed });
    game.debug_found_city(&rules, P1, game.view().auth.map().dimensions.to_tile(classic_rules::math::HexCoord { q: 26, r: 16 }).unwrap());
    game.debug_set_player_kind(BARBARIANS, PlayerKind::Module(ClassicKind::Barbarian));
    game.debug_set_barbarian_mode(classic_rules::world::barbarians::BarbarianMode::Normal);
    let civ = rules.civs.iter().position(|c| c.key == civ_key).unwrap() as u8;
    game.debug_set_player_civ(P0, civ);
    game.debug_sources_changed(&rules, P0);
    let at = |game: &GameState, q, r| game.view().auth.map().dimensions.to_tile(classic_rules::math::HexCoord { q, r }).unwrap();
    let (spot, from) = (at(&game, 9, 9), at(&game, 8, 9));
    if camp {
        game.debug_place_camp(&rules, spot);
    }
    let warrior = unit(&rules, "core:unit/warrior");
    let attacker = game.debug_spawn_unit(&rules, P0, from, warrior);
    let victim = game.debug_spawn_unit(&rules, BARBARIANS, spot, warrior);
    game.debug_set_unit_hp(victim, 1);
    for _ in 0..8 {
        if game.view().auth.active_player() == P0 {
            break;
        }
        let active = game.view().auth.active_player();
        game.execute(&rules, Command::EndTurn { player: active }).unwrap();
    }
    let gold = game.view().auth.players().treasury_of(P0);
    game.execute(&rules, Command::AttackUnit { attacker, defender: victim }).unwrap();
    assert!(game.view().auth.units().get(victim).is_none(), "the barbarian died");
    game.validate(&rules).unwrap();
    (game.view().auth.indexes().units_of(P0).len(), game.view().auth.players().treasury_of(P0) - gold)
}

/// Germany converts a part of the barbarians it defeats in a camp (and
/// earns 25 gold each time); outside a camp, or for another civilization,
/// none.
#[test]
fn germany_converts_barbarians_defeated_in_their_camp() {
    let converted = (0..TRIALS).filter(|seed| fight("core:nation/germany", *seed, true).0 == 2).count();
    assert!((18..=34).contains(&converted), "about two thirds of {TRIALS}: {converted}");
    let (units, gold) = (0..TRIALS)
        .map(|seed| fight("core:nation/germany", seed, true))
        .find(|(units, _)| *units == 2)
        .unwrap();
    assert_eq!((units, gold), (2, 25), "a unit and 25 gold");
    assert_eq!((0..CONTROLS).filter(|seed| fight("core:nation/america", *seed, true).0 == 2).count(), 0, "only Germany");
    assert_eq!((0..CONTROLS).filter(|seed| fight("core:nation/germany", *seed, false).0 == 2).count(), 0, "only in a camp");
}
