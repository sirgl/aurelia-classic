//! A classic new game: the kernel's map and starts (`kernel::scenario`),
//! then the city-states, the ruins, the barbarian player and the victory
//! options.

use kernel::scenario::generator::{assign_nations, pick_starts_with, start_player};
use kernel::scenario::start_balance::{balance_starts, BalanceReport, StartBalance};

use crate::ids::PlayerId;
use crate::rules::CompiledRuleset;
use crate::scenario::{MapScript, ScenarioConfig};
use kernel::scenario::generator::generate_scripted_builder_with;
use kernel::scenario::LandOptions;
use crate::world::state::GameState;
use crate::world::player::{ClassicKind, ClassicPlayers};

/// Options of a new game beyond the map (setup screen).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NewGameOptions {
    pub barbarians: crate::world::barbarians::BarbarianMode,
    /// City-states placed on the map (16 on a standard map; own value, unverified).
    pub city_states: u8,
    /// Victories on and the last round (saved with the game).
    pub victories: crate::world::victory::VictoryOptions,
    /// The shape of the land.
    pub script: MapScript,
    /// The game speed: a position in `rules.speeds` (None: Standard). A game
    /// of another speed whose `victories` keep the Standard last round
    /// (`STANDARD_MAX_ROUNDS`) plays to the speed's last round.
    pub speed: Option<u8>,
    /// The balance of the starts (`kernel::scenario::start_balance`), on by default: fair starts (a
    /// minimum of food and production, starts close in value) and "Strategic balance" (every start has
    /// the early strategic resources, horses and iron in Classic, within three tiles, in a small fixed
    /// amount). The setup screen's switch is `start_balance.strategic`; `StartBalance::OFF` gives the
    /// map of the generator before the balance existed, bit for bit.
    pub start_balance: StartBalance,
}

impl Default for NewGameOptions {
    fn default() -> Self {
        Self {
            barbarians: Default::default(),
            city_states: 0,
            victories: Default::default(),
            script: Default::default(),
            speed: None,
            start_balance: StartBalance::ON,
        }
    }
}

impl NewGameOptions {
    /// The options at the speed named `name` (a key, a name or the key's last
    /// part: `quick`), None for an unknown speed.
    pub fn at_speed(self, rules: &CompiledRuleset, name: &str) -> Option<Self> {
        let index = rules.speeds.iter().position(|s| rules.speed_named(name).is_some_and(|n| n.key == s.key))?;
        Some(Self { speed: Some(index as u8), ..self })
    }

    /// The numbers of the chosen speed (Standard when none).
    pub fn speed_values(&self, rules: &CompiledRuleset) -> kernel::rules::speeds::SpeedValues {
        self.speed
            .and_then(|i| rules.speeds.get(i as usize))
            .map_or(kernel::rules::speeds::SpeedValues::STANDARD, |s| s.values)
    }
}

/// A new game with options: `cfg.players` nations (the first
/// one `human_nation` when given), then the city-states, then the barbarian
/// player when barbarians are on.
pub fn generate_new_game_with(
    rules: &CompiledRuleset,
    cfg: &ScenarioConfig,
    human_nation: Option<u8>,
    options: NewGameOptions,
) -> GameState {
    generate_new_game_on(rules, cfg, human_nation, options, LandOptions::CLASSIC)
}

/// `generate_new_game_with` on a land of `land`'s shares (mountains, hills,
/// forest, rivers, resources; `LandOptions::CLASSIC` is
/// `generate_new_game_with`, bit for bit).
pub fn generate_new_game_on(
    rules: &CompiledRuleset,
    cfg: &ScenarioConfig,
    human_nation: Option<u8>,
    options: NewGameOptions,
    land: LandOptions,
) -> GameState {
    generate_new_game_report(rules, cfg, human_nation, options, land).0
}

/// `generate_new_game_on`, with what the start balance changed on the map (the census and the tests count it).
pub fn generate_new_game_report(
    rules: &CompiledRuleset,
    cfg: &ScenarioConfig,
    human_nation: Option<u8>,
    options: NewGameOptions,
    land: LandOptions,
) -> (GameState, BalanceReport) {
    use crate::world::barbarians::BarbarianMode;
    let barbarians = options.barbarians != BarbarianMode::None;
    let minors = options.city_states.min(rules.module.city_states.len().min(u8::MAX as usize) as u8);
    let total = cfg.players + minors + u8::from(barbarians);
    let empty = ScenarioConfig {
        players: total,
        cities: 0,
        units: 0,
        ..cfg.clone()
    };
    let mut game = generate_scripted_builder_with(&empty, rules, options.script, land);
    let nations = assign_nations(rules, cfg.players, human_nation, cfg.seed);
    let balance = options.start_balance;
    let starts = pick_starts_with(game.state(), rules, &nations, cfg.players, &balance);
    kernel::scenario::sea_resources::support_starts(&mut game, &starts, cfg.seed);
    for (p, &tile) in starts.iter().enumerate() {
        let player = PlayerId(p as u8);
        game.set_player_nation(player, nations[p]);
        start_player(&mut game, player, nations[p], tile);
    }
    let report = balance_starts(&mut game, &starts, cfg.seed, &balance);
    super::city_states::place_city_states(&mut game, cfg.players, minors, cfg.seed);
    super::ruins::place_ruins(&mut game, cfg.seed);
    if barbarians {
        let barbarian = PlayerId(total - 1);
        game.set_player_kind(barbarian, crate::world::player::PlayerKind::Module(ClassicKind::Barbarian));
        let auth = game.auth_mut();
        auth.module.barbarians.mode = options.barbarians;
        if let Some(barbarian) = auth.players.barbarian() {
            auth.players.set_alive(barbarian, options.barbarians != BarbarianMode::None);
        }
    }
    if let Some(speed) = options.speed.and_then(|i| rules.speeds.get(i as usize)) {
        game.set_speed(&speed.key);
    }
    let mut victories = options.victories;
    if victories.max_rounds == crate::world::victory::STANDARD_MAX_ROUNDS {
        victories.max_rounds = options.speed_values(rules).max_rounds();
    }
    game.auth_mut().module.victory.options = victories;
    (game.build(), report)
}

