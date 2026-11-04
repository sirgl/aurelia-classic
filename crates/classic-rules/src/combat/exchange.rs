//! One attack planned (classic/docs/classic/rules.md): the attack's
//! legality, who defends the target tile, both strengths and the damage
//! formulas. `systems::combat` resolves a plan with drawn rolls;
//! `CombatPreview` shows the same plan with expected rolls (UI odds, AI).

use crate::combat::damage::{
    city_strike_damage, melee_damage, ranged_damage, MeleeSide, Roll, DAMAGE_TO_CIVILIAN_UNIT,
};
use crate::combat::strength::{
    defense_strength, defense_vs_ranged, melee_attack_strength, ranged_strength, Fighter,
    MeleeTarget,
};
use crate::navigation::terrain::is_water;
use crate::ids::{CityHandle, PlayerId, TileId, UnitHandle};
use crate::rules::content::Domain;
use crate::rules::effects::{UnitAbility, UnitStat};
use crate::rules::CompiledRuleset;
use crate::world::state::GameView;
use crate::world::unit::Stance;

/// Melee (attacker moves in on a win) or ranged (no answer, no move).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttackKind {
    Melee,
    Ranged,
}

/// Who answers the attack on the target tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Defender {
    /// A combat unit (the best defender of the tile).
    Unit(UnitHandle),
    /// Only civilians stand there: melee captures or destroys them, a
    /// ranged attack deals DAMAGE_TO_CIVILIAN_UNIT.
    Civilian(UnitHandle),
    /// The city on the tile (its garrison only adds strength).
    City(CityHandle),
}

/// Why an attack is not possible (shown to the player).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanError {
    UnknownUnit,
    NotACombatUnit,
    NoMovesLeft,
    NoAttacksLeft,
    OnlyDefensive,
    CitiesOnly,
    NotRanged,
    NotSetUp,
    Embarked,
    OutOfRange,
    NoLineOfSight,
    NotVisible,
    NoTarget,
    NotAtWar,
    WrongDomain,
}

impl PlanError {
    pub fn reason(self) -> &'static str {
        match self {
            PlanError::UnknownUnit => "unknown unit",
            PlanError::NotACombatUnit => "not a combat unit",
            PlanError::NoMovesLeft => "no moves left",
            PlanError::NoAttacksLeft => "no attacks left",
            PlanError::OnlyDefensive => "cannot melee attack",
            PlanError::CitiesOnly => "attacks only cities",
            PlanError::NotRanged => "no ranged attack",
            PlanError::NotSetUp => "must set up first",
            PlanError::Embarked => "embarked",
            PlanError::OutOfRange => "out of range",
            PlanError::NoLineOfSight => "no line of sight",
            PlanError::NotVisible => "target not visible",
            PlanError::NoTarget => "nothing to attack",
            PlanError::NotAtWar => "not at war",
            PlanError::WrongDomain => "cannot attack there",
        }
    }
}

/// A legal attack with everything its resolution needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttackPlan {
    pub attacker: UnitHandle,
    pub attacker_owner: PlayerId,
    pub kind: AttackKind,
    pub target_tile: TileId,
    pub defender: Defender,
    pub defender_owner: PlayerId,
    /// Strengths in hundredths (0 for a civilian defender).
    pub attacker_strength: i64,
    pub defender_strength: i64,
    pub attacker_hp: u16,
    pub defender_hp: u16,
    pub defender_max_hp: u16,
    pub attacker_damage: u16,
    pub defender_damage: u16,
    /// Wounded damage modifiers (WoundedDamagePercent).
    pub attacker_wounded_mod: i64,
    pub defender_wounded_mod: i64,
    /// The attacker is an aircraft (air strike damage, interception).
    pub air: bool,
}

/// Expected outcome shown before an attack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CombatPreview {
    pub attacker_strength: i64,
    pub defender_strength: i64,
    /// (min, expected, max) damage to the defender, capped by its HP.
    pub to_defender: (u16, u16, u16),
    /// The expected roll's damage to the defender before any cap: neither
    /// its HP nor the 1 HP a ranged blow leaves a city (`cap_to_defender`).
    pub uncapped_to_defender: u16,
    /// (min, expected, max) damage to the attacker (0 for ranged attacks).
    pub to_attacker: (u16, u16, u16),
    pub attacker_hp: u16,
    pub defender_hp: u16,
    pub verdict: Verdict,
}

/// What an air strike is expected to cost the aircraft (a forecast for the
/// player who plans it: only the interceptors it sees count; no dice).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AirRisk {
    /// Damage of the target's answer.
    pub answer: u16,
    /// Damage of the best visible interceptor, if it intercepts.
    pub interception: u16,
    /// Its chance to intercept, percent (evasion taken off).
    pub chance: u8,
}

/// Headline of the combat preview.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The defender dies even on the lowest roll (or is captured).
    DecisiveVictory,
    MajorVictory,
    MinorVictory,
    Stalemate,
    MinorDefeat,
    MajorDefeat,
    /// The attacker dies even on its best roll.
    DecisiveDefeat,
}

impl AttackPlan {
    /// Damage dealt to the defender with `roll` (before the HP cap).
    pub fn damage_to_defender(&self, roll: Roll) -> u16 {
        match (self.kind, self.defender) {
            (_, Defender::Civilian(_)) if self.kind == AttackKind::Ranged => {
                DAMAGE_TO_CIVILIAN_UNIT
            }
            (_, Defender::Civilian(_)) => 0,
            (AttackKind::Melee, Defender::City(_)) => melee_damage(
                self.attacker_strength,
                self.defender_strength,
                self.attacker_damage,
                self.attacker_wounded_mod,
                roll,
                MeleeSide::UnitVsCity,
            ),
            (AttackKind::Melee, Defender::Unit(_)) => melee_damage(
                self.attacker_strength,
                self.defender_strength,
                self.attacker_damage,
                self.attacker_wounded_mod,
                roll,
                MeleeSide::UnitVsUnit,
            ),
            (AttackKind::Ranged, _) if self.air => crate::combat::damage::air_damage(
                self.attacker_strength,
                self.defender_strength,
                self.attacker_damage,
                0,
                roll,
            ),
            (AttackKind::Ranged, _) => ranged_damage(
                self.attacker_strength,
                self.defender_strength,
                self.attacker_damage,
                self.attacker_wounded_mod,
                roll,
            ),
        }
    }

    /// Damage the defender deals back (melee only; 0 otherwise).
    pub fn damage_to_attacker(&self, roll: Roll) -> u16 {
        match (self.kind, self.defender) {
            (AttackKind::Ranged, _) | (_, Defender::Civilian(_)) => 0,
            (AttackKind::Melee, Defender::City(_)) => melee_damage(
                self.defender_strength,
                self.attacker_strength,
                0,
                0,
                roll,
                MeleeSide::CityVsUnit,
            ),
            (AttackKind::Melee, Defender::Unit(_)) => melee_damage(
                self.defender_strength,
                self.attacker_strength,
                self.defender_damage,
                self.defender_wounded_mod,
                roll,
                MeleeSide::UnitVsUnit,
            ),
        }
    }

    /// Damage that can actually land on the defender: a ranged attack leaves
    /// a city at 1 HP (§2.5), everything else is capped by the HP left.
    pub fn cap_to_defender(&self, damage: u16) -> u16 {
        match (self.kind, self.defender) {
            (AttackKind::Ranged, Defender::City(_)) => {
                damage.min(self.defender_hp.saturating_sub(1))
            }
            _ => damage.min(self.defender_hp),
        }
    }

    pub fn preview(&self) -> CombatPreview {
        let low = Roll::Drawn(0);
        let high =
            Roll::Drawn(crate::combat::damage::MELEE_EQUAL_STRENGTH_RANDOM_DAMAGE - 1);
        let to_defender = (
            self.cap_to_defender(self.damage_to_defender(low)),
            self.cap_to_defender(self.damage_to_defender(Roll::Expected)),
            self.cap_to_defender(self.damage_to_defender(high)),
        );
        let cap = |d: u16| d.min(self.attacker_hp);
        let to_attacker = (
            cap(self.damage_to_attacker(low)),
            cap(self.damage_to_attacker(Roll::Expected)),
            cap(self.damage_to_attacker(high)),
        );
        CombatPreview {
            attacker_strength: self.attacker_strength,
            defender_strength: self.defender_strength,
            to_defender,
            uncapped_to_defender: self.damage_to_defender(Roll::Expected),
            to_attacker,
            attacker_hp: self.attacker_hp,
            defender_hp: self.defender_hp,
            verdict: self.verdict(to_defender, to_attacker),
        }
    }

    fn verdict(&self, to_defender: (u16, u16, u16), to_attacker: (u16, u16, u16)) -> Verdict {
        if matches!(self.defender, Defender::Civilian(_)) && self.kind == AttackKind::Melee {
            return Verdict::DecisiveVictory;
        }
        if to_defender.0 >= self.defender_hp {
            return Verdict::DecisiveVictory;
        }
        if to_attacker.2 >= self.attacker_hp && to_attacker.0 >= self.attacker_hp {
            return Verdict::DecisiveDefeat;
        }
        let dealt = i32::from(to_defender.1) * 100 / i32::from(self.defender_hp.max(1));
        let taken = i32::from(to_attacker.1) * 100 / i32::from(self.attacker_hp.max(1));
        let margin = dealt - taken;
        match margin {
            m if m >= 40 => Verdict::MajorVictory,
            m if m >= 10 => Verdict::MinorVictory,
            m if m > -10 => Verdict::Stalemate,
            m if m > -40 => Verdict::MinorDefeat,
            _ => Verdict::MajorDefeat,
        }
    }
}

/// Plan an attack of `attacker` on `target` (§2–3, §7.7).
pub fn plan_attack(
    view: GameView<'_>,
    rules: &CompiledRuleset,
    attacker: UnitHandle,
    target: TileId,
    kind: AttackKind,
) -> Result<AttackPlan, PlanError> {
    let auth = view.auth;
    let me = Fighter::of(auth, rules, attacker).ok_or(PlanError::UnknownUnit)?;
    check_ready(&me, rules)?;
    match kind {
        AttackKind::Melee => check_melee_reach(view, rules, &me, target)?,
        AttackKind::Ranged => check_ranged_reach(view, rules, &me, target)?,
    }
    let defender = pick_defender(view, rules, &me, target, kind)?;
    Ok(build_plan(view, rules, &me, target, kind, defender, None))
}

/// [`plan_attack`] as if the attacker stood on `from`, ready to strike (a
/// siege unit set up): the preview of a move and attack. Strength, reach,
/// line of sight and flanking are read from `from`; the world is not
/// changed.
pub fn plan_attack_from(
    view: GameView<'_>,
    rules: &CompiledRuleset,
    attacker: UnitHandle,
    from: TileId,
    target: TileId,
    kind: AttackKind,
) -> Result<AttackPlan, PlanError> {
    let auth = view.auth;
    let mut me = Fighter::of(auth, rules, attacker).ok_or(PlanError::UnknownUnit)?;
    me.core.tile = from;
    if me.mods.has(UnitAbility::MustSetUp) {
        me.core.stance = Stance::SetUp;
    }
    me.mods = crate::effects::unit::unit_modifiers(auth, rules, &me.core);
    check_ready(&me, rules)?;
    match kind {
        AttackKind::Melee => check_melee_reach(view, rules, &me, target)?,
        AttackKind::Ranged => check_ranged_reach(view, rules, &me, target)?,
    }
    let defender = pick_defender(view, rules, &me, target, kind)?;
    Ok(build_plan(view, rules, &me, target, kind, defender, None))
}

/// [`plan_attack_from`] of the attacker as it would stand (`unit`: its core
/// after a promotion not taken yet, `experience::promoted`), striking from
/// `from`: the preview of a promotion's effect on a strike
/// (`ai::promo_fit`). The world is not changed.
pub fn plan_attack_as(
    view: GameView<'_>,
    rules: &CompiledRuleset,
    attacker: UnitHandle,
    unit: crate::world::unit::UnitCore,
    from: TileId,
    target: TileId,
    kind: AttackKind,
) -> Result<AttackPlan, PlanError> {
    let auth = view.auth;
    let mut me = Fighter::of(auth, rules, attacker).ok_or(PlanError::UnknownUnit)?;
    me.core = unit;
    me.core.tile = from;
    if me.mods.has(UnitAbility::MustSetUp) {
        me.core.stance = Stance::SetUp;
    }
    me.mods = crate::effects::unit::unit_modifiers(auth, rules, &me.core);
    check_ready(&me, rules)?;
    match kind {
        AttackKind::Melee => check_melee_reach(view, rules, &me, target)?,
        AttackKind::Ranged => check_ranged_reach(view, rules, &me, target)?,
    }
    let defender = pick_defender(view, rules, &me, target, kind)?;
    Ok(build_plan(view, rules, &me, target, kind, defender, None))
}

/// A foe's strike from `from` on the unit `defender` as it would stand
/// (`unit`: its core after a promotion not taken yet) on its tile: the
/// danger a promotion changes (`ai::promo_fit`). The foe's reach from
/// `from` is checked, its readiness is not (the strike of its next turn);
/// the defender stands for its tile whoever else is there. The world is not
/// changed.
pub fn plan_strike_on(
    view: GameView<'_>,
    rules: &CompiledRuleset,
    foe: UnitHandle,
    from: TileId,
    defender: UnitHandle,
    unit: crate::world::unit::UnitCore,
    kind: AttackKind,
) -> Result<AttackPlan, PlanError> {
    let auth = view.auth;
    let mut me = Fighter::of(auth, rules, foe).ok_or(PlanError::UnknownUnit)?;
    me.core.tile = from;
    if me.mods.has(UnitAbility::MustSetUp) {
        me.core.stance = Stance::SetUp;
    }
    me.mods = crate::effects::unit::unit_modifiers(auth, rules, &me.core);
    if rules.is_civilian(me.core.kind) || (me.base_combat() == 0 && me.base_ranged() == 0) {
        return Err(PlanError::NotACombatUnit);
    }
    let mut other = Fighter::of(auth, rules, defender).ok_or(PlanError::UnknownUnit)?;
    other.core = unit;
    other.mods = crate::effects::unit::unit_modifiers(auth, rules, &other.core);
    let target = other.core.tile;
    match kind {
        AttackKind::Melee => check_melee_reach(view, rules, &me, target)?,
        AttackKind::Ranged => check_ranged_reach(view, rules, &me, target)?,
    }
    if !auth.diplomacy().at_war(me.owner(), other.owner()) {
        return Err(PlanError::NotAtWar);
    }
    if !other.can_defend(auth, rules) {
        return Err(PlanError::NoTarget);
    }
    Ok(build_plan(view, rules, &me, target, kind, Defender::Unit(defender), Some(&other)))
}

fn check_ready(me: &Fighter<'_>, rules: &CompiledRuleset) -> Result<(), PlanError> {
    if rules.is_civilian(me.core.kind) || (me.base_combat() == 0 && me.base_ranged() == 0) {
        return Err(PlanError::NotACombatUnit);
    }
    if me.core.moves_left == 0 {
        return Err(PlanError::NoMovesLeft);
    }
    let attacks = 1 + me.mods.stat(UnitStat::AttacksPerTurn).max(0) as u8;
    if me.core.attacks_made >= attacks {
        return Err(PlanError::NoAttacksLeft);
    }
    Ok(())
}

fn check_melee_reach(
    view: GameView<'_>,
    rules: &CompiledRuleset,
    me: &Fighter<'_>,
    target: TileId,
) -> Result<(), PlanError> {
    let auth = view.auth;
    if me.mods.has(UnitAbility::OnlyDefensive) || me.is_air() || me.base_combat() == 0 {
        return Err(PlanError::OnlyDefensive);
    }
    if !auth.map().dimensions.adjacent(me.tile(), target) {
        return Err(PlanError::OutOfRange);
    }
    let city_here = auth.indexes().city_on_tile(target).is_some();
    if me.mods.has(UnitAbility::CityAttackOnly) && !city_here {
        return Err(PlanError::CitiesOnly);
    }
    let water = is_water(auth.map(), target);
    let ok = match me.domain() {
        // Embarked units fight water units with the embarked strength and
        // land with an amphibious landing; land units never strike water.
        Domain::Land => !water || me.is_embarked(auth),
        // Ships strike units at sea and coastal cities.
        Domain::Sea => water || city_here,
        Domain::Air => false,
    };
    if !ok {
        return Err(PlanError::WrongDomain);
    }
    let _ = rules;
    Ok(())
}

fn check_ranged_reach(
    view: GameView<'_>,
    rules: &CompiledRuleset,
    me: &Fighter<'_>,
    target: TileId,
) -> Result<(), PlanError> {
    let auth = view.auth;
    if me.base_ranged() == 0 || (me.range() == 0 && !me.mods.has(UnitAbility::RangedSupportFire)) {
        return Err(PlanError::NotRanged);
    }
    if me.is_embarked(auth) {
        return Err(PlanError::Embarked);
    }
    if me.mods.has(UnitAbility::MustSetUp) && me.core.stance != Stance::SetUp {
        return Err(PlanError::NotSetUp);
    }
    let dims = auth.map().dimensions;
    let distance = dims.distance(me.tile(), target).unwrap_or(i32::MAX);
    if distance > me.range().max(1) || distance == 0 {
        return Err(PlanError::OutOfRange);
    }
    if !crate::knowledge::view::is_visible(view.derived, me.owner(), target) {
        return Err(PlanError::NotVisible);
    }
    let indirect = me.mods.has(UnitAbility::IndirectFire) || me.is_air();
    if !indirect && !crate::derived::visibility::los_clear(auth.map(), me.tile(), target) {
        return Err(PlanError::NoLineOfSight);
    }
    if me.def.is_some_and(|d| d.combat.ranged_only_in_domain) {
        let water = is_water(auth.map(), target);
        let city = auth.indexes().city_on_tile(target).is_some();
        if !water || city {
            return Err(PlanError::WrongDomain);
        }
    }
    let _ = rules;
    Ok(())
}

/// The defender of the target tile: an enemy city first, then the enemy
/// combat unit with the best defense, then an enemy civilian.
fn pick_defender(
    view: GameView<'_>,
    rules: &CompiledRuleset,
    me: &Fighter<'_>,
    target: TileId,
    kind: AttackKind,
) -> Result<Defender, PlanError> {
    let auth = view.auth;
    let at_war = |owner: PlayerId| auth.diplomacy().at_war(me.owner(), owner);
    if let Some(city) = auth.indexes().city_on_tile(target) {
        let owner = auth
            .cities()
            .get(city)
            .map(|c| c.owner)
            .ok_or(PlanError::NoTarget)?;
        if owner == me.owner() {
            return Err(PlanError::NoTarget);
        }
        if !at_war(owner) {
            return Err(PlanError::NotAtWar);
        }
        return Ok(Defender::City(city));
    }
    let mut best: Option<(i64, UnitHandle)> = None;
    let mut civilian: Option<UnitHandle> = None;
    let mut foreign_peace = false;
    let mut units: Vec<UnitHandle> = auth.indexes().units_on(target).to_vec();
    units.sort_by_key(|h| auth.units().get(*h).map(|u| u.core.id));
    for unit in units {
        let Some(other) = Fighter::of(auth, rules, unit) else {
            continue;
        };
        if other.owner() == me.owner() {
            return Err(PlanError::NoTarget);
        }
        // An invisible unit the attacker's owner has not revealed is not a
        // target.
        if !crate::knowledge::view::unit_seen_by(auth, view.derived, rules, me.owner(), &other.core) {
            continue;
        }
        if !at_war(other.owner()) {
            foreign_peace = true;
            continue;
        }
        if other.is_air() {
            continue;
        }
        if !other.can_defend(auth, rules) {
            civilian = civilian.or(Some(unit));
            continue;
        }
        let strength = match kind {
            AttackKind::Melee => defense_strength(auth, rules, &other, Some(me), false),
            AttackKind::Ranged => defense_vs_ranged(auth, rules, &other, Some(me)),
        };
        if best.map(|(s, _)| strength > s).unwrap_or(true) {
            best = Some((strength, unit));
        }
    }
    if let Some((_, unit)) = best {
        return Ok(Defender::Unit(unit));
    }
    if let Some(unit) = civilian {
        return Ok(Defender::Civilian(unit));
    }
    if foreign_peace {
        return Err(PlanError::NotAtWar);
    }
    Err(PlanError::NoTarget)
}

fn build_plan(
    view: GameView<'_>,
    rules: &CompiledRuleset,
    me: &Fighter<'_>,
    target: TileId,
    kind: AttackKind,
    defender: Defender,
    as_if: Option<&Fighter<'_>>,
) -> AttackPlan {
    let auth = view.auth;
    let air = me.is_air();
    let attacker_wounded_mod = i64::from(me.mods.stat(UnitStat::WoundedDamagePercent));
    let mut plan = AttackPlan {
        attacker: me.handle,
        attacker_owner: me.owner(),
        kind,
        target_tile: target,
        defender,
        defender_owner: me.owner(),
        attacker_strength: 0,
        defender_strength: 0,
        attacker_hp: me.core.hp,
        defender_hp: 0,
        defender_max_hp: 0,
        attacker_damage: me.core.damage(),
        defender_damage: 0,
        attacker_wounded_mod,
        defender_wounded_mod: 0,
        air,
    };
    match defender {
        Defender::City(city) => {
            let core = auth.cities().get(city).expect("picked city exists");
            plan.defender_owner = core.owner;
            plan.defender_max_hp = crate::combat::city::max_hit_points(auth, rules, core);
            plan.defender_hp = plan.defender_max_hp.saturating_sub(core.damage);
            plan.defender_strength = crate::combat::city::city_strength(auth, rules, core);
            let city_target = MeleeTarget::City { city, tile: target };
            plan.attacker_strength = match kind {
                AttackKind::Melee => melee_attack_strength(auth, rules, me, city_target),
                AttackKind::Ranged => {
                    ranged_strength(auth, rules, me, None, Some(city), target, true)
                }
            };
        }
        Defender::Unit(unit) | Defender::Civilian(unit) => {
            // The defender as it would stand (`plan_strike_on`), else as it is.
            let other = match as_if {
                Some(fighter) => fighter.clone(),
                None => Fighter::of(auth, rules, unit).expect("picked unit exists"),
            };
            plan.defender_owner = other.owner();
            plan.defender_hp = other.core.hp;
            plan.defender_max_hp = crate::world::unit::UNIT_MAX_HP;
            plan.defender_damage = other.core.damage();
            plan.defender_wounded_mod = i64::from(other.mods.stat(UnitStat::WoundedDamagePercent));
            if matches!(defender, Defender::Unit(_)) {
                match kind {
                    AttackKind::Melee => {
                        plan.attacker_strength =
                            melee_attack_strength(auth, rules, me, MeleeTarget::Unit(&other));
                        plan.defender_strength =
                            defense_strength(auth, rules, &other, Some(me), false);
                    }
                    AttackKind::Ranged => {
                        plan.attacker_strength =
                            ranged_strength(auth, rules, me, Some(&other), None, target, true);
                        plan.defender_strength = defense_vs_ranged(auth, rules, &other, Some(me));
                    }
                }
            }
        }
    }
    plan
}

/// A city's ranged strike on a unit (§4.3, §2.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CityStrikePlan {
    pub city: CityHandle,
    pub target: UnitHandle,
    pub city_strength: i64,
    pub defender_strength: i64,
    pub defender_hp: u16,
    /// A civilian takes the fixed DAMAGE_TO_CIVILIAN_UNIT.
    pub civilian: bool,
}

impl CityStrikePlan {
    pub fn damage(&self, roll: Roll) -> u16 {
        self.raw_damage(roll).min(self.defender_hp)
    }

    /// The strike's damage with `roll` before the HP cap.
    fn raw_damage(&self, roll: Roll) -> u16 {
        if self.civilian {
            DAMAGE_TO_CIVILIAN_UNIT
        } else {
            city_strike_damage(self.city_strength, self.defender_strength, roll)
        }
    }

    /// The expected outcome shown before the strike (the city takes no
    /// answer; `city_hp` is the city's hit points).
    pub fn preview(&self, city_hp: u16) -> CombatPreview {
        let high = Roll::Drawn(crate::combat::damage::MELEE_EQUAL_STRENGTH_RANDOM_DAMAGE - 1);
        let to_defender = (self.damage(Roll::Drawn(0)), self.damage(Roll::Expected), self.damage(high));
        let share = i32::from(to_defender.1) * 100 / i32::from(self.defender_hp.max(1));
        let verdict = match share {
            _ if to_defender.0 >= self.defender_hp => Verdict::DecisiveVictory,
            s if s >= 40 => Verdict::MajorVictory,
            s if s >= 10 => Verdict::MinorVictory,
            _ => Verdict::Stalemate,
        };
        CombatPreview {
            attacker_strength: self.city_strength,
            defender_strength: self.defender_strength,
            to_defender,
            uncapped_to_defender: self.raw_damage(Roll::Expected),
            to_attacker: (0, 0, 0),
            attacker_hp: city_hp,
            defender_hp: self.defender_hp,
            verdict,
        }
    }
}

/// Plan a city's ranged strike on the best target of `tile`.
pub fn plan_city_strike(
    view: GameView<'_>,
    rules: &CompiledRuleset,
    city: CityHandle,
    tile: TileId,
) -> Result<CityStrikePlan, PlanError> {
    let auth = view.auth;
    let core = auth.cities().get(city).ok_or(PlanError::UnknownUnit)?;
    if core.strike_made {
        return Err(PlanError::NoAttacksLeft);
    }
    let max = crate::combat::city::max_hit_points(auth, rules, core);
    if core.damage >= max {
        return Err(PlanError::NoAttacksLeft);
    }
    let dims = auth.map().dimensions;
    let distance = dims.distance(core.tile, tile).unwrap_or(i32::MAX);
    if distance == 0 || distance > crate::combat::city::CITY_ATTACK_RANGE {
        return Err(PlanError::OutOfRange);
    }
    if !crate::knowledge::view::is_visible(view.derived, core.owner, tile) {
        return Err(PlanError::NotVisible);
    }
    let mut best: Option<(i64, UnitHandle, bool)> = None;
    let mut units: Vec<UnitHandle> = auth.indexes().units_on(tile).to_vec();
    units.sort_by_key(|h| auth.units().get(*h).map(|u| u.core.id));
    for unit in units {
        let Some(other) = Fighter::of(auth, rules, unit) else {
            continue;
        };
        let seen = crate::knowledge::view::unit_seen_by(auth, view.derived, rules, core.owner, &other.core);
        if !auth.diplomacy().at_war(core.owner, other.owner()) || other.is_air() || !seen {
            continue;
        }
        let civilian = !other.can_defend(auth, rules);
        let strength = if civilian {
            0
        } else {
            defense_vs_ranged(auth, rules, &other, None)
        };
        let better = match best {
            None => true,
            Some((s, _, was_civilian)) => {
                (was_civilian && !civilian) || (!civilian && strength > s)
            }
        };
        if better {
            best = Some((strength, unit, civilian));
        }
    }
    let (strength, target, civilian) = best.ok_or(PlanError::NoTarget)?;
    let hp = auth.units().get(target).map(|u| u.core.hp).unwrap_or(0);
    Ok(CityStrikePlan {
        city,
        target,
        city_strength: crate::combat::city::city_ranged_strength(auth, rules, core),
        defender_strength: strength,
        defender_hp: hp,
        civilian,
    })
}

/// Why the unit may not fortify (§1.4), None when it may: combat land
/// units with a defensive bonus, on land, not fortified yet (rules.md,
/// fortifying).
pub fn fortify_problem(
    auth: &crate::world::state::AuthWorld,
    rules: &CompiledRuleset,
    core: &crate::world::unit::UnitCore,
) -> Option<&'static str> {
    if core.stance.is_fortified() {
        return Some("already fortified");
    }
    if rules.is_civilian(core.kind) {
        return Some("not a combat unit");
    }
    let mods = crate::effects::unit::unit_modifiers(auth, rules, core);
    let land = rules
        .units
        .get(core.kind.0 as usize)
        .is_some_and(|d| d.domain == Domain::Land);
    if mods.has(UnitAbility::NoDefensiveBonus) || !land || is_water(auth.map(), core.tile) {
        return Some("cannot fortify");
    }
    None
}

/// Attacks the unit may still make this turn (0 without moves left).
pub fn attacks_left(
    auth: &crate::world::state::AuthWorld,
    rules: &CompiledRuleset,
    core: &crate::world::unit::UnitCore,
) -> u8 {
    if core.moves_left == 0 || rules.is_civilian(core.kind) {
        return 0;
    }
    let mods = crate::effects::unit::unit_modifiers(auth, rules, core);
    let attacks = 1 + mods.stat(UnitStat::AttacksPerTurn).clamp(0, 9) as u8;
    attacks.saturating_sub(core.attacks_made)
}
