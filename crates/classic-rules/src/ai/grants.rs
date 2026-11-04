//! Free choices the player is owed (`OwnEconomy::free_technology_options`,
//! `free_great_person_options`; `derived::grants`): the AI takes them at the
//! start of its turn, one of each per turn (the next is observed again).
//!
//! ```text
//! free technology    the most expensive option (it saves the most
//!                    research), ties by technology order
//! free great person  the first option of: scientist, engineer, merchant,
//!                    writer, artist, musician
//! ```

use crate::command::Command;
use crate::knowledge::observation::AIObservation;
use crate::classic::effects::GreatPersonKind;
use crate::rules::CompiledRuleset;

/// Great person kinds the AI prefers, best first.
const GREAT_PERSON_PREFERENCE: [GreatPersonKind; 6] = [
    GreatPersonKind::Scientist,
    GreatPersonKind::Engineer,
    GreatPersonKind::Merchant,
    GreatPersonKind::Writer,
    GreatPersonKind::Artist,
    GreatPersonKind::Musician,
];

/// The commands that take the next owed free technology and great person.
pub fn free_choice_commands(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Vec<Command> {
    let mut out = Vec::new();
    let options = &obs.economy.free_technology_options;
    let best = options
        .iter()
        .copied()
        .max_by_key(|t| (rules.technologies.get(t.0 as usize).map(|d| d.cost).unwrap_or(0), std::cmp::Reverse(t.0)));
    if let Some(tech) = best {
        out.push(Command::ChooseFreeTechnology {
            player: obs.player,
            tech,
        });
    }
    let kinds = &obs.economy.free_great_person_options;
    let preferred = GREAT_PERSON_PREFERENCE.iter().find(|k| kinds.contains(k));
    if let Some(kind) = preferred.or(kinds.first()) {
        out.push(Command::ChooseFreeGreatPerson {
            player: obs.player,
            kind: *kind,
        });
    }
    out
}
