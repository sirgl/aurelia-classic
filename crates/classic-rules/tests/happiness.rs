//! Happiness of the classic ruleset (classic/docs/classic/rules.md): sources,
//! unhappiness, the mood effects and the golden age meter.

use classic_content::core::core_rules;
use classic_rules::derived::citizens::EmpireContext;
use classic_rules::derived::happiness::{
    golden_age_threshold, growth_percent, mood, player_happiness, yield_penalty_percent, Mood,
};
use classic_rules::ids::{
    BuildingKindId, CityHandle, ImprovementId, PlayerId, ProductionItem, ResourceId, TerrainId,
    TileId, UnitKindId,
};
use classic_rules::math::HexCoord;
use classic_rules::replay::authoritative_hash;
use classic_rules::world::city::CityStatus;
use classic_rules::{compile_rules, Command, CompiledRuleset, GameConfig, GameState};

const P0: PlayerId = PlayerId(0);
const DESERT: TerrainId = TerrainId(3);

struct World {
    game: GameState,
    rules: CompiledRuleset,
}

impl World {
    fn new() -> Self {
        let rules = compile_rules(core_rules()).unwrap();
        let mut game = GameState::new(
            &rules,
            GameConfig {
                players: 1,
                width: 30,
                height: 20,
                rng_seed: 9,
            },
        );
        let all: Vec<_> = (0..600).map(|i| (TileId(i), DESERT)).collect();
        game.debug_set_terrain_batch(&rules, &all);
        World { game, rules }
    }

    fn tile(&self, q: i32, r: i32) -> TileId {
        let dims = self.game.view().auth.map().dimensions;
        dims.to_tile(HexCoord { q, r }).unwrap()
    }

    /// A city at (q, r) with `pop` citizens.
    fn city(&mut self, q: i32, r: i32, pop: u32) -> CityHandle {
        let t = self.tile(q, r);
        let city = self.game.debug_found_city(&self.rules, P0, t);
        self.game.debug_grow_city(&self.rules, city, pop - 1);
        city
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

    fn report(&self) -> classic_rules::derived::happiness::HappinessReport {
        player_happiness(self.game.view().auth, &self.rules, P0)
    }

    fn end_turn(&mut self) {
        self.game
            .execute(&self.rules, Command::EndTurn { player: P0 })
            .unwrap();
    }
}

#[test]
fn four_cities_of_five_cost_thirty_two_unhappiness() {
    // rules.md example: human at the default difficulty level, 4 normal cities of size 5:
    // (4 × 300 + 20 × 100) / 100 = 32.
    let mut w = World::new();
    for (q, r) in [(4, 4), (10, 4), (16, 4), (22, 4)] {
        w.city(q, r, 5);
    }
    let report = w.report();
    assert_eq!((report.from_cities, report.from_population), (12, 20));
    assert_eq!(report.unhappiness(), 32);
    assert_eq!(report.handicap, 9, "the default level's base happiness");
    // The same empire played by the AI: AI_DEFAULT gives 15 and 90 % of
    // the city and population unhappiness: (1080 + 1800) / 100 = 28.
    w.game.configure_players(w.rules.default_difficulty(), &[P0]);
    let ai = w.report();
    assert_eq!((ai.handicap, ai.unhappiness()), (15, 28));
    let ai_row = w.rules.ai_handicap().unwrap();
    assert_eq!(ai_row.values.module.happiness_default, 15);
}

#[test]
fn occupied_cities_and_population_cost_more() {
    // An occupied city of size 10: 500 + 200 + 10 × 200 = 2700 → 27 (Unciv CityStats.kt: a
    // city 5, "Occupied City" 2, a citizen of an annexed city counts double).
    let mut w = World::new();
    let city = w.city(6, 6, 10);
    w.game.debug_set_city_status(city, CityStatus::Occupied);
    let report = w.report();
    assert_eq!(
        (
            report.from_cities,
            report.from_population,
            report.from_occupied
        ),
        (0, 0, 27)
    );
    assert_eq!(report.unhappiness(), 27);
}

#[test]
fn local_happiness_is_capped_by_population_global_is_not() {
    let mut w = World::new();
    let small = w.city(4, 4, 1);
    let colosseum = w.building("core:building/colosseum");
    w.game.debug_add_building(&w.rules, small, colosseum);
    assert_eq!(w.report().local, 1, "colosseum +2, capped at size 1");
    w.game.debug_grow_city(&w.rules, small, 2);
    assert_eq!(w.report().local, 2);
    let circus = w.building("core:building/circus_maximus");
    w.game.debug_add_building(&w.rules, small, circus);
    assert_eq!(
        w.report().buildings,
        5,
        "a wonder's player happiness is global"
    );
}

#[test]
fn a_luxury_counts_once_when_connected() {
    let mut w = World::new();
    let centre = w.tile(8, 8);
    w.city(8, 8, 1);
    let gems = ResourceId(
        w.rules
            .resources
            .iter()
            .position(|r| r.key == "core:resource/gems")
            .unwrap() as u16,
    );
    let mine = ImprovementId(
        w.rules
            .improvements
            .iter()
            .position(|i| i.key == "core:improvement/mine")
            .unwrap() as u16,
    );
    let farm = ImprovementId(
        w.rules
            .improvements
            .iter()
            .position(|i| i.key == "core:improvement/farm")
            .unwrap() as u16,
    );
    let a = w.tile(9, 8);
    let b = w.tile(7, 8);
    w.game
        .debug_set_resource_batch(&w.rules, &[(a, gems), (b, gems)]);
    assert_eq!(
        w.report().luxuries,
        0,
        "not connected without its improvement"
    );
    w.game.debug_set_improvement_batch(&w.rules, &[(a, farm)]);
    assert_eq!(w.report().luxuries, 0, "a farm does not connect gems");
    w.game
        .debug_set_improvement_batch(&w.rules, &[(a, mine), (b, mine)]);
    let report = w.report();
    assert_eq!(
        (report.luxuries, report.luxury_kinds),
        (4, 1),
        "two copies, one kind"
    );
    // A luxury under the city is connected by the city.
    let salt = ResourceId(
        w.rules
            .resources
            .iter()
            .position(|r| r.key == "core:resource/salt")
            .unwrap() as u16,
    );
    w.game.debug_set_resource_batch(&w.rules, &[(centre, salt)]);
    assert_eq!(w.report().luxuries, 8);
}

#[test]
fn natural_wonders_found_give_happiness() {
    let mut w = World::new();
    w.city(4, 4, 1);
    let far = w.tile(20, 10);
    let wonder = classic_rules::rules::content::NaturalWonderId(0);
    w.game
        .debug_set_natural_wonder_batch(&w.rules, &[(far, wonder)]);
    assert_eq!(w.report().natural_wonders, 0, "not found yet");
    w.game.debug_reveal(P0, &[far]);
    assert_eq!(w.report().natural_wonders, 1);
}

/// Three size-6 cities on desert: 9 − (9 + 18) = −18, very unhappy.
fn very_unhappy_world() -> (World, CityHandle) {
    let mut w = World::new();
    let city = w.city(4, 4, 6);
    w.city(10, 4, 6);
    w.city(16, 4, 6);
    (w, city)
}

#[test]
fn moods_follow_the_classic_thresholds() {
    assert_eq!(mood(0), Mood::Content);
    assert_eq!(mood(-1), Mood::Unhappy);
    // Unciv: GlobalUniques.json, very unhappy below -10.
    assert_eq!(mood(-10), Mood::Unhappy);
    assert_eq!(mood(-11), Mood::VeryUnhappy);
    assert_eq!((growth_percent(-10), growth_percent(-11)), (-75, -100));
    assert_eq!(yield_penalty_percent(-5), 0, "no per-point penalty");
    assert_eq!(yield_penalty_percent(-30), -50, "production -50 % below -10");
    assert_eq!(yield_penalty_percent(3), 0);
}

#[test]
fn a_very_unhappy_empire_trains_no_settlers_and_works_worse() {
    let (mut w, city) = very_unhappy_world();
    assert!(w.report().excess() <= -10);
    let settler = UnitKindId(
        w.rules
            .units
            .iter()
            .position(|u| u.key == "core:unit/settler")
            .unwrap() as u16,
    );
    let enqueue = Command::EnqueueProduction {
        player: P0,
        city,
        item: ProductionItem::Unit(settler),
    };
    // The effects read the snapshot of the turn start: none taken yet.
    assert!(check(&w, &enqueue).is_ok());
    w.end_turn();
    let auth = w.game.view().auth;
    let excess = auth.players().economy_of(P0).unwrap().excess_happiness;
    assert!(excess <= -10, "snapshot {excess}");
    let err = check(&w, &enqueue).unwrap_err();
    assert!(format!("{err:?}").contains("very unhappy"));
    let context = EmpireContext::of(auth, P0);
    assert_eq!(context.module.unhappiness_percent, yield_penalty_percent(excess));
    let unit = w
        .game
        .debug_spawn_unit(&w.rules, P0, w.tile(25, 15), settler);
    assert!(check(&w, &Command::FoundCity { settler: unit }).is_err());
    // Stored yields carry the penalty and still match their rebuild.
    w.game.validate(&w.rules).unwrap();
}

#[test]
fn golden_age_thresholds_follow_the_classic_table() {
    // Unciv: GoldenAgeManager.happinessRequiredForNextGoldenAge, 500 × (1 +
    // cities / 100): 505 / 520 / 540 for 1 / 4 / 8 cities.
    let mut w = World::new();
    let auth = |w: &World| golden_age_threshold(w.game.view().auth, P0);
    w.city(2, 2, 1);
    assert_eq!(auth(&w), 505);
    for (q, r) in [(8, 2), (14, 2), (20, 2)] {
        w.city(q, r, 1);
    }
    assert_eq!(auth(&w), 520);
    for (q, r) in [(2, 8), (8, 8), (14, 8), (20, 8)] {
        w.city(q, r, 1);
    }
    assert_eq!(auth(&w), 540);
}

#[test]
fn excess_happiness_fills_the_meter_and_starts_a_golden_age() {
    // One size-1 city: 9 − (3 + 1) = 5 a turn; Circus Maximus +5 → 10.
    let mut w = World::new();
    let city = w.city(4, 4, 1);
    let circus = w.building("core:building/circus_maximus");
    w.game.debug_add_building(&w.rules, city, circus);
    assert_eq!(w.report().excess(), 10);
    w.end_turn();
    let economy = |w: &World| w.game.view().auth.players().economy_of(P0).unwrap().clone();
    assert_eq!(economy(&w).golden_age_meter, 10);
    let mut turns = 1;
    while economy(&w).golden_age_turns == 0 {
        w.end_turn();
        turns += 1;
        assert!(turns < 80, "no golden age");
    }
    // The meter reaches 505 on the 51st turn (if the city stays at size 1
    // the excess stays 10; growth lowers it a little).
    assert!(turns >= 50, "{turns}");
    assert_eq!(economy(&w).golden_age_turns, 10, "UNMODIFIED_GOLDEN_AGE_TURNS");
    assert_eq!(economy(&w).golden_ages, 1);
    w.game.validate(&w.rules).unwrap();
}

#[test]
fn chichen_itza_lengthens_golden_ages() {
    let mut w = World::new();
    let city = w.city(4, 4, 1);
    let chichen = w.building("core:building/chichen_itza");
    w.game.debug_add_building(&w.rules, city, chichen);
    let artist = UnitKindId(
        w.rules
            .units
            .iter()
            .position(|u| u.key == "core:unit/great-artist")
            .unwrap() as u16,
    );
    let at = w.tile(4, 4);
    let unit = w.game.debug_spawn_unit(&w.rules, P0, at, artist);
    let action = classic_rules::derived::great_people::GreatPersonAction::StartGoldenAge;
    w.game
        .execute(&w.rules, Command::GreatPersonAction { unit, action })
        .unwrap();
    let economy = w.game.view().auth.players().economy_of(P0).unwrap();
    assert_eq!(economy.golden_age_turns, 12, "8 × 150 %");
}

#[test]
fn settings_and_snapshot_survive_save_and_load() {
    let (mut w, city) = very_unhappy_world();
    w.game.configure_players("core:handicap/king", &[P0]);
    w.game.debug_set_city_status(city, CityStatus::Puppet);
    w.end_turn();
    let save = classic_rules::persistence::encode::encode(&w.game, &w.rules);
    let loaded = classic_rules::persistence::decode::decode(&save, &w.rules).unwrap();
    assert_eq!(authoritative_hash(&w.game), authoritative_hash(&loaded));
    let auth = loaded.view().auth;
    assert_eq!(auth.settings().difficulty, "core:handicap/king");
    assert!(auth.settings().is_ai(P0));
    assert_eq!(
        auth.cities().get(city).unwrap().status,
        CityStatus::Puppet
    );
    let mut bad = save.clone();
    bad.world.economy.cities[0].process = 7;
    assert!(classic_rules::persistence::decode::decode(&bad, &w.rules).is_err());
    let mut bad = save;
    bad.world.economy.difficulty = "core:handicap/nobody".into();
    assert!(classic_rules::persistence::decode::decode(&bad, &w.rules).is_err());
}

fn check(w: &World, command: &Command) -> Result<(), classic_rules::command::CommandError> {
    let mut scratch = classic_rules::navigation::PathScratch::default();
    classic_rules::command::validate(w.game.view(), &w.rules, command.clone(), &mut scratch).map(|_| ())
}
