//! Great people, the read side (classic/docs/classic/rules.md): the
//! actions a great person can take, birth thresholds, the points a city
//! makes and whether an action is allowed now. The writes (births, actions)
//! are in `systems::great_people`.

use crate::command::{CommandError, ResolvedCommand};
use crate::world::city::MAX_SPECIALIST_KINDS;
use crate::derived::yields::CityBreakdown;
use crate::ids::{ImprovementId, PlayerId, UnitHandle};
use crate::classic::tables::GreatPersonDefinition;
use crate::rules::content::PointsSource;
use crate::classic::effects::GreatPersonKind;
use crate::rules::stats::{Stat, StatTotals};
use crate::rules::CompiledRuleset;
use crate::world::builds::{site, Builder};
use crate::world::facts::TruthFacts;
use crate::world::player_economy::PlayerEconomy;
use crate::world::state::AuthWorld;
use crate::world::player::ClassicPlayers;

/// Points of the first great person of a kind (Standard speed). Unciv:
/// GreatPersonManager, equal (100).
pub const POINTS_FOR_NEXT_GREAT_PERSON: u32 = 100;
/// The threshold doubles for each great person of the kind born (100, 200, 400, ...).
/// Unciv: GreatPersonManager (`pointsForNextGreatPersonCounter *= 2`), equal.
pub const POINTS_FOR_NEXT_GREAT_PERSON_FACTOR: u32 = 2;
/// Shortest golden age a great artist can start, in turns: a guard for a rules file that gives the unit no length. Own design.
pub const MIN_GREAT_PERSON_GOLDEN_AGE_TURNS: u16 = 3;

/// What a great person does (each action uses the unit up).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GreatPersonAction {
    /// Build its improvement on the tile it stands on.
    Construct(ImprovementId),
    /// Great scientist: science of the player's last turns.
    DiscoverTechnology,
    /// Great engineer: production in the city it stands in.
    HurryProduction,
    /// Great artist: a golden age.
    StartGoldenAge,
    /// Great merchant: gold (and influence) in a city-state.
    TradeMission,
    /// Merchant of Venice: the city-state whose land it stands on becomes
    /// its owner's puppet.
    BuyCityState,
}

/// Born-so-far counter that sets the threshold of a kind.
fn created(economy: &PlayerEconomy, kind: GreatPersonKind) -> u32 {
    let n = match kind {
        GreatPersonKind::Writer => economy.great_writers_created,
        GreatPersonKind::Artist => economy.great_artists_created,
        GreatPersonKind::Musician => economy.great_musicians_created,
        GreatPersonKind::Scientist
        | GreatPersonKind::Engineer
        | GreatPersonKind::Merchant
        | GreatPersonKind::General
        | GreatPersonKind::Admiral
        | GreatPersonKind::Prophet => economy.great_people_created,
    };
    u32::from(n)
}

pub(crate) fn count_birth(economy: &mut PlayerEconomy, kind: GreatPersonKind) {
    let counter = match kind {
        GreatPersonKind::Writer => &mut economy.great_writers_created,
        GreatPersonKind::Artist => &mut economy.great_artists_created,
        GreatPersonKind::Musician => &mut economy.great_musicians_created,
        GreatPersonKind::Scientist
        | GreatPersonKind::Engineer
        | GreatPersonKind::Merchant
        | GreatPersonKind::General
        | GreatPersonKind::Admiral
        | GreatPersonKind::Prophet => &mut economy.great_people_created,
    };
    *counter = counter.saturating_add(1);
}

/// Points a player needs for its next great person of `kind`, × the game
/// speed's pace (Unciv: GreatPersonManager.getPointsRequiredForGreatPerson,
/// `modifier`; equal; generals and admirals, whose points come from combat,
/// are not scaled there).
pub fn great_person_threshold(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId, kind: GreatPersonKind) -> u32 {
    let born = auth
        .players()
        .economy_of(player)
        .map(|e| created(e, kind))
        .unwrap_or(0);
    let mut points = POINTS_FOR_NEXT_GREAT_PERSON;
    for _ in 0..born.min(24) {
        points = points.saturating_mul(POINTS_FOR_NEXT_GREAT_PERSON_FACTOR);
    }
    if matches!(kind, GreatPersonKind::General | GreatPersonKind::Admiral) {
        return points;
    }
    kernel::rules::speeds::SpeedValues::scale(points, auth.settings().speed(rules).pace_percent).max(1)
}

/// Great person points a city adds this turn per specialist kind
/// (hundredths; index = `SpecialistId`).
pub fn city_points(
    rules: &CompiledRuleset,
    yields: &CityBreakdown,
    player_rate: &StatTotals,
) -> [u32; MAX_SPECIALIST_KINDS] {
    let mut out = [0u32; MAX_SPECIALIST_KINDS];
    for def in &rules.module.great_people {
        if def.points_source != PointsSource::Specialists {
            continue;
        }
        let Some(specialist) = def.specialist else {
            continue;
        };
        let Some(spec) = rules.specialists.get(specialist.0 as usize) else {
            continue;
        };
        let count = u32::from(
            yields
                .work
                .specialists
                .get(specialist.0 as usize)
                .copied()
                .unwrap_or(0),
        );
        let buildings = yields.modifiers.module.great_person_points[def.kind.index()].max(0) as u32;
        let base = (u32::from(spec.module.great_person_points) * count + buildings) * 100;
        if base == 0 {
            continue;
        }
        let rate =
            rate_percent(&yields.modifiers.stats, def.kind) + rate_percent(player_rate, def.kind);
        let points = i64::from(base) * i64::from(100 + rate).max(0) / 100;
        if let Some(slot) = out.get_mut(specialist.0 as usize) {
            *slot = points.min(i64::from(u32::MAX)) as u32;
        }
    }
    out
}

fn rate_percent(stats: &StatTotals, kind: GreatPersonKind) -> i32 {
    stats.get(Stat::Module(crate::rules::stats::ClassicStat::GreatPeopleRate)) + stats.get(Stat::Module(crate::rules::stats::ClassicStat::GreatPersonRate(kind)))
}

/// Percent of great people points a player gains from its declarations of
/// friendship (rules "Diplomacy" §5.9, Sweden's trait): for each friend, its
/// own per-friend rate and the friend's (unverified whether the bonus is meant
/// for "Sweden and its friend"; the rule function reads the player's own trait).
pub fn friendship_rate(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId) -> i32 {
    friendship_rate_with(auth, player, |p| friend_stat(auth, rules, p))
}

/// `friendship_rate` with the per-friend stat given by `stat_of`.
pub fn friendship_rate_with(auth: &AuthWorld, player: PlayerId, stat_of: impl Fn(PlayerId) -> i32) -> i32 {
    let round = auth.turn().round;
    let relations = auth.module().relations();
    let mut own: Option<i32> = None;
    let mut total = 0;
    for p in 0..auth.players().count() as u8 {
        let friend = PlayerId(p);
        if friend == player || !relations.are_friends(player, friend, round) {
            continue;
        }
        let mine = *own.get_or_insert_with(|| stat_of(player));
        total += mine + stat_of(friend);
    }
    total
}

fn friend_stat(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId) -> i32 {
    let stats = &crate::effects::player::player_modifiers(auth, rules, player).stats;
    stats.get(Stat::Module(crate::rules::stats::ClassicStat::GreatPeopleRatePerFriend))
}

/// `friend_stat` of a modifiers answer.
pub fn friend_stat_of(modifiers: &crate::effects::player::PlayerModifiers) -> i32 {
    modifiers.stats.get(Stat::Module(crate::rules::stats::ClassicStat::GreatPeopleRatePerFriend))
}

/// The great person definition of a unit kind (its own or the one its
/// unique unit replaces).
pub fn great_person_of(
    rules: &CompiledRuleset,
    kind: crate::ids::UnitKindId,
) -> Option<&GreatPersonDefinition> {
    let def = rules.units.get(kind.0 as usize)?;
    rules
        .module.great_people
        .iter()
        .find(|gp| gp.unit == kind || def.replaces == Some(gp.unit))
}

/// Science a great scientist grants: the player's science of its last
/// `turns` turns (Standard speed).
pub fn bulb_science(auth: &AuthWorld, player: PlayerId, turns: u16) -> u32 {
    let Some(economy) = auth.players().economy_of(player) else {
        return 0;
    };
    economy
        .science_history
        .iter()
        .take(turns as usize)
        .fold(0u32, |sum, s| sum.saturating_add(*s))
}

/// A great engineer may hurry the head of the city's queue (rules.md): a building, or a spaceship part when its owner has
/// `PlayerRule::SpaceshipPartHurry` (Spaceflight Pioneers). Units and
/// projects are not hurried.
pub fn engineer_may_hurry(auth: &AuthWorld, rules: &CompiledRuleset, city: &crate::world::city::CityCore) -> bool {
    match city.queue.first() {
        Some(crate::ids::ProductionItem::Building(_)) => true,
        Some(crate::ids::ProductionItem::Unit(kind)) => {
            let part = rules
                .units
                .get(kind.0 as usize)
                .is_some_and(|d| d.role == Some(crate::rules::content::UnitRole::SpaceshipPart));
            if !part {
                return false;
            }
            let stats = &crate::effects::player::player_modifiers(auth, rules, city.owner).stats;
            crate::rules::stats::has_rule(&stats, crate::rules::stats::PlayerRule::SpaceshipPartHurry)
        }
        Some(crate::ids::ProductionItem::Project(_)) | None => false,
    }
}

/// Why a great person may not act (a build site that refuses it speaks for itself).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GreatPersonBlock {
    NotAGreatPerson,
    WrongImprovement,
    CannotDiscover,
    CannotHurry,
    NotInCity,
    NotYourCity,
    NothingToHurry,
    BuildingsOnly,
    CannotStartGoldenAge,
    CannotTrade,
    CannotBuyCityState,
    NeedsCity,
    NeedsCityStateLand,
    NotMet,
    AtWar,
}

impl GreatPersonBlock {
    pub const ALL: [GreatPersonBlock; 15] = [
        GreatPersonBlock::NotAGreatPerson,
        GreatPersonBlock::WrongImprovement,
        GreatPersonBlock::CannotDiscover,
        GreatPersonBlock::CannotHurry,
        GreatPersonBlock::NotInCity,
        GreatPersonBlock::NotYourCity,
        GreatPersonBlock::NothingToHurry,
        GreatPersonBlock::BuildingsOnly,
        GreatPersonBlock::CannotStartGoldenAge,
        GreatPersonBlock::CannotTrade,
        GreatPersonBlock::CannotBuyCityState,
        GreatPersonBlock::NeedsCity,
        GreatPersonBlock::NeedsCityStateLand,
        GreatPersonBlock::NotMet,
        GreatPersonBlock::AtWar,
    ];

    pub fn reason(self) -> &'static str {
        match self {
            GreatPersonBlock::NotAGreatPerson => "not a great person",
            GreatPersonBlock::WrongImprovement => "not this great person's improvement",
            GreatPersonBlock::CannotDiscover => "cannot discover",
            GreatPersonBlock::CannotHurry => "cannot hurry",
            GreatPersonBlock::NotInCity => "not in a city",
            GreatPersonBlock::NotYourCity => "not your city",
            GreatPersonBlock::NothingToHurry => "nothing to hurry",
            GreatPersonBlock::BuildingsOnly => "a great engineer hurries buildings",
            GreatPersonBlock::CannotStartGoldenAge => "cannot start a golden age",
            GreatPersonBlock::CannotTrade => "cannot trade",
            GreatPersonBlock::CannotBuyCityState => "cannot buy a city-state",
            GreatPersonBlock::NeedsCity => "needs a city",
            GreatPersonBlock::NeedsCityStateLand => "needs a city-state's land",
            GreatPersonBlock::NotMet => "not met",
            GreatPersonBlock::AtWar => "at war",
        }
    }
}

impl From<GreatPersonBlock> for CommandError {
    fn from(block: GreatPersonBlock) -> Self {
        CommandError::RuleViolation(block.reason().into())
    }
}

/// Can `unit` do `action` now?
pub fn validate_action(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    unit: UnitHandle,
    action: GreatPersonAction,
) -> Result<ResolvedCommand, CommandError> {
    let view = auth.units().get(unit).ok_or(CommandError::UnknownUnit)?;
    let core = *view.core;
    let gp = great_person_of(rules, core.kind).ok_or(GreatPersonBlock::NotAGreatPerson)?;
    let abilities = rules.units[core.kind.0 as usize].module.economy;
    match action {
        GreatPersonAction::Construct(improvement) => {
            if !gp.improvements.contains(&improvement) {
                return Err(GreatPersonBlock::WrongImprovement.into());
            }
            let facts = TruthFacts::new(auth, rules, core.owner);
            site(
                &facts,
                rules,
                &Builder::of(auth, core.owner),
                core.tile,
                improvement,
            )
            .map_err(|reason| CommandError::RuleViolation(reason.into()))?;
        }
        GreatPersonAction::DiscoverTechnology => {
            if abilities.beakers_turns == 0 {
                return Err(GreatPersonBlock::CannotDiscover.into());
            }
        }
        GreatPersonAction::HurryProduction => {
            if abilities.base_hurry == 0 {
                return Err(GreatPersonBlock::CannotHurry.into());
            }
            let city = auth
                .indexes()
                .city_on_tile(core.tile)
                .and_then(|c| auth.cities().get(c))
                .ok_or(GreatPersonBlock::NotInCity)?;
            if city.owner != core.owner {
                return Err(GreatPersonBlock::NotYourCity.into());
            }
            if city.queue.is_empty() {
                return Err(GreatPersonBlock::NothingToHurry.into());
            }
            if !engineer_may_hurry(auth, rules, city) {
                return Err(GreatPersonBlock::BuildingsOnly.into());
            }
        }
        GreatPersonAction::StartGoldenAge => {
            if abilities.golden_age_turns == 0 {
                return Err(GreatPersonBlock::CannotStartGoldenAge.into());
            }
        }
        GreatPersonAction::TradeMission => {
            if abilities.base_gold == 0 {
                return Err(GreatPersonBlock::CannotTrade.into());
            }
            trade_mission_city_state(auth, core.owner, core.tile)?;
        }
        GreatPersonAction::BuyCityState => {
            let def = &rules.units[core.kind.0 as usize];
            if !def.module.abilities.contains(&crate::classic::content::UnitAbility::BuyCityState) {
                return Err(GreatPersonBlock::CannotBuyCityState.into());
            }
            trade_mission_city_state(auth, core.owner, core.tile)?;
            if auth.indexes().cities_of(core.owner).is_empty() {
                return Err(GreatPersonBlock::NeedsCity.into());
            }
        }
    }
    Ok(ResolvedCommand::GreatPersonAction { unit, action })
}

/// The city-state whose land a trade mission at `tile` serves: alive, met
/// and at peace with `owner` (rules "City-states" §4.11).
pub fn trade_mission_city_state(
    auth: &AuthWorld,
    owner: PlayerId,
    tile: crate::ids::TileId,
) -> Result<PlayerId, GreatPersonBlock> {
    let land = auth.map().owner(tile).flatten();
    let Some(city_state) = land.filter(|p| auth.players().is_city_state(*p) && auth.players().is_alive(*p)) else {
        return Err(GreatPersonBlock::NeedsCityStateLand.into());
    };
    if !crate::systems::diplomacy::has_met(auth, owner, city_state) {
        return Err(GreatPersonBlock::NotMet.into());
    }
    if auth.diplomacy().at_war(owner, city_state) {
        return Err(GreatPersonBlock::AtWar.into());
    }
    Ok(city_state)
}

/// Gold of a trade mission (§4.11): base + per era of the owner, × the game
/// speed's gold percent (Unciv: UnitActionsGreatPerson, `goldCostModifier`), times `100 + the trade mission gold percent` (Entrepreneurship) and
/// times `100 + unit_percent` (the unit's own trade mission gold percent).
pub fn trade_mission_gold(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    owner: PlayerId,
    abilities: crate::rules::content::UnitAbilities,
    unit_percent: i32,
) -> u32 {
    let era = u32::from(crate::world::player::player_era(auth, rules, owner));
    let gold = abilities.base_gold + abilities.gold_per_era * era;
    let gold = kernel::rules::speeds::SpeedValues::scale(gold, auth.settings().speed(rules).gold_percent);
    let percent = crate::systems::city_states::stat(
        auth,
        rules,
        owner,
        crate::classic::effects::CityStateStat::TradeMissionGoldPercent,
    );
    let policy = i64::from(gold) * i64::from((100 + percent).max(0)) / 100;
    (policy * i64::from((100 + unit_percent).max(0)) / 100) as u32
}
