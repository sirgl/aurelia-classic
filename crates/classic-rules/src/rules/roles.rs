//! The classic instance of `kernel::rules::roles` (generic over the rules
//! module), with the classic bindings (`classic::roles`).

pub use kernel::rules::roles::*;

pub use crate::classic::roles::{ClassicRoles, ClassicSourceRoles, IdeologyCase};

use crate::classic::Classic;

pub type SourceRoles = kernel::rules::roles::SourceRoles<Classic>;
pub type Roles = kernel::rules::roles::Roles<Classic>;
