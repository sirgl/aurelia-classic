//! The AI's choice of the next policy branch (`ai::policy_branches`, docs/
//! ai-policies.md): a branch is valued by the effects its policies declare,
//! against the situation of the player. Each rule is checked on the classic
//! policy set and on a small synthetic one (no branch is named by the code).

use classic_content::core::core_rules;
use classic_rules::ai::policy_branches::{best_branch, branch_value, branch_worth, Aim, Situation};
use classic_rules::ai::AiState;
use classic_rules::ids::PolicyId;
use classic_rules::classic::effects::ClassicModifier;
use classic_rules::classic::content::PolicyBranchKind;
use classic_rules::classic::tables::{PolicyBranchDefinition, PolicyDefinition};
use classic_rules::ai::state::SeenCity;
use classic_rules::ids::{PlayerId, TerrainId, TileId};
use classic_rules::math::HexCoord;
use classic_rules::world::state::GameConfig;
use classic_rules::{ClassicGame, GameState};
use classic_rules::rules::content::{PolicyBranchId, PolicyRole, Personality};
use classic_rules::rules::effects::{
    CombatContext, CompiledEffect, Condition, Count, Duration, EffectSet, Modifier, PerCount, Scope, Selector, Stacking, Trigger, YieldKind,
};
use classic_rules::{compile_rules, CompiledRuleset};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
/// Map code of coast (the generator's water).
const COAST: TerrainId = TerrainId(4);

fn classic() -> CompiledRuleset {
    compile_rules(core_rules()).unwrap()
}

fn branch(rules: &CompiledRuleset, key: &str) -> PolicyBranchId {
    PolicyBranchId(rules.module.policy_branches.iter().position(|b| b.key == key).unwrap() as u16)
}

/// A player with one city, a free land, no rival known.
fn open_land() -> Situation {
    Situation {
        cities: 1,
        room: 6,
        food: 40,
        rival: None,
        coastal: 0,
        at_war: false,
        unhappy: false,
        city_states: 0,
        trade_routes: false,
        religion: false,
        personality: Personality::NEUTRAL,
    }
}

/// A policy of `branch` with one passive effect.
fn policy(key: &str, branch: u16, selector: Selector, modifier: Modifier) -> PolicyDefinition {
    conditional(key, branch, selector, Condition::Always, modifier)
}

/// A policy of `branch` with one passive effect that holds under `condition`.
fn conditional(key: &str, branch: u16, selector: Selector, condition: Condition, modifier: Modifier) -> PolicyDefinition {
    let effect = CompiledEffect {
        scope: selector.scope(),
        selector,
        condition,
        second_condition: Condition::Always,
        trigger: Trigger::Passive,
        modifier,
        duration: Duration::WhileSourceExists,
        stacking: Stacking::Stack,
    };
    let mut effects = EffectSet::default();
    match selector.scope() {
        Scope::Player => effects.passive_player.push(effect),
        _ => effects.passive_city.push(effect),
    }
    PolicyDefinition {
        key: key.to_owned(),
        effects,
        name: key.to_owned(),
        branch: Some(PolicyBranchId(branch)),
        role: PolicyRole::Policy,
        prerequisites: Vec::new(),
        grid: (0, 0),
        text: String::new(),
        inert: Vec::new(),
    }
}

fn synthetic_branch(key: &str) -> PolicyBranchDefinition {
    let template = classic().module.policy_branches[0].clone();
    PolicyBranchDefinition { key: key.to_owned(), name: key.to_owned(), opener: None, finisher: None, ..template }
}

/// Three branches of another game: one makes cities grow in every city,
/// one makes every city produce more, one only strengthens the army.
fn synthetic() -> (CompiledRuleset, [PolicyBranchId; 3]) {
    let mut rules = classic();
    rules.module.policy_branches = vec![synthetic_branch("x:fields"), synthetic_branch("x:forges"), synthetic_branch("x:drill")];
    rules.module.policies = vec![
        policy("x:fields/a", 0, Selector::OwnerCities, Modifier::AddYield { yield_kind: YieldKind::Food, amount: 2 }),
        policy("x:forges/a", 1, Selector::OwnerCities, Modifier::AddYield { yield_kind: YieldKind::Production, amount: 2 }),
        policy("x:drill/a", 2, Selector::Owner, Modifier::ModifyCombat { context: CombatContext::Any, percent: 25 }),
        policy("x:drill/b", 2, Selector::Owner, Modifier::ModifyCombat { context: CombatContext::Any, percent: 25 }),
        policy("x:drill/c", 2, Selector::Owner, Modifier::ModifyCombat { context: CombatContext::Any, percent: 25 }),
    ];
    (rules, [PolicyBranchId(0), PolicyBranchId(1), PolicyBranchId(2)])
}

#[test]
fn a_synthetic_set_is_valued_by_what_its_policies_declare() {
    let (rules, [fields, forges, drill]) = synthetic();
    let all = [fields, forges, drill];
    let peaceful = open_land();
    // Rich land wants growth, poor land wants production, war wants the army.
    let rich = Situation { food: 80, ..peaceful };
    let poor = Situation { food: 10, ..peaceful };
    assert_eq!(best_branch(&rules, &rich, &all), Some(fields));
    assert_eq!(best_branch(&rules, &poor, &all), Some(forges));
    let war = Situation { at_war: true, personality: Personality { aggression: 10, ..Personality::NEUTRAL }, ..poor };
    assert_eq!(best_branch(&rules, &war, &all), Some(drill));
}

#[test]
fn per_city_effects_count_the_cities_the_room_will_hold() {
    let (rules, [fields, _, drill]) = synthetic();
    let growth = |situation: &Situation, branch| branch_worth(&rules, situation, branch)[Aim::Growth as usize];
    let cramped = Situation { room: 0, ..open_land() };
    let roomy = Situation { room: 6, ..open_land() };
    // Food of every city: one city now, four with the room of three more.
    assert_eq!(growth(&roomy, fields), 4 * growth(&cramped, fields));
    // A flat bonus does not depend on the cities.
    assert_eq!(branch_worth(&rules, &roomy, drill), branch_worth(&rules, &cramped, drill));
}

#[test]
fn a_near_rival_takes_the_room_away() {
    let open = open_land();
    let squeezed = Situation { rival: Some(4), ..open };
    assert!(squeezed.expected_cities() < open.expected_cities());
    assert_eq!(Situation { rival: Some(40), ..open }.expected_cities(), open.expected_cities());
}

#[test]
fn classic_room_for_cities_unlocks_liberty_and_none_keeps_tradition() {
    let rules = classic();
    let (tradition, liberty) = (branch(&rules, "core:policy/tradition"), branch(&rules, "core:policy/liberty"));
    let both = [tradition, liberty];
    let roomy = Situation { room: 6, food: 25, ..open_land() };
    let boxed_in = Situation { room: 0, food: 25, ..open_land() };
    assert_eq!(best_branch(&rules, &roomy, &both), Some(liberty));
    assert_eq!(best_branch(&rules, &boxed_in, &both), Some(tradition));
}

#[test]
fn equal_branches_are_ordered_by_the_rules_not_by_id() {
    let mut rules = classic();
    let order = rules.roles.module.ai_branches().to_vec();
    let n = rules.module.policy_branches.len() as u16;
    rules.module.policy_branches = (0..n).map(|i| synthetic_branch(&format!("x:{i}"))).collect();
    rules.module.policies = (0..n)
        .map(|i| policy(&format!("x:{i}/a"), i, Selector::OwnerCities, Modifier::AddYield { yield_kind: YieldKind::Food, amount: 2 }))
        .collect();
    let all: Vec<PolicyBranchId> = (0..n).map(PolicyBranchId).collect();
    let reversed: Vec<PolicyBranchId> = all.iter().rev().copied().collect();
    assert_ne!(order[0], PolicyBranchId(0), "the order must differ from the id order for this test");
    assert_eq!(best_branch(&rules, &open_land(), &all), Some(order[0]));
    assert_eq!(best_branch(&rules, &open_land(), &reversed), Some(order[0]));
}

/// The branches still open once the first two are adopted: war and faith
/// have their say in these picks (the first one is room's).
fn later_picks(rules: &CompiledRuleset) -> Vec<PolicyBranchId> {
    let first = [branch(rules, "core:policy/tradition"), branch(rules, "core:policy/liberty")];
    (0..rules.module.policy_branches.len() as u16)
        .map(PolicyBranchId)
        .filter(|b| rules.module.policy_branches[b.0 as usize].kind == PolicyBranchKind::Normal && !first.contains(b))
        .collect()
}

#[test]
fn classic_a_warlike_leader_at_war_takes_honor_and_a_peaceful_one_does_not() {
    let rules = classic();
    let honor = branch(&rules, "core:policy/honor");
    let warlike = Situation { at_war: true, personality: Personality { aggression: 9, ..Personality::NEUTRAL }, ..open_land() };
    assert_eq!(best_branch(&rules, &warlike, &later_picks(&rules)), Some(honor));
    assert_ne!(best_branch(&rules, &open_land(), &later_picks(&rules)), Some(honor));
    // Aggression alone, without a war, does not make the army the choice.
    let hawk = Situation { personality: Personality { aggression: 9, ..Personality::NEUTRAL }, ..open_land() };
    assert_ne!(best_branch(&rules, &hawk, &later_picks(&rules)), Some(honor));
}

#[test]
fn classic_a_devout_leader_with_a_religion_takes_piety() {
    let rules = classic();
    let piety = branch(&rules, "core:policy/piety");
    let devout = Situation { religion: true, personality: Personality { religion: 9, ..Personality::NEUTRAL }, ..open_land() };
    assert_eq!(best_branch(&rules, &devout, &later_picks(&rules)), Some(piety));
    assert_ne!(best_branch(&rules, &open_land(), &later_picks(&rules)), Some(piety));
    // A devout leader without a religion founded does not.
    let pious = Situation { religion: false, ..devout };
    assert_ne!(best_branch(&rules, &pious, &later_picks(&rules)), Some(piety));
}

#[test]
fn classic_war_alone_does_not_make_honor_the_first_branch() {
    // Measured (classic/docs/ai-policies.md): Honor or Piety as the first branch
    // costs a third of the score at round 200, whoever the leader is.
    let rules = classic();
    let all = [branch(&rules, "core:policy/tradition"), branch(&rules, "core:policy/liberty"), branch(&rules, "core:policy/honor"), branch(&rules, "core:policy/piety")];
    let warlike = Situation { at_war: true, personality: Personality { aggression: 9, ..Personality::NEUTRAL }, ..open_land() };
    let devout = Situation { religion: true, personality: Personality { religion: 9, ..Personality::NEUTRAL }, ..open_land() };
    for situation in [warlike, devout] {
        let first = best_branch(&rules, &situation, &all).unwrap();
        assert!(first == all[0] || first == all[1]);
    }
}

/// A land of `width` x 20 tiles; every tile outside the first `land`
/// columns is sea, and the whole map is known to player 0.
fn world(land: i32) -> (GameState, CompiledRuleset) {
    let rules = classic();
    let mut game = GameState::new(&rules, GameConfig { players: 2, width: 30, height: 20, rng_seed: 7 });
    let dims = game.view().auth.map().dimensions;
    let tile = |q, r| dims.to_tile(HexCoord { q, r }).unwrap();
    let sea: Vec<_> = (0..20).flat_map(|r| (land..30).map(move |q| (q, r))).map(|(q, r)| (tile(q, r), COAST)).collect();
    game.debug_set_terrain_batch(&rules, &sea);
    let all: Vec<TileId> = (0..game.view().auth.map().tile_count() as u32).map(TileId).collect();
    game.debug_reveal(P0, &all);
    (game, rules)
}

fn capital(game: &mut GameState, rules: &CompiledRuleset, q: i32, r: i32) -> TileId {
    let tile = game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap();
    game.debug_found_city(rules, P0, tile);
    tile
}

#[test]
fn room_is_the_free_sites_around_the_capital_the_player_knows() {
    let (mut wide, rules) = world(30);
    capital(&mut wide, &rules, 2, 10);
    let (mut island, rules) = world(6);
    capital(&mut island, &rules, 2, 10);
    let (mut shore, rules) = world(6);
    capital(&mut shore, &rules, 5, 10);
    let memory = AiState::new(P0, 0);
    let wide = Situation::observe(&wide.ai_observation(&rules, P0), &rules, &memory);
    let island = Situation::observe(&island.ai_observation(&rules, P0), &rules, &memory);
    let shore = Situation::observe(&shore.ai_observation(&rules, P0), &rules, &memory);
    assert!(wide.room >= 2 * island.room, "an open land holds twice the cities: {} against {}", wide.room, island.room);
    assert_eq!(wide.coastal, 0);
    assert_eq!(shore.coastal, 100, "a capital on the shore is coastal");
}

#[test]
fn the_nearest_known_rival_city_is_the_distance() {
    let (mut game, rules) = world(30);
    let home = capital(&mut game, &rules, 4, 10);
    let rival_city = game.view().auth.map().dimensions.to_tile(HexCoord { q: 12, r: 10 }).unwrap();
    game.debug_found_city(&rules, P1, rival_city);
    game.debug_force_war(&rules, P0, P1);
    let mut memory = AiState::new(P0, 0);
    let unseen = Situation::observe(&game.ai_observation(&rules, P0), &rules, &memory);
    assert_eq!(unseen.rival, None, "a city the AI never saw is no rival");
    assert!(unseen.at_war);
    memory.see_city(SeenCity { tile: rival_city, owner: P1, population: 1, last_seen_round: 1 });
    let seen = Situation::observe(&game.ai_observation(&rules, P0), &rules, &memory);
    let dims = game.view().auth.map().dimensions;
    assert_eq!(seen.rival, dims.distance(home, rival_city));
}

/// What the policies of the classic branch `name` are worth in `situation`.
fn value(rules: &CompiledRuleset, situation: &Situation, name: &str) -> i64 {
    branch_value(rules, situation, &situation.wants(), branch(rules, &format!("core:policy/{name}")))
}

#[test]
fn classic_city_states_raise_patronage_and_commerce() {
    let rules = classic();
    let friendly = Situation { city_states: 5, personality: Personality { diplomacy: 9, ..Personality::NEUTRAL }, ..open_land() };
    for name in ["patronage", "commerce"] {
        assert!(value(&rules, &friendly, name) > value(&rules, &open_land(), name), "{name}");
    }
    assert_eq!(value(&rules, &friendly, "honor"), value(&rules, &open_land(), "honor"));
}

#[test]
fn classic_trade_routes_raise_commerce_and_a_merchant_leader_more() {
    let rules = classic();
    let routes = Situation { trade_routes: true, ..open_land() };
    let merchant = Situation { personality: Personality { gold: 9, ..Personality::NEUTRAL }, ..routes };
    assert!(value(&rules, &routes, "commerce") > value(&rules, &open_land(), "commerce"));
    assert!(value(&rules, &merchant, "commerce") > value(&rules, &routes, "commerce"));
}

#[test]
fn classic_a_scientist_leader_raises_rationalism() {
    let rules = classic();
    let scientist = Situation { personality: Personality { science: 9, ..Personality::NEUTRAL }, ..open_land() };
    assert!(value(&rules, &scientist, "rationalism") > value(&rules, &open_land(), "rationalism"));
    assert_eq!(value(&rules, &scientist, "honor"), value(&rules, &open_land(), "honor"));
}

#[test]
fn classic_a_coast_raises_exploration() {
    let rules = classic();
    let coast = Situation { coastal: 100, ..open_land() };
    assert!(value(&rules, &coast, "exploration") > value(&rules, &open_land(), "exploration"));
    assert_eq!(value(&rules, &coast, "tradition"), value(&rules, &open_land(), "tradition"));
}

#[test]
fn classic_unhappiness_raises_the_branch_that_declares_happiness() {
    let rules = classic();
    let unhappy = Situation { unhappy: true, ..open_land() };
    assert!(value(&rules, &unhappy, "tradition") > value(&rules, &open_land(), "tradition"));
}

/// Two synthetic branches whose only policies are `a` and `b` (policies of
/// branch 0 and branch 1), valued by the situation `open_land`.
fn worth_of_two(a: Vec<PolicyDefinition>, b: Vec<PolicyDefinition>, situation: &Situation) -> ([i32; 11], [i32; 11]) {
    let mut rules = classic();
    rules.module.policy_branches = vec![synthetic_branch("x:a"), synthetic_branch("x:b")];
    rules.module.policies = a.into_iter().chain(b).collect();
    (branch_worth(&rules, situation, PolicyBranchId(0)), branch_worth(&rules, situation, PolicyBranchId(1)))
}

fn food(key: &str, branch: u16) -> PolicyDefinition {
    policy(key, branch, Selector::Owner, Modifier::AddYield { yield_kind: YieldKind::Food, amount: 2 })
}

#[test]
fn a_policy_behind_others_counts_less() {
    // Policy 0 is branch a's; policies 1 and 2 are branch b's, 1 behind 2.
    let mut deep = food("x:b/deep", 1);
    deep.prerequisites = vec![PolicyId(2)];
    let (a, b) = worth_of_two(vec![food("x:a/one", 0)], vec![deep, food("x:b/first", 1)], &open_land());
    let growth = Aim::Growth as usize;
    assert_eq!(b[growth], a[growth] + a[growth] / 2, "the first counts whole, the second a half");
}

#[test]
fn the_finisher_waits_for_every_ordinary_policy() {
    let finisher = |key: &str, branch: u16| PolicyDefinition { role: PolicyRole::Finisher, ..food(key, branch) };
    let (alone, with_four) = worth_of_two(
        vec![finisher("x:a/finisher", 0)],
        vec![finisher("x:b/finisher", 1), food("x:b/1", 1), food("x:b/2", 1), food("x:b/3", 1), food("x:b/4", 1)],
        &open_land(),
    );
    let growth = Aim::Growth as usize;
    // Alone the finisher counts whole; behind four policies a fifth of it.
    assert_eq!(alone[growth], 20);
    assert_eq!(with_four[growth], 4 * 20 + 20 / 5);
}

#[test]
fn a_building_needing_a_technology_pays_later_than_one_anyone_can_build() {
    let rules = classic();
    let early = rules.buildings.iter().position(|b| b.required_tech.is_none() && !b.wonder && !b.unbuildable).unwrap();
    let late = rules.buildings.iter().position(|b| b.required_tech.is_some() && !b.wonder && !b.unbuildable).unwrap();
    let with = |building: usize| {
        let p = conditional(
            "x:a/p",
            0,
            Selector::Owner,
            Condition::HasBuilding(classic_rules::ids::BuildingKindId(building as u16)),
            Modifier::AddYield { yield_kind: YieldKind::Food, amount: 2 },
        );
        worth_of_two(vec![p], vec![], &open_land()).0[Aim::Growth as usize]
    };
    assert_eq!(with(early), 20);
    assert!(with(late) < with(early) / 2, "{} against {}", with(late), with(early));
}

#[test]
fn a_coastal_effect_is_worth_the_share_of_coastal_cities() {
    let effect = |situation: &Situation| {
        let p = conditional("x:a/p", 0, Selector::OwnerCities, Condition::CityCoastal, Modifier::AddYield { yield_kind: YieldKind::Food, amount: 2 });
        worth_of_two(vec![p], vec![], situation).0[Aim::Growth as usize]
    };
    let land = Situation { coastal: 0, ..open_land() };
    let half = Situation { coastal: 50, ..open_land() };
    let coast = Situation { coastal: 100, ..open_land() };
    assert_eq!(effect(&land), 0);
    assert_eq!(2 * effect(&half), effect(&coast));
}

#[test]
fn an_effect_of_peace_is_worth_nothing_at_war() {
    let effect = |situation: &Situation| {
        let p = conditional("x:a/p", 0, Selector::Owner, Condition::AtPeace, Modifier::AddYield { yield_kind: YieldKind::Food, amount: 2 });
        worth_of_two(vec![p], vec![], situation).0[Aim::Growth as usize]
    };
    assert_eq!(effect(&open_land()), 20);
    assert_eq!(effect(&Situation { at_war: true, ..open_land() }), 0);
}

#[test]
fn a_per_count_effect_counts_what_the_empire_has() {
    // "+1 happiness per connected city": nothing for one city, more with room.
    let connected = |situation: &Situation| {
        let p = policy(
            "x:a/p",
            0,
            Selector::Owner,
            Modifier::Module(ClassicModifier::AddHappinessPerCount { amount: 1, per: PerCount { count: Count::ConnectedCities, every: 1, max: None } }),
        );
        worth_of_two(vec![p], vec![], situation).0[Aim::Happiness as usize]
    };
    assert_eq!(connected(&Situation { room: 0, ..open_land() }), 0);
    assert!(connected(&Situation { room: 6, ..open_land() }) > 0);
    // "+1 happiness per 10 citizens" is a tenth of "+1 per citizen".
    let citizens = |every: u16| {
        let p = policy(
            "x:a/p",
            0,
            Selector::OwnerCities,
            Modifier::Module(ClassicModifier::AddHappinessPerCount { amount: 1, per: PerCount { count: Count::Population, every, max: None } }),
        );
        worth_of_two(vec![p], vec![], &open_land()).0[Aim::Happiness as usize]
    };
    assert_eq!(citizens(1), 10 * citizens(10));
}
