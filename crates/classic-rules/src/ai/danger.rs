//! The danger of a position: what the player can see of the blows its own
//! military unit may take between the end of its turn and the start of the
//! next, as a fixed row of integers (`unit_features`) for a learned model,
//! and the tactical search's own estimate of it (`heuristic_exposure`), the
//! baseline such a model must beat.
//!
//! The source is the battlefield model of the tactical layer
//! (`ai::battlefield`), built once per player turn for the whole sight
//! (`Threats::build`): the enemy units and cities of the players at war with
//! us that may reach one of our military land units, their reach on the
//! known map, our bodies and zones of control. Nothing hidden is read: the
//! observation, the player's tile snapshots and the AI's memory of the
//! enemy cities it has seen. Integer and deterministic throughout.
//!
//! A unit decision asks the same row for a tile the unit has not reached
//! yet (`Threats::row` at a `Place`): the unit ends there, the other units
//! stand as they do, the enemy as the turn left it. Its threats are built
//! once per decision (`Threats::around`) from the decision's local
//! observation (fresh where it looks) and the whole sight kept at the start
//! of the turn (`Sight`, the brief's `danger`) beyond; the tiles the movers
//! may reach are modelled too. At the unit's own tile the row is the one
//! `unit_features` gives for the same sight.
//!
//! Every value of the row, in this order (`NAMES` holds the same names):
//!
//! | #  | name               | what                                                                                  |
//! |----|--------------------|---------------------------------------------------------------------------------------|
//! | 0  | `hp`               | the unit's health, 0..=100                                                            |
//! | 1  | `melee`            | its kind's melee (and defence) strength                                               |
//! | 2  | `ranged`           | its kind's ranged strength (0: none)                                                  |
//! | 3  | `range`            | its kind's range in tiles (0: melee)                                                  |
//! | 4  | `moves`            | its kind's movement points                                                            |
//! | 5  | `level`            | its level (1 for a new unit)                                                          |
//! | 6  | `promotions`       | promotions it has                                                                     |
//! | 7  | `fortified`        | 1: fortified                                                                          |
//! | 8  | `fortified_turns`  | turns it has been fortified (0: not)                                                  |
//! | 9  | `moved`            | 1: it spent moves this turn (it does not heal)                                        |
//! | 10 | `attacks_made`     | attacks it made this turn                                                             |
//! | 11 | `defence`          | defence of its tile in percent (`Battlefield::defence_percent`, fortification in)     |
//! | 12 | `embarked`         | 1: it stands on water                                                                 |
//! | 13 | `in_city`          | 1: it stands in an own city                                                           |
//! | 14 | `own_land`         | 1: the tile is ours                                                                   |
//! | 15 | `enemy_land`       | 1: the tile belongs to a player at war with us                                        |
//! | 16 | `adjacent_own`     | own military land units next to it                                                   |
//! | 17 | `own_within_3`     | own military land units within 3 tiles (itself not counted)                           |
//! | 18 | `city_distance`    | tiles to the nearest own city (`FAR` for none or farther)                             |
//! | 19 | `threats`          | enemy units and cities that can strike it next turn (`Battlefield::can_strike`)       |
//! | 20 | `threat_cities`    | of them, cities                                                                       |
//! | 21 | `threat_ranged`    | of them, ranged units                                                                 |
//! | 22 | `blow_sum`         | the sum of their expected blows on it (`Battlefield::blow_by`), hit points            |
//! | 23 | `blow_max`         | the largest of them                                                                   |
//! | 24 | `fog_blow`         | the expected blow of the fog around it (`Battlefield::fog_blow`)                      |
//! | 25 | `prior_blow`       | the fog's typical blow: the largest an enemy unit in the model deals it               |
//! | 26 | `unseen`           | land tiles next to it the player does not see now                                     |
//! | 27 | `foes_1`           | enemy military land units in sight next to it                                         |
//! | 28 | `foes_2`           | within 2 tiles                                                                        |
//! | 29 | `foes_3`           | within 3 tiles                                                                        |
//! | 30 | `foe_power_3`      | their strength × health / 100 within 3 tiles, summed                                  |
//! | 31 | `foe_ranged_3`     | ranged ones within 3 tiles                                                            |
//! | 32 | `foe_cities_2`     | enemy cities, seen now or remembered, within their strike range (2)                   |
//! | 33 | `foe_distance`     | tiles to the nearest enemy military land unit in sight (`FAR` for none or farther)    |
//! | 34 | `foe_city_distance`| tiles to the nearest enemy city, seen now or remembered (`FAR` for none or farther)   |
//! | 35 | `wars`             | players at war with us                                                                |
//! | 36 | `game_round`       | the round of the game                                                                 |
//!
//! "Enemy" is a player at war with us (barbarians included). The search's
//! exposure (`heuristic_exposure`) is `blow_sum + fog_blow`.
//!
//! Differences from the search, on purpose: the search models the 12
//! targets nearest to its theatre (`battlefield::MAX_TARGETS`), `Threats`
//! every target that may reach a unit (in models of at most 12 targets
//! each); the search takes the fog's typical blow from its first member and
//! a city's strength from a preview at the start of its turn (which needs
//! moves left), `Threats` the blow on each unit and the planning estimate
//! of the city (`operations::estimate_city`).

use std::sync::atomic::{AtomicU64, Ordering};

use crate::ai::battlefield::{Battlefield, KindStats, Posture, FOG_PRIOR_PERCENT, MAX_TARGETS};
use crate::ai::danger_net::{self, Danger};
use crate::ai::planner::{role_of, unit_power, AiRole, TurnBrief};
use crate::ai::state::SeenCity;
use crate::ai::tactical::THEATRE_RADIUS;
use crate::combat::city::CITY_ATTACK_RANGE;
use crate::ids::{PlayerId, TileId};
use crate::knowledge::observation::{AIObservation, LocalObservation, ObservedCity, ObservedUnit, OwnUnit, Relation};
use crate::knowledge::view::PlayerView;
use crate::rules::content::Domain;
use crate::rules::CompiledRuleset;
use crate::world::unit::{Stance, MOVE_DENOMINATOR};

/// Values in a row of `unit_features`.
pub const FEATURES: usize = 37;

/// The names of the values, in the row's order (the module's table).
pub const NAMES: [&str; FEATURES] = [
    "hp",
    "melee",
    "ranged",
    "range",
    "moves",
    "level",
    "promotions",
    "fortified",
    "fortified_turns",
    "moved",
    "attacks_made",
    "defence",
    "embarked",
    "in_city",
    "own_land",
    "enemy_land",
    "adjacent_own",
    "own_within_3",
    "city_distance",
    "threats",
    "threat_cities",
    "threat_ranged",
    "blow_sum",
    "blow_max",
    "fog_blow",
    "prior_blow",
    "unseen",
    "foes_1",
    "foes_2",
    "foes_3",
    "foe_power_3",
    "foe_ranged_3",
    "foe_cities_2",
    "foe_distance",
    "foe_city_distance",
    "wars",
    "game_round",
];

/// The column of `threats`: the net reads only rows where it is above 0.
pub const THREATS: usize = 19;

/// A distance this large or larger, or none, is written as this.
pub const FAR: i32 = 20;

/// Is the unit one the danger is read for: a military land unit (the
/// battlefield models land threats and land bodies only)?
pub fn is_subject(rules: &CompiledRuleset, unit: &OwnUnit) -> bool {
    role_of(rules, unit.kind) == AiRole::Military
}

/// An enemy unit the danger counts: a military land unit.
fn is_foe(rules: &CompiledRuleset, unit: &ObservedUnit) -> bool {
    let land = rules.units.get(unit.kind.0 as usize).is_some_and(|d| d.domain == Domain::Land);
    land && KindStats::of(rules, unit.kind).military
}

/// What the danger reads of the player's sight: our military land units
/// and cities, and the enemy's units and cities in sight or remembered.
/// Kept in the turn's brief (`TurnBrief::danger`) for the unit decisions,
/// which see only around the unit.
#[derive(Debug, Clone, Default)]
pub struct Sight {
    /// The player's military land units, by id.
    pub own: Vec<OwnUnit>,
    pub own_cities: Vec<TileId>,
    /// Enemy military land units in sight.
    pub foes: Vec<ObservedUnit>,
    /// Enemy cities in sight.
    pub cities: Vec<ObservedCity>,
    /// Enemy cities out of sight, from the AI's memory.
    pub remembered: Vec<SeenCity>,
}

impl Sight {
    /// The sight `obs`; `seen` is the AI's memory of the cities it has seen
    /// (`AiState::seen_cities`).
    pub fn take(obs: &AIObservation<'_>, rules: &CompiledRuleset, seen: &[SeenCity]) -> Self {
        let at_war = |owner| obs.relation(owner).is_some_and(|r| r.at_war);
        let own = obs.own_units.iter().filter(|u| is_subject(rules, u)).copied().collect();
        let own_cities = obs.own_cities.iter().map(|c| c.tile).collect();
        let foes = obs.visible_foreign_units.iter().filter(|u| at_war(u.owner) && is_foe(rules, u)).copied().collect();
        let cities: Vec<ObservedCity> = obs.visible_foreign_cities.iter().filter(|c| at_war(c.owner)).copied().collect();
        let remembered = seen.iter().filter(|c| at_war(c.owner) && !cities.iter().any(|v| v.tile == c.tile)).copied().collect();
        Self { own, own_cities, foes, cities, remembered }
    }

    /// The sight of a unit decision: the local observation where it looks
    /// (the turn's moves and kills in), this one beyond. Taken at the same
    /// moment as this one it is this one.
    pub fn around(&self, local: &LocalObservation<'_>, rules: &CompiledRuleset) -> Self {
        let dims = local.view.dimensions();
        let beyond = |tile: TileId| dims.distance(local.unit.tile, tile).is_none_or(|d| d > local.radius);
        let at_war = |owner| local.relation(owner).is_some_and(|r| r.at_war);
        let mut own: Vec<OwnUnit> = std::iter::once(&local.unit).chain(&local.own_units).filter(|u| is_subject(rules, u)).copied().collect();
        let fresh: Vec<_> = own.iter().map(|u| u.id).collect();
        own.extend(self.own.iter().filter(|u| beyond(u.tile) && !fresh.contains(&u.id)).copied());
        own.sort_by_key(|u| u.id);
        let mut own_cities: Vec<TileId> = self.own_cities.iter().copied().chain(local.own_cities.iter().map(|c| c.tile)).collect();
        own_cities.sort();
        own_cities.dedup();
        let mut foes: Vec<ObservedUnit> = local.foreign_units.iter().filter(|u| at_war(u.owner) && is_foe(rules, u)).copied().collect();
        foes.extend(self.foes.iter().filter(|u| beyond(u.tile) && at_war(u.owner)).copied());
        let mut cities: Vec<ObservedCity> = local.foreign_cities.iter().filter(|c| at_war(c.owner)).copied().collect();
        cities.extend(self.cities.iter().filter(|c| beyond(c.tile) && at_war(c.owner)).copied());
        let remembered = self.remembered.iter().filter(|c| at_war(c.owner) && !cities.iter().any(|v| v.tile == c.tile)).copied().collect();
        Self { own, own_cities, foes, cities, remembered }
    }
}

/// Where a unit ends its turn, for its row: the tile, our posture then (its
/// body on the tile, the other units where they stand or will), and what is
/// left of the enemy.
#[derive(Debug, Clone, Copy)]
pub struct Place<'p> {
    pub tile: TileId,
    pub posture: &'p Posture,
    /// The health of enemy units and cities the turn changed, by tile (0:
    /// killed or taken); the others as seen.
    pub hurt: &'p [(TileId, u16)],
}

impl Place<'_> {
    /// The health of the enemy on `tile` seen at `hp`; None when it is gone.
    fn foe_hp(&self, tile: TileId, hp: u16) -> Option<u16> {
        match self.hurt.iter().find(|(t, _)| *t == tile) {
            Some((_, 0)) => None,
            Some((_, left)) => Some(*left),
            None => Some(hp),
        }
    }

    /// Our other bodies: the posture's without the unit's own.
    fn others(&self) -> Vec<TileId> {
        let mut others = self.posture.bodies.clone();
        if let Some(at) = others.iter().position(|t| *t == self.tile) {
            others.remove(at);
        }
        others
    }
}

/// The unit as it ends its turn on `tile`: a unit that moves there spent
/// moves and lost its fortification.
pub fn moved(unit: &OwnUnit, tile: TileId) -> OwnUnit {
    if tile == unit.tile {
        return *unit;
    }
    OwnUnit { tile, fortified: false, stance: Stance::Ready, moves_left: unit.moves_left.min(unit.moves_max.saturating_sub(1)), ..*unit }
}

/// The net's expected loss of a unit worth `worth` points: the damage and
/// the chance of its death times its worth, in hit points.
pub fn expected_loss(danger: Danger, worth: i32) -> i32 {
    i32::from(danger.damage) + i32::from(danger.death_permille) * worth / 1000
}

/// The threats on the player's military land units at one moment, built
/// once for the whole sight and read for every unit.
pub struct Threats<'v, 'a> {
    /// The tiles next to every own military land unit, no targets: the
    /// ground of the units the enemy cannot reach (and the tiles the movers
    /// may reach).
    pub near: Battlefield<'v, 'a>,
    /// The models of the enemy's reach: tiles within `THEATRE_RADIUS` of
    /// the engaged units, the movers and the targets, at most `MAX_TARGETS`
    /// targets each (empty: no enemy can reach a unit).
    pub fields: Vec<Battlefield<'v, 'a>>,
    /// Our bodies (the military land units) and zones of control (they and
    /// our cities).
    pub posture: Posture,
    /// The player's military land units, by id.
    pub own: Vec<OwnUnit>,
    pub own_cities: Vec<TileId>,
    /// Enemy military land units in sight.
    pub foes: Vec<ObservedUnit>,
    /// Enemy cities, in sight or remembered.
    pub foe_cities: Vec<TileId>,
    /// The players at war with us.
    pub enemies: Vec<PlayerId>,
    pub wars: i32,
    pub round: u32,
    /// The modelled targets: their tiles and how far they strike.
    targets: Vec<(TileId, i32)>,
}

/// The farthest a target strikes from where it stands: its range and the
/// tiles it walks (the search's own cut, `plan_search`), a city its range.
fn strike_bound(stats: &KindStats, city: bool) -> i32 {
    if city {
        return CITY_ATTACK_RANGE;
    }
    stats.range.max(1) + stats.movement.div_ceil(u32::from(MOVE_DENOMINATOR)) as i32 + 1
}

/// What one model gets: enemy units (with the garrisons of its cities),
/// cities in sight and remembered ones; at most `MAX_TARGETS` targets.
#[derive(Default)]
struct Chunk {
    foes: Vec<ObservedUnit>,
    cities: Vec<ObservedCity>,
    remembered: Vec<SeenCity>,
    targets: usize,
}

/// The chunk the next target goes to: the last one while it has room.
fn room(chunks: &mut Vec<Chunk>) -> usize {
    if chunks.last().is_none_or(|c| c.targets >= MAX_TARGETS) {
        chunks.push(Chunk::default());
    }
    chunks.len() - 1
}

/// What a place sees of the model: the targets that reach one of our
/// bodies there (alive) and the centres the model would have for them.
struct Scene {
    reaching: Vec<TileId>,
    centres: Vec<TileId>,
}

impl Scene {
    fn covers(&self, dims: crate::math::MapDimensions, tile: TileId) -> bool {
        self.centres.iter().any(|c| dims.distance(*c, tile).is_some_and(|d| d <= THEATRE_RADIUS))
    }
}

impl<'v, 'a> Threats<'v, 'a> {
    /// The threats of the sight `obs`: `seen` is the AI's memory of the
    /// cities it has seen (`AiState::seen_cities`), `brief` the turn's brief
    /// (the cities' estimates read the player's technologies).
    pub fn build(obs: &'v AIObservation<'a>, rules: &'a CompiledRuleset, brief: &TurnBrief, seen: &[SeenCity]) -> Self {
        Self::build_with(obs, rules, brief, seen, &[])
    }

    /// `build` with `movers`, the units that may still move and the tiles
    /// they may walk this turn (as `around`).
    pub fn build_with(obs: &'v AIObservation<'a>, rules: &'a CompiledRuleset, brief: &TurnBrief, seen: &[SeenCity], movers: &[(TileId, i32)]) -> Self {
        Self::of(&obs.view, rules, brief, &obs.relations, obs.round, Sight::take(obs, rules, seen), movers)
    }

    /// The threats a unit decision sees: its local observation and the
    /// turn's `sight` beyond (`Sight::around`). `movers` are the units that
    /// may still move, with the tiles they may walk this turn: the targets
    /// that may reach them where they go and the ground there are modelled
    /// too.
    pub fn around(local: &'v LocalObservation<'a>, rules: &'a CompiledRuleset, brief: &TurnBrief, sight: &Sight, movers: &[(TileId, i32)]) -> Self {
        Self::of(&local.view, rules, brief, &local.relations, local.round, sight.around(local, rules), movers)
    }

    #[allow(clippy::too_many_arguments)]
    fn of(view: &'v PlayerView<'a>, rules: &'a CompiledRuleset, brief: &TurnBrief, relations: &[Relation], round: u32, sight: Sight, movers: &[(TileId, i32)]) -> Self {
        let dims = view.dimensions();
        let Sight { own, own_cities, foes, cities, remembered } = sight;
        let enemies: Vec<PlayerId> = relations.iter().filter(|r| r.at_war).map(|r| r.other).collect();
        let mut foe_cities: Vec<TileId> = cities.iter().map(|c| c.tile).chain(remembered.iter().map(|c| c.tile)).collect();
        foe_cities.sort();
        foe_cities.dedup();

        let distance = |a: TileId, b: TileId| dims.distance(a, b).unwrap_or(i32::MAX);
        let reaches = |tile: TileId, bound: i32| {
            own.iter().any(|u| distance(u.tile, tile) <= bound) || movers.iter().any(|(t, steps)| distance(*t, tile) <= bound.saturating_add(*steps))
        };
        let city_bound = strike_bound(&KindStats { melee: 0, ranged: 0, range: 0, movement: 0, military: true }, true);
        // The targets that may strike one of our units, in chunks of at most MAX_TARGETS.
        let mut chunks: Vec<Chunk> = Vec::new();
        let mut targets: Vec<(TileId, i32)> = Vec::new();
        for city in cities.iter().filter(|c| reaches(c.tile, city_bound)) {
            let at = room(&mut chunks);
            let chunk = &mut chunks[at];
            chunk.cities.push(*city);
            chunk.foes.extend(foes.iter().filter(|u| u.tile == city.tile));
            chunk.targets += 1;
            targets.push((city.tile, city_bound));
        }
        for city in remembered.iter().filter(|c| reaches(c.tile, city_bound)) {
            let at = room(&mut chunks);
            chunks[at].remembered.push(*city);
            chunks[at].targets += 1;
            targets.push((city.tile, city_bound));
        }
        for foe in foes.iter().filter(|u| !cities.iter().any(|c| c.tile == u.tile)) {
            let bound = strike_bound(&KindStats::of(rules, foe.kind), false);
            if !reaches(foe.tile, bound) {
                continue;
            }
            let at = room(&mut chunks);
            chunks[at].foes.push(*foe);
            chunks[at].targets += 1;
            targets.push((foe.tile, bound));
        }

        let own_tiles: Vec<TileId> = own.iter().map(|u| u.tile).collect();
        let mut ground = own_tiles.clone();
        for (tile, steps) in movers {
            ground.extend(crate::math::hex_disc(dims, *tile, (*steps).max(0)));
        }
        ground.sort();
        ground.dedup();
        let near = Battlefield::build(view, rules, brief, round, &[], &[], &[], &own_cities, &ground, 1, None);
        // The engaged units: those a target may reach; the models cover them, the movers and the targets.
        let mut centres: Vec<TileId> = own.iter().filter(|u| targets.iter().any(|(t, b)| distance(u.tile, *t) <= *b)).map(|u| u.tile).collect();
        centres.extend(targets.iter().map(|(t, _)| *t));
        if !targets.is_empty() {
            centres.extend(movers.iter().map(|(t, _)| *t));
        }
        centres.sort();
        centres.dedup();
        let fields = chunks
            .iter()
            .map(|c| Battlefield::build(view, rules, brief, round, &c.foes, &c.cities, &c.remembered, &own_cities, &centres, THEATRE_RADIUS, None))
            .collect();
        let mut sources = own_tiles.clone();
        sources.extend(own_cities.iter());
        let posture = Posture { bodies: own_tiles, sources };
        let wars = enemies.len() as i32;
        Self { near, fields, posture, own, own_cities, foes, foe_cities, enemies, wars, round, targets }
    }

    fn distance(&self, a: TileId, b: TileId) -> i32 {
        self.near.distance(a, b)
    }

    /// The unit where it stands, with the posture of the sight.
    pub fn place_of(&self, unit: &OwnUnit) -> Place<'_> {
        Place { tile: unit.tile, posture: &self.posture, hurt: &[] }
    }

    /// Our posture with `unit` on `tile`, the other units where they stand.
    pub fn posture_with(&self, unit: &OwnUnit, tile: TileId) -> Posture {
        let mut bodies = self.posture.bodies.clone();
        match bodies.iter().position(|t| *t == unit.tile) {
            Some(at) => bodies[at] = tile,
            None => bodies.push(tile),
        }
        let mut sources = bodies.clone();
        sources.extend(self.own_cities.iter());
        Posture { bodies, sources }
    }

    /// The targets that reach one of our bodies of the place, and the
    /// centres the model of the sight would have with that posture: the
    /// engaged bodies and those targets.
    fn scene(&self, place: &Place<'_>) -> Scene {
        let bodies = &place.posture.bodies;
        let reaching: Vec<(TileId, i32)> = self
            .targets
            .iter()
            .filter(|(t, b)| place.foe_hp(*t, 1).is_some() && bodies.iter().any(|x| self.distance(*x, *t) <= *b))
            .copied()
            .collect();
        let mut centres: Vec<TileId> = bodies.iter().filter(|x| reaching.iter().any(|(t, b)| self.distance(**x, *t) <= *b)).copied().collect();
        centres.extend(reaching.iter().map(|(t, _)| *t));
        Scene { reaching: reaching.into_iter().map(|(t, _)| t).collect(), centres }
    }

    /// The model that holds the place with the enemy's reach (None: no
    /// enemy model covers it).
    fn field_at(&self, place: &Place<'_>, scene: &Scene) -> Option<&Battlefield<'v, 'a>> {
        self.fields.first().filter(|f| f.tile(place.tile).is_some() && scene.covers(f.dims, place.tile))
    }

    /// The fog's typical blow on the unit at the place: the largest an
    /// enemy unit of the models that reaches us deals it.
    fn prior_blow(&self, stats: &KindStats, hp: u16, place: &Place<'_>, scene: &Scene) -> u16 {
        if self.field_at(place, scene).is_none() {
            return 0;
        }
        self.fields
            .iter()
            .flat_map(|f| f.targets.iter().map(move |t| (f, t)))
            .filter(|(_, t)| t.stats.military && !t.is_city() && scene.reaching.contains(&t.tile))
            .filter_map(|(f, t)| {
                let foe = crate::ai::battlefield::Target { hp: place.foe_hp(t.tile, t.hp)?, ..*t };
                Some(f.blow_by(&foe, stats, hp, place.tile, false))
            })
            .max()
            .unwrap_or(0)
    }

    /// Land tiles next to the place the player does not see now.
    fn unseen(&self, place: &Place<'_>, scene: &Scene) -> i32 {
        let (field, theatre) = match self.field_at(place, scene) {
            Some(field) => (field, true),
            None => (&self.near, false),
        };
        field
            .dims
            .neighbors(place.tile)
            .into_iter()
            .filter(|n| !theatre || scene.covers(field.dims, *n))
            .filter(|n| field.tile(*n).is_some_and(|i| !i.visible && !i.water))
            .count() as i32
    }

    /// The blows of every target that can strike the unit at the place next
    /// turn: (target is a city, target is ranged, expected blow). The
    /// search's reply step (`plan_search`).
    fn blows_at(&self, stats: &KindStats, unit: &OwnUnit, place: &Place<'_>, scene: &Scene) -> Vec<(bool, bool, u16)> {
        let mut out = Vec::new();
        if self.field_at(place, scene).is_none() {
            return out;
        }
        for field in &self.fields {
            for (f, t) in field.targets.iter().enumerate() {
                if !t.threatens() {
                    continue;
                }
                let Some(hp) = place.foe_hp(t.tile, t.hp) else { continue };
                let close = field.distance(t.tile, place.tile) <= t.stats.range.max(1) + t.stats.movement.div_ceil(u32::from(MOVE_DENOMINATOR)) as i32 + 1;
                if !close || !field.can_strike(f, place.tile, place.posture) {
                    continue;
                }
                let foe = crate::ai::battlefield::Target { hp, ..*t };
                let blow = field.blow_by(&foe, stats, unit.hp.max(1), place.tile, unit.fortified);
                out.push((t.is_city(), !t.is_city() && t.stats.is_ranged(), blow));
            }
        }
        out
    }

    /// The fog's typical blow on the unit where it stands.
    pub fn prior_blow_of(&self, rules: &CompiledRuleset, unit: &OwnUnit) -> u16 {
        let place = self.place_of(unit);
        self.prior_blow(&KindStats::of(rules, unit.kind), unit.hp, &place, &self.scene(&place))
    }

    /// The expected blow of the fog on the unit where it stands
    /// (`Battlefield::fog_blow` with the unit's own typical blow).
    pub fn fog_blow(&self, rules: &CompiledRuleset, unit: &OwnUnit) -> u16 {
        let place = self.place_of(unit);
        let scene = self.scene(&place);
        let prior = self.prior_blow(&KindStats::of(rules, unit.kind), unit.hp, &place, &scene);
        (i64::from(prior) * i64::from(self.unseen(&place, &scene)) * FOG_PRIOR_PERCENT / 100) as u16
    }

    /// The blows of every target that can strike the unit where it stands
    /// next turn, with its posture: (target is a city, target is ranged,
    /// expected blow). The search's reply step (`plan_search`), for a unit
    /// that stays.
    pub fn blows(&self, rules: &CompiledRuleset, unit: &OwnUnit) -> Vec<(bool, bool, u16)> {
        let place = self.place_of(unit);
        self.blows_at(&KindStats::of(rules, unit.kind), unit, &place, &self.scene(&place))
    }

    /// The row of `unit` ending its turn at `place` (the module's table);
    /// `unit` as it will be then (`moved`).
    pub fn row(&self, rules: &CompiledRuleset, unit: &OwnUnit, place: &Place<'_>) -> [i32; FEATURES] {
        let tile = place.tile;
        let distance = |a: TileId, b: TileId| self.distance(a, b);
        let stats = KindStats::of(rules, unit.kind);
        let scene = self.scene(place);
        let field = self.field_at(place, &scene).unwrap_or(&self.near);
        let info = field.tile(tile).copied();
        let owner = info.and_then(|i| i.owner);
        let enemy_land = owner.is_some_and(|o| self.enemies.contains(&o));
        let others = place.others();
        let city_distance = self.own_cities.iter().map(|c| distance(*c, tile)).min().unwrap_or(FAR).min(FAR);

        let blows = self.blows_at(&stats, unit, place, &scene);
        let blow_sum: i32 = blows.iter().map(|(_, _, b)| i32::from(*b)).sum();
        let blow_max = blows.iter().map(|(_, _, b)| i32::from(*b)).max().unwrap_or(0);
        let prior = self.prior_blow(&stats, unit.hp, place, &scene);
        let unseen = self.unseen(place, &scene);
        let fog_blow = (i64::from(prior) * i64::from(unseen) * FOG_PRIOR_PERCENT / 100) as u16;
        let foes: Vec<(ObservedUnit, u16)> = self.foes.iter().filter_map(|f| place.foe_hp(f.tile, f.hp).map(|hp| (*f, hp))).collect();
        let within = |r: i32| foes.iter().filter(move |(f, _)| distance(f.tile, tile) <= r);
        let ranged = |f: &&(ObservedUnit, u16)| KindStats::of(rules, f.0.kind).is_ranged();
        let foe_cities: Vec<TileId> = self.foe_cities.iter().copied().filter(|c| place.foe_hp(*c, 1).is_some()).collect();

        let b = |v: bool| i32::from(v);
        [
            i32::from(unit.hp),
            (stats.melee / 100) as i32,
            (stats.ranged / 100) as i32,
            stats.range,
            (stats.movement / u32::from(MOVE_DENOMINATOR)) as i32,
            i32::from(unit.level),
            unit.promotions.iter().count() as i32,
            b(unit.fortified),
            i32::from(unit.stance.fortify_turns()),
            b(unit.moves_left < unit.moves_max),
            i32::from(unit.attacks_made),
            field.defence_percent(tile, self.near.player, unit.fortified) as i32,
            b(info.is_some_and(|i| i.water)),
            b(info.is_some_and(|i| i.own_city)),
            b(owner == Some(self.near.player)),
            b(enemy_land),
            others.iter().filter(|o| self.near.dims.adjacent(**o, tile)).count() as i32,
            others.iter().filter(|o| distance(**o, tile) <= 3).count() as i32,
            city_distance,
            blows.len() as i32,
            blows.iter().filter(|(city, _, _)| *city).count() as i32,
            blows.iter().filter(|(_, ranged, _)| *ranged).count() as i32,
            blow_sum,
            blow_max,
            i32::from(fog_blow),
            i32::from(prior),
            unseen,
            within(1).count() as i32,
            within(2).count() as i32,
            within(3).count() as i32,
            within(3).map(|(f, hp)| unit_power(rules, f.kind, *hp) as i32).sum(),
            within(3).filter(ranged).count() as i32,
            foe_cities.iter().filter(|c| distance(**c, tile) <= CITY_ATTACK_RANGE).count() as i32,
            foes.iter().map(|(f, _)| distance(f.tile, tile)).min().unwrap_or(FAR).min(FAR),
            foe_cities.iter().map(|c| distance(*c, tile)).min().unwrap_or(FAR).min(FAR),
            self.wars,
            self.round as i32,
        ]
    }

    /// The net's answer for `unit` ending its turn at `place` (`unit` as it
    /// will be then); None where no threat reaches it: the net was trained
    /// only on rows with one.
    pub fn net_danger(&self, rules: &CompiledRuleset, unit: &OwnUnit, place: &Place<'_>) -> Option<Danger> {
        let row = self.row(rules, unit, place);
        (row[THREATS] > 0).then(|| danger_net::predict(&row))
    }

    /// The search's estimate of the danger of `unit` ending its turn at
    /// `place` (`unit` as it will be then), capped at its health: the blows of
    /// every target that can strike it there and the fog's
    /// (`heuristic_exposure` at any place), at most its health; it dies when
    /// they reach its health (the search's lethal rule). A unit in an own
    /// city takes no blow: the rules strike the city, whose garrison only
    /// adds strength (`combat::exchange`).
    pub fn search_danger(&self, rules: &CompiledRuleset, unit: &OwnUnit, place: &Place<'_>) -> Danger {
        if self.own_cities.contains(&place.tile) {
            return Danger { damage: 0, death_permille: 0 };
        }
        let stats = KindStats::of(rules, unit.kind);
        let scene = self.scene(place);
        let blows: i32 = self.blows_at(&stats, unit, place, &scene).iter().map(|(_, _, b)| i32::from(*b)).sum();
        let exposure = blows + i32::from(self.fog_at(&stats, unit, place, &scene));
        let hp = i32::from(unit.hp);
        Danger { damage: exposure.min(hp).max(0) as u16, death_permille: if exposure >= hp { 1000 } else { 0 } }
    }

    /// The danger of `unit` ending its turn at `place` by `source`: the
    /// search's estimate, or the net's answer where a threat reaches the unit
    /// and the fog's blow elsewhere (as the search's reply with the net,
    /// `plan_search::net_reply`).
    pub fn danger_by(&self, source: Source, rules: &CompiledRuleset, unit: &OwnUnit, place: &Place<'_>) -> Danger {
        match source {
            Source::Search => self.search_danger(rules, unit, place),
            Source::Net => self.net_danger(rules, unit, place).unwrap_or_else(|| {
                let stats = KindStats::of(rules, unit.kind);
                let fog = self.fog_at(&stats, unit, place, &self.scene(place));
                Danger { damage: fog.min(unit.hp), death_permille: 0 }
            }),
        }
    }

    /// The fog's expected blow on the unit at the place.
    fn fog_at(&self, stats: &KindStats, unit: &OwnUnit, place: &Place<'_>, scene: &Scene) -> u16 {
        let prior = self.prior_blow(stats, unit.hp, place, scene);
        (i64::from(prior) * i64::from(self.unseen(place, scene)) * FOG_PRIOR_PERCENT / 100) as u16
    }
}

/// Where the danger of a unit at a place is read from (`Threats::danger_by`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Source {
    /// The tactical search's estimate, capped (`Threats::search_danger`).
    #[default]
    Search,
    /// The danger net (`Threats::net_danger`).
    Net,
}

/// The unit as it ends its turn on `tile` with `hp`, after a strike there
/// (`struck`: one more attack, moves spent, out of its fortification) or not
/// (`moved`).
pub fn after(unit: &OwnUnit, tile: TileId, hp: u16, struck: bool) -> OwnUnit {
    let mut end = moved(unit, tile);
    end.hp = hp.max(1);
    if struck {
        end.attacks_made = end.attacks_made.saturating_add(1);
        end.moves_left = end.moves_left.min(end.moves_max.saturating_sub(1));
        end.fortified = false;
        end.stance = Stance::Ready;
    }
    end
}

/// Diagnostics of the danger net's use in decisions in this process: the
/// threats built (one per decision that ranks tiles) and the rows read with
/// the net's answer, their count and wall time in nanoseconds. Never read
/// by a decision.
static NET_BUILDS: AtomicU64 = AtomicU64::new(0);
static NET_BUILD_NANOS: AtomicU64 = AtomicU64::new(0);
static NET_ROWS: AtomicU64 = AtomicU64::new(0);
static NET_ROW_NANOS: AtomicU64 = AtomicU64::new(0);

/// A decision built its threats in `nanos`.
pub fn note_build(nanos: u64) {
    NET_BUILDS.fetch_add(1, Ordering::Relaxed);
    NET_BUILD_NANOS.fetch_add(nanos, Ordering::Relaxed);
}

/// A decision read a row and the net's answer in `nanos`.
pub fn note_row(nanos: u64) {
    NET_ROWS.fetch_add(1, Ordering::Relaxed);
    NET_ROW_NANOS.fetch_add(nanos, Ordering::Relaxed);
}

/// (threats built, their ns, rows read, their ns) since the process began.
pub fn net_stats() -> (u64, u64, u64, u64) {
    let load = |a: &AtomicU64| a.load(Ordering::Relaxed);
    (load(&NET_BUILDS), load(&NET_BUILD_NANOS), load(&NET_ROWS), load(&NET_ROW_NANOS))
}

/// The tactical search's estimate of the damage the unit takes where it
/// stands: the blows of every target that can strike it plus the fog's
/// (`plan_search`'s exposure of a member). The baseline of a learned model.
pub fn heuristic_exposure(threats: &Threats<'_, '_>, rules: &CompiledRuleset, unit: &OwnUnit) -> i32 {
    let blows: i32 = threats.blows(rules, unit).iter().map(|(_, _, b)| i32::from(*b)).sum();
    blows + i32::from(threats.fog_blow(rules, unit))
}

/// The row of the unit where it stands (the module's table), from the
/// threats of its player's sight.
pub fn unit_features(threats: &Threats<'_, '_>, rules: &CompiledRuleset, unit: &OwnUnit) -> [i32; FEATURES] {
    threats.row(rules, unit, &threats.place_of(unit))
}
