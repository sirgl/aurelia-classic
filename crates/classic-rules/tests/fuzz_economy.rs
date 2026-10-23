//! Fuzz of the economy and society systems on the classic ruleset (§31, §62):
//! random worlds, workers and great people spawned by the harness, random
//! economy commands mixed with end turns. After every commit the world is
//! validated (derived yields equal their rebuild oracle); a rejected
//! command leaves the hash untouched.

use classic_content::core::core_rules;
use classic_rules::derived::great_people::GreatPersonAction;
use classic_rules::ids::{CityHandle, ImprovementId, PlayerId, TileId, UnitHandle, UnitKindId};
use classic_rules::replay::authoritative_hash;
use classic_rules::rules::content::SpecialistId;
use classic_rules::scenario::{generate, ScenarioConfig};
use classic_rules::world::citizens::CityFocus;
use classic_rules::{compile_rules, Command, CompiledRuleset, GameState};
use classic_rules::classic::content::GREAT_PERSON_ROLE;

struct Rng(u64);

impl Rng {
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

fn unit_kind(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap() as u16)
}

/// A generated world plus one worker and one work boat per player near its
/// cities, and every player knowing the early technologies of the builds.
fn world(rng: &mut Rng, rules: &CompiledRuleset) -> GameState {
    let cfg = ScenarioConfig {
        seed: rng.next(),
        players: 2 + rng.below(2) as u8,
        width: 12 + rng.below(9) as u32,
        height: 12 + rng.below(9) as u32,
        cities: 2 + rng.below(5) as usize,
        units: rng.below(8) as usize,
        water_ratio: 10 + rng.below(25) as u8,
    };
    let mut game = generate(&cfg, rules);
    let worker = unit_kind(rules, "core:unit/worker");
    let cities: Vec<_> = game
        .view()
        .auth
        .cities()
        .iter()
        .map(|(_, c)| (c.owner, c.tile))
        .collect();
    let university = classic_rules::ids::BuildingKindId(
        rules.buildings.iter().position(|b| b.key == "core:building/university").unwrap() as u16,
    );
    let handles: Vec<_> = game.view().auth.cities().iter().map(|(h, _)| h).collect();
    for (city, (owner, tile)) in handles.into_iter().zip(cities) {
        game.debug_spawn_unit(rules, owner, tile, worker);
        game.debug_add_building(rules, city, university);
        let great = GREAT_PEOPLE[rng.below(GREAT_PEOPLE.len() as u64) as usize];
        game.debug_spawn_unit(rules, owner, tile, unit_kind(rules, great));
        // Trade units for the routes.
        game.debug_spawn_unit(rules, owner, tile, unit_kind(rules, "core:unit/caravan"));
        // Units that hold strategic resources (deficits included).
        for _ in 0..rng.below(3) {
            game.debug_spawn_unit(rules, owner, tile, unit_kind(rules, "core:unit/swordsman"));
        }
        // Some cities are puppets or occupied (happiness), some grow big
        // enough to make their empire unhappy.
        let status = match rng.below(4) {
            0 => classic_rules::world::city::CityStatus::Puppet,
            1 => classic_rules::world::city::CityStatus::Occupied,
            _ => classic_rules::world::city::CityStatus::Own,
        };
        game.debug_set_city_status(city, status);
        game.debug_grow_city(rules, city, rng.below(8) as u32);
    }
    // Random AI players and difficulty (handicap rows of happiness and
    // growth).
    let ai: Vec<PlayerId> = (0..cfg.players).filter(|_| rng.below(2) == 0).map(PlayerId).collect();
    let level = ["core:handicap/settler", "core:handicap/prince", "core:handicap/deity"][rng.below(3) as usize];
    game.configure_players(level, &ai);
    // Gold to buy with, and the technologies of more buildings and of the
    // processes.
    for p in 0..cfg.players {
        game.debug_earn(PlayerId(p), rng.below(800) as u32);
        for key in ["core:tech/currency", "core:tech/writing", "core:tech/masonry", "core:tech/education"] {
            if let Some(t) = rules.technologies.iter().position(|t| t.key == key) {
                game.debug_learn(rules, PlayerId(p), classic_rules::ids::TechId(t as u16));
            }
        }
    }
    for key in [
        "core:tech/agriculture",
        "core:tech/mining",
        "core:tech/the-wheel",
        "core:tech/pottery",
        "core:tech/bronze-working",
        "core:tech/animal-husbandry",
        "core:tech/iron-working",
        "core:tech/horseback-riding",
    ] {
        let tech = rules
            .technologies
            .iter()
            .position(|t| t.key == key)
            .unwrap();
        for p in 0..cfg.players {
            game.debug_learn(rules, PlayerId(p), classic_rules::ids::TechId(tech as u16));
        }
    }
    game
}

/// Great people the harness hands out (one per city, at random).
const GREAT_PEOPLE: [&str; 5] = [
    "core:unit/great-scientist",
    "core:unit/great-engineer",
    "core:unit/great-artist",
    "core:unit/great-merchant",
    "core:unit/great-general",
];

fn great_person_action(rng: &mut Rng, rules: &CompiledRuleset) -> GreatPersonAction {
    match rng.below(5) {
        0 => GreatPersonAction::Construct(ImprovementId(rng.below(rules.improvements.len() as u64 + 1) as u16)),
        1 => GreatPersonAction::DiscoverTechnology,
        2 => GreatPersonAction::HurryProduction,
        3 => GreatPersonAction::StartGoldenAge,
        _ => GreatPersonAction::TradeMission,
    }
}

fn pick<T: Copy>(rng: &mut Rng, items: &[T], broken: T) -> T {
    if items.is_empty() || rng.below(12) == 0 {
        broken
    } else {
        items[rng.below(items.len() as u64) as usize]
    }
}

fn own_units(game: &GameState) -> Vec<UnitHandle> {
    let active = game.view().auth.active_player();
    game.view().auth.indexes().units_of(active).to_vec()
}

fn own_cities(game: &GameState) -> Vec<CityHandle> {
    let active = game.view().auth.active_player();
    game.view().auth.indexes().cities_of(active).to_vec()
}

const BROKEN_UNIT: UnitHandle = UnitHandle {
    index: 9999,
    generation: 0,
};
const BROKEN_CITY: CityHandle = CityHandle {
    index: 9999,
    generation: 0,
};

/// A city tile in three rings, sometimes off the map or the city.
fn city_tile(rng: &mut Rng, game: &GameState, city: CityHandle) -> TileId {
    let auth = game.view().auth;
    let Some(core) = auth.cities().get(city) else {
        return TileId(rng.below(auth.map().tile_count() as u64) as u32);
    };
    let ring = classic_rules::math::tiles_in_hex_radius(&auth.map().dimensions, core.tile, 3);
    ring[rng.below(ring.len() as u64) as usize]
}

/// A trade route order of a random unit (to any city, any kind).
fn trade_command(rng: &mut Rng, game: &GameState, units: &[UnitHandle]) -> Command {
    let all: Vec<CityHandle> = game.view().auth.cities().iter().map(|(h, _)| h).collect();
    let unit = pick(rng, units, BROKEN_UNIT);
    match rng.below(3) {
        0 => Command::EstablishTradeRoute {
            unit,
            destination: pick(rng, &all, BROKEN_CITY),
            kind: classic_rules::world::trade::RouteKind::ALL[rng.below(3) as usize],
        },
        1 => Command::ChangeTradeHome {
            unit,
            city: pick(rng, &all, BROKEN_CITY),
        },
        _ => Command::PlunderTradeRoute { unit },
    }
}

/// One random economy command of the active player.
/// The great people among `units`.
fn great_people(game: &GameState, rules: &CompiledRuleset, units: &[UnitHandle]) -> Vec<UnitHandle> {
    let role = |u: &UnitHandle| {
        let kind = game.view().auth.units().get(*u).map(|v| v.core.kind);
        kind.and_then(|k| rules.units[k.0 as usize].role)
    };
    units
        .iter()
        .copied()
        .filter(|u| role(u) == Some(GREAT_PERSON_ROLE))
        .collect()
}

fn command(rng: &mut Rng, game: &GameState, rules: &CompiledRuleset) -> Command {
    let player = game.view().auth.active_player();
    let units = own_units(game);
    let cities = own_cities(game);
    match rng.below(110) {
        0..=19 => Command::BuildImprovement {
            unit: pick(rng, &units, BROKEN_UNIT),
            improvement: ImprovementId(rng.below(rules.improvements.len() as u64 + 2) as u16),
        },
        20..=27 => Command::ChopForest {
            unit: pick(rng, &units, BROKEN_UNIT),
        },
        28..=33 => Command::BuildRoad {
            unit: pick(rng, &units, BROKEN_UNIT),
        },
        34..=43 => {
            let unit = pick(rng, &units, BROKEN_UNIT);
            let tiles = game.view().auth.map().tile_count() as u64;
            Command::MoveUnit {
                unit,
                destination: TileId(rng.below(tiles) as u32),
            }
        }
        44..=53 => {
            let city = pick(rng, &cities, BROKEN_CITY);
            let tile = city_tile(rng, game, city);
            Command::LockWorkedTile { player, city, tile }
        }
        54..=61 => {
            let city = pick(rng, &cities, BROKEN_CITY);
            let tile = city_tile(rng, game, city);
            Command::ToggleWorkedTile { player, city, tile }
        }
        62..=69 => Command::SetCityFocus {
            player,
            city: pick(rng, &cities, BROKEN_CITY),
            focus: CityFocus::ALL[rng.below(8) as usize],
        },
        70..=75 => Command::SetSpecialists {
            player,
            city: pick(rng, &cities, BROKEN_CITY),
            specialist: SpecialistId(rng.below(rules.specialists.len() as u64 + 1) as u16),
            count: rng.below(4) as u8,
        },
        // Great person actions are asked of great people (any unit would
        // be refused almost always: the actions would go untested).
        76..=85 => Command::GreatPersonAction {
            unit: pick(rng, &great_people(game, rules, &units), BROKEN_UNIT),
            action: great_person_action(rng, rules),
        },
        86..=87 => Command::EnqueueProduction {
            player,
            city: pick(rng, &cities, BROKEN_CITY),
            item: classic_rules::ids::ProductionItem::Building(classic_rules::ids::BuildingKindId(
                rng.below(rules.buildings.len() as u64 + 1) as u16,
            )),
        },
        // Units, those that need iron or horses among them.
        88..=89 => Command::EnqueueProduction {
            player,
            city: pick(rng, &cities, BROKEN_CITY),
            item: classic_rules::ids::ProductionItem::Unit(UnitKindId(rng.below(rules.units.len() as u64 + 1) as u16)),
        },
        90..=91 => Command::HurryProduction {
            player,
            city: pick(rng, &cities, BROKEN_CITY),
        },
        92..=93 => {
            let city = pick(rng, &cities, BROKEN_CITY);
            let tile = city_tile(rng, game, city);
            Command::PurchaseTile { player, city, tile }
        }
        100..=105 => trade_command(rng, game, &units),
        94..=95 => Command::SetProcess {
            player,
            city: pick(rng, &cities, BROKEN_CITY),
            process: [None, Some(classic_rules::world::city_economy::Process::Wealth), Some(classic_rules::world::city_economy::Process::Research)]
                [rng.below(3) as usize],
        },
        _ => Command::EndTurn { player },
    }
}

/// Accepted commands: (all, great person actions).
fn run(game: &mut GameState, rules: &CompiledRuleset, rng: &mut Rng, steps: usize) -> (usize, usize) {
    let mut accepted = 0;
    let mut great = 0;
    for _ in 0..steps {
        let cmd = command(rng, game, rules);
        let before = authoritative_hash(game);
        match game.execute(rules, cmd.clone()) {
            Ok(_) => {
                accepted += 1;
                great += usize::from(matches!(cmd, Command::GreatPersonAction { .. }));
                if let Err(e) = game.validate(rules) {
                    panic!("world invalid after {cmd:?}: {e}");
                }
            }
            Err(_) => assert_eq!(
                before,
                authoritative_hash(game),
                "rejected {cmd:?} mutated truth"
            ),
        }
    }
    (accepted, great)
}

#[test]
fn fuzz_economy_validates_every_commit() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut rng = Rng(0xEC0_0001);
    let (mut accepted, mut great) = (0, 0);
    for _ in 0..12 {
        let mut game = world(&mut rng, &rules);
        game.validate(&rules).unwrap();
        let (all, gp) = run(&mut game, &rules, &mut rng, 250);
        accepted += all;
        great += gp;
    }
    assert!(accepted > 400, "the mix must mostly succeed: {accepted}");
    assert!(great >= 3, "great person actions are exercised: {great}");
}

#[test]
fn fuzz_economy_save_load_keeps_the_hash() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut rng = Rng(0xEC0_0002);
    for _ in 0..4 {
        let mut game = world(&mut rng, &rules);
        run(&mut game, &rules, &mut rng, 200);
        let save = classic_rules::persistence::encode::encode(&game, &rules);
        let loaded = classic_rules::persistence::decode::decode(&save, &rules).unwrap();
        assert_eq!(authoritative_hash(&game), authoritative_hash(&loaded));
        loaded.validate(&rules).unwrap();
    }
}
