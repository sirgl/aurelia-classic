//! The army net sees only what its side knows (`army_net::known`). Two guards that keep the class of defect from coming back:
//!
//! 1. Non-interference. On a copy of a game everything the side does not know is changed: the foe's and the third parties' units
//!    it does not see (hit points, moves, experience, place), the cities it does not see (damage, buildings, population), the
//!    tiles it has not explored (terrain, feature, improvement, road), what changed on explored tiles it does not see now (their
//!    improvement and road), the private numbers of the units it sees (moves left). The observation (the orders' normaliser and
//!    planes, the tokens with the third parties and the history, the candidates of every unit of the army) must stay bit for bit
//!    the same. The test names no input: whichever input reads the unknown fails it. Positive controls (a change the side can
//!    see changes the observation) keep it from passing for nothing.
//! 2. The input modules cannot name the full state: `observe.rs` reaches the game only through `Known::new(game, ..)`.

use aurelia_qa::army_net::known::Known;
use aurelia_qa::army_net::observe::{self, Candidate, Tokens};
use aurelia_qa::army_net::{in_army, Aim, MapWindow, Orders};
use aurelia_qa::bench::modules::{majors, BenchRules, ClassicBench};
use aurelia_qa::bench::suite::MapSpec;
use aurelia_qa::trainers::families::{self, Fam};
use aurelia_qa::trainers::lab;
use classic_rules::classic::Classic;
use classic_rules::ids::{ImprovementId, PlayerId, TileId, UnitHandle};
use classic_rules::{ClassicGame, CompiledRuleset, GameState};
use kernel::ai::driver::AiDriver;
use kernel::ai::seats::Seated;
use kernel::ai::StepOutcome;
use kernel::world::map::Route;

/// Everything the net reads at one moment, as bits.
fn fingerprint(game: &GameState, rules: &CompiledRuleset, orders: &Orders) -> Vec<u32> {
    let mut v: Vec<u32> = Vec::new();
    let bits = |v: &mut Vec<u32>, xs: &[f32]| v.extend(xs.iter().map(|x| x.to_bits()));
    v.push(0xF000_0000);
    v.push(orders.mean_strength.to_bits());
    v.push(0xF000_0001);
    bits(&mut v, &orders.statics);
    v.push(0xF000_0002);
    bits(&mut v, &orders.statics_rel);
    // The history: our turn ends, one unit of the army is hit meanwhile, and the next look tells the event (and whether it was
    // in the fire of a foe's city: the cities the side knows).
    observe::reset_history();
    observe::set_history_orders(orders);
    observe::note_turn_end(game, rules);
    let mut hit = game.clone();
    if let Some((h, c)) = Known::new(game, rules, orders.side).own_units().into_iter().find(|(_, c)| orders.army.contains(&c.id)) {
        hit.debug_set_unit_hp(h, c.hp.saturating_sub(3).max(1));
    }
    let t: Tokens = observe::tokens(&hit, rules, orders);
    observe::reset_history();
    v.push(0xF000_0003);
    v.push(t.len() as u32);
    for f in &t.feat {
        bits(&mut v, f);
    }
    for x in &t.xfeat {
        bits(&mut v, x);
    }
    v.extend(t.dist.iter().map(|d| u32::from(*d)));
    v.extend(t.selectable.iter().map(|s| u32::from(*s)));
    v.extend(t.pos.iter().map(|p| *p as u32));
    v.extend(t.tile.iter().map(|x| x.0));
    v.push(0xF000_0004);
    for h in &t.hist {
        bits(&mut v, h);
    }
    v.push(0xF000_0005);
    v.push(t.third.len() as u32);
    for f in &t.third.feat {
        bits(&mut v, f);
    }
    for x in &t.third.x {
        bits(&mut v, x);
    }
    v.extend(t.third.dist.iter().map(|d| u32::from(*d)));
    v.extend(t.third.tile.iter().map(|x| x.0));
    // The candidates of every unit of the army.
    v.push(0xF000_0006);
    for h in t.unit.iter().flatten() {
        if game.view().auth.units().get(*h).is_some_and(|u| u.core.owner == orders.side && orders.army.contains(&u.core.id)) {
            let c: Vec<Candidate> = observe::candidates(game, rules, orders, *h);
            v.push(c.len() as u32);
            for c in &c {
                bits(&mut v, &c.feat);
                bits(&mut v, &c.x);
                v.push(u32::from(c.promo));
                v.push(c.pos as u32);
                v.extend(format!("{:?}", c.action).bytes().map(u32::from));
            }
        }
    }
    v
}

/// The commands offered to the army's units (for the report of a failure).
fn offered(game: &GameState, rules: &CompiledRuleset, orders: &Orders) -> std::collections::BTreeSet<String> {
    let k = Known::new(game, rules, orders.side);
    let mut out = std::collections::BTreeSet::new();
    for (h, c) in k.own_units() {
        if orders.army.contains(&c.id) {
            for cand in observe::candidates(game, rules, orders, h) {
                out.insert(format!("{:?}: {:?}", c.id, cand.action));
            }
        }
    }
    out
}

/// Where two fingerprints first differ: the section and the offset in it.
fn where_differs(a: &[u32], b: &[u32]) -> String {
    let names = ["mean_strength", "statics", "statics_rel", "tokens", "history", "third tokens", "candidates"];
    let (mut sec, mut off) = (0usize, 0usize);
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (a.get(i).copied(), b.get(i).copied());
        if let Some(m) = x.filter(|m| m & 0xF000_0000 == 0xF000_0000 && *m < 0xF000_0010) {
            sec = (m & 0xFF) as usize;
            off = 0;
            if x != y {
                return format!("{}: section length differs", names[sec]);
            }
            continue;
        }
        if x != y {
            return format!("{} at offset {off} (len {} vs {}): {:?} vs {:?}", names[sec], a.len(), b.len(), x, y);
        }
        off += 1;
    }
    "equal".into()
}

/// What the test changed (so that a test that changes nothing cannot pass).
#[derive(Default, Debug)]
struct Changed {
    hidden_units: usize,
    hidden_cities: usize,
    unknown_tiles: usize,
    unseen_tiles: usize,
    private_numbers: usize,
}

/// Changes everything the side does not know. Nothing the side sees or has explored changes.
fn change_the_unknown(game: &mut GameState, rules: &CompiledRuleset, side: PlayerId) -> Changed {
    let mut did = Changed::default();
    let (hidden, hidden_cities, known, seen_tiles) = {
        let auth = game.view().auth;
        let view = game.player_view(rules, side);
        let hidden: Vec<(UnitHandle, kernel::world::unit::UnitCore)> = auth.units().iter_handles().filter(|(_, c)| c.owner != side && !view.sees_unit(c)).map(|(h, c)| (h, *c)).collect();
        let hidden_cities: Vec<_> = auth.cities().iter().filter(|(_, c)| c.owner != side && !view.is_visible(c.tile)).map(|(h, c)| (h, c.tile, c.buildings.clone(), c.population)).collect();
        let n = auth.map().tile_count() as u32;
        let known: Vec<bool> = (0..n).map(|t| view.tile(TileId(t)).snapshot().is_some()).collect();
        let seen_tiles: Vec<bool> = (0..n).map(|t| view.is_visible(TileId(t))).collect();
        (hidden, hidden_cities, known, seen_tiles)
    };
    let occupied = |game: &GameState, t: TileId| {
        let auth = game.view().auth;
        auth.indexes().city_on_tile(t).is_some() || !auth.indexes().units_on(t).is_empty()
    };
    // Where a hidden unit may go: a tile the side does not see, nothing on it.
    let spots: Vec<TileId> = (0..known.len() as u32).map(TileId).filter(|t| !seen_tiles[t.0 as usize]).collect();
    for (i, (h, c)) in hidden.iter().enumerate() {
        game.debug_set_unit_hp(*h, (c.hp / 2).max(1));
        game.debug_set_unit_moves(*h, 0);
        game.debug_set_experience(*h, c.experience + 40);
        // Move it: take it off and put its kind down on another tile the side does not see.
        let to = spots.iter().copied().cycle().skip(i * 7 + 3).take(spots.len().max(1)).find(|t| !occupied(game, *t) && *t != c.tile);
        if let Some(to) = to {
            game.debug_remove_unit(rules, *h);
            game.debug_spawn_unit(rules, c.owner, to, c.kind);
        }
        did.hidden_units += 1;
    }
    // A foe's unit the side never saw, where nobody looks.
    if let (Some(to), Some((_, c))) = (spots.iter().copied().rev().find(|t| !occupied(game, *t)), hidden.first()) {
        game.debug_spawn_unit(rules, c.owner, to, c.kind);
        did.hidden_units += 1;
    }
    for (h, _, buildings, pop) in &hidden_cities {
        game.debug_set_city_damage(*h, 7);
        if let Some(b) = buildings.first() {
            game.debug_remove_building(*h, *b);
        }
        game.debug_set_population(rules, *h, pop + 3);
        did.hidden_cities += 1;
    }
    // Tiles the side has not explored: ground, feature, improvement and road.
    let unknown: Vec<TileId> = (0..known.len() as u32).map(TileId).filter(|t| !known[t.0 as usize]).collect();
    if !unknown.is_empty() {
        let (other_terrain, imp) = {
            let map = game.view().auth.map();
            let first = unknown[0];
            let t0 = map.terrain(first);
            let other = (0..known.len() as u32).map(TileId).filter(|t| known[t.0 as usize]).filter_map(|t| map.terrain(t)).find(|x| Some(*x) != t0);
            (other, ImprovementId(0))
        };
        if let Some(terrain) = other_terrain {
            game.debug_set_terrain_batch(rules, &unknown.iter().take(40).map(|t| (*t, terrain)).collect::<Vec<_>>());
        }
        game.debug_set_improvement_batch(rules, &unknown.iter().take(40).map(|t| (*t, imp)).collect::<Vec<_>>());
        game.debug_set_route_batch(rules, &unknown.iter().take(40).map(|t| (*t, Route::Road)).collect::<Vec<_>>());
        did.unknown_tiles += unknown.len().min(40);
    }
    // Explored tiles the side does not see now: what happens there is not known (the memory keeps what it saw).
    let remembered: Vec<TileId> = (0..known.len() as u32).map(TileId).filter(|t| known[t.0 as usize] && !seen_tiles[t.0 as usize]).filter(|t| !game.view().auth.map().is_water(*t)).take(60).collect();
    if !remembered.is_empty() {
        game.debug_set_improvement_batch(rules, &remembered.iter().map(|t| (*t, ImprovementId(0))).collect::<Vec<_>>());
        game.debug_set_route_batch(rules, &remembered.iter().map(|t| (*t, Route::Road)).collect::<Vec<_>>());
        did.unseen_tiles += remembered.len();
    }
    // The private numbers of units the side sees: moves left (the game shows kind and hit points).
    let seen: Vec<UnitHandle> = {
        let auth = game.view().auth;
        let view = game.player_view(rules, side);
        auth.units().iter_handles().filter(|(_, c)| c.owner != side && view.sees_unit(c)).map(|(h, _)| h).collect()
    };
    for h in seen {
        game.debug_set_unit_moves(h, 0);
        did.private_numbers += 1;
    }
    did
}

/// What the side may see changes: a seen foe loses hit points (the control that the fingerprint reads what is visible).
fn change_the_seen(game: &mut GameState, rules: &CompiledRuleset, side: PlayerId) -> usize {
    let seen: Vec<(UnitHandle, u16)> = {
        let auth = game.view().auth;
        let view = game.player_view(rules, side);
        auth.units().iter_handles().filter(|(_, c)| c.owner != side && view.sees_unit(c)).map(|(h, c)| (h, c.hp)).collect()
    };
    for (h, hp) in &seen {
        game.debug_set_unit_hp(*h, (hp / 2).max(1));
    }
    seen.len()
}

/// The trainers' arena: the whole map known to every seat (`trainers::reveal`).
fn reveal(game: &mut GameState, side: PlayerId, foe: PlayerId, neutral: Option<PlayerId>) {
    let tiles: Vec<TileId> = (0..game.view().auth.map().tile_count() as u32).map(TileId).collect();
    game.debug_reveal(side, &tiles);
    game.debug_reveal(foe, &tiles);
    if let Some(n) = neutral {
        game.debug_reveal(n, &tiles);
    }
}

struct Totals {
    plays: usize,
    hidden_units: usize,
    hidden_cities: usize,
    unknown_tiles: usize,
    unseen_tiles: usize,
    controls: usize,
}

fn check_scene(rules: &CompiledRuleset, fam: Fam, instance: usize, revealed: bool, t: &mut Totals) {
    let era = lab::era_index(rules, "classical").expect("the classical era");
    let (mut game, scene) = families::build(rules, fam, era, instance, 0).expect("the scene builds");
    if revealed {
        reveal(&mut game, scene.learner, scene.enemy, scene.neutral);
    }
    let orders = Orders::of_scene(rules, &game, &scene);
    let before = fingerprint(&game, rules, &orders);
    let mut changed = game.clone();
    let did = change_the_unknown(&mut changed, rules, scene.learner);
    // The orders are made again from the changed game (the normaliser and the planes are part of the observation).
    let orders2 = Orders::of_scene(rules, &changed, &scene);
    let after = fingerprint(&changed, rules, &orders2);
    if before != after {
        let (a, b) = (offered(&game, rules, &orders), offered(&changed, rules, &orders2));
        eprintln!("only before: {:?}\nonly after: {:?}", a.difference(&b).collect::<Vec<_>>(), b.difference(&a).collect::<Vec<_>>());
    }
    assert!(before == after, "{fam:?} #{instance} (revealed: {revealed}): the observation changed when only the unknown did ({did:?}): {}", where_differs(&before, &after));
    t.plays += 1;
    t.hidden_units += did.hidden_units;
    t.hidden_cities += did.hidden_cities;
    t.unknown_tiles += did.unknown_tiles;
    t.unseen_tiles += did.unseen_tiles;
    // The control: a change the side sees is read.
    let mut seen = game.clone();
    if change_the_seen(&mut seen, rules, scene.learner) > 0 {
        assert!(before != fingerprint(&seen, rules, &Orders::of_scene(rules, &seen, &scene)), "{fam:?} #{instance}: a seen foe's hit points are not read");
        t.controls += 1;
    }
}

/// The families the guard walks: the fog ones, the ones with a third party, the siege and the hunt, each revealed (as the
/// trainers play them) and not (the side has explored nothing it did not see).
const FAMILIES: [Fam; 14] = [Fam::Siege, Fam::Prey, Fam::Snipe, Fam::AssaultPeace, Fam::CityFire, Fam::Camp, Fam::AllySiege, Fam::NeutralPath, Fam::AmbushFog, Fam::HuntFog, Fam::ScoutReport, Fam::SurpriseAttack, Fam::BlindSector, Fam::TwoFronts];

#[test]
fn the_observation_does_not_change_with_what_the_side_does_not_know() {
    let rules = ClassicBench::rules();
    let mut t = Totals { plays: 0, hidden_units: 0, hidden_cities: 0, unknown_tiles: 0, unseen_tiles: 0, controls: 0 };
    for fam in FAMILIES {
        for instance in 0..2 {
            for revealed in [true, false] {
                check_scene(&rules, fam, instance, revealed, &mut t);
            }
        }
    }
    assert!(t.plays >= 40, "plays {}", t.plays);
    assert!(t.hidden_units > 0 && t.hidden_cities > 0, "the guard changed hidden units {} and cities {}", t.hidden_units, t.hidden_cities);
    assert!(t.unknown_tiles > 0, "the unrevealed scenes have unexplored tiles to change");
    assert!(t.unseen_tiles > 0, "the revealed scenes have explored tiles out of sight to change");
    assert!(t.controls >= 10, "a seen change is read: {} controls", t.controls);
}

/// A real map with fog: a duel on a generated map, both sides played by the AI for 40 rounds, then the net's orders for the
/// first nation (the foe the second): the foe's units and cities are mostly out of sight, most of the map unexplored.
#[test]
fn a_real_game_with_fog_does_not_leak_either() {
    let rules = ClassicBench::rules();
    let mut seen_hidden = 0;
    for (script, seed) in [("pangaea", 3u64), ("continents", 5)] {
        let mut game = ClassicBench::new_game(&rules, &MapSpec { script, size: "duel" }, seed, 60);
        let mut driver = AiDriver::<Classic, _>::with_mind(ClassicBench::settings(&rules), Vec::new(), Seated::new(ClassicBench::mind("standard").unwrap(), Vec::new()));
        while !game.is_over() && game.view().auth.turn().round <= 40 {
            loop {
                match driver.step(&mut game, &rules) {
                    StepOutcome::Worked | StepOutcome::PlayerDone(_) => {}
                    StepOutcome::RoundDone => break,
                    StepOutcome::Waiting => break,
                }
            }
        }
        let nations = majors(&game);
        let (side, foe) = (nations[0], nations[1]);
        let k = Known::new(&game, &rules, side);
        let army: Vec<_> = k.own_units().iter().filter(|(_, c)| in_army(&rules, c.kind)).map(|(_, c)| c.id).collect();
        let centre = k.own_cities().first().map(|c| c.tile).or_else(|| k.own_units().first().map(|(_, c)| c.tile)).expect("the side has a city or a unit");
        let window = MapWindow::around(&game, centre);
        let statics = aurelia_qa::army_net::statics_of(&rules, &game, &window, side, foe, centre, None);
        let statics_rel = aurelia_qa::army_net::statics_rel_of(&rules, &game, &window, side, foe);
        let orders = Orders { side, foe, aim: Aim::Destroy, tile: centre, refuge: None, optional: false, start_round: game.view().auth.turn().round, rounds: 20, army, mean_strength: 1.0, window, statics, statics_rel, fog: true, price: [0.1, 0.05, 1.0] };
        let before = fingerprint(&game, &rules, &orders);
        let mut changed = game.clone();
        let did = change_the_unknown(&mut changed, &rules, side);
        seen_hidden += did.hidden_units + did.hidden_cities;
        let window2 = MapWindow::around(&changed, centre);
        let orders2 = Orders { statics: aurelia_qa::army_net::statics_of(&rules, &changed, &window2, side, foe, centre, None), statics_rel: aurelia_qa::army_net::statics_rel_of(&rules, &changed, &window2, side, foe), window: window2, ..orders.clone() };
        let after = fingerprint(&changed, &rules, &orders2);
        assert!(before == after, "{script} #{seed}: the observation changed when only the unknown did ({did:?}): {}", where_differs(&before, &after));
        assert!(did.unknown_tiles > 0, "{script} #{seed}: the map is mostly unexplored ({did:?})");
    }
    assert!(seen_hidden > 0, "the real games hold units and cities out of sight");
}

/// The source guard: the module that builds the net's inputs reaches the game only through `Known`. Nothing in it opens the
/// world (`.view()`, `.auth`, the indexes), asks the full-state helpers (`act::defence`, `act::preview`, `act::core_of`) or
/// names a `CityCore`; the only mention of `game` that is not a parameter is `Known::new(game, ..)`.
#[test]
fn the_input_module_cannot_name_the_full_state() {
    let src = include_str!("../src/army_net/observe.rs");
    for (n, line) in src.lines().enumerate() {
        let code = line.split("//").next().unwrap_or("");
        for bad in [".view()", ".auth", "auth.", "indexes()", "act::defence", "act::preview", "act::core_of", "act::reachable", "CityCore", "debug_", "player_view"] {
            assert!(!code.contains(bad), "observe.rs:{} names `{bad}`: an input reads the game through `Known` only", n + 1);
        }
        if code.contains("game") && !code.contains("game: &GameState") && !code.contains("Known::new(game") {
            panic!("observe.rs:{} uses `game` besides the doors (`game: &GameState` parameter, `Known::new(game, ..)`): {}", n + 1, line.trim());
        }
    }
}

/// A reported metric, not a rule (a blind start is ordinary in a real game): per family, the share of scene starts where the
/// side sees none of the enemy units, and none of its third party's. Prints a line per family; with `BLIND_STARTS_OUT=FILE`
/// it writes the table (tab separated: family, fog, enemy blind, enemy starts, third blind, third starts).
#[test]
fn the_share_of_starts_where_the_side_sees_no_enemy_is_reported_per_family() {
    let rules = ClassicBench::rules();
    let era = lab::era_index(&rules, "classical").expect("the classical era");
    let mut table = String::from("family\tfog\tenemy_blind\tenemy_starts\tthird_blind\tthird_starts\tobs_version\n");
    for fam in Fam::ALL {
        let (mut n, mut blind, mut third_n, mut third_blind) = (0, 0, 0, 0);
        let mut fog = false;
        for i in 0..8 {
            let Ok((mut game, scene)) = families::build(&rules, fam, era, i, 0) else { continue };
            reveal(&mut game, scene.learner, scene.enemy, scene.neutral);
            fog = Orders::of_scene(&rules, &game, &scene).fog;
            let k = Known::new(&game, &rules, scene.learner);
            if !scene.theirs.is_empty() {
                n += 1;
                blind += usize::from(k.seen_units(scene.enemy).is_empty());
            }
            if let (Some(nb), false) = (scene.neutral, scene.third.is_empty()) {
                third_n += 1;
                third_blind += usize::from(k.seen_units(nb).is_empty());
            }
        }
        eprintln!("BLIND {} fog={fog} enemy {blind}/{n} third {third_blind}/{third_n}", fam.name());
        table.push_str(&format!("{}\t{fog}\t{blind}\t{n}\t{third_blind}\t{third_n}\t{}\n", fam.name(), aurelia_qa::army_net::known::OBS_VERSION));
    }
    if let Some(out) = std::env::var_os("BLIND_STARTS_OUT") {
        std::fs::write(out, table).expect("the table is written");
    }
}
