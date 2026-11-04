//! The player's words for the reasons the rules give as English `RuleViolation` text. The rules keep
//! the English as their contract (logs, tests); every typed block lists its variants in `ALL`, and
//! the matches here are exhaustive, so a new variant cannot reach the screen unworded. The words
//! live in the bundle (`classic-game-reasons-rules.ftl`); a reason the rules build around a name of
//! the ruleset («needs Iron») is a pattern with a hole (`HOLES`).
use i18n::t;
use classic_rules::derived::archaeology::DigBlock;
use classic_rules::derived::culture::{BranchBlock, IdeologyBlock, PolicyBlock};
use classic_rules::derived::grants::ChoiceBlock;
use classic_rules::derived::great_people::GreatPersonBlock;
use classic_rules::derived::religion::{FaithBlock, FaithPurchaseBlock};
use classic_rules::derived::ruins::RuinsBlock;
use classic_rules::derived::tourism::TourismBlock;
use classic_rules::derived::trade::TradeBlock;
use classic_rules::world::city_states::CityStateBlock;
use classic_rules::world::espionage::SpyBlock;
use classic_rules::world::military::{AirOrderBlock, AirliftBlock, ParadropBlock};
use classic_rules::world::relations::DealBlock;

use classic_rules::CompiledRuleset;

use super::reasons;


/// The text of a rules reason (`RuleViolation`) in the active language, when the reason is typed
/// and worded.
pub fn rule_text(english: &str) -> Option<&'static str> {
    entries().find(|(rules, _)| *rules == english).map(|(_, text)| text)
}

/// Every typed reason of the rules with its text in the active language.
pub(super) fn entries() -> impl Iterator<Item = (&'static str, &'static str)> {
    let own = [(classic_rules::command::NOT_YOUR_TURN, t!("classic.game.reasons-rules.not-your-turn"))];
    own.into_iter()
        .chain(PolicyBlock::ALL.iter().map(|b| (b.reason(), reasons::policy(*b))))
        .chain(BranchBlock::ALL.iter().map(|b| (b.reason(), reasons::branch(*b))))
        .chain(IdeologyBlock::ALL.iter().map(|b| (b.reason(), reasons::ideology(*b))))
        .chain(FaithBlock::ALL.iter().map(|b| (b.reason(), reasons::faith(*b))))
        .chain(FaithPurchaseBlock::ALL.iter().map(|b| (b.reason(), reasons::faith_purchase(*b))))
        .chain(DealBlock::ALL.iter().map(|b| (b.reason(), reasons::deal(*b))))
        .chain(SpyBlock::ALL.iter().map(|b| (b.reason(), reasons::spy(*b))))
        .chain(DigBlock::ALL.iter().map(|b| (b.reason(), dig(*b))))
        .chain(TourismBlock::ALL.iter().map(|b| (b.reason(), tourism(*b))))
        .chain(TradeBlock::ALL.iter().map(|b| (b.reason(), trade(*b))))
        .chain(ChoiceBlock::ALL.iter().map(|b| (b.reason(), choice(*b))))
        .chain(GreatPersonBlock::ALL.iter().map(|b| (b.reason(), great_person(*b))))
        .chain(CityStateBlock::ALL.iter().map(|b| (b.reason(), city_state(*b))))
        .chain(AirliftBlock::ALL.iter().map(|b| (b.reason(), airlift(*b))))
        .chain(ParadropBlock::ALL.iter().map(|b| (b.reason(), paradrop(*b))))
        .chain(RuinsBlock::ALL.iter().map(|b| (b.reason(), ruins(*b))))
        // «out of range» is the trade reason's English text too: a sweep asks `air_order_text`.
        .chain(AirOrderBlock::ALL.iter().filter(|b| **b != AirOrderBlock::OutOfRange).map(|b| (b.reason(), air_order(*b))))
}

/// The words for a refused air sweep or interception order (its own «out of range» included).
pub fn air_order_text(english: &str) -> Option<&'static str> {
    AirOrderBlock::ALL.iter().find(|b| b.reason() == english).map(|b| air_order(*b))
}

pub fn air_order(block: AirOrderBlock) -> &'static str {
    match block {
        AirOrderBlock::CannotSweep => t!("classic.game.reasons-rules.air-cannot-sweep"),
        AirOrderBlock::NoAttacksLeft => t!("classic.game.reasons-rules.air-no-attack-left"),
        AirOrderBlock::OutOfRange => t!("classic.game.reasons-rules.air-sweep-out-of-range"),
        AirOrderBlock::CannotIntercept => t!("classic.game.reasons-rules.air-cannot-intercept"),
        AirOrderBlock::AlreadyIntercepting => t!("classic.game.reasons-rules.air-already-intercepting"),
    }
}

/// Reasons the rules build with `format!` around a name of the ruleset: the text before the
/// hole, the text after it, the message key (argument `$name`) and what the name is.
const HOLES: [(&str, &str, &str, Hole); 2] = [
    ("needs ", " in the city", i18n::key!("classic.game.reasons-rules.needs-building"), Hole::Building),
    ("needs ", "", i18n::key!("classic.game.reasons-rules.needs-resource"), Hole::Resource),
];

#[derive(Clone, Copy)]
enum Hole {
    Resource,
    Building,
}

/// The words for a reason with a name in it («needs Iron»), the name in the active language.
pub fn rule_text_with_name(rules: &CompiledRuleset, english: &str) -> Option<String> {
    HOLES.iter().find_map(|(before, after, key, hole)| {
        let name = english.strip_prefix(before)?.strip_suffix(after)?;
        if name.is_empty() {
            return None;
        }
        let shown = match hole {
            Hole::Resource => rules.resources.iter().find(|d| d.name == name).map(|d| ("resource", &d.key)),
            Hole::Building => rules.buildings.iter().find(|d| d.name == name).map(|d| ("building", &d.key)),
        };
        let shown = match shown {
            Some((kind, row)) => i18n::content("classic", kind, row, "name", name),
            None => name.to_owned(),
        };
        Some(i18n::tr_args(key, &[("name", shown.as_str().into())]))
    })
}

pub fn ruins(block: RuinsBlock) -> &'static str {
    match block {
        RuinsBlock::UnknownUnit => t!("classic.game.reasons.unavailable"),
        RuinsBlock::NotANation => t!("classic.game.reasons-rules.ruins-not-a-nation"),
        RuinsBlock::NoRuinsHere => t!("classic.game.reasons-rules.ruins-none-here"),
        RuinsBlock::NotAnOffer => t!("classic.game.reasons-rules.ruins-not-an-offer"),
    }
}

pub fn dig(block: DigBlock) -> &'static str {
    match block {
        DigBlock::UnknownUnit => t!("classic.game.reasons.unavailable"),
        DigBlock::NotAnArchaeologist => t!("classic.game.reasons-rules.dig-not-an-archaeologist"),
        DigBlock::NoSite => t!("classic.game.reasons-rules.dig-no-site"),
        DigBlock::HiddenSite => t!("classic.game.reasons-rules.dig-hidden-site"),
        DigBlock::AlreadyDigging => t!("classic.game.reasons-rules.dig-already-digging"),
        DigBlock::NotYourLand => t!("classic.game.reasons-rules.dig-not-your-land"),
        DigBlock::NoPendingDig => t!("classic.game.reasons-rules.dig-no-pending-dig"),
        DigBlock::ChoiceUnavailable => t!("classic.game.reasons-rules.dig-choice-unavailable"),
        DigBlock::NoFreeSlot => t!("classic.game.reasons-rules.no-free-slot"),
    }
}

pub fn tourism(block: TourismBlock) -> &'static str {
    match block {
        TourismBlock::UnknownUnit | TourismBlock::UnknownPlayer => t!("classic.game.reasons.unavailable"),
        TourismBlock::WrongUnit => t!("classic.game.reasons-rules.tourism-wrong-unit"),
        TourismBlock::NotNearOwnCity => t!("classic.game.reasons-rules.tourism-not-near-own-city"),
        TourismBlock::NoFreeSlot => t!("classic.game.reasons-rules.no-free-slot"),
        TourismBlock::NotForeignLand => t!("classic.game.reasons-rules.tourism-not-foreign-land"),
        TourismBlock::NoCulture => t!("classic.game.reasons-rules.tourism-no-culture"),
        TourismBlock::NoIdeology => t!("classic.game.reasons-rules.tourism-no-ideology"),
        TourismBlock::OpinionContent => t!("classic.game.reasons-rules.tourism-opinion-content"),
    }
}

pub fn trade(block: TradeBlock) -> &'static str {
    match block {
        TradeBlock::NotATradeUnit => t!("classic.game.reasons-rules.trade-not-a-trade-unit"),
        TradeBlock::NotInOwnCity => t!("classic.game.reasons-rules.trade-not-in-own-city"),
        TradeBlock::UnknownCity => t!("classic.game.reasons.unavailable"),
        TradeBlock::SameCity => t!("classic.game.reasons-rules.trade-same-city"),
        TradeBlock::Unexplored => t!("classic.game.reasons-rules.trade-unexplored"),
        TradeBlock::OwnCityForInternational => t!("classic.game.reasons-rules.trade-own-city-for-international"),
        TradeBlock::ForeignCityForInternal => t!("classic.game.reasons-rules.trade-foreign-city-for-internal"),
        TradeBlock::NoGranary => t!("classic.game.reasons-rules.trade-no-granary"),
        TradeBlock::NoWorkshop => t!("classic.game.reasons-rules.trade-no-workshop"),
        TradeBlock::NotMet => t!("classic.game.reasons.not-met"),
        TradeBlock::AtWar => t!("classic.game.reasons.at-war"),
        TradeBlock::Razing => t!("classic.game.reasons-rules.trade-razing"),
        TradeBlock::RouteExists => t!("classic.game.reasons-rules.trade-route-exists"),
        TradeBlock::NotCoastal => t!("classic.game.reasons-rules.trade-not-coastal"),
        TradeBlock::OutOfRange => t!("classic.game.reasons-rules.trade-out-of-range"),
        TradeBlock::NoRouteLeft => t!("classic.game.reasons-rules.trade-no-route-left"),
        TradeBlock::NoRouteHere => t!("classic.game.reasons-rules.trade-no-route-here"),
        TradeBlock::NotAtWar => t!("classic.game.reasons-rules.trade-not-at-war"),
        TradeBlock::Embargo => t!("classic.game.reasons-rules.trade-embargo"),
    }
}

pub fn choice(block: ChoiceBlock) -> &'static str {
    match block {
        ChoiceBlock::NothingOwed => t!("classic.game.reasons-rules.choice-nothing-owed"),
        ChoiceBlock::NotAnOption => t!("classic.game.reasons-rules.choice-not-an-option"),
        ChoiceBlock::NoCapital => t!("classic.game.reasons-rules.choice-no-capital"),
        ChoiceBlock::UnknownGreatPerson => t!("classic.game.reasons-rules.choice-unknown-great-person"),
    }
}

pub fn great_person(block: GreatPersonBlock) -> &'static str {
    match block {
        GreatPersonBlock::NotAGreatPerson => t!("classic.game.reasons.not-for-this-person"),
        GreatPersonBlock::WrongImprovement => t!("classic.game.reasons-rules.gp-wrong-improvement"),
        GreatPersonBlock::CannotDiscover => t!("classic.game.reasons-rules.gp-cannot-discover"),
        GreatPersonBlock::CannotHurry => t!("classic.game.reasons-rules.gp-cannot-hurry"),
        GreatPersonBlock::NotInCity => t!("classic.game.reasons-rules.gp-not-in-city"),
        GreatPersonBlock::NotYourCity => t!("classic.game.reasons.not-your-city"),
        GreatPersonBlock::NothingToHurry => t!("classic.game.reasons-rules.gp-nothing-to-hurry"),
        GreatPersonBlock::BuildingsOnly => t!("classic.game.reasons-rules.gp-buildings-only"),
        GreatPersonBlock::CannotStartGoldenAge => t!("classic.game.reasons-rules.gp-cannot-start-golden-age"),
        GreatPersonBlock::CannotTrade => t!("classic.game.reasons-rules.gp-cannot-trade"),
        GreatPersonBlock::CannotBuyCityState => t!("classic.game.reasons-rules.gp-cannot-buy-city-state"),
        GreatPersonBlock::NeedsCity => t!("classic.game.reasons-rules.gp-needs-city"),
        GreatPersonBlock::NeedsCityStateLand => t!("classic.game.reasons-rules.gp-needs-city-state-land"),
        GreatPersonBlock::NotMet => t!("classic.game.reasons.not-met"),
        GreatPersonBlock::AtWar => t!("classic.game.reasons.at-war"),
    }
}

pub fn city_state(block: CityStateBlock) -> &'static str {
    match block {
        CityStateBlock::NotANation => t!("classic.game.reasons-rules.cs-not-a-nation"),
        CityStateBlock::NotACityState => t!("classic.game.reasons-rules.cs-not-a-city-state"),
        CityStateBlock::NotMet => t!("classic.game.reasons.not-met"),
        CityStateBlock::AtWar => t!("classic.game.reasons.at-war"),
        CityStateBlock::NotEnoughGold => t!("classic.game.reasons.not-enough-gold-short"),
        CityStateBlock::UnitNotGiftable => t!("classic.game.reasons-rules.cs-unit-not-giftable"),
        CityStateBlock::NotTheirLand => t!("classic.game.reasons-rules.cs-not-their-land"),
        CityStateBlock::NoFreeTile => t!("classic.game.reasons-rules.cs-no-free-tile"),
        CityStateBlock::NoResource => t!("classic.game.reasons-rules.cs-no-resource"),
        CityStateBlock::NoImprovementNeeded => t!("classic.game.reasons-rules.cs-no-improvement-needed"),
        CityStateBlock::ImprovementUnknown => t!("classic.game.reasons-rules.cs-improvement-unknown"),
        CityStateBlock::NotAfraid => t!("classic.game.reasons-rules.cs-not-afraid"),
        CityStateBlock::NoWorker => t!("classic.game.reasons-rules.cs-no-worker"),
        CityStateBlock::NoRoomForWorker => t!("classic.game.reasons-rules.cs-no-room-for-worker"),
        CityStateBlock::NoPledge => t!("classic.game.reasons-rules.cs-no-pledge"),
        CityStateBlock::PledgedRecently => t!("classic.game.reasons-rules.cs-pledged-recently"),
        CityStateBlock::AlreadyPledged => t!("classic.game.reasons-rules.cs-already-pledged"),
        CityStateBlock::NotOnGoodTerms => t!("classic.game.reasons-rules.cs-not-on-good-terms"),
        CityStateBlock::PledgeBrokenRecently => t!("classic.game.reasons-rules.cs-pledge-broken-recently"),
        CityStateBlock::NotTheAlly => t!("classic.game.reasons-rules.cs-not-the-ally"),
        CityStateBlock::CannotAnnex => t!("classic.game.reasons-rules.cs-cannot-annex"),
        CityStateBlock::AlliedTooRecently => t!("classic.game.reasons-rules.cs-allied-too-recently"),
    }
}

pub fn paradrop(block: ParadropBlock) -> &'static str {
    match block {
        ParadropBlock::UnknownUnit => t!("classic.game.reasons.unavailable"),
        ParadropBlock::CannotParadrop => t!("classic.game.reasons-rules.paradrop-cannot"),
        ParadropBlock::AlreadyMoved => t!("classic.game.reasons-rules.airlift-already-moved"),
        ParadropBlock::Embarked => t!("classic.game.reasons-rules.paradrop-embarked"),
        ParadropBlock::NoBase => t!("classic.game.reasons-rules.paradrop-no-base"),
        ParadropBlock::SameTile => t!("classic.game.reasons-rules.paradrop-same-tile"),
        ParadropBlock::OutOfRange => t!("classic.game.reasons-rules.paradrop-out-of-range"),
        ParadropBlock::UnknownTile => t!("classic.game.reasons-rules.paradrop-unknown-tile"),
        ParadropBlock::NotLand => t!("classic.game.reasons-rules.paradrop-not-land"),
        ParadropBlock::ClosedBorders => t!("classic.game.reasons-rules.paradrop-closed-borders"),
        ParadropBlock::TargetOccupied => t!("classic.game.reasons-rules.paradrop-target-occupied"),
        ParadropBlock::TargetFull => t!("classic.game.reasons-rules.paradrop-target-full"),
    }
}

pub fn airlift(block: AirliftBlock) -> &'static str {
    match block {
        AirliftBlock::UnknownUnit | AirliftBlock::UnknownCity => t!("classic.game.reasons.unavailable"),
        AirliftBlock::NotLandUnit => t!("classic.game.reasons-rules.airlift-not-land-unit"),
        AirliftBlock::AlreadyMoved => t!("classic.game.reasons-rules.airlift-already-moved"),
        AirliftBlock::NoAirportHere => t!("classic.game.reasons-rules.airlift-no-airport-here"),
        AirliftBlock::NoAirportThere => t!("classic.game.reasons-rules.airlift-no-airport-there"),
        AirliftBlock::TargetOccupied => t!("classic.game.reasons-rules.airlift-target-occupied"),
    }
}
