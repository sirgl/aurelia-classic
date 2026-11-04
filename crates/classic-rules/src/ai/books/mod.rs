//! Scripted "book" opponents: known strong human strategies of the duel,
//! played on top of `standard`'s machinery (classic/docs/ai-books.md).
//!
//! LEAGUE OPPONENTS AND PROBES ONLY. A book is a variant the bench and
//! `ai-selfplay` seat against the end-to-end net (its E2 league) and against
//! `standard` to probe it; it is never a candidate for `standard`. It names
//! content — unit classes, technologies, policy branches — as the strategy it
//! copies names it (`plans`), which the game's AI never does.
//!
//! A book is a deterministic plan: its build order (`plans::Want`), research
//! beeline, policy line, settling count and a timing attack on the rival of
//! the nearest capital: its city worth most a round of war to the book's army,
//! the capital when none is (`target`). Everything else — the units' moves, the fights, the
//! workers, the defence of a city in danger — is `standard`'s. The attack is
//! one of `standard`'s war operations (`ai::operations`), formed and launched
//! by the book: its army gathers out of the target's strike, the book
//! declares the war when it is assembled, the members march, the front fights.
//! When the attack ends (the capital taken, its time over, the war ended by
//! the enemy) or never starts in time, the player plays `standard` for the
//! rest of the game.

pub mod plans;

use std::collections::BTreeMap;
use std::sync::Mutex;

use crate::ai::operations::{army_role, Routes, MUSTER_RADIUS};
use crate::ai::planner::{Decision, TurnBrief, UnitBrief};
use crate::ai::planner_units::HEAL_BELOW_HP;
use crate::ai::state::{AiState, Composition, ObjectiveKind, Operation, OperationPhase, UnitTask, WarObjective};
use crate::ai::trace::{Actor, Reason};
use crate::command::Command;
use crate::ids::{EntityId, PlayerId, ProductionItem, TechId, TileId, UnitKindId};
use crate::knowledge::observation::AIObservation;
use crate::knowledge::public::PlayerQueries;
use crate::rules::CompiledRuleset;
use crate::world::relations::DealItem;
use plans::{Plan, Want};

/// The books (`Variant::book`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Book {
    /// `rush-siege`: the ancient siege rush of the Huns.
    RushSiege,
    /// `rush-archer`: archers timed with Construction.
    RushArcher,
    /// `rush-sword`: Iron Working first, then swordsmen.
    RushSword,
    /// `timing-uu`: the nation's strongest early or medieval unique unit, massed.
    TimingUu,
    /// `rex-wide`: Liberty and as many cities as the land allows.
    RexWide,
}

/// Rounds (Standard speed) a muster waits for its stragglers before the army
/// marches with half of it.
pub const MUSTER_PATIENCE: u32 = 8;
/// Rounds (Standard speed) after which the army marches from wherever it
/// stands: a muster point the walk at peace no longer reaches (borders grow)
/// is reached at war.
pub const MUSTER_LIMIT: u32 = 24;
/// Rounds (Standard speed) the attack lasts from its launch before the book
/// hands the war to `standard`.
pub const ATTACK_ROUNDS: u32 = 75;
/// Failed tries to find a land way to the target before the book gives the
/// attack up (another landmass).
pub const UNREACHABLE_TRIES: u32 = 6;
/// The book presses the siege once this many members stand within
/// `ASSAULT_RADIUS` of the target.
pub const ASSAULT_GROUP: usize = 3;
pub const ASSAULT_RADIUS: i32 = 6;
/// An enemy unit this close to a marching member hands it to the front.
pub const FOE_RADIUS: i32 = 1;
/// A wounded member this close to the target heals there.
pub const FIELD_HEAL_RADIUS: i32 = 8;
/// A melee member strikes a worn city only when it keeps this much health.
pub const MELEE_KEEPS_HP: u16 = 35;
/// The fewest units that form or re-form the attack's operation.
pub const MIN_ARMY: usize = 2;

/// `BOOK_TRACE` is set: the book notes what it sees each turn in its events
/// (the probes of `qa/tests/ai_books.rs`).
fn tracing() -> bool {
    static TRACING: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *TRACING.get_or_init(|| std::env::var_os("BOOK_TRACE").is_some())
}

/// Where a player's book stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// The opening: the build order, the beeline, the army.
    Build,
    /// The army gathers at its staging tile.
    Muster { since: u32 },
    /// At war: the army marches and fights.
    Attack { since: u32 },
    /// The book is over: `standard` plays.
    Over,
}

/// One player's book memory (the mind's, not the saved `AiState`: a book
/// plays bench games, which are never saved).
#[derive(Debug, Clone)]
struct Memory {
    plan: Plan,
    phase: Phase,
    /// The city the attack aims at, and its holder.
    target: Option<(TileId, PlayerId)>,
    /// Failed searches for a land way to the target.
    unreachable: u32,
    /// Offers of peace from the enemy seen this turn (refused while attacking).
    peace_offers: Vec<u32>,
    /// The round a capital was taken (for the tests and the probe).
    captured: Option<u32>,
    /// The first round the army was ready (or forced) while it staged.
    ready: Option<u32>,
    /// What the book did, by round (tests and the probe).
    events: Vec<(u32, String)>,
}

impl Memory {
    fn note(&mut self, round: u32, event: String) {
        self.events.push((round, event));
    }

    fn enter(&mut self, round: u32, phase: Phase) {
        if self.phase != phase {
            self.note(round, format!("{:?} -> {:?}", self.phase, phase));
            self.phase = phase;
        }
    }
}

/// The book of one mind, with each player's memory.
#[derive(Debug)]
pub struct BookPlayer {
    book: Book,
    memory: Mutex<BTreeMap<PlayerId, Memory>>,
}

/// What the book did at the start of a turn.
#[derive(Debug, Default)]
pub struct BookTurn {
    pub decisions: Vec<Decision>,
}

impl BookPlayer {
    pub fn new(book: Book) -> Self {
        Self { book, memory: Mutex::new(BTreeMap::new()) }
    }

    pub fn book(&self) -> Book {
        self.book
    }

    fn with<T>(&self, player: PlayerId, rules: &CompiledRuleset, nation: u8, f: impl FnOnce(&mut Memory) -> T) -> T {
        let mut memory = self.memory.lock().expect("no book thread panicked");
        let entry = memory.entry(player).or_insert_with(|| Memory {
            plan: Plan::of(self.book, rules, nation),
            phase: Phase::Build,
            target: None,
            unreachable: 0,
            peace_offers: Vec::new(),
            captured: None,
            ready: None,
            events: Vec::new(),
        });
        f(entry)
    }

    /// The player's phase (tests, the probe); None before its first turn.
    pub fn phase(&self, player: PlayerId) -> Option<Phase> {
        self.memory.lock().expect("no book thread panicked").get(&player).map(|m| m.phase)
    }

    /// What the player's book did, by round (tests and the probe).
    pub fn events(&self, player: PlayerId) -> Vec<(u32, String)> {
        self.memory.lock().expect("no book thread panicked").get(&player).map_or_else(Vec::new, |m| m.events.clone())
    }

    /// The player's plan (tests); None before its first turn.
    pub fn plan(&self, player: PlayerId) -> Option<Plan> {
        self.memory.lock().expect("no book thread panicked").get(&player).map(|m| m.plan.clone())
    }

    /// The book still plays: `standard` starts no war operation of its own and
    /// its peace with the target waits.
    pub fn holds_war(&self, player: PlayerId) -> bool {
        let memory = self.memory.lock().expect("no book thread panicked");
        memory.get(&player).is_some_and(|m| m.plan.attacks() && m.phase != Phase::Over)
    }

    /// The start of a full turn, after `standard`'s operations: the target,
    /// the phase, the attack's operation formed, launched or formed again.
    pub fn start_turn(&self, sight: &AIObservation<'_>, brief: &TurnBrief, state: &mut AiState, rules: &CompiledRuleset, routes: &mut Routes<'_>, walls: Option<&crate::ai::siege_walls::SiegeWalls>) -> BookTurn {
        let mut turn = BookTurn::default();
        self.with(brief.player, rules, brief.nation, |m| {
            m.peace_offers = peace_offers(sight, m.target.map(|t| t.1));
            if m.phase == Phase::Over {
                return;
            }
            let speed = brief.speed;
            let round = brief.round;
            if let Some(at) = captured_capital(sight) {
                m.captured.get_or_insert(round);
                m.note(round, format!("capital at {} taken", at.0));
                m.enter(round, Phase::Over);
                return;
            }
            remember_capitals(sight, brief, state);
            // The target is kept while the army gathers and fights for it (and while
            // its holder holds it); before that, and once it is taken, the best city
            // of the rival by its value a round of war (`target`).
            let held = m.target.filter(|(tile, owner)| state.seen_cities().iter().any(|c| c.tile == *tile && c.owner == *owner));
            let fighting = matches!(m.phase, Phase::Muster { .. } | Phase::Attack { .. });
            let known = match held {
                Some(kept) if fighting => Some(kept),
                _ => target(sight, brief, state, rules, &m.plan, walls).or(held),
            };
            if known.is_none() && tracing() && round % 10 == 0 {
                let capitals: Vec<_> = sight.view.original_capitals().into_iter().map(|c| (c.founder.0, c.holder.0, c.tile.map(|t| t.0))).collect();
                let seen: Vec<_> = state.seen_cities().iter().map(|c| (c.tile.0, c.owner.0)).collect();
                let majors: Vec<_> = brief.relations.iter().map(|r| r.other.0).collect();
                m.note(round, format!("no target: capitals {capitals:?} seen {seen:?} majors {majors:?} known {}%", brief.known_percent));
            }
            if known != m.target {
                m.note(round, format!("target {:?}", known.map(|(t, p)| (t.0, p.0))));
                m.target = known;
            }
            if !m.plan.attacks() {
                let cities = brief.cities.len() as u32;
                if cities >= m.plan.cities || round > speed.game_round(m.plan.last_round) {
                    m.enter(round, Phase::Over);
                }
                return;
            }
            match m.phase {
                Phase::Build => {
                    if round > speed.game_round(m.plan.last_round) {
                        m.enter(round, Phase::Over);
                        return;
                    }
                    let Some((tile, enemy)) = m.target else { return };
                    if army_count(brief, &m.plan).0 < MIN_ARMY as u32 {
                        return;
                    }
                    // The army stages at the border as it is built: its first
                    // units walk while the capital builds the rest.
                    match form(brief, state, rules, routes, tile, enemy, &m.plan) {
                        Ok(()) => {
                            m.unreachable = 0;
                            m.note(round, format!("staging at the border, {}/{} units", army_count(brief, &m.plan).0, army_count(brief, &m.plan).1));
                            m.enter(round, Phase::Muster { since: round });
                        }
                        Err(why) => {
                            if why == "no land way" {
                                m.note(round, why.to_owned());
                                m.unreachable += 1;
                            }
                        }
                    }
                    if m.unreachable >= UNREACHABLE_TRIES {
                        m.enter(round, Phase::Over);
                    }
                }
                Phase::Muster { since } => {
                    let Some((tile, enemy)) = m.target else {
                        m.enter(round, Phase::Build);
                        return;
                    };
                    if state.operation(tile).is_none() {
                        if let Err(why) = form(brief, state, rules, routes, tile, enemy, &m.plan) {
                            m.note(round, format!("form again: {why}"));
                            // At war already (the enemy struck first): the war goes on.
                            let next = if brief.at_war(enemy) { Phase::Attack { since: round } } else { Phase::Build };
                            m.enter(round, next);
                            return;
                        }
                    }
                    rally(brief, state, rules, tile);
                    let (have, need, half) = army_count(brief, &m.plan);
                    let tech = m.plan.ready_tech.is_none_or(|t| brief.knows(t));
                    let ready = have >= need && tech;
                    let forced = round >= speed.game_round(m.plan.force_round) && have * 3 >= need * 2 && half;
                    let late = round > speed.game_round(m.plan.last_round);
                    if !(ready || forced || late) {
                        return;
                    }
                    let ready_since = *m.ready.get_or_insert(round);
                    let waited = round.saturating_sub(ready_since.max(since));
                    let patience = waited >= speed.pace_rounds(MUSTER_PATIENCE);
                    let (present, members) = muster_count(brief, state, tile);
                    let limit = (late || waited >= speed.pace_rounds(MUSTER_LIMIT)) && members >= MIN_ARMY;
                    if late && members < MIN_ARMY {
                        m.enter(round, Phase::Over);
                        return;
                    }
                    if limit || (assembled(brief, state, tile, patience) && arms_present(brief, state, &m.plan, tile)) {
                        if let Some(decisions) = launch(sight, brief, state, tile, enemy) {
                            m.note(round, format!("launch: army {have}/{need}, {present}/{members} at the muster"));
                            turn.decisions.extend(decisions);
                            m.enter(round, Phase::Attack { since: round });
                        }
                    }
                }
                Phase::Attack { since } => {
                    let over = round.saturating_sub(since) > speed.pace_rounds(ATTACK_ROUNDS);
                    let Some((tile, enemy)) = m.target.filter(|_| !over) else {
                        m.enter(round, Phase::Over);
                        return;
                    };
                    if !brief.at_war(enemy) && round > since + 1 {
                        // The enemy made peace (a deal it forced, a treaty): the war is over.
                        m.enter(round, Phase::Over);
                        return;
                    }
                    if tracing() {
                        let op = state.operation(tile).copied();
                        let far: Vec<(u16, i32, u16, Option<UnitTask>)> = brief.units.iter().filter(|u| army_role(rules, u.kind).is_some()).map(|u| (u.kind.0, brief.dims.distance(u.tile, tile).unwrap_or(-1), u.hp, state.task_of(u.id).map(|t| t.task))).collect();
                        let city = sight.visible_foreign_cities.iter().find(|c| c.tile == tile).map(|c| c.hp);
                        m.note(round, format!("attack {} op {:?} city hp {city:?} units {far:?}", tile.0, op.map(|o| (o.phase, o.muster.0))));
                    }
                    if state.operation(tile).is_some() {
                        rally(brief, state, rules, tile);
                    }
                    match state.operation(tile).copied() {
                        None => {
                            // At war the army marches at once: no muster behind the front.
                            if form(brief, state, rules, routes, tile, enemy, &m.plan).is_ok() {
                                m.note(round, "operation formed again".to_owned());
                                turn.decisions.extend(launch(sight, brief, state, tile, enemy).unwrap_or_default());
                            }
                        }
                        Some(op) if op.phase == OperationPhase::Forming => {
                            let waited = round.saturating_sub(op.phase_round);
                            let patience = waited >= speed.pace_rounds(MUSTER_PATIENCE);
                            let limit = waited >= speed.pace_rounds(MUSTER_LIMIT) && muster_count(brief, state, tile).1 >= MIN_ARMY;
                            if limit || assembled(brief, state, tile, patience) {
                                turn.decisions.extend(launch(sight, brief, state, tile, enemy).unwrap_or_default());
                            }
                        }
                        Some(_) => {}
                    }
                }
                Phase::Over => {}
            }
        });
        turn
    }

    /// A member of the attack at the target's walls presses the siege as a
    /// rushing player does: a unit that strikes from a distance shoots the city
    /// whenever it is in range (setting up first when it must), a melee unit
    /// strikes it when the strike takes it or the city is worn down to half and
    /// the unit survives well; with the group at the walls the members close in.
    /// None: `standard`'s decision (the front gathers, heals, fights units).
    pub fn assault(&self, view: &crate::ai::planner::UnitView<'_, '_>, state: &AiState, tools: &mut crate::ai::planner::PlanTools<'_>) -> Option<Decision> {
        let local = view.local;
        let unit = local.unit;
        let target = match state.task_of(unit.id).map(|t| t.task) {
            Some(UnitTask::Operation { target }) => target,
            _ => return None,
        };
        let attacking = {
            let memory = self.memory.lock().expect("no book thread panicked");
            memory.get(&local.player).is_some_and(|m| matches!(m.phase, Phase::Attack { .. }) && m.target.is_some_and(|(t, _)| t == target))
        };
        if !attacking {
            return None;
        }
        Self::press(view, state, tools, target)
    }

/// The press of the siege by a member of an operation against `target` that
/// marches at war (`BookPlayer::assault`, and `siege-walls`: `standard` with
/// it, `press_siege`): it walks on in legs past `operations`' cohesion waits,
/// heals by the walls, comes close enough to see them once the group is
/// there, shoots the city from range and strikes a worn one. None: `standard`
/// decides (the front fights the units near it).
fn press(view: &crate::ai::planner::UnitView<'_, '_>, state: &AiState, tools: &mut crate::ai::planner::PlanTools<'_>, target: TileId) -> Option<Decision> {
        let local = view.local;
        let unit = local.unit;
        let brief = view.brief;
        if unit.moves_left == 0 {
            return None;
        }
        let dims = local.view.dimensions();
        let distance = dims.distance(unit.tile, target)?;
        let actor = Actor::Unit(unit.id);
        let strike_range = crate::combat::city::CITY_ATTACK_RANGE;
        if unit.hp < HEAL_BELOW_HP {
            // Wounded near the walls: heal there, out of the city's strike, not a
            // long walk home and back.
            if distance > FIELD_HEAL_RADIUS {
                return None;
            }
            if distance <= strike_range {
                return moved(crate::ai::planner_units::approach_at(view, tools, target, strike_range + 1, false, Reason::Retreat));
            }
            let fortify = Command::Fortify { unit: unit.handle };
            return Some(if local.view.accepts(&fortify) { Decision::act(actor, Reason::Heal, fortify) } else { Decision::refuse(actor, Reason::Heal) });
        }
        if !unit.can_attack {
            return None;
        }
        // On the way: every member walks on. `operations`' march holds a member
        // ahead of the others' median, and in a pass one tile wide the leaders'
        // wait blocks the followers for good (the army stood still twenty-five
        // rounds, 21 to 26 tiles out). With an enemy near, the front decides.
        let foe_near = local.foreign_units.iter().any(|f| local.relation(f.owner).is_some_and(|r| r.at_war) && dims.distance(f.tile, unit.tile).is_some_and(|d| d <= FOE_RADIUS));
        if distance > ASSAULT_RADIUS && !foe_near {
            return walk_toward(view, tools, target);
        }
        let at_walls = brief
            .units
            .iter()
            .filter(|u| state.task_of(u.id).map(|t| t.task) == Some(UnitTask::Operation { target }))
            .filter(|u| dims.distance(u.tile, target).is_some_and(|d| d <= ASSAULT_RADIUS))
            .count();
        if at_walls < ASSAULT_GROUP {
            return None;
        }
        let rules = tools.rules;
        let Some(city) = local.foreign_cities.iter().find(|c| c.tile == target) else {
            // The walls are out of sight: come close enough to see them.
            let range = crate::ai::combat::ranged_range(rules, unit.kind).unwrap_or(2).min(2);
            let step = moved(crate::ai::planner_units::approach_at(view, tools, target, range, false, Reason::Advance));
            if step.is_none() && brief.fog_march {
                return moved(crate::ai::planner_units::fog_approach(view, tools, target, range, Reason::Advance));
            }
            return step;
        };
        match crate::ai::combat::ranged_range(rules, unit.kind) {
            Some(range) => {
                if distance <= range {
                    let shot = Command::RangedAttack { unit: unit.handle, target: city.tile };
                    if local.view.accepts(&shot) {
                        return Some(Decision::act(actor, Reason::AttackCity, shot));
                    }
                    let set_up = Command::SetUpRangedAttack { unit: unit.handle };
                    if local.view.accepts(&set_up) {
                        return Some(Decision::act(actor, Reason::AttackCity, set_up));
                    }
                    return None;
                }
                let step = crate::ai::planner_units::approach_at(view, tools, city.tile, range, false, Reason::Advance);
                moved(step)
            }
            None => {
                let worn = u32::from(city.hp) * 2 <= u32::from(crate::world::city::CITY_BASE_HIT_POINTS);
                if distance == 1 {
                    let preview = local.view.attack_preview_from(unit.handle, unit.tile, city.tile)?;
                    let takes = preview.to_defender.1 >= city.hp;
                    let survives = unit.hp.saturating_sub(preview.to_attacker.1) >= MELEE_KEEPS_HP;
                    let strike = Command::AttackCity { attacker: unit.handle, city: city.handle };
                    if (takes || (worn && survives)) && local.view.accepts(&strike) {
                        return Some(Decision::act(actor, Reason::AttackCity, strike));
                    }
                    return None;
                }
                if worn { moved(crate::ai::planner_units::approach(view, tools, city.tile, Reason::Advance)) } else { None }
            }
        }
    }

    /// The decisions of the turn with the book's peace held back: no peace with
    /// the target while the army gathers or fights.
    pub fn hold_peace(&self, player: PlayerId, decisions: Vec<Decision>) -> Vec<Decision> {
        let memory = self.memory.lock().expect("no book thread panicked");
        let Some(m) = memory.get(&player) else { return decisions };
        let fighting = matches!(m.phase, Phase::Muster { .. } | Phase::Attack { .. });
        let Some((_, enemy)) = m.target.filter(|_| fighting) else { return decisions };
        decisions
            .into_iter()
            .filter(|d| match &d.command {
                Some(Command::MakePeace { enemy: e, .. }) => *e != enemy,
                Some(Command::ProposeDeal { to, items, .. }) => !(*to == enemy && items.iter().any(|(_, i)| matches!(i, DealItem::Peace))),
                Some(Command::RespondDeal { offer, accept: true, .. }) => !m.peace_offers.contains(offer),
                _ => true,
            })
            .collect()
    }

    /// The strategic decisions with the book's research: while its beeline is
    /// unfinished, the next technology of it in place of `standard`'s choice.
    pub fn strategic(&self, brief: &TurnBrief, rules: &CompiledRuleset, decisions: Vec<Decision>) -> Vec<Decision> {
        let decisions = self.hold_peace(brief.player, decisions);
        let next = self.with(brief.player, rules, brief.nation, |m| {
            if m.phase == Phase::Over {
                return None;
            }
            m.plan.techs.iter().copied().find(|t| !brief.knows(*t))
        });
        let Some(next) = next else { return decisions };
        let mut out: Vec<Decision> = decisions.into_iter().filter(|d| !matches!(d.command, Some(Command::ChooseTechnology { .. } | Command::PlanResearch { .. }))).collect();
        if brief.research_queue.last() != Some(&next) {
            out.push(Decision::act(Actor::Player, Reason::Research, Command::PlanResearch { player: brief.player, target: next }));
        }
        out
    }

    /// The end of the turn with the book's production, policies and free
    /// technology in place of `standard`'s where the book has one.
    pub fn end_of_turn(&self, sight: &AIObservation<'_>, brief: &TurnBrief, state: &AiState, rules: &CompiledRuleset, decisions: Vec<Decision>) -> Vec<Decision> {
        let decisions = self.hold_peace(brief.player, decisions);
        let (plan, phase, target) = self.with(brief.player, rules, brief.nation, |m| (m.plan.clone(), m.phase, m.target));
        if phase == Phase::Over {
            return decisions;
        }
        let orders = production(sight, brief, state, rules, &plan, phase, target.is_some());
        // The cities that build the army work their production tiles, as a rushing
        // player sets them.
        let focused: Vec<(u32, u32)> = if plan.attacks() && phase != Phase::Over {
            orders.iter().filter(|(_, (reason, _))| *reason == Reason::ProduceArmy).map(|(k, _)| *k).collect()
        } else {
            Vec::new()
        };
        let policy = policy(sight, rules, &plan);
        let free_tech = free_technology(sight, rules, &plan, brief);
        let produced: Vec<_> = orders.keys().copied().collect();
        let mut out: Vec<Decision> = decisions
            .into_iter()
            .filter(|d| match &d.command {
                Some(Command::EnqueueProduction { city, .. } | Command::ClearProduction { city, .. } | Command::HurryProduction { city, .. }) => !produced.contains(&(city.index, city.generation)),
                Some(Command::AdoptPolicy { .. } | Command::UnlockPolicyBranch { .. }) => policy.is_none(),
                Some(Command::ChooseFreeTechnology { .. }) => free_tech.is_none(),
                Some(Command::SetCityFocus { city, .. }) => !focused.contains(&(city.index, city.generation)),
                _ => true,
            })
            .collect();
        for city in sight.own_cities.iter().filter(|c| focused.contains(&(c.handle.index, c.handle.generation))) {
            let focus = crate::world::citizens::CityFocus::Production;
            if city.citizens.focus != focus {
                out.push(Decision::act(Actor::City(city.id), Reason::SystemAdvisor, Command::SetCityFocus { player: brief.player, city: city.handle, focus }));
            }
        }
        out.extend(orders.into_values().flat_map(|(_, o)| o));
        out.extend(policy.map(|c| Decision::act(Actor::Player, Reason::SystemAdvisor, c)));
        out.extend(free_tech.map(|c| Decision::act(Actor::Player, Reason::Research, c)));
        out
    }
}

/// `siege-walls` (`standard` with the book's siege press, a candidate for the
/// game's AI; `Variant::siege_walls`): a member of an operation that marches at
/// war presses its siege as a book's attack does (`BookPlayer::press`).
pub fn press_siege(view: &crate::ai::planner::UnitView<'_, '_>, state: &AiState, tools: &mut crate::ai::planner::PlanTools<'_>) -> Option<Decision> {
    let target = match state.task_of(view.local.unit.id).map(|t| t.task) {
        Some(UnitTask::Operation { target }) => target,
        _ => return None,
    };
    let op = state.operation(target)?;
    if op.phase != OperationPhase::Marching || !view.brief.at_war(op.enemy) {
        return None;
    }
    BookPlayer::press(view, state, tools, target)
}

/// The offers of peace from `enemy` addressed to the player.
fn peace_offers(sight: &AIObservation<'_>, enemy: Option<PlayerId>) -> Vec<u32> {
    let Some(enemy) = enemy else { return Vec::new() };
    sight.offers_to_us.iter().filter(|o| o.from == enemy && o.items.iter().any(|(_, i)| matches!(i, DealItem::Peace))).map(|o| o.id).collect()
}

/// A rival's original capital the player holds.
fn captured_capital(sight: &AIObservation<'_>) -> Option<TileId> {
    sight.view.original_capitals().into_iter().find(|c| c.founder != sight.player && c.holder == sight.player).and_then(|c| c.tile)
}

/// A rival nation's original capital whose tile the player knows (explored,
/// or shown by an embassy) but never saw with its city: remembered as a city
/// of its holder, of our capital's size (the player's estimate of a city of
/// the same age), so an operation may aim at it (`operations` ends one whose
/// target is not remembered).
fn remember_capitals(sight: &AIObservation<'_>, brief: &TurnBrief, state: &mut AiState) {
    let majors: Vec<PlayerId> = brief.relations.iter().map(|r| r.other).collect();
    let size = brief.cities.first().map_or(1, |c| c.population.max(1));
    for capital in sight.view.original_capitals() {
        let Some(tile) = capital.tile else { continue };
        if capital.holder == brief.player || !majors.contains(&capital.holder) || state.seen_cities().iter().any(|c| c.tile == tile) {
            continue;
        }
        state.see_city(crate::ai::state::SeenCity { tile, owner: capital.holder, population: size, last_seen_round: brief.round });
    }
}

/// The rival the book fights is the holder of the nearest rival original
/// capital the player knows and remembers in its holder's hands (a nation's: a
/// city-state's is no target); its target is the city of that rival worth
/// most a round of war to the book's army (`siege_walls::rate_target`: the
/// capture's value by its chance under the city's strike and healing, over
/// the march and the siege): a weak city near the front before a walled
/// capital, a fortified city only for an army with a shooter that outranges
/// its strike or when it beats the others by `siege_walls::CLEAR_MARGIN`. With
/// no city worth it, the capital (a classic rush's aim). While no capital is
/// known, the nearest city of a rival nation it remembers (the army finds the
/// capital on the way).
fn target(sight: &AIObservation<'_>, brief: &TurnBrief, state: &AiState, rules: &CompiledRuleset, plan: &Plan, walls: Option<&crate::ai::siege_walls::SiegeWalls>) -> Option<(TileId, PlayerId)> {
    let home = brief.cities.first().map(|c| c.tile)?;
    let majors: Vec<PlayerId> = brief.relations.iter().map(|r| r.other).collect();
    let near = |tile: &TileId| (brief.dims.distance(home, *tile).unwrap_or(i32::MAX), tile.0);
    let capital = sight
        .view
        .original_capitals()
        .into_iter()
        .filter(|c| c.holder != brief.player && majors.contains(&c.holder))
        .filter_map(|c| c.tile.map(|t| (t, c.holder)))
        .filter(|(tile, holder)| state.seen_cities().iter().any(|s| s.tile == *tile && s.owner == *holder))
        .min_by_key(|(tile, _)| near(tile));
    let Some((capital_tile, rival)) = capital else {
        return state.seen_cities().iter().filter(|c| majors.contains(&c.owner)).map(|c| (c.tile, c.owner)).min_by_key(|(tile, _)| near(tile));
    };
    // The book's army: each slot's first kind, as many as the slot holds.
    let army: Vec<UnitKindId> = plan.army.iter().flat_map(|s| s.kinds.first().copied().into_iter().cycle().take(s.count as usize)).collect();
    let slowest = army.iter().map(|k| i32::from(rules.units[k.0 as usize].movement.max(1))).min().unwrap_or(1);
    let rated: Vec<(f64, bool, TileId)> = state
        .seen_cities()
        .iter()
        .filter(|c| c.owner == rival)
        .filter_map(|c| {
            let from = brief.nearest_city(brief.dims, c.tile)?;
            let walk = brief.dims.distance(from.tile, c.tile)?;
            let march = ((walk + slowest - 1) / slowest).max(1) as u32;
            let (rate, unanswered) = crate::ai::siege_walls::rate_target(brief, rules, walls, c, &army, march);
            Some((rate, unanswered, c.tile))
        })
        .collect();
    let best_other = |tile: TileId| rated.iter().filter(|(_, _, t)| *t != tile).map(|(r, _, _)| *r).fold(0.0f64, f64::max);
    let best = rated
        .iter()
        .filter(|(rate, unanswered, tile)| *rate > 0.0 && (!unanswered || *rate >= crate::ai::siege_walls::CLEAR_MARGIN * best_other(*tile)))
        .max_by(|a, b| a.0.total_cmp(&b.0).then(near(&b.2).cmp(&near(&a.2))))
        .map(|(_, _, tile)| *tile);
    Some((best.unwrap_or(capital_tile), rival))
}

/// The army's units alive, each counted in the first slot that has room for
/// it, and the army's size; with `half`, whether every slot holds at least
/// half its count (a forced attack goes with every arm: a siege rush without
/// its siege units does not dent a capital).
fn army_count(brief: &TurnBrief, plan: &Plan) -> (u32, u32, bool) {
    let mut room: Vec<u32> = plan.army.iter().map(|s| s.count).collect();
    let mut have = 0;
    for unit in &brief.units {
        if let Some(at) = plan.army.iter().enumerate().position(|(i, s)| room[i] > 0 && s.core.contains(&unit.kind)) {
            room[at] -= 1;
            have += 1;
        }
    }
    let half = plan.army.iter().zip(&room).all(|(s, left)| (s.count - left) * 2 >= s.count);
    (have, plan.army.iter().map(|s| s.count).sum(), half)
}

/// The lowest-id unit holding each city: it stays home.
fn home_guards(brief: &TurnBrief, state: &AiState, rules: &CompiledRuleset) -> Vec<EntityId> {
    let mut out = Vec::new();
    for city in &brief.cities {
        let holder = brief
            .units
            .iter()
            .filter(|u| army_role(rules, u.kind).is_some())
            .filter(|u| u.tile == city.tile || state.task_of(u.id).map(|t| t.task) == Some(UnitTask::Defend { city: city.tile }))
            .map(|u| u.id)
            .min();
        out.extend(holder);
    }
    out
}

/// The units that go: healthy army units without a civil task, but each
/// city's guard and the guards of a city in danger.
fn attackers<'b>(brief: &'b TurnBrief, state: &AiState, rules: &CompiledRuleset, target: TileId) -> Vec<&'b UnitBrief> {
    let guards = home_guards(brief, state, rules);
    brief
        .units
        .iter()
        .filter(|u| army_role(rules, u.kind).is_some() && u.hp >= HEAL_BELOW_HP && !guards.contains(&u.id))
        .filter(|u| match state.task_of(u.id).map(|t| t.task) {
            None | Some(UnitTask::Assault { .. }) | Some(UnitTask::Heal { .. }) => true,
            Some(UnitTask::Operation { target: t }) => t == target,
            Some(UnitTask::Defend { city }) => !brief.endangered(city),
            // A fighter scouting for the empire goes too (recon units are no army units).
            Some(UnitTask::Scout { .. }) => true,
            Some(UnitTask::Settle { .. } | UnitTask::Escort { .. } | UnitTask::Improve { .. } | UnitTask::Raid { .. }) => false,
        })
        .collect()
}

/// Every attacker joins the running operation against `target` again: the
/// healed, the garrisons no city in danger needs, the new units (`standard`
/// gives a unit that left to heal another task, and its recruiting takes only
/// the units on an assault once the army marches).
fn rally(brief: &TurnBrief, state: &mut AiState, rules: &CompiledRuleset, target: TileId) {
    let joining: Vec<EntityId> = attackers(brief, state, rules, target)
        .into_iter()
        .filter(|u| state.task_of(u.id).map(|t| t.task) != Some(UnitTask::Operation { target }))
        .map(|u| u.id)
        .collect();
    for unit in joining {
        state.set_task(unit, UnitTask::Operation { target }, brief.round);
    }
}

/// Form the attack's operation against `target`: the attackers, mustered on
/// the staging tile of a land way from our nearest city. False: too few
/// units or no land way.
fn form(brief: &TurnBrief, state: &mut AiState, rules: &CompiledRuleset, routes: &mut Routes<'_>, target: TileId, enemy: PlayerId, plan: &Plan) -> Result<(), &'static str> {
    let members: Vec<UnitBrief> = attackers(brief, state, rules, target).into_iter().copied().collect();
    if members.len() < MIN_ARMY {
        return Err("too few units");
    }
    let walker = members
        .iter()
        .find(|u| army_role(rules, u.kind) == Some(crate::ai::state::ArmyRole::Melee))
        .or(members.first())
        .map(|u| u.kind)
        .expect("two members");
    let Some(home) = brief.nearest_city(brief.dims, target) else { return Err("no city") };
    // A land way must lead to the target's border; the army gathers at home, in
    // our city nearest to the target, and walks the way at war: a muster point
    // out at the border was cut off by the borders as they grew (the members
    // walked for twenty rounds and never arrived).
    if border_stage(routes, walker, home.tile, target, enemy).is_none() {
        return Err("no land way");
    }
    let muster = home.tile;
    // The need is the book's whole army (and at least the members): `standard`'s
    // own launch (`operations::advance`) waits for it as for its own armies.
    let mut need = Composition::default();
    for slot in &plan.army {
        if let Some(role) = slot.kinds.first().and_then(|k| army_role(rules, *k)) {
            for _ in 0..slot.count {
                need.add(role);
            }
        }
    }
    let mut members_need = Composition::default();
    for unit in &members {
        if let Some(role) = army_role(rules, unit.kind) {
            members_need.add(role);
        }
    }
    if members_need.total() > need.total() {
        need = members_need;
    }
    if let Some(old) = state.operations().iter().map(|o| o.target).find(|t| *t != target) {
        state.end_operation(old);
    }
    if state.operation(target).is_some() {
        state.end_operation(target);
    }
    state.start_operation(Operation { target, enemy, muster, need, phase: OperationPhase::Forming, since_round: brief.round, phase_round: brief.round });
    for unit in &members {
        state.set_task(unit.id, UnitTask::Operation { target }, brief.round);
    }
    Ok(())
}

/// The farthest a muster point may lie from its target.
pub const MUSTER_REACH: i32 = 14;

/// Where the army gathers for `target`: the tile nearest to it, out of its
/// strike, that a walk from `from` over our own and unowned land reaches (at
/// peace another nation's closed borders stop the army: the staging tile of
/// `operations::Routes::stage` may lie beyond them, and the members never reach
/// it). None: no such tile within `MUSTER_REACH` of the target (another
/// landmass, a neighbour's land between, or beyond the search). `enemy` is
/// the target's holder: its land is walked once at war.
fn border_stage(routes: &Routes<'_>, kind: UnitKindId, from: TileId, target: TileId, enemy: PlayerId) -> Option<TileId> {
    use crate::ai::planner_map::{explore, Blockers, KnownMap, MapScratch, SearchStats, Visit};
    let _ = enemy;
    let view = routes.view();
    let me = view.player();
    let map = KnownMap::of_kind(view, kind);
    let dims = view.dimensions();
    let foreign: Vec<TileId> = (0..view.tile_count() as u32)
        .map(TileId)
        .filter(|t| view.tile(*t).snapshot().and_then(|s| s.owner).is_some_and(|o| o != me))
        .collect();
    let mut best: Option<(i32, u32, TileId)> = None;
    let mut visit = |tile: TileId, points: u32| {
        let distance = dims.distance(tile, target).unwrap_or(i32::MAX);
        if distance >= crate::ai::operations::STAGING_DISTANCE && best.is_none_or(|b| (distance, points, tile) < b) {
            best = Some((distance, points, tile));
        }
        Visit::Continue
    };
    let limit = crate::ai::planner::DecisionBudget::default().search();
    explore(&map, from, &Blockers::new(foreign), &mut MapScratch::default(), limit, &mut SearchStats::default(), &mut visit);
    best.filter(|(d, _, _)| *d <= MUSTER_REACH).map(|(_, _, tile)| tile)
}

/// The operation's members at its muster point, and all its members.
fn muster_count(brief: &TurnBrief, state: &AiState, target: TileId) -> (usize, usize) {
    let Some(op) = state.operation(target) else { return (0, 0) };
    let members: Vec<&UnitBrief> = brief.units.iter().filter(|u| state.task_of(u.id).map(|t| t.task) == Some(UnitTask::Operation { target })).collect();
    let present = members.iter().filter(|u| u.hp >= HEAL_BELOW_HP && brief.dims.distance(u.tile, op.muster).is_some_and(|d| d <= MUSTER_RADIUS + 1)).count();
    (present, members.len())
}

/// A member's step toward `target`: the path there, or, when the target is
/// beyond the path search's budget, the path to the known land tile nearest
/// to it within `WAYPOINT_RADIUS` (a long march walked in legs). None: no step
/// (`standard` decides).
fn walk_toward(view: &crate::ai::planner::UnitView<'_, '_>, tools: &mut crate::ai::planner::PlanTools<'_>, target: TileId) -> Option<Decision> {
    let step = crate::ai::planner_units::approach(view, tools, target, Reason::Advance);
    if step.command.is_some() || step.reason == Reason::Waiting {
        return Some(step);
    }
    if view.brief.fog_march {
        // No known way: on through the fog (`fog_approach`). The legs below aim at the
        // known tiles nearest the target, and the tiles an embassy shows around it are
        // known but cut off by the fog: the army stood where they came within reach.
        if let Some(step) = moved(crate::ai::planner_units::fog_approach(view, tools, target, 1, Reason::Advance)) {
            return Some(step);
        }
    }
    let local = view.local;
    let dims = local.view.dimensions();
    let here = local.unit.tile;
    let mut legs: Vec<TileId> = crate::math::hex_disc(dims, here, WAYPOINT_RADIUS)
        .into_iter()
        .filter(|t| *t != here && local.view.is_known(*t) && !crate::ai::fleet::water(&local.view, *t))
        .collect();
    legs.sort_by_key(|t| (dims.distance(*t, target).unwrap_or(i32::MAX), dims.distance(*t, here).unwrap_or(i32::MAX), t.0));
    let closer = dims.distance(here, target).unwrap_or(i32::MAX);
    legs.into_iter()
        .take_while(|t| dims.distance(*t, target).unwrap_or(i32::MAX) < closer)
        .take(WAYPOINT_TRIES)
        .map(|t| crate::ai::planner_units::move_along(view, tools, t, Reason::Advance))
        .find(|d| d.command.is_some())
}

/// A move decision, or None when it moves nothing (no path, over the search's
/// budget): `standard` decides then.
fn moved(decision: Decision) -> Option<Decision> {
    (decision.command.is_some() || decision.reason == Reason::Waiting).then_some(decision)
}

/// A long march's leg reaches this far; this many leg ends are tried.
pub const WAYPOINT_RADIUS: i32 = 8;
pub const WAYPOINT_TRIES: usize = 4;

/// The book's arms present at the muster point: every slot of the army with
/// at least half its count there (the slow siege units come last; an army
/// that marches without them does not dent a capital).
fn arms_present(brief: &TurnBrief, state: &AiState, plan: &Plan, target: TileId) -> bool {
    let Some(op) = state.operation(target) else { return false };
    let mut room: Vec<u32> = plan.army.iter().map(|s| s.count).collect();
    for unit in brief.units.iter().filter(|u| state.task_of(u.id).map(|t| t.task) == Some(UnitTask::Operation { target })) {
        let near = unit.hp >= HEAL_BELOW_HP && brief.dims.distance(unit.tile, op.muster).is_some_and(|d| d <= MUSTER_RADIUS + 1);
        if !near {
            continue;
        }
        if let Some(at) = plan.army.iter().enumerate().position(|(i, s)| room[i] > 0 && s.core.contains(&unit.kind)) {
            room[at] -= 1;
        }
    }
    plan.army.iter().zip(&room).all(|(s, left)| (s.count - left) * 2 >= s.count)
}

/// The operation's healthy members stand at its muster point: all of them, or
/// half after `patience`.
fn assembled(brief: &TurnBrief, state: &AiState, target: TileId, patience: bool) -> bool {
    let Some(op) = state.operation(target) else { return false };
    let members: Vec<&UnitBrief> = brief.units.iter().filter(|u| state.task_of(u.id).map(|t| t.task) == Some(UnitTask::Operation { target })).collect();
    if members.is_empty() {
        return false;
    }
    let present = members.iter().filter(|u| u.hp >= HEAL_BELOW_HP && brief.dims.distance(u.tile, op.muster).is_some_and(|d| d <= MUSTER_RADIUS + 1)).count();
    present == members.len() || (patience && present * 2 >= members.len() && present >= MIN_ARMY)
}

/// March: the war declared first when at peace (when the rules allow it now),
/// the capital the war's objective. None: the rules refuse the war now.
fn launch(sight: &AIObservation<'_>, brief: &TurnBrief, state: &mut AiState, target: TileId, enemy: PlayerId) -> Option<Vec<Decision>> {
    let mut out = Vec::new();
    if !brief.at_war(enemy) {
        let command = Command::DeclareWar { player: brief.player, enemy };
        if !sight.view.accepts(&command) {
            return None;
        }
        out.push(Decision::act(Actor::Player, Reason::DeclareWar, command));
    }
    state.relation_mut(enemy).objective = Some(WarObjective { tile: target, kind: ObjectiveKind::City, chosen_round: brief.round });
    state.set_operation_phase(target, OperationPhase::Marching, brief.round);
    out.push(Decision::refuse(Actor::Player, Reason::LaunchOperation));
    Some(out)
}

/// Units of `kinds` alive and queued in the cities but `except`.
fn counted(sight: &AIObservation<'_>, kinds: &[UnitKindId], except: Option<crate::ids::CityHandle>, ordered: &[UnitKindId]) -> u32 {
    let alive = sight.own_units.iter().filter(|u| kinds.contains(&u.kind)).count();
    let queued = sight
        .own_cities
        .iter()
        .filter(|c| Some(c.handle) != except)
        .flat_map(|c| c.queue.iter())
        .filter(|i| matches!(i, ProductionItem::Unit(k) if kinds.contains(k)))
        .count();
    (alive + queued + ordered.iter().filter(|k| kinds.contains(k)).count()) as u32
}

/// The book's orders for each city it builds in, by the city's (index,
/// generation): the first step of the build order not met that the city may
/// build now. A city in danger, and a city with no step to take, builds what
/// `standard` orders.
fn production(
    sight: &AIObservation<'_>,
    brief: &TurnBrief,
    state: &AiState,
    rules: &CompiledRuleset,
    plan: &Plan,
    phase: Phase,
    target_known: bool,
) -> BTreeMap<(u32, u32), (Reason, Vec<Decision>)> {
    let _ = state;
    let nation = brief.nation;
    let settler = Plan::settler(rules, nation);
    let worker = Plan::worker(rules, nation);
    let recon = Plan::recon(rules, nation);
    let recon_kinds: Vec<UnitKindId> = (0..rules.units.len())
        .filter(|i| rules.units[*i].role == Some(crate::rules::content::UnitRole::Recon))
        .map(|i| UnitKindId(i as u16))
        .collect();
    let workers: Vec<UnitKindId> = (0..rules.units.len()).filter(|i| rules.units[*i].builder && rules.units[*i].domain == crate::rules::content::Domain::Land).map(|i| UnitKindId(i as u16)).collect();
    let founders: Vec<UnitKindId> = (0..rules.units.len()).filter(|i| rules.units[*i].founder).map(|i| UnitKindId(i as u16)).collect();
    let cities = sight.own_cities.len() as u32;
    let attacking = matches!(phase, Phase::Attack { .. } | Phase::Muster { .. });
    let mut ordered: Vec<UnitKindId> = Vec::new();
    let mut out = BTreeMap::new();
    let mut list: Vec<&crate::knowledge::observation::OwnCity> = sight.own_cities.iter().collect();
    list.sort_by_key(|c| (std::cmp::Reverse(c.income.production), c.id));
    for city in list {
        if brief.endangered(city.tile) {
            continue;
        }
        let accepts = |kind: UnitKindId| sight.view.accepts(&Command::EnqueueProduction { player: brief.player, city: city.handle, item: ProductionItem::Unit(kind) });
        let mut wanted: Option<(UnitKindId, Reason)> = None;
        // While attacking: the army's losses and the reinforcements first.
        let steps: Vec<Want> = if attacking { (0..plan.army.len()).map(Want::Army).collect() } else { plan.steps.clone() };
        for (at, step) in steps.iter().enumerate() {
            let pick = match *step {
                Want::Recon(n) if !target_known => recon.filter(|_| counted(sight, &recon_kinds, Some(city.handle), &ordered) < n).map(|k| (k, Reason::ProduceArmy)),
                Want::Recon(_) => None,
                Want::Workers(n) => worker.filter(|_| counted(sight, &workers, Some(city.handle), &ordered) < n).map(|k| (k, Reason::ProduceWorker)),
                Want::WorkersPerCities(d) => worker.filter(|_| counted(sight, &workers, Some(city.handle), &ordered) < cities.div_ceil(d.max(1)).max(1)).map(|k| (k, Reason::ProduceWorker)),
                Want::Settle(n) => settler
                    .filter(|_| city.population >= 2 && cities + counted(sight, &founders, Some(city.handle), &ordered) < n.min(plan.cities))
                    .map(|k| (k, Reason::ProduceSettler)),
                Want::Army(slot) => {
                    let s = &plan.army[slot];
                    let extra = if attacking && slot == 0 { plan.reinforce } else { 0 };
                    // Earlier slots of the same kinds count first (`army_count`).
                    let earlier: u32 = plan.army[..slot].iter().filter(|e| e.core.iter().any(|k| s.core.contains(k))).map(|e| e.count).sum();
                    let have = counted(sight, &s.core, Some(city.handle), &ordered).saturating_sub(earlier);
                    if have < s.count + extra {
                        // The core first; a fallback only when no core kind is on its way
                        // (its technology neither known nor on the beeline).
                        let coming = s.core.iter().any(|k| rules.units[k.0 as usize].required_tech.is_some_and(|t| !brief.knows(t) && on_beeline(rules, plan, brief, t)));
                        s.core.iter().copied().find(|k| accepts(*k)).or_else(|| if coming { None } else { s.kinds.iter().copied().find(|k| accepts(*k)) }).map(|k| (k, Reason::ProduceArmy))
                    } else {
                        None
                    }
                }
            };
            let _ = at;
            if let Some((kind, reason)) = pick.filter(|(k, _)| accepts(*k)) {
                wanted = Some((kind, reason));
                break;
            }
        }
        let Some((kind, reason)) = wanted else { continue };
        ordered.push(kind);
        let item = ProductionItem::Unit(kind);
        let actor = Actor::City(city.id);
        let mut orders = Vec::new();
        if city.queue.first() != Some(&item) {
            if !city.queue.is_empty() {
                orders.push(Decision::act(actor, reason, Command::ClearProduction { player: brief.player, city: city.handle }));
            }
            orders.push(Decision::act(actor, reason, Command::EnqueueProduction { player: brief.player, city: city.handle, item }));
        } else if reason == Reason::ProduceArmy && brief.gold_income >= 0 {
            // The rush buys its army: the head is the book's unit.
            let hurry = Command::HurryProduction { player: brief.player, city: city.handle };
            if sight.view.accepts(&hurry) {
                orders.push(Decision::act(actor, Reason::Hurry, hurry));
            }
        }
        out.insert((city.handle.index, city.handle.generation), (reason, orders));
    }
    out
}

/// The book's next culture decision: in the first branch of its line that is
/// unlocked and has a policy left, that policy (the lowest row first); else the
/// first branch of the line that may be unlocked. None: `standard`'s.
fn policy(sight: &AIObservation<'_>, rules: &CompiledRuleset, plan: &Plan) -> Option<Command> {
    let culture = &sight.culture;
    for branch in &plan.branches {
        if culture.branches.contains(branch) {
            let pick = culture
                .adoptable
                .iter()
                .copied()
                .filter(|p| rules.module.policies.get(p.0 as usize).and_then(|d| d.branch) == Some(*branch))
                .min_by_key(|p| (rules.module.policies[p.0 as usize].grid.1, rules.module.policies[p.0 as usize].grid.0, p.0));
            if let Some(policy) = pick {
                let command = Command::AdoptPolicy { player: sight.player, policy };
                if sight.view.accepts(&command) {
                    return Some(command);
                }
            }
            continue;
        }
        if culture.unlockable.contains(branch) {
            let command = Command::UnlockPolicyBranch { player: sight.player, branch: *branch };
            if sight.view.accepts(&command) {
                return Some(command);
            }
        }
        // A branch of the line not open yet holds the policies back for it.
        return None;
    }
    None
}

/// A free technology owed: the one on the beeline's way (its target or a
/// prerequisite) that costs most. None: `standard`'s choice.
fn free_technology(sight: &AIObservation<'_>, rules: &CompiledRuleset, plan: &Plan, brief: &TurnBrief) -> Option<Command> {
    let options = &sight.economy.free_technology_options;
    if options.is_empty() {
        return None;
    }
    let target = plan.techs.iter().copied().find(|t| !brief.knows(*t))?;
    let way = prerequisites(rules, target, brief);
    options
        .iter()
        .copied()
        .filter(|t| way.contains(t))
        .max_by_key(|t| (rules.technologies[t.0 as usize].cost, std::cmp::Reverse(t.0)))
        .map(|tech| Command::ChooseFreeTechnology { player: sight.player, tech })
        .filter(|c| sight.view.accepts(c))
}

/// `tech` is on the book's beeline: one of its technologies or a prerequisite
/// of one not known yet.
fn on_beeline(rules: &CompiledRuleset, plan: &Plan, brief: &TurnBrief, tech: TechId) -> bool {
    plan.techs.iter().any(|t| !brief.knows(*t) && prerequisites(rules, *t, brief).contains(&tech))
}

/// `target` and its unknown prerequisites.
fn prerequisites(rules: &CompiledRuleset, target: TechId, brief: &TurnBrief) -> Vec<TechId> {
    let mut out = Vec::new();
    let mut open = vec![target];
    while let Some(tech) = open.pop() {
        if brief.knows(tech) || out.contains(&tech) {
            continue;
        }
        out.push(tech);
        open.extend(rules.technologies[tech.0 as usize].requires.iter().copied());
    }
    out
}
