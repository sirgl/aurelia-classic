//! `landless-plan` (`ai::landless`, `operations::start_landless`): a player with units and no city and no settler
//! gives the city its units take to the empire's strategist, whose operation musters, declares and takes it; the
//! choice of the city; the war is the player's (the operation's launch), so a member that cannot take part does not
//! hold the group. The bench's variant only (`experimental-ai`).
#![cfg(feature = "experimental-ai")]

use classic_content::core::core_rules;
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::variant::Variant;
use classic_rules::ai::war_probe;
use classic_rules::ai::AiSettings;
use classic_rules::ids::{PlayerId, TerrainId, TileId, UnitKindId};
use classic_rules::math::HexCoord;
use classic_rules::{compile_rules, Command, CompiledRuleset, GameConfig, GameState};
use kernel::ai::driver::AiDriver;

const AI: PlayerId = PlayerId(0);
const FOE: PlayerId = PlayerId(1);

struct Scene {
    game: GameState,
    rules: CompiledRuleset,
}

impl Scene {
    fn tile(&self, q: i32, r: i32) -> TileId {
        self.game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
    }

    fn kind(&self, key: &str) -> UnitKindId {
        UnitKindId(self.rules.units.iter().position(|u| u.key == key).unwrap() as u16)
    }

    fn owner_of_city(&self, tile: TileId) -> Option<PlayerId> {
        let auth = self.game.view().auth;
        auth.indexes().city_on_tile(tile).and_then(|h| auth.cities().get(h)).map(|c| c.owner)
    }
}

/// A plain 40 x 20 land. The AI has no city and no settler, only `warriors` warriors (the first of them, the lowest
/// id, at `first`, the others round (8, 10)); the rival has a city of one citizen at (15, 10) with nothing in it and
/// no army, and (with `second`) another at (8, 3) with a spearman inside. The two have met and the AI knows the land
/// round both cities.
fn scene(warriors: usize, first: (i32, i32), second: bool) -> Scene {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(&rules, GameConfig { players: 2, width: 40, height: 20, rng_seed: 7 });
    let tiles = game.view().auth.map().dimensions.tile_count() as u32;
    let land: Vec<(TileId, TerrainId)> = (0..tiles).map(|t| (TileId(t), TerrainId(1))).collect();
    game.debug_set_terrain_batch(&rules, &land);
    let mut scene = Scene { game, rules };
    let foe_tile = scene.tile(15, 10);
    scene.game.debug_found_city(&scene.rules, FOE, foe_tile);
    let capital = scene.tile(37, 17);
    scene.game.debug_found_city(&scene.rules, FOE, capital);
    if second {
        let tile = scene.tile(8, 3);
        scene.game.debug_found_city(&scene.rules, FOE, tile);
        let spearman = scene.kind("core:unit/spearman");
        scene.game.debug_spawn_unit(&scene.rules, FOE, tile, spearman);
    }
    let warrior = scene.kind("core:unit/warrior");
    let first = scene.tile(first.0, first.1);
    scene.game.debug_spawn_unit(&scene.rules, AI, first, warrior);
    let around: Vec<TileId> = [(8, 10), (8, 11), (9, 10), (7, 10), (8, 9), (9, 9), (7, 11), (9, 11)].iter().map(|(q, r)| scene.tile(*q, *r)).collect();
    for tile in around.iter().take(warriors - 1) {
        scene.game.debug_spawn_unit(&scene.rules, AI, *tile, warrior);
    }
    scene.game.debug_meet(AI, FOE);
    let dims = scene.game.view().auth.map().dimensions;
    let known: Vec<TileId> = (0..tiles).map(TileId).filter(|t| dims.distance(*t, foe_tile).is_some_and(|d| d <= 9) || dims.distance(*t, scene.tile(8, 3)).is_some_and(|d| d <= 4)).collect();
    scene.game.debug_reveal(AI, &known);
    scene
}

/// Plays `rounds` rounds: `variant` on the AI's seat, the rival passive (its turns end at once).
fn play(scene: &mut Scene, variant: Variant, rounds: u32) -> AiDriver<classic_rules::classic::Classic, ClassicMind> {
    let settings = AiSettings::for_rules(&scene.rules, scene.rules.default_difficulty());
    let mut driver = AiDriver::with_mind(settings, vec![], ClassicMind::new(variant));
    driver.set_trace_capacity(0);
    let start = scene.game.view().auth.turn().round;
    while scene.game.view().auth.turn().round < start + rounds && scene.game.winner().is_none() {
        if scene.game.view().auth.active_player() == FOE {
            scene.game.execute(&scene.rules, Command::EndTurn { player: FOE }).unwrap();
        } else {
            driver.run(&mut scene.game, &scene.rules, u32::MAX);
        }
    }
    driver
}

/// The city the units take is chosen, an operation of the strategist is started on it and launched: the player
/// declares the war (`operations::launch`) and the operation marches; the siege is the game's (`ai::siege_walls`,
/// `ai::front`). That it takes the city is the arenas' measure, not this test's.
#[test]
fn the_strategist_marches_on_the_city_the_units_take() {
    let mut scene = scene(6, (8, 12), false);
    let foe_tile = scene.tile(15, 10);
    war_probe::enable(true);
    play(&mut scene, Variant::LandlessPlan, 8);
    let probe = war_probe::take();
    war_probe::enable(false);
    assert_eq!(probe.landless_goals.first().map(|g| g.2), Some(foe_tile.0), "the empty city is the goal");
    assert!(probe.landless_starts.iter().any(|s| s.2 == foe_tile.0), "an operation was started on it: {:?}", probe.landless_starts);
    assert!(probe.declarations.iter().any(|d| d.1 == AI && d.2 == FOE), "the operation declared the war: {:?}", probe.declarations);
    assert!(probe.launched.iter().any(|l| l.1 == AI && l.2 == FOE), "the operation marched");
}

/// Of two cities the units could reach, the goal is the one they take soonest by the estimate: the empty one, not the
/// one with a spearman in it.
#[test]
fn the_goal_is_the_city_taken_soonest() {
    let mut scene = scene(6, (8, 12), true);
    let empty = scene.tile(15, 10);
    war_probe::enable(true);
    play(&mut scene, Variant::LandlessPlan, 2);
    let probe = war_probe::take();
    war_probe::enable(false);
    assert_eq!(probe.landless_goals.first().map(|g| g.2), Some(empty.0));
}

/// The war is declared by the player when the operation's army has gathered (`operations::launch`), not by one
/// member: the warrior with the lowest id stands 25 tiles away and takes no part, the others go to war.
#[test]
fn a_member_far_away_does_not_hold_the_group() {
    let mut scene = scene(6, (33, 2), false);
    war_probe::enable(true);
    play(&mut scene, Variant::LandlessPlan, 8);
    let probe = war_probe::take();
    war_probe::enable(false);
    assert!(probe.declarations.iter().any(|d| d.1 == AI && d.2 == FOE), "the war was declared: {:?}", probe.declarations);
}

/// `standard` in the same state gives its units nothing to do: no goal, no operation, no war.
#[test]
fn standard_without_a_city_starts_no_war() {
    let mut scene = scene(6, (8, 12), false);
    war_probe::enable(true);
    play(&mut scene, Variant::Standard, 10);
    let probe = war_probe::take();
    war_probe::enable(false);
    assert!(probe.landless_goals.is_empty() && probe.landless_starts.is_empty());
    assert!(!scene.game.view().auth.diplomacy().at_war(AI, FOE));
}
