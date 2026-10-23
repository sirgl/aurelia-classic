//! Fuzz of the culture, policy, religion and tourism commands (track B of
//! M4) on small classic worlds: random and AI commands, the world validated
//! after every commit (derived yields against their rebuild oracle
//! included), rejected commands leave the hash untouched, save → load keeps
//! the hash. Public API only.

use classic_rules::ClassicGame;
use classic_content::core::core_rules;
use classic_rules::ai::{AiState, GreedyPlanner, Planner};
use classic_rules::ids::{
    BeliefId, BuildingKindId, CityHandle, PlayerId, PolicyId, ProductionItem, UnitHandle,
    UnitKindId,
};
use classic_rules::persistence::{decode::decode, encode::encode};
use classic_rules::replay::authoritative_hash;
use classic_rules::rules::content::PolicyBranchId;
use classic_rules::rules::content::ReligionId;
use classic_rules::scenario::{generate_new_game, ScenarioConfig};
use classic_rules::{compile_rules, Command, CompiledRuleset, GameState};

struct FuzzRng(u64);

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

fn world(rng: &mut FuzzRng, rules: &CompiledRuleset) -> GameState {
    let cfg = ScenarioConfig {
        seed: rng.next(),
        players: 2 + rng.below(2) as u8,
        width: 20 + rng.below(9) as u32,
        height: 16 + rng.below(9) as u32,
        cities: 0,
        units: 0,
        water_ratio: rng.below(25) as u8,
    };
    generate_new_game(rules, &cfg)
}

/// A player: mostly the active one, sometimes anyone ("not your turn").
fn pick_player(rng: &mut FuzzRng, game: &GameState) -> PlayerId {
    let players = game.view().auth.players().count() as u64;
    if rng.below(6) == 0 {
        PlayerId(rng.below(players + 1) as u8)
    } else {
        game.view().auth.active_player()
    }
}

/// A unit of the active player or a stale handle.
fn pick_unit(rng: &mut FuzzRng, game: &GameState) -> UnitHandle {
    let active = game.view().auth.active_player();
    let units = game.view().auth.indexes().units_of(active);
    if units.is_empty() || rng.below(8) == 0 {
        return UnitHandle {
            index: 9000 + rng.below(100) as u32,
            generation: 0,
        };
    }
    units[rng.below(units.len() as u64) as usize]
}

/// A city of anyone or a stale handle.
fn pick_city(rng: &mut FuzzRng, game: &GameState) -> CityHandle {
    let cities: Vec<CityHandle> = game.view().auth.cities().iter().map(|(h, _)| h).collect();
    if cities.is_empty() || rng.below(8) == 0 {
        return CityHandle {
            index: 9000 + rng.below(100) as u32,
            generation: 0,
        };
    }
    cities[rng.below(cities.len() as u64) as usize]
}

fn belief(rng: &mut FuzzRng, rules: &CompiledRuleset) -> BeliefId {
    BeliefId(rng.below(rules.module.beliefs.len() as u64 + 2) as u16)
}

/// Random religious commands (founding with a random religion and beliefs,
/// spreads, purchases, great person improvements).
fn religion_command(rng: &mut FuzzRng, game: &GameState, rules: &CompiledRuleset) -> Command {
    let player = pick_player(rng, game);
    match rng.below(7) {
        0 => Command::FoundPantheon {
            player,
            belief: belief(rng, rules),
        },
        1 => Command::FoundReligion {
            unit: pick_unit(rng, game),
            religion: ReligionId(rng.below(rules.module.religions.len() as u64 + 1) as u16),
            founder: belief(rng, rules),
            follower: belief(rng, rules),
            pantheon: if rng.below(2) == 0 {
                None
            } else {
                Some(belief(rng, rules))
            },
            bonus: None,
        },
        2 => Command::EnhanceReligion {
            unit: pick_unit(rng, game),
            follower: belief(rng, rules),
            enhancer: belief(rng, rules),
        },
        3 => Command::SpreadReligion {
            unit: pick_unit(rng, game),
        },
        4 => Command::RemoveHeresy {
            unit: pick_unit(rng, game),
        },
        5 => {
            let item = if rng.below(2) == 0 {
                ProductionItem::Unit(UnitKindId(rng.below(rules.units.len() as u64) as u16))
            } else {
                ProductionItem::Building(BuildingKindId(
                    rng.below(rules.buildings.len() as u64) as u16
                ))
            };
            Command::PurchaseWithFaith {
                player,
                city: pick_city(rng, game),
                item,
            }
        }
        _ => Command::CreateGreatImprovement {
            unit: pick_unit(rng, game),
        },
    }
}

/// Commands of the active player: the AI's choices (founding, moves,
/// research, policies, religion) or random culture and religion commands
/// (ids may be out of range).
/// A random great work, treatise, concert or ideology switch.
fn tourism_command(rng: &mut FuzzRng, game: &GameState) -> Command {
    let unit = pick_unit(rng, game);
    let sites = &game.view().auth.module().tourism().archaeology().sites;
    let site = sites
        .get(rng.below(sites.len() as u64 + 1) as usize)
        .map(|s| s.tile)
        .unwrap_or(classic_rules::ids::TileId(rng.below(64) as u32));
    let choices = classic_rules::derived::archaeology::ArchaeologyChoice::ALL;
    match rng.below(6) {
        0 => Command::CreateGreatWork { unit },
        1 => Command::WriteTreatise { unit },
        2 => Command::PerformConcert { unit },
        3 => Command::Excavate { unit },
        4 => Command::ChooseArchaeology {
            player: pick_player(rng, game),
            tile: site,
            choice: choices[rng.below(choices.len() as u64) as usize],
        },
        _ => Command::SwitchIdeology {
            player: pick_player(rng, game),
        },
    }
}

/// Now and then a great person of culture appears next to a city of the
/// active player, and a building with great work slots in one of its
/// cities (great people are born by track A's mechanic).
fn culture_people(rng: &mut FuzzRng, game: &mut GameState, rules: &CompiledRuleset) {
    let active = game.view().auth.active_player();
    let cities = game.view().auth.indexes().cities_of(active).to_vec();
    let Some(city) = cities
        .get(rng.below(cities.len().max(1) as u64) as usize)
        .copied()
    else {
        return;
    };
    let tile = game.view().auth.cities().get(city).unwrap().tile;
    let people = [
        "core:unit/great-writer",
        "core:unit/great-artist",
        "core:unit/great-musician",
        "core:unit/archaeologist",
    ];
    let key = people[rng.below(4) as usize];
    let kind = rules.units.iter().position(|u| u.key == key).unwrap();
    // Archaeologists start on a site (or at home before the sites appear).
    let at = match (
        key,
        game.view()
            .auth
            .module()
            .tourism()
            .archaeology()
            .sites
            .first(),
    ) {
        ("core:unit/archaeologist", Some(site)) => site.tile,
        _ => tile,
    };
    game.debug_spawn_unit(rules, active, at, classic_rules::ids::UnitKindId(kind as u16));
    if rng.below(4) == 0 {
        let archaeology = rules
            .technologies
            .iter()
            .position(|t| t.key == "core:tech/archaeology");
        game.debug_learn(
            rules,
            active,
            classic_rules::ids::TechId(archaeology.unwrap() as u16),
        );
    }
    let slotted = [
        "core:building/amphitheater",
        "core:building/museum",
        "core:building/opera_house",
    ];
    let key = slotted[rng.below(3) as usize];
    let building = rules.buildings.iter().position(|b| b.key == key).unwrap();
    let building = classic_rules::ids::BuildingKindId(building as u16);
    if !game
        .view()
        .auth
        .cities()
        .get(city)
        .unwrap()
        .buildings
        .contains(&building)
    {
        game.debug_add_building(rules, city, building);
    }
}

fn commands(rng: &mut FuzzRng, game: &GameState, rules: &CompiledRuleset) -> Vec<Command> {
    let active = game.view().auth.active_player();
    match rng.below(100) {
        0..=24 => GreedyPlanner.plan(game.ai_observation(rules, active), rules),
        25..=34 => {
            classic_rules::ai::policies::policy_command(
                &game.ai_observation(rules, active),
                rules,
                &AiState::new(active, 0),
            )
            .into_iter()
            .collect()
        }
        35..=44 => {
            classic_rules::ai::religion::religion_commands(&game.ai_observation(rules, active), rules)
        }
        45..=49 => vec![religion_command(rng, game, rules)],
        50..=59 => vec![Command::AdoptPolicy {
            player: pick_player(rng, game),
            policy: PolicyId(rng.below(rules.module.policies.len() as u64 + 2) as u16),
        }],
        60..=69 => vec![Command::UnlockPolicyBranch {
            player: pick_player(rng, game),
            branch: PolicyBranchId(rng.below(rules.module.policy_branches.len() as u64 + 2) as u16),
        }],
        70..=74 => vec![Command::ChooseIdeology {
            player: pick_player(rng, game),
            ideology: PolicyBranchId(rng.below(rules.module.policy_branches.len() as u64 + 1) as u16),
        }],
        75..=79 => {
            classic_rules::ai::tourism::tourism_commands(&game.ai_observation(rules, active), rules)
        }
        80..=83 => vec![tourism_command(rng, game)],
        _ => vec![Command::EndTurn {
            player: pick_player(rng, game),
        }],
    }
}

fn check(game: &mut GameState, rules: &CompiledRuleset, cmd: Command) {
    let before = authoritative_hash(game);
    match game.execute(rules, cmd.clone()) {
        Ok(_) => game
            .validate(rules)
            .unwrap_or_else(|e| panic!("world invalid after {cmd:?}: {e}")),
        Err(_) => assert_eq!(
            before,
            authoritative_hash(game),
            "rejected {cmd:?} mutated truth"
        ),
    }
}

fn run(game: &mut GameState, rules: &CompiledRuleset, rng: &mut FuzzRng, steps: usize) {
    for step in 0..steps {
        // Culture and faith arrive faster than in play, so policies get
        // adopted and religions founded.
        if rng.below(8) == 0 {
            let player = PlayerId(rng.below(game.view().auth.players().count() as u64) as u8);
            game.debug_grant_culture(player, 40 + rng.below(400) as u32);
            game.debug_grant_faith(player, 20 + rng.below(300) as u32);
        }
        if rng.below(12) == 0 {
            culture_people(rng, game, rules);
        }
        for cmd in commands(rng, game, rules) {
            check(game, rules, cmd);
        }
        if step % 40 == 39 {
            let loaded = decode(&encode(game, rules), rules).expect("own save loads");
            assert_eq!(
                authoritative_hash(&loaded),
                authoritative_hash(game),
                "save/load"
            );
        }
    }
}

#[test]
fn fuzz_culture_commands_keep_the_world_valid() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut rng = FuzzRng(0xC0_17_0E);
    let mut adopted = 0;
    let mut pantheons = 0;
    let mut works = 0;
    let mut digs = 0;
    for _ in 0..12 {
        let mut game = world(&mut rng, &rules);
        game.validate(&rules).unwrap();
        run(&mut game, &rules, &mut rng, 160);
        let auth = game.view().auth;
        for p in 0..auth.players().count() {
            adopted += auth.module().culture().of(PlayerId(p as u8)).policies.len();
        }
        pantheons += usize::from(auth.module().religion().pantheons());
        works += auth.module().tourism().works().len();
        let archaeology = auth.module().tourism().archaeology();
        digs += archaeology.pending.len() + archaeology.landmarks.len() + archaeology.digs.len();
    }
    assert!(
        adopted >= 20,
        "the fuzz reaches policy adoption ({adopted})"
    );
    assert!(pantheons >= 4, "the fuzz reaches pantheons ({pantheons})");
    assert!(works >= 10, "the fuzz creates great works ({works})");
    assert!(digs >= 3, "the fuzz reaches digs ({digs})");
    eprintln!(
        "fuzz_culture: policies {adopted}, pantheons {pantheons}, works {works}, digs {digs}"
    );
}
