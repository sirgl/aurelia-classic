//! Archaeology (owner of `world::tourism` archaeology; rules:
//! derived::archaeology, classic/docs/classic/rules.md).
//! Battles leave history records on tiles; when a player first learns the
//! technology that reveals antiquity sites, the sites appear (weighted by
//! history, crowding, terrain); an archaeologist on a site digs it for
//! `dig_turns` and leaves the dig improvement on the tile (it removes the
//! site's resource: `removes_resource`), then its player chooses
//! (`prompt_when_complete`) a Landmark over the dig (`requires_improvement`)
//! or an artifact, a writing or a culture boost, which empty the tile.
//! Journal: TileChange::ResourceSet (sites on and off the map),
//! TileChange::ImprovementBuilt (dig, Landmark, emptied tile).

use crate::command::CommandError;
use crate::derived::archaeology::{
    choice_status, dig_improvement, dig_turns, excavate_status, ideal_sites, landmark, replaces, site_base_weight,
    site_draw_range, site_resources, site_weight, trigger_tech, writing_sites, ArchaeologyChoice,
    DigBlock, BOOST_TURNS, INVENTED_KINDS, PERCENT_HIDDEN, SITE_CROWD_RADIUS,
};
use crate::derived::tourism::free_slot;
use crate::ids::{PlayerId, TileId, UnitHandle};
use crate::rules::CompiledRuleset;
use crate::transaction::journal::TileChange;
use crate::transaction::transaction::WorldTransaction;
use crate::world::tourism::{
    ArchRecord, ArtifactKind, Dig, DigSite, GreatWork, GreatWorkClass, PendingDig,
};

fn dig_rule(block: DigBlock) -> CommandError {
    CommandError::RuleViolation(block.reason().into())
}

impl<'a> WorldTransaction<'a> {
    /// A battle on `tile` (attacker `first`, defender `second`) is written
    /// into the tile's history until the sites appear.
    pub(crate) fn record_battle(
        &mut self,
        tile: TileId,
        first: PlayerId,
        second: PlayerId,
        rules: &CompiledRuleset,
    ) {
        // Content without antiquity sites keeps no history.
        if trigger_tech(rules).is_none() {
            return;
        }
        let archaeology = self.world.module.tourism.archaeology();
        let kind = ArtifactKind::BattleMelee;
        if archaeology.triggered || !replaces(archaeology.record_at(tile), kind) {
            return;
        }
        let era = crate::world::player::player_era(self.world, rules, first);
        let record = ArchRecord {
            kind,
            era,
            first,
            second: Some(second),
        };
        let records = &mut self.world.module.tourism.archaeology_mut().records;
        match records.binary_search_by_key(&tile, |(t, _)| *t) {
            Ok(i) => records[i].1 = record,
            Err(i) => records.insert(i, (tile, record)),
        }
    }

    /// The archaeologist starts digging the site it stands on.
    pub(crate) fn excavate(
        &mut self,
        unit: UnitHandle,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        let site = excavate_status(self.world, rules, unit).map_err(dig_rule)?;
        let core = *self
            .world
            .units
            .get(unit)
            .ok_or(CommandError::UnknownUnit)?
            .core;
        self.world.module.tourism.archaeology_mut().digs.push(Dig {
            unit: core.id,
            player: core.owner,
            tile: site.tile,
            turns_left: dig_turns(rules),
        });
        Ok(())
    }

    /// The player takes what it chose from a finished dig.
    pub(crate) fn choose_archaeology(
        &mut self,
        player: PlayerId,
        tile: TileId,
        choice: ArchaeologyChoice,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        let pending = choice_status(self.world, rules, player, tile, choice).map_err(dig_rule)?;
        let record = pending.site.record;
        if choice != ArchaeologyChoice::Landmark {
            self.clear_dig(tile);
        }
        match choice {
            ArchaeologyChoice::Landmark => self.build_landmark(tile, record.era, rules),
            ArchaeologyChoice::ArtifactFirst => self.dig_up(
                player,
                tile,
                GreatWorkClass::Artifact,
                record.first,
                record.era,
                rules,
            ),
            ArchaeologyChoice::ArtifactSecond => {
                let second = record.second.unwrap_or(record.first);
                self.dig_up(
                    player,
                    tile,
                    GreatWorkClass::Artifact,
                    second,
                    record.era,
                    rules,
                )
            }
            ArchaeologyChoice::Writing => self.dig_up(
                player,
                tile,
                GreatWorkClass::Literature,
                record.first,
                record.era,
                rules,
            ),
            ArchaeologyChoice::CultureBoost => {
                let culture = self.world.module.culture.of(player).recent_culture(BOOST_TURNS);
                self.gain_culture(player, culture);
            }
        }
        let pending_list = &mut self.world.module.tourism.archaeology_mut().pending;
        pending_list.retain(|p| !(p.player == player && p.site.tile == tile));
        Ok(())
    }

    /// Tourism step: the sites appear on the first discovery of their
    /// technology; the player's digs advance.
    pub(crate) fn archaeology_turn(&mut self, player: PlayerId, rules: &CompiledRuleset) {
        let triggered = self.world.module.tourism.archaeology().triggered;
        let knows =
            trigger_tech(rules).is_some_and(|t| self.world.players.techs_of(player).contains(&t));
        if !triggered && knows {
            self.spawn_sites(rules);
        }
        self.advance_digs(player, rules);
    }

    fn advance_digs(&mut self, player: PlayerId, rules: &CompiledRuleset) {
        let digs: Vec<Dig> = self
            .world
            .module
            .tourism
            .archaeology()
            .digs
            .iter()
            .copied()
            .filter(|d| d.player == player)
            .collect();
        for dig in digs {
            let unit = self
                .world
                .units
                .iter_handles()
                .find(|(_, c)| c.id == dig.unit)
                .map(|(h, c)| (h, c.tile));
            let archaeology = self.world.module.tourism.archaeology_mut();
            archaeology.digs.retain(|d| d.unit != dig.unit);
            let Some((handle, _)) = unit.filter(|(_, at)| *at == dig.tile) else {
                continue;
            };
            if dig.turns_left > 1 {
                archaeology.digs.push(Dig {
                    turns_left: dig.turns_left - 1,
                    ..dig
                });
                continue;
            }
            self.finish_dig(dig, handle, rules);
        }
    }

    /// The dig improvement stands on the site, the archaeologist is spent;
    /// a dig that removes the resource takes the site off the map, a dig
    /// that prompts lets the player choose later (`ChooseArchaeology`).
    fn finish_dig(&mut self, dig: Dig, unit: UnitHandle, rules: &CompiledRuleset) {
        let archaeology = self.world.module.tourism.archaeology_mut();
        let Some(i) = archaeology.sites.iter().position(|s| s.tile == dig.tile) else {
            return;
        };
        let site = archaeology.sites.remove(i);
        let Some(improvement) = dig_improvement(rules) else {
            return;
        };
        let def = &rules.improvements[improvement.0 as usize];
        if def.placement.prompt_when_complete {
            archaeology.pending.push(PendingDig {
                player: dig.player,
                site,
            });
        }
        self.world.map.set_improvement(dig.tile, Some(improvement));
        self.journal
            .tile_changes
            .push(TileChange::ImprovementBuilt { tile: dig.tile });
        let (visible, _) = site_resources(rules);
        let on_map = visible.is_some() && self.world.map.resource(dig.tile).flatten() == visible;
        if def.removes_resource && on_map {
            self.world.map.set_resource(dig.tile, None);
            self.journal
                .tile_changes
                .push(TileChange::ResourceSet { tile: dig.tile });
        }
        let _ = self.destroy_unit(unit);
    }

    /// An artifact, a writing or a culture boost empties the dig's tile.
    fn clear_dig(&mut self, tile: TileId) {
        if self.world.map.improvement(tile).flatten().is_none() {
            return;
        }
        self.world.map.set_improvement(tile, None);
        self.journal
            .tile_changes
            .push(TileChange::ImprovementBuilt { tile });
    }

    fn build_landmark(&mut self, tile: TileId, era: u8, rules: &CompiledRuleset) {
        let Some(improvement) = landmark(rules) else {
            return;
        };
        self.world.map.set_improvement(tile, Some(improvement));
        self.journal
            .tile_changes
            .push(TileChange::ImprovementBuilt { tile });
        let landmarks = &mut self.world.module.tourism.archaeology_mut().landmarks;
        landmarks.retain(|(t, _)| *t != tile);
        landmarks.push((tile, era));
    }

    fn dig_up(
        &mut self,
        player: PlayerId,
        tile: TileId,
        class: GreatWorkClass,
        creator: PlayerId,
        era: u8,
        rules: &CompiledRuleset,
    ) {
        if let Some(slot) = free_slot(self.world, rules, player, class.slot(), tile) {
            self.place_work(
                GreatWork {
                    class,
                    era,
                    creator,
                },
                slot,
            );
        }
    }

    /// The sites appear: the heaviest
    /// tile first, crowding divides the weight around each chosen site.
    fn spawn_sites(&mut self, rules: &CompiledRuleset) {
        let majors = self.world.players.count() as u32;
        let range = site_draw_range(majors).max(1);
        let rng = &mut self.world.module.tourism.archaeology_mut().rng;
        let ideal = ideal_sites(
            majors,
            (
                rng.below(u64::from(range)) as u32,
                rng.below(u64::from(range)) as u32,
            ),
        );
        let chosen = self.choose_site_tiles(ideal, rules);
        let mut writings = writing_sites(ideal);
        let (visible, _) = site_resources(rules);
        for tile in chosen {
            let hidden = self.world.module.tourism.archaeology_mut().rng.below(100) < PERCENT_HIDDEN;
            let mut record = self.site_record(tile, rules);
            if hidden && writings > 0 {
                record.kind = ArtifactKind::Writing;
                writings -= 1;
            }
            self.world.module.tourism.archaeology_mut().sites.push(DigSite {
                tile,
                record,
                hidden,
            });
            if let (false, Some(resource)) = (hidden, visible) {
                self.world.map.set_resource(tile, Some(resource));
                self.journal
                    .tile_changes
                    .push(TileChange::ResourceSet { tile });
            }
        }
        let archaeology = self.world.module.tourism.archaeology_mut();
        archaeology.records.clear();
        archaeology.triggered = true;
    }

    /// Up to `ideal` tiles by weight / crowding (ties: the lower tile id).
    fn choose_site_tiles(&mut self, ideal: u32, rules: &CompiledRuleset) -> Vec<TileId> {
        let dims = self.world.map.dimensions;
        let count = dims.tile_count();
        let mut weight = vec![0u32; count];
        for (t, w) in weight.iter_mut().enumerate() {
            let tile = TileId(t as u32);
            if let Some(base) = site_base_weight(self.world, rules, tile) {
                let noise = self.world.module.tourism.archaeology_mut().rng.below(10) as u32;
                *w = site_weight(self.world, rules, tile, base, noise);
            }
        }
        let mut crowd = vec![1u32; count];
        let mut chosen = Vec::new();
        while chosen.len() < ideal as usize {
            let best = (0..count)
                .filter(|t| weight[*t] > 0)
                .max_by_key(|t| (weight[*t] / crowd[*t], std::cmp::Reverse(*t)));
            let Some(t) = best else {
                break;
            };
            weight[t] = 0;
            chosen.push(TileId(t as u32));
            crowd_around(dims, TileId(t as u32), &mut crowd);
        }
        chosen
    }

    /// The tile's history, or an invented one: era weighted toward the
    /// oldest, kind from the random table, the land's (or the nearest
    /// city's) owner first.
    fn site_record(&mut self, tile: TileId, rules: &CompiledRuleset) -> ArchRecord {
        if let Some(r) = self.world.module.tourism.archaeology().record_at(tile) {
            return *r;
        }
        let players = self.world.players.count() as u8;
        let highest = (0..players)
            .map(|p| crate::world::player::player_era(self.world, rules, PlayerId(p)))
            .max()
            .unwrap_or(0);
        let first = self.nearest_owner(tile);
        let rng = &mut self.world.module.tourism.archaeology_mut().rng;
        let era = invented_era(rng, highest);
        let kind = INVENTED_KINDS[rng.below(INVENTED_KINDS.len() as u64) as usize];
        let paired = matches!(
            kind,
            ArtifactKind::BattleMelee | ArtifactKind::BattleRanged | ArtifactKind::RazedCity
        );
        let second = match (paired, players > 1) {
            (true, true) => Some(PlayerId(
                ((u64::from(first.0) + 1 + rng.below(u64::from(players) - 1)) % u64::from(players))
                    as u8,
            )),
            _ => None,
        };
        ArchRecord {
            kind,
            era,
            first,
            second,
        }
    }

    /// The tile's owner, else the owner of the nearest city, else player 0.
    fn nearest_owner(&self, tile: TileId) -> PlayerId {
        if let Some(owner) = self.world.map.owner(tile).flatten() {
            return owner;
        }
        let dims = self.world.map.dimensions;
        self.world
            .cities
            .iter()
            .min_by_key(|(_, c)| (dims.distance(c.tile, tile).unwrap_or(i32::MAX), c.tile.0))
            .map(|(_, c)| c.owner)
            .unwrap_or(PlayerId(0))
    }
}

/// An era in 0..highest with weight (highest − i); 0 when nobody left the
/// first era.
fn invented_era(rng: &mut crate::rng::StreamRng, highest: u8) -> u8 {
    if highest == 0 {
        return 0;
    }
    let total = u64::from(highest) * (u64::from(highest) + 1) / 2;
    let mut roll = rng.below(total);
    for i in 0..highest {
        let w = u64::from(highest - i);
        if roll < w {
            return i;
        }
        roll -= w;
    }
    0
}

/// A chosen site crowds the tiles around it: `SITE_CROWD_RADIUS + 1 - distance` within the radius (the
/// weights of the tiles are divided by the sum, so sites spread out). Own design.
fn crowd_around(dims: crate::math::MapDimensions, site: TileId, crowd: &mut [u32]) {
    let Some(c) = dims.to_coord(site) else {
        return;
    };
    let r = SITE_CROWD_RADIUS;
    for dq in -r..=r {
        for dr in -r..=r {
            let Some(t) = dims.to_tile(crate::math::HexCoord {
                q: c.q + dq,
                r: c.r + dr,
            }) else {
                continue;
            };
            let Some(d) = dims.distance(site, t) else {
                continue;
            };
            if d <= r {
                crowd[t.0 as usize] += (r + 1 - d) as u32;
            }
        }
    }
}
