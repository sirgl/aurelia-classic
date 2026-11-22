//! Situations made of changes. `plan` reads a position and decides the roles and the base (what both
//! arms share); `edit_of` decides the edit of one strength. Neither depends on the other's level, so
//! the control arm of an instance is played once for all its levels.

use std::collections::BTreeMap;

use classic_rules::ids::{PlayerId, TileId};
use classic_rules::{ClassicGame, CompiledRuleset, GameState};
use serde::{Deserialize, Serialize};

use super::edit::{self, Change, Gift, Yield};
use super::play::Arm;
use super::seat::Script;
use super::position::{mix, Position};
use super::Situation;
use crate::battle::{self, Arm as Kind};
use crate::bench::modules::majors;

/// Who matters in a situation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Roles {
    /// `weak` loses its army; `near` is the neighbour on its landmass whose reaction is measured.
    Neighbour { weak: u8, near: u8 },
    /// `enemy`'s units appear inside `seat`'s land, in a war.
    Invasion { seat: u8, enemy: u8 },
    /// `leader`'s science is multiplied; the other nations are measured.
    Runaway { leader: u8 },
    /// `victim`'s capital (city id `capital`) goes to `taker`.
    Lost { victim: u8, taker: u8, capital: u64 },
    /// `leader` holds the spaceship but for some parts; the others are measured.
    Race { leader: u8 },
    /// `a` and `b` are at war with `enemy`.
    Common { a: u8, b: u8, enemy: u8 },
    /// The congress host `host` is gone.
    Host { host: u8 },
    /// A random edit (`random`): only the outcome of `seat` is judged.
    Seat { seat: u8 },
    /// `enemy`'s army stands at `seat`'s border or inside it at peace; its war starts `war_after`
    /// rounds into the window (0: at once).
    Sneak { seat: u8, enemy: u8, war_after: u32 },
    /// `seat` is weakened (its army, its resources, its treasury); its own outcome is judged.
    Weak { seat: u8 },
    /// The world wonder `building` that `seat`'s city `city` builds is completed by `sniper`.
    Snipe { seat: u8, sniper: u8, city: u64, building: u16 },
    /// `grabber` settles at `seat`'s border on `tile` and raises citadels next to it.
    Grab { seat: u8, grabber: u8, tile: u32 },
    /// `raider`'s units hunt `seat`'s civilians and improvements (a script).
    Raid { seat: u8, raider: u8 },
    /// `trickster`, a friend, sells to `seat`, then declares war (a script).
    Trick { seat: u8, trickster: u8 },
    /// `seat` and `ally` hold a defensive pact; `enemy` declares war on `seat`.
    Ally { seat: u8, ally: u8, enemy: u8 },
    /// `seat` is losing a war to `enemy`: cities and army gone.
    Losing { seat: u8, enemy: u8 },
    /// `defender`'s kill-zone stands by its city nearest to `seat`, at war with it (a script).
    Zone { seat: u8, defender: u8 },
    /// `hunter`'s ships hunt `seat`'s units at sea (a script).
    Hunt { seat: u8, hunter: u8 },
    /// `seat` and `enemy` at war with aircraft.
    Air { seat: u8, enemy: u8 },
    /// `pest`'s missionaries or spies work on `seat` (a script).
    Pest { seat: u8, pest: u8 },
}

impl Roles {
    /// The nations whose outcome the situation judges.
    pub fn focus(&self, game: &GameState) -> Vec<PlayerId> {
        let others = |gone: u8| majors(game).into_iter().filter(|p| p.0 != gone).collect();
        match *self {
            Roles::Neighbour { near, .. } => vec![PlayerId(near)],
            Roles::Invasion { seat, .. } => vec![PlayerId(seat)],
            Roles::Runaway { leader } | Roles::Race { leader } => others(leader),
            Roles::Lost { victim, .. } => vec![PlayerId(victim)],
            Roles::Common { a, b, .. } => vec![PlayerId(a), PlayerId(b)],
            Roles::Host { host } => others(host),
            Roles::Seat { seat }
            | Roles::Sneak { seat, .. }
            | Roles::Weak { seat }
            | Roles::Snipe { seat, .. }
            | Roles::Grab { seat, .. }
            | Roles::Raid { seat, .. }
            | Roles::Trick { seat, .. }
            | Roles::Ally { seat, .. }
            | Roles::Losing { seat, .. }
            | Roles::Zone { seat, .. }
            | Roles::Hunt { seat, .. }
            | Roles::Air { seat, .. }
            | Roles::Pest { seat, .. } => vec![PlayerId(seat)],
        }
    }
}

/// What both arms share, and the roles.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub roles: Roles,
    pub base: Vec<Change>,
    pub info: BTreeMap<String, String>,
}

/// The edit of one strength.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Edit {
    pub changes: Vec<Change>,
    pub gift: Option<Gift>,
    /// The scripted adversary of the edited arm.
    pub script: Option<Script>,
}

impl Situation {
    /// The strengths of the edit: 0 mild, 1 medium, 2 strong; a situation with no strength has one.
    pub fn levels(self) -> u8 {
        match self {
            Situation::D1 | Situation::C5 | Situation::P7 | Situation::D2 | Situation::P7C => 1,
            _ => 3,
        }
    }
}

/// The roles and the base of `situation` on this position.
pub fn plan(situation: Situation, rules: &CompiledRuleset, position: &Position, instance: usize) -> Result<Plan, String> {
    let game = &position.game;
    let flip = instance % 2 == 1;
    let mut info = BTreeMap::new();
    let (roles, base) = match situation {
        Situation::A5 => {
            let (a, b, d) = nearest_pair(game).ok_or("no two nations share a landmass")?;
            info.insert("pair_distance".to_owned(), d.to_string());
            let (weak, near) = if flip { (b, a) } else { (a, b) };
            (Roles::Neighbour { weak: weak.0, near: near.0 }, Vec::new())
        }
        Situation::B1 => {
            let (a, b, base) = war_pair(game, &mut info)?;
            let (seat, enemy) = if flip { (b, a) } else { (a, b) };
            (Roles::Invasion { seat: seat.0, enemy: enemy.0 }, base)
        }
        Situation::A1 => (Roles::Runaway { leader: top_science(game, rules).0 }, Vec::new()),
        Situation::B4 => {
            let (victim, taker, capital) = capital_and_taker(game, rules, flip)?;
            let base = vec![Change::Meet { a: victim.0, b: taker.0 }, Change::War { a: taker.0, b: victim.0 }];
            (Roles::Lost { victim: victim.0, taker: taker.0, capital }, base)
        }
        Situation::B9 => {
            let leader = top_science(game, rules);
            let mut scratch = game.clone();
            let techs = crate::selfplay::give_space_techs(&mut scratch, rules, leader);
            info.insert("space_techs_given".to_owned(), techs.len().to_string());
            (Roles::Race { leader: leader.0 }, vec![Change::Discover { player: leader.0, techs }])
        }
        Situation::D1 => {
            let (enemy, a, b) = common_enemy(game)?;
            let base = vec![Change::Meet { a: a.0, b: enemy.0 }, Change::Meet { a: b.0, b: enemy.0 }, Change::Meet { a: a.0, b: b.0 }];
            (Roles::Common { a: a.0, b: b.0, enemy: enemy.0 }, base)
        }
        Situation::C5 => {
            let host = game.view().auth.module().congress().host().filter(|_| game.view().auth.module().congress().founded()).ok_or("the congress is not founded")?;
            (Roles::Host { host: host.0 }, Vec::new())
        }
        later if later.wave() == 3 => return super::wave3::plan(later, rules, position, instance),
        wave2 => return super::tricks::plan(wave2, rules, position, instance),
    };
    Ok(Plan { roles, base, info })
}

/// The edit of `situation` at `level` on the position.
pub fn edit_of(situation: Situation, level: u8, rules: &CompiledRuleset, position: &Position, plan: &Plan) -> Result<Edit, String> {
    match situation.wave() {
        2 => return super::tricks::edit_of(situation, level, rules, position, plan),
        3 => return super::wave3::edit_of(situation, level, rules, position, plan),
        _ => {}
    }
    let game = &position.game;
    let salt = mix(position.round as u64 ^ u64::from(level) << 8);
    let l = usize::from(level.min(2));
    let changes = match plan.roles {
        Roles::Neighbour { weak, .. } => {
            let percent = [50, 75, 100][l];
            let ids = edit::military_units(game, rules, PlayerId(weak)).into_iter().filter(|id| percent == 100 || mix(*id ^ salt) % 100 < percent).collect();
            vec![Change::RemoveUnits { ids }]
        }
        Roles::Invasion { seat, enemy } => invaders(game, rules, PlayerId(seat), PlayerId(enemy), [4, 7, 10][l], salt)?,
        Roles::Runaway { leader } => return Ok(Edit { changes: Vec::new(), gift: Some(Gift { player: leader, percent: [25, 50, 100][l], yield_kind: Yield::Science }), script: None }),
        Roles::Lost { victim, taker, capital } => lost_cities(game, PlayerId(victim), PlayerId(taker), capital, 1 + l),
        Roles::Race { leader } => spaceship(rules, PlayerId(leader), [3, 2, 1][l]),
        Roles::Common { a, b, enemy } => vec![Change::War { a: a, b: enemy }, Change::War { a: b, b: enemy }],
        Roles::Host { host } => remove_nation(game, PlayerId(host))?,
        other => return Err(format!("{} has no wave-1 edit for {other:?}", situation.name())),
    };
    Ok(Edit { changes, gift: None, script: None })
}

/// The two arms: the control (the base) and the edited (the base and the edit); the ids of the units
/// the edit spawned are followed in the edited arm.
pub fn arms(rules: &CompiledRuleset, position: &Position, plan: &Plan, edit: &Edit) -> Result<(Arm, Arm), String> {
    let mut control = position.game.clone();
    edit::apply_all(&mut control, rules, &plan.base)?;
    let mut edited = control.clone();
    let spawned = edit::apply_all(&mut edited, rules, &edit.changes)?;
    let control = Arm { game: control, gift: None, tracked: Vec::new(), later: edit::later(&plan.base), script: None };
    let script = edit.script.clone().map(|s| s.with_units(&spawned));
    let edited = Arm { game: edited, gift: edit.gift, tracked: spawned, later: [edit::later(&plan.base), edit::later(&edit.changes)].concat(), script };
    Ok((control, edited))
}

// ---------------------------------------------------------------------------
// Reading the position

pub(super) fn city_tiles(game: &GameState, p: PlayerId) -> Vec<TileId> {
    let auth = game.view().auth;
    auth.indexes().cities_of(p).iter().filter_map(|h| auth.cities().get(*h)).map(|c| c.tile).collect()
}

pub(super) fn distance(game: &GameState, a: TileId, b: TileId) -> i32 {
    game.view().auth.map().dimensions.distance(a, b).unwrap_or(i32::MAX)
}

/// The nearest city of `from` to `tile`, on the same landmass, with the distance.
pub(super) fn nearest_on_land(game: &GameState, from: PlayerId, tile: TileId) -> Option<i32> {
    let map = game.view().auth.map();
    city_tiles(game, from).into_iter().filter(|c| map.land_area(*c) == map.land_area(tile)).map(|c| distance(game, c, tile)).min()
}

/// The two living nations whose cities are nearest on one landmass (ties: lower ids), the distance.
pub(super) fn nearest_pair(game: &GameState) -> Option<(PlayerId, PlayerId, i32)> {
    let all = majors(game);
    let mut best: Option<(i32, PlayerId, PlayerId)> = None;
    for a in &all {
        for b in all.iter().filter(|b| b.0 > a.0) {
            let near = city_tiles(game, *a).into_iter().filter_map(|t| nearest_on_land(game, *b, t)).min();
            if let Some(d) = near.filter(|d| best.is_none_or(|(bd, ..)| *d < bd)) {
                best = Some((d, *a, *b));
            }
        }
    }
    best.map(|(d, a, b)| (a, b, d))
}

/// Two nations at war now (the first pair by ids), else the nearest pair put at war (`War` and `Meet`
/// in the base: both arms have it).
fn war_pair(game: &GameState, info: &mut BTreeMap<String, String>) -> Result<(PlayerId, PlayerId, Vec<Change>), String> {
    let all = majors(game);
    let diplomacy = game.view().auth.diplomacy();
    for a in &all {
        for b in all.iter().filter(|b| b.0 > a.0) {
            if diplomacy.at_war(*a, *b) {
                info.insert("war".to_owned(), "real".to_owned());
                return Ok((*a, *b, Vec::new()));
            }
        }
    }
    let (a, b, _) = nearest_pair(game).ok_or("no two nations share a landmass")?;
    info.insert("war".to_owned(), "forced".to_owned());
    Ok((a, b, vec![Change::Meet { a: a.0, b: b.0 }, Change::War { a: a.0, b: b.0 }]))
}

fn science_of(game: &GameState, rules: &CompiledRuleset, p: PlayerId) -> i64 {
    i64::from(game.ai_observation(rules, p).economy.income.science)
}

/// The nation with the most science a turn (ties: the lower id).
fn top_science(game: &GameState, rules: &CompiledRuleset) -> PlayerId {
    majors(game).into_iter().max_by_key(|p| (science_of(game, rules, *p), std::cmp::Reverse(p.0))).unwrap_or(PlayerId(0))
}

/// A victim with a capital and the rival nearest to it on its landmass (the pair nearest of all;
/// `flip` swaps which of two nations is the victim).
fn capital_and_taker(game: &GameState, rules: &CompiledRuleset, flip: bool) -> Result<(PlayerId, PlayerId, u64), String> {
    let (a, b, _) = nearest_pair(game).ok_or("no two nations share a landmass")?;
    let (victim, taker) = if flip { (b, a) } else { (a, b) };
    let auth = game.view().auth;
    let capital = classic_rules::combat::city::capital_of(auth, rules, victim).ok_or("the victim has no capital")?;
    let id = auth.cities().get(capital).ok_or("no capital city")?.id.0;
    Ok((victim, taker, id))
}

/// The nation with the most cities and the two nations nearest to it.
fn common_enemy(game: &GameState) -> Result<(PlayerId, PlayerId, PlayerId), String> {
    let all = majors(game);
    let enemy = all.iter().copied().max_by_key(|p| (game.view().auth.indexes().cities_of(*p).len(), std::cmp::Reverse(p.0))).ok_or("no nations")?;
    let mut near: Vec<(i32, PlayerId)> = all
        .iter()
        .copied()
        .filter(|p| *p != enemy)
        .filter_map(|p| city_tiles(game, enemy).into_iter().filter_map(|t| nearest_on_land(game, p, t)).min().map(|d| (d, p)))
        .collect();
    near.sort_by_key(|(d, p)| (*d, p.0));
    match near.as_slice() {
        [(_, a), (_, b), ..] => Ok((enemy, *a, *b)),
        _ => Err("fewer than two nations share a landmass with the common enemy".to_owned()),
    }
}

// ---------------------------------------------------------------------------
// The edits

/// `n` units of `enemy`'s own kinds inside `seat`'s land: melee, ranged, melee, mounted, ... in the
/// countryside (1 to 3 tiles from a city of the seat), the tiles drawn from `salt`.
pub(super) fn invaders(game: &GameState, rules: &CompiledRuleset, seat: PlayerId, enemy: PlayerId, n: usize, salt: u64) -> Result<Vec<Change>, String> {
    let kinds = edit::kinds_of(game, rules, enemy);
    let melee = *kinds.get(&Kind::Melee).ok_or("the enemy has no melee unit")?;
    let pick = |arm: Kind| kinds.get(&arm).copied().or_else(|| battle::fallback(arm).and_then(|f| kinds.get(&f).copied())).unwrap_or(melee);
    let order = [Kind::Melee, Kind::Ranged, Kind::Melee, Kind::Mounted, Kind::Melee, Kind::Ranged, Kind::Melee, Kind::Mounted, Kind::Melee, Kind::Ranged];
    let auth = game.view().auth;
    let map = auth.map();
    let cities = city_tiles(game, seat);
    let mut tiles: Vec<(u64, TileId)> = (0..map.tile_count() as u32)
        .map(TileId)
        .filter(|&t| {
            map.owner(t).flatten() == Some(seat)
                && auth.indexes().units_on(t).is_empty()
                && auth.indexes().city_on_tile(t).is_none()
                && battle::standable(rules, game, t)
                && cities.iter().map(|c| distance(game, *c, t)).min().is_some_and(|d| (1..=3).contains(&d))
        })
        .map(|t| (mix(u64::from(t.0) ^ salt), t))
        .collect();
    tiles.sort_unstable();
    if tiles.len() < n {
        return Err(format!("only {} free tiles in the seat's land for {n} units", tiles.len()));
    }
    Ok(tiles.iter().take(n).zip(order).map(|((_, tile), arm)| Change::Spawn { player: enemy.0, tile: tile.0, kind: pick(arm).0 }).collect())
}

/// The capital and, beyond one, the cities of the victim nearest to the taker, taken by the taker.
pub(super) fn lost_cities(game: &GameState, victim: PlayerId, taker: PlayerId, capital: u64, count: usize) -> Vec<Change> {
    let auth = game.view().auth;
    let mut others: Vec<(i32, u64)> = auth
        .indexes()
        .cities_of(victim)
        .iter()
        .filter_map(|h| auth.cities().get(*h))
        .filter(|c| c.id.0 != capital)
        .map(|c| (nearest_on_land(game, taker, c.tile).unwrap_or(i32::MAX), c.id.0))
        .collect();
    others.sort_unstable();
    std::iter::once(capital).chain(others.into_iter().map(|(_, id)| id)).take(count).map(|city| Change::CaptureCity { city, conqueror: taker.0 }).collect()
}

/// Apollo and the parts of the ship but `missing` of them (the last parts in the rules' order).
pub(super) fn spaceship(rules: &CompiledRuleset, leader: PlayerId, missing: u16) -> Vec<Change> {
    let mut changes: Vec<Change> = classic_rules::ai::victory::prerequisite_projects(rules).into_iter().map(|p| Change::Project { player: leader.0, project: p.0, times: 1 }).collect();
    let mut parts = classic_rules::world::victory::spaceship_parts(rules);
    parts.sort_by_key(|(p, _)| p.0);
    let mut left = missing;
    for (project, needed) in parts.iter().rev() {
        let skip = left.min(*needed);
        left -= skip;
        if needed - skip > 0 {
            changes.push(Change::Project { player: leader.0, project: project.0, times: needed - skip });
        }
    }
    changes
}

/// A nation taken out of the game: its cities to the nation nearest to them, its units gone.
pub(super) fn remove_nation(game: &GameState, gone: PlayerId) -> Result<Vec<Change>, String> {
    let auth = game.view().auth;
    let taker = majors(game)
        .into_iter()
        .filter(|p| *p != gone)
        .min_by_key(|p| (city_tiles(game, gone).into_iter().filter_map(|t| nearest_on_land(game, *p, t)).min().unwrap_or(i32::MAX), p.0))
        .ok_or("no other nation")?;
    let mut changes: Vec<Change> = auth.indexes().cities_of(gone).iter().filter_map(|h| auth.cities().get(*h)).map(|c| Change::CaptureCity { city: c.id.0, conqueror: taker.0 }).collect();
    let mut units: Vec<u64> = auth.indexes().units_of(gone).iter().filter_map(|h| auth.units().get(*h)).map(|u| u.core.id.0).collect();
    units.sort_unstable();
    // The captured cities' garrisons die with the capture; the rest of the units are removed here.
    changes.push(Change::RemoveUnits { ids: units });
    Ok(changes)
}
