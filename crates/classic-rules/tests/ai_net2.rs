//! `net2` wired into the bench (part E of work/new-net-design-2026-11-22.md): the variant plays the mind of
//! `nets-x-site` with `net2::think` in place of the strategist and its site head in place of the guarded site
//! score, and nothing else changes.
//!
//! - an end-to-end game of `net2`: a plan in the brief on every turn of a seat that has cities, the same
//!   game twice gives the same plans and the same hash;
//! - the site head is the guarded `net_score` when the plan says nothing (no menace, a wide seat), and a
//!   tall seat adds only its gap bonus;
//! - the invariants: `pre-net2` (the standard of the fifteenth promotion) and `pre-nets-x` play as pinned, with
//!   no plan in any brief; `standard` (since the sixteenth promotion) plays as `net2`, plan for plan; the
//!   variants that are not `net2`'s never have one; `net2-defence` plays as `net2` until the offence exists.

#![cfg(feature = "experimental-ai")]

use std::sync::{Arc, Mutex};

use classic_content::core::core_rules;
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::driver::AiDriver as ClassicDriver;
use kernel::ai::driver::AiDriver;
use classic_rules::ai::net2::{plan::MAX_RIVALS, Net2Plan};
use classic_rules::ai::planner::{PlanTools, TurnBrief};
use classic_rules::ai::planner_map::{KnownMap, MapScratch};
use classic_rules::ai::planner_settle::{best_sites_by, HeadRead, NetRead, SiteContext, SiteQuality};
use classic_rules::ai::site::SiteInputs;
use classic_rules::ai::state::AiState;
use classic_rules::ai::trace::AiCounters;
use classic_rules::ai::variant::Variant;
use classic_rules::ai::AiSettings;
use classic_rules::classic::Classic;
use classic_rules::ids::{EntityId, PlayerId, TileId, UnitHandle};
use classic_rules::knowledge::observation::{AIObservation, LocalObservation};
use classic_rules::replay::authoritative_hash;
use classic_rules::scenario::{generate_new_game, settler_kind, ScenarioConfig};
use classic_rules::{compile_rules, ClassicGame, CompiledRuleset, GameState};
use kernel::ai::mind::{AiMind, TurnKind, TurnStart};

/// What one start of a turn showed: who, when, whether it planned the whole turn, and the plan.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Seen {
    round: u32,
    player: PlayerId,
    full: bool,
    cities: u32,
    plan: Option<Net2Plan>,
}

/// A mind that plays as `ClassicMind` and writes down the brief's `net2` at every start of a turn.
#[derive(Debug)]
struct Recorder {
    inner: ClassicMind,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl Recorder {
    fn new(variant: Variant) -> (Self, Arc<Mutex<Vec<Seen>>>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        (Self { inner: ClassicMind::new(variant), seen: seen.clone() }, seen)
    }
}

impl AiMind<Classic> for Recorder {
    type Brief = TurnBrief;
    type Sight<'g> = AIObservation<'g>;
    type Near<'g> = LocalObservation<'g>;

    fn sense<'g>(&self, game: &'g GameState, rules: &'g CompiledRuleset, player: PlayerId) -> AIObservation<'g> {
        self.inner.sense(game, rules, player)
    }

    fn sense_unit<'g>(&self, game: &'g GameState, rules: &'g CompiledRuleset, settings: &AiSettings, player: PlayerId, unit: UnitHandle) -> Option<LocalObservation<'g>> {
        self.inner.sense_unit(game, rules, settings, player, unit)
    }

    fn note_humans(&mut self, humans: &[PlayerId]) {
        self.inner.note_humans(humans);
    }

    fn start_turn(&self, sight: &AIObservation<'_>, rules: &CompiledRuleset, player: PlayerId, state: &mut AiState) -> TurnStart<Classic, TurnBrief> {
        let start = self.inner.start_turn(sight, rules, player, state);
        self.seen.lock().unwrap().push(Seen { round: sight.round, player, full: start.kind == TurnKind::Full, cities: start.cities, plan: start.brief.net2.clone() });
        start
    }

    fn order_units(&self, brief: &TurnBrief, rules: &CompiledRuleset, order: &mut Vec<(EntityId, UnitHandle)>) {
        self.inner.order_units(brief, rules, order);
    }

    fn picks_next(&self, brief: &TurnBrief) -> bool {
        self.inner.picks_next(brief)
    }

    fn next_unit(&self, near: &LocalObservation<'_>, brief: &TurnBrief, state: &AiState, remaining: &[(EntityId, UnitHandle)]) -> Option<EntityId> {
        self.inner.next_unit(near, brief, state, remaining)
    }

    fn plan_strategic(&self, brief: &TurnBrief, state: &mut AiState, rules: &CompiledRuleset) -> Vec<classic_rules::ai::planner::Decision> {
        self.inner.plan_strategic(brief, state, rules)
    }

    fn decide_unit(&self, near: &LocalObservation<'_>, player: PlayerId, unit: UnitHandle, brief: &TurnBrief, state: &mut AiState, tools: &mut PlanTools<'_>) -> Option<classic_rules::ai::planner::Decision> {
        self.inner.decide_unit(near, player, unit, brief, state, tools)
    }

    fn permits(&self, game: &GameState, rules: &CompiledRuleset, player: PlayerId, command: &classic_rules::Command) -> bool {
        self.inner.permits(game, rules, player, command)
    }

    fn plan_end_of_turn(&self, sight: &AIObservation<'_>, player: PlayerId, brief: &TurnBrief, state: &AiState, tools: &mut PlanTools<'_>) -> Vec<classic_rules::ai::planner::Decision> {
        self.inner.plan_end_of_turn(sight, player, brief, state, tools)
    }
}

fn rules() -> CompiledRuleset {
    compile_rules(core_rules()).unwrap()
}

/// The game of the experimental-off pin (`ai_experimental_off.rs`): four seats on 40 × 32, seed `0xA1_5EED`.
fn scenario(rules: &CompiledRuleset) -> GameState {
    generate_new_game(rules, &ScenarioConfig { seed: 0xA1_5EED, players: 4, width: 40, height: 32, cities: 0, units: 0, water_ratio: 15 })
}

/// A game of `rounds` rounds played by `variant` in every seat: the hash at its end and what every start of a
/// turn showed.
fn play(variant: Variant, rounds: usize) -> (u64, Vec<Seen>) {
    let rules = rules();
    let mut game = scenario(&rules);
    let (mind, seen) = Recorder::new(variant);
    let mut driver: AiDriver<Classic, Recorder> = AiDriver::with_mind(AiSettings::for_rules(&rules, rules.default_difficulty()), Vec::new(), mind);
    let mut done = 0;
    let mut stalls = 0;
    while done < rounds && stalls < 50 {
        if driver.run(&mut game, &rules, 100).round_done {
            done += 1;
            stalls = 0;
        } else {
            stalls += 1;
        }
    }
    assert_eq!(done, rounds, "{} stalled", variant.name());
    let seen = seen.lock().unwrap().clone();
    (authoritative_hash(&game), seen)
}

/// The turns that plan a whole turn of a seat with a city (the strategist's: `strategy_net` runs only then).
fn strategic(seen: &[Seen]) -> Vec<&Seen> {
    seen.iter().filter(|s| s.full && s.cities > 0).collect()
}

const ROUNDS: usize = 40;

#[test]
fn a_net2_game_fills_the_plan_every_turn_and_is_deterministic() {
    let (hash, seen) = play(Variant::Net2, ROUNDS);
    let turns = strategic(&seen);
    assert!(turns.len() >= ROUNDS * 3, "the four seats plan most rounds: {} turns", turns.len());
    for turn in &turns {
        let plan = turn.plan.as_ref().unwrap_or_else(|| panic!("round {} player {:?}: no plan in the brief", turn.round, turn.player));
        assert_eq!(plan.round, turn.round, "the plan of this turn, not a held one");
        assert!(plan.rivals.len() <= MAX_RIVALS);
        assert!(plan.rivals.windows(2).all(|w| w[0].rival < w[1].rival), "the rivals read by id");
        assert!(plan.tall_share <= 1000);
    }
    // A turn that plans no strategy carries no plan: the readers play as before.
    assert!(seen.iter().filter(|s| !(s.full && s.cities > 0)).all(|s| s.plan.is_none()), "a plan only where the strategist runs");
    // Deterministic: the same game gives the same plans and the same hash.
    let (again_hash, again) = play(Variant::Net2, ROUNDS);
    assert_eq!(hash, again_hash, "{hash:016x}");
    assert_eq!(seen, again, "the same plans on every turn");
}

#[test]
fn net2_defence_plays_as_net2_until_the_offence_exists() {
    let (net2, net2_seen) = play(Variant::Net2, 12);
    let (defence, defence_seen) = play(Variant::Net2Defence, 12);
    assert_eq!(net2, defence, "no offence is built: the control plays as the candidate");
    assert_eq!(net2_seen, defence_seen);
    assert!(Variant::Net2.net2_offence() && !Variant::Net2Defence.net2_offence());
    assert!(ClassicMind::new(Variant::Net2).net2_offence() && !ClassicMind::new(Variant::Net2Defence).net2_offence());
}

/// The invariants: no variant but `net2`'s has a plan, `pre-net2` (the standard of the fifteenth promotion) plays
/// as `nets-x-site` and `pre-nets-x` as the standard before it, at the hashes the game without the feature pinned
/// before the sixteenth promotion (`ai_experimental_off.rs`: `PRE_NET2_120`, `PRE_NETS_X_120`).
#[test]
fn pre_net2_and_pre_nets_x_play_unchanged_with_no_plan() {
    const PRE_NET2_120: u64 = 0x0d29_24db_3999_6453;
    const PRE_NETS_X_120: u64 = 0xc13f_b8a6_f3df_ced8;
    for (variant, pinned) in [(Variant::PreNet2, PRE_NET2_120), (Variant::NetsXSite, PRE_NET2_120), (Variant::PreNetsX, PRE_NETS_X_120)] {
        let (hash, seen) = play(variant, 120);
        assert!(seen.iter().all(|s| s.plan.is_none()), "{}: a brief with a net2 plan", variant.name());
        assert_eq!(hash, pinned, "{}: {hash:016x}", variant.name());
    }
}

/// The sixteenth promotion: `standard` is the game of `net2`, the same plans on every turn and the same hash.
#[test]
fn standard_plays_net2() {
    let (net2, net2_seen) = play(Variant::Net2, ROUNDS);
    let (standard, standard_seen) = play(Variant::Standard, ROUNDS);
    assert_eq!(standard, net2, "{standard:016x}");
    assert_eq!(standard_seen, net2_seen);
    assert!(strategic(&standard_seen).iter().all(|s| s.plan.is_some()));
}

#[test]
fn only_the_net2_variants_think_with_net2() {
    // The net variants need weights files; the others are all the strategist-line variants.
    for variant in Variant::ALL.into_iter().filter(|v| !matches!(v, Variant::Net | Variant::UnitNet | Variant::Hybrid | Variant::SiteNet | Variant::ChoiceNet | Variant::ChoiceNetProduction | Variant::ChoiceNetResearch)) {
        let mind = ClassicMind::new(variant);
        assert_eq!(mind.plays_net2(), variant.net2(), "{}", variant.name());
    }
    let named: Vec<&str> = Variant::ALL.into_iter().filter(|v| v.net2()).map(|v| v.name()).collect();
    assert_eq!(
        named,
        ["standard", "net2", "net2-defence", "net2-tac", "net2-no-strategy", "net2-no-war", "net2-no-deals", "net2-no-peace", "net2-no-cs", "net2-no-congress", "net2-no-sites", "net2-no-fleet", "net2-no-guard", "net2-no-economy", "net2-no-beliefs", "net2-off", "science-flow"]
    );
    for name in ["net2", "net2-defence", "tac2", "net2-tac", "net2-no-war", "net2-off", "science-flow"] {
        let variant = Variant::from_name(name).unwrap_or_else(|| panic!("{name}"));
        assert_eq!(ClassicMind::new(variant).variant(), variant, "{name}");
    }
    // The tactical candidates carry the unit nets, the others do not (`Variant::unit_net`).
    assert_eq!(Variant::ALL.into_iter().filter(|v| matches!(v, Variant::Tac2 | Variant::Net2Tac)).count(), 2);
    assert!(Variant::Tac2.unit_net() && Variant::Net2Tac.unit_net() && !Variant::Net2.unit_net() && !Variant::Standard.unit_net());
    assert!(!ClassicMind::new(Variant::Tac2).plays_net2() && ClassicMind::new(Variant::Net2Tac).plays_net2());
}

/// The site head with a plan that says nothing (no menace, a wide seat) values every site as the guarded
/// `net_score` does, travel included; a tall seat too, since the gap a tall seat fills (S8) weighs nothing
/// (`weights::site::FILL`, GAP.md).
#[test]
fn the_site_head_is_the_guarded_net_when_the_plan_is_silent() {
    let rules = rules();
    let settings = AiSettings::for_rules(&rules, rules.default_difficulty());
    let silent = Net2Plan::default();
    let tall = Net2Plan { tall_share: 1000, ..Net2Plan::default() };
    let (mut compared, mut menaced) = (0, 0);
    let mut game = scenario(&rules);
    let mut driver = ClassicDriver::new(settings.clone(), Vec::new());
    while game.view().auth.turn().round < 70 {
        driver.run(&mut game, &rules, u32::MAX);
        let round = game.view().auth.turn().round;
        if round % 7 != 0 {
            continue;
        }
        for p in 0..4u8 {
            let player = PlayerId(p);
            let obs = game.ai_observation(&rules, player);
            let Some(origin) = obs.own_cities.first().map(|c| c.tile) else { continue };
            let own: Vec<TileId> = obs.own_cities.iter().map(|c| c.tile).collect();
            // The others' cities and the armies of the others in sight, as the settlers' context has them.
            let others: Vec<(TileId, PlayerId)> = (0..4u8).filter(|q| *q != p).flat_map(|q| game.ai_observation(&rules, PlayerId(q)).own_cities.iter().map(move |c| (c.tile, PlayerId(q))).collect::<Vec<_>>()).collect();
            let foreign: Vec<TileId> = others.iter().map(|(t, _)| *t).collect();
            let danger: Vec<TileId> = (0..4u8).filter(|q| *q != p).flat_map(|q| game.ai_observation(&rules, PlayerId(q)).own_units.iter().map(|u| u.tile).collect::<Vec<_>>()).collect();
            let context = SiteContext {
                view: &obs.view,
                profile: KnownMap::of_kind(&obs.view, settler_kind(&rules)).profile,
                rules: &rules,
                player,
                own_cities: &own,
                foreign_cities: &foreign,
                reserved: &[],
                danger: &danger,
                tile_memo: Default::default(),
            };
            let inputs = SiteInputs { nation: 0, techs: &[], own_cities: &own, foreign_cities: &foreign, round, excess_happiness: 0 };
            let moves = 2 * classic_rules::world::unit::MOVE_DENOMINATOR;
            let limit = settings.budget.candidates;
            let search = |quality: &SiteQuality<'_>| {
                let (mut scratch, mut counters) = (MapScratch::default(), AiCounters::default());
                let mut tools = PlanTools { rules: &rules, settings: &settings, scratch: &mut scratch, counters: &mut counters };
                best_sites_by(&context, origin, moves, &mut tools, limit, quality)
            };
            let guarded = search(&SiteQuality::Net(inputs, NetRead::Fitted));
            let head = search(&SiteQuality::Head(inputs, HeadRead { plan: &silent, rivals: &others }));
            assert_eq!(head, guarded, "round {round} player {p}: a silent plan is the guarded net");
            compared += guarded.len();
            // A tall seat: the same sites and values.
            let tall_sites = search(&SiteQuality::Head(inputs, HeadRead { plan: &tall, rivals: &others }));
            assert_eq!(tall_sites, head, "round {round} player {p}: a gap weighs nothing for a tall seat either");
            // A menacing neighbour doubles the penalty of a site next to it: never a better site, and a worse
            // one where a rival city is within the gap.
            let menace = Net2Plan { rivals: others.iter().map(|(_, owner)| *owner).collect::<std::collections::BTreeSet<_>>().into_iter().map(|rival| menaced_read(rival)).collect(), ..Net2Plan::default() };
            let scared = search(&SiteQuality::Head(inputs, HeadRead { plan: &menace, rivals: &others }));
            for site in &scared {
                let base = guarded.iter().find(|s| s.tile == site.tile).expect("the same sites");
                assert!(site.score <= base.score, "round {round} player {p} tile {:?}: a menace never raises a site", site.tile);
                menaced += usize::from(site.score < base.score);
            }
        }
    }
    assert!(compared > 100, "{compared} sites compared");
    // The case the test is about happens: sites next to a rival.
    assert!(menaced > 0, "no site next to a rival seen");
}

/// A rival read with the menace at its top and every other concept 0.
fn menaced_read(rival: PlayerId) -> classic_rules::ai::net2::plan::RivalRead {
    use classic_rules::ai::net2::plan::{Help, RivalRead, WarKind};
    RivalRead { rival, weak: 0, reach: 0, menace: 1000, distrust: 0, prey: 0, runaway: 0, spoiler: 0, retake: 0, duty: 0, grabber: 0, block: 0, border: 0, brake: 0, war_value: 0, war_kind: WarKind::Prey, trust: 1000, help: Help::Give, soldiers: Default::default() }
}

/// Plays 30 rounds of `net2` for the float twin's parity on real turns: run it with `NET2_TRACE=<file>`
/// (`cargo test --test ai_net2 -- --ignored play_for_the_trace`), then `aurelia_nn.net2.forward.parity` on the file.
#[test]
#[ignore = "writes the trace named by NET2_TRACE; run by hand"]
fn play_for_the_trace() {
    assert!(std::env::var_os("NET2_TRACE").is_some(), "set NET2_TRACE=<file>");
    let (_, seen) = play(Variant::Net2, 30);
    assert!(strategic(&seen).iter().all(|s| s.plan.is_some()));
}

/// A look, not a gate: `net2` against `nets-x-site` on a few seeds (cities at the end, how often the plan
/// carries a war order, a peace wish, a guard, a held help). Run by hand with `--ignored --nocapture`.
#[test]
#[ignore = "prints a comparison; run by hand"]
fn look_net2_against_nets_x_site() {
    let rules = rules();
    for seed in [0xA1_5EEDu64, 7, 11, 23] {
        for variant in [Variant::NetsXSite, Variant::Net2] {
            let mut game = generate_new_game(&rules, &ScenarioConfig { seed, players: 4, width: 40, height: 32, cities: 0, units: 0, water_ratio: 15 });
            let (mind, seen) = Recorder::new(variant);
            let mut driver: AiDriver<Classic, Recorder> = AiDriver::with_mind(AiSettings::for_rules(&rules, rules.default_difficulty()), Vec::new(), mind);
            let mut done = 0;
            while done < 100 {
                done += usize::from(driver.run(&mut game, &rules, 100).round_done);
            }
            let cities: usize = (0..4u8).map(|p| game.ai_observation(&rules, PlayerId(p)).own_cities.len()).sum();
            let seen = seen.lock().unwrap();
            let turns = strategic(&seen);
            let with = |f: &dyn Fn(&Net2Plan) -> bool| turns.iter().filter(|s| s.plan.as_ref().is_some_and(f)).count();
            println!(
                "seed {seed:#x} {:12} cities {cities:2} turns {} plans {} war-order {} peace-wish {} guard {} deny-help {}",
                variant.name(),
                turns.len(),
                with(&|_| true),
                with(&|p| p.war.order().is_some()),
                with(&|p| !p.peace_with.is_empty()),
                with(&|p| p.guard.iter().any(|g| g.level > 0)),
                with(&|p| p.rivals.iter().any(|r| r.help != classic_rules::ai::net2::plan::Help::Give)),
            );
        }
    }
}
