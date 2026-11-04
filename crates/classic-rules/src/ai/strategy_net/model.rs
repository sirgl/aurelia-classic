//! The empire as the economy cell runs it (`cell`): each city's tiles with
//! their yields and the rules' static conditions they meet, its citizens,
//! its food store, the yields that do not come from its tiles, its
//! buildings; the empire's happiness, science and culture. Built from the
//! observation in the game (`of_observation`) or from any tile facts and
//! hand-written cities in the scenario tests (`build`).
//!
//! A static condition is one the tiles decide once (a terrain, a feature,
//! a resource, an improvement, a plot, the coast or a river at the centre:
//! `territory::tile_meets`); the cell decides the others (citizens,
//! buildings, technologies) as it runs.

use super::features::{era_of, slots};
use super::weights::cell::{CENTRE_FOOD, CENTRE_PRODUCTION};
use crate::ai::territory::{tile_meets, TerritoryContext};
use crate::derived::tile_yields::{plot_yields, TechSet};
use crate::ids::{BuildingKindId, PlayerId, PolicyId, ResourceId, TechId, TileId};
use crate::knowledge::observation::AIObservation;
use crate::math::hex_disc;
use crate::rules::content::{PolicyBranchId, TerrainKindId};
use crate::rules::effects::{CompiledEffect, Condition, EffectSet};
use crate::rules::CompiledRuleset;
use crate::world::facts::TileFacts;

/// Static conditions the model keeps at most (bits of a tile).
pub const MAX_CONDITIONS: usize = 256;

/// A worked or workable tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TileModel {
    pub tile: TileId,
    /// Food, production, gold, science, culture, faith (whole yields).
    pub yields: [i64; 6],
    /// The static conditions it meets (`Model::conditions` bits).
    pub bits: [u64; 4],
}

impl TileModel {
    pub fn meets(&self, condition: usize) -> bool {
        condition < MAX_CONDITIONS && self.bits[condition / 64] >> (condition % 64) & 1 == 1
    }
}

/// One city.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CityModel {
    pub id: u64,
    pub tile: TileId,
    pub population: i64,
    /// Food in the store.
    pub food: i64,
    pub centre: TileModel,
    /// The tiles it may work, the centre excluded.
    pub tiles: Vec<TileModel>,
    /// Yields a turn that are not its worked tiles' (buildings, specialists,
    /// percents): its income less the tiles it works, at least 0.
    pub extra: [i64; 6],
    pub buildings: Vec<BuildingKindId>,
    pub capital: bool,
    /// Production a turn now.
    pub production: i64,
    /// Terrain kinds in its reach, a mountain next to the centre or in
    /// reach, the centre on hills; resources in reach.
    pub kinds: Vec<TerrainKindId>,
    pub mountain_adjacent: bool,
    pub mountain_near: bool,
    pub hills: bool,
    pub resources: Vec<ResourceId>,
}

/// The empire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Model {
    pub player: PlayerId,
    pub nation: u8,
    pub cities: Vec<CityModel>,
    /// The static conditions, by bit.
    pub conditions: Vec<Condition>,
    pub techs: Vec<TechId>,
    pub policies: Vec<PolicyId>,
    pub branches: Vec<PolicyBranchId>,
    pub built_wonders: Vec<BuildingKindId>,
    /// Excess happiness now; unhappiness a citizen brings (hundredths).
    pub excess: i64,
    pub citizen_unhappiness: i64,
    /// Beakers and culture a turn; the next policy's culture.
    pub science: i64,
    pub culture: i64,
    pub policy_cost: i64,
    /// The rules' science and production cost percent of the game speed.
    pub science_percent: i64,
    pub pace: i64,
    pub at_war: bool,
    pub religion: bool,
    /// The default specialist's yields (an unemployed citizen).
    pub specialist: [i64; 6],
    pub era: usize,
    /// The eras of the rivals met.
    pub rival_eras: Vec<usize>,
    pub city_states: i64,
}

/// A city as the scenario tests and `of_observation` give it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CitySeed {
    pub id: u64,
    pub tile: TileId,
    pub population: u32,
    pub food: u32,
    /// Tiles worked, the centre included.
    pub worked: Vec<TileId>,
    /// Its income a turn by slot.
    pub income: [i64; 6],
    pub buildings: Vec<BuildingKindId>,
    pub capital: bool,
}

/// What every citizen makes by the rules, by slot (`classic::yields`).
pub fn per_citizen() -> [i64; 6] {
    [0, 0, 0, i64::from(crate::classic::yields::SCIENCE_PER_CITIZEN), 0, 0]
}

/// A condition the tiles decide once.
pub fn is_static(condition: Condition) -> bool {
    matches!(
        condition,
        Condition::TileTerrain(_)
            | Condition::TileFeature(_)
            | Condition::TileResource(_)
            | Condition::TileImprovement(_)
            | Condition::TilePlot(_)
            | Condition::TileResourceClass(_)
            | Condition::TileFeatureUnimproved(_)
            | Condition::CityCoastal
            | Condition::CityOnRiver
            | Condition::CityAdjacentUnimprovedFeature { .. }
    )
}

fn effects_of(set: &EffectSet) -> impl Iterator<Item = &CompiledEffect> {
    set.passive_player.iter().chain(&set.passive_city).chain(&set.passive_unit).chain(&set.passive_tile).chain(set.triggered.iter().flatten())
}

/// Every static condition of the rules' buildings, policies and
/// technologies (the conditions the cell may meet), in rules order.
pub fn static_conditions(rules: &CompiledRuleset) -> Vec<Condition> {
    let mut out: Vec<Condition> = Vec::new();
    let sets = rules
        .buildings
        .iter()
        .map(|b| &b.effects)
        .chain(rules.module.policies.iter().map(|p| &p.effects))
        .chain(rules.technologies.iter().map(|t| &t.effects));
    for set in sets {
        for e in effects_of(set) {
            for c in [e.condition, e.second_condition] {
                if is_static(c) && !out.contains(&c) && out.len() < MAX_CONDITIONS {
                    out.push(c);
                }
            }
        }
    }
    out
}

impl Model {
    /// The model of the cities `seeds` on `facts`: each city's tiles within
    /// the work radius that are known and ours or nobody's.
    #[allow(clippy::too_many_arguments)]
    pub fn build(facts: &impl TileFacts, rules: &CompiledRuleset, player: PlayerId, nation: u8, techs: &[TechId], seeds: &[CitySeed], empire: Empire) -> Model {
        let conditions = static_conditions(rules);
        let set = TechSet::of(techs);
        let dims = facts.dimensions();
        let mut cities = Vec::new();
        for seed in seeds {
            let ctx = TerritoryContext { rules, player, techs, centre: seed.tile, population: seed.population, buildings: &seed.buildings };
            let tile = |t: TileId| {
                let mut bits = [0u64; 4];
                for (i, c) in conditions.iter().enumerate() {
                    if tile_meets(facts, t, *c, &ctx) == Some(true) {
                        bits[i / 64] |= 1 << (i % 64);
                    }
                }
                TileModel { tile: t, yields: slots(plot_yields(facts, rules, t, &set)), bits }
            };
            let mut centre = tile(seed.tile);
            centre.yields[0] = centre.yields[0].max(CENTRE_FOOD);
            centre.yields[1] = centre.yields[1].max(CENTRE_PRODUCTION);
            let reach: Vec<TileId> = hex_disc(dims, seed.tile, crate::world::city::CITY_WORK_RADIUS)
                .filter(|t| *t != seed.tile && facts.terrain(*t).is_some() && facts.owner(*t).is_none_or(|o| o == player) && !(facts.city(*t)))
                .collect();
            let tiles: Vec<TileModel> = reach.iter().map(|t| tile(*t)).collect();
            let mut worked = centre.yields;
            for t in tiles.iter().filter(|t| seed.worked.contains(&t.tile)) {
                for (w, y) in worked.iter_mut().zip(t.yields) {
                    *w += y;
                }
            }
            // What every citizen makes by the rules (beakers) is the cell's,
            // by its own citizens.
            let mut extra = [0i64; 6];
            for y in 0..6 {
                extra[y] = (seed.income[y] - worked[y] - i64::from(seed.population) * per_citizen()[y]).max(0);
            }
            let mut kinds: Vec<TerrainKindId> = reach.iter().chain(std::iter::once(&seed.tile)).filter_map(|t| facts.terrain(*t)).filter_map(|c| rules.terrain_kind_of(c)).collect();
            kinds.sort_by_key(|k| k.0);
            kinds.dedup();
            let mountain = |t: TileId| facts.terrain(t).is_some_and(|c| rules.terrain_kinds.is_mountains(c.0));
            let mut resources: Vec<ResourceId> = reach.iter().filter_map(|t| facts.resource(*t)).collect();
            resources.sort_by_key(|r| r.0);
            resources.dedup();
            cities.push(CityModel {
                id: seed.id,
                tile: seed.tile,
                population: i64::from(seed.population.max(1)),
                food: i64::from(seed.food),
                centre,
                tiles,
                extra,
                buildings: seed.buildings.clone(),
                capital: seed.capital,
                production: seed.income[1].max(1),
                kinds,
                mountain_adjacent: dims.neighbors(seed.tile).as_slice().iter().any(|t| mountain(*t)),
                mountain_near: reach.iter().any(|t| mountain(*t)),
                hills: facts.terrain(seed.tile).is_some_and(|c| rules.terrain_kinds.is_hills(c.0)),
                resources,
            });
        }
        let specialist = rules
            .roles
            .default_specialist
            .and_then(|s| rules.specialists.get(usize::from(s.0)))
            .map_or([0, 1, 0, 0, 0, 0], |d| slots(d.yields));
        Model {
            player,
            nation,
            cities,
            conditions,
            techs: techs.to_vec(),
            policies: empire.policies,
            branches: empire.branches,
            built_wonders: empire.built_wonders,
            excess: empire.excess,
            citizen_unhappiness: empire.citizen_unhappiness.max(1),
            science: empire.science.max(1),
            culture: empire.culture.max(1),
            policy_cost: empire.policy_cost.max(1),
            science_percent: empire.science_percent.max(1),
            pace: empire.pace.max(1),
            at_war: empire.at_war,
            religion: empire.religion,
            specialist,
            era: era_of(rules, techs),
            rival_eras: empire.rival_eras,
            city_states: empire.city_states,
        }
    }

    /// The model of the observed empire.
    pub fn of_observation(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Model {
        let capital = obs.view.own_capital();
        let seeds: Vec<CitySeed> = obs
            .own_cities
            .iter()
            .map(|c| CitySeed {
                id: c.id.0,
                tile: c.tile,
                population: c.population,
                food: c.food,
                worked: c.worked.clone(),
                income: [
                    i64::from(c.income.food),
                    i64::from(c.income.production),
                    i64::from(c.income.gold),
                    i64::from(c.income.science),
                    i64::from(c.income.culture),
                    i64::from(c.income.faith),
                ],
                buildings: c.buildings.clone(),
                capital: Some(c.handle) == capital,
            })
            .collect();
        use crate::knowledge::public::PlayerQueries;
        let empire = Empire {
            policies: obs.culture.policies.clone(),
            branches: obs.culture.branches.clone(),
            built_wonders: obs.built_wonders.clone(),
            excess: i64::from(obs.economy.happiness.excess()),
            citizen_unhappiness: crate::ai::value::Unhappiness::of(obs).citizen,
            science: i64::from(obs.economy.income.science.max(0)),
            culture: i64::from(obs.culture.per_turn),
            policy_cost: i64::from(obs.culture.next_policy_cost),
            science_percent: i64::from(obs.speed.science_percent),
            pace: i64::from(obs.speed.pace_percent),
            at_war: obs.relations.iter().any(|r| r.at_war && r.kind == crate::world::player::PlayerKind::Major),
            religion: obs.faith.overview.religion.is_some(),
            rival_eras: obs.view.met_eras().iter().map(|(_, e)| usize::from(*e)).collect(),
            city_states: obs.city_states.len() as i64,
        };
        Model::build(&obs.view, rules, obs.player, obs.economy.nation, &obs.economy.techs, &seeds, empire)
    }

    /// The capital's index (else the first city's).
    pub fn capital(&self) -> usize {
        self.cities.iter().position(|c| c.capital).unwrap_or(0)
    }

    /// The bit of a static condition (None: not static, or not kept).
    pub fn bit(&self, condition: Condition) -> Option<usize> {
        self.conditions.iter().position(|c| *c == condition)
    }

    pub fn citizens(&self) -> i64 {
        self.cities.iter().map(|c| c.population).sum()
    }

    /// Whether city `i` meets a building's placement (the rules' territory
    /// and site rules, on the model's tiles).
    pub fn placement_ok(&self, rules: &CompiledRuleset, i: usize, building: BuildingKindId) -> bool {
        let Some(def) = rules.buildings.get(usize::from(building.0)) else { return false };
        let Some(city) = self.cities.get(i) else { return false };
        let p = &def.placement;
        if p.capital && !city.capital {
            return false;
        }
        if p.nearby_terrain.is_some_and(|k| !city.kinds.contains(&k)) {
            return false;
        }
        if p.nearby_mountain && !city.mountain_near || p.mountain && !city.mountain_adjacent {
            return false;
        }
        if p.flat && city.hills {
            return false;
        }
        let coastal = self.bit(Condition::CityCoastal).is_some_and(|b| city.centre.meets(b));
        let river = self.bit(Condition::CityOnRiver).is_some_and(|b| city.centre.meets(b));
        if p.water && !coastal || p.river && !river {
            return false;
        }
        if !p.local_resources_any.is_empty() && !p.local_resources_any.iter().any(|r| city.resources.contains(r)) {
            return false;
        }
        p.local_resources_all.iter().all(|r| city.resources.contains(r))
    }
}

/// The empire-wide facts of a model.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Empire {
    pub policies: Vec<PolicyId>,
    pub branches: Vec<PolicyBranchId>,
    pub built_wonders: Vec<BuildingKindId>,
    pub excess: i64,
    pub citizen_unhappiness: i64,
    pub science: i64,
    pub culture: i64,
    pub policy_cost: i64,
    pub science_percent: i64,
    pub pace: i64,
    pub at_war: bool,
    pub religion: bool,
    pub rival_eras: Vec<usize>,
    pub city_states: i64,
}
