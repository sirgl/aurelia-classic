//! The situation exam (`aurelia_qa::stress`, classic/docs/ai-stress.md): each edit does what it says,
//! the control is untouched, a scene plays the same twice, a saved scene rebuilds to its hash, and the
//! gates in the code are the gates in the document. Positions are a tiny game of the smoke suite.

use std::path::PathBuf;
use std::sync::OnceLock;

use aurelia_qa::bench::modules::{majors, BenchRules, ClassicBench};
use aurelia_qa::bench::suite::Suite;
use aurelia_qa::export::{play_log_until, suite_specs_with, write_log};
use aurelia_qa::stress::{self, edit, gates, play, position, scene, Situation};
use classic_rules::ai::variant::Variant;
use classic_rules::{ClassicGame, CompiledRuleset};
use kernel::replay::hash::authoritative_hash;

const ROUND: u32 = 40;

fn rules() -> &'static CompiledRuleset {
    static RULES: OnceLock<CompiledRuleset> = OnceLock::new();
    RULES.get_or_init(ClassicBench::rules)
}

/// The log of the smoke suite's game, its first 45 rounds, written once.
fn log() -> &'static PathBuf {
    static LOG: OnceLock<PathBuf> = OnceLock::new();
    LOG.get_or_init(|| {
        let spec = suite_specs_with(&Suite::named("smoke").expect("the smoke suite"), 1, 0, None, Variant::Standard).remove(0);
        let played = play_log_until(rules(), &spec, ROUND + 5).expect("the game plays");
        let dir = std::env::temp_dir().join(format!("ai-stress-test-{}", std::process::id()));
        write_log(&dir, &spec, &played).expect("the log is written")
    })
}

fn position() -> position::Position {
    position::load(rules(), log(), ROUND).expect("the position loads")
}

fn hash(game: &classic_rules::GameState) -> u64 {
    authoritative_hash(game)
}

#[test]
fn a5_removes_the_military_of_the_weak_and_leaves_the_control_alone() {
    let (rules, p) = (rules(), position());
    let plan = scene::plan(Situation::A5, rules, &p, 0).expect("two nations share the smoke map");
    let scene::Roles::Neighbour { weak, .. } = plan.roles else { panic!("A5 plans a neighbour") };
    let before = edit::military_units(&p.game, rules, classic_rules::ids::PlayerId(weak)).len();
    let strong = scene::edit_of(Situation::A5, 2, rules, &p, &plan).expect("the edit");
    let (control, edited) = scene::arms(rules, &p, &plan, &strong).expect("the arms");
    assert_eq!(hash(&control.game), hash(&p.game), "the control is the position");
    assert_eq!(edit::military_units(&edited.game, rules, classic_rules::ids::PlayerId(weak)).len(), 0, "all {before} units are gone");
    let mild = scene::edit_of(Situation::A5, 0, rules, &p, &plan).expect("the edit");
    let (_, half) = scene::arms(rules, &p, &plan, &mild).expect("the arms");
    let left = edit::military_units(&half.game, rules, classic_rules::ids::PlayerId(weak)).len();
    assert!(left <= before && (before < 2 || left > 0), "the mild edit removes some, not all ({before} -> {left})");
}

#[test]
fn b1_puts_the_enemy_units_in_the_land_of_the_seat() {
    let (rules, p) = (rules(), position());
    let plan = scene::plan(Situation::B1, rules, &p, 0).expect("a war is there or is made");
    let scene::Roles::Invasion { seat, enemy } = plan.roles else { panic!("B1 plans an invasion") };
    let edit_of = scene::edit_of(Situation::B1, 1, rules, &p, &plan).expect("the edit");
    let (control, edited) = scene::arms(rules, &p, &plan, &edit_of).expect("the arms");
    assert_eq!(edited.tracked.len(), 7, "the medium level puts in seven units");
    let auth = edited.game.view().auth;
    for id in &edited.tracked {
        let unit = auth.units().get(auth.units().handle_of(kernel::ids::EntityId(*id)).expect("the unit exists")).expect("the unit");
        assert_eq!(unit.core.owner.0, enemy);
        assert_eq!(auth.map().owner(unit.core.tile).flatten().map(|o| o.0), Some(seat), "inside the seat's land");
    }
    let units = |g: &classic_rules::GameState| g.view().auth.indexes().units_of(classic_rules::ids::PlayerId(enemy)).len();
    assert_eq!(units(&edited.game), units(&control.game) + 7);
}

#[test]
fn b4_gives_the_capital_to_the_taker_only_in_the_edited_arm() {
    let (rules, p) = (rules(), position());
    let plan = scene::plan(Situation::B4, rules, &p, 0).expect("two nations share the smoke map");
    let scene::Roles::Lost { victim, taker, capital } = plan.roles else { panic!("B4 plans a loss") };
    let edit_of = scene::edit_of(Situation::B4, 0, rules, &p, &plan).expect("the edit");
    let (control, edited) = scene::arms(rules, &p, &plan, &edit_of).expect("the arms");
    let owner = |g: &classic_rules::GameState| {
        let auth = g.view().auth;
        auth.cities().get(auth.cities().handle_of(kernel::ids::EntityId(capital)).expect("the city")).expect("the city").owner.0
    };
    assert_eq!(owner(&control.game), victim, "the control keeps its capital");
    assert_eq!(owner(&edited.game), taker, "the edited arm lost it");
    assert!(edited.game.view().auth.diplomacy().at_war(classic_rules::ids::PlayerId(victim), classic_rules::ids::PlayerId(taker)), "the base war is in both arms");
}

#[test]
fn a1_gift_makes_the_leader_research_faster() {
    let (rules, p) = (rules(), position());
    let plan = scene::plan(Situation::A1, rules, &p, 0).expect("a leader");
    let scene::Roles::Runaway { leader } = plan.roles else { panic!("A1 plans a runaway") };
    let edit_of = scene::edit_of(Situation::A1, 2, rules, &p, &plan).expect("the edit");
    assert!(edit_of.changes.is_empty(), "the world is not changed: the gift is the edit");
    let (control, edited) = scene::arms(rules, &p, &plan, &edit_of).expect("the arms");
    assert_eq!(hash(&control.game), hash(&edited.game), "the gift is made in the play loop, not at the start");
    let progress = |arm: play::Arm| {
        let played = play::play_arm(rules, arm, p.variant, 8, false).expect("plays");
        let auth = played.window.view().auth;
        let l = classic_rules::ids::PlayerId(leader);
        (auth.players().techs_of(l).len(), auth.players().science_of(l))
    };
    assert!(progress(edited) > progress(control), "a doubled science is ahead after eight rounds");
}

#[test]
fn an_instance_plays_the_same_twice() {
    let (rules, p) = (rules(), position());
    let run = || stress::run_instance(rules, &p, 0, &[Situation::B4, Situation::A5], None, false);
    let (a, b) = (run(), run());
    assert!(!a.rows.is_empty(), "something was built: {:?}", a.unbuilt);
    let json = |d: &stress::Done| serde_json::to_string(&d.rows).expect("rows serialise");
    assert_eq!(json(&a), json(&b));
}

#[test]
fn a_saved_scene_rebuilds_to_its_hash() {
    let (rules, p) = (rules(), position());
    let dir = std::env::temp_dir().join(format!("ai-stress-scenes-{}", std::process::id()));
    let done = stress::run_instance(rules, &p, 0, &[Situation::B4], Some(&dir), false);
    assert!(!done.rows.is_empty(), "built: {:?}", done.unbuilt);
    let path = dir.join("b4-000-L2.json");
    let saved: stress::Saved = serde_json::from_slice(&std::fs::read(&path).expect("the scene is saved")).expect("it parses");
    let game = stress::rebuild(rules, &saved).expect("it rebuilds");
    assert_eq!(format!("{:016x}", hash(&game)), saved.world);
    let mut broken = saved.clone();
    broken.world = "0000000000000000".to_owned();
    assert!(stress::rebuild(rules, &broken).is_err(), "a wrong hash is refused");
    assert!(majors(&game).len() >= 2);
}

#[test]
fn gates_match_the_document() {
    let doc = std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/ai-stress.md")).expect("the document");
    assert!(doc.contains(&gates::markdown()), "docs/ai-stress.md holds the gates table of `ai-stress gates`");
}

/// Two majors of the position: the first two by id.
fn two(p: &position::Position) -> (classic_rules::ids::PlayerId, classic_rules::ids::PlayerId) {
    let all = majors(&p.game);
    (all[0], all[1])
}

#[test]
fn a_later_change_waits_for_its_round() {
    let (rules, p) = (rules(), position());
    let (a, b) = two(&p);
    let changes = vec![edit::Change::Meet { a: a.0, b: b.0 }, edit::Change::Later { after: 3, then: Box::new(edit::Change::War { a: a.0, b: b.0 }) }];
    let mut game = p.game.clone();
    edit::apply_all(&mut game, rules, &changes).expect("the changes apply");
    assert!(!game.view().auth.diplomacy().at_war(a, b), "nothing at the start");
    let arm = |rounds: u32| {
        let arm = play::Arm { game: game.clone(), gift: None, tracked: Vec::new(), later: edit::later(&changes), script: None };
        play::play_arm(rules, arm, p.variant, rounds, false).expect("plays").window
    };
    assert!(!arm(2).view().auth.diplomacy().at_war(a, b), "not yet after two rounds");
    let after = arm(3);
    let peace_made = after.view().auth.diplomacy().at_war(a, b) || after.view().auth.module().relations().peace_treaty_until(a, b) > 0;
    assert!(peace_made, "the war was declared at the start of the third round");
}

#[test]
fn treasury_adds_and_drains() {
    let (rules, p) = (rules(), position());
    let (a, _) = two(&p);
    let mut game = p.game.clone();
    let before = game.view().auth.players().treasury_of(a);
    edit::apply(&mut game, rules, &edit::Change::Treasury { player: a.0, gold: 500 }).expect("gold in");
    assert_eq!(game.view().auth.players().treasury_of(a), before + 500);
    edit::apply(&mut game, rules, &edit::Change::Treasury { player: a.0, gold: i32::MIN }).expect("gold out");
    assert_eq!(game.view().auth.players().treasury_of(a), 0, "drained to an empty treasury");
}

#[test]
fn a_citadel_takes_the_tiles_around_it_and_a_wonder_ends_the_race() {
    let (rules, p) = (rules(), position());
    let (a, b) = two(&p);
    let auth = p.game.view().auth;
    let map = auth.map();
    // A tile of `b`'s land next to a tile of `a`'s land: the citadel of `a` there takes `b`'s tiles.
    let city = auth.indexes().cities_of(a).first().and_then(|h| auth.cities().get(*h)).map(|c| c.tile).expect("a city of a");
    let tile = map.dimensions.neighbors(city).into_iter().find(|t| auth.indexes().city_on_tile(*t).is_none() && map.land_area(*t).is_some()).expect("a land tile next to the city");
    let citadel = rules.improvements.iter().position(|d| d.culture_bomb_radius > 0).expect("the rules hold a citadel") as u16;
    let mut game = p.game.clone();
    edit::apply(&mut game, rules, &edit::Change::Improve { player: a.0, tile: tile.0, improvement: citadel }).expect("raised");
    assert_eq!(game.view().auth.map().improvement(tile).flatten().map(|i| i.0), Some(citadel));
    assert_eq!(game.view().auth.map().owner(tile).flatten(), Some(a));
    let wonder = (0..rules.buildings.len()).find(|i| rules.buildings[*i].wonder && rules.buildings[*i].required_tech.is_none_or(|t| game.view().auth.players().techs_of(b).contains(&t))).expect("a wonder b can build") as u16;
    edit::apply(&mut game, rules, &edit::Change::Wonder { player: b.0, building: wonder }).expect("the wonder is built");
    let capital = classic_rules::combat::city::capital_of(game.view().auth, rules, b).expect("b's capital");
    assert!(game.view().auth.cities().get(capital).expect("the city").buildings.iter().any(|k| k.0 == wonder));
    assert!(edit::apply(&mut game, rules, &edit::Change::Wonder { player: a.0, building: wonder }).is_err(), "a world wonder is built once");
}

#[test]
fn the_raid_script_plays_its_units_on_the_seat_turn() {
    let (rules, p) = (rules(), position());
    let plan = scene::plan(Situation::P1, rules, &p, 0).expect("two nations share the smoke map");
    let scene::Roles::Raid { raider, .. } = plan.roles else { panic!("P1 plans a raid") };
    let mild = scene::edit_of(Situation::P1, 0, rules, &p, &plan).expect("the edit");
    let (control, edited) = scene::arms(rules, &p, &plan, &mild).expect("the arms");
    assert!(control.script.is_none());
    let Some(stress::seat::Script::Raid { seat, units, .. }) = edited.script.clone() else { panic!("the edited arm has the raid") };
    assert_eq!((seat, units.len()), (raider, 2), "the script plays the two raiders");
    let start: Vec<_> = units.iter().map(|id| edited.game.view().auth.units().handle_of(kernel::ids::EntityId(*id)).and_then(|h| edited.game.view().auth.units().get(h)).map(|u| u.core.tile)).collect();
    let played = play::play_arm(rules, edited, p.variant, 2, false).expect("plays");
    let auth = played.window.view().auth;
    let moved = units.iter().zip(&start).any(|(id, at)| auth.units().handle_of(kernel::ids::EntityId(*id)).and_then(|h| auth.units().get(h)).map(|u| u.core.tile) != *at);
    assert!(moved || played.script.values().any(|v| *v > 0.0), "the raiders moved, captured or pillaged: {:?}", played.script);
}

#[test]
fn every_situation_builds_or_says_why() {
    let (rules, p) = (rules(), position());
    for situation in Situation::ALL {
        let Ok(plan) = scene::plan(situation, rules, &p, 0) else { continue };
        for level in 0..situation.levels() {
            if let Ok(edit_of) = scene::edit_of(situation, level, rules, &p, &plan) {
                scene::arms(rules, &p, &plan, &edit_of).unwrap_or_else(|e| panic!("{} level {level}: {e}", situation.name()));
            }
        }
    }
}

#[test]
fn b5_friendship_holds_in_the_control_and_the_war_ends_it() {
    let (rules, p) = (rules(), position());
    let plan = scene::plan(Situation::B5, rules, &p, 0).expect("the smoke pair is at peace");
    let scene::Roles::Sneak { seat, enemy, .. } = plan.roles else { panic!("B5 plans a sneak war") };
    let mild = scene::edit_of(Situation::B5, 0, rules, &p, &plan).expect("the edit");
    let (control, edited) = scene::arms(rules, &p, &plan, &mild).expect("the arms");
    let (s, e) = (classic_rules::ids::PlayerId(seat), classic_rules::ids::PlayerId(enemy));
    let friends = |g: &classic_rules::GameState| g.view().auth.module().relations().are_friends(s, e, g.view().auth.turn().round);
    assert!(friends(&control.game), "the control keeps the friendship of the base");
    assert!(!friends(&edited.game), "the war ends the friendship (end_ties): the rules keep no memory of it");
    let b2 = scene::plan(Situation::B2, rules, &p, 0).expect("B2 plans where B5 does");
    let (b2_control, _) = scene::arms(rules, &p, &b2, &scene::edit_of(Situation::B2, 0, rules, &p, &b2).expect("the edit")).expect("the arms");
    assert_ne!(hash(&control.game), hash(&b2_control.game), "the friendship is in the world's hash");
}

#[test]
fn the_kill_zone_bait_is_left_alone_by_its_seat_ai() {
    let (rules, p) = (rules(), position());
    let plan = scene::plan(Situation::P6, rules, &p, 0).expect("two nations share the smoke map");
    let mild = scene::edit_of(Situation::P6, 0, rules, &p, &plan).expect("the edit");
    let (_, edited) = scene::arms(rules, &p, &plan, &mild).expect("the arms");
    let Some(stress::seat::Script::KillZone { units, .. }) = edited.script.clone() else { panic!("the edited arm has the kill-zone") };
    assert_eq!(units.len(), 4, "three zone units and the bait");
    let bait = *units.last().expect("the bait is spawned last");
    let at = |g: &classic_rules::GameState| g.view().auth.units().handle_of(kernel::ids::EntityId(bait)).and_then(|h| g.view().auth.units().get(h)).map(|u| (u.core.tile, u.core.owner));
    let start = at(&edited.game).expect("the bait stands");
    let played = play::play_arm(rules, edited, p.variant, 3, false).expect("plays");
    let now = at(&played.window);
    assert!(now.is_none() || now == Some(start), "the bait never moves: its seat's AI leaves it alone ({start:?} -> {now:?})");
}

#[test]
fn the_world_leader_vote_can_be_set_one_delegate_short() {
    let (rules, p) = (rules(), position());
    let (a, _) = two(&p);
    let mut game = p.game.clone();
    game.debug_united_nations(rules, a, 3);
    let short_one = game.debug_delegates_short_of_victory(rules, a, 1);
    let needed = game.ai_observation(rules, a).congress.expect("the congress sits").needed;
    assert_eq!(short_one + 1, needed, "one delegate short of the World Leader");
    assert_eq!(game.debug_delegates_short_of_victory(rules, a, 3), short_one, "never fewer than it holds");
}

#[test]
fn spies_are_added_to_a_nation() {
    let (rules, p) = (rules(), position());
    let (a, _) = two(&p);
    let mut game = p.game.clone();
    let before = game.view().auth.module().espionage().spies_of(a).len();
    edit::apply(&mut game, rules, &edit::Change::Spies { player: a.0, count: 2 }).expect("spies");
    assert_eq!(game.view().auth.module().espionage().spies_of(a).len(), before + 2);
}
