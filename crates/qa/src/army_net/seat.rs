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
    EXPERT_REAL.with(|r| *r.borrow_mut() = false);
    MEMORY.with(|m| *m.borrow_mut() = None);
    NOTES.with(|n| n.borrow_mut().clear());
    STAGE.with(|s| *s.borrow_mut() = None);
    REST.with(|r| *r.borrow_mut() = None);
}

/// Variant M's memory of the play on this thread: the orders and scene it plays, the memory state.
struct MemoryRun {
    orders: Orders,
    scene: Scene,
    state: super::model::MemoryState,
    /// Time in the memory's turn-end updates (the tokens, the body, the step).
    memory_ns: u64,
    /// MB: the map of the turn (the memory state reads it).
    map: Option<super::model::MapState>,
}

thread_local! {
    /// MN: the notes the net wrote, a line per turn, for the trace.
    static NOTES: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
    /// The plan's own stage and engaged after the last turn end (`Net::plan_stage`), for the round log.
    static STAGE: std::cell::RefCell<Option<(usize, bool)>> = const { std::cell::RefCell::new(None) };
    /// The net's own "resting" per army slot after its last turn end (the notes' `rest` of MBN, else the memory's unit readout), for the unit log.
    static REST: std::cell::RefCell<Option<[bool; super::model::SLOTS]>> = const { std::cell::RefCell::new(None) };
}

/// The net's own resting set written at its last turn end (taken: None until the next one).
pub fn take_rest() -> Option<[bool; super::model::SLOTS]> {
    REST.with(|r| r.borrow_mut().take())
}

/// The net's own stage written at its last turn end (taken: None until the next one).
pub fn take_stage() -> Option<(usize, bool)> {
    STAGE.with(|s| s.borrow_mut().take())
}

/// The notes written since the last call (for the play's trace).
pub fn take_notes() -> Vec<String> {
    NOTES.with(|n| std::mem::take(&mut *n.borrow_mut()))
}

/// The time the memory's updates took in this thread's play (ns).
pub fn memory_ns() -> u64 {
    MEMORY.with(|m| m.borrow().as_ref().map_or(0, |r| r.memory_ns))
}

/// The history masked every decision (`ARMY_NET_NO_FACTS=1`: the ablation of MH's proof (a)).
fn history_masked() -> bool {
    static NO: OnceLock<bool> = OnceLock::new();
    *NO.get_or_init(|| std::env::var_os("ARMY_NET_NO_FACTS").is_some())
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

thread_local! {
    /// The net a test installed for its own thread (`install_net`); it comes before the process's `ARMY_NET`.
    static INSTALLED: std::cell::Cell<Option<&'static Net>> = const { std::cell::Cell::new(None) };
}

/// The net for this thread (a test: weights made in the test). Not the process's: the tests of one binary run in
/// parallel threads, and a process-wide slot filled by the first caller (or by a failed `ARMY_NET` load of another
/// test's play) made the result depend on which test came first. The net lives to the end of the process (a leak).
pub fn install_net(net: Net) {
    INSTALLED.with(|i| i.set(Some(Box::leak(Box::new(net)))));
}

thread_local! {
    /// The net of this thread's plays instead of the process's (`set_thread_net`; `SaveAt::then` after a load).
    static THREAD_NET: std::cell::Cell<Option<&'static Net>> = const { std::cell::Cell::new(None) };
    /// `SaveAt::compare`: whether the tokens after the last load equalled those before the save.
    static LOAD_SAME: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// This thread's plays use `net` (None: the process's net again).
pub fn set_thread_net(net: Option<&'static Net>) {
    THREAD_NET.with(|t| t.set(net));
}

/// `SaveAt::compare`: whether the tokens after the last load equalled those before the save (taken).
pub fn take_load_same() -> Option<bool> {
    LOAD_SAME.with(|c| c.take())
}

/// The numbers of the tokens (main, extras, third, history), without the handles a load renumbers.
fn token_numbers(t: &observe::Tokens) -> Vec<u32> {
    let mut v: Vec<u32> = Vec::new();
    let f = |v: &mut Vec<u32>, x: &[f32]| v.extend(x.iter().map(|y| y.to_bits()));
    for (a, b) in t.feat.iter().zip(&t.xfeat) {
        f(&mut v, a);
        f(&mut v, b);
    }
    for (a, b) in t.third.feat.iter().zip(&t.third.x) {
        f(&mut v, a);
        f(&mut v, b);
    }
    for h in &t.hist {
        f(&mut v, h);
    }
    v.extend(t.dist.iter().chain(&t.third.dist).map(|d| u32::from(*d)));
    v.extend(t.pos.iter().chain(&t.third.pos).map(|p| *p as u32));
    v.extend(t.selectable.iter().map(|s| u32::from(*s)));
    v.push(t.len() as u32);
    v.push(t.third.len() as u32);
    v
}

/// Save and load in the middle of a play (the guard of the memory's persistence, `tests/army_net.rs`): at the
/// learner's turn of this round (relative), the game goes through the save format, and the net's memory through
/// its sidecar file beside the save (`sidecar`: false = the sidecar is not written, the memory starts fresh).
#[derive(Debug, Clone, Default)]
pub struct SaveAt {
    pub round: u32,
    pub dir: std::path::PathBuf,
    pub sidecar: bool,
    /// The net that plays on after the load (None: the same): a save of one net played on by another (the old net's
    /// save by its typed conversion, `tests/army_net_types.rs`).
    pub then: Option<&'static Net>,
    /// Compare the tokens before the save and after the load (`take_load_same`).
    pub compare: bool,
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
    MEMORY.with(|m| *m.borrow_mut() = net.initial_memory().map(|fresh| MemoryRun { orders: orders.clone(), scene: scene.clone(), state: if ok { state.clone() } else { fresh }, memory_ns: 0, map: None }));
    ok
}

/// `SaveAt`: the game written in the save format and read back, the memory through its sidecar (or lost).
pub fn save_and_load(rules: &CompiledRuleset, game: &GameState, orders: &Orders, scene: &mut Scene, at: &SaveAt) -> Result<GameState, String> {
    let id = format!("{}-{}-{}-{}", scene.fam.name(), scene.era, scene.instance, scene.attempt);
    std::fs::create_dir_all(&at.dir).map_err(|e| e.to_string())?;
    let save = at.dir.join(format!("{id}.aurelia"));
    let side = at.dir.join(format!("{id}.aurelia.army-memory"));
    let before = at.compare.then(|| token_numbers(&observe::tokens(game, rules, orders)));
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
    // A load gives the units new handles: the scene's pieces are found again by their entity ids (every piece list:
    // ours, theirs, the third nation's of the camp and third-party families).
    let auth = loaded.view().auth;
    for p in scene.mine.iter_mut().chain(scene.theirs.iter_mut()).chain(scene.third.iter_mut()) {
        if let Some((h, _)) = auth.units().iter_handles().find(|(_, c)| c.id == p.id) {
            p.h = h;
        }
    }
    if let Some(n) = at.then {
        set_thread_net(Some(n));
    }
    if let Some(b) = before {
        let after = token_numbers(&observe::tokens(&loaded, rules, orders));
        LOAD_SAME.with(|c| c.set(Some(b == after)));
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
        let t_mem = Instant::now();
        let mut tokens = observe::tokens(game, rules, &run.orders);
        if history_masked() {
            observe::mask_history(&mut tokens);
        }
        let slots = super::record::slots_of(game, &run.orders, &tokens);
        if super::obsdump::wanted() {
            super::obsdump::moment(rules, game, &run.orders, &tokens, &slots, u32::MAX, &[]);
            super::obsdump::moment_notes(run.state.notes.as_ref());
            super::obsdump::choice(-2, -2);
        }
        if super::record::active() {
            let track = EXPERT_TRACK.with(|t| t.borrow().as_ref().map(|t| teacher_state(game, &run.scene, &run.orders, t))).unwrap_or(super::record::NO_TRACK);
            super::record::push_memory_state(game, rules, track);
        }
        if memory_zeroed() {
            return;
        }
        let body = net.body(&tokens, run.map.as_ref(), Some(&super::model::MemIn { state: &run.state, slots: &slots }));
        let mut state = run.state.clone();
        net.memory_step(&body, &tokens, &slots, &mut state);
        STAGE.with(|s| *s.borrow_mut() = net.plan_stage(&state));
        REST.with(|r| {
            *r.borrow_mut() = match state.notes.as_ref() {
                Some(ns) => Some(std::array::from_fn(|s| ns.pull[s] > 0.5)),
                None => net.resting_readout(&state),
            }
        });
        if let Some(ns) = state.notes.as_ref() {
            let round = game.view().auth.turn().round.saturating_sub(run.orders.start_round);
            NOTES.with(|n| n.borrow_mut().push(format!("r{round} {}", ns.written.join(" · "))));
        }
        run.state = state;
        run.memory_ns += t_mem.elapsed().as_nanos() as u64;
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
        sc.track = if first_of_turn && !no_reconcile() { reconciled(rules, game, scene, track) } else { track };
    }
    if turn_probed(scene) {
        if !first_of_turn {
            return;
        }
        let from = super::record::recorded();
        super::record::teach();
        super::observe::pause_history(true);
        families::reference_turn(rules, &mut copy, &mut sc, expert);
        super::observe::pause_history(false);
        super::record::push_end(&copy, rules);
        super::record::mute();
        if truncate_on() {
            TURN_LABELS.with(|t| *t.borrow_mut() = Some((from, super::record::labels_from(from), 0)));
        }
        EXPERT_TRACK.with(|t| *t.borrow_mut() = Some(sc.track));
        return;
    }
    super::record::probe();
    super::observe::pause_history(true);
    families::reference_turn(rules, &mut copy, &mut sc, expert);
    super::observe::pause_history(false);
    if !super::record::probe_done() {
        super::record::push_end(game, rules);
    }
    // A per-decision probe changes the expert's memory only at the turn's first decision (its plan's start).
    if first_of_turn {
        EXPERT_TRACK.with(|t| *t.borrow_mut() = Some(sc.track));
    }
}

/// The expert's memory was written on its copy of our last turn; the net then played that turn its own way. Its
/// commitments hold only where the real turn carried them out (the cause of DAgger's passive assault: the expert
/// believed it had gone in, `engaged`, in half of its engaged turns while the city was untouched, and kept units
/// "resting" that the net had sent on): `engaged` only once our army has struck the city in a real turn; a unit
/// stays resting only if in the real turn it struck nothing and ended out of the city's strike. `ARMY_DAGGER_NO_RECONCILE=1`: the old carry.
fn reconciled(rules: &CompiledRuleset, game: &GameState, scene: &Scene, mut track: crate::trainers::assault::Track) -> crate::trainers::assault::Track {
    let round = game.view().auth.turn().round;
    let Some(prev) = round.checked_sub(1) else { return track };
    let facts = super::observe::facts_of_round(prev);
    let real_engaged = EXPERT_REAL.with(|r| {
        let mut r = r.borrow_mut();
        *r |= facts.city_struck;
        *r
    });
    if track.engaged && !real_engaged {
        track.engaged = false;
    }
    // A slot is the unit's index in the army (`Orders::army`: the scene's military pieces, in order).
    let army: Vec<kernel::ids::EntityId> = scene.mine.iter().filter(|p| super::in_army(rules, p.kind)).map(|p| p.id).collect();
    let struck: Vec<kernel::ids::EntityId> = facts.struck_slots.iter().filter_map(|s| army.get(*s as usize).copied()).collect();
    let city = scene.target.unwrap_or(scene.centre);
    let in_fire = |id: &kernel::ids::EntityId| scene.mine.iter().find(|p| p.id == *id).and_then(|p| act::alive(game, p, scene.learner)).is_some_and(|c| crate::trainers::lab::dist(game, c.tile, city) <= classic_rules::combat::city::CITY_ATTACK_RANGE);
    track.resting.retain(|id| !struck.contains(id) && !in_fire(id));
    track
}

/// The turns the expert drives (β-mixing): a hash of the scene and the round against ARMY_DAGGER_BETA (default 0).
fn beta_turn(game: &GameState, scene: &Scene) -> bool {
    static BETA: OnceLock<f64> = OnceLock::new();
    let beta = *BETA.get_or_init(|| std::env::var("ARMY_DAGGER_BETA").ok().and_then(|v| v.parse().ok()).unwrap_or(0.0));
    if beta <= 0.0 {
        return false;
    }
    let h = crate::trainers::lab::mix((scene.instance as u64) << 20 ^ u64::from(game.view().auth.turn().round) << 4 ^ scene.fam.seed_id() ^ u64::from(scene.era) << 40);
    (h % 10_000) as f64 / 10_000.0 < beta
}

thread_local! {
    /// The probed turn's labels (ARMY_DAGGER_TRUNCATE=1): where they start in the record, (pointer, action) each,
    /// and how many of them the net has followed so far.
    static TURN_LABELS: std::cell::RefCell<Option<(usize, Vec<(i16, i16)>, usize)>> = const { std::cell::RefCell::new(None) };
}

/// Labels only up to the first divergence (the coordinator's fix (1)): the expert's labels describe its own turn's
/// states; once the net chose otherwise, the later ones are of states the net is not in, and are dropped.
fn truncate_on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("ARMY_DAGGER_TRUNCATE").is_some())
}

/// The net chose (pointer, action) at its k-th decision of the turn: keep the labels while it follows the expert.
fn follow(pointer: i16, action: i16) {
    TURN_LABELS.with(|t| {
        let mut t = t.borrow_mut();
        let Some((from, labels, k)) = t.as_mut() else { return };
        let same = labels.get(*k).is_some_and(|l| *l == (pointer, action));
        if same {
            *k += 1;
        } else {
            // The label of this moment stays (its state is the net's); the rest goes.
            super::record::truncate(*from + *k + 1);
            *t = None;
        }
    });
}

fn no_reconcile() -> bool {
    static NO: OnceLock<bool> = OnceLock::new();
    *NO.get_or_init(|| std::env::var_os("ARMY_DAGGER_NO_RECONCILE").is_some())
}

thread_local! {
    /// Whether our army has struck the city in a real turn of this play (for `reconciled`).
    static EXPERT_REAL: std::cell::RefCell<bool> = const { std::cell::RefCell::new(false) };
}

pub fn net() -> Result<&'static Net, String> {
    // Both sides kept: a net set for the rest of this thread's plays after a load (`set_thread_net`, net/types) wins over the
    // net this thread installed (`install_net`, nn/mlx-army), which wins over the process's.
    if let Some(n) = THREAD_NET.with(|t| t.get()) {
        return Ok(n);
    }
    if let Some(net) = INSTALLED.with(std::cell::Cell::get) {
        return Ok(net);
    }
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

/// The decision trace (`ARMY_NET_DECISION_TRACE=file.jsonl`, info only: the play is unchanged): per decision of the net, the
/// play, the round, the unit (army slot, hp, ranged, distance to the objective, the memory's resting readout in force), the
/// pointer's logits (the unit's, the end's, the best other unit's) and every candidate (kind, ends in a foe city's strike range,
/// distance of its tile to the objective, threat, the change of that distance, logit, a move's moves left share and targets in reach
/// from its tile), the candidate applied.
fn decision_trace_on() -> bool {
    trace_file().is_some()
}

fn trace_file() -> Option<&'static std::sync::Mutex<std::fs::File>> {
    static FILE: OnceLock<Option<std::sync::Mutex<std::fs::File>>> = OnceLock::new();
    FILE.get_or_init(|| std::env::var_os("ARMY_NET_DECISION_TRACE").and_then(|p| std::fs::OpenOptions::new().create(true).append(true).open(p).ok()).map(std::sync::Mutex::new)).as_ref()
}

#[allow(clippy::too_many_arguments)]
fn trace_decision(scene: &Scene, orders: &Orders, tokens: &observe::Tokens, memory: &Option<super::model::MemoryState>, slots: &[i16], (at, ptr, end): (usize, &[Option<f32>], f32), candidates: &[observe::Candidate], scores: &[f32], chosen: i16, core: kernel::world::unit::UnitCore, round: u32) {
    use std::io::Write;
    let Ok(net) = net() else { return };
    let slot = orders.army.iter().position(|a| *a == core.id).map_or(-1, |s| s as i64);
    let rest = memory.as_ref().and_then(|m| net.resting_readout(m)).and_then(|r| usize::try_from(slot).ok().and_then(|s| r.get(s).copied()));
    let rest_u = memory.as_ref().and_then(|m| usize::try_from(slot).ok().map(|s| m.u[s * super::model::U_DIM + 1]));
    let other = ptr.iter().enumerate().filter(|(i, l)| *i != at && l.is_some()).filter_map(|(_, l)| *l).fold(f32::NEG_INFINITY, f32::max);
    let f = &tokens.feat[at];
    let city = (0..tokens.len()).find(|i| tokens.feat[*i][3] > 0.5 && tokens.feat[*i][43] > 0.5).map(|i| tokens.feat[i][7]);
    let cands: Vec<serde_json::Value> = candidates
        .iter()
        .zip(scores)
        .map(|(c, s)| {
            let kind = (0..10).find(|k| c.feat[*k] > 0.5).unwrap_or(9);
            serde_json::json!([kind, c.feat[16], (c.feat[22] * 12.0).round(), (c.feat[19] * 4.0).round(), c.feat[23] * 5.0, s, c.feat[27], (c.feat[29] * 3.0).round()])
        })
        .collect();
    let _ = slots;
    let line = serde_json::json!({
        "fam": scene.fam.name(), "inst": scene.instance, "att": scene.attempt, "era": scene.era,
        "round": round, "slot": slot, "hp": core.hp, "ranged": f[13] > 0.0, "d": (f[6] * 12.0).round(), "fire": f[36] > 0.5,
        "rest": rest, "rest_u": rest_u, "city_hp": city,
        "ptr": ptr[at], "end": end, "other": if other.is_finite() { Some(other) } else { None },
        "cands": cands, "chosen": chosen,
    });
    if let Some(file) = trace_file() {
        if let Ok(mut fh) = file.lock() {
            let _ = writeln!(fh, "{line}");
        }
    }
}

/// One turn of the army: decisions until the net ends it.
pub fn turn(rules: &CompiledRuleset, game: &mut GameState, orders: &Orders, scene: &Scene) -> Result<Timing, String> {
    let net = net()?;
    if net.has_memory() {
        MEMORY.with(|m| {
            let mut m = m.borrow_mut();
            if m.is_none() {
                *m = net.initial_memory().map(|state| MemoryRun { orders: orders.clone(), scene: scene.clone(), state, memory_ns: 0, map: None });
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
        if history_masked() {
            observe::mask_history(&mut tokens);
        }
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
        // β-mixing (DAgger's own rule): in a turn-planned family the expert drives the real turn with probability
        // ARMY_DAGGER_BETA (its plan forms; the net's later turns start from states the plan reaches).
        let pre_track = EXPERT_TRACK.with(|t| t.borrow().clone());
        probe(rules, game, scene, step == 0);
        if step == 0 && super::record::active() && turn_probed(scene) && beta_turn(game, scene) {
            if let Some(expert) = experts().and_then(|e| e.get(scene.fam.name())) {
                let mut sc = scene.clone();
                if let Some(t) = pre_track {
                    sc.track = if no_reconcile() { t } else { reconciled(rules, game, scene, t) };
                }
                families::reference_turn(rules, game, &mut sc, expert);
                EXPERT_TRACK.with(|t| *t.borrow_mut() = Some(sc.track));
                break;
            }
        }
        let t_map = Instant::now();
        if net.has_map() {
            let planes = net.planes(&orders.statics, &orders.statics_rel, &tokens);
            let tm = Instant::now();
            match map.as_mut() {
                None => {
                    map = net.map_full(planes);
                    timing.encode_ns += tm.elapsed().as_nanos() as u64;
                }
                // MB (a net with memory): the map once per turn, from the turn's first moment.
                Some(_) if net.has_memory() => {}
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
        if super::obsdump::wanted() {
            let slots = super::record::slots_of(game, orders, &tokens);
            super::obsdump::moment(rules, game, orders, &tokens, &slots, step, &made);
            super::obsdump::moment_notes(memory.as_ref().and_then(|m| m.notes.as_ref()));
        }
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
            follow(-1, -1);
            super::obsdump::choice(-1, -1);
            break;
        }
        let Some(unit) = tokens.unit[at] else { break };
        let Some(core) = act::core_of(game, unit) else { break };
        let t3 = Instant::now();
        let candidates = observe::candidates(game, rules, orders, unit);
        let t4 = Instant::now();
        let scores = net.actions_x(&body, at, &candidates, map.as_ref(), &tokens, mem_in.as_ref());
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
        let mut chosen = -1i16;
        for i in order {
            chosen = i as i16;
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
        super::obsdump::choice(at as i64, if acted { i64::from(chosen) } else { -1 });
        if decision_trace_on() {
            let round = game.view().auth.turn().round.saturating_sub(orders.start_round);
            trace_decision(scene, orders, &tokens, &memory, &slots, (at, &logits, end), &candidates, &scores, if acted { chosen } else { 0 }, core, round);
        }
        follow(at as i16, if acted { chosen } else { 0 });
    }
    TURN_LABELS.with(|t| *t.borrow_mut() = None);
    if map.is_some() {
        MEMORY.with(|m| {
            if let Some(run) = m.borrow_mut().as_mut() {
                run.map = map.clone();
            }
        });
    }
    // The end: the army stands; the seat's mind plays only what is not the army's.
    let auth = game.view().auth;
    let army: Vec<classic_rules::ids::UnitHandle> = auth.indexes().units_of(orders.side).iter().copied().filter(|h| auth.units().get(*h).is_some_and(|u| orders.army.contains(&u.core.id))).collect();
    for h in army {
        act::stand(game, rules, h);
    }
    Ok(timing)
}
