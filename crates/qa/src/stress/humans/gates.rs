//! The gates of the human tricks, written before their first run (22 Nov 2026) and not changed after a
//! result. classic/docs/ai-stress.md, "Defence against humans", holds the same table (the test
//! `human_gates_match_the_document`). The outcome gates read the targeted AI nation against the
//! control: its cities, its share of the score after the window, its real-victory weight at the end
//! of the game; the behaviour gates are diagnostics.

use super::Trick;
use crate::stress::gates::{Gate, Kind, Test};
use crate::stress::Situation;

const fn gate(trick: Trick, kind: Kind, metric: &'static str, test: Test, threshold: [f64; 3], net: &'static str) -> Gate {
    Gate { situation: Situation::Human(trick), kind, metric, test, threshold, net }
}

use Kind::{Behaviour as B, Outcome as O};
use Test::{DeltaAtLeast as DGE, DeltaAtMost as DLE, EditedAtLeast as EGE, EditedAtMost as ELE};
use Trick::{Bait, Beg, Drain, Lobby, OffCitadel, OffLobby, OffPoach, OffRaid, OffRush, OffSneak, OffVulture, Poach, Rush, Sneak, Sniper, Squat, Swindle, Turtle, Vulture};

const NET_FIGHT: &str = "danger net (on: the fight fixes), tactician, commander (off); scripted defence.rs";
const NET_SNEAK: &str = "strategist (posture: no trust term for an army inside the borders), danger net (on: the fight fixes); scripted defence.rs";
const NET_LAND: &str = "tactician, commander (off); none: scripted (planner_workers.rs repair, defence.rs)";
const NET_NEW_CITY: &str = "site net (settling beside an army; on in nets-x-site), danger net (on: the fight fixes); scripted defence.rs";
const NET_DEALS: &str = "none: scripted (peace.rs Negotiation::value: no memory of a betrayal, diplomacy.rs)";
const NET_PEACE: &str = "none: scripted (peace.rs Negotiation::answer)";
const NET_POSTURE: &str = "strategist (posture: war on a weak rich nation)";
const NET_WONDER: &str = "none: scripted wonder_race.rs; strategist production head";
const NET_CONGRESS: &str = "none: scripted (congress.rs, city_states.rs, victory_watch.rs); strategist has no diplomacy outputs";
const NET_ALLIES: &str = "none: scripted (city_states.rs, espionage.rs); strategist has no diplomacy outputs";

pub const GATES: &[Gate] = &[
    gate(Rush, O, "seat_alive", DGE, [-0.02, -0.05, -0.1], NET_FIGHT),
    gate(Rush, O, "seat_cities", DGE, [-0.2, -0.4, -0.6], NET_FIGHT),
    gate(Rush, O, "seat_score_share", DGE, [-0.01, -0.02, -0.03], NET_FIGHT),
    gate(Rush, O, "end_seat_win_weight", DGE, [-0.03, -0.05, -0.08], NET_FIGHT),
    gate(Rush, B, "capital_lost", ELE, [0.1, 0.2, 0.3], NET_FIGHT),
    gate(Rush, B, "army_dead", EGE, [0.5, 0.5, 0.5], NET_FIGHT),
    gate(Sneak, O, "seat_cities", DGE, [-0.3, -0.5, -0.8], NET_SNEAK),
    gate(Sneak, O, "seat_score_share", DGE, [-0.01, -0.02, -0.03], NET_SNEAK),
    gate(Sneak, O, "end_seat_win_weight", DGE, [-0.03, -0.05, -0.08], NET_SNEAK),
    gate(Sneak, B, "military_added_before_war", DGE, [1.0, 1.0, 2.0], NET_SNEAK),
    gate(Sneak, B, "army_dead", EGE, [0.5, 0.5, 0.5], NET_SNEAK),
    gate(Squat, O, "seat_score_share", DGE, [-0.005, -0.01, -0.02], NET_LAND),
    gate(Squat, O, "seat_cities", DGE, [-0.1, -0.2, -0.3], NET_LAND),
    gate(Squat, O, "end_seat_win_weight", DGE, [-0.02, -0.03, -0.05], NET_LAND),
    gate(Squat, B, "army_dead", EGE, [0.5, 0.5, 0.5], NET_LAND),
    gate(Squat, B, "script_squat_share", ELE, [0.3, 0.3, 0.3], NET_LAND),
    gate(Squat, B, "unhappy_rounds", DLE, [3.0, 5.0, 8.0], NET_LAND),
    gate(Bait, O, "seat_cities", DGE, [-0.1, -0.2, -0.3], NET_FIGHT),
    gate(Bait, O, "seat_score_share", DGE, [-0.005, -0.01, -0.02], NET_FIGHT),
    gate(Bait, O, "end_seat_win_weight", DGE, [-0.02, -0.03, -0.05], NET_FIGHT),
    gate(Bait, B, "script_lured", ELE, [1.0, 1.0, 1.0], NET_FIGHT),
    gate(Bait, B, "army_dead", EGE, [0.5, 0.5, 0.5], NET_FIGHT),
    gate(Vulture, O, "seat_cities", DGE, [-0.2, -0.3, -0.4], NET_NEW_CITY),
    gate(Vulture, O, "seat_score_share", DGE, [-0.01, -0.015, -0.02], NET_NEW_CITY),
    gate(Vulture, O, "end_seat_win_weight", DGE, [-0.03, -0.05, -0.08], NET_NEW_CITY),
    gate(Vulture, B, "new_city_lost", ELE, [0.2, 0.3, 0.4], NET_NEW_CITY),
    gate(Vulture, B, "army_dead", EGE, [0.5, 0.5, 0.5], NET_NEW_CITY),
    gate(Swindle, O, "seat_treasury", DGE, [-50.0, -100.0, -150.0], NET_DEALS),
    gate(Swindle, O, "seat_score_share", DGE, [-0.01, -0.015, -0.02], NET_DEALS),
    gate(Swindle, O, "end_seat_win_weight", DGE, [-0.02, -0.03, -0.05], NET_DEALS),
    gate(Swindle, B, "script_paid", ELE, [0.3, 0.2, 0.1], NET_DEALS),
    gate(Swindle, B, "script_paid_again", ELE, [0.05, 0.05, 0.05], NET_DEALS),
    gate(Drain, O, "seat_cities", DGE, [-0.1, -0.2, -0.3], NET_DEALS),
    gate(Drain, O, "seat_score_share", DGE, [-0.005, -0.01, -0.015], NET_DEALS),
    gate(Drain, O, "end_seat_win_weight", DGE, [-0.02, -0.03, -0.05], NET_DEALS),
    gate(Drain, B, "script_paid", ELE, [0.2, 0.2, 0.2], NET_DEALS),
    gate(Drain, B, "purchases", DGE, [-1.0, -1.5, -2.0], NET_DEALS),
    gate(Beg, O, "seat_cities", DGE, [-0.3, -0.3, -0.3], NET_PEACE),
    gate(Beg, O, "seat_score_share", DGE, [-0.01, -0.01, -0.01], NET_PEACE),
    gate(Beg, O, "end_seat_win_weight", DGE, [-0.03, -0.03, -0.03], NET_PEACE),
    gate(Beg, B, "script_white_peace", ELE, [0.3, 0.3, 0.3], NET_PEACE),
    gate(Turtle, O, "seat_cities", DGE, [0.05, 0.1, 0.15], NET_POSTURE),
    gate(Turtle, O, "end_seat_win_weight", DGE, [0.0, 0.0, 0.0], NET_POSTURE),
    gate(Turtle, B, "turtle_attacked", EGE, [0.5, 0.6, 0.7], NET_POSTURE),
    gate(Turtle, B, "turtle_cities_lost", DGE, [0.2, 0.35, 0.5], NET_POSTURE),
    gate(Sniper, O, "seat_score_share", DGE, [-0.02, -0.03, -0.03], NET_WONDER),
    gate(Sniper, O, "end_seat_win_weight", DGE, [-0.03, -0.03, -0.03], NET_WONDER),
    gate(Sniper, B, "stock_burned", DLE, [0.0, 0.0, 0.0], NET_WONDER),
    gate(Lobby, O, "seat_score_share", DGE, [-0.01, -0.02, -0.03], NET_CONGRESS),
    gate(Lobby, O, "end_seat_win_weight", DGE, [-0.05, -0.1, -0.15], NET_CONGRESS),
    gate(Lobby, B, "seat_allies", DGE, [-1.0, -1.5, -2.0], NET_CONGRESS),
    gate(Lobby, B, "embargo_on_seat", ELE, [0.2, 0.3, 0.4], NET_CONGRESS),
    gate(Lobby, B, "gifts_by_seat", DGE, [1.0, 1.0, 1.0], NET_CONGRESS),
    gate(Poach, O, "seat_score_share", DGE, [-0.005, -0.01, -0.015], NET_ALLIES),
    gate(Poach, O, "end_seat_win_weight", DGE, [-0.03, -0.05, -0.08], NET_ALLIES),
    gate(Poach, B, "seat_allies", DGE, [-0.5, -1.0, -1.5], NET_ALLIES),
    gate(Poach, B, "gifts_by_seat", DGE, [1.0, 1.0, 1.0], NET_ALLIES),
];

const OFF_WAR: &str = "strategist (posture, war stakes: no opportunity term), commander (off); scripted operations.rs";
const OFF_RAID: &str = "tactician, commander (off); none: scripted (field_craft.rs not in standard, operations.rs)";
const OFF_CITADEL: &str = "none: scripted great_people.rs (a general waits without a site)";
const OFF_GOLD: &str = "none: scripted (city_states.rs gift_orders, congress.rs); strategist has no diplomacy outputs";

/// The offence gates (written before the offence's first run, 22 Nov 2026): the AI seat that could
/// play the trick, against the control without the opportunity. The behaviour gates read whether it
/// took the opportunity, the outcome gates whether it gained.
pub const OFFENCE: &[Gate] = &[
    gate(OffRush, O, "seat_cities", DGE, [0.1, 0.2, 0.3], OFF_WAR),
    gate(OffRush, O, "seat_score_share", DGE, [0.0, 0.005, 0.01], OFF_WAR),
    gate(OffRush, O, "end_seat_win_weight", DGE, [0.0, 0.0, 0.0], OFF_WAR),
    gate(OffRush, B, "war_on_rival", DGE, [0.3, 0.4, 0.5], OFF_WAR),
    gate(OffRush, B, "rival_capital_taken", EGE, [0.1, 0.2, 0.3], OFF_WAR),
    gate(OffSneak, O, "seat_cities", DGE, [0.1, 0.2, 0.3], OFF_WAR),
    gate(OffSneak, O, "seat_score_share", DGE, [0.0, 0.005, 0.01], OFF_WAR),
    gate(OffSneak, O, "end_seat_win_weight", DGE, [0.0, 0.0, 0.0], OFF_WAR),
    gate(OffSneak, B, "war_on_rival", DGE, [0.2, 0.3, 0.4], OFF_WAR),
    gate(OffSneak, B, "rival_cities_taken", DGE, [0.1, 0.2, 0.3], OFF_WAR),
    gate(OffRaid, O, "seat_score_share", DGE, [0.002, 0.004, 0.006], OFF_RAID),
    gate(OffRaid, O, "end_seat_win_weight", DGE, [0.0, 0.0, 0.0], OFF_RAID),
    gate(OffRaid, B, "rival_civilians_end", DLE, [-0.5, -1.0, -1.5], OFF_RAID),
    gate(OffRaid, B, "rival_pillaged_end", DGE, [1.0, 2.0, 3.0], OFF_RAID),
    gate(OffCitadel, O, "seat_score_share", DGE, [0.002, 0.003, 0.005], OFF_CITADEL),
    gate(OffCitadel, O, "end_seat_win_weight", DGE, [0.0, 0.0, 0.0], OFF_CITADEL),
    gate(OffCitadel, B, "seat_citadels", DGE, [0.5, 1.0, 1.0], OFF_CITADEL),
    gate(OffCitadel, B, "rival_cities_taken", DGE, [0.0, 0.0, 0.2], OFF_CITADEL),
    gate(OffVulture, O, "seat_cities", DGE, [0.2, 0.4, 0.5], OFF_WAR),
    gate(OffVulture, O, "seat_score_share", DGE, [0.0, 0.005, 0.01], OFF_WAR),
    gate(OffVulture, O, "end_seat_win_weight", DGE, [0.0, 0.0, 0.0], OFF_WAR),
    gate(OffVulture, B, "rival_cities_taken", DGE, [0.3, 0.5, 0.6], OFF_WAR),
    gate(OffLobby, O, "seat_score_share", DGE, [0.0, 0.002, 0.004], OFF_GOLD),
    gate(OffLobby, O, "end_seat_win_weight", DGE, [0.0, 0.0, 0.0], OFF_GOLD),
    gate(OffLobby, B, "gifts_by_seat", DGE, [1.0, 2.0, 3.0], OFF_GOLD),
    gate(OffLobby, B, "seat_allies", DGE, [0.5, 1.0, 1.5], OFF_GOLD),
    gate(OffLobby, B, "embargo_proposals_by_seat", DGE, [0.2, 0.3, 0.4], OFF_GOLD),
    gate(OffPoach, O, "seat_score_share", DGE, [0.0, 0.002, 0.004], OFF_GOLD),
    gate(OffPoach, O, "end_seat_win_weight", DGE, [0.0, 0.0, 0.0], OFF_GOLD),
    gate(OffPoach, B, "gifts_by_seat", DGE, [1.0, 2.0, 3.0], OFF_GOLD),
    gate(OffPoach, B, "seat_allies", DGE, [0.5, 1.0, 1.5], OFF_GOLD),
];

/// The gates as the document's table.
pub fn markdown() -> String {
    crate::stress::gates::markdown_of(GATES)
}

/// The offence gates as the document's table.
pub fn offence_markdown() -> String {
    crate::stress::gates::markdown_of(OFFENCE)
}
