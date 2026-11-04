//! A major's empire in a frame, and its decisions over the game: research,
//! production per city, policies and ideology, pantheon, religion and
//! beliefs, great people, trade routes, yields a turn. Each frame keeps a
//! snapshot (`Emp`); the decisions are the differences of consecutive
//! snapshots (a technology learned, a building finished in a city, a branch
//! opened…), dated by the frame's round. Names come from the rules, in
//! Russian where the content bundle has them.

use std::collections::{BTreeMap, BTreeSet};

use classic_rules::classic::content::PolicyRole;
use classic_rules::{CompiledRuleset, GameState};
use kernel::ids::{PlayerId, ProductionItem};
use kernel::world::city_economy::Process;
use serde_json::{json, Value};

use super::{Names, Unit};
use crate::bench::modules::{standing, ClassicBench};

/// One city of the empire: id, name, tile, population, buildings, the item in production
/// (none: the process its hammers go to, if any).
#[derive(Debug, Clone)]
pub(super) struct EmpCity {
    id: u64,
    name: String,
    tile: u32,
    pop: u32,
    buildings: Vec<usize>,
    item: Option<ProductionItem>,
    process: Option<Process>,
}

/// A city's production when its owner ended a turn: what it left the city building.
#[derive(Debug, Clone, Copy)]
pub(super) struct End {
    pub(super) owner: PlayerId,
    pub(super) item: Option<ProductionItem>,
    pub(super) process: Option<Process>,
}

/// What a major has at a frame.
#[derive(Debug, Clone)]
pub(super) struct Emp {
    techs: Vec<usize>,
    research: Option<usize>,
    policies: Vec<usize>,
    branches: Vec<usize>,
    ideology: Option<usize>,
    pantheon: Option<usize>,
    religion: Option<usize>,
    beliefs: Vec<usize>,
    cities: Vec<EmpCity>,
    routes: Vec<(u64, u64)>,
    yields: Value,
}

/// Every player's empire (None: not a major).
/// A city's item and process are those of its owner's last end of turn (`ends`; a
/// captured city's once its new owner has ended one), else its queue now: a frame shows
/// what the owner left the city building, not the empty queue between a completion in
/// the turn's production and the owner's next order.
pub(super) fn snapshot(game: &GameState, rules: &CompiledRuleset, ends: &std::collections::HashMap<u64, End>) -> Vec<Option<Emp>> {
    let auth = game.view().auth;
    let players = auth.players();
    let module = auth.module();
    let city_id = |h| auth.cities().get(h).map_or(0, |c| c.id.0);
    (0..players.count() as u8)
        .map(PlayerId)
        .map(|p| {
            if !players.is_major(p) {
                return None;
            }
            let culture = module.culture().of(p);
            let faith = module.religion().player(p);
            let religion = faith.religion.and_then(|r| module.religion().religion(r));
            let s = standing::<ClassicBench>(game, rules, p);
            let e = &s.economy;
            let mut cities: Vec<EmpCity> = auth
                .cities()
                .iter()
                .filter(|(_, c)| c.owner == p)
                .map(|(_, c)| EmpCity {
                    id: c.id.0,
                    name: c.name.clone(),
                    tile: c.tile.0,
                    pop: c.population,
                    buildings: c.buildings.iter().map(|b| b.0 as usize).collect(),
                    item: ends.get(&c.id.0).filter(|e| e.owner == p).map_or_else(|| c.queue.first().copied(), |e| e.item),
                    process: ends.get(&c.id.0).filter(|e| e.owner == p).map_or(c.economy.process, |e| e.process),
                })
                .collect();
            cities.sort_by_key(|c| c.id);
            Some(Emp {
                techs: players.techs_of(p).iter().map(|t| t.0 as usize).collect(),
                research: players.research_queue_of(p).first().map(|t| t.0 as usize),
                policies: culture.policies.iter().map(|x| x.0 as usize).collect(),
                branches: culture.branches.iter().map(|x| x.0 as usize).collect(),
                ideology: culture.ideology.map(|x| x.0 as usize),
                pantheon: faith.pantheon.map(|x| x.0 as usize),
                religion: faith.religion.map(|x| x.0 as usize),
                beliefs: religion.map_or_else(Vec::new, |r| r.beliefs.iter().map(|b| b.0 as usize).collect()),
                cities,
                routes: module.trade().owned_by(p).map(|r| (city_id(r.origin), city_id(r.destination))).collect(),
                yields: json!({
                    "gold": e.gold, "science": e.science, "culture": e.culture, "faith": e.faith, "production": e.production,
                    "treasury": e.treasury, "happiness": e.happiness, "wonders": e.wonders,
                    "cities": s.cities, "pop": s.population, "techs": s.techs, "military": s.military, "score": s.score,
                }),
            })
        })
        .collect()
}

/// The Russian names of what the empire reads.
pub(super) struct Words<'a> {
    rules: &'a CompiledRuleset,
    names: &'a Names,
    great: BTreeSet<usize>,
}

impl<'a> Words<'a> {
    pub(super) fn new(rules: &'a CompiledRuleset, names: &'a Names) -> Self {
        Words { rules, names, great: rules.module.great_people.iter().map(|g| g.unit.0 as usize).collect() }
    }
    fn tech(&self, i: usize) -> String {
        self.rules.technologies.get(i).map_or_else(|| format!("#{i}"), |d| self.names.get("tech", &d.key, &d.name))
    }
    fn policy(&self, i: usize) -> String {
        self.rules.module.policies.get(i).map_or_else(|| format!("#{i}"), |d| self.names.get("policy", &d.key, &d.name))
    }
    fn branch(&self, i: usize) -> String {
        self.rules.module.policy_branches.get(i).map_or_else(|| format!("#{i}"), |d| self.names.get("policy", &d.key, &d.name))
    }
    fn belief(&self, i: usize) -> String {
        self.rules.module.beliefs.get(i).map_or_else(|| format!("#{i}"), |d| self.names.get("belief", &d.key, &d.name))
    }
    fn religion(&self, i: usize) -> String {
        self.rules.module.religions.get(i).map_or_else(|| format!("#{i}"), |d| self.names.get("religion", &d.key, &d.name))
    }
    fn building(&self, i: usize) -> (String, bool) {
        self.rules.buildings.get(i).map_or_else(|| (format!("#{i}"), false), |d| (self.names.get("building", &d.key, &d.name), d.wonder))
    }
    fn unit(&self, i: usize) -> String {
        self.rules.units.get(i).map_or_else(|| format!("#{i}"), |d| self.names.get("unit", &d.key, &d.name))
    }
    fn item(&self, item: ProductionItem) -> String {
        match item {
            ProductionItem::Unit(k) => self.unit(k.0 as usize),
            ProductionItem::Building(b) => self.building(b.0 as usize).0,
            ProductionItem::Project(p) => self.rules.module.projects.get(p.0 as usize).map_or_else(|| "?".to_owned(), |d| self.names.get("project", &d.key, &d.name)),
        }
    }
    fn process(&self, process: Process) -> String {
        match process {
            Process::Wealth => self.names.0.get("classic.game.city.process-wealth").cloned().unwrap_or_else(|| "Wealth".to_owned()),
            Process::Research => self.names.0.get("classic.game.city.process-research").cloned().unwrap_or_else(|| "Research".to_owned()),
            Process::Contribute(p) => self.item(ProductionItem::Project(p)),
        }
    }
    fn opener(&self, i: usize) -> bool {
        self.rules.module.policies.get(i).is_some_and(|d| matches!(d.role, PolicyRole::Opener))
    }
}

/// The empire panel of a frame: per major, its yields a turn, research, policies, faith, routes and
/// what each city builds.
pub(super) fn status(emps: &[Option<Emp>], w: &Words) -> Value {
    let mut out = serde_json::Map::new();
    for (p, emp) in emps.iter().enumerate() {
        let Some(e) = emp else { continue };
        let build: Vec<Value> = e.cities.iter().map(|c| json!([c.name, c.pop, c.item.map(|i| w.item(i)).or_else(|| c.process.map(|p| w.process(p)))])).collect();
        out.insert(
            p.to_string(),
            json!({
                "yields": e.yields,
                "research": e.research.map(|t| w.tech(t)),
                "policies": e.policies.iter().filter(|&&x| !w.opener(x)).count(),
                "branches": e.branches.iter().map(|&b| w.branch(b)).collect::<Vec<_>>(),
                "ideology": e.ideology.map(|b| w.branch(b)),
                "pantheon": e.pantheon.map(|b| w.belief(b)),
                "religion": e.religion.map(|r| w.religion(r)),
                "routes": e.routes.len(),
                "build": build,
            }),
        );
    }
    Value::Object(out)
}

/// A ruin's reward in words (`sites::events`).
pub(super) fn ruins_reward(e: &Value) -> String {
    let n = e["amount"].as_u64().unwrap_or(0);
    match e["reward"].as_str().unwrap_or("") {
        "citizen" => format!("+{n} жит. в городе {}", e["city"].as_str().unwrap_or("?")),
        "culture" => format!("+{n} культуры"),
        "faith" => format!("+{n} веры"),
        "gold" => format!("+{n} золота"),
        "experience" => format!("+{n} опыта отряду"),
        _ => "карта окрестностей".to_owned(),
    }
}

/// One decision: the frame it shows in, its round, its kind, what, where.
fn decision(f: usize, round: u32, kind: &str, name: String, city: Option<&str>) -> Value {
    match city {
        Some(c) => json!({ "f": f, "r": round, "k": kind, "n": name, "c": c }),
        None => json!({ "f": f, "r": round, "k": kind, "n": name }),
    }
}

/// What `decisions` reads of a frame: its round, its empires, its units, its events.
pub(super) type FrameView<'a> = (u32, &'a [Option<Emp>], &'a [Unit], &'a [Value]);

/// Every major's decisions, frame by frame: what changed in its empire since the frame before.
/// `frames`: per frame its round, its empires, its units and its events (camps cleared and ruins
/// explored are read from those).
pub(super) fn decisions(frames: &[FrameView], w: &Words) -> Value {
    let mut out: BTreeMap<usize, Vec<Value>> = BTreeMap::new();
    for f in 1..frames.len() {
        let (round, now, units, events) = frames[f];
        let (_, before, units_before, _) = frames[f - 1];
        for e in events {
            let Some(p) = e["p"].as_u64().map(|p| p as usize) else { continue };
            let text = match e["t"].as_str() {
                Some("camp_cleared") => match e["gold"].as_u64().filter(|g| *g > 0) {
                    Some(g) => format!("лагерь варваров зачищен, +{g} золота"),
                    None => "лагерь варваров зачищен".to_owned(),
                },
                Some("ruins") => format!("руины: {}", ruins_reward(e)),
                _ => continue,
            };
            let kind = if e["t"] == "ruins" { "ruins" } else { "camp" };
            out.entry(p).or_default().push(json!({ "f": f, "r": e["r"].as_u64().unwrap_or(u64::from(round)), "k": kind, "n": text }));
        }
        let had: BTreeSet<u64> = units_before.iter().map(|u| u.id).collect();
        let all_before: BTreeMap<u64, &EmpCity> = before.iter().flatten().flat_map(|e| e.cities.iter().map(|c| (c.id, c))).collect();
        for (p, emp) in now.iter().enumerate() {
            let (Some(e), Some(b)) = (emp, before.get(p).and_then(Option::as_ref)) else { continue };
            let list = out.entry(p).or_default();
            let mut push = |kind: &str, name: String, city: Option<&str>| list.push(decision(f, round, kind, name, city));
            for c in &e.cities {
                if !b.cities.iter().any(|x| x.id == c.id) {
                    push(if all_before.contains_key(&c.id) { "captured" } else { "founded" }, c.name.clone(), None);
                }
            }
            for c in &b.cities {
                if !e.cities.iter().any(|x| x.id == c.id) {
                    push("lost", c.name.clone(), None);
                }
            }
            for &t in e.techs.iter().filter(|t| !b.techs.contains(t)) {
                push("tech", w.tech(t), None);
            }
            for &x in e.branches.iter().filter(|x| !b.branches.contains(x)) {
                push("branch", w.branch(x), None);
            }
            if e.ideology != b.ideology {
                if let Some(x) = e.ideology {
                    push("ideology", w.branch(x), None);
                }
            }
            for &x in e.policies.iter().filter(|x| !b.policies.contains(x) && !w.opener(**x)) {
                let branch = w.rules.module.policies.get(x).and_then(|d| d.branch).map(|br| w.branch(br.0 as usize));
                push("policy", match branch {
                    Some(br) => format!("{br}: {}", w.policy(x)),
                    None => w.policy(x),
                }, None);
            }
            if e.pantheon != b.pantheon {
                if let Some(x) = e.pantheon {
                    push("pantheon", w.belief(x), None);
                }
            }
            if e.religion != b.religion {
                if let Some(x) = e.religion {
                    push("religion", w.religion(x), None);
                }
            }
            for &x in e.beliefs.iter().filter(|x| !b.beliefs.contains(x) && Some(**x) != e.pantheon) {
                push("belief", w.belief(x), None);
            }
            for c in &e.cities {
                // A city held in both frames: its new buildings (a captured city's are not this player's work).
                let Some(old) = b.cities.iter().find(|x| x.id == c.id) else { continue };
                for &x in c.buildings.iter().filter(|x| !old.buildings.contains(x)) {
                    let (name, wonder) = w.building(x);
                    push(if wonder { "wonder" } else { "building" }, name, Some(&c.name));
                }
                // A project finished: the item left the queue's head and the city did not switch from it by choice is
                // not knowable; projects show as the city's current item instead.
            }
            for u in units.iter().filter(|u| u.owner as usize == p && !had.contains(&u.id)) {
                let kind = u.kind as usize;
                if w.great.contains(&kind) {
                    push("great", w.unit(kind), None);
                } else if let Some(c) = e.cities.iter().find(|c| c.tile == u.tile) {
                    push("unit", w.unit(kind), Some(&c.name));
                }
            }
            let city_name = |id: u64| now.iter().flatten().flat_map(|e| e.cities.iter()).find(|c| c.id == id).map_or_else(|| "?".to_owned(), |c| c.name.clone());
            for &(a, z) in e.routes.iter().filter(|r| !b.routes.contains(r)) {
                list.push(decision(f, round, "trade", format!("{} → {}", city_name(a), city_name(z)), None));
            }
        }
    }
    Value::Object(out.into_iter().map(|(p, v)| (p.to_string(), Value::Array(v))).collect())
}
