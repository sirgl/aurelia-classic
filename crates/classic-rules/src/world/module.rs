//! The classic instance of `kernel::world::module` (generic over the rules module).

pub use kernel::world::module::*;

use crate::classic::Classic;

pub type SavedModule = kernel::world::module::SavedModule<Classic>;
pub type ModuleChangeOf = kernel::world::module::ModuleChangeOf<Classic>;
pub type NewModuleWorld<'a> = kernel::world::module::NewModuleWorld<'a, Classic>;
