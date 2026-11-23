//! The parity source of the MLX economy (`nn/aurelia_nn/tacsim/econ.py`): the whole game's map and
//! players at every round's start, appended as JSON lines to the file `AURELIA_ECON_DUMP` names.
//! A MEASUREMENT tool: it reads the world only; the result hash of a match does not move.
//!
//! Line kinds (field `t`):
//! - `tables` (once per game, first): the yield tables of the rules by id (terrain kinds, features,
//!   resources with their revealing technology and improved yields, improvements with their fresh
//!   water technology yields, mountain and twin yields, natural wonders) and the static map (terrain
//!   kind per tile, mountain, lake, river, fresh-water features and wonders of the neighbourhood are
//!   derived in MLX from the dynamic planes);
//! - `round`: per tile the feature, resource, improvement, pillaged flag, owner, city; per player the
//!   technologies known; and Rust's answer: `tile_yields_for` of every tile for its owner (the first
//!   major on unowned land), six channels.

use std::io::Write;
use std::sync::{Mutex, OnceLock};

use classic_rules::classic::Classic;
use classic_rules::ids::{PlayerId, TileId};
use classic_rules::{CompiledRuleset, GameState};
use kernel::rules::content::{FreshWater, Yields};
use serde_json::{json, Value};

use super::modules::{ClassicBench, MatchRecorder};

pub const ENV: &str = "AURELIA_ECON_DUMP";

fn sink() -> Option<&'static Mutex<std::fs::File>> {
    static SINK: OnceLock<Option<Mutex<std::fs::File>>> = OnceLock::new();
    SINK.get_or_init(|| {
        let path = std::env::var(ENV).ok().filter(|p| !p.is_empty())?;
        let file = std::fs::OpenOptions::new().create(true).append(true).open(path).ok()?;
        Some(Mutex::new(file))
    })
    .as_ref()
}

pub struct EconDump {
    name: String,
    seed: u64,
    started: bool,
    /// `AURELIA_ECON_TURNS`: the end-turn lines too (`bench::econ_turns`).
    turns: bool,
    /// The dynamic planes as the game's last line left them (the turn lines carry the diff).
    last: Option<Vec<[i32; 7]>>,
    /// The ending player, its round and the world before its end of turn.
    pending: Option<(PlayerId, u32, Value)>,
}

impl EconDump {
    pub fn new(name: &str, seed: u64) -> Option<Self> {
        sink()?;
        Some(Self { name: name.to_owned(), seed, started: false, turns: super::econ_turns::enabled(), last: None, pending: None })
    }

    fn diff(&mut self, game: &GameState) -> Value {
        let now = super::econ_turns::tile_planes(game.view().auth);
        let d = super::econ_turns::tile_diff(self.last.as_deref(), &now);
        self.last = Some(now);
        json!(d)
    }

    fn write(&self, v: Value) {
        if let Some(s) = sink() {
            let _ = writeln!(s.lock().unwrap(), "{v}");
        }
    }
}

fn y6(y: &Yields) -> Value {
    json!([y.food, y.production, y.gold, y.science, y.culture, y.module])
}

fn kind_index(k: &kernel::rules::effects::YieldKind<Classic>) -> i32 {
    use kernel::rules::effects::YieldKind as K;
    match k {
        K::Food => 0,
        K::Production => 1,
        K::Gold => 2,
        K::Science => 3,
        K::Culture => 4,
        _ => 5,
    }
}

fn tables(game: &GameState, rules: &CompiledRuleset) -> Value {
    let auth = game.view().auth;
    let map = auth.map();
    let n = map.tile_count() as u32;
    let dims = map.dimensions;
    let terrain_kinds: Vec<Value> = rules.terrains.iter().map(|d| json!({"name": d.name, "yields": y6(&d.yields)})).collect();
    let features: Vec<Value> = rules.features.iter().map(|d| json!({"name": d.name, "yields": y6(&d.yields), "not_additive": d.yield_not_additive, "fresh_water": d.fresh_water})).collect();
    let resources: Vec<Value> = rules
        .resources
        .iter()
        .map(|d| json!({"name": d.name, "yields": y6(&d.yields), "tech": d.required_tech.map_or(-1, |t| i32::from(t.0)),
                        "improved": d.improved_yields.iter().map(|(i, y)| json!([i.0, y6(y)])).collect::<Vec<Value>>()}))
        .collect();
    let improvements: Vec<Value> = rules
        .improvements
        .iter()
        .map(|d| {
            let fw: Vec<Value> = d
                .fresh_water_yields
                .iter()
                .map(|y| json!({"tech": y.tech.map_or(-1, |t| i32::from(t.0)), "kind": kind_index(&y.yield_kind), "amount": y.amount,
                                "fresh": match y.fresh_water { FreshWater::Any => 0, FreshWater::Required => 1, FreshWater::Absent => 2 }}))
                .collect();
            json!({"name": d.name, "yields": y6(&d.yields), "fresh_water": fw, "mountain": y6(&d.adjacent_mountain_yields), "twin_culture": d.adjacent_same_culture})
        })
        .collect();
    let wonders: Vec<Value> = rules.natural_wonders.iter().map(|d| json!({"yields": y6(&d.yields), "fresh_water": d.fresh_water})).collect();
    let tiles = (0..n).map(TileId);
    let kind: Vec<i32> = tiles.clone().map(|t| map.terrain(t).and_then(|c| rules.terrain_kind_of(c)).map_or(-1, |k| i32::from(k.0))).collect();
    let mountain: Vec<bool> = tiles.clone().map(|t| map.terrain(t).is_some_and(|c| rules.terrain_kinds.is_mountains(c.0))).collect();
    let water: Vec<bool> = tiles.clone().map(|t| classic_rules::navigation::terrain::is_water(map, t)).collect();
    let lake: Vec<bool> = tiles.clone().map(|t| map.lake(t)).collect();
    let river: Vec<bool> = tiles.clone().map(|t| map.river(t)).collect();
    let wonder: Vec<i32> = tiles.clone().map(|t| map.natural_wonder(t).map_or(-1, |w| i32::from(w.0))).collect();
    let players = auth.players();
    let majors: Vec<u8> = (0..players.count()).map(|p| p as u8).filter(|p| players.is_major(PlayerId(*p))).collect();
    json!({"t": "tables", "game": self_name(game), "map_w": dims.width, "map_h": dims.height, "players": players.count(), "majors": majors,
           "techs": rules.technologies.len(), "terrain_kinds": terrain_kinds, "features": features, "resources": resources,
           "improvements": improvements, "wonders": wonders,
           "tiles": {"kind": kind, "mountain": mountain, "water": water, "lake": lake, "river": river, "wonder": wonder}})
}

fn self_name(_game: &GameState) -> Value {
    Value::Null
}

fn round_line(game: &GameState, rules: &CompiledRuleset) -> Value {
    let auth = game.view().auth;
    let map = auth.map();
    let n = map.tile_count() as u32;
    let players = auth.players();
    let first_major = (0..players.count()).map(|p| PlayerId(p as u8)).find(|p| players.is_major(*p)).unwrap_or(PlayerId(0));
    let tiles = (0..n).map(TileId);
    let feature: Vec<i32> = tiles.clone().map(|t| map.feature(t).flatten().map_or(-1, |f| i32::from(f.0))).collect();
    let resource: Vec<i32> = tiles.clone().map(|t| map.resource(t).flatten().map_or(-1, |r| i32::from(r.0))).collect();
    let improvement: Vec<i32> = tiles.clone().map(|t| map.improvement(t).flatten().map_or(-1, |i| i32::from(i.0))).collect();
    let pillaged: Vec<bool> = tiles.clone().map(|t| map.pillaged(t)).collect();
    let owner: Vec<i32> = tiles.clone().map(|t| map.owner(t).flatten().map_or(-1, |p| i32::from(p.0))).collect();
    let city: Vec<bool> = tiles.clone().map(|t| auth.indexes().city_on_tile(t).is_some()).collect();
    let techs: Vec<Value> = (0..players.count())
        .map(|p| {
            let known = players.techs_of(PlayerId(p as u8));
            let bits: String = (0..rules.technologies.len()).map(|i| if known.iter().any(|t| t.0 as usize == i) { '1' } else { '0' }).collect();
            json!(bits)
        })
        .collect();
    let yields: Vec<Value> = tiles.clone().map(|t| y6(&kernel::derived::tile_yields::tile_yields_for(auth, rules, t, first_major))).collect();
    json!({"t": "round", "round": auth.turn().round, "viewer": first_major.0, "feature": feature, "resource": resource, "improvement": improvement,
           "pillaged": pillaged, "owner": owner, "city": city, "techs": techs, "yields": yields})
}

impl MatchRecorder<ClassicBench> for EconDump {
    fn round_start(&mut self, game: &GameState, rules: &CompiledRuleset) {
        if !self.started {
            let mut t = tables(game, rules);
            t["game"] = json!(self.name);
            t["seed"] = json!(self.seed);
            self.write(t);
            if self.turns {
                let mut e = super::econ_turns::rules_line(game, rules);
                e["game"] = json!(self.name);
                self.write(e);
            }
            self.started = true;
        }
        let mut r = round_line(game, rules);
        r["game"] = json!(self.name);
        if self.turns {
            let planes = super::econ_turns::tile_planes(game.view().auth);
            r["ocity"] = json!(planes.iter().map(|p| p[5]).collect::<Vec<_>>());
            r["cityh"] = json!(planes.iter().map(|p| p[6]).collect::<Vec<_>>());
            self.last = Some(planes);
        }
        self.write(r);
    }

    fn before_end(&mut self, game: &GameState, rules: &CompiledRuleset, player: PlayerId) {
        if !self.turns {
            return;
        }
        let auth = game.view().auth;
        let tiles = self.diff(game);
        let state = super::econ_turns::player_state(auth, rules, player, true);
        self.pending = Some((player, auth.turn().round, json!({"tiles": tiles, "state": state})));
    }

    fn after_end(&mut self, game: &GameState, rules: &CompiledRuleset, player: PlayerId) {
        let auth = game.view().auth;
        let passed = auth.active_player() != player || game.is_over();
        let Some((p, round, _)) = self.pending.as_ref() else { return };
        if *p != player || !passed {
            return;
        }
        let round = *round;
        let (_, _, pre) = self.pending.take().unwrap();
        let tiles = self.diff(game);
        let state = super::econ_turns::player_state(auth, rules, player, false);
        self.write(json!({"t": "turn", "game": self.name, "round": round, "player": player.0, "pre": pre,
                          "post": {"tiles": tiles, "state": state, "round": auth.turn().round, "over": game.is_over()}}));
    }

    fn turn_start(&mut self, _game: &GameState, _rules: &CompiledRuleset, _player: PlayerId, _states: &kernel::ai::state::AiStates) {}

    fn finish(self: Box<Self>, _game: &GameState, _rules: &CompiledRuleset, _minds: &[&<ClassicBench as super::modules::BenchRules>::Mind]) {}
}
