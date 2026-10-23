//! Encyclopedia model: one page per entity of the compiled ruleset plus the
//! player-facing game concepts. Pages are built once from data —
//! names, stats, effect text, art and cross-links — and never from a game in
//! progress. The screen (`screen_pedia.rs`) only draws and navigates them.
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use classic_rules::rules::compiled::CompiledRuleset;
use i18n::t;

pub mod art;
mod concepts;
pub mod describe;
mod entries;
mod entries_world;
mod links;

/// The table an entity lives in (a page id is a table and an index).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Table {
    Concept,
    Era,
    Civilization,
    CityState,
    CityStateType,
    Technology,
    Unit,
    Promotion,
    Building,
    Project,
    Policy,
    PolicyBranch,
    Belief,
    Religion,
    Resource,
    Improvement,
    Terrain,
    Feature,
    NaturalWonder,
    GreatPerson,
    Specialist,
    Handicap,
}

/// A Encyclopedia page: an entity of one table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PediaRef {
    pub table: Table,
    pub index: u16,
}

impl PediaRef {
    pub const fn new(table: Table, index: u16) -> Self {
        Self { table, index }
    }

    pub fn unit(kind: classic_rules::ids::UnitKindId) -> Self {
        Self::new(Table::Unit, kind.0)
    }

    pub fn building(kind: classic_rules::ids::BuildingKindId) -> Self {
        Self::new(Table::Building, kind.0)
    }

    pub fn tech(id: classic_rules::ids::TechId) -> Self {
        Self::new(Table::Technology, id.0)
    }

    pub fn policy(id: classic_rules::ids::PolicyId) -> Self {
        Self::new(Table::Policy, id.0)
    }

    pub fn branch(id: classic_rules::rules::content::PolicyBranchId) -> Self {
        Self::new(Table::PolicyBranch, id.0)
    }

    pub fn belief(id: classic_rules::ids::BeliefId) -> Self {
        Self::new(Table::Belief, id.0)
    }

    /// The page of a production item.
    pub fn item(item: classic_rules::ids::ProductionItem) -> Option<Self> {
        match item {
            classic_rules::ids::ProductionItem::Unit(kind) => Some(Self::unit(kind)),
            classic_rules::ids::ProductionItem::Building(kind) => Some(Self::building(kind)),
            classic_rules::ids::ProductionItem::Project(project) => Some(Self::new(Table::Project, project.0)),
        }
    }
}

/// The sections of the Encyclopedia, in the order of its contents list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Category {
    Concepts,
    Civilizations,
    CityStates,
    Technologies,
    Units,
    UniqueUnits,
    Promotions,
    Buildings,
    Wonders,
    Projects,
    Policies,
    Beliefs,
    Religions,
    GreatPeople,
    Specialists,
    Resources,
    Improvements,
    Terrain,
    Features,
    NaturalWonders,
    Eras,
    Difficulty,
}

impl Category {
    pub const ALL: [Category; 22] = [
        Category::Concepts,
        Category::Civilizations,
        Category::CityStates,
        Category::Technologies,
        Category::Units,
        Category::UniqueUnits,
        Category::Promotions,
        Category::Buildings,
        Category::Wonders,
        Category::Projects,
        Category::Policies,
        Category::Beliefs,
        Category::Religions,
        Category::GreatPeople,
        Category::Specialists,
        Category::Resources,
        Category::Improvements,
        Category::Terrain,
        Category::Features,
        Category::NaturalWonders,
        Category::Eras,
        Category::Difficulty,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Category::Concepts => t!("classic.pedia.mod.category-concepts"),
            Category::Civilizations => t!("classic.pedia.mod.category-nations"),
            Category::CityStates => t!("classic.pedia.mod.category-city-states"),
            Category::Technologies => t!("classic.pedia.mod.category-technologies"),
            Category::Units => t!("classic.pedia.mod.category-units"),
            Category::UniqueUnits => t!("classic.pedia.mod.category-unique-units"),
            Category::Promotions => t!("classic.pedia.mod.category-promotions"),
            Category::Buildings => t!("classic.pedia.mod.category-buildings"),
            Category::Wonders => t!("classic.pedia.mod.category-wonders"),
            Category::Projects => t!("classic.pedia.mod.category-projects"),
            Category::Policies => t!("classic.pedia.mod.category-policies"),
            Category::Beliefs => t!("classic.pedia.mod.category-beliefs"),
            Category::Religions => t!("classic.pedia.mod.category-religions"),
            Category::GreatPeople => t!("classic.pedia.mod.category-great-people"),
            Category::Specialists => t!("classic.pedia.mod.category-specialists"),
            Category::Resources => t!("classic.pedia.mod.category-resources"),
            Category::Improvements => t!("classic.pedia.mod.category-improvements"),
            Category::Terrain => t!("classic.pedia.mod.category-terrain"),
            Category::Features => t!("classic.pedia.mod.category-features"),
            Category::NaturalWonders => t!("classic.pedia.mod.category-natural-wonders"),
            Category::Eras => t!("classic.pedia.mod.category-eras"),
            Category::Difficulty => t!("classic.pedia.mod.category-difficulty"),
        }
    }
}

/// A stat row of a page ("Combat strength", "8").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stat {
    pub label: &'static str,
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    /// Game text of the catalog (help text, a summary).
    Text,
    /// An effect the engine applies.
    Effect,
    /// A catalog effect no mechanic evaluates yet.
    Inactive,
    /// A formula or table row of the rules (monospace).
    Formula,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub text: String,
    pub kind: LineKind,
}

impl Line {
    pub fn text(text: impl Into<String>) -> Self {
        Self { text: text.into(), kind: LineKind::Text }
    }
    pub fn effect(text: impl Into<String>) -> Self {
        Self { text: text.into(), kind: LineKind::Effect }
    }
    pub fn inactive(text: impl Into<String>) -> Self {
        Self { text: text.into(), kind: LineKind::Inactive }
    }
    pub fn formula(text: impl Into<String>) -> Self {
        Self { text: text.into(), kind: LineKind::Formula }
    }
}

/// Pages a page links to under one heading ("Requires", "Upgrades to").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkGroup {
    pub label: &'static str,
    pub targets: Vec<PediaRef>,
}

/// Artwork of a page; `None` paints the category placeholder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Art {
    None,
    /// Generated card art (`runtime-atlas-index.tsv` id or standalone painting).
    Card(String),
    /// Civilization leader portrait and emblem by slug.
    Civilization(String),
    /// Policy symbol by catalog id (`tradition/legalism`).
    Policy(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: PediaRef,
    pub category: Category,
    pub key: String,
    pub name: String,
    /// One line under the name ("Medieval era · Melee unit").
    pub subtitle: String,
    pub art: Art,
    pub stats: Vec<Stat>,
    pub text: Vec<Line>,
    pub links: Vec<LinkGroup>,
    /// Lower-case name for search.
    search: String,
}

impl Entry {
    fn new(id: PediaRef, category: Category, key: &str, name: &str) -> Self {
        Self {
            id,
            category,
            key: key.to_owned(),
            name: name.to_owned(),
            subtitle: String::new(),
            art: Art::None,
            stats: Vec::new(),
            text: Vec::new(),
            links: Vec::new(),
            search: name.to_lowercase(),
        }
    }

    /// A page named after rule data: the name in the active language (`i18n::content`), or the
    /// catalog's own when no bundle has one.
    fn named(id: PediaRef, category: Category, key: &str, catalog_name: &str) -> Self {
        let name = content_name(id.table, key, catalog_name);
        Self::new(id, category, key, &name)
    }

    fn stat(&mut self, label: &'static str, value: impl ToString) {
        self.stats.push(Stat { label, value: value.to_string() });
    }

    fn link(&mut self, label: &'static str, mut targets: Vec<PediaRef>) {
        targets.retain(|t| *t != self.id);
        if targets.is_empty() {
            return;
        }
        match self.links.iter_mut().find(|g| g.label == label) {
            Some(group) => {
                for t in targets {
                    if !group.targets.contains(&t) {
                        group.targets.push(t);
                    }
                }
            }
            None => self.links.push(LinkGroup { label, targets }),
        }
    }

    /// Every page this page links to.
    pub fn targets(&self) -> impl Iterator<Item = PediaRef> + '_ {
        self.links.iter().flat_map(|g| g.targets.iter().copied())
    }
}

/// All pages, indexed by id and by content key.
#[derive(Debug, Default)]
pub struct Pedia {
    entries: Vec<Entry>,
    by_id: HashMap<PediaRef, usize>,
    by_key: HashMap<String, PediaRef>,
}

impl Pedia {
    pub fn build(rules: &CompiledRuleset) -> Self {
        let mut entries = concepts::entries();
        entries.extend(entries::all(rules));
        entries.extend(entries_world::all(rules));
        links::add_reverse_links(&mut entries);
        entries.sort_by(|a, b| (a.category, &a.name).cmp(&(b.category, &b.name)));
        let by_id = entries.iter().enumerate().map(|(i, e)| (e.id, i)).collect();
        let by_key = entries.iter().map(|e| (e.key.clone(), e.id)).collect();
        Self { entries, by_id, by_key }
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn entry(&self, id: PediaRef) -> Option<&Entry> {
        self.by_id.get(&id).map(|i| &self.entries[*i])
    }

    /// The page of a content key ("core:unit/warrior").
    pub fn by_key(&self, key: &str) -> Option<PediaRef> {
        self.by_key.get(key).copied()
    }

    pub fn category(&self, category: Category) -> impl Iterator<Item = &Entry> {
        self.entries.iter().filter(move |e| e.category == category)
    }

    /// Pages whose name matches: names starting with the query first, then
    /// names with a word starting with it, then names containing it.
    pub fn search(&self, query: &str) -> Vec<PediaRef> {
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return Vec::new();
        }
        let mut found: Vec<(u8, &Entry)> = self
            .entries
            .iter()
            .filter_map(|e| match_rank(&e.search, &query).map(|rank| (rank, e)))
            .collect();
        found.sort_by(|a, b| (a.0, &a.1.name).cmp(&(b.0, &b.1.name)));
        found.into_iter().map(|(_, e)| e.id).collect()
    }
}

fn match_rank(name: &str, query: &str) -> Option<u8> {
    if name.starts_with(query) {
        return Some(0);
    }
    if name.split([' ', '-', '\'']).any(|word| word.starts_with(query)) {
        return Some(1);
    }
    name.contains(query).then_some(2)
}

/// The Encyclopedia of the game's ruleset, built once per language: the pages hold the words of
/// the language that was active when they were built, so a changed language (or catalogue)
/// builds them again.
pub fn game_pedia() -> Arc<Pedia> {
    type Key = (String, Option<String>);
    static PEDIA: Mutex<Vec<(Key, u64, Arc<Pedia>)>> = Mutex::new(Vec::new());
    let (language, scope, generation) = i18n::context();
    let key: Key = (language, scope.map(str::to_owned));
    {
        let cache = PEDIA.lock().unwrap();
        if let Some((_, _, pedia)) = cache.iter().find(|(k, g, _)| *k == key && *g == generation) {
            return pedia.clone();
        }
    }
    let pedia = Arc::new(Pedia::build(&crate::ui::screens::game_rules()));
    let mut cache = PEDIA.lock().unwrap();
    cache.retain(|(k, _, _)| *k != key);
    cache.push((key, generation, pedia.clone()));
    pedia
}

/// Rows of a content table (None for tables without rows in the ruleset).
pub fn table_len(rules: &CompiledRuleset, table: Table) -> Option<usize> {
    Some(match table {
        Table::Concept => concepts::count(),
        Table::Era => rules.eras.len(),
        Table::Civilization => rules.civs.len(),
        Table::CityState => rules.module.city_states.len(),
        Table::CityStateType => rules.module.city_state_types.len(),
        Table::Technology => rules.technologies.len(),
        Table::Unit => rules.units.len(),
        Table::Promotion => rules.promotions.len(),
        Table::Building => rules.buildings.len(),
        Table::Project => rules.module.projects.len(),
        Table::Policy => rules.module.policies.len(),
        Table::PolicyBranch => rules.module.policy_branches.len(),
        Table::Belief => rules.module.beliefs.len(),
        Table::Religion => rules.module.religions.len(),
        Table::Resource => rules.resources.len(),
        Table::Improvement => rules.improvements.len(),
        Table::Terrain => rules.terrains.len(),
        Table::Feature => rules.features.len(),
        Table::NaturalWonder => rules.natural_wonders.len(),
        Table::GreatPerson => rules.module.great_people.len(),
        Table::Specialist => rules.specialists.len(),
        Table::Handicap => rules.handicaps.len(),
    })
}

/// The kind of a table in the content bundle (`classic.<kind>.<id>.name`).
fn content_kind(table: Table) -> Option<&'static str> {
    Some(match table {
        Table::Concept => return None,
        Table::Era => "era",
        Table::Civilization => "nation",
        Table::CityState => "city-state",
        Table::CityStateType => "city-state-type",
        Table::Technology => "tech",
        Table::Unit => "unit",
        Table::Promotion => "promotion",
        Table::Building => "building",
        Table::Project => "project",
        Table::Policy | Table::PolicyBranch => "policy",
        Table::Belief => "belief",
        Table::Religion => "religion",
        Table::Resource => "resource",
        Table::Improvement => "improvement",
        Table::Terrain => "terrain",
        Table::Feature => "feature",
        Table::NaturalWonder => "natural-wonder",
        Table::GreatPerson => "great-person",
        Table::Specialist => "specialist",
        Table::Handicap => "handicap",
    })
}

/// The name of a row of `table` in the active language: the content bundle's text, or the
/// catalog's own `catalog_name` when the bundle has none.
fn content_name(table: Table, key: &str, catalog_name: &str) -> String {
    match content_kind(table) {
        Some(kind) => i18n::content("classic", kind, key, "name", catalog_name),
        None => catalog_name.to_owned(),
    }
}

/// Display name of an entity of the ruleset.
pub fn entity_name(rules: &CompiledRuleset, id: PediaRef) -> Option<String> {
    let i = id.index as usize;
    let (key, name) = match id.table {
        Table::Concept => return concepts::title(i).map(str::to_owned),
        Table::Era => rules.eras.get(i).map(|d| (&d.key, &d.name))?,
        Table::Civilization => rules.civs.get(i).map(|d| (&d.key, &d.name))?,
        Table::CityState => rules.module.city_states.get(i).map(|d| (&d.key, &d.name))?,
        Table::CityStateType => rules.module.city_state_types.get(i).map(|d| (&d.key, &d.name))?,
        Table::Technology => rules.technologies.get(i).map(|d| (&d.key, &d.name))?,
        Table::Unit => rules.units.get(i).map(|d| (&d.key, &d.name))?,
        Table::Promotion => rules.promotions.get(i).map(|d| (&d.key, &d.name))?,
        Table::Building => rules.buildings.get(i).map(|d| (&d.key, &d.name))?,
        Table::Project => rules.module.projects.get(i).map(|d| (&d.key, &d.name))?,
        Table::Policy => rules.module.policies.get(i).map(|d| (&d.key, &d.name))?,
        Table::PolicyBranch => rules.module.policy_branches.get(i).map(|d| (&d.key, &d.name))?,
        Table::Belief => rules.module.beliefs.get(i).map(|d| (&d.key, &d.name))?,
        Table::Religion => rules.module.religions.get(i).map(|d| (&d.key, &d.name))?,
        Table::Resource => rules.resources.get(i).map(|d| (&d.key, &d.name))?,
        Table::Improvement => rules.improvements.get(i).map(|d| (&d.key, &d.name))?,
        Table::Terrain => rules.terrains.get(i).map(|d| (&d.key, &d.name))?,
        Table::Feature => rules.features.get(i).map(|d| (&d.key, &d.name))?,
        Table::NaturalWonder => rules.natural_wonders.get(i).map(|d| (&d.key, &d.name))?,
        Table::GreatPerson => {
            // A great person is named after the unit it is.
            let person = rules.module.great_people.get(i)?;
            let unit = rules.units.get(person.unit.0 as usize)?;
            let shown = content_name(Table::Unit, &unit.key, &unit.name);
            return Some(if shown.is_empty() { describe::pretty(&person.key) } else { shown });
        }
        Table::Specialist => rules.specialists.get(i).map(|d| (&d.key, &d.name))?,
        Table::Handicap => rules.handicaps.get(i).map(|d| (&d.key, &d.name))?,
    };
    let shown = content_name(id.table, key, name);
    if shown.is_empty() {
        return Some(describe::pretty(key));
    }
    Some(shown)
}

/// Content key of an entity (concepts use `concept:<n>`).
pub fn key_of(rules: &CompiledRuleset, id: PediaRef) -> Option<String> {
    let i = id.index as usize;
    Some(match id.table {
        Table::Concept => format!("concept:{i}"),
        Table::Era => rules.eras.get(i)?.key.clone(),
        Table::Civilization => rules.civs.get(i)?.key.clone(),
        Table::CityState => rules.module.city_states.get(i)?.key.clone(),
        Table::CityStateType => rules.module.city_state_types.get(i)?.key.clone(),
        Table::Technology => rules.technologies.get(i)?.key.clone(),
        Table::Unit => rules.units.get(i)?.key.clone(),
        Table::Promotion => rules.promotions.get(i)?.key.clone(),
        Table::Building => rules.buildings.get(i)?.key.clone(),
        Table::Project => rules.module.projects.get(i)?.key.clone(),
        Table::Policy => rules.module.policies.get(i)?.key.clone(),
        Table::PolicyBranch => rules.module.policy_branches.get(i)?.key.clone(),
        Table::Belief => rules.module.beliefs.get(i)?.key.clone(),
        Table::Religion => rules.module.religions.get(i)?.key.clone(),
        Table::Resource => rules.resources.get(i)?.key.clone(),
        Table::Improvement => rules.improvements.get(i)?.key.clone(),
        Table::Terrain => rules.terrains.get(i)?.key.clone(),
        Table::Feature => rules.features.get(i)?.key.clone(),
        Table::NaturalWonder => rules.natural_wonders.get(i)?.key.clone(),
        Table::GreatPerson => rules.module.great_people.get(i)?.key.clone(),
        Table::Specialist => rules.specialists.get(i)?.key.clone(),
        Table::Handicap => rules.handicaps.get(i)?.key.clone(),
    })
}

#[cfg(test)]
mod tests;
