//! The AI turns on a worker play the same game as the sliced path: same commands, same
//! journal, same hashes, same saves (platform/structure.md §37). Failures of the worker surface as
//! an error and never hang the host.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use classic_rules::classic::Classic;
use classic_rules::compile_rules;
use classic_rules::ids::PlayerId;
use classic_rules::persistence::encode::encode;
use classic_rules::replay::hash::authoritative_hash;
use classic_rules::rules::SourceRules;
use classic_rules::save_codec::to_bytes;
use classic_rules::scenario::{generate, ScenarioConfig};
use client::ai_round::{AiWake, RoundError, RoundReply, RoundRequest, RoundWorker, ThreadWorker};
use client::simulation_driver::{DriverConfig, SimulationDriver, TickOutcome};
use client::GameClient;
use render::renderer::Renderer;

type Client = GameClient<Classic>;
type Driver = SimulationDriver<Classic>;

fn client(seed: u64, players: u8, size: u32, cities: usize, units: usize) -> Client {
    let rules = compile_rules(SourceRules::default()).unwrap();
    let cfg = ScenarioConfig {
        seed,
        players,
        width: size,
        height: size,
        cities,
        units,
        water_ratio: 20,
    };
    let game = generate(&cfg, &rules);
    let mut client = GameClient::new(game, Arc::new(rules), Renderer::headless(), PlayerId(0));
    client.record_journal();
    client
}

fn driver(client: &Client) -> Driver {
    let mut driver = SimulationDriver::new(DriverConfig::default(), &client.rules);
    driver.record_journal();
    driver
}

fn counted_wake() -> (AiWake, Arc<AtomicUsize>) {
    let wakes = Arc::new(AtomicUsize::new(0));
    let counter = wakes.clone();
    (
        Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }),
        wakes,
    )
}

/// The wake comes right after the answer is sent, from the worker's thread: wait for it.
fn wait_for_wakes(wakes: &AtomicUsize, at_least: usize) {
    let started = std::time::Instant::now();
    while wakes.load(Ordering::SeqCst) < at_least && started.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        wakes.load(Ordering::SeqCst) >= at_least,
        "the worker woke the host after every answer"
    );
}

fn offloaded(client: &Client) -> (Driver, Arc<AtomicUsize>) {
    let mut driver = driver(client);
    let (wake, wakes) = counted_wake();
    driver.offload_on_thread(client.rules.clone(), wake);
    driver.set_apply_slice(Duration::ZERO); // the tests choose their slices
    assert!(driver.offloaded());
    (driver, wakes)
}

fn hash(client: &Client) -> u64 {
    authoritative_hash(&client.game)
}

/// The game and the AI memory as the save file holds them.
fn save(client: &Client, driver: &Driver) -> Vec<u8> {
    let mut save = encode(&client.game, &client.rules);
    save.ai = driver.save_ai();
    to_bytes(&save)
}

fn journal(client: &mut Client) -> String {
    format!("{:?}", client.take_journal())
}

/// One round with frame-sized ticks (the way the window plays it); returns the ticks it took.
fn round_in_frames(client: &mut Client, driver: &mut Driver, slice: Duration) -> usize {
    driver.end_human_turn(client).unwrap();
    let mut ticks = 0;
    loop {
        ticks += 1;
        match driver.tick(client, slice) {
            TickOutcome::Busy => {
                assert!(
                    driver.busy(client),
                    "gameplay input stays blocked while the AI turns run"
                );
                std::thread::sleep(Duration::from_micros(200));
            }
            TickOutcome::RoundDone => break,
            TickOutcome::Idle => {
                panic!("an AI round ends in RoundDone: {:?}", driver.take_failure())
            }
        }
        assert!(ticks < 1_000_000, "the AI round never ends");
    }
    ticks
}

/// The proof: the same seed played on the worker and sliced gives the same command stream,
/// the same journal, the same hashes and the same save bytes, round after round.
fn same_game(
    seed: u64,
    players: u8,
    size: u32,
    cities: usize,
    units: usize,
    rounds: usize,
    slice: Duration,
) {
    let mut sliced = client(seed, players, size, cities, units);
    let mut worker = client(seed, players, size, cities, units);
    let mut a = driver(&sliced);
    let (mut b, wakes) = offloaded(&worker);
    assert_eq!(hash(&sliced), hash(&worker));
    // Both games record the command journal of platform/docs/saves.md §3 from here.
    assert_eq!(
        sliced.game.begin_journal(&sliced.rules),
        worker.game.begin_journal(&worker.rules)
    );
    let mut commands = 0;
    let mut layers = (sliced.render_world.layers, worker.render_world.layers);
    for round in 0..rounds {
        let counts_before = a.command_counts();
        a.end_human_turn(&mut sliced).unwrap();
        a.finish(&mut sliced);
        round_in_frames(&mut worker, &mut b, slice);
        assert_eq!(
            hash(&sliced),
            hash(&worker),
            "seed {seed}, round {round}: the games differ"
        );
        let (journal_a, journal_b) = (journal(&mut sliced), journal(&mut worker));
        assert!(
            journal_a == journal_b,
            "seed {seed}, round {round}: the journals differ"
        );
        assert!(
            sliced.game.take_journal() == worker.game.take_journal(),
            "seed {seed}, round {round}: the recorded command journals differ"
        );
        commands += a.command_counts().0 - counts_before.0;
        assert_eq!(
            a.command_counts(),
            b.command_counts(),
            "seed {seed}, round {round}: the counters differ"
        );
        assert!(
            save(&sliced, &a) == save(&worker, &b),
            "seed {seed}, round {round}: the saves differ"
        );
        assert_eq!(a.busy(&sliced), b.busy(&worker));
        // The projection moved the same way (the revisions count from the process, so compare the steps).
        let steps = |c: &Client, before: &render::world::state::LayerRevisions| {
            render::world::state::ALL_LAYERS
                .map(|layer| c.render_world.layers.get(layer) - before.get(layer))
        };
        assert_eq!(
            steps(&sliced, &layers.0),
            steps(&worker, &layers.1),
            "round {round}: the projections differ"
        );
        layers = (sliced.render_world.layers, worker.render_world.layers);
    }
    assert!(
        commands > 3 * rounds as u64,
        "seed {seed}: the AI played ({commands} commands)"
    );
    wait_for_wakes(&wakes, rounds);
    worker.game.validate(&worker.rules).unwrap();
}

#[test]
fn a_small_game_on_the_worker_is_the_sliced_game() {
    same_game(77, 3, 24, 1, 2, 12, Duration::from_millis(3));
}

#[test]
fn a_bigger_game_on_the_worker_is_the_sliced_game_at_any_slice() {
    same_game(5, 6, 48, 3, 6, 25, Duration::from_millis(3));
    eprintln!("ok");
    // One command per tick, and a tick that waits for the worker.
    same_game(6, 4, 32, 2, 3, 10, Duration::ZERO);
}

/// A save loaded in the middle of the game: the AI memory travels with the request.
#[test]
fn a_loaded_game_continues_on_the_worker_as_it_would_sliced() {
    let mut live = client(11, 4, 32, 2, 3);
    let mut sliced = driver(&live);
    for _ in 0..8 {
        sliced.round(&mut live);
    }
    let saved = save(&live, &sliced);
    let load = |bytes: &[u8]| {
        let read = classic_rules::save_codec::from_bytes(bytes).unwrap();
        let game = classic_rules::persistence::decode::decode(&read, &live.rules).unwrap();
        let mut client =
            GameClient::new(game, live.rules.clone(), Renderer::headless(), PlayerId(0));
        client.record_journal();
        (client, read.ai)
    };
    let (mut a_client, ai) = load(&saved);
    let mut a = driver(&a_client);
    a.load_ai(&ai, &a_client).unwrap();
    let (mut b_client, ai) = load(&saved);
    let (mut b, _) = offloaded(&b_client);
    b.load_ai(&ai, &b_client).unwrap();
    for round in 0..8 {
        a.round(&mut a_client);
        b.round(&mut b_client);
        assert_eq!(hash(&a_client), hash(&b_client), "round {round}");
        assert!(save(&a_client, &a) == save(&b_client, &b), "round {round}");
    }
}

/// A worker that answers with whatever it is told, to test what the host does with it.
#[derive(Debug)]
struct Scripted {
    tamper: bool,
    fail: Option<RoundError>,
    reply: Option<Result<RoundReply, RoundError>>,
    rules: Arc<classic_rules::CompiledRuleset>,
}

impl RoundWorker for Scripted {
    fn submit(&mut self, request: RoundRequest) {
        self.reply = Some(match self.fail.take() {
            Some(error) => Err(error),
            None => {
                let mut reply =
                    client::ai_round::run_round::<Classic>(&self.rules, &request, &|| false);
                if let (true, Ok(reply)) = (self.tamper, reply.as_mut()) {
                    reply.hash ^= 1;
                }
                reply
            }
        });
    }
    fn poll(&mut self, _wait: Duration) -> Option<Result<RoundReply, RoundError>> {
        self.reply.take()
    }
    fn cancel(&mut self) {}
}

fn scripted(tamper: bool, fail: Option<RoundError>, client: &Client) -> Driver {
    let mut driver = driver(client);
    driver.offload(Box::new(Scripted {
        tamper,
        fail,
        reply: None,
        rules: client.rules.clone(),
    }));
    driver
}

#[test]
fn a_worker_that_panics_is_an_error_not_a_hang() {
    let mut c = client(3, 3, 24, 1, 2);
    let mut d = scripted(false, Some(RoundError::Panicked("boom".into())), &c);
    d.end_human_turn(&mut c).unwrap();
    assert!(d.busy(&c));
    assert_eq!(d.tick(&mut c, Duration::from_millis(3)), TickOutcome::Busy);
    assert_eq!(d.tick(&mut c, Duration::from_millis(3)), TickOutcome::Idle);
    assert!(!d.busy(&c), "the host is not blocked for ever");
    let failure = d.take_failure().expect("the failure is reported");
    assert!(failure.contains("boom"), "{failure}");
    assert_eq!(d.take_failure(), None, "reported once");
    assert_eq!(d.tick(&mut c, Duration::from_millis(3)), TickOutcome::Idle);
}

#[test]
fn a_diverged_worker_is_an_error_not_a_silent_corruption() {
    let mut c = client(3, 3, 24, 1, 2);
    let mut d = scripted(true, None, &c);
    d.end_human_turn(&mut c).unwrap();
    let mut outcome = TickOutcome::Busy;
    for _ in 0..10_000 {
        outcome = d.tick(&mut c, Duration::from_millis(3));
        if outcome != TickOutcome::Busy {
            break;
        }
    }
    assert_eq!(outcome, TickOutcome::Idle);
    let failure = d.take_failure().expect("the divergence is reported");
    assert!(failure.contains("diverged"), "{failure}");
}

/// A real thread: bytes it cannot read come back as an error, the thread lives on and
/// plays the next round; dropping the driver in the middle of a job ends it cleanly.
#[test]
fn the_thread_worker_reports_bad_requests_and_stops_when_dropped() {
    let c = client(3, 3, 24, 1, 2);
    let (wake, wakes) = counted_wake();
    let mut worker = ThreadWorker::new(c.rules.clone(), wake);
    worker.submit(RoundRequest {
        state: vec![1, 2, 3],
        difficulty: String::new(),
        humans: vec![PlayerId(0)],
    });
    let answer = worker.poll(Duration::from_secs(60)).expect("an answer");
    assert!(matches!(answer, Err(RoundError::Request(_))), "{answer:?}");
    wait_for_wakes(&wakes, 1);

    let mut c = c;
    let mut d = driver(&c);
    d.end_human_turn(&mut c).unwrap();
    let request = RoundRequest::of(
        &c.game,
        &c.rules,
        d.save_ai(),
        d.difficulty(),
        vec![PlayerId(0)],
    );
    worker.submit(request);
    drop(worker); // cancels and joins; must not hang
}

/// Replacing the game while the worker plays (a load or a new game) drops the job.
#[test]
fn a_reset_during_a_job_cancels_it() {
    let mut c = client(3, 3, 24, 1, 2);
    let (mut d, _) = offloaded(&c);
    d.end_human_turn(&mut c).unwrap();
    assert_eq!(d.tick(&mut c, Duration::from_millis(3)), TickOutcome::Busy);
    d.reset(&c.rules);
    assert!(d.offloaded(), "the worker stays");
    let mut fresh = client(4, 3, 24, 1, 2);
    let before = fresh.game.view().auth.turn().round;
    d.round(&mut fresh);
    assert_eq!(d.take_failure(), None);
    assert_eq!(fresh.game.view().auth.turn().round, before + 1);
}
