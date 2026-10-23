//! Air combat (classic/docs/classic/rules.md): interception of air
//! strikes, the target's answer, air sweeps, rebasing, interception duty;
//! nuclear weapons (§2.8).

use crate::combat::damage::{
    air_defense_damage, interception_damage, melee_damage, MeleeSide, Roll,
};
use crate::combat::exchange::{AirRisk, AttackPlan, Defender};
use crate::combat::strength::{defense_strength, ranged_strength, Fighter};
use crate::command::CommandError;
use crate::ids::{CityHandle, PlayerId, TileId, UnitHandle};
use crate::rules::content::Domain;
use crate::rules::effects::{CombatContext, UnitAbility, UnitStat};
use crate::rules::CompiledRuleset;
use crate::transaction::transaction::WorldTransaction;
use crate::world::state::AuthWorld;
use crate::world::unit::{Stance, UNIT_MAX_HP};

/// Rebase range of an air unit, in percent of its range. Unciv:
/// MapUnit.getAirUnitMovementRange, twice the range; equal.
pub const REBASE_RANGE_PERCENT: i32 = 200;
/// Air slots of a city before buildings add more. Unciv: ModConstants,
/// cityAirUnitCapacity, 6; equal.
pub const CITY_AIR_UNIT_CAPACITY: i64 = 6;

/// Why an air sweep or an interception order is refused (the interface words each).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AirOrderBlock {
    /// The unit has no AirSweep ability.
    CannotSweep,
    /// No moves or attacks left to sweep with.
    NoAttacksLeft,
    /// The sweep target is beyond the unit's range.
    OutOfRange,
    /// The unit has no interception range.
    CannotIntercept,
    /// The unit already waits to intercept.
    AlreadyIntercepting,
}

impl AirOrderBlock {
    pub const ALL: [AirOrderBlock; 5] = [
        AirOrderBlock::CannotSweep,
        AirOrderBlock::NoAttacksLeft,
        AirOrderBlock::OutOfRange,
        AirOrderBlock::CannotIntercept,
        AirOrderBlock::AlreadyIntercepting,
    ];

    pub fn reason(self) -> &'static str {
        match self {
            AirOrderBlock::CannotSweep => "cannot sweep",
            AirOrderBlock::NoAttacksLeft => "no attacks left",
            AirOrderBlock::OutOfRange => "out of range",
            AirOrderBlock::CannotIntercept => "cannot intercept",
            AirOrderBlock::AlreadyIntercepting => "already intercepting",
        }
    }
}

impl From<AirOrderBlock> for CommandError {
    fn from(block: AirOrderBlock) -> Self {
        CommandError::RuleViolation(block.reason().into())
    }
}

/// An interceptor able to intercept at `tile` against `attacker_owner`, with
/// its current chance (percent).
fn best_interceptor(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    attacker_owner: PlayerId,
    tile: TileId,
    exclude: Option<UnitHandle>,
) -> Option<(UnitHandle, i64)> {
    best_interceptor_seen(auth, rules, attacker_owner, tile, exclude, &|_| true)
}

/// `best_interceptor` among the interceptors `seen` accepts (a player's
/// forecast counts only those it sees).
fn best_interceptor_seen(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    attacker_owner: PlayerId,
    tile: TileId,
    exclude: Option<UnitHandle>,
    seen: &dyn Fn(&crate::world::unit::UnitCore) -> bool,
) -> Option<(UnitHandle, i64)> {
    let dims = auth.map().dimensions;
    let mut best: Option<(i64, UnitHandle)> = None;
    for (handle, core) in auth.units().iter_handles() {
        if Some(handle) == exclude || !auth.diplomacy().at_war(core.owner, attacker_owner) || !seen(core) {
            continue;
        }
        let Some(def) = rules.units.get(core.kind.0 as usize) else {
            continue;
        };
        let range = i32::from(def.combat.air_intercept_range);
        if range == 0 || dims.distance(core.tile, tile).unwrap_or(i32::MAX) > range {
            continue;
        }
        let mods = crate::effects::unit::unit_modifiers(auth, rules, core);
        let max_chance = i64::from(mods.stat(UnitStat::InterceptChance).max(0));
        if max_chance == 0 {
            continue;
        }
        let per_turn = 1 + mods.stat(UnitStat::InterceptionsPerTurn).max(0) as u8;
        if core.interceptions_made >= per_turn {
            continue;
        }
        let air = def.domain == Domain::Air;
        if air && (core.stance != Stance::Intercept || core.has_moved()) {
            continue;
        }
        let chance = if air {
            max_chance * i64::from(core.hp) / i64::from(UNIT_MAX_HP)
        } else {
            max_chance
        };
        if best.map(|(c, _)| chance > c).unwrap_or(true) {
            best = Some((chance, handle));
        }
    }
    best.map(|(c, h)| (h, c))
}

/// Strength an interceptor opposes (§2.7): its ranged strength (not ships)
/// or its defense, raised by its Interception percent.
fn interceptor_strength(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    interceptor: &Fighter<'_>,
    attacker: &Fighter<'_>,
) -> i64 {
    let base = if interceptor.base_ranged() > 0 && interceptor.domain() != Domain::Sea {
        ranged_strength(
            auth,
            rules,
            interceptor,
            Some(attacker),
            None,
            interceptor.tile(),
            false,
        )
    } else {
        defense_strength(auth, rules, interceptor, Some(attacker), true)
    };
    base * (100 + i64::from(interceptor.mods.percent(CombatContext::Interception))) / 100
}

/// The forecast of an air strike by `plan`: the answer of the target and
/// the interception by the best interceptor the attacker's owner sees, with
/// the expected roll (the formulas of `air_answer` and `intercept`).
pub fn strike_risk(
    view: crate::world::state::GameView<'_>,
    rules: &CompiledRuleset,
    plan: &AttackPlan,
) -> AirRisk {
    let auth = view.auth;
    let Some(attacker) = Fighter::of(auth, rules, plan.attacker) else {
        return AirRisk::default();
    };
    let answer = match plan.defender {
        Defender::City(city) => auth
            .cities()
            .get(city)
            .map(|core| {
                let s_city = crate::combat::city::city_strength(auth, rules, core);
                let s_att = ranged_strength(auth, rules, &attacker, None, Some(city), plan.target_tile, true);
                air_defense_damage(s_city, s_att, 0, true, Roll::Expected)
            })
            .unwrap_or(0),
        Defender::Unit(unit) => Fighter::of(auth, rules, unit)
            .map(|target| {
                let s_def = crate::combat::strength::defense_vs_ranged(auth, rules, &target, Some(&attacker));
                let s_att = ranged_strength(auth, rules, &attacker, Some(&target), None, plan.target_tile, true);
                air_defense_damage(s_def, s_att, target.core.damage(), false, Roll::Expected)
            })
            .unwrap_or(0),
        Defender::Civilian(_) => 0,
    };
    let defender_unit = match plan.defender {
        Defender::Unit(u) | Defender::Civilian(u) => Some(u),
        Defender::City(_) => None,
    };
    let owner = plan.attacker_owner;
    let seen = |core: &crate::world::unit::UnitCore| crate::knowledge::view::unit_seen_by(auth, view.derived, rules, owner, core);
    let Some((interceptor, chance)) = best_interceptor_seen(auth, rules, owner, plan.target_tile, defender_unit, &seen) else {
        return AirRisk { answer, interception: 0, chance: 0 };
    };
    let Some(guard) = Fighter::of(auth, rules, interceptor) else {
        return AirRisk { answer, interception: 0, chance: 0 };
    };
    let evasion = i64::from(attacker.mods.stat(UnitStat::EvasionPercent)).clamp(0, 100);
    let s_int = interceptor_strength(auth, rules, &guard, &attacker);
    let s_att = ranged_strength(auth, rules, &attacker, Some(&guard), None, guard.tile(), true);
    let taken = i64::from(attacker.mods.stat(UnitStat::InterceptionDamageTakenPercent));
    let interception = interception_damage(s_int, s_att, guard.core.damage(), taken, Roll::Expected);
    AirRisk { answer, interception, chance: (chance * (100 - evasion) / 100).clamp(0, 100) as u8 }
}

impl<'a> WorldTransaction<'a> {
    /// Interception of an air strike (§2.7): evasion roll, interception
    /// roll, damage to the attacker (applied with the target's answer).
    pub(crate) fn intercept(&mut self, plan: &AttackPlan, rules: &CompiledRuleset) -> u16 {
        let defender_unit = match plan.defender {
            Defender::Unit(u) | Defender::Civilian(u) => Some(u),
            Defender::City(_) => None,
        };
        let Some((interceptor, chance)) = best_interceptor(
            self.world,
            rules,
            plan.attacker_owner,
            plan.target_tile,
            defender_unit,
        ) else {
            return 0;
        };
        let Some(attacker) = Fighter::of(self.world, rules, plan.attacker) else {
            return 0;
        };
        let evasion = i64::from(attacker.mods.stat(UnitStat::EvasionPercent));
        if (self.world.rng.combat.below(100) as i64) < evasion {
            return 0;
        }
        if (self.world.rng.combat.below(100) as i64) >= chance {
            return 0;
        }
        let Some(guard) = Fighter::of(self.world, rules, interceptor) else {
            return 0;
        };
        let s_int = interceptor_strength(self.world, rules, &guard, &attacker);
        let s_att = ranged_strength(
            self.world,
            rules,
            &attacker,
            Some(&guard),
            None,
            guard.tile(),
            true,
        );
        let taken = i64::from(attacker.mods.stat(UnitStat::InterceptionDamageTakenPercent));
        let roll = self.roll();
        let damage = interception_damage(s_int, s_att, guard.core.damage(), taken, roll);
        if let Some(core) = self.world.units.get_mut_core(interceptor) {
            core.interceptions_made = core.interceptions_made.saturating_add(1);
        }
        // Experience only for a hit (Unciv: AirInterception.kt).
        if damage > 0 {
            let xp = crate::combat::experience::EXPERIENCE_DEFENDING_AIR_SWEEP_GROUND;
            let tile = guard.tile();
            self.combat_experience(interceptor, xp, plan.attacker_owner, tile, rules);
        }
        damage
    }

    /// After an air strike: the target's answer plus the interception
    /// damage hit the aircraft (§2.7).
    pub(crate) fn air_aftermath(
        &mut self,
        plan: &AttackPlan,
        interception: u16,
        target_alive: bool,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        let answer = if target_alive {
            self.air_answer(plan, rules)
        } else {
            0
        };
        let total = answer.saturating_add(interception);
        if total > 0 && self.world.units.get(plan.attacker).is_some() {
            self.strike_unit(plan.attacker, total)?;
        }
        Ok(())
    }

    fn air_answer(&mut self, plan: &AttackPlan, rules: &CompiledRuleset) -> u16 {
        let Some(attacker) = Fighter::of(self.world, rules, plan.attacker) else {
            return 0;
        };
        let roll = self.roll();
        match plan.defender {
            Defender::City(city) => {
                let Some(core) = self.world.cities.get(city) else {
                    return 0;
                };
                let s_city = crate::combat::city::city_strength(self.world, rules, core);
                let s_att = ranged_strength(
                    self.world,
                    rules,
                    &attacker,
                    None,
                    Some(city),
                    plan.target_tile,
                    true,
                );
                air_defense_damage(s_city, s_att, 0, true, roll)
            }
            Defender::Unit(unit) => {
                let Some(target) = Fighter::of(self.world, rules, unit) else {
                    return 0;
                };
                let s_def = crate::combat::strength::defense_vs_ranged(
                    self.world,
                    rules,
                    &target,
                    Some(&attacker),
                );
                let s_att = ranged_strength(
                    self.world,
                    rules,
                    &attacker,
                    Some(&target),
                    None,
                    plan.target_tile,
                    true,
                );
                air_defense_damage(s_def, s_att, target.core.damage(), false, roll)
            }
            Defender::Civilian(_) => 0,
        }
    }

    /// Interception duty (stance; the aircraft must not move this turn).
    pub(crate) fn set_intercept(&mut self, unit: UnitHandle) -> Result<(), CommandError> {
        let core = self
            .world
            .units
            .get_mut_core(unit)
            .ok_or(CommandError::UnknownUnit)?;
        core.stance = Stance::Intercept;
        core.goal = None;
        self.journal
            .unit_changes
            .push(crate::transaction::journal::UnitChange::StanceChanged { unit });
        Ok(())
    }

    /// Rebase to another own city or onto a carrier (§9): the move ends the
    /// turn.
    pub(crate) fn rebase(
        &mut self,
        unit: UnitHandle,
        to: TileId,
        carrier: Option<crate::ids::EntityId>,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        self.relocate_unit(unit, to, rules);
        self.set_carrier(unit, carrier);
        let core = self
            .world
            .units
            .get_mut_core(unit)
            .ok_or(CommandError::UnknownUnit)?;
        core.moves_left = 0;
        Ok(())
    }

    /// Air sweep (§2.7): a dogfight with the best air interceptor, or the
    /// ground interceptors spend their interception harmlessly; without
    /// interceptors the sweep only spends the turn.
    pub(crate) fn air_sweep(
        &mut self,
        unit: UnitHandle,
        target: TileId,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        let owner = self
            .world
            .units
            .get(unit)
            .ok_or(CommandError::UnknownUnit)?
            .core
            .owner;
        if let Some((guard, _)) = best_interceptor(self.world, rules, owner, target, None) {
            let air = self
                .world
                .units
                .get(guard)
                .and_then(|u| rules.units.get(u.core.kind.0 as usize))
                .is_some_and(|d| d.domain == Domain::Air);
            if air {
                self.dogfight(unit, guard, rules)?;
            } else if let Some(core) = self.world.units.get_mut_core(guard) {
                core.interceptions_made = core.interceptions_made.saturating_add(1);
            }
        }
        if let Some(core) = self.world.units.get_mut_core(unit) {
            core.moves_left = 0;
            core.attacks_made = core.attacks_made.saturating_add(1);
        }
        Ok(())
    }

    /// Fighter against fighter: melee damage both ways, the sweeper raised
    /// by its AirSweep percent, the mutual-kill rule.
    fn dogfight(
        &mut self,
        sweeper: UnitHandle,
        guard: UnitHandle,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        let (Some(a), Some(d)) = (
            Fighter::of(self.world, rules, sweeper),
            Fighter::of(self.world, rules, guard),
        ) else {
            return Ok(());
        };
        let s_a = ranged_strength(self.world, rules, &a, Some(&d), None, d.tile(), true)
            * (100 + i64::from(a.mods.percent(CombatContext::AirSweep)))
            / 100;
        let s_d = interceptor_strength(self.world, rules, &d, &a);
        let ra = self.roll();
        let rd = self.roll();
        let to_d = melee_damage(s_a, s_d, a.core.damage(), 0, ra, MeleeSide::UnitVsUnit);
        let to_a = melee_damage(s_d, s_a, d.core.damage(), 0, rd, MeleeSide::UnitVsUnit);
        let (to_d, to_a) =
            crate::combat::damage::resolve_mutual_kill(to_d, d.core.hp, to_a, a.core.hp);
        let (owner_a, owner_d) = (a.owner(), d.owner());
        let tile = d.tile();
        let out_d = self.strike_unit(guard, to_d)?;
        let out_a = self.strike_unit(sweeper, to_a)?;
        let xp = crate::combat::experience::EXPERIENCE_ATTACKING_AIR_SWEEP;
        if out_a == crate::systems::combat::StrikeOutcome::Damaged {
            self.combat_experience(sweeper, xp, owner_d, tile, rules);
        }
        if out_d == crate::systems::combat::StrikeOutcome::Damaged {
            self.combat_experience(guard, xp, owner_a, tile, rules);
            if let Some(core) = self.world.units.get_mut_core(guard) {
                core.interceptions_made = core.interceptions_made.saturating_add(1);
            }
        }
        Ok(())
    }
}

/// Aircraft a city can hold (§8: 6 + the buildings' AirCapacity).
pub fn city_air_capacity(auth: &AuthWorld, rules: &CompiledRuleset, city: CityHandle) -> i64 {
    let Some(core) = auth.cities().get(city) else {
        return 0;
    };
    CITY_AIR_UNIT_CAPACITY + crate::combat::city::city_combat_mods(auth, rules, core).air_capacity
}

/// Aircraft of the city's owner based in it.
pub fn aircraft_in_city(auth: &AuthWorld, rules: &CompiledRuleset, city: CityHandle) -> i64 {
    let Some(core) = auth.cities().get(city) else {
        return 0;
    };
    auth.indexes()
        .units_on(core.tile)
        .iter()
        .filter_map(|u| {
            let view = auth.units().get(*u)?;
            let own = view.core.owner == core.owner && crate::systems::cargo::carrier_of(auth, *u).is_none();
            let def = rules.units.get(view.core.kind.0 as usize)?;
            (own && def.domain == Domain::Air).then(|| air_slots(def))
        })
        .sum()
}

/// Air slots a unit takes in a city (1 without a declared number).
pub fn air_slots(def: &crate::rules::compiled::UnitDefinition) -> i64 {
    let slots = def.module.abilities.iter().find_map(|a| match a {
        crate::rules::content::UnitAbility::AirSlots(n) => Some(i64::from(*n)),
        _ => None,
    });
    slots.unwrap_or(1)
}

/// Does the unit sweep (AirSweep ability)?
pub fn can_air_sweep(auth: &AuthWorld, rules: &CompiledRuleset, unit: UnitHandle) -> bool {
    let Some(view) = auth.units().get(unit) else {
        return false;
    };
    crate::effects::unit::unit_modifiers(auth, rules, view.core).has(UnitAbility::AirSweep)
}
