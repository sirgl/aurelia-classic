//! Trade routes of the observer: the overview's trade panel (running
//! routes, trade units with the routes they may start), the order that
//! starts a route, and the notices of routes that end. Numbers are the
//! core's (`derived::trade`).
use classic_rules::derived::trade::{routes_possible, routes_used, trade_offers, TradeOffer};
use classic_rules::ids::{CityHandle, UnitHandle};
use classic_rules::world::trade::{RouteKind, RouteYields, TradeDomain, TradeRoute};
use classic_rules::Command;
use i18n::{t, tf};

use super::session::Session;
use crate::ui::model::NoticeKind;
use crate::ui::overview_model::{TradeOfferRow, TradeRouteRow, TradeUnitRow};

/// A running route at one moment (turn reports): origin and destination.
pub(super) type RouteMark = (CityHandle, CityHandle);

impl Session {
    /// Routes the observer runs or receives.
    pub(super) fn trade_route_rows(&self) -> Vec<TradeRouteRow> {
        let auth = self.game().view().auth;
        let me = self.me();
        auth.module()
            .trade()
            .routes()
            .iter()
            .filter(|r| {
                r.owner == me
                    || auth
                        .cities()
                        .get(r.destination)
                        .is_some_and(|c| c.owner == me)
            })
            .map(|r| self.route_row(r))
            .collect()
    }

    fn route_row(&self, route: &TradeRoute) -> TradeRouteRow {
        let own = route.owner == self.me();
        // An internal route feeds its destination, our own city as well.
        let ours_at_destination = !own || route.kind != RouteKind::International;
        let yields = if ours_at_destination {
            route.destination_yields
        } else {
            route.origin_yields
        };
        let steps_left = route_steps_left(route);
        let speed = classic_rules::derived::trade::speed(route.domain);
        TradeRouteRow {
            from: self.city_label(route.origin),
            to: self.city_label(route.destination),
            kind: kind_label(route.kind, route.domain),
            yields: yields_text(yields),
            other_end: yields_text(if ours_at_destination {
                route.origin_yields
            } else {
                route.destination_yields
            }),
            turns_left: steps_left.div_ceil(speed),
            own,
        }
    }

    /// Own trade units with the routes they may start now.
    pub(super) fn trade_unit_rows(&self) -> Vec<TradeUnitRow> {
        let auth = self.game().view().auth;
        let me = self.me();
        auth.indexes()
            .units_of(me)
            .iter()
            .filter_map(|u| auth.units().get(*u).map(|v| (*u, v)))
            .filter(|(_, v)| classic_rules::derived::trade::is_trade_unit(self.rules(), v.core.kind))
            .map(|(handle, v)| TradeUnitRow {
                unit: handle,
                at: auth
                    .indexes()
                    .city_on_tile(v.core.tile)
                    .map(|c| self.city_label(c))
                    .unwrap_or_default(),
                offers: trade_offers(auth, self.rules(), handle)
                    .iter()
                    .map(|o| self.offer_row(o))
                    .collect(),
            })
            .collect()
    }

    fn offer_row(&self, offer: &TradeOffer) -> TradeOfferRow {
        TradeOfferRow {
            destination: offer.destination,
            kind: offer.kind,
            label: tf!("classic.game.trade.offer",
                city = self.city_label(offer.destination),
                kind = kind_name(offer.kind),
                n = offer.steps
            ),
            yields: yields_text(if offer.kind == RouteKind::International {
                offer.origin
            } else {
                offer.destination_yields
            }),
            other_end: if offer.kind == RouteKind::International {
                yields_text(offer.destination_yields)
            } else {
                String::new()
            },
        }
    }

    /// Active routes, idle units and queued units consume different parts of capacity.
    pub(super) fn trade_summary(&self) -> String {
        let auth = self.game().view().auth;
        let used = routes_used(auth, self.rules(), self.me(), true);
        let possible = routes_possible(auth, self.rules(), self.me());
        let active = auth.module().trade().owned_by(self.me()).count() as u32;
        let committed = routes_used(auth, self.rules(), self.me(), false);
        let idle = committed.saturating_sub(active);
        let queued = used.saturating_sub(committed);
        [
            tf!("classic.game.trade.summary-active", n = active),
            tf!("classic.game.trade.summary-idle", n = idle),
            tf!("classic.game.trade.summary-queued", n = queued),
            tf!("classic.game.trade.summary-capacity", used = used, possible = possible),
        ]
        .join(" · ")
    }

    fn city_label(&self, city: CityHandle) -> String {
        self.game()
            .view()
            .auth
            .cities()
            .get(city)
            .map(|c| c.name.clone())
            .unwrap_or_else(|| "?".into()) // i18n-ok: a placeholder for a missing city
    }

    /// The trade unit starts the chosen route.
    pub(super) fn establish_route(
        &mut self,
        unit: UnitHandle,
        destination: CityHandle,
        kind: RouteKind,
    ) {
        let to = self.city_label(destination);
        if self.submit(Command::EstablishTradeRoute {
            unit,
            destination,
            kind,
        }) {
            self.notify(
                NoticeKind::Production,
                tf!("classic.game.trade.established", city = to),
            );
        }
    }

    pub(super) fn route_marks(&self) -> Vec<RouteMark> {
        let me = self.me();
        self.game()
            .view()
            .auth
            .module()
            .trade()
            .owned_by(me)
            .map(|r| (r.origin, r.destination))
            .collect()
    }

    /// Own routes that ended since the turn began.
    pub(super) fn report_trade(&mut self, before: &[RouteMark]) {
        let now = self.route_marks();
        for (origin, destination) in before {
            if !now.contains(&(*origin, *destination)) {
                let text = tf!("classic.game.trade.ended",
                    from = self.city_label(*origin),
                    to = self.city_label(*destination)
                );
                self.notify(NoticeKind::Production, text);
            }
        }
    }
}

fn route_steps_left(route: &TradeRoute) -> u32 {
    let length = route.length();
    let circuits_left = u32::from(route.circuits_total.saturating_sub(route.circuits_done));
    let back = if route.forward {
        2 * length - u32::from(route.position)
    } else {
        u32::from(route.position)
    };
    (circuits_left.saturating_sub(1)) * 2 * length + back
}

fn kind_name(kind: RouteKind) -> &'static str {
    match kind {
        RouteKind::International => t!("classic.game.trade.kind-international"),
        RouteKind::Food => t!("classic.game.trade.kind-food"),
        RouteKind::Production => t!("classic.game.trade.kind-production"),
    }
}

fn kind_label(kind: RouteKind, domain: TradeDomain) -> &'static str {
    match (kind, domain) {
        (RouteKind::International, TradeDomain::Land) => t!("classic.game.trade.label-caravan"),
        (RouteKind::International, TradeDomain::Sea) => t!("classic.game.trade.label-cargo-ship"),
        (RouteKind::Food, _) => t!("classic.game.trade.kind-food"),
        (RouteKind::Production, _) => t!("classic.game.trade.kind-production"),
    }
}

/// "+7.00 gold  +3.00 science" (hundredths shown as the game shows them).
fn yields_text(y: RouteYields) -> String {
    let parts = [
        (y.gold, i18n::key!("classic.game.trade.yield-gold")),
        (y.science, i18n::key!("classic.game.trade.yield-science")),
        (y.food, i18n::key!("classic.game.trade.yield-food")),
        (y.production, i18n::key!("classic.game.trade.yield-production")),
    ];
    let shown: Vec<String> = parts
        .iter()
        .filter(|(v, _)| *v > 0)
        .map(|(v, key)| i18n::tr_args(key, &[("amount", (f64::from(*v) / 100.0).into())]))
        .collect();
    if shown.is_empty() {
        t!("classic.game.trade.nothing").into()
    } else {
        shown.join("  ")
    }
}
