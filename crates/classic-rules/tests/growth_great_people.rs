//! Growth, starvation, kept food, settlers and great people of the classic
//! ruleset (classic/docs/classic/rules.md): growth thresholds,
//! growth, specialists, great person discovery.

use classic_content::core::core_rules;
use classic_rules::derived::great_people::{bulb_science, great_person_threshold, GreatPersonAction};
use classic_rules::derived::income::{city_income, growth_threshold};
use classic_rules::derived::yields::city_breakdown;
use classic_rules::ids::{
    BuildingKindId, CityHandle, ImprovementId, PlayerId, ProductionItem, TerrainId, TileId,
    UnitHandle, UnitKindId,
};
use classic_rules::math::HexCoord;
use classic_rules::replay::authoritative_hash;
use classic_rules::rules::content::SpecialistId;
use classic_rules::classic::effects::GreatPersonKind;
use classic_rules::world::city::CityCore;
use classic_rules::{compile_rules, Command, CompiledRuleset, GameConfig, GameState};

const P0: PlayerId = PlayerId(0);
const GRASS: TerrainId = TerrainId(1);
const HILLS: TerrainId = TerrainId(2);
const DESERT: TerrainId = TerrainId(3);

struct World {
    game: GameState,
    rules: CompiledRuleset,
    city: CityHandle,
}

impl World {
    /// `ground` everywhere, one grassland at (9,8) and hills at (7,8), a city
    /// of the only player at (8,8).
    fn new(ground: TerrainId) -> Self {
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
        let all: Vec<_> = (0..400).map(|i| (TileId(i), ground)).collect();
        game.debug_set_terrain_batch(&rules, &all);
        let dims = game.view().auth.map().dimensions;
        let t = |q, r| dims.to_tile(HexCoord { q, r }).unwrap();
        game.debug_set_terrain_batch(&rules, &[(t(9, 8), GRASS), (t(7, 8), HILLS)]);
        let city = game.debug_found_city(&rules, P0, t(8, 8));
        World { game, rules, city }
    }

    fn tile(&self, q: i32, r: i32) -> TileId {
        let dims = self.game.view().auth.map().dimensions;
        dims.to_tile(HexCoord { q, r }).unwrap()
    }

    fn core(&self) -> CityCore {
        self.game
            .view()
            .auth
            .cities()
            .get(self.city)
            .unwrap()
            .clone()
    }

    fn exec(&mut self, command: Command) -> Result<(), classic_rules::command::CommandError> {
        self.game.execute(&self.rules, command).map(|_| ())
    }

    fn end_turns(&mut self, n: usize) {
        for _ in 0..n {
            self.exec(Command::EndTurn { player: P0 }).unwrap();
        }
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

    fn unit_kind(&self, key: &str) -> UnitKindId {
        UnitKindId(self.rules.units.iter().position(|u| u.key == key).unwrap() as u16)
    }

    fn improvement(&self, key: &str) -> ImprovementId {
        ImprovementId(
            self.rules
                .improvements
                .iter()
                .position(|i| i.key == key)
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

    fn spawn(&mut self, key: &str, tile: TileId) -> UnitHandle {
        let kind = self.unit_kind(key);
        self.game.debug_spawn_unit(&self.rules, P0, tile, kind)
    }

    fn lock_grass(&mut self) {
        let (city, tile) = (self.city, self.tile(9, 8));
        self.exec(Command::LockWorkedTile {
            player: P0,
            city,
            tile,
        })
        .unwrap();
    }

    fn units_of(&self, key: &str) -> usize {
        let kind = self.unit_kind(key);
        self.game
            .view()
            .auth
            .units()
            .iter_cores()
            .filter(|u| u.kind == kind)
            .count()
    }

    fn economy(&self) -> classic_rules::world::player_economy::PlayerEconomy {
        self.game
            .view()
            .auth
            .players()
            .economy_of(P0)
            .unwrap()
            .clone()
    }

    fn act(
        &mut self,
        unit: UnitHandle,
        action: GreatPersonAction,
    ) -> Result<(), classic_rules::command::CommandError> {
        self.exec(Command::GreatPersonAction { unit, action })
    }
}

#[test]
fn growth_thresholds_follow_the_classic_table() {
    // rules.md, Standard speed, Ancient start: 15, 24, 33, 44, ...
    let expected = [15, 24, 33, 44, 55, 66, 77, 89, 101, 114];
    let got: Vec<u32> = (1..=10).map(growth_threshold).collect();
    assert_eq!(got, expected);
    // 15 + 8·29 + ⌊29^1.5⌋ = 15 + 232 + 156.
    assert_eq!(growth_threshold(30), 403);
}

#[test]
fn a_city_grows_at_the_threshold_and_keeps_the_rest() {
    // Desert centre (2F 1P by the city minimum) + locked grassland (2F),
    // one citizen eats 2: +2 a turn. 8 turns: 16 ≥ 15 → size 2, 1 left.
    let mut w = World::new(DESERT);
    w.lock_grass();
    w.end_turns(7);
    assert_eq!((w.core().population, w.core().food_times100), (1, 1400));
    w.end_turns(1);
    assert_eq!((w.core().population, w.core().food_times100), (2, 100));
    w.game.validate(&w.rules).unwrap();
}

#[test]
fn an_aqueduct_keeps_forty_percent_of_the_threshold() {
    // Kept food grows with the surplus up to 15 × 40 % = 6: the store after
    // growth is 16 − (15 − 6) = 7 instead of 1.
    let mut w = World::new(DESERT);
    let aqueduct = w.building("core:building/aqueduct");
    w.game.debug_add_building(&w.rules, w.city, aqueduct);
    w.lock_grass();
    w.end_turns(8);
    let core = w.core();
    assert_eq!((core.population, core.food_times100), (2, 700));
    assert_eq!(core.economy.food_kept, 6);
}

#[test]
fn a_hungry_city_starves_one_citizen_and_size_one_never_shrinks() {
    // Size 3 on desert: 2 (centre) + 2 (grassland) food at most, 6 eaten.
    let mut w = World::new(DESERT);
    w.game.debug_grow_city(&w.rules, w.city, 2);
    let income = city_income(w.game.view().auth, &w.rules, &w.core());
    assert!(income.food_surplus_times100 < 0, "{income:?}");
    w.end_turns(1);
    assert_eq!((w.core().population, w.core().food_times100), (2, 0));
    // A size-1 city on bare desert eats exactly its centre: it stays.
    let mut lone = World::new(DESERT);
    let (city, grass) = (lone.city, lone.tile(9, 8));
    lone.exec(Command::ToggleWorkedTile {
        player: P0,
        city,
        tile: grass,
    })
    .unwrap();
    lone.end_turns(3);
    assert_eq!(lone.core().population, 1);
    lone.game.validate(&lone.rules).unwrap();
}

#[test]
fn a_settler_needs_size_two_and_turns_surplus_food_into_production() {
    let mut w = World::new(GRASS);
    let city = w.city;
    let settler = ProductionItem::Unit(w.unit_kind("core:unit/settler"));
    let enqueue = Command::EnqueueProduction {
        player: P0,
        city,
        item: settler,
    };
    assert!(
        w.exec(enqueue.clone()).is_err(),
        "size 1 cannot build settlers"
    );
    w.game.debug_grow_city(&w.rules, city, 1);
    w.exec(enqueue).unwrap();
    let before = w.core();
    let rates = city_breakdown(w.game.view().auth, &w.rules, &before).rates;
    let income = city_income(w.game.view().auth, &w.rules, &before);
    // Grassland everywhere: 6 food, 4 eaten → +2 surplus → +2 production
    // (the first 2 count in full).
    assert_eq!(rates.food - 4, 2);
    assert_eq!(income.production, rates.production + 2);
    assert_eq!(income.food_surplus_times100, 0);
    w.end_turns(1);
    let after = w.core();
    assert_eq!(
        after.food_times100, before.food_times100,
        "the store is frozen"
    );
    assert_eq!(
        after.production,
        before.production + income.production as u32
    );
}

/// A size-1 city with one scientist (university slots): 3 points a turn.
fn scientist_city() -> World {
    let mut w = World::new(DESERT);
    let university = w.building("core:building/university");
    w.game.debug_add_building(&w.rules, w.city, university);
    let scientist = w.specialist("core:specialist/scientist");
    let city = w.city;
    w.exec(Command::SetSpecialists {
        player: P0,
        city,
        specialist: scientist,
        count: 1,
    })
    .unwrap();
    w
}

#[test]
fn specialists_bring_a_great_person_at_the_threshold() {
    let mut w = scientist_city();
    let scientist = w.specialist("core:specialist/scientist").0 as usize;
    w.end_turns(33);
    let points = |w: &World| w.game.view().auth.cities().module_of(w.city).unwrap().great_person_points[scientist];
    assert_eq!(points(&w), 9900);
    assert_eq!(w.units_of("core:unit/great-scientist"), 0);
    w.end_turns(1);
    assert_eq!(
        w.units_of("core:unit/great-scientist"),
        1,
        "102 points ≥ 100"
    );
    assert_eq!(points(&w), 0);
    assert_eq!(w.economy().great_people_created, 1);
    let auth = w.game.view().auth;
    assert_eq!(
        great_person_threshold(auth, P0, GreatPersonKind::Engineer),
        200,
        "shared counter"
    );
    assert_eq!(
        great_person_threshold(auth, P0, GreatPersonKind::Writer),
        100,
        "own counter"
    );
    w.game.validate(&w.rules).unwrap();
}

#[test]
fn great_person_progress_survives_save_and_load() {
    let mut w = scientist_city();
    w.lock_grass();
    w.end_turns(5);
    let save = classic_rules::persistence::encode::encode(&w.game, &w.rules);
    let loaded = classic_rules::persistence::decode::decode(&save, &w.rules).unwrap();
    assert_eq!(authoritative_hash(&w.game), authoritative_hash(&loaded));
    let core = loaded.view().auth.cities().get(w.city).unwrap();
    assert_eq!(core.economy, w.core().economy);
    let city_state = |game: &classic_rules::GameState| game.view().auth.cities().module_of(w.city).cloned();
    assert_eq!(city_state(&loaded), city_state(&w.game));
    assert_eq!(core.food_times100, w.core().food_times100);
    assert_eq!(
        loaded.view().auth.players().economy_of(P0),
        Some(&w.economy())
    );
    let mut bad = save.clone();
    bad.world.economy.cities[0].module.pop();
    assert!(classic_rules::persistence::decode::decode(&bad, &w.rules).is_err());
}

#[test]
fn a_great_scientist_grants_the_science_of_the_last_eight_turns() {
    let mut w = World::new(DESERT);
    w.end_turns(10);
    let at = w.core().tile;
    let unit = w.spawn("core:unit/great-scientist", at);
    let auth = w.game.view().auth;
    let bulb = bulb_science(auth, P0, 8);
    let history = w.economy().science_history;
    assert_eq!(bulb, history.iter().sum::<u32>());
    assert!(bulb > 0);
    let before = auth.players().science_of(P0);
    w.act(unit, GreatPersonAction::HurryProduction).unwrap_err();
    w.act(unit, GreatPersonAction::DiscoverTechnology).unwrap();
    assert_eq!(w.game.view().auth.players().science_of(P0), before + bulb);
    assert_eq!(w.units_of("core:unit/great-scientist"), 0, "used up");
}

#[test]
fn a_great_engineer_rushes_production_in_its_city() {
    let mut w = World::new(DESERT);
    let masonry = classic_rules::ids::TechId(
        w.rules
            .technologies
            .iter()
            .position(|t| t.key == "core:tech/masonry")
            .unwrap() as u16,
    );
    w.game.debug_learn(&w.rules, P0, masonry);
    // The Mausoleum (185): the Pyramids need Liberty.
    let pyramids = w.building("core:building/mausoleum_of_halicarnassus");
    let city = w.city;
    let at = w.core().tile;
    let unit = w.spawn("core:unit/great-engineer", at);
    w.act(unit, GreatPersonAction::HurryProduction).unwrap_err();
    let item = ProductionItem::Building(pyramids);
    w.exec(Command::EnqueueProduction {
        player: P0,
        city,
        item,
    })
    .unwrap();
    // 300 + 30 × 1 ≥ 185: the wonder is done at once.
    w.act(unit, GreatPersonAction::HurryProduction).unwrap();
    assert!(w.core().buildings.contains(&pyramids));
    assert_eq!(w.units_of("core:unit/great-engineer"), 0);
    w.game.validate(&w.rules).unwrap();
}

#[test]
fn a_great_artist_starts_a_golden_age_that_counts_down() {
    let mut w = World::new(DESERT);
    w.lock_grass();
    let at = w.core().tile;
    let production = city_breakdown(w.game.view().auth, &w.rules, &w.core())
        .rates
        .production;
    let unit = w.spawn("core:unit/great-artist", at);
    w.act(unit, GreatPersonAction::StartGoldenAge).unwrap();
    assert_eq!(
        (w.economy().golden_age_turns, w.economy().golden_ages),
        (8, 1)
    );
    let golden = city_breakdown(w.game.view().auth, &w.rules, &w.core())
        .rates
        .production;
    assert!(golden >= production, "golden age: +20 % production");
    w.game.validate(&w.rules).unwrap();
    w.end_turns(1);
    assert_eq!(w.economy().golden_age_turns, 7);
    w.end_turns(7);
    assert_eq!(
        (w.economy().golden_age_turns, w.economy().golden_ages),
        (0, 1)
    );
    w.game.validate(&w.rules).unwrap();
}

#[test]
fn great_people_build_their_own_improvements_and_the_citadel_claims_land() {
    let mut w = World::new(DESERT);
    let near = w.tile(8, 9);
    let academy = w.improvement("core:improvement/academy");
    let citadel = w.improvement("core:improvement/citadel");
    let scientist = w.spawn("core:unit/great-scientist", near);
    w.act(scientist, GreatPersonAction::Construct(citadel))
        .unwrap_err();
    w.act(scientist, GreatPersonAction::Construct(academy))
        .unwrap();
    assert_eq!(
        w.game.view().auth.map().improvement(near),
        Some(Some(academy))
    );
    let edge = w.tile(9, 7);
    let beyond = w.tile(10, 6);
    assert_eq!(w.game.view().auth.map().owner(beyond).flatten(), None);
    let general = w.spawn("core:unit/great-general", edge);
    w.act(general, GreatPersonAction::Construct(citadel))
        .unwrap();
    assert_eq!(
        w.game.view().auth.map().improvement(edge),
        Some(Some(citadel))
    );
    assert_eq!(w.game.view().auth.map().owner(beyond).flatten(), Some(P0));
    w.game.validate(&w.rules).unwrap();
}

#[test]
fn a_trade_mission_needs_a_city_state() {
    let mut w = World::new(DESERT);
    let at = w.core().tile;
    let merchant = w.spawn("core:unit/great-merchant", at);
    assert!(w.act(merchant, GreatPersonAction::TradeMission).is_err());
    assert_eq!(w.units_of("core:unit/great-merchant"), 1);
}
