//! A player's classic economy and society state (authoritative, saved,
//! hashed; the kernel's `PlayerCore::economy` through
//! `ModuleWorld::Player`): golden ages, great people counters, the science
//! record the great scientist reads, the happiness snapshot, natural
//! wonders found, owed free choices. Its record in the economy section of a
//! save holds the kernel's fields of the player where they always were
//! (`KernelPlayerRecord`), so saves and hashes keep their bytes.

use kernel::persistence::codec::{CodecError, Field, Reader};
use kernel::world::module::{KernelPlayerRecord, ModulePlayer};

use crate::classic::Classic;
use crate::ids::PlayerId;
use crate::world::state::AuthWorld;

/// Turns of science a player remembers; a great scientist grants the sum
/// (own value, unverified).
pub const SCIENCE_HISTORY_TURNS: usize = 8;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlayerEconomy {
    /// Turns of the running golden age (0: none).
    pub golden_age_turns: u16,
    /// Happiness accumulated toward the next golden age.
    pub golden_age_meter: u32,
    /// Golden ages started so far (raises the next threshold).
    pub golden_ages: u16,
    /// Great people born from points (shared by scientists, engineers and
    /// merchants).
    pub great_people_created: u16,
    pub great_writers_created: u16,
    pub great_artists_created: u16,
    pub great_musicians_created: u16,
    /// Science of the player's last turns, newest first.
    pub science_history: [u32; SCIENCE_HISTORY_TURNS],
    /// Excess happiness at the start of the player's last turn, cached: the
    /// unhappiness effects of the turn read this snapshot
    /// (`derived::happiness` computes the live value).
    pub excess_happiness: i32,
    /// Natural wonder sites the player has been rewarded for finding, in
    /// the order found (`systems::natural_wonders`).
    pub natural_wonders_found: Vec<crate::ids::TileId>,
    /// Free choices the player is owed (`Modifier::Grant`).
    pub owed: OwedChoices,
}

/// Technologies and great people granted for the player to choose, oldest
/// first: a choice command answers the head (`ChooseFreeTechnology`,
/// `ChooseFreeGreatPerson`), so one grant is one decision.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OwedChoices {
    pub technologies: Vec<FreeTechnology>,
    pub great_people: Vec<FreeGreatPerson>,
    /// Great person kinds the Maya Long Count gave, bit =
    /// `GreatPersonKind::index()` (all given: the set starts over).
    pub long_count_given: u16,
    /// Cities that already gave the player a conquest technology
    /// (Assyria: once per city), ascending.
    pub conquest_tech_cities: Vec<crate::ids::EntityId>,
    /// Cities that received the player's unique luxury (Indonesia).
    pub unique_luxury_cities: u8,
}

impl OwedChoices {
    /// Nothing differs from a new game (the world hash mixes it only then).
    pub fn is_empty(&self) -> bool {
        *self == OwedChoices::default()
    }
}

/// Which technologies a free technology may be.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FreeTechnology {
    /// Any the player may research now (Great Library, Oxford, Rationalism).
    Any,
    /// One the player may research now that this player knows (Assyria:
    /// the previous owner of a conquered city).
    KnownBy(crate::ids::PlayerId),
}

/// Which great people a free great person may be.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FreeGreatPerson {
    /// Any kind born from specialists (Liberty, Leaning Tower of Pisa).
    Any,
    /// A kind the Long Count has not given yet (Maya).
    LongCount,
}

/// Economy state of one player (players in id order).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SavedPlayerEconomy {
    pub golden_age_turns: u16,
    pub golden_age_meter: u32,
    pub golden_ages: u16,
    pub great_people_created: u16,
    pub great_writers_created: u16,
    pub great_artists_created: u16,
    pub great_musicians_created: u16,
    pub science_history: Vec<u32>,
    pub excess_happiness: i32,
    pub plots_bought: u16,
    /// Natural wonder sites found (tile ids, in the order found).
    pub natural_wonders_found: Vec<u32>,
    /// The AI plays this player.
    pub ai: bool,
    /// Owed free technologies, oldest first: `FREE_TECH_ANY`, else the id
    /// of the player whose technologies it may be.
    pub free_technologies: Vec<u16>,
    /// Owed free great people, oldest first: 0 any kind, 1 Long Count.
    pub free_great_people: Vec<u8>,
    pub long_count_given: u16,
    /// Entity ids of the cities that gave a conquest technology, ascending.
    pub conquest_tech_cities: Vec<u64>,
    pub unique_luxury_cities: u8,
}

/// `SavedPlayerEconomy::free_technologies` code of `FreeTechnology::Any`.
pub const FREE_TECH_ANY: u16 = u16::MAX;


fn encode_player(e: &PlayerEconomy, kernel: KernelPlayerRecord) -> SavedPlayerEconomy {
    SavedPlayerEconomy {
        golden_age_turns: e.golden_age_turns,
        golden_age_meter: e.golden_age_meter,
        golden_ages: e.golden_ages,
        great_people_created: e.great_people_created,
        great_writers_created: e.great_writers_created,
        great_artists_created: e.great_artists_created,
        great_musicians_created: e.great_musicians_created,
        science_history: e.science_history.to_vec(),
        excess_happiness: e.excess_happiness,
        plots_bought: kernel.plots_bought,
        natural_wonders_found: e.natural_wonders_found.iter().map(|t| t.0).collect(),
        ai: kernel.ai,
        free_technologies: e.owed.technologies.iter().map(|t| free_tech_code(*t)).collect(),
        free_great_people: e.owed.great_people.iter().map(|g| free_great_person_code(*g)).collect(),
        long_count_given: e.owed.long_count_given,
        conquest_tech_cities: e.owed.conquest_tech_cities.iter().map(|c| c.0).collect(),
        unique_luxury_cities: e.owed.unique_luxury_cities,
    }
}

fn free_tech_code(t: FreeTechnology) -> u16 {
    match t {
        FreeTechnology::Any => FREE_TECH_ANY,
        FreeTechnology::KnownBy(p) => u16::from(p.0),
    }
}

fn free_great_person_code(g: FreeGreatPerson) -> u8 {
    match g {
        FreeGreatPerson::Any => 0,
        FreeGreatPerson::LongCount => 1,
    }
}

/// The owed choices of a saved player (`players`: the player count).
fn decode_owed(p: &SavedPlayerEconomy, players: usize) -> Result<OwedChoices, &'static str> {
    let mut technologies = Vec::with_capacity(p.free_technologies.len());
    for code in &p.free_technologies {
        let t = match *code {
            FREE_TECH_ANY => FreeTechnology::Any,
            id if usize::from(id) < players => FreeTechnology::KnownBy(PlayerId(id as u8)),
            _ => return Err("free technology"),
        };
        technologies.push(t);
    }
    let mut great_people = Vec::with_capacity(p.free_great_people.len());
    for code in &p.free_great_people {
        let g = match code {
            0 => FreeGreatPerson::Any,
            1 => FreeGreatPerson::LongCount,
            _ => return Err("free great person"),
        };
        great_people.push(g);
    }
    let ascending = p.conquest_tech_cities.windows(2).all(|w| w[0] < w[1]);
    if !ascending {
        return Err("conquest technology cities");
    }
    Ok(OwedChoices {
        technologies,
        great_people,
        long_count_given: p.long_count_given,
        conquest_tech_cities: p.conquest_tech_cities.iter().map(|c| crate::ids::EntityId(*c)).collect(),
        unique_luxury_cities: p.unique_luxury_cities,
    })
}


fn decode_player(p: &SavedPlayerEconomy, players: usize) -> Result<PlayerEconomy, &'static str> {
    if p.science_history.len() != SCIENCE_HISTORY_TURNS {
        return Err("science history");
    }
    let mut science_history = [0u32; SCIENCE_HISTORY_TURNS];
    science_history.copy_from_slice(&p.science_history);
    Ok(PlayerEconomy {
        golden_age_turns: p.golden_age_turns,
        golden_age_meter: p.golden_age_meter,
        golden_ages: p.golden_ages,
        great_people_created: p.great_people_created,
        great_writers_created: p.great_writers_created,
        great_artists_created: p.great_artists_created,
        great_musicians_created: p.great_musicians_created,
        science_history,
        excess_happiness: p.excess_happiness,
        natural_wonders_found: p.natural_wonders_found.iter().map(|t| crate::ids::TileId(*t)).collect(),
        owed: decode_owed(p, players)?,
    })
}

/// Hash words of a player's owed choices (none when there are none).
fn owed_hash_words(p: &SavedPlayerEconomy) -> Vec<u64> {
    let empty = p.free_technologies.is_empty()
        && p.free_great_people.is_empty()
        && p.long_count_given == 0
        && p.conquest_tech_cities.is_empty()
        && p.unique_luxury_cities == 0;
    if empty {
        return Vec::new();
    }
    let mut words = vec![0x6f77_6564];
    words.extend(p.free_technologies.iter().map(|t| 0x1_0000 | u64::from(*t)));
    words.extend(p.free_great_people.iter().map(|g| 0x2_0000 | u64::from(*g)));
    words.push(0x3_0000 | u64::from(p.long_count_given));
    words.extend(p.conquest_tech_cities.iter().copied());
    words.push(0x4_0000 | u64::from(p.unique_luxury_cities));
    words
}


/// Mix one record into the economy fold (the order of the fields).
fn hash_player(p: &SavedPlayerEconomy, mix: &mut dyn FnMut(u64)) {
    mix(u64::from(p.golden_age_turns));
    mix(u64::from(p.golden_age_meter));
    mix(u64::from(p.golden_ages));
    mix(u64::from(p.great_people_created));
    mix(u64::from(p.great_writers_created));
    mix(u64::from(p.great_artists_created));
    mix(u64::from(p.great_musicians_created));
    for s in &p.science_history {
        mix(u64::from(*s));
    }
    mix(p.excess_happiness as u32 as u64);
    mix(u64::from(p.plots_bought));
    for t in &p.natural_wonders_found {
        mix(0x3_0000_0000 | u64::from(*t));
    }
    mix(u64::from(p.ai));
    // Mixed only when something is owed or given: games without free
    // choices keep their hashes.
    for w in owed_hash_words(p) {
        mix(w);
    }
}

impl ModulePlayer<Classic> for PlayerEconomy {
    type Saved = SavedPlayerEconomy;

    fn encode(&self, kernel: KernelPlayerRecord) -> SavedPlayerEconomy {
        encode_player(self, kernel)
    }

    fn kernel_part(saved: &SavedPlayerEconomy) -> KernelPlayerRecord {
        KernelPlayerRecord { plots_bought: saved.plots_bought, ai: saved.ai }
    }

    fn decode(saved: &SavedPlayerEconomy, players: usize, auth: &AuthWorld) -> Result<Self, String> {
        let economy = decode_player(saved, players).map_err(str::to_owned)?;
        let wonder_site = |t: &crate::ids::TileId| auth.map().natural_wonder(*t).is_some();
        if !economy.natural_wonders_found.iter().all(wonder_site) {
            return Err("found a natural wonder that is not on the map".to_owned());
        }
        Ok(economy)
    }

    fn hash(saved: &SavedPlayerEconomy, mix: &mut dyn FnMut(u64)) {
        hash_player(saved, mix)
    }
}

impl Field for SavedPlayerEconomy {
    fn put(&self, out: &mut Vec<u8>) {
        self.golden_age_turns.put(out);
        self.golden_age_meter.put(out);
        self.golden_ages.put(out);
        self.great_people_created.put(out);
        self.great_writers_created.put(out);
        self.great_artists_created.put(out);
        self.great_musicians_created.put(out);
        self.science_history.put(out);
        self.excess_happiness.put(out);
        self.plots_bought.put(out);
        self.natural_wonders_found.put(out);
        self.ai.put(out);
        self.free_technologies.put(out);
        self.free_great_people.put(out);
        self.long_count_given.put(out);
        self.conquest_tech_cities.put(out);
        self.unique_luxury_cities.put(out);
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            golden_age_turns: Field::take(r)?,
            golden_age_meter: Field::take(r)?,
            golden_ages: Field::take(r)?,
            great_people_created: Field::take(r)?,
            great_writers_created: Field::take(r)?,
            great_artists_created: Field::take(r)?,
            great_musicians_created: Field::take(r)?,
            science_history: Field::take(r)?,
            excess_happiness: Field::take(r)?,
            plots_bought: Field::take(r)?,
            natural_wonders_found: Field::take(r)?,
            ai: Field::take(r)?,
            free_technologies: Field::take(r)?,
            free_great_people: Field::take(r)?,
            long_count_given: Field::take(r)?,
            conquest_tech_cities: Field::take(r)?,
            unique_luxury_cities: Field::take(r)?,
        })
    }
}

