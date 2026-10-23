//! The classic instance of `kernel::transaction::journal` (generic over the rules module).

pub use kernel::transaction::journal::*;

use crate::classic::Classic;

pub type ChangeJournal = kernel::transaction::journal::ChangeJournal<Classic>;
pub type AppliedCommand = kernel::transaction::journal::AppliedCommand<Classic>;
pub type CommandResult = kernel::transaction::journal::CommandResult<Classic>;
