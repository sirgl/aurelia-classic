//! The army net (`work/nets/army-net/DESIGN.md`): a learned policy that conducts the army of one side
//! for a turn, unit decision by unit decision, toward an objective given from above. A MEASUREMENT
//! tool of the trainers (`ai-exam suite`, classic/docs/ai-trainers.md): it changes no rule and no AI.
//!
//! - `observe`: what the net sees: the tokens (the objective, every unit, every city) and, for the unit
//!   that acts, its legal actions (`knowledge::actions::unit_actions` and "done") with their features.
//!   One builder serves the recorder and the player, so training and play read the same numbers.
//! - `record`: the teachers' decisions (the references, any AI variant) on the trainers' scenes, in the
//!   binary format `nn/aurelia_nn/army/data.py` reads.
//! - `model`: the forward pass in f32 on the CPU, weights exported from MLX (`army/export.py`).
//! - `seat`: the player `army-net` of `ai-exam suite`.

pub mod map_conv;
pub mod model;
pub mod obsdump;
pub mod observe;
pub mod parity;
pub mod record;
pub mod seat;

use classic_rules::ids::{PlayerId, TileId};
use classic_rules::{CompiledRuleset, GameState};
use kernel::ids::EntityId;

use crate::trainers::families::Fam;
use crate::trainers::{act, Scene};

/// What the army is asked to do (the objective token's kind).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Aim {
    /// Kill the enemy's force.
    Destroy,
    /// Keep a tile or a city.
    Hold,
    /// Take a city.
    TakeCity,
    /// Keep our civilians or our land from raiders.
    Protect,
    /// Pillage the land around an enemy city.
    Pillage,
    /// Save the army.
    Survive,
    /// Catch an enemy civilian.
    Catch,
    /// Bring our settler to a site.
    Escort,
}

pub const AIMS: usize = 8;

impl Aim {
    pub fn index(self) -> usize {
        self as usize
    }

    /// The aim a trainer family gives the army: what a commander above would say for that scene.
    pub fn of(fam: Fam) -> Aim {
        match fam {
            // The history families (Feint .. TwinSiege) and the look families (SurpriseAttack .. NavalApproach): the order is the
            // operation (the siege, a hold, a kill), the home is marked as the refuge; the net must look beyond it.
            Fam::Duel | Fam::Focus | Fam::Kite | Fam::Flank | Fam::General | Fam::Sink | Fam::Airstrike | Fam::Harass | Fam::Choke | Fam::Citadel | Fam::Bait | Fam::NeutralPath | Fam::NeutralJoinsWar | Fam::FogSniper => Aim::Destroy,
            Fam::Hold | Fam::Rush | Fam::Feint | Fam::ScoutReport | Fam::AirThreat | Fam::NavalApproach => Aim::Hold,
            Fam::Siege | Fam::Ram | Fam::Bombard | Fam::Prey | Fam::AssaultHeal | Fam::PromotionChoice | Fam::HillArchers | Fam::CityFire | Fam::PlannedAssault | Fam::AssaultReinforce | Fam::AssaultPeace | Fam::Camp | Fam::CampRace | Fam::AllySiege | Fam::ClosedBorders | Fam::CapitalRace | Fam::CityReinforced | Fam::TwoFronts | Fam::SecondCity | Fam::TwinSiege | Fam::SurpriseAttack | Fam::BlindSector | Fam::SecondFront | Fam::BaitLook => Aim::TakeCity,
            Fam::Guard | Fam::Counter | Fam::DefendFriend => Aim::Protect,
            Fam::Raid | Fam::SpareFriends => Aim::Pillage,
            Fam::Retreat | Fam::Wounded | Fam::Surrounded | Fam::AmbushFog => Aim::Survive,
            Fam::Snipe | Fam::HuntFog => Aim::Catch,
            Fam::Convoy | Fam::LandingEscort => Aim::Escort,
            Fam::NavalDuel | Fam::AirRecon => Aim::Destroy,
            Fam::AirIntercept => Aim::Hold,
        }
    }
}

/// The objective token and the side the net plays: everything the net is told from above.
#[derive(Debug, Clone)]
pub struct Orders {
    pub side: PlayerId,
    pub foe: PlayerId,
    pub aim: Aim,
    /// The objective's tile (the city, the pass, the refuge, the arena's centre).
    pub tile: TileId,
    /// The tile to rest on (our home city), when the scene has one.
    pub refuge: Option<TileId>,
    /// The aim may be declined (an opportunity: holding back is also right).
    pub optional: bool,
    pub start_round: u32,
    pub rounds: u32,
    /// The units the net commands (our military scene pieces).
    pub army: Vec<EntityId>,
    /// The mean base strength of the scene's military units (melee or ranged, whichever is higher):
    /// strengths enter the net relative to it, so every era reads alike.
    pub mean_strength: f32,
    /// The map window of the encoder (variant B), around the arena's centre.
    pub window: MapWindow,
    /// The window's static planes (`[tile][S_CH]`, f16-rounded): the ground and who holds it at the start.
    pub statics: Vec<f32>,
    /// The window's relation planes (`[tile][S_REL]`, `statics_rel_of`): the third parties' land by relation at the start.
    pub statics_rel: Vec<f32>,
    /// The memory families (`trainers::memory`): the tokens hold only the foes our side sees.
    pub fog: bool,
    /// The stake (`price_of`): the capture's worth in units / 8, the time pressure, "given".
    pub price: [f32; 3],
}

/// The units each win rule lets us lose (`trainers/families.rs`, `assault.rs`); None: half the army; a family not
/// listed sets no cap (the whole army). The same table as `army/potential.py::CAP`.
fn loss_cap(family: &str) -> Option<Option<usize>> {
    match family {
        "assault-heal" | "promotion-choice" | "choke" | "citadel" => Some(None),
        "hill-archers" | "city-fire" | "planned-assault" | "focus" | "bait" | "raid" | "flank" | "wounded" | "harass" => Some(Some(1)),
        "kite" => Some(Some(0)),
        "camp" => Some(Some(1)),
        "camp-race" => Some(Some(2)),
        "neutral-path" | "spare-friends" | "neutral-joins-war" | "ally-siege" => Some(Some(1)),
        "defend-friend" | "closed-borders" => Some(Some(2)),
        "capital-race" => Some(Some(5)),
        _ => None,
    }
}

/// `ARMY_PRICE`: the stake given to the net instead of the scene's (the conditional test P3 and the paired data teach7p:
/// the same scenes told "careful" or "any cost"). `price_set` reads it (None: the scene's own price).
fn price_override() -> Option<String> {
    std::env::var("ARMY_PRICE").ok().filter(|v| !v.trim().is_empty())
}

/// The stake `ARMY_PRICE` sets for a scene of `family` (`units` ours, `rounds` its limit, `deadline` its own), or None
/// (no such form). Forms:
/// - `worth/8,pressure,given`: these three numbers, every scene alike;
/// - `careful`: worth = the loss cap (half the army where the rule says half; ONE unit where the rule sets no cap),
///   pressure = 1 / the round limit;
/// - `anycost` / `anycost:D`: worth = the whole army, pressure = 1 / D (no D: the scene's deadline, else its limit).
pub fn price_set(form: &str, family: &str, units: usize, rounds: u32, deadline: Option<u32>) -> Option<[f32; 3]> {
    let form = form.trim();
    let press = |d: u32| 1.0 / d.max(1) as f32;
    if form == "careful" {
        let worth = match loss_cap(family) {
            Some(None) => units / 2,
            Some(Some(cap)) => cap,
            None => 1,
        };
        return Some([(worth as f32).max(0.5) / 8.0, press(rounds), 1.0]);
    }
    if let Some(rest) = form.strip_prefix("anycost") {
        let d = match rest.strip_prefix(':') {
            Some(d) => d.trim().parse().ok()?,
            None if rest.is_empty() => deadline.unwrap_or(rounds),
            None => return None,
        };
        return Some([(units as f32).max(0.5) / 8.0, press(d), 1.0]);
    }
    let x: Vec<f32> = form.split(',').filter_map(|p| p.trim().parse().ok()).collect();
    <[f32; 3]>::try_from(x.as_slice()).ok()
}

/// The play's stake: [the capture's worth in units / 8 (the loss cap; the whole army without one), the time
/// pressure (1 / the round that matters: the deadline of the "at any cost" families, else the scene's round limit;
/// None: 0, a recording older than the field), 1]. `army/potential.py::price_of` reads it from the recording.
pub fn price_of(family: &str, units: usize, deadline: Option<u32>) -> [f32; 3] {
    let worth = match loss_cap(family) {
        Some(None) => units / 2,
        Some(Some(cap)) => cap,
        None => units,
    };
    let pressure = deadline.map_or(0.0, |d| 1.0 / d.max(1) as f32);
    [(worth as f32).max(0.5) / 8.0, pressure, 1.0]
}

/// The side of the map window (tiles).
pub const W: usize = 24;
/// Static planes of a window tile (`statics_of`).
pub const S_CH: usize = 16;
/// Relation planes of a window tile (`statics_rel_of`; typed tokens, step 1.5).
pub const S_REL: usize = 6;

/// A 24 x 24 window of the map in its axial coordinates (column q, row r), clamped to the map.
#[derive(Debug, Clone, Copy)]
pub struct MapWindow {
    pub q0: i32,
    pub r0: i32,
    pub map_w: i32,
    pub map_h: i32,
}

impl MapWindow {
    pub fn around(game: &GameState, centre: TileId) -> MapWindow {
        let dims = game.view().auth.map().dimensions;
        let (mw, mh) = (dims.width as i32, dims.height as i32);
        let (cq, cr) = (centre.0 as i32 % mw, centre.0 as i32 / mw);
        let w = W as i32;
        MapWindow { q0: (cq - w / 2).clamp(0, (mw - w).max(0)), r0: (cr - w / 2).clamp(0, (mh - w).max(0)), map_w: mw, map_h: mh }
    }

    /// The window index (`y * W + x`) of a map tile; None outside.
    pub fn index(&self, t: TileId) -> Option<usize> {
        let (q, r) = (t.0 as i32 % self.map_w - self.q0, t.0 as i32 / self.map_w - self.r0);
        let w = W as i32;
        (q >= 0 && r >= 0 && q < w && r < w).then_some((r * w + q) as usize)
    }

    /// The map tile at window index `i`; None off the map.
    pub fn tile(&self, i: usize) -> Option<TileId> {
        let (q, r) = (self.q0 + (i % W) as i32, self.r0 + (i / W) as i32);
        (q < self.map_w && r < self.map_h).then(|| TileId((r * self.map_w + q) as u32))
    }
}

/// The static planes of the window: on the map, hills, mountain, water, rough, defence (ours), a river on
/// a side, road, owned by us, owned by the foe, an improvement, a citadel's damage, the objective, the
/// refuge, our city, the foe's city.
pub fn statics_of(rules: &CompiledRuleset, game: &GameState, win: &MapWindow, side: PlayerId, foe: PlayerId, objective: TileId, refuge: Option<TileId>) -> Vec<f32> {
    use kernel::navigation::terrain::{is_hills, is_mountain, is_rough};
    let auth = game.view().auth;
    let map = auth.map();
    let mut v = vec![0f32; W * W * S_CH];
    for i in 0..W * W {
        let Some(t) = win.tile(i) else { continue };
        let f = &mut v[i * S_CH..(i + 1) * S_CH];
        let owner = map.owner(t).flatten();
        let city = auth.indexes().city_on_tile(t).and_then(|c| auth.cities().get(c)).map(|c| c.owner);
        let citadel = map.improvement(t).flatten().filter(|_| !map.pillaged(t)).and_then(|x| rules.improvements.get(x.0 as usize)).is_some_and(|d| d.nearby_enemy_damage > 0);
        let river = map.dimensions.neighbors(t).into_iter().any(|n| map.river_between(t, n));
        let flags = [true, is_hills(map, t), is_mountain(map, t), map.is_water(t), is_rough(rules, map, t)];
        for (k, b) in flags.iter().enumerate() {
            f[k] = f32::from(u8::from(*b));
        }
        f[5] = observe::f16_round(act::defence(game, rules, t, side) as f32 / 100.0);
        f[6] = f32::from(u8::from(river));
        f[7] = f32::from(u8::from(map.road(t)));
        f[8] = f32::from(u8::from(owner == Some(side)));
        f[9] = f32::from(u8::from(owner == Some(foe)));
        f[10] = f32::from(u8::from(map.working_improvement(t).is_some()));
        f[11] = f32::from(u8::from(citadel));
        f[12] = f32::from(u8::from(t == objective));
        f[13] = f32::from(u8::from(Some(t) == refuge));
        f[14] = f32::from(u8::from(city == Some(side)));
        f[15] = f32::from(u8::from(city == Some(foe)));
    }
    v
}

/// The relation planes of the window (typed tokens, step 1.5): the land of a third party (a player other than `side` and
/// `foe`) by its relation to `side` (`observe::relation`): an ally, a nation at peace, at war, a city-state, the
/// barbarians; and a third party's city. The planes "ours" and "the foe's" stay in `statics_of` (8, 9, 14, 15). The
/// relations are the start's, as the ownership planes; a declaration of war later shows in the tokens' relation.
pub fn statics_rel_of(rules: &CompiledRuleset, game: &GameState, win: &MapWindow, side: PlayerId, foe: PlayerId) -> Vec<f32> {
    let auth = game.view().auth;
    let map = auth.map();
    let mut v = vec![0f32; W * W * S_REL];
    for i in 0..W * W {
        let Some(t) = win.tile(i) else { continue };
        let f = &mut v[i * S_REL..(i + 1) * S_REL];
        if let Some(owner) = map.owner(t).flatten().filter(|o| *o != side && *o != foe) {
            let r = observe::relation(game, rules, side, foe, owner);
            let k = match r {
                observe::R_ALLY => Some(0),
                observe::R_PEACE => Some(1),
                observe::R_WAR => Some(2),
                observe::R_CS => Some(3),
                observe::R_BARB => Some(4),
                _ => None,
            };
            if let Some(k) = k {
                f[k] = 1.0;
            }
        }
        let city = auth.indexes().city_on_tile(t).and_then(|c| auth.cities().get(c)).map(|c| c.owner);
        f[5] = f32::from(u8::from(city.is_some_and(|o| o != side && o != foe)));
    }
    v
}

impl Orders {
    /// The orders of the learner's side in a trainer scene.
    pub fn of_scene(rules: &CompiledRuleset, game: &GameState, scene: &Scene) -> Orders {
        let aim = Aim::of(scene.fam);
        let tile = match aim {
            Aim::Survive => scene.refuge.or(scene.hold).unwrap_or(scene.centre),
            Aim::Destroy | Aim::Protect => scene.hold.unwrap_or(scene.centre),
            _ => scene.target.or(scene.hold).or(scene.refuge).unwrap_or(scene.centre),
        };
        let army: Vec<EntityId> = scene.mine.iter().filter(|p| in_army(rules, p.kind)).map(|p| p.id).collect();
        let strengths: Vec<f32> = scene.mine.iter().chain(&scene.theirs).filter(|p| !act::is_civilian(rules, p.kind)).map(|p| base_strength(rules, p.kind)).collect();
        let mean_strength = if strengths.is_empty() { 1.0 } else { strengths.iter().sum::<f32>() / strengths.len() as f32 };
        let optional = scene.fam == Fam::Prey && scene.info.get("expected").is_some_and(|e| e == "either");
        let start_round = game.view().auth.turn().round;
        let window = MapWindow::around(game, scene.centre);
        let statics = statics_of(rules, game, &window, scene.learner, scene.enemy, tile, scene.refuge);
        let statics_rel = statics_rel_of(rules, game, &window, scene.learner, scene.enemy);
        Orders { side: scene.learner, foe: scene.enemy, aim, tile, refuge: scene.refuge, optional, start_round, rounds: scene.rounds, army, mean_strength: mean_strength.max(1.0), window, statics, statics_rel, fog: crate::trainers::memory::is_memory(scene.fam) || scene.fam == Fam::AirRecon || crate::trainers::history_families::is_fog(scene.fam), price: price_override().map(|f| price_set(&f, scene.fam.name(), scene.mine.len(), scene.rounds, scene.deadline).unwrap_or_else(|| panic!("ARMY_PRICE={f}: not a price form (a,b,c | careful | anycost[:D])"))).unwrap_or_else(|| price_of(scene.fam.name(), scene.mine.len(), Some(scene.deadline.unwrap_or(scene.rounds)))) }
    }
}

/// A unit the net commands: a military kind, or a Great General (no strength, but a piece of the army).
pub fn in_army(rules: &CompiledRuleset, kind: classic_rules::ids::UnitKindId) -> bool {
    !act::is_civilian(rules, kind) || crate::trainers::lab::arms_of(rules, kind).contains(&crate::trainers::lab::Arm::General)
}

/// The higher of a kind's melee and ranged base strength.
pub fn base_strength(rules: &CompiledRuleset, kind: classic_rules::ids::UnitKindId) -> f32 {
    let d = &rules.units[kind.0 as usize];
    f32::from(d.strength.max(d.ranged_strength))
}
