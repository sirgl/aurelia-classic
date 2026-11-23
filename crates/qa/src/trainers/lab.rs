//! The lab: the world a trainer scene is painted on, and the unit kinds of each era and arm.
//!
//! The world is a generated two-nation game (no city-states, no barbarians, Quick speed) repainted
//! to flat open land everywhere, every unit removed, a home city for each nation in a far corner,
//! the two nations at war, and every technology of the scene's era and the eras before it known
//! to both. A scene clones the lab of its era and paints its own ground (hills, forest, water,
//! mountains) and units around the arena centre. Nothing here names a unit, a terrain or a nation
//! except where a trick names a unique unit (then by its rules id, with a fallback by arm).

use std::collections::BTreeMap;

use classic_rules::ids::{PlayerId, TileId, UnitKindId};
use classic_rules::scenario::MapScript;
use classic_rules::{ClassicGame, CompiledRuleset, GameState};
use kernel::ids::{FeatureId, TerrainId};
use kernel::rules::content::{Domain, UnitRole};

use crate::spec::{GameSpec, WorldSize};

/// SplitMix64's finaliser.
pub fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// A seeded draw source (a scene's every choice reads it in a fixed order).
#[derive(Debug, Clone)]
pub struct Draw(pub std::cell::Cell<u64>);

impl Draw {
    pub fn new(seed: u64) -> Draw {
        Draw(std::cell::Cell::new(seed))
    }
    pub fn next(&self) -> u64 {
        let v = mix(self.0.get());
        self.0.set(v);
        v
    }
    /// In `lo..=hi`.
    pub fn range(&self, lo: i64, hi: i64) -> i64 {
        if hi <= lo {
            return lo;
        }
        lo + (self.next() % (hi - lo + 1) as u64) as i64
    }
    pub fn chance(&self, percent: u64) -> bool {
        self.next() % 100 < percent
    }
    pub fn pick<T: Copy>(&self, v: &[T]) -> Option<T> {
        if v.is_empty() {
            None
        } else {
            Some(v[(self.next() % v.len() as u64) as usize])
        }
    }
    pub fn shuffle<T>(&self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            let j = (self.next() % (i as u64 + 1)) as usize;
            v.swap(i, j);
        }
    }
}

/// The arms a trainer draws units by (from the rules' role, domain, movement and promotions).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Arm {
    /// Foot melee and gunpowder infantry.
    Melee,
    /// Mounted melee (and armour).
    Mounted,
    /// Foot ranged.
    Ranged,
    /// Ranged with movement 4 or more (chariot archer, horse archer, keshik, camel archer).
    MountedRanged,
    /// Units that move after attacking with movement 4 or more (hit and run).
    HitRun,
    Siege,
    /// Melee that attacks only cities (battering ram, siege tower).
    CityAssault,
    NavalMelee,
    NavalRanged,
    Fighter,
    Bomber,
    General,
}

pub const ARMS: [Arm; 12] = [Arm::Melee, Arm::Mounted, Arm::Ranged, Arm::MountedRanged, Arm::HitRun, Arm::Siege, Arm::CityAssault, Arm::NavalMelee, Arm::NavalRanged, Arm::Fighter, Arm::Bomber, Arm::General];

impl Arm {
    pub fn name(self) -> &'static str {
        match self {
            Arm::Melee => "melee",
            Arm::Mounted => "mounted",
            Arm::Ranged => "ranged",
            Arm::MountedRanged => "mounted-ranged",
            Arm::HitRun => "hit-run",
            Arm::Siege => "siege",
            Arm::CityAssault => "city-assault",
            Arm::NavalMelee => "naval-melee",
            Arm::NavalRanged => "naval-ranged",
            Arm::Fighter => "fighter",
            Arm::Bomber => "bomber",
            Arm::General => "general",
        }
    }
}

fn has_promotion(rules: &CompiledRuleset, kind: UnitKindId, key: &str) -> bool {
    rules.units[kind.0 as usize].free_promotions.iter().any(|p| rules.promotions.get(p.0 as usize).is_some_and(|d| d.key == key))
}

/// The arms a kind serves in (one kind may serve in two: a keshik is mounted ranged and hit-and-run).
pub fn arms_of(rules: &CompiledRuleset, kind: UnitKindId) -> Vec<Arm> {
    let d = &rules.units[kind.0 as usize];
    let mut v = Vec::new();
    let moves_after = has_promotion(rules, kind, "core:promotion/can-move-after-attacking");
    let cities_only = has_promotion(rules, kind, "core:promotion/only-attacks-cities");
    match (d.domain, d.role) {
        (Domain::Land, _) if has_promotion(rules, kind, "core:promotion/great-general") => v.push(Arm::General),
        (Domain::Land, Some(UnitRole::Melee | UnitRole::Gun)) if cities_only => v.push(Arm::CityAssault),
        (Domain::Land, Some(UnitRole::Melee | UnitRole::Gun)) => v.push(Arm::Melee),
        (Domain::Land, Some(UnitRole::Mounted | UnitRole::Armor)) => v.push(Arm::Mounted),
        (Domain::Land, Some(UnitRole::Archer)) if d.movement >= 4 => v.push(Arm::MountedRanged),
        (Domain::Land, Some(UnitRole::Archer)) => v.push(Arm::Ranged),
        (Domain::Land, Some(UnitRole::Siege)) => v.push(Arm::Siege),
        (Domain::Sea, Some(UnitRole::NavalMelee)) => v.push(Arm::NavalMelee),
        (Domain::Sea, Some(UnitRole::NavalRanged)) => v.push(Arm::NavalRanged),
        (Domain::Air, Some(UnitRole::Fighter)) => v.push(Arm::Fighter),
        (Domain::Air, Some(UnitRole::Bomber)) => v.push(Arm::Bomber),
        _ => {}
    }
    if d.domain == Domain::Land && moves_after && d.movement >= 4 && !cities_only && d.strength > 0 {
        v.push(Arm::HitRun);
    }
    v
}

/// The era index of a kind: its own era, else its technology's.
pub fn era_of(rules: &CompiledRuleset, kind: UnitKindId) -> Option<u8> {
    let d = &rules.units[kind.0 as usize];
    if let Some(e) = d.era.and_then(|e| rules.eras.get(e.0 as usize)) {
        return Some(e.index);
    }
    d.required_tech.and_then(|t| rules.technologies.get(t.0 as usize)).and_then(|t| t.content_era).and_then(|e| rules.eras.get(e.0 as usize)).map(|e| e.index)
}

/// The era names of the rules, by index.
pub fn era_names(rules: &CompiledRuleset) -> Vec<String> {
    let mut v: Vec<(u8, String)> = rules.eras.iter().map(|e| (e.index, e.key.rsplit('/').next().unwrap_or(&e.key).to_owned())).collect();
    v.sort();
    v.into_iter().map(|(_, n)| n).collect()
}

pub fn era_index(rules: &CompiledRuleset, name: &str) -> Option<u8> {
    era_names(rules).iter().position(|n| n == name).map(|i| i as u8)
}

/// The kinds of an era by arm: the common kinds (no nation) of the arm introduced in the era;
/// an arm the era adds nothing to keeps the latest kind of an earlier era that the era does not
/// make obsolete. Unique kinds are listed apart (`uniques`).
#[derive(Debug, Clone, Default)]
pub struct EraKinds {
    pub era: u8,
    pub by_arm: BTreeMap<Arm, Vec<UnitKindId>>,
    /// Unique kinds of the era (a nation's), by arm.
    pub uniques: BTreeMap<Arm, Vec<UnitKindId>>,
}

impl EraKinds {
    pub fn of(rules: &CompiledRuleset, era: u8) -> EraKinds {
        let known_obsolete = |kind: UnitKindId| {
            rules.units[kind.0 as usize].obsolete_tech.and_then(|t| rules.technologies.get(t.0 as usize)).and_then(|t| t.content_era).and_then(|e| rules.eras.get(e.0 as usize)).is_some_and(|e| e.index <= era)
        };
        let mut out = EraKinds { era, ..Default::default() };
        for arm in ARMS {
            let mut best: Option<(u8, Vec<UnitKindId>)> = None;
            for i in 0..rules.units.len() {
                let k = UnitKindId(i as u16);
                let d = &rules.units[i];
                if d.nation.is_some() || d.unbuildable && arm != Arm::General || !arms_of(rules, k).contains(&arm) {
                    continue;
                }
                let Some(e) = era_of(rules, k) else { continue };
                if e > era || (e < era && known_obsolete(k) && arm != Arm::General) {
                    continue;
                }
                match &mut best {
                    Some((be, v)) if *be == e => v.push(k),
                    Some((be, _)) if *be > e => {}
                    _ => best = Some((e, vec![k])),
                }
            }
            if let Some((_, mut v)) = best {
                v.sort_by_key(|k| k.0);
                out.by_arm.insert(arm, v);
            }
            let mut u: Vec<UnitKindId> = (0..rules.units.len())
                .map(|i| UnitKindId(i as u16))
                .filter(|&k| rules.units[k.0 as usize].nation.is_some() && arms_of(rules, k).contains(&arm) && era_of(rules, k) == Some(era))
                .collect();
            u.sort_by_key(|k| k.0);
            if !u.is_empty() {
                out.uniques.insert(arm, u);
            }
        }
        out
    }

    pub fn has(&self, arm: Arm) -> bool {
        self.by_arm.contains_key(&arm)
    }

    /// A kind of `arm` drawn (the common kinds), None if the era has none.
    pub fn draw(&self, arm: Arm, d: &Draw) -> Option<UnitKindId> {
        self.by_arm.get(&arm).and_then(|v| d.pick(v))
    }
}

/// A kind by its rules id (a trick names it), else None.
pub fn kind_by_key(rules: &CompiledRuleset, key: &str) -> Option<UnitKindId> {
    rules.units.iter().position(|d| d.key == key).map(|i| UnitKindId(i as u16))
}

/// The ground kinds the painter uses, found from the rules' data.
#[derive(Debug, Clone)]
pub struct Grounds {
    /// Flat open land (no defence bonus, move cost 1, a city may be founded; the most food).
    pub open: TerrainId,
    /// The open land's index in the rules' terrains (features and improvements name it).
    pub open_index: usize,
    /// Hills (the rules' high ground, defence bonus).
    pub hills: TerrainId,
    /// Impassable land.
    pub mountain: Option<TerrainId>,
    /// Shallow water and deep water.
    pub coast: TerrainId,
    pub ocean: Option<TerrainId>,
    /// A feature with a defence bonus that is not impassable (forest-like), and which terrains take it.
    pub cover: Option<FeatureId>,
}

impl Grounds {
    pub fn find(rules: &CompiledRuleset, game: &GameState) -> Result<Grounds, String> {
        let kinds = game.view().auth.map().terrain_kinds();
        // The map holds terrain codes (`map_code`); features and improvements name the rules' index.
        let ids: Vec<(usize, u16, &kernel::rules::compiled::TerrainDefinition)> = rules.terrains.iter().enumerate().filter_map(|(i, d)| d.map_code.map(|c| (i, c, d))).collect();
        let food = |d: &kernel::rules::compiled::TerrainDefinition| d.yields.food;
        let (open_index, open) = ids
            .iter()
            .filter(|(_, c, d)| !d.water && !d.impassable && d.can_found_city && d.movement_cost <= 1 && !kinds.is_hills(*c) && !kinds.is_mountains(*c))
            .max_by_key(|(i, _, d)| (food(d), std::cmp::Reverse(*i)))
            .map(|(i, c, _)| (*i, TerrainId(*c)))
            .ok_or("no open land terrain")?;
        let hills = ids.iter().find(|(_, c, d)| !d.water && kinds.is_hills(*c)).map(|(_, c, _)| TerrainId(*c)).ok_or("no hills terrain")?;
        let mountain = ids.iter().find(|(_, _, d)| !d.water && d.impassable).map(|(_, c, _)| TerrainId(*c));
        let mut water: Vec<(usize, u16, &kernel::rules::compiled::TerrainDefinition)> = ids.iter().filter(|(_, _, d)| d.water).copied().collect();
        // Shallow water first: the water kind with the most food is the coast.
        water.sort_by_key(|(i, _, d)| (std::cmp::Reverse(food(d)), *i));
        let coast = water.first().map(|(_, c, _)| TerrainId(*c)).ok_or("no water terrain")?;
        let ocean = water.get(1).map(|(_, c, _)| TerrainId(*c));
        let cover = rules
            .features
            .iter()
            .enumerate()
            .filter(|(_, f)| f.defense_pct > 0 && !f.impassable && !f.requires_river && f.valid_terrains.iter().any(|t| usize::from(t.0) == open_index))
            .min_by_key(|(i, f)| (f.movement_cost, *i))
            .map(|(i, _)| FeatureId(i as u16));
        Ok(Grounds { open, open_index, hills, mountain, coast, ocean, cover })
    }
}

/// The fast-mode switches of a run (set once from the command line, read by the lab and `play`).
pub mod mode {
    use std::sync::atomic::{AtomicU8, Ordering};
    /// World size of the lab: 0 small (the whole map as before), 1 tiny, 2 duel.
    pub static WORLD: AtomicU8 = AtomicU8::new(0);
    /// Fog: 0 the whole map shown to both sides (as before), 1 each side sees what its units and cities see.
    pub static FOG: AtomicU8 = AtomicU8::new(0);
    /// AI: 0 full, 1 combat layer only.
    pub static AI: AtomicU8 = AtomicU8::new(0);
    pub fn world() -> u8 {
        WORLD.load(Ordering::Relaxed)
    }
    pub fn fog() -> bool {
        FOG.load(Ordering::Relaxed) == 1
    }
    pub fn combat_only() -> bool {
        AI.load(Ordering::Relaxed) == 1
    }
}

/// The lab of an era: the world before the scene.
#[derive(Clone)]
pub struct Lab {
    pub game: GameState,
    pub learner: PlayerId,
    pub enemy: PlayerId,
    pub era: u8,
    pub grounds: Grounds,
    pub kinds: EraKinds,
    /// The arena centre (the scene's units stand within ARENA of it).
    pub centre: TileId,
    pub homes: [TileId; 2],
}

/// The arena's reach around its centre (tiles a scene may paint and place on).
pub const ARENA: i32 = 9;
/// Gold each side gets (upkeep of the scene's units; the closed arena takes away what it buys).
pub const CHEST: u32 = 300;

pub fn dist(game: &GameState, a: TileId, b: TileId) -> i32 {
    game.view().auth.map().dimensions.distance(a, b).unwrap_or(i32::MAX)
}

pub fn tiles_within(game: &GameState, centre: TileId, reach: i32) -> Vec<TileId> {
    let dims = game.view().auth.map().dimensions;
    (0..game.view().auth.map().tile_count() as u32).map(TileId).filter(|t| dims.distance(*t, centre).is_some_and(|d| d <= reach)).collect()
}

/// The lab of `era` (deterministic: the same for every scene of the era).
pub fn build_lab(rules: &CompiledRuleset, era: u8) -> Result<Lab, String> {
    let mut spec = GameSpec::new(MapScript::Pangaea, match mode::world() { 1 => WorldSize::Tiny, 2 => WorldSize::Duel, _ => WorldSize::Small }, 0x7a1e_5ab0);
    spec.nations = 2;
    spec.city_states = 0;
    spec.barbarians = classic_rules::world::barbarians::BarbarianMode::None;
    let speed = rules.speed_named("quick").ok_or("no speed quick")?;
    spec = spec.speed(&speed.key);
    let mut game = crate::record::new_game(rules, &spec);
    let grounds = Grounds::find(rules, &game)?;
    // Repaint: every tile open land, no feature, no improvement, no resource.
    let n = game.view().auth.map().tile_count() as u32;
    let all: Vec<TileId> = (0..n).map(TileId).collect();
    let paint: Vec<(TileId, TerrainId)> = all.iter().map(|t| (*t, grounds.open)).collect();
    game.debug_set_terrain_batch(rules, &paint);
    game.debug_clear_features(rules, &all);
    for &t in &all {
        if matches!(game.view().auth.map().resource(t), Some(Some(_))) {
            game.debug_clear_resource(rules, t);
        }
    }
    let majors = crate::bench::modules::majors(&game);
    if majors.len() < 2 {
        return Err("the lab needs two nations".to_owned());
    }
    let (learner, enemy) = (majors[0], majors[1]);
    // The map's corners: home cities far from the centre.
    let dims = game.view().auth.map().dimensions;
    let (w, h) = (dims.width as i64, dims.height as i64);
    let at = |x: i64, y: i64| TileId((y.clamp(0, h - 1) * w + x.clamp(0, w - 1)) as u32);
    let centre = at(w / 2, h / 2);
    let homes = [at(2, h / 2), at(w - 3, h / 2)];
    // Every unit of every player away; home cities.
    let units: Vec<classic_rules::ids::UnitHandle> = game.view().auth.units().iter_handles().map(|(h, _)| h).collect();
    for u in units {
        game.debug_remove_unit(rules, u);
    }
    for (p, t) in [(learner, homes[0]), (enemy, homes[1])] {
        game.debug_found_city(rules, p, t);
    }
    // The era's technologies.
    for p in [learner, enemy] {
        for (i, t) in rules.technologies.iter().enumerate() {
            let e = t.content_era.and_then(|e| rules.eras.get(e.0 as usize)).map_or(0, |e| e.index);
            if e <= era && !t.repeatable {
                game.debug_learn_tech(rules, p, kernel::ids::TechId(i as u16));
            }
        }
        game.debug_earn(p, CHEST);
    }
    game.debug_meet(learner, enemy);
    game.debug_force_war(rules, learner, enemy);
    if !game.view().auth.diplomacy().at_war(learner, enemy) {
        return Err("the lab's nations could not go to war".to_owned());
    }
    let kinds = EraKinds::of(rules, era);
    Ok(Lab { game, learner, enemy, era, grounds, kinds, centre, homes })
}

/// The lab of an era, built once per thread and cloned for each scene.
pub fn lab(rules: &CompiledRuleset, era: u8) -> Result<Lab, String> {
    thread_local! {
        static LABS: std::cell::RefCell<BTreeMap<u8, Result<Lab, String>>> = const { std::cell::RefCell::new(BTreeMap::new()) };
    }
    LABS.with(|cell| {
        let mut m = cell.borrow_mut();
        m.entry(era).or_insert_with(|| build_lab(rules, era)).clone()
    })
}
