//! Citizens of a city: automatic assignment by
//! focus, locked tiles, hand-assigned specialists, unemployed citizens; the
//! orders survive save and load.

use classic_content::core::core_rules;
use classic_rules::derived::yields::city_work;
use classic_rules::ids::{BuildingKindId, CityHandle, PlayerId, TerrainId, TileId};
use classic_rules::math::HexCoord;
use classic_rules::replay::authoritative_hash;
use classic_rules::rules::content::SpecialistId;
use classic_rules::world::citizens::CityFocus;
use classic_rules::{compile_rules, Command, CompiledRuleset, GameConfig, GameState};

const P0: PlayerId = PlayerId(0);

struct World {
    game: GameState,
    rules: CompiledRuleset,
    city: CityHandle,
}

impl World {
    /// Desert around a city at (8,8): ring 1 holds one grassland (food) and
    /// one hills tile (production).
    fn new() -> Self {
        let rules = compile_rules(core_rules()).unwrap();
        let mut game = GameState::new(
            &rules,
            GameConfig {
                players: 1,
                width: 20,
                height: 20,
                rng_seed: 3,
            },
        );
        let all: Vec<_> = (0..400).map(|i| (TileId(i), TerrainId(3))).collect();
        game.debug_set_terrain_batch(&rules, &all);
        let dims = game.view().auth.map().dimensions;
        let t = |q, r| dims.to_tile(HexCoord { q, r }).unwrap();
        game.debug_set_terrain_batch(&rules, &[(t(9, 8), TerrainId(1)), (t(7, 8), TerrainId(2))]);
        let city = game.debug_found_city(&rules, P0, t(8, 8));
        World { game, rules, city }
    }

    fn tile(&self, q: i32, r: i32) -> TileId {
        let dims = self.game.view().auth.map().dimensions;
        dims.to_tile(HexCoord { q, r }).unwrap()
    }

    fn worked(&self) -> Vec<TileId> {
        let core = self.game.view().auth.cities().get(self.city).unwrap();
        city_work(self.game.view().auth, &self.rules, core)
            .tiles()
            .collect()
    }

    fn exec(&mut self, command: Command) -> Result<(), classic_rules::command::CommandError> {
        self.game.execute(&self.rules, command).map(|_| ())
    }

    fn building(&self, key: &str) -> BuildingKindId {
        BuildingKindId(
            self.rules
                .buildings
                .iter()
                .position(|b| b.key == key)
                .unwrap() as u16,
        )
    }

    fn specialist(&self, key: &str) -> SpecialistId {
        SpecialistId(
            self.rules
                .specialists
                .iter()
                .position(|s| s.key == key)
                .unwrap() as u16,
        )
    }
}

#[test]
fn default_focus_feeds_the_city_production_focus_takes_hills() {
    let mut w = World::new();
    let grass = w.tile(9, 8);
    let hills = w.tile(7, 8);
    assert!(
        w.worked().contains(&grass),
        "a hungry city works food first"
    );
    let city = w.city;
    w.exec(Command::SetCityFocus {
        player: P0,
        city,
        focus: CityFocus::Production,
    })
    .unwrap();
    assert!(
        w.worked().contains(&hills),
        "production focus works the hills"
    );
    assert!(
        w.exec(Command::SetCityFocus {
            player: P0,
            city,
            focus: CityFocus::Production
        })
        .is_err(),
        "the same focus twice is a no-op error"
    );
    w.game.validate(&w.rules).unwrap();
}

#[test]
fn a_locked_tile_is_worked_first() {
    let mut w = World::new();
    let hills = w.tile(7, 8);
    let city = w.city;
    assert!(!w.worked().contains(&hills));
    w.exec(Command::LockWorkedTile {
        player: P0,
        city,
        tile: hills,
    })
    .unwrap();
    assert!(w.worked().contains(&hills));
    assert!(w
        .game
        .view()
        .auth
        .cities()
        .get(city)
        .unwrap()
        .citizens
        .locked
        .contains(&hills));
    // Unlock: back to the automatic choice.
    w.exec(Command::LockWorkedTile {
        player: P0,
        city,
        tile: hills,
    })
    .unwrap();
    assert!(!w.worked().contains(&hills));
    // Off work drops the lock.
    w.exec(Command::LockWorkedTile {
        player: P0,
        city,
        tile: hills,
    })
    .unwrap();
    w.exec(Command::ToggleWorkedTile {
        player: P0,
        city,
        tile: hills,
    })
    .unwrap();
    let core = w.game.view().auth.cities().get(city).unwrap();
    assert!(core.citizens.locked.is_empty() && core.unworked.contains(&hills));
    // A tile of no city cannot be locked.
    let far = w.tile(15, 15);
    assert!(w
        .exec(Command::LockWorkedTile {
            player: P0,
            city,
            tile: far
        })
        .is_err());
    w.game.validate(&w.rules).unwrap();
}

#[test]
fn specialists_need_slots_and_come_from_the_citizens() {
    let mut w = World::new();
    let city = w.city;
    let scientist = w.specialist("core:specialist/scientist");
    assert!(
        w.exec(Command::SetSpecialists {
            player: P0,
            city,
            specialist: scientist,
            count: 1
        })
        .is_err(),
        "no slot without a building"
    );
    let university = w.building("core:building/university");
    w.game.debug_add_building(&w.rules, city, university);
    let slots = classic_rules::derived::citizens::specialist_slots(
        &w.rules,
        w.game.view().auth.cities().get(city).unwrap(),
    );
    assert!(
        slots[scientist.0 as usize] >= 1,
        "the university has scientist slots"
    );
    w.exec(Command::SetSpecialists {
        player: P0,
        city,
        specialist: scientist,
        count: 1,
    })
    .unwrap();
    let core = w.game.view().auth.cities().get(city).unwrap().clone();
    let work = city_work(w.game.view().auth, &w.rules, &core);
    assert_eq!(work.specialists[scientist.0 as usize], 1);
    assert_eq!(
        work.tiles().count(),
        1,
        "the only citizen is the scientist: the centre alone is worked"
    );
    assert!(
        w.exec(Command::SetSpecialists {
            player: P0,
            city,
            specialist: scientist,
            count: 5
        })
        .is_err(),
        "more than the slots"
    );
    w.game.validate(&w.rules).unwrap();
}

#[test]
fn citizens_without_a_good_tile_are_unemployed() {
    // Desert gives nothing: the unemployed citizen (+1 production) beats it.
    let mut w = World::new();
    let city = w.city;
    let grass = w.tile(9, 8);
    let hills = w.tile(7, 8);
    w.exec(Command::ToggleWorkedTile {
        player: P0,
        city,
        tile: grass,
    })
    .unwrap();
    w.exec(Command::ToggleWorkedTile {
        player: P0,
        city,
        tile: hills,
    })
    .unwrap();
    let core = w.game.view().auth.cities().get(city).unwrap().clone();
    let work = city_work(w.game.view().auth, &w.rules, &core);
    let unemployed = w.rules.roles.default_specialist.unwrap();
    assert_eq!(work.specialists[unemployed.0 as usize], 1);
    assert_eq!(work.specialist_yields.production, 1);
}

#[test]
fn citizen_orders_survive_save_and_load() {
    let mut w = World::new();
    let city = w.city;
    let hills = w.tile(7, 8);
    w.exec(Command::SetCityFocus {
        player: P0,
        city,
        focus: CityFocus::Gold,
    })
    .unwrap();
    w.exec(Command::LockWorkedTile {
        player: P0,
        city,
        tile: hills,
    })
    .unwrap();
    let university = w.building("core:building/university");
    w.game.debug_add_building(&w.rules, city, university);
    let scientist = w.specialist("core:specialist/scientist");
    w.exec(Command::SetSpecialists {
        player: P0,
        city,
        specialist: scientist,
        count: 1,
    })
    .unwrap();
    let save = classic_rules::persistence::encode::encode(&w.game, &w.rules);
    let loaded = classic_rules::persistence::decode::decode(&save, &w.rules).unwrap();
    assert_eq!(authoritative_hash(&w.game), authoritative_hash(&loaded));
    let plan = &loaded.view().auth.cities().get(city).unwrap().citizens;
    assert_eq!(plan.focus, CityFocus::Gold);
    assert_eq!(plan.locked, vec![hills]);
    assert_eq!(plan.specialists, vec![(scientist, 1)]);
    // A damaged section is an error, not a panic.
    let mut bad = save.clone();
    bad.world.economy.cities[0].focus = 99;
    assert!(classic_rules::persistence::decode::decode(&bad, &w.rules).is_err());
}

#[test]
fn avoiding_growth_is_an_order_of_the_city_that_survives_save_and_load() {
    let mut w = World::new();
    let city = w.city;
    w.exec(Command::SetAvoidGrowth { player: P0, city, avoid: true }).unwrap();
    assert!(w.exec(Command::SetAvoidGrowth { player: P0, city, avoid: true }).is_err());
    let save = classic_rules::persistence::encode::encode(&w.game, &w.rules);
    let loaded = classic_rules::persistence::decode::decode(&save, &w.rules).unwrap();
    assert_eq!(authoritative_hash(&w.game), authoritative_hash(&loaded));
    let plan = &loaded.view().auth.cities().get(city).unwrap().citizens;
    assert!(plan.avoid_growth);
    assert_eq!(plan.focus, CityFocus::Default, "the flag is not a focus");
    // The flag sits beside the focus: both survive.
    w.exec(Command::SetCityFocus { player: P0, city, focus: CityFocus::Gold }).unwrap();
    let save = classic_rules::persistence::encode::encode(&w.game, &w.rules);
    let plan = &classic_rules::persistence::decode::decode(&save, &w.rules).unwrap().view().auth.cities().get(city).unwrap().citizens.clone();
    assert_eq!((plan.focus, plan.avoid_growth), (CityFocus::Gold, true));
}
