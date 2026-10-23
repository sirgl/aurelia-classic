//! Combat strength of units (classic/docs/classic/rules.md): `Base × (100 + M)`
//! in hundredths, M the additive sum of the percents that hold for the
//! strike, the result at least 1.0 (Unciv: `BattleDamage.kt`).
//!
//! The percents come from the unit's effects (`effects::unit`): every
//! context that holds for the strike adds its unconditional percent and its
//! situational terms whose condition holds (battle tile, opponent). The
//! modifiers that are not effect contexts go into the same sum (Unciv adds
//! all modifiers into one factor, `BattleDamage.kt` `modifiersToFinalBonus`;
//! it has terrain defense, fortification, river crossing, landing, flanking,
//! great generals and the barbarian bonus there, checked 2026-10-20):
//! terrain defense, fortification, river crossing, amphibious landing,
//! flanking, great generals, barbarians, and enemy auras (not in Unciv,
//! unverified).

use crate::classic::effects::{ClassicCondition, ClassicUnitAbility, ClassicUnitStat};
use crate::combat::terrain::{is_friendly_territory, tile_defense_percent};
use crate::navigation::terrain::{is_rough, is_water, terrain_kind};
use crate::effects::unit::{own_ability, own_stat, unit_modifiers, CombatTerm, UnitModifiers};
use crate::ids::{CityHandle, PlayerId, TileId, UnitHandle};
use crate::rules::compiled::{UnitDefinition, DEFAULT_STRENGTH};
use crate::rules::content::{Domain, UnitRole};
use crate::rules::effects::{CombatContext, Condition, UnitAbility, UnitStat};
use crate::rules::CompiledRuleset;
use crate::world::state::AuthWorld;
use crate::world::unit::UnitCore;
use crate::world::player::ClassicPlayers;

/// Strength bonus (percent) of a unit near a great general or admiral (the
/// largest bonus counts, they do not add). Unciv: `Units.json` (Great General,
/// "+15 % Strength bonus for Military Land units within 2 tiles"; Great
/// Admiral the same for Water units), `battle/GreatGeneralImplementation.kt`;
/// equal.
pub const GREAT_GENERAL_BONUS_PERCENT: i64 = 15;
/// Range in tiles of a great general's bonus. Unciv: `Units.json` (within 2
/// tiles); equal.
pub const GREAT_GENERAL_BONUS_RANGE: i32 = 2;
/// Flanking: percent per adjacent friendly unit. Unciv: `BattleConstants.kt`
/// (`BASE_FLANKING_BONUS`, 10); equal.
pub const BASE_FLANKING_BONUS: i64 = 10;
/// Attack across a river, percent. Unciv: `BattleConstants.kt`
/// (`ATTACKING_ACROSS_RIVER_MALUS`, -20); equal.
pub const ATTACKING_ACROSS_RIVER_MALUS: i64 = -20;
/// Amphibious landing attack, percent. Unciv: `BattleConstants.kt`
/// (`LANDING_MALUS`, -50); equal.
pub const LANDING_MALUS: i64 = -50;
/// Fortification, percent per fortified turn. Unciv: `BattleConstants.kt`
/// (`FORTIFICATION_BONUS`, 20, times the fortified turns); equal.
pub const FORTIFICATION_BONUS: i64 = 20;
/// Attack bonus (percent) against a city whose walls were sapped, and the range
/// of a sapper's effect in tiles. Own values, not compared with a public source
/// (unverified).
pub const SAPPED_WALLS_ATTACK_PERCENT: i64 = 50;
pub const SAPPER_EFFECT_RANGE: i32 = 2;
/// Bonus (percent) of every non-barbarian unit fighting barbarians, by the
/// game's difficulty level (Settler .. Deity); every player gets the game's
/// level. Unciv: `Difficulties.json` (`barbarianBonus` 0.75 / 0.5 / 0.4 /
/// 0.33 / 0.25 / 0.2 / 0.1 / 0), read in `battle/BattleDamage.kt`; equal.
pub const BARBARIAN_COMBAT_BONUS_BY_LEVEL: [i64; 8] = [75, 50, 40, 33, 25, 20, 10, 0];
/// The bonus without a difficulty table (legacy fixtures): Prince's. Unciv:
/// `Difficulties.json`, Prince 0.33; equal.
pub const BARBARIAN_COMBAT_BONUS: i64 = 33;
/// Least strength after the modifiers, in hundredths (1.0): the modifier sum
/// has no floor. Unciv: `battle/BattleDamage.kt` (`max(1f, strength x
/// modifier)`); equal.
pub const MIN_STRENGTH: i64 = 100;

/// A unit in a strike: its state, content and modifiers.
#[derive(Debug, Clone)]
pub struct Fighter<'r> {
    pub handle: UnitHandle,
    pub core: UnitCore,
    pub def: Option<&'r UnitDefinition>,
    pub mods: UnitModifiers,
}

impl<'r> Fighter<'r> {
    pub fn of(auth: &AuthWorld, rules: &'r CompiledRuleset, handle: UnitHandle) -> Option<Self> {
        let core = *auth.units().get(handle)?.core;
        Some(Self {
            handle,
            core,
            def: rules.units.get(core.kind.0 as usize),
            mods: unit_modifiers(auth, rules, &core),
        })
    }

    pub fn owner(&self) -> PlayerId {
        self.core.owner
    }

    pub fn tile(&self) -> TileId {
        self.core.tile
    }

    pub fn domain(&self) -> Domain {
        self.def.map(|d| d.domain).unwrap_or_default()
    }

    pub fn role(&self) -> Option<UnitRole> {
        self.def.and_then(|d| d.role)
    }

    /// Melee strength of the content (`Combat`).
    pub fn base_combat(&self) -> i64 {
        match self.def {
            Some(def) => i64::from(def.strength),
            None => i64::from(DEFAULT_STRENGTH),
        }
    }

    /// Ranged strength of the content; Combat / 2 for support fire.
    pub fn base_ranged(&self) -> i64 {
        if self.mods.has(UnitAbility::RangedSupportFire) {
            return self.base_combat() / 2;
        }
        self.def.map(|d| i64::from(d.ranged_strength)).unwrap_or(0)
    }

    pub fn range(&self) -> i32 {
        let base = self.def.map(|d| i32::from(d.range)).unwrap_or(0);
        (base + self.mods.stat(UnitStat::Range)).max(0)
    }

    /// A land unit on water.
    pub fn is_embarked(&self, auth: &AuthWorld) -> bool {
        self.domain() == Domain::Land && is_water(auth.map(), self.core.tile)
    }

    /// Can fight back when attacked: a combat unit or
    /// an embarked unit. Civilians are captured or destroyed instead.
    pub fn can_defend(&self, auth: &AuthWorld, rules: &CompiledRuleset) -> bool {
        if rules.is_civilian(self.core.kind) {
            return false;
        }
        self.base_combat() > 0 || self.is_embarked(auth)
    }

    /// Has a ranged attack (ranged strength above 0 and a range).
    pub fn is_ranged(&self) -> bool {
        self.def
            .is_some_and(|d| d.ranged_strength > 0 && d.range > 0)
    }

    pub fn is_air(&self) -> bool {
        self.domain() == Domain::Air
    }
}

/// What a strike is about, for the contexts and situational terms.
#[derive(Debug, Clone, Copy)]
struct Strike<'a, 'r> {
    me: &'a Fighter<'r>,
    other: Option<&'a Fighter<'r>>,
    /// The tile whose terrain counts (the defender's tile).
    battle_tile: TileId,
    contexts: ContextSet,
    /// The player on the other side (the other unit's or the city's owner).
    opponent: Option<PlayerId>,
}

/// The player on the other side of a strike: the other unit's owner, else
/// the owner of the city struck.
fn opponent_of(auth: &AuthWorld, other: Option<&Fighter<'_>>, city: Option<CityHandle>) -> Option<PlayerId> {
    other
        .map(|o| o.owner())
        .or_else(|| city.and_then(|c| auth.cities().get(c)).map(|c| c.owner))
}

/// The contexts that hold for one strike.
#[derive(Debug, Clone, Copy, Default)]
struct ContextSet(u32);

impl ContextSet {
    fn with(mut self, context: CombatContext) -> Self {
        self.0 |= 1 << crate::effects::unit::combat_slot(context);
        self
    }

    fn with_if(self, holds: bool, context: CombatContext) -> Self {
        if holds {
            self.with(context)
        } else {
            self
        }
    }

    fn holds(&self, context: CombatContext) -> bool {
        self.0 & (1 << crate::effects::unit::combat_slot(context)) != 0
    }
}

/// The summable contexts (all but the parametric ones, which the strength
/// functions read themselves).
const SUMMABLE: [CombatContext; 19] = [
    CombatContext::Any,
    CombatContext::Attack,
    CombatContext::Defense,
    CombatContext::VsCity,
    CombatContext::AttackOpen,
    CombatContext::AttackRough,
    CombatContext::DefenseOpen,
    CombatContext::DefenseRough,
    CombatContext::RangedAttackOpen,
    CombatContext::RangedAttackRough,
    CombatContext::DefenseVsRanged,
    CombatContext::RangedAttack,
    CombatContext::VsWounded,
    CombatContext::VsFortified,
    CombatContext::FriendlyLand,
    CombatContext::OutsideFriendlyLand,
    CombatContext::AttackFriendlyLand,
    CombatContext::AdjacentFriendly,
    CombatContext::Interception,
];

/// Σ of the unit's percents in the holding contexts, situational terms
/// included.
fn context_sum(auth: &AuthWorld, rules: &CompiledRuleset, strike: &Strike<'_, '_>) -> i64 {
    let mods = &strike.me.mods;
    let mut sum = 0i64;
    for context in SUMMABLE {
        if strike.contexts.holds(context) {
            sum += i64::from(mods.percent(context));
        }
    }
    for term in &mods.situational {
        if strike.contexts.holds(term.context) && term_holds(auth, rules, strike, term) {
            sum += i64::from(term.percent);
        }
    }
    sum
}

/// Does a situational term hold for the strike? Terrain terms follow the
/// A feature replaces the terrain (the feature attack modifier or the
/// terrain attack modifier).
fn term_holds(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    strike: &Strike<'_, '_>,
    term: &CombatTerm,
) -> bool {
    let map = auth.map();
    let tile = strike.battle_tile;
    match term.condition {
        Condition::TileFeature(f) => map.feature(tile).flatten() == Some(f),
        Condition::TileTerrain(t) => {
            map.feature(tile).flatten().is_none() && terrain_kind(rules, map, tile) == Some(t)
        }
        // City production percents never reach a unit (the compiler).
        Condition::Module(ClassicCondition::MinFollowers(_))
        | Condition::EventNearSourceCity(_)
        | Condition::Producing(_)
        | Condition::CityOnRiver
        | Condition::AtPeace
        | Condition::ConnectedToCapital => false,
        Condition::Module(ClassicCondition::NearFollowingCity(relation)) => {
            crate::classic::religion_effects::near_following_city(auth, strike.me.owner(), tile, relation)
        }
        Condition::TileTerrainUntil(t, tech) => {
            let known = auth.players().techs_of(strike.me.owner()).contains(&tech);
            !known && terrain_kind(rules, map, tile) == Some(t)
        }
        Condition::OpponentRole(role) => strike.other.is_some_and(|o| o.role() == Some(role)),
        Condition::OpponentDomain(domain) => strike.other.is_some_and(|o| o.domain() == domain),
        Condition::OpponentClassOf(kind) => {
            let class = rules
                .units
                .get(kind.0 as usize)
                .and_then(|d| d.class.as_ref());
            let other = strike
                .other
                .and_then(|o| o.def)
                .and_then(|d| d.class.as_ref());
            class.is_some() && class == other
        }
        Condition::Module(ClassicCondition::OpponentIsBarbarian) => strike
            .other
            .is_some_and(|o| auth.players().is_barbarian(o.owner())),
        Condition::Module(ClassicCondition::OpponentIsCityState) => {
            strike.opponent.is_some_and(|p| auth.players().is_city_state(p))
        }
        Condition::Module(ClassicCondition::OpponentHasMoreCities) => strike.opponent.is_some_and(|p| {
            let cities = |q: PlayerId| auth.indexes().cities_of(q).len();
            cities(p) > cities(strike.me.owner())
        }),
        Condition::NearOwnImprovement { improvement, range } => {
            near_own_improvement(auth, strike.me.owner(), strike.me.tile(), improvement, range)
        }
        Condition::UnitDomain(_) | Condition::UnitRole(_) => true,
        // Compiler: Player scope only, never a combat term.
        Condition::Module(ClassicCondition::GreatPersonIs(_)) => false,
        Condition::Always
        | Condition::HasTechnology(_)
        | Condition::LacksTechnology(_)
        | Condition::MinPopulation(_)
        | Condition::HasBuilding(_)
        | Condition::TileImprovement(_)
        | Condition::TileResource(_)
        | Condition::Module(ClassicCondition::GoldenAge)
        | Condition::Module(ClassicCondition::GreatGeneralEarned)
        | Condition::CityGarrisoned
        | Condition::CityCoastal
        | Condition::LearnedTechnology(_) => true,
        // Tile and City conditions: the compiler keeps them off Unit scope.
        Condition::TilePlot(_)
        | Condition::TileResourceClass(_)
        | Condition::TileFeatureUnimproved(_)
        | Condition::CityAdjacentUnimprovedFeature { .. } => true,
    }
}

/// An improvement of `owner`'s kind `improvement`, not pillaged, stands on
/// a tile of the owner within `range` tiles of `tile` (the tile itself
/// included).
fn near_own_improvement(
    auth: &AuthWorld,
    owner: PlayerId,
    tile: TileId,
    improvement: crate::ids::ImprovementId,
    range: u8,
) -> bool {
    let map = auth.map();
    crate::math::hex_disc(map.dimensions, tile, i32::from(range)).any(|near| {
        map.working_improvement(near) == Some(improvement) && map.owner(near).flatten() == Some(owner)
    })
}

/// `M_generic` (§1.2) without the context percents: great generals, enemy
/// auras, flanking (unless `ignore_adjacency`), barbarians, the hooks of
/// other systems.
fn generic_modifier(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    me: &Fighter<'_>,
    other: Option<&Fighter<'_>>,
    ignore_adjacency: bool,
) -> i64 {
    let mut m = unhappiness_penalty(auth, me.owner());
    m += great_general_bonus(auth, rules, me);
    m += enemy_aura(auth, rules, me);
    m += near_capital_bonus(auth, rules, me);
    let Some(other) = other else {
        return m;
    };
    if !ignore_adjacency {
        m += flanking_bonus(auth, rules, me, other);
    }
    if auth.players().is_barbarian(other.owner()) && !auth.players().is_barbarian(me.owner()) {
        m += barbarian_combat_bonus(auth, rules);
    }
    m
}

/// The bonus against barbarians at the game's difficulty level (Unciv applies
/// it to every civilization, city-states included).
pub fn barbarian_combat_bonus(auth: &AuthWorld, rules: &CompiledRuleset) -> i64 {
    match auth.settings().level_index(rules) {
        Some(level) => BARBARIAN_COMBAT_BONUS_BY_LEVEL
            .get(usize::from(level))
            .copied()
            .unwrap_or(BARBARIAN_COMBAT_BONUS),
        None => BARBARIAN_COMBAT_BONUS,
    }
}

/// Hook of the happiness system (track A): −2 % per point of excess
/// unhappiness, at most −40 % (`VERY_UNHAPPY_*`). The core has no
/// happiness state yet: 0.
fn unhappiness_penalty(_auth: &AuthWorld, _owner: PlayerId) -> i64 {
    0
}

/// +15 % when an own great general (land) or great admiral (sea) stands
/// within two tiles (Unciv: `Units.json`); the general's StackedGeneralPercent
/// when it stands on the unit's own tile.
fn great_general_bonus(auth: &AuthWorld, rules: &CompiledRuleset, me: &Fighter<'_>) -> i64 {
    let leader = match me.domain() {
        Domain::Land => UnitAbility::Module(ClassicUnitAbility::GreatGeneral),
        Domain::Sea => UnitAbility::Module(ClassicUnitAbility::GreatAdmiral),
        Domain::Air => return 0,
    };
    let dims = auth.map().dimensions;
    let mut near = false;
    let mut stacked = 0i64;
    for tile in crate::math::hex_disc(dims, me.tile(), GREAT_GENERAL_BONUS_RANGE) {
        for &other in auth.indexes().units_on(tile) {
            if other == me.handle {
                continue;
            }
            let Some(view) = auth.units().get(other) else {
                continue;
            };
            if view.core.owner != me.owner() {
                continue;
            }
            if !own_ability(rules, view.core, leader) {
                continue;
            }
            near = true;
            if tile == me.tile() {
                let bonus = own_stat(rules, view.core, UnitStat::Module(ClassicUnitStat::StackedGeneralPercent));
                stacked = stacked.max(i64::from(bonus));
            }
        }
    }
    if near {
        // China's trait: the owner's generals and admirals give more.
        let aura = crate::effects::unit::owner_unit_modifiers(auth, rules, me.owner())
            .stat(UnitStat::Module(ClassicUnitStat::GeneralAuraPercent));
        GREAT_GENERAL_BONUS_PERCENT + stacked + i64::from(aura)
    } else {
        0
    }
}

/// Enemy units with an aura (Feared Elephant, Haka
/// War Dance) within their aura range of the unit.
fn enemy_aura(auth: &AuthWorld, rules: &CompiledRuleset, me: &Fighter<'_>) -> i64 {
    if !rules.effect_index.has_passive_unit {
        return 0;
    }
    let dims = auth.map().dimensions;
    let mut sum = 0i64;
    for tile in crate::math::hex_disc(dims, me.tile(), GREAT_GENERAL_BONUS_RANGE) {
        for &other in auth.indexes().units_on(tile) {
            let Some(view) = auth.units().get(other) else {
                continue;
            };
            if !auth.diplomacy().at_war(view.core.owner, me.owner()) {
                continue;
            }
            let percent = own_stat(rules, view.core, UnitStat::EnemyAuraPercent);
            let range = own_stat(rules, view.core, UnitStat::AuraRange);
            let distance = dims.distance(tile, me.tile()).unwrap_or(i32::MAX);
            if percent != 0 && distance <= range {
                sum += i64::from(percent);
            }
        }
    }
    sum
}

/// The near-capital percent plus the distance × `CapitalDefenseFalloff`, when
/// positive.
fn near_capital_bonus(auth: &AuthWorld, rules: &CompiledRuleset, me: &Fighter<'_>) -> i64 {
    let percent = i64::from(me.mods.percent(CombatContext::NearCapital));
    if percent == 0 {
        return 0;
    }
    let Some(capital) = crate::combat::city::capital_of(auth, rules, me.owner()) else {
        return 0;
    };
    let Some(city) = auth.cities().get(capital) else {
        return 0;
    };
    let distance = auth
        .map()
        .dimensions
        .distance(city.tile, me.tile())
        .unwrap_or(i32::MAX);
    let falloff = i64::from(me.mods.stat(UnitStat::CapitalDefenseFalloff));
    (percent + i64::from(distance) * falloff).max(0)
}

/// Flanking (§1.5), melee attacks only: +10 % per other melee combat unit of
/// the attacker's owner next to the defender; `Flanking` percent raises it.
/// Unciv: `battle/BattleDamage.kt` (`getAttackModifiers`: own melee military
/// units around the defender, no defensive flanking); equal.
fn flanking_bonus(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    me: &Fighter<'_>,
    other: &Fighter<'_>,
) -> i64 {
    let dims = auth.map().dimensions;
    let mut count = 0i64;
    for tile in dims.neighbors(other.tile()) {
        for &unit in auth.indexes().units_on(tile) {
            if unit == me.handle {
                continue;
            }
            let Some(view) = auth.units().get(unit) else {
                continue;
            };
            let core = view.core;
            if core.owner != me.owner() || rules.is_civilian(core.kind) {
                continue;
            }
            let def = rules.units.get(core.kind.0 as usize);
            let combat = def.map(|d| d.strength > 0).unwrap_or(true);
            let melee = def.map(|d| d.ranged_strength == 0).unwrap_or(true);
            if combat && melee {
                count += 1;
            }
        }
    }
    let mut flank = BASE_FLANKING_BONUS * count;
    let raise = i64::from(me.mods.percent(CombatContext::Flanking));
    if raise > 0 {
        flank = flank * (100 + raise) / 100;
    }
    flank
}

/// An own combat unit stands next to the unit, on the same kind of tile
/// (same area).
fn friendly_adjacent(auth: &AuthWorld, rules: &CompiledRuleset, me: &Fighter<'_>) -> bool {
    let map = auth.map();
    let my_water = is_water(map, me.tile());
    map.dimensions.neighbors(me.tile()).into_iter().any(|tile| {
        is_water(map, tile) == my_water
            && auth.indexes().units_on(tile).iter().any(|u| {
                auth.units()
                    .get(*u)
                    .is_some_and(|v| v.core.owner == me.owner() && !rules.is_civilian(v.core.kind))
            })
    })
}

fn finish(base: i64, modifier: i64) -> i64 {
    (base * (100 + modifier)).max(MIN_STRENGTH)
}

/// What a melee attacker attacks.
#[derive(Debug, Clone, Copy)]
pub enum MeleeTarget<'a, 'r> {
    Unit(&'a Fighter<'r>),
    City { city: CityHandle, tile: TileId },
}

/// Melee attack strength (§1.3), from the
/// attacker's tile into `target`.
pub fn melee_attack_strength(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    attacker: &Fighter<'_>,
    target: MeleeTarget<'_, '_>,
) -> i64 {
    let map = auth.map();
    let (to, defender, city) = match target {
        MeleeTarget::Unit(d) => (d.tile(), Some(d), None),
        MeleeTarget::City { city, tile } => (tile, None, Some(city)),
    };
    let from = attacker.tile();
    // Embarked attacking water: the era's embarked defense.
    if attacker.is_embarked(auth) && is_water(map, to) {
        return embarked_defense(auth, rules, attacker);
    }
    let friendly = is_friendly_territory(auth, attacker.owner(), to);
    let open_or_rough = city.is_none();
    let rough = is_rough(rules, map, to);
    let contexts = ContextSet::default()
        .with(CombatContext::Any)
        .with(CombatContext::Attack)
        .with_if(city.is_some(), CombatContext::VsCity)
        .with_if(open_or_rough && !rough, CombatContext::AttackOpen)
        .with_if(open_or_rough && rough, CombatContext::AttackRough)
        .with_if(friendly, CombatContext::FriendlyLand)
        .with_if(friendly, CombatContext::AttackFriendlyLand)
        .with_if(!friendly, CombatContext::OutsideFriendlyLand)
        .with_if(
            defender.is_some_and(|d| d.core.hp < crate::world::unit::UNIT_MAX_HP),
            CombatContext::VsWounded,
        )
        .with_if(
            defender.is_some_and(|d| d.core.stance.is_fortified()),
            CombatContext::VsFortified,
        )
        .with_if(
            friendly_adjacent(auth, rules, attacker),
            CombatContext::AdjacentFriendly,
        );
    let strike = Strike {
        me: attacker,
        other: defender,
        battle_tile: to,
        contexts,
        opponent: opponent_of(auth, defender, city),
    };
    let mut m = context_sum(auth, rules, &strike)
        + generic_modifier(auth, rules, attacker, defender, false);
    if let Some(city) = city {
        if near_sapper(auth, rules, attacker.owner(), city) {
            m += SAPPED_WALLS_ATTACK_PERCENT;
        }
    }
    let amphibious = attacker.mods.has(UnitAbility::Amphibious);
    let river_free = amphibious || attacker.mods.has(UnitAbility::RiverCrossing);
    if map.river_between(from, to) && !river_free {
        m += ATTACKING_ACROSS_RIVER_MALUS;
    }
    if attacker.domain() == Domain::Land && is_water(map, from) && !is_water(map, to) && !amphibious
    {
        m += LANDING_MALUS;
    }
    let strength = finish(attacker.base_combat(), m);
    let heavy = attacker.mods.has(UnitAbility::HeavyCharge);
    let can_withdraw = defender.is_some_and(|d| d.mods.stat(UnitStat::WithdrawChance) > 0);
    if heavy && defender.is_some() && !can_withdraw {
        return strength * 150 / 100;
    }
    strength
}

/// Is an own unit with Sapper within SAPPER_EFFECT_RANGE of the city?
fn near_sapper(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    owner: PlayerId,
    city: CityHandle,
) -> bool {
    let Some(core) = auth.cities().get(city) else {
        return false;
    };
    let dims = auth.map().dimensions;
    crate::math::hex_disc(dims, core.tile, SAPPER_EFFECT_RANGE).any(|tile| {
        auth.indexes().units_on(tile).iter().any(|u| {
            auth.units().get(*u).is_some_and(|v| {
                v.core.owner == owner && own_ability(rules, v.core, UnitAbility::Sapper)
            })
        })
    })
}

/// Embarked defense (§1.8): the owner's era value × 100, raised by
/// EmbarkedDefense; no other modifier applies.
pub fn embarked_defense(auth: &AuthWorld, rules: &CompiledRuleset, unit: &Fighter<'_>) -> i64 {
    let era = crate::combat::city::current_era(auth, rules, unit.owner());
    let base = era.map(|e| i64::from(e.embarked_unit_defense)).unwrap_or(3);
    let mut value = base * 100;
    let raise = i64::from(unit.mods.percent(CombatContext::EmbarkedDefense));
    if raise > 0 {
        value = value * (100 + raise) / 100;
    }
    value.max(1)
}

/// Defense strength (§1.4) on the defender's own
/// tile against `attacker` (melee or ranged).
pub fn defense_strength(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    defender: &Fighter<'_>,
    attacker: Option<&Fighter<'_>>,
    from_ranged: bool,
) -> i64 {
    if defender.is_embarked(auth) {
        return embarked_defense(auth, rules, defender);
    }
    let map = auth.map();
    let tile = defender.tile();
    let rough = is_rough(rules, map, tile);
    let friendly = is_friendly_territory(auth, defender.owner(), tile);
    let contexts = ContextSet::default()
        .with(CombatContext::Any)
        .with(CombatContext::Defense)
        .with_if(from_ranged, CombatContext::DefenseVsRanged)
        .with_if(!rough, CombatContext::DefenseOpen)
        .with_if(rough, CombatContext::DefenseRough)
        .with_if(friendly, CombatContext::FriendlyLand)
        .with_if(!friendly, CombatContext::OutsideFriendlyLand)
        .with_if(
            !from_ranged && friendly_adjacent(auth, rules, defender),
            CombatContext::AdjacentFriendly,
        );
    let strike = Strike {
        me: defender,
        other: attacker,
        battle_tile: tile,
        contexts,
        opponent: opponent_of(auth, attacker, None),
    };
    // No defensive flanking (Unciv: `BattleDamage.kt`).
    let mut m = context_sum(auth, rules, &strike)
        + generic_modifier(auth, rules, defender, attacker, true);
    let no_bonus = defender.mods.has(UnitAbility::NoDefensiveBonus);
    let terrain = if is_water(map, tile) {
        0
    } else {
        tile_defense_percent(auth, rules, tile, defender.owner())
    };
    if !no_bonus || terrain < 0 {
        m += terrain;
    }
    if !defender.mods.has(UnitAbility::RangedSupportFire) {
        m += i64::from(defender.core.stance.fortify_turns()) * FORTIFICATION_BONUS;
    }
    finish(defender.base_combat(), m)
}

/// Ranged strength (§1.7).
/// `attacking`: the unit strikes (else it defends with its ranged strength
/// against a ranged or air attack). `city`: the target is a city.
pub fn ranged_strength(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    me: &Fighter<'_>,
    other: Option<&Fighter<'_>>,
    city: Option<CityHandle>,
    target_tile: TileId,
    attacking: bool,
) -> i64 {
    let map = auth.map();
    let rough = is_rough(rules, map, target_tile);
    let friendly = is_friendly_territory(auth, me.owner(), target_tile);
    let unit_target = attacking && city.is_none();
    let contexts = ContextSet::default()
        .with(CombatContext::Any)
        .with_if(attacking, CombatContext::Attack)
        .with_if(attacking, CombatContext::RangedAttack)
        .with_if(!attacking, CombatContext::Defense)
        .with_if(!attacking, CombatContext::DefenseVsRanged)
        .with_if(city.is_some(), CombatContext::VsCity)
        .with_if(unit_target && !rough, CombatContext::RangedAttackOpen)
        .with_if(unit_target && rough, CombatContext::RangedAttackRough)
        .with_if(attacking && friendly, CombatContext::FriendlyLand)
        .with_if(attacking && !friendly, CombatContext::OutsideFriendlyLand)
        .with_if(
            other.is_some_and(|o| o.core.hp < crate::world::unit::UNIT_MAX_HP),
            CombatContext::VsWounded,
        )
        .with_if(
            other.is_some_and(|o| o.core.stance.is_fortified()),
            CombatContext::VsFortified,
        );
    let strike = Strike {
        me,
        other,
        battle_tile: target_tile,
        contexts,
        opponent: opponent_of(auth, other, city),
    };
    // Ranged strength has no flanking and no adjacency bonus.
    let mut m = context_sum(auth, rules, &strike) + generic_modifier(auth, rules, me, other, true);
    if let Some(city) = city {
        if near_sapper(auth, rules, me.owner(), city) {
            m += SAPPED_WALLS_ATTACK_PERCENT;
        }
    }
    if !attacking {
        let terrain = tile_defense_percent(auth, rules, me.tile(), me.owner());
        if !me.mods.has(UnitAbility::NoDefensiveBonus) || terrain < 0 {
            m += terrain;
        }
    }
    finish(me.base_ranged(), m)
}

/// The strength a unit opposes to a ranged attack or a city strike (§2.5):
/// embarked defense, its own ranged strength (land ranged units), else its
/// defense against a ranged attack.
pub fn defense_vs_ranged(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    defender: &Fighter<'_>,
    attacker: Option<&Fighter<'_>>,
) -> i64 {
    if defender.is_embarked(auth) {
        return embarked_defense(auth, rules, defender);
    }
    let ranged_land = !defender.mods.has(UnitAbility::RangedSupportFire)
        && defender.domain() != Domain::Sea
        && defender.base_ranged() > 0;
    if ranged_land {
        return ranged_strength(
            auth,
            rules,
            defender,
            attacker,
            None,
            defender.tile(),
            false,
        );
    }
    defense_strength(auth, rules, defender, attacker, true)
}
