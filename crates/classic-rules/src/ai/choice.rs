//! The learned chooser of what a city builds and what a player researches
//! (classic/docs/ai-choice.md): the features of a choice, the exploration
//! that makes its data, and the hook through which the allocator's choice
//! (`ai::allocator`) and the research choice (`planner_research`) either
//! stay as they are, explore, or follow the choice net (`ai::choice_net`).
//!
//! A choice set is the allocator's own top `TOP` candidates by the key it
//! chooses by (value per turn of production; value per beaker): the net
//! and the exploration only ever pick among them, so every order stays one
//! the allocator found legal and worth something.
//!
//! - `ChoiceMode::Off` (every variant but two): nothing here runs unless
//!   the dataset tool records on the thread (`log`), and then it only
//!   reads; the choice is the allocator's.
//! - `ChoiceMode::Explore` (the dataset tool `ai-choice` only): with
//!   probability `permille`/1000 the choice is a uniformly random one of the
//!   top `top`, drawn from the game's seed, the round, the player and the
//!   city (deterministic); the record says it explored, so a model can
//!   learn from those choices alone, where the taken candidate does not
//!   depend on the state.
//! - `ChoiceMode::Net` (`Variant::ChoiceNet` and its two halves): the
//!   candidate of the top with the best answer of the choice net when it
//!   leads the allocator's best by more than the net's margin
//!   (`choice_net::best`).
//!
//! Every feature comes from the rules' tables, the observation and the
//! allocator's prices: no content name.
//!
//! Features, one `i32` each (`*_NAMES` order). "Slots" are the six yields
//! `food, production, gold, science, culture, faith` (`Output` counts as
//! production); prices and values are centi-points (`value::Centi`).
//!
//! | group | features |
//! |---|---|
//! | empire (`EMPIRE_NAMES`) | the round; cities, citizens; beakers, gold a turn, treasury, excess happiness; technologies known and the deepest era of them; military power, map known percent, world wonders built anywhere, wars; the prices' rounds left and the price of a food, production, gold, beaker, culture and happiness |
//! | city (`CITY_NAMES`) | population; gross food, surplus, production, science, gold, culture, faith; buildings, wonders, buildings that add science, culture, gold; worked tiles; capital, occupied |
//! | production candidate (`PRODUCTION_NAMES`) | its kind (settler, worker, work boat, trade unit, other unit, building, world wonder, national wonder, project); the allocator's value (points), value per turn, that per mille of the best, its rank; turns, cost, upkeep; the allocator's value of its effects by slot, happiness and the rest (`Prices::effect_in`); the amounts of its plain effects by slot: flat, percent, per citizen (hundredths), and happiness |
//! | research candidate (`RESEARCH_NAMES`) | cost, turns at today's beakers; the planner's value per beaker (`planner_research`), that per mille of the best, its rank; its own value, the best follow-up, the score points, the sea value; its era; what it unlocks: buildings, world wonders nobody built, units, the strength over the best known unit, improvements, projects, embarking, roads, the map; the planner's value of those buildings, units, improvements and its own effects; the amounts of the unlocked buildings' plain effects as for production |

use std::cell::RefCell;

use crate::ai::planner::{role_of, AiRole, TurnBrief};
use crate::ai::planner_value::{building_value, effects_value, improvement_value, unit_strength, Weights};
use crate::ai::value::{Centi, Prices};
use crate::ids::{BuildingKindId, PlayerId, ProductionItem, TechId};
use crate::knowledge::observation::{AIObservation, OwnCity};
use crate::rules::content::BuildingKind;
use crate::rules::effects::{CompiledEffect, Condition, Modifier, Trigger, YieldKind};
use crate::rules::CompiledRuleset;

/// Candidates of a choice set: the allocator's best this many.
pub const TOP: usize = 4;

/// How the choices of a turn are made.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ChoiceMode {
    /// The allocator's choice.
    #[default]
    Off,
    /// The allocator's choice, or now and then a random one of its top.
    Explore(Explore),
    /// The choice net's best of the allocator's top, for the choices it
    /// names (the others: the allocator's).
    Net { production: bool, research: bool },
}

impl ChoiceMode {
    /// The net for both choices.
    pub const NET: ChoiceMode = ChoiceMode::Net { production: true, research: true };

    /// The mode of one kind of choice: a net mode without that kind is
    /// `Off` for it.
    pub fn of(self, kind: crate::ai::choice_net::Kind) -> ChoiceMode {
        use crate::ai::choice_net::Kind;
        match (self, kind) {
            (ChoiceMode::Net { production: false, .. }, Kind::Production) | (ChoiceMode::Net { research: false, .. }, Kind::Research) => ChoiceMode::Off,
            (mode, _) => mode,
        }
    }
}

/// The exploration of the dataset's games.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Explore {
    /// The game's seed.
    pub seed: u64,
    /// Chance of a random choice, per mille.
    pub permille: u32,
    /// The random choice is among the best this many (at most `TOP`).
    pub top: usize,
}

impl Explore {
    /// The index into a choice set of `options` candidates (best first) a
    /// random choice takes; None: this choice is the allocator's. `key`
    /// tells the choices of one round and player apart (a city's id; the
    /// research `RESEARCH_KEY`).
    pub fn pick(&self, round: u32, player: PlayerId, key: u64, options: usize) -> Option<usize> {
        let top = self.top.clamp(1, TOP).min(options);
        if top == 0 {
            return None;
        }
        let draw = mix(self.seed ^ mix(u64::from(round) << 8 | u64::from(player.0)) ^ mix(key ^ 0x5EED));
        if draw % 1000 >= u64::from(self.permille) {
            return None;
        }
        Some((mix(draw) % top as u64) as usize)
    }
}

/// The `key` of a research choice for `Explore::pick`.
pub const RESEARCH_KEY: u64 = u64::MAX;

/// splitmix64's finaliser.
fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

pub const EMPIRE_FEATURES: usize = 20;
pub const CITY_FEATURES: usize = 16;
pub const PRODUCTION_FEATURES: usize = 43;
pub const RESEARCH_FEATURES: usize = 42;
/// A production row: the empire, the city, the candidate.
pub const PRODUCTION_INPUTS: usize = EMPIRE_FEATURES + CITY_FEATURES + PRODUCTION_FEATURES;
/// A research row: the empire, the candidate.
pub const RESEARCH_INPUTS: usize = EMPIRE_FEATURES + RESEARCH_FEATURES;

pub const EMPIRE_NAMES: [&str; EMPIRE_FEATURES] = [
    "round",
    "cities",
    "citizens",
    "beakers",
    "gold_income",
    "treasury",
    "happiness",
    "techs",
    "era",
    "military_power",
    "known_percent",
    "built_wonders",
    "wars",
    "rounds_left",
    "price_food",
    "price_production",
    "price_gold",
    "price_science",
    "price_culture",
    "price_happiness",
];

pub const CITY_NAMES: [&str; CITY_FEATURES] = [
    "city_population",
    "city_food",
    "city_food_surplus",
    "city_production",
    "city_science",
    "city_gold",
    "city_culture",
    "city_faith",
    "city_buildings",
    "city_wonders",
    "city_science_buildings",
    "city_culture_buildings",
    "city_gold_buildings",
    "city_worked",
    "city_capital",
    "city_occupied",
];

/// The amounts of a building's plain effects (`amounts`), named after a
/// prefix.
const AMOUNT_NAMES: [&str; 19] = [
    "flat_food",
    "flat_production",
    "flat_gold",
    "flat_science",
    "flat_culture",
    "flat_faith",
    "percent_food",
    "percent_production",
    "percent_gold",
    "percent_science",
    "percent_culture",
    "percent_faith",
    "per_citizen_food",
    "per_citizen_production",
    "per_citizen_gold",
    "per_citizen_science",
    "per_citizen_culture",
    "per_citizen_faith",
    "flat_happiness",
];

pub const PRODUCTION_NAMES: [&str; PRODUCTION_FEATURES] = [
    "is_settler",
    "is_worker",
    "is_sea_worker",
    "is_trade_unit",
    "is_unit",
    "is_building",
    "is_world_wonder",
    "is_national_wonder",
    "is_project",
    "value",
    "value_per_turn",
    "permille_of_best",
    "rank",
    "turns",
    "cost",
    "upkeep",
    "value_food",
    "value_production",
    "value_gold",
    "value_science",
    "value_culture",
    "value_faith",
    "value_happiness",
    "value_other",
    AMOUNT_NAMES[0],
    AMOUNT_NAMES[1],
    AMOUNT_NAMES[2],
    AMOUNT_NAMES[3],
    AMOUNT_NAMES[4],
    AMOUNT_NAMES[5],
    AMOUNT_NAMES[6],
    AMOUNT_NAMES[7],
    AMOUNT_NAMES[8],
    AMOUNT_NAMES[9],
    AMOUNT_NAMES[10],
    AMOUNT_NAMES[11],
    AMOUNT_NAMES[12],
    AMOUNT_NAMES[13],
    AMOUNT_NAMES[14],
    AMOUNT_NAMES[15],
    AMOUNT_NAMES[16],
    AMOUNT_NAMES[17],
    AMOUNT_NAMES[18],
];

pub const RESEARCH_NAMES: [&str; RESEARCH_FEATURES] = [
    "tech_cost",
    "tech_turns",
    "value_per_beaker",
    "permille_of_best",
    "rank",
    "own_value",
    "follow_up",
    "points",
    "sea_value",
    "tech_era",
    "unlocks_buildings",
    "unlocks_wonders",
    "unlocks_units",
    "strength_gain",
    "unlocks_improvements",
    "unlocks_projects",
    "embarking",
    "roads",
    "map",
    "value_buildings",
    "value_units",
    "value_improvements",
    "value_effects",
    AMOUNT_NAMES[0],
    AMOUNT_NAMES[1],
    AMOUNT_NAMES[2],
    AMOUNT_NAMES[3],
    AMOUNT_NAMES[4],
    AMOUNT_NAMES[5],
    AMOUNT_NAMES[6],
    AMOUNT_NAMES[7],
    AMOUNT_NAMES[8],
    AMOUNT_NAMES[9],
    AMOUNT_NAMES[10],
    AMOUNT_NAMES[11],
    AMOUNT_NAMES[12],
    AMOUNT_NAMES[13],
    AMOUNT_NAMES[14],
    AMOUNT_NAMES[15],
    AMOUNT_NAMES[16],
    AMOUNT_NAMES[17],
    AMOUNT_NAMES[18],
];

/// An `i64` as a feature (saturated).
fn int(value: i64) -> i32 {
    value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

/// The slot of a yield (`Output` counts as production).
fn slot(kind: YieldKind) -> usize {
    match kind {
        YieldKind::Food => 0,
        YieldKind::Production | YieldKind::Output => 1,
        YieldKind::Gold => 2,
        YieldKind::Science => 3,
        YieldKind::Culture => 4,
        YieldKind::Module(crate::classic::effects::ClassicYield::Faith) => 5,
    }
}

/// The empire at the moment of a choice (`EMPIRE_NAMES`); the prices are
/// zeros without them.
pub fn empire_features(brief: &TurnBrief, rules: &CompiledRuleset, prices: Option<&Prices>) -> [i32; EMPIRE_FEATURES] {
    let era = brief.techs.iter().filter_map(|t| rules.technologies.get(usize::from(t.0))).map(|d| i32::from(d.era)).max().unwrap_or(0);
    let price = |f: fn(&Prices) -> Centi| prices.map_or(0, |p| int(f(p)));
    [
        int(i64::from(brief.round)),
        brief.cities.len() as i32,
        int(i64::from(brief.total_population)),
        brief.science,
        brief.gold_income,
        int(i64::from(brief.treasury)),
        brief.happiness,
        brief.techs.len() as i32,
        era,
        int(i64::from(brief.military_power)),
        int(i64::from(brief.known_percent)),
        brief.built_wonders.len() as i32,
        brief.relations.iter().filter(|r| r.at_war).count() as i32,
        prices.map_or(0, |p| int(i64::from(p.rounds_left))),
        price(|p| p.food),
        price(|p| p.production),
        price(|p| p.gold),
        price(|p| p.science),
        price(|p| p.culture),
        price(|p| p.happiness),
    ]
}

/// The city of a production choice (`CITY_NAMES`).
pub fn city_features(obs: &AIObservation<'_>, rules: &CompiledRuleset, city: &OwnCity) -> [i32; CITY_FEATURES] {
    use crate::ai::taste::Sides;
    let defs = city.buildings.iter().filter_map(|b| rules.buildings.get(usize::from(b.0)).map(|d| (*b, d)));
    let (mut wonders, mut science, mut culture, mut gold) = (0, 0, 0, 0);
    for (kind, def) in defs {
        if def.wonder {
            wonders += 1;
            continue;
        }
        let sides = Sides::of_building(rules, kind);
        science += i32::from(sides.has(Sides::SCIENCE));
        culture += i32::from(sides.has(Sides::CULTURE));
        gold += i32::from(sides.has(Sides::GOLD));
    }
    let income = &city.income;
    [
        int(i64::from(city.population)),
        income.food,
        income.food_surplus,
        income.production,
        income.science,
        income.gold,
        int(i64::from(income.culture)),
        int(i64::from(income.faith)),
        city.buildings.len() as i32,
        wonders,
        science,
        culture,
        gold,
        city.worked.len() as i32,
        i32::from(obs.view.own_capital() == Some(city.handle)),
        i32::from(city.status == crate::world::city::CityStatus::Occupied),
    ]
}

/// The amounts of the plain effects (always, passive) of a building
/// (`AMOUNT_NAMES`).
fn amounts(rules: &CompiledRuleset, kind: BuildingKindId, out: &mut [i32]) {
    let Some(def) = rules.buildings.get(usize::from(kind.0)) else { return };
    let set = &def.effects;
    let effects = set.passive_player.iter().chain(&set.passive_city).chain(&set.passive_unit).chain(&set.passive_tile);
    for effect in effects.filter(|e| matches!(e.condition, Condition::Always) && matches!(e.trigger, Trigger::Passive)) {
        match effect.modifier {
            Modifier::AddYield { yield_kind, amount } => out[slot(yield_kind)] += amount,
            Modifier::AddYieldPercent { yield_kind, percent } => out[6 + slot(yield_kind)] += percent,
            Modifier::AddYieldPerPopulation { yield_kind, hundredths } => out[12 + slot(yield_kind)] += hundredths,
            Modifier::Module(crate::classic::effects::ClassicModifier::AddHappiness { amount }) => out[18] += amount,
            _ => {}
        }
    }
}

/// The value slot of an effect: its yield's, happiness (6) or the rest (7).
fn value_slot(effect: &CompiledEffect) -> usize {
    match effect.modifier {
        Modifier::AddYield { yield_kind, .. }
        | Modifier::AddYieldPercent { yield_kind, .. }
        | Modifier::AddYieldPerPopulation { yield_kind, .. }
        | Modifier::AddYieldPerCount { yield_kind, .. }
        | Modifier::AddYieldPercentPerCount { yield_kind, .. } => slot(yield_kind),
        Modifier::Module(crate::classic::effects::ClassicModifier::AddHappiness { .. }) => 6,
        _ => 7,
    }
}

/// One candidate of a production choice as the allocator valued it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProductionOption {
    pub item: ProductionItem,
    /// Its value once done (`Allocator::candidates`).
    pub value: Centi,
    pub turns: u32,
    /// Value per turn: the key the allocator chooses by.
    pub ratio: i64,
}

/// A production candidate (`PRODUCTION_NAMES`): `best` is the best
/// ratio of its set, `rank` its place there.
#[allow(clippy::too_many_arguments)]
pub fn production_features(
    obs: &AIObservation<'_>,
    rules: &CompiledRuleset,
    prices: &Prices,
    city: &OwnCity,
    cities: i32,
    option: &ProductionOption,
    best: i64,
    rank: usize,
) -> [i32; PRODUCTION_FEATURES] {
    let mut out = [0i32; PRODUCTION_FEATURES];
    match option.item {
        ProductionItem::Unit(kind) => {
            let flag = match role_of(rules, kind) {
                AiRole::Settler => 0,
                AiRole::Worker if crate::ai::sea_work::is_sea_builder(rules, kind) => 2,
                AiRole::Worker => 1,
                _ if crate::derived::trade::is_trade_unit(rules, kind) => 3,
                _ => 4,
            };
            out[flag] = 1;
        }
        ProductionItem::Building(kind) => {
            let def = &rules.buildings[usize::from(kind.0)];
            out[if def.wonder {
                6
            } else if def.kind == BuildingKind::NationalWonder {
                7
            } else {
                5
            }] = 1;
            out[15] = int(i64::from(def.maintenance));
            let set = &def.effects;
            let effects = set.passive_player.iter().chain(&set.passive_city).chain(&set.passive_unit).chain(&set.passive_tile).chain(set.triggered.iter().flatten());
            let mut values = [0i64; 8];
            for effect in effects {
                values[value_slot(effect)] += prices.effect_in(obs, rules, city, effect, cities);
            }
            for (k, v) in values.iter().enumerate() {
                out[16 + k] = int(*v);
            }
            amounts(rules, kind, &mut out[24..43]);
        }
        ProductionItem::Project(_) => out[8] = 1,
    }
    out[9] = int(option.value / 100);
    out[10] = int(option.ratio);
    out[11] = if best > 0 { int(option.ratio * 1000 / best) } else { 0 };
    out[12] = rank as i32;
    out[13] = int(i64::from(option.turns.min(1000)));
    out[14] = int(i64::from(obs.view.own_production_cost(city.handle, option.item).unwrap_or(0)));
    out
}

/// One candidate of a research choice as `planner_research` valued it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResearchOption {
    pub tech: TechId,
    /// Value per beaker: the key the research chooses by.
    pub key: i64,
    /// Its own value, its best follow-up's, its score points, its sea
    /// value (`planner_research::research_by`).
    pub own: i32,
    pub next: i32,
    pub points: i32,
    pub sea: i32,
}

/// A research candidate (`RESEARCH_NAMES`): `w`, `cities` and `strength`
/// as the research values with; `best` the best key of its set.
#[allow(clippy::too_many_arguments)]
pub fn research_features(brief: &TurnBrief, rules: &CompiledRuleset, w: &Weights, cities: i32, strength: i32, option: &ResearchOption, best: i64, rank: usize) -> [i32; RESEARCH_FEATURES] {
    let mut out = [0i32; RESEARCH_FEATURES];
    let Some(def) = rules.technologies.get(usize::from(option.tech.0)) else { return out };
    out[0] = int(i64::from(def.cost));
    out[1] = int(i64::from(def.cost) / i64::from(brief.science.max(1)));
    out[2] = int(option.key);
    out[3] = if best > 0 { int(option.key * 1000 / best) } else { 0 };
    out[4] = rank as i32;
    out[5] = option.own;
    out[6] = option.next;
    out[7] = option.points;
    out[8] = option.sea;
    out[9] = i32::from(def.era);
    out[10] = def.unlocks_buildings.len() as i32;
    out[11] = def.unlocks_buildings.iter().filter(|b| rules.buildings[usize::from(b.0)].wonder && !brief.built_wonders.contains(b)).count() as i32;
    out[12] = def.unlocks_units.len() as i32;
    out[13] = def.unlocks_units.iter().map(|u| (unit_strength(rules, *u) - strength).max(0)).max().unwrap_or(0);
    out[14] = def.unlocks_improvements.len() as i32;
    out[15] = def.unlocks_projects.len() as i32;
    out[16] = i32::from(def.allows_embarking || def.embarked_all_water);
    out[17] = i32::from(def.bridge_building || def.road_move_change != 0);
    out[18] = i32::from(def.map_visible);
    out[19] = def.unlocks_buildings.iter().map(|b| building_value(rules, *b, w, 1).max(0) * cities.max(1) / 2).sum();
    out[20] = def.unlocks_units.iter().map(|u| (unit_strength(rules, *u) - strength).max(0) * w.military * 5).sum();
    out[21] = def.unlocks_improvements.iter().map(|i| improvement_value(rules, *i, w).max(0) * cities.max(1) / 2).sum();
    out[22] = effects_value(&def.effects, w, cities);
    for building in &def.unlocks_buildings {
        amounts(rules, *building, &mut out[23..42]);
    }
    out
}

/// The indices of the best `TOP` by `key`, best first (ties: the earlier).
pub fn top(keys: impl Iterator<Item = i64>) -> Vec<usize> {
    let mut order: Vec<(i64, usize)> = keys.enumerate().map(|(i, k)| (k, i)).collect();
    order.sort_by_key(|&(k, i)| (std::cmp::Reverse(k), i));
    order.into_iter().take(TOP).map(|(_, i)| i).collect()
}

/// The production choice of `city` among `options` (the allocator's
/// candidates; `picked` its own choice): an index into `options`.
#[allow(clippy::too_many_arguments)]
pub fn choose_production(
    mode: ChoiceMode,
    empire: &[i32; EMPIRE_FEATURES],
    obs: &AIObservation<'_>,
    rules: &CompiledRuleset,
    prices: &Prices,
    city: &OwnCity,
    cities: i32,
    options: &[ProductionOption],
    picked: Option<usize>,
) -> Option<usize> {
    let logging = log::enabled();
    let mode = mode.of(crate::ai::choice_net::Kind::Production);
    if (mode == ChoiceMode::Off && !logging) || options.is_empty() {
        return picked;
    }
    let set = top(options.iter().map(|o| o.ratio));
    let best = options[set[0]].ratio;
    let city_row = city_features(obs, rules, city);
    let rows: Vec<[i32; PRODUCTION_INPUTS]> = set
        .iter()
        .enumerate()
        .map(|(rank, &i)| {
            let candidate = production_features(obs, rules, prices, city, cities, &options[i], best, rank);
            let mut row = [0i32; PRODUCTION_INPUTS];
            row[..EMPIRE_FEATURES].copy_from_slice(empire);
            row[EMPIRE_FEATURES..EMPIRE_FEATURES + CITY_FEATURES].copy_from_slice(&city_row);
            row[EMPIRE_FEATURES + CITY_FEATURES..].copy_from_slice(&candidate);
            row
        })
        .collect();
    let (chosen, explored) = decide(mode, obs.round, obs.player, city.id.0, &set, picked, || {
        crate::ai::choice_net::best(crate::ai::choice_net::Kind::Production, rows.iter().map(|r| &r[..]))
    });
    if logging {
        log::push(Record {
            kind: crate::ai::choice_net::Kind::Production,
            player: obs.player.0,
            round: obs.round,
            city: city.id.0,
            items: set.iter().map(|&i| item_code(options[i].item)).collect(),
            rows: rows.iter().map(|r| r.to_vec()).collect(),
            chosen: chosen.and_then(|c| set.iter().position(|&i| i == c)),
            explored,
        });
    }
    chosen
}

/// The research choice among `options` (`planner_research`; `picked` its
/// own choice): an index into `options`.
#[allow(clippy::too_many_arguments)]
pub fn choose_research(
    mode: ChoiceMode,
    brief: &TurnBrief,
    rules: &CompiledRuleset,
    w: &Weights,
    cities: i32,
    strength: i32,
    options: &[ResearchOption],
    picked: Option<usize>,
) -> Option<usize> {
    let logging = log::enabled();
    let mode = mode.of(crate::ai::choice_net::Kind::Research);
    if (mode == ChoiceMode::Off && !logging) || options.is_empty() {
        return picked;
    }
    let set = top(options.iter().map(|o| o.key));
    let best = options[set[0]].key;
    let empire = empire_features(brief, rules, brief.prices.as_ref());
    let rows: Vec<[i32; RESEARCH_INPUTS]> = set
        .iter()
        .enumerate()
        .map(|(rank, &i)| {
            let mut row = [0i32; RESEARCH_INPUTS];
            row[..EMPIRE_FEATURES].copy_from_slice(&empire);
            row[EMPIRE_FEATURES..].copy_from_slice(&research_features(brief, rules, w, cities, strength, &options[i], best, rank));
            row
        })
        .collect();
    let (chosen, explored) = decide(mode, brief.round, brief.player, RESEARCH_KEY, &set, picked, || {
        crate::ai::choice_net::best(crate::ai::choice_net::Kind::Research, rows.iter().map(|r| &r[..]))
    });
    if logging {
        log::push(Record {
            kind: crate::ai::choice_net::Kind::Research,
            player: brief.player.0,
            round: brief.round,
            city: RESEARCH_KEY,
            items: set.iter().map(|&i| u32::from(options[i].tech.0)).collect(),
            rows: rows.iter().map(|r| r.to_vec()).collect(),
            chosen: chosen.and_then(|c| set.iter().position(|&i| i == c)),
            explored,
        });
    }
    chosen
}

/// The choice of a set (`set`: indices into the options, best first) and
/// whether exploration made it; `net` is the net's index into the set.
fn decide(mode: ChoiceMode, round: u32, player: PlayerId, key: u64, set: &[usize], picked: Option<usize>, net: impl FnOnce() -> Option<usize>) -> (Option<usize>, bool) {
    match mode {
        ChoiceMode::Off => (picked, false),
        ChoiceMode::Explore(explore) => match explore.pick(round, player, key, set.len()) {
            Some(at) => (Some(set[at]), true),
            None => (picked, false),
        },
        // The net's choice, the allocator's best included: a leader's taste
        // (`ai::taste`) no longer moves the choice within the top.
        ChoiceMode::Net { .. } => (net().map(|at| set[at]).or(picked), false),
    }
}

/// A production item as one number: a unit kind, 10 000 + a building
/// kind, 20 000 + a project.
pub fn item_code(item: ProductionItem) -> u32 {
    match item {
        ProductionItem::Unit(kind) => u32::from(kind.0),
        ProductionItem::Building(kind) => 10_000 + u32::from(kind.0),
        ProductionItem::Project(id) => 20_000 + u32::from(id.0),
    }
}

/// One choice as the dataset records it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub kind: crate::ai::choice_net::Kind,
    pub player: u8,
    pub round: u32,
    /// The city's id (`RESEARCH_KEY` for research).
    pub city: u64,
    /// The candidates of the set, best first by the allocator: their items
    /// (`item_code`, a technology's id) and rows (the net's inputs).
    pub items: Vec<u32>,
    pub rows: Vec<Vec<i32>>,
    /// The candidate taken (None: one outside the set, the leader's taste).
    pub chosen: Option<usize>,
    /// The choice was a random one of the set (`ChoiceMode::Explore`).
    pub explored: bool,
}

/// The choices of the thread for the dataset tool: off unless enabled, never
/// read by a decision.
pub mod log {
    use super::*;

    thread_local! {
        static LOG: RefCell<Option<Vec<Record>>> = const { RefCell::new(None) };
    }

    /// Start (or stop) recording on this thread.
    pub fn enable(on: bool) {
        LOG.with(|log| *log.borrow_mut() = on.then(Vec::new));
    }

    pub fn enabled() -> bool {
        LOG.with(|log| log.borrow().is_some())
    }

    /// The choices recorded since the last call.
    pub fn take() -> Vec<Record> {
        LOG.with(|log| log.borrow_mut().as_mut().map(std::mem::take).unwrap_or_default())
    }

    pub(super) fn push(record: Record) {
        LOG.with(|log| {
            if let Some(log) = log.borrow_mut().as_mut() {
                log.push(record);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exploration is a function of its inputs, fires at about its
    /// rate and spreads over the top.
    #[test]
    fn exploration_is_deterministic_and_uniform() {
        let explore = Explore { seed: 7, permille: 150, top: 4 };
        let mut fired = 0;
        let mut at = [0u32; TOP];
        for round in 0..400 {
            for city in 0..10u64 {
                let a = explore.pick(round, PlayerId(1), city, 6);
                assert_eq!(a, explore.pick(round, PlayerId(1), city, 6));
                if let Some(i) = a {
                    fired += 1;
                    at[i] += 1;
                }
            }
        }
        assert!((480..720).contains(&fired), "about 15 % of 4 000: {fired}");
        assert!(at.iter().all(|n| *n > fired / 8), "every one of the top: {at:?}");
        assert_eq!(Explore { permille: 0, ..explore }.pick(3, PlayerId(1), 2, 6), None);
        assert_eq!(Explore { permille: 1000, top: 4, seed: 1 }.pick(3, PlayerId(1), 2, 1), Some(0), "a set of one");
    }

    #[test]
    fn the_top_is_by_key_then_order() {
        assert_eq!(top([5, 9, 9, 1, 7, 3].into_iter()), vec![1, 2, 4, 0]);
        assert_eq!(top([2].into_iter()), vec![0]);
    }

    #[test]
    fn names_are_unique() {
        let mut names: Vec<&str> = EMPIRE_NAMES.iter().chain(&CITY_NAMES).chain(&PRODUCTION_NAMES).copied().collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), PRODUCTION_INPUTS);
        let mut names: Vec<&str> = EMPIRE_NAMES.iter().chain(&RESEARCH_NAMES).copied().collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), RESEARCH_INPUTS);
    }
}
