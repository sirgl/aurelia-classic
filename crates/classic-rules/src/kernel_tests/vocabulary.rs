//! Kernel and module tags of each family cover `0..count` once: the
//! numbering the saves, the per-family arrays and the fingerprint share.

use crate::rules::vocabulary::*;
use crate::rules::effects::{CostTarget, Count, Grant, Scope, Selector, Trigger, YieldKind};

/// The kernel's 10 costs and the module's.
const COST_TARGETS: usize = 10 + <crate::classic::effects::ClassicVocabulary as Vocabulary<crate::classic::Classic>>::COSTS.len();

fn dense(family: &str, tags: impl IntoIterator<Item = u8>, count: usize) {
    let mut seen = vec![false; count];
    for tag in tags {
        let slot = seen.get_mut(usize::from(tag)).unwrap_or_else(|| panic!("{family}: tag {tag} ≥ {count}"));
        assert!(!*slot, "{family}: tag {tag} twice");
        *slot = true;
    }
    assert!(seen.iter().all(|s| *s), "{family}: tags leave a gap in 0..{count}");
}

#[test]
fn indexed_families_cover_their_arrays() {
    dense("Trigger", Trigger::all().map(|t| t.tag()), Trigger::all().count());
    assert_eq!(Trigger::slots(), Trigger::all().count(), "per-trigger arrays have one slot per trigger");
    dense("YieldKind", YieldKind::all().map(|y| y.tag()), YieldKind::all().count());
    assert!(YieldKind::all().enumerate().all(|(i, y)| y.index() == i), "all() is in index order");
    let costs = (0..COST_TARGETS as u8).filter_map(cost_of);
    dense("CostTarget", costs.map(|c| c.tag()), COST_TARGETS);
}

#[test]
fn a_tag_finds_its_trigger() {
    for trigger in Trigger::all() {
        assert_eq!(Trigger::from_tag(trigger.tag()), Some(trigger));
    }
    assert_eq!(Trigger::from_tag(Trigger::slots() as u8), None);
}

/// Every cost, kernel and module, by tag.
fn cost_of(tag: u8) -> Option<CostTarget> {
    use crate::classic::effects::ClassicCost;
    let all = [
        CostTarget::Units,
        CostTarget::Buildings,
        CostTarget::Technologies,
        CostTarget::TilePurchase,
        CostTarget::Hurry,
        CostTarget::BorderGrowth,
        CostTarget::BorderGrowthExponent,
        CostTarget::UnitUpgrade,
        CostTarget::UnitPurchase,
        CostTarget::BuildingPurchase,
        CostTarget::Module(ClassicCost::Policies),
        CostTarget::Module(ClassicCost::PolicyCityPenalty),
        CostTarget::Module(ClassicCost::FaithPurchase),
    ];
    all.into_iter().find(|c| c.tag() == tag)
}

#[test]
fn small_families_keep_their_tags() {
    use crate::classic::effects::{ClassicCount, ClassicGrant, ClassicScope, ClassicSelector};
    let scopes = [Scope::Player, Scope::City, Scope::Unit, Scope::Tile, Scope::Module(ClassicScope::Religion)];
    dense("Scope", scopes.map(Scope::tag), 5);
    let selectors = [
        Selector::Owner,
        Selector::SourceCity,
        Selector::OwnerCities,
        Selector::EventCity,
        Selector::SourceUnit,
        Selector::OwnerUnits,
        Selector::EventUnit,
        Selector::SourceCityTiles,
        Selector::OwnerCityTiles,
        Selector::Capital,
        Selector::Module(ClassicSelector::FollowingCities),
        Selector::Module(ClassicSelector::FollowingCityTiles),
        Selector::Module(ClassicSelector::HolyCity),
        Selector::Module(ClassicSelector::Founder),
        Selector::Module(ClassicSelector::OwnReligion),
    ];
    dense("Selector", selectors.map(Selector::tag), 15);
    let counts = [
        Count::Population,
        Count::WorldWonders,
        Count::ConnectedCities,
        Count::Module(ClassicCount::Policies),
        Count::Module(ClassicCount::MajorityFollowers),
        Count::Module(ClassicCount::ReligionsPresent),
        Count::Module(ClassicCount::PeacefulForeignFollowers),
        Count::Module(ClassicCount::OtherReligionFollowers),
        Count::Module(ClassicCount::TradePartners),
        Count::Module(ClassicCount::CityStateRoutes),
    ];
    dense("Count", counts.map(Count::tag), 10);
    let grants = [
        Grant::FreeTechnologies(1),
        Grant::Module(ClassicGrant::FreeGreatPeople(1)),
        Grant::Module(ClassicGrant::LongCountGreatPerson),
        Grant::ConquestTechnology,
        Grant::Module(ClassicGrant::GoldenAge { turns: 1 }),
        Grant::MilitaryUnitCopies,
        Grant::Population(1),
    ];
    dense("Grant", grants.map(Grant::tag), 7);
}
