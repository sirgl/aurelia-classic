//! Core content as effect data (M3.0): the building and wonder bonuses that
//! were string kinds and the per-technology bonus that was code now are
//! typed effects in core.ron, and they give the same numbers as the v1
//! rules. A new building with effects needs one RON record and no code.
//! Runs on the frozen pre-classic content (`classic_content::legacy`): these are its
//! numbers, not those of the classic ruleset.

use classic_content::legacy::{legacy_rules as core_rules, CORE_RON};
use classic_rules::derived::yields::YieldState;
use classic_rules::ids::{BuildingKindId, CityHandle, PlayerId, ProductionItem, TechId};
use classic_rules::math::HexCoord;
use classic_rules::rules::effects::EffectSet;
use classic_rules::{compile_rules, Command, CompiledRuleset, GameConfig, GameState};

/// v1 numbers (core.ron before M3.0): flat `add_yield` and `multiply_yield`
/// percent (which was additive) per building.
const V1_BONUSES: [(&str, i32, i32); 15] = [
    ("core:building/bank", 0, 15),
    ("core:building/barracks", 3, 0),
    ("core:building/factory", 0, 20),
    ("core:building/granary", 2, 0),
    ("core:building/great_library", 5, 0),
    ("core:building/library", 3, 0),
    ("core:building/lighthouse", 3, 0),
    ("core:building/market", 4, 0),
    ("core:building/monument", 2, 0),
    ("core:building/oracle", 0, 15),
    ("core:building/pyramids", 4, 0),
    ("core:building/shrine", 2, 0),
    ("core:building/temple", 4, 0),
    ("core:building/university", 5, 0),
    ("core:building/workshop", 0, 15),
];

/// v1 order: base + flat, then + percent of that (integer division).
fn v1_output(base: i32, flat: i32, percent: i32) -> i32 {
    let v = base + flat;
    v + v * percent / 100
}

const P0: PlayerId = PlayerId(0);

fn new_game(rules: &CompiledRuleset) -> (GameState, CityHandle) {
    let config = GameConfig {
        players: 2,
        width: 16,
        height: 16,
        rng_seed: 7,
    };
    let mut game = GameState::new(rules, config);
    let tile = game
        .view()
        .auth
        .map()
        .dimensions
        .to_tile(HexCoord { q: 8, r: 8 })
        .unwrap();
    let city = game.debug_found_city(rules, P0, tile);
    (game, city)
}

fn building(rules: &CompiledRuleset, key: &str) -> BuildingKindId {
    let index = rules.buildings.iter().position(|b| b.key == key);
    BuildingKindId(index.unwrap_or_else(|| panic!("{key} must exist")) as u16)
}

/// Production: the legacy `Output` modifiers land on production.
fn output(game: &GameState, city: CityHandle) -> i32 {
    game.derived().yields.of(city).unwrap().production
}

#[test]
fn every_core_building_gives_its_v1_bonus() {
    let rules = compile_rules(core_rules()).unwrap();
    assert_eq!(rules.buildings.len(), V1_BONUSES.len());
    for (key, flat, percent) in V1_BONUSES {
        let (mut game, city) = new_game(&rules);
        let base = output(&game, city);
        game.debug_add_building(&rules, city, building(&rules, key));
        game.validate(&rules).unwrap();
        assert_eq!(output(&game, city), v1_output(base, flat, percent), "{key}");
    }
}

#[test]
fn monument_granary_and_wonders_together_give_v1_bonus() {
    let rules = compile_rules(core_rules()).unwrap();
    let (mut game, city) = new_game(&rules);
    let base = output(&game, city);
    let keys = [
        "core:building/monument",
        "core:building/granary",
        "core:building/pyramids",
        "core:building/great_library",
        "core:building/oracle",
    ];
    for key in keys {
        game.debug_add_building(&rules, city, building(&rules, key));
    }
    game.validate(&rules).unwrap();
    assert_eq!(output(&game, city), v1_output(base, 2 + 2 + 4 + 5, 15));
}

/// The same rules without technology effects: what v1 called the base.
fn without_tech_effects(rules: &CompiledRuleset) -> CompiledRuleset {
    let mut stripped = rules.clone();
    for tech in &mut stripped.technologies {
        tech.effects = EffectSet::default();
    }
    stripped
}

#[test]
fn technology_bonus_is_data_and_gives_v1_ten_percent_each() {
    let rules = compile_rules(core_rules()).unwrap();
    let (mut game, city) = new_game(&rules);
    game.debug_add_building(&rules, city, building(&rules, "core:building/monument"));
    for key in ["core:tech/agriculture", "core:tech/pottery"] {
        let index = rules
            .technologies
            .iter()
            .position(|t| t.key == key)
            .unwrap();
        let technology = TechId(index as u16);
        game.execute(
            &rules,
            Command::ChooseTechnology {
                player: P0,
                technology,
            },
        )
        .unwrap();
        for _ in 0..200 {
            if game
                .view()
                .auth
                .players()
                .techs_of(P0)
                .contains(&technology)
            {
                break;
            }
            game.execute(&rules, Command::EndTurn { player: P0 })
                .unwrap();
        }
    }
    assert_eq!(game.view().auth.players().techs_of(P0).len(), 2);
    let stripped = without_tech_effects(&rules);
    let base = YieldState::rebuild(game.view().auth, &stripped)
        .of(city)
        .unwrap()
        .production;
    assert_eq!(
        output(&game, city),
        v1_output(base, 0, 20),
        "two techs: +20%"
    );
}

/// core.ron plus one record: a building nobody wrote code for.
const NEW_BUILDING: &str = r#"
        (id: "core:building/aqueduct", cost: 30, required_tech: None, effects: [
            (scope: City, selector: SourceCity, modifier: AddYield(yield_kind: Output, amount: 7)),
            (scope: City, selector: SourceCity, trigger: OnBuilt, modifier: FreeUnit(unit: "core:unit/worker", count: 1), duration: Once),
        ]),"#;

fn rules_with_new_building() -> CompiledRuleset {
    let anchor = "    buildings: [";
    let at = CORE_RON
        .find(anchor)
        .expect("core.ron has a buildings table")
        + anchor.len();
    let text = format!("{}{}{}", &CORE_RON[..at], NEW_BUILDING, &CORE_RON[at..]);
    let mut source = classic_content::ron::parse_ron(&text).unwrap();
    source.civs = classic_content::legacy::legacy_civs();
    compile_rules(source).unwrap()
}

#[test]
fn new_building_with_effects_is_one_ron_record() {
    let rules = rules_with_new_building();
    let aqueduct = building(&rules, "core:building/aqueduct");
    let (mut game, city) = new_game(&rules);
    let item = ProductionItem::Building(aqueduct);
    game.execute(
        &rules,
        Command::EnqueueProduction {
            player: P0,
            city,
            item,
        },
    )
    .unwrap();
    for _ in 0..40 {
        if game
            .view()
            .auth
            .cities()
            .get(city)
            .unwrap()
            .buildings
            .contains(&aqueduct)
        {
            break;
        }
        game.execute(&rules, Command::EndTurn { player: P0 })
            .unwrap();
    }
    let core = game.view().auth.cities().get(city).unwrap();
    assert!(core.buildings.contains(&aqueduct), "aqueduct must be built");
    // OnBuilt: one free worker on the city tile.
    let worker = rules
        .units
        .iter()
        .position(|u| u.key == "core:unit/worker")
        .unwrap();
    let workers = game
        .view()
        .auth
        .units()
        .iter_cores()
        .filter(|u| u.kind.0 as usize == worker);
    assert_eq!(workers.count(), 1);
    // Passive: exactly +7 output compared with the same world without it.
    let mut stripped = rules.clone();
    stripped.buildings[aqueduct.0 as usize].effects = EffectSet::default();
    let without = YieldState::rebuild(game.view().auth, &stripped)
        .of(city)
        .unwrap()
        .production;
    assert_eq!(output(&game, city), without + 7);
    game.validate(&rules).unwrap();
}

#[test]
fn misspelt_effect_field_fails_to_parse() {
    let text = r#"(units: [], buildings: [(id: "core:building/x", cost: 5, effects: [
        (scope: City, selector: SourceCity, modifer: AddYield(yield_kind: Output, amount: 1)),
    ])])"#;
    assert!(classic_content::ron::parse_ron(text).is_err());
}

#[test]
fn ruleset_hash_tracks_one_number_in_core_ron() {
    let compile_text = |text: &str| {
        let mut source = classic_content::ron::parse_ron(text).unwrap();
        source.civs = classic_content::legacy::legacy_civs();
        classic_content::legacy::add_legacy_world(&mut source);
        compile_rules(source).unwrap()
    };
    let base = compile_text(CORE_RON).hash();
    assert_eq!(base, compile_rules(core_rules()).unwrap().hash());
    let granary = r#"(id: "core:building/granary", cost: 60,"#;
    assert!(CORE_RON.contains(granary));
    let cheaper = CORE_RON.replacen(granary, r#"(id: "core:building/granary", cost: 61,"#, 1);
    assert_ne!(compile_text(&cheaper).hash(), base, "granary cost 60 → 61");
    let tech_bonus = "percent: 10))]";
    let weaker = CORE_RON.replacen(tech_bonus, "percent: 9))]", 1);
    assert_ne!(
        compile_text(&weaker).hash(),
        base,
        "one tech effect 10% → 9%"
    );
}
