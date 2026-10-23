//! AI of great works, concerts and ideology (track B, `ClassicSystem::Culture`):
//! reads `AIObservation::tourism` (`derived::tourism::TourismObservation`:
//! slots, influence, public opinion) and the own units; keeps no state.
//!
//! - a Great Writer, Artist or Musician fills a free slot of its kind: it
//!   creates the work where it stands next to an own city, else walks to
//!   the nearest own city;
//! - without a free slot a writer writes its treatise (culture), a musician
//!   walks into the land of a civilization it may enter and plays its
//!   concert there; an artist waits (golden ages are track A);
//! - an archaeologist digs the nearest open antiquity site; a finished dig
//!   becomes an artifact, else a writing, a Landmark, a culture boost;
//! - an upheaval makes the player switch to the preferred ideology.

use crate::ai::system::{AdvisorInput, Proposal, SystemAdvisor};
use crate::command::Command;
use crate::derived::archaeology::ArchaeologyChoice;
use crate::derived::tourism::work_class;
use crate::ids::{PlayerId, TileId};
use crate::knowledge::observation::{AIObservation, OwnUnit};
use crate::rules::content::UnitAbility;
use crate::rules::CompiledRuleset;
use crate::world::tourism::{GreatWorkClass, PublicOpinion};

#[derive(Debug, Clone, Copy, Default)]
pub struct TourismAdvisor;

impl SystemAdvisor for TourismAdvisor {
    fn end_of_turn(&self, input: &AdvisorInput<'_, '_>) -> Vec<Proposal> {
        tourism_commands(input.observation, input.rules)
            .into_iter()
            .map(|command| Proposal {
                priority: 15,
                command,
            })
            .collect()
    }
}

/// The commands of the turn: one per great person of culture, and the
/// ideology switch.
pub fn tourism_commands(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Vec<Command> {
    let mut out: Vec<Command> = obs
        .own_units
        .iter()
        .filter_map(|u| {
            great_person_command(obs, rules, u).or_else(|| archaeologist_command(obs, rules, u))
        })
        .collect();
    out.extend(obs.tourism.pending.iter().filter_map(|(tile, open)| {
        let choice = ARCHAEOLOGY_PREFERENCE
            .into_iter()
            .find(|c| open.contains(c))?;
        Some(Command::ChooseArchaeology {
            player: obs.player,
            tile: *tile,
            choice,
        })
    }));
    if obs.tourism.opinion == PublicOpinion::Upheaval && obs.tourism.preferred.is_some() {
        out.push(Command::SwitchIdeology { player: obs.player });
    }
    out
}

fn great_person_command(
    obs: &AIObservation<'_>,
    rules: &CompiledRuleset,
    unit: &OwnUnit,
) -> Option<Command> {
    let class = work_class(rules, unit.kind)?;
    if has_free_slot(obs, class) {
        return Some(create_or_walk(obs, unit));
    }
    match class {
        GreatWorkClass::Literature if obs.culture.per_turn > 0 => {
            Some(Command::WriteTreatise { unit: unit.handle })
        }
        GreatWorkClass::Music => concert_or_walk(obs, unit),
        _ => None,
    }
}

/// Artifacts first (tourism and culture now), then the writing, then a
/// Landmark, then the culture boost.
const ARCHAEOLOGY_PREFERENCE: [ArchaeologyChoice; 5] = [
    ArchaeologyChoice::ArtifactFirst,
    ArchaeologyChoice::ArtifactSecond,
    ArchaeologyChoice::Writing,
    ArchaeologyChoice::Landmark,
    ArchaeologyChoice::CultureBoost,
];

/// An archaeologist digs the open site it stands on, else walks to the
/// nearest open site on land it may enter; it waits while digging.
fn archaeologist_command(
    obs: &AIObservation<'_>,
    rules: &CompiledRuleset,
    unit: &OwnUnit,
) -> Option<Command> {
    let digs = rules
        .units
        .get(unit.kind.0 as usize)
        .is_some_and(|u| u.module.abilities.contains(&UnitAbility::Excavates));
    if !digs || obs.tourism.digging.contains(&unit.tile) {
        return None;
    }
    if obs.tourism.sites.contains(&unit.tile) {
        return Some(Command::Excavate { unit: unit.handle });
    }
    let d = dims(obs);
    let enterable = |t: &TileId| {
        let owner = obs.view.tile(*t).snapshot().and_then(|s| s.owner);
        owner.is_none_or(|o| o == obs.player || open_to(obs, o))
    };
    let site = obs
        .tourism
        .sites
        .iter()
        .filter(|t| enterable(t))
        .min_by_key(|t| (d.distance(**t, unit.tile).unwrap_or(i32::MAX), t.0))?;
    Some(Command::MarchTo {
        unit: unit.handle,
        destination: *site,
    })
}

fn open_to(obs: &AIObservation<'_>, owner: PlayerId) -> bool {
    obs.relations
        .iter()
        .any(|r| r.other == owner && r.they_grant_entry && !r.at_war)
}

fn has_free_slot(obs: &AIObservation<'_>, class: GreatWorkClass) -> bool {
    obs.tourism
        .slots
        .iter()
        .any(|(slot, filled, total)| *slot == class.slot() && filled < total)
}

fn dims(obs: &AIObservation<'_>) -> crate::math::MapDimensions {
    crate::math::MapDimensions {
        width: obs.map_width,
        height: obs.map_height,
    }
}

/// Create the work next to an own city, else walk to the nearest one.
fn create_or_walk(obs: &AIObservation<'_>, unit: &OwnUnit) -> Command {
    let d = dims(obs);
    let nearest = obs
        .own_cities
        .iter()
        .min_by_key(|c| (d.distance(c.tile, unit.tile).unwrap_or(i32::MAX), c.tile.0));
    match nearest {
        Some(city) if d.distance(city.tile, unit.tile).is_some_and(|x| x <= 1) => {
            Command::CreateGreatWork { unit: unit.handle }
        }
        Some(city) => Command::MarchTo {
            unit: unit.handle,
            destination: city.tile,
        },
        None => Command::CreateGreatWork { unit: unit.handle },
    }
}

/// Play the concert on land of a civilization the player met, else walk
/// next to the nearest known city of one it may enter.
fn concert_or_walk(obs: &AIObservation<'_>, unit: &OwnUnit) -> Option<Command> {
    let owner = obs.view.tile(unit.tile).snapshot().and_then(|s| s.owner);
    let met = |p: PlayerId| {
        obs.relations
            .iter()
            .any(|r| r.other == p && r.contact.met_round.is_some())
    };
    if owner.is_some_and(|o| o != obs.player && met(o)) {
        return Some(Command::PerformConcert { unit: unit.handle });
    }
    let destination = foreign_city_edge(obs, unit.tile)?;
    Some(Command::MarchTo {
        unit: unit.handle,
        destination,
    })
}

/// A tile next to the nearest known foreign city whose owner lets the
/// player in.
fn foreign_city_edge(obs: &AIObservation<'_>, from: TileId) -> Option<TileId> {
    let d = dims(obs);
    let open = |p: PlayerId| open_to(obs, p);
    let city = obs
        .faith
        .cities
        .iter()
        .filter(|c| c.owner != obs.player && open(c.owner))
        .min_by_key(|c| (d.distance(c.tile, from).unwrap_or(i32::MAX), c.tile.0))?;
    d.neighbors(city.tile)
        .into_iter()
        .min_by_key(|t| (d.distance(*t, from).unwrap_or(i32::MAX), t.0))
}
