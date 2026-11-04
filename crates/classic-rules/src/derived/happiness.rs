//! Happiness of an empire (classic/docs/classic/rules.md): what makes the people
//! happy, what makes them unhappy, and what an unhappy empire suffers.
//!
//! ```text
//! happiness   = the difficulty's base happiness (default level 9, AI 15)
//!             + Σ luxuries with a copy available (own, imported, from
//!               allied city-states; each kind once: its happiness, ×1.5
//!               from allies with Cultural Diplomacy, + the difficulty's extra
//!               happiness per luxury
//!               + Stat::Module(crate::rules::stats::ClassicStat::HappinessPerLuxury))
//!             + Σ cities min(city happiness, population)      (local, capped)
//!             + player happiness of buildings and wonders      (global)
//!             + cities × Stat::Module(crate::rules::stats::ClassicStat::HappinessPerCity)
//!             + natural wonders: 1 per one the player found × (100 + Stat %)
//!               + their own happiness inside the borders
//!             + World Congress project rewards (International Games)
//! unhappiness = [ Σ normal cities 300 × (100 + city-count %) × handicap %
//!                 × map size % (100 on every size)
//!               + Σ occupied cities 500 × the same + 200
//!               + Σ normal cities 100·pop × (100 + city pop %) then
//!                 × (100 + player pop %) × handicap population %
//!               + Σ occupied cities ⌊134·pop⌋ × handicap population % ] / 100
//!               × the AI's unhappiness percent for an AI
//! excess      = happiness − unhappiness
//! ```
//!
//! Effects (`Mood` of the excess at the start of the player's turn,
//! `PlayerEconomy::excess_happiness`; Unciv: GlobalUniques.json): unhappy
//! (−1 to −10): growth −75 %; very unhappy (below −10): growth −100 %,
//! production −50 %, combat strength −33 % (`combat_penalty_percent`, in
//! `effects::unit`), no settlers trained, no city founded; rebels below −20
//! (`uprising_possible`, barbarians are track C).
//!
//! Not yet counted (other tracks): religion, city-states, World Congress,
//! policies' happiness per X, trade route connections, units.

use crate::derived::resources::{from_city_states, resource_ledger};
use crate::effects::city::CityModifiers;
use crate::ids::PlayerId;
use crate::rules::content::ResourceClass;
use crate::rules::stats::{SocietyStat, Stat, StatTotals};
use crate::rules::CompiledRuleset;
use crate::world::city::CityCore;
use crate::world::state::AuthWorld;

/// Unhappiness of each normal city (hundredths). Unciv: CityStats.kt, equal (3).
pub const UNHAPPINESS_FROM_CITY_TIMES100: i64 = 300;
/// Unhappiness of each occupied city before the flat occupation term
/// (hundredths): 3 for a city and 2 more for an annexed one, scaled like a
/// city's. Unciv: CityStats.kt, equal (5).
pub const UNHAPPINESS_FROM_ANNEXED_CITY_TIMES100: i64 = 500;
/// Flat unhappiness of each occupied city, not scaled by any percent
/// (hundredths). Unciv: CityStats.kt ("Occupied City" -2), equal.
pub const OCCUPIED_CITY_UNHAPPINESS_TIMES100: i64 = 200;
/// Unhappiness of each citizen of a normal city (hundredths). Unciv: CityStats.kt, equal (1).
pub const UNHAPPINESS_FROM_CITIZENS_TIMES100: i64 = 100;
/// Unhappiness of each citizen of an occupied city (hundredths): twice a normal citizen's.
/// Unciv: CityStats.kt (`unhappinessFromCitizens *= 2` for an annexed city), equal.
pub const UNHAPPINESS_FROM_ANNEXED_CITIZENS_TIMES100: i64 = 200;
/// Happiness of each natural wonder the player found. Own value, unverified.
pub const NATURAL_WONDER_HAPPINESS: i32 = 1;
/// Excess happiness at or below which the empire is very unhappy (below
/// -10). Unciv: GlobalUniques.json (`when below [-10] [Happiness]`), equal.
pub const VERY_UNHAPPY_THRESHOLD: i32 = -11;
/// Production of the cities of a very unhappy empire (percent); no gold
/// penalty. Unciv: GlobalUniques.json, equal (-50).
pub const VERY_UNHAPPY_PRODUCTION_PERCENT: i32 = -50;
/// Combat strength of the units of a very unhappy empire (percent, flat).
/// Unciv: GlobalUniques.json, equal (-33).
pub const VERY_UNHAPPY_COMBAT_PERCENT: i32 = -33;
/// Excess happiness at or below which rebels may rise (below -20). Unciv:
/// GlobalUniques.json (`Rebel units may spawn <when below [-20] [Happiness]>`), equal.
pub const REBELLION_THRESHOLD: i32 = -21;
/// Growth modifier (percent) of an unhappy (-1 to -10) and of a very unhappy
/// empire (growth nullified). Unciv: GlobalUniques.json, equal.
pub const UNHAPPY_GROWTH_PERCENT: i32 = -75;
/// Unciv: GlobalUniques.json, equal.
pub const VERY_UNHAPPY_GROWTH_PERCENT: i32 = -100;

/// Where an empire's happiness comes from (whole points).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HappinessReport {
    pub handicap: i32,
    pub luxuries: i32,
    /// Kinds of luxuries the player has.
    pub luxury_kinds: u32,
    /// Local happiness of the cities, each capped at its population.
    pub local: i32,
    /// Player-wide happiness of buildings and wonders.
    pub buildings: i32,
    pub per_city: i32,
    pub natural_wonders: i32,
    /// Happiness World Congress projects gave (International Games).
    pub league: i32,
    /// Unhappiness of the people with the empire's ideology under foreign cultural pressure.
    pub from_opinion: i32,
    pub from_cities: i32,
    pub from_population: i32,
    pub from_occupied: i32,
    /// The unhappiness total, summed in hundredths before rounding down
    /// (the lines above are rounded each on its own).
    pub unhappiness_total: i32,
}

impl HappinessReport {
    pub fn happiness(&self) -> i32 {
        self.handicap
            + self.luxuries
            + self.local
            + self.buildings
            + self.per_city
            + self.natural_wonders
            + self.league
    }

    pub fn unhappiness(&self) -> i32 {
        self.unhappiness_total
    }

    pub fn excess(&self) -> i32 {
        self.happiness() - self.unhappiness()
    }
}

/// How the empire feels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mood {
    Content,
    /// Excess below 0.
    Unhappy,
    /// Excess at or below `VERY_UNHAPPY_THRESHOLD`.
    VeryUnhappy,
}

pub fn mood(excess: i32) -> Mood {
    if excess <= VERY_UNHAPPY_THRESHOLD {
        Mood::VeryUnhappy
    } else if excess < 0 {
        Mood::Unhappy
    } else {
        Mood::Content
    }
}

/// Growth modifier of the empire's mood (on a positive food surplus).
pub fn growth_percent(excess: i32) -> i32 {
    match mood(excess) {
        Mood::Content => 0,
        Mood::Unhappy => UNHAPPY_GROWTH_PERCENT,
        Mood::VeryUnhappy => VERY_UNHAPPY_GROWTH_PERCENT,
    }
}

/// Production modifier of cities (percent, ≤ 0): only a very unhappy
/// empire suffers (Unciv: GlobalUniques.json).
pub fn yield_penalty_percent(excess: i32) -> i32 {
    match mood(excess) {
        Mood::VeryUnhappy => VERY_UNHAPPY_PRODUCTION_PERCENT,
        _ => 0,
    }
}

/// Combat strength modifier of the player's units (percent, ≤ 0): flat for
/// a very unhappy empire (Unciv: GlobalUniques.json).
pub fn combat_penalty_percent(excess: i32) -> i32 {
    match mood(excess) {
        Mood::VeryUnhappy => VERY_UNHAPPY_COMBAT_PERCENT,
        _ => 0,
    }
}

/// Rebels may rise (Unciv: below -20 happiness).
pub fn uprising_possible(excess: i32) -> bool {
    excess <= REBELLION_THRESHOLD
}

/// The excess happiness the turn's effects read (snapshot taken at the
/// start of the player's last turn).
pub fn excess_snapshot(auth: &AuthWorld, player: PlayerId) -> i32 {
    auth.players()
        .economy_of(player)
        .map(|e| e.excess_happiness)
        .unwrap_or(0)
}

/// A city's own happiness before the cap, and after it.
pub fn city_local_happiness(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    core: &CityCore,
) -> (i32, i32) {
    let raw = crate::effects::city::city_modifiers(auth, rules, core).module.happiness;
    (raw, capped(raw, core))
}

/// Local happiness counts up to the city's population.
fn capped(raw: i32, core: &CityCore) -> i32 {
    raw.min(core.population as i32).max(0)
}

/// The empire's happiness now.
pub fn player_happiness(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    player: PlayerId,
) -> HappinessReport {
    let handicap = auth.settings().handicap(rules, player);
    let mods = crate::effects::player::player_modifiers(auth, rules, player);
    let ally_bonus = mods.module.city_state[crate::classic::effects::CityStateStat::AllyResourceBonus.index()] > 0;
    let (luxuries, luxury_kinds) = luxury_happiness(
        auth,
        rules,
        player,
        &mods.stats,
        handicap.module.extra_happiness_per_luxury,
        ally_bonus,
    );
    // Each city's modifiers once: its happiness and its unhappiness stats.
    let cities: Vec<(&CityCore, CityModifiers)> = auth
        .indexes()
        .cities_of(player)
        .iter()
        .filter_map(|c| auth.cities().get(*c))
        .map(|core| {
            (
                core,
                crate::effects::city::city_modifiers(auth, rules, core),
            )
        })
        .collect();
    let local = cities.iter().map(|(c, m)| capped(m.module.happiness, c)).sum();
    let mut report = HappinessReport {
        handicap: handicap.module.happiness_default,
        luxuries,
        luxury_kinds,
        local,
        buildings: mods.module.happiness,
        per_city: cities.len() as i32 * mods.stats.get(Stat::Module(crate::rules::stats::ClassicStat::HappinessPerCity)),
        natural_wonders: natural_wonder_happiness(auth, rules, player, &mods.stats),
        league: i32::from(auth.module().congress().rewards_of(player).happiness),
        ..HappinessReport::default()
    };
    unhappiness(auth, rules, player, &cities, &mods.stats, &mut report);
    report
}

/// Luxuries the player has a copy of left (its own, imported, from allied
/// city-states; exports lower it: rules.md), each kind once. A luxury
/// whose last copy is traded away keeps `LuxuryRetentionPercent` of its own
/// happiness (the Netherlands). With `ally_bonus` (Cultural Diplomacy) a
/// luxury that allied city-states give makes `ALLY_LUXURY_HAPPINESS_PERCENT`
/// percent of its happiness (unverified whether this scales the resource's
/// own happiness only, not the per-luxury extras).
fn luxury_happiness(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    player: PlayerId,
    stats: &StatTotals,
    extra: i32,
    ally_bonus: bool,
) -> (i32, u32) {
    let ledger = resource_ledger(auth, rules, player);
    let retention = stats.get(Stat::Module(crate::rules::stats::ClassicStat::Society(SocietyStat::LuxuryRetentionPercent))).max(0);
    let per_luxury = extra + stats.get(Stat::Module(crate::rules::stats::ClassicStat::HappinessPerLuxury));
    let from_allies = if ally_bonus {
        from_city_states(auth, rules, player)
    } else {
        Vec::new()
    };
    let mut total = 0;
    let mut kinds = 0;
    for (i, def) in rules.resources.iter().enumerate() {
        if def.class != Some(ResourceClass::Luxury) {
            continue;
        }
        let resource = crate::ids::ResourceId(i as u16);
        // A luxury the World Congress banned makes nobody happy.
        if crate::systems::congress::luxury_banned(auth, resource) {
            continue;
        }
        if ledger.available(resource) > 0 {
            kinds += 1;
            let allied = from_allies.get(i).is_some_and(|n| *n > 0);
            let own = if allied { def.module.happiness * ALLY_LUXURY_HAPPINESS_PERCENT / 100 } else { def.module.happiness };
            total += own + per_luxury;
        } else if retention > 0 && ledger.used(resource) > 0 {
            // Luxuries are used only by exports: the last copy left by deal.
            total += def.module.happiness * retention / 100 + per_luxury;
        }
    }
    (total, kinds)
}

/// Percent of a luxury's happiness when allied city-states give it and the player
/// has Cultural Diplomacy. Own value, unverified.
pub const ALLY_LUXURY_HAPPINESS_PERCENT: i32 = 150;

/// The happiness one more found natural wonder adds to `player`: `NATURAL_WONDER_HAPPINESS` × the
/// stat percent (the wonders' own happiness inside the borders is not a find's).
pub fn natural_wonder_find_happiness(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId) -> i32 {
    let mods = crate::effects::player::player_modifiers(auth, rules, player);
    NATURAL_WONDER_HAPPINESS * (100 + mods.stats.get(Stat::Module(crate::rules::stats::ClassicStat::NaturalWonderHappinessPercent))) / 100
}

/// 1 per natural wonder the player found (× the stat percent), plus the
/// wonders' own happiness inside the player's borders.
fn natural_wonder_happiness(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    player: PlayerId,
    stats: &StatTotals,
) -> i32 {
    let map = auth.map();
    let Some(knowledge) = auth.knowledge().get(player.0 as usize) else {
        return 0;
    };
    let mut found = 0;
    let mut own = 0;
    for &tile in map.natural_wonder_tiles() {
        let Some(wonder) = map.natural_wonder(tile) else {
            continue;
        };
        if knowledge.is_explored(tile) {
            found += NATURAL_WONDER_HAPPINESS;
        }
        if map.owner(tile).flatten() == Some(player) {
            own += rules
                .natural_wonders
                .get(wonder.0 as usize)
                .map(|w| w.module.happiness)
                .unwrap_or(0);
        }
    }
    found * (100 + stats.get(Stat::Module(crate::rules::stats::ClassicStat::NaturalWonderHappinessPercent))) / 100 + own
}

/// Unhappiness of a set of cities in hundredths, before the AI percentage:
/// (city count, population, occupied cities with their population).
#[derive(Debug, Clone, Copy, Default)]
struct Share {
    count: i64,
    population: i64,
    occupied: i64,
}

/// The percentages of the player and its handicap (percent each).
struct Percents {
    city_count: i64,
    handicap_cities: i64,
    /// The map size's percent of the city count (Large 80, Huge 60).
    world_cities: i64,
    population: i64,
    handicap_population: i64,
}

fn percents(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    player: PlayerId,
    stats: &StatTotals,
) -> Percents {
    let handicap = auth.settings().handicap(rules, player);
    Percents {
        city_count: i64::from(100 + stats.get(Stat::Module(crate::rules::stats::ClassicStat::CityCountUnhappinessPercent))).max(0),
        handicap_cities: i64::from(handicap.module.num_cities_unhappiness_mod),
        world_cities: crate::classic::world_size::city_count_unhappiness_percent(crate::world::size::WorldSize::of(auth.map().dimensions)),
        population: i64::from(100 + stats.get(Stat::Module(crate::rules::stats::ClassicStat::PopulationUnhappinessPercent))).max(0),
        handicap_population: i64::from(handicap.module.population_unhappiness_mod),
    }
}

/// Citizens of a city that count for its unhappiness: specialists count
/// `SpecialistUnhappinessPercent` less (Universal Suffrage −50:
/// `pop − (specialists + 1) / 2`, rounded in the city's favour).
fn unhappy_citizens(auth: &AuthWorld, rules: &CompiledRuleset, core: &CityCore, city_stats: &StatTotals) -> i64 {
    let pop = i64::from(core.population);
    let percent = i64::from(city_stats.get(Stat::Module(crate::rules::stats::ClassicStat::Society(SocietyStat::SpecialistUnhappinessPercent))));
    if percent >= 0 {
        return pop;
    }
    let specialists = i64::from(crate::derived::yields::city_work(auth, rules, core).specialist_count());
    let spared = (specialists * -percent + 99) / 100;
    (pop - spared).max(0)
}

/// Annexed without a courthouse, or burning: occupied (track C's conquest
/// status, unless the city is exempt from occupied unhappiness).
fn occupied(core: &CityCore, city_stats: &StatTotals) -> bool {
    use crate::world::city::CityStatus;
    match core.status {
        CityStatus::Razing => true,
        CityStatus::Occupied => city_stats.get(Stat::Module(crate::rules::stats::ClassicStat::Society(SocietyStat::NoOccupiedUnhappiness))) <= 0,
        _ => false,
    }
}

/// One city's share (rules.md: the player's population percent and
/// the handicap apply to the sum; a city's own percent to its citizens).
/// `pop`: its citizens that count (`unhappy_citizens`).
fn city_share(core: &CityCore, pop: i64, city_stats: &StatTotals, p: &Percents) -> Share {
    let count = |base: i64| base * p.city_count / 100 * p.handicap_cities / 100 * p.world_cities / 100;
    if occupied(core, city_stats) {
        let people =
            pop * UNHAPPINESS_FROM_ANNEXED_CITIZENS_TIMES100 * p.handicap_population / 100;
        return Share {
            occupied: count(UNHAPPINESS_FROM_ANNEXED_CITY_TIMES100) + OCCUPIED_CITY_UNHAPPINESS_TIMES100 + people,
            ..Share::default()
        };
    }
    let own = i64::from(100 + city_stats.get(Stat::Module(crate::rules::stats::ClassicStat::PopulationUnhappinessPercent))).max(0);
    let people = pop * UNHAPPINESS_FROM_CITIZENS_TIMES100 * own / 100;
    Share {
        count: count(UNHAPPINESS_FROM_CITY_TIMES100),
        population: people * p.population / 100 * p.handicap_population / 100,
        occupied: 0,
    }
}

/// Unhappiness one city brings its empire (whole points, before the AI
/// percentage; the city screen shows it).
pub fn city_unhappiness(auth: &AuthWorld, rules: &CompiledRuleset, core: &CityCore) -> i32 {
    let stats = &crate::effects::player::player_modifiers(auth, rules, core.owner).stats;
    let city_stats = crate::effects::city::city_modifiers(auth, rules, core).stats;
    let pop = unhappy_citizens(auth, rules, core, &city_stats);
    let share = city_share(
        core,
        pop,
        &city_stats,
        &percents(auth, rules, core.owner, &stats),
    );
    ((share.count + share.population + share.occupied) / 100) as i32
}

/// The unhappiness lines of the report (see the module docs).
fn unhappiness(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    player: PlayerId,
    cities: &[(&CityCore, CityModifiers)],
    stats: &StatTotals,
    report: &mut HappinessReport,
) {
    let p = percents(auth, rules, player, stats);
    let mut sum = Share::default();
    for (core, mods) in cities {
        let pop = unhappy_citizens(auth, rules, core, &mods.stats);
        let share = city_share(core, pop, &mods.stats, &p);
        sum.count += share.count;
        sum.population += share.population;
        sum.occupied += share.occupied;
    }
    let ai_percent = if auth.settings().is_ai(player) {
        i64::from(
            auth.settings()
                .handicap(rules, player)
                .module.ai_unhappiness_percent,
        )
    } else {
        100
    };
    let whole = |v: i64| (v * ai_percent / 100 / 100) as i32;
    report.from_cities = whole(sum.count);
    report.from_population = whole(sum.population);
    report.from_occupied = whole(sum.occupied);
    report.from_opinion = crate::derived::tourism::public_opinion_unhappiness(auth, player) as i32;
    report.unhappiness_total = whole(sum.count + sum.population + sum.occupied) + report.from_opinion;
}

pub use crate::systems::golden_age::{golden_age_length, golden_age_threshold, UNMODIFIED_GOLDEN_AGE_TURNS};
