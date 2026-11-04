//! What game to play: the options of the desktop setup screen for an
//! AI-only game.

use classic_rules::scenario::MapScript;
use classic_rules::scenario::{NewGameOptions, ScenarioConfig};
use classic_rules::world::barbarians::BarbarianMode;
use classic_rules::world::victory::{VictoryKind, VictoryOptions, STANDARD_MAX_ROUNDS};

pub use classic_rules::world::size::WorldSize;

/// Medium sea level of the setup screen (the share Fractal keeps).
pub const MEDIUM_SEA_LEVEL: u8 = 25;

/// The classic world sizes: default nations of a size.
pub fn nations(size: WorldSize) -> u8 {
    match size {
        WorldSize::Duel => 2,
        WorldSize::Tiny => 4,
        WorldSize::Small => 6,
        WorldSize::Standard => 8,
        WorldSize::Large => 10,
        WorldSize::Huge => 12,
    }
}

/// The classic world sizes: default city-states of a size.
pub fn city_states(size: WorldSize) -> u8 {
    match size {
        WorldSize::Duel => 4,
        WorldSize::Tiny => 8,
        WorldSize::Small => 12,
        WorldSize::Standard => 16,
        WorldSize::Large => 20,
        WorldSize::Huge => 24,
    }
}

pub fn size_name(size: WorldSize) -> &'static str {
    match size {
        WorldSize::Duel => "duel",
        WorldSize::Tiny => "tiny",
        WorldSize::Small => "small",
        WorldSize::Standard => "standard",
        WorldSize::Large => "large",
        WorldSize::Huge => "huge",
    }
}

pub fn size_from_name(name: &str) -> Option<WorldSize> {
    classic_rules::world::size::WORLD_SIZES
        .into_iter()
        .find(|s| size_name(*s) == name)
}

pub fn victory_name(kind: VictoryKind) -> &'static str {
    match kind {
        VictoryKind::Time => "time",
        VictoryKind::Science => "science",
        VictoryKind::Domination => "domination",
        VictoryKind::Cultural => "culture",
        VictoryKind::Diplomatic => "diplomatic",
    }
}

pub fn victory_from_name(name: &str) -> Option<VictoryKind> {
    VictoryKind::ALL.into_iter().find(|k| victory_name(*k) == name)
}

/// One AI-only game.
#[derive(Debug, Clone)]
pub struct GameSpec {
    pub seed: u64,
    pub script: MapScript,
    pub size: WorldSize,
    /// Handicap key, `core:handicap/<level>`.
    pub difficulty: String,
    pub nations: u8,
    pub city_states: u8,
    pub barbarians: BarbarianMode,
    pub victories: VictoryOptions,
    /// The game speed: a speed key (`core:speed/quick`) or name; empty:
    /// Standard. A Standard last round (500) becomes the speed's.
    pub speed: String,
}

impl GameSpec {
    /// The setup screen's defaults for a size: its nations and
    /// city-states, normal barbarians, every victory, 500 rounds, Prince.
    pub fn new(script: MapScript, size: WorldSize, seed: u64) -> Self {
        Self {
            seed,
            script,
            size,
            difficulty: "core:handicap/prince".to_owned(),
            nations: nations(size),
            city_states: city_states(size),
            barbarians: BarbarianMode::Normal,
            victories: VictoryOptions {
                disabled: 0,
                max_rounds: STANDARD_MAX_ROUNDS,
            },
            speed: String::new(),
        }
    }

    /// The game at the speed `speed` (a key or a name; empty: Standard).
    pub fn speed(mut self, speed: &str) -> Self {
        self.speed = speed.to_owned();
        self
    }

    pub fn difficulty(mut self, level: &str) -> Self {
        self.difficulty = format!("core:handicap/{level}");
        self
    }

    /// Only `kind` and the time victory stay on (time ends the game).
    pub fn only_victory(mut self, kind: VictoryKind) -> Self {
        let mut options = self.victories;
        for other in VictoryKind::ALL {
            options = options.with(other, other == kind || other == VictoryKind::Time);
        }
        self.victories = options;
        self
    }

    pub fn max_rounds(mut self, rounds: u32) -> Self {
        self.victories.max_rounds = rounds;
        self
    }

    pub fn label(&self) -> String {
        let level = self.difficulty.rsplit('/').next().unwrap_or("?");
        let speed = match self.speed.rsplit('/').next() {
            Some(s) if !s.is_empty() && !s.eq_ignore_ascii_case("standard") => format!("/{s}"),
            _ => String::new(),
        };
        format!(
            "{}/{}/{}{speed}/seed {}",
            self.script.key(),
            size_name(self.size),
            level,
            self.seed
        )
    }

    /// The map and the new-game options (the speed looked up in `rules`; an
    /// unknown speed plays Standard).
    pub fn scenario(&self, rules: &classic_rules::CompiledRuleset) -> (ScenarioConfig, NewGameOptions) {
        let (width, height) = self.size.grid();
        let config = ScenarioConfig {
            seed: self.seed,
            players: self.nations,
            width,
            height,
            cities: 0,
            units: 0,
            water_ratio: self.script.water_ratio(MEDIUM_SEA_LEVEL),
        };
        let options = NewGameOptions {
            barbarians: self.barbarians,
            city_states: self.city_states,
            victories: self.victories,
            script: self.script,
            speed: None,
            ..NewGameOptions::default()
        };
        let options = if self.speed.is_empty() { options } else { options.at_speed(rules, &self.speed).unwrap_or(options) };
        (config, options)
    }
}
