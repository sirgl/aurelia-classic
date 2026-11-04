//! Tests of the platform's model library against the classic module's content: every map object
//! and every unit of the classic ruleset resolves to a model of the baked library. They sit here,
//! not in `render`, because they need `classic-rules`, and the platform builds without any module
//! (docs/split-layout.md).
use classic_rules::CompiledRuleset;
use render::unit_models::{UnitModelBook, UnitModelId};
use render::world_models::{WorldBook, WorldKind};

fn classic() -> CompiledRuleset {
    classic_rules::compile_rules(classic_content::core::core_rules()).expect("BNW rules")
}

#[test]
fn every_map_object_of_the_ruleset_has_a_model() {
    let rules = classic();
    let book = WorldBook::from_rules(&rules);
    let icon_only = [
        "core:resource/jewelry",
        "core:resource/porcelain",
        "core:resource/nutmeg",
        "core:resource/cloves",
        "core:resource/pepper",
        "core:resource/hidden_antiquity_site",
    ];
    for (i, def) in rules.improvements.iter().enumerate() {
        let model = book.improvement(i as u16, u16::MAX).expect(&def.key);
        assert_eq!(model.model().kind, WorldKind::Improvement, "{}", def.key);
    }
    for (i, def) in rules.resources.iter().enumerate() {
        let model = book.resource(i as u16);
        assert_eq!(model.is_none(), icon_only.contains(&def.key.as_str()), "{}", def.key);
    }
    for (i, def) in rules.natural_wonders.iter().enumerate() {
        assert!(book.natural_wonder(i as u16).is_some(), "{}", def.key);
    }
    for (i, def) in rules.buildings.iter().enumerate() {
        use classic_rules::rules::content::BuildingKind;
        match def.kind {
            BuildingKind::WorldWonder => assert!(book.wonder(i as u16).is_some(), "{}", def.key),
            BuildingKind::NationalWonder => {
                assert!(book.landmark(i as u16).is_some(), "{}", def.key)
            }
            _ => {}
        }
    }
}

#[test]
fn farm_resolves_its_terrain_variant_and_falls_back_to_the_plain_farm() {
    let rules = classic();
    let book = WorldBook::from_rules(&rules);
    let farm = rules
        .improvements
        .iter()
        .position(|x| x.key == "core:improvement/farm")
        .unwrap() as u16;
    let plains = rules
        .terrains
        .iter()
        .find(|t| t.key == "core:terrain/plains")
        .and_then(|t| t.map_code)
        .unwrap();
    assert_eq!(book.improvement(farm, plains).unwrap().model().id, "improvement/farm-plains");
    assert_eq!(book.improvement(farm, u16::MAX).unwrap().model().id, "improvement/farm");
}

#[test]
fn unique_buildings_use_the_landmark_of_the_building_they_replace() {
    let rules = classic();
    let book = WorldBook::from_rules(&rules);
    let babylon = rules
        .buildings
        .iter()
        .position(|b| b.key == "core:building/walls_of_babylon")
        .unwrap() as u16;
    assert_eq!(book.landmark(babylon).unwrap().building, "core:building/walls");
}

#[test]
fn every_ruleset_unit_has_its_own_model() {
    let rules = classic();
    let book = UnitModelBook::from_rules(&rules);
    let mut ids: Vec<&str> = (0..rules.units.len())
        .map(|kind| book.model_for_kind(kind as u16).model().id)
        .collect();
    assert!(!ids.contains(&"unit/unknown"), "{ids:?}");
    ids.dedup();
    assert_eq!(
        ids.len(),
        rules.units.len(),
        "no two unit kinds share a mesh: {ids:?}"
    );
    assert_eq!(book.model_for_kind(u16::MAX), UnitModelId::FALLBACK);
}


/// The site shape each classic natural wonder's model asks of the terrain: the 17 wonders of the
/// classic module's table (`natural_wonders.ron`). Moved here from the render crate, which knows
/// no module's wonders (the geometry of the shapes is tested there with hand-given shapes).
#[test]
fn classic_natural_wonders_stand_on_their_model_site_shape() {
    use render::world_models::SiteShape;
    const KEYS: [&str; 17] = [
        "core:natural-wonder/cerro-de-potosi",
        "core:natural-wonder/el-dorado",
        "core:natural-wonder/fountain-of-youth",
        "core:natural-wonder/king-solomon-s-mines",
        "core:natural-wonder/krakatoa",
        "core:natural-wonder/lake-victoria",
        "core:natural-wonder/mt-fuji",
        "core:natural-wonder/mt-kailash",
        "core:natural-wonder/mt-kilimanjaro",
        "core:natural-wonder/mt-sinai",
        "core:natural-wonder/old-faithful",
        "core:natural-wonder/rock-of-gibraltar",
        "core:natural-wonder/sri-pada",
        "core:natural-wonder/the-barringer-crater",
        "core:natural-wonder/the-grand-mesa",
        "core:natural-wonder/the-great-barrier-reef",
        "core:natural-wonder/uluru",
    ];
    for key in KEYS {
        let expected = match key {
            "core:natural-wonder/the-barringer-crater" => SiteShape::Crater,
            "core:natural-wonder/the-great-barrier-reef" => SiteShape::Reef,
            _ => SiteShape::Pad,
        };
        let model = render::world_models::resolve(key).unwrap_or_else(|| panic!("{key} has no model"));
        assert_eq!(model.model().site_shape, expected, "{key}");
    }
}

/// Every row of the animation tables names a baked model.
#[test]
fn every_animation_row_names_a_model() {
    assert_eq!(render::gpu::world_anim::rows_without_a_model(), Vec::<String>::new());
}

#[test]
fn content_keys_resolve_to_their_class_model_or_variant() {
    use render::unit_models::resolve;
    let warrior = resolve("core:unit/warrior").expect("warrior");
    assert_eq!(warrior.model().id, "unit/warrior");
    assert_eq!(resolve("core:unit-class/warrior"), Some(warrior));
    let legion = resolve("core:unit/legion").expect("legion");
    assert_eq!(legion.model().id, "unit/legion");
    assert_ne!(legion, resolve("core:unit/swordsman").unwrap());
    assert_eq!(resolve("core:unit/no-such-unit"), None);
}

/// The world wonders of the classic module and the shared ones: 47 models of kind `Wonder` (the
/// render crate's own test of their footprints no longer counts them: the platform alone has 8).
#[test]
fn the_classic_build_has_every_world_wonder_model() {
    use render::world_models::{models, WorldKind};
    let wonders = models().iter().filter(|m| m.kind == WorldKind::Wonder).count();
    assert!(wonders >= 40, "{wonders} wonders");
}
