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
                   "placement": dbg(&d.placement), "water_routes": d.placement.allows_water_routes})
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
                   "replaces": d.replaces.map(|u| u.0), "era": era_index(d.era), "strength": d.strength,
                   "no_maintenance": d.module.economy.no_maintenance})
        })
        .collect();
    let specialists: Vec<Value> = rules
        .specialists
        .iter()
        .map(|d| json!({"name": d.name, "yields": y6(&d.yields), "points": kernel::rules::rows::SpecialistExtension::points(&d.module),
                        "gpp": d.module.great_person_points}))
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
        .map(|d| json!({"wonder": d.wonder_production.map(|w| json!([w.percent, era_index(w.before_era)])), "class": dbg(&d.class),
                        "tech": d.required_tech.map(|t| t.0), "improvements": d.improvements.iter().map(|i| i.0).collect::<Vec<_>>()}))
        .collect();
    let players = auth.players();
    let settings = auth.settings();
    let seats: Vec<Value> = (0..players.count())
        .map(|p| {
            let pid = PlayerId(p as u8);
            json!({"ai": settings.is_ai(pid), "major": players.is_major(pid), "nation": players.nation_of(pid),
                   "handicap": dbg(&settings.handicap(rules, pid)),
                   "road_features": kernel::navigation::profile::own_land_road_features(auth, rules, pid).iter().map(|f| f.0).collect::<Vec<_>>()})
        })
        .collect();
    let size = classic_rules::world::size::WorldSize::of(auth.map().dimensions);
    let map = auth.map();
    let terrain: Vec<i32> = (0..map.tile_count() as u32).map(|t| map.terrain(TileId(t)).map_or(-1, |c| i32::from(c.0))).collect();
    let policies: Vec<Value> = rules
        .module
        .policies
        .iter()
        .map(|d| json!({"name": d.name, "branch": d.branch.map(|b| b.0), "role": dbg(&d.role), "requires": d.prerequisites.iter().map(|q| q.0).collect::<Vec<_>>()}))
        .collect();
    let branches: Vec<Value> = rules
        .module
        .policy_branches
        .iter()
        .map(|d| {
            json!({"name": d.name, "kind": dbg(&d.kind), "era": era_index(d.era_prerequisite), "opener": d.opener.map(|q| q.0),
                   "finisher": d.finisher.map(|q| q.0), "exclusive": d.mutually_exclusive_with.iter().map(|b| b.0).collect::<Vec<_>>(),
                   "first_free": d.first_adopter_free_policies, "second_free": d.second_adopter_free_policies})
        })
        .collect();
    let great_people: Vec<Value> = rules
        .module
        .great_people
        .iter()
        .map(|d| json!({"kind": dbg(&d.kind), "kind_index": d.kind.index(), "unit": d.unit.0, "specialist": d.specialist.map(|s| s.0), "source": dbg(&d.points_source)}))
        .collect();
    json!({"t": "econ_rules", "effects": effects, "terrain": terrain, "policies": policies, "branches": branches, "great_people": great_people,
           "max_rounds": settings.speed(rules).max_rounds(), "policy_per_city": classic_rules::classic::world_size::policy_cost_per_city(size),
           "heritage_per_wonder": classic_rules::world::congress::HERITAGE_CULTURE_PER_WONDER, "boost_percent": classic_rules::world::congress::BOOST_PERCENT,
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
/// (slot index), city on it, the resource's amount, the route (0 none, 1 road, 2 railroad).
pub fn tile_planes(auth: Auth<'_>) -> Vec<[i32; 9]> {
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
                i32::from(map.resource_amount(t)),
                match map.route(t) {
                    kernel::world::map::Route::None => 0,
                    kernel::world::map::Route::Road => 1,
                    kernel::world::map::Route::Railroad => 2,
                },
            ]
        })
        .collect()
}

/// The tiles whose planes differ from `last` (all of them without one): `[tile, planes...]`.
pub fn tile_diff(last: Option<&[[i32; 9]]>, now: &[[i32; 9]]) -> Vec<Value> {
    now.iter()
        .enumerate()
        .filter(|(i, p)| last.is_none_or(|l| l.get(*i) != Some(*p)))
        .map(|(i, p)| json!([i, p[0], p[1], p[2], p[3], p[4], p[5], p[6], p[7], p[8]]))
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
    // The great person rate of the player as the city step reads it (`accumulate_great_people`):
    // its stats, the friends' rate and the World Congress funding.
    let gp_friends = classic_rules::derived::great_people::friendship_rate_with(auth, p, |q| {
        classic_rules::derived::great_people::friend_stat_of(&kernel::effects::player::player_modifiers(auth, rules, q))
    });
    let gp_funding: Vec<i32> = rules.module.great_people.iter().map(|d| classic_rules::world::congress::funding_percent(auth, d.kind)).collect();
    let mut gp_rate = kernel::effects::player::player_modifiers(auth, rules, p).stats.clone();
    {
        use classic_rules::rules::stats::{ClassicStat, Stat};
        gp_rate.add(Stat::Module(ClassicStat::GreatPeopleRate), gp_friends);
        let mut seen = Vec::new();
        for (d, shift) in rules.module.great_people.iter().zip(&gp_funding) {
            if *shift != 0 && !seen.contains(&d.kind) {
                seen.push(d.kind);
                gp_rate.add(Stat::Module(ClassicStat::GreatPersonRate(d.kind)), *shift);
            }
        }
    }
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
            "gpp": auth.cities().module_of(h).map(|m| m.great_person_points.to_vec()).unwrap_or_default(),
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
                "gp_points": classic_rules::derived::great_people::city_points(rules, &b, &gp_rate).to_vec(),
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
                                                         "excess": e.excess_happiness, "gp_created": e.great_people_created,
                                                         "writers": e.great_writers_created, "artists": e.great_artists_created,
                                                         "musicians": e.great_musicians_created, "science_history": e.science_history.to_vec()})),
    });
    s["resources"] = json!(resource_lines(auth, rules, p));
    let u = classic_rules::derived::maintenance::upkeep(auth, rules, p);
    s["upkeep"] = json!([u.units, u.buildings, u.routes, u.connections]);
    s["unit_weights"] = json!(unit_weights(auth, rules, p));
    s["army_tax"] = json!(auth.module().congress().is_active(classic_rules::world::congress::ResolutionKind::StandingArmyTax));
    s["heritage"] = json!(auth.module().congress().is_active(classic_rules::world::congress::ResolutionKind::CulturalHeritageSites));
    s["gold_deals"] = json!(auth.module().relations().trades().iter().filter(|t| t.from == p || t.to == p).filter_map(|t| match t.flow {
            classic_rules::world::relations::TradeFlow::GoldPerTurn(g) => Some(json!([t.from.0, t.to.0, g, t.until])),
            _ => None,
        }).collect::<Vec<_>>());
    let c = auth.module().culture().of(p);
    s["culture"] = json!({"pool": c.culture, "lifetime": c.lifetime, "recent": c.recent, "branches": c.branches.iter().map(|b| b.0).collect::<Vec<_>>(),
                   "ideology": c.ideology.map(|b| b.0), "free_policies": c.free_policies, "free_tenets": c.free_tenets, "free_used": c.free_used,
                   "max_cities": c.max_cities, "free_buildings": c.free_buildings.iter().map(|(b, n)| json!([b.0, n])).collect::<Vec<_>>(),
                   "policy_cost": classic_rules::derived::culture::next_policy_cost(auth, rules, p),
                   "cs": classic_rules::world::city_states::player_bonus(auth, rules, p, classic_rules::rules::content::CityStateBonusKind::Culture),
                   "boost_turns": auth.module().congress().rewards_of(p).culture_boost_turns});
    s["gp"] = json!({"threshold": rules.module.great_people.iter().map(|d| classic_rules::derived::great_people::great_person_threshold(auth, rules, p, d.kind)).collect::<Vec<_>>(),
                     "friends": gp_friends, "funding": gp_funding});
    s["income_gold"] = json!(classic_rules::derived::income::player_income(auth, rules, p).gold);
    s["round"] = json!(auth.turn().round);
    // The cities a route network (or two harbors on one sea) links to the capital, computed on this
    // world (`derived::connections`): the connection gold's answer.
    s["reached"] = json!(kernel::world::city::capital_of(auth, rules, p)
        .map(|cap| kernel::derived::connections::compute_connected_cities(auth, rules, p, cap).iter().map(|c| c.index).collect::<Vec<_>>())
        .unwrap_or_default());
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

/// The player's resource ledger (`derived::resources::resource_ledger`): per resource with a total
/// or a use, `[resource, total, used, untradeable, imports, from allied city-states, used beyond the
/// own units and buildings]` — the last four are the parts MLX does not derive (Feitoria copies, deals,
/// city-state alliances).
fn resource_lines(auth: Auth<'_>, rules: &CompiledRuleset, p: PlayerId) -> Vec<Value> {
    use classic_rules::world::player::ClassicPlayers;
    let ledger = classic_rules::derived::resources::resource_ledger(auth, rules, p);
    let n = rules.resources.len();
    let mut own = vec![0i32; n];
    for &u in auth.indexes().units_of(p) {
        if let Some(d) = auth.units().get(u).and_then(|v| rules.units.get(v.core.kind.0 as usize)) {
            for (r, k) in &d.required_resources {
                own[r.0 as usize] += i32::from(*k);
            }
        }
    }
    for &h in auth.indexes().cities_of(p) {
        if let Some(c) = auth.cities().get(h) {
            for b in &c.buildings {
                if let Some(d) = rules.buildings.get(b.0 as usize) {
                    for (r, k) in &d.consumes_resources {
                        own[r.0 as usize] += i32::from(*k);
                    }
                }
            }
        }
    }
    let round = auth.turn().round;
    let mut imports = vec![0i32; n];
    for t in auth.module().relations().trades() {
        if let classic_rules::world::relations::TradeFlow::Resource { resource, amount } = t.flow {
            if round < t.until && t.to == p {
                imports[resource.0 as usize] += i32::from(amount);
            }
        }
    }
    let from_cs: Vec<u16> = if auth.players().is_city_state(p) {
        vec![0; n]
    } else {
        classic_rules::derived::resources::from_city_states(auth, rules, p)
    };
    (0..n)
        .filter_map(|i| {
            let r = classic_rules::ids::ResourceId(i as u16);
            let (total, used) = (ledger.total(r), ledger.used(r));
            (total != 0 || used != 0).then(|| {
                json!([i, total, used, ledger.available(r) - ledger.tradeable(r), imports[i], from_cs.get(i).copied().unwrap_or(0), used - own[i]])
            })
        })
        .collect()
}

/// The maintenance weight (hundredths of a unit, `maintenance::unit_weights_times100`) of each unit
/// kind the player has: 0 with NoMaintenance, else 100 + its MaintenancePercent.
fn unit_weights(auth: Auth<'_>, rules: &CompiledRuleset, p: PlayerId) -> Vec<Value> {
    use classic_rules::classic::effects::ClassicUnitStat;
    use kernel::rules::effects::UnitStat;
    let owner_part = kernel::effects::unit::owner_unit_modifiers(auth, rules, p);
    let mut kinds: Vec<u16> = auth.indexes().units_of(p).iter().filter_map(|u| auth.units().get(*u)).map(|v| v.core.kind.0).collect();
    kinds.sort_unstable();
    kinds.dedup();
    kinds
        .into_iter()
        .map(|k| {
            let kind = classic_rules::ids::UnitKindId(k);
            let free = rules.units.get(k as usize).is_some_and(|d| d.module.economy.no_maintenance);
            let pct = kernel::effects::unit::owner_stat_for_kind(auth, rules, &owner_part, p, kind, UnitStat::Module(ClassicUnitStat::MaintenancePercent));
            json!([k, if free { 0 } else { (100 + pct).max(0) }])
        })
        .collect()
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
