//! The classic instance of `kernel::effects::module` (generic over the rules module).

pub use kernel::effects::module::*;

use crate::classic::Classic;

pub type Visit<'v> = kernel::effects::module::Visit<'v, Classic>;
pub type ModuleCityModifiers = kernel::effects::module::ModuleCityModifiers<Classic>;
pub type ModulePlayerModifiers = kernel::effects::module::ModulePlayerModifiers<Classic>;
