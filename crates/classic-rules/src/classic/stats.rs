//! The classic stats (`Vocabulary::Stat`, `Stat::Module`): the numbers of
//! happiness, great people, golden ages, trade routes, plunder, rule
//! exceptions, society, espionage and the World Congress that effects add
//! to. Each keeps its tag in the one closed enum the stats were before the
//! split (`kernel::rules::stats`), so the fingerprint and the order of sums
//! are what they were.

use std::hash::{Hash, Hasher};

use kernel::rules::vocabulary::{hash_tag, StatWord, Word};

use crate::classic::Classic;
use crate::rules::effects::Scope;
use crate::rules::stats::GreatPersonKind;

/// A classic stat (variants in tag order).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub enum ClassicStat {
    /// Percent of every great person's points.
    GreatPeopleRate,
    /// Percent of one kind's points (great scientist, ...).
    GreatPersonRate(GreatPersonKind),
    /// Happiness per city of the player.
    HappinessPerCity,
    /// Happiness of each luxury the player has.
    HappinessPerLuxury,
    /// Percent of the natural wonder happiness.
    NaturalWonderHappinessPercent,
    /// Percent of the unhappiness from the number of cities (policies and
    /// traits).
    CityCountUnhappinessPercent,
    /// Percent of the unhappiness from population (policies, the capital's
    /// buildings and traits): a player's applies to every city, a city's to
    /// that city.
    PopulationUnhappinessPercent,
    /// Percent of the length of golden ages.
    GoldenAgeLengthPercent,
    /// Trade route numbers (classic/docs/classic/rules.md).
    Trade(TradeStat),
    /// Percent of the gold the player takes from barbarian camps and
    /// captured cities (a trait: Songhai +200).
    PlunderGoldPercent,
    /// Percent of the gold the city's captor takes (Burial Tomb +100).
    CapturedGoldPercent,
    /// An exception to a rule for the player: it holds while the stat's
    /// sum is positive (each source adds 1).
    Rule(PlayerRule),
    /// Society numbers of policies, buildings and traits (rules.md).
    Society(SocietyStat),
    /// Espionage numbers (classic/docs/classic/rules.md).
    Espionage(EspionageStat),
    /// World Congress delegates (classic/docs/classic/rules.md).
    League(LeagueStat),
    /// Percent of the gold of each city connected to the capital
    /// (Machu Picchu).
    ConnectionGoldPercent,
    /// Percent points added to the research agreement payout, base 50
    /// (Porcelain Tower, Scientific Revolution; rules "Diplomacy" §5.8).
    ResearchAgreementPercent,
    /// Percent of the science a great scientist grants
    /// (International Space Station; §10.6).
    GreatScientistSciencePercent,
    /// Percent of every great person's points per declaration of
    /// friendship the player has (Sweden; §5.9).
    GreatPeopleRatePerFriend,
    /// Units whose gold maintenance the player does not pay, on top of the
    /// difficulty level's (Volunteer Army).
    FreeMaintenanceUnits,
    /// Percent chance that a land barbarian unit defeated inside its camp
    /// joins the victor with the camp's gold (Germany).
    LandBarbarianConversionPercent,
}

impl Word for ClassicStat {
    fn tag(self) -> u8 {
        match self {
            ClassicStat::GreatPeopleRate => 0,
            ClassicStat::GreatPersonRate(_) => 1,
            ClassicStat::HappinessPerCity => 4,
            ClassicStat::HappinessPerLuxury => 5,
            ClassicStat::NaturalWonderHappinessPercent => 6,
            ClassicStat::CityCountUnhappinessPercent => 7,
            ClassicStat::PopulationUnhappinessPercent => 8,
            ClassicStat::GoldenAgeLengthPercent => 9,
            ClassicStat::Trade(_) => 10,
            ClassicStat::PlunderGoldPercent => 11,
            ClassicStat::CapturedGoldPercent => 12,
            ClassicStat::Rule(_) => 19,
            ClassicStat::Society(_) => 20,
            ClassicStat::Espionage(_) => 21,
            ClassicStat::League(_) => 22,
            ClassicStat::ConnectionGoldPercent => 24,
            ClassicStat::ResearchAgreementPercent => 25,
            ClassicStat::GreatScientistSciencePercent => 26,
            ClassicStat::GreatPeopleRatePerFriend => 27,
            ClassicStat::FreeMaintenanceUnits => 28,
            ClassicStat::LandBarbarianConversionPercent => 29,
        }
    }
}

impl Hash for ClassicStat {
    fn hash<H: Hasher>(&self, state: &mut H) {
        hash_tag(self.tag(), state);
        match self {
            ClassicStat::GreatPersonRate(kind) => kind.hash(state),
            ClassicStat::Trade(t) => t.hash(state),
            ClassicStat::Rule(r) => r.hash(state),
            ClassicStat::Society(s) => s.hash(state),
            ClassicStat::Espionage(e) => e.hash(state),
            ClassicStat::League(l) => l.hash(state),
            _ => {}
        }
    }
}

impl StatWord<Classic> for ClassicStat {
    fn fits(self, scope: Scope) -> bool {
        match self {
            ClassicStat::GreatPeopleRate | ClassicStat::GreatPersonRate(_) => matches!(scope, Scope::City | Scope::Player),
            ClassicStat::PopulationUnhappinessPercent => matches!(scope, Scope::City | Scope::Player),
            ClassicStat::HappinessPerCity
            | ClassicStat::HappinessPerLuxury
            | ClassicStat::NaturalWonderHappinessPercent
            | ClassicStat::CityCountUnhappinessPercent
            | ClassicStat::GoldenAgeLengthPercent
            | ClassicStat::ConnectionGoldPercent
            | ClassicStat::ResearchAgreementPercent
            | ClassicStat::GreatScientistSciencePercent
            | ClassicStat::GreatPeopleRatePerFriend
            | ClassicStat::FreeMaintenanceUnits
            | ClassicStat::LandBarbarianConversionPercent
            | ClassicStat::PlunderGoldPercent
            | ClassicStat::Rule(_)
            | ClassicStat::League(_) => scope == Scope::Player,
            ClassicStat::CapturedGoldPercent => scope == Scope::City,
            ClassicStat::Trade(t) => t.fits(scope),
            ClassicStat::Society(s) => s.fits(scope),
            ClassicStat::Espionage(e) => e.fits(scope),
        }
    }
}

/// The espionage stats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub enum EspionageStat {
    /// City: percent of the rate at which other civilizations' spies steal
    /// technologies in the city (from its buildings and from its owner's:
    /// Constabulary -25, Great Firewall -100 here and -25 in every city, NIA
    /// -15 in every city).
    EnemyStealPercent,
    /// Player: percent of the rate at which its spies steal
    /// (Industrial Espionage +100).
    StealPercent,
    /// Player: percent of the detection roll against spies stealing in its
    /// cities (Double Agents +100).
    CatchPercent,
    /// Player: percent of the votes its spies gather in city-state
    /// elections (Covert Action +100).
    RigElectionPercent,
    /// Player: spies added with its first spy (a trait, England +1).
    ExtraSpies,
    /// Player: ranks its new and revived spies start with
    /// (National Intelligence Agency +1).
    StartingRank,
    /// Player, religion founder: pressure of its religion in each city one
    /// of its spies is in, in multiples of the adjacent-city pressure
    /// (a belief: Underground Sect 5).
    SpyPressure,
}

impl EspionageStat {
    fn fits(self, scope: Scope) -> bool {
        match self {
            EspionageStat::EnemyStealPercent => scope == Scope::City,
            _ => scope == Scope::Player,
        }
    }
}

/// The World Congress stats of a player.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub enum LeagueStat {
    /// Delegates in every session (Forbidden Palace +2).
    Delegates,
    /// Delegates per diplomat in the capital of a living civilization
    /// (Globalization +1).
    DelegatesPerDiplomat,
}

/// Numbers of the happiness, culture, science and city systems that
/// policies, buildings and civilization traits change. A flag is a stat
/// whose total is above zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub enum SocietyStat {
    /// City: percent of the food a specialist eats (Civil Society −50).
    SpecialistFoodPercent,
    /// City: percent of the unhappiness of a specialist (Universal Suffrage
    /// −50).
    SpecialistUnhappinessPercent,
    /// City flag: an occupied city is unhappy as a normal one (Courthouse).
    NoOccupiedUnhappiness,
    /// City: percent added to the culture of its world wonders (Flourishing
    /// of the Arts).
    WonderCulturePercent,
    /// Player: percent of the excess happiness added to the culture pool
    /// each turn (Fine Arts).
    HappinessToCulturePercent,
    /// Player: percent of the cities' science added while the empire is not
    /// unhappy (Rationalism).
    ScienceWhileHappyPercent,
    /// Player: percent of a luxury's happiness kept when its last copy is
    /// traded away (the Netherlands).
    LuxuryRetentionPercent,
    /// Player: percent added to the citizens a razing city loses each turn
    /// (the Huns).
    RazeSpeedPercent,
    /// Player: tiles a founded city claims besides its ring (the Shoshone).
    FoundingTerritoryTiles,
    /// Player flag: a science building completed in the capital adds half a
    /// median technology of science (Korea).
    CapitalScienceBuildingBoost,
    /// Player: the first cities founded on another landmass get a unique
    /// luxury (Indonesia).
    UniqueLuxuryCities,
    /// Player: copies of that luxury.
    UniqueLuxuryQuantity,
    /// Player flag: only cities on another landmass than the first
    /// capital count.
    UniqueLuxuryNewLandmass,
}

impl SocietyStat {
    fn fits(self, scope: Scope) -> bool {
        match self {
            SocietyStat::SpecialistFoodPercent
            | SocietyStat::SpecialistUnhappinessPercent
            | SocietyStat::NoOccupiedUnhappiness
            | SocietyStat::WonderCulturePercent => scope == Scope::City,
            SocietyStat::HappinessToCulturePercent
            | SocietyStat::ScienceWhileHappyPercent
            | SocietyStat::LuxuryRetentionPercent
            | SocietyStat::RazeSpeedPercent
            | SocietyStat::FoundingTerritoryTiles
            | SocietyStat::CapitalScienceBuildingBoost
            | SocietyStat::UniqueLuxuryCities
            | SocietyStat::UniqueLuxuryQuantity
            | SocietyStat::UniqueLuxuryNewLandmass => scope == Scope::Player,
        }
    }
}

/// Exceptions to a rule that policies and traits give a player.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub enum PlayerRule {
    /// Units garrisoned in cities cost no maintenance
    /// (Oligarchy, rules.md).
    GarrisonFreeMaintenance,
    /// Roads and railroads on hills cost no maintenance
    /// (Inca, rules.md).
    FreeHillsRouteMaintenance,
    /// Spaceship parts may be bought with gold (Space Procurements,
    /// rules.md).
    SpaceshipPartPurchase,
    /// A great engineer may hurry spaceship parts (Spaceflight Pioneers).
    SpaceshipPartHurry,
    /// A barbarian camp that appears on a tile the player explored shows in
    /// its tile memory (Honor; `systems::barbarians`).
    SeeBarbarianCamps,
}

/// Domain of a trade route: caravans by land, cargo ships by sea.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub enum TradeDomain {
    Land,
    Sea,
}

/// The trade route stats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub enum TradeStat {
    /// Trade routes the player may run.
    Routes,
    /// Tiles added to the range (by technologies).
    RangeTiles(TradeDomain),
    /// Percent of the range of routes from the city.
    RangePercent(TradeDomain),
    /// Gold of international routes from the city or of the player.
    RouteGold(TradeDomain),
    /// Gold the sender of a route to the city gets.
    TargetGold,
    /// Gold the city's owner gets from a route to it.
    RecipientGold,
    /// Percent of internal routes' food and production.
    InternalPercent,
    /// Percent of the trade routes the player may run, after their sum
    /// (Venice).
    RoutesPercent,
    /// Percent of the gold from differing resources of the player's
    /// international routes (Portugal).
    ResourceGoldPercent,
    /// Gold the sender of an international route to one of the player's
    /// cities gets (Morocco).
    IncomingRouteGold,
    /// Gold of the player's international routes to city-states
    /// (Merchant Confederacy).
    CityStateRouteGold,
    /// Gold of the player's international routes to a civilization that
    /// follows the player's ideology (Economic Union).
    SharedIdeologyRouteGold,
    /// Percent of the religious pressure a city of the player exerts on the
    /// destination of its trade route (Arabia; 3.9).
    ReligiousPressurePercent,
}

impl TradeStat {
    fn fits(self, scope: Scope) -> bool {
        match self {
            TradeStat::Routes
            | TradeStat::RangeTiles(_)
            | TradeStat::InternalPercent
            | TradeStat::RoutesPercent
            | TradeStat::ResourceGoldPercent
            | TradeStat::IncomingRouteGold
            | TradeStat::CityStateRouteGold
            | TradeStat::SharedIdeologyRouteGold
            | TradeStat::ReligiousPressurePercent => scope == Scope::Player,
            TradeStat::RouteGold(_) => matches!(scope, Scope::City | Scope::Player),
            TradeStat::RangePercent(_) | TradeStat::TargetGold | TradeStat::RecipientGold => scope == Scope::City,
        }
    }
}

