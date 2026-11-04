//! Faith and religion (authoritative: persisted and hashed), classic/docs/classic/rules.md. The owner is `systems::religion`; everyone else reads.
//!
//! ```text
//! players      faith pool, pantheon belief, founded religion, prophets spawned,
//!              great people bought with faith (per kind)
//! religions    founded religions in founding order: founder, holy city,
//!              beliefs (pantheon, founder, follower, enhancer, reformation)
//! cities       religious pressure per creed (atheism, pantheon, religions) and
//!              the population the pressure was last reconciled with
//! units        religious units: the religion they carry, spreads left
//! pantheons    pantheons founded in the game (price of the next one)
//! prophet_rng  the stream of the faith Great Prophet roll (§24: own stream)
//! ```
//!
//! Followers and the majority are not stored: `derived::religion` computes
//! them from the pressure and the city's population.

use crate::ids::{BeliefId, CityHandle, EntityId, PlayerId};
use crate::rng::StreamRng;
use crate::rules::content::ReligionId;
use crate::classic::effects::GreatPersonKind;

/// What a citizen may follow: nothing (atheism), a pantheon, or a
/// founded religion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Creed {
    Atheism,
    /// The pantheon of the city's owner.
    Pantheon,
    Religion(ReligionId),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlayerFaith {
    pub faith: u32,
    pub pantheon: Option<BeliefId>,
    /// The religion this player founded.
    pub religion: Option<ReligionId>,
    /// Great Prophets spawned or bought.
    pub prophets: u16,
    /// Great people bought with faith, per kind, kind order.
    pub bought: Vec<(GreatPersonKind, u16)>,
}

impl PlayerFaith {
    pub fn bought_of(&self, kind: GreatPersonKind) -> u16 {
        for (k, n) in &self.bought {
            if *k == kind {
                return *n;
            }
        }
        0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundedReligion {
    pub religion: ReligionId,
    pub founder: PlayerId,
    pub holy_city: CityHandle,
    /// Every belief of the religion: the founder's pantheon belief first (if
    /// any), then in the order they were chosen.
    pub beliefs: Vec<BeliefId>,
    pub enhanced: bool,
}

/// Pressure of one creed in a city (1000 per citizen).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pressure {
    pub creed: Creed,
    pub amount: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CityFaith {
    /// Creeds with pressure, sorted by creed (one entry per creed).
    pub pressure: Vec<Pressure>,
    /// The population the pressure was last reconciled with (growth adds
    /// pressure to the majority).
    pub population: u32,
    /// The first-conversion bonus of the city (Initiation Rites).
    pub adoption_bonus: AdoptionBonus,
}

/// A city pays the first-conversion bonus of a religion once in the game:
/// the first time it converts to a religion whose beliefs have an
/// `OnCityConverted` effect.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AdoptionBonus {
    #[default]
    Unpaid,
    Paid,
}

impl CityFaith {
    pub fn pressure_of(&self, creed: Creed) -> u32 {
        self.pressure
            .iter()
            .find(|p| p.creed == creed)
            .map(|p| p.amount)
            .unwrap_or(0)
    }

    pub(crate) fn add(&mut self, creed: Creed, amount: u32) {
        if amount == 0 {
            return;
        }
        match self.pressure.iter_mut().find(|p| p.creed == creed) {
            Some(p) => p.amount = p.amount.saturating_add(amount),
            None => {
                self.pressure.push(Pressure { creed, amount });
                self.pressure.sort_by_key(|p| p.creed);
            }
        }
    }

    /// Lower the pressure of `creed` (never below 0); empty entries go.
    pub(crate) fn reduce(&mut self, creed: Creed, amount: u32) {
        if let Some(p) = self.pressure.iter_mut().find(|p| p.creed == creed) {
            p.amount = p.amount.saturating_sub(amount);
        }
        self.pressure.retain(|p| p.amount > 0);
    }
}

/// A missionary, inquisitor or prophet: the religion it carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReligiousUnit {
    pub unit: EntityId,
    pub religion: ReligionId,
    pub spreads_left: u8,
    /// Percent of its original religious strength the unit lost in rival
    /// land, 0..=100.
    pub strength_lost: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReligionState {
    players: Vec<PlayerFaith>,
    religions: Vec<FoundedReligion>,
    /// Index = city slot (cities are never removed); missing = no pressure.
    cities: Vec<CityFaith>,
    units: Vec<ReligiousUnit>,
    pantheons: u16,
    prophet_rng: StreamRng,
}

impl ReligionState {
    pub fn new(players: usize, seed: u64) -> Self {
        Self {
            players: vec![PlayerFaith::default(); players],
            religions: Vec::new(),
            cities: Vec::new(),
            units: Vec::new(),
            pantheons: 0,
            prophet_rng: StreamRng::new(seed ^ 0x9_0F_E7),
        }
    }

    pub fn player(&self, p: PlayerId) -> &PlayerFaith {
        static EMPTY: PlayerFaith = PlayerFaith {
            faith: 0,
            pantheon: None,
            religion: None,
            prophets: 0,
            bought: Vec::new(),
        };
        self.players.get(p.0 as usize).unwrap_or(&EMPTY)
    }

    pub fn players(&self) -> &[PlayerFaith] {
        &self.players
    }

    pub fn religions(&self) -> &[FoundedReligion] {
        &self.religions
    }

    pub fn religion(&self, id: ReligionId) -> Option<&FoundedReligion> {
        self.religions.iter().find(|r| r.religion == id)
    }

    /// The religion whose holy city `city` is.
    pub fn holy_city_of(&self, city: CityHandle) -> Option<&FoundedReligion> {
        self.religions.iter().find(|r| r.holy_city == city)
    }

    pub fn city(&self, city: CityHandle) -> &CityFaith {
        static EMPTY: CityFaith = CityFaith {
            pressure: Vec::new(),
            population: 0,
            adoption_bonus: AdoptionBonus::Unpaid,
        };
        self.cities.get(city.index as usize).unwrap_or(&EMPTY)
    }

    pub fn city_slots(&self) -> &[CityFaith] {
        &self.cities
    }

    pub fn units(&self) -> &[ReligiousUnit] {
        &self.units
    }

    pub fn unit(&self, unit: EntityId) -> Option<&ReligiousUnit> {
        self.units.iter().find(|u| u.unit == unit)
    }

    pub fn pantheons(&self) -> u16 {
        self.pantheons
    }

    pub fn prophet_rng(&self) -> StreamRng {
        self.prophet_rng
    }

    /// The belief is part of a pantheon or a religion already.
    pub fn belief_taken(&self, belief: BeliefId) -> bool {
        self.players.iter().any(|p| p.pantheon == Some(belief))
            || self.religions.iter().any(|r| r.beliefs.contains(&belief))
    }

    pub(crate) fn player_mut(&mut self, p: PlayerId) -> Option<&mut PlayerFaith> {
        self.players.get_mut(p.0 as usize)
    }

    pub(crate) fn religion_mut(&mut self, id: ReligionId) -> Option<&mut FoundedReligion> {
        self.religions.iter_mut().find(|r| r.religion == id)
    }

    pub(crate) fn add_religion(&mut self, religion: FoundedReligion) {
        self.religions.push(religion);
    }

    pub(crate) fn city_mut(&mut self, city: CityHandle) -> &mut CityFaith {
        let index = city.index as usize;
        if self.cities.len() <= index {
            self.cities.resize(index + 1, CityFaith::default());
        }
        &mut self.cities[index]
    }

    pub(crate) fn count_pantheon(&mut self) {
        self.pantheons = self.pantheons.saturating_add(1);
    }

    pub(crate) fn add_unit(&mut self, unit: ReligiousUnit) {
        self.units.retain(|u| u.unit != unit.unit);
        self.units.push(unit);
    }

    pub(crate) fn remove_unit(&mut self, unit: EntityId) {
        self.units.retain(|u| u.unit != unit);
    }


    pub(crate) fn roll_prophet(&mut self, below: u64) -> u64 {
        self.prophet_rng.below(below)
    }

    /// Restore from the save schema.
    pub(crate) fn from_parts(parts: ReligionParts) -> Self {
        Self {
            players: parts.players,
            religions: parts.religions,
            cities: parts.cities,
            units: parts.units,
            pantheons: parts.pantheons,
            prophet_rng: parts.prophet_rng,
        }
    }
}

/// Every field of the state, for the save decoder.
pub(crate) struct ReligionParts {
    pub players: Vec<PlayerFaith>,
    pub religions: Vec<FoundedReligion>,
    pub cities: Vec<CityFaith>,
    pub units: Vec<ReligiousUnit>,
    pub pantheons: u16,
    pub prophet_rng: StreamRng,
}
