//! The worked cases of the design (§4.8), each verdict computed by hand in its comment; the war order's
//! timing (§3.3); the float twin's fixture (§3.2).

use crate::ai::strategy_net::weights::{fe, kc};
use crate::ai::strategy_net::{intent_from, Held, Intent, Logits, Milli, Net, Tilt};
use crate::ids::{EntityId, PlayerId, TileId};

use super::levers::{self, After, Pass};
use super::plan::{AimWhy, Ballot, OrderPhase, CityInputs, Parts, CoreMemory, Embargo, Escorts, Generals, Help, Inputs, Opening, RivalInputs, SeatInputs, SpentWar, TargetInputs, WarAim, WarKind, WarOrder, WarPlan};
use super::weights::{a, q};

const ROUND: u32 = 100;
const PACE: u32 = 100;

fn rival(id: u8) -> RivalInputs {
    RivalInputs::stranger(PlayerId(id))
}

fn seat() -> SeatInputs {
    SeatInputs { war_gain: 500, ..SeatInputs::default() }
}

/// A seat at peace with nothing hostile in sight (force 1), its leader's `warlike` 300.
fn situation() -> [Milli; fe::N] {
    let mut f = [0; fe::N];
    f[fe::FORCE] = 1000;
    f
}

fn carried() -> [Milli; kc::N] {
    let mut k = [0; kc::N];
    k[kc::WARLIKE] = 300;
    k
}

fn inputs(seat: SeatInputs, rivals: Vec<RivalInputs>, cities: Vec<CityInputs>) -> Inputs {
    Inputs { situation: situation(), seat, rivals, cities, pairs: vec![], aims: vec![], targets: vec![], space: vec![], capital: None }
}

fn pass_of(inputs: &Inputs, held: &CoreMemory, round: u32) -> Pass {
    levers::before_intent(inputs, &carried(), held, round, PACE, Parts::All)
}

fn pass(inputs: &Inputs) -> Pass {
    pass_of(inputs, &CoreMemory::default(), ROUND)
}

/// The intent of flat logits with the pass's tilt (no temperature), and the levers.
fn after(inputs: &Inputs, pass: &Pass) -> After {
    let logits = Logits { concepts: carried(), style: [0; 3], aim: [0; 4], posture: [0; 4], style_margin: 1150 };
    let tilt = levers::tilt(pass);
    let intent = intent_from(&logits, &inputs.situation, Held::default(), 3, 200, PACE, Net::default(), &tilt);
    levers::after_intent(pass, &intent, inputs, ROUND)
}

/// A5 strong: R4 50, R6 800, R5 0, R2 1000, R1 800, `warlike` 300.
fn disarmed_neighbour(id: u8) -> RivalInputs {
    RivalInputs { soldiers: 50, open_cities: 800, seen_army: 0, land: 1000, near: 800, ..rival(id) }
}

#[test]
fn a5_the_disarmed_neighbour_is_prey() {
    let inputs = inputs(seat(), vec![disarmed_neighbour(1)], vec![]);
    let p = pass(&inputs);
    let c = &p.rivals[0].concepts;
    // weak = unit(1000 − 2000·0.05 + 300·0.8) = unit(1140) = 1000; reach = 1000·0.8 = 800.
    assert_eq!((c[q::WEAK], c[q::REACH]), (1000, 800));
    // prey = unit(−1700 + 1600 + 1600·0.8 + 400·0.3) = unit(1300) = 1000 → a Prey order with the budget of 25.
    assert_eq!(c[q::PREY], 1000);
    let order = WarOrder { rival: PlayerId(1), kind: WarKind::Prey, since: ROUND, budget: 25, value: 1000, phase: OrderPhase::Preparing, aim: None, opening: Opening::Plain, raid: false };
    assert_eq!(p.core.war, WarPlan::Order(order));
    // At peace with it: the posture prepares (+1800·1.0), conquest gains +1500 − 400.
    let tilt = levers::tilt(&p);
    assert_eq!((tilt.posture[2], tilt.posture[3]), (1800, 0));
    assert_eq!(tilt.style, [0, -1200, 1100]);
}

/// Casebook E4 (A5 #11): the order's target gets no friendship, pact, agreement or open borders while the order lasts;
/// another rival keeps its help.
#[test]
fn the_war_orders_target_gets_no_help() {
    let inputs = inputs(seat(), vec![disarmed_neighbour(1), RivalInputs { soldiers: 500, ..rival(2) }], vec![]);
    let p = pass(&inputs);
    assert_eq!(p.core.war.order().map(|o| o.rival), Some(PlayerId(1)));
    let plan = after(&inputs, &p).plan;
    assert!(plan.denies_help(PlayerId(1)), "the order's target");
    assert!(!plan.denies_help(PlayerId(2)), "a rival without an order");
}

/// The aluminium case (capital-fix FIX.md Part 2): in the open race the copies the missing parts need are held; outside
/// it, none.
#[test]
fn the_space_race_holds_the_parts_copies() {
    use crate::ids::ResourceId;
    let need = super::plan::SpaceNeed { resource: ResourceId(7), need: 3, free: 2 };
    let racing = Inputs { space: vec![need], ..inputs(SeatInputs { space_race: 1000, parts_short: 333, ..seat() }, vec![], vec![]) };
    let p = pass(&racing);
    let a = after(&racing, &p);
    // part_hold = unit(−500 + 1500 + 300·0.333) = 1000: every copy the missing parts need.
    assert_eq!(a.part_hold, 1000);
    assert_eq!((a.plan.space_hold.clone(), a.plan.held(ResourceId(7)), a.plan.held(ResourceId(8))), (vec![(ResourceId(7), 3)], 3, 0));
    let idle = inputs(seat(), vec![], vec![]);
    let q = pass(&idle);
    assert!(after(&idle, &q).plan.space_hold.is_empty());
}

/// The city cap of a real tall seat (`weights::CITY_CAP`) on Rome, archipelago 1810002, round 55 (R4 trace): 2 cities,
/// 4 free sites (room 666), excess happiness +8 (900), the conquest share 0.40 and wide 0.07 under the conquest bias;
/// the size model 0 more (its floor of 6 after the city). The tilt founds on; a pure tall seat keeps the model's count.
#[test]
fn the_tall_cap_is_the_model_and_the_tilt() {
    use super::concepts::{fire, Frame};
    use super::weights::{g, CITY_CAP};
    let mut f = [0; fe::N];
    (f[fe::ROOM], f[fe::CITIES], f[fe::HAPPY], f[fe::PROGRESS]) = (666, 250, 900, 166);
    let k = [0; kc::N];
    let aggs = [0; a::N];
    let rome = [530, 70, 400];
    let cap = |f: &[Milli; fe::N], model: Milli, aggs: &[Milli; a::N], style: [Milli; 3]| {
        let mut gs = [0; g::N];
        gs[g::SIZE_MODEL] = model;
        let mut y = [0; super::weights::y::N];
        y[0..3].copy_from_slice(&style);
        fire(&CITY_CAP, &Frame { f, k: &k, g: &gs, a: aggs, y: &y, ..Frame::default() })
    };
    // 3000·0.40·0.666 = 3·266 (the product rounded down); the wide share reads no weight.
    assert_eq!(cap(&f, 0, &aggs, rome), 798);
    assert_eq!(levers::cities_of(798), 1, "one more: the target 3");
    // A pure tall seat: the model's 2 and 3·33 → 2.
    assert_eq!(levers::cities_of(cap(&f, 500, &aggs, [900, 50, 50])), 2);
    // Unhappy (−4: 300) and a menacing stack at the border: 798 − 8000·0.2 − 2000·0.8 < 0 → none.
    let mut sad = f;
    sad[fe::HAPPY] = 300;
    let mut menaced = aggs;
    menaced[a::MAX_MENACE] = 800;
    assert_eq!(levers::cities_of(cap(&sad, 0, &menaced, rome)), 0);
    // Late, with 6 cities and one free site: 198 − 4000·0.375 − 5000·0.3 < 0 → none.
    let mut late = f;
    (late[fe::ROOM], late[fe::CITIES], late[fe::PROGRESS]) = (166, 750, 900);
    assert_eq!(levers::cities_of(cap(&late, 0, &aggs, rome)), 0);
    assert_eq!(levers::cities_of(499), 0, "half a city rounds up, less does not");
    assert_eq!(levers::cities_of(500), 1);
}

/// The research on the ship's way (`weights::SPACE_RESEARCH`): England, pangaea 1810001, at robotics (aim science 0.95,
/// the way 0.6) keeps to it; a diplomatic seat (aim 0.2) at the same point does not.
#[test]
fn the_science_seat_researches_the_ships_way() {
    use super::concepts::{fire, Frame};
    use super::weights::SPACE_RESEARCH;
    let mut f = [0; fe::N];
    f[fe::SPACE_OWN] = 600;
    let level = |aim: Milli| {
        let mut y = [0; super::weights::y::N];
        y[super::weights::y::AIM_SCIENCE] = aim;
        fire(&SPACE_RESEARCH, &Frame { f: &f, y: &y, ..Frame::default() })
    };
    assert_eq!(level(950), 1000, "unit(−1000 + 1900 + 600)");
    assert_eq!(level(200), 0, "unit(−1000 + 400 + 600)");
    assert!(level(450) >= super::weights::gate::SPACE_RESEARCH.0, "−1000 + 900 + 600 = 500");
}

/// The capital kept for the parts (`weights::CAPITAL_GATE`): a part outside the capital opens it, none keeps it shut
/// (Korea, pangaea 2201008: a worker parked in the capital shut the engine out from r317 to r330).
#[test]
fn a_part_outside_keeps_the_capital() {
    use super::concepts::{fire, Frame};
    use super::weights::{g, gate, CAPITAL_GATE};
    let level = |out: Milli| {
        let mut row = [0; g::N];
        row[g::PART_OUT] = out;
        fire(&CAPITAL_GATE, &Frame { g: &row, ..Frame::default() })
    };
    assert_eq!(level(1000), 1000, "unit(−500 + 1500)");
    assert_eq!(level(0), 0, "unit(−500)");
    assert!(level(1000) >= gate::CAPITAL_GATE.0 && level(0) < gate::CAPITAL_GATE.0);
}

#[test]
fn an_average_neighbour_is_no_prey() {
    let inputs = inputs(seat(), vec![RivalInputs { soldiers: 500, open_cities: 0, ..disarmed_neighbour(1) }], vec![]);
    let p = pass(&inputs);
    // weak = unit(1000 − 1000) = 0; prey = unit(−1700 + 1280 + 120) = 0 → no war.
    assert_eq!((p.rivals[0].concepts[q::WEAK], p.rivals[0].concepts[q::PREY]), (0, 0));
    assert_eq!(p.core.war, WarPlan::NoWar);
    // No order and no war: the offensive postures are capped (−3000 each).
    let tilt = levers::tilt(&p);
    assert_eq!((tilt.posture[2], tilt.posture[3]), (-3000, -3000));
}

#[test]
fn a_disarmed_friend_is_prey_at_a_price() {
    // WAR-LEVER.md K3: the rules add nothing to a declaration on a friend; its yields are the price, no veto.
    let inputs = inputs(seat(), vec![RivalInputs { friend: 1000, ..disarmed_neighbour(1) }], vec![]);
    let p = pass(&inputs);
    // prey = unit(1300 − 600) = 700 ≥ 550 → a Prey order.
    assert_eq!(p.rivals[0].concepts[q::PREY], 700);
    assert_eq!(p.core.war.order().map(|o| o.kind), Some(WarKind::Prey));
}

/// A known city of `rival` on `tile`: `size`, `garrison`, `near`, `walls` (T1–T4).
fn target(rival: u8, tile: u32, size: Milli, garrison: Milli, near: Milli, walls: Milli) -> TargetInputs {
    TargetInputs { rival: PlayerId(rival), city: TileId(tile), size, garrison, near, walls }
}

/// A neighbour with a level army (weak 0), reachable (land, near 800: reach 800).
fn level_neighbour(id: u8) -> RivalInputs {
    RivalInputs { soldiers: 500, land: 1000, near: 800, ..rival(id) }
}

#[test]
fn a_level_neighbour_with_a_soft_city_is_prey_and_the_order_aims_at_it() {
    // WAR-LEVER.md K1: 5 citizens (size 250), no garrison seen, 4 tiles (near 1000), walls 200 against our army.
    // ease = unit(−2200 + 750 + 800 + 1000 + 480) = 830; a large guarded city: unit(−2200 + 0 + 320 + 600 + 300) = 0.
    let mut inputs = inputs(seat(), vec![level_neighbour(1)], vec![]);
    inputs.targets = vec![target(1, 70, 1000, 600, 600, 500), target(1, 71, 250, 0, 1000, 200)];
    let p = pass(&inputs);
    assert_eq!(p.eases, vec![0, 830]);
    assert_eq!(p.rivals[0].inputs[super::weights::r::SOFT], 830);
    // prey = unit(−1700 + 0 + 1280 + 120 + 996) = 696 → a Prey order aimed at the soft city.
    assert_eq!(p.rivals[0].concepts[q::PREY], 696);
    let aim = p.core.war.order().and_then(|o| o.aim);
    assert_eq!(aim, Some(WarAim { city: TileId(71), why: AimWhy::Ease }));
}

#[test]
fn a_level_neighbour_with_only_large_guarded_cities_is_no_prey() {
    let mut inputs = inputs(seat(), vec![level_neighbour(1)], vec![]);
    inputs.targets = vec![target(1, 70, 1000, 600, 600, 500), target(1, 72, 600, 400, 600, 400)];
    let p = pass(&inputs);
    // ease of the second = unit(−2300 + 400 + 480 + 600 + 360) = 0 → soft 0; prey = unit(−1700 + 1280 + 120) = 0.
    assert_eq!(p.rivals[0].concepts[q::PREY], 0);
    assert_eq!(p.core.war, WarPlan::NoWar);
}

#[test]
fn the_infamy_closes_a_new_prey_at_peace_but_not_the_war_under_way() {
    // One declaration at the era's 70 %: 175 points → G11 = 700; infamy_cost = unit(−100 + 700) = 600.
    let infamous = SeatInputs { own_infamy: 700, ..seat() };
    let at_peace = pass(&inputs(infamous, vec![disarmed_neighbour(1)], vec![]));
    assert_eq!(at_peace.seat[super::weights::s::INFAMY_COST], 600);
    // prey = unit(1300 − 900·0.6) = 760 (still an order: a disarmed neighbour); with near 400: unit(660 − 540) = 120.
    assert_eq!(at_peace.rivals[0].concepts[q::PREY], 760);
    let far = pass(&inputs(infamous, vec![RivalInputs { near: 400, ..disarmed_neighbour(1) }], vec![]));
    assert_eq!(far.core.war, WarPlan::NoWar);
    // At war with it the declaration is paid: unit(660 − 540 + 540) = 660.
    let at_war = pass(&inputs(infamous, vec![prey_at(400)], vec![]));
    assert_eq!(at_war.rivals[0].concepts[q::PREY], 660);
}

#[test]
fn a_disarmed_nation_over_the_ocean_is_no_prey() {
    // The class the design's formula missed: no land and no sea route, reach 0.
    let inputs = inputs(seat(), vec![RivalInputs { land: 0, sea: 0, ..disarmed_neighbour(1) }], vec![]);
    let p = pass(&inputs);
    // prey = unit(−1700 + 1600 + 0 + 120) = 20 → no war.
    assert_eq!(p.rivals[0].concepts[q::PREY], 20);
    assert_eq!(p.core.war, WarPlan::NoWar);
}

#[test]
fn a_disarmed_neighbour_over_a_strait_is_an_overseas_prey() {
    let inputs = inputs(seat(), vec![RivalInputs { land: 0, sea: 1000, near: 1000, ..disarmed_neighbour(1) }], vec![]);
    let p = pass(&inputs);
    // reach = 600·1.0 = 600; prey = unit(−1700 + 1600 + 960 + 120) = 980 → Overseas, budget 30, 2 escorts.
    assert_eq!((p.rivals[0].concepts[q::REACH], p.rivals[0].concepts[q::PREY]), (600, 980));
    assert_eq!(p.core.war.order().map(|o| (o.kind, o.budget)), Some((WarKind::Overseas, 30)));
    assert_eq!(after(&inputs, &p).plan.escorts, Escorts::Ships(2));
}

/// A runaway (tree 1000, lead 1000: runaway 1000), reachable (land, near 800), a level army, our force even there.
fn runaway(id: u8) -> RivalInputs {
    RivalInputs { tree: 1000, lead: 1000, land: 1000, near: 800, soldiers: 500, local: 500, ..rival(id) }
}

#[test]
fn a_runaway_with_a_level_army_gets_no_war_but_no_help_and_an_embargo() {
    let inputs = inputs(seat(), vec![runaway(2)], vec![]);
    let p = pass(&inputs);
    let c = &p.rivals[0].concepts;
    // runaway = unit(−1000 + 1200 + 1000) = 1000; spoiler = unit(−1400 + 1000 + 700·0.8 + 0 + 0) = 160 → no war.
    assert_eq!((c[q::RUNAWAY], c[q::SPOILER]), (1000, 160));
    assert_eq!(p.core.war, WarPlan::NoWar);
    let plan = after(&inputs, &p).plan;
    assert_eq!(plan.rivals[0].help, Help::Deny);
    assert_eq!(plan.embargo, Embargo::On(PlayerId(2)));
}

/// The World Leader ballot (GAP.md): a rival whose own delegates reach the votes is blocked; one short is not (every
/// seat votes for itself, so it wins nothing, and a block would only elect the candidate we pooled on).
#[test]
fn the_ballot_blocks_only_a_rival_with_the_votes_of_its_own() {
    let blocked = |alone: Milli| {
        let inputs = inputs(seat(), vec![rival(2), RivalInputs { alone, ..rival(3) }], vec![]);
        let p = pass(&inputs);
        (p.rivals[1].concepts[q::BLOCK], after(&inputs, &p).plan.ballot)
    };
    assert_eq!(blocked(1000), (1000, Ballot::Block(PlayerId(3))));
    assert_eq!(blocked(0), (0, Ballot::Own));
}

#[test]
fn a_runaway_we_can_beat_locally_gets_a_spoiler_war() {
    let inputs = inputs(seat(), vec![RivalInputs { soldiers: 250, local: 800, ..runaway(2) }], vec![]);
    let p = pass(&inputs);
    // weak = unit(1000 − 500) = 500; spoiler = unit(160 + 600·0.5 + 1000·0.3) = 760 → Spoiler, budget 30.
    assert_eq!(p.rivals[0].concepts[q::SPOILER], 760);
    assert_eq!(p.core.war.order().map(|o| (o.kind, o.budget, o.value)), Some((WarKind::Spoiler, 30, 760)));
}

/// B4: the capital lost to an enemy we are at war with, our force even there, tired.
fn b4() -> Inputs {
    let tired = SeatInputs { capital_lost: 1000, war_rounds: 1000, unhappy_trend: 1000, ..seat() };
    let taker = RivalInputs { holds_capital: 1000, local: 500, at_war: 1000, land: 1000, retake_case: 1000, ..rival(3) };
    let other = RivalInputs { at_war: 1000, ..rival(4) };
    let mut inputs = inputs(tired, vec![taker, other], vec![]);
    inputs.situation[fe::AT_WAR] = 1000;
    inputs
}

#[test]
fn b4_a_lost_capital_is_retaken_even_tired() {
    let inputs = b4();
    let p = pass(&inputs);
    // war_weary = unit(−400 + 1200 + 900 + 0 + 0 − 0) = 1000.
    assert_eq!(p.seat[super::weights::s::WAR_WEARY], 1000);
    // retake = unit(−900 + 1800 + 600·0.5 + 300 − 500) = 1000 → Retake order, budget 20.
    assert_eq!(p.rivals[0].concepts[q::RETAKE], 1000);
    assert_eq!(p.core.war.order().map(|o| (o.rival, o.kind, o.budget)), Some((PlayerId(3), WarKind::Retake, 20)));
    // Peace: not with the taker (the order, the retake case); with the other enemy:
    // unit(−300 + 1000 + 600·1.0) = 1000 ≥ 500.
    let after = after(&inputs, &p);
    assert_eq!(after.plan.peace_with, vec![PlayerId(4)]);
    assert_eq!(after.peace, vec![0, 1000], "the taker's peace: unit(−300 + 1000 + 0 − 1500) = 0");
    assert_eq!(after.plan.generals, Generals::AsToday);
}

#[test]
fn a_war_fought_with_local_strength_alone_is_no_retake() {
    // The class the design's formula missed: at war, strong locally, nothing of ours in its hands.
    let inputs = inputs(seat(), vec![RivalInputs { local: 1000, at_war: 1000, land: 1000, ..rival(3) }], vec![]);
    // retake = unit(−900 + 600 + 300) = 0.
    assert_eq!(pass(&inputs).rivals[0].concepts[q::RETAKE], 0);
}

/// P3: a stack of 7 inside our borders at peace (R11 700) of a rival we distrust at 400.
fn p3() -> Inputs {
    // distrust = unit(−500 + 700·0.7 + 500·0.22 + 300·1.0) = 400.
    let stacker = RivalInputs { stack: 700, warmonger: 220, near: 1000, ..rival(5) };
    let near = CityInputs { city: EntityId(10), stack_owner: Some(PlayerId(5)), danger: 0, rivals_near: vec![], young: 0 };
    let far = CityInputs { city: EntityId(11), stack_owner: None, danger: 0, rivals_near: vec![], young: 0 };
    inputs(seat(), vec![stacker], vec![near, far])
}

#[test]
fn p3_a_stack_at_the_border_is_defence() {
    let inputs = p3();
    let p = pass(&inputs);
    let c = &p.rivals[0].concepts;
    // menace = unit(−300 + 1500·0.7 + 400·0.4) = 910.
    assert_eq!((c[q::DISTRUST], c[q::MENACE]), (400, 910));
    assert_eq!(p.aggregates[a::MAX_MENACE], 910);
    // The posture's defence: +1500·0.91 = 1365.
    assert_eq!(levers::tilt(&p).posture[1], 1365);
    let after = after(&inputs, &p);
    // guard = unit(−200 + 1200·0.91) = 892 near the stack, 0 elsewhere; the military price +800·0.91 = 728.
    assert_eq!(after.plan.guard.iter().map(|g| g.level).collect::<Vec<_>>(), vec![892, 0]);
    assert_eq!(after.price_tilt[crate::ai::strategy_net::weights::pd::MILITARY], 728);
    assert_eq!(after.plan.trust(PlayerId(5)), 600);
    assert_eq!(after.plan.generals, Generals::Hold, "no order, no war: the generals stay home");
}

// ---- the war order's timing (§3.3, §4.4) ----

fn held(value: Milli, since: u32) -> CoreMemory {
    // A war under way since `since` (the budget's clock runs from the declaration).
    CoreMemory { war: WarPlan::Order(WarOrder { rival: PlayerId(1), kind: WarKind::Prey, since, budget: 25, value, phase: OrderPhase::Fighting { since }, aim: None, opening: Opening::Plain, raid: false }), spent: vec![], hires: vec![] }
}

/// A neighbour whose prey value is `prey`-ish: weak 1000, its reach set to land it at the value.
fn prey_at(near: Milli) -> RivalInputs {
    RivalInputs { at_war: 1000, ..RivalInputs { near, ..disarmed_neighbour(1) } }
}

#[test]
fn an_order_is_kept_above_its_keep_and_dropped_below() {
    // near 400: prey = unit(−1700 + 1600 + 640 + 120) = 660 ≥ 350, kept (it would not start below 550 either way).
    let kept = pass_of(&inputs(seat(), vec![prey_at(400)], vec![]), &held(700, ROUND - 10), ROUND);
    assert_eq!(kept.core.war.order().map(|o| (o.since, o.value)), Some((ROUND - 10, 660)));
    // near 100: prey = unit(−1700 + 1600 + 160 + 120) = 180 < 350: dropped, no new order.
    let dropped = pass_of(&inputs(seat(), vec![prey_at(100)], vec![]), &held(700, ROUND - 10), ROUND);
    assert_eq!(dropped.core.war, WarPlan::NoWar);
    assert!(dropped.core.spent.is_empty(), "inside the budget a lost reason is no spent war");
}

#[test]
fn an_order_under_its_army_is_kept_inside_its_budget() {
    // near 100: prey 180 < 350, but an operation forms against it: kept inside the budget, dropped after it.
    let forming = RivalInputs { operating: 1000, ..prey_at(100) };
    let kept = pass_of(&inputs(seat(), vec![forming], vec![]), &held(700, ROUND - 10), ROUND);
    assert_eq!(kept.core.war.order().map(|o| (o.since, o.value)), Some((ROUND - 10, 180)));
    let late = pass_of(&inputs(seat(), vec![forming], vec![]), &held(700, ROUND - 30), ROUND);
    assert_eq!(late.core.war, WarPlan::NoWar);
}

#[test]
fn a_war_that_did_not_pay_ends_with_peace_and_is_not_restarted() {
    let inputs = inputs(seat(), vec![prey_at(800)], vec![]);
    let p = pass_of(&inputs, &held(1000, ROUND - 26), ROUND);
    assert_eq!(p.core.war, WarPlan::NoWar, "the budget of 25 is spent, the gain even");
    assert_eq!(p.core.spent, vec![SpentWar { rival: PlayerId(1), until: ROUND + 20 }]);
    assert_eq!(after(&inputs, &p).plan.peace_with, vec![PlayerId(1)]);
    let later = pass_of(&inputs, &p.core, ROUND + 5);
    assert_eq!(later.core.war, WarPlan::NoWar, "no new order on it while spent");
    let after_cooldown = pass_of(&inputs, &p.core, ROUND + 20);
    assert!(after_cooldown.core.war.order().is_some(), "the cooldown over, the reason opens a new order");
}

#[test]
fn a_war_that_pays_runs_past_its_budget() {
    let inputs = inputs(seat(), vec![RivalInputs { captured_recent: 1000, ..prey_at(800) }], vec![]);
    let p = pass_of(&inputs, &held(1000, ROUND - 40), ROUND);
    assert_eq!(p.core.war.order().map(|o| o.since), Some(ROUND - 40));
}

#[test]
fn one_order_at_a_time_held_against_a_better_target() {
    let better = RivalInputs { near: 1000, ..disarmed_neighbour(2) };
    let p = pass_of(&inputs(seat(), vec![prey_at(400), better], vec![]), &held(700, ROUND - 5), ROUND);
    assert_eq!(p.core.war.order().map(|o| o.rival), Some(PlayerId(1)));
}

#[test]
fn the_default_plan_is_todays_behaviour() {
    let plan = super::plan::Net2Plan::default();
    assert_eq!(plan.war, WarPlan::NoWar);
    assert_eq!(plan.trust(PlayerId(1)), 1000);
    assert!(!plan.denies_help(PlayerId(1)) && !plan.makes_peace_with(PlayerId(1)));
    assert_eq!((plan.cs_gold_share, plan.guard_of(EntityId(1))), (0, 0));
    assert_eq!((plan.embargo, plan.escorts, plan.generals), (Embargo::Nobody, Escorts::AsToday, Generals::AsToday));
    assert_eq!(plan.offence, super::plan::Offence::default(), "no offence: the design's play (`net2-defence`)");
}

// ---- the float twin's fixture ----

/// A deterministic stream of numbers (an LCG) for the generated turns.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
    /// 0…1000: a third 0, a sixth 1000, the rest uniform.
    fn milli(&mut self) -> Milli {
        match self.below(6) {
            0 | 1 => 0,
            2 => 1000,
            _ => self.below(1001) as Milli,
        }
    }
    fn flag(&mut self) -> Milli {
        if self.below(3) == 0 {
            1000
        } else {
            0
        }
    }
}

fn generated_rival(rng: &mut Lcg, id: u8) -> RivalInputs {
    let mut row = [0; super::weights::r::N];
    // `chest_wait` stays 0: only `science-flow` writes it (`science::read_waits`), and the turns before it keep their draws.
    for v in row.iter_mut().take(super::weights::r::CHEST_WAIT) {
        *v = rng.milli();
    }
    let flags = [super::weights::r::LAND, super::weights::r::SEA, super::weights::r::FRIEND, super::weights::r::PACT, super::weights::r::AT_WAR, super::weights::r::HOLDS_CAPITAL, super::weights::r::ALLY_HIT, super::weights::r::RETAKE_CASE, super::weights::r::CAPTURED_RECENT, super::weights::r::DENOUNCED_US, super::weights::r::ALONE];
    for at in flags {
        row[at] = rng.flag();
    }
    let r = |i: usize| row[i];
    use super::weights::r as ri;
    RivalInputs {
        rival: PlayerId(id),
        near: r(ri::NEAR),
        land: r(ri::LAND),
        sea: r(ri::SEA),
        soldiers: r(ri::SOLDIERS),
        seen_army: r(ri::SEEN_ARMY),
        open_cities: r(ri::OPEN_CITIES),
        tree: r(ri::TREE),
        lead: r(ri::LEAD),
        pace: r(ri::PACE),
        culture: r(ri::CULTURE),
        stack: r(ri::STACK),
        warmonger: r(ri::WARMONGER),
        betrayed: r(ri::BETRAYED),
        denounced_us: r(ri::DENOUNCED_US),
        friend: r(ri::FRIEND),
        pact: r(ri::PACT),
        at_war: r(ri::AT_WAR),
        war_rounds: r(ri::WAR_ROUNDS),
        holds_capital: r(ri::HOLDS_CAPITAL),
        holds_cities: r(ri::HOLDS_CITIES),
        grab: r(ri::GRAB),
        ally_hit: r(ri::ALLY_HIT),
        local: r(ri::LOCAL),
        score: r(ri::SCORE),
        retake_case: r(ri::RETAKE_CASE),
        captured_recent: r(ri::CAPTURED_RECENT),
        operating: if r(ri::OPERATING) > 500 { 1000 } else { 0 },
        alone: r(ri::ALONE),
        cs_held: r(ri::CS_HELD),
        ..RivalInputs::stranger(PlayerId(id))
    }
}

fn generated_city(rng: &mut Lcg, id: u64, rivals: &[RivalInputs]) -> CityInputs {
    let pick = |rng: &mut Lcg| rivals[rng.below(rivals.len() as u64) as usize].rival;
    let stack_owner = if rng.below(2) == 0 { Some(pick(rng)) } else { None };
    let rivals_near = (0..rng.below(3)).map(|_| pick(rng)).collect();
    CityInputs { city: EntityId(id), stack_owner, danger: rng.flag(), rivals_near, young: rng.flag() }
}

fn generated_target(rng: &mut Lcg, tile: u32, rivals: &[RivalInputs]) -> TargetInputs {
    let rival = rivals[rng.below(rivals.len() as u64) as usize].rival;
    TargetInputs { rival, city: TileId(tile), size: rng.milli(), garrison: rng.milli(), near: rng.milli(), walls: rng.milli() }
}

fn generated_held(rng: &mut Lcg, rivals: &[RivalInputs], round: u32) -> CoreMemory {
    let war = if rng.below(2) == 0 {
        WarPlan::NoWar
    } else {
        let rival = rivals[rng.below(rivals.len() as u64) as usize].rival;
        let kind = WarKind::ALL[rng.below(6) as usize];
        let since = round - rng.below(40) as u32;
        let phase = if rng.below(2) == 0 { OrderPhase::Preparing } else { OrderPhase::Fighting { since: since + rng.below(10) as u32 } };
        WarPlan::Order(WarOrder { rival, kind, since, budget: kind.budget(), value: rng.milli(), phase, aim: None, opening: Opening::Plain, raid: false })
    };
    let spent = if rng.below(4) == 0 { vec![SpentWar { rival: rivals[0].rival, until: round + rng.below(20) as u32 - 5 }] } else { vec![] };
    CoreMemory { war, spent, hires: vec![] }
}

/// One generated turn's line (the record of `trace::line`).
fn generated_line(rng: &mut Lcg) -> String {
    let round = 50 + rng.below(250) as u32;
    let mut situation = [0; fe::N];
    for v in situation.iter_mut() {
        *v = rng.milli();
    }
    situation[fe::AT_WAR] = rng.flag();
    let mut k = [0; kc::N];
    for v in k.iter_mut() {
        *v = rng.milli();
    }
    let seat_row: Vec<Milli> = (0..super::weights::g::N).map(|_| rng.milli()).collect();
    let seat = SeatInputs {
        capital_lost: if seat_row[0] > 700 { 1000 } else { 0 },
        cities_lost: seat_row[1],
        war_rounds: seat_row[2],
        war_gain: seat_row[3],
        unhappy_trend: seat_row[4],
        embark: if seat_row[5] > 300 { 1000 } else { 0 },
        enemy_air: seat_row[6],
        gold_stock: seat_row[7],
        cs_allies: seat_row[8],
        cs_contested: seat_row[9],
        own_infamy: seat_row[10],
        ties_at_risk: seat_row[11],
        space_race: if seat_row[12] > 600 { 1000 } else { 0 },
        parts_short: seat_row[13],
        size_model: seat_row[14],
        part_out: if seat_row[15] > 500 { 1000 } else { 0 },
        opening: seat_row[16] / 250 * 250,
        opening_gap: seat_row[17] / 250 * 250,
        lux_open: seat_row[18] / 500 * 500,
        ..SeatInputs::default()
    };
    let rivals: Vec<RivalInputs> = (0..1 + rng.below(7)).map(|i| generated_rival(rng, 1 + i as u8)).collect();
    let cities = (0..1 + rng.below(6)).map(|i| generated_city(rng, 100 + i, &rivals)).collect();
    let held = generated_held(rng, &rivals, round);
    let targets = (0..rng.below(6)).map(|i| generated_target(rng, 500 + i as u32, &rivals)).collect();
    let inputs = Inputs { situation, seat, rivals, cities, pairs: vec![], aims: vec![], targets, space: vec![], capital: None };
    let spread = |rng: &mut Lcg| rng.below(6001) as Milli - 3000;
    let logits = Logits { concepts: k, style: [spread(rng), spread(rng), spread(rng)], aim: [spread(rng), spread(rng), spread(rng), spread(rng)], posture: [spread(rng), spread(rng), spread(rng), spread(rng)], style_margin: 1150 };
    let tempered = rng.below(2) == 0;
    let pass = levers::before_intent(&inputs, &k, &held, round, PACE, Parts::All);
    let tilt = levers::tilt(&pass);
    let net = Net { sharp: tempered, ..Net::default() };
    let intent: Intent = intent_from(&logits, &inputs.situation, Held::default(), 3, 200, PACE, net, &tilt);
    let after = levers::after_intent(&pass, &intent, &inputs, round);
    let shares = levers::shares(&intent);
    let rec = super::trace::Record { player: 0, round, pace: PACE, inputs: &inputs, held: &held, logits: &logits, tempered, pass: &pass, tilt: &tilt, shares: &shares, after: &after };
    super::trace::line(&rec)
}

fn fixture_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../nn/aurelia_nn/net2/fixtures")
}

/// The fixture the float twin is checked against (`classic/nn/tests/test_net2_parity.py`): 200 generated
/// turns and the weight tables, written by the integer net. `NET2_WRITE_FIXTURE=1` rewrites them; the
/// test fails when the net and the committed fixture part.
#[test]
fn the_parity_fixture_is_the_integer_net() {
    let mut rng = Lcg(0x6e65_7432);
    let mut lines = String::new();
    for _ in 0..200 {
        lines.push_str(&generated_line(&mut rng));
        lines.push('\n');
    }
    let weights = super::trace::export();
    let dir = fixture_dir();
    if std::env::var_os("NET2_WRITE_FIXTURE").is_some() {
        std::fs::create_dir_all(&dir).expect("the fixture's directory can be made");
        std::fs::write(dir.join("parity.jsonl"), &lines).expect("the fixture is written");
        std::fs::write(dir.join("weights.json"), &weights).expect("the weights are written");
    }
    let committed = std::fs::read_to_string(dir.join("parity.jsonl")).unwrap_or_default();
    let committed_weights = std::fs::read_to_string(dir.join("weights.json")).unwrap_or_default();
    assert!(committed == lines, "the parity fixture is not the integer net's: rerun with NET2_WRITE_FIXTURE=1 and the twin's test");
    assert!(committed_weights == weights, "the weights' export changed: rerun with NET2_WRITE_FIXTURE=1 and the twin's test");
}

#[test]
fn the_weights_count() {
    let n = super::weights::count();
    assert_eq!(n, 170, "net2's own hand weights (`early-growth`: the opening's term of `CITY_CAP` and its gate; the 13 carried concepts and the carried logits are `strategy_net::weights`'s)");
}

#[test]
fn the_tilt_of_nothing_is_the_bias_terms_only() {
    let inputs = inputs(seat(), vec![], vec![]);
    let tilt = levers::tilt(&pass(&inputs));
    assert_eq!(tilt, Tilt { style: [0, -1200, -400], aim: [0; 4], posture: [0, 0, -3000, -3000] });
}

#[test]
fn the_offence_is_declared_and_off() {
    use super::weights::{OFFENCE, OFFENCE_RIVAL, OFFENCE_SEAT};
    assert!(!OFFENCE, "net2 plays as net2-defence until the offence is built");
    assert_eq!(OFFENCE_SEAT.len() + OFFENCE_RIVAL.len(), 7, "infamy_cost is built (`SEAT`)");
    for neuron in OFFENCE_SEAT.iter().chain(OFFENCE_RIVAL.iter()) {
        assert_eq!(super::concepts::fire(neuron, &super::concepts::Frame::default()), 0, "{} fires 0", neuron.name);
    }
    assert!(WarKind::Prey.waits_own_cooldown() && !WarKind::Retake.waits_own_cooldown() && !WarKind::Duty.waits_own_cooldown());
}
