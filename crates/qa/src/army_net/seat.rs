//! The player `army-net` of `ai-exam suite` (DESIGN.md, «Play»): on the learner's turn the net picks
//! a unit of the army (or the end), then one of its legal actions, the game applies it, and the net
//! looks again; when it ends, every army unit stands, so the seat's mind (`standard`) plays only the
//! rest of the seat (the civilians, the home city). Greedy: the highest logit wins; ties by order.
//! The weights come from `ARMY_NET` (a file of `army/export.py`), loaded once per process.

use std::sync::OnceLock;
use std::time::Instant;

use classic_rules::{CompiledRuleset, GameState};

use super::model::Net;
use super::observe::{self, Action};
use super::Orders;
use crate::trainers::act;

/// Decisions of one unit in one turn at most (a unit that keeps moving back and forth is stopped).
const PER_UNIT: u32 = 6;
/// Decisions of one turn at most.
const PER_TURN: u32 = 96;

static NET: OnceLock<Result<Net, String>> = OnceLock::new();

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
    /// The net's forward passes.
    pub forward_ns: u64,
}

/// One turn of the army: decisions until the net ends it.
pub fn turn(rules: &CompiledRuleset, game: &mut GameState, orders: &Orders) -> Result<Timing, String> {
    let net = net()?;
    let mut timing = Timing::default();
    let mut made: Vec<(kernel::ids::EntityId, u32)> = Vec::new();
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
        let t1 = Instant::now();
        let body = net.body(&tokens);
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
        let scores = net.actions(&body, at, &candidates);
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
