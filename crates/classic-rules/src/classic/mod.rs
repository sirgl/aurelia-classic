//! The classic rules module (platform/PLATFORM.md): the marker type `Classic` the
//! kernel is instantiated with, and what it names for the kernel — its
//! world state, words and their evaluation, tables, turn steps and their
//! order, AI advisors and save sections. The systems, derived mechanics and
//! AI of the module live in the rest of this crate.

pub mod advisors;
pub mod calendar;
pub mod city;
pub mod effects;
pub mod evaluate;
pub mod player;
pub mod religion_effects;
pub mod roles;
pub mod source;
pub mod rows;
pub mod change;
pub(crate) mod compile_effects;
pub(crate) mod compile_tables;
pub mod content;
pub(crate) mod hash;
pub mod save;
pub mod stats;
pub mod systems;
pub mod tables;
pub mod turn;
pub mod unit;
pub mod world;
pub mod world_size;
pub mod yields;

pub use advisors::ClassicSystem;
pub use change::ClassicChange;
pub use save::SavedClassic;
pub use turn::{ClassicCityStep, ClassicStep};
pub use world::ClassicWorld;

use kernel::module::Rejected;

use crate::command::validate;
use crate::derived::DerivedWorld;
use crate::ids::PlayerId;
use crate::module::RulesModule;
use crate::navigation::PathScratch;
use crate::rules::CompiledRuleset;
use crate::transaction::journal::ChangeJournal;
use crate::transaction::transaction::WorldTransaction;
use crate::turn::resolution::TurnSchedule;
use crate::world::state::{AuthWorld, GameView};

/// The classic rules module (the marker the kernel's types take).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub struct Classic;

/// The fingerprint of the current content (`CompiledRuleset::hash`).
pub const CLASSIC_FINGERPRINT: u64 = 14820141147641077048;

impl RulesModule for Classic {
    type World = ClassicWorld;
    type Vocabulary = effects::ClassicVocabulary;
    type Tables = tables::ClassicTables;
    type Yields = yields::ClassicYields;
    type Command = crate::command::Command;
    type Systems = systems::ClassicSystems;
    type Ai = crate::ai::AiDriver;
    type Step = ClassicStep;
    type CityStep = ClassicCityStep;

    const SCHEDULE: TurnSchedule<ClassicStep, ClassicCityStep> = turn::SCHEDULE;

    /// Snapshots of the content before the AI role list (one ordered
    /// list of policy branches in `roles.ron` instead of two), before the
    /// gameplay wave (civilization
    /// traits that were inert, city name lists, spy names), before the unit
    /// abilities (purchase rules, founding abroad, air slots, target
    /// priority), before the naval rules (embarkation traits, sight) and of
    /// the editions before it (before city names, before the module id,
    /// before role bindings) and the edition before the resources at sea
    /// (the rows differ in what the map generator places only) and the edition
    /// before them, with the AI role list of main (the same rows plus the AI branch list),
    /// and the edition before the rename (its module id and content keys carried an earlier
    /// name) load under
    /// the current content (`tests/rules_fingerprint.rs`).
    const EARLIER_EDITIONS: &'static [(u64, u64)] = &[
        (13258587229036033521, CLASSIC_FINGERPRINT),
        (2736398163960058391, CLASSIC_FINGERPRINT),
        (8423526412409906946, CLASSIC_FINGERPRINT),
        (1312007536065307279, CLASSIC_FINGERPRINT),
        (12001440589935184063, CLASSIC_FINGERPRINT),
        (8950411716607108964, CLASSIC_FINGERPRINT),
        (13783899014204876011, CLASSIC_FINGERPRINT),
        (9041359597632342489, CLASSIC_FINGERPRINT),
        (2151245367700835936, CLASSIC_FINGERPRINT),
        (16018243681386759787, CLASSIC_FINGERPRINT),
        (11011276917743099964, CLASSIC_FINGERPRINT),
        // Before the data was re-sourced from Unciv (docs/classic/RESOURCING.md).
        (10671736804218944378, CLASSIC_FINGERPRINT),
        // Before the formula pass and the Encyclopedia/Unciv checks of the catalogs (docs/classic/RESOURCING.md).
        (5756561721247808815, CLASSIC_FINGERPRINT),
            // Before the own trade route system (docs/classic/BNW-DESIGN.md)
        (18180021184894311762, CLASSIC_FINGERPRINT),
];

    fn execute(
        world: &mut AuthWorld,
        derived: &DerivedWorld,
        rules: &CompiledRuleset,
        command: crate::command::Command,
        scratch: &mut PathScratch,
    ) -> Result<ChangeJournal, Rejected<Classic>> {
        let rejected = |error| Rejected { error, partial: ChangeJournal::default() };
        let view = GameView { auth: world, derived };
        let resolved = validate(view, rules, command, scratch).map_err(rejected)?;
        let mut tx = WorldTransaction::new(world);
        if let Err(error) = tx.apply_resolved(&resolved, rules, scratch, derived) {
            let partial = std::mem::take(&mut tx.journal);
            return Err(Rejected { error, partial });
        }
        tx.commit_command(rules).map_err(rejected)
    }

    fn victor(world: &AuthWorld) -> Option<PlayerId> {
        world.module.victory.outcome().map(|outcome| outcome.winner)
    }

    fn is_over(world: &AuthWorld) -> bool {
        world.module.victory.outcome().is_some() && !world.module.victory.continued()
    }

    fn end_turn(player: PlayerId) -> crate::command::Command {
        crate::command::Command::EndTurn { player }
    }
}
