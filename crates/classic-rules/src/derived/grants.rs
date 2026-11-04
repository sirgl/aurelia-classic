//! Free choices owed to a player (`Modifier::Grant`, classic/docs/classic/rules.md): what the head of each list may be now. One set of rules
//! for validation, commit, the UI and the AI.
//!
//! ```text
//! free technology   Any: a technology the player does not know whose
//!                   prerequisites it knows;
//!                   KnownBy(p): one of those that p knows (Assyria)
//! free great person Any: a kind born from specialists (artist, engineer,
//!                   merchant, scientist, writer, musician; unverified);
//!                   LongCount: one the Long Count has not given yet, any
//!                   when all were given (Maya)
//! ```
//!
//! A great person appears in the capital: without a capital nothing can
//! be chosen.

use crate::command::CommandError;
use crate::ids::{PlayerId, TechId};
use crate::classic::effects::GreatPersonKind;
use crate::rules::CompiledRuleset;
use crate::world::player_economy::{FreeGreatPerson, FreeTechnology, OwedChoices};
use crate::world::state::AuthWorld;

/// Why a free choice is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChoiceBlock {
    NothingOwed,
    NotAnOption,
    NoCapital,
    UnknownGreatPerson,
}

impl ChoiceBlock {
    pub const ALL: [ChoiceBlock; 4] = [
        ChoiceBlock::NothingOwed,
        ChoiceBlock::NotAnOption,
        ChoiceBlock::NoCapital,
        ChoiceBlock::UnknownGreatPerson,
    ];

    pub fn reason(self) -> &'static str {
        match self {
            ChoiceBlock::NothingOwed => "no free choice owed",
            ChoiceBlock::NotAnOption => "not one of the options",
            ChoiceBlock::NoCapital => "no capital",
            ChoiceBlock::UnknownGreatPerson => "unknown great person",
        }
    }
}

impl From<ChoiceBlock> for CommandError {
    fn from(block: ChoiceBlock) -> Self {
        CommandError::RuleViolation(block.reason().into())
    }
}

/// The player's owed choices (empty for an unknown player).
pub fn owed(auth: &AuthWorld, player: PlayerId) -> OwedChoices {
    match auth.players().economy_of(player) {
        Some(economy) => economy.owed.clone(),
        None => OwedChoices::default(),
    }
}

/// Technologies `player` may research now (unknown, prerequisites known),
/// ascending.
pub fn researchable(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId) -> Vec<TechId> {
    let known = auth.players().techs_of(player);
    let mut out = Vec::new();
    for (i, def) in rules.technologies.iter().enumerate() {
        let tech = TechId(i as u16);
        if known.contains(&tech) {
            continue;
        }
        if def.requires.iter().all(|r| known.contains(r)) {
            out.push(tech);
        }
    }
    out
}

/// What one owed free technology may be, ascending.
pub fn technology_options(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    player: PlayerId,
    free: FreeTechnology,
) -> Vec<TechId> {
    match free {
        FreeTechnology::Any => researchable(auth, rules, player),
        FreeTechnology::KnownBy(other) => {
            crate::systems::espionage::stealable(auth, rules, player, other)
        }
    }
}

/// What the next free technology may be (empty: none owed).
pub fn next_technology_options(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId) -> Vec<TechId> {
    match owed(auth, player).technologies.first() {
        Some(free) => technology_options(auth, rules, player, *free),
        None => Vec::new(),
    }
}

/// The kinds a free great person may be (those born from specialists).
fn specialist_kinds(rules: &CompiledRuleset) -> Vec<GreatPersonKind> {
    let mut kinds: Vec<GreatPersonKind> = rules
        .module.great_people
        .iter()
        .filter(|d| d.specialist.is_some())
        .map(|d| d.kind)
        .collect();
    kinds.sort_by_key(|k| k.index());
    kinds.dedup();
    kinds
}

/// What one owed free great person may be, in kind order.
pub fn great_person_options(rules: &CompiledRuleset, owed: &OwedChoices, free: FreeGreatPerson) -> Vec<GreatPersonKind> {
    let kinds = specialist_kinds(rules);
    match free {
        FreeGreatPerson::Any => kinds,
        FreeGreatPerson::LongCount => {
            let left: Vec<GreatPersonKind> = kinds
                .iter()
                .copied()
                .filter(|k| owed.long_count_given & (1 << k.index()) == 0)
                .collect();
            if left.is_empty() {
                kinds
            } else {
                left
            }
        }
    }
}

/// What the next free great person may be (empty: none owed or no
/// capital to appear in).
pub fn next_great_person_options(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId) -> Vec<GreatPersonKind> {
    let owed = owed(auth, player);
    let Some(free) = owed.great_people.first() else {
        return Vec::new();
    };
    if crate::world::city::capital_of(auth, rules, player).is_none() {
        return Vec::new();
    }
    great_person_options(rules, &owed, *free)
}

/// May `player` take `tech` as its next free technology?
pub fn free_technology_status(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    player: PlayerId,
    tech: TechId,
) -> Result<(), ChoiceBlock> {
    let Some(free) = owed(auth, player).technologies.first().copied() else {
        return Err(ChoiceBlock::NothingOwed);
    };
    if !technology_options(auth, rules, player, free).contains(&tech) {
        return Err(ChoiceBlock::NotAnOption);
    }
    Ok(())
}

/// May `player` take a great person of `kind` as its next free one?
pub fn free_great_person_status(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    player: PlayerId,
    kind: GreatPersonKind,
) -> Result<(), ChoiceBlock> {
    let owed = owed(auth, player);
    let Some(free) = owed.great_people.first().copied() else {
        return Err(ChoiceBlock::NothingOwed);
    };
    if crate::world::city::capital_of(auth, rules, player).is_none() {
        return Err(ChoiceBlock::NoCapital);
    }
    if !great_person_options(rules, &owed, free).contains(&kind) {
        return Err(ChoiceBlock::NotAnOption);
    }
    Ok(())
}

/// The city's tile bears a nation's unique luxury (Indonesia's
/// cities "can never be razed").
pub fn holds_unique_luxury(auth: &AuthWorld, rules: &CompiledRuleset, city: &crate::world::city::CityCore) -> bool {
    let Some(Some(resource)) = auth.map().resource(city.tile) else {
        return false;
    };
    rules
        .resources
        .get(resource.0 as usize)
        .is_some_and(|r| r.nation.is_some())
}
