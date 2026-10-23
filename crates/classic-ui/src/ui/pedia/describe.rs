//! Effect text derived from the compiled rules: typed effects say what the
//! engine applies, inert effects say what the catalog promises and is not
//! evaluated yet. Nothing here is written per entity.
//!
//! Every sentence is a message of `classic-pedia-describe.ftl` with named arguments (a number,
//! a name from the rules). A sentence made of optional parts (trigger, target, condition,
//! duration) is built by wrapping: each `with-*` message takes the text so far as `$text`,
//! so a language can put the new part before or after it.
use classic_rules::rules::compiled::CompiledRuleset;
use classic_rules::rules::content::{ContentRef, ContentTable, InertAmount, InertEffect, Yields};
use classic_rules::rules::effects::{UnitStat, CombatContext, CompiledEffect, Condition, CostTarget, Count, Duration, EffectSet, Modifier, PerCount, ProductionFilter, Selector, Trigger, YieldKind, Forbidden, Grant};
use classic_rules::rules::stats::{EspionageStat, LeagueStat, SocietyStat};
use i18n::{t, tf};

use super::{Line, PediaRef, Table};

#[path = "describe_classic.rs"]
mod classic;
pub use classic::{great_person, league_reward};


/// "core:unit-combat/melee" -> "Melee".
pub fn pretty(key: &str) -> String {
    let slug = key.rsplit('/').next().unwrap_or(key);
    let words: Vec<String> = slug
        .split(['-', '_'])
        .filter(|w| !w.is_empty())
        .map(capitalize)
        .collect();
    words.join(" ")
}

pub fn capitalize(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// A number with its sign, as the rules write bonuses (`+2`, `-10`, `+0`).
pub(super) fn signed(value: impl Into<i64>) -> String {
    let value = value.into();
    let sign = if value < 0 { '-' } else { '+' };
    format!("{sign}{}", i18n::number_int(value.abs()))
}

/// "a, b, c": the separator is the language's (`join`).
pub(super) fn join_list(items: Vec<String>) -> String {
    let mut items = items.into_iter();
    let Some(mut text) = items.next() else { return String::new() };
    for next in items {
        text = tf!("classic.pedia.describe.join", a = text, b = next);
    }
    text
}

/// "a or b or c" (`join-or`).
pub(super) fn join_or(items: Vec<String>) -> String {
    let mut items = items.into_iter();
    let Some(mut text) = items.next() else { return String::new() };
    for next in items {
        text = tf!("classic.pedia.describe.join-or", a = text, b = next);
    }
    text
}

/// "+2 Food, +1 Production" (empty for no yields).
pub fn yields(y: &Yields) -> String {
    let parts: Vec<String> = [
        (y.food, yield_name(YieldKind::Food)),
        (y.production, yield_name(YieldKind::Production)),
        (y.gold, yield_name(YieldKind::Gold)),
        (y.science, yield_name(YieldKind::Science)),
        (y.culture, yield_name(YieldKind::Culture)),
        (y.module, t!("classic.pedia.describe.yield-faith")),
    ]
    .into_iter()
    .filter(|(amount, _)| *amount != 0)
    .map(|(amount, name)| tf!("classic.pedia.describe.yield-amount", amount = signed(amount), yield = name))
    .collect();
    join_list(parts)
}

/// Every effect of a source, typed first, then inert.
pub fn effect_lines(rules: &CompiledRuleset, set: &EffectSet, inert: &[InertEffect]) -> Vec<Line> {
    let mut lines: Vec<Line> = all_effects(set).map(|e| Line::effect(effect(rules, e))).collect();
    lines.extend(inert.iter().filter(|e| !internal_unit_metadata(e)).map(|e| Line::inactive(inert_effect(rules, e))));
    lines
}

/// The effects of a source as one line of text, for cards that have no room for a list. The
/// description of a policy or belief is made from its typed effects (the rule data carries no
/// prose of its own).
pub fn effect_summary(rules: &CompiledRuleset, set: &EffectSet, inert: &[InertEffect]) -> String {
    effect_lines(rules, set, inert).iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join("; ")
}

/// The entities every effect and inert effect of a source names.
pub fn effect_refs(rules: &CompiledRuleset, set: &EffectSet, inert: &[InertEffect]) -> Vec<PediaRef> {
    let mut refs = Vec::new();
    for e in all_effects(set) {
        refs.extend(modifier_ref(&e.modifier));
        refs.extend(condition_ref(&e.condition));
        refs.extend(condition_ref(&e.second_condition));
    }
    for e in inert {
        refs.extend(e.refs.iter().filter_map(|r| content_ref(rules, r)));
    }
    refs.sort();
    refs.dedup();
    refs
}

fn all_effects(set: &EffectSet) -> impl Iterator<Item = &CompiledEffect> {
    let passive = [
        &set.passive_player,
        &set.passive_module,
        &set.passive_city,
        &set.passive_unit,
        &set.passive_tile,
    ];
    passive.into_iter().chain(set.triggered.iter()).flatten()
}

/// One sentence for one typed effect: trigger, modifier, target, condition, duration.
pub fn effect(rules: &CompiledRuleset, e: &CompiledEffect) -> String {
    let mut text = modifier(rules, &e.modifier);
    let target = selector(e.selector);
    if !target.is_empty() {
        text = tf!("classic.pedia.describe.with-selector", text = text, selector = target);
    }
    let first = condition(rules, &e.condition);
    if !first.is_empty() {
        text = tf!("classic.pedia.describe.with-condition", text = text, condition = first);
    }
    let second = condition(rules, &e.second_condition);
    if !second.is_empty() {
        text = tf!("classic.pedia.describe.with-second-condition", text = text, condition = second);
    }
    if let Duration::Turns(turns) = e.duration {
        text = tf!("classic.pedia.describe.with-duration", text = text, turns = turns);
    }
    let when = trigger(e.trigger);
    if !when.is_empty() {
        text = tf!("classic.pedia.describe.with-trigger", trigger = when, text = text);
    }
    capitalize(&text)
}

fn trigger(trigger: Trigger) -> &'static str {
    match trigger {
        Trigger::Passive => "",
        Trigger::OnBuilt => t!("classic.pedia.describe.trigger-on-built"),
        Trigger::OnTurnStart => t!("classic.pedia.describe.trigger-on-turn-start"),
        Trigger::OnTechnologyUnlocked => t!("classic.pedia.describe.trigger-on-technology-unlocked"),
        Trigger::OnCityFounded => t!("classic.pedia.describe.trigger-on-city-founded"),
        Trigger::OnUnitKilled => t!("classic.pedia.describe.trigger-on-unit-killed"),
        Trigger::OnUnitTrained => t!("classic.pedia.describe.trigger-on-unit-trained"),
        Trigger::OnCityConquered => t!("classic.pedia.describe.trigger-on-city-conquered"),
        Trigger::OnEraEntered => t!("classic.pedia.describe.trigger-on-era-entered"),
        Trigger::Module(trigger) => classic::trigger(trigger),
    }
}

fn selector(selector: Selector) -> &'static str {
    match selector {
        Selector::Owner => "",
        Selector::SourceCity => t!("classic.pedia.describe.selector-source-city"),
        Selector::OwnerCities => t!("classic.pedia.describe.selector-owner-cities"),
        Selector::EventCity => t!("classic.pedia.describe.selector-event-city"),
        Selector::SourceUnit => "",
        Selector::OwnerUnits => t!("classic.pedia.describe.selector-owner-units"),
        Selector::EventUnit => t!("classic.pedia.describe.selector-event-unit"),
        Selector::SourceCityTiles => t!("classic.pedia.describe.selector-source-city-tiles"),
        Selector::OwnerCityTiles => t!("classic.pedia.describe.selector-owner-city-tiles"),
        Selector::Capital => t!("classic.pedia.describe.selector-capital"),
        Selector::Module(selector) => classic::selector(selector),
    }
}

fn condition(rules: &CompiledRuleset, condition: &Condition) -> String {
    let name = |table: Table, index: u16| name_of(rules, PediaRef::new(table, index));
    match condition {
        Condition::Always => String::new(),
        Condition::HasTechnology(t) => tf!("classic.pedia.describe.condition-has-technology", name = name(Table::Technology, t.0)),
        Condition::LacksTechnology(t) => tf!("classic.pedia.describe.condition-lacks-technology", name = name(Table::Technology, t.0)),
        Condition::MinPopulation(n) => tf!("classic.pedia.describe.condition-min-population", n = *n),
        Condition::HasBuilding(b) => tf!("classic.pedia.describe.condition-has-building", name = name(Table::Building, b.0)),
        Condition::TileImprovement(i) => tf!("classic.pedia.describe.condition-on-tile", name = name(Table::Improvement, i.0)),
        Condition::TileResource(r) => tf!("classic.pedia.describe.condition-on-tile", name = name(Table::Resource, r.0)),
        Condition::TileFeature(f) => tf!("classic.pedia.describe.condition-on-tile", name = name(Table::Feature, f.0)),
        Condition::TileTerrain(t) => tf!("classic.pedia.describe.condition-on-tile", name = name(Table::Terrain, t.0)),
        Condition::TileTerrainUntil(t, tech) => tf!(
            "classic.pedia.describe.condition-terrain-until",
            terrain = name(Table::Terrain, t.0),
            technology = name(Table::Technology, tech.0)
        ),
        Condition::OpponentRole(role) => tf!("classic.pedia.describe.condition-against-role", role = role_words(*role)),
        Condition::OpponentDomain(domain) => tf!("classic.pedia.describe.condition-against-role", role = domain_word(*domain)),
        Condition::OpponentClassOf(u) => tf!("classic.pedia.describe.condition-against-class", name = name(Table::Unit, u.0)),
        Condition::UnitDomain(domain) => tf!("classic.pedia.describe.condition-for-units", kind = domain_word(*domain)),
        Condition::UnitRole(role) => tf!("classic.pedia.describe.condition-for-units", kind = role_words(*role)),
        Condition::EventNearSourceCity(n) => tf!("classic.pedia.describe.condition-near-city", n = *n),
        Condition::TilePlot(plot) => plot_filter(*plot).into(),
        Condition::TileResourceClass(class) => tf!("classic.pedia.describe.condition-resource-class", class = resource_class_word(*class)),
        Condition::TileFeatureUnimproved(f) => tf!("classic.pedia.describe.condition-unimproved-feature", name = name(Table::Feature, f.0)),
        Condition::CityAdjacentUnimprovedFeature { feature, at_least } => tf!(
            "classic.pedia.describe.condition-adjacent-unimproved-feature",
            n = *at_least,
            name = name(Table::Feature, feature.0)
        ),
        Condition::Producing(filter) => tf!("classic.pedia.describe.condition-producing", what = production_filter(rules, *filter)),
        Condition::CityGarrisoned => t!("classic.pedia.describe.condition-garrisoned").into(),
        Condition::CityCoastal => t!("classic.pedia.describe.condition-coastal").into(),
        Condition::LearnedTechnology(t) => tf!("classic.pedia.describe.condition-learned-technology", name = name(Table::Technology, t.0)),
        Condition::CityOnRiver => t!("classic.pedia.describe.condition-on-river").into(),
        Condition::AtPeace => t!("classic.pedia.describe.condition-at-peace").into(),
        Condition::ConnectedToCapital => t!("classic.pedia.describe.condition-connected").into(),
        Condition::NearOwnImprovement { improvement, range } => tf!(
            "classic.pedia.describe.condition-near-improvement",
            n = *range,
            name = name(Table::Improvement, improvement.0)
        ),
        Condition::Module(condition) => classic::condition(*condition),
    }
}

fn plot_filter(plot: classic_rules::rules::effects::PlotFilter) -> &'static str {
    use classic_rules::rules::effects::PlotFilter;
    match plot {
        PlotFilter::River => t!("classic.pedia.describe.plot-river"),
        PlotFilter::Sea => t!("classic.pedia.describe.plot-sea"),
        PlotFilter::Lake => t!("classic.pedia.describe.plot-lake"),
        PlotFilter::SeaResource => t!("classic.pedia.describe.plot-sea-resource"),
        PlotFilter::NaturalWonder => t!("classic.pedia.describe.plot-natural-wonder"),
    }
}

/// The items of a production percent ("land units", "Ancient wonders").
fn production_filter(rules: &CompiledRuleset, filter: ProductionFilter) -> String {
    let name = |table: Table, index: u16| name_of(rules, PediaRef::new(table, index));
    match filter {
        ProductionFilter::Units => t!("classic.pedia.describe.production-units").into(),
        ProductionFilter::UnitDomain(domain) => tf!("classic.pedia.describe.production-kind-units", kind = domain_word(domain)),
        ProductionFilter::UnitRole(role) => tf!("classic.pedia.describe.production-kind-units", kind = role_words(role)),
        ProductionFilter::UnitClassOf(u) => tf!("classic.pedia.describe.production-a", name = name(Table::Unit, u.0)),
        ProductionFilter::MilitaryUnits => t!("classic.pedia.describe.production-military-units").into(),
        ProductionFilter::SpaceshipParts => t!("classic.pedia.describe.production-spaceship-parts").into(),
        ProductionFilter::Buildings => t!("classic.pedia.describe.production-buildings").into(),
        ProductionFilter::WorldWonders { before_era: None } => t!("classic.pedia.describe.production-world-wonders").into(),
        ProductionFilter::WorldWonders { before_era: Some(era) } => {
            tf!("classic.pedia.describe.production-world-wonders-before", era = name(Table::Era, era.0))
        }
        ProductionFilter::BuildingClassOf(b) => tf!("classic.pedia.describe.production-a", name = name(Table::Building, b.0)),
        ProductionFilter::BuiltInCapital => t!("classic.pedia.describe.production-built-in-capital").into(),
    }
}

/// A unit role in words ("naval melee", the classic "great person").
fn role_words(role: classic_rules::rules::content::UnitRole) -> String {
    use classic_rules::classic::content::{GREAT_PERSON_ROLE, RELIGIOUS_ROLE};
    use classic_rules::rules::content::UnitRole as R;
    match role {
        GREAT_PERSON_ROLE => t!("classic.pedia.describe.role-great-person").into(),
        RELIGIOUS_ROLE => t!("classic.pedia.describe.role-religious").into(),
        R::Civilian => t!("classic.pedia.describe.role-civilian").into(),
        R::SpaceshipPart => t!("classic.pedia.describe.role-spaceship-part").into(),
        R::Recon => t!("classic.pedia.describe.role-recon").into(),
        R::Melee => t!("classic.pedia.describe.role-melee").into(),
        R::Archer => t!("classic.pedia.describe.role-archer").into(),
        R::Mounted => t!("classic.pedia.describe.role-mounted").into(),
        R::Siege => t!("classic.pedia.describe.role-siege").into(),
        R::Gun => t!("classic.pedia.describe.role-gun").into(),
        R::Armor => t!("classic.pedia.describe.role-armor").into(),
        R::NavalMelee => t!("classic.pedia.describe.role-naval-melee").into(),
        R::NavalRanged => t!("classic.pedia.describe.role-naval-ranged").into(),
        R::Submarine => t!("classic.pedia.describe.role-submarine").into(),
        R::Carrier => t!("classic.pedia.describe.role-carrier").into(),
        R::Fighter => t!("classic.pedia.describe.role-fighter").into(),
        R::Bomber => t!("classic.pedia.describe.role-bomber").into(),
        R::Helicopter => t!("classic.pedia.describe.role-helicopter").into(),
        R::Missile => t!("classic.pedia.describe.role-missile").into(),
        _ => lower(role),
    }
}

fn domain_word(domain: classic_rules::rules::content::Domain) -> &'static str {
    use classic_rules::rules::content::Domain;
    match domain {
        Domain::Land => t!("classic.pedia.describe.domain-land"),
        Domain::Sea => t!("classic.pedia.describe.domain-sea"),
        Domain::Air => t!("classic.pedia.describe.domain-air"),
    }
}

fn resource_class_word(class: classic_rules::rules::content::ResourceClass) -> &'static str {
    use classic_rules::rules::content::ResourceClass;
    match class {
        ResourceClass::Bonus => t!("classic.pedia.describe.resource-class-bonus"),
        ResourceClass::Strategic => t!("classic.pedia.describe.resource-class-strategic"),
        ResourceClass::Luxury => t!("classic.pedia.describe.resource-class-luxury"),
        ResourceClass::Artifact => t!("classic.pedia.describe.resource-class-artifact"),
    }
}

/// A value's name in lower case words ("NavalMelee" -> "naval melee"); only for the roles of
/// a module that has no message.
fn lower(value: impl std::fmt::Debug) -> String {
    let mut words=String::new();
    for (index,letter) in format!("{value:?}").chars().enumerate() {
        if index>0 && letter.is_uppercase() { words.push(' '); }
        words.push(letter.to_ascii_lowercase());
    }
    words
}

/// The modifier as a phrase ("+2 Food", "-50% tile purchase cost").
pub fn modifier(rules: &CompiledRuleset, modifier: &Modifier) -> String {
    let name = |id: PediaRef| name_of(rules, id);
    match *modifier {
        Modifier::AddYield { yield_kind, amount } => {
            tf!("classic.pedia.describe.modifier-add-yield", amount = signed(amount), yield = yield_name(yield_kind))
        }
        Modifier::AddYieldPercent { yield_kind, percent } => {
            tf!("classic.pedia.describe.modifier-add-yield-percent", percent = signed(percent), yield = yield_name(yield_kind))
        }
        Modifier::MultiplyYield { yield_kind, percent } => tf!(
            "classic.pedia.describe.modifier-multiply-yield",
            factor = i18n::number(f64::from(percent) / 100.0, 2, 2),
            yield = yield_name(yield_kind)
        ),
        Modifier::ModifyGrowth { percent } => tf!("classic.pedia.describe.modifier-growth", percent = signed(percent)),
        Modifier::ModifyCost { target, percent } => {
            tf!("classic.pedia.describe.modifier-cost", percent = signed(percent), target = cost_name(target))
        }
        Modifier::AddYieldPerPopulation { yield_kind, hundredths } => {
            per_population(yield_kind, hundredths)
        }
        Modifier::ModifyCombat { context, percent } => {
            let text = tf!("classic.pedia.describe.modifier-combat", percent = signed(percent));
            match combat_context(context) {
                "" => text,
                context => tf!("classic.pedia.describe.modifier-combat-in", text = text, context = context),
            }
        }
        Modifier::ModifyMovement { amount } => tf!("classic.pedia.describe.modifier-movement", amount = signed(amount)),
        Modifier::ModifyCityCombat { stat, amount } => city_combat(stat, amount),
        Modifier::UnlockUnit(u) => tf!("classic.pedia.describe.modifier-unlocks", name = name(PediaRef::new(Table::Unit, u.0))),
        Modifier::UnlockBuilding(b) => tf!("classic.pedia.describe.modifier-unlocks", name = name(PediaRef::new(Table::Building, b.0))),
        Modifier::FreeUnit { unit, count } => {
            tf!("classic.pedia.describe.modifier-free-unit", count = count, name = name(PediaRef::new(Table::Unit, unit.0)))
        }
        Modifier::FreeBuilding(b) => tf!("classic.pedia.describe.modifier-free-building", name = name(PediaRef::new(Table::Building, b.0))),
        Modifier::AddPromotion(p) => tf!("classic.pedia.describe.modifier-promotion", name = name(PediaRef::new(Table::Promotion, p.0))),
        Modifier::FreeBuildingInCities { building, count } => tf!(
            "classic.pedia.describe.modifier-free-building-in-cities",
            count = count,
            name = name(PediaRef::new(Table::Building, building.0))
        ),
        Modifier::AddYieldPerCount { yield_kind, amount, per } => tf!(
            "classic.pedia.describe.modifier-yield-per-count",
            amount = signed(amount),
            yield = yield_name(yield_kind),
            per = per_count(per)
        ),
        Modifier::AddYieldPercentPerCount { yield_kind, percent, per } => tf!(
            "classic.pedia.describe.modifier-yield-percent-per-count",
            percent = signed(percent),
            yield = yield_name(yield_kind),
            per = per_count(per)
        ),
        Modifier::YieldFromKill { yield_kind, .. } => {
            tf!("classic.pedia.describe.modifier-yield-from-kill", yield = yield_name(yield_kind))
        }
        Modifier::Grant(grant) => grant_phrase(grant),
        Modifier::Forbid(Forbidden::UnitClassOf(u)) => {
            tf!("classic.pedia.describe.modifier-forbid-unit", name = name(PediaRef::new(Table::Unit, u.0)))
        }
        Modifier::Forbid(Forbidden::Annexing) => t!("classic.pedia.describe.modifier-forbid-annexing").into(),
        Modifier::AddStat { stat, amount } => stat_phrase(rules, stat, amount),
        Modifier::ModifyUnitStat { stat, amount } => unit_stat(stat,amount),
        Modifier::GrantAbility(ability) => combat_ability(ability).into(),
        Modifier::PostCombatPromotion(p) => {
            tf!("classic.pedia.describe.modifier-post-combat-promotion", name = name(PediaRef::new(Table::Promotion, p.0)))
        }
        Modifier::PassivePromotion(p) => {
            tf!("classic.pedia.describe.modifier-passive-promotion", name = name(PediaRef::new(Table::Promotion, p.0)))
        }
        Modifier::GrantExperience(amount) => tf!("classic.pedia.describe.modifier-experience", amount = amount),

        Modifier::Module(modifier) => classic::modifier(rules, modifier),
    }
}

fn espionage_phrase(stat: EspionageStat, amount: i32) -> String {
    match stat {
        EspionageStat::EnemyStealPercent => tf!("classic.pedia.describe.espionage-enemy-steal", amount = signed(amount)),
        EspionageStat::StealPercent => tf!("classic.pedia.describe.espionage-steal", amount = signed(amount)),
        EspionageStat::CatchPercent => tf!("classic.pedia.describe.espionage-catch", amount = signed(amount)),
        EspionageStat::RigElectionPercent => tf!("classic.pedia.describe.espionage-rig-election", amount = signed(amount)),
        EspionageStat::ExtraSpies => tf!("classic.pedia.describe.espionage-extra-spies", amount = signed(amount)),
        EspionageStat::StartingRank => tf!("classic.pedia.describe.espionage-starting-rank", amount = signed(amount)),
        EspionageStat::SpyPressure => tf!("classic.pedia.describe.espionage-spy-pressure", amount = amount),
    }
}

fn league_phrase(stat: LeagueStat, amount: i32) -> String {
    match stat {
        LeagueStat::Delegates => tf!("classic.pedia.describe.league-delegates", amount = signed(amount)),
        LeagueStat::DelegatesPerDiplomat => tf!("classic.pedia.describe.league-delegates-per-diplomat", amount = signed(amount)),
    }
}

/// A stat of the economy and society systems as a phrase.
fn stat_phrase(rules: &CompiledRuleset, stat: classic_rules::rules::stats::Stat, amount: i32) -> String {
    use classic_rules::rules::stats::{ClassicStat, Stat};
    let resource = |r: classic_rules::ids::ResourceId| name_of(rules, PediaRef::new(Table::Resource, r.0));
    match stat {
        Stat::ResourceCopies(r) => tf!("classic.pedia.describe.stat-resource-copies", amount = amount, resource = resource(r)),
        Stat::ResourceQuantityPercent(r) => {
            tf!("classic.pedia.describe.stat-resource-quantity", amount = signed(amount), resource = resource(r))
        }
        Stat::StrategicQuantityPercent => tf!("classic.pedia.describe.stat-strategic-quantity", amount = signed(amount)),
        Stat::ExtraLuxuryCopies => tf!("classic.pedia.describe.stat-extra-luxury-copies", amount = signed(amount)),
        Stat::Module(ClassicStat::GreatPeopleRate) => tf!("classic.pedia.describe.stat-great-people-rate", amount = signed(amount)),
        Stat::Module(ClassicStat::GreatPersonRate(kind)) => {
            tf!("classic.pedia.describe.stat-great-person-rate", amount = signed(amount), kind = great_person(kind))
        }
        Stat::FoodKept => tf!("classic.pedia.describe.stat-food-kept", amount = signed(amount)),
        Stat::SpecialistYield { yield_kind, .. } => {
            tf!("classic.pedia.describe.stat-specialist-yield", amount = signed(amount), yield = yield_name(yield_kind))
        }
        Stat::Module(ClassicStat::HappinessPerCity) => tf!("classic.pedia.describe.stat-happiness-per-city", amount = signed(amount)),
        Stat::Module(ClassicStat::HappinessPerLuxury) => tf!("classic.pedia.describe.stat-happiness-per-luxury", amount = signed(amount)),
        Stat::Module(ClassicStat::NaturalWonderHappinessPercent) => tf!("classic.pedia.describe.stat-natural-wonder-happiness", amount = signed(amount)),
        Stat::Module(ClassicStat::CityCountUnhappinessPercent) => tf!("classic.pedia.describe.stat-city-count-unhappiness", amount = signed(amount)),
        Stat::Module(ClassicStat::PopulationUnhappinessPercent) => tf!("classic.pedia.describe.stat-population-unhappiness", amount = signed(amount)),
        Stat::Module(ClassicStat::GoldenAgeLengthPercent) => tf!("classic.pedia.describe.stat-golden-age-length", amount = signed(amount)),
        Stat::Module(ClassicStat::Trade(trade)) => trade_phrase(trade, amount),
        Stat::Module(ClassicStat::ConnectionGoldPercent) => tf!("classic.pedia.describe.stat-connection-gold", amount = signed(amount)),
        Stat::Module(ClassicStat::ResearchAgreementPercent) => tf!("classic.pedia.describe.stat-research-agreement", amount = signed(amount)),
        Stat::Module(ClassicStat::GreatScientistSciencePercent) => tf!("classic.pedia.describe.stat-great-scientist-science", amount = signed(amount)),
        Stat::Module(ClassicStat::GreatPeopleRatePerFriend) => tf!("classic.pedia.describe.stat-great-people-per-friend", amount = signed(amount)),
        Stat::Module(ClassicStat::FreeMaintenanceUnits) => tf!("classic.pedia.describe.stat-free-maintenance-units", amount = signed(amount)),
        Stat::Module(ClassicStat::LandBarbarianConversionPercent) => tf!("classic.pedia.describe.stat-barbarian-conversion", amount = amount),
        Stat::Module(ClassicStat::PlunderGoldPercent) => tf!("classic.pedia.describe.stat-plunder-gold", amount = signed(amount)),
        Stat::Module(ClassicStat::CapturedGoldPercent) => tf!("classic.pedia.describe.stat-captured-gold", amount = signed(amount)),
        Stat::WorkerSpeedPercent => tf!("classic.pedia.describe.stat-worker-speed", amount = signed(amount)),
        Stat::RouteMaintenancePercent => tf!("classic.pedia.describe.stat-route-maintenance", amount = signed(amount)),
        Stat::Module(ClassicStat::Rule(rule)) => rule_phrase(rule).into(),
        Stat::Module(ClassicStat::Society(stat)) => society_phrase(stat, amount),
        Stat::Module(ClassicStat::Espionage(stat)) => espionage_phrase(stat, amount),
        Stat::Module(ClassicStat::League(stat)) => league_phrase(stat, amount),
        Stat::AnySpecialistYield(kind) => {
            tf!("classic.pedia.describe.stat-any-specialist-yield", amount = signed(amount), yield = yield_name(kind))
        }
    }
}

fn rule_phrase(rule: classic_rules::rules::stats::PlayerRule) -> &'static str {
    use classic_rules::rules::stats::PlayerRule;
    match rule {
        PlayerRule::GarrisonFreeMaintenance => t!("classic.pedia.describe.rule-garrison-free-maintenance"),
        PlayerRule::FreeHillsRouteMaintenance => t!("classic.pedia.describe.rule-free-hills-route-maintenance"),
        PlayerRule::SpaceshipPartPurchase => t!("classic.pedia.describe.rule-spaceship-part-purchase"),
        PlayerRule::SpaceshipPartHurry => t!("classic.pedia.describe.rule-spaceship-part-hurry"),
        PlayerRule::SeeBarbarianCamps => t!("classic.pedia.describe.rule-see-barbarian-camps"),
    }
}

fn trade_phrase(stat: classic_rules::rules::stats::TradeStat, amount: i32) -> String {
    use classic_rules::rules::stats::{TradeDomain, TradeStat};
    let domain = |d: TradeDomain| match d {
        TradeDomain::Land => t!("classic.pedia.describe.domain-land"),
        TradeDomain::Sea => t!("classic.pedia.describe.domain-sea"),
    };
    let a = signed(amount);
    match stat {
        TradeStat::Routes => tf!("classic.pedia.describe.trade-routes", amount = a),
        TradeStat::RangeTiles(d) => tf!("classic.pedia.describe.trade-range-tiles", amount = a, domain = domain(d)),
        TradeStat::RangePercent(d) => tf!("classic.pedia.describe.trade-range-percent", amount = a, domain = domain(d)),
        TradeStat::RouteGold(d) => tf!("classic.pedia.describe.trade-route-gold", amount = a, domain = domain(d)),
        TradeStat::TargetGold => tf!("classic.pedia.describe.trade-target-gold", amount = a),
        TradeStat::RecipientGold => tf!("classic.pedia.describe.trade-recipient-gold", amount = a),
        TradeStat::InternalPercent => tf!("classic.pedia.describe.trade-internal-percent", amount = a),
        TradeStat::RoutesPercent => tf!("classic.pedia.describe.trade-routes-percent", amount = a),
        TradeStat::ResourceGoldPercent => tf!("classic.pedia.describe.trade-resource-gold-percent", amount = a),
        TradeStat::IncomingRouteGold => tf!("classic.pedia.describe.trade-incoming-route-gold", amount = a),
        TradeStat::CityStateRouteGold => tf!("classic.pedia.describe.trade-city-state-route-gold", amount = a),
        TradeStat::SharedIdeologyRouteGold => tf!("classic.pedia.describe.trade-shared-ideology-route-gold", amount = a),
        TradeStat::ReligiousPressurePercent => tf!("classic.pedia.describe.trade-religious-pressure", amount = a),
    }
}

/// Where the map generator places a natural wonder, one sentence per rule.
pub fn placement_lines(
    rules: &CompiledRuleset,
    p: &classic_rules::rules::natural_wonders::NaturalWonderPlacement,
) -> Vec<Line> {
    use classic_rules::rules::natural_wonders::{
        AdjacentBound, AdjacentKind, CoreShape, PlacementMethod,
    };
    let terrain = |t: classic_rules::rules::content::TerrainKindId| name_of(rules, PediaRef::new(Table::Terrain, t.0));
    let mut out = Vec::new();
    let site = match p.method {
        PlacementMethod::Land => t!("classic.pedia.describe.placement-land"),
        PlacementMethod::Water => t!("classic.pedia.describe.placement-water"),
        PlacementMethod::Reef => t!("classic.pedia.describe.placement-reef"),
        PlacementMethod::PeninsulaTip => t!("classic.pedia.describe.placement-peninsula-tip"),
    };
    let mut first = site.to_string();
    if !p.core_terrains.is_empty() {
        let names: Vec<String> = p.core_terrains.iter().map(|t| terrain(*t)).collect();
        first = tf!("classic.pedia.describe.placement-of-terrains", text = first, terrains = join_or(names));
    }
    if p.avoid_ocean_coast {
        first = tf!("classic.pedia.describe.placement-away-from-coast", text = first);
    }
    if p.avoid_biggest_landmass {
        first = tf!("classic.pedia.describe.placement-off-largest-landmass", text = first);
    }
    out.push(Line::text(tf!("classic.pedia.describe.placement-sentence", text = first)));
    for rule in &p.adjacent {
        let what = match rule.kind {
            AdjacentKind::Hills => t!("classic.pedia.describe.adjacent-hills").to_string(),
            AdjacentKind::Mountain => t!("classic.pedia.describe.adjacent-mountains").to_string(),
            AdjacentKind::HillsOrMountain => t!("classic.pedia.describe.adjacent-hills-or-mountains").to_string(),
            AdjacentKind::Land => t!("classic.pedia.describe.adjacent-land").to_string(),
            AdjacentKind::Water => t!("classic.pedia.describe.adjacent-water").to_string(),
            AdjacentKind::Terrain(t) => terrain(t),
            AdjacentKind::Feature(f) => name_of(rules, PediaRef::new(Table::Feature, f.0)),
        };
        let text = match rule.bound {
            AdjacentBound::AtLeast(n) => tf!("classic.pedia.describe.adjacent-at-least", n = n, what = what),
            AdjacentBound::AtMost(0) => tf!("classic.pedia.describe.adjacent-none", what = what),
            AdjacentBound::AtMost(n) => tf!("classic.pedia.describe.adjacent-at-most", n = n, what = what),
        };
        out.push(Line::text(text));
    }
    let shape = match p.core_shape {
        CoreShape::Mountain => Some(t!("classic.pedia.describe.shape-mountain")),
        CoreShape::Flat => Some(t!("classic.pedia.describe.shape-flat")),
        CoreShape::Keep => None,
    };
    out.extend(shape.map(Line::text));
    if p.surround_with_water {
        out.push(Line::text(t!("classic.pedia.describe.surrounded-by-sea")));
    }
    out
}

fn grant_phrase(grant: Grant) -> String {
    match grant {
        Grant::FreeTechnologies(n) => tf!("classic.pedia.describe.grant-free-technologies", n = n),
        Grant::ConquestTechnology => t!("classic.pedia.describe.grant-conquest-technology").into(),
        Grant::MilitaryUnitCopies => t!("classic.pedia.describe.grant-military-unit-copies").into(),
        Grant::Population(n) => tf!("classic.pedia.describe.grant-population", n = n, amount = signed(n)),
        Grant::Module(grant) => classic::grant(grant),
    }
}

fn society_phrase(stat: SocietyStat, amount: i32) -> String {
    match stat {
        SocietyStat::SpecialistFoodPercent => tf!("classic.pedia.describe.society-specialist-food", amount = signed(amount)),
        SocietyStat::SpecialistUnhappinessPercent => tf!("classic.pedia.describe.society-specialist-unhappiness", amount = signed(amount)),
        SocietyStat::NoOccupiedUnhappiness => t!("classic.pedia.describe.society-no-occupied-unhappiness").into(),
        SocietyStat::WonderCulturePercent => tf!("classic.pedia.describe.society-wonder-culture", amount = signed(amount)),
        SocietyStat::HappinessToCulturePercent => t!("classic.pedia.describe.society-happiness-to-culture").into(),
        SocietyStat::ScienceWhileHappyPercent => tf!("classic.pedia.describe.society-science-while-happy", amount = signed(amount)),
        SocietyStat::LuxuryRetentionPercent => tf!("classic.pedia.describe.society-luxury-retention", amount = amount),
        SocietyStat::RazeSpeedPercent => tf!("classic.pedia.describe.society-raze-speed", amount = signed(amount)),
        SocietyStat::FoundingTerritoryTiles => tf!("classic.pedia.describe.society-founding-territory", amount = amount),
        SocietyStat::CapitalScienceBuildingBoost => t!("classic.pedia.describe.society-capital-science-boost").into(),
        SocietyStat::UniqueLuxuryCities => tf!("classic.pedia.describe.society-unique-luxury-cities", amount = amount),
        SocietyStat::UniqueLuxuryQuantity => tf!("classic.pedia.describe.society-unique-luxury-quantity", amount = amount),
        SocietyStat::UniqueLuxuryNewLandmass => t!("classic.pedia.describe.society-unique-luxury-new-landmass").into(),
    }
}

/// " per 10 citizens (at most 15)" without the leading space.
fn per_count(per: PerCount) -> String {
    let noun = match per.count {
        Count::Population => tf!("classic.pedia.describe.count-population", n = per.every),
        Count::WorldWonders => tf!("classic.pedia.describe.count-world-wonders", n = per.every),
        Count::ConnectedCities => tf!("classic.pedia.describe.count-connected-cities", n = per.every),
        Count::Module(count) => classic::count_name(count, per.every),
    };
    let step = if per.every == 1 {
        tf!("classic.pedia.describe.per-count-one", unit = noun)
    } else {
        tf!("classic.pedia.describe.per-count-many", every = per.every, unit = noun)
    };
    match per.max {
        Some(max) => tf!("classic.pedia.describe.per-count-capped", per = step, max = max),
        None => step,
    }
}

fn per_population(kind: YieldKind, hundredths: i32) -> String {
    if hundredths > 0 && 100 % hundredths == 0 {
        return tf!("classic.pedia.describe.per-population-step", n = 100 / hundredths, yield = yield_name(kind));
    }
    tf!("classic.pedia.describe.per-population", amount = hundredths_text(hundredths), yield = yield_name(kind))
}

/// 150 -> "+1.5", 200 -> "+2".
fn hundredths_text(value: i32) -> String {
    let sign = if value < 0 { '-' } else { '+' };
    format!("{sign}{}", i18n::number(f64::from(value.unsigned_abs()) / 100.0, 0, 2))
}

/// Importer annotations and AI classification are not player effects. They remain
/// in the catalog; suppress only these explicit inert metadata fields in the UI.
fn internal_unit_metadata(effect: &InertEffect) -> bool {
    let key=effect.modifier.split('=').next().unwrap_or(&effect.modifier);
    matches!(key, "unit.ability.civilian_attack_priority" | "unit.ability.special"
        | "unit.ability.work_rate" | "unit.ability.mechanized" | "unit.ability.policy_type")
}

fn city_combat(stat: classic_rules::rules::effects::CityCombatStat, amount:i32) -> String {
    use classic_rules::rules::effects::CityCombatStat as C;
    match stat {
        C::Defense => tf!("classic.pedia.describe.city-combat-defense", amount = hundredths_text(amount)),
        C::MaxHitPoints => tf!("classic.pedia.describe.city-combat-hit-points", amount = signed(amount)),
        C::DefensePercent => tf!("classic.pedia.describe.city-combat-defense-percent", amount = signed(amount)),
        C::GarrisonRangedStrikePercent => tf!("classic.pedia.describe.city-combat-garrison-ranged", amount = signed(amount)),
        C::NukeDamagePercent => tf!("classic.pedia.describe.city-combat-nuke-damage", amount = signed(amount)),
        C::AirCapacity => tf!("classic.pedia.describe.city-combat-air-capacity", amount = signed(amount)),
        C::RangedStrikePercent => tf!("classic.pedia.describe.city-combat-ranged", amount = signed(amount)),
    }
}

/// Exhaustive descriptions keep catalog numbers in their actual gameplay units.
pub fn unit_ability(ability:classic_rules::rules::content::UnitAbility) -> String {
    use classic_rules::rules::content::{CivilianPriority as C, SpecialUnit as S, UnitAbility as A};
    match ability {
        A::FoundReligion => t!("classic.pedia.describe.ability-found-religion").into(),
        A::SpreadReligion => t!("classic.pedia.describe.ability-spread-religion").into(),
        A::ReligionSpreads(n) => tf!("classic.pedia.describe.ability-religion-spreads", n = n),
        A::ReligiousStrength(n) => tf!("classic.pedia.describe.ability-religious-strength", n = n),
        A::RemoveHeresy => t!("classic.pedia.describe.ability-remove-heresy").into(),
        A::ProhibitsSpread => t!("classic.pedia.describe.ability-prohibits-spread").into(),
        A::RequiresEnhancedReligion => t!("classic.pedia.describe.ability-requires-enhanced-religion").into(),
        A::OneShotTourism(_) => t!("classic.pedia.describe.ability-one-shot-tourism").into(),
        A::OneShotTourismPercentOthers(n) => tf!("classic.pedia.describe.ability-one-shot-tourism-others", n = n),
        A::CultureTurnsToCount(_) => t!("classic.pedia.describe.ability-culture-turns").into(),
        A::Excavates => t!("classic.pedia.describe.ability-excavates").into(),
        A::Food => t!("classic.pedia.describe.ability-food").into(),
        A::GoldenAgeTurns(n) => tf!("classic.pedia.describe.ability-golden-age-turns", n = n),
        A::BeakersTurns(_) => t!("classic.pedia.describe.ability-beakers-turns").into(),
        A::BaseHurry(_) => t!("classic.pedia.describe.ability-base-hurry").into(),
        A::HurryMultiplier(_) => t!("classic.pedia.describe.ability-hurry-multiplier").into(),
        A::BaseGold(_) => t!("classic.pedia.describe.ability-base-gold").into(),
        A::GoldPerEra(_) => t!("classic.pedia.describe.ability-gold-per-era").into(),
        A::NoMaintenance => t!("classic.pedia.describe.ability-no-maintenance").into(),
        A::Trade => t!("classic.pedia.describe.ability-trade").into(),
        A::FaithPurchaseNeedsBelief => t!("classic.pedia.describe.ability-faith-purchase-needs-belief").into(),
        A::Mechanized => t!("classic.pedia.describe.ability-mechanized").into(),
        A::CivilianAttackPriority(p) => match p {
            C::Low => t!("classic.pedia.describe.ability-priority-low").into(),
            C::HighEarlyGameOnly => t!("classic.pedia.describe.ability-priority-early").into(),
            C::High => t!("classic.pedia.describe.ability-priority-high").into(),
        },
        A::Conscription(n) => tf!("classic.pedia.describe.ability-conscription", n = n),
        A::Special(S::People) => t!("classic.pedia.describe.ability-great-person").into(),
        A::Special(S::Stealth) => t!("classic.pedia.describe.ability-stealth").into(),
        A::AirSlots(n) => tf!("classic.pedia.describe.ability-air-slots", n = n),
        A::WorkRate(n) => tf!("classic.pedia.describe.ability-work-rate", n = n),
        A::MoveAfterPurchase => t!("classic.pedia.describe.ability-move-after-purchase").into(),
        A::NoBadGoodies => t!("classic.pedia.describe.ability-no-bad-goodies").into(),
        A::PurchaseOnly => t!("classic.pedia.describe.ability-purchase-only").into(),
        A::BuyCityState => t!("classic.pedia.describe.ability-buy-city-state").into(),
        A::FoundAbroad => t!("classic.pedia.describe.ability-found-abroad").into(),
        A::ExoticGoods(n) => tf!("classic.pedia.describe.ability-exotic-goods", n = n),
    }
}

fn unit_stat(stat: UnitStat, amount:i32) -> String {
    use UnitStat as U;
    let a = signed(amount);
    match stat {
        U::Sight => tf!("classic.pedia.describe.unit-stat-sight", amount = a),
        U::EmbarkedSight => tf!("classic.pedia.describe.unit-stat-embarked-sight", amount = a),
        U::Range => tf!("classic.pedia.describe.unit-stat-range", amount = a),
        U::AttacksPerTurn => tf!("classic.pedia.describe.unit-stat-attacks-per-turn", amount = a),
        U::Cargo => tf!("classic.pedia.describe.unit-stat-cargo", amount = a),
        U::InterceptChance => tf!("classic.pedia.describe.unit-stat-intercept-chance", amount = a),
        U::InterceptionsPerTurn => tf!("classic.pedia.describe.unit-stat-interceptions-per-turn", amount = a),
        U::EvasionPercent => tf!("classic.pedia.describe.unit-stat-evasion", amount = a),
        U::InterceptionDamageTakenPercent => tf!("classic.pedia.describe.unit-stat-interception-damage", amount = a),
        U::ParadropRange => tf!("classic.pedia.describe.unit-stat-paradrop-range", amount = a),
        U::NavalMoves => tf!("classic.pedia.describe.unit-stat-naval-moves", amount = a),
        U::ExperiencePercent => tf!("classic.pedia.describe.unit-stat-experience", amount = a),
        U::WithdrawChance => tf!("classic.pedia.describe.unit-stat-withdraw-chance", amount = a),
        U::CityAttackPlunderPercent => t!("classic.pedia.describe.unit-stat-city-attack-plunder").into(),
        U::HealEnemyLand => tf!("classic.pedia.describe.unit-stat-heal-enemy-land", amount = a),
        U::HealNeutralLand => tf!("classic.pedia.describe.unit-stat-heal-neutral-land", amount = a),
        U::HealFriendlyLand => tf!("classic.pedia.describe.unit-stat-heal-friendly-land", amount = a),
        U::HealAdjacent => tf!("classic.pedia.describe.unit-stat-heal-adjacent", amount = a),
        U::HealOnKill => tf!("classic.pedia.describe.unit-stat-heal-on-kill", amount = a),
        U::EnemyAuraPercent => tf!("classic.pedia.describe.unit-stat-enemy-aura", amount = a),
        U::AuraRange => tf!("classic.pedia.describe.unit-stat-aura-range", amount = a),
        U::AdjacentDamageOnAttack => tf!("classic.pedia.describe.unit-stat-adjacent-damage", amount = a),
        U::CapitalDefenseFalloff => t!("classic.pedia.describe.unit-stat-capital-defense-falloff").into(),
        U::EmbarkedMoves => tf!("classic.pedia.describe.unit-stat-embarked-moves", amount = a),
        U::WoundedDamagePercent => tf!("classic.pedia.describe.unit-stat-wounded-damage", amount = a),
        U::Module(stat) => classic::unit_stat(stat, amount),
    }
}

fn combat_ability(ability:classic_rules::rules::effects::UnitAbility) -> &'static str {
    use classic_rules::rules::effects::UnitAbility as A;
    match ability {
        A::MoveAfterAttacking => t!("classic.pedia.describe.combat-ability-move-after-attacking"),
        A::AlwaysHeal => t!("classic.pedia.describe.combat-ability-always-heal"),
        A::Embarkation => t!("classic.pedia.describe.combat-ability-embarkation"),
        A::EmbarkedAllWater => t!("classic.pedia.describe.combat-ability-embarked-all-water"),
        A::MoveImpassable => t!("classic.pedia.describe.combat-ability-move-impassable"),
        A::RangedSupportFire => t!("classic.pedia.describe.combat-ability-ranged-support-fire"),
        A::HillsDoubleMove => t!("classic.pedia.describe.combat-ability-hills-double-move"),
        A::DoubleMove => t!("classic.pedia.describe.combat-ability-double-move"),
        A::Impassable => t!("classic.pedia.describe.combat-ability-impassable"),
        A::InstaHeal => t!("classic.pedia.describe.combat-ability-insta-heal"),
        A::Amphibious => t!("classic.pedia.describe.combat-ability-amphibious"),
        A::RiverCrossing => t!("classic.pedia.describe.combat-ability-river-crossing"),
        A::IndirectFire => t!("classic.pedia.describe.combat-ability-indirect-fire"),
        A::HealOutsideFriendly => t!("classic.pedia.describe.combat-ability-heal-outside-friendly"),
        A::AirSweep => t!("classic.pedia.describe.combat-ability-air-sweep"),
        A::RivalTerritory => t!("classic.pedia.describe.combat-ability-rival-territory"),
        A::RoughTerrainEndsTurn => t!("classic.pedia.describe.combat-ability-rough-terrain-ends-turn"),
        A::IgnoreTerrainCost => t!("classic.pedia.describe.combat-ability-ignore-terrain-cost"),
        A::FlatMovementCost => t!("classic.pedia.describe.combat-ability-flat-movement-cost"),
        A::Hovering => t!("classic.pedia.describe.combat-ability-hovering"),
        A::NoCapture => t!("classic.pedia.describe.combat-ability-no-capture"),
        A::OnlyDefensive => t!("classic.pedia.describe.combat-ability-only-defensive"),
        A::NoDefensiveBonus => t!("classic.pedia.describe.combat-ability-no-defensive-bonus"),
        A::MustSetUp => t!("classic.pedia.describe.combat-ability-must-set-up"),
        A::FreePillageMoves => t!("classic.pedia.describe.combat-ability-free-pillage-moves"),
        A::CityAttackOnly => t!("classic.pedia.describe.combat-ability-city-attack-only"),
        A::CaptureDefeatedEnemy => t!("classic.pedia.describe.combat-ability-capture-defeated-enemy"),
        A::HeavyCharge => t!("classic.pedia.describe.combat-ability-heavy-charge"),
        A::Sapper => t!("classic.pedia.describe.combat-ability-sapper"),
        A::IgnoreZoneOfControl => t!("classic.pedia.describe.combat-ability-ignore-zone-of-control"),
        A::EmbarkedToLandFlatCost => t!("classic.pedia.describe.combat-ability-embarked-to-land-flat-cost"),
        A::HillsIgnoreTerrainCost => t!("classic.pedia.describe.combat-ability-hills-ignore-terrain-cost"),
        A::CrossMountains => t!("classic.pedia.describe.combat-ability-cross-mountains"),
        A::RoadInFriendlyLand => t!("classic.pedia.describe.combat-ability-road-in-friendly-land"),
        A::Invisible => t!("classic.pedia.describe.combat-ability-invisible"),
        A::SeeInvisible => t!("classic.pedia.describe.combat-ability-see-invisible"),
        A::Module(ability) => classic::ability(ability),
    }
}

pub fn yield_name(kind: YieldKind) -> &'static str {
    match kind {
        YieldKind::Output => t!("classic.pedia.describe.yield-output"),
        YieldKind::Food => t!("classic.pedia.describe.yield-food"),
        YieldKind::Production => t!("classic.pedia.describe.yield-production"),
        YieldKind::Gold => t!("classic.pedia.describe.yield-gold"),
        YieldKind::Science => t!("classic.pedia.describe.yield-science"),
        YieldKind::Culture => t!("classic.pedia.describe.yield-culture"),
        YieldKind::Module(kind) => classic::yield_kind_name(kind),
    }
}

fn cost_name(target: CostTarget) -> &'static str {
    match target {
        CostTarget::Units => t!("classic.pedia.describe.cost-units"),
        CostTarget::Buildings => t!("classic.pedia.describe.cost-buildings"),
        CostTarget::Technologies => t!("classic.pedia.describe.cost-technologies"),
        CostTarget::TilePurchase => t!("classic.pedia.describe.cost-tile-purchase"),
        CostTarget::Hurry => t!("classic.pedia.describe.cost-hurry"),
        CostTarget::BorderGrowth => t!("classic.pedia.describe.cost-border-growth"),
        CostTarget::BorderGrowthExponent => t!("classic.pedia.describe.cost-border-growth-exponent"),
        CostTarget::UnitUpgrade => t!("classic.pedia.describe.cost-unit-upgrade"),
        CostTarget::UnitPurchase => t!("classic.pedia.describe.cost-unit-purchase"),
        CostTarget::BuildingPurchase => t!("classic.pedia.describe.cost-building-purchase"),
        CostTarget::Module(target) => classic::cost_name(target),
    }
}

/// Where a combat bonus applies ("when attacking"); empty for any fight.
fn combat_context(context: CombatContext) -> &'static str {
    match context {
        CombatContext::Any => "",
        CombatContext::Attack => t!("classic.pedia.describe.context-attack"),
        CombatContext::Defense => t!("classic.pedia.describe.context-defense"),
        CombatContext::VsCity => t!("classic.pedia.describe.context-vs-city"),
        CombatContext::AttackOpen => t!("classic.pedia.describe.context-attack-open"),
        CombatContext::AttackRough => t!("classic.pedia.describe.context-attack-rough"),
        CombatContext::DefenseOpen => t!("classic.pedia.describe.context-defense-open"),
        CombatContext::DefenseRough => t!("classic.pedia.describe.context-defense-rough"),
        CombatContext::RangedAttackOpen => t!("classic.pedia.describe.context-ranged-attack-open"),
        CombatContext::RangedAttackRough => t!("classic.pedia.describe.context-ranged-attack-rough"),
        CombatContext::DefenseVsRanged => t!("classic.pedia.describe.context-defense-vs-ranged"),
        CombatContext::RangedAttack => t!("classic.pedia.describe.context-ranged-attack"),
        CombatContext::VsWounded => t!("classic.pedia.describe.context-vs-wounded"),
        CombatContext::VsFortified => t!("classic.pedia.describe.context-vs-fortified"),
        CombatContext::FriendlyLand => t!("classic.pedia.describe.context-friendly-land"),
        CombatContext::OutsideFriendlyLand => t!("classic.pedia.describe.context-outside-friendly-land"),
        CombatContext::AttackFriendlyLand => t!("classic.pedia.describe.context-attack-friendly-land"),
        CombatContext::Flanking => t!("classic.pedia.describe.context-flanking"),
        CombatContext::AdjacentFriendly => t!("classic.pedia.describe.context-adjacent-friendly"),
        CombatContext::NearCapital => t!("classic.pedia.describe.context-near-capital"),
        CombatContext::EmbarkedDefense => t!("classic.pedia.describe.context-embarked-defense"),
        CombatContext::Interception => t!("classic.pedia.describe.context-interception"),
        CombatContext::AirSweep => t!("classic.pedia.describe.context-air-sweep"),
    }
}

/// An inert catalog effect: "Tile purchase cost: -50% (all cities)". The subject and the
/// trigger are the catalog's own identifiers read as words; the frame is the language's.
pub fn inert_effect(rules: &CompiledRuleset, e: &InertEffect) -> String {
    let (subject, unit) = modifier_subject(&e.modifier);
    let mut text = capitalize(&subject);
    let amount = amount_text(e.amount, unit);
    if !amount.is_empty() {
        text = tf!("classic.pedia.describe.inert-amount", subject = text, amount = amount);
    }
    let names: Vec<String> = e.refs.iter().map(|r| ref_name(rules, r)).collect();
    if !names.is_empty() {
        text = tf!("classic.pedia.describe.inert-refs", text = text, names = join_list(names));
    }
    if e.trigger != "passive" && !e.trigger.is_empty() {
        text = tf!("classic.pedia.describe.inert-trigger", trigger = capitalize(&e.trigger.replace('_', " ")), text = text);
    }
    text
}

/// "combat.attack_vs_wounded.pct" -> ("combat attack vs wounded", Percent).
fn modifier_subject(modifier: &str) -> (String, Unit) {
    let mut parts: Vec<&str> = modifier.split('.').collect();
    let unit = match parts.last().copied() {
        Some("pct") | Some("percent") | Some("mod") => Unit::Percent,
        Some("flat") | Some("change") => Unit::Flat,
        _ => Unit::Plain,
    };
    if unit != Unit::Plain && parts.len() > 1 {
        parts.pop();
    }
    let subject = parts.join(" ").replace('_', " ");
    (subject, unit)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Unit {
    Percent,
    Flat,
    Plain,
}

fn amount_text(amount: InertAmount, unit: Unit) -> String {
    let suffix = if unit == Unit::Percent { "%" } else { "" };
    match amount {
        InertAmount::None | InertAmount::Flag => String::new(),
        InertAmount::Int(v) if unit == Unit::Plain && v == 1 => String::new(),
        InertAmount::Int(v) if unit == Unit::Plain => i18n::number_int(i64::from(v)),
        InertAmount::Int(v) => format!("{}{suffix}", signed(i64::from(v))),
        InertAmount::Hundredths(v) => format!("{}{suffix}", hundredths_text(v as i32)),
    }
}

/// Name of a validated inert reference (tables by name, vocabulary by key).
fn ref_name(rules: &CompiledRuleset, r: &ContentRef) -> String {
    match content_ref(rules, r) {
        Some(id) => name_of(rules, id),
        None => pretty(&r.key),
    }
}

/// The Encyclopedia page of a validated reference (None for vocabulary and classes).
pub fn content_ref(rules: &CompiledRuleset, r: &ContentRef) -> Option<PediaRef> {
    use classic_rules::classic::tables;
    let table = match r.table {
        ContentTable::Unit => Table::Unit,
        ContentTable::Building => Table::Building,
        ContentTable::Technology => Table::Technology,
        ContentTable::Improvement => Table::Improvement,
        ContentTable::Feature => Table::Feature,
        ContentTable::Resource => Table::Resource,
        ContentTable::Civilization => Table::Civilization,
        ContentTable::Promotion => Table::Promotion,
        tables::POLICIES => Table::Policy,
        tables::BELIEFS => Table::Belief,
        ContentTable::Era => Table::Era,
        ContentTable::Terrain => Table::Terrain,
        ContentTable::NaturalWonder => Table::NaturalWonder,
        ContentTable::Project => Table::Project,
        tables::POLICY_BRANCHES => Table::PolicyBranch,
        tables::RELIGIONS => Table::Religion,
        tables::CITY_STATE_TYPES => Table::CityStateType,
        tables::CITY_STATES => Table::CityState,
        ContentTable::Specialist => Table::Specialist,
        tables::GREAT_PEOPLE => Table::GreatPerson,
        _ => return None,
    };
    let id = PediaRef::new(table, r.index);
    super::table_len(rules, table).filter(|len| (r.index as usize) < *len)?;
    Some(id)
}

#[allow(unreachable_patterns)]
fn modifier_ref(modifier: &Modifier) -> Option<PediaRef> {
    match *modifier {
        Modifier::UnlockUnit(u) | Modifier::FreeUnit { unit: u, .. } => Some(PediaRef::new(Table::Unit, u.0)),
        Modifier::UnlockBuilding(b)
        | Modifier::FreeBuilding(b)
        | Modifier::FreeBuildingInCities { building: b, .. } => Some(PediaRef::new(Table::Building, b.0)),
        Modifier::AddPromotion(p) => Some(PediaRef::new(Table::Promotion, p.0)),
        Modifier::Forbid(Forbidden::UnitClassOf(u)) => Some(PediaRef::new(Table::Unit, u.0)),
        Modifier::Module(modifier) => classic::modifier_ref(modifier),
        Modifier::AddStat {
            stat:
                classic_rules::rules::stats::Stat::ResourceCopies(r)
                | classic_rules::rules::stats::Stat::ResourceQuantityPercent(r),
            ..
        } => Some(PediaRef::new(Table::Resource, r.0)),
        _ => None,
    }
}

fn condition_ref(condition: &Condition) -> Option<PediaRef> {
    match *condition {
        Condition::HasTechnology(t) | Condition::LacksTechnology(t) | Condition::LearnedTechnology(t) => {
            Some(PediaRef::new(Table::Technology, t.0))
        }
        Condition::HasBuilding(b) => Some(PediaRef::new(Table::Building, b.0)),
        Condition::TileImprovement(i) | Condition::NearOwnImprovement { improvement: i, .. } => {
            Some(PediaRef::new(Table::Improvement, i.0))
        }
        Condition::TileResource(r) => Some(PediaRef::new(Table::Resource, r.0)),
        Condition::TileFeature(f)
        | Condition::TileFeatureUnimproved(f)
        | Condition::CityAdjacentUnimprovedFeature { feature: f, .. } => Some(PediaRef::new(Table::Feature, f.0)),
        Condition::TileTerrain(t) | Condition::TileTerrainUntil(t, _) => Some(PediaRef::new(Table::Terrain, t.0)),
        Condition::OpponentClassOf(u) => Some(PediaRef::new(Table::Unit, u.0)),
        Condition::TilePlot(_) | Condition::TileResourceClass(_) => None,
        Condition::Always
        | Condition::MinPopulation(_)
        | Condition::CityOnRiver
        | Condition::AtPeace
        | Condition::ConnectedToCapital
        | Condition::EventNearSourceCity(_)
        | Condition::OpponentRole(_)
        | Condition::OpponentDomain(_)
        | Condition::UnitDomain(_)
        | Condition::UnitRole(_) => None,
        Condition::Producing(filter) => match filter {
            ProductionFilter::UnitClassOf(u) => Some(PediaRef::new(Table::Unit, u.0)),
            ProductionFilter::BuildingClassOf(b) => Some(PediaRef::new(Table::Building, b.0)),
            ProductionFilter::WorldWonders { before_era: Some(era) } => Some(PediaRef::new(Table::Era, era.0)),
            _ => None,
        },
        Condition::Module(_)
        | Condition::CityGarrisoned
        | Condition::CityCoastal => None,
    }
}

/// Display name of any ruleset entity.
pub fn name_of(rules: &CompiledRuleset, id: PediaRef) -> String {
    super::entity_name(rules, id).unwrap_or_else(|| t!("classic.pedia.describe.unknown").into())
}

/// Building capabilities are player actions or consequences, never debug enum names.
pub fn building_ability(ability: classic_rules::rules::content::BuildingAbility) -> String {
    use classic_rules::rules::content::BuildingAbility as B;
    match ability {
        B::TriggersIdeologyChoice(n) => tf!("classic.pedia.describe.building-ideology-choice", n = n),
        B::FreeGreatWork => t!("classic.pedia.describe.building-free-great-work").into(),
        B::NullifyInfluence => t!("classic.pedia.describe.building-nullify-influence").into(),
        B::MissionaryExtraSpreads(n) => tf!("classic.pedia.describe.building-missionary-extra-spreads", n = n),
        B::ReligiousPressure(n) => tf!("classic.pedia.describe.building-religious-pressure", n = n),
        B::FoodTradeRoutes => t!("classic.pedia.describe.building-food-trade-routes").into(),
        B::ProductionTradeRoutes => t!("classic.pedia.describe.building-production-trade-routes").into(),
        B::CounterEspionage => t!("classic.pedia.describe.building-counter-espionage").into(),
        B::AffectSpiesNow => t!("classic.pedia.describe.building-affect-spies-now").into(),
        B::BorderObstacle => t!("classic.pedia.describe.building-border-obstacle").into(),
        B::Airlift => t!("classic.pedia.describe.building-airlift").into(),
        B::AllowsRangeStrike => t!("classic.pedia.describe.building-allows-range-strike").into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_read_like_the_game() {
        assert_eq!(hundredths_text(50), "+0.5");
        assert_eq!(hundredths_text(250), "+2.5");
        assert_eq!(hundredths_text(-200), "-2");
        assert_eq!(per_population(YieldKind::Science, 50), "+1 Science per 2 citizens");
        assert_eq!(modifier_subject("combat.attack_vs_wounded.pct").0, "combat attack vs wounded");
        assert_eq!(amount_text(InertAmount::Int(-50), Unit::Percent), "-50%");
        assert_eq!(pretty("core:unit-combat/naval-melee"), "Naval Melee");
    }

    #[test]
    fn every_effect_of_the_ruleset_reads_as_a_sentence() {
        let rules = crate::ui::screens::game_rules();
        let mut count = 0;
        for unit in &rules.units {
            for line in effect_lines(&rules, &unit.effects, &unit.inert) {
                assert!(!line.text.is_empty() && !line.text.contains("Unknown"), "{}: {}", unit.key, line.text);
                count += 1;
            }
        }
        for policy in &rules.module.policies {
            for line in effect_lines(&rules, &policy.effects, &policy.inert) {
                assert!(!line.text.contains("Unknown"), "{}: {}", policy.key, line.text);
                count += 1;
            }
        }
        assert!(count > 100, "{count} effect lines");
    }
}

#[cfg(test)]
mod ability_presentation_tests {
    use super::*;
    use classic_rules::rules::effects::CityCombatStat;

    #[test]
    fn city_combat_numbers_use_strength_hit_points_percent_and_aircraft_units() {
        assert_eq!(city_combat(CityCombatStat::Defense,500),"+5 city combat strength from defenses");
        assert_eq!(city_combat(CityCombatStat::MaxHitPoints,50),"+50 city hit points");
        assert_eq!(city_combat(CityCombatStat::DefensePercent,25),"+25% defensive strength from buildings");
        assert_eq!(city_combat(CityCombatStat::GarrisonRangedStrikePercent,50),"+50% city ranged attack strength while garrisoned");
        assert_eq!(city_combat(CityCombatStat::NukeDamagePercent,-50),"-50% damage received from nuclear attacks");
        assert_eq!(city_combat(CityCombatStat::AirCapacity,4),"+4 aircraft capacity");
    }

    #[test]
    fn every_shipped_unit_and_building_ability_has_an_explicit_player_phrase() {
        let rules=crate::ui::screens::game_rules();
        for unit in &rules.units {
            for ability in &unit.module.abilities {
                let text=unit_ability(*ability);
                assert!(text.contains(' '),"{}: {text}",unit.key);
                assert_ne!(text,format!("{ability:?}"),"{}: raw enum",unit.key);
                assert!(!text.contains(['(',')','{','}']),"{}: {text}",unit.key);
            }
        }
        for building in &rules.buildings {
            for ability in &building.module.abilities {
                let text=building_ability(*ability);
                assert!(text.contains(' '),"{}: {text}",building.key);
                assert_ne!(text,format!("{ability:?}"),"{}: raw enum",building.key);
            }
        }
    }
}

#[cfg(test)]
mod combat_effect_presentation_tests {
    use super::*;
    use classic_rules::classic::effects::{CityStateStat, ClassicModifier};
    use classic_rules::rules::effects::UnitAbility;

    #[test]
    fn adjacent_combat_families_preserve_counts_percentages_and_content_names() {
        let rules=crate::ui::screens::game_rules();
        for (effect,phrase) in [
            (Modifier::ModifyUnitStat { stat:UnitStat::Sight,amount:2 },"+2 tiles of sight"),
            (Modifier::ModifyUnitStat { stat:UnitStat::InterceptChance,amount:25 },"+25 percentage points to interception chance"),
            (Modifier::ModifyUnitStat { stat:UnitStat::HealOnKill,amount:50 },"+50 HP healed after destroying an enemy unit"),
            (Modifier::GrantAbility(UnitAbility::MoveAfterAttacking),"can move after attacking"),
            (Modifier::GrantExperience(15),"+15 experience points"),
            (Modifier::Module(ClassicModifier::ModifyCityState { stat:CityStateStat::RestingPoint,amount:20 }),"+20 resting influence with city-states"),
            (Modifier::Module(ClassicModifier::ModifyCityState { stat:CityStateStat::FriendshipRate,amount:100 }),"+100% influence recovery with city-states; -50% influence decay"),
            (Modifier::Module(ClassicModifier::ModifyCityState { stat:CityStateStat::FriendshipRate,amount:40 }),"+40% influence recovery with city-states; -20% influence decay"),
            (Modifier::Module(ClassicModifier::ModifyCityState { stat:CityStateStat::AllyResourceBonus,amount:1 }),"allied city-states give twice their resources, their luxuries 50% more happiness"),
            (Modifier::Module(ClassicModifier::ModifyCityState { stat:CityStateStat::AllyScience,amount:1 }),"a quarter of the science of allied city-states"),
            (Modifier::Module(ClassicModifier::ModifyCityState { stat:CityStateStat::AnnexCityStates,amount:1 }),"may buy out city-states allied for five turns"),
        ] { assert_eq!(modifier(&rules,&effect),phrase); }
        let promotion=classic_rules::ids::PromotionId(0);
        let name=name_of(&rules,PediaRef::new(Table::Promotion,promotion.0));
        assert_eq!(modifier(&rules,&Modifier::PassivePromotion(promotion)),format!("eligible units receive the effects of the {name} promotion"));
        assert_eq!(modifier(&rules,&Modifier::PostCombatPromotion(promotion)),format!("may gain the {name} promotion after its first combat"));
        assert_eq!(lower(classic_rules::rules::content::UnitRole::NavalMelee),"naval melee");
    }
}
