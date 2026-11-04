//! Tests of the platform's map-object drawing (`render::gpu::world_objects`) on classic games:
//! improvements, resources, wonders and the city kit stand where the classic rules put them. They
//! sit here, not in `render`, because they need the classic module and the platform builds without
//! any module (docs/split-layout.md).
use render::gpu::buffers::HexInstance;
use render::gpu::world_objects::*;
use render::scene::terrain::fields::TerrainGeometry;
use render::world::state::RenderWorld;
use render::world_models::{Ground, WorldBook};
use render::scene::heightfield::build_heightfield;
use render::scene::style::{compile, default_style};
use render::scene::terrain::evaluator::sample_surface;
use render::scene::terrain::fields::build_terrain_geometry;
use render::world::state::{CityMarker, TerrainCell};
use kernel::ids::TileId;
use classic_rules::CompiledRuleset;

const W: u32 = 9;
const PLAINS: u16 = 0;

fn rules() -> CompiledRuleset {
    classic_rules::compile_rules(classic_content::core::core_rules()).expect("BNW rules")
}

fn index(keys: impl Iterator<Item = String>, key: &str) -> u16 {
    keys.enumerate().find(|(_, k)| k == key).map(|(i, _)| i as u16).expect(key)
}

fn world() -> RenderWorld {
    let n = (W * W) as usize;
    RenderWorld {
        width: W,
        height: W,
        visible: vec![1; n],
        explored: vec![1; n],
        forest: vec![0; n],
        river: vec![0; n],
        road: vec![0; n],
        improvements: vec![None; n],
        natural_wonders: vec![None; n],
        terrain: (0..n as u32)
            .map(|i| TerrainCell { tile: TileId(i), terrain: PLAINS, owner: None, resource: None })
            .collect(),
        ..Default::default()
    }
}

fn geometry(world: &RenderWorld) -> TerrainGeometry {
    let style = compile(default_style()).unwrap();
    build_terrain_geometry(world, &style, &build_heightfield(world, &style))
}

fn objects(rules: &CompiledRuleset) -> WorldObjects {
    let mut objects = WorldObjects::default();
    objects.set_book(WorldBook::from_rules(rules));
    objects
}

/// Model ids drawn on a tile, from built props.
fn ids_at(objects: &WorldObjects, instances: &[HexInstance], world: &RenderWorld, tile: u32) -> Vec<&'static str> {
    let (x, z) = tile_center(world, tile);
    let mut out = Vec::new();
    for (model, range) in objects.props.placed() {
        for instance in &instances[range] {
            if (instance.offset[0] - x).abs() < 1e-4 && (instance.offset[2] - z).abs() < 1e-4 {
                out.push(model.model().id);
            }
        }
    }
    out.sort();
    out
}

#[test]
fn tiles_draw_their_improvement_variant_resource_and_natural_wonder() {
    let rules = rules();
    let mut world = world();
    let farm = index(rules.improvements.iter().map(|x| x.key.clone()), "core:improvement/farm");
    let wheat = index(rules.resources.iter().map(|x| x.key.clone()), "core:resource/wheat");
    let fuji = index(rules.natural_wonders.iter().map(|x| x.key.clone()), "core:natural-wonder/mt-fuji");
    world.improvements[20] = Some(farm);
    world.terrain[20].resource = Some(wheat);
    world.unlocked_resources = vec![wheat];
    world.terrain[22].resource = Some(wheat);
    world.natural_wonders[40] = Some(fuji);
    world.improvements[40] = Some(farm);
    let geom = geometry(&world);
    let mut objects = objects(&rules);
    let props = objects.build_props(&world, &geom);
    assert_eq!(ids_at(&objects, &props, &world, 20), ["improvement/farm-plains", "resource/wheat"]);
    assert_eq!(ids_at(&objects, &props, &world, 22), ["resource/wheat"]);
    assert_eq!(ids_at(&objects, &props, &world, 40), ["natural-wonder/mt-fuji"], "the wonder covers its tile");
    assert_eq!(props.len(), 4);
    let again = objects.build_props(&world, &geom);
    assert_eq!(bytemuck::cast_slice::<_, u8>(&props), bytemuck::cast_slice::<_, u8>(&again), "deterministic");
    // A resource the observer cannot see yet is not drawn.
    world.unlocked_resources.clear();
    let hidden = objects.build_props(&world, &geom);
    assert_eq!(ids_at(&objects, &hidden, &world, 22), Vec::<&str>::new());
}

#[test]
fn unknown_tiles_draw_nothing_and_remembered_tiles_keep_their_objects() {
    let rules = rules();
    let mut world = world();
    let mine = index(rules.improvements.iter().map(|x| x.key.clone()), "core:improvement/mine");
    world.improvements[30] = Some(mine);
    world.improvements[31] = Some(mine);
    world.visible[30] = 0; // remembered: explored, not visible now
    world.visible[31] = 0;
    world.explored[31] = 0; // unknown
    let geom = geometry(&world);
    let mut objects = objects(&rules);
    let props = objects.build_props(&world, &geom);
    assert_eq!(ids_at(&objects, &props, &world, 30), ["improvement/mine"]);
    assert!(ids_at(&objects, &props, &world, 31).is_empty());
    assert!(props.iter().all(|i| i.fog == FOG_KNOWN), "the GPU fog field dims remembered tiles");
}

#[test]
fn objects_stand_on_the_final_surface() {
    let rules = rules();
    let mut world = world();
    for (i, cell) in world.terrain.iter_mut().enumerate() {
        cell.terrain = if i % 3 == 0 { 2 } else { PLAINS }; // hills among plains: uneven ground
    }
    let mine = index(rules.improvements.iter().map(|x| x.key.clone()), "core:improvement/mine");
    let farm = index(rules.improvements.iter().map(|x| x.key.clone()), "core:improvement/farm");
    for t in [30, 31, 32, 39, 40, 41] {
        world.improvements[t] = Some(if t % 2 == 0 { farm } else { mine });
    }
    let geom = geometry(&world);
    let mut objects = objects(&rules);
    let props = objects.build_props(&world, &geom);
    let mut plinths = 0;
    for (model, range) in objects.props.placed() {
        let m = model.model();
        for instance in &props[range] {
            match m.ground {
                Ground::Upright if m.kind == render::world_models::WorldKind::Support => plinths += 1,
                Ground::Upright if m.seat == render::world_models::Seat::Bare && render::debug_view::slope_mode_new() => {
                    // A bare model is sheared onto the ground: no steeper than the limit, its base never
                    // hangs more than the allowance above the ground under any contact.
                    assert!(instance.slope[0].hypot(instance.slope[1]) <= place::BARE_SLOPE_MAX + 1e-4, "{}", m.id);
                    let allowance = place::BARE_HANG + (-m.low.min(0.0)) * instance.scale_height[1];
                    for c in m.contacts {
                        let p = instance.transform([c[0], 0.0, c[1]]);
                        let g = sample_surface(&geom, p[0], p[2]).map_or(p[1], |s| s.y);
                        assert!(p[1] - g <= allowance + 1e-3, "{} hangs {} above the ground", m.id, p[1] - g);
                    }
                }
                Ground::Upright if m.seat == render::world_models::Seat::Bare => {
                    // A bare model stands between the lowest ground under its contacts and
                    // a share of its radius above it.
                    let low = m
                        .contacts
                        .iter()
                        .filter_map(|c| {
                            let p = instance.transform([c[0], 0.0, c[1]]);
                            sample_surface(&geom, p[0], p[2]).map(|s| s.y)
                        })
                        .fold(f32::INFINITY, f32::min);
                    let reach = place::BARE_REACH * m.foot[2] * instance.scale_height[0];
                    assert!(instance.offset[1] >= low - 1e-4, "{} sinks below its lowest contact", m.id);
                    assert!(instance.offset[1] <= low + reach + 1e-4, "{} hangs {} above it", m.id, instance.offset[1] - low);
                }
                Ground::Upright => {
                    // The ground never stands more than the foundation course above the base.
                    for c in m.contacts {
                        let p = instance.transform([c[0], 0.0, c[1]]);
                        let g = sample_surface(&geom, p[0], p[2]).map_or(p[1], |s| s.y);
                        assert!(
                            g - instance.offset[1] <= place::SINK + place::plinth_max() + 1e-4,
                            "{} is buried {} deep",
                            m.id,
                            g - instance.offset[1]
                        );
                    }
                }
                Ground::Plane => {
                    // The plane runs close to the ground under its contacts and never steeper than the limit.
                    assert!(instance.slope[0].hypot(instance.slope[1]) <= place::SLOPE_MAX + 1e-4, "{}", m.id);
                    for c in m.contacts {
                        let p = instance.transform([c[0], m.low, c[1]]);
                        let g = sample_surface(&geom, p[0], p[2]).map_or(p[1], |s| s.y);
                        assert!(p[1] - g <= place::PLANE_HANG + 1e-3, "{} hangs {} above the ground", m.id, p[1] - g);
                        assert!(g - p[1] < 0.35, "{} is buried {}", m.id, g - p[1]);
                    }
                }
                Ground::Water => {}
            }
        }
    }
    assert_eq!(plinths, 0, "improvements are wide compositions: centred on the ground, no plinth");
}

#[test]
fn buildings_on_uneven_ground_stand_on_plinths() {
    let rules = rules();
    let mut world = world();
    for (i, cell) in world.terrain.iter_mut().enumerate() {
        cell.terrain = if i % 2 == 0 { 2 } else { PLAINS }; // a hill beside every plain
    }
    world.cities.push(CityMarker {
        tile: TileId(40),
        owner: 1,
        name: "Thebes".into(),
        population: Some(20),
        buildings: Vec::new(),
        era: 3,
        showpieces: Vec::new(),
        unworked: Vec::new(),
    });
    let geom = geometry(&world);
    let mut objects = objects(&rules);
    let cities = objects.build_cities(&world, &geom);
    let again = objects.build_cities(&world, &geom);
    assert_eq!(bytemuck::cast_slice::<_, u8>(&cities), bytemuck::cast_slice::<_, u8>(&again), "deterministic");
    let mut buildings = 0;
    let mut plinths = 0;
    for (model, range) in objects.cities.placed() {
        let m = model.model();
        if m.kind == render::world_models::WorldKind::Support {
            plinths += range.len();
            // The lowest tier of a plinth reaches the lowest ground under its widest disc (the
            // upper tier rests on it, so it is not checked on its own).
            let mut stacks: std::collections::BTreeMap<(i32, i32), (f32, f32, f32, f32)> = Default::default();
            for instance in &cities[range] {
                let key = ((instance.offset[0] * 1000.0) as i32, (instance.offset[2] * 1000.0) as i32);
                let e = stacks.entry(key).or_insert((instance.offset[0], instance.offset[2], f32::INFINITY, 0.0));
                e.2 = e.2.min(instance.offset[1]);
                e.3 = e.3.max(instance.scale_height[0]);
            }
            for (x, z, bottom, r) in stacks.into_values() {
                for k in 0..8 {
                    let a = std::f32::consts::TAU * k as f32 / 8.0;
                    if let Some(g) = sample_surface(&geom, x + r * a.cos(), z + r * a.sin()) {
                        assert!(bottom <= g.y + 0.02, "plinth bottom {bottom} over ground {} at ring point {k}", g.y);
                    }
                }
            }
            continue;
        }
        for instance in &cities[range] {
            buildings += 1;
            for c in m.contacts {
                let p = instance.transform([c[0], 0.0, c[1]]);
                let g = sample_surface(&geom, p[0], p[2]).map_or(p[1], |s| s.y);
                assert!(g - instance.offset[1] <= place::SINK + place::plinth_max() + 1e-4, "{} is buried", m.id);
            }
        }
    }
    assert!(buildings > 1, "the city draws its center and houses");
    assert!(plinths > 0, "houses on a hill edge stand on plinths");
}

#[test]
fn cities_compose_their_tile_and_wonders_take_a_ring_tile() {
    let rules = rules();
    let mut world = world();
    let pyramids = index(rules.buildings.iter().map(|x| x.key.clone()), "core:building/pyramids");
    let walls = index(rules.buildings.iter().map(|x| x.key.clone()), "core:building/walls");
    let farm = index(rules.improvements.iter().map(|x| x.key.clone()), "core:improvement/farm");
    for t in 0..(W * W) as usize {
        world.improvements[t] = Some(farm);
    }
    world.cities.push(CityMarker {
        tile: TileId(40),
        owner: 1,
        name: "Thebes".into(),
        population: Some(8),
        buildings: vec![walls.min(pyramids), walls.max(pyramids)],
        era: 0,
        showpieces: Vec::new(),
        unworked: Vec::new(),
    });
    let geom = geometry(&world);
    let mut objects = objects(&rules);
    let cities = objects.build_cities(&world, &geom);
    let ids: Vec<&str> = objects.cities.placed().map(|(m, _)| m.model().id).collect();
    assert!(ids.contains(&"wonder/pyramids") && ids.contains(&"city/walls#0"), "{ids:?}");
    let (pyramids_model, range) = objects.cities.placed().find(|(m, _)| m.model().id == "wonder/pyramids").unwrap();
    let scale = city::wonder_scale(pyramids_model);
    assert!(scale < 1.0, "the pyramids fill their hex and are scaled into it");
    assert_eq!(cities[range.start].scale_height, [scale, scale], "the wonder is drawn at its scale");
    let supports = objects.cities.placed().filter(|(m, _)| m.model().kind == render::world_models::WorldKind::Support).map(|(_, r)| r.len()).sum::<usize>();
    assert_eq!(
        cities.len() - supports,
        1 + render::world_models::wall_parts().count() + render::world_models::house_count(8) + 1,
        "center, wall parts, houses and the wonder"
    );
    let props = objects.build_props(&world, &geom);
    assert!(ids_at(&objects, &props, &world, 40).is_empty(), "no farm under the city");
    let wonder = objects.occupied.iter().copied().find(|t| *t != 40).expect("wonder site");
    assert!(ids_at(&objects, &props, &world, wonder).is_empty(), "no farm under the wonder");
    assert_eq!(props.len(), (W * W) as usize - 2);
}

fn model_view(camera: &render::camera::CameraController) -> ModelView {
    let distance = camera.distance();
    ModelView {
        planes: camera.frustum_planes(16.0 / 9.0),
        px_per_unit: camera.pixels_per_unit(1080.0),
        eye: [camera.x, distance, camera.y + distance],
        focus: distance * std::f32::consts::SQRT_2,
    }
}

#[test]
fn draws_skip_cells_outside_the_view_and_use_coarser_lods_far_away() {
    let rules = rules();
    let n = 48u32;
    let tiles = (n * n) as usize;
    let farm = index(rules.improvements.iter().map(|x| x.key.clone()), "core:improvement/farm");
    let world = RenderWorld {
        width: n,
        height: n,
        visible: vec![1; tiles],
        explored: vec![1; tiles],
        road: vec![0; tiles],
        improvements: vec![Some(farm); tiles],
        natural_wonders: vec![None; tiles],
        terrain: (0..n * n)
            .map(|i| TerrainCell { tile: TileId(i), terrain: PLAINS, owner: None, resource: None })
            .collect(),
        ..Default::default()
    };
    let geom = geometry(&world);
    let mut objects = objects(&rules);
    let props = objects.build_props(&world, &geom);
    let mut camera = render::camera::CameraController::fit_map(n, n);
    let (x, z) = tile_center(&world, 2 * n + 2);
    (camera.x, camera.y, camera.zoom) = (x, z, 4.0);
    let close = objects.props.draws(&model_view(&camera));
    let drawn: usize = close.iter().map(|d| d.instances.len()).sum();
    assert!(drawn > 0 && drawn < props.len() / 2, "culled to the corner: {drawn} of {}", props.len());
    camera.zoom = 0.6;
    let far: usize = objects.props.draws(&model_view(&camera)).iter().map(|d| d.triangles()).sum();
    let near_per_farm = close.iter().map(|d| d.triangles()).sum::<usize>() / drawn;
    assert!(far < near_per_farm * props.len(), "a zoomed-out view draws far markers");
}

#[test]
fn stone_and_marble_remain_semantic_resource_models() {
    let rules = rules();
    let mut world = world();
    for (tile, name) in [(20, "stone"), (22, "marble")] {
        let key = format!("core:resource/{name}");
        let resource = index(rules.resources.iter().map(|x| x.key.clone()), &key);
        world.terrain[tile].resource = Some(resource);
        world.unlocked_resources.push(resource);
    }
    let geom = geometry(&world);
    let mut objects = objects(&rules);
    let props = objects.build_props(&world, &geom);
    assert_eq!(ids_at(&objects, &props, &world, 20), ["resource/stone"]);
    assert_eq!(ids_at(&objects, &props, &world, 22), ["resource/marble"]);
    assert!(ids_at(&objects, &props, &world, 21).is_empty());
}

/// The late strategic deposits have models, and a remembered tile (explored, out of sight)
/// draws them once the observer's technology unlocks them (R355).
#[test]
fn late_strategic_resources_draw_on_remembered_tiles_with_their_technology() {
    let rules = rules();
    let mut world = world();
    let names = [(20, "uranium"), (22, "oil"), (24, "aluminum")];
    for (tile, name) in names {
        let key = format!("core:resource/{name}");
        let resource = index(rules.resources.iter().map(|x| x.key.clone()), &key);
        world.terrain[tile].resource = Some(resource);
        world.visible[tile] = 0; // remembered
    }
    let geom = geometry(&world);
    let mut objects = objects(&rules);
    let hidden = objects.build_props(&world, &geom);
    for (tile, _) in names {
        assert!(ids_at(&objects, &hidden, &world, tile as u32).is_empty(), "locked: not drawn");
    }
    for (tile, name) in names {
        let key = format!("core:resource/{name}");
        world.unlocked_resources.push(index(rules.resources.iter().map(|x| x.key.clone()), &key));
        let _ = tile;
    }
    let props = objects.build_props(&world, &geom);
    for (tile, name) in names {
        let want = format!("resource/{name}");
        assert_eq!(ids_at(&objects, &props, &world, tile as u32), vec![want.as_str()]);
    }
}

/// The generator places no sea resources (R370), but when a game has one - by a scenario, a save or a
/// later generator - it is drawn on its water tile: fish with fishing boats, whales and pearls without
/// them, and the tile stays drawn once the observer's sight has moved on (explored, not visible).
#[test]
fn sea_resources_on_a_coast_tile_are_drawn_visible_and_remembered() {
    let rules = rules();
    let mut world = world();
    let water = world.water_code();
    // A sea: rows 3..=7 are water, rows 0..=2 are land; the resources lie on row 3, the coast.
    for i in 3 * W as usize..world.terrain.len() {
        world.terrain[i].terrain = water;
    }
    let boats = index(rules.improvements.iter().map(|x| x.key.clone()), "core:improvement/fishing-boats");
    let sea = |name: &str| index(rules.resources.iter().map(|x| x.key.clone()), &format!("core:resource/{name}"));
    let (fish, whales, pearls) = (sea("fish"), sea("whales"), sea("pearls"));
    let (a, b, c) = (3 * W as usize + 2, 3 * W as usize + 4, 3 * W as usize + 6);
    world.terrain[a].resource = Some(fish);
    world.improvements[a] = Some(boats);
    world.terrain[b].resource = Some(whales);
    world.terrain[c].resource = Some(pearls);
    world.unlocked_resources = vec![fish, whales, pearls];
    let geom = geometry(&world);
    let mut objects = objects(&rules);
    let props = objects.build_props(&world, &geom);
    assert_eq!(ids_at(&objects, &props, &world, a as u32), ["improvement/fishing-boats", "resource/fish"]);
    assert_eq!(ids_at(&objects, &props, &world, b as u32), ["resource/whales"]);
    assert_eq!(ids_at(&objects, &props, &world, c as u32), ["resource/pearls"]);
    // Every sea object stands at the sea level of its tile (this hand-built world has no visual surface,
    // so the level is the heightfield's), not on the land beside it.
    let (lx, lz) = tile_center(&world, 2 * W + 2);
    let land = sample_surface(&geom, lx, lz).unwrap().y;
    for (model, range) in objects.props.placed() {
        if !model.model().id.starts_with("resource/") {
            continue;
        }
        for instance in &props[range] {
            let sea = sample_surface(&geom, instance.offset[0], instance.offset[2]).unwrap().y;
            assert!((instance.offset[1] - sea).abs() < 1e-4, "{} off the sea level", model.model().id);
            assert!(instance.offset[1] < land, "{} stands on the shore, not in the sea", model.model().id);
        }
    }
    // The sight moves on: the tiles are remembered, and the sea resources stay.
    for tile in [a, b, c] {
        world.visible[tile] = 0;
    }
    let remembered = objects.build_props(&world, &geom);
    assert_eq!(ids_at(&objects, &remembered, &world, a as u32), ["improvement/fishing-boats", "resource/fish"]);
    assert_eq!(ids_at(&objects, &remembered, &world, c as u32), ["resource/pearls"]);
    assert!(remembered.iter().all(|i| i.fog == FOG_KNOWN), "remembered objects are dimmed by the fog field");
}

/// A generated map draws its sea deposits: the fish and the sea oil of a real new game stand on their
/// water tiles as the fish and oil models (oil once the observer knows Biology), on the first coast
/// the generator gives a start and on a map of every script.
#[test]
fn a_generated_map_draws_fish_and_sea_oil_on_the_coast() {
    use render::world::projection::rebuild_revealed_world;
    use kernel::ids::PlayerId;
    use classic_rules::scenario::{generate_new_game_with, MapScript, NewGameOptions, ScenarioConfig};
    let rules = rules();
    let (fish_key, oil_key) = ("core:resource/fish", "core:resource/oil");
    let oil_def = rules.resources.iter().find(|r| r.key == oil_key).unwrap();
    let biology = oil_def.required_tech.expect("oil needs a technology");
    let cfg = ScenarioConfig { seed: 3, players: 2, width: 40, height: 24, cities: 0, units: 0, water_ratio: MapScript::Continents.water_ratio(25) };
    let mut game = generate_new_game_with(&rules, &cfg, Some(0), NewGameOptions { city_states: 4, script: MapScript::Continents, ..Default::default() });
    game.debug_learn(&rules, PlayerId(0), biology);
    let world = rebuild_revealed_world(game.view(), PlayerId(0), &rules);
    let map = game.view().auth.map();
    let on_water = |key: &str| -> Vec<u32> {
        (0..world.terrain.len() as u32)
            .filter(|&t| map.is_water(TileId(t)) && matches!(map.resource(TileId(t)), Some(Some(r)) if rules.resources[r.0 as usize].key == key))
            .collect()
    };
    let (fish, oil) = (on_water(fish_key), on_water(oil_key));
    assert!(!fish.is_empty() && !oil.is_empty(), "the map has fish ({}) and sea oil ({})", fish.len(), oil.len());
    let geom = geometry(&world);
    let mut objects = objects(&rules);
    let props = objects.build_props(&world, &geom);
    for tile in &fish {
        assert!(ids_at(&objects, &props, &world, *tile).contains(&"resource/fish"), "fish on tile {tile}");
    }
    for tile in &oil {
        assert!(ids_at(&objects, &props, &world, *tile).contains(&"resource/oil"), "sea oil on tile {tile}");
    }
}
