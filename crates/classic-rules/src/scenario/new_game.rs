//! A classic new game: the kernel's map and starts (`kernel::scenario`),
//! then the city-states, the ruins, the barbarian player and the victory
//! options.

use kernel::scenario::generator::{assign_civilizations, pick_starts, start_player};

use crate::ids::PlayerId;
use crate::rules::CompiledRuleset;
use crate::scenario::{MapScript, ScenarioConfig};
use kernel::scenario::generator::generate_scripted_builder;
use crate::world::state::GameState;
use crate::world::player::{ClassicKind, ClassicPlayers};

/// Options of a new game beyond the map (setup screen).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NewGameOptions {
    pub barbarians: crate::world::barbarians::BarbarianMode,
    /// City-states placed on the map (16 on a standard map; own value, unverified).
    pub city_states: u8,
    /// Victories on and the last round (saved with the game).
    pub victories: crate::world::victory::VictoryOptions,
    /// The shape of the land.
    pub script: MapScript,
}

/// A new game with options: `cfg.players` civilizations (the first
/// one `human_civ` when given), then the city-states, then the barbarian
/// player when barbarians are on.
pub fn generate_new_game_with(
    rules: &CompiledRuleset,
    cfg: &ScenarioConfig,
    human_civ: Option<u8>,
    options: NewGameOptions,
) -> GameState {
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
    let mut game = generate_scripted_builder(&empty, rules, options.script);
    let civs = assign_civilizations(rules, cfg.players, human_civ, cfg.seed);
    let starts = pick_starts(game.state(), rules, &civs, cfg.players);
    kernel::scenario::sea_resources::support_starts(&mut game, &starts, cfg.seed);
    for (p, &tile) in starts.iter().enumerate() {
        let player = PlayerId(p as u8);
        game.set_player_civ(player, civs[p]);
        start_player(&mut game, player, civs[p], tile);
    }
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
    game.auth_mut().module.victory.options = options.victories;
    game.build()
}

