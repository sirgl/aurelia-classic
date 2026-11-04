//! Headless render verification: a real GPU (no window) draws the world
//! into a texture, we read the pixels. Proves the wgpu pipe works end to end:
//! shaders compile, chunks/instances/grid arrive, colours on screen.

use classic_rules::ids::PlayerId;
use classic_rules::rules::SourceRules;
use classic_rules::scenario::{generate, ScenarioConfig};
use classic_rules::{compile_rules, GameState};
use render::camera::CameraController;
use render::gpu::context::GpuCore;
use render::renderer::Renderer;
use render::world::projection::rebuild_render_world;

fn is_water(px: &[u8]) -> bool {
    px[2] > 100 && px[2] as u16 > px[0] as u16 + 40 && px[2] as u16 > px[1] as u16 + 20
}

fn is_land(px: &[u8]) -> bool {
    // Green/brown/yellow terrains: r or g noticeably above the 0.04-0.1 background.
    px[0] > 50 || px[1] > 70
}

/// A 256x256 frame into bytes: texture -> draw -> readback.
fn render_pixels(
    core: &GpuCore,
    renderer: &mut Renderer,
    world: &render::RenderWorld,
    camera: &CameraController,
    grid: bool,
) -> Vec<u8> {
    let tex = core.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("test target"),
        size: wgpu::Extent3d {
            width: 256,
            height: 256,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
    renderer.draw(
        core,
        world,
        camera,
        &view,
        (256, 256),
        render::FrameFlags {
            grid,
            selected_city: None,
            yields: true,
        },
    );
    let pitch = 256 * 4;
    let buf = core.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("test readback"),
        size: (pitch * 256) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = core
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("test copy"),
        });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buf,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(pitch),
                rows_per_image: Some(256),
            },
        },
        wgpu::Extent3d {
            width: 256,
            height: 256,
            depth_or_array_layers: 1,
        },
    );
    core.queue.submit(std::iter::once(encoder.finish()));
    let slice = buf.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    let _ = core.device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: None,
    });
    rx.recv().unwrap().unwrap();
    let data = slice.get_mapped_range().unwrap().to_vec();
    buf.unmap();
    data
}

#[test]
fn headless_renders_terrain_and_water() {
    // Without an adapter (CPU-only CI): SKIP, not fail: render correctness
    // is checked by the CPU tests of meshes/projection, this one is only the GPU end of the pipe.
    let core = match GpuCore::new_blocking() {
        Ok(core) => core,
        Err(e) => {
            println!("SKIP-GPU: {}", e);
            return;
        }
    };
    println!("adapter: {:?}", core.adapter_info());
    let rules = compile_rules(SourceRules::default()).unwrap();
    let cfg = ScenarioConfig {
        seed: 5,
        players: 2,
        width: 48,
        height: 32,
        cities: 5,
        units: 20,
        water_ratio: 25,
    };
    let game = generate(&cfg, &rules);
    let world = rebuild_render_world(game.view(), PlayerId(0), &rules);
    assert_eq!(world.terrain.len(), 48 * 32);

    let mut renderer = {
        let layers = render::ground::layers();
        Renderer::with_gpu(&core, wgpu::TextureFormat::Rgba8Unorm, &layers)
    };
    assert!(renderer.is_gpu());
    let mut camera = CameraController::fit_map(48, 32);
    camera.zoom = 0.35; // the whole map in 256px

    let _scenario_pixels = render_pixels(&core, &mut renderer, &world, &camera, true);
    let stats = renderer.stats();
    // 48x32 -> chunks 2x1 + units + cities + grid + borders + icons + rivers + mountains.
    // Empty batches are not drawn; units/cities get a draw per kind/tier (v36).
    // Grid/borders/icons/rivers/mountains are asserted below as non-empty.
    assert_eq!(stats.chunks_drawn, 2, "48x32 tiles are two terrain chunks");
    assert!(stats.feature_draws > 0, "feature chunks draw");
    let expected_draws = 2 + 4 + stats.feature_draws + stats.unit_draws + stats.city_draws;
    assert_eq!(
        stats.draw_calls, expected_draws,
        "2 chunks + grid + borders + icons + rivers + feature chunks + unit/city group draws"
    );
    if stats.unit_instances > 0 {
        assert!(stats.unit_draws > 0, "present units draw");
    }
    if stats.city_instances > 0 {
        assert!(stats.city_draws > 0, "present cities draw");
    }
    assert!(stats.border_segments > 0, "owned tiles draw borders");
    assert!(stats.river_segments > 0, "rivers draw channels");
    assert!(stats.feature_tris > 0, "mountains draw on hills");
    assert!(
        stats.icon_instances > 100,
        "yield icons cover explored land"
    );
    assert!(
        stats.terrain_tris > 48 * 32 * 6,
        "cliffs add tris beyond flat fans"
    );
    let known_tiles = world.terrain.iter().filter(|cell| {
        let i = cell.tile.0 as usize;
        world.explored.get(i).copied().unwrap_or(0) > 0 || world.visible.get(i).copied().unwrap_or(0) > 0
    }).count();
    assert!(known_tiles > 0);
    assert!(stats.grid_segments >= known_tiles * 3 && stats.grid_segments <= known_tiles * 6,
        "deduplicated grid covers known tiles only: {} edges for {known_tiles} tiles", stats.grid_segments);
    assert_eq!(stats.texture_layers, 7, "6 photos + procedural water");
    // The style<->textures boundary: water must lie on WATER_LAYER on both sides.
    assert_eq!(
        render::scene::style::WATER_LAYER,
        render::gpu::textures::WATER_LAYER
    );

    // A controlled, fully visible coast exercises the palette independently of
    // the random scenario's spawn visibility. Unknown paper is now brighter,
    // so the old explored-brighter-than-black-fog assertion is inappropriate.
    let mut palette = render::RenderWorld {
        width: 20, height: 20, layers: world.layers,
        visible: vec![1; 400], explored: vec![1; 400],
        terrain: (0..400).map(|i| render::world::state::TerrainCell {
            tile: classic_rules::ids::TileId(i), terrain: if i % 20 >= 10 { 4 } else { 1 },
            owner: None, resource: None,
        }).collect(),
        ..Default::default()
    };
    palette.touch_all();
    let mut palette_camera = CameraController::fit_map(20, 20);
    palette_camera.zoom = 0.65;
    let palette_pixels = render_pixels(&core, &mut renderer, &palette, &palette_camera, false);
    let water = palette_pixels.chunks_exact(4).filter(|p| is_water(p)).count();
    let land = palette_pixels.chunks_exact(4).filter(|p| is_land(p) && !is_water(p)).count();
    assert!(water > 100, "controlled visible coast must show blue water: {water}");
    assert!(land > 1000, "controlled visible coast must show land: {land}");
    palette.visible.fill(0);
    palette.explored.fill(0);
    palette.touch_all();
    let hidden_pixels = render_pixels(&core, &mut renderer, &palette, &palette_camera, false);
    let hidden_water = hidden_pixels.chunks_exact(4).filter(|p| is_water(p)).count();
    assert_eq!(hidden_water, 0, "unknown geography must not reveal blue sea");
    let changed_by_fog = palette_pixels.iter().zip(&hidden_pixels).filter(|(a,b)| a != b).count();
    assert!(changed_by_fog > 2000, "visibility must affect the actual GPU image");

    // The grid is proved differentially: frames with grid ON/OFF must differ
    // (the background is dark too, so an absolute threshold of dark pixels proves nothing).
    palette.visible.fill(1);
    palette.explored.fill(1);
    palette.touch_all();
    let data_on = render_pixels(&core, &mut renderer, &palette, &palette_camera, true);
    let data_off = render_pixels(&core, &mut renderer, &palette, &palette_camera, false);
    let changed = data_on
        .iter()
        .zip(data_off.iter())
        .filter(|(a, b)| a != b)
        .count();
    println!("grid-changed bytes: {}", changed);
    assert!(
        changed > 2000,
        "grid toggle must change pixels, got {}",
        changed
    );
    let _ = GameState::new;
    let _ = PlayerId(0);
}

/// The game draws only while the renderer has work left (`has_pending_work`): at rest the
/// work must run out, and a terrain change must bring it back until it is published.
#[test]
fn background_terrain_work_settles_so_the_window_can_idle() {
    let core = match GpuCore::new_blocking() {
        Ok(core) => core,
        Err(e) => {
            println!("SKIP-GPU: {}", e);
            return;
        }
    };
    let rules = compile_rules(SourceRules::default()).unwrap();
    let cfg = ScenarioConfig { seed: 5, players: 2, width: 48, height: 32, cities: 5, units: 20, water_ratio: 25 };
    let game = generate(&cfg, &rules);
    let mut world = rebuild_render_world(game.view(), PlayerId(0), &rules);
    let layers = render::ground::layers();
    let mut renderer = Renderer::with_gpu(&core, wgpu::TextureFormat::Rgba8Unorm, &layers);
    renderer.set_terrain_sync(render::gpu::terrain_gpu::TerrainSync::Background);
    renderer.set_terrain_lod(true);
    let mut camera = CameraController::fit_map(48, 32);
    camera.zoom = 0.35;
    let frames_to_settle = |renderer: &mut Renderer, world: &render::RenderWorld| {
        for frame in 1..=600 {
            render_pixels(&core, renderer, world, &camera, false);
            if !renderer.has_pending_work() {
                return frame;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("the renderer never runs out of work: the window would draw forever");
    };
    frames_to_settle(&mut renderer, &world);
    render_pixels(&core, &mut renderer, &world, &camera, false);
    assert!(!renderer.has_pending_work(), "a settled world stays settled");

    world.layers.terrain += 1;
    render_pixels(&core, &mut renderer, &world, &camera, false);
    assert!(renderer.has_pending_work(), "new terrain is built in the background: keep drawing");
    let frames = frames_to_settle(&mut renderer, &world);
    println!("new terrain published after {frames} frames");
}
