//! The classic instance of `kernel::rules::vocabulary` (generic over the rules module).

pub use kernel::rules::vocabulary::*;

use crate::classic::Classic;

pub type ModuleVocabulary = kernel::rules::vocabulary::ModuleVocabulary<Classic>;
pub type ModuleScope = kernel::rules::vocabulary::ModuleScope<Classic>;
pub type ModuleSelector = kernel::rules::vocabulary::ModuleSelector<Classic>;
pub type ModuleTrigger = kernel::rules::vocabulary::ModuleTrigger<Classic>;
pub type ModuleCondition = kernel::rules::vocabulary::ModuleCondition<Classic>;
pub type ModuleModifier = kernel::rules::vocabulary::ModuleModifier<Classic>;
pub type ModuleUnitStat = kernel::rules::vocabulary::ModuleUnitStat<Classic>;
pub type ModuleUnitAbility = kernel::rules::vocabulary::ModuleUnitAbility<Classic>;
pub type ModuleYield = kernel::rules::vocabulary::ModuleYield<Classic>;
pub type ModuleCost = kernel::rules::vocabulary::ModuleCost<Classic>;
pub type ModuleCount = kernel::rules::vocabulary::ModuleCount<Classic>;
pub type ModuleGrant = kernel::rules::vocabulary::ModuleGrant<Classic>;
pub type ModuleTable = kernel::rules::vocabulary::ModuleTable<Classic>;
pub type ModuleSource = kernel::rules::vocabulary::ModuleSource<Classic>;
pub type ModuleSourceModifier = kernel::rules::vocabulary::ModuleSourceModifier<Classic>;
pub type ModuleSourceCondition = kernel::rules::vocabulary::ModuleSourceCondition<Classic>;
pub type ModuleIndex = kernel::rules::vocabulary::ModuleIndex<Classic>;
