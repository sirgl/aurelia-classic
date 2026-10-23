//! Which generated card art exists for a content key. The runtime atlas index
//! lists every card region; a missing index (art not installed) means no card
//! art at all, and pages paint their placeholder instead.
use std::collections::HashSet;
use std::sync::OnceLock;

use classic_rules::rules::content::BuildingKind;

fn atlas_ids() -> &'static HashSet<String> {
    static IDS: OnceLock<HashSet<String>> = OnceLock::new();
    IDS.get_or_init(|| {
        let path = client::shell::assets::path("visual/generated/runtime-atlas-index.tsv");
        let text = client::shell::assets::read_to_string(&path).unwrap_or_default();
        text.lines()
            .skip(1)
            .filter_map(|line| line.split('\t').next())
            .map(str::to_owned)
            .collect()
    })
}

/// Standalone paintings of early items (`<name>-v1.png` beside the atlases).
fn standalone(key: &str) -> Option<&'static str> {
    Some(match key {
        "core:unit/warrior" => "warrior",
        "core:tech/agriculture" => "agriculture",
        "core:tech/pottery" => "pottery",
        "core:tech/mining" => "mining",
        "core:tech/animal-husbandry" => "husbandry",
        "core:tech/writing" => "writing",
        "core:building/granary" => "granary",
        "core:building/watermill" => "watermill",
        _ => return None,
    })
}

fn slug(key: &str) -> &str {
    key.rsplit('/').next().unwrap_or(key)
}

/// The first id among `candidates` with art, else a standalone painting.
fn first_existing(key: &str, candidates: &[String]) -> Option<String> {
    if let Some(name) = standalone(key) {
        return Some(name.to_owned());
    }
    let ids = atlas_ids();
    candidates.iter().find(|c| ids.contains(c.as_str())).cloned()
}

pub fn unit(key: &str) -> Option<String> {
    let art_slug = if slug(key) == "immortal" { "persian-immortal" } else { slug(key) };
    first_existing(key, &[format!("bnw:unit/{}", slug(key)), key.to_owned(), format!("art:unit/{art_slug}")])
}

pub fn building(key: &str, kind: BuildingKind) -> Option<String> {
    let s = slug(key);
    let first = match kind {
        BuildingKind::WorldWonder => format!("bnw:world_wonder/{s}"),
        BuildingKind::NationalWonder => format!("bnw:national_wonder/{s}"),
        _ => format!("bnw:building/{s}"),
    };
    first_existing(key, &[first, format!("bnw:building/{s}"), key.to_owned()])
}

pub fn technology(key: &str) -> Option<String> {
    first_existing(key, &[key.to_owned()])
}

pub fn project(key: &str) -> Option<String> {
    first_existing(key, &[format!("bnw:project/{}", slug(key)), format!("art:unit/{}", slug(key).replace('_', "-"))])
}

pub fn resource(key: &str) -> Option<String> {
    let name = match slug(key) { "gold" => "gold-ore", "hidden_antiquity_site" => "antiquity_site", name => name };
    first_existing(key, &[key.to_owned(), format!("art:resource/{name}"), format!("art:resource/{}", name.replace('_', "-"))])
}

pub fn improvement(key: &str) -> Option<String> {
    first_existing(key, &[key.to_owned(), format!("art:improvement/{}", slug(key))])
}

/// Original illustrative works; the stable instance selects a visual, not a historical attribution.
pub fn great_work(class: classic_rules::world::tourism::GreatWorkClass, instance: usize) -> Option<String> {
    use classic_rules::world::tourism::GreatWorkClass;
    let names: &[&str] = match class {
        GreatWorkClass::Art => &["valley-at-dawn", "sailing-below-the-cliffs", "the-astronomer", "garden-of-two-birds", "the-lyre-player", "bronze-strider"],
        GreatWorkClass::Artifact => &["vessel-of-voyages"],
        GreatWorkClass::Literature => &["atlas-of-distant-stars"],
        GreatWorkClass::Music => &["song-for-a-new-harbour"],
    };
    first_existing("", &[format!("art:great-work/{}", names[instance % names.len()])])
}

pub fn belief(key: &str) -> Option<String> {
    first_existing(key, &[format!("bnw:belief/{}", slug(key))])
}

/// Art id a production option or a building card uses for a content key:
/// the generated card when one exists, else the key (painted as a placeholder).
pub fn existing_card_for_key(key: &str) -> Option<String> {
    let found = if key.starts_with("core:unit/") {
        unit(key)
    } else if key.starts_with("core:building/") {
        let kinds = [BuildingKind::Regular, BuildingKind::WorldWonder, BuildingKind::NationalWonder];
        kinds.into_iter().find_map(|kind| building(key, kind))
    } else if key.starts_with("core:resource/") {
        resource(key)
    } else if key.starts_with("core:improvement/") {
        improvement(key)
    } else if key.starts_with("core:project/") {
        project(key)
    } else {
        first_existing(key, &[key.to_owned()])
    };
    found
}

pub fn card_for_key(key: &str) -> String {
    existing_card_for_key(key).unwrap_or_else(|| key.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_actual_content_categories_resolve_after_asset_integration() {
        let rules = crate::ui::screens::game_rules();
        for def in &rules.units { assert!(unit(&def.key).is_some(), "missing unit {}", def.key); }
        for def in &rules.module.projects { assert!(project(&def.key).is_some(), "missing project {}", def.key); }
        for def in &rules.resources { assert!(resource(&def.key).is_some(), "missing resource {}", def.key); }
        for def in &rules.improvements { assert!(improvement(&def.key).is_some(), "missing improvement {}", def.key); }
        for def in &rules.technologies { assert!(technology(&def.key).is_some(), "missing technology {}", def.key); }
        for def in &rules.buildings { assert!(building(&def.key, def.kind).is_some(), "missing building {}", def.key); }
        assert_eq!(unit("core:unit/immortal").as_deref(), Some("art:unit/persian-immortal"));
        assert_eq!(resource("core:resource/gold").as_deref(), Some("art:resource/gold-ore"));
        assert_eq!(project("core:project/ss_cockpit").as_deref(), Some("art:unit/ss-cockpit"));
        for class in [classic_rules::world::tourism::GreatWorkClass::Art, classic_rules::world::tourism::GreatWorkClass::Artifact, classic_rules::world::tourism::GreatWorkClass::Literature, classic_rules::world::tourism::GreatWorkClass::Music] {
            assert!(great_work(class, 5).is_some());
        }
    }

    #[test]
    fn content_keys_find_their_generated_cards() {
        assert_eq!(unit("core:unit/warrior").as_deref(), Some("warrior"));
        if atlas_ids().is_empty() {
            return; // art not installed: every page shows its placeholder
        }
        assert_eq!(unit("core:unit/great-scientist").as_deref(), Some("bnw:unit/great-scientist"));
        assert_eq!(
            building("core:building/alhambra", BuildingKind::WorldWonder).as_deref(),
            Some("bnw:world_wonder/alhambra")
        );
        assert_eq!(card_for_key("core:building/alhambra"), "bnw:world_wonder/alhambra");
        assert_eq!(card_for_key("core:unit/no-such-unit"), "core:unit/no-such-unit");
        assert_eq!(existing_card_for_key("core:unit/no-such-unit"), None);
        for id in atlas_ids().iter().filter(|id|id.starts_with("bnw:project/")) {
            assert_eq!(card_for_key(&id.replacen("classic:","core:",1)), *id);
        }
    }
}
