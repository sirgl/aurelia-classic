//! Frames of a recorded game for the HTML replay player (`ai-replay-frames`,
//! `crates/qa/replay/player.html`): a log is replayed command by command
//! through the kernel (round hashes checked, `export::replay_with`), with a
//! training ground's edits and gift made again where they were made at play
//! (`grounds::Edit`, `selfplay::Gift`), and the world is written as frames —
//! after every player-turn (a battle) or every round (a whole game).
//!
//! A frame holds the tiles that changed (owner, improvement, route, feature),
//! every unit and city in the window, the majors' standings, and the events
//! since the frame before, found as the difference of the world before and
//! after each command: attacks with the hit points they took, units lost and
//! seized, cities founded, captured and razed, wars and peace, units new to a
//! player. Every name comes from the rules (in Russian where the content
//! bundle has it, `assets/lang/ru/content-classic.ftl`); nothing names content.
//!
//! A battle of a training ground (`.e2.json` with `ground.battle`) is shown
//! from the scene's set-up on, in a window of hexes around its target city
//! wide enough for both armies; a whole game shows the whole map.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use classic_rules::world::player::ClassicKind;
use classic_rules::{Command, CompiledRuleset, GameState};
use kernel::ids::{PlayerId, TileId};
use kernel::world::player::PlayerKind;
use serde_json::{json, Value};

use crate::bench::modules::{standing, ClassicBench};
use crate::export::{self, LogSpec};

mod empire;
mod sites;

/// How a log is turned into frames.
#[derive(Debug, Clone, Copy, Default)]
pub struct Options {
    /// A battle's whole game, not just the battle.
    pub whole: bool,
    /// Frames after every player-turn (`Some(true)`) or every round
    /// (`Some(false)`); `None`: per player-turn for a battle, per round for a
    /// whole game.
    pub per_turn: Option<bool>,
}

/// The renderer's terrain colours by map code (`render::scene::style::default_style`,
/// platform): the player paints the map as the game does. A code beyond it is
/// coloured from what the rules say the terrain is.
const RENDER_TERRAIN_RGB: [[f32; 3]; 8] = [
    [0.66, 0.70, 0.36],
    [0.55, 0.67, 0.33],
    [0.59, 0.64, 0.39],
    [0.80, 0.70, 0.43],
    [0.08, 0.46, 0.66],
    [0.85, 0.87, 0.90],
    [0.46, 0.49, 0.56],
    [0.58, 0.60, 0.48],
];

/// Russian names of the rules' content (`classic.<kind>.<slug>.name`), the
/// rules' own name when the bundle has none.
struct Names(HashMap<String, String>);

impl Names {
    fn load() -> Self {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/lang/ru/content-classic.ftl");
        // The city screen's words too: the names of the processes (`classic.game.city.process-*`).
        let city = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/lang/ru/classic-game-city.ftl");
        let mut map = HashMap::new();
        for text in [path, city].iter().filter_map(|p| std::fs::read_to_string(p).ok()) {
            for line in text.lines() {
                if let Some((k, v)) = line.split_once(" = ") {
                    map.insert(k.trim().to_owned(), v.trim().to_owned());
                }
            }
        }
        Names(map)
    }

    /// `i18n::content_slug`'s rule: no namespace, no table, `_` and `/` as `-` and `.`.
    fn get(&self, kind: &str, key: &str, fallback: &str) -> String {
        let id = key.split_once(':').map_or(key, |(_, rest)| rest);
        let id = match id.split_once('/') {
            Some((_, rest)) if !rest.is_empty() => rest,
            _ => id,
        };
        let slug = id.to_ascii_lowercase().replace('/', ".").replace(['_', ':', ' '], "-");
        self.0.get(&format!("classic.{kind}.{slug}.name")).cloned().unwrap_or_else(|| fallback.to_owned())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Unit {
    id: u64,
    owner: u8,
    kind: u16,
    hp: u16,
    tile: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct City {
    id: u64,
    owner: u8,
    tile: u32,
    damage: u16,
    pop: u32,
    name: String,
}

/// The world as the diff of a command reads it.
#[derive(Debug, Clone, Default)]
struct Snap {
    units: Vec<Unit>,
    cities: Vec<City>,
    wars: BTreeSet<(u8, u8)>,
}

fn snap(game: &GameState) -> Snap {
    let auth = game.view().auth;
    let mut units: Vec<Unit> = auth.units().iter_cores().map(|u| Unit { id: u.id.0, owner: u.owner.0, kind: u.kind.0, hp: u.hp, tile: u.tile.0 }).collect();
    units.sort_by_key(|u| u.id);
    let mut cities: Vec<City> =
        auth.cities().iter().map(|(_, c)| City { id: c.id.0, owner: c.owner.0, tile: c.tile.0, damage: c.damage, pop: c.population, name: c.name.clone() }).collect();
    cities.sort_by_key(|c| c.id);
    Snap { units, cities, wars: wars(game) }
}

/// The pairs at war, barbarians left out (always at war).
fn wars(game: &GameState) -> BTreeSet<(u8, u8)> {
    let auth = game.view().auth;
    let players = auth.players();
    let n = players.count() as u8;
    let counted = |p: u8| !matches!(players.kind_of(PlayerId(p)), PlayerKind::Module(ClassicKind::Barbarian));
    let mut out = BTreeSet::new();
    for a in 0..n {
        for b in a + 1..n {
            if counted(a) && counted(b) && auth.diplomacy().at_war(PlayerId(a), PlayerId(b)) {
                out.insert((a, b));
            }
        }
    }
    out
}

fn unit_ref(u: &Unit) -> Value {
    json!({ "id": u.id, "p": u.owner, "k": u.kind, "tile": u.tile })
}

fn city_ref(c: &City) -> Value {
    json!({ "id": c.id, "p": c.owner, "tile": c.tile, "name": c.name })
}

/// What a command is about, read before it runs (its handles die with their units).
#[derive(Debug, Clone, Default)]
struct Act {
    name: String,
    player: u8,
    round: u32,
    attack: bool,
    actor: Option<Unit>,
    city: Option<City>,
    target: Option<u32>,
    before: Snap,
}

fn variant(command: &Command) -> String {
    let text = format!("{command:?}");
    text.split([' ', '{', '(']).next().unwrap_or("").to_owned()
}

fn act(game: &GameState, command: &Command, round: u32, player: PlayerId) -> Act {
    let auth = game.view().auth;
    let before = snap(game);
    let unit = |h| auth.units().get(h).and_then(|u| before.units.iter().find(|x| x.id == u.core.id.0).copied());
    let city = |h| auth.cities().get(h).and_then(|c| before.cities.iter().find(|x| x.id == c.id.0).cloned());
    let mut a = Act { name: variant(command), player: player.0, round, ..Act::default() };
    match *command {
        Command::AttackUnit { attacker, defender } => {
            a.attack = true;
            a.actor = unit(attacker);
            a.target = unit(defender).map(|u| u.tile);
        }
        Command::AttackCity { attacker, city: c } => {
            a.attack = true;
            a.actor = unit(attacker);
            a.target = city(c).map(|c| c.tile);
        }
        Command::RangedAttack { unit: u, target } | Command::AirSweep { unit: u, target } | Command::Nuke { unit: u, target } => {
            a.attack = true;
            a.actor = unit(u);
            a.target = Some(target.0);
        }
        Command::CityStrike { city: c, target } => {
            a.attack = true;
            a.city = city(c);
            a.target = Some(target.0);
        }
        _ => {
            a.actor = classic_rules::knowledge::actions::acting_unit(command).and_then(unit);
        }
    }
    a.before = before;
    a
}

/// The events of one command: the world before (`act`) against the world after.
fn events(game: &GameState, rules: &CompiledRuleset, act: Act) -> Vec<Value> {
    let after = snap(game);
    let auth = game.view().auth;
    let max_hp = |id: u64| {
        auth.cities().iter().find(|(_, c)| c.id.0 == id).map_or(0, |(_, c)| classic_rules::combat::city::max_hit_points(auth, rules, c))
    };
    let mut out = Vec::new();
    let now: BTreeMap<u64, &Unit> = after.units.iter().map(|u| (u.id, u)).collect();
    let was: BTreeMap<u64, &Unit> = act.before.units.iter().map(|u| (u.id, u)).collect();
    let cities_now: BTreeMap<u64, &City> = after.cities.iter().map(|c| (c.id, c)).collect();
    let cities_was: BTreeMap<u64, &City> = act.before.cities.iter().map(|c| (c.id, c)).collect();
    let founded: Vec<&City> = after.cities.iter().filter(|c| !cities_was.contains_key(&c.id)).collect();
    // Units hit or gone.
    let mut hits = Vec::new();
    let mut lost = Vec::new();
    for u in &act.before.units {
        match now.get(&u.id) {
            Some(n) if n.hp < u.hp && act.attack => hits.push(json!({ "unit": unit_ref(u), "hp0": u.hp, "hp1": n.hp, "dead": false })),
            Some(n) if n.owner != u.owner => out.push(json!({ "t": "seized", "unit": unit_ref(u), "to": n.owner, "tiles": [u.tile] })),
            Some(_) => {}
            None if act.attack && (u.owner != act.player || act.actor.is_some_and(|a| a.id == u.id)) => {
                // A civilian taken by the blow reappears as the attacker's unit of its kind on its tile.
                let seized = after.units.iter().any(|n| !was.contains_key(&n.id) && n.owner == act.player && n.tile == u.tile && n.kind == u.kind);
                hits.push(json!({ "unit": unit_ref(u), "hp0": u.hp, "hp1": 0, "dead": !seized, "seized": seized }))
            }
            None if u.owner != act.player => lost.push(u),
            None => {}
        }
    }
    let mut city_hit = Value::Null;
    for c in &act.before.cities {
        let Some(n) = cities_now.get(&c.id) else {
            out.push(json!({ "t": "razed", "city": city_ref(c), "by": act.player, "tiles": [c.tile] }));
            continue;
        };
        if act.attack && (n.damage != c.damage || n.owner != c.owner) && Some(c.tile) == act.target {
            let max = max_hp(c.id);
            city_hit = json!({ "city": city_ref(c), "hp0": max.saturating_sub(c.damage), "hp1": max.saturating_sub(n.damage), "max": max });
        }
        if n.owner != c.owner {
            out.push(json!({ "t": "captured", "city": city_ref(c), "from": c.owner, "to": n.owner, "pop": n.pop, "tiles": [c.tile] }));
        }
    }
    if act.attack {
        let by = match (&act.actor, &act.city) {
            (Some(u), _) => json!({ "unit": unit_ref(u), "hp1": now.get(&u.id).map_or(0, |n| n.hp) }),
            (None, Some(c)) => json!({ "city": city_ref(c) }),
            _ => Value::Null,
        };
        let mut tiles: Vec<u32> = act.target.into_iter().collect();
        tiles.extend(act.actor.map(|u| u.tile));
        tiles.extend(act.city.as_ref().map(|c| c.tile));
        out.insert(0, json!({ "t": "attack", "cmd": act.name, "p": act.player, "by": by, "at": act.target, "hits": hits, "city": city_hit, "tiles": tiles }));
    }
    for u in lost {
        // A unit taken where a new one of the actor's stands: a capture (a civilian), else a loss.
        let taken = after.units.iter().find(|n| !was.contains_key(&n.id) && n.owner == act.player && n.tile == u.tile && n.kind == u.kind);
        out.push(json!({ "t": if taken.is_some() { "seized" } else { "lost" }, "unit": unit_ref(u), "to": act.player, "tiles": [u.tile] }));
    }
    for c in founded {
        out.push(json!({ "t": "founded", "city": city_ref(c), "tiles": [c.tile] }));
    }
    for n in &after.units {
        if !was.contains_key(&n.id) {
            let seized = act.before.units.iter().any(|u| u.tile == n.tile && u.kind == n.kind && u.owner != n.owner && !now.contains_key(&u.id));
            if !seized {
                out.push(json!({ "t": "new", "unit": unit_ref(n), "tiles": [n.tile] }));
            }
        }
    }
    for &(a, b) in after.wars.difference(&act.before.wars) {
        out.push(json!({ "t": "war", "a": a, "b": b, "by": act.player, "tiles": [] }));
    }
    for &(a, b) in act.before.wars.difference(&after.wars) {
        out.push(json!({ "t": "peace", "a": a, "b": b, "by": act.player, "tiles": [] }));
    }
    for e in &mut out {
        e["r"] = json!(act.round);
    }
    out
}

/// One frame, on the whole map (cut to the window at the end).
#[derive(Debug, Clone)]
struct Frame {
    round: u32,
    /// The player whose turn just ended (None: the scene's set-up).
    done: Option<u8>,
    tiles: Vec<[u16; 4]>,
    units: Vec<Unit>,
    cities: Vec<(City, u16)>,
    alive: Vec<bool>,
    standings: Vec<Value>,
    events: Vec<Value>,
    /// Every major's empire (`empire::Emp`; None: not a major).
    empire: Vec<Option<empire::Emp>>,
}

const NONE: u16 = u16::MAX;

fn tile_layers(game: &GameState) -> Vec<[u16; 4]> {
    let map = game.view().auth.map();
    (0..map.tile_count() as u32)
        .map(|t| {
            let l = map.layers(TileId(t));
            let owner = l.and_then(|l| l.owner).map_or(NONE, |p| u16::from(p.0));
            let imp = l.and_then(|l| l.improvement).map_or(NONE, |i| i.0 | if map.pillaged(TileId(t)) { 0x8000 } else { 0 });
            let route = l.map_or(0, |l| u16::from(l.route.code()));
            let feature = l.and_then(|l| l.feature).map_or(NONE, |f| f.0);
            [owner, imp, route, feature]
        })
        .collect()
}

/// `ends`: each city's production when its owner ended its last turn (`Collect::ends`).
fn frame(game: &GameState, rules: &CompiledRuleset, round: u32, done: Option<u8>, events: Vec<Value>, ends: &HashMap<u64, empire::End>) -> Frame {
    let auth = game.view().auth;
    let s = snap(game);
    let cities = s
        .cities
        .into_iter()
        .map(|c| {
            let max = auth.cities().iter().find(|(_, x)| x.id.0 == c.id).map_or(0, |(_, x)| classic_rules::combat::city::max_hit_points(auth, rules, x));
            (c, max)
        })
        .collect();
    let players = auth.players();
    let n = players.count() as u8;
    let standings = (0..n)
        .map(PlayerId)
        .filter(|p| players.is_major(*p))
        .map(|p| {
            let s = standing::<ClassicBench>(game, rules, p);
            json!({ "p": p.0, "score": s.score, "cities": s.cities, "pop": s.population, "techs": s.techs, "military": s.military })
        })
        .collect();
    Frame {
        round,
        done,
        tiles: tile_layers(game),
        units: s.units,
        cities,
        alive: (0..n).map(|p| players.is_alive(PlayerId(p))).collect(),
        standings,
        events,
        empire: empire::snapshot(game, rules, ends),
    }
}

/// The frames being collected while the log replays.
struct Collect {
    from: u64,
    per_turn: bool,
    position: u64,
    last: Option<(u32, u8)>,
    pending: Vec<Value>,
    frames: Vec<Frame>,
    /// The Classic era (`EraDefinition::index`) of each major at the first frame.
    eras: Vec<Option<u8>>,
    /// Each city's production when its owner ended its last turn, by city id: a frame
    /// shows it rather than the queue at the frame's moment, which the turn's production
    /// empties on a completion until the owner's next end of turn.
    ends: HashMap<u64, empire::End>,
    /// The tiles still holding ancient ruins (None: not looked at yet).
    ruins: Option<Vec<u32>>,
}

fn op_event(op: &crate::grounds::Op, round: u32) -> Value {
    let mut v = serde_json::to_value(op).unwrap_or(Value::Null);
    let tiles: Vec<u32> = match op {
        crate::grounds::Op::Spawn { tile, .. } | crate::grounds::Op::Damage { tile, .. } => vec![*tile],
        _ => Vec::new(),
    };
    if matches!(op, crate::grounds::Op::Reveal { .. }) {
        v["tiles"] = json!([]);
        v["count"] = json!(match op {
            crate::grounds::Op::Reveal { tiles, .. } => tiles.len(),
            _ => 0,
        });
    }
    json!({ "t": "scene", "op": v, "tiles": tiles, "r": round })
}

/// The frames of the log at `path` (its `.json` spec beside it, and its
/// `.e2.json` when it is a self-play game) as the player's JSON.
pub fn frames_of_log(rules: &CompiledRuleset, path: &Path, options: Options) -> Result<Value, String> {
    let (spec, journal) = export::read_log(path)?;
    let sidecar = path.with_extension("e2.json");
    let meta: Value = if sidecar.exists() {
        serde_json::from_slice(&std::fs::read(&sidecar).map_err(|e| format!("{}: {e}", sidecar.display()))?).map_err(|e| format!("{}: {e}", sidecar.display()))?
    } else {
        Value::Null
    };
    frames(rules, &spec, &journal, &meta, path.file_stem().and_then(|s| s.to_str()).unwrap_or(""), options)
}

/// `frames_of_log` of a spec, a journal and the game's `.e2.json` (`Null` without one).
pub fn frames(rules: &CompiledRuleset, spec: &LogSpec, journal: &[u8], meta: &Value, stem: &str, options: Options) -> Result<Value, String> {
    let gift = crate::e2::gift_of(meta)?;
    let edits = crate::grounds::edits_of(meta)?;
    let battle = meta.get("ground").and_then(|g| g.get("battle")).filter(|b| !b.is_null()).cloned();
    let in_battle = battle.is_some() && !options.whole;
    let sides: Vec<u8> = battle.as_ref().map_or_else(Vec::new, |b| ["learner", "enemy"].iter().filter_map(|k| b.get(*k).and_then(Value::as_u64)).map(|p| p as u8).collect());
    let from = if in_battle { edits.iter().map(|e| e.position).min().or_else(|| meta.get("from_commands").and_then(Value::as_u64)).unwrap_or(0) } else { 0 };
    let per_turn = options.per_turn.unwrap_or(in_battle);
    let collect = RefCell::new(Collect { from, per_turn, position: 0, last: None, pending: Vec::new(), frames: Vec::new(), eras: Vec::new(), ends: HashMap::new(), ruins: None });
    let names = Names::load();

    let (game, checked, commands) = export::replay_with(
        rules,
        spec,
        journal,
        |game, position| {
            if let Some(gift) = gift.as_ref().filter(|g| g.position == position) {
                crate::selfplay::apply_gift(game, rules, gift);
            }
            let round = game.view().auth.turn().round;
            let mut c = collect.borrow_mut();
            for e in edits.iter().filter(|e| e.position == position) {
                crate::grounds::apply(game, rules, &e.op);
                if position >= c.from {
                    c.pending.push(op_event(&e.op, round));
                }
            }
        },
        |game, command, round, player| {
            let mut c = collect.borrow_mut();
            // A battle's frames follow its two sides' turns; the others' (city-states, a third nation) join the turn before.
            let other = in_battle && !sides.is_empty() && !sides.contains(&player.0);
            let key = match c.last {
                Some(last) if other && c.per_turn && c.position > c.from => last,
                // Others play before the sides at the battle's start: nobody's turn yet.
                _ if other && c.per_turn => (round, u8::MAX),
                _ if c.per_turn => (round, player.0),
                _ => (round, 0),
            };
            let boundary = c.last.is_some_and(|l| l != key);
            let done = c.last.map(|l| l.1).filter(|p| *p != u8::MAX);
            if c.position == c.from || (c.position > c.from && boundary) {
                let events = std::mem::take(&mut c.pending);
                let first = c.frames.is_empty();
                let done = if first && in_battle { None } else { done.filter(|_| c.per_turn) };
                // The round played: the finished turn's (the scene's set-up: the round it starts in).
                let played = if first { round } else { c.last.map_or(round, |l| l.0) };
                if first {
                    let players = game.view().auth.players();
                    c.eras = (0..players.count() as u8).map(PlayerId).map(|p| players.is_major(p).then(|| players.nation_era_of(p, rules))).collect();
                }
                let frame = frame(game, rules, played, done, events, &c.ends);
                c.frames.push(frame);
            }
            if let Command::EndTurn { player } = command {
                let auth = game.view().auth;
                for h in auth.indexes().cities_of(*player) {
                    if let Some(city) = auth.cities().get(*h) {
                        c.ends.insert(city.id.0, empire::End { owner: *player, item: city.queue.first().copied(), process: city.economy.process });
                    }
                }
            }
            c.last = Some(key);
            c.position += 1;
            let ruins = c.ruins.get_or_insert_with(|| sites::ruins_tiles(game, rules));
            let before = sites::sites(game, rules, ruins, player);
            (act(game, command, round, player), before, player)
        },
        |game, (a, before, player)| {
            let mut c = collect.borrow_mut();
            let round = a.round;
            let (found_sites, ruins) = sites::events(game, rules, &before, player, round);
            c.ruins = Some(ruins);
            if c.position > c.from {
                let found = events(game, rules, a);
                c.pending.extend(found);
                c.pending.extend(found_sites);
            }
        },
    )?;
    let mut c = collect.into_inner();
    let events = std::mem::take(&mut c.pending);
    let done = c.last.map(|l| l.1).filter(|_| c.per_turn);
    let played = c.last.map_or(game.view().auth.turn().round, |l| l.0);
    let frame = frame(&game, rules, played, done, events, &c.ends);
    c.frames.push(frame);

    let auth = game.view().auth;
    let dims = auth.map().dimensions;
    let target = battle.as_ref().and_then(|b| b.get("target")).and_then(Value::as_u64).map(|t| t as u32);
    // The window: a battle's hexes around its target, wide enough for its armies; else the map.
    let window: Vec<u32> = match target.filter(|_| in_battle) {
        Some(t) => {
            let near = |tile: u32| dims.distance(TileId(t), TileId(tile)).unwrap_or(i32::MAX);
            // The armies: the units the scene spawned (its edits), else both sides' units near the target.
            let spawned: Vec<(u8, u32, u16)> = edits
                .iter()
                .filter_map(|e| match e.op {
                    crate::grounds::Op::Spawn { player, tile, kind, .. } => Some((player, tile, kind)),
                    _ => None,
                })
                .collect();
            let army = |u: &Unit| if spawned.is_empty() { sides.contains(&u.owner) && near(u.tile) <= 8 } else { spawned.contains(&(u.owner, u.tile, u.kind)) };
            let first = c.frames.first().map(|f| f.units.iter().filter(|u| army(u)).map(|u| u.id).collect::<BTreeSet<_>>()).unwrap_or_default();
            let reach = c.frames.iter().flat_map(|f| f.units.iter()).filter(|u| first.contains(&u.id)).map(|u| near(u.tile)).max().unwrap_or(0);
            let radius = (reach + 2).clamp(4, 10);
            kernel::math::hex_disc(dims, TileId(t), radius).map(|t| t.0).collect()
        }
        None => (0..dims.tile_count() as u32).collect(),
    };
    let index: HashMap<u32, usize> = window.iter().enumerate().map(|(i, t)| (*t, i)).collect();

    // Static layers of the window.
    let map = auth.map();
    let mut terrain = Vec::new();
    let mut resource = Vec::new();
    let mut coords = Vec::new();
    for &t in &window {
        let l = map.layers(TileId(t));
        terrain.push(l.map_or(0, |l| l.terrain.0));
        resource.push(l.and_then(|l| l.resource).map_or(-1, |r| i32::from(r.0)));
        let c = dims.to_coord(TileId(t)).map_or((0, 0), |c| (c.q, c.r));
        coords.push([c.0, c.1]);
    }
    let rivers: Vec<[usize; 2]> = map.river_edges().iter().filter_map(|&(a, b)| Some([*index.get(&a)?, *index.get(&b)?])).collect();

    // Legends: what the codes of the frames are, from the rules.
    let codes: BTreeSet<u16> = terrain.iter().copied().collect();
    let terrains: Value = codes
        .iter()
        .map(|&code| {
            let def = rules.terrain_by_code.get(code as usize).copied().flatten().and_then(|k| rules.terrains.get(k.0 as usize));
            let water = rules.terrain_kinds.is_water(code);
            let rgb = RENDER_TERRAIN_RGB.get(code as usize).copied().unwrap_or(if water { [0.08, 0.46, 0.66] } else { [0.6, 0.6, 0.5] });
            let colour = format!("#{:02x}{:02x}{:02x}", (rgb[0] * 255.0) as u8, (rgb[1] * 255.0) as u8, (rgb[2] * 255.0) as u8);
            let name = def.map_or_else(|| format!("{code}"), |d| names.get("terrain", &d.key, &d.name));
            (code.to_string(), json!({ "name": name, "color": colour, "water": water, "hills": rules.terrain_kinds.is_hills(code), "mountains": rules.terrain_kinds.is_mountains(code) }))
        })
        .collect::<serde_json::Map<_, _>>()
        .into();
    let units: Vec<Value> = rules
        .units
        .iter()
        .map(|u| {
            json!({ "name": names.get("unit", &u.key, &u.name), "role": u.role.map(|r| format!("{r:?}")), "domain": format!("{:?}", u.domain),
                    "strength": u.strength, "ranged": u.ranged_strength, "civilian": u.strength == 0 && u.ranged_strength == 0 })
        })
        .collect();
    let features: Vec<String> = rules.features.iter().map(|f| names.get("feature", &f.key, &f.name)).collect();
    let resources: Vec<String> = rules.resources.iter().map(|r| names.get("resource", &r.key, &r.name)).collect();
    let improvements: Vec<String> = rules.improvements.iter().map(|i| names.get("improvement", &i.key, &i.name)).collect();

    // Players: nation (or city-state, barbarians), colour, the seat's mind.
    let seats: HashMap<u64, Value> = meta.get("seats").and_then(Value::as_array).map(|s| s.iter().filter_map(|x| Some((x.get("player")?.as_u64()?, x.clone()))).collect()).unwrap_or_default();
    let learners: Vec<u64> = meta.get("learners").and_then(Value::as_array).map(|l| l.iter().filter_map(Value::as_u64).collect()).unwrap_or_default();
    let players_store = auth.players();
    let players: Vec<Value> = (0..players_store.count() as u8)
        .map(|p| {
            let id = PlayerId(p);
            let (kind, name, colour) = match players_store.kind_of(id) {
                PlayerKind::Major => {
                    let n = rules.nations.get(players_store.nation_of(id) as usize);
                    let c = n.map_or((128, 128, 128), |n| n.color);
                    ("major", n.map_or_else(|| format!("{p}"), |n| names.get("nation", &n.key, &n.name)), format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2))
                }
                PlayerKind::Module(ClassicKind::CityState(cs)) => {
                    let d = rules.module.city_states.get(cs.0 as usize);
                    ("city-state", d.map_or_else(String::new, |d| names.get("city-state", &d.key, &d.name)), String::new())
                }
                PlayerKind::Module(ClassicKind::Barbarian) => ("barbarian", String::new(), String::new()),
            };
            let seat = seats.get(&u64::from(p));
            let learner_mind = meta.get("learner_mind").and_then(Value::as_str).filter(|_| learners.contains(&u64::from(p)));
            let mind = learner_mind
                .or_else(|| seat.and_then(|s| s.get("mind")).and_then(Value::as_str))
                .map(str::to_owned)
                .unwrap_or_else(|| if learners.contains(&u64::from(p)) { "learner".to_owned() } else { spec.variant.clone() });
            json!({ "id": p, "kind": kind, "name": name, "color": colour, "mind": mind, "learner": learners.contains(&u64::from(p)) })
        })
        .collect();

    // Frames cut to the window, tiles as changes from the frame before.
    let inside = |tiles: &Value| tiles.as_array().is_none_or(|t| t.is_empty() || t.iter().any(|x| x.as_u64().is_some_and(|x| index.contains_key(&(x as u32)))));
    let words = empire::Words::new(rules, &names);
    let decisions = empire::decisions(&c.frames.iter().map(|f| (f.round, f.empire.as_slice(), f.units.as_slice(), f.events.as_slice())).collect::<Vec<_>>(), &words);
    let mut prev: Option<&Frame> = None;
    let mut out_frames = Vec::new();
    for f in &c.frames {
        let mut changes = Vec::new();
        for (i, &t) in window.iter().enumerate() {
            let now = f.tiles[t as usize];
            if prev.is_none_or(|p| p.tiles[t as usize] != now) {
                changes.push(json!([i, now[0], now[1], now[2], now[3]]));
            }
        }
        let units: Vec<Value> = f.units.iter().filter_map(|u| Some(json!([u.id, u.owner, u.kind, u.hp, index.get(&u.tile)?]))).collect();
        let cities: Vec<Value> = f.cities.iter().filter_map(|(c, max)| Some(json!([c.id, c.owner, index.get(&c.tile)?, c.pop, max.saturating_sub(c.damage), max, c.name]))).collect();
        let events: Vec<Value> = f
            .events
            .iter()
            .filter(|e| !in_battle || inside(&e["tiles"]) || e["t"] == "war" || e["t"] == "peace")
            .map(|e| {
                let mut e = e.clone();
                if let Some(obj) = e.as_object_mut() {
                    let tiles: Vec<Value> = obj.remove("tiles").and_then(|t| t.as_array().cloned()).unwrap_or_default();
                    obj.insert("at_i".into(), json!(tiles.iter().filter_map(|t| index.get(&(t.as_u64()? as u32))).collect::<Vec<_>>()));
                }
                e
            })
            .collect();
        out_frames.push(json!({ "round": f.round, "done": f.done, "tiles": changes, "units": units, "cities": cities, "alive": f.alive, "standings": f.standings, "events": events, "empire": empire::status(&f.empire, &words) }));
        prev = Some(f);
    }

    // The era of the scene: the learner's (a whole game: the leader's at the end), with its Russian name.
    let era_of = |i: u8| rules.eras.iter().find(|e| e.index == i).map(|e| json!({ "index": i, "name": names.get("era", &e.key, &e.name) }));
    let era = match sides.first() {
        Some(&l) if in_battle => c.eras.get(l as usize).copied().flatten().and_then(era_of),
        _ => {
            let players = auth.players();
            (0..players.count() as u8).map(PlayerId).filter(|p| players.is_major(*p)).map(|p| players.nation_era_of(p, rules)).max().and_then(era_of)
        }
    }
    .unwrap_or(Value::Null);
    // The armies at the scene's start: the learner's combat units within 10 of the target (its army,
    // even a ring rounds of march back), the defender's within 3, as counts by kind name.
    let armies = match (target.filter(|_| in_battle), c.frames.first()) {
        (Some(t), Some(f)) => {
            let near = |tile: u32| dims.distance(TileId(t), TileId(tile)).unwrap_or(i32::MAX);
            let count = |side: u8, radius: i32| {
                let mut by: BTreeMap<String, u32> = BTreeMap::new();
                for u in f.units.iter().filter(|u| u.owner == side && near(u.tile) <= radius) {
                    let Some(def) = rules.units.get(u.kind as usize) else { continue };
                    if def.strength == 0 && def.ranged_strength == 0 {
                        continue;
                    }
                    *by.entry(names.get("unit", &def.key, &def.name)).or_default() += 1;
                }
                let mut v: Vec<(String, u32)> = by.into_iter().collect();
                v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
                json!(v)
            };
            json!({ "learner": sides.first().map_or(Value::Null, |&p| count(p, 10)), "enemy": sides.get(1).map_or(Value::Null, |&p| count(p, 3)) })
        }
        _ => Value::Null,
    };

    // The scene: what was played and how it ended.
    let standings = export::standings(&game, rules);
    let scene = json!({
        "stem": stem,
        "kind": if in_battle { meta["ground"]["kind"].clone() } else if meta.get("ground").is_some_and(|g| !g.is_null()) { json!(format!("game:{}", meta["ground"]["kind"].as_str().unwrap_or(""))) } else { json!("game") },
        "battle": if in_battle { battle.clone().unwrap_or(Value::Null) } else { Value::Null },
        "spec": spec,
        "net": meta.get("net").cloned().unwrap_or(Value::Null),
        "final_round": auth.turn().round,
        "standings": standings,
        "victory": meta.get("victory").cloned().unwrap_or(Value::Null),
        "checked": checked,
        "commands": commands,
        "from_position": from,
        "target_i": target.and_then(|t| index.get(&t)),
        "era": era,
        "armies": armies,
    });
    Ok(json!({
        "format": 1,
        "scene": scene,
        "map": { "width": dims.width, "height": dims.height, "coords": coords, "terrain": terrain, "resource": resource, "rivers": rivers },
        "legend": { "terrain": terrains, "units": units, "features": features, "resources": resources, "improvements": improvements, "improvement_kinds": sites::kinds(rules), "unit_max_hp": kernel::world::unit::UNIT_MAX_HP },
        "players": players,
        "per_turn": per_turn,
        "decisions": decisions,
        "frames": out_frames,
    }))
}
