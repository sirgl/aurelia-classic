//! Barbarian camps and ancient ruins in a replay: which tiles hold them
//! (the improvements of those kinds in the rules), and the events of a
//! command — a camp appeared, a camp cleared (by whom, the gold it paid), a
//! ruin explored (by whom, the reward it gave). The reward is read from the
//! explorer's own difference over the command: a citizen in a city, culture,
//! faith, gold, a unit's experience, else the map around.

use classic_rules::classic::content::BARBARIAN_CAMP;
use classic_rules::{CompiledRuleset, GameState};
use kernel::ids::{PlayerId, TileId};
use kernel::rules::content::ImprovementKind;
use serde_json::{json, Value};

/// What a camp or ruins improvement of the rules is ("camp", "ruins"; None: neither), by improvement id.
pub(super) fn kinds(rules: &CompiledRuleset) -> Vec<Option<&'static str>> {
    rules
        .improvements
        .iter()
        .map(|d| match d.kind {
            ImprovementKind::AncientRuins => Some("ruins"),
            k if k == BARBARIAN_CAMP => Some("camp"),
            _ => None,
        })
        .collect()
}

fn is_ruins(game: &GameState, rules: &CompiledRuleset, tile: u32) -> bool {
    let imp = game.view().auth.map().improvement(TileId(tile)).flatten();
    imp.and_then(|i| rules.improvements.get(i.0 as usize)).is_some_and(|d| d.kind == ImprovementKind::AncientRuins)
}

/// Every tile with ruins (ruins never appear after the map is made: later only fewer are checked).
pub(super) fn ruins_tiles(game: &GameState, rules: &CompiledRuleset) -> Vec<u32> {
    (0..game.view().auth.map().tile_count() as u32).filter(|&t| is_ruins(game, rules, t)).collect()
}

/// The camps, the ruins still there, and the acting player's stocks a reward would change.
#[derive(Debug, Clone, Default)]
pub(super) struct Sites {
    camps: Vec<u32>,
    ruins: Vec<u32>,
    treasury: u32,
    culture: u32,
    faith: u32,
    /// The player's cities: id, name, population.
    cities: Vec<(u64, String, u32)>,
    /// The player's units: id, experience.
    xp: Vec<(u64, u32)>,
}

pub(super) fn sites(game: &GameState, rules: &CompiledRuleset, ruins: &[u32], player: PlayerId) -> Sites {
    let auth = game.view().auth;
    let module = auth.module();
    Sites {
        camps: module.barbarians().camps().iter().map(|c| c.tile.0).collect(),
        ruins: ruins.iter().copied().filter(|&t| is_ruins(game, rules, t)).collect(),
        treasury: auth.players().treasury_of(player),
        culture: module.culture().player(player).map_or(0, |c| c.culture),
        faith: module.religion().players().get(player.0 as usize).map_or(0, |f| f.faith),
        cities: auth.cities().iter().filter(|(_, c)| c.owner == player).map(|(_, c)| (c.id.0, c.name.clone(), c.population)).collect(),
        xp: auth.units().iter_cores().filter(|u| u.owner == player).map(|u| (u.id.0, u.experience)).collect(),
    }
}

/// The camp and ruin events of a command by `player` (`before`: `sites` before it), and the ruins left.
pub(super) fn events(game: &GameState, rules: &CompiledRuleset, before: &Sites, player: PlayerId, round: u32) -> (Vec<Value>, Vec<u32>) {
    let now = sites(game, rules, &before.ruins, player);
    let mut out = Vec::new();
    let gold = now.treasury.saturating_sub(before.treasury);
    for &t in now.camps.iter().filter(|t| !before.camps.contains(t)) {
        out.push(json!({ "t": "camp", "tiles": [t], "r": round }));
    }
    let cleared = game.view().auth.module().barbarians().cleared().to_vec();
    for &t in before.camps.iter().filter(|t| !now.camps.contains(t)) {
        match cleared.iter().rev().find(|c| c.tile.0 == t) {
            Some(c) => out.push(json!({ "t": "camp_cleared", "p": c.player.0, "gold": if c.player == player { gold } else { 0 }, "tiles": [t], "r": round })),
            None => out.push(json!({ "t": "camp_gone", "tiles": [t], "r": round })),
        }
    }
    for &t in before.ruins.iter().filter(|t| !now.ruins.contains(t)) {
        let grown = now.cities.iter().find_map(|(id, name, pop)| {
            let was = before.cities.iter().find(|c| c.0 == *id)?.2;
            (*pop > was).then(|| (name.clone(), pop - was))
        });
        let xp = now.xp.iter().filter_map(|(id, x)| Some(x.saturating_sub(before.xp.iter().find(|u| u.0 == *id)?.1))).max().unwrap_or(0);
        let (reward, amount, city) = if let Some((name, n)) = grown {
            ("citizen", n, Some(name))
        } else if now.culture > before.culture {
            ("culture", now.culture - before.culture, None)
        } else if now.faith > before.faith {
            ("faith", now.faith - before.faith, None)
        } else if gold > 0 {
            ("gold", gold, None)
        } else if xp > 0 {
            ("experience", xp, None)
        } else {
            ("map", 0, None)
        };
        out.push(json!({ "t": "ruins", "p": player.0, "reward": reward, "amount": amount, "city": city, "tiles": [t], "r": round }));
    }
    (out, now.ruins)
}
