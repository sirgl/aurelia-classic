//! Extra attacks and moves after an attack (Unciv `Battle.kt`): after an attack a unit keeps its
//! movement when it can move after attacking (`MoveAfterAttacking`) or has attacks left this turn
//! (`AttacksPerTurn`); otherwise the turn's moves end. One test per ability, one over every
//! promotion that grants extra attacks, and the shipped Chu-Ko-Nu.

use classic_content::core::core_rules;
use classic_rules::ids::{PlayerId, PromotionId, TileId, UnitHandle, UnitKindId};
use classic_rules::math::HexCoord;
use classic_rules::rules::effects::{Modifier, UnitAbility, UnitStat};
use classic_rules::world::state::GameConfig;
use classic_rules::{compile_rules, ClassicGame, Command, CompiledRuleset, GameState};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

/// Which of the two abilities the rules give to the promotions that carry extra attacks.
#[derive(Clone, Copy)]
enum Data {
    /// As shipped.
    Shipped,
    /// Extra attacks without "move after attacking" (the ability alone, as Unciv's own data
    /// can combine them).
    AttacksOnly,
}

fn rules(data: Data) -> CompiledRuleset {
    let mut rules = compile_rules(core_rules()).expect("the ruleset compiles");
    if let Data::AttacksOnly = data {
        let move_after = Modifier::GrantAbility(UnitAbility::MoveAfterAttacking);
        for promotion in &mut rules.promotions {
            if promotion.effects.passive_unit.iter().any(|e| e.modifier == Modifier::ModifyUnitStat { stat: UnitStat::AttacksPerTurn, amount: 1 }) {
                promotion.effects.passive_unit.retain(|e| e.modifier != move_after);
            }
        }
    }
    rules
}

fn game(rules: &CompiledRuleset) -> GameState {
    let mut game = GameState::new(rules, GameConfig { players: 2, width: 16, height: 16, rng_seed: 7 });
    game.debug_meet(P0, P1);
    game.execute(rules, Command::DeclareWar { player: P0, enemy: P1 }).expect("war");
    game
}

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).expect("on the map")
}

fn kind(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

fn promotion(rules: &CompiledRuleset, key: &str) -> PromotionId {
    PromotionId(rules.promotions.iter().position(|p| p.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

fn moves_left(game: &GameState, unit: UnitHandle) -> u32 {
    u32::from(game.view().auth.units().get(unit).expect("alive").core.moves_left)
}

/// An archer at (4,4) with two enemy warriors in range 2; the archer carries `promotions`.
fn shooter(rules: &CompiledRuleset, game: &mut GameState, key: &str, promotions: &[&str]) -> (UnitHandle, [TileId; 2]) {
    let unit = game.debug_spawn_unit(rules, P0, tile(game, 4, 4), kind(rules, key));
    for key in promotions {
        game.debug_grant_promotion(rules, unit, promotion(rules, key));
    }
    let targets = [tile(game, 6, 4), tile(game, 4, 6)];
    for at in targets {
        game.debug_spawn_unit(rules, P1, at, kind(rules, "core:unit/warrior"));
    }
    (unit, targets)
}

fn shoot(game: &mut GameState, rules: &CompiledRuleset, unit: UnitHandle, target: TileId) -> bool {
    game.execute(rules, Command::RangedAttack { unit, target }).is_ok()
}

/// `AttacksPerTurn` alone: the first shot keeps the movement, the second comes, a third does not.
#[test]
fn extra_attacks_without_move_after_attacking_attack_twice() {
    let rules = rules(Data::AttacksOnly);
    let mut game = game(&rules);
    let (unit, [a, b]) = shooter(&rules, &mut game, "core:unit/archer", &["core:promotion/second-attack"]);
    assert!(shoot(&mut game, &rules, unit, a), "the first attack");
    assert!(moves_left(&game, unit) > 0, "an attack is left, so the moves stay");
    assert!(shoot(&mut game, &rules, unit, b), "the second attack of the turn");
    assert_eq!(moves_left(&game, unit), 0, "no attack left, no move after attacking: the turn ends");
    assert!(!shoot(&mut game, &rules, unit, a), "two attacks a turn, not three");
}

/// `MoveAfterAttacking` alone: one attack, and the unit still moves after it.
#[test]
fn move_after_attacking_keeps_the_moves_but_gives_no_second_attack() {
    let rules = rules(Data::Shipped);
    let mut game = game(&rules);
    let (unit, [a, b]) = shooter(&rules, &mut game, "core:unit/archer", &["core:promotion/can-move-after-attacking"]);
    assert!(shoot(&mut game, &rules, unit, a));
    assert!(moves_left(&game, unit) > 0, "it can move after attacking");
    assert!(!shoot(&mut game, &rules, unit, b), "but it has one attack a turn");
}

/// Neither ability: the first attack ends the turn's moves.
#[test]
fn without_either_ability_an_attack_ends_the_moves() {
    let rules = rules(Data::Shipped);
    let mut game = game(&rules);
    let (unit, [a, _]) = shooter(&rules, &mut game, "core:unit/archer", &[]);
    assert!(shoot(&mut game, &rules, unit, a));
    assert_eq!(moves_left(&game, unit), 0);
}

/// Every promotion that grants an extra attack, on a unit that can take it, gives two attacks
/// with and without "move after attacking": the class, not the one unit.
#[test]
fn every_promotion_with_extra_attacks_gives_two_attacks() {
    for data in [Data::Shipped, Data::AttacksOnly] {
        let rules = rules(data);
        let carriers: Vec<String> = rules
            .promotions
            .iter()
            .filter(|p| p.effects.passive_unit.iter().any(|e| e.modifier == Modifier::ModifyUnitStat { stat: UnitStat::AttacksPerTurn, amount: 1 }))
            .map(|p| p.key.clone())
            .collect();
        assert!(carriers.len() >= 4, "the data has several: {carriers:?}");
        for key in carriers {
            let mut game = game(&rules);
            let (unit, [a, b]) = shooter(&rules, &mut game, "core:unit/archer", &[key.as_str()]);
            assert!(shoot(&mut game, &rules, unit, a), "{key}: first");
            assert!(shoot(&mut game, &rules, unit, b), "{key}: second");
            assert!(!shoot(&mut game, &rules, unit, a), "{key}: not a third");
        }
    }
}

/// The shipped Chu-Ko-Nu: two attacks in one turn.
#[test]
fn chu_ko_nu_attacks_twice() {
    let rules = rules(Data::Shipped);
    let mut game = game(&rules);
    let (unit, [a, b]) = shooter(&rules, &mut game, "core:unit/chu-ko-nu", &[]);
    assert!(shoot(&mut game, &rules, unit, a));
    assert!(shoot(&mut game, &rules, unit, b), "the second attack of the turn");
    assert!(!shoot(&mut game, &rules, unit, a));
}

/// An air sweep is an attack too: with an extra attack the fighter sweeps twice.
#[test]
fn a_fighter_with_an_extra_attack_sweeps_twice() {
    let rules = rules(Data::AttacksOnly);
    let mut game = game(&rules);
    let fighter = game.debug_spawn_unit(&rules, P0, tile(&game, 4, 4), kind(&rules, "core:unit/fighter"));
    game.debug_grant_promotion(&rules, fighter, promotion(&rules, "core:promotion/second-attack"));
    let target = tile(&game, 6, 4);
    let sweep = |game: &mut GameState| game.execute(&rules, Command::AirSweep { unit: fighter, target }).is_ok();
    assert!(sweep(&mut game), "the first sweep");
    assert!(sweep(&mut game), "the second sweep");
    assert!(!sweep(&mut game), "not a third");
}
