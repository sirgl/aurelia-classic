//! The classic content types the module's fields on kernel rows hold
//! (`classic::rows`): the unit and building abilities its systems read, great
//! work slots and theming bonuses.

use kernel::rules::content::{BuildingKind, BuildingKindTag, ImprovementKind, ImprovementKindTag, RoleTag, UnitRole};

/// Indexes into the classic tables (`classic::tables::ClassicTables`, sorted by key).
macro_rules! table_ids {
    ($($name:ident),* $(,)?) => {$(
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(pub u16);
    )*};
}

table_ids!(PolicyBranchId, ReligionId, CityStateTypeId, CityStateId, GreatPersonId);

/// A leader's personality (classic/docs/ai-leaders.md): how much the AI of a
/// civilization cares for each side of the game, from 0 to 10. `NEUTRAL`
/// (5 everywhere) values every side by the score alone, as the AI of
/// 16 October (now `allocator`) does. The values are this project's own
/// judgement from public descriptions of the leaders (classic/assets/rules/classic/
/// personalities.ron says why for each), never another game's files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Personality {
    /// New cities.
    pub expansion: u8,
    pub wonders: u8,
    pub science: u8,
    pub culture: u8,
    pub gold: u8,
    pub religion: u8,
    /// Appetite for wars of conquest; above neutral, the domination victory.
    pub aggression: u8,
    /// How little the leader minds what a conquest costs at home (the
    /// unrest of conquered cities).
    pub war_tolerance: u8,
    /// City-states and the World Congress; above neutral, the diplomatic
    /// victory.
    pub diplomacy: u8,
}

impl Personality {
    /// The middle of the scale: no leaning.
    /// Unciv: models/ruleset/nation/Personality.kt (scale 0 to 10, default 5), equal.
    pub const MIDDLE: u8 = 5;
    /// Unciv: models/ruleset/nation/Personality.kt (scale 0 to 10, default 5), equal.
    pub const MAX: u8 = 10;
    pub const NEUTRAL: Personality = Personality {
        expansion: Self::MIDDLE,
        wonders: Self::MIDDLE,
        science: Self::MIDDLE,
        culture: Self::MIDDLE,
        gold: Self::MIDDLE,
        religion: Self::MIDDLE,
        aggression: Self::MIDDLE,
        war_tolerance: Self::MIDDLE,
        diplomacy: Self::MIDDLE,
    };

    /// Every value, in field order.
    pub fn values(&self) -> [u8; 9] {
        [
            self.expansion,
            self.wonders,
            self.science,
            self.culture,
            self.gold,
            self.religion,
            self.aggression,
            self.war_tolerance,
            self.diplomacy,
        ]
    }
}

/// Unit abilities the economy and society systems read; the other abilities stay
/// inert effects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct UnitAbilities {
    /// While it is built the city's food surplus goes to production and the
    /// city neither grows nor starves (settler).
    pub food: bool,
    /// Golden age turns of a great artist.
    pub golden_age_turns: u16,
    /// Turns of science a great scientist grants.
    pub beakers_turns: u16,
    /// Production of a great engineer's hurry: base + per citizen.
    pub base_hurry: u32,
    pub hurry_multiplier: u32,
    /// Gold of a great merchant's trade mission: base + per era.
    pub base_gold: u32,
    pub gold_per_era: u32,
    /// Costs no gold maintenance (caravan, cargo ship).
    pub no_maintenance: bool,
    /// Establishes trade routes (caravan, cargo ship).
    pub trade: bool,
}

/// Typed unit ability that a mechanic reads (catalog `unit_ability.*`
/// with an implemented mechanic; the rest stay inert effects).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnitAbility {
    /// Great Prophet: founds or enhances a religion (`found_religion`).
    FoundReligion,
    /// Spreads its religion to a city (`spread_religion`).
    SpreadReligion,
    /// Spreads available (`religion_spreads`).
    ReligionSpreads(u8),
    /// Religious strength (`religious_strength`).
    ReligiousStrength(u16),
    /// Inquisitor: removes other religions from a city (`remove_heresy`).
    RemoveHeresy,
    /// Inquisitor: its presence blocks other religions' spread
    /// (`prohibits_spread`).
    ProhibitsSpread,
    /// Bought only with an enhanced religion (`requires_enhanced_religion`).
    RequiresEnhancedReligion,
    /// Great Musician concert: tourism × the player's tourism
    /// (`one_shot_tourism`).
    OneShotTourism(u16),
    /// Share of a concert's influence on the other civilizations
    /// (`one_shot_tourism_percent_others`).
    OneShotTourismPercentOthers(u16),
    /// Great Writer treatise: culture of this many previous turns
    /// (`base_culture_turns_to_count`).
    CultureTurnsToCount(u16),
    /// Digs antiquity sites (Archaeologist).
    Excavates,
    /// While it is built the city's food surplus goes to production (settler).
    Food,
    /// Golden age turns of a great artist.
    GoldenAgeTurns(u16),
    /// Turns of science a great scientist grants.
    BeakersTurns(u16),
    /// A great engineer's hurry production: base and per citizen.
    BaseHurry(u32),
    HurryMultiplier(u32),
    /// A great merchant's trade mission gold: base and per era.
    BaseGold(u32),
    GoldPerEra(u32),
    /// Costs no gold maintenance (caravan, cargo ship).
    NoMaintenance,
    /// Establishes trade routes (caravan, cargo ship).
    Trade,
    /// Bought with faith only in a city whose belief unlocks its era (military
    /// units).
    FaithPurchaseNeedsBelief,
    /// A machine, not people. Data only: no rule here reads it (unverified
    /// whether any should).
    Mechanized,
    /// How eagerly an enemy goes for this civilian (the AI's choice of an
    /// adjacent target).
    CivilianAttackPriority(CivilianPriority),
    /// Conscription level. Data only: no rule here drafts units (unverified
    /// whether any should).
    Conscription(u8),
    /// Special unit type of a unit that is not cargo (great people, the
    /// stealth bomber).
    Special(SpecialUnit),
    /// Air slots it takes in a city.
    AirSlots(u8),
    /// Work rate of a unit that is not a worker (the Legion and the Samurai
    /// build at 100; great people 1, their improvement is instant).
    WorkRate(u16),
    /// A unit bought with gold may move at once.
    MoveAfterPurchase,
    /// Ancient ruins give it no bad outcome. Data only: the map has no
    /// ancient ruins that hurt.
    NoBadGoodies,
    /// Bought, never trained (Landsknecht).
    PurchaseOnly,
    /// May buy out a city-state (Merchant of Venice).
    BuyCityState,
    /// Founds cities only off its owner's capital's landmass (Conquistador).
    FoundAbroad,
    /// Sells this many cargoes of exotic goods next to a foreign city (Nau).
    ExoticGoods(u8),
}

/// How high a civilian unit stands on an enemy's list of targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CivilianPriority {
    Low,
    /// High in the first eras only (the worker).
    HighEarlyGameOnly,
    High,
}

/// The special kinds of a unit that is not cargo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpecialUnit {
    /// A great person (the unique ones too).
    People,
    /// The stealth bomber (no carrier takes it).
    Stealth,
}

impl UnitAbilities {
    /// The economy abilities of a unit's ability list.
    pub fn of(abilities: &[UnitAbility]) -> Self {
        let mut out = UnitAbilities::default();
        for ability in abilities {
            match *ability {
                UnitAbility::Food => out.food = true,
                UnitAbility::GoldenAgeTurns(n) => out.golden_age_turns = n,
                UnitAbility::BeakersTurns(n) => out.beakers_turns = n,
                UnitAbility::BaseHurry(n) => out.base_hurry = n,
                UnitAbility::HurryMultiplier(n) => out.hurry_multiplier = n,
                UnitAbility::BaseGold(n) => out.base_gold = n,
                UnitAbility::GoldPerEra(n) => out.gold_per_era = n,
                UnitAbility::NoMaintenance => out.no_maintenance = true,
                UnitAbility::Trade => out.trade = true,
                _ => {}
            }
        }
        out
    }
}

/// Typed building ability that a mechanic reads (catalog `building.*`
/// flags with an implemented mechanic).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuildingAbility {
    /// Owning this many of the building lets the player choose an ideology
    /// (Factory, 3).
    TriggersIdeologyChoice(u16),
    /// Completing it puts a great work of art by its owner into its slot
    /// (the Parthenon frieze).
    FreeGreatWork,
    /// Other civilizations' Internet bonus does not reach its owner (Great
    /// Firewall).
    NullifyInfluence,
    /// Missionaries bought in its city spread this many more times (Great
    /// Mosque of Djenne, 1).
    MissionaryExtraSpreads(u8),
    /// Percent added to the religious pressure its city exerts (Grand
    /// Temple, 100).
    ReligiousPressure(u16),
    /// Its city may send food trade routes to own cities (Granary).
    FoodTradeRoutes,
    /// Its city may send production trade routes to own cities (Workshop,
    /// Longhouse).
    ProductionTradeRoutes,
    /// A counter-espionage building (Constabulary, Police Station, Great
    /// Firewall, National Intelligence Agency): the AI builds it where its
    /// technologies are at risk.
    CounterEspionage,
    /// Completing it recomputes at once the steal rate of the spies already
    /// gathering intelligence in its city (Great Firewall); other buildings
    /// change the rate from the next theft.
    AffectSpiesNow,
    /// Enemy land units pay one more movement point per tile of its
    /// owner's territory until the owner learns the building's obsolete
    /// technology (the Great Wall; rules.md).
    BorderObstacle,
    /// Its city may send and receive airlifts (Airport).
    Airlift,
    /// May strike at range (Walls, Castle, Arsenal, Military Base and their
    /// replacements): data only, every city can strike here
    /// (classic/docs/classic/rules.md).
    AllowsRangeStrike,
}

/// Great work slot types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GreatWorkSlot {
    /// Art and artifacts.
    ArtArtifact,
    Literature,
    Music,
}

impl GreatWorkSlot {
    /// The slot named by a `core:great-work-slot/<slug>` key.
    pub fn from_key(key: &str) -> Option<Self> {
        match key.strip_prefix("core:great-work-slot/")? {
            "art-artifact" => Some(GreatWorkSlot::ArtArtifact),
            "literature" => Some(GreatWorkSlot::Literature),
            "music" => Some(GreatWorkSlot::Music),
            _ => None,
        }
    }
}

/// One theming bonus of a building; the
/// first bonus whose every requirement holds is the building's theme.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct ThemingBonus {
    pub bonus: u16,
    /// All works of one era.
    pub same_era: bool,
    /// No two works of one era.
    pub unique_eras: bool,
    /// Art only.
    pub must_be_art: bool,
    /// Artifacts only.
    pub must_be_artifact: bool,
    /// As many art works as artifacts.
    pub must_be_equal_art_artifact: bool,
    /// Every work is by the building's owner.
    pub requires_owner: bool,
    /// No work is by the building's owner.
    pub requires_any_but_owner: bool,
    /// All works by one civilization.
    pub requires_same_player: bool,
    /// No two works by one civilization.
    pub requires_unique_players: bool,
}

/// The kind of a city-state: what it gives its friends and allies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CityStateTrait {
    Cultured,
    Militaristic,
    Maritime,
    Mercantile,
    Religious,
}

/// The relation a city-state bonus needs (allies get both).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CityStateLevel {
    Friends,
    Allies,
}

/// What a city-state bonus gives (classic/docs/classic/rules.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CityStateBonusKind {
    /// Food ×100 on the capital's tile.
    CapitalFood,
    /// Food ×100 on every city's tile (the capital included).
    AllCitiesFood,
    /// Culture per turn.
    Culture,
    /// Faith per turn.
    Faith,
    /// Happiness.
    Happiness,
    /// Turns between gifted units (base; allies: change).
    UnitGiftTurns,
    /// Random extra turns between gifted units.
    UnitGiftRandom,
}

/// Where a great person's points come from (specialists and
/// buildings for most; combat experience for generals and admirals
/// (rules.md); faith for prophets).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PointsSource {
    #[default]
    Specialists,
    CombatExperience,
    Faith,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProjectKind {
    Project,
    /// World Congress project (contributed to, not built).
    LeagueProject,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PolicyRole {
    #[default]
    Policy,
    /// Ideology tenet of tier 1..=3.
    Tenet(u8),
    /// Bonus of adopting the branch.
    Opener,
    /// Bonus of completing the branch.
    Finisher,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PolicyBranchKind {
    Normal,
    Ideology,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BeliefCategory {
    Pantheon,
    Founder,
    Follower,
    Enhancer,
    Reformation,
}

/// Great person kinds that accumulate points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub enum GreatPersonKind {
    Artist,
    Engineer,
    Merchant,
    Scientist,
    Writer,
    Musician,
    General,
    Admiral,
    Prophet,
}

impl GreatPersonKind {
    pub fn index(self) -> usize {
        match self {
            GreatPersonKind::Artist => 0,
            GreatPersonKind::Engineer => 1,
            GreatPersonKind::Merchant => 2,
            GreatPersonKind::Scientist => 3,
            GreatPersonKind::Writer => 4,
            GreatPersonKind::Musician => 5,
            GreatPersonKind::General => 6,
            GreatPersonKind::Admiral => 7,
            GreatPersonKind::Prophet => 8,
        }
    }
}

pub const GREAT_PERSON_KINDS: usize = 9;

/// The classic words in the kernel's content enums, by the tags they had
/// there before the split (`kernel::rules::content::module_tag!`).
pub const GREAT_PERSON_ROLE: UnitRole = UnitRole::Module(RoleTag::new(1));
pub const RELIGIOUS_ROLE: UnitRole = UnitRole::Module(RoleTag::new(2));
/// Bought with faith only (Monastery, Cathedral, Mosque, Pagoda).
pub const RELIGIOUS_BUILDING: BuildingKind = BuildingKind::Module(BuildingKindTag::new(1));
pub const GUILD: BuildingKind = BuildingKind::Module(BuildingKindTag::new(2));
/// Created by a great person (academy, citadel, ...).
pub const GREAT_PERSON_IMPROVEMENT: ImprovementKind = ImprovementKind::Module(ImprovementKindTag::new(1));
pub const BARBARIAN_CAMP: ImprovementKind = ImprovementKind::Module(ImprovementKindTag::new(3));
