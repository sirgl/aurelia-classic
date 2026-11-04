//! Experience from combat, promotions and great generals (classic/docs/classic/rules.md). Journal: UnitChange::Promoted, UnitChange::Created (a great
//! general is born).

use crate::classic::effects::{ClassicUnitAbility, ClassicUnitStat};
use crate::classic::ClassicChange;
use crate::combat::experience::{experience_gain, great_general_threshold, INSTANT_HEAL_HIT_POINTS};
use crate::command::CommandError;
use crate::effects::unit::unit_modifiers;
use crate::ids::{PlayerId, PromotionId, TileId, UnitHandle};
use crate::rules::content::Domain;
use crate::rules::effects::{Modifier, UnitAbility, UnitStat};
use crate::classic::effects::GreatPersonKind;
use crate::rules::CompiledRuleset;
use crate::transaction::journal::UnitChange;
use crate::transaction::transaction::WorldTransaction;
use crate::world::player::ClassicPlayers;

impl<'a> WorldTransaction<'a> {
    /// Experience of one combat for `unit` against `opponent` on `tile`
    /// (§6.2): barbarians get none, fighting barbarians gives at most 30,
    /// and the owner's great general (land) or admiral (sea) points grow
    /// by the base experience × the unit's GreatGeneralPointsPercent —
    /// except against barbarians.
    pub(crate) fn combat_experience(
        &mut self,
        unit: UnitHandle,
        base: u32,
        opponent: PlayerId,
        tile: TileId,
        rules: &CompiledRuleset,
    ) {
        let Some(view) = self.world.units.get(unit) else {
            return;
        };
        let core = *view.core;
        if self.world.players.is_barbarian(core.owner) {
            return;
        }
        let against_barbarian = self.world.players.is_barbarian(opponent);
        let mods = unit_modifiers(self.world, rules, &core);
        let gain = experience_gain(
            base,
            core.experience,
            mods.stat(UnitStat::ExperiencePercent),
            against_barbarian,
        );
        if let Some(c) = self.world.units.get_mut_core(unit) {
            c.experience = c.experience.saturating_add(gain);
        }
        if against_barbarian {
            return;
        }
        let domain = rules
            .units
            .get(core.kind.0 as usize)
            .map(|d| d.domain)
            .unwrap_or_default();
        let percent = 100 + mods.stat(UnitStat::Module(ClassicUnitStat::GreatGeneralPointsPercent));
        let points = (i64::from(base) * i64::from(percent.max(0)) / 100) as u32;
        self.add_leader_points(core.owner, domain, points, tile, rules);
    }

    /// Great general / admiral points; at the threshold a leader is born
    /// next to the battle.
    fn add_leader_points(
        &mut self,
        owner: PlayerId,
        domain: Domain,
        points: u32,
        tile: TileId,
        rules: &CompiledRuleset,
    ) {
        if points == 0 || domain == Domain::Air || !self.world.players.is_major(owner) {
            return;
        }
        let Some(m) = self.world.module.military.get_mut(owner) else {
            return;
        };
        let (kind, ready) = match domain {
            Domain::Sea => {
                m.admiral_points = m.admiral_points.saturating_add(points);
                let threshold = great_general_threshold(m.admirals_born);
                let ready = m.admiral_points >= threshold;
                if ready {
                    m.admiral_points -= threshold;
                    m.admirals_born += 1;
                }
                (GreatPersonKind::Admiral, ready)
            }
            Domain::Land | Domain::Air => {
                m.general_points = m.general_points.saturating_add(points);
                let threshold = great_general_threshold(m.generals_born);
                let ready = m.general_points >= threshold;
                if ready {
                    m.general_points -= threshold;
                    m.generals_born += 1;
                    self.journal.module_changes.push(ClassicChange::GeneralBorn { player: owner });
                }
                (GreatPersonKind::General, ready)
            }
        };
        if ready {
            self.birth_leader(owner, kind, tile, rules);
        }
    }

    /// Start of `player`'s turn, after the moves are restored: a Great
    /// General standing on the tile of a unit that lends movement (the
    /// Hakkapeliitta) moves at that unit's rate if it is faster.
    pub(crate) fn lend_movement_to_generals(&mut self, player: PlayerId, rules: &CompiledRuleset) {
        let lends = UnitAbility::Module(ClassicUnitAbility::LendsMovementToGeneral);
        let general = UnitAbility::Module(ClassicUnitAbility::GreatGeneral);
        if !crate::effects::unit::any_unit_effect(rules, |e| e.modifier == Modifier::GrantAbility(lends)) {
            return;
        }
        let mut lenders: Vec<(TileId, u16)> = Vec::new();
        let mut generals: Vec<UnitHandle> = Vec::new();
        for handle in self.world.indexes().units_of(player).to_vec() {
            let Some(view) = self.world.units.get(handle) else {
                continue;
            };
            let core = *view.core;
            if crate::effects::unit::own_ability(rules, &core, lends) {
                lenders.push((core.tile, core.moves_max));
            }
            if crate::effects::unit::own_ability(rules, &core, general) {
                generals.push(handle);
            }
        }
        for handle in generals {
            let Some(core) = self.world.units.get_mut_core(handle) else {
                continue;
            };
            let rate = lenders.iter().filter(|(tile, _)| *tile == core.tile).map(|(_, moves)| *moves).max();
            if let Some(rate) = rate.filter(|rate| *rate > core.moves_max) {
                core.moves_max = rate;
                core.moves_left = rate;
            }
        }
    }

    /// A great general (or admiral) appears at the battle, or next to it.
    fn birth_leader(
        &mut self,
        owner: PlayerId,
        kind: GreatPersonKind,
        tile: TileId,
        rules: &CompiledRuleset,
    ) {
        let born_in_combat = |g: &&crate::classic::tables::GreatPersonDefinition| {
            g.kind == kind && g.points_source == crate::rules::content::PointsSource::CombatExperience
        };
        let Some(person) = rules.module.great_people.iter().find(born_in_combat) else {
            return;
        };
        let nation = self.world.players.nation_of(owner);
        let key = &rules.units[person.unit.0 as usize].key;
        let unit = rules.nation_variant(nation, key).unwrap_or(person.unit);
        let spot = self.free_tile_near(owner, tile, unit, rules);
        self.spawn_unit(owner, spot, unit, rules);
    }

    /// Instant experience (free XP of a building or policy, not combat).
    pub(crate) fn grant_experience(&mut self, unit: UnitHandle, amount: u16) {
        if let Some(core) = self.world.units.get_mut_core(unit) {
            core.experience = core.experience.saturating_add(u32::from(amount));
        }
    }

    /// Take a promotion (§6.3): level + 1; InstaHeal heals 50 HP instead of
    /// staying on the unit. The unit's turn goes on.
    pub(crate) fn promote(
        &mut self,
        unit: UnitHandle,
        promotion: PromotionId,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        let insta_heal = rules
            .promotions
            .get(promotion.0 as usize)
            .is_some_and(|def| def.heals_instantly());
        let core = self
            .world
            .units
            .get_mut_core(unit)
            .ok_or(CommandError::UnknownUnit)?;
        core.level = core.level.saturating_add(1);
        if insta_heal {
            core.hp = (core.hp + INSTANT_HEAL_HIT_POINTS).min(crate::world::unit::UNIT_MAX_HP);
            return Ok(());
        }
        if core.promotions.insert(promotion) {
            self.journal
                .unit_changes
                .push(UnitChange::Promoted { unit, promotion });
        }
        Ok(())
    }
}
