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

pub mod model;
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
            Fam::Duel | Fam::Focus | Fam::Kite | Fam::Flank | Fam::General | Fam::Sink | Fam::Airstrike | Fam::Harass | Fam::Choke | Fam::Citadel | Fam::Bait => Aim::Destroy,
            Fam::Hold | Fam::Rush => Aim::Hold,
            Fam::Siege | Fam::Ram | Fam::Bombard | Fam::Prey | Fam::AssaultHeal | Fam::PromotionChoice | Fam::HillArchers | Fam::CityFire | Fam::PlannedAssault => Aim::TakeCity,
            Fam::Guard | Fam::Counter => Aim::Protect,
            Fam::Raid => Aim::Pillage,
            Fam::Retreat | Fam::Wounded | Fam::Surrounded => Aim::Survive,
            Fam::Snipe => Aim::Catch,
            Fam::Convoy => Aim::Escort,
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
}

/// The side of the map window (tiles).
pub const W: usize = 24;
/// Static planes of a window tile (`statics_of`).
pub const S_CH: usize = 16;

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
        Orders { side: scene.learner, foe: scene.enemy, aim, tile, refuge: scene.refuge, optional, start_round, rounds: scene.rounds, army, mean_strength: mean_strength.max(1.0), window, statics }
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
