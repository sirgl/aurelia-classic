//! How the effect engine evaluates the classic words (`effects::module`):
//! adopted policies and beliefs as sources, the classic numbers of a city
//! and a player, golden-age and follower conditions, religious, policy and
//! trade-route counts.

use crate::classic::Classic;
use crate::classic::effects::{
    ClassicCondition, ClassicCount, ClassicModifier, ClassicVocabulary, TourismStat, CITY_STATE_STATS,
    GREAT_PERSON_KINDS, TOURISM_CONDITIONS,
};
use crate::effects::count::Counted;
use crate::effects::module::{ModuleEffects, Visit};
use crate::effects::{Bucket, ConditionContext};
use crate::ids::PlayerId;
use crate::rules::content::ReligionId;
use crate::rules::effects::Count;
use crate::rules::CompiledRuleset;
use crate::world::city::CityCore;
use crate::world::religion::Creed;
use crate::world::state::AuthWorld;

/// The classic numbers of one city (City-scope modifiers).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ClassicCityModifiers {
    pub happiness: i32,
    /// Index = `GreatPersonKind::index()`.
    pub great_person_points: [i32; GREAT_PERSON_KINDS],
    pub tourism: CityTourismModifiers,
}

/// City-scope tourism numbers (`TourismStat` city stats).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CityTourismModifiers {
    pub flat: i32,
    pub percent: i32,
    pub great_works_percent: i32,
    pub landmarks_percent: i32,
    pub per_faith_building: i32,
    /// Percent of the theming bonuses after the player's (France's capital).
    pub theming_percent: i32,
}

impl CityTourismModifiers {
    fn apply(&mut self, stat: TourismStat, amount: i32) {
        match stat {
            TourismStat::Flat => self.flat += amount,
            TourismStat::Percent => self.percent += amount,
            TourismStat::GreatWorksPercent => self.great_works_percent += amount,
            TourismStat::LandmarksPercent => self.landmarks_percent += amount,
            TourismStat::PerFaithBuilding => self.per_faith_building += amount,
            TourismStat::ThemingPercent => self.theming_percent += amount,
            TourismStat::Spread | TourismStat::GreatWorkCulture => {
                unreachable!("compiler rejects {stat:?} for City scope")
            }
        }
    }
}

/// The classic numbers of one player (Player-scope modifiers).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ClassicPlayerModifiers {
    pub happiness: i32,
    /// City-state numbers, index = `CityStateStat::index()`.
    pub city_state: [i32; CITY_STATE_STATS],
    /// The player may add a Reformation belief to its religion.
    pub reformation: bool,
    /// Beliefs the player may add when it founds its religion, beyond the
    /// founder and follower (Byzantium: 1).
    pub extra_founding_beliefs: u8,
    /// Percent of all tourism a Great Firewall nullifies (the Internet).
    pub tourism_spread: i32,
    /// Percent added to theming bonuses.
    pub theming_percent: i32,
    /// Culture added to each great work.
    pub great_work_culture: i32,
    /// Tourism percent toward a nation, index =
    /// `TourismCondition::index()` (applied when the condition holds).
    pub tourism_toward: [i32; TOURISM_CONDITIONS],
}

impl ModuleEffects<Classic> for ClassicVocabulary {
    type CityModifiers = ClassicCityModifiers;
    type PlayerModifiers = ClassicPlayerModifiers;

    fn apply_city(mods: &mut ClassicCityModifiers, modifier: ClassicModifier, times: u32) {
        let n = times as i32;
        match modifier {
            ClassicModifier::AddHappiness { amount } => mods.happiness += amount * n,
            ClassicModifier::AddGreatPersonPoints { kind, amount } => {
                mods.great_person_points[kind.index()] += amount * n
            }
            ClassicModifier::ModifyTourism { stat, amount } => mods.tourism.apply(stat, amount * n),
            // Read by the faith purchase rules, not a city number.
            ClassicModifier::UnlockFaithBuilding(_) | ClassicModifier::UnlockFaithUnits(_) => {}
            // Count-based amounts arrive converted (`religion_effects::counted`,
            // `resolve_count`).
            ClassicModifier::AddYieldPerFollowingCity { .. }
            | ClassicModifier::AddYieldPerFollowers { .. }
            | ClassicModifier::AddHappinessPerCount { .. } => {
                unreachable!("{modifier:?} is converted before it is applied")
            }
            ClassicModifier::ModifyCityState { .. }
            | ClassicModifier::FreePolicies { .. }
            | ClassicModifier::InstantTourism { .. }
            | ClassicModifier::GrantExperiencePerGreatWork(_)
            | ClassicModifier::GrantSpies { .. }
            | ClassicModifier::PromoteSpies { .. }
            | ClassicModifier::AddHappinessPerFollowingCity { .. }
            | ClassicModifier::ModifyReligion { .. }
            | ClassicModifier::AllowReformationBelief
            | ClassicModifier::TourismToward { .. }
            | ClassicModifier::GrantReligionAbility(_)
            | ClassicModifier::ExtraFoundingBelief
            | ClassicModifier::SecondReligionPantheon => {
                unreachable!("compiler rejects {modifier:?} for City scope")
            }
        }
    }

    fn apply_player(mods: &mut ClassicPlayerModifiers, modifier: ClassicModifier, times: u32) {
        let n = times as i32;
        match modifier {
            ClassicModifier::AddHappiness { amount } => mods.happiness += amount * n,
            ClassicModifier::ModifyCityState { stat, amount } => mods.city_state[stat.index()] += amount * n,
            ClassicModifier::AllowReformationBelief => mods.reformation = true,
            ClassicModifier::ExtraFoundingBelief => {
                mods.extra_founding_beliefs = mods.extra_founding_beliefs.saturating_add(times as u8)
            }
            // Read through `ClassicIndex::second_pantheon_policies` by the
            // city's belief lookup (`religion_effects`).
            ClassicModifier::SecondReligionPantheon => {}
            ClassicModifier::ModifyTourism { stat, amount } => match stat {
                TourismStat::Spread => mods.tourism_spread += amount * n,
                TourismStat::ThemingPercent => mods.theming_percent += amount * n,
                TourismStat::GreatWorkCulture => mods.great_work_culture += amount * n,
                _ => unreachable!("compiler rejects {stat:?} for Player scope"),
            },
            ClassicModifier::TourismToward { when, percent } => {
                mods.tourism_toward[when.index()] += percent * n
            }
            // Instant grants are applied by systems::effects, never evaluated.
            ClassicModifier::FreePolicies { .. }
            | ClassicModifier::GrantSpies { .. }
            | ClassicModifier::PromoteSpies { .. }
            | ClassicModifier::InstantTourism { .. }
            | ClassicModifier::GrantExperiencePerGreatWork(_) => {
                unreachable!("{modifier:?} is instant, never passive")
            }
            ClassicModifier::AddYieldPerFollowingCity { .. }
            | ClassicModifier::AddYieldPerFollowers { .. }
            | ClassicModifier::AddHappinessPerFollowingCity { .. }
            | ClassicModifier::AddHappinessPerCount { .. } => {
                unreachable!("{modifier:?} is converted before it is applied")
            }
            ClassicModifier::AddGreatPersonPoints { .. }
            | ClassicModifier::ModifyReligion { .. }
            | ClassicModifier::GrantReligionAbility(_)
            | ClassicModifier::UnlockFaithBuilding(_)
            | ClassicModifier::UnlockFaithUnits(_) => {
                unreachable!("compiler rejects {modifier:?} for Player scope")
            }
        }
    }

    fn condition_holds(
        auth: &AuthWorld,
        _rules: &CompiledRuleset,
        at: ConditionContext<'_>,
        condition: ClassicCondition,
    ) -> bool {
        match condition {
            // Situational: the combat and healing profiles evaluate them.
            ClassicCondition::OpponentIsBarbarian
            | ClassicCondition::OpponentIsCityState
            | ClassicCondition::OpponentHasMoreCities
            | ClassicCondition::NearFollowingCity(_) => false,
            ClassicCondition::MinFollowers(n) => at.city.is_some_and(|city| majority_followers(auth, city) >= n),
            ClassicCondition::GoldenAge => in_golden_age(auth, at.owner),
            ClassicCondition::GreatGeneralEarned => auth.module().military.of(at.owner).generals_born > 0,
            // Read from the event at firing (`systems::effects`), never here.
            ClassicCondition::GreatPersonIs(_) => false,
        }
    }

    fn count(auth: &AuthWorld, _rules: &CompiledRuleset, target: Counted<'_>, count: ClassicCount) -> u32 {
        match (count, target) {
            (ClassicCount::MajorityFollowers, Counted::City(core)) => majority_followers(auth, core),
            (ClassicCount::ReligionsPresent, Counted::City(core)) => religions_present(auth, core),
            (ClassicCount::Policies, Counted::Player(player)) => {
                auth.module().culture().of(player).policies.len() as u32
            }
            (ClassicCount::PeacefulForeignFollowers, Counted::Player(player)) => {
                peaceful_foreign_followers(auth, player)
            }
            (ClassicCount::OtherReligionFollowers, Counted::Event { city, subject }) => {
                other_religion_followers(auth, city, crate::rules::content::ReligionId(subject))
            }
            (ClassicCount::TradePartners, target) => match owner_of(target) {
                Some(owner) => crate::derived::trade::trade_partners(auth, owner),
                None => 0,
            },
            (ClassicCount::CityStateRoutes, target) => match owner_of(target) {
                Some(owner) => crate::derived::trade::city_state_routes(auth, owner),
                None => 0,
            },
            _ => 0,
        }
    }

    fn resolve_count(modifier: ClassicModifier, count: &mut dyn FnMut(Count) -> u32) -> Option<ClassicModifier> {
        let ClassicModifier::AddHappinessPerCount { amount, per } = modifier else {
            return None;
        };
        let amount = per.scale(amount, count(per.count));
        Some(ClassicModifier::AddHappiness { amount })
    }

    /// Adopted policies, in adoption order.
    fn visit_owner_sources(auth: &AuthWorld, rules: &CompiledRuleset, owner: PlayerId, bucket: Bucket, visit: Visit<'_>) {
        for policy in &auth.module().culture().of(owner).policies {
            if let Some(def) = rules.module.policies.get(policy.0 as usize) {
                for effect in bucket(&def.effects) {
                    visit(effect, 1);
                }
            }
        }
    }

    /// The beliefs of the city's majority.
    fn visit_city_sources(auth: &AuthWorld, rules: &CompiledRuleset, core: &CityCore, bucket: Bucket, visit: Visit<'_>) {
        crate::classic::religion_effects::visit_city_beliefs(auth, rules, core, bucket, visit);
    }

    /// The beliefs of the religion the player founded.
    fn visit_player_sources(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId, visit: Visit<'_>) {
        crate::classic::religion_effects::visit_founder_beliefs(auth, rules, player, visit);
    }

    /// The beliefs of the owner's creed.
    /// An unhappy owner fights worse (from the turn's happiness snapshot); so
    /// does a unit whose strategic resource is in deficit.
    fn unit_combat_percent(auth: &AuthWorld, rules: &CompiledRuleset, unit: &crate::world::unit::UnitCore) -> i32 {
        let excess = crate::derived::happiness::excess_snapshot(auth, unit.owner);
        crate::derived::happiness::combat_penalty_percent(excess)
            + crate::derived::resources::unit_resource_penalty(auth, rules, unit.owner, unit.kind)
    }

    fn city_resources(
        auth: &AuthWorld,
        rules: &CompiledRuleset,
        city: crate::ids::CityHandle,
    ) -> Vec<crate::ids::ResourceId> {
        crate::derived::resources::city_resources(auth, rules, city)
    }

    fn visit_unit_sources(auth: &AuthWorld, rules: &CompiledRuleset, owner: PlayerId, visit: Visit<'_>) {
        crate::classic::religion_effects::visit_creed_unit_beliefs(auth, rules, owner, visit);
    }
}

/// The player is in a golden age now.
pub(crate) fn in_golden_age(auth: &AuthWorld, player: PlayerId) -> bool {
    auth.players()
        .economy_of(player)
        .is_some_and(|e| e.golden_age_turns > 0)
}

/// Followers of the city's majority creed (0 without a majority).
pub(crate) fn majority_followers(auth: &AuthWorld, city: &CityCore) -> u32 {
    let Some(handle) = auth.indexes().city_on_tile(city.tile) else {
        return 0;
    };
    let faith = auth.module().religion().city(handle);
    match crate::derived::religion::majority(faith, city.population) {
        Some(creed) => crate::derived::religion::followers(faith, city.population)
            .iter()
            .find(|c| c.0 == creed)
            .map(|c| c.1)
            .unwrap_or(0),
        None => 0,
    }
}

/// The player a count of the owner's things looks at (None: a spread
/// target counts the city's followers, not an owner's things).
fn owner_of(target: Counted<'_>) -> Option<PlayerId> {
    match target {
        Counted::City(core) => Some(core.owner),
        Counted::Player(player) => Some(player),
        Counted::Event { .. } => None,
    }
}

/// Followers of the city's creeds, or none for a city not in the world.
fn city_followers(auth: &AuthWorld, core: &CityCore) -> Vec<(Creed, u32)> {
    match auth.indexes().city_on_tile(core.tile) {
        Some(handle) => crate::derived::religion::followers(auth.module().religion().city(handle), core.population),
        None => Vec::new(),
    }
}

/// Founded religions with at least one follower in the city.
fn religions_present(auth: &AuthWorld, core: &CityCore) -> u32 {
    let followers = city_followers(auth, core);
    followers.iter().filter(|(creed, _)| matches!(creed, Creed::Religion(_))).count() as u32
}

/// Followers of the religion `player` founded in the cities of the other
/// players it is not at war with (majors and city-states).
fn peaceful_foreign_followers(auth: &AuthWorld, player: PlayerId) -> u32 {
    let Some(religion) = auth.module().religion().player(player).religion else {
        return 0;
    };
    let creed = Creed::Religion(religion);
    let mut total = 0u32;
    for (handle, core) in auth.cities().iter() {
        if core.owner == player || auth.diplomacy().at_war(player, core.owner) {
            continue;
        }
        total += crate::derived::religion::city_followers(auth, handle, creed);
    }
    total
}

/// Followers of the religions other than `religion` in a city whose
/// majority is another creed (counted before the spread lands).
fn other_religion_followers(auth: &AuthWorld, core: &CityCore, religion: ReligionId) -> u32 {
    let Some(handle) = auth.indexes().city_on_tile(core.tile) else {
        return 0;
    };
    let faith = auth.module().religion().city(handle);
    let majority = crate::derived::religion::majority(faith, core.population);
    if majority.is_none() || majority == Some(Creed::Religion(religion)) {
        return 0;
    }
    let mut total = 0u32;
    for (creed, n) in crate::derived::religion::followers(faith, core.population) {
        if matches!(creed, Creed::Religion(r) if r != religion) {
            total += n;
        }
    }
    total
}

