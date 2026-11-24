//! The trainer families: how each scene is drawn, what wins it, its references and its scripted human.
//!
//! WIN RULES (fixed before the first run; "lost" counts the learner's scene units that are dead or
//! captured, "killed" the enemy's combat units dead; a round is a game round, 0 = the start):
//! The enemy's units in duel, focus, hold, kite, retreat and general are a scripted HUNTER (they strike
//! at even odds or better, else close on the nearest of ours; ranged keep at range behind a melee unit):
//! the enemy's AI, given hunt tasks, wandered off the field in the pilot, and a scene nobody attacks in
//! tests nothing. In hold the hunter commits to the assault (any blow the rules accept).
//! - duel: every enemy unit dead and one of ours alive within 12 rounds (N of ours, 2-4, against N or,
//!   from N = 3, N - 1).
//! - focus: 3 enemy units killed within 4 rounds with at most 1 of ours lost (2 of the 4 foes start
//!   wounded to 25-70 %).
//! - hold: (a wall of mountains across the map, a pass three tiles wide on hills) no enemy unit has
//!   stood on the pass's middle tile or west of the wall at a round end, and one of ours is
//!   alive when 10 rounds end (or every attacker is dead first); the attackers are melee and mounted,
//!   one and a half times the defenders (at least 3).
//! - choke: every holder dead within 14 rounds with at least half of ours alive (the holders are a
//!   scripted human: they hold the pass on hills, fortify and shoot, never advance).
//! - bait: 2 ambushers killed with at most 1 of ours lost within 12 rounds (a scripted human leaves a
//!   civilian 2 tiles in front of our post, an ambush behind it; from round 4 the ambush comes on).
//! - kite: every enemy unit dead within 14 rounds with none of ours lost (2 hit-and-run units of the
//!   era against 2-3 slow melee of the era before).
//! - guard: every civilian of ours alive and ours when 12 rounds end (or every raider dead); the
//!   raiders are a scripted human that hunts civilians.
//! - convoy: a city of ours founded within 14 rounds by the settler (its start is too close to our
//!   city to found on); a scripted raider hunts it.
//! - raid: 3 enemy improvements pillaged within 10 rounds with at most 1 of ours lost.
//! - counter: fewer than 2 of our improvements pillaged when 10 rounds end (or every raider dead);
//!   the raiders are a scripted human that pillages.
//! - siege: the city (walls, and castle from the medieval era on, by the rules' buildings) taken
//!   within 20 rounds. Measures: rounds to take, units and production lost (quick vs cheap).
//! - ram: the enemy capital taken within 14 rounds by an army with a city-assault unit (battering
//!   ram / siege tower by rules id; fallback the era's siege unit) and a capturer.
//! - rush: our capital still ours when 14 rounds end (or every rusher dead); a scripted human rushes
//!   it (ranged shoot from 2 tiles, melee takes it).
//! - retreat: at least 75 % of our army alive when 10 rounds end against a force twice its size.
//! - flank: the strong defender killed within 3 rounds with at most 1 of ours lost.
//! - general: every enemy unit dead within 12 rounds and our Great General alive.
//! - citadel: every holder dead within 14 rounds with at least half of ours alive; the holder stands
//!   on a citadel (the rules' improvement that damages adjacent enemies).
//! - bombard: (3 ranged ships and a melee ship against a city of 2-4 without walls) the coastal city taken, or at 10 % of its hit points or less at a round end, within 12
//!   rounds, by ships.
//! - sink: every embarked enemy unit dead within 6 rounds.
//! - airstrike: 2 enemy units killed within 6 rounds (bombers based in our city, a melee spotter in
//!   front: an air strike needs its target in sight).
//! - prey (an opportunity, Roman's rule: the right call depends on the context): context `land`
//!   (one landmass, a short route, no other threat: attacking is expected): the weak city taken within
//!   14 rounds. Context `sea` (the city over water) and `guarded` (a strong army of the enemy beside
//!   the city): either call is fine: WIN = the city taken, or at least 75 % of our army alive when 14
//!   rounds end. Never a bare attack rate.
//! Added after the first run (the defence sides of tricks the first families played only one side of):
//! - wounded: at most 1 of ours lost when 6 rounds end (2 of our 3-4 units start at 30-50 %; the enemy
//!   is the hunter, which strikes the weakest it reaches).
//! - harass: both raiders dead within 12 rounds with at most 1 of ours lost (2 slow melee, a mounted
//!   and a ranged unit of ours against 2 horse archers of a scripted human, the keshik half the time, that shoot and
//!   step back out of reach).
//! - snipe: an enemy civilian (worker, Great General) captured or killed within 8 rounds (it walks to its
//!   city with an escort).
//! - surrounded: our strong unit alive when 4 rounds end, or 2 attackers killed (3-4 melee of a scripted
//!   human close round it, then strike).

use std::collections::BTreeMap;

use classic_rules::ids::{CityHandle, PlayerId, TileId, UnitKindId};
use classic_rules::{ClassicGame, Command, CompiledRuleset, GameState};
use kernel::ids::BuildingKindId;
use serde::{Deserialize, Serialize};

use super::act::{self, Aim, Nerve};
use super::assault;
use super::camp;
use super::memory;
use super::lab::{self, dist, Arm, Draw, Lab};
use super::{Piece, Scene};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Fam {
    Duel,
    Focus,
    Hold,
    Choke,
    Bait,
    Kite,
    Guard,
    Convoy,
    Raid,
    Counter,
    Siege,
    Ram,
    Rush,
    Retreat,
    Flank,
    General,
    Citadel,
    Bombard,
    Sink,
    Airstrike,
    Prey,
    // Added after the first run (appended: the older families keep their seeds).
    Wounded,
    Harass,
    Snipe,
    Surrounded,
    // Part 2: the planned assault (appended again: the older families keep their seeds).
    AssaultHeal,
    PromotionChoice,
    HillArchers,
    CityFire,
    PlannedAssault,
    // The memory trainers (`memory.rs`; appended again: the older families keep their seeds).
    AmbushFog,
    HuntFog,
    // The "at any cost" assault trainers (`assault.rs`; appended again: the older families keep their seeds).
    AssaultReinforce,
    AssaultPeace,
    // The camp trainers (`camp.rs`; appended again: the older families keep their seeds).
    Camp,
    CampRace,
}

impl Fam {
    pub const ALL: [Fam; 36] = [
        Fam::Duel,
        Fam::Focus,
        Fam::Hold,
        Fam::Choke,
        Fam::Bait,
        Fam::Kite,
        Fam::Guard,
        Fam::Convoy,
        Fam::Raid,
        Fam::Counter,
        Fam::Siege,
        Fam::Ram,
        Fam::Rush,
        Fam::Retreat,
        Fam::Flank,
        Fam::General,
        Fam::Citadel,
        Fam::Bombard,
        Fam::Sink,
        Fam::Airstrike,
        Fam::Prey,
        Fam::Wounded,
        Fam::Harass,
        Fam::Snipe,
        Fam::Surrounded,
        Fam::AssaultHeal,
        Fam::PromotionChoice,
        Fam::HillArchers,
        Fam::CityFire,
        Fam::PlannedAssault,
        Fam::AmbushFog,
        Fam::HuntFog,
        Fam::AssaultReinforce,
        Fam::AssaultPeace,
        Fam::Camp,
        Fam::CampRace,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Fam::Duel => "duel",
            Fam::Focus => "focus",
            Fam::Hold => "hold",
            Fam::Choke => "choke",
            Fam::Bait => "bait",
            Fam::Kite => "kite",
            Fam::Guard => "guard",
            Fam::Convoy => "convoy",
            Fam::Raid => "raid",
            Fam::Counter => "counter",
            Fam::Siege => "siege",
            Fam::Ram => "ram",
            Fam::Rush => "rush",
            Fam::Retreat => "retreat",
            Fam::Flank => "flank",
            Fam::General => "general",
            Fam::Citadel => "citadel",
            Fam::Bombard => "bombard",
            Fam::Sink => "sink",
            Fam::Airstrike => "airstrike",
            Fam::Prey => "prey",
            Fam::Wounded => "wounded",
            Fam::Harass => "harass",
            Fam::Snipe => "snipe",
            Fam::Surrounded => "surrounded",
            Fam::AssaultHeal => "assault-heal",
            Fam::PromotionChoice => "promotion-choice",
            Fam::HillArchers => "hill-archers",
            Fam::CityFire => "city-fire",
            Fam::PlannedAssault => "planned-assault",
            Fam::AmbushFog => "ambush-fog",
            Fam::HuntFog => "hunt-fog",
            Fam::AssaultReinforce => "assault-reinforce",
            Fam::AssaultPeace => "assault-peace",
            Fam::Camp => "camp",
            Fam::CampRace => "camp-race",
        }
    }

    pub fn parse(s: &str) -> Result<Fam, String> {
        Fam::ALL.into_iter().find(|f| f.name() == s).ok_or_else(|| format!("unknown family {s} ({})", Fam::ALL.iter().map(|f| f.name()).collect::<Vec<_>>().join(", ")))
    }

    pub fn rounds(self) -> u32 {
        match self {
            Fam::Duel | Fam::Bait | Fam::Guard | Fam::General | Fam::Bombard => 12,
            Fam::Focus => 4,
            Fam::Flank => 3,
            Fam::Hold | Fam::Raid | Fam::Counter | Fam::Retreat => 10,
            Fam::Choke | Fam::Kite | Fam::Convoy | Fam::Ram | Fam::Rush | Fam::Citadel | Fam::Prey => 14,
            Fam::Siege => 20,
            Fam::Sink | Fam::Airstrike | Fam::Wounded => 6,
            Fam::Harass => 12,
            Fam::Snipe => 8,
            Fam::Surrounded => 4,
            f if memory::is_memory(f) => memory::rounds(f),
            f if camp::is_camp(f) => camp::rounds(f),
            f => assault::rounds(f),
        }
    }

    /// The solvers of the family, the main reference first.
    pub fn solvers(self) -> &'static [&'static str] {
        match self {
            Fam::Duel => &["ref-fight", "ref-hold", "ref-careful"],
            Fam::Focus => &["ref-focus", "ref-fight"],
            Fam::Hold => &["ref-hold", "ref-hold-even"],
            Fam::Choke | Fam::Citadel => &["ref-choke", "ref-fight"],
            Fam::Bait => &["ref-post", "ref-fight"],
            Fam::Kite => &["ref-kite", "ref-kite-careful"],
            Fam::Guard => &["ref-guard", "ref-guard-strike"],
            Fam::Convoy => &["ref-convoy"],
            Fam::Raid => &["ref-pillage"],
            Fam::Counter => &["ref-hunt", "ref-fight"],
            Fam::Siege | Fam::Ram => &["ref-siege", "ref-press"],
            Fam::Rush => &["ref-defend", "ref-fight"],
            Fam::Retreat => &["ref-flee"],
            Fam::Flank => &["ref-flank", "ref-press"],
            Fam::General => &["ref-fight", "ref-careful"],
            Fam::Bombard => &["ref-bombard"],
            Fam::Sink => &["ref-fight"],
            Fam::Airstrike => &["ref-air"],
            Fam::Prey => &["ref-siege", "ref-hold"],
            Fam::Wounded => &["ref-flee-wounded", "ref-fight"],
            Fam::Harass => &["ref-screen", "ref-fight"],
            Fam::Snipe => &["ref-snipe"],
            Fam::Surrounded => &["ref-break", "ref-flee"],
            f if memory::is_memory(f) => memory::solvers(f),
            f if camp::is_camp(f) => camp::solvers(f),
            f => assault::solvers(f),
        }
    }

    /// The tricks of the human playbook the family stands for, with the side the learner plays
    /// (`U`: the learner may use it, `D`: the learner faces a scripted human using it).
    pub fn tricks(self) -> &'static [(&'static str, char)] {
        match self {
            Fam::Duel => &[("MT4", 'U')],
            Fam::Focus => &[("MT7", 'U'), ("MT4", 'U')],
            Fam::Hold => &[("MT1", 'U'), ("MT15", 'U')],
            Fam::Choke => &[("MT1", 'D')],
            Fam::Bait => &[("MT2", 'D'), ("MT3", 'D')],
            Fam::Kite => &[("MT7", 'U')],
            Fam::Guard => &[("MO7", 'D'), ("E1", 'D'), ("MT8", 'D'), ("CS4", 'D')],
            Fam::Convoy => &[("M2", 'D'), ("M4", 'D'), ("MO7", 'D')],
            Fam::Raid => &[("MO6", 'U'), ("MO5", 'U')],
            Fam::Counter => &[("MO6", 'D'), ("MO5", 'D'), ("D3", 'D')],
            Fam::Siege => &[("MO9", 'U'), ("MO10", 'U'), ("V2", 'U')],
            Fam::Ram => &[("MO1", 'U')],
            Fam::Rush => &[("MO1", 'D'), ("MO8", 'D'), ("MO12", 'D')],
            Fam::Retreat => &[],
            Fam::Flank => &[("MT4", 'U')],
            Fam::General => &[("MT12", 'U'), ("MT8", 'U')],
            Fam::Citadel => &[("MT10", 'D'), ("MO2", 'D'), ("MO4", 'D')],
            Fam::Bombard => &[("N3", 'U')],
            Fam::Sink => &[("MT5", 'U'), ("MT6", 'U'), ("N1", 'U'), ("N2", 'U')],
            Fam::Airstrike => &[("A1", 'U'), ("A3", 'U')],
            Fam::Prey => &[("MO8", 'U'), ("MO1", 'U'), ("MO11", 'U')],
            Fam::Wounded => &[("MT7", 'D')],
            Fam::Harass => &[("MO12", 'D')],
            Fam::Snipe => &[("MO7", 'U'), ("MT8", 'U'), ("E1", 'U'), ("M4", 'U'), ("CS4", 'U')],
            Fam::Surrounded => &[("MT4", 'D')],
            f if memory::is_memory(f) || camp::is_camp(f) => &[],
            f => assault::tricks(f),
        }
    }

    /// What the family asks, for the doc and the page.
    pub fn task(self) -> &'static str {
        match self {
            Fam::Duel => "skirmish N vs M on open ground with clutter",
            Fam::Focus => "kill the wounded first: 3 of 4 foes in 4 rounds",
            Fam::Hold => "hold a mountain pass on hills against 1.5 times the force",
            Fam::Choke => "break a human holding a pass (melee on hills, ranged behind)",
            Fam::Bait => "a civilian bait in front of our post, an ambush behind it",
            Fam::Kite => "hit-and-run units (keshik, horseman, cavalry, tanks) against slow melee, no loss",
            Fam::Guard => "keep civilians (worker, Great General) from raiders",
            Fam::Convoy => "walk a settler past a raider and found a city",
            Fam::Raid => "pillage 3 improvements around an enemy city",
            Fam::Counter => "stop raiders pillaging our land",
            Fam::Siege => "take a walled city with siege units",
            Fam::Ram => "take the capital with a battering ram or siege tower and a capturer",
            Fam::Rush => "hold the capital against an early rush",
            Fam::Retreat => "save the army from a force twice its size",
            Fam::Flank => "surround and kill a strong defender in 3 rounds",
            Fam::General => "fight with a Great General's aura and keep him alive",
            Fam::Citadel => "break a holder on a citadel (damage to every adjacent unit)",
            Fam::Bombard => "bombard and take a coastal city with ships",
            Fam::Sink => "sink embarked units",
            Fam::Airstrike => "strike a stack with bombers",
            Fam::Prey => "a weak fresh city in reach: take it when the context says so",
            Fam::Wounded => "a human focus-fires our wounded units: save them",
            Fam::Harass => "slow melee and a ranged unit against a human's hit-and-run horse (keshik)",
            Fam::Snipe => "catch an escorted civilian (worker, Great General) of the enemy",
            Fam::Surrounded => "a strong unit of ours about to be surrounded: break out or survive",
            f if memory::is_memory(f) => memory::task(f),
            f if camp::is_camp(f) => camp::task(f),
            f => assault::task(f),
        }
    }

    /// Whether the family can be drawn in an era (the kinds it needs exist in the era's rules data).
    pub fn claims(self, rules: &CompiledRuleset, k: &lab::EraKinds) -> bool {
        let land = k.has(Arm::Melee) && k.has(Arm::Ranged);
        match self {
            Fam::Kite => k.has(Arm::HitRun) && k.has(Arm::Melee),
            Fam::Harass => (k.has(Arm::MountedRanged) || k.uniques.contains_key(&Arm::MountedRanged)) && k.has(Arm::Melee),
            Fam::Snipe => (k.has(Arm::HitRun) || k.has(Arm::Mounted)) && k.has(Arm::Melee),
            Fam::Siege => land && (k.has(Arm::Siege)),
            Fam::Ram => land && (k.uniques.contains_key(&Arm::CityAssault) || k.has(Arm::CityAssault)),
            Fam::General => land && k.has(Arm::General),
            Fam::Citadel => land && citadel(rules).is_some(),
            // An era that brings its own ranged ship (an older one does not scratch the era's cities).
            Fam::Bombard => k.by_arm.get(&Arm::NavalRanged).is_some_and(|v| v.iter().any(|x| lab::era_of(rules, *x) == Some(k.era))) || k.uniques.contains_key(&Arm::NavalRanged),
            Fam::Sink => (k.has(Arm::NavalMelee) || k.has(Arm::NavalRanged)) && embark_known(rules, k.era),
            Fam::Airstrike => k.has(Arm::Bomber),
            // The assault families need the era's siege unit (a city of any era wears down the archers of the era before it).
            f if assault::is_assault(f) => land && k.has(Arm::Siege),
            _ => land,
        }
    }
}

fn embark_known(rules: &CompiledRuleset, era: u8) -> bool {
    rules.technologies.iter().any(|t| t.allows_embarking && t.content_era.and_then(|e| rules.eras.get(e.0 as usize)).is_some_and(|e| e.index <= era))
}

/// The rules' citadel: the improvement that damages adjacent enemies.
pub fn citadel(rules: &CompiledRuleset) -> Option<kernel::ids::ImprovementId> {
    rules.improvements.iter().position(|d| d.nearby_enemy_damage > 0).map(|i| kernel::ids::ImprovementId(i as u16))
}

/// An improvement to pillage: valid on open land, with yields, the most food (a farm by data).
fn farm(rules: &CompiledRuleset, open: usize) -> Option<kernel::ids::ImprovementId> {
    rules
        .improvements
        .iter()
        .enumerate()
        .filter(|(_, d)| d.nation.is_none() && !d.unbuildable && d.nearby_enemy_damage == 0 && d.valid_terrains.iter().any(|t| usize::from(t.0) == open) && d.yields.food > 0)
        .max_by_key(|(i, d)| (d.yields.food, std::cmp::Reverse(*i)))
        .map(|(i, _)| kernel::ids::ImprovementId(i as u16))
}

// ---------------------------------------------------------------------------
// Building helpers

pub(super) struct Painter<'a> {
    pub(super) rules: &'a CompiledRuleset,
    pub(super) lab: &'a Lab,
    pub(super) game: GameState,
    pub(super) d: Draw,
    pub(super) w: i64,
    pub(super) h: i64,
    pub(super) cx: i64,
    pub(super) cy: i64,
}

impl Painter<'_> {
    pub(super) fn xy(&self, t: TileId) -> (i64, i64) {
        (i64::from(t.0) % self.w, i64::from(t.0) / self.w)
    }
    pub(super) fn at(&self, dx: i64, dy: i64) -> Option<TileId> {
        let (x, y) = (self.cx + dx, self.cy + dy);
        (x >= 0 && y >= 0 && x < self.w && y < self.h).then(|| TileId((y * self.w + x) as u32))
    }
    pub(super) fn standable(&self, t: TileId) -> bool {
        crate::battle::standable(self.rules, &self.game, t)
    }
    pub(super) fn free(&self, t: TileId) -> bool {
        let auth = self.game.view().auth;
        self.standable(t) && auth.indexes().units_on(t).is_empty() && auth.indexes().city_on_tile(t).is_none()
    }
    /// `n` free land tiles within `r` of `anchor`, nearest first (ties drawn).
    pub(super) fn spots(&mut self, anchor: TileId, r: i32, n: usize) -> Result<Vec<TileId>, String> {
        let mut v: Vec<(i32, u64, TileId)> = lab::tiles_within(&self.game, anchor, r).into_iter().filter(|t| self.free(*t)).map(|t| (dist(&self.game, t, anchor), 0, t)).collect();
        for x in &mut v {
            x.1 = self.d.next();
        }
        v.sort();
        if v.len() < n {
            return Err(format!("room for {} of {n} units", v.len()));
        }
        Ok(v.into_iter().take(n).map(|x| x.2).collect())
    }
    pub(super) fn paint(&mut self, tiles: &[TileId], terrain: kernel::ids::TerrainId) {
        let p: Vec<(TileId, kernel::ids::TerrainId)> = tiles.iter().map(|t| (*t, terrain)).collect();
        self.game.debug_set_terrain_batch(self.rules, &p);
    }
    pub(super) fn cover(&mut self, tiles: &[TileId]) {
        if let Some(f) = self.lab.grounds.cover {
            let p: Vec<(TileId, kernel::ids::FeatureId)> = tiles.iter().map(|t| (*t, f)).collect();
            self.game.debug_set_feature_batch(self.rules, &p);
        }
    }
    /// Random hills and cover on `n` arena tiles away from `keep`.
    pub(super) fn clutter(&mut self, n: usize, keep: &[TileId]) {
        let mut tiles: Vec<TileId> = lab::tiles_within(&self.game, self.lab.centre, lab::ARENA).into_iter().filter(|t| keep.iter().all(|k| dist(&self.game, *k, *t) > 0) && self.free(*t)).collect();
        self.d.shuffle(&mut tiles);
        let hills: Vec<TileId> = tiles.iter().take(n / 2).copied().collect();
        let cover: Vec<TileId> = tiles.iter().skip(n / 2).take(n - n / 2).copied().collect();
        let g = self.lab.grounds.hills;
        self.paint(&hills, g);
        self.cover(&cover);
    }
    pub(super) fn put(&mut self, side: PlayerId, tile: TileId, kind: UnitKindId, role: &'static str) -> Piece {
        let h = self.game.debug_spawn_unit(self.rules, side, tile, kind);
        let id = self.game.view().auth.units().get(h).expect("a spawned unit").core.id;
        Piece { h, id, kind, role }
    }
    /// A kind of `arm` with its fallbacks (an arm the era lacks).
    pub(super) fn kind(&mut self, arm: Arm) -> Option<UnitKindId> {
        let chain: &[Arm] = match arm {
            Arm::Mounted => &[Arm::Mounted, Arm::Melee],
            Arm::MountedRanged => &[Arm::MountedRanged, Arm::Ranged],
            Arm::HitRun => &[Arm::HitRun, Arm::Mounted, Arm::Melee],
            Arm::Siege => &[Arm::Siege, Arm::Ranged],
            Arm::CityAssault => &[Arm::CityAssault, Arm::Siege, Arm::Ranged],
            Arm::NavalRanged => &[Arm::NavalRanged, Arm::NavalMelee],
            Arm::NavalMelee => &[Arm::NavalMelee, Arm::NavalRanged],
            a => std::slice::from_ref(match a {
                Arm::Melee => &Arm::Melee,
                Arm::Ranged => &Arm::Ranged,
                Arm::Fighter => &Arm::Fighter,
                Arm::Bomber => &Arm::Bomber,
                _ => &Arm::General,
            }),
        };
        for a in chain {
            if let Some(k) = self.lab.kinds.draw(*a, &self.d) {
                return Some(k);
            }
        }
        None
    }
    /// A unique kind of the era for `arm`, by rules id first (a trick names it), drawn with `pct`.
    pub(super) fn unique(&mut self, arm: Arm, key: Option<&str>, pct: u64) -> Option<UnitKindId> {
        if !self.d.chance(pct) {
            return None;
        }
        if let Some(k) = key.and_then(|k| lab::kind_by_key(self.rules, k)).filter(|k| lab::era_of(self.rules, *k) == Some(self.lab.era)) {
            return Some(k);
        }
        self.lab.kinds.uniques.get(&arm).and_then(|v| self.d.pick(v))
    }
    pub(super) fn city(&mut self, side: PlayerId, tile: TileId, pop: u32, walls: bool) -> CityHandle {
        let c = self.game.debug_found_city(self.rules, side, tile);
        self.game.debug_set_population(self.rules, c, pop);
        if walls {
            for b in defences(self.rules, self.lab.era) {
                self.game.debug_add_building(self.rules, c, b);
            }
        }
        c
    }
}

/// The city defences of an era: the rules' walls and their successors known by the era.
pub(super) fn defences(rules: &CompiledRuleset, era: u8) -> Vec<BuildingKindId> {
    let keys = ["core:building/walls", "core:building/castle", "core:building/arsenal", "core:building/military-base"];
    rules
        .buildings
        .iter()
        .enumerate()
        .filter(|(_, b)| keys.contains(&b.key.as_str()))
        .filter(|(_, b)| b.required_tech.and_then(|t| rules.technologies.get(t.0 as usize)).and_then(|t| t.content_era).and_then(|e| rules.eras.get(e.0 as usize)).map_or(0, |e| e.index) <= era)
        .map(|(i, _)| BuildingKindId(i as u16))
        .collect()
}

pub(super) fn blank(fam: Fam, lab: &Lab, instance: usize, attempt: u32) -> Scene {
    Scene {
        fam,
        era: lab.era,
        instance,
        attempt,
        learner: lab.learner,
        enemy: lab.enemy,
        mine: Vec::new(),
        theirs: Vec::new(),
        target: None,
        hold: None,
        refuge: None,
        improved: Vec::new(),
        centre: lab.centre,
        rounds: fam.rounds(),
        hunt: false,
        hunter: false,
        aim_learner: None,
        aim_enemy: None,
        context: None,
        info: BTreeMap::new(),
        cities0: 0,
        start_round: 0,
        pillaged: Default::default(),
        taken: None,
        cap_broken: None,
        neutral: None,
        third: Vec::new(),
        founded: None,
        crossed: false,
        city_low: false,
        hold_lost: None,
        first_kill: None,
        kills_by: BTreeMap::new(),
        track: Default::default(),
        deadline: None,
        reinforce: Vec::new(),
    }
}

/// The seed of an attempt of an instance (the redraw rule: attempt a of instance i of a family and
/// era is drawn from these four numbers alone).
pub fn seed(fam: Fam, era: u8, instance: usize, attempt: u32) -> u64 {
    lab::mix(lab::mix(lab::mix((fam as u64) << 32 ^ u64::from(era)) ^ instance as u64) ^ u64::from(attempt) << 17)
}

pub(super) fn kinds_label(rules: &CompiledRuleset, ps: &[Piece]) -> String {
    let mut m: BTreeMap<String, u32> = BTreeMap::new();
    for p in ps {
        *m.entry(rules.units[p.kind.0 as usize].name.clone()).or_default() += 1;
    }
    m.into_iter().map(|(k, n)| if n > 1 { format!("{k} x{n}") } else { k }).collect::<Vec<_>>().join(", ")
}

/// Builds attempt `attempt` of `instance` of `fam` in `era`.
pub fn build(rules: &CompiledRuleset, fam: Fam, era: u8, instance: usize, attempt: u32) -> Result<(GameState, Scene), String> {
    // The camp families have a world of their own (the barbarians on, a neutral nation); every other family's lab is unchanged.
    let lab = if camp::is_camp(fam) { lab::camp_lab(rules, era)? } else { lab::lab(rules, era)? };
    if !fam.claims(rules, &lab.kinds) {
        return Err(format!("{} does not claim era {era}", fam.name()));
    }
    let dims = lab.game.view().auth.map().dimensions;
    let (w, h) = (i64::from(dims.width), i64::from(dims.height));
    let mut p = Painter { rules, lab: &lab, game: lab.game.clone(), d: Draw::new(seed(fam, era, instance, attempt)), w, h, cx: w / 2, cy: h / 2 };
    if assault::is_assault(fam) {
        let mut s = assault::build(&mut p, fam, &lab, instance, attempt)?;
        s.info.insert("attempt".into(), attempt.to_string());
        return Ok((p.game, s));
    }
    if camp::is_camp(fam) {
        let mut s = camp::build(&mut p, fam, &lab, instance, attempt)?;
        s.info.insert("attempt".into(), attempt.to_string());
        return Ok((p.game, s));
    }
    if memory::is_memory(fam) {
        let mut s = memory::build(&mut p, fam, &lab, instance, attempt)?;
        s.info.insert("attempt".into(), attempt.to_string());
        return Ok((p.game, s));
    }
    let mut s = blank(fam, &lab, instance, attempt);
    let (me, foe) = (lab.learner, lab.enemy);
    let c = lab.centre;
    let at = |p: &Painter, dx: i64, dy: i64| p.at(dx, dy).ok_or_else(|| "off the map".to_owned());
    match fam {
        Fam::Duel | Fam::General => {
            let n = p.d.range(2, 4) as usize;
            let arms = [Arm::Melee, Arm::Ranged, Arm::Mounted, Arm::Ranged, Arm::MountedRanged, Arm::Melee];
            let start = p.d.range(0, 2) as usize;
            let mine_k: Vec<UnitKindId> = (0..n).filter_map(|i| p.kind(if i == 0 { Arm::Melee } else { arms[(start + i) % arms.len()] })).collect();
            let m = if n >= 3 && p.d.chance(50) { n - 1 } else { n };
            let theirs_k: Vec<UnitKindId> = (0..m).filter_map(|i| p.kind(if i == 0 { Arm::Melee } else { arms[(start + i + 1) % arms.len()] })).collect();
            let gap = p.d.range(5, 7);
            let (a, b) = (at(&p, -gap / 2, 0)?, at(&p, gap - gap / 2, 0)?);
            p.clutter(p.d.range(4, 10) as usize, &[]);
            for (k, t) in mine_k.iter().zip(p.spots(a, 2, mine_k.len())?) {
                let role = if act::range_of(rules, *k).is_some() { "ranged" } else { "melee" };
                let piece = p.put(me, t, *k, role);
                s.mine.push(piece);
            }
            if fam == Fam::General {
                let g = p.kind(Arm::General).ok_or("no general kind")?;
                let t = p.spots(a, 2, 1)?[0];
                let piece = p.put(me, t, g, "general");
                s.mine.push(piece);
            }
            for (k, t) in theirs_k.iter().zip(p.spots(b, 2, theirs_k.len())?) {
                let piece = p.put(foe, t, *k, "foe");
                s.theirs.push(piece);
            }
            s.hunter = true;
        }
        Fam::Focus => {
            let a = at(&p, -2, 0)?;
            let b = at(&p, 1, 0)?;
            let mut mk = vec![p.kind(Arm::Melee).ok_or("no melee")?];
            for _ in 0..3 {
                mk.push(p.kind(Arm::Ranged).ok_or("no ranged")?);
            }
            for (k, t) in mk.iter().zip(p.spots(a, 1, 4)?) {
                let role = if act::range_of(rules, *k).is_some() { "ranged" } else { "melee" };
                let piece = p.put(me, t, *k, role);
                s.mine.push(piece);
            }
            let tk: Vec<UnitKindId> = [Arm::Melee, Arm::Mounted, Arm::Ranged, Arm::Melee].iter().filter_map(|a| p.kind(*a)).collect();
            let spots = p.spots(b, 1, tk.len())?;
            let mut wounded: Vec<usize> = (0..tk.len()).collect();
            p.d.shuffle(&mut wounded);
            wounded.truncate(2);
            for (i, (k, t)) in tk.iter().zip(spots).enumerate() {
                let piece = p.put(foe, t, *k, "foe");
                if wounded.contains(&i) {
                    let hp = (u32::from(kernel::world::unit::UNIT_MAX_HP) * p.d.range(25, 70) as u32 / 100) as u16;
                    p.game.debug_set_unit_hp(piece.h, hp);
                }
                s.theirs.push(piece);
            }
            s.hunter = true;
        }
        Fam::Hold | Fam::Choke | Fam::Citadel => {
            // A wall of impassable land across the map at the centre's column, one pass on hills.
            let wall_x = 0;
            let pass_dy = p.d.range(-2, 2);
            let mountain = lab.grounds.mountain.ok_or("no impassable terrain")?;
            // The pass is three tiles wide (a line of sight through it for the ranged units behind).
            let wall: Vec<TileId> = (-p.cy..p.h - p.cy).filter(|dy| (*dy - pass_dy).abs() > 1).filter_map(|dy| p.at(wall_x, dy)).collect();
            if fam != Fam::Citadel {
                p.paint(&wall, mountain);
            }
            let pass = at(&p, wall_x, pass_dy)?;
            let hills = lab.grounds.hills;
            let gap: Vec<TileId> = (-1..=1).filter_map(|k| p.at(wall_x, pass_dy + k)).collect();
            p.paint(&gap, hills);
            // The defenders' back tiles on hills, the attackers' side open with a little cover.
            let (def_side, att_side) = if fam == Fam::Hold { (-1, 1) } else { (1, -1) };
            let back: Vec<TileId> = lab::tiles_within(&p.game, pass, 1).into_iter().filter(|t| p.xy(*t).0 - p.cx == def_side).collect();
            p.paint(&back, hills);
            let (def, att) = if fam == Fam::Hold { (me, foe) } else { (foe, me) };
            let n_def_melee = if fam == Fam::Hold { p.d.range(1, 2) as usize } else { 1 };
            let n_def_ranged = p.d.range(1, 2) as usize;
            let mut def_pieces = Vec::new();
            let mk = p.kind(Arm::Melee).ok_or("no melee")?;
            let first = p.put(def, pass, mk, if fam == Fam::Hold { "melee" } else { "holder" });
            def_pieces.push(first);
            let mut backs = back.clone();
            backs.retain(|t| p.free(*t));
            p.d.shuffle(&mut backs);
            for i in 0..(n_def_melee - 1 + n_def_ranged) {
                let Some(&t) = backs.get(i) else { break };
                let (k, role) = if i < n_def_melee - 1 { (mk, "melee") } else { (p.kind(Arm::Ranged).ok_or("no ranged")?, if fam == Fam::Hold { "ranged" } else { "holder" }) };
                let piece = p.put(def, t, k, role);
                def_pieces.push(piece);
            }
            if fam == Fam::Citadel {
                // Raised by the holders' side (`debug_raise_citadel` claims the land round it): the damage zone
                // hurts only units next to a citadel on land an enemy at war owns (`citadel_damage`), so a citadel
                // set on neutral land damaged nobody.
                // The rules give the pass to a city of the holders within 5 tiles (`CITADEL_CITY_REACH`): a small
                // town of theirs stands 5 tiles behind the pass, out of the attackers' way (and of its own strike).
                citadel(rules).ok_or("no citadel")?;
                let town = at(&p, def_side * 5, pass_dy)?;
                p.game.debug_found_city(rules, def, town);
                if !p.game.debug_raise_citadel(rules, def, pass) {
                    return Err("no citadel to raise".into());
                }
                let owned = p.game.view().auth.map().owner(pass).flatten() == Some(def);
                if !owned {
                    return Err("the citadel's land is not the holders'".into());
                }
            }
            let n_att = if fam == Fam::Hold { (def_pieces.len() * 3).div_ceil(2).max(3) } else { (def_pieces.len() * 2).max(3) };
            let anchor = at(&p, att_side * p.d.range(4, 5), pass_dy)?;
            // Against a citadel the army is mostly shooters (the zone hurts what stands next to the pass).
            let arms = if fam == Fam::Citadel { [Arm::Ranged, Arm::Ranged, Arm::Melee, Arm::Siege, Arm::Ranged, Arm::Melee] } else { [Arm::Melee, Arm::Mounted, Arm::Ranged, Arm::Melee, Arm::Siege, Arm::Ranged] };
            let mut att_pieces = Vec::new();
            for (i, t) in p.spots(anchor, 2, n_att)?.into_iter().enumerate() {
                let arm = if fam == Fam::Hold { [Arm::Melee, Arm::Mounted, Arm::Melee, Arm::Mounted][i % 4] } else { arms[i % arms.len()] };
                let k = p.kind(arm).ok_or("no kind")?;
                let role = if fam == Fam::Hold { "foe" } else if act::range_of(rules, k).is_some() { "ranged" } else { "melee" };
                let piece = p.put(att, t, k, role);
                att_pieces.push(piece);
            }
            for q in &def_pieces {
                let _ = act::fortify(&mut p.game, rules, def, q.h);
            }
            if fam == Fam::Hold {
                s.mine = def_pieces;
                s.theirs = att_pieces;
                s.hunter = true;
            } else {
                s.mine = att_pieces;
                s.theirs = def_pieces;
            }
            s.hold = Some(pass);
            s.info.insert("pass".into(), pass.0.to_string());
        }
        Fam::Bait => {
            let post = at(&p, -2, 0)?;
            let hills = lab.grounds.hills;
            p.paint(&[post], hills);
            let mk = p.kind(Arm::Melee).ok_or("no melee")?;
            let melee = p.put(me, post, mk, "melee");
            s.mine.push(melee.clone());
            let _ = act::fortify(&mut p.game, rules, me, melee.h);
            for t in p.spots(at(&p, -3, 0)?, 1, 2)? {
                let k = p.kind(Arm::Ranged).ok_or("no ranged")?;
                let piece = p.put(me, t, k, "ranged");
                s.mine.push(piece);
            }
            let bait_t = at(&p, 0, 0)?;
            let civ = civil_kind(rules).ok_or("no worker kind")?;
            let bait = p.put(foe, bait_t, civ, "bait");
            s.theirs.push(bait);
            let amb_anchor = at(&p, 2, 0)?;
            let amb = p.spots(amb_anchor, 1, 3)?;
            p.cover(&amb);
            for (i, t) in amb.into_iter().enumerate() {
                let k = p.kind(if i == 1 { Arm::Mounted } else { Arm::Melee }).ok_or("no melee")?;
                let piece = p.put(foe, t, k, "ambush");
                s.theirs.push(piece);
            }
        }
        Fam::Kite => {
            let keshik = p.unique(Arm::HitRun, Some("core:unit/keshik"), 50);
            let n = 2;
            let a = at(&p, -3, 0)?;
            for t in p.spots(a, 1, n)? {
                let k = keshik.or_else(|| p.kind(Arm::HitRun)).ok_or("no hit-and-run kind")?;
                let piece = p.put(me, t, k, "hitrun");
                s.mine.push(piece);
            }
            // Slow melee: the era's melee with the fewest moves.
            // Slow melee: the melee with the fewest moves of the era before (foot against horse).
            let before = lab::EraKinds::of(rules, lab.era.saturating_sub(1));
            let slow = before.by_arm.get(&Arm::Melee).and_then(|v| v.iter().copied().min_by_key(|k| (rules.units[k.0 as usize].movement, rules.units[k.0 as usize].strength, k.0))).ok_or("no melee")?;
            let b = at(&p, p.d.range(2, 3), 0)?;
            let m = p.d.range(2, 3) as usize;
            for t in p.spots(b, 1, m)? {
                let piece = p.put(foe, t, slow, "foe");
                s.theirs.push(piece);
            }
            p.clutter(p.d.range(2, 6) as usize, &[]);
            s.hunter = true;
        }
        Fam::Wounded => {
            let n = p.d.range(3, 4) as usize;
            let arms = [Arm::Melee, Arm::Ranged, Arm::Mounted, Arm::Melee];
            for (i, t) in p.spots(at(&p, -2, 0)?, 1, n)?.into_iter().enumerate() {
                let k = p.kind(arms[i]).ok_or("no kind")?;
                let role = if act::range_of(rules, k).is_some() { "ranged" } else { "melee" };
                let piece = p.put(me, t, k, role);
                if i < 2 {
                    let hp = (u32::from(kernel::world::unit::UNIT_MAX_HP) * p.d.range(30, 50) as u32 / 100) as u16;
                    p.game.debug_set_unit_hp(piece.h, hp);
                }
                s.mine.push(piece);
            }
            for (i, t) in p.spots(at(&p, 3, 0)?, 1, n)?.into_iter().enumerate() {
                let k = p.kind([Arm::Melee, Arm::Mounted, Arm::Ranged, Arm::Mounted][i]).ok_or("no kind")?;
                let piece = p.put(foe, t, k, "foe");
                s.theirs.push(piece);
            }
            s.refuge = Some(at(&p, -9, 0)?);
            p.clutter(p.d.range(2, 6) as usize, &[]);
            s.hunter = true;
        }
        Fam::Harass => {
            let before = lab::EraKinds::of(rules, lab.era);
            let slow = before.by_arm.get(&Arm::Melee).and_then(|v| v.iter().copied().min_by_key(|k| (rules.units[k.0 as usize].movement, k.0))).ok_or("no melee")?;
            for (i, t) in p.spots(at(&p, -2, 0)?, 1, 4)?.into_iter().enumerate() {
                // Two slow melee, a ranged unit and a horse of our own (the answer to horse archers).
                let (k, role) = match i {
                    3 => (p.kind(Arm::Ranged).ok_or("no ranged")?, "ranged"),
                    2 => (p.kind(Arm::Mounted).ok_or("no mounted")?, "mounted"),
                    _ => (slow, "melee"),
                };
                let piece = p.put(me, t, k, role);
                s.mine.push(piece);
            }
            // Horse archers: the keshik by rules id half the time (it moves after it shoots), else the
            // era's mounted ranged unit, else one of the era's unique ones.
            let keshik = p.unique(Arm::MountedRanged, Some("core:unit/keshik"), 50);
            let common = lab.kinds.draw(Arm::MountedRanged, &p.d);
            let uniq = lab.kinds.uniques.get(&Arm::MountedRanged).and_then(|v| p.d.pick(v));
            for t in p.spots(at(&p, 4, 0)?, 1, 2)? {
                let k = keshik.or(common).or(uniq).ok_or("no mounted ranged kind")?;
                let piece = p.put(foe, t, k, "harasser");
                s.theirs.push(piece);
            }
            p.clutter(p.d.range(3, 7) as usize, &[]);
        }
        Fam::Snipe => {
            let home_t = at(&p, 6, 0)?;
            p.city(foe, home_t, 3, false);
            s.refuge = Some(home_t);
            let civ = if p.d.chance(33) && lab.kinds.has(Arm::General) { p.kind(Arm::General).ok_or("no general")? } else { civil_kind(rules).ok_or("no worker")? };
            let start = at(&p, 2, p.d.range(-2, 2))?;
            let ward = p.put(foe, start, civ, "civilian");
            s.theirs.push(ward);
            if p.d.chance(60) {
                let ek = p.kind(Arm::Melee).ok_or("no escort")?;
                let t = p.spots(start, 1, 1)?[0];
                let esc = p.put(foe, t, ek, "escort");
                s.theirs.push(esc);
            }
            for t in p.spots(at(&p, -2, 0)?, 1, 2)? {
                let k = p.kind(Arm::HitRun).ok_or("no fast kind")?;
                let piece = p.put(me, t, k, "raider");
                s.mine.push(piece);
            }
            s.aim_learner = Some(home_t);
        }
        Fam::Surrounded => {
            let t0 = at(&p, 0, 0)?;
            let strong = lab.kinds.by_arm.get(&Arm::Melee).and_then(|v| v.iter().copied().max_by_key(|k| (rules.units[k.0 as usize].strength, k.0))).ok_or("no melee")?;
            let me_p = p.put(me, t0, strong, "melee");
            s.mine.push(me_p);
            s.refuge = Some(at(&p, -9, 0)?);
            let n = p.d.range(3, 4) as usize;
            for (i, t) in p.spots(at(&p, 3, 0)?, 2, n)?.into_iter().enumerate() {
                let k = p.kind(if i % 2 == 0 { Arm::Melee } else { Arm::Mounted }).ok_or("no kind")?;
                let piece = p.put(foe, t, k, "attacker");
                s.theirs.push(piece);
            }
            s.hold = Some(t0);
        }
        Fam::Guard | Fam::Convoy => {
            let home_t = at(&p, -6, 0)?;
            p.city(me, home_t, 3, false);
            s.refuge = Some(home_t);
            let civ = if fam == Fam::Convoy { settler_kind(rules).ok_or("no settler")? } else if p.d.chance(33) && lab.kinds.has(Arm::General) { p.kind(Arm::General).ok_or("no general")? } else { civil_kind(rules).ok_or("no worker")? };
            let start = at(&p, -4, p.d.range(-1, 1))?;
            let ward = p.put(me, start, civ, "civilian");
            s.mine.push(ward);
            let n_civ2 = usize::from(fam == Fam::Guard && p.d.chance(40));
            if n_civ2 > 0 {
                let t = p.spots(at(&p, -3, 2)?, 1, 1)?[0];
                let w2 = p.put(me, t, civil_kind(rules).ok_or("no worker")?, "civilian");
                s.mine.push(w2);
            }
            let ek = p.kind(if p.d.chance(50) { Arm::Melee } else { Arm::Ranged }).ok_or("no escort")?;
            let t = p.spots(start, 1, 1)?[0];
            let esc = p.put(me, t, ek, "escort");
            s.mine.push(esc);
            let n_raid = p.d.range(1, 2) as usize;
            for t in p.spots(at(&p, p.d.range(0, 1), 0)?, 2, n_raid)? {
                let k = p.kind(Arm::Mounted).ok_or("no raider")?;
                let piece = p.put(foe, t, k, "raider");
                s.theirs.push(piece);
            }
            if fam == Fam::Convoy {
                // A site the rules accept, 4-6 tiles from the start, away from our city.
                let mut sites: Vec<TileId> = lab::tiles_within(&p.game, start, 6).into_iter().filter(|t| (4..=6).contains(&dist(&p.game, *t, start)) && dist(&p.game, *t, home_t) >= 5 && p.free(*t)).collect();
                p.d.shuffle(&mut sites);
                let settler = settler_kind(rules).ok_or("no settler")?;
                let mut site = None;
                for &t in sites.iter().take(20) {
                    let mut trial = p.game.clone();
                    let u = trial.debug_spawn_unit(rules, me, t, settler);
                    if trial.accepts_order(rules, me, &Command::FoundCity { settler: u }) {
                        site = Some(t);
                        break;
                    }
                }
                let site = site.ok_or("no site")?;
                s.target = Some(site);
                if p.game.accepts_order(rules, me, &Command::FoundCity { settler: s.mine[0].h }) {
                    return Err("the settler could found where it starts".to_owned());
                }
            }
            s.cities0 = p.game.view().auth.indexes().cities_of(me).len();
        }
        Fam::Raid | Fam::Counter => {
            let (owner, raider) = if fam == Fam::Raid { (foe, me) } else { (me, foe) };
            let city_t = at(&p, if fam == Fam::Raid { 3 } else { -3 }, 0)?;
            p.city(owner, city_t, p.d.range(2, 4) as u32, false);
            let farm = farm(rules, lab.grounds.open_index).ok_or("no improvement to pillage")?;
            let mut ring: Vec<TileId> = lab::tiles_within(&p.game, city_t, 3).into_iter().filter(|t| (2..=3).contains(&dist(&p.game, *t, city_t)) && p.free(*t)).collect();
            p.d.shuffle(&mut ring);
            ring.truncate(p.d.range(5, 7) as usize);
            p.game.debug_set_improvement_batch(rules, &ring.iter().map(|t| (*t, farm)).collect::<Vec<_>>());
            s.improved = ring;
            let gk = p.kind(Arm::Melee).ok_or("no melee")?;
            let g = p.put(owner, city_t, gk, if fam == Fam::Raid { "garrison" } else { "melee" });
            if fam == Fam::Raid {
                s.theirs.push(g);
            } else {
                s.mine.push(g);
                let k = p.kind(if p.d.chance(50) { Arm::Mounted } else { Arm::Ranged }).ok_or("no defender")?;
                let t = p.spots(city_t, 1, 1)?[0];
                let d2 = p.put(me, t, k, if act::range_of(rules, k).is_some() { "ranged" } else { "melee" });
                s.mine.push(d2);
            }
            let n = p.d.range(2, 3) as usize;
            let a = at(&p, if fam == Fam::Raid { -2 } else { 3 }, 0)?;
            for t in p.spots(a, 1, n)? {
                let k = p.kind(Arm::HitRun).ok_or("no fast kind")?;
                let piece = p.put(raider, t, k, "raider");
                if fam == Fam::Raid {
                    s.mine.push(piece);
                } else {
                    s.theirs.push(piece);
                }
            }
            s.target = Some(city_t);
            if fam == Fam::Raid {
                s.aim_learner = Some(city_t);
            }
        }
        Fam::Siege | Fam::Ram | Fam::Prey => {
            let ram = fam == Fam::Ram;
            // The ram scene takes the enemy's capital (its first city, the lab's home), the others a
            // city placed at the arena.
            let (city_t, army_anchor) = if ram {
                let t = lab.homes[1];
                let (x, y) = p.xy(t);
                (t, TileId(((y) * p.w + (x - p.d.range(5, 6))) as u32))
            } else if fam == Fam::Prey {
                (at(&p, 3, 0)?, at(&p, -3, 0)?)
            } else {
                (at(&p, 3, 0)?, at(&p, -p.d.range(3, 4), 0)?)
            };
            let context = if fam == Fam::Prey { Some(["land", "sea", "guarded"][(p.d.next() % 3) as usize]) } else { None };
            if !ram {
                let pop = if fam == Fam::Prey { 1 } else { p.d.range(3, 8) as u32 };
                p.city(foe, city_t, pop, fam == Fam::Siege);
            } else {
                let c = p.game.view().auth.indexes().city_on_tile(city_t).ok_or("no capital")?;
                p.game.debug_set_population(rules, c, p.d.range(3, 5) as u32);
                if p.d.chance(50) {
                    for b in defences(rules, lab.era).into_iter().take(1) {
                        p.game.debug_add_building(rules, c, b);
                    }
                }
            }
            if context == Some("sea") {
                // Water between: a band of water three tiles wide around the city's side.
                let coast = lab.grounds.coast;
                let band: Vec<TileId> = lab::tiles_within(&p.game, city_t, 4).into_iter().filter(|t| (2..=3).contains(&dist(&p.game, *t, city_t))).collect();
                p.paint(&band, coast);
                let far: Vec<TileId> = lab::tiles_within(&p.game, city_t, 6).into_iter().filter(|t| dist(&p.game, *t, city_t) >= 4 && p.xy(*t).0 > p.xy(city_t).0).collect();
                let _ = far;
            }
            let gk = p.kind(if fam == Fam::Prey { Arm::Melee } else if p.d.chance(50) { Arm::Ranged } else { Arm::Melee }).ok_or("no garrison")?;
            let g = p.put(foe, city_t, gk, "garrison");
            s.theirs.push(g);
            if fam == Fam::Siege && p.d.chance(50) {
                let t = p.spots(city_t, 1, 1)?[0];
                let k = p.kind(Arm::Melee).ok_or("no melee")?;
                let piece = p.put(foe, t, k, "garrison");
                s.theirs.push(piece);
            }
            if context == Some("guarded") {
                for t in p.spots(city_t, 2, 5)? {
                    let k = p.kind(if p.d.chance(50) { Arm::Melee } else { Arm::Mounted }).ok_or("no melee")?;
                    let piece = p.put(foe, t, k, "guard");
                    s.theirs.push(piece);
                }
            }
            let mut plan: Vec<(Arm, &'static str)> = Vec::new();
            if ram {
                let rk = p.unique(Arm::CityAssault, None, 100).or_else(|| p.kind(Arm::CityAssault));
                let n_ram = p.d.range(1, 2);
                for _ in 0..n_ram {
                    plan.push((Arm::CityAssault, "ram"));
                }
                plan.extend([(Arm::Ranged, "ranged"), (Arm::Ranged, "ranged"), (Arm::Melee, "capturer")]);
                if p.d.chance(50) {
                    plan.push((Arm::Melee, "capturer"));
                }
                let spots = p.spots(army_anchor, 2, plan.len())?;
                for ((arm, role), t) in plan.iter().zip(spots) {
                    let k = if *arm == Arm::CityAssault { rk.ok_or("no city-assault kind")? } else { p.kind(*arm).ok_or("no kind")? };
                    let piece = p.put(me, t, k, role);
                    s.mine.push(piece);
                }
            } else {
                let n_siege = if fam == Fam::Siege { p.d.range(2, 3) } else { p.d.range(0, 1) };
                for _ in 0..n_siege {
                    plan.push((Arm::Siege, "siege"));
                }
                for _ in 0..p.d.range(2, 3) {
                    plan.push((Arm::Melee, "capturer"));
                }
                for _ in 0..p.d.range(1, 2) {
                    plan.push((Arm::Ranged, "ranged"));
                }
                let spots = p.spots(army_anchor, 2, plan.len())?;
                for ((arm, role), t) in plan.iter().zip(spots) {
                    let k = p.kind(*arm).ok_or("no kind")?;
                    let piece = p.put(me, t, k, role);
                    s.mine.push(piece);
                }
            }
            if !ram {
                p.clutter(p.d.range(2, 6) as usize, &[city_t]);
            }
            s.target = Some(city_t);
            s.aim_learner = Some(city_t);
            if ram {
                s.centre = city_t;
            }
            if let Some(cx) = context {
                s.context = Some(cx.to_owned());
                s.info.insert("context".into(), cx.to_owned());
                s.info.insert("expected".into(), if cx == "land" { "attack".into() } else { "either".into() });
            }
        }
        Fam::Rush => {
            let cap = lab.homes[0];
            let c = p.game.view().auth.indexes().city_on_tile(cap).ok_or("no capital")?;
            p.game.debug_set_population(rules, c, p.d.range(2, 4) as u32);
            s.target = Some(cap);
            s.centre = cap;
            let mk = p.kind(Arm::Melee).ok_or("no melee")?;
            let g = p.put(me, cap, mk, "melee");
            s.mine.push(g);
            if p.d.chance(60) {
                let t = p.spots(cap, 1, 1)?[0];
                let k = p.kind(Arm::Ranged).ok_or("no ranged")?;
                let piece = p.put(me, t, k, "ranged");
                s.mine.push(piece);
            }
            let (x, y) = p.xy(cap);
            let anchor = TileId((y * p.w + x + p.d.range(5, 6)) as u32);
            let plan = [Arm::Ranged, Arm::Ranged, Arm::Ranged, Arm::Melee, Arm::Melee];
            let n = p.d.range(4, 5) as usize;
            let spots = p.spots(anchor, 2, n + 1)?;
            for (i, t) in spots.iter().take(n).enumerate() {
                let k = p.kind(plan[i]).ok_or("no kind")?;
                let role = if act::range_of(rules, k).is_some() { "ranged" } else { "melee" };
                let piece = p.put(foe, *t, k, role);
                s.theirs.push(piece);
            }
            // The capital assault (MO1 D): a battering ram / siege tower where the era has one, else a
            // siege unit where the era has one, half the time (an instance added to the rush, not a new one).
            if p.d.chance(50) {
                let extra = p.unique(Arm::CityAssault, None, 100).map(|k| (k, "ram")).or_else(|| lab.kinds.draw(Arm::Siege, &p.d).map(|k| (k, "ranged")));
                if let Some((k, role)) = extra {
                    let piece = p.put(foe, spots[n], k, role);
                    s.theirs.push(piece);
                }
            }
        }
        Fam::Retreat => {
            let home_t = at(&p, -8, 0)?;
            p.city(me, home_t, 3, false);
            s.refuge = Some(home_t);
            let n = p.d.range(4, 6) as usize;
            let arms = [Arm::Melee, Arm::Ranged, Arm::Siege, Arm::Ranged, Arm::Mounted, Arm::Melee];
            for (i, t) in p.spots(at(&p, -1, 0)?, 2, n)?.into_iter().enumerate() {
                let k = p.kind(arms[i]).ok_or("no kind")?;
                let piece = p.put(me, t, k, "army");
                s.mine.push(piece);
            }
            let m = n * 2;
            for (i, t) in p.spots(at(&p, p.d.range(4, 5), 0)?, 3, m)?.into_iter().enumerate() {
                let k = p.kind([Arm::Melee, Arm::Mounted, Arm::Ranged][i % 3]).ok_or("no kind")?;
                let piece = p.put(foe, t, k, "foe");
                s.theirs.push(piece);
            }
            s.hunter = true;
        }
        Fam::Flank => {
            let t0 = at(&p, 1, 0)?;
            if p.d.chance(40) {
                let hills = lab.grounds.hills;
                p.paint(&[t0], hills);
            }
            let strong = lab.kinds.by_arm.get(&Arm::Melee).and_then(|v| v.iter().copied().max_by_key(|k| (rules.units[k.0 as usize].strength, k.0))).ok_or("no melee")?;
            let target = p.put(foe, t0, strong, "target");
            let _ = act::fortify(&mut p.game, rules, foe, target.h);
            s.theirs.push(target);
            let n = p.d.range(3, 4) as usize;
            for (i, t) in p.spots(at(&p, -2, 0)?, 2, n)?.into_iter().enumerate() {
                let k = p.kind(if i % 2 == 0 { Arm::Melee } else { Arm::Mounted }).ok_or("no kind")?;
                let piece = p.put(me, t, k, "melee");
                s.mine.push(piece);
            }
            s.hold = Some(t0);
        }
        Fam::Bombard | Fam::Sink => {
            // A sea east of the centre: a band 7 tiles wide, the shore at the centre's column.
            let coast = lab.grounds.coast;
            let sea: Vec<TileId> = lab::tiles_within(&p.game, c, lab::ARENA + 2).into_iter().filter(|t| (1..=8).contains(&(p.xy(*t).0 - p.cx)) && (p.xy(*t).1 - p.cy).abs() <= 6).collect();
            p.paint(&sea, coast);
            // Islets wherever the water would be deep (no land beside it): every sea tile stays shallow,
            // so the early ships (ocean-impassable) sail everywhere.
            let open = lab.grounds.open;
            for t in sea.clone() {
                let map = p.game.view().auth.map();
                let deep = map.is_water(t) && map.dimensions.neighbors(t).as_slice().iter().all(|n| map.is_water(*n));
                if deep {
                    p.paint(&[t], open);
                }
            }
            let water_at = |p: &Painter, anchor: TileId, n: usize, d: &Draw| -> Vec<TileId> {
                let auth = p.game.view().auth;
                let mut v: Vec<TileId> = lab::tiles_within(&p.game, anchor, 2).into_iter().filter(|t| auth.map().is_water(*t) && auth.indexes().units_on(*t).is_empty()).collect();
                v.sort_by_key(|t| (dist(&p.game, *t, anchor), t.0));
                v.truncate(n + 2);
                d.shuffle(&mut v);
                v.truncate(n);
                v
            };
            if fam == Fam::Bombard {
                let city_t = at(&p, 0, 0)?;
                p.city(foe, city_t, p.d.range(2, 4) as u32, false);
                let gk = p.kind(Arm::Ranged).ok_or("no ranged")?;
                let g = p.put(foe, city_t, gk, "garrison");
                s.theirs.push(g);
                let anchor = at(&p, 4, 0)?;
                let dd = p.d.clone();
                let tiles = water_at(&p, anchor, 4, &dd);
                p.d = dd;
                if tiles.len() < 4 {
                    return Err("no sea room".to_owned());
                }
                let uq = p.unique(Arm::NavalRanged, None, 20);
                for (i, t) in tiles.into_iter().enumerate() {
                    let k = if i < 3 { uq.or_else(|| p.kind(Arm::NavalRanged)).ok_or("no ship")? } else { p.kind(Arm::NavalMelee).ok_or("no ship")? };
                    let piece = p.put(me, t, k, "ship");
                    s.mine.push(piece);
                }
                s.target = Some(city_t);
                s.aim_learner = Some(city_t);
            } else {
                // Our coastal city on the far shore; the enemy's land army embarked crossing to it.
                let home_t = at(&p, 9, 0)?;
                if p.game.view().auth.map().is_water(home_t) {
                    return Err("the far shore is water".to_owned());
                }
                p.city(me, home_t, 3, false);
                s.refuge = Some(home_t);
                let dd = p.d.clone();
                let emb = water_at(&p, at(&p, 3, 0)?, 3, &dd);
                let ships = water_at(&p, at(&p, 6, p.d.range(-2, 2))?, 2, &dd);
                p.d = dd;
                if emb.len() < 2 || ships.len() < 2 {
                    return Err("no sea room".to_owned());
                }
                for t in emb.into_iter().take(p.d.range(2, 3) as usize) {
                    let k = p.kind(if p.d.chance(50) { Arm::Melee } else { Arm::Ranged }).ok_or("no kind")?;
                    let piece = p.put(foe, t, k, "embarked");
                    s.theirs.push(piece);
                }
                for t in ships {
                    let k = p.kind(if p.d.chance(50) { Arm::NavalMelee } else { Arm::NavalRanged }).ok_or("no ship")?;
                    let piece = p.put(me, t, k, "ship");
                    s.mine.push(piece);
                }
                s.aim_enemy = Some(home_t);
            }
        }
        Fam::AssaultHeal | Fam::PromotionChoice | Fam::HillArchers | Fam::CityFire | Fam::PlannedAssault | Fam::AssaultReinforce | Fam::AssaultPeace => unreachable!("built by assault::build"),
        Fam::AmbushFog | Fam::HuntFog => unreachable!("built by memory::build"),
        Fam::Camp | Fam::CampRace => unreachable!("built by camp::build"),
        Fam::Airstrike => {
            let home_t = at(&p, -3, 0)?;
            p.city(me, home_t, 4, false);
            s.refuge = Some(home_t);
            let gk = p.kind(Arm::Melee).ok_or("no melee")?;
            let g = p.put(me, home_t, gk, "garrison");
            s.mine.push(g);
            for _ in 0..p.d.range(2, 3) {
                let k = p.kind(Arm::Bomber).ok_or("no bomber")?;
                let piece = p.put(me, home_t, k, "bomber");
                s.mine.push(piece);
            }
            let adx = p.d.range(2, 4);
            let anchor = at(&p, adx, 0)?;
            // A spotter (air strikes need a target in sight): a melee unit two tiles short of the stack.
            let spot_t = at(&p, adx - 2, 0)?;
            let sk = p.kind(Arm::Melee).ok_or("no melee")?;
            let spotter = p.put(me, spot_t, sk, "spotter");
            let _ = act::fortify(&mut p.game, rules, me, spotter.h);
            s.mine.push(spotter);
            for (i, t) in p.spots(anchor, 1, p.d.range(3, 4) as usize)?.into_iter().enumerate() {
                let k = p.kind([Arm::Melee, Arm::Ranged, Arm::Siege, Arm::Mounted][i % 4]).ok_or("no kind")?;
                let piece = p.put(foe, t, k, "foe");
                let _ = act::fortify(&mut p.game, rules, foe, piece.h);
                s.theirs.push(piece);
            }
        }
    }
    s.info.insert("mine".into(), kinds_label(rules, &s.mine));
    s.info.insert("theirs".into(), kinds_label(rules, &s.theirs));
    s.info.insert("attempt".into(), attempt.to_string());
    Ok((p.game, s))
}

pub(super) fn civil_kind(rules: &CompiledRuleset) -> Option<UnitKindId> {
    (0..rules.units.len())
        .map(|i| UnitKindId(i as u16))
        .filter(|&k| {
            let d = &rules.units[k.0 as usize];
            d.domain == kernel::rules::content::Domain::Land && !d.unbuildable && d.cost > 0 && d.required_tech.is_none() && d.nation.is_none() && d.builder && !d.founder
        })
        .min_by_key(|k| (rules.units[k.0 as usize].cost, k.0))
}

fn settler_kind(rules: &CompiledRuleset) -> Option<UnitKindId> {
    (0..rules.units.len())
        .map(|i| UnitKindId(i as u16))
        .filter(|&k| {
            let d = &rules.units[k.0 as usize];
            d.domain == kernel::rules::content::Domain::Land && !d.unbuildable && d.cost > 0 && d.required_tech.is_none() && d.nation.is_none() && d.founder
        })
        .min_by_key(|k| (rules.units[k.0 as usize].cost, k.0))
}

// ---------------------------------------------------------------------------
// Scripts

fn handles(ps: &[Piece]) -> Vec<classic_rules::ids::UnitHandle> {
    ps.iter().map(|p| p.h).collect()
}

/// The tiles a side's scripts strike: the other side's scene units (and the target city).
fn targets_of(scene: &Scene, of_enemy: bool, city: bool) -> impl Fn(&GameState) -> Vec<TileId> + '_ {
    move |g: &GameState| {
        let (side, ps) = if of_enemy { (scene.enemy, &scene.theirs) } else { (scene.learner, &scene.mine) };
        let mut v: Vec<TileId> = ps.iter().filter_map(|p| act::alive(g, p, side)).map(|c| c.tile).collect();
        if city {
            if let Some(t) = scene.target {
                let auth = g.view().auth;
                if auth.indexes().city_on_tile(t).and_then(|c| auth.cities().get(c)).is_some_and(|c| c.owner == side) {
                    v.push(t);
                }
            }
        }
        v.sort_by_key(|t| t.0);
        v.dedup();
        v
    }
}

fn city_hp(rules: &CompiledRuleset, game: &GameState, t: TileId) -> Option<(i64, i64)> {
    let auth = game.view().auth;
    let c = auth.indexes().city_on_tile(t).and_then(|c| auth.cities().get(c))?;
    let max = i64::from(classic_rules::combat::city::max_hit_points(auth, rules, c));
    Some((max - i64::from(c.damage), max))
}

/// The learner's reference `name` plays the scene units of the learner's seat for this turn.
pub fn reference_turn(rules: &CompiledRuleset, game: &mut GameState, scene: &mut Scene, name: &str) {
    if let Some(f) = assault::flags(name) {
        assault::turn(rules, game, scene, f);
        return;
    }
    if camp::is_camp(scene.fam) && matches!(name, "ref-camp" | "ref-camp-race") {
        camp::reference_turn(rules, game, scene, name);
        return;
    }
    if memory::is_memory(scene.fam) && matches!(name, "ref-recall" | "ref-forget") {
        memory::reference_turn(rules, game, scene, name);
        return;
    }
    let (me, foe) = (scene.learner, scene.enemy);
    let sc = scene.clone();
    let friends = handles(&sc.mine);
    let foes_only = targets_of(&sc, true, false);
    let foes_city = targets_of(&sc, true, true);
    let mut order: Vec<Piece> = sc.mine.clone();
    // Ranged and siege first (they soften), then melee, then the rest.
    order.sort_by_key(|p| (match p.role {
        "civilian" => 0,
        "ranged" | "siege" | "ship" | "bomber" => 1,
        "ram" => 2,
        "general" => 9,
        _ => 3,
    }, p.id.0));
    let refuge = sc.refuge.unwrap_or(sc.centre);
    for (turn_ix, p) in order.clone().into_iter().enumerate() {
        let Some(c) = act::alive(game, &p, me) else { continue };
        // The units of this script that have still to act this turn (this one first): the focus is judged on them.
        let to_act: Vec<classic_rules::ids::UnitHandle> = order[turn_ix..].iter().filter(|q| act::alive(game, q, me).is_some()).map(|q| q.h).collect();
        let mut focus: Option<TileId> = None;
        let ranged = act::range_of(rules, c.kind).is_some();
        let (aim, nerve, city): (Aim, Nerve, bool) = match (name, sc.fam) {
            ("passive", _) => continue,
            (_, Fam::Prey) if name == "ref-hold" => (Aim::Hold(c.tile), Nerve::Careful, false),
            ("ref-hold" | "ref-hold-even", Fam::Hold) => {
                let pass = sc.hold.unwrap_or(c.tile);
                let nerve = if name == "ref-hold" { Nerve::Careful } else { Nerve::Even };
                if p.role == "melee" && act::alive(game, &sc.mine[0], me).is_some_and(|f| f.tile == c.tile) {
                    (Aim::Hold(pass), nerve, false)
                } else if p.role == "melee" && !game.view().auth.indexes().units_on(pass).iter().any(|h| act::core_of(game, *h).is_some_and(|u| u.owner == me)) {
                    // The pass is empty: the reserve takes it.
                    (Aim::Hold(pass), nerve, false)
                } else {
                    (Aim::Hold(c.tile), Nerve::Even, false)
                }
            }
            ("ref-hold", _) => {
                let best = act::reachable(game, rules, me, p.h).into_iter().map(|(t, _)| t).max_by_key(|t| (act::defence(game, rules, *t, me), std::cmp::Reverse(t.0))).unwrap_or(c.tile);
                (if ranged { Aim::Support } else { Aim::Hold(best) }, Nerve::Even, false)
            }
            ("ref-choke", fam) if fam != Fam::Citadel && { focus = act::focus_target(game, rules, me, &to_act, &foes_only(game)); focus.is_some() } => {
                // Every unit strikes the weakest holder it can reach, the shooters first (the order above). No waiting
                // off the pass for the holders to be worn down: a holder that does not strike heals every turn (the
                // rules), so the wait only paid while the harness let it stay hurt; a blow now is worth more.
                (if ranged { Aim::Support } else { Aim::Close }, Nerve::Even, false)
            }
            ("ref-choke", _) => {
                let pass = sc.hold.unwrap_or(sc.centre);
                let held = sc.theirs_alive(game).iter().any(|(_, f)| f.tile == pass);
                // Against a citadel the melee go in only for a kill (its zone hurts every unit next to the pass each turn).
                let below = if sc.fam == Fam::Citadel { 3 } else { 6 };
                let weak = !held || sc.theirs_alive(game).iter().any(|(_, f)| f.hp < kernel::world::unit::UNIT_MAX_HP * below / 10);
                if ranged {
                    (Aim::Support, Nerve::Even, false)
                } else if weak {
                    (Aim::Close, Nerve::Even, false)
                } else {
                    // Wait two tiles off the pass, out of the holders' melee reach.
                    let stage = act::reachable(game, rules, me, p.h).into_iter().map(|(t, _)| t).min_by_key(|t| ((dist(game, *t, pass) - 2).abs(), act::threat_on(game, rules, foe, *t), t.0)).unwrap_or(c.tile);
                    (Aim::To(stage), Nerve::Careful, false)
                }
            }
            ("ref-post", _) => {
                if ranged {
                    (Aim::Hold(c.tile), Nerve::Even, false)
                } else {
                    (Aim::Hold(c.tile), Nerve::Careful, false)
                }
            }
            ("ref-kite" | "ref-kite-careful", _) => (Aim::Kite, if name == "ref-kite" { Nerve::Even } else { Nerve::Careful }, false),
            ("ref-guard" | "ref-guard-strike" | "ref-convoy", _) => {
                if p.role == "civilian" {
                    if let (Some(site), Fam::Convoy) = (sc.target, sc.fam) {
                        if c.tile == site || game.accepts_order(rules, me, &Command::FoundCity { settler: p.h }) && dist(game, c.tile, site) <= 1 {
                            let _ = act::try_cmd(game, rules, me, Command::FoundCity { settler: p.h });
                            continue;
                        }
                        (Aim::To(site), Nerve::Careful, false)
                    } else {
                        (Aim::To(refuge), Nerve::Careful, false)
                    }
                } else {
                    let ward = sc.mine.iter().find(|q| q.role == "civilian" && act::alive(game, q, me).is_some()).map(|q| q.h);
                    match ward {
                        Some(w) => (Aim::Guard(w), if name == "ref-guard" { Nerve::Careful } else { Nerve::Even }, false),
                        None => (Aim::Close, Nerve::Even, false),
                    }
                }
            }
            ("ref-pillage", _) => {
                pillage_turn(rules, game, me, &p, &sc.improved, sc.target);
                continue;
            }
            ("ref-hunt", _) => (Aim::Close, Nerve::Even, false),
            ("ref-siege", _) => {
                let low = sc.target.and_then(|t| city_hp(rules, game, t)).is_some_and(|(hp, max)| hp * 2 <= max);
                if ranged || p.role == "ram" {
                    (if p.role == "ram" { Aim::Close } else { Aim::Support }, if p.role == "ram" { Nerve::Reckless } else { Nerve::Even }, true)
                } else if low || !sc.mine.iter().any(|q| matches!(q.role, "siege" | "ranged" | "ram") && act::alive(game, q, me).is_some()) {
                    (Aim::To(sc.target.unwrap_or(sc.centre)), Nerve::Even, true)
                } else {
                    let t = sc.target.unwrap_or(sc.centre);
                    let stage = act::reachable(game, rules, me, p.h).into_iter().map(|(x, _)| x).min_by_key(|x| ((dist(game, *x, t) - 3).abs(), act::threat_on(game, rules, foe, *x), x.0)).unwrap_or(c.tile);
                    (Aim::To(stage), Nerve::Careful, true)
                }
            }
            ("ref-press", _) => (sc.target.map_or(Aim::Close, Aim::To), Nerve::Reckless, true),
            ("ref-defend", _) => {
                let cap = sc.target.unwrap_or(sc.centre);
                (Aim::Hold(if ranged || p.role == "melee" && c.tile == cap { cap } else { c.tile }), Nerve::Even, false)
            }
            ("ref-flee", _) => (Aim::Away(refuge), Nerve::Careful, false),
            ("ref-flank", _) => {
                let t0 = sc.hold.unwrap_or(sc.centre);
                let around = sc.mine_alive(game).iter().filter(|(_, m)| dist(game, m.tile, t0) == 1).count();
                if around >= 3 || game.view().auth.turn().round > sc.start_round + 1 {
                    (Aim::Close, Nerve::Reckless, false)
                } else {
                    (Aim::To(t0), Nerve::Careful, false)
                }
            }
            ("ref-bombard", _) => {
                let low = sc.target.and_then(|t| city_hp(rules, game, t)).is_some_and(|(hp, max)| hp * 3 <= max);
                if ranged {
                    (Aim::Support, Nerve::Even, true)
                } else if low {
                    (sc.target.map_or(Aim::Close, Aim::To), Nerve::Even, true)
                } else {
                    (Aim::Hold(c.tile), Nerve::Careful, true)
                }
            }
            ("ref-air", _) => {
                if p.role == "bomber" {
                    if act::debug() {
                        let ts = foes_only(game);
                        for t in &ts {
                            let cmd = Command::RangedAttack { unit: p.h, target: *t };
                            eprintln!("  bomber {} -> {} d{} accepts {:?} preview {:?}", c.tile.0, t.0, dist(game, c.tile, *t), game.accepts_order(rules, me, &cmd), act::preview(game, rules, p.h, None, *t));
                        }
                    }
                    if let Some((_, t)) = act::best_blow(game, rules, me, p.h, &foes_only(game), Nerve::Reckless) {
                        let _ = act::strike(game, rules, me, p.h, t);
                    }
                    act::stand(game, rules, p.h);
                    continue;
                }
                (Aim::Hold(c.tile), Nerve::Careful, false)
            }
            ("ref-careful", _) => (if ranged { Aim::Support } else { Aim::Close }, Nerve::Careful, false),
            ("ref-flee-wounded", _) => {
                if c.hp * 10 < kernel::world::unit::UNIT_MAX_HP * 6 {
                    (Aim::Away(refuge), Nerve::Careful, false)
                } else if ranged {
                    (Aim::Support, Nerve::Even, false)
                } else {
                    let ward = sc.mine_alive(game).into_iter().filter(|(q, u)| q.id != p.id && u.hp * 10 < kernel::world::unit::UNIT_MAX_HP * 6).map(|(q, _)| q.h).next();
                    (ward.map_or(Aim::Close, Aim::Guard), Nerve::Even, false)
                }
            }
            ("ref-screen", _) => {
                if ranged {
                    (Aim::Support, Nerve::Even, false)
                } else {
                    let shooter = sc.mine.iter().find(|q| q.role == "ranged" && act::alive(game, q, me).is_some()).map(|q| q.h);
                    if p.role == "mounted" {
                        (Aim::Close, Nerve::Even, false)
                    } else {
                        (shooter.map_or(Aim::Hold(c.tile), Aim::Guard), Nerve::Even, false)
                    }
                }
            }
            ("ref-snipe", _) => {
                civilian_hunt(rules, game, &sc, me, foe, &p);
                continue;
            }
            ("ref-break", _) => (Aim::Away(refuge), Nerve::Even, false),
            // ref-fight and every other name: the competent fight.
            _ => {
                if p.role == "general" {
                    (aura_aim(game, rules, &sc, me), Nerve::Careful, false)
                } else {
                    (if ranged { Aim::Support } else { Aim::Close }, Nerve::Even, false)
                }
            }
        };
        let by_city = |g: &GameState| {
            let t = sc.target.unwrap_or(sc.centre);
            let mut v: Vec<TileId> = foes_city(g).into_iter().filter(|x| dist(g, *x, t) <= 2).collect();
            if v.is_empty() {
                v = foes_only(g);
            }
            v
        };
        let focused = |_: &GameState| focus.into_iter().collect::<Vec<TileId>>();
        let targets: &dyn Fn(&GameState) -> Vec<TileId> = if focus.is_some() { &focused } else if city && matches!(name, "ref-siege" | "ref-bombard") { &by_city } else if city { &foes_city } else { &foes_only };
        act::unit_turn(game, rules, me, foe, p.h, &aim, nerve, targets, &friends);
    }
}

/// The general's tile: next to the most friends, out of the foes' reach.
fn aura_aim(game: &GameState, rules: &CompiledRuleset, sc: &Scene, me: PlayerId) -> Aim {
    let friends: Vec<TileId> = sc.mine_alive(game).iter().filter(|(p, _)| p.role != "general").map(|(_, c)| c.tile).collect();
    let g = sc.mine.iter().find(|p| p.role == "general").map(|p| p.h);
    let Some(g) = g else { return Aim::Hold(sc.centre) };
    let best = act::reachable(game, rules, me, g).into_iter().map(|(t, _)| t).min_by_key(|t| (act::threat_on(game, rules, sc.enemy, *t), -(friends.iter().filter(|f| dist(game, **f, *t) <= 2).count() as i32), t.0));
    Aim::Hold(best.unwrap_or(sc.centre))
}

/// One unit walks to the nearest unpillaged improvement of the list (outside the city's strike when
/// another is left) and pillages it.
fn pillage_turn(rules: &CompiledRuleset, game: &mut GameState, side: PlayerId, p: &Piece, improved: &[TileId], city: Option<TileId>) {
    let Some(c) = act::alive(game, p, side) else { return };
    let auth = game.view().auth;
    let open: Vec<TileId> = improved.iter().copied().filter(|t| !auth.map().pillaged(*t) && matches!(auth.map().improvement(*t), Some(Some(_)))).collect();
    if open.contains(&c.tile) && act::try_cmd(game, rules, side, Command::Pillage { unit: p.h }) {
        act::stand(game, rules, p.h);
        return;
    }
    let strike = classic_rules::combat::city::CITY_ATTACK_RANGE;
    let safe: Vec<TileId> = open.iter().copied().filter(|t| city.is_none_or(|ct| dist(game, *t, ct) > strike)).collect();
    let goals = if safe.is_empty() { open.clone() } else { safe };
    if let Some(goal) = goals.iter().copied().min_by_key(|t| (dist(game, c.tile, *t), t.0)) {
        act::move_by(game, rules, side, p.h, |g, x, _| (dist(g, x, goal), x.0));
        if act::alive(game, p, side).is_some_and(|n| n.tile == goal) {
            let _ = act::try_cmd(game, rules, side, Command::Pillage { unit: p.h });
        }
    }
    act::stand(game, rules, p.h);
}

/// The scripted human of the enemy's seat (the defence scenes); the other families leave the
/// enemy's units to its AI.
pub fn enemy_turn(rules: &CompiledRuleset, game: &mut GameState, scene: &mut Scene) {
    if assault::is_assault(scene.fam) {
        assault::enemy_turn(rules, game, scene);
        return;
    }
    if memory::is_memory(scene.fam) {
        memory::enemy_turn(rules, game, scene);
        return;
    }
    if camp::is_camp(scene.fam) {
        camp::enemy_turn(rules, game, scene);
        return;
    }
    let (me, foe) = (scene.enemy, scene.learner);
    let sc = scene.clone();
    let friends = handles(&sc.theirs);
    let ours = targets_of(&sc, false, false);
    let ours_city = targets_of(&sc, false, true);
    let rel = game.view().auth.turn().round.saturating_sub(sc.start_round);
    if sc.hunter {
        // The scripted hunter: every enemy unit strikes at even odds or better, else closes on the
        // nearest of ours (ranged units keep at their range behind a melee unit).
        for p in &sc.theirs {
            let Some(c) = act::alive(game, p, me) else { continue };
            let ranged = act::range_of(rules, c.kind).is_some();
            // The assault on the pass commits (a human attacker takes the poor odds of a fortified hill).
            let nerve = if sc.fam == Fam::Hold { Nerve::Reckless } else { Nerve::Even };
            act::unit_turn(game, rules, me, foe, p.h, if ranged { &Aim::Support } else { &Aim::Close }, nerve, &ours, &friends);
        }
        return;
    }
    match sc.fam {
        Fam::Choke | Fam::Citadel => {
            for p in &sc.theirs {
                if let Some(c) = act::alive(game, p, me) {
                    act::unit_turn(game, rules, me, foe, p.h, &Aim::Hold(c.tile), Nerve::Even, &ours, &friends);
                }
            }
        }
        Fam::Bait => {
            let near = sc.theirs_alive(game).iter().filter(|(p, _)| p.role == "ambush").any(|(_, a)| sc.mine_alive(game).iter().any(|(_, m)| dist(game, m.tile, a.tile) <= 2));
            for p in &sc.theirs {
                let Some(c) = act::alive(game, p, me) else { continue };
                if p.role == "bait" {
                    act::stand(game, rules, p.h);
                    continue;
                }
                let aim = if near || rel >= 4 { Aim::Close } else { Aim::Hold(c.tile) };
                act::unit_turn(game, rules, me, foe, p.h, &aim, if near { Nerve::Reckless } else { Nerve::Even }, &ours, &friends);
            }
        }
        Fam::Guard | Fam::Convoy => {
            for p in &sc.theirs {
                civilian_hunt(rules, game, &sc, me, foe, p);
            }
        }
        Fam::Harass => {
            for p in &sc.theirs {
                act::unit_turn(game, rules, me, foe, p.h, &Aim::Kite, Nerve::Reckless, &ours, &friends);
            }
        }
        Fam::Snipe => {
            let refuge = sc.refuge.unwrap_or(sc.centre);
            let ward = sc.theirs.iter().find(|q| q.role == "civilian" && act::alive(game, q, me).is_some()).map(|q| q.h);
            for p in &sc.theirs {
                let aim = if p.role == "civilian" { Aim::To(refuge) } else { ward.map_or(Aim::Close, Aim::Guard) };
                act::unit_turn(game, rules, me, foe, p.h, &aim, Nerve::Careful, &ours, &friends);
            }
        }
        Fam::Surrounded => {
            let t0 = sc.mine_alive(game).first().map_or(sc.centre, |(_, c)| c.tile);
            let around = sc.theirs_alive(game).iter().filter(|(_, u)| dist(game, u.tile, t0) == 1).count();
            let go = around >= 3 || rel >= 2;
            for p in &sc.theirs {
                let aim = if go { Aim::Close } else { Aim::To(t0) };
                act::unit_turn(game, rules, me, foe, p.h, &aim, if go { Nerve::Reckless } else { Nerve::Careful }, &ours, &friends);
            }
        }
        Fam::Counter => {
            for p in &sc.theirs {
                // A kill when one is offered, else pillage.
                if let Some((score, t)) = act::best_blow(game, rules, me, p.h, &ours(game), Nerve::Careful) {
                    if score >= 10_000 && act::strike(game, rules, me, p.h, t) {
                        act::stand(game, rules, p.h);
                        continue;
                    }
                }
                pillage_turn(rules, game, me, p, &sc.improved, sc.target);
            }
        }
        Fam::Rush => {
            let cap = sc.target.unwrap_or(sc.centre);
            let low = city_hp(rules, game, cap).is_some_and(|(hp, max)| hp * 3 <= max);
            for p in &sc.theirs {
                let Some(c) = act::alive(game, p, me) else { continue };
                let ranged = act::range_of(rules, c.kind).is_some();
                let aim = if ranged { Aim::Support } else if low || p.role == "ram" { Aim::To(cap) } else { Aim::Close };
                act::unit_turn(game, rules, me, foe, p.h, &aim, Nerve::Even, &ours_city, &friends);
            }
        }
        _ => {}
    }
}

/// A civilian hunter (the guard and convoy raiders, the snipe reference): captures a civilian it can reach, strikes an escort only for a
/// kill or a good exchange, else walks to the nearest civilian outside the strike of our cities.
fn civilian_hunt(rules: &CompiledRuleset, game: &mut GameState, sc: &Scene, me: PlayerId, foe: PlayerId, p: &Piece) {
    let Some(c) = act::alive(game, p, me) else { return };
    let prey_of_foe = if foe == sc.learner { sc.mine_alive(game) } else { sc.theirs_alive(game) };
    let civs: Vec<(Piece, TileId)> = prey_of_foe.into_iter().filter(|(q, _)| q.role == "civilian").map(|(q, u)| (q, u.tile)).collect();
    // Capture: a civilian alone on a tile reached this turn.
    for (q, t) in &civs {
        let guarded = game.view().auth.indexes().units_on(*t).iter().filter_map(|h| act::core_of(game, *h)).any(|u| u.owner == foe && !act::is_civilian(rules, u.kind));
        if guarded {
            continue;
        }
        if act::try_cmd(game, rules, me, Command::AttackUnit { attacker: p.h, defender: q.h }) {
            act::stand(game, rules, p.h);
            return;
        }
        let near: Vec<TileId> = act::reachable(game, rules, me, p.h).into_iter().filter(|(x, left)| *left > 0 && dist(game, *x, *t) == 1 && act::free_for(game, rules, p.h, *x)).map(|(x, _)| x).collect();
        if let Some(&x) = near.iter().min_by_key(|x| x.0) {
            if act::try_cmd(game, rules, me, Command::MoveUnit { unit: p.h, destination: x }) && act::try_cmd(game, rules, me, Command::AttackUnit { attacker: p.h, defender: q.h }) {
                act::stand(game, rules, p.h);
                return;
            }
        }
    }
    let targets = targets_of(sc, foe == sc.enemy, false);
    if let Some((_, t)) = act::best_blow(game, rules, me, p.h, &targets(game), Nerve::Careful) {
        if act::strike(game, rules, me, p.h, t) {
            act::stand(game, rules, p.h);
            return;
        }
    }
    let goal = civs.iter().map(|(_, t)| *t).min_by_key(|t| (dist(game, c.tile, *t), t.0));
    let cities: Vec<TileId> = game.view().auth.indexes().cities_of(foe).iter().filter_map(|h| game.view().auth.cities().get(*h)).map(|c| c.tile).collect();
    let strike = classic_rules::combat::city::CITY_ATTACK_RANGE;
    if let Some(goal) = goal {
        act::move_by(game, rules, me, p.h, |g, x, _| (cities.iter().any(|ct| dist(g, *ct, x) <= strike), dist(g, x, goal), x.0));
    }
    act::stand(game, rules, p.h);
}

// ---------------------------------------------------------------------------
// Judging

fn dead_combat(game: &GameState, rules: &CompiledRuleset, sc: &Scene) -> usize {
    sc.theirs.iter().filter(|p| !act::is_civilian(rules, p.kind) && act::alive(game, p, sc.enemy).is_none()).count()
}

fn lost(game: &GameState, sc: &Scene) -> usize {
    sc.mine.len() - sc.mine_alive(game).len()
}

fn owner_of_city(game: &GameState, t: TileId) -> Option<PlayerId> {
    let auth = game.view().auth;
    auth.indexes().city_on_tile(t).and_then(|c| auth.cities().get(c)).map(|c| c.owner)
}

/// The verdict after round `rel` (None: the scene goes on).
pub fn judge(rules: &CompiledRuleset, game: &GameState, sc: &mut Scene, rel: u32) -> Option<bool> {
    let limit = rel >= sc.rounds;
    let mine = sc.mine_alive(game);
    let theirs = sc.theirs_alive(game);
    let combat_theirs = theirs.iter().filter(|(p, _)| !act::is_civilian(rules, p.kind)).count();
    let killed = dead_combat(game, rules, sc);
    if killed > 0 && sc.first_kill.is_none() {
        sc.first_kill = Some(rel);
    }
    sc.kills_by.insert(rel, killed as u32);
    let lost = lost(game, sc);
    for &t in &sc.improved {
        if game.view().auth.map().pillaged(t) {
            sc.pillaged.insert(t);
        }
    }
    if let Some(t) = sc.target {
        if owner_of_city(game, t) == Some(sc.learner) && sc.taken.is_none() && sc.fam != Fam::Rush {
            sc.taken = Some(rel);
        }
        if let Some((hp, max)) = city_hp(rules, game, t) {
            if hp * 10 <= max {
                sc.city_low = true;
            }
        }
    }
    if sc.founded.is_none() && game.view().auth.indexes().cities_of(sc.learner).len() > sc.cities0 && sc.fam == Fam::Convoy {
        sc.founded = Some(rel);
    }
    let n = sc.mine.len();
    let civ_ok = sc.mine.iter().filter(|p| p.role == "civilian").all(|p| act::alive(game, p, sc.learner).is_some());
    let v = match sc.fam {
        Fam::AmbushFog | Fam::HuntFog => memory::judge(rules, game, sc, rel),
        Fam::Camp | Fam::CampRace => camp::judge(rules, game, sc, rel),
        Fam::Duel => {
            if combat_theirs == 0 && !mine.is_empty() {
                Some(true)
            } else if mine.is_empty() || limit {
                Some(false)
            } else {
                None
            }
        }
        Fam::General => {
            let general = sc.mine.iter().find(|p| p.role == "general").is_some_and(|p| act::alive(game, p, sc.learner).is_some());
            if !general || limit && combat_theirs > 0 {
                Some(false)
            } else if combat_theirs == 0 {
                Some(true)
            } else {
                None
            }
        }
        Fam::Focus => {
            if killed >= 3 && lost <= 1 {
                Some(true)
            } else if lost >= 2 || limit {
                Some(false)
            } else {
                None
            }
        }
        Fam::Hold => {
            let wall_x = i64::from(sc.centre.0) % i64::from(game.view().auth.map().dimensions.width);
            let w = i64::from(game.view().auth.map().dimensions.width);
            let pass = sc.hold;
            let crossed = theirs.iter().any(|(_, c)| i64::from(c.tile.0) % w < wall_x || Some(c.tile) == pass);
            if crossed {
                sc.crossed = true;
            }
            if crossed || mine.is_empty() {
                Some(false)
            } else if combat_theirs == 0 || limit {
                Some(true)
            } else {
                None
            }
        }
        Fam::Choke | Fam::Citadel => {
            if mine.len() * 2 < n {
                Some(false)
            } else if combat_theirs == 0 {
                Some(true)
            } else if limit {
                Some(false)
            } else {
                None
            }
        }
        Fam::Bait => {
            let amb = sc.theirs.iter().filter(|p| p.role == "ambush" && act::alive(game, p, sc.enemy).is_none()).count();
            if amb >= 2 && lost <= 1 {
                Some(true)
            } else if lost >= 2 || limit || mine.is_empty() {
                Some(false)
            } else {
                None
            }
        }
        Fam::Kite => {
            if lost > 0 {
                Some(false)
            } else if combat_theirs == 0 {
                Some(true)
            } else if limit {
                Some(false)
            } else {
                None
            }
        }
        Fam::Guard => {
            if !civ_ok {
                Some(false)
            } else if combat_theirs == 0 || limit {
                Some(true)
            } else {
                None
            }
        }
        Fam::Convoy => {
            if sc.founded.is_some() {
                Some(true)
            } else if !sc.mine.iter().any(|p| p.role == "civilian" && act::alive(game, p, sc.learner).is_some()) || limit {
                Some(false)
            } else {
                None
            }
        }
        Fam::Raid => {
            if sc.pillaged.len() >= 3 && lost <= 1 {
                Some(true)
            } else if limit || lost >= 2 {
                Some(false)
            } else {
                None
            }
        }
        Fam::Counter => {
            if sc.pillaged.len() >= 2 {
                Some(false)
            } else if combat_theirs == 0 || limit {
                Some(true)
            } else {
                None
            }
        }
        f @ (Fam::AssaultHeal | Fam::PromotionChoice | Fam::HillArchers | Fam::CityFire | Fam::PlannedAssault | Fam::AssaultReinforce | Fam::AssaultPeace) => {
            let cap = assault::loss_cap(f, sc.mine.len());
            if lost > cap && sc.cap_broken.is_none() {
                sc.cap_broken = Some(rel);
            }
            let broken = sc.cap_broken.is_some();
            if broken && !play_on_past_cap() {
                Some(false)
            } else if let Some(t) = sc.taken {
                // The "at any cost" families: a capture after the deadline (the reinforcements landed, the peace) loses.
                Some(!broken && sc.deadline.is_none_or(|d| t <= d))
            } else if limit || !mine.iter().any(|(p, _)| !act::is_civilian(rules, p.kind)) {
                Some(false)
            } else {
                None
            }
        }
        Fam::Siege | Fam::Ram => {
            if sc.taken.is_some() {
                Some(true)
            } else if limit || !mine.iter().any(|(p, _)| !act::is_civilian(rules, p.kind)) {
                Some(false)
            } else {
                None
            }
        }
        Fam::Prey => {
            let either = sc.context.as_deref() != Some("land");
            if sc.taken.is_some() {
                Some(true)
            } else if either && mine.len() * 4 < n * 3 {
                Some(false)
            } else if limit {
                Some(either && mine.len() * 4 >= n * 3)
            } else {
                None
            }
        }
        Fam::Rush => {
            let held = sc.target.is_some_and(|t| owner_of_city(game, t) == Some(sc.learner));
            if !held {
                Some(false)
            } else if combat_theirs == 0 || limit {
                Some(true)
            } else {
                None
            }
        }
        Fam::Retreat => {
            if mine.len() * 4 < n * 3 {
                Some(false)
            } else if limit {
                Some(true)
            } else {
                None
            }
        }
        Fam::Flank => {
            let target_dead = sc.theirs.iter().filter(|p| p.role == "target").all(|p| act::alive(game, p, sc.enemy).is_none());
            if target_dead && lost <= 1 {
                Some(true)
            } else if lost >= 2 || limit {
                Some(false)
            } else {
                None
            }
        }
        Fam::Bombard => {
            if sc.taken.is_some() || sc.city_low {
                Some(true)
            } else if limit || mine.is_empty() {
                Some(false)
            } else {
                None
            }
        }
        Fam::Sink => {
            if combat_theirs == 0 {
                Some(true)
            } else if limit {
                Some(false)
            } else {
                None
            }
        }
        Fam::Wounded => {
            if lost >= 2 {
                Some(false)
            } else if limit || combat_theirs == 0 {
                Some(true)
            } else {
                None
            }
        }
        Fam::Harass => {
            if lost >= 2 || limit && combat_theirs > 0 {
                Some(false)
            } else if combat_theirs == 0 {
                Some(true)
            } else {
                None
            }
        }
        Fam::Snipe => {
            let caught = sc.theirs.iter().filter(|q| q.role == "civilian").any(|q| act::alive(game, q, sc.enemy).is_none());
            if caught {
                Some(true)
            } else if limit || mine.is_empty() {
                Some(false)
            } else {
                None
            }
        }
        Fam::Surrounded => {
            if mine.is_empty() {
                Some(false)
            } else if limit || killed >= 2 {
                Some(true)
            } else {
                None
            }
        }
        Fam::Airstrike => {
            if killed >= 2 {
                Some(true)
            } else if limit {
                Some(false)
            } else {
                None
            }
        }
    };
    v
}

/// `ARMY_PLAY_ON_CAP=1`: an assault scene whose loss cap broke plays on to its capture or its round limit, so
/// the capture rate is measured beside the win (the verdict stays a loss: the win rules do not change).
fn play_on_past_cap() -> bool {
    std::env::var("ARMY_PLAY_ON_CAP").is_ok_and(|v| v == "1")
}

fn cost(rules: &CompiledRuleset, k: UnitKindId) -> f64 {
    f64::from(rules.units[k.0 as usize].cost)
}

pub fn measures(rules: &CompiledRuleset, game: &GameState, sc: &Scene, rel: u32) -> BTreeMap<String, f64> {
    let mut m = BTreeMap::new();
    let dead_mine: Vec<&Piece> = sc.mine.iter().filter(|p| act::alive(game, p, sc.learner).is_none()).collect();
    let dead_theirs: Vec<&Piece> = sc.theirs.iter().filter(|p| act::alive(game, p, sc.enemy).is_none() && !act::is_civilian(rules, p.kind)).collect();
    m.insert("units".into(), sc.mine.len() as f64);
    m.insert("lost".into(), dead_mine.len() as f64);
    m.insert("lost_cost".into(), dead_mine.iter().map(|p| cost(rules, p.kind)).sum());
    m.insert("killed".into(), dead_theirs.len() as f64);
    m.insert("killed_cost".into(), dead_theirs.iter().map(|p| cost(rules, p.kind)).sum());
    m.insert("rounds".into(), f64::from(rel));
    m.insert("first_kill".into(), sc.first_kill.map_or(-1.0, f64::from));
    if !sc.improved.is_empty() {
        m.insert("pillaged".into(), sc.pillaged.len() as f64);
    }
    if sc.target.is_some() {
        m.insert("taken".into(), sc.taken.map_or(-1.0, f64::from));
        m.insert("cap_broken".into(), sc.cap_broken.map_or(-1.0, f64::from));
        if let Some(d) = sc.deadline {
            m.insert("deadline".into(), f64::from(d));
        }
        if let Some((hp, max)) = sc.target.and_then(|t| city_hp(rules, game, t)) {
            m.insert("city_hp_share".into(), hp as f64 / max as f64);
        }
    }
    if assault::is_assault(sc.fam) {
        let t = &sc.track;
        m.insert("dmg_city".into(), t.dmg_enemy_turn as f64);
        m.insert("dmg_own".into(), t.dmg_own_turn as f64);
        m.insert("dmg_total".into(), (t.dmg_enemy_turn + t.dmg_own_turn) as f64);
        m.insert("city_dealt".into(), t.city_dealt as f64);
        m.insert("dealt_per_round".into(), t.city_dealt as f64 / f64::from(rel.max(1)));
        m.insert("rested".into(), f64::from(t.rested));
        m.insert("picks".into(), t.picks.len() as f64);
    }
    if camp::is_camp(sc.fam) {
        m.insert("camp_taken".into(), sc.taken.map_or(-1.0, f64::from));
        m.insert("camp_lost_to_neutral".into(), f64::from(u8::from(sc.info.contains_key("lost_to"))));
        m.insert("lost_at_clear".into(), sc.info.get("lost_at_clear").and_then(|v| v.parse().ok()).unwrap_or(-1.0));
        m.insert("cap_broken".into(), sc.cap_broken.map_or(-1.0, f64::from));
    }
    if sc.fam == Fam::Convoy {
        m.insert("founded".into(), sc.founded.map_or(-1.0, f64::from));
    }
    if sc.fam == Fam::Hold {
        m.insert("crossed".into(), f64::from(u8::from(sc.crossed)));
    }
    if sc.fam == Fam::Focus {
        m.insert("killed_by_2".into(), f64::from(sc.kills_by.get(&2).copied().unwrap_or(0)));
    }
    m
}

/// One trace line: where every scene unit stands, its hit points.
pub fn line(rules: &CompiledRuleset, game: &GameState, sc: &Scene, rel: u32) -> String {
    let part = |ps: &[Piece], side: PlayerId| -> String {
        ps.iter()
            .map(|p| {
                let n: String = rules.units[p.kind.0 as usize].name.chars().take(9).collect();
                match act::alive(game, p, side) {
                    Some(c) => format!("{n}[{}]@{}h{}", p.role.chars().next().unwrap_or('?'), c.tile.0, c.hp),
                    None => format!("{n}[{}]+gone", p.role.chars().next().unwrap_or('?')),
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    };
    let city = sc.target.and_then(|t| city_hp(rules, game, t).map(|(hp, max)| format!(" city@{} p{} hp{hp}/{max}", t.0, owner_of_city(game, t).map_or(9, |o| o.0))));
    let third = sc.neutral.map(|n| format!(" | neutral: {}", part(&sc.third, n))).unwrap_or_default();
    format!("r+{rel}{} | mine: {} | foe: {}{third} | pillaged {}", city.unwrap_or_default(), part(&sc.mine, sc.learner), part(&sc.theirs, sc.enemy), sc.pillaged.len())
}
