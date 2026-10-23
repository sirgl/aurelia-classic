//! Offscreen captures of the representative 25k map for visual checks of the terrain
//! level of detail, borders and shadows: game and strategic zoom, LOD off and on.
//!
//!   cargo run --release --offline -p aurelia-render --example terrain_shots -- <out dir> [observer|revealed]
//!
//! Writes <out dir>/<knowledge>-z<zoom>-lod_<off|on>.png (1664x936), the camera over the
//! first unit of player 0 (game zoom) and over the map centre (strategic zooms).
use classic_rules::ids::{PlayerId, TileId};
use classic_rules::scenario::{generate, ScenarioConfig};
use render::camera::CameraController;
use render::gpu::context::GpuCore;
use render::gpu::terrain_gpu::TerrainSync;
use render::renderer::{FrameFlags, Renderer};
use render::world::projection::rebuild_render_world;
use render::RenderWorld;

const SIZE: (u32, u32) = (1664, 936);

fn reveal_all(world: &mut RenderWorld, game: &classic_rules::GameState) {
    let map = game.view().auth.map();
    for i in 0..world.terrain.len() {
        let tile = TileId(i as u32);
        world.visible[i] = 1;
        world.explored[i] = 1;
        world.terrain[i].terrain = map.terrain(tile).map(|t| t.0).unwrap_or(u16::MAX);
        world.terrain[i].owner = map.owner(tile).flatten().map(|p| p.0);
        world.forest[i] = map.feature(tile).flatten().is_some() as u8;
        world.river[i] = map.river(tile) as u8;
    }
    world.river_edges = map.river_edges().to_vec();
    world.touch_all();
}

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "/tmp".into());
    let knowledge = std::env::args().nth(2).unwrap_or_else(|| "revealed".into());
    let rules = classic_rules::compile_rules(classic_content::core::core_rules()).expect("core rules");
    let game = generate(&ScenarioConfig::representative_late_game(), &rules);
    let mut world = rebuild_render_world(game.view(), PlayerId(0), &rules);
    world.visual_surface = true;
    if knowledge == "revealed" {
        reveal_all(&mut world, &game);
    }
    let core = GpuCore::new_blocking().expect("GPU adapter");
    let layers = render::ground::layers();
    let mut renderer = Renderer::with_gpu(&core, wgpu::TextureFormat::Rgba8Unorm, &layers);
    renderer.set_unit_models(render::unit_models::UnitModelBook::from_rules(&rules));
    renderer.set_world_models(render::world_models::WorldBook::from_rules(&rules));
    renderer.set_terrain_sync(TerrainSync::Immediate);
    renderer.set_animation_time_override(Some(1.0));
    let unit = game.view().auth.indexes().units_of(PlayerId(0)).first().copied();
    let home = unit.and_then(|u| game.view().auth.units().get(u)).map(|u| u.core.tile).unwrap_or(TileId(0));
    let (ux, uz) = render::scene::layout::tile_to_world(home.0 % world.width, home.0 / world.width);
    let (cx, cz) = render::scene::layout::map_center(world.width, world.height);
    let flags = FrameFlags { grid: false, selected_city: None, yields: false };
    for (zoom, (x, z)) in [(2.25f32, (ux, uz)), (0.8, (cx, cz)), (0.5, (cx, cz))] {
        for lod in [false, true] {
            renderer.set_terrain_lod(lod);
            let camera = CameraController { x, y: z, zoom };
            // Levels follow the camera with hysteresis: settle before the capture.
            for _ in 0..3 {
                renderer.render_offscreen(&core, &world, &camera, SIZE.0, SIZE.1, flags);
            }
            let pixels = renderer.render_offscreen(&core, &world, &camera, SIZE.0, SIZE.1, flags);
            let stats = renderer.stats();
            let path = format!("{out}/{knowledge}-z{zoom}-lod_{}.png", if lod { "on" } else { "off" });
            image::save_buffer(&path, &pixels, SIZE.0, SIZE.1, image::ColorType::Rgba8).expect("write png");
            println!("{path}: terrain {} tris, {} draws, shadow tris {}", stats.terrain_tris, stats.draw_calls, stats.shadow_tris);
        }
    }
}
