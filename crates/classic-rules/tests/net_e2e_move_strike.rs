//! Format 10: a move candidate says what the unit could strike from its
//! destination this turn and the next, and how near the hostile units are (`encode::MoveStrike`, classic/docs/ai-net-data.md, «Format
//! 10: the strike from a move»). The columns are the rules' own: each move
//! of each unit of a siege scene is played on a copy of the world (a siege
//! unit then sets up when it may), and the attack candidates the unit has
//! there give what the move's row said — the best blow's capture, kill and
//! share of the defender's hit points, the city's share, the targets. A
//! format-9 weights file (the candidate rows cut before these columns) still
//! plays; another layout does not.

use classic_content::core::core_rules;
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::net_e2e::encode::{self, Candidate, C};
use classic_rules::ai::net_e2e::mind::{layout_format, layout_hash, layout_text, layout_text_of};
use classic_rules::ai::net_e2e::model::fnv1a64;
use classic_rules::ai::variant::NET_BASE;
use classic_rules::ai::AiSettings;
use classic_rules::ids::{PlayerId, TerrainId, TileId, UnitHandle, UnitKindId};
use classic_rules::knowledge::public::PlayerQueries;
use classic_rules::math::HexCoord;
use classic_rules::{compile_rules, ClassicGame, Command, CompiledRuleset, GameConfig, GameState};
use kernel::ai::mind::AiMind;

const AI: PlayerId = PlayerId(0);
const ENEMY: PlayerId = PlayerId(1);
const GRASSLAND: u16 = 1;

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
}

fn unit_kind(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap() as u16)
}

/// An enemy city at (14, 10), battered (a charge may take it), a spearman
/// beside it, a wounded warrior below; the AI's swordsmen two tiles out (a step, then a charge), a
/// horseman farther, an archer a step from its range, catapults three tiles
/// out (a catapult that steps has no move left to set up and shoot).
fn scene(rules: &CompiledRuleset, war: bool) -> GameState {
    let mut game = GameState::new(rules, GameConfig { players: 2, width: 28, height: 20, rng_seed: 5 });
    let dims = game.view().auth.map().dimensions;
    let land: Vec<(TileId, TerrainId)> = (0..dims.tile_count() as u32).map(|t| (TileId(t), TerrainId(GRASSLAND))).collect();
    game.debug_set_terrain_batch(rules, &land);
    game.debug_found_city(rules, AI, tile(&game, 3, 10));
    let target = tile(&game, 14, 10);
    let city = game.debug_found_city(rules, ENEMY, target);
    game.debug_set_city_damage(city, 185);
    game.debug_spawn_unit(rules, ENEMY, tile(&game, 15, 9), unit_kind(rules, "core:unit/spearman"));
    let warrior = game.debug_spawn_unit(rules, ENEMY, tile(&game, 13, 12), unit_kind(rules, "core:unit/warrior"));
    game.debug_set_unit_hp(warrior, 20);
    for (q, r, key) in [
        (12, 10, "core:unit/swordsman"),
        (12, 11, "core:unit/swordsman"),
        (10, 10, "core:unit/horseman"),
        (12, 9, "core:unit/archer"),
        (11, 12, "core:unit/catapult"),
        (10, 11, "core:unit/catapult"),
        // A scout that keeps the spearman in sight (a step revealing a foe
        // changes the reach; the rows know only what the player sees).
        (17, 9, "core:unit/scout"),
    ] {
        game.debug_spawn_unit(rules, AI, tile(&game, q, r), unit_kind(rules, key));
    }
    let everything: Vec<TileId> = (0..dims.tile_count() as u32).map(TileId).collect();
    game.debug_reveal(AI, &everything);
    game.debug_meet(AI, ENEMY);
    if war {
        game.execute(rules, Command::DeclareWar { player: AI, enemy: ENEMY }).unwrap();
    }
    game
}

fn own_units(game: &GameState, rules: &CompiledRuleset) -> Vec<UnitHandle> {
    game.ai_observation(rules, AI).own_units.iter().map(|u| u.handle).collect()
}

fn candidates(game: &GameState, rules: &CompiledRuleset, unit: UnitHandle) -> Vec<Candidate> {
    let settings = AiSettings::for_rules(rules, rules.default_difficulty());
    let probe = ClassicMind::new(NET_BASE);
    let sight = game.ai_observation(rules, AI);
    let mut turn = encode::Turn::encode(&sight, rules);
    let near = probe.sense_unit(game, rules, &settings, AI, unit).expect("an own unit");
    turn.own_unit_row(&near.unit, rules);
    encode::unit_candidates(&mut turn, &near, rules)
}

fn col(c: &Candidate, column: C) -> i16 {
    c.row[column as usize]
}

/// The format-10 columns of a move as the unit's attack candidates give
/// them once it stands there.
fn expected(game: &GameState, rules: &CompiledRuleset, unit: UnitHandle, destination: TileId, city: TileId) -> [i16; 7] {
    let mut copy = game.clone();
    copy.execute(rules, Command::MoveUnit { unit, destination }).expect("the move");
    if copy.player_view(rules, AI).must_set_up_first(unit) {
        let _ = copy.execute(rules, Command::SetUpRangedAttack { unit });
    }
    // What the player sees now: a target the step itself brings into sight is
    // not one the move's row can know.
    let seen = |t: i32| game.player_view(rules, AI).is_visible(TileId(t as u32));
    let attacks: Vec<Candidate> =
        candidates(&copy, rules, unit).into_iter().filter(|c| col(c, C::ActMelee) + col(c, C::ActRanged) > 0 && col(c, C::DefenderHp) > 0 && seen(c.meta[1])).collect();
    let share = |c: &Candidate| i16::try_from(i64::from(col(c, C::ToDefender)) * 100 / i64::from(col(c, C::DefenderHp).max(1))).unwrap();
    let best = attacks.iter().map(|c| (col(c, C::BlowCaptures), col(c, C::BlowKills), share(c))).max().unwrap_or((0, 0, 0));
    let on_city: Vec<i16> = attacks.iter().filter(|c| col(c, C::TargetForeignCity) == 1).map(share).collect();
    let adjacent = game.view().auth.map().dimensions.adjacent(destination, city);
    [best.0, best.1, best.2, on_city.iter().copied().max().unwrap_or(0), i16::from(adjacent), i16::from(!on_city.is_empty()), attacks.len() as i16]
}

/// The columns of this turn's strike (checked against the attacks there).
const COLUMNS: [C; 7] = [C::MoveBlowCaptures, C::MoveBlowKills, C::MoveBlowDamage, C::MoveCityDamage, C::MoveCityAdjacent, C::MoveCityInStrike, C::MoveTargets];
/// The columns of the hostile units' distance and the next turn.
const NEXT: [C; 5] = [C::MoveEnemyDistance, C::MoveWoundedDistance, C::MoveWoundedHp, C::MoveNextTargets, C::MoveNextKills];
/// The columns of the cities the player remembers out of sight.
const UNSEEN: [C; 2] = [C::MoveCityDistance, C::MoveUnseenCityInReach];

/// The hostile units the AI sees outside cities: tile and hit points.
fn hostile_units(game: &GameState, rules: &CompiledRuleset) -> Vec<(TileId, u16)> {
    let auth = game.view().auth;
    let view = game.player_view(rules, AI);
    let mut out: Vec<(TileId, u16)> = auth
        .units()
        .iter_handles()
        .filter(|(_, u)| u.owner == ENEMY && view.is_visible(u.tile) && auth.indexes().city_on_tile(u.tile).is_none())
        .map(|(_, u)| (u.tile, u.hp))
        .collect();
    out.sort();
    out
}

#[test]
fn a_moves_strike_columns_are_the_attacks_from_its_destination() {
    let rules = compile_rules(core_rules()).unwrap();
    let game = scene(&rules, true);
    let city = tile(&game, 14, 10);
    let (mut moves, mut striking, mut captures, mut kills, mut adjacent_only) = (0, 0, 0, 0, 0);
    let mut by_kind = std::collections::BTreeMap::<String, (u32, u32)>::new();
    for unit in own_units(&game, &rules) {
        let kind = game.view().auth.units().get(unit).unwrap().core.kind;
        let key = rules.units[kind.0 as usize].key.clone();
        for c in candidates(&game, &rules, unit) {
            let Some(Command::MoveUnit { destination, .. }) = c.command else {
                assert!(COLUMNS.iter().chain(&NEXT).chain(&UNSEEN).all(|k| col(&c, *k) == 0), "{key}: a candidate other than a move has no move-strike column: {:?}", c.command);
                continue;
            };
            let got: Vec<i16> = COLUMNS.iter().map(|k| col(&c, *k)).collect();
            let want = expected(&game, &rules, unit, destination, city);
            assert_eq!(got, want, "{key} to {destination:?} (moves after {})", col(&c, C::MovesAfter));
            // The nearest hostile unit and the nearest wounded one, by the map's distance.
            let dims = game.view().auth.map().dimensions;
            let foes: Vec<(i32, u16)> = hostile_units(&game, &rules).iter().map(|(t, hp)| (dims.distance(destination, *t).unwrap(), *hp)).collect();
            let near = foes.iter().map(|f| f.0).min().unwrap();
            let wounded = foes.iter().filter(|f| f.1 < 100).min().copied().unwrap_or((0, 0));
            assert_eq!([col(&c, C::MoveEnemyDistance), col(&c, C::MoveWoundedDistance), col(&c, C::MoveWoundedHp)], [near as i16, wounded.0 as i16, wounded.1 as i16], "{key} to {destination:?}");
            moves += 1;
            striking += u32::from(got[6] > 0);
            captures += u32::from(got[0] == 1);
            kills += u32::from(got[1] == 1);
            adjacent_only += u32::from(got[4] == 1 && got[5] == 0);
            let e = by_kind.entry(key.clone()).or_default();
            e.0 += 1;
            e.1 += u32::from(got[6] > 0);
        }
    }
    eprintln!("{moves} moves, {striking} with a strike, {captures} capture, {kills} kill, {adjacent_only} beside the city out of strike; by kind {by_kind:?}");
    assert!(moves > 40 && striking >= 6 && captures >= 2 && kills > captures, "the scene shows each column: {moves} {striking} {captures} {kills}");
    assert!(adjacent_only >= 1, "a move beside the city without a strike (a catapult's, the moves spent)");
    assert_eq!(by_kind["core:unit/catapult"].1, 0, "a catapult that steps cannot set up and shoot: {by_kind:?}");
    assert!(by_kind["core:unit/archer"].1 > 0, "an archer steps into range and shoots: {by_kind:?}");
}

/// At peace the units see no hostile tile: every move's columns are zero
/// (the cost is then one pass over the near's foreign units and cities).
#[test]
fn at_peace_every_move_column_is_zero() {
    let rules = compile_rules(core_rules()).unwrap();
    let game = scene(&rules, false);
    let mut moves = 0;
    for unit in own_units(&game, &rules) {
        for c in candidates(&game, &rules, unit) {
            moves += usize::from(matches!(c.command, Some(Command::MoveUnit { .. })));
            assert!(COLUMNS.iter().chain(&NEXT).chain(&UNSEEN).all(|k| col(&c, *k) == 0), "{:?}", c.command);
        }
    }
    assert!(moves > 40, "{moves} moves");
}

/// A format-9 file's layout is this build's with the candidate rows cut
/// before the format-10 columns: it plays (as its prefix); another does not.
#[test]
fn a_format_9_layout_is_accepted_as_the_prefix() {
    let rules = compile_rules(core_rules()).unwrap();
    assert_eq!(encode::FORMAT, 10);
    assert_eq!(encode::FORMAT_9_CANDIDATE_COLUMNS, C::COUNT - COLUMNS.len() - NEXT.len() - UNSEEN.len());
    let full = layout_text(&rules);
    let cut = layout_text_of(&rules, encode::FORMAT_9_CANDIDATE_COLUMNS);
    let tail = ";move_blow_captures/1;move_blow_kills/1;move_blow_damage_percent/1;move_city_damage_percent/1;move_city_adjacent/1;move_city_in_strike/1;move_strike_targets/1;\
                move_enemy_distance/1;move_wounded_enemy_distance/1;move_wounded_enemy_hp_percent/1;move_next_turn_targets/1;move_next_turn_kills/1;move_city_distance/1;move_unseen_city_in_reach/1;";
    let candidate = full.lines().find(|l| l.starts_with("candidate:")).unwrap();
    assert!(candidate.ends_with(tail), "the format-10 tail (aurelia_nn.move_strike.FORMAT_10_CANDIDATE): {candidate}");
    assert!(cut.lines().find(|l| l.starts_with("candidate:")).unwrap().ends_with(";preview_city_floor/1;"));
    assert_eq!(layout_format(&rules, layout_hash(&rules)), Some(10));
    assert_eq!(layout_format(&rules, fnv1a64(cut.as_bytes())), Some(9));
    assert_eq!(layout_format(&rules, fnv1a64(layout_text_of(&rules, encode::FORMAT_9_CANDIDATE_COLUMNS - 1).as_bytes())), None);
}

/// The next turn's columns: the reach of a turn begun on the destination
/// (`own_reachable_from`) is the move preview the unit has there once a
/// fresh turn begins (the move played on a copy, both nations' turns ended:
/// nobody else moves), and the hostile units it could strike from a stop of
/// it are the ones the move's row counts; whether one blow kills, where the
/// turn's healing left every enemy as it was.
#[test]
fn the_next_turn_columns_are_the_fresh_turns_reach_and_strikes() {
    use classic_rules::ai::net_e2e::encode::{FORMAT, C as Col};
    let rules = compile_rules(core_rules()).unwrap();
    let game = scene(&rules, true);
    assert_eq!(FORMAT, 10);
    let view = game.player_view(&rules, AI);
    let (mut checked, mut reaching, mut killing) = (0, 0, 0);
    for unit in own_units(&game, &rules) {
        let core = *game.view().auth.units().get(unit).unwrap().core;
        let mut sorted = |mut v: Vec<(TileId, u16)>| {
            v.sort();
            v
        };
        assert_eq!(sorted(view.own_reachable_from(unit, core.tile)), sorted(view.own_reachable(unit)), "a fresh unit's turn from its own tile is its move preview");
        let sets_up = game.player_view(&rules, AI).must_set_up_first(unit);
        for c in candidates(&game, &rules, unit) {
            let Some(Command::MoveUnit { destination, .. }) = c.command else { continue };
            let mut copy = game.clone();
            copy.execute(&rules, Command::MoveUnit { unit, destination }).expect("the move");
            copy.execute(&rules, Command::EndTurn { player: AI }).unwrap();
            copy.execute(&rules, Command::EndTurn { player: ENEMY }).unwrap();
            let fresh = copy.player_view(&rules, AI);
            let stops = sorted(fresh.own_reachable(unit));
            // A hostile unit the step brings into sight blocks its tile only once seen.
            let unseen = |t: &TileId| copy.view().auth.indexes().units_on(*t).iter().any(|u| copy.view().auth.units().get(*u).is_some_and(|u| u.core.owner == ENEMY)) && !view.is_visible(*t);
            let known = |v: &Vec<(TileId, u16)>| v.iter().filter(|(t, _)| !unseen(t)).copied().collect::<Vec<_>>();
            assert_eq!(known(&sorted(view.own_reachable_from(unit, destination))), known(&stops), "the turn begun on {destination:?}");
            let keep = if sets_up { classic_rules::world::unit::MOVE_DENOMINATOR } else { 0 };
            let dims = copy.view().auth.map().dimensions;
            let reach = if rules.units[core.kind.0 as usize].ranged_strength > 0 { i32::from(rules.units[core.kind.0 as usize].range) + 1 } else { 1 };
            let mut targets = 0;
            let mut kills = false;
            // The hostile units seen before the step (the row knows no other).
            for (tile, _) in hostile_units(&game, &rules) {
                // From the stop with the most moves left (then the lowest tile) that can strike it.
                let mut order: Vec<&(TileId, u16)> = stops.iter().filter(|(s, left)| *left > keep && dims.distance(*s, tile).is_some_and(|d| d <= reach)).collect();
                order.sort_by_key(|(s, left)| (std::cmp::Reverse(*left), s.0));
                let blow = order.iter().find_map(|(s, _)| fresh.attack_preview_from(unit, *s, tile));
                targets += i16::from(blow.is_some());
                kills |= blow.is_some_and(|p| p.verdict == classic_rules::combat::exchange::Verdict::DecisiveVictory);
            }
            if col(&c, Col::MoveEnemyDistance) == 0 {
                continue;
            }
            assert_eq!(col(&c, Col::MoveNextTargets), targets, "next-turn targets from {destination:?}");
            if hostile_units(&game, &rules).iter().all(|f| hostile_units(&copy, &rules).contains(f)) {
                assert_eq!(col(&c, Col::MoveNextKills), i16::from(kills), "a next-turn kill from {destination:?}");
            }
            checked += 1;
            reaching += usize::from(targets > 0);
            killing += usize::from(col(&c, Col::MoveNextKills) == 1);
        }
    }
    eprintln!("{checked} moves checked, {reaching} with a next-turn target, {killing} with a next-turn kill");
    assert!(checked > 40 && reaching >= 10 && killing >= 1, "{checked} {reaching} {killing}");
}

/// The training-only targets (`unit_candidates_with_targets`, `encode::A`):
/// a move's danger is the hostile units' own attack previews on the unit
/// standing there (each from the tile the target names), summed; focus
/// fire on the battered city reaches its hit points with a capturer among
/// the strikers, so every move that strikes it this turn says it falls.
#[test]
fn the_training_targets_are_the_rules_previews() {
    use classic_rules::ai::net_e2e::encode::A;
    let rules = compile_rules(core_rules()).unwrap();
    let game = scene(&rules, true);
    let city = tile(&game, 14, 10);
    let settings = AiSettings::for_rules(&rules, rules.default_difficulty());
    let probe = ClassicMind::new(NET_BASE);
    let (mut dangerous, mut deadly, mut on_city) = (0, 0, 0);
    for unit in own_units(&game, &rules) {
        let sight = game.ai_observation(&rules, AI);
        let mut turn = encode::Turn::encode(&sight, &rules);
        let near = probe.sense_unit(&game, &rules, &settings, AI, unit).unwrap();
        let plain = encode::unit_candidates(&mut turn, &near, &rules);
        let with = encode::unit_candidates_with_targets(&mut turn, &near, &rules);
        assert_eq!(plain.iter().map(|c| (&c.row, c.meta)).collect::<Vec<_>>(), with.iter().map(|c| (&c.row, c.meta)).collect::<Vec<_>>(), "the targets change no input");
        let hp = game.view().auth.units().get(unit).unwrap().core.hp;
        for c in &with {
            assert_eq!(c.targets.len(), A::COUNT);
            let t = |k: A| c.targets[k as usize];
            let blow_on_city = c.meta[1] == city.0 as i32 && col(c, C::ActMelee) + col(c, C::ActRanged) > 0;
            if blow_on_city || col(c, C::MoveCityInStrike) == 1 {
                on_city += 1;
                assert_eq!(t(A::FocusTakes), 1, "the ring takes the battered city: {:?}", c.command);
                assert!(t(A::FocusDamage) >= 100, "{:?}", c.command);
            }
            let Some(Command::MoveUnit { destination, .. }) = c.command else {
                assert_eq!((t(A::DangerDamage), t(A::DangerDies)), (0, 0));
                continue;
            };
            // The enemy's own previews of its strikes on the unit standing there.
            let mut copy = game.clone();
            copy.execute(&rules, Command::MoveUnit { unit, destination }).unwrap();
            let enemy = copy.player_view(&rules, ENEMY);
            let dims = copy.view().auth.map().dimensions;
            let mut want = 0i32;
            let mut seen = true;
            for (handle, foe) in copy.view().auth.units().iter_handles().filter(|(_, u)| u.owner == ENEMY) {
                let def = &rules.units[foe.kind.0 as usize];
                let range = if def.ranged_strength > 0 && def.range > 0 { i32::from(def.range) } else { 1 };
                let d = dims.distance(foe.tile, destination).unwrap();
                if d - range > i32::from(def.movement) || !game.player_view(&rules, AI).is_visible(foe.tile) {
                    continue;
                }
                let from = if d <= range && range > 1 {
                    foe.tile
                } else {
                    classic_rules::math::hex_disc(dims, destination, range).filter(|t| dims.distance(*t, destination) == Some(range)).min_by_key(|t| (dims.distance(*t, foe.tile).unwrap(), t.0)).unwrap()
                };
                if !enemy.is_visible(destination) {
                    seen = false;
                }
                want += enemy.attack_preview_from(handle, from, destination).map_or(0, |p| i32::from(p.to_defender.1));
            }
            if seen {
                assert_eq!(i32::from(t(A::DangerDamage)), want, "danger on {destination:?}");
            }
            assert_eq!(t(A::DangerDies), i16::from(i32::from(t(A::DangerDamage)) >= i32::from(hp)));
            dangerous += usize::from(t(A::DangerDamage) > 0);
            deadly += usize::from(t(A::DangerDies) == 1);
        }
    }
    eprintln!("{on_city} blows on the city, {dangerous} dangerous moves, {deadly} deadly");
    assert!(on_city >= 2 && dangerous >= 5, "{on_city} {dangerous}");
}

/// A hostile city the player remembers out of sight: the step beside it
/// says so (adjacent, its distance, a blow within reach whose outcome the
/// player cannot preview), and no strike column claims to know its fate.
#[test]
fn a_remembered_city_out_of_sight_counts_by_its_tile() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(&rules, GameConfig { players: 2, width: 28, height: 20, rng_seed: 5 });
    let dims = game.view().auth.map().dimensions;
    let land: Vec<(TileId, TerrainId)> = (0..dims.tile_count() as u32).map(|t| (TileId(t), TerrainId(GRASSLAND))).collect();
    game.debug_set_terrain_batch(&rules, &land);
    game.debug_found_city(&rules, AI, tile(&game, 3, 10));
    let city = tile(&game, 16, 10);
    game.debug_found_city(&rules, ENEMY, city);
    // A horseman four tiles out: sight 2, four moves.
    let unit = game.debug_spawn_unit(&rules, AI, tile(&game, 12, 10), unit_kind(&rules, "core:unit/horseman"));
    let everything: Vec<TileId> = (0..dims.tile_count() as u32).map(TileId).collect();
    game.debug_reveal(AI, &everything);
    game.debug_meet(AI, ENEMY);
    game.execute(&rules, Command::DeclareWar { player: AI, enemy: ENEMY }).unwrap();
    assert!(!game.player_view(&rules, AI).is_visible(city), "the city out of sight");
    let (mut beside, mut in_reach) = (0, 0);
    for c in candidates(&game, &rules, unit) {
        let Some(Command::MoveUnit { destination, .. }) = c.command else { continue };
        let d = dims.distance(destination, city).unwrap();
        assert_eq!(col(&c, C::MoveCityDistance), d as i16, "{destination:?}");
        assert_eq!(col(&c, C::MoveCityAdjacent), i16::from(d == 1));
        assert_eq!(col(&c, C::MoveUnseenCityInReach), i16::from(d == 1 && col(&c, C::MovesAfter) > 0));
        assert_eq!((col(&c, C::MoveCityInStrike), col(&c, C::MoveBlowCaptures), col(&c, C::MoveTargets)), (0, 0, 0), "nothing previewed out of sight");
        beside += usize::from(d == 1);
        in_reach += usize::from(col(&c, C::MoveUnseenCityInReach) == 1);
    }
    assert!(beside >= 1 && in_reach >= 1, "{beside} {in_reach}");
}
