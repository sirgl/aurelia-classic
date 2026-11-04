//! Validation: read-only, without side effects, deterministic (§11).
//!
//! Scratch for path search is passed from outside (owned by GameState, reused):
//! the output of validate depends only on (state, rules, command), observably pure.

use crate::command::{Command, CommandError, ResolvedCommand};
use crate::ids::{PlayerId, UnitHandle};
use crate::world::espionage::SpyBlock;
use crate::navigation::PathScratch;
use kernel::command::validate_kernel;
use crate::rules::CompiledRuleset;
use crate::world::state::GameView;
use crate::systems::combat::validate as combat;
use crate::systems::diplomacy::validate as diplomacy;

/// The rejection of a command of a player who is not on turn.
pub const NOT_YOUR_TURN: &str = "not your turn";

pub fn validate(
    state: GameView<'_>,
    rules: &CompiledRuleset,
    command: Command,
    scratch: &mut PathScratch,
) -> Result<ResolvedCommand, CommandError> {
    if let Some(command) = command.kernel() {
        return validate_kernel(state, rules, command, scratch).map(ResolvedCommand::Kernel);
    }
    // Sequential turns: only the active player acts (turn::state).
    if let Some(actor) = acting_player(state, &command) {
        if actor != state.auth.active_player() {
            return Err(CommandError::RuleViolation(NOT_YOUR_TURN.into()));
        }
    }
    match command {
        Command::MoveUnit { .. }
        | Command::MarchTo { .. }
        | Command::FoundCity { .. }
        | Command::Fortify { .. }
        | Command::BuildImprovement { .. }
        | Command::BuildRoad { .. }
        | Command::ChopForest { .. }
        | Command::ChooseTechnology { .. }
        | Command::PlanResearch { .. }
        | Command::EnqueueProduction { .. }
        | Command::ClearProduction { .. }
        | Command::HurryProduction { .. }
        | Command::PurchaseTile { .. }
        | Command::ToggleWorkedTile { .. }
        | Command::LockWorkedTile { .. }
        | Command::SetCityFocus { .. }
        | Command::SetAvoidGrowth { .. }
        | Command::ChooseCityName { .. }
        | Command::SetSpecialists { .. }
        | Command::EndTurn { .. } => unreachable!("a kernel command is validated by the kernel"),
        Command::UnlockPolicyBranch { player, branch } => {
            crate::derived::culture::branch_status(state.auth, rules, player, branch)
                .map_err(|b| CommandError::RuleViolation(b.reason().into()))?;
            Ok(ResolvedCommand::UnlockPolicyBranch { player, branch })
        }
        Command::AdoptPolicy { player, policy } => {
            crate::derived::culture::policy_status(state.auth, rules, player, policy)
                .map_err(|b| CommandError::RuleViolation(b.reason().into()))?;
            Ok(ResolvedCommand::AdoptPolicy { player, policy })
        }
        Command::ChooseIdeology { player, ideology } => {
            crate::derived::culture::ideology_status(state.auth, rules, player, ideology)
                .map_err(|b| CommandError::RuleViolation(b.reason().into()))?;
            Ok(ResolvedCommand::ChooseIdeology { player, ideology })
        }
        Command::FoundPantheon { player, belief } => {
            crate::derived::religion::pantheon_status(state.auth, rules, player, belief).map_err(faith_rule)?;
            Ok(ResolvedCommand::FoundPantheon { player, belief })
        }
        Command::FoundReligion {
            unit,
            religion,
            founder,
            follower,
            pantheon,
            bonus,
        } => {
            let choice = crate::derived::religion::FoundingChoice {
                religion,
                founder,
                follower,
                pantheon,
                bonus,
            };
            crate::derived::religion::found_status(state.auth, rules, unit, &choice).map_err(faith_rule)?;
            Ok(ResolvedCommand::FoundReligion {
                unit,
                religion,
                founder,
                follower,
                pantheon,
                bonus,
            })
        }
        Command::EnhanceReligion {
            unit,
            follower,
            enhancer,
        } => {
            crate::derived::religion::enhance_status(state.auth, rules, unit, follower, enhancer)
                .map_err(faith_rule)?;
            Ok(ResolvedCommand::EnhanceReligion {
                unit,
                follower,
                enhancer,
            })
        }
        Command::AddReformationBelief { player, belief } => {
            crate::derived::religion::reformation_status(state.auth, rules, player, belief).map_err(faith_rule)?;
            Ok(ResolvedCommand::AddReformationBelief { player, belief })
        }
        Command::SpreadReligion { unit } => {
            crate::derived::religion::spread_status(state.auth, rules, unit).map_err(faith_rule)?;
            Ok(ResolvedCommand::SpreadReligion { unit })
        }
        Command::RemoveHeresy { unit } => {
            crate::derived::religion::heresy_status(state.auth, rules, unit).map_err(faith_rule)?;
            Ok(ResolvedCommand::RemoveHeresy { unit })
        }
        Command::PurchaseWithFaith { player, city, item } => {
            needs_resources(state.auth, rules, player, item)?;
            crate::derived::religion::faith_purchase_status(state.auth, rules, player, city, item)
                .map_err(|b| CommandError::RuleViolation(b.reason().into()))?;
            Ok(ResolvedCommand::PurchaseWithFaith { player, city, item })
        }
        Command::CreateGreatImprovement { unit } => {
            crate::derived::religion::great_improvement_status(state.auth, rules, unit).map_err(faith_rule)?;
            Ok(ResolvedCommand::CreateGreatImprovement { unit })
        }
        Command::CreateGreatWork { unit } => {
            crate::derived::tourism::great_work_status(state.auth, rules, unit).map_err(tourism_rule)?;
            Ok(ResolvedCommand::CreateGreatWork { unit })
        }
        Command::WriteTreatise { unit } => {
            crate::derived::tourism::treatise_status(state.auth, rules, unit).map_err(tourism_rule)?;
            Ok(ResolvedCommand::WriteTreatise { unit })
        }
        Command::PerformConcert { unit } => {
            crate::derived::tourism::concert_status(state.auth, rules, unit).map_err(tourism_rule)?;
            Ok(ResolvedCommand::PerformConcert { unit })
        }
        Command::Excavate { unit } => {
            crate::derived::archaeology::excavate_status(state.auth, rules, unit).map_err(dig_rule)?;
            Ok(ResolvedCommand::Excavate { unit })
        }
        Command::ChooseFreeTechnology { player, tech } => {
            crate::derived::grants::free_technology_status(state.auth, rules, player, tech)
                .map_err(|b| CommandError::RuleViolation(b.reason().into()))?;
            Ok(ResolvedCommand::ChooseFreeTechnology { player, tech })
        }
        Command::ChooseFreeGreatPerson { player, kind } => {
            crate::derived::grants::free_great_person_status(state.auth, rules, player, kind)
                .map_err(|b| CommandError::RuleViolation(b.reason().into()))?;
            Ok(ResolvedCommand::ChooseFreeGreatPerson { player, kind })
        }
        Command::ClaimRuins { unit, reward } => {
            let tile = crate::derived::ruins::claim_status(state.auth, rules, unit, reward)?;
            Ok(ResolvedCommand::ClaimRuins { unit, tile, reward })
        }
        Command::ChooseArchaeology { player, tile, choice } => {
            crate::derived::archaeology::choice_status(state.auth, rules, player, tile, choice).map_err(dig_rule)?;
            Ok(ResolvedCommand::ChooseArchaeology { player, tile, choice })
        }
        Command::SwitchIdeology { player } => {
            crate::derived::tourism::switch_ideology_status(state.auth, player).map_err(tourism_rule)?;
            Ok(ResolvedCommand::SwitchIdeology { player })
        }
        Command::AttackUnit { attacker, defender } => {
            let target = state
                .auth
                .units()
                .get(defender)
                .ok_or(CommandError::UnknownUnit)?
                .core
                .tile;
            combat::validate_attack_tile(state, rules, attacker, target)
        }
        Command::AttackCity { attacker, city } => {
            let target = state.auth.cities().get(city).ok_or(CommandError::UnknownCity)?.tile;
            combat::validate_attack_tile(state, rules, attacker, target)
        }
        Command::RangedAttack { unit, target } => combat::validate_ranged(state, rules, unit, target),
        Command::CityStrike { city, target } => {
            combat::validate_city_strike(state, rules, city, target)
        }
        Command::SetUpRangedAttack { unit } => combat::validate_set_up(state, rules, unit),
        Command::Promote { unit, promotion } => {
            combat::validate_promote(state, rules, unit, promotion)
        }
        Command::UpgradeUnit { unit } => combat::validate_upgrade(state, rules, unit),
        Command::DecideCity {
            player,
            city,
            decision,
        } => combat::validate_decide_city(state, rules, player, city, decision),
        Command::Pillage { unit } => combat::validate_pillage(state, rules, unit),
        Command::Rebase { unit, city } => combat::validate_rebase(state, rules, unit, city),
        Command::SellExoticGoods { unit } => combat::validate_exotic_goods(state, rules, unit),
        Command::RebaseToCarrier { unit, carrier } => combat::validate_rebase_to_carrier(state, rules, unit, carrier),
        Command::AirSweep { unit, target } => {
            combat::validate_air_sweep(state, rules, unit, target)
        }
        Command::Intercept { unit } => combat::validate_intercept(state, rules, unit),
        Command::Nuke { unit, target } => combat::validate_nuke(state, rules, unit, target),
        Command::BuildCitadel { unit } => combat::validate_citadel(state, rules, unit),
        Command::EstablishTradeRoute { unit, destination, kind } => {
            crate::derived::trade::route_status(state.auth, rules, unit, destination, kind).map_err(trade_rule)?;
            Ok(ResolvedCommand::EstablishTradeRoute { unit, destination, kind })
        }
        Command::ChangeTradeHome { unit, city } => {
            crate::derived::trade::home_status(state.auth, rules, unit, city).map_err(trade_rule)?;
            Ok(ResolvedCommand::ChangeTradeHome { unit, city })
        }
        Command::PlunderTradeRoute { unit } => {
            crate::derived::trade::plunder_target(state.auth, rules, unit).map_err(trade_rule)?;
            Ok(ResolvedCommand::PlunderTradeRoute { unit })
        }
        Command::Airlift { unit, city } => {
            crate::systems::airlift::airlift_status(state.auth, rules, unit, city)?;
            Ok(ResolvedCommand::Airlift { unit, city })
        }
        Command::Paradrop { unit, destination } => {
            crate::systems::paradrop::paradrop_status(state, rules, unit, destination)?;
            Ok(ResolvedCommand::Paradrop { unit, to: destination })
        }
        Command::DeclareWar { player, enemy } => diplomacy::validate_declare_war(state, player, enemy),
        Command::MakePeace { player, enemy } => diplomacy::validate_make_peace(state, player, enemy),
        Command::OpenBorders { host, guest } => {
            diplomacy::validate_open_borders(state, rules, host, guest)
        }
        Command::Denounce { player, target } => diplomacy::validate_denounce(state, player, target),
        Command::ProposeDeal { from, to, ref items } => {
            diplomacy::validate_propose_deal(state, rules, from, to, items)
        }
        Command::RespondDeal {
            player,
            offer,
            accept,
        } => diplomacy::validate_respond_deal(state, rules, player, offer, accept),
        Command::ProposeResolution {
            player,
            kind,
            choice,
            repeal,
        } => match crate::systems::congress::proposal_problem(state.auth, rules, player, kind, choice, repeal) {
            Some(reason) => Err(CommandError::RuleViolation(reason.into())),
            None => Ok(ResolvedCommand::ProposeResolution {
                player,
                kind,
                choice,
                repeal,
            }),
        },
        Command::CastVotes {
            player,
            proposal,
            choice,
            delegates,
        } => match crate::systems::congress::vote_problem(state.auth, player, proposal, choice, delegates) {
            Some(reason) => Err(CommandError::RuleViolation(reason.into())),
            None => Ok(ResolvedCommand::CastVotes {
                player,
                proposal,
                choice,
                delegates,
            }),
        },
        Command::MoveSpy {
            player,
            spy,
            city,
            diplomat,
        } => {
            if state.auth.module().espionage().spies_of(player).get(spy as usize).is_none() {
                return Err(SpyBlock::NoSuchSpy.into());
            }
            match crate::systems::espionage::move_problem(state.auth, player, spy as usize, city, diplomat) {
                Some(block) => Err(block.into()),
                None => Ok(ResolvedCommand::MoveSpy {
                    player,
                    spy,
                    city,
                    diplomat,
                }),
            }
        }
        Command::StealTechnology { player, from, tech } => {
            let rights = state.auth.module().espionage().steal_rights(player, from);
            let ok = rights > 0 && crate::systems::espionage::stealable(state.auth, rules, player, from).contains(&tech);
            if !ok {
                return Err(SpyBlock::NothingToSteal.into());
            }
            Ok(ResolvedCommand::StealTechnology { player, from, tech })
        }
        Command::StageCoup { player, spy } => {
            let possible = state
                .auth
                .module()
                .espionage()
                .spies_of(player)
                .get(spy as usize)
                .and_then(|s| crate::systems::espionage::coup_chance(state.auth, player, s))
                .is_some();
            if !possible {
                return Err(SpyBlock::NoCoup.into());
            }
            Ok(ResolvedCommand::StageCoup { player, spy })
        }
        Command::BuildSpaceshipPart { unit } => {
            let project = crate::systems::victory::spaceship_problem(state.auth, rules, unit)
                .map_err(|r| CommandError::RuleViolation(r.into()))?;
            Ok(ResolvedCommand::InstallSpaceshipPart { unit, project })
        }
        Command::GiftGold {
            player,
            city_state,
            gold,
        } => crate::systems::city_states::validate::validate_gift_gold(state, player, city_state, gold),
        Command::GiftUnit { unit, city_state } => {
            crate::systems::city_states::validate::validate_gift_unit(state, rules, unit, city_state)
        }
        Command::GiftImprovement {
            player,
            city_state,
            tile,
        } => crate::systems::city_states::validate::validate_gift_improvement(state, rules, player, city_state, tile),
        Command::BullyGold { player, city_state } => {
            crate::systems::city_states::validate::validate_bully(state, rules, player, city_state, false)
        }
        Command::BullyWorker { player, city_state } => {
            crate::systems::city_states::validate::validate_bully(state, rules, player, city_state, true)
        }
        Command::PledgeProtection {
            player,
            city_state,
            pledge,
        } => crate::systems::city_states::validate::validate_pledge(state, player, city_state, pledge),
        Command::BuyoutCityState { player, city_state } => {
            crate::systems::city_states::validate::validate_buyout(state, rules, player, city_state)
        }
        Command::GreatPersonAction { unit, action } => {
            crate::derived::great_people::validate_action(state.auth, rules, unit, action)
        }
        Command::SetProcess { player, city, process } => {
            crate::systems::processes::validate_process(state.auth, rules, player, city, process)
        }
    }
}

/// The player a command acts for: the owner of the ordered unit, or the
/// player named by the command. None for an unknown unit (the command's
/// own check reports it).
pub(crate) fn acting_player(state: GameView<'_>, command: &Command) -> Option<PlayerId> {
    let unit_owner = |unit: UnitHandle| state.auth.units().get(unit).map(|u| u.core.owner);
    match *command {
        Command::MoveUnit { unit, .. }
        | Command::Fortify { unit }
        | Command::BuildImprovement { unit, .. }
        | Command::BuildRoad { unit }
        | Command::ChopForest { unit }
        | Command::GreatPersonAction { unit, .. }
        | Command::MarchTo { unit, .. }
        | Command::RangedAttack { unit, .. }
        | Command::SetUpRangedAttack { unit }
        | Command::Promote { unit, .. }
        | Command::UpgradeUnit { unit }
        | Command::Pillage { unit }
        | Command::Rebase { unit, .. }
        | Command::RebaseToCarrier { unit, .. }
        | Command::SellExoticGoods { unit }
        | Command::AirSweep { unit, .. }
        | Command::Intercept { unit }
        | Command::Nuke { unit, .. }
        | Command::BuildCitadel { unit }
        | Command::EstablishTradeRoute { unit, .. }
        | Command::ChangeTradeHome { unit, .. }
        | Command::PlunderTradeRoute { unit }
        | Command::Airlift { unit, .. }
        | Command::Paradrop { unit, .. } => unit_owner(unit),
        Command::CityStrike { city, .. } => state.auth.cities().get(city).map(|c| c.owner),
        Command::DecideCity { player, .. } => Some(player),
        Command::FoundCity { settler } => unit_owner(settler),
        Command::AttackUnit { attacker, .. } | Command::AttackCity { attacker, .. } => {
            unit_owner(attacker)
        }
        Command::PlanResearch { player, .. }
        | Command::ChooseTechnology { player, .. }
        | Command::DeclareWar { player, .. }
        | Command::MakePeace { player, .. }
        | Command::EnqueueProduction { player, .. }
        | Command::ClearProduction { player, .. }
        | Command::ToggleWorkedTile { player, .. }
        | Command::LockWorkedTile { player, .. }
        | Command::SetCityFocus { player, .. }
        | Command::SetAvoidGrowth { player, .. }
        | Command::ChooseCityName { player, .. }
        | Command::SetSpecialists { player, .. }
        | Command::SetProcess { player, .. }
        | Command::HurryProduction { player, .. }
        | Command::PurchaseTile { player, .. }
        | Command::EndTurn { player }
        | Command::UnlockPolicyBranch { player, .. }
        | Command::AdoptPolicy { player, .. }
        | Command::ChooseIdeology { player, .. }
        | Command::FoundPantheon { player, .. }
        | Command::AddReformationBelief { player, .. }
        | Command::PurchaseWithFaith { player, .. }
        | Command::SwitchIdeology { player }
        | Command::ChooseArchaeology { player, .. }
        | Command::ChooseFreeTechnology { player, .. }
        | Command::ChooseFreeGreatPerson { player, .. } => Some(player),
        Command::FoundReligion { unit, .. }
        | Command::EnhanceReligion { unit, .. }
        | Command::SpreadReligion { unit }
        | Command::RemoveHeresy { unit }
        | Command::CreateGreatImprovement { unit }
        | Command::CreateGreatWork { unit }
        | Command::WriteTreatise { unit }
        | Command::PerformConcert { unit }
        | Command::ClaimRuins { unit, .. }
        | Command::Excavate { unit } => unit_owner(unit),
        Command::OpenBorders { host, .. } => Some(host),
        Command::Denounce { player, .. } => Some(player),
        Command::ProposeDeal { from, .. } => Some(from),
        Command::GiftGold { player, .. }
        | Command::GiftImprovement { player, .. }
        | Command::BullyGold { player, .. }
        | Command::BullyWorker { player, .. }
        | Command::PledgeProtection { player, .. }
        | Command::BuyoutCityState { player, .. }
        | Command::ProposeResolution { player, .. }
        | Command::CastVotes { player, .. }
        | Command::MoveSpy { player, .. }
        | Command::StealTechnology { player, .. }
        | Command::StageCoup { player, .. } => Some(player),
        Command::GiftUnit { unit, .. } | Command::BuildSpaceshipPart { unit } => unit_owner(unit),
        // Answers come on any turn. In a simultaneous
        // window (multiplayer) the answer is the answering seat's own: a seat must not accept
        // or refuse an offer made to another.
        Command::RespondDeal { player, .. } => state.auth.turn().is_acting().then_some(player),
    }
}

fn dig_rule(block: crate::derived::archaeology::DigBlock) -> CommandError {
    CommandError::RuleViolation(block.reason().into())
}

fn tourism_rule(block: crate::derived::tourism::TourismBlock) -> CommandError {
    CommandError::RuleViolation(block.reason().into())
}

/// The player has the strategic resources `item` takes (derived::resources).
pub(crate) fn needs_resources(
    auth: &crate::world::state::AuthWorld,
    rules: &CompiledRuleset,
    player: PlayerId,
    item: crate::ids::ProductionItem,
) -> Result<(), CommandError> {
    crate::derived::resources::production_resources(auth, rules, player, item).map_err(|r| {
        let name = rules.resources.get(r.0 as usize).map_or("a resource", |d| d.name.as_str());
        CommandError::RuleViolation(format!("needs {name}"))
    })
}

pub(crate) fn trade_rule(block: crate::derived::trade::TradeBlock) -> CommandError {
    CommandError::RuleViolation(block.reason().into())
}

fn faith_rule(block: crate::derived::religion::FaithBlock) -> CommandError {
    CommandError::RuleViolation(block.reason().into())
}

/// The player's empire is very unhappy (no settlers, no new cities).
pub(crate) fn very_unhappy(auth: &crate::world::state::AuthWorld, player: crate::ids::PlayerId) -> bool {
    let excess = crate::derived::happiness::excess_snapshot(auth, player);
    crate::derived::happiness::mood(excess) == crate::derived::happiness::Mood::VeryUnhappy
}
