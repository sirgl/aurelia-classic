// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Derived from Unciv (https://github.com/yairm210/Unciv), tag 4.22.10, MPL-2.0:
// core/src/com/unciv/logic/automation/civilization/UseGoldAutomation.kt and
// unit/UnitAutomation.tryUpgradeUnit.

//! Unciv's gold: every unit upgrades when it may (to the cheapest unit it
//! upgrades to, new strategic resources only while they are free); a
//! city-state alliance worth having gets 500 or 1000 gold at once; the most
//! productive cities buy what they build when the gold covers it and nothing
//! was put into it yet; a city buys a highly desirable tile next to it.

use crate::ai::planner::Decision;
use crate::ai::trace::{Actor, Reason};
use crate::command::Command;
use crate::knowledge::observation::AIObservation;
use crate::knowledge::public::PlayerQueries;
use crate::rules::content::ResourceClass;
use crate::rules::CompiledRuleset;

use super::personality::UncivPersonality;

fn act(command: Command, reason: Reason) -> Decision {
    Decision::act(Actor::Player, reason, command)
}

/// `UseGoldAutomation.useGold`. The orders are planned against one treasury
/// counted down as they spend.
pub fn use_gold(obs: &AIObservation<'_>, rules: &CompiledRuleset, personality: &UncivPersonality) -> Vec<Decision> {
    let mut out = Vec::new();
    let mut gold = obs.economy.treasury as i64;
    // `tryUpgradeUnit` for every unit.
    let mut taken: Vec<(crate::ids::ResourceId, i32)> = Vec::new();
    for unit in &obs.own_units {
        let Some((to, price)) = obs.view.upgrade_offer(unit.handle) else {
            continue;
        };
        if gold < i64::from(price) {
            continue;
        }
        let old = &rules.units[unit.kind.0 as usize].required_resources;
        let new = &rules.units[to.0 as usize].required_resources;
        let fits = new.iter().filter(|(r, _)| !old.iter().any(|(o, _)| o == r)).all(|(r, n)| {
            let used: i32 = taken.iter().filter(|(t, _)| t == r).map(|(_, k)| *k).sum();
            obs.economy.resources.available(*r) - used >= i32::from(*n)
        });
        if !fits {
            continue;
        }
        let command = Command::UpgradeUnit { unit: unit.handle };
        if obs.view.accepts(&command) {
            for (r, n) in new {
                taken.push((*r, i32::from(*n)));
            }
            gold -= i64::from(price);
            out.push(Decision::act(Actor::Unit(unit.id), Reason::SystemAdvisor, command));
        }
    }
    if obs.view.own_kind() == crate::world::player::PlayerKind::Major {
        gold = city_states(obs, personality, gold, &mut out);
    }
    // Purchases: the cities by production, what they build when it pays.
    let acceptable_modifier = (gold.max(0) / 100) as i32;
    let mut cities: Vec<&crate::knowledge::observation::OwnCity> = obs.own_cities.iter().collect();
    cities.sort_by_key(|c| (std::cmp::Reverse(c.income.production), c.id));
    for city in cities {
        let Some(head) = city.queue.first() else {
            continue;
        };
        let Some(price) = obs.view.own_hurry_price(city.handle) else {
            continue;
        };
        let modifier = match head {
            crate::ids::ProductionItem::Building(b) => rules.buildings[b.0 as usize].hurry_cost_modifier,
            _ => 0,
        };
        // Unciv buys only what nothing was put into yet; its city chooses the next item as
        // the last one completes, ours at the end of the turn, so one turn's production
        // in the item counts as nothing put in.
        let invested = city.production > city.income.production.max(0) as u32;
        if gold < i64::from(price) || modifier > acceptable_modifier || invested {
            continue;
        }
        let command = Command::HurryProduction { player: obs.player, city: city.handle };
        if obs.view.accepts(&command) {
            gold -= i64::from(price);
            out.push(act(command, Reason::Hurry));
        }
    }
    buy_tiles(obs, rules, gold, &mut out);
    out
}

/// `useGoldForCityStates`: a buyout where the rules allow it, else 500 or
/// 1000 gold to the city-state whose alliance is worth most.
fn city_states(obs: &AIObservation<'_>, personality: &UncivPersonality, mut gold: i64, out: &mut Vec<Decision>) -> i64 {
    let player = obs.player;
    let not_targets: Vec<&crate::knowledge::observation::CityStateInfo> = obs.city_states.iter().filter(|c| !c.at_war).collect();
    if gold >= 330 && obs.economy.happiness.excess() > 0 {
        for info in &not_targets {
            if info.can_buyout {
                let command = Command::BuyoutCityState { player, city_state: info.player };
                if obs.view.accepts(&command) {
                    out.push(act(command, Reason::SystemAdvisor));
                    return gold;
                }
            }
        }
    }
    if gold < 500 || not_targets.is_empty() {
        return gold;
    }
    let best = not_targets
        .iter()
        .filter(|c| c.ally != Some(player))
        .map(|c| (super::civ::value_city_state_alliance(obs, c, personality), *c))
        .filter(|(v, _)| *v > 0)
        .max_by_key(|(v, c)| (*v, std::cmp::Reverse(c.player)));
    if let Some((value, info)) = best {
        let stop = info.influence > 60 + 2 * value;
        if info.influence >= 10 && !stop {
            let amount = if gold >= 1000 { 1000 } else { 500 };
            let command = Command::GiftGold { player, city_state: info.player, gold: amount };
            if obs.view.accepts(&command) {
                gold -= i64::from(amount);
                out.push(act(command, Reason::GiftGold));
            }
        }
    }
    gold
}

/// `maybeBuyCityTiles`: an unowned tile two from a city, next to its land,
/// with a natural wonder, a luxury we hold fewer than two of, or food and
/// production of three or more; from round 20 of the game's speed on.
fn buy_tiles(obs: &AIObservation<'_>, rules: &CompiledRuleset, mut gold: i64, out: &mut Vec<Decision>) {
    if gold <= 0 || obs.round < obs.speed.game_round(20) {
        return;
    }
    let techs = crate::derived::tile_yields::TechSet::of(&obs.economy.techs);
    let dims = obs.view.dimensions();
    let mut wanted: Vec<(u8, crate::ids::TileId, crate::ids::CityHandle, u32)> = Vec::new();
    for city in obs.own_cities.iter().filter(|c| c.status != crate::world::city::CityStatus::Puppet) {
        for (tile, price) in obs.view.own_tile_offers(city.handle) {
            if dims.distance(tile, city.tile) != Some(2) || !obs.view.is_visible(tile) {
                continue;
            }
            let known = obs.view.tile(tile);
            let Some(s) = known.snapshot() else {
                continue;
            };
            let wonder = s.natural_wonder.is_some();
            let luxury = s.resource.is_some_and(|r| rules.resources.get(r.0 as usize).is_some_and(|d| d.class == Some(ResourceClass::Luxury)) && obs.economy.resources.available(r) < 2);
            let y = crate::derived::tile_yields::plot_yields(&obs.view, rules, tile, &techs);
            let rich = y.food + y.production >= 3;
            if !(wonder || luxury || rich) {
                continue;
            }
            let strategic = s.resource.is_some_and(|r| rules.resources.get(r.0 as usize).is_some_and(|d| d.class == Some(ResourceClass::Strategic)));
            let rank = if wonder { 0 } else if luxury { 1 } else if strategic { 2 } else { 3 };
            wanted.push((rank, tile, city.handle, price));
        }
    }
    wanted.sort_by_key(|(rank, tile, _, price)| (*rank, *price, *tile));
    let mut bought: Vec<crate::ids::TileId> = Vec::new();
    for (_, tile, city, price) in wanted {
        if bought.contains(&tile) || gold < i64::from(price) {
            continue;
        }
        let command = Command::PurchaseTile { player: obs.player, city, tile };
        if obs.view.accepts(&command) {
            gold -= i64::from(price);
            bought.push(tile);
            out.push(act(command, Reason::BuyTile));
        }
    }
}
