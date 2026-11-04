//! Census of the start positions of new games: for every start of every map it prints one CSV row with
//! what lies around the start tile, read from the ruleset's real tile yields.
//!
//! `cargo run --profile fasttest -p classic-rules --example start_census -- [sizes] [scripts] [seeds] [first-seed] [off]`
//! sizes: comma list of duel,tiny,small,standard,large,huge (default duel,tiny,small,standard), or WxH:N (a grid and
//! N nations, no city-states; `32x24:4` is the world of the offscreen `--screenshot`);
//! scripts: comma list of the map script keys (default continents,fractal); seeds: a count (default 200);
//! `off` turns the start balance off (the generator before it: the BEFORE tables).
//! The rows go to stdout, `tools/start-census-report.py` turns them into the tables of a report.
//!
//! Per start and per radius (2 and 3 tiles; the start tile itself is left out, it is the city centre) the row holds
//! the food, the production and the gold of the other tiles (`tile_yields_for`: terrain, feature, the resource once
//! its reveal technology is known, as the owner of the start sees it on turn 0), the tiles by kind, and, within
//! radius 3, the number of deposits of every strategic resource and the counts of luxury and bonus deposits.
use classic_content::core::core_rules;
use classic_rules::derived::tile_yields::{fresh_water, tile_yields_for};
use classic_rules::ids::{PlayerId, TileId};
use classic_rules::math::hex_disc;
use classic_rules::rules::content::ResourceClass;
use classic_rules::scenario::{generate_new_game_report, LandOptions, MapScript, NewGameOptions, ScenarioConfig, StartBalance};
use classic_rules::world::facts::TruthFacts;
use classic_rules::world::size::WorldSize;
use classic_rules::compile_rules;

/// A map size of the census: a name, the grid and the numbers of nations and city-states.
struct Size {
    name: String,
    width: u32,
    height: u32,
    nations: u8,
    minors: u8,
}

/// A world size by name, or `WxH:N` (nations N, no city-states: the offscreen screenshot's world is `32x24:4`).
fn size_named(name: &str) -> Size {
    if let Some((grid, nations)) = name.split_once(':') {
        let (w, h) = grid.split_once('x').expect("WxH:N");
        return Size { name: name.into(), width: w.parse().unwrap(), height: h.parse().unwrap(), nations: nations.parse().unwrap(), minors: 0 };
    }
    let (size, nations, minors) = match name {
        "duel" => (WorldSize::Duel, 2, 4),
        "tiny" => (WorldSize::Tiny, 4, 8),
        "small" => (WorldSize::Small, 6, 12),
        "standard" => (WorldSize::Standard, 8, 16),
        "large" => (WorldSize::Large, 10, 20),
        "huge" => (WorldSize::Huge, 12, 24),
        other => panic!("unknown size {other}"),
    };
    let (width, height) = size.grid();
    Size { name: format!("{size:?}"), width, height, nations, minors }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let sizes: Vec<Size> = args
        .first()
        .map(|s| s.split(',').map(size_named).collect())
        .unwrap_or_else(|| ["duel", "tiny", "small", "standard"].map(size_named).into());
    let scripts: Vec<MapScript> = args
        .get(1)
        .map(|s| s.split(',').map(|k| MapScript::from_key(k).expect("known map script")).collect())
        .unwrap_or_else(|| vec![MapScript::Continents, MapScript::Fractal]);
    let seeds: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(200);
    let first: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(1);
    let balance = if args.get(4).map(String::as_str) == Some("off") { StartBalance::OFF } else { StartBalance::ON };
    let rules = compile_rules(core_rules()).unwrap();

    let strategic: Vec<usize> = (0..rules.resources.len())
        .filter(|&k| rules.resources[k].class == Some(ResourceClass::Strategic))
        .collect();
    let short = |k: usize| rules.resources[k].key.rsplit('/').next().unwrap().to_string();
    let mut header = String::from("size,script,seed,player,nation,q,r,coastal");
    for r in [2, 3] {
        header += &format!(
            ",f{r},p{r},g{r},land{r},water{r},hills{r},mount{r},fresh{r},food2_{r},prod2_{r},prod1_{r}"
        );
    }
    header += ",fresh_centre,lux3,lux_kinds3,bonus3,under_centre,edit_strategic,edit_missing,edit_bonus,edit_turned,edit_cleared";
    for &k in &strategic {
        header += &format!(",s_{}", short(k));
    }
    println!("{header}");

    for size in sizes {
        let (w, h, nations, minors) = (size.width, size.height, size.nations, size.minors);
        for script in &scripts {
            for seed in first..first + seeds {
                let cfg = ScenarioConfig {
                    seed,
                    players: nations,
                    width: w,
                    height: h,
                    cities: 0,
                    units: 0,
                    water_ratio: script.water_ratio(25),
                };
                let options = NewGameOptions { city_states: minors, script: *script, start_balance: balance, ..Default::default() };
                let (game, edits) = generate_new_game_report(&rules, &cfg, None, options, LandOptions::CLASSIC);
                let auth = game.view().auth;
                let map = auth.map();
                let dims = map.dimensions;
                for p in 0..nations {
                    let player = PlayerId(p);
                    let Some(start) = auth.indexes().units_of(player).first().and_then(|u| auth.units().get(*u)).map(|u| u.core.tile)
                    else {
                        continue;
                    };
                    let coord = dims.to_coord(start).unwrap();
                    let facts = TruthFacts::new(auth, &rules, player);
                    let nation = i32::from(auth.players().nation_of(player));
                    let coastal = hex_disc(dims, start, 3).any(|t| map.is_water(t));
                    let mut row = format!("{},{},{seed},{p},{nation},{},{},{}", size.name, script.key(), coord.q, coord.r, u8::from(coastal));
                    for radius in [2, 3] {
                        let (mut f, mut pr, mut g, mut land, mut water, mut hills, mut mount, mut fresh) = (0, 0, 0, 0, 0, 0, 0, 0);
                        let (mut food2, mut prod2, mut prod1) = (0, 0, 0);
                        for tile in hex_disc(dims, start, radius) {
                            if tile == start {
                                continue;
                            }
                            let y = tile_yields_for(auth, &rules, tile, player);
                            f += y.food;
                            pr += y.production;
                            g += y.gold;
                            food2 += i32::from(y.food >= 2);
                            prod2 += i32::from(y.production >= 2);
                            prod1 += i32::from(y.production >= 1);
                            if map.is_water(tile) {
                                water += 1;
                            } else {
                                land += 1;
                                fresh += i32::from(fresh_water(&facts, &rules, tile));
                                let ground = map.terrain(tile).and_then(|c| rules.terrain_kinds.palette().ground(c.0));
                                match ground {
                                    Some(classic_rules::rules::roles::Ground::Hills) => hills += 1,
                                    Some(classic_rules::rules::roles::Ground::Mountains) => mount += 1,
                                    _ => {}
                                }
                            }
                        }
                        row += &format!(",{f},{pr},{g},{land},{water},{hills},{mount},{fresh},{food2},{prod2},{prod1}");
                    }
                    let mut strat = vec![0i32; strategic.len()];
                    let (mut lux, mut bonus) = (0, 0);
                    let mut kinds = std::collections::BTreeSet::new();
                    let mut under = 0;
                    for tile in hex_disc(dims, start, 3) {
                        let Some(Some(res)) = map.resource(tile) else { continue };
                        let def = &rules.resources[res.0 as usize];
                        match def.class {
                            Some(ResourceClass::Luxury) => {
                                lux += 1;
                                kinds.insert(res.0);
                            }
                            Some(ResourceClass::Bonus) => bonus += 1,
                            Some(ResourceClass::Strategic) => {
                                if tile == start {
                                    under += 1;
                                } else if let Some(i) = strategic.iter().position(|&k| k == res.0 as usize) {
                                    strat[i] += 1;
                                }
                            }
                            _ => {}
                        }
                    }
                    row += &format!(",{},{lux},{},{bonus},{under}", u8::from(fresh_water(&facts, &rules, start)), kinds.len());
                    // What the start balance changed in the whole game (the same on each of its rows).
                    row += &format!(",{},{},{},{},{}", edits.strategic_added, edits.strategic_missing, edits.bonus_added, edits.terrain_turned, edits.features_cleared);
                    for s in strat {
                        row += &format!(",{s}");
                    }
                    println!("{row}");
                }
                let _ = TileId(0);
            }
        }
    }
}
