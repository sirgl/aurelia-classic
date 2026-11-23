//! The player `army-net` of `ai-exam suite` (DESIGN.md, «Play»): on the learner's turn the net picks
//! a unit of the army (or the end), then one of its legal actions, the game applies it, and the net
//! looks again; when it ends, every army unit stands, so the seat's mind (`standard`) plays only the
//! rest of the seat (the civilians, the home city). Greedy: the highest logit wins; ties by order.
//! The weights come from `ARMY_NET` (a file of `army/export.py`), loaded once per process.
//!
//! DAgger (`ARMY_DAGGER=teachers.json` of `army/train.py`, with `suite --record`): before each of the
//! net's decisions the family's best reference plays its turn on a copy of the game; its first decision
//! (or "end" when it gives none) is recorded as the label of the moment the net is in, and the net's
//! own choice is what the game applies.

use std::collections::BTreeMap;
use std::sync::OnceLock;
use std::time::Instant;

use classic_rules::{CompiledRuleset, GameState};

use super::model::Net;
use super::observe::{self, Action};
use super::Orders;
use crate::trainers::{act, families, Scene};

/// Decisions of one unit in one turn at most (a unit that keeps moving back and forth is stopped).
const PER_UNIT: u32 = 6;
/// Decisions of one turn at most.
const PER_TURN: u32 = 96;

static NET: OnceLock<Result<Net, String>> = OnceLock::new();

thread_local! {
    /// The DAgger expert's own memory between the turns of one play (the staged assault's "engaged",
    /// "waited", "resting" live in `Scene::track`): a probe plays on a copy, and without this the copy's
    /// commitments were thrown away, so the expert answered every turn as its first ("wait for the stage").
    static EXPERT_TRACK: std::cell::RefCell<Option<crate::trainers::assault::Track>> = const { std::cell::RefCell::new(None) };
}

/// A new play: the expert's memory starts empty, and so does the net's (variant M).
pub fn reset_expert() {
    EXPERT_TRACK.with(|t| *t.borrow_mut() = None);
    MEMORY.with(|m| *m.borrow_mut() = None);
}

/// Variant M's memory of the play on this thread: the orders and scene it plays, the memory state.
struct MemoryRun {
    orders: Orders,
    scene: Scene,
    state: super::model::MemoryState,
}

thread_local! {
    static MEMORY: std::cell::RefCell<Option<MemoryRun>> = const { std::cell::RefCell::new(None) };
}

/// The memory kept at its start every turn (`ARMY_NET_ZERO_MEMORY=1`: the ablation of proof (a)).
fn memory_zeroed() -> bool {
    static ZERO: OnceLock<bool> = OnceLock::new();
    *ZERO.get_or_init(|| std::env::var_os("ARMY_NET_ZERO_MEMORY").is_some())
}

/// The teacher's own state numbers (`record::TRACK`) for a staged-assault scene: engaged, waited, the stage
/// (0 gather: a field force stands; 1 march; 2 stage: waiting for the shooters; 3 assault; 4 heal-rotate:
/// someone rests; 5 withdraw: not played by the planner), the resting slots as bits. None elsewhere.
pub fn teacher_state(game: &GameState, scene: &Scene, orders: &Orders, track: &crate::trainers::assault::Track) -> [i16; super::record::TRACK] {
    if !crate::trainers::assault::is_assault(scene.fam) {
        return super::record::NO_TRACK;
    }
    let field = scene.theirs_alive(game).iter().any(|(p, _)| p.role == "field");
    let stage = if field {
        0
    } else if !track.resting.is_empty() {
        4
    } else if track.engaged {
        3
    } else if track.waited > 0 {
        2
    } else {
        1
    };
    let mask = orders.army.iter().take(super::model::SLOTS).enumerate().filter(|(_, id)| track.resting.contains(id)).fold(0u16, |m, (s, _)| m | (1 << s));
    [i16::from(track.engaged), track.waited.min(i16::MAX as u32) as i16, stage, mask as i16]
}

/// The net for this process (a test: weights made in the test), when none is loaded yet.
pub fn install_net(net: Net) -> bool {
    NET.set(Ok(net)).is_ok()
}

/// Save and load in the middle of a play (the guard of the memory's persistence, `tests/army_net.rs`): at the
/// learner's turn of this round (relative), the game goes through the save format, and the net's memory through
/// its sidecar file beside the save (`sidecar`: false = the sidecar is not written, the memory starts fresh).
#[derive(Debug, Clone)]
pub struct SaveAt {
    pub round: u32,
    pub dir: std::path::PathBuf,
    pub sidecar: bool,
}

thread_local! {
    static SAVE_AT: std::cell::RefCell<Option<SaveAt>> = const { std::cell::RefCell::new(None) };
}

pub fn set_save_at(at: Option<SaveAt>) {
    SAVE_AT.with(|s| *s.borrow_mut() = at);
}

pub fn save_at() -> Option<SaveAt> {
    SAVE_AT.with(|s| s.borrow().clone())
}

const SIDECAR_MAGIC: u32 = u32::from_le_bytes(*b"ARMM");
const SIDECAR_VERSION: u32 = 1;

/// The memory of this thread's play as the sidecar of a save keyed by the game's id (until the net moves into the
/// AI and the memory into the save body, as net2's): `"ARMM" u32 | version u32 | id_len u32 | id | u, p, alive (f32)`.
pub fn memory_sidecar(id: &str) -> Option<Vec<u8>> {
    MEMORY.with(|m| {
        let m = m.borrow();
        let st = &m.as_ref()?.state;
        let mut out = Vec::new();
        for v in [SIDECAR_MAGIC, SIDECAR_VERSION, id.len() as u32] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(id.as_bytes());
        for x in st.u.iter().chain(&st.p).chain(&st.alive) {
            out.extend_from_slice(&x.to_le_bytes());
        }
        Some(out)
    })
}

/// The memory from a sidecar: false (and the memory starts fresh) when it is not this game's or does not decode.
pub fn restore_memory(id: &str, bytes: &[u8], orders: &Orders, scene: &Scene) -> bool {
    let Ok(net) = net() else { return false };
    let Some(mut state) = net.initial_memory() else { return false };
    let u32_at = |i: usize| bytes.get(i..i + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    let ok = (|| {
        if u32_at(0)? != SIDECAR_MAGIC || u32_at(4)? != SIDECAR_VERSION {
            return None;
        }
        let n = u32_at(8)? as usize;
        if bytes.get(12..12 + n)? != id.as_bytes() {
            return None;
        }
        let floats: Vec<f32> = bytes.get(12 + n..)?.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
        let (nu, np) = (state.u.len(), state.p.len());
        if floats.len() != nu + np + super::model::SLOTS {
            return None;
        }
        state.u.copy_from_slice(&floats[..nu]);
        state.p.copy_from_slice(&floats[nu..nu + np]);
        state.alive.copy_from_slice(&floats[nu + np..]);
        Some(())
    })()
    .is_some();
    MEMORY.with(|m| *m.borrow_mut() = net.initial_memory().map(|fresh| MemoryRun { orders: orders.clone(), scene: scene.clone(), state: if ok { state.clone() } else { fresh } }));
    ok
}

/// `SaveAt`: the game written in the save format and read back, the memory through its sidecar (or lost).
pub fn save_and_load(rules: &CompiledRuleset, game: &GameState, orders: &Orders, scene: &mut Scene, at: &SaveAt) -> Result<GameState, String> {
    let id = format!("{}-{}-{}-{}", scene.fam.name(), scene.era, scene.instance, scene.attempt);
    std::fs::create_dir_all(&at.dir).map_err(|e| e.to_string())?;
    let save = at.dir.join(format!("{id}.aurelia"));
    let side = at.dir.join(format!("{id}.aurelia.army-memory"));
    std::fs::write(&save, classic_rules::save_codec::to_bytes(&classic_rules::persistence::encode::encode(game, rules))).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&side);
    if at.sidecar {
        if let Some(bytes) = memory_sidecar(&id) {
            std::fs::write(&side, bytes).map_err(|e| e.to_string())?;
        }
    }
    // A new process: nothing of the play in memory.
    MEMORY.with(|m| *m.borrow_mut() = None);
    let bytes = std::fs::read(&save).map_err(|e| e.to_string())?;
    let loaded = classic_rules::persistence::decode::decode(&classic_rules::save_codec::from_bytes(&bytes).map_err(|e| format!("{e:?}"))?, rules).map_err(|e| format!("{e:?}"))?;
    // A load gives the units new handles: the scene's pieces are found again by their entity ids.
    let auth = loaded.view().auth;
    for p in scene.mine.iter_mut().chain(scene.theirs.iter_mut()) {
        if let Some((h, _)) = auth.units().iter_handles().find(|(_, c)| c.id == p.id) {
            p.h = h;
        }
    }
    let side_bytes = std::fs::read(&side).unwrap_or_default();
    restore_memory(&id, &side_bytes, orders, scene);
    Ok(loaded)
}

/// Our turn ends (before the end-of-turn command runs): variant M's memory takes this moment, and in a DAgger
/// play the moment is recorded as an unlabelled memory state with the expert's state then.
pub fn turn_end(game: &GameState, rules: &CompiledRuleset) {
    let Ok(net) = net() else { return };
    MEMORY.with(|m| {
        let mut m = m.borrow_mut();
        let Some(run) = m.as_mut() else { return };
        let tokens = observe::tokens(game, rules, &run.orders);
        let slots = super::record::slots_of(game, &run.orders, &tokens);
        if super::record::active() {
            let track = EXPERT_TRACK.with(|t| t.borrow().as_ref().map(|t| teacher_state(game, &run.scene, &run.orders, t))).unwrap_or(super::record::NO_TRACK);
            super::record::push_memory_state(game, rules, track);
        }
        if memory_zeroed() {
            return;
        }
        let body = net.body(&tokens, None, Some(&super::model::MemIn { state: &run.state, slots: &slots }));
        let mut state = run.state.clone();
        net.memory_step(&body, &tokens, &slots, &mut state);
        run.state = state;
    });
}
static EXPERTS: OnceLock<Option<BTreeMap<String, String>>> = OnceLock::new();

/// The DAgger expert of each family: the reference with the highest win rate on the training plays
/// (`teachers.json`: `wins` = {"family/player": [won, played]}).
fn experts() -> Option<&'static BTreeMap<String, String>> {
    EXPERTS
        .get_or_init(|| {
            let path = std::env::var_os("ARMY_DAGGER")?;
            let text = std::fs::read_to_string(path).ok()?;
            let v: serde_json::Value = serde_json::from_str(&text).ok()?;
            let mut best: BTreeMap<String, (f64, String)> = BTreeMap::new();
            for (key, wn) in v["wins"].as_object()? {
                let Some((fam, player)) = key.split_once('/') else { continue };
                if !player.starts_with("ref-") {
                    continue;
                }
                let rate = wn[0].as_f64().unwrap_or(0.0) / wn[1].as_f64().unwrap_or(1.0).max(1.0);
                if best.get(fam).is_none_or(|b| rate > b.0 || (rate == b.0 && player < b.1.as_str())) {
                    best.insert(fam.to_owned(), (rate, player.to_owned()));
                }
            }
            Some(best.into_iter().map(|(f, (_, p))| (f, p)).collect())
        })
        .as_ref()
}

/// The families whose expert plans the whole turn (the staged assault: go only when every shooter can
/// arrive; the siege's wait until the city is at half): a probe in the middle of the net's turn asks such a
/// planner about a turn it did not plan (half the army has moved), and its answer is "wait". There the
/// probe is per TURN: at the start of each of the net's turns the expert plays the whole turn on a copy, and
/// every decision of it is a label (`ARMY_DAGGER_TURN`, comma list; default: the siege set).
fn turn_probed(scene: &Scene) -> bool {
    let list = std::env::var("ARMY_DAGGER_TURN").unwrap_or_else(|_| "siege,ram,assault-heal,city-fire,hill-archers,planned-assault,promotion-choice,rush".to_owned());
    list.split(',').any(|f| f == scene.fam.name())
}

/// The families DAgger records (`ARMY_DAGGER_FAMILIES`, comma list; default: all).
fn dagger_family(scene: &Scene) -> bool {
    match std::env::var("ARMY_DAGGER_FAMILIES") {
        Ok(list) => list.split(',').any(|f| f == scene.fam.name()),
        Err(_) => true,
    }
}

/// DAgger: the expert's label for this moment (`record::probe` on a copy of the game), or, for a turn-planned
/// family at the turn's first decision, the expert's whole turn from here.
fn probe(rules: &CompiledRuleset, game: &GameState, scene: &Scene, first_of_turn: bool) {
    let Some(expert) = experts().and_then(|e| e.get(scene.fam.name())) else { return };
    if !super::record::active() || !dagger_family(scene) {
        return;
    }
    probe_with(rules, game, scene, expert, first_of_turn);
}

/// The guard of the probe (`tests/army_net.rs`, player `probe:<reference>`): the reference plays the real
/// game itself, muted, and before each of its turns the probe plays the same turn on a copy, recorded. When
/// the probe is right its labels are the reference's own. The probe is given the scene the net's seat holds:
/// the reference's commitments (engaged, waited, resting) are not in it, because the net plays the real game
/// and the real scene's track never gets them; the probe must carry them itself (`EXPERT_TRACK`). Without
/// this the guard passed on code that dropped them (the real scene handed them to the copy).
pub fn probe_and_play(rules: &CompiledRuleset, game: &mut GameState, scene: &mut Scene, expert: &str) {
    if super::record::active() {
        let mut blind = scene.clone();
        blind.track = scene.track.without_commitments();
        probe_with(rules, game, &blind, expert, true);
    }
    families::reference_turn(rules, game, scene, expert);
}

fn probe_with(rules: &CompiledRuleset, game: &GameState, scene: &Scene, expert: &str, first_of_turn: bool) {
    let mut copy = game.clone();
    let mut sc = scene.clone();
    if let Some(track) = EXPERT_TRACK.with(|t| t.borrow().clone()) {
        sc.track = track;
    }
    if turn_probed(scene) {
        if !first_of_turn {
            return;
        }
        super::record::teach();
        families::reference_turn(rules, &mut copy, &mut sc, expert);
        super::record::push_end(&copy, rules);
        super::record::mute();
        EXPERT_TRACK.with(|t| *t.borrow_mut() = Some(sc.track));
        return;
    }
    super::record::probe();
    families::reference_turn(rules, &mut copy, &mut sc, expert);
    if !super::record::probe_done() {
        super::record::push_end(game, rules);
    }
    // A per-decision probe changes the expert's memory only at the turn's first decision (its plan's start).
    if first_of_turn {
        EXPERT_TRACK.with(|t| *t.borrow_mut() = Some(sc.track));
    }
}

pub fn net() -> Result<&'static Net, String> {
    let loaded = NET.get_or_init(|| {
        let path = std::env::var_os("ARMY_NET").ok_or("set ARMY_NET to the weights of the army net")?;
        Net::load(std::path::Path::new(&path))
    });
    loaded.as_ref().map_err(Clone::clone)
}

/// Time spent in one turn, for the decision-time measurement.
#[derive(Debug, Default, Clone, Copy)]
pub struct Timing {
    pub decisions: u64,
    /// Building the tokens and the candidates.
    pub observe_ns: u64,
    /// The net's forward passes (variant B: without the map encoder's work).
    pub forward_ns: u64,
    /// Variant B: the map encoded whole at the start of the turn, and its incremental updates.
    pub encode_ns: u64,
    pub update_ns: u64,
    pub updates: u64,
    pub turns: u64,
}

/// One turn of the army: decisions until the net ends it.
pub fn turn(rules: &CompiledRuleset, game: &mut GameState, orders: &Orders, scene: &Scene) -> Result<Timing, String> {
    let net = net()?;
    if net.has_memory() {
        MEMORY.with(|m| {
            let mut m = m.borrow_mut();
            if m.is_none() {
                *m = net.initial_memory().map(|state| MemoryRun { orders: orders.clone(), scene: scene.clone(), state });
            }
        });
    }
    let memory = MEMORY.with(|m| m.borrow().as_ref().map(|r| r.state.clone()));
    let mut timing = Timing::default();
    let mut made: Vec<(kernel::ids::EntityId, u32)> = Vec::new();
    let mut map: Option<super::model::MapState> = None;
    for step in 0..PER_TURN {
        let t0 = Instant::now();
        let mut tokens = observe::tokens(game, rules, orders);
        // A unit stopped by the limit is not offered again.
        for i in 0..tokens.len() {
            let id = tokens.unit[i].and_then(|h| act::core_of(game, h)).map(|c| c.id);
            if id.is_some_and(|id| made.iter().any(|(x, n)| *x == id && *n >= PER_UNIT)) {
                tokens.selectable[i] = false;
            }
        }
        if !tokens.selectable.iter().any(|s| *s) {
            break;
        }
        let t_obs = Instant::now();
        probe(rules, game, scene, step == 0);
        let t_map = Instant::now();
        if net.has_map() {
            let planes = super::model::planes(&orders.statics, &tokens);
            let tm = Instant::now();
            match map.as_mut() {
                None => {
                    map = net.map_full(planes);
                    timing.encode_ns += tm.elapsed().as_nanos() as u64;
                }
                Some(st) => {
                    net.map_update(st, planes);
                    timing.update_ns += tm.elapsed().as_nanos() as u64;
                    timing.updates += 1;
                }
            }
        }
        let t1 = Instant::now();
        let slots = if memory.is_some() { super::record::slots_of(game, orders, &tokens) } else { Vec::new() };
        let mem_in = memory.as_ref().map(|state| super::model::MemIn { state, slots: &slots });
        let body = net.body(&tokens, map.as_ref(), mem_in.as_ref());
        let (logits, end) = net.pointer(&body, &tokens);
        let t2 = Instant::now();
        let pick = logits.iter().enumerate().filter_map(|(i, l)| l.map(|l| (i, l))).fold(None::<(usize, f32)>, |best, (i, l)| if best.is_none_or(|b| l > b.1) { Some((i, l)) } else { best });
        // The observation without the DAgger probe and the map encoder (timed on their own).
        timing.observe_ns += (t_obs - t0).as_nanos() as u64;
        let _ = t_map;
        timing.forward_ns += (t2 - t1).as_nanos() as u64;
        let Some((at, best)) = pick else { break };
        if end >= best {
            timing.decisions += 1;
            break;
        }
        let Some(unit) = tokens.unit[at] else { break };
        let Some(core) = act::core_of(game, unit) else { break };
        let t3 = Instant::now();
        let candidates = observe::candidates(game, rules, orders, unit);
        let t4 = Instant::now();
        let scores = net.actions(&body, at, &candidates, map.as_ref());
        let t5 = Instant::now();
        timing.observe_ns += (t4 - t3).as_nanos() as u64;
        timing.forward_ns += (t5 - t4).as_nanos() as u64;
        timing.decisions += 1;
        match made.iter_mut().find(|(x, _)| *x == core.id) {
            Some(entry) => entry.1 += 1,
            None => made.push((core.id, 1)),
        }
        let mut order: Vec<usize> = (0..candidates.len()).collect();
        order.sort_by(|a, b| scores[*b].total_cmp(&scores[*a]).then(a.cmp(b)));
        let mut acted = false;
        for i in order {
            match &candidates[i].action {
                Action::Done => {
                    act::stand(game, rules, unit);
                    acted = true;
                }
                Action::Command(cmd) => acted = act::try_cmd(game, rules, orders.side, cmd.clone()),
            }
            if acted {
                break;
            }
        }
        if !acted {
            act::stand(game, rules, unit);
        }
    }
    // The end: the army stands; the seat's mind plays only what is not the army's.
    let auth = game.view().auth;
    let army: Vec<classic_rules::ids::UnitHandle> = auth.indexes().units_of(orders.side).iter().copied().filter(|h| auth.units().get(*h).is_some_and(|u| orders.army.contains(&u.core.id))).collect();
    for h in army {
        act::stand(game, rules, h);
    }
    Ok(timing)
}
