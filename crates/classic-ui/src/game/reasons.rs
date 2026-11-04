//! Why a control is blocked, in words for the player (Russian, short enough for a line under a
//! button). Every typed reason of the rules (`Refusal` and the `*Block` enums of the derived
//! rules) has exactly one text here; the matches are exhaustive, so a new reason cannot reach the
//! screen unworded. Reasons the rules still give as rule text stay in English (`names::reason`).
use classic_rules::derived::culture::{BranchBlock, IdeologyBlock, PolicyBlock};
use classic_rules::derived::religion::{FaithBlock, FaithPurchaseBlock};
use classic_rules::world::espionage::SpyBlock;
use classic_rules::world::relations::DealBlock;
use classic_rules::command::{Refusal, SiteProblem};
use classic_rules::rules::CompiledRuleset;

use i18n::{t, tf};

use super::names;

/// Reasons the screens know without the rules (state they read themselves).
pub fn full_health() -> &'static str {
    t!("classic.game.reasons.full-health")
}
pub fn not_for_this_person() -> &'static str {
    t!("classic.game.reasons.not-for-this-person")
}
pub fn no_trade_target() -> &'static str {
    t!("classic.game.reasons.no-trade-target")
}
pub fn no_moves() -> &'static str {
    t!("classic.game.reasons.no-moves")
}
pub fn free_tech_first() -> &'static str {
    t!("classic.game.reasons.free-tech-first")
}
pub fn queue_empty() -> &'static str {
    t!("classic.game.reasons.queue-empty")
}
pub fn all_assigned() -> &'static str {
    t!("classic.game.reasons.all-assigned")
}
pub fn city_chooses() -> &'static str {
    t!("classic.game.reasons.city-chooses")
}
pub fn no_manual_specialist() -> &'static str {
    t!("classic.game.reasons.no-manual-specialist")
}
pub fn no_pantheon_belief() -> &'static str {
    t!("classic.game.reasons.no-pantheon-belief")
}
/// The hint on a rename button with nothing to choose.
pub fn no_free_names() -> &'static str {
    t!("classic.game.reasons.no-free-names")
}
pub fn not_your_city() -> &'static str {
    t!("classic.game.reasons.not-your-city")
}
pub fn no_coup() -> &'static str {
    t!("classic.game.reasons.no-coup")
}
pub fn nothing_to_build() -> &'static str {
    t!("classic.game.reasons.nothing-to-build")
}

pub fn refusal(rules: &CompiledRuleset, refusal: Refusal) -> String {
    match refusal {
        Refusal::NoMovesLeft => no_moves().into(),
        Refusal::ClosedBorders => t!("classic.game.reasons.closed-borders").into(),
        Refusal::AlreadyFortified => t!("classic.game.reasons.already-fortified").into(),
        Refusal::NotEnoughGold { price, treasury } => tf!("classic.game.reasons.not-enough-gold", price = price, treasury = treasury),
        Refusal::NothingQueued => queue_empty().into(),
        Refusal::TileNotForSale => t!("classic.game.reasons.tile-not-for-sale").into(),
        Refusal::AlreadyWorking => t!("classic.game.reasons.already-working").into(),
        Refusal::Site(problem) => site(problem).into(),
        Refusal::NameTaken => t!("classic.game.reasons.name-taken").into(),
        Refusal::NeedsTechnology(tech) => tf!("classic.game.reasons.needs-technology", tech = names::tech(rules, tech)),
        Refusal::AlreadyBuilt => t!("classic.game.reasons.already-built").into(),
        Refusal::AlreadyQueued => t!("classic.game.reasons.already-queued").into(),
        Refusal::QueuedElsewhere => t!("classic.game.reasons.queued-elsewhere").into(),
        Refusal::WonderBuilt => t!("classic.game.reasons.wonder-built").into(),
        Refusal::OtherNation => t!("classic.game.reasons.other-nation").into(),
        Refusal::NotProducible => t!("classic.game.reasons.not-producible").into(),
        Refusal::ReplacedByUniqueUnit => t!("classic.game.reasons.replaced-by-unique-unit").into(),
    }
}

pub fn site(problem: SiteProblem) -> &'static str {
    match problem {
        SiteProblem::Unsettleable => t!("classic.game.reasons.site-unsettleable"),
        SiteProblem::ForeignTerritory => t!("classic.game.reasons.site-foreign-territory"),
        SiteProblem::TooClose => t!("classic.game.reasons.site-too-close"),
    }
}

pub fn policy(block: PolicyBlock) -> &'static str {
    match block {
        PolicyBlock::UnknownPlayer | PolicyBlock::UnknownPolicy => t!("classic.game.reasons.unavailable"),
        PolicyBlock::NotChoosable => t!("classic.game.reasons.policy-not-choosable"),
        PolicyBlock::BranchLocked => t!("classic.game.reasons.policy-branch-locked"),
        PolicyBlock::AlreadyAdopted => t!("classic.game.reasons.policy-already-adopted"),
        PolicyBlock::MissingPrerequisite => t!("classic.game.reasons.policy-missing-prerequisite"),
        PolicyBlock::OtherIdeology => t!("classic.game.reasons.policy-other-ideology"),
        PolicyBlock::TierLocked => t!("classic.game.reasons.policy-tier-locked"),
        PolicyBlock::NotEnoughCulture => t!("classic.game.reasons.not-enough-culture"),
    }
}

pub fn branch(block: BranchBlock) -> &'static str {
    match block {
        BranchBlock::UnknownPlayer | BranchBlock::UnknownBranch => t!("classic.game.reasons.unavailable"),
        BranchBlock::Ideology => t!("classic.game.reasons.branch-ideology"),
        BranchBlock::AlreadyUnlocked => t!("classic.game.reasons.branch-already-unlocked"),
        BranchBlock::EraTooEarly => t!("classic.game.reasons.branch-era-too-early"),
        BranchBlock::Excluded => t!("classic.game.reasons.branch-excluded"),
        BranchBlock::NotEnoughCulture => t!("classic.game.reasons.not-enough-culture"),
    }
}

pub fn ideology(block: IdeologyBlock) -> &'static str {
    match block {
        IdeologyBlock::UnknownPlayer | IdeologyBlock::NotAnIdeology => t!("classic.game.reasons.unavailable"),
        IdeologyBlock::AlreadyChosen => t!("classic.game.reasons.ideology-already-chosen"),
        IdeologyBlock::TooEarly => t!("classic.game.reasons.ideology-too-early"),
    }
}

pub fn faith(block: FaithBlock) -> &'static str {
    match block {
        FaithBlock::UnknownPlayer | FaithBlock::UnknownUnit | FaithBlock::NotYourUnit => t!("classic.game.reasons.unavailable"),
        FaithBlock::NotAProphet => t!("classic.game.reasons.faith-not-a-prophet"),
        FaithBlock::NotAMissionary => t!("classic.game.reasons.faith-not-a-missionary"),
        FaithBlock::NotAnInquisitor => t!("classic.game.reasons.faith-not-an-inquisitor"),
        FaithBlock::AlreadyHasPantheon => t!("classic.game.reasons.faith-already-has-pantheon"),
        FaithBlock::AlreadyHasReligion => t!("classic.game.reasons.faith-already-has-religion"),
        FaithBlock::NoReligion => t!("classic.game.reasons.faith-no-religion"),
        FaithBlock::AlreadyEnhanced => t!("classic.game.reasons.faith-already-enhanced"),
        FaithBlock::NotEnoughFaith => t!("classic.game.reasons.not-enough-faith"),
        FaithBlock::NoPantheonsLeft => t!("classic.game.reasons.faith-no-pantheons-left"),
        FaithBlock::NoReligionsLeft => t!("classic.game.reasons.faith-no-religions-left"),
        FaithBlock::ReligionTaken => t!("classic.game.reasons.faith-religion-taken"),
        FaithBlock::BeliefTaken => t!("classic.game.reasons.faith-belief-taken"),
        FaithBlock::WrongBeliefCategory => t!("classic.game.reasons.faith-wrong-belief-category"),
        FaithBlock::NeedsPantheonBelief => t!("classic.game.reasons.faith-needs-pantheon-belief"),
        FaithBlock::NotInOwnCity => t!("classic.game.reasons.faith-not-in-own-city"),
        FaithBlock::NotNearCity => t!("classic.game.reasons.faith-not-near-city"),
        FaithBlock::NoSpreadsLeft => t!("classic.game.reasons.faith-no-spreads-left"),
        FaithBlock::AlreadyFollows => t!("classic.game.reasons.faith-already-follows"),
        FaithBlock::NoReformation => t!("classic.game.reasons.faith-no-reformation"),
        FaithBlock::AlreadyReformed => t!("classic.game.reasons.faith-already-reformed"),
        FaithBlock::NothingToRemove => t!("classic.game.reasons.faith-nothing-to-remove"),
        FaithBlock::NotAGreatPerson => t!("classic.game.reasons.faith-not-a-great-person"),
        FaithBlock::NotOwnLand => t!("classic.game.reasons.faith-not-own-land"),
        FaithBlock::WrongTerrain => t!("classic.game.reasons.faith-wrong-terrain"),
        FaithBlock::NoBonusBelief => t!("classic.game.reasons.faith-no-bonus-belief"),
        FaithBlock::SpreadProhibited => t!("classic.game.reasons.faith-spread-prohibited"),
    }
}

pub fn faith_purchase(block: FaithPurchaseBlock) -> &'static str {
    match block {
        FaithPurchaseBlock::UnknownCity | FaithPurchaseBlock::UnknownItem => t!("classic.game.reasons.unavailable"),
        FaithPurchaseBlock::NotYourCity => not_your_city(),
        FaithPurchaseBlock::NoFaithPrice => t!("classic.game.reasons.purchase-no-faith-price"),
        FaithPurchaseBlock::TechnologyMissing => t!("classic.game.reasons.purchase-technology-missing"),
        FaithPurchaseBlock::NeedsReligion => t!("classic.game.reasons.purchase-needs-religion"),
        FaithPurchaseBlock::NeedsEnhancedReligion => t!("classic.game.reasons.purchase-needs-enhanced-religion"),
        FaithPurchaseBlock::NeedsBelief => t!("classic.game.reasons.purchase-needs-belief"),
        FaithPurchaseBlock::NeedsBranch => t!("classic.game.reasons.purchase-needs-branch"),
        FaithPurchaseBlock::TooEarly => t!("classic.game.reasons.purchase-too-early"),
        FaithPurchaseBlock::AlreadyBuilt => t!("classic.game.reasons.already-built"),
        FaithPurchaseBlock::MissingBuilding => t!("classic.game.reasons.purchase-missing-building"),
        FaithPurchaseBlock::OtherNation => t!("classic.game.reasons.purchase-other-nation"),
        FaithPurchaseBlock::NotEnoughFaith => t!("classic.game.reasons.not-enough-faith"),
    }
}

pub fn deal(block: DealBlock) -> &'static str {
    match block {
        DealBlock::NotOurDeal => t!("classic.game.reasons.deal-not-our-deal"),
        DealBlock::GoldNeedsFriendship => t!("classic.game.reasons.deal-gold-needs-friendship"),
        DealBlock::NotEnoughGold => t!("classic.game.reasons.not-enough-gold-short"),
        DealBlock::AtWar => t!("classic.game.reasons.at-war"),
        DealBlock::NotEnoughIncome => t!("classic.game.reasons.deal-not-enough-income"),
        DealBlock::NotTradeable => t!("classic.game.reasons.deal-not-tradeable"),
        DealBlock::NotEnoughResource => t!("classic.game.reasons.deal-not-enough-resource"),
        DealBlock::NoSuchCity => t!("classic.game.reasons.no-such-city"),
        DealBlock::NotACityToGive => t!("classic.game.reasons.deal-not-a-city-to-give"),
        DealBlock::EmbassyExists => t!("classic.game.reasons.deal-embassy-exists"),
        DealBlock::NoCityForEmbassy => t!("classic.game.reasons.deal-no-city-for-embassy"),
        DealBlock::EmbassyNeedsTechnology => t!("classic.game.reasons.deal-embassy-needs-technology"),
        DealBlock::BordersOpen => t!("classic.game.reasons.deal-borders-open"),
        DealBlock::BordersNeedTechnology => t!("classic.game.reasons.deal-borders-need-technology"),
        DealBlock::BordersNeedEmbassy => t!("classic.game.reasons.deal-borders-need-embassy"),
        DealBlock::NoPact => t!("classic.game.reasons.deal-no-pact"),
        DealBlock::PactNeedsTechnology => t!("classic.game.reasons.deal-pact-needs-technology"),
        DealBlock::PactNeedsEmbassies => t!("classic.game.reasons.embassies-both"),
        DealBlock::NoAgreement => t!("classic.game.reasons.deal-no-agreement"),
        DealBlock::AgreementNeedsTechnology => t!("classic.game.reasons.deal-agreement-needs-technology"),
        DealBlock::AgreementNeedsEmbassies => t!("classic.game.reasons.embassies-both"),
        DealBlock::AgreementNeedsFriendship => t!("classic.game.reasons.deal-agreement-needs-friendship"),
        DealBlock::NoFriendship => t!("classic.game.reasons.deal-no-friendship"),
        DealBlock::NotAtWar => t!("classic.game.reasons.deal-not-at-war"),
        DealBlock::CannotDeclareWar => t!("classic.game.reasons.deal-cannot-declare-war"),
        DealBlock::CannotMakePeace => t!("classic.game.reasons.deal-cannot-make-peace"),
        DealBlock::PeaceTooSoon => t!("classic.game.reasons.deal-peace-too-soon"),
    }
}

pub fn spy(block: SpyBlock) -> &'static str {
    match block {
        SpyBlock::Dead => t!("classic.game.reasons.spy-dead"),
        SpyBlock::NoSuchCity => t!("classic.game.reasons.no-such-city"),
        SpyBlock::NotMet => t!("classic.game.reasons.not-met"),
        SpyBlock::Occupied => t!("classic.game.reasons.spy-occupied"),
        SpyBlock::DiplomatTarget => t!("classic.game.reasons.spy-diplomat-target"),
        SpyBlock::NoSuchSpy => t!("classic.game.reasons.spy-no-such-spy"),
        SpyBlock::NothingToSteal => t!("classic.game.reasons.spy-nothing-to-steal"),
        SpyBlock::NoCoup => no_coup(),
    }
}

#[cfg(test)]
pub(super) fn only_cyrillic(text: &str) {
    assert!(!text.is_empty());
    assert!(!text.chars().any(|c| c.is_ascii_alphabetic()), "Latin letters in a Russian reason: {text}");
    assert!(text.chars().any(|c| ('\u{430}'..='\u{44f}').contains(&c.to_lowercase().next().unwrap_or(' '))), "{text}"); // the lowercase Russian alphabet range
}

#[cfg(test)]
mod tests {
    use super::*;
    use classic_rules::derived::culture::{BranchBlock, IdeologyBlock, PolicyBlock};
    use classic_rules::derived::religion::{FaithBlock, FaithPurchaseBlock};

    /// Every typed reason has a text in the bundle of `language` (not its key, and for Russian
    /// not the English fallback).
    fn worded_in(language: &str) {
        let check = |text: &str| {
            assert!(!text.starts_with("classic."), "a key shows instead of a text: {text}");
            if language == "ru" {
                only_cyrillic(text);
            }
        };
        i18n::with_language(language, || {
            for block in [
                PolicyBlock::UnknownPlayer,
                PolicyBlock::NotChoosable,
                PolicyBlock::BranchLocked,
                PolicyBlock::AlreadyAdopted,
                PolicyBlock::MissingPrerequisite,
                PolicyBlock::OtherIdeology,
                PolicyBlock::TierLocked,
                PolicyBlock::NotEnoughCulture,
            ] {
                check(policy(block));
            }
            for block in [
                BranchBlock::UnknownBranch,
                BranchBlock::Ideology,
                BranchBlock::AlreadyUnlocked,
                BranchBlock::EraTooEarly,
                BranchBlock::Excluded,
                BranchBlock::NotEnoughCulture,
            ] {
                check(branch(block));
            }
            for block in [IdeologyBlock::NotAnIdeology, IdeologyBlock::AlreadyChosen, IdeologyBlock::TooEarly] {
                check(ideology(block));
            }
            for block in FaithBlock::ALL {
                check(faith(block));
            }
            for block in FaithPurchaseBlock::ALL {
                check(faith_purchase(block));
            }
            for block in DealBlock::ALL {
                check(deal(block));
            }
            for block in SpyBlock::ALL {
                check(spy(block));
            }
            for problem in [SiteProblem::Unsettleable, SiteProblem::ForeignTerritory, SiteProblem::TooClose] {
                check(site(problem));
            }
        });
    }

    #[test]
    fn every_typed_reason_is_worded_in_english() {
        worded_in("en");
    }

    #[test]
    fn every_typed_reason_is_worded_in_russian() {
        worded_in("ru");
    }

    #[test]
    fn every_refusal_is_worded_in_both_languages() {
        let rules = classic_rules::compile_rules(classic_content::classic::classic_rules()).unwrap();
        let tech = classic_rules::ids::TechId(0);
        let all = [
            Refusal::NoMovesLeft,
            Refusal::ClosedBorders,
            Refusal::AlreadyFortified,
            Refusal::NotEnoughGold { price: 120, treasury: 15 },
            Refusal::NothingQueued,
            Refusal::TileNotForSale,
            Refusal::AlreadyWorking,
            Refusal::Site(SiteProblem::TooClose),
            Refusal::NameTaken,
            Refusal::NeedsTechnology(tech),
            Refusal::AlreadyBuilt,
            Refusal::AlreadyQueued,
            Refusal::QueuedElsewhere,
            Refusal::WonderBuilt,
            Refusal::OtherNation,
            Refusal::NotProducible,
            Refusal::ReplacedByUniqueUnit,
        ];
        for item in all {
            let text = refusal(&rules, item);
            assert!(!text.starts_with("classic."), "{text}");
            i18n::with_language("ru", || {
                let text = refusal(&rules, item);
                let without_name = text.replace(&names::tech(&rules, tech), "");
                only_cyrillic(&without_name);
            });
        }
        assert_eq!(refusal(&rules, Refusal::NotEnoughGold { price: 120, treasury: 15 }), "Need 120 gold, have 15");
        let ru = i18n::with_language("ru", || refusal(&rules, Refusal::NotEnoughGold { price: 120, treasury: 15 }));
        assert_eq!(ru, "\u{41d}\u{443}\u{436}\u{43d}\u{43e} 120 \u{437}\u{43e}\u{43b}\u{43e}\u{442}\u{430}, \u{435}\u{441}\u{442}\u{44c} 15"); // Russian: "120 gold needed, 15 available"
    }

    #[test]
    fn the_screens_own_reasons_are_worded_in_both_languages() {
        let all: [fn() -> &'static str; 14] = [
            full_health,
            not_for_this_person,
            no_trade_target,
            no_moves,
            free_tech_first,
            queue_empty,
            all_assigned,
            city_chooses,
            no_manual_specialist,
            no_pantheon_belief,
            no_free_names,
            not_your_city,
            no_coup,
            nothing_to_build,
        ];
        for reason in all {
            assert!(!reason().starts_with("classic."), "{}", reason());
            i18n::with_language("ru", || only_cyrillic(reason()));
        }
    }
}
