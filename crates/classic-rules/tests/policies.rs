//! Social policies on the classic ruleset (classic/docs/classic/rules.md): branch
//! unlock with its opener, prerequisites, completion with the finisher,
//! policy effects reaching cities, free buildings, ideologies with free
//! tenets and the tier rule, save/load, and the policy AI.

use classic_rules::ai::AiState;
use classic_rules::ClassicGame;
use classic_content::core::core_rules;
use classic_rules::derived::culture::{culture_overview, next_policy_cost, policy_status, PolicyBlock};
use classic_rules::derived::income::city_income;
use classic_rules::ids::{BuildingKindId, CityHandle, PlayerId, PolicyId, TechId, TileId};
use classic_rules::persistence::{decode::decode, encode::encode};
use classic_rules::replay::authoritative_hash;
use classic_rules::rules::content::PolicyBranchId;
use classic_rules::{compile_rules, Command, CompiledRuleset, GameConfig, GameState};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn rules() -> CompiledRuleset {
    compile_rules(core_rules()).unwrap()
}

fn world(rules: &CompiledRuleset) -> GameState {
    GameState::new(
        rules,
        GameConfig {
            players: 3,
            width: 30,
            height: 20,
            rng_seed: 5,
        },
    )
}

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    let dims = game.view().auth.map().dimensions;
    dims.to_tile(classic_rules::math::HexCoord { q, r }).unwrap()
}

fn branch(rules: &CompiledRuleset, key: &str) -> PolicyBranchId {
    let i = rules
        .module.policy_branches
        .iter()
        .position(|b| b.key == key)
        .unwrap();
    PolicyBranchId(i as u16)
}

fn policy(rules: &CompiledRuleset, key: &str) -> PolicyId {
    let i = rules.module.policies.iter().position(|p| p.key == key).unwrap();
    PolicyId(i as u16)
}

fn building(rules: &CompiledRuleset, key: &str) -> BuildingKindId {
    let i = rules.buildings.iter().position(|b| b.key == key).unwrap();
    BuildingKindId(i as u16)
}

fn tech(rules: &CompiledRuleset, key: &str) -> TechId {
    let i = rules
        .technologies
        .iter()
        .position(|t| t.key == key)
        .unwrap();
    TechId(i as u16)
}

fn found(
    game: &mut GameState,
    rules: &CompiledRuleset,
    player: PlayerId,
    q: i32,
    r: i32,
) -> CityHandle {
    let t = tile(game, q, r);
    game.debug_found_city(rules, player, t)
}

#[test]
fn first_city_is_the_capital_with_a_palace() {
    let rules = rules();
    let mut game = world(&rules);
    let capital = found(&mut game, &rules, P0, 5, 5);
    let second = found(&mut game, &rules, P0, 15, 5);
    let palace = building(&rules, "core:building/palace");
    let auth = game.view().auth;
    assert!(auth
        .cities()
        .get(capital)
        .unwrap()
        .buildings
        .contains(&palace));
    assert!(!auth
        .cities()
        .get(second)
        .unwrap()
        .buildings
        .contains(&palace));
    assert_eq!(
        classic_rules::world::city::capital_of(auth, &rules, P0),
        Some(capital)
    );
    // Palace: +1 culture, the capital's only culture.
    assert_eq!(
        city_income(auth, &rules, auth.cities().get(capital).unwrap()).culture,
        1
    );
    assert_eq!(
        city_income(auth, &rules, auth.cities().get(second).unwrap()).culture,
        0
    );
}

#[test]
fn unlocking_tradition_pays_the_cost_and_adopts_the_opener() {
    let rules = rules();
    let mut game = world(&rules);
    let capital = found(&mut game, &rules, P0, 5, 5);
    let tradition = branch(&rules, "core:policy/tradition");
    assert_eq!(next_policy_cost(game.view().auth, &rules, P0), 25);
    let err = game
        .execute(
            &rules,
            Command::UnlockPolicyBranch {
                player: P0,
                branch: tradition,
            },
        )
        .unwrap_err();
    assert_eq!(format!("{err:?}"), "RuleViolation(\"not enough culture\")");
    game.debug_grant_culture(P0, 30);
    game.execute(
        &rules,
        Command::UnlockPolicyBranch {
            player: P0,
            branch: tradition,
        },
    )
    .unwrap();
    let state = game.view().auth.module().culture().of(P0);
    assert_eq!(state.culture, 5, "30 - 25");
    assert!(state.has_branch(tradition));
    assert!(state.has_policy(policy(&rules, "core:policy/tradition/opener")));
    // Opener: +3 culture in the capital (with the Palace's 1).
    let auth = game.view().auth;
    assert_eq!(
        city_income(auth, &rules, auth.cities().get(capital).unwrap()).culture,
        4
    );
    // The opener counts: the next policy costs 30 (n = 1).
    assert_eq!(next_policy_cost(auth, &rules, P0), 30);
    game.validate(&rules).unwrap();
}

#[test]
fn prerequisites_and_the_finisher() {
    let rules = rules();
    let mut game = world(&rules);
    found(&mut game, &rules, P0, 5, 5);
    let tradition = branch(&rules, "core:policy/tradition");
    game.debug_grant_culture(P0, 100_000);
    game.execute(
        &rules,
        Command::UnlockPolicyBranch {
            player: P0,
            branch: tradition,
        },
    )
    .unwrap();
    let monarchy = policy(&rules, "core:policy/tradition/monarchy");
    assert_eq!(
        policy_status(game.view().auth, &rules, P0, monarchy),
        Err(PolicyBlock::MissingPrerequisite)
    );
    let order = [
        "core:policy/tradition/aristocracy",
        "core:policy/tradition/oligarchy",
        "core:policy/tradition/legalism",
        "core:policy/tradition/landed-elite",
        "core:policy/tradition/monarchy",
    ];
    let finisher = policy(&rules, "core:policy/tradition/finisher");
    for key in order {
        assert!(!game
            .view()
            .auth
            .module()
            .culture()
            .of(P0)
            .has_policy(finisher));
        game.execute(
            &rules,
            Command::AdoptPolicy {
                player: P0,
                policy: policy(&rules, key),
            },
        )
        .unwrap_or_else(|e| panic!("{key}: {e:?}"));
    }
    let state = game.view().auth.module().culture().of(P0);
    assert!(
        state.has_policy(finisher),
        "the last policy completes the branch"
    );
    assert_eq!(state.policies.len(), 7, "opener + 5 + finisher");
    assert_eq!(state.free_used, 1, "the finisher is free");
    assert_eq!(state.paid_policies(), 6);
    game.validate(&rules).unwrap();
}

#[test]
fn legalism_gives_monuments_to_the_first_four_cities_and_owes_the_rest() {
    let rules = rules();
    let mut game = world(&rules);
    let a = found(&mut game, &rules, P0, 3, 3);
    let b = found(&mut game, &rules, P0, 10, 3);
    let monument = building(&rules, "core:building/monument");
    game.debug_add_building(&rules, b, monument);
    game.debug_grant_culture(P0, 100_000);
    game.execute(
        &rules,
        Command::UnlockPolicyBranch {
            player: P0,
            branch: branch(&rules, "core:policy/tradition"),
        },
    )
    .unwrap();
    for key in [
        "core:policy/tradition/oligarchy",
        "core:policy/tradition/legalism",
    ] {
        game.execute(
            &rules,
            Command::AdoptPolicy {
                player: P0,
                policy: policy(&rules, key),
            },
        )
        .unwrap_or_else(|e| panic!("{key}: {e:?}"));
    }
    let has = |game: &GameState, c: CityHandle| {
        game.view()
            .auth
            .cities()
            .get(c)
            .unwrap()
            .buildings
            .contains(&monument)
    };
    assert!(has(&game, a), "a city without a monument gets one");
    // b already had one: it does not use a grant; 3 are owed to new cities.
    let owed = &game.view().auth.module().culture().of(P0).free_buildings;
    assert_eq!(owed, &vec![(monument, 3)]);
    // A new city founded with a settler receives one.
    let settler = rules
        .units
        .iter()
        .position(|u| u.key == "core:unit/settler")
        .unwrap();
    let t = tile(&game, 20, 10);
    let unit = game.debug_spawn_unit(&rules, P0, t, classic_rules::ids::UnitKindId(settler as u16));
    game.execute(&rules, Command::FoundCity { settler: unit })
        .unwrap();
    let c = game.view().auth.indexes().city_on_tile(t).unwrap();
    assert!(has(&game, c));
    assert_eq!(
        game.view().auth.module().culture().of(P0).free_buildings,
        vec![(monument, 2)]
    );
    game.validate(&rules).unwrap();
}

#[test]
fn policy_cost_grows_with_cities_and_representation_cuts_it() {
    let rules = rules();
    let mut game = world(&rules);
    for q in [3, 9, 15, 21] {
        found(&mut game, &rules, P0, q, 5);
    }
    // 4 cities, n = 0: 25 + 25·3·10/100 = 32 → 30 (rules.md).
    assert_eq!(next_policy_cost(game.view().auth, &rules, P0), 30);
    game.debug_grant_culture(P0, 100_000);
    game.execute(
        &rules,
        Command::UnlockPolicyBranch {
            player: P0,
            branch: branch(&rules, "core:policy/liberty"),
        },
    )
    .unwrap();
    // n = 1, 4 cities: 34 + 34·3·10/100 = 44 → 40.
    assert_eq!(next_policy_cost(game.view().auth, &rules, P0), 40);
    let keys = [
        "core:policy/liberty/republic",
        "core:policy/liberty/collective-rule",
        "core:policy/liberty/citizenship",
    ];
    for key in keys {
        game.execute(
            &rules,
            Command::AdoptPolicy {
                player: P0,
                policy: policy(&rules, key),
            },
        )
        .unwrap_or_else(|e| panic!("{key}: {e:?}"));
    }
    // n = 4, 4 cities (Unciv: PolicyManager.getPolicyCultureCost):
    // 25 + 12^2.01 = 172.6; × 1.3 = 224.4 → 224 → 220.
    assert_eq!(next_policy_cost(game.view().auth, &rules, P0), 220);
    game.execute(
        &rules,
        Command::AdoptPolicy {
            player: P0,
            policy: policy(&rules, "core:policy/liberty/representation"),
        },
    )
    .unwrap();
    // n = 5, per-city 10 % × 67 % = 6.7 %: 25 + 15^2.01 = 256.2;
    // × 1.201 = 307.7 → 308 → 305.
    assert_eq!(next_policy_cost(game.view().auth, &rules, P0), 305);
}

#[test]
fn on_adopt_grants_a_unit_in_the_capital() {
    let rules = rules();
    let mut game = world(&rules);
    let capital = found(&mut game, &rules, P0, 5, 5);
    game.debug_grant_culture(P0, 100_000);
    game.execute(
        &rules,
        Command::UnlockPolicyBranch {
            player: P0,
            branch: branch(&rules, "core:policy/liberty"),
        },
    )
    .unwrap();
    game.execute(
        &rules,
        Command::AdoptPolicy {
            player: P0,
            policy: policy(&rules, "core:policy/liberty/republic"),
        },
    )
    .unwrap();
    let before = game.view().auth.indexes().units_of(P0).len();
    game.execute(
        &rules,
        Command::AdoptPolicy {
            player: P0,
            policy: policy(&rules, "core:policy/liberty/collective-rule"),
        },
    )
    .unwrap();
    let units = game.view().auth.indexes().units_of(P0).to_vec();
    assert_eq!(units.len(), before + 1, "Collective Rule: a free settler");
    let settler = &game
        .view()
        .auth
        .units()
        .get(*units.last().unwrap())
        .unwrap();
    assert_eq!(
        rules.units[settler.core.kind.0 as usize].key,
        "core:unit/settler"
    );
    let city_tile = game.view().auth.cities().get(capital).unwrap().tile;
    let dims = game.view().auth.map().dimensions;
    assert!(
        dims.distance(settler.core.tile, city_tile).unwrap() <= 1,
        "in or by the capital"
    );
}

#[test]
fn era_gates_branches() {
    let rules = rules();
    let mut game = world(&rules);
    found(&mut game, &rules, P0, 5, 5);
    game.debug_grant_culture(P0, 100_000);
    let aesthetics = branch(&rules, "core:policy/aesthetics");
    let err = game
        .execute(
            &rules,
            Command::UnlockPolicyBranch {
                player: P0,
                branch: aesthetics,
            },
        )
        .unwrap_err();
    assert_eq!(format!("{err:?}"), "RuleViolation(\"era too early\")");
    // A Classical technology: the player is in the Classical era.
    game.debug_learn(&rules, P0, tech(&rules, "core:tech/drama-and-poetry"));
    game.execute(
        &rules,
        Command::UnlockPolicyBranch {
            player: P0,
            branch: aesthetics,
        },
    )
    .unwrap();
}

#[test]
fn ideologies_need_two_factories_and_reward_the_first_adopters() {
    let rules = rules();
    let mut game = world(&rules);
    let factory = building(&rules, "core:building/factory");
    let freedom = branch(&rules, "core:policy/freedom");
    let mut cities = Vec::new();
    for q in [3, 10, 17] {
        cities.push(found(&mut game, &rules, P0, q, 5));
    }
    for q in [3, 10, 17] {
        cities.push(found(&mut game, &rules, P1, q, 15));
    }
    let err = game
        .execute(
            &rules,
            Command::ChooseIdeology {
                player: P0,
                ideology: freedom,
            },
        )
        .unwrap_err();
    assert_eq!(
        format!("{err:?}"),
        "RuleViolation(\"needs a later era or more factories\")"
    );
    for c in &cities {
        game.debug_add_building(&rules, *c, factory);
    }
    game.execute(
        &rules,
        Command::ChooseIdeology {
            player: P0,
            ideology: freedom,
        },
    )
    .unwrap();
    let state = game.view().auth.module().culture().of(P0);
    assert_eq!(state.ideology, Some(freedom));
    assert_eq!(state.free_tenets, 3, "first adopter");
    // Tier rule (derived/culture.rs): a tier-2 tenet needs three tier-1 tenets first.
    let tier2 = policy(&rules, "core:policy/freedom/universal-suffrage");
    assert_eq!(
        policy_status(game.view().auth, &rules, P0, tier2),
        Err(PolicyBlock::TierLocked)
    );
    let adopt = |game: &mut GameState, key: &str| {
        game.execute(&rules, Command::AdoptPolicy { player: P0, policy: policy(&rules, key) })
            .unwrap_or_else(|e| panic!("{key}: {e:?}"));
    };
    adopt(&mut game, "core:policy/freedom/avant-garde");
    adopt(&mut game, "core:policy/freedom/civil-society");
    assert_eq!(
        policy_status(game.view().auth, &rules, P0, tier2),
        Err(PolicyBlock::TierLocked),
        "two tenets of the first tier are not enough"
    );
    adopt(&mut game, "core:policy/freedom/creative-expression");
    let state = game.view().auth.module().culture().of(P0);
    assert_eq!(state.free_tenets, 0);
    assert_eq!(
        state.paid_policies(),
        0,
        "free tenets do not raise the cost"
    );
    assert_eq!(
        policy_status(game.view().auth, &rules, P0, tier2),
        Err(PolicyBlock::NotEnoughCulture)
    );
    // Player 1 is the second adopter of Freedom: one free tenet.
    game.execute(&rules, Command::EndTurn { player: P0 })
        .unwrap();
    game.execute(
        &rules,
        Command::ChooseIdeology {
            player: P1,
            ideology: freedom,
        },
    )
    .unwrap();
    assert_eq!(game.view().auth.module().culture().of(P1).free_tenets, 1);
    // A tenet of another ideology is refused.
    let order_tenet = policy(&rules, "core:policy/order/five-year-plan");
    assert_eq!(
        policy_status(game.view().auth, &rules, P1, order_tenet),
        Err(PolicyBlock::OtherIdeology)
    );
    game.validate(&rules).unwrap();
}

#[test]
fn policies_survive_save_and_load() {
    let rules = rules();
    let mut game = world(&rules);
    found(&mut game, &rules, P0, 5, 5);
    game.debug_grant_culture(P0, 1000);
    game.execute(
        &rules,
        Command::UnlockPolicyBranch {
            player: P0,
            branch: branch(&rules, "core:policy/tradition"),
        },
    )
    .unwrap();
    game.execute(&rules, Command::EndTurn { player: P0 })
        .unwrap();
    let save = encode(&game, &rules);
    let loaded = decode(&save, &rules).unwrap();
    assert_eq!(authoritative_hash(&loaded), authoritative_hash(&game));
    assert_eq!(
        loaded.view().auth.module().culture().of(P0),
        game.view().auth.module().culture().of(P0)
    );
    // A damaged policy id is a corrupt save, not a panic.
    let mut bad = save.clone();
    bad.world.module.culture.players[0].policies.push(60_000);
    assert!(decode(&bad, &rules).is_err());
}

#[test]
fn the_ai_unlocks_tradition_for_a_small_empire_then_fills_it() {
    let rules = rules();
    let mut game = world(&rules);
    found(&mut game, &rules, P0, 5, 5);
    game.debug_grant_culture(P0, 100_000);
    let mut adopted = Vec::new();
    for _ in 0..4 {
        let obs = game.ai_observation(&rules, P0);
        let cmd = classic_rules::ai::policies::policy_command(&obs, &rules, &AiState::new(P0, 0)).expect("a decision");
        game.execute(&rules, cmd.clone()).unwrap();
        adopted.push(cmd);
    }
    let tradition = branch(&rules, "core:policy/tradition");
    assert!(
        matches!(adopted[0], Command::UnlockPolicyBranch { branch, .. } if branch == tradition)
    );
    let state = game.view().auth.module().culture().of(P0);
    let in_tradition = state
        .policies
        .iter()
        .filter(|p| rules.module.policies[p.0 as usize].branch == Some(tradition))
        .count();
    assert_eq!(in_tradition, 4, "opener + 3 policies of the same branch");
    // Observation lists what the core allows: nothing more without culture.
    let overview = culture_overview(game.view().auth, &rules, P1, 0);
    assert!(overview.adoptable.is_empty() && overview.unlockable.is_empty());
}
