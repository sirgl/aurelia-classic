//! The unit kind a city orders, chosen by what the unit is for (`army-fit`,
//! classic/docs/ai-army.md). The game's AI orders the land unit with the highest
//! `strength` for every purpose (`planner_cities::unit_of_role`), whatever it meets; this
//! module rates each kind the player may train by the rules' own combat: its strength with
//! the percents of its own effects and its free promotions in the strike's contexts, those
//! against the opponent's role, domain or class among them, put through the rules' damage
//! formulas (`combat::damage`, the expected roll) in a duel with each kind the foes field.
//!
//! - **Hold** (a garrison, a defender of a city in danger): the duel's exchange, the damage
//!   the kind deals over the damage it takes when it strikes the foe and the foe strikes it;
//!   per unit (a city holds a few), the cheaper on a near tie.
//! - **Take** (the melee member of an army sent against a city): the duel and the exchange
//!   with the target city (its strike back, the kind's percents against cities), per its
//!   price — hammers at the allocator's price and upkeep to the end; only kinds that capture.
//! - **Strike** (the ranged member): the same for kinds with a range.
//!
//! The foes are the land military units the player sees of those it is at war with (the
//! barbarians among them) and of the enemy of its war operation; with none in sight, the
//! kinds the player itself may train (an even match). Nothing is named: every number is the
//! rules' data, the prices the allocator's.

use crate::ai::planner::{role_of, AiRole};
use crate::ai::state::ArmyRole;
use crate::ai::value::{Centi, Prices};
use crate::combat::damage::{melee_damage, ranged_damage, MeleeSide, Roll};
use crate::ids::{PlayerId, UnitKindId};
use crate::knowledge::observation::AIObservation;
use crate::rules::content::Domain;
use crate::rules::effects::{CombatContext, Condition, Modifier, UnitAbility};
use crate::rules::CompiledRuleset;

/// What a unit is ordered for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    Hold,
    Take,
    Strike,
}

/// The kinds the foes field, each with the count seen.
#[derive(Debug, Clone, Default)]
pub struct Foes {
    pub kinds: Vec<(UnitKindId, u32)>,
}

impl Foes {
    /// The land military units in sight of the owners the player is at war with (the
    /// barbarians among them) and of `enemy` (the target of a war it prepares).
    pub fn seen(obs: &AIObservation<'_>, rules: &CompiledRuleset, enemy: Option<PlayerId>) -> Self {
        let mut kinds: Vec<(UnitKindId, u32)> = Vec::new();
        for unit in &obs.visible_foreign_units {
            let hostile = obs.relation(unit.owner).is_some_and(|r| r.at_war) || Some(unit.owner) == enemy;
            if !hostile || role_of(rules, unit.kind) != AiRole::Military {
                continue;
            }
            match kinds.iter_mut().find(|(k, _)| *k == unit.kind) {
                Some((_, n)) => *n += 1,
                None => kinds.push((unit.kind, 1)),
            }
        }
        kinds.sort_by_key(|(k, _)| k.0);
        Self { kinds }
    }

    /// The foes, or `mirror` (one of each) when none is seen.
    fn or_mirror(&self, mirror: &[UnitKindId]) -> Vec<(UnitKindId, u32)> {
        if self.kinds.is_empty() {
            mirror.iter().map(|k| (*k, 1)).collect()
        } else {
            self.kinds.clone()
        }
    }
}

/// The kind's percents in `contexts` (its own effects and its free promotions), with the
/// terms against `foe`'s role, domain or class; terrain terms and owner conditions are left
/// out (the place of the fight is not known).
fn percent(rules: &CompiledRuleset, kind: UnitKindId, contexts: &[CombatContext], foe: Option<UnitKindId>) -> i64 {
    let Some(def) = rules.units.get(kind.0 as usize) else { return 0 };
    let foe_def = foe.and_then(|f| rules.units.get(f.0 as usize));
    let holds = |condition: Condition| match condition {
        Condition::Always | Condition::UnitRole(_) | Condition::UnitDomain(_) => true,
        Condition::OpponentRole(role) => foe_def.is_some_and(|f| f.role == Some(role)),
        Condition::OpponentDomain(domain) => foe_def.is_some_and(|f| f.domain == domain),
        Condition::OpponentClassOf(k) => {
            let class = rules.units.get(k.0 as usize).and_then(|d| d.class.as_ref());
            class.is_some() && class == foe_def.and_then(|f| f.class.as_ref())
        }
        _ => false,
    };
    let promotions = def.free_promotions.iter().filter_map(|p| rules.promotions.get(p.0 as usize)).map(|p| &p.effects.passive_unit);
    std::iter::once(&def.effects.passive_unit)
        .chain(promotions)
        .flatten()
        .filter_map(|e| match e.modifier {
            Modifier::ModifyCombat { context, percent } if contexts.contains(&context) && holds(e.condition) => Some(i64::from(percent)),
            _ => None,
        })
        .sum()
}

/// The kind has the ability by its own effects or its free promotions.
pub fn has_ability(rules: &CompiledRuleset, kind: UnitKindId, ability: UnitAbility) -> bool {
    let Some(def) = rules.units.get(kind.0 as usize) else { return false };
    let promotions = def.free_promotions.iter().filter_map(|p| rules.promotions.get(p.0 as usize)).map(|p| &p.effects.passive_unit);
    std::iter::once(&def.effects.passive_unit)
        .chain(promotions)
        .flatten()
        .any(|e| matches!(e.modifier, Modifier::GrantAbility(a) if a == ability) && !e.condition.is_situational())
}

/// A kind with a range that strikes from afar (a siege kind too).
fn shoots(rules: &CompiledRuleset, kind: UnitKindId) -> bool {
    rules.units.get(kind.0 as usize).is_some_and(|d| d.range > 0 && d.ranged_strength > 0)
}

/// Strength in hundredths: base × (100 + the percents).
fn strength(base: u16, percent: i64) -> i64 {
    (i64::from(base) * (100 + percent)).max(100)
}

/// One round of a duel of `me` with `foe`: `me` strikes (a shot, or a melee charge and its
/// answer), then `foe` strikes back the same way. (damage `me` deals, damage it takes).
fn duel(rules: &CompiledRuleset, me: UnitKindId, foe: UnitKindId) -> (i64, i64) {
    let (Some(m), Some(f)) = (rules.units.get(me.0 as usize), rules.units.get(foe.0 as usize)) else { return (0, 1) };
    let expected = Roll::Expected;
    let (mut dealt, mut taken) = (0i64, 0i64);
    // Our strike.
    if shoots(rules, me) {
        let a = strength(m.ranged_strength, percent(rules, me, &[CombatContext::Any, CombatContext::Attack, CombatContext::RangedAttack], Some(foe)));
        let d = strength(f.strength, percent(rules, foe, &[CombatContext::Any, CombatContext::Defense, CombatContext::DefenseVsRanged], Some(me)));
        let mut shot = i64::from(ranged_damage(a, d, 0, 0, expected));
        // A kind that must set up first shoots every other round in the field.
        if has_ability(rules, me, UnitAbility::MustSetUp) {
            shot /= 2;
        }
        dealt += shot;
    } else if !has_ability(rules, me, UnitAbility::OnlyDefensive) {
        let a = strength(m.strength, percent(rules, me, &[CombatContext::Any, CombatContext::Attack], Some(foe)));
        let d = strength(f.strength, percent(rules, foe, &[CombatContext::Any, CombatContext::Defense], Some(me)));
        dealt += i64::from(melee_damage(a, d, 0, 0, expected, MeleeSide::UnitVsUnit));
        taken += i64::from(melee_damage(d, a, 0, 0, expected, MeleeSide::UnitVsUnit));
    }
    // Their strike.
    if shoots(rules, foe) {
        let a = strength(f.ranged_strength, percent(rules, foe, &[CombatContext::Any, CombatContext::Attack, CombatContext::RangedAttack], Some(me)));
        let d = strength(m.strength, percent(rules, me, &[CombatContext::Any, CombatContext::Defense, CombatContext::DefenseVsRanged], Some(foe)));
        let mut shot = i64::from(ranged_damage(a, d, 0, 0, expected));
        if has_ability(rules, foe, UnitAbility::MustSetUp) {
            shot /= 2;
        }
        taken += shot;
    } else if !has_ability(rules, foe, UnitAbility::OnlyDefensive) {
        let a = strength(f.strength, percent(rules, foe, &[CombatContext::Any, CombatContext::Attack], Some(me)));
        let d = strength(m.strength, percent(rules, me, &[CombatContext::Any, CombatContext::Defense], Some(foe)));
        taken += i64::from(melee_damage(a, d, 0, 0, expected, MeleeSide::UnitVsUnit));
        dealt += i64::from(melee_damage(d, a, 0, 0, expected, MeleeSide::UnitVsUnit));
    }
    (dealt, taken.max(1))
}

/// The exchange (per mille: damage dealt over damage taken) of `kind` with the foes,
/// weighted by their counts.
pub fn field_exchange(rules: &CompiledRuleset, kind: UnitKindId, foes: &[(UnitKindId, u32)]) -> i64 {
    let (mut sum, mut weight) = (0i64, 0i64);
    for &(foe, n) in foes {
        let (dealt, taken) = duel(rules, kind, foe);
        sum += dealt * 1000 / taken * i64::from(n);
        weight += i64::from(n);
    }
    if weight == 0 { 1000 } else { sum / weight }
}

/// The exchange (per mille) of `kind` with a city of strength `city` (hundredths): its blow
/// or shot and the city's strike back (the city strikes a unit next to it as hard as it
/// answers a melee blow).
fn city_exchange(rules: &CompiledRuleset, kind: UnitKindId, city: i64) -> i64 {
    let Some(def) = rules.units.get(kind.0 as usize) else { return 0 };
    let vs_city = [CombatContext::Any, CombatContext::Attack, CombatContext::VsCity];
    if shoots(rules, kind) {
        let a = strength(def.ranged_strength, percent(rules, kind, &[CombatContext::Any, CombatContext::Attack, CombatContext::RangedAttack, CombatContext::VsCity], None));
        let d = strength(def.strength, percent(rules, kind, &[CombatContext::Any, CombatContext::Defense, CombatContext::DefenseVsRanged], None));
        let dealt = i64::from(ranged_damage(a, city, 0, 0, Roll::Expected));
        let taken = i64::from(crate::combat::damage::city_strike_damage(city, d, Roll::Expected)).max(1);
        dealt * 1000 / taken
    } else {
        let a = strength(def.strength, percent(rules, kind, &vs_city, None));
        let dealt = i64::from(melee_damage(a, city, 0, 0, Roll::Expected, MeleeSide::UnitVsCity));
        let taken = i64::from(melee_damage(city, a, 0, 0, Roll::Expected, MeleeSide::CityVsUnit)).max(1);
        dealt * 1000 / taken
    }
}

/// What a new unit of `kind` costs: its hammers at the allocator's price and its upkeep to the
/// end (None without prices: its hammers).
fn price(rules: &CompiledRuleset, kind: UnitKindId, prices: Option<&Prices>, upkeep: Centi) -> i64 {
    let hammers = i64::from(rules.units.get(kind.0 as usize).map_or(1, |d| d.cost.max(1)));
    match prices {
        Some(p) => (hammers * p.production / i64::from(p.rounds_left.max(1)) + upkeep).max(1),
        None => hammers,
    }
}

/// A rated candidate: (score, cost, kind).
type Rated = (i64, u32, UnitKindId);

/// The kind to order for `purpose` among `candidates` (the land military kinds the player may
/// train now): None when none fits. `city` is the strength (hundredths) of the city an army
/// is sent against; `upkeep` the price of one more unit's upkeep to the end.
pub fn pick(rules: &CompiledRuleset, candidates: &[UnitKindId], purpose: Purpose, foes: &Foes, city: Option<i64>, prices: Option<&Prices>, upkeep: Centi) -> Option<UnitKindId> {
    let fit: Vec<UnitKindId> = candidates
        .iter()
        .copied()
        .filter(|k| rules.units.get(k.0 as usize).is_some_and(|d| d.domain == Domain::Land))
        .filter(|k| match purpose {
            // A garrison defends in melee: no kind that must set up or strikes only from afar
            // with a weak body (a ranged kind defends with its melee strength like any other).
            Purpose::Hold => !has_ability(rules, *k, UnitAbility::MustSetUp),
            Purpose::Take => crate::ai::operations::army_role(rules, *k) == Some(ArmyRole::Melee) && !has_ability(rules, *k, UnitAbility::NoCapture),
            Purpose::Strike => shoots(rules, *k),
        })
        .collect();
    let foes = foes.or_mirror(candidates);
    let mut best: Option<Rated> = None;
    for kind in fit {
        let cost = rules.units[kind.0 as usize].cost;
        let field = field_exchange(rules, kind, &foes);
        let score = match purpose {
            Purpose::Hold => field,
            Purpose::Take | Purpose::Strike => {
                let siege = city.map_or(field, |c| city_exchange(rules, kind, c));
                // The field and the walls weigh alike; per its price (per mille of a price unit).
                (field + siege) * 1_000_000 / price(rules, kind, prices, upkeep)
            }
        };
        // Per unit: a kind within 5 % of the best is as good, and the cheaper wins.
        let better = match best {
            None => true,
            Some((s, c, k)) => match purpose {
                Purpose::Hold if score * 100 >= s * 105 => true,
                Purpose::Hold if score * 105 > s * 100 => cost < c || (cost == c && kind.0 < k.0),
                Purpose::Hold => false,
                _ => score > s || (score == s && (cost, kind.0) < (c, k.0)),
            },
        };
        if better {
            best = Some((score, cost, kind));
        }
    }
    best.map(|(_, _, k)| k)
}
