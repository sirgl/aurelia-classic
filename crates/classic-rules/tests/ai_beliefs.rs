//! Scenarios of the belief head (`ai::strategy_net::beliefs`): a belief is worth what its declared effects
//! bring us at the plan's prices, by the religion's reach. The scenarios name content only to set the scene
//! and to check the answer; the head reads the rules' data.

use classic_content::core::core_rules;
use classic_rules::ai::strategy_net::beliefs::{in_our_frame, Head, Reach};
use classic_rules::ai::strategy_net::features::neutral;
use classic_rules::ai::strategy_net::model::{CitySeed, Empire, Model};
use classic_rules::ai::strategy_net::{intent, price_mult, Base, Held, Intent, Plan};
use classic_rules::classic::effects::{ClassicCondition, ClassicSelector};
use classic_rules::ids::{BeliefId, BuildingKindId, FeatureId, ImprovementId, PlayerId, ResourceId, TechId, TerrainId, TileId};
use classic_rules::math::MapDimensions;
use classic_rules::rules::effects::{Condition, Selector};
use classic_rules::world::facts::TileFacts;
use classic_rules::{compile_rules, CompiledRuleset};

const W: u32 = 16;

/// Grassland everywhere, all nobody's.
struct Grass(TerrainId);

impl TileFacts for Grass {
    fn dimensions(&self) -> MapDimensions {
        MapDimensions { width: W, height: W }
    }
    fn terrain(&self, _: TileId) -> Option<TerrainId> {
        Some(self.0)
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
    fn river(&self, _: TileId) -> bool {
        false
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

fn rules() -> CompiledRuleset {
    compile_rules(core_rules()).unwrap()
}

fn belief(rules: &CompiledRuleset, key: &str) -> BeliefId {
    BeliefId(rules.module.beliefs.iter().position(|b| b.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

fn building(rules: &CompiledRuleset, key: &str) -> BuildingKindId {
    BuildingKindId(rules.buildings.iter().position(|b| b.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

/// Three cities of `population` on grassland with `buildings`.
fn model(rules: &CompiledRuleset, population: u32, buildings: &[BuildingKindId]) -> Model {
    let grass = rules.terrains.iter().find(|t| t.key == "core:terrain/grassland").and_then(|t| t.map_code).unwrap();
    let techs: Vec<TechId> = rules.technologies.iter().enumerate().filter(|(_, t)| t.requires.is_empty()).map(|(i, _)| TechId(i as u16)).collect();
    let seeds: Vec<CitySeed> = [TileId(3 * W + 3), TileId(3 * W + 11), TileId(11 * W + 7)]
        .into_iter()
        .enumerate()
        .map(|(i, tile)| CitySeed {
            id: i as u64 + 1,
            tile,
            population,
            food: 0,
            worked: vec![tile],
            income: [2 + 2 * i64::from(population), 6, 3, 2 + i64::from(population), 2, 1],
            buildings: buildings.to_vec(),
            capital: i == 0,
        })
        .collect();
    let empire = Empire { citizen_unhappiness: 100, science: 12, culture: 6, policy_cost: 60, science_percent: 67, pace: 67, religion: true, ..Empire::default() };
    Model::build_with(&Grass(TerrainId(grass)), rules, PlayerId(0), 0, &techs, &seeds, empire, classic_rules::ai::strategy_net::model::belief_conditions(rules))
}

fn base() -> Base {
    Base { rounds_left: 150, food: 2500, production: 1200, gold: 500, science: 800, culture: 300, faith: 250, happiness: 2000, new_city: 15000, citizen: 450, wonder: 4000, tech: 400 }
}

fn plan() -> Plan {
    let i: Intent = intent(&neutral(&[]), Held::default(), 3, 150, 67);
    Plan { round: 50, features: neutral(&[]), mult: price_mult(&i), intent: i, goals: Vec::new(), target: None, city_target: 3, growth: Vec::new(), war_open: false, wishes: Default::default() }
}

fn reach(foreign_cities: i64, foreign_followers: i64) -> Reach {
    Reach { own_share: 100, own_cities: 3, own_followers: 15, foreign_cities, foreign_followers, specialist_cities: 0, faith: 10 }
}

#[test]
fn our_following_cities_are_our_cities() {
    let rules = rules();
    let def = &rules.module.beliefs[belief(&rules, "core:belief/follower/religious-center").0 as usize];
    let ours = in_our_frame(&def.effects);
    let e = ours.passive_city[0];
    assert_eq!(def.effects.passive_city[0].selector, Selector::Module(ClassicSelector::FollowingCities));
    assert_eq!(e.selector, Selector::OwnerCities);
    assert_eq!(def.effects.passive_city[0].second_condition, Condition::Module(ClassicCondition::MinFollowers(5)));
    assert_eq!(e.second_condition, Condition::MinPopulation(5));
}

/// A belief that makes a yield at a building needs the building: no shrine, no temple, no food.
#[test]
fn a_belief_on_a_building_is_worth_it_only_where_it_stands() {
    let rules = rules();
    let (m, b, p) = (model(&rules, 6, &[]), base(), plan());
    let head = Head::new(&m, &rules, &p, &b, reach(2, 12));
    assert!(head.worth(belief(&rules, "core:belief/follower/feed-the-world"), &[]).total() == 0, "no shrine, no temple: no food");
    let temples = [building(&rules, "core:building/shrine"), building(&rules, "core:building/temple")];
    let m = model(&rules, 6, &temples);
    let head = Head::new(&m, &rules, &p, &b, reach(2, 12));
    assert!(head.worth(belief(&rules, "core:belief/follower/feed-the-world"), &[]).total() > 0, "food in the cities with a shrine and a temple");
}

/// A gate on a following city's followers holds in our city of as many citizens.
#[test]
fn a_temple_belief_waits_for_the_temple_and_the_followers() {
    let rules = rules();
    let (b, p) = (base(), plan());
    let center = belief(&rules, "core:belief/follower/religious-center");
    let temple = building(&rules, "core:building/temple");
    let worth = |population: u32, buildings: &[BuildingKindId]| {
        let m = model(&rules, population, buildings);
        Head::new(&m, &rules, &p, &b, reach(2, 12)).worth(center, &[]).total()
    };
    assert_eq!(worth(6, &[]), 0, "no temple");
    assert_eq!(worth(4, &[temple]), 0, "four followers of five");
    assert!(worth(6, &[temple]) > 0);
}

/// A founder belief pays by the reach: per follower, per following city.
#[test]
fn a_founder_pays_by_the_reach() {
    let rules = rules();
    let (m, b, p) = (model(&rules, 6, &[]), base(), plan());
    let tithe = belief(&rules, "core:belief/founder/tithe");
    let property = belief(&rules, "core:belief/founder/church-property");
    let near = Head::new(&m, &rules, &p, &b, reach(1, 6));
    let far = Head::new(&m, &rules, &p, &b, reach(8, 48));
    assert!(far.worth(tithe, &[]).total() > near.worth(tithe, &[]).total());
    assert!(far.worth(property, &[]).total() > near.worth(property, &[]).total());
    assert!(near.worth(tithe, &[]).total() > 0);
}

/// A number of the religion (its pressure) is worth the foreign cities it adds at what each pays us: nothing
/// without a founder belief that pays per following city.
#[test]
fn a_spread_enhancer_is_worth_what_a_foreign_city_pays() {
    let rules = rules();
    let (m, b, p) = (model(&rules, 6, &[]), base(), plan());
    let head = Head::new(&m, &rules, &p, &b, reach(2, 12));
    let texts = belief(&rules, "core:belief/enhancer/religious-texts");
    assert_eq!(head.worth(texts, &[]).total(), 0);
    let tithe = belief(&rules, "core:belief/founder/tithe");
    assert!(head.worth(texts, &[tithe]).total() > 0);
}

/// The choice reads the beliefs, not their order in the list it is given.
#[test]
fn the_choice_does_not_depend_on_the_list_order() {
    let rules = rules();
    let (m, b, p) = (model(&rules, 6, &[building(&rules, "core:building/temple")]), base(), plan());
    let head = Head::new(&m, &rules, &p, &b, reach(2, 12));
    let followers: Vec<BeliefId> = rules
        .module
        .beliefs
        .iter()
        .enumerate()
        .filter(|(_, d)| d.category == Some(classic_rules::classic::content::BeliefCategory::Follower))
        .map(|(i, _)| BeliefId(i as u16))
        .collect();
    let mut reversed = followers.clone();
    reversed.reverse();
    let (a, _) = head.choose(&followers, &[], &[]);
    let (b2, _) = head.choose(&reversed, &[], &[]);
    assert!(a.is_some());
    assert_eq!(a, b2);
    let (c, _) = head.choose(&followers, &[a.unwrap()], &[]);
    assert_ne!(c, a, "a taken belief is not chosen again");
}
