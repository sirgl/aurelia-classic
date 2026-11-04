//! Tests of the platform's render projection (`render::world::projection`) against the classic
//! module's rules: the incremental path equals the full rebuild after random commands, and what
//! the observer knows is what is drawn. They sit here, not in `render`, because they play classic
//! games and the platform builds without any module (docs/split-layout.md).
use render::world::projection::*;
use render::world::state::*;
use kernel::ids::{PlayerId, TileId};
use kernel::transaction::journal::ChangeJournal;
use kernel::ids::{CityHandle, ImprovementId, ResourceId, TechId, UnitHandle, UnitKindId};
use classic_rules::rules::SourceRules;
use classic_rules::scenario::{generate, generate_player_game, ScenarioConfig};
use classic_rules::{compile_rules, Command, CompiledRuleset, GameState};

fn world_cfg() -> (GameState, CompiledRuleset) {
    let rules = compile_rules(SourceRules::default()).unwrap();
    let cfg = ScenarioConfig {
        seed: 5,
        players: 2,
        width: 16,
        height: 16,
        cities: 2,
        units: 4,
        water_ratio: 10,
    };
    (generate(&cfg, &rules), rules)
}

fn game_rules() -> CompiledRuleset {
    compile_rules(classic_content::core::core_rules()).unwrap()
}

/// Deterministic xorshift for the differential test.
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

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

fn all_units(game: &GameState) -> Vec<UnitHandle> {
    let auth = game.view().auth;
    let mut out = Vec::new();
    for p in 0..auth.players().count() {
        out.extend_from_slice(auth.indexes().units_of(PlayerId(p as u8)));
    }
    out
}

fn near(game: &GameState, rng: &mut Rng, unit: UnitHandle, reach: i32) -> TileId {
    let auth = game.view().auth;
    let dims = auth.map().dimensions;
    let tile = auth.units().get(unit).map_or(TileId(0), |u| u.core.tile);
    let q = (tile.0 % dims.width) as i32 + rng.below(2 * reach as usize + 1) as i32 - reach;
    let r = (tile.0 / dims.width) as i32 + rng.below(2 * reach as usize + 1) as i32 - reach;
    let q = q.clamp(0, dims.width as i32 - 1) as u32;
    let r = r.clamp(0, dims.height as i32 - 1) as u32;
    TileId(r * dims.width + q)
}

/// A random command, mostly of the active player (turns are sequential: other players'
/// commands fail validation); defenders and cities are anyone's.
fn random_command(rng: &mut Rng, game: &GameState) -> Command {
    let auth = game.view().auth;
    let players = auth.players().count();
    let units = all_units(game);
    let active = auth.active_player();
    let own = auth.indexes().units_of(active);
    let unit = match (own.is_empty(), units.is_empty()) {
        (false, _) if rng.below(10) < 9 => own[rng.below(own.len())],
        (_, false) => units[rng.below(units.len())],
        _ => UnitHandle { index: 0, generation: 0 },
    };
    let cities: Vec<CityHandle> = auth.cities().iter().map(|(h, _)| h).collect();
    let city = if cities.is_empty() {
        CityHandle { index: 0, generation: 0 }
    } else {
        cities[rng.below(cities.len())]
    };
    let player = if rng.below(10) < 9 { active } else { PlayerId(rng.below(players) as u8) };
    let tiles = auth.map().tile_count();
    match rng.below(100) {
        0..=29 => Command::MoveUnit { unit, destination: near(game, rng, unit, 1) },
        30..=39 => Command::MarchTo { unit, destination: near(game, rng, unit, 6) },
        40..=45 => Command::FoundCity { settler: unit },
        46..=51 => Command::BuildRoad { unit },
        52..=57 => Command::BuildImprovement { unit, improvement: ImprovementId(rng.below(5) as u16) },
        58..=62 => Command::ChopForest { unit },
        63..=66 => Command::Fortify { unit },
        67..=71 => {
            let defender = if units.is_empty() { unit } else { units[rng.below(units.len())] };
            Command::AttackUnit { attacker: unit, defender }
        }
        72..=74 => Command::AttackCity { attacker: unit, city },
        75..=77 => Command::DeclareWar { player, enemy: PlayerId(((player.0 as usize + 1) % players) as u8) },
        78..=80 => Command::PurchaseTile { player, city, tile: TileId(rng.below(tiles) as u32) },
        81..=83 => Command::ChooseTechnology { player, technology: TechId(rng.below(8) as u16) },
        84..=85 => Command::EnqueueProduction {
            player,
            city,
            item: kernel::ids::ProductionItem::Unit(UnitKindId(rng.below(4) as u16)),
        },
        _ => Command::EndTurn { player },
    }
}

fn assert_same_content(fast: &RenderWorld, oracle: &RenderWorld, context: &str) {
    let differs = changed_layers(fast, oracle);
    assert!(differs.is_empty(), "{context}: apply != rebuild in {:?}", differs.layers());
}

/// §62: incremental projection == full rebuild after every command of random sequences,
/// and apply bumps exactly the layers whose content changed.
#[test]
fn incremental_projection_matches_rebuild_after_random_commands() {
    let rules = game_rules();
    let mut rng = Rng(0x5EED_CAFE);
    let mut applied = 0;
    let mut bumps = LayerSet::default();
    for world_index in 0..12 {
        let cfg = ScenarioConfig {
            seed: rng.next(),
            players: 2 + rng.below(3) as u8,
            width: 14 + rng.below(12) as u32,
            height: 12 + rng.below(10) as u32,
            cities: 0,
            units: 0,
            water_ratio: [10, 25, 40][rng.below(3)],
        };
        let mut game = generate_player_game(&rules, &cfg, 0);
        let observer = PlayerId(rng.below(cfg.players as usize) as u8);
        let mut fast = rebuild_render_world(game.view(), observer, &rules);
        for step in 0..220 {
            let command = random_command(&mut rng, &game);
            let Ok(result) = game.execute(&rules, command.clone()) else {
                continue;
            };
            let before = fast.clone();
            let changed = apply_changes(&mut fast, game.view(), result.changes(), &rules);
            let oracle = rebuild_render_world(game.view(), observer, &rules);
            let context = format!("world {world_index} step {step} {command:?}");
            assert_same_content(&fast, &oracle, &context);
            assert_eq!(changed, changed_layers(&before, &fast), "{context}: wrong layer bumps");
            for layer in ALL_LAYERS {
                let bumped = fast.layers.get(layer) != before.layers.get(layer);
                assert_eq!(bumped, changed.contains(layer), "{context}: {layer:?} revision");
            }
            applied += 1;
            bumps.0 |= changed.0;
        }
    }
    assert!(applied > 500, "differential test needs real commands, got {applied}");
    for layer in [Layer::Terrain, Layer::Units, Layer::Cities, Layer::Borders, Layer::Knowledge] {
        assert!(bumps.contains(layer), "random commands never changed {layer:?}");
    }
}

/// Quiet batches use refresh: same content as the oracle, bumps only differing layers.
#[test]
fn refresh_after_quiet_commands_matches_rebuild() {
    let rules = game_rules();
    let mut rng = Rng(0xBA7C4);
    let cfg = ScenarioConfig {
        seed: 9,
        players: 3,
        width: 20,
        height: 16,
        cities: 0,
        units: 0,
        water_ratio: 25,
    };
    let mut game = generate_player_game(&rules, &cfg, 0);
    let mut world = rebuild_render_world(game.view(), PlayerId(0), &rules);
    for batch in 0..30 {
        for _ in 0..12 {
            let command = random_command(&mut rng, &game);
            let _ = game.execute(&rules, command);
        }
        let before = world.clone();
        let changed = refresh(&mut world, game.view(), PlayerId(0), &rules);
        let oracle = rebuild_render_world(game.view(), PlayerId(0), &rules);
        assert_same_content(&world, &oracle, &format!("batch {batch}"));
        assert_eq!(changed, changed_layers(&before, &oracle));
        let untouched = refresh(&mut world, game.view(), PlayerId(0), &rules);
        assert!(untouched.is_empty(), "a second refresh changes nothing");
    }
}

/// A move inside explored land changes units and visibility only: no terrain, materials,
/// features or borders work downstream.
#[test]
fn move_in_explored_land_bumps_only_units_and_knowledge() {
    let rules = game_rules();
    let cfg = ScenarioConfig {
        seed: 20260928,
        players: 2,
        width: 24,
        height: 20,
        cities: 0,
        units: 0,
        water_ratio: 10,
    };
    let mut game = generate_player_game(&rules, &cfg, 0);
    let mut world = rebuild_render_world(game.view(), PlayerId(0), &rules);
    let unit = game.view().auth.indexes().units_of(PlayerId(0))[0];
    let home = game.view().auth.units().get(unit).unwrap().core.tile;
    let dims = game.view().auth.map().dimensions;
    let away = kernel::math::tiles_in_hex_radius(&dims, home, 1)
        .into_iter()
        .find(|t| *t != home && game.view().auth.map().terrain(*t).map(|k| k.0) != Some(4))
        .expect("a land neighbour");
    let mut moves = 0;
    for _ in 0..40 {
        let here = game.view().auth.units().get(unit).unwrap().core.tile;
        let target = if here == home { away } else { home };
        let command = Command::MoveUnit { unit, destination: target };
        let Ok(result) = game.execute(&rules, command) else {
            for p in 0..cfg.players {
                let _ = game.execute(&rules, Command::EndTurn { player: PlayerId(p) });
            }
            refresh(&mut world, game.view(), PlayerId(0), &rules);
            continue;
        };
        let changed = apply_changes(&mut world, game.view(), result.changes(), &rules);
        assert!(changed.contains(Layer::Units), "the unit moved");
        moves += 1;
        if moves > 2 {
            // Both tiles and their surroundings are explored after the first round trip.
            for layer in [Layer::Terrain, Layer::Materials, Layer::Features, Layer::Borders] {
                assert!(!changed.contains(layer), "move {moves} bumped {layer:?}");
            }
        }
    }
    assert!(moves >= 6, "fixture must exercise moves in explored land, got {moves}");
}

#[test]
fn hidden_enemy_excluded_from_projection() {
    // The renderer sees no hidden unit: the same guarantee as the AI.
    let rules = compile_rules(SourceRules::default()).unwrap();
    let mut game = classic_rules::GameState::new(
        &rules,
        classic_rules::GameConfig {
            players: 2,
            width: 32,
            height: 32,
            rng_seed: 0,
        },
    );
    let dims = game.view().auth.map().dimensions;
    let t0 = dims.to_tile(kernel::math::HexCoord { q: 2, r: 2 }).unwrap();
    let far = dims.to_tile(kernel::math::HexCoord { q: 30, r: 30 }).unwrap();
    game.debug_spawn_unit(&rules, PlayerId(0), t0, UnitKindId(0));
    game.debug_spawn_unit(&rules, PlayerId(1), far, UnitKindId(0));
    let rw = rebuild_render_world(game.view(), PlayerId(0), &rules);
    assert_eq!(rw.units.len(), 1, "only own unit projected");
    assert_eq!(rw.units[0].owner, 0);
}

#[test]
fn unknown_tiles_carry_no_geography() {
    let (game, rules) = world_cfg();
    let world = rebuild_render_world(game.view(), PlayerId(0), &rules);
    let unknown: Vec<usize> = (0..world.terrain.len()).filter(|i| !world.is_known(*i)).collect();
    assert!(!unknown.is_empty(), "fixture must have unexplored tiles");
    for i in unknown {
        assert_eq!(world.terrain[i].terrain, UNKNOWN_TERRAIN);
        assert!(world.terrain[i].owner.is_none() && world.terrain[i].resource.is_none());
        assert_eq!((world.forest[i], world.improvements[i], world.road[i], world.river[i]), (0, None, 0, 0));
    }
    for (a, b) in &world.river_edges {
        assert!(world.explored[*a as usize] == 1 && world.explored[*b as usize] == 1);
    }
}

#[test]
fn mask_unknown_matches_projection_rules() {
    let (game, rules) = world_cfg();
    let mut raw = rebuild_render_world(game.view(), PlayerId(0), &rules);
    let reference = raw.clone();
    mask_unknown(&mut raw);
    assert!(changed_layers(&raw, &reference).is_empty(), "projection is already masked");
}

#[test]
fn empty_journal_changes_nothing() {
    let (game, rules) = world_cfg();
    let mut rw = rebuild_render_world(game.view(), PlayerId(0), &rules);
    let r0 = rw.layers;
    let changed = apply_changes(&mut rw, game.view(), &ChangeJournal::default(), &rules);
    assert!(changed.is_empty());
    assert_eq!(rw.layers, r0, "empty journal must not bump revisions");
}

/// Regression (P1): the projection carried the true terrain, owner, resource and layers
/// of every tile, unknown ones included, and the present state of tiles out of sight;
/// the HUD showed what the player could not know. Unknown tiles now carry a placeholder
/// and nothing else, remembered tiles what the player last saw.
#[test]
fn projection_shows_knowledge_not_truth() {
    use kernel::ids::TerrainId;
    let rules = compile_rules(SourceRules::default()).unwrap();
    let mut game = classic_rules::GameState::new(
        &rules,
        classic_rules::GameConfig {
            players: 2,
            width: 32,
            height: 32,
            rng_seed: 0,
        },
    );
    let dims = game.view().auth.map().dimensions;
    let tile = |q, r| dims.to_tile(kernel::math::HexCoord { q, r }).unwrap();
    let far = tile(28, 28);
    game.debug_set_terrain_batch(&rules, &[(far, TerrainId(2))]);
    game.debug_found_city(&rules, PlayerId(1), far);
    let scout = game.debug_spawn_unit(&rules, PlayerId(0), tile(10, 10), UnitKindId(0));
    let rw = rebuild_render_world(game.view(), PlayerId(0), &rules);
    let cell = rw.terrain[far.0 as usize];
    assert_eq!((cell.terrain, cell.owner), (UNKNOWN_TERRAIN, None), "unknown tile");
    assert!(rw.cities.is_empty(), "an unknown city is not drawn");
    // A tile seen, then left: it keeps the owner the scout saw (none), though P1 claims
    // it later.
    let seen = tile(12, 10);
    let walk = Command::MoveUnit { unit: scout, destination: tile(9, 10) };
    game.execute(&rules, walk).unwrap();
    game.execute(&rules, Command::EndTurn { player: PlayerId(0) }).unwrap();
    game.debug_found_city(&rules, PlayerId(1), tile(13, 10));
    let rw = rebuild_render_world(game.view(), PlayerId(0), &rules);
    assert_eq!(game.view().auth.map().owner(seen).flatten(), Some(PlayerId(1)));
    assert_eq!(rw.terrain[seen.0 as usize].owner, None, "remembered as unowned");
    assert_eq!(rw.visible[seen.0 as usize], 0);
    assert_eq!(rw.explored[seen.0 as usize], 1);
}

#[test]
fn fogged_tiles_show_remembered_improvements_routes_and_natural_wonders() {
    use classic_rules::rules::content::NaturalWonderId;
    use classic_rules::world::map::Route;
    let rules = game_rules();
    let mut game = GameState::new(
        &rules,
        classic_rules::GameConfig { players: 2, width: 16, height: 16, rng_seed: 0 },
    );
    let far = TileId(12 * 16 + 12);
    game.debug_set_natural_wonder_batch(&rules, &[(far, NaturalWonderId(0))]);
    game.debug_reveal(PlayerId(0), &[far]);
    let known = rebuild_render_world(game.view(), PlayerId(0), &rules);
    let i = far.0 as usize;
    assert!(known.is_known(i) && known.visible[i] == 0);
    assert_eq!(known.natural_wonders[i], Some(0), "natural wonders never change: shown when known");
    game.debug_set_improvement_batch(&rules, &[(far, ImprovementId(0))]);
    game.debug_set_route_batch(&rules, &[(far, Route::Railroad)]);
    let fogged = rebuild_render_world(game.view(), PlayerId(0), &rules);
    assert_eq!((fogged.improvements[i], fogged.road[i]), (None, 0), "built unseen: not shown");
    game.debug_spawn_unit(&rules, PlayerId(0), far, UnitKindId(0));
    let seen = rebuild_render_world(game.view(), PlayerId(0), &rules);
    assert_eq!((seen.improvements[i], seen.road[i]), (Some(0), 2), "seen again: memory caught up");
}

/// R355: a deposit on an explored hex out of sight is read from the map and filtered by
/// the observer's technologies: uranium shows on remembered land the moment Atomic Theory
/// is learned, a deposit that appeared later shows at once, an unexplored hex shows nothing.
#[test]
fn a_remembered_hex_shows_a_late_deposit_with_its_technology() {
    let rules = game_rules();
    let key = |table: &[String], wanted: &str| table.iter().position(|k| k == wanted).unwrap();
    let resources: Vec<String> = rules.resources.iter().map(|r| r.key.clone()).collect();
    let uranium = ResourceId(key(&resources, "core:resource/uranium") as u16);
    let tech = rules.resources[uranium.0 as usize].required_tech.expect("uranium needs a technology");
    let mut game = GameState::new(
        &rules,
        classic_rules::GameConfig { players: 2, width: 16, height: 16, rng_seed: 0 },
    );
    let (known, unknown) = (TileId(12 * 16 + 12), TileId(13 * 16 + 3));
    game.debug_reveal(PlayerId(0), &[known]);
    game.debug_set_resource_batch(&rules, &[(known, uranium), (unknown, uranium)]);
    let shown = |game: &GameState, tile: TileId| rebuild_render_world(game.view(), PlayerId(0), &rules).terrain[tile.0 as usize].resource;
    let world = rebuild_render_world(game.view(), PlayerId(0), &rules);
    assert!(world.is_known(known.0 as usize) && world.visible[known.0 as usize] == 0, "remembered");
    assert_eq!(shown(&game, known), None, "hidden before Atomic Theory");
    game.debug_learn(&rules, PlayerId(0), tech);
    assert_eq!(shown(&game, known), Some(uranium.0), "shown on the remembered hex at once");
    assert_eq!(shown(&game, unknown), None, "an unexplored hex shows nothing");
    let world = rebuild_render_world(game.view(), PlayerId(0), &rules);
    assert!(world.unlocked_resources.contains(&uranium.0));
}

/// The feature kinds come from the roles the content binds, not from keys:
/// every classic feature draws as the key table before the roles drew it.
#[test]
fn classic_features_draw_as_their_role() {
    let rules = game_rules();
    let expected = |key: &str| match key {
        "core:feature/forest" => FeatureKind::Forest,
        "core:feature/jungle" => FeatureKind::Jungle,
        "core:feature/marsh" => FeatureKind::Marsh,
        "core:feature/oasis" => FeatureKind::Oasis,
        "core:feature/flood-plains" => FeatureKind::FloodPlains,
        "core:feature/ice" => FeatureKind::Ice,
        "core:feature/atoll" => FeatureKind::Atoll,
        "core:feature/fallout" => FeatureKind::Fallout,
        _ => FeatureKind::Other,
    };
    assert_eq!(rules.features.len(), 8);
    for (i, feature) in rules.features.iter().enumerate() {
        let kind = FeatureKind::of(&rules, kernel::ids::FeatureId(i as u16));
        assert_eq!(kind, expected(&feature.key), "{}", feature.key);
    }
}

/// The sixth yield channel is the rules module's yield (classic: faith).
#[test]
fn tile_yields_end_with_the_module_channel() {
    use kernel::rules::content::Yields;
    let yields = Yields { food: 1, production: 2, gold: 3, science: 4, culture: 5, module: 200 };
    assert_eq!(tile_yields::<classic_rules::classic::Classic>(yields), [1, 2, 3, 4, 5, i8::MAX]);
}

/// A city out of sight is drawn as the observer last saw it, through the fast path
/// as through the rebuild: growth, a razed hex or a quiet round does not move the picture,
/// seeing the city again does.
#[test]
fn a_remembered_city_is_drawn_as_last_seen_by_both_paths() {
    let rules = game_rules();
    let mut game = GameState::new(&rules, classic_rules::GameConfig { players: 2, width: 24, height: 24, rng_seed: 3 });
    let dims = game.view().auth.map().dimensions;
    let tile = |q, r| dims.to_tile(kernel::math::HexCoord { q, r }).unwrap();
    let site = tile(14, 8);
    let city = game.debug_found_city(&rules, PlayerId(1), site);
    let scout = game.debug_spawn_unit(&rules, PlayerId(0), tile(12, 8), UnitKindId(0));
    let mut fast = rebuild_render_world(game.view(), PlayerId(0), &rules);
    assert_eq!(fast.cities.len(), 1, "the city in sight is live");
    let step = |game: &mut GameState, fast: &mut RenderWorld, command: Command| {
        let result = game.execute(&rules, command).unwrap();
        apply_changes(fast, game.view(), result.changes(), &rules);
        assert_same_content(fast, &rebuild_render_world(game.view(), PlayerId(0), &rules), "after a command");
    };
    step(&mut game, &mut fast, Command::MoveUnit { unit: scout, destination: tile(10, 8) });
    assert_eq!(fast.visible[site.0 as usize], 0, "the scout left");
    let seen_at = fast.cities[0].population;
    assert!(seen_at.is_some(), "a city a scout looked at has its numbers");
    game.debug_grow_city(&rules, city, 4);
    refresh(&mut fast, game.view(), PlayerId(0), &rules);
    assert_eq!(fast.cities.len(), 1);
    assert_eq!(fast.cities[0].population, seen_at, "growth out of sight is not drawn");
    step(&mut game, &mut fast, Command::EndTurn { player: PlayerId(0) });
    assert_eq!(fast.cities[0].population, seen_at);
}

/// A generated map's sea deposits (fish, sea oil) follow the observer's knowledge like the land's: an
/// unexplored tile shows nothing, an explored one shows its fish at once, and sea oil waits for the
/// technology that reveals it (Biology), whether the tile is seen or only remembered.
#[test]
fn generated_sea_deposits_follow_what_the_observer_knows() {
    let rules = game_rules();
    let key_of = |tile: TileId, game: &GameState| -> Option<String> {
        let map = game.view().auth.map();
        match map.resource(tile) {
            Some(Some(r)) if map.is_water(tile) => Some(rules.resources[r.0 as usize].key.clone()),
            _ => None,
        }
    };
    let biology = rules.resources.iter().find(|r| r.key == "core:resource/oil").and_then(|r| r.required_tech).expect("oil is revealed by a technology");
    let mut checked = false;
    for seed in 1..=12u64 {
        let cfg = ScenarioConfig { seed, players: 2, width: 40, height: 24, cities: 0, units: 0, water_ratio: classic_rules::scenario::MapScript::Continents.water_ratio(25) };
        let options = classic_rules::scenario::NewGameOptions { city_states: 4, script: classic_rules::scenario::MapScript::Continents, ..Default::default() };
        let mut game = classic_rules::scenario::generate_new_game_with(&rules, &cfg, Some(0), options);
        let world = rebuild_render_world(game.view(), PlayerId(0), &rules);
        let hidden = |name: &str| {
            (0..world.terrain.len() as u32)
                .map(TileId)
                .find(|&t| !world.is_known(t.0 as usize) && key_of(t, &game).as_deref() == Some(&format!("core:resource/{name}")))
        };
        let (Some(fish), Some(oil)) = (hidden("fish"), hidden("oil")) else {
            continue;
        };
        let shown = |game: &GameState, tile: TileId| rebuild_render_world(game.view(), PlayerId(0), &rules).terrain[tile.0 as usize].resource;
        assert_eq!((shown(&game, fish), shown(&game, oil)), (None, None), "unexplored sea shows nothing");
        game.debug_reveal(PlayerId(0), &[fish, oil]);
        let fish_id = game.view().auth.map().resource(fish).flatten().unwrap().0;
        assert_eq!(shown(&game, fish), Some(fish_id), "fish is shown on explored water");
        assert_eq!(shown(&game, oil), None, "sea oil is hidden before its technology");
        game.debug_learn(&rules, PlayerId(0), biology);
        let oil_id = game.view().auth.map().resource(oil).flatten().unwrap().0;
        assert_eq!(shown(&game, oil), Some(oil_id), "sea oil is shown once the technology is known");
        checked = true;
        break;
    }
    assert!(checked, "a seed within the first twelve has unexplored fish and sea oil");
}

/// A land unit afloat is projected as embarked, on both paths; a ship, a land unit on land
/// and a foreign unit out of sight are not (the last is not projected at all).
#[test]
fn a_land_unit_on_water_is_projected_as_embarked_and_only_when_seen() {
    let rules = game_rules();
    let cfg = ScenarioConfig { seed: 7, players: 2, width: 24, height: 24, cities: 0, units: 0, water_ratio: 45 };
    let mut game = generate_player_game(&rules, &cfg, 0);
    let kind = |key: &str| UnitKindId(rules.units.iter().position(|u| u.key == key).expect(key) as u16);
    let (warrior, trireme) = (kind("core:unit/warrior"), kind("core:unit/trireme"));
    let auth = game.view().auth;
    let kinds = auth.map().terrain_kinds();
    let tiles: Vec<TileId> = (0..auth.map().tile_count() as u32).map(TileId).collect();
    let is_water = |t: &TileId| auth.map().terrain(*t).is_some_and(|k| kinds.is_water(k.0));
    let home = auth.units().get(auth.indexes().units_of(PlayerId(0))[0]).unwrap().core.tile;
    let dims = auth.map().dimensions;
    let water = *tiles.iter().filter(|t| is_water(t)).min_by_key(|t| dims.distance(home, **t).unwrap_or(i32::MAX)).expect("water");
    let land = *tiles.iter().filter(|t| !is_water(t)).find(|t| **t != home).expect("land");
    let apart = |t: &&TileId| [home, water, land].iter().map(|o| dims.distance(*o, **t).unwrap_or(0)).min().unwrap_or(0);
    let hidden = *tiles.iter().filter(|t| is_water(t)).max_by_key(apart).expect("water out of sight");
    game.debug_spawn_unit(&rules, PlayerId(0), water, warrior);
    game.debug_spawn_unit(&rules, PlayerId(0), water, trireme);
    game.debug_spawn_unit(&rules, PlayerId(0), land, warrior);
    game.debug_spawn_unit(&rules, PlayerId(1), hidden, warrior);
    let world = rebuild_render_world(game.view(), PlayerId(0), &rules);
    let at = |tile: TileId, kind: UnitKindId| world.units.iter().find(|u| u.tile == tile && u.kind == kind.0 && u.owner == 0).map(|u| u.embarked);
    assert_eq!(at(water, warrior), Some(true), "a warrior on water is afloat");
    assert_eq!(at(water, trireme), Some(false), "a ship is not embarked");
    assert_eq!(at(land, warrior), Some(false), "a warrior ashore is not");
    assert_eq!(world.visible[hidden.0 as usize], 0, "the fixture hides the rival's tile");
    assert!(world.units.iter().all(|u| u.owner == 0), "the rival's unit out of sight is not projected");
    let mut fast = rebuild_render_world(game.view(), PlayerId(0), &rules);
    refresh(&mut fast, game.view(), PlayerId(0), &rules);
    assert_same_content(&fast, &world, "refresh");
}
