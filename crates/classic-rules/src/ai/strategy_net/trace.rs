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
    /// The cell's food price over the allocator's before its clamps (`weights::FOOD_MIN`,
    /// `FOOD_MAX`), thousandths, of the review being traced.
    static FOOD_RAW: Cell<Option<i64>> = const { Cell::new(None) };
}

/// The cell's food price over the allocator's before the clamps (`goals::review`): printed on
/// the review's line.
pub fn food_raw(raw: i64) {
    if enabled() {
        FOOD_RAW.with(|c| c.set(Some(raw)));
    }
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

pub fn push(line: String) {
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
    // The food price before its clamps and every feature the net read (thousandths, in the order
    // of `weights::FEATURES`).
    let raw = FOOD_RAW.with(|c| c.take());
    let features: Vec<String> = plan.features.iter().map(|v| v.to_string()).collect();
    LINES.with(|l| {
        if let Some(last) = l.borrow_mut().last_mut() {
            last.push_str(&format!(" | food_raw {} | f[{}]", raw.map_or("-".to_owned(), |r| format!("{:.2}", r as f64 / 1000.0)), features.join(",")));
        }
    });
    // `war-open`'s features and its window (none of the other variants has them).
    use super::weights::{fe, kc};
    let open = [fe::OPPORTUNITY, fe::OP_FORMING, fe::STRENGTH, fe::PAYS].map(|f| plan.features[f]);
    if open.iter().any(|v| *v != 0) {
        LINES.with(|l| {
            if let Some(last) = l.borrow_mut().last_mut() {
                last.push_str(&format!(
                    " | war opportunity={:.2} forming={:.2} strength={:.2} window={:.2} pays={:.0}",
                    open[0] as f64 / 1000.0,
                    open[1] as f64 / 1000.0,
                    open[2] as f64 / 1000.0,
                    i.concepts[kc::STRIKE_WINDOW] as f64 / 1000.0,
                    open[3] as f64 / 1000.0
                ));
            }
        });
    }
}

/// The diplomatic standing at a review: the grand strategy's path, the city-states we and the
/// best-allied rival lead, the gold, the World Congress (trace only).
pub fn diplo(obs: &crate::knowledge::observation::AIObservation<'_>, state: &crate::ai::state::AiState, intent: &super::Intent) {
    if !enabled() {
        return;
    }
    let ours = obs.city_states.iter().filter(|c| c.ally == Some(obs.player)).count();
    let mut rivals: Vec<u8> = obs.city_states.iter().filter_map(|c| c.ally).filter(|p| *p != obs.player).map(|p| p.0).collect();
    rivals.sort_unstable();
    let best = rivals.chunk_by(|a, b| a == b).map(<[u8]>::len).max().unwrap_or(0);
    let congress = obs.congress.as_ref().map_or_else(|| "-".to_owned(), |c| format!("un {} needed {}", c.united_nations, c.needed));
    let ids: Vec<String> = obs.city_states.iter().filter(|c| c.ally == Some(obs.player)).map(|c| c.player.0.to_string()).collect();
    push(format!(
        "r{} p{} DIPLO path {} aim {} tall_real {} | cs allies {} of {} (best rival {}) | gold {} treasury {} | congress {} | ids {}",
        obs.round,
        obs.player.0,
        crate::ai::grand_strategy::path_of(state).name(),
        AIMS[intent.aim_pick],
        intent.tall_real,
        ours,
        obs.city_states.len(),
        best,
        obs.economy.income.gold,
        obs.economy.treasury,
        congress,
        if ids.is_empty() { "-".to_owned() } else { ids.join(",") }
    ));
}

/// A gold gift the allocator bought, with the city-state's ally and our influence before it
/// (trace only; every variant that prices gifts, `PriceModel::Uses`).
pub fn gift(obs: &crate::knowledge::observation::AIObservation<'_>, city_state: crate::ids::PlayerId, gold: u32, value: i64) {
    if !enabled() {
        return;
    }
    let info = obs.city_states.iter().find(|c| c.player == city_state);
    let ally = info.and_then(|c| c.ally).map_or_else(|| "-".to_owned(), |p| p.0.to_string());
    push(format!(
        "r{} p{} GIFT cs {} gold {} value {} ally {} influence {} treasury {}",
        obs.round,
        obs.player.0,
        city_state.0,
        gold,
        value,
        ally,
        info.map_or(0, |c| c.influence),
        obs.economy.treasury
    ));
}

/// Our delegates cast for ourselves in a World Leader vote, against the delegates it needs and
/// the proposals they are split over (trace only).
pub fn leader_vote(obs: &crate::knowledge::observation::AIObservation<'_>, cast: u16, all: u16, needed: u16, proposals: usize) {
    if !enabled() {
        return;
    }
    push(format!("r{} p{} VOTE leader cast {cast} of {all} needed {needed} proposals {proposals}", obs.round, obs.player.0));
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

/// The diplomatic race at a round of the United Nations (trace only, the measurements of the
/// vote guard): our delegates, the known rivals', the vote's date, our gold and the alliances
/// with our influence in each.
pub fn watch(obs: &crate::knowledge::observation::AIObservation<'_>) {
    if !enabled() {
        return;
    }
    let Some(c) = obs.congress.as_ref().filter(|c| c.united_nations) else { return };
    let own: Vec<String> = obs.city_states.iter().filter(|i| i.ally == Some(obs.player)).map(|i| format!("{}:{}", i.player.0, i.influence)).collect();
    let all: Vec<String> = c.delegates.iter().map(|(p, d)| format!("{}:{d}", p.0)).collect();
    push(format!(
        "r{} p{} WATCH needed {} due {} in {} session {} | treasury {} income {} | delegates {} | allies {}",
        obs.round,
        obs.player.0,
        c.needed,
        c.leader_vote_next,
        c.turns_until_session,
        c.in_session,
        obs.economy.treasury,
        obs.economy.income.gold,
        all.join(","),
        if own.is_empty() { "-".to_owned() } else { own.join(",") }
    ));
}

/// The tracker's readings of the rivals' roads to the victories (trace only, the vote guard):
/// the diplomatic race (delegates, gap, chance), the original capitals held and the era against
/// the spaceship's. The last two are readings, not acted on.
pub fn readings(obs: &crate::knowledge::observation::AIObservation<'_>, rules: &CompiledRuleset) {
    if !enabled() {
        return;
    }
    let dipl = crate::ai::victory_watch::diplomatic(obs).map_or_else(
        || "-".to_owned(),
        |d| format!("needed {} in {} reach {} own {}/{} | {}", d.needed, d.rounds, d.reach, d.own.delegates, d.own.chance, d.rivals.iter().map(|r| format!("{}:{}/{}", r.rival.0, r.delegates, r.chance)).collect::<Vec<_>>().join(",")),
    );
    let others = crate::ai::victory_watch::others(obs, super::sharp::space_era(rules));
    let dom: Vec<String> = others.iter().map(|o| format!("{}:{}/{}", o.rival.0, o.capitals_held, o.capitals_all)).collect();
    let sci: Vec<String> = others.iter().map(|o| format!("{}:{}", o.rival.0, o.era_vs_space)).collect();
    push(format!("r{} p{} READ diplomatic {} | domination {} | science {}", obs.round, obs.player.0, dipl, dom.join(","), sci.join(",")));
}
