//! One palace per nation: which city holds it, and what a save of an older build is repaired to.
//!
//! The rule is Unciv 4.22.10's, and Unciv keeps it at one place, `CityConquestFunctions.moveToCiv`
//! (every way a city changes owner ends there: `conquerCity` for a conquest and a liberation,
//! `TradeLogic` for a deal, `CityStateFunctions` for the annexation of a city-state):
//!
//! * a city that holds the capital indicator (`City.isCapital`: a building with `IndicatesCapital`,
//!   here the Palace) hands it over before it leaves: `Civilization.moveCapitalToNextLargest` gives
//!   it to the largest city of the old owner that is not a puppet, else to the largest puppet, which
//!   is annexed; the old owner with no other city has no palace;
//! * a city that arrives in a civilization with no other city gets the indicator
//!   (`Civilization.moveCapitalTo`);
//! * `City.destroyCity` hands the palace over the same way.
//!
//! So a nation with a city holds exactly one palace, and the palace never travels with a city.
//! The transaction half of the rule is `WorldTransaction::strip_palace` and `restore_palace`
//! (`systems::combat::palace`), called from `transfer_city` and `remove_city`.

use crate::ids::{BuildingKindId, CityHandle, PlayerId};
use crate::rules::content::BuildingKind;
use crate::rules::CompiledRuleset;
use crate::world::city::{CityCore, CityStatus, CityWrite};
use crate::world::state::AuthWorld;

/// The palace of the ruleset, as a building id (the building of kind Palace; None without one).
pub fn palace_building(rules: &CompiledRuleset) -> Option<BuildingKindId> {
    kernel::systems::city_founding::palace(rules)
}

/// The city holds a building of kind Palace.
pub fn holds_palace(rules: &CompiledRuleset, core: &CityCore) -> bool {
    crate::world::city::is_capital(rules, core)
}

/// The city gets the palace. The handle comes from the nation's own city list, so it is live: a stale
/// one is a broken invariant of the world, and the game stops there.
pub fn give_palace(auth: &mut AuthWorld, city: CityHandle, palace: BuildingKindId) {
    match auth.cities.add_building(city, palace) {
        CityWrite::Applied => {}
        CityWrite::TargetGone | CityWrite::QueueFull => panic!("give_palace: the city {city:?} is listed for its owner but gone"),
    }
}

/// The city loses its palace.
pub fn remove_palace(rules: &CompiledRuleset, core: &mut CityCore) {
    core.buildings.retain(|b| rules.buildings.get(b.0 as usize).is_none_or(|d| d.kind != BuildingKind::Palace));
}

/// The cities of `owner` that hold a palace, oldest first (one while the invariant holds).
pub fn palace_cities(auth: &AuthWorld, rules: &CompiledRuleset, owner: PlayerId) -> Vec<CityHandle> {
    let mut out = Vec::new();
    for &handle in auth.indexes().cities_of(owner) {
        if auth.cities().get(handle).is_some_and(|core| holds_palace(rules, core)) {
            out.push(handle);
        }
    }
    out
}

/// Unciv `Civilization.moveCapitalToNextLargest`, the choice: among the cities of `owner` that hold
/// no palace, the one with the most citizens that is not a puppet (the oldest on a tie), else the
/// largest puppet. None when `owner` has no such city.
pub fn next_capital(auth: &AuthWorld, rules: &CompiledRuleset, owner: PlayerId) -> Option<CityHandle> {
    let free = |core: &CityCore| !holds_palace(rules, core);
    largest_city(auth, owner, |core| free(core) && core.status != CityStatus::Puppet)
        .or_else(|| largest_city(auth, owner, free))
}

fn largest_city(auth: &AuthWorld, owner: PlayerId, wanted: impl Fn(&CityCore) -> bool) -> Option<CityHandle> {
    let mut best: Option<(CityHandle, u32)> = None;
    for &handle in auth.indexes().cities_of(owner) {
        let Some(core) = auth.cities().get(handle) else {
            continue;
        };
        if !wanted(core) {
            continue;
        }
        match best {
            Some((_, population)) if population >= core.population => {}
            _ => best = Some((handle, core.population)),
        }
    }
    best.map(|(handle, _)| handle)
}

/// A nation that has cities and does not hold exactly one palace (the invariant the rules keep and
/// `ClassicWorld::validate` checks); the text names it. A ruleset without a palace has none to keep.
pub fn palace_problem(auth: &AuthWorld, rules: &CompiledRuleset) -> Option<String> {
    palace_building(rules)?;
    for index in 0..auth.players().count() {
        let owner = PlayerId(index as u8);
        if auth.indexes().cities_of(owner).is_empty() {
            continue;
        }
        let palaces = palace_cities(auth, rules, owner).len();
        if palaces != 1 {
            return Some(format!("player {index} holds {palaces} palaces in {} cities", auth.indexes().cities_of(owner).len()));
        }
    }
    None
}

/// A world loaded from a save of a build that let a palace travel with a city (a buyout, a deal, a
/// liberation) or let a nation lose its palace in a conquest: every nation with cities holds one
/// palace. A nation with several keeps the one in its original capital, else the oldest; a nation
/// with none gets it in its original capital if it still holds it, else by `next_capital`. A world
/// that keeps the invariant is not touched.
pub fn repair_palaces(auth: &mut AuthWorld, rules: &CompiledRuleset) {
    let Some(palace) = palace_building(rules) else {
        return;
    };
    for index in 0..auth.players().count() {
        let owner = PlayerId(index as u8);
        let holders = palace_cities(auth, rules, owner);
        if holders.len() == 1 || auth.indexes().cities_of(owner).is_empty() {
            continue;
        }
        let heir = if holders.is_empty() { original_capital_of(auth, owner).or_else(|| next_capital(auth, rules, owner)) } else { None };
        let kept = holders.iter().copied().find(|h| Some(*h) == original_capital_of(auth, owner)).or(holders.first().copied());
        for holder in holders.iter().filter(|h| Some(**h) != kept) {
            if let Some(core) = auth.cities.get_mut(*holder) {
                remove_palace(rules, core);
            }
        }
        if let Some(city) = heir {
            give_palace(auth, city, palace);
        }
    }
}

/// The city of `owner` that was its first, if it still owns it.
fn original_capital_of(auth: &AuthWorld, owner: PlayerId) -> Option<CityHandle> {
    auth.indexes().cities_of(owner).iter().copied().find(|h| {
        auth.cities().get(*h).is_some_and(|core| core.original_capital && core.original_owner == owner)
    })
}
