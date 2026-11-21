//! Playing an arm on: the game's own AI (the log's variant) on every seat, round after round, a
//! `Watch` reading each round. The science gift (`edit::Gift`) is made here, at the start of each
//! round, and nowhere else.

use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::variant::Variant;
use classic_rules::classic::Classic;
use classic_rules::ids::PlayerId;
use classic_rules::{ClassicGame, CompiledRuleset, GameState};
use kernel::ai::driver::AiDriver;

use super::edit::Gift;
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
}

/// An arm played: the world and the watch after the window, and the world when the game ended (played
/// on with no watch; `None` when the arm was played for the window only).
pub struct Played {
    pub window: GameState,
    pub watch: Watch,
    pub end: Option<GameState>,
}

/// Plays `arm` for `rounds` rounds (fewer when the game ends), then, with `to_end`, to the end of the game.
pub fn play_arm(rules: &CompiledRuleset, arm: Arm, variant: Variant, rounds: u32, to_end: bool) -> Result<Played, String> {
    let Arm { mut game, gift, tracked } = arm;
    let mut driver = AiDriver::<Classic, ClassicMind>::with_mind(ClassicBench::settings(rules), Vec::new(), ClassicMind::new(variant));
    driver.set_trace_capacity(TRACE_CAPACITY);
    let mut watch = Watch::new(&game, &tracked);
    let trace = std::env::var_os("STRESS_TRACE").is_some();
    for _ in 0..rounds {
        if game.is_over() {
            break;
        }
        if let Some(gift) = gift {
            give_science(&mut game, rules, gift);
        }
        let report = driver.run(&mut game, rules, u32::MAX);
        if !report.round_done && !game.is_over() {
            return Err(format!("round {} waits for a human", game.view().auth.turn().round));
        }
        watch.observe(&game, driver.trace());
        if trace {
            eprintln!("{}", watch.line(&game));
        }
    }
    let end = if to_end { Some(play_out(rules, &mut driver, game.clone())?) } else { None };
    Ok(Played { window: game, watch, end })
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

/// The beakers `gift.percent` of the player's science a round.
fn give_science(game: &mut GameState, rules: &CompiledRuleset, gift: Gift) {
    let player = PlayerId(gift.player);
    let earned = game.ai_observation(rules, player).economy.income.science.max(0) as u32;
    game.debug_earn_science(player, earned * gift.percent / 100);
}
