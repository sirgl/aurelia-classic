//! The classic instance of `kernel::rules::compiled` (generic over the rules module).

pub use kernel::rules::compiled::*;

use crate::classic::Classic;

pub type UnitDefinition = kernel::rules::compiled::UnitDefinition<Classic>;
pub type BuildingDefinition = kernel::rules::compiled::BuildingDefinition<Classic>;
pub type TechnologyDefinition = kernel::rules::compiled::TechnologyDefinition<Classic>;
pub type ImprovementDefinition = kernel::rules::compiled::ImprovementDefinition<Classic>;
pub type TechYield = kernel::rules::compiled::TechYield<Classic>;
pub type NationDefinition = kernel::rules::compiled::NationDefinition<Classic>;
pub type PromotionDefinition = kernel::rules::compiled::PromotionDefinition<Classic>;
pub type CompiledRuleset = kernel::rules::compiled::CompiledRuleset<Classic>;
pub type ResourceDefinition = kernel::rules::compiled::ResourceDefinition<Classic>;
pub type EraDefinition = kernel::rules::compiled::EraDefinition<Classic>;
pub type NaturalWonderDefinition = kernel::rules::compiled::NaturalWonderDefinition<Classic>;
pub type SpecialistDefinition = kernel::rules::compiled::SpecialistDefinition<Classic>;
pub type HandicapDefinition = kernel::rules::compiled::HandicapDefinition<Classic>;
