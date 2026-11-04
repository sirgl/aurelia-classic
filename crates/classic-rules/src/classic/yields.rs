//! The classic terms of the city yields (`kernel::derived::yields::YieldTerms`):
//! the golden age and the unhappiness of the empire, landmarks, great
//! works, wonders' culture, maritime city-states, science per citizen and
//! trade routes.

use kernel::derived::yields::{add_once, YieldTerms};

use crate::classic::effects::{ClassicModifier, ClassicSelector};
use crate::classic::{Classic, ClassicChange};
use crate::effects::city::CityModifiers;
use crate::ids::{CityHandle, PlayerId, TileId};
use crate::rules::content::Yields;
use crate::rules::effects::{Modifier, Selector, YieldKind};
use crate::rules::CompiledRuleset;
use crate::transaction::journal::{ChangeJournal, CityChange};
use crate::world::city::CityCore;
use crate::world::state::AuthWorld;

/// Science of every citizen. Unciv: CityStats.kt (population stats), equal.
pub const SCIENCE_PER_CITIZEN: i32 = 1;
/// Percent production bonus of a city during a golden age (classic/docs/classic/terrain.json
/// `golden_age_city_pct`). Unciv: CityStats.kt (golden age percent bonuses), equal.
pub const GOLDEN_AGE_PRODUCTION_PERCENT: i32 = 20;
/// Golden age: a tile with at least this much gold gets `GOLDEN_AGE_TILE_GOLD`
/// more. Unciv: TileStatFunctions.kt (a tile with gold gets +1), equal.
pub const GOLDEN_AGE_TILE_GOLD_THRESHOLD: i32 = 1;
/// Unciv: TileStatFunctions.kt, equal.
pub const GOLDEN_AGE_TILE_GOLD: i32 = 1;

/// The classic yield terms.
#[derive(Debug, Clone, Copy)]
pub struct ClassicYields;

/// What the city yields read of the owner's empire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ClassicEmpire {
    /// Golden age: tiles with at least 1 gold get +1, production +20 %.
    pub golden_age: bool,
    /// Production and gold percent of an unhappy empire (≤ 0, from the
    /// turn's happiness snapshot).
    pub unhappiness_percent: i32,
}

impl YieldTerms<Classic> for ClassicYields {
    type Empire = ClassicEmpire;

    fn empire(auth: &AuthWorld, player: PlayerId) -> ClassicEmpire {
        let excess = crate::derived::happiness::excess_snapshot(auth, player);
        ClassicEmpire {
            golden_age: auth.players().economy_of(player).is_some_and(|e| e.golden_age_turns > 0),
            unhappiness_percent: crate::derived::happiness::yield_penalty_percent(excess),
        }
    }

    fn tile_base(auth: &AuthWorld, rules: &CompiledRuleset, core: &CityCore, tile: TileId) -> Yields {
        Yields { culture: landmark_era_culture(auth, rules, core, tile), ..Yields::default() }
    }

    fn tile_final(empire: &ClassicEmpire, yields: &mut Yields) {
        if empire.golden_age && yields.gold >= GOLDEN_AGE_TILE_GOLD_THRESHOLD {
            yields.gold += GOLDEN_AGE_TILE_GOLD;
        }
    }

    /// Maritime city-states feed their friends' and allies' cities (§4.6);
    /// every citizen makes science; great works and themes, and the
    /// wonders' culture bonus, add culture.
    fn city_base(auth: &AuthWorld, rules: &CompiledRuleset, core: &CityCore, modifiers: &CityModifiers) -> Yields {
        let mut base = Yields::default();
        base.food += crate::systems::city_states::city_food_bonus(auth, rules, core);
        base.science += SCIENCE_PER_CITIZEN * core.population as i32;
        let great_work_culture = match auth.indexes().city_on_tile(core.tile) {
            Some(city) => crate::derived::tourism::great_work_culture(auth, rules, city, core, modifiers),
            None => 0,
        };
        base.culture += great_work_culture.min(i32::MAX as u32) as i32;
        base.culture += wonder_culture_bonus(rules, core, modifiers);
        base
    }

    fn channel_percent(empire: &ClassicEmpire, kind: YieldKind) -> i32 {
        match kind {
            YieldKind::Production if empire.golden_age => GOLDEN_AGE_PRODUCTION_PERCENT + empire.unhappiness_percent,
            // Unciv: GlobalUniques.json, production only (no gold penalty).
            YieldKind::Production => empire.unhappiness_percent,
            _ => 0,
        }
    }

    /// Trade routes add after the city's percents (rules "Trade routes"
    /// 3.6, 3.8): food before what the citizens eat.
    fn city_after(auth: &AuthWorld, city: CityHandle) -> Yields {
        let trade = crate::derived::trade::city_trade_yields(auth, city);
        let part = |v: u32| v.min(i32::MAX as u32) as i32;
        Yields {
            food: part(trade.food),
            production: part(trade.production),
            gold: part(trade.gold),
            science: part(trade.science),
            ..Yields::default()
        }
    }

    /// Founder beliefs count the followers of a religion anywhere (Church
    /// Property, Tithe, ...): a city changing faith moves its founder's
    /// yields. A city that grows or shrinks changes its followers of every
    /// religion with pressure there: their founders too.
    fn stale_players(changes: &ChangeJournal, auth: &AuthWorld, rules: &CompiledRuleset, out: &mut Vec<PlayerId>) {
        let religion_moved = changes
            .module_changes
            .iter()
            .any(|c| matches!(c, ClassicChange::ReligionChanged { .. }));
        if religion_moved {
            for religion in auth.module().religion().religions() {
                add_once(out, religion.founder);
            }
        }
        for cc in &changes.city_changes {
            if let CityChange::PopulationChanged { city } = cc {
                for founder in founders_present(auth, rules, *city) {
                    add_once(out, founder);
                }
            }
        }
    }
}

/// The per-era culture of a Landmark on `tile`: one per era between the
/// tile's history and the era of the working city's owner. The improvement's
/// own yield is in the plot.
pub(crate) fn landmark_era_culture(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    core: &CityCore,
    tile: TileId,
) -> i32 {
    let Some(imp) = auth.map().working_improvement(tile) else {
        return 0;
    };
    let per_era = rules.improvements.get(imp.0 as usize).map_or(0, |d| d.culture_per_era);
    if per_era == 0 {
        return 0;
    }
    let Some(era) = auth.module().tourism().archaeology().landmark_era(tile) else {
        return 0;
    };
    let now = crate::world::player::player_era(auth, rules, core.owner);
    i32::from(per_era) * i32::from(now.saturating_sub(era))
}

/// Flourishing of the Arts: the percent of the city's
/// `WonderCulturePercent` of the flat culture its world wonders give the
/// city (added to the building's culture when it is processed).
fn wonder_culture_bonus(rules: &CompiledRuleset, core: &CityCore, modifiers: &CityModifiers) -> i32 {
    use crate::rules::effects::{Condition, Modifier, Selector};
    use crate::rules::stats::{SocietyStat, Stat};
    let percent = modifiers.stats.get(Stat::Module(crate::rules::stats::ClassicStat::Society(SocietyStat::WonderCulturePercent)));
    if percent == 0 {
        return 0;
    }
    let mut culture = 0;
    for building in &core.buildings {
        let Some(def) = rules.buildings.get(building.0 as usize) else {
            continue;
        };
        if !def.wonder {
            continue;
        }
        for effect in &def.effects.passive_city {
            let own = effect.selector == Selector::SourceCity && effect.condition == Condition::Always;
            if let (true, Modifier::AddYield { yield_kind: YieldKind::Culture, amount }) = (own, effect.modifier) {
                culture += amount;
            }
        }
    }
    culture * percent / 100
}


/// Founders of the religions with pressure in the city whose holy city
/// counts followers elsewhere (the only city yields that read another
/// city's followers).
fn founders_present(auth: &AuthWorld, rules: &CompiledRuleset, city: CityHandle) -> Vec<PlayerId> {
    let mut out = Vec::new();
    for p in &auth.module().religion().city(city).pressure {
        let crate::world::religion::Creed::Religion(r) = p.creed else {
            continue;
        };
        let Some(founded) = auth.module().religion().religion(r) else {
            continue;
        };
        if holy_city_counts_followers(rules, &founded.beliefs) {
            add_once(&mut out, founded.founder);
        }
    }
    out
}

/// A belief gives its holy city a yield per follower or per following city
/// (World Church, Pilgrimage).
fn holy_city_counts_followers(rules: &CompiledRuleset, beliefs: &[crate::ids::BeliefId]) -> bool {
    beliefs.iter().any(|b| {
        rules.module.beliefs[b.0 as usize].effects.passive_city.iter().any(|e| {
            e.selector == Selector::Module(ClassicSelector::HolyCity)
                && matches!(
                    e.modifier,
                    Modifier::Module(ClassicModifier::AddYieldPerFollowers { .. }) | Modifier::Module(ClassicModifier::AddYieldPerFollowingCity { .. })
                )
        })
    })
}

