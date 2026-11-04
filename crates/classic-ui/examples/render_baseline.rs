//! Render baseline: CPU chunk assembly + a GPU frame (if there is an adapter).
//! The same line protocol as the sim harness (`m name k=v ... wall_ms=x`).
//! Without a GPU: CPU cases run, draw cases are SKIP (exit 0), so it works in CI.

use std::hint::black_box;
use std::time::Instant;

use classic_rules::rules::SourceRules;
use classic_rules::scenario::{generate, ScenarioConfig};
use classic_rules::{compile_rules, GameState};
use render::camera::CameraController;
use render::gpu::buffers::{build_chunk, build_grid_lines};
use render::scene::heightfield::build_heightfield;
use render::scene::style::{compile, default_style};
use render::world::projection::rebuild_render_world;

fn ms(d: std::time::Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut lines = vec!["# aurelia-bench v1 render".to_string()];

    // Features (mountains + forest) are taken from core_rules, the rest of the ruleset is default:
    // the features.rep line measures the game's real feature batch, while yields/
    // icons do not shift (comparability with the baseline history).
    let core = classic_content::core::core_rules();
    let rules = compile_rules(SourceRules {
        features: core.features,
        terrains: core.terrains,
        ..Default::default()
    })
    .unwrap();
    let rep = ScenarioConfig::representative_late_game();
    let game = generate(&rep, &rules);
    let world = rebuild_render_world(game.view(), classic_rules::ids::PlayerId(0), &rules);
    let style = compile(default_style()).unwrap();

    // CPU: heightfield + geometry + all chunks + grid (without GPU, always).
    let t0 = Instant::now();
    let hf = build_heightfield(&world, &style);
    let geom = render::scene::terrain::fields::build_terrain_geometry(&world, &style, &hf);
    let cols = world.width.div_ceil(32).max(1);
    let rows = world.height.div_ceil(32).max(1);
    let mut verts = 0usize;
    let mut tris = 0usize;
    for cy in 0..rows {
        for cx in 0..cols {
            let m = build_chunk(&world, &style, &geom, cx, cy);
            verts += m.vertices.len();
            tris += m.indices.len() / 3;
            black_box(m);
        }
    }
    let t_grid = Instant::now();
    let grid = build_grid_lines(&world, &hf);
    let grid_segments = grid.len() / 2;
    black_box(&grid);
    let grid_ms = ms(t_grid.elapsed());
    lines.push(format!(
        "m render.chunk.build.rep c_tiles={} c_entities={} c_bytes={} verts={} tris={} chunks={} wall_ms={:.3}",
        rep.width * rep.height,
        0,
        verts * 44 + tris * 3 * 2,
        verts,
        tris,
        cols * rows,
        ms(t0.elapsed()),
    ));
    lines.push(format!(
        "m render.grid.build c_tiles=0 c_entities=0 c_bytes={} segments={} wall_ms={:.3}",
        grid.len() * 12,
        grid_segments,
        grid_ms,
    ));

    // CPU: the selected city overlay (outlines of the WORK_RADIUS ring).
    // Counters gate: the segments are fixed by geometry (30 at radius 1).
    let t_sel = Instant::now();
    let sel_tile = classic_rules::ids::TileId((rep.height / 2) * rep.width + (rep.width / 2));
    let sel = render::gpu::buffers::build_city_highlight(
        &world,
        &hf,
        sel_tile,
        classic_rules::world::city::CITY_WORK_RADIUS,
        &[],
    );
    black_box(&sel);
    lines.push(format!(
        "m render.selection.build c_tiles=0 c_entities=0 c_bytes={} segments={} wall_ms={:.3}",
        sel.len() * 24,
        sel.len() / 2,
        ms(t_sel.elapsed()),
    ));

    // CPU: yield icons (flat instances of explored land).
    // Counters gate: instances = sum of food/production icons (deterministic).
    let t_icons = Instant::now();
    let icons = render::gpu::buffers::build_yield_instances(&world, &style, &geom);
    black_box(&icons);
    lines.push(format!(
        "m render.icons.build c_tiles=0 c_entities={} c_bytes={} icons={} wall_ms={:.3}",
        icons.len(),
        icons.len() * 20,
        icons.len(),
        ms(t_icons.elapsed()),
    ));

    // CPU: mountains (peaks on range cores; non-indexed 40B).
    // Counters gate: vertices = sum of peaks (deterministic).
    let t_feat = Instant::now();
    let feats = render::gpu::feature::build_mountain_mesh(&world, &geom);
    black_box(&feats);
    lines.push(format!(
        "m render.features.build c_tiles=0 c_entities=0 c_bytes={} verts={} wall_ms={:.3}",
        feats.len() * 40,
        feats.len(),
        ms(t_feat.elapsed()),
    ));

    // GPU: a whole frame (only with an adapter, otherwise SKIP exit 0).
    match render::gpu::context::GpuCore::new_blocking() {
        Err(e) => {
            println!("SKIP-GPU: {}", e);
        }
        Ok(core) => {
            let set = nation_content_assets();
            let mut renderer = render::renderer::Renderer::with_gpu(
                &core,
                wgpu::TextureFormat::Rgba8Unorm,
                &set,
            );
            let mut camera = CameraController::fit_map(rep.width, rep.height);
            camera.zoom = 0.35;
            let t0 = Instant::now();
            let iters = 5;
            let mut stats = render::RenderStats::default();
            let mut first = true;
            for _ in 0..iters {
                let tex = core.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("bench target"),
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
                let st = black_box(renderer.draw(
                    &core,
                    &world,
                    &camera,
                    &view,
                    (256, 256),
                    render::FrameFlags::default(),
                ));
                // Upload metrics from the first iteration (the revision gate zeroes them afterwards).
                if first {
                    stats = st;
                    first = false;
                }
            }
            lines.push(format!(
                "m render.draw.rep c_tiles={} c_entities={} c_bytes={} draws={} chunks_drawn={} tris={} wall_ms={:.3}",
                rep.width * rep.height,
                0,
                stats.bytes_uploaded,
                stats.draw_calls,
                stats.chunks_drawn,
                stats.terrain_tris,
                ms(t0.elapsed()) / iters as f64,
            ));
        }
    }

    let _ = GameState::new;
    if args.iter().any(|a| a == "--out") {
        let idx = args.iter().position(|a| a == "--out").unwrap();
        if let Some(path) = args.get(idx + 1) {
            std::fs::write(path, lines.join("\n") + "\n").unwrap();
            eprintln!("wrote {}", path);
        }
    } else {
        for l in &lines {
            println!("{}", l);
        }
    }
}

/// The ground layers: the platform's, shared with every module.
fn nation_content_assets() -> Vec<(u32, u32, Vec<u8>)> {
    render::ground::layers()
}
