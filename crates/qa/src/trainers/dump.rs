//! `ai-exam suite-dump`: the parity source of the MLX tactical simulator (`nn/aurelia_nn/tacsim`).
//! A MEASUREMENT tool: it changes no rule and no AI.
//!
//! For a trainer scene (family, era, instance, attempt) it writes one JSON line: the scene's state in
//! a window of the map (tiles, the units with their compiled combat profiles and step-cost tables, the
//! cities, the combat RNG), then a scripted random play of both sides, move by move: for every unit
//! action the legal options the rules accept, the option drawn, and the state after it (units, cities,
//! the RNG, both sides' visibility). The MLX simulator rebuilds the scene from the line, replays the
//! same options and must match the state after every move.
//!
//! The scripted play: each side in turn; each scene unit (slot order) takes up to two actions, each
//! drawn uniformly among the accepted options and "nothing" (which ends the unit's turn); each city
//! may strike. Moves are restricted to the inner window (one tile off the border), so the window holds
//! the play. Then the side ends its turn. At a round end the closed arena takes off every unit of the
//! seats that is not a scene piece (as the trainers do). The play stops when a city changes hands,
//! when a side has no unit left, or after `--rounds` rounds.

use std::collections::BTreeMap;
use std::io::Write;

use classic_rules::combat::strength::{embarked_defense, Fighter};
use classic_rules::effects::unit::{combat_slot, unit_modifiers};
use classic_rules::ids::{PlayerId, TileId, UnitHandle};
use classic_rules::navigation::cost::{step_cost_for, StepCost};
use classic_rules::navigation::MoveProfile;
use classic_rules::rules::effects::{CombatContext, Condition, UnitAbility, UnitStat};
use classic_rules::classic::effects::{ClassicUnitAbility, ClassicUnitStat};
use classic_rules::{ClassicGame, Command, CompiledRuleset, GameState};
use kernel::ids::EntityId;
use kernel::world::unit::Stance;
use serde_json::{json, Value};

use super::act;
use super::families::{self, Fam};
use super::lab::{self, Draw};
use super::Scene;

/// Window side (tiles) in axial coordinates around the arena centre.
pub const WINDOW: i32 = 24;
/// Unit slots of a scene (pieces first, then units created in the play: captured civilians).
pub const SLOTS: usize = 24;
/// Moves stay this far inside the window: paths stay in it.
pub const INNER: i32 = 2;

const CONTEXTS: [CombatContext; 23] = [
    CombatContext::Any,
    CombatContext::Attack,
    CombatContext::Defense,
    CombatContext::VsCity,
    CombatContext::AttackOpen,
    CombatContext::AttackRough,
    CombatContext::DefenseOpen,
    CombatContext::DefenseRough,
    CombatContext::RangedAttackOpen,
    CombatContext::RangedAttackRough,
    CombatContext::DefenseVsRanged,
    CombatContext::RangedAttack,
    CombatContext::VsWounded,
    CombatContext::VsFortified,
    CombatContext::FriendlyLand,
    CombatContext::OutsideFriendlyLand,
    CombatContext::AttackFriendlyLand,
    CombatContext::Flanking,
    CombatContext::AdjacentFriendly,
    CombatContext::NearCapital,
    CombatContext::EmbarkedDefense,
    CombatContext::Interception,
    CombatContext::AirSweep,
];

/// The window: axial origin and side.
#[derive(Debug, Clone, Copy)]
pub struct Window {
    pub q0: i32,
    pub r0: i32,
    pub w: i32,
    pub map_w: i32,
    pub map_h: i32,
}

impl Window {
    fn around(game: &GameState, centre: TileId) -> Window {
        let dims = game.view().auth.map().dimensions;
        let (mw, mh) = (dims.width as i32, dims.height as i32);
        let (cq, cr) = (centre.0 as i32 % mw, centre.0 as i32 / mw);
        let q0 = (cq - WINDOW / 2).clamp(0, (mw - WINDOW).max(0));
        let r0 = (cr - WINDOW / 2).clamp(0, (mh - WINDOW).max(0));
        Window { q0, r0, w: WINDOW, map_w: mw, map_h: mh }
    }
    /// Window index of a map tile (None outside).
    pub fn index(&self, t: TileId) -> Option<i32> {
        let (q, r) = (t.0 as i32 % self.map_w, t.0 as i32 / self.map_w);
        let (x, y) = (q - self.q0, r - self.r0);
        (x >= 0 && y >= 0 && x < self.w && y < self.w).then_some(y * self.w + x)
    }
    pub fn tile(&self, i: i32) -> TileId {
        let (x, y) = (i % self.w, i / self.w);
        TileId(((self.r0 + y) * self.map_w + self.q0 + x) as u32)
    }
    /// Inside the window, INNER tiles off its border.
    fn inner(&self, t: TileId) -> bool {
        self.index(t).is_some_and(|i| {
            let (x, y) = (i % self.w, i / self.w);
            x >= INNER && y >= INNER && x < self.w - INNER && y < self.w - INNER
        })
    }
    fn tiles(&self) -> impl Iterator<Item = TileId> + '_ {
        (0..self.w * self.w).map(|i| self.tile(i))
    }
}

/// Axial neighbour offsets in the order the simulator uses (direction index 0..5).
pub const DIRS: [(i32, i32); 6] = [(1, 0), (1, -1), (0, -1), (-1, 0), (-1, 1), (0, 1)];

fn neighbour(win: &Window, t: TileId, d: usize) -> Option<TileId> {
    let (q, r) = (t.0 as i32 % win.map_w + DIRS[d].0, t.0 as i32 / win.map_w + DIRS[d].1);
    (q >= 0 && r >= 0 && q < win.map_w && r < win.map_h).then(|| TileId((r * win.map_w + q) as u32))
}

fn is_water(game: &GameState, t: TileId) -> bool {
    classic_rules::navigation::terrain::is_water(game.view().auth.map(), t)
}

fn deep(game: &GameState, t: TileId) -> bool {
    let map = game.view().auth.map();
    is_water(game, t) && map.dimensions.neighbors(t).into_iter().all(|n| is_water(game, n))
}

fn condition_json(rules: &CompiledRuleset, c: &Condition) -> Value {
    match c {
        Condition::TileFeature(f) => json!(["feature", f.0]),
        Condition::TileTerrain(t) => json!(["terrain", t.0]),
        Condition::OpponentRole(r) => json!(["role", format!("{r:?}")]),
        Condition::OpponentDomain(d) => json!(["domain", format!("{d:?}")]),
        Condition::OpponentClassOf(k) => json!(["class", rules.units.get(k.0 as usize).and_then(|d| d.class.clone()).unwrap_or_default()]),
        Condition::UnitDomain(_) | Condition::UnitRole(_) => json!(["always"]),
        other => json!(["other", format!("{other:?}")]),
    }
}

/// The compiled combat and movement profile of a unit (its kind, promotions and owner).
fn profile(rules: &CompiledRuleset, game: &GameState, win: &Window, h: UnitHandle) -> Value {
    let auth = game.view().auth;
    let core = *auth.units().get(h).unwrap().core;
    let mods = unit_modifiers(auth, rules, &core);
    let def = &rules.units[core.kind.0 as usize];
    let fighter = Fighter::of(auth, rules, h).unwrap();
    let contexts: Vec<i32> = CONTEXTS.iter().map(|c| mods.combat[combat_slot(*c)]).collect();
    let terms: Vec<Value> = mods.situational.iter().map(|t| json!({"ctx": combat_slot(t.context), "cond": condition_json(rules, &t.condition), "pct": t.percent})).collect();
    let abilities = [
        ("move_after_attacking", UnitAbility::MoveAfterAttacking),
        ("heavy_charge", UnitAbility::HeavyCharge),
        ("river_crossing", UnitAbility::RiverCrossing),
        ("amphibious", UnitAbility::Amphibious),
        ("no_defensive_bonus", UnitAbility::NoDefensiveBonus),
        ("ranged_support_fire", UnitAbility::RangedSupportFire),
        ("only_defensive", UnitAbility::OnlyDefensive),
        ("city_attack_only", UnitAbility::CityAttackOnly),
        ("must_set_up", UnitAbility::MustSetUp),
        ("indirect_fire", UnitAbility::IndirectFire),
        ("no_capture", UnitAbility::NoCapture),
        ("capture_defeated", UnitAbility::CaptureDefeatedEnemy),
        ("always_heal", UnitAbility::AlwaysHeal),
        ("sapper", UnitAbility::Sapper),
        ("ignore_zoc", UnitAbility::IgnoreZoneOfControl),
        ("heal_outside_friendly", UnitAbility::HealOutsideFriendly),
        ("great_general", UnitAbility::Module(ClassicUnitAbility::GreatGeneral)),
        ("heal_on_kill_not_barbarians", UnitAbility::Module(ClassicUnitAbility::HealOnKillNotBarbarians)),
    ];
    let mut flags = serde_json::Map::new();
    for (k, a) in abilities {
        flags.insert(k.to_owned(), json!(mods.has(a)));
    }
    let stats = [
        ("range", UnitStat::Range),
        ("withdraw", UnitStat::WithdrawChance),
        ("heal_on_kill", UnitStat::HealOnKill),
        ("wounded_mod", UnitStat::WoundedDamagePercent),
        ("attacks_per_turn", UnitStat::AttacksPerTurn),
        ("xp_percent", UnitStat::ExperiencePercent),
        ("adjacent_damage", UnitStat::AdjacentDamageOnAttack),
        ("enemy_aura", UnitStat::EnemyAuraPercent),
        ("aura_range", UnitStat::AuraRange),
        ("heal_adjacent", UnitStat::HealAdjacent),
        ("heal_friendly", UnitStat::HealFriendlyLand),
        ("heal_neutral", UnitStat::HealNeutralLand),
        ("heal_enemy", UnitStat::HealEnemyLand),
        ("sight", UnitStat::Sight),
        ("embarked_sight", UnitStat::EmbarkedSight),
        ("stacked_general", UnitStat::Module(ClassicUnitStat::StackedGeneralPercent)),
        ("general_aura", UnitStat::Module(ClassicUnitStat::GeneralAuraPercent)),
        ("gg_points_percent", UnitStat::Module(ClassicUnitStat::GreatGeneralPointsPercent)),
        ("adjacent_damage_chance", UnitStat::Module(ClassicUnitStat::AdjacentDamageChance)),
        ("capital_falloff", UnitStat::CapitalDefenseFalloff),
    ];
    let mut st = serde_json::Map::new();
    for (k, s) in stats {
        st.insert(k.to_owned(), json!(mods.stat(s)));
    }
    let situational_stats = mods.situational_stats.len();
    // The step-cost table: per window tile and direction, the cost code of the unit's own price of the
    // step (points; 255 all moves; -1 no step; -2 outside the window).
    let prof = MoveProfile::of_unit(auth, rules, &core);
    let mut cost: Vec<i32> = Vec::with_capacity((win.w * win.w * 6) as usize);
    for t in win.tiles() {
        for d in 0..6 {
            let code = match neighbour(win, t, d) {
                Some(n) if win.index(n).is_some() => match step_cost_for(auth, rules, core.owner, &prof, t, n) {
                    Some(StepCost::Points(p)) => p as i32,
                    Some(StepCost::AllMoves) => 255,
                    None => -1,
                },
                _ => -2,
            };
            cost.push(code);
        }
    }
    let sight = classic_rules::derived::visibility::UnitSight::of(rules, &mods, core.kind);
    let sight_land = sight.at(auth.map(), TileId(u32::MAX)).max(0);
    json!({
        "kind": def.name, "kind_id": core.kind.0, "owner": core.owner.0,
        "domain": format!("{:?}", def.domain), "role": def.role.map(|r| format!("{r:?}")), "class": def.class,
        "strength": def.strength, "ranged_strength": def.ranged_strength, "range_base": def.range,
        "base_combat": fighter.base_combat(), "base_ranged": fighter.base_ranged(), "range": fighter.range(),
        "is_ranged": fighter.is_ranged(), "civilian": rules.is_civilian(core.kind),
        "ranged_only_in_domain": def.combat.ranged_only_in_domain, "suicide": def.combat.suicide,
        "capture_kind": def.combat.capture.map(|k| k.0),
        "capture_moves": def.combat.capture.and_then(|k| rules.units.get(k.0 as usize)).map(|d| i32::from(d.movement) * i32::from(kernel::world::unit::MOVE_DENOMINATOR)),
        "founder": def.founder,
        "moves_land": mods.movement(rules, core.kind), "moves_embarked": mods.embarked_movement(),
        "embarks": prof.embarks(), "embarked_defense": embarked_defense(auth, rules, &fighter),
        "sight_base": sight_land, "ignore_zoc": prof.ignore_zone_of_control,
        "contexts": contexts, "terms": terms, "flags": flags, "stats": st,
        "situational_stats": situational_stats, "post_combat": mods.post_combat.len(),
        "terrain_rules": mods.terrain_rules.len(),
        "cost": cost,
    })
}

fn stance_code(s: Stance) -> i32 {
    match s {
        Stance::Ready => 0,
        Stance::Fortified { turns } => i32::from(turns),
        Stance::SetUp => 3,
        Stance::Intercept => 4,
    }
}

/// The play's slot table: entity id -> slot.
struct Slots {
    ids: Vec<EntityId>,
}

impl Slots {
    fn slot_of(&mut self, id: EntityId) -> Option<usize> {
        if let Some(i) = self.ids.iter().position(|x| *x == id) {
            return Some(i);
        }
        if self.ids.len() >= SLOTS {
            return None;
        }
        self.ids.push(id);
        Some(self.ids.len() - 1)
    }
}

fn state(rules: &CompiledRuleset, game: &GameState, win: &Window, slots: &mut Slots, sides: [PlayerId; 2], cities: &[TileId]) -> Value {
    let auth = game.view().auth;
    // New units of the two seats get slots in id order (captured civilians).
    let mut new: Vec<(EntityId, UnitHandle)> = auth.units().iter_handles().filter(|(_, c)| sides.contains(&c.owner) && !slots.ids.contains(&c.id)).map(|(h, c)| (c.id, h)).collect();
    new.sort_by_key(|(id, _)| *id);
    for (id, _) in &new {
        slots.slot_of(*id);
    }
    let by_id: BTreeMap<EntityId, UnitHandle> = auth.units().iter_handles().map(|(h, c)| (c.id, h)).collect();
    let units: Vec<Value> = slots
        .ids
        .iter()
        .map(|id| match by_id.get(id).and_then(|h| auth.units().get(*h)) {
            Some(u) => {
                let c = u.core;
                json!([1, win.index(c.tile).unwrap_or(-1), c.hp, c.moves_left, c.moves_max, c.attacks_made, stance_code(c.stance), c.experience, sides.iter().position(|s| *s == c.owner).map_or(-1, |p| p as i32), c.kind.0])
            }
            None => json!([0, -1, 0, 0, 0, 0, 0, 0, -1, -1]),
        })
        .collect();
    let cs: Vec<Value> = cities
        .iter()
        .map(|t| match auth.indexes().city_on_tile(*t).and_then(|c| auth.cities().get(c)) {
            Some(c) => json!([sides.iter().position(|s| *s == c.owner).map_or(-1, |p| p as i32), c.damage, c.strike_made, c.population]),
            None => json!([-1, 0, false, 0]),
        })
        .collect();
    let vis = &game.view().derived.visibility;
    let bits: Vec<String> = sides
        .iter()
        .map(|p| {
            let mut s = String::with_capacity((win.w * win.w) as usize);
            for t in win.tiles() {
                let i = p.0 as usize * vis.tiles + t.0 as usize;
                s.push(if vis.counts.get(i).is_some_and(|c| *c > 0) { '1' } else { '0' });
            }
            s
        })
        .collect();
    let owners: String = win.tiles().map(|t| match auth.map().owner(t).flatten() {
        Some(o) if o == sides[0] => '0',
        Some(o) if o == sides[1] => '1',
        Some(_) => '2',
        None => '.',
    }).collect();
    let _ = rules;
    json!({"units": units, "cities": cs, "rng": auth.rng().combat.state().to_string(), "vis": bits, "owner": owners})
}

/// The options of one unit action, in a fixed order: fortify, set up, found, moves by tile, attacks by tile.
fn options(rules: &CompiledRuleset, game: &GameState, win: &Window, side: PlayerId, h: UnitHandle) -> Vec<(String, i32, Command)> {
    let mut v = Vec::new();
    let Some(me) = act::core_of(game, h) else { return v };
    let mut push = |v: &mut Vec<(String, i32, Command)>, k: &str, t: i32, c: Command| {
        if game.accepts_order(rules, side, &c) {
            v.push((k.to_owned(), t, c));
        }
    };
    push(&mut v, "F", -1, Command::Fortify { unit: h });
    push(&mut v, "U", -1, Command::SetUpRangedAttack { unit: h });
    push(&mut v, "C", -1, Command::FoundCity { settler: h });
    for (t, _) in act::reachable(game, rules, side, h) {
        if t != me.tile && win.inner(t) && act::free_for(game, rules, h, t) {
            push(&mut v, "M", win.index(t).unwrap(), Command::MoveUnit { unit: h, destination: t });
        }
    }
    let auth = game.view().auth;
    let reach = act::range_of(rules, me.kind).unwrap_or(1);
    let dims = auth.map().dimensions;
    let mut targets: Vec<TileId> = classic_rules::math::hex_disc(dims, me.tile, reach).filter(|t| *t != me.tile && win.index(*t).is_some()).collect();
    targets.sort_by_key(|t| t.0);
    for t in targets {
        let foreign = auth.indexes().units_on(t).iter().filter_map(|u| auth.units().get(*u)).any(|u| u.core.owner != side)
            || auth.indexes().city_on_tile(t).and_then(|c| auth.cities().get(c)).is_some_and(|c| c.owner != side);
        if !foreign {
            continue;
        }
        if let Some(cmd) = act::attack_cmd(game, rules, h, t) {
            push(&mut v, "A", win.index(t).unwrap(), cmd);
        }
    }
    v
}

fn city_options(rules: &CompiledRuleset, game: &GameState, win: &Window, side: PlayerId, city_tile: TileId) -> Vec<(String, i32, Command)> {
    let auth = game.view().auth;
    let Some(city) = auth.indexes().city_on_tile(city_tile) else { return Vec::new() };
    if auth.cities().get(city).is_none_or(|c| c.owner != side) {
        return Vec::new();
    }
    let dims = auth.map().dimensions;
    let mut ts: Vec<TileId> = classic_rules::math::hex_disc(dims, city_tile, classic_rules::combat::city::CITY_ATTACK_RANGE).filter(|t| *t != city_tile && win.index(*t).is_some()).collect();
    ts.sort_by_key(|t| t.0);
    ts.into_iter()
        .filter_map(|t| {
            let c = Command::CityStrike { city, target: t };
            game.accepts_order(rules, side, &c).then(|| ("S".to_owned(), win.index(t).unwrap(), c))
        })
        .collect()
}

fn scene_json(rules: &CompiledRuleset, game: &GameState, sc: &Scene, win: &Window, cities: &[TileId], slots: &mut Slots) -> Value {
    let auth = game.view().auth;
    let map = auth.map();
    let sides = [sc.learner, sc.enemy];
    let mut tiles = serde_json::Map::new();
    let tl: Vec<TileId> = win.tiles().collect();
    let col = |f: &dyn Fn(TileId) -> Value| -> Value { Value::Array(tl.iter().map(|t| f(*t)).collect()) };
    let hills = |t: TileId| classic_rules::navigation::terrain::is_hills(map, t);
    let mountain = |t: TileId| classic_rules::navigation::terrain::is_mountain(map, t);
    tiles.insert("terrain".into(), col(&|t| json!(map.terrain(t).map_or(-1, |x| i32::from(x.0)))));
    tiles.insert("terrain_kind".into(), col(&|t| json!(classic_rules::navigation::terrain::terrain_kind(rules, map, t).map_or(-1, |k| i32::from(k.0)))));
    tiles.insert("hills".into(), col(&|t| json!(hills(t))));
    tiles.insert("mountain".into(), col(&|t| json!(mountain(t))));
    tiles.insert("water".into(), col(&|t| json!(is_water(game, t))));
    tiles.insert("deep".into(), col(&|t| json!(deep(game, t))));
    tiles.insert("feature".into(), col(&|t| json!(map.feature(t).flatten().map_or(-1, |f| i32::from(f.0)))));
    tiles.insert("rough".into(), col(&|t| json!(classic_rules::navigation::terrain::is_rough(rules, map, t))));
    tiles.insert("road".into(), col(&|t| json!(map.road(t))));
    tiles.insert("ground_def".into(), col(&|t| json!(classic_rules::combat::terrain::ground_defense_percent(rules, hills(t) || mountain(t), map.feature(t).flatten()))));
    tiles.insert("improvement".into(), col(&|t| json!(map.working_improvement(t).map_or(-1, |i| i32::from(i.0)))));
    tiles.insert("improvement_def".into(), col(&|t| json!(map.working_improvement(t).and_then(|i| rules.improvements.get(i.0 as usize)).map_or(0, |d| i64::from(d.defense_pct)))));
    tiles.insert("sight_level".into(), col(&|t| {
        let base = if hills(t) { 1 } else if mountain(t) { 3 } else { 0 };
        let hides = map.feature(t).flatten().and_then(|f| rules.features.get(f.0 as usize)).is_some_and(|d| d.see_through > 0);
        json!(base + i32::from(hides))
    }));
    let rivers: Vec<Value> = tl.iter().map(|t| json!((0..6).map(|d| neighbour(win, *t, d).is_some_and(|n| map.river_between(*t, n))).collect::<Vec<bool>>())).collect();
    tiles.insert("river".into(), Value::Array(rivers));
    // Units: pieces in order (mine, then theirs).
    let pieces: Vec<UnitHandle> = sc.mine.iter().chain(&sc.theirs).map(|p| p.h).collect();
    for p in sc.mine.iter().chain(&sc.theirs) {
        slots.slot_of(p.id);
    }
    let profiles: Vec<Value> = pieces.iter().map(|h| profile(rules, game, win, *h)).collect();
    let city_list: Vec<Value> = cities
        .iter()
        .map(|t| {
            let c = auth.indexes().city_on_tile(*t).and_then(|c| auth.cities().get(c)).unwrap();
            let mods = classic_rules::combat::city::city_combat_mods(auth, rules, c);
            let base = classic_rules::combat::city::CITY_STRENGTH_BASE + i64::from(c.population) * classic_rules::combat::city::CITY_STRENGTH_PER_POP + mods.modified_defense() + classic_rules::combat::city::tech_strength(auth, rules, c.owner) + if hills(*t) { classic_rules::combat::city::CITY_HILL_STRENGTH } else { 0 };
            let draws = c.buildings.iter().filter(|b| rules.buildings.get(b.0 as usize).is_some_and(|d| !matches!(d.kind, kernel::rules::content::BuildingKind::Palace | kernel::rules::content::BuildingKind::WorldWonder) && !d.never_capture && d.conquest_survival_pct < 100)).count();
            json!({
                "tile": win.index(*t).unwrap_or(-1), "owner": sides.iter().position(|s| *s == c.owner).map_or(-1, |p| p as i32),
                "max_hp": classic_rules::combat::city::max_hit_points(auth, rules, c), "damage": c.damage,
                "population": c.population, "strength_no_garrison": base, "strength": classic_rules::combat::city::city_strength(auth, rules, c),
                "ranged_strength": classic_rules::combat::city::city_ranged_strength(auth, rules, c),
                "garrison_strike_percent": mods.garrison_strike_percent, "ranged_strike_percent": mods.ranged_strike_percent,
                "capture_draws": draws, "original_owner": sides.iter().position(|s| *s == c.original_owner).map_or(-1, |p| p as i32),
                "strike_made": c.strike_made,
            })
        })
        .collect();
    // Where a founder of each side could found a city (the site rules, static in a scene): a settler of
    // the piece's kind spawned on each window tile of a clone, the order asked.
    let mut found_ok: Vec<String> = Vec::new();
    for (si, side) in sides.iter().enumerate() {
        let founder = sc.mine.iter().chain(&sc.theirs).find(|p| rules.units[p.kind.0 as usize].founder && auth.units().get(p.h).is_some_and(|u| u.core.owner == *side));
        let Some(f) = founder else {
            found_ok.push(String::new());
            continue;
        };
        // The clone without the scene's units (the site rules do not read units).
        let mut g = game.clone();
        let units: Vec<UnitHandle> = g.view().auth.units().iter_handles().filter(|(_, c)| sides.contains(&c.owner)).map(|(h, _)| h).collect();
        for h in units {
            g.debug_remove_unit(rules, h);
        }
        let mut bits = String::new();
        for t in win.tiles() {
            let ok = classic_rules::navigation::cost::enter_cost_on_map(g.view().auth.map(), t).is_some() && {
                let h = g.debug_spawn_unit(rules, *side, t, f.kind);
                let ok = g.accepts_order(rules, *side, &Command::FoundCity { settler: h });
                g.debug_remove_unit(rules, h);
                ok
            };
            bits.push(if ok { '1' } else { '0' });
        }
        let _ = si;
        found_ok.push(bits);
    }
    // Cities of each side on the whole map (a side with none left is eliminated: its units go).
    let city_counts: Vec<usize> = sides.iter().map(|p| auth.indexes().cities_of(*p).len()).collect();
    json!({
        "found_ok": found_ok,
        "city_counts": city_counts,
        "family": sc.fam.name(), "era": lab::era_names(rules).get(sc.era as usize), "era_index": sc.era, "instance": sc.instance, "attempt": sc.attempt,
        "window": {"q0": win.q0, "r0": win.r0, "w": win.w, "map_w": win.map_w, "map_h": win.map_h},
        "tiles": tiles, "profiles": profiles, "cities": city_list,
        "n_mine": sc.mine.len(), "n_theirs": sc.theirs.len(),
        "move_denominator": kernel::world::unit::MOVE_DENOMINATOR,
    })
}

/// One dumped play of a scene.
pub fn dump_one(rules: &CompiledRuleset, fam: Fam, era: u8, instance: usize, attempt: u32, rounds: u32, seed: u64, aggressive: bool, record: bool) -> Result<Value, String> {
    let (mut game, sc) = families::build(rules, fam, era, instance, attempt)?;
    // The whole map known to both sides, as the trainers play it.
    let all: Vec<TileId> = (0..game.view().auth.map().tile_count() as u32).map(TileId).collect();
    game.debug_reveal(sc.learner, &all);
    game.debug_reveal(sc.enemy, &all);
    let win = Window::around(&game, sc.centre);
    let sides = [sc.learner, sc.enemy];
    let mut cities: Vec<TileId> = game.view().auth.cities().iter().map(|(_, c)| c.tile).filter(|t| win.index(*t).is_some()).collect();
    cities.sort_by_key(|t| t.0);
    let mut slots = Slots { ids: Vec::new() };
    let scene = if record { scene_json(rules, &game, &sc, &win, &cities, &mut slots) } else {
        for p in sc.mine.iter().chain(&sc.theirs) {
            slots.slot_of(p.id);
        }
        Value::Null
    };
    let initial = if record { state(rules, &game, &win, &mut slots, sides, &cities) } else { Value::Null };
    let play_started = std::time::Instant::now();
    let mut actions = 0u64;
    let mut rounds_played = 0u32;
    let d = Draw::new(lab::mix(seed ^ families::seed(fam, era, instance, attempt)));
    let mut steps: Vec<Value> = Vec::new();
    let piece_ids: Vec<EntityId> = sc.mine.iter().chain(&sc.theirs).map(|p| p.id).collect();
    let owners0: Vec<i32> = cities.iter().map(|t| game.view().auth.indexes().city_on_tile(*t).and_then(|c| game.view().auth.cities().get(c)).map_or(-1, |c| c.owner.0 as i32)).collect();
    let mut stop = String::new();
    let n_cities0 = game.view().auth.cities().iter().count();
    'play: for round in 0..rounds {
        for _ in 0..2 {
            if game.is_over() {
                stop = "game over".into();
                break 'play;
            }
            let side = game.view().auth.active_player();
            let Some(si) = sides.iter().position(|s| *s == side) else {
                // Another player's turn (none in the lab): end it.
                let _ = game.execute(rules, Command::EndTurn { player: side });
                continue;
            };
            // Scene units of this side, slot order.
            let mine: Vec<(usize, EntityId)> = slots.ids.iter().enumerate().filter(|(_, id)| piece_ids.contains(id)).map(|(i, id)| (i, *id)).collect();
            for (slot, id) in mine {
                for _ in 0..2 {
                    let auth = game.view().auth;
                    let Some((h, c)) = auth.units().iter_handles().find(|(_, c)| c.id == id).map(|(h, c)| (h, *c)) else { break };
                    if c.owner != side {
                        break;
                    }
                    let before_vis = if record { state(rules, &game, &win, &mut slots, sides, &cities)["vis"].clone() } else { Value::Null };
                    let opts = options(rules, &game, &win, side, h);
                    let pick = if aggressive { aggressive_pick(&game, &win, &opts, side, &d) } else { (d.next() % (opts.len() as u64 + 1)) as usize };
                    let listed: Vec<Value> = opts.iter().map(|(k, t, _)| json!([k, t])).collect();
                    let mut ok = true;
                    if pick < opts.len() {
                        ok = game.execute(rules, opts[pick].2.clone()).is_ok();
                    }
                    actions += 1;
                    if record {
                        let after = state(rules, &game, &win, &mut slots, sides, &cities);
                        steps.push(json!({"round": round, "side": si, "unit": slot, "options": listed, "pick": pick, "ok": ok, "vis_before": before_vis, "after": after}));
                    }
                    if pick >= opts.len() {
                        break;
                    }
                    let owners: Vec<i32> = cities.iter().map(|t| game.view().auth.indexes().city_on_tile(*t).and_then(|c| game.view().auth.cities().get(c)).map_or(-1, |c| c.owner.0 as i32)).collect();
                    if owners != owners0 {
                        stop = "city taken".into();
                        break 'play;
                    }
                    let n_cities = game.view().auth.cities().iter().count();
                    if n_cities != n_cities0 {
                        stop = "city founded".into();
                        break 'play;
                    }
                }
            }
            for (ci, t) in cities.iter().enumerate() {
                let opts = city_options(rules, &game, &win, side, *t);
                if game.view().auth.indexes().city_on_tile(*t).and_then(|c| game.view().auth.cities().get(c)).is_none_or(|c| c.owner != side) {
                    continue;
                }
                let before_vis = if record { state(rules, &game, &win, &mut slots, sides, &cities)["vis"].clone() } else { Value::Null };
                let pick = if aggressive && !opts.is_empty() { (d.next() % opts.len() as u64) as usize } else { (d.next() % (opts.len() as u64 + 1)) as usize };
                let listed: Vec<Value> = opts.iter().map(|(k, t, _)| json!([k, t])).collect();
                let mut ok = true;
                if pick < opts.len() {
                    ok = game.execute(rules, opts[pick].2.clone()).is_ok();
                }
                if record {
                    let after = state(rules, &game, &win, &mut slots, sides, &cities);
                    steps.push(json!({"round": round, "side": si, "city": ci, "options": listed, "pick": pick, "ok": ok, "vis_before": before_vis, "after": after}));
                }
            }
            let ended = game.execute(rules, Command::EndTurn { player: side }).is_ok();
            if record {
                let after = state(rules, &game, &win, &mut slots, sides, &cities);
                steps.push(json!({"round": round, "side": si, "end_turn": ended, "after": after}));
            }
            if !ended {
                stop = "end turn refused".into();
                break 'play;
            }
        }
        // The closed arena.
        let gone: Vec<UnitHandle> = game.view().auth.units().iter_handles().filter(|(_, c)| sides.contains(&c.owner) && !piece_ids.contains(&c.id)).map(|(h, _)| h).collect();
        let removed = gone.len();
        for h in gone {
            game.debug_remove_unit(rules, h);
        }
        if record {
            let after = state(rules, &game, &win, &mut slots, sides, &cities);
            steps.push(json!({"round": round, "round_end": removed, "after": after}));
        }
        rounds_played += 1;
        let alive = |s: PlayerId| game.view().auth.units().iter_handles().any(|(_, c)| c.owner == s && piece_ids.contains(&c.id));
        if !alive(sides[0]) || !alive(sides[1]) {
            stop = "a side has no unit".into();
            break;
        }
    }
    Ok(json!({"scene": scene, "initial": initial, "steps": steps, "stop": stop, "seed": seed, "slots": slots.ids.len(),
        "play_s": play_started.elapsed().as_secs_f64(), "actions": actions, "rounds_played": rounds_played}))
}

/// The aggressive script: an attack when one is offered (9 in 10), else (8 in 10) the move that comes
/// nearest to an enemy unit or city (ties by the draw), else the uniform draw.
fn aggressive_pick(game: &GameState, win: &Window, opts: &[(String, i32, Command)], side: PlayerId, d: &Draw) -> usize {
    let n = opts.len();
    let roll = d.next() % 10;
    let attacks: Vec<usize> = (0..n).filter(|i| opts[*i].0 == "A").collect();
    if !attacks.is_empty() && roll < 9 {
        return attacks[(d.next() % attacks.len() as u64) as usize];
    }
    let auth = game.view().auth;
    let mut foes: Vec<TileId> = auth.units().iter_handles().filter(|(_, c)| c.owner != side).map(|(_, c)| c.tile).collect();
    foes.extend(auth.cities().iter().filter(|(_, c)| c.owner != side).map(|(_, c)| c.tile));
    let dims = auth.map().dimensions;
    let near = |t: TileId| foes.iter().filter_map(|f| dims.distance(*f, t)).min().unwrap_or(99);
    let moves: Vec<usize> = (0..n).filter(|i| opts[*i].0 == "M").collect();
    if !moves.is_empty() && roll < 8 {
        let best = moves.iter().map(|i| near(win.tile(opts[*i].1))).min().unwrap();
        let tied: Vec<usize> = moves.into_iter().filter(|i| near(win.tile(opts[*i].1)) == best).collect();
        return tied[(d.next() % tied.len() as u64) as usize];
    }
    (d.next() % (n as u64 + 1)) as usize
}

/// `ai-exam suite-dump --kept FILE --families duel,siege --per-cell 4 --rounds 10 --seed 1 --out FILE.jsonl --jobs 4`, or one
/// scene: `--family duel --era medieval --instance 3 --attempt 0`.
pub fn main(rules: &CompiledRuleset, args: &[String]) -> Result<(), String> {
    let mut kept: Option<String> = None;
    let mut families_filter: Option<Vec<String>> = None;
    let mut per_cell = 4usize;
    let mut rounds = 10u32;
    let mut seed = 1u64;
    let mut out = "dump.jsonl".to_owned();
    let mut jobs = 1usize;
    let mut aggressive = false;
    let mut record = true;
    let mut one: Option<(Fam, String, usize, u32)> = None;
    let (mut fam, mut era, mut inst, mut att) = (None, None, 0usize, 0u32);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut v = || it.next().cloned().ok_or(format!("{a} needs a value"));
        match a.as_str() {
            "--kept" => kept = Some(v()?),
            "--families" => families_filter = Some(v()?.split(',').map(str::to_owned).collect()),
            "--per-cell" => per_cell = v()?.parse().map_err(|e| format!("{e}"))?,
            "--rounds" => rounds = v()?.parse().map_err(|e| format!("{e}"))?,
            "--seed" => seed = v()?.parse().map_err(|e| format!("{e}"))?,
            "--out" => out = v()?,
            "--jobs" => jobs = v()?.parse().map_err(|e| format!("{e}"))?,
            "--policy" => aggressive = v()? == "aggressive",
            "--no-record" => record = false,
            "--family" => fam = Some(Fam::parse(&v()?)?),
            "--era" => era = Some(v()?),
            "--instance" => inst = v()?.parse().map_err(|e| format!("{e}"))?,
            "--attempt" => att = v()?.parse().map_err(|e| format!("{e}"))?,
            other => return Err(format!("unknown option {other}")),
        }
    }
    if let (Some(f), Some(e)) = (fam, era) {
        one = Some((f, e, inst, att));
    }
    let mut cells: Vec<(Fam, u8, usize, u32)> = Vec::new();
    if let Some((f, e, i, a)) = one {
        cells.push((f, lab::era_index(rules, &e).ok_or("unknown era")?, i, a));
    } else {
        let text = std::fs::read_to_string(kept.as_deref().ok_or("--kept or --family/--era")?).map_err(|e| e.to_string())?;
        let mut per: BTreeMap<(String, String), usize> = BTreeMap::new();
        for l in text.lines() {
            let k: super::suite::Kept = serde_json::from_str(l).map_err(|e| e.to_string())?;
            let Some(a) = k.attempt else { continue };
            if families_filter.as_ref().is_some_and(|f| !f.contains(&k.family)) {
                continue;
            }
            let n = per.entry((k.family.clone(), k.era.clone())).or_default();
            if *n >= per_cell {
                continue;
            }
            *n += 1;
            cells.push((Fam::parse(&k.family)?, lab::era_index(rules, &k.era).ok_or("era")?, k.instance, a));
        }
    }
    let file = std::sync::Mutex::new(std::fs::File::create(&out).map_err(|e| e.to_string())?);
    let started = std::time::Instant::now();
    let totals = std::sync::Mutex::new((0f64, 0u64, 0u64));
    let ix: Vec<usize> = (0..cells.len()).collect();
    crate::export::parallel(&ix, jobs, |&i| {
        let (f, e, inst, a) = cells[i];
        match dump_one(rules, f, e, inst, a, rounds, seed, aggressive, record) {
            Ok(v) => {
                let mut t = totals.lock().unwrap();
                t.0 += v["play_s"].as_f64().unwrap_or(0.0);
                t.1 += v["actions"].as_u64().unwrap_or(0);
                t.2 += v["rounds_played"].as_u64().unwrap_or(0);
                if record {
                    let _ = writeln!(file.lock().unwrap(), "{v}");
                }
            }
            Err(err) => eprintln!("{} {e} {inst}.{a}: {err}", f.name()),
        }
    });
    let t = totals.lock().unwrap();
    eprintln!("{} scenes in {:.1} s; play {:.3} s on {jobs} threads (summed), {} unit actions, {} scene-rounds: {:.1} scene-rounds/s per thread", cells.len(), started.elapsed().as_secs_f64(), t.0, t.1, t.2, t.2 as f64 / t.0.max(1e-9));
    Ok(())
}
