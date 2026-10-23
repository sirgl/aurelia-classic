//! A city as its owner's city screen shows it, for the observer's read-only cards: the
//! screen's own view model (`CityView`, built by `Session::city_view`) over a copy of the
//! game, seen through the owner's eyes. Nothing here changes the observed game.
use std::sync::Arc;

use client::selection::Selection;
use classic_rules::ids::TileId;
use classic_rules::{CompiledRuleset, GameState};

use super::session::Session;
use crate::ui::model::CityView;

/// The city screen's data of one city, with what the screen's header says around it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedCity {
    pub view: CityView,
    /// The owner's civilization.
    pub civilization: String,
    pub owner: u8,
    pub capital: bool,
}

impl Session {
    /// The city on `tile` of a major civilization, as its owner's city screen shows it.
    /// `game` is a copy: the session over it is thrown away with the result.
    pub fn observed_city(game: GameState, rules: Arc<CompiledRuleset>, tile: TileId) -> Option<ObservedCity> {
        let (handle, owner) = {
            let auth = game.view().auth;
            let handle = auth.indexes().city_on_tile(tile)?;
            let owner = auth.cities().get(handle)?.owner;
            if !auth.players().is_major(owner) {
                return None;
            }
            (handle, owner)
        };
        let difficulty = match game.view().auth.settings().difficulty.as_str() {
            "" => rules.default_difficulty().to_owned(),
            saved => saved.to_owned(),
        };
        let mut session = Self::starting(game, rules, &difficulty, Some(owner));
        session.client.selection = Selection::City(handle);
        let view = session.city_view()?;
        let core = session.game().view().auth.cities().get(handle)?;
        Some(ObservedCity {
            view,
            civilization: session.civilization_name(owner),
            owner: owner.0,
            capital: session.is_capital(core),
        })
    }
}
