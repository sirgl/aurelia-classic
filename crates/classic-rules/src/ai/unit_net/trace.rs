//! The unit net's trace (`AURELIA_UNIT_NET_TRACE`: a player id, or `all`):
//! the operations and prices at the start of a turn, each plan made or
//! updated (the targets with their values and the key, the assignments, the
//! queue, the expected outcome), each "who is next" answer and each
//! decision with its row, concepts and motives, on standard error. Never
//! read by a decision.

use super::commander::{Op, Order, TurnPlan};
use super::tactician::Verdict;
use super::weights::{prices::POSTURES, MOTIVES, TABLE};
use super::Prices;
use crate::ai::planner::TurnBrief;
use crate::ids::EntityId;
use crate::knowledge::observation::OwnUnit;

/// The environment variable that turns the trace on.
pub const ENV: &str = "AURELIA_UNIT_NET_TRACE";

fn on(brief: &TurnBrief) -> bool {
    thread_local! {
        static WHO: Option<String> = std::env::var(ENV).ok().filter(|v| !v.is_empty());
    }
    WHO.with(|w| match w.as_deref() {
        None => false,
        Some("all") => true,
        Some(v) => v.split(',').any(|p| p.trim().parse::<u8>().ok() == Some(brief.player.0)),
    })
}

pub fn turn(brief: &TurnBrief, prices: &Prices, ops: &[Op], orders: &[Order]) {
    if !on(brief) {
        return;
    }
    eprintln!("unit-net r{} p{} posture {} (kill {} city {} damage {} unit {} risk {})", brief.round, brief.player.0, POSTURES[prices.posture].name, prices.kill, prices.city, prices.damage, prices.unit, prices.risk);
    for op in ops {
        eprintln!(
            "  op {:?} target {} rally {} phase {} since r{} members {} readiness {} rho {} p_take {} options {:?} choice {}",
            op.kind,
            op.target.0,
            op.rally.0,
            op.phase.name(),
            op.since,
            op.members.len(),
            op.readiness,
            op.rho,
            op.p_take,
            op.options,
            super::commander::OPTIONS[op.choice.min(3)]
        );
    }
    for o in orders {
        eprintln!("  order: a defender near city {} by r{} worth {}", o.city.0, o.round, o.value);
    }
}

pub fn plan(brief: &TurnBrief, plan: &TurnPlan, why: &str) {
    if !on(brief) {
        return;
    }
    eprintln!("  plan r{} p{} ({why}, update {}): expected kills {} captures {} losses {} dealt {}; reply rounds {}", plan.round, brief.player.0, plan.updates, plan.expected.kills, plan.expected.captures, plan.expected.losses, plan.expected.dealt, plan.reply_moves);
    for t in &plan.targets {
        eprintln!("    target {} {} hp {} V {}{} assigned {:?} P(kill) {}", t.tile.0, if t.city { "city" } else { "unit" }, t.hp, t.value, if t.key { " KEY" } else { "" }, t.assigned, t.p_kill);
    }
    for a in &plan.assignments {
        eprintln!("    unit {} {} {:?} gain {}", a.unit.0, a.task.name(), a.task, a.gain);
    }
    eprintln!("    queue {:?} spent {:?}", plan.queue.iter().map(|u| u.0).collect::<Vec<_>>(), plan.spent.iter().map(|u| u.0).collect::<Vec<_>>());
}

pub fn next(brief: &TurnBrief, head: EntityId, next: Option<EntityId>, p: &TurnPlan) {
    if !on(brief) {
        return;
    }
    eprintln!("  who is next: head {} → {}", head.0, next.map_or("head".to_owned(), |n| n.0.to_string()));
    plan(brief, p, "fast update");
}

pub fn decision(brief: &TurnBrief, unit: &OwnUnit, v: &Verdict, p: Option<&TurnPlan>) {
    if !on(brief) {
        return;
    }
    let c = &v.chosen;
    let motives: Vec<String> = MOTIVES.iter().zip(c.motives).filter(|(_, m)| *m != 0).map(|(n, m)| format!("{n} {m}")).collect();
    eprintln!(
        "  unit {} at {} hp {} row «{}» {:?} → {:?} dest {} score {} (assigned {}) [{}] of {} candidates; task {:?}",
        unit.id.0,
        unit.tile.0,
        unit.hp,
        TABLE[v.row].name,
        v.concepts,
        c.command,
        c.dest.0,
        c.score,
        c.assigned,
        motives.join(", "),
        v.candidates,
        p.and_then(|p| p.task_of(unit.id))
    );
}
