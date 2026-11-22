//! The behaviour numbers of the human tricks (the outcome, `seat_*`, is `stress::metrics`'s, the same
//! for every situation). What the scripts counted comes as `script_*`.

use classic_rules::ids::{BuildingKindId, PlayerId, ProductionItem};
use classic_rules::world::congress::ResolutionKind;
use classic_rules::world::player::ClassicPlayers;
use classic_rules::{CompiledRuleset, GameState};

use super::{Cast, Trick};
use crate::stress::metrics::Measures;
use crate::stress::watch::Watch;

/// The measures of `cast` after the window (from round `start`).
pub fn measure(m: &mut Measures, cast: Cast, rules: &CompiledRuleset, game: &GameState, watch: &Watch) {
    let flag = |b: bool| f64::from(u8::from(b));
    let (seat, human) = (cast.seat, cast.human);
    let lost = watch.captures.iter().filter(|c| c.from.0 == seat).count();
    m.insert("cities_lost".into(), lost as f64);
    m.insert("capital_lost".into(), flag(capital_lost(game, watch, seat)));
    m.insert("army_dead".into(), f64::from(watch.tracked_dead) / watch.followed().max(1) as f64);
    m.insert("purchases".into(), watch.purchases.iter().filter(|(_, p)| p.0 == seat).count() as f64);
    m.insert("unhappy_rounds".into(), watch.of(seat).filter(|s| s.happiness < 0).count() as f64);
    m.insert("gifts_by_seat".into(), watch.gifts.iter().filter(|(_, p)| p.0 == seat).count() as f64);
    m.insert("seat_allies".into(), allies(game, seat) as f64);
    match cast.trick {
        Trick::Sneak => {
            let war = watch.declared.iter().filter(|e| e.by.0 == human && e.on.0 == seat).map(|e| e.round).min();
            let begin = watch.of(seat).next().map_or(0.0, |s| f64::from(s.military));
            let at = war.and_then(|r| watch.of(seat).find(|s| s.round >= r)).map_or(begin, |s| f64::from(s.military));
            m.insert("military_added_before_war".into(), at - begin);
        }
        Trick::Turtle => {
            m.insert("turtle_attacked".into(), flag(watch.declared.iter().any(|e| e.on.0 == human)));
            m.insert("turtle_cities_lost".into(), watch.captures.iter().filter(|c| c.from.0 == human).count() as f64);
        }
        Trick::Sniper => {
            m.insert("stock_burned".into(), f64::from(stock_burned(rules, game, watch, PlayerId(seat))));
        }
        Trick::Lobby => {
            let embargo = game.view().auth.module().congress().active().iter().any(|a| a.kind == ResolutionKind::PlayerEmbargo && a.choice == u32::from(seat));
            m.insert("embargo_on_seat".into(), flag(embargo));
        }
        _ => {}
    }
}

fn capital_lost(game: &GameState, watch: &Watch, seat: u8) -> bool {
    let auth = game.view().auth;
    watch.captures.iter().filter(|c| c.from.0 == seat).any(|c| {
        let city = auth.cities().handle_of(kernel::ids::EntityId(c.city)).and_then(|h| auth.cities().get(h));
        city.is_some_and(|city| city.original_capital && city.original_owner.0 == seat)
    })
}

/// The city-states allied to `seat` now.
fn allies(game: &GameState, seat: u8) -> usize {
    let auth = game.view().auth;
    let states = auth.module().city_states();
    (0..auth.players().count() as u8).filter(|p| auth.players().is_city_state(PlayerId(*p)) && states.ally(PlayerId(*p)) == Some(PlayerId(seat))).count()
}

/// The stock the seat's cities lost when a world wonder left the head of their queue (the order was
/// lost: the stock fell from one round to the next while the head changed).
fn stock_burned(rules: &CompiledRuleset, game: &GameState, watch: &Watch, seat: PlayerId) -> u32 {
    let auth = game.view().auth;
    let wonder = |item: Option<ProductionItem>| match item {
        Some(ProductionItem::Building(BuildingKindId(b))) => rules.buildings.get(b as usize).is_some_and(|d| d.wonder),
        _ => false,
    };
    let mut burned = 0;
    for (id, heads) in &watch.heads {
        let theirs = auth.cities().handle_of(kernel::ids::EntityId(*id)).and_then(|h| auth.cities().get(h)).is_some_and(|c| c.owner == seat);
        if !theirs {
            continue;
        }
        for w in heads.windows(2) {
            if wonder(w[0].item) && w[1].item != w[0].item {
                burned += w[0].stock.saturating_sub(w[1].stock);
            }
        }
    }
    burned
}
