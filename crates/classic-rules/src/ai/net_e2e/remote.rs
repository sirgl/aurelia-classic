//! The remote player (classic/docs/ai-net-model.md, «Remote MPS player»): a
//! [`Scorer`] whose net runs in another process, a float checkpoint on the
//! GPU served by `nn/aurelia_nn/remote.py` over a Unix socket. It sends the
//! tables the int8 player reads (`TurnInput`, each decision's candidates) and
//! gets back the value and the logits (Q8, as the int8 net's); the memory and
//! the turn trace live on the server, keyed by this scorer's connection and
//! the player's session (`TurnCache::session`).
//!
//! `NetPlayer::from_path("remote:SOCKET[,log=FILE][,check=W.bin]")`, so the
//! bench's `net:remote:SOCKET` (also `net-remote:SOCKET`), `ai-selfplay --net
//! remote:SOCKET` and the E2 loop all play it. `log=` appends a line of the
//! scorer's counters (requests, round-trip time) when it is dropped (the end
//! of a game); `check=` plays the int8 weights file and asks the server the
//! same decisions alongside, counting how often both take the same
//! candidate ([`Checked`]).
//!
//! One connection per scorer (one per game on the bench: `ClassicBench::mind`
//! makes a player a seat), one request at a time; the server batches the
//! requests of the games that wait together.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use crate::ai::net_e2e::encode::{head, Candidate};
use crate::ai::net_e2e::mind::{ModelScorer, Scorer};
use crate::ai::net_e2e::model::{fnv1a64, Memory, TurnCache, TurnInput};

/// The prefix of a remote weights spec (`remote:SOCKET`).
pub const PREFIX: &str = "remote:";

/// The wire's version (`remote.py` `VERSION`).
const VERSION: u32 = 1;
const HELLO: u8 = 1;
const BEGIN: u8 = 2;
const SCORE: u8 = 3;

/// What the server said it serves.
#[derive(Debug, Clone)]
pub struct Hello {
    pub layout_hash: u64,
    pub heads: usize,
    pub defer: bool,
    pub coarse: bool,
    pub events: bool,
    pub c_tile: usize,
    pub c_global: usize,
    pub c_entity: usize,
    pub c_candidate: usize,
    pub c_event: usize,
    pub label: String,
}

/// Round trips and their time.
#[derive(Debug, Clone, Copy, Default)]
pub struct Counters {
    pub begins: u64,
    pub begin_ns: u64,
    pub scores: u64,
    pub score_ns: u64,
    pub candidates: u64,
    pub bytes_sent: u64,
}

#[derive(Debug)]
struct Conn {
    stream: UnixStream,
    /// Map keys whose tables the server has from this connection.
    maps: Vec<u64>,
    next_session: u64,
    /// Per session: the last decision's candidates (rows and meta) and the
    /// index taken, sent with the session's next SCORE (the turn trace).
    last: BTreeMap<u64, (Vec<(Vec<i16>, [i32; 4])>, i32)>,
    counters: Counters,
    buf: Vec<u8>,
}

impl Conn {
    fn call(&mut self, payload: &[u8]) -> Result<Vec<u8>, String> {
        self.stream.write_all(&(payload.len() as u32).to_le_bytes()).and_then(|_| self.stream.write_all(payload)).map_err(|e| format!("the remote net: {e}"))?;
        self.counters.bytes_sent += 4 + payload.len() as u64;
        let mut len = [0u8; 4];
        self.stream.read_exact(&mut len).map_err(|e| format!("the remote net: {e}"))?;
        let mut body = vec![0u8; u32::from_le_bytes(len) as usize];
        self.stream.read_exact(&mut body).map_err(|e| format!("the remote net: {e}"))?;
        match body.first() {
            Some(0) => Ok(body.split_off(1)),
            _ => Err(format!("the remote net: {}", String::from_utf8_lossy(body.get(1..).unwrap_or_default()))),
        }
    }
}

/// The scorer of a remote net.
#[derive(Debug)]
pub struct RemoteScorer {
    socket: PathBuf,
    hello: Hello,
    conn: Mutex<Conn>,
    log: Option<PathBuf>,
}

struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        if self.0.len() < n {
            return Err("the remote net: a short reply".to_owned());
        }
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Ok(head)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().expect("4 bytes")))
    }
    fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().expect("8 bytes")))
    }
}

fn put_i16s(out: &mut Vec<u8>, v: &[i16]) {
    out.reserve(2 * v.len());
    v.iter().for_each(|x| out.extend_from_slice(&x.to_le_bytes()));
}

fn put_i32s(out: &mut Vec<u8>, v: &[i32]) {
    out.reserve(4 * v.len());
    v.iter().for_each(|x| out.extend_from_slice(&x.to_le_bytes()));
}

/// The map key of a turn's neighbour and region tables.
fn map_key(input: &TurnInput<'_>) -> u64 {
    let mut bytes = Vec::with_capacity(4 * (input.neighbors.len() + input.region.len()));
    put_i32s(&mut bytes, input.neighbors);
    put_i32s(&mut bytes, input.region);
    fnv1a64(&bytes)
}

impl RemoteScorer {
    /// Connects to the server at `socket` and asks what it serves.
    pub fn connect(socket: &str, log: Option<PathBuf>) -> Result<Self, String> {
        let stream = UnixStream::connect(socket).map_err(|e| format!("the remote net at {socket}: {e} (is `python -m aurelia_nn.remote serve` running?)"))?;
        let mut conn = Conn { stream, maps: Vec::new(), next_session: 1, last: BTreeMap::new(), counters: Counters::default(), buf: Vec::new() };
        let mut req = vec![HELLO];
        req.extend_from_slice(&VERSION.to_le_bytes());
        let reply = conn.call(&req)?;
        let mut r = Reader(&reply);
        let version = r.u32()?;
        if version != VERSION {
            return Err(format!("the remote net at {socket} speaks protocol {version}, this build {VERSION}"));
        }
        let layout_hash = r.u64()?;
        let heads = r.u32()? as usize;
        let (defer, coarse, events) = (r.u8()? != 0, r.u8()? != 0, r.u8()? != 0);
        let mut c = [0usize; 5];
        for x in &mut c {
            *x = r.u32()? as usize;
        }
        let n = r.u32()? as usize;
        let label = String::from_utf8_lossy(r.take(n)?).into_owned();
        if coarse {
            return Err(format!("the remote net at {socket}: coarse regions are not sent"));
        }
        let hello = Hello { layout_hash, heads, defer, coarse, events, c_tile: c[0], c_global: c[1], c_entity: c[2], c_candidate: c[3], c_event: c[4], label };
        Ok(Self { socket: PathBuf::from(socket), hello, conn: Mutex::new(conn), log })
    }

    pub fn hello(&self) -> &Hello {
        &self.hello
    }

    pub fn counters(&self) -> Counters {
        self.lock().counters
    }

    fn lock(&self) -> MutexGuard<'_, Conn> {
        self.conn.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Drop for RemoteScorer {
    fn drop(&mut self) {
        let Some(path) = &self.log else { return };
        let c = self.lock().counters;
        let line = format!(
            "{{\"socket\":{:?},\"label\":{:?},\"begins\":{},\"begin_ms\":{:.3},\"scores\":{},\"score_ms\":{:.3},\"candidates\":{},\"bytes_sent\":{}}}\n",
            self.socket.display().to_string(),
            self.hello.label,
            c.begins,
            c.begin_ns as f64 / 1e6,
            c.scores,
            c.score_ns as f64 / 1e6,
            c.candidates,
            c.bytes_sent
        );
        append(path, &line);
    }
}

/// Appends a line to a file (several processes and threads at once: one write).
fn append(path: &PathBuf, line: &str) {
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(line.as_bytes());
    }
}

impl Scorer for RemoteScorer {
    fn label(&self) -> String {
        format!("{PREFIX}{} ({})", self.socket.display(), self.hello.label)
    }

    fn memory(&self) -> Memory {
        Memory::default()
    }

    fn has_head(&self, head: i32) -> bool {
        usize::try_from(head).is_ok_and(|h| h < self.hello.heads)
    }

    fn layout_hash(&self) -> Option<u64> {
        Some(self.hello.layout_hash)
    }

    fn defers(&self) -> bool {
        self.hello.defer
    }

    fn begin_turn(&self, input: &TurnInput<'_>, _memory: &Memory, cache: &mut TurnCache) {
        let start = Instant::now();
        let mut conn = self.lock();
        let conn = &mut *conn;
        if cache.session == 0 {
            cache.session = conn.next_session;
            conn.next_session += 1;
        }
        let h = &self.hello;
        let tiles = input.region.len();
        let entities = input.entity_ids.len();
        let events = if h.events { input.event_meta.len() / 4 } else { 0 };
        let key = map_key(input);
        let has_map = !conn.maps.contains(&key);
        let mut req = std::mem::take(&mut conn.buf);
        req.clear();
        req.push(BEGIN);
        req.extend_from_slice(&cache.session.to_le_bytes());
        req.extend_from_slice(&key.to_le_bytes());
        req.extend_from_slice(&(tiles as u32).to_le_bytes());
        req.push(u8::from(has_map));
        if has_map {
            put_i32s(&mut req, input.neighbors);
            put_i32s(&mut req, input.region);
        }
        assert_eq!(input.tiles.len(), tiles * h.c_tile, "the remote net's tile columns");
        put_i16s(&mut req, input.tiles);
        assert_eq!(input.global.len(), h.c_global, "the remote net's global columns");
        put_i16s(&mut req, input.global);
        req.extend_from_slice(&(entities as u32).to_le_bytes());
        assert_eq!(input.entities.len(), entities * h.c_entity, "the remote net's entity columns");
        put_i16s(&mut req, input.entities);
        put_i32s(&mut req, input.entity_meta);
        put_i32s(&mut req, input.entity_ids);
        req.extend_from_slice(&(events as u32).to_le_bytes());
        if events > 0 {
            put_i16s(&mut req, &input.events[..events * h.c_event]);
            put_i32s(&mut req, &input.event_meta[..events * 4]);
        }
        let reply = conn.call(&req).unwrap_or_else(|e| panic!("{e}"));
        conn.buf = req;
        if has_map {
            conn.maps.push(key);
        }
        let value = i32::from_le_bytes(reply.get(..4).and_then(|b| b.try_into().ok()).expect("the remote net's value"));
        cache.set_value(value);
        conn.last.insert(cache.session, (Vec::new(), -1));
        conn.counters.begins += 1;
        conn.counters.begin_ns += start.elapsed().as_nanos() as u64;
    }

    fn score(&self, cache: &TurnCache, head: usize, actor: i32, candidates: &[Candidate], out: &mut Vec<i32>) {
        out.clear();
        if candidates.is_empty() {
            return;
        }
        let start = Instant::now();
        let mut conn = self.lock();
        let conn = &mut *conn;
        let width = self.hello.c_candidate;
        let note = conn.last.get(&cache.session).map_or(-1, |(_, at)| *at);
        let mut req = std::mem::take(&mut conn.buf);
        req.clear();
        req.push(SCORE);
        req.extend_from_slice(&cache.session.to_le_bytes());
        req.extend_from_slice(&note.to_le_bytes());
        req.extend_from_slice(&(head as u32).to_le_bytes());
        req.extend_from_slice(&actor.to_le_bytes());
        req.extend_from_slice(&(candidates.len() as u32).to_le_bytes());
        for c in candidates {
            // A net of an older format reads the prefix of the row it knows.
            let n = width.min(c.row.len());
            put_i16s(&mut req, &c.row[..n]);
            req.resize(req.len() + 2 * (width - n), 0);
        }
        for c in candidates {
            put_i32s(&mut req, &c.meta);
        }
        let reply = conn.call(&req).unwrap_or_else(|e| panic!("{e}"));
        conn.buf = req;
        assert_eq!(reply.len(), 4 * candidates.len(), "the remote net's logits");
        out.extend(reply.chunks_exact(4).map(|b| i32::from_le_bytes(b.try_into().expect("4 bytes"))));
        let listed = candidates.iter().map(|c| (c.row[..width.min(c.row.len())].to_vec(), c.meta)).collect();
        conn.last.insert(cache.session, (listed, -1));
        conn.counters.scores += 1;
        conn.counters.candidates += candidates.len() as u64;
        conn.counters.score_ns += start.elapsed().as_nanos() as u64;
    }

    fn end_turn(&self, _cache: &TurnCache, memory: &Memory) -> Memory {
        memory.clone()
    }

    fn note_choice(&self, cache: &mut TurnCache, _head: usize, _actor: i32, candidate: &Candidate) {
        let mut conn = self.lock();
        let width = self.hello.c_candidate;
        if let Some((listed, at)) = conn.last.get_mut(&cache.session) {
            let row = &candidate.row[..width.min(candidate.row.len())];
            *at = listed.iter().position(|(r, m)| *m == candidate.meta && r[..] == *row).map_or(-1, |i| i as i32);
        }
    }
}

/// The int8 weights file playing while the remote net is asked the same
/// decisions (`remote:SOCKET,check=W.bin`): how often both nets take the
/// same candidate (the argmax, ties to the first), per head. The remote's
/// turn trace follows the int8 player's choices, as a recorded game's
/// labels force the trainer's.
#[derive(Debug)]
pub struct Checked {
    int8: ModelScorer,
    remote: RemoteScorer,
    /// Per head (`encode::head`): decisions, same argmax.
    counts: Mutex<[[u64; 2]; 8]>,
    /// Where the two disagree, how much the remote net prefers its pick to
    /// the int8's (its logits, nats): below 0.1, 0.3, 1, and above; and the
    /// sum of |Δ| of the mean-centred logits over all candidates (Q8), their
    /// count.
    gaps: Mutex<([u64; 4], f64, u64)>,
    log: Option<PathBuf>,
    scratch: Mutex<Vec<i32>>,
}

impl Checked {
    pub fn new(int8: ModelScorer, remote: RemoteScorer, log: Option<PathBuf>) -> Result<Self, String> {
        let theirs = remote.hello.layout_hash;
        match int8.layout_hash() {
            Some(ours) if ours != theirs => Err(format!("check: the int8 file's layout {ours:#018x}, the remote net's {theirs:#018x}")),
            _ => Ok(Self { int8, remote, counts: Mutex::new([[0; 2]; 8]), gaps: Mutex::new(([0; 4], 0.0, 0)), log, scratch: Mutex::new(Vec::new()) }),
        }
    }

    /// Per head: decisions, and how many the two nets agree on.
    pub fn counts(&self) -> [[u64; 2]; 8] {
        *self.counts.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn argmax(v: &[i32]) -> Option<usize> {
    (0..v.len()).fold(None, |best: Option<usize>, i| match best {
        Some(b) if v[b] >= v[i] => Some(b),
        _ => Some(i),
    })
}

impl Drop for Checked {
    fn drop(&mut self) {
        let counts = self.counts();
        let per: Vec<String> = counts
            .iter()
            .enumerate()
            .filter(|(_, c)| c[0] > 0)
            .map(|(h, c)| format!("\"{}\":[{},{}]", head::NAMES.get(h).copied().unwrap_or("?"), c[1], c[0]))
            .collect();
        let (gaps, abs, n) = *self.gaps.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let line = format!(
            "{{\"check\":{:?},\"agree_of\":{{{}}},\"disagree_gap_lt\":{{\"0.1\":{},\"0.3\":{},\"1\":{},\"more\":{}}},\"mean_abs_centred_diff\":{:.4}}}\n",
            self.int8.label(),
            per.join(","),
            gaps[0],
            gaps[1],
            gaps[2],
            gaps[3],
            abs / n.max(1) as f64 / 256.0
        );
        match &self.log {
            Some(path) => append(path, &line),
            None => eprint!("{line}"),
        }
    }
}

impl Scorer for Checked {
    fn label(&self) -> String {
        format!("{} checked against {}", self.int8.label(), self.remote.label())
    }
    fn memory(&self) -> Memory {
        self.int8.memory()
    }
    fn coarse(&self) -> bool {
        self.int8.coarse()
    }
    fn has_head(&self, head: i32) -> bool {
        self.int8.has_head(head)
    }
    fn layout_hash(&self) -> Option<u64> {
        self.int8.layout_hash()
    }
    fn memory_widths(&self) -> [usize; 3] {
        self.int8.memory_widths()
    }
    fn defers(&self) -> bool {
        self.int8.defers()
    }
    fn begin_turn(&self, input: &TurnInput<'_>, memory: &Memory, cache: &mut TurnCache) {
        self.int8.begin_turn(input, memory, cache);
        let value = cache.value();
        self.remote.begin_turn(input, memory, cache);
        cache.set_value(value);
    }
    fn score(&self, cache: &TurnCache, head: usize, actor: i32, candidates: &[Candidate], out: &mut Vec<i32>) {
        self.int8.score(cache, head, actor, candidates, out);
        let mut theirs = self.scratch.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        self.remote.score(cache, head, actor, candidates, &mut theirs);
        if let (Some(a), Some(b)) = (argmax(out), argmax(&theirs)) {
            let mut counts = self.counts.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let c = &mut counts[head.min(7)];
            c[0] += 1;
            c[1] += u64::from(a == b);
            let mut gaps = self.gaps.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if a != b {
                let gap = f64::from(theirs[b] - theirs[a]) / 256.0;
                gaps.0[[0.1, 0.3, 1.0].iter().position(|t| gap < *t).unwrap_or(3)] += 1;
            }
            let mean = |v: &[i32]| v.iter().map(|x| f64::from(*x)).sum::<f64>() / v.len() as f64;
            let (ma, mb) = (mean(out), mean(&theirs));
            gaps.1 += out.iter().zip(theirs.iter()).map(|(x, y)| ((f64::from(*x) - ma) - (f64::from(*y) - mb)).abs()).sum::<f64>();
            gaps.2 += out.len() as u64;
        }
    }
    fn end_turn(&self, cache: &TurnCache, memory: &Memory) -> Memory {
        self.int8.end_turn(cache, memory)
    }
    fn note_choice(&self, cache: &mut TurnCache, head: usize, actor: i32, candidate: &Candidate) {
        self.int8.note_choice(cache, head, actor, candidate);
        self.remote.note_choice(cache, head, actor, candidate);
    }
}

/// The scorer of a spec `SOCKET[,log=FILE][,check=W.bin]` (after
/// [`PREFIX`]).
pub fn scorer(spec: &str) -> Result<Arc<dyn Scorer>, String> {
    let mut parts = spec.split(',');
    let socket = parts.next().filter(|s| !s.is_empty()).ok_or_else(|| format!("{PREFIX}{spec}: no socket"))?;
    let (mut log, mut check) = (None, None);
    for part in parts {
        match part.split_once('=') {
            Some(("log", v)) => log = Some(PathBuf::from(v)),
            Some(("check", v)) => check = Some(v.to_owned()),
            _ => return Err(format!("{PREFIX}{spec}: `{part}` is no `log=FILE` or `check=W.bin`")),
        }
    }
    let remote = RemoteScorer::connect(socket, log.clone())?;
    Ok(match check {
        Some(path) => Arc::new(Checked::new(ModelScorer::load(std::path::Path::new(&path))?, remote, log)?),
        None => Arc::new(remote),
    })
}
