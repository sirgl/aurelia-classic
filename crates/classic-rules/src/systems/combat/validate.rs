//! Validation of the combat commands (pure: no RNG, no mutation). The
//! dispatcher `command::validate` calls these.

use crate::classic::effects::ClassicUnitAbility;
use crate::systems::combat::air::AirOrderBlock;
use crate::combat::exchange::{plan_city_strike, AttackKind, PlanError};
use crate::combat::plan_attack;
use crate::command::{CityDecision, CommandError, ResolvedCommand};
use crate::effects::unit::unit_modifiers;
use crate::ids::{CityHandle, PlayerId, PromotionId, TileId, UnitHandle};
use crate::rules::content::Domain;
use crate::rules::effects::UnitAbility;
use crate::rules::CompiledRuleset;
use crate::world::city::CityStatus;
use crate::world::state::GameView;
use crate::world::unit::Stance;
use crate::classic::content::BARBARIAN_CAMP;

fn violation(reason: &str) -> CommandError {
    CommandError::RuleViolation(reason.into())
}

fn plan_error(e: PlanError) -> CommandError {
    match e {
        PlanError::UnknownUnit => CommandError::UnknownUnit,
        other => violation(other.reason()),
    }
}

/// How a unit attacks when ordered to attack a tile: ranged units shoot
/// (a ranged unit never melees), the others charge.
pub(crate) fn attack_kind(
    view: GameView<'_>,
    rules: &CompiledRuleset,
    unit: UnitHandle,
) -> Option<AttackKind> {
    let core = view.auth.units().get(unit)?.core;
    let def = rules.units.get(core.kind.0 as usize);
    let ranged = def.is_some_and(|d| d.ranged_strength > 0 && d.range > 0);
    if ranged || def.is_some_and(|d| d.domain == Domain::Air) {
        return Some(AttackKind::Ranged);
    }
    Some(AttackKind::Melee)
}

/// AttackUnit / AttackCity: attack the target's tile the unit's way.
pub(crate) fn validate_attack_tile(
    view: GameView<'_>,
    rules: &CompiledRuleset,
    unit: UnitHandle,
    target: TileId,
) -> Result<ResolvedCommand, CommandError> {
    let kind = attack_kind(view, rules, unit).ok_or(CommandError::UnknownUnit)?;
    let plan = plan_attack(view, rules, unit, target, kind).map_err(plan_error)?;
    Ok(ResolvedCommand::Attack(plan))
}

pub(crate) fn validate_ranged(
    view: GameView<'_>,
    rules: &CompiledRuleset,
    unit: UnitHandle,
    target: TileId,
) -> Result<ResolvedCommand, CommandError> {
    let plan = plan_attack(view, rules, unit, target, AttackKind::Ranged).map_err(plan_error)?;
    Ok(ResolvedCommand::Attack(plan))
}

pub(crate) fn validate_city_strike(
    view: GameView<'_>,
    rules: &CompiledRuleset,
    city: CityHandle,
    target: TileId,
) -> Result<ResolvedCommand, CommandError> {
    let core = view
        .auth
        .cities()
        .get(city)
        .ok_or(CommandError::UnknownCity)?;
    if core.status == CityStatus::AwaitingDecision || core.resistance_turns > 0 {
        return Err(violation("city in resistance"));
    }
    let plan = plan_city_strike(view, rules, city, target).map_err(plan_error)?;
    Ok(ResolvedCommand::CityStrike(plan))
}

pub(crate) fn validate_set_up(
    view: GameView<'_>,
    rules: &CompiledRuleset,
    unit: UnitHandle,
) -> Result<ResolvedCommand, CommandError> {
    let core = *view
        .auth
        .units()
        .get(unit)
        .ok_or(CommandError::UnknownUnit)?
        .core;
    let mods = unit_modifiers(view.auth, rules, &core);
    if !mods.has(UnitAbility::MustSetUp) {
        return Err(violation("needs no set up"));
    }
    if core.stance == Stance::SetUp {
        return Err(violation("already set up"));
    }
    if core.moves_left == 0 {
        return Err(violation("no moves left"));
    }
    if crate::navigation::terrain::is_water(view.auth.map(), core.tile) {
        return Err(violation("embarked"));
    }
    Ok(ResolvedCommand::SetUpRangedAttack { unit })
}

pub(crate) fn validate_promote(
    view: GameView<'_>,
    rules: &CompiledRuleset,
    unit: UnitHandle,
    promotion: PromotionId,
) -> Result<ResolvedCommand, CommandError> {
    let core = *view
        .auth
        .units()
        .get(unit)
        .ok_or(CommandError::UnknownUnit)?
        .core;
    let needed = crate::combat::experience::experience_needed_by(view.auth, rules, &core);
    if core.experience < needed {
        return Err(violation("not enough experience"));
    }
    let available = crate::combat::experience::available_promotions(view.auth, rules, &core);
    if !available.contains(&promotion) {
        return Err(violation("promotion not available"));
    }
    Ok(ResolvedCommand::Promote { unit, promotion })
}

pub(crate) fn validate_upgrade(
    view: GameView<'_>,
    rules: &CompiledRuleset,
    unit: UnitHandle,
) -> Result<ResolvedCommand, CommandError> {
    let core = *view
        .auth
        .units()
        .get(unit)
        .ok_or(CommandError::UnknownUnit)?
        .core;
    let offer = crate::systems::combat::upgrade::upgrade_offer(view.auth, rules, &core)
        .map_err(violation)?;
    if view.auth.players().treasury_of(core.owner) < offer.price {
        return Err(violation("not enough gold"));
    }
    Ok(ResolvedCommand::UpgradeUnit {
        unit,
        to: offer.to,
        price: offer.price,
    })
}

pub(crate) fn validate_decide_city(
    view: GameView<'_>,
    rules: &crate::rules::CompiledRuleset,
    player: PlayerId,
    city: CityHandle,
    decision: CityDecision,
) -> Result<ResolvedCommand, CommandError> {
    let core = view
        .auth
        .cities()
        .get(city)
        .ok_or(CommandError::UnknownCity)?;
    if core.owner != player {
        return Err(violation("not your city"));
    }
    if core.status != CityStatus::AwaitingDecision {
        return Err(violation("no decision pending"));
    }
    match decision {
        CityDecision::Raze if core.original_capital => {
            return Err(violation("an original capital cannot be razed"));
        }
        CityDecision::Raze if crate::derived::grants::holds_unique_luxury(view.auth, rules, core) => {
            return Err(violation("a city with a unique luxury cannot be razed"));
        }
        CityDecision::Annex
            if crate::effects::player::forbidden(
                view.auth,
                rules,
                player,
                crate::rules::effects::Forbidden::Annexing,
            ) =>
        {
            return Err(violation("this nation cannot annex cities"));
        }
        CityDecision::Liberate => {
            let original = core.original_owner;
            if original == player || !view.auth.players().is_alive(original) {
                return Err(violation("nobody to liberate"));
            }
        }
        CityDecision::Annex | CityDecision::Puppet | CityDecision::Raze => {}
    }
    Ok(ResolvedCommand::DecideCity {
        player,
        city,
        decision,
    })
}

/// Pillage (§7.8): a unit able to pillage (a land unit on land, a ship on
/// water), not in a city, on a tile with an improvement or road that is
/// not its own, at war with the tile's owner (or the tile unowned), moves
/// left.
pub(crate) fn validate_pillage(
    view: GameView<'_>,
    rules: &CompiledRuleset,
    unit: UnitHandle,
) -> Result<ResolvedCommand, CommandError> {
    let auth = view.auth;
    let core = *auth
        .units()
        .get(unit)
        .ok_or(CommandError::UnknownUnit)?
        .core;
    let def = rules.units.get(core.kind.0 as usize);
    if !def.is_some_and(|d| d.combat.pillage) {
        return Err(violation("cannot pillage"));
    }
    if core.moves_left == 0 {
        return Err(violation("no moves left"));
    }
    let tile = core.tile;
    let map = auth.map();
    if auth.indexes().city_on_tile(tile).is_some() {
        return Err(violation("nothing to pillage"));
    }
    // Ships pillage water improvements (fishing boats, offshore platforms),
    // except barbarian ships; an embarked land unit pillages nothing.
    let water = crate::navigation::terrain::is_water(map, tile);
    let sea = def.is_some_and(|d| d.domain == Domain::Sea);
    if water && !sea {
        return Err(violation("embarked"));
    }
    if sea && crate::world::player::ClassicPlayers::is_barbarian(auth.players(), core.owner) {
        return Err(violation("cannot pillage"));
    }
    let improvement = map.working_improvement(tile);
    if improvement.is_none() && !map.road(tile) {
        return Err(violation("nothing to pillage"));
    }
    if let Some(imp) = improvement {
        let permanent = rules.improvements.get(imp.0 as usize).is_some_and(|d| {
            matches!(
                d.kind,
                crate::rules::content::ImprovementKind::AncientRuins
                    | BARBARIAN_CAMP
            )
        });
        if permanent {
            return Err(violation("nothing to pillage"));
        }
    }
    match map.owner(tile).flatten() {
        Some(host) if host == core.owner => return Err(violation("own tile")),
        Some(host) if !auth.diplomacy().at_war(core.owner, host) => {
            return Err(violation("not at war"))
        }
        _ => {}
    }
    Ok(ResolvedCommand::Pillage { unit, tile })
}

/// Gold of a cargo sold, from 100 next to the capital to 400 at the far end
/// of the map. Own values, not compared with a public source (unverified).
pub const EXOTIC_GOODS_GOLD_MIN: u32 = 100;
pub const EXOTIC_GOODS_GOLD_MAX: u32 = 400;
/// Experience of the sale on the same scale (unverified).
pub const EXOTIC_GOODS_XP_MIN: u32 = 5;
pub const EXOTIC_GOODS_XP_MAX: u32 = 20;

/// Sell the exotic cargo (Portugal's Nau; the promotion «Exotic Cargo»
/// grants `ExoticCargo`): the unit has moves, has its cargo, and stands on
/// or next to land of another nation or city-state that is alive and
/// not at war with its owner. The gold and experience grow with the
/// distance to the owner's capital, up to half the map's width plus
/// height. Rules: classic/docs/classic/rules.md has no number for it, the scale is a
/// guess of the observable behaviour (more distance, more gold), unverified.
pub(crate) fn validate_exotic_goods(
    view: GameView<'_>,
    rules: &CompiledRuleset,
    unit: UnitHandle,
) -> Result<ResolvedCommand, CommandError> {
    let auth = view.auth;
    let core = *auth.units().get(unit).ok_or(CommandError::UnknownUnit)?.core;
    let mods = unit_modifiers(auth, rules, &core);
    if !mods.has(UnitAbility::Module(ClassicUnitAbility::ExoticCargo)) {
        return Err(violation("no cargo to sell"));
    }
    let cargo = core
        .promotions
        .iter()
        .find(|p| {
            rules.promotions.get(p.0 as usize).is_some_and(|def| {
                def.effects.passive_unit.iter().any(|e| {
                    e.modifier == crate::rules::effects::Modifier::GrantAbility(UnitAbility::Module(ClassicUnitAbility::ExoticCargo))
                })
            })
        })
        .ok_or_else(|| violation("no cargo to sell"))?;
    if core.moves_left == 0 {
        return Err(violation("no moves left"));
    }
    let map = auth.map();
    let foreign = |tile: TileId| {
        map.owner(tile).flatten().filter(|host| {
            *host != core.owner
                && auth.players().is_alive(*host)
                && !crate::world::player::ClassicPlayers::is_barbarian(auth.players(), *host)
                && !auth.diplomacy().at_war(core.owner, *host)
        })
    };
    if map.owner(core.tile).flatten() == Some(core.owner) {
        return Err(violation("in your own land"));
    }
    let near = foreign(core.tile).is_some() || map.dimensions.neighbors(core.tile).into_iter().any(|t| foreign(t).is_some());
    if !near {
        return Err(violation("needs a foreign land next to it"));
    }
    let capital = crate::combat::city::capital_of(auth, rules, core.owner)
        .and_then(|c| auth.cities().get(c))
        .ok_or_else(|| violation("needs a capital"))?;
    let dims = map.dimensions;
    let distance = dims.distance(core.tile, capital.tile).unwrap_or(0).max(0) as u32;
    let threshold = ((dims.width + dims.height) / 2).max(1);
    let near_share = distance.min(threshold);
    let gold = EXOTIC_GOODS_GOLD_MIN + (EXOTIC_GOODS_GOLD_MAX - EXOTIC_GOODS_GOLD_MIN) * near_share / threshold;
    let experience = EXOTIC_GOODS_XP_MIN + (EXOTIC_GOODS_XP_MAX - EXOTIC_GOODS_XP_MIN) * near_share / threshold;
    Ok(ResolvedCommand::SellExoticGoods { unit, cargo, gold, experience: experience as u16 })
}

/// Rebase (§9): an aircraft with moves to an own city within twice its
/// range that has room.
pub(crate) fn validate_rebase(
    view: GameView<'_>,
    rules: &CompiledRuleset,
    unit: UnitHandle,
    city: CityHandle,
) -> Result<ResolvedCommand, CommandError> {
    let auth = view.auth;
    let core = *auth
        .units()
        .get(unit)
        .ok_or(CommandError::UnknownUnit)?
        .core;
    let def = rules
        .units
        .get(core.kind.0 as usize)
        .ok_or(violation("not an aircraft"))?;
    if def.domain != Domain::Air {
        return Err(violation("not an aircraft"));
    }
    if core.moves_left == 0 {
        return Err(violation("no moves left"));
    }
    let target = auth.cities().get(city).ok_or(CommandError::UnknownCity)?;
    if target.owner != core.owner || target.tile == core.tile {
        return Err(violation("not your city"));
    }
    let range =
        i32::from(def.range) * crate::systems::combat::air::REBASE_RANGE_PERCENT / 100;
    let distance = auth
        .map()
        .dimensions
        .distance(core.tile, target.tile)
        .unwrap_or(i32::MAX);
    if distance > range {
        return Err(violation("out of range"));
    }
    // The unit's air slots must fit the city's.
    let room = crate::systems::combat::air::city_air_capacity(auth, rules, city)
        >= crate::systems::combat::air::aircraft_in_city(auth, rules, city)
            + crate::systems::combat::air::air_slots(def);
    if !room {
        return Err(violation("no room for aircraft"));
    }
    Ok(ResolvedCommand::Rebase {
        unit,
        to: target.tile,
        carrier: None,
    })
}

/// Rebase onto a carrier (§8–9): an aircraft or missile with moves, an own
/// carrier of its cargo kind within twice its range, with room.
pub(crate) fn validate_rebase_to_carrier(
    view: GameView<'_>,
    rules: &CompiledRuleset,
    unit: UnitHandle,
    carrier: UnitHandle,
) -> Result<ResolvedCommand, CommandError> {
    use crate::systems::cargo::{cargo_of, cargo_space, carrier_of};
    let auth = view.auth;
    let core = *auth.units().get(unit).ok_or(CommandError::UnknownUnit)?.core;
    let host = *auth.units().get(carrier).ok_or(CommandError::UnknownUnit)?.core;
    let def = rules.units.get(core.kind.0 as usize).ok_or(violation("not an aircraft"))?;
    let host_def = rules.units.get(host.kind.0 as usize).ok_or(violation("not a carrier"))?;
    let fits = def.combat.carried_as.is_some() && def.combat.carried_as == host_def.combat.carries;
    if !fits {
        return Err(violation("cannot be carried there"));
    }
    if host.owner != core.owner || carrier_of(auth, carrier).is_some() {
        return Err(violation("not your carrier"));
    }
    if core.moves_left == 0 {
        return Err(violation("no moves left"));
    }
    if carrier_of(auth, unit) == Some(host.id) {
        return Err(violation("already on it"));
    }
    let range = i32::from(def.range) * crate::systems::combat::air::REBASE_RANGE_PERCENT / 100;
    let distance = auth.map().dimensions.distance(core.tile, host.tile).unwrap_or(i32::MAX);
    if distance > range {
        return Err(violation("out of range"));
    }
    if cargo_of(auth, carrier).len() as i64 >= cargo_space(auth, rules, carrier) {
        return Err(violation("no room for aircraft"));
    }
    Ok(ResolvedCommand::Rebase {
        unit,
        to: host.tile,
        carrier: Some(host.id),
    })
}

pub(crate) fn validate_air_sweep(
    view: GameView<'_>,
    rules: &CompiledRuleset,
    unit: UnitHandle,
    target: TileId,
) -> Result<ResolvedCommand, CommandError> {
    let auth = view.auth;
    let core = *auth
        .units()
        .get(unit)
        .ok_or(CommandError::UnknownUnit)?
        .core;
    if !crate::systems::combat::air::can_air_sweep(auth, rules, unit) {
        return Err(AirOrderBlock::CannotSweep.into());
    }
    if core.moves_left == 0 || core.attacks_made > 0 {
        return Err(AirOrderBlock::NoAttacksLeft.into());
    }
    let range = rules
        .units
        .get(core.kind.0 as usize)
        .map(|d| i32::from(d.range))
        .unwrap_or(0);
    if auth
        .map()
        .dimensions
        .distance(core.tile, target)
        .unwrap_or(i32::MAX)
        > range
    {
        return Err(AirOrderBlock::OutOfRange.into());
    }
    Ok(ResolvedCommand::AirSweep { unit, target })
}

pub(crate) fn validate_intercept(
    view: GameView<'_>,
    rules: &CompiledRuleset,
    unit: UnitHandle,
) -> Result<ResolvedCommand, CommandError> {
    let core = *view
        .auth
        .units()
        .get(unit)
        .ok_or(CommandError::UnknownUnit)?
        .core;
    let range = rules
        .units
        .get(core.kind.0 as usize)
        .map(|d| d.combat.air_intercept_range)
        .unwrap_or(0);
    if range == 0 {
        return Err(AirOrderBlock::CannotIntercept.into());
    }
    if core.stance == Stance::Intercept {
        return Err(AirOrderBlock::AlreadyIntercepting.into());
    }
    Ok(ResolvedCommand::Intercept { unit })
}

/// Nuke (§2.8): a nuclear weapon with moves, a known target within its range,
/// every other player with units, cities or land in the blast at war.
pub(crate) fn validate_nuke(
    view: GameView<'_>,
    rules: &CompiledRuleset,
    unit: UnitHandle,
    target: TileId,
) -> Result<ResolvedCommand, CommandError> {
    let auth = view.auth;
    let core = *auth
        .units()
        .get(unit)
        .ok_or(CommandError::UnknownUnit)?
        .core;
    let def = rules
        .units
        .get(core.kind.0 as usize)
        .ok_or(violation("not a nuclear weapon"))?;
    if def.combat.nuke_level == 0 {
        return Err(violation("not a nuclear weapon"));
    }
    if core.moves_left == 0 {
        return Err(violation("no moves left"));
    }
    let dims = auth.map().dimensions;
    if dims.distance(core.tile, target).unwrap_or(i32::MAX) > i32::from(def.range) {
        return Err(violation("out of range"));
    }
    // The player aims at ground it knows: a refusal because of what stands
    // on a tile it has never seen would tell it what is there.
    let known = crate::knowledge::view::is_visible(view.derived, core.owner, target)
        || auth.knowledge().get(core.owner.0 as usize).is_some_and(|k| k.is_explored(target));
    if !known {
        return Err(violation("target tile unknown"));
    }
    let radius = crate::systems::combat::actions::NUKE_BLAST_RADIUS;
    for tile in crate::math::tiles_in_hex_radius(&dims, target, radius) {
        let mut victims: Vec<PlayerId> = auth
            .indexes()
            .units_on(tile)
            .iter()
            .filter_map(|u| auth.units().get(*u).map(|v| v.core.owner))
            .collect();
        victims.extend(auth.map().owner(tile).flatten());
        for victim in victims {
            if victim != core.owner && !auth.diplomacy().at_war(core.owner, victim) {
                return Err(violation("would hit a player not at war"));
            }
        }
    }
    Ok(ResolvedCommand::Nuke { unit, target })
}

/// Citadel: a great general on a land tile of its owner or next to it, not
/// on a city.
pub(crate) fn validate_citadel(
    view: GameView<'_>,
    rules: &CompiledRuleset,
    unit: UnitHandle,
) -> Result<ResolvedCommand, CommandError> {
    let auth = view.auth;
    let core = *auth
        .units()
        .get(unit)
        .ok_or(CommandError::UnknownUnit)?
        .core;
    if !crate::effects::unit::own_ability(rules, &core, UnitAbility::Module(ClassicUnitAbility::GreatGeneral)) {
        return Err(violation("not a great general"));
    }
    if crate::systems::combat::actions::citadel_of(rules).is_none() {
        return Err(violation("no citadel in the rules"));
    }
    let tile = core.tile;
    let map = auth.map();
    if crate::navigation::cost::enter_cost_on_map(map, tile).is_none()
        || auth.indexes().city_on_tile(tile).is_some()
    {
        return Err(violation("cannot build here"));
    }
    let own = map.owner(tile).flatten() == Some(core.owner);
    let next_to_own = map
        .dimensions
        .neighbors(tile)
        .into_iter()
        .any(|t| map.owner(t).flatten() == Some(core.owner));
    let foreign = map.owner(tile).flatten().is_some_and(|o| o != core.owner);
    if foreign || !(own || next_to_own) {
        return Err(violation("in or next to own territory"));
    }
    if auth.indexes().cities_of(core.owner).is_empty() {
        return Err(violation("no city"));
    }
    Ok(ResolvedCommand::BuildCitadel { unit, tile })
}
