//! Effect conditions over a city's territory, as the player knows it: the
//! AI's counterpart of the engine's `kernel::effects::condition_holds` for
//! the tile and city conditions of the rules' effects, read from tile facts
//! (`PlayerView` as `TileFacts`, or facts built on it) instead of the
//! authoritative world.
//!
//! ```text
//! tile conditions   TileResource, TileResourceClass, TileFeature,
//!                   TileFeatureUnimproved, TileTerrain, TileImprovement,
//!                   TilePlot: the tile's known snapshot (a resource only
//!                   once its reveal technology is known, as the view shows)
//! city conditions   CityCoastal, CityOnRiver, CityAdjacentUnimprovedFeature:
//!                   the centre's snapshot and its neighbours
//! city state        MinPopulation, HasBuilding: the context's city (a planned
//!                   city states its own)
//! owner             Always, HasTechnology, LacksTechnology: the context's
//!                   technologies
//! not decided       everything else (None): unit situations, events, the
//!                   rules module's words, peace, garrisons, the queue head
//! ```
//!
//! Nothing here reads the truth: what the player does not know does not
//! match (an unknown tile meets no tile condition).

use crate::ids::{BuildingKindId, PlayerId, TechId, TileId};
use crate::knowledge::view::PlayerView;
use crate::math::hex_disc;
use crate::rules::content::Yields;
use crate::rules::effects::{CompiledEffect, Condition, EffectSet, Modifier, PlotFilter, Selector};
use crate::rules::CompiledRuleset;
use crate::world::facts::TileFacts;

/// Where conditions are evaluated: the owner's knowledge and the city the
/// tiles belong to (an existing city or a planned one).
#[derive(Debug, Clone, Copy)]
pub struct TerritoryContext<'c> {
    pub rules: &'c CompiledRuleset,
    pub player: PlayerId,
    /// The owner's technologies (the observation's).
    pub techs: &'c [TechId],
    /// The city's centre: city conditions read it.
    pub centre: TileId,
    /// The city's citizens (a planned city: what it is assumed to reach).
    pub population: u32,
    /// The city's buildings (a planned city: what it is assumed to have).
    pub buildings: &'c [BuildingKindId],
}

/// Does `condition` hold on `tile` of the context's city? None: the
/// condition is not one of the tile, city or owner conditions this module
/// decides (see the module docs). A city condition reads the centre
/// whatever the tile, as the engine reads the working city.
pub fn tile_meets(facts: &impl TileFacts, tile: TileId, condition: Condition, ctx: &TerritoryContext<'_>) -> Option<bool> {
    let rules = ctx.rules;
    let met = match condition {
        Condition::Always => true,
        Condition::HasTechnology(tech) => ctx.techs.contains(&tech),
        Condition::LacksTechnology(tech) => !ctx.techs.contains(&tech),
        Condition::MinPopulation(n) => ctx.population >= n,
        Condition::HasBuilding(building) => ctx.buildings.contains(&building),
        // The engine reads the working improvement: a pillaged one does not count.
        Condition::TileImprovement(improvement) => {
            facts.improvement(tile) == Some(improvement) && !facts.pillaged(tile)
        }
        Condition::TileResource(resource) => facts.resource(tile) == Some(resource),
        Condition::TileFeature(feature) => facts.feature(tile) == Some(feature),
        Condition::TileTerrain(kind) => {
            let code = rules.terrains.get(kind.0 as usize).and_then(|t| t.map_code);
            code.is_some() && facts.terrain(tile).map(|t| t.0) == code
        }
        Condition::TilePlot(plot) => plot_matches(facts, rules, tile, plot),
        Condition::TileResourceClass(class) => resource_class(facts, rules, tile) == Some(class),
        Condition::TileFeatureUnimproved(feature) => {
            facts.feature(tile) == Some(feature) && facts.improvement(tile).is_none()
        }
        Condition::CityAdjacentUnimprovedFeature { feature, at_least } => {
            let around = facts.dimensions().neighbors(ctx.centre);
            let count = around
                .into_iter()
                .filter(|n| facts.feature(*n) == Some(feature) && facts.improvement(*n).is_none())
                .count();
            count >= usize::from(at_least)
        }
        Condition::CityCoastal => facts.coastal_land(ctx.centre),
        Condition::CityOnRiver => facts.river(ctx.centre),
        _ => return None,
    };
    Some(met)
}

/// Both gates of an effect on `tile`: false as soon as one is known false,
/// None when one is undecided and none is false.
pub fn effect_meets(facts: &impl TileFacts, tile: TileId, effect: &CompiledEffect, ctx: &TerritoryContext<'_>) -> Option<bool> {
    let first = tile_meets(facts, tile, effect.condition, ctx);
    let second = tile_meets(facts, tile, effect.second_condition, ctx);
    match (first, second) {
        (Some(false), _) | (_, Some(false)) => Some(false),
        (Some(true), Some(true)) => Some(true),
        _ => None,
    }
}

/// The engine's plot rule (`kernel::effects::plot_matches`) on known facts.
fn plot_matches(facts: &impl TileFacts, rules: &CompiledRuleset, tile: TileId, plot: PlotFilter) -> bool {
    match plot {
        PlotFilter::River => facts.river(tile),
        PlotFilter::Sea => facts.is_water(tile) && !facts.lake(tile),
        PlotFilter::Lake => facts.lake(tile),
        PlotFilter::SeaResource => facts.is_water(tile) && resource_class(facts, rules, tile).is_some(),
        PlotFilter::NaturalWonder => facts.natural_wonder(tile).is_some(),
    }
}

/// The class of the resource the facts show on `tile` (the view shows only
/// revealed ones, which is the engine's "the owner sees it").
fn resource_class(facts: &impl TileFacts, rules: &CompiledRuleset, tile: TileId) -> Option<crate::rules::content::ResourceClass> {
    let resource = facts.resource(tile)?;
    rules.resources.get(resource.0 as usize)?.class
}

/// The tiles a city at `centre` would work: within `radius`, known to the
/// player, and owned by the player or by nobody. Tiles of other players are
/// left out (the city cannot work them unless borders change), and so are
/// unknown tiles (nothing can be said about them). In tile order of the
/// hex disc, the centre included.
pub fn territory(view: &PlayerView<'_>, centre: TileId, radius: i32, player: PlayerId) -> Vec<TileId> {
    hex_disc(view.dimensions(), centre, radius)
        .filter(|tile| {
            view.tile(*tile)
                .snapshot()
                .is_some_and(|s| s.owner.is_none_or(|owner| owner == player))
        })
        .collect()
}

/// One yield effect of a building and how much of the territory meets it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectCount {
    pub effect: CompiledEffect,
    /// Tiles that meet both conditions (a city effect: 1 when the city
    /// does, else 0).
    pub met: u32,
    /// Tiles (or the city) whose answer is not decided here (`tile_meets`
    /// None); they add nothing.
    pub undecided: u32,
}

/// What a source's passive flat yield effects add over a territory.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TerritoryBonus {
    /// Its Tile-scope flat yields, each with the tiles that meet it.
    pub tile_effects: Vec<EffectCount>,
    /// Its City-scope flat yields that reach the city, each with whether
    /// the city meets it.
    pub city_effects: Vec<EffectCount>,
    /// The yields the tile effects add, summed over the tiles that meet them.
    pub tiles: Yields,
    /// The yields the met city effects add.
    pub city: Yields,
}

impl TerritoryBonus {
    /// Both sums.
    pub fn total(&self) -> Yields {
        self.tiles.plus(self.city)
    }

    /// Adds another source's bonus (the effect lists too).
    pub fn add(&mut self, other: TerritoryBonus) {
        self.tile_effects.extend(other.tile_effects);
        self.city_effects.extend(other.city_effects);
        self.tiles = self.tiles.plus(other.tiles);
        self.city = self.city.plus(other.city);
    }
}

/// What the passive flat yield effects of `building` add over `territory`
/// (`effect_set_bonus` of its effects; nothing for an unknown building).
pub fn building_tile_bonus(
    facts: &impl TileFacts,
    rules: &CompiledRuleset,
    building: BuildingKindId,
    territory: &[TileId],
    ctx: &TerritoryContext<'_>,
) -> TerritoryBonus {
    match rules.buildings.get(building.0 as usize) {
        Some(def) => effect_set_bonus(facts, &def.effects, territory, ctx),
        None => TerritoryBonus::default(),
    }
}

/// For every passive flat yield effect (`Modifier::AddYield`) of a source
/// whose effects reach the context's city (a building of it, a policy or
/// technology of its owner, a belief it follows: the caller's word): each
/// Tile-scope one with the territory tiles that meet its conditions and the
/// yield it adds over them; each City-scope one with whether the city meets
/// it, but for the capital's and an event's (a city is not assumed to be
/// the capital). Percent stages are not counted: their base is the rest of
/// the city's yield.
pub fn effect_set_bonus(
    facts: &impl TileFacts,
    effects: &EffectSet,
    territory: &[TileId],
    ctx: &TerritoryContext<'_>,
) -> TerritoryBonus {
    let mut bonus = TerritoryBonus::default();
    for effect in &effects.passive_tile {
        let Modifier::AddYield { yield_kind, amount } = effect.modifier else {
            continue;
        };
        let mut count = EffectCount { effect: *effect, met: 0, undecided: 0 };
        for tile in territory {
            match effect_meets(facts, *tile, effect, ctx) {
                Some(true) => count.met += 1,
                Some(false) => {}
                None => count.undecided += 1,
            }
        }
        bonus.tiles.add(yield_kind, amount * count.met as i32);
        bonus.tile_effects.push(count);
    }
    for effect in &effects.passive_city {
        let Modifier::AddYield { yield_kind, amount } = effect.modifier else {
            continue;
        };
        if matches!(effect.selector, Selector::Capital | Selector::EventCity) {
            continue;
        }
        let mut count = EffectCount { effect: *effect, met: 0, undecided: 0 };
        match effect_meets(facts, ctx.centre, effect, ctx) {
            Some(true) => count.met = 1,
            Some(false) => {}
            None => count.undecided = 1,
        }
        bonus.city.add(yield_kind, amount * count.met as i32);
        bonus.city_effects.push(count);
    }
    bonus
}
