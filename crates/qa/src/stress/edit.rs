//! The changes a scene is made of. A change is data (`Change`): a scene is a list of them, a saved
//! scene holds the list, and `apply` makes one on a world. The harness setters do the work; nothing
//! here goes through the AI or changes a rule.

use classic_rules::ids::{BuildingKindId, ImprovementId, PlayerId, TechId, TileId, UnitKindId};
use classic_rules::world::relations::DealItem;
use classic_rules::rules::content::ProjectId;
use classic_rules::{ClassicGame, CompiledRuleset, GameState};
use kernel::ai::plan::{role_of, AiRole};
use kernel::ids::EntityId;
use kernel::rules::content::Domain;
use serde::{Deserialize, Serialize};

use crate::battle::{self, Arm};

/// One change of a world.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "change", rename_all = "snake_case")]
pub enum Change {
    /// The two nations have met.
    Meet { a: u8, b: u8 },
    /// `a` declares war on `b`.
    War { a: u8, b: u8 },
    /// The units (by their persistent ids) are gone, as a death leaves the world.
    RemoveUnits { ids: Vec<u64> },
    /// A unit of `kind` for `player` on `tile`.
    Spawn { player: u8, tile: u32, kind: u16 },
    /// The city (by its persistent id) is taken by `conqueror` as a strike on it would take it.
    CaptureCity { city: u64, conqueror: u8 },
    /// The technologies are known (`debug_discover`: no research, the unlocks follow).
    Discover { player: u8, techs: Vec<u16> },
    /// `times` completions of a project for `player` (Apollo, spaceship parts).
    Project { player: u8, project: u16, times: u16 },
    /// `then` made `after` rounds into the window, at the start of that round (a stack placed at
    /// peace, the war declared later). The play loop makes it (`play::play_arm`); at the start it is
    /// nothing.
    Later { after: u32, then: Box<Change> },
    /// Gold into a treasury (`gold` > 0), or out of it down to an empty one (`gold` < 0).
    Treasury { player: u8, gold: i32 },
    /// An improvement of `player` on `tile`; an improvement with a culture bomb (the citadel) is
    /// raised as a great general raises it, the tiles around it taken.
    Improve { player: u8, tile: u32, improvement: u16 },
    /// The world wonder `building` completed in `player`'s capital: the race for it is over for
    /// everyone else.
    Wonder { player: u8, building: u16 },
    /// A city of `player` founded on `tile` (a forward settle).
    Found { player: u8, tile: u32 },
    /// An accord between the two as an accepted deal makes it.
    Accord { giver: u8, receiver: u8, accord: Accord },
    /// The resources of the tiles gone (a lost strategic or luxury resource).
    Deplete { tiles: Vec<u32> },
}

/// The deal items a scene makes (`Change::Accord`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Accord {
    Friendship,
    DefensivePact,
    /// The giver opens its borders to the receiver.
    OpenBorders,
}

/// What a per-round gift adds to (`Gift`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Yield {
    #[default]
    Science,
    Culture,
    Gold,
}

/// A yield gift: every round the edited arm adds `percent` of the science (culture, gold) `player`
/// earns to its pool (science x1.25 is 25). The one thing the rules have no per-seat setting for (the
/// difficulty is one for all seats, bench/modules.rs); it lives in the exam's play loop only
/// (`play::play_arm`), never in a normal game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gift {
    pub player: u8,
    pub percent: u32,
    /// Science when a saved scene of wave 1 holds no yield.
    #[serde(default)]
    pub yield_kind: Yield,
}

/// Makes `change`; an error names what the world does not hold.
/// A spawned unit's persistent id is returned.
pub fn apply(game: &mut GameState, rules: &CompiledRuleset, change: &Change) -> Result<Option<u64>, String> {
    match change {
        Change::Meet { a, b } => game.debug_meet(PlayerId(*a), PlayerId(*b)),
        Change::War { a, b } => {
            game.debug_force_war(rules, PlayerId(*a), PlayerId(*b));
            if !game.view().auth.diplomacy().at_war(PlayerId(*a), PlayerId(*b)) {
                return Err(format!("the rules refuse the war of {a} on {b}"));
            }
        }
        Change::RemoveUnits { ids } => {
            // A unit that is gone already (it died with a captured city) is not an error.
            for id in ids {
                if let Some(handle) = game.view().auth.units().handle_of(EntityId(*id)) {
                    game.debug_remove_unit(rules, handle);
                }
            }
        }
        Change::Spawn { player, tile, kind } => {
            let unit = game.debug_spawn_unit(rules, PlayerId(*player), TileId(*tile), UnitKindId(*kind));
            return game.view().auth.units().get(unit).map(|u| Some(u.core.id.0)).ok_or_else(|| "the spawned unit is gone".to_owned());
        }
        Change::CaptureCity { city, conqueror } => {
            let handle = game.view().auth.cities().handle_of(EntityId(*city)).ok_or_else(|| format!("no city {city}"))?;
            game.debug_capture_city(rules, handle, PlayerId(*conqueror));
        }
        Change::Discover { player, techs } => {
            for tech in techs {
                game.debug_discover(rules, PlayerId(*player), TechId(*tech));
            }
        }
        Change::Project { player, project, times } => {
            for _ in 0..*times {
                game.debug_add_project(PlayerId(*player), ProjectId(*project));
            }
        }
        Change::Later { .. } => {}
        Change::Treasury { player, gold } => {
            if *gold >= 0 {
                game.debug_earn(PlayerId(*player), gold.unsigned_abs());
            } else {
                game.debug_spend(PlayerId(*player), gold.unsigned_abs());
            }
        }
        Change::Improve { player, tile, improvement } => improve(game, rules, PlayerId(*player), TileId(*tile), ImprovementId(*improvement))?,
        Change::Wonder { player, building } => wonder(game, rules, PlayerId(*player), BuildingKindId(*building))?,
        Change::Found { player, tile } => {
            if game.view().auth.indexes().city_on_tile(TileId(*tile)).is_some() {
                return Err(format!("a city stands on {tile}"));
            }
            game.debug_found_city(rules, PlayerId(*player), TileId(*tile));
        }
        Change::Accord { giver, receiver, accord } => {
            let item = match accord {
                Accord::Friendship => DealItem::Friendship,
                Accord::DefensivePact => DealItem::DefensivePact,
                Accord::OpenBorders => DealItem::OpenBorders,
            };
            game.debug_deal_item(rules, PlayerId(*giver), PlayerId(*receiver), item)?;
        }
        Change::Deplete { tiles } => {
            for tile in tiles {
                game.debug_clear_resource(rules, TileId(*tile));
            }
        }
    }
    Ok(None)
}

/// The changes `Later` holds, with the round of the window each is made in (1 = the first).
pub fn later(changes: &[Change]) -> Vec<(u32, Change)> {
    let mut out = Vec::new();
    for change in changes {
        if let Change::Later { after, then } = change {
            out.push((*after, (**then).clone()));
        }
    }
    out
}

fn improve(game: &mut GameState, rules: &CompiledRuleset, player: PlayerId, tile: TileId, improvement: ImprovementId) -> Result<(), String> {
    let def = rules.improvements.get(improvement.0 as usize).ok_or_else(|| format!("no improvement {}", improvement.0))?;
    if def.culture_bomb_radius > 0 {
        if !game.debug_raise_citadel(rules, player, tile) {
            return Err("the rules hold no citadel".to_owned());
        }
        return Ok(());
    }
    game.debug_set_improvement_batch(rules, &[(tile, improvement)]);
    Ok(())
}

fn wonder(game: &mut GameState, rules: &CompiledRuleset, player: PlayerId, building: BuildingKindId) -> Result<(), String> {
    let capital = classic_rules::combat::city::capital_of(game.view().auth, rules, player).ok_or("the wonder's builder has no capital")?;
    game.debug_complete_building(rules, capital, building);
    let built = game.view().auth.cities().get(capital).is_some_and(|c| c.buildings.contains(&building));
    if !built {
        return Err(format!("the rules refuse the wonder {} in the capital of {}", building.0, player.0));
    }
    Ok(())
}

/// Makes every change in order; the ids of the units spawned.
pub fn apply_all(game: &mut GameState, rules: &CompiledRuleset, changes: &[Change]) -> Result<Vec<u64>, String> {
    let mut spawned = Vec::new();
    for change in changes {
        spawned.extend(apply(game, rules, change)?);
    }
    Ok(spawned)
}

/// The military units of `player` (any domain), by persistent id, in id order.
pub fn military_units(game: &GameState, rules: &CompiledRuleset, player: PlayerId) -> Vec<u64> {
    let auth = game.view().auth;
    let mut ids: Vec<u64> = auth
        .indexes()
        .units_of(player)
        .iter()
        .filter_map(|h| auth.units().get(*h))
        .filter(|u| role_of(rules, u.core.kind) == AiRole::Military)
        .map(|u| u.core.id.0)
        .collect();
    ids.sort_unstable();
    ids
}

/// The strongest land unit of each arm `player` can build by its technologies and nation: the
/// enemy's own unit list (no unit is named here). Resources and buildings are not asked: the unit
/// is placed, not trained.
pub(crate) fn kinds_of(game: &GameState, rules: &CompiledRuleset, player: PlayerId) -> std::collections::BTreeMap<Arm, UnitKindId> {
    let auth = game.view().auth;
    let known = auth.players().techs_of(player);
    let nation = auth.players().nation_of(player);
    let buildable: Vec<UnitKindId> = (0..rules.units.len())
        .map(|i| UnitKindId(i as u16))
        .filter(|&kind| {
            let def = &rules.units[kind.0 as usize];
            def.domain == Domain::Land
                && !def.unbuildable
                && role_of(rules, kind) == AiRole::Military
                && battle::kind_arm(rules, kind).is_some()
                && def.required_tech.is_none_or(|t| known.contains(&t))
                && def.obsolete_tech.is_none_or(|t| !known.contains(&t))
                && def.nation.is_none_or(|n| n == nation)
        })
        .collect();
    battle::best_kinds(rules, &buildable)
}
