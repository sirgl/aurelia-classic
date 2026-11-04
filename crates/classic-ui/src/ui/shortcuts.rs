//! The classic keyboard: the rows of the module's shortcut table (`client::shell::keymap`),
//! which unit order each unit-order row gives, and what each screen row opens. The defaults
//! follow the classic 4X keys (platform/docs/shortcuts.md has the source of each).
use crate::ui::model::Action;
use crate::ui::screens::OverviewTab;
use client::shell::keymap::{ActionDef, Scope, ShortcutSet};

/// Shell id of the city screen (`Screen::City.id()`; a test keeps them equal).
pub const CITY_SCREEN: u8 = 0;

const GAME: Scope = Scope::Game;
const MAP: Scope = Scope::Map;
const CITY: Scope = Scope::Screen(CITY_SCREEN);

pub const SET: ShortcutSet = ShortcutSet { name: "classic", rows: ROWS };

pub const ROWS: &[ActionDef] = &[
    ActionDef::new("encyclopedia", "screens", GAME, "F1"),
    ActionDef::new("overview-economy", "screens", GAME, "F2"),
    ActionDef::new("overview-military", "screens", GAME, "F3"),
    ActionDef::new("overview-diplomacy", "screens", GAME, "F4"),
    ActionDef::new("policies", "screens", GAME, "F5"),
    ActionDef::new("technologies", "screens", GAME, "F6"),
    ActionDef::new("notifications", "screens", GAME, "F7"),
    ActionDef::new("victory", "screens", GAME, "F8"),
    ActionDef::new("demographics", "screens", GAME, "F9"),
    ActionDef::new("culture", "screens", GAME, "F10"),
    ActionDef::new("next-unit", "units", MAP, "Period"), // i18n-ok: keycap names and file keys
    ActionDef::new("previous-unit", "units", MAP, "Comma"), // i18n-ok: keycap names and file keys
    ActionDef::new("order-move", "units", MAP, "M"),
    ActionDef::new("order-attack", "units", MAP, "Ctrl+A"),
    ActionDef::new("order-fortify", "units", MAP, "F"),
    ActionDef::new("order-sleep", "units", MAP, "A"),
    ActionDef::new("order-skip", "units", MAP, "Space"), // i18n-ok: keycap names and file keys
    ActionDef::new("order-heal", "units", MAP, "H"),
    ActionDef::new("order-found-city", "units", MAP, "B"),
    ActionDef::new("order-improve", "units", MAP, "I"),
    ActionDef::new("order-road", "units", MAP, "R"),
    ActionDef::new("order-chop", "units", MAP, "Alt+C"),
    ActionDef::new("order-upgrade", "units", MAP, "U"),
    ActionDef::new("order-set-up", "units", MAP, "S"),
    ActionDef::new("order-pillage", "units", MAP, "P"),
    ActionDef::new("order-promote", "units", MAP, "O"),
    // Air orders. P is the pillage key (a fighter cannot pillage, but one key never gives two orders),
    // so the interception takes N.
    ActionDef::new("order-intercept", "units", MAP, "N"),
    ActionDef::new("order-sweep", "units", MAP, "E"),
    ActionDef::new("next-city", "cities", CITY, "ArrowRight"),
    ActionDef::new("previous-city", "cities", CITY, "ArrowLeft"),
];

/// The unit order a row gives (the order must be among the selected unit's to work).
pub fn order_of(id: &str) -> Option<&'static [Action]> {
    // `order-fortify` is Fortify for a unit that can, else Sleep: one key for "stay here".
    Some(match id {
        "order-move" => &[Action::Move],
        "order-attack" => &[Action::Attack],
        "order-fortify" => &[Action::Fortify, Action::Sleep],
        "order-sleep" => &[Action::Sleep],
        "order-skip" => &[Action::Skip],
        "order-heal" => &[Action::Heal],
        "order-found-city" => &[Action::FoundCity],
        "order-improve" => &[Action::Improve],
        "order-road" => &[Action::Road],
        "order-chop" => &[Action::Chop],
        "order-upgrade" => &[Action::Upgrade],
        "order-set-up" => &[Action::SetUp],
        "order-pillage" => &[Action::Pillage],
        "order-promote" => &[Action::Promote],
        "order-intercept" => &[Action::Intercept],
        "order-sweep" => &[Action::Sweep],
        _ => return None,
    })
}

/// The row whose key a unit-order button shows in its tooltip.
pub fn row_of(action: Action) -> Option<&'static str> {
    ROWS.iter().map(|r| r.id).filter(|id| order_of(id).is_some_and(|a| a[0] == action)).next()
}

/// The overview tab a row opens.
pub fn overview_of(id: &str) -> Option<OverviewTab> {
    Some(match id {
        "overview-economy" => OverviewTab::Cities,
        "overview-military" => OverviewTab::Units,
        "overview-diplomacy" => OverviewTab::Diplomacy,
        "notifications" => OverviewTab::Notifications,
        "victory" => OverviewTab::Victory,
        "demographics" => OverviewTab::Demographics,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use client::shell::keymap::{Chord, Keymap};

    #[test]
    fn the_default_keys_have_no_duplicates() {
        assert_eq!(Keymap::defaults(SET).duplicates(), vec![]);
    }

    #[test]
    fn the_city_screen_scope_is_the_city_screen() {
        assert_eq!(crate::ui::screens::Screen::City.id().0, CITY_SCREEN);
    }

    #[test]
    fn every_row_has_an_english_label_and_a_group() {
        for def in ROWS {
            assert!(i18n::tr_dynamic(&def.label_key(), &[]).is_some(), "{} has no message", def.label_key());
            assert!(i18n::tr_dynamic(&format!("shortcut.group.{}", def.group), &[]).is_some(), "group {}", def.group);
        }
    }

    #[test]
    fn every_order_row_gives_an_order_and_a_tab_row_a_tab() {
        for def in ROWS {
            let is_order = def.id.starts_with("order-");
            assert_eq!(order_of(def.id).is_some(), is_order, "{}", def.id);
        }
        for id in ["overview-economy", "overview-military", "overview-diplomacy", "notifications", "victory", "demographics"] {
            assert!(overview_of(id).is_some(), "{id}");
        }
        assert_eq!(row_of(Action::FoundCity), Some("order-found-city"));
    }

    #[test]
    fn the_air_orders_have_their_keys() {
        let map = Keymap::defaults(SET);
        assert_eq!(map.hint("order-intercept").unwrap_or_default(), "N");
        assert_eq!(map.hint("order-sweep").unwrap_or_default(), "E");
        assert_eq!(row_of(Action::Intercept), Some("order-intercept"));
        assert_eq!(row_of(Action::Sweep), Some("order-sweep"));
    }

    #[test]
    fn the_keys_of_the_classic_set() {
        let map = Keymap::defaults(SET);
        let key = |id: &str| map.hint(id).unwrap_or_default();
        assert_eq!(key("order-found-city"), "B");
        assert_eq!(key("order-attack"), "Ctrl+A");
        assert_eq!(key("technologies"), "F6");
        assert_eq!(key("end-turn"), "Enter");
        assert_eq!(map.chords("end-turn"), &[Chord::parse("Enter").unwrap(), Chord::parse("Ctrl+Space").unwrap()]);
    }
}
