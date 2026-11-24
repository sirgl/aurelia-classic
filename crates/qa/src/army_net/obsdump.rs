//! The army net's observations with the scene state they were built from (`ARMY_OBS_DUMP=FILE`, the player
//! `army-net` of `ai-exam suite`): the parity source of the MLX twin of `observe` (`nn/aurelia_nn/tacsim/armyobs.py`).
//! A MEASUREMENT tool: it changes no rule, no AI and no decision of the net.
//!
//! Per play a `scene` line: the scene in the form of `ai-exam suite-dump` (`trainers::dump::Track`: window
//! tiles, units with their compiled profiles, cities, the RNG), the orders (the objective token's inputs, the army's
//! slots, the static map planes) and a table of unit kinds (the arm, range, moves, strengths the tokens read). Per
//! decision of the net a `moment` line: the state (the dump's state record), every slot's map tile and entity id,
//! the history's turn-start snapshot (numbers 46 and 47), the tokens (f16 bits, hex), the distances, the window
//! positions, the army slots, and the candidates of every selectable army unit; then the net's pick and the
//! candidate it applied (the in-loop comparison). Lines of a play are written together at its end.

use std::cell::RefCell;
use std::io::Write;
use std::sync::Mutex;

use classic_rules::{CompiledRuleset, GameState};
use serde_json::{json, Value};

use super::observe::{self, Action, Candidate, Tokens};
use super::Orders;
use crate::trainers::dump::Track;
use crate::trainers::{act, Scene};

struct Play {
    name: String,
    track: Track,
    lines: Vec<String>,
    moment: Option<Value>,
    /// The units and cities at the last event (`ARMY_OBS_EVENTS`): an event writes what changed since.
    last: Option<(Vec<Value>, Vec<Value>)>,
    /// The third seat's rows at the last event (info-only: its changes go to the event's `t`).
    last_third: Vec<Value>,
}

thread_local! {
    static PLAY: RefCell<Option<Play>> = const { RefCell::new(None) };
    /// The source the events of `act`'s commands carry ("act"; "rule" while the scene rule plays).
    static ACT_SRC: std::cell::Cell<&'static str> = const { std::cell::Cell::new("act") };
}

thread_local! {
    /// What the scene rule would have chosen for the next mind event (the shadow, a measurement).
    static SHADOW: RefCell<Option<Value>> = const { RefCell::new(None) };
}

/// The scene rule's shadow (rule off, events on; a MEASUREMENT): what `trainers::scene_rule` would choose for the
/// actor of the mind's command about to run, on the state before it: a city strike's target (the rule's key and
/// the most-damage key), a shot's target, a promotion; at a seat's turn end, its pieces the rule would still
/// fortify, promote or let shoot.
pub fn shadow(rules: &CompiledRuleset, game: &GameState, cmd: Option<&classic_rules::Command>) {
    use crate::trainers::scene_rule as r;
    if !events_wanted() || r::on() {
        return;
    }
    let v = PLAY.with(|p| {
        let mut p = p.borrow_mut();
        let play = p.as_mut()?;
        let auth = game.view().auth;
        let side = auth.active_player();
        if play.track.side_of(side) < 0 {
            return None;
        }
        let ids: Vec<kernel::ids::EntityId> = play.track.ids().to_vec();
        let by_id: std::collections::BTreeMap<kernel::ids::EntityId, (classic_rules::ids::UnitHandle, kernel::world::unit::UnitCore)> =
            auth.units().iter_handles().filter(|(_, c)| ids.contains(&c.id)).map(|(h, c)| (c.id, (h, *c))).collect();
        let mut foes: Vec<classic_rules::ids::TileId> = by_id.values().filter(|(_, c)| c.owner != side && play.track.side_of(c.owner) >= 0).map(|(_, c)| c.tile).collect();
        foes.sort_by_key(|t| t.0);
        foes.dedup();
        let win = |t: Option<classic_rules::ids::TileId>| t.map_or(-1, |t| i64::from(play.track.index(t)));
        use classic_rules::Command as C;
        match cmd {
            Some(C::CityStrike { city, .. }) => Some(json!([win(r::city_target(game, rules, side, *city, &foes)), win(r::city_target_by(game, rules, side, *city, &foes, true))])),
            Some(C::RangedAttack { unit, .. }) => Some(json!([win(r::shot_target(game, rules, side, *unit, &foes))])),
            Some(C::Promote { unit, .. }) => act::core_of(game, *unit).map(|c| json!([r::promotion_pick(game, rules, &c).map_or(-1, |p| i64::from(p.0))])),
            Some(_) => None,
            None => {
                let mine: Vec<(usize, classic_rules::ids::UnitHandle, kernel::world::unit::UnitCore)> =
                    ids.iter().enumerate().filter_map(|(k, id)| by_id.get(id).filter(|(_, c)| c.owner == side).map(|(h, c)| (k, *h, *c))).collect();
                let fort: Vec<usize> = mine.iter().filter(|(_, h, _)| r::fortifies(game, rules, side, *h)).map(|x| x.0).collect();
                let promo: Vec<usize> = mine.iter().filter(|(_, _, c)| r::promotion_pick(game, rules, c).is_some()).map(|x| x.0).collect();
                let shot: Vec<usize> = mine.iter().filter(|(_, h, _)| r::shot_target(game, rules, side, *h, &foes).is_some()).map(|x| x.0).collect();
                Some(json!({"fort": fort, "promo": promo, "shot": shot}))
            }
        }
    });
    SHADOW.with(|x| *x.borrow_mut() = v);
}

pub fn shadow_clear() {
    SHADOW.with(|x| x.borrow_mut().take());
}

pub fn set_act_src(src: &'static str) {
    ACT_SRC.with(|c| c.set(src));
}

pub fn act_src() -> &'static str {
    ACT_SRC.with(|c| c.get())
}

static OUT: Mutex<Option<std::fs::File>> = Mutex::new(None);

pub fn wanted() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("ARMY_OBS_DUMP").is_some())
}

fn hex16(v: &[f32]) -> String {
    v.iter().map(|x| format!("{:04x}", observe::f32_to_f16(*x))).collect()
}

/// A new play of the net: the scene line.
pub fn start(rules: &CompiledRuleset, game: &GameState, scene: &Scene, orders: &Orders) {
    if !wanted() {
        return;
    }
    let (mut track, sc, initial) = Track::start(rules, game, scene);
    let name = format!("{}/{}/{}/{}", scene.fam.name(), scene.era, scene.instance, scene.attempt);
    let army: Vec<i64> = orders.army.iter().map(|id| track.slot_of(*id).map_or(-1, |s| s as i64)).collect();
    let (q0, r0, w, map_w) = track.window();
    let kinds: Vec<Value> = (0..rules.units.len())
        .map(|k| {
            let kind = classic_rules::ids::UnitKindId(k as u16);
            let d = &rules.units[k];
            json!([
                observe::arm_slot(rules, kind),
                act::range_of(rules, kind).unwrap_or(-1),
                act::moves_of(rules, kind),
                act::is_civilian(rules, kind),
                d.strength,
                d.ranged_strength,
                d.domain == classic_rules::rules::content::Domain::Land,
                super::in_army(rules, kind),
                super::base_strength(rules, kind),
                d.combat.pillage,
                d.combat.air_intercept_range
            ])
        })
        .collect();
    let auth = game.view().auth;
    let far: Vec<Value> = auth
        .cities()
        .iter()
        .filter(|(_, c)| (c.owner == orders.side || c.owner == orders.foe) && track.index(c.tile) < 0)
        .map(|(_, c)| json!([c.tile.0, if c.owner == orders.side { 0 } else { 1 }]))
        .collect();
    let city_handles: Vec<i64> = track.cities().iter().map(|t| auth.indexes().city_on_tile(*t).map_or(-1, |h| i64::from(h.index))).collect();
    let o = json!({
        "aim": orders.aim.index(), "tile": orders.tile.0, "refuge": orders.refuge.map_or(-1, |t| t.0 as i64),
        "optional": orders.optional, "start_round": orders.start_round, "rounds": orders.rounds, "army": army,
        "mean_strength": orders.mean_strength, "fog": orders.fog, "price": orders.price,
        "statics": hex16(&orders.statics), "map_window": [orders.window.q0, orders.window.r0, orders.window.map_w, orders.window.map_h],
        "window": [q0, r0, w, map_w], "far_cities": far, "city_handles": city_handles,
        "promos": track.promos().iter().map(|p| p.0).collect::<Vec<u16>>(),
        "pillageable": pillageable(rules, game, &track),
        "ruins": ruins(rules, game, &track),
        // The judge's inputs (`families::judge`): the scene's improved tiles (window indices), its context, the pieces.
        "improved": scene.improved.iter().map(|t| track.index(*t)).collect::<Vec<i32>>(),
        "context": scene.context, "n_mine": scene.mine.len(), "family": scene.fam.name(),
        "roles": scene.mine.iter().chain(&scene.theirs).map(|p| p.role).collect::<Vec<&str>>(),
        "deadline": scene.deadline,
        // The scene's private facts the family scripts read (the memory families' side, hiding place and first step): info only.
        "centre": scene.centre.0, "info": scene.info,
    });
    let line = json!({"type": "scene", "play": name, "scene": sc, "initial": initial, "orders": o, "kinds": kinds});
    PLAY.with(|p| *p.borrow_mut() = Some(Play { name, track, lines: vec![line.to_string()], moment: None, last: Some(units_cities(&initial)), last_third: initial["third"].as_array().cloned().unwrap_or_default() }));
}

fn units_cities(state: &Value) -> (Vec<Value>, Vec<Value>) {
    (state["units"].as_array().cloned().unwrap_or_default(), state["cities"].as_array().cloned().unwrap_or_default())
}

/// The events beside the dump (`ARMY_OBS_EVENTS=1` with `ARMY_OBS_DUMP`): every command a seat of the scene
/// executes, whoever issues it (the net or a family script through `act`, the seat's AI mind through the driver),
/// and each turn's end, with what it changed. A MEASUREMENT: it reads the game and changes nothing.
pub fn events_wanted() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| wanted() && std::env::var_os("ARMY_OBS_EVENTS").is_some())
}

/// One event: `src` who issued it ("act": the net or a family script; "mind": the seat's AI; "end": the turn's end),
/// the command (its name, the acting unit's slot or the city's index, the target's window tile), and the units
/// (`[slot, state record]`) and cities (`[index, record]`) that changed.
pub fn event(rules: &CompiledRuleset, game: &GameState, side: classic_rules::ids::PlayerId, src: &str, cmd: Option<&classic_rules::Command>) {
    if !events_wanted() {
        return;
    }
    PLAY.with(|p| {
        let mut p = p.borrow_mut();
        let Some(play) = p.as_mut() else { return };
        let auth = game.view().auth;
        let state = play.track.state(rules, game);
        let (units, cities) = units_cities(&state);
        let (lu, lc) = play.last.take().unwrap_or_default();
        let du: Vec<Value> = units.iter().enumerate().filter(|(k, u)| lu.get(*k) != Some(*u)).map(|(k, u)| json!([k, u])).collect();
        let dc: Vec<Value> = cities.iter().enumerate().filter(|(k, c)| lc.get(*k) != Some(*c)).map(|(k, c)| json!([k, c])).collect();
        play.last = Some((units, cities));
        // Info-only: the third seat's changed rows `[index, row]`, its cities and the camp's countdown.
        let third: Vec<Value> = state["third"].as_array().cloned().unwrap_or_default();
        let dt: Vec<Value> = third.iter().enumerate().filter(|(k, u)| play.last_third.get(*k) != Some(*u)).map(|(k, u)| json!([k, u])).collect();
        play.last_third = third;
        use classic_rules::Command as C;
        let (name, who, tgt) = match cmd {
            None => ("End".to_owned(), -1i64, -1i64),
            Some(c) => {
                let name: String = format!("{c:?}").chars().take_while(|ch| ch.is_alphanumeric()).collect();
                let slot_of = |h: classic_rules::ids::UnitHandle| auth.units().get(h).and_then(|u| play.track.ids().iter().position(|x| *x == u.core.id)).map_or(-2, |s| s as i64);
                let city_of = |h: classic_rules::ids::CityHandle| auth.cities().get(h).and_then(|c| play.track.cities().iter().position(|t| *t == c.tile)).map_or(-2, |s| s as i64);
                let win = |t: classic_rules::ids::TileId| i64::from(play.track.index(t));
                let who = match *c {
                    C::CityStrike { city, .. } => city_of(city),
                    _ => classic_rules::knowledge::actions::acting_unit(c).map_or(-1, slot_of),
                };
                let tgt = match *c {
                    C::MoveUnit { destination, .. } | C::MarchTo { destination, .. } => win(destination),
                    C::AttackUnit { defender, .. } => auth.units().get(defender).map_or(-2, |u| win(u.core.tile)),
                    C::AttackCity { city, .. } => auth.cities().get(city).map_or(-2, |x| win(x.tile)),
                    C::RangedAttack { target, .. } | C::CityStrike { target, .. } => win(target),
                    _ => -1,
                };
                (name, who, tgt)
            }
        };
        let rel = play.track.side_of(side);
        let mut line = json!({"type": "ev", "round": auth.turn().round, "side": rel, "src": src, "cmd": name, "who": who, "tgt": tgt, "u": du, "c": dc, "rng": state["rng"], "vis": state["vis"]});
        if let Some(r) = SHADOW.with(|x| x.borrow_mut().take()) {
            line["rule"] = r;
        }
        if !state["third"].is_null() {
            line["t"] = json!(dt);
            line["tc"] = state["third_cities"].clone();
            line["camp"] = state["camp"].clone();
        }
        play.lines.push(line.to_string());
    });
}

/// `AiDriver::step` for a seat that is not recorded, with its commands and turn end as events.
pub fn step<P: kernel::ai::mind::AiMind<classic_rules::classic::Classic>>(driver: &mut kernel::ai::driver::AiDriver<classic_rules::classic::Classic, P>, game: &mut GameState, rules: &CompiledRuleset) -> kernel::ai::StepOutcome {
    use kernel::ai::{Advance, StepOutcome};
    driver.configure_game(game);
    match driver.advance(game, rules) {
        Advance::Worked => StepOutcome::Worked,
        Advance::RoundDone => StepOutcome::RoundDone,
        Advance::Waiting => StepOutcome::Waiting,
        Advance::Command(command) => {
            let side = game.view().auth.active_player();
            shadow(rules, game, Some(&command));
            let seen = command.clone();
            let result = game.execute(rules, command);
            if result.is_ok() {
                event(rules, game, side, "mind", Some(&seen));
            } else {
                shadow_clear();
            }
            driver.executed(result)
        }
        Advance::EndTurn(command) => {
            let side = game.view().auth.active_player();
            shadow(rules, game, None);
            let result = game.execute(rules, command);
            event(rules, game, side, "end", None);
            driver.executed(result)
        }
    }
}

fn action_of(rules: &CompiledRuleset, game: &GameState, track: &mut Track, c: &Candidate) -> Value {
    let auth = game.view().auth;
    let kind = c.feat[..10].iter().position(|x| *x > 0.5).unwrap_or(9);
    let extra: i64 = match &c.action {
        Action::Done => -1,
        Action::Command(cmd) => match *cmd {
            classic_rules::Command::AttackUnit { defender, .. } => auth.units().get(defender).and_then(|u| track.slot_of(u.core.id)).map_or(-2, |s| s as i64),
            classic_rules::Command::AttackCity { city, .. } => auth.cities().get(city).map_or(-2, |c| i64::from(c.tile.0)),
            classic_rules::Command::MoveUnit { destination, .. } => i64::from(destination.0),
            classic_rules::Command::RangedAttack { target, .. } => i64::from(target.0),
            _ => -1,
        },
    };
    // The Rust plan of a strike (diagnostics of the twin's previews): strengths, the attacker's damage and wounded mod.
    let plan = match &c.action {
        Action::Command(cmd) => match *cmd {
            classic_rules::Command::AttackUnit { attacker, defender } => auth.units().get(defender).map(|u| (attacker, u.core.tile)),
            classic_rules::Command::AttackCity { attacker, city } => auth.cities().get(city).map(|x| (attacker, x.tile)),
            classic_rules::Command::RangedAttack { unit, target } => Some((unit, target)),
            _ => None,
        },
        Action::Done => None,
    };
    let p = plan.and_then(|(u, t)| {
        let me = act::core_of(game, u)?;
        let how = if act::range_of(rules, me.kind).is_some() { classic_rules::combat::AttackKind::Ranged } else { classic_rules::combat::AttackKind::Melee };
        classic_rules::combat::plan_attack(game.view(), rules, u, t, how).ok().map(|p| json!([p.attacker_strength, p.defender_strength, p.attacker_damage, p.attacker_wounded_mod, p.defender_hp]))
    });
    json!([kind, c.pos, c.promo, extra, p])
}


/// Per window tile: something a unit could pillage there (a working improvement that is not permanent, or a road).
fn pillageable(rules: &CompiledRuleset, game: &GameState, track: &Track) -> String {
    let map = game.view().auth.map();
    let (q0, r0, w, map_w) = track.window();
    (0..w * w)
        .map(|i| {
            let (q, r) = (q0 + i % w, r0 + i / w);
            if q >= map_w || r * map_w + q < 0 || (r * map_w + q) as usize >= map.tile_count() {
                return '0';
            }
            let t = classic_rules::ids::TileId((r * map_w + q) as u32);
            let permanent = map.working_improvement(t).and_then(|imp| rules.improvements.get(imp.0 as usize)).is_some_and(|d| {
                matches!(d.kind, classic_rules::rules::content::ImprovementKind::AncientRuins | classic_rules::classic::content::BARBARIAN_CAMP)
            });
            if (map.working_improvement(t).is_some() && !permanent) || map.road(t) {
                '1'
            } else {
                '0'
            }
        })
        .collect()
}

/// Per window tile: the ancient ruins still there (an AI unit that enters their tile takes the reward and they are gone).
fn ruins(rules: &CompiledRuleset, game: &GameState, track: &Track) -> String {
    let map = game.view().auth.map();
    let (q0, r0, w, map_w) = track.window();
    (0..w * w)
        .map(|i| {
            let (q, r) = (q0 + i % w, r0 + i / w);
            if q >= map_w || r * map_w + q < 0 || (r * map_w + q) as usize >= map.tile_count() {
                return '0';
            }
            let t = classic_rules::ids::TileId((r * map_w + q) as u32);
            let is_ruins = map
                .working_improvement(t)
                .and_then(|imp| rules.improvements.get(imp.0 as usize))
                .is_some_and(|d| matches!(d.kind, classic_rules::rules::content::ImprovementKind::AncientRuins));
            if is_ruins { '1' } else { '0' }
        })
        .collect()
}

/// Per unit slot: its place in the tile's unit list (`units_on`; -1 none). The strike features read the FIRST foreign unit of
/// a tile in that list, which is the one that arrived there first.
fn tile_places(game: &GameState, ids: &[u64]) -> Vec<i64> {
    let auth = game.view().auth;
    ids.iter()
        .map(|i| {
            auth.units()
                .iter_handles()
                .find(|(_, c)| c.id.0 == *i)
                .and_then(|(h, c)| auth.indexes().units_on(c.tile).iter().position(|x| *x == h))
                .map_or(-1, |p| p as i64)
        })
        .collect()
}

/// A decision moment of the net (before its pick): the state and what the builder made of it.
#[allow(clippy::too_many_arguments)]
pub fn moment(rules: &CompiledRuleset, game: &GameState, orders: &Orders, tokens: &Tokens, slots: &[i16], step: u32, made: &[(kernel::ids::EntityId, u32)]) {
    if !wanted() {
        return;
    }
    PLAY.with(|p| {
        let mut p = p.borrow_mut();
        let Some(play) = p.as_mut() else { return };
        let auth = game.view().auth;
        let state = play.track.state(rules, game);
        let ids: Vec<u64> = play.track.ids().iter().map(|i| i.0).collect();
        let by_id: std::collections::BTreeMap<u64, (u32, u32)> = auth.units().iter_handles().map(|(h, c)| (c.id.0, (c.tile.0, h.index))).collect();
        let maptile: Vec<i64> = ids.iter().map(|i| by_id.get(i).map_or(-1, |x| i64::from(x.0))).collect();
        let handles: Vec<i64> = ids.iter().map(|i| by_id.get(i).map_or(-1, |x| i64::from(x.1))).collect();
        let prev: Vec<Value> = observe::history_prev().into_iter().filter_map(|(id, t, hp)| play.track.ids().iter().position(|x| *x == id).map(|s| json!([s, t.0, hp]))).collect();
        let n = tokens.len();
        let feat: Vec<f32> = tokens.feat.iter().flat_map(|f| f.iter().copied()).collect();
        let dist: String = tokens.dist.iter().map(|d| format!("{d:02x}")).collect();
        let refs: Vec<Value> = (0..n)
            .map(|i| match tokens.unit[i] {
                Some(h) => auth.units().get(h).and_then(|u| play.track.slot_of(u.core.id)).map_or(json!(["u", -1]), |s| json!(["u", s])),
                None if i == 0 => json!(["o", -1]),
                None => json!(["c", tokens.tile[i].0]),
            })
            .collect();
        let mut cands = serde_json::Map::new();
        for i in 0..n {
            if !tokens.selectable[i] {
                continue;
            }
            let Some(h) = tokens.unit[i] else { continue };
            let list = observe::candidates(game, rules, orders, h);
            let feat: Vec<f32> = list.iter().flat_map(|c| c.feat.iter().copied()).collect();
            let acts: Vec<Value> = list.iter().map(|c| action_of(rules, game, &mut play.track, c)).collect();
            cands.insert(i.to_string(), json!({"feat": hex16(&feat), "act": acts}));
        }
        let wm: Vec<i64> = ids
            .iter()
            .map(|i| {
                auth.units().iter_handles().find(|(_, c)| c.id.0 == *i).map_or(0, |(_, c)| {
                    i64::from(classic_rules::effects::unit::unit_modifiers(auth, rules, c).stat(classic_rules::rules::effects::UnitStat::WoundedDamagePercent))
                })
            })
            .collect();
        let war = auth.diplomacy().at_war(orders.side, orders.foe);
        // The window cities' strength without the garrison and hit points now (techs and buildings change them: an
        // exogenous schedule for the simulator, whose scene has them at the start).
        use classic_rules::combat::city as cc;
        let cstr: Vec<Value> = play
            .track
            .cities()
            .iter()
            .map(|t| match auth.indexes().city_on_tile(*t).and_then(|c| auth.cities().get(c)) {
                Some(c) => {
                    let mods = cc::city_combat_mods(auth, rules, c);
                    let hill = if classic_rules::navigation::terrain::is_hills(auth.map(), *t) { cc::CITY_HILL_STRENGTH } else { 0 };
                    json!([cc::CITY_STRENGTH_BASE + i64::from(c.population) * cc::CITY_STRENGTH_PER_POP + mods.modified_defense() + cc::tech_strength(auth, rules, c.owner) + hill, cc::max_hit_points(auth, rules, c)])
                }
                None => json!([0, 1]),
            })
            .collect();
        let pill = pillageable(rules, game, &play.track);
        let made: Vec<Value> = made.iter().filter_map(|(id, k)| play.track.ids().iter().position(|x| x == id).map(|s| json!([s, k]))).collect();
        play.moment = Some(json!({
            "type": "moment", "play": play.name, "round": auth.turn().round, "step": step, "state": state, "ids": ids, "maptile": maptile,
            "handles": handles, "prev": prev, "made": made, "wm": wm, "war": war, "pillageable": pill, "ruins": ruins(rules, game, &play.track), "upos": tile_places(game, &ids), "cstr": cstr,
            "tok": {"n": n, "feat": hex16(&feat), "dist": dist, "pos": tokens.pos, "tile": tokens.tile.iter().map(|t| t.0).collect::<Vec<u32>>(), "slot": slots, "ref": refs},
            "cands": cands,
            // The history tokens the net reads (MBH; every net's tokens carry them): numbers (f16 bits), army slots, ages.
            "hist": {"f": hex16(&tokens.hist.iter().flat_map(|f| f.iter().copied()).collect::<Vec<f32>>()), "slot": tokens.hist_slot, "age": tokens.hist_age},
        }));
    });
}

/// The note stream the moment's decision reads (MBN; `moment` first): written beside the moment, info only.
pub fn moment_notes(ns: Option<&super::model::NoteState>) {
    let Some(ns) = ns else { return };
    if !wanted() {
        return;
    }
    PLAY.with(|p| {
        let mut p = p.borrow_mut();
        let Some(play) = p.as_mut() else { return };
        if let Some(m) = play.moment.as_mut() {
            m["notes"] = json!({"stage": ns.stage, "stage_age": ns.stage_age, "engaged": ns.engaged, "waited": ns.waited, "free": ns.free,
                "change_age": ns.change_age, "reason": ns.reason, "rest": ns.pull, "rest_age": ns.pull_age});
        }
    });
}

/// The net's choice at the moment: the picked token (-1 the end) and the candidate it applied (-1 none).
pub fn choice(pick: i64, applied: i64) {
    PLAY.with(|p| {
        let mut p = p.borrow_mut();
        let Some(play) = p.as_mut() else { return };
        if let Some(mut m) = play.moment.take() {
            m["pick"] = json!(pick);
            m["applied"] = json!(applied);
            play.lines.push(m.to_string());
        }
    });
}

/// The play is over: its lines go to the file together, with its outcome.
pub fn finish(win: bool, rounds: u32) {
    let Some(mut play) = PLAY.with(|p| p.borrow_mut().take()) else { return };
    if let Some(m) = play.moment.take() {
        play.lines.push(m.to_string());
    }
    play.lines.push(json!({"type": "end", "play": play.name, "win": win, "rounds": rounds}).to_string());
    let mut out = OUT.lock().unwrap_or_else(|e| e.into_inner());
    if out.is_none() {
        let Some(path) = std::env::var_os("ARMY_OBS_DUMP") else { return };
        *out = std::fs::OpenOptions::new().create(true).append(true).open(path).ok();
    }
    if let Some(f) = out.as_mut() {
        let mut text = play.lines.join("\n");
        text.push('\n');
        let _ = f.write_all(text.as_bytes());
    }
}
