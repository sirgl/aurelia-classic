//! Every lever `net2` declares has a reader (lane `lever-readers`, 23 November). The class: a lever written by the net
//! and read by no executor is a decision that is never played. `GUARD_PURCHASE` (design §4.5) was declared
//! with no reader from the promotion on; it is removed.
//!
//! The guard reads `plan.rs` itself: every field of `Net2Plan`, `CityGuard`, `Hold`, `CityHold`, `WarOrder` and `Offence` and every `pub const` there
//! must be in `READERS` with the file that reads it and the token it reads it by, and that token must stand on a line
//! of code (not a comment) of that file. A lever declared and not yet built names why in `DECLARED` instead (the
//! offence addendum, off by `weights::OFFENCE`); a new lever with neither fails here.

/// (the lever, the file under `src/ai/` that reads it, the token on a code line of that file).
const READERS: &[(&str, &str, &str)] = &[
    // Net2Plan
    ("parts", "planner.rs", "plan.parts.plays(part)"),
    ("war", "operations.rs", "plan.war"),
    ("peace_with", "peace.rs", "makes_peace_with(enemy)"),
    ("rivals", "operations.rs", "plan.rival(rival)"),
    ("guard", "defence.rs", "plan.guard_of(city.id)"),
    ("cs_gold_share", "city_states.rs", "plan.cs_gold_share"),
    ("embargo", "congress.rs", "p.embargo"),
    ("ballot", "congress.rs", "plan.ballot"),
    ("escorts", "fleet.rs", "plan.escorts"),
    ("generals", "great_people.rs", "plan.generals"),
    ("tall_share", "planner_settle.rs", "read.plan.tall_share"),
    ("order_eases", "peace.rs", "p.order_eases"),
    ("space_hold", "planner_cities.rs", "p.space_hold"),
    ("space_research", "strategy_net/heads.rs", "n.space_research"),
    ("capital_gate", "victory.rs", "|p| p.capital_gate"),
    ("science", "economy.rs", "plan.science.scientists"),
    ("hold", "planner_units.rs", "plan.hold.extra_at(city)"),
    // Hold and CityHold: the hold of the cities (`layout-defence`)
    ("cities", "planner_cities.rs", "p.hold.cities.is_empty()"),
    ("chest", "allocator.rs", "plan.hold.chest"),
    ("tile", "net2/plan.rs", "c.tile == tile"),
    ("level", "net2/hold.rs", "(c.level, std::cmp::Reverse(c.city))"),
    ("extra", "net2/plan.rs", "usize::from(c.extra)"),
    // CityGuard: the guard's level by city
    ("city", "net2/plan.rs", "g.city == city"),
    ("level", "defence.rs", "plan.guard_of(city.id)"),
    // WarOrder: what the war machine reads, and the order's life that the net keeps itself
    ("rival", "operations.rs", "o.rival == city.owner"),
    ("kind", "operations.rs", "order.kind == WarKind::Retake"),
    ("since", "net2/levers.rs", "order.since +"),
    ("budget", "net2/levers.rs", "since + order.budget"),
    ("phase", "operations.rs", "o.war_since()"),
    ("value", "retake.rs", "map(|o| o.value)"),
    ("aim", "operations.rs", "order.aim"),
    ("raid", "raid_war.rs", "o.raid"),
    // the thresholds an executor compares a lever with
    ("SOLDIERS_DOUBT_MAX", "operations.rs", "SOLDIERS_DOUBT_MAX"),
    ("TRUST_MIN", "diplomacy.rs", "TRUST_MIN"),
    // the input caps and the default, read inside the net
    ("MAX_RIVALS", "net2/rival_inputs.rs", "MAX_RIVALS"),
    ("MAX_TARGETS", "net2/inputs.rs", "MAX_TARGETS"),
    ("TRUST_UNKNOWN", "net2/plan.rs", "None => TRUST_UNKNOWN"),
];

/// (the lever, why it has no reader yet).
const DECLARED: &[(&str, &str)] = &[
    ("round", "the plan's stamp (the round it was thought), not a lever"),
    ("offence", "the offence addendum's levers: `weights::OFFENCE` is false and `levers::after_intent` writes `Offence::default()`; their readers are on `work/tricks` (tricks/NEEDS-FROM-NET2.md)"),
    ("opening", "the offence addendum's staged opening (MO3): `Opening::Plain` until the offence is built"),
    ("hire", "offence addendum, off"),
    ("milk", "offence addendum, off"),
    ("sell_spares", "offence addendum, off"),
    ("ask_entry", "offence addendum, off"),
    ("tourism_entry", "offence addendum, off"),
    ("cs_sweep", "offence addendum, off"),
    ("ban_luxury", "offence addendum, off"),
    ("coup_floor", "offence addendum, off"),
    ("bully_worker", "offence addendum, off"),
    ("faith_target", "offence addendum, off"),
    ("citadel_at", "offence addendum, off"),
    ("liberate", "offence addendum, off"),
];

fn ai_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ai")
}

/// The field names of `pub struct <name> { … }` in `source`.
fn fields(source: &str, name: &str) -> Vec<String> {
    let head = format!("pub struct {name} {{");
    let start = source.find(&head).unwrap_or_else(|| panic!("plan.rs declares {name}")) + head.len();
    let body = &source[start..start + source[start..].find("\n}").expect("the struct ends")];
    body.lines()
        .map(str::trim)
        .filter(|l| l.starts_with("pub ") && l.contains(':'))
        .map(|l| l["pub ".len()..l.find(':').unwrap()].trim().to_owned())
        .collect()
}

/// The names of the top-level `pub const`s in `source`.
fn consts(source: &str) -> Vec<String> {
    source
        .lines()
        .filter_map(|l| l.strip_prefix("pub const "))
        .map(|l| l[..l.find(':').unwrap()].trim().to_owned())
        .collect()
}

/// `source` without its comments (`//` to the end of a line).
fn code(source: &str) -> String {
    source.lines().map(|l| l.find("//").map_or(l, |i| &l[..i])).collect::<Vec<_>>().join("\n")
}

#[test]
fn every_net2_lever_has_a_reader() {
    let plan = std::fs::read_to_string(ai_dir().join("net2/plan.rs")).expect("plan.rs is read");
    let mut levers: Vec<String> = ["Net2Plan", "CityGuard", "Hold", "CityHold", "WarOrder", "Offence"].iter().flat_map(|s| fields(&plan, s)).collect();
    levers.extend(consts(&plan));
    assert!(levers.len() > 30, "the parse finds the levers: {levers:?}");
    let mut missing = Vec::new();
    for lever in &levers {
        let declared = DECLARED.iter().any(|(name, _)| name == lever);
        let read: Vec<&(&str, &str, &str)> = READERS.iter().filter(|(name, _, _)| name == lever).collect();
        if read.is_empty() && !declared {
            missing.push(format!("{lever}: no reader and not declared"));
        }
        for (_, file, token) in read {
            let source = std::fs::read_to_string(ai_dir().join(file)).unwrap_or_else(|_| panic!("{file} is read"));
            if !code(&source).contains(token) {
                missing.push(format!("{lever}: `{token}` not on a code line of {file}"));
            }
        }
    }
    for (name, _) in DECLARED {
        assert!(levers.iter().any(|l| l == name), "{name} is declared here and no longer in plan.rs: drop it");
    }
    assert!(missing.is_empty(), "net2's levers without a reader (wire one, or declare it with why): {missing:#?}");
}
