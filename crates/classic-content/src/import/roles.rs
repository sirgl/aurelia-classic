//! The role bindings of the classic ruleset (`classic_rules::rules::roles`): which
//! classic row plays each part the kernel names. Written to `roles.ron`; the
//! handicap roles live in the hand-written `handicaps.ron`.

use super::ron_out::{quote, Record};

/// The terrain of every ground; ocean is the deep water.
const GROUNDS: [(&str, &str); 8] = [
    ("Water", "core:terrain/coast"),
    ("Mountains", "core:terrain/mountain"),
    ("Hills", "core:terrain/hill"),
    ("Flat(Humid)", "core:terrain/grassland"),
    ("Flat(Temperate)", "core:terrain/plains"),
    ("Flat(Arid)", "core:terrain/desert"),
    ("Flat(Cold)", "core:terrain/tundra"),
    ("Flat(Frozen)", "core:terrain/snow"),
];

/// The feature of every feature role.
const FEATURES: [(&str, &str); 8] = [
    ("Forest", "core:feature/forest"),
    ("Jungle", "core:feature/jungle"),
    ("Marsh", "core:feature/marsh"),
    ("Oasis", "core:feature/oasis"),
    ("FloodPlains", "core:feature/flood-plains"),
    ("Ice", "core:feature/ice"),
    ("Atoll", "core:feature/atoll"),
    ("Fallout", "core:feature/fallout"),
];

/// The regular unit of every starting role.
const STARTING_UNITS: [(&str, &str); 3] = [
    ("Defense", "core:unit/warrior"),
    ("Worker", "core:unit/worker"),
    ("Explore", "core:unit/scout"),
];

/// The vocabulary key a technology unlocks each process with.
const PROCESSES: [(&str, &str); 2] = [("Wealth", "core:process/wealth"), ("Research", "core:process/research")];

/// The order that breaks a tie between policy branches the AI values alike
/// (`ai::policy_branches` scores them by their declared effects).
const AI_BRANCHES: [&str; 9] = [
    "core:policy/tradition",
    "core:policy/piety",
    "core:policy/liberty",
    "core:policy/patronage",
    "core:policy/aesthetics",
    "core:policy/commerce",
    "core:policy/rationalism",
    "core:policy/honor",
    "core:policy/exploration",
];

/// The ideology the AI picks: Order when wide, Autocracy at war, else Freedom.
const AI_IDEOLOGIES: [(&str, &str); 3] =
    [("Wide", "core:policy/order"), ("AtWar", "core:policy/autocracy"), ("AtPeace", "core:policy/freedom")];

/// Completing the branch lets a player buy the great person with faith
/// (rules.md; the catalog tables do not say it, so it is coded here).
const FAITH_GREAT_PEOPLE: [(&str, &str); 8] = [
    ("Writer", "core:policy/aesthetics"),
    ("Artist", "core:policy/aesthetics"),
    ("Musician", "core:policy/aesthetics"),
    ("Scientist", "core:policy/rationalism"),
    ("Merchant", "core:policy/commerce"),
    ("Engineer", "core:policy/tradition"),
    ("General", "core:policy/honor"),
    ("Admiral", "core:policy/exploration"),
];

/// The ideology public opinion prefers on a tie (rules.md).
const IDEOLOGY_TIES: [(&str, &str); 3] = [
    ("core:policy/freedom", "core:policy/order"),
    ("core:policy/autocracy", "core:policy/freedom"),
    ("core:policy/order", "core:policy/autocracy"),
];

/// `[(Role, "key"), ...]` items, one per line.
fn pairs(bindings: &[(&str, &str)]) -> Vec<String> {
    bindings.iter().map(|(role, key)| format!("({role}, {})", quote(key))).collect()
}

pub fn roles() -> Record {
    Record::new()
        .lines("grounds", pairs(&GROUNDS))
        .opt_str("deep_water", Some("core:terrain/ocean"))
        .lines("features", pairs(&FEATURES))
        .lines("starting_units", pairs(&STARTING_UNITS))
        .lines("processes", pairs(&PROCESSES))
        .opt_str("road", Some("core:route/road"))
        .opt_str("default_specialist", Some("core:specialist/citizen"))
        .lines("ai_branches", AI_BRANCHES.iter().map(|k| quote(k)).collect())
        .lines("ai_ideologies", pairs(&AI_IDEOLOGIES))
        .lines("faith_great_people", pairs(&FAITH_GREAT_PEOPLE))
        .lines("ideology_ties", IDEOLOGY_TIES.iter().map(|(a, b)| format!("({}, {})", quote(a), quote(b))).collect())
}
