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

/// DAgger: the expert's label for this moment (`record::probe` on a copy of the game).
fn probe(rules: &CompiledRuleset, game: &GameState, scene: &Scene) {
    let Some(expert) = experts().and_then(|e| e.get(scene.fam.name())) else { return };
    if !super::record::active() {
        return;
    }
    let mut copy = game.clone();
    let mut sc = scene.clone();
    super::record::probe();
    families::reference_turn(rules, &mut copy, &mut sc, expert);
    if !super::record::probe_done() {
        super::record::push_end(game, rules);
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
    let mut timing = Timing::default();
    let mut made: Vec<(kernel::ids::EntityId, u32)> = Vec::new();
    let mut map: Option<super::model::MapState> = None;
    for _ in 0..PER_TURN {
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
        probe(rules, game, scene);
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
        let body = net.body(&tokens, map.as_ref());
        let (logits, end) = net.pointer(&body, &tokens);
        let t2 = Instant::now();
        let pick = logits.iter().enumerate().filter_map(|(i, l)| l.map(|l| (i, l))).fold(None::<(usize, f32)>, |best, (i, l)| if best.is_none_or(|b| l > b.1) { Some((i, l)) } else { best });
        timing.observe_ns += (t1 - t0).as_nanos() as u64;
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
