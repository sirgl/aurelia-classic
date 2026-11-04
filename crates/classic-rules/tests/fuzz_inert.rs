//! Fuzz of the effects brought from inert to typed (classic/docs/classic/implemented.md)
//! on small classic worlds: random policies adopted, random buildings and
//! wonders completed (OnBuilt included), populations changed, technologies
//! learned, AI and random commands; the world is validated after every
//! commit (derived yields and the other caches against their rebuild
//! oracle), rejected commands leave the hash untouched, save → load keeps
//! the hash. Public API only. A family that becomes typed adds what drives
//! it here (a command, a debug action) so its consumers run under the
//! oracle.

use classic_rules::ClassicGame;
use classic_content::core::core_rules;
use classic_rules::ai::{GreedyPlanner, Planner};
use classic_rules::ids::{
    BeliefId, BuildingKindId, CityHandle, FeatureId, ImprovementId, PlayerId, PolicyId, ProductionItem,
    TechId, TileId, UnitHandle, UnitKindId,
};
use classic_rules::rules::content::ProjectId;
use classic_rules::world::city_economy::Process;
use classic_rules::derived::great_people::GreatPersonAction;
use classic_rules::navigation::cost::enter_cost_on_map;
use classic_rules::rules::content::NaturalWonderId;
use classic_rules::persistence::{decode::decode, encode::encode};
use classic_rules::replay::authoritative_hash;
use classic_rules::scenario::{
    generate_new_game, generate_new_game_with, NewGameOptions, ScenarioConfig,
};
use classic_rules::{compile_rules, Command, CompiledRuleset, GameState};
use classic_rules::world::player::ClassicPlayers;

struct FuzzRng(u64);

impl FuzzRng {
    fn next(&mut self) -> u64 {
        let mut x = self.0.max(1);
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

fn world(rng: &mut FuzzRng, rules: &CompiledRuleset) -> GameState {
    let cfg = ScenarioConfig {
        seed: rng.next(),
        players: 2 + rng.below(2) as u8,
        width: 20 + rng.below(9) as u32,
        height: 16 + rng.below(9) as u32,
        cities: 2,
        units: 2,
        water_ratio: rng.below(25) as u8,
    };
    let mut game = generate_new_game(rules, &cfg);
    // Random nations: their traits (grants, bans, founding) run too.
    for p in 0..cfg.players {
        let nation = rng.below(rules.nations.len() as u64) as u8;
        game.debug_set_player_nation(PlayerId(p), nation);
        game.debug_sources_changed(rules, PlayerId(p));
    }
    game
}

fn random_city(rng: &mut FuzzRng, game: &GameState) -> Option<CityHandle> {
    let active = game.view().auth.active_player();
    let cities = game.view().auth.indexes().cities_of(active).to_vec();
    cities.get(rng.below(cities.len().max(1) as u64) as usize).copied()
}

/// A unit of another player (barbarians included) next to which the
/// active player may put a warrior: (victim, free land tile beside it).
fn skirmish_site(rng: &mut FuzzRng, game: &GameState) -> Option<(UnitHandle, TileId)> {
    let auth = game.view().auth;
    let active = auth.active_player();
    let enemies: Vec<(UnitHandle, TileId)> = auth
        .units()
        .iter_handles()
        .filter(|(_, c)| c.owner != active)
        .map(|(h, c)| (h, c.tile))
        .collect();
    let (victim, at) = *enemies.get(rng.below(enemies.len().max(1) as u64) as usize)?;
    let map = auth.map();
    let free = map.dimensions.neighbors(at).into_iter().find(|t| {
        enter_cost_on_map(map, *t).is_some()
            && auth.indexes().units_on(*t).is_empty()
            && auth.indexes().city_on_tile(*t).is_none()
            && map.owner(*t).flatten().is_none_or(|o| o == active)
    })?;
    Some((victim, free))
}

/// A kill for the effects of kills: a wounded unit of another player and
/// a fresh warrior of the active player beside it, at war; the attack goes
/// through the command path.
fn skirmish(rng: &mut FuzzRng, game: &mut GameState, rules: &CompiledRuleset) -> Option<Command> {
    let (victim, at) = skirmish_site(rng, game)?;
    let auth = game.view().auth;
    let active = auth.active_player();
    let enemy = auth.units().get(victim)?.core.owner;
    if !auth.players().is_barbarian(enemy) && !auth.diplomacy().at_war(active, enemy) {
        game.debug_meet(active, enemy);
        game.debug_force_war(rules, active, enemy);
    }
    let warrior = rules.units.iter().position(|u| u.key == "core:unit/warrior")?;
    let attacker = game.debug_spawn_unit(rules, active, at, UnitKindId(warrior as u16));
    game.debug_set_unit_hp(victim, 1);
    Some(Command::AttackUnit { attacker, defender: victim })
}

/// A capture for the plunder effects: a city of another player at 0 HP
/// and a fresh warrior of the active player beside it, at war.
fn siege(rng: &mut FuzzRng, game: &mut GameState, rules: &CompiledRuleset) -> Option<Command> {
    let auth = game.view().auth;
    let active = auth.active_player();
    if !auth.players().is_major(active) {
        return None;
    }
    let cities: Vec<(CityHandle, PlayerId, TileId)> = auth
        .cities()
        .iter()
        .filter(|(_, c)| c.owner != active)
        .map(|(h, c)| (h, c.owner, c.tile))
        .collect();
    let (city, owner, at) = *cities.get(rng.below(cities.len().max(1) as u64) as usize)?;
    let map = auth.map();
    let free = map.dimensions.neighbors(at).into_iter().find(|t| {
        enter_cost_on_map(map, *t).is_some() && auth.indexes().units_on(*t).is_empty()
    })?;
    if !auth.diplomacy().at_war(active, owner) {
        game.debug_meet(active, owner);
        game.debug_force_war(rules, active, owner);
    }
    let warrior = rules.units.iter().position(|u| u.key == "core:unit/warrior")?;
    let attacker = game.debug_spawn_unit(rules, active, free, UnitKindId(warrior as u16));
    let auth = game.view().auth;
    let max = classic_rules::combat::city::max_hit_points(auth, rules, auth.cities().get(city)?);
    game.debug_set_city_damage(city, max);
    Some(Command::AttackCity { attacker, city })
}

/// An airlift of a random own unit between two own cities given airports
/// (rejected when the unit is not in a city or moved: the hash must hold).
fn airlift(rng: &mut FuzzRng, game: &mut GameState, rules: &CompiledRuleset) -> Option<Command> {
    let auth = game.view().auth;
    let active = auth.active_player();
    let cities = auth.indexes().cities_of(active).to_vec();
    let units = auth.indexes().units_of(active).to_vec();
    let unit = *units.get(rng.below(units.len().max(1) as u64) as usize)?;
    let to = *cities.get(rng.below(cities.len().max(1) as u64) as usize)?;
    let airport = rules.buildings.iter().position(|b| b.key == "core:building/airport")?;
    let airport = BuildingKindId(airport as u16);
    for city in cities {
        let has = game.view().auth.cities().get(city)?.buildings.contains(&airport);
        if !has {
            game.debug_add_building(rules, city, airport);
        }
    }
    Some(Command::Airlift { unit, city: to })
}

/// A pantheon of a random belief (God of War, Faith Healers, ...).
fn pantheon(rng: &mut FuzzRng, game: &mut GameState, rules: &CompiledRuleset) -> Option<Command> {
    let active = game.view().auth.active_player();
    if game.view().auth.module().religion().player(active).pantheon.is_some() {
        return None;
    }
    let pantheons: Vec<usize> = (0..rules.module.beliefs.len())
        .filter(|b| rules.module.beliefs[*b].key.starts_with("core:belief/pantheon/"))
        .collect();
    let belief = *pantheons.get(rng.below(pantheons.len().max(1) as u64) as usize)?;
    game.debug_grant_faith(active, 200);
    Some(Command::FoundPantheon { player: active, belief: BeliefId(belief as u16) })
}

/// A situation next to combat set up by debug actions, and the command
/// that drives its effects (kills, pantheons, captures, airlifts).
fn combat_action(rng: &mut FuzzRng, game: &mut GameState, rules: &CompiledRuleset) -> Option<Command> {
    match rng.below(4) {
        0 => skirmish(rng, game, rules),
        1 => pantheon(rng, game, rules),
        2 => siege(rng, game, rules),
        _ => airlift(rng, game, rules),
    }
}

/// One debug action that puts an effect source in play.
fn debug_action(rng: &mut FuzzRng, game: &mut GameState, rules: &CompiledRuleset) {
    let active = game.view().auth.active_player();
    match rng.below(10) {
        0 => {
            let policy = PolicyId(rng.below(rules.module.policies.len() as u64) as u16);
            game.debug_adopt_policy(rules, active, policy);
        }
        1 => {
            let Some(city) = random_city(rng, game) else { return };
            let building = BuildingKindId(rng.below(rules.buildings.len() as u64) as u16);
            let core = game.view().auth.cities().get(city).unwrap();
            if !core.buildings.contains(&building) {
                game.debug_complete_building(rules, city, building);
            }
        }
        2 => {
            let Some(city) = random_city(rng, game) else { return };
            game.debug_set_population(rules, city, 1 + rng.below(24) as u32);
        }
        3 => {
            let tech = TechId(rng.below(rules.technologies.len() as u64) as u16);
            game.debug_learn(rules, active, tech);
        }
        4 => wonder_resource(rng, game, rules),
        5 => {
            // As by research: OnTechnologyUnlocked, OnEraEntered, free
            // choices pruned, the map revealed (Satellites).
            let options = classic_rules::derived::grants::researchable(game.view().auth, rules, active);
            if let Some(tech) = options.get(rng.below(options.len().max(1) as u64) as usize) {
                game.debug_discover(rules, active, *tech);
            }
        }
        6 => {
            // A natural wonder comes into view (found at the end of the turn).
            let sites = game.view().auth.map().natural_wonder_tiles().to_vec();
            if let Some(site) = sites.get(rng.below(sites.len().max(1) as u64) as usize) {
                game.debug_reveal(active, &[*site]);
            }
        }
        7 => {
            // A nation's traits (Spain, Japan, Russia, the Celts ...);
            // a learned technology refreshes the derived yields.
            let nation = rng.below(rules.nations.len() as u64) as u8;
            game.debug_set_player_nation(active, nation);
            game.debug_sources_changed(rules, active);
            game.debug_learn(rules, active, TechId(0));
        }
        8 => tile_action(rng, game, rules),
        _ => league_action(rng, game, rules),
    }
}

/// A forest, a natural wonder or an improvement next to a random city
/// (tile conditions: plots, unimproved features, adjacent forests).
fn tile_action(rng: &mut FuzzRng, game: &mut GameState, rules: &CompiledRuleset) {
    let Some(city) = random_city(rng, game) else { return };
    let auth = game.view().auth;
    let centre = auth.cities().get(city).unwrap().tile;
    let around: Vec<_> = auth.map().dimensions.neighbors(centre).into_iter().collect();
    let Some(&tile) = around.get(rng.below(around.len() as u64) as usize) else { return };
    if auth.map().is_water(tile) {
        return;
    }
    match rng.below(3) {
        0 => {
            let forest = rules.features.iter().position(|f| f.key == "core:feature/forest").unwrap();
            game.debug_set_feature_batch(rules, &[(tile, FeatureId(forest as u16))]);
        }
        1 if !rules.natural_wonders.is_empty() => {
            let wonder = NaturalWonderId(rng.below(rules.natural_wonders.len() as u64) as u16);
            game.debug_set_natural_wonder_batch(rules, &[(tile, wonder)]);
        }
        _ => {
            let improvement = ImprovementId(rng.below(rules.improvements.len() as u64) as u16);
            game.debug_set_improvement_batch(rules, &[(tile, improvement)]);
        }
    }
}

fn found_league(game: &mut GameState, rules: &CompiledRuleset) {
    let active = game.view().auth.active_player();
    let printing = rules.technologies.iter().position(|t| t.module.allows_world_congress).unwrap();
    game.debug_learn(rules, active, TechId(printing as u16));
    // Each project needs a member who knows its technology (World's Fair:
    // Industrialization, International Games: Radio, ISS: Satellites).
    for project in &rules.module.projects {
        if let Some(tech) = project.league.as_ref().and_then(|l| l.tech_any_member) {
            game.debug_learn(rules, active, tech);
        }
    }
    game.debug_meet_all();
}

/// The World Congress founded (Printing Press, everyone met), a random
/// World Congress project started and contributed to, a city set on it.
fn league_action(rng: &mut FuzzRng, game: &mut GameState, rules: &CompiledRuleset) {
    let active = game.view().auth.active_player();
    if !game.view().auth.module().congress().founded() {
        found_league(game, rules);
        return;
    }
    let project = ProjectId(rng.below(rules.module.projects.len() as u64) as u16);
    game.debug_start_league_project(rules, project);
    if game.view().auth.module().congress().project(project).is_none() {
        return;
    }
    // Games end in a few dozen rounds once players are eliminated, so a
    // contribution is large enough for projects to finish and pay rewards.
    game.debug_contribute_to_league_project(rules, active, project, 500 + rng.below(3000) as u32);
    if let Some(city) = random_city(rng, game) {
        let order = Command::SetProcess { player: active, city, process: Some(Process::Contribute(project)) };
        let _ = game.execute(rules, order);
    }
}

/// A connected wonder resource (Marble with its quarry) on a tile up to 5
/// from a city: production toward wonders there changes (a tile beyond
/// the worked rings refreshes the city too).
fn wonder_resource(rng: &mut FuzzRng, game: &mut GameState, rules: &CompiledRuleset) {
    let Some(city) = random_city(rng, game) else { return };
    let Some(&resource) = rules.effect_index.wonder_resources.first() else { return };
    let Some(&improvement) = rules.resources[resource.0 as usize].improvements.first() else { return };
    let auth = game.view().auth;
    let center = auth.cities().get(city).unwrap().tile;
    let near = classic_rules::math::tiles_in_hex_radius(&auth.map().dimensions, center, 5);
    let tile = near[rng.below(near.len() as u64) as usize];
    if auth.map().is_water(tile) || auth.indexes().city_on_tile(tile).is_some() {
        return;
    }
    game.debug_set_resource_batch(rules, &[(tile, resource)]);
    game.debug_set_improvement_batch(rules, &[(tile, improvement)]);
}

/// A random order for a city's queue: enqueue, clear or buy the head.
fn production_command(rng: &mut FuzzRng, game: &GameState, rules: &CompiledRuleset) -> Option<Command> {
    let player = game.view().auth.active_player();
    let city = random_city(rng, game)?;
    Some(match rng.below(4) {
        0 => Command::ClearProduction { player, city },
        1 => Command::HurryProduction { player, city },
        2 => {
            let unit = rng.below(rules.units.len() as u64) as u16;
            Command::EnqueueProduction { player, city, item: ProductionItem::Unit(UnitKindId(unit)) }
        }
        _ => {
            let building = rng.below(rules.buildings.len() as u64) as u16;
            Command::EnqueueProduction { player, city, item: ProductionItem::Building(BuildingKindId(building)) }
        }
    })
}

/// Answers to the free choices the active player is owed (a random option).
fn free_choices(rng: &mut FuzzRng, game: &GameState, rules: &CompiledRuleset) -> Vec<Command> {
    let active = game.view().auth.active_player();
    let auth = game.view().auth;
    let mut out = Vec::new();
    let techs = classic_rules::derived::grants::next_technology_options(auth, rules, active);
    if let Some(tech) = techs.get(rng.below(techs.len().max(1) as u64) as usize) {
        out.push(Command::ChooseFreeTechnology { player: active, tech: *tech });
    }
    let kinds = classic_rules::derived::grants::next_great_person_options(auth, rules, active);
    if let Some(kind) = kinds.get(rng.below(kinds.len().max(1) as u64) as usize) {
        out.push(Command::ChooseFreeGreatPerson { player: active, kind: *kind });
    }
    out
}

fn commands(rng: &mut FuzzRng, game: &GameState, rules: &CompiledRuleset) -> Vec<Command> {
    let active = game.view().auth.active_player();
    match rng.below(100) {
        0..=39 => GreedyPlanner.plan(game.ai_observation(rules, active), rules),
        40..=54 => production_command(rng, game, rules).into_iter().collect(),
        55..=64 => free_choices(rng, game, rules),
        65..=74 => spy_order(rng, game).into_iter().collect(),
        // Pillage with a random unit (improvement pillage and repair).
        75..=79 => {
            let units = game.view().auth.indexes().units_of(active).to_vec();
            match units.get(rng.below(units.len().max(1) as u64) as usize) {
                Some(unit) => vec![Command::Pillage { unit: *unit }],
                None => Vec::new(),
            }
        }
        _ => vec![Command::EndTurn { player: active }],
    }
}

/// A random spy of the active player sent to a random city, sometimes as
/// a diplomat (steal rates, counter-espionage, spy pressure, delegates).
fn spy_order(rng: &mut FuzzRng, game: &GameState) -> Option<Command> {
    let auth = game.view().auth;
    let player = auth.active_player();
    let spies = auth.module().espionage().spies_of(player).len();
    if spies == 0 {
        return None;
    }
    let cities: Vec<CityHandle> = auth.cities().iter().map(|(h, _)| h).collect();
    let city = cities.get(rng.below(cities.len().max(1) as u64) as usize).copied();
    Some(Command::MoveSpy {
        player,
        spy: rng.below(spies as u64) as u8,
        city,
        diplomat: rng.below(4) == 0,
    })
}

fn pick<T: Copy>(rng: &mut FuzzRng, items: &[T]) -> Option<T> {
    items.get(rng.below(items.len().max(1) as u64) as usize).copied()
}

fn unit_kind(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap() as u16)
}

/// A city of another player than `player` (spreads, conversions).
fn foreign_city(rng: &mut FuzzRng, game: &GameState, player: PlayerId) -> Option<CityHandle> {
    let auth = game.view().auth;
    let cities: Vec<CityHandle> = auth.cities().iter().filter(|(_, c)| c.owner != player).map(|(h, _)| h).collect();
    pick(rng, &cities)
}

/// Religion drivers (workstream D): a prophet founds a religion with
/// random free beliefs (a bonus one too), a Reformation belief, a
/// missionary or a prophet spreads into a foreign city, an inquisitor
/// removes heresy, a great artist is expended, a war, roads between cities.
/// The units are placed by the harness; the commands are checked.
fn religion_commands(rng: &mut FuzzRng, game: &mut GameState, rules: &CompiledRuleset) -> Vec<Command> {
    use classic_rules::derived::religion::{free_beliefs, free_religions};
    use classic_rules::rules::content::BeliefCategory;
    let active = game.view().auth.active_player();
    let Some(city) = random_city(rng, game) else { return Vec::new() };
    let at = game.view().auth.cities().get(city).unwrap().tile;
    match rng.below(7) {
        0 => {
            let unit = game.debug_spawn_unit(rules, active, at, unit_kind(rules, "core:unit/great-prophet"));
            let auth = game.view().auth;
            let free = |c| free_beliefs(auth, rules, c);
            let (Some(religion), Some(founder), Some(follower)) = (
                pick(rng, &free_religions(auth, rules)),
                pick(rng, &free(BeliefCategory::Founder)),
                pick(rng, &free(BeliefCategory::Follower)),
            ) else {
                return Vec::new();
            };
            let pantheon = match auth.module().religion().player(active).pantheon {
                Some(_) => None,
                None => pick(rng, &free(BeliefCategory::Pantheon)),
            };
            let bonus = if rng.below(2) == 0 { pick(rng, &free(BeliefCategory::Enhancer)) } else { None };
            vec![Command::FoundReligion { unit, religion, founder, follower, pantheon, bonus }]
        }
        1 => {
            game.debug_adopt_policy(rules, active, policy_of(rules, "core:policy/piety/reformation"));
            let free = free_beliefs(game.view().auth, rules, BeliefCategory::Reformation);
            pick(rng, &free).map(|belief| Command::AddReformationBelief { player: active, belief }).into_iter().collect()
        }
        2 | 3 => {
            let Some(target) = foreign_city(rng, game, active) else { return Vec::new() };
            let next_to = game.view().auth.cities().get(target).unwrap().tile;
            let key = if rng.below(3) == 0 { "core:unit/great-prophet" } else { "core:unit/missionary" };
            let unit = game.debug_spawn_unit(rules, active, next_to, unit_kind(rules, key));
            vec![Command::SpreadReligion { unit }]
        }
        4 => {
            let unit = game.debug_spawn_unit(rules, active, at, unit_kind(rules, "core:unit/inquisitor"));
            vec![Command::RemoveHeresy { unit }]
        }
        5 => {
            let unit = game.debug_spawn_unit(rules, active, at, unit_kind(rules, "core:unit/great-artist"));
            vec![Command::GreatPersonAction { unit, action: GreatPersonAction::StartGoldenAge }]
        }
        _ => {
            if let Some(other) = foreign_city(rng, game, active) {
                let enemy = game.view().auth.cities().get(other).unwrap().owner;
                game.debug_force_war(rules, active, enemy);
            }
            let dims = game.view().auth.map().dimensions;
            let road: Vec<_> = dims.neighbors(at).into_iter().collect();
            game.debug_set_road_batch(rules, &road);
            Vec::new()
        }
    }
}

fn policy_of(rules: &CompiledRuleset, key: &str) -> PolicyId {
    PolicyId(rules.module.policies.iter().position(|p| p.key == key).unwrap() as u16)
}

fn check(game: &mut GameState, rules: &CompiledRuleset, cmd: Command) {
    let before = authoritative_hash(game);
    match game.execute(rules, cmd.clone()) {
        Ok(_) => game
            .validate(rules)
            .unwrap_or_else(|e| panic!("world invalid after {cmd:?}: {e}")),
        Err(_) => assert_eq!(before, authoritative_hash(game), "rejected {cmd:?} mutated truth"),
    }
}

fn run(game: &mut GameState, rules: &CompiledRuleset, rng: &mut FuzzRng, steps: usize) {
    for step in 0..steps {
        if rng.below(3) == 0 {
            debug_action(rng, game, rules);
            game.validate(rules).unwrap_or_else(|e| panic!("world invalid after a debug action: {e}"));
        }
        if rng.below(4) == 0 {
            let driven = combat_action(rng, game, rules);
            game.validate(rules).unwrap_or_else(|e| panic!("world invalid after a combat set-up: {e}"));
            if let Some(cmd) = driven {
                check(game, rules, cmd);
            }
        }
        if rng.below(4) == 0 {
            for cmd in religion_commands(rng, game, rules) {
                check(game, rules, cmd);
            }
            game.validate(rules).unwrap_or_else(|e| panic!("world invalid after a religion step: {e}"));
        }
        for cmd in commands(rng, game, rules) {
            check(game, rules, cmd);
        }
        if step % 40 == 39 {
            let loaded = decode(&encode(game, rules), rules).expect("own save loads");
            assert_eq!(authoritative_hash(&loaded), authoritative_hash(game), "save/load");
        }
    }
}

#[test]
fn fuzz_typed_effects_keep_the_world_valid() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut rng = FuzzRng(0x1_4E27);
    let mut policies = 0;
    let mut wonders = 0;
    let mut spies = 0;
    let mut completed = 0;
    let mut finds = 0;
    let mut religions = 0;
    for _ in 0..8 {
        let mut game = world(&mut rng, &rules);
        game.validate(&rules).unwrap();
        // Lost last cities eliminate players, so random skirmishes leave one
        // nation within a few dozen steps; found the Congress while
        // everyone is still alive so its projects get exercised.
        found_league(&mut game, &rules);
        run(&mut game, &rules, &mut rng, 160);
        let auth = game.view().auth;
        religions += auth.module().religion().religions().len();
        for p in 0..auth.players().count() {
            policies += auth.module().culture().of(PlayerId(p as u8)).policies.len();
            spies += auth.module().espionage().spies_of(PlayerId(p as u8)).len();
            finds += auth.players().economy_of(PlayerId(p as u8)).map_or(0, |e| e.natural_wonders_found.len());
        }
        completed += auth.module().congress().completed().len();
        for (_, core) in auth.cities().iter() {
            wonders += core.buildings.iter().filter(|b| rules.buildings[b.0 as usize].wonder).count();
        }
    }
    assert!(policies >= 40, "the fuzz adopts policies ({policies})");
    assert!(wonders >= 10, "the fuzz completes wonders ({wonders})");
    assert!(spies >= 4, "the fuzz grants spies ({spies})");
    assert!(completed >= 2, "the fuzz completes World Congress projects ({completed})");
    assert!(finds >= 10, "the fuzz finds natural wonders ({finds})");
    assert!(religions >= 4, "the fuzz founds religions ({religions})");
    eprintln!(
        "fuzz_inert: policies {policies}, wonders {wonders}, spies {spies}, league projects {completed}, natural wonder finds {finds}, religions {religions}"
    );
}

// ---- trade routes, city-states, diplomacy, tourism (workstream F) ----

/// Nations, policies and buildings of the trade and city-state
/// families: what the second fuzz puts in play.
const TRADE_CIVS: [&str; 6] = [
    "core:nation/arabia",
    "core:nation/morocco",
    "core:nation/portugal",
    "core:nation/venice",
    "core:nation/sweden",
    "core:nation/germany",
];
const TRADE_POLICIES: [&str; 12] = [
    "core:policy/patronage/merchant-confederacy",
    "core:policy/patronage/cultural-diplomacy",
    "core:policy/patronage/scholasticism",
    "core:policy/patronage/finisher",
    "core:policy/freedom/economic-union",
    "core:policy/freedom/treaty-organization",
    "core:policy/autocracy/gunboat-diplomacy",
    "core:policy/autocracy/united-front",
    "core:policy/autocracy/futurism",
    "core:policy/commerce/entrepreneurship",
    "core:policy/rationalism/scientific-revolution",
    "core:policy/autocracy/total-war",
];
const TRADE_BUILDINGS: [&str; 4] = [
    "core:building/hanse",
    "core:building/machu_picchu",
    "core:building/porcelain_tower",
    "core:building/international_space_station",
];
const GREAT_PEOPLE: [&str; 5] = [
    "core:unit/great-writer",
    "core:unit/great-artist",
    "core:unit/great-musician",
    "core:unit/great-merchant",
    "core:unit/great-general",
];

fn trade_world(rng: &mut FuzzRng, rules: &CompiledRuleset) -> GameState {
    let cfg = ScenarioConfig {
        seed: rng.next(),
        players: 2 + rng.below(2) as u8,
        width: 24 + rng.below(9) as u32,
        height: 18 + rng.below(7) as u32,
        cities: 2,
        units: 2,
        water_ratio: rng.below(25) as u8,
    };
    let options = NewGameOptions { city_states: 2, ..NewGameOptions::default() };
    let mut game = generate_new_game_with(rules, &cfg, None, options);
    game.debug_meet_all();
    let every: Vec<classic_rules::ids::TileId> = (0..cfg.width * cfg.height).map(classic_rules::ids::TileId).collect();
    for p in 0..game.view().auth.players().count() {
        game.debug_reveal(PlayerId(p as u8), &every);
    }
    for key in ["core:tech/animal-husbandry", "core:tech/sailing", "core:tech/writing"] {
        let tech = TechId(rules.technologies.iter().position(|t| t.key == key).unwrap() as u16);
        for p in 0..cfg.players {
            game.debug_learn(rules, PlayerId(p), tech);
        }
    }
    for p in 0..cfg.players {
        let key = pick_key(rng, &TRADE_CIVS);
        let nation = rules.nations.iter().position(|c| c.key == key).unwrap();
        game.debug_set_player_nation(PlayerId(p), nation as u8);
    }
    // The nations changed under the caches: load rebuilds them.
    decode(&encode(&game, rules), rules).expect("own save loads")
}

fn pick_key<'a>(rng: &mut FuzzRng, keys: &[&'a str]) -> &'a str {
    keys[rng.below(keys.len() as u64) as usize]
}

/// A trade unit of the active player in one of its cities starts the
/// first route offered (international or internal).
fn start_route(rng: &mut FuzzRng, game: &mut GameState, rules: &CompiledRuleset) {
    let Some(city) = random_city(rng, game) else { return };
    let active = game.view().auth.active_player();
    let tile = game.view().auth.cities().get(city).unwrap().tile;
    let caravan = UnitKindId(rules.units.iter().position(|u| u.key == "core:unit/caravan").unwrap() as u16);
    let unit = game.debug_spawn_unit(rules, active, tile, caravan);
    game.validate(rules).unwrap_or_else(|e| panic!("world invalid after a caravan: {e}"));
    let offers = classic_rules::derived::trade::trade_offers(game.view().auth, rules, unit);
    if let Some(offer) = offers.get(rng.below(offers.len().max(1) as u64) as usize) {
        let cmd = Command::EstablishTradeRoute { unit, destination: offer.destination, kind: offer.kind };
        check(game, rules, cmd);
    }
}

/// One debug action of the trade and city-state families.
fn trade_action(rng: &mut FuzzRng, game: &mut GameState, rules: &CompiledRuleset) {
    let active = game.view().auth.active_player();
    if !game.view().auth.players().is_major(active) {
        return;
    }
    let minors: Vec<PlayerId> = (0..game.view().auth.players().count() as u8)
        .map(PlayerId)
        .filter(|p| game.view().auth.players().is_city_state(*p))
        .collect();
    match rng.below(5) {
        0 => {
            let key = pick_key(rng, &TRADE_POLICIES);
            let policy = PolicyId(rules.module.policies.iter().position(|p| p.key == key).unwrap() as u16);
            game.debug_adopt_policy(rules, active, policy);
        }
        1 => {
            let Some(city) = random_city(rng, game) else { return };
            let key = pick_key(rng, &TRADE_BUILDINGS);
            let building = BuildingKindId(rules.buildings.iter().position(|b| b.key == key).unwrap() as u16);
            if !game.view().auth.cities().get(city).unwrap().buildings.contains(&building) {
                game.debug_complete_building(rules, city, building);
            }
        }
        2 => {
            let Some(&minor) = minors.get(rng.below(minors.len().max(1) as u64) as usize) else { return };
            game.debug_set_influence(rules, minor, active, rng.below(120) as i32 - 30);
        }
        3 => start_route(rng, game, rules),
        _ => {
            let Some(city) = random_city(rng, game) else { return };
            let tile = game.view().auth.cities().get(city).unwrap().tile;
            let key = pick_key(rng, &GREAT_PEOPLE);
            let kind = UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap() as u16);
            game.debug_spawn_unit(rules, active, tile, kind);
        }
    }
}

#[test]
fn fuzz_trade_and_city_state_effects_keep_the_world_valid() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut rng = FuzzRng(0x7_2ADE);
    let mut routes = 0;
    for _ in 0..6 {
        let mut game = trade_world(&mut rng, &rules);
        game.validate(&rules).unwrap();
        for step in 0..150 {
            if rng.below(2) == 0 {
                trade_action(&mut rng, &mut game, &rules);
                game.validate(&rules).unwrap_or_else(|e| panic!("world invalid after a trade action: {e}"));
            }
            for cmd in commands(&mut rng, &game, &rules) {
                check(&mut game, &rules, cmd);
            }
            routes = routes.max(game.view().auth.module().trade().routes().len());
            if step % 50 == 49 {
                let loaded = decode(&encode(&game, &rules), &rules).expect("own save loads");
                assert_eq!(authoritative_hash(&loaded), authoritative_hash(&game), "save/load");
            }
        }
    }
    assert!(routes >= 2, "the fuzz runs trade routes ({routes})");
}
