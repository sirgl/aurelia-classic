//! The pieces of the tactical layer (classic/docs/ai-tactical.md): roles from the
//! unit data, the enemy's reach and the threat map of the battlefield model,
//! the plan search's rules (ranged first, the capture last, nothing ends where
//! the best reply kills it), and the variant's place in the list.

use classic_content::core::core_rules;
use classic_rules::ai::battlefield::{Battlefield, KindStats, Posture};
use classic_rules::ai::planner::TurnBrief;
use classic_rules::ai::roles::{self, Role};
use classic_rules::ai::variant::Variant;
use classic_rules::ids::{PlayerId, ResourceId, TechId, TerrainId, TileId, UnitKindId};
use classic_rules::knowledge::observation::OwnUnit;
use classic_rules::math::HexCoord;
use classic_rules::{compile_rules, ClassicGame, CompiledRuleset, GameConfig, GameState};

const US: PlayerId = PlayerId(0);
const THEM: PlayerId = PlayerId(1);
/// The codes of the map `GameState::new` builds (not the rules' list order).
const GRASSLAND: u16 = 1;

fn unit_kind(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    let full = format!("core:unit/{key}");
    UnitKindId(rules.units.iter().position(|u| u.key == full).unwrap() as u16)
}

/// Open grassland 30 × 20, a city for each side and everybody knowing the map.
fn world(rules: &CompiledRuleset) -> GameState {
    let mut game = GameState::new(rules, GameConfig { players: 2, width: 30, height: 20, rng_seed: 5 });
    let dims = game.view().auth.map().dimensions;
    let land: Vec<(TileId, TerrainId)> = (0..dims.tile_count() as u32).map(|t| (TileId(t), TerrainId(GRASSLAND))).collect();
    game.debug_set_terrain_batch(rules, &land);
    let tiles: Vec<TileId> = land.iter().map(|(t, _)| *t).collect();
    game.debug_clear_features(rules, &tiles);
    game.debug_set_river_edges(rules, &[]);
    game.debug_found_city(rules, US, tile(&game, 2, 10));
    game.debug_found_city(rules, THEM, tile(&game, 26, 10));
    let iron = rules.resources.iter().position(|r| r.key == "core:resource/iron").unwrap();
    game.debug_set_deposit(rules, tile(&game, 2, 10), ResourceId(iron as u16), 6);
    for key in ["core:tech/bronze-working", "core:tech/iron-working", "core:tech/archery", "core:tech/mathematics"] {
        let tech = rules.technologies.iter().position(|t| t.key == key).unwrap();
        game.debug_learn(rules, US, TechId(tech as u16));
    }
    game.debug_meet(US, THEM);
    game.debug_reveal(US, &tiles);
    game
}

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
}

fn own_units(game: &GameState, rules: &CompiledRuleset) -> Vec<OwnUnit> {
    game.ai_observation(rules, US).own_units.clone()
}

/// Roles come from strength, range, movement and health, never from names.
#[test]
fn roles_follow_the_unit_data() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = world(&rules);
    let spawn = |game: &mut GameState, key: &str, q: i32, r: i32| {
        game.debug_spawn_unit(&rules, US, tile(game, q, r), unit_kind(&rules, key));
    };
    spawn(&mut game, "swordsman", 8, 8);
    spawn(&mut game, "swordsman", 8, 9);
    spawn(&mut game, "archer", 7, 8);
    spawn(&mut game, "catapult", 7, 9);
    spawn(&mut game, "warrior", 8, 10);
    let units = own_units(&game, &rules);
    let military: Vec<OwnUnit> = units.iter().filter(|u| rules.units[u.kind.0 as usize].strength > 0).copied().collect();
    let assigned = roles::assign(&rules, &military);
    let role_of = |key: &str| {
        let kind = unit_kind(&rules, key);
        military.iter().zip(&assigned).find(|(u, _)| u.kind == kind).map(|(_, r)| *r).unwrap()
    };
    assert_eq!(role_of("archer"), Role::Artillery);
    assert_eq!(role_of("catapult"), Role::Artillery);
    assert_eq!(role_of("swordsman"), Role::Anvil);
    assert_eq!(role_of("warrior"), Role::Screen, "a melee unit far weaker than the best is a screen");
    // The same unit, badly hurt, is a reserve: it leaves the line.
    let mut hurt = military.clone();
    for u in hurt.iter_mut().filter(|u| u.kind == unit_kind(&rules, "swordsman")) {
        u.hp = 30;
    }
    let assigned = roles::assign(&rules, &hurt);
    assert!(hurt.iter().zip(&assigned).filter(|(u, _)| u.kind == unit_kind(&rules, "swordsman")).all(|(_, r)| *r == Role::Reserve));
}

#[test]
fn veterans_leave_earlier() {
    assert!(roles::withdraw_below(3) > roles::withdraw_below(0));
    assert_eq!(roles::withdraw_below(0), roles::WITHDRAW_BELOW);
}

/// The enemy's reach: a melee unit with two moves strikes a tile three steps away
/// from the second step only if it arrives with movement to spare; our bodies and
/// zones of control close the ways to a covered tile.
#[test]
fn the_threat_map_follows_reach_and_cover() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = world(&rules);
    let sword = unit_kind(&rules, "swordsman");
    game.debug_found_city(&rules, THEM, tile(&game, 28, 2));
    let foe = game.debug_spawn_unit(&rules, THEM, tile(&game, 14, 10), sword);
    let _ = foe;
    let ours = game.debug_spawn_unit(&rules, US, tile(&game, 9, 10), unit_kind(&rules, "archer"));
    let _ = ours;
    let obs = game.ai_observation(&rules, US);
    let brief = TurnBrief::from_observation(&obs, &rules);
    let foes = obs.visible_foreign_units.clone();
    // The archer sees nothing yet at 5 tiles (sight 2): give the view the foe by hand.
    let view = obs.view;
    let own_city = [tile(&game, 2, 10)];
    let centres = [tile(&game, 9, 10), tile(&game, 14, 10)];
    let foe_unit = classic_rules::knowledge::observation::ObservedUnit {
        handle: game.view().auth.indexes().units_on(tile(&game, 14, 10))[0],
        tile: tile(&game, 14, 10),
        owner: THEM,
        kind: sword,
        hp: 100,
    };
    let _ = foes;
    let field = Battlefield::build(&view, &rules, &brief, 1, &[foe_unit], &[], &[], &own_city, &centres, 6, None);
    let free = Posture::default();
    let at = |q, r| tile(&game, q, r);
    // Two moves: one step and then a blow (a unit that spends both moves on the way
    // has none left to strike). So 13,9 and 12,10 can be struck (a step to 13,10 or 14,9
    // puts the foe next to them), 12,9 and 9,10 cannot.
    assert!(field.can_strike(0, at(13, 9), &free));
    assert!(field.can_strike(0, at(12, 10), &free));
    assert!(!field.can_strike(0, at(12, 9), &free), "two steps leave no move for the blow");
    assert!(!field.can_strike(0, at(9, 10), &free), "five tiles away is out of reach");
    // Bodies of ours on every tile the foe could stand on next to 12,10 leave it nowhere to stand.
    let ring: Vec<TileId> = field.dims.neighbors(at(12, 10)).into_iter().collect();
    let covered = Posture { bodies: ring.clone(), sources: ring };
    assert!(!field.can_strike(0, at(12, 10), &covered), "bodies on all the neighbours leave nowhere to stand");
    // A zone of control: a unit of ours at 13,8 slows the step 14,9 -> 13,9 (the rules: a
    // source next to both tiles), so 13,9 is reached with no move left by that way, but
    // the foe's other way, 14,10 -> 13,10, is open and 13,9 can still be struck from there.
    let zone = Posture { bodies: vec![at(13, 8)], sources: vec![at(13, 8)] };
    assert!(field.can_strike(0, at(13, 9), &zone));
    // The ranged strength of the data is what an archer strikes with.
    let archer = KindStats::of(&rules, unit_kind(&rules, "archer"));
    assert!(archer.is_ranged() && archer.attack() == archer.ranged && archer.melee < archer.ranged);
}

/// The new variant is listed after the game's AI; it and its copy with the
/// danger net (`tactical-net`) are the only ones that fight with the tactical
/// layer.
#[test]
fn the_variant_is_listed_and_alone() {
    assert_eq!(Variant::ALL[0], Variant::Standard);
    assert!(Variant::ALL.contains(&Variant::Tactical));
    let tactical: Vec<Variant> = Variant::ALL.iter().copied().filter(|v| v.tactical()).collect();
    assert_eq!(tactical, [Variant::Tactical, Variant::TacticalNet]);
    assert!(!Variant::Tactical.danger_net() && Variant::TacticalNet.danger_net() && Variant::DangerNet.danger_net() && !Variant::Standard.danger_net());
    assert_eq!(Variant::from_name("tactical"), Some(Variant::Tactical));
    assert!(!Variant::Standard.tactical());
    // It plays as the game's AI in everything else.
    assert_eq!(Variant::Tactical.price_model(), Variant::Standard.price_model());
    assert_eq!(Variant::Tactical.science_pricing(), Variant::Standard.science_pricing());
}
