//! Belief effects over the world (read-only): which belief effects reach a
//! city (its majority's beliefs, the holy city's founder beliefs), the
//! founder beliefs of a player's religion, and the numbers of a religion
//! (`ModifyReligion`). Count-based modifiers (per following city, per
//! followers) become plain flat amounts here, so the consumers see
//! `AddYield` / `AddHappiness` only.

use crate::classic::effects::{ClassicModifier, ClassicSelector};
use crate::effects::Bucket;
use crate::ids::{CityHandle, PlayerId};
use crate::rules::content::ReligionId;
use crate::rules::effects::{CompiledEffect, Modifier, Selector};
use crate::classic::effects::{ReligionAbility, RELIGION_ABILITIES, RELIGION_STATS};
use crate::rules::CompiledRuleset;
use crate::world::city::CityCore;
use crate::world::religion::Creed;
use crate::world::state::AuthWorld;

/// Percent changes of one religion's numbers, index = `ReligionStat::index()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ReligionModifiers {
    pub percent: [i32; RELIGION_STATS],
    /// Faith of the founder's next prophet (Messiah −25).
    pub prophet_cost: i32,
    /// Index = `ReligionAbility::index()`.
    pub abilities: [bool; RELIGION_ABILITIES],
}

impl ReligionModifiers {
    pub fn of(&self, stat: crate::classic::effects::ReligionStat) -> i32 {
        self.percent[stat.index()]
    }

    pub fn has(&self, ability: ReligionAbility) -> bool {
        self.abilities[ability.index()]
    }
}

pub fn religion_modifiers(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    religion: ReligionId,
) -> ReligionModifiers {
    let mut out = ReligionModifiers::default();
    let Some(founded) = auth.module().religion().religion(religion) else {
        return out;
    };
    let at = crate::effects::ConditionContext {
        owner: founded.founder,
        city: None,
        tile: None,
        unit: None,
        producing: None,
    };
    for belief in &founded.beliefs {
        let Some(def) = rules.module.beliefs.get(belief.0 as usize) else {
            continue;
        };
        for effect in &def.effects.passive_module {
            if !crate::effects::effect_holds(auth, rules, at, effect) {
                continue;
            }
            match effect.modifier {
                Modifier::Module(ClassicModifier::ModifyReligion { stat, percent }) => {
                    out.percent[stat.index()] += percent;
                    if stat == crate::classic::effects::ReligionStat::ProphetCost {
                        out.prophet_cost += percent;
                    }
                }
                Modifier::Module(ClassicModifier::GrantReligionAbility(ability)) => out.abilities[ability.index()] = true,
                _ => {}
            }
        }
    }
    out
}

/// Visit the belief effects of `bucket` reaching `core`: the following
/// effects of its majority's beliefs and, for a holy city, the holy-city
/// effects of its religion. Count-based modifiers arrive as flat amounts.
pub(crate) fn visit_city_beliefs(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    core: &CityCore,
    bucket: Bucket,
    visit: &mut dyn FnMut(&CompiledEffect, u32),
) {
    if !rules.effect_index.module.has_passive_belief {
        return;
    }
    // No pantheon and no religion yet: no belief reaches any city.
    if auth.module().religion().pantheons() == 0 && auth.module().religion().religions().is_empty() {
        return;
    }
    let Some(handle) = auth.indexes().city_on_tile(core.tile) else {
        return;
    };
    let majority =
        crate::derived::religion::majority(auth.module().religion().city(handle), core.population);
    let mut beliefs = beliefs_of(auth, majority, core.owner);
    if let Some(extra) = second_religion_pantheon(auth, rules, core, majority) {
        if !beliefs.contains(&extra) {
            beliefs.push(extra);
        }
    }
    for belief in &beliefs {
        let Some(def) = rules.module.beliefs.get(belief.0 as usize) else {
            continue;
        };
        for effect in bucket(&def.effects) {
            if matches!(
                effect.selector,
                Selector::Module(ClassicSelector::FollowingCities) | Selector::Module(ClassicSelector::FollowingCityTiles)
            ) {
                visit(effect, 1);
            }
        }
    }
    visit_holy_city(auth, rules, handle, bucket, visit);
}

fn visit_holy_city(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    handle: CityHandle,
    bucket: Bucket,
    visit: &mut dyn FnMut(&CompiledEffect, u32),
) {
    let Some(founded) = auth.module().religion().holy_city_of(handle) else {
        return;
    };
    let religion = founded.religion;
    for belief in &founded.beliefs {
        let Some(def) = rules.module.beliefs.get(belief.0 as usize) else {
            continue;
        };
        for effect in bucket(&def.effects) {
            if effect.selector != Selector::Module(ClassicSelector::HolyCity) {
                continue;
            }
            visit(&counted(auth, religion, effect), 1);
        }
    }
}

/// The pantheon belief of a city's second religion when its owner adopted
/// a `SecondReligionPantheon` policy (Religious Tolerance): the city has a
/// religion as its majority, and the founded religion with the most
/// followers after it (ties: more pressure, then religion order) has a
/// pantheon belief.
fn second_religion_pantheon(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    core: &CityCore,
    majority: Option<Creed>,
) -> Option<crate::ids::BeliefId> {
    let policies = &rules.effect_index.module.second_pantheon_policies;
    if policies.is_empty() {
        return None;
    }
    let adopted = &auth.module().culture().of(core.owner).policies;
    if !policies.iter().any(|p| adopted.contains(p)) {
        return None;
    }
    let Some(Creed::Religion(first)) = majority else {
        return None;
    };
    let second = second_religion(auth, core, first)?;
    let founded = auth.module().religion().religion(second)?;
    founded.beliefs.iter().copied().find(|b| {
        rules.module.beliefs[b.0 as usize].category == Some(crate::rules::content::BeliefCategory::Pantheon)
    })
}

/// The founded religion other than `first` with the most followers in the
/// city (at least one; ties: more pressure, then religion order).
fn second_religion(auth: &AuthWorld, core: &CityCore, first: ReligionId) -> Option<ReligionId> {
    let handle = auth.indexes().city_on_tile(core.tile)?;
    let faith = auth.module().religion().city(handle);
    let mut best: Option<(ReligionId, u32, u32)> = None;
    for (creed, followers) in crate::derived::religion::followers(faith, core.population) {
        let Creed::Religion(religion) = creed else {
            continue;
        };
        if religion == first {
            continue;
        }
        let pressure = faith.pressure_of(creed);
        let better = match best {
            None => true,
            Some((_, n, p)) => followers > n || (followers == n && pressure > p),
        };
        if better {
            best = Some((religion, followers, pressure));
        }
    }
    best.map(|(religion, _, _)| religion)
}

/// The beliefs of a majority creed for a city of `owner`.
pub(crate) fn beliefs_of(
    auth: &AuthWorld,
    majority: Option<Creed>,
    owner: PlayerId,
) -> Vec<crate::ids::BeliefId> {
    match majority {
        Some(Creed::Pantheon) => auth.module().religion().player(owner).pantheon.into_iter().collect(),
        Some(Creed::Religion(r)) => match auth.module().religion().religion(r) {
            Some(founded) => founded.beliefs.clone(),
            None => Vec::new(),
        },
        Some(Creed::Atheism) | None => Vec::new(),
    }
}

/// The beliefs of `owner`'s own creed: its pantheon and the religion it
/// founded (whose list starts with that pantheon), each once.
fn creed_beliefs(auth: &AuthWorld, owner: PlayerId) -> Vec<crate::ids::BeliefId> {
    let faith = auth.module().religion().player(owner);
    let mut out: Vec<crate::ids::BeliefId> = faith.pantheon.into_iter().collect();
    let founded = faith.religion.and_then(|r| auth.module().religion().religion(r));
    if let Some(founded) = founded {
        for belief in &founded.beliefs {
            if !out.contains(belief) {
                out.push(*belief);
            }
        }
    }
    out
}

/// Visit the Unit-scope effects of `owner`'s own creed (Defender of the
/// Faith, Faith Healers): they reach its units; their situational
/// condition (`Condition::NearFollowingCity`) is checked where the unit
/// fights or heals.
pub(crate) fn visit_creed_unit_beliefs(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    owner: PlayerId,
    visit: &mut dyn FnMut(&CompiledEffect, u32),
) {
    if !rules.effect_index.module.has_passive_unit_belief {
        return;
    }
    for belief in creed_beliefs(auth, owner) {
        let Some(def) = rules.module.beliefs.get(belief.0 as usize) else {
            continue;
        };
        for effect in &def.effects.passive_unit {
            visit(effect, 1);
        }
    }
}

/// `Condition::NearFollowingCity`: the city whose territory holds `tile`
/// is `owner`'s (Own) or at war with it (Enemy), and its majority follows
/// `owner`'s creed — the religion it founded, or its pantheon in its own
/// city. Own reading, unverified: the city whose territory holds the tile
/// counts, not the city working it; a pantheon belief counts in every own
/// city following the pantheon, an enhancer belief of the founded religion
/// too (the rule checks the belief in the majority's religion).
pub fn near_following_city(
    auth: &AuthWorld,
    owner: PlayerId,
    tile: crate::ids::TileId,
    relation: crate::classic::effects::CityRelation,
) -> bool {
    use crate::classic::effects::CityRelation;
    let Some(city) = auth.map().owning_city(tile) else {
        return false;
    };
    let Some(core) = auth.cities().get(city) else {
        return false;
    };
    let related = match relation {
        CityRelation::Own => core.owner == owner,
        CityRelation::Enemy => auth.diplomacy().at_war(owner, core.owner),
    };
    if !related {
        return false;
    }
    let faith = auth.module().religion().player(owner);
    let majority = crate::derived::religion::majority(auth.module().religion().city(city), core.population);
    match majority {
        Some(Creed::Religion(r)) => faith.religion == Some(r),
        Some(Creed::Pantheon) => relation == CityRelation::Own && faith.pantheon.is_some(),
        Some(Creed::Atheism) | None => false,
    }
}

/// Visit the founder effects (Player scope) of the religion `owner` founded.
pub(crate) fn visit_founder_beliefs(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    owner: PlayerId,
    visit: &mut dyn FnMut(&CompiledEffect, u32),
) {
    let Some(religion) = auth.module().religion().player(owner).religion else {
        return;
    };
    let Some(founded) = auth.module().religion().religion(religion) else {
        return;
    };
    for belief in &founded.beliefs {
        let Some(def) = rules.module.beliefs.get(belief.0 as usize) else {
            continue;
        };
        for effect in &def.effects.passive_player {
            if effect.selector == Selector::Module(ClassicSelector::Founder) {
                visit(&counted(auth, religion, effect), 1);
            }
        }
    }
}

/// The effect with a count-based modifier turned into its flat amount for
/// the religion's current followers (other effects unchanged).
fn counted(auth: &AuthWorld, religion: ReligionId, effect: &CompiledEffect) -> CompiledEffect {
    let modifier = match effect.modifier {
        Modifier::Module(ClassicModifier::AddYieldPerFollowingCity {
            yield_kind,
            amount,
            foreign_only,
        }) => {
            let counts = crate::derived::religion::religion_counts(auth, religion);
            let cities = if foreign_only {
                counts.foreign_cities
            } else {
                counts.cities
            };
            Modifier::AddYield {
                yield_kind,
                amount: amount * cities as i32,
            }
        }
        Modifier::Module(ClassicModifier::AddYieldPerFollowers {
            yield_kind,
            per,
            foreign_only,
        }) => {
            let counts = crate::derived::religion::religion_counts(auth, religion);
            let followers = if foreign_only {
                counts.foreign_followers
            } else {
                counts.followers
            };
            Modifier::AddYield {
                yield_kind,
                amount: (followers / u32::from(per.max(1))) as i32,
            }
        }
        Modifier::Module(ClassicModifier::AddHappinessPerFollowingCity { hundredths }) => {
            let counts = crate::derived::religion::religion_counts(auth, religion);
            Modifier::Module(ClassicModifier::AddHappiness {
                amount: hundredths * counts.cities as i32 / 100,
            })
        }
        other => other,
    };
    CompiledEffect {
        modifier,
        ..*effect
    }
}
