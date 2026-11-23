//! `net2`'s research on the spaceship's way (`strategy_net::heads::space_way`, the `space_research` lever of
//! `Net2Plan`): with the lever the research head sees only the available technologies on the way to the
//! spaceship (the technologies that unlock its projects and parts, and their missing prerequisites); without
//! the lever, without a `net2` plan, or when no available technology is on the way, it sees every available
//! one. Built in every build: the game's AI plays `net2` since the sixteenth promotion.

use std::collections::BTreeSet;

use classic_content::core::core_rules;
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::net2::Net2Plan;
use classic_rules::ai::planner::TurnBrief;
use classic_rules::ai::state::AiState;
use classic_rules::ai::strategy_net::heads::space_way;
use classic_rules::ai::strategy_net::sharp::space_techs;
use classic_rules::ai::variant::Variant;
use classic_rules::ids::{PlayerId, TechId};
use classic_rules::scenario::{generate_new_game, ScenarioConfig};
use classic_rules::{compile_rules, CompiledRuleset};
use kernel::ai::mind::AiMind;

const US: PlayerId = PlayerId(0);

fn rules() -> CompiledRuleset {
    compile_rules(core_rules()).unwrap()
}

/// The brief of the first seat's first turn of a fresh 4-seat game.
fn first_brief(rules: &CompiledRuleset) -> TurnBrief {
    let game = generate_new_game(rules, &ScenarioConfig { seed: 0x5BACE, players: 4, width: 40, height: 32, cities: 0, units: 0, water_ratio: 15 });
    let mind = ClassicMind::new(Variant::Standard);
    let mut state = AiState::new(US, game.view().auth.turn().round);
    let sight = mind.sense(&game, rules, US);
    mind.start_turn(&sight, rules, US, &mut state).brief
}

/// The way computed apart from the head: the spaceship's technologies and every prerequisite, less the known.
fn way(rules: &CompiledRuleset, known: &[TechId]) -> BTreeSet<u16> {
    let mut out = BTreeSet::new();
    let mut stack: Vec<TechId> = space_techs(rules);
    while let Some(t) = stack.pop() {
        if known.contains(&t) || !out.insert(t.0) {
            continue;
        }
        stack.extend(rules.technologies[usize::from(t.0)].requires.iter().copied());
    }
    out
}

fn with_lever(mut brief: TurnBrief, on: bool) -> TurnBrief {
    brief.net2 = Some(Net2Plan { space_research: on, ..Net2Plan::default() });
    brief
}

#[test]
fn the_lever_keeps_the_research_on_the_ships_way() {
    let rules = rules();
    let brief = first_brief(&rules);
    assert!(!space_techs(&rules).is_empty(), "the rules have a spaceship");
    let way = way(&rules, &brief.techs);
    let all: Vec<TechId> = (0..rules.technologies.len() as u16).map(TechId).filter(|t| !brief.techs.contains(t)).collect();
    let (on, off): (Vec<TechId>, Vec<TechId>) = all.iter().partition(|t| way.contains(&t.0));
    assert!(!on.is_empty() && !off.is_empty(), "the tree has technologies on the way and off it: {} on, {} off", on.len(), off.len());

    // With the lever: of every unknown technology offered, only the ones on the way stay, in their order.
    let lever = with_lever(brief.clone(), true);
    assert_eq!(space_way(&lever, &rules, all.clone()), on);
    // One technology on the way among the ones off it: it alone stays.
    let mut mixed = off.clone();
    mixed.push(on[0]);
    assert_eq!(space_way(&lever, &rules, mixed), vec![on[0]]);
}

#[test]
fn without_a_tech_on_the_way_or_without_the_lever_every_available_one_stays() {
    let rules = rules();
    let brief = first_brief(&rules);
    let way = way(&rules, &brief.techs);
    let all: Vec<TechId> = (0..rules.technologies.len() as u16).map(TechId).filter(|t| !brief.techs.contains(t)).collect();
    let off: Vec<TechId> = all.iter().copied().filter(|t| !way.contains(&t.0)).collect();
    assert!(!off.is_empty());

    // The fallback: nothing available is on the way, so every available technology stays.
    assert_eq!(space_way(&with_lever(brief.clone(), true), &rules, off.clone()), off);
    // The way is done (every technology known but the ones off it): every available one stays.
    let mut done = with_lever(brief.clone(), true);
    done.techs = (0..rules.technologies.len() as u16).map(TechId).filter(|t| !off.contains(t)).collect();
    assert!(way_of(&rules, &done).is_empty());
    assert_eq!(space_way(&done, &rules, off.clone()), off);
    // No lever, or no plan of `net2` (every variant but `net2`'s): the list is untouched.
    assert_eq!(space_way(&with_lever(brief.clone(), false), &rules, all.clone()), all);
    let mut none = brief.clone();
    none.net2 = None;
    assert_eq!(space_way(&none, &rules, all.clone()), all);
    // An empty list stays empty (the head refuses before it asks).
    assert!(space_way(&with_lever(brief, true), &rules, Vec::new()).is_empty());
}

fn way_of(rules: &CompiledRuleset, brief: &TurnBrief) -> BTreeSet<u16> {
    way(rules, &brief.techs)
}
