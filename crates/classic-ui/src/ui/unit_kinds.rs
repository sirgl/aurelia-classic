//! Unit kinds of the running ruleset, resolved by stable content key.
//!
//! `UnitKindId` is an index into the compiled ruleset; its numbering follows sorted keys
//! and changes whenever content changes. The UI therefore never matches on the number:
//! it reads the key, the rules flags and the model category through this table.
use classic_rules::CompiledRuleset;
use render::unit_models::{UnitModelBook, UnitModelId};

/// What the showcase order panel and the map badges need to know about a kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitRole {
    /// Founds cities (rules: `founder`).
    Founder,
    /// Builds improvements (rules: `builder`).
    Builder,
    /// Never fights (model category: civilian, religious, great person, spaceship part).
    Civilian,
    Military,
}

impl UnitRole {
    pub fn is_civilian(self) -> bool {
        !matches!(self, Self::Military)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitKind {
    pub key: String,
    /// The English name made from the key; show `display_name()`.
    pub name: String,
    pub role: UnitRole,
}

impl UnitKind {
    /// The name in the active language (the bundle's text for the key, else `name`).
    pub fn display_name(&self) -> String {
        i18n::content("classic", "unit", &self.key, "name", &self.name)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UnitKinds {
    kinds: Vec<UnitKind>,
}

impl UnitKinds {
    pub fn from_rules(rules: &CompiledRuleset) -> Self {
        let models = UnitModelBook::from_rules(rules);
        let kinds = rules
            .units
            .iter()
            .enumerate()
            .map(|(kind, unit)| {
                let model: UnitModelId = models.model_for_kind(kind as u16);
                let role = if unit.founder {
                    UnitRole::Founder
                } else if unit.builder {
                    UnitRole::Builder
                } else if model.model().category.is_civilian() {
                    UnitRole::Civilian
                } else {
                    UnitRole::Military
                };
                UnitKind {
                    key: unit.key.clone(),
                    name: crate::game::names::pretty(&unit.key),
                    role,
                }
            })
            .collect();
        Self { kinds }
    }

    /// Names and symbols of the kinds for the shell's map badges.
    pub fn labels(&self) -> Vec<client::shell::module::KindLabel> {
        let label = |k: &UnitKind| client::shell::module::KindLabel { name: k.display_name(), civilian: k.role.is_civilian() };
        self.kinds.iter().map(label).collect()
    }

    pub fn get(&self, kind: u16) -> Option<&UnitKind> {
        self.kinds.get(kind as usize)
    }

    /// Kind index of a content key in this ruleset.
    pub fn kind_of(&self, key: &str) -> Option<u16> {
        self.kinds
            .iter()
            .position(|k| k.key == key)
            .map(|i| i as u16)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_and_names_come_from_keys_and_rules_not_indices() {
        let kinds = UnitKinds::from_rules(&crate::ui::screens::game_rules());
        let role = |key| kinds.get(kinds.kind_of(key).expect(key)).unwrap().role;
        assert_eq!(role("core:unit/settler"), UnitRole::Founder);
        assert_eq!(role("core:unit/worker"), UnitRole::Builder);
        for key in [
            "core:unit/warrior",
            "core:unit/archer",
            "core:unit/horseman",
            "core:unit/swordsman",
        ] {
            assert_eq!(role(key), UnitRole::Military, "{key}");
        }
        let warrior = kinds.kind_of("core:unit/warrior").unwrap();
        assert_eq!(kinds.get(warrior).unwrap().name, "Warrior");
        assert_eq!(kinds.get(u16::MAX), None);
    }
}
