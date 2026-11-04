//! The classic instance of `kernel::rules::source` (generic over the rules module).

pub use kernel::rules::source::*;

pub use crate::classic::source::*;

use crate::classic::Classic;

pub type SourceUnit = kernel::rules::source::SourceUnit<Classic>;
pub type SourceBuilding = kernel::rules::source::SourceBuilding<Classic>;
pub type SourceTechnology = kernel::rules::source::SourceTechnology<Classic>;
pub type SourceImprovement = kernel::rules::source::SourceImprovement<Classic>;
pub type SourceTechYield = kernel::rules::source::SourceTechYield<Classic>;
pub type SourcePolicy = crate::classic::source::SourcePolicy<Classic>;
pub type SourceEffect = kernel::rules::source::SourceEffect<Classic>;
pub type SourceCondition = kernel::rules::source::SourceCondition<Classic>;
pub type SourceModifier = kernel::rules::source::SourceModifier<Classic>;
pub type SourcePromotion = kernel::rules::source::SourcePromotion<Classic>;
pub type SourceBelief = crate::classic::source::SourceBelief<Classic>;
pub type SourceNation = kernel::rules::source::SourceNation<Classic>;
pub type SourceRules = kernel::rules::source::SourceRules<Classic>;
pub type SourceResource = kernel::rules::source::SourceResource<Classic>;
pub type SourceEra = kernel::rules::source::SourceEra<Classic>;
pub type SourceNaturalWonder = kernel::rules::source::SourceNaturalWonder<Classic>;
pub type SourceSpecialist = kernel::rules::source::SourceSpecialist<Classic>;
pub type SourceHandicap = kernel::rules::source::SourceHandicap<Classic>;
pub type HandicapValues = kernel::rules::source::HandicapValues<Classic>;
