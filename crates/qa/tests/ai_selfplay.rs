//! The E2 games (`aurelia_qa::selfplay`) and their shards (`aurelia_qa::e2`,
//! classic/docs/ai-net-data.md, «E2: the net's own games»): a sampled game
//! is a function of its job and replays from its log to the same world; the
//! export rebuilds every decision of the learner exactly (no decision
//! dropped), labels it with the net's own choice and carries the sidecar's
//! log-probabilities; the members of a group share the start and differ in
//! the sampling.

use aurelia_qa::bench::modules::{BenchRules, ClassicBench};
use aurelia_qa::bench::suite::Suite;
use aurelia_qa::e2;
use aurelia_qa::export::Selection;
use aurelia_qa::grounds;
use aurelia_qa::selfplay::{self, Config, FromLogs, LearnerSeat};
use classic_rules::ai::net_e2e::encode::head;

fn config() -> Config {
    let weights = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../classic-rules/tests/fixtures/net_mind/random-small.bin");
    Config {
        suite: Suite::named("smoke").unwrap(),
        sizes: Vec::new(),
        rounds: 25,
        net: weights.to_str().unwrap().to_owned(),
        net_seats: 1,
        opponents: vec!["standard".to_owned(), "pre-war-fix".to_owned()],
        temperature: 1.0,
        group: 2,
        seed_offset: 0,
        sample_seed: 5,
        from: None,
        learner_mind: None,
        speed: String::new(),
        heads: None,
        decision_values: None,
        ladder_solvable: false,
        grounds: None,
    }
}

/// The science-victory fields of a living seat are within their bounds.
fn assert_space_sane(meta: &selfplay::GameMeta) {
    for seat in meta.seats.iter().filter(|s| s.alive) {
        let x = &seat.space;
        assert!(x.techs > 0 && x.science > 0, "{seat:?}");
        assert!(x.space_techs_needed > 0 && x.space_techs <= x.space_techs_needed && x.space_techs <= x.techs, "{seat:?}");
        assert!(x.projects_needed > 0 && x.projects <= x.projects_needed, "{seat:?}");
        assert!(x.parts_needed > 0 && x.parts_installed <= x.parts && x.parts <= x.parts_needed, "{seat:?}");
    }
    for seat in &meta.seats {
        assert_eq!(seat.victory.is_some(), seat.victory_round.is_some());
        assert_eq!(seat.victory.is_some(), meta.victory.as_ref().is_some_and(|v| v.winner == seat.player));
    }
}

#[test]
fn sampled_games_replay_and_export_aligned() {
    let rules = ClassicBench::rules();
    let config = config();
    let jobs = selfplay::jobs(&config, 3);
    assert_eq!(jobs.iter().map(|j| (j.group, j.member)).collect::<Vec<_>>(), [(0, 0), (0, 1), (1, 0)]);
    assert_eq!(jobs[0].spec, jobs[1].spec, "a group shares its start");
    assert_ne!(jobs[0].sample_seed, jobs[1].sample_seed);
    assert_ne!(jobs[0].spec.seed, jobs[2].spec.seed);
    let mut hashes = Vec::new();
    for job in &jobs {
        let played = selfplay::play(&rules, &config, job).unwrap();
        let again = selfplay::play(&rules, &config, job).unwrap();
        assert_eq!(played.journal, again.journal, "{}: the same job, the same log", job.stem());
        assert_eq!(played.trace, again.trace);
        assert_eq!(played.meta.seats, again.meta.seats);
        let learner = played.meta.seats.iter().find(|s| s.learner).expect("a learner seat");
        assert_eq!(played.meta.learners, vec![learner.player]);
        assert_eq!(played.meta.seats.iter().filter(|s| s.rank == 1).count(), 1);
        assert_space_sane(&played.meta);
        let (shard, arrays, counts) = e2::shard(&rules, &job.spec, &job.stem(), &played.journal, &played.trace, Selection::Sample(1)).unwrap();
        assert_eq!(counts.hash, played.meta.hash, "{}: the log replays to the game's world", job.stem());
        assert!(counts.dropped.is_empty(), "{}: {:?}", job.stem(), counts.dropped);
        assert_eq!(counts.unit_commands_not_sent, 0);
        assert_eq!(counts.aligned + counts.empty, counts.decisions);
        assert!(counts.aligned > 50, "{counts:?}");
        // Labels and log-probabilities: the sidecar's, in the shard's order (one learner: the trace's order).
        let records: Vec<_> = played.trace.turns.iter().flat_map(|t| &t.decisions).filter(|d| d.candidates > 0).collect();
        assert_eq!(records.len(), shard.decisions.len());
        assert_eq!(arrays.logp.len(), shard.decisions.len());
        assert_eq!(arrays.accepted.len(), shard.candidate_meta.len());
        assert_eq!(arrays.value.len(), shard.turns.len());
        for (i, (record, row)) in records.iter().zip(&shard.decisions).enumerate() {
            assert_eq!(row[1], i32::from(record.head));
            assert_eq!(row[4], i32::from(record.candidates));
            assert_eq!(Some(row[5] as u16), record.chosen, "decision {i}");
            assert_eq!(arrays.logp[i], record.logp);
            let first = row[3] as usize;
            let accepted: u32 = arrays.accepted[first..first + row[4] as usize].iter().map(|&a| u32::from(a)).sum();
            assert_eq!(accepted, u32::from(record.accepted));
            assert_eq!(arrays.accepted[first + row[5] as usize], 1, "the choice was accepted");
            let meta = shard.candidate_meta[first + row[5] as usize];
            assert_eq!([meta[0], meta[1], meta[3]], record.chosen_meta);
        }
        assert!(shard.decisions.iter().any(|d| d[1] == head::UNIT) && shard.decisions.iter().any(|d| d[1] == head::PRODUCTION));
        assert!(arrays.value.iter().all(|v| *v != i32::MIN), "the model's value per turn");
        hashes.push(played.meta.hash);
    }
    assert_ne!(hashes[0], hashes[1], "the members of a group sample other games");
}

/// The files of a game: written whole, read back by the exporter, a done
/// game is skipped; windows keep part of the turns, still aligned.
#[test]
fn the_files_of_a_game_round_trip() {
    let rules = ClassicBench::rules();
    let config = Config { rounds: 12, group: 1, ..config() };
    let job = &selfplay::jobs(&config, 1)[0];
    let dir = std::env::temp_dir().join(format!("aurelia-selfplay-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    assert!(!selfplay::done(&dir, job));
    let played = selfplay::play(&rules, &config, job).unwrap();
    let meta = selfplay::write(&dir, &played).unwrap();
    assert!(selfplay::done(&dir, job));
    let [log, _, sidecar, meta_path] = selfplay::paths(&dir, &job.stem());
    assert_eq!(selfplay::read_meta(&meta_path).unwrap(), meta);
    assert_eq!(std::fs::metadata(&sidecar).unwrap().len(), meta.sidecar_bytes);
    let (spec, journal) = aurelia_qa::export::read_log(&log).unwrap();
    let (trace, game) = e2::read_sidecar(&log).unwrap();
    assert_eq!(trace, played.trace);
    let (shard, arrays, counts) = e2::shard(&rules, &spec, &job.stem(), &journal, &trace, Selection::Windows { length: 4, per_player: 2 }).unwrap();
    assert!(counts.dropped.is_empty(), "{:?}", counts.dropped);
    assert_eq!(shard.turns.len(), 8);
    assert_eq!(shard.windows.len(), 2);
    let (out, _) = e2::write(&dir.join("shards"), &shard, &arrays, &counts, &game, &rules).unwrap();
    for file in ["meta.json", "decisions.npy", "e2_logp.npy", "e2_decisions.npy", "e2_accepted.npy", "e2_value.npy", "e2.json"] {
        assert!(out.join(file).exists(), "{file}");
    }
    let shard_meta: serde_json::Value = serde_json::from_slice(&std::fs::read(out.join("meta.json")).unwrap()).unwrap();
    assert_eq!(shard_meta["e2"]["learners"], serde_json::json!(meta.learners));
    assert_eq!(shard_meta["e2"]["seats"].as_array().map(Vec::len), Some(meta.seats.len()));
    assert_eq!(shard_meta["e2"]["temperature"], serde_json::json!(config.temperature));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// THE POTENTIAL ROUND BY ROUND (`.e2.json` `potential`, format 3): one
/// entry when the learner starts and one after every round, the last at
/// the game's end; per learner the raw holdings of each entry are its
/// world's at that round (the game's log replayed to it), and the total is
/// the holdings at one price sheet, taken at the first entry with a city.
/// A file without the record (format 2) still reads.
#[test]
fn the_potential_is_recorded_round_by_round() {
    let rules = ClassicBench::rules();
    let dir = std::env::temp_dir().join(format!("aurelia-selfplay-potential-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let config = Config { rounds: 16, group: 1, ..config() };
    let job = &selfplay::jobs(&config, 1)[0];
    let played = selfplay::play(&rules, &config, job).unwrap();
    let meta = &played.meta;
    assert_eq!(meta.format, 3);
    let track = meta.potential.as_ref().expect("the potential");
    let n = track.round.len();
    assert_eq!(track.round[0], played.trace.turns[0].round, "entry 0: the learner's first turn's round");
    assert_eq!(track.round[n - 1], meta.final_round);
    assert!(track.round.windows(2).all(|w| w[1] == w[0] + 1 || (w[1] == w[0] && w[1] == meta.final_round)), "{:?}", track.round);
    assert!(played.trace.turns.iter().all(|t| track.round[..n - 1].contains(&t.round)), "every learner turn has an entry and a next one");
    assert_eq!(track.seats.iter().map(|s| s.player).collect::<Vec<_>>(), meta.learners);
    let seat = &track.seats[0];
    let sheet = seat.prices.as_ref().expect("the learner founded a city");
    assert_eq!(sheet.variant, "standard", "a game from round 0: the scripted AI's prices");
    let first_city = seat.raw["cities"].iter().position(|c| *c > 0.0).unwrap();
    assert_eq!(sheet.round, track.round[first_city], "the sheet of the first entry with a city");
    assert_eq!(seat.raw.len(), grounds::YIELDS.len() + grounds::STOCKS.len());
    let holdings_at = |k: usize| grounds::Holdings { raw: seat.raw.iter().map(|(c, v)| (c.clone(), f64::from(v[k]))).collect(), buildings: 0 };
    for k in 0..n {
        assert_eq!(seat.raw.values().map(Vec::len).collect::<Vec<_>>(), vec![n; seat.raw.len()]);
        let total = ((grounds::potential(&holdings_at(k), sheet).total * 100.0).round() / 100.0) as f32;
        assert!((seat.total[k] - total).abs() <= 1e-3 * total.abs().max(1.0), "entry {k}: {} vs {total}", seat.total[k]);
    }
    assert!(seat.total[n - 1] > seat.total[first_city] && seat.total[first_city] > 0.0, "{:?}", seat.total);
    // The raw holdings are the world's at each entry: the game's log replayed to that round.
    selfplay::write(&dir, &played).unwrap();
    let [log, _, _, meta_path] = selfplay::paths(&dir, &job.stem());
    for k in [0, first_city, n / 2, n - 1] {
        let (world, _) = &*selfplay::expert_at(&rules, &log, track.round[k]).unwrap();
        let theirs = &world.seats[&seat.player].0;
        for (c, v) in &seat.raw {
            assert_eq!(v[k], theirs.raw[c] as f32, "entry {k} (round {}): {c}", track.round[k]);
        }
    }
    // A format-2 file, without the record, reads with none.
    let mut old: serde_json::Value = serde_json::from_slice(&std::fs::read(&meta_path).unwrap()).unwrap();
    old["format"] = serde_json::json!(2);
    old.as_object_mut().unwrap().remove("potential");
    let old: selfplay::GameMeta = serde_json::from_value(old).unwrap();
    assert!(old.potential.is_none());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// MID-GAME STARTS: a scripted game's log (the expert's) replayed to the
/// start of round R; the learner takes over the nation with the most
/// science and the game goes on. The branched log replays from round 0 to
/// the game's hash, the export aligns every decision and holds only the
/// turns from R on, the members of a group share the position at R and
/// sample other games; the outcome's science fields are sane; a bench mind
/// can sit in the learner's seat as a control.
#[test]
fn mid_game_starts_branch_from_a_log() {
    let rules = ClassicBench::rules();
    let dir = std::env::temp_dir().join(format!("aurelia-selfplay-mid-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let source = aurelia_qa::export::suite_specs(&Suite::named("smoke").unwrap(), 1, 0, Some(14)).remove(0);
    let log = aurelia_qa::export::play_log(&rules, &source).unwrap();
    aurelia_qa::export::write_log(&dir.join("logs"), &source, &log).unwrap();
    let from_round = 8;
    let from = FromLogs::read(&dir.join("logs"), &[], from_round, 0, LearnerSeat::BestScience).unwrap();
    let branched = Config { rounds: 20, group: 2, from: Some(from.clone()), ..config() };
    let jobs = selfplay::jobs(&branched, 2);
    assert_eq!(jobs[0].start, jobs[1].start, "a group shares its start");
    assert_eq!(jobs[0].start.as_ref().map(|s| s.round), Some(from_round));
    assert_eq!(jobs[0].spec.rounds, 20, "the continued game's time victory");
    // The learner: the living nation with the most science at R.
    let at_r = selfplay::start_position(&rules, &jobs[0]).unwrap();
    let auth = at_r.view().auth;
    let best = aurelia_qa::bench::modules::majors(&at_r)
        .into_iter()
        .filter(|p| auth.players().is_alive(*p))
        .max_by_key(|p| (classic_rules::derived::income::player_income(auth, &rules, *p).science, std::cmp::Reverse(p.0)))
        .unwrap();
    let mut played_games = Vec::new();
    for job in &jobs {
        let played = selfplay::play(&rules, &branched, job).unwrap();
        let meta = &played.meta;
        assert_eq!(meta.format, 3);
        assert_eq!(meta.from_round, Some(from_round));
        assert_eq!(meta.learners, vec![best.0], "best-science");
        assert_eq!(meta.learner_seat.as_deref(), Some("best-science"));
        assert!(meta.source.as_deref().is_some_and(|s| s.ends_with(".aurlog")));
        let prefix = meta.from_commands.expect("the replayed prefix");
        assert!(prefix > 0 && meta.commands > prefix);
        assert_eq!(meta.from_hash, Some(kernel::replay::hash::authoritative_hash(&at_r)), "the position at R");
        assert!(!played.trace.turns.is_empty());
        assert!(played.trace.turns.iter().all(|t| t.round >= from_round && u64::from(t.position) >= prefix), "the sidecar holds only the decisions after R");
        assert_space_sane(meta);
        // The whole log, prefix and continuation, replays from round 0 and exports aligned.
        let (shard, arrays, counts) = e2::shard(&rules, &job.spec, &job.stem(), &played.journal, &played.trace, Selection::Sample(1)).unwrap();
        assert_eq!(counts.hash, meta.hash, "{}: the branched log replays to the game's world", job.stem());
        assert!(counts.dropped.is_empty(), "{:?}", counts.dropped);
        assert_eq!(counts.aligned + counts.empty, counts.decisions);
        assert!(counts.aligned > 20, "{counts:?}");
        assert_eq!(arrays.logp.len(), shard.decisions.len());
        assert!(!shard.turns.is_empty() && shard.turns.iter().all(|t| t[0] >= from_round as i32), "the shard holds only turns from R on");
        played_games.push(played);
    }
    assert_eq!(played_games[0].meta.from_hash, played_games[1].meta.from_hash, "the members share the position at R");
    assert_eq!(played_games[0].meta.from_commands, played_games[1].meta.from_commands);
    assert_ne!(played_games[0].meta.hash, played_games[1].meta.hash, "the members sample other games");
    // The potential: from R on, at the source's AI variant's prices taken at R, the same for the members.
    let tracks: Vec<_> = played_games.iter().map(|p| p.meta.potential.clone().expect("the potential")).collect();
    for t in &tracks {
        assert_eq!(t.round[0], from_round);
        let sheet = t.seats[0].prices.as_ref().expect("the learner holds a city at R");
        assert_eq!((sheet.round, sheet.variant.as_str()), (from_round, source.variant.as_str()));
    }
    assert_eq!(tracks[0].seats[0].prices, tracks[1].seats[0].prices, "a group shares its sheet");
    assert_eq!(tracks[0].seats[0].total[0], tracks[1].seats[0].total[0], "and its potential at R");
    // The files: `.e2.json` and the shard's meta.json record the start.
    let games = dir.join("games");
    let meta = selfplay::write(&games, &played_games[0]).unwrap();
    let [log_path, _, _, meta_path] = selfplay::paths(&games, &jobs[0].stem());
    assert!(jobs[0].stem().ends_with(&format!("-r{from_round}")));
    assert_eq!(selfplay::read_meta(&meta_path).unwrap(), meta);
    let (spec, journal) = aurelia_qa::export::read_log(&log_path).unwrap();
    let (trace, game) = e2::read_sidecar(&log_path).unwrap();
    let (shard, arrays, counts) = e2::shard(&rules, &spec, &jobs[0].stem(), &journal, &trace, Selection::Sample(1)).unwrap();
    let (out, _) = e2::write(&dir.join("shards"), &shard, &arrays, &counts, &game, &rules).unwrap();
    let shard_meta: serde_json::Value = serde_json::from_slice(&std::fs::read(out.join("meta.json")).unwrap()).unwrap();
    assert_eq!(shard_meta["e2"]["from_round"], serde_json::json!(from_round));
    assert!(shard_meta["e2"]["seats"][0]["space"]["parts_needed"].as_u64().is_some_and(|n| n > 0));
    // Another seat by its id; a bench mind in the learner's seat records nothing.
    let other = aurelia_qa::bench::modules::majors(&at_r).into_iter().find(|p| *p != best && auth.players().is_alive(*p)).unwrap();
    let pinned = Config { rounds: 10, group: 1, from: Some(FromLogs { seat: LearnerSeat::Player(other.0), ..from.clone() }), learner_mind: Some("pre-war-fix".to_owned()), ..config() };
    let job = &selfplay::jobs(&pinned, 1)[0];
    let control = selfplay::play(&rules, &pinned, job).unwrap();
    assert_eq!(control.meta.learners, vec![other.0]);
    assert_eq!(control.meta.learner_mind.as_deref(), Some("pre-war-fix"));
    assert!(control.trace.turns.is_empty());
    let (_, _, counts) = e2::shard(&rules, &job.spec, &job.stem(), &control.journal, &control.trace, Selection::Sample(1)).unwrap();
    assert_eq!(counts.hash, control.meta.hash);
    // The space technologies given at R: recorded, made again by the export, which aligns.
    let gifted = Config { rounds: 12, group: 1, from: Some(FromLogs { give_space_techs: 1.0, ..from.clone() }), ..config() };
    let job = &selfplay::jobs(&gifted, 1)[0];
    assert!(job.start.as_ref().is_some_and(|s| s.give_space_techs));
    let played = selfplay::play(&rules, &gifted, job).unwrap();
    assert!(played.meta.gave_space_techs);
    let gift = played.meta.gift.clone().expect("the gift");
    assert_eq!(gift.position, played.meta.from_commands.unwrap());
    assert_eq!(gift.players.iter().map(|(p, _)| *p).collect::<Vec<_>>(), played.meta.learners);
    assert!(gift.players[0].1.len() > 20, "most of the tree is given at round 8");
    let learner = played.meta.seats.iter().find(|s| s.learner).unwrap();
    assert_eq!(learner.space.space_techs, learner.space.space_techs_needed, "{learner:?}");
    let game_json = serde_json::to_value(&played.meta).unwrap();
    assert_eq!(e2::gift_of(&game_json).unwrap().as_ref(), Some(&gift));
    let (_, _, counts) = e2::shard_with(&rules, &job.spec, &job.stem(), &played.journal, &played.trace, Selection::Sample(1), Some(&gift)).unwrap();
    assert_eq!(counts.hash, played.meta.hash, "the gifted log replays with its gift");
    assert!(counts.dropped.is_empty() && counts.aligned > 0, "{counts:?}");
    assert!(e2::shard(&rules, &job.spec, &job.stem(), &played.journal, &played.trace, Selection::Sample(1)).is_err(), "without the gift the replay diverges");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The jitter of the start: drawn per group within R ± J, the same for a
/// group's members, never past the source's last round.
#[test]
fn the_start_round_jitters_per_group() {
    let spec = aurelia_qa::export::suite_specs(&Suite::named("smoke").unwrap(), 1, 0, Some(300)).remove(0);
    let from = FromLogs { logs: vec![("a.aurlog".into(), spec)], round: 296, jitter: 8, seat: LearnerSeat::Random, give_space_techs: 0.25 };
    let config = Config { rounds: 500, group: 3, from: Some(from), ..config() };
    let jobs = selfplay::jobs(&config, 1200);
    let rounds: Vec<u32> = jobs.iter().map(|j| j.start.as_ref().unwrap().round).collect();
    assert!(rounds.iter().all(|r| (288..=300).contains(r)), "{rounds:?}");
    assert!(jobs.chunks(3).all(|g| g.iter().all(|j| j.start == g[0].start)), "a group shares its round and its gift");
    assert!(rounds.iter().any(|r| *r < 296) && rounds.iter().any(|r| *r > 296));
    let gifted = jobs.chunks(3).filter(|g| g[0].start.as_ref().unwrap().give_space_techs).count();
    assert!((70..=130).contains(&gifted), "about a quarter of 400 groups: {gifted}");
}

/// THE CURRICULUM'S POINT: the spaceship's technologies given to the
/// learner at round 300 make the science victory reachable by round 500.
/// Eight scripted games of 300 rounds (Tiny, the standard suite) are the
/// sources; `pre-war-fix` takes the best scientist with every technology of
/// the closure, three `standard` play the others.
#[test]
fn given_space_techs_reach_the_science_victory() {
    let rules = ClassicBench::rules();
    let dir = std::env::temp_dir().join(format!("aurelia-selfplay-space-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let sources: Vec<_> = aurelia_qa::export::suite_specs(&Suite::named("standard").unwrap(), 80, 0, Some(300)).into_iter().filter(|s| s.size == "tiny").take(8).collect();
    assert_eq!(sources.len(), 8);
    let jobs = std::thread::available_parallelism().map_or(1, |n| n.get()).min(8);
    for result in aurelia_qa::export::parallel(&sources, jobs, |spec| aurelia_qa::export::play_log(&rules, spec).and_then(|log| aurelia_qa::export::write_log(&dir.join("logs"), spec, &log).map_err(|e| e.to_string()))) {
        result.unwrap();
    }
    let from = FromLogs { give_space_techs: 1.0, ..FromLogs::read(&dir.join("logs"), &[], 300, 0, LearnerSeat::BestScience).unwrap() };
    let config = Config { rounds: 500, group: 1, from: Some(from), learner_mind: Some("pre-war-fix".to_owned()), opponents: vec!["standard".to_owned()], ..config() };
    let games = selfplay::jobs(&config, 8);
    let played: Vec<_> = aurelia_qa::export::parallel(&games, jobs, |job| selfplay::play(&rules, &config, job)).into_iter().map(Result::unwrap).collect();
    let mut won = Vec::new();
    for game in &played {
        assert!(game.meta.gave_space_techs);
        assert_space_sane(&game.meta);
        let learner = game.meta.seats.iter().find(|s| s.learner).unwrap();
        assert_eq!(learner.space.space_techs, learner.space.space_techs_needed);
        if learner.victory.as_deref() == Some("science") {
            won.push(learner.victory_round.unwrap());
        }
    }
    let ends: Vec<_> = played.iter().map(|g| (g.meta.victory.clone(), g.meta.learners.clone())).collect();
    eprintln!("science victories of the learner: {} of 8, rounds {won:?}; ends {ends:?}", won.len());
    assert!(won.len() >= 3, "the learner won by science in {} of 8 games: {ends:?}", won.len());
    // A gifted log replays with its gift to the game's world.
    let (game, job) = (&played[0], &games[0]);
    let (_, _, counts) = e2::shard_with(&rules, &job.spec, &job.stem(), &game.journal, &game.trace, Selection::Sample(1), game.meta.gift.as_ref()).unwrap();
    assert_eq!(counts.hash, game.meta.hash);
    std::fs::remove_dir_all(&dir).unwrap();
}
