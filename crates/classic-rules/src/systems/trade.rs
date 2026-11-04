//! Trade routes (classic/docs/classic/rules.md; rules:
//! derived::trade). Commands: EstablishTradeRoute, ChangeTradeHome,
//! PlunderTradeRoute. Turn step Trade (the player's turn start, before the
//! cities): values of every route of the player at either end are updated,
//! the player's trade units move along their paths, finished routes end
//! and their unit comes back to the origin. Hooks: a war ends the routes
//! between the two players (systems::diplomacy); a captured or razed city
//! ends its routes (systems::combat::city).
//! Journal: ClassicChange::TradeChanged for both ends of a route that starts,
//! ends or changes its values; PlayerChange::EffectsChanged for both
//! owners of a route that starts or ends when some effect counts routes
//! (`EffectIndex::trade_counts`: Morocco's partners, the Hanse);
//! UnitChange::Created / Destroyed.

use crate::command::CommandError;
use crate::derived::trade::{self as rules_trade, TradeBlock};
use crate::ids::{CityHandle, PlayerId, UnitHandle};
use crate::rules::CompiledRuleset;
use crate::classic::ClassicChange;
use crate::transaction::journal::PlayerChange;
use crate::transaction::transaction::WorldTransaction;
use crate::world::trade::{RouteKind, TradeRoute};

fn trade_rule(block: TradeBlock) -> CommandError {
    CommandError::RuleViolation(block.reason().into())
}

impl<'a> WorldTransaction<'a> {
    /// The trade unit in its city starts a route to `destination`; the unit
    /// becomes the route and takes its first steps.
    pub(crate) fn establish_trade_route(
        &mut self,
        unit: UnitHandle,
        destination: CityHandle,
        kind: RouteKind,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        let plan = rules_trade::route_status(self.world, rules, unit, destination, kind).map_err(trade_rule)?;
        let core = self.destroy_unit(unit)?;
        let mut route = rules_trade::draft(core.owner, core.kind, kind, &plan);
        let (origin, destination_yields) = rules_trade::route_values(self.world, rules, &route);
        route.origin_yields = origin;
        route.destination_yields = destination_yields;
        self.journal_route_started_or_ended(&route, rules);
        let index = self.world.module.trade.routes().len();
        self.world.module.trade.push(route);
        self.move_route(index, rules);
        self.drop_finished(rules);
        Ok(())
    }

    /// A trade unit moves to another own city at once; a cargo ship needs a
    /// coastal city.
    pub(crate) fn change_trade_home(
        &mut self,
        unit: UnitHandle,
        city: CityHandle,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        let tile = crate::derived::trade::home_status(self.world, rules, unit, city).map_err(trade_rule)?;
        self.relocate_unit(unit, tile, rules);
        if let Some(core) = self.world.units.get_mut_core(unit) {
            core.moves_left = 0;
        }
        Ok(())
    }

    /// A combat unit at war with a route's owner, on the tile of its trade
    /// unit, plunders it.
    pub(crate) fn plunder_trade_route(&mut self, unit: UnitHandle, rules: &CompiledRuleset) -> Result<(), CommandError> {
        let index = crate::derived::trade::plunder_target(self.world, rules, unit).map_err(trade_rule)?;
        let owner = self.world.units.get(unit).map(|v| v.core.owner).ok_or(CommandError::UnknownUnit)?;
        self.plunder(index, owner, rules);
        Ok(())
    }

    /// Turn step Trade (rules 3.5): values of the player's routes at either
    /// end, then its trade units move; finished routes end.
    pub(crate) fn trade_turn(&mut self, player: PlayerId, rules: &CompiledRuleset) {
        if self.world.module.trade.routes().is_empty() {
            return;
        }
        self.update_values(player, rules);
        let owned: Vec<usize> = (0..self.world.module.trade.routes().len())
            .filter(|i| self.world.module.trade.routes()[*i].owner == player)
            .collect();
        // Plundered routes leave the list: walk from the back.
        for index in owned.into_iter().rev() {
            self.move_route(index, rules);
        }
        self.drop_finished(rules);
    }

    fn update_values(&mut self, player: PlayerId, rules: &CompiledRuleset) {
        for index in 0..self.world.module.trade.routes().len() {
            let route = &self.world.module.trade.routes()[index];
            let destination_owner = self.world.cities.get(route.destination).map(|c| c.owner);
            if route.owner != player && destination_owner != Some(player) {
                continue;
            }
            let (origin, destination) = rules_trade::route_values(self.world, rules, route);
            if (origin, destination) == (route.origin_yields, route.destination_yields) {
                continue;
            }
            let route = &mut self.world.module.trade.routes_mut()[index];
            route.origin_yields = origin;
            route.destination_yields = destination;
            let route = route.clone();
            self.journal_route(&route);
        }
    }

    /// Up to the domain's speed of steps.
    fn move_route(&mut self, index: usize, rules: &CompiledRuleset) {
        let Some(route) = self.world.module.trade.routes().get(index) else {
            return;
        };
        let steps = rules_trade::speed(route.domain);
        for _ in 0..steps {
            let route = &mut self.world.module.trade.routes_mut()[index];
            if route.finished() {
                return;
            }
            step(route);
            let (tile, owner) = (route.unit_tile(), route.owner);
            if let Some(_plunderer) = crate::derived::trade::enemy_on(self.world, rules, tile, owner) {
                self.plunder(index, _plunderer, rules);
                return;
            }
        }
    }

    /// The route ends: its trade unit is lost, the plunderer takes the gold.
    fn plunder(&mut self, index: usize, plunderer: PlayerId, rules: &CompiledRuleset) {
        let route = self.world.module.trade.routes_mut().remove(index);
        let era = u32::from(crate::world::player::player_era(self.world, rules, route.owner));
        let gold = rules_trade::plunder_gold(era, route.domain);
        self.world.players.earn(plunderer, gold);
        if gold > 0 {
            let tile = route.unit_tile();
            self.journal.module_changes.push(ClassicChange::Plundered {
                player: plunderer,
                tile,
                source: crate::systems::plunder::PlunderSource::TradeRoute,
                gold,
            });
        }
        self.journal_route_started_or_ended(&route, rules);
    }

    /// Finished routes end; their unit comes back to the origin when the
    /// owner still has it.
    fn drop_finished(&mut self, rules: &CompiledRuleset) {
        let routes = std::mem::take(self.world.module.trade.routes_mut());
        let (done, running): (Vec<TradeRoute>, Vec<TradeRoute>) = routes.into_iter().partition(|r| r.finished());
        *self.world.module.trade.routes_mut() = running;
        for route in done {
            self.journal_route_started_or_ended(&route, rules);
            self.return_unit(&route, rules);
        }
    }

    fn return_unit(&mut self, route: &TradeRoute, rules: &CompiledRuleset) {
        let Some(origin) = self.world.cities.get(route.origin) else {
            return;
        };
        if origin.owner == route.owner {
            let tile = origin.tile;
            self.spawn_at_city(route.owner, tile, route.unit_kind, rules);
        }
    }

    fn journal_route(&mut self, route: &TradeRoute) {
        for city in [route.origin, route.destination] {
            self.journal.module_changes.push(ClassicChange::TradeChanged { city });
        }
    }

    /// A route starts or ends: both ends, and both owners' effects when
    /// some effect counts routes.
    fn journal_route_started_or_ended(&mut self, route: &TradeRoute, rules: &CompiledRuleset) {
        self.journal_route(route);
        if !rules.effect_index.module.trade_counts {
            return;
        }
        let receiver = self.world.cities.get(route.destination).map(|c| c.owner);
        for player in std::iter::once(route.owner).chain(receiver) {
            self.journal.player_changes.push(PlayerChange::EffectsChanged { player });
        }
    }

    /// War: every route between the two players ends without its unit.
    pub(crate) fn cancel_trade_between(&mut self, a: PlayerId, b: PlayerId, rules: &CompiledRuleset) {
        let world = &mut *self.0.world;
        let cities = &world.cities;
        let between = |r: &TradeRoute| {
            let d = cities.get(r.destination).map(|c| c.owner);
            (r.owner == a && d == Some(b)) || (r.owner == b && d == Some(a))
        };
        let routes = std::mem::take(world.module.trade.routes_mut());
        let (ended, kept): (Vec<TradeRoute>, Vec<TradeRoute>) = routes.into_iter().partition(between);
        *self.world.module.trade.routes_mut() = kept;
        for route in ended {
            self.journal_route_started_or_ended(&route, rules);
        }
    }

    /// A World Congress embargo comes into force: every international route it forbids ends without its unit
    /// coming back.
    pub(crate) fn end_embargoed_routes(&mut self, rules: &CompiledRuleset) {
        let routes = std::mem::take(self.world.module.trade.routes_mut());
        let (ended, kept): (Vec<TradeRoute>, Vec<TradeRoute>) = routes.into_iter().partition(|r| {
            let to = self.world.cities.get(r.destination).map(|c| c.owner);
            r.kind == RouteKind::International
                && to.is_some_and(|to| crate::systems::congress::trade_embargoed(self.world, r.owner, to))
        });
        *self.world.module.trade.routes_mut() = kept;
        for route in ended {
            self.journal_route_started_or_ended(&route, rules);
        }
    }

    /// A city changes hands or burns: its routes end; a route that went to
    /// it gives its unit back to the origin.
    pub(crate) fn clear_city_trade_routes(&mut self, city: CityHandle, rules: &CompiledRuleset) {
        let routes = std::mem::take(self.world.module.trade.routes_mut());
        let (ended, kept): (Vec<TradeRoute>, Vec<TradeRoute>) =
            routes.into_iter().partition(|r| r.origin == city || r.destination == city);
        *self.world.module.trade.routes_mut() = kept;
        for route in ended {
            self.journal_route_started_or_ended(&route, rules);
            if route.destination == city && self.world.players.is_alive(route.owner) {
                self.return_unit(&route, rules);
            }
        }
    }
}

/// One step along the path; turning at either end, a return to the origin
/// completes a circuit.
fn step(route: &mut TradeRoute) {
    let last = route.path.len().saturating_sub(1) as u16;
    if route.forward && route.position >= last {
        route.forward = false;
    } else if !route.forward && route.position == 0 {
        route.forward = true;
    }
    if route.forward {
        route.position = (route.position + 1).min(last);
    } else {
        route.position = route.position.saturating_sub(1);
        if route.position == 0 {
            route.circuits_done += 1;
        }
    }
}
