//! Reverse cross-links: a page that says "Requires X" makes X say "Leads to"
//! it, so every relation is navigable from both ends.
use std::collections::HashMap;

use i18n::key;

use super::{Entry, PediaRef};

/// Forward label -> label on the target page, as message keys: the labels are resolved in the
/// active language when the links are built. Relations missing here are
/// shown one way only (the target lists them itself, like a nation's
/// unique units, or they would only repeat the page's own text).
const REVERSE: [(&str, &str); 13] = [
    (key!("classic.pedia.entries.link-requires"), key!("classic.pedia.entries.link-leads-to")),
    (key!("classic.pedia.entries.link-requires-one-of"), key!("classic.pedia.entries.link-leads-to")),
    (key!("classic.pedia.entries.link-required-technology"), key!("classic.pedia.entries.link-unlocks")),
    (key!("classic.pedia.entries.link-obsolete-with"), key!("classic.pedia.entries.link-makes-obsolete")),
    (key!("classic.pedia.entries.link-upgrades-to"), key!("classic.pedia.entries.link-upgrades-from")),
    (key!("classic.pedia.entries.link-replaces"), key!("classic.pedia.entries.link-replaced-by")),
    (key!("classic.pedia.entries.link-requires-resources"), key!("classic.pedia.entries.link-needed-by")),
    (key!("classic.pedia.entries.link-requires-buildings"), key!("classic.pedia.entries.link-needed-by")),
    (key!("classic.pedia.entries.link-free-promotions"), key!("classic.pedia.entries.link-given-to")),
    (key!("classic.pedia.entries-world.link-improved-by"), key!("classic.pedia.entries-world.link-improves")),
    (key!("classic.pedia.entries-world.link-revealed-by"), key!("classic.pedia.entries-world.link-reveals")),
    (key!("classic.pedia.entries.link-requires-projects"), key!("classic.pedia.entries.link-needed-by")),
    (key!("classic.pedia.entries.link-type"), key!("classic.pedia.entries.link-city-states")),
];

pub fn add_reverse_links(entries: &mut [Entry]) {
    let at: HashMap<PediaRef, usize> = entries.iter().enumerate().map(|(i, e)| (e.id, i)).collect();
    let mut reverse: Vec<(usize, &'static str, PediaRef)> = Vec::new();
    for entry in entries.iter() {
        for group in &entry.links {
            let Some(&(_, back)) = REVERSE.iter().find(|(forward, _)| i18n::tr(forward) == group.label) else {
                continue;
            };
            for target in &group.targets {
                if let Some(&i) = at.get(target) {
                    reverse.push((i, back, entry.id));
                }
            }
        }
    }
    for (i, label, source) in reverse {
        entries[i].link(i18n::tr(label), vec![source]);
    }
}
