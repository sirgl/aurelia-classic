//! The city-state's own turn (§4): first-contact
//! gifts, influence decay and recovery, quests, militaristic unit gifts,
//! intrusion. A nation's turn counts down to the great person its
//! allied city-states give (Patronage finisher, §4.7).
//!
//! ```text
//! seed    37 + max(-10, 1 - allies) + rand(7) turns, halved the first time
//! turn    -1; at 0 a random allied city-state gives a random great person
//!         (not a prophet) at the nation's city nearest to it, and
//!         the count starts again; a new ally takes a turn off (at least 1)
//! ```

use super::{is_friends, majors, type_of};
use crate::classic::effects::CityStateStat;
use crate::ids::{PlayerId, UnitKindId};
use crate::rules::content::{CityStateBonusKind, CityStateLevel, Domain, UnitRole};
use crate::rules::CompiledRuleset;
use crate::transaction::transaction::WorldTransaction;
use crate::world::city_states::Personality;
use crate::world::player::ClassicPlayers;

/// Influence change per round, ×100: the decay above the resting point, its
/// hostile and aggressor variants (hostile wins), and the recovery below
/// it. Unciv: logic/nation/diplomacy/DiplomacyManager.kt
/// (`getCityStateInfluenceDegrade`: 1, 1.5, 2) and DiplomacyTurnManager.kt
/// (`getCityStateInfluenceRecovery`: 1); equal.
pub const DROP_PER_TURN: i32 = -100;
/// Unciv: DiplomacyManager.kt and DiplomacyTurnManager.kt, equal.
pub const DROP_PER_TURN_HOSTILE: i32 = -150;
/// Unciv: DiplomacyManager.kt and DiplomacyTurnManager.kt, equal.
pub const DROP_PER_TURN_AGGRESSOR: i32 = -200;
/// Unciv: DiplomacyManager.kt and DiplomacyTurnManager.kt, equal.
pub const RECOVERY_PER_TURN: i32 = 100;
/// Gold for the first major to meet a city-state, and for each later one;
/// faith from a city-state that gives faith as a bonus (Religious).
/// Unciv: logic/nation/diplomacy/DiplomacyFunctions.kt
/// (`makeNationsMeet`: 15 gold, doubled for the first major; 4
/// faith); equal.
pub const CONTACT_GOLD_FIRST: u32 = 30;
/// Unciv: DiplomacyFunctions.kt (`makeNationsMeet`), equal.
pub const CONTACT_GOLD_OTHER: u32 = 15;
/// Unciv: DiplomacyFunctions.kt (`makeNationsMeet`), equal.
pub const CONTACT_FAITH: u32 = 4;
/// Influence lost (×100 per round) while any military unit of a
/// non-friend stands on the city-state's land, whatever their number.
/// Unciv: logic/nation/diplomacy/CityStateFunctions.kt
/// (`updateDiplomaticRelationshipForCityState`, -10); equal.
pub const INTRUSION_PER_UNIT: i32 = -1000;
/// Percent of the full interval the first gifted unit waits. Unciv:
/// logic/nation/diplomacy/DiplomacyTurnManager.kt (the first
/// countdown is the full `N + -1..1` turns); equal.
pub const UNIT_SPAWN_BIAS: i32 = 100;
/// Percent change of the decay, and of the recovery, while the
/// city-state's capital follows the major's religion as majority. Unciv:
/// logic/nation/diplomacy/DiplomacyManager.kt and
/// DiplomacyTurnManager.kt (-25 % decay, +50 % recovery); equal.
pub const SAME_RELIGION_DECAY_PERCENT: i32 = -25;
/// Unciv: DiplomacyTurnManager.kt (+50 % recovery), equal.
pub const SAME_RELIGION_RECOVERY_PERCENT: i32 = 50;

impl<'a> WorldTransaction<'a> {
    /// The `CityStates` turn step: only a city-state's own turn does work.
    pub(crate) fn city_state_turn(&mut self, player: PlayerId, rules: &CompiledRuleset) {
        if self.world.players.is_major(player) {
            self.ally_great_person_turn(player, rules);
            return;
        }
        if !self.world.players.is_city_state(player) || !self.world.players.is_alive(player) {
            return;
        }
        for major in majors(self.world) {
            self.contact_gift(player, major, rules);
            self.influence_drift(player, major, rules);
            self.unit_gift_turn(player, major, rules);
            self.intrusion(player, major, rules);
        }
        self.update_ally(player, rules);
        self.city_state_election(player, rules);
        self.quests_turn(player, rules);
    }

    /// First contact (§4.6): 30 gold to the first nation met, 15 to
    /// the others, unless it is an aggressor against city-states.
    fn contact_gift(&mut self, city_state: PlayerId, major: PlayerId, rules: &CompiledRuleset) {
        let met = crate::systems::diplomacy::has_met(self.world, city_state, major);
        let standing = self.world.module.city_states.standing(city_state, major);
        if !met || standing.contacted {
            return;
        }
        let first = !(0..self.world.players.count()).any(|p| {
            self.world
                .module
                .city_states
                .standing(city_state, PlayerId(p as u8))
                .contacted
        });
        if let Some(s) = self.world.module.city_states.standing_mut(city_state, major) {
            s.contacted = true;
        }
        if self.world.module.city_states.major(major).minors_attacked >= super::AGGRESSOR_THRESHOLD {
            return;
        }
        let gold = if first {
            CONTACT_GOLD_FIRST
        } else {
            CONTACT_GOLD_OTHER
        };
        self.world.players.earn(major, gold);
        let gives_faith = type_of(self.world, rules, city_state).is_some_and(|t| {
            t.bonuses
                .iter()
                .any(|b| b.kind == CityStateBonusKind::Faith && b.amount > 0)
        });
        if gives_faith {
            if let Some(state) = self.world.module.religion.player_mut(major) {
                state.faith = state.faith.saturating_add(CONTACT_FAITH);
            }
        }
    }

    /// Decay towards the resting point from above, recovery from below,
    /// plus the major's per-turn shifts (§4.4); decay stops at the resting
    /// point.
    fn influence_drift(&mut self, city_state: PlayerId, major: PlayerId, rules: &CompiledRuleset) {
        let base = self.world.module.city_states.influence(city_state, major);
        let rest = super::resting_point(self.world, rules, city_state, major);
        let stats = super::stats(self.world, rules, major);
        let drift = match base.cmp(&rest) {
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => {
                let same = super::follows_religion_of(self.world, rules, city_state, major);
                let extra = if same { SAME_RELIGION_DECAY_PERCENT } else { 0 };
                self.decay_rate(city_state, major, &stats, extra)
            }
            std::cmp::Ordering::Less => {
                let same = super::follows_religion_of(self.world, rules, city_state, major);
                recovery_rate(&stats, if same { SAME_RELIGION_RECOVERY_PERCENT } else { 0 })
            }
        };
        let change = drift + self.influence_shift(city_state, major, &stats, rules);
        if change == 0 {
            return;
        }
        let next = base + change / 100;
        if base >= rest && next < rest {
            self.set_influence(city_state, major, rest * 100, rules);
        } else {
            self.change_influence(city_state, major, change, rules);
        }
    }

    /// Influence ×100 a met major gains every turn on top of the drift
    /// (§4.4): Treaty Organization with a city-state a trade route
    /// connects, Gunboat Diplomacy with one afraid of it (the bully score
    /// for gold is at least 0).
    fn influence_shift(
        &self,
        city_state: PlayerId,
        major: PlayerId,
        stats: &super::CityStateStats,
        rules: &CompiledRuleset,
    ) -> i32 {
        if !crate::systems::diplomacy::has_met(self.world, city_state, major) {
            return 0;
        }
        let mut shift = 0;
        let trade = stats[CityStateStat::TradeRouteInfluence.index()];
        if trade != 0 && crate::derived::tourism::trade_connected(self.world, major, city_state) {
            shift += trade * 100;
        }
        let afraid = stats[CityStateStat::AfraidInfluence.index()];
        if afraid != 0 && super::gifts::bully_score(self.world, rules, city_state, major, false) >= 0 {
            shift += afraid * 100;
        }
        shift
    }

    /// Decay ×100 a turn (§4.4): hostile, aggressor or plain, times
    /// `100 + DecayPercent - FriendshipRate / 2` percent.
    fn decay_rate(&self, city_state: PlayerId, major: PlayerId, stats: &super::CityStateStats, extra: i32) -> i32 {
        let base = if self.world.module.city_states.minor(city_state).personality == Personality::Hostile {
            DROP_PER_TURN_HOSTILE
        } else if self.world.module.city_states.major(major).minors_attacked >= super::AGGRESSOR_THRESHOLD
        {
            DROP_PER_TURN_AGGRESSOR
        } else {
            DROP_PER_TURN
        };
        let decay = stats[CityStateStat::DecayPercent.index()];
        let rate = stats[CityStateStat::FriendshipRate.index()];
        base * (100 + decay - rate / 2 + extra).max(0) / 100
    }

    /// Combat units of a non-friend on the city-state's land cost it
    /// influence each turn (§4.11), unless at war.
    fn intrusion(&mut self, city_state: PlayerId, major: PlayerId, rules: &CompiledRuleset) {
        if self.world.diplomacy.at_war(city_state, major)
            || is_friends(self.world, rules, city_state, major)
        {
            return;
        }
        let intruders = self
            .world
            .indexes
            .units_of(major)
            .iter()
            .filter_map(|u| self.world.units.get(*u))
            .filter(|u| !rules.is_civilian(u.core.kind))
            .filter(|u| self.world.map.owner(u.core.tile).flatten() == Some(city_state))
            .count() as i32;
        if intruders > 0 {
            self.change_influence(city_state, major, INTRUSION_PER_UNIT, rules);
        }
    }

    /// Militaristic city-states gift friends a unit every few turns (§4.8).
    fn unit_gift_turn(&mut self, city_state: PlayerId, major: PlayerId, rules: &CompiledRuleset) {
        let friend = is_friends(self.world, rules, city_state, major);
        let counter = self
            .world
            .module
            .city_states
            .standing(city_state, major)
            .spawn_counter;
        let base = self.base_gift_turns(city_state, major, rules);
        if !friend || counter < 0 || base == 0 {
            return;
        }
        // A count longer than the current level allows (a friend became an
        // ally) starts again (Unciv: DiplomacyTurnManager.kt).
        let random = type_of(self.world, rules, city_state)
            .map(|d| {
                super::bonus_amount(rules, d, CityStateLevel::Friends, CityStateBonusKind::UnitGiftRandom, 0)
            })
            .unwrap_or(0)
            .max(1);
        if i32::from(counter) > base + random - 1 {
            self.seed_unit_gift(city_state, major, false, rules);
            return;
        }
        let left = (counter - 1).max(0);
        if let Some(s) = self.world.module.city_states.standing_mut(city_state, major) {
            s.spawn_counter = left;
        }
        if left == 0 {
            self.gift_unit_to(city_state, major, rules);
            self.seed_unit_gift(city_state, major, false, rules);
        }
    }

    /// Friends' base turns plus the allies' change (0: no unit gifts),
    /// shortened by United Front while the major and the city-state share
    /// an enemy: `turns × 100 / (100 + percent)` (§4.8).
    fn base_gift_turns(
        &self,
        city_state: PlayerId,
        major: PlayerId,
        rules: &CompiledRuleset,
    ) -> i32 {
        let Some(def) = type_of(self.world, rules, city_state) else {
            return 0;
        };
        let friends = super::bonus_amount(
            rules,
            def,
            CityStateLevel::Friends,
            CityStateBonusKind::UnitGiftTurns,
            0,
        );
        if friends == 0 {
            return 0;
        }
        let allies = super::bonus_amount(
            rules,
            def,
            CityStateLevel::Allies,
            CityStateBonusKind::UnitGiftTurns,
            0,
        );
        let ally = super::is_ally(self.world, city_state, major);
        let turns = friends + if ally { allies } else { 0 };
        let faster = super::stat(self.world, rules, major, CityStateStat::UnitGiftRatePercent);
        if faster > 0 && common_enemy(self.world, major, city_state) {
            return turns * 100 / (100 + faster);
        }
        turns
    }

    /// Rounds until the next unit gift: base + random rounds, halved for the first.
    pub(crate) fn seed_unit_gift(
        &mut self,
        city_state: PlayerId,
        major: PlayerId,
        first: bool,
        rules: &CompiledRuleset,
    ) {
        let base = self.base_gift_turns(city_state, major, rules);
        if base == 0 {
            return;
        }
        let random = type_of(self.world, rules, city_state)
            .map(|d| {
                super::bonus_amount(
                    rules,
                    d,
                    CityStateLevel::Friends,
                    CityStateBonusKind::UnitGiftRandom,
                    0,
                )
            })
            .unwrap_or(0)
            .max(1);
        let mut turns = base + self.world.rng.events.below(random as u64) as i32;
        if first {
            turns = turns * UNIT_SPAWN_BIAS / 100;
        }
        if let Some(s) = self.world.module.city_states.standing_mut(city_state, major) {
            s.spawn_counter = turns.clamp(1, i16::MAX as i32) as i16;
        }
    }

    /// The gifted unit appears at the friend's city nearest to the
    /// city-state's capital.
    fn gift_unit_to(&mut self, city_state: PlayerId, major: PlayerId, rules: &CompiledRuleset) {
        let Some(kind) = competitive_unit(self.world, rules, major) else {
            return;
        };
        let Some(from) = crate::combat::city::capital_of(self.world, rules, city_state)
            .and_then(|c| self.world.cities.get(c))
            .map(|c| c.tile)
        else {
            return;
        };
        let dims = self.world.map.dimensions;
        let spot = self
            .world
            .indexes
            .cities_of(major)
            .iter()
            .filter_map(|c| self.world.cities.get(*c))
            .map(|c| c.tile)
            .min_by_key(|t| (dims.distance(*t, from).unwrap_or(i32::MAX), t.0))
            .unwrap_or(from);
        let tile = self.free_tile_near(major, spot, kind, rules);
        if crate::world::stacking::can_stop_at(self.world, rules, major, kind, None, tile).is_ok() {
            self.spawn_unit(major, tile, kind, rules);
        }
    }
}

/// Great person gifts from allied city-states: base rounds, random extra
/// rounds (0..6), the percent for the first gift, and the cap on the
/// rounds each ally takes off (the gift comes once the count is below
/// `min(allies, 10)`; the count runs only while there is an ally). Unciv:
/// logic/nation/diplomacy/CityStateFunctions.kt
/// (`turnsForGreatPersonFromCityState`), managers/TurnManager.kt and
/// UniqueTriggerActivation.kt (first count halved); equal.
pub const ALLY_GREAT_PERSON_BASE: i32 = 37;
/// Unciv: CityStateFunctions.kt, equal.
pub const ALLY_GREAT_PERSON_RANDOM: u64 = 7;
/// Unciv: CityStateFunctions.kt, equal.
pub const ALLY_GREAT_PERSON_FIRST_PERCENT: i32 = 50;
/// Unciv: CityStateFunctions.kt, equal.
pub const ALLY_GREAT_PERSON_MAX_ALLIES_CHANGE: i32 = -10;

impl<'a> WorldTransaction<'a> {
    /// Patronage finisher: count down while the
    /// major has the effect; at 0 an ally gives a great person.
    fn ally_great_person_turn(&mut self, major: PlayerId, rules: &CompiledRuleset) {
        if super::stat(self.world, rules, major, CityStateStat::AllyGreatPeople) <= 0 {
            return;
        }
        let turns = self.world.module.city_states.major(major).ally_great_person_turns;
        if turns == 0 {
            self.seed_ally_great_person(major, true);
            return;
        }
        let allies = super::allies_of(self.world, major).len() as i32;
        if allies == 0 {
            return;
        }
        let left = turns - 1;
        self.set_ally_great_person_turns(major, left.max(1));
        if i32::from(left) >= allies.min(-ALLY_GREAT_PERSON_MAX_ALLIES_CHANGE) {
            return;
        }
        self.ally_gives_great_person(major, rules);
        self.seed_ally_great_person(major, false);
    }

    /// Rounds until the next great person gift, × the game speed's pace
    /// (Unciv: turnsForGreatPersonFromCityState, `modifier`; equal).
    fn seed_ally_great_person(&mut self, major: PlayerId, first: bool) {
        let random = self.world.rng.events.below(ALLY_GREAT_PERSON_RANDOM) as i32;
        let pace = self.world.settings().speed_values().pace_percent;
        let mut turns = kernel::rules::speeds::SpeedValues::scale_i32(ALLY_GREAT_PERSON_BASE + random, pace);
        if first {
            turns = turns * ALLY_GREAT_PERSON_FIRST_PERCENT / 100;
        }
        self.set_ally_great_person_turns(major, turns.max(1) as u16);
    }

    fn set_ally_great_person_turns(&mut self, major: PlayerId, turns: u16) {
        if let Some(record) = self.world.module.city_states.major_mut(major) {
            record.ally_great_person_turns = turns;
        }
    }

    /// A random allied city-state gives a random great person (not a
    /// prophet) at the major's city nearest to its capital (unverified: the
    /// pick among the great person units is by a random score; the gift
    /// does not count toward the next birth threshold).
    fn ally_gives_great_person(&mut self, major: PlayerId, rules: &CompiledRuleset) {
        let allies = super::allies_of(self.world, major);
        if allies.is_empty() {
            return;
        }
        let ally = allies[self.world.rng.events.below(allies.len() as u64) as usize];
        let kinds: Vec<&crate::classic::tables::GreatPersonDefinition> = rules
            .module.great_people
            .iter()
            .filter(|gp| gp.kind != crate::classic::effects::GreatPersonKind::Prophet)
            .collect();
        if kinds.is_empty() {
            return;
        }
        let def = kinds[self.world.rng.events.below(kinds.len() as u64) as usize];
        let Some(tile) = nearest_city_tile(self.world, rules, major, ally) else {
            return;
        };
        let nation = self.world.players.nation_of(major);
        let key = &rules.units[def.unit.0 as usize].key;
        let unit = rules.nation_variant(nation, key).unwrap_or(def.unit);
        self.spawn_at_city(major, tile, unit, rules);
    }
}

/// The tile of `major`'s city nearest to the city-state's capital.
fn nearest_city_tile(
    auth: &crate::world::state::AuthWorld,
    rules: &CompiledRuleset,
    major: PlayerId,
    city_state: PlayerId,
) -> Option<crate::ids::TileId> {
    let dims = auth.map().dimensions;
    let from = crate::combat::city::capital_of(auth, rules, city_state)
        .and_then(|c| auth.cities().get(c))
        .map(|c| c.tile);
    auth.indexes()
        .cities_of(major)
        .iter()
        .filter_map(|c| auth.cities().get(*c))
        .map(|c| c.tile)
        .min_by_key(|t| {
            let distance = from.and_then(|f| dims.distance(*t, f)).unwrap_or(i32::MAX);
            (distance, t.0)
        })
}

/// A third living player at war with both.
fn common_enemy(auth: &crate::world::state::AuthWorld, a: PlayerId, b: PlayerId) -> bool {
    let diplomacy = auth.diplomacy();
    (0..auth.players().count() as u8)
        .map(PlayerId)
        .filter(|x| *x != a && *x != b && auth.players().is_alive(*x))
        .any(|x| diplomacy.at_war(a, x) && diplomacy.at_war(b, x))
}

/// Recovery ×100 a turn from below the resting point, times
/// `100 + FriendshipRate` percent (Greece: double).
fn recovery_rate(stats: &super::CityStateStats, extra: i32) -> i32 {
    let rate = stats[CityStateStat::FriendshipRate.index()];
    RECOVERY_PER_TURN * (100 + rate + extra).max(0) / 100
}

/// The strongest regular land combat unit `major` can build (the best
/// one here, not a random pick among the top few [approximation]).
pub fn competitive_unit(
    auth: &crate::world::state::AuthWorld,
    rules: &CompiledRuleset,
    major: PlayerId,
) -> Option<UnitKindId> {
    let techs = auth.players().techs_of(major);
    let fighting = |r: Option<UnitRole>| {
        matches!(
            r,
            Some(
                UnitRole::Melee
                    | UnitRole::Archer
                    | UnitRole::Mounted
                    | UnitRole::Gun
                    | UnitRole::Armor
            )
        )
    };
    rules
        .units
        .iter()
        .enumerate()
        .filter(|(_, d)| fighting(d.role) && d.domain == Domain::Land && !d.unbuildable)
        .filter(|(_, d)| d.nation.is_none())
        .filter(|(_, d)| d.required_tech.is_none_or(|t| techs.contains(&t)))
        .filter(|(_, d)| d.obsolete_tech.is_none_or(|t| !techs.contains(&t)))
        .max_by_key(|(i, d)| (d.strength.max(d.ranged_strength), std::cmp::Reverse(*i)))
        .map(|(i, _)| UnitKindId(i as u16))
}
