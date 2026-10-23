//! Diplomacy between civilizations (classic/docs/classic/rules.md):
//! war and peace with peace treaties, defensive pacts that drag partners
//! into wars, deals (gold, gold per turn, resources, cities, embassies,
//! open borders, pacts, research agreements, friendship, peace, third-party
//! war and peace), denunciations, warmonger memory.
//!
//! Journal: PlayerChange::WarDeclared / PeaceMade / BordersOpened /
//! RelationsChanged, CityChange::OwnerChanged (ceded cities).

pub(crate) mod deals;
pub(crate) mod turn;
pub(crate) mod validate;

use crate::command::CommandError;
use crate::ids::{PlayerId, TileId};
use crate::rules::CompiledRuleset;
use crate::classic::ClassicChange;
use crate::transaction::journal::PlayerChange;
use crate::transaction::transaction::WorldTransaction;
use crate::world::relations::RelationsState;
use crate::world::player::ClassicPlayers;

/// Rounds a timed deal lasts at Standard speed: gold per turn, resources,
/// open borders, defensive pacts, research agreements. Unciv: Speeds.json
/// (Gods & Kings ruleset), Standard dealDuration; equal.
pub const DEAL_DURATION: u32 = 30;
/// Rounds after a peace treaty during which no new war may start, at
/// Standard speed. Unciv: Speeds.json, Standard peaceDealDuration; equal.
pub const PEACE_DURATION: u32 = 10;
/// Rounds a declaration of friendship or a denunciation lasts. Unciv:
/// logic/civilization/diplomacy/DiplomacyManager.kt (both flags set to 30
/// turns, not scaled by speed); equal.
pub const RELATIONSHIP_DURATION: u32 = 30;
/// Warmonger weight of an attack on a major or a city-state (the same for
/// both). Own value (no Unciv equivalent of the same shape: Unciv keeps
/// warmongering as an opinion modifier, -5 per war declaration, halved
/// towards 0 each turn, in DeclareWar.kt / DiplomacyTurnManager.kt).
pub const WARMONGER_ATTACKED_WEIGHT: i32 = 250;
/// Change of the warmonger memory per round (negative: it fades). Own value
/// (see `WARMONGER_ATTACKED_WEIGHT`).
pub const WARMONGER_DECAY: i32 = -5;
/// Warmonger weight of a nuclear weapon of level 1; level 2 weighs twice.
/// Using one is a public act: every civilization that met the user, and
/// every owner of what the blast hit, remembers it (own design, the
/// "infamy" of classic/docs/classic/RESOURCING.md; the weight of a
/// declaration of war is `WARMONGER_ATTACKED_WEIGHT` = 250, a nuclear weapon
/// weighs more than one and a half declarations: it passes the denunciation
/// line of the AI (250) at once, and fades in about 80 rounds at the
/// nuclear era's percent).
pub const WARMONGER_NUKE_WEIGHT: i32 = 400;
/// Cities expected on a standard-size map; it scales the warmonger weight
/// of a captured city. Own value (see `WARMONGER_ATTACKED_WEIGHT`).
pub const EXPECTED_CITY_COUNT: i32 = 52;
/// Percent of the warmonger weight kept when the victim is a city-state.
/// Own value (see `WARMONGER_ATTACKED_WEIGHT`).
pub const WARMONGER_CITY_STATE_PERCENT: i32 = 50;
/// Divisor of the boost a research agreement gives (the smaller of the two
/// partners' science during the agreement, / 3), and the base percent of
/// it paid (50, plus the player's research-agreement percent). Unciv:
/// logic/civilization/diplomacy/DiplomacyTurnManager.kt and
/// managers/TechManager.kt (`scienceFromResearchAgreements`: min / 3 x
/// 0.5); equal.
pub const RESEARCH_AGREEMENT_POOL_DIVISOR: u32 = 3;
/// Unciv: DiplomacyTurnManager.kt and TechManager.kt, equal.
pub const MEDIAN_TECH_PERCENT: u32 = 50;

/// Percent by which the warmonger weight is scaled, per era index (later
/// eras weigh more). Own value (see `WARMONGER_ATTACKED_WEIGHT`).
const WARMONGER_PERCENT: [i32; 8] = [50, 60, 70, 80, 90, 100, 100, 100];

impl<'a> WorldTransaction<'a> {
    /// War (§5.1): the declarer's pacts end, every deal between the two
    /// ends (research without payout), embassies close, friendship breaks,
    /// the victim's defensive partners join, observers remember the
    /// aggression.
    pub(crate) fn declare_war(
        &mut self,
        player: PlayerId,
        enemy: PlayerId,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        self.declare_war_as(player, enemy, false, rules);
        Ok(())
    }

    fn declare_war_as(
        &mut self,
        player: PlayerId,
        enemy: PlayerId,
        by_pact: bool,
        rules: &CompiledRuleset,
    ) {
        if self.world.diplomacy.at_war(player, enemy) {
            return;
        }
        let round = self.world.turn.round;
        self.cancel_pacts_of(player);
        self.end_ties(player, enemy);
        self.evacuate_diplomats(player, enemy);
        let partners = self.pact_partners(enemy, round);
        self.set_war_state(player, enemy, true);
        self.cancel_trade_between(player, enemy, rules);
        let relations = &mut self.world.module.relations;
        let players = relations.players;
        RelationsState::set_pair(&mut relations.war_since, players, player, enemy, round);
        self.journal
            .player_changes
            .push(PlayerChange::WarDeclared { player, enemy });
        if !by_pact {
            self.warmonger_for_war(player, enemy, rules);
        }
        if self.world.players.is_major(player) && self.world.players.is_city_state(enemy) {
            self.war_on_city_state(player, enemy, rules);
        }
        for cs in [player, enemy] {
            if self.world.players.is_city_state(cs) {
                self.update_ally(cs, rules);
            }
        }
        for partner in partners {
            if partner != player && self.world.players.is_alive(partner) {
                self.declare_war_as(partner, player, true, rules);
            }
        }
        self.minor_allies_join_war(player, enemy, rules);
    }

    /// The one writer of war and peace between two players. A city-state's
    /// friendship counts only in peace (`effective_influence`), so a
    /// civilization facing a city-state recomputes its effects.
    pub(crate) fn set_war_state(&mut self, a: PlayerId, b: PlayerId, at_war: bool) {
        self.world.diplomacy.set_war(a, b, at_war);
        for (city_state, major) in [(a, b), (b, a)] {
            if self.world.players.is_city_state(city_state) && self.world.players.is_major(major) {
                self.journal
                    .player_changes
                    .push(PlayerChange::EffectsChanged { player: major });
            }
        }
    }

    /// Players with a defensive pact with `victim`.
    fn pact_partners(&self, victim: PlayerId, round: u32) -> Vec<PlayerId> {
        let relations = &self.world.module.relations;
        (0..relations.players)
            .map(|p| PlayerId(p as u8))
            .filter(|p| *p != victim && relations.has_pact(*p, victim, round))
            .collect()
    }

    /// `cancelDefensivePacts`: every pact of `player`, both ways.
    fn cancel_pacts_of(&mut self, player: PlayerId) {
        let relations = &mut self.world.module.relations;
        let players = relations.players;
        for other in 0..players {
            RelationsState::set_pair(
                &mut relations.pact_until,
                players,
                player,
                PlayerId(other as u8),
                0,
            );
        }
    }

    /// Everything that ties two players ends (war or denunciation of a
    /// friend): trades, research agreements without payout, embassies,
    /// pacts, friendship, open borders.
    fn end_ties(&mut self, a: PlayerId, b: PlayerId) {
        let relations = &mut self.world.module.relations;
        let players = relations.players;
        relations
            .trades
            .retain(|t| !((t.from == a && t.to == b) || (t.from == b && t.to == a)));
        RelationsState::set_pair(&mut relations.research_until, players, a, b, 0);
        RelationsState::set_pair(&mut relations.pact_until, players, a, b, 0);
        RelationsState::set_pair(&mut relations.friends_until, players, a, b, 0);
        RelationsState::set_pair(&mut relations.open_until, players, a, b, 0);
        if let (Some(i), Some(j)) = (relations.slot(a, b), relations.slot(b, a)) {
            relations.embassy[i] = 0;
            relations.embassy[j] = 0;
            relations.research_points[i] = 0;
            relations.research_points[j] = 0;
        }
        self.world.diplomacy.set_open(a, b, false);
        self.world.diplomacy.set_open(b, a, false);
        self.relations_changed(a);
        self.relations_changed(b);
    }

    pub(crate) fn relations_changed(&mut self, player: PlayerId) {
        self.journal
            .module_changes
            .push(ClassicChange::RelationsChanged { player });
    }

    /// Peace (§5.2): the war ends, no new war for `PEACE_DURATION` rounds,
    /// units on the other's land without open borders are sent home.
    pub(crate) fn make_peace(
        &mut self,
        player: PlayerId,
        enemy: PlayerId,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        if !self.world.diplomacy().at_war(player, enemy) {
            return Err(CommandError::RuleViolation("not at war".into()));
        }
        self.set_war_state(player, enemy, false);
        let round = self.world.turn.round;
        let relations = &mut self.world.module.relations;
        let players = relations.players;
        RelationsState::set_pair(
            &mut relations.peace_until,
            players,
            player,
            enemy,
            round + PEACE_DURATION + 1,
        );
        self.journal
            .player_changes
            .push(PlayerChange::PeaceMade { player, enemy });
        self.expel_units(player, enemy, rules);
        self.expel_units(enemy, player, rules);
        for cs in [player, enemy] {
            if self.world.players.is_city_state(cs) {
                self.update_ally(cs, rules);
            }
        }
        Ok(())
    }

    /// Open the host's borders to the guest (one side of a deal; with the
    /// technologies that allow it the grant lasts `DEAL_DURATION` rounds).
    pub(crate) fn open_borders(
        &mut self,
        host: PlayerId,
        guest: PlayerId,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        self.world.diplomacy.set_open(host, guest, true);
        let timed = rules.technologies.iter().any(|t| t.allows_open_borders);
        if timed {
            let round = self.world.turn.round;
            let relations = &mut self.world.module.relations;
            if let Some(i) = relations.slot(host, guest) {
                relations.open_until[i] = round + DEAL_DURATION;
            }
        }
        self.journal
            .player_changes
            .push(PlayerChange::BordersOpened { host, guest });
        Ok(())
    }

    /// Denounce (§5.10): for `RELATIONSHIP_DURATION` rounds; embassies close
    /// both ways (with them the pacts), friendship breaks.
    pub(crate) fn denounce(
        &mut self,
        player: PlayerId,
        target: PlayerId,
    ) -> Result<(), CommandError> {
        let round = self.world.turn.round;
        let relations = &mut self.world.module.relations;
        let players = relations.players;
        if let (Some(i), Some(j)) = (
            relations.slot(player, target),
            relations.slot(target, player),
        ) {
            relations.denounced_until[i] = round + RELATIONSHIP_DURATION;
            relations.embassy[i] = 0;
            relations.embassy[j] = 0;
        }
        RelationsState::set_pair(&mut relations.pact_until, players, player, target, 0);
        RelationsState::set_pair(&mut relations.friends_until, players, player, target, 0);
        self.relations_changed(player);
        self.relations_changed(target);
        Ok(())
    }

    /// Units of `guest` on `host`'s land that may no longer be there go to
    /// the nearest tile they may enter (or die without one).
    pub(crate) fn expel_units(&mut self, host: PlayerId, guest: PlayerId, rules: &CompiledRuleset) {
        let mut units = self.world.indexes.units_of(guest).to_vec();
        units.sort_by_key(|u| self.world.units.get(*u).map(|v| v.core.id));
        for unit in units {
            let Some(view) = self.world.units.get(unit) else {
                continue;
            };
            let core = *view.core;
            let on_host = self.world.map.owner(core.tile).flatten() == Some(host);
            if !on_host || crate::world::territory::may_enter(self.world, guest, core.tile) {
                continue;
            }
            match self.nearest_free_tile(guest, &core, rules) {
                Some(tile) => self.relocate_unit(unit, tile, rules),
                None => {
                    let _ = self.destroy_unit(unit);
                }
            }
        }
    }

    fn nearest_free_tile(
        &self,
        owner: PlayerId,
        unit: &crate::world::unit::UnitCore,
        rules: &CompiledRuleset,
    ) -> Option<TileId> {
        let dims = self.world.map.dimensions;
        let profile = crate::navigation::MoveProfile::of_unit(self.world, rules, unit);
        for radius in 1..=8 {
            let mut ring: Vec<TileId> = crate::math::tiles_in_hex_radius(&dims, unit.tile, radius)
                .into_iter()
                .filter(|t| dims.distance(*t, unit.tile) == Some(radius))
                .collect();
            ring.sort_by_key(|t| t.0);
            let found = ring.into_iter().find(|t| {
                crate::navigation::cost::destination_ok(self.world, &profile, *t)
                    && crate::world::territory::may_enter(self.world, owner, *t)
                    && self.world.indexes.city_on_tile(*t).is_none()
                    && crate::world::stacking::can_stop_at(
                        self.world, rules, owner, unit.kind, None, *t,
                    )
                    .is_ok()
            });
            if found.is_some() {
                return found;
            }
        }
        None
    }

    /// Warmonger memory of a declaration of war (§5.11 p. 1–2): every
    /// major that met the aggressor and is not at war with the victim
    /// (the victim included) remembers 250, scaled by the game's era.
    fn warmonger_for_war(
        &mut self,
        aggressor: PlayerId,
        victim: PlayerId,
        rules: &CompiledRuleset,
    ) {
        if self.world.players.is_barbarian(victim) {
            return;
        }
        let victim_minor = self.world.players.is_city_state(victim);
        let percent = warmonger_era_percent(self.world, rules);
        for o in 0..self.world.players.count() {
            let observer = PlayerId(o as u8);
            if observer == aggressor
                || !self.world.players.is_major(observer)
                || !self.world.players.is_alive(observer)
            {
                continue;
            }
            if !has_met(self.world, observer, aggressor) && observer != victim {
                continue;
            }
            if !victim_minor && observer != victim && self.world.diplomacy.at_war(observer, victim)
            {
                continue;
            }
            self.change_warmonger(
                observer,
                aggressor,
                WARMONGER_ATTACKED_WEIGHT * percent / 100,
            );
        }
    }

    /// Warmonger memory of a conquest (§5.11 p. 3): the offset grows with
    /// the world's expected cities and shrinks with the victim's cities;
    /// halved for an observer at war with the victim and for city-states.
    pub(crate) fn warmonger_for_conquest(
        &mut self,
        conqueror: PlayerId,
        victim: PlayerId,
        rules: &CompiledRuleset,
    ) {
        if self.world.players.is_city_state(conqueror) || self.world.players.is_barbarian(conqueror)
        {
            return;
        }
        let victim_cities = self.world.indexes.cities_of(victim).len().max(1) as i32;
        let world_cities = self.world.cities.len().max(1) as i32;
        let mut offset = 1000 * EXPECTED_CITY_COUNT / (world_cities * victim_cities);
        if self.world.players.is_city_state(victim) {
            offset = offset * WARMONGER_CITY_STATE_PERCENT / 100;
        }
        let percent = warmonger_era_percent(self.world, rules);
        for o in 0..self.world.players.count() {
            let observer = PlayerId(o as u8);
            if observer == conqueror
                || !self.world.players.is_major(observer)
                || !self.world.players.is_alive(observer)
            {
                continue;
            }
            if !has_met(self.world, observer, conqueror) && observer != victim {
                continue;
            }
            let mut amount = offset;
            if self.world.diplomacy.at_war(observer, victim) {
                amount /= 2;
            }
            self.change_warmonger(observer, conqueror, amount * percent / 100);
        }
    }

    /// Warmonger memory of a nuclear strike: every living major that met the
    /// user (and every `victim`) remembers `WARMONGER_NUKE_WEIGHT` times the
    /// weapon's level, scaled by the game's era, whatever the war between
    /// them (the human user and the AI alike). Not city-states or
    /// barbarians, which have no memory of this kind.
    pub(crate) fn warmonger_for_nuke(
        &mut self,
        user: PlayerId,
        level: u8,
        victims: &[PlayerId],
        rules: &CompiledRuleset,
    ) {
        if !self.world.players.is_major(user) {
            return;
        }
        let percent = warmonger_era_percent(self.world, rules);
        let amount = WARMONGER_NUKE_WEIGHT * i32::from(level.max(1)) * percent / 100;
        for o in 0..self.world.players.count() {
            let observer = PlayerId(o as u8);
            if observer == user || !self.world.players.is_major(observer) || !self.world.players.is_alive(observer) {
                continue;
            }
            if has_met(self.world, observer, user) || victims.contains(&observer) {
                self.change_warmonger(observer, user, amount);
            }
        }
    }

    /// Liberation lowers the liberator's warmonger memory (§5.11 p. 4).
    pub(crate) fn warmonger_for_liberation(
        &mut self,
        liberator: PlayerId,
        liberated: PlayerId,
        rules: &CompiledRuleset,
    ) {
        let cities = self.world.indexes.cities_of(liberated).len().max(1) as i32;
        let world_cities = self.world.cities.len().max(1) as i32;
        let offset = 1000 * EXPECTED_CITY_COUNT / (world_cities * cities);
        let percent = warmonger_era_percent(self.world, rules);
        for o in 0..self.world.players.count() {
            let observer = PlayerId(o as u8);
            if observer != liberator
                && self.world.players.is_major(observer)
                && has_met(self.world, observer, liberator)
            {
                self.change_warmonger(observer, liberator, -offset * percent / 100);
            }
        }
    }

    pub(crate) fn change_warmonger(&mut self, observer: PlayerId, player: PlayerId, delta: i32) {
        let relations = &mut self.world.module.relations;
        if let Some(i) = relations.slot(observer, player) {
            relations.warmonger[i] = (relations.warmonger[i] + delta).max(0);
        }
    }

    /// City-state allies of either side join the war (§5.1 p. 10); filled
    /// by the city-state system.
    fn minor_allies_join_war(&mut self, a: PlayerId, b: PlayerId, rules: &CompiledRuleset) {
        let allies_of =
            |tx: &Self, p: PlayerId| crate::systems::city_states::allies_of(tx.world, p);
        let allies_a = allies_of(self, a);
        let allies_b = allies_of(self, b);
        for cs in allies_a {
            if !self.world.diplomacy.at_war(cs, b) && cs != b {
                self.declare_war_as(cs, b, true, rules);
            }
        }
        for cs in allies_b {
            if !self.world.diplomacy.at_war(cs, a) && cs != a {
                self.declare_war_as(cs, a, true, rules);
            }
        }
    }
}

/// Has `a` met `b` (contact memory of `a`)?
pub(crate) fn has_met(auth: &crate::world::state::AuthWorld, a: PlayerId, b: PlayerId) -> bool {
    auth.knowledge()
        .get(a.0 as usize)
        .is_some_and(|k| k.contact(b).met_round.is_some())
}

/// Warmonger percent of the game's era: the average current era of the
/// alive players (rules.md).
pub(crate) fn warmonger_era_percent(
    auth: &crate::world::state::AuthWorld,
    rules: &CompiledRuleset,
) -> i32 {
    let mut sum = 0usize;
    let mut alive = 0usize;
    for p in 0..auth.players().count() {
        let player = PlayerId(p as u8);
        if !auth.players().is_alive(player) {
            continue;
        }
        alive += 1;
        sum += crate::combat::city::current_era(auth, rules, player)
            .map(|e| e.index as usize)
            .unwrap_or(0);
    }
    let era = if alive == 0 { 0 } else { sum / alive };
    WARMONGER_PERCENT[era.min(WARMONGER_PERCENT.len() - 1)]
}
