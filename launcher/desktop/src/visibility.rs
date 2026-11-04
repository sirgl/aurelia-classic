//! Observer view for hand-built showcase worlds.
//!
//! Game projections are already masked by `render::world::projection`. The visual
//! sandbox builds its world by hand, so it masks unknown geography with the same rules.
use render::world::projection::mask_unknown;
use render::world::state::RenderWorld;

pub fn projection(source: &RenderWorld) -> RenderWorld {
    let mut view = source.clone();
    mask_unknown(&mut view);
    view
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernel::ids::TileId;
    use render::world::state::{TerrainCell, UnitMarker};

    fn two_tiles() -> RenderWorld {
        RenderWorld {
            width: 2,
            height: 1,
            terrain: (0..2)
                .map(|i| TerrainCell {
                    tile: TileId(i),
                    terrain: 6,
                    owner: Some(1),
                    resource: Some(2),
                })
                .collect(),
            units: vec![
                UnitMarker {
                    id: 0, embarked: false, fortified: false, tile: TileId(0),
                    owner: 1,
                    kind: 4,
                    hp: 100,
                },
                UnitMarker {
                    id: 0, embarked: false, fortified: false, tile: TileId(1),
                    owner: 2,
                    kind: 4,
                    hp: 100,
                },
            ],
            visible: vec![1, 0],
            explored: vec![1, 0],
            improvements: vec![Some(0), Some(0)],
            road: vec![1, 1],
            river: vec![1, 1],
            forest: vec![1, 1],
            ..Default::default()
        }
    }

    #[test]
    fn unknown_tiles_lose_geography_and_hidden_units() {
        let view = projection(&two_tiles());
        assert_eq!(view.terrain[0].terrain, 6);
        assert_eq!(view.terrain[1].terrain, 1);
        assert!(view.terrain[1].owner.is_none() && view.terrain[1].resource.is_none());
        assert_eq!(
            (view.forest[1], view.improvements[1], view.road[1]),
            (0, None, 0)
        );
        assert_eq!(view.units.len(), 1);
    }
}
