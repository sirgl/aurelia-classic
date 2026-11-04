//! Production processes (Wealth and Research):
//! a city with nothing in its queue and a process set turns a quarter of its
//! production into gold or science instead of building its default unit.
//! The process is unlocked by a technology (`core:process/<slug>` in its
//! `unlocks_actions`). Journal: none (no derived system reads it; the
//! turn's gold and science take the converted yield).

use crate::command::{CommandError, ResolvedCommand};
use crate::ids::{CityHandle, PlayerId};
use crate::rules::CompiledRuleset;
use crate::transaction::transaction::WorldTransaction;
use crate::world::city_economy::Process;
use crate::world::state::AuthWorld;

/// Share (percent) of a city's production that a process turns into gold or
/// science. Own value, not compared with a public source (unverified).
pub const PROCESS_PERCENT: i32 = 25;

/// Gold and science a city's `production` gives under `process`.
pub fn process_yield(process: Option<Process>, production: i32) -> (i32, i32) {
    let converted = production.max(0) * PROCESS_PERCENT / 100;
    match process {
        Some(Process::Wealth) => (converted, 0),
        Some(Process::Research) => (0, converted),
        // The production itself goes to the project (`contribute_to_project`).
        Some(Process::Contribute(_)) | None => (0, 0),
    }
}

pub fn validate_process(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    player: PlayerId,
    city: CityHandle,
    process: Option<Process>,
) -> Result<ResolvedCommand, CommandError> {
    let core = auth.cities().get(city).ok_or(CommandError::UnknownCity)?;
    if core.owner != player {
        return Err(CommandError::RuleViolation("not your city".into()));
    }
    if core.economy.process == process {
        return Err(CommandError::RuleViolation("process already set".into()));
    }
    let Some(process) = process else {
        return Ok(ResolvedCommand::SetProcess { player, city, process: None });
    };
    let open = match process.role() {
        Some(role) => {
            let tech = rules.roles.process_tech(role);
            tech.is_some_and(|t| auth.players().techs_of(player).contains(&t))
        }
        // A World Congress project takes contributions while it is under
        // way, from nations (city-states are members without a say).
        None => {
            let Process::Contribute(project) = process else {
                unreachable!("only league projects have no role")
            };
            auth.module().congress().project(project).is_some() && auth.players().is_major(player)
        }
    };
    if !open {
        return Err(CommandError::RuleViolation("process locked".into()));
    }
    let process = Some(process);
    Ok(ResolvedCommand::SetProcess {
        player,
        city,
        process,
    })
}

impl<'a> WorldTransaction<'a> {
    pub(crate) fn set_process(
        &mut self,
        player: PlayerId,
        city: CityHandle,
        process: Option<Process>,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        validate_process(self.world, rules, player, city, process)?;
        let core = self
            .world
            .cities
            .get_mut(city)
            .ok_or(CommandError::UnknownCity)?;
        core.economy.process = process;
        Ok(())
    }
}
