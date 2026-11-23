//! The end-turn lines of the MLX economy's parity source (`AURELIA_ECON_TURNS=1` with
//! `AURELIA_ECON_DUMP`): the city economy of each player around its end of turn.
//! A MEASUREMENT tool: it reads the world only; the result hash of a match does not move.
//!
//! Line kinds (field `t`):
//! - `econ_rules` (once per game, after `tables`): every passive City, Tile and Player effect of
//!   the nations, technologies, policies, buildings and beliefs (the compiled effect as its Rust
//!   `Debug` text), the buildings, units, specialists, technologies and eras the economy reads,
//!   the speed, the players' handicaps and the world size's research numbers;
//! - `turn` (each player's end of turn): `pre` is the world after the player's commands, before
//!   its end turn; `post` the world after it. `tiles` are the tiles whose dynamic planes changed
//!   since the game's previous line (round or turn). Each city of `pre` carries Rust's
//!   `city_breakdown` (worked tiles, specialists, base, rates) as the answer of the yields, and
//!   the terms of the subsystems not yet in MLX as inputs (city-state food, great works, trade
//!   routes, landmarks, the beliefs that reach it, its timed effects); `post` carries the same
//!   inputs as they stand after the turn (what the turn's city step saw: the happiness snapshot,
//!   the golden age, the routes).

use classic_rules::classic::Classic;
use classic_rules::ids::{PlayerId, TileId};
use classic_rules::{CompiledRuleset, GameState};
use kernel::derived::yields::YieldTerms;
use kernel::effects::module::ModuleEffects;
use kernel::rules::content::Yields;
use kernel::rules::effects::{CompiledEffect, EffectSet};
use kernel::world::effects::EffectTarget;
use serde_json::{json, Value};

type Auth<'a> = &'a kernel::world::state::AuthWorld<Classic>;
type Vocab = classic_rules::classic::effects::ClassicVocabulary;
type ClassicYields = classic_rules::classic::yields::ClassicYields;

pub const ENV: &str = "AURELIA_ECON_TURNS";

pub fn enabled() -> bool {
    std::env::var(ENV).is_ok_and(|v| !v.is_empty() && v != "0")
}

fn y6(y: &Yields) -> Value {
    json!([y.food, y.production, y.gold, y.science, y.culture, y.module])
}

fn dbg<T: std::fmt::Debug>(v: &T) -> Value {
    json!(format!("{v:?}"))
}

fn city_bucket(s: &EffectSet<Classic>) -> &[CompiledEffect<Classic>] {
    &s.passive_city
}

fn tile_bucket(s: &EffectSet<Classic>) -> &[CompiledEffect<Classic>] {
    &s.passive_tile
}

fn effect_rows(out: &mut Vec<Value>, src: &str, id: usize, set: &EffectSet<Classic>) {
    for (bucket, list) in [("city", &set.passive_city), ("tile", &set.passive_tile), ("player", &set.passive_player)] {
        for e in list.iter() {
            out.push(json!([src, id, bucket, format!("{e:?}")]));
        }
    }
    // Triggered effects (each trigger's slot): the bucket is "triggered", the effect names its trigger.
    for list in set.triggered.iter() {
        for e in list.iter() {
            out.push(json!([src, id, "triggered", format!("{e:?}")]));
        }
    }
}

/// The rules the economy reads, once per game.
pub fn rules_line(game: &GameState, rules: &CompiledRuleset) -> Value {
    let auth = game.view().auth;
    let mut effects = Vec::new();
    for (i, d) in rules.nations.iter().enumerate() {
        effect_rows(&mut effects, "nation", i, &d.effects);
    }
    for (i, d) in rules.technologies.iter().enumerate() {
        effect_rows(&mut effects, "tech", i, &d.effects);
    }
    for (i, d) in rules.module.policies.iter().enumerate() {
        effect_rows(&mut effects, "policy", i, &d.effects);
    }
    for (i, d) in rules.buildings.iter().enumerate() {
        effect_rows(&mut effects, "building", i, &d.effects);
    }
    for (i, d) in rules.module.beliefs.iter().enumerate() {
        effect_rows(&mut effects, "belief", i, &d.effects);
    }
    let era_index = |e: Option<classic_rules::rules::content::EraId>| e.and_then(|e| rules.eras.get(e.0 as usize)).map_or(-1, |d| i32::from(d.index));
    let buildings: Vec<Value> = rules
        .buildings
        .iter()
        .map(|d| {
            json!({"name": d.name, "wonder": d.wonder, "kind": dbg(&d.kind), "slots": d.specialist_slots.map(|(k, n)| json!([k.0, n])),
                   "cost": d.cost, "cpc": d.cost_per_city_pct, "era": era_index(d.era), "class": d.class, "tech": d.required_tech.map(|t| t.0),
                   "requires": d.requires_buildings.iter().map(|b| b.0).collect::<Vec<_>>(),
                   "requires_empire": d.requires_buildings_in_empire.iter().map(|b| b.0).collect::<Vec<_>>(),
                   "nation": d.nation, "replaces": d.replaces.map(|b| b.0), "unbuildable": d.unbuildable,
                   "obsolete": d.obsolete_tech.map(|t| t.0), "maintenance": d.maintenance, "hurry": d.hurry_cost_modifier,
                   "consumes": d.consumes_resources.iter().map(|(r, n)| json!([r.0, n])).collect::<Vec<_>>(),
                   "placement": dbg(&d.placement)})
        })
        .collect();
    let units: Vec<Value> = rules
        .units
        .iter()
        .map(|d| {
            json!({"name": d.name, "cost": d.cost, "founder": d.founder, "food": kernel::rules::rows::UnitExtension::built_with_food(&d.module),
                   "purchase_only": kernel::rules::rows::UnitExtension::purchase_only(&d.module), "role": dbg(&d.role), "domain": dbg(&d.domain),
                   "class": d.class, "tech": d.required_tech.map(|t| t.0), "obsolete": d.obsolete_tech.map(|t| t.0), "unbuildable": d.unbuildable,
                   "resources": d.required_resources.iter().map(|(r, n)| json!([r.0, n])).collect::<Vec<_>>(),
                   "requires": d.requires_buildings.iter().map(|b| b.0).collect::<Vec<_>>(), "nation": d.nation,
                   "replaces": d.replaces.map(|u| u.0), "era": era_index(d.era), "strength": d.strength})
        })
        .collect();
    let specialists: Vec<Value> = rules
        .specialists
        .iter()
        .map(|d| json!({"name": d.name, "yields": y6(&d.yields), "points": kernel::rules::rows::SpecialistExtension::points(&d.module)}))
        .collect();
    let techs: Vec<Value> = rules
        .technologies
        .iter()
        .map(|d| {
            json!({"name": d.name, "cost": d.cost, "requires": d.requires.iter().map(|t| t.0).collect::<Vec<_>>(),
                   "era": era_index(d.content_era), "buildings": d.unlocks_buildings.iter().map(|b| b.0).collect::<Vec<_>>(),
                   "units": d.unlocks_units.iter().map(|u| u.0).collect::<Vec<_>>(), "repeatable": d.repeatable})
        })
        .collect();
    let resources: Vec<Value> = rules
        .resources
        .iter()
        .map(|d| json!({"wonder": d.wonder_production.map(|w| json!([w.percent, era_index(w.before_era)])), "class": dbg(&d.class)}))
        .collect();
    let players = auth.players();
    let settings = auth.settings();
    let seats: Vec<Value> = (0..players.count())
        .map(|p| {
            let pid = PlayerId(p as u8);
            json!({"ai": settings.is_ai(pid), "major": players.is_major(pid), "nation": players.nation_of(pid),
                   "handicap": dbg(&settings.handicap(rules, pid))})
        })
        .collect();
    let size = classic_rules::world::size::WorldSize::of(auth.map().dimensions);
    let map = auth.map();
    let terrain: Vec<i32> = (0..map.tile_count() as u32).map(|t| map.terrain(TileId(t)).map_or(-1, |c| i32::from(c.0))).collect();
    json!({"t": "econ_rules", "effects": effects, "terrain": terrain,
           "terrain_codes": rules.terrains.iter().map(|d| d.map_code.map_or(-1, i32::from)).collect::<Vec<_>>(), "buildings": buildings, "units": units, "specialists": specialists,
           "default_specialist": rules.roles.default_specialist.map(|s| s.0), "techs": techs, "resources": resources,
           "eras": rules.eras.iter().map(|e| e.index).collect::<Vec<_>>(),
           "feature_removal": rules.features.iter().map(|f| f.removal.as_ref().map_or(-1, |r| r.production as i64)).collect::<Vec<_>>(),
           "improvement_features": rules.improvements.iter().map(|d| d.valid_features.iter().map(|f| f.0).collect::<Vec<_>>()).collect::<Vec<_>>(),
           "owner_wide": rules.effect_index.owner_wide_buildings.iter().map(|b| b.0).collect::<Vec<_>>(),
           "wonder_resources": rules.effect_index.wonder_resources.iter().map(|r| r.0).collect::<Vec<_>>(),
           "speed": dbg(&settings.speed_values()), "speed_rules": dbg(&settings.speed(rules)), "players": seats,
           "world": {"research_percent": classic_rules::classic::world_size::research_percent(size),
                     "city_unhappiness_percent": classic_rules::classic::world_size::city_count_unhappiness_percent(size),
                     "per_city": classic_rules::classic::world_size::tech_cost_per_city(size)}})
}

/// The dynamic planes of one tile: feature, resource, improvement, pillaged, owner, owning city
/// (slot index), city on it.
pub fn tile_planes(auth: Auth<'_>) -> Vec<[i32; 7]> {
    let map = auth.map();
    (0..map.tile_count() as u32)
        .map(TileId)
        .map(|t| {
            [
                map.feature(t).flatten().map_or(-1, |f| i32::from(f.0)),
                map.resource(t).flatten().map_or(-1, |r| i32::from(r.0)),
                map.improvement(t).flatten().map_or(-1, |i| i32::from(i.0)),
                i32::from(map.pillaged(t)),
                map.owner(t).flatten().map_or(-1, |p| i32::from(p.0)),
                map.owning_city(t).map_or(-1, |c| c.index as i32),
                auth.indexes().city_on_tile(t).map_or(-1, |c| c.index as i32),
            ]
        })
        .collect()
}

/// The tiles whose planes differ from `last` (all of them without one): `[tile, planes...]`.
pub fn tile_diff(last: Option<&[[i32; 7]]>, now: &[[i32; 7]]) -> Vec<Value> {
    now.iter()
        .enumerate()
        .filter(|(i, p)| last.is_none_or(|l| l.get(*i) != Some(*p)))
        .map(|(i, p)| json!([i, p[0], p[1], p[2], p[3], p[4], p[5], p[6]]))
        .collect()
}

fn techs_bits(auth: Auth<'_>, rules: &CompiledRuleset, p: PlayerId) -> String {
    let known = auth.players().techs_of(p);
    (0..rules.technologies.len()).map(|i| if known.iter().any(|t| t.0 as usize == i) { '1' } else { '0' }).collect()
}

fn effects_list(list: &[(CompiledEffect<Classic>, u32)]) -> Value {
    json!(list.iter().map(|(e, n)| json!([format!("{e:?}"), n])).collect::<Vec<_>>())
}

/// The inputs of the subsystems not yet in MLX for one city, as the world stands.
fn city_inputs(auth: Auth<'_>, rules: &CompiledRuleset, core: &kernel::world::city::CityCore, handle: kernel::ids::CityHandle) -> Value {
    let mods = kernel::effects::city::city_modifiers(auth, rules, core);
    let base = ClassicYields::city_base(auth, rules, core, &mods);
    let after = ClassicYields::city_after(auth, handle);
    let gw = classic_rules::derived::tourism::great_work_culture(auth, rules, handle, core, &mods);
    let mut beliefs_city = Vec::new();
    Vocab::visit_city_sources(auth, rules, core, city_bucket, &mut |e, n| beliefs_city.push((*e, n)));
    let mut beliefs_tile = Vec::new();
    Vocab::visit_city_sources(auth, rules, core, tile_bucket, &mut |e, n| beliefs_tile.push((*e, n)));
    let mut timed = Vec::new();
    for entry in auth.active_effects().iter() {
        if entry.target == EffectTarget::City(handle) {
            if let Some(e) = entry.effect.resolve(rules) {
                timed.push((*e, 1));
            }
        }
    }
    let map = auth.map();
    let landmarks: Vec<Value> = kernel::math::hex_disc(map.dimensions, core.tile, kernel::world::city::CITY_WORK_RADIUS)
        .filter_map(|t| {
            let c = ClassicYields::tile_base(auth, rules, core, t).culture;
            (c != 0).then(|| json!([t.0, c]))
        })
        .collect();
    let wonder_res: Vec<u16> = Vocab::city_resources(auth, rules, handle)
        .into_iter()
        .filter(|r| rules.effect_index.wonder_resources.contains(r))
        .map(|r| r.0)
        .collect();
    use classic_rules::classic::effects::ClassicCount as CC;
    let count = |c: CC| Vocab::count(auth, rules, kernel::effects::count::Counted::City(core), c);
    let counts = json!({"MajorityFollowers": count(CC::MajorityFollowers), "ReligionsPresent": count(CC::ReligionsPresent),
                        "TradePartners": count(CC::TradePartners), "CityStateRoutes": count(CC::CityStateRoutes)});
    json!({"base": y6(&base), "after": y6(&after), "great_works": gw, "counts": counts, "beliefs_city": effects_list(&beliefs_city),
           "beliefs_tile": effects_list(&beliefs_tile), "timed": effects_list(&timed), "landmarks": landmarks,
           "wonder_res": wonder_res, "connected": core.capital_connection == kernel::world::city::CapitalConnection::Connected,
           "garrison": kernel::world::city::garrison(auth, rules, core).is_some()})
}

/// One player's cities and empire.
pub fn player_state(auth: Auth<'_>, rules: &CompiledRuleset, p: PlayerId, answers: bool) -> Value {
    let players = auth.players();
    let map = auth.map();
    let mut cities = Vec::new();
    for &h in auth.indexes().cities_of(p) {
        let Some(core) = auth.cities().get(h) else { continue };
        let mut c = json!({
            "h": h.index, "tile": core.tile.0, "pop": core.population, "food": core.food_times100, "kept": core.economy.food_kept,
            "prod": core.production, "queue": core.queue.iter().map(|q| format!("{q:?}")).collect::<Vec<_>>(),
            "process": dbg(&core.economy.process), "buildings": core.buildings.iter().map(|b| b.0).collect::<Vec<_>>(),
            "focus": dbg(&core.citizens.focus), "avoid": core.citizens.avoid_growth,
            "locked": core.citizens.locked.iter().map(|t| t.0).collect::<Vec<_>>(),
            "specs": core.citizens.specialists.iter().map(|(k, n)| json!([k.0, n])).collect::<Vec<_>>(),
            "unworked": core.unworked.iter().map(|t| t.0).collect::<Vec<_>>(), "resist": core.resistance_turns,
            "capital": kernel::world::city::is_capital(rules, core), "coastal": map.coastal_land(core.tile), "river": map.river(core.tile),
            "culture": core.culture, "acquired": core.acquired, "radius": core.radius, "status": dbg(&core.status),
            "inputs": city_inputs(auth, rules, core, h),
            "blocked": core.queue.iter().map(|i| <classic_rules::classic::systems::ClassicSystems as kernel::systems::module::ModuleSystems<Classic>>::queue_item_blocked(auth, rules, p, *i)).collect::<Vec<_>>(),
        });
        if answers {
            let b = kernel::derived::yields::city_breakdown(auth, rules, core);
            let m = &b.modifiers;
            let totals: Vec<Value> = kernel::derived::yields::channels::<Classic>()
                .map(|k| {
                    let t = kernel::derived::yields::channel_totals(m, k);
                    json!([t.flat, t.percent, format!("{t:?}")])
                })
                .collect();
            c["rust"] = json!({
                "worked": b.work.worked().iter().map(|(t, y)| json!([t.0, y6(y)])).collect::<Vec<_>>(),
                "specialists": b.work.specialists.iter().map(|n| *n as u32).collect::<Vec<_>>(),
                "tile_yields": y6(&b.work.tile_yields), "specialist_yields": y6(&b.work.specialist_yields),
                "base": y6(&b.base), "rates": y6(&b.rates), "growth_percent": m.growth_percent,
                "item_percent": m.item_production_percent, "totals": totals, "cost_percent": m.cost_percent.to_vec(),
                "stats": format!("{:?}", m.stats), "handicap": dbg(&m.handicap_cost), "owner_cities": m.owner_cities,
                "head_cost": core.queue.first().and_then(|i| m.production_cost(rules, *i)),
                "happiness": m.module.happiness,
                "unhappiness": classic_rules::derived::happiness::city_unhappiness(auth, rules, core),
            });
        }
        cities.push(c);
    }
    let empire = ClassicYields::empire(auth, p);
    let mut founder = Vec::new();
    Vocab::visit_player_sources(auth, rules, p, &mut |e, n| founder.push((*e, n)));
    let mut timed = Vec::new();
    for entry in auth.active_effects().iter() {
        if entry.target == EffectTarget::Player(p) {
            if let Some(e) = entry.effect.resolve(rules) {
                timed.push((*e, 1));
            }
        }
    }
    let n = players.count();
    let met: Vec<bool> = (0..n).map(|o| classic_rules::derived::tourism::met(auth, p, PlayerId(o as u8))).collect();
    let alive: Vec<bool> = (0..n).map(|o| players.is_alive(PlayerId(o as u8))).collect();
    let war: Vec<bool> = (0..n).map(|o| o != p.0 as usize && auth.diplomacy().at_war(p, PlayerId(o as u8))).collect();
    let mut units: std::collections::BTreeMap<u16, u32> = std::collections::BTreeMap::new();
    for &u in auth.indexes().units_of(p) {
        if let Some(v) = auth.units().get(u) {
            *units.entry(v.core.kind.0).or_default() += 1;
        }
    }
    let mut s = json!({
        "techs": techs_bits(auth, rules, p), "science": players.science_of(p), "treasury": players.treasury_of(p),
        "queue": players.research_queue_of(p).iter().map(|t| t.0).collect::<Vec<_>>(),
        "policies": auth.module().culture().of(p).policies.iter().map(|x| x.0).collect::<Vec<_>>(),
        "golden_age": empire.golden_age, "unhappy_percent": empire.unhappiness_percent,
        "excess": classic_rules::derived::happiness::excess_snapshot(auth, p),
        "era": kernel::world::player::player_era(auth, rules, p), "effective_cities": classic_rules::derived::culture::effective_cities(auth, p),
        "founder": effects_list(&founder), "timed": effects_list(&timed), "met": met, "alive": alive, "war": war,
        "units": units.iter().map(|(k, n)| json!([k, n])).collect::<Vec<_>>(), "cities": cities,
        "player_counts": {
            "Policies": Vocab::count(auth, rules, kernel::effects::count::Counted::Player(p), classic_rules::classic::effects::ClassicCount::Policies),
            "PeacefulForeignFollowers": Vocab::count(auth, rules, kernel::effects::count::Counted::Player(p), classic_rules::classic::effects::ClassicCount::PeacefulForeignFollowers),
            "TradePartners": Vocab::count(auth, rules, kernel::effects::count::Counted::Player(p), classic_rules::classic::effects::ClassicCount::TradePartners),
            "CityStateRoutes": Vocab::count(auth, rules, kernel::effects::count::Counted::Player(p), classic_rules::classic::effects::ClassicCount::CityStateRoutes),
            "ConnectedCities": kernel::world::city::capital_of(auth, rules, p).map_or(0, |cap| kernel::derived::connections::connected_cities(auth, rules, p, cap).len().saturating_sub(1)),
        },
        "unlocked": unlocked(auth, rules, p),
        "research_agreements": (0..n).filter_map(|o| {
            let rel = auth.module().relations();
            let other = PlayerId(o as u8);
            let until = rel.research_agreement_until(p, other);
            (until > 0).then(|| json!([o, until, rel.research_points(p, other), rel.research_points(other, p)]))
        }).collect::<Vec<_>>(),
        "ai": auth.settings().is_ai(p), "handicap": dbg(&auth.settings().handicap(rules, p)),
        "work_rate": kernel::world::builds::work_rate(auth, rules, p),
        "jobs": auth.indexes().units_of(p).iter().filter_map(|u| auth.units().get(*u)).filter_map(|v| v.core.work.map(|w| {
            let site = match w.target {
                kernel::ids::WorkTarget::Improvement(i) => kernel::world::builds::improvement_site(auth, rules, p, w.tile, i).is_ok(),
                _ => true,
            };
            json!([w.tile.0, format!("{:?}", w.target), w.work_left, site])
        })).collect::<Vec<_>>(),
        "all_techs": (0..n).map(|o| techs_bits(auth, rules, PlayerId(o as u8))).collect::<Vec<_>>(),
        "scholars": auth.module().congress().is_active(classic_rules::world::congress::ResolutionKind::ScholarsInResidence),
        "allied_science": classic_rules::derived::income::allied_city_state_science(auth, p),
        "wonders_built": auth.cities().iter().flat_map(|(_, c)| c.buildings.iter().copied()).filter(|b| rules.buildings.get(b.0 as usize).is_some_and(|d| d.wonder)).map(|b| b.0).collect::<std::collections::BTreeSet<_>>(),
        "economy": players.economy_of(p).map(|e| json!({"ga_turns": e.golden_age_turns, "ga_meter": e.golden_age_meter, "ga_count": e.golden_ages,
                                                         "excess": e.excess_happiness})),
    });
    if answers {
        let pm = kernel::effects::player::player_modifiers(auth, rules, p);
        let sci: i32 = auth
            .indexes()
            .cities_of(p)
            .iter()
            .filter_map(|h| auth.cities().get(*h))
            .map(|c| kernel::derived::yields::city_breakdown(auth, rules, c).rates.science)
            .sum();
        s["rust"] = json!({
            "player_yields": pm.yields.iter().map(|t| format!("{t:?}")).collect::<Vec<_>>(), "tech_cost": dbg(&pm.tech_cost),
            "stats": format!("{:?}", pm.stats),
            "cities_science": sci,
            "science_of": <classic_rules::classic::systems::ClassicSystems as kernel::systems::module::ModuleSystems<Classic>>::science(auth, rules, p, sci),
            "head_cost": players.research_queue_of(p).first().map(|t| classic_rules::derived::research::technology_cost(auth, rules, p, *t)),
            "happiness": format!("{:?}", classic_rules::derived::happiness::player_happiness(auth, rules, p)),
            "excess_now": classic_rules::derived::happiness::player_happiness(auth, rules, p).excess(),
            "ga_threshold": classic_rules::derived::happiness::golden_age_threshold(auth, rules, p),
            "ga_length": classic_rules::derived::happiness::golden_age_length(auth, rules, p, classic_rules::derived::happiness::UNMODIFIED_GOLDEN_AGE_TURNS),
        });
    }
    s
}

/// The items the player's technologies and nation allow (the kernel's gates before any city rule:
/// `production_gate`, a regular unit its nation's unique one replaces, the required technology):
/// one character per unit kind, then per building kind.
fn unlocked(auth: Auth<'_>, rules: &CompiledRuleset, p: PlayerId) -> String {
    let nation = auth.players().nation_of(p);
    let known = auth.players().techs_of(p);
    let tech_ok = |t: Option<classic_rules::ids::TechId>| t.is_none_or(|t| known.contains(&t));
    let units = rules.units.iter().enumerate().map(|(i, d)| {
        let kind = classic_rules::ids::UnitKindId(i as u16);
        rules.production_gate(kernel::ids::ProductionItem::Unit(kind), nation).is_none() && !rules.replaced_for(nation, kind) && tech_ok(d.required_tech)
    });
    let buildings = rules.buildings.iter().enumerate().map(|(i, d)| {
        let kind = classic_rules::ids::BuildingKindId(i as u16);
        rules.production_gate(kernel::ids::ProductionItem::Building(kind), nation).is_none() && tech_ok(d.required_tech)
    });
    units.chain(buildings).map(|ok| if ok { '1' } else { '0' }).collect()
}
