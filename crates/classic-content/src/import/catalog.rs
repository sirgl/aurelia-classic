//! Reading the classic/docs/classic JSON catalogs: small typed accessors over
//! `serde_json::Value` that fail loudly on a missing or mistyped field.

use std::path::Path;

use serde_json::Value;

pub struct Catalogs {
    pub techs: Value,
    pub units: Value,
    pub unique_units: Value,
    pub buildings: Value,
    pub unique_buildings: Value,
    pub wonders: Value,
    pub policies: Value,
    pub beliefs: Value,
    pub resources: Value,
    pub improvements: Value,
    pub terrain: Value,
    pub features: Value,
    pub natural_wonders: Value,
    pub nations: Value,
    pub city_states: Value,
    pub promotions: Value,
    pub great_people: Value,
    pub eras: Value,
}

fn load(dir: &Path, name: &str) -> Result<Value, String> {
    let path = dir.join(name);
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

impl Catalogs {
    pub fn load(dir: &Path) -> Result<Self, String> {
        Ok(Self {
            techs: load(dir, "techs.json")?,
            units: load(dir, "units.json")?,
            unique_units: load(dir, "unique_units.json")?,
            buildings: load(dir, "buildings.json")?,
            unique_buildings: load(dir, "unique_buildings.json")?,
            wonders: load(dir, "wonders.json")?,
            policies: load(dir, "policies.json")?,
            beliefs: load(dir, "beliefs.json")?,
            resources: load(dir, "resources.json")?,
            improvements: load(dir, "improvements.json")?,
            terrain: load(dir, "terrain.json")?,
            features: load(dir, "features.json")?,
            natural_wonders: load(dir, "natural_wonders.json")?,
            nations: load(dir, "nations.json")?,
            city_states: load(dir, "city_states.json")?,
            promotions: load(dir, "promotions.json")?,
            great_people: load(dir, "great_people.json")?,
            eras: load(dir, "eras.json")?,
        })
    }
}

/// Records of a catalog list, sorted by `id` (stable output).
pub fn items<'a>(catalog: &'a Value, list: &str) -> Vec<&'a Value> {
    let mut out: Vec<&Value> = catalog[list]
        .as_array()
        .map(|a| a.iter().collect())
        .unwrap_or_default();
    out.sort_by(|a, b| id(a).cmp(id(b)));
    out
}

pub fn id(v: &Value) -> &str {
    v["id"]
        .as_str()
        .unwrap_or_else(|| panic!("record without id: {v}"))
}

pub fn text<'a>(v: &'a Value, field: &str) -> &'a str {
    v[field].as_str().unwrap_or("")
}

pub fn opt<'a>(v: &'a Value, field: &str) -> Option<&'a str> {
    v[field].as_str()
}

pub fn int(v: &Value, field: &str) -> i64 {
    match &v[field] {
        Value::Number(n) => n.as_f64().map(|f| f as i64).unwrap_or(0),
        _ => 0,
    }
}

pub fn flag(v: &Value, field: &str) -> bool {
    v[field].as_bool().unwrap_or(false)
}

pub fn strings(v: &Value, field: &str) -> Vec<String> {
    match &v[field] {
        Value::Array(items) => items
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect(),
        Value::String(s) => vec![s.clone()],
        _ => Vec::new(),
    }
}

/// Yields map `{food: 1, gold: 2}` as RON `(food: 1, gold: 2)`; None when
/// empty or absent.
pub fn yields(v: &Value) -> Option<String> {
    let map = v.as_object()?;
    let mut parts = Vec::new();
    for name in ["food", "production", "gold", "science", "culture", "faith"] {
        if let Some(n) = map.get(name).and_then(|n| n.as_i64()) {
            if n != 0 {
                parts.push(format!("{name}: {n}"));
            }
        }
    }
    for key in map.keys() {
        let known = ["food", "production", "gold", "science", "culture", "faith"];
        assert!(known.contains(&key.as_str()), "unknown yield {key} in {v}");
    }
    if parts.is_empty() {
        return None;
    }
    Some(format!("({})", parts.join(", ")))
}

/// `[("core:resource/oil", 1)]` from `[{resource, amount}]`.
pub fn key_counts(v: &Value, key_field: &str, count_field: &str) -> Vec<String> {
    let mut out = Vec::new();
    for item in v.as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
        let key = item[key_field]
            .as_str()
            .unwrap_or_else(|| panic!("no {key_field} in {item}"));
        let count = item[count_field].as_i64().unwrap_or(1);
        out.push(format!("({}, {count})", crate::import::ron_out::quote(key)));
    }
    out
}

/// The slug of a key (`core:unit/great-writer` → `great-writer`).
pub fn slug(key: &str) -> &str {
    key.rsplit('/').next().unwrap_or(key)
}
