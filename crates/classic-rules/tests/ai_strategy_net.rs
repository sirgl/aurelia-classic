//! Scenarios of the strategy net (`ai::strategy_net`, classic/docs/ai-strategy-net.md):
//! hand-made situations where the right choice is known, each with the
//! choice the hand-written net must make. The situations are features
//! written by hand and small maps of real terrain (`ToyMap`) under the
//! classic rules; the net reads the rules' data, so a scenario names content
//! only to set the scene and to check the answer.

use classic_content::core::core_rules;
use classic_rules::ai::strategy_net::cell::{food_price, run, CellPrices, Delta, Outcome, Place, Plan as CellPlan};
use classic_rules::ai::strategy_net::features::neutral;
use classic_rules::ai::strategy_net::heads::{choose_policy, pick_tech, score_techs, Culture, PolicyChoice};
use classic_rules::ai::strategy_net::model::{CitySeed, Empire, Model};
use classic_rules::ai::strategy_net::weights::{fe, kc, pd};
use classic_rules::ai::strategy_net::{goals, intent, price_mult, Base, Goal, GoalKind, Held, Intent, Memory, Plan, StyleLock};
use classic_rules::ids::{BuildingKindId, FeatureId, ImprovementId, PlayerId, ResourceId, TechId, TerrainId, TileId};
use classic_rules::math::{hex_disc, MapDimensions};
use classic_rules::rules::content::PolicyBranchId;
use classic_rules::world::facts::TileFacts;
use classic_rules::{compile_rules, CompiledRuleset};

fn classic() -> CompiledRuleset {
    compile_rules(core_rules()).unwrap()
}

const P0: PlayerId = PlayerId(0);
const W: u32 = 24;

/// A small map of known tiles, all nobody's: terrain codes and rivers.
struct ToyMap {
    terrain: Vec<TerrainId>,
    river: Vec<bool>,
}

impl ToyMap {
    fn filled(code: TerrainId) -> Self {
        ToyMap { terrain: vec![code; (W * W) as usize], river: vec![false; (W * W) as usize] }
    }
    fn dims() -> MapDimensions {
        MapDimensions { width: W, height: W }
    }
    /// Every tile within `radius` of `centre` but the centre gets `code`.
    fn ring(&mut self, centre: TileId, radius: i32, code: TerrainId) {
        for t in hex_disc(Self::dims(), centre, radius) {
            if t != centre {
                self.terrain[t.0 as usize] = code;
            }
        }
    }
}

impl TileFacts for ToyMap {
    fn dimensions(&self) -> MapDimensions {
        Self::dims()
    }
    fn terrain(&self, tile: TileId) -> Option<TerrainId> {
        self.terrain.get(tile.0 as usize).copied()
    }
    fn feature(&self, _: TileId) -> Option<FeatureId> {
        None
    }
    fn natural_wonder(&self, _: TileId) -> Option<classic_rules::rules::content::NaturalWonderId> {
        None
    }
    fn resource(&self, _: TileId) -> Option<ResourceId> {
        None
    }
    fn improvement(&self, _: TileId) -> Option<ImprovementId> {
        None
    }
    fn pillaged(&self, _: TileId) -> bool {
        false
    }
    fn owner(&self, _: TileId) -> Option<PlayerId> {
        None
    }
    fn city(&self, _: TileId) -> bool {
        false
    }
    fn river(&self, tile: TileId) -> bool {
        self.river.get(tile.0 as usize).copied().unwrap_or(false)
    }
    fn lake(&self, _: TileId) -> bool {
        false
    }
    fn coastal_land(&self, _: TileId) -> bool {
        false
    }
    fn is_water(&self, _: TileId) -> bool {
        false
    }
    fn is_host(&self, _: PlayerId) -> bool {
        false
    }
}

fn terrain(rules: &CompiledRuleset, key: &str) -> TerrainId {
    let def = rules.terrains.iter().find(|t| t.key == key).unwrap_or_else(|| panic!("{key}"));
    TerrainId(def.map_code.unwrap())
}

fn building(rules: &CompiledRuleset, key: &str) -> BuildingKindId {
    BuildingKindId(rules.buildings.iter().position(|b| b.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

fn branch(rules: &CompiledRuleset, key: &str) -> PolicyBranchId {
    PolicyBranchId(rules.module.policy_branches.iter().position(|b| b.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

fn tech(rules: &CompiledRuleset, key: &str) -> TechId {
    TechId(rules.technologies.iter().position(|t| t.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

/// The technologies a game starts with: those without a prerequisite.
fn start_techs(rules: &CompiledRuleset) -> Vec<TechId> {
    rules.technologies.iter().enumerate().filter(|(_, t)| t.requires.is_empty()).map(|(i, _)| TechId(i as u16)).collect()
}

/// The allocator's prices at the start of a Quick game (centi-points).
fn base() -> Base {
    Base { rounds_left: 198, food: 2500, production: 1200, gold: 500, science: 800, culture: 300, faith: 250, happiness: 2000, new_city: 15000, citizen: 450, wonder: 4000, tech: 400 }
}

fn centre() -> TileId {
    TileId(12 * W + 12)
}

/// One city of `population` on `map`, working its best tiles.
fn model(rules: &CompiledRuleset, map: &ToyMap, population: u32, techs: &[TechId], excess: i64) -> Model {
    model_with(rules, map, population, techs, excess, 3)
}

fn model_with(rules: &CompiledRuleset, map: &ToyMap, population: u32, techs: &[TechId], excess: i64, production: i64) -> Model {
    let seed = CitySeed {
        id: 1,
        tile: centre(),
        population,
        food: 0,
        worked: vec![centre()],
        income: [2 + 2 * i64::from(population), production, 2, 1 + i64::from(population), 1, 0],
        // The capital's palace (the rules give it in the capital).
        buildings: rules.buildings.iter().position(|b| b.kind == classic_rules::rules::content::BuildingKind::Palace).map(|i| vec![BuildingKindId(i as u16)]).unwrap_or_default(),
        capital: true,
    };
    let empire = Empire {
        excess,
        citizen_unhappiness: 100,
        science: 3 + i64::from(population),
        culture: 2,
        policy_cost: 25,
        science_percent: 67,
        pace: 67,
        ..Empire::default()
    };
    // The city's yields are its tiles' (no buildings yet); `production` is
    // what it builds wonders with.
    let mut m = Model::build(map, rules, P0, 0, techs, &[seed], empire);
    m.cities[0].extra = [0; 6];
    m.cities[0].production = production;
    m
}

/// The plan of a situation: its intent, prices, the cell's review.
fn plan(rules: &CompiledRuleset, features: &[i64; fe::N], model: &Model) -> Plan {
    let intent = intent(features, Held::default(), model.cities.len() as u32, 198, 67);
    let mut mult = price_mult(&intent);
    let review = goals::review(model, rules, &intent, &mut mult, &base(), 1, false);
    let target = review.goals.iter().filter(|g| !g.path.is_empty()).max_by_key(|g| g.rate).and_then(|g| g.path.last().copied());
    Plan { round: 1, features: *features, city_target: 1 + intent.more_cities, intent, mult, goals: review.goals, target, growth: review.growth }
}

fn grassland_capital(rules: &CompiledRuleset) -> ToyMap {
    let mut map = ToyMap::filled(terrain(rules, "core:terrain/grassland"));
    for t in hex_disc(ToyMap::dims(), centre(), 2) {
        map.river[t.0 as usize] = true;
    }
    map
}

fn start_culture(rules: &CompiledRuleset) -> Culture {
    let unlockable = rules
        .module
        .policy_branches
        .iter()
        .enumerate()
        .filter(|(_, b)| b.era_prerequisite.is_none() && b.kind != classic_rules::classic::content::PolicyBranchKind::Ideology && !b.locked_without_religion)
        .map(|(i, _)| PolicyBranchId(i as u16))
        .collect();
    Culture { unlockable, ..Culture::default() }
}

fn great_land_no_room() -> [i64; fe::N] {
    neutral(&[(fe::CAP_FOOD, 900), (fe::CAP_PROD, 600), (fe::SITES_FOOD, 500), (fe::CAP_LUX, 700), (fe::ROOM, 0), (fe::RIVAL_NEAR, 300), (fe::RIVAL_KNOWN, 1000), (fe::CARAVAN_FOOD, 300)])
}

fn open_land() -> [i64; fe::N] {
    neutral(&[(fe::CAP_FOOD, 500), (fe::CAP_PROD, 400), (fe::SITES_FOOD, 600), (fe::ROOM, 1000), (fe::HAPPY, 800), (fe::CAP_LUX, 600)])
}

// ---- the intent ----

/// Scenario 1: Great land, no room: tall.
#[test]
fn great_land_without_room_plays_tall() {
    let i = intent(&great_land_no_room(), Held::default(), 1, 198, 67);
    assert_eq!(i.style_pick, 0, "tall: {:?}", i.style);
    assert!(i.concepts[kc::TALL_GROWTH] > 500);
    assert!(i.more_cities <= 1, "no room: at most one city more ({})", i.more_cities);
}

/// Scenario 2: Room and no threat: wide, and several cities more.
#[test]
fn open_land_plays_wide() {
    let i = intent(&open_land(), Held::default(), 1, 198, 67);
    assert_eq!(i.style_pick, 1, "wide: {:?}", i.style);
    assert!(i.more_cities >= 4, "{}", i.more_cities);
    let tall = intent(&great_land_no_room(), Held::default(), 1, 198, 67);
    assert!(price_mult(&i)[pd::EXPANSION] > price_mult(&tall)[pd::EXPANSION] + 250, "a city is worth more to wide play");
}

/// Scenario 3: A rival capital next door and a warlike nation early: the strike window,
/// conquest and the posture "prepare".
#[test]
fn a_warlike_nation_with_a_rival_next_door_prepares_a_strike() {
    let f = neutral(&[(fe::RIVAL_NEAR, 1000), (fe::RIVAL_KNOWN, 1000), (fe::T_AGGRESSION, 900), (fe::T_WAR_TOLERANCE, 800), (fe::UNIQUE_MILITARY, 1000), (fe::ROOM, 300)]);
    let i = intent(&f, Held::default(), 1, 198, 67);
    assert!(i.concepts[kc::STRIKE_WINDOW] > 500, "{:?}", i.concepts);
    assert_eq!(i.style_pick, 2, "conquest: {:?}", i.style);
    assert_eq!(i.posture_pick, 2, "prepare: {:?}", i.posture);
    assert!(price_mult(&i)[pd::MILITARY] > 2000);
}

/// Scenario 4: The same nation without a rival in reach: no window.
#[test]
fn no_rival_in_reach_no_strike_window() {
    let f = neutral(&[(fe::RIVAL_NEAR, 0), (fe::T_AGGRESSION, 900), (fe::UNIQUE_MILITARY, 1000), (fe::ROOM, 800)]);
    let i = intent(&f, Held::default(), 1, 198, 67);
    assert!(i.concepts[kc::STRIKE_WINDOW] < 100);
    assert_ne!(i.style_pick, 2);
    assert_eq!(i.posture_pick, 0, "peace: {:?}", i.posture);
}

/// Scenario 5: A happiness crisis: happiness first, no new city.
#[test]
fn a_happiness_crisis_prices_happiness_first_and_stops_founding() {
    let f = neutral(&[(fe::UNHAPPY, 800), (fe::HAPPY, 100), (fe::ROOM, 1000)]);
    let i = intent(&f, Held::default(), 3, 198, 67);
    assert!(i.concepts[kc::HAPPINESS_CRISIS] > 900);
    assert_eq!(i.more_cities, 0);
    let m = price_mult(&i);
    let top = (0..pd::N - 1).max_by_key(|d| m[*d]).unwrap();
    assert_eq!(top, pd::HAPPINESS, "{m:?}");
    assert!(m[pd::EXPANSION] < 500);
}

/// Scenario 6: Danger (camps, a threatened city): defence, and the army's price up.
#[test]
fn danger_turns_to_defence() {
    let f = neutral(&[(fe::BARBS, 900), (fe::THREAT, 500), (fe::FORCE, 300)]);
    let i = intent(&f, Held::default(), 2, 198, 67);
    assert_eq!(i.posture_pick, 1, "defence: {:?}", i.posture);
    let m = price_mult(&i);
    assert!(m[pd::MILITARY] > 1400 && m[pd::WONDER] < 1000, "{m:?}");
}

/// Scenario 7: A war under way: the war posture.
#[test]
fn a_war_under_way_is_fought() {
    let f = neutral(&[(fe::AT_WAR, 1000), (fe::FORCE, 700)]);
    assert_eq!(intent(&f, Held::default(), 3, 198, 67).posture_pick, 3);
}

/// Scenario 8: A scientific leader ahead in technology aims at science; a cultural one
/// with wonders at culture; a diplomatic one among city-states at diplomacy.
#[test]
fn the_victory_aim_follows_the_leader_and_the_lead() {
    let science = neutral(&[(fe::TECH_LEAD, 1000), (fe::T_SCIENCE, 900)]);
    assert_eq!(intent(&science, Held::default(), 3, 198, 67).aim_pick, 0);
    let culture = neutral(&[(fe::T_CULTURE, 1000), (fe::T_SCIENCE, 200), (fe::T_WONDERS, 1000), (fe::CAP_PROD, 1000), (fe::OUR_WONDERS, 1000), (fe::TECH_GAP, 500)]);
    assert_eq!(intent(&culture, Held::default(), 3, 198, 67).aim_pick, 1);
    let diplomacy = neutral(&[(fe::CITY_STATES, 1000), (fe::CS_NEAR, 1000), (fe::T_DIPLOMACY, 1000), (fe::GOLD, 1000), (fe::T_SCIENCE, 200), (fe::TECH_GAP, 500)]);
    assert_eq!(intent(&diplomacy, Held::default(), 3, 198, 67).aim_pick, 3);
}

/// Scenario 9: Hysteresis: a held style stays while the new one leads by less than 15 %.
#[test]
fn the_intent_is_held_unless_the_new_one_leads_by_15_percent() {
    // Between tall and wide: the room at which the two are within 15 %.
    let close = (0..=100).map(|r| neutral(&[(fe::CAP_FOOD, 800), (fe::ROOM, r * 10), (fe::SITES_FOOD, 400)])).find(|f| {
        let s = intent(f, Held::default(), 1, 198, 67).style;
        s[0] != s[1] && s[0].max(s[1]) * 1000 <= s[0].min(s[1]) * 1150
    });
    let f = close.expect("a room where tall and wide are a close call");
    let free = intent(&f, Held::default(), 1, 198, 67);
    let other = if free.style_pick == 0 { 1 } else { 0 };
    let held = intent(&f, Held { style: Some(other), ..Held::default() }, 1, 198, 67);
    assert_eq!(held.style_pick, other, "the held style stays");
}

/// Scenario 10: The horizon is longer for tall science, shorter for a strike.
#[test]
fn the_horizon_follows_the_intent() {
    let tall = intent(&great_land_no_room(), Held::default(), 1, 198, 67).horizon;
    let strike = intent(&neutral(&[(fe::RIVAL_NEAR, 1000), (fe::T_AGGRESSION, 1000), (fe::UNIQUE_MILITARY, 1000), (fe::AT_WAR, 1000)]), Held::default(), 1, 198, 67).horizon;
    assert!(tall > strike + 10, "tall {tall}, strike {strike}");
}

// ---- the policies ----

/// Scenario 11: Great land, no room → Tradition (from the policies' effects in the cell).
#[test]
fn tall_on_great_land_opens_tradition() {
    let rules = classic();
    let map = grassland_capital(&rules);
    let m = model(&rules, &map, 3, &start_techs(&rules), 4);
    let p = plan(&rules, &great_land_no_room(), &m);
    let choice = choose_policy(&m, &rules, &p, &base(), &start_culture(&rules));
    match choice {
        Some(PolicyChoice::Unlock(b)) => assert_eq!(b, branch(&rules, "core:policy/tradition"), "{}", rules.module.policy_branches[b.0 as usize].key),
        _ => panic!("a branch is opened"),
    }
}

/// Scenario 12: Room and no threat → Liberty.
#[test]
fn wide_on_open_land_opens_liberty() {
    let rules = classic();
    let map = grassland_capital(&rules);
    let m = model(&rules, &map, 2, &start_techs(&rules), 6);
    let p = plan(&rules, &open_land(), &m);
    match choose_policy(&m, &rules, &p, &base(), &start_culture(&rules)) {
        Some(PolicyChoice::Unlock(b)) => assert_eq!(b, branch(&rules, "core:policy/liberty"), "{}", rules.module.policy_branches[b.0 as usize].key),
        _ => panic!("a branch is opened"),
    }
}

/// Scenario 13: A started branch is finished before a new one is opened when the new
/// one does not lead by 10 %.
#[test]
fn a_started_branch_is_finished_first() {
    let rules = classic();
    let map = grassland_capital(&rules);
    let tradition = branch(&rules, "core:policy/tradition");
    let mut m = model(&rules, &map, 3, &start_techs(&rules), 4);
    m.branches.push(tradition);
    let opener = rules.module.policy_branches[tradition.0 as usize].opener.unwrap();
    m.policies.push(opener);
    let p = plan(&rules, &great_land_no_room(), &m);
    let adoptable: Vec<_> = rules
        .module
        .policies
        .iter()
        .enumerate()
        .filter(|(_, d)| d.branch == Some(tradition) && d.prerequisites.is_empty() && d.role == classic_rules::rules::content::PolicyRole::Policy)
        .map(|(i, _)| classic_rules::ids::PolicyId(i as u16))
        .collect();
    let mut culture = start_culture(&rules);
    culture.unlockable.retain(|b| *b != tradition);
    culture.adoptable = adoptable.clone();
    match choose_policy(&m, &rules, &p, &base(), &culture) {
        Some(PolicyChoice::Adopt(policy)) => assert!(adoptable.contains(&policy)),
        Some(PolicyChoice::Unlock(b)) => panic!("opened {} before finishing", rules.module.policy_branches[b.0 as usize].key),
        _ => panic!("no choice"),
    }
}

// ---- the cell and the goals ----

/// A capital among desert and hills.
fn desert_capital(rules: &CompiledRuleset) -> ToyMap {
    let mut map = ToyMap::filled(terrain(rules, "core:terrain/desert"));
    // Hills around, a few grassland tiles at the river.
    for (k, t) in hex_disc(ToyMap::dims(), centre(), 3).enumerate() {
        if k % 3 == 1 {
            map.terrain[t.0 as usize] = terrain(rules, "core:terrain/hill");
        }
    }
    // The river: grassland along it next to the centre (the city's food).
    for t in hex_disc(ToyMap::dims(), centre(), 1) {
        map.river[t.0 as usize] = true;
        map.terrain[t.0 as usize] = terrain(rules, "core:terrain/grassland");
    }
    map.terrain[centre().0 as usize] = terrain(rules, "core:terrain/plains");
    map
}

/// Scenario 14: The desert-bonus wonder: its effect counts on the desert tiles the
/// citizens will work, and it is worth nothing on grassland (it may not even
/// stand there: the rules' placement).
#[test]
fn the_desert_wonder_is_worth_its_desert_tiles() {
    let rules = classic();
    let petra = building(&rules, "core:building/petra");
    let desert = model(&rules, &desert_capital(&rules), 6, &start_techs(&rules), 6);
    let green = model(&rules, &grassland_capital(&rules), 6, &start_techs(&rules), 6);
    assert!(desert.placement_ok(&rules, 0, petra));
    assert!(!green.placement_ok(&rules, 0, petra), "it needs a desert in reach");
    let mult = [1000; pd::N];
    let prices = CellPrices::of(&base(), &mult);
    let gain = |m: &Model| {
        let empty = CellPlan::new(m);
        let mut with = CellPlan::new(m);
        with.add_building(m, &rules, 0, petra, 0);
        run(m, &with, &prices, 80).value - run(m, &empty, &prices, 80).value
    };
    assert!(gain(&desert) > 2 * gain(&green).max(1), "desert {} grassland {}", gain(&desert), gain(&green));
}

/// Scenario 15: The compounding inside the cell: after a wonder that feeds the city,
/// the desert wonder is worth more (more citizens work more desert tiles).
#[test]
fn growth_then_desert_tiles_compound() {
    let rules = classic();
    let petra = building(&rules, "core:building/petra");
    let gardens = building(&rules, "core:building/hanging_gardens");
    // Desert all round but a river of grassland next to the centre; content
    // citizens (the happiness is not what binds here).
    let mut map = ToyMap::filled(terrain(&rules, "core:terrain/desert"));
    map.ring(centre(), 1, terrain(&rules, "core:terrain/grassland"));
    let m = model(&rules, &map, 4, &start_techs(&rules), 40);
    let prices = CellPrices::of(&base(), &[1000; pd::N]);
    let value = |items: &[BuildingKindId]| {
        let mut plan = CellPlan::new(&m);
        for b in items {
            plan.add_building(&m, &rules, 0, *b, 0);
        }
        run(&m, &plan, &prices, 100).value
    };
    let alone = value(&[petra]) - value(&[]);
    let after = value(&[gardens, petra]) - value(&[gardens]);
    assert!(after > alone, "Petra alone {alone}, after the gardens {after}");
}

/// Scenario 16: On a desert-and-hills capital the desert-bonus wonder is a goal
/// (its worth from our desert tiles, the second worth most), and as the target
/// the research takes its path.
#[test]
fn a_desert_capital_rates_the_desert_wonder_and_beelines_its_technology() {
    let rules = classic();
    let petra = building(&rules, "core:building/petra");
    // A capital of size 6 that works its hills (10 hammers), Bronze Working known,
    // luxuries in reach (+15 happiness).
    let mut known = start_techs(&rules);
    known.push(tech(&rules, "core:tech/bronze-working"));
    let mut m = model_with(&rules, &desert_capital(&rules), 6, &known, 15, 10);
    m.science = 12;
    let f = neutral(&[(fe::CAP_FOOD, 200), (fe::CAP_PROD, 800), (fe::ROOM, 200), (fe::T_WONDERS, 800), (fe::CAP_LUX, 700)]);
    let p = plan(&rules, &f, &m);
    let ranked = goals::ranked(&m, &rules, &p.intent, &p.mult, &base());
    let at = ranked.iter().position(|(k, _, _, _)| matches!(k, GoalKind::Wonder { building, .. } if *building == petra)).expect("a candidate");
    let worth = ranked[at].1;
    assert!(at < 8 && worth > 0, "rank {at}, worth {worth}");
    let mut by_worth: Vec<i64> = ranked.iter().map(|r| r.1).collect();
    by_worth.sort_unstable_by(|a, b| b.cmp(a));
    assert!(by_worth.iter().position(|w| *w == worth).unwrap() < 5, "among the five worth most");
    assert!(p.goals.iter().any(|g| matches!(g.kind, GoalKind::Wonder { building, .. } if building == petra)), "a goal");
    // As the target: the research takes a technology on its path.
    let currency = rules.buildings[petra.0 as usize].required_tech.unwrap();
    let path = goals::path(&rules, &known, &[], currency);
    let goal = Goal { kind: GoalKind::Wonder { building: petra, city: 1 }, worth, rate: ranked[at].2, turns: ranked[at].3, path: path.clone() };
    // The beeline alone (the other goals' paths would compete: the target is
    // held only while no other leads it by 15 %).
    let mut p = p;
    p.goals = vec![goal];
    p.target = Some(currency);
    let available: Vec<TechId> = (0..rules.technologies.len()).map(|i| TechId(i as u16)).filter(|t| !known.contains(t) && rules.technologies[t.0 as usize].requires.iter().all(|r| known.contains(r))).collect();
    let scores = score_techs(&rules, &p, &available, 5, 67, |_| 400);
    let pick = pick_tech(&p, &scores).unwrap();
    assert!(path.contains(&pick), "{} on the way to {}", rules.technologies[pick.0 as usize].key, rules.technologies[currency.0 as usize].key);
}

/// Scenario 17: The food price grows with the horizon (the growth formula: earlier
/// citizens make more over a longer horizon).
#[test]
fn the_food_price_grows_with_the_horizon() {
    let rules = classic();
    let m = model(&rules, &grassland_capital(&rules), 3, &start_techs(&rules), 40);
    let prices = CellPrices::of(&base(), &[1000; pd::N]);
    let plan = CellPlan::new(&m);
    let short = food_price(&m, &plan, &prices, 20);
    let long = food_price(&m, &plan, &prices, 120);
    assert!(long > short && short > 0, "short {short}, long {long}");
}

/// The cell's run with the citizens' tile choice as first written: each
/// citizen scans the free tiles for the best weight, the lower index on a
/// tie. `run` keeps the weights and the order between steps; it must give
/// this run's outcome exactly.
fn run_by_scan(model: &Model, plan: &CellPlan, prices: &CellPrices, horizon: i64) -> Outcome {
    use classic_rules::ai::strategy_net::weights::cell::{FED_SURPLUS, STEPS, TILE_WEIGHTS, UNHAPPY_GROWTH, VERY_UNHAPPY};
    use classic_rules::derived::income::{growth_threshold, FOOD_PER_CITIZEN};
    let threshold = |pop: i64| i64::from(growth_threshold(pop.clamp(1, 200) as u32)) * model.pace;
    let steps = STEPS;
    let dt = (horizon / steps).max(1);
    let n = model.cities.len();
    let mut pop: Vec<i64> = model.cities.iter().map(|c| c.population).collect();
    let mut store: Vec<i64> = model.cities.iter().map(|c| c.food * 100).collect();
    let start: i64 = pop.iter().sum();
    let mut produced = [0i64; 6];
    let mut last = [0i64; 6];
    let mut first = vec![[0i64; 6]; n];
    let mut sums = vec![[0i64; 6]; n];
    let food_per_citizen = i64::from(FOOD_PER_CITIZEN) * 100;
    let spec_weight: i64 = (1..6).map(|y| model.specialist[y] * 100 * TILE_WEIGHTS[y] * 100).sum::<i64>() + model.specialist[0] * 100 * 100;
    for step in 0..steps {
        let t = step * dt;
        let happy: i64 = (0..n).flat_map(|i| plan.deltas[i].iter().filter(|d| d.from <= t).map(|d| d.happy)).sum();
        let mood = model.excess * 100 + happy;
        let mood_growth = if mood >= 0 {
            100
        } else if mood > VERY_UNHAPPY * 100 {
            UNHAPPY_GROWTH
        } else {
            0
        };
        for i in 0..n {
            let city = &model.cities[i];
            let active: Vec<&Delta> = plan.deltas[i].iter().filter(|d| d.from <= t && pop[i] >= d.min_pop).collect();
            let mut tiles: Vec<[i64; 6]> = city.tiles.iter().map(|t| t.yields.map(|v| v * 100)).collect();
            let mut centre = city.centre.yields.map(|v| v * 100);
            for d in &active {
                for (at, y) in &d.tiles {
                    let target = if *at == usize::MAX { &mut centre } else { &mut tiles[*at] };
                    for s in 0..6 {
                        target[s] += y[s];
                    }
                }
            }
            let food_weight = 100 + (TILE_WEIGHTS[0] * 100 - 100) * mood_growth / 100;
            let weight = |y: &[i64; 6], fw: i64| y[0] * fw + (1..6).map(|s| y[s] * TILE_WEIGHTS[s] * 100).sum::<i64>();
            let mut free: Vec<usize> = (0..tiles.len()).collect();
            let mut total = centre;
            let mut citizens = pop[i];
            let fed = food_per_citizen * pop[i] + FED_SURPLUS * 100 - city.extra[0] * 100;
            while citizens > 0 && !free.is_empty() {
                let fw = if total[0] < fed { food_weight } else { 100 };
                let (at, &k) = free.iter().enumerate().max_by_key(|(_, k)| (weight(&tiles[**k], fw), std::cmp::Reverse(**k))).unwrap();
                if weight(&tiles[k], fw) <= spec_weight {
                    break;
                }
                free.swap_remove(at);
                for s in 0..6 {
                    total[s] += tiles[k][s];
                }
                citizens -= 1;
            }
            let each = classic_rules::ai::strategy_net::model::per_citizen();
            for s in 0..6 {
                total[s] += citizens * model.specialist[s] * 100 + city.extra[s] * 100 + pop[i] * each[s] * 100;
            }
            let mut percent = [0i64; 6];
            let mut growth = 0;
            for d in &active {
                for s in 0..6 {
                    total[s] += d.flat[s] + d.per_pop[s] * pop[i];
                    percent[s] += d.percent[s];
                }
                growth += d.growth;
            }
            for s in 0..6 {
                total[s] = total[s].max(0) * (100 + percent[s]).max(0) / 100;
            }
            if step == 0 {
                first[i] = total;
            }
            for s in 0..6 {
                sums[i][s] += total[s];
            }
            let mut surplus = total[0] - food_per_citizen * pop[i];
            if surplus > 0 {
                surplus = surplus * (100 + growth) / 100 * mood_growth / 100;
            }
            store[i] += surplus * dt;
            while store[i] >= threshold(pop[i]) && pop[i] < 200 {
                store[i] -= threshold(pop[i]);
                pop[i] += 1;
            }
            if store[i] < 0 {
                store[i] = 0;
                if pop[i] > 1 {
                    pop[i] -= 1;
                }
            }
            for s in 1..6 {
                produced[s] += total[s] * dt;
                if step == steps - 1 {
                    last[s] += total[s];
                }
            }
        }
    }
    let gained = pop.iter().sum::<i64>() - start;
    let tail = (prices.rounds_left - steps * dt).max(0);
    let value = (1..6).map(|s| (produced[s] + last[s] * tail) / 100 * prices.unit[s] / 1000).sum::<i64>() + gained * prices.citizen;
    let mean = sums.iter().map(|s| s.map(|v| v / steps)).collect();
    Outcome { value, citizens: gained, first, mean }
}

/// A small deterministic generator for the spread of the cell's inputs.
struct Lcg(u64);

impl Lcg {
    fn below(&mut self, n: u64) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 33) % n.max(1)
    }
    fn between(&mut self, lo: i64, hi: i64) -> i64 {
        lo + self.below((hi - lo + 1) as u64) as i64
    }
}

/// Scenario 18: The cell's tile choice on kept weights and orders gives the
/// outcome of the scan it replaces, bit for bit: one to four cities of 1–30
/// citizens on real terrain and on tiles of random yields (many ties, some
/// negative), content, unhappy and very unhappy empires, plans of the
/// rules' buildings and policies and of hand-made deltas on tiles and the
/// centre that come at a round, need citizens (starving cities drop them
/// again) and move the mood, over horizons of 1–300 rounds.
#[test]
fn the_cells_kept_tile_choice_equals_the_scan() {
    let rules = classic();
    let mut rng = Lcg(18);
    let all_techs: Vec<TechId> = (0..rules.technologies.len()).map(|i| TechId(i as u16)).collect();
    let maps = [grassland_capital(&rules), desert_capital(&rules), {
        let kinds = ["grassland", "plains", "desert", "hill", "tundra", "mountain"].map(|k| terrain(&rules, &format!("core:terrain/{k}")));
        let mut map = ToyMap::filled(kinds[0]);
        for (k, t) in map.terrain.iter_mut().enumerate() {
            *t = kinds[(k * 7 + k / 5) % kinds.len()];
        }
        map
    }];
    let centres = [centre(), TileId(5 * W + 5), TileId(18 * W + 6), TileId(6 * W + 17)];
    let buildings: Vec<BuildingKindId> = (0..rules.buildings.len()).map(|i| BuildingKindId(i as u16)).collect();
    let (mut cases, mut on_tiles) = (0, 0);
    for round in 0..60 {
        let map = &maps[round % maps.len()];
        let count = 1 + rng.below(4) as usize;
        let seeds: Vec<CitySeed> = centres[..count]
            .iter()
            .enumerate()
            .map(|(k, tile)| CitySeed { id: k as u64 + 1, tile: *tile, population: 1 + rng.below(30) as u32, food: rng.below(60) as u32, worked: vec![*tile], income: [rng.between(0, 12), rng.between(1, 20), rng.between(0, 10), rng.between(0, 15), rng.between(0, 5), 0], buildings: Vec::new(), capital: k == 0 })
            .collect();
        let excess = [12, -4, -15][round % 3];
        let techs = if round % 2 == 0 { all_techs.clone() } else { start_techs(&rules) };
        let empire = Empire { excess, citizen_unhappiness: 100, science: 10, culture: 4, policy_cost: 25, science_percent: 67, pace: 67, ..Empire::default() };
        let mut m = Model::build(map, &rules, P0, 0, &techs, &seeds, empire);
        if round % 3 == 2 {
            for city in &mut m.cities {
                for tile in &mut city.tiles {
                    tile.yields = [rng.between(-1, 3), rng.between(0, 3), rng.between(0, 2), rng.between(0, 1), rng.between(0, 1), 0];
                }
            }
        }
        let horizon = [1, 7, 20, 45, 80, 150, 300][round % 7];
        let prices = CellPrices::of(&Base { rounds_left: 40 + 10 * round as i64, ..base() }, &[1000; pd::N]);
        let mut plan = CellPlan::new(&m);
        assert_eq!(run(&m, &plan, &prices, horizon), run_by_scan(&m, &plan, &prices, horizon), "round {round}, the empty plan");
        for item in 0..12 {
            let i = rng.below(count as u64) as usize;
            let from = rng.between(0, horizon);
            match rng.below(4) {
                0 => plan.add_building(&m, &rules, i, buildings[rng.below(buildings.len() as u64) as usize], from),
                1 => {
                    let policy = &rules.module.policies[rng.below(rules.module.policies.len() as u64) as usize];
                    plan.add(&m, &rules, &policy.effects, Place::Empire, from);
                }
                _ => {
                    let mut d = Delta { from, min_pop: rng.between(0, 12), growth: rng.between(-20, 50), happy: rng.between(-800, 400), ..Delta::default() };
                    d.flat[rng.below(6) as usize] = rng.between(-300, 400);
                    for _ in 0..rng.below(6) {
                        let at = if rng.below(5) == 0 || m.cities[i].tiles.is_empty() { usize::MAX } else { rng.below(m.cities[i].tiles.len() as u64) as usize };
                        let mut y = [0; 6];
                        y[rng.below(6) as usize] = rng.between(-200, 300);
                        d.tiles.push((at, y));
                    }
                    plan.deltas[i].push(d);
                }
            }
            on_tiles += usize::from(plan.deltas.iter().flatten().any(|d| d.tiles.iter().any(|(at, _)| *at != usize::MAX)));
            cases += 1;
            assert_eq!(run(&m, &plan, &prices, horizon), run_by_scan(&m, &plan, &prices, horizon), "round {round}, item {item}");
        }
    }
    assert!(cases == 720 && on_tiles >= 300, "{cases} cases, {on_tiles} with deltas on tiles");
}

// ---- the research head ----

fn goal_through(path: Vec<TechId>, worth: i64) -> Goal {
    Goal { kind: GoalKind::Tech(*path.last().unwrap()), worth, rate: worth, turns: 20, path }
}

fn plan_with(goals: Vec<Goal>, target: Option<TechId>) -> Plan {
    let i: Intent = intent(&neutral(&[]), Held::default(), 1, 198, 67);
    Plan { round: 1, features: neutral(&[]), mult: price_mult(&i), intent: i, goals, target, city_target: 1, growth: Vec::new() }
}

/// Scenario 18: A goal behind a bridge technology worth nothing itself: the bridge first.
#[test]
fn a_useless_bridge_technology_is_researched_for_its_goal() {
    let rules = classic();
    let currency = tech(&rules, "core:tech/currency");
    let bronze = tech(&rules, "core:tech/bronze-working");
    let other = tech(&rules, "core:tech/pottery");
    let p = plan_with(vec![goal_through(vec![bronze, currency], 30_000)], Some(currency));
    let own = |t: TechId| if t == bronze { 0 } else { 3_000 };
    let scores = score_techs(&rules, &p, &[bronze, other], 5, 67, own);
    assert_eq!(pick_tech(&p, &scores), Some(bronze));
    // Without the goal the other one.
    let none = plan_with(Vec::new(), None);
    assert_eq!(pick_tech(&none, &score_techs(&rules, &none, &[bronze, other], 5, 67, own)), Some(other));
}

/// Scenario 19: A prerequisite shared by two goals gets 30 % more of their value.
#[test]
fn a_shared_prerequisite_is_worth_more() {
    let rules = classic();
    let bronze = tech(&rules, "core:tech/bronze-working");
    let currency = tech(&rules, "core:tech/currency");
    let iron = tech(&rules, "core:tech/iron-working");
    let one = plan_with(vec![goal_through(vec![bronze, currency], 10_000)], None);
    let two = plan_with(vec![goal_through(vec![bronze, currency], 10_000), goal_through(vec![bronze, iron], 10_000)], None);
    let g1 = score_techs(&rules, &one, &[bronze], 5, 67, |_| 0)[0].goal;
    let g2 = score_techs(&rules, &two, &[bronze], 5, 67, |_| 0)[0].goal;
    assert!(g2 * 10 >= g1 * 2 * 13 - 10, "one goal {g1}, two goals {g2}");
}

/// Scenario 20: The target's path is kept unless another technology leads it by 15 %.
#[test]
fn the_target_is_kept_unless_another_leads_by_15_percent() {
    let rules = classic();
    let bronze = tech(&rules, "core:tech/bronze-working");
    let currency = tech(&rules, "core:tech/currency");
    let pottery = tech(&rules, "core:tech/pottery");
    let p = plan_with(vec![goal_through(vec![bronze, currency], 1)], Some(currency));
    // Equal turns (beakers plenty): own values decide.
    let close = score_techs(&rules, &p, &[bronze, pottery], 1_000_000, 67, |t| if t == bronze { 100_000 } else { 110_000 });
    assert_eq!(pick_tech(&p, &close), Some(bronze), "10 % is not enough");
    let far = score_techs(&rules, &p, &[bronze, pottery], 1_000_000, 67, |t| if t == bronze { 100_000 } else { 130_000 });
    assert_eq!(pick_tech(&p, &far), Some(pottery), "30 % is");
}

/// Scenario 21: The memory survives the round trip through `AiState::mind_memory`.
#[test]
fn the_memory_round_trips() {
    let rules = classic();
    let m = model(&rules, &desert_capital(&rules), 4, &start_techs(&rules), 8);
    let i = intent(&great_land_no_room(), Held::default(), 1, 198, 67);
    let mut mult = price_mult(&i);
    let review = goals::review(&m, &rules, &i, &mut mult, &base(), 7, false);
    let memory = Memory { held: Held { style: Some(0), aim: Some(1), posture: None, lock: Some(StyleLock { round: 13, situation: great_land_no_room() }) }, survey: None, review: Some(review), target: Some(TechId(5)) };
    assert_eq!(Memory::decode(&memory.encode()), memory);
    assert_eq!(Memory::decode(&[1, 2, 3]), Memory::default(), "another mind's bytes");
}

/// The net counts a few hundred hand weights.
#[test]
fn the_weights_are_counted() {
    let n = classic_rules::ai::strategy_net::weights::count();
    assert!((150..2000).contains(&n), "{n}");
}



// ---- the style lock and the branches (Roman's play, 3 November) ----

fn branch_policies(rules: &CompiledRuleset, b: PolicyBranchId) -> Vec<classic_rules::ids::PolicyId> {
    rules.module.policies.iter().enumerate().filter(|(_, d)| d.branch == Some(b)).map(|(i, _)| classic_rules::ids::PolicyId(i as u16)).collect()
}

fn opened(choice: Option<PolicyChoice>, rules: &CompiledRuleset) -> Option<String> {
    match choice {
        Some(PolicyChoice::Unlock(b)) => Some(rules.module.policy_branches[b.0 as usize].key.clone()),
        _ => None,
    }
}

/// Scenario 23: Each branch's character comes from its effects in the rules: the growth
/// branch is tall, the expansion branch wide, the war branch war, and so on.
#[test]
fn branch_characters_come_from_the_rules() {
    use classic_rules::ai::strategy_net::branches::{character, dominant};
    use classic_rules::ai::strategy_net::weights::branch::ch;
    let rules = classic();
    for (key, want) in [
        ("core:policy/tradition", ch::TALL),
        ("core:policy/liberty", ch::WIDE),
        ("core:policy/honor", ch::WAR),
        ("core:policy/rationalism", ch::SCIENCE),
        ("core:policy/commerce", ch::GOLD),
        ("core:policy/piety", ch::FAITH),
        ("core:policy/aesthetics", ch::CULTURE),
        ("core:policy/patronage", ch::DIPLOMACY),
    ] {
        let c = character(&rules, branch(&rules, key));
        assert_eq!(dominant(&c), Some(want), "{key}: {c:?}");
        assert!(c[want] >= 500, "{key}: {c:?}");
    }
}

/// Scenario 24: The war branch is shut outside the war posture: a conquest plan that
/// prepares a strike opens the expansion branch, a plan at war may open the war branch.
#[test]
fn the_war_branch_only_at_war() {
    let rules = classic();
    let map = grassland_capital(&rules);
    let m = model(&rules, &map, 3, &start_techs(&rules), 4);
    let honor = branch(&rules, "core:policy/honor");
    let strike = neutral(&[(fe::RIVAL_NEAR, 1000), (fe::RIVAL_KNOWN, 1000), (fe::T_AGGRESSION, 900), (fe::T_WAR_TOLERANCE, 800), (fe::UNIQUE_MILITARY, 1000), (fe::ROOM, 300)]);
    let p = plan(&rules, &strike, &m);
    assert_eq!((p.intent.style_pick, p.intent.posture_pick), (2, 2), "conquest, prepare");
    assert!(classic_rules::ai::strategy_net::branches::branch_fit(&rules, &p.intent, honor).is_none());
    assert_eq!(opened(choose_policy(&m, &rules, &p, &base(), &start_culture(&rules)), &rules).as_deref(), Some("core:policy/liberty"), "conquest opens the expansion branch");
    for f in [great_land_no_room(), open_land()] {
        let p = plan(&rules, &f, &m);
        assert_ne!(opened(choose_policy(&m, &rules, &p, &base(), &start_culture(&rules)), &rules).as_deref(), Some("core:policy/honor"));
        assert!(classic_rules::ai::strategy_net::branches::branch_fit(&rules, &p.intent, honor).is_none(), "shut in peace");
    }
    let war = neutral(&[(fe::AT_WAR, 1000), (fe::FORCE, 700)]);
    let p = plan(&rules, &war, &m);
    assert_eq!(p.intent.posture_pick, 3);
    assert!(classic_rules::ai::strategy_net::branches::branch_fit(&rules, &p.intent, honor).is_some_and(|f| f > 1000), "at war it pays");
}

/// Scenario 25: The style is chosen once: a wide style locked on open land stays wide
/// when its own cities have filled the room (the same situation unlocked is tall), and a
/// close call never moves a locked style.
#[test]
fn a_locked_style_holds_while_the_empire_grows() {
    let lock = StyleLock { round: 13, situation: open_land() };
    let wide = intent(&open_land(), Held { lock: Some(lock), ..Held::default() }, 1, 198, 67);
    assert_eq!(wide.style_pick, 1);
    assert!(wide.locked);
    // The room is gone: our own cities took it; the happiness is spent on them.
    let filled = neutral(&[(fe::CAP_FOOD, 900), (fe::CAP_PROD, 600), (fe::SITES_FOOD, 500), (fe::CAP_LUX, 700), (fe::ROOM, 0), (fe::HAPPY, 500), (fe::CARAVAN_FOOD, 400)]);
    assert_eq!(intent(&filled, Held::default(), 6, 150, 67).style_pick, 0, "unlocked: tall");
    let held = intent(&filled, Held { style: Some(1), lock: Some(lock), ..Held::default() }, 6, 150, 67);
    assert_eq!(held.style_pick, 1, "locked: still wide {:?}", held.style);
}

/// Scenario 26: Only an event moves a locked style: a war for survival (at war, the
/// cities threatened) lowers the margin to the ordinary 15 %.
#[test]
fn a_war_for_survival_reopens_the_style() {
    use classic_rules::ai::strategy_net::{concepts, survival};
    let calm = neutral(&[(fe::ROOM, 500)]);
    assert!(!survival(&calm, &concepts(&calm)));
    let siege = neutral(&[(fe::ROOM, 500), (fe::AT_WAR, 1000), (fe::THREAT, 600), (fe::FORCE, 300)]);
    assert!(survival(&siege, &concepts(&siege)));
    // Locked tall on great land; at war with the cities threatened, the style is
    // read with the events and held only by 15 %.
    let lock = StyleLock { round: 13, situation: great_land_no_room() };
    let at_war = intent(&siege, Held { style: Some(0), lock: Some(lock), ..Held::default() }, 3, 150, 67);
    let shares = at_war.style;
    let best = (0..3).max_by_key(|s| shares[*s]).unwrap();
    if best != 0 && shares[best] * 1000 > shares[0] * 1150 {
        assert_eq!(at_war.style_pick, best, "survival: the 15 % margin {shares:?}");
    } else {
        assert_eq!(at_war.style_pick, 0);
    }
}

/// Scenario 27: Tradition → Rationalism: with the science aim, the growth branch whole
/// and the science branch's era ahead, the culture waits (no filler branch); once the
/// science branch may be opened it is.
#[test]
fn the_science_aim_waits_for_the_science_branch() {
    let rules = classic();
    let map = grassland_capital(&rules);
    let tradition = branch(&rules, "core:policy/tradition");
    let rationalism = branch(&rules, "core:policy/rationalism");
    let mut m = model(&rules, &map, 6, &start_techs(&rules), 6);
    m.branches.push(tradition);
    m.policies.extend(branch_policies(&rules, tradition));
    let p = plan(&rules, &great_land_no_room(), &m);
    assert_eq!((p.intent.style_pick, p.intent.aim_pick), (0, 0), "tall, science");
    let mut culture = start_culture(&rules);
    culture.unlockable.retain(|b| *b != tradition);
    assert_eq!(opened(choose_policy(&m, &rules, &p, &base(), &culture), &rules), None, "the culture waits");
    let mut later = culture.clone();
    later.unlockable.extend(rules.module.policy_branches.iter().enumerate().filter(|(_, d)| d.era_prerequisite.is_some() && d.kind != classic_rules::classic::content::PolicyBranchKind::Ideology).map(|(i, _)| PolicyBranchId(i as u16)));
    assert!(later.unlockable.contains(&rationalism));
    assert_eq!(opened(choose_policy(&m, &rules, &p, &base(), &later), &rules).as_deref(), Some("core:policy/rationalism"));
}

/// Scenario 28: The ideology follows the plan by the rules' roles: the tall style's peace
/// ideology (Freedom after Tradition), the wide style's (Order after Liberty), the war
/// posture's (Autocracy).
#[test]
fn the_ideology_follows_the_style() {
    let rules = classic();
    let map = grassland_capital(&rules);
    let m = model(&rules, &map, 6, &start_techs(&rules), 6);
    let all: Vec<(PolicyBranchId, u8)> = ["core:policy/freedom", "core:policy/order", "core:policy/autocracy"].iter().map(|k| (branch(&rules, k), 0)).collect();
    let culture = Culture { ideologies: all, ..Culture::default() };
    let pick = |f: [i64; fe::N]| match choose_policy(&m, &rules, &plan(&rules, &f, &m), &base(), &culture) {
        Some(PolicyChoice::Ideology(b)) => rules.module.policy_branches[b.0 as usize].key.clone(),
        _ => panic!("an ideology"),
    };
    assert_eq!(pick(great_land_no_room()), "core:policy/freedom");
    assert_eq!(pick(open_land()), "core:policy/order");
    assert_eq!(pick(neutral(&[(fe::AT_WAR, 1000), (fe::FORCE, 700)])), "core:policy/autocracy");
}

/// The worked example of the weights page (work/strategy-net-weights.html):
/// `cargo test --test ai_strategy_net example_dump -- --ignored --nocapture`.
#[test]
#[ignore]
fn example_dump() {
    let rules = classic();
    let mut known = start_techs(&rules);
    known.push(tech(&rules, "core:tech/bronze-working"));
    let mut m = model_with(&rules, &desert_capital(&rules), 6, &known, 8, 10);
    m.science = 12;
    let f = neutral(&[(fe::CAP_FOOD, 200), (fe::CAP_PROD, 800), (fe::ROOM, 200), (fe::T_WONDERS, 800)]);
    let i = intent(&f, Held::default(), 1, 198, 67);
    let mut mult = price_mult(&i);
    let review = goals::review(&m, &rules, &i, &mut mult, &base(), 1, false);
    println!("FEATURES {:?}", f);
    println!("CONCEPTS {:?}", i.concepts);
    println!("STYLE {:?} AIM {:?} POSTURE {:?} more {} H {}", i.style, i.aim, i.posture, i.more_cities, i.horizon);
    println!("MULT {:?}", mult);
    for g in &review.goals { println!("GOAL {} worth {} rate {} turns {} path {:?}", classic_rules::ai::strategy_net::trace::goal_name(&rules, g.kind), g.worth, g.rate, g.turns, g.path.iter().map(|t| rules.technologies[t.0 as usize].key.clone()).collect::<Vec<_>>()); }
    for (k, w, r, t) in goals::ranked(&m, &rules, &i, &mult, &base()).into_iter().take(10) { println!("RANK {} {w} {r} {t}", classic_rules::ai::strategy_net::trace::goal_name(&rules, k)); }
    println!("TILES {:?}", m.cities[0].tiles.iter().map(|t| t.yields).collect::<Vec<_>>());
}


#[test]
#[ignore]
fn policy_dump() {
    let rules = classic();
    let map = grassland_capital(&rules);
    let m = model(&rules, &map, 3, &start_techs(&rules), 4);
    let p = plan(&rules, &great_land_no_room(), &m);
    println!("mult {:?} target {} goals {:?}", p.mult, p.city_target, p.goals.iter().map(|g| classic_rules::ai::strategy_net::trace::goal_name(&rules, g.kind)).collect::<Vec<_>>());
    for (pol, cell, other) in classic_rules::ai::strategy_net::heads::policy_values(&m, &rules, &p, &base()) {
        let d = &rules.module.policies[pol.0 as usize];
        if d.key.contains("tradition") || d.key.contains("honor") || d.key.contains("liberty") { println!("{} cell {cell} other {other}", d.key); }
    }
}

/// The characters of the branches (the weights page):
/// `cargo test --test ai_strategy_net character_dump -- --ignored --nocapture`.
#[test]
#[ignore]
fn character_dump() {
    let rules = classic();
    for (i, b) in rules.module.policy_branches.iter().enumerate() {
        let c = classic_rules::ai::strategy_net::branches::character(&rules, PolicyBranchId(i as u16));
        println!("{:32} {:?}", b.key, c);
    }
}

#[test]
#[ignore]
fn era_dump() {
    let rules = classic();
    for (i, e) in rules.eras.iter().enumerate() { println!("era {i} {}", e.key); }
    for b in rules.module.policy_branches.iter() { println!("{} era {:?} relig {} excl {:?}", b.key, b.era_prerequisite, b.locked_without_religion, b.mutually_exclusive_with); }
}
