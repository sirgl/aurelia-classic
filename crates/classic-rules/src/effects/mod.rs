//! The classic instance of `kernel::effects` (generic over the rules module).

pub use kernel::effects::*;
pub mod city;
pub mod module;
pub mod player;
pub mod unit;

use crate::classic::Classic;

pub type Bucket = kernel::effects::Bucket<Classic>;
