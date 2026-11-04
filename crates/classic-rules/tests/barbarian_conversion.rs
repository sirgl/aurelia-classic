//! Germany (Furor Teutonicus): a land barbarian unit defeated inside its
//! camp joins the victor with a 67 % chance and brings 25 gold. Each trial
//! is a game of its own with a seed of its own; the rule draws its roll
//! only where it applies, so the count over many seeds shows the chance.

use classic_content::core::core_rules;
use classic_rules::classic::ClassicChange;
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
fn fight(nation_key: &str, seed: u64, camp: bool) -> (usize, u32) {
    let (units, gold, _) = fight_journal(nation_key, seed, camp);
    (units, gold)
}

/// `fight` and the module changes of the attack.
fn fight_journal(nation_key: &str, seed: u64, camp: bool) -> (usize, u32, Vec<ClassicChange>) {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(&rules, GameConfig { players: 3, width: 30, height: 20, rng_seed: seed });
    game.debug_found_city(&rules, P1, game.view().auth.map().dimensions.to_tile(classic_rules::math::HexCoord { q: 26, r: 16 }).unwrap());
    game.debug_set_player_kind(BARBARIANS, PlayerKind::Module(ClassicKind::Barbarian));
    game.debug_set_barbarian_mode(classic_rules::world::barbarians::BarbarianMode::Normal);
    let nation = rules.nations.iter().position(|c| c.key == nation_key).unwrap() as u8;
    game.debug_set_player_nation(P0, nation);
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
    let result = game.execute(&rules, Command::AttackUnit { attacker, defender: victim }).unwrap();
    assert!(game.view().auth.units().get(victim).is_none(), "the barbarian died");
    game.validate(&rules).unwrap();
    let units = game.view().auth.indexes().units_of(P0).len();
    (units, game.view().auth.players().treasury_of(P0) - gold, result.changes().module_changes.clone())
}

/// Germany converts a part of the barbarians it defeats in a camp (and
/// earns 25 gold each time); outside a camp, or for another nation,
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

/// The journal names what a kill in a camp gave: the unit that joined brings its 25 gold, and the
/// camp is not cleared by that attack (the new unit stands on the tile, so the attacker does not
/// move in); a kill after which nobody joins clears the camp for its own gold.
#[test]
fn the_journal_names_the_conversion_and_the_camp_gold() {
    let at = |units: usize| (0..TRIALS).map(|seed| fight_journal("core:nation/germany", seed, true)).find(|(n, ..)| *n == units).unwrap();
    let (_, gold, changes) = at(2);
    let joined: Vec<u32> = changes
        .iter()
        .filter_map(|c| match c {
            ClassicChange::CampUnitConverted { player, gold, .. } if *player == P0 => Some(*gold),
            _ => None,
        })
        .collect();
    assert_eq!(joined, [25], "the unit that joined brought its gold");
    assert_eq!(joined[0], gold, "and that is the treasury's gain");
    assert!(!changes.iter().any(|c| matches!(c, ClassicChange::CampCleared { .. })), "the camp stands: nobody walked in");
    let (_, gold, changes) = at(1);
    assert!(!changes.iter().any(|c| matches!(c, ClassicChange::CampUnitConverted { .. })), "nobody joined");
    assert!(changes.iter().any(|c| matches!(c, ClassicChange::CampCleared { player, gold: g, .. } if *player == P0 && *g == gold)), "the camp gold alone");
}
