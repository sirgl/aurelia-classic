//! `--bench-art`: how soon the picture-heavy screens show every picture. The research
//! tree opened cold and warm, then scrolled fast to its end; the city's list of every
//! production option scrolled fast; Encyclopedia pages opened cold and flipped fast. Frames run at 60 Hz of wall time, so the decoders get the time a
//! player gives them. "Cold" means the game's art caches are empty; the operating system's
//! file cache stays warm.
use std::time::{Duration, Instant};

use client::shell::art::ArtFrame;

use super::headless::{open_pedia, opening, request, Offscreen};
use crate::ui::model::GameRequest;
use crate::ui::pedia::{game_pedia, Category};
use crate::ui::screens::Screen;

const FRAME: Duration = Duration::from_micros(16_667);
/// Waiting for the pictures stops after this long.
const PATIENCE: Duration = Duration::from_secs(20);
/// The fast scroll: points per frame and frames (the whole tree in about a second).
const SCROLL_STEP: f32 = 80.;
const SCROLL_FRAMES: usize = 70;
/// The fast flip: frames each Encyclopedia page stays (100 ms) and pages flipped.
const PAGE_FRAMES: usize = 6;
const PAGES: usize = 40;

/// One paced frame: when it ended since the case began, its time, and its pictures.
struct Sample {
    at: Duration,
    frame_ms: f64,
    art: ArtFrame,
}

pub fn bench_art() {
    let mut run = Offscreen::new();
    run.start_match();
    run.frame();
    run.app.screens.open(Screen::Research.into());
    report_open("research tree: cold open", &wait_for_art(&mut run, Instant::now()));
    run.app.screens.open(Screen::Map.into());
    run.frame();
    run.app.screens.open(Screen::Research.into());
    report_open("research tree: warm reopen", &wait_for_art(&mut run, Instant::now()));
    let tree = egui::vec2(-SCROLL_STEP, 0.);
    scroll(&mut run, "research tree: fast scroll to the end", (0.5, 0.6), tree);
    run.app.module.screens.forget_art();
    production_list(&mut run);
    run.app.module.screens.forget_art();
    let pedia = game_pedia();
    let units: Vec<_> = pedia.category(Category::Units).map(|e| e.id).take(PAGES).collect();
    open_pedia(&mut run.app, units.first().copied());
    report_open("Encyclopedia unit page: cold open", &wait_for_art(&mut run, Instant::now()));
    flip_pages(&mut run, &units[1..]);
}

/// The capital's city screen listing every production option: opened cold, then
/// scrolled fast to the end of the list.
fn production_list(run: &mut Offscreen) {
    let Some(capital) = opening(&mut run.app) else {
        println!("city production list: no capital founded");
        return;
    };
    request(&mut run.app, GameRequest::SelectCity(capital));
    run.app.screens.open(Screen::City.into());
    if let Some(city) = run.app.module.session.as_mut().and_then(|s| s.city_view()) {
        run.app.module.screens.list_all_production(&city);
    }
    report_open("city production list (all options): cold open", &wait_for_art(run, Instant::now()));
    let list = egui::vec2(0., -SCROLL_STEP);
    scroll(run, "city production list: fast scroll to the end", (0.9, 0.3), list);
}

/// The wheel turned `step` points a frame with the pointer at `at` (a fraction of the
/// screen), then still until every picture shows.
fn scroll(run: &mut Offscreen, name: &str, at: (f32, f32), step: egui::Vec2) {
    let ppp = run.app.ctx.pixels_per_point();
    let size = egui::vec2(run.size.0 as f32, run.size.1 as f32) / ppp;
    let pointer = egui::pos2(size.x * at.0, size.y * at.1);
    let start = Instant::now();
    let mut samples = vec![paced(run, start, vec![egui::Event::PointerMoved(pointer)])];
    for _ in 0..SCROLL_FRAMES {
        let wheel = egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: step,
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::NONE,
        };
        samples.push(paced(run, start, vec![wheel]));
    }
    let scrolling = samples.len();
    samples.extend(wait_for_art(run, start));
    report_scroll(name, &samples, scrolling);
}

/// Encyclopedia pages shown one after another for `PAGE_FRAMES` frames each.
fn flip_pages(run: &mut Offscreen, pages: &[crate::ui::pedia::PediaRef]) {
    let start = Instant::now();
    let mut samples = Vec::new();
    let mut shown_in_time = 0;
    for &page in pages {
        open_pedia(&mut run.app, Some(page));
        let frames: Vec<_> = (0..PAGE_FRAMES).map(|_| paced(run, start, Vec::new())).collect();
        if frames.last().is_some_and(|s| s.art.loading == 0) {
            shown_in_time += 1;
        }
        samples.extend(frames);
    }
    let flipping = samples.len();
    samples.extend(wait_for_art(run, start));
    report_scroll("Encyclopedia: unit pages flipped every 100 ms", &samples, flipping);
    println!("  pages whose picture showed before the next page: {shown_in_time}/{}", pages.len());
}

/// Paced frames until a frame paints pictures and none is loading.
fn wait_for_art(run: &mut Offscreen, start: Instant) -> Vec<Sample> {
    let mut samples = Vec::new();
    loop {
        let sample = paced(run, start, Vec::new());
        let done = sample.art.loading == 0 && sample.art.shown + sample.art.missing > 0;
        samples.push(sample);
        if done || start.elapsed() > PATIENCE {
            return samples;
        }
    }
}

fn paced(run: &mut Offscreen, start: Instant, events: Vec<egui::Event>) -> Sample {
    let begin = Instant::now();
    let frame_ms = run.frame_with(events);
    let art = run.app.module.screens.art_frame();
    if let Some(rest) = FRAME.checked_sub(begin.elapsed()) {
        std::thread::sleep(rest);
    }
    Sample { at: start.elapsed(), frame_ms, art }
}

fn report_open(name: &str, samples: &[Sample]) {
    let Some(last) = samples.last() else { return };
    let state = if last.art.loading == 0 { "all pictures" } else { "STILL LOADING" };
    println!(
        "{name}: {state} after {:.0} ms ({} frames; {} shown, {} without art); frame {}",
        last.at.as_secs_f64() * 1000.,
        samples.len(),
        last.art.shown,
        last.art.missing,
        frame_times(samples),
    );
}

/// A moving case: how many picture slots were empty while it moved, and how long the
/// pictures took after it stopped.
fn report_scroll(name: &str, samples: &[Sample], moving: usize) {
    let (during, after) = samples.split_at(moving.min(samples.len()));
    let slots: u32 = during.iter().map(|s| s.art.shown + s.art.loading).sum();
    let empty: u32 = during.iter().map(|s| s.art.loading).sum();
    let empty_frames = during.iter().filter(|s| s.art.loading > 0).count();
    let stopped = during.last().map(|s| s.at).unwrap_or_default();
    let settled = after.last().map(|s| s.at).unwrap_or(stopped);
    println!(
        "{name}: moving {} frames, {empty_frames} with empty slots, {:.1}% of slots empty; all pictures {:.0} ms after it stopped; frame {}",
        during.len(),
        100. * empty as f64 / slots.max(1) as f64,
        (settled - stopped).as_secs_f64() * 1000.,
        frame_times(samples),
    );
}

fn frame_times(samples: &[Sample]) -> String {
    let mut times: Vec<f64> = samples.iter().map(|s| s.frame_ms).collect();
    times.sort_by(f64::total_cmp);
    let Some(max) = times.last() else { return String::new() };
    let median = times[times.len() / 2];
    let p95 = times[(times.len() * 95).div_ceil(100) - 1];
    format!("median {median:.2} ms, p95 {p95:.2} ms, max {max:.2} ms")
}
