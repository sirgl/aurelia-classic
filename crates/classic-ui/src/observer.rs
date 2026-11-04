//! The classic module in the observer mode (the shell's `observer`): the nations'
//! names and the figures of their panels' headers. The match itself, the views through
//! every nation's eyes and the clock are the platform's.
use classic_rules::classic::Classic;
use classic_rules::ids::{PlayerId, TileId};
use classic_rules::world::victory::{score, VictoryKind};
use classic_rules::{ClassicGame, CompiledRuleset, GameState};
use client::shell::observer::{study, Books, Card, Describe, ObserverMatch, Power, Round, Stat};
use render::unit_models::UnitModelBook;
use render::world::state::RenderWorld;
use render::world_models::WorldBook;

use i18n::{t, tf};

use crate::observer_cards;

/// A match of the classic module played by the AI alone.
pub type ClassicMatch = ObserverMatch<Classic, ClassicPowers>;

/// How the nations show in the panels' headers.
#[derive(Debug, Clone, Copy, Default)]
pub struct ClassicPowers;

impl Describe<Classic> for ClassicPowers {
    fn name(&self, game: &GameState, rules: &CompiledRuleset, player: PlayerId) -> String {
        crate::game::names::player(game, rules, player)
    }

    /// The score, the treasury, the cities, the technologies and the culture.
    fn standings(&self, game: &GameState, rules: &CompiledRuleset, players: &[PlayerId]) -> Vec<Vec<Stat>> {
        let auth = game.view().auth;
        players
            .iter()
            .map(|&player| {
                vec![
                    Stat::new(t!("classic.observer.stat-score"), score(auth, rules, player)),
                    Stat::new(t!("classic.observer.stat-gold"), auth.players().treasury_of(player)),
                    Stat::new(t!("classic.observer.stat-cities"), auth.indexes().cities_of(player).len()),
                    Stat::new(t!("classic.observer.stat-techs"), auth.players().techs_of(player).len()),
                    Stat::new(t!("classic.observer.stat-culture"), auth.module().culture().of(player).culture),
                ]
            })
            .collect()
    }

    /// The turn as the game shows it (counted from 1), with no limit: any victory can end
    /// the match earlier, and the time victory's own turn (the game's victory screen says
    /// 501 for 500 rounds) would read as a mistake in the bar.
    fn round(&self, game: &GameState, _: &CompiledRuleset) -> Round {
        Round { now: game.view().auth.turn().round + 1, cap: None }
    }

    /// The research and its progress, and the gold a turn brings.
    fn details(&self, game: &GameState, rules: &CompiledRuleset, player: PlayerId) -> Vec<Stat> {
        let research = match study(game, rules, player) {
            Some(now) => Stat::new(t!("classic.observer.stat-research"), now.name).with_percent(now.percent),
            None => Stat::new(t!("classic.observer.stat-research"), "—"),
        };
        let gold = classic_rules::derived::income::player_income(game.view().auth, rules, player).gold;
        vec![research, Stat::new(t!("classic.observer.stat-gold-per-turn"), format!("{gold:+}"))]
    }

    fn cause(&self, game: &GameState, _: &CompiledRuleset, _: &str) -> String {
        let kind = game.outcome().map(|outcome| outcome.kind);
        match kind {
            Some(VictoryKind::Time) => t!("classic.game.victory.kind-time"),
            Some(VictoryKind::Science) => t!("classic.game.victory.kind-science"),
            Some(VictoryKind::Domination) => t!("classic.game.victory.kind-domination"),
            Some(VictoryKind::Cultural) => t!("classic.game.victory.kind-cultural"),
            Some(VictoryKind::Diplomatic) => t!("classic.game.victory.kind-diplomatic"),
            None => t!("classic.game.victory.title-victory"),
        }
        .to_owned()
    }

    fn books(&self, rules: &CompiledRuleset) -> Books {
        Books { units: UnitModelBook::from_rules(rules), world: WorldBook::from_rules(rules) }
    }

    fn summary(&self, _: &GameState, _: &CompiledRuleset, powers: &[Power]) -> String {
        tf!("classic.observer.summary", n = powers.len())
    }

    /// The ordinary game's tooltip of the tile, from the viewer's own view.
    fn tile_card(&self, game: &GameState, rules: &CompiledRuleset, world: &RenderWorld, _: PlayerId, tile: TileId) -> Option<Card> {
        observer_cards::tile_card(game, world, tile, |player| self.name(game, rules, player))
    }

    /// The ordinary city screen's facts of the city, with no orders.
    fn city_card(&self, game: &GameState, rules: &CompiledRuleset, world: &RenderWorld, viewer: PlayerId, tile: TileId, full: bool) -> Option<Card> {
        observer_cards::city_card(game, rules, world, viewer, tile, full, |player| self.name(game, rules, player))
    }
}
