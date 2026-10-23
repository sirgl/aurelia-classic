//! RON mirror of the role bindings (`classic_rules::rules::roles`): which row of
//! the content plays each role the kernel names.

use classic_rules::rules::roles::{ClassicSourceRoles, Climate, FeatureRole, Ground, IdeologyCase, ProcessRole, SourceRoles, StartingUnit};
use serde::Deserialize;

use crate::ron_types::{key, keys, opt_key, RonGreatPersonKind};

#[derive(Debug, Clone, Copy, Deserialize)]
pub(crate) enum RonClimate {
    Humid,
    Temperate,
    Arid,
    Cold,
    Frozen,
}

impl From<RonClimate> for Climate {
    fn from(value: RonClimate) -> Self {
        match value {
            RonClimate::Humid => Climate::Humid,
            RonClimate::Temperate => Climate::Temperate,
            RonClimate::Arid => Climate::Arid,
            RonClimate::Cold => Climate::Cold,
            RonClimate::Frozen => Climate::Frozen,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub(crate) enum RonGround {
    Water,
    Mountains,
    Hills,
    Flat(RonClimate),
}

impl From<RonGround> for Ground {
    fn from(value: RonGround) -> Self {
        match value {
            RonGround::Water => Ground::Water,
            RonGround::Mountains => Ground::Mountains,
            RonGround::Hills => Ground::Hills,
            RonGround::Flat(climate) => Ground::Flat(climate.into()),
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub(crate) enum RonFeatureRole {
    Forest,
    Jungle,
    Marsh,
    Oasis,
    FloodPlains,
    Ice,
    Atoll,
    Fallout,
}

impl From<RonFeatureRole> for FeatureRole {
    fn from(value: RonFeatureRole) -> Self {
        match value {
            RonFeatureRole::Forest => FeatureRole::Forest,
            RonFeatureRole::Jungle => FeatureRole::Jungle,
            RonFeatureRole::Marsh => FeatureRole::Marsh,
            RonFeatureRole::Oasis => FeatureRole::Oasis,
            RonFeatureRole::FloodPlains => FeatureRole::FloodPlains,
            RonFeatureRole::Ice => FeatureRole::Ice,
            RonFeatureRole::Atoll => FeatureRole::Atoll,
            RonFeatureRole::Fallout => FeatureRole::Fallout,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub(crate) enum RonStartingUnit {
    Defense,
    Worker,
    Explore,
}

impl From<RonStartingUnit> for StartingUnit {
    fn from(value: RonStartingUnit) -> Self {
        match value {
            RonStartingUnit::Defense => StartingUnit::Defense,
            RonStartingUnit::Worker => StartingUnit::Worker,
            RonStartingUnit::Explore => StartingUnit::Explore,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub(crate) enum RonProcessRole {
    Wealth,
    Research,
}

impl From<RonProcessRole> for ProcessRole {
    fn from(value: RonProcessRole) -> Self {
        match value {
            RonProcessRole::Wealth => ProcessRole::Wealth,
            RonProcessRole::Research => ProcessRole::Research,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub(crate) enum RonIdeologyCase {
    Wide,
    AtWar,
    AtPeace,
}

impl From<RonIdeologyCase> for IdeologyCase {
    fn from(value: RonIdeologyCase) -> Self {
        match value {
            RonIdeologyCase::Wide => IdeologyCase::Wide,
            RonIdeologyCase::AtWar => IdeologyCase::AtWar,
            RonIdeologyCase::AtPeace => IdeologyCase::AtPeace,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RonRoles {
    #[serde(default)]
    grounds: Vec<(RonGround, String)>,
    #[serde(default)]
    deep_water: Option<String>,
    #[serde(default)]
    features: Vec<(RonFeatureRole, String)>,
    #[serde(default)]
    starting_units: Vec<(RonStartingUnit, String)>,
    #[serde(default)]
    processes: Vec<(RonProcessRole, String)>,
    #[serde(default)]
    road: Option<String>,
    #[serde(default)]
    default_specialist: Option<String>,
    #[serde(default)]
    default_difficulty: Option<String>,
    #[serde(default)]
    ai_handicap: Option<String>,
    #[serde(default)]
    ai_branches: Vec<String>,
    #[serde(default)]
    ai_ideologies: Vec<(RonIdeologyCase, String)>,
    #[serde(default)]
    faith_great_people: Vec<(RonGreatPersonKind, String)>,
    #[serde(default)]
    ideology_ties: Vec<(String, String)>,
}

pub(crate) fn roles(r: RonRoles) -> SourceRoles {
    SourceRoles {
        grounds: r.grounds.into_iter().map(|(g, k)| (g.into(), key(k))).collect(),
        deep_water: opt_key(r.deep_water),
        features: r.features.into_iter().map(|(f, k)| (f.into(), key(k))).collect(),
        starting_units: r.starting_units.into_iter().map(|(u, k)| (u.into(), key(k))).collect(),
        processes: r.processes.into_iter().map(|(p, k)| (p.into(), key(k))).collect(),
        road: opt_key(r.road),
        default_specialist: opt_key(r.default_specialist),
        default_difficulty: opt_key(r.default_difficulty),
        ai_handicap: opt_key(r.ai_handicap),
        module: ClassicSourceRoles {
            ai_branches: keys(r.ai_branches),
            ai_ideologies: r.ai_ideologies.into_iter().map(|(c, k)| (c.into(), key(k))).collect(),
            faith_great_people: r.faith_great_people.into_iter().map(|(g, k)| (g.into(), key(k))).collect(),
            ideology_ties: r.ideology_ties.into_iter().map(|(a, b)| (key(a), key(b))).collect(),
        },
    }
}
