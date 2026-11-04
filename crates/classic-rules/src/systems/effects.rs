//! Triggered effects: the owning system calls `fire` at the point of its
//! event, inside its transaction (explicit call, no event bus — §10).
//!
//! | trigger              | fired by                                  |
//! |----------------------|-------------------------------------------|
//! | OnBuilt              | production completion (building / unit)   |
//! | OnTurnStart          | end-turn sequence, TurnStart phase        |
//! | OnTechnologyUnlocked | research, when a technology is learned    |
//! | OnCityFounded        | city founding                             |
//! | OnUnitKilled         | combat, when a unit kills a unit          |
//! | OnUnitTrained        | production, when a city trains a unit     |
//! | OnAdopt              | policies, when the policy is adopted      |
//! | OnNaturalWonderFound(First) | natural wonders, a find settled at the end of the finder's turn |
//! | OnCityConquered      | combat, when a city is captured           |
//! | OnEraEntered         | research, when a technology starts an era |
//! | OnLongCountCycle     | end-turn sequence, TurnStart phase        |
//! | OnGreatPersonExpended| a great person's action uses it up        |
//! | OnCityConverted      | religion, a city's first conversion       |
//! | OnReligionSpread     | religion, a missionary or prophet spreads |
//!
//! Belief sources fire with the beliefs of a religion: the religion the
//! owner founded (great person expended), the converted city's new
//! religion (its founder), the spread religion (the unit's owner).
//! | OnGreatPersonBorn    | any great person unit appearing (spawn)   |
//!
//! Firing visits only the sources listed for the trigger in
//! `EffectIndex::by_trigger`, in canonical order: nation,
//! technologies (learning order), adopted policies (adoption order),
//! buildings (city order), unit kinds and
//! promotions (unit order). Instant modifiers change the world once,
//! `Turns(n)` effects become entries of the timed-effect store.

use crate::classic::ClassicChange;
use crate::classic::effects::{ClassicCondition, ClassicModifier, ClassicSelector, ClassicSource, ClassicTrigger, ClassicYield};
use crate::effects::{effect_holds, ConditionContext};
use crate::ids::{BuildingKindId, CityHandle, PlayerId, TechId, UnitHandle, UnitKindId, TileId};
use crate::rules::effects::{CompiledEffect, Condition, Duration, Grant, Modifier, Scope, Selector, SourceRef, Stacking, Trigger, YieldKind};
use crate::classic::effects::GreatPersonKind;
use crate::rules::content::ReligionId;
use crate::rules::CompiledRuleset;
use crate::transaction::journal::{CityChange, PlayerChange, UnitChange};
use crate::transaction::transaction::WorldTransaction;
use crate::world::city::CityWrite;
use crate::world::effects::{ActiveEffect, EffectRef, EffectTarget};
use crate::world::player::ClassicPlayers;

/// An event that may fire triggered effects.
#[derive(Debug, Clone, Copy)]
pub(crate) enum TriggerEvent {
    BuildingBuilt {
        city: CityHandle,
        building: BuildingKindId,
    },
    /// OnBuilt of the unit kind itself.
    UnitTrained {
        city: CityHandle,
        unit: UnitHandle,
        kind: UnitKindId,
    },
    /// OnUnitTrained of the owner's sources (buildings: this city only).
    UnitTrainedHere {
        city: CityHandle,
        unit: UnitHandle,
    },
    TurnStart {
        player: PlayerId,
    },
    TechnologyLearned {
        player: PlayerId,
        tech: TechId,
    },
    CityFounded {
        player: PlayerId,
        city: CityHandle,
    },
    UnitKilled {
        player: PlayerId,
        killer: UnitHandle,
        victim: Victim,
    },
    /// OnAdopt of the adopted policy itself.
    PolicyAdopted {
        player: PlayerId,
        policy: crate::ids::PolicyId,
    },
    /// The player found a natural wonder (`first`: no major nation
    /// found it before).
    NaturalWonderFound { player: PlayerId, tile: TileId, first: bool },
    /// `player` captured `city` from `from`.
    CityConquered {
        player: PlayerId,
        city: CityHandle,
        from: PlayerId,
    },
    EraEntered {
        player: PlayerId,
    },
    LongCountCycle {
        player: PlayerId,
    },
    /// A great person of `player` was used up by its action.
    GreatPersonExpended {
        player: PlayerId,
    },
    /// `city` first converted to `religion` (its founder's beliefs fire).
    CityConverted {
        city: CityHandle,
        religion: ReligionId,
    },
    /// A unit of `player` spreads `religion` into `city` (the religion's
    /// beliefs fire for `player`).
    ReligionSpread {
        player: PlayerId,
        city: CityHandle,
        religion: ReligionId,
    },
    /// A great person unit of the player appeared (the event unit).
    GreatPersonBorn {
        player: PlayerId,
        unit: UnitHandle,
    },
}

/// The unit an OnUnitKilled event killed, read before it left the world.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Victim {
    pub owner: PlayerId,
    pub kind: UnitKindId,
    pub tile: TileId,
}

impl Victim {
    /// The unit as a victim (None: no such unit).
    pub(crate) fn of(world: &crate::world::state::AuthWorld, unit: UnitHandle) -> Option<Self> {
        let core = world.units.get(unit)?.core;
        Some(Self {
            owner: core.owner,
            kind: core.kind,
            tile: core.tile,
        })
    }

    /// The strength a kill is valued by: the greater of its melee and
    /// ranged strength.
    pub(crate) fn strength(&self, rules: &CompiledRuleset) -> u32 {
        match rules.units.get(self.kind.0 as usize) {
            Some(def) => u32::from(def.strength.max(def.ranged_strength)),
            None => 0,
        }
    }
}

/// The firing context shared by all effects of one event.
#[derive(Debug, Clone, Copy)]
struct Firing {
    owner: PlayerId,
    trigger: Trigger,
    event_city: Option<CityHandle>,
    event_unit: Option<UnitHandle>,
    victim: Option<Victim>,
    /// OnNaturalWonderFound(First): the wonder's tile.
    event_tile: Option<TileId>,
    /// OnTechnologyUnlocked: the learned technology.
    event_tech: Option<TechId>,
    /// OnCityConquered: the city's owner before the capture.
    previous_owner: Option<PlayerId>,
    /// The religion of a religion event (conversion, spread).
    event_religion: Option<ReligionId>,
}

/// One owned instance of a source (a building in a given city, a unit).
#[derive(Debug, Clone, Copy)]
struct Instance {
    source: SourceRef,
    city: Option<CityHandle>,
    unit: Option<UnitHandle>,
}

impl Instance {
    fn of(source: SourceRef) -> Self {
        Self {
            source,
            city: None,
            unit: None,
        }
    }
}

impl<'a> WorldTransaction<'a> {
    /// Apply every triggered effect of `event` (no-op for a trigger without
    /// sources — the common case costs one index lookup).
    pub(crate) fn fire(&mut self, rules: &CompiledRuleset, event: TriggerEvent) {
        let Some(firing) = self.firing(event) else {
            return;
        };
        if rules.effect_index.sources(firing.trigger).is_empty() {
            return;
        }
        for instance in self.instances(rules, event, firing) {
            self.apply_instance(rules, firing, instance);
        }
    }

    fn firing(&self, event: TriggerEvent) -> Option<Firing> {
        let city_owner = |city: CityHandle| self.world.cities.get(city).map(|c| c.owner);
        let victim = match event {
            TriggerEvent::UnitKilled { victim, .. } => Some(victim),
            _ => None,
        };
        let (owner, trigger, event_city, event_unit) = match event {
            TriggerEvent::BuildingBuilt { city, .. } => {
                (city_owner(city)?, Trigger::OnBuilt, Some(city), None)
            }
            TriggerEvent::UnitTrained { city, unit, .. } => {
                (city_owner(city)?, Trigger::OnBuilt, Some(city), Some(unit))
            }
            TriggerEvent::UnitTrainedHere { city, unit } => (
                city_owner(city)?,
                Trigger::OnUnitTrained,
                Some(city),
                Some(unit),
            ),
            TriggerEvent::TurnStart { player } => (player, Trigger::OnTurnStart, None, None),
            TriggerEvent::TechnologyLearned { player, .. } => {
                (player, Trigger::OnTechnologyUnlocked, None, None)
            }
            TriggerEvent::CityFounded { player, city } => {
                (player, Trigger::OnCityFounded, Some(city), None)
            }
            TriggerEvent::UnitKilled { player, killer, .. } => {
                (player, Trigger::OnUnitKilled, None, Some(killer))
            }
            TriggerEvent::PolicyAdopted { player, .. } => (player, Trigger::Module(ClassicTrigger::OnAdopt), None, None),
            TriggerEvent::NaturalWonderFound { player, first: true, .. } => {
                (player, Trigger::Module(ClassicTrigger::OnNaturalWonderFoundFirst), None, None)
            }
            TriggerEvent::NaturalWonderFound { player, first: false, .. } => {
                (player, Trigger::Module(ClassicTrigger::OnNaturalWonderFound), None, None)
            }
            TriggerEvent::CityConquered { player, city, .. } => {
                (player, Trigger::OnCityConquered, Some(city), None)
            }
            TriggerEvent::EraEntered { player } => (player, Trigger::OnEraEntered, None, None),
            TriggerEvent::LongCountCycle { player } => {
                (player, Trigger::Module(ClassicTrigger::OnLongCountCycle), None, None)
            }
            TriggerEvent::GreatPersonExpended { player } => {
                (player, Trigger::Module(ClassicTrigger::OnGreatPersonExpended), None, None)
            }
            TriggerEvent::CityConverted { city, religion } => {
                let founder = self.world.module.religion.religion(religion)?.founder;
                (founder, Trigger::Module(ClassicTrigger::OnCityConverted), Some(city), None)
            }
            TriggerEvent::ReligionSpread { player, city, .. } => {
                (player, Trigger::Module(ClassicTrigger::OnReligionSpread), Some(city), None)
            }
            TriggerEvent::GreatPersonBorn { player, unit } => {
                (player, Trigger::Module(ClassicTrigger::OnGreatPersonBorn), None, Some(unit))
            }
        };
        let event_religion = match event {
            TriggerEvent::CityConverted { religion, .. }
            | TriggerEvent::ReligionSpread { religion, .. } => Some(religion),
            _ => None,
        };
        let event_tile = match event {
            TriggerEvent::NaturalWonderFound { tile, .. } => Some(tile),
            _ => None,
        };
        let event_tech = match event {
            TriggerEvent::TechnologyLearned { tech, .. } => Some(tech),
            _ => None,
        };
        let previous_owner = match event {
            TriggerEvent::CityConquered { from, .. } => Some(from),
            _ => None,
        };
        Some(Firing {
            owner,
            trigger,
            event_city,
            event_unit,
            victim,
            event_tile,
            event_tech,
            previous_owner,
            event_religion,
        })
    }

    /// Owned source instances with effects of the trigger, canonical order.
    fn instances(&self, rules: &CompiledRuleset, event: TriggerEvent, f: Firing) -> Vec<Instance> {
        match event {
            TriggerEvent::BuildingBuilt { city, building } => {
                let mut instance = Instance::of(SourceRef::Building(building));
                instance.city = Some(city);
                vec![instance]
            }
            TriggerEvent::UnitTrained { city, unit, kind } => {
                let mut instance = Instance::of(SourceRef::Unit(kind));
                instance.city = Some(city);
                instance.unit = Some(unit);
                vec![instance]
            }
            TriggerEvent::UnitTrainedHere { city, .. } => {
                self.trained_here_instances(rules, f, city)
            }
            TriggerEvent::TechnologyLearned { tech, .. } => {
                self.owned_instances(rules, f, Some(tech), None)
            }
            TriggerEvent::UnitKilled { killer, victim, .. } => {
                let mut out = self.owned_instances(rules, f, None, Some(killer));
                self.kill_belief_instances(rules, f.owner, victim.tile, &mut out);
                out
            }
            TriggerEvent::GreatPersonBorn { unit, .. } => {
                self.owned_instances(rules, f, None, Some(unit))
            }
            TriggerEvent::TurnStart { .. }
            | TriggerEvent::CityFounded { .. }
            | TriggerEvent::NaturalWonderFound { .. }
            | TriggerEvent::CityConquered { .. }
            | TriggerEvent::EraEntered { .. }
            | TriggerEvent::LongCountCycle { .. } => self.owned_instances(rules, f, None, None),
            TriggerEvent::PolicyAdopted { policy, .. } => {
                vec![Instance::of(SourceRef::Module(ClassicSource::Policy(policy)))]
            }
            TriggerEvent::GreatPersonExpended { player } => {
                let mut out = self.owned_instances(rules, f, None, None);
                if let Some(religion) = self.world.module.religion.player(player).religion {
                    self.belief_instances(rules, f, religion, &mut out);
                }
                out
            }
            TriggerEvent::CityConverted { religion, .. }
            | TriggerEvent::ReligionSpread { religion, .. } => {
                let mut out = Vec::new();
                self.belief_instances(rules, f, religion, &mut out);
                out
            }
        }
    }

    /// The beliefs of `religion` with effects of the trigger, in the order
    /// the religion took them.
    fn belief_instances(
        &self,
        rules: &CompiledRuleset,
        f: Firing,
        religion: ReligionId,
        out: &mut Vec<Instance>,
    ) {
        let sources = &rules.effect_index.sources(f.trigger).module;
        if sources.is_empty() {
            return;
        }
        let Some(founded) = self.world.module.religion.religion(religion) else {
            return;
        };
        for belief in &founded.beliefs {
            let source = ClassicSource::Belief(*belief);
            if sources.contains(&source) {
                out.push(Instance::of(SourceRef::Module(source)));
            }
        }
    }

    /// `only_tech`: a technology source fires only for this technology.
    /// `only_unit`: unit kind and promotion sources fire only for this unit.
    fn owned_instances(
        &self,
        rules: &CompiledRuleset,
        f: Firing,
        only_tech: Option<TechId>,
        only_unit: Option<UnitHandle>,
    ) -> Vec<Instance> {
        let sources = rules.effect_index.sources(f.trigger);
        let mut out = Vec::new();
        self.player_instances(f.owner, sources, only_tech, &mut out);
        let cities = match self.world.indexes.cities_by_player.get(&f.owner.0) {
            Some(cities) => cities.clone(),
            None => Vec::new(),
        };
        self.building_instances(&cities, &sources.buildings, &mut out);
        self.unit_instances(f.owner, sources, only_unit, &mut out);
        out
    }

    /// OnUnitTrained: nation, technologies, buildings of the training
    /// city (unit and promotion sources cannot use this trigger).
    fn trained_here_instances(
        &self,
        rules: &CompiledRuleset,
        f: Firing,
        city: CityHandle,
    ) -> Vec<Instance> {
        let sources = rules.effect_index.sources(f.trigger);
        let mut out = Vec::new();
        self.player_instances(f.owner, sources, None, &mut out);
        self.building_instances(&[city], &sources.buildings, &mut out);
        out
    }

    /// Nation, known technologies and adopted policies of the owner
    /// (a policy's own adoption fires `PolicyAdopted` instead).
    fn player_instances(
        &self,
        owner: PlayerId,
        sources: &crate::rules::effects::TriggerSources,
        only_tech: Option<TechId>,
        out: &mut Vec<Instance>,
    ) {
        let nation = self.world.players.nation_of(owner);
        if sources.nations.contains(&nation) {
            out.push(Instance::of(SourceRef::Nation(nation)));
        }
        for tech in self.world.players.techs_of(owner) {
            let wanted = only_tech.map(|t| t == *tech).unwrap_or(true);
            if wanted && sources.technologies.contains(tech) {
                out.push(Instance::of(SourceRef::Technology(*tech)));
            }
        }
        // Adopted policies (adoption order): Resettlement on founding, Iron
        // Curtain on conquest. OnAdopt fires for the adopted policy alone.
        if sources.module.is_empty() {
            return;
        }
        for policy in &self.world.module.culture.of(owner).policies {
            let source = ClassicSource::Policy(*policy);
            if sources.module.contains(&source) {
                out.push(Instance::of(SourceRef::Module(source)));
            }
        }
    }

    fn building_instances(
        &self,
        cities: &[CityHandle],
        buildings: &[BuildingKindId],
        out: &mut Vec<Instance>,
    ) {
        if buildings.is_empty() {
            return;
        }
        for &city in cities {
            let Some(core) = self.world.cities.get(city) else {
                continue;
            };
            for building in &core.buildings {
                if buildings.contains(building) {
                    let mut instance = Instance::of(SourceRef::Building(*building));
                    instance.city = Some(city);
                    out.push(instance);
                }
            }
        }
    }

    /// Beliefs reach a kill through the killer's city nearest to it: the
    /// beliefs of that city's majority, with
    /// the city as the source city (`Condition::EventNearSourceCity`).
    fn kill_belief_instances(
        &self,
        rules: &CompiledRuleset,
        owner: PlayerId,
        tile: TileId,
        out: &mut Vec<Instance>,
    ) {
        let sources = &rules.effect_index.sources(Trigger::OnUnitKilled).module;
        if sources.is_empty() {
            return;
        }
        let Some(city) = self.nearest_city(owner, tile) else {
            return;
        };
        let Some(core) = self.world.cities.get(city) else {
            return;
        };
        let faith = self.world.module.religion.city(city);
        let majority = crate::derived::religion::majority(faith, core.population);
        for belief in crate::classic::religion_effects::beliefs_of(self.world, majority, owner) {
            let source = ClassicSource::Belief(belief);
            if sources.contains(&source) {
                let mut instance = Instance::of(SourceRef::Module(source));
                instance.city = Some(city);
                out.push(instance);
            }
        }
    }

    /// The city of `owner` nearest to `tile` (the first in city order on a
    /// tie).
    pub(crate) fn nearest_city(&self, owner: PlayerId, tile: TileId) -> Option<CityHandle> {
        let cities = self.world.indexes.cities_by_player.get(&owner.0)?;
        let dims = self.world.map.dimensions;
        let mut best: Option<(i32, CityHandle)> = None;
        for &city in cities {
            let Some(core) = self.world.cities.get(city) else {
                continue;
            };
            let Some(distance) = dims.distance(core.tile, tile) else {
                continue;
            };
            let closer = match best {
                Some((nearest, _)) => distance < nearest,
                None => true,
            };
            if closer {
                best = Some((distance, city));
            }
        }
        best.map(|(_, city)| city)
    }

    fn unit_instances(
        &self,
        owner: PlayerId,
        sources: &crate::rules::effects::TriggerSources,
        only_unit: Option<UnitHandle>,
        out: &mut Vec<Instance>,
    ) {
        if sources.units.is_empty() && sources.promotions.is_empty() {
            return;
        }
        let units: Vec<UnitHandle> = match only_unit {
            Some(unit) => vec![unit],
            None => self.world.indexes.units_of(owner).to_vec(),
        };
        for unit in units {
            let Some(view) = self.world.units.get(unit) else {
                continue;
            };
            let core = *view.core;
            if sources.units.contains(&core.kind) {
                let mut instance = Instance::of(SourceRef::Unit(core.kind));
                instance.unit = Some(unit);
                out.push(instance);
            }
            for promotion in core.promotions.iter() {
                if sources.promotions.contains(&promotion) {
                    let mut instance = Instance::of(SourceRef::Promotion(promotion));
                    instance.unit = Some(unit);
                    out.push(instance);
                }
            }
        }
    }

    fn apply_instance(&mut self, rules: &CompiledRuleset, f: Firing, instance: Instance) {
        let Some(set) = rules.effect_set(instance.source) else {
            return;
        };
        let effects: Vec<CompiledEffect> = set.triggered(f.trigger).to_vec();
        for (slot, effect) in effects.iter().enumerate() {
            let reference = EffectRef {
                source: instance.source,
                trigger: f.trigger,
                slot: slot as u16,
            };
            // A timed unit effect is one entry for all the owner's units.
            let targets = if effect.scope == Scope::Unit && matches!(effect.duration, Duration::Turns(_)) {
                vec![Target::Units(f.owner)]
            } else {
                self.targets(rules, f, instance, effect.selector)
            };
            for target in targets {
                if self.effect_condition_holds(rules, f, instance, effect, target) {
                    self.apply_to_target(rules, f, reference, effect, target);
                }
            }
        }
    }

    /// Entities addressed by a triggered effect's selector.
    fn targets(
        &self,
        rules: &CompiledRuleset,
        f: Firing,
        instance: Instance,
        selector: Selector,
    ) -> Vec<Target> {
        let cities = |handles: Option<CityHandle>| handles.into_iter().map(Target::City).collect();
        let units = |handles: Option<UnitHandle>| handles.into_iter().map(Target::Unit).collect();
        match selector {
            Selector::Owner => vec![Target::Player(f.owner)],
            Selector::SourceCity => cities(instance.city),
            Selector::EventCity => cities(f.event_city),
            Selector::OwnerCities => {
                let list = self.world.indexes.cities_by_player.get(&f.owner.0);
                list.map(|l| l.iter().map(|c| Target::City(*c)).collect())
                    .unwrap_or_default()
            }
            Selector::SourceUnit => units(instance.unit),
            Selector::EventUnit => units(f.event_unit),
            Selector::OwnerUnits => {
                let list = self.world.indexes.units_of(f.owner);
                list.iter().map(|u| Target::Unit(*u)).collect()
            }
            Selector::Capital => {
                cities(crate::world::city::capital_of(self.world, rules, f.owner))
            }
            Selector::SourceCityTiles
            | Selector::OwnerCityTiles
            | Selector::Module(ClassicSelector::FollowingCityTiles) => {
                unreachable!("compiler rejects triggered Tile-scope effects")
            }
            // Triggered belief effects address the event's player (Owner);
            // the importer never gives them these selectors.
            Selector::Module(ClassicSelector::FollowingCities)
            | Selector::Module(ClassicSelector::HolyCity)
            | Selector::Module(ClassicSelector::Founder)
            | Selector::Module(ClassicSelector::OwnReligion) => Vec::new(),
        }
    }

    fn apply_to_target(
        &mut self,
        rules: &CompiledRuleset,
        f: Firing,
        reference: EffectRef,
        effect: &CompiledEffect,
        target: Target,
    ) {
        match effect.duration {
            Duration::Turns(turns) => self.start_timed(f.owner, reference, effect, target, turns),
            Duration::Once => {
                let effect = self.counted_once(rules, f, effect, target);
                self.apply_once(rules, f, reference.source, &effect, target)
            }
            Duration::WhileSourceExists => unreachable!("compiler: passive effects never fire"),
        }
    }

    /// The kind of the event's great person (OnGreatPersonBorn).
    fn event_great_person(&self, rules: &CompiledRuleset, f: Firing) -> Option<GreatPersonKind> {
        let unit = self.world.units.get(f.event_unit?)?;
        crate::derived::great_people::great_person_of(rules, unit.core.kind).map(|gp| gp.kind)
    }

    /// The effect's condition for this event: the event's own conditions
    /// (the victim of a kill, the distance to the source's city) here, the
    /// rest on the target.
    fn effect_condition_holds(
        &self,
        rules: &CompiledRuleset,
        f: Firing,
        instance: Instance,
        effect: &CompiledEffect,
        target: Target,
    ) -> bool {
        let condition = effect.condition;
        if let Condition::EventNearSourceCity(range) = condition {
            return self.event_near(f, instance.city, range);
        }
        if let Condition::Module(ClassicCondition::GreatPersonIs(kind)) = condition {
            return self.event_great_person(rules, f) == Some(kind);
        }
        if condition.is_opponent() {
            return match f.victim {
                Some(victim) => self.victim_matches(rules, victim, condition),
                None => false,
            };
        }
        self.target_condition_holds(rules, f, effect, target)
    }

    /// The kill happened within `range` tiles of the source's city.
    fn event_near(&self, f: Firing, city: Option<CityHandle>, range: u8) -> bool {
        let (Some(victim), Some(city)) = (f.victim, city) else {
            return false;
        };
        let Some(core) = self.world.cities.get(city) else {
            return false;
        };
        match self.world.map.dimensions.distance(core.tile, victim.tile) {
            Some(distance) => distance <= i32::from(range),
            None => false,
        }
    }

    /// An opponent condition on the victim of a kill (the rules of
    /// `crate::combat::strength`, on the victim's kind).
    fn victim_matches(&self, rules: &CompiledRuleset, victim: Victim, condition: Condition) -> bool {
        let def = rules.units.get(victim.kind.0 as usize);
        match condition {
            Condition::Module(ClassicCondition::OpponentIsBarbarian) => self.world.players.is_barbarian(victim.owner),
            Condition::OpponentRole(role) => def.is_some_and(|d| d.role == Some(role)),
            Condition::OpponentDomain(domain) => def.is_some_and(|d| d.domain == domain),
            Condition::OpponentClassOf(kind) => {
                let class = rules.units.get(kind.0 as usize).and_then(|d| d.class.as_ref());
                let victim_class = def.and_then(|d| d.class.as_ref());
                class.is_some() && class == victim_class
            }
            _ => false,
        }
    }

    fn target_condition_holds(
        &self,
        rules: &CompiledRuleset,
        f: Firing,
        effect: &CompiledEffect,
        target: Target,
    ) -> bool {
        if let Condition::LearnedTechnology(tech) = effect.condition {
            return f.event_tech == Some(tech);
        }
        let owner = f.owner;
        let city = match target {
            Target::City(handle) => self.world.cities.get(handle),
            Target::Player(_) | Target::Unit(_) | Target::Units(_) => None,
        };
        let unit = match target {
            Target::Unit(handle) => self
                .world
                .units
                .get(handle)
                .map(|u| crate::effects::UnitTraits::of(rules, u.core.kind)),
            Target::Player(_) | Target::City(_) | Target::Units(_) => None,
        };
        let at = ConditionContext {
            owner,
            city,
            tile: None,
            unit,
            producing: None,
        };
        effect_holds(self.world, rules, at, effect)
    }

    /// A one-shot per-count grant with its count read now: on the city a
    /// religion spreads into (OnReligionSpread), else on the target city
    /// or player. Other effects come back unchanged.
    fn counted_once(
        &self,
        rules: &CompiledRuleset,
        f: Firing,
        effect: &CompiledEffect,
        target: Target,
    ) -> CompiledEffect {
        use crate::effects::count::{resolve, Counted};
        let spread_city = match (f.trigger, f.event_city, f.event_religion) {
            (Trigger::Module(ClassicTrigger::OnReligionSpread), Some(city), Some(religion)) => {
                self.world.cities.get(city).map(|core| (core, religion))
            }
            _ => None,
        };
        let counted = match (spread_city, target) {
            (Some((city, religion)), _) => Counted::Event { city, subject: religion.0 },
            (None, Target::City(city)) => match self.world.cities.get(city) {
                Some(core) => Counted::City(core),
                None => return *effect,
            },
            (None, Target::Player(player)) => Counted::Player(player),
            (None, Target::Unit(_) | Target::Units(_)) => return *effect,
        };
        resolve(self.world, rules, counted, effect)
    }

    fn start_timed(
        &mut self,
        owner: PlayerId,
        reference: EffectRef,
        effect: &CompiledEffect,
        target: Target,
        turns: u16,
    ) {
        let target = match target {
            Target::Player(p) => EffectTarget::Player(p),
            Target::City(c) => EffectTarget::City(c),
            Target::Units(p) => EffectTarget::Units(p),
            Target::Unit(_) => unreachable!("compiler: a timed unit effect targets all units"),
        };
        let entry = ActiveEffect {
            owner,
            target,
            effect: reference,
            turns_left: turns,
        };
        self.world
            .effects
            .start(entry, effect.stacking == Stacking::Unique);
        self.journal
            .player_changes
            .push(PlayerChange::EffectsChanged { player: owner });
    }

    fn apply_once(
        &mut self,
        rules: &CompiledRuleset,
        f: Firing,
        source: SourceRef,
        effect: &CompiledEffect,
        target: Target,
    ) {
        let owner = f.owner;
        match (effect.modifier, target) {
            (Modifier::AddYield { yield_kind, amount }, target) => {
                let mut amount = amount.max(0) as u32;
                // Gold for a natural wonder found (Spain) is a sum like the finder gold: × the speed's gold percent.
                let wonder_find = matches!(f.trigger, Trigger::Module(ClassicTrigger::OnNaturalWonderFoundFirst | ClassicTrigger::OnNaturalWonderFound));
                if wonder_find && yield_kind == YieldKind::Gold {
                    amount = kernel::rules::speeds::SpeedValues::scale(amount, self.world.settings().speed_values().gold_percent);
                }
                self.grant_yield(owner, yield_kind, amount, target);
                self.journal_trigger_yield(f, source, yield_kind, amount);
            }
            (Modifier::YieldFromKill { yield_kind, percent }, Target::Player(player)) => {
                let strength = f.victim.map(|v| v.strength(rules)).unwrap_or(0);
                let amount = strength * percent.max(0) as u32 / 100;
                self.grant_yield(player, yield_kind, amount, target);
                self.journal_trigger_yield(f, source, yield_kind, amount);
            }
            (Modifier::FreeUnit { unit, count }, Target::City(city)) => {
                for _ in 0..count {
                    self.grant_unit(rules, city, unit);
                }
            }
            (Modifier::FreeBuilding(building), Target::City(city)) => {
                self.grant_building(city, building)
            }
            (Modifier::AddPromotion(promotion), Target::Unit(unit)) => {
                self.grant_promotion(unit, promotion)
            }
            (Modifier::GrantExperience(amount), Target::Unit(unit)) => {
                self.grant_experience(unit, amount)
            }
            (Modifier::Module(ClassicModifier::GrantExperiencePerGreatWork(amount)), Target::Unit(unit)) => {
                if let Some(city) = f.event_city {
                    let works = crate::derived::tourism::great_works_in_city(self.world, city);
                    let total = u32::from(amount).saturating_mul(works).min(u32::from(u16::MAX));
                    self.grant_experience(unit, total as u16);
                }
            }
            (Modifier::Module(ClassicModifier::FreePolicies { count }), Target::Player(player)) => {
                self.grant_free_policies(player, count)
            }
            (Modifier::FreeBuildingInCities { building, count }, Target::Player(player)) => {
                self.grant_free_buildings_in_cities(player, building, count)
            }
            (Modifier::Grant(Grant::ConquestTechnology), Target::Player(player)) => {
                if let (Some(city), Some(from)) = (f.event_city, f.previous_owner) {
                    self.grant_conquest_technology(rules, player, city, from);
                }
            }
            (Modifier::Grant(grant), Target::Player(player)) => {
                self.grant_to_player(rules, player, grant)
            }
            (Modifier::Grant(grant), Target::City(city)) => self.grant_to_city(rules, city, grant),
            (Modifier::Module(ClassicModifier::GrantSpies { count }), Target::Player(player)) => {
                self.add_spies(player, count, rules)
            }
            (Modifier::Module(ClassicModifier::PromoteSpies { ranks }), Target::Player(player)) => {
                self.promote_spies(player, ranks)
            }
            (Modifier::Module(ClassicModifier::InstantTourism { amount }), Target::Player(player)) => {
                self.tourism_burst(player, u64::from(amount))
            }
            (modifier, target) => {
                unreachable!("compiler rejects one-shot {modifier:?} on {target:?}")
            }
        }
    }

    /// A yield an OnUnitKilled effect paid goes to the journal (`ClassicChange::KillYield`), one a
    /// natural wonder find paid too (`ClassicChange::WonderYield`).
    fn journal_trigger_yield(&mut self, f: Firing, source: SourceRef, kind: YieldKind, amount: u32) {
        if amount == 0 {
            return;
        }
        match (f.trigger, f.victim, f.event_tile) {
            (Trigger::OnUnitKilled, Some(victim), _) => {
                let barbarian = self.world.players.is_barbarian(victim.owner);
                self.journal.module_changes.push(ClassicChange::KillYield { player: f.owner, tile: victim.tile, kind, amount, source, barbarian });
            }
            (Trigger::Module(ClassicTrigger::OnNaturalWonderFoundFirst | ClassicTrigger::OnNaturalWonderFound), _, Some(tile)) => {
                self.journal.module_changes.push(ClassicChange::WonderYield { player: f.owner, tile, kind, amount, source });
            }
            _ => {}
        }
    }

    /// One-shot grant into a stock (compiler: amount > 0, stock exists).
    fn grant_yield(&mut self, owner: PlayerId, kind: YieldKind, amount: u32, target: Target) {
        let city = match target {
            Target::City(city) => Some(city),
            Target::Player(_) | Target::Unit(_) | Target::Units(_) => None,
        };
        let write = match (kind, city) {
            (YieldKind::Gold, _) => {
                self.world.players.earn(owner, amount);
                CityWrite::Applied
            }
            (YieldKind::Science, _) => {
                self.world.players.earn_science(owner, amount);
                CityWrite::Applied
            }
            // Faith is a player pool (`world::religion`), whatever the target.
            (YieldKind::Module(ClassicYield::Faith), _) => {
                if let Some(faith) = self.world.module.religion.player_mut(owner) {
                    faith.faith = faith.faith.saturating_add(amount);
                }
                CityWrite::Applied
            }
            (YieldKind::Culture, None) => {
                self.gain_culture(owner, amount);
                CityWrite::Applied
            }
            (YieldKind::Food, Some(city)) => self.world.cities.add_food(city, amount),
            (YieldKind::Production, Some(city)) => self.world.cities.add_production(city, amount),
            (YieldKind::Culture, Some(city)) => self.world.cities.add_culture(city, amount),
            (other, _) => unreachable!("compiler rejects a grant of {other:?} here"),
        };
        debug_assert!(!matches!(write, CityWrite::QueueFull));
    }

    fn grant_unit(&mut self, rules: &CompiledRuleset, city: CityHandle, kind: UnitKindId) {
        let Some(core) = self.world.cities.get(city) else {
            return;
        };
        let (owner, tile) = (core.owner, core.tile);
        self.spawn_at_city(owner, tile, kind, rules);
    }

    pub(crate) fn grant_building(&mut self, city: CityHandle, building: BuildingKindId) {
        let present = match self.world.cities.get(city) {
            Some(core) => core.buildings.contains(&building),
            None => return,
        };
        if present {
            return;
        }
        if let CityWrite::Applied = self.world.cities.add_building(city, building) {
            self.journal
                .city_changes
                .push(CityChange::BuildingAdded { city, building });
        }
    }

    fn grant_promotion(&mut self, unit: UnitHandle, promotion: crate::ids::PromotionId) {
        let Some(core) = self.world.units.get_mut_core(unit) else {
            return;
        };
        if core.promotions.insert(promotion) {
            self.journal
                .unit_changes
                .push(UnitChange::Promoted { unit, promotion });
        }
    }

}

#[derive(Debug, Clone, Copy)]
enum Target {
    Player(PlayerId),
    City(CityHandle),
    Unit(UnitHandle),
    /// Every unit of the player (timed unit effects).
    Units(PlayerId),
}

#[cfg(test)]
#[path = "effects_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "effects_vocabulary_tests.rs"]
mod vocabulary_tests;
