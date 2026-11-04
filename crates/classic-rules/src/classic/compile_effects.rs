//! The classic part of effect compilation (`rules::compile_effects` asks it
//! through `rules::vocabulary::Vocabulary`): the module's words resolved
//! from content and checked against the scope, the source and the trigger
//! of their effect.

use crate::classic::effects::{
    ClassicCondition, ClassicGrant, ClassicModifier, ClassicScope, ClassicSelector, ClassicSourceModifier,
    ClassicTable, ClassicTrigger,
};
use crate::ids::BuildingKindId;
use crate::rules::compile::RulesError;
use crate::rules::compile_effects::SourceKind;
use crate::rules::content::{ContentTable, EraId};
use crate::rules::effects::{CompiledEffect, Count, Grant, Modifier, PerCount, Scope, Trigger};
use crate::rules::key_index::KeyIndex;

const POLICY: SourceKind = SourceKind::Module(ClassicTable::Policy);
const BELIEF: SourceKind = SourceKind::Module(ClassicTable::Belief);

pub(crate) fn resolve_modifier(ix: &KeyIndex, source: &ClassicSourceModifier) -> Result<ClassicModifier, RulesError> {
    use ClassicSourceModifier as S;
    Ok(match *source {
        S::AddHappiness { amount } => ClassicModifier::AddHappiness { amount },
        S::AddGreatPersonPoints { kind, amount } => ClassicModifier::AddGreatPersonPoints { kind, amount },
        S::ModifyCityState { stat, amount } => ClassicModifier::ModifyCityState { stat, amount },
        S::FreePolicies { count } => ClassicModifier::FreePolicies { count },
        S::AddYieldPerFollowingCity { yield_kind, amount, foreign_only } => {
            ClassicModifier::AddYieldPerFollowingCity { yield_kind, amount, foreign_only }
        }
        S::AddYieldPerFollowers { yield_kind, per, foreign_only } => {
            ClassicModifier::AddYieldPerFollowers { yield_kind, per, foreign_only }
        }
        S::AddHappinessPerFollowingCity { hundredths } => {
            ClassicModifier::AddHappinessPerFollowingCity { hundredths }
        }
        S::ModifyReligion { stat, percent } => ClassicModifier::ModifyReligion { stat, percent },
        S::UnlockFaithBuilding(ref k) => {
            ClassicModifier::UnlockFaithBuilding(BuildingKindId(ix.id(ContentTable::Building, &k.0)?))
        }
        S::UnlockFaithUnits(ref k) => ClassicModifier::UnlockFaithUnits(EraId(ix.id(ContentTable::Era, &k.0)?)),
        S::AllowReformationBelief => ClassicModifier::AllowReformationBelief,
        S::ModifyTourism { stat, amount } => ClassicModifier::ModifyTourism { stat, amount },
        S::TourismToward { when, percent } => ClassicModifier::TourismToward { when, percent },
        S::AddHappinessPerCount { amount, per } => ClassicModifier::AddHappinessPerCount { amount, per },
        S::GrantSpies { count } => ClassicModifier::GrantSpies { count },
        S::PromoteSpies { ranks } => ClassicModifier::PromoteSpies { ranks },
        S::GrantReligionAbility(ability) => ClassicModifier::GrantReligionAbility(ability),
        S::ExtraFoundingBelief => ClassicModifier::ExtraFoundingBelief,
        S::SecondReligionPantheon => ClassicModifier::SecondReligionPantheon,
        S::InstantTourism { amount } => ClassicModifier::InstantTourism { amount },
        S::GrantExperiencePerGreatWork(amount) => ClassicModifier::GrantExperiencePerGreatWork(amount),
    })
}

/// Belief selectors address the belief's religion and its followers.
pub(crate) fn selector_fits(_selector: ClassicSelector, kind: SourceKind, effect: &CompiledEffect) -> bool {
    kind == BELIEF && effect.trigger == Trigger::Passive
}

pub(crate) fn condition_fits(condition: ClassicCondition, kind: SourceKind, effect: &CompiledEffect) -> bool {
    let scope = effect.scope;
    match condition {
        // The opponent of a kill is its victim, whatever the scope.
        ClassicCondition::OpponentIsBarbarian => {
            scope == Scope::Unit || effect.trigger == Trigger::OnUnitKilled
        }
        // Combat only: a kill names its victim's kind, not its owner's size.
        ClassicCondition::OpponentIsCityState | ClassicCondition::OpponentHasMoreCities => scope == Scope::Unit,
        ClassicCondition::MinFollowers(_) => matches!(scope, Scope::City | Scope::Tile),
        // The owner's creed: only a belief reaches a unit through it.
        ClassicCondition::NearFollowingCity(_) => scope == Scope::Unit && kind == BELIEF,
        ClassicCondition::GoldenAge => true,
        // The owner's history: its units (and nothing else) read it.
        ClassicCondition::GreatGeneralEarned => scope == Scope::Unit,
        // The kind of the great person of the event, read at firing.
        ClassicCondition::GreatPersonIs(_) => {
            scope == Scope::Player && effect.trigger == Trigger::Module(ClassicTrigger::OnGreatPersonBorn)
        }
    }
}

pub(crate) fn modifier_fits_scope(modifier: ClassicModifier, scope: Scope) -> bool {
    const RELIGION: Scope = Scope::Module(ClassicScope::Religion);
    match modifier {
        ClassicModifier::AddYieldPerFollowingCity { yield_kind, .. }
        | ClassicModifier::AddYieldPerFollowers { yield_kind, .. } => match scope {
            Scope::Player => yield_kind.is_player_yield(),
            Scope::City => true,
            _ => false,
        },
        ClassicModifier::AddHappinessPerFollowingCity { .. }
        | ClassicModifier::AllowReformationBelief
        | ClassicModifier::TourismToward { .. }
        | ClassicModifier::ModifyCityState { .. }
        | ClassicModifier::FreePolicies { .. }
        | ClassicModifier::GrantSpies { .. }
        | ClassicModifier::PromoteSpies { .. }
        | ClassicModifier::InstantTourism { .. }
        | ClassicModifier::ExtraFoundingBelief
        | ClassicModifier::SecondReligionPantheon => scope == Scope::Player,
        ClassicModifier::GrantExperiencePerGreatWork(_) => scope == Scope::Unit,
        ClassicModifier::ModifyReligion { .. } | ClassicModifier::GrantReligionAbility(_) => scope == RELIGION,
        ClassicModifier::ModifyTourism { stat, .. } => stat.fits(scope),
        ClassicModifier::UnlockFaithBuilding(_)
        | ClassicModifier::UnlockFaithUnits(_)
        | ClassicModifier::AddGreatPersonPoints { .. } => scope == Scope::City,
        ClassicModifier::AddHappiness { .. } => matches!(scope, Scope::Player | Scope::City),
        ClassicModifier::AddHappinessPerCount { per, .. } => {
            matches!(scope, Scope::Player | Scope::City) && per.count.fits(scope)
        }
    }
}

pub(crate) fn trigger_fits(trigger: ClassicTrigger, kind: SourceKind) -> bool {
    match trigger {
        ClassicTrigger::OnAdopt => kind == POLICY,
        // The religion's beliefs fire for its founder or the spreading unit.
        ClassicTrigger::OnCityConverted | ClassicTrigger::OnReligionSpread => kind == BELIEF,
        // The great person is gone: its kind and promotions cannot fire.
        ClassicTrigger::OnGreatPersonExpended => !matches!(kind, SourceKind::Unit | SourceKind::Promotion),
        // Owner-level sources: a unit or a building has no say in a birth.
        ClassicTrigger::OnGreatPersonBorn => {
            matches!(kind, SourceKind::Nation | SourceKind::Technology) || kind == POLICY
        }
        ClassicTrigger::OnNaturalWonderFoundFirst
        | ClassicTrigger::OnNaturalWonderFound
        | ClassicTrigger::OnLongCountCycle => true,
    }
}

/// The step of any per-count modifier, the kernel's or the module's.
pub(crate) fn per_count(modifier: &Modifier) -> Option<PerCount> {
    match *modifier {
        Modifier::Module(ClassicModifier::AddHappinessPerCount { per, .. }) => Some(per),
        _ => crate::rules::compile_effects::per_count(modifier),
    }
}

pub(crate) fn check_effect(kind: SourceKind, effect: &CompiledEffect) -> Result<(), RulesError> {
    let invalid = |reason: &str| Err(RulesError::InvalidModifier(reason.into()));
    let per = per_count(&effect.modifier);
    if per.is_some_and(|per| per.every == 0) {
        return invalid("a per-count modifier needs every > 0");
    }
    let spread_count = per.is_some_and(|per| matches!(per.count, Count::Module(c) if c.needs_spread_event()));
    if spread_count && effect.trigger != Trigger::Module(ClassicTrigger::OnReligionSpread) {
        return invalid("this count is read on the city of OnReligionSpread only");
    }
    if zero_count(effect.modifier) {
        return invalid("a grant needs count > 0");
    }
    // Its index lists policies only (`ClassicIndex::second_pantheon_policies`).
    let second_pantheon = effect.modifier == Modifier::Module(ClassicModifier::SecondReligionPantheon);
    if second_pantheon && kind != POLICY {
        return invalid("SecondReligionPantheon belongs to a policy");
    }
    Ok(())
}

/// A grant of nothing.
fn zero_count(modifier: Modifier) -> bool {
    matches!(
        modifier,
        Modifier::Module(
            ClassicModifier::FreePolicies { count: 0 }
                | ClassicModifier::GrantSpies { count: 0 }
                | ClassicModifier::PromoteSpies { ranks: 0 }
                | ClassicModifier::InstantTourism { amount: 0 }
                | ClassicModifier::GrantExperiencePerGreatWork(0)
        ) | Modifier::Grant(Grant::Module(
            ClassicGrant::FreeGreatPeople(0) | ClassicGrant::GoldenAge { turns: 0 }
        ))
    )
}
