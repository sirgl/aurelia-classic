//! The tactics exam (`ai-exam`; /Users/roman.ivanov/civ/work/nets/exam/REPORT.md): scenes of a few
//! units (the tactician: T1-T5) and of an operation (the commander: C1, C3) that a named AI variant plays
//! like a player, with no scripted help (`BattleScene::strike` and `hunt` are NOT used: the unit fights
//! of the training ladders are won with the oracle's strikes under a named learner mind). A measurement:
//! it changes no rule and no AI.
//!
//! Every instance is a function of its index alone (the world, the units, the tiles), so every player
//! plays the same scene; the world is the suite game played by `standard-scripted` to a drawn round
//! (`play_prefix`, `--battle-vary`'s world), the two nations put at war (`go_to_war`), the scene's units
//! spawned by role and era from the rules' data (no unit or nation is named here).
//!
//! WIN RULES, fixed before the first run and not changed after seeing results:
//! - T1a / T1b: every enemy unit dead with at least one of ours alive, within 10 rounds. T1a: one melee
//!   + one ranged against two melee of the learner's melee kind; T1b: two melee + one ranged against three.
//! - T2: the worker is alive and ours when 12 rounds end (or when every raider is dead).
//! - T3: a city of the learner is founded within 14 rounds (the settler walks to a site that the rules
//!   accept, 5-6 tiles from where it starts); measure also `settler_alive`.
//! - T4: both workers alive and ours when 12 rounds end (or every raider dead). `one_saved`: at least one.
//! - T5 / T5r / T5b: raw score = improvements of the enemy pillaged (ever) in 14 rounds and units kept.
//!   WIN (computed by the report against the scripted reference `ref-pillage` on the same instance, that
//!   needs the instance's reference score r >= 1): pillaged >= ceil(r / 2). Instances with r = 0 are
//!   not counted (the scene cannot be done by the simple script). T5: free units (the AI decides whether to
//!   raid), T5r: the units hold the AI's own raid task (`UnitTask::Raid`), the operations layer's choice
//!   is taken out; T5b: free like T5, the improved tiles under the city's strike and one unit wounded to 40 %. (A task given
//!   to units is replaced by `standard`'s planner every turn, so T5r tests the variants that read tasks.)
//! - C1: the enemy city 8-12 tiles from the army (12 units over up to three cities) is taken within 36
//!   rounds. Raw: the round the army set out (the mean distance to the city shrank by 2), units within 3
//!   tiles of the city at the first arrival and two rounds later, units lost.
//! - C3: as C1; when the first army unit comes within 6 tiles of the city, a strong reinforcement (as
//!   many units as the army, the enemy's best of each arm) is put in and around the city. The scene counts
//!   only when that trigger happened; WIN = at least 75 % of the army alive when 36 rounds end (the
//!   army saved: it withdrew or never fought). The reference `ref-press` (a script: every army unit attacks the
//!   city or the foe next to it every turn, else walks at the city) is the "presses on" line, and the
//!   ceiling of C1.

use std::collections::{BTreeMap, BTreeSet};

use classic_rules::Command;
use kernel::ai::state::{ObjectiveKind, SeenCity, UnitTask, WarObjective};
use kernel::ids::EntityId;

use super::*;

/// The scenes of the exam.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Sc {
    T1a,
    T1b,
    T2,
    T3,
    T4,
    T5,
    T5r,
    T5b,
    C1,
    C3,
}

impl Sc {
    pub const ALL: [Sc; 10] = [Sc::T1a, Sc::T1b, Sc::T2, Sc::T3, Sc::T4, Sc::T5, Sc::T5r, Sc::T5b, Sc::C1, Sc::C3];

    pub fn name(self) -> &'static str {
        match self {
            Sc::T1a => "t1a",
            Sc::T1b => "t1b",
            Sc::T2 => "t2",
            Sc::T3 => "t3",
            Sc::T4 => "t4",
            Sc::T5 => "t5",
            Sc::T5r => "t5r",
            Sc::T5b => "t5b",
            Sc::C1 => "c1",
            Sc::C3 => "c3",
        }
    }

    pub fn parse(text: &str) -> Result<Sc, String> {
        Sc::ALL.into_iter().find(|s| s.name() == text).ok_or_else(|| format!("unknown scene {text} ({})", Sc::ALL.iter().map(|s| s.name()).collect::<Vec<_>>().join(", ")))
    }

    /// The rounds a scene lasts.
    pub fn rounds(self) -> u32 {
        match self {
            Sc::T1a | Sc::T1b => 10,
            Sc::T2 | Sc::T4 => 12,
            Sc::T3 | Sc::T5 | Sc::T5r | Sc::T5b => 14,
            Sc::C1 | Sc::C3 => 36,
        }
    }

    /// Scenes with raiders (a control without them: `Flags::control`).
    pub fn has_raiders(self) -> bool {
        matches!(self, Sc::T2 | Sc::T3 | Sc::T4)
    }

    /// The scripted reference players of a scene.
    pub fn references(self) -> &'static [&'static str] {
        match self {
            Sc::T5 | Sc::T5r | Sc::T5b => &["ref-pillage"],
            Sc::C1 | Sc::C3 => &["ref-press"],
            _ => &[],
        }
    }
}

/// A scene unit.
#[derive(Debug, Clone)]
pub struct Piece {
    pub h: UnitHandle,
    pub id: EntityId,
    pub kind: UnitKindId,
    pub role: &'static str,
}

/// The world of an instance: the suite game at the war's round, the nations at war, nothing spawned.
pub struct World {
    pub game: GameState,
    pub learner: PlayerId,
    pub enemy: PlayerId,
    pub kinds: Kinds,
    pub seed: u64,
    pub start: u32,
    pub vary: u64,
    pub label: String,
}

/// The instance jobs: `n` worlds of the standard suite at Quick on the minimal map, vary on.
pub fn jobs(rules: &CompiledRuleset, n: usize, offset: u64) -> Result<Vec<crate::selfplay::Job>, String> {
    let suite = crate::bench::suite::Suite::named("standard").ok_or("no suite standard")?;
    let speed = rules.speed_named("quick").ok_or("no speed quick")?;
    let rounds = speed.values.game_round(suite.rounds);
    let config = crate::selfplay::Config {
        suite,
        sizes: vec!["small".to_owned()],
        rounds,
        net: "mind:exam".to_owned(),
        net_seats: 1,
        opponents: vec!["standard".to_owned()],
        temperature: 0.0,
        group: 1,
        seed_offset: offset,
        sample_seed: 0,
        from: None,
        learner_mind: Some("standard".to_owned()),
        speed: if speed.values.is_standard() { String::new() } else { speed.key.clone() },
        heads: None,
        decision_values: None,
        ladder_solvable: false,
        grounds: Some(Grounds::Battle {
            kind: BattleKind::Skirmish,
            rounds: (20, 40),
            army: (6, 10),
            scripts: BATTLE_SCRIPTS.iter().map(|s| (*s).to_owned()).collect(),
            difficulty: 1.0,
            stage: None,
            rung: None,
            minimal: true,
            vary: true,
        }),
    };
    let out = crate::selfplay::jobs(&config, n);
    if out.len() < n {
        return Err(format!("the suite's maps give {} instances, not {n}", out.len()));
    }
    Ok(out)
}

/// Builds the world of `job`: the prefix, the war, the first mover the learner, the chests.
pub fn build_world(rules: &CompiledRuleset, job: &crate::selfplay::Job) -> Result<World, String> {
    let fitted = fit_world(rules, job)?;
    let mut game = crate::export::start(rules, &fitted.spec.game()?);
    let (_, kinds) = play_prefix(rules, &mut game, &fitted.spec, fitted.ground.as_ref())?.ok_or("no prefix")?;
    let Some(GroundJob::Battle { seed, start, .. }) = fitted.ground else { return Err("not a battle job".to_owned()) };
    let mut rec = Recorder::default();
    let sides = go_to_war(rules, &mut game, &mut rec, 0)?;
    let (learner, enemy) = {
        let auth = game.view().auth;
        let (active, n) = (u32::from(auth.active_player().0), auth.players().count().max(1) as u32);
        let after = |p: PlayerId| (u32::from(p.0) + n - active) % n;
        if after(sides[0]) < after(sides[1]) { (sides[0], sides[1]) } else { (sides[1], sides[0]) }
    };
    for side in [learner, enemy] {
        apply(&mut game, rules, &Op::Earn { player: side.0, gold: battle::WAR_CHEST });
    }
    let vary = mix(seed ^ 0x7a71e7);
    Ok(World { game, learner, enemy, kinds, seed, start, vary, label: format!("{}-{}-s{}", fitted.spec.script, fitted.spec.seed, start) })
}

// ---------------------------------------------------------------------------
// Helpers

fn dist(game: &GameState, a: TileId, b: TileId) -> i32 {
    game.view().auth.map().dimensions.distance(a, b).unwrap_or(i32::MAX)
}

fn cities_of(game: &GameState, p: PlayerId) -> Vec<TileId> {
    let auth = game.view().auth;
    let mut v: Vec<TileId> = auth.indexes().cities_of(p).iter().filter_map(|h| auth.cities().get(*h)).map(|c| c.tile).collect();
    v.sort_by_key(|t| t.0);
    v
}

/// Free land of `area` a unit may stand on (no unit, no city).
fn free_land(rules: &CompiledRuleset, game: &GameState, area: Option<u32>) -> Vec<TileId> {
    let auth = game.view().auth;
    let map = auth.map();
    (0..map.tile_count() as u32)
        .map(TileId)
        .filter(|&t| map.land_area(t) == area && auth.indexes().units_on(t).is_empty() && auth.indexes().city_on_tile(t).is_none() && battle::standable(rules, game, t))
        .collect()
}

fn owner_of(game: &GameState, t: TileId) -> Option<PlayerId> {
    game.view().auth.map().owner(t).flatten()
}

fn spawn(game: &mut GameState, rules: &CompiledRuleset, owner: PlayerId, tile: TileId, kind: UnitKindId, role: &'static str) -> Piece {
    let h = game.debug_spawn_unit(rules, owner, tile, kind);
    let id = game.view().auth.units().get(h).expect("a spawned unit").core.id;
    Piece { h, id, kind, role }
}

/// The cheapest land worker (a builder that does not found) or settler available from the start.
fn civil_kind(rules: &CompiledRuleset, founder: bool) -> Option<UnitKindId> {
    (0..rules.units.len())
        .map(|i| UnitKindId(i as u16))
        .filter(|&k| {
            let d = &rules.units[k.0 as usize];
            d.domain == kernel::rules::content::Domain::Land && !d.unbuildable && d.cost > 0 && d.required_tech.is_none() && d.nation.is_none() && d.replaces.is_none() && if founder { d.founder } else { d.builder && !d.founder }
        })
        .min_by_key(|k| (rules.units[k.0 as usize].cost, k.0))
}

fn name(rules: &CompiledRuleset, k: UnitKindId) -> String {
    rules.units.get(k.0 as usize).map_or_else(|| format!("#{}", k.0), |d| d.name.clone())
}

/// The enemy's military units outside cities within `radius` of `centre` removed: the scene's raiders are the
/// only hostile units about (the world's own are the war of a real game, not the scene).
fn clear_enemy_near(game: &mut GameState, rules: &CompiledRuleset, enemy: PlayerId, centre: TileId, radius: i32, keep: &[UnitHandle]) -> usize {
    let gone: Vec<UnitHandle> = {
        let auth = game.view().auth;
        auth.indexes()
            .units_of(enemy)
            .iter()
            .copied()
            .filter(|h| !keep.contains(h))
            .filter(|h| auth.units().get(*h).is_some_and(|u| kernel::ai::plan::role_of(rules, u.core.kind) == kernel::ai::plan::AiRole::Military && dist(game, u.core.tile, centre) <= radius && auth.indexes().city_on_tile(u.core.tile).is_none()))
            .collect()
    };
    for h in &gone {
        game.debug_remove_unit(rules, *h);
    }
    gone.len()
}

fn reveal(game: &mut GameState, rules: &CompiledRuleset, side: PlayerId, centre: TileId, reach: i32) {
    let tiles = around(game, centre, reach);
    apply(game, rules, &Op::Reveal { player: side.0, tiles });
}

/// The enemy's raider kind: its mounted unit, else its melee one.
fn raider_kind(enemy: &BTreeMap<Arm, UnitKindId>) -> Option<UnitKindId> {
    enemy.get(&Arm::Mounted).or_else(|| enemy.get(&Arm::Melee)).copied()
}

/// One kind per arm drawn as `drawn_kinds`, but of the kinds that fight in the field (a kind with a bonus against
/// cities, a ram, is left out while the arm has another).
fn field_kinds(rules: &CompiledRuleset, kinds: &[UnitKindId], seed: u64) -> BTreeMap<Arm, UnitKindId> {
    let field: Vec<UnitKindId> = kinds.iter().copied().filter(|k| classic_rules::ai::unciv::sight::vs_city_percent(rules, *k) <= 0).collect();
    let mut drawn = drawn_kinds(rules, &field, seed);
    for (arm, k) in drawn_kinds(rules, kinds, seed) {
        drawn.entry(arm).or_insert(k);
    }
    drawn
}

/// A guard: the learner's melee unit or (odd draws) its ranged one.
fn guard_kind(mine: &BTreeMap<Arm, UnitKindId>, bit: bool) -> Option<UnitKindId> {
    if bit { mine.get(&Arm::Ranged).or_else(|| mine.get(&Arm::Melee)) } else { mine.get(&Arm::Melee).or_else(|| mine.get(&Arm::Ranged)) }.copied()
}

fn pick_free(mut tiles: Vec<TileId>, seed: u64) -> Option<TileId> {
    tiles.sort_by_key(|t| t.0);
    shuffle(&mut tiles, seed);
    tiles.first().copied()
}

// ---------------------------------------------------------------------------
// The run

/// An instance's scene, built.
#[derive(Clone)]
pub struct Run {
    pub sc: Sc,
    pub learner: PlayerId,
    pub enemy: PlayerId,
    pub mine: Vec<Piece>,
    pub theirs: Vec<Piece>,
    pub start_round: u32,
    pub target: Option<TileId>,
    pub aim_enemy: Option<TileId>,
    pub aim_learner: Option<TileId>,
    pub raid: Option<TileId>,
    /// Players play without raiders (T2-T4 controls).
    pub control: bool,
    // State and measures.
    over: bool,
    win: Option<bool>,
    cities0: usize,
    n0: usize,
    imp: Vec<TileId>,
    pillaged: BTreeSet<TileId>,
    d0: f64,
    set_out: Option<u32>,
    arrived: Option<u32>,
    near_first: u32,
    near_plus2: u32,
    taken: Option<u32>,
    trigger: Option<u32>,
    reinforce_kinds: Vec<UnitKindId>,
    lost_at_trigger: u32,
    trig_near: Vec<EntityId>,
    min_dist: f64,
    founded: Option<u32>,
    pub info: BTreeMap<String, String>,
    pub trace: Vec<String>,
    wounded_start: u32,
}

/// What a built instance describes.
pub fn build(sc: Sc, rules: &CompiledRuleset, world: &World, control: bool) -> Result<(GameState, Run), String> {
    let mut game = world.game.clone();
    let (learner, enemy) = (world.learner, world.enemy);
    let kinds_of = |p: PlayerId| world.kinds.get(&p).cloned().unwrap_or_default();
    let v = world.vary ^ (sc as u64).wrapping_mul(0x9e37_79b9);
    let (mine_k, theirs_k) = if matches!(sc, Sc::C1 | Sc::C3) {
        (drawn_kinds(rules, &kinds_of(learner), v ^ 0x1ea), drawn_kinds(rules, &kinds_of(enemy), v ^ 0xe1e))
    } else {
        (field_kinds(rules, &kinds_of(learner), v ^ 0x1ea), field_kinds(rules, &kinds_of(enemy), v ^ 0xe1e))
    };
    let own = battle::front_city(&game, enemy, learner).ok_or("the learner has no city on the enemy's land")?;
    let far = battle::front_city(&game, learner, enemy).ok_or("the enemy has no city on the learner's land")?;
    let area = game.view().auth.map().land_area(far);
    let bit = |n: u64| (mix(v ^ n) & 1) == 1;
    let mut run = Run {
        sc,
        learner,
        enemy,
        mine: Vec::new(),
        theirs: Vec::new(),
        start_round: game.view().auth.turn().round,
        target: None,
        aim_enemy: None,
        aim_learner: None,
        raid: None,
        control,
        over: false,
        win: None,
        cities0: cities_of(&game, learner).len(),
        n0: 0,
        imp: Vec::new(),
        pillaged: BTreeSet::new(),
        d0: 0.0,
        set_out: None,
        arrived: None,
        near_first: 0,
        near_plus2: 0,
        taken: None,
        trigger: None,
        reinforce_kinds: Vec::new(),
        lost_at_trigger: 0,
        trig_near: Vec::new(),
        min_dist: f64::MAX,
        founded: None,
        info: BTreeMap::new(),
        trace: Vec::new(),
        wounded_start: 0,
    };
    run.info.insert("era".into(), game.view().auth.players().nation_era_of(learner, rules).to_string());
    run.info.insert("enemy_era".into(), game.view().auth.players().nation_era_of(enemy, rules).to_string());
    run.info.insert("world".into(), world.label.clone());
    match sc {
        Sc::T1a | Sc::T1b => {
            let (n_melee, m) = if sc == Sc::T1a { (1, 2) } else { (2, 3) };
            let melee = *mine_k.get(&Arm::Melee).ok_or("no melee kind")?;
            let ranged = *mine_k.get(&Arm::Ranged).ok_or("no ranged kind")?;
            let mut units = vec![melee; n_melee];
            units.push(ranged);
            let others = vec![melee; m];
            let step = rung(units.len() as u32, m as u32, true, Foe::Same, 100, 3, 1, 10);
            let s = skirmish_scene(rules, &game, learner, enemy, step, units, others, Some(v))?;
            for (k, t) in s.learner.0.iter().zip(&s.learner.1) {
                let role = if *k == ranged { "ranged" } else { "melee" };
                run.mine.push(spawn(&mut game, rules, learner, *t, *k, role));
            }
            for (k, t) in s.enemy.0.iter().zip(&s.enemy.1) {
                run.theirs.push(spawn(&mut game, rules, enemy, *t, *k, "foe"));
            }
            let keep: Vec<UnitHandle> = run.theirs.iter().map(|p| p.h).collect();
            let c0 = run.mine[0].clone();
            let at = game.view().auth.units().get(c0.h).map_or(own, |u| u.core.tile);
            let cleared = clear_enemy_near(&mut game, rules, enemy, at, 9, &keep);
            run.info.insert("cleared".into(), cleared.to_string());
            run.info.insert("kinds".into(), format!("{} + {} vs {} x{m}", name(rules, melee), name(rules, ranged), name(rules, melee)));
            reveal(&mut game, rules, learner, own, 10);
            reveal(&mut game, rules, enemy, own, 10);
        }
        Sc::T2 | Sc::T4 => {
            let worker = civil_kind(rules, false).ok_or("no worker kind")?;
            let guard = guard_kind(&mine_k, bit(1)).ok_or("no guard kind")?;
            let raider = raider_kind(&theirs_k).ok_or("no raider kind")?;
            let free = free_land(rules, &game, area);
            let homes: Vec<TileId> = cities_of(&game, learner).into_iter().filter(|c| game.view().auth.map().land_area(*c) == area).collect();
            if homes.is_empty() {
                return Err("the learner has no city on the shared land".to_owned());
            }
            let n_workers = if sc == Sc::T2 { 1 } else { 2 };
            let n_guards = if sc == Sc::T2 { 1 } else { 1 + usize::from(bit(2)) };
            let n_raiders = if sc == Sc::T2 { 1 + usize::from(bit(3)) } else { 2 };
            let mut spots: Vec<TileId> = Vec::new();
            for w in 0..n_workers {
                let home = homes[(mix(v ^ 0x40 ^ w as u64) % homes.len() as u64) as usize];
                let near: Vec<TileId> = free
                    .iter()
                    .copied()
                    .filter(|&t| (3..=4).contains(&dist(&game, t, home)) && owner_of(&game, t) == Some(learner) && spots.iter().all(|s| dist(&game, *s, t) >= if w > 0 { 4 } else { 1 }))
                    .collect();
                let near = if near.is_empty() {
                    free.iter().copied().filter(|&t| (3..=4).contains(&dist(&game, t, home)) && spots.iter().all(|s| dist(&game, *s, t) >= if w > 0 { 3 } else { 1 })).collect()
                } else {
                    near
                };
                spots.push(pick_free(near, v ^ 0x41 ^ w as u64).ok_or("no tile for a worker")?);
            }
            for (i, t) in spots.iter().enumerate() {
                run.mine.push(spawn(&mut game, rules, learner, *t, worker, "worker"));
                if i < n_guards {
                    let beside: Vec<TileId> = free.iter().copied().filter(|&u| u != *t && dist(&game, u, *t) == 1 && game.view().auth.indexes().units_on(u).is_empty()).collect();
                    let g = pick_free(beside, v ^ 0x42 ^ i as u64).ok_or("no tile for a guard")?;
                    run.mine.push(spawn(&mut game, rules, learner, g, guard, "guard"));
                }
            }
            if !control {
                for r in 0..n_raiders {
                    let w = spots[r % spots.len()];
                    let mut cands: Vec<TileId> = free
                        .iter()
                        .copied()
                        .filter(|&t| (5..=6).contains(&dist(&game, t, w)) && homes.iter().all(|c| dist(&game, *c, t) > 2) && game.view().auth.indexes().units_on(t).is_empty() && run.theirs.iter().all(|p| game.view().auth.units().get(p.h).is_some_and(|u| u.core.tile != t)))
                        .collect();
                    // Toward the enemy's side first.
                    cands.sort_by_key(|&t| (dist(&game, t, far), t.0));
                    cands.truncate(8);
                    let t = pick_free(cands, v ^ 0x43 ^ r as u64).ok_or("no tile for a raider")?;
                    run.theirs.push(spawn(&mut game, rules, enemy, t, raider, "raider"));
                }
            }
            let keep: Vec<UnitHandle> = run.theirs.iter().map(|p| p.h).collect();
            let cleared: usize = spots.iter().map(|sp| clear_enemy_near(&mut game, rules, enemy, *sp, 11, &keep)).sum();
            run.info.insert("cleared".into(), cleared.to_string());
            reveal(&mut game, rules, learner, spots[0], 9);
            reveal(&mut game, rules, enemy, spots[0], 9);
            run.info.insert("kinds".into(), format!("{} + {} vs {} x{}", name(rules, worker), name(rules, guard), name(rules, raider), run.theirs.len()));
        }
        Sc::T3 => {
            let settler = civil_kind(rules, true).ok_or("no settler kind")?;
            let guard = guard_kind(&mine_k, bit(1)).ok_or("no guard kind")?;
            let raider = raider_kind(&theirs_k).ok_or("no raider kind")?;
            let free = free_land(rules, &game, area);
            let homes: Vec<TileId> = cities_of(&game, learner).into_iter().filter(|c| game.view().auth.map().land_area(*c) == area).collect();
            if homes.is_empty() {
                return Err("the learner has no city on the shared land".to_owned());
            }
            let all_cities: Vec<TileId> = (0..game.view().auth.players().count() as u8).flat_map(|p| cities_of(&game, PlayerId(p))).collect();
            let home = homes[(mix(v ^ 0x50) % homes.len() as u64) as usize];
            let starts: Vec<TileId> = free.iter().copied().filter(|&t| dist(&game, t, home) == 2 && owner_of(&game, t) == Some(learner)).collect();
            let s = pick_free(starts, v ^ 0x51).ok_or("no tile for a settler")?;
            // A site the rules accept: tried on a copy, 5-6 tiles away, nobody else's land.
            let mut sites: Vec<TileId> = free
                .iter()
                .copied()
                .filter(|&t| (5..=6).contains(&dist(&game, t, s)) && matches!(owner_of(&game, t), None) | (owner_of(&game, t) == Some(learner)))
                .filter(|&t| all_cities.iter().all(|c| dist(&game, *c, t) >= 4))
                .collect();
            sites.sort_by_key(|t| t.0);
            shuffle(&mut sites, v ^ 0x52);
            let mut site = None;
            for &t in sites.iter().take(24) {
                let mut trial = game.clone();
                let u = trial.debug_spawn_unit(rules, learner, t, settler);
                if trial.accepts_order(rules, learner, &Command::FoundCity { settler: u }) {
                    site = Some(t);
                    break;
                }
            }
            let site = site.ok_or("no site a settler could found a city on")?;
            run.mine.push(spawn(&mut game, rules, learner, s, settler, "settler"));
            let beside: Vec<TileId> = free.iter().copied().filter(|&u| u != s && dist(&game, u, s) == 1 && game.view().auth.indexes().units_on(u).is_empty()).collect();
            let g = pick_free(beside, v ^ 0x53).ok_or("no tile for the escort")?;
            run.mine.push(spawn(&mut game, rules, learner, g, guard, "guard"));
            if !control {
                let mut cands: Vec<TileId> = free
                    .iter()
                    .copied()
                    .filter(|&t| (4..=5).contains(&dist(&game, t, s)) && dist(&game, t, site) <= 3 && game.view().auth.indexes().units_on(t).is_empty())
                    .collect();
                cands.sort_by_key(|t| t.0);
                let t = pick_free(cands, v ^ 0x54).ok_or("no tile for the raider")?;
                run.theirs.push(spawn(&mut game, rules, enemy, t, raider, "raider"));
            }
            run.target = Some(site);
            let keep: Vec<UnitHandle> = run.theirs.iter().map(|p| p.h).collect();
            let cleared = clear_enemy_near(&mut game, rules, enemy, s, 10, &keep) + clear_enemy_near(&mut game, rules, enemy, site, 6, &keep);
            run.info.insert("cleared".into(), cleared.to_string());
            reveal(&mut game, rules, learner, site, 8);
            reveal(&mut game, rules, enemy, site, 8);
            run.info.insert("kinds".into(), format!("{} + {} vs {}", name(rules, settler), name(rules, guard), name(rules, raider)));
            run.info.insert("site_dist".into(), dist(&game, s, site).to_string());
        }
        Sc::T5 | Sc::T5r | Sc::T5b => {
            let fire = classic_rules::combat::city::CITY_ATTACK_RANGE;
            let enemy_cities = cities_of(&game, enemy);
            let auth = game.view().auth;
            let map = auth.map();
            let n = 2 + usize::from(bit(4));
            // The improved, unpillaged tiles of the enemy on the shared land, by the city they lie around.
            let improved: Vec<TileId> = (0..map.tile_count() as u32)
                .map(TileId)
                .filter(|&t| map.land_area(t) == area && map.owner(t).flatten() == Some(enemy) && matches!(map.improvement(t), Some(Some(_))) && !map.pillaged(t) && auth.indexes().units_on(t).is_empty())
                .collect();
            let near_fire = |t: TileId| enemy_cities.iter().map(|c| dist(&game, t, *c)).min().unwrap_or(i32::MAX);
            let wanted = |t: TileId| if sc == Sc::T5b { (2..=fire).contains(&near_fire(t)) } else { near_fire(t) > fire };
            let mut best: Option<(usize, TileId)> = None;
            for &c in &enemy_cities {
                if map.land_area(c) != area {
                    continue;
                }
                let k = improved.iter().filter(|&&t| wanted(t) && dist(&game, t, c) <= 4).count();
                if k >= 3 && best.is_none_or(|b| k > b.0) {
                    best = Some((k, c));
                }
            }
            let (_, city) = best.ok_or("no enemy city has three improved tiles of the kind")?;
            let tiles: Vec<TileId> = improved.iter().copied().filter(|&t| wanted(t) && dist(&game, t, city) <= 4).collect();
            let pill = guard_kind_fast(&mine_k).ok_or("no fast or melee kind")?;
            let free = free_land(rules, &game, area);
            // Start tiles: 2-3 tiles from the nearest improved tile, outside the strike (T5/T5b both), nearest the learner's side.
            let mut starts: Vec<TileId> = free
                .iter()
                .copied()
                .filter(|&t| tiles.iter().map(|i| dist(&game, t, *i)).min().is_some_and(|d| (2..=3).contains(&d)) && enemy_cities.iter().all(|c| dist(&game, t, *c) > fire) && owner_of(&game, t) != Some(learner))
                .collect();
            starts.sort_by_key(|&t| (dist(&game, t, own), t.0));
            starts.truncate(12);
            let mut chosen: Vec<TileId> = Vec::new();
            shuffle(&mut starts, v ^ 0x60);
            for t in starts {
                if chosen.len() < n && chosen.iter().all(|c| dist(&game, *c, t) <= 3) {
                    chosen.push(t);
                }
            }
            if chosen.len() < n {
                return Err("no start tiles near the improved land".to_owned());
            }
            let imp_n = tiles.len();
            for (i, t) in chosen.iter().enumerate() {
                let p = spawn(&mut game, rules, learner, *t, pill, "raider");
                if sc == Sc::T5b && i == 0 {
                    game.debug_set_unit_hp(p.h, (u32::from(kernel::world::unit::UNIT_MAX_HP) * 40 / 100) as u16);
                    run.wounded_start = 1;
                }
                run.mine.push(p);
            }
            run.imp = improved.iter().copied().filter(|&t| dist(&game, t, city) <= 5).collect();
            run.target = Some(city);
            if sc == Sc::T5r {
                run.raid = Some(city);
            }
            reveal(&mut game, rules, learner, city, 8);
            reveal(&mut game, rules, enemy, city, 8);
            run.info.insert("kinds".into(), format!("{} x{n}", name(rules, pill)));
            run.info.insert("improved".into(), imp_n.to_string());
        }
        Sc::C1 | Sc::C3 => {
            let homes: Vec<TileId> = cities_of(&game, learner).into_iter().filter(|c| game.view().auth.map().land_area(*c) == area).collect();
            if homes.len() < 2 {
                return Err(format!("the learner has {} cities on the shared land, the scene needs 2", homes.len()));
            }
            let enemy_cities: Vec<TileId> = cities_of(&game, enemy).into_iter().filter(|c| game.view().auth.map().land_area(*c) == area).collect();
            // Muster cities: up to three, the nearest the enemy's.
            let mut ordered = homes.clone();
            ordered.sort_by_key(|&c| (enemy_cities.iter().map(|e| dist(&game, c, *e)).min().unwrap_or(i32::MAX), c.0));
            ordered.truncate(3);
            let reach_of = |game: &GameState, c: TileId| ordered.iter().map(|m| dist(game, *m, c)).min().unwrap_or(i32::MAX);
            let reach = |c: TileId| reach_of(&world.game, c);
            // The enemy city nearest the muster cities (no other enemy city on the way), 8-12 tiles away.
            let nearest = enemy_cities.iter().copied().min_by_key(|&c| (reach(c), c.0)).ok_or("the enemy has no city on the shared land")?;
            if !(8..=12).contains(&reach(nearest)) {
                return Err(format!("the nearest enemy city is {} tiles from the muster cities", reach(nearest)));
            }
            let target = nearest;
            let target_dist = reach(target);
            let units = plan(&mine_k, [3, 1, 3, 3], 12, v ^ 0x71)?;
            let free = free_land(rules, &game, area);
            let mut used: Vec<TileId> = Vec::new();
            for (i, k) in units.iter().enumerate() {
                let city = ordered[i % ordered.len()];
                let mut cands: Vec<TileId> = free.iter().copied().filter(|&t| (1..=2).contains(&dist(&game, t, city)) && !used.contains(&t)).collect();
                cands.sort_by_key(|&t| (dist(&game, t, city), dist(&game, t, target), t.0));
                let t = *cands.first().ok_or("no room around a muster city")?;
                used.push(t);
                run.mine.push(spawn(&mut game, rules, learner, t, *k, "army"));
            }
            run.n0 = run.mine.len();
            run.target = Some(target);
            run.aim_learner = Some(target);
            run.aim_enemy = Some(ordered[0]);
            if sc == Sc::C3 {
                let best = battle::best_kinds(rules, &kinds_of(enemy));
                run.reinforce_kinds = plan(&best, [3, 0, 3, 0], 12, v ^ 0x72)?;
            }
            let cleared = clear_enemy_near(&mut game, rules, enemy, target, 8, &[]);
            run.info.insert("cleared".into(), cleared.to_string());
            reveal(&mut game, rules, learner, target, 14);
            reveal(&mut game, rules, enemy, target, 14);
            run.info.insert("kinds".into(), units.iter().map(|k| name(rules, *k)).collect::<BTreeSet<_>>().into_iter().collect::<Vec<_>>().join(","));
            run.info.insert("muster_cities".into(), ordered.len().to_string());
            run.info.insert("target_dist".into(), target_dist.to_string());
            run.info.insert("garrison".into(), {
                let auth = game.view().auth;
                (0..auth.players().count() as u8).filter(|&p| PlayerId(p) == enemy).map(|p| auth.indexes().units_of(PlayerId(p)).iter().filter(|h| auth.units().get(**h).is_some_and(|u| dist(&game, u.core.tile, target) <= 2)).count()).sum::<usize>().to_string()
            });
        }
    }
    run.n0 = run.n0.max(run.mine.len());
    run.d0 = run.mean_dist(&game);
    Ok((game, run))
}

/// The fastest kind: mounted, else melee.
fn guard_kind_fast(mine: &BTreeMap<Arm, UnitKindId>) -> Option<UnitKindId> {
    mine.get(&Arm::Mounted).or_else(|| mine.get(&Arm::Melee)).copied()
}

impl Run {
    fn alive_h(&self, game: &GameState, p: &Piece, side: PlayerId) -> Option<kernel::world::unit::UnitCore> {
        game.view().auth.units().get(p.h).filter(|u| u.core.owner == side && u.core.id == p.id).map(|u| *u.core)
    }

    fn mine_alive(&self, game: &GameState) -> Vec<(Piece, kernel::world::unit::UnitCore)> {
        self.mine.iter().filter_map(|p| self.alive_h(game, p, self.learner).map(|c| (p.clone(), c))).collect()
    }

    fn theirs_alive(&self, game: &GameState) -> Vec<(Piece, kernel::world::unit::UnitCore)> {
        self.theirs.iter().filter_map(|p| self.alive_h(game, p, self.enemy).map(|c| (p.clone(), c))).collect()
    }

    /// The mean distance of the army to the target (0 without).
    fn mean_dist(&self, game: &GameState) -> f64 {
        let Some(t) = self.target else { return 0.0 };
        let v: Vec<f64> = self.mine_alive(game).iter().map(|(_, c)| f64::from(dist(game, c.tile, t))).collect();
        if v.is_empty() { 0.0 } else { v.iter().sum::<f64>() / v.len() as f64 }
    }

    /// Before the first round: the war memory the sides have.
    pub fn prepare<P: AiMind<Classic>>(&self, driver: &mut AiDriver<Classic, P>, game: &GameState, reference: Option<&str>) {
        let round = game.view().auth.turn().round;
        if let Some(t) = self.aim_enemy {
            battle::aim(driver, game, self.enemy, t, round);
        }
        if let Some(t) = self.aim_learner {
            battle::aim(driver, game, self.learner, t, round);
        }
        if let (Some(city), true) = (self.raid, matches!(self.sc, Sc::T5r | Sc::T5b)) {
            let auth = game.view().auth;
            if let Some(c) = auth.indexes().city_on_tile(city).and_then(|c| auth.cities().get(c)) {
                driver.state_mut(self.learner, round).see_city(SeenCity { tile: city, owner: c.owner, population: c.population, last_seen_round: round });
            }
        }
        self.tasks(driver, game, reference);
    }

    /// Tasks renewed every round: raiders hunt our civilians, a raid holds its task, a reference presses.
    pub fn tasks<P: AiMind<Classic>>(&self, driver: &mut AiDriver<Classic, P>, game: &GameState, _reference: Option<&str>) {
        let auth = game.view().auth;
        let round = auth.turn().round;
        match self.sc {
            Sc::T1a | Sc::T1b => {
                // The foes hunt the nearest of our units (an engagement in the open, not a march on our city).
                let mine = self.mine_alive(game);
                for (r, rc) in self.theirs_alive(game) {
                    if let Some((_, c)) = mine.iter().min_by_key(|(_, c)| (dist(game, c.tile, rc.tile), c.tile.0)) {
                        let state = driver.state_mut(self.enemy, round);
                        state.relation_mut(self.learner).objective = Some(WarObjective { tile: c.tile, kind: ObjectiveKind::Territory, chosen_round: round });
                        state.set_task(r.id, UnitTask::Assault { target: c.tile }, round);
                    }
                }
            }
            Sc::T5r | Sc::T5b => {
                if let Some(city) = self.raid {
                    let state = driver.state_mut(self.learner, round);
                    for (p, _) in self.mine_alive(game) {
                        state.set_task(p.id, UnitTask::Raid { target: city }, round);
                    }
                }
            }
            _ => {}
        }
    }

    /// `ref-press`: every army unit presses on the city each turn. A shooter fires at the city, else at the
    /// nearest foe in range; a melee unit attacks the city when next to it, else a foe next to it; else the unit
    /// walks to the tile within its move nearest the city. Returns commands made.
    fn press_turn(&mut self, rules: &CompiledRuleset, game: &mut GameState) -> u64 {
        let Some(target) = self.target else { return 0 };
        if game.view().auth.active_player() != self.learner {
            return 0;
        }
        let mut made = 0;
        for _ in 0..3 {
            for (p, c) in self.mine_alive(game) {
                let city_h = game.view().auth.indexes().city_on_tile(target);
                let ranged = classic_rules::ai::combat::ranged_range(rules, c.kind).is_some();
                let mut cmds: Vec<Command> = Vec::new();
                if ranged {
                    cmds.push(Command::RangedAttack { unit: p.h, target });
                } else if let Some(city) = city_h {
                    cmds.push(Command::AttackCity { attacker: p.h, city });
                }
                // Foes next to or near the unit.
                let auth = game.view().auth;
                let mut foes: Vec<(UnitHandle, TileId)> = auth.indexes().units_of(self.enemy).iter().filter_map(|h| auth.units().get(*h).map(|u| (*h, u.core.tile))).filter(|(_, t)| dist(game, c.tile, *t) <= if ranged { 3 } else { 1 }).collect();
                foes.sort_by_key(|(_, t)| (dist(game, c.tile, *t), t.0));
                for (fh, ft) in foes {
                    cmds.push(if ranged { Command::RangedAttack { unit: p.h, target: ft } } else { Command::AttackUnit { attacker: p.h, defender: fh } });
                }
                let mut acted = false;
                for cmd in cmds {
                    if game.accepts_order(rules, self.learner, &cmd) && game.execute(rules, cmd).is_ok() {
                        made += 1;
                        acted = true;
                        break;
                    }
                }
                if acted || dist(game, c.tile, target) <= 1 {
                    continue;
                }
                let steps = rules.units.get(c.kind.0 as usize).map_or(1, |d| i32::from(d.movement.max(1)));
                let auth = game.view().auth;
                let mut tiles: Vec<TileId> = (0..auth.map().tile_count() as u32)
                    .map(TileId)
                    .filter(|t| *t != c.tile && dist(game, c.tile, *t) <= steps && auth.indexes().units_on(*t).is_empty() && auth.indexes().city_on_tile(*t).is_none() && dist(game, *t, target) < dist(game, c.tile, target))
                    .collect();
                tiles.sort_by_key(|&t| (dist(game, t, target), t.0));
                tiles.truncate(10);
                for t in tiles {
                    let cmd = Command::MoveUnit { unit: p.h, destination: t };
                    if game.accepts_order(rules, self.learner, &cmd) && game.execute(rules, cmd).is_ok() {
                        made += 1;
                        break;
                    }
                }
            }
        }
        made
    }

    /// The raiders' turn (T2-T4), scripted and the same for every player: each raider attacks the best prey
    /// in reach (a civilian first; a fighter when the exchange is not bad), else walks toward the nearest
    /// prey outside the strike of the learner's cities, then attacks if it can. Stands in for the barbarians,
    /// which this minimal world has none of. Run at the enemy's turn start. Returns commands made.
    pub fn raiders_turn(&mut self, rules: &CompiledRuleset, game: &mut GameState) -> u64 {
        use classic_rules::combat::damage::Roll;
        use classic_rules::combat::exchange::{plan_attack, AttackKind};
        if !matches!(self.sc, Sc::T2 | Sc::T3 | Sc::T4) || game.view().auth.active_player() != self.enemy {
            return 0;
        }
        let mut made = 0;
        let fire = classic_rules::combat::city::CITY_ATTACK_RANGE;
        for raider in self.theirs.clone() {
            for _ in 0..2 {
                let Some(me) = self.alive_h(game, &raider, self.enemy) else { break };
                let prey = self.mine_alive(game);
                let ranged = classic_rules::ai::combat::ranged_range(rules, me.kind).is_some();
                let mut best: Option<(i64, Command)> = None;
                for (p, c) in &prey {
                    let (cmd, how) = if ranged { (Command::RangedAttack { unit: raider.h, target: c.tile }, AttackKind::Ranged) } else { (Command::AttackUnit { attacker: raider.h, defender: p.h }, AttackKind::Melee) };
                    if !game.accepts_order(rules, self.enemy, &cmd) {
                        continue;
                    }
                    let civilian = p.role == "worker" || p.role == "settler";
                    let Ok(plan) = plan_attack(game.view(), rules, raider.h, c.tile, how) else { continue };
                    let (dealt, taken) = (i64::from(plan.damage_to_defender(Roll::Expected)), i64::from(plan.damage_to_attacker(Roll::Expected)));
                    if !civilian && !(dealt >= i64::from(c.hp) || (dealt >= taken && taken < i64::from(me.hp))) {
                        continue;
                    }
                    let score = if civilian { 2000 } else { 0 } + if dealt >= i64::from(c.hp) { 1000 } else { 0 } + dealt - taken;
                    if best.as_ref().is_none_or(|b| score > b.0) {
                        best = Some((score, cmd));
                    }
                }
                if let Some((_, cmd)) = best {
                    if game.execute(rules, cmd).is_ok() {
                        made += 1;
                    }
                    break;
                }
                // Walk toward the nearest prey, never into the strike of a city of the learner.
                let Some(goal) = prey.iter().map(|(_, c)| c.tile).min_by_key(|t| (dist(game, me.tile, *t), t.0)) else { break };
                let steps = rules.units.get(me.kind.0 as usize).map_or(1, |d| i32::from(d.movement.max(1)));
                let cities = cities_of(game, self.learner);
                let auth = game.view().auth;
                let mut tiles: Vec<TileId> = (0..auth.map().tile_count() as u32)
                    .map(TileId)
                    .filter(|t| *t != me.tile && dist(game, me.tile, *t) <= steps && auth.indexes().units_on(*t).is_empty() && auth.indexes().city_on_tile(*t).is_none() && cities.iter().all(|c| dist(game, *c, *t) > fire))
                    .collect();
                tiles.sort_by_key(|&t| (dist(game, t, goal), t.0));
                tiles.truncate(12);
                let mut moved = false;
                for t in tiles {
                    let cmd = Command::MoveUnit { unit: raider.h, destination: t };
                    if dist(game, t, goal) < dist(game, me.tile, goal) && game.accepts_order(rules, self.enemy, &cmd) && game.execute(rules, cmd).is_ok() {
                        made += 1;
                        moved = true;
                        break;
                    }
                }
                if !moved {
                    break;
                }
            }
        }
        made
    }

    /// The scripted pillage reference at the learner's turn start (`ref-pillage`): each unit walks to
    /// the nearest unpillaged improvement of the enemy and pillages it. Returns commands made.
    pub fn reference_turn(&mut self, rules: &CompiledRuleset, game: &mut GameState) -> u64 {
        let mut made = 0;
        if matches!(self.sc, Sc::C1 | Sc::C3) {
            return self.press_turn(rules, game);
        }
        if !matches!(self.sc, Sc::T5 | Sc::T5r | Sc::T5b) || game.view().auth.active_player() != self.learner {
            return 0;
        }
        for _ in 0..3 {
            for (p, c) in self.mine_alive(game) {
                let imp: Vec<TileId> = self.imp.iter().copied().filter(|&t| !game.view().auth.map().pillaged(t) && matches!(game.view().auth.map().improvement(t), Some(Some(_)))).collect();
                if imp.contains(&c.tile) {
                    let cmd = Command::Pillage { unit: p.h };
                    if game.accepts_order(rules, self.learner, &cmd) && game.execute(rules, cmd).is_ok() {
                        made += 1;
                    }
                    continue;
                }
                let Some(goal) = imp.iter().copied().min_by_key(|&t| (dist(game, c.tile, t), t.0)) else { continue };
                let steps = rules.units.get(c.kind.0 as usize).map_or(1, |d| i32::from(d.movement.max(1)));
                let auth = game.view().auth;
                let mut tiles: Vec<TileId> = (0..auth.map().tile_count() as u32)
                    .map(TileId)
                    .filter(|t| *t != c.tile && dist(game, c.tile, *t) <= steps && (auth.indexes().units_on(*t).is_empty()))
                    .collect();
                tiles.sort_by_key(|&t| (dist(game, t, goal), t.0));
                tiles.truncate(12);
                for t in tiles {
                    let cmd = Command::MoveUnit { unit: p.h, destination: t };
                    if game.accepts_order(rules, self.learner, &cmd) && game.execute(rules, cmd).is_ok() {
                        made += 1;
                        break;
                    }
                }
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
        made
    }

    /// After a round: updates the measures, the trace line; whether the scene is over.
    pub fn after_round<P: AiMind<Classic>>(&mut self, rules: &CompiledRuleset, game: &mut GameState, driver: &mut AiDriver<Classic, P>) -> bool {
        let round = game.view().auth.turn().round;
        let rel = round.saturating_sub(self.start_round);
        let mine = self.mine_alive(game);
        let theirs = self.theirs_alive(game);
        let auth = game.view().auth;
        // Pillage bookkeeping.
        for &t in &self.imp {
            if auth.map().pillaged(t) {
                self.pillaged.insert(t);
            }
        }
        // Operation bookkeeping.
        if let Some(target) = self.target.filter(|_| matches!(self.sc, Sc::C1 | Sc::C3)) {
            let d = self.mean_dist(game);
            if self.set_out.is_none() && d <= self.d0 - 2.0 {
                self.set_out = Some(rel);
            }
            let near = mine.iter().filter(|(_, c)| dist(game, c.tile, target) <= 3).count() as u32;
            if self.arrived.is_none() && near > 0 {
                self.arrived = Some(rel);
                self.near_first = near;
            }
            if let Some(a) = self.arrived {
                if rel == a + 2 {
                    self.near_plus2 = near;
                }
            }
            if let Some(m) = mine.iter().map(|(_, c)| f64::from(dist(game, c.tile, target))).reduce(f64::min) {
                self.min_dist = self.min_dist.min(m);
            }
            if auth.indexes().city_on_tile(target).and_then(|c| auth.cities().get(c)).is_some_and(|c| c.owner == self.learner) && self.taken.is_none() {
                self.taken = Some(rel);
            }
            // C3: the reinforcement when the first unit comes within 6 tiles.
            if self.sc == Sc::C3 && self.trigger.is_none() && mine.iter().any(|(_, c)| dist(game, c.tile, target) <= 6) {
                self.trigger = Some(rel);
                self.lost_at_trigger = (self.n0 - mine.len()) as u32;
                self.trig_near = mine.iter().filter(|(_, c)| dist(game, c.tile, target) <= 6).map(|(p, _)| p.id).collect();
                let free: Vec<TileId> = free_land(rules, game, game.view().auth.map().land_area(target));
                let mut ring: Vec<TileId> = free.into_iter().filter(|&t| (1..=2).contains(&dist(game, t, target))).collect();
                ring.sort_by_key(|&t| (dist(game, t, target), t.0));
                let kinds = self.reinforce_kinds.clone();
                for (k, t) in kinds.iter().zip(ring) {
                    self.theirs.push(spawn(game, rules, self.enemy, t, *k, "reinforcement"));
                }
            }
        }
        if self.sc == Sc::T3 && self.founded.is_none() && cities_of(game, self.learner).len() > self.cities0 {
            self.founded = Some(rel);
        }
        self.trace.push(self.line(rules, game, rel));
        // Over?
        let limit = rel >= self.sc.rounds();
        let alive_enemy = theirs.iter().filter(|(p, _)| p.role != "reinforcement").count();
        let over = match self.sc {
            Sc::T1a | Sc::T1b => {
                if !theirs.is_empty() && !mine.is_empty() && !limit {
                    false
                } else if limit && !theirs.is_empty() {
                    self.win = Some(false);
                    true
                } else {
                    self.win = Some(theirs.is_empty() && !mine.is_empty());
                    true
                }
            }
            Sc::T2 | Sc::T4 => {
                let workers = mine.iter().filter(|(p, _)| p.role == "worker").count();
                let need = self.mine.iter().filter(|p| p.role == "worker").count();
                if workers < need && self.sc == Sc::T2 {
                    self.win = Some(false);
                    true
                } else if (!self.control && theirs.is_empty()) || limit {
                    self.win = Some(workers == need);
                    true
                } else {
                    false
                }
            }
            Sc::T3 => {
                if self.founded.is_some() {
                    self.win = Some(true);
                    true
                } else if !mine.iter().any(|(p, _)| p.role == "settler") {
                    self.win = Some(false);
                    true
                } else if limit {
                    self.win = Some(false);
                    true
                } else {
                    false
                }
            }
            Sc::T5 | Sc::T5r | Sc::T5b => {
                if mine.is_empty() || self.imp.iter().all(|t| self.pillaged.contains(t)) || limit {
                    self.win = None;
                    true
                } else {
                    false
                }
            }
            Sc::C1 => {
                if self.taken.is_some() || mine.is_empty() || limit {
                    self.win = Some(self.taken.is_some());
                    true
                } else {
                    false
                }
            }
            Sc::C3 => {
                if self.taken.is_some() || mine.is_empty() || limit {
                    let saved = mine.len() * 4 >= self.n0 * 3;
                    self.win = Some(self.trigger.is_some() && saved);
                    true
                } else {
                    false
                }
            }
        };
        let _ = alive_enemy;
        if !game.view().auth.players().is_alive(self.learner) {
            self.win.get_or_insert(false);
            self.over = true;
            return true;
        }
        if !over {
            self.tasks(driver, game, None);
        }
        self.over = over;
        over
    }

    /// The tasks a reference needs renewed (`ref-press`): called with the reference's name.
    pub fn renew_reference<P: AiMind<Classic>>(&self, driver: &mut AiDriver<Classic, P>, game: &GameState, reference: &str) {
        self.tasks(driver, game, Some(reference));
    }

    fn line(&self, rules: &CompiledRuleset, game: &GameState, rel: u32) -> String {
        let auth = game.view().auth;
        let part = |ps: &[Piece], side: PlayerId| -> String {
            ps.iter()
                .map(|p| {
                    let label = format!("{}[{}]", name(rules, p.kind).chars().take(8).collect::<String>(), p.role.chars().next().unwrap_or('?'));
                    match auth.units().get(p.h).filter(|u| u.core.id == p.id) {
                        Some(u) if u.core.owner == side => format!("{label}@{}h{}", u.core.tile.0, u.core.hp),
                        Some(u) => format!("{label}@{}->p{}", u.core.tile.0, u.core.owner.0),
                        None => {
                            // Captured units come back under another id: find the kind on the other side near.
                            let elsewhere = auth.units().iter_cores().find(|u| u.id == p.id).map(|u| format!("->p{}@{}", u.owner.0, u.tile.0));
                            let copies: Vec<String> = auth.units().iter_cores().filter(|u| u.kind == p.kind && u.owner != side).map(|u| format!("p{}@{}", u.owner.0, u.tile.0)).take(4).collect();
                            format!("{label}+gone{} {}", elsewhere.unwrap_or_default(), copies.join(","))
                        }
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        };
        let target = self.target.map_or_else(String::new, |t| {
            let c = auth.indexes().city_on_tile(t).and_then(|c| auth.cities().get(c)).map(|c| format!("p{} dmg{}", c.owner.0, c.damage)).unwrap_or_default();
            format!(" target@{} {c} meand{:.1}", t.0, self.mean_dist(game))
        });
        format!("r+{rel}{target} | mine: {} | foe: {} | pillaged {}", part(&self.mine, self.learner), part(&self.theirs, self.enemy), self.pillaged.len())
    }

    pub fn line_at_start(&self, rules: &CompiledRuleset, game: &GameState) -> String {
        self.line(rules, game, 0)
    }

    /// The raw measures and the verdict at the end.
    pub fn finish(&self, game: &GameState) -> (Option<bool>, BTreeMap<String, f64>) {
        let mut m = BTreeMap::new();
        let mine = self.mine_alive(game);
        let theirs = self.theirs_alive(game);
        m.insert("alive".to_owned(), mine.len() as f64);
        m.insert("units".to_owned(), self.mine.len() as f64);
        m.insert("lost".to_owned(), (self.mine.len() - mine.len()) as f64);
        m.insert("foes_left".to_owned(), theirs.len() as f64);
        match self.sc {
            Sc::T2 | Sc::T4 => {
                let workers = mine.iter().filter(|(p, _)| p.role == "worker").count();
                m.insert("workers_alive".to_owned(), workers as f64);
                m.insert("one_saved".to_owned(), f64::from(u8::from(workers >= 1)));
            }
            Sc::T3 => {
                m.insert("founded_round".to_owned(), self.founded.map_or(-1.0, f64::from));
                m.insert("settler_alive".to_owned(), f64::from(u8::from(mine.iter().any(|(p, _)| p.role == "settler"))));
            }
            Sc::T5 | Sc::T5r | Sc::T5b => {
                m.insert("pillaged".to_owned(), self.pillaged.len() as f64);
                m.insert("improved".to_owned(), self.imp.len() as f64);
                m.insert("units_kept".to_owned(), mine.len() as f64);
            }
            Sc::C1 | Sc::C3 => {
                m.insert("set_out".to_owned(), self.set_out.map_or(-1.0, f64::from));
                m.insert("arrived".to_owned(), self.arrived.map_or(-1.0, f64::from));
                m.insert("near_first".to_owned(), f64::from(self.near_first));
                m.insert("near_plus2".to_owned(), f64::from(self.near_plus2));
                m.insert("taken".to_owned(), self.taken.map_or(-1.0, f64::from));
                m.insert("min_dist".to_owned(), if self.min_dist == f64::MAX { -1.0 } else { self.min_dist });
                m.insert("trigger".to_owned(), self.trigger.map_or(-1.0, f64::from));
                m.insert("lost_at_trigger".to_owned(), f64::from(self.lost_at_trigger));
                // Exam addition (not part of the win rule): the units within 6 tiles when the reinforcement came, and how many of them live at the end.
                m.insert("near_at_trigger".to_owned(), self.trig_near.len() as f64);
                m.insert("near_alive_end".to_owned(), mine.iter().filter(|(p, _)| self.trig_near.contains(&p.id)).count() as f64);
            }
            _ => {}
        }
        (self.win, m)
    }
}

// ---------------------------------------------------------------------------
// Playing a built scene

/// One played instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Row {
    pub scene: String,
    pub instance: usize,
    pub player: String,
    pub win: Option<bool>,
    pub rounds: u32,
    pub measures: BTreeMap<String, f64>,
    pub info: BTreeMap<String, String>,
    pub trace: Vec<String>,
}

/// Plays `run` on `game` with `player` (a variant name, or a reference `ref-…` that plays `standard`)
/// on the learner's seat and `standard` on the other. `live`: the trace on stderr as it goes.
pub fn play(rules: &CompiledRuleset, mut game: GameState, mut run: Run, instance: usize, player: &str, live: bool) -> Result<Row, String> {
    let mind = |n: &str| ClassicBench::mind(n).ok_or_else(|| format!("no AI variant {n}"));
    let reference = player.starts_with("ref-").then_some(player);
    // `v2`: the big net's only weights on disk (read only), in process (`net:v2:FILE`); EXAM_V2 names another file.
    let v2 = format!("net:v2:{}", std::env::var("EXAM_V2").unwrap_or_else(|_| "/Users/roman.ivanov/civ/work/v2-gpu/data/C-cont.v2".to_owned()));
    let base = if reference.is_some() { "standard" } else if player == "v2" { v2.as_str() } else { player };
    let nations = majors(&game);
    let mut seats = Vec::new();
    for p in &nations {
        if *p == run.learner {
            seats.push((*p, mind(base)?));
        } else {
            seats.push((*p, mind("standard")?));
        }
    }
    let mut driver = AiDriver::<Classic, kernel::ai::seats::Seated<ClassicMind>>::with_mind(ClassicBench::settings(rules), Vec::new(), kernel::ai::seats::Seated::new(mind("standard")?, seats));
    let dbg = std::env::var_os("EXAM_DEBUG").is_some();
    driver.set_trace_capacity(if dbg { 4000 } else { 0 });
    let mut alive_before = run.mine.len();
    run.prepare(&mut driver, &game, reference);
    let start = run.start_round;
    let first = run.line_at_start(rules, &game);
    run.trace.push(first);
    let mut over = false;
    let mut guard = 0;
    let mut raided: Option<u32> = None;
    while !game.is_over() && !over && guard < 200 {
        guard += 1;
        if reference.is_some() {
            run.reference_turn(rules, &mut game);
        }
        loop {
            let now_round = game.view().auth.turn().round;
            if game.view().auth.active_player() == run.enemy && raided != Some(now_round) {
                raided = Some(now_round);
                run.raiders_turn(rules, &mut game);
            }
            let stepped = driver.step(&mut game, rules);
            if dbg {
                let now = run.mine_alive(&game).len();
                if now < alive_before {
                    let tail: Vec<String> = driver.trace().entries().map(|e| format!("{e:?}")).collect();
                    for l in tail.iter().rev().take(6).rev() {
                        eprintln!("DEBUG loss ({alive_before}->{now}): {l}");
                    }
                }
                alive_before = now;
            }
            match stepped {
                kernel::ai::StepOutcome::Worked => {}
                kernel::ai::StepOutcome::PlayerDone(_) => {}
                kernel::ai::StepOutcome::RoundDone => break,
                kernel::ai::StepOutcome::Waiting if game.is_over() => break,
                kernel::ai::StepOutcome::Waiting => return Err(format!("round {} waits for a human", game.view().auth.turn().round)),
            }
        }
        if std::env::var("EXAM_DEBUG").is_ok_and(|v| v == "2") {
            let ids: Vec<EntityId> = run.theirs.iter().chain(&run.mine).map(|p| p.id).collect();
            let r = game.view().auth.turn().round;
            for e in driver.trace().entries().filter(|e| e.round + 1 == r && matches!(e.actor, kernel::ai::trace::Actor::Unit(id) if ids.contains(&id))) {
                eprintln!("DEBUG r{r} {:?} {:?} {:?} {:?}", e.actor, e.reason, e.command, e.outcome);
            }
        }
        over = run.after_round(rules, &mut game, &mut driver);
        if live {
            if let Some(l) = run.trace.last() {
                eprintln!("{} {} #{instance} {player}: {l}", run.sc.name(), run.info.get("world").map_or("", |s| s.as_str()));
            }
        }
        if !over {
            if let Some(r) = reference {
                run.renew_reference(&mut driver, &game, r);
            }
        }
    }
    let (win, measures) = run.finish(&game);
    let round = game.view().auth.turn().round;
    Ok(Row { scene: run.sc.name().to_owned(), instance, player: player.to_owned(), win, rounds: round.saturating_sub(start), measures, info: run.info.clone(), trace: run.trace.clone() })
}
