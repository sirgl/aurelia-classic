//! Compiler rules for effects (platform/project.md §19): every invalid combination is
//! a compile error before the game starts.

use crate::classic::effects::ClassicSourceModifier;
use crate::classic::effects::{ClassicCondition, ClassicCount, ClassicScope, ClassicSelector, ClassicTrigger};
use crate::rules::effects::{Condition, Count, Duration, Modifier, PerCount, Scope, Selector, Trigger, YieldKind};

use crate::classic::effects::ReligionAbility;
use crate::rules::source::{SourceBelief, SourceEffect, SourceModifier, StableContentKey};
use crate::rules::{compile_rules, CompiledRuleset, RulesError};

const UNITS: &str = r#"units: [
    (id: "core:unit/settler", movement: 2, strength: 1, founder: true, cost: 40),
    (id: "core:unit/warrior", movement: 2, strength: 10, cost: 30),
],
promotions: [(id: "core:promotion/shock", effects: [
    (scope: Unit, selector: SourceUnit, modifier: ModifyCombat(context: Any, percent: 15)),
])],"#;

/// A ruleset with one ordinary building carrying `effects` (RON list body).
fn with_building_effects(effects: &str) -> Result<CompiledRuleset, RulesError> {
    let text = format!(
        "({UNITS} buildings: [\
           (id: \"core:building/hall\", cost: 40, effects: [{effects}]),\
           (id: \"core:building/wonder\", cost: 90, wonder: true),\
         ], technologies: [(id: \"core:tech/agriculture\", cost: 20)])"
    );
    compile_rules(crate::fixtures::parse_ron_text(&text))
}

fn with_tech_effects(effects: &str) -> Result<CompiledRuleset, RulesError> {
    let text = format!(
        "({UNITS} buildings: [], technologies: [(id: \"core:tech/x\", cost: 5, effects: [{effects}])])"
    );
    compile_rules(crate::fixtures::parse_ron_text(&text))
}

fn expect_error(result: Result<CompiledRuleset, RulesError>, expected: fn(&RulesError) -> bool) {
    match result {
        Err(e) if expected(&e) => {}
        Err(e) => panic!("unexpected error: {e:?}"),
        Ok(_) => panic!("invalid content compiled"),
    }
}

#[test]
fn valid_effects_land_in_their_buckets_and_indexes() {
    let rules = with_building_effects(
        "(scope: City, selector: SourceCity, modifier: AddYield(yield_kind: Food, amount: 2)),\
         (scope: Player, selector: Owner, modifier: AddYield(yield_kind: Gold, amount: 1)),\
         (scope: City, selector: SourceCity, trigger: OnBuilt, modifier: FreeUnit(unit: \"core:unit/warrior\", count: 1), duration: Once)",
    )
    .unwrap();
    let hall = &rules.buildings[0];
    assert_eq!(hall.key, "core:building/hall");
    assert_eq!(hall.effects.passive_city.len(), 1);
    assert_eq!(hall.effects.passive_player.len(), 1);
    assert_eq!(hall.effects.triggered(Trigger::OnBuilt).len(), 1);
    let food = Modifier::AddYield {
        yield_kind: YieldKind::Food,
        amount: 2,
    };
    assert_eq!(hall.effects.passive_city[0].modifier, food);
    let index = &rules.effect_index;
    assert_eq!(index.sources(Trigger::OnBuilt).buildings.len(), 1);
    assert!(index.sources(Trigger::OnTurnStart).is_empty());
    assert_eq!(
        index.owner_wide_buildings.len(),
        1,
        "Owner selector is owner-wide"
    );
    let shock = &rules.promotions[0].effects.passive_unit[0];
    assert_eq!(shock.selector, Selector::SourceUnit);
}

#[test]
fn unknown_references_are_rejected() {
    expect_error(
        with_building_effects("(scope: City, selector: SourceCity, trigger: OnBuilt, modifier: FreeUnit(unit: \"core:unit/nope\", count: 1), duration: Once)"),
        |e| matches!(e, RulesError::UnknownReference(m) if m.contains("core:unit/nope") && m.contains("hall")),
    );
    expect_error(
        with_building_effects("(scope: City, selector: SourceCity, condition: HasTechnology(\"core:tech/nope\"), modifier: AddYield(yield_kind: Food, amount: 1))"),
        |e| matches!(e, RulesError::UnknownReference(_)),
    );
    expect_error(
        with_tech_effects("(scope: Unit, selector: EventUnit, trigger: OnUnitKilled, modifier: AddPromotion(\"core:promotion/nope\"), duration: Once)"),
        |e| matches!(e, RulesError::UnknownReference(_)),
    );
}

#[test]
fn invalid_selectors_are_rejected() {
    // The selector addresses another scope.
    expect_error(
        with_building_effects(
            "(scope: City, selector: Owner, modifier: AddYield(yield_kind: Food, amount: 1))",
        ),
        |e| matches!(e, RulesError::InvalidSelector(_)),
    );
    // A technology has no source city.
    expect_error(
        with_tech_effects(
            "(scope: City, selector: SourceCity, modifier: AddYield(yield_kind: Food, amount: 1))",
        ),
        |e| matches!(e, RulesError::InvalidSelector(_)),
    );
    // EventUnit exists only for OnUnitKilled.
    expect_error(
        with_tech_effects("(scope: Unit, selector: EventUnit, trigger: OnCityFounded, modifier: AddPromotion(\"core:promotion/shock\"), duration: Once)"),
        |e| matches!(e, RulesError::InvalidSelector(_)),
    );
}

#[test]
fn modifiers_conditions_and_triggers_are_checked() {
    expect_error(
        with_building_effects(
            "(scope: City, selector: SourceCity, modifier: ModifyCombat(context: Any, percent: 10))",
        ),
        |e| matches!(e, RulesError::InvalidModifier(_)),
    );
    expect_error(
        with_building_effects("(scope: City, selector: SourceCity, trigger: OnBuilt, modifier: FreeBuilding(\"core:building/wonder\"), duration: Once)"),
        |e| matches!(e, RulesError::InvalidModifier(_)),
    );
    expect_error(
        with_building_effects("(scope: Player, selector: Owner, condition: HasBuilding(\"core:building/hall\"), modifier: Module(AddHappiness(amount: 1)))"),
        |e| matches!(e, RulesError::InvalidCondition(_)),
    );
    expect_error(
        with_tech_effects("(scope: Player, selector: Owner, trigger: OnBuilt, modifier: AddYield(yield_kind: Gold, amount: 5), duration: Once)"),
        |e| matches!(e, RulesError::InvalidTrigger(_)),
    );
    expect_error(
        with_building_effects(
            "(scope: City, selector: SourceCity, modifier: FreeUnit(unit: \"core:unit/warrior\", count: 1))",
        ),
        |e| matches!(e, RulesError::InvalidTrigger(_)),
    );
}

#[test]
fn durations_must_fit_trigger_and_modifier() {
    expect_error(
        with_building_effects("(scope: City, selector: SourceCity, modifier: AddYield(yield_kind: Food, amount: 1), duration: Turns(3))"),
        |e| matches!(e, RulesError::InvalidDuration(_)),
    );
    // Output has no stock: a one-shot grant of it is meaningless.
    expect_error(
        with_tech_effects("(scope: City, selector: OwnerCities, trigger: OnTechnologyUnlocked, modifier: AddYield(yield_kind: Output, amount: 5), duration: Once)"),
        |e| matches!(e, RulesError::InvalidDuration(_)),
    );
    // Timed effects reach all the owner's units at once, never one unit.
    let all_units = with_tech_effects("(scope: Unit, selector: OwnerUnits, trigger: OnTurnStart, modifier: ModifyCombat(context: Any, percent: 5), duration: Turns(2), stacking: Unique)");
    assert!(all_units.is_ok(), "{all_units:?}");
    expect_error(
        with_tech_effects("(scope: Unit, selector: EventUnit, trigger: OnUnitKilled, modifier: ModifyCombat(context: Any, percent: 5), duration: Turns(2))"),
        |e| matches!(e, RulesError::InvalidDuration(_)),
    );
}

#[test]
fn forbidden_stacking_is_rejected() {
    expect_error(
        with_building_effects("(scope: City, selector: SourceCity, modifier: MultiplyYield(yield_kind: Food, percent: 150))"),
        |e| matches!(e, RulesError::InvalidStacking(_)),
    );
    expect_error(
        with_tech_effects("(scope: Player, selector: Owner, trigger: OnTechnologyUnlocked, modifier: AddYield(yield_kind: Gold, amount: 5), duration: Once, stacking: Unique)"),
        |e| matches!(e, RulesError::InvalidStacking(_)),
    );
    let multiplier = "(scope: City, selector: OwnerCities, modifier: MultiplyYield(yield_kind: Food, percent: 110), stacking: Unique)";
    let eight = [multiplier; 8].join(",");
    assert!(
        with_tech_effects(&eight).is_ok(),
        "the bound itself is allowed"
    );
    let nine = [multiplier; 9].join(",");
    expect_error(with_tech_effects(&nine), |e| {
        matches!(e, RulesError::InvalidStacking(_))
    });
}

#[test]
fn per_unit_sources_keep_passive_effects_on_the_unit() {
    let text = r#"(
        units: [(id: "core:unit/scout", movement: 2, strength: 1, cost: 10, effects: [
            (scope: Unit, selector: OwnerUnits, modifier: ModifyMovement(amount: 1)),
        ])],
    )"#;
    let result = compile_rules(crate::fixtures::parse_ron_text(text));
    expect_error(result, |e| matches!(e, RulesError::InvalidSelector(_)));
}

#[test]
fn per_count_modifiers_need_a_step_and_a_scope_of_their_count() {
    let ok = with_building_effects(
        "(scope: City, selector: SourceCity, modifier: AddYieldPerCount(yield_kind: Module(Faith), amount: 2, per: (count: WorldWonders, every: 1, max: None))),\
         (scope: Player, selector: Owner, modifier: Module(AddHappinessPerCount(amount: 1, per: (count: Module(Policies), every: 2, max: None))))",
    );
    assert!(ok.is_ok(), "{ok:?}");
    expect_error(
        with_building_effects(
            "(scope: City, selector: SourceCity, modifier: Module(AddHappinessPerCount(amount: 1, per: (count: Population, every: 0, max: None))))",
        ),
        |e| matches!(e, RulesError::InvalidModifier(_)),
    );
    // Policies are counted on a player, citizens on a city.
    expect_error(
        with_building_effects(
            "(scope: City, selector: SourceCity, modifier: Module(AddHappinessPerCount(amount: 1, per: (count: Module(Policies), every: 2, max: None))))",
        ),
        |e| matches!(e, RulesError::InvalidModifier(_)),
    );
    expect_error(
        with_building_effects(
            "(scope: Player, selector: Owner, modifier: AddYieldPerCount(yield_kind: Gold, amount: 1, per: (count: Population, every: 1, max: None)))",
        ),
        |e| matches!(e, RulesError::InvalidModifier(_)),
    );
}

#[test]
fn espionage_and_league_effects_are_checked() {
    // Valid: the National Intelligence Agency's shape.
    let rules = with_building_effects(
        "(scope: City, selector: OwnerCities, modifier: AddStat(stat: Module(Espionage(EnemyStealPercent)), amount: -15)),\
         (scope: Player, selector: Owner, modifier: AddStat(stat: Module(Espionage(StartingRank)), amount: 1)),\
         (scope: Player, selector: Owner, modifier: AddStat(stat: Module(League(Delegates)), amount: 2)),\
         (scope: Player, selector: Owner, trigger: OnBuilt, modifier: Module(PromoteSpies(ranks: 1)), duration: Once),\
         (scope: Player, selector: Owner, trigger: OnBuilt, modifier: Module(GrantSpies(count: 1)), duration: Once)",
    )
    .unwrap();
    assert_eq!(rules.buildings[0].effects.triggered(Trigger::OnBuilt).len(), 2);
    // The enemy steal rate is a city number; the others are player numbers.
    expect_error(
        with_building_effects("(scope: Player, selector: Owner, modifier: AddStat(stat: Module(Espionage(EnemyStealPercent)), amount: -25))"),
        |e| matches!(e, RulesError::InvalidModifier(_)),
    );
    expect_error(
        with_building_effects("(scope: City, selector: SourceCity, modifier: AddStat(stat: Module(Espionage(StealPercent)), amount: 100))"),
        |e| matches!(e, RulesError::InvalidModifier(_)),
    );
    expect_error(
        with_building_effects("(scope: City, selector: SourceCity, modifier: AddStat(stat: Module(League(Delegates)), amount: 2))"),
        |e| matches!(e, RulesError::InvalidModifier(_)),
    );
    // Spies are granted once, to a player, at least one.
    expect_error(
        with_building_effects("(scope: Player, selector: Owner, modifier: Module(GrantSpies(count: 1)))"),
        |e| matches!(e, RulesError::InvalidTrigger(_)),
    );
    expect_error(
        with_building_effects("(scope: City, selector: SourceCity, trigger: OnBuilt, modifier: Module(GrantSpies(count: 1)), duration: Once)"),
        |e| matches!(e, RulesError::InvalidModifier(_)),
    );
    expect_error(
        with_building_effects("(scope: Player, selector: Owner, trigger: OnBuilt, modifier: Module(PromoteSpies(ranks: 0)), duration: Once)"),
        |e| matches!(e, RulesError::InvalidModifier(_)),
    );
}

#[test]
fn yields_from_kills_need_a_kill_a_player_and_a_player_stock() {
    let ok = with_tech_effects(
        "(scope: Player, selector: Owner, condition: Module(OpponentIsBarbarian), trigger: OnUnitKilled, modifier: YieldFromKill(yield_kind: Culture, percent: 100), duration: Once)",
    );
    assert!(ok.is_ok(), "{ok:?}");
    // Another trigger has no victim.
    expect_error(
        with_tech_effects("(scope: Player, selector: Owner, trigger: OnTurnStart, modifier: YieldFromKill(yield_kind: Gold, percent: 100), duration: Once)"),
        |e| matches!(e, RulesError::InvalidTrigger(_)),
    );
    // Food has no player pool; a city is not a target.
    expect_error(
        with_tech_effects("(scope: Player, selector: Owner, trigger: OnUnitKilled, modifier: YieldFromKill(yield_kind: Food, percent: 100), duration: Once)"),
        |e| matches!(e, RulesError::InvalidModifier(_)),
    );
    expect_error(
        with_tech_effects("(scope: City, selector: OwnerCities, trigger: OnUnitKilled, modifier: YieldFromKill(yield_kind: Gold, percent: 100), duration: Once)"),
        |e| matches!(e, RulesError::InvalidModifier(_)),
    );
    // A passive grant never fires.
    expect_error(
        with_tech_effects("(scope: Player, selector: Owner, modifier: YieldFromKill(yield_kind: Gold, percent: 100))"),
        |e| matches!(e, RulesError::InvalidTrigger(_)),
    );
}

#[test]
fn event_conditions_need_their_event_and_a_source_city() {
    let kill = "(scope: Player, selector: Owner, condition: EventNearSourceCity(4), trigger: OnUnitKilled, modifier: YieldFromKill(yield_kind: Module(Faith), percent: 50), duration: Once)";
    let ok = with_building_effects(kill);
    assert!(ok.is_ok(), "{ok:?}");
    // A technology has no city.
    expect_error(with_tech_effects(kill), |e| matches!(e, RulesError::InvalidCondition(_)));
    // Outside a kill there is neither a victim nor an event tile.
    expect_error(
        with_building_effects("(scope: Player, selector: Owner, condition: EventNearSourceCity(4), trigger: OnTurnStart, modifier: AddYield(yield_kind: Gold, amount: 5), duration: Once)"),
        |e| matches!(e, RulesError::InvalidCondition(_)),
    );
    expect_error(
        with_tech_effects("(scope: Player, selector: Owner, condition: Module(OpponentIsBarbarian), trigger: OnTurnStart, modifier: AddYield(yield_kind: Gold, amount: 5), duration: Once)"),
        |e| matches!(e, RulesError::InvalidCondition(_)),
    );
}

/// A ruleset with one belief carrying `effects`.
fn with_belief_effects(effects: &str) -> Result<CompiledRuleset, RulesError> {
    let text = format!(
        "({UNITS} buildings: [], technologies: [], beliefs: [(id: \"core:belief/pantheon/x\", effects: [{effects}])])"
    );
    compile_rules(crate::fixtures::parse_ron_text(&text))
}

#[test]
fn near_following_city_is_a_belief_condition_of_combat_and_healing() {
    let combat = "(scope: Unit, selector: OwnerUnits, condition: Module(NearFollowingCity(Own)), modifier: ModifyCombat(context: Any, percent: 20))";
    let heal = "(scope: Unit, selector: OwnerUnits, condition: Module(NearFollowingCity(Own)), modifier: ModifyUnitStat(stat: HealFriendlyLand, amount: 30))";
    let ok = with_belief_effects(&format!("{combat}, {heal}"));
    assert!(ok.is_ok(), "{ok:?}");
    // Only a belief has a creed to follow.
    expect_error(with_tech_effects(combat), |e| matches!(e, RulesError::InvalidCondition(_)));
    // No system reads other stats or movement near a following city.
    expect_error(
        with_belief_effects("(scope: Unit, selector: OwnerUnits, condition: Module(NearFollowingCity(Enemy)), modifier: ModifyUnitStat(stat: Sight, amount: 1))"),
        |e| matches!(e, RulesError::InvalidCondition(_)),
    );
    expect_error(
        with_belief_effects("(scope: Unit, selector: OwnerUnits, condition: Module(NearFollowingCity(Own)), modifier: GrantAbility(DoubleMove))"),
        |e| matches!(e, RulesError::InvalidCondition(_)),
    );
}

#[test]
fn tile_conditions_need_their_scope_and_tiles_have_no_multiplier() {
    let ok = with_building_effects(
        "(scope: Tile, selector: SourceCityTiles, condition: TilePlot(River), modifier: AddYield(yield_kind: Production, amount: 1)),\
         (scope: Tile, selector: SourceCityTiles, condition: TileResourceClass(Strategic), modifier: AddYieldPercent(yield_kind: Food, percent: 100))",
    )
    .unwrap();
    assert_eq!(ok.buildings[0].effects.passive_tile.len(), 2);
    expect_error(
        with_building_effects("(scope: City, selector: SourceCity, condition: TilePlot(Sea), modifier: AddYield(yield_kind: Food, amount: 1))"),
        |e| matches!(e, RulesError::InvalidCondition(_)),
    );
    expect_error(
        with_building_effects("(scope: City, selector: SourceCity, condition: TileResourceClass(Luxury), modifier: AddYield(yield_kind: Food, amount: 1))"),
        |e| matches!(e, RulesError::InvalidCondition(_)),
    );
    expect_error(
        with_building_effects("(scope: Tile, selector: SourceCityTiles, condition: TileFeatureUnimproved(\"core:feature/nope\"), modifier: AddYield(yield_kind: Food, amount: 1))"),
        |e| matches!(e, RulesError::UnknownReference(_)),
    );
    expect_error(
        with_building_effects("(scope: Tile, selector: SourceCityTiles, condition: CityAdjacentUnimprovedFeature(feature: \"core:feature/nope\", at_least: 1), modifier: AddYield(yield_kind: Food, amount: 1))"),
        |e| matches!(e, RulesError::UnknownReference(_)),
    );
    expect_error(
        with_building_effects("(scope: Tile, selector: SourceCityTiles, stacking: Unique, modifier: MultiplyYield(yield_kind: Food, percent: 200))"),
        |e| matches!(e, RulesError::InvalidModifier(_)),
    );
}

#[test]
fn natural_wonder_placement_bounds_are_checked() {
    use crate::rules::natural_wonders::{AdjacentBound, AdjacentKind, AdjacentRule};
    use crate::rules::source::{SourceNaturalWonder, StableContentKey};
    let with_rules = |adjacent: Vec<AdjacentRule<StableContentKey, StableContentKey>>| {
        let mut source = crate::fixtures::parse_ron_text("()");
        let mut wonder = SourceNaturalWonder {
            id: Some(StableContentKey::new("core:natural-wonder/test")),
            name: "Test".into(),
            ..Default::default()
        };
        wonder.placement.adjacent = adjacent;
        source.natural_wonders.push(wonder);
        compile_rules(source)
    };
    let hills = |bound| AdjacentRule { kind: AdjacentKind::Hills, bound };
    assert!(with_rules(vec![hills(AdjacentBound::AtLeast(2))]).is_ok());
    expect_error(with_rules(vec![hills(AdjacentBound::AtMost(7))]), |e| {
        matches!(e, RulesError::InvalidModifier(_))
    });
    expect_error(
        with_rules(vec![hills(AdjacentBound::AtLeast(1)), hills(AdjacentBound::AtMost(3))]),
        |e| matches!(e, RulesError::InvalidModifier(_)),
    );
    let unknown = AdjacentRule {
        kind: AdjacentKind::Feature(StableContentKey::new("core:feature/nowhere")),
        bound: AdjacentBound::AtMost(0),
    };
    expect_error(with_rules(vec![unknown]), |e| matches!(e, RulesError::UnknownReference(_)));
}

#[test]
fn producing_conditions_gate_only_city_production_percents() {
    let percent = "modifier: AddYieldPercent(yield_kind: Production, percent: 15)";
    let rules = with_building_effects(&format!(
        "(scope: City, selector: SourceCity, condition: Producing(UnitDomain(Land)), {percent}),\
         (scope: City, selector: OwnerCities, condition: Producing(UnitClassOf(\"core:unit/settler\")), {percent}),\
         (scope: City, selector: OwnerCities, condition: Producing(BuiltInCapital), {percent})"
    ))
    .unwrap();
    assert!(rules.effect_index.has_production_percents);
    assert!(rules.effect_index.has_capital_production);
    // Another modifier, another scope, a trigger: errors.
    let invalid = [
        "(scope: City, selector: SourceCity, condition: Producing(Units), modifier: AddYield(yield_kind: Production, amount: 2))".to_string(),
        "(scope: City, selector: SourceCity, condition: Producing(Units), modifier: AddYieldPercent(yield_kind: Food, percent: 10))".to_string(),
        "(scope: Player, selector: Owner, condition: Producing(Buildings), modifier: AddYieldPercent(yield_kind: Gold, percent: 10))".to_string(),
        format!("(scope: City, selector: SourceCity, condition: Producing(Buildings), trigger: OnBuilt, {percent}, duration: Turns(3))"),
    ];
    for effect in invalid {
        expect_error(with_building_effects(&effect), |e| matches!(e, RulesError::InvalidCondition(_)));
    }
    let unknown = [
        format!("(scope: City, selector: SourceCity, condition: Producing(BuildingClassOf(\"core:building/nope\")), {percent})"),
        format!("(scope: City, selector: SourceCity, condition: Producing(WorldWonders(before_era: Some(\"core:era/nope\"))), {percent})"),
    ];
    for effect in unknown {
        expect_error(with_building_effects(&effect), |e| matches!(e, RulesError::UnknownReference(_)));
    }
}

#[test]
fn purchase_costs_and_player_stats_need_their_scopes() {
    with_building_effects(
        "(scope: City, selector: OwnerCities, modifier: ModifyCost(target: UnitPurchase, percent: -33)),\
         (scope: City, selector: OwnerCities, modifier: ModifyCost(target: BuildingPurchase, percent: -33)),\
         (scope: Player, selector: Owner, modifier: AddStat(stat: WorkerSpeedPercent, amount: 25)),\
         (scope: Player, selector: Owner, modifier: AddStat(stat: RouteMaintenancePercent, amount: -50)),\
         (scope: Player, selector: Owner, modifier: AddStat(stat: Module(Rule(GarrisonFreeMaintenance)), amount: 1))",
    )
    .unwrap();
    let invalid = [
        "(scope: Player, selector: Owner, modifier: ModifyCost(target: UnitPurchase, percent: -33))",
        "(scope: City, selector: SourceCity, modifier: AddStat(stat: WorkerSpeedPercent, amount: 25))",
        "(scope: City, selector: SourceCity, modifier: AddStat(stat: Module(Rule(SpaceshipPartHurry)), amount: 1))",
    ];
    for effect in invalid {
        expect_error(with_building_effects(effect), |e| matches!(e, RulesError::InvalidModifier(_)));
    }
}

#[test]
fn grants_need_their_scope_a_trigger_and_a_count() {
    let ok = with_building_effects(
        "(scope: Player, selector: Owner, trigger: OnBuilt, modifier: Grant(FreeTechnologies(1)), duration: Once),\
         (scope: Player, selector: Owner, trigger: OnBuilt, modifier: Grant(Module(GoldenAge(turns: 10))), duration: Once),\
         (scope: City, selector: SourceCity, trigger: OnBuilt, modifier: Grant(MilitaryUnitCopies), duration: Once),\
         (scope: City, selector: OwnerCities, trigger: OnBuilt, modifier: Grant(Population(1)), duration: Once),\
         (scope: Player, selector: Owner, trigger: OnCityConquered, modifier: Grant(ConquestTechnology), duration: Once)",
    );
    assert!(ok.is_ok(), "{ok:?}");
    // A grant is instant: never passive.
    expect_error(
        with_building_effects("(scope: Player, selector: Owner, modifier: Grant(Module(FreeGreatPeople(1))))"),
        |e| matches!(e, RulesError::InvalidTrigger(_)),
    );
    // Citizens land in a city, technologies with a player.
    expect_error(
        with_building_effects("(scope: Player, selector: Owner, trigger: OnBuilt, modifier: Grant(Population(1)), duration: Once)"),
        |e| matches!(e, RulesError::InvalidModifier(_)),
    );
    expect_error(
        with_building_effects("(scope: City, selector: SourceCity, trigger: OnBuilt, modifier: Grant(FreeTechnologies(1)), duration: Once)"),
        |e| matches!(e, RulesError::InvalidModifier(_)),
    );
    expect_error(
        with_building_effects("(scope: Player, selector: Owner, trigger: OnBuilt, modifier: Grant(FreeTechnologies(0)), duration: Once)"),
        |e| matches!(e, RulesError::InvalidModifier(_)),
    );
    expect_error(
        with_building_effects("(scope: Player, selector: Owner, trigger: OnBuilt, modifier: Grant(Module(GoldenAge(turns: 0))), duration: Once)"),
        |e| matches!(e, RulesError::InvalidModifier(_)),
    );
    // A conquest technology needs the conquered city's previous owner.
    expect_error(
        with_building_effects("(scope: Player, selector: Owner, trigger: OnBuilt, modifier: Grant(ConquestTechnology), duration: Once)"),
        |e| matches!(e, RulesError::InvalidTrigger(_)),
    );
    // A grant is one-shot, not timed.
    expect_error(
        with_building_effects("(scope: Player, selector: Owner, trigger: OnBuilt, modifier: Grant(FreeTechnologies(1)), duration: Turns(3))"),
        |e| matches!(e, RulesError::InvalidDuration(_)),
    );
}

#[test]
fn society_conditions_need_their_scope_and_event() {
    let ok = with_building_effects(
        "(scope: City, selector: OwnerCities, condition: CityGarrisoned, modifier: Module(AddHappiness(amount: 1))),\
         (scope: City, selector: OwnerCities, condition: CityCoastal, modifier: AddYield(yield_kind: Production, amount: 1)),\
         (scope: Player, selector: Owner, condition: Module(GoldenAge), modifier: AddStat(stat: Module(GreatPeopleRate), amount: 50)),\
         (scope: City, selector: Capital, condition: LearnedTechnology(\"core:tech/agriculture\"), trigger: OnTechnologyUnlocked, modifier: FreeUnit(unit: \"core:unit/warrior\", count: 1), duration: Once),\
         (scope: City, selector: EventCity, condition: CityCoastal, trigger: OnCityFounded, modifier: FreeBuilding(\"core:building/hall\"), duration: Once),\
         (scope: City, selector: EventCity, trigger: OnCityConquered, modifier: FreeBuilding(\"core:building/hall\"), duration: Once)",
    );
    assert!(ok.is_ok(), "{ok:?}");
    assert!(ok.unwrap().effect_index.has_garrison_condition);
    // A garrison belongs to a city.
    expect_error(
        with_building_effects("(scope: Player, selector: Owner, condition: CityGarrisoned, modifier: Module(AddHappiness(amount: 1)))"),
        |e| matches!(e, RulesError::InvalidCondition(_)),
    );
    // The learned technology is a fact of OnTechnologyUnlocked only.
    expect_error(
        with_building_effects("(scope: Player, selector: Owner, condition: LearnedTechnology(\"core:tech/agriculture\"), modifier: Module(AddHappiness(amount: 1)))"),
        |e| matches!(e, RulesError::InvalidCondition(_)),
    );
    // EventCity: founding or conquest only.
    expect_error(
        with_building_effects("(scope: City, selector: EventCity, trigger: OnEraEntered, modifier: FreeBuilding(\"core:building/hall\"), duration: Once)"),
        |e| matches!(e, RulesError::InvalidSelector(_)),
    );
}

#[test]
fn bans_and_society_stats_are_passive_on_their_scope() {
    let ok = with_tech_effects(
        "(scope: Player, selector: Owner, modifier: Forbid(UnitClassOf(\"core:unit/settler\"))),\
         (scope: Player, selector: Owner, modifier: Forbid(Annexing)),\
         (scope: Player, selector: Owner, modifier: AddStat(stat: Module(Society(RazeSpeedPercent)), amount: 100)),\
         (scope: City, selector: OwnerCities, modifier: AddStat(stat: Module(Society(SpecialistFoodPercent)), amount: -50)),\
         (scope: Player, selector: Owner, modifier: Module(AddHappinessPerCount(amount: 1, per: (count: ConnectedCities, every: 1, max: None))))",
    );
    assert!(ok.is_ok(), "{ok:?}");
    expect_error(
        with_tech_effects("(scope: Player, selector: Owner, trigger: OnTechnologyUnlocked, modifier: Forbid(Annexing), duration: Once)"),
        |e| matches!(e, RulesError::InvalidTrigger(_)),
    );
    expect_error(
        with_tech_effects("(scope: City, selector: OwnerCities, modifier: Forbid(Annexing))"),
        |e| matches!(e, RulesError::InvalidModifier(_)),
    );
    expect_error(
        with_tech_effects("(scope: City, selector: OwnerCities, modifier: AddStat(stat: Module(Society(RazeSpeedPercent)), amount: 100))"),
        |e| matches!(e, RulesError::InvalidModifier(_)),
    );
    expect_error(
        with_tech_effects("(scope: Player, selector: Owner, modifier: AddStat(stat: Module(Society(NoOccupiedUnhappiness)), amount: 1))"),
        |e| matches!(e, RulesError::InvalidModifier(_)),
    );
    expect_error(
        with_tech_effects("(scope: City, selector: OwnerCities, modifier: Module(AddHappinessPerCount(amount: 1, per: (count: ConnectedCities, every: 1, max: None))))"),
        |e| matches!(e, RulesError::InvalidModifier(_)),
    );
}

/// The fixture ruleset plus one belief with `effects` (fixtures have no
/// belief table of their own).
fn with_belief_source_effects(effects: Vec<SourceEffect>) -> Result<CompiledRuleset, RulesError> {
    let text = format!("({UNITS} buildings: [], technologies: [(id: \"core:tech/agriculture\", cost: 20)])");
    let mut source = crate::fixtures::parse_ron_text(&text);
    source.module.beliefs.push(SourceBelief {
        id: Some(StableContentKey::new("core:belief/test".to_string())),
        effects,
        ..Default::default()
    });
    compile_rules(source)
}

fn effect(scope: Scope, selector: Selector, trigger: Trigger, modifier: SourceModifier) -> SourceEffect {
    let once = trigger != Trigger::Passive;
    SourceEffect {
        trigger,
        duration: if once { Duration::Once } else { Duration::WhileSourceExists },
        ..SourceEffect::passive(scope, selector, modifier)
    }
}

#[test]
fn a_second_condition_is_owner_level_and_follows_a_first_one() {
    let ok = with_building_effects(
        "(scope: City, selector: OwnerCities, condition: HasBuilding(\"core:building/wonder\"), second_condition: Module(MinFollowers(3)), modifier: Module(AddHappiness(amount: 1)))",
    );
    let rules = ok.expect("building and followers compile");
    let e = rules.buildings[0].effects.passive_city[0];
    assert_eq!(e.second_condition, Condition::Module(ClassicCondition::MinFollowers(3)));
    expect_error(
        with_building_effects(
            "(scope: City, selector: OwnerCities, second_condition: Module(MinFollowers(3)), modifier: Module(AddHappiness(amount: 1)))",
        ),
        |e| matches!(e, RulesError::InvalidCondition(_)),
    );
    // Unit scope keeps one condition: combat and movement never read two.
    expect_error(
        with_building_effects(
            "(scope: Unit, selector: OwnerUnits, condition: UnitDomain(Land), second_condition: HasTechnology(\"core:tech/agriculture\"), modifier: ModifyMovement(amount: 1))",
        ),
        |e| matches!(e, RulesError::InvalidCondition(_)),
    );
    expect_error(
        with_building_effects(
            "(scope: City, selector: SourceCity, condition: MinPopulation(2), second_condition: Module(OpponentIsBarbarian), modifier: AddYield(yield_kind: Food, amount: 1))",
        ),
        |e| matches!(e, RulesError::InvalidCondition(_)),
    );
    expect_error(
        with_building_effects(
            "(scope: Player, selector: Owner, condition: HasTechnology(\"core:tech/agriculture\"), second_condition: Module(MinFollowers(3)), modifier: Module(AddHappiness(amount: 1)))",
        ),
        |e| matches!(e, RulesError::InvalidCondition(_)),
    );
}

#[test]
fn river_peace_and_connection_conditions_have_their_scopes_and_indexes() {
    let rules = with_building_effects(
        "(scope: City, selector: OwnerCities, condition: CityOnRiver, modifier: Module(AddHappiness(amount: 1))),\
         (scope: City, selector: OwnerCities, condition: AtPeace, modifier: ModifyGrowth(percent: 15)),\
         (scope: City, selector: OwnerCities, condition: ConnectedToCapital, modifier: AddYield(yield_kind: Science, amount: 2))",
    )
    .unwrap();
    assert!(rules.effect_index.uses_capital_connection);
    assert!(rules.effect_index.uses_at_peace);
    let plain = with_building_effects("(scope: City, selector: SourceCity, modifier: Module(AddHappiness(amount: 1)))").unwrap();
    assert!(!plain.effect_index.uses_capital_connection && !plain.effect_index.uses_at_peace);
    for wrong in [
        "(scope: Player, selector: Owner, condition: CityOnRiver, modifier: Module(AddHappiness(amount: 1)))",
        "(scope: Player, selector: Owner, condition: ConnectedToCapital, modifier: Module(AddHappiness(amount: 1)))",
        "(scope: Unit, selector: OwnerUnits, condition: AtPeace, modifier: ModifyMovement(amount: 1))",
    ] {
        expect_error(with_building_effects(wrong), |e| matches!(e, RulesError::InvalidCondition(_)));
    }
}

#[test]
fn religion_and_great_person_triggers_are_checked() {
    // A one-shot faith grant lands in the faith pool now (YieldKind::Faith has a stock).
    let ok = with_building_effects(
        "(scope: Player, selector: Owner, trigger: Module(OnGreatPersonExpended), modifier: AddYield(yield_kind: Module(Faith), amount: 50), duration: Once)",
    )
    .unwrap();
    assert_eq!(ok.effect_index.sources(Trigger::Module(ClassicTrigger::OnGreatPersonExpended)).buildings.len(), 1);
    // Conversions and spreads belong to a religion's beliefs.
    for trigger in ["Module(OnCityConverted)", "Module(OnReligionSpread)"] {
        let text = format!(
            "(scope: Player, selector: Owner, trigger: {trigger}, modifier: AddYield(yield_kind: Gold, amount: 100), duration: Once)"
        );
        expect_error(with_building_effects(&text), |e| matches!(e, RulesError::InvalidTrigger(_)));
    }
    let spread_science = SourceModifier::AddYieldPerCount {
        yield_kind: YieldKind::Science,
        amount: 10,
        per: PerCount { count: Count::Module(ClassicCount::OtherReligionFollowers), every: 1, max: None },
    };
    let ok = with_belief_source_effects(vec![effect(Scope::Player, Selector::Owner, Trigger::Module(ClassicTrigger::OnReligionSpread), spread_science.clone())]);
    assert!(ok.is_ok(), "{ok:?}");
    // The spread count is read on the spread's city only.
    expect_error(
        with_belief_source_effects(vec![effect(Scope::Player, Selector::Owner, Trigger::Module(ClassicTrigger::OnCityConverted), spread_science.clone())]),
        |e| matches!(e, RulesError::InvalidModifier(_)),
    );
    // Triggered belief effects address the event's player, not the founder.
    expect_error(
        with_belief_source_effects(vec![effect(Scope::Player, Selector::Module(ClassicSelector::Founder), Trigger::Module(ClassicTrigger::OnReligionSpread), spread_science)]),
        |e| matches!(e, RulesError::InvalidSelector(_)),
    );
}

#[test]
fn religion_rules_have_their_scopes_and_sources() {
    let ok = with_belief_source_effects(vec![effect(
        Scope::Module(ClassicScope::Religion),
        Selector::Module(ClassicSelector::OwnReligion),
        Trigger::Passive,
        SourceModifier::Module(ClassicSourceModifier::GrantReligionAbility(ReligionAbility::ConvertsBarbarians)),
    )]);
    assert!(ok.is_ok(), "{ok:?}");
    expect_error(
        with_belief_source_effects(vec![effect(
            Scope::Player,
            Selector::Module(ClassicSelector::Founder),
            Trigger::Passive,
            SourceModifier::Module(ClassicSourceModifier::GrantReligionAbility(ReligionAbility::ConvertsBarbarians)),
        )]),
        |e| matches!(e, RulesError::InvalidModifier(_)),
    );
    // The second-religion pantheon is a policy's (its index lists policies).
    expect_error(
        with_building_effects("(scope: Player, selector: Owner, modifier: Module(SecondReligionPantheon))"),
        |e| matches!(e, RulesError::InvalidModifier(_)),
    );
    assert!(with_building_effects("(scope: Player, selector: Owner, modifier: Module(ExtraFoundingBelief))").is_ok());
    expect_error(
        with_building_effects("(scope: City, selector: SourceCity, modifier: Module(ExtraFoundingBelief))"),
        |e| matches!(e, RulesError::InvalidModifier(_)),
    );
}

#[test]
fn great_person_births_take_owner_sources_and_their_kind_condition() {
    let futurism = "(scope: Player, selector: Owner, condition: Module(GreatPersonIs(Writer)), trigger: Module(OnGreatPersonBorn), modifier: Module(InstantTourism(amount: 250)), duration: Once)";
    let rules = with_tech_effects(futurism).unwrap();
    assert!(!rules.effect_index.sources(Trigger::Module(ClassicTrigger::OnGreatPersonBorn)).is_empty());
    // A building or a unit has no say in a birth.
    expect_error(with_building_effects(futurism), |e| matches!(e, RulesError::InvalidTrigger(_)));
    // The kind of the event's great person exists only at a birth.
    expect_error(
        with_tech_effects("(scope: Player, selector: Owner, condition: Module(GreatPersonIs(Writer)), trigger: OnTurnStart, modifier: AddYield(yield_kind: Gold, amount: 5), duration: Once)"),
        |e| matches!(e, RulesError::InvalidCondition(_)),
    );
    expect_error(
        with_tech_effects("(scope: Player, selector: Owner, condition: Module(GreatPersonIs(Writer)), modifier: AddYield(yield_kind: Gold, amount: 1))"),
        |e| matches!(e, RulesError::InvalidCondition(_)),
    );
    // Instant tourism: a trigger, a player, an amount.
    expect_error(
        with_tech_effects("(scope: Player, selector: Owner, modifier: Module(InstantTourism(amount: 250)))"),
        |e| matches!(e, RulesError::InvalidTrigger(_)),
    );
    expect_error(
        with_tech_effects("(scope: Player, selector: Owner, trigger: Module(OnGreatPersonBorn), modifier: Module(InstantTourism(amount: 0)), duration: Once)"),
        |e| matches!(e, RulesError::InvalidModifier(_)),
    );
    expect_error(
        with_tech_effects("(scope: City, selector: Capital, trigger: Module(OnGreatPersonBorn), modifier: Module(InstantTourism(amount: 250)), duration: Once)"),
        |e| matches!(e, RulesError::InvalidModifier(_)),
    );
}

#[test]
fn trade_and_diplomacy_stats_and_route_counts_keep_their_scopes() {
    let ok = with_building_effects(
        "(scope: City, selector: SourceCity, modifier: AddYieldPercentPerCount(yield_kind: Production, percent: 5, per: (count: Module(CityStateRoutes), every: 1, max: None))),\
         (scope: City, selector: Capital, modifier: AddYieldPerCount(yield_kind: Gold, amount: 3, per: (count: Module(TradePartners), every: 1, max: None))),\
         (scope: Player, selector: Owner, modifier: AddStat(stat: Module(ConnectionGoldPercent), amount: 25)),\
         (scope: Player, selector: Owner, modifier: AddStat(stat: Module(Trade(RoutesPercent)), amount: 100)),\
         (scope: Player, selector: Owner, modifier: Module(ModifyCityState(stat: AfraidInfluence, amount: 6)))",
    )
    .unwrap();
    assert!(ok.effect_index.module.trade_counts, "a route count refreshes cities when routes change");
    let no_counts = with_building_effects("(scope: City, selector: SourceCity, modifier: AddYield(yield_kind: Food, amount: 1))").unwrap();
    assert!(!no_counts.effect_index.module.trade_counts);
    for stat in [
        "ConnectionGoldPercent",
        "ResearchAgreementPercent",
        "GreatScientistSciencePercent",
        "GreatPeopleRatePerFriend",
        "Trade(RoutesPercent)",
        "Trade(IncomingRouteGold)",
        "Trade(ReligiousPressurePercent)",
    ] {
        let text = format!("(scope: City, selector: SourceCity, modifier: AddStat(stat: Module({stat}), amount: 1))");
        expect_error(with_building_effects(&text), |e| matches!(e, RulesError::InvalidModifier(_)));
    }
    expect_error(
        with_building_effects("(scope: City, selector: SourceCity, modifier: Module(ModifyCityState(stat: AllyScience, amount: 1)))"),
        |e| matches!(e, RulesError::InvalidModifier(_)),
    );
}
