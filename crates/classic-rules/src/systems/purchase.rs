//! A unit bought with gold (`KernelEvent::UnitPurchased`): it
//! cannot move on the turn it is bought, unless its kind may move after
//! its purchase (`MoveAfterPurchase`: the Landsknecht). A trained unit
//! keeps its moves. Journal: none (the unit's moves are not hashed
//! separately).

use crate::ids::{UnitHandle, UnitKindId};
use crate::rules::content::UnitAbility;
use crate::rules::CompiledRuleset;
use crate::transaction::transaction::WorldTransaction;

impl<'a> WorldTransaction<'a> {
    pub(crate) fn bought_unit_waits(&mut self, unit: UnitHandle, kind: UnitKindId, rules: &CompiledRuleset) {
        let moves_on = rules
            .units
            .get(kind.0 as usize)
            .is_some_and(|d| d.module.abilities.contains(&UnitAbility::MoveAfterPurchase));
        if moves_on {
            return;
        }
        if let Some(core) = self.world.units.get_mut_core(unit) {
            core.moves_left = 0;
        }
    }
}
