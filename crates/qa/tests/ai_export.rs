//! The data of the end-to-end net (`aurelia_qa::export`,
//! classic/docs/ai-net-data.md): a logged game is the plain game, its log
//! replays with every round's hash, and the shard made from it is
//! consistent: offsets inside their arrays, choices inside their
//! candidates, the layout as pinned here.

use aurelia_qa::bench::modules::{BenchRules, ClassicBench};
use aurelia_qa::bench::suite::Suite;
use aurelia_qa::export::{choose_windows, features, features_of, features_with, play_log, start, suite_specs, truth_fields, write_shard, LogSpec, Scope, Selection, Shard, AUX};
use classic_rules::ai::net_e2e::encode::{self, head, kind, tag, C, E, G, T, V};
use kernel::ids::PlayerId;
use classic_rules::ai::{AiDriver, AiSettings};
use kernel::replay::hash::authoritative_hash;

const ROUNDS: u32 = 30;

fn spec() -> LogSpec {
    let suite = Suite::named("smoke").expect("the smoke suite");
    suite_specs(&suite, 1, 0, Some(ROUNDS)).remove(0)
}

/// The plain mind's game, as the bench plays it: the net's base (`NET_BASE`, `pre-puppet`).
fn plain_hash(rules: &classic_rules::CompiledRuleset, spec: &LogSpec) -> u64 {
    let game_spec = spec.game().unwrap();
    let mut game = aurelia_qa::record::new_game(rules, &game_spec);
    let mind = classic_rules::ai::driver::ClassicMind::new(classic_rules::ai::variant::NET_BASE);
    let mut driver = AiDriver::with_mind(AiSettings::for_rules(rules, &game_spec.difficulty), Vec::new(), mind);
    while !game.is_over() && game.view().auth.turn().round <= ROUNDS {
        assert!(driver.run(&mut game, rules, u32::MAX).round_done || game.is_over());
    }
    authoritative_hash(&game)
}

fn exported() -> (classic_rules::CompiledRuleset, LogSpec, Shard) {
    let rules = ClassicBench::rules();
    let spec = spec();
    let log = play_log(&rules, &spec).expect("the game plays");
    let shard = features(&rules, &spec, &log.journal, Selection::Sample(1)).expect("the log replays");
    (rules, spec, shard)
}

#[test]
fn a_logged_game_is_the_plain_game_and_its_log_replays_round_by_round() {
    let rules = ClassicBench::rules();
    let spec = spec();
    let log = play_log(&rules, &spec).expect("the game plays");
    assert_eq!(log.hash, plain_hash(&rules, &spec), "the logged game is the plain mind's game");
    assert!(log.journal.len() < 100_000, "a log is tens of kilobytes: {}", log.journal.len());
    let shard = features(&rules, &spec, &log.journal, Selection::Sample(1)).expect("the log replays");
    assert_eq!(shard.counts.checkpoints, u64::from(ROUNDS), "every round's hash is checked");
    // The start the log names is the spec's world.
    let game = start(&rules, &spec.game().unwrap());
    let (header, _) = kernel::replay::journal::JournalHeader::read(&log.journal).unwrap();
    assert_eq!(header.world, authoritative_hash(&game));
}

#[test]
fn the_layout_is_pinned() {
    let rules = ClassicBench::rules();
    let layout = encode::layout(&rules);
    assert_eq!((layout.tile.len(), layout.entity.len(), layout.candidate.len()), (47, 95, 163), "C_tile, C_entity, C_candidate");
    assert_eq!(G::COUNT, 64);
    assert_eq!(layout.global.len(), G::COUNT + rules.technologies.len() + rules.module.policies.len() + rules.module.policy_branches.len());
    assert_eq!((T::COUNT, E::COUNT, C::COUNT), (layout.tile.len(), layout.entity.len(), layout.candidate.len()));
    assert_eq!(layout.tile.first().map(|f| f.name.as_str()), Some("known"));
    assert_eq!(layout.tile.last().map(|f| f.name.as_str()), Some("own_city_distance"));
    assert_eq!(layout.entity.last().map(|f| f.name.as_str()), Some("holy_city_other_religion"));
    // Format 8: the blow's outcome after the format-7 columns.
    let tail: Vec<&str> = layout.candidate[143..149].iter().map(|f| f.name.as_str()).collect();
    assert_eq!(tail, ["item_queued_other_cities", "target_claimed_by_own", "blow_kills", "blow_captures", "preview_damage_to_defender_uncapped", "preview_city_floor"]);
    // Format 10: the strike from a move's destination after format 9's.
    assert_eq!(encode::FORMAT_9_CANDIDATE_COLUMNS, 149);
    let strike: Vec<&str> = layout.candidate[149..].iter().map(|f| f.name.as_str()).collect();
    assert_eq!(strike, ["move_blow_captures", "move_blow_kills", "move_blow_damage_percent", "move_city_damage_percent", "move_city_adjacent", "move_city_in_strike", "move_strike_targets",
        "move_enemy_distance", "move_wounded_enemy_distance", "move_wounded_enemy_hp_percent", "move_next_turn_targets", "move_next_turn_kills",
        "move_city_distance", "move_unseen_city_in_reach"]);
    assert_eq!(layout.global.last().map(|f| f.name.clone()), Some(format!("branch_{}", rules.module.policy_branches.len() - 1)));
    let moves = layout.entity.iter().find(|f| f.name == "moves_left").unwrap();
    assert_eq!(moves.scale.value(), 10.0, "moves in tenths");
    let treasury = layout.global.iter().find(|f| f.name == "treasury").unwrap();
    assert_eq!(treasury.scale.value(), 0.1, "the treasury in tens");
    let pressure = layout.entity.iter().find(|f| f.name == "own_religion_pressure").unwrap();
    assert_eq!(pressure.scale.value(), 10.0, "pressure in tenths of a citizen");
    let speed = layout.global.iter().find(|f| f.name == "speed_production_cost_percent").unwrap();
    assert_eq!(speed.scale.value(), 100.0, "a percent of Standard");
    // Format 7: the event table.
    assert_eq!((layout.event.len(), V::COUNT), (19, 19), "C_event");
    assert_eq!(layout.event.first().map(|f| f.name.as_str()), Some("event_kind_id_plus_1"));
    assert_eq!(layout.event.last().map(|f| f.name.as_str()), Some("ours"));
    assert_eq!(classic_rules::ai::net_e2e::events::kind::NAMES.len(), 20);
}

#[test]
fn a_shard_is_consistent_and_its_choices_are_in_range() {
    let (rules, _, shard) = exported();
    check_consistent(&rules, &shard);
    assert_eq!(shard.counts.dropped.get("citizens_not_listed"), None, "every city's settings are among its candidates");
    for name in ["unit", "research", "production", "policy", "citizens"] {
        assert!(shard.counts.decisions.get(name).copied().unwrap_or(0) > 0, "decisions of the {name} head");
    }
}

/// Religion: the pantheon, the founding and enhancing of a religion split
/// into one decision per belief slot, faith purchases and saves; the picks
/// of every founding assemble to the expert's command again.
#[test]
fn religious_commands_split_into_slots_and_assemble_again() {
    let rules = ClassicBench::rules();
    let suite = Suite::named("standard").expect("the standard suite");
    let spec = suite_specs(&suite, 1, 0, Some(150)).remove(0);
    let log = play_log(&rules, &spec).expect("the game plays");
    let shard = features(&rules, &spec, &log.journal, Selection::Sample(1)).expect("the log replays");
    check_consistent(&rules, &shard);
    let c = &shard.counts;
    println!("religion: {:?} decisions, {} foundings assembled, {} faith saves, dropped {:?}", c.decisions.get("religion"), c.religion_assembled, c.faith_saves, c.dropped);
    assert!(c.religion_assembled > shard.standings.len() as u64, "every pantheon and at least one religion founded");
    assert_eq!(c.religion_assembly_mismatch, 0, "the picks assemble to the expert's command");
    assert_eq!(c.religion_assembled_refused, 0, "the assembled command is accepted by the rules");
    assert_eq!(c.religion_other_identity, 0, "the religion is the first free one");
    assert!(c.dropped.keys().all(|k| !k.contains("relig") && !k.contains("faith")), "{:?}", c.dropped);
    let chosen: std::collections::BTreeSet<i32> = shard
        .decisions
        .iter()
        .filter(|d| d[1] == head::RELIGION)
        .map(|d| shard.candidate_meta[(d[3] + d[5]) as usize][0])
        .collect();
    for slot in [tag::PANTHEON, tag::FOUND_FOUNDER, tag::FOUND_FOLLOWER] {
        assert!(chosen.contains(&slot), "a decision of slot {slot}: {chosen:?}");
    }
    // Format 6: the cities' religion as the founder sees it.
    let rows: Vec<&[i16]> = shard.entities.chunks(E::COUNT).collect();
    let cities = |k: i32| shard.entity_meta.iter().zip(&rows).filter(move |(m, _)| m[0] == k).map(|(_, r)| *r);
    assert!(cities(kind::OWN_CITY).any(|r| r[E::HolyOwn as usize] == 1 && r[E::OwnFollowers as usize] > 0 && r[E::OwnPressure as usize] > 0), "a founder's holy city");
    assert!(cities(kind::OWN_CITY).any(|r| r[E::MajorityOwn as usize] == 1), "an own city following the own religion");
    for r in cities(kind::OWN_CITY).chain(cities(kind::FOREIGN_CITY)) {
        let flags = r[E::MajorityNone as usize] + r[E::MajorityOwn as usize] + r[E::MajorityOther as usize];
        assert!(flags <= 1 && r[E::MajorityFollowers as usize] <= r[E::Population as usize].max(0), "{r:?}");
        assert!(r[E::HolyOwn as usize] + r[E::HolyOther as usize] <= 1);
    }
    assert!(cities(kind::OWN_CITY).all(|r| r[E::MajorityNone as usize] + r[E::MajorityOwn as usize] + r[E::MajorityOther as usize] == 1), "an own city's religion is always seen");
    // The demand signals.
    let g = |t: usize, c: G| shard.globals[t * encode::global_width(&rules) + c as usize];
    let turns = shard.turns.len();
    assert!((0..turns).any(|t| g(t, G::WorkerJobs) > 0) && (0..turns).any(|t| g(t, G::CitySites) > 0) && (0..turns).any(|t| g(t, G::SettlersQueued) > 0));
    assert!((0..turns).all(|t| g(t, G::FreeWorkerJobs) == g(t, G::WorkerJobs) - 4 * (g(t, G::WorkersAlive) + g(t, G::WorkersQueued))));
    assert!((0..turns).all(|t| g(t, G::SpeedProduction) == 100 && g(t, G::MaxRounds) > 0));
    let queued_elsewhere = shard.candidates.chunks(C::COUNT).filter(|r| r[C::ItemQueuedElsewhere as usize] > 0).count();
    assert!(queued_elsewhere > 0, "an item queued in another own city");
    let unit_head = |m: &[i32; 4]| m[0] < tag::RESEARCH;
    assert!(shard.candidates.chunks(C::COUNT).zip(&shard.candidate_meta).all(|(r, m)| unit_head(m) || r[C::TargetClaimed as usize] == 0), "claims only on the unit head");
}

/// Offsets inside their arrays, choices inside their candidates.
fn check_consistent(rules: &classic_rules::CompiledRuleset, shard: &Shard) {
    let tiles = shard.tile_count();
    let turns = shard.turns.len();
    assert!(turns > 0);
    assert_eq!(tiles, (shard.width * shard.height) as usize);
    assert_eq!(shard.region.len(), tiles);
    assert!(shard.region.iter().all(|r| (*r as usize) < shard.regions));
    assert!(shard.neighbors.iter().flatten().all(|n| *n >= -1 && (*n as i64) < tiles as i64));
    assert_eq!(shard.tiles.len(), turns * tiles * T::COUNT);
    assert_eq!(shard.globals.len(), turns * encode::global_width(rules));
    assert_eq!(shard.entities.len(), shard.entity_meta.len() * E::COUNT);
    assert_eq!(shard.candidates.len(), shard.candidate_meta.len() * C::COUNT);
    // Turns: entity blocks one after another.
    let mut next = 0;
    for t in &shard.turns {
        assert_eq!(t[2], next, "the entity rows of a turn follow the last turn's");
        next += t[3];
    }
    assert_eq!(next as usize, shard.entity_meta.len());
    // Decisions: their turn, actor and candidates inside the arrays, the
    // choice inside the candidates, the candidates one block after another.
    let mut first = 0;
    for d in &shard.decisions {
        let [turn, head_code, actor, start, count, choice] = *d;
        let t = shard.turns[turn as usize];
        assert!((0..6).contains(&head_code));
        if head_code == head::UNIT || head_code == head::PRODUCTION || head_code == head::CITIZENS {
            assert!(actor >= t[2] && actor < t[2] + t[3], "the actor is a row of its turn");
        } else if head_code == head::RELIGION {
            assert!(actor == -1 || (actor >= t[2] && actor < t[2] + t[3]), "a prophet is a row of its turn");
        } else {
            assert_eq!(actor, -1);
        }
        assert_eq!(start, first);
        assert!(count > 0 && choice >= 0 && choice < count, "the choice is a candidate");
        first += count;
        for m in &shard.candidate_meta[start as usize..(start + count) as usize] {
            assert!(m[1] >= -1 && (m[1] as i64) < tiles as i64);
            assert!(m[2] == -1 || (m[2] >= t[2] && m[2] < t[2] + t[3]), "a target entity is a row of the turn");
            let unit_tag = (0..=81).contains(&m[0]) || m[0] == tag::WAIT;
            match head_code {
                head::UNIT => assert!(unit_tag, "tag {}", m[0]),
                head::RESEARCH => assert_eq!(m[0], tag::RESEARCH),
                head::PRODUCTION => assert!((tag::PRODUCE_UNIT..=tag::PRODUCE_PROJECT).contains(&m[0])),
                head::POLICY => assert!((tag::ADOPT_POLICY..=tag::CHOOSE_IDEOLOGY).contains(&m[0])),
                head::RELIGION => assert!((tag::PANTHEON..=tag::FAITH_SAVE).contains(&m[0])),
                _ => assert!((tag::FOCUS..=tag::SPECIALISTS).contains(&m[0])),
            }
        }
        if head_code == head::CITIZENS {
            let block = &shard.candidate_meta[start as usize..(start + count) as usize];
            assert!(block.iter().all(|m| m[0] == block[0][0] && m[2] == actor), "one setting of one city");
            let rows = &shard.candidates[start as usize * C::COUNT..(start + count) as usize * C::COUNT];
            assert_eq!(rows.chunks(C::COUNT).filter(|r| r[C::ActWait as usize] == 1).count(), 1, "the current setting is one candidate");
        }
        if head_code == head::UNIT {
            let tags: Vec<i32> = shard.candidate_meta[start as usize..(start + count) as usize].iter().map(|m| m[0]).collect();
            assert_eq!(tags.iter().filter(|t| **t == tag::WAIT).count(), 1, "waiting is one candidate of every unit decision");
        }
    }
    assert_eq!(first as usize, shard.candidate_meta.len());
    // Format 7: the events of each turn, one block after another; an actor
    // is a nation or city-state row of its turn.
    assert_eq!(shard.turn_events.len(), turns);
    assert_eq!(shard.events.len(), shard.event_meta.len() * V::COUNT);
    let mut next_event = 0;
    for (t, &[v0, nv]) in shard.turn_events.iter().enumerate() {
        assert_eq!(v0, next_event, "the event rows of a turn follow the last turn's");
        next_event += nv;
        let [round, _, base, count, start] = shard.turns[t];
        // Format 8: the rows of the turn's start, then the rows the turn
        // appended (units and cities it named later, never a nation).
        assert!((0..=count).contains(&start));
        let appended = &shard.entity_meta[(base + start) as usize..(base + count) as usize];
        assert!(appended.iter().all(|m| m[0] < kind::CITY_STATE), "an appended row is a unit or a city");
        for (m, row) in shard.event_meta[v0 as usize..(v0 + nv) as usize].iter().zip(shard.events[v0 as usize * V::COUNT..].chunks(V::COUNT)) {
            assert!((0..20).contains(&m[0]) && i32::from(row[V::KindId as usize]) == m[0] + 1);
            assert!(m[1] >= -1 && (m[1] as i64) < tiles as i64);
            assert!(m[2] == -1 || (m[2] >= base && m[2] < base + start && [kind::PLAYER, kind::CITY_STATE].contains(&shard.entity_meta[m[2] as usize][0])), "an actor is a nation or city-state row");
            assert!((0..10).contains(&row[V::Age as usize]) && (row[V::Age as usize] as i32) <= round, "events of the last 10 rounds");
        }
        assert!(nv <= 64);
    }
    assert_eq!(next_event as usize, shard.event_meta.len());
    // Training-only arrays: one row a turn.
    let nations = shard.standings.len() as i16;
    assert_eq!(shard.aux.len(), turns * AUX.len());
    assert_eq!(shard.truth.len(), turns * truth_fields().len());
    for (t, row) in shard.aux.chunks(AUX.len()).enumerate() {
        let rank = row[8];
        assert!((1..=nations).contains(&rank), "turn {t}: rank {rank}");
        assert!((0..=5).contains(&row[9]) && (0..=1000).contains(&row[7]));
        assert!(row[1] >= 0 && row[5] <= 1 && row[6] <= 1);
    }
    for (t, row) in shard.truth.chunks(truth_fields().len()).enumerate() {
        assert!(row[1] > 0, "turn {t}: the player itself knows technologies");
        assert!(row[7 * shard.standings.len()..].iter().all(|v| *v == 0), "slots past the nations are zeros");
    }
    // Research lookahead: three steps see at least what two do.
    let la2 = C::La2Buildings as usize;
    let la3 = C::La3Buildings as usize;
    for (meta, row) in shard.candidate_meta.iter().zip(shard.candidates.chunks(C::COUNT)) {
        if meta[0] == tag::RESEARCH {
            for i in 0..5 {
                assert!(row[la3 + i] >= row[la2 + i], "lookahead counts grow with depth");
            }
            assert!(row[C::TechsToProject as usize] >= -1 && row[C::TechsToLastEra as usize] >= 0);
        }
    }
}

#[test]
fn a_small_game_drops_few_decisions_and_reports_them() {
    let (rules, _, shard) = exported();
    let kept: u64 = shard.counts.decisions.values().sum();
    let dropped: u64 = shard.counts.dropped.values().sum();
    let share = dropped as f64 / (kept + dropped).max(1) as f64;
    println!(
        "{kept} decisions kept ({:?}), {dropped} dropped ({:.1} %: {:?}); marches: {} exact, {} reachable, {} first step, {} near; {} waits",
        shard.counts.decisions,
        share * 100.0,
        shard.counts.dropped,
        shard.counts.march_exact,
        shard.counts.march_reachable,
        shard.counts.march_first_step,
        shard.counts.march_near,
        shard.counts.waits
    );
    assert!(share < 0.05, "at most 5 % of the decisions are dropped: {share}");
    // Written: the arrays as NumPy reads them.
    let dir = std::env::temp_dir().join(format!("aurelia-ai-export-{}", std::process::id()));
    let (out, bytes) = write_shard(&dir, &shard, &rules).expect("the shard is written");
    assert!(bytes > 0);
    let decisions = std::fs::read(out.join("decisions.npy")).unwrap();
    assert_eq!(&decisions[..8], b"\x93NUMPY\x01\x00");
    let header_len = u16::from_le_bytes([decisions[8], decisions[9]]) as usize;
    assert_eq!((10 + header_len) % 64, 0, "the header pads the data to 64 bytes");
    let header = std::str::from_utf8(&decisions[10..10 + header_len]).unwrap();
    assert!(header.contains(&format!("'shape': ({}, 6)", shard.decisions.len())), "{header}");
    assert_eq!(decisions.len() - 10 - header_len, shard.decisions.len() * 6 * 4);
    let meta: serde_json::Value = serde_json::from_slice(&std::fs::read(out.join("meta.json")).unwrap()).unwrap();
    assert_eq!(meta["format"], encode::FORMAT);
    assert_eq!(meta["layout"]["event"].as_array().map(|e| e.len()), Some(V::COUNT));
    for file in ["events.npy", "event_meta.npy", "turn_events.npy"] {
        assert!(out.join(file).exists(), "{file}");
    }
    let windows = std::fs::read(out.join("windows.npy")).unwrap();
    let header_len = u16::from_le_bytes([windows[8], windows[9]]) as usize;
    assert_eq!(windows.len() - 10 - header_len, shard.windows.len() * 3 * 4);
    assert_eq!(meta["layout"]["tile"].as_array().unwrap().len(), T::COUNT);
    std::fs::remove_dir_all(dir).unwrap();
}

/// The windows of a shard cover its turns once, each the consecutive turns
/// of one player in round order.
fn check_windows(shard: &Shard) {
    let mut next = 0;
    for w in &shard.windows {
        let [player, first, len] = *w;
        assert_eq!(first, next, "windows follow one another");
        assert!(len > 0);
        for t in first..first + len {
            let turn = shard.turns[t as usize];
            assert_eq!(turn[1], player, "a window is one player's");
            if t > first {
                assert_eq!(turn[0], shard.turns[t as usize - 1][0] + 1, "a window's turns are consecutive rounds");
            }
        }
        next += len;
    }
    assert_eq!(next as usize, shard.turns.len());
}

#[test]
fn entities_keep_their_ids_and_windows_are_consecutive_turns() {
    let (_, spec, shard) = exported();
    check_windows(&shard);
    let nations = shard.standings.len();
    assert_eq!(shard.windows.len(), nations, "all turns: one window a nation");
    // Within a window: the nations and city-states met stay; own units keep their ids.
    let ids = |t: usize, k: i32| -> std::collections::BTreeSet<i32> {
        let [_, _, base, count, _] = shard.turns[t];
        shard.entity_meta[base as usize..(base + count) as usize].iter().filter(|m| m[0] == k).map(|m| m[3]).collect()
    };
    let mut kept_units = 0;
    for w in &shard.windows {
        for t in (w[1] + 1) as usize..(w[1] + w[2]) as usize {
            assert!(ids(t - 1, kind::PLAYER).is_subset(&ids(t, kind::PLAYER)), "a nation met stays in every turn");
            assert!(ids(t - 1, kind::CITY_STATE).is_subset(&ids(t, kind::CITY_STATE)), "a city-state met stays in every turn");
            kept_units += ids(t - 1, kind::OWN_UNIT).intersection(&ids(t, kind::OWN_UNIT)).count();
        }
    }
    assert!(kept_units > 0, "own units carry their persistent id from turn to turn");
    assert!(shard.entity_meta.iter().filter(|m| m[0] != kind::FOREIGN_UNIT).all(|m| m[3] >= 0), "every entity but a foreign unit has an id");
    assert!(shard.entity_meta.iter().filter(|m| m[0] == kind::FOREIGN_UNIT).all(|m| m[3] == -1));

    // Windows of K turns: M a nation, inside the game, not overlapping, deterministic.
    let rules = ClassicBench::rules();
    let log = play_log(&rules, &spec).unwrap();
    let windowed = features(&rules, &spec, &log.journal, Selection::Windows { length: 8, per_player: 2 }).unwrap();
    check_windows(&windowed);
    assert_eq!(windowed.windows.len(), 2 * nations);
    assert!(windowed.windows.iter().all(|w| w[2] == 8));
    assert_eq!(windowed.turns.len(), 16 * nations);
    let again = features(&rules, &spec, &log.journal, Selection::Windows { length: 8, per_player: 2 }).unwrap();
    assert_eq!(again.turns, windowed.turns, "the windows are drawn from the seed");
}

#[test]
fn a_seat_filter_keeps_exactly_those_nations_turns() {
    let rules = ClassicBench::rules();
    let spec = spec();
    let log = play_log(&rules, &spec).unwrap();
    let all = features(&rules, &spec, &log.journal, Selection::Sample(1)).unwrap();
    let only = Scope { players: Some([(0, 0)].into()), ..Scope::default() };
    for selection in [Selection::Sample(1), Selection::Windows { length: 8, per_player: 2 }] {
        let kept = features_of(&rules, &spec, &log.journal, selection, false, &only).unwrap();
        check_windows(&kept);
        assert!(!kept.turns.is_empty() && kept.turns.iter().all(|t| t[1] == 0), "only the chosen nation's turns");
        if matches!(selection, Selection::Sample(1)) {
            let mine: Vec<_> = all.turns.iter().filter(|t| t[1] == 0).map(|t| t[0]).collect();
            assert_eq!(kept.turns.iter().map(|t| t[0]).collect::<Vec<_>>(), mine, "every turn of it, as unfiltered");
        }
    }
    // From a round on (a learner's seat taken mid-game, `export::scope_of_game`): its turns from that round.
    let rounds: Vec<i32> = all.turns.iter().filter(|t| t[1] == 0).map(|t| t[0]).collect();
    let from = rounds[rounds.len() / 2];
    let late = Scope { players: Some([(0, from as u32)].into()), ..Scope::default() };
    let kept = features_of(&rules, &spec, &log.journal, Selection::Sample(1), false, &late).unwrap();
    assert_eq!(kept.turns.iter().map(|t| t[0]).collect::<Vec<_>>(), rounds.iter().copied().filter(|r| *r >= from).collect::<Vec<_>>());
}

#[test]
fn windows_never_overlap_and_fit_short_games() {
    for turns in [0, 3, 32, 33, 100, 300] {
        for player in 0..4 {
            let w = choose_windows(7, PlayerId(player), turns, 32, 4);
            let mut end = 0;
            for (first, len) in &w {
                assert!(*first >= end && first + len <= turns.max(1), "{turns}: {w:?}");
                end = first + len;
            }
            match turns {
                0 => assert!(w.is_empty()),
                t if t <= 32 => assert_eq!(w, vec![(0, t)]),
                t => assert_eq!(w.len(), (t / 32).min(4)),
            }
        }
    }
}

/// Hindsight marches: a founder's moves towards the site it founds on are
/// labelled with the march to that site; rows never change, only choices.
#[test]
fn hindsight_labels_a_founders_moves_with_the_site_it_founds_on() {
    let rules = ClassicBench::rules();
    // Long enough for settlers trained in cities to walk to their sites.
    let suite = Suite::named("smoke").expect("the smoke suite");
    let spec = suite_specs(&suite, 1, 0, Some(100)).remove(0);
    let log = play_log(&rules, &spec).expect("the game plays");
    let plain = features(&rules, &spec, &log.journal, Selection::Sample(1)).expect("the log replays");
    let hind = features_with(&rules, &spec, &log.journal, Selection::Sample(1), true).expect("the log replays");
    check_consistent(&rules, &hind);
    assert!(plain.counts.hindsight.is_none(), "off by default");
    assert_eq!((&plain.turns, &plain.tiles, &plain.entities, &plain.entity_meta), (&hind.turns, &hind.tiles, &hind.entities, &hind.entity_meta), "the rows stay");
    let h = hind.counts.hindsight.clone().expect("counted");
    println!("hindsight: {h:?}");
    assert!(h.hindsight_relabelled > 0);
    let dims = classic_rules::math::MapDimensions { width: hind.width, height: hind.height };
    // Per shard: (turn, actor, k-th decision of the actor in the turn) -> chosen candidate's meta.
    let chosen = |s: &Shard| -> std::collections::BTreeMap<(i32, i32, usize), [i32; 4]> {
        let mut seen = std::collections::BTreeMap::new();
        let mut out = std::collections::BTreeMap::new();
        for d in s.decisions.iter().filter(|d| d[1] == head::UNIT) {
            let k = seen.entry((d[0], d[2])).or_insert(0usize);
            out.insert((d[0], d[2], *k), s.candidate_meta[(d[3] + d[5]) as usize]);
            *k += 1;
        }
        out
    };
    let (before, after) = (chosen(&plain), chosen(&hind));
    let founder = |actor: i32| hind.entities[actor as usize * E::COUNT + E::Founder as usize] == 1;
    let id = |actor: i32| hind.entity_meta[actor as usize][3];
    // Where each founder founded its city (FoundCity, codec tag 1: on its own tile).
    let sites: std::collections::BTreeMap<i32, u32> =
        after.iter().filter(|((_, actor, _), m)| founder(*actor) && m[0] == 1).map(|((_, actor, _), _)| (id(*actor), hind.entity_meta[*actor as usize][1] as u32)).collect();
    assert!(!sites.is_empty(), "founders found cities");
    let mut to_site = 0;
    for (key, m) in &after {
        let (_, actor, _) = *key;
        if !founder(actor) || m[0] != tag::MARCH || before.get(key).map(|b| b[0]) != Some(0) {
            continue;
        }
        if let Some(site) = sites.get(&id(actor)) {
            let d = dims.distance(kernel::ids::TileId(m[1] as u32), kernel::ids::TileId(*site)).unwrap();
            to_site += usize::from(d <= encode::MARCH_NEAR);
        }
    }
    let founder_moves = before.iter().filter(|((_, a, _), m)| founder(*a) && m[0] == 0).count();
    println!("founder moves {founder_moves}, relabelled to the founded site {to_site}");
    assert!(to_site > 0, "a founder's move is relabelled as the march to the site it founds on");
}
