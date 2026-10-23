//! Natural wonders in play on the classic ruleset (`systems::natural_wonders`):
//! the first finder's gold (El Dorado), Spain's find gold, the adjacent
//! free promotion (Fountain of Youth), the no-improvement and rough-ground
//! rules, and the finds kept across save and load.

use classic_content::core::core_rules;
use classic_rules::ids::{ImprovementId, PlayerId, TileId, UnitKindId};
use classic_rules::persistence::{decode::decode, encode::encode};
use classic_rules::replay::authoritative_hash;
use classic_rules::rules::content::NaturalWonderId;
use classic_rules::{compile_rules, Command, CompiledRuleset, GameConfig, GameState};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn rules() -> CompiledRuleset {
    compile_rules(core_rules()).unwrap()
}

/// Three civilizations, each alive with a scout in its own corner.
fn world(rules: &CompiledRuleset) -> GameState {
    let mut game = GameState::new(rules, GameConfig { players: 3, width: 24, height: 16, rng_seed: 7 });
    let scout = unit_kind(rules, "core:unit/scout");
    for (p, (q, r)) in [(0, 1), (22, 1), (1, 14)].into_iter().enumerate() {
        let at = tile(&game, q, r);
        game.debug_spawn_unit(rules, PlayerId(p as u8), at, scout);
    }
    game
}

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(classic_rules::math::HexCoord { q, r }).unwrap()
}

fn wonder(rules: &CompiledRuleset, key: &str) -> NaturalWonderId {
    NaturalWonderId(rules.natural_wonders.iter().position(|w| w.key == key).unwrap() as u16)
}

fn unit_kind(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap() as u16)
}

fn gold(game: &GameState, player: PlayerId) -> u32 {
    game.view().auth.players().treasury_of(player)
}

fn end_turn(game: &mut GameState, rules: &CompiledRuleset) {
    let player = game.view().auth.active_player();
    game.execute(rules, Command::EndTurn { player }).unwrap();
    game.validate(rules).unwrap();
}

/// Gold `player` gained over its own end of turn (no city, no unit: the
/// find is the only income).
fn gold_over_turn(game: &mut GameState, rules: &CompiledRuleset, player: PlayerId) -> i64 {
    assert_eq!(game.view().auth.active_player(), player);
    let before = i64::from(gold(game, player));
    end_turn(game, rules);
    i64::from(gold(game, player)) - before
}

#[test]
fn el_dorado_pays_its_first_finder_only() {
    let rules = rules();
    let mut game = world(&rules);
    let site = tile(&game, 10, 8);
    game.debug_set_natural_wonder_batch(&rules, &[(site, wonder(&rules, "core:natural-wonder/el-dorado"))]);
    game.debug_reveal(P0, &[site]);
    game.debug_reveal(P1, &[site]);
    assert_eq!(gold_over_turn(&mut game, &rules, P0), 500, "the first finder");
    assert_eq!(gold_over_turn(&mut game, &rules, P1), 0, "found before by P0");
    let found = |p: PlayerId| game.view().auth.players().economy_of(p).unwrap().natural_wonders_found.clone();
    assert_eq!(found(P0), vec![site]);
    assert_eq!(found(P1), vec![site]);
    // Found once: the next turns pay nothing.
    end_turn(&mut game, &rules);
    assert_eq!(gold_over_turn(&mut game, &rules, P0), 0);
    let loaded = decode(&encode(&game, &rules), &rules).expect("own save loads");
    assert_eq!(authoritative_hash(&loaded), authoritative_hash(&game), "finds survive save/load");
}

#[test]
fn spain_earns_five_hundred_for_a_first_find_and_one_hundred_after_others() {
    let rules = rules();
    let mut game = world(&rules);
    let spain = rules.civs.iter().position(|c| c.key == "core:nation/spain").unwrap() as u8;
    game.debug_set_player_civ(P1, spain);
    let fuji = tile(&game, 6, 6);
    let sinai = tile(&game, 16, 10);
    game.debug_set_natural_wonder_batch(
        &rules,
        &[
            (fuji, wonder(&rules, "core:natural-wonder/mt-fuji")),
            (sinai, wonder(&rules, "core:natural-wonder/mt-sinai")),
        ],
    );
    game.debug_reveal(P0, &[fuji]);
    assert_eq!(gold_over_turn(&mut game, &rules, P0), 0, "no finder gold for Mt. Fuji");
    game.debug_reveal(P1, &[fuji, sinai]);
    assert_eq!(gold_over_turn(&mut game, &rules, P1), 100 + 500, "Fuji after P0, Sinai first");
}

#[test]
fn a_unit_passing_the_fountain_of_youth_heals_faster_for_good() {
    let rules = rules();
    let mut game = world(&rules);
    let fountain = tile(&game, 10, 8);
    game.debug_set_natural_wonder_batch(&rules, &[(fountain, wonder(&rules, "core:natural-wonder/fountain-of-youth"))]);
    let promotion = rules.natural_wonders[wonder(&rules, "core:natural-wonder/fountain-of-youth").0 as usize]
        .adjacent_unit_free_promotion
        .unwrap();
    let start = tile(&game, 7, 8);
    let next_to = tile(&game, 9, 8);
    let warrior = game.debug_spawn_unit(&rules, P0, start, unit_kind(&rules, "core:unit/warrior"));
    let worker = game.debug_spawn_unit(&rules, P0, tile(&game, 7, 9), unit_kind(&rules, "core:unit/worker"));
    let promoted = |game: &GameState, unit| game.view().auth.units().get(unit).unwrap().core.promotions.contains(promotion);
    assert!(!promoted(&game, warrior));
    game.execute(&rules, Command::MoveUnit { unit: warrior, destination: next_to }).unwrap();
    assert!(promoted(&game, warrior), "a melee unit next to the wonder takes it");
    game.execute(&rules, Command::MoveUnit { unit: worker, destination: tile(&game, 9, 9) }).unwrap();
    assert!(!promoted(&game, worker), "a worker's role may not take it");
    game.validate(&rules).unwrap();
}

#[test]
fn a_natural_wonder_is_rough_ground_and_takes_no_improvement() {
    let rules = rules();
    let mut game = world(&rules);
    let site = tile(&game, 10, 8);
    game.debug_set_natural_wonder_batch(&rules, &[(site, wonder(&rules, "core:natural-wonder/uluru"))]);
    let map = game.view().auth.map();
    assert!(classic_rules::navigation::terrain::is_rough(&rules, map, site));
    let farm = rules.improvements.iter().position(|i| i.key == "core:improvement/farm").unwrap();
    let agriculture = rules.technologies.iter().position(|t| t.key == "core:tech/agriculture").unwrap();
    game.debug_learn(&rules, P0, classic_rules::ids::TechId(agriculture as u16));
    let site_rule = classic_rules::world::builds::improvement_site(game.view().auth, &rules, P0, site, ImprovementId(farm as u16));
    assert_eq!(site_rule.err(), Some("natural wonder"));
}
