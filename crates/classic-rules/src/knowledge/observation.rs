//! The AI gets only this (§22 platform/structure.md; R-07 AI stage B contract).
//!
//! The constructor (inside the core) reads the truth and hands out:
//! - the player's own units and cities in full (moves, work, stance, move
//!   orders; population, stocks, queue, buildings, worked tiles, income);
//! - the player's own economy (treasury, science, income, research);
//! - the map as the player knows it (`PlayerView`: Unknown / Remembered /
//!   Visible tiles);
//! - other players' units and cities only where the player sees them now;
//! - public diplomacy: wars and border grants between the players it has
//!   met, and its own contacts (met, last seen).
//! Nothing else of the truth is reachable through the type: two worlds that
//! look the same to the player give equal observations.

use crate::derived::income::{city_income, player_income_from, CityIncome, PlayerIncome};
use crate::derived::DerivedWorld;
use crate::ids::{
    BuildingKindId, CityHandle, EntityId, PlayerId, ProductionItem, TechId, TileId, UnitHandle,
};
pub use kernel::ai::observation::{observed_city, observed_unit, own_unit, units, ObservedCity, ObservedUnit, OwnUnit};

use crate::knowledge::state::Contact;
use crate::knowledge::view::PlayerView;
use crate::metrics::WorkCounters;
use crate::rules::CompiledRuleset;
use crate::world::city::CityCore;
use crate::world::state::AuthWorld;
use crate::world::player::ClassicPlayers;


/// One of the player's own cities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnCity {
    pub handle: CityHandle,
    /// Persistent identity (AI memory survives save/load by it).
    pub id: EntityId,
    pub tile: TileId,
    pub population: u32,
    pub food: u32,
    pub production: u32,
    pub culture: u32,
    pub damage: u16,
    /// Hit points left (maximum minus damage).
    pub hp: u16,
    pub status: crate::world::city::CityStatus,
    pub queue: Vec<ProductionItem>,
    pub buildings: Vec<BuildingKindId>,
    pub worked: Vec<TileId>,
    pub income: CityIncome,
    /// Citizen orders (focus, locked tiles, hand-assigned specialists).
    pub citizens: crate::world::citizens::CitizenPlan,
    /// What the city makes with an empty queue (Wealth, Research, a World
    /// Congress project).
    pub process: Option<crate::world::city_economy::Process>,
}

/// The player's own economy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnEconomy {
    /// The player's civilization (index into the rules' civilizations).
    pub civ: u8,
    pub treasury: u32,
    pub science: u32,
    pub income: PlayerIncome,
    pub techs: Vec<TechId>,
    pub research_queue: Vec<TechId>,
    /// Projects the player completed, by project id (Apollo, spaceship
    /// parts installed: the victory screen shows them to everyone).
    pub projects: Vec<u16>,
    /// The empire's happiness now (`derived::happiness`).
    pub happiness: crate::derived::happiness::HappinessReport,
    /// The excess happiness of the turn's snapshot (what the rules read:
    /// no settlers and no new city at very unhappy).
    pub turn_happiness: i32,
    /// Happiness the golden age meter needs for the next golden age (the
    /// top panel shows it).
    pub golden_age_threshold: u32,
    /// The empire's strategic and luxury resources (`derived::resources`).
    pub resources: crate::derived::resources::ResourceLedger,
    /// Trade routes the player may still start (the trade route screen).
    pub trade_routes_left: u32,
    /// Routes each own trade unit may start now with their yields (what
    /// the trade route screen shows a human player).
    pub trade_offers: Vec<(UnitHandle, Vec<crate::derived::trade::TradeOffer>)>,
    /// What the next free technology the player is owed may be (empty:
    /// none owed; `derived::grants`).
    pub free_technology_options: Vec<TechId>,
    /// What the next free great person the player is owed may be (empty:
    /// none owed).
    pub free_great_person_options: Vec<crate::classic::effects::GreatPersonKind>,
    /// The player may annex captured cities (Venice may not).
    pub may_annex: bool,
}

/// What the player knows about another player's relations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Relation {
    pub other: PlayerId,
    pub contact: Contact,
    pub at_war: bool,
    /// The other player lets our units onto its land in peace (its
    /// open-borders grant, or a city-state's land open to all).
    pub they_grant_entry: bool,
    /// We let the other player's units onto our land in peace.
    pub we_grant_entry: bool,
    /// The other player's kind (civilization, city-state, barbarians).
    pub kind: crate::world::player::PlayerKind,
    /// No war before this round (peace treaty; 0 = none).
    pub peace_treaty_until: u32,
    /// Round our war began (meaningful while at war).
    pub war_since: u32,
    pub our_embassy: bool,
    pub their_embassy: bool,
    pub defensive_pact: bool,
    pub research_agreement: bool,
    pub friends: bool,
    pub we_denounced: bool,
    pub they_denounced: bool,
    /// Our warmonger memory of them (public to us: it is our own).
    pub warmonger: i32,
    /// We may open our borders to them now (`Command::OpenBorders` would
    /// pass: technology, embassy, peace).
    pub can_open_borders: bool,
}

#[derive(Debug, Clone)]
pub struct AIObservation<'a> {
    pub player: PlayerId,
    /// Global round (the game's turn number).
    pub round: u32,
    /// Map sizes are common knowledge (not hidden), needed by the AI's geometry.
    pub map_width: u32,
    pub map_height: u32,
    /// The map as the player knows it.
    pub view: PlayerView<'a>,
    pub own_units: Vec<OwnUnit>,
    pub own_cities: Vec<OwnCity>,
    pub economy: OwnEconomy,
    /// Units and cities of other players on tiles the player sees now.
    pub visible_foreign_units: Vec<ObservedUnit>,
    pub visible_foreign_cities: Vec<ObservedCity>,
    /// Every other alive player the player has met or is at war with, in
    /// id order (its own diplomacy is no secret to it).
    pub relations: Vec<Relation>,
    /// Deal offers addressed to the player, waiting for its answer.
    pub offers_to_us: Vec<crate::world::relations::DealOffer>,
    /// The player's culture pool, policies and what it may adopt now.
    pub culture: crate::derived::culture::CultureOverview,
    /// The player's faith, its religious units, the religion of the cities it
    /// sees and the public facts of every religion.
    pub faith: FaithObservation,
    /// The player's great work slots, tourism, influence and public opinion.
    pub tourism: crate::derived::tourism::TourismObservation,
    /// World wonders built anywhere (every completed wonder is announced to
    /// all players), sorted.
    pub built_wonders: Vec<BuildingKindId>,
    /// The city-states the player met (the city-state screen), by id.
    pub city_states: Vec<CityStateInfo>,
    /// The World Congress as its screen shows it (None: not founded).
    pub congress: Option<CongressInfo>,
    /// Our spies (the espionage overview).
    pub spies: Vec<SpyInfo>,
    /// Victims we hold steal rights on, with what may be stolen.
    pub steal: Vec<(PlayerId, Vec<TechId>)>,
    /// The victories of the game and its last round (the setup screen).
    pub victories: crate::world::victory::VictoryOptions,
}

/// One of our spies as the espionage overview shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpyInfo {
    pub index: u8,
    /// Which of the civilization's spy names it carries (`world::espionage::spy_name`).
    pub name: u8,
    pub rank: u8,
    pub state: crate::world::espionage::SpyState,
    pub city: Option<CityHandle>,
    pub diplomat: bool,
    /// Chance of a coup now (city-states with another ally).
    pub coup_chance: Option<i64>,
    /// Turns until the theft it is gathering intelligence for (the
    /// overview's "turns left"; None in other states).
    pub theft_turns: Option<u32>,
}

/// The World Congress screen of one civilization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CongressInfo {
    pub host: Option<PlayerId>,
    pub united_nations: bool,
    pub in_session: bool,
    pub turns_until_session: u32,
    /// We may propose before the next session.
    pub can_propose: bool,
    pub proposals: Vec<crate::world::congress::Proposal>,
    pub active: Vec<crate::world::congress::Active>,
    /// Our delegates in the open session not cast yet.
    pub delegates_left: u16,
    /// Delegates the World Leader needs.
    pub needed: u16,
    /// World Congress projects under way (the project panel).
    pub projects: Vec<ProjectInfo>,
    /// World Congress projects that may be proposed now.
    pub proposable_projects: Vec<crate::rules::content::ProjectId>,
}

/// A World Congress project under way as the Congress screen shows it
/// (production ×100).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectInfo {
    pub project: crate::rules::content::ProjectId,
    /// Our contribution.
    pub ours: u64,
    /// The largest contribution of a member.
    pub best: u64,
    /// One civilization's share (tier 2 needs it, tier 1 half of it).
    pub share: u64,
    /// Contributions so far and the whole cost.
    pub total: u64,
    pub cost: u64,
}

/// One met city-state as its screen shows it to a civilization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CityStateInfo {
    pub player: PlayerId,
    pub minor_trait: Option<crate::rules::content::CityStateTrait>,
    /// Our effective influence and its resting point.
    pub influence: i32,
    pub resting: i32,
    pub friends: bool,
    pub ally: Option<PlayerId>,
    pub at_war: bool,
    pub pledged: bool,
    /// Tribute in gold would be paid now (and its amount).
    pub can_bully_gold: bool,
    pub bully_gold: u32,
    /// We may buy it out now (Austria's Diplomatic Marriage, §4.15).
    pub can_buyout: bool,
    /// Our quests with this city-state.
    pub quests: Vec<crate::world::city_states::QuestKind>,
    /// The city-state's capital, when known.
    pub capital: Option<TileId>,
}

/// Faith part of the observation.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FaithObservation {
    pub overview: crate::derived::religion::FaithOverview,
    /// Own religious units: the religion they carry and spreads left.
    pub units: Vec<(UnitHandle, crate::rules::content::ReligionId, u8)>,
    /// Own cities and the foreign cities seen now, with their majority.
    pub cities: Vec<CityFaithView>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CityFaithView {
    pub handle: CityHandle,
    pub tile: TileId,
    pub owner: PlayerId,
    pub majority: Option<crate::world::religion::Creed>,
}

impl<'a> AIObservation<'a> {
    /// Build a filtered snapshot. Pub: used by both the AI and the render
    /// projection: ONE "what the player sees" filter for two consumers (§28).
    pub fn for_player(
        auth: &'a AuthWorld,
        derived: &'a DerivedWorld,
        rules: &'a CompiledRuleset,
        player: PlayerId,
    ) -> (Self, WorkCounters) {
        let view = PlayerView::new(auth, derived, rules, player);
        let mut c = WorkCounters::default();
        let (own_units, visible_foreign_units) = {
            let _span = prof::span("civ5/obs units");
            units(auth, &view, &mut c)
        };
        let (own_cities, visible_foreign_cities) = {
            let _span = prof::span("civ5/obs cities");
            cities(auth, rules, &view, &mut c)
        };
        c.bytes_allocated = ((own_units.len() + visible_foreign_units.len()) * 40
            + (own_cities.len() + visible_foreign_cities.len()) * 96)
            as u64;
        let economy = {
            let _span = prof::span("civ5/obs economy");
            economy(auth, rules, player, &own_cities)
        };
        let culture_per_turn = own_cities.iter().map(|c| c.income.culture).sum();
        let culture = {
            let _span = prof::span("civ5/obs culture");
            crate::derived::culture::culture_overview(auth, rules, player, culture_per_turn)
        };
        let faith = {
            let _span = prof::span("civ5/obs faith");
            faith_observation(auth, rules, &view, &own_cities)
        };
        let tourism = {
            let _span = prof::span("civ5/obs tourism");
            let mut tourism = crate::derived::tourism::tourism_observation(auth, rules, player);
            // An antiquity site is a deposit on the map: the player knows the ones on tiles it has explored
            // (the tourism query lists every site of the map).
            tourism.sites.retain(|tile| view.is_known(*tile));
            tourism
        };
        let relations = {
            let _span = prof::span("civ5/obs relations");
            relations(auth, &view, player)
        };
        let built_wonders = {
            let _span = prof::span("civ5/obs built_wonders");
            built_wonders(auth, rules)
        };
        let city_states = {
            let _span = prof::span("civ5/obs city_states");
            city_states_known(auth, rules, player)
        };
        let congress = {
            let _span = prof::span("civ5/obs congress");
            congress_info(auth, rules, player)
        };
        let _spies_span = prof::span("civ5/obs spies+steal");
        let observation = Self {
            player,
            round: auth.turn().round,
            map_width: auth.map().dimensions.width,
            map_height: auth.map().dimensions.height,
            view,
            own_units,
            own_cities,
            economy,
            visible_foreign_units,
            visible_foreign_cities,
            relations,
            offers_to_us: auth
                .module()
                .relations()
                .offers()
                .iter()
                .filter(|o| o.to == player)
                .cloned()
                .collect(),
            culture,
            faith,
            tourism,
            built_wonders,
            city_states,
            congress,
            spies: spies_of(auth, player),
            steal: steal_rights(auth, rules, player),
            victories: auth.module().victory().options(),
        };
        (observation, c)
    }

    /// Our relation with `other` (None: not met).
    pub fn relation(&self, other: PlayerId) -> Option<&Relation> {
        self.relations.iter().find(|r| r.other == other)
    }
}

/// What one decision of one unit sees, rebuilt after every command (R-07
/// §4: refresh the facts around the actor, not the whole world). Same
/// filters as `AIObservation`: the unit itself, the player's own units and
/// cities within `radius`, other players' units and cities visible there.
#[derive(Debug, Clone)]
pub struct LocalObservation<'a> {
    pub player: PlayerId,
    pub round: u32,
    pub view: PlayerView<'a>,
    pub unit: OwnUnit,
    pub radius: i32,
    /// The player's other units within the radius, in id order.
    pub own_units: Vec<OwnUnit>,
    /// The player's cities within the radius.
    pub own_cities: Vec<ObservedCity>,
    /// Other players' units visible within the radius, in tile order.
    pub foreign_units: Vec<ObservedUnit>,
    /// Other players' cities visible within the radius.
    pub foreign_cities: Vec<ObservedCity>,
    pub relations: Vec<Relation>,
}

impl<'a> LocalObservation<'a> {
    /// None when the unit is gone or belongs to another player.
    pub fn for_unit(
        auth: &'a AuthWorld,
        derived: &'a DerivedWorld,
        rules: &'a CompiledRuleset,
        player: PlayerId,
        unit: UnitHandle,
        radius: i32,
    ) -> Option<(Self, WorkCounters)> {
        let core = auth.units().get(unit)?.core;
        if core.owner != player {
            return None;
        }
        let view = PlayerView::new(auth, derived, rules, player);
        let mut observation = Self {
            player,
            round: auth.turn().round,
            view,
            unit: own_unit(auth, rules, unit, core),
            radius,
            own_units: Vec::new(),
            own_cities: Vec::new(),
            foreign_units: Vec::new(),
            foreign_cities: Vec::new(),
            relations: relations(auth, &view, player),
        };
        let mut counters = WorkCounters::default();
        for tile in crate::math::hex_disc(auth.map().dimensions, core.tile, radius) {
            counters.tiles_visited += 1;
            observation.collect_units(auth, tile, unit, &mut counters);
            observation.collect_city(auth, tile);
        }
        observation.own_units.sort_by_key(|u| u.id);
        observation
            .foreign_units
            .sort_by_key(|u| (u.tile, u.handle.index));
        Some((observation, counters))
    }

    fn collect_units(
        &mut self,
        auth: &AuthWorld,
        tile: TileId,
        actor: UnitHandle,
        c: &mut WorkCounters,
    ) {
        for &handle in auth.indexes().units_on(tile) {
            let Some(core) = auth.units().get(handle).map(|u| u.core) else {
                continue;
            };
            c.entities_visited += 1;
            if core.owner == self.player {
                if handle != actor {
                    self.own_units.push(own_unit(auth, self.view.rules(), handle, core));
                }
            } else if self.view.sees_unit(core) {
                self.foreign_units.push(observed_unit(handle, core));
            }
        }
    }

    fn collect_city(&mut self, auth: &AuthWorld, tile: TileId) {
        let Some(handle) = auth.indexes().city_on_tile(tile) else {
            return;
        };
        let Some(core) = auth.cities().get(handle) else {
            return;
        };
        let rules = self.view.rules();
        if core.owner == self.player {
            self.own_cities.push(observed_city(auth, rules, handle, core));
        } else if self.view.is_visible(tile) {
            self.foreign_cities.push(observed_city(auth, rules, handle, core));
        }
    }

    /// Our relation with `other` (None: not met).
    pub fn relation(&self, other: PlayerId) -> Option<&Relation> {
        self.relations.iter().find(|r| r.other == other)
    }
}





fn cities(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    view: &PlayerView<'_>,
    c: &mut WorkCounters,
) -> (Vec<OwnCity>, Vec<ObservedCity>) {
    let player = view.player();
    let mut own = Vec::new();
    let mut foreign = Vec::new();
    for (handle, core) in auth.cities().iter() {
        c.entities_visited += 1;
        if core.owner == player {
            own.push(own_city(auth, rules, handle, core));
        } else if view.is_visible(core.tile) {
            foreign.push(observed_city(auth, rules, handle, core));
        }
    }
    own.sort_by_key(|city| city.id);
    (own, foreign)
}

fn own_city(auth: &AuthWorld, rules: &CompiledRuleset, handle: CityHandle, core: &CityCore) -> OwnCity {
    let worked = crate::derived::yields::city_work(auth, rules, core).tiles().collect();
    OwnCity {
        handle,
        id: core.id,
        tile: core.tile,
        population: core.population,
        food: core.food_times100 / 100,
        production: core.production,
        culture: core.culture,
        damage: core.damage,
        hp: crate::combat::city::max_hit_points(auth, rules, core).saturating_sub(core.damage),
        status: core.status,
        queue: core.queue.clone(),
        buildings: core.buildings.clone(),
        worked,
        income: city_income(auth, rules, core),
        citizens: core.citizens.clone(),
        process: core.economy.process,
    }
}

fn economy(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    player: PlayerId,
    cities: &[OwnCity],
) -> OwnEconomy {
    let players = auth.players();
    let incomes: Vec<CityIncome> = cities.iter().map(|c| c.income).collect();
    OwnEconomy {
        civ: players.civ_of(player),
        treasury: players.treasury_of(player),
        science: players.science_of(player),
        income: player_income_from(auth, rules, player, &incomes),
        techs: players.techs_of(player).to_vec(),
        research_queue: players.research_queue_of(player).to_vec(),
        projects: (0..rules.module.projects.len())
            .map(|p| auth.module().victory().project_count(player, crate::rules::content::ProjectId(p as u16)))
            .collect(),
        happiness: crate::derived::happiness::player_happiness(auth, rules, player),
        turn_happiness: crate::derived::happiness::excess_snapshot(auth, player),
        golden_age_threshold: crate::systems::golden_age::golden_age_threshold(auth, player),
        resources: crate::derived::resources::resource_ledger(auth, rules, player),
        trade_routes_left: crate::derived::trade::routes_possible(auth, rules, player)
            .saturating_sub(crate::derived::trade::routes_used(auth, rules, player, true)),
        trade_offers: own_trade_offers(auth, rules, player),
        free_technology_options: crate::derived::grants::next_technology_options(auth, rules, player),
        free_great_person_options: crate::derived::grants::next_great_person_options(auth, rules, player),
        may_annex: !crate::effects::player::forbidden(
            auth,
            rules,
            player,
            crate::rules::effects::Forbidden::Annexing,
        ),
    }
}

/// Offers of the player's trade units (none without trade units: the
/// usual case costs one pass over the player's units).
fn own_trade_offers(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    player: PlayerId,
) -> Vec<(UnitHandle, Vec<crate::derived::trade::TradeOffer>)> {
    auth.indexes()
        .units_of(player)
        .iter()
        .filter(|u| auth.units().get(**u).is_some_and(|v| crate::derived::trade::is_trade_unit(rules, v.core.kind)))
        .map(|u| (*u, crate::derived::trade::trade_offers(auth, rules, *u)))
        .collect()
}

/// The player's faith, its religious units and the religion of the cities
/// it owns or sees now (a city's religion is shown on its banner).
fn faith_observation(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    view: &PlayerView<'_>,
    own: &[OwnCity],
) -> FaithObservation {
    let player = view.player();
    let per_turn = own.iter().map(|c| c.income.faith).sum();
    let overview = crate::derived::religion::faith_overview(auth, rules, player, per_turn);
    let mut units = Vec::new();
    for handle in auth.indexes().units_of(player) {
        let Some(core) = auth.units().get(*handle).map(|v| *v.core) else {
            continue;
        };
        if let Some(carried) = auth.module().religion().unit(core.id) {
            units.push((*handle, carried.religion, carried.spreads_left));
        }
    }
    let mut cities = Vec::new();
    for (handle, core) in auth.cities().iter() {
        if core.owner != player && !view.is_visible(core.tile) {
            continue;
        }
        cities.push(CityFaithView {
            handle,
            tile: core.tile,
            owner: core.owner,
            majority: crate::derived::religion::majority(
                auth.module().religion().city(handle),
                core.population,
            ),
        });
    }
    FaithObservation {
        overview,
        units,
        cities,
    }
}

/// World wonders standing in any city (public: they are announced).
fn built_wonders(auth: &AuthWorld, rules: &CompiledRuleset) -> Vec<BuildingKindId> {
    let is_wonder =
        |b: &BuildingKindId| rules.buildings.get(b.0 as usize).is_some_and(|d| d.wonder);
    let mut out: Vec<BuildingKindId> = Vec::new();
    for (_, core) in auth.cities().iter() {
        out.extend(core.buildings.iter().filter(|b| is_wonder(b)));
    }
    out.sort_by_key(|b| b.0);
    out.dedup();
    out
}

/// Public diplomacy with every alive player met or at war with: war and
/// border grants are known to both sides (the diplomacy screen shows
/// them), eliminations are announced.
fn relations(auth: &AuthWorld, view: &PlayerView<'_>, player: PlayerId) -> Vec<Relation> {
    let diplomacy = auth.diplomacy();
    let mut out = Vec::new();
    for p in 0..auth.players().count() {
        let other = PlayerId(p as u8);
        let contact = view.contact(other);
        let known = contact.met_round.is_some() || diplomacy.at_war(player, other);
        if other == player || !known || !auth.players().is_alive(other) {
            continue;
        }
        let r = auth.module().relations();
        let round = auth.turn().round;
        out.push(Relation {
            other,
            contact,
            at_war: diplomacy.at_war(player, other),
            they_grant_entry: crate::world::territory::lets_in(auth, other, player),
            we_grant_entry: crate::world::territory::lets_in(auth, player, other),
            kind: auth.players().kind_of(other),
            peace_treaty_until: r.peace_treaty_until(player, other),
            war_since: r.war_since(player, other),
            our_embassy: r.has_embassy(player, other),
            their_embassy: r.has_embassy(other, player),
            defensive_pact: r.has_pact(player, other, round),
            research_agreement: r.has_research_agreement(player, other, round),
            friends: r.are_friends(player, other, round),
            we_denounced: r.denounced(player, other, round),
            they_denounced: r.denounced(other, player, round),
            warmonger: r.warmonger(player, other),
            can_open_borders: auth.players().is_major(player)
                && crate::systems::diplomacy::deals::item_problem(
                    auth,
                    view.rules(),
                    player,
                    other,
                    crate::world::relations::DealItem::OpenBorders,
                    false,
                )
                .is_none(),
        });
    }
    out
}


/// The city-states `player` met (civilizations only; nothing for others).
fn city_states_known(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId) -> Vec<CityStateInfo> {
    use crate::world::city_states as cs;
    if !auth.players().is_major(player) {
        return Vec::new();
    }
    (0..auth.players().count())
        .map(|p| PlayerId(p as u8))
        .filter(|p| auth.players().is_city_state(*p) && auth.players().is_alive(*p))
        .filter(|p| crate::systems::diplomacy::has_met(auth, player, *p))
        .map(|p| CityStateInfo {
            player: p,
            minor_trait: cs::type_of(auth, rules, p).and_then(|t| t.minor_trait),
            influence: cs::effective_influence(auth, p, player),
            // The one resting point of the rules (Papal Primacy included).
            resting: crate::systems::city_states::resting_point(auth, rules, p, player),
            friends: cs::is_friends(auth, rules, p, player),
            ally: auth.module().city_states().ally(p),
            at_war: auth.diplomacy().at_war(p, player),
            pledged: auth.module().city_states().standing(p, player).pledged.is_some(),
            can_bully_gold: cs::bully_score(auth, rules, p, player, false) >= 0,
            bully_gold: cs::bully_gold_amount(auth.turn().round),
            can_buyout: !auth.diplomacy().at_war(p, player)
                && cs::buyout_problem(auth, rules, p, player).is_none(),
            quests: auth
                .module()
                .city_states()
                .quests()
                .iter()
                .filter(|q| q.city_state == p && q.major == player)
                .map(|q| q.kind)
                .collect(),
            capital: auth
                .indexes()
                .cities_of(p)
                .first()
                .and_then(|c| auth.cities().get(*c))
                .map(|c| c.tile),
        })
        .collect()
}

fn congress_info(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId) -> Option<CongressInfo> {
    let c = auth.module().congress();
    if !c.founded() || !auth.players().is_major(player) {
        return None;
    }
    Some(CongressInfo {
        host: c.host(),
        united_nations: c.united_nations(),
        in_session: c.in_session(),
        turns_until_session: c.turns_until_session(),
        can_propose: c.proposers().contains(&player)
            && !c.proposals().iter().any(|p| p.proposer == Some(player)),
        proposals: c.proposals().to_vec(),
        active: c.active().to_vec(),
        delegates_left: c.delegates_left(player),
        needed: crate::systems::congress::votes_needed(auth),
        projects: project_infos(auth, rules, player),
        proposable_projects: (0..rules.module.projects.len())
            .map(|i| crate::rules::content::ProjectId(i as u16))
            .filter(|p| crate::systems::league_projects::project_problem(auth, rules, *p).is_none())
            .collect(),
    })
}

fn project_infos(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId) -> Vec<ProjectInfo> {
    let mut out = Vec::new();
    for progress in auth.module().congress().projects() {
        let Some(league) = crate::systems::league_projects::league_of(rules, progress.project) else {
            continue;
        };
        out.push(ProjectInfo {
            project: progress.project,
            ours: progress.of(player),
            best: progress.contributions.iter().copied().max().unwrap_or(0),
            share: crate::systems::league_projects::share_times100(league),
            total: progress.total(),
            cost: crate::systems::league_projects::cost_times100(auth, league),
        });
    }
    out
}

fn spies_of(auth: &AuthWorld, player: PlayerId) -> Vec<SpyInfo> {
    auth.module().espionage()
        .spies_of(player)
        .iter()
        .enumerate()
        .map(|(i, s)| SpyInfo {
            index: i as u8,
            name: s.name,
            rank: s.rank,
            state: s.state,
            city: s.city,
            diplomat: s.diplomat,
            coup_chance: crate::systems::espionage::coup_chance(auth, player, s),
            theft_turns: theft_turns(s),
        })
        .collect()
}

/// Turns until a spy gathering intelligence is done.
fn theft_turns(spy: &crate::world::espionage::Spy) -> Option<u32> {
    if spy.state != crate::world::espionage::SpyState::GatheringIntel || spy.rate <= 0 {
        return None;
    }
    let left = (spy.goal - spy.amount).max(0);
    Some(((left + spy.rate - 1) / spy.rate).min(i64::from(u32::MAX)) as u32)
}

fn steal_rights(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId) -> Vec<(PlayerId, Vec<TechId>)> {
    (0..auth.players().count())
        .map(|p| PlayerId(p as u8))
        .filter(|victim| auth.module().espionage().steal_rights(player, *victim) > 0)
        .map(|victim| (victim, crate::systems::espionage::stealable(auth, rules, player, victim)))
        .filter(|(_, techs)| !techs.is_empty())
        .collect()
}
