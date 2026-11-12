//! The check of the model of the rivals (`rival-net`, `classic_rules::ai::rival_model`): at every
//! `EVERY_ENV` rounds, at the start of each nation's turn, the seat's estimate of every rival it
//! has met (from its observation and the book its strategy net keeps in its memory, so the very
//! numbers its net reads this turn) beside the rival's true numbers from the world. One CSV line
//! per seat and rival (in a duel not yet met: the one other nation, `unmet` 1, only its rows and
//! technologies, which are all the seat can tell of it); on when `AURELIA_RIVAL_NET` names a file (lines are appended, the header
//! when the file is new). Reads only: the result hash of a match does not move.

use std::io::Write;
use std::sync::{Mutex, OnceLock};

use classic_rules::ai::rival_model::{estimate, others_mean, MEASURES};
use classic_rules::ai::strategy_net::features::era_of;
use classic_rules::ai::strategy_net::Memory;
use classic_rules::derived::standing::{demographics, Measure};
use classic_rules::knowledge::observation::AIObservation;
use kernel::ai::state::AiStates;
use kernel::ids::PlayerId;
use kernel::rules::CompiledRuleset;
use kernel::world::state::GameState;

use crate::bench::modules::{ClassicBench, MatchRecorder};

/// The environment variable that turns the check on (the CSV file).
pub const ENV: &str = "AURELIA_RIVAL_NET";
/// Rounds between two checks (default `EVERY`).
pub const EVERY_ENV: &str = "AURELIA_RIVAL_NET_EVERY";
pub const EVERY: u32 = 25;

type Classic = classic_rules::classic::Classic;

fn sink() -> Option<&'static Mutex<std::fs::File>> {
    static SINK: OnceLock<Option<Mutex<std::fs::File>>> = OnceLock::new();
    SINK.get_or_init(|| {
        let path = std::env::var(ENV).ok().filter(|p| !p.is_empty())?;
        let new = std::fs::metadata(&path).map_or(true, |m| m.len() == 0);
        let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path).ok()?;
        if new {
            let _ = writeln!(file, "{}", header());
        }
        Some(Mutex::new(file))
    })
    .as_ref()
}

/// Whether the check is on.
pub fn enabled() -> bool {
    sink().is_some()
}

const ROWS: [&str; MEASURES] = ["pop", "crop", "goods", "gnp", "lit", "land", "soldiers"];

fn header() -> String {
    let mut cols: Vec<String> = ["game", "seed", "round", "seat", "rival", "living", "met", "seat_era", "rival_era"].iter().map(|s| s.to_string()).collect();
    for r in ROWS {
        for c in ["e", "lo", "hi", "mean", "t", "own"] {
            cols.push(format!("{r}_{c}"));
        }
    }
    for c in ["techs_e", "techs_lo", "techs_hi", "techs_t", "techs_own", "sci_e", "sci_t", "sci_own", "score_e", "score_t", "trend_e", "trend_own", "cap_e", "cap_t", "cap_all", "del_e", "del_t", "del_need", "cul_e", "cul_t", "cul_need", "unmet"] {
        cols.push(c.to_owned());
    }
    cols.join(",")
}

pub struct RivalNetRecorder {
    name: String,
    seed: u64,
    every: u32,
    lines: Vec<String>,
}

impl RivalNetRecorder {
    pub fn new(name: &str, seed: u64) -> Option<Self> {
        enabled().then(|| Self {
            name: name.to_owned(),
            seed,
            every: std::env::var(EVERY_ENV).ok().and_then(|v| v.parse().ok()).filter(|v| *v > 0).unwrap_or(EVERY),
            lines: Vec::new(),
        })
    }
}

fn opt<T: ToString>(v: Option<T>) -> String {
    v.map_or(String::new(), |v| v.to_string())
}

impl MatchRecorder<ClassicBench> for RivalNetRecorder {
    fn round_start(&mut self, _game: &GameState<Classic>, _rules: &CompiledRuleset<Classic>) {}

    fn turn_start(&mut self, game: &GameState<Classic>, rules: &CompiledRuleset<Classic>, player: PlayerId, states: &AiStates) {
        let view = game.view();
        let auth = view.auth;
        let round = auth.turn().round;
        if round == 0 || round % self.every != 0 || !auth.players().is_major(player) || !auth.players().is_alive(player) {
            return;
        }
        let (obs, _) = AIObservation::for_player(auth, view.derived, rules, player);
        let book = states.get(player).map(|s| Memory::decode(&s.mind_memory).rivals).unwrap_or_default();
        let model = estimate(&obs, rules, &book);
        let seat_era = era_of(rules, &obs.economy.techs);
        let congress = obs.congress.is_some();
        let needed = classic_rules::derived::tourism::nations_needed_for_cultural_victory(auth);
        for r in &model.rivals {
            let p = r.player;
            let truth = demographics(auth, rules, view.derived, p);
            let mine = &obs.public.demographics;
            let mut cols = vec![
                self.name.clone(),
                self.seed.to_string(),
                round.to_string(),
                player.0.to_string(),
                p.0.to_string(),
                model.living.to_string(),
                model.rivals.len().to_string(),
                seat_era.to_string(),
                era_of(rules, auth.players().techs_of(p)).to_string(),
            ];
            for (m, measure) in Measure::ALL.iter().enumerate() {
                let g = r.rows[m];
                let t = truth.iter().find(|d| d.measure == *measure).map_or(0, |d| d.value);
                let mean = mine.iter().find(|d| d.measure == *measure).and_then(|d| others_mean(d, model.living));
                cols.extend([g.value.to_string(), g.lo.to_string(), g.hi.to_string(), opt(mean), t.to_string(), model.own[m].to_string()]);
            }
            let income = classic_rules::derived::income::player_income(auth, rules, p);
            let (_, held) = classic_rules::world::victory::capitals_held(auth, p);
            cols.extend([
                r.techs.value.to_string(),
                r.techs.lo.to_string(),
                r.techs.hi.to_string(),
                auth.players().techs_of(p).len().to_string(),
                model.own_techs.to_string(),
                opt(r.science),
                income.science.to_string(),
                model.own_science.to_string(),
                r.score.to_string(),
                classic_rules::world::victory::score(auth, rules, p).to_string(),
                opt(r.score_trend),
                opt(model.own_trend),
                r.capitals.to_string(),
                held.to_string(),
                model.capitals_all.to_string(),
                opt(r.delegates),
                if congress { classic_rules::world::congress::delegates(auth, rules, p).to_string() } else { String::new() },
                opt(model.delegates_needed),
                r.influence_on_us.to_string(),
                classic_rules::derived::tourism::influential_on(auth, p).to_string(),
                needed.to_string(),
                "0".to_owned(),
            ]);
            self.lines.push(cols.join(","));
        }
        // A duel before the meeting: the one other nation, by its rows.
        if let Some(u) = &model.unmet {
            let others: Vec<PlayerId> = (0..auth.players().count() as u8).map(PlayerId).filter(|p| *p != player && auth.players().is_major(*p) && auth.players().is_alive(*p)).collect();
            if let [p] = others[..] {
                let truth = demographics(auth, rules, view.derived, p);
                let mine = &obs.public.demographics;
                let mut cols = vec![
                    self.name.clone(),
                    self.seed.to_string(),
                    round.to_string(),
                    player.0.to_string(),
                    p.0.to_string(),
                    model.living.to_string(),
                    "0".to_owned(),
                    seat_era.to_string(),
                    era_of(rules, auth.players().techs_of(p)).to_string(),
                ];
                for (m, measure) in Measure::ALL.iter().enumerate() {
                    let g = u.rows[m];
                    let t = truth.iter().find(|d| d.measure == *measure).map_or(0, |d| d.value);
                    let mean = mine.iter().find(|d| d.measure == *measure).and_then(|d| others_mean(d, model.living));
                    cols.extend([g.value.to_string(), g.lo.to_string(), g.hi.to_string(), opt(mean), t.to_string(), model.own[m].to_string()]);
                }
                cols.extend([
                    u.techs.value.to_string(),
                    u.techs.lo.to_string(),
                    u.techs.hi.to_string(),
                    auth.players().techs_of(p).len().to_string(),
                    model.own_techs.to_string(),
                ]);
                // Science, score, trend, capitals, delegates, culture: nothing is read of it.
                cols.extend(std::iter::repeat_n(String::new(), 16));
                cols.push("1".to_owned());
                self.lines.push(cols.join(","));
            }
        }
    }

    fn finish(self: Box<Self>, _game: &GameState<Classic>, _rules: &CompiledRuleset<Classic>, _minds: &[&classic_rules::ai::driver::ClassicMind]) {
        if let Some(sink) = sink() {
            if let Ok(mut file) = sink.lock() {
                let mut text = self.lines.join("\n");
                if !text.is_empty() {
                    text.push('\n');
                    let _ = file.write_all(text.as_bytes());
                }
            }
        }
    }
}
