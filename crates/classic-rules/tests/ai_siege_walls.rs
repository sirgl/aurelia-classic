//! The siege of a walled city by `siege-walls` (`ai::siege_walls`,
//! classic/docs/ai-siege-walls.md): the army gathers out of the city's reach
//! and strikes together, every shooter in one turn; `standard` sends its units
//! up as they come and does not take the city. The truth is read only to set
//! the scene and judge it.

use classic_content::core::core_rules;
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::siege_walls::{arrival, simulate, waited, Awaited, CityModel, Fighter, MAX_WAIT, STRIKE_WINDOW};
use classic_rules::ai::state::{ObjectiveKind, SeenCity, WarObjective};
use classic_rules::ai::trace::Outcome;
use classic_rules::ai::variant::Variant;
use classic_rules::ai::{AiSettings, UnitTask};
use classic_rules::classic::Classic;
use kernel::ai::driver::AiDriver;
use classic_rules::ids::{BuildingKindId, PlayerId, TerrainId, TileId, UnitKindId};
use classic_rules::math::HexCoord;
use classic_rules::{compile_rules, ClassicGame, Command, CompiledRuleset, GameConfig, GameState};
use kernel::ai::seats::Seated;

const AI: PlayerId = PlayerId(0);
const ENEMY: PlayerId = PlayerId(1);
const GRASSLAND: u16 = 1;
/// Rounds the siege is played.
const ROUNDS: u32 = 30;

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
}

fn unit_kind(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap() as u16)
}

/// Open grassland. The enemy's capital of six at (20, 10) with walls (250 hit
/// points), an archer and a spearman inside; the AI's city far west, and its
/// army eight tiles from the walls, every unit on an assault of the capital:
/// three catapults (they set up before they shoot), two archers, three
/// swordsmen on iron. At war.
fn scene(rules: &CompiledRuleset) -> (GameState, TileId) {
    let mut game = GameState::new(rules, GameConfig { players: 2, width: 34, height: 20, rng_seed: 17 });
    let dims = game.view().auth.map().dimensions;
    let land: Vec<(TileId, TerrainId)> = (0..dims.tile_count() as u32).map(|t| (TileId(t), TerrainId(GRASSLAND))).collect();
    game.debug_set_terrain_batch(rules, &land);
    game.debug_found_city(rules, AI, tile(&game, 4, 10));
    let iron = rules.resources.iter().position(|r| r.key == "core:resource/iron").unwrap();
    game.debug_set_deposit(rules, tile(&game, 4, 10), classic_rules::ids::ResourceId(iron as u16), 6);
    let bronze = rules.technologies.iter().position(|t| t.key == "core:tech/bronze-working").unwrap();
    game.debug_learn(rules, AI, classic_rules::ids::TechId(bronze as u16));
    let target = tile(&game, 20, 10);
    let city = game.debug_found_city(rules, ENEMY, target);
    game.debug_set_population(rules, city, 6);
    let walls = rules.buildings.iter().position(|b| b.key == "core:building/walls").unwrap();
    game.debug_complete_building(rules, city, BuildingKindId(walls as u16));
    game.debug_spawn_unit(rules, ENEMY, target, unit_kind(rules, "core:unit/archer"));
    game.debug_spawn_unit(rules, ENEMY, tile(&game, 21, 10), unit_kind(rules, "core:unit/spearman"));
    let archer = unit_kind(rules, "core:unit/archer");
    let catapult = unit_kind(rules, "core:unit/catapult");
    let swordsman = unit_kind(rules, "core:unit/swordsman");
    for (q, r) in [(12, 9), (12, 10), (12, 11)] {
        game.debug_spawn_unit(rules, AI, tile(&game, q, r), catapult);
    }
    for (q, r) in [(11, 10), (11, 11)] {
        game.debug_spawn_unit(rules, AI, tile(&game, q, r), archer);
    }
    for (q, r) in [(13, 9), (13, 10), (13, 11)] {
        game.debug_spawn_unit(rules, AI, tile(&game, q, r), swordsman);
    }
    let everything: Vec<TileId> = (0..dims.tile_count() as u32).map(TileId).collect();
    game.debug_reveal(AI, &everything);
    game.debug_meet(AI, ENEMY);
    game.debug_earn(AI, 3_000);
    game.debug_earn(ENEMY, 300);
    game.execute(rules, Command::DeclareWar { player: AI, enemy: ENEMY }).unwrap();
    (game, target)
}

/// What a siege under `variant` did.
#[derive(Debug)]
struct Siege {
    /// The round the AI took the capital.
    taken: Option<u32>,
    /// Shots at the capital by round of the AI's turns.
    volleys: Vec<(u32, usize)>,
    /// The AI's military units within the city's strike at the end of its
    /// turns before its first shot at the city, by round.
    exposed_before: Vec<(u32, usize)>,
}

fn siege(variant: Variant) -> Siege {
    let rules = compile_rules(core_rules()).unwrap();
    let (mut game, target) = scene(&rules);
    let settings = AiSettings::for_rules(&rules, rules.default_difficulty());
    let mind = Seated::new(ClassicMind::new(Variant::Standard), vec![(AI, ClassicMind::new(variant))]);
    let mut driver = AiDriver::<Classic, Seated<ClassicMind>>::with_mind(settings, Vec::new(), mind);
    driver.set_trace_capacity(1_000_000);
    let start = game.view().auth.turn().round;
    {
        let state = driver.state_mut(AI, start);
        state.see_city(SeenCity { tile: target, owner: ENEMY, population: 6, last_seen_round: start });
        state.relation_mut(ENEMY).objective = Some(WarObjective { tile: target, kind: ObjectiveKind::City, chosen_round: start });
    }
    let auth = game.view().auth;
    let army: Vec<_> = auth.indexes().units_of(AI).iter().filter_map(|h| auth.units().get(*h)).map(|u| u.core.id).collect();
    for unit in army {
        driver.state_mut(AI, start).set_task(unit, UnitTask::Assault { target }, start);
    }
    let mut out = Siege { taken: None, volleys: Vec::new(), exposed_before: Vec::new() };
    let mut seen = 0;
    while game.view().auth.turn().round < start + ROUNDS && out.taken.is_none() && game.winner().is_none() {
        let round = game.view().auth.turn().round;
        driver.run(&mut game, &rules, u32::MAX);
        let shots = driver
            .trace()
            .entries()
            .filter(|e| e.player == AI && e.outcome == Outcome::Applied)
            .filter(|e| matches!(e.command, Some(Command::RangedAttack { target: t, .. }) if t == target))
            .count();
        out.volleys.push((round, shots - seen));
        seen = shots;
        let auth = game.view().auth;
        let dims = auth.map().dimensions;
        if shots == 0 {
            let near = auth.indexes().units_of(AI).iter().filter_map(|h| auth.units().get(*h)).filter(|u| dims.distance(u.core.tile, target).is_some_and(|d| d <= 2)).count();
            out.exposed_before.push((round, near));
        }
        let owner = auth.indexes().city_on_tile(target).and_then(|c| auth.cities().get(c)).map(|c| c.owner);
        if owner == Some(AI) {
            out.taken = Some(round);
        }
    }
    out
}

/// `standard` (since the seventh promotion; `siege-walls` its alias) gathers
/// out of the walls' strike, steps in and shoots with every archer in one
/// turn, and takes the capital; `pre-siege`, the standard before, with the
/// same army does not take it in the same rounds.
#[test]
fn the_army_musters_out_of_reach_strikes_together_and_takes_the_walls() {
    let walls = siege(Variant::Standard);
    eprintln!("standard: {walls:?}");
    let alias = siege(Variant::SiegeWalls);
    assert_eq!(format!("{alias:?}"), format!("{walls:?}"), "`siege-walls` plays as `standard`");
    let standard = siege(Variant::PreSiege);
    eprintln!("pre-siege: {standard:?}");
    // Out of the walls' strike until the strike: one member at most (the one that looks at
    // the city out of sight).
    assert!(walls.exposed_before.iter().all(|(_, n)| *n <= 1), "the army waits out of the walls' strike: {walls:?}");
    // The strike: every shooter fires within its first two turns (the catapults set up in
    // the first), most of them in one volley; no shooter comes up alone.
    let first = walls.volleys.iter().position(|(_, n)| *n > 0).expect("siege-walls shoots the city");
    let opening: usize = walls.volleys[first..(first + 2).min(walls.volleys.len())].iter().map(|(_, n)| n).sum();
    assert!(opening >= 5, "all five shooters fire in the strike's first two turns: {walls:?}");
    assert!(walls.volleys.iter().any(|(_, n)| *n >= 4), "a volley of four shooters at once: {walls:?}");
    assert!(walls.taken.is_some(), "siege-walls takes the capital: {walls:?}");
    assert!(standard.taken.is_none(), "pre-siege does not take it in {ROUNDS} rounds: {standard:?}");
}

/// `siege-join` (classic/docs/ai-siege-walls.md, "Why `standard` fails the
/// training grounds' siege") is `pre-join` but for who besieges: with every
/// unit of the army on its assault, as here, it plays the same siege. Since the
/// ninth promotion `standard` carries its switches and `siege-join` is its alias.
#[test]
fn siege_join_plays_the_tasked_siege_as_pre_join() {
    use classic_rules::ai::siege_walls::SiegeJoin;
    assert!(Variant::SiegeJoin.siege_join() == SiegeJoin::ALL && Variant::SiegeJoin.siege_walls());
    assert!(Variant::Standard.siege_join() == SiegeJoin::ALL && Variant::PreJoin.siege_join() == SiegeJoin::NONE && Variant::PreJoin.siege_walls());
    // Unciv's modules inside `standard` (`std-unciv-*`) besiege as it does, and so do
    // `pre-camps` and `pre-workers` (the standards before the eleventh and the tenth promotion) and the candidates built on it
    // that change one thing elsewhere (`fog-march`, the nets' candidates and `standard-scripted`, the standard
    // before the twelfth promotion, too).
    let on_standard = [Variant::PreCamps, Variant::PreWorkers, Variant::NoIdleCities, Variant::ScoutEarly, Variant::WorkersFit, Variant::TradeFit, Variant::ArmyFit, Variant::Camps, Variant::ArmyCamps, Variant::PromoFit, Variant::HealFirst, Variant::FogMarch, Variant::StrategyNet, Variant::UnitNet, Variant::Hybrid, Variant::StandardScripted, Variant::PreCsGifts];
    assert!(on_standard.iter().all(|v| v.siege_join() == Variant::Standard.siege_join()));
    assert_eq!(Variant::ALL.iter().filter(|v| v.siege_join().any() && !v.unciv().parts().any() && !on_standard.contains(v)).count(), 2, "only `standard` and its alias `siege-join` carry the switches");
    assert_eq!(Variant::from_name("siege-join"), Some(Variant::SiegeJoin));
    let before = siege(Variant::PreJoin);
    let join = siege(Variant::SiegeJoin);
    assert_eq!(format!("{join:?}"), format!("{before:?}"), "`siege-join` besieges a tasked army as `pre-join`");
    assert_eq!(format!("{:?}", siege(Variant::Standard)), format!("{join:?}"), "`standard` besieges as its alias `siege-join`");
}

/// The seventh promotion: `standard` besieges walled cities, `siege-walls`
/// stays its alias (playing as `pre-puppet` since the eighth promotion), `net`
/// plays on it, `pre-siege` is the standard before.
#[test]
fn standard_besieges_walls_and_pre_siege_is_the_standard_before() {
    assert_eq!(Variant::ALL[0], Variant::Standard);
    // The controls of the eighth to the eleventh promotion, `pre-puppet`, `pre-join`,
    // `pre-workers` and `pre-camps`, came in before it.
    assert_eq!(Variant::ALL[5], Variant::PreSiege);
    assert_eq!(Variant::from_name("pre-siege"), Some(Variant::PreSiege));
    assert!(Variant::Standard.siege_walls() && Variant::SiegeWalls.siege_walls() && Variant::Net.siege_walls());
    assert!(Variant::PrePuppet.siege_walls() && Variant::PuppetAll.siege_walls() && Variant::PreJoin.siege_walls());
    assert!(!Variant::PreSiege.siege_walls() && !Variant::PreTall.siege_walls());
    assert_eq!(ClassicMind::new(Variant::SiegeWalls).variant(), Variant::SiegeWalls);
    assert_eq!(ClassicMind::new(Variant::PreSiege).variant(), Variant::PreSiege);
}

/// The measured arrival: a member is likelier to be with the group the more
/// rounds it is given beyond its path's turns, and less likely when it fights
/// where it stands, is wounded, or walks through the enemy's reach; a chance
/// always.
#[test]
fn the_arrival_falls_with_the_engagement_and_rises_with_the_slack() {
    let free = Awaited { turns: 3, engaged: false, wounded: false, threat: 0.0 };
    let mut last = 0.0;
    for by in 0..=12 {
        let p = arrival(&free, by);
        assert!((0.0..=1.0).contains(&p), "{by}: {p}");
        assert!(p >= last, "more rounds, no less arrival: {by}: {p} < {last}");
        last = p;
    }
    let p = arrival(&free, 5);
    for worse in [Awaited { engaged: true, ..free }, Awaited { wounded: true, ..free }, Awaited { threat: 1.0, ..free }, Awaited { turns: 8, ..free }] {
        let q = arrival(&worse, 5 + worse.turns - free.turns);
        assert!(q < p, "{worse:?}: {q} not below {p}");
    }
}

/// Units unlikely to arrive are not waited for: a group that can take the
/// worn city now strikes with what is there when the reinforcement it would
/// wait for is engaged far away; the same reinforcement on a free road a
/// turn out is worth the wait for a city the group alone cannot take.
#[test]
fn the_group_strikes_with_what_is_there_when_the_awaited_will_not_come() {
    let shooter = |ready: u32, arrive: f64| Fighter { ranged: true, hp: 100, worth: 120, dealt: (32, 36, 40), taken: (0, 0, 0), city_hit: (18, 22, 26), ready, exposed: ready, covered: false, arrive };
    let melee = Fighter { ranged: false, hp: 100, worth: 140, dealt: (35, 40, 45), taken: (20, 25, 30), city_hit: (12, 15, 18), ready: 0, exposed: 0, covered: false, arrive: 1.0 };
    let city = |hp: u16| CityModel { hp, cap: 250, heal: 20, value: 1120, time_cost: 37, extra_hits: 0 };
    // Two shooters and a swordsman at the walls of a worn city; four shooters on their way.
    let far = Awaited { turns: 6, engaged: true, wounded: false, threat: 0.8 };
    let near = Awaited { turns: 1, engaged: false, wounded: false, threat: 0.0 };
    let force = |way: Awaited| {
        let mut f = vec![shooter(0, 1.0), shooter(0, 1.0), melee];
        let mut ways = vec![None, None, None];
        for _ in 0..4 {
            f.push(shooter(way.turns, arrival(&way, way.turns)));
            ways.push(Some(way));
        }
        (f, ways)
    };
    let best_wait = |fighters: &[Fighter], ways: &[Option<Awaited>], hp: u16| {
        (1..=MAX_WAIT)
            .map(|k| {
                let (later, c) = waited(fighters, &vec![false; fighters.len()], ways, &city(hp), k);
                simulate(&later, &c, k, STRIKE_WINDOW).value
            })
            .fold(f64::MIN, f64::max)
    };
    // Now: only the group strikes (the others are not at the walls).
    let (f, ways) = force(far);
    let now = simulate(&f[..3], &city(90), 0, STRIKE_WINDOW);
    assert!(now.value > 0.0, "the group takes the worn city: {now:?}");
    assert!(now.value >= best_wait(&f, &ways, 90), "the engaged reinforcement is not waited for: now {now:?}");
    // A full city the group alone cannot take: waiting for the free reinforcement pays.
    let (f, ways) = force(near);
    let alone = simulate(&f[..3], &city(250), 0, STRIKE_WINDOW);
    assert!(best_wait(&f, &ways, 250) > alone.value.max(0.0), "the near reinforcement is worth the wait: alone {alone:?}");
}

/// The value model: a complete force strikes now; a lone shooter waits; a
/// worn city is struck again in place rather than left to heal.
#[test]
fn the_strike_is_chosen_by_its_expected_value() {
    let shooter = |hp: u16, ready: u32| Fighter { ranged: true, hp, worth: 120, dealt: (32, 36, 40), taken: (0, 0, 0), city_hit: (18, 22, 26), ready, exposed: ready, covered: false, arrive: 1.0 };
    let melee = Fighter { ranged: false, hp: 100, worth: 140, dealt: (35, 40, 45), taken: (20, 25, 30), city_hit: (12, 15, 18), ready: 0, exposed: 0, covered: false, arrive: 1.0 };
    let city = |hp: u16| CityModel { hp, cap: 250, heal: 20, value: 1120, time_cost: 37, extra_hits: 1 };
    let mut force: Vec<Fighter> = (0..5).map(|_| shooter(100, 0)).collect();
    force.push(melee);
    let strike = simulate(&force, &city(250), 0, STRIKE_WINDOW);
    assert!(strike.capture > 0.9 && strike.value > 0.0, "{strike:?}");
    let lone = simulate(&[shooter(100, 0), melee], &city(250), 0, STRIKE_WINDOW);
    assert!(lone.value < strike.value && lone.capture < 0.5, "{lone:?}");
    let worn: Vec<Fighter> = vec![shooter(55, 0), shooter(60, 0), shooter(100, 0), melee];
    let press = simulate(&worn, &city(80), 0, STRIKE_WINDOW);
    let (healed, later) = waited(&worn, &[false; 4], &[None; 4], &city(80), 4);
    let heal = simulate(&healed, &later, 4, STRIKE_WINDOW);
    assert!(press.value > heal.value, "press {press:?} against heal {heal:?}");
}
