//! Strategic and luxury resources of the observer: the overview rows, the
//! top-bar line and the notices of new connections and deficits. Every
//! number is the core's ledger (`derived::resources`).
use classic_rules::derived::resources::{resource_ledger, strategic_combat_penalty, ResourceLedger};
use classic_rules::ids::{ResourceId, UnitKindId};
use classic_rules::rules::content::ResourceClass;

use i18n::{t, tf};

use super::names;
use super::session::Session;
use crate::ui::model::NoticeKind;
use crate::ui::overview_model::ResourceRow;

/// A resource's total and availability at one moment (turn reports).
pub(super) type ResourceMark = (ResourceId, i32, i32);

impl Session {
    fn ledger(&self) -> ResourceLedger {
        resource_ledger(self.game().view().auth, self.rules(), self.me())
    }

    /// The empire's resources by class (strategic first), then name.
    pub(super) fn resource_rows(&self) -> Vec<ResourceRow> {
        let ledger = self.ledger();
        let mut rows: Vec<(u8, ResourceRow)> = ledger
            .known()
            .map(|r| {
                let def = &self.rules().resources[r.0 as usize];
                let row = ResourceRow {
                    name: names::named("resource", &def.name, &def.key),
                    class: class_name(def.class),
                    total: ledger.total(r),
                    used: ledger.used(r),
                    available: ledger.available(r),
                };
                (class_order(def.class), row)
            })
            .collect();
        rows.sort_by(|(a_order, a), (b_order, b)| a_order.cmp(b_order).then_with(|| a.name.cmp(&b.name)));
        rows.into_iter().map(|(_, row)| row).collect()
    }

    /// "2 Iron · 4 Horses": the strategic resources free for new units.
    pub(super) fn resource_bar(&self) -> String {
        let ledger = self.ledger();
        let parts: Vec<String> = ledger
            .known()
            .filter(|r| self.rules().resources[r.0 as usize].class == Some(ResourceClass::Strategic))
            .map(|r| {
                let def = &self.rules().resources[r.0 as usize];
                format!("{} {}", ledger.available(r), names::named("resource", &def.name, &def.key)) // i18n-ok: a number and a name
            })
            .collect();
        parts.join(" · ")
    }

    pub(super) fn resource_marks(&self) -> Vec<ResourceMark> {
        let ledger = self.ledger();
        ledger.known().map(|r| (r, ledger.total(r), ledger.available(r))).collect()
    }

    /// A resource connected for the first time; a deficit that began.
    pub(super) fn report_resources(&mut self, before: &[ResourceMark]) {
        let ledger = self.ledger();
        for r in ledger.known() {
            let (total, available) = before
                .iter()
                .find(|(b, _, _)| *b == r)
                .map(|(_, t, a)| (*t, *a))
                .unwrap_or((0, 0));
            let def = &self.rules().resources[r.0 as usize];
            let name = names::named("resource", &def.name, &def.key);
            if total == 0 && ledger.total(r) > 0 {
                let text = tf!("classic.game.resources.connected", name = name.as_str(), n = ledger.available(r));
                self.notify(NoticeKind::Production, text);
            }
            if available >= 0 && ledger.available(r) < 0 {
                let penalty = self.worst_penalty(&ledger, r);
                let text = tf!("classic.game.resources.deficit", name = name.as_str(), penalty = penalty);
                self.notify(NoticeKind::Combat, text);
            }
        }
    }

    /// The combat penalty of the units that need `r`.
    fn worst_penalty(&self, ledger: &ResourceLedger, r: ResourceId) -> i32 {
        let rules = self.rules();
        (0..rules.units.len())
            .map(|i| UnitKindId(i as u16))
            .filter(|k| rules.units[k.0 as usize].required_resources.iter().any(|(x, _)| *x == r))
            .map(|k| strategic_combat_penalty(ledger, rules, k))
            .min()
            .unwrap_or(0)
    }
}

fn class_name(class: Option<ResourceClass>) -> &'static str {
    match class {
        Some(ResourceClass::Strategic) => t!("classic.game.resources.class-strategic"),
        Some(ResourceClass::Luxury) => t!("classic.game.resources.class-luxury"),
        Some(ResourceClass::Bonus) => t!("classic.game.resources.class-bonus"),
        _ => t!("classic.game.resources.class-other"),
    }
}

fn class_order(class: Option<ResourceClass>) -> u8 {
    match class {
        Some(ResourceClass::Strategic) => 0,
        Some(ResourceClass::Luxury) => 1,
        _ => 2,
    }
}
