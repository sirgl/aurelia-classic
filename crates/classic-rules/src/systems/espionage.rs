//! Espionage (classic/docs/classic/rules.md): spies granted by eras, travel,
//! surveillance, stealing technologies with detection and death, counter
//! intelligence, rigging city-state elections, coups, diplomats, death and
//! replacement.
//!
//! Each spy is processed in its owner's `Espionage` turn step (after
//! research): `amount += rate`, and the goal moves it to the next state.
//! Elections run in the city-states' turns every 15 rounds.

use crate::command::CommandError;
use crate::ids::{BuildingKindId, CityHandle, PlayerId, TechId};
use crate::rules::content::BuildingAbility;
use crate::rules::effects::{CompiledEffect, Modifier, Selector};
use crate::rules::stats::{EspionageStat, Stat};
use crate::rules::CompiledRuleset;
use crate::transaction::transaction::WorldTransaction;
use crate::world::espionage::{Spy, SpyState, MAX_RANK};
use crate::world::state::AuthWorld;
use crate::world::player::ClassicPlayers;

/// Rounds a spy travels to a new city. Unciv: models/Spy.kt (`moveTo`), equal (1).
pub const TRAVEL_TURNS: i64 = 1;
/// Rounds a diplomat spends making introductions in a foreign capital before it works. Own design (Unciv has no
/// diplomats): the length of the network a spy sets up (3) and a revival (5) halved over the two, rounded up: 4.
pub const INTRODUCTION_TURNS: i64 = 4;
/// Unciv: models/Spy.kt (`killSpy`, 5 turns), equal.
pub const REVIVE_TURNS: u8 = 5;
/// Turns a spy needs to set up its network in a foreign city. Unciv:
/// models/Spy.kt (EstablishNetwork, 3 turns); equal.
pub const SURVEILLANCE_TURNS: i64 = 3;
/// Percent of the dearest stealable technology that gathering intel must
/// reach, and the percent a spy's rate grows per rank. Unciv:
/// models/ModConstants.kt (`spyTechStealCostModifier` 1.25,
/// `spyRankStealPercentBonus` 25 with rank 1 at 100 %); equal.
pub const INTEL_COST_PERCENT: i64 = 125;
/// Unciv: models/ModConstants.kt (`spyRankStealPercentBonus` 25), equal.
pub const RANK_RATE_PERCENT: i64 = 25;
/// Rounds between city-state elections at Standard speed. Unciv:
/// models/ModConstants.kt (`cityStateElectionTurns` 15); equal.
pub const ELECTION_ROUNDS: u32 = 15;
/// Influence gained by the winner of a rigged election and lost by every
/// other major that knows the city-state (by every rigging major when the
/// neutral party wins); the votes of the neutral party. A spy's votes are
/// half its owner's influence plus its skill. Unciv:
/// logic/nation/diplomacy/CityStateFunctions.kt (`holdElections`);
/// equal.
pub const ELECTION_GAIN: i32 = 20;
/// Unciv: CityStateFunctions.kt (`holdElections`), equal.
pub const ELECTION_LOSS: i32 = 5;
/// Unciv: CityStateFunctions.kt (`holdElections`), equal.
pub const ELECTION_NEUTRAL_VOTES: i64 = 20;
/// Coup: base chance (percent), minus half the influence gap to the ally,
/// plus half the skill gap (skill `RANK_SKILL_PERCENT` × (rank + 1) of the
/// spy against the ally's spy in the city), capped at 85; on success the
/// spy's owner takes the ally's influence, the ally loses 20 and every
/// other major 10; on failure the spy dies and its owner loses 20. Unciv:
/// models/Spy.kt (`getCoupChanceOfSuccess`, `initiateCoup`); equal.
pub const COUP_BASE_CHANCE: i64 = 50;
/// Unciv: models/Spy.kt (coup: cap 85, ally -20, others -10, failure -20), equal.
pub const COUP_MAX_CHANCE: i64 = 85;
/// Unciv: models/Spy.kt (coup: cap 85, ally -20, others -10, failure -20), equal.
pub const COUP_ALLY_DROP: i32 = 20;
/// Unciv: models/Spy.kt (coup: cap 85, ally -20, others -10, failure -20), equal.
pub const COUP_OTHERS_DROP: i32 = 10;
/// Unciv: models/Spy.kt (coup: cap 85, ally -20, others -10, failure -20), equal.
pub const COUP_FAIL_DROP: i32 = 20;
/// Skill percent per spy rank (rank 0 here is Unciv's rank 1). Unciv:
/// models/ModConstants.kt (`spyRankSkillPercentBonus` 30); equal.
pub const RANK_SKILL_PERCENT: i64 = 30;
/// Era index of the Renaissance: every nation gets a spy once a nation enters it. Unciv:
/// Eras.json (Renaissance era, "Every major Nation gains a spy once a nation enters this era"), equal.
pub const RENAISSANCE: u8 = 3;
/// Era index of the Industrial era: from it on a nation gets one more spy as it enters each era. Own design
/// (Unciv grants spies only at the Renaissance): the espionage layer keeps up with the size of a late game.
pub const INDUSTRIAL: u8 = 4;

/// Research cost of `tech` for `player` (rules.md, its modifiers
/// included).
pub fn tech_cost(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId, tech: TechId) -> i64 {
    i64::from(crate::derived::research::technology_cost(auth, rules, player, tech))
}

/// The victim's technologies the thief could
/// research now (prerequisites known), by id.
pub fn stealable(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    thief: PlayerId,
    victim: PlayerId,
) -> Vec<TechId> {
    let mine = auth.players().techs_of(thief);
    let mut out: Vec<TechId> = auth
        .players()
        .techs_of(victim)
        .iter()
        .copied()
        .filter(|t| !mine.contains(t))
        .filter(|t| {
            rules
                .technologies
                .get(t.0 as usize)
                .is_some_and(|d| d.requires.iter().all(|r| mine.contains(r)))
        })
        .collect();
    out.sort_by_key(|t| t.0);
    out
}

/// An espionage stat of a player (`Stat::Module(crate::rules::stats::ClassicStat::Espionage)`, Player scope).
pub fn player_stat(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId, stat: EspionageStat) -> i32 {
    crate::effects::player::player_modifiers(auth, rules, player)
        .stats
        .get(Stat::Module(crate::rules::stats::ClassicStat::Espionage(stat)))
}

/// Percent of the steal rate of other nations' spies in `city`: its
/// buildings and its owner's empire-wide ones (`EnemyStealPercent`).
pub fn enemy_steal_percent(auth: &AuthWorld, rules: &CompiledRuleset, city: CityHandle) -> i32 {
    let Some(core) = auth.cities().get(city) else {
        return 0;
    };
    crate::effects::city::city_modifiers(auth, rules, core)
        .stats
        .get(Stat::Module(crate::rules::stats::ClassicStat::Espionage(EspionageStat::EnemyStealPercent)))
}

/// The rank new and revived spies of `player` start with (`StartingRank`:
/// Agent after the National Intelligence Agency).
pub fn starting_rank(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId) -> u8 {
    let ranks = player_stat(auth, rules, player, EspionageStat::StartingRank);
    ranks.clamp(0, i32::from(MAX_RANK)) as u8
}

/// Rate of a spy gathering intel (§7.4): the city's science ×100, changed
/// by the city's and its owner's counter-espionage and the thief's own
/// percent (at least 1), then +25 % per rank of the spy.
pub fn intel_rate(auth: &AuthWorld, rules: &CompiledRuleset, city: CityHandle, thief: PlayerId, rank: u8) -> i64 {
    let science = auth
        .cities()
        .get(city)
        .map(|c| crate::derived::income::city_income(auth, rules, c).science)
        .unwrap_or(0);
    let base = i64::from(science) * 100;
    let percent = enemy_steal_percent(auth, rules, city) + player_stat(auth, rules, thief, EspionageStat::StealPercent);
    let rate = (base * (100 + i64::from(percent)) / 100).max(1);
    rate * (100 + RANK_RATE_PERCENT * i64::from(rank)) / 100
}

/// Votes a spy's gathered `amount` is worth in an election (§7.8) with its
/// owner's `RigElectionPercent` (Covert Action +100: twice the votes).
pub fn rigged_votes(amount: i64, percent: i32) -> i64 {
    amount * (100 + i64::from(percent)).max(0) / 100
}

/// What the owner of a robbed city learns at the end of a theft (§7.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Detection {
    Undetected,
    Detected,
    Identified,
    /// The counter-spy killed the thief: no theft.
    Killed,
}

/// The detection roll (§7.5): `roll` in 0..300 plus the skill of the
/// owner's counter-spy if one is in the city, scaled by the owner's
/// `CatchPercent` (Double Agents, own effect), minus the thief's skill;
/// below 0 undetected, below 100 detected, below 200 identified, from 200
/// the thief is killed (with or without a counter-spy). Unciv:
/// models/Spy.kt (`stealTech`); equal apart from the catch percent.
pub fn detection(roll: i64, thief_rank: u8, counter_rank: Option<u8>, catch_percent: i32) -> Detection {
    let skill = |rank: u8| RANK_SKILL_PERCENT * (i64::from(rank) + 1);
    let defence = counter_rank.map(skill).unwrap_or(0);
    let result = (roll + defence) * (100 + i64::from(catch_percent)) / 100 - skill(thief_rank);
    match result {
        r if r < 0 => Detection::Undetected,
        r if r < 100 => Detection::Detected,
        r if r < 200 => Detection::Identified,
        _ => Detection::Killed,
    }
}

/// Amount a spy must gather: 125 % of the dearest stealable tech.
fn intel_goal(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    thief: PlayerId,
    victim: PlayerId,
) -> Option<i64> {
    let max = stealable(auth, rules, thief, victim)
        .into_iter()
        .map(|t| tech_cost(auth, rules, thief, t) * 100)
        .max()?;
    Some(max * INTEL_COST_PERCENT / 100)
}

/// Chance of a coup (§7.7), the code's always-on 0.5 included.
pub fn coup_chance(auth: &AuthWorld, player: PlayerId, spy: &Spy) -> Option<i64> {
    let city = auth.cities().get(spy.city?)?;
    let minor = city.owner;
    let ally = auth.module().city_states().ally(minor).filter(|a| *a != player)?;
    if !auth.players().is_city_state(minor) || !spy.has_surveillance() {
        return None;
    }
    let ally_rank = auth
        .module()
        .espionage()
        .spy_in(ally, spy.city?)
        .map(|(_, s)| s.rank);
    let gap = i64::from(
        crate::systems::city_states::effective_influence(auth, minor, ally)
            - crate::systems::city_states::effective_influence(auth, minor, player),
    );
    let skill = |rank: u8| RANK_SKILL_PERCENT * (i64::from(rank) + 1);
    let defence = ally_rank.map(skill).unwrap_or(0);
    let chance = (2 * COUP_BASE_CHANCE - gap + skill(spy.rank) - defence) / 2;
    Some(chance.clamp(0, COUP_MAX_CHANCE))
}

/// Why a spy command is refused (a move, a theft, a coup).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpyBlock {
    Dead,
    NoSuchCity,
    /// The city belongs to a nation the player has not met.
    NotMet,
    /// Another spy of the player is in the city.
    Occupied,
    /// A diplomat goes to a foreign capital, at peace.
    DiplomatTarget,
    NoSuchSpy,
    /// No steal right is left, or the victim has nothing to steal.
    NothingToSteal,
    NoCoup,
}

impl From<SpyBlock> for CommandError {
    fn from(block: SpyBlock) -> Self {
        CommandError::RuleViolation(block.reason().into())
    }
}

impl SpyBlock {
    pub const ALL: [SpyBlock; 8] = [
        SpyBlock::Dead,
        SpyBlock::NoSuchCity,
        SpyBlock::NotMet,
        SpyBlock::Occupied,
        SpyBlock::DiplomatTarget,
        SpyBlock::NoSuchSpy,
        SpyBlock::NothingToSteal,
        SpyBlock::NoCoup,
    ];

    pub fn reason(self) -> &'static str {
        match self {
            SpyBlock::Dead => "the spy is dead",
            SpyBlock::NoSuchCity => "no such city",
            SpyBlock::NotMet => "not met",
            SpyBlock::Occupied => "a spy is already there",
            SpyBlock::DiplomatTarget => "diplomats go to foreign capitals at peace",
            SpyBlock::NoSuchSpy => "no such spy",
            SpyBlock::NothingToSteal => "nothing to steal",
            SpyBlock::NoCoup => "no coup possible",
        }
    }
}

/// Why the spy may not move to `city` (None: it may).
pub fn move_problem(
    auth: &AuthWorld,
    player: PlayerId,
    index: usize,
    city: Option<CityHandle>,
    diplomat: bool,
) -> Option<SpyBlock> {
    let spy = auth.module().espionage().spies_of(player).get(index)?;
    if spy.state == SpyState::Dead {
        return Some(SpyBlock::Dead);
    }
    let Some(target) = city else {
        return None;
    };
    let Some(core) = auth.cities().get(target) else {
        return Some(SpyBlock::NoSuchCity);
    };
    if core.owner != player && !crate::systems::diplomacy::has_met(auth, player, core.owner) {
        return Some(SpyBlock::NotMet);
    }
    if auth.module().espionage().spy_in(player, target).is_some() {
        return Some(SpyBlock::Occupied);
    }
    if diplomat {
        let is_capital = core.original_capital && core.original_owner == core.owner;
        if core.owner == player || !is_capital || auth.diplomacy().at_war(player, core.owner) {
            return Some(SpyBlock::DiplomatTarget);
        }
    }
    None
}

impl<'a> WorldTransaction<'a> {
    /// The `Espionage` turn step: spies granted by the era, every spy
    /// processed.
    pub(crate) fn espionage_turn(&mut self, player: PlayerId, rules: &CompiledRuleset) {
        if !self.world.players.is_major(player) {
            return;
        }
        self.grant_spies(player, rules);
        let count = self.world.module.espionage.spies_of(player).len();
        for index in 0..count {
            self.process_spy(player, index, rules);
        }
    }

    /// Spies of the eras (§7.2): Renaissance gives every nation one
    /// (the first nation to reach it triggers it), Industrial and
    /// later eras one to the nation entering them.
    fn grant_spies(&mut self, player: PlayerId, rules: &CompiledRuleset) {
        let era = crate::combat::city::current_era(self.world, rules, player)
            .map(|e| e.index)
            .unwrap_or(0);
        let granted = self
            .world
            .module
            .espionage
            .era_granted
            .get(player.0 as usize)
            .copied()
            .unwrap_or(0);
        if era <= granted {
            return;
        }
        if let Some(g) = self.world.module.espionage.era_granted.get_mut(player.0 as usize) {
            *g = era;
        }
        if era >= RENAISSANCE && !self.world.module.espionage.renaissance_granted {
            self.world.module.espionage.renaissance_granted = true;
            for major in crate::systems::city_states::majors(self.world) {
                self.era_spy(major, rules);
            }
        }
        if era >= INDUSTRIAL {
            self.era_spy(player, rules);
        }
    }

    /// The spy of an era; a nation without spies first gets its
    /// `ExtraSpies` (England: its first spy comes with a second one).
    fn era_spy(&mut self, player: PlayerId, rules: &CompiledRuleset) {
        let mut count = 1;
        if self.world.module.espionage.spies_of(player).is_empty() {
            let extra = player_stat(self.world, rules, player, EspionageStat::ExtraSpies);
            count += extra.clamp(0, i32::from(u8::MAX)) as u8;
        }
        self.add_spies(player, count, rules);
    }

    /// `count` new spies at the player's starting rank (era grants, the
    /// National Intelligence Agency's `GrantSpies`).
    pub(crate) fn add_spies(&mut self, player: PlayerId, count: u8, rules: &CompiledRuleset) {
        let rank = starting_rank(self.world, rules, player);
        let Some(spies) = self.world.module.espionage.spies.get_mut(player.0 as usize) else {
            return;
        };
        for _ in 0..count {
            let name = crate::world::espionage::next_name_in(spies);
            spies.push(Spy {
                name,
                rank,
                ..Spy::default()
            });
        }
        self.relations_changed(player);
    }

    /// Every living spy of the player rises `ranks` ranks, at most Special
    /// Agent (`PromoteSpies`: the National Intelligence Agency).
    pub(crate) fn promote_spies(&mut self, player: PlayerId, ranks: u8) {
        let Some(spies) = self.world.module.espionage.spies.get_mut(player.0 as usize) else {
            return;
        };
        for spy in spies.iter_mut() {
            if spy.state != SpyState::Dead {
                spy.rank = spy.rank.saturating_add(ranks).min(MAX_RANK);
            }
        }
        self.relations_changed(player);
    }

    fn process_spy(&mut self, player: PlayerId, index: usize, rules: &CompiledRuleset) {
        let Some(mut spy) = self.world.module.espionage.spies_of(player).get(index).copied() else {
            return;
        };
        match spy.state {
            SpyState::Dead => {
                spy.dead_turns += 1;
                if spy.dead_turns >= REVIVE_TURNS {
                    spy = Spy {
                        name: self.world.module.espionage.next_name(player),
                        rank: starting_rank(self.world, rules, player),
                        ..Spy::default()
                    };
                }
            }
            SpyState::Unassigned | SpyState::CounterIntel | SpyState::Schmooze => {}
            SpyState::RigElection => spy.amount += (i64::from(spy.rank) + 1).pow(2),
            _ => spy.amount += spy.rate,
        }
        if spy.state != SpyState::Dead
            && spy.state != SpyState::Unassigned
            && spy.amount >= spy.goal
        {
            spy = self.advance_spy(player, spy, rules);
        }
        if let Some(slot) = self.world.module.espionage.spy_mut(player, index) {
            *slot = spy;
        }
    }

    /// The goal of a state is reached: the next state.
    fn advance_spy(&mut self, player: PlayerId, mut spy: Spy, rules: &CompiledRuleset) -> Spy {
        let Some(city) = spy
            .city
            .and_then(|c| self.world.cities.get(c))
            .map(|c| (c.owner, spy.city.unwrap()))
        else {
            return Spy {
                state: SpyState::Unassigned,
                city: None,
                ..spy
            };
        };
        let (owner, handle) = city;
        match spy.state {
            SpyState::Travelling if spy.diplomat => set(
                &mut spy,
                SpyState::MakingIntroductions,
                1,
                INTRODUCTION_TURNS,
            ),
            SpyState::Travelling if owner == player => set(&mut spy, SpyState::CounterIntel, 0, 1),
            SpyState::Travelling => set(&mut spy, SpyState::Surveillance, 1, SURVEILLANCE_TURNS),
            SpyState::Surveillance if self.world.players.is_city_state(owner) => {
                set(&mut spy, SpyState::RigElection, 0, 1)
            }
            SpyState::Surveillance => {
                if let Some(goal) = intel_goal(self.world, rules, player, owner) {
                    let rate = intel_rate(self.world, rules, handle, player, spy.rank);
                    set(&mut spy, SpyState::GatheringIntel, rate, goal);
                }
            }
            SpyState::GatheringIntel => spy = self.heist(player, spy, owner, handle, rules),
            SpyState::MakingIntroductions => set(&mut spy, SpyState::Schmooze, 0, 1),
            _ => {}
        }
        spy
    }

    /// A theft is complete (§7.5): the detection roll; the thief gains a
    /// steal right and a rank unless a counter-spy kills it.
    fn heist(
        &mut self,
        player: PlayerId,
        mut spy: Spy,
        owner: PlayerId,
        city: CityHandle,
        rules: &CompiledRuleset,
    ) -> Spy {
        let counter = self
            .world
            .module
            .espionage
            .spy_in(owner, city)
            .map(|(i, s)| (i, s.rank));
        let roll = self.world.rng.events.below(300) as i64;
        let catch = player_stat(self.world, rules, owner, EspionageStat::CatchPercent);
        let outcome = detection(roll, spy.rank, counter.map(|(_, rank)| rank), catch);
        if outcome == Detection::Killed {
            if let Some((i, _)) = counter {
                if let Some(c) = self.world.module.espionage.spy_mut(owner, i) {
                    c.rank = (c.rank + 1).min(MAX_RANK);
                }
            }
            return Spy {
                state: SpyState::Dead,
                city: None,
                amount: 0,
                rate: 0,
                goal: 0,
                dead_turns: 0,
                ..spy
            };
        }
        let rights = self.world.module.espionage.steal_rights(player, owner);
        self.world
            .module
            .espionage
            .set_steal_rights(player, owner, rights.saturating_add(1));
        spy.rank = (spy.rank + 1).min(MAX_RANK);
        let rate = intel_rate(self.world, rules, city, player, spy.rank);
        match intel_goal(self.world, rules, player, owner) {
            Some(goal) => set(&mut spy, SpyState::GatheringIntel, rate, goal),
            None => set(&mut spy, SpyState::Surveillance, 1, 1),
        }
        spy.amount = if spy.state == SpyState::Surveillance {
            1
        } else {
            0
        };
        self.relations_changed(player);
        spy
    }

    /// Recompute the steal rate of spies gathering intelligence; their
    /// progress and goal stay.
    pub(crate) fn refresh_intel_rates(&mut self, refresh: RateRefresh, rules: &CompiledRuleset) {
        for thief in 0..self.world.players.count() {
            let thief = PlayerId(thief as u8);
            let count = self.world.module.espionage.spies_of(thief).len();
            for index in 0..count {
                let spy = self.world.module.espionage.spies_of(thief)[index];
                let Some(city) = spy.city else {
                    continue;
                };
                if spy.state != SpyState::GatheringIntel || !refresh.reaches(self.world, thief, city) {
                    continue;
                }
                let rate = intel_rate(self.world, rules, city, thief, spy.rank);
                if let Some(slot) = self.world.module.espionage.spy_mut(thief, index) {
                    slot.rate = rate;
                }
            }
        }
    }

    /// A completed building changes steal rates at once when it has
    /// `AffectSpiesNow` (its city) or an empire-wide `EnemyStealPercent`
    /// (every city of its owner).
    pub(crate) fn espionage_after_building(&mut self, city: CityHandle, building: BuildingKindId, rules: &CompiledRuleset) {
        let def = &rules.buildings[building.0 as usize];
        if def.module.abilities.contains(&BuildingAbility::AffectSpiesNow) {
            self.refresh_intel_rates(RateRefresh::City(city), rules);
        }
        let empire_wide = def.effects.passive_city.iter().any(|e| {
            e.selector == Selector::OwnerCities && is_espionage_stat(e, EspionageStat::EnemyStealPercent)
        });
        if !empire_wide {
            return;
        }
        if let Some(owner) = self.world.cities.get(city).map(|c| c.owner) {
            self.refresh_intel_rates(RateRefresh::CitiesOf(owner), rules);
        }
    }

    /// A policy with a `StealPercent` (Industrial Espionage) speeds up the
    /// player's spies at once.
    pub(crate) fn espionage_after_policy(&mut self, player: PlayerId, policy: crate::ids::PolicyId, rules: &CompiledRuleset) {
        let effects = &rules.module.policies[policy.0 as usize].effects.passive_player;
        if effects.iter().any(|e| is_espionage_stat(e, EspionageStat::StealPercent)) {
            self.refresh_intel_rates(RateRefresh::SpiesOf(player), rules);
        }
    }

    pub(crate) fn move_spy(
        &mut self,
        player: PlayerId,
        index: usize,
        city: Option<CityHandle>,
        diplomat: bool,
    ) -> Result<(), CommandError> {
        let spy = self
            .world
            .module
            .espionage
            .spy_mut(player, index)
            .ok_or(SpyBlock::NoSuchSpy)?;
        *spy = Spy {
            state: if city.is_some() {
                SpyState::Travelling
            } else {
                SpyState::Unassigned
            },
            city,
            amount: 0,
            rate: 1,
            goal: TRAVEL_TURNS,
            diplomat,
            ..*spy
        };
        self.relations_changed(player);
        Ok(())
    }

    /// Spend a steal right on a technology of the victim.
    pub(crate) fn steal_technology(
        &mut self,
        player: PlayerId,
        victim: PlayerId,
        tech: TechId,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        let rights = self.world.module.espionage.steal_rights(player, victim);
        self.world
            .module
            .espionage
            .set_steal_rights(player, victim, rights.saturating_sub(1));
        self.learn(player, tech, rules);
        if stealable(self.world, rules, player, victim).is_empty() {
            self.world.module.espionage.set_steal_rights(player, victim, 0);
        }
        Ok(())
    }

    /// A coup (§7.7): success swaps our influence with the ally's and the
    /// others lose 20; failure sets ours to −10 and kills the spy.
    pub(crate) fn stage_coup(
        &mut self,
        player: PlayerId,
        index: usize,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        let spy = self
            .world
            .module
            .espionage
            .spies_of(player)
            .get(index)
            .copied()
            .ok_or(SpyBlock::NoSuchSpy)?;
        let chance = coup_chance(self.world, player, &spy)
            .ok_or(SpyBlock::NoCoup)?;
        let minor = spy
            .city
            .and_then(|c| self.world.cities.get(c))
            .map(|c| c.owner)
            .ok_or(CommandError::UnknownCity)?;
        let roll = self.world.rng.events.below(100) as i64;
        if roll < chance {
            self.coup_succeeds(player, minor, rules);
        } else {
            self.change_influence(minor, player, -COUP_FAIL_DROP * 100, rules);
            let defender = self.world.module.city_states.ally(minor).and_then(|ally| {
                let city = spy.city?;
                self.world.module.espionage.spy_in(ally, city).map(|(i, _)| (ally, i))
            });
            if let Some((ally, i)) = defender {
                if let Some(c) = self.world.module.espionage.spy_mut(ally, i) {
                    c.rank = (c.rank + 1).min(MAX_RANK);
                }
            }
            if let Some(s) = self.world.module.espionage.spy_mut(player, index) {
                *s = Spy {
                    state: SpyState::Dead,
                    city: None,
                    ..*s
                };
            }
        }
        Ok(())
    }

    fn coup_succeeds(&mut self, player: PlayerId, minor: PlayerId, rules: &CompiledRuleset) {
        let Some(ally) = self.world.module.city_states.ally(minor) else {
            return;
        };
        let theirs = self.world.module.city_states.standing(minor, ally).influence;
        for p in crate::systems::city_states::majors(self.world) {
            if p == ally {
                self.change_influence(minor, p, -COUP_ALLY_DROP * 100, rules);
            } else if p != player && crate::systems::diplomacy::has_met(self.world, minor, p) {
                self.change_influence(minor, p, -COUP_OTHERS_DROP * 100, rules);
            }
        }
        self.set_influence(minor, player, theirs, rules);
    }

    /// City-state elections every 15 rounds (§7.8): one nation wins,
    /// weighted by the votes its spies gathered.
    pub(crate) fn city_state_election(&mut self, minor: PlayerId, rules: &CompiledRuleset) {
        if self.world.turn.round % ELECTION_ROUNDS != 0 {
            return;
        }
        let Some(capital) = self.world.indexes.cities_of(minor).first().copied() else {
            return;
        };
        let mut votes: Vec<(Option<PlayerId>, i64)> = Vec::new();
        for major in crate::systems::city_states::majors(self.world) {
            if let Some((i, spy)) = self
                .world
                .module
                .espionage
                .spy_in(major, capital)
                .map(|(i, s)| (i, *s))
            {
                if spy.state == SpyState::RigElection {
                    // The rigging spy owner's own rig-election percent
                    // (Covert Action) scales the spy's skill, as Unciv's
                    // spy efficiency does.
                    let percent = player_stat(self.world, rules, major, EspionageStat::RigElectionPercent);
                    let skill = RANK_SKILL_PERCENT * (i64::from(spy.rank) + 1);
                    let influence = i64::from(self.world.module.city_states.influence(minor, major));
                    votes.push((Some(major), (influence / 2 + rigged_votes(skill, percent)).max(0)));
                    if let Some(s) = self.world.module.espionage.spy_mut(major, i) {
                        s.amount = 0;
                    }
                }
            }
        }
        if votes.is_empty() {
            return;
        }
        let riggers: Vec<PlayerId> = votes.iter().filter_map(|(p, _)| *p).collect();
        votes.push((None, ELECTION_NEUTRAL_VOTES));
        let total: i64 = votes.iter().map(|(_, v)| v).sum();
        let mut pick = self.world.rng.events.below(total.max(1) as u64) as i64;
        let winner = votes
            .iter()
            .find(|(_, v)| {
                pick -= v;
                pick < 0
            })
            .and_then(|(p, _)| *p);
        let Some(winner) = winner else {
            for major in riggers {
                self.change_influence(minor, major, -ELECTION_LOSS * 100, rules);
            }
            return;
        };
        self.change_influence(minor, winner, ELECTION_GAIN * 100, rules);
        for major in crate::systems::city_states::majors(self.world) {
            if major != winner && crate::systems::diplomacy::has_met(self.world, minor, major) {
                self.change_influence(minor, major, -ELECTION_LOSS * 100, rules);
            }
        }
    }

    /// Spies in a captured or razed city leave it.
    pub(crate) fn evict_spies(&mut self, city: CityHandle) {
        for spies in self.world.module.espionage.spies.iter_mut() {
            for spy in spies
                .iter_mut()
                .filter(|s| s.city == Some(city) && s.state != SpyState::Dead)
            {
                *spy = Spy {
                    state: SpyState::Unassigned,
                    city: None,
                    amount: 0,
                    rate: 0,
                    goal: 0,
                    ..*spy
                };
            }
        }
    }

    /// War sends the diplomats of both sides home.
    pub(crate) fn evacuate_diplomats(&mut self, a: PlayerId, b: PlayerId) {
        let owners: Vec<(PlayerId, PlayerId)> = vec![(a, b), (b, a)];
        for (me, other) in owners {
            let cities: Vec<CityHandle> = self.world.indexes.cities_of(other).to_vec();
            if let Some(spies) = self.world.module.espionage.spies.get_mut(me.0 as usize) {
                for spy in spies
                    .iter_mut()
                    .filter(|s| s.diplomat && s.city.is_some_and(|c| cities.contains(&c)))
                {
                    *spy = Spy {
                        state: SpyState::Unassigned,
                        city: None,
                        diplomat: false,
                        amount: 0,
                        rate: 0,
                        goal: 0,
                        ..*spy
                    };
                }
            }
        }
    }
}

/// Which spies `refresh_intel_rates` recomputes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RateRefresh {
    /// Spies in this city (a building with `AffectSpiesNow`).
    City(CityHandle),
    /// Spies in every city of this player (its empire-wide percent changed).
    CitiesOf(PlayerId),
    /// The spies of this player (its own steal percent changed).
    SpiesOf(PlayerId),
}

impl RateRefresh {
    fn reaches(self, auth: &AuthWorld, thief: PlayerId, city: CityHandle) -> bool {
        match self {
            RateRefresh::City(target) => target == city,
            RateRefresh::CitiesOf(owner) => auth.cities().get(city).is_some_and(|c| c.owner == owner),
            RateRefresh::SpiesOf(player) => player == thief,
        }
    }
}

fn is_espionage_stat(effect: &CompiledEffect, wanted: EspionageStat) -> bool {
    matches!(effect.modifier, Modifier::AddStat { stat: Stat::Module(crate::rules::stats::ClassicStat::Espionage(stat)), .. } if stat == wanted)
}

fn set(spy: &mut Spy, state: SpyState, rate: i64, goal: i64) {
    spy.state = state;
    spy.amount = 0;
    spy.rate = rate;
    spy.goal = goal;
}
