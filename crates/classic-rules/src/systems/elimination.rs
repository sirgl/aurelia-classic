//! Elimination (classic/docs/classic/rules.md, Elimination): the rules forget an eliminated
//! nation at once. The kernel calls `forget_eliminated` in the transaction of the death
//! (`ModuleSystems::player_eliminated`, and once more for every dead player of a loaded save);
//! one cleaner per owner of the state ends every tie it keeps with the dead nation, so no reader
//! has to ask whether a player still lives. Unciv: `Civilization.destroy` clears the trades and
//! the trade requests and removes the spies; the city-state ally (`updateAllyCivForCityState`)
//! and the other readers skip a defeated civ.
//!
//! Not forgotten:
//! - the barbarians: they leave the game only while barbarians are off and come back, still at
//!   war with everyone, when they are on;
//! - a player that never had a city. The harness builds the players of a scene in any order, and
//!   an empty player "dies" at every commit of the setup and lives again with its first unit, so
//!   a war declared in the setup must survive it. In a game such a nation is one that lost its
//!   first units before it founded a city; the only tie it can hold is a war.
//!
//! The guard: `world::references` lists what still names a player; `tests/elimination.rs` asks
//! for an empty list after a death. A new tie between players joins a cleaner here and that list.

use crate::ids::PlayerId;
use crate::rules::CompiledRuleset;
use crate::transaction::transaction::WorldTransaction;
use crate::world::player::ClassicPlayers;

impl<'a> WorldTransaction<'a> {
    /// `dead` was eliminated: every tie the rules keep with it ends. The city-states go before
    /// the league (an ally's delegates move with the alliance).
    pub(crate) fn forget_eliminated(&mut self, dead: PlayerId, rules: &CompiledRuleset) {
        // As the kernel's elimination counts it: the module's memory, or a city it founded.
        let had_city = self.world.module.culture.of(dead).max_cities > 0
            || self.world.cities.iter().any(|(_, city)| city.original_owner == dead);
        if self.world.players.is_barbarian(dead) || !had_city {
            return;
        }
        self.forget_in_relations(dead);
        self.forget_in_city_states(dead, rules);
        self.forget_in_congress(dead, rules);
        self.forget_in_espionage(dead);
        self.forget_in_victory(dead);
        self.forget_in_revolts(dead);
        self.forget_in_archaeology(dead);
    }
}
