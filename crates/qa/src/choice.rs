//! The choice dataset: what a city built and what a player researched in
//! AI-only games with exploration, and what followed — data for the choice
//! net (`classic_rules::ai::choice_net`, classic/docs/ai-choice.md).
//!
//! The games are played with `ChoiceMode::Explore`: now and then a choice
//! is a random one of the allocator's top (`ai::choice`), so the outcome of
//! those choices compares the candidates and not the states. Every choice
//! of a production or research is recorded on the thread (`choice::log`)
//! with the rows of its set (the net's inputs) and the candidate taken; when
//! a replanned city chooses twice in one round, the last choice counts.
//!
//! After every round the probe reads, for every nation, its empire (score,
//! technologies, beakers, citizens, cities, production) and each of its
//! cities (population and yields) from the game; a choice's labels are those
//! at the end of its round and `HORIZONS` rounds later (empty when the game
//! ended before, or the city was no longer the player's). The game's end
//! (the winner, the victory and its round) is on every row.
//!
//! The labels read the truth (the score of `world::victory`), the features
//! only the player's own sight.

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::time::Instant;

use classic_rules::ai::choice::{self, ChoiceMode, Explore, Record, CITY_NAMES, EMPIRE_NAMES, PRODUCTION_NAMES, RESEARCH_NAMES};
use classic_rules::ai::choice_net::Kind;
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::variant::Variant;
use classic_rules::{CompiledRuleset, GameState};
use kernel::ai::plan::AiSettings;
use classic_rules::ClassicGame;

use crate::bench::modules::majors;
use crate::record::new_game;
use crate::spec::{victory_name, GameSpec};

/// Rounds after a choice at which its labels are read.
pub const HORIZONS: [u32; 2] = [30, 60];

/// A city as its owner sees it, at the end of a round.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CityState {
    pub population: i64,
    pub food: i64,
    pub production: i64,
    pub science: i64,
    pub gold: i64,
    pub culture: i64,
}

pub const CITY_STATE_NAMES: [&str; 6] = ["population", "food", "production", "science", "gold", "culture"];

impl CityState {
    fn values(&self) -> [i64; 6] {
        [self.population, self.food, self.production, self.science, self.gold, self.culture]
    }
}

/// A nation at the end of a round.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EmpireState {
    /// The score of the time victory (`world::victory::score`).
    pub score: i64,
    pub techs: i64,
    pub beakers: i64,
    pub citizens: i64,
    pub cities: i64,
    pub production: i64,
}

pub const EMPIRE_STATE_NAMES: [&str; 6] = ["score", "techs", "beakers", "citizens", "cities", "production"];

impl EmpireState {
    fn values(&self) -> [i64; 6] {
        [self.score, self.techs, self.beakers, self.citizens, self.cities, self.production]
    }
}

/// How a game ended.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct End {
    /// The winner, the victory (`spec::victory_name`) and its round.
    pub winner: Option<u8>,
    pub victory: Option<&'static str>,
    pub round: u32,
}

/// One choice with its labels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub game: u64,
    pub record: Record,
    /// The city's state and the empire's at the end of the choice's round
    /// (index 0) and `HORIZONS` later.
    pub city: [Option<CityState>; 3],
    pub empire: [Option<EmpireState>; 3],
}

/// Each nation's empire and cities at the end of one round.
type Snapshot = BTreeMap<u8, (EmpireState, Vec<(u64, CityState)>)>;

fn snapshot(game: &GameState, rules: &CompiledRuleset) -> Snapshot {
    let mut out = Snapshot::new();
    for player in majors(game) {
        let obs = game.ai_observation(rules, player);
        let cities: Vec<(u64, CityState)> = obs
            .own_cities
            .iter()
            .map(|c| {
                let state = CityState {
                    population: i64::from(c.population),
                    food: i64::from(c.income.food),
                    production: i64::from(c.income.production),
                    science: i64::from(c.income.science),
                    gold: i64::from(c.income.gold),
                    culture: i64::from(c.income.culture),
                };
                (c.id.0, state)
            })
            .collect();
        let empire = EmpireState {
            score: classic_rules::world::victory::score(game.view().auth, rules, player),
            techs: obs.economy.techs.len() as i64,
            beakers: i64::from(obs.economy.income.science),
            citizens: cities.iter().map(|(_, c)| c.population).sum(),
            cities: cities.len() as i64,
            production: cities.iter().map(|(_, c)| c.production).sum(),
        };
        out.insert(player.0, (empire, cities));
    }
    out
}

/// What a probed game came to.
#[derive(Debug, Clone)]
pub struct Probed {
    pub rows: Vec<Row>,
    pub end: End,
    /// The last round played.
    pub rounds: u32,
    pub hash: u64,
    /// Wall time of the game, and of the snapshots in it.
    pub seconds: f64,
    pub snapshot_seconds: f64,
}

/// Plays `spec` with every seat the variant's mind exploring (`explore`
/// per mille among the top `top`; 0: the variant's own choice, recorded),
/// to its end or `spec.victories.max_rounds`.
pub fn probe_game(rules: &CompiledRuleset, spec: &GameSpec, variant: Variant, permille: u32, top: usize) -> Result<Probed, String> {
    let started = Instant::now();
    let mut game = new_game(rules, spec);
    let settings = AiSettings::for_rules(rules, &spec.difficulty);
    let mode = if permille == 0 { variant.choice_mode() } else { ChoiceMode::Explore(Explore { seed: spec.seed, permille, top }) };
    let mut driver = classic_rules::ai::driver::AiDriver::with_mind(settings, Vec::new(), ClassicMind::with_choice(variant, mode));
    choice::log::enable(true);
    let last = spec.victories.max_rounds;
    let mut snapshots: BTreeMap<u32, Snapshot> = BTreeMap::new();
    let mut records: BTreeMap<(Kind, u8, u32, u64), Record> = BTreeMap::new();
    let mut snapshot_seconds = 0.0;
    let result = loop {
        if game.is_over() || game.view().auth.turn().round > last {
            break Ok(());
        }
        let round = game.view().auth.turn().round;
        let report = driver.run(&mut game, rules, u32::MAX);
        for record in choice::log::take() {
            records.insert((record.kind, record.player, record.round, record.city), record);
        }
        if !report.round_done && !game.is_over() {
            break Err(format!("round {round} waits for a human"));
        }
        let at = Instant::now();
        snapshots.insert(round, snapshot(&game, rules));
        snapshot_seconds += at.elapsed().as_secs_f64();
    };
    choice::log::enable(false);
    result?;
    let outcome = game.outcome();
    let end = End {
        winner: outcome.map(|o| o.winner.0),
        victory: outcome.map(|o| victory_name(o.kind)),
        round: outcome.map_or(game.view().auth.turn().round, |o| o.round),
    };
    let rows = records
        .into_values()
        .map(|record| {
            let at = |offset: u32| snapshots.get(&(record.round + offset)).and_then(|s| s.get(&record.player));
            let mut city = [None; 3];
            let mut empire = [None; 3];
            for (k, offset) in [0].into_iter().chain(HORIZONS).enumerate() {
                if let Some((state, cities)) = at(offset) {
                    empire[k] = Some(*state);
                    city[k] = cities.iter().find(|(id, _)| *id == record.city).map(|(_, c)| *c);
                }
            }
            Row { game: spec.seed, record, city, empire }
        })
        .collect();
    Ok(Probed {
        rows,
        end,
        rounds: game.view().auth.turn().round,
        hash: kernel::replay::hash::authoritative_hash(&game),
        seconds: started.elapsed().as_secs_f64(),
        snapshot_seconds,
    })
}

/// The names of a kind's feature columns.
pub fn feature_names(kind: Kind) -> Vec<&'static str> {
    match kind {
        Kind::Production => EMPIRE_NAMES.iter().chain(&CITY_NAMES).chain(&PRODUCTION_NAMES).copied().collect(),
        Kind::Research => EMPIRE_NAMES.iter().chain(&RESEARCH_NAMES).copied().collect(),
    }
}

/// The CSV header of a kind's file: one line a candidate of a set.
pub fn header(kind: Kind) -> String {
    let mut columns: Vec<String> = ["set", "game", "player", "round", "city", "explored", "chosen", "rank", "taken", "item"].map(str::to_owned).to_vec();
    columns.extend(feature_names(kind).into_iter().map(str::to_owned));
    for offset in [0].into_iter().chain(HORIZONS) {
        columns.extend(CITY_STATE_NAMES.iter().map(|n| format!("city_{n}_{offset}")));
        columns.extend(EMPIRE_STATE_NAMES.iter().map(|n| format!("empire_{n}_{offset}")));
    }
    columns.extend(["winner", "victory", "victory_round"].map(str::to_owned));
    columns.join(",")
}

/// The lines of one set: every candidate of an explored set, only the
/// taken one of another (its other candidates have no outcome and the net
/// is not trained on them); `set` numbers the set in the file.
pub fn lines(row: &Row, set: u64, end: &End, out: &mut dyn Write) -> io::Result<()> {
    let r = &row.record;
    let mut labels = String::new();
    for k in 0..3 {
        for value in row.city[k].map(|c| c.values().map(Some)).unwrap_or([None; 6]).into_iter().chain(row.empire[k].map(|e| e.values().map(Some)).unwrap_or([None; 6])) {
            labels.push(',');
            if let Some(value) = value {
                labels.push_str(&value.to_string());
            }
        }
    }
    let winner = end.winner.map_or(String::new(), |w| w.to_string());
    let victory = end.victory.unwrap_or("");
    let chosen = r.chosen.map_or(-1, |c| c as i64);
    let city = if r.kind == Kind::Research { -1 } else { r.city as i64 };
    for (rank, (item, features)) in r.items.iter().zip(&r.rows).enumerate() {
        let taken = r.chosen == Some(rank);
        if !r.explored && !taken {
            continue;
        }
        let mut line = format!("{set},{},{},{},{city},{},{chosen},{rank},{},{item}", row.game, r.player, r.round, u8::from(r.explored), u8::from(taken));
        for value in features {
            line.push(',');
            line.push_str(&value.to_string());
        }
        writeln!(out, "{line}{labels},{winner},{victory},{}", end.round)?;
    }
    Ok(())
}
