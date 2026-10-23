//! The classic module's save sections (`SavedWorldV1::module`). The sections
//! and their byte order are the ones the save had before the platform
//! split: the byte codec writes the trade routes at the end of the economy
//! section and the other sections after the player kinds, so saves keep
//! their bytes (`aurelia-qa/tests/save_bytes.rs`).

use crate::classic::ClassicWorld;
use crate::ids::{CityHandle, EntityId, PlayerId, TileId};
use crate::persistence::culture::SavedCulture;
use crate::persistence::decode::DecodeError;
use crate::persistence::religion::SavedFaith;
use crate::persistence::schema::{
    SavedCityStates, SavedCongress, SavedEspionage, SavedRelations, SavedVictory,
};
use crate::persistence::tourism::SavedTourism;
use crate::persistence::trade::SavedTradeRoute;
use crate::rules::CompiledRuleset;
use crate::world::barbarians::{BarbarianMode, BarbarianState, Camp, ClearedCamp};
use crate::world::military::{MilitaryState, PlayerMilitary};
use crate::world::state::AuthWorld;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SavedClassic {
    /// Running trade routes (`persistence::trade`).
    pub trade_routes: Vec<SavedTradeRoute>,
    /// Great general and admiral points per player:
    /// (general points, admiral points, generals born, admirals born).
    pub military: Vec<(u32, u32, u16, u16)>,
    /// Diplomatic relations (`world::relations`).
    pub relations: SavedRelations,
    pub barbarians: SavedBarbarians,
    /// City-states (`persistence::city_states`).
    pub city_states: SavedCityStates,
    /// Victory options, projects, World Leader, outcome.
    pub victory: SavedVictory,
    /// The World Congress.
    pub congress: SavedCongress,
    /// Spies and steal rights.
    pub espionage: SavedEspionage,
    /// Culture pools and social policies.
    pub culture: SavedCulture,
    /// Faith, pantheons, religions, pressure, religious units.
    pub faith: SavedFaith,
    /// Great works, influence, public opinion, archaeology.
    pub tourism: SavedTourism,
}

/// Barbarians: mode (0 none, 1 normal, 2 raging), camps (tile, spawn
/// countdown), first wave released, cleared camps (tile, player, round),
/// the barbarian random stream.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SavedBarbarians {
    pub mode: u8,
    pub camps: Vec<(u32, u8)>,
    pub released: bool,
    pub cleared: Vec<(u32, u8, u32)>,
    pub rng: u64,
}

fn corrupt(msg: impl Into<String>) -> DecodeError {
    DecodeError::Corrupt(msg.into())
}

pub(crate) fn encode(world: &ClassicWorld, auth: &AuthWorld) -> SavedClassic {
    let city_ordinals = crate::persistence::encode::city_ordinals(auth);
    SavedClassic {
        trade_routes: crate::persistence::trade::encode_routes(auth),
        military: world
            .military
            .all()
            .iter()
            .map(|m| {
                (
                    m.general_points,
                    m.admiral_points,
                    m.generals_born,
                    m.admirals_born,
                )
            })
            .collect(),
        relations: crate::persistence::relations::encode_relations(
            &world.relations,
            &city_ordinals,
        ),
        barbarians: encode_barbarians(&world.barbarians),
        city_states: crate::persistence::city_states::encode_city_states(&world.city_states),
        victory: crate::persistence::victory::encode_victory(&world.victory),
        congress: crate::persistence::congress::encode_congress(&world.congress),
        espionage: crate::persistence::espionage::encode_espionage(
            &world.espionage,
            &city_ordinals,
        ),
        culture: crate::persistence::culture::encode_culture(auth),
        faith: crate::persistence::religion::encode_faith(auth),
        tourism: crate::persistence::tourism::encode_tourism(auth),
    }
}

fn encode_barbarians(b: &BarbarianState) -> SavedBarbarians {
    SavedBarbarians {
        mode: encode_barbarian_mode(b.mode),
        camps: b.camps().iter().map(|c| (c.tile.0, c.spawn_in)).collect(),
        released: b.released(),
        cleared: b
            .cleared()
            .iter()
            .map(|c| (c.tile.0, c.player.0, c.round))
            .collect(),
        rng: b.rng().state(),
    }
}

pub(crate) fn encode_barbarian_mode(mode: BarbarianMode) -> u8 {
    match mode {
        BarbarianMode::None => 0,
        BarbarianMode::Normal => 1,
        BarbarianMode::Raging => 2,
    }
}

/// The module state of a save; `auth` holds the loaded kernel world (cities
/// and units in save order). Sections decode in the order of the save.
pub(crate) fn decode(
    saved: &SavedClassic,
    auth: &AuthWorld,
    rules: &CompiledRuleset,
) -> Result<ClassicWorld, DecodeError> {
    let players = auth.players().count();
    let tiles = auth.map().tile_count();
    let cities: Vec<CityHandle> = auth.cities().iter().map(|(handle, _)| handle).collect();
    let units: Vec<EntityId> = auth.units().iter_cores().map(|u| u.id).collect();
    crate::systems::cargo::validate_cargo(auth).map_err(|e| DecodeError::Corrupt(format!("{e:?}")))?;
    Ok(ClassicWorld {
        relations: crate::persistence::relations::decode_relations(
            &saved.relations,
            players,
            rules,
            &cities,
        )?,
        military: decode_military(&saved.military, players)?,
        barbarians: decode_barbarians(&saved.barbarians, players, tiles)?,
        city_states: crate::persistence::city_states::decode_city_states(
            &saved.city_states,
            players,
            tiles,
            rules,
        )?,
        victory: crate::persistence::victory::decode_victory(
            &saved.victory,
            players,
            rules.module.projects.len(),
        )?,
        congress: crate::persistence::congress::decode_congress(
            &saved.congress,
            players,
            rules.module.projects.len(),
        )?,
        espionage: crate::persistence::espionage::decode_espionage(
            &saved.espionage,
            players,
            &cities,
        )?,
        culture: crate::persistence::culture::decode_culture(&saved.culture, rules, players)?,
        religion: crate::persistence::religion::decode_faith(
            &saved.faith,
            rules,
            players,
            &cities,
            &units,
        )?,
        tourism: crate::persistence::tourism::decode_tourism(
            &saved.tourism,
            &crate::persistence::tourism::TourismContext {
                rules,
                players,
                tiles,
                cities: &cities,
                units: &units,
            },
        )?,
        trade: crate::persistence::trade::decode_routes(auth, &saved.trade_routes, rules)
            .map_err(|why| corrupt(format!("trade {why}")))?,
    })
}

fn decode_military(
    saved: &[(u32, u32, u16, u16)],
    players: usize,
) -> Result<MilitaryState, DecodeError> {
    if saved.len() != players {
        return Err(corrupt("military size"));
    }
    let players = saved
        .iter()
        .map(|(g, a, gb, ab)| PlayerMilitary {
            general_points: *g,
            admiral_points: *a,
            generals_born: *gb,
            admirals_born: *ab,
        })
        .collect();
    Ok(MilitaryState::from_parts(players))
}

fn decode_barbarians(
    saved: &SavedBarbarians,
    players: usize,
    tiles: usize,
) -> Result<BarbarianState, DecodeError> {
    let mode = match saved.mode {
        0 => BarbarianMode::None,
        1 => BarbarianMode::Normal,
        2 => BarbarianMode::Raging,
        _ => return Err(corrupt("barbarian mode")),
    };
    let tile = |t: u32| -> Result<TileId, DecodeError> {
        if (t as usize) < tiles {
            Ok(TileId(t))
        } else {
            Err(corrupt("barbarian tile"))
        }
    };
    let mut camps = Vec::with_capacity(saved.camps.len());
    for (t, spawn_in) in &saved.camps {
        camps.push(Camp {
            tile: tile(*t)?,
            spawn_in: *spawn_in,
        });
    }
    let mut cleared = Vec::with_capacity(saved.cleared.len());
    for (t, p, round) in &saved.cleared {
        if usize::from(*p) >= players {
            return Err(corrupt("barbarian cleared player"));
        }
        cleared.push(ClearedCamp {
            tile: tile(*t)?,
            player: PlayerId(*p),
            round: *round,
        });
    }
    let rng = crate::rng::StreamRng::restore(saved.rng);
    Ok(BarbarianState::from_parts(
        mode,
        camps,
        saved.released,
        cleared,
        rng,
    ))
}
