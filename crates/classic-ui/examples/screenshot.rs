//! Screenshot harness: scene -> PNG in /tmp (we check the visuals by eye:
//! icons, ribbons, joints). Not a test (artifacts by eye, not asserts);
//! run by hand: cargo run -p aurelia-render --example screenshot -- [out]
//!
//! Scene: core_rules content, a P0 city + units, the camera above the city,
//! selection on the city (highlight), icons on.

use classic_rules::ids::PlayerId;
use render::camera::CameraController;
use render::gpu::context::GpuCore;
use render::renderer::Renderer;
use render::world::projection::rebuild_render_world;

fn main() {
    let out = std::env::args()
        .nth(1)
        .unwrap_or("/tmp/civ_shot.png".to_string());
    // Icon atlas dump (art diagnostics, no GPU): cells x8, the background is
    // a checkerboard so that transparency and outline read. Env AURELIA_SHOT_ATLAS=1.
    if std::env::var("AURELIA_SHOT_ATLAS").is_ok() {
        let (aw, ah, apx) = render::gpu::buffers::build_yield_atlas();
        let k = 8u32;
        let (bw, bh) = (aw * k, ah * k);
        let mut big = vec![0u8; (bw * bh * 4) as usize];
        for y in 0..bh {
            for x in 0..bw {
                let si = (((y / k) * aw + x / k) * 4) as usize;
                let di = ((y * bw + x) * 4) as usize;
                if apx[si + 3] == 0 {
                    let bg = if ((x / k) + (y / k)).is_multiple_of(2) {
                        208
                    } else {
                        176
                    };
                    big[di..di + 4].copy_from_slice(&[bg, bg, bg, 255]);
                } else {
                    big[di..di + 4].copy_from_slice(&apx[si..si + 4]);
                }
            }
        }
        image::save_buffer(&out, &big, bw, bh, image::ColorType::Rgba8).unwrap();
        eprintln!("wrote atlas {} ({}x{})", out, bw, bh);
        return;
    }
    let zoom: f32 = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(2.2);
    let (w, h) = (1280u32, 800u32);

    let core = GpuCore::new_blocking().expect("screenshot needs GPU adapter");
    let rules = classic_rules::compile_rules(classic_content::core::core_rules()).unwrap();
    // Map seed: Env AURELIA_SHOT_SEED (river diagnostics), otherwise 20260928.
    let seed: u64 = std::env::var("AURELIA_SHOT_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(20260928);
    // A new game: a settler per player (deterministic, no luck
    // in distributing units among owners).
    let mut game = classic_rules::scenario::generate_new_game(
        &rules,
        &classic_rules::scenario::ScenarioConfig {
            seed,
            players: 2,
            width: 24,
            height: 16,
            cities: 0,
            units: 0,
            water_ratio: 10,
        },
    );
    // Founding through the real command (territory + queue as in the game):
    // the first unit for which FoundCity succeeded.
    let units = game.view().auth.indexes().units_of(PlayerId(0)).to_vec();
    let mut founded = false;
    for s in units {
        if game
            .execute(&rules, classic_rules::Command::FoundCity { settler: s })
            .is_ok()
        {
            founded = true;
            break;
        }
    }
    assert!(founded, "scenario must contain a settler");
    let mut world = rebuild_render_world(game.view(), PlayerId(0), &rules);
    eprintln!(
        "world: {} tiles, {} river edges, {} units (kinds {:?}), {} cities, {} forest",
        world.terrain.len(),
        world.river_edges.len(),
        world.units.len(),
        world.units.iter().map(|u| u.kind).collect::<Vec<_>>(),
        world.cities.len(),
        world.forest.iter().filter(|&&f| f != 0).count()
    );
    for (a, b) in &world.river_edges {
        eprintln!("  edge {}-{}", a, b);
    }
    let cores = world
        .terrain
        .iter()
        .filter(|c| render::scene::terrain::features::is_mountain_core(&world, c.tile))
        .count();
    eprintln!("mountain cores: {cores}");
    // Forest diagnostics: an ASCII map (F=forest at the tile centre, m=mountain, ~=water).
    for r in 0..world.height {
        let mut line = String::new();
        for q in 0..world.width {
            let t = (r * world.width + q) as usize;
            let tr = world.terrain.get(t).map(|c| c.terrain).unwrap_or(99);
            let ch = match tr {
                4 => '~',
                6 => 'm',
                2 => 'h',
                _ => '.',
            };
            line.push(if world.forest.get(t).copied().unwrap_or(0) != 0 {
                'F'
            } else {
                ch
            });
        }
        eprintln!("  {line}");
    }

    // Relief diagnostics (we were pressed on flat mountains): a biome histogram,
    // heights of the patch control points of a mountain/hill/plain and the feature apex.
    let mut hist = [0u32; 16];
    for c in &world.terrain {
        hist[(c.terrain as usize).min(15)] += 1;
    }
    eprintln!(
        "biomes {:?}",
        hist.iter()
            .enumerate()
            .filter(|(_, &c)| c > 0)
            .collect::<Vec<_>>()
    );
    let style =
        render::scene::style::compile(render::scene::style::default_style()).unwrap();
    let hf = render::scene::heightfield::build_heightfield(&world, &style);
    let geom = render::scene::terrain::fields::build_terrain_geometry(&world, &style, &hf);
    for want in [6u16, 2, 1] {
        if let Some(cell) = world.terrain.iter().find(|c| c.terrain == want) {
            let w = world.width.max(1);
            let (q, r) = (cell.tile.0 % w, cell.tile.0 / w);
            let (c0, inner, corners, edges) =
                render::scene::terrain::fields::patch_control_points(&geom, cell.tile, q, r);
            let ys = |pts: &[[f32; 3]; 6]| -> Vec<String> {
                pts.iter().map(|p| format!("{:.2}", p[1])).collect()
            };
            eprintln!(
                "tile {} t={} center={:.2} inner=[{}] corners=[{}] edges=[{}]",
                cell.tile.0,
                want,
                c0[1],
                ys(&inner).join(","),
                ys(&corners).join(","),
                ys(&edges).join(",")
            );
            if want == 6 {
                let apex = render::scene::terrain::features::mountain_apex(&geom, cell.tile);
                eprintln!("  peak apex y={:.2} (base+raise)", apex[1]);
            }
        }
    }
    let city_tile = world.cities.iter().find(|m| m.owner == 0).map(|m| m.tile.0);
    // Icon isolation mode (diagnostics): only the city tile is explored.
    // Env AURELIA_SHOT_ISOLATE=1. Otherwise the whole explored world as in the game.
    if std::env::var("AURELIA_SHOT_ISOLATE").is_ok() {
        if let Some(ct) = city_tile {
            for (i, v) in world.explored.iter_mut().enumerate() {
                *v = if i as u32 == ct { 1 } else { 0 };
            }
        }
    }
    // Full lighting (water/shore diagnostics): everything is visible.
    // Env AURELIA_SHOT_ALL=1.
    if std::env::var("AURELIA_SHOT_ALL").is_ok() {
        for v in world.visible.iter_mut() {
            *v = 1;
        }
        for v in world.explored.iter_mut() {
            *v = 1;
        }
    }

    // Model gallery (v36): diagnostic markers around the starting city.
    // AURELIA_SHOT_GALLERY=unit   : one kind0 unit on the western neighbour.
    // AURELIA_SHOT_GALLERY=center : all units+tiers on the city tile.
    let gallery_mode = std::env::var("AURELIA_SHOT_GALLERY").unwrap_or_default();
    if !gallery_mode.is_empty() {
        let Some(base) = world.cities.first().map(|c| c.tile.0) else {
            panic!("gallery needs a founded city");
        };
        let w = world.width.max(1);
        let (bq, br) = (base % w, base / w);
        let ring: Vec<(i64, i64)> = [(-1, 0), (0, -1), (1, -1), (-1, 1), (1, 0), (0, 1)]
            .iter()
            .filter(|(dq, dr)| {
                let (q, r) = (bq as i64 + dq, br as i64 + dr);
                q >= 0 && r >= 0 && (q as u32) < w && (r as u32) < world.height
            })
            .map(|(dq, dr)| (bq as i64 + dq, br as i64 + dr))
            .collect();
        assert!(ring.len() >= 6, "capital must not hug the border");
        use render::world::state::{CityMarker, UnitMarker};
        if gallery_mode == "center" {
            for kind in 0u16..3 {
                world.units.push(UnitMarker {
                    id: 0, embarked: false, fortified: false, tile: classic_rules::ids::TileId(base),
                    owner: 0,
                    kind,
                    hp: 100,
                });
            }
            for pop in [3u32, 6] {
                world.cities.push(CityMarker {
                    tile: classic_rules::ids::TileId(base),
                    owner: 0,
                    name: format!("C{pop}"),
                    population: Some(pop),
                    buildings: Vec::new(),
                    era: 0,
                    showpieces: Vec::new(),
                    unworked: Vec::new(),
                });
            }
        } else if gallery_mode == "unit" {
            let (q, r) = ring[0];
            world.units.push(UnitMarker {
                id: 0, embarked: false, fortified: false, tile: classic_rules::ids::TileId(r as u32 * w + q as u32),
                owner: 0,
                kind: 0,
                hp: 100,
            });
        } else if gallery_mode == "ring" {
            for (kind, (q, r)) in (0u16..3).zip(ring.iter().take(3)) {
                world.units.push(UnitMarker {
                    id: 0, embarked: false, fortified: false, tile: classic_rules::ids::TileId((*r as u32) * w + *q as u32),
                    owner: 0,
                    kind,
                    hp: 100,
                });
            }
            for (pop, (q, r)) in [3u32, 6].iter().zip(ring.iter().skip(3)) {
                world.cities.push(CityMarker {
                    tile: classic_rules::ids::TileId((*r as u32) * w + *q as u32),
                    owner: 0,
                    name: format!("Gallery{pop}"),
                    population: Some(*pop),
                    buildings: Vec::new(),
                    era: 0,
                    showpieces: Vec::new(),
                    unworked: Vec::new(),
                });
            }
        }
        eprintln!(
            "gallery mode={gallery_mode}: {} units, {} cities",
            world.units.len(),
            world.cities.len()
        );
    }

    let layers = render::ground::layers();
    let mut renderer = Renderer::with_gpu(&core, wgpu::TextureFormat::Rgba8Unorm, &layers);
    renderer.set_unit_models(render::unit_models::UnitModelBook::from_rules(&rules));
    renderer.set_world_models(render::world_models::WorldBook::from_rules(&rules));

    // Camera above the city, closer (icons/ribbons visible).
    // If there are rivers, onto the middle of the first (a by-eye meshing check).
    // Env AURELIA_SHOT_FOCUS=mountain: onto the first core of a range (a peak check).
    // The middle is the average of the WORLD coordinates of the ends (not ids!).
    let mut camera = CameraController::fit_map(24, 16);
    // Env AURELIA_SHOT_FOCUS=city: a close-up above the city (units/cities v36).
    let city_mid = std::env::var("AURELIA_SHOT_FOCUS")
        .is_ok_and(|v| v == "city")
        .then(|| city_tile.map(|ct| render::scene::layout::tile_to_world(ct % 24, ct / 24)));
    let mountain_mid = std::env::var("AURELIA_SHOT_FOCUS")
        .is_ok_and(|v| v == "mountain")
        .then(|| {
            world
                .terrain
                .iter()
                .find(|c| render::scene::terrain::features::is_mountain_core(&world, c.tile))
                .map(|c| {
                    let w = 24u32;
                    render::scene::layout::tile_to_world(c.tile.0 % w, c.tile.0 / w)
                })
        });
    let forest_mid = std::env::var("AURELIA_SHOT_FOCUS")
        .is_ok_and(|v| v == "forest")
        .then(|| {
            // The forest tile nearest to the map centre (a deterministic
            // choice: checking the pins at mid range).
            let w = 24u32;
            world
                .forest
                .iter()
                .enumerate()
                .filter(|(_, f)| **f != 0)
                .min_by_key(|(i, _)| {
                    let (q, r) = (*i as u32 % w, *i as u32 / w);
                    (q as i32 - 12).pow(2) + (r as i32 - 8).pow(2)
                })
                .map(|(i, _)| render::scene::layout::tile_to_world(i as u32 % w, i as u32 / w))
        });
    let river_mid = world.river_edges.first().map(|(a, b)| {
        let w = 24u32;
        let (ax, az) = render::scene::layout::tile_to_world(a % w, a / w);
        let (bx, bz) = render::scene::layout::tile_to_world(b % w, b / w);
        ((ax + bx) * 0.5, (az + bz) * 0.5)
    });
    if let Some((wx, wz)) = city_mid
        .flatten()
        .or(mountain_mid.flatten())
        .or(forest_mid.flatten())
        .or(river_mid)
    {
        camera.x = wx;
        camera.y = wz;
    } else if let Some(ct) = city_tile {
        let (wx, wz) = render::scene::layout::tile_to_world(ct % 24, ct / 24);
        camera.x = wx;
        camera.y = wz;
    }
    camera.zoom = zoom;

    let tex = core.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("shot target"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view_tex = tex.create_view(&wgpu::TextureViewDescriptor::default());
    let stats = renderer.draw(
        &core,
        &world,
        &camera,
        &view_tex,
        (w, h),
        render::FrameFlags {
            // Env AURELIA_SHOT_GRID=0: a frame without the grid (line diagnostics).
            grid: std::env::var("AURELIA_SHOT_GRID")
                .map(|v| v != "0")
                .unwrap_or(true),
            selected_city: city_tile,
            yields: true,
        },
    );
    eprintln!(
        "stats: units={} ({} draws) cities={} ({} draws) draws={}",
        stats.unit_instances,
        stats.unit_draws,
        stats.city_instances,
        stats.city_draws,
        stats.draw_calls
    );
    let pitch = w * 4;
    let buf = core.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("shot readback"),
        size: (pitch * h) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = core
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("shot copy"),
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
                rows_per_image: Some(h),
            },
        },
        wgpu::Extent3d {
            width: w,
            height: h,
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
    image::save_buffer(&out, &data, w, h, image::ColorType::Rgba8).unwrap();
    eprintln!("wrote {} ({}x{})", out, w, h);
}
