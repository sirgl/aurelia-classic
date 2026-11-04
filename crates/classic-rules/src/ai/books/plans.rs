//! The five books as data: each one's research beeline, policy line, settling
//! count, opening steps and attack army, resolved for one nation from the
//! rules' units (class, `nation`, `replaces`).
//!
//! LEAGUE OPPONENTS AND PROBES ONLY: the books name content (unit classes,
//! technologies, policy branches) as the human strategies they copy name it.
//! They never become `standard`, whose decisions read roles and effects only
//! (classic/docs/ai-books.md).

use crate::ai::books::Book;
use crate::ids::{TechId, UnitKindId};
use crate::rules::content::Domain;
use crate::rules::CompiledRuleset;

/// One pick of a slot, in the slot's order of preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pick {
    /// The nation's unique unit of this class, when it has one.
    Unique(&'static str),
    /// The nation's unit of this class: its unique unit, else the generic one.
    Class(&'static str),
}

/// What a city builds for an opening step, counted against `count`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Want {
    /// Recon units, while the target capital is unknown.
    Recon(u32),
    /// Workers.
    Workers(u32),
    /// One worker for every this many cities (at least one).
    WorkersPerCities(u32),
    /// Settlers until the cities, the living settlers and the queued ones make this many.
    Settle(u32),
    /// The army slot of this index (`Plan::army`).
    Army(usize),
}

/// Units of one role of the attack army: the kinds in their order of
/// preference (the first the rules accept is built) and how many.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slot {
    pub kinds: Vec<UnitKindId>,
    pub count: u32,
    /// The slot's own arm: its kinds up to the first one the nation builds
    /// without a resource, and the kinds that upgrade to one of them (archers
    /// to composite bowmen). Only these count toward the army; the rest are
    /// built only when no core kind can come (a siege slot never fills with
    /// warriors while its catapults are researched).
    pub core: Vec<UnitKindId>,
}

/// A book resolved for one nation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// Researched in this order while the book plays (each by `PlanResearch`, its
    /// prerequisites first).
    pub techs: Vec<TechId>,
    /// Policy branches in this order: the first unlockable one is unlocked, the
    /// first unlocked one with a policy left adopts it.
    pub branches: Vec<crate::rules::content::PolicyBranchId>,
    /// The cities the book settles before its attack or its end (the rushes
    /// attack from their capital alone: a settler is two archers early on).
    pub cities: u32,
    /// The build order: every city takes the first step not yet met.
    pub steps: Vec<Want>,
    /// The attack army (empty: no timing attack).
    pub army: Vec<Slot>,
    /// The attack waits for this technology (rush-archer: Construction).
    pub ready_tech: Option<TechId>,
    /// Standard-speed rounds: from this round the attack goes with two thirds
    /// of its army.
    pub force_round: u32,
    /// Standard-speed rounds: no attack starts after this round, and a book
    /// without an attack ends here.
    pub last_round: u32,
    /// Reinforcements built during the attack, beyond the army.
    pub reinforce: u32,
}

impl Plan {
    /// The units of every army slot, without repetition.
    pub fn army_kinds(&self) -> Vec<UnitKindId> {
        let mut out: Vec<UnitKindId> = self.army.iter().flat_map(|s| s.kinds.iter().copied()).collect();
        out.sort_by_key(|k| k.0);
        out.dedup();
        out
    }

    pub fn attacks(&self) -> bool {
        !self.army.is_empty()
    }
}

fn unit_by_key(rules: &CompiledRuleset, key: &str) -> Option<UnitKindId> {
    rules.units.iter().position(|u| u.key == key).map(|i| UnitKindId(i as u16))
}

fn tech_by_key(rules: &CompiledRuleset, key: &str) -> Option<TechId> {
    rules.technologies.iter().position(|t| t.key == key).map(|i| TechId(i as u16))
}

fn branch_by_key(rules: &CompiledRuleset, key: &str) -> Option<crate::rules::content::PolicyBranchId> {
    rules.module.policy_branches.iter().position(|b| b.key == key).map(|i| crate::rules::content::PolicyBranchId(i as u16))
}

fn class_key(class: &str) -> String {
    format!("core:unit-class/{class}")
}

/// The nation's unique unit of `class`, if it has one.
fn unique_of(rules: &CompiledRuleset, nation: u8, class: &str) -> Option<UnitKindId> {
    let key = class_key(class);
    rules.units.iter().position(|u| u.nation == Some(nation) && u.class.as_deref() == Some(key.as_str())).map(|i| UnitKindId(i as u16))
}

/// The generic unit of `class` (no nation).
fn generic_of(rules: &CompiledRuleset, class: &str) -> Option<UnitKindId> {
    let key = class_key(class);
    rules.units.iter().position(|u| u.nation.is_none() && u.class.as_deref() == Some(key.as_str()) && !u.unbuildable).map(|i| UnitKindId(i as u16))
}

/// The nation's unit for a generic one: its unique unit replacing it, else the generic one.
pub fn nation_unit(rules: &CompiledRuleset, nation: u8, generic: UnitKindId) -> UnitKindId {
    rules.units.iter().position(|u| u.nation == Some(nation) && u.replaces == Some(generic)).map_or(generic, |i| UnitKindId(i as u16))
}

fn resolve(rules: &CompiledRuleset, nation: u8, picks: &[Pick]) -> Vec<UnitKindId> {
    let mut out = Vec::new();
    for pick in picks {
        let kind = match pick {
            Pick::Unique(class) => unique_of(rules, nation, class),
            Pick::Class(class) => unique_of(rules, nation, class).or_else(|| generic_of(rules, class)),
        };
        if let Some(kind) = kind.filter(|k| !out.contains(k)) {
            out.push(kind);
        }
    }
    out
}

/// A slot of `kinds` with its core (`Slot::core`).
fn core_slot(rules: &CompiledRuleset, nation: u8, kinds: Vec<UnitKindId>, count: u32) -> Slot {
    let free = |k: &UnitKindId| rules.units[k.0 as usize].nation == Some(nation) || rules.units[k.0 as usize].required_resources.is_empty();
    let end = kinds.iter().position(free).map_or(kinds.len(), |i| i + 1);
    let mut core: Vec<UnitKindId> = kinds[..end].to_vec();
    let upgrades = |k: &UnitKindId, core: &[UnitKindId]| rules.units[k.0 as usize].upgrades_to.iter().any(|u| core.contains(u) || core.iter().any(|c| rules.units[c.0 as usize].replaces == Some(*u)));
    let extra: Vec<UnitKindId> = kinds[end..].iter().copied().filter(|k| upgrades(k, &core)).collect();
    core.extend(extra);
    Slot { kinds, count, core }
}

/// The technology the beeline takes for a slot: that of its first kind that is
/// the nation's own or needs no resource (a generic unit that needs one is
/// built when the rules accept it, not researched for).
fn slot_tech(rules: &CompiledRuleset, nation: u8, slot: &Slot) -> Option<TechId> {
    slot.kinds
        .iter()
        .map(|k| &rules.units[k.0 as usize])
        .find(|u| u.nation == Some(nation) || u.required_resources.is_empty())
        .and_then(|u| u.required_tech)
}

/// A land combat unit's strength: the larger of melee and ranged.
fn power(rules: &CompiledRuleset, kind: UnitKindId) -> u16 {
    let u = &rules.units[kind.0 as usize];
    u.strength.max(u.ranged_strength)
}

fn ranged(rules: &CompiledRuleset, kind: UnitKindId) -> bool {
    let u = &rules.units[kind.0 as usize];
    u.range > 0 && u.ranged_strength > 0
}

/// A unit's technology is of the eras a timing unit may come from (none: the start).
fn timing_era(rules: &CompiledRuleset, kind: UnitKindId) -> bool {
    let era = rules.units[kind.0 as usize].required_tech.and_then(|t| rules.technologies.get(t.0 as usize)).and_then(|t| t.era_name.as_deref());
    era.is_none_or(|e| TIMING_ERAS.contains(&e))
}

/// The eras a timing unit may come from: early or medieval.
const TIMING_ERAS: [&str; 3] = ["Ancient", "Classical", "Medieval"];

/// `timing-uu`'s unit: the nation's strongest ancient to medieval unique land
/// combat unit; without one, the strongest generic land combat unit of those eras
/// that needs no resource and no building.
pub fn timing_unit(rules: &CompiledRuleset, nation: u8) -> Option<UnitKindId> {
    let land_fighter = |i: usize| {
        let u = &rules.units[i];
        u.domain == Domain::Land && power(rules, UnitKindId(i as u16)) > 0 && !u.unbuildable && u.cost > 0 && !u.founder && !u.builder
            && u.role != Some(crate::rules::content::UnitRole::Recon)
            && timing_era(rules, UnitKindId(i as u16))
    };
    let best = |filter: &dyn Fn(usize) -> bool| {
        (0..rules.units.len())
            .filter(|i| land_fighter(*i) && filter(*i))
            .map(|i| UnitKindId(i as u16))
            .max_by_key(|k| (power(rules, *k), std::cmp::Reverse(rules.units[k.0 as usize].cost), std::cmp::Reverse(k.0)))
    };
    best(&|i| rules.units[i].nation == Some(nation)).or_else(|| {
        best(&|i| {
            let u = &rules.units[i];
            u.nation.is_none() && u.required_resources.is_empty() && u.requires_buildings.is_empty()
        })
    })
}

/// Standard-speed rounds of the books' timings.
mod timing {
    pub const SIEGE_FORCE: u32 = 55;
    pub const SIEGE_LAST: u32 = 95;
    pub const ARCHER_FORCE: u32 = 55;
    pub const ARCHER_LAST: u32 = 95;
    pub const SWORD_FORCE: u32 = 60;
    pub const SWORD_LAST: u32 = 100;
    pub const UU_FORCE: u32 = 130;
    pub const UU_LAST: u32 = 175;
    pub const REX_LAST: u32 = 130;
}

impl Plan {
    /// `book` for the nation `nation` of `rules`.
    pub fn of(book: Book, rules: &CompiledRuleset, nation: u8) -> Plan {
        use Pick::{Class, Unique};
        let slot = |picks: &[Pick], count: u32| core_slot(rules, nation, resolve(rules, nation, picks), count);
        let techs = |keys: &[&str]| keys.iter().filter_map(|k| tech_by_key(rules, &format!("core:tech/{k}"))).collect::<Vec<_>>();
        let branches = |keys: &[&str]| keys.iter().filter_map(|k| branch_by_key(rules, &format!("core:policy/{k}"))).collect::<Vec<_>>();
        let mut plan = match book {
            // The Huns' rush: rams (or catapults) and horse archers (or archers),
            // about four ranged and three siege or melee units at the capital as
            // early as they exist.
            Book::RushSiege => Plan {
                techs: Vec::new(),
                branches: branches(&["honor", "tradition"]),
                cities: 1,
                steps: vec![Want::Recon(2), Want::Workers(1), Want::Army(0), Want::Army(1), Want::Army(2)],
                army: vec![
                    slot(&[Unique("chariot-archer"), Unique("archer"), Class("chariot-archer"), Class("archer"), Class("composite-bowman")], 4),
                    slot(&[Unique("spearman"), Class("catapult"), Class("spearman"), Class("warrior")], 3),
                    // The melee units that take the city once the walls are down.
                    slot(&[Unique("spearman"), Class("spearman"), Unique("warrior"), Class("warrior")], 2),
                ],
                ready_tech: None,
                force_round: timing::SIEGE_FORCE,
                last_round: timing::SIEGE_LAST,
                reinforce: 3,
            },
            // Five archers timed with Construction (composite bowmen) and melee
            // units to take the city.
            Book::RushArcher => Plan {
                techs: techs(&["archery", "bronze-working", "construction"]),
                branches: branches(&["honor", "tradition"]),
                cities: 1,
                steps: vec![Want::Recon(2), Want::Workers(1), Want::Army(0), Want::Army(1)],
                army: vec![
                    slot(&[Unique("composite-bowman"), Unique("archer"), Class("composite-bowman"), Class("archer")], 5),
                    slot(&[Unique("spearman"), Unique("warrior"), Class("spearman"), Class("warrior")], 2),
                ],
                ready_tech: tech_by_key(rules, "core:tech/construction"),
                force_round: timing::ARCHER_FORCE,
                last_round: timing::ARCHER_LAST,
                reinforce: 3,
            },
            // Iron Working first, then swordsmen and a few archers.
            Book::RushSword => Plan {
                techs: techs(&["iron-working", "archery"]),
                branches: branches(&["honor", "tradition"]),
                cities: 1,
                steps: vec![Want::Recon(2), Want::Workers(1), Want::Army(0), Want::Army(1)],
                army: vec![
                    slot(&[Class("swordsman"), Unique("spearman"), Class("spearman"), Class("warrior")], 4),
                    slot(&[Unique("composite-bowman"), Unique("archer"), Class("composite-bowman"), Class("archer")], 2),
                ],
                ready_tech: None,
                force_round: timing::SWORD_FORCE,
                last_round: timing::SWORD_LAST,
                reinforce: 3,
            },
            // The nation's strongest early or medieval unique unit, massed at its timing.
            Book::TimingUu => {
                let main = timing_unit(rules, nation);
                let mut kinds: Vec<UnitKindId> = main.into_iter().collect();
                if let Some(generic) = main.and_then(|m| rules.units[m.0 as usize].replaces) {
                    kinds.push(generic);
                }
                // Without its resource the unit is not built: the strongest that needs none.
                let fallback = timing_unit(rules, u8::MAX);
                kinds.extend(fallback.filter(|f| !kinds.contains(f)));
                let main_ranged = main.is_some_and(|m| ranged(rules, m));
                let support = if main_ranged {
                    slot(&[Class("pikeman"), Unique("spearman"), Class("spearman"), Class("warrior")], 2)
                } else {
                    slot(&[Class("crossbowman"), Class("composite-bowman"), Class("archer")], 2)
                };
                Plan {
                    techs: Vec::new(),
                    branches: branches(&["tradition", "honor"]),
                    cities: 3,
                    steps: vec![Want::Recon(1), Want::Workers(1), Want::Settle(2), Want::Settle(3), Want::WorkersPerCities(2), Want::Army(0), Want::Army(1)],
                    army: vec![core_slot(rules, nation, kinds, 6), support],
                    ready_tech: None,
                    force_round: timing::UU_FORCE,
                    last_round: timing::UU_LAST,
                    reinforce: 3,
                }
            }
            // Liberty, as many cities as the land allows, then growth.
            Book::RexWide => Plan {
                techs: techs(&["pottery", "animal-husbandry", "calendar", "mining", "bronze-working"]),
                branches: branches(&["liberty"]),
                cities: 8,
                steps: vec![Want::Recon(1), Want::Settle(3), Want::Workers(1), Want::Army(0), Want::Settle(8), Want::WorkersPerCities(2)],
                army: Vec::new(),
                ready_tech: None,
                force_round: timing::REX_LAST,
                last_round: timing::REX_LAST,
                reinforce: 0,
            },
        };
        if book == Book::RexWide {
            // Guards against the barbarians, not an army: built by `Want::Army(0)`.
            plan.army = vec![slot(&[Unique("warrior"), Class("warrior"), Class("spearman")], 2)];
            plan.reinforce = 0;
        }
        if plan.techs.is_empty() || book == Book::RushArcher || book == Book::RushSword {
            for slot in &plan.army {
                if let Some(tech) = slot_tech(rules, nation, slot).filter(|t| !plan.techs.contains(t)) {
                    plan.techs.push(tech);
                }
            }
        }
        plan
    }

    /// The book's settler: the nation's settler.
    pub fn settler(rules: &CompiledRuleset, nation: u8) -> Option<UnitKindId> {
        unit_by_key(rules, "core:unit/settler").map(|k| nation_unit(rules, nation, k))
    }

    /// The book's worker.
    pub fn worker(rules: &CompiledRuleset, nation: u8) -> Option<UnitKindId> {
        unit_by_key(rules, "core:unit/worker").map(|k| nation_unit(rules, nation, k))
    }

    /// The book's recon unit.
    pub fn recon(rules: &CompiledRuleset, nation: u8) -> Option<UnitKindId> {
        unit_by_key(rules, "core:unit/scout").map(|k| nation_unit(rules, nation, k))
    }
}
