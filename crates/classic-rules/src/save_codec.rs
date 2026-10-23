//! The bytes of a classic save: the kernel's codec
//! (`kernel::persistence::codec`) with the classic sections at the module's
//! places — the trade routes last in the economy section, the rest after
//! the player kinds — exactly where they were before the platform split.

pub use kernel::persistence::codec::{CodecError, Field, ModuleSections, Reader};

use crate::classic::save::{SavedBarbarians, SavedClassic};
use crate::persistence::culture::{SavedCulture, SavedPlayerCulture};
use crate::persistence::religion::{
    SavedCityFaith, SavedFaith, SavedPlayerFaith, SavedReligion, SavedReligiousUnit,
};
use crate::persistence::schema::SavedRelations;
use crate::persistence::tourism::{SavedPlayerTourism, SavedTourism};
use crate::persistence::trade::SavedTradeRoute;
use crate::persistence::SaveGameV1;

/// 4: the AI memory section after the world (`codec::ai`);
/// 5: culture pools and social policies appended to the world;
/// 6: faith and religion appended;
/// 7: great works, influence and archaeology appended;
/// 8: M4 track C appended (combat, diplomacy, barbarians, city-states,
/// victory); 9: the World Congress; 10: espionage; 11: the M4-A economy
/// section and resource deposit amounts; 12: trade routes; 13: natural
/// wonders found (economy section); 14: owed free technologies and great
/// people (economy section); 15: World Congress projects and their rewards;
/// 16: a city's capital connection and its paid first-conversion bonus;
/// 17: the Patronage finisher's countdown per city-state friend; 18: the
/// round a city was acquired; 19: pillaged improvements; 20: the carrier of
/// a unit (the unit record's module part); 21: the religious strength a
/// missionary lost in rival land; 22: the deposits and cities each player
/// last saw (the kernel's knowledge sections, `SIGHTINGS_SINCE`).
pub const CODEC_VERSION: u32 = 22;

/// The codec version whose religious unit record gained `strength_lost`;
/// older saves read it as nothing lost.
const STRENGTH_LOST_SINCE: u32 = 21;

pub fn to_bytes(save: &SaveGameV1) -> Vec<u8> {
    kernel::persistence::codec::to_bytes(save)
}

pub fn from_bytes(bytes: &[u8]) -> Result<SaveGameV1, CodecError> {
    kernel::persistence::codec::from_bytes(bytes)
}

impl ModuleSections for SavedClassic {
    const CODEC_VERSION: u32 = CODEC_VERSION;
    /// Version 19 differs only by the unit record's last field: every unit
    /// of an older save is on no carrier.
    const OLDEST_CODEC_VERSION: u32 = 19;
    /// Older saves remember the map's deposits and know no city's numbers.
    const SIGHTINGS_SINCE: u32 = 22;
    type EconomyPart = Vec<SavedTradeRoute>;

    fn put_economy_part(&self, out: &mut Vec<u8>) {
        self.trade_routes.put(out);
    }

    fn put_sections(&self, out: &mut Vec<u8>) {
        self.military.put(out);
        self.relations.put(out);
        self.barbarians.put(out);
        self.city_states.put(out);
        self.victory.put(out);
        self.congress.put(out);
        self.espionage.put(out);
        self.culture.put(out);
        self.faith.put(out);
        self.tourism.put(out);
    }

    fn take_economy_part(r: &mut Reader<'_>) -> Result<Vec<SavedTradeRoute>, CodecError> {
        Field::take(r)
    }

    fn take_sections(trade_routes: Vec<SavedTradeRoute>, r: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok(SavedClassic {
            trade_routes,
            military: Field::take(r)?,
            relations: Field::take(r)?,
            barbarians: Field::take(r)?,
            city_states: Field::take(r)?,
            victory: Field::take(r)?,
            congress: Field::take(r)?,
            espionage: Field::take(r)?,
            culture: Field::take(r)?,
            faith: Field::take(r)?,
            tourism: Field::take(r)?,
        })
    }
}

impl Field for SavedBarbarians {
    fn put(&self, out: &mut Vec<u8>) {
        self.mode.put(out);
        self.camps.put(out);
        self.released.put(out);
        self.cleared.put(out);
        self.rng.put(out);
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            mode: Field::take(r)?,
            camps: Field::take(r)?,
            released: Field::take(r)?,
            cleared: Field::take(r)?,
            rng: Field::take(r)?,
        })
    }
}

impl Field for crate::persistence::schema::SavedCityStates {
    fn put(&self, out: &mut Vec<u8>) {
        self.standings.put(out);
        self.counters.put(out);
        self.minors.put(out);
        self.majors.put(out);
        self.quests.put(out);
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            standings: Field::take(r)?,
            counters: Field::take(r)?,
            minors: Field::take(r)?,
            majors: Field::take(r)?,
            quests: Field::take(r)?,
        })
    }
}

impl Field for SavedPlayerFaith {
    fn put(&self, out: &mut Vec<u8>) {
        self.faith.put(out);
        self.pantheon.put(out);
        self.religion.put(out);
        self.prophets.put(out);
        self.bought.put(out);
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            faith: Field::take(r)?,
            pantheon: Field::take(r)?,
            religion: Field::take(r)?,
            prophets: Field::take(r)?,
            bought: Field::take(r)?,
        })
    }
}

impl Field for crate::persistence::schema::SavedEspionage {
    fn put(&self, out: &mut Vec<u8>) {
        self.spies.put(out);
        self.steal_rights.put(out);
        self.renaissance_granted.put(out);
        self.era_granted.put(out);
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            spies: Field::take(r)?,
            steal_rights: Field::take(r)?,
            renaissance_granted: Field::take(r)?,
            era_granted: Field::take(r)?,
        })
    }
}

impl Field for crate::persistence::schema::SavedCongress {
    fn put(&self, out: &mut Vec<u8>) {
        self.founded.put(out);
        self.host.put(out);
        self.special.put(out);
        self.united_nations.put(out);
        self.turns_until_session.put(out);
        self.session_round.put(out);
        self.leader_vote_due.put(out);
        self.proposers.put(out);
        self.proposals.put(out);
        self.delegates.put(out);
        self.votes.put(out);
        self.active.put(out);
        self.extra.put(out);
        self.last_round.put(out);
        self.projects.put(out);
        self.completed.put(out);
        self.rewards.put(out);
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            founded: Field::take(r)?,
            host: Field::take(r)?,
            special: Field::take(r)?,
            united_nations: Field::take(r)?,
            turns_until_session: Field::take(r)?,
            session_round: Field::take(r)?,
            leader_vote_due: Field::take(r)?,
            proposers: Field::take(r)?,
            proposals: Field::take(r)?,
            delegates: Field::take(r)?,
            votes: Field::take(r)?,
            active: Field::take(r)?,
            extra: Field::take(r)?,
            last_round: Field::take(r)?,
            projects: Field::take(r)?,
            completed: Field::take(r)?,
            rewards: Field::take(r)?,
        })
    }
}

impl Field for crate::persistence::schema::SavedVictory {
    fn put(&self, out: &mut Vec<u8>) {
        self.disabled.put(out);
        self.max_rounds.put(out);
        self.project_counts.put(out);
        self.world_leader.put(out);
        self.outcome.put(out);
        self.continued.put(out);
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            disabled: Field::take(r)?,
            max_rounds: Field::take(r)?,
            project_counts: Field::take(r)?,
            world_leader: Field::take(r)?,
            outcome: Field::take(r)?,
            continued: Field::take(r)?,
        })
    }
}

impl Field for SavedReligion {
    fn put(&self, out: &mut Vec<u8>) {
        self.religion.put(out);
        self.founder.put(out);
        self.holy_city.put(out);
        self.beliefs.put(out);
        self.enhanced.put(out);
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            religion: Field::take(r)?,
            founder: Field::take(r)?,
            holy_city: Field::take(r)?,
            beliefs: Field::take(r)?,
            enhanced: Field::take(r)?,
        })
    }
}

impl Field for SavedRelations {
    fn put(&self, out: &mut Vec<u8>) {
        self.peace_until.put(out);
        self.war_since.put(out);
        self.embassy.put(out);
        self.pact_until.put(out);
        self.research_until.put(out);
        self.research_points.put(out);
        self.friends_until.put(out);
        self.denounced_until.put(out);
        self.open_until.put(out);
        self.warmonger.put(out);
        self.trades.put(out);
        self.offers.put(out);
        self.next_offer.put(out);
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            peace_until: Field::take(r)?,
            war_since: Field::take(r)?,
            embassy: Field::take(r)?,
            pact_until: Field::take(r)?,
            research_until: Field::take(r)?,
            research_points: Field::take(r)?,
            friends_until: Field::take(r)?,
            denounced_until: Field::take(r)?,
            open_until: Field::take(r)?,
            warmonger: Field::take(r)?,
            trades: Field::take(r)?,
            offers: Field::take(r)?,
            next_offer: Field::take(r)?,
        })
    }
}

impl Field for SavedCityFaith {
    fn put(&self, out: &mut Vec<u8>) {
        self.pressure.put(out);
        self.population.put(out);
        self.adoption_bonus_paid.put(out);
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            pressure: Field::take(r)?,
            population: Field::take(r)?,
            adoption_bonus_paid: Field::take(r)?,
        })
    }
}

impl Field for SavedReligiousUnit {
    fn put(&self, out: &mut Vec<u8>) {
        self.unit.put(out);
        self.religion.put(out);
        self.spreads_left.put(out);
        self.strength_lost.put(out);
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            unit: Field::take(r)?,
            religion: Field::take(r)?,
            spreads_left: Field::take(r)?,
            strength_lost: r.since(STRENGTH_LOST_SINCE)?,
        })
    }
}

impl Field for SavedFaith {
    fn put(&self, out: &mut Vec<u8>) {
        self.players.put(out);
        self.religions.put(out);
        self.cities.put(out);
        self.units.put(out);
        self.pantheons.put(out);
        self.prophet_rng.put(out);
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            players: Field::take(r)?,
            religions: Field::take(r)?,
            cities: Field::take(r)?,
            units: Field::take(r)?,
            pantheons: Field::take(r)?,
            prophet_rng: Field::take(r)?,
        })
    }
}

impl Field for SavedPlayerCulture {
    fn put(&self, out: &mut Vec<u8>) {
        self.culture.put(out);
        self.lifetime.put(out);
        self.recent.put(out);
        self.policies.put(out);
        self.branches.put(out);
        self.ideology.put(out);
        self.free_policies.put(out);
        self.free_tenets.put(out);
        self.free_used.put(out);
        self.max_cities.put(out);
        self.free_buildings.put(out);
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            culture: Field::take(r)?,
            lifetime: Field::take(r)?,
            recent: Field::take(r)?,
            policies: Field::take(r)?,
            branches: Field::take(r)?,
            ideology: Field::take(r)?,
            free_policies: Field::take(r)?,
            free_tenets: Field::take(r)?,
            free_used: Field::take(r)?,
            max_cities: Field::take(r)?,
            free_buildings: Field::take(r)?,
        })
    }
}

impl Field for SavedCulture {
    fn put(&self, out: &mut Vec<u8>) {
        self.players.put(out);
        self.ideology_adopters.put(out);
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            players: Field::take(r)?,
            ideology_adopters: Field::take(r)?,
        })
    }
}


impl Field for SavedPlayerTourism {
    fn put(&self, out: &mut Vec<u8>) {
        self.influence.put(out);
        self.influence_before.put(out);
        self.lifetime_before.put(out);
        self.opinion.put(out);
        self.dissatisfaction.put(out);
        self.preferred.put(out);
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            influence: Field::take(r)?,
            influence_before: Field::take(r)?,
            lifetime_before: Field::take(r)?,
            opinion: Field::take(r)?,
            dissatisfaction: Field::take(r)?,
            preferred: Field::take(r)?,
        })
    }
}

impl Field for SavedTourism {
    fn put(&self, out: &mut Vec<u8>) {
        self.works.put(out);
        self.cities.put(out);
        self.players.put(out);
        self.records.put(out);
        self.sites.put(out);
        self.digs.put(out);
        self.pending.put(out);
        self.landmarks.put(out);
        self.triggered.put(out);
        self.rng.put(out);
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            works: Field::take(r)?,
            cities: Field::take(r)?,
            players: Field::take(r)?,
            records: Field::take(r)?,
            sites: Field::take(r)?,
            digs: Field::take(r)?,
            pending: Field::take(r)?,
            landmarks: Field::take(r)?,
            triggered: Field::take(r)?,
            rng: Field::take(r)?,
        })
    }
}

impl Field for SavedTradeRoute {
    fn put(&self, out: &mut Vec<u8>) {
        self.owner.put(out);
        self.origin.put(out);
        self.destination.put(out);
        self.domain.put(out);
        self.kind.put(out);
        self.unit_kind.put(out);
        self.path.put(out);
        self.position.put(out);
        self.forward.put(out);
        self.circuits_done.put(out);
        self.circuits_total.put(out);
        self.yields.put(out);
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            owner: Field::take(r)?,
            origin: Field::take(r)?,
            destination: Field::take(r)?,
            domain: Field::take(r)?,
            kind: Field::take(r)?,
            unit_kind: Field::take(r)?,
            path: Field::take(r)?,
            position: Field::take(r)?,
            forward: Field::take(r)?,
            circuits_done: Field::take(r)?,
            circuits_total: Field::take(r)?,
            yields: Field::take(r)?,
        })
    }
}

