//! Happiness at the start of a player's resolution: the live value of
//! `derived::happiness` becomes the snapshot the turn's effects read
//! (`PlayerEconomy::excess_happiness`). Journal: PlayerChange::
//! HappinessChanged when the production and gold penalty of the player's
//! cities changes (their stored yields depend on it).

use crate::classic::ClassicChange;
use crate::derived::happiness::{player_happiness, yield_penalty_percent};
use crate::ids::PlayerId;
use crate::rules::CompiledRuleset;
use crate::transaction::transaction::WorldTransaction;

impl<'a> WorldTransaction<'a> {
    pub(crate) fn update_happiness(&mut self, player: PlayerId, rules: &CompiledRuleset) {
        let excess = player_happiness(self.world, rules, player).excess();
        let Some(economy) = self.world.players.economy_mut(player) else {
            return;
        };
        let before = yield_penalty_percent(economy.excess_happiness);
        economy.excess_happiness = excess;
        if yield_penalty_percent(excess) != before {
            self.journal
                .module_changes
                .push(ClassicChange::HappinessChanged { player });
        }
    }
}
