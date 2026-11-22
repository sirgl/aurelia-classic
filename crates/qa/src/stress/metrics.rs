//! The numbers of an arm: the outcome of the nations the situation judges (the primary measures:
//! `seat_*`, the same in every situation) and the behaviour the situation expects (the secondary
//! diagnostics, named for the expectation). Shares are 0 or 1 a position; the report averages them.

use std::collections::BTreeMap;

use classic_rules::ids::{BuildingKindId, PlayerId, ProductionItem};
use classic_rules::world::player::ClassicPlayers;
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
            m.insert("capital_lost".into(), flag(capital_lost(game, watch, seat)));
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
        Roles::Seat { seat } => {
            m.insert("seat_recaptures".into(), count(watch.captures.iter().filter(|c| c.to.0 == seat).count()));
            m.insert("seat_cities_lost".into(), count(watch.captures.iter().filter(|c| c.from.0 == seat).count()));
        }
        Roles::Sneak { seat, enemy, war_after } => sneak(&mut m, watch, (seat, enemy, war_after), start, window),
        Roles::Weak { seat } => weak(&mut m, watch, seat),
        Roles::Snipe { city, building, .. } => snipe(&mut m, watch, city, building),
        Roles::Grab { seat, grabber, tile } => {
            let city = game.view().auth.indexes().city_on_tile(classic_rules::ids::TileId(tile)).and_then(|h| game.view().auth.cities().get(h));
            m.insert("forward_city_held".into(), flag(city.is_some_and(|c| c.owner.0 == grabber)));
            m.insert("forward_city_taken".into(), flag(city.is_some_and(|c| c.owner.0 == seat)));
            m.insert("seat_tiles".into(), count(owned_tiles(game, seat)));
            m.insert("war_by_seat".into(), flag(watch.declared.iter().any(|e| e.by.0 == seat && e.on.0 == grabber)));
        }
        Roles::Raid { seat, .. } => {
            let last = watch.of(seat).last().copied();
            m.insert("raiders_dead".into(), f64::from(watch.tracked_dead) / watch.followed().max(1) as f64);
            m.insert("pillaged_end".into(), last.map_or(0.0, |s| f64::from(s.pillaged)));
            m.insert("civilians_end".into(), last.map_or(0.0, |s| f64::from(s.civilians)));
        }
        Roles::Trick { seat, trickster } => {
            m.insert("at_war_with_trickster".into(), flag(!watch.war_rounds(seat, trickster).is_empty()));
            m.insert("cities_taken".into(), count(watch.captures.iter().filter(|c| c.to.0 == seat && c.from.0 == trickster).count()));
            m.insert("cities_lost".into(), count(watch.captures.iter().filter(|c| c.from.0 == seat).count()));
        }
        Roles::Ally { seat, ally, enemy } => {
            m.insert("ally_at_war".into(), flag(!watch.war_rounds(ally, enemy).is_empty()));
            m.insert("ally_struck".into(), flag(watch.strikes.iter().any(|e| e.by.0 == ally && e.on.0 == enemy)));
            let alone = game.view().auth.diplomacy().at_war(PlayerId(seat), PlayerId(enemy)) && !game.view().auth.diplomacy().at_war(PlayerId(ally), PlayerId(enemy));
            m.insert("ally_left".into(), flag(alone && !watch.war_rounds(ally, enemy).is_empty()));
            m.insert("cities_lost".into(), count(watch.captures.iter().filter(|c| c.from.0 == seat).count()));
        }
        Roles::Losing { seat, enemy } => {
            let at_war = watch.war_rounds(seat, enemy);
            let first_peace = watch.wars.iter().find(|(_, w)| !w.contains(&(seat.min(enemy), seat.max(enemy)))).map(|(r, _)| *r);
            m.insert("peace".into(), flag(!game.view().auth.diplomacy().at_war(PlayerId(seat), PlayerId(enemy))));
            m.insert("peace_round".into(), first(&mut first_peace.into_iter()));
            m.insert("war_rounds".into(), count(at_war.len()));
            m.insert("further_losses".into(), count(watch.captures.iter().filter(|c| c.from.0 == seat).count()));
        }
        Roles::Zone { seat, .. } => {
            m.insert("zone_dead".into(), f64::from(watch.tracked_dead) / watch.followed().max(1) as f64);
            m.insert("seat_units_lost".into(), lost(watch, seat));
        }
        Roles::Hunt { seat, .. } => {
            m.insert("seat_units_lost".into(), lost(watch, seat));
            m.insert("at_sea_end".into(), watch.of(seat).last().map_or(0.0, |s| f64::from(s.at_sea)));
        }
        Roles::Air { seat, .. } => air(&mut m, watch, seat),
        Roles::Pest { seat, pest } => pests(&mut m, game, watch, seat, pest),
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

/// The share of the score of all nations that the nations in focus hold.
pub fn score_share(roles: &Roles, rules: &CompiledRuleset, game: &GameState) -> f64 {
    let auth = game.view().auth;
    let total: i64 = majors(game).iter().map(|p| classic_rules::world::victory::score(auth, rules, *p)).sum::<i64>().max(1);
    let score: i64 = roles.focus(game).iter().map(|p| classic_rules::world::victory::score(auth, rules, *p)).sum();
    score as f64 / total as f64
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
    let last: Vec<_> = focus.iter().filter_map(|p| watch.of(p.0).last().copied()).collect();
    m.insert("seat_military".into(), last.iter().map(|s| f64::from(s.military)).sum::<f64>() / n);
    m.insert("seat_treasury".into(), last.iter().map(|s| f64::from(s.treasury)).sum::<f64>() / n);
    m
}

/// Military units of `seat` lost in the window.
fn lost(watch: &Watch, seat: u8) -> f64 {
    f64::from(watch.losses.get(&seat).copied().unwrap_or(0))
}

/// The seat's air war: its losses, its aircraft at the end and added, its orders to them.
fn air(m: &mut Measures, watch: &Watch, seat: u8) {
    let first = watch.of(seat).next().map_or(0.0, |s| f64::from(s.air));
    let last = watch.of(seat).last().map_or(0.0, |s| f64::from(s.air));
    let by_seat = |orders: &[(u32, PlayerId)]| orders.iter().filter(|(_, p)| p.0 == seat).count() as f64;
    m.insert("seat_units_lost".into(), lost(watch, seat));
    m.insert("seat_air_end".into(), last);
    m.insert("seat_air_added".into(), last - first);
    m.insert("seat_air_orders".into(), by_seat(&watch.air_orders));
    m.insert("seat_interceptions".into(), by_seat(&watch.interceptions));
}

/// The pest's work on the seat: its cities that follow the pest's religion at the end, the seat's
/// faith answers and spies sent home, and its technology lead over the pest.
fn pests(m: &mut Measures, game: &GameState, watch: &Watch, seat: u8, pest: u8) {
    let auth = game.view().auth;
    let religion = auth.module().religion().player(PlayerId(pest)).religion;
    let follows = |h: &classic_rules::ids::CityHandle| religion.is_some_and(|r| classic_rules::derived::religion::city_majority(auth, *h) == Some(classic_rules::world::religion::Creed::Religion(r)));
    let converted = auth.indexes().cities_of(PlayerId(seat)).iter().filter(|h| follows(h)).count();
    let by_seat = |orders: &[(u32, PlayerId)]| orders.iter().filter(|(_, p)| p.0 == seat).count() as f64;
    let techs = |p: u8| auth.players().techs_of(PlayerId(p)).len() as f64;
    m.insert("cities_converted".into(), converted as f64);
    m.insert("faith_answers".into(), by_seat(&watch.faith_orders));
    m.insert("spies_home".into(), by_seat(&watch.spies_home));
    m.insert("tech_lead_over_pest".into(), techs(seat) - techs(pest));
}

/// The seat lost its own original capital in the window.
fn capital_lost(game: &GameState, watch: &Watch, seat: u8) -> bool {
    let auth = game.view().auth;
    watch.captures.iter().filter(|c| c.from.0 == seat).any(|c| auth.cities().handle_of(kernel::ids::EntityId(c.city)).and_then(|h| auth.cities().get(h)).is_some_and(|city| city.original_capital && city.original_owner.0 == seat))
}

fn owned_tiles(game: &GameState, player: u8) -> usize {
    let map = game.view().auth.map();
    (0..map.tile_count() as u32).filter(|t| map.owner(classic_rules::ids::TileId(*t)).flatten() == Some(PlayerId(player))).count()
}

/// An army at the border or inside it, the war at `war_after`: how fast and how well the seat answers.
fn sneak(m: &mut Measures, watch: &Watch, (seat, enemy, war_after): (u8, u8, u32), start: u32, window: u32) {
    let war_round = start + war_after;
    let after_war = |r: u32| r.saturating_sub(war_round).max(1);
    let flag = |b: bool| f64::from(u8::from(b));
    m.insert("cities_lost".into(), watch.captures.iter().filter(|c| c.from.0 == seat).count() as f64);
    m.insert("first_counter".into(), watch.first_hurt.map_or(f64::from(window + 1), |r| f64::from(after_war(r))));
    m.insert("counter_by_3".into(), flag(watch.first_hurt.is_some_and(|r| after_war(r) <= 3)));
    m.insert("invaders_dead".into(), f64::from(watch.tracked_dead));
    m.insert("purchases".into(), watch.purchases.iter().filter(|(_, p)| p.0 == seat).count() as f64);
    m.insert("seat_struck_first".into(), flag(watch.declared.iter().any(|e| e.by.0 == seat && e.on.0 == enemy && e.round <= war_round)));
    let at = |round: u32| watch.of(seat).find(|s| s.round >= round).map_or(0.0, |s| f64::from(s.military));
    let begin = watch.of(seat).next().map_or(0.0, |s| f64::from(s.military));
    m.insert("military_added_before_war".into(), if war_after == 0 { 0.0 } else { at(war_round) - begin });
}

/// The seat weakened: does it keep its cities, its treasury and its people content.
fn weak(m: &mut Measures, watch: &Watch, seat: u8) {
    m.insert("cities_lost".into(), watch.captures.iter().filter(|c| c.from.0 == seat).count() as f64);
    m.insert("wars_declared".into(), watch.declared.iter().filter(|e| e.by.0 == seat).count() as f64);
    m.insert("wars_on_seat".into(), watch.declared.iter().filter(|e| e.on.0 == seat).count() as f64);
    m.insert("unhappy_rounds".into(), watch.of(seat).filter(|s| s.happiness < 0).count() as f64);
    m.insert("broke_rounds".into(), watch.of(seat).filter(|s| s.treasury == 0).count() as f64);
}

/// The lost wonder: how long it stays at the head of the city and the stock burned on it (the
/// stock that fell while the lost wonder was the head and the head changed).
fn snipe(m: &mut Measures, watch: &Watch, city: u64, building: u16) {
    let lost = Some(ProductionItem::Building(BuildingKindId(building)));
    let heads = watch.heads.get(&city).map(Vec::as_slice).unwrap_or_default();
    m.insert("lost_head_rounds".into(), heads.iter().filter(|h| h.item == lost).count() as f64);
    m.insert("dropped_by_1".into(), f64::from(u8::from(heads.first().is_some_and(|h| h.item != lost))));
    let burned: u32 = heads.windows(2).filter(|w| w[0].item == lost && w[1].item != lost).map(|w| w[0].stock.saturating_sub(w[1].stock)).sum();
    m.insert("hammers_burned".into(), f64::from(burned));
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
    let states = auth.module().city_states();
    let allies = (0..auth.players().count() as u8).filter(|p| auth.players().is_city_state(PlayerId(*p)) && states.ally(PlayerId(*p)) == Some(PlayerId(leader))).count();
    m.insert("leader_allies".into(), allies as f64);
    m.insert("gifts_others".into(), watch.gifts.iter().filter(|(_, p)| p.0 != leader).count() as f64);
    let defensive = watch.wars.last().map_or(0, |(_, w)| w.iter().filter(|(a, b)| *a == leader || *b == leader).count());
    m.insert("wars_with_leader_end".into(), defensive as f64);
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
