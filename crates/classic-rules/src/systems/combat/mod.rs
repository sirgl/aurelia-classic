//! Combat system (classic/docs/classic/rules.md): melee and ranged
//! attacks, city strikes and captures, experience and promotions, unit
//! upgrades, healing. The numbers come from `crate::combat` (one read
//! model for commit, preview and AI); this module applies them.
//!
//! Rolls happen in the commit (validate stays pure, §11), from the combat
//! stream in a fixed order: the attacker's damage roll, then the
//! defender's; then capture, withdrawal and promotion rolls.
//!
//! Journal: UnitChange::Damaged / Destroyed / Created / Moved / Promoted,
//! CityChange::OwnerChanged / Damaged / Removed, TileChange::OwnerChanged.
//! Fires OnUnitKilled.

pub(crate) mod actions;
pub(crate) mod air;
pub(crate) mod city;
pub(crate) mod experience;
pub(crate) mod heal;
pub(crate) mod upgrade;
pub(crate) mod validate;

use crate::classic::effects::{ClassicUnitAbility, ClassicUnitStat};
use crate::combat::damage::{
    resolve_mutual_kill, Roll, MELEE_EQUAL_STRENGTH_RANDOM_DAMAGE,
};
use crate::combat::exchange::{AttackKind, AttackPlan, Defender};
use crate::command::CommandError;
use crate::effects::unit::unit_modifiers;
use crate::ids::{PlayerId, TileId, UnitHandle, UnitKindId};
use crate::rules::effects::{Modifier, UnitAbility, UnitStat};
use crate::rules::CompiledRuleset;
use crate::systems::effects::{TriggerEvent, Victim};
use crate::transaction::journal::UnitChange;
use crate::transaction::transaction::WorldTransaction;
use crate::world::unit::{Stance, MOVE_DENOMINATOR};
use crate::world::player::ClassicPlayers;

/// A unit of `player` (`killer`) killed `victim` (read before the strike).
#[derive(Debug, Clone, Copy)]
struct Kill {
    player: PlayerId,
    killer: UnitHandle,
    victim: Victim,
}

/// Result of one strike on a unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StrikeOutcome {
    Damaged,
    Killed,
}

/// Capture of a defeated unit (prize ships): chance `min(80, 10 + 40 x
/// attacker strength / defender strength)` percent, and the captured unit
/// keeps 50 hit points. Unciv: `battle/BattleUnitCapture.kt`
/// (`unitCapturedPrizeShipsUnique`, `spawnCapturedUnit`); equal, except that
/// the ratio here uses the base strengths (the defender is already gone).
const UNIT_CAPTURE_MIN_CHANCE: i64 = 10;
const UNIT_CAPTURE_MAX_CHANCE: i64 = 80;
const UNIT_CAPTURE_RATIO_PERCENT: i64 = 40;
pub(crate) const CAPTURED_UNIT_HP: u16 = 50;

impl<'a> WorldTransaction<'a> {
    /// Siege set up: one move point (§3.2).
    pub(crate) fn set_up_ranged(&mut self, unit: UnitHandle) -> Result<(), CommandError> {
        let core = self
            .world
            .units
            .get_mut_core(unit)
            .ok_or(CommandError::UnknownUnit)?;
        core.moves_left = core.moves_left.saturating_sub(MOVE_DENOMINATOR);
        core.stance = Stance::SetUp;
        core.goal = None;
        self.journal
            .unit_changes
            .push(UnitChange::StanceChanged { unit });
        Ok(())
    }

    /// Resolve a planned attack (melee or ranged) with drawn rolls.
    pub(crate) fn attack(
        &mut self,
        plan: &AttackPlan,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        let Some(attacker) = self.world.units.get(plan.attacker).map(|u| u.core.owner) else {
            return Err(CommandError::UnknownUnit);
        };
        if let Defender::Unit(unit) = plan.defender {
            // Archaeology: a battle between units stays in the tile's history.
            if let Some(defender) = self.world.units.get(unit).map(|u| u.core.owner) {
                self.record_battle(plan.target_tile, attacker, defender, rules);
            }
        }
        match (plan.kind, plan.defender) {
            (AttackKind::Melee, Defender::Civilian(unit)) => self.melee_civilian(plan, unit, rules),
            (AttackKind::Melee, Defender::Unit(unit)) => self.melee_unit(plan, unit, rules),
            (AttackKind::Melee, Defender::City(city)) => self.melee_city(plan, city, rules),
            (AttackKind::Ranged, _) => self.ranged(plan, rules),
        }
    }

    pub(crate) fn roll(&mut self) -> Roll {
        Roll::Drawn(
            self.world
                .rng
                .combat
                .below(MELEE_EQUAL_STRENGTH_RANDOM_DAMAGE),
        )
    }

    /// Melee on a tile of civilians: capture (or destroy) them and move in.
    fn melee_civilian(
        &mut self,
        plan: &AttackPlan,
        civilian: UnitHandle,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        self.capture_civilians(plan.attacker, plan.target_tile, rules);
        let _ = civilian;
        self.advance(plan.attacker, plan.target_tile, rules)?;
        self.finish_attack(plan.attacker, rules, false)
    }

    /// Melee unit against unit (§2.3).
    fn melee_unit(
        &mut self,
        plan: &AttackPlan,
        defender: UnitHandle,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        if self.try_withdraw(plan, defender, rules) {
            self.capture_civilians(plan.attacker, plan.target_tile, rules);
            self.advance(plan.attacker, plan.target_tile, rules)?;
            return self.finish_attack(plan.attacker, rules, false);
        }
        let attacker_roll = self.roll();
        let defender_roll = self.roll();
        let to_defender = plan.damage_to_defender(attacker_roll);
        let to_attacker = plan.damage_to_attacker(defender_roll);
        let (to_defender, to_attacker) =
            resolve_mutual_kill(to_defender, plan.defender_hp, to_attacker, plan.attacker_hp);
        let defender_victim = Victim::of(self.world, defender).ok_or(CommandError::UnknownUnit)?;
        let attacker_victim =
            Victim::of(self.world, plan.attacker).ok_or(CommandError::UnknownUnit)?;
        let defender_outcome = self.strike_unit(defender, to_defender)?;
        let attacker_outcome = self.strike_unit(plan.attacker, to_attacker)?;
        let barbarian_fight = self.world.players.is_barbarian(plan.defender_owner)
            || self.world.players.is_barbarian(plan.attacker_owner);
        if attacker_outcome == StrikeOutcome::Damaged {
            let xp = crate::combat::experience::EXPERIENCE_ATTACKING_UNIT_MELEE;
            self.combat_experience(
                plan.attacker,
                xp,
                plan.defender_owner,
                plan.target_tile,
                rules,
            );
        }
        if defender_outcome == StrikeOutcome::Damaged {
            let xp = crate::combat::experience::EXPERIENCE_DEFENDING_UNIT_MELEE;
            self.combat_experience(defender, xp, plan.attacker_owner, plan.target_tile, rules);
            self.after_first_combat(defender, rules);
        }
        if attacker_outcome == StrikeOutcome::Killed {
            if defender_outcome == StrikeOutcome::Damaged {
                let kill = Kill { player: plan.defender_owner, killer: defender, victim: attacker_victim };
                self.unit_killed(rules, kill, barbarian_fight);
            }
            return Ok(());
        }
        self.after_first_combat(plan.attacker, rules);
        if defender_outcome == StrikeOutcome::Killed {
            let kill = Kill { player: plan.attacker_owner, killer: plan.attacker, victim: defender_victim };
            self.unit_killed(rules, kill, barbarian_fight);
            self.try_capture_defeated(plan, defender, rules);
            if self.no_defender_left(plan.attacker_owner, plan.target_tile, rules) {
                self.capture_civilians(plan.attacker, plan.target_tile, rules);
                self.advance(plan.attacker, plan.target_tile, rules)?;
                return self.finish_attack(plan.attacker, rules, false);
            }
        }
        self.adjacent_damage_on_attack(plan, rules);
        self.finish_attack(plan.attacker, rules, true)
    }

    /// Ranged attack or air strike (§2.5, §2.7): no answer (an aircraft
    /// takes the target's answer and the interception), the shooter stays.
    fn ranged(&mut self, plan: &AttackPlan, rules: &CompiledRuleset) -> Result<(), CommandError> {
        let interception = if plan.air {
            self.intercept(plan, rules)
        } else {
            0
        };
        let roll = self.roll();
        let damage = if plan.air
            && interception > 0
            && matches!(plan.defender, Defender::Unit(_) | Defender::City(_))
        {
            crate::combat::damage::air_damage(
                plan.attacker_strength,
                plan.defender_strength,
                plan.attacker_damage,
                interception,
                roll,
            )
        } else {
            plan.damage_to_defender(roll)
        };
        let damage = plan.cap_to_defender(damage);
        let barbarian_fight = self.world.players.is_barbarian(plan.defender_owner)
            || self.world.players.is_barbarian(plan.attacker_owner);
        let (attack_xp, defend_xp) = ranged_experience(plan);
        let target_alive = match plan.defender {
            Defender::City(city) => {
                self.damage_city(city, damage);
                true
            }
            Defender::Unit(unit) | Defender::Civilian(unit) => {
                let victim = Victim::of(self.world, unit).ok_or(CommandError::UnknownUnit)?;
                let outcome = self.strike_unit(unit, damage)?;
                let alive = outcome == StrikeOutcome::Damaged;
                if alive && matches!(plan.defender, Defender::Unit(_)) {
                    self.combat_experience(
                        unit,
                        defend_xp,
                        plan.attacker_owner,
                        plan.target_tile,
                        rules,
                    );
                }
                if !alive {
                    let kill = Kill { player: plan.attacker_owner, killer: plan.attacker, victim };
                    self.unit_killed(rules, kill, barbarian_fight);
                }
                alive
            }
        };
        self.combat_experience(
            plan.attacker,
            attack_xp,
            plan.defender_owner,
            plan.target_tile,
            rules,
        );
        if plan.air {
            self.air_aftermath(plan, interception, target_alive, rules)?;
        }
        if self.world.units.get(plan.attacker).is_none() {
            return Ok(());
        }
        self.after_first_combat(plan.attacker, rules);
        let suicide = self
            .world
            .units
            .get(plan.attacker)
            .and_then(|u| rules.units.get(u.core.kind.0 as usize))
            .is_some_and(|d| d.combat.suicide);
        if suicide {
            self.destroy_unit(plan.attacker)?;
            return Ok(());
        }
        self.finish_attack(plan.attacker, rules, true)
    }

    /// The attack is over for the attacker (§7.7): one attack more, no
    /// stance, no work; the moves: a lost or stopped attack costs a point,
    /// and without MoveAfterAttacking the turn ends.
    pub(crate) fn finish_attack(
        &mut self,
        attacker: UnitHandle,
        rules: &CompiledRuleset,
        stopped: bool,
    ) -> Result<(), CommandError> {
        let Some(view) = self.world.units.get(attacker) else {
            return Ok(());
        };
        let core = *view.core;
        let mods = unit_modifiers(self.world, rules, &core);
        let core = self
            .world
            .units
            .get_mut_core(attacker)
            .ok_or(CommandError::UnknownUnit)?;
        core.attacks_made = core.attacks_made.saturating_add(1);
        core.stance = match core.stance {
            Stance::SetUp => Stance::SetUp,
            _ => Stance::Ready,
        };
        core.work = None;
        core.goal = None;
        if stopped {
            core.moves_left = core.moves_left.saturating_sub(MOVE_DENOMINATOR);
        }
        if !mods.has(UnitAbility::MoveAfterAttacking) {
            core.moves_left = 0;
        }
        Ok(())
    }

    /// Apply `damage` to a unit: destroy it or damage it.
    pub(crate) fn strike_unit(
        &mut self,
        unit: UnitHandle,
        damage: u16,
    ) -> Result<StrikeOutcome, CommandError> {
        let hp = self
            .world
            .units
            .get(unit)
            .ok_or(CommandError::UnknownUnit)?
            .core
            .hp;
        if damage == 0 {
            return Ok(StrikeOutcome::Damaged);
        }
        if damage >= hp {
            self.destroy_unit(unit)?;
            return Ok(StrikeOutcome::Killed);
        }
        let core = self
            .world
            .units
            .get_mut_core(unit)
            .ok_or(CommandError::UnknownUnit)?;
        core.hp -= damage;
        self.journal
            .unit_changes
            .push(UnitChange::Damaged { unit, damage });
        Ok(StrikeOutcome::Damaged)
    }

    /// A kill: OnUnitKilled, heal on kill (§5.3).
    fn unit_killed(&mut self, rules: &CompiledRuleset, kill: Kill, barbarian_fight: bool) {
        let Kill { player, killer, victim } = kill;
        self.fire(rules, TriggerEvent::UnitKilled { player, killer, victim });
        self.convert_camp_unit(player, victim, rules);
        let Some(view) = self.world.units.get(killer) else {
            return;
        };
        let core = *view.core;
        let mods = unit_modifiers(self.world, rules, &core);
        // Golden Age Points: a share of the victim's strength fills the
        // owner's golden age meter (not while a golden age runs).
        let share = mods.stat(UnitStat::Module(ClassicUnitStat::GoldenAgePointsOnKillPercent)).max(0) as u32;
        if share > 0 {
            self.add_golden_age_points(player, victim.strength(rules) * share / 100);
        }
        let heal = mods.stat(UnitStat::HealOnKill).max(0) as u16;
        if heal == 0 || (barbarian_fight && mods.has(UnitAbility::Module(ClassicUnitAbility::HealOnKillNotBarbarians))) {
            return;
        }
        if let Some(core) = self.world.units.get_mut_core(killer) {
            core.hp = (core.hp + heal).min(crate::world::unit::UNIT_MAX_HP);
        }
    }

    /// Withdrawal before melee: the defender steps back
    /// to a free tile away from the attacker.
    fn try_withdraw(
        &mut self,
        plan: &AttackPlan,
        defender: UnitHandle,
        rules: &CompiledRuleset,
    ) -> bool {
        let Some(view) = self.world.units.get(defender) else {
            return false;
        };
        let core = *view.core;
        let chance = unit_modifiers(self.world, rules, &core).stat(UnitStat::WithdrawChance);
        if chance <= 0 {
            return false;
        }
        let Some(to) = self.withdraw_tile(plan, &core, rules) else {
            return false;
        };
        if self.world.rng.combat.below(100) as i32 >= chance {
            return false;
        }
        self.relocate_unit(defender, to, rules);
        true
    }

    fn withdraw_tile(
        &self,
        plan: &AttackPlan,
        defender: &crate::world::unit::UnitCore,
        rules: &CompiledRuleset,
    ) -> Option<TileId> {
        let attacker_tile = self.world.units.get(plan.attacker)?.core.tile;
        let dims = self.world.map.dimensions;
        let mut candidates: Vec<TileId> = dims
            .neighbors(defender.tile)
            .into_iter()
            .filter(|t| !dims.adjacent(*t, attacker_tile) && *t != attacker_tile)
            .collect();
        candidates.sort_by_key(|t| t.0);
        // The defender's own element: a ship withdraws over water (Destroyer),
        // a land unit over land it can enter.
        let profile = crate::navigation::MoveProfile::of_unit(self.world, rules, defender);
        let element = |t: TileId| match profile.domain {
            crate::rules::content::Domain::Sea => crate::navigation::terrain::is_water(&self.world.map, t),
            _ => crate::navigation::cost::enter_cost_on_map(&self.world.map, t).is_some(),
        };
        candidates.into_iter().find(|t| {
            element(*t)
                && crate::navigation::cost::destination_ok(self.world, &profile, *t)
                && crate::world::territory::may_enter(self.world, defender.owner, *t)
                && crate::world::stacking::can_stop_at(
                    self.world,
                    rules,
                    defender.owner,
                    defender.kind,
                    None,
                    *t,
                )
                .is_ok()
                && self.world.indexes.city_on_tile(*t).is_none()
        })
    }

    /// Move a unit to a tile without spending moves (withdrawal, capture
    /// placement): store, index and journal.
    pub(crate) fn relocate_unit(&mut self, unit: UnitHandle, to: TileId, rules: &CompiledRuleset) {
        let Some(core) = self.world.units.get_mut_core(unit) else {
            return;
        };
        let from = core.tile;
        if from == to {
            return;
        }
        core.tile = to;
        core.stance = Stance::Ready;
        core.goal = None;
        core.work = None;
        self.world.indexes.move_unit(unit, from, to);
        self.journal
            .unit_changes
            .push(UnitChange::Moved { unit, from, to });
        self.refresh_allowance_across(unit, from, to, rules);
        // A carrier that withdraws takes its aircraft along.
        self.carry_cargo(unit, to, rules);
    }

    /// CaptureDefeatedEnemy (§2.3): a killed unit of the attacker's kind may
    /// join it with half HP.
    fn try_capture_defeated(
        &mut self,
        plan: &AttackPlan,
        _defender: UnitHandle,
        rules: &CompiledRuleset,
    ) {
        let Some(view) = self.world.units.get(plan.attacker) else {
            return;
        };
        let core = *view.core;
        let mods = unit_modifiers(self.world, rules, &core);
        if !mods.has(UnitAbility::CaptureDefeatedEnemy) {
            return;
        }
        let Some(kind) = self.last_destroyed_kind() else {
            return;
        };
        let strength = |k: UnitKindId| {
            rules
                .units
                .get(k.0 as usize)
                .map(|d| i64::from(d.strength))
                .unwrap_or(10)
                .max(1)
        };
        let ratio = strength(core.kind) * UNIT_CAPTURE_RATIO_PERCENT / strength(kind);
        let chance = (UNIT_CAPTURE_MIN_CHANCE + ratio).min(UNIT_CAPTURE_MAX_CHANCE);
        if self.world.rng.combat.below(100) as i64 >= chance {
            return;
        }
        let tile = core.tile;
        let spot = self.free_tile_near(plan.attacker_owner, tile, kind, rules);
        let handle = self.spawn_unit(plan.attacker_owner, spot, kind, rules);
        if let Some(c) = self.world.units.get_mut_core(handle) {
            c.hp = CAPTURED_UNIT_HP;
            c.moves_left = 0;
        }
    }

    /// The kind of the unit destroyed last in this transaction.
    fn last_destroyed_kind(&self) -> Option<UnitKindId> {
        self.journal
            .unit_changes
            .iter()
            .rev()
            .find_map(|c| match c {
                UnitChange::Destroyed { kind, .. } => Some(*kind),
                _ => None,
            })
    }

    /// No unit able to defend stands on the tile any more (enemies of the
    /// attacker); civilians do not hold a tile.
    fn no_defender_left(
        &self,
        attacker_owner: PlayerId,
        tile: TileId,
        rules: &CompiledRuleset,
    ) -> bool {
        if self.world.indexes.city_on_tile(tile).is_some() {
            return false;
        }
        !self.world.indexes.units_on(tile).iter().any(|u| {
            self.world
                .units
                .get(*u)
                .is_some_and(|v| v.core.owner != attacker_owner && !rules.is_civilian(v.core.kind))
        })
    }

    /// Enemy civilians on `tile` become the captor's (a settler becomes a
    /// worker) or die; NoCapture
    /// units destroy them.
    pub(crate) fn capture_civilians(
        &mut self,
        captor: UnitHandle,
        tile: TileId,
        rules: &CompiledRuleset,
    ) {
        let Some(view) = self.world.units.get(captor) else {
            return;
        };
        let captor_core = *view.core;
        let no_capture =
            unit_modifiers(self.world, rules, &captor_core).has(UnitAbility::NoCapture);
        let mut victims: Vec<UnitHandle> = self
            .world
            .indexes
            .units_on(tile)
            .iter()
            .copied()
            .filter(|u| {
                self.world.units.get(*u).is_some_and(|v| {
                    v.core.owner != captor_core.owner && rules.is_civilian(v.core.kind)
                })
            })
            .collect();
        victims.sort_by_key(|u| self.world.units.get(*u).map(|v| v.core.id));
        for victim in victims {
            let Ok(core) = self.destroy_unit(victim) else {
                continue;
            };
            let captured = rules
                .units
                .get(core.kind.0 as usize)
                .and_then(|d| d.combat.capture);
            match captured {
                Some(kind) if !no_capture => {
                    let spot = self.free_tile_near(captor_core.owner, tile, kind, rules);
                    let handle = self.spawn_unit(captor_core.owner, spot, kind, rules);
                    if let Some(c) = self.world.units.get_mut_core(handle) {
                        c.moves_left = 0;
                    }
                }
                _ => {}
            }
        }
    }

    /// The attacker moves into the conquered tile (§2.3), paying the step.
    pub(crate) fn advance(
        &mut self,
        unit: UnitHandle,
        to: TileId,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        let Some(view) = self.world.units.get(unit) else {
            return Ok(());
        };
        let core = *view.core;
        let stop = crate::world::stacking::can_stop_at(
            self.world,
            rules,
            core.owner,
            core.kind,
            Some(unit),
            to,
        );
        if stop.is_err() {
            return Ok(());
        }
        // The unit's own price of the step (a ship sails in, an embarked
        // unit lands; §7.9); whatever it cannot price takes all moves.
        let profile = crate::navigation::MoveProfile::of_unit(self.world, rules, &core);
        let step = crate::navigation::cost::step_cost_for(self.world, rules, core.owner, &profile, core.tile, to);
        let cost = match step {
            Some(crate::navigation::cost::StepCost::Points(points)) => {
                points.min(u32::from(core.moves_left)) as u16
            }
            _ => core.moves_left,
        };
        let from = core.tile;
        let c = self
            .world
            .units
            .get_mut_core(unit)
            .ok_or(CommandError::UnknownUnit)?;
        c.tile = to;
        c.moves_left -= cost;
        c.stance = Stance::Ready;
        c.work = None;
        self.world.indexes.move_unit(unit, from, to);
        self.journal
            .unit_changes
            .push(UnitChange::Moved { unit, from, to });
        self.refresh_allowance_across(unit, from, to, rules);
        self.clear_camp_at(core.owner, to, rules);
        self.touch_natural_wonders(unit, to, rules);
        Ok(())
    }

    /// A free tile for a new unit of `owner` at or next to `tile`.
    pub(crate) fn free_tile_near(
        &self,
        owner: PlayerId,
        tile: TileId,
        kind: UnitKindId,
        rules: &CompiledRuleset,
    ) -> TileId {
        use crate::world::stacking::can_stop_at;
        if can_stop_at(self.world, rules, owner, kind, None, tile).is_ok() {
            return tile;
        }
        let around = self.world.map.dimensions.neighbors(tile);
        let free = around.into_iter().find(|t| {
            let profile = crate::navigation::MoveProfile::of_kind(self.world, rules, owner, kind);
            let passable = crate::navigation::cost::destination_ok(self.world, &profile, *t);
            passable && can_stop_at(self.world, rules, owner, kind, None, *t).is_ok()
        });
        free.unwrap_or(tile)
    }

    /// Enemy Blade (AdjacentDamageOnAttack): enemies next to the attacker
    /// take damage after its attack.
    fn adjacent_damage_on_attack(&mut self, plan: &AttackPlan, rules: &CompiledRuleset) {
        let Some(view) = self.world.units.get(plan.attacker) else {
            return;
        };
        let core = *view.core;
        let mods = unit_modifiers(self.world, rules, &core);
        let damage = mods.stat(UnitStat::AdjacentDamageOnAttack);
        if damage <= 0 {
            return;
        }
        // With a chance (Enemy Blade: 100 %) the damage happens only when
        // the roll falls under it; without one it always happens.
        let chance = mods.stat(UnitStat::Module(ClassicUnitStat::AdjacentDamageChance));
        if chance > 0 && chance < 100 && self.world.rng.combat.below(100) >= chance as u64 {
            return;
        }
        let dims = self.world.map.dimensions;
        let mut victims: Vec<UnitHandle> = Vec::new();
        for tile in dims.neighbors(core.tile) {
            for u in self.world.indexes.units_on(tile) {
                let enemy = self.world.units.get(*u).is_some_and(|v| {
                    self.world.diplomacy.at_war(core.owner, v.core.owner)
                        && !rules.is_civilian(v.core.kind)
                });
                if enemy {
                    victims.push(*u);
                }
            }
        }
        victims.sort_by_key(|u| self.world.units.get(*u).map(|v| v.core.id));
        for victim in victims {
            let _ = self.strike_unit(victim, damage as u16);
        }
    }

    /// Mystic Blade (PostCombatPromotion): after its first fight the unit
    /// gains one of its candidate promotions at random.
    fn after_first_combat(&mut self, unit: UnitHandle, rules: &CompiledRuleset) {
        let Some(view) = self.world.units.get(unit) else {
            return;
        };
        let core = *view.core;
        let mods = unit_modifiers(self.world, rules, &core);
        if mods.post_combat.is_empty()
            || mods
                .post_combat
                .iter()
                .any(|p| core.promotions.contains(*p))
        {
            return;
        }
        let pick = self.world.rng.combat.below(mods.post_combat.len() as u64) as usize;
        let promotion = mods.post_combat[pick];
        // The promotion that carries PostCombatPromotions is replaced by
        // the random one.
        let marker = core.promotions.iter().find(|p| {
            rules.promotions.get(p.0 as usize).is_some_and(|def| {
                def.effects.passive_unit.iter().any(|e| {
                    e.modifier == Modifier::GrantAbility(UnitAbility::Module(ClassicUnitAbility::PostCombatPromotions))
                })
            })
        });
        if let Some(c) = self.world.units.get_mut_core(unit) {
            if let Some(marker) = marker {
                c.promotions.remove(marker);
            }
            if c.promotions.insert(promotion) {
                self.journal
                    .unit_changes
                    .push(UnitChange::Promoted { unit, promotion });
            }
        }
    }
}

/// Experience of a ranged attack for the attacker and a surviving defender
/// (§6.1).
fn ranged_experience(plan: &AttackPlan) -> (u32, u32) {
    use crate::combat::experience as xp;
    match (plan.air, plan.defender) {
        (true, Defender::City(_)) => (xp::EXPERIENCE_ATTACKING_CITY_AIR, 0),
        (true, _) => (
            xp::EXPERIENCE_ATTACKING_UNIT_AIR,
            xp::EXPERIENCE_DEFENDING_UNIT_AIR,
        ),
        (false, Defender::City(_)) => (xp::EXPERIENCE_ATTACKING_CITY_RANGED, 0),
        (false, _) => (
            xp::EXPERIENCE_ATTACKING_UNIT_RANGED,
            xp::EXPERIENCE_DEFENDING_UNIT_RANGED,
        ),
    }
}
