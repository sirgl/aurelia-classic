//! The classic instance of `kernel::module` (generic over the rules module).

pub use kernel::module::*;

use crate::classic::Classic;

pub type Rejected = kernel::module::Rejected<Classic>;
