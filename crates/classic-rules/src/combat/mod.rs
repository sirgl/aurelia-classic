//! Combat rules as a read model (classic/docs/classic/rules.md):
//! strengths, damage, the exchange of one attack and its preview.
//!
//! Read-only over `AuthWorld` + `CompiledRuleset`. The combat system
//! (`systems::combat`) applies an exchange inside a transaction with the
//! drawn rolls; the interface and the AI ask `preview` for the same
//! exchange with the expected rolls — one formula for all three.
//!
//! | module       | what                                                   |
//! |--------------|--------------------------------------------------------|
//! | `damage`     | damage formulas, exact strength ratio                  |
//! | `strength`   | unit strengths: melee attack, defense, ranged          |
//! | `city`       | city strength, hit points, healing, capital, era       |
//! | `terrain`    | battle tile: rough/open, defense, water, friendly land |
//! | `experience` | levels, promotions available, great general points    |
//! | `exchange`   | one attack planned: who fights whom with what          |

pub mod city;
pub mod damage;
pub mod exchange;
pub mod experience;
pub mod strength;
pub mod terrain;

pub use exchange::{plan_attack, AttackKind, AttackPlan, CombatPreview, Defender, PlanError};
/// Radius (tiles) of a nuclear blast.
pub use crate::systems::combat::actions::NUKE_BLAST_RADIUS;
