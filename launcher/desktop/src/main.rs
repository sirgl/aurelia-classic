//! Aurelia (the classic game), the desktop launcher: registers the module and the product, the rest
//! is `launcher_core::desktop` (window, frame loop, music, `--replay`).
use classic_ui::ClassicUi;
use launcher_core::desktop::{self, Product};

#[cfg(feature = "heap-stats")]
#[global_allocator]
static HEAP: launcher_core::client::perf::CountingAllocator = launcher_core::client::perf::CountingAllocator;

/// The developer commands of this game: offscreen captures and benches.
#[cfg(feature = "dev-tools")]
fn dev(flag: &str, args: &[String]) -> bool {
    use classic_ui::dev::{art_bench, headless};
    match flag {
        "--screenshot" => headless::screenshot(args.get(2).map(String::as_str).unwrap_or("output/play-loop.png")),
        "--bench" => headless::bench(),
        "--bench-world" => headless::bench_world(),
        "--bench-scenes" => classic_ui::dev::scene_bench::bench_scenes(),
        "--bench-animation" => headless::bench_animation(),
        "--bench-ui" => headless::bench_ui(),
        "--bench-art" => art_bench::bench_art(),
        _ => return desktop::refuse_developer_flag(flag),
    }
    true
}

#[cfg(not(feature = "dev-tools"))]
fn dev(flag: &str, _args: &[String]) -> bool {
    desktop::refuse_developer_flag(flag)
}

/// `--replay`: a classic journal on the classic rules (docs/saves.md 3).
fn replay(module: &str, save: &[u8], journal: &[u8]) -> Result<String, String> {
    match module {
        "classic" => launcher_core::replay::run(save, journal, &classic_ui::ui::screens::game_rules()),
        other => Err(format!("The {other} rules are not part of this game")),
    }
}

fn main() {
    // What a bug report says about the build (`AURELIA_GIT_HASH`: set it when building to name the commit).
    launcher_core::client::shell::report::set_build(env!("CARGO_PKG_VERSION"), option_env!("AURELIA_GIT_HASH"));
    desktop::main(Product {
        title: "Aurelia",
        module: || (ClassicUi::game(), classic_ui::terrain_layers()),
        dev,
        replay,
    });
}
