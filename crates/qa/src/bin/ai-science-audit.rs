//! The science of chosen seats of a recorded game, city by city and source
//! by source, as the rules compute it (classic/docs/ai-science-audit.md):
//! the log is replayed through the kernel and, at the start of each chosen
//! round's turn of each chosen seat, one JSON line per seat is printed with
//! the state an independent calculation needs (population, buildings,
//! specialists, worked tiles' science, policies, happiness) and the engine's
//! numbers (base, flat, percent, rate, trade, process, the player's total).
//!
//! `ai-science-audit LOG.aurlog --players 1,0 --rounds 100,150,198`

use std::collections::BTreeMap;
use std::path::PathBuf;

use aurelia_qa::bench::modules::{BenchRules, ClassicBench};
use classic_rules::rules::effects::YieldKind;
use classic_rules::{Command, CompiledRuleset, GameState};
use kernel::ids::PlayerId;
use serde_json::{json, Value};

fn main() {
    let mut log = None;
    let mut players: Vec<u8> = vec![1];
    let mut rounds: Vec<u32> = vec![100, 150, 198];
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--players" => players = it.next().unwrap().split(',').map(|s| s.parse().unwrap()).collect(),
            "--rounds" => rounds = it.next().unwrap().split(',').map(|s| s.parse().unwrap()).collect(),
            path => log = Some(PathBuf::from(path)),
        }
    }
    let log = log.expect("a log");
    let rules = ClassicBench::rules();
    let (spec, journal) = aurelia_qa::export::read_log(&log).unwrap();
    let mut done: BTreeMap<(u32, u8), bool> = BTreeMap::new();
    let result = aurelia_qa::export::replay_with(
        &rules,
        &spec,
        &journal,
        |_, _| {},
        |game: &GameState, _command: &Command, round: u32, player: PlayerId| {
            if rounds.contains(&round) && players.contains(&player.0) && !done.contains_key(&(round, player.0)) {
                done.insert((round, player.0), true);
                println!("{}", seat(game, &rules, player, round));
            }
        },
        |_, _| {},
    );
    if let Err(e) = result {
        eprintln!("replay: {e}");
        std::process::exit(1);
    }
}

fn seat(game: &GameState, rules: &CompiledRuleset, player: PlayerId, round: u32) -> Value {
    use classic_rules::derived::income::{city_income, player_income};
    let auth = game.view().auth;
    let culture = auth.module().culture().of(player);
    let policies: Vec<String> = culture.policies.iter().map(|p| format!("{} / {}", rules.module.policies[p.0 as usize].key, rules.module.policies[p.0 as usize].name)).collect();
    let mut cities = Vec::new();
    let mut sum = 0i64;
    for handle in auth.indexes().cities_of(player) {
        let Some(core) = auth.cities().get(*handle) else { continue };
        let b = classic_rules::derived::yields::city_breakdown(auth, rules, core);
        let income = city_income(auth, rules, core);
        let trade = classic_rules::derived::trade::city_trade_yields(auth, *handle);
        let totals = classic_rules::derived::yields::channel_totals(&b.modifiers, YieldKind::Science);
        let mut specialists = BTreeMap::new();
        for (i, n) in b.work.specialists.iter().enumerate() {
            if *n > 0 {
                specialists.insert(rules.specialists[i].key.clone(), *n);
            }
        }
        let table = kernel::derived::citizens::specialist_yield_table(rules, &b.modifiers.stats);
        let per_specialist: BTreeMap<String, i32> = rules.specialists.iter().enumerate().map(|(i, d)| (d.key.clone(), table[i].science)).collect();
        let worked: Vec<Value> = b.work.worked()
            .iter()
            .filter(|(_, y)| y.science != 0)
            .map(|(t, y)| {
                let map = auth.map();
                json!({"tile": t.0, "science": y.science, "feature": map.feature(*t).flatten().map(|f| rules.features[f.0 as usize].key.clone()), "improvement": map.working_improvement(*t).map(|i| rules.improvements[i.0 as usize].key.clone())})
            })
            .collect();
        let buildings: Vec<String> = core.buildings.iter().map(|k| rules.buildings[k.0 as usize].key.clone()).collect();
        sum += i64::from(income.science);
        let mountain_next = {
            let map = auth.map();
            let kinds = map.terrain_kinds();
            map.dimensions.neighbors(core.tile).as_slice().iter().any(|t| map.terrain(*t).is_some_and(|c| kinds.is_mountains(c.0)))
        };
        cities.push(json!({
            "name": core.name,
            "capital": classic_rules::world::city::is_capital(rules, core),
            "population": core.population,
            "buildings": buildings,
            "specialists": specialists,
            "science_per_specialist": per_specialist,
            "worked_tiles": b.work.worked().len(),
            "tile_science": b.work.tile_yields.science,
            "tiles_with_science": worked,
            "specialist_science": b.work.specialist_yields.science,
            "base_science": b.base.science,
            "flat": totals.flat,
            "percent": totals.percent,
            "rate": b.rates.science,
            "trade_science": trade.science,
            "process_science": income.science - b.rates.science,
            "income_science": income.science,
            "queue_empty": core.queue.is_empty(),
            "focus": format!("{:?}", core.citizens.focus),
            "mountain_next": mountain_next,
            "food_surplus": income.food_surplus,
            "food": income.food,
            "production": income.production,
            "threshold": classic_rules::derived::income::player_growth_threshold(auth, rules, player, core.population),
            "head": core.queue.first().map(|i| format!("{i:?}")),
        }));
    }
    let income = player_income(auth, rules, player);
    let excess = classic_rules::derived::happiness::excess_snapshot(auth, player);
    let economy = auth.players().economy_of(player);
    let head = auth.players().research_queue_of(player).first().copied();
    json!({
        "round": round,
        "player": player.0,
        "nation": rules.nations.get(usize::from(auth.players().nation_of(player))).map(|n| n.key.clone()),
        "techs": auth.players().techs_of(player).len(),
        "policies": policies,
        "ideology": culture.ideology.map(|b| rules.module.policy_branches[b.0 as usize].key.clone()),
        "excess_happiness": excess,
        "golden_age": economy.is_some_and(|e| e.golden_age_turns > 0),
        "cities_science": sum,
        "player_science": income.science,
        "gold_income": income.gold,
        "research": head.map(|t| json!({"tech": rules.technologies[t.0 as usize].key, "cost": classic_rules::derived::research::technology_cost(auth, rules, player, t), "base": rules.technologies[t.0 as usize].cost})),
        "culture_stock": culture.culture,
        "culture_per_turn": classic_rules::derived::culture::culture_per_turn(auth, rules, player),
        "next_policy_cost": classic_rules::derived::culture::next_policy_cost(auth, rules, player),
        "era": classic_rules::world::player::player_era(auth, rules, player),
        "allied_cs_science": classic_rules::derived::income::allied_city_state_science(auth, player),
                "cities": cities,
    })
}
