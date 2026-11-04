//! Great works, tourism and cultural influence (read-only formulas):
//! classic/docs/classic/rules.md–2.3. Systems apply them (`systems::tourism`), the UI and the AI read
//! them; nothing here is cached.
//!
//! Hooks for other tracks: `has_cultural_victory` (victories),
//! `public_opinion_unhappiness` (happiness), `trade_route_science_bonus` and
//! `conquest_reduction` (trade routes, conquest). A trade route between two
//! players adds the shared-route percent (rules 3.10, track A's
//! `world::trade`); the happiness percent (`LessHappy`) holds while the
//! sender's excess happiness is above the target's.

use crate::derived::happiness::excess_snapshot;
use crate::world::city::capital_of;
use crate::effects::city::CityModifiers;
use crate::effects::player::PlayerModifiers;
use crate::ids::{BuildingKindId, CityHandle, PlayerId, TileId, UnitHandle, UnitKindId};
use crate::rules::content::{
    BuildingAbility, GreatWorkSlot, PolicyBranchId, ThemingBonus, UnitAbility,
};
use crate::rules::effects::{Modifier, Selector, YieldKind};
use crate::classic::effects::{GreatPersonKind, TourismCondition};
use crate::rules::CompiledRuleset;
use crate::world::city::CityCore;
use crate::world::religion::Creed;
use crate::world::state::AuthWorld;
use crate::world::tourism::{GreatWork, GreatWorkClass, PublicOpinion};
use crate::world::unit::UnitCore;

// ---- tuning: great works, tourism, influence and public opinion (own design) ----
//
// The design record is classic/docs/classic/BNW-DESIGN.md; the tests at the end of this file check
// the numbers against a reference economy.

/// Culture of a great work per turn in its first era. Own design (Unciv has no great works).
pub const WORK_CULTURE_BASE: u32 = 1;
/// Eras of the work per extra culture a turn: a later work is worth a little more. Own design.
pub const WORK_CULTURE_ERAS_PER_STEP: u32 = 3;
/// Tourism of a great work per turn in its first era. Own design (Unciv has no great works).
pub const WORK_TOURISM_BASE: u32 = 2;
/// Eras of the work per extra tourism a turn. Own design.
pub const WORK_TOURISM_ERAS_PER_STEP: u32 = 2;

/// Culture a work of the era `era` gives per turn. Own design: 1 for the first three eras, 2 for the
/// next three, 3 for the last two.
pub const fn work_culture(era: u8) -> u32 {
    WORK_CULTURE_BASE + era as u32 / WORK_CULTURE_ERAS_PER_STEP
}

/// Tourism a work of the era `era` gives per turn. Own design: from 2 in the first two eras to 5 in
/// the last two.
pub const fn work_tourism(era: u8) -> u32 {
    WORK_TOURISM_BASE + era as u32 / WORK_TOURISM_ERAS_PER_STEP
}

/// Tourism percent when 0, 1, 2 or 3 of the ties (shared religion, open borders, a trade route) join
/// the two nations. Own design: each tie helps less than the one before, so a nation with every tie
/// gets +27 %, not three times the first.
pub const TIE_TOURISM_PERCENT: [i32; 4] = [0, 12, 21, 27];
/// Tourism percent for different ideologies: a quarter lost, so rivals of another creed are the hard
/// targets. Own design.
pub const DIFFERENT_IDEOLOGY_TOURISM_PERCENT: i32 = -25;
/// Tourism percent bonus for a diplomat in the target's capital, only when the ideologies differ: it
/// undoes more than half of the ideology penalty. Own design.
pub const DIPLOMAT_TOURISM_PERCENT: i32 = 15;

/// Per mille of the influence stock that fades each turn: what a nation stops spreading is forgotten
/// with a half-life of about 34 turns. Own design (Unciv has no cultural influence).
pub const INFLUENCE_DECAY_PERMILLE: u64 = 20;
/// Floor of the target's recent culture (the last 8 turns) the influence is measured against, so a
/// nation that makes almost no culture is not "dominated" by a trickle of tourism. Own design.
pub const CULTURE_WINDOW_FLOOR: u64 = 40;

/// Influence needed for level `level` (1 Noticed ... 5 Prevailing), per mille of the target's culture of
/// its last 8 turns: `(825 l^2 + 675 l) / 2`, that is 750, 2325, 4725, 7950 and 12000. Own design: a
/// smooth quadratic through "the stock equals six turns of the target's culture" for the first level and
/// "ninety-six turns" for Prevailing; in steady state (stock = tourism / decay) the levels need the nation's
/// tourism to be 0.12 / 0.37 / 0.76 / 1.27 / 1.92 times the target's culture per turn.
pub const fn level_threshold(level: usize) -> u64 {
    let l = if level > 5 { 5 } else { level as u64 };
    (825 * l * l + 675 * l) / 2
}
/// Thresholds of the five levels, per mille of the target's recent culture ([`level_threshold`]).
pub const LEVEL_THRESHOLDS: [u64; 5] = [
    level_threshold(1),
    level_threshold(2),
    level_threshold(3),
    level_threshold(4),
    level_threshold(5),
];

/// Dissatisfaction (points) at which the people start to murmur, then unrest, then upheaval. Own design.
pub const OPINION_STEPS: [u16; 3] = [1, 3, 6];
/// Unhappiness of the people with their ideology: percent of the population at one point of
/// dissatisfaction (`3 + 3 d`), and the most. Own design: a visible but never crushing penalty.
pub const OPINION_UNHAPPY_BASE_PERCENT: u32 = 3;
/// See [`OPINION_UNHAPPY_BASE_PERCENT`].
pub const OPINION_UNHAPPY_PER_POINT_PERCENT: u32 = 3;
/// See [`OPINION_UNHAPPY_BASE_PERCENT`].
pub const OPINION_UNHAPPY_MAX_PERCENT: u32 = 30;
/// Cap of the pressure one nation puts on another's opinion, in points (a nation cannot be talked out
/// of its creed by one rival alone). Own design.
pub const OPINION_PRESSURE_CAP: u32 = 4;

/// Ideology tenets kept when switching: all but a quarter of them, at least one lost. Own design.
pub const fn tenets_lost_on_switch(tenets: u16) -> u16 {
    let quarter = tenets / 4;
    if quarter == 0 { 1 } else { quarter }
}

/// Stock of influence left after a turn of forgetting. Own design.
pub const fn faded(stock: u64) -> u64 {
    stock - stock / 1000 * INFLUENCE_DECAY_PERMILLE - stock % 1000 * INFLUENCE_DECAY_PERMILLE / 1000
}

/// Tourism a turn that holds `level` (1..5) on a target that made `recent` culture in its last 8 turns:
/// in steady state the stock is tourism / decay, so the tourism is the threshold stock times the decay.
/// Own design; the test `the_levels_ask_for_a_growing_multiple_of_the_targets_culture` states the multiples.
pub fn tourism_to_hold(level: usize, recent: u64) -> u64 {
    (level_threshold(level) * recent.max(CULTURE_WINDOW_FLOOR) / 1000 * INFLUENCE_DECAY_PERMILLE).div_ceil(1000)
}

/// The stock as per mille of the target's recent culture (`recent`: culture of its last 8 turns).
pub fn permille_of(stock: u64, recent: u64) -> u64 {
    stock.saturating_mul(1000) / recent.max(CULTURE_WINDOW_FLOOR)
}

/// Influence level on a nation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum InfluenceLevel {
    Unknown,
    Noticed,
    Admired,
    Imitated,
    Embraced,
    Prevailing,
}

impl InfluenceLevel {
    pub fn index(self) -> u8 {
        self as u8
    }

    pub fn name(self) -> &'static str {
        match self {
            InfluenceLevel::Unknown => "Unknown",
            InfluenceLevel::Noticed => "Noticed",
            InfluenceLevel::Admired => "Admired",
            InfluenceLevel::Imitated => "Imitated",
            InfluenceLevel::Embraced => "Embraced",
            InfluenceLevel::Prevailing => "Prevailing",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InfluenceTrend {
    Falling,
    Static,
    Rising,
}

// ---- slots ----

/// A building slot a new work goes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FreeSlot {
    pub city: CityHandle,
    pub building: BuildingKindId,
}

/// The free slot of type `slot` nearest to `near`: cities by distance, then list
/// order; buildings in the city's order.
pub fn free_slot(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    player: PlayerId,
    slot: GreatWorkSlot,
    near: TileId,
) -> Option<FreeSlot> {
    let dims = auth.map().dimensions;
    let mut best: Option<(i32, FreeSlot)> = None;
    for city in auth.indexes().cities_of(player) {
        let Some(core) = auth.cities().get(*city) else {
            continue;
        };
        let Some(building) = free_building_slot(auth, rules, *city, core, slot) else {
            continue;
        };
        let distance = dims.distance(core.tile, near).unwrap_or(i32::MAX);
        if best.is_none_or(|(d, _)| distance < d) {
            best = Some((
                distance,
                FreeSlot {
                    city: *city,
                    building,
                },
            ));
        }
    }
    best.map(|(_, s)| s)
}

fn free_building_slot(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    city: CityHandle,
    core: &CityCore,
    slot: GreatWorkSlot,
) -> Option<BuildingKindId> {
    let works = auth.module().tourism().city(city);
    core.buildings.iter().copied().find(|b| {
        rules.buildings[b.0 as usize]
            .module.great_work_slots
            .is_some_and(|(s, n)| s == slot && works.count_in(*b) < usize::from(n))
    })
}

/// Slots of each type the player has, filled and total (UI).
pub fn slot_counts(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    player: PlayerId,
    slot: GreatWorkSlot,
) -> (u32, u32) {
    let (mut filled, mut total) = (0, 0);
    for city in auth.indexes().cities_of(player) {
        let Some(core) = auth.cities().get(*city) else {
            continue;
        };
        for b in &core.buildings {
            if let Some((s, n)) = rules.buildings[b.0 as usize].module.great_work_slots {
                if s == slot {
                    total += u32::from(n);
                    filled += auth.module().tourism().city(*city).count_in(*b) as u32;
                }
            }
        }
    }
    (filled, total)
}

// ---- theming ----

/// The theme of a building whose every slot holds a work (the first
/// theming bonus whose requirements hold).
pub fn theme_of(
    rules: &CompiledRuleset,
    building: BuildingKindId,
    works: &[GreatWork],
    owner: PlayerId,
) -> Option<ThemingBonus> {
    let def = &rules.buildings[building.0 as usize];
    let (_, count) = def.module.great_work_slots?;
    if works.len() != usize::from(count) || works.is_empty() {
        return None;
    }
    def.module.theming
        .iter()
        .copied()
        .find(|t| theme_matches(t, works, owner))
}

fn theme_matches(t: &ThemingBonus, works: &[GreatWork], owner: PlayerId) -> bool {
    let art = works
        .iter()
        .filter(|w| w.class == GreatWorkClass::Art)
        .count();
    let artifacts = works
        .iter()
        .filter(|w| w.class == GreatWorkClass::Artifact)
        .count();
    let same_era = works.iter().all(|w| w.era == works[0].era);
    let same_player = works.iter().all(|w| w.creator == works[0].creator);
    let distinct = |key: fn(&GreatWork) -> u8| {
        let mut keys: Vec<u8> = works.iter().map(key).collect();
        keys.sort_unstable();
        keys.windows(2).all(|p| p[0] != p[1])
    };
    let by_owner = works.iter().filter(|w| w.creator == owner).count();
    !(t.must_be_art && artifacts > 0
        || t.must_be_artifact && art > 0
        || t.must_be_equal_art_artifact && art != artifacts
        || t.same_era && !same_era
        || t.unique_eras && !distinct(|w| w.era)
        || t.requires_owner && by_owner != works.len()
        || t.requires_same_player && !same_player
        || t.requires_unique_players && !distinct(|w| w.creator.0)
        || t.requires_any_but_owner && by_owner > 0)
}

/// The works of `building` in `city`, in fill order.
pub fn works_in(auth: &AuthWorld, city: CityHandle, building: BuildingKindId) -> Vec<GreatWork> {
    let tourism = auth.module().tourism();
    tourism
        .city(city)
        .in_building(building)
        .filter_map(|id| tourism.work(id).copied())
        .collect()
}

/// The great works standing in a city's slots.
pub fn city_works(auth: &AuthWorld, city: CityHandle) -> Vec<GreatWork> {
    let tourism = auth.module().tourism();
    tourism.city(city).works.iter().filter_map(|placed| tourism.work(placed.work).copied()).collect()
}

/// How many Great Works stand in the city's buildings.
pub fn great_works_in_city(auth: &AuthWorld, city: CityHandle) -> u32 {
    let Some(core) = auth.cities().get(city) else {
        return 0;
    };
    core.buildings.iter().map(|b| works_in(auth, city, *b).len() as u32).sum()
}

/// Theming bonuses of a city after the player's and the capital's percents
/// (per building, summed; rules.md, great works).
pub fn theming_bonus(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    city: CityHandle,
    core: &CityCore,
    player: &PlayerModifiers,
    local: &CityModifiers,
) -> u32 {
    let mut total = 0u32;
    for building in &core.buildings {
        if rules.buildings[building.0 as usize].module.theming.is_empty() {
            continue;
        }
        let works = works_in(auth, city, *building);
        let Some(theme) = theme_of(rules, *building, &works, core.owner) else {
            continue;
        };
        let mut bonus = i64::from(theme.bonus) * (100 + i64::from(player.module.theming_percent)) / 100;
        if local.module.tourism.theming_percent > 0 {
            bonus = bonus * (100 + i64::from(local.module.tourism.theming_percent)) / 100;
        }
        total = total.saturating_add(bonus.max(0) as u32);
    }
    total
}

// ---- culture of works ----

/// Culture of the city's great works: per work [`work_culture`] of its era + the player's
/// great work culture change, plus the theming bonuses.
pub fn great_work_culture(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    city: CityHandle,
    core: &CityCore,
    local: &CityModifiers,
) -> u32 {
    let works = city_works(auth, city);
    if works.is_empty() {
        return 0;
    }
    let player = crate::effects::player::player_modifiers(auth, rules, core.owner);
    let culture: u32 = works
        .iter()
        .map(|w| (work_culture(w.era) as i32 + player.module.great_work_culture).max(0) as u32)
        .sum();
    culture + theming_bonus(auth, rules, city, core, &player, local)
}

// ---- tourism ----

/// Tourism of one city and its parts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CityTourism {
    pub works: u32,
    pub theming: u32,
    /// Culture of wonders and improvements turned into tourism.
    pub landmarks: u32,
    /// Flat tourism of buildings, wonders and beliefs.
    pub flat: u32,
    pub before_modifiers: u32,
    /// Percent applied to `before_modifiers` (the Internet, Media Culture).
    pub percent: i32,
    pub total: u32,
}

pub fn city_tourism(auth: &AuthWorld, rules: &CompiledRuleset, city: CityHandle) -> CityTourism {
    let Some(core) = auth.cities().get(city) else {
        return CityTourism::default();
    };
    if !may_make_tourism(auth, rules, city, core) {
        return CityTourism::default();
    }
    let local = crate::effects::city::city_modifiers(auth, rules, core);
    let player = crate::effects::player::player_modifiers(auth, rules, core.owner);
    let tourism = local.module.tourism;
    let mut works: i64 = city_works(auth, city).iter().map(|w| i64::from(work_tourism(w.era))).sum();
    works += works * i64::from(tourism.great_works_percent) / 100;
    let theming = theming_bonus(auth, rules, city, core, &player, &local);
    let landmarks = match tourism.landmarks_percent > 0 {
        true => {
            i64::from(landmark_culture(auth, rules, core)) * i64::from(tourism.landmarks_percent)
                / 100
        }
        false => 0,
    };
    let faith = i64::from(tourism.per_faith_building) * faith_buildings(rules, core);
    let flat = i64::from(tourism.flat) + faith;
    let before = (works + i64::from(theming) + landmarks + flat).max(0);
    let percent = player.module.tourism_spread.max(0) + tourism.percent.max(0);
    let total = if percent > 0 {
        before * (100 + i64::from(percent)) / 100
    } else {
        before
    };
    CityTourism {
        works: works.max(0) as u32,
        theming,
        landmarks: landmarks.max(0) as u32,
        flat: flat.max(0) as u32,
        before_modifiers: before as u32,
        percent,
        total: total as u32,
    }
}

/// Without works and without a tourism building no source can give the
/// city tourism (percents multiply nothing): skip its modifiers.
fn may_make_tourism(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    city: CityHandle,
    core: &CityCore,
) -> bool {
    let index = &rules.effect_index.module;
    index.tourism_everywhere
        || !auth.module().tourism().city(city).works.is_empty()
        || core.buildings.iter().any(|b| {
            index
                .tourism_buildings
                .get(b.0 as usize)
                .copied()
                .unwrap_or(false)
        })
}

/// Buildings only faith buys (Sacred Sites: «bought with faith»).
fn faith_buildings(rules: &CompiledRuleset, core: &CityCore) -> i64 {
    core.buildings
        .iter()
        .filter(|b| {
            let def = &rules.buildings[b.0 as usize];
            def.unbuildable && def.module.faith_cost.is_some()
        })
        .count() as i64
}

/// Culture of the city's world wonders and of its worked improved tiles
/// (natural wonders have no culture yield in this ruleset yet).
fn landmark_culture(auth: &AuthWorld, rules: &CompiledRuleset, core: &CityCore) -> u32 {
    let mut culture = 0i32;
    for b in &core.buildings {
        let def = &rules.buildings[b.0 as usize];
        if !def.wonder {
            continue;
        }
        for effect in &def.effects.passive_city {
            if let Modifier::AddYield {
                yield_kind: YieldKind::Culture,
                amount,
            } = effect.modifier
            {
                culture += amount * i32::from(effect.selector == Selector::SourceCity);
            }
        }
    }
    let effects = crate::effects::city::city_tile_effects(auth, rules, core);
    for t in crate::derived::yields::city_work(auth, rules, core).tiles() {
        let Some(imp) = auth.map().working_improvement(t) else {
            continue;
        };
        let def = &rules.improvements[imp.0 as usize];
        culture += def.yields.of(YieldKind::Culture)
            + crate::classic::yields::landmark_era_culture(auth, rules, core, t);
        if !effects.is_empty() {
            culture += effects.bonus(auth, rules, core, t).flat[YieldKind::Culture.index()];
        }
    }
    culture.max(0) as u32
}

/// The player's tourism: the sum of its cities' (rules.md, tourism).
pub fn player_tourism(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId) -> u32 {
    auth.indexes()
        .cities_of(player)
        .iter()
        .map(|c| city_tourism(auth, rules, *c).total)
        .sum()
}

// ---- influence ----

/// `me` has met `other` (influence needs contact).
pub fn met(auth: &AuthWorld, me: PlayerId, other: PlayerId) -> bool {
    auth.knowledge()
        .get(me.0 as usize)
        .is_some_and(|k| k.contact(other).met_round.is_some())
}

/// The religion most of the player's cities follow: ties to the lower id, none without a city.
pub fn religion_in_most_cities(
    auth: &AuthWorld,
    player: PlayerId,
) -> Option<crate::rules::content::ReligionId> {
    let mut counts: Vec<(crate::rules::content::ReligionId, u32)> = Vec::new();
    for city in auth.indexes().cities_of(player) {
        if let Some(Creed::Religion(r)) = crate::derived::religion::city_majority(auth, *city) {
            match counts.iter_mut().find(|(x, _)| *x == r) {
                Some((_, n)) => *n += 1,
                None => counts.push((r, 1)),
            }
        }
    }
    counts.sort_by_key(|(r, n)| (std::cmp::Reverse(*n), r.0));
    counts.first().map(|(r, _)| *r)
}

/// Percent of `me`'s tourism toward `target`.
pub fn tourism_multiplier(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    me: PlayerId,
    target: PlayerId,
) -> i32 {
    let toward = crate::effects::player::player_modifiers(auth, rules, me).module.tourism_toward;
    let of = |c: TourismCondition| toward[c.index()];
    let mut m = 0;
    let mut ties = 0usize;
    let mine = religion_in_most_cities(auth, me);
    if mine.is_some() && mine == religion_in_most_cities(auth, target) {
        ties += 1;
        m += of(TourismCondition::SharedReligion);
    }
    if auth.diplomacy().grants_entry(target, me) {
        ties += 1;
        m += of(TourismCondition::OpenBorders);
    }
    if trade_connected(auth, me, target) {
        ties += 1;
        m += of(TourismCondition::TradeRoute);
    }
    m += TIE_TOURISM_PERCENT[ties];
    let (a, b) = (ideology(auth, me), ideology(auth, target));
    if a.is_some() && b.is_some() && a != b {
        m += DIFFERENT_IDEOLOGY_TOURISM_PERCENT;
        if crate::systems::congress::diplomat_visiting(auth, me, target) {
            m += DIPLOMAT_TOURISM_PERCENT;
        }
    }
    if a.is_some() && a == b {
        m += of(TourismCondition::SameIdeology);
    }
    if excess_snapshot(auth, me) > excess_snapshot(auth, target) {
        m += of(TourismCondition::LessHappy);
    }
    m + of(TourismCondition::CommonEnemy) * common_enemies(auth, me, target)
}

/// A trade route runs between the two players, either way.
pub fn trade_connected(auth: &AuthWorld, a: PlayerId, b: PlayerId) -> bool {
    let owner_of = |c: CityHandle| auth.cities().get(c).map(|core| core.owner);
    auth.module().trade().routes().iter().any(|r| {
        let d = owner_of(r.destination);
        (r.owner == a && d == Some(b)) || (r.owner == b && d == Some(a))
    })
}

fn ideology(auth: &AuthWorld, player: PlayerId) -> Option<PolicyBranchId> {
    auth.module().culture().of(player).ideology
}

/// Third nations both players are at war with.
fn common_enemies(auth: &AuthWorld, me: PlayerId, target: PlayerId) -> i32 {
    let diplomacy = auth.diplomacy();
    (0..auth.players().count() as u8)
        .map(PlayerId)
        .filter(|x| *x != me && *x != target && auth.players().is_alive(*x))
        .filter(|x| diplomacy.at_war(me, *x) && diplomacy.at_war(target, *x))
        .count() as i32
}

/// The target owns a building that nullifies the Internet (Great Firewall).
fn has_firewall(auth: &AuthWorld, rules: &CompiledRuleset, target: PlayerId) -> bool {
    auth.indexes().cities_of(target).iter().any(|c| {
        auth.cities().get(*c).is_some_and(|core| {
            core.buildings.iter().any(|b| {
                rules.buildings[b.0 as usize]
                    .module.abilities
                    .contains(&BuildingAbility::NullifyInfluence)
            })
        })
    })
}

/// Influence `me` gains on `target` per turn.
pub fn influence_per_turn(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    me: PlayerId,
    target: PlayerId,
) -> u64 {
    influence_toward(
        auth,
        rules,
        &PlayerTourismTotals::of(auth, rules, me),
        target,
    )
}

/// The player's tourism summed once for every target: the total and the
/// part a Great Firewall takes away (the Internet's share of the base).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerTourismTotals {
    pub player: PlayerId,
    pub total: i64,
    pub spread_part: i64,
}

impl PlayerTourismTotals {
    pub fn of(auth: &AuthWorld, rules: &CompiledRuleset, me: PlayerId) -> Self {
        let (mut total, mut before) = (0i64, 0i64);
        for city in auth.indexes().cities_of(me) {
            let t = city_tourism(auth, rules, *city);
            total += i64::from(t.total);
            before += i64::from(t.before_modifiers);
        }
        let spread = match before > 0 {
            true => crate::effects::player::player_modifiers(auth, rules, me).module.tourism_spread,
            false => 0,
        };
        // International Games: +100 % tourism for a while (§6.8).
        let boost = i64::from(crate::systems::league_projects::boost_percent(
            auth.module().congress().rewards_of(me).tourism_boost_turns,
        ));
        Self {
            player: me,
            total: total * (100 + boost) / 100,
            spread_part: before * i64::from(spread.max(0)) / 100 * (100 + boost) / 100,
        }
    }
}

/// Influence per turn on `target` from the player's summed tourism.
pub fn influence_toward(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    totals: &PlayerTourismTotals,
    target: PlayerId,
) -> u64 {
    let me = totals.player;
    if me == target || !auth.players().is_alive(target) || !met(auth, me, target) {
        return 0;
    }
    let mut total = totals.total;
    if total <= 0 {
        return 0;
    }
    if totals.spread_part > 0 && has_firewall(auth, rules, target) {
        total -= totals.spread_part;
    }
    let percent = match capital_of(auth, rules, me) {
        Some(_) => tourism_multiplier(auth, rules, me, target),
        None => 0,
    };
    (total * (100 + i64::from(percent)) / 100).max(0) as u64
}

/// The influence of `me` on `target` as per mille of the target's culture of its last 8 turns (None: not
/// met). The level thresholds ([`level_threshold`]) are read from this number.
pub fn influence_permille(auth: &AuthWorld, me: PlayerId, target: PlayerId) -> Option<u64> {
    if !met(auth, me, target) {
        return None;
    }
    let recent = u64::from(auth.module().culture().of(target).recent_culture(crate::world::culture::RECENT_TURNS));
    let stock = auth.module().tourism().player(me).influence_on(target);
    Some(permille_of(stock, recent))
}

/// Level of `me`'s influence on `target` (None: not met): the number of thresholds the influence has
/// reached (see [`influence_permille`]).
pub fn influence_level(auth: &AuthWorld, me: PlayerId, target: PlayerId) -> Option<InfluenceLevel> {
    let permille = influence_permille(auth, me, target)?;
    let level = LEVEL_THRESHOLDS.iter().filter(|t| permille >= **t).count();
    Some(match level {
        0 => InfluenceLevel::Unknown,
        1 => InfluenceLevel::Noticed,
        2 => InfluenceLevel::Admired,
        3 => InfluenceLevel::Imitated,
        4 => InfluenceLevel::Embraced,
        _ => InfluenceLevel::Prevailing,
    })
}

/// The influence trend: the stock compared with the stock before the last turn (the stock already
/// forgets a part each turn, so a rising stock means this turn's tourism beat the forgetting).
pub fn influence_trend(auth: &AuthWorld, me: PlayerId, target: PlayerId) -> InfluenceTrend {
    let state = auth.module().tourism().player(me);
    match state.influence_on(target).cmp(&state.influence_before_on(target)) {
        std::cmp::Ordering::Greater => InfluenceTrend::Rising,
        std::cmp::Ordering::Less => InfluenceTrend::Falling,
        std::cmp::Ordering::Equal => InfluenceTrend::Static,
    }
}

/// The influence stock that makes `level` (1..5) on `target` now: the threshold times its culture of the last
/// 8 turns (at least [`CULTURE_WINDOW_FLOOR`]).
pub fn stock_for_level(auth: &AuthWorld, target: PlayerId, level: usize) -> u64 {
    let recent = u64::from(auth.module().culture().of(target).recent_culture(crate::world::culture::RECENT_TURNS));
    level_threshold(level) * recent.max(CULTURE_WINDOW_FLOOR) / 1000
}

/// Turns until a stock of `stock` that gains `gain` a turn and forgets [`INFLUENCE_DECAY_PERMILLE`] per mille
/// reaches `need` (0 when it has; None when it never does: the stock tends to gain / decay).
pub fn turns_to_reach(stock: u64, gain: u64, need: u64) -> Option<u32> {
    let mut stock = stock;
    if stock >= need {
        return Some(0);
    }
    for turns in 1..=999u32 {
        stock = faded(stock) + gain;
        if stock >= need {
            return Some(turns);
        }
    }
    None
}

/// Turns until Embraced: 0 when there, 999 when the stock never gets there at the present tourism
/// (the stock tends to tourism / decay).
pub fn turns_to_influential(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    me: PlayerId,
    target: PlayerId,
) -> u32 {
    if influence_level(auth, me, target) >= Some(InfluenceLevel::Embraced) {
        return 0;
    }
    let need = stock_for_level(auth, target, 4);
    let gain = influence_per_turn(auth, rules, me, target);
    let stock = auth.module().tourism().player(me).influence_on(target);
    turns_to_reach(stock, gain, need).unwrap_or(999)
}

/// Alive nations `me` is Embraced (or more) on (city-states and
/// barbarians are not nations).
pub fn influential_on(auth: &AuthWorld, me: PlayerId) -> u32 {
    (0..auth.players().count() as u8)
        .map(PlayerId)
        .filter(|p| *p != me && auth.players().is_alive(*p) && auth.players().is_major(*p))
        .filter(|p| influence_level(auth, me, *p) >= Some(InfluenceLevel::Embraced))
        .count() as u32
}

/// Nations to be Embraced on for a cultural victory: every other
/// alive one.
pub fn nations_needed_for_cultural_victory(auth: &AuthWorld) -> u32 {
    let alive = (0..auth.players().count() as u8)
        .map(PlayerId)
        .filter(|p| auth.players().is_alive(*p) && auth.players().is_major(*p))
        .count() as u32;
    alive.saturating_sub(1)
}

/// Cultural victory query for the victories track: `me` is Embraced on
/// every other alive nation (it must have met them all).
pub fn has_cultural_victory(auth: &AuthWorld, me: PlayerId) -> bool {
    let needed = nations_needed_for_cultural_victory(auth);
    needed > 0 && auth.players().is_alive(me) && influential_on(auth, me) >= needed
}

/// Science (hundredths a turn) a trade route of ours into `target`'s city gains from our influence on it:
/// half a science per level from Admired (0.5, 1.0, 1.5, 2.0 up to Prevailing). Own design.
pub fn trade_route_science_bonus(level: Option<InfluenceLevel>) -> u32 {
    match level {
        Some(l) if l >= InfluenceLevel::Admired => 50 * (u32::from(l.index()) - 1),
        _ => 0,
    }
}

/// Percent of the citizens a captured city of `target` keeps that conquest would have cost: 10 per level from
/// Admired (10, 20, 30, 40 up to Prevailing). A people that knows our culture resists less. Own design.
pub fn conquest_reduction(level: Option<InfluenceLevel>) -> u32 {
    match level {
        Some(l) if l >= InfluenceLevel::Admired => 10 * (u32::from(l.index()) - 1),
        _ => 0,
    }
}

// ---- overview ----

/// Influence on one nation the player met (Culture overview).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InfluenceView {
    pub other: PlayerId,
    pub influence: u64,
    /// The influence as per mille of its culture of the last 8 turns (what the level is read from).
    pub permille: u64,
    /// The influence needed to be Embraced on it now (the stock; it moves with its culture).
    pub needed: u64,
    /// Its lifetime culture.
    pub their_lifetime: u64,
    pub per_turn: u64,
    pub level: InfluenceLevel,
    pub trend: InfluenceTrend,
    /// Its level on us (its pressure on our ideology).
    pub their_level: InfluenceLevel,
}

/// The player's cultural standing: what the Culture overview shows and the
/// AI reads.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TourismObservation {
    pub tourism: u32,
    /// (slot type, filled, total) for art/artifacts, literature, music.
    pub slots: Vec<(GreatWorkSlot, u32, u32)>,
    pub opinion: PublicOpinion,
    pub dissatisfaction: u16,
    pub preferred: Option<PolicyBranchId>,
    /// Every alive nation the player met, in id order.
    pub influence: Vec<InfluenceView>,
    pub influential_on: u32,
    pub needed_for_victory: u32,
    /// Antiquity sites the player sees and nobody digs yet.
    pub sites: Vec<TileId>,
    /// Tiles the player's archaeologists dig now.
    pub digging: Vec<TileId>,
    /// The player's finished digs and the choices each offers now.
    pub pending: Vec<(TileId, Vec<crate::derived::archaeology::ArchaeologyChoice>)>,
}

pub fn tourism_observation(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    me: PlayerId,
) -> TourismObservation {
    let totals = PlayerTourismTotals::of(auth, rules, me);
    let slots = [
        GreatWorkSlot::ArtArtifact,
        GreatWorkSlot::Literature,
        GreatWorkSlot::Music,
    ]
    .into_iter()
    .map(|slot| {
        let (filled, total) = slot_counts(auth, rules, me, slot);
        (slot, filled, total)
    })
    .collect();
    let state = auth.module().tourism().player(me);
    let influence = (0..auth.players().count() as u8)
        .map(PlayerId)
        .filter(|p| *p != me && auth.players().is_alive(*p))
        .filter_map(|p| influence_view(auth, rules, &totals, p))
        .collect();
    TourismObservation {
        tourism: totals.total.max(0) as u32,
        slots,
        opinion: state.opinion,
        dissatisfaction: state.dissatisfaction,
        preferred: state.preferred,
        influence,
        influential_on: influential_on(auth, me),
        needed_for_victory: nations_needed_for_cultural_victory(auth),
        sites: crate::derived::archaeology::open_sites(auth, rules, me),
        digging: auth
            .module()
            .tourism()
            .archaeology()
            .digs
            .iter()
            .filter(|d| d.player == me)
            .map(|d| d.tile)
            .collect(),
        pending: crate::derived::archaeology::pending_choices(auth, rules, me),
    }
}

fn influence_view(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    totals: &PlayerTourismTotals,
    other: PlayerId,
) -> Option<InfluenceView> {
    let me = totals.player;
    let level = influence_level(auth, me, other)?;
    Some(InfluenceView {
        other,
        influence: auth.module().tourism().player(me).influence_on(other),
        permille: influence_permille(auth, me, other).unwrap_or(0),
        needed: stock_for_level(auth, other, 4),
        their_lifetime: auth.module().culture().of(other).lifetime,
        per_turn: influence_toward(auth, rules, totals, other),
        level,
        trend: influence_trend(auth, me, other),
        their_level: influence_level(auth, other, me).unwrap_or(InfluenceLevel::Unknown),
    })
}

// ---- public opinion ----

/// Public opinion of a player's ideology.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OpinionResult {
    pub opinion: PublicOpinion,
    pub dissatisfaction: u16,
    pub preferred: Option<PolicyBranchId>,
}

/// Public opinion of `me` about its own ideology. Own design: every nation with an ideology presses on
/// `me` by how much further its influence on `me` reaches than `me`'s on it, in points of 1000 per
/// mille ([`influence_permille`]), at most [`OPINION_PRESSURE_CAP`] points a nation; the pressure of
/// each ideology is the sum over its nations. The dissatisfaction is the pressure of the other
/// ideologies minus that of `me`'s own, rounded up.
pub fn public_opinion(auth: &AuthWorld, rules: &CompiledRuleset, me: PlayerId) -> OpinionResult {
    let Some(mine) = ideology(auth, me) else {
        return OpinionResult::default();
    };
    // Pressure per ideology in per mille.
    let mut pressure: Vec<(PolicyBranchId, i32)> =
        ideologies(rules).into_iter().map(|b| (b, 0)).collect();
    for p in (0..auth.players().count() as u8).map(PlayerId) {
        let Some(theirs) = ideology(auth, p).filter(|_| p != me && auth.players().is_alive(p))
        else {
            continue;
        };
        let reach = |a, b| influence_permille(auth, a, b).unwrap_or(0) as i64;
        let d = (reach(p, me) - reach(me, p)).clamp(0, i64::from(OPINION_PRESSURE_CAP) * 1000) as i32;
        if let Some(entry) = pressure.iter_mut().find(|(b, _)| *b == theirs) {
            entry.1 += d;
        }
    }
    opinion_from(mine, &pressure, rules.roles.module.ideology_tie(mine))
}

/// Opinion from the pressure of every ideology (per mille); `tie` is preferred when the two others press
/// equally.
fn opinion_from(
    mine: PolicyBranchId,
    pressure: &[(PolicyBranchId, i32)],
    tie: Option<PolicyBranchId>,
) -> OpinionResult {
    let own = pressure
        .iter()
        .find(|(b, _)| *b == mine)
        .map(|(_, p)| *p)
        .unwrap_or(0);
    let others: Vec<(PolicyBranchId, i32)> = pressure
        .iter()
        .copied()
        .filter(|(b, _)| *b != mine)
        .collect();
    let sum: i32 = others.iter().map(|(_, p)| *p).sum();
    if own >= sum {
        return OpinionResult::default();
    }
    // Rounded up to whole points.
    let dissatisfaction = ((sum - own + 999) / 1000).clamp(0, i32::from(u16::MAX)) as u16;
    let top = others.iter().map(|(_, p)| *p).max().unwrap_or(0);
    let tied: Vec<PolicyBranchId> = others
        .iter()
        .filter(|(_, p)| *p == top)
        .map(|(b, _)| *b)
        .collect();
    let preferred = match tied.as_slice() {
        [one] => Some(*one),
        _ => tie.filter(|b| tied.contains(b)).or(tied.first().copied()),
    };
    let opinion = match dissatisfaction {
        d if d >= OPINION_STEPS[2] => PublicOpinion::Upheaval,
        d if d >= OPINION_STEPS[1] => PublicOpinion::Unrest,
        d if d >= OPINION_STEPS[0] => PublicOpinion::Murmurs,
        _ => PublicOpinion::Content,
    };
    OpinionResult {
        opinion,
        dissatisfaction,
        preferred,
    }
}

/// The ideology branches of the rules, in id order.
pub fn ideologies(rules: &CompiledRuleset) -> Vec<PolicyBranchId> {
    (0..rules.module.policy_branches.len())
        .filter(|i| {
            rules.module.policy_branches[*i].kind == crate::rules::content::PolicyBranchKind::Ideology
        })
        .map(|i| PolicyBranchId(i as u16))
        .collect()
}



/// Unhappiness the player's public opinion causes: [`opinion_unhappy_percent`] of the population of
/// all its cities, rounded up (0 while the people are content).
pub fn public_opinion_unhappiness(auth: &AuthWorld, me: PlayerId) -> u32 {
    let d = auth.module().tourism().player(me).dissatisfaction;
    let percent = opinion_unhappy_percent(d);
    if percent == 0 {
        return 0;
    }
    let population: u32 = auth
        .indexes()
        .cities_of(me)
        .iter()
        .filter_map(|c| auth.cities().get(*c))
        .map(|c| c.population)
        .sum();
    (population * percent).div_ceil(100)
}

/// Percent of the population that is unhappy with its ideology at `d` points of dissatisfaction:
/// `3 + 3 d`, at most 30 (0 at 0 points). Own design.
pub const fn opinion_unhappy_percent(d: u16) -> u32 {
    if d == 0 {
        return 0;
    }
    let percent = OPINION_UNHAPPY_BASE_PERCENT + OPINION_UNHAPPY_PER_POINT_PERCENT * d as u32;
    if percent > OPINION_UNHAPPY_MAX_PERCENT { OPINION_UNHAPPY_MAX_PERCENT } else { percent }
}

// ---- great people ----

/// Culture of a Great Writer's Political Treatise: the player's culture of
/// the last `turns` turns.
pub fn treatise_culture(auth: &AuthWorld, player: PlayerId, turns: u16) -> u32 {
    auth.module()
        .culture()
        .of(player)
        .recent_culture(usize::from(turns))
}

/// Influence of a Great Musician's concert on the land's owner: the musician's `one_shot` turns of the
/// player's tourism, but never less than one level's worth of the audience's culture
/// ([`level_threshold`] of the first level, so a concert always makes the audience at least Noticed).
/// Taken when the concert plays, not when the musician is born. Own design.
pub fn concert_strength(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    player: PlayerId,
    audience: PlayerId,
    one_shot: u16,
) -> u64 {
    let strength = u64::from(one_shot) * u64::from(player_tourism(auth, rules, player));
    let recent = u64::from(auth.module().culture().of(audience).recent_culture(crate::world::culture::RECENT_TURNS));
    strength.max(level_threshold(1) * recent.max(CULTURE_WINDOW_FLOOR) / 1000 + 1)
}

// ---- great person actions ----

/// Why a tourism action is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TourismBlock {
    UnknownUnit,
    /// Not a Great Writer, Artist or Musician (or not this action's one).
    WrongUnit,
    /// The great person must stand in or next to one of its cities.
    NotNearOwnCity,
    NoFreeSlot,
    /// The concert needs the land of another nation the player met.
    NotForeignLand,
    NoCulture,
    NoIdeology,
    OpinionContent,
    UnknownPlayer,
}

impl TourismBlock {
    pub const ALL: [TourismBlock; 9] = [
        TourismBlock::UnknownUnit,
        TourismBlock::WrongUnit,
        TourismBlock::NotNearOwnCity,
        TourismBlock::NoFreeSlot,
        TourismBlock::NotForeignLand,
        TourismBlock::NoCulture,
        TourismBlock::NoIdeology,
        TourismBlock::OpinionContent,
        TourismBlock::UnknownPlayer,
    ];

    pub fn reason(self) -> &'static str {
        match self {
            TourismBlock::UnknownUnit => "unknown unit",
            TourismBlock::WrongUnit => "this unit cannot do that",
            TourismBlock::NotNearOwnCity => "must stand in or next to one of your cities",
            TourismBlock::NoFreeSlot => "no free great work slot of this kind",
            TourismBlock::NotForeignLand => "must stand in the land of a nation you met",
            TourismBlock::NoCulture => "no culture of the last turns to give",
            TourismBlock::NoIdeology => "no ideology",
            TourismBlock::OpinionContent => "the people are content with the ideology",
            TourismBlock::UnknownPlayer => "unknown player",
        }
    }
}

/// The class of work a great person creates (Writer, Artist, Musician).
pub fn work_class(rules: &CompiledRuleset, kind: UnitKindId) -> Option<GreatWorkClass> {
    match crate::derived::religion::great_person_kind(rules, kind)? {
        GreatPersonKind::Writer => Some(GreatWorkClass::Literature),
        GreatPersonKind::Artist => Some(GreatWorkClass::Art),
        GreatPersonKind::Musician => Some(GreatWorkClass::Music),
        _ => None,
    }
}

fn unit_core(auth: &AuthWorld, unit: UnitHandle) -> Result<UnitCore, TourismBlock> {
    auth.units()
        .get(unit)
        .map(|v| *v.core)
        .ok_or(TourismBlock::UnknownUnit)
}

fn ability_value(
    rules: &CompiledRuleset,
    kind: UnitKindId,
    pick: fn(&UnitAbility) -> Option<u16>,
) -> Option<u16> {
    rules.units[kind.0 as usize].module.abilities.iter().find_map(pick)
}

/// The great person may create its work: it stands in or next to one of
/// its cities and a slot of the work's type is free.
pub fn great_work_status(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    unit: UnitHandle,
) -> Result<(GreatWorkClass, FreeSlot), TourismBlock> {
    let core = unit_core(auth, unit)?;
    let class = work_class(rules, core.kind).ok_or(TourismBlock::WrongUnit)?;
    let dims = auth.map().dimensions;
    let near = auth.indexes().cities_of(core.owner).iter().any(|c| {
        auth.cities()
            .get(*c)
            .is_some_and(|city| dims.distance(city.tile, core.tile).is_some_and(|d| d <= 1))
    });
    if !near {
        return Err(TourismBlock::NotNearOwnCity);
    }
    let slot = free_slot(auth, rules, core.owner, class.slot(), core.tile)
        .ok_or(TourismBlock::NoFreeSlot)?;
    Ok((class, slot))
}

/// The Great Writer's Political Treatise and the culture it gives.
pub fn treatise_status(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    unit: UnitHandle,
) -> Result<u32, TourismBlock> {
    let core = unit_core(auth, unit)?;
    let turns = ability_value(rules, core.kind, |a| match a {
        UnitAbility::CultureTurnsToCount(n) => Some(*n),
        _ => None,
    })
    .ok_or(TourismBlock::WrongUnit)?;
    // × the game speed's culture percent (Unciv: PolicyManager's culture of the last turns, `cultureCostModifier`).
    let culture = treatise_culture(auth, core.owner, turns);
    match kernel::rules::speeds::SpeedValues::scale(culture, auth.settings().speed(rules).culture_percent) {
        0 => Err(TourismBlock::NoCulture),
        culture => Ok(culture),
    }
}

/// The Great Musician's concert: the land's owner and the influence on it.
pub fn concert_status(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    unit: UnitHandle,
) -> Result<(PlayerId, u64), TourismBlock> {
    let core = unit_core(auth, unit)?;
    let one_shot = ability_value(rules, core.kind, |a| match a {
        UnitAbility::OneShotTourism(n) => Some(*n),
        _ => None,
    })
    .ok_or(TourismBlock::WrongUnit)?;
    let target = auth
        .map()
        .owner(core.tile)
        .flatten()
        .filter(|o| *o != core.owner && auth.players().is_alive(*o) && met(auth, core.owner, *o))
        .ok_or(TourismBlock::NotForeignLand)?;
    Ok((target, concert_strength(auth, rules, core.owner, target, one_shot)))
}

/// Percent of a concert's influence that reaches the other nations
/// (`OneShotTourismPercentOthers`).
pub fn concert_share(rules: &CompiledRuleset, kind: UnitKindId) -> u64 {
    let share = ability_value(rules, kind, |a| match a {
        UnitAbility::OneShotTourismPercentOthers(n) => Some(*n),
        _ => None,
    });
    u64::from(share.unwrap_or(0))
}

/// The player may switch to the ideology its people prefer (not content).
pub fn switch_ideology_status(
    auth: &AuthWorld,
    player: PlayerId,
) -> Result<PolicyBranchId, TourismBlock> {
    ideology(auth, player).ok_or(TourismBlock::NoIdeology)?;
    let state = auth.module().tourism().player(player);
    match (state.opinion, state.preferred) {
        (PublicOpinion::Content, _) | (_, None) => Err(TourismBlock::OpinionContent),
        (_, Some(preferred)) => Ok(preferred),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn work(class: GreatWorkClass, era: u8, creator: u8) -> GreatWork {
        GreatWork {
            class,
            era,
            creator: PlayerId(creator),
        }
    }

    const ART: GreatWorkClass = GreatWorkClass::Art;
    const ARTIFACT: GreatWorkClass = GreatWorkClass::Artifact;

    #[test]
    fn theming_requirements_follow_the_rules_table() {
        let owner = PlayerId(0);
        let hermitage = ThemingBonus {
            bonus: 3,
            unique_eras: true,
            must_be_art: true,
            requires_unique_players: true,
            ..ThemingBonus::default()
        };
        let mixed = [work(ART, 1, 0), work(ART, 2, 1), work(ART, 3, 2)];
        assert!(theme_matches(&hermitage, &mixed, owner));
        let same_era = [work(ART, 1, 0), work(ART, 1, 1), work(ART, 3, 2)];
        assert!(!theme_matches(&hermitage, &same_era, owner), "eras repeat");
        let with_artifact = [work(ART, 1, 0), work(ARTIFACT, 2, 1), work(ART, 3, 2)];
        assert!(
            !theme_matches(&hermitage, &with_artifact, owner),
            "art only"
        );
        let louvre = ThemingBonus {
            bonus: 4,
            unique_eras: true,
            must_be_equal_art_artifact: true,
            requires_unique_players: true,
            ..ThemingBonus::default()
        };
        let half = [
            work(ART, 0, 0),
            work(ARTIFACT, 1, 1),
            work(ART, 2, 2),
            work(ARTIFACT, 3, 3),
        ];
        assert!(theme_matches(&louvre, &half, owner));
        let foreign = ThemingBonus {
            bonus: 2,
            same_era: true,
            must_be_art: true,
            requires_any_but_owner: true,
            ..ThemingBonus::default()
        };
        assert!(theme_matches(
            &foreign,
            &[work(ART, 2, 1), work(ART, 2, 3)],
            owner
        ));
        assert!(!theme_matches(
            &foreign,
            &[work(ART, 2, 1), work(ART, 2, 0)],
            owner
        ));
    }

    #[test]
    fn influence_tables_of_trade_routes_and_conquest() {
        let levels = [
            None,
            Some(InfluenceLevel::Unknown),
            Some(InfluenceLevel::Noticed),
            Some(InfluenceLevel::Admired),
            Some(InfluenceLevel::Imitated),
            Some(InfluenceLevel::Embraced),
            Some(InfluenceLevel::Prevailing),
        ];
        let science: Vec<u32> = levels
            .iter()
            .map(|l| trade_route_science_bonus(*l))
            .collect();
        assert_eq!(science, [0, 0, 0, 50, 100, 150, 200]);
        let conquest: Vec<u32> = levels.iter().map(|l| conquest_reduction(*l)).collect();
        assert_eq!(conquest, [0, 0, 0, 10, 20, 30, 40]);
    }

    #[test]
    fn opinion_levels_and_the_tie_preference() {
        let [freedom, autocracy, order] = [0, 1, 2].map(PolicyBranchId);
        let content = opinion_from(
            freedom,
            &[(freedom, 3000), (autocracy, 1000), (order, 2000)],
            Some(order),
        );
        assert_eq!(content.opinion, PublicOpinion::Content);
        // Freedom's tie goes to Order (rules.md).
        let tie = opinion_from(
            freedom,
            &[(freedom, 0), (autocracy, 1000), (order, 1000)],
            Some(order),
        );
        assert_eq!(
            (tie.opinion, tie.dissatisfaction, tie.preferred),
            (PublicOpinion::Murmurs, 2, Some(order))
        );
        let resistance = opinion_from(
            order,
            &[(freedom, 4000), (autocracy, 0), (order, 0)],
            Some(autocracy),
        );
        assert_eq!(
            (resistance.opinion, resistance.preferred),
            (PublicOpinion::Unrest, Some(freedom))
        );
        let wave = opinion_from(
            autocracy,
            &[(freedom, 3000), (autocracy, 0), (order, 3000)],
            Some(freedom),
        );
        assert_eq!(
            (wave.opinion, wave.preferred),
            (PublicOpinion::Upheaval, Some(freedom))
        );
    }

    // ---- own functions: values, levels, forgetting, opinion ----

    #[test]
    fn a_later_work_is_worth_a_little_more_culture_and_tourism() {
        let culture: Vec<u32> = (0..8u8).map(work_culture).collect();
        let tourism: Vec<u32> = (0..8u8).map(work_tourism).collect();
        assert_eq!(culture, [1, 1, 1, 2, 2, 2, 3, 3]);
        assert_eq!(tourism, [2, 2, 3, 3, 4, 4, 5, 5]);
        // A work always gives more tourism than culture: tourism is what the world sees, culture what the owner keeps.
        assert!(culture.iter().zip(&tourism).all(|(c, t)| t > c));
    }

    #[test]
    fn the_level_thresholds_grow_by_a_widening_step() {
        assert_eq!(LEVEL_THRESHOLDS, [750, 2325, 4725, 7950, 12000]);
        let steps: Vec<u64> = LEVEL_THRESHOLDS.windows(2).map(|w| w[1] - w[0]).collect();
        assert!(steps.windows(2).all(|s| s[1] > s[0]), "each level costs more than the one before: {steps:?}");
        assert_eq!(level_threshold(9), level_threshold(5), "no level beyond Prevailing");
    }

    #[test]
    fn the_levels_ask_for_a_growing_multiple_of_the_targets_culture() {
        // A target making 40 culture a turn (320 in its last 8 turns): the tourism that holds each level.
        let needed: Vec<u64> = (1..=5).map(|l| tourism_to_hold(l, 320)).collect();
        assert_eq!(needed, [5, 15, 31, 51, 77]);
        // Multiples of the target's culture per turn: 0.12, 0.37, 0.76, 1.27, 1.92.
        let ratio: Vec<u64> = needed.iter().map(|n| n * 100 / 40).collect();
        assert_eq!(ratio, [12, 37, 77, 127, 192]);
        // Even a nation with no culture at all costs something (the floor of 40).
        assert_eq!(tourism_to_hold(1, 0), 1);
    }

    #[test]
    fn influence_is_forgotten_at_two_percent_a_turn() {
        assert_eq!(faded(0), 0);
        assert_eq!(faded(1000), 980);
        assert_eq!(faded(999), 980, "the forgotten part is rounded down: 19.98 -> 19");
        let mut stock = 10_000u64;
        let mut halved_after = 0;
        for turn in 1..=100 {
            stock = faded(stock);
            if stock <= 5_000 && halved_after == 0 {
                halved_after = turn;
            }
        }
        assert!((33..=36).contains(&halved_after), "half-life {halved_after} turns");
        // Steady state: tourism t a turn settles the stock near t / 0.02 = 50 t.
        let mut stock = 0u64;
        for _ in 0..600 {
            stock = faded(stock) + 40;
        }
        assert!((1950..=2010).contains(&stock), "{stock}");
    }

    #[test]
    fn ties_help_less_each_time() {
        assert_eq!(TIE_TOURISM_PERCENT, [0, 12, 21, 27]);
        let gains: Vec<i32> = TIE_TOURISM_PERCENT.windows(2).map(|w| w[1] - w[0]).collect();
        assert!(gains.windows(2).all(|g| g[1] < g[0]), "{gains:?}");
        assert!(DIFFERENT_IDEOLOGY_TOURISM_PERCENT + DIPLOMAT_TOURISM_PERCENT < 0, "a diplomat softens the friction, never removes it");
    }

    #[test]
    fn the_unhappiness_of_a_divided_people_is_visible_but_capped() {
        let percent: Vec<u32> = (0..12u16).map(opinion_unhappy_percent).collect();
        assert_eq!(percent, [0, 6, 9, 12, 15, 18, 21, 24, 27, 30, 30, 30]);
        // A reference empire of 60 citizens: from 4 to 18 unhappy.
        let unhappy = |d: u16| (60 * opinion_unhappy_percent(d)).div_ceil(100);
        assert_eq!((unhappy(1), unhappy(3), unhappy(6), unhappy(9)), (4, 8, 13, 18));
    }

    #[test]
    fn switching_ideology_costs_a_quarter_of_the_tenets_at_least_one() {
        let lost: Vec<u16> = [0u16, 1, 3, 4, 8, 12, 16].into_iter().map(tenets_lost_on_switch).collect();
        assert_eq!(lost, [1, 1, 1, 1, 2, 3, 4]);
    }

    #[test]
    fn opinion_steps_name_the_unrest() {
        let [freedom, autocracy, order] = [0, 1, 2].map(PolicyBranchId);
        let result = |d: i32| opinion_from(freedom, &[(freedom, 0), (autocracy, d), (order, 0)], None);
        assert_eq!(result(0).opinion, PublicOpinion::Content);
        assert_eq!(result(1).opinion, PublicOpinion::Murmurs);
        assert_eq!(result(1000).dissatisfaction, 1);
        assert_eq!(result(1001).dissatisfaction, 2, "rounded up to a whole point");
        assert_eq!(result(2000).opinion, PublicOpinion::Murmurs);
        assert_eq!(result(2001).opinion, PublicOpinion::Unrest, "2.001 points count as 3");
        assert_eq!(result(5000).opinion, PublicOpinion::Unrest);
        assert_eq!(result(5001).opinion, PublicOpinion::Upheaval);
    }
}
