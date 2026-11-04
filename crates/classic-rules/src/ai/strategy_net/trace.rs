//! The strategy net's trace: one line per review of the goals, per research
//! choice, per culture decision and per production choice that differs from
//! the allocator's, kept on the thread while a tool enables it
//! (`enable`; `ai-strategy-trace`). Off, nothing is formatted. Recording
//! never changes a decision.

use std::cell::{Cell, RefCell};

use super::weights::{AIMS, POSTURES, PRICES, STYLES};
use super::{GoalKind, Plan};
use crate::command::Command;
use crate::ids::{ProductionItem, TechId};
use crate::rules::CompiledRuleset;

thread_local! {
    static ON: Cell<bool> = const { Cell::new(false) };
    static LINES: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

pub fn enable(on: bool) {
    ON.with(|c| c.set(on));
}

pub fn enabled() -> bool {
    ON.with(|c| c.get())
}

/// The lines recorded since the last call.
pub fn take() -> Vec<String> {
    LINES.with(|l| std::mem::take(&mut *l.borrow_mut()))
}

fn push(line: String) {
    LINES.with(|l| l.borrow_mut().push(line));
}

fn short(key: &str) -> &str {
    key.rsplit('/').next().unwrap_or(key)
}

pub fn tech_name(rules: &CompiledRuleset, t: TechId) -> String {
    rules.technologies.get(usize::from(t.0)).map_or_else(|| format!("tech{}", t.0), |d| short(&d.key).to_owned())
}

/// A goal in words.
pub fn goal_name(rules: &CompiledRuleset, kind: GoalKind) -> String {
    match kind {
        GoalKind::Wonder { building, city } => format!("wonder:{}@{city}", rules.buildings.get(usize::from(building.0)).map_or("?", |d| short(&d.key))),
        GoalKind::Building(b) => format!("building:{}", rules.buildings.get(usize::from(b.0)).map_or("?", |d| short(&d.key))),
        GoalKind::Branch(b) => format!("branch:{}", rules.module.policy_branches.get(usize::from(b.0)).map_or("?", |d| short(&d.key))),
        GoalKind::Tech(t) => format!("tech:{}", tech_name(rules, t)),
    }
}

fn item_name(rules: &CompiledRuleset, item: ProductionItem) -> String {
    match item {
        ProductionItem::Unit(k) => rules.units.get(usize::from(k.0)).map_or("?", |d| short(&d.key)).to_owned(),
        ProductionItem::Building(b) => rules.buildings.get(usize::from(b.0)).map_or("?", |d| short(&d.key)).to_owned(),
        ProductionItem::Project(p) => format!("project{}", p.0),
    }
}

/// The plan of a turn (a line on review turns only).
pub fn turn(player: u8, plan: &Plan, reviewed: bool, rules: &CompiledRuleset) {
    if !enabled() || !reviewed {
        return;
    }
    let i = &plan.intent;
    let shares = |names: &[&str], v: &[i64]| names.iter().zip(v).map(|(n, x)| format!("{n}={:.2}", *x as f64 / 1000.0)).collect::<Vec<_>>().join(" ");
    let mult: Vec<String> = PRICES.iter().zip(plan.mult.iter()).filter(|(_, m)| **m != 1000).map(|(n, m)| format!("{n}×{:.2}", *m as f64 / 1000.0)).collect();
    let goals: Vec<String> = plan.goals.iter().map(|g| format!("{} (worth {}, {} rounds)", goal_name(rules, g.kind), g.worth / 100, g.turns)).collect();
    push(format!(
        "r{} p{} REVIEW style[{}] → {} | aim[{}] → {} | posture[{}] → {} | cities → {} | H {} | prices {} | goals: {} | target {}",
        plan.round,
        player,
        shares(&STYLES, &i.style),
        STYLES[i.style_pick],
        shares(&AIMS, &i.aim),
        AIMS[i.aim_pick],
        shares(&POSTURES, &i.posture),
        POSTURES[i.posture_pick],
        plan.city_target,
        i.horizon,
        mult.join(" "),
        goals.join("; "),
        plan.target.map_or("-".to_owned(), |t| tech_name(rules, t)),
    ));
}

/// The style locked.
pub fn lock(player: u8, round: u32, intent: &super::Intent) {
    if !enabled() {
        return;
    }
    let shares: Vec<String> = STYLES.iter().zip(intent.style).map(|(n, x)| format!("{n}={:.2}", x as f64 / 1000.0)).collect();
    push(format!("r{round} p{player} LOCK style {} [{}]", STYLES[intent.style_pick], shares.join(" ")));
}

/// The held style changed.
pub fn style_change(player: u8, round: u32, from: usize, intent: &super::Intent) {
    if !enabled() {
        return;
    }
    push(format!("r{round} p{player} STYLE {} → {} (locked: {})", STYLES[from], STYLES[intent.style_pick], intent.locked));
}

pub fn research(player: u8, round: u32, tech: TechId, scores: &[super::heads::TechScore], rules: &CompiledRuleset) {
    if !enabled() {
        return;
    }
    let s = scores.iter().find(|s| s.tech == tech);
    push(format!(
        "r{round} p{player} RESEARCH {} (own {}, goal {}, {} turns; of {} available)",
        tech_name(rules, tech),
        s.map_or(0, |s| s.own / 100),
        s.map_or(0, |s| s.goal / 100),
        s.map_or(0, |s| s.turns),
        scores.len()
    ));
}

pub fn policy(player: u8, round: u32, command: &Command, rules: &CompiledRuleset) {
    if !enabled() {
        return;
    }
    let text = match command {
        Command::AdoptPolicy { policy, .. } => format!("adopt {}", rules.module.policies.get(usize::from(policy.0)).map_or("?", |d| d.key.as_str())),
        Command::UnlockPolicyBranch { branch, .. } => format!("open {}", rules.module.policy_branches.get(usize::from(branch.0)).map_or("?", |d| d.key.as_str())),
        Command::ChooseIdeology { ideology, .. } => format!("ideology {}", rules.module.policy_branches.get(usize::from(ideology.0)).map_or("?", |d| d.key.as_str())),
        other => format!("{other:?}"),
    };
    push(format!("r{round} p{player} POLICY {text}"));
}

/// A goal wonder of a city the allocator does not offer it.
pub fn missing(player: u8, round: u32, city: u64, building: crate::ids::BuildingKindId, rules: &CompiledRuleset) {
    if !enabled() {
        return;
    }
    push(format!("r{round} p{player} GOALMISS city {city}: {} not among the allocator's orders", item_name(rules, ProductionItem::Building(building))));
}

pub fn production(player: u8, round: u32, city: u64, item: ProductionItem, allocator: Option<ProductionItem>, rules: &CompiledRuleset) {
    if !enabled() || Some(item) == allocator {
        return;
    }
    push(format!(
        "r{round} p{player} BUILD city {city}: {} (allocator: {})",
        item_name(rules, item),
        allocator.map_or("-".to_owned(), |a| item_name(rules, a))
    ));
}
