//! The remote player (`ai::net_e2e::remote`, classic/docs/ai-net-model.md,
//! «Remote MPS player») against a stand-in server in a thread that speaks the
//! wire and scores every candidate with `LinearScorer`'s function: the remote
//! player plays the stand-in scorer's game byte for byte, every nation keeps
//! its session, every BEGIN carries tables of the layout's widths, and the
//! chosen candidates come back with the next decision (the turn trace).
#![cfg(unix)]

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use classic_content::core::core_rules;
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::net_e2e::encode;
use classic_rules::ai::net_e2e::mind::{layout_hash, NetPlayer};
use classic_rules::ai::variant::Variant;
use classic_rules::ai::AiSettings;
use classic_rules::classic::Classic;
use classic_rules::replay::authoritative_hash;
use classic_rules::scenario::{generate_new_game, ScenarioConfig};
use classic_rules::{compile_rules, CompiledRuleset, GameState};
use kernel::ai::driver::AiDriver;

type Driver = AiDriver<Classic, ClassicMind>;

fn scenario(seed: u64) -> ScenarioConfig {
    ScenarioConfig { seed, players: 4, width: 32, height: 24, cities: 1, units: 2, water_ratio: 10 }
}

fn play(rules: &CompiledRuleset, game: &mut GameState, mind: ClassicMind, rounds: u32) -> u64 {
    let settings = AiSettings::for_rules(rules, rules.default_difficulty());
    let mut driver: Driver = AiDriver::with_mind(settings, Vec::new(), mind);
    while game.view().auth.turn().round < rounds && !game.is_over() {
        driver.run(game, rules, u32::MAX);
    }
    authoritative_hash(game)
}

/// What the stand-in server saw.
#[derive(Debug, Default)]
struct Seen {
    begins: u64,
    scores: u64,
    /// Notes (a chosen candidate of the session's previous decision) in range.
    notes: u64,
    sessions: BTreeMap<u64, u64>,
}

struct Widths {
    tile: usize,
    global: usize,
    entity: usize,
    candidate: usize,
    event: usize,
}

fn read_frame(s: &mut UnixStream) -> Option<Vec<u8>> {
    let mut len = [0u8; 4];
    s.read_exact(&mut len).ok()?;
    let mut body = vec![0u8; u32::from_le_bytes(len) as usize];
    s.read_exact(&mut body).ok()?;
    Some(body)
}

fn reply(s: &mut UnixStream, body: &[u8]) {
    s.write_all(&(body.len() as u32 + 1).to_le_bytes()).unwrap();
    s.write_all(&[0]).unwrap();
    s.write_all(body).unwrap();
}

struct Cur<'a>(&'a [u8]);

impl Cur<'_> {
    fn take(&mut self, n: usize) -> &[u8] {
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        a
    }
    fn u32(&mut self) -> u32 {
        u32::from_le_bytes(self.take(4).try_into().unwrap())
    }
    fn i32(&mut self) -> i32 {
        i32::from_le_bytes(self.take(4).try_into().unwrap())
    }
    fn u64(&mut self) -> u64 {
        u64::from_le_bytes(self.take(8).try_into().unwrap())
    }
}

/// One connection: HELLO, then BEGINs and SCOREs, each answered with
/// `LinearScorer`'s logits (weights −8 … 8 by column).
fn serve(mut s: UnixStream, hash: u64, w: &Widths, seen: &Mutex<Seen>) {
    let mut last_k: BTreeMap<u64, usize> = BTreeMap::new();
    while let Some(frame) = read_frame(&mut s) {
        let mut c = Cur(&frame[1..]);
        match frame[0] {
            1 => {
                assert_eq!(c.u32(), 1, "the protocol version");
                let mut out = Vec::new();
                out.extend_from_slice(&1u32.to_le_bytes());
                out.extend_from_slice(&hash.to_le_bytes());
                out.extend_from_slice(&6u32.to_le_bytes());
                out.extend_from_slice(&[0, 0, 1]);
                for x in [w.tile, w.global, w.entity, w.candidate, w.event] {
                    out.extend_from_slice(&(x as u32).to_le_bytes());
                }
                out.extend_from_slice(&4u32.to_le_bytes());
                out.extend_from_slice(b"fake");
                reply(&mut s, &out);
            }
            2 => {
                let session = c.u64();
                let _key = c.u64();
                let tiles = c.u32() as usize;
                if c.take(1)[0] == 1 {
                    c.take(tiles * 7 * 4);
                }
                c.take(tiles * w.tile * 2 + w.global * 2);
                let e = c.u32() as usize;
                c.take(e * (w.entity * 2 + 3 * 4 + 4));
                let v = c.u32() as usize;
                c.take(v * (w.event * 2 + 16));
                assert!(c.0.is_empty(), "a BEGIN of the layout's widths");
                let mut seen = seen.lock().unwrap();
                seen.begins += 1;
                *seen.sessions.entry(session).or_default() += 1;
                last_k.insert(session, 0);
                reply(&mut s, &0i32.to_le_bytes());
            }
            3 => {
                let session = c.u64();
                let note = c.i32();
                let (_head, _actor, k) = (c.u32(), c.i32(), c.u32() as usize);
                let rows = c.take(k * w.candidate * 2).to_vec();
                c.take(k * 16);
                assert!(c.0.is_empty(), "a SCORE of the layout's width");
                let prev = last_k.get(&session).copied().expect("a SCORE after its BEGIN");
                assert!(note < prev as i32, "a note within the previous decision's candidates");
                let mut out = Vec::with_capacity(4 * k);
                for r in rows.chunks_exact(w.candidate * 2) {
                    let sum: i64 = r.chunks_exact(2).enumerate().map(|(i, b)| i64::from(i16::from_le_bytes([b[0], b[1]])) * (((i as i64 * 37 + 11) % 17) - 8)).sum();
                    out.extend_from_slice(&(sum.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32).to_le_bytes());
                }
                last_k.insert(session, k);
                let mut seen = seen.lock().unwrap();
                seen.scores += 1;
                seen.notes += u64::from(note >= 0);
                drop(seen);
                reply(&mut s, &out);
            }
            other => panic!("request kind {other}"),
        }
    }
}

#[test]
fn the_remote_player_plays_its_servers_game() {
    let rules = compile_rules(core_rules()).unwrap();
    let layout = encode::layout(&rules);
    let w = Arc::new(Widths { tile: layout.tile.len(), global: layout.global.len(), entity: layout.entity.len(), candidate: layout.candidate.len(), event: layout.event.len() });
    let hash = layout_hash(&rules);
    let dir: PathBuf = std::env::temp_dir().join(format!("aurr{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let sock = dir.join("s");
    let _ = std::fs::remove_file(&sock);
    let listener = UnixListener::bind(&sock).unwrap();
    let seen = Arc::new(Mutex::new(Seen::default()));
    {
        let (w, seen) = (w.clone(), seen.clone());
        std::thread::spawn(move || {
            for s in listener.incoming() {
                let (w, seen) = (w.clone(), seen.clone());
                std::thread::spawn(move || serve(s.unwrap(), hash, &w, &seen));
            }
        });
    }
    let mut game = generate_new_game(&rules, &scenario(0x0_9E7A));
    let local = play(&rules, &mut game, ClassicMind::with_net(Variant::Net, NetPlayer::dummy()), 12);
    let mut game = generate_new_game(&rules, &scenario(0x0_9E7A));
    let net = NetPlayer::from_path(&format!("remote:{}", sock.display())).expect("the stand-in server");
    assert!(net.label().starts_with("remote:"), "{}", net.label());
    let remote = play(&rules, &mut game, ClassicMind::with_net(Variant::Net, net), 12);
    let seen = seen.lock().unwrap();
    eprintln!("{seen:?}");
    assert_eq!(remote, local, "the remote player plays the stand-in scorer's game");
    assert_eq!(seen.sessions.len(), 4, "a session per nation");
    assert!(seen.sessions.values().all(|&n| n >= 10), "each nation's turns in its session");
    assert!(seen.scores > 100 && seen.notes > seen.scores / 2, "the chosen candidates sent back");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_remote_player_without_a_server_is_an_error() {
    let err = NetPlayer::from_path("remote:/nonexistent/aurelia.sock").unwrap_err();
    assert!(err.contains("aurelia_nn.remote serve"), "{err}");
}
