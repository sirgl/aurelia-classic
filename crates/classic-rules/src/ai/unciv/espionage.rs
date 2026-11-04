// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Derived from Unciv (https://github.com/yairm210/Unciv), tag 4.22.10, MPL-2.0:
// core/src/com/unciv/logic/automation/unit/EspionageAutomation.kt.

//! Unciv's spies: one guards the capital; an idle spy is sent (by a random
//! draw of ten) to steal from the nation it spies on least, to rig the
//! election of the city-state where our influence is highest, or to counter
//! intelligence in our city of most science; a coup is staged at a 70 % chance
//! one time in five. The cities a spy may go to are those the player sees
//! now (the rules' order names a city by its handle).

use crate::ai::planner::Decision;
use crate::ai::trace::{Actor, Reason};
use crate::command::Command;
use crate::ids::CityHandle;
use crate::knowledge::observation::AIObservation;
use crate::knowledge::public::PlayerQueries;
use crate::world::espionage::SpyState;
use crate::world::player::{ClassicKind, PlayerKind};

fn act(command: Command) -> Decision {
    Decision::act(Actor::Player, Reason::SystemAdvisor, command)
}

/// `automateSpies` and the thefts the rules leave to a command.
pub fn automate_spies(obs: &AIObservation<'_>, rules: &crate::rules::CompiledRuleset) -> Vec<Decision> {
    let mut out = Vec::new();
    let player = obs.player;
    let capital = obs.view.own_capital();
    let spies = &obs.spies;
    let mut moved_to: Vec<CityHandle> = spies.iter().filter_map(|s| s.city).collect();
    let mut capital_guarded = spies.iter().any(|s| s.city.is_some() && s.city == capital);
    let is_major = |p| obs.relation(p).is_some_and(|r| r.kind == PlayerKind::Major);
    let is_city_state = |p| obs.relation(p).is_some_and(|r| matches!(r.kind, PlayerKind::Module(ClassicKind::CityState(_))));
    let mv = |spy: u8, city: CityHandle, diplomat: bool| Command::MoveSpy { player, spy, city: Some(city), diplomat };
    for spy in spies.iter().filter(|s| matches!(s.state, SpyState::Unassigned)) {
        let seed = spies.len() as u64 + u64::from(spy.index) + u64::from(obs.round);
        let roll = super::next_turn::draw(obs.round, seed, 0x7370, 10);
        let mut options: Vec<Command> = Vec::new();
        if let Some(c) = capital.filter(|_| !capital_guarded) {
            options.push(mv(spy.index, c, false));
        } else {
            let steal = obs
                .visible_foreign_cities
                .iter()
                .filter(|c| is_major(c.owner) && !moved_to.contains(&c.handle))
                .max_by_key(|c| (c.population, std::cmp::Reverse(c.handle.index)))
                .map(|c| mv(spy.index, c.handle, false));
            let rig = obs
                .city_states
                .iter()
                .filter(|cs| !cs.at_war && (cs.influence < 150 || cs.ally != Some(player)))
                .filter_map(|cs| obs.visible_foreign_cities.iter().find(|c| c.owner == cs.player && is_city_state(c.owner) && !moved_to.contains(&c.handle)).map(|c| (cs.influence, c.handle)))
                .max_by_key(|(i, h)| (*i, std::cmp::Reverse(h.index)))
                .map(|(_, h)| mv(spy.index, h, false));
            let counter = obs.own_cities.iter().filter(|c| !moved_to.contains(&c.handle)).max_by_key(|c| (c.income.science, c.id)).map(|c| mv(spy.index, c.handle, false));
            if roll <= 7 {
                options.extend(steal.clone());
            }
            if roll <= 9 {
                options.extend(rig.clone());
            }
            options.extend(counter);
            options.extend(steal);
            options.extend(rig);
        }
        if let Some(command) = options.into_iter().find(|c| obs.view.accepts(c)) {
            if let Command::MoveSpy { city: Some(c), .. } = &command {
                moved_to.push(*c);
                if Some(*c) == capital {
                    capital_guarded = true;
                }
            }
            out.push(act(command));
        }
    }
    // `checkIfShouldStageCoup`.
    for spy in spies {
        if spy.coup_chance.is_some_and(|c| c >= 70) && super::next_turn::draw(obs.round, u64::from(spy.index) + 7, 0x6370, 100) < 20 {
            let command = Command::StageCoup { player, spy: spy.index };
            if obs.view.accepts(&command) {
                out.push(act(command));
            }
        }
    }
    // A theft the rules leave to a command: the most expensive technology.
    for (victim, techs) in &obs.steal {
        if let Some(tech) = techs.iter().copied().max_by_key(|t| (rules.technologies[t.0 as usize].cost, t.0)) {
            let command = Command::StealTechnology { player, from: *victim, tech };
            if obs.view.accepts(&command) {
                out.push(act(command));
            }
        }
    }
    out
}
