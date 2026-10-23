//! Test content fixtures: RON strings through generic ron/serde.
//! There are no handwritten Source* tables in tests: dependencies and prices are data,
//! as in the game. The schema is a local minimum (only the needed fields, everything else
//! #[serde(default)]): the evolution of the real schema does not break the fixture.
//! For tests only (cfg). LIB does not depend on content (the dev cycle gives two
//! copies of the crate; verified) and does not drag the parser into prod.

use crate::rules::effects::{Duration, Scope, Selector, Stacking, Trigger};
use crate::rules::source::{
    SourceBuilding, SourceCondition, SourceEffect, SourceFeature, SourceImprovement,
    SourceModifier, SourcePromotion, SourceResource, SourceRules, SourceTechnology, SourceUnit,
    StableContentKey,
};
use crate::rules::{compile_rules, CompiledRuleset};
use serde::Deserialize;

/// Same RON shape as classic-content's effect schema (defaults: plain passive).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FxEffect {
    scope: Scope,
    selector: Selector,
    #[serde(default = "always")]
    condition: SourceCondition,
    #[serde(default = "always")]
    second_condition: SourceCondition,
    #[serde(default = "passive")]
    trigger: Trigger,
    modifier: SourceModifier,
    #[serde(default = "while_source_exists")]
    duration: Duration,
    #[serde(default = "stack")]
    stacking: Stacking,
}

fn always() -> SourceCondition {
    SourceCondition::Always
}

fn passive() -> Trigger {
    Trigger::Passive
}

fn while_source_exists() -> Duration {
    Duration::WhileSourceExists
}

fn stack() -> Stacking {
    Stacking::Stack
}

fn effects(src: Vec<FxEffect>) -> Vec<SourceEffect> {
    src.into_iter()
        .map(|e| SourceEffect {
            scope: e.scope,
            selector: e.selector,
            condition: e.condition,
            second_condition: e.second_condition,
            trigger: e.trigger,
            modifier: e.modifier,
            duration: e.duration,
            stacking: e.stacking,
        })
        .collect()
}

#[derive(Debug, Deserialize)]
struct FxPromotion {
    id: String,
    #[serde(default)]
    effects: Vec<FxEffect>,
}

#[derive(Debug, Deserialize)]
struct FxUnit {
    id: String,
    movement: u16,
    strength: u16,
    #[serde(default)]
    founder: bool,
    #[serde(default)]
    builder: bool,
    cost: u32,
    #[serde(default)]
    effects: Vec<FxEffect>,
    required_tech: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FxBuilding {
    id: String,
    cost: u32,
    #[serde(default)]
    effects: Vec<FxEffect>,
    #[serde(default)]
    required_tech: Option<String>,
    #[serde(default)]
    wonder: bool,
}

#[derive(Debug, Deserialize)]
struct FxTech {
    id: String,
    #[serde(default)]
    unlocks_buildings: Vec<String>,
    cost: u32,
    #[serde(default)]
    requires: Vec<String>,
    #[serde(default)]
    effects: Vec<FxEffect>,
}

#[derive(Debug, Deserialize)]
struct FxImp {
    id: String,
    work_turns: u16,
    #[serde(default)]
    food_bonus: i32,
    #[serde(default)]
    prod_bonus: i32,
    #[serde(default)]
    river_food: i32,
    #[serde(default)]
    required_tech: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FxFeat {
    id: String,
    #[serde(default)]
    chop_turns: u16,
    #[serde(default)]
    prod_bonus: i32,
    #[serde(default)]
    chop_grant: u32,
    #[serde(default)]
    movement_cost: u8,
}

#[derive(Debug, Deserialize)]
struct FxRes {
    id: String,
    #[serde(default)]
    food_bonus: i32,
    #[serde(default)]
    prod_bonus: i32,
    #[serde(default)]
    gold_bonus: u32,
    #[serde(default)]
    requires_improvement: Option<String>,
    #[serde(default)]
    required_tech: Option<String>,
    #[serde(default)]
    terrains: Vec<u16>,
    #[serde(default)]
    density: u8,
}

#[derive(Debug, Deserialize)]
struct FxRules {
    #[serde(default)]
    units: Vec<FxUnit>,
    #[serde(default)]
    buildings: Vec<FxBuilding>,
    #[serde(default)]
    technologies: Vec<FxTech>,
    #[serde(default)]
    improvements: Vec<FxImp>,
    #[serde(default)]
    features: Vec<FxFeat>,
    #[serde(default)]
    resources: Vec<FxRes>,
    #[serde(default)]
    promotions: Vec<FxPromotion>,
    #[serde(default)]
    beliefs: Vec<FxBelief>,
}

#[derive(Debug, Deserialize)]
struct FxBelief {
    id: String,
    #[serde(default)]
    effects: Vec<FxEffect>,
}

fn key(s: String) -> StableContentKey {
    StableContentKey::new(s)
}

/// The terrain table every fixture gets: yields of the map codes (own values,
/// unverified; plains 1F 1P, grassland 2F, hills 0F 2P, desert 0, coast 1F, snow 0,
/// mountain 0, tundra 1F), on the legacy palette.
pub(crate) fn fixture_terrains() -> Vec<crate::rules::source::SourceTerrain> {
    use crate::rules::content::Yields;
    let terrain = |slug: &str, code: u16, food: i32, production: i32, water: bool| {
        crate::rules::source::SourceTerrain {
            id: Some(key(format!("core:terrain/{slug}"))),
            name: slug.to_string(),
            yields: Yields {
                food,
                production,
                ..Yields::default()
            },
            movement_cost: 1,
            water,
            can_found_city: !water,
            map_code: Some(code),
            ..Default::default()
        }
    };
    vec![
        terrain("plains", 0, 1, 1, false),
        terrain("grassland", 1, 2, 0, false),
        terrain("hill", 2, 0, 2, false),
        terrain("desert", 3, 0, 0, false),
        terrain("coast", 4, 1, 0, true),
        terrain("snow", 5, 0, 0, false),
        terrain("mountain", 6, 0, 0, false),
        terrain("tundra", 7, 1, 0, false),
    ]
}

/// Map codes of the fixture palette (`TerrainPalette::LEGACY`), for tests
/// that paint terrain.
pub(crate) const WATER: crate::ids::TerrainId = legacy(crate::rules::roles::Ground::Water);
pub(crate) const HILLS: crate::ids::TerrainId = legacy(crate::rules::roles::Ground::Hills);
pub(crate) const MOUNTAINS: crate::ids::TerrainId = legacy(crate::rules::roles::Ground::Mountains);

const fn legacy(ground: crate::rules::roles::Ground) -> crate::ids::TerrainId {
    crate::ids::TerrainId(crate::rules::roles::TerrainPalette::LEGACY.code(ground))
}

/// The grounds the fixture terrains play and the roles of the rows a
/// fixture names (`rules::roles`): its forest, warrior and worker.
pub(crate) fn fixture_roles(rules: &SourceRules) -> crate::rules::roles::SourceRoles {
    use crate::rules::roles::{Climate, FeatureRole, Ground, StartingUnit};
    let grounds = [
        (Ground::Water, "coast"),
        (Ground::Mountains, "mountain"),
        (Ground::Hills, "hill"),
        (Ground::Flat(Climate::Humid), "grassland"),
        (Ground::Flat(Climate::Temperate), "plains"),
        (Ground::Flat(Climate::Arid), "desert"),
        (Ground::Flat(Climate::Cold), "tundra"),
        (Ground::Flat(Climate::Frozen), "snow"),
    ];
    let named = |ids: Vec<&Option<StableContentKey>>, k: &str| {
        let k = key(k.to_string());
        ids.contains(&&Some(k.clone())).then_some(k)
    };
    let forest = named(rules.features.iter().map(|f| &f.id).collect(), "core:feature/forest");
    let units = [(StartingUnit::Defense, "core:unit/warrior"), (StartingUnit::Worker, "core:unit/worker")];
    let unit = |k| named(rules.units.iter().map(|u| &u.id).collect(), k);
    crate::rules::roles::SourceRoles {
        grounds: grounds.into_iter().map(|(g, slug)| (g, key(format!("core:terrain/{slug}")))).collect(),
        features: forest.map(|k| (FeatureRole::Forest, k)).into_iter().collect(),
        starting_units: units.into_iter().filter_map(|(role, k)| Some((role, unit(k)?))).collect(),
        ..Default::default()
    }
}

/// RON text -> SourceRules WITHOUT compilation (for the compiler's own tests:
/// invalid content must error, not compile).
pub(crate) fn parse_ron_text(text: &str) -> SourceRules {
    let fx: FxRules = ron::de::from_str(text).expect("fixture RON must parse");
    let mut rules = SourceRules {
        terrains: fixture_terrains(),
        units: fx
            .units
            .into_iter()
            .map(|u| SourceUnit {
                id: Some(key(u.id)),
                movement: u.movement,
                strength: u.strength,
                founder: u.founder,
                builder: u.builder,
                cost: u.cost,
                effects: effects(u.effects),
                required_tech: u.required_tech.map(key),
                ..Default::default()
            })
            .collect(),
        buildings: fx
            .buildings
            .into_iter()
            .map(|b| SourceBuilding {
                id: Some(key(b.id)),
                cost: b.cost,
                effects: effects(b.effects),
                required_tech: b.required_tech.map(key),
                wonder: b.wonder,
                ..Default::default()
            })
            .collect(),
        technologies: fx
            .technologies
            .into_iter()
            .map(|t| SourceTechnology {
                id: Some(key(t.id)),
                era_name: None,
                unlocks_buildings: t.unlocks_buildings.into_iter().map(key).collect(),
                cost: t.cost,
                requires: t.requires.into_iter().map(key).collect(),
                effects: effects(t.effects),
                ..Default::default()
            })
            .collect(),
        improvements: fx
            .improvements
            .into_iter()
            .map(|i| SourceImprovement {
                id: Some(key(i.id)),
                work_turns: i.work_turns,
                food_bonus: i.food_bonus,
                prod_bonus: i.prod_bonus,
                river_food: i.river_food,
                required_tech: i.required_tech.map(key),
                ..Default::default()
            })
            .collect(),
        features: fx
            .features
            .into_iter()
            .map(|f| SourceFeature {
                id: Some(key(f.id)),
                chop_turns: f.chop_turns,
                prod_bonus: f.prod_bonus,
                chop_grant: f.chop_grant,
                movement_cost: f.movement_cost,
                ..Default::default()
            })
            .collect(),
        resources: fx
            .resources
            .into_iter()
            .map(|r| SourceResource {
                id: Some(key(r.id)),
                food_bonus: r.food_bonus,
                prod_bonus: r.prod_bonus,
                gold_bonus: r.gold_bonus,
                requires_improvement: r.requires_improvement.map(key),
                required_tech: r.required_tech.map(key),
                terrains: r.terrains,
                density: r.density,
                ..Default::default()
            })
            .collect(),
        module: crate::classic::tables::ClassicSourceTables {
            beliefs: fx
                .beliefs
                .into_iter()
                .map(|b| crate::rules::source::SourceBelief {
                    id: Some(key(b.id)),
                    effects: effects(b.effects),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        },

        promotions: fx
            .promotions
            .into_iter()
            .map(|p| SourcePromotion {
                id: Some(key(p.id)),
                effects: effects(p.effects),
                ..Default::default()
            })
            .collect(),
        civs: vec![],
        ..Default::default()
    };
    rules.roles = fixture_roles(&rules);
    rules
}

/// RON text -> compiled content (the same compiler as the game).
pub(crate) fn compile_ron(text: &str) -> CompiledRuleset {
    compile_rules(parse_ron_text(text)).expect("fixture content must compile")
}

/// Arena: settler(0)/warrior(1)/worker(2), farm/mine, wheat(1)+gold(0),
/// flat techs agri(0)/pottery(1)/writing(2). Most mechanics.
pub(crate) const ARENA_RON: &str = r#"(
    units: [
        (id: "core:unit/settler", movement: 2, strength: 1, founder: true, cost: 40),
        (id: "core:unit/warrior", movement: 2, strength: 10, cost: 30),
        (id: "core:unit/worker", movement: 2, strength: 1, builder: true, cost: 30),
    ],
    improvements: [
        (id: "core:improvement/farm", work_turns: 3, food_bonus: 2, prod_bonus: 0, river_food: 1),
        (id: "core:improvement/mine", work_turns: 3, prod_bonus: 2),
    ],
    features: [
        (id: "core:feature/forest", chop_turns: 2, prod_bonus: 1, chop_grant: 8, movement_cost: 2),
    ],
    resources: [
        (id: "core:resource/wheat", food_bonus: 2, requires_improvement: Some("core:improvement/farm"), terrains: [0, 1], density: 8),
        (id: "core:resource/gold_ore", gold_bonus: 3, requires_improvement: Some("core:improvement/mine"), required_tech: Some("core:tech/writing"), terrains: [2, 3], density: 4),
    ],
    technologies: [
        (id: "core:tech/agriculture", cost: 20, effects: [(scope: City, selector: OwnerCities, modifier: AddYieldPercent(yield_kind: Output, percent: 10))]),
        (id: "core:tech/pottery", cost: 40, effects: [(scope: City, selector: OwnerCities, modifier: AddYieldPercent(yield_kind: Output, percent: 10))]),
        (id: "core:tech/writing", cost: 60, effects: [(scope: City, selector: OwnerCities, modifier: AddYieldPercent(yield_kind: Output, percent: 10))]),
    ],
)"#;

/// A chain: the same units/improvements, granary, techs agri->pottery->writing.
/// Sorted ids: granary=Building(0), gold_ore=Resource(0).
pub(crate) const CHAINED_RON: &str = r#"(
    units: [
        (id: "core:unit/settler", movement: 2, strength: 1, founder: true, cost: 40),
        (id: "core:unit/warrior", movement: 2, strength: 10, cost: 30),
        (id: "core:unit/worker", movement: 2, strength: 1, builder: true, cost: 30),
    ],
    buildings: [
        (id: "core:building/granary", cost: 60, effects: [(scope: City, selector: SourceCity, modifier: AddYield(yield_kind: Output, amount: 2))], required_tech: Some("core:tech/pottery")),
    ],
    improvements: [
        (id: "core:improvement/farm", work_turns: 3, food_bonus: 2, prod_bonus: 0, river_food: 1),
        (id: "core:improvement/mine", work_turns: 3, prod_bonus: 2),
    ],
    resources: [
        (id: "core:resource/gold_ore", gold_bonus: 3, requires_improvement: Some("core:improvement/mine"), required_tech: Some("core:tech/writing"), terrains: [2, 3], density: 4),
    ],
    technologies: [
        (id: "core:tech/agriculture", cost: 20, effects: [(scope: City, selector: OwnerCities, modifier: AddYieldPercent(yield_kind: Output, percent: 10))]),
        (id: "core:tech/pottery", unlocks_buildings: ["core:building/granary"], cost: 40, requires: ["core:tech/agriculture"], effects: [(scope: City, selector: OwnerCities, modifier: AddYieldPercent(yield_kind: Output, percent: 10))]),
        (id: "core:tech/writing", cost: 60, requires: ["core:tech/pottery"], effects: [(scope: City, selector: OwnerCities, modifier: AddYieldPercent(yield_kind: Output, percent: 10))]),
    ],
)"#;

pub(crate) fn arena_content() -> CompiledRuleset {
    compile_ron(ARENA_RON)
}

pub(crate) fn chained_content() -> CompiledRuleset {
    compile_ron(CHAINED_RON)
}

/// Ends the turns of whoever is active until `player` is the active player
/// (sequential turns: tests act for a player only on its own turn).
pub(crate) fn play_until_active(
    game: &mut crate::GameState,
    rules: &crate::CompiledRuleset,
    player: crate::ids::PlayerId,
) {
    for _ in 0..=u8::MAX {
        let active = game.view().auth.active_player();
        if active == player {
            return;
        }
        game.execute(rules, crate::Command::EndTurn { player: active })
            .expect("the active player can end its turn");
    }
    panic!("player {player:?} never became active");
}

/// One full round from the current player back to the same player.
pub(crate) fn play_round(game: &mut crate::GameState, rules: &crate::CompiledRuleset) {
    let start = game.view().auth.active_player();
    game.execute(rules, crate::Command::EndTurn { player: start })
        .expect("the active player can end its turn");
    play_until_active(game, rules, start);
}
