//! The AI bench (docs/ai-bench.md): a tournament is deterministic whatever
//! the threads, the `standard` variant plays exactly the game's AI, and the
//! variants are chosen by name and seat.

use aurelia_qa::bench::modules::{majors, BenchRules, ClassicBench};
use aurelia_qa::bench::play::{play_match, tournament, Pairing};
use aurelia_qa::bench::report::{RunInfo, TournamentReport};
use aurelia_qa::bench::sequential::{play_sequential, Decision, Plan, Reference, Sprt};
use aurelia_qa::bench::suite::{Mode, Seating, Side, Suite};
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::variant::Variant;
use classic_rules::ai::{AiDriver, AiSettings};
use kernel::ai::seats::Seated;
use kernel::ids::PlayerId;
use kernel::replay::hash::authoritative_hash;

/// The result hash of the smoke suite, `standard` against `standard`
/// (pinned: a change of the classic AI, its rules or the kernel shows here).
///
/// Re-pin log:
/// - 16 October, the economy AI became the game's AI (`standard`; the old
///   one is `legacy`): 31339486988a2ee4 (legacy against legacy) ->
///   487cc1fcfba56622.
/// - 16 October, every nation's units enter a city-state's land without
///   open borders (docs/classic/rules.md §5.6): 487cc1fcfba56622 ->
///   0aaf168b801dd818. With the rule switched off the old hash returns.
/// - 16 October, an embassy reveals the host's capital and the tiles
///   within two of it (docs/classic/rules.md §5.5): 0aaf168b801dd818 ->
///   1afae5e0a2fed790. Without the reveal the previous hash returns.
/// - 16 October, leaders with defence and econ2 became the game's AI
///   (`defence-econ2` is `standard`; the old `standard` is `allocator`,
///   which still gives 1afae5e0a2fed790): 1afae5e0a2fed790 ->
///   2b91c16d710beaa0.
/// - 18 October, the classic cost formulas (docs/classic/rules.md §10.2, §9.1,
///   §5.2) and the pacing AI (one queued order, a shrine after the second
///   city, gold tiles only for a deficit the treasury cannot carry):
///   2b91c16d710beaa0 -> bf9ba0f246dac862. The costs alone give
///   2b91c16d710beaa0 -> 360e38bb90384106.
/// - 18 October, `ai/econ2-fixes` (the opening cities' citizens as growth, the
///   happiness steps) came into `main` after the pacing AI: bf9ba0f246dac862 ->
///   d4b57f5e0feec948.
/// - 18 October, Avoid Growth (the priced AI's cities stop growing at -2 excess
///   happiness) and the per-citizen yields (Library) priced for the city's
///   citizens: d4b57f5e0feec948 -> bf8be0bb5a6667b0.
/// - 18 October, units see by their own sight (an embarked unit one tile,
///   a submarine only up close): bf8be0bb5a6667b0 -> 841601f704d84cea.
/// - 18 October, settlers: nearer sites and an escort built for a settler that
///   met danger: 841601f704d84cea -> 9e13669dac704ede.
/// - 19 October, the pantheon by the score of its effects on the land and the
///   situation (`ai::pantheon`) instead of the fixed list, and a prophet's Holy
///   Site only where the terrain suits it: 9e13669dac704ede -> 1891d5ff0e8d95c0.
/// - 19 October, the science economy (docs/ai-allocator.md «Наука»): the cities
///   order only what the rules accept (no Factory without Coal, no unit without
///   its policy) and a beaker is priced by what the technologies it buys open,
///   not by their score: 1891d5ff0e8d95c0 -> 85bb1af6ebe95a00.
/// - 20 October, the policy branches by the declared effects of the branches against what the
///   player sees (`ai::policy_branches`, docs/ai-policies.md) instead of a fixed order:
///   85bb1af6ebe95a00 -> 804e9accb3c9ea92;
/// - 20 October, the scouts take their targets from one allocation (`ai::scouting`,
///   docs/ai-scouting.md) and the barbarian ships march only where they may go:
///   804e9accb3c9ea92 -> a57fa3bf38058928.
/// - 20 October, resources at sea: the map generator lays fish, whales, pearls, crab and sea oil
///   (a fish for a coastal start) and balances the land oil and uranium counts, so the games are
///   played on other maps: a57fa3bf38058928 -> b95286cdd4b95586 (after the scouts).
/// - 31 October, the bench moved into the classic repository on the current
///   kernel and rules (everything since the monorepo snapshot):
///   b95286cdd4b95586 -> b1e56ea8a4605186.
/// - 31 October, workers wanted only for the jobs their finder offers
///   (classic/docs/ai-effects-checked.md): b1e56ea8a4605186 -> 86814d00cd09d362.
/// - 1 November, the fourth promotion: `standard` plays as `war-fix`
///   (classic/docs/ai-war-fix.md "Promoted"; the old `war-fix` against itself gave
///   this hash, `pre-war-fix` against itself keeps the old one):
///   86814d00cd09d362 -> 5749d0ae77b8a502.
/// - 1 November, the fifth promotion: `standard` plays the switches of `unit-abilities` and
///   `religion-fix` (classic/docs/ai-unit-abilities.md "Promoted"); no order of the smoke game's
///   70 rounds changes, the hash stays (as `pre-abilities` against itself): 5749d0ae77b8a502.
/// - 1 November, the sixth promotion: `standard` plays as `tall-science` (a city count, sites,
///   policies and routes priced by their science; classic/docs/ai-science-pace.md "Promoted");
///   `pre-tall` against itself keeps the old hash: 5749d0ae77b8a502 -> 0df05201ff04d810.
/// - 1 November, the seventh promotion: `standard` besieges walled cities as `siege-walls`
///   (classic/docs/ai-siege-walls.md "Promoted"); no army besieges a city in the smoke game's
///   70 rounds, the hash stays (as `pre-siege` against itself): 0df05201ff04d810.
/// - 2 November, the eighth promotion: `standard` puppets every captured city as `puppet-all`
///   (classic/docs/ai-conquest-payoff.md "Promoted"); no city is captured in the smoke game's
///   70 rounds, the hash stays (as `pre-puppet` against itself): 0df05201ff04d810.
/// - 2 November, the ninth promotion: `standard` besieges as `siege-join` (who besieges, the
///   melee closing in, the line of sight; classic/docs/ai-siege-walls.md "Promoted: `siege-join`");
///   the smoke game's 70 rounds play as `pre-join` against itself, the hash stays: 0df05201ff04d810
///   (at Quick speed too: 79d85a0b81b6f870 before and after).
/// - 2 November, the tenth promotion: `standard` fits its workers as `workers-fit` (a worker more
///   for the work left, the flight mid-build, the wait in a city, the repair;
///   classic/docs/ai-workers.md "Promoted"); the smoke game's 70 rounds play the same (old
///   `workers-fit` against itself gave old `standard`'s hash, measured, not traced), the hash stays,
///   as `pre-workers` against itself: 0df05201ff04d810 (at Quick speed too: 79d85a0b81b6f870).
/// - 2 November, the eleventh promotion: `standard` orders its units by their purpose and clears
///   the barbarian camps as `army-camps` (classic/docs/ai-army.md "Promoted"); the smoke game's 70
///   rounds play the same (old `army-camps` against itself gave old `standard`'s hash), the hash
///   stays, as `pre-camps` against itself: 0df05201ff04d810 (at Quick speed too: 79d85a0b81b6f870).
/// - 3 November, the twelfth promotion: `standard` plays the strategy net (classic/docs/ai-strategy-net.md);
///   its research, policies and production change the smoke game: 6eb9f185cee7f36e, the hash of the
///   benched `strategy-net` (build 4077901) against itself, measured; `standard-scripted` against
///   itself keeps the old 0df05201ff04d810.
/// - 4 November, balanced starts (classic/docs/start-balance.md): the smoke game's maps have other
///   starts (chosen by the real yields round them, with horses and iron near each) and other deposits, so
///   the game differs: 6eb9f185cee7f36e -> 6ee9092d56f035ca. No change of the AI.
const SMOKE_RESULT_HASH: &str = "6ee9092d56f035ca";

fn pairing(candidate: &str) -> Pairing {
    Pairing { baseline: "standard".to_owned(), candidate: candidate.to_owned() }
}

fn report(suite: &Suite, mode: Mode, pairing: &Pairing, jobs: usize) -> TournamentReport {
    let results = tournament::<ClassicBench>(suite, &suite.matches(mode, 0), pairing, jobs).expect("every match plays");
    let run = RunInfo { rules: "classic", suite, mode, pairing, seed_offset: 0, jobs, wall_seconds: 0.0 };
    TournamentReport::new(&run, &results)
}

#[test]
fn the_smoke_suite_gives_the_same_result_on_any_number_of_threads() {
    let suite = Suite::named("smoke").unwrap();
    let one = report(&suite, Mode::Versus, &pairing("standard"), 1);
    let two = report(&suite, Mode::Versus, &pairing("standard"), 2);
    assert_eq!(one.result_hash, two.result_hash);
    assert_eq!(one.result_hash, SMOKE_RESULT_HASH, "the smoke suite is pinned");

    // Equal minds play the same game in both seatings: the same player wins
    // it, once on a candidate seat and once on a baseline seat.
    assert_eq!(one.matches[0].hash, one.matches[1].hash);
    assert_eq!(one.matches[0].winner, one.matches[1].winner);
    let versus = one.versus.unwrap();
    assert_eq!((versus.candidate_wins, versus.draws, versus.share.mean), (1, 0, 0.5));
}

/// The bench's `standard` on every seat plays the game's own AI driver
/// (`classic_rules::ai::AiDriver`, `ClassicMind::default()`) move for move.
#[test]
fn the_standard_variant_plays_the_games_ai() {
    let suite = Suite::named("smoke").unwrap();
    let rules = ClassicBench::rules();
    let game_match = suite.matches(Mode::Solo, 0)[0];
    let bench = play_match::<ClassicBench>(&rules, &suite, &game_match, &pairing("standard")).unwrap();

    let mut game = ClassicBench::new_game(&rules, &game_match.map, game_match.seed, suite.rounds);
    let mut driver = AiDriver::new(AiSettings::for_rules(&rules, "core:handicap/prince"), Vec::new());
    while !game.is_over() && game.view().auth.turn().round <= suite.rounds {
        assert!(driver.run(&mut game, &rules, u32::MAX).round_done || game.is_over());
    }
    assert_eq!(bench.hash, authoritative_hash(&game));
    assert_eq!(bench.rounds, game.view().auth.turn().round);
}

/// With every leader's personality neutral, `leaders` plays the AI before
/// the leaders (`allocator`) move for move in every game without a war
/// (docs/ai-leaders.md): a neutral leader prices nothing otherwise and
/// pursues the score alone. Its differences are in wars: a capture that
/// would win the game, and peace by deal instead of alone (`ai::peace`).
/// The game's AI, `standard`, adds the defence (`ai::defence`) to the
/// leaders: its games may differ without a war.
#[test]
fn neutral_leaders_play_the_allocator() {
    neutral_leaders_on("smoke", "leaders", "allocator");
}

/// The same for the variants that price gold by its best use and happiness
/// by its effects (`PriceModel::Uses`): `leaders-econ2` is `allocator-econ2`
/// played by the leaders.
#[test]
fn neutral_econ2_leaders_play_the_econ2_allocator() {
    neutral_leaders_on("smoke", "leaders-econ2", "allocator-econ2");
}

/// The same on the games of the standard suite without a war (slow: run
/// with `--ignored`).
#[test]
#[ignore]
fn neutral_leaders_play_the_allocator_suite() {
    neutral_leaders_on("standard", "leaders", "allocator");
}

fn neutral_leaders_on(suite: &str, leaders: &str, allocator: &str) {
    let suite = Suite::named(suite).unwrap();
    let mut rules = ClassicBench::rules();
    rules.module.personalities.clear();
    for game_match in suite.matches(Mode::Versus, 0) {
        let against = |candidate: &str| Pairing { baseline: allocator.to_owned(), candidate: candidate.to_owned() };
        let alone = play_match::<ClassicBench>(&rules, &suite, &game_match, &against(allocator)).unwrap();
        if alone.seats.iter().any(|s| s.combat.wars > 0) {
            continue;
        }
        let led = play_match::<ClassicBench>(&rules, &suite, &game_match, &against(leaders)).unwrap();
        assert_eq!(led.hash, alone.hash, "{}", game_match.label());
    }
}

#[test]
fn variants_are_chosen_by_name_and_seat() {
    for variant in Variant::ALL {
        assert_eq!(Variant::from_name(variant.name()), Some(variant));
    }
    assert!(ClassicBench::mind("strongest").is_none());
    let suite = Suite::named("smoke").unwrap();
    let error = tournament::<ClassicBench>(&suite, &suite.matches(Mode::Versus, 0), &pairing("strongest"), 1).unwrap_err();
    assert!(error[0].contains("no AI variant strongest"), "{error:?}");

    let seats = vec![(PlayerId(0), ClassicMind::new(Variant::OneCity)), (PlayerId(2), ClassicMind::new(Variant::Nudged))];
    let seated = Seated::new(ClassicMind::new(Variant::Legacy), seats);
    let variants: Vec<Variant> = (0..4).map(|p| seated.mind(PlayerId(p)).variant()).collect();
    assert_eq!(variants, [Variant::OneCity, Variant::Legacy, Variant::Nudged, Variant::Legacy]);
    assert_eq!(ClassicMind::default().variant(), Variant::ALL[0], "the game plays the first variant");
    assert_eq!(Variant::ALL[0].name(), "standard");
}

/// Even and odd seatings give the candidate the alternate nations; a
/// weaker candidate changes the games and loses them.
#[test]
fn the_candidate_takes_alternate_seats() {
    let suite = Suite::named("smoke").unwrap();
    let rules = ClassicBench::rules();
    let game = ClassicBench::new_game(&rules, &suite.maps[0], suite.seeds[0], suite.rounds);
    assert_eq!(majors(&game).len(), 4);
    let sides = |seating: Seating| -> Vec<Side> { (0..4).map(|i| seating.side(i)).collect() };
    assert_eq!(sides(Seating::Even), [Side::Candidate, Side::Baseline, Side::Candidate, Side::Baseline]);
    assert_eq!(sides(Seating::Odd), [Side::Baseline, Side::Candidate, Side::Baseline, Side::Candidate]);

    let weaker = report(&suite, Mode::Versus, &pairing("one-city"), 2);
    let standard = report(&suite, Mode::Versus, &pairing("standard"), 2);
    assert_ne!(weaker.matches[0].hash, standard.matches[0].hash, "the variant plays differently");
    assert_eq!(weaker.versus.unwrap().candidate_wins, 0);
    let candidate = &weaker.sides[0];
    assert_eq!(candidate.side, Side::Candidate);
    assert_eq!(candidate.standings.last().unwrap().cities.mean, 1.0, "one city each");
    assert!(weaker.sides[1].standings.last().unwrap().cities.mean > 1.0, "the standard AI expands");
}

/// A sequential run (`--sprt`) decides on the seed blocks in the
/// schedule's order whatever the threads: `one-city` loses every block and
/// is rejected at the minimum; equal versions take half of every block and
/// play to the cap; against their own saved reports as the reference every
/// difference is 0, and a game the reference lacks is an error.
#[test]
fn a_sequential_run_decides_on_the_blocks_in_order() {
    let suite = Suite::named("smoke").unwrap();
    let sprt = Sprt { s0: 0.45, s1: 0.55, alpha: 0.05, beta: 0.05 };
    let plan = Plan { min_blocks: 2, max_blocks: 3 };
    let labels = |games: &[aurelia_qa::bench::play::MatchResult]| -> Vec<(String, u64)> { games.iter().map(|g| (g.label.clone(), g.hash)).collect() };
    let weak = |jobs| play_sequential::<ClassicBench>(&suite, &pairing("one-city"), 0, sprt, plan, None, jobs).unwrap();
    let (one, three) = (weak(1), weak(3));
    assert_eq!((one.outcome.decision, one.outcome.blocks, one.outcome.games), (Decision::H0, 2, 4));
    assert_eq!(labels(&one.results), labels(&three.results), "the same games on any number of threads");
    assert_eq!(one.results[2].seed, 1001, "the second block is the next offset's seed");

    let equal = play_sequential::<ClassicBench>(&suite, &pairing("standard"), 0, sprt, plan, None, 2).unwrap();
    assert_eq!((equal.outcome.decision, equal.outcome.blocks, equal.outcome.mean), (Decision::Undecided, 3, 0.5));

    let dir = std::env::temp_dir().join(format!("ai-bench-reference-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let paths: Vec<String> = (0..2)
        .map(|offset| {
            let pairing = pairing("standard");
            let results = tournament::<ClassicBench>(&suite, &suite.matches(Mode::Versus, offset), &pairing, 2).unwrap();
            let run = RunInfo { rules: "classic", suite: &suite, mode: Mode::Versus, pairing: &pairing, seed_offset: offset, jobs: 2, wall_seconds: 0.0 };
            let path = dir.join(format!("offset-{offset}.json"));
            std::fs::write(&path, serde_json::to_string(&TournamentReport::new(&run, &results)).unwrap()).unwrap();
            path.display().to_string()
        })
        .collect();
    let reference = Reference::load(&paths, &suite, &pairing("standard")).unwrap();
    let paired = Sprt { s0: -0.05, s1: 0.05, ..sprt };
    let two = Plan { min_blocks: 2, max_blocks: 2 };
    let same = play_sequential::<ClassicBench>(&suite, &pairing("standard"), 0, paired, two, Some(&reference), 2).unwrap();
    assert_eq!((same.outcome.decision, same.outcome.mean, same.outcome.reference_share), (Decision::Undecided, 0.0, Some(0.5)));
    let missing = play_sequential::<ClassicBench>(&suite, &pairing("standard"), 0, paired, plan, Some(&reference), 2).err().unwrap();
    assert!(missing[0].contains("seed 2001"), "{missing:?}");
    std::fs::remove_dir_all(&dir).unwrap();
}

