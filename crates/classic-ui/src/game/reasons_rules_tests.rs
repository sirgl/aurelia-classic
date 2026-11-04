//! Every reason the rules give as English text for a blocked action has a text in the player's
//! language (the bundles hold Russian and English), and the rules do not grow raw English
//! literals that nobody worded.
use classic_rules::derived::archaeology::DigBlock;
use classic_rules::derived::grants::ChoiceBlock;
use classic_rules::derived::great_people::GreatPersonBlock;
use classic_rules::derived::ruins::RuinsBlock;
use classic_rules::derived::tourism::TourismBlock;
use classic_rules::derived::trade::TradeBlock;
use classic_rules::world::city_states::CityStateBlock;
use classic_rules::world::espionage::SpyBlock;
use classic_rules::world::military::{AirOrderBlock, AirliftBlock, ParadropBlock};

use super::reasons::only_cyrillic;
use super::reasons_rules::{air_order_text, entries, rule_text};

fn worded<B: Copy>(all: &[B], reason: fn(B) -> &'static str) {
    for block in all {
        let english = reason(*block);
        let text = rule_text(english).unwrap_or_else(|| panic!("no text for «{english}»"));
        assert!(!text.starts_with("classic."), "a key shows instead of a text: {text}");
        i18n::with_language("ru", || {
            let text = rule_text(english).unwrap_or_else(|| panic!("no Russian text for «{english}»"));
            only_cyrillic(text);
        });
    }
}

#[test]
fn every_typed_block_of_the_rules_is_worded_in_both_languages() {
    worded(&TradeBlock::ALL, TradeBlock::reason);
    worded(&TourismBlock::ALL, TourismBlock::reason);
    worded(&DigBlock::ALL, DigBlock::reason);
    worded(&ChoiceBlock::ALL, ChoiceBlock::reason);
    worded(&GreatPersonBlock::ALL, GreatPersonBlock::reason);
    worded(&AirliftBlock::ALL, AirliftBlock::reason);
    worded(&ParadropBlock::ALL, ParadropBlock::reason);
    worded(&RuinsBlock::ALL, RuinsBlock::reason);
    worded(&SpyBlock::ALL, SpyBlock::reason);
    worded(&CityStateBlock::ALL, CityStateBlock::reason);
    i18n::with_language("ru", || {
        for (_, text) in entries() {
            only_cyrillic(text);
        }
    });
    for (_, text) in entries() {
        assert!(!text.starts_with("classic."), "{text}");
    }
}

/// The refusals of a sweep and of an interception order are worded in every language; «out of
/// range» is the one the trade reason owns in `rule_text`, so the sweep asks `air_order_text`.
#[test]
fn the_refusals_of_air_orders_are_worded() {
    for block in AirOrderBlock::ALL {
        let text = air_order_text(block.reason()).unwrap_or_else(|| panic!("no text for «{}»", block.reason()));
        assert!(!text.starts_with("classic."), "{text}");
        i18n::with_language("ru", || only_cyrillic(air_order_text(block.reason()).unwrap()));
        let owned = block != AirOrderBlock::OutOfRange;
        assert_eq!(rule_text(block.reason()).is_some_and(|t| t == text), owned, "{block:?}");
    }
    assert_ne!(air_order_text("out of range"), rule_text("out of range"), "the sweep has its own words");
}

#[test]
fn one_english_reason_has_one_text() {
    let split: Vec<_> = entries().filter(|(english, text)| rule_text(english) != Some(*text)).collect();
    assert!(split.is_empty(), "worded two ways: {split:?}");
}

#[test]
fn an_unknown_reason_has_no_text() {
    assert_eq!(rule_text("no such reason anywhere"), None);
}

const SCANNED: [(&str, &str); 18] = [
    ("systems/espionage.rs", include_str!("../../../classic-rules/src/systems/espionage.rs")),
    ("command/validate.rs", include_str!("../../../classic-rules/src/command/validate.rs")),
    ("systems/city_states/validate.rs", include_str!("../../../classic-rules/src/systems/city_states/validate.rs")),
    ("systems/city_states/gifts.rs", include_str!("../../../classic-rules/src/systems/city_states/gifts.rs")),
    ("systems/city_states/buyout.rs", include_str!("../../../classic-rules/src/systems/city_states/buyout.rs")),
    ("derived/great_people.rs", include_str!("../../../classic-rules/src/derived/great_people.rs")),
    ("systems/great_people.rs", include_str!("../../../classic-rules/src/systems/great_people.rs")),
    ("systems/grants.rs", include_str!("../../../classic-rules/src/systems/grants.rs")),
    ("derived/grants.rs", include_str!("../../../classic-rules/src/derived/grants.rs")),
    ("derived/trade.rs", include_str!("../../../classic-rules/src/derived/trade.rs")),
    ("systems/trade.rs", include_str!("../../../classic-rules/src/systems/trade.rs")),
    ("derived/tourism.rs", include_str!("../../../classic-rules/src/derived/tourism.rs")),
    ("systems/tourism.rs", include_str!("../../../classic-rules/src/systems/tourism.rs")),
    ("derived/archaeology.rs", include_str!("../../../classic-rules/src/derived/archaeology.rs")),
    ("systems/archaeology.rs", include_str!("../../../classic-rules/src/systems/archaeology.rs")),
    ("derived/ruins.rs", include_str!("../../../classic-rules/src/derived/ruins.rs")),
    ("systems/ruins.rs", include_str!("../../../classic-rules/src/systems/ruins.rs")),
    ("systems/airlift.rs", include_str!("../../../classic-rules/src/systems/airlift.rs")),
];

/// Reasons the rules build with `format!` from names of the ruleset: they stay English.
const DYNAMIC: [(&str, &str); 1] = [("needs {name}", "names a resource of the ruleset")];

/// The string literal right after `violation(` / `RuleViolation(`, directly or through `format!`.
fn raw_literals(source: &str) -> Vec<(&str, bool)> {
    let mut found = Vec::new();
    for marker in ["violation(", "RuleViolation("] {
        for (at, _) in source.match_indices(marker) {
            let mut rest = source[at + marker.len()..].trim_start();
            let formatted = rest.starts_with("format!(") || rest.starts_with("&format!(");
            if formatted {
                rest = rest[rest.find('(').unwrap_or(0) + 1..].trim_start();
            }
            if let Some(literal) = rest.strip_prefix('"').and_then(|r| r.split('"').next()) {
                found.push((literal, formatted));
            }
        }
    }
    found
}

#[test]
fn the_rules_name_no_raw_english_reason_without_a_text() {
    let mut dynamic_seen = Vec::new();
    for (file, source) in SCANNED {
        for (literal, formatted) in raw_literals(source) {
            if formatted {
                assert!(DYNAMIC.iter().any(|(text, _)| *text == literal), "{file}: dynamic «{literal}» is not listed");
                dynamic_seen.push(literal);
            } else {
                assert!(rule_text(literal).is_some(), "{file}: «{literal}» has no text");
            }
        }
    }
    for (text, reason) in DYNAMIC {
        assert!(dynamic_seen.contains(&text), "«{text}» ({reason}) is gone from the rules: drop it from DYNAMIC");
    }
}

#[test]
fn the_scan_finds_the_literals_in_every_form() {
    let source = "violation( \"a\" ) RuleViolation(\n  \"b\".into()) RuleViolation(format!(\"c {x}\")) fn violation(reason: &str)";
    assert_eq!(raw_literals(source), [("a", false), ("b", false), ("c {x}", true)]);
}
