//! The end-to-end neural AI (classic/docs/ai-neural-architecture.md): the
//! features of a turn (`encode`), the events a player learns between its
//! turns (`events`), the integer forward pass (`model`) and the
//! player that plays with them (`mind`, `Variant::Net`); `kernels` holds the
//! aarch64 kernels of the forward pass; `remote` asks a net served by
//! another process (`nn/aurelia_nn/remote.py`); `v2` runs the big float net
//! in process (the trunk on the GPU, the decisions on the CPU).

pub mod encode;
pub mod events;
mod kernels;
pub mod mind;
pub mod model;
#[cfg(unix)]
pub mod remote;
pub mod sample;
pub mod v2;
