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
use classic_rules::ids::{PlayerId, PromotionId, TileId, UnitHandle};
use classic_rules::navigation::cost::{step_cost_for, StepCost};
use classic_rules::navigation::MoveProfile;
use classic_rules::rules::effects::{CombatContext, Condition, UnitAbility, UnitStat};
use classic_rules::classic::effects::{ClassicUnitAbility, ClassicUnitStat};
use classic_rules::{ClassicGame, Command, CompiledRuleset, GameState};
use kernel::ids::EntityId;
use kernel::world::unit::{Stance, UnitCore};
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
    /// Rows (the window is `w` x `h`; the whole map with `--whole-map`).
    pub h: i32,
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
        Window { q0, r0, w: WINDOW.min(mw), h: WINDOW.min(mh), map_w: mw, map_h: mh }
    }
    /// The whole map as the window.
    fn whole(game: &GameState) -> Window {
        let dims = game.view().auth.map().dimensions;
        let (mw, mh) = (dims.width as i32, dims.height as i32);
        Window { q0: 0, r0: 0, w: mw, h: mh, map_w: mw, map_h: mh }
    }
    /// Window index of a map tile (None outside).
    pub fn index(&self, t: TileId) -> Option<i32> {
        let (q, r) = (t.0 as i32 % self.map_w, t.0 as i32 / self.map_w);
        let (x, y) = (q - self.q0, r - self.r0);
        (x >= 0 && y >= 0 && x < self.w && y < self.h).then_some(y * self.w + x)
    }
    pub fn tile(&self, i: i32) -> TileId {
        let (x, y) = (i % self.w, i / self.w);
        TileId(((self.r0 + y) * self.map_w + self.q0 + x) as u32)
    }
    /// Inside the window, INNER tiles off its border.
    fn inner(&self, t: TileId) -> bool {
        self.index(t).is_some_and(|i| {
            let (x, y) = (i % self.w, i / self.w);
            x >= INNER && y >= INNER && x < self.w - INNER && y < self.h - INNER
        })
    }
    fn tiles(&self) -> impl Iterator<Item = TileId> + '_ {
        (0..self.w * self.h).map(|i| self.tile(i))
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
    let core = *game.view().auth.units().get(h).unwrap().core;
    profile_of(rules, game, win, h, core, true)
}

/// The profile of `core` (the unit `h` as it would stand: another promotion set), with or without
/// its per-tile tables (step costs, where it may stop, where it may withdraw).
fn profile_of(rules: &CompiledRuleset, game: &GameState, win: &Window, h: UnitHandle, core: UnitCore, tables: bool) -> Value {
    let auth = game.view().auth;
    let mods = unit_modifiers(auth, rules, &core);
    let def = &rules.units[core.kind.0 as usize];
    let fighter = Fighter { handle: h, core, def: rules.units.get(core.kind.0 as usize), mods: mods.clone() };
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
        ("air_sweep", UnitAbility::AirSweep),
        ("cross_mountains", UnitAbility::CrossMountains),
        ("hovering", UnitAbility::Hovering),
        ("insta_heal", UnitAbility::InstaHeal),
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
        ("intercept_chance", UnitStat::InterceptChance),
        ("interceptions_per_turn", UnitStat::InterceptionsPerTurn),
        ("evasion", UnitStat::EvasionPercent),
        ("interception_taken", UnitStat::InterceptionDamageTakenPercent),
        ("xp_needed_pct", UnitStat::Module(ClassicUnitStat::ExperienceNeededPercent)),
    ];
    let mut st = serde_json::Map::new();
    for (k, s) in stats {
        st.insert(k.to_owned(), json!(mods.stat(s)));
    }
    let situational_stats = mods.situational_stats.len();
    let prof = MoveProfile::of_unit(auth, rules, &core);
    let sight = classic_rules::derived::visibility::UnitSight::of(rules, &mods, core.kind);
    let sight_land = sight.at(auth.map(), TileId(u32::MAX)).max(0);
    let mut v = json!({
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
        "sight_base": sight_land, "sight_embarked": (1 + mods.stat(UnitStat::EmbarkedSight)).clamp(1, 7),
        "ignore_zoc": prof.ignore_zone_of_control,
        "air_intercept_range": def.combat.air_intercept_range,
        "contexts": contexts, "terms": terms, "flags": flags, "stats": st,
        "situational_stats": situational_stats, "post_combat": mods.post_combat.len(),
        "terrain_rules": mods.terrain_rules.len(),
    });
    if tables {
        // The step-cost table: per window tile and direction, the cost code of the unit's own price of
        // the step (points; 255 all moves; -1 no step; -2 outside the window).
        let mut cost: Vec<i32> = Vec::with_capacity((win.w * win.h * 6) as usize);
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
        // Where the unit may stand (`destination_ok`), and where it may withdraw to (its element, the
        // destination rule and the borders; the units and cities there are the simulator's).
        let map = auth.map();
        let dest: String = win.tiles().map(|t| if classic_rules::navigation::cost::destination_ok(auth, &prof, t) { '1' } else { '0' }).collect();
        let withdraw: String = win
            .tiles()
            .map(|t| {
                let element = match prof.domain {
                    kernel::rules::content::Domain::Sea => classic_rules::navigation::terrain::is_water(map, t),
                    _ => classic_rules::navigation::cost::enter_cost_on_map(map, t).is_some(),
                };
                let ok = element && classic_rules::navigation::cost::destination_ok(auth, &prof, t) && classic_rules::world::territory::may_enter(auth, core.owner, t);
                if ok { '1' } else { '0' }
            })
            .collect();
        v["cost"] = json!(cost);
        v["dest_ok"] = json!(dest);
        v["withdraw_ok"] = json!(withdraw);
    }
    v
}

/// What `new` changes against `base`: numbers as differences, other scalars as the new value, arrays
/// of equal length index by index (`[i, change]`), other arrays as the added and removed elements,
/// objects key by key. Null when nothing changes.
fn diff(base: &Value, new: &Value) -> Value {
    match (base, new) {
        (Value::Number(a), Value::Number(b)) => {
            let (a, b) = (a.as_i64().unwrap_or(0), b.as_i64().unwrap_or(0));
            if a == b { Value::Null } else { json!(b - a) }
        }
        (Value::Array(a), Value::Array(b)) if a.len() == b.len() && a.iter().all(|x| !x.is_object() && !x.is_array()) => {
            let ch: Vec<Value> = a.iter().zip(b).enumerate().filter_map(|(i, (x, y))| {
                let d = diff(x, y);
                (!d.is_null()).then(|| json!([i, d]))
            }).collect();
            if ch.is_empty() { Value::Null } else { Value::Array(ch) }
        }
        (Value::Array(a), Value::Array(b)) => {
            let mut left: Vec<&Value> = a.iter().collect();
            let mut added = Vec::new();
            for y in b {
                if let Some(i) = left.iter().position(|x| *x == y) {
                    left.remove(i);
                } else {
                    added.push(y.clone());
                }
            }
            if added.is_empty() && left.is_empty() { Value::Null } else { json!({"added": added, "removed": left}) }
        }
        (Value::Object(a), Value::Object(b)) => {
            let mut m = serde_json::Map::new();
            for (k, y) in b {
                let d = match a.get(k) { Some(x) => diff(x, y), None => y.clone() };
                if !d.is_null() {
                    m.insert(k.clone(), d);
                }
            }
            if m.is_empty() { Value::Null } else { Value::Object(m) }
        }
        (a, b) => if a == b { Value::Null } else { b.clone() },
    }
}

/// `base` with the change `d` (the inverse of `diff`).
fn apply(base: &Value, d: &Value) -> Value {
    if d.is_null() {
        return base.clone();
    }
    match (base, d) {
        (Value::Number(a), Value::Number(x)) => json!(a.as_i64().unwrap_or(0) + x.as_i64().unwrap_or(0)),
        (Value::Array(a), Value::Array(ch)) => {
            let mut out = a.clone();
            for c in ch {
                let i = c[0].as_u64().unwrap_or(0) as usize;
                if i < out.len() {
                    out[i] = apply(&out[i], &c[1]);
                }
            }
            Value::Array(out)
        }
        (Value::Array(a), Value::Object(o)) => {
            let mut out = a.clone();
            for r in o.get("removed").and_then(|r| r.as_array()).into_iter().flatten() {
                if let Some(i) = out.iter().position(|x| x == r) {
                    out.remove(i);
                }
            }
            out.extend(o.get("added").and_then(|r| r.as_array()).cloned().unwrap_or_default());
            Value::Array(out)
        }
        (Value::Object(a), Value::Object(o)) => {
            let mut out = a.clone();
            for (k, x) in o {
                let v = match a.get(k) { Some(y) => apply(y, x), None => x.clone() };
                out.insert(k.clone(), v);
            }
            Value::Object(out)
        }
        (_, x) => x.clone(),
    }
}

/// Equal as profiles: arrays of objects (the situational terms) compared as multisets.
fn same(a: &Value, b: &Value) -> bool {
    diff(a, b).is_null()
}

/// The promotions a unit may ever choose in the scene (choosable, listed for its kind, fitting, the
/// technology known): `available_promotions` without the held and prerequisite tests, which the
/// simulator makes.
fn choosable_for(rules: &CompiledRuleset, game: &GameState, core: &UnitCore) -> Vec<PromotionId> {
    let techs = game.view().auth.players().techs_of(core.owner);
    let mut out = Vec::new();
    for (i, def) in rules.promotions.iter().enumerate() {
        let id = PromotionId(i as u16);
        let listed = !def.unit_roles.is_empty() || def.civilian_units.contains(&core.kind);
        if !def.choosable || !listed || !classic_rules::effects::unit::promotion_fits(rules, id, core.kind) {
            continue;
        }
        if def.required_tech.is_some_and(|t| !techs.contains(&t)) {
            continue;
        }
        out.push(id);
    }
    out
}

/// The scene's promotion table: every promotion a piece holds or may choose, and their
/// prerequisites (global ids, in id order).
fn promotion_table(rules: &CompiledRuleset, game: &GameState, pieces: &[UnitHandle]) -> Vec<PromotionId> {
    let auth = game.view().auth;
    let mut set: std::collections::BTreeSet<u16> = std::collections::BTreeSet::new();
    for h in pieces {
        let core = *auth.units().get(*h).unwrap().core;
        for p in core.promotions.iter() {
            set.insert(p.0);
        }
        for p in choosable_for(rules, game, &core) {
            set.insert(p.0);
        }
    }
    loop {
        let more: Vec<u16> = set
            .iter()
            .flat_map(|p| rules.promotions[*p as usize].requires.iter().chain(&rules.promotions[*p as usize].requires_any_of).map(|q| q.0))
            .filter(|q| !set.contains(q))
            .collect();
        if more.is_empty() {
            break;
        }
        set.extend(more);
    }
    set.into_iter().map(PromotionId).collect()
}

/// The promotions of a piece in the simulator's form: the ones it may choose (local ids), the change
/// each makes to its profile alone, the changes of the pairs of those that touch its movement tables
/// (step costs, stopping, withdrawal: not additive), and the pairs whose changes do not add up.
fn promotion_part(rules: &CompiledRuleset, game: &GameState, win: &Window, h: UnitHandle, table: &[PromotionId], base: &Value) -> Value {
    let core = *game.view().auth.units().get(h).unwrap().core;
    let local = |p: PromotionId| table.iter().position(|q| *q == p).map_or(-1, |i| i as i32);
    let fit: Vec<PromotionId> = choosable_for(rules, game, &core);
    let with = |ps: &[PromotionId], tables: bool| {
        let mut c = core;
        for p in ps {
            c.promotions.insert(*p);
        }
        profile_of(rules, game, win, h, c, tables)
    };
    let mut deltas = serde_json::Map::new();
    let mut moving: Vec<PromotionId> = Vec::new();
    for p in &fit {
        let d = diff(base, &with(&[*p], true));
        if ["cost", "dest_ok", "withdraw_ok", "embarks", "ignore_zoc"].iter().any(|k| d.get(*k).is_some()) {
            moving.push(*p);
        }
        deltas.insert(local(*p).to_string(), d);
    }
    let mut pairs = Vec::new();
    for (i, p) in moving.iter().enumerate() {
        for q in &moving[i + 1..] {
            pairs.push(json!({"set": [local(*p), local(*q)], "delta": diff(base, &with(&[*p, *q], true))}));
        }
    }
    // The additivity guard (without the tables): the profile with two promotions is the base plus both
    // changes.
    let mut base_nt = base.clone();
    for k in ["cost", "dest_ok", "withdraw_ok"] {
        base_nt.as_object_mut().unwrap().remove(k);
    }
    let strip = |v: &Value| {
        let mut v = v.clone();
        for k in ["cost", "dest_ok", "withdraw_ok"] {
            if let Some(o) = v.as_object_mut() {
                o.remove(k);
            }
        }
        v
    };
    let mut nonadditive = Vec::new();
    for (i, p) in fit.iter().enumerate() {
        for q in &fit[i + 1..] {
            let dp = strip(&deltas[&local(*p).to_string()]);
            let dq = strip(&deltas[&local(*q).to_string()]);
            let want = with(&[*p, *q], false);
            if !same(&apply(&apply(&base_nt, &dp), &dq), &want) {
                nonadditive.push(json!([local(*p), local(*q)]));
            }
        }
    }
    json!({"fit": fit.iter().map(|p| local(*p)).collect::<Vec<i32>>(), "delta": deltas, "pairs": pairs, "nonadditive": nonadditive})
}

pub(crate) fn stance_code(s: Stance) -> i32 {
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
    /// The scene's promotion table (global ids; the state's promotion bits follow it).
    promos: Vec<PromotionId>,
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
    let promos = slots.promos.clone();
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
                let bits: String = promos.iter().map(|p| if c.promotions.contains(*p) { '1' } else { '0' }).collect();
                json!([1, win.index(c.tile).unwrap_or(-1), c.hp, c.moves_left, c.moves_max, c.attacks_made, stance_code(c.stance), c.experience, sides.iter().position(|s| *s == c.owner).map_or(-1, |p| p as i32), c.kind.0, c.level, bits, c.interceptions_made])
            }
            None => json!([0, -1, 0, 0, 0, 0, 0, 0, -1, -1, 0, "", 0]),
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
            let mut s = String::with_capacity((win.w * win.h) as usize);
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
fn options(rules: &CompiledRuleset, game: &GameState, win: &Window, side: PlayerId, h: UnitHandle, promos: &[PromotionId]) -> Vec<(String, i32, Command)> {
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
    for (i, p) in promos.iter().enumerate() {
        push(&mut v, "P", i as i32, Command::Promote { unit: h, promotion: *p });
    }
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
    // The citadel's damage to adjacent enemies (an intact improvement that has it).
    tiles.insert("citadel_dmg".into(), col(&|t| {
        let imp = map.improvement(t).flatten().filter(|_| !map.pillaged(t));
        json!(imp.and_then(|i| rules.improvements.get(i.0 as usize)).map_or(0, |d| d.nearby_enemy_damage))
    }));
    let rivers: Vec<Value> = tl.iter().map(|t| json!((0..6).map(|d| neighbour(win, *t, d).is_some_and(|n| map.river_between(*t, n))).collect::<Vec<bool>>())).collect();
    tiles.insert("river".into(), Value::Array(rivers));
    // Units: pieces in order (mine, then theirs).
    let pieces: Vec<UnitHandle> = sc.mine.iter().chain(&sc.theirs).map(|p| p.h).collect();
    for p in sc.mine.iter().chain(&sc.theirs) {
        slots.slot_of(p.id);
    }
    let mut profiles: Vec<Value> = pieces.iter().map(|h| profile(rules, game, win, *h)).collect();
    // Promotions: each piece's choices and the change of each to its profile.
    for (i, h) in pieces.iter().enumerate() {
        let part = promotion_part(rules, game, win, *h, &slots.promos, &profiles[i]);
        profiles[i]["promo"] = part;
    }
    let promotions: Vec<Value> = slots
        .promos
        .iter()
        .map(|p| {
            let def = &rules.promotions[p.0 as usize];
            let local = |q: &PromotionId| slots.promos.iter().position(|x| x == q).map_or(-1, |i| i as i32);
            json!({"id": p.0, "key": def.key, "name": def.name, "insta_heal": def.heals_instantly(),
                   "requires": def.requires.iter().map(local).collect::<Vec<i32>>(),
                   "requires_any": def.requires_any_of.iter().map(local).collect::<Vec<i32>>()})
        })
        .collect();
    // Units a capture brings: the defeated piece's kind as the captor's (CaptureDefeatedEnemy), and
    // what a captured civilian becomes (its capture kind as the captor's, closed over captures of
    // captured units). Profiles on a clone (the profile reads the owner, not the tile).
    let mut spawn: Vec<Value> = Vec::new();
    let mut spawn_ix: BTreeMap<(u16, u8), i32> = BTreeMap::new();
    let other_side = |o: PlayerId| if o == sides[0] { sides[1] } else { sides[0] };
    let mut make = |kind: classic_rules::ids::UnitKindId, owner: PlayerId, at: TileId, spawn: &mut Vec<Value>| -> i32 {
        if let Some(i) = spawn_ix.get(&(kind.0, owner.0)) {
            return *i;
        }
        let mut g = game.clone();
        let nh = g.debug_spawn_unit(rules, owner, at, kind);
        let core = *g.view().auth.units().get(nh).unwrap().core;
        let i = spawn.len() as i32;
        let mut v = profile_of(rules, &g, win, nh, core, true);
        v["prom_bits"] = json!(slots.promos.iter().map(|p| if core.promotions.contains(*p) { '1' } else { '0' }).collect::<String>());
        v["level"] = json!(core.level);
        spawn.push(v);
        spawn_ix.insert((kind.0, owner.0), i);
        i
    };
    let mut civ_spawn: Vec<i32> = Vec::new();
    for (i, h) in pieces.iter().enumerate() {
        let me = *auth.units().get(*h).unwrap().core;
        let captures = profiles[i]["flags"]["capture_defeated"].as_bool().unwrap_or(false);
        let mut row: Vec<i32> = Vec::new();
        for o in &pieces {
            let other = *auth.units().get(*o).unwrap().core;
            row.push(if captures && other.owner != me.owner { make(other.kind, me.owner, me.tile, &mut spawn) } else { -1 });
        }
        profiles[i]["capture_spawn"] = json!(row);
        let civ = rules.units[me.kind.0 as usize].combat.capture.map_or(-1, |k| make(k, other_side(me.owner), me.tile, &mut spawn));
        civ_spawn.push(civ);
    }
    for (i, c) in civ_spawn.into_iter().enumerate() {
        profiles[i]["civ_spawn"] = json!(c);
    }
    // Close: a spawned civilian may be captured again.
    let mut k = 0;
    while k < spawn.len() {
        let kind = spawn[k]["capture_kind"].as_u64();
        let owner = PlayerId(spawn[k]["owner"].as_u64().unwrap_or(0) as u8);
        let at = pieces.first().and_then(|h| auth.units().get(*h)).map_or(TileId(0), |u| u.core.tile);
        let c = kind.map_or(-1, |kd| make(classic_rules::ids::UnitKindId(kd as u16), other_side(owner), at, &mut spawn));
        spawn[k]["civ_spawn"] = json!(c);
        k += 1;
    }
    // The enemy's reinforcements (assault-reinforce) are spawned later in the play: the spawn row of each, for a simulator
    // that lands them (an info-only field; the rows come after the capture spawns, whose indices stay).
    let reinforce_at = pieces.first().and_then(|h| auth.units().get(*h)).map_or(TileId(0), |u| u.core.tile);
    let reinforce: Vec<i32> = sc.reinforce.iter().map(|k| make(*k, sc.enemy, reinforce_at, &mut spawn)).collect();
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
        "window": {"q0": win.q0, "r0": win.r0, "w": win.w, "h": win.h, "map_w": win.map_w, "map_h": win.map_h},
        "tiles": tiles, "profiles": profiles, "cities": city_list, "promotions": promotions, "spawn": spawn, "reinforce": reinforce,
        "n_mine": sc.mine.len(), "n_theirs": sc.theirs.len(),
        "move_denominator": kernel::world::unit::MOVE_DENOMINATOR,
    })
}

/// A play of `ai-exam suite` in the dump's form (`army_net::obsdump`, a MEASUREMENT): the scene line the MLX
/// simulator rebuilds and the state at any moment, with the same window, slots and city list as `dump_one`.
pub struct Track {
    win: Window,
    slots: Slots,
    cities: Vec<TileId>,
    sides: [PlayerId; 2],
    /// The scene's third seat (the camp and third-party families): its player and its pieces' ids, info-only (the
    /// twin's third side; the scene seats' slots and rows are the same as without it).
    third: Option<(PlayerId, Vec<EntityId>)>,
    /// The camp's tile (the camp families), whose countdown the third seat's rows carry.
    camp: Option<TileId>,
}

impl Track {
    /// The scene (`scene_json`) and the initial state of the play `sc` on `game` (the whole map known to both
    /// sides, as the trainers play it).
    pub fn start(rules: &CompiledRuleset, game: &GameState, sc: &Scene) -> (Track, Value, Value) {
        let win = Window::around(game, sc.centre);
        let mut cities: Vec<TileId> = game.view().auth.cities().iter().map(|(_, c)| c.tile).filter(|t| win.index(*t).is_some()).collect();
        cities.sort_by_key(|t| t.0);
        let pieces: Vec<UnitHandle> = sc.mine.iter().chain(&sc.theirs).chain(&sc.third).map(|p| p.h).collect();
        let mut slots = Slots { ids: Vec::new(), promos: promotion_table(rules, game, &pieces) };
        let mut scene = scene_json(rules, game, sc, &win, &cities, &mut slots);
        let sides = [sc.learner, sc.enemy];
        let initial = state(rules, game, &win, &mut slots, sides, &cities);
        let third = sc.neutral.map(|n| (n, sc.third.iter().map(|p| p.id).collect::<Vec<EntityId>>()));
        let camp = if super::camp::is_camp(sc.fam) { Some(super::camp::camp_tile(sc)) } else { None };
        let mut track = Track { win, slots, cities, sides, third, camp };
        scene["third"] = track.third_scene(rules, game, sc);
        let mut initial = initial;
        track.add_third(game, &mut initial);
        (track, scene, initial)
    }

    /// Info-only: the third seat's pieces (profiles with their promotions, in the order of `sc.third`), the camp's spawn
    /// kind. Null without a third seat.
    fn third_scene(&self, rules: &CompiledRuleset, game: &GameState, sc: &Scene) -> Value {
        if self.third.is_none() {
            return Value::Null;
        }
        let mut profiles: Vec<Value> = sc.third.iter().map(|p| profile(rules, game, &self.win, p.h)).collect();
        for (i, p) in sc.third.iter().enumerate() {
            let part = promotion_part(rules, game, &self.win, p.h, &self.slots.promos, &profiles[i]);
            profiles[i]["promo"] = part;
        }
        json!({"ids": sc.third.iter().map(|p| p.id.0).collect::<Vec<u64>>(), "profiles": profiles,
               "kinds": sc.third.iter().map(|p| p.kind.0).collect::<Vec<u16>>(),
               "camp": self.camp.map_or(-1, |t| self.win.index(t).map_or(-1, i64::from)),
               "spawn_kind": self.camp.and_then(|_| classic_rules::barbarian_unit(game.view().auth, rules, sc.enemy, classic_rules::rules::content::Domain::Land)).map_or(-1, |k| i64::from(k.0))})
    }

    /// Info-only: the third seat's unit rows (the dump's unit record, side 2), its cities (a city of the third seat in the
    /// window: its index), and the camp's countdown (-1: no camp) are added to a state record.
    fn add_third(&self, game: &GameState, st: &mut Value) {
        let Some((player, ids)) = &self.third else { return };
        let auth = game.view().auth;
        let by_id: BTreeMap<EntityId, UnitHandle> = auth.units().iter_handles().map(|(h, c)| (c.id, h)).collect();
        let rows: Vec<Value> = ids
            .iter()
            .map(|id| match by_id.get(id).and_then(|h| auth.units().get(*h)) {
                Some(u) => {
                    let c = u.core;
                    let bits: String = self.slots.promos.iter().map(|p| if c.promotions.contains(*p) { '1' } else { '0' }).collect();
                    json!([1, self.win.index(c.tile).unwrap_or(-1), c.hp, c.moves_left, c.moves_max, c.attacks_made, stance_code(c.stance), c.experience, 2, c.kind.0, c.level, bits, c.interceptions_made])
                }
                None => json!([0, -1, 0, 0, 0, 0, 0, 0, -1, -1, 0, "", 0]),
            })
            .collect();
        st["third"] = Value::Array(rows);
        let held: Vec<bool> = self.cities.iter().map(|t| auth.indexes().city_on_tile(*t).and_then(|c| auth.cities().get(c)).is_some_and(|c| c.owner == *player)).collect();
        st["third_cities"] = json!(held);
        let spawn_in = self.camp.and_then(|t| auth.module().barbarians().camps().iter().find(|c| c.tile == t).map(|c| i64::from(c.spawn_in)));
        st["camp"] = json!(spawn_in.unwrap_or(-1));
    }

    /// The state now (units by slot, cities, the RNG, both sides' sight, the owners).
    pub fn state(&mut self, rules: &CompiledRuleset, game: &GameState) -> Value {
        let mut st = state(rules, game, &self.win, &mut self.slots, self.sides, &self.cities);
        self.add_third(game, &mut st);
        st
    }

    /// The slot of a unit (new units get the next slot).
    pub fn slot_of(&mut self, id: EntityId) -> Option<usize> {
        self.slots.slot_of(id)
    }

    /// The slots' entity ids.
    pub fn ids(&self) -> &[EntityId] {
        &self.slots.ids
    }

    /// The window index of a map tile (-1 outside).
    pub fn index(&self, t: TileId) -> i32 {
        self.win.index(t).unwrap_or(-1)
    }

    /// The scene side of a player: 0 the learner, 1 the enemy, -1 another.
    pub fn side_of(&self, p: PlayerId) -> i64 {
        self.sides.iter().position(|s| *s == p).map_or(-1, |i| i as i64)
    }

    /// The window's cities (map tiles, in the scene's city order).
    pub fn cities(&self) -> &[TileId] {
        &self.cities
    }

    /// The window's origin and side, the map's width.
    pub fn window(&self) -> (i32, i32, i32, i32) {
        (self.win.q0, self.win.r0, self.win.w, self.win.map_w)
    }

    /// The scene's promotion table (global ids, in the state's bit order).
    pub fn promos(&self) -> &[PromotionId] {
        &self.slots.promos
    }
}

/// One dumped play of a scene.
/// How a dump is taken: the play's script, the variants, the window.
#[derive(Debug, Clone, Default)]
pub struct DumpOptions {
    pub aggressive: bool,
    pub record: bool,
    pub plant: Vec<String>,
    /// The whole map as the window (not 24 x 24 around the arena).
    pub whole_map: bool,
    /// Multi-turn path costs to this many goals per piece at the start (`find_path_cost`).
    pub paths: usize,
}

pub fn dump_one(rules: &CompiledRuleset, fam: Fam, era: u8, instance: usize, attempt: u32, rounds: u32, seed: u64, o: &DumpOptions) -> Result<Value, String> {
    let (aggressive, record, plant) = (o.aggressive, o.record, &o.plant[..]);
    let (mut game, mut sc) = families::build(rules, fam, era, instance, attempt)?;
    let planted = plant_extras(rules, &mut game, &mut sc, plant);
    // The whole map known to both sides, as the trainers play it.
    let all: Vec<TileId> = (0..game.view().auth.map().tile_count() as u32).map(TileId).collect();
    game.debug_reveal(sc.learner, &all);
    game.debug_reveal(sc.enemy, &all);
    let win = if o.whole_map { Window::whole(&game) } else { Window::around(&game, sc.centre) };
    let sides = [sc.learner, sc.enemy];
    let mut cities: Vec<TileId> = game.view().auth.cities().iter().map(|(_, c)| c.tile).filter(|t| win.index(*t).is_some()).collect();
    cities.sort_by_key(|t| t.0);
    let pieces: Vec<UnitHandle> = sc.mine.iter().chain(&sc.theirs).map(|p| p.h).collect();
    let mut slots = Slots { ids: Vec::new(), promos: promotion_table(rules, &game, &pieces) };
    let scene = if record { scene_json(rules, &game, &sc, &win, &cities, &mut slots) } else {
        for p in sc.mine.iter().chain(&sc.theirs) {
            slots.slot_of(p.id);
        }
        Value::Null
    };
    let initial = if record { state(rules, &game, &win, &mut slots, sides, &cities) } else { Value::Null };
    let paths = if record && o.paths > 0 { path_costs(rules, &game, &win, &pieces, o.paths) } else { Value::Null };
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
                    let opts = options(rules, &game, &win, side, h, &slots.promos);
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
    Ok(json!({"scene": scene, "initial": initial, "steps": steps, "stop": stop, "seed": seed, "slots": slots.ids.len(), "planted": planted, "paths": paths,
        "play_s": play_started.elapsed().as_secs_f64(), "actions": actions, "rounds_played": rounds_played}))
}

/// The multi-turn path cost of each piece to `n` goals spread over the window (every k-th tile, a
/// per-piece offset): `[goal, turns, moves_left]` of `find_path_cost` (turns before the last step,
/// points left after it), `[goal, -1, -1]` when no path. The MLX wavefront must give the same.
fn path_costs(rules: &CompiledRuleset, game: &GameState, win: &Window, pieces: &[UnitHandle], n: usize) -> Value {
    let tiles = (win.w * win.h) as usize;
    let stride = (tiles / n.max(1)).max(1);
    let mut scratch = classic_rules::navigation::PathScratch::default();
    let out: Vec<Value> = pieces
        .iter()
        .enumerate()
        .map(|(i, h)| {
            let goals: Vec<Value> = (0..n)
                .map(|k| ((k * stride + i * 7) % tiles) as i32)
                .map(|g| {
                    let goal = win.tile(g);
                    let req = classic_rules::navigation::PathRequest::for_unit(game.view().auth, rules, *h, goal);
                    match req.and_then(|r| classic_rules::navigation::find_path_cost(game.view(), rules, &r, &mut scratch)) {
                        Some(c) => json!([g, c.turns, c.moves_left]),
                        None => json!([g, -1, -1]),
                    }
                })
                .collect();
            Value::Array(goals)
        })
        .collect();
    Value::Array(out)
}

/// Parity variants of a scene (`--plant`), to exercise rules the families rarely reach; measurement
/// only, the families are not changed. `citadel`: a citadel on the enemy's land nearest the arena
/// centre (the families' citadel stands on unowned land, where it hurts nobody); `aa`: an enemy
/// anti-aircraft unit (the first land kind with an interception range) next to the enemy's first piece,
/// in scenes with aircraft.
fn plant_extras(rules: &CompiledRuleset, game: &mut GameState, sc: &mut Scene, plant: &[String]) -> Vec<String> {
    let mut done = Vec::new();
    let dims = game.view().auth.map().dimensions;
    if plant.iter().any(|p| p == "citadel") {
        let citadel = rules.improvements.iter().position(|d| d.nearby_enemy_damage > 0).map(|i| classic_rules::ids::ImprovementId(i as u16));
        let auth = game.view().auth;
        let map = auth.map();
        let mut best: Option<(i32, TileId)> = None;
        for t in (0..map.tile_count() as u32).map(TileId) {
            let ok = map.owner(t).flatten() == Some(sc.enemy)
                && !classic_rules::navigation::terrain::is_water(map, t)
                && auth.indexes().city_on_tile(t).is_none()
                && auth.indexes().units_on(t).is_empty();
            if ok {
                let d = dims.distance(t, sc.centre).unwrap_or(999);
                if best.is_none_or(|(b, _)| d < b) {
                    best = Some((d, t));
                }
            }
        }
        if let (Some(c), Some((_, t))) = (citadel, best) {
            game.debug_set_improvement_batch(rules, &[(t, c)]);
            done.push(format!("citadel {}", t.0));
        }
    }
    if plant.iter().any(|p| p == "aa") {
        let has_air = sc.mine.iter().chain(&sc.theirs).any(|p| rules.units[p.kind.0 as usize].domain == kernel::rules::content::Domain::Air);
        let aa = rules.units.iter().position(|d| d.domain == kernel::rules::content::Domain::Land && d.combat.air_intercept_range > 0);
        if let (true, Some(k), Some(first)) = (has_air, aa, sc.theirs.first().map(|p| p.h)) {
            let kind = classic_rules::ids::UnitKindId(k as u16);
            let at = game.view().auth.units().get(first).map(|u| u.core.tile);
            if let Some(at) = at {
                let auth = game.view().auth;
                let spot = dims.neighbors(at).into_iter().find(|t| {
                    !classic_rules::navigation::terrain::is_water(auth.map(), *t) && auth.indexes().units_on(*t).is_empty() && auth.indexes().city_on_tile(*t).is_none()
                        && classic_rules::navigation::cost::enter_cost_on_map(auth.map(), *t).is_some()
                });
                if let Some(t) = spot {
                    let h = game.debug_spawn_unit(rules, sc.enemy, t, kind);
                    let id = game.view().auth.units().get(h).unwrap().core.id;
                    sc.theirs.push(super::Piece { h, id, kind, role: "planted-aa" });
                    done.push(format!("aa {} at {}", rules.units[k].name, t.0));
                }
            }
        }
    }
    // `withdraw`: every military piece of both sides gets Withdraw Before Melee; `capture`: Prize Ships
    // (CaptureDefeatedEnemy). The families rarely field the units that carry them (caravels,
    // destroyers, privateers); granted to every piece, the aggressive script meets both rules often.
    for (key, name) in [("withdraw", "Withdraw Before Melee"), ("capture", "Prize Ships")] {
        if !plant.iter().any(|p| p == key) {
            continue;
        }
        let Some(promo) = rules.promotions.iter().position(|d| d.name == name).map(|i| classic_rules::ids::PromotionId(i as u16)) else {
            continue;
        };
        let pieces: Vec<UnitHandle> = sc.mine.iter().chain(&sc.theirs).map(|p| p.h).collect();
        let mut n = 0;
        for h in pieces {
            let Some(kind) = game.view().auth.units().get(h).map(|u| u.core.kind) else { continue };
            if rules.is_civilian(kind) || rules.units[kind.0 as usize].domain == kernel::rules::content::Domain::Air {
                continue;
            }
            game.debug_grant_promotion(rules, h, promo);
            n += 1;
        }
        done.push(format!("{key} on {n} pieces"));
    }
    done
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
    let mut plant: Vec<String> = Vec::new();
    let mut whole_map = false;
    let mut paths = 0usize;
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
            "--plant" => plant = v()?.split(',').map(str::to_owned).collect(),
            "--whole-map" => whole_map = true,
            "--paths" => paths = v()?.parse().map_err(|e| format!("{e}"))?,
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
        let o = DumpOptions { aggressive, record, plant: plant.clone(), whole_map, paths };
        match dump_one(rules, f, e, inst, a, rounds, seed, &o) {
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
