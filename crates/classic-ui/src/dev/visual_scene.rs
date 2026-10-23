use classic_rules::ids::TileId;
use render::scene::layout::tile_to_world;
use render::world::state::{CityMarker, RenderWorld, TerrainCell, UnitMarker};

pub const WIDTH: u32 = 32;
pub const HEIGHT: u32 = 28;
pub fn center() -> (f32, f32) {
    tile_to_world(12, 12)
}
fn noise(q: u32, r: u32) -> u32 {
    render::scene::terrain::fields::hash_u32(q.wrapping_mul(1777) ^ r.wrapping_mul(7981) ^ 7127)
}
/// The scene's ruleset: unit kinds and resources are looked up in it by content key.
pub fn rules() -> classic_rules::CompiledRuleset {
    classic_rules::rules::compile_rules(classic_content::loader::load_core_rules())
        .expect("embedded visual scene content")
}

/// AURELIA_VISUAL_SCENE=biomes: a map of the scenario generator (a land of every
/// terrain, feature and natural wonder site), fully revealed to player 0
/// through the game's own projection, plus a patch of fallout (which only
/// nuclear weapons make). AURELIA_VISUAL_SEED and AURELIA_VISUAL_MAP=WxH choose the map.
pub fn biomes() -> RenderWorld {
    use classic_rules::ids::{FeatureId, PlayerId};
    let rules = rules();
    let seed = std::env::var("AURELIA_VISUAL_SEED").ok().and_then(|v| v.parse().ok()).unwrap_or(11);
    let (width, height) = std::env::var("AURELIA_VISUAL_MAP")
        .ok()
        .and_then(|v| v.split_once('x').map(|(w, h)| (w.parse().unwrap_or(64), h.parse().unwrap_or(44))))
        .unwrap_or((64, 44));
    let config = classic_rules::scenario::ScenarioConfig { seed, players: 2, width, height, cities: 0, units: 0, water_ratio: 35 };
    let mut game = classic_rules::scenario::generate(&config, &rules);
    if let Some(fallout) = rules.features.iter().position(|f| f.key == "core:feature/fallout") {
        let map = game.view().auth.map();
        let land: Vec<TileId> = (0..map.tile_count() as u32)
            .map(TileId)
            .filter(|t| map.terrain(*t).is_some_and(|k| matches!(k.0, 0 | 1)) && map.feature(*t) == Some(None))
            .skip(map.tile_count() / 5)
            .take(3)
            .collect();
        let batch: Vec<_> = land.iter().map(|t| (*t, FeatureId(fallout as u16))).collect();
        game.debug_set_feature_batch(&rules, &batch);
    }
    let all: Vec<TileId> = (0..width * height).map(TileId).collect();
    game.debug_reveal(PlayerId(0), &all);
    let mut world = render::world::projection::rebuild_render_world(game.view(), PlayerId(0), &rules);
    // Where to look: one tile of every feature and terrain, as camera centres.
    let mut listed: Vec<String> = Vec::new();
    for i in 0..world.terrain.len() {
        let name = match (world.natural_wonders[i], world.feature[i]) {
            (Some(w), _) => world.natural_wonder_keys[w as usize].clone(),
            (None, render::world::state::FeatureKind::None) => format!("terrain {}", world.terrain[i].terrain),
            (None, kind) => format!("{kind:?}"),
        };
        if !listed.contains(&name) {
            let (x, z) = tile_to_world(i as u32 % width, i as u32 / width);
            eprintln!("biomes: {name} at AURELIA_VISUAL_CENTER={x:.1},{z:.1}");
            listed.push(name);
        }
    }
    world.visual_surface = true;
    world.observer = 0;
    world.visible.fill(1);
    world.explored.fill(1);
    world.touch_all();
    world
}

pub fn build() -> RenderWorld {
    let count = (WIDTH * HEIGHT) as usize;
    let mut w = RenderWorld {
        visual_surface: std::env::var("AURELIA_VISUAL_SURFACE").as_deref() != Ok("legacy"),
        width: WIDTH,
        height: HEIGHT,
        observer: 1,
        layers: render::world::state::LayerRevisions::INITIAL,
        visible: vec![1; count],
        explored: vec![1; count],
        forest: vec![0; count],
        improvements: vec![None; count],
        natural_wonders: vec![None; count],
        road: vec![0; count],
        river: vec![0; count],
        ..Default::default()
    };
    let (cx, cz) = center();
    for r in 0..HEIGHT {
        for q in 0..WIDTH {
            let t = r * WIDTH + q;
            let (x, z) = tile_to_world(q, r);
            let (dx, dz) = (x - cx, z - cz);
            let n = noise(q, r);
            let coast = 7.3 + 1.3 * (dz * 0.46).sin() + 0.7 * (dz * 0.95).cos();
            let water = dx > coast || dx < -18.0 + 1.5 * (dz * 0.6).sin() || dz.abs() > 18.0;
            let peak = ((dx + 5.5) / 3.2).powi(2) + ((dz + 8.0) / 3.1).powi(2) < 1.0
                || ((dx + 11.0) / 3.0).powi(2) + ((dz - 0.5) / 4.6).powi(2) < 1.0
                || ((dx + 8.0) / 2.5).powi(2) + ((dz - 12.5) / 2.7).powi(2) < 1.0;
            let terrain = if water {
                4
            } else if peak {
                6
            } else if n % 13 == 0 {
                2
            } else if n % 3 == 0 {
                0
            } else {
                1
            };
            w.terrain.push(TerrainCell {
                tile: TileId(t),
                terrain,
                owner: None,
                resource: None,
            });
            if !water && !peak && n % 100 < 57 {
                w.forest[t as usize] = 1;
            }
            let known = (dx / 15.5).powi(2) + (dz / 15.5).powi(2) < 1.0;
            if !known {
                w.visible[t as usize] = 0;
                w.explored[t as usize] = 0;
            }
        }
    }
    // Curated open valley, settlement and fields remain readable at strategic scale.
    for (q, r) in [
        (15, 12),
        (14, 12),
        (15, 11),
        (14, 13),
        (13, 14),
        (14, 14),
        (12, 15),
        (11, 15),
        (12, 16),
        (13, 16),
        (10, 12),
        (11, 12),
        (10, 13),
        (11, 13),
        (14, 11),
    ] {
        let t = (r * WIDTH + q) as usize;
        w.terrain[t].terrain = 1;
        w.forest[t] = 0;
    }
    w.cities.push(CityMarker {
        tile: TileId(12 * WIDTH + 15),
        owner: 1,
        name: "Rivermere".into(),
        population: Some(6),
        buildings: Vec::new(),
        era: 0,
        showpieces: Vec::new(),
        unworked: vec![],
    });
    // City territory is tile ownership, so its border follows the same renderer
    // and knowledge filtering as any other civilization's territory.
    for cell in &mut w.terrain {
        let dq = (cell.tile.0 % WIDTH) as i32 - 15;
        let dr = (cell.tile.0 / WIDTH) as i32 - 12;
        if dq.abs().max(dr.abs()).max((dq + dr).abs()) <= 2 {
            cell.owner = Some(1);
        }
    }
    let rules = rules();
    let kinds = crate::ui::unit_kinds::UnitKinds::from_rules(&rules);
    let kind = |key: &str| kinds.kind_of(key).expect("unit exists in visual scene content");
    for (q, r, owner) in [(12, 16, 1), (10, 12, 0)] {
        w.units.push(UnitMarker {
            id: 0, embarked: false, fortified: false, tile: TileId(r * WIDTH + q),
            owner,
            kind: kind("core:unit/warrior"),
            hp: 100,
        });
    }
    // Civilian companies make the contextual action artwork inspectable in-map.
    for (q, r, key) in [(13, 12, "core:unit/settler"), (14, 13, "core:unit/worker")] {
        w.units.push(UnitMarker { id: 0, embarked: false, fortified: false, tile: TileId(r * WIDTH + q), owner: 1, kind: kind(key), hp: 100 });
    }
    let resource = |key: &str| rules.resources.iter().position(|r| r.key == key)
        .unwrap_or_else(|| panic!("{key} exists in visual scene content")) as u16;
    let farm = rules.improvements.iter().position(|i| i.key == "core:improvement/farm")
        .expect("farm exists in visual scene content") as u16;
    let wheat = resource("core:resource/wheat");
    w.unlocked_resources.extend([wheat, resource("core:resource/deer"), resource("core:resource/horses")]);
    for (q, r) in [(14, 13), (13, 14), (14, 11), (14, 14)] {
        let t = (r * WIDTH + q) as usize;
        w.improvements[t] = Some(farm);
        w.terrain[t].resource = Some(wheat);
    }
    // One continuous chain of SHARED EDGES, not a walk between tile centers.
    // Successive tile pairs meet at an exact hex corner; the last reaches the sea.
    let river_edges = [
        ((11, 6), (12, 6)),
        ((11, 6), (11, 7)),
        ((10, 7), (11, 7)),
        ((11, 7), (10, 8)),
        ((10, 8), (11, 8)),
        ((11, 8), (10, 9)),
        ((10, 9), (11, 9)),
        ((11, 9), (10, 10)),
        ((10, 10), (11, 10)),
        ((11, 10), (10, 11)),
        ((10, 11), (11, 11)),
        ((11, 11), (10, 12)),
        ((10, 12), (11, 12)),
        ((11, 12), (10, 13)),
        ((11, 12), (11, 13)),
        ((12, 12), (11, 13)),
        ((11, 13), (12, 13)),
        ((11, 13), (11, 14)),
        ((10, 14), (11, 14)),
        ((10, 14), (10, 15)),
        ((9, 15), (10, 15)),
        ((10, 15), (9, 16)),
        ((9, 16), (10, 16)),
        ((10, 16), (9, 17)),
        ((9, 17), (10, 17)),
        ((10, 17), (9, 18)),
        ((9, 18), (10, 18)),
        ((10, 18), (9, 19)),
        ((10, 18), (10, 19)),
        ((11, 18), (10, 19)),
        ((11, 18), (11, 19)),
        ((12, 18), (11, 19)),
        ((12, 18), (12, 19)),
    ];
    for (index, ((aq, ar), (bq, br))) in river_edges.into_iter().enumerate() {
        let a = ar * WIDTH + aq;
        let b = br * WIDTH + bq;
        w.river_edges.push((a.min(b), a.max(b)));
        for t in [a, b] {
            w.river[t as usize] = 1;
            w.forest[t as usize] = 0;
            if index > 1 && w.terrain[t as usize].terrain != 4 {
                w.terrain[t as usize].terrain = 1;
            }
        }
    }
    let lighthouse = (9 * WIDTH + 18) as usize;
    w.terrain[lighthouse].terrain = 2;
    w.forest[lighthouse] = 0;
    let (cityx, cityz) = tile_to_world(15, 12);
    for cell in &w.terrain {
        let (x, z) = tile_to_world(cell.tile.0 % WIDTH, cell.tile.0 / WIDTH);
        if (x - cityx).hypot(z - cityz) < 2.5 {
            w.forest[cell.tile.0 as usize] = 0;
        }
    }
    // Give coves and outcrops room to read instead of burying the coast in pines.
    for cell in &w.terrain {
        let id = cell.tile.0;
        let q = (id % WIDTH) as i32;
        let r = (id / WIDTH) as i32;
        let coastal = render::scene::layout::EDGE_NEIGHBORS
            .iter()
            .any(|(dq, dr)| {
                let (nq, nr) = (q + dq, r + dr);
                nq >= 0
                    && nr >= 0
                    && nq < WIDTH as i32
                    && nr < HEIGHT as i32
                    && w.terrain[(nr as u32 * WIDTH + nq as u32) as usize].terrain == 4
            });
        if coastal && noise(q as u32, r as u32) % 3 != 0 {
            w.forest[id as usize] = 0;
        }
    }
    // Grazing herds: real resources, drawn by their world models.
    for (q, r, key) in [(13, 9, "core:resource/deer"), (12, 14, "core:resource/horses")] {
        let tile = (r * WIDTH + q) as usize;
        w.terrain[tile].terrain = 1;
        w.terrain[tile].resource = Some(resource(key));
        w.forest[tile] = 0;
    }
    w.river_edges.sort();
    w.river_edges.dedup();
    w
}

/// Presentation-only occlusion check, not a change to gameplay occupancy rules.
pub fn place_occlusion_study(world: &mut RenderWorld) {
    let forest_tile = TileId(13 * WIDTH + 15);
    world.terrain[forest_tile.0 as usize].terrain = 1;
    world.forest[forest_tile.0 as usize] = 1;
    world.units[0].tile = forest_tile;
    world.units[1].tile = world.cities[0].tile;
    world.units[1].owner = world.cities[0].owner;
}

#[cfg(test)]
mod tests {
    use super::*;
    use render::scene::{layout::EDGE_NEIGHBORS, terrain::corner_lattice};
    use std::collections::BTreeMap;
    #[test]
    fn river_edges_are_one_unbroken_chain_from_mountain_to_sea() {
        let w = build();
        let mut degrees = BTreeMap::new();
        for &(a, b) in &w.river_edges {
            let (aq, ar) = ((a % WIDTH) as i32, (a / WIDTH) as i32);
            let (bq, br) = ((b % WIDTH) as i32, (b / WIDTH) as i32);
            let i = EDGE_NEIGHBORS
                .iter()
                .position(|&(dq, dr)| aq + dq == bq && ar + dr == br)
                .expect("adjacent tiles");
            for p in [
                corner_lattice(aq, ar, i),
                corner_lattice(aq, ar, (i + 1) % 6),
            ] {
                *degrees.entry(p).or_insert(0) += 1;
            }
        }
        assert_eq!(degrees.values().filter(|&&d| d == 1).count(), 2);
        assert!(degrees.values().all(|&d| d == 1 || d == 2));
        assert_eq!(
            degrees.len(),
            w.river_edges.len() + 1,
            "one simple chain, no loops"
        );
        assert!(w
            .river_edges
            .iter()
            .any(|&(a, b)| [a, b].iter().any(|&t| w.terrain[t as usize].terrain == 4)));
        assert!(w
            .river_edges
            .iter()
            .any(|&(a, b)| [a, b].iter().any(|&t| w.terrain[t as usize].terrain == 6)));
    }
}
