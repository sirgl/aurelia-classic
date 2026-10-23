//! Fuzz of the naval mechanics on the classic ruleset (§31 platform/project.md): random
//! archipelagos with land units that embark (Optics, Astronomy, Polynesia,
//! Denmark), ships, a carrier with fighters, cities that strike; random
//! moves over land and sea, melee, ranged and city attacks, city strikes,
//! pillage, rebasing, war and turns. After every accepted command the world
//! validates (indexes, cargo, derived state against its rebuild: coverage,
//! unit sight, occupancy); a refused command leaves the hash untouched;
//! every 40 commands a save and load keeps the hash.

use classic_content::core::core_rules;
use classic_rules::ids::{CityHandle, PlayerId, TechId, TerrainId, TileId, UnitHandle, UnitKindId};
use classic_rules::persistence::{decode::decode, encode::encode};
use classic_rules::replay::authoritative_hash;
use classic_rules::world::state::GameConfig;
use classic_rules::ClassicGame;
use classic_rules::{compile_rules, Command, CompiledRuleset, GameState};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0.max(1);
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

const W: u32 = 18;
const H: u32 = 14;

const LAND_UNITS: [&str; 5] = ["core:unit/warrior", "core:unit/archer", "core:unit/settler", "core:unit/scout", "core:unit/catapult"];
const SHIPS: [&str; 5] = ["core:unit/trireme", "core:unit/frigate", "core:unit/caravel", "core:unit/submarine", "core:unit/destroyer"];
const TECHS: [&str; 3] = ["core:tech/optics", "core:tech/astronomy", "core:tech/steam-power"];
const CIVS: [&str; 3] = ["core:nation/polynesia", "core:nation/denmark", "core:nation/england"];

fn kind(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

/// Islands: noise over a grid, a third to a half water.
fn archipelago(rng: &mut Rng, rules: &CompiledRuleset) -> GameState {
    let mut game = GameState::new(
        rules,
        GameConfig {
            players: 3,
            width: W,
            height: H,
            rng_seed: rng.next(),
        },
    );
    let water = 35 + rng.below(20);
    let terrain: Vec<(TileId, TerrainId)> = (0..W * H)
        .map(|t| {
            let code = if rng.below(100) < water { 4 } else { [0, 1, 1, 2][rng.below(4) as usize] };
            (TileId(t), TerrainId(code))
        })
        .collect();
    game.debug_set_terrain_batch(rules, &terrain);
    let is_water = |t: u32| terrain[t as usize].1 == TerrainId(4);
    for p in 0..3u8 {
        let player = PlayerId(p);
        if rng.below(3) == 0 {
            let civ = CIVS[rng.below(CIVS.len() as u64) as usize];
            let index = rules.civs.iter().position(|c| c.key == civ).unwrap() as u8;
            game.debug_set_player_civ(player, index);
        }
        for tech in TECHS {
            if rng.below(2) == 0 {
                let id = TechId(rules.technologies.iter().position(|t| t.key == tech).unwrap() as u16);
                game.debug_learn(rules, player, id);
            }
        }
        place_units(&mut game, rules, rng, player, &is_water);
    }
    game.debug_meet_all();
    for (a, b) in [(0, 1), (1, 2), (0, 2)] {
        if rng.below(3) != 0 {
            game.debug_force_war(rules, PlayerId(a), PlayerId(b));
        }
    }
    game
}

fn place_units(game: &mut GameState, rules: &CompiledRuleset, rng: &mut Rng, player: PlayerId, is_water: &dyn Fn(u32) -> bool) {
    let mut city_placed = false;
    for _ in 0..12 {
        let tile = rng.below(u64::from(W * H)) as u32;
        let occupied = !game.view().auth.indexes().units_on(TileId(tile)).is_empty()
            || game.view().auth.indexes().city_on_tile(TileId(tile)).is_some();
        if occupied {
            continue;
        }
        if is_water(tile) {
            let ship = SHIPS[rng.below(SHIPS.len() as u64) as usize];
            game.debug_spawn_unit(rules, player, TileId(tile), kind(rules, ship));
            continue;
        }
        let free_site = classic_rules::world::territory::city_site_check(game.view().auth, player, TileId(tile)).is_ok();
        if !city_placed && free_site {
            game.debug_found_city(rules, player, TileId(tile));
            city_placed = true;
            continue;
        }
        let land = LAND_UNITS[rng.below(LAND_UNITS.len() as u64) as usize];
        game.debug_spawn_unit(rules, player, TileId(tile), kind(rules, land));
    }
}

fn units_of(game: &GameState, player: PlayerId) -> Vec<UnitHandle> {
    game.view().auth.indexes().units_of(player).to_vec()
}

fn all_units(game: &GameState) -> Vec<UnitHandle> {
    (0..3).flat_map(|p| units_of(game, PlayerId(p))).collect()
}

fn pick<T: Copy>(rng: &mut Rng, items: &[T], fallback: T) -> T {
    if items.is_empty() {
        return fallback;
    }
    items[rng.below(items.len() as u64) as usize]
}

/// Foreign units within `radius` of `at` (the targets an attack or a city
/// strike can reach).
fn foreign_near(game: &GameState, owner: PlayerId, at: TileId, radius: i32) -> Vec<(UnitHandle, TileId)> {
    let auth = game.view().auth;
    let dims = auth.map().dimensions;
    classic_rules::math::tiles_in_hex_radius(&dims, at, radius)
        .into_iter()
        .flat_map(|t| auth.indexes().units_on(t).to_vec().into_iter().map(move |u| (u, t)))
        .filter(|(u, _)| auth.units().get(*u).is_some_and(|v| v.core.owner != owner))
        .collect()
}

fn random_command(rng: &mut Rng, game: &GameState) -> Command {
    let auth = game.view().auth;
    let active = auth.active_player();
    let mine = units_of(game, active);
    let none = UnitHandle { index: 9999, generation: 0 };
    let unit = pick(rng, &mine, none);
    let anyone = pick(rng, &all_units(game), none);
    let tile = TileId(rng.below(u64::from(W * H)) as u32);
    let at = auth.units().get(unit).map_or(tile, |u| u.core.tile);
    let near = pick(rng, &classic_rules::math::tiles_in_hex_radius(&auth.map().dimensions, at, 3), tile);
    let foes = foreign_near(game, active, at, 2);
    let (foe, foe_tile) = pick(rng, &foes, (anyone, near));
    let cities: Vec<CityHandle> = auth.cities().iter().map(|(h, _)| h).collect();
    let city = pick(rng, &cities, CityHandle { index: 9999, generation: 0 });
    let city_target = auth.cities().get(city).map_or(near, |c| {
        let foes = foreign_near(game, c.owner, c.tile, 2);
        pick(rng, &foes, (anyone, near)).1
    });
    match rng.below(100) {
        0..=24 => Command::MoveUnit { unit, destination: near },
        25..=32 => Command::MarchTo { unit, destination: tile },
        33..=47 => Command::AttackUnit { attacker: unit, defender: foe },
        48..=57 => Command::RangedAttack { unit, target: foe_tile },
        58..=61 => Command::AttackCity { attacker: unit, city },
        62..=69 => Command::CityStrike { city, target: city_target },
        70..=72 => Command::Pillage { unit },
        73..=75 => Command::FoundCity { settler: unit },
        76..=78 => Command::RebaseToCarrier { unit, carrier: anyone },
        79..=80 => {
            let enemy = PlayerId(((active.0 as u64 + 1 + rng.below(2)) % 3) as u8);
            Command::DeclareWar { player: active, enemy }
        }
        _ => Command::EndTurn { player: active },
    }
}

/// A carrier with two fighters for the first player (rebasing, cargo
/// moving with its carrier, sinking with it).
fn add_carrier(game: &mut GameState, rules: &CompiledRuleset) {
    let auth = game.view().auth;
    let water = (0..W * H).map(TileId).find(|t| auth.map().is_water(*t) && auth.indexes().units_on(*t).is_empty());
    let Some(water) = water else {
        return;
    };
    let carrier = game.debug_spawn_unit(rules, PlayerId(0), water, kind(rules, "core:unit/carrier"));
    let base = game.view().auth.indexes().cities_of(PlayerId(0)).first().copied();
    let Some(base) = base else {
        return;
    };
    let base_tile = game.view().auth.cities().get(base).unwrap().tile;
    for _ in 0..2 {
        let fighter = game.debug_spawn_unit(rules, PlayerId(0), base_tile, kind(rules, "core:unit/fighter"));
        let _ = game.execute(rules, Command::RebaseToCarrier { unit: fighter, carrier });
    }
}

#[test]
fn naval_fuzz_validates_every_commit_and_round_trips_saves() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut rng = Rng(0x0A7A_1F00_5EA5);
    let mut accepted = 0;
    for world in 0..24 {
        let mut game = archipelago(&mut rng, &rules);
        add_carrier(&mut game, &rules);
        game.validate(&rules).unwrap_or_else(|e| panic!("world {world}: {e:?}"));
        for step in 0..160 {
            let command = random_command(&mut rng, &game);
            let before = authoritative_hash(&game);
            match game.execute(&rules, command.clone()) {
                Ok(_) => {
                    accepted += 1;
                    game.validate(&rules)
                        .unwrap_or_else(|e| panic!("world {world} step {step} {command:?}: {e:?}"));
                }
                Err(_) => assert_eq!(before, authoritative_hash(&game), "world {world} step {step}: a refused {command:?} changed the world"),
            }
            if step % 40 == 39 {
                let loaded = decode(&encode(&game, &rules), &rules).expect("the save loads");
                assert_eq!(authoritative_hash(&loaded), authoritative_hash(&game), "world {world} step {step}: save and load");
            }
        }
    }
    assert!(accepted > 1000, "enough accepted commands: {accepted}");
}
