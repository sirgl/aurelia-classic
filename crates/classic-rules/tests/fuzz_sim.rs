//! Property/fuzz harness (§31 platform/project.md): random worlds + random commands,
//! validate after EVERY commit, rejected commands do not change the hash.
//!
//! Important: only the public API (GameState::new/execute/validate, generate).
//! If the public surface is enough for the harness, we trust the §41 boundary.

use classic_content::ron::parse_ron;
use classic_rules::ids::{
    BuildingKindId, CityHandle, PlayerId, ProductionItem, TechId, TileId, UnitHandle, UnitKindId,
};
use classic_rules::replay::authoritative_hash;
use classic_rules::rules::SourceRules;
use classic_rules::scenario::{generate, ScenarioConfig};
use classic_rules::{compile_rules, Command, CompiledRuleset, GameState};

struct FuzzRng(u64);

/// Content for the fuzz: a founder + a warrior + 2 techs (otherwise FoundCity/ChooseTech
/// always reject, and coverage of the successful paths dies).
/// The keys are the same as in the game's core content (settler=0, warrior=1, tech=0..1).
/// A RON string through the real parser (no hardcoded tables).
fn fuzz_content() -> SourceRules {
    parse_ron(
        r#"(
        units: [
            (id: "core:unit/settler", movement: 2, strength: 1, founder: true, cost: 40),
            (id: "core:unit/warrior", movement: 2, strength: 10, cost: 30),
        ],
        technologies: [
            (id: "core:tech/agriculture", cost: 20, effects: [(scope: City, selector: OwnerCities, modifier: AddYieldPercent(yield_kind: Output, percent: 10))]),
            (id: "core:tech/pottery", cost: 40, effects: [(scope: City, selector: OwnerCities, modifier: AddYieldPercent(yield_kind: Output, percent: 10))]),
        ],
    )"#,
    )
    .map(with_classic_terrains)
    .expect("fuzz fixture must parse")
}

/// The fixtures name no terrains: they get the classic terrain table, so
/// tiles yield by the classic terrain values (as the legacy content does).
fn with_classic_terrains(mut rules: SourceRules) -> SourceRules {
    classic_content::classic::add_classic_terrains(&mut rules);
    rules
}

impl FuzzRng {
    fn next(&mut self) -> u64 {
        let mut x = self.0.max(1);
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

fn random_world(rng: &mut FuzzRng, rules: &CompiledRuleset) -> GameState {
    let cfg = ScenarioConfig {
        seed: rng.next(),
        players: 2 + rng.below(2) as u8, // 2..=3
        width: 8 + rng.below(9) as u32,  // 8..=16
        height: 8 + rng.below(9) as u32,
        cities: rng.below(6) as usize,
        units: rng.below(11) as usize,
        water_ratio: rng.below(31) as u8, // 0..=30
    };
    generate(&cfg, rules)
}

fn pool_units(game: &GameState) -> Vec<UnitHandle> {
    let n = game.view().auth.players().count();
    let mut out = Vec::new();
    for i in 0..n {
        out.extend_from_slice(game.view().auth.indexes().units_of(PlayerId(i as u8)));
    }
    out
}

fn pick_unit(rng: &mut FuzzRng, units: &[UnitHandle]) -> UnitHandle {
    if rng.below(10) == 0 {
        UnitHandle {
            index: 5000 + rng.below(5000) as u32,
            generation: 0,
        }
    } else if units.is_empty() {
        UnitHandle {
            index: 0,
            generation: 0,
        }
    } else {
        units[rng.below(units.len() as u64) as usize]
    }
}

/// Mostly the active player (sequential turns), sometimes anyone (the
/// rejection path of "not your turn").
fn pick_player(rng: &mut FuzzRng, game: &GameState) -> PlayerId {
    let players = game.view().auth.players().count() as u64;
    if rng.below(5) == 0 {
        PlayerId(rng.below(players) as u8)
    } else {
        game.view().auth.active_player()
    }
}

fn pick_city(rng: &mut FuzzRng, game: &GameState) -> CityHandle {
    let cities: Vec<CityHandle> = game.view().auth.cities().iter().map(|(h, _)| h).collect();
    if rng.below(10) == 0 || cities.is_empty() {
        CityHandle {
            index: 5000 + rng.below(5000) as u32,
            generation: 0,
        }
    } else {
        cities[rng.below(cities.len() as u64) as usize]
    }
}

fn random_command(rng: &mut FuzzRng, game: &GameState) -> Command {
    let players = game.view().auth.players().count();
    let tiles = game.view().auth.map().tile_count();
    let units = if rng.below(5) == 0 {
        pool_units(game)
    } else {
        let active = game.view().auth.active_player();
        game.view().auth.indexes().units_of(active).to_vec()
    };
    // Sometimes we slip in knowingly broken handles/tiles (rejection purity).
    match rng.below(100) {
        0..=34 => Command::MoveUnit {
            unit: pick_unit(rng, &units),
            destination: TileId(rng.below(tiles as u64 + 3) as u32), // +3: sometimes OOB
        },
        35..=44 => Command::MarchTo {
            unit: pick_unit(rng, &units),
            destination: TileId(rng.below(tiles as u64) as u32),
        },
        45..=54 => Command::FoundCity {
            settler: pick_unit(rng, &units),
        },
        55..=66 => {
            let a = pick_unit(rng, &units);
            let b = pick_unit(rng, &units);
            Command::AttackUnit {
                attacker: a,
                defender: b,
            }
        }
        67..=74 => {
            let a = pick_player(rng, game);
            let mut b = rng.below(players as u64) as usize;
            if b == a.0 as usize {
                b = (b + 1) % players.max(1);
            }
            Command::DeclareWar {
                player: a,
                enemy: PlayerId(b as u8),
            }
        }
        75..=89 => Command::EndTurn {
            player: pick_player(rng, game),
        },
        90..=94 => Command::PurchaseTile {
            player: pick_player(rng, game),
            city: pick_city(rng, game),
            tile: TileId(rng.below(tiles as u64 + 3) as u32), // +3: sometimes OOB
        },
        _ => Command::ChooseTechnology {
            player: pick_player(rng, game),
            technology: TechId(rng.below(3) as u16),
        },
    }
}

fn run_script(game: &mut GameState, rules: &CompiledRuleset, rng: &mut FuzzRng, steps: usize) {
    for _ in 0..steps {
        let cmd = random_command(rng, game);
        let before = authoritative_hash(game);
        match game.execute(rules, cmd) {
            Ok(_) => {
                // Invariants after every commit (§30 + §12 oracle inside validate).
                game.validate(rules)
                    .expect("world must stay valid after commit");
            }
            Err(_) => {
                // Rejected = the world is untouched (§54).
                assert_eq!(
                    before,
                    authoritative_hash(game),
                    "rejected command mutated truth"
                );
            }
        }
    }
}

#[test]
fn fuzz_validate_every_commit() {
    let rules = compile_rules(fuzz_content()).unwrap();
    let mut rng = FuzzRng(0xF0CACC1A);
    // 40 worlds x 150 commands: mechanics coverage without an endless run in debug.
    for _ in 0..40 {
        let mut game = random_world(&mut rng, &rules);
        game.validate(&rules).unwrap();
        run_script(&mut game, &rules, &mut rng, 150);
    }
}

/// Fold of the final world hashes of the 40 fuzz worlds. Every deliberate
/// change is logged in the re-pin log of tests/baseline_hashes.rs.
const FUZZ_WORLDS_HASH: u64 = 0x3125b1c593dab40e;

#[test]
fn fuzz_world_hash_baseline() {
    let rules = compile_rules(fuzz_content()).unwrap();
    let mut rng = FuzzRng(0xF0CACC1A);
    let mut folded = 0xcbf29ce484222325u64;
    for _ in 0..40 {
        let mut game = random_world(&mut rng, &rules);
        run_script(&mut game, &rules, &mut rng, 150);
        folded = (folded ^ authoritative_hash(&game)).wrapping_mul(0x100000001b3);
    }
    eprintln!("fuzz: worlds={:#018x}", folded);
    assert_eq!(folded, FUZZ_WORLDS_HASH, "fuzz world hashes changed");
}

#[test]
fn fuzz_deterministic_replay() {
    // The same script seed -> the same final (RNG streams + order are deterministic).
    let rules = compile_rules(fuzz_content()).unwrap();
    for world in 0..10 {
        let mut rng = FuzzRng(0xbeef + world);
        let mut g1 = random_world(&mut rng, &rules);
        // We assemble the script on the first branch (only Ok commands).
        let mut script = Vec::new();
        for _ in 0..80 {
            let cmd = random_command(&mut rng, &g1);
            if g1.execute(&rules, cmd.clone()).is_ok() {
                script.push(cmd);
            }
        }
        let h1 = authoritative_hash(&g1);
        // The second branch: the same world seed + the same script.
        let mut rng2 = FuzzRng(0xbeef + world);
        let mut g2 = random_world(&mut rng2, &rules);
        for cmd in &script {
            g2.execute(&rules, cmd.clone()).unwrap();
        }
        assert_eq!(
            h1,
            authoritative_hash(&g2),
            "replay diverged on world {}",
            world
        );
    }
}

// ---- effect system under fuzz (§62: oracle after every commit) ----

/// Content where every trigger, a timed effect, promotions, owner-wide and
/// per-city buildings and cost modifiers are reachable by random commands.
fn effects_content() -> SourceRules {
    parse_ron(
        r#"(
        units: [
            (id: "core:unit/settler", movement: 2, strength: 1, founder: true, cost: 40),
            (id: "core:unit/warrior", movement: 2, strength: 10, cost: 30, effects: [
                (scope: Unit, selector: SourceUnit, trigger: OnBuilt, modifier: AddPromotion("core:promotion/drill"), duration: Once),
            ]),
        ],
        promotions: [
            (id: "core:promotion/drill", effects: [(scope: Unit, selector: SourceUnit, modifier: ModifyCombat(context: Any, percent: 25))]),
            (id: "core:promotion/veteran", effects: [(scope: Unit, selector: SourceUnit, modifier: ModifyMovement(amount: 1))]),
        ],
        buildings: [
            (id: "core:building/barracks", cost: 8, effects: [
                (scope: Unit, selector: EventUnit, trigger: OnUnitTrained, modifier: AddPromotion("core:promotion/veteran"), duration: Once),
            ]),
            (id: "core:building/forge", cost: 8, effects: [
                (scope: City, selector: SourceCity, trigger: OnBuilt, modifier: FreeUnit(unit: "core:unit/warrior", count: 1), duration: Once),
                (scope: City, selector: SourceCity, modifier: ModifyCost(target: Units, percent: -30)),
            ]),
            (id: "core:building/market", cost: 8, effects: [
                (scope: City, selector: SourceCity, modifier: AddYieldPercent(yield_kind: Output, percent: 25)),
                (scope: City, selector: OwnerCities, modifier: AddYield(yield_kind: Output, amount: 1), stacking: Unique),
                (scope: City, selector: SourceCity, modifier: ModifyCost(target: Hurry, percent: -20)),
            ]),
            (id: "core:building/shrine", cost: 8, effects: [
                (scope: City, selector: SourceCity, trigger: OnTurnStart, modifier: AddYield(yield_kind: Culture, amount: 3), duration: Once),
                (scope: City, selector: SourceCity, modifier: AddYieldPerPopulation(yield_kind: Output, hundredths: 50)),
            ]),
        ],
        technologies: [
            (id: "core:tech/agriculture", cost: 20, effects: [
                (scope: City, selector: OwnerCities, modifier: AddYieldPercent(yield_kind: Output, percent: 10)),
                (scope: City, selector: OwnerCities, trigger: OnTechnologyUnlocked, modifier: AddYield(yield_kind: Output, amount: 4), duration: Turns(3), stacking: Unique),
            ]),
            (id: "core:tech/pottery", cost: 40, effects: [
                (scope: City, selector: EventCity, trigger: OnCityFounded, modifier: FreeBuilding("core:building/shrine"), duration: Once),
                (scope: Player, selector: Owner, trigger: OnUnitKilled, modifier: AddYield(yield_kind: Gold, amount: 10), duration: Once),
                (scope: City, selector: OwnerCities, modifier: MultiplyYield(yield_kind: Output, percent: 110), stacking: Unique),
            ]),
        ],
    )"#,
    )
    .map(with_classic_terrains)
    .expect("effects fuzz fixture must parse")
}

/// Production and hurry orders on top of the generic command mix.
fn effects_command(rng: &mut FuzzRng, game: &GameState, rules: &CompiledRuleset) -> Command {
    let player = pick_player(rng, game);
    match rng.below(10) {
        0..=1 => {
            let index = rng.below(rules.buildings.len() as u64 + rules.units.len() as u64);
            let item = if (index as usize) < rules.buildings.len() {
                ProductionItem::Building(BuildingKindId(index as u16))
            } else {
                ProductionItem::Unit(UnitKindId((index as usize - rules.buildings.len()) as u16))
            };
            let city = pick_city(rng, game);
            Command::EnqueueProduction { player, city, item }
        }
        2 => Command::HurryProduction {
            player,
            city: pick_city(rng, game),
        },
        _ => random_command(rng, game),
    }
}

#[test]
fn fuzz_effects_validate_every_commit_and_roundtrip_saves() {
    use classic_rules::persistence::{decode::decode, encode::encode};
    let rules = compile_rules(effects_content()).unwrap();
    let mut rng = FuzzRng(0xEFFEC7);
    let mut promoted = 0usize;
    let mut timed_seen = false;
    let mut buildings = 0usize;
    // 60 worlds (30 before): with the minimal city distance 3 (Unciv:
    // ModConstants.kt minimalCityDistance) fewer random FoundCity orders
    // succeed, and 30 worlds no longer built a single unit (no promotion).
    for _ in 0..60 {
        let mut game = random_world(&mut rng, &rules);
        for step in 0..200 {
            let cmd = effects_command(&mut rng, &game, &rules);
            let before = authoritative_hash(&game);
            match game.execute(&rules, cmd) {
                Ok(_) => game.validate(&rules).expect("valid after commit"),
                Err(_) => assert_eq!(
                    before,
                    authoritative_hash(&game),
                    "rejected command mutated truth"
                ),
            }
            timed_seen |= !game.view().auth.active_effects().is_empty();
            if step % 50 == 49 {
                let saved = encode(&game, &rules);
                let loaded = decode(&saved, &rules).expect("own save loads");
                assert_eq!(
                    authoritative_hash(&loaded),
                    authoritative_hash(&game),
                    "save roundtrip"
                );
            }
        }
        let cities = game.view().auth.cities();
        buildings += cities.iter().map(|(_, c)| c.buildings.len()).sum::<usize>();
        let units = game.view().auth.units();
        promoted += units
            .iter_cores()
            .filter(|u| !u.promotions.is_empty())
            .count();
    }
    assert!(promoted > 0, "fuzz must reach promotions (coverage)");
    assert!(buildings > 0, "fuzz must complete buildings (coverage)");
    assert!(timed_seen, "fuzz must start timed effects (coverage)");
}
