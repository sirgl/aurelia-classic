//! Which unit actions the AI uses (classic/docs/ai-unit-abilities.md): every
//! seat of a suite's games plays one variant through a counting mind; for
//! each kind of unit command it prints the unit-turns it was legal in and
//! how often the AI issued it, and per nation the cities at the end.
//!
//! `cargo run --release -p aurelia-qa --bin ai-abilities -- [options]`
//!
//! --variant X       the variant every seat plays (default: `standard`)
//! --suite S         smoke|standard|duel|pace (default standard)
//! --seed-offset K   an independent suite of the same shape
//! --jobs N          games in parallel (default 3)
//! --limit N         only the suite's first N games
//! --nation KEY      the first nation of every game is this one (a rules key,
//!                   `core:nation/venice`); default: the suite's own nations
//!
//! Only the nations' units are counted (city-states and barbarians are not).
//! The player's own commands (policies, deals, the Congress, purchases …) are
//! counted as issued under `player:<Kind>` (they have no legal list).
//! Legal: the unit-turns in which `knowledge::actions::unit_actions` listed
//! the kind for the unit, enumerated at the turn's start, at each of the
//! unit's decisions and at the end of the turn (once per unit, round and
//! kind). Issued: the commands of the kind in the AI's decisions, in every
//! phase. `FoundReligion`, `EnhanceReligion` and `MarchTo` are not
//! enumerated (their legal count is empty). Holders: the unit-turns of units
//! whose kind carries the ability (`UnitAbility`), once per unit and round.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use aurelia_qa::bench::modules::{majors, BenchRules, ClassicBench};
use aurelia_qa::bench::suite::{Mode, Suite};
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::planner::TurnBrief;
use classic_rules::classic::Classic;
use classic_rules::derived::great_people::GreatPersonAction;
use classic_rules::knowledge::actions::{acting_unit, unit_actions};
use classic_rules::knowledge::observation::{AIObservation, LocalObservation};
use classic_rules::{Command, CompiledRuleset, GameState};
use kernel::ai::driver::AiDriver;
use kernel::ai::mind::{AiMind, TurnStart};
use kernel::ai::plan::{AiSettings, Decision, PlanTools};
use kernel::ai::state::AiState;
use kernel::ids::{PlayerId, UnitHandle};

/// The kind of a unit command: its variant, a great person action by its
/// action.
fn kind(command: &Command) -> String {
    if let Command::GreatPersonAction { action, .. } = command {
        let action = match action {
            GreatPersonAction::Construct(_) => "Construct".to_owned(),
            other => format!("{other:?}"),
        };
        return format!("GreatPersonAction::{action}");
    }
    let text = format!("{command:?}");
    text.split(|c: char| !c.is_alphanumeric()).next().unwrap_or("").to_owned()
}

/// An ability by its variant name.
fn ability_name(ability: &classic_rules::classic::content::UnitAbility) -> String {
    let text = format!("{ability:?}");
    text.split(|c: char| !c.is_alphanumeric()).next().unwrap_or("").to_owned()
}

#[derive(Debug, Default, Clone)]
struct Tally {
    legal: BTreeMap<String, u64>,
    issued: BTreeMap<String, u64>,
    holders: BTreeMap<String, u64>,
    /// Distinct units the kind was legal for, and that were given it.
    legal_units: BTreeMap<String, u64>,
    issued_units: BTreeMap<String, u64>,
    /// Issued by nation and kind.
    by_nation: BTreeMap<(String, String), u64>,
}

impl Tally {
    fn add(&mut self, other: &Tally) {
        for (k, v) in &other.legal {
            *self.legal.entry(k.clone()).or_default() += v;
        }
        for (k, v) in &other.issued {
            *self.issued.entry(k.clone()).or_default() += v;
        }
        for (k, v) in &other.legal_units {
            *self.legal_units.entry(k.clone()).or_default() += v;
        }
        for (k, v) in &other.issued_units {
            *self.issued_units.entry(k.clone()).or_default() += v;
        }
        for (k, v) in &other.holders {
            *self.holders.entry(k.clone()).or_default() += v;
        }
        for (k, v) in &other.by_nation {
            *self.by_nation.entry(k.clone()).or_default() += v;
        }
    }
}

/// A unit's kind of command (or ability) once a round: player, unit, kind.
type Seen = (u8, u64, String);

/// A mind that counts the legal and the issued unit commands; every call is
/// delegated unchanged.
struct Counting<'r> {
    mind: ClassicMind,
    rules: &'r CompiledRuleset,
    settings: AiSettings,
    nations: BTreeMap<u8, String>,
    tally: RefCell<Tally>,
    /// (round, player, unit, kind) seen legal; cleared every round.
    seen: RefCell<(u32, HashSet<Seen>)>,
    /// (kind, unit) seen legal, and given, over the game.
    units_legal: RefCell<HashSet<(String, UnitHandle)>>,
    units_issued: RefCell<HashSet<(String, UnitHandle)>>,
}

impl std::fmt::Debug for Counting<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Counting").field("mind", &self.mind).finish_non_exhaustive()
    }
}

impl Counting<'_> {
    fn legal(&self, round: u32, player: PlayerId, near: &LocalObservation<'_>) {
        if !self.nations.contains_key(&player.0) {
            return;
        }
        let mut seen = self.seen.borrow_mut();
        if seen.0 != round {
            *seen = (round, HashSet::new());
        }
        let mut tally = self.tally.borrow_mut();
        for command in unit_actions(near) {
            let kind = kind(&command);
            if self.units_legal.borrow_mut().insert((kind.clone(), near.unit.handle)) {
                *tally.legal_units.entry(kind.clone()).or_default() += 1;
            }
            if seen.1.insert((player.0, near.unit.id.0, kind.clone())) {
                *tally.legal.entry(kind).or_default() += 1;
            }
        }
    }

    fn issued(&self, player: PlayerId, decisions: &[Decision<Classic>]) {
        let Some(nation) = self.nations.get(&player.0).cloned() else { return };
        let mut tally = self.tally.borrow_mut();
        for command in decisions.iter().filter_map(|d| d.command.as_ref()) {
            if let Some(unit) = acting_unit(command) {
                let kind = kind(command);
                if self.units_issued.borrow_mut().insert((kind.clone(), unit)) {
                    *tally.issued_units.entry(kind.clone()).or_default() += 1;
                }
                *tally.by_nation.entry((nation.clone(), kind.clone())).or_default() += 1;
                *tally.issued.entry(kind).or_default() += 1;
            } else {
                // A player's own command (policies, deals, the Congress, purchases …): issued only.
                *tally.issued.entry(format!("player:{}", kind(command))).or_default() += 1;
            }
        }
    }

    /// Every own unit's legal list where it stands, and the holders of the
    /// abilities (once a round).
    fn survey(&self, game: &GameState, rules: &CompiledRuleset, player: PlayerId, sight: &AIObservation<'_>) {
        if !game.view().auth.players().is_major(player) {
            return;
        }
        for unit in &sight.own_units {
            if let Some(near) = self.mind.sense_unit(game, rules, &self.settings, player, unit.handle) {
                self.legal(sight.round, player, &near);
            }
        }
        let mut seen = self.seen.borrow_mut();
        if seen.0 != sight.round {
            *seen = (sight.round, HashSet::new());
        }
        let mut tally = self.tally.borrow_mut();
        for unit in &sight.own_units {
            let Some(def) = self.rules.units.get(unit.kind.0 as usize) else { continue };
            for ability in &def.module.abilities {
                let name = ability_name(ability);
                if seen.1.insert((player.0, unit.id.0, format!("has {name}"))) {
                    *tally.holders.entry(name).or_default() += 1;
                }
            }
        }
    }
}

impl AiMind<Classic> for Counting<'_> {
    type Brief = TurnBrief;
    type Sight<'g> = AIObservation<'g>;
    type Near<'g> = LocalObservation<'g>;

    fn note_humans(&mut self, humans: &[PlayerId]) {
        self.mind.note_humans(humans);
    }

    fn sense<'g>(&self, game: &'g GameState, rules: &'g CompiledRuleset, player: PlayerId) -> AIObservation<'g> {
        let sight = self.mind.sense(game, rules, player);
        self.survey(game, rules, player, &sight);
        sight
    }

    fn sense_unit<'g>(&self, game: &'g GameState, rules: &'g CompiledRuleset, settings: &AiSettings, player: PlayerId, unit: UnitHandle) -> Option<LocalObservation<'g>> {
        self.mind.sense_unit(game, rules, settings, player, unit)
    }

    fn start_turn(&self, sight: &AIObservation<'_>, rules: &CompiledRuleset, player: PlayerId, state: &mut AiState) -> TurnStart<Classic, TurnBrief> {
        let start = self.mind.start_turn(sight, rules, player, state);
        self.issued(player, &start.opening);
        start
    }

    fn plan_strategic(&self, brief: &TurnBrief, state: &mut AiState, rules: &CompiledRuleset) -> Vec<Decision<Classic>> {
        let decisions = self.mind.plan_strategic(brief, state, rules);
        self.issued(brief.player, &decisions);
        decisions
    }

    fn decide_unit(&self, near: &LocalObservation<'_>, player: PlayerId, unit: UnitHandle, brief: &TurnBrief, state: &mut AiState, tools: &mut PlanTools<'_, Classic>) -> Option<Decision<Classic>> {
        self.legal(near.round, player, near);
        let decision = self.mind.decide_unit(near, player, unit, brief, state, tools);
        if let Some(decision) = &decision {
            self.issued(player, std::slice::from_ref(decision));
        }
        decision
    }

    fn permits(&self, game: &GameState, rules: &CompiledRuleset, player: PlayerId, command: &Command) -> bool {
        self.mind.permits(game, rules, player, command)
    }

    fn plan_end_of_turn(&self, sight: &AIObservation<'_>, player: PlayerId, brief: &TurnBrief, state: &AiState, tools: &mut PlanTools<'_, Classic>) -> Vec<Decision<Classic>> {
        let decisions = self.mind.plan_end_of_turn(sight, player, brief, state, tools);
        self.issued(player, &decisions);
        decisions
    }
}

/// One nation's end of one game.
#[derive(Debug, Clone)]
struct NationEnd {
    nation: String,
    cities: u32,
    score: i64,
    /// 1: the best score of the game's nations.
    rank: u32,
}

/// The suite's game, its first nation `nation` when given.
fn new_game(rules: &CompiledRuleset, suite: &Suite, game_match: &aurelia_qa::bench::suite::Match, nation: Option<u8>) -> GameState {
    let Some(nation) = nation else {
        return ClassicBench::new_game(rules, &game_match.map, game_match.seed, suite.rounds);
    };
    let script = classic_rules::scenario::MapScript::from_key(game_match.map.script).expect("map script");
    let size = aurelia_qa::spec::size_from_name(game_match.map.size).expect("world size");
    let spec = aurelia_qa::GameSpec::new(script, size, game_match.seed).max_rounds(suite.rounds);
    let (config, options) = spec.scenario(rules);
    classic_rules::scenario::generate_new_game_with(rules, &config, Some(nation), options)
}

fn play(rules: &CompiledRuleset, suite: &Suite, game_match: &aurelia_qa::bench::suite::Match, variant: &str, nation: Option<&str>) -> Result<(Tally, Vec<NationEnd>), String> {
    let nation = match nation {
        Some(key) => Some(rules.nations.iter().position(|n| n.key == key).ok_or_else(|| format!("no nation {key}"))? as u8),
        None => None,
    };
    let mut game = new_game(rules, suite, game_match, nation);
    let nations: BTreeMap<u8, String> = majors(&game).into_iter().map(|p| (p.0, ClassicBench::leader(&game, rules, p))).collect();
    let mind = ClassicBench::mind(variant).ok_or_else(|| format!("no variant {variant}"))?;
    let settings = ClassicBench::settings(rules);
    let counting = Counting { mind, rules, settings: settings.clone(), nations: nations.clone(), tally: RefCell::default(), seen: RefCell::default(), units_legal: RefCell::default(), units_issued: RefCell::default() };
    let mut driver: AiDriver<Classic, Counting<'_>> = AiDriver::with_mind(settings, Vec::new(), counting);
    driver.set_trace_capacity(0);
    while !game.is_over() && game.view().auth.turn().round <= suite.rounds {
        let report = driver.run(&mut game, rules, u32::MAX);
        if !report.round_done && !game.is_over() {
            return Err(format!("{}: round {} waits for a human", game_match.label(), game.view().auth.turn().round));
        }
    }
    let tally = driver.mind_mut().tally.borrow().clone();
    let mut ends: Vec<NationEnd> = nations
        .iter()
        .map(|(p, nation)| {
            let player = PlayerId(*p);
            NationEnd { nation: nation.clone(), cities: game.view().auth.indexes().cities_of(player).len() as u32, score: ClassicBench::score(&game, rules, player), rank: 0 }
        })
        .collect();
    let scores: Vec<i64> = ends.iter().map(|e| e.score).collect();
    for end in &mut ends {
        end.rank = 1 + scores.iter().filter(|s| **s > end.score).count() as u32;
    }
    Ok((tally, ends))
}

fn main() {
    let mut variant = "standard".to_owned();
    let mut suite_name = "standard".to_owned();
    let mut offset = 0u64;
    let mut jobs = 3usize;
    let mut nation: Option<String> = None;
    let mut limit: Option<usize> = None;
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let value = args.next().unwrap_or_else(|| panic!("{flag} needs a value"));
        match flag.as_str() {
            "--variant" => variant = value,
            "--suite" => suite_name = value,
            "--seed-offset" => offset = value.parse().expect("--seed-offset"),
            "--jobs" => jobs = value.parse().expect("--jobs"),
            "--nation" => nation = Some(value),
            "--limit" => limit = Some(value.parse().expect("--limit")),
            other => panic!("unknown option {other}"),
        }
    }
    let suite = Suite::named(&suite_name).unwrap_or_else(|| panic!("unknown suite {suite_name}"));
    let mut matches = suite.matches(Mode::Solo, offset);
    if let Some(limit) = limit {
        matches.truncate(limit);
    }
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<(Tally, Vec<NationEnd>)>> = Mutex::new(Vec::new());
    let started = std::time::Instant::now();
    std::thread::scope(|scope| {
        for _ in 0..jobs.clamp(1, matches.len().max(1)) {
            scope.spawn(|| {
                let rules = ClassicBench::rules();
                loop {
                    let at = next.fetch_add(1, Ordering::Relaxed);
                    let Some(game_match) = matches.get(at) else { break };
                    match play(&rules, &suite, game_match, &variant, nation.as_deref()) {
                        Ok(result) => {
                            // The first nation's end, for a pairing across variants.
                            if let (Some(_), Some(first)) = (&nation, result.1.first()) {
                                eprintln!("GAME {} {} {} {} {}", game_match.label().replace(' ', "_"), first.nation, first.cities, first.score, first.rank);
                            }
                            results.lock().expect("results").push(result)
                        }
                        Err(e) => eprintln!("{e}"),
                    }
                    eprintln!("{} done ({:.0} s)", game_match.label(), started.elapsed().as_secs_f64());
                }
            });
        }
    });
    let results = results.into_inner().expect("results");
    let mut tally = Tally::default();
    let mut ends: Vec<NationEnd> = Vec::new();
    for (t, e) in &results {
        tally.add(t);
        ends.extend(e.iter().cloned());
    }
    println!("# {variant}, suite {suite_name} offset {offset}: {} games\n", results.len());
    println!("| command | legal unit-turns | issued | issued / legal | units legal | units given it | units given / legal |");
    println!("|---|---:|---:|---:|---:|---:|---:|");
    let kinds: std::collections::BTreeSet<&String> = tally.legal.keys().chain(tally.issued.keys()).collect();
    for kind in kinds {
        let legal = tally.legal.get(kind).copied().unwrap_or(0);
        let issued = tally.issued.get(kind).copied().unwrap_or(0);
        let share = if legal > 0 { format!("{:.1} %", 100.0 * issued as f64 / legal as f64) } else { "–".to_owned() };
        let (lu, iu) = (tally.legal_units.get(kind).copied().unwrap_or(0), tally.issued_units.get(kind).copied().unwrap_or(0));
        let unit_share = if lu > 0 { format!("{:.1} %", 100.0 * iu as f64 / lu as f64) } else { "–".to_owned() };
        println!("| {kind} | {legal} | {issued} | {share} | {lu} | {iu} | {unit_share} |");
    }
    println!("\n| ability | holder unit-turns |");
    println!("|---|---:|");
    for (ability, n) in &tally.holders {
        println!("| {ability} | {n} |");
    }
    println!("\n| nation | games | cities at the end (mean ± 95 %) | score (mean ± 95 %) | rank (mean) | great person actions issued |");
    println!("|---|---:|---:|---:|---:|---|");
    let mut by_nation: BTreeMap<&str, Vec<&NationEnd>> = BTreeMap::new();
    for end in &ends {
        by_nation.entry(end.nation.as_str()).or_default().push(end);
    }
    for (nation, list) in by_nation {
        let n = list.len() as f64;
        let interval = |values: Vec<f64>| {
            let mean = values.iter().sum::<f64>() / n;
            let var = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n - 1.0).max(1.0);
            (mean, 1.96 * (var / n).sqrt())
        };
        let (cities, cities_ci) = interval(list.iter().map(|e| f64::from(e.cities)).collect());
        let (score, score_ci) = interval(list.iter().map(|e| e.score as f64).collect());
        let rank = list.iter().map(|e| f64::from(e.rank)).sum::<f64>() / n;
        let actions: Vec<String> = tally
            .by_nation
            .iter()
            .filter(|((na, k), _)| na == nation && k.starts_with("GreatPersonAction::"))
            .map(|((_, k), v)| format!("{} {v}", k.trim_start_matches("GreatPersonAction::")))
            .collect();
        println!("| {nation} | {} | {cities:.2} ± {cities_ci:.2} | {score:.0} ± {score_ci:.0} | {rank:.2} | {} |", list.len(), actions.join(", "));
    }
}
