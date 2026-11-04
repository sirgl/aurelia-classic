//! The book opponents of the E2 league (classic/docs/ai-books.md): league
//! opponents and probes only, never candidates for `standard`. Each book
//! issues its plan's first orders in a fresh duel, and the rushes declare war
//! and attack by a bounded round.
//!
//! `BOOK_TRACE=1` prints each book's events (the probe of a book that never
//! takes a capital).

use aurelia_qa::bench::modules::{majors, BenchRules, ClassicBench};
use aurelia_qa::bench::suite::MapSpec;
use classic_rules::ai::books::{Book, Phase};
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::variant::Variant;
use classic_rules::classic::Classic;
use kernel::ai::driver::AiDriver;
use classic_rules::{Command, CompiledRuleset, GameState};
use kernel::ai::seats::Seated;
use kernel::ai::StepOutcome;
use kernel::ids::PlayerId;

type Driver = AiDriver<Classic, Seated<ClassicMind>>;

const BOOKS: [&str; 5] = ["rush-siege", "rush-archer", "rush-sword", "timing-uu", "rex-wide"];

fn round(game: &GameState) -> u32 {
    game.view().auth.turn().round
}

/// A duel on `script` with seed `seed`: the book on the first nation's seat,
/// `standard` on the other.
fn duel(rules: &CompiledRuleset, book: &str, script: &'static str, seed: u64) -> (GameState, Driver, PlayerId) {
    duel_against(rules, book, script, seed, "standard")
}

/// [`duel`] against the rival `rival` (a bench mind by its name).
fn duel_against(rules: &CompiledRuleset, book: &str, script: &'static str, seed: u64, rival: &str) -> (GameState, Driver, PlayerId) {
    // The owner measures at Quick speed (198 rounds); `BOOK_SPEED` overrides.
    let speed = std::env::var("BOOK_SPEED").unwrap_or_else(|_| "quick".to_owned());
    let rounds = if speed == "quick" { 198 } else { 300 };
    let game = ClassicBench::new_game_at(rules, &MapSpec { script, size: "duel" }, seed, rounds, &speed);
    let nations = majors(&game);
    let seat = nations[0];
    let mind = ClassicBench::mind(book).expect("a book by its name");
    let mut driver = Driver::with_mind(ClassicBench::settings(rules), Vec::new(), Seated::new(ClassicBench::mind(rival).unwrap(), vec![(seat, mind)]));
    driver.record_journal();
    (game, driver, seat)
}

/// Plays until the end of round `last` (or the game's end); the commands the
/// book's player had applied, by round.
fn play(game: &mut GameState, rules: &CompiledRuleset, driver: &mut Driver, seat: PlayerId, last: u32) -> Vec<(u32, Command)> {
    let mut applied = Vec::new();
    while !game.is_over() && round(game) <= last {
        loop {
            let now = round(game);
            let active = game.view().auth.active_player();
            let outcome = driver.step(game, rules);
            for command in driver.take_journal() {
                if active == seat {
                    applied.push((now, command.command));
                }
            }
            match outcome {
                StepOutcome::Worked | StepOutcome::PlayerDone(_) => {}
                StepOutcome::RoundDone => break,
                StepOutcome::Waiting => return applied,
            }
        }
    }
    applied
}

fn trace(driver: &mut Driver, seat: PlayerId, label: &str) {
    if std::env::var_os("BOOK_TRACE").is_some() {
        let book = driver.mind_mut().mind(seat).book().expect("a book");
        eprintln!("== {label}: {:?}", book.plan(seat).map(|p| (p.techs, p.cities, p.army)));
        for (round, event) in book.events(seat) {
            eprintln!("  r{round} {event}");
        }
    }
}

#[test]
fn the_books_are_league_variants_that_play_standard_with_a_book() {
    for name in BOOKS {
        let variant = Variant::from_name(name).expect("a book by its name");
        assert!(variant.book().is_some(), "{name} is a book");
        let mind = ClassicMind::new(variant);
        assert_eq!(mind.variant(), variant, "{name}: the mind names its book");
        assert!(mind.book().is_some());
    }
    assert!(Variant::Standard.book().is_none());
    // `siege-walls`: the alias of `standard` since the seventh promotion (not a book).
    let walls = ClassicMind::new(Variant::from_name("siege-walls").expect("siege-walls"));
    assert_eq!(walls.variant(), Variant::SiegeWalls);
    assert!(walls.book().is_none());
    assert_eq!(Variant::from_name("timing-uu").and_then(|v| v.book()), Some(Book::TimingUu));
}

/// A league book on another base (`book@base`, `ClassicMind::book_on`, classic/docs/ai-books.md
/// «Re-based on the 2 November standard»): the books as measured are `book@pre-puppet` and
/// `tall-book@pre-tall`, game for game; on another base the mind keeps the book's name.
#[test]
fn a_book_on_its_measured_base_plays_its_games() {
    let rules = ClassicBench::rules();
    for (old, named) in [("rush-siege", "rush-siege@pre-puppet"), ("tall-book", "tall-book@pre-tall")] {
        let hashes: Vec<u64> = [old, named]
            .into_iter()
            .map(|name| {
                let (mut game, mut driver, seat) = duel(&rules, name, "pangaea", 1);
                play(&mut game, &rules, &mut driver, seat, 100);
                kernel::replay::hash::authoritative_hash(&game)
            })
            .collect();
        assert_eq!(hashes[0], hashes[1], "{named} plays {old}'s games");
    }
    let rush = ClassicBench::mind("rush-siege@standard").expect("a book on standard");
    assert_eq!(rush.variant(), Variant::RushSiege);
    assert!(rush.book().is_some());
    let tall = ClassicBench::mind("tall-book@standard").expect("the tall book on standard");
    assert_eq!(tall.variant(), Variant::TallBook);
    assert!(ClassicBench::mind("rush-siege@net").is_none() && ClassicBench::mind("rush-siege@timing-uu").is_none());
}

/// The march into the fog (classic/docs/ai-books.md «The march into the fog»): on
/// the land duel of seed 19 (seed 11 before the balanced starts of 4 November moved every world; the
/// rounds and distances below were measured on that old world, on seed 19 `standard-scripted`'s army
/// stops 7 tiles out of sight and `fog-march`'s 2 tiles from the walls) the rival's capital is known from the embassy of round 35,
/// the land between is not. `rush-siege@standard` launches at round 63 and its army
/// stands 8-9 tiles out from round 74 on, out of the capital's sight (the march's legs
/// aim at the tiles the embassy showed around the capital, cut off by the fog); on
/// `fog-march` it walks on through the unseen land to the capital's walls.
#[test]
fn a_book_on_fog_march_walks_through_the_fog_to_its_target() {
    use classic_rules::ai::war_probe;
    let rules = ClassicBench::rules();
    let mut seen = Vec::new();
    // The scene was measured on the standard `fog-march` is built on: since the twelfth promotion
    // that is `standard-scripted`, the base and the rival (`standard` plays the strategy net).
    for book in ["rush-siege@standard-scripted", "rush-siege@fog-march"] {
        war_probe::enable(true);
        let (mut game, mut driver, seat) = duel_against(&rules, book, "pangaea", 19, "standard-scripted");
        play(&mut game, &rules, &mut driver, seat, 115);
        let probe = war_probe::take();
        war_probe::enable(false);
        let ours: Vec<_> = probe.marches.iter().filter(|s| s.player == seat && s.nearest >= 0).collect();
        assert!(!ours.is_empty(), "{book} marches by round 115");
        let closest = ours.iter().map(|s| s.nearest).min().unwrap_or(i32::MAX);
        seen.push((ours.iter().any(|s| s.sighted), closest));
    }
    assert!(!seen[0].0 && seen[0].1 >= 7, "on standard the army stalls out of sight: {:?}", seen[0]);
    assert!(seen[1].1 <= 3, "on fog-march it reaches the walls: {:?}", seen[1]);
}

/// Every book resolves a plan for every nation: a beeline, a policy line, an
/// army of units the nation may build (its unique unit where one fits).
#[test]
fn every_book_resolves_its_plan_for_every_nation() {
    let rules = ClassicBench::rules();
    let key = |k: kernel::ids::UnitKindId| rules.units[k.0 as usize].key.clone();
    let huns = (0..=u8::MAX).find(|n| rules.units.iter().any(|u| u.nation == Some(*n) && u.key == "core:unit/battering-ram")).expect("the Huns");
    let siege = classic_rules::ai::books::plans::Plan::of(Book::RushSiege, &rules, huns);
    assert_eq!(key(siege.army[0].kinds[0]), "core:unit/horse-archer", "the Huns' ranged units are horse archers");
    assert_eq!(key(siege.army[1].kinds[0]), "core:unit/battering-ram", "the Huns' siege units are rams");
    let arabia = (0..=u8::MAX).find(|n| rules.units.iter().any(|u| u.nation == Some(*n) && u.key == "core:unit/camel-archer")).expect("Arabia");
    let uu = classic_rules::ai::books::plans::timing_unit(&rules, arabia).map(key);
    assert_eq!(uu.as_deref(), Some("core:unit/camel-archer"), "Arabia's timing unit");
    let nations: Vec<u8> = {
        let mut n: Vec<u8> = rules.units.iter().filter_map(|u| u.nation).collect();
        n.sort();
        n.dedup();
        n
    };
    for book in [Book::RushSiege, Book::RushArcher, Book::RushSword, Book::TimingUu, Book::RexWide] {
        for nation in &nations {
            let plan = classic_rules::ai::books::plans::Plan::of(book, &rules, *nation);
            assert!(!plan.techs.is_empty(), "{book:?} for nation {nation}: a beeline");
            assert!(!plan.branches.is_empty(), "{book:?}: a policy line");
            assert!(plan.army.iter().all(|s| !s.kinds.is_empty()), "{book:?} for nation {nation}: every slot has a unit");
        }
    }
}

/// In a fresh duel every book orders its plan's first steps: the beeline's
/// research and the build order's first units, within the first rounds.
#[test]
fn every_book_issues_its_first_orders_in_a_fresh_game() {
    let rules = ClassicBench::rules();
    for name in BOOKS {
        let (mut game, mut driver, seat) = duel(&rules, name, "pangaea", 1);
        let applied = play(&mut game, &rules, &mut driver, seat, 6);
        let plan = driver.mind_mut().mind(seat).book().and_then(|b| b.plan(seat)).expect("the book planned");
        let researched = applied.iter().any(|(_, c)| matches!(c, Command::PlanResearch { target, .. } if plan.techs.contains(target)));
        assert!(researched, "{name}: the beeline's research is ordered");
        let units: Vec<_> = applied.iter().filter_map(|(_, c)| match c {
            Command::EnqueueProduction { item: kernel::ids::ProductionItem::Unit(kind), .. } => Some(rules.units[kind.0 as usize].key.clone()),
            _ => None,
        }).collect();
        assert!(!units.is_empty(), "{name}: the build order's units are queued");
        trace(&mut driver, seat, name);
    }
}

/// The rushes declare war and take the field in a duel by a bounded round
/// (seed 1 of the pangaea duel: the rival capital is on the same land).
#[test]
fn the_rushes_declare_war_and_attack_by_a_bounded_round() {
    let rules = ClassicBench::rules();
    for (name, by) in [("rush-siege", 90), ("rush-archer", 100), ("rush-sword", 105)] {
        let (mut game, mut driver, seat) = duel(&rules, name, "pangaea", 1);
        let applied = play(&mut game, &rules, &mut driver, seat, by);
        trace(&mut driver, seat, name);
        if std::env::var_os("BOOK_TRACE").is_some() {
            for (r, c) in &applied {
                match c {
                    Command::EnqueueProduction { item, .. } => eprintln!("  r{r} build {item:?}"),
                    Command::HurryProduction { .. } | Command::DeclareWar { .. } | Command::PlanResearch { .. } | Command::AdoptPolicy { .. } | Command::UnlockPolicyBranch { .. } | Command::FoundCity { .. } => eprintln!("  r{r} {c:?}"),
                    _ => {}
                }
            }
        }
        let declared = applied.iter().find(|(_, c)| matches!(c, Command::DeclareWar { .. })).map(|(r, _)| *r);
        assert!(declared.is_some(), "{name}: war declared by round {by}");
        let attacks = applied.iter().filter(|(_, c)| matches!(c, Command::AttackCity { .. } | Command::AttackUnit { .. } | Command::RangedAttack { .. })).count();
        assert!(attacks > 0, "{name}: its army attacked by round {by}");
        let phase = driver.mind_mut().mind(seat).book().and_then(|b| b.phase(seat));
        assert!(matches!(phase, Some(Phase::Attack { .. } | Phase::Over)), "{name}: {phase:?}");
    }
}

/// The probe of one duel (`BOOK_PROBE=book:script:seed:rounds`): the book's
/// events and both players' war and peace commands, by round.
#[test]
#[ignore = "a probe: BOOK_PROBE=rush-siege:pangaea:1:150"]
fn probe() {
    let spec = std::env::var("BOOK_PROBE").unwrap_or_else(|_| "rush-siege:pangaea:1:150".to_owned());
    let parts: Vec<&str> = spec.split(':').collect();
    let script: &'static str = Box::leak(parts[1].to_owned().into_boxed_str());
    let rules = ClassicBench::rules();
    let (mut game, mut driver, seat) = duel(&rules, parts[0], script, parts[2].parse().unwrap());
    let last: u32 = parts[3].parse().unwrap();
    driver.set_trace_capacity(2_000_000);
    while !game.is_over() && round(&game) <= last {
        loop {
            let now = round(&game);
            let active = game.view().auth.active_player();
            let outcome = driver.step(&mut game, &rules);
            for applied in driver.take_journal() {
                let c = &applied.command;
                if matches!(c, Command::DeclareWar { .. } | Command::MakePeace { .. } | Command::ProposeDeal { .. } | Command::RespondDeal { .. } | Command::AttackCity { .. }) {
                    eprintln!("r{now} p{} {c:?}", active.0);
                }
                for change in &applied.changes.city_changes {
                    if let kernel::transaction::journal::CityChange::OwnerChanged { from, to, .. } = change {
                        eprintln!("r{now} city taken {from:?} -> {to:?}");
                    }
                }
            }
            match outcome {
                StepOutcome::RoundDone | StepOutcome::Waiting => break,
                _ => {}
            }
        }
    }
    if let Some((from, to)) = std::env::var("BOOK_PROBE_ROUNDS").ok().and_then(|w| w.split_once('-').map(|(a, b)| (a.parse::<u32>().unwrap(), b.parse::<u32>().unwrap()))) {
        for entry in driver.trace().entries().filter(|e| e.player == seat && e.round >= from && e.round <= to) {
            eprintln!("trace {entry}");
        }
    }
    let book = driver.mind_mut().mind(seat).book().expect("a book");
    for (round, event) in book.events(seat) {
        eprintln!("book r{round} {event}");
    }
}

/// The probe over the duel suite's first seeds (`BOOK_PROBE=book:rounds:seeds`):
/// per game, the book's phase changes and the wars, peaces and captures.
#[test]
#[ignore = "a probe: BOOK_PROBE=rush-siege:150:4"]
fn probe_suite() {
    let spec = std::env::var("BOOK_PROBE").unwrap_or_else(|_| "rush-siege:150:4".to_owned());
    let parts: Vec<&str> = spec.split(':').collect();
    let (book, last, seeds): (&str, u32, u64) = (parts[0], parts[1].parse().unwrap(), parts[2].parse().unwrap());
    let rules = ClassicBench::rules();
    let scripts: Vec<String> = std::env::var("BOOK_SCRIPTS").map(|s| s.split(',').map(str::to_owned).collect()).unwrap_or_else(|_| ["continents", "pangaea", "fractal", "archipelago", "small-continents"].map(str::to_owned).to_vec());
    for script in &scripts {
        let script: &'static str = Box::leak(script.clone().into_boxed_str());
        for seed in 1..=seeds {
            let (mut game, mut driver, seat) = duel(&rules, book, script, seed);
            let mut log = Vec::new();
            while !game.is_over() && round(&game) <= last {
                loop {
                    let now = round(&game);
                    let active = game.view().auth.active_player();
                    let outcome = driver.step(&mut game, &rules);
                    for applied in driver.take_journal() {
                        match &applied.command {
                            Command::DeclareWar { .. } => log.push(format!("r{now} p{} war", active.0)),
                            Command::RespondDeal { accept: true, .. } => log.push(format!("r{now} p{} accepts", active.0)),
                            Command::AttackCity { .. } => log.push(format!("r{now} p{} hits city", active.0)),
                            _ => {}
                        }
                        for change in &applied.changes.city_changes {
                            if let kernel::transaction::journal::CityChange::OwnerChanged { from, to, .. } = change {
                                log.push(format!("r{now} city {from:?}->{to:?}"));
                            }
                        }
                    }
                    if matches!(outcome, StepOutcome::RoundDone | StepOutcome::Waiting) {
                        break;
                    }
                }
            }
            log.dedup();
            let book = driver.mind_mut().mind(seat).book().expect("a book");
            let events: Vec<String> = book.events(seat).into_iter().filter(|(_, e)| !e.starts_with("attack ")).map(|(r, e)| format!("r{r} {e}")).collect();
            eprintln!("{script}:{seed} seat p{} | {} | {}", seat.0, events.join("; "), log.join("; "));
        }
    }
}
