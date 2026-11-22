//! The trace of `net2` for its float twin (`classic/nn/aurelia_nn/net2`, design §3.2): the weight tables
//! as JSON (`export`) and one JSON line a turn with the inputs and every neuron (`line`). With
//! `NET2_TRACE=<file>` the game appends a line a turn (`turn`); the parity fixture is the same lines on
//! generated turns (`tests::the_parity_fixture_is_the_integer_net`).

use std::fmt::Write as _;

use crate::ai::strategy_net::weights::Act;
use crate::ai::strategy_net::{Logits, Milli, Tilt};

use super::levers::{After, Pass};
use super::plan::{Ballot, CityInputs, CoreMemory, Embargo, Help, Inputs, TargetInputs, WarPlan};
use super::weights::{self as w, Join, Neuron, Shape, Src, Table, Term};

/// The JSON of every table of `net2`: the neurons, the tilt, the gates, the site head.
pub fn export() -> String {
    let mut out = String::from("{\n  \"neurons\": {\n");
    let neurons = w::neurons();
    for (i, n) in neurons.iter().enumerate() {
        let comma = if i + 1 < neurons.len() { "," } else { "" };
        let _ = writeln!(out, "    \"{}\": {}{comma}", n.name, neuron_json(n));
    }
    out.push_str("  },\n");
    let _ = writeln!(out, "  \"seat\": [{}],", names(&w::SEAT));
    let _ = writeln!(out, "  \"rival\": [{}],", names(&w::RIVAL));
    let tilt: Vec<String> = w::TILT.iter().map(|(table, at, term)| format!("{{\"table\": \"{}\", \"at\": {at}, \"term\": {}}}", table_name(*table), term_json(term))).collect();
    let _ = writeln!(out, "  \"tilt\": [\n    {}\n  ],", tilt.join(",\n    "));
    let gates = [("war_start", w::gate::WAR_START.0), ("war_keep", w::gate::WAR_KEEP.0), ("peace", w::gate::PEACE.0), ("deny_help", w::gate::DENY_HELP.0), ("embargo", w::gate::EMBARGO.0), ("block", w::gate::BLOCK.0)];
    let gates: Vec<String> = gates.iter().map(|(k, v)| format!("\"{k}\": {v}")).collect();
    let _ = writeln!(out, "  \"gates\": {{{}}},", gates.join(", "));
    let names = format!(
        "{{\"g\": [{}], \"r\": [{}], \"a\": [{}], \"t\": [{}], \"at_war_feature\": {}}}",
        quoted(&w::g::NAMES),
        quoted(&w::r::NAMES),
        quoted(&w::a::NAMES),
        quoted(&w::t::NAMES),
        crate::ai::strategy_net::weights::fe::AT_WAR
    );
    let _ = writeln!(out, "  \"names\": {names},");
    let _ = writeln!(out, "  \"site\": {{\"bound\": {}, \"gap\": {}, \"army\": {}, \"travel\": {}, \"fill\": {}}}", w::site::BOUND.0, w::site::GAP.0, w::site::ARMY.0, w::site::TRAVEL.0, w::site::FILL.0);
    out.push_str("}\n");
    out
}

fn quoted(names: &[&str]) -> String {
    let names: Vec<String> = names.iter().map(|n| format!("\"{n}\"")).collect();
    names.join(", ")
}

fn names(table: &[Neuron]) -> String {
    let names: Vec<String> = table.iter().map(|n| format!("\"{}\"", n.name)).collect();
    names.join(", ")
}

fn table_name(table: Table) -> &'static str {
    match table {
        Table::Style => "style",
        Table::Aim => "aim",
        Table::Posture => "posture",
    }
}

fn neuron_json(n: &Neuron) -> String {
    let join = match n.join {
        Join::Sum(Act::Unit) => "unit",
        Join::Sum(Act::Linear) => "linear",
        Join::Max => "max",
    };
    let terms: Vec<String> = n.terms.iter().map(term_json).collect();
    format!("{{\"bias\": {}, \"join\": \"{join}\", \"terms\": [{}]}}", n.bias, terms.join(", "))
}

fn src_json(src: Src) -> String {
    let (layer, i) = match src {
        Src::F(i) => ("F", i),
        Src::K(i) => ("K", i),
        Src::G(i) => ("G", i),
        Src::S(i) => ("S", i),
        Src::R(i) => ("R", i),
        Src::Q(i) => ("Q", i),
        Src::A(i) => ("A", i),
        Src::Y(i) => ("Y", i),
        Src::C(i) => ("C", i),
        Src::T(i) => ("T", i),
    };
    format!("[\"{layer}\", {i}]")
}

fn term_json(term: &Term) -> String {
    let shape = match term.shape {
        Shape::Plain => "[\"plain\"]".to_string(),
        Shape::Not => "[\"not\"]".to_string(),
        Shape::Above(c) => format!("[\"above\", {c}]"),
        Shape::Centered(c) => format!("[\"centered\", {c}]"),
        Shape::Times(other) => format!("[\"times\", {}]", src_json(other)),
    };
    format!("{{\"src\": {}, \"shape\": {shape}, \"w\": {}}}", src_json(term.src), term.w)
}

/// What one turn's line carries.
pub struct Record<'a> {
    pub player: u8,
    pub round: u32,
    /// The game's pace (percent of Standard's rounds).
    pub pace: u32,
    pub inputs: &'a Inputs,
    /// The core's memory before the turn (the war order held).
    pub held: &'a CoreMemory,
    /// The carried logits before the tilt.
    pub logits: &'a Logits,
    /// The intent's softmaxes run on `sharp`'s temperatures (`strategy_net::sharp::w`).
    pub tempered: bool,
    pub pass: &'a Pass,
    pub tilt: &'a Tilt,
    pub shares: &'a [Milli],
    pub after: &'a After,
}

fn list<T: std::fmt::Display>(values: &[T]) -> String {
    let parts: Vec<String> = values.iter().map(|v| v.to_string()).collect();
    format!("[{}]", parts.join(","))
}

fn rows<T, F: Fn(&T) -> String>(items: &[T], each: F) -> String {
    let parts: Vec<String> = items.iter().map(each).collect();
    format!("[{}]", parts.join(","))
}

fn war_json(war: &WarPlan) -> String {
    match war.order() {
        Some(o) => {
            let aim = match o.aim {
                Some(aim) => aim.city.0.to_string(),
                None => "null".to_string(),
            };
            let war_since = match o.war_since() {
                Some(since) => since.to_string(),
                None => "null".to_string(),
            };
            format!("{{\"rival\":{},\"kind\":\"{}\",\"since\":{},\"budget\":{},\"war_since\":{war_since},\"value\":{},\"aim\":{aim}}}", o.rival.0, o.kind.name(), o.since, o.budget, o.value)
        }
        None => "null".to_string(),
    }
}

fn target_json(target: &TargetInputs) -> String {
    format!("{{\"rival\":{},\"city\":{},\"t\":{}}}", target.rival.0, target.city.0, list(&target.row()))
}

fn core_json(core: &CoreMemory) -> String {
    format!("{{\"war\":{},\"spent\":{}}}", war_json(&core.war), rows(&core.spent, |s| format!("[{},{}]", s.rival.0, s.until)))
}

fn city_json(city: &CityInputs) -> String {
    let owner = match city.stack_owner {
        Some(p) => p.0.to_string(),
        None => "null".to_string(),
    };
    let near: Vec<u8> = city.rivals_near.iter().map(|p| p.0).collect();
    format!("{{\"stack_owner\":{owner},\"danger\":{},\"near\":{},\"young\":{}}}", city.danger, list(&near), city.young)
}

fn input_json(rec: &Record<'_>) -> String {
    let inputs = rec.inputs;
    let l = rec.logits;
    format!(
        "{{\"round\":{},\"pace\":{},\"held\":{},\"f\":{},\"k\":{},\"g\":{},\"rivals\":{},\"cities\":{},\"targets\":{},\"logits\":{{\"style\":{},\"aim\":{},\"posture\":{}}},\"tempered\":{}}}",
        rec.round,
        rec.pace,
        core_json(rec.held),
        list(&inputs.situation),
        list(&rec.pass.carried),
        list(&inputs.seat.row()),
        rows(&rec.pass.rivals, |x| format!("{{\"id\":{},\"r\":{}}}", x.rival.0, list(&x.inputs))),
        rows(&inputs.cities, city_json),
        rows(&inputs.targets, target_json),
        list(&l.style),
        list(&l.aim),
        list(&l.posture),
        rec.tempered
    )
}

fn output_json(rec: &Record<'_>) -> String {
    let plan = &rec.after.plan;
    let guards: Vec<Milli> = plan.guard.iter().map(|g| g.level).collect();
    let peace_with: Vec<u8> = plan.peace_with.iter().map(|p| p.0).collect();
    let deny: Vec<u8> = plan.rivals.iter().filter(|x| x.help == Help::Deny).map(|x| x.rival.0).collect();
    let embargo = match plan.embargo {
        Embargo::On(p) => p.0.to_string(),
        Embargo::Nobody => "null".to_string(),
    };
    let ballot = match plan.ballot {
        Ballot::Block(p) => p.0.to_string(),
        Ballot::Own => "null".to_string(),
    };
    format!(
        "{{\"s\":{},\"ease\":{},\"q\":{},\"core\":{},\"a\":{},\"tilt\":{{\"style\":{},\"aim\":{},\"posture\":{}}},\"y\":{},\"peace\":{},\"peace_with\":{},\"deny_help\":{},\"embargo\":{embargo},\"ballot\":{ballot},\"guard\":{},\"military\":{},\"cs_gold_share\":{},\"part_hold\":{}}}",
        list(&rec.pass.seat),
        list(&rec.pass.eases),
        rows(&rec.pass.rivals, |x| list(&x.concepts)),
        core_json(&rec.pass.core),
        list(&rec.pass.aggregates),
        list(&rec.tilt.style),
        list(&rec.tilt.aim),
        list(&rec.tilt.posture),
        list(rec.shares),
        list(&rec.after.peace),
        list(&peace_with),
        list(&deny),
        list(&guards),
        rec.after.price_tilt[crate::ai::strategy_net::weights::pd::MILITARY],
        plan.cs_gold_share,
        rec.after.part_hold
    )
}

/// One turn as a JSON line: the inputs (`in`) and every neuron (`out`).
pub fn line(rec: &Record<'_>) -> String {
    format!("{{\"player\":{},\"in\":{},\"out\":{}}}", rec.player, input_json(rec), output_json(rec))
}

/// The turn's line appended to `NET2_TRACE`'s file, when it names one.
pub fn turn(rec: &Record<'_>) {
    let Some(path) = std::env::var_os("NET2_TRACE") else {
        return;
    };
    use std::io::Write as _;
    let file = std::fs::OpenOptions::new().create(true).append(true).open(path);
    if let Ok(mut file) = file {
        let _ = writeln!(file, "{}", line(rec));
    }
}
