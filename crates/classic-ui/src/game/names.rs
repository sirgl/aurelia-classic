//! Display names and artwork ids for content keys ("core:unit/warrior").
use classic_rules::ids::{BuildingKindId, ProductionItem, TechId, UnitKindId};
use classic_rules::CompiledRuleset;
use i18n::t;

/// "core:tech/bronze-working" -> "Bronze Working".
pub fn pretty(key: &str) -> String {
    let slug = key.rsplit('/').next().unwrap_or(key);
    let words: Vec<String> = slug
        .split(['-', '_'])
        .filter(|w| !w.is_empty())
        .map(capitalize)
        .collect();
    words.join(" ")
}

fn capitalize(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Artwork id of a content key: the generated card the atlas indexes for it
/// (a unit or wonder card, a standalone painting such as
/// "warrior"), else the key itself, which paints the neutral placeholder.
pub fn art(key: &str) -> String {
    crate::ui::pedia::art::card_for_key(key)
}

/// The name of a row in the active language: the bundle's text for `kind` and the row key,
/// else the ruleset's own name, else one made from the key.
pub fn named(kind: &str, name: &str, key: &str) -> String {
    let fallback = if name.is_empty() { pretty(key) } else { name.to_owned() };
    i18n::content("classic", kind, key, "name", &fallback)
}

/// The era a technology belongs to (`era_name` is the ruleset's text of the era) in the active
/// language.
pub fn era_named(rules: &CompiledRuleset, era_name: &str) -> String {
    match rules.eras.iter().find(|era| era.name == era_name) {
        Some(def) => named("era", &def.name, &def.key),
        None => era_name.to_owned(),
    }
}

/// The name of nation `id` (an index into `rules.civs`) in the active language.
pub fn nation(rules: &CompiledRuleset, id: u8) -> String {
    match rules.civs.get(id as usize) {
        Some(def) => named("nation", &def.name, &def.key),
        None => String::new(),
    }
}

pub fn unit(rules: &CompiledRuleset, kind: UnitKindId) -> String {
    match rules.units.get(kind.0 as usize) {
        Some(def) => named("unit", &def.name, &def.key),
        None => t!("classic.game.names.unit").into(),
    }
}

pub fn building(rules: &CompiledRuleset, kind: BuildingKindId) -> String {
    match rules.buildings.get(kind.0 as usize) {
        Some(def) => named("building", &def.name, &def.key),
        None => t!("classic.game.names.building").into(),
    }
}

pub fn tech(rules: &CompiledRuleset, id: TechId) -> String {
    match rules.technologies.get(id.0 as usize) {
        Some(def) => named("tech", &def.name, &def.key),
        None => t!("classic.game.names.technology").into(),
    }
}

/// The era with this order (`player_era`): "Classical Era".
pub fn era(rules: &CompiledRuleset, index: u8) -> String {
    match rules.eras.iter().find(|era| era.index == index) {
        Some(def) => named("era", &def.name, &def.key),
        None => t!("classic.game.names.era").into(),
    }
}

pub fn item(rules: &CompiledRuleset, item: ProductionItem) -> String {
    match item {
        ProductionItem::Unit(kind) => unit(rules, kind),
        ProductionItem::Building(kind) => building(rules, kind),
        ProductionItem::Project(id) => match rules.module.projects.get(id.0 as usize) {
            Some(def) => named("project", &def.name, &def.key),
            None => t!("classic.game.names.project").into(),
        },
    }
}

pub fn item_cost(rules: &CompiledRuleset, item: ProductionItem) -> u32 {
    let cost = match item {
        ProductionItem::Unit(kind) => rules.units.get(kind.0 as usize).map(|d| d.cost),
        ProductionItem::Building(kind) => rules.buildings.get(kind.0 as usize).map(|d| d.cost),
        ProductionItem::Project(id) => rules.module.projects.get(id.0 as usize).map(|d| d.cost),
    };
    cost.unwrap_or(0)
}

pub fn terrain(terrain: u16) -> &'static str {
    match terrain {
        0 => t!("classic.game.names.terrain-plains"),
        1 => t!("classic.game.names.terrain-grassland"),
        2 => t!("classic.game.names.terrain-hills"),
        3 => t!("classic.game.names.terrain-desert"),
        4 => t!("classic.game.names.terrain-water"),
        5 => t!("classic.game.names.terrain-snow"),
        6 => t!("classic.game.names.terrain-mountains"),
        _ => t!("classic.game.names.terrain-land"),
    }
}

/// Short sentence for a rejected command ("prerequisite missing" -> "Prerequisite missing").
pub fn reason(rules: &CompiledRuleset, error: &classic_rules::command::CommandError) -> String {
    use classic_rules::command::CommandError;
    match error {
        CommandError::Refused(refusal) => super::reasons::refusal(rules, *refusal),
        CommandError::RuleViolation(text) => {
            super::reasons_rules::rule_text(text)
                .map(String::from)
                .or_else(|| super::reasons_rules::rule_text_with_name(rules, text))
                .unwrap_or_else(|| capitalize(text))
        }
        CommandError::Internal(text) => capitalize(text),
        CommandError::UnknownUnit => t!("classic.game.names.error-unknown-unit").into(),
        CommandError::UnknownCity => t!("classic.game.names.error-unknown-city").into(),
        CommandError::UnknownPlayer => t!("classic.game.names.error-unknown-player").into(),
        CommandError::NotOwner => t!("classic.game.names.error-not-owner").into(),
        CommandError::BadDestination => t!("classic.game.names.error-bad-destination").into(),
        CommandError::Impassable => t!("classic.game.names.error-impassable").into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keys_become_readable_names() {
        assert_eq!(pretty("core:tech/bronze-working"), "Bronze Working");
        assert_eq!(pretty("core:building/great_library"), "Great Library");
        assert_eq!(art("core:unit/warrior"), "warrior");
        assert_eq!(art("core:tech/masonry"), "core:tech/masonry");
        assert_eq!(art("core:unit/no-such-unit"), "core:unit/no-such-unit");
    }

    #[test]
    fn a_worded_rule_reason_is_shown_in_the_players_language_and_the_rest_in_english() {
        use classic_rules::command::CommandError;
        let rules = classic_rules::compile_rules(classic_content::classic::classic_rules()).unwrap();
        let worded = reason(&rules, &CommandError::RuleViolation("no coup possible".into()));
        assert_eq!(worded, "A coup is not possible here");
        let ru = i18n::with_language("ru", || reason(&rules, &CommandError::RuleViolation("no coup possible".into())));
        assert_eq!(ru, "\u{41f}\u{435}\u{440}\u{435}\u{432}\u{43e}\u{440}\u{43e}\u{442} \u{437}\u{434}\u{435}\u{441}\u{44c} \u{43d}\u{435}\u{432}\u{43e}\u{437}\u{43c}\u{43e}\u{436}\u{435}\u{43d}"); // Russian: "A coup is impossible here"
        let raw = reason(&rules, &CommandError::RuleViolation("needs a project first".into()));
        assert_eq!(raw, "Needs a project first");
    }

    #[test]
    fn a_reason_with_a_name_in_it_is_worded_around_the_name() {
        use classic_rules::command::CommandError;
        let rules = classic_rules::compile_rules(classic_content::classic::classic_rules()).unwrap();
        let iron = rules.resources.iter().find(|d| d.key.ends_with("iron")).expect("iron");
        let text = CommandError::RuleViolation(format!("needs {}", iron.name));
        assert_eq!(reason(&rules, &text), "Needs Iron");
        let ru = i18n::with_language("ru", || reason(&rules, &text));
        assert_eq!(ru, "\u{41d}\u{443}\u{436}\u{435}\u{43d} \u{440}\u{435}\u{441}\u{443}\u{440}\u{441}: \u{416}\u{435}\u{43b}\u{435}\u{437}\u{43e}"); // Russian: "Resource needed: Iron"
        let stranger = CommandError::RuleViolation("needs Unobtainium".into());
        assert_eq!(reason(&rules, &stranger), "Needs Unobtainium");
    }

    #[test]
    fn names_of_rows_and_terrain_come_from_the_catalogue() {
        let rules = classic_rules::compile_rules(classic_content::classic::classic_rules()).unwrap();
        assert_eq!(terrain(2), "Hills");
        assert_eq!(i18n::with_language("ru", || terrain(2)), "\u{425}\u{43e}\u{43b}\u{43c}\u{44b}"); // Russian: "Hills"
        assert_eq!(unit(&rules, classic_rules::ids::UnitKindId(u16::MAX)), "Unit");
        assert_eq!(i18n::with_language("ru", || unit(&rules, classic_rules::ids::UnitKindId(u16::MAX))), "\u{41e}\u{442}\u{440}\u{44f}\u{434}"); // Russian: "Detachment" (fallback unit name)
    }
}
