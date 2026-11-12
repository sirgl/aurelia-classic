//! The landless arenas of the exam (L1, L2, L3a, L3b, L4; added 11 Nov, the older scenes unchanged): a player with
//! units and no city and no settler. A player that never had a city lives while it has a unit
//! (`kernel systems/elimination.rs`), a captured settler becomes a worker, civilians are captured by a melee unit
//! that attacks their tile, a city-state's land is open to everybody at peace, a nation's is not. A measurement:
//! it changes no rule and no AI.
//!
//! WORLD (`landless_world`): the suite game on the minimal map (two nations) WITH the size's city-states and
//! without barbarians, a drawn map script as `fit_world` draws them; the learner L is one of the two nations
//! (drawn); every founder unit of L is removed at round 0, its other start units stay, and L is a human of the
//! scripted prefix (its seat is ended without a move: the units are inert) while the other nation plays the
//! scripted expert and the city-states play themselves, to a drawn round of 20-40 (L4: 60-80, the nations improve land later). At the scene start L's
//! leftover units go and the scene's units come (spawned first: the last unit of a player that never had a city
//! is its life). L has met the owner of the target, is at peace with everybody, has the war chest.
//!
//! SCENES (all 30 rounds):
//! - L1: a city-state's city 4-9 tiles from the army (outside every border), the army the rules' own estimate
//!   (`composition_by` of the operations layer, on the real strikes: the combat previews against the real city,
//!   `siege_rounds_within`, `PLAN_SIEGE_ROUNDS`) takes with the fewest units, plus three (the estimate has every unit
//!   strike every round at full health; measured, one extra unit let the reference take 32 %), of the strongest kinds
//!   the other nation could build, at most 8 melee, the city with at least 4 open sides. WIN: L owns the city when
//!   the scene ends.
//! - L2: the same world and city, three tenths of the army (at least two; the estimate takes it in more than 12
//!   rounds). No win; the gate reads `attacked_city` and `eliminated`.
//! - L3a / L3b: L1 / L2 against a fresh city of the other nation (founded by the harness on a free tile: one
//!   citizen, no buildings, its owner's military units within 3 tiles removed: no walls, no garrison).
//! - L4: nothing to take (the estimate says the 2-3 units take no city within 12 rounds), within reach: at least three
//!   improved tiles of the other nation round one of its cities, one of them outside every city's strike (as T5), 1-2 of its workers on its land, one of its settlers. No win. Measures
//!   `harm` = tiles pillaged (ever, any tile) + civilians captured (distinct civilians the learner owns at a round
//!   end), `eliminated`.

use super::*;

use classic_rules::ai::front::{siege_rounds_within, Besieger};
use classic_rules::ai::operations::{army_role, HEAVY_STRIKE_HP, MAX_ARMY};
use classic_rules::world::player::ClassicPlayers;
use kernel::ai::state::ArmyRole;
use classic_rules::combat::city::CITY_HEAL_PER_TURN;
use classic_rules::combat::damage::Roll;
use classic_rules::combat::exchange::{plan_attack_from, AttackKind};
use classic_rules::world::barbarians::BarbarianMode;

/// World draws (script and seed) tried per scene before the instance is unbuilt.
const DRAWS: u64 = 8;
/// Rounds the estimate may take for L1 / L3a (the "enough" army), and L2 / L3b must not take it within.
const ENOUGH_ROUNDS: u32 = classic_rules::ai::operations::PLAN_SIEGE_ROUNDS;
/// Melee units that can strike one city together: the tiles round it (the rules' estimate counts none of this).
const MAX_MELEE: usize = 8;
/// Units added to the estimate's smallest army: the model has every unit strike every round at full health, the field
/// has them arrive in turn, wounded, and be shot at (measured: a margin of one unit let the reference take 32 %).
const MARGIN: usize = 3;
/// Land tiles round the city that a melee unit can stand on, at least (the model has every unit strike at once).
const MIN_SIDES: usize = 4;
const NOT_ENOUGH_ROUNDS: u32 = 12;
/// The farthest the army may be from striking distance (rounds of walking at peace-free paths).
const MAX_ARRIVE: u32 = 8;
/// Improved tiles out of the cities' strike L4 needs around one city.
const MIN_IMPROVED: usize = 3;
/// ... of which out of every city's strike.
const MIN_SAFE: usize = 1;
/// `ref-takecity` gathers its units this far from the city (outside its strike) and declares when all are there.
const GATHER: i32 = 3;

/// The state of a landless arena.
#[derive(Clone, Default)]
pub(super) struct Ls {
    pub reference: Option<String>,
    /// The target city's damage at the start.
    damage0: u16,
    declared: bool,
    struck: bool,
    taken: Option<u32>,
    /// The round (relative) the scene's own target city was taken (the first version's win).
    taken_target: Option<u32>,
    pill0: BTreeSet<TileId>,
    pillaged: BTreeSet<TileId>,
    captured: BTreeSet<EntityId>,
    /// The learner's moves left in a round are not tracked: the rounds in which it had a unit alive.
    eliminated: Option<u32>,
}

/// The learner's state before one step of the driver (`Run::watch_before`).
pub(super) struct Watch {
    learner_turn: bool,
    war: bool,
}

/// The world of a landless instance.
pub struct LWorld {
    pub game: GameState,
    pub learner: PlayerId,
    pub other: PlayerId,
    /// The generic land military kinds the other nation could build at the scene start.
    pub kinds: Vec<UnitKindId>,
    pub vary: u64,
    pub start: u32,
    pub draw: u64,
    pub label: String,
}

fn city_states_of(game: &GameState) -> Vec<PlayerId> {
    let players = game.view().auth.players();
    (0..players.count() as u8).map(PlayerId).filter(|p| !players.is_major(*p) && !players.is_barbarian(*p) && players.is_alive(*p)).collect()
}

/// The world of `job` at draw `draw` (script and seed as `fit_world` draws them).
pub fn landless_world(rules: &CompiledRuleset, job: &crate::selfplay::Job, draw: u64, late: bool) -> Result<LWorld, String> {
    let Some(GroundJob::Battle { seed, .. }) = job.ground else { return Err("not a battle job".to_owned()) };
    let scripts: Vec<&str> = kernel::scenario::map_script::MapScript::ALL.iter().map(|s| s.key()).collect();
    let at = scripts.iter().position(|s| *s == job.spec.script).unwrap_or(0);
    let mut tried = job.clone();
    tried.spec.seed = job.spec.seed + draw * crate::bench::suite::SEED_STRIDE;
    tried.spec.script = scripts[(at + draw as usize) % scripts.len()].to_owned();
    let mut spec = tried.spec.game()?;
    spec.city_states = crate::spec::city_states(spec.size);
    spec.barbarians = BarbarianMode::None;
    let mut game = crate::export::start(rules, &spec);
    let vary = mix(seed ^ 0x1a4d_1e55 ^ draw.wrapping_mul(0x9e37_79b9_7f4a_7c15));
    // L4 needs improved land, which the nations have only later: its worlds run to round 60-80.
    let start = if late { 60 } else { 20 } + (mix(vary ^ 0x57a7) % 21) as u32;
    let ms = majors(&game);
    if ms.len() != 2 {
        return Err(format!("{} nations, the arena needs 2", ms.len()));
    }
    let l = ms[(mix(vary ^ 0x1) & 1) as usize];
    let o = if ms[0] == l { ms[1] } else { ms[0] };
    // Round 0: the founders of L go, its other units stay (L is alive while it has a unit).
    let founders: Vec<UnitHandle> = {
        let auth = game.view().auth;
        auth.indexes().units_of(l).iter().copied().filter(|h| auth.units().get(*h).is_some_and(|u| rules.units[u.core.kind.0 as usize].founder)).collect()
    };
    for h in founders {
        game.debug_remove_unit(rules, h);
    }
    if game.view().auth.indexes().units_of(l).is_empty() {
        return Err("the learner has no start unit besides its founders".to_owned());
    }
    // The prefix: L is a human whose turns end at once; the rest as the exam's world.
    let mut driver = AiDriver::with_mind(ClassicBench::settings(rules), vec![l], ClassicMind::new(crate::bench::modules::SCRIPTED_EXPERT_VARIANT));
    driver.set_trace_capacity(0);
    let mut kinds: Vec<UnitKindId> = Vec::new();
    let mut recorded = false;
    let mut steps = 0u64;
    while game.view().auth.turn().round < start {
        if game.is_over() {
            return Err(format!("the game ended at round {}", game.view().auth.turn().round));
        }
        steps += 1;
        if steps > 5_000_000 {
            return Err("the prefix does not end".to_owned());
        }
        let last = game.view().auth.turn().round + 1 >= start;
        if last && !recorded && game.view().auth.active_player() == o {
            kinds = battle::buildable(rules, &game);
            recorded = true;
        }
        match driver.step(&mut game, rules) {
            kernel::ai::StepOutcome::Worked | kernel::ai::StepOutcome::PlayerDone(_) | kernel::ai::StepOutcome::RoundDone => {}
            kernel::ai::StepOutcome::Waiting if game.is_over() => break,
            kernel::ai::StepOutcome::Waiting => {
                if game.view().auth.active_player() != l {
                    return Err(format!("round {} waits for a human", game.view().auth.turn().round));
                }
                game.execute(rules, Command::EndTurn { player: l }).map_err(|e| format!("the learner's turn does not end: {e:?}"))?;
            }
        }
    }
    {
        let auth = game.view().auth;
        if !auth.players().is_alive(l) {
            return Err("the learner died in the prefix".to_owned());
        }
        if !auth.players().is_alive(o) || cities_of(&game, o).is_empty() {
            return Err("the other nation has no city".to_owned());
        }
        if !cities_of(&game, l).is_empty() || auth.cities().iter().any(|(_, c)| c.original_owner == l) {
            return Err("the learner has a city".to_owned());
        }
    }
    // Generic kinds only (no nation's own unit), as the exam's kinds.
    kinds.retain(|k| rules.units[k.0 as usize].nation.is_none());
    if kinds.is_empty() {
        return Err("the other nation can build no generic military unit".to_owned());
    }
    Ok(LWorld { game, learner: l, other: o, kinds, vary, start, draw, label: format!("{}-{}-d{}-s{}", tried.spec.script, tried.spec.seed, draw, start) })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Family {
    CityState,
    Nation,
    Raid,
}

fn family_of(sc: Sc) -> Family {
    match sc {
        Sc::L1 | Sc::L2 => Family::CityState,
        Sc::L3a | Sc::L3b => Family::Nation,
        _ => Family::Raid,
    }
}

/// The landless scenes among `wanted`, built: one entry per scene with its game and run, or why it is unbuilt.
/// The two halves of a pair are built from one world and one target.
pub fn landless_scenes(rules: &CompiledRuleset, job: &crate::selfplay::Job, wanted: &[Sc]) -> Vec<(Sc, Result<(GameState, Run), String>)> {
    let mut out: Vec<(Sc, Result<(GameState, Run), String>)> = Vec::new();
    let mut worlds: [Vec<Option<Result<LWorld, String>>>; 2] = [Vec::new(), Vec::new()];
    for fam in [Family::CityState, Family::Nation, Family::Raid] {
        let scs: Vec<Sc> = Sc::LANDLESS.into_iter().filter(|s| family_of(*s) == fam && wanted.contains(s)).collect();
        if scs.is_empty() {
            continue;
        }
        let mut errors: Vec<String> = Vec::new();
        let mut done = None;
        for draw in 0..DRAWS {
            let late = fam == Family::Raid;
            let cache = &mut worlds[usize::from(late)];
            while cache.len() <= draw as usize {
                cache.push(None);
            }
            let world = cache[draw as usize].get_or_insert_with(|| landless_world(rules, job, draw, late));
            let world = match world {
                Ok(w) => w,
                Err(e) => {
                    errors.push(format!("d{draw} world: {e}"));
                    continue;
                }
            };
            let built = match fam {
                Family::Raid => build_raid(rules, world).map(|x| vec![x]),
                f => build_take(rules, world, f == Family::Nation).map(|(a, b)| vec![a, b]),
            };
            match built {
                Ok(v) => {
                    done = Some(v);
                    break;
                }
                Err(e) => errors.push(format!("d{draw} {e}")),
            }
        }
        match done {
            Some(v) => {
                for (sc, g, r) in v {
                    if scs.contains(&sc) {
                        out.push((sc, Ok((g, r))));
                    }
                }
            }
            None => {
                for sc in scs {
                    out.push((sc, Err(errors.join("; "))));
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// The estimate

/// The expected strike of a unit of `kind` of `learner` on the city at `city`, by the rules' combat preview on a
/// copy of `base` with the war on (the real strength, hit points, walls, the garrison's share of the strength).
fn real_strike(rules: &CompiledRuleset, base: &GameState, learner: PlayerId, kind: UnitKindId, city: TileId) -> Option<Besieger> {
    let range = classic_rules::ai::combat::ranged_range(rules, kind);
    let mut trial = base.clone();
    let owner = owner_of_city(&trial, city)?;
    apply(&mut trial, rules, &Op::Meet { a: learner.0, b: owner.0 });
    apply(&mut trial, rules, &Op::War { a: learner.0, b: owner.0 });
    let want = range.unwrap_or(1);
    let mut tiles: Vec<TileId> = around(&trial, city, want)
        .into_iter()
        .map(TileId)
        .filter(|&t| t != city && dist(&trial, t, city) >= 1 && trial.view().auth.indexes().units_on(t).is_empty() && trial.view().auth.indexes().city_on_tile(t).is_none() && battle::standable(rules, &trial, t))
        .collect();
    tiles.sort_by_key(|&t| (std::cmp::Reverse(dist(&trial, t, city)), t.0));
    let from = *tiles.first()?;
    let h = trial.debug_spawn_unit(rules, learner, from, kind);
    let how = if range.is_some() { AttackKind::Ranged } else { AttackKind::Melee };
    let plan = plan_attack_from(trial.view(), rules, h, from, city, how).ok()?;
    let dealt = plan.damage_to_defender(Roll::Expected).min(u16::MAX as _) as u16;
    let taken = if range.is_some() { 0 } else { plan.damage_to_attacker(Roll::Expected).min(u16::MAX as _) as u16 };
    Some(Besieger { ranged: range.is_some(), dealt, taken, hp: kernel::world::unit::UNIT_MAX_HP })
}

fn owner_of_city(game: &GameState, city: TileId) -> Option<PlayerId> {
    let auth = game.view().auth;
    auth.indexes().city_on_tile(city).and_then(|h| auth.cities().get(h)).map(|c| c.owner)
}

/// What the real city is: strength (hundredths), hit points now, units of its owner standing on or next to it.
fn city_numbers(rules: &CompiledRuleset, game: &GameState, city: TileId) -> Option<(i64, u16, usize)> {
    let auth = game.view().auth;
    let h = auth.indexes().city_on_tile(city)?;
    let c = auth.cities().get(h)?;
    let owner = c.owner;
    let strength = classic_rules::combat::city::city_strength(auth, rules, &c);
    let hp = classic_rules::combat::city::hit_points(auth, rules, &c);
    let garrison = auth.indexes().units_of(owner).iter().filter(|u| auth.units().get(**u).is_some_and(|v| dist(game, v.core.tile, city) <= 1 && !rules.is_civilian(v.core.kind))).count();
    Some((strength, hp, garrison))
}

/// The army the operations layer would raise against the real city (its `composition_by` rule on the real strikes):
/// melee 2 + one distance unit, then a distance unit when the melee strike is heavy or distance units are fewer than
/// melee ones, else a melee one, until the group takes the city within `limit` rounds. Returns the counts (melee,
/// distance) and the strikes.
struct Estimate {
    melee_kind: UnitKindId,
    dist_kind: Option<UnitKindId>,
    melee: Besieger,
    distance: Option<Besieger>,
    hp: u16,
}

impl Estimate {
    fn of(rules: &CompiledRuleset, base: &GameState, learner: PlayerId, city: TileId, mine_k: &BTreeMap<Arm, UnitKindId>) -> Result<Estimate, String> {
        let melee_kind = *mine_k.get(&Arm::Melee).ok_or("no melee kind")?;
        let dist_kind = mine_k.get(&Arm::Siege).or_else(|| mine_k.get(&Arm::Ranged)).copied().filter(|k| army_role(rules, *k).is_some_and(|r| r != ArmyRole::Melee));
        let melee = real_strike(rules, base, learner, melee_kind, city).ok_or("no preview of a melee strike on the city")?;
        let distance = match dist_kind {
            Some(k) => Some(real_strike(rules, base, learner, k, city).ok_or("no preview of a distance strike on the city")?),
            None => None,
        };
        let (_, hp, _) = city_numbers(rules, base, city).ok_or("no city")?;
        Ok(Estimate { melee_kind, dist_kind, melee, distance, hp })
    }

    fn group(&self, m: usize, d: usize) -> Vec<Besieger> {
        let mut g = vec![self.melee; m];
        if let Some(x) = self.distance {
            g.extend(std::iter::repeat_n(x, d));
        }
        g
    }

    fn rounds(&self, m: usize, d: usize, limit: u32) -> Option<u32> {
        siege_rounds_within(&mut self.group(m, d), self.hp, CITY_HEAL_PER_TURN as u16, limit)
    }

    /// The smallest army by the rule that takes the city within `limit` rounds.
    fn smallest(&self, limit: u32) -> Result<(usize, usize), String> {
        let (mut m, mut d) = (2usize, usize::from(self.distance.is_some()));
        loop {
            if self.rounds(m, d, limit).is_some() {
                return Ok((m, d));
            }
            if (m + d) as u32 >= MAX_ARMY {
                return Err(format!("more than {MAX_ARMY} units to take the city in {limit} rounds"));
            }
            let heavy = self.melee.taken >= HEAVY_STRIKE_HP;
            if self.distance.is_some() && (heavy || d < m) {
                d += 1;
            } else {
                m += 1;
            }
        }
    }

    fn units(&self, m: usize, d: usize) -> Vec<UnitKindId> {
        let mut v = vec![self.melee_kind; m];
        if let Some(k) = self.dist_kind {
            v.extend(std::iter::repeat_n(k, d));
        }
        v
    }
}

// ---------------------------------------------------------------------------
// Placing the scene

/// The tiles that army of `n` units may stand on: free land of the targets' landmass 6-9 tiles from the nearest
/// target, in nobody's borders, 3 tiles from every city, from which the slowest unit (`move_pts`) reaches one of
/// `goals` (on a copy where the war is on) within MAX_ARRIVE rounds. Returns the tiles, the rounds, the distance.
#[allow(clippy::too_many_arguments)]
fn muster(rules: &CompiledRuleset, base: &GameState, learner: PlayerId, owner: PlayerId, targets: &[TileId], goals: &[TileId], min_goals: usize, n: usize, move_pts: i32, v: u64) -> Result<(Vec<TileId>, u32, i32), String> {
    let area = base.view().auth.map().land_area(targets[0]);
    let mut war = base.clone();
    apply(&mut war, rules, &Op::Meet { a: learner.0, b: owner.0 });
    apply(&mut war, rules, &Op::War { a: learner.0, b: owner.0 });
    let all_cities: Vec<TileId> = (0..base.view().auth.players().count() as u8).flat_map(|p| cities_of(base, PlayerId(p))).collect();
    let free = |t: TileId| {
        let auth = base.view().auth;
        auth.map().land_area(t) == area && owner_of(base, t).is_none() && auth.indexes().units_on(t).is_empty() && auth.indexes().city_on_tile(t).is_none() && battle::standable(rules, base, t) && all_cities.iter().all(|c| dist(base, *c, t) >= 3)
    };
    let near = |t: TileId| targets.iter().map(|g| dist(base, t, *g)).min().unwrap_or(i32::MAX);
    let mut cands: Vec<TileId> = (0..base.view().auth.map().tile_count() as u32).map(TileId).filter(|&t| (6..=9).contains(&near(t)) && free(t)).collect();
    cands.sort_by_key(|t| t.0);
    shuffle(&mut cands, v ^ 0x3a5);
    for &s0 in cands.iter().take(14) {
        let mut rest: Vec<TileId> = (0..base.view().auth.map().tile_count() as u32).map(TileId).filter(|&t| t != s0 && (1..=2).contains(&dist(base, s0, t)) && free(t)).collect();
        rest.sort_by_key(|&t| (dist(base, s0, t), t.0));
        if rest.len() + 1 < n {
            continue;
        }
        let reach = reach_rounds(&war, rules, learner, s0, move_pts, MAX_ARRIVE + 2);
        // Audit fix (landless2): the sides must be reachable and free (a ring tile behind the garrison, or across water,
        // is a side only on paper: l1 #10 had four land sides and the army could stand on two).
        let open = goals.iter().filter(|g| base.view().auth.indexes().units_on(**g).is_empty() && reach.get(*g).is_some_and(|r| *r <= MAX_ARRIVE)).count();
        if open < min_goals {
            continue;
        }
        let Some(arrive) = goals.iter().filter_map(|g| reach.get(g)).min().copied().filter(|r| *r <= MAX_ARRIVE) else { continue };
        let mut tiles = vec![s0];
        tiles.extend(rest.into_iter().take(n - 1));
        return Ok((tiles, arrive, near(s0)));
    }
    Err("no tile to muster the army on".to_owned())
}

fn strikes_from(base: &GameState, city: TileId) -> Vec<TileId> {
    base.view().auth.map().dimensions.neighbors(city).as_slice().to_vec()
}

fn move_of(rules: &CompiledRuleset, k: UnitKindId) -> i32 {
    i32::from(rules.units[k.0 as usize].movement.max(1))
}

/// The learner's side of every landless scene: the army spawned, then the leftover units removed, the chest, the
/// meeting, the sight. Returns the run (blank of the scene's units).
#[allow(clippy::too_many_arguments)]
fn stage(rules: &CompiledRuleset, world: &LWorld, game: &mut GameState, sc: Sc, owner: PlayerId, target: TileId, units: &[UnitKindId], tiles: &[TileId], sight: i32) -> Result<Run, String> {
    let l = world.learner;
    let old: Vec<UnitHandle> = game.view().auth.indexes().units_of(l).to_vec();
    let mut run = Run::blank(sc, l, owner, game.view().auth.turn().round, 0, false);
    for (k, t) in units.iter().zip(tiles) {
        run.mine.push(spawn(game, rules, l, *t, *k, "army"));
    }
    for h in old {
        game.debug_remove_unit(rules, h);
    }
    apply(game, rules, &Op::Earn { player: l.0, gold: battle::WAR_CHEST });
    apply(game, rules, &Op::Meet { a: l.0, b: owner.0 });
    let auth = game.view().auth;
    if !auth.players().is_alive(l) {
        return Err("the learner is not alive at the scene start".to_owned());
    }
    if !cities_of(game, l).is_empty() {
        return Err("the learner has a city at the scene start".to_owned());
    }
    let players = auth.players();
    if (0..players.count() as u8).map(PlayerId).any(|p| p != l && auth.diplomacy().at_war(l, p)) {
        return Err("the learner is at war at the scene start".to_owned());
    }
    reveal(game, rules, l, target, sight);
    run.target = Some(target);
    run.n0 = run.mine.len();
    Ok(run)
}

fn describe(rules: &CompiledRuleset, world: &LWorld, game: &GameState, run: &mut Run, owner: PlayerId, fam: &str) {
    let auth = game.view().auth;
    let owner_kind = if auth.players().is_major(owner) { "nation" } else { "city-state" };
    run.info.insert("world".into(), world.label.clone());
    run.info.insert("draw".into(), world.draw.to_string());
    run.info.insert("learner".into(), format!("p{}", world.learner.0));
    run.info.insert("owner".into(), format!("p{} {owner_kind}", owner.0));
    run.info.insert("era".into(), auth.players().nation_era_of(world.learner, rules).to_string());
    run.info.insert("family".into(), fam.to_owned());
    run.info.insert("learner_alive".into(), auth.players().is_alive(world.learner).to_string());
    run.info.insert("learner_cities".into(), cities_of(game, world.learner).len().to_string());
    run.info.insert("ever_had_city".into(), auth.cities().iter().any(|(_, c)| c.original_owner == world.learner).to_string());
    run.info.insert("learner_units".into(), auth.indexes().units_of(world.learner).len().to_string());
    if let Some(t) = run.target {
        if let Some((s, hp, g)) = city_numbers(rules, game, t) {
            run.info.insert("city_strength".into(), format!("{:.2}", s as f64 / 100.0));
            run.info.insert("city_hp".into(), hp.to_string());
            run.info.insert("garrison".into(), g.to_string());
        }
        let d = run.mine.iter().filter_map(|p| game.view().auth.units().get(p.h).map(|u| dist(game, u.core.tile, t))).min();
        run.info.insert("target_dist".into(), d.map_or(String::new(), |d| d.to_string()));
    }
}

fn counts(rules: &CompiledRuleset, units: &[UnitKindId]) -> String {
    let mut by: BTreeMap<String, usize> = BTreeMap::new();
    for k in units {
        *by.entry(name(rules, *k)).or_default() += 1;
    }
    by.into_iter().map(|(n, c)| format!("{n} x{c}")).collect::<Vec<_>>().join(", ")
}

/// L1 + L2 (or L3a + L3b): one city, two armies.
fn build_take(rules: &CompiledRuleset, world: &LWorld, nation: bool) -> Result<((Sc, GameState, Run), (Sc, GameState, Run)), String> {
    let (sc_full, sc_half) = if nation { (Sc::L3a, Sc::L3b) } else { (Sc::L1, Sc::L2) };
    let v = world.vary ^ sc_full.seed_id().wrapping_mul(0x9e37_79b9);
    let mine_k = battle::best_kinds(rules, &world.kinds);
    let l = world.learner;
    // The candidate targets.
    let mut targets: Vec<(TileId, PlayerId, GameState)> = Vec::new();
    if nation {
        let o = world.other;
        let base = &world.game;
        let all_cities: Vec<TileId> = (0..base.view().auth.players().count() as u8).flat_map(|p| cities_of(base, PlayerId(p))).collect();
        let homes = cities_of(base, o);
        let settler = civil_kind(rules, true).ok_or("no settler kind")?;
        let mut areas: Vec<Option<u32>> = homes.iter().map(|h| base.view().auth.map().land_area(*h)).collect();
        areas.sort();
        areas.dedup();
        let mut sites: Vec<TileId> = areas
            .into_iter()
            .flat_map(|a| free_land(rules, base, a))
            .filter(|&t| {
                let area = base.view().auth.map().land_area(t);
                homes.iter().any(|h| base.view().auth.map().land_area(*h) == area && dist(base, *h, t) <= 8) && all_cities.iter().all(|c| dist(base, *c, t) >= 4) && owner_of(base, t).is_none_or(|p| p == o)
            })
            .collect();
        sites.sort_by_key(|t| t.0);
        shuffle(&mut sites, v ^ 0x52);
        for &t in sites.iter().take(10) {
            let mut trial = base.clone();
            let u = trial.debug_spawn_unit(rules, o, t, settler);
            if !trial.accepts_order(rules, o, &Command::FoundCity { settler: u }) {
                continue;
            }
            let mut g = base.clone();
            g.debug_found_city(rules, o, t);
            clear_enemy_near(&mut g, rules, o, t, 3, &[]);
            targets.push((t, o, g));
            if targets.len() >= 4 {
                break;
            }
        }
    } else {
        let mut cs: Vec<(TileId, PlayerId)> = city_states_of(&world.game).into_iter().flat_map(|p| cities_of(&world.game, p).into_iter().map(move |t| (t, p))).collect();
        cs.sort_by_key(|(t, _)| t.0);
        shuffle(&mut cs, v ^ 0x51);
        targets = cs.into_iter().take(6).map(|(t, p)| (t, p, world.game.clone())).collect();
    }
    if targets.is_empty() {
        return Err("no city to take".to_owned());
    }
    let mut last = String::from("no target");
    for (city, owner, base) in targets {
        let tried = (|| -> Result<_, String> {
            let open = strikes_from(&base, city).into_iter().filter(|t| base.view().auth.map().land_area(*t) == base.view().auth.map().land_area(city) && battle::standable(rules, &base, *t)).count();
            if open < MIN_SIDES {
                return Err(format!("the city has {open} open sides, {MIN_SIDES} needed"));
            }
            let est = Estimate::of(rules, &base, l, city, &mine_k)?;
            let (m, d) = est.smallest(ENOUGH_ROUNDS)?;
            let r_min = est.rounds(m, d, ENOUGH_ROUNDS);
            // One unit of margin, as the operations layer grows an army.
            let (mut mf, mut df) = (m, d);
            for _ in 0..MARGIN {
                if est.distance.is_some() && df < mf { df += 1 } else { mf += 1 }
            }
            if mf > MAX_MELEE {
                return Err(format!("{mf} melee units to take the city in {ENOUGH_ROUNDS} rounds, only {MAX_MELEE} fit round it"));
            }
            let full = est.units(mf, df);
            let r_full = est.rounds(mf, df, NOT_ENOUGH_ROUNDS);
            // Short: three tenths of the units, at least two (a field player took a fresh city with half the
            // army four times in ten, with three tenths not at all), that the estimate does not
            // take within 12 rounds.
            let total = mf + df;
            let n_short = (total * 3 / 10).max(2);
            let mut dh = if est.distance.is_some() { ((df * n_short) as f64 / total as f64).round() as usize } else { 0 };
            let mut mh = n_short.saturating_sub(dh).max(1);
            while est.rounds(mh, dh, NOT_ENOUGH_ROUNDS).is_some() {
                if mh + dh <= 2 {
                    return Err("two units still take the city in 12 rounds".to_owned());
                }
                if dh > 0 && dh >= mh {
                    dh -= 1;
                } else if mh > 1 {
                    mh -= 1;
                } else {
                    dh -= 1;
                }
            }
            let half = est.units(mh, dh);
            let slowest = full.iter().map(|k| move_of(rules, *k)).min().unwrap_or(1);
            let goals = strikes_from(&base, city);
            let (tiles, arrive, d0) = muster(rules, &base, l, owner, &[city], &goals, MIN_SIDES, full.len(), slowest, v)?;
            Ok((est, full, half, tiles, arrive, d0, r_min, r_full))
        })();
        let (est, full, half, tiles, arrive, d0, r_min, r_full) = match tried {
            Ok(x) => x,
            Err(e) => {
                last = e;
                continue;
            }
        };
        let mut pair = Vec::new();
        for (sc, units) in [(sc_full, &full), (sc_half, &half)] {
            let mut game = base.clone();
            let mut run = stage(rules, world, &mut game, sc, owner, city, units, &tiles[..units.len()], d0 + 3)?;
            run.ls = Some(Ls { damage0: game.view().auth.indexes().city_on_tile(city).and_then(|h| game.view().auth.cities().get(h)).map_or(0, |c| c.damage), pill0: pillaged_now(&game), ..Ls::default() });
            describe(rules, world, &game, &mut run, owner, if nation { "nation" } else { "city-state" });
            run.info.insert("kinds".into(), counts(rules, units));
            run.info.insert("army".into(), units.len().to_string());
            run.info.insert("army_full".into(), full.len().to_string());
            run.info.insert("est_rounds_min".into(), r_min.map_or("none".into(), |r| r.to_string()));
            run.info.insert("est_rounds_full".into(), r_full.map_or("none".into(), |r| r.to_string()));
            run.info.insert("est_rounds_this".into(), {
                let (mm, dd) = (units.iter().filter(|k| **k == est.melee_kind).count(), units.iter().filter(|k| Some(**k) == est.dist_kind && **k != est.melee_kind).count());
                est.rounds(mm, dd, 12).map_or("none".into(), |r| r.to_string())
            });
            run.info.insert("melee_strike".into(), format!("{}/{}", est.melee.dealt, est.melee.taken));
            run.info.insert("distance_strike".into(), est.distance.map_or(String::new(), |b| b.dealt.to_string()));
            run.info.insert("arrive_rounds".into(), arrive.to_string());
            run.d0 = run.mean_dist(&game);
            pair.push((sc, game, run));
        }
        let b = pair.pop().expect("two scenes");
        let a = pair.pop().expect("two scenes");
        return Ok((a, b));
    }
    Err(last)
}

/// The walking cost (move points by the rules' step costs, a step that takes all moves counted as 12 moves) from each
/// tile to `goal` over the tiles `player` can enter; the goal itself 0.
fn cost_field(game: &GameState, rules: &CompiledRuleset, player: PlayerId, goal: TileId) -> BTreeMap<TileId, u32> {
    use kernel::navigation::cost::{step_cost, StepCost};
    let auth = game.view().auth;
    let all = u32::from(kernel::world::unit::MOVE_DENOMINATOR) * 12;
    let mut best: BTreeMap<TileId, u32> = BTreeMap::new();
    let mut heap = std::collections::BinaryHeap::new();
    best.insert(goal, 0);
    heap.push(std::cmp::Reverse((0u32, goal.0)));
    while let Some(std::cmp::Reverse((cost, t))) = heap.pop() {
        let t = TileId(t);
        if best.get(&t).is_some_and(|&b| b < cost) {
            continue;
        }
        for &n in auth.map().dimensions.neighbors(t).as_slice() {
            // The step from `n` to `t` is the one the unit makes.
            let Some(step) = step_cost(auth, rules, player, n, t) else { continue };
            let add = match step {
                StepCost::Points(p) => p,
                StepCost::AllMoves => all,
            };
            let next = cost + add.max(1);
            if best.get(&n).is_none_or(|&b| next < b) {
                best.insert(n, next);
                heap.push(std::cmp::Reverse((next, n.0)));
            }
        }
    }
    best
}

fn pillaged_now(game: &GameState) -> BTreeSet<TileId> {
    let map = game.view().auth.map();
    (0..map.tile_count() as u32).map(TileId).filter(|t| map.pillaged(*t)).collect()
}

/// L4: improved land of the other nation, its workers and a settler; a small fast force; nothing to take.
fn build_raid(rules: &CompiledRuleset, world: &LWorld) -> Result<(Sc, GameState, Run), String> {
    let sc = Sc::L4;
    let v = world.vary ^ sc.seed_id().wrapping_mul(0x9e37_79b9);
    let l = world.learner;
    let o = world.other;
    let base = &world.game;
    let bit = |n: u64| (mix(v ^ n) & 1) == 1;
    let fire = classic_rules::combat::city::CITY_ATTACK_RANGE;
    let o_cities = cities_of(base, o);
    let auth = base.view().auth;
    let map = auth.map();
    let improved: Vec<TileId> = (0..map.tile_count() as u32)
        .map(TileId)
        .filter(|&t| map.land_area(t).is_some() && map.owner(t).flatten() == Some(o) && matches!(map.improvement(t), Some(Some(_))) && !map.pillaged(t) && auth.indexes().units_on(t).is_empty())
        .collect();
    let near_fire = |t: TileId| o_cities.iter().map(|c| dist(base, t, *c)).min().unwrap_or(i32::MAX);
    // The city with the most improved tiles round it (within 4, on its land), at least MIN_IMPROVED of them and
    // MIN_SAFE out of every city's strike (improvements lie round the cities: the strike covers most of them).
    let round = |c: TileId| -> Vec<TileId> { improved.iter().copied().filter(|&t| dist(base, t, c) <= 4 && map.land_area(t) == map.land_area(c)).collect() };
    let mut best: Option<(usize, usize, TileId)> = None;
    for &c in &o_cities {
        let all = round(c);
        let safe = all.iter().filter(|&&t| near_fire(t) > fire).count();
        if all.len() >= MIN_IMPROVED && safe >= MIN_SAFE && best.is_none_or(|b| (safe, all.len()) > (b.0, b.1)) {
            best = Some((safe, all.len(), c));
        }
    }
    let (_, _, city) = best.ok_or_else(|| format!("no city of the other nation has {MIN_IMPROVED} improved tiles, {MIN_SAFE} out of its strike ({} improved, {} cities)", improved.len(), o_cities.len()))?;
    let tiles: Vec<TileId> = round(city);
    let mine_k = field_kinds(rules, &world.kinds, v ^ 0x1ea);
    let pill = guard_kind_fast(&mine_k).ok_or("no fast or melee kind")?;
    let n = 2 + usize::from(bit(4));
    let units = vec![pill; n];
    let (muster_tiles, arrive, d0) = muster(rules, base, l, o, &tiles, &tiles, 1, n, move_of(rules, pill), v)?;
    // Nothing to take: the estimate says this force takes no city within 12 rounds (and none is next to the muster).
    let mut taking = None;
    for c in (0..auth.players().count() as u8).flat_map(|p| cities_of(base, PlayerId(p))) {
        if dist(base, muster_tiles[0], c) > 14 {
            continue;
        }
        let melee_only = Estimate::of(rules, base, l, c, &BTreeMap::from([(Arm::Melee, pill)]));
        if let Ok(est) = melee_only {
            if est.rounds(n, 0, NOT_ENOUGH_ROUNDS).is_some() {
                taking = Some(c);
            }
        }
    }
    if taking.is_some() {
        return Err("the force could take a city in reach".to_owned());
    }
    let mut game = base.clone();
    let mut run = stage(rules, world, &mut game, sc, o, city, &units, &muster_tiles, d0 + 3)?;
    // Prey: the other nation's workers on its land near the improved tiles, one of its settlers on free land near the army.
    let worker = civil_kind(rules, false).ok_or("no worker kind")?;
    let settler = civil_kind(rules, true).ok_or("no settler kind")?;
    let area = game.view().auth.map().land_area(city);
    let free = free_land(rules, &game, area);
    let n_workers = 1 + usize::from(bit(5));
    for w in 0..n_workers {
        let spots: Vec<TileId> = free
            .iter()
            .copied()
            .filter(|&t| owner_of(&game, t) == Some(o) && tiles.iter().any(|i| dist(&game, t, *i) <= 2) && o_cities.iter().all(|c| dist(&game, *c, t) > fire) && game.view().auth.indexes().units_on(t).is_empty())
            .collect();
        let t = pick_free(spots, v ^ 0x61 ^ w as u64).ok_or("no tile for a worker")?;
        run.theirs.push(spawn(&mut game, rules, o, t, worker, "worker"));
    }
    let all_cities: Vec<TileId> = (0..game.view().auth.players().count() as u8).flat_map(|p| cities_of(&game, PlayerId(p))).collect();
    let spots: Vec<TileId> = free.iter().copied().filter(|&t| (4..=8).contains(&dist(&game, t, muster_tiles[0])) && owner_of(&game, t).is_none() && all_cities.iter().all(|c| dist(&game, *c, t) >= 4) && game.view().auth.indexes().units_on(t).is_empty()).collect();
    let t = pick_free(spots, v ^ 0x62).ok_or("no tile for a settler")?;
    run.theirs.push(spawn(&mut game, rules, o, t, settler, "settler"));
    run.imp = improved.iter().copied().filter(|&t| dist(&game, t, city) <= 5).collect();
    run.ls = Some(Ls { pill0: pillaged_now(&game), ..Ls::default() });
    describe(rules, world, &game, &mut run, o, "raid");
    run.info.insert("kinds".into(), counts(rules, &units));
    run.info.insert("improved".into(), tiles.len().to_string());
    run.info.insert("prey".into(), run.theirs.iter().map(|p| p.role).collect::<Vec<_>>().join(","));
    run.info.insert("arrive_rounds".into(), arrive.to_string());
    run.info.insert("nearest_city".into(), all_cities.iter().map(|c| dist(&game, *c, muster_tiles[0])).min().map_or(String::new(), |d| d.to_string()));
    run.d0 = run.mean_dist(&game);
    Ok((sc, game, run))
}

// ---------------------------------------------------------------------------
// Playing

impl Run {
    fn city_tile(&self) -> Option<TileId> {
        self.target
    }

    fn at_war_with_foe(&self, game: &GameState) -> bool {
        game.view().auth.diplomacy().at_war(self.learner, self.enemy)
    }

    /// Before a driver step: whether it is the learner's turn and the war state.
    pub(super) fn watch_before(&self, game: &GameState) -> Option<Watch> {
        self.ls.as_ref()?;
        Some(Watch { learner_turn: game.view().auth.active_player() == self.learner, war: self.at_war_with_foe(game) })
    }

    /// After it: a war that began in the learner's turn is its declaration; damage to the target city is its strike.
    pub(super) fn watch_after(&mut self, game: &GameState, w: Watch) {
        let war = self.at_war_with_foe(game);
        let target = self.city_tile();
        // Audit fix (landless2): a strike on ANY city of the target's owner, or owning any city, is an attack on a city
        // (the first version watched the target city only: a group that went for the owner's other city, which is
        // what a player does when it is the nearer one, counted as not having attacked).
        let (hit, owns) = {
            let auth = game.view().auth;
            let hit = auth.cities().iter().any(|(_, c)| {
                let is_target = target.is_some_and(|t| auth.indexes().city_on_tile(t).and_then(|h| auth.cities().get(h)).is_some_and(|x| x.tile == c.tile));
                c.owner == self.enemy && c.damage > if is_target { self.ls.as_ref().map_or(0, |l| l.damage0) } else { 0 }
            });
            (hit, !auth.indexes().cities_of(self.learner).is_empty())
        };
        let Some(ls) = self.ls.as_mut() else { return };
        if w.learner_turn && war && !w.war {
            ls.declared = true;
        }
        if self.sc != Sc::L4 && (hit || owns) {
            ls.struck = true;
        }
    }

    /// After a round.
    pub(super) fn after_round_landless(&mut self, rules: &CompiledRuleset, game: &mut GameState) -> bool {
        let round = game.view().auth.turn().round;
        let rel = round.saturating_sub(self.start_round);
        let learner = self.learner;
        let alive = game.view().auth.players().is_alive(learner);
        let (pill_now, civilians, owns, owns_target): (Vec<TileId>, Vec<EntityId>, bool, bool) = {
            let auth = game.view().auth;
            let map = auth.map();
            let pill_now: Vec<TileId> = (0..map.tile_count() as u32).map(TileId).filter(|t| map.pillaged(*t)).collect();
            let civilians: Vec<EntityId> = auth.indexes().units_of(learner).iter().filter_map(|h| auth.units().get(*h)).filter(|u| rules.is_civilian(u.core.kind)).map(|u| u.core.id).collect();
            // Audit fix: any city of the learner is a city taken (not only the scene's target).
            let owns = !auth.indexes().cities_of(learner).is_empty();
            let owns_target = self.target.is_some_and(|t| auth.indexes().city_on_tile(t).and_then(|h| auth.cities().get(h)).is_some_and(|c| c.owner == learner));
            (pill_now, civilians, owns, owns_target)
        };
        let sc = self.sc;
        if let Some(ls) = self.ls.as_mut() {
            for t in pill_now {
                if !ls.pill0.contains(&t) {
                    ls.pillaged.insert(t);
                }
            }
            ls.captured.extend(civilians);
            if owns_target && ls.taken_target.is_none() {
                ls.taken_target = Some(rel);
            }
            if owns && sc != Sc::L4 && ls.taken.is_none() {
                ls.taken = Some(rel);
                ls.struck = true;
            }
            if !alive && ls.eliminated.is_none() {
                ls.eliminated = Some(rel);
            }
        }
        let line = self.line(rules, game, rel);
        self.trace.push(line);
        let taken = self.ls.as_ref().is_some_and(|l| l.taken.is_some());
        let limit = rel >= self.sc.rounds();
        let over = !alive || limit || (taken && self.sc != Sc::L4);
        if over {
            self.win = match self.sc {
                Sc::L1 | Sc::L3a => Some(taken),
                _ => None,
            };
        }
        self.over = over;
        over
    }

    /// The measures of a landless arena.
    pub(super) fn landless_measures(&self, game: &GameState, m: &mut BTreeMap<String, f64>) {
        let Some(ls) = &self.ls else { return };
        let b = |x: bool| f64::from(u8::from(x));
        let alive = game.view().auth.players().is_alive(self.learner);
        m.insert("eliminated".into(), b(!alive));
        m.insert("declared".into(), b(ls.declared));
        m.insert("struck".into(), b(ls.struck));
        // Audit fix: attacking the city is a strike on it; a war declared on its owner for other prey (a worker, a
        // settler) is not (`declared` stays as a measure of its own, the first version OR-ed the two).
        m.insert("attacked_city".into(), b(ls.struck));
        m.insert("attacked_or_declared".into(), b(ls.declared || ls.struck));
        m.insert("taken".into(), ls.taken.map_or(-1.0, f64::from));
        m.insert("taken_target".into(), ls.taken_target.map_or(-1.0, f64::from));
        m.insert("at_war_end".into(), b(self.at_war_with_foe(game)));
        if self.sc == Sc::L4 {
            let (p, c) = (ls.pillaged.len(), ls.captured.len());
            m.insert("pillaged".into(), p as f64);
            m.insert("captured".into(), c as f64);
            m.insert("harm".into(), (p + c) as f64);
        }
    }

    // -----------------------------------------------------------------------
    // The references

    /// The learner's turn of a landless reference: `ref-takecity` or `ref-raid`.
    pub(super) fn landless_reference(&mut self, rules: &CompiledRuleset, game: &mut GameState) -> u64 {
        if game.view().auth.active_player() != self.learner {
            return 0;
        }
        let name = self.ls.as_ref().and_then(|l| l.reference.clone()).unwrap_or_default();
        let made = match name.as_str() {
            "ref-raid" => self.raid_turn(rules, game),
            _ => self.takecity_turn(rules, game),
        };
        for (p, _) in self.mine_alive(game) {
            stand(game, p.h);
        }
        made
    }

    fn declare(&mut self, rules: &CompiledRuleset, game: &mut GameState) -> bool {
        let cmd = Command::DeclareWar { player: self.learner, enemy: self.enemy };
        if game.accepts_order(rules, self.learner, &cmd) && game.execute(rules, cmd).is_ok() {
            if let Some(ls) = self.ls.as_mut() {
                ls.declared = true;
            }
            return true;
        }
        false
    }

    /// One step of the unit toward `goal`: the reachable tile with the least walking cost to it (a field over the
    /// rules' step costs, so a river or a bay does not trap the unit), the straight distance on a tie; nearer than now.
    fn step_toward(&self, rules: &CompiledRuleset, game: &mut GameState, p: &Piece, goal: TileId) -> bool {
        let Some(c) = self.alive_h(game, p, self.learner) else { return false };
        if c.moves_left == 0 {
            return false;
        }
        let field = cost_field(game, rules, self.learner, goal);
        let key = |g: &GameState, t: TileId| (field.get(&t).copied().unwrap_or(u32::MAX), dist(g, t, goal), t.0);
        let here = key(game, c.tile);
        let mut tiles: Vec<TileId> = game
            .player_view(rules, self.learner)
            .own_reachable(p.h)
            .into_iter()
            .map(|(t, _)| t)
            .filter(|t| *t != c.tile && game.view().auth.indexes().units_on(*t).is_empty() && game.view().auth.indexes().city_on_tile(*t).is_none() && key(game, *t) < here)
            .collect();
        tiles.sort_by_key(|&t| key(game, t));
        for t in tiles.into_iter().take(8) {
            let cmd = Command::MoveUnit { unit: p.h, destination: t };
            if game.accepts_order(rules, self.learner, &cmd) && game.execute(rules, cmd).is_ok() {
                return true;
            }
        }
        false
    }

    /// `ref-takecity`: at peace, every unit walks at the city and stops three tiles from it; when all are there, or
    /// none can get nearer (a border), war is declared; at war every unit attacks the city (a shooter shoots it)
    /// or walks at it, every turn.
    fn takecity_turn(&mut self, rules: &CompiledRuleset, game: &mut GameState) -> u64 {
        let Some(target) = self.target else { return 0 };
        let mut made = 0;
        let mut stuck = false;
        for _ in 0..4 {
            let war = self.at_war_with_foe(game);
            let units = self.mine_alive(game);
            if !war {
                let ready = !units.is_empty() && units.iter().all(|(_, c)| dist(game, c.tile, target) <= GATHER);
                if (ready || stuck) && self.declare(rules, game) {
                    made += 1;
                    continue;
                }
            }
            let mut tried = 0;
            let mut moved = 0;
            for (p, c) in units {
                if c.moves_left == 0 || (!war && dist(game, c.tile, target) <= GATHER) {
                    continue;
                }
                tried += 1;
                for round in 0..2 {
                    let city_h = game.view().auth.indexes().city_on_tile(target);
                    let ranged = classic_rules::ai::combat::ranged_range(rules, c.kind).is_some();
                    let cmd = if ranged { Some(Command::RangedAttack { unit: p.h, target }) } else { city_h.map(|city| Command::AttackCity { attacker: p.h, city }) };
                    if war {
                        if let Some(cmd) = cmd {
                            if game.accepts_order(rules, self.learner, &cmd) && game.execute(rules, cmd).is_ok() {
                                made += 1;
                                break;
                            }
                        }
                    }
                    if round == 0 && self.step_toward(rules, game, &p, target) {
                        made += 1;
                        moved += 1;
                        continue;
                    }
                    break;
                }
            }
            if !war && tried > 0 && moved == 0 {
                stuck = true;
            }
        }
        made
    }

    /// `ref-raid`: declares war on the owner of the prey at once; melee units capture the nearest civilian (attack
    /// its tile), else walk to the nearest unpillaged improvement and pillage it; danger is ignored.
    fn raid_turn(&mut self, rules: &CompiledRuleset, game: &mut GameState) -> u64 {
        let mut made = 0;
        if !self.at_war_with_foe(game) && self.declare(rules, game) {
            made += 1;
        }
        for _ in 0..3 {
            for (p, c) in self.mine_alive(game) {
                if c.moves_left == 0 {
                    continue;
                }
                // Audit fix: the reference sees what the learner sees (a tile it knows, a unit in sight), as the AI under
                // test does; the first version read the whole map (a settler beyond every sight, tiles never explored).
                let imp: Vec<TileId> = {
                    let pv = game.player_view(rules, self.learner);
                    self.imp.iter().copied().filter(|&t| pv.tile(t).snapshot().is_some() && !game.view().auth.map().pillaged(t) && matches!(game.view().auth.map().improvement(t), Some(Some(_)))).collect()
                };
                if imp.contains(&c.tile) {
                    let cmd = Command::Pillage { unit: p.h };
                    if game.accepts_order(rules, self.learner, &cmd) && game.execute(rules, cmd).is_ok() {
                        made += 1;
                    }
                    continue;
                }
                // The nearest civilian of the other side.
                let prey: Option<(UnitHandle, TileId)> = {
                    let auth = game.view().auth;
                    let pv = game.player_view(rules, self.learner);
                    auth.indexes()
                        .units_of(self.enemy)
                        .iter()
                        .filter_map(|h| auth.units().get(*h).filter(|u| pv.sees_unit(&u.core)).map(|u| (*h, u.core.tile, u.core.kind)))
                        .filter(|(_, _, k)| rules.is_civilian(*k))
                        .map(|(h, t, _)| (h, t))
                        .min_by_key(|(_, t)| (dist(game, c.tile, *t), t.0))
                };
                if let Some((h, t)) = prey {
                    let cmd = Command::AttackUnit { attacker: p.h, defender: h };
                    if dist(game, c.tile, t) <= 1 && game.accepts_order(rules, self.learner, &cmd) && game.execute(rules, cmd).is_ok() {
                        made += 1;
                        continue;
                    }
                    if self.step_toward(rules, game, &p, t) {
                        made += 1;
                        if let Some(now) = self.alive_h(game, &p, self.learner) {
                            let cmd = Command::AttackUnit { attacker: p.h, defender: h };
                            if dist(game, now.tile, t) <= 1 && game.accepts_order(rules, self.learner, &cmd) && game.execute(rules, cmd).is_ok() {
                                made += 1;
                            }
                        }
                        continue;
                    }
                }
                if let Some(goal) = imp.iter().copied().min_by_key(|&t| (dist(game, c.tile, t), t.0)) {
                    if self.step_toward(rules, game, &p, goal) {
                        made += 1;
                        if let Some(now) = self.alive_h(game, &p, self.learner) {
                            if imp.contains(&now.tile) {
                                let cmd = Command::Pillage { unit: p.h };
                                if game.accepts_order(rules, self.learner, &cmd) && game.execute(rules, cmd).is_ok() {
                                    made += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
        made
    }
}
