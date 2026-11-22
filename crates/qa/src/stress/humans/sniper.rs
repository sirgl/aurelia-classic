//! The wonder sniper (V1, E5): a human who builds the same world wonder in parallel and completes it
//! a few rounds before the AI's city does, so the AI's stock goes to waste. The script does not
//! model the sniper's own production: at the start of each of its turns it reads the victim's
//! cities, and for each whose head order is a world wonder nobody holds it estimates the rounds left
//! (cost less stock, over the city's production a round); when they are `rounds_before` or fewer, the
//! wonder is completed in the sniper's capital (`Change::Wonder`: the same completion the rules make
//! for a built wonder, so the race is lost for the victim's city).

use std::collections::BTreeMap;

use classic_rules::ids::{BuildingKindId, PlayerId, ProductionItem};
use classic_rules::knowledge::observation::OwnCity;
use classic_rules::{ClassicGame, CompiledRuleset, GameState};
use serde::{Deserialize, Serialize};

use super::{scripted, Cast, Order, Trick};
use crate::bench::modules::majors;
use crate::stress::edit::{self, Change};
use crate::stress::position::Position;
use crate::stress::scene::{Edit, Plan, Roles};
use crate::stress::seat::Seat;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sniper {
    /// The sniper, the human.
    pub seat: u8,
    /// The AI nation whose wonders are sniped.
    pub victim: u8,
    /// The sniper completes a wonder when the victim's city has this many rounds or fewer left.
    pub rounds_before: u32,
}

pub struct Play {
    order: Sniper,
    snipes: u32,
    stock_at_snipe: u64,
}

impl Play {
    pub fn new(order: Sniper) -> Self {
        Play { order, snipes: 0, stock_at_snipe: 0 }
    }
}

/// A world wonder at the head of a city's queue that nobody holds.
fn open_wonder(rules: &CompiledRuleset, built: &[BuildingKindId], city: &OwnCity) -> Option<BuildingKindId> {
    let Some(ProductionItem::Building(kind)) = city.queue.first().copied() else { return None };
    let is_wonder = rules.buildings.get(kind.0 as usize).is_some_and(|d| d.wonder);
    (is_wonder && !built.contains(&kind)).then_some(kind)
}

/// Rounds the city needs to finish `kind` at its head: none when it makes nothing.
fn rounds_left(city: &OwnCity, cost: u32) -> Option<u32> {
    let per_round = u32::try_from(city.income.production).ok().filter(|p| *p > 0)?;
    Some(cost.saturating_sub(city.production).div_ceil(per_round))
}

impl Play {
    /// The wonders the victim's cities are about to finish: (wonder, the city's stock).
    fn ripe(&self, game: &GameState, rules: &CompiledRuleset) -> Vec<(BuildingKindId, u32)> {
        let obs = game.ai_observation(rules, PlayerId(self.order.victim));
        let mut ripe = Vec::new();
        for city in &obs.own_cities {
            let Some(kind) = open_wonder(rules, &obs.built_wonders, city) else { continue };
            let Some(cost) = obs.view.own_production_cost(city.handle, ProductionItem::Building(kind)) else { continue };
            let Some(left) = rounds_left(city, cost) else { continue };
            if left <= self.order.rounds_before {
                ripe.push((kind, city.production));
            }
        }
        ripe
    }
}

impl Seat for Play {
    fn player(&self) -> PlayerId {
        PlayerId(self.order.seat)
    }

    fn play(&mut self, game: &mut GameState, rules: &CompiledRuleset) {
        for (kind, stock) in self.ripe(game, rules) {
            let change = Change::Wonder { player: self.order.seat, building: kind.0 };
            // A second city of the victim on the same wonder: the first snipe has built it already.
            if edit::apply(game, rules, &change).is_ok() {
                self.snipes += 1;
                self.stock_at_snipe += u64::from(stock);
            }
        }
    }

    fn counts(&self) -> BTreeMap<String, f64> {
        BTreeMap::from([("script_snipes".to_owned(), f64::from(self.snipes)), ("script_stock_at_snipe".to_owned(), self.stock_at_snipe as f64)])
    }
}

/// The victim: the nation whose city heads its queue with a world wonder nobody holds, with the most
/// stock; the sniper: the nation with the most score but the victim.
pub(super) fn plan(rules: &CompiledRuleset, position: &Position, _instance: usize) -> Result<Plan, String> {
    let game = &position.game;
    let auth = game.view().auth;
    let all = majors(game);
    let built: Vec<BuildingKindId> = auth.cities().iter().flat_map(|(_, c)| c.buildings.iter().copied()).filter(|b| rules.buildings[b.0 as usize].wonder).collect();
    let best = auth
        .cities()
        .iter()
        .filter(|(_, c)| all.contains(&c.owner))
        .filter_map(|(_, c)| match c.queue.first() {
            Some(ProductionItem::Building(b)) if rules.buildings[b.0 as usize].wonder && !built.contains(b) => Some((c.production, c.id.0, c.owner, *b)),
            _ => None,
        })
        .max_by_key(|(stock, id, ..)| (*stock, std::cmp::Reverse(*id)))
        .ok_or("no city builds a world wonder")?;
    let (stock, _, victim, building) = best;
    let sniper = all
        .iter()
        .copied()
        .filter(|p| *p != victim)
        .max_by_key(|p| (classic_rules::world::victory::score(auth, rules, *p), std::cmp::Reverse(p.0)))
        .ok_or("the victim has no rival")?;
    let info = BTreeMap::from([("stock".to_owned(), stock.to_string()), ("wonder".to_owned(), rules.buildings[building.0 as usize].key.clone())]);
    Ok(Plan { roles: Roles::Human(Cast { trick: Trick::Sniper, seat: victim.0, human: sniper.0 }), base: Vec::new(), info })
}

/// The sniper completes the wonder 3, 2 or 1 rounds before the victim's city does.
pub(super) fn edit_of(cast: Cast, level: usize, _rules: &CompiledRuleset, _position: &Position) -> Result<Edit, String> {
    let rounds_before = [3, 2, 1][level];
    Ok(scripted(Vec::new(), Order::Sniper(Sniper { seat: cast.human, victim: cast.seat, rounds_before })))
}
