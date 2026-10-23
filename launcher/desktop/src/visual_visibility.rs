//! Visibility fixture for the static presentation scene, not simulation knowledge storage.
use render::{scene::layout::tile_to_world, world::state::RenderWorld};
pub use super::visibility::projection;

pub fn apply(world: &mut RenderWorld, shifted: bool, reveal: bool) {
    let (cx, cz) = super::visual_scene::center();
    for cell in &world.terrain {
        let i = cell.tile.0 as usize;
        let (x, z) = tile_to_world(cell.tile.0 % world.width, cell.tile.0 / world.width);
        let dx = x - cx - if shifted { 10.0 } else { 0.0 };
        let dz = z - cz;
        let visible = reveal || (dx / 12.8).powi(2) + (dz / 12.3).powi(2) < 1.0;
        world.visible[i] = u8::from(visible);
        world.explored[i] |= u8::from(visible);
    }
    world.touch(render::world::state::Layer::Knowledge);
    world.touch(render::world::state::Layer::Terrain);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn visibility_keeps_memory_and_filters_hidden_units_and_geography() {
        let mut source = super::super::visual_scene::build();
        apply(&mut source, false, false);
        let remembered = source.explored.clone();
        apply(&mut source, true, false);
        for (old, new) in remembered.iter().zip(&source.explored) {
            assert!(*new >= *old);
        }
        let hidden = source.units[0].tile.0 as usize;
        source.visible[hidden] = 0;
        source.explored[hidden] = 0;
        source.terrain[hidden].terrain = 6;
        source.forest[hidden] = 1;
        source.improvements[hidden] = Some(0);
        let view = projection(&source);
        assert!(!view.units.iter().any(|u| u.tile.0 as usize == hidden));
        assert_eq!(view.terrain[hidden].terrain, 1);
        assert_eq!(view.forest[hidden], 0);
        assert_eq!(view.improvements[hidden], None);
        assert!(source
            .visible
            .iter()
            .zip(&source.explored)
            .all(|(v, e)| v <= e));
    }
}
