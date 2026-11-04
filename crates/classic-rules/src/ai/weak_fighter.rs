//! Control fighters: deliberately worse ways for a land military unit at war
//! to fight, the rest of the AI `Standard`'s (classic/docs/ai-combat-diagnosis.md).
//! The battle bench (`ai-battle`) must rank each of them far below `standard`;
//! a control it does not rank lower shows that the bench measures something
//! other than the fighting.
//!
//! A control decides for a land military unit that sees a unit or a city of a
//! player at war with it (the unit's local observation); every other decision,
//! the march to the front among them, stays the planners'. The start-of-turn
//! strikes of the unit's own (the military advisor's shots and siege set-ups,
//! `ai::combat::MilitaryAdvisor`) are dropped (`strips`): the control alone
//! decides whether a unit strikes. City strikes stay.

use crate::ai::planner::{role_of, AiRole, Decision, UnitView};
use crate::ai::planner_units::hold;
use crate::ai::trace::{Actor, Reason};
use crate::command::Command;
use crate::ids::TileId;
use crate::knowledge::actions::unit_actions;
use crate::knowledge::observation::OwnUnit;
use crate::rules::content::Domain;
use crate::rules::CompiledRuleset;

/// How a control fights.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeakFighter {
    /// Never strikes: holds where it stands, fortified when it can.
    Passive,
    /// A uniformly random legal command of the unit (`unit_actions`), drawn
    /// from the round, the unit and its state: the same game gives the same
    /// draws.
    Random,
    /// Strikes the nearest enemy it can strike, whatever the odds; else
    /// walks toward the nearest enemy.
    Reckless,
}

/// A land military unit of the player: the units a control commands.
fn commands(rules: &CompiledRuleset, unit: &OwnUnit) -> bool {
    role_of(rules, unit.kind) == AiRole::Military && rules.units.get(unit.kind.0 as usize).is_some_and(|d| d.domain == Domain::Land)
}

/// A strike of a unit a control commands, decided outside the control (the
/// start-of-turn shots and set-ups): dropped from the opening.
pub fn strips(rules: &CompiledRuleset, own: &[OwnUnit], command: &Command) -> bool {
    let unit = match command {
        Command::RangedAttack { unit, .. } | Command::SetUpRangedAttack { unit } => *unit,
        Command::AttackUnit { attacker, .. } | Command::AttackCity { attacker, .. } => *attacker,
        _ => return false,
    };
    own.iter().find(|u| u.handle == unit).is_some_and(|u| commands(rules, u))
}

/// The control's decision for the asked unit; None when it is not a land
/// military unit or sees no enemy at war (the planners decide then).
pub fn decide(fighter: WeakFighter, view: &UnitView<'_, '_>) -> Option<Decision> {
    let local = view.local;
    let rules = local.view.rules();
    if !commands(rules, &local.unit) {
        return None;
    }
    let foes = foes(view);
    if foes.is_empty() {
        return None;
    }
    let decision = match fighter {
        WeakFighter::Passive => hold(view, Reason::Fortify),
        WeakFighter::Random => random(view),
        WeakFighter::Reckless => reckless(view, &foes),
    };
    Some(decision)
}

/// The tiles of the enemy units and cities in sight, nearest first.
fn foes(view: &UnitView<'_, '_>) -> Vec<TileId> {
    let local = view.local;
    let at_war = |owner| local.relation(owner).is_some_and(|r| r.at_war);
    let units = local.foreign_units.iter().filter(|u| at_war(u.owner)).map(|u| u.tile);
    let cities = local.foreign_cities.iter().filter(|c| at_war(c.owner)).map(|c| c.tile);
    let mut tiles: Vec<TileId> = units.chain(cities).collect();
    tiles.sort_by_key(|t| (distance(view, local.unit.tile, *t), *t));
    tiles.dedup();
    tiles
}

fn distance(view: &UnitView<'_, '_>, a: TileId, b: TileId) -> i32 {
    view.local.view.dimensions().distance(a, b).unwrap_or(i32::MAX)
}

/// A command that leaves the fight rather than fights: a gift, an upgrade
/// (the advisor's, with the gold reserve) and the civilian works.
fn fighting(command: &Command) -> bool {
    !matches!(command, Command::GiftUnit { .. } | Command::UpgradeUnit { .. } | Command::BuildImprovement { .. } | Command::BuildRoad { .. } | Command::ChopForest { .. } | Command::BuildCitadel { .. })
}

/// A uniformly random legal command of the unit; holds when it has none.
fn random(view: &UnitView<'_, '_>) -> Decision {
    let unit = view.local.unit;
    let actions: Vec<Command> = unit_actions(view.local).into_iter().filter(fighting).collect();
    if actions.is_empty() {
        return hold(view, Reason::Idle);
    }
    let key = (u64::from(view.local.round) << 40) ^ (unit.id.0 << 8) ^ u64::from(unit.moves_left) ^ (u64::from(unit.hp) << 24) ^ (u64::from(unit.tile.0) << 32);
    let command = actions[(mix(key) % actions.len() as u64) as usize].clone();
    Decision::act(Actor::Unit(unit.id), reason_of(&command), command)
}

/// SplitMix64's finaliser.
fn mix(seed: u64) -> u64 {
    let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn reason_of(command: &Command) -> Reason {
    match command {
        Command::AttackCity { .. } => Reason::AttackCity,
        Command::RangedAttack { .. } | Command::AttackUnit { .. } | Command::SetUpRangedAttack { .. } => Reason::AttackUnit,
        Command::Fortify { .. } => Reason::Fortify,
        _ => Reason::Advance,
    }
}

/// The legal strike on the nearest foe (a siege unit sets up first); else a
/// step toward the nearest foe: the reachable tile nearest it, a melee unit
/// keeping a move to strike from there when it can; else it holds.
fn reckless(view: &UnitView<'_, '_>, foes: &[TileId]) -> Decision {
    let unit = view.local.unit;
    let actor = Actor::Unit(unit.id);
    let actions = unit_actions(view.local);
    let target_of = |command: &Command| -> Option<TileId> {
        match command {
            Command::RangedAttack { target, .. } => Some(*target),
            Command::AttackUnit { defender, .. } => view.local.foreign_units.iter().find(|u| u.handle == *defender).map(|u| u.tile),
            Command::AttackCity { city, .. } => view.local.foreign_cities.iter().find(|c| c.handle == *city).map(|c| c.tile),
            _ => None,
        }
    };
    let range = crate::ai::combat::ranged_range(view.local.view.rules(), unit.kind);
    // A ranged unit shoots (the rules also let it charge), a melee unit charges.
    let own_kind = |c: &&Command| matches!(c, Command::RangedAttack { .. }) == range.is_some();
    let strike = actions
        .iter()
        .filter(own_kind)
        .filter_map(|c| Some((foes.iter().position(|f| Some(*f) == target_of(c))?, c.clone())))
        .min_by_key(|(rank, _)| *rank)
        .map(|(_, c)| c);
    if let Some(command) = strike {
        return Decision::act(actor, reason_of(&command), command);
    }
    let nearest = foes[0];
    if range.is_some_and(|r| distance(view, unit.tile, nearest) <= r) {
        if let Some(set_up) = actions.iter().find(|c| matches!(c, Command::SetUpRangedAttack { .. })) {
            return Decision::act(actor, Reason::AttackUnit, set_up.clone());
        }
        return hold(view, Reason::AttackUnit);
    }
    let reach = range.unwrap_or(1);
    let moves_left: Vec<(TileId, u16)> = view.local.view.own_reachable(unit.handle);
    let left_on = |tile: TileId| moves_left.iter().find(|(t, _)| *t == tile).map_or(0, |(_, l)| *l);
    let step = actions
        .iter()
        .filter_map(|c| match c {
            Command::MoveUnit { destination, .. } => Some(*destination),
            _ => None,
        })
        .min_by_key(|t| {
            let d = distance(view, *t, nearest);
            // In reach with a move left to strike first, then nearest.
            (!(d <= reach && left_on(*t) > 0), d, std::cmp::Reverse(left_on(*t)), *t)
        });
    match step {
        Some(tile) if distance(view, tile, nearest) < distance(view, unit.tile, nearest) => {
            Decision::act(actor, Reason::Advance, Command::MoveUnit { unit: unit.handle, destination: tile })
        }
        _ => hold(view, Reason::Advance),
    }
}

#[cfg(test)]
mod tests {
    use super::WeakFighter;
    use crate::ai::variant::Variant;

    /// The three controls are listed by name and play `standard` but for
    /// their fighting; no other variant fights as a control.
    #[test]
    fn the_controls_are_the_standard_but_for_the_fight() {
        let controls = [("fight-passive", WeakFighter::Passive), ("fight-random", WeakFighter::Random), ("fight-reckless", WeakFighter::Reckless)];
        for (name, fighter) in controls {
            let variant = Variant::from_name(name).unwrap();
            assert_eq!(variant.weak_fighter(), Some(fighter));
            assert_eq!(variant.price_model(), Variant::Standard.price_model());
            assert_eq!(variant.science_pricing(), Variant::Standard.science_pricing());
            assert_eq!(variant.peace_making(), Variant::Standard.peace_making());
            assert!(!variant.tactical() && !variant.danger_net());
        }
        assert_eq!(Variant::ALL.iter().filter(|v| v.weak_fighter().is_some()).count(), controls.len());
    }
}
