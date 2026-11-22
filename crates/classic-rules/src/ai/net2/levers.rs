//! The forward pass of `net2` around the carried intent (design §4.2–§4.7): the rivals' concepts and
//! reads, the war order with its hysteresis and budget, the aggregates and the intent's tilt; after
//! the intent, the levers the executors read.

use crate::ai::strategy_net::layers::ONE;
use crate::ai::strategy_net::weights::{fe, kc, pd};
use crate::ai::strategy_net::{Intent, Milli, Tilt};
use crate::ids::PlayerId;

use super::concepts::{self, fire, Frame};
use super::plan::{AimWhy, OrderPhase, CityGuard, Part, Parts, CityInputs, CoreMemory, Embargo, Escorts, Generals, Help, Inputs, Net2Plan, Offence, Opening, RivalRead, Soldiers, SpentWar, TargetInputs, WarAim, WarKind, WarOrder, WarPlan};
use super::weights::{self as w, a, gate, q, r, s, y, Src, Table, Term};

/// One rival's layer: its inputs and its concepts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RivalPass {
    pub rival: PlayerId,
    pub inputs: [Milli; r::N],
    pub concepts: [Milli; q::N],
}

/// The pass before the intent: everything the tilt and the levers read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pass {
    pub carried: [Milli; kc::N],
    pub seat: [Milli; s::N],
    pub rivals: Vec<RivalPass>,
    pub reads: Vec<RivalRead>,
    /// The war order and the spent ones after this turn.
    pub core: CoreMemory,
    pub aggregates: [Milli; a::N],
    /// The ease of every target (`Inputs::targets`' order).
    pub eases: Vec<Milli>,
    /// The parts of `net2` that play (the ablation, `plan::Parts`).
    pub parts: Parts,
}

/// The pass before the intent: the seat's and the rivals' concepts, the war order (from `held`; none when the
/// war is not `net2`'s part, `Part::War`), the aggregates.
pub fn before_intent(inputs: &Inputs, carried: &[Milli; kc::N], held: &CoreMemory, round: u32, pace: u32, parts: Parts) -> Pass {
    let seat_row = inputs.seat.row();
    let seat = concepts::seat(&inputs.situation, carried, &seat_row);
    let eases: Vec<Milli> = inputs.targets.iter().map(concepts::ease).collect();
    let mut rivals = Vec::with_capacity(inputs.rivals.len());
    let mut reads = Vec::with_capacity(inputs.rivals.len());
    for rival in &inputs.rivals {
        let row = rival.row(soft_of(rival.rival, &inputs.targets, &eases));
        let concepts = concepts::rival(carried, &seat, &row);
        reads.push(read_of(rival.rival, &row, &concepts));
        rivals.push(RivalPass { rival: rival.rival, inputs: row, concepts });
    }
    let mut core = if parts.plays(Part::War) { war_plan(held, &reads, &rivals, inputs.seat.war_gain, round, pace) } else { CoreMemory::default() };
    core.war = aimed(core.war, &inputs.targets, &eases);
    let aggregates = aggregates_of(&reads, &rivals, &core.war, inputs.situation[fe::AT_WAR]);
    Pass { carried: *carried, seat, rivals, reads, core, aggregates, eases, parts }
}

/// `soft_r` (R27): the largest ease of `rival`'s targets, 0 without one.
pub fn soft_of(rival: PlayerId, targets: &[TargetInputs], eases: &[Milli]) -> Milli {
    let mut best = 0;
    for (target, ease) in targets.iter().zip(eases) {
        if target.rival == rival {
            best = best.max(*ease);
        }
    }
    best
}

/// The order aims at its rival's easiest target (the first on a tie: the nearest), every turn; a retake keeps
/// its lost cities first (`operations::targets_for`) and aims at none.
fn aimed(war: WarPlan, targets: &[TargetInputs], eases: &[Milli]) -> WarPlan {
    let WarPlan::Order(order) = war else { return war };
    if order.kind == WarKind::Retake {
        return WarPlan::Order(WarOrder { aim: None, ..order });
    }
    let mut best: Option<(&TargetInputs, Milli)> = None;
    for (target, ease) in targets.iter().zip(eases) {
        if target.rival == order.rival && best.is_none_or(|(_, e)| *ease > e) {
            best = Some((target, *ease));
        }
    }
    let aim = best.map(|(target, _)| WarAim { city: target.city, why: AimWhy::Ease });
    WarPlan::Order(WarOrder { aim, ..order })
}

/// A rival's read: its concepts by name, the war kind, the trust and the help.
fn read_of(rival: PlayerId, inputs: &[Milli; r::N], c: &[Milli; q::N]) -> RivalRead {
    let kind = war_kind(inputs, c);
    let help = if c[q::RUNAWAY] >= gate::DENY_HELP.0 { Help::Deny } else { Help::Give };
    RivalRead {
        rival,
        weak: c[q::WEAK],
        reach: c[q::REACH],
        menace: c[q::MENACE],
        distrust: c[q::DISTRUST],
        prey: c[q::PREY],
        runaway: c[q::RUNAWAY],
        spoiler: c[q::SPOILER],
        retake: c[q::RETAKE],
        duty: c[q::DUTY],
        grabber: c[q::GRABBER],
        border: c[q::BORDER],
        brake: 0,
        war_value: c[q::WAR_VALUE],
        war_kind: kind,
        trust: ONE - c[q::DISTRUST],
        help,
        soldiers: Soldiers::Unread,
    }
}

/// The argmax of the war kinds (the first on a tie); `Overseas` when not a retake and no land route.
fn war_kind(inputs: &[Milli; r::N], c: &[Milli; q::N]) -> WarKind {
    const KINDS: [WarKind; 5] = [WarKind::Prey, WarKind::Retake, WarKind::Spoiler, WarKind::Duty, WarKind::Border];
    let mut best = 0;
    for (i, at) in q::KINDS.iter().enumerate() {
        if c[*at] > c[q::KINDS[best]] {
            best = i;
        }
    }
    let kind = KINDS[best];
    if kind != WarKind::Retake && inputs[r::LAND] == 0 {
        return WarKind::Overseas;
    }
    kind
}

/// What becomes of a held order this turn.
enum Verdict {
    Keep(WarOrder),
    /// Its budget ran out without paying: no new order on its rival for a while, peace with it.
    Spent,
    /// Its reason is gone (or its rival).
    Dropped,
}

/// The war order of the turn (§4.4): the held one kept while its reason lasts inside its budget, or a
/// capture or a retake case running; else a new one on the largest war value ≥ the start.
fn war_plan(held: &CoreMemory, reads: &[RivalRead], rivals: &[RivalPass], war_gain: Milli, round: u32, pace: u32) -> CoreMemory {
    let mut spent: Vec<SpentWar> = held.spent.iter().filter(|s| s.until > round).copied().collect();
    let hires: Vec<SpentWar> = held.hires.iter().filter(|s| s.until > round).copied().collect();
    if let WarPlan::Order(order) = held.war {
        match judge(&order, reads, rivals, war_gain, round, pace) {
            Verdict::Keep(kept) => return CoreMemory { war: WarPlan::Order(kept), spent, hires },
            Verdict::Spent => spent.push(SpentWar { rival: order.rival, until: round + paced(gate::SPENT_ROUNDS.0, pace) }),
            Verdict::Dropped => {}
        }
    }
    CoreMemory { war: start(reads, rivals, &spent, round, pace), spent, hires }
}

/// The order's phase this turn: a war with its rival begun since the last turn starts the budget's clock.
fn phase_now(order: &WarOrder, at_war: bool, round: u32) -> Option<OrderPhase> {
    match (order.phase, at_war) {
        (OrderPhase::Preparing, false) => Some(OrderPhase::Preparing),
        (OrderPhase::Preparing, true) => Some(OrderPhase::Fighting { since: round }),
        (OrderPhase::Fighting { since }, true) => Some(OrderPhase::Fighting { since }),
        // The war it fought has ended: the order with it.
        (OrderPhase::Fighting { .. }, false) => None,
    }
}

fn judge(order: &WarOrder, reads: &[RivalRead], rivals: &[RivalPass], war_gain: Milli, round: u32, pace: u32) -> Verdict {
    let (Some(read), Some(rival)) = (reads.iter().find(|x| x.rival == order.rival), rivals.iter().find(|x| x.rival == order.rival)) else {
        return Verdict::Dropped;
    };
    let Some(phase) = phase_now(order, rival.inputs[r::AT_WAR] > 0, round) else { return Verdict::Dropped };
    let in_budget = match phase {
        OrderPhase::Preparing => round <= order.since + paced(gate::PREPARE_ROUNDS.0, pace),
        OrderPhase::Fighting { since } => round <= since + order.budget,
    };
    let retaking = rival.inputs[r::RETAKE_CASE] > 0;
    let pays = rival.inputs[r::CAPTURED_RECENT] > 0 || retaking;
    let kept = WarOrder { value: read.war_value, phase, ..*order };
    if read.war_value >= gate::WAR_KEEP.0 && (in_budget || pays) {
        return Verdict::Keep(kept);
    }
    // Its army forms or marches: a reason that dips (the local force moves with the army) does not disband it
    // inside the budget (WAR-LEVER.md: 18 of 36 operations ended so, `unordered`).
    if in_budget && rival.inputs[r::OPERATING] > 0 {
        return Verdict::Keep(kept);
    }
    if !in_budget && war_gain <= ONE / 2 && !retaking {
        return Verdict::Spent;
    }
    Verdict::Dropped
}

/// A new order on the rival with the largest war value at least the start (the lower id on a tie),
/// none on a rival whose last order was spent; at war with it already, its war's clock starts now.
fn start(reads: &[RivalRead], rivals: &[RivalPass], spent: &[SpentWar], round: u32, pace: u32) -> WarPlan {
    let mut best: Option<&RivalRead> = None;
    for read in reads {
        if read.war_value < gate::WAR_START.0 || spent.iter().any(|s| s.rival == read.rival) {
            continue;
        }
        if best.is_none_or(|b| read.war_value > b.war_value) {
            best = Some(read);
        }
    }
    match best {
        Some(read) => {
            let at_war = rivals.iter().any(|x| x.rival == read.rival && x.inputs[r::AT_WAR] > 0);
            let phase = if at_war { OrderPhase::Fighting { since: round } } else { OrderPhase::Preparing };
            WarPlan::Order(WarOrder { rival: read.rival, kind: read.war_kind, since: round, budget: paced(read.war_kind.budget(), pace), phase, value: read.war_value, aim: None, opening: Opening::Plain, raid: false })
        }
        None => WarPlan::NoWar,
    }
}

/// `rounds` Standard rounds at the game's pace (percent), at least 1.
fn paced(rounds: u32, pace: u32) -> u32 {
    (rounds * pace.max(1) / 100).max(1)
}

/// The aggregates (`weights::a`).
fn aggregates_of(reads: &[RivalRead], rivals: &[RivalPass], war: &WarPlan, at_war: Milli) -> [Milli; a::N] {
    let mut out = [0; a::N];
    out[a::ONE] = ONE;
    for read in reads {
        out[a::MAX_PREY] = out[a::MAX_PREY].max(read.prey);
        out[a::MAX_MENACE] = out[a::MAX_MENACE].max(read.menace);
    }
    match war {
        WarPlan::Order(order) => {
            let at_war_with = rivals.iter().any(|x| x.rival == order.rival && x.inputs[r::AT_WAR] > 0);
            let side = if at_war_with { a::ORDER_AT_WAR } else { a::ORDER_AT_PEACE };
            out[side] = order.value;
            out[a::ORDER_VALUE] = order.value;
        }
        WarPlan::NoWar => {
            if at_war == 0 {
                out[a::NO_ORDER] = ONE;
            }
        }
    }
    out
}

/// The terms of `weights::TILT` on the carried logits, of the parts that play.
pub fn tilt(pass: &Pass) -> Tilt {
    let frame = Frame { s: &pass.seat, a: &pass.aggregates, ..Frame::default() };
    let mut out = Tilt::NONE;
    for (table, at, term) in w::TILT {
        if !pass.parts.plays(tilt_part(term)) {
            continue;
        }
        let add = term.w * frame.shaped(term) / ONE;
        match table {
            Table::Style => out.style[*at] += add,
            Table::Aim => out.aim[*at] += add,
            Table::Posture => out.posture[*at] += add,
        }
    }
    out
}

/// The part a tilt term belongs to: the war order's posture terms (the order's value, no order) are the war's,
/// the others the strategy's.
fn tilt_part(term: &Term) -> Part {
    match term.src {
        Src::A(a::ORDER_AT_PEACE | a::ORDER_AT_WAR | a::NO_ORDER) => Part::War,
        _ => Part::Strategy,
    }
}

/// The intent's shares in `weights::y`'s order.
pub fn shares(intent: &Intent) -> [Milli; y::N] {
    let mut out = [0; y::N];
    out[0..3].copy_from_slice(&intent.style);
    out[3..7].copy_from_slice(&intent.aim);
    out[7..11].copy_from_slice(&intent.posture);
    out
}

/// The levers after the intent and the tilt of the prices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct After {
    pub plan: Net2Plan,
    pub price_tilt: [Milli; pd::N],
    /// The peace neuron per rival at war (by `Pass::rivals`' order; 0 where not evaluated).
    pub peace: Vec<Milli>,
}

/// The levers of the turn (§4.4–§4.7) from the pass and the intent.
pub fn after_intent(pass: &Pass, intent: &Intent, inputs: &Inputs, round: u32) -> After {
    let y = shares(intent);
    let peace = peace_scores(pass);
    let mut peace_with = Vec::new();
    for (rival, score) in pass.rivals.iter().zip(&peace) {
        let spent = pass.core.spent.iter().any(|s| s.rival == rival.rival);
        if rival.inputs[r::AT_WAR] > 0 && pass.core.war.order().is_none_or(|o| o.rival != rival.rival) && (spent || *score >= gate::PEACE.0) {
            peace_with.push(rival.rival);
        }
    }
    let guard = inputs.cities.iter().map(|c| CityGuard { city: c.city, level: guard_of(pass, c) }).collect();
    let frame = Frame { f: &inputs.situation, k: &pass.carried, g: &inputs.seat.row(), y: &y, ..Frame::default() };
    let cs_gold_share = fire(&w::CS_SHARE, &frame);
    let mut price_tilt = [0; pd::N];
    price_tilt[pd::MILITARY] = fire(&w::MILITARY, &Frame { s: &pass.seat, a: &pass.aggregates, y: &y, ..Frame::default() });
    let at_war = inputs.situation[fe::AT_WAR] > 0;
    let plan = Net2Plan {
        parts: pass.parts,
        round,
        war: pass.core.war,
        peace_with,
        rivals: pass.reads.clone(),
        guard,
        cs_gold_share,
        embargo: embargo_of(&pass.reads),
        escorts: escorts_of(&pass.core.war),
        generals: if pass.core.war.order().is_none() && !at_war { Generals::Hold } else { Generals::AsToday },
        tall_share: intent.style[0],
        offence: Offence::default(),
        aim_ease: super::orders::aim_ease(&pass.core.war, &inputs.targets, &pass.eases),
    };
    After { plan, price_tilt, peace }
}

/// The peace neuron of every rival at war with us (0 for the others).
fn peace_scores(pass: &Pass) -> Vec<Milli> {
    let mut out = Vec::with_capacity(pass.rivals.len());
    for rival in &pass.rivals {
        let score = if rival.inputs[r::AT_WAR] > 0 { fire(&w::PEACE, &Frame { s: &pass.seat, r: &rival.inputs, q: &rival.concepts, ..Frame::default() }) } else { 0 };
        out.push(score);
    }
    out
}

/// One city's guard inputs (`weights::c`).
pub fn city_row(reads: &[RivalRead], city: &CityInputs) -> [Milli; w::c::N] {
    let read = |p: PlayerId| reads.iter().find(|x| x.rival == p);
    let menace = match city.stack_owner.and_then(read) {
        Some(x) => x.menace,
        None => 0,
    };
    let mut distrust = 0;
    for p in &city.rivals_near {
        if let Some(x) = read(*p) {
            distrust = distrust.max(x.distrust);
        }
    }
    [menace, city.danger, distrust, city.young]
}

fn guard_of(pass: &Pass, city: &CityInputs) -> Milli {
    let row = city_row(&pass.reads, city);
    fire(&w::GUARD, &Frame { s: &pass.seat, c: &row, ..Frame::default() })
}

/// The rival with the largest runaway at least the embargo's gate (the lower id on a tie).
fn embargo_of(reads: &[RivalRead]) -> Embargo {
    let mut best: Option<&RivalRead> = None;
    for read in reads {
        if read.runaway >= gate::EMBARGO.0 && best.is_none_or(|b| read.runaway > b.runaway) {
            best = Some(read);
        }
    }
    match best {
        Some(read) => Embargo::On(read.rival),
        None => Embargo::Nobody,
    }
}

fn escorts_of(war: &WarPlan) -> Escorts {
    match war.order() {
        Some(order) if order.kind == WarKind::Overseas => Escorts::Ships(gate::ESCORT_SHIPS.0),
        _ => Escorts::AsToday,
    }
}
