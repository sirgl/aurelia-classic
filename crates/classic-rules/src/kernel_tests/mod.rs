//! Tests of kernel code that need a whole game: the classic rules compiled
//! from RON fixtures, a scenario, the module's commands. They live with the
//! module because the kernel cannot depend on it; the kernel keeps the
//! tests that need no module or only its test module.

mod astar;
mod city_names;
mod compile;
mod compile_effects;
mod decode;
mod fingerprint;
mod harness_oracle;
mod memory;
mod movement;
mod replay;
mod roles;
mod visibility;
mod vocabulary;
