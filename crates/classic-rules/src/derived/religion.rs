//! Faith and religion formulas (classic/docs/classic/rules.md): followers and
//! majority from pressure, the beliefs reaching a city, follower counts of a
//! religion, faith costs, and the status of every religious action. One set
//! of read-only functions for the turn, validation, the HUD and the AI.

use crate::classic::effects::{ClassicCost, ClassicModifier};
use crate::ids::{BeliefId, CityHandle, PlayerId, UnitHandle, UnitKindId};
use crate::rules::content::{BeliefCategory, ReligionId, UnitAbility};
use crate::classic::effects::GreatPersonKind;
use crate::rules::CompiledRuleset;
use crate::world::religion::{CityFaith, Creed};
use crate::world::state::AuthWorld;

/// Pressure of the "no religion" creed per citizen of a city (a pantheon
/// gets twice this per citizen when founded). Unciv:
/// logic/city/managers/CityReligionManager.kt (100 per citizen) and
/// ReligionManager.kt (pantheon 200 per citizen); equal at the 10 times
/// larger scale used here.
pub const PRESSURE_PER_POP: u32 = 1000;
/// Pressure a new religion gets in its founding city (× population). Unciv:
/// logic/civilization/managers/ReligionManager.kt (500 per citizen); equal
/// at the 10 times larger scale used here.
pub const FOUNDING_CITY_PRESSURE: u32 = 5000;
/// Pressure a holy city adds to its own religion each turn, as a multiple of
/// `ADJACENT_CITY_PRESSURE`. Unciv: CityReligionManager.getAffectedBySurroundingCities,
/// equal (5).
pub const HOLY_CITY_PRESSURE: u32 = 5;
/// Pressure a city spreads to a city in range each turn, at Standard speed. Unciv:
/// Speeds.json (Standard, religiousPressureAdjacentCity 6), equal at the 10 times
/// larger scale used here.
pub const ADJACENT_CITY_PRESSURE: u32 = 60;
/// Range in tiles within which a city spreads its religion. Unciv:
/// CityReligionManager.getSpreadRange, equal (10).
pub const ADJACENT_CITY_DISTANCE: i32 = 10;
/// Pressure of a missionary action is its religious strength times this: the
/// scale of this module. Unciv: logic/map/mapunit/actions/
/// UnitActionsReligion.kt (pressure = religious strength); equal.
pub const MISSIONARY_PRESSURE_MULTIPLIER: u32 = 10;
/// Faith for the first pantheon. Unciv: ModConstants.pantheonBase, equal (10).
pub const FIRST_PANTHEON_FAITH: u32 = 10;
/// Faith added to the pantheon price for each pantheon founded. Unciv:
/// ModConstants.pantheonGrowth, equal (5).
pub const NEXT_PANTHEON_FAITH: u32 = 5;
/// Faith for the first Great Prophet. Unciv: ReligionManager.faithForNextGreatProphet,
/// equal (200).
pub const FIRST_PROPHET_FAITH: u32 = 200;
/// The step between two Great Prophet prices grows by this much each time. Unciv:
/// ReligionManager.faithForNextGreatProphet, equal (100).
pub const NEXT_PROPHET_FAITH: u32 = 100;
/// Base chance (percent) of a Great Prophet appearing from faith each turn,
/// plus one per faith above the price. Unciv: logic/civilization/managers/
/// ReligionManager.kt (`5 + stored faith - prophet cost`); equal.
pub const PROPHET_BASE_CHANCE: u32 = 5;
/// Faith for the first great person bought with faith. Unciv: Policies.json
/// (Gods & Kings finishers: 1000, increasing by 500, from the Industrial
/// era); equal.
pub const FIRST_GREAT_PERSON_FAITH: u32 = 1000;
/// The step between two such prices grows by this much each time. Unciv:
/// Policies.json (500); equal.
pub const NEXT_GREAT_PERSON_FAITH: u32 = 500;
/// Order of the Industrial era: prophets stop spawning from faith, great
/// people may be bought with faith.
pub const INDUSTRIAL_ERA: u8 = 4;

/// Followers of each creed in a city of `population` citizens. Creed order; creeds without
/// followers left out.
pub fn followers(faith: &CityFaith, population: u32) -> Vec<(Creed, u32)> {
    if population == 0 {
        return Vec::new();
    }
    let total: u64 = faith.pressure.iter().map(|p| u64::from(p.amount)).sum();
    if total == 0 {
        return vec![(Creed::Atheism, population)];
    }
    let per = (total / u64::from(population)).max(1);
    let mut counts: Vec<(Creed, u32, u64)> = faith
        .pressure
        .iter()
        .map(|p| {
            let amount = u64::from(p.amount);
            let n = (amount / per) as u32;
            (p.creed, n, amount - u64::from(n) * per)
        })
        .collect();
    let mut assigned: u32 = counts.iter().map(|c| c.1).sum();
    while assigned < population {
        let best = counts
            .iter()
            .enumerate()
            .filter(|(_, c)| c.2 > 0)
            .max_by(|a, b| a.1 .2.cmp(&b.1 .2).then(b.0.cmp(&a.0)))
            .map(|(i, _)| i);
        let Some(i) = best else {
            break;
        };
        counts[i].1 += 1;
        counts[i].2 = 0;
        assigned += 1;
    }
    counts
        .into_iter()
        .filter(|c| c.1 > 0)
        .map(|c| (c.0, c.1.min(population)))
        .collect()
}

/// The creed with a majority: the most
/// followers (ties: more pressure, then creed order), and at least half of
/// the citizens rounded down (3 of 7). Atheism is never a majority. Unciv:
/// logic/city/managers/CityReligionManager.kt (`getMajorityReligionName`:
/// followers >= population / 2 in integers); equal.
pub fn majority(faith: &CityFaith, population: u32) -> Option<Creed> {
    let counts = followers(faith, population);
    let total: u32 = counts.iter().map(|c| c.1).sum();
    let mut best: Option<(Creed, u32, u32)> = None;
    for (creed, n) in counts {
        let pressure = faith.pressure_of(creed);
        let better = match best {
            None => true,
            Some((_, bn, bp)) => n > bn || (n == bn && pressure > bp),
        };
        if better {
            best = Some((creed, n, pressure));
        }
    }
    let (creed, n, _) = best?;
    if creed == Creed::Atheism || n < total / 2 {
        return None;
    }
    Some(creed)
}

/// The majority creed of a live city.
pub fn city_majority(auth: &AuthWorld, city: CityHandle) -> Option<Creed> {
    let core = auth.cities().get(city)?;
    majority(auth.module().religion().city(city), core.population)
}

/// Followers of `creed` in a live city.
pub fn city_followers(auth: &AuthWorld, city: CityHandle, creed: Creed) -> u32 {
    let Some(core) = auth.cities().get(city) else {
        return 0;
    };
    let counts = followers(auth.module().religion().city(city), core.population);
    counts
        .iter()
        .find(|c| c.0 == creed)
        .map(|c| c.1)
        .unwrap_or(0)
}

/// The beliefs reaching a city through its majority: the owner's pantheon
/// for a pantheon majority, every belief of the religion for a religion.
pub fn city_beliefs(auth: &AuthWorld, city: CityHandle, owner: PlayerId) -> Vec<BeliefId> {
    match city_majority(auth, city) {
        Some(Creed::Pantheon) => auth.module().religion().player(owner).pantheon.into_iter().collect(),
        Some(Creed::Religion(r)) => match auth.module().religion().religion(r) {
            Some(founded) => founded.beliefs.clone(),
            None => Vec::new(),
        },
        Some(Creed::Atheism) | None => Vec::new(),
    }
}

/// Cities and citizens following a religion, and those outside its
/// founder's empire.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FollowerCounts {
    pub cities: u32,
    pub followers: u32,
    pub foreign_cities: u32,
    pub foreign_followers: u32,
}

/// Followers of `religion`, answered from the world's memo while the world
/// does not change (`kernel::effects::memo`).
pub fn religion_counts(auth: &AuthWorld, religion: ReligionId) -> FollowerCounts {
    #[derive(Clone, Copy, PartialEq, Eq, Hash)]
    struct Key(ReligionId);
    auth.memo().module_value(Key(religion), || compute_religion_counts(auth, religion))
}

/// `religion_counts` computed from the world, never from the memo.
pub fn compute_religion_counts(auth: &AuthWorld, religion: ReligionId) -> FollowerCounts {
    let mut out = FollowerCounts::default();
    let Some(founded) = auth.module().religion().religion(religion) else {
        return out;
    };
    let creed = Creed::Religion(religion);
    for (handle, core) in auth.cities().iter() {
        let faith = auth.module().religion().city(handle);
        let counts = followers(faith, core.population);
        let n = counts
            .iter()
            .find(|c| c.0 == creed)
            .map(|c| c.1)
            .unwrap_or(0);
        let foreign = core.owner != founded.founder;
        out.followers += n;
        if foreign {
            out.foreign_followers += n;
        }
        if majority(faith, core.population) == Some(creed) {
            out.cities += 1;
            if foreign {
                out.foreign_cities += 1;
            }
        }
    }
    out
}

/// Faith for the next pantheon of the game (rules.md, Standard).
pub fn pantheon_cost(auth: &AuthWorld) -> u32 {
    FIRST_PANTHEON_FAITH + NEXT_PANTHEON_FAITH * u32::from(auth.module().religion().pantheons())
}

/// Faith price of the n-th Great Prophet: 200, 300, 500, 800, ...
pub fn prophet_number(n: u32) -> u32 {
    let mut cost = FIRST_PROPHET_FAITH;
    for k in 2..=n.max(1) {
        cost += NEXT_PROPHET_FAITH * (k - 1);
    }
    cost
}

/// Faith price of the n-th great person bought with faith: 1000, 1500, 2500, 4000, ...
pub fn great_person_number(n: u32) -> u32 {
    let mut cost = FIRST_GREAT_PERSON_FAITH;
    for k in 2..=n.max(1) {
        cost += NEXT_GREAT_PERSON_FAITH * (k - 1);
    }
    cost
}

/// Faith for the player's next Great Prophet, with
/// the prophet cost percent of its religion (Messiah −25 %).
pub fn prophet_cost(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId) -> u32 {
    let n = u32::from(auth.module().religion().player(player).prophets) + 1;
    let base = prophet_number(n) as i64;
    let percent = match auth.module().religion().player(player).religion {
        Some(r) => crate::classic::religion_effects::religion_modifiers(auth, rules, r).prophet_cost,
        None => 0,
    };
    (base * (100 + i64::from(percent)) / 100).max(1) as u32
}

/// Religions that can still be founded.
pub fn religions_left(auth: &AuthWorld) -> u32 {
    let max = u32::from(crate::classic::world_size::max_religions(crate::classic::world_size::major_count(auth)));
    max.saturating_sub(auth.module().religion().religions().len() as u32)
}

/// The great person kind a unit is (None for other units).
pub fn great_person_kind(rules: &CompiledRuleset, kind: UnitKindId) -> Option<GreatPersonKind> {
    let of = |unit: UnitKindId| rules.module.great_people.iter().find(|g| g.unit == unit).map(|g| g.kind);
    if let Some(found) = of(kind) {
        return Some(found);
    }
    // A unique great person (the `People` special unit: the Khan, the Merchant of
    // Venice) is the great person it replaces.
    let def = rules.units.get(kind.0 as usize)?;
    let people = def.module.abilities.contains(&UnitAbility::Special(crate::rules::content::SpecialUnit::People));
    if !people {
        return None;
    }
    of(def.replaces?)
}

/// A unit kind has the ability.
pub fn has_ability(rules: &CompiledRuleset, kind: UnitKindId, wanted: UnitAbility) -> bool {
    rules
        .units
        .get(kind.0 as usize)
        .map(|u| u.module.abilities.contains(&wanted))
        .unwrap_or(false)
}

/// The numeric value of a unit ability (spread charges, religious strength).
pub fn ability_value(
    rules: &CompiledRuleset,
    kind: UnitKindId,
    wanted: fn(UnitAbility) -> Option<u16>,
) -> Option<u16> {
    let def = rules.units.get(kind.0 as usize)?;
    def.module.abilities.iter().find_map(|a| wanted(*a))
}

/// Beliefs of a category not taken by anyone yet, table order.
pub fn free_beliefs(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    category: BeliefCategory,
) -> Vec<BeliefId> {
    (0..rules.module.beliefs.len())
        .map(|i| BeliefId(i as u16))
        .filter(|b| rules.module.beliefs[b.0 as usize].category == Some(category))
        .filter(|b| !auth.module().religion().belief_taken(*b))
        .collect()
}

/// Religions of the table nobody founded yet.
pub fn free_religions(auth: &AuthWorld, rules: &CompiledRuleset) -> Vec<ReligionId> {
    (0..rules.module.religions.len())
        .map(|i| ReligionId(i as u16))
        .filter(|r| auth.module().religion().religion(*r).is_none())
        .collect()
}

/// Why a religious action is refused (`reason()` is the player-facing text).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaithBlock {
    UnknownPlayer,
    UnknownUnit,
    NotYourUnit,
    NotAProphet,
    NotAMissionary,
    NotAnInquisitor,
    AlreadyHasPantheon,
    AlreadyHasReligion,
    NoReligion,
    AlreadyEnhanced,
    NotEnoughFaith,
    NoPantheonsLeft,
    NoReligionsLeft,
    ReligionTaken,
    BeliefTaken,
    WrongBeliefCategory,
    NeedsPantheonBelief,
    NotInOwnCity,
    NotNearCity,
    NoSpreadsLeft,
    AlreadyFollows,
    NoReformation,
    AlreadyReformed,
    NothingToRemove,
    NotAGreatPerson,
    NotOwnLand,
    WrongTerrain,
    NoBonusBelief,
}

impl FaithBlock {
    pub const ALL: [FaithBlock; 28] = [
        FaithBlock::UnknownPlayer,
        FaithBlock::UnknownUnit,
        FaithBlock::NotYourUnit,
        FaithBlock::NotAProphet,
        FaithBlock::NotAMissionary,
        FaithBlock::NotAnInquisitor,
        FaithBlock::AlreadyHasPantheon,
        FaithBlock::AlreadyHasReligion,
        FaithBlock::NoReligion,
        FaithBlock::AlreadyEnhanced,
        FaithBlock::NotEnoughFaith,
        FaithBlock::NoPantheonsLeft,
        FaithBlock::NoReligionsLeft,
        FaithBlock::ReligionTaken,
        FaithBlock::BeliefTaken,
        FaithBlock::WrongBeliefCategory,
        FaithBlock::NeedsPantheonBelief,
        FaithBlock::NotInOwnCity,
        FaithBlock::NotNearCity,
        FaithBlock::NoSpreadsLeft,
        FaithBlock::AlreadyFollows,
        FaithBlock::NoReformation,
        FaithBlock::AlreadyReformed,
        FaithBlock::NothingToRemove,
        FaithBlock::NotAGreatPerson,
        FaithBlock::NotOwnLand,
        FaithBlock::WrongTerrain,
        FaithBlock::NoBonusBelief,
    ];

    pub fn reason(self) -> &'static str {
        match self {
            FaithBlock::UnknownPlayer => "unknown player",
            FaithBlock::UnknownUnit => "unknown unit",
            FaithBlock::NotYourUnit => "not your unit",
            FaithBlock::NotAProphet => "only a Great Prophet can",
            FaithBlock::NotAMissionary => "this unit cannot spread a religion",
            FaithBlock::NotAnInquisitor => "this unit cannot remove heresy",
            FaithBlock::AlreadyHasPantheon => "pantheon already founded",
            FaithBlock::AlreadyHasReligion => "religion already founded",
            FaithBlock::NoReligion => "no religion founded",
            FaithBlock::AlreadyEnhanced => "religion already enhanced",
            FaithBlock::NotEnoughFaith => "not enough faith",
            FaithBlock::NoPantheonsLeft => "no pantheons left",
            FaithBlock::NoReligionsLeft => "no religions left to found",
            FaithBlock::ReligionTaken => "religion already founded by another",
            FaithBlock::BeliefTaken => "belief already taken",
            FaithBlock::WrongBeliefCategory => "wrong kind of belief",
            FaithBlock::NeedsPantheonBelief => "choose a pantheon belief too",
            FaithBlock::NotInOwnCity => "must stand in one of your cities",
            FaithBlock::NotNearCity => "must stand in or next to a city",
            FaithBlock::NoSpreadsLeft => "no spreads left",
            FaithBlock::AlreadyFollows => "the city already follows this religion",
            FaithBlock::NoReformation => "needs a Reformation policy",
            FaithBlock::AlreadyReformed => "religion already reformed",
            FaithBlock::NothingToRemove => "no other religion here",
            FaithBlock::NotAGreatPerson => "not a great person with an improvement",
            FaithBlock::NotOwnLand => "must stand on your land outside a city",
            FaithBlock::WrongTerrain => "the improvement cannot be built on this terrain",
            FaithBlock::NoBonusBelief => "no bonus belief when founding",
        }
    }
}

/// The city in which or next to which a unit stands (own tile first,
/// then the neighbours in direction order).
pub fn city_at_or_next_to(auth: &AuthWorld, unit: UnitHandle) -> Option<CityHandle> {
    cities_at_or_next_to(auth, unit).first().copied()
}

/// Every city in which or next to which a unit stands, same order.
pub fn cities_at_or_next_to(auth: &AuthWorld, unit: UnitHandle) -> Vec<CityHandle> {
    let Some(core) = auth.units().get(unit).map(|v| *v.core) else {
        return Vec::new();
    };
    let mut out: Vec<CityHandle> = auth.indexes().city_on_tile(core.tile).into_iter().collect();
    let dims = auth.map().dimensions;
    for t in dims.neighbors(core.tile) {
        if let Some(city) = auth.indexes().city_on_tile(t) {
            out.push(city);
        }
    }
    out
}

/// Faith cost of the next pantheon if `player` may found one with `belief`.
pub fn pantheon_status(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    player: PlayerId,
    belief: BeliefId,
) -> Result<u32, FaithBlock> {
    if !auth.players().is_alive(player) {
        return Err(FaithBlock::UnknownPlayer);
    }
    let faith = auth.module().religion().player(player);
    if faith.pantheon.is_some() {
        return Err(FaithBlock::AlreadyHasPantheon);
    }
    if faith.religion.is_some() {
        return Err(FaithBlock::AlreadyHasReligion);
    }
    check_belief(auth, rules, belief, BeliefCategory::Pantheon)?;
    let enhanced = auth
        .module()
        .religion()
        .religions()
        .iter()
        .filter(|r| r.enhanced)
        .count();
    let max = u32::from(crate::classic::world_size::max_religions(crate::classic::world_size::major_count(auth)));
    if enhanced > 0 && u32::from(auth.module().religion().pantheons()) >= max {
        return Err(FaithBlock::NoPantheonsLeft);
    }
    let cost = pantheon_cost(auth);
    if faith.faith < cost {
        return Err(FaithBlock::NotEnoughFaith);
    }
    Ok(cost)
}

fn check_belief(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    belief: BeliefId,
    category: BeliefCategory,
) -> Result<(), FaithBlock> {
    let def = rules
        .module.beliefs
        .get(belief.0 as usize)
        .ok_or(FaithBlock::WrongBeliefCategory)?;
    if def.category != Some(category) {
        return Err(FaithBlock::WrongBeliefCategory);
    }
    if auth.module().religion().belief_taken(belief) {
        return Err(FaithBlock::BeliefTaken);
    }
    Ok(())
}

/// A Great Prophet of `player` (or the reason it is not one).
fn own_prophet(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    unit: UnitHandle,
) -> Result<(PlayerId, crate::ids::TileId), FaithBlock> {
    let view = auth.units().get(unit).ok_or(FaithBlock::UnknownUnit)?;
    if !has_ability(rules, view.core.kind, UnitAbility::FoundReligion) {
        return Err(FaithBlock::NotAProphet);
    }
    Ok((view.core.owner, view.core.tile))
}

/// What founding a religion needs: a Great Prophet in one of its owner's
/// cities (not a holy city yet), a religion left to found, a free religion
/// of the table, a free founder and follower belief, and a pantheon belief
/// when the player has no pantheon (rules.md).
pub struct FoundingChoice {
    pub religion: ReligionId,
    pub founder: BeliefId,
    pub follower: BeliefId,
    /// Required exactly when the player has no pantheon.
    pub pantheon: Option<BeliefId>,
    /// One more pantheon, founder, follower or enhancer belief, allowed
    /// when the player has the extra founding belief (Byzantium); may be left
    /// out.
    pub bonus: Option<BeliefId>,
}

/// The holy city the religion would get.
pub fn found_status(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    unit: UnitHandle,
    choice: &FoundingChoice,
) -> Result<CityHandle, FaithBlock> {
    let (player, tile) = own_prophet(auth, rules, unit)?;
    let city = auth
        .indexes()
        .city_on_tile(tile)
        .ok_or(FaithBlock::NotInOwnCity)?;
    let owner = auth.cities().get(city).map(|c| c.owner);
    if owner != Some(player) || auth.module().religion().holy_city_of(city).is_some() {
        return Err(FaithBlock::NotInOwnCity);
    }
    let faith = auth.module().religion().player(player);
    if faith.religion.is_some() {
        return Err(FaithBlock::AlreadyHasReligion);
    }
    if religions_left(auth) == 0 {
        return Err(FaithBlock::NoReligionsLeft);
    }
    if usize::from(choice.religion.0) >= rules.module.religions.len()
        || auth.module().religion().religion(choice.religion).is_some()
    {
        return Err(FaithBlock::ReligionTaken);
    }
    check_belief(auth, rules, choice.founder, BeliefCategory::Founder)?;
    check_belief(auth, rules, choice.follower, BeliefCategory::Follower)?;
    match (faith.pantheon, choice.pantheon) {
        (None, None) => return Err(FaithBlock::NeedsPantheonBelief),
        (None, Some(p)) => check_belief(auth, rules, p, BeliefCategory::Pantheon)?,
        (Some(_), Some(_)) => return Err(FaithBlock::AlreadyHasPantheon),
        (Some(_), None) => {}
    }
    if let Some(bonus) = choice.bonus {
        check_bonus_belief(auth, rules, player, choice, bonus)?;
    }
    Ok(city)
}

/// Categories a founding bonus belief may come from.
pub const BONUS_BELIEF_CATEGORIES: [BeliefCategory; 4] = [
    BeliefCategory::Pantheon,
    BeliefCategory::Founder,
    BeliefCategory::Follower,
    BeliefCategory::Enhancer,
];

/// A bonus belief: the player may add one, it is free, of a bonus
/// category and not one of the other chosen beliefs.
fn check_bonus_belief(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    player: PlayerId,
    choice: &FoundingChoice,
    bonus: BeliefId,
) -> Result<(), FaithBlock> {
    let allowed = crate::effects::player::player_modifiers(auth, rules, player).module.extra_founding_beliefs;
    if allowed == 0 {
        return Err(FaithBlock::NoBonusBelief);
    }
    let category = rules
        .module.beliefs
        .get(bonus.0 as usize)
        .and_then(|b| b.category)
        .ok_or(FaithBlock::WrongBeliefCategory)?;
    if !BONUS_BELIEF_CATEGORIES.contains(&category) {
        return Err(FaithBlock::WrongBeliefCategory);
    }
    let chosen = [Some(choice.founder), Some(choice.follower), choice.pantheon];
    if chosen.contains(&Some(bonus)) || auth.module().religion().belief_taken(bonus) {
        return Err(FaithBlock::BeliefTaken);
    }
    Ok(())
}

/// Enhancing needs a Great Prophet of the founder in its own land, a
/// religion not enhanced yet, a free follower and a free enhancer belief.
pub fn enhance_status(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    unit: UnitHandle,
    follower: BeliefId,
    enhancer: BeliefId,
) -> Result<ReligionId, FaithBlock> {
    let (player, tile) = own_prophet(auth, rules, unit)?;
    if auth.map().owner(tile).flatten() != Some(player) {
        return Err(FaithBlock::NotInOwnCity);
    }
    let religion = auth
        .module()
        .religion()
        .player(player)
        .religion
        .ok_or(FaithBlock::NoReligion)?;
    let founded = auth
        .module()
        .religion()
        .religion(religion)
        .ok_or(FaithBlock::NoReligion)?;
    if founded.enhanced {
        return Err(FaithBlock::AlreadyEnhanced);
    }
    check_belief(auth, rules, follower, BeliefCategory::Follower)?;
    check_belief(auth, rules, enhancer, BeliefCategory::Enhancer)?;
    Ok(religion)
}

/// A Reformation belief: the founder adopted a policy that allows it and
/// its religion has none yet.
pub fn reformation_status(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    player: PlayerId,
    belief: BeliefId,
) -> Result<ReligionId, FaithBlock> {
    if !auth.players().is_alive(player) {
        return Err(FaithBlock::UnknownPlayer);
    }
    let religion = auth
        .module()
        .religion()
        .player(player)
        .religion
        .ok_or(FaithBlock::NoReligion)?;
    let founded = auth
        .module()
        .religion()
        .religion(religion)
        .ok_or(FaithBlock::NoReligion)?;
    let reformed = founded
        .beliefs
        .iter()
        .any(|b| rules.module.beliefs[b.0 as usize].category == Some(BeliefCategory::Reformation));
    if reformed {
        return Err(FaithBlock::AlreadyReformed);
    }
    if !crate::effects::player::player_modifiers(auth, rules, player).module.reformation {
        return Err(FaithBlock::NoReformation);
    }
    check_belief(auth, rules, belief, BeliefCategory::Reformation)?;
    Ok(religion)
}

/// A spread: the city in or next to which the unit stands, the religion it
/// carries, the pressure it adds, and whether it is a prophet (prophets
/// remove the other religions first).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spread {
    pub city: CityHandle,
    pub religion: ReligionId,
    pub pressure: u32,
    pub prophet: bool,
}

pub fn spread_status(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    unit: UnitHandle,
) -> Result<Spread, FaithBlock> {
    let view = auth.units().get(unit).ok_or(FaithBlock::UnknownUnit)?;
    let kind = view.core.kind;
    if !has_ability(rules, kind, UnitAbility::SpreadReligion) {
        return Err(FaithBlock::NotAMissionary);
    }
    let religion = unit_religion(auth, unit).ok_or(FaithBlock::NoReligion)?;
    if spreads_left(auth, rules, unit) == 0 {
        return Err(FaithBlock::NoSpreadsLeft);
    }
    let prophet = has_ability(rules, kind, UnitAbility::FoundReligion);
    let candidates = cities_at_or_next_to(auth, unit);
    let first = *candidates.first().ok_or(FaithBlock::NotNearCity)?;
    let not_following = candidates
        .iter()
        .copied()
        .find(|c| city_majority(auth, *c) != Some(Creed::Religion(religion)));
    let city = match (not_following, prophet) {
        (Some(city), _) => city,
        (None, true) => first,
        (None, false) => return Err(FaithBlock::AlreadyFollows),
    };
    let strength = ability_value(rules, kind, |a| match a {
        UnitAbility::ReligiousStrength(n) => Some(n),
        _ => None,
    })
    .unwrap_or(1000);
    let stat = if prophet {
        crate::classic::effects::ReligionStat::ProphetStrength
    } else {
        crate::classic::effects::ReligionStat::MissionaryStrength
    };
    let percent = crate::classic::religion_effects::religion_modifiers(auth, rules, religion).of(stat);
    let factor = (100 + i64::from(percent.max(-100))) as u64;
    let kept = 100u64.saturating_sub(u64::from(unit_strength_lost(auth, unit)));
    let pressure = u64::from(MISSIONARY_PRESSURE_MULTIPLIER) * u64::from(strength) * factor / 100 * kept / 100;
    Ok(Spread {
        city,
        religion,
        pressure: pressure.min(u64::from(u32::MAX)) as u32,
        prophet,
    })
}

/// Percent of its original religious strength the unit lost in rival land.
pub fn unit_strength_lost(auth: &AuthWorld, unit: UnitHandle) -> u8 {
    let Some(core) = auth.units().get(unit).map(|v| *v.core) else {
        return 0;
    };
    auth.module().religion().unit(core.id).map_or(0, |u| u.strength_lost)
}

/// Spreads a religious unit has left: its record, or the full count of its
/// kind when it never spread (units created by grants or the harness).
pub fn spreads_left(auth: &AuthWorld, rules: &CompiledRuleset, unit: UnitHandle) -> u8 {
    let Some(core) = auth.units().get(unit).map(|v| *v.core) else {
        return 0;
    };
    if let Some(carried) = auth.module().religion().unit(core.id) {
        return carried.spreads_left;
    }
    ability_value(rules, core.kind, |a| match a {
        UnitAbility::ReligionSpreads(n) => Some(u16::from(n)),
        _ => None,
    })
    .unwrap_or(1) as u8
}

/// The religion a religious unit carries (missionaries: from their city of
/// purchase; prophets: their owner's religion).
pub fn unit_religion(auth: &AuthWorld, unit: UnitHandle) -> Option<ReligionId> {
    let core = auth.units().get(unit)?.core;
    if let Some(carried) = auth.module().religion().unit(core.id) {
        return Some(carried.religion);
    }
    auth.module().religion().player(core.owner).religion
}

/// Removing heresy: an inquisitor in or next to one of its owner's cities
/// that has another religion than the inquisitor's.
pub fn heresy_status(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    unit: UnitHandle,
) -> Result<(CityHandle, ReligionId), FaithBlock> {
    let view = auth.units().get(unit).ok_or(FaithBlock::UnknownUnit)?;
    if !has_ability(rules, view.core.kind, UnitAbility::RemoveHeresy) {
        return Err(FaithBlock::NotAnInquisitor);
    }
    let religion = unit_religion(auth, unit).ok_or(FaithBlock::NoReligion)?;
    let city = city_at_or_next_to(auth, unit).ok_or(FaithBlock::NotNearCity)?;
    if auth.cities().get(city).map(|c| c.owner) != Some(view.core.owner) {
        return Err(FaithBlock::NotInOwnCity);
    }
    let other = auth
        .module()
        .religion()
        .city(city)
        .pressure
        .iter()
        .any(|p| match p.creed {
            Creed::Religion(r) => r != religion,
            Creed::Atheism | Creed::Pantheon => false,
        });
    if !other {
        return Err(FaithBlock::NothingToRemove);
    }
    Ok((city, religion))
}

/// `x / 10 * 10` of the faith price formulas.
fn round_ten(cost: i64) -> u32 {
    (cost.max(0) / 10 * 10).min(i64::from(u32::MAX)) as u32
}

fn era_multiplier(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId) -> i64 {
    let era = crate::world::player::player_era(auth, rules, player);
    rules
        .eras
        .iter()
        .find(|e| e.index == era)
        .map(|e| i64::from(e.module.faith_cost_multiplier))
        .filter(|m| *m > 0)
        .unwrap_or(100)
}

/// The faith price of `item` in `city` for `player`, or why it cannot be
/// bought (rules.md).
pub fn faith_purchase_status(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    player: PlayerId,
    city: CityHandle,
    item: crate::ids::ProductionItem,
) -> Result<u32, FaithPurchaseBlock> {
    let core = auth
        .cities()
        .get(city)
        .ok_or(FaithPurchaseBlock::UnknownCity)?;
    if core.owner != player {
        return Err(FaithPurchaseBlock::NotYourCity);
    }
    let cost = match item {
        crate::ids::ProductionItem::Unit(kind) => unit_faith_cost(auth, rules, player, city, kind)?,
        crate::ids::ProductionItem::Building(b) => {
            building_faith_cost(auth, rules, player, city, b)?
        }
        crate::ids::ProductionItem::Project(_) => return Err(FaithPurchaseBlock::NoFaithPrice),
    };
    if auth.module().religion().player(player).faith < cost {
        return Err(FaithPurchaseBlock::NotEnoughFaith);
    }
    Ok(cost)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaithPurchaseBlock {
    UnknownCity,
    NotYourCity,
    UnknownItem,
    NoFaithPrice,
    TechnologyMissing,
    NeedsReligion,
    NeedsEnhancedReligion,
    NeedsBelief,
    NeedsBranch,
    TooEarly,
    AlreadyBuilt,
    MissingBuilding,
    OtherCivilization,
    NotEnoughFaith,
}

impl FaithPurchaseBlock {
    pub const ALL: [FaithPurchaseBlock; 14] = [
        FaithPurchaseBlock::UnknownCity,
        FaithPurchaseBlock::NotYourCity,
        FaithPurchaseBlock::UnknownItem,
        FaithPurchaseBlock::NoFaithPrice,
        FaithPurchaseBlock::TechnologyMissing,
        FaithPurchaseBlock::NeedsReligion,
        FaithPurchaseBlock::NeedsEnhancedReligion,
        FaithPurchaseBlock::NeedsBelief,
        FaithPurchaseBlock::NeedsBranch,
        FaithPurchaseBlock::TooEarly,
        FaithPurchaseBlock::AlreadyBuilt,
        FaithPurchaseBlock::MissingBuilding,
        FaithPurchaseBlock::OtherCivilization,
        FaithPurchaseBlock::NotEnoughFaith,
    ];

    pub fn reason(self) -> &'static str {
        match self {
            FaithPurchaseBlock::UnknownCity => "unknown city",
            FaithPurchaseBlock::NotYourCity => "not your city",
            FaithPurchaseBlock::UnknownItem => "unknown item",
            FaithPurchaseBlock::NoFaithPrice => "cannot be bought with faith",
            FaithPurchaseBlock::TechnologyMissing => "technology missing",
            FaithPurchaseBlock::NeedsReligion => "the city must follow a religion",
            FaithPurchaseBlock::NeedsEnhancedReligion => "needs an enhanced religion",
            FaithPurchaseBlock::NeedsBelief => "needs a belief of the city's religion",
            FaithPurchaseBlock::NeedsBranch => "needs the matching policy branch completed",
            FaithPurchaseBlock::TooEarly => "great people from the Industrial era",
            FaithPurchaseBlock::AlreadyBuilt => "already built",
            FaithPurchaseBlock::MissingBuilding => "a required building is missing",
            FaithPurchaseBlock::OtherCivilization => {
                "another nation's building, or one a unique building replaces"
            }
            FaithPurchaseBlock::NotEnoughFaith => "not enough faith",
        }
    }
}

fn unit_faith_cost(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    player: PlayerId,
    city: CityHandle,
    kind: UnitKindId,
) -> Result<u32, FaithPurchaseBlock> {
    let def = rules
        .units
        .get(kind.0 as usize)
        .ok_or(FaithPurchaseBlock::UnknownItem)?;
    if let Some(gp) = great_person_kind(rules, kind) {
        return great_person_faith_cost(auth, rules, player, gp);
    }
    let base = i64::from(def.module.faith_cost.ok_or(FaithPurchaseBlock::NoFaithPrice)?);
    if let Some(tech) = def.required_tech {
        if !auth.players().techs_of(player).contains(&tech) {
            return Err(FaithPurchaseBlock::TechnologyMissing);
        }
    }
    let core = auth.cities().get(city).ok_or(FaithPurchaseBlock::UnknownCity)?;
    if crate::world::building_rules::first_missing(rules, core, &def.requires_buildings).is_some() {
        return Err(FaithPurchaseBlock::MissingBuilding);
    }
    let religious = def.module.abilities.contains(&UnitAbility::SpreadReligion)
        || def.module.abilities.contains(&UnitAbility::RemoveHeresy);
    let majority = city_majority(auth, city);
    let mut cost = base * era_multiplier(auth, rules, player) / 100;
    if religious {
        let Some(Creed::Religion(religion)) = majority else {
            return Err(FaithPurchaseBlock::NeedsReligion);
        };
        let founded = auth
            .module()
            .religion()
            .religion(religion)
            .ok_or(FaithPurchaseBlock::NeedsReligion)?;
        if def
            .module.abilities
            .contains(&UnitAbility::RequiresEnhancedReligion)
            && !founded.enhanced
        {
            return Err(FaithPurchaseBlock::NeedsEnhancedReligion);
        }
        cost = cost * (100 + i64::from(faith_cost_percent(auth, rules, player))) / 100;
        let mods = crate::classic::religion_effects::religion_modifiers(auth, rules, religion);
        cost = cost
            * (100 + i64::from(mods.of(crate::classic::effects::ReligionStat::MissionaryCost)))
            / 100;
        return Ok(round_ten(cost));
    }
    // Units without `FaithPurchaseNeedsBelief` are bought at their
    // price; the others (military units) need a belief of the city that
    // unlocks their era (Holy Warriors, Religious Fervor).
    if !def.module.abilities.contains(&UnitAbility::FaithPurchaseNeedsBelief) {
        return Ok(round_ten(cost));
    }
    let era = def.era.ok_or(FaithPurchaseBlock::NeedsBelief)?;
    let unlocked = city_beliefs(auth, city, player).iter().any(|b| {
        rules.module.beliefs[b.0 as usize]
            .effects
            .passive_city
            .iter()
            .any(|e| e.modifier == crate::rules::effects::Modifier::Module(ClassicModifier::UnlockFaithUnits(era)))
    });
    if !unlocked {
        return Err(FaithPurchaseBlock::NeedsBelief);
    }
    Ok(round_ten(cost))
}

/// Cost percent modifier of faith purchases (Mandate of Heaven).
fn faith_cost_percent(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId) -> i32 {
    crate::effects::player::player_modifiers(auth, rules, player).cost_percent
        [crate::rules::effects::CostTarget::Module(ClassicCost::FaithPurchase).index()]
}

fn great_person_faith_cost(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    player: PlayerId,
    kind: GreatPersonKind,
) -> Result<u32, FaithPurchaseBlock> {
    let faith = auth.module().religion().player(player);
    if kind == GreatPersonKind::Prophet {
        if faith.religion.is_none() {
            return Err(FaithPurchaseBlock::NeedsReligion);
        }
        return Ok(round_ten(i64::from(prophet_cost(auth, rules, player))));
    }
    if crate::world::player::player_era(auth, rules, player) < INDUSTRIAL_ERA {
        return Err(FaithPurchaseBlock::TooEarly);
    }
    let n = u32::from(faith.bought_of(kind)) + 1;
    let price = round_ten(i64::from(great_person_number(n)));
    let any_kind = crate::classic::effects::ReligionAbility::FaithPurchaseAllGreatPeople;
    if founded_religion_has(auth, rules, player, any_kind) {
        return Ok(price);
    }
    // Completing the branch the rules name for the kind (rules.md,
    // `faith_great_people` role) lets a player buy it from the Industrial era.
    let branch = rules.roles.module.faith_branch(kind).ok_or(FaithPurchaseBlock::NeedsBranch)?;
    if !crate::derived::culture::branch_complete(rules, auth.module().culture().of(player), branch) {
        return Err(FaithPurchaseBlock::NeedsBranch);
    }
    Ok(price)
}

/// The religion `player` founded has the ability (Reformation beliefs).
pub fn founded_religion_has(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    player: PlayerId,
    ability: crate::classic::effects::ReligionAbility,
) -> bool {
    match auth.module().religion().player(player).religion {
        Some(religion) => {
            crate::classic::religion_effects::religion_modifiers(auth, rules, religion).has(ability)
        }
        None => false,
    }
}

fn building_faith_cost(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    player: PlayerId,
    city: CityHandle,
    building: crate::ids::BuildingKindId,
) -> Result<u32, FaithPurchaseBlock> {
    let def = rules
        .buildings
        .get(building.0 as usize)
        .ok_or(FaithPurchaseBlock::UnknownItem)?;
    let base = i64::from(def.module.faith_cost.ok_or(FaithPurchaseBlock::NoFaithPrice)?);
    let core = auth
        .cities()
        .get(city)
        .ok_or(FaithPurchaseBlock::UnknownCity)?;
    if core.buildings.contains(&building) {
        return Err(FaithPurchaseBlock::AlreadyBuilt);
    }
    if let Some(tech) = def.required_tech {
        if !auth.players().techs_of(player).contains(&tech) {
            return Err(FaithPurchaseBlock::TechnologyMissing);
        }
    }
    if crate::world::building_rules::first_missing(rules, core, &def.requires_buildings).is_some() {
        return Err(FaithPurchaseBlock::MissingBuilding);
    }
    if crate::world::building_rules::civilization_rule(auth, rules, core, building, def).is_err() {
        return Err(FaithPurchaseBlock::OtherCivilization);
    }
    if def.module.faith_purchase_needs_belief && !belief_unlocks_building(auth, rules, city, player, building) {
        return Err(FaithPurchaseBlock::NeedsBelief);
    }
    let cost = base * era_multiplier(auth, rules, player) / 100;
    let cost = cost * (100 + i64::from(faith_cost_percent(auth, rules, player))) / 100;
    Ok(round_ten(cost))
}

/// A belief of the city's majority unlocks `building` for faith: it names
/// the building or the regular building it replaces (a unique building
/// takes its class's unlock: Siam's Wat with Jesuit Education).
fn belief_unlocks_building(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    city: CityHandle,
    player: PlayerId,
    building: crate::ids::BuildingKindId,
) -> bool {
    let replaces = rules.buildings[building.0 as usize].replaces;
    let unlocks = |modifier: crate::rules::effects::Modifier| match modifier {
        crate::rules::effects::Modifier::Module(ClassicModifier::UnlockFaithBuilding(b)) => b == building || Some(b) == replaces,
        _ => false,
    };
    city_beliefs(auth, city, player).iter().any(|b| {
        let effects = &rules.module.beliefs[b.0 as usize].effects.passive_city;
        effects.iter().any(|e| unlocks(e.modifier))
    })
}

/// What a player knows of faith and religions (HUD, religion screen, AI):
/// its own pool and choices, and the public facts of every founded religion.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FaithOverview {
    pub faith: u32,
    pub per_turn: u32,
    pub pantheon: Option<BeliefId>,
    pub religion: Option<ReligionId>,
    /// Faith of the next pantheon of the game.
    pub pantheon_cost: u32,
    /// Faith of the player's next Great Prophet.
    pub prophet_cost: u32,
    pub religions_left: u32,
    pub can_found_pantheon: bool,
    pub can_add_reformation: bool,
    /// Bonus beliefs the player may add when it founds its religion
    /// (Byzantium: 1).
    pub extra_founding_beliefs: u8,
    pub free_pantheon: Vec<BeliefId>,
    pub free_founder: Vec<BeliefId>,
    pub free_follower: Vec<BeliefId>,
    pub free_enhancer: Vec<BeliefId>,
    pub free_reformation: Vec<BeliefId>,
    pub free_religions: Vec<ReligionId>,
    pub religions: Vec<ReligionSummary>,
    /// The first own city where a missionary can be bought now, and its
    /// price (None: no city may, or the pool is short).
    pub missionary_purchase: Option<(CityHandle, u32)>,
}

/// Public facts of a founded religion (the religion overview).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReligionSummary {
    pub religion: ReligionId,
    pub founder: PlayerId,
    pub holy_city: CityHandle,
    pub beliefs: Vec<BeliefId>,
    pub enhanced: bool,
    pub counts: FollowerCounts,
}

pub fn faith_overview(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    player: PlayerId,
    per_turn: u32,
) -> FaithOverview {
    let faith = auth.module().religion().player(player);
    let free_pantheon = free_beliefs(auth, rules, BeliefCategory::Pantheon);
    let can_found_pantheon = free_pantheon
        .first()
        .is_some_and(|b| pantheon_status(auth, rules, player, *b).is_ok());
    let free_reformation = free_beliefs(auth, rules, BeliefCategory::Reformation);
    let can_add_reformation = free_reformation
        .first()
        .is_some_and(|b| reformation_status(auth, rules, player, *b).is_ok());
    FaithOverview {
        faith: faith.faith,
        per_turn,
        pantheon: faith.pantheon,
        religion: faith.religion,
        pantheon_cost: pantheon_cost(auth),
        prophet_cost: prophet_cost(auth, rules, player),
        religions_left: religions_left(auth),
        can_found_pantheon,
        can_add_reformation,
        extra_founding_beliefs: crate::effects::player::player_modifiers(auth, rules, player)
            .module.extra_founding_beliefs,
        free_pantheon,
        free_founder: free_beliefs(auth, rules, BeliefCategory::Founder),
        free_follower: free_beliefs(auth, rules, BeliefCategory::Follower),
        free_enhancer: free_beliefs(auth, rules, BeliefCategory::Enhancer),
        free_reformation,
        free_religions: free_religions(auth, rules),
        religions: auth
            .module()
            .religion()
            .religions()
            .iter()
            .map(|r| ReligionSummary {
                religion: r.religion,
                founder: r.founder,
                holy_city: r.holy_city,
                beliefs: r.beliefs.clone(),
                enhanced: r.enhanced,
                counts: religion_counts(auth, r.religion),
            })
            .collect(),
        missionary_purchase: missionary_purchase(auth, rules, player),
    }
}

/// The missionary: the unit kind that spreads a religion, is not a
/// prophet and is bought with faith.
pub fn missionary_kind(rules: &CompiledRuleset) -> Option<UnitKindId> {
    let kind = rules.units.iter().position(|u| {
        u.module.abilities.contains(&UnitAbility::SpreadReligion)
            && !u.module.abilities.contains(&UnitAbility::FoundReligion)
            && u.module.faith_cost.is_some()
    })?;
    Some(UnitKindId(kind as u16))
}

/// The first own city where the player can buy a missionary now.
fn missionary_purchase(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    player: PlayerId,
) -> Option<(CityHandle, u32)> {
    auth.module().religion().player(player).religion?;
    let item = crate::ids::ProductionItem::Unit(missionary_kind(rules)?);
    auth.indexes().cities_of(player).iter().find_map(|city| {
        faith_purchase_status(auth, rules, player, *city, item)
            .ok()
            .map(|cost| (*city, cost))
    })
}

/// Faith the player's cities make in a turn (the pool's income).
pub fn faith_per_turn(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId) -> u32 {
    let mut total = 0u32;
    for handle in auth.indexes().cities_of(player) {
        if let Some(core) = auth.cities().get(*handle) {
            total =
                total.saturating_add(crate::derived::income::city_income(auth, rules, core).faith);
        }
    }
    total
}

/// A great person's improvement on its own tile (Holy Site, ...): the tile is
/// its owner's land without a city, and the terrain suits the improvement.
pub fn great_improvement_status(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    unit: UnitHandle,
) -> Result<(crate::ids::TileId, crate::ids::ImprovementId), FaithBlock> {
    let core = auth.units().get(unit).ok_or(FaithBlock::UnknownUnit)?.core;
    let person = rules
        .module.great_people
        .iter()
        .find(|g| g.unit == core.kind)
        .ok_or(FaithBlock::NotAGreatPerson)?;
    let improvement = *person
        .improvements
        .first()
        .ok_or(FaithBlock::NotAGreatPerson)?;
    if auth.map().owner(core.tile).flatten() != Some(core.owner)
        || auth.indexes().city_on_tile(core.tile).is_some()
    {
        return Err(FaithBlock::NotOwnLand);
    }
    let def = &rules.improvements[improvement.0 as usize];
    let terrain = auth.map().terrain(core.tile);
    let suits = def.valid_terrains.is_empty()
        || def.valid_terrains.iter().any(|t| {
            let code = rules.terrains.get(t.0 as usize).and_then(|d| d.map_code);
            code.is_some() && code.map(crate::ids::TerrainId) == terrain
        });
    if !suits {
        return Err(FaithBlock::WrongTerrain);
    }
    Ok((core.tile, improvement))
}

#[cfg(test)]
#[path = "religion_tests.rs"]
mod tests;
