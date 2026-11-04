//! The commands the rules accept for one unit now (`unit_actions`): the action
//! list a learned policy chooses from and the recorder writes beside each unit
//! decision. Candidates come from what the player knows (its reachable tiles,
//! what it sees around the unit, its rules); the rules' own check (`refusal`)
//! keeps the legal ones. Not enumerated: `MarchTo` (any known tile: unbounded),
//! `FoundReligion` and `EnhanceReligion` (belief combinations); `enumerated`
//! says which commands the list can hold.
//!
//! Only the public view API and the observation are read here: the list is
//! what the player could work out itself, never the truth behind the fog.

use crate::command::Command;
use crate::derived::great_people::GreatPersonAction;
use crate::ids::{ImprovementId, UnitHandle};
use crate::knowledge::observation::LocalObservation;
use crate::knowledge::public::PlayerQueries;
use crate::rules::content::Domain;
use crate::systems::ruins::{Reward, REWARDS};
use crate::world::player::{ClassicKind, PlayerKind};
use crate::world::trade::RouteKind;

/// The legal commands of the observation's unit, in a fixed order: by the
/// command's codec tag (`command/codec.rs`), then by its parameters (tile,
/// then id, ascending).
pub fn unit_actions(near: &LocalObservation<'_>) -> Vec<Command> {
    let unit = near.unit.handle;
    let mut out = Vec::new();
    for command in parameterless(unit) {
        push_if_accepted(near, &mut out, command);
    }
    moves(near, &mut out);
    attacks(near, &mut out);
    promotions(near, &mut out);
    builds(near, &mut out);
    great_person(near, &mut out);
    ruins(near, &mut out);
    bases(near, &mut out);
    trade(near, &mut out);
    strikes_on_disc(near, &mut out);
    gift(near, &mut out);
    out.sort_by_key(order_key);
    out.dedup_by_key(|c| order_key(c));
    out
}

/// Whether `unit_actions` can list a command of this kind (false for the
/// kinds it leaves out, and for commands that are not a unit's).
pub fn enumerated(command: &Command) -> bool {
    order_key(command).is_some()
}

/// The codec tag of a command `unit_actions` can list (`command/codec.rs`);
/// None for the kinds it leaves out.
pub fn codec_tag(command: &Command) -> Option<u8> {
    order_key(command).map(|(tag, _, _)| tag)
}

/// The codec tag of `MarchTo` (`command/codec.rs`), not enumerated.
pub const MARCH_TAG: u8 = 10;

/// The unit a unit command is given to (None: not a unit's command).
pub fn acting_unit(command: &Command) -> Option<UnitHandle> {
    use Command as C;
    Some(match *command {
        C::FoundCity { settler } => settler,
        C::AttackUnit { attacker, .. } | C::AttackCity { attacker, .. } => attacker,
        C::MoveUnit { unit, .. }
        | C::MarchTo { unit, .. }
        | C::Fortify { unit }
        | C::BuildImprovement { unit, .. }
        | C::BuildRoad { unit }
        | C::ChopForest { unit }
        | C::GreatPersonAction { unit, .. }
        | C::RangedAttack { unit, .. }
        | C::SetUpRangedAttack { unit }
        | C::Promote { unit, .. }
        | C::UpgradeUnit { unit }
        | C::Pillage { unit }
        | C::Rebase { unit, .. }
        | C::SellExoticGoods { unit }
        | C::RebaseToCarrier { unit, .. }
        | C::AirSweep { unit, .. }
        | C::Intercept { unit }
        | C::Nuke { unit, .. }
        | C::BuildCitadel { unit }
        | C::EstablishTradeRoute { unit, .. }
        | C::ChangeTradeHome { unit, .. }
        | C::PlunderTradeRoute { unit }
        | C::Airlift { unit, .. }
        | C::Paradrop { unit, .. }
        | C::BuildSpaceshipPart { unit }
        | C::GiftUnit { unit, .. }
        | C::FoundReligion { unit, .. }
        | C::EnhanceReligion { unit, .. }
        | C::SpreadReligion { unit }
        | C::RemoveHeresy { unit }
        | C::CreateGreatImprovement { unit }
        | C::CreateGreatWork { unit }
        | C::WriteTreatise { unit }
        | C::PerformConcert { unit }
        | C::Excavate { unit }
        | C::ClaimRuins { unit, .. } => unit,
        _ => return None,
    })
}

/// Keep `command` when the rules accept it now.
fn push_if_accepted(near: &LocalObservation<'_>, out: &mut Vec<Command>, command: Command) {
    if near.view.refusal(&command).is_none() {
        out.push(command);
    }
}

/// The unit commands with no parameter but the unit.
fn parameterless(unit: UnitHandle) -> [Command; 19] {
    [
        Command::FoundCity { settler: unit },
        Command::Fortify { unit },
        Command::BuildRoad { unit },
        Command::ChopForest { unit },
        Command::SetUpRangedAttack { unit },
        Command::UpgradeUnit { unit },
        Command::Pillage { unit },
        Command::Intercept { unit },
        Command::BuildCitadel { unit },
        Command::PlunderTradeRoute { unit },
        Command::BuildSpaceshipPart { unit },
        Command::SpreadReligion { unit },
        Command::RemoveHeresy { unit },
        Command::CreateGreatImprovement { unit },
        Command::CreateGreatWork { unit },
        Command::WriteTreatise { unit },
        Command::PerformConcert { unit },
        Command::Excavate { unit },
        Command::SellExoticGoods { unit },
    ]
}

/// A move to every tile the unit reaches this turn (its own tile aside).
fn moves(near: &LocalObservation<'_>, out: &mut Vec<Command>) {
    let unit = near.unit.handle;
    for (destination, _) in near.view.own_reachable(unit) {
        if destination != near.unit.tile {
            push_if_accepted(near, out, Command::MoveUnit { unit, destination });
        }
    }
}

/// Charges on the foreign units and cities in sight, and shots at their
/// tiles (the rules refuse what is out of range or not attackable).
fn attacks(near: &LocalObservation<'_>, out: &mut Vec<Command>) {
    let attacker = near.unit.handle;
    for foe in &near.foreign_units {
        push_if_accepted(near, out, Command::AttackUnit { attacker, defender: foe.handle });
        push_if_accepted(near, out, Command::RangedAttack { unit: attacker, target: foe.tile });
    }
    for city in &near.foreign_cities {
        push_if_accepted(near, out, Command::AttackCity { attacker, city: city.handle });
        push_if_accepted(near, out, Command::RangedAttack { unit: attacker, target: city.tile });
    }
}

fn promotions(near: &LocalObservation<'_>, out: &mut Vec<Command>) {
    let unit = near.unit.handle;
    for promotion in near.view.promotions_available(unit) {
        push_if_accepted(near, out, Command::Promote { unit, promotion });
    }
}

/// Every improvement of the rules on the unit's tile.
fn builds(near: &LocalObservation<'_>, out: &mut Vec<Command>) {
    let unit = near.unit.handle;
    for improvement in improvements(near) {
        push_if_accepted(near, out, Command::BuildImprovement { unit, improvement });
    }
}

/// Every great person action, `Construct` once per improvement.
fn great_person(near: &LocalObservation<'_>, out: &mut Vec<Command>) {
    let unit = near.unit.handle;
    let fixed = [
        GreatPersonAction::DiscoverTechnology,
        GreatPersonAction::HurryProduction,
        GreatPersonAction::StartGoldenAge,
        GreatPersonAction::TradeMission,
        GreatPersonAction::BuyCityState,
    ];
    for action in improvements(near).map(GreatPersonAction::Construct).chain(fixed) {
        push_if_accepted(near, out, Command::GreatPersonAction { unit, action });
    }
}

fn ruins(near: &LocalObservation<'_>, out: &mut Vec<Command>) {
    let unit = near.unit.handle;
    for reward in REWARDS {
        push_if_accepted(near, out, Command::ClaimRuins { unit, reward });
    }
}

/// Rebase, airlift and a new trade home to every own city in the disc, a
/// carrier among every own unit there. Cities and carriers beyond the disc
/// are not offered.
fn bases(near: &LocalObservation<'_>, out: &mut Vec<Command>) {
    let unit = near.unit.handle;
    for city in &near.own_cities {
        push_if_accepted(near, out, Command::Rebase { unit, city: city.handle });
        push_if_accepted(near, out, Command::Airlift { unit, city: city.handle });
        push_if_accepted(near, out, Command::ChangeTradeHome { unit, city: city.handle });
    }
    for carrier in &near.own_units {
        push_if_accepted(near, out, Command::RebaseToCarrier { unit, carrier: carrier.handle });
    }
}

/// The routes a trade unit standing in an own city could start.
fn trade(near: &LocalObservation<'_>, out: &mut Vec<Command>) {
    let unit = near.unit.handle;
    let Some(home) = near.own_cities.iter().find(|c| c.tile == near.unit.tile) else {
        return;
    };
    for offer in near.view.trade_prospects(home.handle, near.unit.kind) {
        let command = Command::EstablishTradeRoute { unit, destination: offer.destination, kind: offer.kind };
        push_if_accepted(near, out, command);
    }
}

/// Paradrops, air sweeps and nuclear strikes on every tile of the disc, for
/// a unit whose kind can do them (the rules decide the rest).
fn strikes_on_disc(near: &LocalObservation<'_>, out: &mut Vec<Command>) {
    let unit = near.unit.handle;
    let def = near.view.rules().units.get(near.unit.kind.0 as usize);
    let paradrops = near.view.paradrop_range(unit) > 0;
    let flies = def.is_some_and(|d| d.domain == Domain::Air);
    let nuclear = def.is_some_and(|d| d.combat.nuke_level > 0);
    if !paradrops && !flies && !nuclear {
        return;
    }
    for target in crate::math::hex_disc(near.view.dimensions(), near.unit.tile, near.radius) {
        if paradrops {
            push_if_accepted(near, out, Command::Paradrop { unit, destination: target });
        }
        if flies {
            push_if_accepted(near, out, Command::AirSweep { unit, target });
        }
        if nuclear {
            push_if_accepted(near, out, Command::Nuke { unit, target });
        }
    }
}

/// A gift to the city-state whose land the unit stands on.
fn gift(near: &LocalObservation<'_>, out: &mut Vec<Command>) {
    let unit = near.unit.handle;
    let Some(owner) = near.view.tile(near.unit.tile).snapshot().and_then(|s| s.owner) else {
        return;
    };
    let city_state = near.relation(owner).is_some_and(|r| matches!(r.kind, PlayerKind::Module(ClassicKind::CityState(_))));
    if city_state {
        push_if_accepted(near, out, Command::GiftUnit { unit, city_state: owner });
    }
}

fn improvements(near: &LocalObservation<'_>) -> impl Iterator<Item = ImprovementId> {
    (0..near.view.rules().improvements.len()).map(|i| ImprovementId(i as u16))
}

/// The place of a command in the list: its codec tag (`command/codec.rs`),
/// then its parameters (tile, then id). None for a command the list never
/// holds.
fn order_key(command: &Command) -> Option<(u8, u32, u32)> {
    let action_key = |action: GreatPersonAction| match action {
        GreatPersonAction::Construct(improvement) => (0, u32::from(improvement.0)),
        GreatPersonAction::DiscoverTechnology => (1, 0),
        GreatPersonAction::HurryProduction => (2, 0),
        GreatPersonAction::StartGoldenAge => (3, 0),
        GreatPersonAction::TradeMission => (4, 0),
        GreatPersonAction::BuyCityState => (5, 0),
    };
    let route_key = |kind: RouteKind| match kind {
        RouteKind::International => 0,
        RouteKind::Food => 1,
        RouteKind::Production => 2,
    };
    let reward_key = |reward: Reward| match reward {
        Reward::Citizen => 0,
        Reward::Culture => 1,
        Reward::Faith => 2,
        Reward::Gold => 3,
        Reward::Experience => 4,
        Reward::Map => 5,
    };
    Some(match *command {
        Command::MoveUnit { destination, .. } => (0, destination.0, 0),
        Command::FoundCity { .. } => (1, 0, 0),
        Command::AttackUnit { defender, .. } => (4, defender.index, 0),
        Command::AttackCity { city, .. } => (5, city.index, 0),
        Command::Fortify { .. } => (6, 0, 0),
        Command::BuildImprovement { improvement, .. } => (7, u32::from(improvement.0), 0),
        Command::BuildRoad { .. } => (8, 0, 0),
        Command::ChopForest { .. } => (9, 0, 0),
        Command::GreatPersonAction { action, .. } => {
            let (variant, improvement) = action_key(action);
            (22, variant, improvement)
        }
        Command::RangedAttack { target, .. } => (25, target.0, 0),
        Command::SetUpRangedAttack { .. } => (27, 0, 0),
        Command::Promote { promotion, .. } => (28, u32::from(promotion.0), 0),
        Command::UpgradeUnit { .. } => (29, 0, 0),
        Command::Pillage { .. } => (31, 0, 0),
        Command::Rebase { city, .. } => (32, city.index, 0),
        Command::AirSweep { target, .. } => (33, target.0, 0),
        Command::Intercept { .. } => (34, 0, 0),
        Command::Nuke { target, .. } => (35, target.0, 0),
        Command::BuildCitadel { .. } => (36, 0, 0),
        Command::EstablishTradeRoute { destination, kind, .. } => (37, destination.index, route_key(kind)),
        Command::ChangeTradeHome { city, .. } => (38, city.index, 0),
        Command::PlunderTradeRoute { .. } => (39, 0, 0),
        Command::Airlift { city, .. } => (40, city.index, 0),
        Command::BuildSpaceshipPart { .. } => (49, 0, 0),
        Command::GiftUnit { city_state, .. } => (51, u32::from(city_state.0), 0),
        Command::SpreadReligion { .. } => (64, 0, 0),
        Command::RemoveHeresy { .. } => (65, 0, 0),
        Command::CreateGreatImprovement { .. } => (67, 0, 0),
        Command::CreateGreatWork { .. } => (68, 0, 0),
        Command::WriteTreatise { .. } => (69, 0, 0),
        Command::PerformConcert { .. } => (70, 0, 0),
        Command::Excavate { .. } => (72, 0, 0),
        Command::RebaseToCarrier { carrier, .. } => (77, carrier.index, 0),
        Command::SellExoticGoods { .. } => (78, 0, 0),
        Command::ClaimRuins { reward, .. } => (80, reward_key(reward), 0),
        Command::Paradrop { destination, .. } => (81, destination.0, 0),
        _ => return None,
    })
}
