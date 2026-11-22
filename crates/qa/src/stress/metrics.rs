//! The numbers of an arm: the outcome of the nations the situation judges (the primary measures:
//! `seat_*`, the same in every situation) and the behaviour the situation expects (the secondary
//! diagnostics, named for the expectation). Shares are 0 or 1 a position; the report averages them.

use std::collections::BTreeMap;

use classic_rules::ids::PlayerId;
use classic_rules::world::congress::ResolutionKind;
use classic_rules::world::victory::VictoryKind;
use classic_rules::{ClassicGame, CompiledRuleset, GameState};

use super::scene::Roles;
use super::watch::{Event, Watch};
use crate::bench::modules::majors;

pub type Measures = BTreeMap<String, f64>;

/// The measures of an arm after `window` rounds from round `start`.
pub fn measure(roles: &Roles, rules: &CompiledRuleset, game: &GameState, watch: &Watch, start: u32, window: u32) -> Measures {
    let mut m = outcome(roles, rules, game, watch);
    let none = f64::from(window + 1);
    // The rounds after the start of the first event that `pick` finds (none: window + 1).
    let first = |events: &mut dyn Iterator<Item = u32>| events.map(|r| r.saturating_sub(start).max(1)).min().map_or(none, f64::from);
    let flag = |b: bool| f64::from(u8::from(b));
    let count = |n: usize| n as f64;
    match *roles {
        Roles::Neighbour { weak, near } => {
            let lost: Vec<_> = watch.captures.iter().filter(|c| c.from.0 == weak).collect();
            m.insert("weak_captured".into(), flag(!lost.is_empty()));
            m.insert("weak_cities_lost".into(), count(lost.len()));
            m.insert("first_capture".into(), first(&mut lost.iter().map(|c| c.round)));
            m.insert("near_captured".into(), count(lost.iter().filter(|c| c.to.0 == near).count()));
            let declared = watch.declared.iter().any(|e| e.on.0 == weak);
            m.insert("nobody_attacked".into(), flag(!declared && lost.is_empty()));
        }
        Roles::Invasion { seat, enemy } => {
            m.insert("cities_lost".into(), count(watch.captures.iter().filter(|c| c.from.0 == seat).count()));
            m.insert("first_counter".into(), first(&mut watch.first_hurt.into_iter()));
            // Before the fix of 22 Nov `counter_by_3` was this (an invader hurt by anyone, a city's strike
            // included); it is kept as `invader_hurt_by_3`. `counter_by_3` is a strike of a unit of the seat
            // on the enemy (melee or ranged; a city's strike is not one) within three rounds.
            m.insert("invader_hurt_by_3".into(), flag(watch.first_hurt.is_some_and(|r| r.saturating_sub(start) <= 3)));
            m.insert("counter_by_3".into(), flag(watch.strikes.iter().any(|e| e.by.0 == seat && e.on.0 == enemy && e.round.saturating_sub(start) <= 3)));
            m.insert("invaders_dead".into(), f64::from(watch.tracked_dead));
            m.insert("purchases".into(), count(watch.purchases.iter().filter(|(_, p)| p.0 == seat).count()));
        }
        Roles::Runaway { leader } => runaway(&mut m, rules, game, watch, leader),
        Roles::Lost { victim, taker, capital } => {
            let back = watch.captures.iter().filter(|c| c.city == capital && c.to.0 == victim);
            m.insert("capital_retaken".into(), flag(watch.captures.iter().any(|c| c.city == capital && c.to.0 == victim)));
            m.insert("retake_round".into(), first(&mut back.map(|c| c.round)));
            m.insert("further_losses".into(), count(watch.captures.iter().filter(|c| c.from.0 == victim && c.city != capital).count()));
            m.insert("peace".into(), flag(!game.view().auth.diplomacy().at_war(PlayerId(victim), PlayerId(taker))));
        }
        Roles::Race { leader } => {
            m.insert("rival_wins_science".into(), flag(watch.victory.is_some_and(|(_, w, k)| w.0 == leader && k == VictoryKind::Science)));
            m.insert("rival_cities_lost".into(), count(watch.captures.iter().filter(|c| c.from.0 == leader).count()));
        }
        Roles::Common { a, b, enemy } => common(&mut m, watch, (a, b, enemy), start, window),
        Roles::Seat { .. } => {}
        Roles::Host { .. } => {
            let dead = watch.hosts.last().is_some_and(|(_, host, alive)| host.is_some() && !alive);
            m.insert("host_dead_at_end".into(), flag(dead));
            m.insert("dead_host_rounds".into(), count(watch.hosts.iter().filter(|(_, _, alive)| !alive).count()));
        }
    }
    m
}

/// The outcome at the end of the game of the nations in focus: their cities and share of the score,
/// and the victory weight (a real victory of one of them 1, a time victory a quarter, else 0).
pub fn end_outcome(roles: &Roles, rules: &CompiledRuleset, game: &GameState) -> Measures {
    let auth = game.view().auth;
    let focus = roles.focus(game);
    let n = focus.len().max(1) as f64;
    let total: i64 = majors(game).iter().map(|p| classic_rules::world::victory::score(auth, rules, *p)).sum::<i64>().max(1);
    let score: i64 = focus.iter().map(|p| classic_rules::world::victory::score(auth, rules, *p)).sum();
    let cities: usize = focus.iter().map(|p| auth.indexes().cities_of(*p).len()).sum();
    let weight = match game.outcome() {
        Some(o) if focus.contains(&o.winner) => if o.kind == VictoryKind::Time { 0.25 } else { 1.0 },
        _ => 0.0,
    };
    let mut m = Measures::new();
    m.insert("end_seat_cities".into(), cities as f64 / n);
    m.insert("end_seat_score_share".into(), score as f64 / total as f64);
    m.insert("end_seat_win_weight".into(), weight);
    m
}

/// The outcome of the nations in focus: their cities, technologies and share of the score (means over
/// the nations), whether one of them won by a real victory, and how many are alive.
fn outcome(roles: &Roles, rules: &CompiledRuleset, game: &GameState, watch: &Watch) -> Measures {
    let auth = game.view().auth;
    let focus = roles.focus(game);
    let n = focus.len().max(1) as f64;
    let total: i64 = majors(game).iter().map(|p| classic_rules::world::victory::score(auth, rules, *p)).sum::<i64>().max(1);
    let score: i64 = focus.iter().map(|p| classic_rules::world::victory::score(auth, rules, *p)).sum();
    let cities: usize = focus.iter().map(|p| auth.indexes().cities_of(*p).len()).sum();
    let techs: usize = focus.iter().map(|p| auth.players().techs_of(*p).len()).sum();
    let won = watch.victory.is_some_and(|(_, w, k)| k != VictoryKind::Time && focus.contains(&w));
    let mut m = Measures::new();
    m.insert("seat_cities".into(), cities as f64 / n);
    m.insert("seat_techs".into(), techs as f64 / n);
    m.insert("seat_score_share".into(), score as f64 / total as f64);
    m.insert("seat_won".into(), f64::from(u8::from(won)));
    m.insert("seat_alive".into(), focus.iter().filter(|p| auth.players().is_alive(**p)).count() as f64 / n);
    m
}

fn runaway(m: &mut Measures, _rules: &CompiledRuleset, game: &GameState, watch: &Watch, leader: u8) {
    let auth = game.view().auth;
    let others: Vec<PlayerId> = majors(game).into_iter().filter(|p| p.0 != leader).collect();
    let techs = |p: PlayerId| auth.players().techs_of(p).len() as f64;
    let mean = others.iter().map(|p| techs(*p)).sum::<f64>() / others.len().max(1) as f64;
    let on_leader = |e: &&Event| e.on.0 == leader && e.by.0 != leader;
    m.insert("tech_gap".into(), techs(PlayerId(leader)) - mean);
    m.insert("wars_on_leader".into(), watch.declared.iter().filter(on_leader).count() as f64);
    m.insert("denouncements_of_leader".into(), watch.denounced.iter().filter(on_leader).count() as f64);
    m.insert("research_pacts_others".into(), watch.pacts.iter().filter(|e| e.by.0 != leader && e.on.0 != leader).count() as f64);
    let embargo = watch.proposals.iter().filter(|p| p.kind == ResolutionKind::PlayerEmbargo && p.choice == u32::from(leader) && p.by.0 != leader).count();
    m.insert("embargo_proposals_on_leader".into(), embargo as f64);
    m.insert("leader_won".into(), f64::from(u8::from(watch.victory.is_some_and(|(_, w, k)| w.0 == leader && k != VictoryKind::Time))));
}

fn common(m: &mut Measures, watch: &Watch, (a, b, enemy): (u8, u8, u8), start: u32, window: u32) {
    let first = |who: u8| watch.strikes.iter().filter(|e| e.by.0 == who && e.on.0 == enemy).map(|e| e.round.saturating_sub(start).max(1)).min();
    let (fa, fb) = (first(a), first(b));
    m.insert("both_struck".into(), f64::from(u8::from(fa.is_some() && fb.is_some())));
    m.insert("strike_gap".into(), match (fa, fb) {
        (Some(x), Some(y)) => f64::from(x.abs_diff(y)),
        _ => f64::from(window),
    });
    let peace = |who: u8| watch.peaces.iter().any(|e| e.by.0 == who && e.on.0 == enemy);
    m.insert("separate_peace".into(), f64::from(u8::from(peace(a) != peace(b))));
}
