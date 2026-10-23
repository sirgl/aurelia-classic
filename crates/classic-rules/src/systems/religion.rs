//! Faith and religion (classic/docs/classic/rules.md): the faith pool, the
//! pantheon, founding / enhancing / reforming a religion, religious
//! pressure between cities each round, missionaries, prophets and
//! inquisitors, faith purchases, the Great Prophet roll and the improvement
//! a great person creates. The rules live in `derived::religion` (one set for
//! validation, commit, HUD and AI); this module only mutates.
//!
//! Beliefs that act here: OnReligionSpread beliefs fire before a spread
//! lands (Interfaith Dialogue), a city's first conversion fires the
//! OnCityConverted beliefs of its new religion once (Initiation Rites,
//! `AdoptionBonus`), a missionary erodes the other religions
//! (`OtherReligionErosion`) and may convert barbarians
//! (`ConvertsBarbarians`), removed religions keep `PressureRetention`,
//! friendly city-states take more pressure (`FriendlyCityStatePressure`),
//! a founder may take a bonus belief (`ExtraFoundingBelief`). Spending a
//! prophet fires OnGreatPersonExpended (`systems::great_people::expend_unit`).
//!
//! Journal: ClassicChange::ReligionChanged for every city whose majority,
//! followers or beliefs may have changed (its yields follow), UnitChange::
//! Created / Destroyed for spawned and spent units, CityChange::
//! BuildingAdded for bought buildings, TileChange::ImprovementBuilt.

use crate::classic::effects::{ClassicSource, ClassicTrigger};
use crate::command::CommandError;
use crate::derived::religion::{
    enhance_status, faith_purchase_status, found_status, heresy_status, pantheon_status,
    prophet_cost, reformation_status, spread_status, FoundingChoice, ADJACENT_CITY_DISTANCE,
    ADJACENT_CITY_PRESSURE, FOUNDING_CITY_PRESSURE, HOLY_CITY_PRESSURE, INDUSTRIAL_ERA,
    PRESSURE_PER_POP, PROPHET_BASE_CHANCE,
};
use crate::ids::{BeliefId, CityHandle, PlayerId, ProductionItem, UnitHandle};
use crate::rules::content::{ReligionId, UnitAbility};
use crate::rules::effects::{Modifier, Trigger};
use crate::classic::effects::{GreatPersonKind, ReligionAbility, ReligionStat};
use crate::rules::CompiledRuleset;
use crate::systems::effects::TriggerEvent;
use crate::classic::ClassicChange;
use crate::transaction::journal::{CityChange, TileChange};
use crate::transaction::transaction::WorldTransaction;
use crate::world::religion::{AdoptionBonus, Creed, FoundedReligion, ReligiousUnit};
use crate::world::player::ClassicPlayers;

impl<'a> WorldTransaction<'a> {
    /// Found the player's pantheon: pay the game's next pantheon price, every
    /// city of the player gets pantheon pressure (2 × 1000 per citizen).
    pub(crate) fn found_pantheon(
        &mut self,
        player: PlayerId,
        belief: BeliefId,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        let cost = pantheon_status(self.world, rules, player, belief).map_err(rule)?;
        if let Some(faith) = self.world.module.religion.player_mut(player) {
            faith.faith -= cost;
            faith.pantheon = Some(belief);
        }
        self.world.module.religion.count_pantheon();
        for city in self.world.indexes.cities_of(player).to_vec() {
            let population = self.population(city);
            self.add_pressure(city, Creed::Pantheon, population * PRESSURE_PER_POP * 2, rules);
        }
        Ok(())
    }

    /// A Great Prophet founds a religion in its city (the holy city).
    pub(crate) fn found_religion(
        &mut self,
        unit: UnitHandle,
        choice: &FoundingChoice,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        let city = found_status(self.world, rules, unit, choice).map_err(rule)?;
        let owner = self.unit_owner(unit)?;
        let mut beliefs = Vec::new();
        let pantheon = choice
            .pantheon
            .or(self.world.module.religion.player(owner).pantheon);
        if let Some(p) = pantheon {
            beliefs.push(p);
        }
        beliefs.push(choice.founder);
        beliefs.push(choice.follower);
        if let Some(bonus) = choice.bonus {
            beliefs.push(bonus);
        }
        if let Some(faith) = self.world.module.religion.player_mut(owner) {
            faith.pantheon = pantheon;
            faith.religion = Some(choice.religion);
        }
        self.world.module.religion.add_religion(FoundedReligion {
            religion: choice.religion,
            founder: owner,
            holy_city: city,
            beliefs,
            enhanced: false,
        });
        let population = self.population(city);
        self.add_pressure(
            city,
            Creed::Religion(choice.religion),
            FOUNDING_CITY_PRESSURE * population,
            rules,
        );
        self.spend_religious_unit(unit, rules)
    }

    /// A Great Prophet enhances its owner's religion: one more follower and
    /// an enhancer belief.
    pub(crate) fn enhance_religion(
        &mut self,
        unit: UnitHandle,
        follower: BeliefId,
        enhancer: BeliefId,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        let religion = enhance_status(self.world, rules, unit, follower, enhancer).map_err(rule)?;
        if let Some(founded) = self.world.module.religion.religion_mut(religion) {
            founded.beliefs.push(follower);
            founded.beliefs.push(enhancer);
            founded.enhanced = true;
        }
        self.journal_religion_cities(religion);
        self.spend_religious_unit(unit, rules)
    }

    /// The founder adds a Reformation belief (Piety's Reformation policy).
    pub(crate) fn add_reformation_belief(
        &mut self,
        player: PlayerId,
        belief: BeliefId,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        let religion = reformation_status(self.world, rules, player, belief).map_err(rule)?;
        if let Some(founded) = self.world.module.religion.religion_mut(religion) {
            founded.beliefs.push(belief);
        }
        self.journal_religion_cities(religion);
        Ok(())
    }

    /// A missionary or a prophet spreads its religion to the city it stands
    /// in or next to (rules.md): the religion's OnReligionSpread
    /// beliefs fire first (Interfaith Dialogue counts the city as it was);
    /// a prophet removes the other religions (each keeps its
    /// `PressureRetention` percent); a missionary's pressure erodes the
    /// other religions by `OtherReligionErosion` percent of it and may
    /// convert the barbarians next to it; one spread is used, the unit goes
    /// when none is left.
    pub(crate) fn spread_religion(
        &mut self,
        unit: UnitHandle,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        let spread = spread_status(self.world, rules, unit).map_err(rule)?;
        let owner = self.unit_owner(unit)?;
        self.fire(
            rules,
            TriggerEvent::ReligionSpread {
                player: owner,
                city: spread.city,
                religion: spread.religion,
            },
        );
        let mods = crate::classic::religion_effects::religion_modifiers(self.world, rules, spread.religion);
        let erosion = match spread.prophet {
            true => 0,
            false => percent_of(spread.pressure, mods.of(ReligionStat::OtherReligionErosion)),
        };
        let retention = self.retention_in(spread.city, rules);
        let creed = Creed::Religion(spread.religion);
        self.change_city_faith(spread.city, rules, |faith| {
            if spread.prophet {
                remove_other_religions(faith, spread.religion, &retention);
            }
            faith.add(creed, spread.pressure);
            faith.reduce(Creed::Pantheon, spread.pressure);
            erode_other_religions(faith, spread.religion, erosion);
        });
        if !spread.prophet && mods.has(ReligionAbility::ConvertsBarbarians) {
            self.convert_barbarians_near(unit, owner, rules);
        }
        let id = self.unit_id(unit)?;
        let left =
            crate::derived::religion::spreads_left(self.world, rules, unit).saturating_sub(1);
        let strength_lost = self.world.module.religion.unit(id).map_or(0, |u| u.strength_lost);
        self.world.module.religion.add_unit(ReligiousUnit {
            unit: id,
            religion: spread.religion,
            spreads_left: left,
            strength_lost,
        });
        if left == 0 {
            return self.spend_religious_unit(unit, rules);
        }
        Ok(())
    }

    /// An inquisitor removes the other religions from one of its owner's
    /// cities and is spent.
    pub(crate) fn remove_heresy(
        &mut self,
        unit: UnitHandle,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        let (city, religion) = heresy_status(self.world, rules, unit).map_err(rule)?;
        let retention = self.retention_in(city, rules);
        self.change_city_faith(city, rules, |faith| {
            remove_other_religions(faith, religion, &retention);
        });
        self.spend_religious_unit(unit, rules)
    }

    /// The `PressureRetention` percent of every religion with pressure in
    /// the city (Unity of the Prophets), religions without it left out.
    fn retention_in(&self, city: CityHandle, rules: &CompiledRuleset) -> Vec<(ReligionId, i32)> {
        let mut out = Vec::new();
        for p in &self.world.module.religion.city(city).pressure {
            let Creed::Religion(religion) = p.creed else {
                continue;
            };
            let mods = crate::classic::religion_effects::religion_modifiers(self.world, rules, religion);
            let percent = mods.of(ReligionStat::PressureRetention);
            if percent > 0 {
                out.push((religion, percent));
            }
        }
        out
    }

    /// Heathen Conversion: the barbarian land units next to the missionary
    /// become its owner's (hit points and experience kept, no moves left
    /// this turn). Unverified: the exact trigger moment (when the missionary
    /// acts) is not in rules.md.
    fn convert_barbarians_near(&mut self, unit: UnitHandle, owner: PlayerId, rules: &CompiledRuleset) {
        let Some(barbarian) = self.world.players.barbarian() else {
            return;
        };
        let Some(tile) = self.world.units.get(unit).map(|v| v.core.tile) else {
            return;
        };
        let mut converts: Vec<UnitHandle> = Vec::new();
        for near in self.world.map.dimensions.neighbors(tile) {
            for other in self.world.indexes.units_on(near) {
                let land_barbarian = self.world.units.get(*other).is_some_and(|v| {
                    v.core.owner == barbarian
                        && rules.units[v.core.kind.0 as usize].domain == crate::rules::content::Domain::Land
                });
                if land_barbarian {
                    converts.push(*other);
                }
            }
        }
        for barbarian_unit in converts {
            let Ok(old) = self.destroy_unit(barbarian_unit) else {
                continue;
            };
            let new = self.spawn_unit(owner, old.tile, old.kind, rules);
            if let Some(core) = self.world.units.get_mut_core(new) {
                core.moves_left = 0;
                core.hp = old.hp;
                core.experience = old.experience;
                core.promotions = old.promotions;
            }
        }
    }

    /// Buy a unit or a building with faith (rules.md).
    pub(crate) fn purchase_with_faith(
        &mut self,
        player: PlayerId,
        city: CityHandle,
        item: ProductionItem,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        let cost = faith_purchase_status(self.world, rules, player, city, item)
            .map_err(|b| CommandError::RuleViolation(b.reason().into()))?;
        if let Some(faith) = self.world.module.religion.player_mut(player) {
            faith.faith -= cost;
        }
        match item {
            ProductionItem::Building(building) => {
                if let crate::world::city::CityWrite::Applied =
                    self.world.cities.add_building(city, building)
                {
                    self.journal
                        .city_changes
                        .push(CityChange::BuildingAdded { city, building });
                }
            }
            ProductionItem::Unit(kind) => {
                let majority = crate::derived::religion::city_majority(self.world, city);
                let tile = self
                    .world
                    .cities
                    .get(city)
                    .map(|c| c.tile)
                    .ok_or(CommandError::UnknownCity)?;
                let unit = self.spawn_at_city(player, tile, kind, rules);
                self.count_faith_purchase(player, kind, rules);
                let extra = missionary_extra_spreads(self.world, rules, city, kind);
                self.register_religious_unit(unit, kind, majority, extra, rules);
            }
            // Refused by `faith_purchase_status` (projects have no faith price).
            ProductionItem::Project(_) => {}
        }
        Ok(())
    }

    /// A great person creates its improvement on its own tile (Holy Site,
    /// Academy, ...) and is spent.
    pub(crate) fn create_great_improvement(
        &mut self,
        unit: UnitHandle,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        let (tile, improvement) =
            crate::derived::religion::great_improvement_status(self.world, rules, unit)
                .map_err(rule)?;
        self.world.map.set_improvement(tile, Some(improvement));
        self.journal
            .tile_changes
            .push(TileChange::ImprovementBuilt { tile });
        self.expend_unit(unit, rules).map(|_| ())
    }

    /// Faith of the turn into the pool, then the Great Prophet roll
    /// (rules.md).
    pub(crate) fn collect_faith(
        &mut self,
        player: PlayerId,
        cities_total: u32,
        rules: &CompiledRuleset,
    ) {
        let faith = crate::derived::income::player_faith(self.world, rules, player, cities_total);
        if let Some(state) = self.world.module.religion.player_mut(player) {
            state.faith = state.faith.saturating_add(faith);
        }
        self.roll_prophet(player, rules);
    }

    /// The prophet roll: before the Industrial era, with faith at
    /// least the next prophet's price and a religion founded or still
    /// foundable, a roll of 5 + (faith − price) out of 100 spawns a prophet
    /// in the holy city (or the capital); all faith is spent.
    fn roll_prophet(&mut self, player: PlayerId, rules: &CompiledRuleset) {
        if crate::world::player::player_era(self.world, rules, player) >= INDUSTRIAL_ERA {
            return;
        }
        let state = self.world.module.religion.player(player);
        if state.religion.is_none() && crate::derived::religion::religions_left(self.world) == 0 {
            return;
        }
        let Some(prophet) = rules
            .module.great_people
            .iter()
            .find(|g| g.points_source == crate::rules::content::PointsSource::Faith)
        else {
            return;
        };
        let cost = prophet_cost(self.world, rules, player);
        let faith = state.faith;
        if faith < cost {
            return;
        }
        let chance = u64::from(PROPHET_BASE_CHANCE) + u64::from(faith - cost);
        if self.world.module.religion.roll_prophet(100) >= chance {
            return;
        }
        let Some(city) = self.prophet_city(player, rules) else {
            return;
        };
        let Some(tile) = self.world.cities.get(city).map(|c| c.tile) else {
            return;
        };
        if let Some(state) = self.world.module.religion.player_mut(player) {
            state.faith = 0;
            state.prophets = state.prophets.saturating_add(1);
        }
        let unit = self.spawn_at_city(player, tile, prophet.unit, rules);
        self.register_religious_unit(unit, prophet.unit, None, 0, rules);
    }

    /// The player's holy city if it still owns it, else its capital, else
    /// its first city.
    fn prophet_city(&self, player: PlayerId, rules: &CompiledRuleset) -> Option<CityHandle> {
        if let Some(religion) = self.world.module.religion.player(player).religion {
            if let Some(founded) = self.world.module.religion.religion(religion) {
                let owner = self.world.cities.get(founded.holy_city).map(|c| c.owner);
                if owner == Some(player) {
                    return Some(founded.holy_city);
                }
            }
        }
        crate::world::city::capital_of(self.world, rules, player)
            .or_else(|| self.world.indexes.cities_of(player).first().copied())
    }

    /// Religious pressure of one round (rules.md, per-turn spread →
    /// spread): holy cities press their religion,
    /// every city with a religious majority presses the cities within reach
    /// and the cities a trade route links it with, at any distance (rules
    /// "Trade routes" 3.9; the origin of a route presses its destination
    /// harder by its owner's `ReligiousPressurePercent`, Arabia).
    pub(crate) fn religious_pressure_round(&mut self, rules: &CompiledRuleset) {
        if self.world.module.religion.religions().is_empty() {
            return;
        }
        let links: Vec<(CityHandle, CityHandle)> = self
            .world
            .module
            .trade
            .routes()
            .iter()
            .map(|r| (r.origin, r.destination))
            .collect();
        let sources = self.pressure_sources(rules, &links);
        let spies = self.spy_pressure(rules);
        let cities: Vec<(CityHandle, crate::ids::TileId)> =
            self.world.cities.iter().map(|(h, c)| (h, c.tile)).collect();
        let dims = self.world.map.dimensions;
        for (target, tile) in cities {
            let mut gains: Vec<(ReligionId, u32)> = Vec::new();
            if let Some(founded) = self.world.module.religion.holy_city_of(target) {
                gains.push((
                    founded.religion,
                    ADJACENT_CITY_PRESSURE * HOLY_CITY_PRESSURE,
                ));
            }
            for source in &sources {
                if source.city == target {
                    continue;
                }
                let within = dims
                    .distance(source.tile, tile)
                    .is_some_and(|d| d <= source.reach);
                let sends = links.contains(&(source.city, target));
                let receives = links.contains(&(target, source.city));
                if !within && !sends && !receives {
                    continue;
                }
                let pressure = self.pressure_on(source, target, rules);
                let pressure = if sends { pressure * source.route_percent / 100 } else { pressure };
                gains.push((source.religion, pressure));
            }
            for spy in spies.iter().filter(|s| s.city == target) {
                gains.push((spy.religion, spy.pressure));
            }
            for (religion, amount) in gains {
                self.add_pressure(target, Creed::Religion(religion), amount, rules);
            }
        }
    }

    /// The pressure founders' spies exert where they are (rules.md
    /// step 3, Underground Sect): the founder's religion gets the adjacent
    /// city pressure × `Espionage(SpyPressure)` in every city a living spy
    /// of the founder is assigned to.
    fn spy_pressure(&self, rules: &CompiledRuleset) -> Vec<SpyPressure> {
        let mut out = Vec::new();
        for player in crate::systems::city_states::majors(self.world) {
            let Some(religion) = self.world.module.religion.player(player).religion else {
                continue;
            };
            let spies = self.world.module.espionage.spies_of(player);
            if spies.iter().all(|s| s.city.is_none()) {
                continue;
            }
            let times = crate::systems::espionage::player_stat(
                self.world,
                rules,
                player,
                crate::rules::stats::EspionageStat::SpyPressure,
            );
            if times <= 0 {
                continue;
            }
            // A dead spy has no city.
            for city in spies.iter().filter_map(|s| s.city) {
                out.push(SpyPressure {
                    city,
                    religion,
                    pressure: ADJACENT_CITY_PRESSURE * times as u32,
                });
            }
        }
        out
    }

    /// The pressure `source` puts on `target`: more on a city-state that is
    /// a friend or an ally of the religion's founder
    /// (`FriendlyCityStatePressure`, Religious Unity).
    fn pressure_on(&self, source: &PressureSource, target: CityHandle, rules: &CompiledRuleset) -> u32 {
        if source.friendly_city_state_percent == 0 {
            return source.pressure;
        }
        let Some(owner) = self.world.cities.get(target).map(|c| c.owner) else {
            return source.pressure;
        };
        let friendly = self.world.players.is_city_state(owner)
            && crate::systems::city_states::is_friends(self.world, rules, owner, source.founder);
        if !friendly {
            return source.pressure;
        }
        let factor = i64::from(100 + source.friendly_city_state_percent.max(-100));
        (i64::from(source.pressure) * factor / 100) as u32
    }

    /// Every city with a religious majority: the pressure it exerts and how
    /// far (beliefs of its religion change both), and the percent of it
    /// along its own trade routes.
    fn pressure_sources(
        &self,
        rules: &CompiledRuleset,
        links: &[(CityHandle, CityHandle)],
    ) -> Vec<PressureSource> {
        let mut out = Vec::new();
        for (handle, core) in self.world.cities.iter() {
            let faith = self.world.module.religion.city(handle);
            let Some(Creed::Religion(religion)) =
                crate::derived::religion::majority(faith, core.population)
            else {
                continue;
            };
            let mods = crate::classic::religion_effects::religion_modifiers(self.world, rules, religion);
            let distance = mods.of(ReligionStat::SpreadDistance).max(0);
            let reach = ADJACENT_CITY_DISTANCE * (100 + distance) / 100;
            let percent =
                (mods.of(ReligionStat::PressureOut) + city_pressure_percent(rules, core)).max(-100);
            let pressure =
                (i64::from(ADJACENT_CITY_PRESSURE) * (100 + i64::from(percent)) / 100) as u32;
            let Some(founder) = self.world.module.religion.religion(religion).map(|r| r.founder) else {
                continue;
            };
            let route_percent = if links.iter().any(|(origin, _)| *origin == handle) {
                route_pressure_percent(self.world, rules, core.owner)
            } else {
                100
            };
            out.push(PressureSource {
                city: handle,
                tile: core.tile,
                religion,
                founder,
                reach,
                pressure,
                friendly_city_state_percent: mods.of(ReligionStat::FriendlyCityStatePressure),
                route_percent,
            });
        }
        out
    }

    /// Add pressure of a creed to a city; a religion's pressure lowers the
    /// pantheon's by the same amount (rules.md). Journals the city when
    /// its followers change.
    pub(crate) fn add_pressure(
        &mut self,
        city: CityHandle,
        creed: Creed,
        amount: u32,
        rules: &CompiledRuleset,
    ) {
        if amount == 0 {
            return;
        }
        self.change_city_faith(city, rules, |faith| {
            faith.add(creed, amount);
            if let Creed::Religion(_) = creed {
                faith.reduce(Creed::Pantheon, amount);
            }
        });
    }

    /// Change a city's pressure after bringing it up to date; journal the
    /// city when its followers or its majority (whose beliefs apply)
    /// differ from what its yields were built on. A city whose majority
    /// becomes a religion may pay its first-conversion bonus.
    fn change_city_faith(
        &mut self,
        city: CityHandle,
        rules: &CompiledRuleset,
        change: impl FnOnce(&mut crate::world::religion::CityFaith),
    ) {
        let population = self.population(city);
        let before = self.faith_facts(city, population);
        self.touch_city(city);
        change(self.world.module.religion.city_mut(city));
        let after = self.faith_facts(city, population);
        if before == after {
            return;
        }
        self.journal
            .module_changes
            .push(ClassicChange::ReligionChanged { city });
        if before.1 == after.1 {
            return;
        }
        if let Some(Creed::Religion(religion)) = after.1 {
            self.pay_adoption_bonus(city, religion, rules);
        }
    }

    /// The first time a city converts to a religion with OnCityConverted
    /// beliefs, they fire for its founder (Initiation Rites); the city never
    /// pays again.
    fn pay_adoption_bonus(&mut self, city: CityHandle, religion: ReligionId, rules: &CompiledRuleset) {
        if self.world.module.religion.city(city).adoption_bonus == AdoptionBonus::Paid {
            return;
        }
        let sources = &rules.effect_index.sources(Trigger::Module(ClassicTrigger::OnCityConverted)).module;
        let Some(founded) = self.world.module.religion.religion(religion) else {
            return;
        };
        if !founded.beliefs.iter().any(|b| sources.contains(&ClassicSource::Belief(*b))) {
            return;
        }
        self.world.module.religion.city_mut(city).adoption_bonus = AdoptionBonus::Paid;
        self.fire(rules, TriggerEvent::CityConverted { city, religion });
    }

    /// What a city's yields read of its faith: the followers of each creed
    /// and the majority.
    fn faith_facts(&self, city: CityHandle, population: u32) -> (Vec<(Creed, u32)>, Option<Creed>) {
        let faith = self.world.module.religion.city(city);
        let followers = crate::derived::religion::followers(faith, population);
        (followers, crate::derived::religion::majority(faith, population))
    }

    /// Bring a city's pressure up to date before it changes: a city never
    /// touched gets its atheists (1000 per citizen); growth since the last
    /// touch adds pressure to the majority.
    fn touch_city(&mut self, city: CityHandle) {
        let population = self.population(city);
        let majority = {
            let faith = self.world.module.religion.city(city);
            crate::derived::religion::majority(faith, faith.population.max(1))
        };
        let faith = self.world.module.religion.city_mut(city);
        if faith.pressure.is_empty() {
            faith.add(Creed::Atheism, population * PRESSURE_PER_POP);
            faith.population = population;
            return;
        }
        if population > faith.population {
            let creed = majority.unwrap_or(Creed::Atheism);
            faith.add(creed, (population - faith.population) * PRESSURE_PER_POP);
        }
        faith.population = population;
    }

    fn population(&self, city: CityHandle) -> u32 {
        self.world
            .cities
            .get(city)
            .map(|c| c.population)
            .unwrap_or(0)
    }

    /// Every city following the religion: its yields may change with new
    /// beliefs.
    fn journal_religion_cities(&mut self, religion: ReligionId) {
        let creed = Creed::Religion(religion);
        let following: Vec<CityHandle> = self
            .world
            .cities
            .iter()
            .filter(|(h, c)| {
                crate::derived::religion::majority(self.world.module.religion.city(*h), c.population)
                    == Some(creed)
            })
            .map(|(h, _)| h)
            .collect();
        for city in following {
            self.journal
                .module_changes
                .push(ClassicChange::ReligionChanged { city });
        }
    }

    /// Start of `player`'s turn: each religious unit with
    /// `RivalLandStrengthLossPercent` that ended the last turn in land its
    /// owner may not enter in peace loses that percent of its original
    /// strength; at nothing left it is lost.
    pub(crate) fn religious_strength_loss(&mut self, player: PlayerId, rules: &CompiledRuleset) {
        let stat = crate::rules::effects::UnitStat::Module(crate::classic::effects::ClassicUnitStat::RivalLandStrengthLossPercent);
        let loses = |e: &crate::rules::effects::CompiledEffect| {
            matches!(e.modifier, Modifier::ModifyUnitStat { stat: s, .. } if s == stat)
        };
        if !crate::effects::unit::any_unit_effect(rules, loses) {
            return;
        }
        for handle in self.world.indexes().units_of(player).to_vec() {
            let Some(core) = self.world.units.get(handle).map(|v| *v.core) else {
                continue;
            };
            let percent = crate::effects::unit::unit_modifiers(self.world, rules, &core).stat(stat);
            if percent <= 0 {
                continue;
            }
            let host = self.world.map.owner(core.tile).flatten();
            if host.is_none_or(|h| h == player) || crate::world::territory::may_enter(self.world, player, core.tile) {
                continue;
            }
            let Some(religion) = crate::derived::religion::unit_religion(self.world, handle) else {
                continue;
            };
            let spreads_left = crate::derived::religion::spreads_left(self.world, rules, handle);
            let lost = self
                .world
                .module
                .religion
                .unit(core.id)
                .map_or(0, |u| u.strength_lost)
                .saturating_add(percent.min(100) as u8)
                .min(100);
            self.world.module.religion.add_unit(ReligiousUnit {
                unit: core.id,
                religion,
                spreads_left,
                strength_lost: lost,
            });
            if lost >= 100 {
                let _ = self.spend_religious_unit(handle, rules);
            }
        }
    }

    /// Remember the religion and the spreads of a new religious unit
    /// (missionaries carry their city's majority religion, prophets their
    /// owner's religion).
    fn register_religious_unit(
        &mut self,
        unit: UnitHandle,
        kind: crate::ids::UnitKindId,
        majority: Option<Creed>,
        extra_spreads: u8,
        rules: &CompiledRuleset,
    ) {
        let Some(def) = rules.units.get(kind.0 as usize) else {
            return;
        };
        let carries = def.module.abilities.iter().any(|a| {
            matches!(
                a,
                UnitAbility::SpreadReligion
                    | UnitAbility::RemoveHeresy
                    | UnitAbility::FoundReligion
            )
        });
        if !carries {
            return;
        }
        let Some(core) = self.world.units.get(unit).map(|v| *v.core) else {
            return;
        };
        let religion = match majority {
            Some(Creed::Religion(r)) => Some(r),
            _ => self.world.module.religion.player(core.owner).religion,
        };
        let Some(religion) = religion else {
            return;
        };
        let spreads = def
            .module.abilities
            .iter()
            .find_map(|a| match a {
                UnitAbility::ReligionSpreads(n) => Some(*n),
                _ => None,
            })
            .unwrap_or(1);
        self.world.module.religion.add_unit(ReligiousUnit {
            unit: core.id,
            religion,
            spreads_left: spreads.saturating_add(extra_spreads),
            strength_lost: 0,
        });
    }

    /// Faith purchases of great people count per kind; a bought prophet
    /// counts as a spawned one (rules.md).
    fn count_faith_purchase(
        &mut self,
        player: PlayerId,
        kind: crate::ids::UnitKindId,
        rules: &CompiledRuleset,
    ) {
        let Some(gp) = crate::derived::religion::great_person_kind(rules, kind) else {
            return;
        };
        let Some(state) = self.world.module.religion.player_mut(player) else {
            return;
        };
        if gp == GreatPersonKind::Prophet {
            state.prophets = state.prophets.saturating_add(1);
            return;
        }
        match state.bought.iter_mut().find(|(k, _)| *k == gp) {
            Some((_, n)) => *n = n.saturating_add(1),
            None => {
                state.bought.push((gp, 1));
                state.bought.sort_by_key(|(k, _)| k.index());
            }
        }
    }

    /// The unit is used up: it leaves the map and the religious unit list.
    fn spend_religious_unit(&mut self, unit: UnitHandle, rules: &CompiledRuleset) -> Result<(), CommandError> {
        self.expend_unit(unit, rules).map(|_| ())
    }

    fn unit_id(&self, unit: UnitHandle) -> Result<crate::ids::EntityId, CommandError> {
        self.world
            .units
            .get(unit)
            .map(|v| v.core.id)
            .ok_or(CommandError::UnknownUnit)
    }
}

/// A city pressing its majority religion on its neighbours this round.
/// Pressure a founder's spy exerts in the city it is in.
struct SpyPressure {
    city: CityHandle,
    religion: ReligionId,
    pressure: u32,
}

struct PressureSource {
    city: CityHandle,
    tile: crate::ids::TileId,
    religion: ReligionId,
    founder: PlayerId,
    reach: i32,
    pressure: u32,
    /// `FriendlyCityStatePressure` of the religion.
    friendly_city_state_percent: i32,
    /// Percent of `pressure` toward the destinations of the city's own
    /// trade routes (100 + the owner's `ReligiousPressurePercent`).
    route_percent: u32,
}

/// 100 + the owner's trade route pressure percent (Arabia: 200).
fn route_pressure_percent(
    auth: &crate::world::state::AuthWorld,
    rules: &CompiledRuleset,
    owner: PlayerId,
) -> u32 {
    use crate::rules::stats::{Stat, TradeStat};
    let stats = &crate::effects::player::player_modifiers(auth, rules, owner).stats;
    (100 + stats.get(Stat::Module(crate::rules::stats::ClassicStat::Trade(TradeStat::ReligiousPressurePercent)))).max(0) as u32
}

/// `percent` of `amount` (never negative).
fn percent_of(amount: u32, percent: i32) -> u32 {
    (i64::from(amount) * i64::from(percent.max(0)) / 100) as u32
}

/// Remove every religion but `keep` from a city; a religion with a
/// `PressureRetention` percent keeps that share of its pressure (Unity of
/// the Prophets).
fn remove_other_religions(
    faith: &mut crate::world::religion::CityFaith,
    keep: ReligionId,
    retention: &[(ReligionId, i32)],
) {
    for p in faith.pressure.iter_mut() {
        let Creed::Religion(religion) = p.creed else {
            continue;
        };
        if religion == keep {
            continue;
        }
        let kept = retention.iter().find(|(r, _)| *r == religion).map(|(_, n)| *n).unwrap_or(0);
        p.amount = percent_of(p.amount, kept.min(100));
    }
    faith.pressure.retain(|p| p.amount > 0);
}

/// Take `erosion` pressure from the religions other than `spreading`, in
/// proportion to their pressure (Evangelism).
fn erode_other_religions(
    faith: &mut crate::world::religion::CityFaith,
    spreading: ReligionId,
    erosion: u32,
) {
    if erosion == 0 {
        return;
    }
    let other = |p: &crate::world::religion::Pressure| {
        matches!(p.creed, Creed::Religion(r) if r != spreading)
    };
    let total: u64 = faith.pressure.iter().filter(|p| other(p)).map(|p| u64::from(p.amount)).sum();
    if total == 0 {
        return;
    }
    for p in faith.pressure.iter_mut() {
        if other(p) {
            let cut = u64::from(erosion) * u64::from(p.amount) / total;
            p.amount = p.amount.saturating_sub(cut as u32);
        }
    }
    faith.pressure.retain(|p| p.amount > 0);
}

fn rule(block: crate::derived::religion::FaithBlock) -> CommandError {
    CommandError::RuleViolation(block.reason().into())
}

/// Extra spreads of a missionary bought in `city` (Great Mosque of Djenne).
fn missionary_extra_spreads(
    world: &crate::world::state::AuthWorld,
    rules: &CompiledRuleset,
    city: CityHandle,
    kind: crate::ids::UnitKindId,
) -> u8 {
    let abilities = &rules.units[kind.0 as usize].module.abilities;
    let missionary = abilities.contains(&UnitAbility::SpreadReligion)
        && !abilities.contains(&UnitAbility::FoundReligion);
    let Some(core) = world.cities.get(city).filter(|_| missionary) else {
        return 0;
    };
    core.buildings
        .iter()
        .flat_map(|b| rules.buildings[b.0 as usize].module.abilities.iter())
        .map(|a| match a {
            crate::rules::content::BuildingAbility::MissionaryExtraSpreads(n) => *n,
            _ => 0,
        })
        .fold(0u8, u8::saturating_add)
}

/// Percent its buildings add to the pressure a city exerts (Grand Temple).
fn city_pressure_percent(rules: &CompiledRuleset, core: &crate::world::city::CityCore) -> i32 {
    core.buildings
        .iter()
        .flat_map(|b| rules.buildings[b.0 as usize].module.abilities.iter())
        .map(|a| match a {
            crate::rules::content::BuildingAbility::ReligiousPressure(n) => i32::from(*n),
            _ => 0,
        })
        .sum()
}
