//! The integration's sanity look at `net2` (work/new-net-design-2026-11-22.md §6.2 step 4), not a gate: whole
//! classic games of the bench's shape (Small map, six nations, twelve city-states, barbarians, Quick speed),
//! `net2` on the even seats and `nets-x-site` on the odd ones, 100 rounds. For each side it prints how often the
//! plan carries each lever and what the executors then did (applied commands), so that a lever that never moves
//! or never reaches its executor shows. Run by hand:
//! `cargo test --features experimental-ai --test ai_net2_sanity -- --ignored --nocapture`.

#![cfg(feature = "experimental-ai")]

use std::sync::{Arc, Mutex};

use classic_content::core::core_rules;
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::net2::plan::{Embargo, Escorts, Generals, Help};
use classic_rules::ai::net2::Net2Plan;
use classic_rules::ai::planner::{PlanTools, TurnBrief};
use classic_rules::ai::state::AiState;
use classic_rules::ai::variant::Variant;
use classic_rules::ai::AiSettings;
use classic_rules::classic::Classic;
use classic_rules::ids::{EntityId, PlayerId, UnitHandle};
use classic_rules::knowledge::observation::{AIObservation, LocalObservation};
use classic_rules::scenario::{generate_new_game_with, MapScript, NewGameOptions, ScenarioConfig};
use classic_rules::world::barbarians::BarbarianMode;
use classic_rules::world::congress::ResolutionKind;
use classic_rules::world::relations::DealItem;
use classic_rules::{compile_rules, ClassicGame, Command, CompiledRuleset, GameState};
use kernel::ai::driver::AiDriver;
use kernel::ai::mind::{AiMind, TurnKind, TurnStart};
use kernel::world::size::WorldSize;

/// One strategic turn of a `net2` seat: who, and the plan.
#[derive(Debug, Clone)]
struct Seen {
    player: PlayerId,
    round: u32,
    plan: Option<Net2Plan>,
    /// The seat's grand strategy at the turn (the war machine starts only in some, `strategy_wages_war`).
    strategy: String,
}

/// `net2` on the even seats, `nets-x-site` on the odd ones; every strategic start of a turn is written down.
#[derive(Debug)]
struct Pair {
    net2: ClassicMind,
    baseline: ClassicMind,
    seen: Arc<Mutex<Vec<Seen>>>,
    /// Nanoseconds of the mind's calls per side (`net2`, `nets-x-site`): the AI time of its seats.
    spent: Mutex<[u64; 2]>,
}

fn is_net2(player: PlayerId) -> bool {
    player.0 % 2 == 0
}

impl Pair {
    /// Runs `call` and adds its time to `player`'s side.
    fn timed<T>(&self, player: PlayerId, call: impl FnOnce() -> T) -> T {
        let start = std::time::Instant::now();
        let out = call();
        let side = usize::from(!is_net2(player));
        self.spent.lock().unwrap()[side] += start.elapsed().as_nanos() as u64;
        out
    }

    fn mind(&self, player: PlayerId) -> &ClassicMind {
        if is_net2(player) {
            &self.net2
        } else {
            &self.baseline
        }
    }
}

impl AiMind<Classic> for Pair {
    type Brief = TurnBrief;
    type Sight<'g> = AIObservation<'g>;
    type Near<'g> = LocalObservation<'g>;

    fn sense<'g>(&self, game: &'g GameState, rules: &'g CompiledRuleset, player: PlayerId) -> AIObservation<'g> {
        self.mind(player).sense(game, rules, player)
    }

    fn sense_unit<'g>(&self, game: &'g GameState, rules: &'g CompiledRuleset, settings: &AiSettings, player: PlayerId, unit: UnitHandle) -> Option<LocalObservation<'g>> {
        self.mind(player).sense_unit(game, rules, settings, player, unit)
    }

    fn note_humans(&mut self, humans: &[PlayerId]) {
        self.net2.note_humans(humans);
        self.baseline.note_humans(humans);
    }

    fn start_turn(&self, sight: &AIObservation<'_>, rules: &CompiledRuleset, player: PlayerId, state: &mut AiState) -> TurnStart<Classic, TurnBrief> {
        let start = self.timed(player, || self.mind(player).start_turn(sight, rules, player, state));
        if start.kind == TurnKind::Full && start.cities > 0 {
            self.seen.lock().unwrap().push(Seen { player, round: sight.round, plan: start.brief.net2.clone(), strategy: format!("{:?}", state.strategy.current) });
        }
        start
    }

    fn order_units(&self, brief: &TurnBrief, rules: &CompiledRuleset, order: &mut Vec<(EntityId, UnitHandle)>) {
        self.mind(brief.player).order_units(brief, rules, order);
    }

    fn picks_next(&self, brief: &TurnBrief) -> bool {
        self.mind(brief.player).picks_next(brief)
    }

    fn next_unit(&self, near: &LocalObservation<'_>, brief: &TurnBrief, state: &AiState, remaining: &[(EntityId, UnitHandle)]) -> Option<EntityId> {
        self.timed(brief.player, || self.mind(brief.player).next_unit(near, brief, state, remaining))
    }

    fn plan_strategic(&self, brief: &TurnBrief, state: &mut AiState, rules: &CompiledRuleset) -> Vec<classic_rules::ai::planner::Decision> {
        self.timed(brief.player, || self.mind(brief.player).plan_strategic(brief, state, rules))
    }

    fn decide_unit(&self, near: &LocalObservation<'_>, player: PlayerId, unit: UnitHandle, brief: &TurnBrief, state: &mut AiState, tools: &mut PlanTools<'_>) -> Option<classic_rules::ai::planner::Decision> {
        self.timed(player, || self.mind(player).decide_unit(near, player, unit, brief, state, tools))
    }

    fn permits(&self, game: &GameState, rules: &CompiledRuleset, player: PlayerId, command: &Command) -> bool {
        self.mind(player).permits(game, rules, player, command)
    }

    fn plan_end_of_turn(&self, sight: &AIObservation<'_>, player: PlayerId, brief: &TurnBrief, state: &AiState, tools: &mut PlanTools<'_>) -> Vec<classic_rules::ai::planner::Decision> {
        self.timed(player, || self.mind(player).plan_end_of_turn(sight, player, brief, state, tools))
    }
}

/// The bench's game of `pace` at Quick on a Small map (`qa::spec::GameSpec`): six nations, twelve city-states,
/// barbarians, the setup screen's medium sea.
fn game(rules: &CompiledRuleset, script: &str, seed: u64) -> GameState {
    let script = MapScript::from_key(script).expect("a map script");
    let (width, height) = WorldSize::Small.grid();
    let config = ScenarioConfig { seed, players: 6, width, height, cities: 0, units: 0, water_ratio: script.water_ratio(25) };
    let options = NewGameOptions { barbarians: BarbarianMode::Normal, city_states: 12, script, ..NewGameOptions::default() };
    let options = options.at_speed(rules, "quick").expect("the quick speed");
    generate_new_game_with(rules, &config, None, options)
}

/// The rounds of a game: 100, or `NET2_SANITY_ROUNDS` (330 is the whole game of `pace` at Quick).
fn rounds() -> usize {
    std::env::var("NET2_SANITY_ROUNDS").ok().and_then(|r| r.parse().ok()).unwrap_or(100)
}

#[test]
#[ignore = "prints the lever counts of four games; run by hand"]
fn sanity_the_levers_move_against_nets_x_site() {
    let rules = compile_rules(core_rules()).unwrap();
    for (script, seed) in [("pangaea", 1_700_001u64), ("continents", 1_700_002), ("fractal", 1_700_003), ("small-continents", 1_700_004)] {
        let mut game = game(&rules, script, seed);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let pair = Pair { net2: ClassicMind::new(Variant::Net2), baseline: ClassicMind::new(Variant::NetsXSite), seen: seen.clone(), spent: Mutex::new([0, 0]) };
        let _ = prof::drain();
        let mut driver: AiDriver<Classic, Pair> = AiDriver::with_mind(AiSettings::for_rules(&rules, rules.default_difficulty()), Vec::new(), pair);
        driver.set_trace_capacity(usize::MAX);
        classic_rules::ai::war_probe::enable(true);
        let majors: Vec<PlayerId> = (0..6u8).map(PlayerId).collect();
        let mut operations: Vec<(PlayerId, PlayerId, classic_rules::ids::TileId)> = Vec::new();
        let mut done = 0;
        let mut stalls = 0;
        while done < rounds() && !game.is_over() && stalls < 50 {
            if driver.run(&mut game, &rules, 100).round_done {
                done += 1;
                stalls = 0;
                let round = game.view().auth.turn().round;
                for p in &majors {
                    for o in driver.state_mut(*p, round).operations() {
                        if !operations.contains(&(*p, o.enemy, o.target)) {
                            operations.push((*p, o.enemy, o.target));
                        }
                    }
                }
            } else {
                stalls += 1;
            }
        }
        let probe = classic_rules::ai::war_probe::take();
        print_time(&driver.mind_mut().spent.lock().unwrap(), &prof::drain());
        classic_rules::ai::war_probe::enable(false);
        let seen = seen.lock().unwrap();
        for p in majors.iter().filter(|p| is_net2(**p)) {
            let cities: Vec<(u8, u32)> = driver.state_mut(*p, 0).seen_cities().iter().map(|c| (c.owner.0, c.population)).collect();
            println!("  p{} knows cities (owner, population) at the end: {cities:?}", p.0);
        }
        // Every turn with a war order: where the war machine stopped (no start check: the start's gates of
        // season, cities, round, strategy and posture; a check: its candidates and whether one started).
        for s in seen.iter().filter(|s| s.plan.as_ref().is_some_and(|p| p.war.order().is_some())) {
            let order = s.plan.as_ref().and_then(|p| p.war.order()).copied();
            let check = probe.checks.iter().find(|c| c.player == s.player && c.round == s.round);
            println!("  order r{} p{} {:?} strategy {} check {:?}", s.round, s.player.0, order.map(|o| (o.rival.0, o.kind, o.value)), s.strategy, check.map(|c| (c.free, c.army, c.candidates, c.started)));
            for e in probe.estimates.iter().filter(|e| e.player == s.player && e.round == s.round) {
                println!("    estimate city {} pop {} distance {} value {} gain {} cost {} chance {} rounds {} missing {}/{}", e.target, e.population, e.distance, e.value, e.gain, e.cost, e.chance, e.rounds, e.missing, e.need);
            }
        }
        let applied: Vec<(PlayerId, Command)> = driver.trace().entries().filter(|e| e.outcome == kernel::ai::trace::Outcome::Applied).filter_map(|e| e.command.clone().map(|c| (e.player, c))).collect();
        for side in [true, false] {
            let turns: Vec<&Seen> = seen.iter().filter(|s| is_net2(s.player) == side).collect();
            let with = |f: &dyn Fn(&Net2Plan) -> bool| turns.iter().filter(|s| s.plan.as_ref().is_some_and(f)).count();
            let mine: Vec<&Command> = applied.iter().filter(|(p, _)| is_net2(*p) == side && majors.contains(p)).map(|(_, c)| c).collect();
            let count = |f: &dyn Fn(&Command) -> bool| mine.iter().filter(|c| f(c)).count();
            let top = |f: &dyn Fn(&classic_rules::ai::net2::plan::RivalRead) -> i64| turns.iter().filter_map(|s| s.plan.as_ref()).flat_map(|p| p.rivals.iter().map(f)).max().unwrap_or(0);
            let gold: u32 = mine.iter().map(|c| if let Command::GiftGold { gold, .. } = c { *gold } else { 0 }).sum();
            let cities: usize = majors.iter().filter(|p| is_net2(**p) == side).map(|p| game.ai_observation(&rules, *p).own_cities.len()).sum();
            println!(
                "{script:16} {seed} {:11} rounds {done} turns {} plans {} war-order {} peace-wish {} guard {} deny-help {} embargo {} cs-share {} hold-generals {} escorts {} | declared {} operations {} peace-offers {} denounced {} embargo-proposals {} gifts {} ({gold} gold) cities {cities} | top war-value {} runaway {} prey {} retake {} spoiler {}",
                if side { "net2" } else { "nets-x-site" },
                turns.len(),
                with(&|_| true),
                with(&|p| p.war.order().is_some()),
                with(&|p| !p.peace_with.is_empty()),
                with(&|p| p.guard.iter().any(|g| g.level > 0)),
                with(&|p| p.rivals.iter().any(|r| r.help == Help::Deny)),
                with(&|p| p.embargo != Embargo::Nobody),
                with(&|p| p.cs_gold_share > 0),
                with(&|p| p.generals == Generals::Hold),
                with(&|p| matches!(p.escorts, Escorts::Ships(_))),
                count(&|c| matches!(c, Command::DeclareWar { .. })),
                operations.iter().filter(|(p, _, _)| is_net2(*p) == side).count(),
                count(&|c| matches!(c, Command::ProposeDeal { items, .. } if items.iter().any(|(_, item)| *item == DealItem::Peace))),
                count(&|c| matches!(c, Command::Denounce { .. })),
                count(&|c| matches!(c, Command::ProposeResolution { kind: ResolutionKind::PlayerEmbargo, .. })),
                count(&|c| matches!(c, Command::GiftGold { .. })),
                top(&|r| r.war_value),
                top(&|r| r.runaway),
                top(&|r| r.prey),
                top(&|r| r.retake),
                top(&|r| r.spoiler),
            );
        }
    }
}

/// The AI time of each side and, with `--features prof/enabled`, the spans of `net2` and of the executors (summed
/// over every path that ends in the span; the executors' spans hold both sides' calls).
fn print_time(spent: &[u64; 2], records: &[prof::Record]) {
    println!("  AI time: net2 {:.0} ms, nets-x-site {:.0} ms ({:.2} x)", spent[0] as f64 / 1e6, spent[1] as f64 / 1e6, spent[0] as f64 / spent[1].max(1) as f64);
    let mut sums: Vec<(&str, u64, u64)> = Vec::new();
    for r in records {
        let name = r.path.rsplit(" > ").next().unwrap_or(&r.path);
        match sums.iter_mut().find(|(n, _, _)| *n == name) {
            Some(entry) => {
                entry.1 += r.incl_ns;
                entry.2 += r.calls;
            }
            None => sums.push((name, r.incl_ns, r.calls)),
        }
    }
    sums.sort_by_key(|(_, ns, _)| std::cmp::Reverse(*ns));
    for (name, ns, calls) in sums.iter().filter(|(n, _, _)| n.starts_with("net2/") || n.starts_with("classic/")).take(16) {
        println!("    span {name}: {:.0} ms, {calls} calls, {:.1} % of net2's AI time", *ns as f64 / 1e6, *ns as f64 * 100.0 / spent[0].max(1) as f64);
    }
}
