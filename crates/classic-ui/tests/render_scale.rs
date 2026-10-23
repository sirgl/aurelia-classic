//! Render scale on a real device: the scene is drawn at a fraction of the target size and
//! upscaled; resizes and scale changes rebuild the targets, and the ledger follows.

use classic_rules::compile_rules;
use classic_rules::ids::PlayerId;
use classic_rules::rules::SourceRules;
use classic_rules::scenario::{generate, ScenarioConfig};
use render::camera::CameraController;
use render::gpu::context::GpuCore;
use render::gpu::memory::Category;
use render::renderer::Renderer;
use render::world::projection::rebuild_render_world;

#[test]
fn render_scale_upscales_and_follows_resizes() {
    let core = match GpuCore::new_blocking() {
        Ok(core) => core,
        Err(e) => {
            println!("SKIP-GPU: {e}");
            return;
        }
    };
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
    let layers = render::ground::layers();
    let mut renderer = Renderer::with_gpu(&core, wgpu::TextureFormat::Rgba8Unorm, &layers);
    let mut camera = CameraController::fit_map(48, 32);
    camera.zoom = 0.35;
    renderer.set_animation_time_override(Some(1.0));
    let flags = render::FrameFlags::default();

    let full = renderer.render_offscreen(&core, &world, &camera, 256, 256, flags);
    let full_targets = core.memory.report().of(Category::RenderTargets);

    renderer.set_render_scale(0.5);
    assert_eq!(renderer.render_scale(), 0.5);
    let half = renderer.render_offscreen(&core, &world, &camera, 256, 256, flags);
    assert_eq!(half.len(), full.len());
    assert!(
        half.chunks(4).any(|p| p[0] != half[0] || p[1] != half[1]),
        "the upscaled scene is not one flat color"
    );
    // The scaled frame has the same picture: the mean color stays close to the full one.
    let mean = |px: &[u8]| {
        px.chunks(4)
            .map(|p| p[0] as f64 + p[1] as f64 + p[2] as f64)
            .sum::<f64>()
            / (px.len() / 4) as f64
    };
    assert!(
        (mean(&full) - mean(&half)).abs() < 12.0,
        "{} vs {}",
        mean(&full),
        mean(&half)
    );
    assert!(
        half.chunks(4).all(|p| p[3] == 255),
        "the blit covers every pixel"
    );

    // Another size (widths must be 64-aligned for the readback) and another scale rebuild every target; the ledger counts the scaled color.
    let odd = renderer.render_offscreen(&core, &world, &camera, 320, 177, flags);
    assert_eq!(odd.len(), 320 * 177 * 4);
    renderer.set_render_scale(0.75);
    renderer.render_offscreen(&core, &world, &camera, 320, 177, flags);

    // One sample: the pipelines and targets are rebuilt, the picture stays close.
    renderer.set_msaa_samples(1);
    assert_eq!(renderer.msaa_samples(), 1);
    renderer.set_render_scale(1.0);
    let plain = renderer.render_offscreen(&core, &world, &camera, 256, 256, flags);
    assert!(
        (mean(&full) - mean(&plain)).abs() < 6.0,
        "{} vs {}",
        mean(&full),
        mean(&plain)
    );
    renderer.set_render_scale(0.5);
    renderer.render_offscreen(&core, &world, &camera, 256, 256, flags);
    renderer.set_msaa_samples(4);

    // Back to full scale: the intermediate texture is released and the output matches the first frame.
    renderer.set_render_scale(1.0);
    let again = renderer.render_offscreen(&core, &world, &camera, 256, 256, flags);
    let differing = again
        .chunks(4)
        .zip(full.chunks(4))
        .filter(|(a, b)| a != b)
        .count();
    // The device itself is not bit-exact between two identical frames (measured: 5-7 of 65536
    // pixels flicker on one card), so "exactly" means all but a handful.
    assert!(
        differing <= 64,
        "scale 1.0 draws as before; {differing} pixels differ"
    );
    assert_eq!(
        core.memory.report().of(Category::RenderTargets),
        full_targets
    );
}
