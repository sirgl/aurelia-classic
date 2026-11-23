//! `science-flow` (work/nets/science-flow/SCIENCE-FLOW.md): the science specialists' executor reads `net2`'s level and
//! keeps each city's food; the research agreement's ties lift `net2`'s denial for the friendship and the agreement
//! only, get the ladder at their cadence, and keep the agreement's gold (the rules' cost) in the allocator's chest until
//! the partner signs or the friendship ends; `standard` plays none of it. The tests name classic content only to set
//! the scene.

use classic_content::core::core_rules;
use classic_rules::ai::allocator::Allocator;
use classic_rules::ai::citizens::science_specialist_commands;
use classic_rules::ai::diplomacy::{DiplomacyAdvisor, PROPOSAL_INTERVAL};
use classic_rules::ai::net2::plan::{ChestWait, Help, Net2Plan, RivalRead, Science, ScienceMemory, Soldiers, Tie, WarKind};
use classic_rules::ai::net2::science;
use classic_rules::ai::planner::TurnBrief;
use classic_rules::ai::state::AiState;
use classic_rules::ai::system::{AdvisorInput, SystemAdvisor};
use classic_rules::ai::variant::Variant;
use classic_rules::command::Command;
use classic_rules::ids::{BuildingKindId, CityHandle, PlayerId, TechId, TileId};
use classic_rules::math::HexCoord;
use classic_rules::world::relations::DealItem;
use classic_rules::world::state::GameConfig;
use classic_rules::{compile_rules, ClassicGame, CompiledRuleset, GameState};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
}

fn building(rules: &CompiledRuleset, key: &str) -> BuildingKindId {
    BuildingKindId(rules.buildings.iter().position(|b| b.key == key).unwrap() as u16)
}

/// One city of 8 citizens with a library and a university (2 scientist slots), every technology to the Renaissance.
fn scholar() -> (GameState, CompiledRuleset, CityHandle) {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(&rules, GameConfig { players: 2, width: 40, height: 30, rng_seed: 5 });
    let renaissance = rules.eras.iter().find(|e| e.key == "core:era/renaissance").unwrap().index;
    for (i, tech) in rules.technologies.iter().enumerate() {
        if tech.content_era.and_then(|e| rules.eras.get(e.0 as usize)).map_or(0, |e| e.index) <= renaissance {
            game.debug_learn(&rules, P0, TechId(i as u16));
        }
    }
    let city = game.debug_found_city(&rules, P0, tile(&game, 5, 5));
    game.debug_found_city(&rules, P1, tile(&game, 30, 20));
    game.debug_set_population(&rules, city, 8);
    game.debug_add_building(&rules, city, building(&rules, "core:building/library"));
    game.debug_add_building(&rules, city, building(&rules, "core:building/university"));
    (game, rules, city)
}

/// The scientists the executor sets in the city at `level` with `food` a turn to spare (none: no command).
fn scientists(level: i64, food: i32) -> Option<u8> {
    let (game, rules, city) = scholar();
    let mut obs = game.ai_observation(&rules, P0);
    obs.own_cities.iter_mut().find(|c| c.handle == city).unwrap().income.food_surplus = food;
    let commands = science_specialist_commands(&obs, &rules, level, 2);
    let scientist = rules.specialists.iter().position(|s| s.key == "core:specialist/scientist").unwrap() as u16;
    commands.iter().find_map(|c| match c {
        Command::SetSpecialists { city: at, specialist, count, .. } if *at == city && specialist.0 == scientist => Some(*count),
        _ => None,
    })
}

/// The level fills its share of the slots while the food stays at the floor; a city at the floor takes none; level 0
/// leaves the rules' own assignment (no command).
#[test]
fn the_scientists_follow_the_level_and_keep_the_food() {
    assert_eq!(scientists(1000, 20), Some(2), "fed: both slots");
    assert_eq!(scientists(500, 20), Some(1), "half the level, half the slots");
    assert_eq!(scientists(1000, 2), None, "at the floor no tile's food can go");
    assert_eq!(scientists(0, 20), None, "no level: the rules assign");
}

fn read(rival: PlayerId, help: Help) -> RivalRead {
    RivalRead {
        rival,
        weak: 0,
        reach: 0,
        menace: 0,
        distrust: 0,
        prey: 0,
        runaway: 900,
        spoiler: 0,
        retake: 0,
        duty: 0,
        grabber: 0,
        block: 0,
        border: 0,
        brake: 0,
        war_value: 0,
        war_kind: WarKind::Prey,
        trust: 1000,
        help,
        soldiers: Soldiers::Unread,
    }
}

/// The runaway is denied every help; a tie takes the friendship and the agreement out of the denial, nothing else.
#[test]
fn a_tie_lifts_the_denial_for_the_friendship_and_the_agreement_only() {
    let mut plan = Net2Plan { rivals: vec![read(P1, Help::Deny)], ..Net2Plan::default() };
    for item in [DealItem::Friendship, DealItem::ResearchAgreement, DealItem::OpenBorders, DealItem::DefensivePact] {
        assert!(plan.denies_item(P1, item), "{item:?} denied without a tie");
    }
    plan.science = Science { ties: vec![Tie { rival: P1, every: 1 }], ..Science::default() };
    assert!(!plan.denies_item(P1, DealItem::Friendship) && !plan.denies_item(P1, DealItem::ResearchAgreement));
    assert!(plan.denies_item(P1, DealItem::OpenBorders) && plan.denies_item(P1, DealItem::DefensivePact));
}

/// `standard` and every other variant keep the science flow off; `science-flow` is `standard` with it, and
/// `early-growth` is `science-flow` with the opening read (`net2::weights::g::OPENING`).
#[test]
fn only_science_flow_plays_the_science_flow() {
    assert!(Variant::ALL.iter().all(|v| v.science_flow() == matches!(v, Variant::ScienceFlow | Variant::EarlyGrowth)));
    assert!(Variant::ALL.iter().all(|v| v.early_growth() == (*v == Variant::EarlyGrowth)));
    assert_eq!(Variant::EarlyGrowth.net2_parts(), Variant::Standard.net2_parts());
    assert_eq!(Variant::EarlyGrowth.net2_offence(), Variant::Standard.net2_offence());
    assert_eq!(Variant::ScienceFlow.net2_parts(), Variant::Standard.net2_parts());
    assert_eq!(Variant::ScienceFlow.net2_offence(), Variant::Standard.net2_offence());
    assert_eq!(Net2Plan::default().science, Science::default());
}

// ---- the ladder, the chest and its cost ----

/// P0 and P1 met, each with a city and 5000 gold; P0 knows every technology to the Renaissance (research agreements
/// allowed), P1 every one to `p1_era`; embassies both ways and a friendship. P0 is the active player in a round that is
/// not its round of proposals.
fn friends(p1_era: &str, speed: &str) -> (GameState, CompiledRuleset) {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(&rules, GameConfig { players: 2, width: 40, height: 30, rng_seed: 5 });
    game.set_speed(&rules, speed);
    let index = |key: &str| rules.eras.iter().find(|e| e.key == key).unwrap().index;
    let era_of = |tech: &classic_rules::rules::compiled::TechnologyDefinition| tech.content_era.and_then(|e| rules.eras.get(e.0 as usize)).map_or(0, |e| e.index);
    for (i, tech) in rules.technologies.iter().enumerate() {
        if era_of(tech) <= index("core:era/renaissance") {
            game.debug_learn(&rules, P0, TechId(i as u16));
        }
        if era_of(tech) <= index(p1_era) {
            game.debug_learn(&rules, P1, TechId(i as u16));
        }
    }
    game.debug_found_city(&rules, P0, tile(&game, 5, 5));
    game.debug_found_city(&rules, P1, tile(&game, 30, 20));
    game.debug_meet(P0, P1);
    game.debug_earn(P0, 5000);
    game.debug_earn(P1, 5000);
    game.debug_deal_item(&rules, P0, P1, DealItem::Embassy).unwrap();
    game.debug_deal_item(&rules, P1, P0, DealItem::Embassy).unwrap();
    game.debug_deal_item(&rules, P0, P1, DealItem::Friendship).unwrap();
    end_round(&mut game, &rules);
    while (game.view().auth.turn().round + u32::from(P0.0)) % PROPOSAL_INTERVAL == 0 {
        end_round(&mut game, &rules);
    }
    while game.view().auth.active_player() != P0 {
        let active = game.view().auth.active_player();
        game.execute(&rules, Command::EndTurn { player: active }).unwrap();
    }
    (game, rules)
}

fn end_round(game: &mut GameState, rules: &CompiledRuleset) {
    let round = game.view().auth.turn().round;
    while game.view().auth.turn().round == round {
        let active = game.view().auth.active_player();
        game.execute(rules, Command::EndTurn { player: active }).unwrap();
    }
}

fn tied(every: u32) -> Net2Plan {
    Net2Plan { round: 1, science: Science { ties: vec![Tie { rival: P1, every }], ..Science::default() }, ..Net2Plan::default() }
}

/// Between the seat's own rounds of proposals the diplomacy offers a tie the next step of its ladder (here the
/// agreement) in the rounds of the tie's cadence, and nothing to a rival that is no tie.
#[test]
fn the_ladder_is_offered_at_the_ties_cadence() {
    let (game, rules) = friends("core:era/renaissance", "core:speed/standard");
    let round = game.view().auth.turn().round;
    let agreements = |net2: Option<Net2Plan>| {
        let obs = game.ai_observation(&rules, P0);
        let mut brief = TurnBrief::from_observation(&obs, &rules);
        brief.net2 = net2;
        let state = AiState::new(P0, obs.round);
        let input = AdvisorInput { observation: &obs, brief: &brief, state: &state, rules: &rules };
        DiplomacyAdvisor::default().start_of_turn(&input).into_iter().filter(|p| matches!(&p.command, Command::ProposeDeal { to, items, .. } if *to == P1 && items.contains(&(P0, DealItem::ResearchAgreement)))).count()
    };
    assert_eq!(agreements(None), 0, "not the seat's round: today nothing");
    assert_eq!(agreements(Some(Net2Plan { round: 1, ..Net2Plan::default() })), 0, "no tie, no ladder");
    assert_eq!(agreements(Some(tied(1))), 1, "a sure tie asks in every round");
    let off_beat = (2..PROPOSAL_INTERVAL + 2).find(|every| (round + u32::from(P0.0)) % every != 0).unwrap();
    assert_eq!(agreements(Some(tied(off_beat))), 0, "a tie every {off_beat} rounds does not ask in round {round}");
}

/// The chest reads the agreement's cost as the rules charge it: the higher era of the two at the game speed's gold
/// percent (P1 a later era than ours, Quick speed), the gold the rules take when the agreement is signed.
#[test]
fn the_chest_reads_the_rules_cost_of_the_higher_era() {
    let (mut game, rules) = friends("core:era/modern", "core:speed/quick");
    let read = science::agreement_cost(&game.ai_observation(&rules, P0), &rules, P1);
    let ours = rules.eras.iter().find(|e| e.key == "core:era/renaissance").unwrap().research_agreement_cost;
    assert!(read > 0 && read != ours, "the read cost {read} is not our era's {ours}");
    let before = game.view().auth.players().treasury_of(P0);
    game.debug_deal_item(&rules, P0, P1, DealItem::ResearchAgreement).unwrap();
    assert_eq!(before - game.view().auth.players().treasury_of(P0), read, "the rules charged what the chest keeps");
}

/// A tie that is a friend without an agreement holds its cost in the chest and waits a round a turn; the wait feeds
/// `r::CHEST_WAIT`; the agreement signed empties the chest and starts the wait again.
#[test]
fn the_chest_holds_a_friends_gold_and_counts_its_wait_until_it_signs() {
    let (mut game, rules) = friends("core:era/renaissance", "core:speed/standard");
    let ties = [Tie { rival: P1, every: 1 }];
    let mut memory = ScienceMemory::default();
    let obs = game.ai_observation(&rules, P0);
    let cost = science::agreement_cost(&obs, &rules, P1);
    assert_eq!(science::chest(&obs, &rules, &ties, &mut memory), cost);
    assert_eq!(science::chest(&obs, &rules, &[], &mut ScienceMemory::default()), 0, "no tie, no chest");
    assert_eq!(science::chest(&obs, &rules, &ties, &mut memory), cost, "a second pass in the round");
    assert_eq!(memory.waits, vec![ChestWait { rival: P1, rounds: 1 }], "adds no round");
    end_round(&mut game, &rules);
    let obs = game.ai_observation(&rules, P0);
    science::chest(&obs, &rules, &ties, &mut memory);
    assert_eq!(memory.waits, vec![ChestWait { rival: P1, rounds: 2 }]);
    let mut rivals = vec![classic_rules::ai::net2::plan::RivalInputs::stranger(P1)];
    science::read_waits(&memory, &obs, &mut rivals);
    assert_eq!(rivals[0].chest_wait, 100, "2 of 20 rounds");
    game.debug_deal_item(&rules, P0, P1, DealItem::ResearchAgreement).unwrap();
    end_round(&mut game, &rules);
    let obs = game.ai_observation(&rules, P0);
    assert_eq!(science::chest(&obs, &rules, &ties, &mut memory), 0, "signed: nothing to keep");
    assert!(memory.waits.is_empty(), "the wait starts again");
}

/// The allocator keeps the plan's chest out of its purchases, on top of what it keeps without one.
#[test]
fn the_allocator_keeps_the_chest() {
    let (game, rules) = friends("core:era/renaissance", "core:speed/standard");
    let obs = game.ai_observation(&rules, P0);
    let state = AiState::new(P0, obs.round);
    let kept = |chest: u32| {
        let mut brief = TurnBrief::from_observation(&obs, &rules);
        brief.net2 = Some(Net2Plan { science: Science { chest, ..Science::default() }, ..Net2Plan::default() });
        Allocator::new(&obs, &brief, &state, &rules).war_chest()
    };
    assert_eq!(kept(300), kept(0) + 300);
}
