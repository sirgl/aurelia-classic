//! Combat on the classic ruleset (classic/docs/classic/rules.md "Combat and
//! movement"): the worked examples of the rules, ranged attacks, city
//! strength and capture decisions, civilians, promotions, upgrades,
//! embarkation, ships, aircraft, nuclear weapons, save/load of the combat
//! state.

use classic_rules::ClassicGame;
use classic_content::core::core_rules;
use classic_rules::combat::exchange::{AttackKind, Defender};
use classic_rules::combat::plan_attack;
use classic_rules::command::CityDecision;
use classic_rules::ids::{CityHandle, PlayerId, TechId, TerrainId, TileId, UnitHandle, UnitKindId};
use classic_rules::math::HexCoord;
use classic_rules::replay::authoritative_hash;
use classic_rules::world::city::CityStatus;
use classic_rules::world::state::GameConfig;
use classic_rules::world::unit::Stance;
use classic_rules::{compile_rules, Command, CompiledRuleset, GameState};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const WATER: TerrainId = TerrainId(4);
const HILLS: TerrainId = TerrainId(2);

fn setup() -> (GameState, CompiledRuleset) {
    let rules = compile_rules(core_rules()).unwrap();
    let game = GameState::new(
        &rules,
        GameConfig {
            players: 2,
            width: 16,
            height: 16,
            rng_seed: 7,
        },
    );
    (game, rules)
}

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view()
        .auth
        .map()
        .dimensions
        .to_tile(HexCoord { q, r })
        .unwrap()
}

fn unit(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(
        rules
            .units
            .iter()
            .position(|u| u.key == key)
            .unwrap_or_else(|| panic!("{key}")) as u16,
    )
}

fn tech(rules: &CompiledRuleset, key: &str) -> TechId {
    TechId(
        rules
            .technologies
            .iter()
            .position(|t| t.key == key)
            .unwrap_or_else(|| panic!("{key}")) as u16,
    )
}

fn promotion(rules: &CompiledRuleset, key: &str) -> classic_rules::ids::PromotionId {
    classic_rules::ids::PromotionId(rules.promotions.iter().position(|p| p.key == key).unwrap() as u16)
}

fn war(game: &mut GameState, rules: &CompiledRuleset) {
    game.debug_meet(P0, P1);
    game.execute(
        rules,
        Command::DeclareWar {
            player: P0,
            enemy: P1,
        },
    )
    .unwrap();
}

/// End turns until `player` is active.
fn until_active(game: &mut GameState, rules: &CompiledRuleset, player: PlayerId) {
    for _ in 0..8 {
        let active = game.view().auth.active_player();
        if active == player {
            return;
        }
        game.execute(rules, Command::EndTurn { player: active })
            .unwrap();
    }
    panic!("{player:?} never became active");
}

fn next_round(game: &mut GameState, rules: &CompiledRuleset) {
    let start = game.view().auth.active_player();
    let other = if start == P0 { P1 } else { P0 };
    until_active(game, rules, other);
    until_active(game, rules, start);
}

fn forest(game: &mut GameState, rules: &CompiledRuleset, at: TileId) {
    let f = rules
        .features
        .iter()
        .position(|f| f.key == "core:feature/forest")
        .unwrap();
    game.debug_set_feature_batch(rules, &[(at, classic_rules::ids::FeatureId(f as u16))]);
}

/// Iron for P0's swordsmen: a city of P0 far away stands on an iron
/// deposit (Iron Working known). Without it a swordsman fights at −50 %
/// (rules.md).
fn iron_supply(game: &mut GameState, rules: &CompiledRuleset) {
    let at = tile(game, 12, 12);
    game.debug_found_city(rules, P0, at);
    let iron = rules.resources.iter().position(|r| r.key == "core:resource/iron").unwrap();
    game.debug_set_resource_batch(rules, &[(at, classic_rules::ids::ResourceId(iron as u16))]);
    game.debug_learn(rules, P0, tech(rules, "core:tech/iron-working"));
}

fn hp(game: &GameState, u: UnitHandle) -> Option<u16> {
    game.view().auth.units().get(u).map(|v| v.core.hp)
}

/// rules.md §1.10 / §2.4: a swordsman (14) attacks a spearman (11)
/// fortified one turn in a forest: 1400 against 1595; expected damage 28
/// to the spearman and 32 to the swordsman (Unciv's rounding to the nearest
/// HP, `BattleDamage.kt` roundToInt).
#[test]
fn worked_example_swordsman_against_fortified_spearman_in_forest() {
    let (mut game, rules) = setup();
    let a = tile(&game, 4, 4);
    let d = tile(&game, 5, 4);
    forest(&mut game, &rules, d);
    iron_supply(&mut game, &rules);
    let sword = game.debug_spawn_unit(&rules, P0, a, unit(&rules, "core:unit/swordsman"));
    let spear = game.debug_spawn_unit(&rules, P1, d, unit(&rules, "core:unit/spearman"));
    war(&mut game, &rules);
    until_active(&mut game, &rules, P1);
    game.execute(&rules, Command::Fortify { unit: spear })
        .unwrap();
    until_active(&mut game, &rules, P0);
    assert_eq!(
        game.view().auth.units().get(spear).unwrap().core.stance,
        Stance::Fortified { turns: 1 },
        "one turn of fortification when the enemy strikes"
    );
    let plan = plan_attack(game.view(), &rules, sword, d, AttackKind::Melee).unwrap();
    assert_eq!(plan.defender, Defender::Unit(spear));
    assert_eq!(plan.attacker_strength, 1400);
    assert_eq!(plan.defender_strength, 1595);
    let preview = plan.preview();
    assert_eq!(preview.to_defender.1, 28);
    assert_eq!(preview.to_attacker.1, 32);
    assert_eq!((preview.to_defender.0, preview.to_defender.2), (22, 34));
    assert_eq!((preview.to_attacker.0, preview.to_attacker.2), (26, 39));
    game.execute(
        &rules,
        Command::AttackUnit {
            attacker: sword,
            defender: spear,
        },
    )
    .unwrap();
    let dealt = 100 - hp(&game, spear).unwrap();
    let taken = 100 - hp(&game, sword).unwrap();
    assert!((22..=34).contains(&dealt), "damage in the range: {dealt}");
    assert!((26..=39).contains(&taken), "answer in the range: {taken}");
    let core = game.view().auth.units().get(sword).unwrap().core;
    assert_eq!(core.experience, 5, "melee attack experience");
    assert_eq!(core.moves_left, 0);
    assert_eq!(
        game.view().auth.units().get(spear).unwrap().core.experience,
        4
    );
    game.validate(&rules).unwrap();
}

/// A spearman's Anti-Mounted promotion (+50 % vs mounted) applies only
/// against mounted units; a river crossing costs 20 %; flanking adds 10 %
/// per unit next to the defender.
#[test]
fn situational_modifiers_follow_the_opponent_and_the_ground() {
    let (mut game, rules) = setup();
    let a = tile(&game, 4, 4);
    let d = tile(&game, 5, 4);
    let spear = game.debug_spawn_unit(&rules, P0, a, unit(&rules, "core:unit/spearman"));
    let horse = game.debug_spawn_unit(&rules, P1, d, unit(&rules, "core:unit/horseman"));
    let warrior_tile = tile(&game, 6, 3);
    war(&mut game, &rules);
    let vs_horse = plan_attack(game.view(), &rules, spear, d, AttackKind::Melee).unwrap();
    assert_eq!(vs_horse.attacker_strength, 11 * 150, "anti-mounted +50 %");
    game.debug_spawn_unit(&rules, P0, warrior_tile, unit(&rules, "core:unit/warrior"));
    let flanked = plan_attack(game.view(), &rules, spear, d, AttackKind::Melee).unwrap();
    assert_eq!(
        flanked.attacker_strength,
        11 * 160,
        "one flanking unit +10 %"
    );
    game.debug_set_river_edges(&rules, &[(a, d)]);
    let river = plan_attack(game.view(), &rules, spear, d, AttackKind::Melee).unwrap();
    assert_eq!(river.attacker_strength, 11 * 140, "river crossing −20 %");
    let _ = horse;
}

/// An archer shoots two tiles away: damage, no answer, experience 2/2.
#[test]
fn ranged_attack_hits_without_answer() {
    let (mut game, rules) = setup();
    let archer = game.debug_spawn_unit(
        &rules,
        P0,
        tile(&game, 4, 4),
        unit(&rules, "core:unit/archer"),
    );
    let target = tile(&game, 6, 4);
    let warrior = game.debug_spawn_unit(&rules, P1, target, unit(&rules, "core:unit/warrior"));
    war(&mut game, &rules);
    let melee = game.execute(
        &rules,
        Command::AttackUnit {
            attacker: archer,
            defender: warrior,
        },
    );
    assert!(
        melee.is_ok(),
        "attack orders of a ranged unit shoot: {melee:?}"
    );
    assert!(hp(&game, warrior).unwrap() < 100);
    assert_eq!(hp(&game, archer), Some(100), "no answer to a ranged attack");
    assert_eq!(
        game.view()
            .auth
            .units()
            .get(archer)
            .unwrap()
            .core
            .experience,
        2
    );
    assert_eq!(
        game.view()
            .auth
            .units()
            .get(warrior)
            .unwrap()
            .core
            .experience,
        2
    );
    let again = game.execute(
        &rules,
        Command::RangedAttack {
            unit: archer,
            target,
        },
    );
    assert!(again.is_err(), "one attack per turn");
    // Out of range and behind a blocker.
    next_round(&mut game, &rules);
    let far = tile(&game, 8, 4);
    assert!(game
        .execute(
            &rules,
            Command::RangedAttack {
                unit: archer,
                target: far
            }
        )
        .is_err());
    game.validate(&rules).unwrap();
}

/// A catapult must set up before it shoots (§3.2).
#[test]
fn siege_units_set_up_first() {
    let (mut game, rules) = setup();
    let catapult = game.debug_spawn_unit(
        &rules,
        P0,
        tile(&game, 4, 4),
        unit(&rules, "core:unit/catapult"),
    );
    let target = tile(&game, 5, 4);
    game.debug_spawn_unit(&rules, P1, target, unit(&rules, "core:unit/warrior"));
    war(&mut game, &rules);
    let early = game.execute(
        &rules,
        Command::RangedAttack {
            unit: catapult,
            target,
        },
    );
    assert!(early.is_err(), "not set up");
    game.execute(&rules, Command::SetUpRangedAttack { unit: catapult })
        .unwrap();
    game.execute(
        &rules,
        Command::RangedAttack {
            unit: catapult,
            target,
        },
    )
    .unwrap();
}

/// Civilians are captured (a settler becomes a worker) and the attacker
/// moves in; killing is not capturing (§2.3).
#[test]
fn melee_captures_civilians() {
    let (mut game, rules) = setup();
    let warrior = game.debug_spawn_unit(
        &rules,
        P0,
        tile(&game, 4, 4),
        unit(&rules, "core:unit/warrior"),
    );
    let target = tile(&game, 5, 4);
    let settler = game.debug_spawn_unit(&rules, P1, target, unit(&rules, "core:unit/settler"));
    game.debug_spawn_unit(
        &rules,
        P1,
        tile(&game, 12, 12),
        unit(&rules, "core:unit/warrior"),
    );
    war(&mut game, &rules);
    game.execute(
        &rules,
        Command::AttackUnit {
            attacker: warrior,
            defender: settler,
        },
    )
    .unwrap();
    let auth = game.view().auth;
    assert!(auth.units().get(settler).is_none());
    let worker = unit(&rules, "core:unit/worker");
    let mine: Vec<_> = auth
        .indexes()
        .units_of(P0)
        .iter()
        .map(|u| auth.units().get(*u).unwrap().core)
        .collect();
    assert!(
        mine.iter().any(|c| c.kind == worker && c.tile == target),
        "the settler becomes our worker"
    );
    assert_eq!(
        auth.units().get(warrior).unwrap().core.tile,
        target,
        "the attacker moves in"
    );
    game.validate(&rules).unwrap();
}

/// Ten experience buys a promotion (§6.3): Shock I adds 15 % attack on
/// open ground; the level grows; taking it twice is refused.
#[test]
fn promotions_take_experience_and_change_strength() {
    let (mut game, rules) = setup();
    let warrior = game.debug_spawn_unit(
        &rules,
        P0,
        tile(&game, 4, 4),
        unit(&rules, "core:unit/warrior"),
    );
    let target = tile(&game, 5, 4);
    game.debug_spawn_unit(&rules, P1, target, unit(&rules, "core:unit/warrior"));
    war(&mut game, &rules);
    let shock = promotion(&rules, "core:promotion/shock-1");
    let early = game.execute(
        &rules,
        Command::Promote {
            unit: warrior,
            promotion: shock,
        },
    );
    assert!(early.is_err(), "no experience yet");
    let before = plan_attack(game.view(), &rules, warrior, target, AttackKind::Melee).unwrap();
    game.debug_set_experience(warrior, 10);
    let core = *game.view().auth.units().get(warrior).unwrap().core;
    let offered =
        classic_rules::combat::experience::available_promotions(game.view().auth, &rules, &core);
    let melee = rules.units[core.kind.0 as usize].role.unwrap();
    assert!(offered.contains(&shock));
    for p in &offered {
        let def = &rules.promotions[p.0 as usize];
        assert!(def.unit_roles.contains(&melee), "{} is not for melee units", def.key);
    }
    game.execute(
        &rules,
        Command::Promote {
            unit: warrior,
            promotion: shock,
        },
    )
    .unwrap();
    let core = game.view().auth.units().get(warrior).unwrap().core;
    assert_eq!(core.level, 2);
    assert!(core.promotions.contains(shock));
    let after = plan_attack(game.view(), &rules, warrior, target, AttackKind::Melee).unwrap();
    assert_eq!(
        after.attacker_strength,
        before.attacker_strength * 115 / 100
    );
    let again = game.execute(
        &rules,
        Command::Promote {
            unit: warrior,
            promotion: shock,
        },
    );
    assert!(
        again.is_err(),
        "level 2 needs 30 experience and Shock I is taken"
    );
}

/// A promotion's preview before it is taken (`PlayerQueries::promoted_attack_preview`,
/// `promoted_defense_preview`, `ai::promo_fit`) is the preview after it is taken; the
/// instant heal's adds its health; without a promotion it is the plain preview.
#[test]
fn a_promotion_is_previewed_before_it_is_taken() {
    use classic_rules::knowledge::public::PlayerQueries;
    let (mut game, rules) = setup();
    let from = tile(&game, 4, 4);
    let warrior = game.debug_spawn_unit(&rules, P0, from, unit(&rules, "core:unit/warrior"));
    let target = tile(&game, 5, 4);
    let foe = game.debug_spawn_unit(&rules, P1, target, unit(&rules, "core:unit/warrior"));
    war(&mut game, &rules);
    game.debug_set_experience(warrior, 10);
    game.debug_set_unit_hp(warrior, 40);
    let shock = promotion(&rules, "core:promotion/shock-1");
    let heal = promotion(&rules, "core:promotion/insta-heal");
    let (plain, shocked, healed, guard, guard_healed) = {
        let view = game.player_view(&rules, P0);
        assert_eq!(view.promoted_attack_preview(warrior, None, from, target), view.attack_preview_from(warrior, from, target));
        (
            view.attack_preview_from(warrior, from, target).unwrap(),
            view.promoted_attack_preview(warrior, Some(shock), from, target).unwrap(),
            view.promoted_attack_preview(warrior, Some(heal), from, target).unwrap(),
            view.promoted_defense_preview(warrior, None, foe, target).unwrap(),
            view.promoted_defense_preview(warrior, Some(heal), foe, target).unwrap(),
        )
    };
    assert_eq!(healed.attacker_hp, 90);
    assert!(healed.to_defender.1 > plain.to_defender.1, "a healthier unit strikes harder");
    assert_eq!(guard.defender_hp, 40);
    assert_eq!(guard_healed.defender_hp, 90);
    game.execute(&rules, Command::Promote { unit: warrior, promotion: shock }).unwrap();
    let view = game.player_view(&rules, P0);
    assert_eq!(view.attack_preview_from(warrior, from, target), Some(shocked));
    assert_eq!(shocked.attacker_strength, plain.attacker_strength * 115 / 100);
}

/// Upgrade for gold in own territory (rules.md): warrior (40) →
/// swordsman (75) costs 10 + 35 × 2 = 80 and needs the swordsman's iron;
/// experience and level stay.
#[test]
fn upgrade_costs_gold_and_keeps_experience() {
    let (mut game, rules) = setup();
    let center = tile(&game, 6, 6);
    game.debug_found_city(&rules, P0, center);
    let warrior = game.debug_spawn_unit(
        &rules,
        P0,
        tile(&game, 6, 7),
        unit(&rules, "core:unit/warrior"),
    );
    game.debug_set_experience(warrior, 12);
    let refused = game.execute(&rules, Command::UpgradeUnit { unit: warrior });
    assert!(refused.is_err(), "Iron Working unknown");
    game.debug_learn(&rules, P0, tech(&rules, "core:tech/iron-working"));
    game.debug_learn(&rules, P0, tech(&rules, "core:tech/iron-working"));
    game.debug_earn(P0, 100);
    // Regression: the swordsman needs iron, as when it is trained; the
    // upgrade went through without it.
    let no_iron = game.execute(&rules, Command::UpgradeUnit { unit: warrior });
    assert!(no_iron.is_err(), "no iron");
    let iron = classic_rules::ids::ResourceId(rules.resources.iter().position(|r| r.key == "core:resource/iron").unwrap() as u16);
    game.debug_set_resource_batch(&rules, &[(center, iron)]);
    game.execute(&rules, Command::UpgradeUnit { unit: warrior })
        .unwrap();
    let auth = game.view().auth;
    assert_eq!(auth.players().treasury_of(P0), 20);
    let sword = unit(&rules, "core:unit/swordsman");
    let upgraded = auth
        .indexes()
        .units_of(P0)
        .iter()
        .map(|u| auth.units().get(*u).unwrap().core)
        .find(|c| c.kind == sword)
        .expect("a swordsman");
    assert_eq!(upgraded.experience, 12);
    assert_eq!(upgraded.moves_left, 0, "the upgrade ends the turn");
}

/// Land units enter water only after Optics; embarking ends the move and
/// an embarked unit defends with the era's value (§1.8, §7.5).
#[test]
fn embarkation_needs_optics_and_defends_weakly() {
    let (mut game, rules) = setup();
    let shore = tile(&game, 5, 5);
    let sea: Vec<(TileId, TerrainId)> = (6..12)
        .flat_map(|q| (2..10).map(move |r| (q, r)))
        .map(|(q, r)| (tile(&game, q, r), WATER))
        .collect();
    game.debug_set_terrain_batch(&rules, &sea);
    let warrior = game.debug_spawn_unit(&rules, P0, shore, unit(&rules, "core:unit/warrior"));
    let water = tile(&game, 6, 5);
    assert!(game
        .execute(
            &rules,
            Command::MoveUnit {
                unit: warrior,
                destination: water
            }
        )
        .is_err());
    game.debug_learn(&rules, P0, tech(&rules, "core:tech/optics"));
    game.execute(
        &rules,
        Command::MoveUnit {
            unit: warrior,
            destination: water,
        },
    )
    .unwrap();
    let core = game.view().auth.units().get(warrior).unwrap().core;
    assert_eq!(core.tile, water);
    assert_eq!(core.moves_left, 0, "embarking ends the move");
    // A trireme attacks the embarked warrior: Optics is classical, defense 4.
    let trireme = game.debug_spawn_unit(
        &rules,
        P1,
        tile(&game, 7, 5),
        unit(&rules, "core:unit/trireme"),
    );
    war(&mut game, &rules);
    until_active(&mut game, &rules, P1);
    let plan = plan_attack(game.view(), &rules, trireme, water, AttackKind::Melee).unwrap();
    assert_eq!(
        plan.defender_strength, 400,
        "embarked defense of the classical era"
    );
    // Ships cannot enter land.
    let land = tile(&game, 4, 4);
    assert!(game
        .execute(
            &rules,
            Command::MoveUnit {
                unit: trireme,
                destination: land
            }
        )
        .is_err());
    game.validate(&rules).unwrap();
}

/// City strength (§4.1): 800 + 40/citizen + walls 500 + 20 % of the
/// garrison + the technology share + 500 on hills; the strike ×75 %
/// without the building defense; 200 HP + 50 for walls.
#[test]
fn city_strength_follows_the_rules() {
    let (mut game, rules) = setup();
    let center = tile(&game, 8, 8);
    game.debug_set_terrain_batch(&rules, &[(center, HILLS)]);
    // The capital (with its Palace) elsewhere: the city under test has none.
    game.debug_found_city(&rules, P0, tile(&game, 2, 2));
    let city = game.debug_found_city(&rules, P0, center);
    let walls = rules
        .buildings
        .iter()
        .position(|b| b.key == "core:building/walls")
        .unwrap();
    game.debug_add_building(&rules, city, classic_rules::ids::BuildingKindId(walls as u16));
    game.debug_set_population(&rules, city, 6);
    game.debug_spawn_unit(&rules, P0, center, unit(&rules, "core:unit/spearman"));
    let mut learned = 0;
    for (i, _) in rules.technologies.iter().enumerate() {
        if learned == 20 {
            break;
        }
        game.debug_learn(&rules, P0, TechId(i as u16));
        learned += 1;
    }
    let auth = game.view().auth;
    let core = auth.cities().get(city).unwrap();
    // rules.md: 1977 + 500 hills = 2477.
    assert_eq!(
        classic_rules::combat::city::city_strength(auth, &rules, core),
        2477
    );
    // Unciv CityCombatant.getAttackingStrength: the whole strength (walls included) x 0.75.
    assert_eq!(
        classic_rules::combat::city::city_ranged_strength(auth, &rules, core),
        2477 * 75 / 100
    );
    assert_eq!(
        classic_rules::combat::city::max_hit_points(auth, &rules, core),
        250
    );
}

/// The city strikes once a turn (§4.3), not an ally.
#[test]
fn city_strikes_once_a_turn() {
    let (mut game, rules) = setup();
    let city = game.debug_found_city(&rules, P0, tile(&game, 8, 8));
    let target = tile(&game, 10, 8);
    let warrior = game.debug_spawn_unit(&rules, P1, target, unit(&rules, "core:unit/warrior"));
    assert!(
        game.execute(&rules, Command::CityStrike { city, target })
            .is_err(),
        "not at war"
    );
    war(&mut game, &rules);
    game.execute(&rules, Command::CityStrike { city, target })
        .unwrap();
    assert!(hp(&game, warrior).unwrap() < 100);
    assert!(
        game.execute(&rules, Command::CityStrike { city, target })
            .is_err(),
        "once a turn"
    );
    next_round(&mut game, &rules);
    game.execute(&rules, Command::CityStrike { city, target })
        .expect("again next turn");
}

fn captured_city(
    game: &mut GameState,
    rules: &CompiledRuleset,
    population: u32,
) -> (CityHandle, UnitHandle) {
    let center = tile(game, 8, 8);
    game.debug_found_city(rules, P0, tile(game, 2, 2));
    // The capital first: the captured city is not an original capital.
    game.debug_found_city(rules, P1, tile(game, 13, 13));
    let city = game.debug_found_city(rules, P1, center);
    game.debug_set_population(rules, city, population);
    let attacker = game.debug_spawn_unit(
        rules,
        P0,
        tile(game, 7, 8),
        unit(rules, "core:unit/warrior"),
    );
    war(game, rules);
    let max = classic_rules::combat::city::max_hit_points(
        game.view().auth,
        rules,
        game.view().auth.cities().get(city).unwrap(),
    );
    game.debug_set_city_damage(city, max - 1);
    game.execute(rules, Command::AttackCity { attacker, city })
        .unwrap();
    (city, attacker)
}

/// Capture keeps half the population and waits for the decision; razing
/// burns a citizen per turn and removes the city.
#[test]
fn captured_city_is_razed_citizen_by_citizen() {
    let (mut game, rules) = setup();
    let (city, _) = captured_city(&mut game, &rules, 4);
    let core = game.view().auth.cities().get(city).unwrap();
    assert_eq!(core.owner, P0);
    assert_eq!(core.population, 2, "half the population stays");
    assert_eq!(core.status, CityStatus::AwaitingDecision);
    assert_eq!(core.resistance_turns, 2);
    game.execute(
        &rules,
        Command::DecideCity {
            player: P0,
            city,
            decision: CityDecision::Raze,
        },
    )
    .unwrap();
    for _ in 0..3 {
        next_round(&mut game, &rules);
    }
    assert!(game.view().auth.cities().get(city).is_none(), "razed");
    let center = tile(&game, 8, 8);
    assert_eq!(
        game.view().auth.map().owner(center),
        Some(None),
        "its land is free"
    );
    game.validate(&rules).unwrap();
}

/// An undecided capture becomes a puppet at the end of the turn; an
/// annexed city is occupied.
#[test]
fn undecided_capture_becomes_a_puppet() {
    let (mut game, rules) = setup();
    let (city, _) = captured_city(&mut game, &rules, 3);
    until_active(&mut game, &rules, P1);
    assert_eq!(
        game.view().auth.cities().get(city).unwrap().status,
        CityStatus::Puppet
    );
    let (mut game2, rules2) = setup();
    let (city2, _) = captured_city(&mut game2, &rules2, 3);
    game2
        .execute(
            &rules2,
            Command::DecideCity {
                player: P0,
                city: city2,
                decision: CityDecision::Annex,
            },
        )
        .unwrap();
    assert_eq!(
        game2.view().auth.cities().get(city2).unwrap().status,
        CityStatus::Occupied
    );
}

/// The first city of a major nation is its original capital and
/// gets the palace; it cannot be razed.
#[test]
fn original_capital_has_the_palace_and_cannot_be_razed() {
    let (mut game, rules) = setup();
    game.debug_spawn_unit(
        &rules,
        P0,
        tile(&game, 1, 1),
        unit(&rules, "core:unit/warrior"),
    );
    let settler = game.debug_spawn_unit(
        &rules,
        P1,
        tile(&game, 8, 8),
        unit(&rules, "core:unit/settler"),
    );
    until_active(&mut game, &rules, P1);
    game.execute(&rules, Command::FoundCity { settler })
        .unwrap();
    let auth = game.view().auth;
    let city = auth.indexes().cities_of(P1)[0];
    let core = auth.cities().get(city).unwrap();
    assert!(core.original_capital);
    let palace = rules
        .buildings
        .iter()
        .position(|b| b.key == "core:building/palace")
        .unwrap();
    assert!(core
        .buildings
        .contains(&classic_rules::ids::BuildingKindId(palace as u16)));
    until_active(&mut game, &rules, P0);
    game.debug_found_city(&rules, P0, tile(&game, 2, 2));
    let attacker = game.debug_spawn_unit(
        &rules,
        P0,
        tile(&game, 7, 8),
        unit(&rules, "core:unit/warrior"),
    );
    war(&mut game, &rules);
    let max = classic_rules::combat::city::max_hit_points(
        game.view().auth,
        &rules,
        game.view().auth.cities().get(city).unwrap(),
    );
    game.debug_set_city_damage(city, max - 1);
    game.execute(&rules, Command::AttackCity { attacker, city })
        .unwrap();
    let raze = game.execute(
        &rules,
        Command::DecideCity {
            player: P0,
            city,
            decision: CityDecision::Raze,
        },
    );
    assert!(raze.is_err(), "an original capital cannot be razed");
    let core = game.view().auth.cities().get(city).unwrap();
    assert!(
        !core
            .buildings
            .contains(&classic_rules::ids::BuildingKindId(palace as u16)),
        "the palace is lost"
    );
}

/// Combat state survives save and load with the same world hash.
#[test]
fn combat_state_survives_save_and_load() {
    let (mut game, rules) = setup();
    let (city, attacker) = captured_city(&mut game, &rules, 4);
    game.debug_set_experience(attacker, 17);
    let save = classic_rules::persistence::encode::encode(&game, &rules);
    let loaded = classic_rules::persistence::decode::decode(&save, &rules).unwrap();
    assert_eq!(authoritative_hash(&loaded), authoritative_hash(&game));
    let core = loaded
        .view()
        .auth
        .cities()
        .iter()
        .find(|(_, c)| c.status == CityStatus::AwaitingDecision);
    assert!(core.is_some());
    let _ = city;
}

/// A nuclear missile destroys units at its target and leaves fallout
/// (§2.8); it needs war with everyone it hits.
#[test]
fn nuclear_strike_destroys_and_leaves_fallout() {
    let (mut game, rules) = setup();
    game.debug_found_city(&rules, P0, tile(&game, 2, 2));
    let missile = game.debug_spawn_unit(
        &rules,
        P0,
        tile(&game, 2, 2),
        unit(&rules, "core:unit/nuclear-missile"),
    );
    let target = tile(&game, 8, 6);
    let victim = game.debug_spawn_unit(&rules, P1, target, unit(&rules, "core:unit/warrior"));
    // A nuclear strike aims at ground the player knows.
    game.debug_reveal(P0, &[target]);
    game.debug_spawn_unit(
        &rules,
        P1,
        tile(&game, 14, 14),
        unit(&rules, "core:unit/warrior"),
    );
    assert!(
        game.execute(
            &rules,
            Command::Nuke {
                unit: missile,
                target
            }
        )
        .is_err(),
        "not at war"
    );
    war(&mut game, &rules);
    game.execute(
        &rules,
        Command::Nuke {
            unit: missile,
            target,
        },
    )
    .unwrap();
    assert!(hp(&game, victim).is_none());
    assert!(hp(&game, missile).is_none(), "the missile is spent");
    let fallout = rules
        .features
        .iter()
        .position(|f| f.key == "core:feature/fallout")
        .unwrap();
    assert_eq!(
        game.view().auth.map().feature(target),
        Some(Some(classic_rules::ids::FeatureId(fallout as u16)))
    );
    game.validate(&rules).unwrap();
}

/// A bomber strikes a unit; an enemy fighter on interception duty may
/// intercept (§2.7). The strike costs the bomber its turn.
#[test]
fn bomber_strikes_and_takes_the_answer() {
    let (mut game, rules) = setup();
    let base = game.debug_found_city(&rules, P0, tile(&game, 3, 3));
    let bomber = game.debug_spawn_unit(
        &rules,
        P0,
        tile(&game, 3, 3),
        unit(&rules, "core:unit/bomber"),
    );
    let target = tile(&game, 8, 8);
    let infantry = game.debug_spawn_unit(&rules, P1, target, unit(&rules, "core:unit/warrior"));
    // A scout sees the target (air strikes need a visible target).
    game.debug_spawn_unit(
        &rules,
        P0,
        tile(&game, 8, 6),
        unit(&rules, "core:unit/scout"),
    );
    war(&mut game, &rules);
    game.execute(
        &rules,
        Command::RangedAttack {
            unit: bomber,
            target,
        },
    )
    .unwrap();
    assert!(hp(&game, infantry).map(|h| h < 100).unwrap_or(true));
    assert_eq!(
        game.view()
            .auth
            .units()
            .get(bomber)
            .unwrap()
            .core
            .moves_left,
        0
    );
    // Aircraft cannot walk.
    let _ = base;
    assert!(game
        .execute(
            &rules,
            Command::MoveUnit {
                unit: bomber,
                destination: tile(&game, 4, 3)
            }
        )
        .is_err());
}

/// Great general aura (+15 %) for land units within two tiles.
#[test]
fn great_general_aura_adds_fifteen_percent() {
    let (mut game, rules) = setup();
    let a = tile(&game, 4, 4);
    let d = tile(&game, 5, 4);
    let warrior = game.debug_spawn_unit(&rules, P0, a, unit(&rules, "core:unit/warrior"));
    game.debug_spawn_unit(&rules, P1, d, unit(&rules, "core:unit/warrior"));
    war(&mut game, &rules);
    let plain = plan_attack(game.view(), &rules, warrior, d, AttackKind::Melee).unwrap();
    game.debug_spawn_unit(
        &rules,
        P0,
        tile(&game, 3, 4),
        unit(&rules, "core:unit/great-general"),
    );
    let led = plan_attack(game.view(), &rules, warrior, d, AttackKind::Melee).unwrap();
    assert_eq!(led.attacker_strength, plain.attacker_strength * 115 / 100);
}

/// The AI's combat helpers act on the public previews: the archer shoots
/// the visible enemy, the warrior charges, the captured city is decided.
#[test]
fn ai_combat_helpers_use_the_previews() {
    let (mut game, rules) = setup();
    let archer = game.debug_spawn_unit(
        &rules,
        P0,
        tile(&game, 4, 4),
        unit(&rules, "core:unit/archer"),
    );
    let warrior = game.debug_spawn_unit(
        &rules,
        P0,
        tile(&game, 5, 5),
        unit(&rules, "core:unit/warrior"),
    );
    let foe = tile(&game, 6, 4);
    game.debug_spawn_unit(&rules, P1, foe, unit(&rules, "core:unit/warrior"));
    war(&mut game, &rules);
    let obs = game.ai_observation(&rules, P0);
    let shots = classic_rules::ai::combat::ranged_orders(&obs, &rules);
    assert!(
        matches!(shots.as_slice(), [Command::RangedAttack { unit, target }] if *unit == archer && *target == foe),
        "{shots:?}"
    );
    let charges = classic_rules::ai::combat::melee_orders(&obs, &rules);
    assert!(
        charges
            .iter()
            .any(|c| matches!(c, Command::AttackUnit { attacker, .. } if *attacker == warrior)),
        "{charges:?}"
    );
    for cmd in shots.into_iter().chain(charges) {
        game.execute(&rules, cmd).unwrap();
    }
    let (mut game, rules) = setup();
    let (city, _) = captured_city(&mut game, &rules, 3);
    let decisions = classic_rules::ai::combat::conquest_decisions(&game.ai_observation(&rules, P0));
    assert_eq!(decisions.len(), 1);
    for cmd in decisions {
        game.execute(&rules, cmd).unwrap();
    }
    assert_eq!(
        game.view().auth.cities().get(city).unwrap().status,
        CityStatus::Puppet
    );
}

/// The conquest variants (classic/docs/ai-conquest-payoff.md "The priced decision"):
/// `puppet-all` puppets; `puppet-price` prices the three keepings from the rules' data
/// (the exempting building is found by its effect), the annex below the puppet (more
/// unrest, a courthouse to build, one city more in the research and policy cost), the
/// raze open (not an original capital), and its decision is one the rules accept.
#[test]
fn the_conquest_variants_decide_a_captured_city() {
    use classic_rules::ai::conquest::{decisions, exempting_buildings, priced_case, Conquest};
    use classic_rules::ai::value::{PriceModel, Prices, SciencePricing};
    let (mut game, rules) = setup();
    let (city, _) = captured_city(&mut game, &rules, 8);
    let obs = game.ai_observation(&rules, P0);
    let exempting: Vec<&str> = exempting_buildings(&rules).map(|b| rules.buildings[b.0 as usize].key.as_str()).collect();
    assert!(exempting.contains(&"core:building/courthouse"), "{exempting:?}");
    let puppets = decisions(&obs, &rules, None, Conquest::PuppetAll);
    assert!(matches!(puppets.as_slice(), [Command::DecideCity { decision: CityDecision::Puppet, .. }]), "{puppets:?}");
    let prices = Prices::of_effects(&obs, &rules, PriceModel::Uses, SciencePricing::Reach, true);
    let own = obs.own_cities.iter().find(|c| c.handle == city).unwrap();
    let case = priced_case(&obs, &rules, &prices, obs.economy.happiness.excess(), own);
    assert!(case.annex.is_some_and(|a| a < case.puppet), "{case:?}");
    assert!(case.raze.is_some(), "{case:?}");
    let commands = decisions(&obs, &rules, Some(&prices), Conquest::Priced);
    assert_eq!(commands.len(), 1);
    for cmd in commands {
        game.execute(&rules, cmd).unwrap();
    }
    assert_ne!(game.view().auth.cities().get(city).unwrap().status, CityStatus::AwaitingDecision);
}
