//! The classic instance of `kernel::rules::effects` (generic over the rules module).

pub use kernel::rules::effects::*;

use crate::classic::Classic;

pub type Scope = kernel::rules::effects::Scope<Classic>;
pub type Selector = kernel::rules::effects::Selector<Classic>;
pub type Trigger = kernel::rules::effects::Trigger<Classic>;
pub type YieldKind = kernel::rules::effects::YieldKind<Classic>;
pub type CostTarget = kernel::rules::effects::CostTarget<Classic>;
pub type UnitStat = kernel::rules::effects::UnitStat<Classic>;
pub type UnitAbility = kernel::rules::effects::UnitAbility<Classic>;
pub type Count = kernel::rules::effects::Count<Classic>;
pub type PerCount = kernel::rules::effects::PerCount<Classic>;
pub type Condition = kernel::rules::effects::Condition<Classic>;
pub type Grant = kernel::rules::effects::Grant<Classic>;
pub type Modifier = kernel::rules::effects::Modifier<Classic>;
pub type CompiledEffect = kernel::rules::effects::CompiledEffect<Classic>;
pub type EffectSet = kernel::rules::effects::EffectSet<Classic>;
pub type SourceRef = kernel::rules::effects::SourceRef<Classic>;
pub type TriggerSources = kernel::rules::effects::TriggerSources<Classic>;
pub type EffectIndex = kernel::rules::effects::EffectIndex<Classic>;
