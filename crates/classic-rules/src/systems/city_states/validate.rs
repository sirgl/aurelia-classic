//! Validation of the city-state commands (pure).

use super::block::CityStateBlock;
use super::gifts::{bully_score, giftable_unit, pledge_problem, IMPROVEMENT_GIFT_COST};
use crate::command::{CommandError, ResolvedCommand};
use crate::ids::{PlayerId, TileId, UnitHandle};
use crate::rules::content::ResourceClass;
use crate::rules::CompiledRuleset;
use crate::world::state::{AuthWorld, GameView};
use crate::world::player::ClassicPlayers;

/// A nation and a city-state it met, both alive, at peace.
fn nation_and_city_state(
    auth: &AuthWorld,
    major: PlayerId,
    city_state: PlayerId,
) -> Result<(), CommandError> {
    let players = auth.players();
    if !players.is_major(major) || !players.is_alive(major) {
        return Err(CityStateBlock::NotANation.into());
    }
    if !players.is_city_state(city_state) || !players.is_alive(city_state) {
        return Err(CityStateBlock::NotACityState.into());
    }
    if !crate::systems::diplomacy::has_met(auth, major, city_state) {
        return Err(CityStateBlock::NotMet.into());
    }
    if auth.diplomacy().at_war(major, city_state) {
        return Err(CityStateBlock::AtWar.into());
    }
    Ok(())
}

pub(crate) fn validate_gift_gold(
    view: GameView<'_>,
    player: PlayerId,
    city_state: PlayerId,
    gold: u32,
) -> Result<ResolvedCommand, CommandError> {
    nation_and_city_state(view.auth, player, city_state)?;
    if gold == 0 || view.auth.players().treasury_of(player) < gold {
        return Err(CityStateBlock::NotEnoughGold.into());
    }
    Ok(ResolvedCommand::GiftGold {
        player,
        city_state,
        gold,
    })
}

/// A combat unit (not a scout) standing on the city-state's land, or a
/// great person of a nation whose great person gifts bring
/// influence (Sweden, §4.11).
pub(crate) fn validate_gift_unit(
    view: GameView<'_>,
    rules: &CompiledRuleset,
    unit: UnitHandle,
    city_state: PlayerId,
) -> Result<ResolvedCommand, CommandError> {
    let core = *view
        .auth
        .units()
        .get(unit)
        .ok_or(CommandError::UnknownUnit)?
        .core;
    nation_and_city_state(view.auth, core.owner, city_state)?;
    let great_person = super::gifts::great_person_gift_influence(view.auth, rules, core.owner, core.kind) > 0;
    if !great_person && !giftable_unit(rules, core.kind) {
        return Err(CityStateBlock::UnitNotGiftable.into());
    }
    if view.auth.map().owner(core.tile).flatten() != Some(city_state) {
        return Err(CityStateBlock::NotTheirLand.into());
    }
    Ok(ResolvedCommand::GiftUnit { unit, city_state })
}

/// 200 gold: the improvement a strategic or luxury resource of the
/// city-state needs, when the giver could build it.
pub(crate) fn validate_gift_improvement(
    view: GameView<'_>,
    rules: &CompiledRuleset,
    player: PlayerId,
    city_state: PlayerId,
    tile: TileId,
) -> Result<ResolvedCommand, CommandError> {
    let auth = view.auth;
    nation_and_city_state(auth, player, city_state)?;
    if auth.map().owner(tile).flatten() != Some(city_state)
        || auth.map().improvement(tile).flatten().is_some()
    {
        return Err(CityStateBlock::NoFreeTile.into());
    }
    let resource = auth
        .map()
        .resource(tile)
        .flatten()
        .and_then(|r| rules.resources.get(r.0 as usize))
        .filter(|d| {
            matches!(
                d.class,
                Some(ResourceClass::Strategic | ResourceClass::Luxury)
            )
        })
        .ok_or(CityStateBlock::NoResource)?;
    let improvement = resource
        .improvements
        .first()
        .copied()
        .ok_or(CityStateBlock::NoImprovementNeeded)?;
    let known = rules
        .improvements
        .get(improvement.0 as usize)
        .and_then(|d| d.required_tech)
        .is_none_or(|t| auth.players().techs_of(player).contains(&t));
    if !known {
        return Err(CityStateBlock::ImprovementUnknown.into());
    }
    if auth.players().treasury_of(player) < IMPROVEMENT_GIFT_COST {
        return Err(CityStateBlock::NotEnoughGold.into());
    }
    Ok(ResolvedCommand::GiftImprovement {
        player,
        tile,
        improvement,
    })
}

pub(crate) fn validate_bully(
    view: GameView<'_>,
    rules: &CompiledRuleset,
    player: PlayerId,
    city_state: PlayerId,
    worker: bool,
) -> Result<ResolvedCommand, CommandError> {
    nation_and_city_state(view.auth, player, city_state)?;
    if bully_score(view.auth, rules, city_state, player, worker) < 0 {
        return Err(CityStateBlock::NotAfraid.into());
    }
    if worker {
        return Ok(ResolvedCommand::BullyWorker { player, city_state });
    }
    Ok(ResolvedCommand::BullyGold { player, city_state })
}

pub(crate) fn validate_pledge(
    view: GameView<'_>,
    player: PlayerId,
    city_state: PlayerId,
    pledge: bool,
) -> Result<ResolvedCommand, CommandError> {
    nation_and_city_state(view.auth, player, city_state)?;
    if let Some(block) = pledge_problem(view.auth, city_state, player, pledge) {
        return Err(block.into());
    }
    Ok(ResolvedCommand::Pledge {
        player,
        city_state,
        pledge,
    })
}

pub(crate) fn validate_buyout(
    view: GameView<'_>,
    rules: &CompiledRuleset,
    player: PlayerId,
    city_state: PlayerId,
) -> Result<ResolvedCommand, CommandError> {
    nation_and_city_state(view.auth, player, city_state)?;
    if let Some(block) = super::buyout::buyout_problem(view.auth, rules, city_state, player) {
        return Err(block.into());
    }
    Ok(ResolvedCommand::BuyoutCityState { player, city_state })
}
