//! Raids (classic/docs/ai-raids.md): a small fast group of the `raids` variant pillages the land of
//! an enemy's city and escapes, plunders a trade route, is chosen where a conquest does not pay,
//! and never forms against a player the host named a human.

use classic_content::core::core_rules;
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::state::UnitTask;
use classic_rules::ai::variant::Variant;
use classic_rules::ai::war_probe;
use classic_rules::ai::AiSettings;
use classic_rules::ids::{ImprovementId, PlayerId, TechId, TerrainId, TileId, UnitKindId};
use classic_rules::math::HexCoord;
use classic_rules::world::trade::RouteKind;
use classic_rules::{compile_rules, ClassicGame, Command, CompiledRuleset, GameConfig, GameState};
use kernel::ai::driver::AiDriver;
use kernel::ai::trace::{Outcome, Reason};

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

    fn improvement(&self, key: &str) -> ImprovementId {
        ImprovementId(self.rules.improvements.iter().position(|i| i.key == key).unwrap() as u16)
    }

    fn learn(&mut self, player: PlayerId, key: &str) {
        let tech = TechId(self.rules.technologies.iter().position(|t| t.key == key).unwrap() as u16);
        self.game.debug_learn(&self.rules, player, tech);
    }

    fn owned_by(&self, tile: TileId) -> Option<PlayerId> {
        self.game.view().auth.map().owner(tile).flatten()
    }
}

/// A plain 40 x 20 land. The AI has three cities of five citizens on the west, each with a warrior
/// (the garrison) and a horseman (free); the rival has a city of four citizens at (14, 10), its
/// land planted with farms, and no army. The AI has seen everything; `war` starts the war.
fn scene(war: bool) -> Scene {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(&rules, GameConfig { players: 2, width: 40, height: 20, rng_seed: 7 });
    let tiles = game.view().auth.map().dimensions.tile_count() as u32;
    let land: Vec<(TileId, TerrainId)> = (0..tiles).map(|t| (TileId(t), TerrainId(1))).collect();
    game.debug_set_terrain_batch(&rules, &land);
    let mut scene = Scene { game, rules };
    for (q, r) in [(5, 10), (5, 5), (5, 15)] {
        let tile = scene.tile(q, r);
        let city = scene.game.debug_found_city(&scene.rules, AI, tile);
        scene.game.debug_set_population(&scene.rules, city, 5);
    }
    // The rival's capital lies far to the east, out of the AI's sight; the city of the scene is its second.
    let capital = scene.tile(37, 3);
    scene.game.debug_found_city(&scene.rules, FOE, capital);
    let foe_tile = scene.tile(14, 10);
    let foe = scene.game.debug_found_city(&scene.rules, FOE, foe_tile);
    scene.game.debug_set_population(&scene.rules, foe, 4);
    for (q, r) in [(5, 10), (5, 5), (5, 15)] {
        let tile = scene.tile(q, r);
        let warrior = scene.kind("core:unit/warrior");
        scene.game.debug_spawn_unit(&scene.rules, AI, tile, warrior);
    }
    for (q, r) in [(5, 10), (5, 5), (5, 15)] {
        let tile = scene.tile(q, r);
        let horseman = scene.kind("core:unit/horseman");
        scene.game.debug_spawn_unit(&scene.rules, AI, tile, horseman);
    }
    scene.learn(AI, "core:tech/horseback-riding");
    scene.learn(AI, "core:tech/bronze-working");
    // The rival buys its land out to the third ring (its borders after a few eras), then plants it.
    scene.game.execute(&scene.rules, Command::EndTurn { player: AI }).unwrap();
    let dims = scene.game.view().auth.map().dimensions;
    for ring in 2..=3 {
        for tile in classic_rules::math::tiles_in_hex_radius(&dims, foe_tile, ring) {
            if dims.distance(tile, foe_tile) == Some(ring) {
                // The rival has just the gold the tile costs: nothing left to buy a peace with.
                for _ in 0..600 {
                    if scene.game.execute(&scene.rules, Command::PurchaseTile { player: FOE, city: foe, tile }).is_ok() {
                        break;
                    }
                    scene.game.debug_earn(FOE, 5);
                }
            }
        }
    }
    scene.game.execute(&scene.rules, Command::EndTurn { player: FOE }).unwrap();
    let farm = scene.improvement("core:improvement/farm");
    let planted: Vec<(TileId, ImprovementId)> = classic_rules::math::tiles_in_hex_radius(&dims, foe_tile, 3)
        .into_iter()
        .filter(|t| scene.owned_by(*t) == Some(FOE) && dims.distance(*t, foe_tile).is_some_and(|d| d >= 2))
        .map(|t| (t, farm))
        .collect();
    eprintln!("planted {} farms on the rival's land", planted.len());
    scene.game.debug_set_improvement_batch(&scene.rules, &planted);
    let everything: Vec<TileId> = (0..tiles).map(TileId).collect();
    scene.game.debug_reveal(AI, &everything);
    scene.game.debug_meet(AI, FOE);
    scene.game.debug_meet(FOE, AI);
    if war {
        scene.game.debug_force_war(&scene.rules, FOE, AI);
    }
    scene
}

/// The rounds pass with nobody playing (the season of war opens at round 30).
fn pass_rounds(scene: &mut Scene, rounds: u32) {
    let start = scene.game.view().auth.turn().round;
    while scene.game.view().auth.turn().round < start + rounds {
        let active = scene.game.view().auth.active_player();
        scene.game.execute(&scene.rules, Command::EndTurn { player: active }).unwrap();
    }
}

/// Plays `rounds` rounds with the AI of `variant` on the seat of the AI and the passive rival (its
/// turn ends at once), keeping the rival's city in the AI's memory as a scout would.
fn play(scene: &mut Scene, variant: Variant, humans: Vec<PlayerId>, rounds: u32) -> AiDriver<classic_rules::classic::Classic, ClassicMind> {
    let settings = AiSettings::for_rules(&scene.rules, scene.rules.default_difficulty());
    let mut driver = AiDriver::with_mind(settings, humans, ClassicMind::new(variant));
    driver.set_trace_capacity(1_000_000);
    // The rival's war has just begun for its memory: it holds it for the minimum length.
    let now = scene.game.view().auth.turn().round;
    if scene.game.view().auth.diplomacy().at_war(AI, FOE) {
        let memory = driver.state_mut(FOE, now).relation_mut(AI);
        memory.at_war = true;
        memory.last_change_round = Some(now);
        let memory = driver.state_mut(AI, now).relation_mut(FOE);
        memory.at_war = true;
        memory.last_change_round = Some(now);
    }
    let foe_tile = scene.tile(14, 10);
    let start = scene.game.view().auth.turn().round;
    while scene.game.view().auth.turn().round < start + rounds && scene.game.winner().is_none() {
        let active = scene.game.view().auth.active_player();
        if active == FOE {
            scene.game.execute(&scene.rules, Command::EndTurn { player: FOE }).unwrap();
        } else {
            let round = scene.game.view().auth.turn().round;
            driver.state_mut(AI, round).see_city(classic_rules::ai::state::SeenCity { tile: foe_tile, owner: FOE, population: 4, last_seen_round: round });
            driver.run(&mut scene.game, &scene.rules, u32::MAX);
            if std::env::var("RAID_DEBUG").is_ok() {
                let auth = scene.game.view().auth;
                let dims = auth.map().dimensions;
                let horsemen: Vec<_> = auth.units().iter_handles().filter(|(_, u)| u.owner == AI && u.kind == scene.kind("core:unit/horseman")).map(|(_, u)| (dims.to_coord(u.tile).map(|c| (c.q, c.r)), u.hp)).collect();
                let routes: Vec<_> = auth.module().trade().routes().iter().map(|r| dims.to_coord(r.unit_tile()).map(|c| (c.q, c.r))).collect();
                eprintln!("r{round} horsemen {horsemen:?} routes {routes:?} war {}", auth.diplomacy().at_war(AI, FOE));
            }
        }
    }
    driver
}

fn applied(driver: &AiDriver<classic_rules::classic::Classic, ClassicMind>, reason: Reason) -> usize {
    driver.trace().entries().filter(|e| e.reason == reason && e.outcome == Outcome::Applied).count()
}

fn pillaged_tiles(scene: &Scene) -> usize {
    let auth = scene.game.view().auth;
    (0..auth.map().tile_count() as u32).filter(|t| auth.map().pillaged(TileId(*t))).count()
}

/// A raid group of the `raids` variant, in a war the rival began and with a rival whose land is
/// planted and open, pillages its improvements and goes home with most of the group alive.
#[test]
fn a_raid_group_pillages_a_border_region_and_escapes() {
    let mut scene = scene(true);
    war_probe::enable(true);
    let driver = play(&mut scene, Variant::Raids, vec![], 40);
    let probe = war_probe::take();
    war_probe::enable(false);
    eprintln!("round {} winner {:?} active {:?} counters {:?}", scene.game.view().auth.turn().round, scene.game.winner(), scene.game.view().auth.active_player(), driver.counters());
    let formed = applied_or_any(&driver, Reason::FormRaid);
    let pillages = applied(&driver, Reason::Pillage);
    let tiles = pillaged_tiles(&scene);
    let horsemen = scene.kind("core:unit/horseman");
    let alive = scene.game.view().auth.units().iter_handles().filter(|(_, u)| u.owner == AI && u.kind == horsemen).count();
    eprintln!("raids formed {formed}, pillages {pillages}, tiles pillaged {tiles}, horsemen alive {alive} of 3, probe {:?}", probe.raids);
    assert!(formed >= 1, "a raid forms");
    assert!(pillages >= 2 && tiles >= 2, "the group pillages: {pillages} orders, {tiles} tiles");
    assert!(alive >= 2, "the group escapes: {alive} of 3 horsemen alive");
}

fn applied_or_any(driver: &AiDriver<classic_rules::classic::Classic, ClassicMind>, reason: Reason) -> usize {
    driver.trace().entries().filter(|e| e.reason == reason).count()
}

/// The game's AI does not raid: the same scene with `standard` pillages nothing.
#[test]
fn the_games_ai_does_not_raid() {
    let mut scene = scene(true);
    let driver = play(&mut scene, Variant::Standard, vec![], 40);
    assert_eq!(applied_or_any(&driver, Reason::FormRaid), 0);
    assert_eq!(applied(&driver, Reason::Pillage), 0);
    assert_eq!(pillaged_tiles(&scene), 0);
}

/// The raid is chosen where the conquest is not: every estimate of the war on the city is
/// negative, and the raid still forms.
#[test]
fn a_raid_is_chosen_where_the_conquest_is_negative() {
    let mut scene = scene(true);
    pass_rounds(&mut scene, 34);
    // Five defenders of the rival round its city: a conquest of it does not pay.
    for (q, r) in [(14, 10), (13, 10), (15, 10), (14, 9), (14, 11)] {
        let tile = scene.tile(q, r);
        let spearman = scene.kind("core:unit/spearman");
        scene.game.debug_spawn_unit(&scene.rules, FOE, tile, spearman);
    }
    war_probe::enable(true);
    let driver = play(&mut scene, Variant::Raids, vec![], 3);
    let probe = war_probe::take();
    war_probe::enable(false);
    eprintln!("estimates {:?}", probe.estimates.iter().map(|e| e.value).collect::<Vec<_>>());
    assert!(applied_or_any(&driver, Reason::FormRaid) >= 1, "a raid forms");
    assert!(!probe.estimates.is_empty(), "the conquest was priced");
    assert!(probe.estimates.iter().all(|e| e.value < 0), "and every estimate of it is negative");
    assert!(probe.raids.chosen_negative_conquest >= 1);
    assert!(probe.launched.is_empty(), "no operation set out");
}

/// A player the host named a human is attacked by no new aim: no raid forms in a war that runs and
/// none is declared at peace; the same peace scene against a player that plays itself ends in a
/// war declared for a raid.
#[test]
fn no_new_aggressive_aim_against_a_human() {
    let mut scene = scene(true);
    let driver = play(&mut scene, Variant::Raids, vec![FOE], 40);
    assert_eq!(applied_or_any(&driver, Reason::FormRaid), 0, "no raid on a human in a war that runs");
    assert_eq!(applied(&driver, Reason::Pillage), 0);
    // At peace, past the season of war: a rival that plays itself is raided, a human is not.
    let mut other = self::scene(false);
    pass_rounds(&mut other, 44);
    war_probe::enable(true);
    let driver = play(&mut other, Variant::Raids, vec![], 6);
    let probe = war_probe::take();
    war_probe::enable(false);
    assert!(applied(&driver, Reason::DeclareWar) >= 1, "the war is declared for a raid");
    assert!(probe.declarations.iter().all(|d| d.3), "and every declaration is the raid's");
    assert!(applied_or_any(&driver, Reason::FormRaid) >= 1);
    let mut peace = self::scene(false);
    pass_rounds(&mut peace, 44);
    let driver = play(&mut peace, Variant::Raids, vec![FOE], 20);
    assert_eq!(applied(&driver, Reason::DeclareWar), 0, "no war on a human at peace");
    assert_eq!(applied_or_any(&driver, Reason::FormRaid), 0);
    assert!(!peace.game.view().auth.diplomacy().at_war(AI, FOE));
}

/// A trade route of the rival's own cities runs through its land; a raider that meets the
/// route's unit plunders it.
#[test]
fn a_trade_route_is_plundered() {
    let mut scene = scene(false);
    // The rival's second city and a food route between its two cities.
    let other_tile = scene.tile(14, 16);
    scene.game.debug_found_city(&scene.rules, FOE, other_tile);
    scene.learn(FOE, "core:tech/animal-husbandry");
    let granary = classic_rules::ids::BuildingKindId(scene.rules.buildings.iter().position(|b| b.key == "core:building/granary").unwrap() as u16);
    let home = scene.game.view().auth.indexes().city_on_tile(scene.tile(14, 10)).unwrap();
    let other = scene.game.view().auth.indexes().city_on_tile(other_tile).unwrap();
    scene.game.debug_add_building(&scene.rules, home, granary);
    let caravan = scene.kind("core:unit/caravan");
    let home_tile = scene.tile(14, 10);
    let unit = scene.game.debug_spawn_unit(&scene.rules, FOE, home_tile, caravan);
    scene.game.execute(&scene.rules, Command::EndTurn { player: AI }).unwrap();
    scene.game.execute(&scene.rules, Command::EstablishTradeRoute { unit, destination: other, kind: RouteKind::Food }).unwrap();
    assert_eq!(scene.game.view().auth.module().trade().routes().len(), 1);
    scene.game.debug_force_war(&scene.rules, FOE, AI);
    let gold = scene.game.view().auth.players().treasury_of(AI);
    let driver = play(&mut scene, Variant::Raids, vec![], 30);
    if std::env::var("RAID_DEBUG").is_ok() {
        for e in driver.trace().entries().filter(|e| matches!(e.reason, Reason::MakePeace | Reason::ProposeDeal | Reason::AcceptDeal | Reason::RefuseDeal | Reason::FormRaid | Reason::EndRaid) || matches!(e.command, Some(Command::MakePeace { .. } | Command::ProposeDeal { .. } | Command::RespondDeal { .. }))) {
            eprintln!("  trace {e}");
        }
    }
    let plundered = scene.game.view().auth.module().trade().routes().is_empty() || applied(&driver, Reason::Plunder) >= 1;
    eprintln!("routes left {}, plunder orders {}, gold {} -> {}", scene.game.view().auth.module().trade().routes().len(), applied(&driver, Reason::Plunder), gold, scene.game.view().auth.players().treasury_of(AI));
    assert!(plundered, "the route was plundered");
    let _ = UnitTask::Raid { target: home_tile };
}
