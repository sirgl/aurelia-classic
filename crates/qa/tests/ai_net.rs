//! The net player on the bench (classic/docs/ai-neural-architecture.md,
//! Playing): `net:<weights>` seats the net with that weights file (here
//! `net:dummy`, the deterministic stand-in scorer), and a match with net
//! seats is deterministic.

use aurelia_qa::bench::modules::{BenchRules, ClassicBench};
use aurelia_qa::bench::play::{play_match, Pairing};
use aurelia_qa::bench::suite::{Mode, Suite};
use classic_rules::ai::variant::Variant;

/// The same match with net seats twice gives the same world: the result
/// hash is equal, and the nets played (their seats differ from a game of
/// `standard` alone).
#[test]
fn a_match_with_net_seats_is_deterministic() {
    let suite = Suite::named("smoke").unwrap();
    let rules = ClassicBench::rules();
    let game_match = &suite.matches(Mode::Versus, 0)[0];
    let pairing = |candidate: &str| Pairing { baseline: "standard".to_owned(), candidate: candidate.to_owned() };
    let first = play_match::<ClassicBench>(&rules, &suite, game_match, &pairing("net:dummy")).unwrap();
    let second = play_match::<ClassicBench>(&rules, &suite, game_match, &pairing("net:dummy")).unwrap();
    assert_eq!(first.hash, second.hash, "{}", game_match.label());
    let standard = play_match::<ClassicBench>(&rules, &suite, game_match, &pairing("standard")).unwrap();
    assert_ne!(first.hash, standard.hash, "the net seats played their own game");
}

/// `net` is a variant of the bench; `net:<path>` names its weights.
#[test]
fn the_net_is_seated_by_name() {
    assert_eq!(Variant::from_name("net"), Some(Variant::Net));
    assert!(ClassicBench::variants().contains(&"net"));
    let mind = ClassicBench::mind("net:dummy").expect("a net seat");
    assert_eq!(mind.variant(), Variant::Net);
    assert_eq!(mind.net().map(|n| n.label()), Some("dummy".to_owned()));
}

/// A weights file that cannot be read is a loud error.
#[test]
#[should_panic(expected = "weights file")]
fn a_missing_weights_file_is_an_error() {
    ClassicBench::mind("net:/nonexistent/weights.bin");
}

mod same_rows {
    //! The exporter and the playing mind read the same rows of the same
    //! world: a short smoke game is logged, the exporter makes its shard,
    //! and the log is replayed through the net player (`NetPlayer`, a
    //! scorer that records what it is given) at the same moments.

    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};

    use aurelia_qa::bench::modules::{BenchRules, ClassicBench};
    use aurelia_qa::bench::suite::Suite;
    use aurelia_qa::export::{features, play_log, start, suite_specs, Selection};
    use classic_rules::ai::driver::ClassicMind;
    use classic_rules::ai::net_e2e::encode::{head, Candidate};
    use classic_rules::ai::net_e2e::mind::{NetPlayer, Scorer};
    use classic_rules::ai::net_e2e::model::{Memory, TurnCache, TurnInput};
    use classic_rules::ai::variant::NET_BASE;
    use classic_rules::ai::AiSettings;
    use classic_rules::classic::Classic;
    use classic_rules::knowledge::actions::acting_unit;
    use classic_rules::{ClassicGame, Command};
    use kernel::ai::mind::AiMind;
    use kernel::replay::journal::{take_record, JournalHeader, Record};

    /// A turn's input rows as the mind gave them to the scorer.
    #[derive(Debug, Clone, Default)]
    struct TurnRows {
        tiles: Vec<i16>,
        global: Vec<i16>,
        entities: Vec<i16>,
        meta: Vec<i32>,
        ids: Vec<i32>,
        events: Vec<i16>,
        event_meta: Vec<i32>,
    }

    /// One scored decision: its turn, head, actor and candidates.
    type Block = (usize, i32, i32, Vec<Vec<i16>>, Vec<[i32; 4]>);

    #[derive(Debug, Default)]
    struct Recorder {
        turns: Mutex<Vec<TurnRows>>,
        blocks: Mutex<Vec<Block>>,
    }

    impl Scorer for Recorder {
        fn label(&self) -> String {
            "recorder".to_owned()
        }

        fn memory(&self) -> Memory {
            Memory::default()
        }

        fn begin_turn(&self, input: &TurnInput<'_>, _memory: &Memory, _cache: &mut TurnCache) {
            let rows = TurnRows {
                tiles: input.tiles.to_vec(),
                global: input.global.to_vec(),
                entities: input.entities.to_vec(),
                meta: input.entity_meta.to_vec(),
                ids: input.entity_ids.to_vec(),
                events: input.events.to_vec(),
                event_meta: input.event_meta.to_vec(),
            };
            self.turns.lock().unwrap().push(rows);
        }

        fn score(&self, _cache: &TurnCache, head: usize, actor: i32, candidates: &[Candidate], out: &mut Vec<i32>) {
            let turn = self.turns.lock().unwrap().len() - 1;
            let rows = candidates.iter().map(|c| c.row.clone()).collect();
            let metas = candidates.iter().map(|c| c.meta).collect();
            self.blocks.lock().unwrap().push((turn, head as i32, actor, rows, metas));
            out.clear();
            out.resize(candidates.len(), 0);
        }

        fn end_turn(&self, _cache: &TurnCache, memory: &Memory) -> Memory {
            memory.clone()
        }
    }

    #[test]
    fn the_exporter_and_the_net_player_read_identical_rows() {
        let rules = ClassicBench::rules();
        let spec = suite_specs(&Suite::named("smoke").unwrap(), 1, 0, Some(20)).remove(0);
        let log = play_log(&rules, &spec).unwrap();
        let shard = features(&rules, &spec, &log.journal, Selection::Sample(1)).unwrap();

        // The replay through the net player.
        let recorder = Arc::new(Recorder::default());
        let net = NetPlayer::new(recorder.clone());
        let game_spec = spec.game().unwrap();
        let settings = AiSettings::for_rules(&rules, &game_spec.difficulty);
        let probe = ClassicMind::new(NET_BASE);
        let mut game = start(&rules, &game_spec);
        let (_, mut reader) = JournalHeader::read(&log.journal).unwrap();
        let mut open: Option<(u32, u8)> = None;
        let mut turn_of: BTreeMap<(u32, u8), usize> = BTreeMap::new();
        let mut produced: std::collections::BTreeSet<(usize, u32, u32)> = Default::default();
        type Production = (usize, i32, Vec<Vec<i16>>, Vec<[i32; 4]>);
        let mut productions: Vec<Production> = Vec::new();
        let mut states: BTreeMap<u8, classic_rules::ai::AiState> = BTreeMap::new();
        while !reader.is_at_end() {
            let Ok(record) = take_record::<Classic>(&mut reader, game.view().auth) else { break };
            let Record::Command(command) = record else { continue };
            let auth = game.view().auth;
            let (round, player) = (auth.turn().round, auth.active_player());
            if open != Some((round, player.0)) {
                open = Some((round, player.0));
                let state = states.entry(player.0).or_insert_with(|| classic_rules::ai::AiState::new(player, round));
                if net.begin(&game.ai_observation(&rules, player), &rules, state) {
                    turn_of.insert((round, player.0), recorder.turns.lock().unwrap().len() - 1);
                }
            }
            let mut units = Vec::new();
            if let Some(unit) = acting_unit(&command) {
                units.push(unit);
            } else if matches!(command, Command::EndTurn { .. }) {
                let auth = game.view().auth;
                let mut waiting: Vec<_> = auth.indexes().units_of(player).iter().filter_map(|h| auth.units().get(*h).map(|u| (u.core.id, *h, u.core.moves_left))).filter(|u| u.2 > 0).collect();
                waiting.sort_by_key(|u| u.0);
                units.extend(waiting.into_iter().map(|u| u.1));
            }
            if let Some(&turn) = turn_of.get(&(round, player.0)) {
                for unit in units {
                    if let Some(near) = probe.sense_unit(&game, &rules, &settings, player, unit) {
                        net.decide_unit(&near, &rules, &mut || unreachable!("not recording"));
                    }
                }
                // Production: the net's candidates where it decides at play —
                // before the expert's clear ran (emulated), or at the first order.
                if let Command::ClearProduction { city, .. } | Command::EnqueueProduction { city, .. } = command {
                    if produced.insert((turn, city.index, city.generation)) {
                        let sight = game.ai_observation(&rules, player);
                        if let Some(own) = sight.own_cities.iter().find(|c| c.handle == city) {
                            let cleared = matches!(command, Command::ClearProduction { .. });
                            let (actor, candidates) = net.production_candidates(&sight, &rules, own, cleared).expect("the open turn");
                            productions.push((turn, actor, candidates.iter().map(|c| c.row.clone()).collect::<Vec<_>>(), candidates.iter().map(|c| c.meta).collect::<Vec<_>>()));
                        }
                    }
                }
            }
            game.execute(&rules, command).unwrap();
        }

        // The turn rows: equal over the rows the turn starts with.
        let turns = recorder.turns.lock().unwrap();
        let (tiles, globals) = (shard.tile_count(), shard.c_global);
        let width = |rows: usize, total: usize| total / rows.max(1);
        let c_tile = width(shard.turns.len() * tiles, shard.tiles.len());
        let c_entity = width(shard.entity_meta.len(), shard.entities.len());
        let mut compared = 0;
        let mut events = 0;
        let c_event = width(shard.event_meta.len(), shard.events.len());
        for (t, &[round, player, base, _, _]) in shard.turns.iter().enumerate() {
            let mine = &turns[turn_of[&(round as u32, player as u8)]];
            assert_eq!(&shard.tiles[t * tiles * c_tile..(t + 1) * tiles * c_tile], &mine.tiles[..], "tiles of round {round} player {player}");
            assert_eq!(&shard.globals[t * globals..(t + 1) * globals], &mine.global[..], "global of round {round} player {player}");
            let n = mine.ids.len();
            let base = base as usize;
            assert_eq!(&shard.entities[base * c_entity..(base + n) * c_entity], &mine.entities[..], "entities of round {round} player {player}");
            let meta: Vec<i32> = shard.entity_meta[base..base + n].iter().flat_map(|m| [m[0], m[1], m[2]]).collect();
            let ids: Vec<i32> = shard.entity_meta[base..base + n].iter().map(|m| m[3]).collect();
            assert_eq!((meta, ids), (mine.meta.clone(), mine.ids.clone()), "entity meta of round {round} player {player}");
            // Format 7: the events, as the net player's tracker (in its AI
            // state) and the exporter's found them.
            let [v0, nv] = shard.turn_events[t];
            let (v0, nv) = (v0 as usize, nv as usize);
            assert_eq!(&shard.events[v0 * c_event..(v0 + nv) * c_event], &mine.events[..], "events of round {round} player {player}");
            let event_meta: Vec<i32> = shard.event_meta[v0..v0 + nv].iter().flat_map(|m| [m[0], m[1], if m[2] >= 0 { m[2] - base as i32 } else { -1 }, m[3]]).collect();
            assert_eq!(event_meta, mine.event_meta, "event meta of round {round} player {player}");
            events += nv;
            compared += 1;
        }

        // Every unit decision of the shard is one the net player scored, with
        // the same actor, rows and meta (entity indices local to the turn).
        let blocks = recorder.blocks.lock().unwrap();
        let c_candidate = width(shard.candidate_meta.len(), shard.candidates.len());
        let mut matched = 0;
        for &[turn_row, head_code, actor, first, count, _] in shard.decisions.iter().filter(|d| d[1] == head::UNIT) {
            let [round, player, base, _, _] = shard.turns[turn_row as usize];
            let turn = turn_of[&(round as u32, player as u8)];
            let local = |e: i32| if e >= 0 { e - base } else { e };
            let rows: Vec<Vec<i16>> = (first..first + count).map(|i| shard.candidates[i as usize * c_candidate..(i as usize + 1) * c_candidate].to_vec()).collect();
            let metas: Vec<[i32; 4]> = (first..first + count).map(|i| {
                let mut m = shard.candidate_meta[i as usize];
                m[2] = local(m[2]);
                m
            }).collect();
            let found = blocks.iter().any(|b| b.0 == turn && b.1 == head_code && b.2 == local(actor) && b.3 == rows && b.4 == metas);
            assert!(found, "round {round} player {player}: a unit decision of the shard the net player did not see the same way");
            matched += 1;
        }
        assert!(compared >= 20 && matched >= 40 && events >= 10, "{compared} turns, {matched} unit decisions, {events} events");

        // Every production decision of the shard is one the net player sees
        // the same way (actor row, rows, meta).
        let mut production_matched = 0;
        for &[turn_row, _, actor, first, count, _] in shard.decisions.iter().filter(|d| d[1] == head::PRODUCTION) {
            let [round, player, base, _, _] = shard.turns[turn_row as usize];
            let turn = turn_of[&(round as u32, player as u8)];
            let local = |e: i32| if e >= 0 { e - base } else { e };
            let rows: Vec<Vec<i16>> = (first..first + count).map(|i| shard.candidates[i as usize * c_candidate..(i as usize + 1) * c_candidate].to_vec()).collect();
            let metas: Vec<[i32; 4]> = (first..first + count).map(|i| {
                let mut m = shard.candidate_meta[i as usize];
                m[2] = local(m[2]);
                m
            }).collect();
            let found = productions.iter().any(|p| p.0 == turn && p.1 == local(actor) && p.2 == rows && p.3 == metas);
            assert!(found, "round {round} player {player}: a production decision the net player sees otherwise");
            production_matched += 1;
        }
        assert!(production_matched >= 8, "{production_matched} production decisions");
    }
}

mod dagger {
    //! DAgger recording: a net seat (here the dummy scorer) records its turns
    //! with the scripted mind's choices as labels; at a state both games
    //! share (the first player's first turn) the rows and labels are the
    //! exporter's of a plain `standard` game.

    use aurelia_qa::bench::modules::{BenchRules, ClassicBench};
    use aurelia_qa::bench::suite::Suite;
    use aurelia_qa::export::{features, play_log, start, suite_specs, Selection};
    use classic_rules::ai::driver::ClassicMind;
    use classic_rules::ai::net_e2e::encode::{head, tag, Turn};
    use classic_rules::ai::net_e2e::mind::NetPlayer;
    use classic_rules::ai::variant::Variant;
    use classic_rules::ai::AiDriver;
    use classic_rules::ClassicGame;

    #[test]
    fn the_net_records_the_exporters_rows_and_labels_at_a_shared_state() {
        let rules = ClassicBench::rules();
        let spec = suite_specs(&Suite::named("smoke").unwrap(), 1, 0, Some(2)).remove(0);
        let log = play_log(&rules, &spec).unwrap();
        let shard = features(&rules, &spec, &log.journal, Selection::Sample(1)).unwrap();

        let game_spec = spec.game().unwrap();
        let mut game = start(&rules, &game_spec);
        let first = game.view().auth.active_player();
        let start_rows = Turn::encode(&game.ai_observation(&rules, first), &rules);
        let mind = ClassicMind::with_net(Variant::Net, NetPlayer::dummy().recording());
        let mut driver = AiDriver::with_mind(ClassicBench::settings(&rules), Vec::new(), mind);
        driver.run(&mut game, &rules, u32::MAX);
        let recording = driver.mind_mut().net().unwrap().take_recording();
        let mine = recording.turns.iter().find(|t| t.round == start_rows.round && t.player == first).expect("the first turn recorded");
        assert!(recording.turns.len() >= 2 && recording.turns.iter().all(|t| !t.decisions.is_empty()), "every net turn has labels");

        // The turn rows of the start.
        let t = shard.turns.iter().position(|r| r[0] as u32 == start_rows.round && r[1] == i32::from(first.0)).expect("the exporter's first turn");
        let tiles = shard.tile_count() * start_rows.tiles.len() / shard.tile_count().max(1);
        assert_eq!(&shard.tiles[t * tiles..(t + 1) * tiles], &mine.turn.tiles[..], "tiles");
        assert_eq!(&shard.globals[t * shard.c_global..(t + 1) * shard.c_global], &mine.turn.global[..], "global");
        let (base, n, c_entity) = (shard.turns[t][2] as usize, start_rows.entity_count(), start_rows.entities.len() / start_rows.entity_count().max(1));
        assert_eq!(&shard.entities[base * c_entity..(base + n) * c_entity], &mine.turn.entities[..n * c_entity], "entities");
        assert_eq!(&shard.entity_meta[base..base + n], &mine.turn.entity_meta[..n], "entity meta");

        // The exporter's decisions of that turn, entity indices local.
        let c_candidate = shard.candidates.len() / shard.candidate_meta.len();
        type Block = (i32, i32, Vec<Vec<i16>>, Vec<[i32; 4]>, usize);
        let theirs: Vec<Block> = shard
            .decisions
            .iter()
            .filter(|d| d[0] as usize == t)
            .map(|&[_, h, actor, first, count, choice]| {
                let local = |e: i32| if e >= 0 { e - base as i32 } else { e };
                let rows = (first..first + count).map(|i| shard.candidates[i as usize * c_candidate..(i as usize + 1) * c_candidate].to_vec()).collect();
                let metas = (first..first + count).map(|i| {
                    let mut m = shard.candidate_meta[i as usize];
                    m[2] = local(m[2]);
                    m
                }).collect();
                (h, local(actor), rows, metas, choice as usize)
            })
            .collect();
        let found = |h: i32, actor: i32, cands: &[classic_rules::ai::net_e2e::encode::Candidate], label: usize| {
            let rows: Vec<Vec<i16>> = cands.iter().map(|c| c.row.clone()).collect();
            let metas: Vec<[i32; 4]> = cands.iter().map(|c| c.meta).collect();
            theirs.iter().any(|d| d.0 == h && d.1 == actor && d.2 == rows && d.3 == metas && d.4 == label)
        };
        let mut compared = 0;
        // Research: the same state at the turn's start.
        if let Some((h, a, c, l)) = mine.decisions.iter().find(|d| d.0 == head::RESEARCH) {
            assert!(found(*h, *a, c, *l), "the research decision and its label");
            compared += 1;
        }
        // The first unit decision (the same state: no unit has moved yet);
        // a wait the exporter places at the turn's end, in another state.
        let unit = mine.decisions.iter().find(|d| d.0 == head::UNIT).expect("a unit decision");
        if unit.2[unit.3].meta[0] != tag::WAIT {
            assert!(found(unit.0, unit.1, &unit.2, unit.3), "the first unit decision and its label");
            compared += 1;
        }
        assert_eq!(compared, 2, "the research and the first unit decision compared");
    }
}
