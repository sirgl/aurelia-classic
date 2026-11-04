//! The features of a player's turn for the end-to-end net
//! (classic/docs/ai-net-data.md, classic/docs/ai-neural-architecture.md).
//!
//! Pure functions from what a player knows (`AIObservation`,
//! `LocalObservation`, the public view `knowledge/public.rs`) and the rules'
//! tables to the rows of the data contract:
//!
//! - `Turn::encode`: the tile rows of every tile of the map, the global row
//!   and the entity rows (with their meta) of one player at the start of its
//!   turn; entities the turn meets later (a unit bought, a city founded, a
//!   foreign unit that came into sight) are appended as they are named
//!   (`Turn::own_unit_row` and the like);
//! - the candidates of the four heads with their rows and meta: a unit's
//!   commands (`unit_candidates`: the legal list `knowledge::actions`, long
//!   marches to points of interest, and waiting), the technologies
//!   researchable now (`research_candidates`), the items one city may build
//!   now (`production_candidates`), the policies, branches and ideologies
//!   adoptable now (`policy_candidates`);
//! - the neighbour table (`neighbor_table`) and the region table
//!   (`region_table`) of a map;
//! - the layout (`layout`): per table the list of `{name, scale}`.
//!
//! Rules of the features (the contract's): raw facts only — no scripted
//! valuation (no prices, no allocator values, no planner ranks, no danger
//! estimates); no content name (an item is its table data plus its id as a
//! parameter the net may embed); no absolute coordinate (distances are
//! clipped at `CLIP`); every value an `i16` with a fixed scale (`value =
//! stored / scale`, saturated); integer math only, so the playing mind
//! computes the same rows on every platform.

use std::collections::BTreeMap;

use crate::combat::exchange::{AttackKind, Verdict};
use crate::command::{Command, CommandError, Refusal};
use crate::derived::tile_yields::{plot_yields, TechSet};
use crate::ids::{BuildingKindId, CityHandle, ImprovementId, PlayerId, PolicyId, ProductionItem, TechId, TileId, UnitHandle, UnitKindId};
use crate::knowledge::actions::{codec_tag, unit_actions, MARCH_TAG};
use crate::knowledge::observation::{AIObservation, FaithObservation, LocalObservation, ObservedCity, ObservedUnit, OwnCity, OwnUnit, Relation};
use crate::knowledge::public::PlayerQueries;
use crate::knowledge::view::PlayerView;
use crate::math::{hex_disc, MapDimensions, HEX_DIRS};
use crate::rules::content::{BuildingKind, Domain, PolicyBranchId, ResourceClass, Yields};
use crate::rules::effects::{Condition, EffectSet, Modifier, Trigger, YieldKind};
use crate::rules::CompiledRuleset;
use crate::world::facts::TileFacts;
use crate::world::player::{ClassicKind, PlayerKind};

/// The format number of the contract (`meta.json`). 7: the event rows
/// (`events.rs`); 8: the blow's outcome on the attack candidates, one
/// candidate per blow, and the turn's entity rows at its start
/// (`Turn::start_entity_count`); 9: a unit's decision may end with the
/// defer candidate (`tag::DEFER`, `defer_candidate`) — the layout and every
/// other row as in format 8; 10: a move candidate says what the unit can
/// strike from its destination (`MoveStrike`, fourteen candidate columns after
/// format 9's, which keep their values).
pub const FORMAT: u32 = 10;

/// A worker serves this many jobs (the scripted mind's
/// `genome::Knob::JobsPerWorker`): `free_worker_jobs` = jobs − 4 × workers
/// alive or queued.
pub const JOBS_PER_WORKER: i64 = 4;

/// Distances are clipped here; "none" (no capital, no city) is `CLIP`.
pub const CLIP: i32 = 32;

/// Tiles of a region's side (blocks of `REGION` × `REGION` tiles).
pub const REGION: u32 = 4;

/// A march matches a point of interest at most this far from its
/// destination (the nearest one; exact first).
pub const MARCH_NEAR: i32 = 2;

/// City sites are offered to a founder within this many tiles.
pub const SITE_RADIUS: i32 = 10;

/// Candidate tags beside the codec tags of the unit commands (0–81, `MARCH`
/// for a long march).
pub mod tag {
    /// A long march (`Command::MarchTo`) to a point of interest.
    pub const MARCH: i32 = crate::knowledge::actions::MARCH_TAG as i32;
    /// The unit does nothing more this turn (no command).
    pub const WAIT: i32 = 250;
    /// Format 9: the unit decides again after the rest of the turn's queue
    /// (no command; `defer_candidate`). Its tag bucket (`1 + 251 mod 127` of
    /// the 128 a head has) is no other unit tag's.
    pub const DEFER: i32 = 251;
    pub const RESEARCH: i32 = 300;
    pub const PRODUCE_UNIT: i32 = 400;
    pub const PRODUCE_BUILDING: i32 = 401;
    pub const PRODUCE_PROJECT: i32 = 402;
    pub const ADOPT_POLICY: i32 = 500;
    pub const UNLOCK_BRANCH: i32 = 501;
    pub const CHOOSE_IDEOLOGY: i32 = 502;
    /// `FoundPantheon`: the pantheon belief.
    pub const PANTHEON: i32 = 600;
    /// `FoundReligion`, one decision per belief slot in this order: founder,
    /// follower, the pantheon belief (only without a pantheon), the bonus
    /// belief (only with an extra founding belief).
    pub const FOUND_FOUNDER: i32 = 601;
    pub const FOUND_FOLLOWER: i32 = 602;
    pub const FOUND_PANTHEON: i32 = 603;
    pub const FOUND_BONUS: i32 = 604;
    /// `EnhanceReligion`: follower, then enhancer.
    pub const ENHANCE_FOLLOWER: i32 = 605;
    pub const ENHANCE_ENHANCER: i32 = 606;
    /// `AddReformationBelief`.
    pub const REFORMATION: i32 = 607;
    /// `PurchaseWithFaith` of a unit or a building in one own city.
    pub const FAITH_UNIT: i32 = 610;
    pub const FAITH_BUILDING: i32 = 611;
    /// Keep the faith (no purchase).
    pub const FAITH_SAVE: i32 = 612;
    /// A city's focus (`SetCityFocus`); parameter the focus code
    /// (`CityFocus::code`).
    pub const FOCUS: i32 = 700;
    /// A city's avoid growth (`SetAvoidGrowth`); parameter 0 grow, 1 avoid.
    pub const AVOID_GROWTH: i32 = 701;
    /// A city's hand-assigned specialists of one kind (`SetSpecialists`);
    /// parameter `kind * 100 + count` (0: the automatic assignment).
    pub const SPECIALISTS: i32 = 702;
}

/// The heads (`decisions.npy` column 1).
pub mod head {
    pub const UNIT: i32 = 0;
    pub const RESEARCH: i32 = 1;
    pub const PRODUCTION: i32 = 2;
    pub const POLICY: i32 = 3;
    pub const RELIGION: i32 = 4;
    pub const CITIZENS: i32 = 5;
    pub const NAMES: [&str; 6] = ["unit", "research", "production", "policy", "religion", "citizens"];
}

/// Entity kinds (`entity_meta.npy` column 0). Column 3 is the entity's
/// persistent id, stable over the turns of a game: an own unit's and an own
/// city's entity id, a foreign city's tile (a city never moves; the player
/// knows a city by where it stands), a city-state's and a nation's player
/// id; `-1` for a foreign unit (the player cannot tell two foreign units of
/// one kind apart once they leave its sight). Every nation and city-state
/// the player has met (or is at war with) and that is alive has its row in
/// every turn.
pub mod kind {
    pub const OWN_UNIT: i32 = 0;
    pub const OWN_CITY: i32 = 1;
    pub const FOREIGN_UNIT: i32 = 2;
    pub const FOREIGN_CITY: i32 = 3;
    pub const CITY_STATE: i32 = 4;
    pub const PLAYER: i32 = 5;
}

/// The relation of an owner to the player (`entity_meta.npy` column 2, the
/// owner one-hots of the rows).
pub mod relation {
    pub const OWN: i32 = 0;
    /// A nation we declared friendship or a defensive pact with.
    pub const FRIEND: i32 = 1;
    /// A met nation at peace.
    pub const NEUTRAL: i32 = 2;
    /// At war (a city-state at war included; barbarians are `BARBARIAN`).
    pub const WAR: i32 = 3;
    pub const CITY_STATE: i32 = 4;
    pub const BARBARIAN: i32 = 5;
    /// A player we have not met.
    pub const UNMET: i32 = 6;
    pub const COUNT: usize = 7;
}

/// How many stored units one unit of the named value is
/// (`value = stored / scale`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scale {
    pub num: u32,
    pub den: u32,
}

impl Scale {
    pub fn value(self) -> f64 {
        f64::from(self.num) / f64::from(self.den)
    }
}

/// One column of a table.
#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub name: String,
    pub scale: Scale,
}

macro_rules! columns {
    ($(#[$doc:meta])* $name:ident { $($col:ident = ($label:literal, $num:literal, $den:literal)),* $(,)? }) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        #[repr(usize)]
        pub enum $name { $($col),* }

        impl $name {
            pub const FIELDS: &'static [(&'static str, Scale)] = &[$(($label, Scale { num: $num, den: $den })),*];
            pub const COUNT: usize = Self::FIELDS.len();
        }
    };
}

columns! {
    /// The tile row (`tiles.npy`). Unknown tiles are all zeros.
    T {
        Known = ("known", 1, 1),
        Visible = ("visible", 1, 1),
        SeenAgo = ("rounds_since_seen", 1, 1),
        Water = ("water", 1, 1),
        Lake = ("lake", 1, 1),
        CoastalLand = ("coastal_land", 1, 1),
        Hills = ("hills", 1, 1),
        Mountains = ("mountains", 1, 1),
        Impassable = ("impassable", 1, 1),
        MoveCost = ("move_cost", 1, 1),
        Defense = ("defense_percent", 1, 1),
        River = ("river", 1, 1),
        Rough = ("rough", 1, 1),
        CanFound = ("terrain_can_found_city", 1, 1),
        TerrainId = ("terrain_id_plus_1", 1, 1),
        FeatureId = ("feature_id_plus_1", 1, 1),
        ResourceId = ("resource_id_plus_1", 1, 1),
        ImprovementId = ("improvement_id_plus_1", 1, 1),
        WonderId = ("natural_wonder_id_plus_1", 1, 1),
        ResourceBonus = ("resource_bonus", 1, 1),
        ResourceStrategic = ("resource_strategic", 1, 1),
        ResourceLuxury = ("resource_luxury", 1, 1),
        Pillaged = ("pillaged", 1, 1),
        Road = ("road", 1, 1),
        Railroad = ("railroad", 1, 1),
        Food = ("yield_food", 1, 1),
        Production = ("yield_production", 1, 1),
        Gold = ("yield_gold", 1, 1),
        Science = ("yield_science", 1, 1),
        Culture = ("yield_culture", 1, 1),
        Faith = ("yield_faith", 1, 1),
        OwnerOwn = ("owner_own", 1, 1),
        OwnerFriend = ("owner_friend", 1, 1),
        OwnerNeutral = ("owner_neutral", 1, 1),
        OwnerWar = ("owner_war", 1, 1),
        OwnerCityState = ("owner_city_state", 1, 1),
        OwnerBarbarian = ("owner_barbarian", 1, 1),
        OwnerUnmet = ("owner_unmet", 1, 1),
        City = ("city", 1, 1),
        Worked = ("worked_by_own_city", 1, 1),
        OwnStrength = ("own_military_strength", 1, 1),
        OwnUnits = ("own_units", 1, 1),
        PeaceStrength = ("foreign_strength_at_peace", 1, 1),
        WarStrength = ("foreign_strength_at_war", 1, 1),
        ForeignUnits = ("foreign_units", 1, 1),
        CapitalDistance = ("capital_distance", 1, 1),
        CityDistance = ("own_city_distance", 1, 1),
    }
}

columns! {
    /// The fixed part of the global row (`globals.npy`); the multi-hot
    /// technologies, policies and branches follow (`layout`).
    G {
        Round = ("round", 1, 1),
        RoundsLeft = ("rounds_left", 1, 1),
        Era = ("era", 1, 1),
        Treasury = ("treasury", 1, 10),
        GoldIncome = ("gold_per_turn", 1, 1),
        ScienceIncome = ("science_per_turn", 1, 1),
        CultureIncome = ("culture_per_turn", 1, 1),
        FaithIncome = ("faith_per_turn", 1, 1),
        CulturePool = ("culture_pool", 1, 10),
        PolicyCost = ("next_policy_cost", 1, 10),
        FreePolicies = ("free_policies", 1, 1),
        FreeTenets = ("free_tenets", 1, 1),
        FaithPool = ("faith_pool", 1, 10),
        Happiness = ("excess_happiness", 1, 1),
        TurnHappiness = ("turn_happiness", 1, 1),
        GoldenAgeThreshold = ("golden_age_threshold", 1, 10),
        Cities = ("cities", 1, 1),
        Citizens = ("citizens", 1, 1),
        Units = ("units", 1, 1),
        MilitaryUnits = ("military_units", 1, 1),
        Techs = ("technologies", 1, 1),
        Policies = ("policies", 1, 1),
        ResearchQueue = ("research_queue", 1, 1),
        KnownPermille = ("known_tiles_permille", 1, 1),
        VisiblePermille = ("visible_tiles_permille", 1, 1),
        MapTiles = ("map_tiles", 1, 10),
        MajorsMet = ("nations_met", 1, 1),
        CityStatesMet = ("city_states_met", 1, 1),
        Wars = ("wars", 1, 1),
        Allies = ("city_state_allies", 1, 1),
        TradeRoutesLeft = ("trade_routes_left", 1, 1),
        OwnScore = ("own_score", 1, 10),
        BestRivalScore = ("best_rival_score", 1, 10),
        WorldWonders = ("world_wonders_built", 1, 1),
        FreeTechOwed = ("free_technology_owed", 1, 1),
        HasCapital = ("has_capital", 1, 1),
        HasPantheon = ("has_pantheon", 1, 1),
        HasReligion = ("has_religion", 1, 1),
        HasIdeology = ("has_ideology", 1, 1),
        VictoryScience = ("victory_science", 1, 1),
        VictoryDomination = ("victory_domination", 1, 1),
        VictoryCultural = ("victory_cultural", 1, 1),
        VictoryDiplomatic = ("victory_diplomatic", 1, 1),
        ReligionsFounded = ("religions_founded", 1, 1),
        ReligionsLeft = ("religions_left", 1, 1),
        CitiesFollowing = ("own_cities_following_own_religion", 1, 1),
        PantheonCost = ("pantheon_cost", 1, 10),
        ProphetCost = ("prophet_cost", 1, 10),
        ReligionEnhanced = ("religion_enhanced", 1, 1),
        // Format 6: the empire's demand signals.
        SettlersAlive = ("settlers_alive", 1, 1),
        SettlersQueued = ("settlers_queued", 1, 1),
        WorkersAlive = ("workers_alive", 1, 1),
        WorkersQueued = ("workers_queued", 1, 1),
        MilitaryQueued = ("military_units_queued", 1, 1),
        WorkerJobs = ("worker_jobs", 1, 1),
        FreeWorkerJobs = ("free_worker_jobs", 1, 1),
        CitySites = ("known_city_sites", 1, 1),
        // Format 6: the game's speed and length (`speed_facts`).
        SpeedProduction = ("speed_production_cost_percent", 100, 1),
        SpeedScience = ("speed_science_cost_percent", 100, 1),
        SpeedCulture = ("speed_culture_cost_percent", 100, 1),
        SpeedFaith = ("speed_faith_cost_percent", 100, 1),
        SpeedImprovement = ("speed_improvement_time_percent", 100, 1),
        MaxRounds = ("max_rounds", 100, 1),
        RoundShare = ("round_permille_of_max", 1000, 1),
    }
}

columns! {
    /// The entity row (`entities.npy`): one layout for every kind, a field
    /// that does not apply is 0.
    E {
        KindOwnUnit = ("kind_own_unit", 1, 1),
        KindOwnCity = ("kind_own_city", 1, 1),
        KindForeignUnit = ("kind_foreign_unit", 1, 1),
        KindForeignCity = ("kind_foreign_city", 1, 1),
        KindCityState = ("kind_city_state", 1, 1),
        KindPlayer = ("kind_player", 1, 1),
        RelOwn = ("relation_own", 1, 1),
        RelFriend = ("relation_friend", 1, 1),
        RelNeutral = ("relation_neutral", 1, 1),
        RelWar = ("relation_war", 1, 1),
        RelCityState = ("relation_city_state", 1, 1),
        RelBarbarian = ("relation_barbarian", 1, 1),
        RelUnmet = ("relation_unmet", 1, 1),
        UnitKindId = ("unit_kind_id_plus_1", 1, 1),
        Strength = ("strength", 1, 1),
        RangedStrength = ("ranged_strength", 1, 1),
        Range = ("range", 1, 1),
        Movement = ("movement", 1, 1),
        Sight = ("sight", 1, 1),
        DomainLand = ("domain_land", 1, 1),
        DomainSea = ("domain_sea", 1, 1),
        DomainAir = ("domain_air", 1, 1),
        Founder = ("founder", 1, 1),
        Builder = ("builder", 1, 1),
        Civilian = ("civilian", 1, 1),
        UnitCost = ("unit_cost", 1, 10),
        UnitEra = ("unit_era", 1, 1),
        Hp = ("hp", 1, 1),
        MovesLeft = ("moves_left", 10, 1),
        MovesMax = ("moves_max", 10, 1),
        Fortified = ("fortified", 1, 1),
        CanFortify = ("can_fortify", 1, 1),
        CanAttack = ("can_attack", 1, 1),
        Experience = ("experience", 1, 1),
        Level = ("level", 1, 1),
        Promotions = ("promotions", 1, 1),
        AttacksMade = ("attacks_made", 1, 1),
        HasGoal = ("has_march_order", 1, 1),
        GoalDistance = ("march_order_distance", 1, 1),
        Working = ("working", 1, 1),
        Population = ("population", 1, 1),
        FoodStock = ("food_stock", 1, 1),
        ProductionStock = ("production_stock", 1, 1),
        CityHp = ("city_hp", 1, 1),
        CityDamage = ("city_damage", 1, 1),
        Buildings = ("buildings", 1, 1),
        Wonders = ("wonders", 1, 1),
        WorkedTiles = ("worked_tiles", 1, 1),
        FoodSurplus = ("food_surplus", 1, 1),
        CityProduction = ("city_production", 1, 1),
        CityGold = ("city_gold", 1, 1),
        CityScience = ("city_science", 1, 1),
        CityCulture = ("city_culture", 1, 1),
        CityFaith = ("city_faith", 1, 1),
        Capital = ("capital", 1, 1),
        NotOwnStatus = ("occupied_or_puppet", 1, 1),
        QueueLength = ("queue_length", 1, 1),
        QueueUnit = ("queue_head_unit", 1, 1),
        QueueBuilding = ("queue_head_building", 1, 1),
        QueueProject = ("queue_head_project", 1, 1),
        Remembered = ("remembered_not_seen", 1, 1),
        Influence = ("influence", 1, 1),
        Resting = ("influence_resting", 1, 1),
        Friends = ("friends", 1, 1),
        AllyUs = ("ally_us", 1, 1),
        AllyOther = ("ally_other", 1, 1),
        AtWar = ("at_war", 1, 1),
        Pledged = ("pledged", 1, 1),
        CanBully = ("can_bully", 1, 1),
        Quests = ("quests", 1, 1),
        CanBuyout = ("can_buyout", 1, 1),
        DefensivePact = ("defensive_pact", 1, 1),
        OurEmbassy = ("our_embassy", 1, 1),
        TheirEmbassy = ("their_embassy", 1, 1),
        WeDenounced = ("we_denounced", 1, 1),
        TheyDenounced = ("they_denounced", 1, 1),
        Warmonger = ("warmonger", 1, 1),
        PeaceRounds = ("peace_treaty_rounds_left", 1, 1),
        WarRounds = ("war_rounds", 1, 1),
        TheyGrantEntry = ("they_grant_entry", 1, 1),
        WeGrantEntry = ("we_grant_entry", 1, 1),
        ResearchAgreement = ("research_agreement", 1, 1),
        PactsKnown = ("pacts_known", 1, 1),
        ScorePermille = ("score_permille_of_own", 1, 1),
        MetRounds = ("rounds_since_met", 1, 1),
        CapitalDistance = ("capital_distance", 1, 1),
        // Format 6: a city's religion (own cities and foreign cities in
        // sight at the turn's start; 0 where the player does not see it).
        MajorityNone = ("majority_religion_none", 1, 1),
        MajorityOwn = ("majority_religion_own", 1, 1),
        MajorityOther = ("majority_religion_other", 1, 1),
        OwnFollowers = ("own_religion_followers", 1, 1),
        MajorityFollowers = ("majority_religion_followers", 1, 1),
        OwnPressure = ("own_religion_pressure", 10, 1),
        OtherPressure = ("strongest_other_religion_pressure", 10, 1),
        HolyOwn = ("holy_city_own_religion", 1, 1),
        HolyOther = ("holy_city_other_religion", 1, 1),
    }
}

columns! {
    /// The candidate row (`candidates.npy`): one layout for all heads, a
    /// field that does not apply is 0.
    C {
        ActMove = ("act_move", 1, 1),
        ActMarch = ("act_march", 1, 1),
        ActMelee = ("act_melee", 1, 1),
        ActRanged = ("act_ranged", 1, 1),
        ActFound = ("act_found_city", 1, 1),
        ActFortify = ("act_fortify", 1, 1),
        ActWork = ("act_work", 1, 1),
        ActPillage = ("act_pillage", 1, 1),
        ActGreatPerson = ("act_great_person", 1, 1),
        ActPromote = ("act_promote", 1, 1),
        ActUpgrade = ("act_upgrade", 1, 1),
        ActTrade = ("act_trade", 1, 1),
        ActReligion = ("act_religion", 1, 1),
        ActAir = ("act_air", 1, 1),
        ActOther = ("act_other", 1, 1),
        ActWait = ("act_wait", 1, 1),
        TargetDistance = ("target_distance", 1, 1),
        TargetOwnCity = ("target_own_city", 1, 1),
        TargetForeignCity = ("target_foreign_city", 1, 1),
        TargetForeignUnit = ("target_foreign_unit", 1, 1),
        TargetOwnUnit = ("target_own_unit", 1, 1),
        MovesAfter = ("moves_after", 10, 1),
        ToDefender = ("preview_damage_to_defender", 1, 1),
        ToAttacker = ("preview_damage_to_attacker", 1, 1),
        DefenderHp = ("preview_defender_hp", 1, 1),
        PoiOwnCity = ("poi_own_city", 1, 1),
        PoiForeignCity = ("poi_foreign_city", 1, 1),
        PoiCityState = ("poi_city_state", 1, 1),
        PoiSite = ("poi_city_site", 1, 1),
        ImpFood = ("improvement_food", 1, 1),
        ImpProduction = ("improvement_production", 1, 1),
        ImpGold = ("improvement_gold", 1, 1),
        ImpScience = ("improvement_science", 1, 1),
        ImpCulture = ("improvement_culture", 1, 1),
        ImpFaith = ("improvement_faith", 1, 1),
        WorkTurns = ("work_turns", 1, 1),
        TechCost = ("tech_cost", 1, 10),
        TechTurns = ("tech_turns", 1, 1),
        TechEra = ("tech_era", 1, 1),
        TechBuildings = ("tech_unlocks_buildings", 1, 1),
        TechWonders = ("tech_unlocks_world_wonders", 1, 1),
        TechUnits = ("tech_unlocks_units", 1, 1),
        TechImprovements = ("tech_unlocks_improvements", 1, 1),
        TechProjects = ("tech_unlocks_projects", 1, 1),
        TechLeadsTo = ("tech_leads_to", 1, 1),
        TechBestStrength = ("tech_best_unit_strength", 1, 1),
        TechEmbarking = ("tech_embarking", 1, 1),
        La2Buildings = ("lookahead2_buildings", 1, 1),
        La2WorldWonders = ("lookahead2_world_wonders", 1, 1),
        La2Units = ("lookahead2_units", 1, 1),
        La2Improvements = ("lookahead2_improvements", 1, 1),
        La2Projects = ("lookahead2_projects", 1, 1),
        La2Science = ("lookahead2_science", 1, 1),
        La2Production = ("lookahead2_production", 1, 1),
        La2Gold = ("lookahead2_gold", 1, 1),
        La2Food = ("lookahead2_food", 1, 1),
        La2Culture = ("lookahead2_culture", 1, 1),
        La2SciencePercent = ("lookahead2_science_percent", 1, 1),
        La2ProductionPercent = ("lookahead2_production_percent", 1, 1),
        La2GoldPercent = ("lookahead2_gold_percent", 1, 1),
        La2FoodPercent = ("lookahead2_food_percent", 1, 1),
        La2CulturePercent = ("lookahead2_culture_percent", 1, 1),
        La2BestUnitStrength = ("lookahead2_best_unit_strength", 1, 1),
        La3Buildings = ("lookahead3_buildings", 1, 1),
        La3WorldWonders = ("lookahead3_world_wonders", 1, 1),
        La3Units = ("lookahead3_units", 1, 1),
        La3Improvements = ("lookahead3_improvements", 1, 1),
        La3Projects = ("lookahead3_projects", 1, 1),
        La3Science = ("lookahead3_science", 1, 1),
        La3Production = ("lookahead3_production", 1, 1),
        La3Gold = ("lookahead3_gold", 1, 1),
        La3Food = ("lookahead3_food", 1, 1),
        La3Culture = ("lookahead3_culture", 1, 1),
        La3SciencePercent = ("lookahead3_science_percent", 1, 1),
        La3ProductionPercent = ("lookahead3_production_percent", 1, 1),
        La3GoldPercent = ("lookahead3_gold_percent", 1, 1),
        La3FoodPercent = ("lookahead3_food_percent", 1, 1),
        La3CulturePercent = ("lookahead3_culture_percent", 1, 1),
        La3BestUnitStrength = ("lookahead3_best_unit_strength", 1, 1),
        TechsToProject = ("techs_to_project", 1, 1),
        TechsToLastEra = ("techs_to_last_era", 1, 1),
        ItemUnit = ("item_unit", 1, 1),
        ItemBuilding = ("item_building", 1, 1),
        ItemWorldWonder = ("item_world_wonder", 1, 1),
        ItemNationalWonder = ("item_national_wonder", 1, 1),
        ItemProject = ("item_project", 1, 1),
        ItemCost = ("item_cost", 1, 10),
        ItemTurns = ("item_turns", 1, 1),
        ItemMaintenance = ("item_maintenance", 1, 1),
        ItemQueued = ("item_queued", 1, 1),
        ItemStrength = ("item_strength", 1, 1),
        ItemRanged = ("item_ranged_strength", 1, 1),
        ItemMovement = ("item_movement", 1, 1),
        ItemDomainSea = ("item_domain_sea", 1, 1),
        ItemDomainAir = ("item_domain_air", 1, 1),
        ItemFounder = ("item_founder", 1, 1),
        ItemBuilder = ("item_builder", 1, 1),
        ItemEra = ("item_era", 1, 1),
        PolAdopt = ("policy_adopt", 1, 1),
        PolBranch = ("policy_unlock_branch", 1, 1),
        PolIdeology = ("policy_ideology", 1, 1),
        PolBranchSize = ("policy_branch_size", 1, 1),
        PolBranchAdopted = ("policy_branch_adopted", 1, 1),
        PolFreeTenets = ("policy_free_tenets", 1, 1),
        PolBranchEra = ("policy_branch_era", 1, 1),
        EffFood = ("effect_food", 1, 1),
        EffProduction = ("effect_production", 1, 1),
        EffGold = ("effect_gold", 1, 1),
        EffScience = ("effect_science", 1, 1),
        EffCulture = ("effect_culture", 1, 1),
        EffFaith = ("effect_faith", 1, 1),
        EffFoodPct = ("effect_food_percent", 1, 1),
        EffProductionPct = ("effect_production_percent", 1, 1),
        EffGoldPct = ("effect_gold_percent", 1, 1),
        EffSciencePct = ("effect_science_percent", 1, 1),
        EffCulturePct = ("effect_culture_percent", 1, 1),
        EffFaithPct = ("effect_faith_percent", 1, 1),
        EffFoodPop = ("effect_food_per_citizen", 100, 1),
        EffProductionPop = ("effect_production_per_citizen", 100, 1),
        EffGoldPop = ("effect_gold_per_citizen", 100, 1),
        EffSciencePop = ("effect_science_per_citizen", 100, 1),
        EffCulturePop = ("effect_culture_per_citizen", 100, 1),
        EffFaithPop = ("effect_faith_per_citizen", 100, 1),
        EffHappiness = ("effect_happiness", 1, 1),
        BelPantheon = ("belief_pantheon", 1, 1),
        BelFounder = ("belief_founder", 1, 1),
        BelFollower = ("belief_follower", 1, 1),
        BelEnhancer = ("belief_enhancer", 1, 1),
        BelReformation = ("belief_reformation", 1, 1),
        CondFood = ("conditional_food", 1, 1),
        CondProduction = ("conditional_production", 1, 1),
        CondGold = ("conditional_gold", 1, 1),
        CondScience = ("conditional_science", 1, 1),
        CondCulture = ("conditional_culture", 1, 1),
        CondFaith = ("conditional_faith", 1, 1),
        CondHappiness = ("conditional_happiness", 1, 1),
        Conditional = ("conditional_effects", 1, 1),
        Triggered = ("triggered_effects", 1, 1),
        TilesMeeting = ("worked_tiles_meeting_conditions", 1, 1),
        TileYield = ("yield_on_worked_tiles_meeting", 1, 1),
        CitiesMeeting = ("own_cities_meeting_conditions", 1, 1),
        FaithPrice = ("faith_price", 1, 10),
        FaithSave = ("faith_save", 1, 1),
        // Format 6: other own cities with the item queued or ordered this turn.
        ItemQueuedElsewhere = ("item_queued_other_cities", 1, 1),
        // Format 6: other own units marching to the unit candidate's target.
        TargetClaimed = ("target_claimed_by_own", 1, 1),
        // Format 8: the blow's outcome as the rules' preview gives it.
        BlowKills = ("blow_kills", 1, 1),
        BlowCaptures = ("blow_captures", 1, 1),
        ToDefenderUncapped = ("preview_damage_to_defender_uncapped", 1, 1),
        CityFloor = ("preview_city_floor", 1, 1),
        // Format 10: the strike from a move's destination (`MoveStrike`),
        // on the move candidates of a unit that fights; 0 elsewhere.
        MoveBlowCaptures = ("move_blow_captures", 1, 1),
        MoveBlowKills = ("move_blow_kills", 1, 1),
        MoveBlowDamage = ("move_blow_damage_percent", 1, 1),
        MoveCityDamage = ("move_city_damage_percent", 1, 1),
        MoveCityAdjacent = ("move_city_adjacent", 1, 1),
        MoveCityInStrike = ("move_city_in_strike", 1, 1),
        MoveTargets = ("move_strike_targets", 1, 1),
        MoveEnemyDistance = ("move_enemy_distance", 1, 1),
        MoveWoundedDistance = ("move_wounded_enemy_distance", 1, 1),
        MoveWoundedHp = ("move_wounded_enemy_hp_percent", 1, 1),
        MoveNextTargets = ("move_next_turn_targets", 1, 1),
        MoveNextKills = ("move_next_turn_kills", 1, 1),
        MoveCityDistance = ("move_city_distance", 1, 1),
        MoveUnseenCityInReach = ("move_unseen_city_in_reach", 1, 1),
    }
}

columns! {
    /// Format 10, training only: per unit candidate, the team's quantities a
    /// single unit's preview does not give (`candidate_targets.npy`,
    /// `unit_candidates_with_targets`); targets of an auxiliary head, never
    /// an input of the playing net.
    A {
        FocusDamage = ("focus_damage_percent", 1, 1),
        FocusTakes = ("focus_takes", 1, 1),
        DangerDamage = ("danger_damage", 1, 1),
        DangerDies = ("danger_dies", 1, 1),
    }
}

/// The candidate columns of a format-9 file: the format-10 columns
/// (`C::MoveBlowCaptures` on) are appended after them, so a format-9 net
/// reads the first `FORMAT_9_CANDIDATE_COLUMNS` of every row
/// (`mind::layout_text_of`).
pub const FORMAT_9_CANDIDATE_COLUMNS: usize = C::MoveBlowCaptures as usize;

columns! {
    /// The event row (`events.npy`, format 7): one event of the player's
    /// last rounds (`events.rs`). The kind and the content are ids the net
    /// embeds; the actor's relation is the one the player has now.
    V {
        KindId = ("event_kind_id_plus_1", 1, 1),
        ParamId = ("event_param_id_plus_1", 1, 1),
        Age = ("age", 1, 1),
        ActorOwn = ("actor_rel_own", 1, 1),
        ActorFriend = ("actor_rel_friend", 1, 1),
        ActorNeutral = ("actor_rel_neutral", 1, 1),
        ActorWar = ("actor_rel_war", 1, 1),
        ActorCityState = ("actor_rel_city_state", 1, 1),
        ActorBarbarian = ("actor_rel_barbarian", 1, 1),
        ActorUnmet = ("actor_rel_unmet", 1, 1),
        HasTile = ("has_tile", 1, 1),
        Visible = ("visible", 1, 1),
        CapitalDistance = ("capital_distance", 1, 1),
        CityDistance = ("own_city_distance", 1, 1),
        ItemEra = ("item_era", 1, 1),
        ItemCost = ("item_cost", 1, 10),
        Value = ("value", 1, 1),
        Count = ("count", 1, 1),
        Ours = ("ours", 1, 1),
    }
}

/// Per table the list of `{name, scale}` (`meta.json`).
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub tile: Vec<Field>,
    pub global: Vec<Field>,
    pub entity: Vec<Field>,
    pub candidate: Vec<Field>,
    /// Format 7: the event rows.
    pub event: Vec<Field>,
}

fn fields(table: &[(&str, Scale)]) -> Vec<Field> {
    table.iter().map(|(name, scale)| Field { name: (*name).to_owned(), scale: *scale }).collect()
}

/// The layout of the rules: the global row ends with a multi-hot of every
/// technology (`tech_<id>`), policy (`policy_<id>`) and branch
/// (`branch_<id>`) of the rules.
pub fn layout(rules: &CompiledRuleset) -> Layout {
    let one = Scale { num: 1, den: 1 };
    let mut global = fields(G::FIELDS);
    let named = |prefix: &str, n: usize| (0..n).map(move |i| Field { name: format!("{prefix}_{i}"), scale: one }).collect::<Vec<_>>();
    global.extend(named("tech", rules.technologies.len()));
    global.extend(named("policy", rules.module.policies.len()));
    global.extend(named("branch", rules.module.policy_branches.len()));
    Layout { tile: fields(T::FIELDS), global, entity: fields(E::FIELDS), candidate: fields(C::FIELDS), event: fields(V::FIELDS) }
}

/// The training-only candidate targets' table (`A`, format 10;
/// `candidate_targets.npy`, meta.json `layout.candidate_targets`).
pub fn target_fields() -> Vec<Field> {
    fields(A::FIELDS)
}

/// The width of the global row of the rules.
pub fn global_width(rules: &CompiledRuleset) -> usize {
    G::COUNT + rules.technologies.len() + rules.module.policies.len() + rules.module.policy_branches.len()
}

fn sat(v: i64) -> i16 {
    v.clamp(i64::from(i16::MIN), i64::from(i16::MAX)) as i16
}

macro_rules! set {
    ($row:expr, $col:expr, $v:expr) => {
        $row[$col as usize] = sat(($v) as i64)
    };
}

fn clip(d: Option<i32>) -> i32 {
    d.map_or(CLIP, |d| d.clamp(0, CLIP))
}

/// The six neighbours of every tile in the kernel's direction order
/// (`math::HEX_DIRS`), `-1` off the map.
pub fn neighbor_table(dims: MapDimensions) -> Vec<[i32; 6]> {
    (0..dims.tile_count() as u32)
        .map(|n| {
            let mut out = [-1; 6];
            if let Some(c) = dims.to_coord(TileId(n)) {
                for (i, d) in HEX_DIRS.iter().enumerate() {
                    let next = crate::math::HexCoord { q: c.q + d.q, r: c.r + d.r };
                    out[i] = dims.to_tile(next).map_or(-1, |t| t.0 as i32);
                }
            }
            out
        })
        .collect()
}

/// The region of every tile: blocks of `REGION` × `REGION` tiles of the
/// map's storage grid (`q = id % width`, `r = id / width`), numbered row by
/// row: `(r / 4) * ceil(width / 4) + q / 4`. Also the region count.
pub fn region_table(dims: MapDimensions) -> (Vec<i32>, usize) {
    let across = dims.width.div_ceil(REGION);
    let down = dims.height.div_ceil(REGION);
    let regions = (0..dims.tile_count() as u32).map(|n| ((n / dims.width / REGION) * across + (n % dims.width) / REGION) as i32).collect();
    (regions, (across * down) as usize)
}

/// The relation code of `other` for `player` (`relation`).
pub fn relation_code(player: PlayerId, relations: &[Relation], other: PlayerId) -> i32 {
    if other == player {
        return relation::OWN;
    }
    let Some(r) = relations.iter().find(|r| r.other == other) else {
        return relation::UNMET;
    };
    match r.kind {
        PlayerKind::Module(ClassicKind::Barbarian) => relation::BARBARIAN,
        _ if r.at_war => relation::WAR,
        PlayerKind::Module(ClassicKind::CityState(_)) => relation::CITY_STATE,
        _ if r.friends || r.defensive_pact => relation::FRIEND,
        _ => relation::NEUTRAL,
    }
}

/// The military strength of a unit as the rules' tables give it: its best
/// strength scaled by its health.
fn unit_strength(rules: &CompiledRuleset, kind: UnitKindId, hp: u16) -> i64 {
    if rules.is_civilian(kind) {
        return 0;
    }
    let Some(def) = rules.units.get(usize::from(kind.0)) else { return 0 };
    i64::from(def.strength.max(def.ranged_strength)) * i64::from(hp) / i64::from(crate::world::unit::UNIT_MAX_HP)
}

fn era_index(rules: &CompiledRuleset, era: Option<crate::rules::content::EraId>) -> i64 {
    era.and_then(|e| rules.eras.get(usize::from(e.0))).map_or(0, |e| i64::from(e.index) + 1)
}

/// The yield slot of a yield kind (`Output` counts as production).
fn slot(kind: YieldKind) -> usize {
    match kind {
        YieldKind::Food => 0,
        YieldKind::Production | YieldKind::Output => 1,
        YieldKind::Gold => 2,
        YieldKind::Science => 3,
        YieldKind::Culture => 4,
        YieldKind::Module(crate::classic::effects::ClassicYield::Faith) => 5,
    }
}

/// The amounts of the plain effects (always, passive) of a table row: flat
/// yields, yield percents, yields per citizen (hundredths), happiness.
fn amounts(set: &EffectSet) -> [i64; 19] {
    let mut out = [0; 19];
    let effects = set.passive_player.iter().chain(&set.passive_city).chain(&set.passive_unit).chain(&set.passive_tile);
    for effect in effects.filter(|e| matches!(e.condition, Condition::Always) && matches!(e.trigger, Trigger::Passive)) {
        match effect.modifier {
            Modifier::AddYield { yield_kind, amount } => out[slot(yield_kind)] += i64::from(amount),
            Modifier::AddYieldPercent { yield_kind, percent } => out[6 + slot(yield_kind)] += i64::from(percent),
            Modifier::AddYieldPerPopulation { yield_kind, hundredths } => out[12 + slot(yield_kind)] += i64::from(hundredths),
            Modifier::Module(crate::classic::effects::ClassicModifier::AddHappiness { amount }) => out[18] += i64::from(amount),
            _ => {}
        }
    }
    out
}

fn put_amounts(row: &mut [i16], set: &EffectSet) {
    for (i, v) in amounts(set).into_iter().enumerate() {
        row[C::EffFood as usize + i] = sat(v);
    }
}

fn put_yields(row: &mut [i16], at: usize, y: Yields) {
    for (i, v) in [y.food, y.production, y.gold, y.science, y.culture, y.module].into_iter().enumerate() {
        row[at + i] = sat(i64::from(v));
    }
}

/// A key of a handle (index, generation).
fn unit_key(h: UnitHandle) -> (u32, u32) {
    (h.index, h.generation)
}

/// One player's turn as the net reads it: rows of the turn's start, and
/// the entities named later in the turn appended.
#[derive(Debug, Clone)]
pub struct Turn {
    pub player: PlayerId,
    pub round: u32,
    pub dims: MapDimensions,
    /// `tiles × T::COUNT`, tile after tile.
    pub tiles: Vec<i16>,
    /// `global_width` values.
    pub global: Vec<i16>,
    /// `entity_meta.len() × E::COUNT`.
    pub entities: Vec<i16>,
    /// Kind, tile (`-1` none), owner relation (`-1` none), persistent id
    /// (`entity_id`).
    pub entity_meta: Vec<[i32; 4]>,
    /// Format 7: `event_meta.len() × V::COUNT`, the events of the player's
    /// last rounds (`put_events`; empty until then).
    pub events: Vec<i16>,
    /// Per event: kind, tile (`-1` none), the actor's entity row of the
    /// turn (`-1` none), parameter (`-1` none).
    pub event_meta: Vec<[i32; 4]>,
    /// Relation code per player id.
    relations: Vec<i32>,
    capital: Option<TileId>,
    own_units: BTreeMap<u64, i32>,
    own_cities: BTreeMap<u64, i32>,
    own_city_handles: BTreeMap<(u32, u32), i32>,
    foreign_units: BTreeMap<(u32, u32), i32>,
    foreign_cities: BTreeMap<u32, i32>,
    /// Production decisions of the turn so far, per own city (`note_order`).
    ordered: BTreeMap<(u32, u32), Vec<ProductionItem>>,
    /// The own units' march orders at the turn's start (`OwnUnit::goal`).
    marches: BTreeMap<u64, TileId>,
    /// The entity rows of the turn's start (`encode`); the rows after them
    /// were appended as the turn named them.
    start_entities: usize,
}

impl Turn {
    /// A production decision of the turn: `city` was ordered `item`. The
    /// later production candidates of the turn count it as queued there
    /// (`item_queued_other_cities`) even when the observation they are made
    /// from predates the order (the net player decides a phase's cities on
    /// one observation; the exporter sees each order applied).
    pub fn note_order(&mut self, city: CityHandle, item: ProductionItem) {
        let items = self.ordered.entry((city.index, city.generation)).or_default();
        if !items.contains(&item) {
            items.push(item);
        }
    }

    /// The rows of `obs`'s player at the start of its turn.
    pub fn encode(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Self {
        let dims = obs.view.dimensions();
        let players = obs.relations.iter().map(|r| usize::from(r.other.0) + 1).max().unwrap_or(0).max(usize::from(obs.player.0) + 1);
        let relations = (0..players.max(64)).map(|p| relation_code(obs.player, &obs.relations, PlayerId(p as u8))).collect();
        let capital = obs.view.own_capital().and_then(|h| obs.own_cities.iter().find(|c| c.handle == h)).map(|c| c.tile);
        let mut turn = Self {
            player: obs.player,
            round: obs.round,
            dims,
            tiles: Vec::new(),
            global: vec![0; global_width(rules)],
            entities: Vec::new(),
            entity_meta: Vec::new(),
            events: Vec::new(),
            event_meta: Vec::new(),
            relations,
            capital,
            own_units: BTreeMap::new(),
            own_cities: BTreeMap::new(),
            own_city_handles: BTreeMap::new(),
            foreign_units: BTreeMap::new(),
            foreign_cities: BTreeMap::new(),
            ordered: BTreeMap::new(),
            marches: obs.own_units.iter().filter_map(|u| u.goal.map(|g| (u.id.0, g))).collect(),
            start_entities: 0,
        };
        turn.tiles = tile_rows(obs, rules, &turn);
        turn.global_row(obs, rules);
        for unit in &obs.own_units {
            turn.own_unit_row(unit, rules);
        }
        for city in &obs.own_cities {
            turn.own_city_row(city, obs, rules);
        }
        for unit in &obs.visible_foreign_units {
            turn.foreign_unit_row(unit, rules);
        }
        for city in &obs.visible_foreign_cities {
            let row = turn.foreign_city_row(city.tile, city.owner, city.population, city.hp, false);
            turn.put_city_religion(row, &obs.faith, city.tile);
        }
        let remembered: Vec<(TileId, PlayerId, u32)> = obs
            .view
            .remembered_cities()
            .map(|(tile, s)| (tile, s.owner, s.seen.as_ref().map_or(0, |seen| seen.population)))
            .collect();
        for (tile, owner, population) in remembered {
            if owner != obs.player {
                turn.foreign_city_row(tile, owner, population, 0, true);
            }
        }
        for info in &obs.city_states {
            turn.city_state_row(info);
        }
        let race = obs.view.race();
        for r in obs.relations.iter().filter(|r| r.kind == PlayerKind::Major) {
            turn.player_row(r, &race);
        }
        turn.start_entities = turn.entity_count();
        turn
    }

    /// The rows of `obs`'s player at the start of its turn with its events:
    /// `tracker` observes the player first (`events::Tracker::observe`), its
    /// log becomes the turn's event rows. The exporter, the E2 rebuild and
    /// the net player all build a turn this way.
    pub fn encode_tracked(obs: &AIObservation<'_>, rules: &CompiledRuleset, tracker: &mut super::events::Tracker) -> Self {
        tracker.observe(obs, rules);
        let mut turn = Self::encode(obs, rules);
        turn.put_events(tracker.events(), obs, rules);
        turn
    }

    pub fn entity_count(&self) -> usize {
        self.entity_meta.len()
    }

    /// The entity rows the turn had at its start, before any row the turn
    /// appended: what the playing net reads (`Model::begin_turn` encodes
    /// the turn once, so a candidate's target row appended later reaches it
    /// as no entity). Format 8 writes it with the turn (`turns.npy`) so the
    /// trainer reads the turn as the player did.
    pub fn start_entity_count(&self) -> usize {
        self.start_entities
    }

    pub fn event_count(&self) -> usize {
        self.event_meta.len()
    }

    /// The event rows of `events` (format 7; after the entity rows of the
    /// turn's start, which an event's actor points to): the kind and the
    /// content as ids (`event_param_id_plus_1` = 1 + kind × 1024 + the
    /// content id, so each kind has its own ids), the age in rounds, the
    /// actor's relation now (none: all zero), the place (in sight now, the
    /// distances to the capital and to the nearest own city, 32 none), the
    /// item's era and cost, the amount, the count, whether it touched the
    /// player's own plans.
    pub fn put_events(&mut self, events: &[super::events::Event], obs: &AIObservation<'_>, _rules: &CompiledRuleset) {
        let cities: Vec<TileId> = obs.own_cities.iter().map(|c| c.tile).collect();
        for e in events {
            let actor_row = e.actor.and_then(|p| self.entity_meta.iter().position(|m| (m[0] == kind::PLAYER || m[0] == kind::CITY_STATE) && m[3] == i32::from(p.0)));
            self.event_meta.push([i32::from(e.kind), e.tile.map_or(-1, |t| t.0 as i32), actor_row.map_or(-1, |r| r as i32), e.param]);
            let mut r = [0i16; V::COUNT];
            set!(r, V::KindId, i64::from(e.kind) + 1);
            if e.param >= 0 {
                set!(r, V::ParamId, 1 + i64::from(e.kind) * 1024 + i64::from(e.param.min(1023)));
            }
            set!(r, V::Age, self.round.saturating_sub(e.round));
            if let Some(p) = e.actor {
                let rel = self.relation(p);
                if (0..relation::COUNT as i32).contains(&rel) {
                    r[V::ActorOwn as usize + rel as usize] = 1;
                }
            }
            if let Some(tile) = e.tile {
                set!(r, V::HasTile, 1);
                set!(r, V::Visible, obs.view.is_visible(tile));
                set!(r, V::CapitalDistance, self.capital_distance(tile));
                set!(r, V::CityDistance, clip(cities.iter().filter_map(|c| self.dims.distance(*c, tile)).min()));
            } else {
                set!(r, V::CapitalDistance, CLIP);
                set!(r, V::CityDistance, CLIP);
            }
            set!(r, V::ItemEra, e.item_era);
            set!(r, V::ItemCost, e.item_cost / 10);
            set!(r, V::Value, e.value);
            set!(r, V::Count, e.count);
            set!(r, V::Ours, e.ours);
            self.events.extend_from_slice(&r);
        }
    }

    fn relation(&self, other: PlayerId) -> i32 {
        self.relations.get(usize::from(other.0)).copied().unwrap_or(relation::UNMET)
    }

    fn capital_distance(&self, tile: TileId) -> i32 {
        clip(self.capital.and_then(|c| self.dims.distance(c, tile)))
    }

    fn push(&mut self, kind_code: i32, tile: Option<TileId>, owner: i32, id: i64) -> (usize, i32) {
        let at = self.entity_meta.len();
        let id = i32::try_from(id).unwrap_or(i32::MAX);
        self.entity_meta.push([kind_code, tile.map_or(-1, |t| t.0 as i32), owner, id]);
        self.entities.extend(std::iter::repeat_n(0, E::COUNT));
        let row = &mut self.entities[at * E::COUNT..];
        row[E::KindOwnUnit as usize + kind_code as usize] = 1;
        if owner >= 0 {
            row[E::RelOwn as usize + owner as usize] = 1;
        }
        if let Some(tile) = tile {
            let d = self.capital_distance(tile);
            self.entities[at * E::COUNT + E::CapitalDistance as usize] = sat(i64::from(d));
        }
        (at, at as i32)
    }

    fn row_mut(&mut self, at: usize) -> &mut [i16] {
        &mut self.entities[at * E::COUNT..(at + 1) * E::COUNT]
    }

    /// The row of one of the player's units, appended when the turn has none.
    pub fn own_unit_row(&mut self, unit: &OwnUnit, rules: &CompiledRuleset) -> i32 {
        if let Some(&row) = self.own_units.get(&unit.id.0) {
            return row;
        }
        let (at, row) = self.push(kind::OWN_UNIT, Some(unit.tile), relation::OWN, unit.id.0 as i64);
        let goal = unit.goal.and_then(|g| self.dims.distance(unit.tile, g));
        let r = self.row_mut(at);
        unit_kind_fields(r, rules, unit.kind);
        set!(r, E::Hp, unit.hp);
        set!(r, E::MovesLeft, unit.moves_left / 6);
        set!(r, E::MovesMax, unit.moves_max / 6);
        set!(r, E::Fortified, unit.fortified);
        set!(r, E::CanFortify, unit.can_fortify);
        set!(r, E::CanAttack, unit.can_attack);
        set!(r, E::Experience, unit.experience);
        set!(r, E::Level, unit.level);
        set!(r, E::Promotions, unit.promotions.iter().count());
        set!(r, E::AttacksMade, unit.attacks_made);
        set!(r, E::HasGoal, unit.goal.is_some());
        set!(r, E::GoalDistance, if unit.goal.is_some() { clip(goal) } else { 0 });
        set!(r, E::Working, unit.work.is_some());
        self.own_units.insert(unit.id.0, row);
        row
    }

    /// The row of one of the player's cities, appended when the turn has none.
    pub fn own_city_row(&mut self, city: &OwnCity, obs: &AIObservation<'_>, rules: &CompiledRuleset) -> i32 {
        if let Some(&row) = self.own_cities.get(&city.id.0) {
            return row;
        }
        let (at, row) = self.push(kind::OWN_CITY, Some(city.tile), relation::OWN, city.id.0 as i64);
        let capital = obs.view.own_capital() == Some(city.handle);
        let r = self.row_mut(at);
        set!(r, E::Population, city.population);
        set!(r, E::FoodStock, city.food);
        set!(r, E::ProductionStock, city.production);
        set!(r, E::CityHp, city.hp);
        set!(r, E::CityDamage, city.damage);
        set!(r, E::Buildings, city.buildings.len());
        let wonder = |b: &&BuildingKindId| rules.buildings.get(usize::from(b.0)).is_some_and(|d| d.wonder);
        set!(r, E::Wonders, city.buildings.iter().filter(wonder).count());
        set!(r, E::WorkedTiles, city.worked.len());
        set!(r, E::FoodSurplus, city.income.food_surplus);
        set!(r, E::CityProduction, city.income.production);
        set!(r, E::CityGold, city.income.gold);
        set!(r, E::CityScience, city.income.science);
        set!(r, E::CityCulture, city.income.culture);
        set!(r, E::CityFaith, city.income.faith);
        set!(r, E::Capital, capital);
        set!(r, E::NotOwnStatus, city.status != crate::world::city::CityStatus::Own);
        set!(r, E::QueueLength, city.queue.len());
        match city.queue.first() {
            Some(ProductionItem::Unit(_)) => set!(r, E::QueueUnit, 1),
            Some(ProductionItem::Building(_)) => set!(r, E::QueueBuilding, 1),
            Some(ProductionItem::Project(_)) => set!(r, E::QueueProject, 1),
            None => {}
        }
        self.put_city_religion(row, &obs.faith, city.tile);
        self.own_cities.insert(city.id.0, row);
        self.own_city_handles.insert((city.handle.index, city.handle.generation), row);
        row
    }

    /// The row of a foreign unit in sight, appended when the turn has none.
    pub fn foreign_unit_row(&mut self, unit: &ObservedUnit, rules: &CompiledRuleset) -> i32 {
        if let Some(&row) = self.foreign_units.get(&unit_key(unit.handle)) {
            return row;
        }
        let owner = self.relation(unit.owner);
        let (at, row) = self.push(kind::FOREIGN_UNIT, Some(unit.tile), owner, -1);
        let r = self.row_mut(at);
        unit_kind_fields(r, rules, unit.kind);
        set!(r, E::Hp, unit.hp);
        self.foreign_units.insert(unit_key(unit.handle), row);
        row
    }

    fn foreign_city_row(&mut self, tile: TileId, owner: PlayerId, population: u32, hp: u16, remembered: bool) -> i32 {
        if let Some(&row) = self.foreign_cities.get(&tile.0) {
            return row;
        }
        let relation = self.relation(owner);
        let (at, row) = self.push(kind::FOREIGN_CITY, Some(tile), relation, i64::from(tile.0));
        let r = self.row_mut(at);
        set!(r, E::Population, population);
        set!(r, E::CityHp, hp);
        set!(r, E::Remembered, remembered);
        self.foreign_cities.insert(tile.0, row);
        row
    }

    /// A city row's religion as the observation shows it (the city's
    /// religion panel: own cities and the foreign cities in sight; nothing
    /// for another city): the majority's relation (none — no majority or
    /// the pantheon —, the player's religion, another religion), the
    /// followers of the player's religion and of the majority religion, the
    /// pressure of the player's religion and of the strongest other religion
    /// (in tenths of a citizen's 1 000), and whether it is the holy city of
    /// the player's religion or of another.
    fn put_city_religion(&mut self, row: i32, faith: &FaithObservation, tile: TileId) {
        use crate::world::religion::Creed;
        let Some(city) = faith.cities.iter().find(|c| c.tile == tile) else { return };
        let ours = faith.overview.religion;
        let majority = city.majority.and_then(|c| match c {
            Creed::Religion(r) => Some(r),
            _ => None,
        });
        let followers = crate::derived::religion::followers(&city.faith, city.population);
        let followers_of = |r: Option<crate::rules::content::ReligionId>| r.map_or(0, |r| followers.iter().find(|(c, _)| *c == Creed::Religion(r)).map_or(0, |(_, n)| *n));
        let own_pressure = ours.map_or(0, |r| city.faith.pressure_of(Creed::Religion(r)));
        let other_pressure = city.faith.pressure.iter().filter(|p| matches!(p.creed, Creed::Religion(r) if Some(r) != ours)).map(|p| p.amount).max().unwrap_or(0);
        let holy = faith.overview.religions.iter().find(|r| r.holy_city == city.handle).map(|r| r.religion);
        let r = self.row_mut(row as usize);
        match majority {
            None => set!(r, E::MajorityNone, 1),
            Some(m) if Some(m) == ours => set!(r, E::MajorityOwn, 1),
            Some(_) => set!(r, E::MajorityOther, 1),
        }
        set!(r, E::OwnFollowers, followers_of(ours));
        set!(r, E::MajorityFollowers, followers_of(majority));
        set!(r, E::OwnPressure, own_pressure / 100);
        set!(r, E::OtherPressure, other_pressure / 100);
        set!(r, E::HolyOwn, holy.is_some() && holy == ours);
        set!(r, E::HolyOther, holy.is_some() && holy != ours);
    }

    /// The row of a foreign city in sight (by its tile), appended when the
    /// turn has none.
    pub fn observed_city_row(&mut self, city: &ObservedCity) -> i32 {
        self.foreign_city_row(city.tile, city.owner, city.population, city.hp, false)
    }

    fn city_state_row(&mut self, info: &crate::knowledge::observation::CityStateInfo) {
        let relation = self.relation(info.player);
        let (at, _) = self.push(kind::CITY_STATE, info.capital, relation, i64::from(info.player.0));
        let me = self.player;
        let r = self.row_mut(at);
        set!(r, E::Influence, info.influence);
        set!(r, E::Resting, info.resting);
        set!(r, E::Friends, info.friends);
        set!(r, E::AllyUs, info.ally == Some(me));
        set!(r, E::AllyOther, info.ally.is_some_and(|a| a != me));
        set!(r, E::AtWar, info.at_war);
        set!(r, E::Pledged, info.pledged);
        set!(r, E::CanBully, info.can_bully_gold);
        set!(r, E::Quests, info.quests.len());
        set!(r, E::CanBuyout, info.can_buyout);
    }

    fn player_row(&mut self, rel: &Relation, race: &crate::knowledge::public::Race) {
        let relation = self.relation(rel.other);
        let (at, _) = self.push(kind::PLAYER, None, relation, i64::from(rel.other.0));
        let round = i64::from(self.round);
        let own = race.own_score().max(1);
        let theirs = race.scores.iter().find(|(p, _)| *p == rel.other).map(|(_, s)| *s);
        let r = self.row_mut(at);
        set!(r, E::AtWar, rel.at_war);
        set!(r, E::Friends, rel.friends);
        set!(r, E::DefensivePact, rel.defensive_pact);
        set!(r, E::OurEmbassy, rel.our_embassy);
        set!(r, E::TheirEmbassy, rel.their_embassy);
        set!(r, E::WeDenounced, rel.we_denounced);
        set!(r, E::TheyDenounced, rel.they_denounced);
        set!(r, E::Warmonger, rel.warmonger);
        set!(r, E::PeaceRounds, (i64::from(rel.peace_treaty_until) - round).max(0));
        set!(r, E::WarRounds, if rel.at_war { (round - i64::from(rel.war_since)).max(0) } else { 0 });
        set!(r, E::TheyGrantEntry, rel.they_grant_entry);
        set!(r, E::WeGrantEntry, rel.we_grant_entry);
        set!(r, E::ResearchAgreement, rel.research_agreement);
        set!(r, E::PactsKnown, rel.pacts_known);
        set!(r, E::ScorePermille, theirs.map_or(0, |s| s * 1000 / own));
        set!(r, E::MetRounds, rel.contact.met_round.map_or(0, |m| (round - i64::from(m)).max(0)));
    }

    fn global_row(&mut self, obs: &AIObservation<'_>, rules: &CompiledRuleset) {
        let e = &obs.economy;
        let c = &obs.culture;
        let tiles = obs.view.tile_count().max(1) as i64;
        let known = (0..tiles as u32).filter(|t| obs.view.is_known(TileId(*t))).count() as i64;
        let visible = (0..tiles as u32).filter(|t| obs.view.is_visible(TileId(*t))).count() as i64;
        let race = obs.view.race();
        let met = |kind: fn(&PlayerKind) -> bool| obs.relations.iter().filter(|r| kind(&r.kind)).count();
        let options = obs.victories;
        let enabled = |k| options.enabled(k);
        use crate::world::victory::VictoryKind as V;
        let g = &mut self.global;
        set!(g, G::Round, obs.round);
        set!(g, G::RoundsLeft, (i64::from(options.max_rounds) - i64::from(obs.round)).max(0));
        set!(g, G::Era, obs.view.own_era());
        set!(g, G::Treasury, e.treasury / 10);
        set!(g, G::GoldIncome, e.income.gold);
        set!(g, G::ScienceIncome, e.income.science);
        set!(g, G::CultureIncome, e.income.culture);
        set!(g, G::FaithIncome, e.income.faith);
        set!(g, G::CulturePool, c.culture / 10);
        set!(g, G::PolicyCost, c.next_policy_cost / 10);
        set!(g, G::FreePolicies, c.free_policies);
        set!(g, G::FreeTenets, c.free_tenets);
        set!(g, G::FaithPool, obs.faith.overview.faith / 10);
        set!(g, G::Happiness, e.happiness.excess());
        set!(g, G::TurnHappiness, e.turn_happiness);
        set!(g, G::GoldenAgeThreshold, e.golden_age_threshold / 10);
        set!(g, G::Cities, obs.own_cities.len());
        set!(g, G::Citizens, obs.own_cities.iter().map(|c| i64::from(c.population)).sum::<i64>());
        set!(g, G::Units, obs.own_units.len());
        set!(g, G::MilitaryUnits, obs.own_units.iter().filter(|u| !rules.is_civilian(u.kind)).count());
        set!(g, G::Techs, e.techs.len());
        set!(g, G::Policies, c.policies.len());
        set!(g, G::ResearchQueue, e.research_queue.len());
        set!(g, G::KnownPermille, known * 1000 / tiles);
        set!(g, G::VisiblePermille, visible * 1000 / tiles);
        set!(g, G::MapTiles, tiles / 10);
        set!(g, G::MajorsMet, met(|k| *k == PlayerKind::Major));
        set!(g, G::CityStatesMet, met(|k| matches!(k, PlayerKind::Module(ClassicKind::CityState(_)))));
        set!(g, G::Wars, obs.relations.iter().filter(|r| r.at_war && r.kind == PlayerKind::Major).count());
        set!(g, G::Allies, obs.city_states.iter().filter(|c| c.ally == Some(obs.player)).count());
        set!(g, G::TradeRoutesLeft, e.trade_routes_left);
        set!(g, G::OwnScore, race.own_score() / 10);
        set!(g, G::BestRivalScore, race.best_rival() / 10);
        set!(g, G::WorldWonders, obs.built_wonders.len());
        set!(g, G::FreeTechOwed, !e.free_technology_options.is_empty());
        set!(g, G::HasCapital, self.capital.is_some());
        set!(g, G::HasPantheon, obs.faith.overview.pantheon.is_some());
        set!(g, G::HasReligion, obs.faith.overview.religion.is_some());
        set!(g, G::HasIdeology, c.ideology.is_some());
        set!(g, G::VictoryScience, enabled(V::Science));
        set!(g, G::VictoryDomination, enabled(V::Domination));
        set!(g, G::VictoryCultural, enabled(V::Cultural));
        set!(g, G::VictoryDiplomatic, enabled(V::Diplomatic));
        let faith = &obs.faith.overview;
        set!(g, G::ReligionsFounded, faith.religions.len());
        set!(g, G::ReligionsLeft, faith.religions_left);
        let ours = faith.religion.map(crate::world::religion::Creed::Religion);
        let following = obs.faith.cities.iter().filter(|c| c.owner == obs.player && ours.is_some() && c.majority == ours).count();
        set!(g, G::CitiesFollowing, following);
        set!(g, G::PantheonCost, faith.pantheon_cost / 10);
        set!(g, G::ProphetCost, faith.prophet_cost / 10);
        set!(g, G::ReligionEnhanced, faith.religions.iter().any(|r| Some(r.religion) == faith.religion && r.enhanced));
        let demand = Demand::of(obs, rules);
        set!(g, G::SettlersAlive, demand.settlers.0);
        set!(g, G::SettlersQueued, demand.settlers.1);
        set!(g, G::WorkersAlive, demand.workers.0);
        set!(g, G::WorkersQueued, demand.workers.1);
        set!(g, G::MilitaryQueued, demand.military_queued);
        let jobs = worker_jobs(obs, rules);
        set!(g, G::WorkerJobs, jobs);
        set!(g, G::FreeWorkerJobs, jobs - JOBS_PER_WORKER * (demand.workers.0 + demand.workers.1));
        set!(g, G::CitySites, known_city_sites(obs, rules));
        let speed = speed_facts(obs, rules);
        set!(g, G::SpeedProduction, speed.production_percent);
        set!(g, G::SpeedScience, speed.science_percent);
        set!(g, G::SpeedCulture, speed.culture_percent);
        set!(g, G::SpeedFaith, speed.faith_percent);
        set!(g, G::SpeedImprovement, speed.improvement_percent);
        let max_rounds = i64::from(options.max_rounds);
        set!(g, G::MaxRounds, max_rounds);
        set!(g, G::RoundShare, i64::from(obs.round) * 1000 / max_rounds.max(1));
        let techs = G::COUNT;
        let policies = techs + rules.technologies.len();
        let branches = policies + rules.module.policies.len();
        for t in &e.techs {
            if let Some(v) = g.get_mut(techs + usize::from(t.0)).filter(|_| usize::from(t.0) < rules.technologies.len()) {
                *v = 1;
            }
        }
        for p in &c.policies {
            if usize::from(p.0) < rules.module.policies.len() {
                g[policies + usize::from(p.0)] = 1;
            }
        }
        for b in c.branches.iter().chain(c.ideology.iter()) {
            if usize::from(b.0) < rules.module.policy_branches.len() {
                g[branches + usize::from(b.0)] = 1;
            }
        }
    }

    /// Own and foreign cities of the turn's rows as points of interest:
    /// (tile, entity row, relation).
    fn city_points(&self) -> Vec<(TileId, i32, i32)> {
        self.entity_meta
            .iter()
            .enumerate()
            .filter(|(_, m)| (m[0] == kind::OWN_CITY || m[0] == kind::FOREIGN_CITY) && m[1] >= 0)
            .map(|(i, m)| (TileId(m[1] as u32), i as i32, m[2]))
            .collect()
    }
}

fn unit_kind_fields(r: &mut [i16], rules: &CompiledRuleset, kind: UnitKindId) {
    set!(r, E::UnitKindId, i64::from(kind.0) + 1);
    let Some(def) = rules.units.get(usize::from(kind.0)) else { return };
    set!(r, E::Strength, def.strength);
    set!(r, E::RangedStrength, def.ranged_strength);
    set!(r, E::Range, def.range);
    set!(r, E::Movement, def.movement);
    set!(r, E::Sight, def.sight);
    match def.domain {
        Domain::Land => set!(r, E::DomainLand, 1),
        Domain::Sea => set!(r, E::DomainSea, 1),
        Domain::Air => set!(r, E::DomainAir, 1),
    }
    set!(r, E::Founder, def.founder);
    set!(r, E::Builder, def.builder);
    set!(r, E::Civilian, rules.is_civilian(kind));
    set!(r, E::UnitCost, def.cost / 10);
    set!(r, E::UnitEra, era_index(rules, def.era));
}

/// The tile rows of every tile of the map.
fn tile_rows(obs: &AIObservation<'_>, rules: &CompiledRuleset, turn: &Turn) -> Vec<i16> {
    let view = &obs.view;
    let dims = turn.dims;
    let count = dims.tile_count();
    let mut out = vec![0i16; count * T::COUNT];
    let techs = TechSet::of(&obs.economy.techs);
    let kinds = view.terrain_kinds();
    // Units and cities by tile.
    let mut own_strength = vec![0i64; count];
    let mut own_units = vec![0i64; count];
    let mut peace_strength = vec![0i64; count];
    let mut war_strength = vec![0i64; count];
    let mut foreign_units = vec![0i64; count];
    let mut worked = vec![false; count];
    for u in &obs.own_units {
        if let Some(i) = Some(u.tile.0 as usize).filter(|i| *i < count) {
            own_units[i] += 1;
            own_strength[i] += unit_strength(rules, u.kind, u.hp);
        }
    }
    for u in &obs.visible_foreign_units {
        if let Some(i) = Some(u.tile.0 as usize).filter(|i| *i < count) {
            foreign_units[i] += 1;
            let s = unit_strength(rules, u.kind, u.hp);
            match turn.relation(u.owner) {
                relation::WAR | relation::BARBARIAN => war_strength[i] += s,
                _ => peace_strength[i] += s,
            }
        }
    }
    for c in &obs.own_cities {
        for t in &c.worked {
            if let Some(w) = worked.get_mut(t.0 as usize) {
                *w = true;
            }
        }
    }
    let own_city_tiles: Vec<TileId> = obs.own_cities.iter().map(|c| c.tile).collect();
    for n in 0..count {
        let tile = TileId(n as u32);
        let known = view.tile(tile);
        let (snapshot, seen_ago, visible) = match known {
            crate::knowledge::view::KnownTile::Unknown => continue,
            crate::knowledge::view::KnownTile::Remembered { snapshot, last_seen_round } => (snapshot, i64::from(obs.round.saturating_sub(last_seen_round)), false),
            crate::knowledge::view::KnownTile::Visible(snapshot) => (snapshot, 0, true),
        };
        let r = &mut out[n * T::COUNT..(n + 1) * T::COUNT];
        set!(r, T::Known, 1);
        set!(r, T::Visible, visible);
        set!(r, T::SeenAgo, seen_ago.min(500));
        set!(r, T::Water, view.is_water(tile));
        set!(r, T::Lake, view.lake(tile));
        set!(r, T::CoastalLand, view.coastal_land(tile));
        set!(r, T::Hills, kinds.is_hills(snapshot.terrain.0));
        set!(r, T::Mountains, kinds.is_mountains(snapshot.terrain.0));
        let terrain = rules.terrain_kind_of(snapshot.terrain).and_then(|k| rules.terrains.get(usize::from(k.0)).map(|d| (k, d)));
        let feature = snapshot.feature.and_then(|f| rules.features.get(usize::from(f.0)));
        let wonder = snapshot.natural_wonder.and_then(|w| rules.natural_wonders.get(usize::from(w.0)));
        let impassable = terrain.is_some_and(|(_, d)| d.impassable) || feature.is_some_and(|f| f.impassable) || wonder.is_some_and(|w| w.impassable);
        set!(r, T::Impassable, impassable);
        let moves = [terrain.map_or(0, |(_, d)| d.movement_cost), feature.map_or(0, |f| f.movement_cost), wonder.map_or(0, |w| w.movement_cost)];
        set!(r, T::MoveCost, moves.into_iter().max().unwrap_or(0));
        set!(r, T::Defense, terrain.map_or(0, |(_, d)| d.defense_pct) + feature.map_or(0, |f| f.defense_pct) + wonder.map_or(0, |w| w.defense_pct));
        set!(r, T::River, snapshot.river);
        set!(r, T::Rough, feature.is_some_and(|f| f.rough) || wonder.is_some_and(|w| w.rough));
        set!(r, T::CanFound, terrain.is_some_and(|(_, d)| d.can_found_city));
        set!(r, T::TerrainId, terrain.map_or(0, |(k, _)| i64::from(k.0) + 1));
        set!(r, T::FeatureId, snapshot.feature.map_or(0, |f| i64::from(f.0) + 1));
        set!(r, T::ResourceId, snapshot.resource.map_or(0, |x| i64::from(x.0) + 1));
        set!(r, T::ImprovementId, snapshot.improvement.map_or(0, |x| i64::from(x.0) + 1));
        set!(r, T::WonderId, snapshot.natural_wonder.map_or(0, |x| i64::from(x.0) + 1));
        match snapshot.resource.and_then(|x| rules.resources.get(usize::from(x.0))).and_then(|d| d.class) {
            Some(ResourceClass::Bonus) => set!(r, T::ResourceBonus, 1),
            Some(ResourceClass::Strategic) => set!(r, T::ResourceStrategic, 1),
            Some(ResourceClass::Luxury) => set!(r, T::ResourceLuxury, 1),
            _ => {}
        }
        set!(r, T::Pillaged, view.known_pillaged(tile));
        set!(r, T::Road, snapshot.route == crate::world::map::Route::Road);
        set!(r, T::Railroad, snapshot.route == crate::world::map::Route::Railroad);
        put_yields(r, T::Food as usize, plot_yields(view, rules, tile, &techs));
        if let Some(owner) = snapshot.owner {
            r[T::OwnerOwn as usize + turn.relation(owner) as usize] = 1;
        }
        set!(r, T::City, snapshot.city);
        set!(r, T::Worked, worked[n]);
        set!(r, T::OwnStrength, own_strength[n]);
        set!(r, T::OwnUnits, own_units[n]);
        set!(r, T::PeaceStrength, peace_strength[n]);
        set!(r, T::WarStrength, war_strength[n]);
        set!(r, T::ForeignUnits, foreign_units[n]);
        set!(r, T::CapitalDistance, turn.capital_distance(tile));
        set!(r, T::CityDistance, clip(own_city_tiles.iter().filter_map(|c| dims.distance(*c, tile)).min()));
    }
    out
}

/// One candidate of a decision: its row, its meta (tag, target tile,
/// target entity row of the turn, parameter id; `-1` none) and the command
/// it stands for (None: waiting, the unit does nothing more).
#[derive(Debug, Clone)]
pub struct Candidate {
    pub row: Vec<i16>,
    pub meta: [i32; 4],
    pub command: Option<Command>,
    /// Format 10, training only: the `A` targets (`unit_candidates_with_targets`;
    /// empty elsewhere).
    pub targets: Vec<i16>,
}

impl Candidate {
    fn new(tag: i32, command: Option<Command>) -> Self {
        Self { row: vec![0; C::COUNT], meta: [tag, -1, -1, -1], command, targets: Vec::new() }
    }
}

/// What kind of action a unit command is (the `act_*` one-hot).
fn act_of(command: &Command) -> C {
    use Command as K;
    match command {
        K::MoveUnit { .. } => C::ActMove,
        K::MarchTo { .. } => C::ActMarch,
        K::AttackUnit { .. } | K::AttackCity { .. } => C::ActMelee,
        K::RangedAttack { .. } | K::SetUpRangedAttack { .. } => C::ActRanged,
        K::FoundCity { .. } => C::ActFound,
        K::Fortify { .. } => C::ActFortify,
        K::BuildImprovement { .. } | K::BuildRoad { .. } | K::ChopForest { .. } | K::BuildCitadel { .. } => C::ActWork,
        K::Pillage { .. } => C::ActPillage,
        K::GreatPersonAction { .. } | K::CreateGreatImprovement { .. } | K::CreateGreatWork { .. } | K::WriteTreatise { .. } | K::PerformConcert { .. } => C::ActGreatPerson,
        K::Promote { .. } => C::ActPromote,
        K::UpgradeUnit { .. } => C::ActUpgrade,
        K::EstablishTradeRoute { .. } | K::ChangeTradeHome { .. } | K::PlunderTradeRoute { .. } | K::SellExoticGoods { .. } => C::ActTrade,
        K::SpreadReligion { .. } | K::RemoveHeresy { .. } => C::ActReligion,
        K::Rebase { .. } | K::Airlift { .. } | K::AirSweep { .. } | K::Intercept { .. } | K::Nuke { .. } | K::Paradrop { .. } | K::RebaseToCarrier { .. } => C::ActAir,
        _ => C::ActOther,
    }
}

fn improvement_fields(row: &mut [i16], rules: &CompiledRuleset, improvement: ImprovementId) {
    if let Some(def) = rules.improvements.get(usize::from(improvement.0)) {
        put_yields(row, C::ImpFood as usize, def.yields);
        set!(row, C::WorkTurns, def.work_turns);
    }
}

/// The candidates of one unit decision: the legal commands of the near's
/// unit (`knowledge::actions::unit_actions`, its order), then long marches to
/// the points of interest (every city of the turn's rows, own and foreign,
/// city-states' included; for a founder also the city sites within
/// `SITE_RADIUS`: known land a city may be founded on as far as the player
/// knows — its terrain allows a city, unowned or own, no known city within
/// `MIN_CITY_RANGE`), then waiting (no command). Entities the turn has no
/// row for yet get one. A candidate with a target tile (a founding: the
/// unit's own) counts the other own units whose march order ends within
/// `MARCH_NEAR` of it (`target_claimed_by_own`): the orders of the units
/// near it as the world has them now (`LocalObservation::own_units`, so a
/// march given earlier in the turn counts), the others' at the turn's
/// start.
pub fn unit_candidates(turn: &mut Turn, near: &LocalObservation<'_>, rules: &CompiledRuleset) -> Vec<Candidate> {
    let unit = near.unit;
    let dims = turn.dims;
    let distance = |tile: TileId| clip(dims.distance(unit.tile, tile));
    let reachable: BTreeMap<u32, u16> = near.view.own_reachable(unit.handle).into_iter().map(|(t, m)| (t.0, m)).collect();
    let captures = near.view.captures_cities(unit.handle);
    let strike = MoveStrike::new(near, rules, captures, &turn.city_points());
    let mut out = Vec::new();
    for command in one_per_blow(near, unit_actions(near)) {
        let tag = codec_tag(&command).map_or(-1, i32::from);
        let mut c = Candidate::new(tag, None);
        set!(c.row, act_of(&command), 1);
        let mut target: Option<TileId> = None;
        let mut entity = -1;
        let mut param = -1;
        let mut preview: Option<TileId> = None;
        match command {
            Command::MoveUnit { destination, .. } => {
                target = Some(destination);
                let after = reachable.get(&destination.0).copied().unwrap_or(0);
                set!(c.row, C::MovesAfter, after / 6);
                if let Some(strike) = &strike {
                    let _span = prof::span("net/move_strike");
                    strike.fill(near, &mut c.row, destination, after);
                }
            }
            Command::AttackUnit { defender, .. } => {
                if let Some(foe) = near.foreign_units.iter().find(|f| f.handle == defender) {
                    target = Some(foe.tile);
                    entity = turn.foreign_unit_row(foe, rules);
                    preview = target;
                    set!(c.row, C::TargetForeignUnit, 1);
                }
            }
            Command::AttackCity { city, .. } => {
                if let Some(foe) = near.foreign_cities.iter().find(|f| f.handle == city) {
                    target = Some(foe.tile);
                    entity = turn.observed_city_row(foe);
                    preview = target;
                    set!(c.row, C::TargetForeignCity, 1);
                }
            }
            Command::RangedAttack { target: at, .. } | Command::AirSweep { target: at, .. } | Command::Nuke { target: at, .. } => {
                target = Some(at);
                preview = matches!(command, Command::RangedAttack { .. }).then_some(at);
                if let Some(foe) = near.foreign_cities.iter().find(|f| f.tile == at) {
                    entity = turn.observed_city_row(foe);
                    set!(c.row, C::TargetForeignCity, 1);
                } else if let Some(foe) = near.foreign_units.iter().find(|f| f.tile == at) {
                    entity = turn.foreign_unit_row(foe, rules);
                    set!(c.row, C::TargetForeignUnit, 1);
                }
            }
            Command::Paradrop { destination, .. } => target = Some(destination),
            Command::BuildImprovement { improvement, .. } => {
                param = i32::from(improvement.0);
                improvement_fields(&mut c.row, rules, improvement);
            }
            Command::GreatPersonAction { action: crate::derived::great_people::GreatPersonAction::Construct(improvement), .. } => {
                param = i32::from(improvement.0);
                improvement_fields(&mut c.row, rules, improvement);
            }
            Command::Promote { promotion, .. } => {
                param = i32::from(promotion.0);
                if let Some(def) = rules.promotions.get(usize::from(promotion.0)) {
                    put_amounts(&mut c.row, &def.effects);
                }
            }
            Command::UpgradeUnit { .. } => {
                if let Some((to, _)) = near.view.upgrade_offer(unit.handle) {
                    param = i32::from(to.0);
                }
            }
            Command::Rebase { city, .. } | Command::Airlift { city, .. } | Command::ChangeTradeHome { city, .. } => {
                if let Some(own) = near.own_cities.iter().find(|o| o.handle == city) {
                    target = Some(own.tile);
                    entity = turn.own_city_handles.get(&(city.index, city.generation)).copied().unwrap_or(-1);
                    set!(c.row, C::TargetOwnCity, 1);
                }
            }
            Command::EstablishTradeRoute { destination, kind, .. } => {
                param = kind as i32;
                if let Some(own) = near.own_cities.iter().find(|o| o.handle == destination) {
                    target = Some(own.tile);
                    entity = turn.own_city_handles.get(&(destination.index, destination.generation)).copied().unwrap_or(-1);
                    set!(c.row, C::TargetOwnCity, 1);
                }
            }
            Command::RebaseToCarrier { carrier, .. } => {
                if let Some(own) = near.own_units.iter().find(|o| o.handle == carrier) {
                    target = Some(own.tile);
                    entity = turn.own_unit_row(own, rules);
                    set!(c.row, C::TargetOwnUnit, 1);
                }
            }
            Command::ClaimRuins { reward, .. } => param = reward as i32,
            Command::GiftUnit { city_state, .. } => param = i32::from(city_state.0),
            _ => {}
        }
        if let Some(at) = preview {
            if let Some(p) = near.view.attack_preview(unit.handle, at) {
                set!(c.row, C::ToDefender, p.to_defender.1);
                set!(c.row, C::ToAttacker, p.to_attacker.1);
                set!(c.row, C::DefenderHp, p.defender_hp);
                // Format 8: the blow's outcome. A city on the tile defends it
                // (`pick_defender`); a shot leaves it at 1 HP at least.
                let shot = blow(near, &command).is_some_and(|(kind, _)| kind == AttackKind::Ranged);
                let city = near.foreign_cities.iter().any(|f| f.tile == at);
                let kills = p.verdict == Verdict::DecisiveVictory;
                set!(c.row, C::ToDefenderUncapped, p.uncapped_to_defender);
                set!(c.row, C::BlowKills, kills);
                set!(c.row, C::BlowCaptures, kills && city && !shot && captures);
                set!(c.row, C::CityFloor, city && shot);
            }
        }
        if let Some(at) = target {
            set!(c.row, C::TargetDistance, distance(at));
        }
        c.meta = [tag, target.map_or(-1, |t| t.0 as i32), entity, param];
        c.command = Some(command);
        out.push(c);
    }
    for (tile, entity, rel) in turn.city_points() {
        if tile == unit.tile {
            continue;
        }
        let mut c = march(unit.handle, tile, distance(tile));
        let own = rel == relation::OWN;
        set!(c.row, if own { C::PoiOwnCity } else if rel == relation::CITY_STATE { C::PoiCityState } else { C::PoiForeignCity }, 1);
        set!(c.row, if own { C::TargetOwnCity } else { C::TargetForeignCity }, 1);
        c.meta[2] = entity;
        out.push(c);
    }
    if rules.units.get(usize::from(unit.kind.0)).is_some_and(|d| d.founder) {
        for tile in city_sites(&near.view, rules, near.player, unit.tile) {
            let mut c = march(unit.handle, tile, distance(tile));
            set!(c.row, C::PoiSite, 1);
            out.push(c);
        }
    }
    let mut marches = turn.marches.clone();
    for other in &near.own_units {
        match other.goal {
            Some(goal) => marches.insert(other.id.0, goal),
            None => marches.remove(&other.id.0),
        };
    }
    marches.remove(&unit.id.0);
    for c in &mut out {
        let target = match c.command {
            _ if c.meta[1] >= 0 => Some(TileId(c.meta[1] as u32)),
            Some(Command::FoundCity { .. }) => Some(unit.tile),
            _ => None,
        };
        if let Some(target) = target {
            let near = |goal: &TileId| dims.distance(*goal, target).is_some_and(|d| d <= MARCH_NEAR);
            set!(c.row, C::TargetClaimed, marches.values().filter(|goal| near(goal)).count());
        }
    }
    let mut wait = Candidate::new(tag::WAIT, None);
    set!(wait.row, C::ActWait, 1);
    out.push(wait);
    out
}

/// Format 10: what a unit that fights could strike after a move, from the
/// move's destination — this turn with the moves left there, and the next
/// turn with its full moves — by the rules' own move and combat previews
/// (`PlayerView::own_reachable`, `PlayerQueries::own_reachable_from`,
/// `PlayerQueries::attack_preview_from`), so a move candidate tells "walk in
/// and strike" from a walk alone, and a step toward a prey from a step away:
///
/// - `move_blow_captures`, `move_blow_kills`: the best blow this turn from
///   there (by capture, then kill, then the share of the defender's hit
///   points) takes a city, kills its defender — `blow_captures` and
///   `blow_kills` of that blow's attack candidate once the unit stood there;
/// - `move_blow_damage_percent`: that blow's expected damage, % of the
///   defender's hit points before it (capped as the rules cap it);
/// - `move_city_damage_percent`: the best blow on a hostile city from
///   there, % of the city's hit points (0: no city in strike);
/// - `move_city_adjacent`: the destination borders a hostile city (at war,
///   or a barbarian's; one in sight or remembered out of it: the player
///   knows its tile, a city never moves), whatever the moves left;
/// - `move_city_in_strike`: a hostile city in sight is in the unit's strike
///   from there this turn;
/// - `move_strike_targets`: the tiles in sight the unit could strike from
///   there this turn;
/// - `move_enemy_distance`: tiles from there to the nearest hostile unit in
///   sight outside a city (0: none);
/// - `move_wounded_enemy_distance`, `move_wounded_enemy_hp_percent`: the
///   nearest such unit below full health (the least healthy at that
///   distance) and its hit points, % of a unit's (0: none);
/// - `move_next_turn_targets`: the hostile units outside cities the unit
///   could strike on its next turn begun there (its full moves; a stop with
///   a move left, more than a set-up's for a unit that must set up), the
///   enemy standing where it stands now;
/// - `move_next_turn_kills`: one of those blows kills its defender (the
///   preview's decisive verdict, struck from the stop with the most moves
///   left that reaches it; the unit's health as now);
/// - `move_city_distance`: tiles from there to the nearest hostile city, in
///   sight or remembered (0: none known);
/// - `move_unseen_city_in_reach`: a hostile city out of sight (its hit
///   points unknown to the player, so no preview) is within the unit's
///   reach from there with the moves to strike this turn: a blow the step
///   may allow, its outcome unknown.
///
/// The strike this turn needs a move left after the move, and a unit that
/// must set up (a siege unit: a move undoes its set-up) more than one move:
/// the set-up costs one (`systems::combat::set_up_ranged`). The rest the
/// preview checks as for an attack now: the attacks left this turn (so a
/// unit that has struck already and still moves shows no next-turn kill
/// either), reach, line of sight, domain, war. Only hostile tiles the unit
/// sees are counted; the next turn is previewed only for a destination
/// within `NEXT_TURN_TILES` × the unit's moves (roads) plus its reach of a
/// hostile unit, so a unit far from any enemy costs one pass over the near's
/// foreign units and cities.
struct MoveStrike {
    /// Hostile tiles in sight (each once; a city's tile is the city), with
    /// whether a city stands there.
    targets: Vec<(TileId, bool)>,
    /// Hostile units in sight outside cities: tile and hit points.
    units: Vec<(TileId, u16)>,
    /// Hostile cities the player remembers out of sight (the turn's rows).
    unseen: Vec<TileId>,
    /// The farthest a blow reaches from the destination: 1 for a charge, the
    /// range (one more for a promotion) for a shot.
    reach: i32,
    /// Moves (in `MOVE_DENOMINATOR`ths) the unit must keep after a move to
    /// strike: more than a set-up's for a unit that must set up.
    keep: u16,
    /// The farthest hostile unit whose next-turn strike is previewed.
    horizon: i32,
    /// The unit shoots (a shot leaves a city at 1 hit point).
    shot: bool,
    /// The unit takes a city its blow brings to 0 hit points
    /// (`PlayerQueries::captures_cities`).
    captures: bool,
}

/// The next turn's strike is previewed for a destination at most this many
/// tiles per move of the unit (a road's third of a move) plus its reach from
/// a hostile unit.
const NEXT_TURN_TILES: i32 = 3;

impl MoveStrike {
    /// None for a unit that does not fight (a civilian, no strength) or has
    /// no hostile tile in sight.
    fn new(near: &LocalObservation<'_>, rules: &CompiledRuleset, captures: bool, known_cities: &[(TileId, i32, i32)]) -> Option<Self> {
        let unit = &near.unit;
        let def = rules.units.get(usize::from(unit.kind.0))?;
        if rules.is_civilian(unit.kind) || (def.strength == 0 && def.ranged_strength == 0) {
            return None;
        }
        let kind = near.view.own_attack_kind(unit.handle)?;
        let hostile = |owner: PlayerId| matches!(relation_code(near.player, &near.relations, owner), relation::WAR | relation::BARBARIAN);
        let mut targets: BTreeMap<u32, bool> = BTreeMap::new();
        for city in near.foreign_cities.iter().filter(|c| hostile(c.owner)) {
            targets.insert(city.tile.0, true);
        }
        let mut units: BTreeMap<u32, u16> = BTreeMap::new();
        for foe in near.foreign_units.iter().filter(|f| hostile(f.owner)) {
            targets.entry(foe.tile.0).or_insert(false);
            if !targets[&foe.tile.0] {
                let hp = units.entry(foe.tile.0).or_insert(foe.hp);
                *hp = (*hp).max(foe.hp);
            }
        }
        let unseen: Vec<TileId> = known_cities
            .iter()
            .filter(|(t, _, rel)| matches!(*rel, relation::WAR | relation::BARBARIAN) && !targets.contains_key(&t.0) && !near.view.is_visible(*t))
            .map(|(t, _, _)| *t)
            .collect();
        if targets.is_empty() && unseen.is_empty() {
            return None;
        }
        let reach = match kind {
            AttackKind::Melee => 1,
            AttackKind::Ranged => i32::from(def.range) + 1,
        };
        let sets_up = near.view.must_set_up_first(unit.handle) || unit.stance == crate::world::unit::Stance::SetUp;
        let keep = if sets_up { crate::world::unit::MOVE_DENOMINATOR } else { 0 };
        let moves = i32::from(unit.moves_max.div_ceil(crate::world::unit::MOVE_DENOMINATOR)).max(1);
        Some(Self {
            targets: targets.into_iter().map(|(t, city)| (TileId(t), city)).collect(),
            units: units.into_iter().map(|(t, hp)| (TileId(t), hp)).collect(),
            unseen,
            reach,
            keep,
            horizon: NEXT_TURN_TILES * moves + reach,
            shot: kind == AttackKind::Ranged,
            captures,
        })
    }

    /// The columns of a move to `to` with `after` moves left there.
    fn fill(&self, near: &LocalObservation<'_>, row: &mut [i16], to: TileId, after: u16) {
        let dims = near.view.dimensions();
        let mut adjacent = false;
        let mut targets = 0;
        // (captures, kills, damage %) of the best blow; the best on a city.
        let mut best: Option<(bool, bool, i64)> = None;
        let mut city_best: Option<i64> = None;
        for &(tile, city) in &self.targets {
            let Some(d) = dims.distance(to, tile) else { continue };
            adjacent |= city && d == 1;
            if d > self.reach || after <= self.keep {
                continue;
            }
            let Some(p) = near.view.attack_preview_from(near.unit.handle, to, tile) else { continue };
            targets += 1;
            let kills = p.verdict == Verdict::DecisiveVictory;
            let share = i64::from(p.to_defender.1) * 100 / i64::from(p.defender_hp.max(1));
            let blow = (kills && city && !self.shot && self.captures, kills, share);
            if best.is_none_or(|b| blow > b) {
                best = Some(blow);
            }
            if city {
                city_best = Some(city_best.map_or(share, |b| b.max(share)));
            }
        }
        if let Some((captures, kills, share)) = best {
            set!(row, C::MoveBlowCaptures, captures);
            set!(row, C::MoveBlowKills, kills);
            set!(row, C::MoveBlowDamage, share);
        }
        // The cities out of sight: where they stand is known, not how they stand.
        let mut nearest_city = self.targets.iter().filter(|t| t.1).filter_map(|t| dims.distance(to, t.0)).min();
        let mut unseen_in_reach = false;
        for &tile in &self.unseen {
            let Some(d) = dims.distance(to, tile) else { continue };
            adjacent |= d == 1;
            unseen_in_reach |= d <= self.reach && after > self.keep;
            nearest_city = Some(nearest_city.map_or(d, |n| n.min(d)));
        }
        set!(row, C::MoveCityDistance, nearest_city.map_or(0, |d| d.clamp(1, CLIP)));
        set!(row, C::MoveUnseenCityInReach, unseen_in_reach);
        set!(row, C::MoveCityDamage, city_best.unwrap_or(0));
        set!(row, C::MoveCityAdjacent, adjacent);
        set!(row, C::MoveCityInStrike, city_best.is_some());
        set!(row, C::MoveTargets, targets);
        // The hostile units outside cities: how near, the wounded, next turn.
        let distances: Vec<(i32, TileId, u16)> = self.units.iter().filter_map(|&(t, hp)| Some((dims.distance(to, t)?, t, hp))).collect();
        let Some(nearest) = distances.iter().map(|d| d.0).min() else { return };
        set!(row, C::MoveEnemyDistance, nearest.clamp(1, CLIP));
        let max_hp = crate::world::unit::UNIT_MAX_HP;
        if let Some(&(d, _, hp)) = distances.iter().filter(|x| x.2 < max_hp).min_by_key(|x| (x.0, x.2, x.1 .0)) {
            set!(row, C::MoveWoundedDistance, d.clamp(1, CLIP));
            set!(row, C::MoveWoundedHp, i64::from(hp) * 100 / i64::from(max_hp));
        }
        if nearest > self.horizon {
            return;
        }
        let mut stops: Vec<(TileId, u16)> = near.view.own_reachable_from(near.unit.handle, to).into_iter().filter(|(_, left)| *left > self.keep).collect();
        stops.sort_by_key(|(t, left)| (std::cmp::Reverse(*left), t.0));
        let (mut reached, mut kills) = (0, false);
        for &(d, tile, _) in &distances {
            if d > self.horizon {
                continue;
            }
            let from = stops.iter().filter(|(s, _)| dims.distance(*s, tile).is_some_and(|x| x <= self.reach)).find_map(|(s, _)| near.view.attack_preview_from(near.unit.handle, *s, tile));
            if let Some(p) = from {
                reached += 1;
                kills |= p.verdict == Verdict::DecisiveVictory;
            }
        }
        set!(row, C::MoveNextTargets, reached);
        set!(row, C::MoveNextKills, kills);
    }
}

/// [`unit_candidates`] with each candidate's training-only targets (`A`,
/// format 10; the exporter's, never the playing net's), by the rules'
/// previews over what the player sees, ignoring the order of the strikes:
///
/// - `focus_damage_percent`, `focus_takes`: per hostile tile (a city, or a
///   unit outside one), the expected damage every own unit near that can
///   still strike this turn (`OwnUnit::can_attack`: moves and an attack
///   left) could deal it this turn — each its best blow, from where it
///   stands or after a move (`own_reachable`, a move left, more than a
///   set-up's for a unit that must set up) — summed, % of the defender's hit
///   points (at most 1 000), and whether the sum reaches them (a city: and a
///   unit that may take it, `captures_cities`, among the strikers). On an
///   attack candidate its target's; on a move candidate the best (by takes,
///   then damage) of the targets the unit could strike from the
///   destination this turn; 0 elsewhere.
/// - `danger_damage`, `danger_dies`: on a move candidate, the expected
///   damage the hostile units in sight could deal the unit on the
///   destination on their next turn, summed (each from the tile next to the
///   destination nearest to it, a shooter from where it stands when in
///   range, else from the nearest tile at its range; one that cannot get
///   there with its moves, roads aside, is left out), and whether the sum
///   reaches the unit's hit points; 0 elsewhere.
pub fn unit_candidates_with_targets(turn: &mut Turn, near: &LocalObservation<'_>, rules: &CompiledRuleset) -> Vec<Candidate> {
    let mut out = unit_candidates(turn, near, rules);
    for c in &mut out {
        c.targets = vec![0; A::COUNT];
    }
    let hostile = |owner: PlayerId| matches!(relation_code(near.player, &near.relations, owner), relation::WAR | relation::BARBARIAN);
    let cities: Vec<(TileId, u16)> = near.foreign_cities.iter().filter(|c| hostile(c.owner)).map(|c| (c.tile, c.hp)).collect();
    let foes: Vec<&ObservedUnit> = near.foreign_units.iter().filter(|f| hostile(f.owner)).collect();
    let mut targets: Vec<(TileId, u16, bool)> = cities.iter().map(|&(t, hp)| (t, hp, true)).collect();
    for f in &foes {
        if !targets.iter().any(|t| t.0 == f.tile) {
            targets.push((f.tile, f.hp, false));
        }
    }
    if targets.is_empty() {
        return out;
    }
    let dims = near.view.dimensions();
    // Focus fire: per target, the summed best blow of every striker near.
    let mut strikers: Vec<OwnUnit> = near.own_units.iter().filter(|u| u.can_attack).copied().collect();
    if near.unit.can_attack {
        strikers.push(near.unit);
    }
    let mut focus: Vec<(i64, bool)> = vec![(0, false); targets.len()];
    for u in &strikers {
        let Some(def) = rules.units.get(usize::from(u.kind.0)) else { continue };
        if rules.is_civilian(u.kind) || (def.strength == 0 && def.ranged_strength == 0) {
            continue;
        }
        let Some(kind) = near.view.own_attack_kind(u.handle) else { continue };
        let reach = if kind == AttackKind::Ranged { i32::from(def.range) + 1 } else { 1 };
        let sets_up = near.view.must_set_up_first(u.handle) || u.stance == crate::world::unit::Stance::SetUp;
        let keep = if sets_up { crate::world::unit::MOVE_DENOMINATOR } else { 0 };
        let capturer = near.view.captures_cities(u.handle);
        let stops: Vec<TileId> = near.view.own_reachable(u.handle).into_iter().filter(|(t, left)| *t != u.tile && *left > keep).map(|(t, _)| t).collect();
        for (i, &(tile, hp, city)) in targets.iter().enumerate() {
            let here = near.view.attack_preview(u.handle, tile);
            let moved = stops.iter().filter(|s| dims.distance(**s, tile).is_some_and(|d| d <= reach)).filter_map(|s| near.view.attack_preview_from(u.handle, *s, tile));
            let Some(best) = here.into_iter().chain(moved).map(|p| i64::from(p.to_defender.1)).max() else { continue };
            focus[i].0 += best.min(i64::from(hp));
            focus[i].1 |= !city || capturer;
        }
    }
    let focus_of = |i: usize| {
        let (sum, able) = focus[i];
        let hp = i64::from(targets[i].1.max(1));
        ((sum * 100 / hp).min(1000), able && sum >= hp)
    };
    // Danger: the hostile units' strikes on a destination.
    let unit = near.unit;
    for c in &mut out {
        let Some(command) = &c.command else { continue };
        let attack_tile = blow(near, command).map(|(_, t)| t);
        let pick = match (command, attack_tile) {
            (_, Some(tile)) => targets.iter().position(|t| t.0 == tile).map(|i| focus_of(i)),
            (Command::MoveUnit { destination, .. }, None) => targets
                .iter()
                .enumerate()
                .filter(|(_, t)| dims.distance(*destination, t.0).is_some_and(|d| d <= 1 + i32::from(rules.units.get(usize::from(unit.kind.0)).map_or(0, |d| d.range))))
                .filter(|(_, t)| near.view.attack_preview_from(unit.handle, *destination, t.0).is_some())
                .map(|(i, _)| focus_of(i))
                .max_by_key(|(damage, takes)| (*takes, *damage)),
            _ => None,
        };
        if let Some((damage, takes)) = pick {
            set!(c.targets, A::FocusDamage, damage);
            set!(c.targets, A::FocusTakes, takes);
        }
        let Command::MoveUnit { destination, .. } = *command else { continue };
        let mut danger = 0i64;
        for f in &foes {
            let Some(def) = rules.units.get(usize::from(f.kind.0)) else { continue };
            if rules.is_civilian(f.kind) || (def.strength == 0 && def.ranged_strength == 0) {
                continue;
            }
            let Some(d) = dims.distance(f.tile, destination) else { continue };
            let range = if def.ranged_strength > 0 && def.range > 0 { i32::from(def.range) } else { 1 };
            if d - range > i32::from(def.movement) {
                continue;
            }
            let from = if d <= range && range > 1 {
                Some(f.tile)
            } else {
                hex_disc(dims, destination, range).filter(|t| dims.distance(*t, destination) == Some(range)).min_by_key(|t| (dims.distance(*t, f.tile).unwrap_or(i32::MAX), t.0))
            };
            if let Some(p) = from.and_then(|from| near.view.defense_preview_at(unit.handle, destination, f.handle, from)) {
                danger += i64::from(p.to_defender.1);
            }
        }
        set!(c.targets, A::DangerDamage, danger);
        set!(c.targets, A::DangerDies, danger >= i64::from(unit.hp));
    }
    out
}

/// Format 9: the defer candidate, offered last in a unit's decision when the
/// net may defer the unit (`mind::NetPlayer`: weights that know it, a
/// deferral left this turn, `kernel::ai::driver::DEFERS_PER_UNIT`): the unit
/// goes to the end of the turn's queue and decides again there. Its row is
/// all zeros (no feature of its own: the net reads the moment from the
/// actor's token and the turn trace), no target, no parameter.
pub fn defer_candidate() -> Candidate {
    Candidate::new(tag::DEFER, None)
}

/// The blow an attack command of the near's unit strikes: how and which
/// tile. The rules resolve `AttackUnit` and `AttackCity` to an attack of the
/// target's tile the unit's way (`validate_attack_tile`: a ranged unit or an
/// aircraft shoots, the others charge; on a city's tile the city defends,
/// whoever it holds), `RangedAttack` to a shot at its tile
/// (`validate_ranged`). Two commands with one blow are one action. None: no
/// attack, or a target the near does not see.
pub fn blow(near: &LocalObservation<'_>, command: &Command) -> Option<(AttackKind, TileId)> {
    let kind = || near.view.own_attack_kind(near.unit.handle);
    match *command {
        Command::AttackUnit { defender, .. } => Some((kind()?, near.foreign_units.iter().find(|f| f.handle == defender)?.tile)),
        Command::AttackCity { city, .. } => Some((kind()?, near.foreign_cities.iter().find(|f| f.handle == city)?.tile)),
        Command::RangedAttack { target, .. } => Some((AttackKind::Ranged, target)),
        _ => None,
    }
}

/// The legal list with one command per blow (`blow`): the shot
/// (`RangedAttack`) of a tile, else the charge on its city (`AttackCity`),
/// else the first charge on a unit of it; every other command as listed, in
/// the list's order. Nothing the rules allow is lost: a dropped command
/// strikes the blow of the one kept (format 8; before it, a garrisoned
/// city's capture was two candidates, `AttackUnit` on the garrison and
/// `AttackCity`).
fn one_per_blow(near: &LocalObservation<'_>, commands: Vec<Command>) -> Vec<Command> {
    let rank = |c: &Command| match c {
        Command::RangedAttack { .. } => 0,
        Command::AttackCity { .. } => 1,
        _ => 2,
    };
    let blows: Vec<_> = commands.iter().map(|c| blow(near, c)).collect();
    let mut best: BTreeMap<(bool, u32), usize> = BTreeMap::new();
    for (i, b) in blows.iter().enumerate() {
        if let Some((kind, tile)) = *b {
            let at = best.entry((kind == AttackKind::Ranged, tile.0)).or_insert(i);
            if rank(&commands[i]) < rank(&commands[*at]) {
                *at = i;
            }
        }
    }
    let kept: Vec<bool> = blows.iter().enumerate().map(|(i, b)| b.is_none_or(|(kind, tile)| best[&(kind == AttackKind::Ranged, tile.0)] == i)).collect();
    commands.into_iter().zip(kept).filter_map(|(c, k)| k.then_some(c)).collect()
}

/// The candidate of `candidates` that strikes the blow of the attack
/// `command` (`blow`): where the expert's or a recorded attack is placed
/// once the list keeps one command per blow. None for another command.
pub fn place_blow(near: &LocalObservation<'_>, candidates: &[Candidate], command: &Command) -> Option<Option<usize>> {
    let target = blow(near, command)?;
    Some(candidates.iter().position(|c| c.command.as_ref().is_some_and(|k| blow(near, k) == Some(target))))
}

fn march(unit: UnitHandle, tile: TileId, distance: i32) -> Candidate {
    let mut c = Candidate::new(tag::MARCH, Some(Command::MarchTo { unit, destination: tile }));
    set!(c.row, C::ActMarch, 1);
    set!(c.row, C::TargetDistance, distance);
    c.meta[1] = tile.0 as i32;
    c
}

/// Known land within `SITE_RADIUS` of `from` (its own tile aside) a city may
/// be founded on as far as `player` knows: the terrain allows a city and is
/// passable, the tile is unowned or the player's, no known city within
/// `MIN_CITY_RANGE` (the core's `city_site_check` on known tiles).
pub fn city_sites(view: &PlayerView<'_>, rules: &CompiledRuleset, player: PlayerId, from: TileId) -> Vec<TileId> {
    let dims = view.dimensions();
    let mut out: Vec<TileId> = hex_disc(dims, from, SITE_RADIUS).filter(|&tile| tile != from && is_city_site(view, rules, player, tile)).collect();
    out.sort_by_key(|t| t.0);
    out
}

/// The city-site rule of `city_sites` for one tile.
fn is_city_site(view: &PlayerView<'_>, rules: &CompiledRuleset, player: PlayerId, tile: TileId) -> bool {
    let Some(s) = view.tile(tile).snapshot().copied() else { return false };
    let terrain = rules.terrain_kind_of(s.terrain).and_then(|k| rules.terrains.get(usize::from(k.0)));
    let land = terrain.is_some_and(|d| d.can_found_city && !d.impassable && !d.water);
    let feature_ok = s.feature.and_then(|f| rules.features.get(usize::from(f.0))).is_none_or(|f| !f.impassable);
    let free = land && feature_ok && s.natural_wonder.is_none() && s.owner.is_none_or(|o| o == player);
    free && hex_disc(view.dimensions(), tile, crate::world::territory::MIN_CITY_RANGE).all(|t| !view.tile(t).snapshot().is_some_and(|s| s.city))
}

/// The known city sites within the empire's reach: the tiles of the
/// city-site rule (`city_sites`) within `SITE_RADIUS` of an own city (with
/// no city: of an own founder), taken in tile order while no site taken is
/// within `MIN_CITY_RANGE` (two cities may stand on them). Raw facts: the
/// planner's count (`planner_cities::known_sites`) values the sites and
/// needs the scripted mind's state, so the net gets this one.
pub fn known_city_sites(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> usize {
    let view = &obs.view;
    let dims = view.dimensions();
    let founder = |kind: UnitKindId| rules.units.get(usize::from(kind.0)).is_some_and(|d| d.founder);
    let mut origins: Vec<TileId> = obs.own_cities.iter().map(|c| c.tile).collect();
    if origins.is_empty() {
        origins = obs.own_units.iter().filter(|u| founder(u.kind)).map(|u| u.tile).collect();
    }
    let mut seen = vec![false; dims.tile_count()];
    let mut tiles = Vec::new();
    for origin in origins {
        for tile in hex_disc(dims, origin, SITE_RADIUS) {
            if let Some(s) = seen.get_mut(tile.0 as usize).filter(|s| !**s) {
                *s = true;
                if is_city_site(view, rules, obs.player, tile) {
                    tiles.push(tile);
                }
            }
        }
    }
    tiles.sort_by_key(|t| t.0);
    let range = crate::world::territory::MIN_CITY_RANGE;
    let mut taken: Vec<TileId> = Vec::new();
    for tile in tiles {
        if taken.iter().all(|t| dims.distance(*t, tile).is_none_or(|d| d > range)) {
            taken.push(tile);
        }
    }
    taken.len()
}

/// The game speed's percents (Unciv's `Speeds.json`: 100 at Standard):
/// the cost of production, science, culture and faith and the improvements'
/// build time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpeedFacts {
    pub production_percent: i64,
    pub science_percent: i64,
    pub culture_percent: i64,
    pub faith_percent: i64,
    pub improvement_percent: i64,
}

impl SpeedFacts {
    pub const STANDARD: SpeedFacts = SpeedFacts { production_percent: 100, science_percent: 100, culture_percent: 100, faith_percent: 100, improvement_percent: 100 };
}

/// The speed of the game `obs` sees: the game's settings
/// (`GameSettings::speed`, public through `PlayerView::speed`; Standard when
/// the game names none).
pub fn speed_facts(obs: &AIObservation<'_>, _rules: &CompiledRuleset) -> SpeedFacts {
    let v = obs.view.speed();
    SpeedFacts {
        production_percent: i64::from(v.production_percent),
        science_percent: i64::from(v.science_percent),
        culture_percent: i64::from(v.culture_percent),
        faith_percent: i64::from(v.faith_percent),
        improvement_percent: i64::from(v.improvement_percent),
    }
}

/// Settlers, workers and military units of the empire: alive and in its
/// cities' queues (every item of a queue). A settler founds cities, a
/// worker builds improvements on land (the table's roles; builders of the
/// sea aside, as the scripted mind counts them), a military unit is not a
/// civilian.
struct Demand {
    settlers: (i64, i64),
    workers: (i64, i64),
    military_queued: i64,
}

impl Demand {
    fn of(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Self {
        let def = |kind: UnitKindId| rules.units.get(usize::from(kind.0));
        let settler = |kind: UnitKindId| def(kind).is_some_and(|d| d.founder);
        let worker = |kind: UnitKindId| def(kind).is_some_and(|d| d.builder && !d.founder && d.domain == Domain::Land);
        let alive = |f: &dyn Fn(UnitKindId) -> bool| obs.own_units.iter().filter(|u| f(u.kind)).count() as i64;
        let queued_kinds: Vec<UnitKindId> = obs
            .own_cities
            .iter()
            .flat_map(|c| &c.queue)
            .filter_map(|item| match item {
                ProductionItem::Unit(kind) => Some(*kind),
                _ => None,
            })
            .collect();
        let queued = |f: &dyn Fn(UnitKindId) -> bool| queued_kinds.iter().filter(|k| f(**k)).count() as i64;
        Self {
            settlers: (alive(&settler), queued(&settler)),
            workers: (alive(&worker), queued(&worker)),
            military_queued: queued(&|k| !rules.is_civilian(k)),
        }
    }
}

/// The jobs a worker could take now on the empire's worked tiles: worked
/// land (no city) without an improvement where the player's workers may
/// build one the rules accept there (the core's site rule
/// `world::builds::site`: technology, nation, terrain, feature removal,
/// resource, ownership) that adds food, production or gold or improves the
/// tile's resource — the workers' job finder (`planner_workers::open_jobs`,
/// the scripted allocator's worker demand) without its valuation (which
/// only ranks the improvements and asks a clear gain to replace one).
pub fn worker_jobs(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> i64 {
    use crate::rules::content::ImprovementKind;
    let nation = obs.economy.nation;
    let builder = crate::world::builds::Builder { player: obs.player, nation, techs: TechSet::of(&obs.economy.techs) };
    let kinds = obs.view.terrain_kinds();
    let buildable: Vec<ImprovementId> = rules
        .improvements
        .iter()
        .enumerate()
        .filter(|(_, d)| {
            let kind_ok = d.kind == ImprovementKind::Regular || d.kind == ImprovementKind::Unique && d.nation == Some(nation);
            let worker_build = (!d.unbuildable && d.work_turns > 0 || d.placement.water) && !d.placement.prompt_when_complete;
            kind_ok && worker_build
        })
        .map(|(i, _)| ImprovementId(i as u16))
        .collect();
    let mut tiles: Vec<TileId> = obs.own_cities.iter().flat_map(|c| c.worked.iter().copied()).collect();
    tiles.sort_by_key(|t| t.0);
    tiles.dedup();
    let open = |tile: TileId| {
        let Some(s) = obs.view.tile(tile).snapshot().copied() else { return false };
        if s.city || s.improvement.is_some() || kinds.is_water(s.terrain.0) {
            return false;
        }
        buildable.iter().any(|&k| {
            let d = &rules.improvements[usize::from(k.0)];
            let gains = d.yields.food + d.yields.production + d.yields.gold > 0 || s.resource.is_some_and(|r| d.resources.contains(&r));
            gains && crate::world::builds::site(&obs.view, rules, &builder, tile, k).is_ok()
        })
    };
    tiles.into_iter().filter(|t| open(*t)).count() as i64
}

/// The technologies the rules let `obs`'s player research now
/// (`ChooseTechnology` accepted), by id.
pub fn research_candidates(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Vec<Candidate> {
    let beakers = i64::from(obs.economy.income.science.max(1));
    let tree = TechTree::new(rules, &obs.economy.techs);
    let known = |t: TechId| obs.economy.techs.contains(&t);
    (0..rules.technologies.len())
        .map(|i| TechId(i as u16))
        .filter(|&technology| obs.view.accepts(&Command::ChooseTechnology { player: obs.player, technology }))
        .map(|tech| {
            let def = &rules.technologies[usize::from(tech.0)];
            let mut c = Candidate::new(tag::RESEARCH, Some(Command::ChooseTechnology { player: obs.player, technology: tech }));
            c.meta[3] = i32::from(tech.0);
            let r = &mut c.row;
            set!(r, C::TechCost, def.cost / 10);
            set!(r, C::TechTurns, (i64::from(def.cost) / beakers).min(500));
            set!(r, C::TechEra, i64::from(def.era) + 1);
            set!(r, C::TechBuildings, def.unlocks_buildings.len());
            let wonder = |b: &&BuildingKindId| rules.buildings.get(usize::from(b.0)).is_some_and(|d| d.wonder) && !obs.built_wonders.contains(b);
            set!(r, C::TechWonders, def.unlocks_buildings.iter().filter(wonder).count());
            set!(r, C::TechUnits, def.unlocks_units.len());
            set!(r, C::TechImprovements, def.unlocks_improvements.len());
            set!(r, C::TechProjects, def.unlocks_projects.len());
            set!(r, C::TechLeadsTo, rules.technologies.iter().filter(|d| d.requires.contains(&tech) && d.requires.iter().all(|q| *q == tech || known(*q))).count());
            let best = def.unlocks_units.iter().filter_map(|u| rules.units.get(usize::from(u.0))).map(|d| d.strength.max(d.ranged_strength)).max().unwrap_or(0);
            set!(r, C::TechBestStrength, best);
            set!(r, C::TechEmbarking, def.allows_embarking || def.embarked_all_water);
            tree.lookahead(rules, tech, &obs.built_wonders, r);
            put_amounts(r, &def.effects);
            c
        })
        .collect()
}

/// The technology tree as one player stands in it: per technology every
/// prerequisite it needs, near or far (`ancestors`), and which are known.
struct TechTree {
    known: Vec<bool>,
    ancestors: Vec<Vec<bool>>,
    last_era: u8,
}

impl TechTree {
    fn new(rules: &CompiledRuleset, known: &[TechId]) -> Self {
        let n = rules.technologies.len();
        let mut known_set = vec![false; n];
        for t in known {
            if let Some(k) = known_set.get_mut(usize::from(t.0)) {
                *k = true;
            }
        }
        let mut ancestors: Vec<Option<Vec<bool>>> = vec![None; n];
        fn closure(rules: &CompiledRuleset, t: usize, memo: &mut Vec<Option<Vec<bool>>>, depth: usize) -> Vec<bool> {
            if let Some(done) = &memo[t] {
                return done.clone();
            }
            let mut out = vec![false; memo.len()];
            if depth < memo.len() {
                for r in &rules.technologies[t].requires {
                    let r = usize::from(r.0);
                    if r < out.len() && !out[r] {
                        out[r] = true;
                        for (i, a) in closure(rules, r, memo, depth + 1).into_iter().enumerate() {
                            out[i] |= a;
                        }
                    }
                }
            }
            memo[t] = Some(out.clone());
            out
        }
        for t in 0..n {
            closure(rules, t, &mut ancestors, 0);
        }
        let last_era = rules.technologies.iter().map(|d| d.era).max().unwrap_or(0);
        Self { known: known_set, ancestors: ancestors.into_iter().map(|a| a.unwrap_or_default()).collect(), last_era }
    }

    /// The technologies to research to have `t`: its unknown prerequisites,
    /// near and far, and itself; and whether `first` is among them.
    fn path(&self, t: usize, first: usize) -> (usize, bool) {
        let mut count = usize::from(!self.known[t]);
        let mut through = t == first;
        for (i, a) in self.ancestors[t].iter().enumerate() {
            if *a && !self.known[i] {
                count += 1;
                through |= i == first;
            }
        }
        (count, through)
    }

    /// The lookahead fields of `candidate`: what the technologies it opens
    /// within 2 and within 3 researched technologies (itself the first, the
    /// path's other prerequisites known or on the path) unlock; and the
    /// fewest technologies still to research after it to a technology that
    /// unlocks a project and to one of the last era (-1: none).
    fn lookahead(&self, rules: &CompiledRuleset, candidate: TechId, built: &[BuildingKindId], r: &mut [i16]) {
        let c = usize::from(candidate.0);
        let mut groups = [[0i64; 16]; 2];
        let (mut to_project, mut to_last) = (i64::MAX, i64::MAX);
        for (t, def) in rules.technologies.iter().enumerate() {
            if self.known[t] {
                continue;
            }
            let (steps, through) = self.path(t, c);
            let left = steps as i64 - i64::from(through);
            if !def.unlocks_projects.is_empty() {
                to_project = to_project.min(left);
            }
            if def.era == self.last_era {
                to_last = to_last.min(left);
            }
            if t == c || !through || steps > 3 {
                continue;
            }
            for (g, within) in [2usize, 3].into_iter().enumerate() {
                if steps > within {
                    continue;
                }
                let out = &mut groups[g];
                for b in &def.unlocks_buildings {
                    let Some(bd) = rules.buildings.get(usize::from(b.0)) else { continue };
                    if bd.wonder {
                        out[1] += i64::from(!built.contains(b));
                    } else {
                        out[0] += 1;
                    }
                    let a = amounts(&bd.effects);
                    // Science, production, gold, food, culture: flat, then percent.
                    for (i, slot) in [3usize, 1, 2, 0, 4].into_iter().enumerate() {
                        out[5 + i] += a[slot];
                        out[10 + i] += a[6 + slot];
                    }
                }
                out[2] += def.unlocks_units.len() as i64;
                out[3] += def.unlocks_improvements.len() as i64;
                out[4] += def.unlocks_projects.len() as i64;
                let best = def.unlocks_units.iter().filter_map(|u| rules.units.get(usize::from(u.0))).map(|d| i64::from(d.strength.max(d.ranged_strength))).max().unwrap_or(0);
                out[15] = out[15].max(best);
            }
        }
        for (g, start) in [(0, C::La2Buildings as usize), (1, C::La3Buildings as usize)] {
            for (i, v) in groups[g].iter().enumerate() {
                r[start + i] = sat(*v);
            }
        }
        let none = |v: i64| if v == i64::MAX { -1 } else { v };
        set!(r, C::TechsToProject, none(to_project));
        set!(r, C::TechsToLastEra, none(to_last));
    }
}

/// The items `city` may build now: every unit, building and project whose
/// order the rules accept (`EnqueueProduction`), or refuse only because the
/// city has it queued already (the expert clears and orders again). Each
/// counts the other own cities that have it queued or were ordered it this
/// turn (`Turn::note_order`).
pub fn production_candidates(turn: &mut Turn, obs: &AIObservation<'_>, city: &OwnCity, rules: &CompiledRuleset) -> Vec<Candidate> {
    let entity = turn.own_city_row(city, obs, rules);
    let items = (0..rules.units.len())
        .map(|i| ProductionItem::Unit(UnitKindId(i as u16)))
        .chain((0..rules.buildings.len()).map(|i| ProductionItem::Building(BuildingKindId(i as u16))))
        .chain((0..rules.module.projects.len()).map(|i| ProductionItem::Project(crate::rules::content::ProjectId(i as u16))));
    let production = i64::from(city.income.production.max(1));
    let others: Vec<(&[ProductionItem], &[ProductionItem])> = obs
        .own_cities
        .iter()
        .filter(|c| c.handle != city.handle)
        .map(|c| (c.queue.as_slice(), turn.ordered.get(&(c.handle.index, c.handle.generation)).map_or(&[][..], Vec::as_slice)))
        .collect();
    let elsewhere = |item: &ProductionItem| others.iter().filter(|(queue, ordered)| queue.contains(item) || ordered.contains(item)).count();
    let mut out = Vec::new();
    for item in items {
        let command = Command::EnqueueProduction { player: obs.player, city: city.handle, item };
        let queued = match obs.view.refusal(&command) {
            None => false,
            Some(CommandError::Refused(Refusal::AlreadyQueued)) => true,
            Some(_) => continue,
        };
        let (tag, param) = match item {
            ProductionItem::Unit(k) => (tag::PRODUCE_UNIT, k.0),
            ProductionItem::Building(k) => (tag::PRODUCE_BUILDING, k.0),
            ProductionItem::Project(k) => (tag::PRODUCE_PROJECT, k.0),
        };
        let mut c = Candidate::new(tag, Some(command));
        c.meta = [tag, city.tile.0 as i32, entity, i32::from(param)];
        let cost = i64::from(obs.view.own_production_cost(city.handle, item).unwrap_or(0));
        let r = &mut c.row;
        set!(r, C::ItemCost, cost / 10);
        set!(r, C::ItemTurns, (cost / production).min(500));
        set!(r, C::ItemQueued, queued || city.queue.contains(&item));
        set!(r, C::ItemQueuedElsewhere, elsewhere(&item));
        item_fields(r, rules, item);
        out.push(c);
    }
    out
}

/// The table data of a production item (kind one-hot, a unit's strengths
/// and roles, a building's maintenance, the era, the plain effects).
fn item_fields(r: &mut [i16], rules: &CompiledRuleset, item: ProductionItem) {
    match item {
        ProductionItem::Unit(kind) => {
            set!(r, C::ItemUnit, 1);
            if let Some(def) = rules.units.get(usize::from(kind.0)) {
                set!(r, C::ItemStrength, def.strength);
                set!(r, C::ItemRanged, def.ranged_strength);
                set!(r, C::ItemMovement, def.movement);
                set!(r, C::ItemDomainSea, def.domain == Domain::Sea);
                set!(r, C::ItemDomainAir, def.domain == Domain::Air);
                set!(r, C::ItemFounder, def.founder);
                set!(r, C::ItemBuilder, def.builder);
                set!(r, C::ItemEra, era_index(rules, def.era));
                put_amounts(r, &def.effects);
            }
        }
        ProductionItem::Building(kind) => {
            if let Some(def) = rules.buildings.get(usize::from(kind.0)) {
                match def.kind {
                    BuildingKind::WorldWonder => set!(r, C::ItemWorldWonder, 1),
                    BuildingKind::NationalWonder => set!(r, C::ItemNationalWonder, 1),
                    _ if def.wonder => set!(r, C::ItemWorldWonder, 1),
                    _ => set!(r, C::ItemBuilding, 1),
                }
                set!(r, C::ItemMaintenance, def.maintenance);
                set!(r, C::ItemEra, era_index(rules, def.era));
                put_amounts(r, &def.effects);
            }
        }
        ProductionItem::Project(_) => set!(r, C::ItemProject, 1),
    }
}

/// The culture choices `obs`'s player may make now (`CultureOverview`): the
/// ideologies it may choose, the policies and tenets it may adopt, the
/// branches it may unlock — in that order, each by id.
pub fn policy_candidates(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Vec<Candidate> {
    let culture = &obs.culture;
    let player = obs.player;
    let branch_size = |b: PolicyBranchId| rules.module.policies.iter().filter(|p| p.branch == Some(b)).count();
    let adopted = |b: PolicyBranchId| culture.policies.iter().filter(|p| rules.module.policies.get(usize::from(p.0)).and_then(|d| d.branch) == Some(b)).count();
    let branch_fields = |r: &mut [i16], b: PolicyBranchId| {
        set!(r, C::PolBranchSize, branch_size(b));
        set!(r, C::PolBranchAdopted, adopted(b));
        let era = rules.module.policy_branches.get(usize::from(b.0)).and_then(|d| d.era_prerequisite);
        set!(r, C::PolBranchEra, era_index(rules, era));
    };
    let mut out = Vec::new();
    for &(ideology, free) in &culture.ideologies {
        let mut c = Candidate::new(tag::CHOOSE_IDEOLOGY, Some(Command::ChooseIdeology { player, ideology }));
        c.meta[3] = i32::from(ideology.0);
        set!(c.row, C::PolIdeology, 1);
        set!(c.row, C::PolFreeTenets, free);
        branch_fields(&mut c.row, ideology);
        out.push(c);
    }
    for &policy in &culture.adoptable {
        let mut c = Candidate::new(tag::ADOPT_POLICY, Some(Command::AdoptPolicy { player, policy }));
        c.meta[3] = i32::from(policy.0);
        set!(c.row, C::PolAdopt, 1);
        if let Some(def) = rules.module.policies.get(usize::from(policy.0)) {
            if let Some(b) = def.branch {
                branch_fields(&mut c.row, b);
            }
            put_amounts(&mut c.row, &def.effects);
        }
        out.push(c);
    }
    for &branch in &culture.unlockable {
        let mut c = Candidate::new(tag::UNLOCK_BRANCH, Some(Command::UnlockPolicyBranch { player, branch }));
        c.meta[3] = i32::from(branch.0);
        set!(c.row, C::PolBranch, 1);
        branch_fields(&mut c.row, branch);
        let opener = rules.module.policy_branches.get(usize::from(branch.0)).and_then(|d| d.opener);
        if let Some(def) = opener.and_then(|p: PolicyId| rules.module.policies.get(usize::from(p.0))) {
            put_amounts(&mut c.row, &def.effects);
        }
        out.push(c);
    }
    out
}

/// A religious choice of beliefs: one command made of single choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Founding {
    /// `FoundPantheon`.
    Pantheon,
    /// `FoundReligion` by a Great Prophet.
    Religion,
    /// `EnhanceReligion` by a Great Prophet.
    Enhance,
    /// `AddReformationBelief`.
    Reformation,
}

/// The belief slots of a founding as the rules define them for the player
/// now, in decision order (their tags): a pantheon one belief; a religion a
/// founder and a follower belief, a pantheon belief when the player has no
/// pantheon, and a bonus belief when it may add one (`extra_founding_beliefs`);
/// an enhancement a follower and an enhancer belief; a reformation one
/// belief.
pub fn founding_slots(faith: &crate::derived::religion::FaithOverview, founding: Founding) -> Vec<i32> {
    match founding {
        Founding::Pantheon => vec![tag::PANTHEON],
        Founding::Religion => {
            let mut slots = vec![tag::FOUND_FOUNDER, tag::FOUND_FOLLOWER];
            if faith.pantheon.is_none() {
                slots.push(tag::FOUND_PANTHEON);
            }
            if faith.extra_founding_beliefs > 0 {
                slots.push(tag::FOUND_BONUS);
            }
            slots
        }
        Founding::Enhance => vec![tag::ENHANCE_FOLLOWER, tag::ENHANCE_ENHANCER],
        Founding::Reformation => vec![tag::REFORMATION],
    }
}

/// The beliefs one slot may take now, by id: the free beliefs of the slot's
/// category (the rules' own list, `FaithOverview::free_*`); for the bonus
/// slot the free beliefs of every bonus category
/// (`derived::religion::BONUS_BELIEF_CATEGORIES`); never one of the `earlier`
/// picks of the same command. A pantheon is also checked by the rules
/// (`FoundPantheon` accepted).
pub fn belief_candidates(obs: &AIObservation<'_>, rules: &CompiledRuleset, slot: i32, earlier: &[crate::ids::BeliefId]) -> Vec<Candidate> {
    let f = &obs.faith.overview;
    let mut list: Vec<crate::ids::BeliefId> = match slot {
        tag::PANTHEON | tag::FOUND_PANTHEON => f.free_pantheon.clone(),
        tag::FOUND_FOUNDER => f.free_founder.clone(),
        tag::FOUND_FOLLOWER | tag::ENHANCE_FOLLOWER => f.free_follower.clone(),
        tag::ENHANCE_ENHANCER => f.free_enhancer.clone(),
        tag::REFORMATION => f.free_reformation.clone(),
        tag::FOUND_BONUS => f.free_pantheon.iter().chain(&f.free_founder).chain(&f.free_follower).chain(&f.free_enhancer).copied().collect(),
        _ => Vec::new(),
    };
    list.sort_by_key(|b| b.0);
    list.dedup();
    list.retain(|b| !earlier.contains(b));
    if slot == tag::PANTHEON {
        list.retain(|&belief| obs.view.accepts(&Command::FoundPantheon { player: obs.player, belief }));
    }
    let context = BeliefContext::of(obs, rules);
    list.into_iter()
        .map(|belief| {
            let mut c = Candidate::new(slot, None);
            c.meta[3] = i32::from(belief.0);
            if let Some(def) = rules.module.beliefs.get(usize::from(belief.0)) {
                use crate::classic::content::BeliefCategory as B;
                let category = match def.category {
                    Some(B::Pantheon) => Some(C::BelPantheon),
                    Some(B::Founder) => Some(C::BelFounder),
                    Some(B::Follower) => Some(C::BelFollower),
                    Some(B::Enhancer) => Some(C::BelEnhancer),
                    Some(B::Reformation) => Some(C::BelReformation),
                    None => None,
                };
                if let Some(col) = category {
                    set!(c.row, col, 1);
                }
                put_amounts(&mut c.row, &def.effects);
                context.conditions(rules, &def.effects, &mut c.row);
            }
            c
        })
        .collect()
}

/// What a belief's conditions are measured against: the player's worked
/// tiles as it knows them and its cities.
struct BeliefContext<'o, 'a> {
    obs: &'o AIObservation<'a>,
    worked: Vec<crate::knowledge::view::TileSnapshot>,
}

impl<'o, 'a> BeliefContext<'o, 'a> {
    fn of(obs: &'o AIObservation<'a>, _rules: &CompiledRuleset) -> Self {
        let worked = obs.own_cities.iter().flat_map(|c| c.worked.iter()).filter_map(|t| obs.view.tile(*t).snapshot().copied()).collect();
        Self { obs, worked }
    }

    /// Worked tiles meeting a tile condition (None: not a tile condition).
    fn tiles(&self, rules: &CompiledRuleset, condition: &Condition) -> Option<i64> {
        let kinds = self.obs.view.terrain_kinds();
        let test: Box<dyn Fn(&crate::knowledge::view::TileSnapshot) -> bool> = match *condition {
            Condition::TileImprovement(i) => Box::new(move |s| s.improvement == Some(i)),
            Condition::TileResource(r) => Box::new(move |s| s.resource == Some(r)),
            Condition::TileFeature(f) => Box::new(move |s| s.feature == Some(f)),
            Condition::TileFeatureUnimproved(f) => Box::new(move |s| s.feature == Some(f) && s.improvement.is_none()),
            Condition::TileResourceClass(class) => {
                Box::new(move |s| s.resource.and_then(|r| rules.resources.get(usize::from(r.0))).is_some_and(|d| d.class == Some(class)))
            }
            Condition::TileTerrain(k) => {
                let hills = rules.terrains.get(usize::from(k.0)).and_then(|d| d.map_code).is_some_and(|code| kinds.is_hills(code));
                Box::new(move |s| rules.terrain_kind_of(s.terrain) == Some(k) || (hills && kinds.is_hills(s.terrain.0)))
            }
            _ => return None,
        };
        Some(self.worked.iter().filter(|s| test(s)).count() as i64)
    }

    /// Own cities meeting a city condition (None: not a city condition).
    fn cities(&self, condition: &Condition) -> Option<i64> {
        let cities = &self.obs.own_cities;
        let n = match *condition {
            Condition::HasBuilding(b) => cities.iter().filter(|c| c.buildings.contains(&b)).count(),
            Condition::MinPopulation(p) => cities.iter().filter(|c| c.population >= p).count(),
            Condition::CityCoastal => cities.iter().filter(|c| self.obs.view.coastal_land(c.tile)).count(),
            _ => return None,
        };
        Some(n as i64)
    }

    /// The conditional and triggered effects of a table row, summarized: the
    /// flat yields and happiness its conditional effects add, how many
    /// effects are conditional and triggered, and how many of our worked
    /// tiles and cities meet their conditions (with the flat yield those
    /// tiles would add).
    fn conditions(&self, rules: &CompiledRuleset, set: &EffectSet, r: &mut [i16]) {
        let (mut conditional, mut triggered, mut tiles, mut tile_yield, mut cities) = (0i64, 0i64, 0i64, 0i64, 0i64);
        let mut yields = [0i64; 7];
        let passive = set.passive_player.iter().chain(&set.passive_module).chain(&set.passive_city).chain(&set.passive_unit).chain(&set.passive_tile);
        for effect in passive.chain(set.triggered.iter().flatten()) {
            if !matches!(effect.trigger, Trigger::Passive) {
                triggered += 1;
            }
            let conditions = [&effect.condition, &effect.second_condition];
            if conditions.iter().all(|c| matches!(c, Condition::Always)) {
                continue;
            }
            conditional += 1;
            let amount = match effect.modifier {
                Modifier::AddYield { yield_kind, amount } => {
                    yields[slot(yield_kind)] += i64::from(amount);
                    i64::from(amount)
                }
                Modifier::Module(crate::classic::effects::ClassicModifier::AddHappiness { amount }) => {
                    yields[6] += i64::from(amount);
                    0
                }
                _ => 0,
            };
            for condition in conditions {
                if let Some(n) = self.tiles(rules, condition) {
                    tiles += n;
                    tile_yield += n * amount;
                } else if let Some(n) = self.cities(condition) {
                    cities += n;
                }
            }
        }
        for (i, v) in yields.into_iter().enumerate() {
            r[C::CondFood as usize + i] = sat(v);
        }
        set!(r, C::Conditional, conditional);
        set!(r, C::Triggered, triggered);
        set!(r, C::TilesMeeting, tiles);
        set!(r, C::TileYield, tile_yield);
        set!(r, C::CitiesMeeting, cities);
    }
}

/// The command a founding's picks make (`(slot tag, belief)` in slot order):
/// the religion is the first of the table nobody founded
/// (`FaithOverview::free_religions`, as the scripted mind takes it: names
/// are cosmetic); a slot without a pick is left out where the rules allow
/// it (the pantheon and bonus beliefs). None when a required pick, the
/// prophet or a free religion is missing.
pub fn assemble_founding(obs: &AIObservation<'_>, founding: Founding, unit: Option<UnitHandle>, picks: &[(i32, crate::ids::BeliefId)]) -> Option<Command> {
    let pick = |slot: i32| picks.iter().find(|(s, _)| *s == slot).map(|(_, b)| *b);
    let player = obs.player;
    Some(match founding {
        Founding::Pantheon => Command::FoundPantheon { player, belief: pick(tag::PANTHEON)? },
        Founding::Religion => Command::FoundReligion {
            unit: unit?,
            religion: *obs.faith.overview.free_religions.first()?,
            founder: pick(tag::FOUND_FOUNDER)?,
            follower: pick(tag::FOUND_FOLLOWER)?,
            pantheon: pick(tag::FOUND_PANTHEON),
            bonus: pick(tag::FOUND_BONUS),
        },
        Founding::Enhance => Command::EnhanceReligion { unit: unit?, follower: pick(tag::ENHANCE_FOLLOWER)?, enhancer: pick(tag::ENHANCE_ENHANCER)? },
        Founding::Reformation => Command::AddReformationBelief { player, belief: pick(tag::REFORMATION)? },
    })
}

/// A founding command's kind, prophet, religion and picks by slot.
pub type Split = (Founding, Option<UnitHandle>, Option<crate::rules::content::ReligionId>, Vec<(i32, crate::ids::BeliefId)>);

/// A founding command as its single choices (`assemble_founding`
/// backwards): its kind, its prophet, its religion and its picks by slot.
pub fn split_founding(command: &Command) -> Option<Split> {
    Some(match *command {
        Command::FoundPantheon { belief, .. } => (Founding::Pantheon, None, None, vec![(tag::PANTHEON, belief)]),
        Command::FoundReligion { unit, religion, founder, follower, pantheon, bonus } => {
            let mut picks = vec![(tag::FOUND_FOUNDER, founder), (tag::FOUND_FOLLOWER, follower)];
            picks.extend(pantheon.map(|b| (tag::FOUND_PANTHEON, b)));
            picks.extend(bonus.map(|b| (tag::FOUND_BONUS, b)));
            (Founding::Religion, Some(unit), Some(religion), picks)
        }
        Command::EnhanceReligion { unit, follower, enhancer } => (Founding::Enhance, Some(unit), None, vec![(tag::ENHANCE_FOLLOWER, follower), (tag::ENHANCE_ENHANCER, enhancer)]),
        Command::AddReformationBelief { belief, .. } => (Founding::Reformation, None, None, vec![(tag::REFORMATION, belief)]),
        _ => return None,
    })
}

/// The faith purchases the player may make now: every unit and building
/// each own city may buy with faith (`PurchaseWithFaith` accepted), its
/// faith price and table data; then keeping the faith (`FAITH_SAVE`, no
/// command). Only the save when nothing may be bought.
pub fn faith_candidates(turn: &mut Turn, obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Vec<Candidate> {
    let mut out = Vec::new();
    let per_turn = i64::from(obs.faith.overview.per_turn.max(1));
    if obs.faith.overview.faith > 0 {
        let items: Vec<ProductionItem> = (0..rules.units.len())
            .map(|i| ProductionItem::Unit(UnitKindId(i as u16)))
            .chain((0..rules.buildings.len()).map(|i| ProductionItem::Building(BuildingKindId(i as u16))))
            .collect();
        for city in &obs.own_cities {
            let mut entity = None;
            for &item in &items {
                let Some(price) = obs.view.faith_price(city.handle, item) else { continue };
                let command = Command::PurchaseWithFaith { player: obs.player, city: city.handle, item };
                if !obs.view.accepts(&command) {
                    continue;
                }
                let row = *entity.get_or_insert_with(|| turn.own_city_row(city, obs, rules));
                let (code, id) = match item {
                    ProductionItem::Unit(k) => (tag::FAITH_UNIT, k.0),
                    ProductionItem::Building(k) => (tag::FAITH_BUILDING, k.0),
                    ProductionItem::Project(k) => (tag::FAITH_BUILDING, k.0),
                };
                let mut c = Candidate::new(code, Some(command));
                c.meta = [code, city.tile.0 as i32, row, i32::from(id)];
                set!(c.row, C::FaithPrice, price / 10);
                set!(c.row, C::ItemTurns, (i64::from(price) / per_turn).min(500));
                item_fields(&mut c.row, rules, item);
                out.push(c);
            }
        }
    }
    let mut save = Candidate::new(tag::FAITH_SAVE, None);
    set!(save.row, C::FaithSave, 1);
    out.push(save);
    out
}

/// The citizen settings one own city may take now, one group per setting:
/// its focus (every `CityFocus`, described by the rules' assignment weights
/// of food, production, gold, science, culture and faith in the `effect_*`
/// columns), its avoid growth (off, on), and per specialist kind the city
/// has slots for (its buildings' `specialist_slots`) the hand-assigned
/// count, 0 (the automatic assignment) to the slots (at most
/// `MAX_FORCED_SPECIALISTS`), one specialist's yields in the `effect_*`
/// columns. In every group the current setting is marked by `act_wait` and
/// has no command (keeping it); every other candidate carries the command
/// that sets it, kept only when the rules accept it.
pub fn citizen_candidates(turn: &mut Turn, obs: &AIObservation<'_>, city: &OwnCity, rules: &CompiledRuleset) -> Vec<Vec<Candidate>> {
    use crate::world::citizens::CityFocus;
    let entity = turn.own_city_row(city, obs, rules);
    let player = obs.player;
    let plan = &city.citizens;
    let make = |code: i32, param: i32, current: bool, command: Command| -> Option<Candidate> {
        if !current && !obs.view.accepts(&command) {
            return None;
        }
        let mut c = Candidate::new(code, (!current).then_some(command));
        c.meta = [code, city.tile.0 as i32, entity, param];
        if current {
            set!(c.row, C::ActWait, 1);
        }
        Some(c)
    };
    let mut groups = Vec::new();
    let focus: Vec<Candidate> = CityFocus::ALL
        .into_iter()
        .filter_map(|focus| {
            let mut c = make(tag::FOCUS, i32::from(focus.code()), plan.focus == focus, Command::SetCityFocus { player, city: city.handle, focus })?;
            let w = kernel::derived::citizens::weights(focus);
            for (i, v) in w.iter().take(6).enumerate() {
                c.row[C::EffFood as usize + i] = sat(i64::from(*v));
            }
            Some(c)
        })
        .collect();
    groups.push(focus);
    let growth: Vec<Candidate> =
        [false, true].into_iter().filter_map(|avoid| make(tag::AVOID_GROWTH, i32::from(avoid), plan.avoid_growth == avoid, Command::SetAvoidGrowth { player, city: city.handle, avoid })).collect();
    groups.push(growth);
    for (kind, slots) in specialist_slots(rules, city).into_iter().enumerate() {
        if slots == 0 {
            continue;
        }
        let specialist = crate::rules::content::SpecialistId(kind as u16);
        let most = slots.min(u16::from(crate::world::citizens::MAX_FORCED_SPECIALISTS)) as u8;
        let yields = rules.specialists.get(kind).map(|d| d.yields).unwrap_or_default();
        let group: Vec<Candidate> = (0..=most)
            .filter_map(|count| {
                let mut c = make(tag::SPECIALISTS, kind as i32 * 100 + i32::from(count), plan.forced(specialist) == count, Command::SetSpecialists { player, city: city.handle, specialist, count })?;
                put_yields(&mut c.row, C::EffFood as usize, yields);
                Some(c)
            })
            .collect();
        groups.push(group);
    }
    groups
}

/// Specialist slots of a city per kind (index = `SpecialistId`), from its
/// buildings' table rows.
fn specialist_slots(rules: &CompiledRuleset, city: &OwnCity) -> Vec<u16> {
    let mut slots = vec![0u16; rules.specialists.len()];
    for def in city.buildings.iter().filter_map(|b| rules.buildings.get(usize::from(b.0))) {
        if let Some((kind, count)) = def.specialist_slots {
            if let Some(slot) = slots.get_mut(usize::from(kind.0)) {
                *slot = slot.saturating_add(count);
            }
        }
    }
    slots
}

/// The command a citizen candidate stands for (None: keep the setting).
pub fn citizen_command(candidate: &Candidate) -> Option<Command> {
    candidate.command.clone()
}

/// The parameter of a citizen group's setting in `plan` (the expert's
/// choice is the candidate with it).
pub fn citizen_param(plan: &crate::world::citizens::CitizenPlan, group: &[Candidate]) -> Option<i32> {
    let first = group.first()?;
    Some(match first.meta[0] {
        tag::FOCUS => i32::from(plan.focus.code()),
        tag::AVOID_GROWTH => i32::from(plan.avoid_growth),
        tag::SPECIALISTS => {
            let kind = first.meta[3] / 100;
            kind * 100 + i32::from(plan.forced(crate::rules::content::SpecialistId(kind as u16)))
        }
        _ => return None,
    })
}

/// Where the expert's long march lands among the candidates: the march to
/// its destination, else the nearest march within `MARCH_NEAR` of it (the
/// first on a tie). None: no point of interest is near.
pub fn place_march(candidates: &[Candidate], destination: TileId, dims: MapDimensions) -> Option<(usize, bool)> {
    let marches = candidates.iter().enumerate().filter(|(_, c)| c.meta[0] == tag::MARCH && c.meta[1] >= 0);
    let mut best: Option<(i32, usize)> = None;
    for (i, c) in marches {
        let Some(d) = dims.distance(TileId(c.meta[1] as u32), destination) else { continue };
        if d <= MARCH_NEAR && best.is_none_or(|(b, _)| d < b) {
            best = Some((d, i));
        }
    }
    best.map(|(d, i)| (i, d == 0))
}

/// The tag of a unit command among the candidates (the codec tag, `MARCH`
/// for a long march; None: a kind the candidates never hold).
pub fn unit_tag(command: &Command) -> Option<i32> {
    match command {
        Command::MarchTo { .. } => Some(i32::from(MARCH_TAG)),
        _ => codec_tag(command).map(i32::from),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_names_of_every_table_are_unique() {
        for table in [T::FIELDS, G::FIELDS, E::FIELDS, C::FIELDS] {
            let mut names: Vec<&str> = table.iter().map(|(n, _)| *n).collect();
            names.sort_unstable();
            let before = names.len();
            names.dedup();
            assert_eq!(names.len(), before);
        }
    }

    #[test]
    fn neighbours_follow_the_kernel_and_regions_tile_the_map() {
        let dims = MapDimensions { width: 9, height: 6 };
        let table = neighbor_table(dims);
        for (n, row) in table.iter().enumerate() {
            let on_map: Vec<i32> = dims.neighbors(TileId(n as u32)).into_iter().map(|t| t.0 as i32).collect();
            let listed: Vec<i32> = row.iter().copied().filter(|t| *t >= 0).collect();
            assert_eq!(listed, on_map, "tile {n}");
        }
        let (regions, count) = region_table(dims);
        assert_eq!(count, 3 * 2);
        assert_eq!(regions[0], 0);
        assert_eq!(regions[8], 2);
        assert_eq!(regions[4 * 9], 3);
        assert!(regions.iter().all(|r| (*r as usize) < count));
    }
}
