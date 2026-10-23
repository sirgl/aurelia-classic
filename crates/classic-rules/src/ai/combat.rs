//! Minimal combat decisions over `AIObservation` (M4 track C): which
//! attacks to make, city strikes, promotions, upgrades and what to do with
//! captured cities. Every number comes from the same public previews the
//! interface shows (`PlayerView::attack_preview`); nothing hidden is read.
//! The AI framework (planner, driver) decides when to call these.

use crate::knowledge::public::PlayerQueries;
use crate::combat::exchange::{CombatPreview, Verdict};
use crate::command::{CityDecision, Command};
use crate::ids::{PlayerId, PromotionId, TileId, UnitHandle};
use crate::knowledge::observation::{AIObservation, OwnUnit};
use crate::rules::CompiledRuleset;
use crate::world::city::CityStatus;

/// A possible attack and its value for the attacker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttackChoice {
    pub unit: UnitHandle,
    pub target: TileId,
    pub score: i32,
    pub preview: CombatPreview,
}

/// Value of an attack: share of the defender's HP taken minus 60 % of the
/// share of the attacker's HP lost; +100 for a kill on the lowest roll.
pub fn attack_score(preview: &CombatPreview) -> i32 {
    let dealt = i32::from(preview.to_defender.1) * 100 / i32::from(preview.defender_hp.max(1));
    let taken = i32::from(preview.to_attacker.1) * 100 / i32::from(preview.attacker_hp.max(1));
    let kill = if preview.verdict == Verdict::DecisiveVictory {
        100
    } else {
        0
    };
    dealt - taken * 60 / 100 + kill
}

/// Tiles of enemies the player is at war with and sees now.
fn enemy_targets(obs: &AIObservation<'_>) -> Vec<TileId> {
    let at_war = |owner: PlayerId| obs.relation(owner).is_some_and(|r| r.at_war);
    let mut tiles: Vec<TileId> = obs
        .visible_foreign_units
        .iter()
        .filter(|u| at_war(u.owner))
        .map(|u| u.tile)
        .chain(
            obs.visible_foreign_cities
                .iter()
                .filter(|c| at_war(c.owner))
                .map(|c| c.tile),
        )
        .collect();
    tiles.sort_by_key(|t| t.0);
    tiles.dedup();
    tiles
}

fn distance(obs: &AIObservation<'_>, a: TileId, b: TileId) -> i32 {
    let dims = crate::math::MapDimensions {
        width: obs.map_width,
        height: obs.map_height,
    };
    dims.distance(a, b).unwrap_or(i32::MAX)
}

/// The best attack of one unit (None: nothing worth it). Melee attacks
/// that are likely lost are skipped; ranged attacks cost nothing.
pub fn best_attack(
    obs: &AIObservation<'_>,
    rules: &CompiledRuleset,
    unit: &OwnUnit,
) -> Option<AttackChoice> {
    if unit.moves_left == 0 || rules.is_civilian(unit.kind) {
        return None;
    }
    let reach = rules
        .units
        .get(unit.kind.0 as usize)
        .map(|d| i32::from(d.range).max(1))
        .unwrap_or(1);
    let mut best: Option<AttackChoice> = None;
    for target in enemy_targets(obs) {
        if distance(obs, unit.tile, target) > reach.max(1) + 8 {
            continue;
        }
        let Some(preview) = obs.view.attack_preview(unit.handle, target) else {
            continue;
        };
        let risky = matches!(
            preview.verdict,
            Verdict::MajorDefeat | Verdict::DecisiveDefeat
        );
        if risky {
            continue;
        }
        let score = attack_score(&preview);
        if best.map(|b| score > b.score).unwrap_or(true) {
            best = Some(AttackChoice {
                unit: unit.handle,
                target,
                score,
                preview,
            });
        }
    }
    best.filter(|b| b.score > 0)
}

/// Ranged attack orders (archers, siege, ships, aircraft) for every own
/// ranged unit with a worthwhile shot, best first (shots soften targets
/// for the charges of `melee_orders`).
pub fn ranged_orders(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Vec<Command> {
    let ranged = |u: &&OwnUnit| {
        rules
            .units
            .get(u.kind.0 as usize)
            .is_some_and(|d| d.ranged_strength > 0 && d.range > 0)
    };
    let mut choices: Vec<AttackChoice> = obs
        .own_units
        .iter()
        .filter(ranged)
        .filter_map(|u| best_attack(obs, rules, u))
        .collect();
    choices.sort_by_key(|c| (-c.score, c.unit.index));
    choices
        .into_iter()
        .map(|c| Command::RangedAttack {
            unit: c.unit,
            target: c.target,
        })
        .collect()
}

/// Melee orders (attack the tile's defender) for the units whose best
/// attack is a charge. Use after `ranged_orders`.
pub fn melee_orders(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Vec<Command> {
    let mut out = Vec::new();
    for unit in &obs.own_units {
        let ranged = rules
            .units
            .get(unit.kind.0 as usize)
            .is_some_and(|d| d.ranged_strength > 0 && d.range > 0);
        if ranged {
            continue;
        }
        let Some(choice) = best_attack(obs, rules, unit) else {
            continue;
        };
        if let Some(city) = obs
            .visible_foreign_cities
            .iter()
            .find(|c| c.tile == choice.target)
        {
            out.push(Command::AttackCity {
                attacker: unit.handle,
                city: city.handle,
            });
        } else if let Some(foe) = obs
            .visible_foreign_units
            .iter()
            .find(|u| u.tile == choice.target)
        {
            out.push(Command::AttackUnit {
                attacker: unit.handle,
                defender: foe.handle,
            });
        }
    }
    out
}

/// Each own city strikes the visible enemy it hurts most.
pub fn city_strikes(obs: &AIObservation<'_>) -> Vec<Command> {
    let mut out = Vec::new();
    let targets = enemy_targets(obs);
    for city in &obs.own_cities {
        let best = targets
            .iter()
            .filter_map(|t| {
                obs.view
                    .city_strike_damage(city.handle, *t)
                    .map(|d| (d, *t))
            })
            .max_by_key(|(d, t)| (*d, std::cmp::Reverse(t.0)));
        if let Some((_, target)) = best {
            out.push(Command::CityStrike {
                city: city.handle,
                target,
            });
        }
    }
    out
}

/// Promotion choices: heal a badly wounded unit when it can, else the
/// first available promotion in content order (deterministic).
pub fn promotion_choices(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Vec<Command> {
    let mut out = Vec::new();
    for unit in &obs.own_units {
        let available = obs.view.promotions_available(unit.handle);
        if available.is_empty() {
            continue;
        }
        let heals = |p: &PromotionId| rules.promotions[p.0 as usize].heals_instantly();
        let heal = available.iter().copied().find(heals);
        let pick = match heal {
            Some(p) if unit.hp < 50 => p,
            _ => *available.iter().find(|p| !heals(p)).unwrap_or(&available[0]),
        };
        out.push(Command::Promote {
            unit: unit.handle,
            promotion: pick,
        });
    }
    out
}

/// Upgrades while the treasury stays above `reserve` gold and the
/// strategic resources of this turn's upgrades last (each offer counts the
/// empire's free copies alone).
pub fn upgrade_orders(obs: &AIObservation<'_>, rules: &CompiledRuleset, reserve: u32) -> Vec<Command> {
    let mut gold = obs.economy.treasury;
    let mut taken: Vec<(crate::ids::ResourceId, i32)> = Vec::new();
    let mut out = Vec::new();
    for unit in &obs.own_units {
        let Some((to, price)) = obs.view.upgrade_offer(unit.handle) else {
            continue;
        };
        if gold < price + reserve {
            continue;
        }
        let extra = extra_resources(rules, unit.kind, to);
        let fits = extra.iter().all(|(r, n)| {
            let already: i32 = taken.iter().filter(|(t, _)| t == r).map(|(_, k)| *k).sum();
            obs.economy.resources.available(*r) - already >= *n
        });
        if !fits {
            continue;
        }
        gold -= price;
        taken.extend(extra);
        out.push(Command::UpgradeUnit { unit: unit.handle });
    }
    out
}

/// Copies the upgrade `from → to` takes beyond what the old unit holds.
fn extra_resources(
    rules: &CompiledRuleset,
    from: crate::ids::UnitKindId,
    to: crate::ids::UnitKindId,
) -> Vec<(crate::ids::ResourceId, i32)> {
    let held = &rules.units[from.0 as usize].required_resources;
    let mut out = Vec::new();
    for (r, n) in &rules.units[to.0 as usize].required_resources {
        let freed: u16 = held.iter().filter(|(h, _)| h == r).map(|(_, k)| *k).sum();
        let extra = i32::from(*n) - i32::from(freed);
        if extra > 0 {
            out.push((*r, extra));
        }
    }
    out
}

/// Citizens of a captured city the AI annexes; a smaller one is a puppet.
pub const ANNEX_POPULATION: u32 = 4;

/// Captured cities: keep cities of `ANNEX_POPULATION` citizens and more
/// (annex), puppet the rest; raze nothing (warmonger memory and lost land
/// outweigh it). A civilization that may not annex (Venice) puppets every
/// city.
pub fn conquest_decisions(obs: &AIObservation<'_>) -> Vec<Command> {
    obs.own_cities
        .iter()
        .filter(|c| c.status == CityStatus::AwaitingDecision)
        .map(|c| Command::DecideCity {
            player: obs.player,
            city: c.handle,
            decision: if c.population >= ANNEX_POPULATION && obs.economy.may_annex {
                CityDecision::Annex
            } else {
                CityDecision::Puppet
            },
        })
        .collect()
}

/// Siege units with a visible enemy in range set up (they shoot after).
pub fn stance_orders(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Vec<Command> {
    let targets = enemy_targets(obs);
    obs.own_units
        .iter()
        .filter(|u| u.moves_left > 0 && obs.view.must_set_up_first(u.handle))
        .filter(|u| {
            let range = ranged_range(rules, u.kind).unwrap_or(0);
            targets.iter().any(|t| distance(obs, u.tile, *t) <= range)
        })
        .map(|u| Command::SetUpRangedAttack { unit: u.handle })
        .collect()
}

/// Range of a ranged unit kind (None: it fights in melee).
pub fn ranged_range(rules: &CompiledRuleset, kind: crate::ids::UnitKindId) -> Option<i32> {
    let def = rules.units.get(kind.0 as usize)?;
    (def.ranged_strength > 0 && def.range > 0).then_some(i32::from(def.range))
}

/// The best shot of the player's ranged `unit` at one of `targets` by the
/// public preview (None: no target worth a shot).
pub fn best_shot(
    view: &crate::knowledge::view::PlayerView<'_>,
    unit: UnitHandle,
    targets: &[TileId],
) -> Option<(TileId, i32)> {
    targets
        .iter()
        .filter_map(|t| {
            view.attack_preview(unit, *t)
                .map(|p| (*t, attack_score(&p)))
        })
        .filter(|(_, score)| *score > 0)
        .max_by_key(|(t, score)| (*score, std::cmp::Reverse(t.0)))
}

/// Gold the military advisor keeps when it upgrades units.
pub const UPGRADE_RESERVE: u32 = 150;

/// The military system's advisor (`ai::system`): before the units move,
/// city strikes, siege set-up and ranged shots; at the end of the turn,
/// captured cities, promotions and upgrades.
#[derive(Debug, Clone, Copy, Default)]
pub struct MilitaryAdvisor;

impl crate::ai::system::SystemAdvisor for MilitaryAdvisor {
    fn start_of_turn(
        &self,
        input: &crate::ai::system::AdvisorInput<'_, '_>,
    ) -> Vec<crate::ai::system::Proposal> {
        let obs = input.observation;
        let mut out = proposals(30, city_strikes(obs));
        out.extend(proposals(25, stance_orders(obs, input.rules)));
        let mut shots = ranged_orders(obs, input.rules);
        if input.brief.air {
            // The aircraft of a variant with an air arm are commanded by it
            // (`ai::air`: the answer of the target, the interceptors, the
            // base); the shots here would ignore them.
            shots.retain(|c| !matches!(c, Command::RangedAttack { unit, .. } if obs.own_units.iter().any(|u| u.handle == *unit && crate::ai::air::wing(input.rules, u.kind).is_some())));
        }
        if input.brief.strikes {
            // Missiles and nuclear weapons are used up by a shot: they are
            // commanded by `ai::strikes`, which weighs the shot.
            shots.retain(|c| !matches!(c, Command::RangedAttack { unit, .. } if obs.own_units.iter().any(|u| u.handle == *unit && crate::ai::strikes::weapon(input.rules, u.kind).is_some())));
        }
        out.extend(proposals(20, shots));
        out
    }

    fn end_of_turn(
        &self,
        input: &crate::ai::system::AdvisorInput<'_, '_>,
    ) -> Vec<crate::ai::system::Proposal> {
        let obs = input.observation;
        let mut out = proposals(30, conquest_decisions(obs));
        out.extend(proposals(10, promotion_choices(obs, input.rules)));
        out.extend(proposals(5, upgrade_orders(obs, input.rules, UPGRADE_RESERVE)));
        out
    }
}

/// Commands as proposals of one priority.
pub fn proposals(priority: i32, commands: Vec<Command>) -> Vec<crate::ai::system::Proposal> {
    commands
        .into_iter()
        .map(|command| crate::ai::system::Proposal { priority, command })
        .collect()
}
