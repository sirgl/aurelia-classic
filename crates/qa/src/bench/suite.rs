//! The suites: fixed maps, seeds and round limits, and the matches they
//! expand to. Head to head, every map and seed is played twice: the
//! candidate on the even seats among the nations, then on the odd ones, so
//! both versions play every start and every place in the turn order
//! equally often.

use serde::{Deserialize, Serialize};

/// One map of a suite, by the module's names: a map script and a world
/// size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapSpec {
    pub script: &'static str,
    pub size: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suite {
    pub name: &'static str,
    pub maps: Vec<MapSpec>,
    pub seeds: Vec<u64>,
    /// The last round: the round of the time victory.
    pub rounds: u32,
    /// Rounds the players' standings are taken at (the end is always taken).
    pub checkpoints: Vec<u32>,
    /// The game speed's key (empty: Standard; `Suite::at_speed`).
    pub speed: String,
}

/// Seeds of an independent suite of the same shape are `SEED_STRIDE`
/// apart (`--seed-offset`).
pub const SEED_STRIDE: u64 = 1000;

const CLASSIC_SCRIPTS: [&str; 5] = ["continents", "pangaea", "fractal", "archipelago", "small-continents"];

impl Suite {
    pub const NAMES: [&'static str; 5] = ["smoke", "standard", "duel", "duel-land", "pace"];

    pub fn named(name: &str) -> Option<Suite> {
        match name {
            "smoke" => Some(Suite {
                name: "smoke",
                maps: vec![MapSpec { script: "pangaea", size: "tiny" }],
                seeds: vec![1],
                rounds: 70,
                checkpoints: vec![35],
                speed: String::new(),
            }),
            "standard" => Some(Suite {
                name: "standard",
                maps: ["tiny", "small"]
                    .into_iter()
                    .flat_map(|size| CLASSIC_SCRIPTS.into_iter().map(move |script| MapSpec { script, size }))
                    .collect(),
                seeds: (1..=4).collect(),
                rounds: 300,
                checkpoints: vec![100, 200],
                speed: String::new(),
            }),
            // Two nations: one capital to take, the domination victory in
            // reach (docs/ai-leaders.md).
            "duel" => Some(Suite {
                name: "duel",
                maps: CLASSIC_SCRIPTS.into_iter().map(|script| MapSpec { script, size: "duel" }).collect(),
                seeds: (1..=8).collect(),
                rounds: 300,
                checkpoints: vec![100, 200],
                speed: String::new(),
            }),
            // The duel on one landmass: the pangaea script, whose two capitals
            // share their land (the book rushes, classic/docs/ai-books.md: on the
            // other scripts the rival is mostly over the sea); as many games as
            // `duel`.
            "duel-land" => Some(Suite {
                name: "duel-land",
                maps: vec![MapSpec { script: "pangaea", size: "duel" }],
                seeds: (1..=40).collect(),
                rounds: 300,
                checkpoints: vec![100, 200],
                speed: String::new(),
            }),
            // The classic pacing against public references: a whole
            // Standard-speed game (its time victory at round 500) on Small
            // maps (six nations), twenty seeds — twenty seed blocks in solo
            // mode (docs/ai-bench.md, «Темп»).
            "pace" => Some(Suite {
                name: "pace",
                maps: CLASSIC_SCRIPTS.into_iter().map(|script| MapSpec { script, size: "small" }).collect(),
                seeds: (1..=20).collect(),
                rounds: 500,
                checkpoints: vec![100, 200, 300, 400],
                speed: String::new(),
            }),
            _ => None,
        }
    }

    /// The suite at a game speed: its key (empty for Standard), its last
    /// round and checkpoints at the same share of the speed's game
    /// (`SpeedValues::game_round`: 300 rounds at Standard are 198 at Quick).
    pub fn at_speed(mut self, key: &str, speed: &kernel::rules::speeds::SpeedValues) -> Self {
        self.speed = if speed.is_standard() { String::new() } else { key.to_owned() };
        self.rounds = speed.game_round(self.rounds);
        for c in &mut self.checkpoints {
            *c = speed.game_round(*c);
        }
        self
    }

    /// The matches of the suite, its seeds moved by `offset` strides.
    pub fn matches(&self, mode: Mode, offset: u64) -> Vec<Match> {
        let seatings: &[Seating] = match mode {
            Mode::Versus => &[Seating::Even, Seating::Odd],
            Mode::Solo => &[Seating::All],
        };
        let mut matches = Vec::new();
        for map in &self.maps {
            for seed in &self.seeds {
                for seating in seatings {
                    matches.push(Match {
                        map: *map,
                        seed: seed + offset * SEED_STRIDE,
                        seating: *seating,
                    });
                }
            }
        }
        matches
    }
}

/// Head to head (a candidate against a baseline) or every seat one variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Versus,
    Solo,
}

/// Which version a seat plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Candidate,
    Baseline,
}

/// Which nations' seats the candidate takes, by their index among the
/// nations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seating {
    Even,
    Odd,
    All,
}

impl Seating {
    pub fn side(self, index: usize) -> Side {
        let candidate = match self {
            Seating::Even => index % 2 == 0,
            Seating::Odd => index % 2 == 1,
            Seating::All => true,
        };
        if candidate {
            Side::Candidate
        } else {
            Side::Baseline
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Seating::Even => "even",
            Seating::Odd => "odd",
            Seating::All => "all",
        }
    }
}

/// One game of a tournament.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Match {
    pub map: MapSpec,
    pub seed: u64,
    pub seating: Seating,
}

impl Match {
    pub fn label(&self) -> String {
        format!("{}/{}/seed {}/{}", self.map.script, self.map.size, self.seed, self.seating.name())
    }
}
