//! Gifts, bullying and pledges (§4.10–4.13): gold for influence, units,
//! improvements of the city-state's resources, tribute of gold or a
//! worker, the pledge to protect.

use super::block::CityStateBlock;
use super::effective_influence;
use crate::command::CommandError;
use crate::ids::{PlayerId, TileId, UnitHandle, UnitKindId};
use crate::rules::content::{Domain, UnitRole};
use crate::classic::effects::CityStateStat;
use crate::rules::CompiledRuleset;
use crate::transaction::transaction::WorldTransaction;
use crate::world::city_states::Personality;
use crate::world::state::AuthWorld;
use crate::classic::content::RELIGIOUS_ROLE;

/// Exponent and divisor of the influence a gold gift gives:
/// `gold^1.01 / 9.8`. Unciv: logic/nation/diplomacy/
/// CityStateFunctions.kt (`influenceGainedByGift`); equal.
pub const GOLD_GIFT_EXPONENT: f64 = 1.01;
/// Unciv: CityStateFunctions.kt, equal.
pub const GOLD_GIFT_DIVISOR: f64 = 9.8;
/// Rounds of the approximate game progress that shrinks a gold gift by up
/// to two thirds (Standard speed). Unciv: CityStateFunctions.kt
/// (`turns / (400 × speed)`); equal.
pub const ESTIMATED_END_ROUND: u32 = 400;
/// Least influence a gold gift gives, and the step the gift is rounded
/// down to. Unciv: CityStateFunctions.kt (rounded down to 5, at least 5);
/// equal.
pub const GOLD_GIFT_MINIMUM: i32 = 5;
/// Unciv: CityStateFunctions.kt, equal.
pub const VISIBLE_DIVISOR: i32 = 5;
/// Influence given by a gifted unit. Unciv: logic/map/mapunit/actions/
/// UnitActions.kt (`getGiftActions`, +5); equal.
pub const UNIT_GIFT_INFLUENCE: i32 = 5;
/// Gold cost of gifting a tile improvement. Own value (no Unciv
/// equivalent: Unciv has no improvement gifts).
pub const IMPROVEMENT_GIFT_COST: u32 = 200;
/// Gold a bully takes: the base, plus `BULLY_GOLD_GROWTH` for every
/// `BULLY_GOLD_INTERVAL_TIMES10` / 10 rounds (Standard speed). Unciv:
/// CityStateFunctions.kt (`goldGainedByTribute`: 50 + 5 per 6.5 turns,
/// Speeds.json `cityStateTributeScalingInterval`); equal.
pub const BULLY_GOLD_BASE: u32 = 50;
/// Unciv: CityStateFunctions.kt, equal.
pub const BULLY_GOLD_GROWTH: u32 = 5;
/// Unciv: CityStateFunctions.kt, equal.
pub const BULLY_GOLD_INTERVAL_TIMES10: u32 = 65;
// The Standard speed's tribute interval is this constant.
const _: () = assert!(BULLY_GOLD_INTERVAL_TIMES10 == kernel::rules::speeds::SpeedValues::STANDARD.tribute_interval_times10);
/// Influence below which a city-state cannot be bullied (-300 to the
/// score). Unciv: CityStateFunctions.kt (`getTributeModifiers`, -30);
/// equal.
pub const CAN_BULLY_THRESHOLD: i32 = -30;
/// Influence lost by a successful bullying for gold, and for a worker
/// (×100). Unciv: CityStateFunctions.kt (`tributeGold` -15,
/// `tributeWorker` -50); equal.
pub const BULLY_GOLD_DROP: i32 = -1500;
/// Unciv: CityStateFunctions.kt, equal.
pub const BULLY_WORKER_DROP: i32 = -5000;
/// Influence needed to pledge to protect; rounds after a withdrawn or
/// broken pledge before a new one, rounds before a pledge may be
/// withdrawn. Unciv: logic/nation/diplomacy/CityStateFunctions.kt
/// (influence at least 0, RecentlyWithdrewProtection 20 turns,
/// RecentlyPledgedProtection 10 turns); equal.
pub const CAN_PLEDGE_THRESHOLD: i32 = 0;
/// Unciv: CityStateFunctions.kt, equal.
pub const PLEDGE_AFTER_BROKEN: u32 = 20;
/// Unciv: CityStateFunctions.kt, equal.
pub const PLEDGE_MIN_ROUNDS: u32 = 10;
/// Influence (×100) a major loses when it withdraws its pledge. Unciv:
/// CityStateFunctions.kt (`removeProtectorCiv`, -20); equal.
pub const PLEDGE_WITHDRAW_DROP: i32 = -2000;

/// Influence of a gold gift (§4.10) in points, without the giver's
/// policies and beliefs.
pub fn gold_gift_influence(round: u32, gold: u32, invest: bool) -> i32 {
    gold_gift_influence_with(round, gold, invest, 0)
}

/// Influence of a gold gift with `percent` more from the giver's policies
/// and beliefs (Philanthropy +25, Charitable Missions +30), at Standard speed.
pub fn gold_gift_influence_with(round: u32, gold: u32, invest: bool, percent: i32) -> i32 {
    gold_gift_influence_at(&kernel::rules::speeds::SpeedValues::STANDARD, round, gold, invest, percent)
}

/// `gold_gift_influence_with` at a game speed: the game progress counts
/// `ESTIMATED_END_ROUND` × the speed's pace, the influence × the speed's
/// gold gift percent. Unciv: CityStateFunctions.influenceGainedByGift
/// (`400 × speed.modifier`, `goldGiftModifier`); equal.
pub fn gold_gift_influence_at(
    speed: &kernel::rules::speeds::SpeedValues,
    round: u32,
    gold: u32,
    invest: bool,
    percent: i32,
) -> i32 {
    // Unciv's shape: everything in floating point, rounded down to 5 once
    // at the end, at least 5.
    let mut f = f64::from(gold).powf(GOLD_GIFT_EXPONENT) / GOLD_GIFT_DIVISOR;
    let end = if speed.pace_percent == 100 {
        f64::from(ESTIMATED_END_ROUND)
    } else {
        f64::from(ESTIMATED_END_ROUND) * f64::from(speed.pace_percent) / 100.0
    };
    let progress = (f64::from(round) / end).min(1.0);
    f *= 1.0 - progress * 2.0 / 3.0;
    if speed.gold_gift_percent != 100 {
        f *= f64::from(speed.gold_gift_percent) / 100.0;
    }
    if percent != 0 {
        f *= f64::from(100 + percent) / 100.0;
    }
    if invest {
        f *= 1.5;
    }
    let step = f64::from(VISIBLE_DIVISOR);
    let f = (f - f % step) as i32;
    f.max(GOLD_GIFT_MINIMUM)
}

/// Gold of a successful tribute (§4.12.2) at Standard speed.
pub fn bully_gold_amount(round: u32) -> u32 {
    bully_gold_at(&kernel::rules::speeds::SpeedValues::STANDARD, round)
}

/// Gold of a successful tribute at a game speed: the base × the speed's
/// gold gift percent (rounded down to 5), plus `BULLY_GOLD_GROWTH` per the
/// speed's tribute interval. Unciv: CityStateFunctions.goldGainedByTribute
/// (`(10 × goldGiftModifier).toInt() × 5`, `cityStateTributeScalingInterval`);
/// equal: Quick 60, Standard 50, Epic 35, Marathon 30 at round 0.
pub fn bully_gold_at(speed: &kernel::rules::speeds::SpeedValues, round: u32) -> u32 {
    let base = BULLY_GOLD_BASE / VISIBLE_DIVISOR as u32 * speed.gold_gift_percent / 100 * VISIBLE_DIVISOR as u32;
    base + BULLY_GOLD_GROWTH * (round * 10 / speed.tribute_interval_times10.max(1))
}

/// `x^1.5` for powers, exact in integers.
fn power_of(strength: i64) -> i64 {
    let s = strength.max(0);
    s * ((s * 10_000) as f64).sqrt() as i64 / 100
}

/// Unit power (rules.md): strength^1.5 scaled by health [approximation: the
/// ranged exponent 1.45 and promotion terms are left out].
fn unit_power(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    unit: &crate::world::unit::UnitCore,
) -> i64 {
    let Some(def) = rules.units.get(unit.kind.0 as usize) else {
        return 0;
    };
    let strength = i64::from(def.strength.max(def.ranged_strength));
    let _ = auth;
    power_of(strength) * i64::from(unit.hp) / i64::from(crate::world::unit::UNIT_MAX_HP)
}

/// Units of `owner` within `radius` of `center` (combat units only).
fn power_near(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    owner: PlayerId,
    center: TileId,
    radius: i32,
) -> i64 {
    let dims = auth.map().dimensions;
    auth.indexes()
        .units_of(owner)
        .iter()
        .filter_map(|u| auth.units().get(*u))
        .filter(|u| !rules.is_civilian(u.core.kind))
        .filter(|u| {
            dims.distance(u.core.tile, center)
                .is_some_and(|d| d <= radius)
        })
        .map(|u| unit_power(auth, rules, u.core))
        .sum()
}

/// Military might: unit power (ships half) times the treasury factor.
pub fn military_might(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId) -> i64 {
    let units: i64 = auth
        .indexes()
        .units_of(player)
        .iter()
        .filter_map(|u| auth.units().get(*u))
        .map(|u| {
            let p = unit_power(auth, rules, u.core);
            let naval = rules
                .units
                .get(u.core.kind.0 as usize)
                .is_some_and(|d| d.domain == Domain::Sea);
            if naval {
                p / 2
            } else {
                p
            }
        })
        .sum();
    let gold = f64::from(auth.players().treasury_of(player));
    let factor = (1.0 + gold.sqrt() / 100.0).min(2.0);
    (units as f64 * factor) as i64
}

/// The global part of the bully score (0..100): rank of military might.
/// Unciv: models/ModConstants.kt (`tributeGlobalModifier` 100) and
/// CityStateFunctions.kt (`(alive - rank) × 100 / alive`); equal.
fn global_score(auth: &AuthWorld, rules: &CompiledRuleset, major: PlayerId) -> i32 {
    let majors = super::majors(auth);
    let mine = military_might(auth, rules, major);
    let rank = majors
        .iter()
        .filter(|p| **p != major && military_might(auth, rules, **p) > mine)
        .count() as i32;
    let n = majors.len().max(1) as i32;
    (n - rank) * TRIBUTE_GLOBAL_MODIFIER / n
}

/// Highest global and local parts of the bully score. Unciv:
/// models/ModConstants.kt (`tributeGlobalModifier`,
/// `tributeLocalModifier`, both 100); equal.
pub const TRIBUTE_GLOBAL_MODIFIER: i32 = 100;
/// Unciv: models/ModConstants.kt (`tributeLocalModifier` 100), equal.
pub const TRIBUTE_LOCAL_MODIFIER: i32 = 100;

/// The local part (0..100): the bully's units near the capital against
/// the city-state's capital and units, within `width / 10` tiles clamped to
/// 5..10; the ratio must exceed 3, 2, 1.5, 1, 0.5 for 100, 80, 60, 40, 20.
/// Unciv: CityStateFunctions.kt (`getTributeModifiers`); equal.
fn local_score(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    city_state: PlayerId,
    major: PlayerId,
) -> i32 {
    let Some(capital) =
        crate::combat::city::capital_of(auth, rules, city_state).and_then(|c| auth.cities().get(c))
    else {
        return 0;
    };
    let radius = (auth.map().dimensions.width as i32 / 10).clamp(5, 10);
    let city = power_of(crate::combat::city::city_strength(auth, rules, capital) / 100);
    let minor = (city + power_near(auth, rules, city_state, capital.tile, radius)).max(1);
    let bully = power_near(auth, rules, major, capital.tile, radius);
    let l = TRIBUTE_LOCAL_MODIFIER;
    match () {
        _ if bully > 3 * minor => l,
        _ if bully > 2 * minor => l * 4 / 5,
        _ if 2 * bully > 3 * minor => l * 3 / 5,
        _ if bully > minor => l * 2 / 5,
        _ if 2 * bully > minor => l / 5,
        _ => 0,
    }
}

/// Bully score (§4.12.1): tribute succeeds when ≥ 0.
pub fn bully_score(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    city_state: PlayerId,
    major: PlayerId,
    for_unit: bool,
) -> i32 {
    let power = global_score(auth, rules, major) + local_score(auth, rules, city_state, major);
    let percent = super::stat(auth, rules, major, CityStateStat::BullyScorePercent);
    let mut score = power + power * percent / 100 - 110;
    if effective_influence(auth, city_state, major) < CAN_BULLY_THRESHOLD {
        score -= 300;
    }
    score += recent_bully_penalty(auth, city_state, major);
    if for_unit {
        score -= 30 + small_capital_penalty(auth, rules, city_state);
    }
    score += standing_penalties(auth, rules, city_state, major);
    score
}

fn recent_bully_penalty(auth: &AuthWorld, city_state: PlayerId, major: PlayerId) -> i32 {
    let round = auth.turn().round;
    match auth.module().city_states().standing(city_state, major).last_bullied {
        Some(last) if last + 10 >= round => -300,
        Some(last) if last + 20 >= round => -40,
        _ => 0,
    }
}

fn small_capital_penalty(auth: &AuthWorld, rules: &CompiledRuleset, city_state: PlayerId) -> i32 {
    let population = crate::combat::city::capital_of(auth, rules, city_state)
        .and_then(|c| auth.cities().get(c))
        .map(|c| c.population)
        .unwrap_or(0);
    if population < 4 {
        300
    } else {
        0
    }
}

/// Another ally, pledges of others, a hostile city-state. Unciv:
/// CityStateFunctions.kt (-10, -20, -10; no Militaristic term); equal.
fn standing_penalties(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    city_state: PlayerId,
    major: PlayerId,
) -> i32 {
    let states = auth.module().city_states();
    let mut score = 0;
    if states.ally(city_state).is_some_and(|a| a != major) {
        score -= 10;
    }
    let pledged = super::majors(auth)
        .into_iter()
        .any(|p| p != major && states.standing(city_state, p).pledged.is_some());
    if pledged {
        score -= 20;
    }
    if states.minor(city_state).personality == Personality::Hostile {
        score -= 10;
    }
    let _ = rules;
    score
}

/// May the unit be given away (§4.11): a combat unit, not a scout.
pub fn giftable_unit(rules: &CompiledRuleset, kind: UnitKindId) -> bool {
    rules.units.get(kind.0 as usize).is_some_and(|d| {
        let combat = d.strength > 0 || d.ranged_strength > 0;
        combat && !d.founder && !matches!(d.role, Some(UnitRole::Recon | RELIGIOUS_ROLE))
    })
}

/// Influence a great person of `kind` brings when `owner` gifts it (0:
/// not a great person, or no such effect).
pub fn great_person_gift_influence(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    owner: PlayerId,
    kind: UnitKindId,
) -> i32 {
    if crate::derived::great_people::great_person_of(rules, kind).is_none() {
        return 0;
    }
    super::stat(auth, rules, owner, CityStateStat::GreatPersonGiftInfluence).max(0)
}

/// The worker a tribute brings: the first regular builder unit.
pub fn worker_kind(rules: &CompiledRuleset) -> Option<UnitKindId> {
    rules
        .units
        .iter()
        .position(|d| d.builder && !d.founder && d.nation.is_none() && !d.unbuildable)
        .map(|i| UnitKindId(i as u16))
}

impl<'a> WorldTransaction<'a> {
    pub(crate) fn gift_gold(
        &mut self,
        major: PlayerId,
        city_state: PlayerId,
        gold: u32,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        if !self.world.players.spend(major, gold) {
            return Err(CityStateBlock::NotEnoughGold.into());
        }
        if let Some(record) = self.world.module.city_states.major_mut(major) {
            record.gold_gifted = record.gold_gifted.saturating_add(gold);
        }
        let invest = self.world.module.city_states.quests().iter().any(|q| {
            q.city_state == city_state
                && q.major == major
                && q.kind == crate::world::city_states::QuestKind::Invest
        });
        let percent = super::stat(self.world, rules, major, CityStateStat::GoldGiftPercent);
        let speed = self.world.settings().speed(rules);
        let influence = gold_gift_influence_at(&speed, self.world.turn.round, gold, invest, percent);
        self.change_influence(city_state, major, influence * 100, rules);
        self.check_quests(city_state, rules);
        Ok(())
    }

    /// The unit joins the city-state; +5 influence. A great person is used
    /// up for the giver's great person gift influence instead (Sweden).
    pub(crate) fn gift_unit(
        &mut self,
        unit: UnitHandle,
        city_state: PlayerId,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        let (owner, kind) = self
            .world
            .units
            .get(unit)
            .map(|v| (v.core.owner, v.core.kind))
            .ok_or(CommandError::UnknownUnit)?;
        let great_person = great_person_gift_influence(self.world, rules, owner, kind);
        if great_person > 0 {
            self.destroy_unit(unit)?;
            self.change_influence(city_state, owner, great_person * 100, rules);
            return Ok(());
        }
        let core = self.destroy_unit(unit)?;
        let tile = self.free_tile_near(city_state, core.tile, core.kind, rules);
        if crate::world::stacking::can_stop_at(self.world, rules, city_state, core.kind, None, tile)
            .is_ok()
        {
            self.spawn_unit(city_state, tile, core.kind, rules);
        }
        let extra = super::stat(self.world, rules, core.owner, CityStateStat::UnitGiftInfluence).max(0);
        self.change_influence(city_state, core.owner, (UNIT_GIFT_INFLUENCE + extra) * 100, rules);
        Ok(())
    }

    /// The improvement its resource needs, built on the city-state's tile.
    pub(crate) fn gift_improvement(
        &mut self,
        major: PlayerId,
        tile: TileId,
        improvement: crate::ids::ImprovementId,
    ) -> Result<(), CommandError> {
        if !self.world.players.spend(major, IMPROVEMENT_GIFT_COST) {
            return Err(CityStateBlock::NotEnoughGold.into());
        }
        self.world.map.set_improvement(tile, Some(improvement));
        self.journal
            .tile_changes
            .push(crate::transaction::journal::TileChange::ImprovementBuilt { tile });
        Ok(())
    }

    pub(crate) fn bully_gold(
        &mut self,
        major: PlayerId,
        city_state: PlayerId,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        let gold = bully_gold_at(&self.world.settings().speed(rules), self.world.turn.round);
        self.world.players.earn(major, gold);
        self.bullied(city_state, major, BULLY_GOLD_DROP, rules);
        Ok(())
    }

    pub(crate) fn bully_worker(
        &mut self,
        major: PlayerId,
        city_state: PlayerId,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        let (Some(kind), Some(capital)) = (
            worker_kind(rules),
            crate::combat::city::capital_of(self.world, rules, city_state)
                .and_then(|c| self.world.cities.get(c))
                .map(|c| c.tile),
        ) else {
            return Err(CityStateBlock::NoWorker.into());
        };
        let tile = self.free_tile_near(major, capital, kind, rules);
        if crate::world::stacking::can_stop_at(self.world, rules, major, kind, None, tile).is_err()
        {
            return Err(CityStateBlock::NoRoomForWorker.into());
        }
        let worker = self.spawn_unit(major, tile, kind, rules);
        if let Some(core) = self.world.units.get_mut_core(worker) {
            core.moves_left = 0;
        }
        self.bullied(city_state, major, BULLY_WORKER_DROP, rules);
        Ok(())
    }

    /// After a bullying: the round is remembered, influence drops,
    /// quests are checked (bully quests elsewhere complete, revoked ones end).
    fn bullied(
        &mut self,
        city_state: PlayerId,
        major: PlayerId,
        drop: i32,
        rules: &CompiledRuleset,
    ) {
        let round = self.world.turn.round;
        if let Some(s) = self.world.module.city_states.standing_mut(city_state, major) {
            s.last_bullied = Some(round);
        }
        self.change_influence(city_state, major, drop, rules);
        for cs in super::city_states(self.world) {
            self.check_quests(cs, rules);
        }
    }

    pub(crate) fn pledge(
        &mut self,
        major: PlayerId,
        city_state: PlayerId,
        pledge: bool,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        let round = self.world.turn.round;
        if let Some(s) = self.world.module.city_states.standing_mut(city_state, major) {
            s.pledged = pledge.then_some(round);
            if !pledge {
                s.pledge_broken = Some(round);
            }
        }
        if !pledge {
            // Withdrawal costs influence and blocks a new pledge for
            // `PLEDGE_AFTER_BROKEN` rounds (Unciv: CityStateFunctions.kt).
            self.change_influence(city_state, major, PLEDGE_WITHDRAW_DROP, rules);
        }
        self.relations_changed(major);
        self.check_quests(city_state, rules);
        Ok(())
    }
}

/// Why `major` may not pledge (or withdraw) now (None: it may).
pub fn pledge_problem(
    auth: &AuthWorld,
    city_state: PlayerId,
    major: PlayerId,
    pledge: bool,
) -> Option<CityStateBlock> {
    let round = auth.turn().round;
    let standing = auth.module().city_states().standing(city_state, major);
    if !pledge {
        return match standing.pledged {
            None => Some(CityStateBlock::NoPledge),
            Some(since) if since + PLEDGE_MIN_ROUNDS > round => Some(CityStateBlock::PledgedRecently),
            Some(_) => None,
        };
    }
    if standing.pledged.is_some() {
        return Some(CityStateBlock::AlreadyPledged);
    }
    if auth.diplomacy().at_war(city_state, major)
        || effective_influence(auth, city_state, major) < CAN_PLEDGE_THRESHOLD
    {
        return Some(CityStateBlock::NotOnGoodTerms);
    }
    match standing.pledge_broken {
        Some(broken) if broken + PLEDGE_AFTER_BROKEN > round => {
            Some(CityStateBlock::PledgeBrokenRecently)
        }
        _ => None,
    }
}
