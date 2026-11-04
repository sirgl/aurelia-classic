//! The classic state of one city (`kernel::world::module::ModuleCity`): the
//! great person points of each specialist kind. Saved and hashed in the
//! city's record of the economy section, after the food kept, as before
//! the split.

use crate::classic::Classic;
use crate::rules::CompiledRuleset;
use crate::world::city::MAX_SPECIALIST_KINDS;
use crate::world::module::ModuleCity;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClassicCity {
    /// Great person points per specialist kind, in hundredths (index =
    /// `SpecialistId`).
    pub great_person_points: [u32; MAX_SPECIALIST_KINDS],
}

impl ModuleCity<Classic> for ClassicCity {
    /// One entry per tracked specialist kind.
    type Saved = Vec<u32>;

    fn encode(&self) -> Vec<u32> {
        self.great_person_points.to_vec()
    }

    fn decode(saved: &Vec<u32>, _: &CompiledRuleset) -> Result<Self, &'static str> {
        let points = <[u32; MAX_SPECIALIST_KINDS]>::try_from(saved.as_slice()).map_err(|_| "great person points")?;
        Ok(ClassicCity { great_person_points: points })
    }

    fn hash(saved: &Vec<u32>, mix: &mut dyn FnMut(u64)) {
        for p in saved {
            mix(u64::from(*p));
        }
    }
}
