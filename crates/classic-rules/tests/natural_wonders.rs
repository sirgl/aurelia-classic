//! Natural wonders in play on the classic ruleset (`systems::natural_wonders`):
//! the first finder's gold (El Dorado), Spain's find gold, the adjacent
//! free promotion (Fountain of Youth), the no-improvement and rough-ground
//! rules, and the finds kept across save and load.

use classic_content::core::core_rules;
use classic_rules::classic::ClassicChange;
use classic_rules::rules::effects::{SourceRef, YieldKind};
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

/// Three nations, each alive with a scout in its own corner.
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
    let spain = rules.nations.iter().position(|c| c.key == "core:nation/spain").unwrap() as u8;
    game.debug_set_player_nation(P1, spain);
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

/// The natural-wonder changes of the active player's end of turn, in journal order.
fn finds_over_turn(game: &mut GameState, rules: &CompiledRuleset) -> Vec<ClassicChange> {
    let player = game.view().auth.active_player();
    let result = game.execute(rules, Command::EndTurn { player }).unwrap();
    game.validate(rules).unwrap();
    result
        .changes()
        .module_changes
        .iter()
        .filter(|c| matches!(c, ClassicChange::NaturalWonderFound { .. } | ClassicChange::WonderYield { .. }))
        .copied()
        .collect()
}

/// The journal names a find: the wonder, where, whether it was first, the finder gold the treasury got
/// (El Dorado's 500 for the first finder, none after) and the happiness a found wonder adds.
#[test]
fn a_find_is_journaled_with_the_gold_it_paid_and_the_happiness_it_adds() {
    let rules = rules();
    let mut game = world(&rules);
    let site = tile(&game, 10, 8);
    let el_dorado = wonder(&rules, "core:natural-wonder/el-dorado");
    game.debug_set_natural_wonder_batch(&rules, &[(site, el_dorado)]);
    game.debug_reveal(P0, &[site]);
    game.debug_reveal(P1, &[site]);
    let before = gold(&game, P0);
    let first = finds_over_turn(&mut game, &rules);
    assert_eq!(first, [ClassicChange::NaturalWonderFound { player: P0, tile: site, wonder: el_dorado, first: true, gold: 500, happiness: 1 }]);
    assert_eq!(gold(&game, P0) - before, 500, "what the journal names is what the treasury got");
    let later = finds_over_turn(&mut game, &rules);
    assert_eq!(later, [ClassicChange::NaturalWonderFound { player: P1, tile: site, wonder: el_dorado, first: false, gold: 0, happiness: 1 }]);
    assert!(finds_over_turn(&mut game, &rules).is_empty(), "the third nation found nothing");
    assert!(finds_over_turn(&mut game, &rules).is_empty(), "a find is told once");
}

/// What an effect paid for the find (Spain) is journaled before the find itself, with its source.
#[test]
fn spains_find_gold_is_journaled_as_a_wonder_yield() {
    let rules = rules();
    let mut game = world(&rules);
    let spain = rules.nations.iter().position(|c| c.key == "core:nation/spain").unwrap() as u8;
    game.debug_set_player_nation(P1, spain);
    let (fuji, sinai) = (tile(&game, 6, 6), tile(&game, 16, 10));
    let (fuji_id, sinai_id) = (wonder(&rules, "core:natural-wonder/mt-fuji"), wonder(&rules, "core:natural-wonder/mt-sinai"));
    game.debug_set_natural_wonder_batch(&rules, &[(fuji, fuji_id), (sinai, sinai_id)]);
    game.debug_reveal(P0, &[fuji]);
    let by_p0 = finds_over_turn(&mut game, &rules);
    assert!(matches!(by_p0[..], [ClassicChange::NaturalWonderFound { player: P0, first: true, gold: 0, .. }]), "{by_p0:?}");
    game.debug_reveal(P1, &[fuji, sinai]);
    let by_spain = finds_over_turn(&mut game, &rules);
    let gold_of = |tile, amount| ClassicChange::WonderYield { player: P1, tile, kind: YieldKind::Gold, amount, source: SourceRef::Nation(spain) };
    assert_eq!(by_spain.len(), 4, "{by_spain:?}");
    assert!(by_spain.contains(&gold_of(fuji, 100)), "after P0: 100");
    assert!(by_spain.contains(&gold_of(sinai, 500)), "first: 500");
    assert!(by_spain.iter().any(|c| matches!(c, ClassicChange::NaturalWonderFound { tile, first: false, .. } if *tile == fuji)));
    assert!(by_spain.iter().any(|c| matches!(c, ClassicChange::NaturalWonderFound { tile, first: true, .. } if *tile == sinai)));
}

/// The gold a find pays follows the game speed's gold percent like the camp gold: Standard 500, Quick
/// less, and the journal names exactly what the treasury got, for the finder gold and Spain's gold alike.
#[test]
fn find_gold_follows_the_game_speed() {
    let speeds = compile_rules(classic_content::classic::classic_rules()).unwrap().speeds;
    let (mut standard, mut quick) = (None, None);
    for speed in speeds.iter().map(|s| s.key.clone()) {
        let rules = compile_rules(classic_content::classic::classic_rules()).unwrap();
        let percent = rules.speed(&speed).unwrap().values.gold_percent;
        let mut game = world(&rules);
        game.set_speed(&rules, &speed);
        let spain = rules.nations.iter().position(|c| c.key == "core:nation/spain").unwrap() as u8;
        game.debug_set_player_nation(P0, spain);
        let site = tile(&game, 10, 8);
        let el_dorado = wonder(&rules, "core:natural-wonder/el-dorado");
        game.debug_set_natural_wonder_batch(&rules, &[(site, el_dorado)]);
        game.debug_reveal(P0, &[site]);
        let before = gold(&game, P0);
        let changes = finds_over_turn(&mut game, &rules);
        let scaled = |gold| kernel::rules::speeds::SpeedValues::scale(gold, percent);
        let finder = changes.iter().find_map(|c| match c {
            ClassicChange::NaturalWonderFound { gold, .. } => Some(*gold),
            _ => None,
        });
        let effect = changes.iter().find_map(|c| match c {
            ClassicChange::WonderYield { amount, .. } => Some(*amount),
            _ => None,
        });
        assert_eq!(finder, Some(scaled(500)), "{speed}: the finder gold");
        assert_eq!(effect, Some(scaled(500)), "{speed}: Spain's first-find gold");
        assert_eq!(gold(&game, P0) - before, 2 * scaled(500), "{speed}: the treasury got what the journal names");
        match speed.as_str() {
            "core:speed/standard" => standard = finder,
            "core:speed/quick" => quick = finder,
            _ => {}
        }
    }
    assert_eq!(standard, Some(500), "Standard pays as before");
    assert!(quick.unwrap() < 500, "Quick pays less: {quick:?}");
}

/// Every wonder a nation has found is one it knows (the find sees what the knowledge sync will remember).
fn finds_are_explored(game: &GameState) {
    let auth = game.view().auth;
    for p in 0..3 {
        let found = auth.players().economy_of(PlayerId(p)).unwrap().natural_wonders_found.clone();
        for site in found {
            assert!(auth.knowledge()[p as usize].is_explored(site), "P{p} found {site:?} without knowing it");
        }
    }
}

/// A unit that comes within sight of a wonder finds it in the move itself: the gold and the change are
/// in that command, not in the end of the turn; the first to see it stays the first.
#[test]
fn a_unit_finds_the_wonder_it_sees_in_the_move_itself() {
    let rules = rules();
    let mut game = world(&rules);
    let site = tile(&game, 10, 8);
    let el_dorado = wonder(&rules, "core:natural-wonder/el-dorado");
    game.debug_set_natural_wonder_batch(&rules, &[(site, el_dorado)]);
    let scout = unit_kind(&rules, "core:unit/scout");
    let first = game.debug_spawn_unit(&rules, P0, tile(&game, 6, 8), scout);
    let second = game.debug_spawn_unit(&rules, P1, tile(&game, 14, 8), scout);
    let found = |game: &GameState, p: PlayerId| game.view().auth.players().economy_of(p).unwrap().natural_wonders_found.clone();

    let before = gold(&game, P0);
    let result = game.execute(&rules, Command::MoveUnit { unit: first, destination: tile(&game, 7, 8) }).unwrap();
    assert!(found(&game, P0).is_empty(), "three tiles away: not in sight yet");
    assert!(!result.changes().module_changes.iter().any(|c| matches!(c, ClassicChange::NaturalWonderFound { .. })));
    let result = game.execute(&rules, Command::MoveUnit { unit: first, destination: tile(&game, 8, 8) }).unwrap();
    let told: Vec<_> = result.changes().module_changes.iter().filter(|c| matches!(c, ClassicChange::NaturalWonderFound { .. })).copied().collect();
    assert_eq!(told, [ClassicChange::NaturalWonderFound { player: P0, tile: site, wonder: el_dorado, first: true, gold: 500, happiness: 1 }]);
    assert_eq!(gold(&game, P0) - before, 500, "the gold came with the move, in the same turn");
    assert_eq!(found(&game, P0), vec![site]);
    finds_are_explored(&game);
    game.validate(&rules).unwrap();

    // Nothing again for the same wonder, at the end of the turn either.
    assert!(finds_over_turn(&mut game, &rules).is_empty(), "P0's end of turn finds nothing more");

    // P1 comes second: no finder gold.
    let before = gold(&game, P1);
    let result = game.execute(&rules, Command::MoveUnit { unit: second, destination: tile(&game, 12, 8) }).unwrap();
    let told: Vec<_> = result.changes().module_changes.iter().filter(|c| matches!(c, ClassicChange::NaturalWonderFound { .. })).copied().collect();
    assert_eq!(told, [ClassicChange::NaturalWonderFound { player: P1, tile: site, wonder: el_dorado, first: false, gold: 0, happiness: 1 }]);
    assert_eq!(gold(&game, P1), before);
    finds_are_explored(&game);
    let loaded = decode(&encode(&game, &rules), &rules).expect("own save loads");
    assert_eq!(authoritative_hash(&loaded), authoritative_hash(&game), "finds survive save/load");
}
