//! Catalog effects of the combat system (promotions, buildings, policies,
//! technologies, nations) → typed RON effects: combat contexts with
//! their situational conditions, unit stats, unit abilities, city combat
//! stats, free experience, passive promotions (classic/docs/classic/rules.md).
//!
//! Returns None when the effect is not a military one (the generic mapping
//! of `effects.rs` handles it), Some(Err(mechanic)) when it is military but
//! does not fit (it stays inert, `mechanic` names why).

use serde_json::Value;

use crate::import::effects::Owner;
use crate::import::ron_out::quote;

/// `combat.<name>.pct` → CombatContext.
fn combat_context(name: &str) -> Option<&'static str> {
    Some(match name {
        "strength" => "Any",
        "attack" => "Attack",
        "defense" => "Defense",
        "attack_vs_city" => "VsCity",
        "attack_open_terrain" => "AttackOpen",
        "attack_rough_terrain" => "AttackRough",
        "defense_open_terrain" => "DefenseOpen",
        "defense_rough_terrain" => "DefenseRough",
        "ranged_attack_open_terrain" => "RangedAttackOpen",
        "ranged_attack_rough_terrain" => "RangedAttackRough",
        "defense_vs_ranged" => "DefenseVsRanged",
        "ranged_attack" => "RangedAttack",
        "attack_vs_wounded" => "VsWounded",
        "attack_vs_fortified" => "VsFortified",
        "in_friendly_land" => "FriendlyLand",
        "outside_friendly_land" => "OutsideFriendlyLand",
        "attack_in_friendly_land" => "AttackFriendlyLand",
        "flank_bonus" => "Flanking",
        "adjacent_friendly_unit" => "AdjacentFriendly",
        "near_capital" => "NearCapital",
        "embarked_defense" => "EmbarkedDefense",
        _ => return None,
    })
}

/// Catalog modifier → UnitStat (flat amount).
fn unit_stat(modifier: &str, selector: &str) -> Option<&'static str> {
    Some(match (modifier, selector) {
        // China's trait raises the general's aura; a promotion's raises
        // the general points of its unit's battles.
        ("combat.great_general_bonus.pct", "all_units") => "GeneralAuraPercent",
        ("combat.great_general_bonus.pct", _) => "GreatGeneralPointsPercent",
        // A unit's own bonus; the player's (Entrepreneurship) is a
        // city-state stat.
        ("trade_mission.gold.pct", "this_unit") => "TradeMissionGoldPercent",
        ("trade_mission.influence.pct", "this_unit") => "TradeMissionInfluencePercent",
        ("damage.on_attack.chance_pct", _) => "AdjacentDamageChance",
        ("religion.strength_loss_rival_land.pct", _) => "RivalLandStrengthLossPercent",
        ("golden_age_points.on_kill.pct_of_strength", _) => "GoldenAgePointsOnKillPercent",
        ("unit.maintenance.pct", _) => "MaintenancePercent",
        ("unit.xp_needed.pct", _) => "ExperienceNeededPercent",
        ("combat.withdraw_chance.pct", _) => "WithdrawChance",
        ("combat.city_attack_gold.pct_of_damage", _) => "CityAttackPlunderPercent",
        ("combat.wounded_penalty.pct", _) => "WoundedDamagePercent",
        ("combat.strength.pct", "enemy_units_in_range") => "EnemyAuraPercent",
        ("air.intercept_chance.flat", _) => "InterceptChance",
        ("air.evasion.pct", _) => "EvasionPercent",
        ("air.interceptions_per_turn.flat", _) => "InterceptionsPerTurn",
        ("air.damage_taken_from_interception.pct", _) => "InterceptionDamageTakenPercent",
        ("unit.sight.flat", _) => "Sight",
        ("unit.embarked_sight.flat", _) => "EmbarkedSight",
        ("unit.range.flat", _) => "Range",
        ("unit.attacks_per_turn.flat", _) => "AttacksPerTurn",
        ("unit.cargo.flat", _) => "Cargo",
        ("unit.paradrop_range.flat", _) => "ParadropRange",
        ("unit.naval_moves.flat", _) => "NavalMoves",
        ("unit.xp_gain.pct", _) => "ExperiencePercent",
        ("unit.embarked_moves.flat", _) => "EmbarkedMoves",
        ("heal.in_enemy_land.flat", _) => "HealEnemyLand",
        ("heal.in_neutral_land.flat", _) => "HealNeutralLand",
        ("heal.in_friendly_land.flat", _) => "HealFriendlyLand",
        ("heal.flat", "adjacent_friendly_units") => "HealAdjacent",
        // Faith Healers: own land near a following city (the condition).
        ("heal.flat", "all_units") => "HealFriendlyLand",
        ("heal.on_kill.flat", _) => "HealOnKill",
        ("aura.range", _) => "AuraRange",
        ("damage.on_attack.flat", _) => "AdjacentDamageOnAttack",
        ("promotion.capital_defense_falloff", _) => "CapitalDefenseFalloff",
        ("promotion.great_general_combat_modifier", _) => "StackedGeneralPercent",
        _ => return None,
    })
}

/// `promotion.<flag>` / `unit.<flag>` → UnitAbility.
fn unit_ability(modifier: &str) -> Option<&'static str> {
    Some(match modifier {
        "promotion.can_move_after_attacking" => "MoveAfterAttacking",
        "promotion.always_heal" => "AlwaysHeal",
        "promotion.allows_embarkation" => "Embarkation",
        "promotion.embarked_all_water" => "EmbarkedAllWater",
        "promotion.can_move_impassable" => "MoveImpassable",
        "promotion.ranged_support_fire" => "RangedSupportFire",
        "promotion.great_general" => "GreatGeneral",
        "promotion.great_admiral" => "GreatAdmiral",
        "promotion.heal_if_destroy_excludes_barbarians" => "HealOnKillNotBarbarians",
        "promotion.hills_double_move" => "HillsDoubleMove",
        "unit.double_move" => "DoubleMove",
        "unit.impassable_terrain" => "Impassable",
        "promotion.insta_heal" => "InstaHeal",
        "promotion.amphib" => "Amphibious",
        "promotion.range_attack_ignore_los" => "IndirectFire",
        "promotion.heal_outside_friendly" => "HealOutsideFriendly",
        "promotion.air_sweep_capable" => "AirSweep",
        "promotion.rival_territory" => "RivalTerritory",
        "promotion.rough_terrain_ends_turn" => "RoughTerrainEndsTurn",
        "promotion.ignore_terrain_cost" => "IgnoreTerrainCost",
        "promotion.flat_movement_cost" => "FlatMovementCost",
        "promotion.hovering_unit" => "Hovering",
        "promotion.no_capture" => "NoCapture",
        "promotion.only_defensive" => "OnlyDefensive",
        "promotion.no_defensive_bonus" => "NoDefensiveBonus",
        "promotion.must_set_up_to_ranged_attack" => "MustSetUp",
        "promotion.free_pillage_moves" => "FreePillageMoves",
        "promotion.city_attack_only" => "CityAttackOnly",
        "promotion.capture_defeated_enemy" => "CaptureDefeatedEnemy",
        "promotion.heavy_charge" => "HeavyCharge",
        "promotion.sapper" => "Sapper",
        "promotion.ignore_zoc" => "IgnoreZoneOfControl",
        "promotion.has_post_combat_promotions" => "PostCombatPromotions",
        // Submarines (the only invisibility type of the ruleset).
        "promotion.invisible" => "Invisible",
        "promotion.see_invisible" => "SeeInvisible",
        _ => return None,
    })
}

/// `city.<name>` → CityCombatStat and the amount scale (defense in
/// hundredths of strength).
fn city_stat(modifier: &str) -> Option<(&'static str, f64)> {
    Some(match modifier {
        "city.defense.flat" => ("Defense", 100.0),
        "city.max_hp.flat" => ("MaxHitPoints", 1.0),
        "city.defense.pct" => ("DefensePercent", 1.0),
        "city.ranged_strike.pct" => ("GarrisonRangedStrikePercent", 1.0),
        "city.nuke_damage.pct" => ("NukeDamagePercent", 1.0),
        "city.air_capacity.flat" => ("AirCapacity", 1.0),
        _ => return None,
    })
}

/// The modifier families this module owns.
fn is_military(modifier: &str, selector: &str) -> bool {
    let combat = modifier
        .strip_prefix("combat.")
        .and_then(|rest| rest.strip_suffix(".pct"))
        .is_some_and(|name| combat_context(name).is_some());
    let air_context = matches!(
        modifier,
        "air.interception_strength.pct" | "air.sweep_strength.pct"
    );
    combat
        || air_context
        || unit_stat(modifier, selector).is_some()
        || unit_ability(modifier).is_some()
        || city_stat(modifier).is_some()
        || matches!(
            modifier,
            "unit.free_xp"
                | "unit.free_xp_per_great_work"
                | "unit.free_maintenance_slots.flat"
                | "promotion.recon"
                | "grant.random_promotion_after_combat"
                | "unit.upgrade_cost.pct"
        )
        || (matches!(modifier, "grant.promotion" | "unit.moves.flat") && selector == "all_units")
}

/// Map a military catalog effect (see the module docs).
/// Nation traits that are unit movement and combat rules (classic/docs/classic/
/// rules.md), as unit effects on all the owner's units: the part of
/// each effect after its selector. None: not such a trait.
fn unit_trait(modifier: &str) -> Option<&'static [&'static str]> {
    Some(match modifier {
        // Embarks onto any water from the start, +1 sight embarked.
        "trait.embarked_all_water" => &[
            "modifier: GrantAbility(Embarkation)",
            "modifier: GrantAbility(EmbarkedAllWater)",
            "modifier: ModifyUnitStat(stat: EmbarkedSight, amount: 1)",
        ],
        // Landing costs one move point.
        "trait.embarked_to_land_flat_cost" => &["modifier: GrantAbility(EmbarkedToLandFlatCost)"],
        // Inca: hills cost one point.
        "trait.faster_in_hills" => &["modifier: GrantAbility(HillsIgnoreTerrainCost)"],
        // Iroquois: forests and jungles of the own land are roads.
        "trait.move_friendly_woods_as_road" => &[
            "condition: TileFeature(\"core:feature/forest\"), modifier: GrantAbility(RoadInFriendlyLand)",
            "condition: TileFeature(\"core:feature/jungle\"), modifier: GrantAbility(RoadInFriendlyLand)",
        ],
        // Carthage: mountains are open to the units once a general is earned.
        "trait.crosses_mountains_after_great_general" => {
            &["condition: GreatGeneralEarned, modifier: GrantAbility(CrossMountains)"]
        }
        // Japan: a wounded unit hits as hard as a whole one (the wounded
        // factor of `combat::damage` is `WOUNDED_DAMAGE_PERCENT` 33).
        "trait.fight_well_damaged" => &["modifier: ModifyUnitStat(stat: WoundedDamagePercent, amount: -33)"],
        _ => return None,
    })
}

/// Polynesia (the catalog's nearby-improvement combat bonus trait, with its
/// range and improvement rows as the parameters): +10 % combat strength
/// within two tiles of an own Moai. The catalog spreads one rule over three
/// rows; the first carries the percent, the other two
/// (`MOAI_BONUS_PARAMETERS`) are read here and covered.
const MOAI_IMPROVEMENT: &str = "core:improvement/moai";
const MOAI_RANGE: u8 = 2;

/// The catalog rows that are only parameters of the Moai bonus.
pub const MOAI_BONUS_PARAMETERS: [&str; 2] = ["nearby_improvement_bonus_range", "combat_bonus_improvement"];

pub fn military_effect(e: &Value, owner: Owner) -> Option<Result<String, String>> {
    let modifier = e["modifier"].as_str().unwrap_or("");
    let selector = e["selector"]["kind"].as_str().unwrap_or("");
    // Germany (Furor Teutonicus): the percent chance of the conversion.
    if owner == Owner::Nation && modifier == "trait.land_barbarian_conversion_percent" {
        return Some(amount(e).map(|percent| {
            format!("(scope: Player, selector: Owner, modifier: AddStat(stat: LandBarbarianConversionPercent, amount: {percent}))")
        }));
    }
    if owner == Owner::Nation && e["tag"].as_str() == Some("nearby_improvement_combat_bonus") {
        let percent = match amount(e) {
            Ok(p) => p,
            Err(missing) => return Some(Err(missing)),
        };
        return Some(Ok(format!(
            "(scope: Unit, selector: OwnerUnits, condition: NearOwnImprovement(improvement: \"{MOAI_IMPROVEMENT}\", range: {MOAI_RANGE}), modifier: ModifyCombat(context: Any, percent: {percent}))"
        )));
    }
    if let (Some(modifiers), Owner::Nation) = (unit_trait(modifier), owner) {
        let effects: Vec<String> = modifiers
            .iter()
            .map(|m| format!("(scope: Unit, selector: OwnerUnits, {m})"))
            .collect();
        // One effect per line of the written list.
        return Some(Ok(effects.join(",\n                ")));
    }
    // The Hakkapeliitta's general stacking: the promotion lends its rate.
    if e["needs_mechanic"].as_str() == Some("great_general_receives_movement") && owner == Owner::Promotion {
        return Some(Ok("(scope: Unit, selector: SourceUnit, modifier: GrantAbility(LendsMovementToGeneral))".into()));
    }
    // «Native Tongue» (Shoshone): the best of the rewards of Ancient Ruins.
    if e["needs_mechanic"].as_str() == Some("hardcoded_promotion_goody_hut_picker") && owner == Owner::Promotion {
        return Some(Ok("(scope: Unit, selector: SourceUnit, modifier: GrantAbility(ChoosesRuinsReward))".into()));
    }
    // «Bonus vs Helicopters» (a hard-coded promotion, no number in the catalog):
    // the strength of the other single-role counters (Bonus vs Tanks, 100).
    if e["needs_mechanic"].as_str() == Some("hardcoded_promotion_anti_helicopter") && owner == Owner::Promotion {
        return Some(Ok(
            "(scope: Unit, selector: SourceUnit, condition: OpponentRole(Helicopter), modifier: ModifyCombat(context: Any, percent: 100))".into(),
        ));
    }
    // «Exotic Cargo» (Nau): the promotion carries the cargo.
    if e["needs_mechanic"].as_str() == Some("hardcoded_promotion_sell_exotic_goods") && owner == Owner::Promotion {
        return Some(Ok("(scope: Unit, selector: SourceUnit, modifier: GrantAbility(ExoticCargo))".into()));
    }
    if !is_military(modifier, selector) || owner == Owner::Other {
        return None;
    }
    Some(map(e, owner, modifier, selector))
}

fn amount(e: &Value) -> Result<i64, String> {
    match &e["amount"] {
        Value::Number(n) => Ok(n.as_f64().unwrap_or(0.0).round() as i64),
        Value::Bool(true) => Ok(1),
        _ => Err(format!("amount:{}", e["modifier"].as_str().unwrap_or(""))),
    }
}

fn map(e: &Value, owner: Owner, modifier: &str, selector: &str) -> Result<String, String> {
    if modifier == "unit.upgrade_cost.pct" {
        return finish(
            "Player",
            "Owner",
            None,
            None,
            format!("ModifyCost(target: UnitUpgrade, percent: {})", amount(e)?),
            false,
        );
    }
    if let Some((stat, scale)) = city_stat(modifier) {
        let value = e["amount"]
            .as_f64()
            .ok_or_else(|| format!("amount:{modifier}"))?;
        let (selector, stat) = match selector {
            "this_city" if owner == Owner::Building => ("SourceCity", stat),
            "all_cities" => ("OwnerCities", stat),
            // A belief's ranged strike holds garrisoned or not (§4.3).
            "following_cities" if owner == Owner::Belief && stat == "GarrisonRangedStrikePercent" => {
                ("FollowingCities", "RangedStrikePercent")
            }
            other => return Err(format!("selector:{other}")),
        };
        let text = format!(
            "ModifyCityCombat(stat: {stat}, amount: {})",
            (value * scale).round() as i64
        );
        return finish("City", selector, condition(e)?, None, text, false);
    }
    if modifier == "unit.free_maintenance_slots.flat" {
        // Volunteer Army: units whose maintenance the player does not pay.
        if owner != Owner::Policy || selector != "player" {
            return Err(format!("selector:{selector}"));
        }
        let text = format!("AddStat(stat: FreeMaintenanceUnits, amount: {})", amount(e)?);
        return finish("Player", "Owner", None, None, text, false);
    }
    if matches!(modifier, "unit.free_xp" | "unit.free_xp_per_great_work") {
        if e["trigger"].as_str() != Some("on_unit_trained") {
            return Err(format!("trigger:{}", e["trigger"].as_str().unwrap_or("")));
        }
        let filter = unit_filter(&e["selector"]["filter"])?;
        let variant = if modifier == "unit.free_xp" { "GrantExperience" } else { "GrantExperiencePerGreatWork" };
        let text = format!("{variant}({})", amount(e)?);
        return finish(
            "Unit",
            "EventUnit",
            filter,
            Some("OnUnitTrained"),
            text,
            true,
        );
    }
    if e["tag"].as_str() == Some("attack_bonus_turns") {
        return attack_bonus_turns(e, owner);
    }
    check_passive_trigger(e, owner, modifier)?;
    let target = unit_target(owner, selector, e)?;
    let text = unit_modifier(e, modifier, selector)?;
    let cond = match (condition(e)?, kind_filter(e, selector)?) {
        (None, kind) => kind,
        (cond, None) => cond,
        (Some(_), Some(_)) => return Err(format!("selector:{selector}")),
    };
    if text.starts_with("GrantAbility(DoubleMove)") || text.starts_with("GrantAbility(Impassable)")
    {
        if cond.is_none() {
            return Err("condition:on_tile".into());
        }
    }
    finish("Unit", target, cond, None, text, false)
}

/// Sight of an aircraft (the unit's sight column) and the radius Air Recon
/// gives it ("6 tiles in every direction always visible").
const AIR_UNIT_SIGHT: i64 = 2;
const AIR_RECON_SIGHT: i64 = 6;

/// The attack percent of Clausewitz's Legacy while its bonus turns last
/// (rules.md). Own value, not compared with a public source
/// (unverified).
const ADOPT_ATTACK_BONUS_PERCENT: i64 = 25;

/// Clausewitz's Legacy (attack bonus turns): the catalog amount is the
/// number of turns, the percent is the constant above: +25 % attack of
/// all the owner's units for 50 turns after adoption. Unverified whether it
/// applies to ranged attacks too; here every attack (`Attack`) gets it.
fn attack_bonus_turns(e: &Value, owner: Owner) -> Result<String, String> {
    let trigger = e["trigger"].as_str().unwrap_or("");
    if owner != Owner::Policy || trigger != "on_adopt" {
        return Err(format!("trigger:{trigger}"));
    }
    let turns = amount(e)?;
    Ok(format!(
        "(scope: Unit, selector: OwnerUnits, trigger: OnAdopt, modifier: ModifyCombat(context: Attack, percent: {ADOPT_ATTACK_BONUS_PERCENT}), duration: Turns({turns}), stacking: Unique)"
    ))
}

/// The catalog triggers a passive unit property absorbs: the property is
/// read when its event happens (heal on kill, plunder on a city attack,
/// damage on attack, the post-combat promotion), and a technology's effect
/// holds while the technology is known. Any other trigger (adoption bonus
/// for a few turns, ...) is a timed mechanic: not typed here.
fn check_passive_trigger(e: &Value, owner: Owner, modifier: &str) -> Result<(), String> {
    let trigger = e["trigger"].as_str().unwrap_or("passive");
    let ok = match trigger {
        "passive" => true,
        "on_kill" => matches!(modifier, "heal.on_kill.flat" | "golden_age_points.on_kill.pct_of_strength"),
        "on_attack_city" => modifier == "combat.city_attack_gold.pct_of_damage",
        "on_attack" => matches!(modifier, "damage.on_attack.flat" | "damage.on_attack.chance_pct"),
        "after_first_combat" => modifier == "grant.random_promotion_after_combat",
        "on_research" => owner == Owner::Technology,
        _ => false,
    };
    if ok {
        Ok(())
    } else {
        Err(format!("trigger:{trigger}"))
    }
}

/// Unit-scope selector of the owner kind.
fn unit_target(owner: Owner, selector: &str, e: &Value) -> Result<&'static str, String> {
    let promotion_selectors = [
        "this_unit",
        "enemy_units_in_range",
        "adjacent_friendly_units",
        "adjacent_enemy_units",
    ];
    if owner == Owner::Promotion && promotion_selectors.contains(&selector) {
        return Ok("SourceUnit");
    }
    // A policy's unit maintenance percent (Nationalism) reaches each unit.
    if owner == Owner::Policy && selector == "player" && e["modifier"].as_str() == Some("unit.maintenance.pct") {
        return Ok("OwnerUnits");
    }
    if owner != Owner::Promotion && selector == "all_units" {
        let filter = &e["selector"]["filter"];
        let known = filter
            .as_object()
            .is_none_or(|m| m.keys().all(|k| matches!(k.as_str(), "promotion" | "unit_combat" | "domain")));
        if known {
            return Ok("OwnerUnits");
        }
    }
    Err(format!("selector:{selector}"))
}

fn unit_modifier(e: &Value, modifier: &str, selector: &str) -> Result<String, String> {
    if let Some(stat) = unit_stat(modifier, selector) {
        return Ok(format!(
            "ModifyUnitStat(stat: {stat}, amount: {})",
            amount(e)?
        ));
    }
    if let Some(ability) = unit_ability(modifier) {
        return Ok(format!("GrantAbility({ability})"));
    }
    match modifier {
        // Air Recon: the aircraft sees six tiles (an air unit's own sight is
        // two), so the promotion adds the other four.
        "promotion.recon" => return Ok(format!("ModifyUnitStat(stat: Sight, amount: {})", AIR_RECON_SIGHT - AIR_UNIT_SIGHT)),
        "unit.moves.flat" => return Ok(format!("ModifyMovement(amount: {})", amount(e)?)),
        "air.interception_strength.pct" => {
            return Ok(format!(
                "ModifyCombat(context: Interception, percent: {})",
                amount(e)?
            ));
        }
        "air.sweep_strength.pct" => {
            return Ok(format!(
                "ModifyCombat(context: AirSweep, percent: {})",
                amount(e)?
            ));
        }
        "grant.random_promotion_after_combat" | "grant.promotion" => {
            let target = e["target"]
                .as_str()
                .or_else(|| e["selector"]["filter"]["promotion"].as_str())
                .ok_or_else(|| format!("modifier:{modifier}"))?;
            let variant = if modifier == "grant.promotion" {
                "PassivePromotion"
            } else {
                "PostCombatPromotion"
            };
            return Ok(format!("{variant}({})", quote(target)));
        }
        _ => {}
    }
    let context = modifier
        .strip_prefix("combat.")
        .and_then(|rest| rest.strip_suffix(".pct"))
        .and_then(combat_context)
        .ok_or_else(|| format!("modifier:{modifier}"))?;
    Ok(format!(
        "ModifyCombat(context: {context}, percent: {})",
        amount(e)?
    ))
}

/// The catalog's condition of a military effect.
fn condition(e: &Value) -> Result<Option<String>, String> {
    let Some(map) = e["condition"].as_object() else {
        return Ok(None);
    };
    let until = map.get("until_tech").and_then(|t| t.as_str());
    if let Some(on_tile) = map.get("on_tile") {
        let feature = on_tile["feature"].as_str();
        let terrain = on_tile["terrain"].as_str();
        return Ok(Some(match (feature, terrain, until) {
            (Some(f), None, None) => format!("TileFeature({})", quote(f)),
            (None, Some(t), None) => format!("TileTerrain({})", quote(t)),
            (None, Some(t), Some(tech)) => {
                format!("TileTerrainUntil({}, {})", quote(t), quote(tech))
            }
            _ => return Err("condition:on_tile".into()),
        }));
    }
    if let Some(opponent) = map.get("opponent") {
        match opponent.as_str() {
            Some("barbarian") => return Ok(Some("OpponentIsBarbarian".into())),
            Some("city_state") => return Ok(Some("OpponentIsCityState".into())),
            Some("nation_with_more_cities") => return Ok(Some("OpponentHasMoreCities".into())),
            _ => {}
        }
        if let Some(role) = opponent["unit_combat"].as_str() {
            return Ok(Some(format!("OpponentRole({})", role_variant(role)?)));
        }
        if let Some(domain) = opponent["domain"].as_str() {
            return Ok(Some(format!("OpponentDomain({})", domain_variant(domain)?)));
        }
        if let Some(unit) = opponent["unit"].as_str() {
            return Ok(Some(format!("OpponentClassOf({})", quote(unit))));
        }
        return Err("condition:opponent".into());
    }
    // Beliefs: near a city following the owner's creed.
    let near_city = [
        ("near_friendly_city_following_religion", "Own"),
        ("adjacent_to_friendly_city", "Own"),
        ("near_enemy_city_following_religion", "Enemy"),
    ];
    for (key, relation) in near_city {
        if map.len() == 1 && map.get(key).and_then(|v| v.as_bool()) == Some(true) {
            return Ok(Some(format!("NearFollowingCity({relation})")));
        }
    }
    if map.len() == 1 {
        if map.get("golden_age") == Some(&Value::Bool(true)) {
            return Ok(Some("GoldenAge".into()));
        }
        if map.get("city_garrisoned") == Some(&Value::Bool(true)) {
            return Ok(Some("CityGarrisoned".into()));
        }
        if let Some(t) = map.get("tech_known").and_then(|t| t.as_str()) {

            return Ok(Some(format!("HasTechnology({})", quote(t))));
        }
        if let Some(t) = until {
            return Ok(Some(format!("LacksTechnology({})", quote(t))));
        }
    }
    let keys: Vec<&str> = map.keys().map(|k| k.as_str()).collect();
    Err(format!("condition:{}", keys.join("+")))
}

/// The kind filter of an all-units selector (its combat role or domain;
/// the promotion it names is the grant's target) → kind-level condition.
fn kind_filter(e: &Value, selector: &str) -> Result<Option<String>, String> {
    if selector != "all_units" {
        return Ok(None);
    }
    let Some(map) = e["selector"]["filter"].as_object() else {
        return Ok(None);
    };
    let mut kinds = map.clone();
    kinds.remove("promotion");
    unit_filter(&Value::Object(kinds))
}

/// A units-trained-here filter → kind-level condition.
fn unit_filter(filter: &Value) -> Result<Option<String>, String> {
    let Some(map) = filter.as_object() else {
        return Ok(None);
    };
    if map.is_empty() {
        return Ok(None);
    }
    if let Some(domain) = map.get("domain").and_then(|d| d.as_str()) {
        return Ok(Some(format!("UnitDomain({})", domain_variant(domain)?)));
    }
    if let Some(role) = map.get("unit_combat").and_then(|r| r.as_str()) {
        return Ok(Some(format!("UnitRole({})", role_variant(role)?)));
    }
    Err("selector:units_trained_here".into())
}

pub(crate) fn domain_variant(key: &str) -> Result<&'static str, String> {
    Ok(match key {
        "core:domain/land" => "Land",
        "core:domain/sea" => "Sea",
        "core:domain/air" => "Air",
        other => return Err(format!("condition:domain={other}")),
    })
}

pub(crate) fn role_variant(key: &str) -> Result<&'static str, String> {
    Ok(match key.strip_prefix("core:unit-combat/").unwrap_or("") {
        "recon" => "Recon",
        "melee" => "Melee",
        "archer" => "Archer",
        "mounted" => "Mounted",
        "siege" => "Siege",
        "gun" => "Gun",
        "armor" => "Armor",
        "navalmelee" => "NavalMelee",
        "navalranged" => "NavalRanged",
        "submarine" => "Submarine",
        "carrier" => "Carrier",
        "fighter" => "Fighter",
        "bomber" => "Bomber",
        "helicopter" => "Helicopter",
        "missile" => "Missile",
        other => return Err(format!("condition:unit_combat={other}")),
    })
}

/// The RON effect (instant ones are triggered and Once).
fn finish(
    scope: &str,
    selector: &str,
    condition: Option<String>,
    trigger: Option<&str>,
    modifier: String,
    instant: bool,
) -> Result<String, String> {
    let mut parts = vec![format!("scope: {scope}"), format!("selector: {selector}")];
    if let Some(c) = condition {
        parts.push(format!("condition: {c}"));
    }
    if let Some(t) = trigger {
        parts.push(format!("trigger: {t}"));
    }
    parts.push(format!("modifier: {modifier}"));
    if instant {
        parts.push("duration: Once".into());
    }
    Ok(format!("({})", parts.join(", ")))
}
