//! Natural wonders in play (rules.md):
//!
//! ```text
//! found        a major nation's unit sees a wonder's tile (the unit has
//!              entered a tile: `touch_natural_wonders`, in the command that
//!              moves it) and the find is recorded at once; a tile the nation
//!              came to know otherwise (a city founded, borders grown) is
//!              found at the end of its turn. The first nation to find it gets
//!              the wonder's FirstFinderGold (× the game speed's gold
//!              percent), and finds fire OnNaturalWonderFoundFirst /
//!              OnNaturalWonderFound (Spain)
//! promotion    a unit entering a tile next to a wonder with an
//!              AdjacentUnitFreePromotion gains it when its role may take
//!              it (Fountain of Youth, Mt. Kilimanjaro)
//! ```
//!
//! Happiness per wonder found and the wonders' own happiness inside the
//! borders are counted by `derived::happiness`; tile yields by
//! `derived::tile_yields`; rough ground by `combat::terrain`.
//!
//! Timing: a unit that enters a tile finds the wonders it sees from there
//! inside the move (the knowledge sync that remembers the tiles runs after
//! the command, so the sight is computed here by the same rule:
//! `UnitSight` and `covered_tiles`); what the nation comes to know in other
//! ways is settled at the end of its turn. "First" is the first find in
//! time.

use crate::classic::ClassicChange;
use crate::ids::{PlayerId, TileId, UnitHandle};
use crate::rules::CompiledRuleset;
use crate::systems::effects::TriggerEvent;
use crate::transaction::journal::UnitChange;
use kernel::derived::visibility::{covered_tiles, UnitSight};
use kernel::rules::speeds::SpeedValues;
use crate::transaction::transaction::WorldTransaction;

impl<'a> WorldTransaction<'a> {
    /// End of `player`'s turn: every wonder site it has explored and not yet
    /// found is found now, in tile order.
    pub(crate) fn settle_natural_wonder_finds(&mut self, player: PlayerId, rules: &CompiledRuleset) {
        if !self.world.players.is_major(player) {
            return;
        }
        let sites = self.world.map.natural_wonder_tiles().to_vec();
        for tile in sites {
            if self.explored(player, tile) && !self.has_found(player, tile) {
                self.find_natural_wonder(player, tile, rules);
            }
        }
    }

    fn explored(&self, player: PlayerId, tile: TileId) -> bool {
        self.world
            .knowledge
            .get(player.0 as usize)
            .is_some_and(|k| k.is_explored(tile))
    }

    fn has_found(&self, player: PlayerId, tile: TileId) -> bool {
        self.world
            .players
            .economy_of(player)
            .is_some_and(|e| e.natural_wonders_found.contains(&tile))
    }

    /// No other major nation found the site before.
    fn first_to_find(&self, player: PlayerId, tile: TileId) -> bool {
        let count = self.world.players.count();
        (0..count)
            .map(|p| PlayerId(p as u8))
            .filter(|p| *p != player && self.world.players.is_major(*p))
            .all(|p| !self.has_found(p, tile))
    }

    fn find_natural_wonder(&mut self, player: PlayerId, tile: TileId, rules: &CompiledRuleset) {
        let first = self.first_to_find(player, tile);
        let Some(economy) = self.world.players.economy_mut(player) else {
            return;
        };
        economy.natural_wonders_found.push(tile);
        let wonder = self.world.map.natural_wonder(tile);
        let gold = wonder
            .and_then(|w| rules.natural_wonders.get(w.0 as usize))
            .map(|def| def.first_finder_gold)
            .unwrap_or(0);
        // × the game speed's gold percent, like the camp gold (`barbarians::clear_camp_at`).
        let gold = if first { SpeedValues::scale(gold, self.world.settings().speed_values().gold_percent) } else { 0 };
        self.world.players.earn(player, gold);
        self.fire(rules, TriggerEvent::NaturalWonderFound { player, tile, first });
        if let Some(wonder) = wonder {
            let happiness = crate::derived::happiness::natural_wonder_find_happiness(self.world, rules, player);
            self.journal.module_changes.push(ClassicChange::NaturalWonderFound { player, tile, wonder, first, gold, happiness });
        }
    }

    /// `unit` entered `tile`: the wonders it sees are found, and the free promotions of the wonders
    /// next to it are taken.
    pub(crate) fn touch_natural_wonders(&mut self, unit: UnitHandle, tile: TileId, rules: &CompiledRuleset) {
        if self.world.map.natural_wonder_tiles().is_empty() {
            return;
        }
        self.find_wonders_in_sight(unit, tile, rules);
        let around = self.world.map.dimensions.neighbors(tile);
        for near in around {
            let promotion = self
                .world
                .map
                .natural_wonder(near)
                .and_then(|w| rules.natural_wonders.get(w.0 as usize))
                .and_then(|def| def.adjacent_unit_free_promotion);
            if let Some(promotion) = promotion {
                self.grant_wonder_promotion(unit, promotion, rules);
            }
        }
    }

    /// The natural wonders the unit sees from `tile` (the sight of the knowledge sync: the unit's
    /// radius there, the lines of sight of the map) are found by its owner now.
    fn find_wonders_in_sight(&mut self, unit: UnitHandle, tile: TileId, rules: &CompiledRuleset) {
        let Some(view) = self.world.units.get(unit) else {
            return;
        };
        let core = *view.core;
        if !self.world.players.is_major(core.owner) {
            return;
        }
        let owner_part = crate::effects::unit::owner_unit_modifiers(self.world, rules, core.owner);
        let mods = crate::effects::unit::with_own_sources(self.world, rules, owner_part, &core);
        let radius = UnitSight::of(rules, &mods, core.kind).at(&self.world.map, tile);
        let seen: Vec<TileId> = covered_tiles(&self.world.map, tile, radius)
            .filter(|t| self.world.map.natural_wonder(*t).is_some())
            .collect();
        for site in seen {
            if !self.has_found(core.owner, site) {
                self.find_natural_wonder(core.owner, site, rules);
            }
        }
    }

    /// The unit's role is one the promotion lists, or the unit is one of its
    /// civilian units.
    fn grant_wonder_promotion(&mut self, unit: UnitHandle, promotion: crate::ids::PromotionId, rules: &CompiledRuleset) {
        let Some(def) = rules.promotions.get(promotion.0 as usize) else {
            return;
        };
        let Some(core) = self.world.units.get_mut_core(unit) else {
            return;
        };
        let kind = rules.units.get(core.kind.0 as usize);
        let role_fits = kind.and_then(|k| k.role).is_some_and(|r| def.unit_roles.contains(&r));
        if !role_fits && !def.civilian_units.contains(&core.kind) {
            return;
        }
        if core.promotions.insert(promotion) {
            self.journal.unit_changes.push(UnitChange::Promoted { unit, promotion });
        }
    }
}
