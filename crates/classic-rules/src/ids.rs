//! The kernel's ids (`kernel::ids`) with the ids of the classic tables whose
//! rows carry effects: policies and beliefs.

pub use kernel::ids::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PolicyId(pub u16);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BeliefId(pub u16);
