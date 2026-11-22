//! Playing an arm on: the game's own AI (the log's variant) on every seat, round after round, a
//! `Watch` reading each round. Three things of a scene live here and nowhere else: the yield gift
//! (`edit::Gift`) and the changes made later (`edit::Change::Later`), both at the start of a round,
//! and the scripted adversary (`seat::Seat`), at the start of its seat's turn.

use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::variant::Variant;
use classic_rules::classic::Classic;
use classic_rules::ids::PlayerId;
use classic_rules::{ClassicGame, CompiledRuleset, GameState};
use kernel::ai::driver::AiDriver;

use super::edit::{self, Change, Gift, Yield};
use super::seat::{Script, Seat};
use super::watch::Watch;
use crate::bench::modules::{BenchRules, ClassicBench};

/// Decision-trace entries kept: a round of six nations holds a few thousand.
const TRACE_CAPACITY: usize = 120_000;

/// What an arm is played with.
pub struct Arm {
    pub game: GameState,
    pub gift: Option<Gift>,
    /// Units the watch follows.
    pub tracked: Vec<u64>,
    /// Changes made at the start of a round of the window (1 = the first round).
    pub later: Vec<(u32, Change)>,
    /// The scripted adversary of the arm.
    pub script: Option<Script>,
}

/// An arm played: the world and the watch after the window, and the world when the game ended (played
/// on with no watch; `None` when the arm was played for the window only).
pub struct Played {
    pub window: GameState,
    pub watch: Watch,
    pub end: Option<GameState>,
    /// What the script counted (`Seat::counts`), empty without a script.
    pub script: std::collections::BTreeMap<String, f64>,
}

/// Plays `arm` for `rounds` rounds (fewer when the game ends), then, with `to_end`, to the end of the game.
pub fn play_arm(rules: &CompiledRuleset, arm: Arm, variant: Variant, rounds: u32, to_end: bool) -> Result<Played, String> {
    let Arm { mut game, gift, tracked, later, script } = arm;
    let mut driver = AiDriver::<Classic, ClassicMind>::with_mind(ClassicBench::settings(rules), Vec::new(), ClassicMind::new(variant));
    driver.set_trace_capacity(TRACE_CAPACITY);
    let mut watch = Watch::new(&game, rules, &tracked);
    let mut seat = script.as_ref().map(Script::start);
    let trace = std::env::var_os("STRESS_TRACE").is_some();
    for round in 1..=rounds {
        if game.is_over() {
            break;
        }
        if let Some(gift) = gift {
            give(&mut game, rules, gift);
        }
        for (_, change) in later.iter().filter(|(after, _)| *after == round) {
            edit::apply(&mut game, rules, change)?;
        }
        play_round(rules, &mut driver, &mut game, &mut seat)?;
        watch.observe(&game, driver.trace());
        if trace {
            eprintln!("{}", watch.line(&game));
        }
    }
    let end = if to_end { Some(play_out(rules, &mut driver, game.clone())?) } else { None };
    let script = seat.map(|s| s.counts()).unwrap_or_default();
    Ok(Played { window: game, watch, end, script })
}

/// One round by the driver, step by step; the script plays its part at the start of its seat's
/// turn, before the seat's AI looks at the world.
fn play_round(rules: &CompiledRuleset, driver: &mut AiDriver<Classic, ClassicMind>, game: &mut GameState, seat: &mut Option<Box<dyn Seat>>) -> Result<(), String> {
    let mut scripted = false;
    loop {
        if let Some(script) = seat.as_mut() {
            let active = game.view().auth.active_player();
            if !scripted && driver.at_player_boundary() && active == script.player() && game.view().auth.players().is_alive(active) {
                script.play(game, rules);
                scripted = true;
            }
        }
        match driver.step(game, rules) {
            kernel::ai::StepOutcome::RoundDone => return Ok(()),
            kernel::ai::StepOutcome::Waiting if game.is_over() => return Ok(()),
            kernel::ai::StepOutcome::Waiting => return Err(format!("round {} waits for a human", game.view().auth.turn().round)),
            kernel::ai::StepOutcome::Worked | kernel::ai::StepOutcome::PlayerDone(_) => {}
        }
    }
}

/// The game played to its end by the same driver (the science gift stops with the window).
fn play_out(rules: &CompiledRuleset, driver: &mut AiDriver<Classic, ClassicMind>, mut game: GameState) -> Result<GameState, String> {
    driver.set_trace_capacity(0);
    while !game.is_over() {
        let report = driver.run(&mut game, rules, u32::MAX);
        if !report.round_done && !game.is_over() {
            return Err(format!("round {} waits for a human", game.view().auth.turn().round));
        }
    }
    Ok(game)
}

/// `gift.percent` of what the player earns a round of the gift's yield, into its pool.
fn give(game: &mut GameState, rules: &CompiledRuleset, gift: Gift) {
    let player = PlayerId(gift.player);
    let income = game.ai_observation(rules, player).economy.income;
    match gift.yield_kind {
        Yield::Science => game.debug_earn_science(player, income.science.max(0) as u32 * gift.percent / 100),
        Yield::Culture => game.debug_grant_culture(player, income.culture * gift.percent / 100),
        Yield::Gold => game.debug_earn(player, income.gold.max(0) as u32 * gift.percent / 100),
    }
}
