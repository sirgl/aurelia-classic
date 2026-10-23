//! Great people (classic/docs/classic/rules.md): points from
//! specialists and buildings in every city, births at the player's
//! threshold, and the actions of great people. Journal: UnitChange::Created
//! (births), UnitChange::Destroyed (an action uses the unit up),
//! TileChange::ImprovementBuilt / OwnerChanged (improvements, citadel),
//! ClassicChange::GoldenAgeChanged.
//!
//! ```text
//! points of a specialist kind S in a city, each turn (hundredths):
//!   (GreatPeopleRateChange(S) × count(S) + building points of S's kind) × 100
//!   × (100 + Σ GreatPeopleRate + Σ GreatPersonRate(kind), city and player) / 100
//! birth when points ≥ threshold × 100: points = 0, the unit appears in the
//!   city, the counter of its kind grows
//! threshold = 100 + 100 × born so far   (writers, artists, musicians: own
//!   counters; scientists, engineers, merchants share one)
//! ```
//!
//! Actions (`GreatPersonAction`), each uses the unit up:
//! construct    its improvement on the tile it stands on (academy, manufactory,
//!              customs house, citadel — the citadel claims the tiles around)
//! discover     science of the player's last turns (great scientist),
//!              × (100 + Stat::Module(crate::rules::stats::ClassicStat::GreatScientistSciencePercent)) / 100
//! hurry        production 300 + 30 × population in the city it stands in
//!              (great engineer)
//! golden age   a golden age of 8 turns (great artist)
//! trade        on a city-state's land at peace (great merchant): gold
//!              (300 + 100 × era) × (100 + TradeMissionGoldPercent) / 100
//!              and +30 influence with that city-state (§4.11)
//!
//! Generals and admirals come from combat experience (track C calls
//! `gain_combat_experience`); the prophet comes from faith (track B).

use crate::classic::content::GreatPersonKind;
use crate::command::CommandError;
use crate::derived::great_people::{
    bulb_science, city_points, count_birth, great_person_threshold, trade_mission_city_state,
    trade_mission_gold, validate_action, GreatPersonAction, MIN_GREAT_PERSON_GOLDEN_AGE_TURNS,
};

/// Influence a trade mission gives with a city-state. Unciv: Units.json
/// (Gods & Kings, Great Merchant, 30 Influence); equal. The gold is the
/// unit's data, (350 + 50 × era) as Unciv: UnitActionsGreatPerson.kt at
/// Standard speed.
pub const TRADE_MISSION_INFLUENCE: i32 = 30;
use crate::classic::effects::ClassicUnitStat;
use crate::derived::yields::CityBreakdown;
use crate::rules::effects::UnitStat;
use crate::ids::{CityHandle, ImprovementId, PlayerId, TileId, UnitHandle};
use crate::classic::tables::GreatPersonDefinition;
use crate::rules::CompiledRuleset;
use crate::transaction::journal::TileChange;
use crate::transaction::transaction::WorldTransaction;

impl<'a> WorldTransaction<'a> {
    /// Points of one city this turn and the births they bring. Runs first
    /// in the city's steps.
    pub(crate) fn accumulate_great_people(
        &mut self,
        owner: PlayerId,
        city: CityHandle,
        yields: &CityBreakdown,
        rules: &CompiledRuleset,
    ) {
        let specialists = yields.work.specialist_count() > 0;
        let buildings = yields.modifiers.module.great_person_points.iter().any(|p| *p > 0);
        if !specialists && !buildings {
            return;
        }
        // The city loop asks again and again: the transaction keeps the answer
        // while its journal is unchanged.
        let mut player_rate = self.0.player_modifiers_scoped(rules, owner).stats.clone();
        let friends = crate::derived::great_people::friendship_rate_with(self.world, owner, |p| {
            crate::derived::great_people::friend_stat_of(&self.0.player_modifiers_scoped(rules, p))
        });
        player_rate.add(crate::rules::stats::Stat::Module(crate::rules::stats::ClassicStat::GreatPeopleRate), friends);
        // The World Congress funding resolutions move the points of whole families of great people.
        for kind in [GreatPersonKind::Writer, GreatPersonKind::Artist, GreatPersonKind::Musician, GreatPersonKind::Scientist, GreatPersonKind::Engineer, GreatPersonKind::Merchant] {
            let shift = crate::systems::congress::funding_percent(self.world, kind);
            if shift != 0 {
                player_rate.add(crate::rules::stats::Stat::Module(crate::rules::stats::ClassicStat::GreatPersonRate(kind)), shift);
            }
        }
        let points = city_points(rules, yields, &player_rate);
        for def in &rules.module.great_people {
            let Some(specialist) = def.specialist else {
                continue;
            };
            let s = specialist.0 as usize;
            let threshold = great_person_threshold(self.world, owner, def.kind);
            let Some(state) = self.world.cities.module_mut(city) else {
                return;
            };
            let Some(progress) = state.great_person_points.get_mut(s) else {
                continue;
            };
            *progress = progress.saturating_add(points[s]);
            if *progress / 100 < threshold {
                continue;
            }
            *progress = 0;
            let Some(tile) = self.world.cities.get(city).map(|core| core.tile) else {
                return;
            };
            self.give_birth(owner, tile, def, rules);
        }
    }

    /// A great person of `def` appears at `tile` (a city) and counts.
    fn give_birth(
        &mut self,
        owner: PlayerId,
        tile: TileId,
        def: &GreatPersonDefinition,
        rules: &CompiledRuleset,
    ) {
        let civ = self.world.players.civ_of(owner);
        let key = &rules.units[def.unit.0 as usize].key;
        let unit = rules.civ_variant(civ, key).unwrap_or(def.unit);
        self.spawn_at_city(owner, tile, unit, rules);
        if let Some(economy) = self.world.players.economy_mut(owner) {
            count_birth(economy, def.kind);
        }
    }

    /// Carry out a validated great person action (see the module docs).
    pub(crate) fn great_person_action(
        &mut self,
        unit: UnitHandle,
        action: GreatPersonAction,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        validate_action(self.world, rules, unit, action)?;
        let core = *self
            .world
            .units
            .get(unit)
            .ok_or(CommandError::UnknownUnit)?
            .core;
        let owner = core.owner;
        let abilities = rules.units[core.kind.0 as usize].module.economy;
        match action {
            GreatPersonAction::Construct(improvement) => {
                self.construct(owner, core.tile, improvement, rules)
            }
            GreatPersonAction::DiscoverTechnology => {
                let science = scientist_science(self.world, rules, owner, abilities.beakers_turns);
                self.world.players.earn_science(owner, science);
                self.research(owner, rules);
            }
            GreatPersonAction::HurryProduction => {
                let city = self
                    .world
                    .indexes
                    .city_on_tile(core.tile)
                    .ok_or(CommandError::UnknownCity)?;
                let population = self
                    .world
                    .cities
                    .get(city)
                    .map(|c| c.population)
                    .unwrap_or(0);
                let production = abilities.base_hurry + abilities.hurry_multiplier * population;
                self.world.cities.add_production(city, production);
                let mods = {
                    let city_core = self
                        .world
                        .cities
                        .get(city)
                        .ok_or(CommandError::UnknownCity)?;
                    crate::effects::city::city_modifiers(self.world, rules, city_core)
                };
                self.drain_queue(owner, city, core.tile, rules, &mods);
            }
            GreatPersonAction::StartGoldenAge => {
                let base = abilities.golden_age_turns.max(MIN_GREAT_PERSON_GOLDEN_AGE_TURNS);
                let turns = crate::systems::golden_age::golden_age_length(self.world, rules, owner, base);
                self.start_golden_age(owner, turns);
            }
            GreatPersonAction::TradeMission => {
                let city_state = trade_mission_city_state(self.world, owner, core.tile)?;
                // The unit's own bonus (the Merchant of Venice) multiplies the
                // player's (Entrepreneurship).
                let mods = crate::effects::unit::unit_modifiers(self.world, rules, &core);
                let unit_gold = mods.stat(UnitStat::Module(ClassicUnitStat::TradeMissionGoldPercent));
                let unit_influence = mods.stat(UnitStat::Module(ClassicUnitStat::TradeMissionInfluencePercent));
                let gold = trade_mission_gold(self.world, rules, owner, abilities, unit_gold);
                self.world.players.earn(owner, gold);
                let influence = TRADE_MISSION_INFLUENCE * (100 + unit_influence).max(0) / 100;
                self.change_influence(city_state, owner, influence * 100, rules);
            }
            GreatPersonAction::BuyCityState => {
                let city_state = trade_mission_city_state(self.world, owner, core.tile)?;
                self.expend_unit(unit, rules)?;
                return self.acquire_city_state(owner, city_state, crate::world::city::CityStatus::Puppet, rules);
            }
        }
        self.expend_unit(unit, rules)?;
        Ok(())
    }

    /// A unit is used up by its action: it leaves the map, and when it is a
    /// great person its owner's OnGreatPersonExpended effects fire
    /// (Reliquary, Mausoleum of Halicarnassus).
    /// Every action that spends a great person goes through here; deaths
    /// in combat, gifts and disbanding do not.
    pub(crate) fn expend_unit(
        &mut self,
        unit: UnitHandle,
        rules: &CompiledRuleset,
    ) -> Result<crate::world::unit::UnitCore, CommandError> {
        let core = self.destroy_unit(unit)?;
        if crate::derived::great_people::great_person_of(rules, core.kind).is_some() {
            let player = core.owner;
            self.fire(rules, crate::systems::effects::TriggerEvent::GreatPersonExpended { player });
        }
        Ok(core)
    }

    /// Build a great person improvement where the unit stands; a culture
    /// bomb (citadel) claims the tiles around for the tile's city.
    fn construct(
        &mut self,
        owner: PlayerId,
        tile: TileId,
        improvement: ImprovementId,
        rules: &CompiledRuleset,
    ) {
        self.place_improvement(owner, tile, improvement, rules);
        let radius = rules.improvements[improvement.0 as usize].culture_bomb_radius;
        if radius == 0 {
            return;
        }
        let Some(city) = self.world.map.owning_city(tile) else {
            return;
        };
        let dims = self.world.map.dimensions;
        for near in crate::math::hex_disc(dims, tile, i32::from(radius)) {
            let foreign_city = self.world.indexes.city_on_tile(near).is_some();
            let mine = self.world.map.owner(near).flatten() == Some(owner);
            if near == tile || foreign_city || mine {
                continue;
            }
            let from = self.world.map.owner(near).flatten();
            self.world.map.set_owner(near, Some(owner), Some(city));
            self.journal.tile_changes.push(TileChange::OwnerChanged {
                tile: near,
                from,
                to: Some(owner),
            });
        }
    }

    /// Remember this turn's science (the great scientist reads the last
    /// turns), newest first.
    pub(crate) fn record_science(&mut self, player: PlayerId, science: u32) {
        let Some(economy) = self.world.players.economy_mut(player) else {
            return;
        };
        economy.science_history.rotate_right(1);
        economy.science_history[0] = science;
    }
}

/// A great scientist's science: the last turns' science, plus the owner's
/// `GreatScientistSciencePercent` of it (International Space
/// Station).
pub fn scientist_science(
    auth: &crate::world::state::AuthWorld,
    rules: &CompiledRuleset,
    owner: PlayerId,
    turns: u16,
) -> u32 {
    let base = bulb_science(auth, owner, turns);
    let stats = &crate::effects::player::player_modifiers(auth, rules, owner).stats;
    let percent = stats.get(crate::rules::stats::Stat::Module(crate::rules::stats::ClassicStat::GreatScientistSciencePercent));
    (i64::from(base) * i64::from((100 + percent).max(0)) / 100) as u32
}
