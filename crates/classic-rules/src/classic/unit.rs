//! The classic state of one unit (`kernel::world::module::ModuleUnit`): the
//! carrier an aircraft or missile is based on (its cargo). Saved from
//! codec version 20 on, hashed only when set, so a game without carriers
//! keeps its hash.

use crate::classic::Classic;
use crate::ids::EntityId;
use crate::rules::CompiledRuleset;
use crate::world::module::ModuleUnit;

/// The first codec version whose unit records carry the carrier.
pub const SAVED_SINCE: u32 = 20;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ClassicUnit {
    /// The carrier (aircraft carrier, missile cruiser, nuclear submarine)
    /// this aircraft or missile is based on; None in a city. A carried unit
    /// stands on its carrier's tile and moves with it.
    pub carried_by: Option<EntityId>,
}

impl ModuleUnit<Classic> for ClassicUnit {
    type Saved = Option<u64>;
    type Seen = ();
    const SAVED_SINCE: u32 = SAVED_SINCE;

    fn encode(&self) -> Option<u64> {
        self.carried_by.map(|c| c.0)
    }

    fn decode(saved: &Option<u64>, _: &CompiledRuleset) -> Result<Self, &'static str> {
        Ok(ClassicUnit { carried_by: saved.map(EntityId) })
    }

    fn hash(&self, mix: &mut dyn FnMut(u64)) {
        if let Some(carrier) = self.carried_by {
            mix(0x6361_7272_6965_72 ^ carrier.0);
        }
    }

    fn seen(&self) {}
}
