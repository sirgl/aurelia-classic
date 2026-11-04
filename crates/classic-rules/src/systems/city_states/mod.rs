//! City-states (classic/docs/classic/rules.md): influence and
//! its decay to the resting point, friends and allies with their bonuses,
//! first-contact gifts, quests, gifts of gold, units and improvements,
//! bullying, pledges to protect, militaristic unit gifts, alliance wars.
//!
//! Influence is stored ×100 per (city-state, nation) in
//! `world::city_states`; the effective influence at war is −60. The ally is
//! recomputed on every change (strictly more influence takes the alliance).
//! Journal: PlayerChange::EffectsChanged for nations whose friend or
//! ally status changed (the maritime food lives in city yields),
//! RelationsChanged otherwise, WarDeclared when a city-state follows its
//! ally into a war.

pub(crate) mod block;
pub(crate) mod buyout;
pub(crate) mod gifts;
pub(crate) mod quests;
pub(crate) mod turn;
pub(crate) mod validate;

use crate::ids::PlayerId;
use crate::classic::tables::CityStateTypeDefinition;
use crate::rules::content::{CityStateBonusKind, CityStateLevel};
use crate::rules::CompiledRuleset;
use crate::transaction::journal::PlayerChange;
use crate::transaction::transaction::WorldTransaction;
use crate::world::city::CityCore;
use crate::world::state::AuthWorld;
use crate::world::player::ClassicKind;
use crate::world::player::ClassicPlayers;

/// Influence at which a city-state counts a major as a friend and as an
/// ally (the types carry the same). Unciv: DiplomacyManager.relationshipLevel,
/// 30 and 60; equal.
pub const FRIENDS_THRESHOLD: i32 = 30;
/// Unciv: DiplomacyManager.relationshipLevel (60), equal.
pub const ALLIES_THRESHOLD: i32 = 60;
/// Effective influence at war, the floor. Unciv: DiplomacyManager,
/// MINIMUM_INFLUENCE, -60; equal.
pub const AT_WAR_INFLUENCE: i32 = -60;
/// Shift of the resting point for a major the city-state is protected by,
/// and for one it is wary of. Unciv: logic/nation/diplomacy/
/// DiplomacyManager.kt (`getCityStateInfluenceRestingPoint`: protector +10,
/// wary -20); equal.
pub const ANCHOR_PROTECTED: i32 = 10;
/// Unciv: DiplomacyManager.kt (wary -20), equal.
pub const ANCHOR_WARY: i32 = -20;
/// Attacks on city-states after which a major counts as an aggressor, and
/// as a warmonger. Unciv: logic/nation/Nation.kt
/// (`isMinorCivAggressor` 2, `isMinorCivWarmonger` 4); equal.
pub const AGGRESSOR_THRESHOLD: u8 = 2;
/// Unciv: Nation.kt (`isMinorCivWarmonger` 4), equal.
pub const WARMONGER_THRESHOLD: u8 = 4;
/// Chance (percent) that an aggressor makes the city-state wary of it.
/// Unciv: CityStateFunctions.kt (`cityStateAttacked`, 50 %); equal.
pub const WARY_AGGRESSOR_CHANCE: u64 = 50;

/// The type of a city-state player (None: not a city-state).
pub fn type_of<'r>(
    auth: &AuthWorld,
    rules: &'r CompiledRuleset,
    city_state: PlayerId,
) -> Option<&'r CityStateTypeDefinition> {
    let crate::world::player::PlayerKind::Module(ClassicKind::CityState(id)) = auth.players().kind_of(city_state) else {
        return None;
    };
    let def = rules.module.city_states.get(id.0 as usize)?;
    rules.module.city_state_types.get(def.city_state_type.0 as usize)
}

/// Effective influence in points: −60 at war, else the base.
pub fn effective_influence(auth: &AuthWorld, city_state: PlayerId, major: PlayerId) -> i32 {
    if auth.diplomacy().at_war(city_state, major) {
        return AT_WAR_INFLUENCE;
    }
    auth.module().city_states().influence(city_state, major)
}

fn thresholds(auth: &AuthWorld, rules: &CompiledRuleset, city_state: PlayerId) -> (i32, i32) {
    type_of(auth, rules, city_state)
        .map(|t| (t.friends_threshold, t.allies_threshold))
        .unwrap_or((FRIENDS_THRESHOLD, ALLIES_THRESHOLD))
}

pub fn is_friends(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    city_state: PlayerId,
    major: PlayerId,
) -> bool {
    effective_influence(auth, city_state, major) >= thresholds(auth, rules, city_state).0
}

pub fn is_ally(auth: &AuthWorld, city_state: PlayerId, major: PlayerId) -> bool {
    auth.module().city_states().ally(city_state) == Some(major)
}

/// The resting point of the influence (§4.3): 0, +5 under a pledge,
/// −20 when the city-state is wary.
pub fn anchor(auth: &AuthWorld, city_state: PlayerId, major: PlayerId) -> i32 {
    let standing = auth.module().city_states().standing(city_state, major);
    let protected = if standing.pledged.is_some() {
        ANCHOR_PROTECTED
    } else {
        0
    };
    let wary = if standing.wary { ANCHOR_WARY } else { 0 };
    protected + wary
}

/// Every city-state number of a major, index = `CityStateStat::index()`.
pub type CityStateStats = [i32; crate::classic::effects::CITY_STATE_STATS];

/// All of `major`'s city-state numbers in one evaluation.
pub fn stats(auth: &AuthWorld, rules: &CompiledRuleset, major: PlayerId) -> CityStateStats {
    crate::effects::player::player_modifiers(auth, rules, major).module.city_state
}

/// A city-state number `major`'s policies, beliefs and traits change
/// (`CityStateStat`).
pub fn stat(auth: &AuthWorld, rules: &CompiledRuleset, major: PlayerId, stat: crate::classic::effects::CityStateStat) -> i32 {
    crate::effects::player::player_modifiers(auth, rules, major).module.city_state[stat.index()]
}

/// The resting point with `major`'s own changes (Consulates: +20), and its
/// founder belief when the city-state's capital follows the religion
/// `major` founded (Papal Primacy: +15; rules.md).
pub fn resting_point(auth: &AuthWorld, rules: &CompiledRuleset, city_state: PlayerId, major: PlayerId) -> i32 {
    use crate::classic::effects::CityStateStat;
    let mods = crate::effects::player::player_modifiers(auth, rules, major);
    let mut own = mods.module.city_state[CityStateStat::RestingPoint.index()];
    let same_religion = mods.module.city_state[CityStateStat::RestingPointSameReligion.index()];
    if same_religion != 0 && follows_religion_of(auth, rules, city_state, major) {
        own += same_religion;
    }
    (anchor(auth, city_state, major) + own).max(AT_WAR_INFLUENCE)
}

/// The city-state's capital has the religion `major` founded as majority.
pub(super) fn follows_religion_of(auth: &AuthWorld, rules: &CompiledRuleset, city_state: PlayerId, major: PlayerId) -> bool {
    let Some(religion) = auth.module().religion().player(major).religion else {
        return false;
    };
    // A city-state without a Palace: its first city is its capital.
    let capital = crate::world::city::capital_of(auth, rules, city_state)
        .or_else(|| auth.indexes().cities_of(city_state).first().copied());
    let Some(capital) = capital else {
        return false;
    };
    let majority = crate::derived::religion::city_majority(auth, capital);
    majority == Some(crate::world::religion::Creed::Religion(religion))
}

/// Every city-state player.
pub fn city_states(auth: &AuthWorld) -> Vec<PlayerId> {
    (0..auth.players().count())
        .map(|p| PlayerId(p as u8))
        .filter(|p| auth.players().is_city_state(*p))
        .collect()
}

/// Every alive nation.
pub fn majors(auth: &AuthWorld) -> Vec<PlayerId> {
    (0..auth.players().count())
        .map(|p| PlayerId(p as u8))
        .filter(|p| auth.players().is_major(*p) && auth.players().is_alive(*p))
        .collect()
}

/// City-states allied to `major`.
pub(crate) fn allies_of(auth: &AuthWorld, major: PlayerId) -> Vec<PlayerId> {
    city_states(auth)
        .into_iter()
        .filter(|cs| auth.players().is_alive(*cs) && is_ally(auth, *cs, major))
        .collect()
}

/// Peace between `a` and `b` is blocked: one is a city-state whose ally
/// is at war with the other (permanent war is off).
pub(crate) fn peace_blocked(auth: &AuthWorld, a: PlayerId, b: PlayerId) -> bool {
    let blocked = |cs: PlayerId, other: PlayerId| {
        auth.module().city_states()
            .ally(cs)
            .is_some_and(|ally| ally != other && auth.diplomacy().at_war(ally, other))
    };
    (auth.players().is_city_state(a) && blocked(a, b))
        || (auth.players().is_city_state(b) && blocked(b, a))
}

/// The bonus of `kind` a city-state type gives at `level` in the major's
/// era: the entry of the latest era reached (an entry without era counts
/// from the start).
pub fn bonus_amount(
    rules: &CompiledRuleset,
    def: &CityStateTypeDefinition,
    level: CityStateLevel,
    kind: CityStateBonusKind,
    era_index: u8,
) -> i32 {
    let era_of = |b: &crate::classic::tables::CityStateBonus| {
        b.era
            .and_then(|e| rules.eras.get(e.0 as usize))
            .map(|e| e.index)
            .unwrap_or(0)
    };
    def.bonuses
        .iter()
        .filter(|b| b.level == level && b.kind == kind && era_of(b) <= era_index)
        .max_by_key(|b| era_of(b))
        .map(|b| b.amount)
        .unwrap_or(0)
}

/// The friend (and ally) bonus of `kind` one city-state gives `major` now.
pub fn bonus_from(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    city_state: PlayerId,
    major: PlayerId,
    kind: CityStateBonusKind,
) -> i32 {
    let Some(def) = type_of(auth, rules, city_state) else {
        return 0;
    };
    if !auth.players().is_alive(city_state) {
        return 0;
    }
    let era = crate::combat::city::current_era(auth, rules, major)
        .map(|e| e.index)
        .unwrap_or(0);
    let mut total = 0;
    if is_friends(auth, rules, city_state, major) {
        total += bonus_amount(rules, def, CityStateLevel::Friends, kind, era);
    }
    if is_ally(auth, city_state, major) {
        total += bonus_amount(rules, def, CityStateLevel::Allies, kind, era);
    }
    // Siam: +50 % food, culture and faith of friends and allies.
    let scaled = matches!(
        kind,
        CityStateBonusKind::CapitalFood
            | CityStateBonusKind::AllCitiesFood
            | CityStateBonusKind::Culture
            | CityStateBonusKind::Faith
    );
    if scaled && total != 0 {
        let percent = stat(auth, rules, major, crate::classic::effects::CityStateStat::FriendBonusPercent);
        total = total * (100 + percent) / 100;
    }
    total
}

/// The bonus of `kind` all city-states give `major` (culture, faith,
/// happiness: for the systems that own those channels).
pub fn player_bonus(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    major: PlayerId,
    kind: CityStateBonusKind,
) -> i32 {
    city_states(auth)
        .into_iter()
        .map(|cs| bonus_from(auth, rules, cs, major, kind))
        .sum()
}

/// Maritime food on the city's tile (§4.6): capital and all-cities bonus
/// ×100 of every city-state, rounded down once per city.
pub fn city_food_bonus(auth: &AuthWorld, rules: &CompiledRuleset, city: &CityCore) -> i32 {
    let minors = city_states(auth);
    if minors.is_empty() || !auth.players().is_major(city.owner) {
        return 0;
    }
    let capital = crate::combat::city::capital_of(auth, rules, city.owner)
        .and_then(|h| auth.cities().get(h))
        .is_some_and(|c| c.tile == city.tile);
    let mut times100 = 0;
    for cs in minors {
        times100 += bonus_from(
            auth,
            rules,
            cs,
            city.owner,
            CityStateBonusKind::AllCitiesFood,
        );
        if capital {
            times100 += bonus_from(auth, rules, cs, city.owner, CityStateBonusKind::CapitalFood);
        }
    }
    times100 / 100
}

impl<'a> WorldTransaction<'a> {
    /// Change the base influence by `delta` ×100 (floor −60), then the
    /// statuses.
    pub(crate) fn change_influence(
        &mut self,
        city_state: PlayerId,
        major: PlayerId,
        delta: i32,
        rules: &CompiledRuleset,
    ) {
        let old = self.world.module.city_states.standing(city_state, major).influence;
        self.set_influence(city_state, major, old.saturating_add(delta), rules);
    }

    /// Set the base influence ×100 (floor −60), then the statuses.
    pub(crate) fn set_influence(
        &mut self,
        city_state: PlayerId,
        major: PlayerId,
        value: i32,
        rules: &CompiledRuleset,
    ) {
        let was_friend = is_friends(self.world, rules, city_state, major);
        if let Some(s) = self.world.module.city_states.standing_mut(city_state, major) {
            s.influence = value.max(AT_WAR_INFLUENCE * 100);
        }
        let friend = is_friends(self.world, rules, city_state, major);
        if friend && !was_friend {
            self.became_friend(city_state, major, rules);
        }
        if friend != was_friend {
            self.journal
                .player_changes
                .push(PlayerChange::EffectsChanged { player: major });
        }
        self.relations_changed(major);
        self.update_ally(city_state, rules);
    }

    fn became_friend(&mut self, city_state: PlayerId, major: PlayerId, rules: &CompiledRuleset) {
        let seed = self
            .world
            .module
            .city_states
            .standing(city_state, major)
            .spawn_counter
            < 0;
        if let Some(s) = self.world.module.city_states.standing_mut(city_state, major) {
            s.ever_friends = true;
        }
        if seed {
            self.seed_unit_gift(city_state, major, true, rules);
        }
    }

    /// The ally (§4.2): the current one keeps the alliance while above the
    /// threshold unless another has strictly more; otherwise the one with
    /// the most influence above the threshold (lowest id on a tie). Unciv:
    /// CityStateFunctions.kt (`updateAllyCivForCityState`: the most
    /// influence, at least 60); the tie-break is our own (Unciv takes the
    /// first maximum in its map order, which is not a game rule).
    pub(crate) fn update_ally(&mut self, city_state: PlayerId, rules: &CompiledRuleset) {
        let allies = thresholds(self.world, rules, city_state).1;
        let current = self.world.module.city_states.ally(city_state);
        let score = |p: PlayerId| effective_influence(self.world, city_state, p);
        let best = majors(self.world)
            .into_iter()
            .filter(|p| score(*p) >= allies)
            .max_by_key(|p| (score(*p), std::cmp::Reverse(p.0)));
        let next = match (current, best) {
            (Some(c), Some(b)) if score(c) >= allies && score(b) <= score(c) => Some(c),
            (_, best) => best,
        };
        if next != current {
            self.set_ally(city_state, current, next, rules);
        }
    }

    fn set_ally(
        &mut self,
        city_state: PlayerId,
        old: Option<PlayerId>,
        new: Option<PlayerId>,
        rules: &CompiledRuleset,
    ) {
        let round = self.world.turn.round;
        if let Some(minor) = self.world.module.city_states.minor_mut(city_state) {
            minor.ally = new;
            minor.allied_since = round;
        }
        for p in old.into_iter().chain(new) {
            self.journal
                .player_changes
                .push(PlayerChange::EffectsChanged { player: p });
        }
        if let Some(ally) = new {
            self.join_wars_of(city_state, ally, rules);
        }
    }

    /// A new ally's wars become the city-state's (§4.5).
    fn join_wars_of(&mut self, city_state: PlayerId, ally: PlayerId, _rules: &CompiledRuleset) {
        let enemies: Vec<PlayerId> = (0..self.world.players.count())
            .map(|p| PlayerId(p as u8))
            .filter(|p| *p != city_state && self.world.players.is_alive(*p))
            .filter(|p| {
                self.world.diplomacy.at_war(ally, *p)
                    && !self.world.diplomacy.at_war(city_state, *p)
            })
            .collect();
        for enemy in enemies {
            self.set_war_state(city_state, enemy, true);
            self.journal.player_changes.push(PlayerChange::WarDeclared {
                player: city_state,
                enemy,
            });
        }
    }

    /// A nation declared war on a city-state (§4.5): influence −60,
    /// one more city-state attacked, the city-state may become wary, the
    /// pledge and the quests of the pair end.
    pub(crate) fn war_on_city_state(
        &mut self,
        major: PlayerId,
        city_state: PlayerId,
        rules: &CompiledRuleset,
    ) {
        let attacked = match self.world.module.city_states.major_mut(major) {
            Some(record) => {
                record.minors_attacked = record.minors_attacked.saturating_add(1);
                record.minors_attacked
            }
            None => return,
        };
        let wary = attacked >= WARMONGER_THRESHOLD
            || (attacked >= AGGRESSOR_THRESHOLD
                && self.world.rng.events.below(100) < WARY_AGGRESSOR_CHANCE);
        let round = self.world.turn.round;
        if let Some(s) = self.world.module.city_states.standing_mut(city_state, major) {
            s.wary |= wary;
            if s.pledged.take().is_some() {
                // A war ends the pledge as a forced withdrawal: no new
                // pledge for `PLEDGE_AFTER_BROKEN` rounds (Unciv:
                // DeclareWar.kt, CityStateFunctions.kt).
                s.pledge_broken = Some(round);
            }
        }
        self.world
            .module
            .city_states
            .quests
            .retain(|q| !(q.city_state == city_state && q.major == major));
        self.set_influence(city_state, major, AT_WAR_INFLUENCE * 100, rules);
    }
}
