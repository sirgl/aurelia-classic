//! RON schema of natural wonder placement (mirror of
//! `classic_rules::rules::natural_wonders::SourcePlacement`).
//!
//! ```ron
//! placement: (avoid_ocean_coast: true, core_terrains: ["core:terrain/desert"],
//!   adjacent: [AtMost(Mountain, 2), AtMost(Terrain("core:terrain/grassland"), 0)],
//!   core_shape: Mountain),
//! ```

use classic_rules::rules::natural_wonders::{
    AdjacentBound, AdjacentKind, AdjacentRule, CoreShape, PlacementMethod, SourcePlacement,
};
use classic_rules::rules::source::StableContentKey;
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RonPlacement {
    #[serde(default)]
    method: RonMethod,
    #[serde(default)]
    avoid_ocean_coast: bool,
    #[serde(default)]
    avoid_biggest_landmass: bool,
    #[serde(default)]
    core_terrains: Vec<String>,
    #[serde(default)]
    adjacent: Vec<RonAdjacent>,
    #[serde(default)]
    core_shape: RonShape,
    #[serde(default)]
    core_terrain: Option<String>,
    #[serde(default)]
    surround_with_water: bool,
}

#[derive(Debug, Default, Deserialize)]
enum RonMethod {
    #[default]
    Land,
    Water,
    Reef,
    PeninsulaTip,
}

#[derive(Debug, Default, Deserialize)]
enum RonShape {
    #[default]
    Keep,
    Mountain,
    Flat,
}

#[derive(Debug, Deserialize)]
enum RonKind {
    Hills,
    Mountain,
    HillsOrMountain,
    Land,
    Water,
    Terrain(String),
    Feature(String),
}

/// One rule: `AtLeast(Hills, 1)`, `AtMost(Terrain("core:terrain/grassland"), 0)`.
#[derive(Debug, Deserialize)]
enum RonAdjacent {
    AtLeast(RonKind, u8),
    AtMost(RonKind, u8),
}

fn key(s: String) -> StableContentKey {
    StableContentKey::new(s)
}

impl From<RonKind> for AdjacentKind<StableContentKey, StableContentKey> {
    fn from(kind: RonKind) -> Self {
        match kind {
            RonKind::Hills => AdjacentKind::Hills,
            RonKind::Mountain => AdjacentKind::Mountain,
            RonKind::HillsOrMountain => AdjacentKind::HillsOrMountain,
            RonKind::Land => AdjacentKind::Land,
            RonKind::Water => AdjacentKind::Water,
            RonKind::Terrain(k) => AdjacentKind::Terrain(key(k)),
            RonKind::Feature(k) => AdjacentKind::Feature(key(k)),
        }
    }
}

impl From<RonAdjacent> for AdjacentRule<StableContentKey, StableContentKey> {
    fn from(a: RonAdjacent) -> Self {
        match a {
            RonAdjacent::AtLeast(kind, n) => AdjacentRule { kind: kind.into(), bound: AdjacentBound::AtLeast(n) },
            RonAdjacent::AtMost(kind, n) => AdjacentRule { kind: kind.into(), bound: AdjacentBound::AtMost(n) },
        }
    }
}

impl From<RonPlacement> for SourcePlacement {
    fn from(p: RonPlacement) -> Self {
        SourcePlacement {
            method: match p.method {
                RonMethod::Land => PlacementMethod::Land,
                RonMethod::Water => PlacementMethod::Water,
                RonMethod::Reef => PlacementMethod::Reef,
                RonMethod::PeninsulaTip => PlacementMethod::PeninsulaTip,
            },
            avoid_ocean_coast: p.avoid_ocean_coast,
            avoid_biggest_landmass: p.avoid_biggest_landmass,
            core_terrains: p.core_terrains.into_iter().map(key).collect(),
            adjacent: p.adjacent.into_iter().map(Into::into).collect(),
            core_shape: match p.core_shape {
                RonShape::Keep => CoreShape::Keep,
                RonShape::Mountain => CoreShape::Mountain,
                RonShape::Flat => CoreShape::Flat,
            },
            core_terrain: p.core_terrain.map(key),
            surround_with_water: p.surround_with_water,
        }
    }
}
