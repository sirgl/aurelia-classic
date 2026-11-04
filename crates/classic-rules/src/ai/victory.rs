//! Decisions towards a science victory (M4 track C):
//!
//! - production (asked by the city planner before buildings): the Apollo
//!   Program once Rocketry is known, then exactly the spaceship parts the
//!   ship still lacks — parts installed, parts walking and parts queued
//!   count — in the most productive cities;
//! - units: parts walk to the capital and are installed there; an own
//!   civilian resting in the capital steps aside for them (one civilian
//!   per tile).
//!
//! The variant `space-finish` (classic/docs/ai-science-victory.md) plans
//! the empire's space orders at once instead (`space_plan`): each item in
//! the city that has it at the capital soonest, before the items the
//! allocator chose there, a waiting part moved where it is finished much
//! sooner; its parts walk the AI's own path around units in the way
//! (`walk_around`). The prerequisite project and the parts come from the
//! rules' data (`prerequisite_projects`, `part_kinds`).
//!
//! Whether an order is possible is asked through `PlayerView::accepts`
//! (the same check the interface greys buttons with).

use crate::knowledge::public::PlayerQueries;
use crate::ai::planner::{role_of, AiRole};
use crate::command::Command;
use crate::ids::{ProductionItem, TileId, UnitKindId};
use crate::knowledge::observation::{AIObservation, OwnCity};
use crate::rules::content::{BuildingKind, ProjectId};
use crate::rules::CompiledRuleset;
use crate::world::victory::VictoryKind;

/// The capital: the city with the palace (parts are installed there),
/// else the oldest own city.
fn capital<'o>(obs: &'o AIObservation<'_>, rules: &CompiledRuleset) -> Option<&'o OwnCity> {
    let palace = |city: &&OwnCity| {
        city.buildings
            .iter()
            .any(|b| rules.buildings.get(b.0 as usize).is_some_and(|d| d.kind == BuildingKind::Palace))
    };
    obs.own_cities.iter().find(palace).or(obs.own_cities.first())
}

/// The projects the spaceship's parts need before a city may build them
/// (their `required_project`: the Apollo Program of the classic rules), by
/// id.
pub fn prerequisite_projects(rules: &CompiledRuleset) -> Vec<ProjectId> {
    let mut out: Vec<ProjectId> = part_kinds(rules)
        .into_iter()
        .filter_map(|(kind, _, _)| rules.units.get(kind.0 as usize).and_then(|d| d.required_project))
        .collect();
    out.sort_by_key(|p| p.0);
    out.dedup();
    out
}

/// Spaceship part units with the project each one installs and how many
/// of it the ship needs.
pub fn part_kinds(rules: &CompiledRuleset) -> Vec<(UnitKindId, ProjectId, u16)> {
    let parts = crate::world::victory::spaceship_parts(rules);
    let mut out = Vec::new();
    for (index, def) in rules.units.iter().enumerate() {
        let Some(project) = def.spaceship_project else {
            continue;
        };
        if let Some((_, needed)) = parts.iter().find(|(p, _)| *p == project) {
            out.push((UnitKindId(index as u16), project, *needed));
        }
    }
    out
}

fn completed(obs: &AIObservation<'_>, project: ProjectId) -> u16 {
    obs.economy.projects.get(project.0 as usize).copied().unwrap_or(0)
}

/// Queued in any own city or already planned this turn.
fn ordered(obs: &AIObservation<'_>, planned: &[ProductionItem], item: ProductionItem) -> usize {
    let queued = obs.own_cities.iter().flat_map(|c| c.queue.iter()).filter(|i| **i == item).count();
    queued + planned.iter().filter(|i| **i == item).count()
}

/// The next space order of `city` (None: nothing towards the spaceship is
/// wanted or possible here). `planned` holds the orders already given this
/// turn in other cities.
pub fn next_space_item(
    obs: &AIObservation<'_>,
    rules: &CompiledRuleset,
    city: &OwnCity,
    planned: &[ProductionItem],
) -> Option<ProductionItem> {
    if !obs.victories.enabled(VictoryKind::Science) {
        return None;
    }
    let accepted = |item: ProductionItem| {
        obs.view.accepts(&Command::EnqueueProduction {
            player: obs.player,
            city: city.handle,
            item,
        })
    };
    if let Some(project) = prerequisite_projects(rules).into_iter().find(|p| completed(obs, *p) == 0) {
        let item = ProductionItem::Project(project);
        let wanted = ordered(obs, planned, item) == 0 && accepted(item);
        return wanted.then_some(item);
    }
    for (kind, project, needed) in part_kinds(rules) {
        let item = ProductionItem::Unit(kind);
        let walking = obs.own_units.iter().filter(|u| u.kind == kind).count();
        let have = usize::from(completed(obs, project)) + walking + ordered(obs, planned, item);
        if have < usize::from(needed) && accepted(item) {
            return Some(item);
        }
    }
    None
}

/// A part waiting in a city is moved when another city finishes it in at
/// most `1 / STUCK_FACTOR` of the turns it still needs where it is, and at
/// least `STUCK_MIN_TURNS` sooner (own values: a move must be worth more
/// than the noise of the estimate, and must not swing back the next turn;
/// `STUCK_MIN_TURNS` is production turns at Standard speed, × the speed's pace).
pub const STUCK_FACTOR: u32 = 2;
pub const STUCK_MIN_TURNS: u32 = 5;

/// A city's new queue in the space plan (`space_plan`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpaceQueue {
    pub city: crate::ids::CityHandle,
    pub queue: Vec<ProductionItem>,
}

/// A space item: a prerequisite project or a part.
fn is_space_item(rules: &CompiledRuleset, prerequisites: &[ProjectId], item: ProductionItem) -> bool {
    match item {
        ProductionItem::Unit(kind) => is_part(rules, kind),
        ProductionItem::Project(project) => prerequisites.contains(&project),
        ProductionItem::Building(_) => false,
    }
}

/// An item a space order goes before: one the allocator chose, whose
/// only loss is the delay (a building that is no world wonder, a worker, a
/// civilian). The space need comes before the allocator in the city
/// planner's order of needs (`planner_cities::choose_order`); a world
/// wonder (a race the allocator prices by its turns), a unit of the army,
/// a settler and a project keep their place.
fn yields_to_space(rules: &CompiledRuleset, item: ProductionItem) -> bool {
    match item {
        ProductionItem::Building(kind) => rules.buildings.get(kind.0 as usize).is_some_and(|b| !b.wonder),
        ProductionItem::Unit(kind) => matches!(role_of(rules, kind), AiRole::Worker | AiRole::Civilian),
        ProductionItem::Project(_) => false,
    }
}

/// One city's queue as the space plan sees it: the head items that keep
/// their place, the space items, the rest.
struct Line<'o> {
    city: &'o OwnCity,
    kept: Vec<ProductionItem>,
    space: Vec<ProductionItem>,
    rest: Vec<ProductionItem>,
}

impl<'o> Line<'o> {
    fn of(rules: &CompiledRuleset, prerequisites: &[ProjectId], city: &'o OwnCity) -> Self {
        let space = |item: &ProductionItem| is_space_item(rules, prerequisites, *item);
        let head = city.queue.iter().take_while(|i| !space(i) && !yields_to_space(rules, **i)).count();
        let (kept, tail) = city.queue.split_at(head);
        let (space, rest) = tail.iter().partition(|i| space(i));
        Self { city, kept: kept.to_vec(), space, rest }
    }

    /// Turns until the first `count` space items are done (after the kept
    /// head), and the stock left for the next item: the city's stock goes
    /// to its first item.
    fn done_after(&self, obs: &AIObservation<'_>, rules: &CompiledRuleset, count: usize) -> (u32, u32) {
        let mut stock = self.city.production;
        let mut turns = 0u32;
        for item in self.kept.iter().chain(self.space.iter().take(count)) {
            turns = turns.saturating_add(crate::ai::planner_cities::turns_from(obs, rules, self.city, *item, stock));
            stock = 0;
        }
        (turns, stock)
    }

    /// Turns until `item` would be done here, ordered after the space
    /// items the city holds, and taken to the capital in `walk` more (None:
    /// the rules refuse it here, the city produces nothing, or there is no
    /// way to the capital).
    fn finish(&self, obs: &AIObservation<'_>, rules: &CompiledRuleset, item: ProductionItem, walk: Option<u32>) -> Option<u32> {
        let walk = walk?;
        let order = Command::EnqueueProduction { player: obs.player, city: self.city.handle, item };
        if !obs.view.accepts(&order) {
            return None;
        }
        let (ahead, stock) = self.done_after(obs, rules, self.space.len());
        let own = crate::ai::planner_cities::turns_from(obs, rules, self.city, item, stock);
        (own != u32::MAX).then(|| ahead.saturating_add(own).saturating_add(walk))
    }

    fn queue(&self) -> Vec<ProductionItem> {
        let mut out: Vec<ProductionItem> = self.kept.iter().chain(&self.space).chain(&self.rest).copied().collect();
        out.truncate(crate::world::city::MAX_QUEUE);
        out
    }
}

/// The line other than `skip` that finishes `item` soonest, and in how
/// many turns; ties go to the more productive city, then the older one.
fn soonest(obs: &AIObservation<'_>, rules: &CompiledRuleset, lines: &[Line<'_>], walks: &Walks, item: ProductionItem, skip: Option<usize>) -> Option<(usize, u32)> {
    lines
        .iter()
        .enumerate()
        .filter(|(at, _)| Some(*at) != skip)
        .filter_map(|(at, line)| line.finish(obs, rules, item, walks.turns(at, item)).map(|turns| (at, turns)))
        .min_by_key(|(at, turns)| (*turns, std::cmp::Reverse(lines[*at].city.income.production), lines[*at].city.id))
}

/// Turns a new part needs from each line's city to the capital, by part
/// kind: the AI's known map searched once from the capital outward
/// (`planner_map::explore`; a way back is priced as the way out). None: no
/// known way. A project stays where it is built: no walk.
struct Walks {
    kinds: Vec<(UnitKindId, Vec<Option<u32>>)>,
}

impl Walks {
    fn of(obs: &AIObservation<'_>, rules: &CompiledRuleset, lines: &[Line<'_>]) -> Self {
        use crate::ai::planner_map::{explore, turns_for, Blockers, KnownMap, MapScratch, SearchLimit, SearchStats, Visit};
        let Some(capital) = capital(obs, rules).map(|c| c.tile) else {
            return Self { kinds: Vec::new() };
        };
        let mut kinds = Vec::new();
        let mut scratch = MapScratch::default();
        for (kind, _, _) in part_kinds(rules) {
            let moves = rules.units.get(kind.0 as usize).map_or(1, |d| d.movement.max(1)) * crate::world::unit::MOVE_DENOMINATOR;
            let mut points: Vec<Option<u32>> = vec![None; lines.len()];
            let map = KnownMap::of_kind(&obs.view, kind);
            let limit = SearchLimit { max_nodes: obs.view.tile_count() as u32 };
            let mut left = lines.len();
            explore(&map, capital, &Blockers::new(Vec::new()), &mut scratch, limit, &mut SearchStats::default(), &mut |tile, cost| {
                for (at, line) in lines.iter().enumerate() {
                    if line.city.tile == tile && points[at].is_none() {
                        points[at] = Some(turns_for(cost, moves));
                        left -= 1;
                    }
                }
                if left == 0 { Visit::Stop } else { Visit::Continue }
            });
            kinds.push((kind, points));
        }
        Self { kinds }
    }

    fn turns(&self, line: usize, item: ProductionItem) -> Option<u32> {
        match item {
            ProductionItem::Unit(kind) => self.kinds.iter().find(|(k, _)| *k == kind).and_then(|(_, t)| t.get(line).copied().flatten()),
            ProductionItem::Project(_) | ProductionItem::Building(_) => Some(0),
        }
    }
}

/// The space items the empire still lacks and nobody has queued: the first
/// prerequisite project not completed, else every part beyond those
/// installed, walking and queued; the dearest first.
fn missing_items(obs: &AIObservation<'_>, rules: &CompiledRuleset, prerequisites: &[ProjectId]) -> Vec<ProductionItem> {
    if let Some(project) = prerequisites.iter().copied().find(|p| completed(obs, *p) == 0) {
        let item = ProductionItem::Project(project);
        return if ordered(obs, &[], item) == 0 { vec![item] } else { Vec::new() };
    }
    let mut out = Vec::new();
    for (kind, project, needed) in part_kinds(rules) {
        let item = ProductionItem::Unit(kind);
        let walking = obs.own_units.iter().filter(|u| u.kind == kind).count();
        let have = usize::from(completed(obs, project)) + walking + ordered(obs, &[], item);
        out.extend(std::iter::repeat_n(item, usize::from(needed).saturating_sub(have)));
    }
    let cost = |item: &ProductionItem| match item {
        ProductionItem::Unit(kind) => rules.units.get(kind.0 as usize).map_or(0, |d| d.cost),
        _ => 0,
    };
    out.sort_by_key(|i| std::cmp::Reverse(cost(i)));
    out
}

/// Where the spaceship is built (variant `space-finish`,
/// classic/docs/ai-science-victory.md): every space item goes to the city
/// that finishes it soonest — by its production toward the item (the item's
/// own percents) and what the city is building first — before the items
/// the allocator chose there (`yields_to_space`); a space item waiting
/// where another city finishes it much sooner (`STUCK_FACTOR`,
/// `STUCK_MIN_TURNS`) is moved there. The city that pays the World
/// Congress project (`ai::congress`) is left to it. The new queues of the
/// cities whose queue changes, in the order of the cities' ids.
pub fn space_plan(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Vec<SpaceQueue> {
    if !obs.victories.enabled(VictoryKind::Science) {
        return Vec::new();
    }
    let prerequisites = prerequisite_projects(rules);
    let contributing = crate::ai::congress::contributing_city(obs);
    let mut cities: Vec<&OwnCity> = obs.own_cities.iter().filter(|c| Some(c.handle) != contributing).collect();
    cities.sort_by_key(|c| c.id);
    let mut lines: Vec<Line<'_>> = cities.into_iter().map(|c| Line::of(rules, &prerequisites, c)).collect();
    let missing = missing_items(obs, rules, &prerequisites);
    if missing.is_empty() && lines.iter().all(|l| l.space.is_empty()) {
        return Vec::new();
    }
    // The rules let no part start before its projects: no walk to price.
    let parts_open = prerequisites.iter().all(|p| completed(obs, *p) > 0);
    let walks = if parts_open { Walks::of(obs, rules, &lines) } else { Walks { kinds: Vec::new() } };
    for at in 0..lines.len() {
        let mut index = 0;
        while index < lines[at].space.len() {
            let item = lines[at].space[index];
            let here = lines[at].done_after(obs, rules, index + 1).0.saturating_add(walks.turns(at, item).unwrap_or(u32::MAX));
            match soonest(obs, rules, &lines, &walks, item, Some(at)) {
                Some((there, turns)) if turns.saturating_mul(STUCK_FACTOR) <= here && here - turns >= obs.speed.pace_rounds(STUCK_MIN_TURNS) => {
                    lines[at].space.remove(index);
                    lines[there].space.push(item);
                }
                _ => index += 1,
            }
        }
    }
    for item in missing {
        if let Some((there, _)) = soonest(obs, rules, &lines, &walks, item, None) {
            lines[there].space.push(item);
        }
    }
    lines
        .iter()
        .map(|line| SpaceQueue { city: line.city.handle, queue: line.queue() })
        .filter(|plan| obs.own_cities.iter().any(|c| c.handle == plan.city && c.queue != plan.queue))
        .collect()
}

fn is_part(rules: &CompiledRuleset, kind: UnitKindId) -> bool {
    rules.units.get(kind.0 as usize).is_some_and(|d| d.spaceship_project.is_some())
}

/// Parts in the capital are installed; the others march there.
pub fn spaceship_orders(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Vec<Command> {
    spaceship_orders_with(obs, rules, false)
}

/// `spaceship_orders`; with `around` (variant `space-finish`) a part walks
/// the AI's own path to the capital, around the tiles where it may not
/// stop (`walk_around`), instead of one march order.
pub fn spaceship_orders_with(obs: &AIObservation<'_>, rules: &CompiledRuleset, around: bool) -> Vec<Command> {
    let Some(capital) = capital(obs, rules).map(|c| c.tile) else {
        return Vec::new();
    };
    if around {
        return walked_orders(obs, rules, capital);
    }
    let waiting = obs.own_units.iter().any(|u| is_part(rules, u.kind) && u.tile != capital);
    let mut out: Vec<Command> = Vec::new();
    if waiting {
        out.extend(clear_capital(obs, rules, capital, Clearing::NEAR));
    }
    let parts = obs
        .own_units
        .iter()
        .filter(|u| is_part(rules, u.kind))
        .map(|u| {
            if u.tile == capital {
                Command::BuildSpaceshipPart { unit: u.handle }
            } else {
                Command::MarchTo {
                    unit: u.handle,
                    destination: capital,
                }
            }
        })
        .filter(|c| obs.view.accepts(c));
    out.extend(parts);
    out
}

/// `spaceship_orders` with the parts walking around blockers: the capital
/// cleared while a part waits outside, parts in it installed, the others
/// walked (`walk_around`).
fn walked_orders(obs: &AIObservation<'_>, rules: &CompiledRuleset, capital: TileId) -> Vec<Command> {
    let parts: Vec<_> = obs.own_units.iter().filter(|u| is_part(rules, u.kind)).collect();
    let mut out: Vec<Command> = Vec::new();
    if parts.iter().any(|u| u.tile != capital) {
        out.extend(clear_capital(obs, rules, capital, Clearing::WIDE));
    }
    let mut taken: Vec<TileId> = Vec::new();
    for unit in parts {
        if unit.tile == capital {
            out.push(Command::BuildSpaceshipPart { unit: unit.handle });
            continue;
        }
        let steps: Vec<Command> = walk_around(obs, rules, unit, capital, &taken)
            .into_iter()
            .map(|tile| Command::MoveUnit { unit: unit.handle, destination: tile })
            .collect();
        match steps.first() {
            Some(first) if obs.view.accepts(first) => {
                if let Some(Command::MoveUnit { destination, .. }) = steps.last() {
                    taken.push(*destination);
                }
                out.extend(steps);
            }
            _ => out.push(Command::MarchTo { unit: unit.handle, destination: capital }),
        }
    }
    out.retain(|c| !matches!(c, Command::BuildSpaceshipPart { .. } | Command::MarchTo { .. }) || obs.view.accepts(c));
    out
}

/// The tiles a part steps through this turn on the AI's own path to the
/// capital (`planner_map::find_path`): around the tiles it may not stop on
/// (a foreign unit; an own unit of its stacking class, any own unit on
/// water; `taken` by another part this turn), as far as its moves go, each
/// step to a neighbouring tile. Empty: no such path (the caller marches).
/// One march order to the capital stopped for good at a city-state's
/// warrior standing in the way (pace map pangaea, seed 6): the rules'
/// march does not go around units.
fn walk_around(obs: &AIObservation<'_>, rules: &CompiledRuleset, unit: &crate::knowledge::observation::OwnUnit, capital: TileId, taken: &[TileId]) -> Vec<TileId> {
    use crate::ai::planner_map::{find_path, step_points, Blockers, KnownMap, MapScratch, PathOutcome, SearchLimit, SearchStats};
    let class = kernel::world::stacking::stack_class(rules, unit.kind);
    let kinds = obs.view.terrain_kinds();
    let water = |tile: TileId| obs.view.tile(tile).snapshot().is_some_and(|s| kinds.is_water(s.terrain.0));
    let mut blocked: Vec<TileId> = obs.visible_foreign_units.iter().map(|u| u.tile).collect();
    blocked.extend(
        obs.own_units
            .iter()
            .filter(|u| u.handle != unit.handle && (water(u.tile) || kernel::world::stacking::stack_class(rules, u.kind) == class))
            .map(|u| u.tile),
    );
    blocked.extend(taken.iter().copied());
    blocked.retain(|t| *t != capital);
    let map = KnownMap::of_unit(&obs.view, unit.handle);
    let limit = SearchLimit { max_nodes: obs.view.tile_count() as u32 };
    let outcome = find_path(&map, unit.tile, capital, &Blockers::new(blocked), &mut MapScratch::default(), limit, &mut SearchStats::default());
    let PathOutcome::Found { tiles, .. } = outcome else {
        return Vec::new();
    };
    let mut left = u32::from(unit.moves_left);
    let mut steps = Vec::new();
    for leg in tiles.windows(2) {
        match step_points(&map, leg[0], leg[1]) {
            Some(points) if points <= left => left -= points,
            _ => break,
        }
        steps.push(leg[1]);
        if left == 0 {
            break;
        }
    }
    steps
}

/// Own civilians (not parts) standing in the capital march to free land
/// nearby (own or unclaimed), so a part can enter: one civilian per tile, and cities
/// training units over a full tile stack several there.
fn clear_capital(obs: &AIObservation<'_>, rules: &CompiledRuleset, capital: TileId, clearing: Clearing) -> Vec<Command> {
    let civilian = |kind: UnitKindId| !matches!(role_of(rules, kind), AiRole::Military | AiRole::Naval);
    let blockers = obs
        .own_units
        .iter()
        .filter(|u| u.tile == capital && civilian(u.kind) && !is_part(rules, u.kind) && u.moves_left > 0);
    let kinds = obs.view.terrain_kinds();
    let free = |tile: &TileId| {
        let known = obs.view.tile(*tile);
        let snapshot = known.snapshot();
        let own = snapshot.is_some_and(|s| s.owner.is_none_or(|o| o == obs.player) && !s.city);
        let water = clearing.water && snapshot.is_some_and(|s| kinds.is_water(s.terrain.0));
        let land = !water && snapshot.is_some_and(|s| kinds.enter_cost(s.terrain).is_some());
        // On water every land unit is embarked: one of any kind per tile.
        let room = if land {
            !obs.own_units.iter().any(|u| u.tile == *tile && civilian(u.kind))
        } else {
            water && !obs.own_units.iter().any(|u| u.tile == *tile)
        };
        own && room && !obs.visible_foreign_units.iter().any(|u| u.tile == *tile)
    };
    let dims = obs.view.dimensions();
    let mut tiles: Vec<TileId> = crate::math::hex_disc(dims, capital, clearing.radius).filter(free).collect();
    tiles.sort_by_key(|t| (dims.distance(capital, *t).unwrap_or(i32::MAX), t.0));
    let mut tiles = tiles.into_iter();
    let mut out = Vec::new();
    for blocker in blockers {
        let command = tiles
            .by_ref()
            .map(|tile| Command::MarchTo { unit: blocker.handle, destination: tile })
            .find(|command| obs.view.accepts(command));
        out.extend(command);
    }
    out
}

/// Civilians leaving the capital go at most this far.
const CLEARING_RADIUS: i32 = 3;

/// Where civilians leaving the capital may go: as far as `radius`, and to
/// water too (embarked, one unit a tile) with `water`.
#[derive(Debug, Clone, Copy)]
struct Clearing {
    radius: i32,
    water: bool,
}

impl Clearing {
    /// The game's AI: free land within `CLEARING_RADIUS`.
    const NEAR: Clearing = Clearing { radius: CLEARING_RADIUS, water: false };
    /// Variant `space-finish`: twice as far, water included — a crowded
    /// island capital kept two parts at its gate to the end (pace map
    /// archipelago, seed 2).
    const WIDE: Clearing = Clearing { radius: 2 * CLEARING_RADIUS, water: true };
}

/// The victory system's advisor (`ai::system`): parts move and install at
/// the start of the turn (their production is the city planner's).
#[derive(Debug, Clone, Copy, Default)]
pub struct VictoryAdvisor;

impl crate::ai::system::SystemAdvisor for VictoryAdvisor {
    fn start_of_turn(
        &self,
        input: &crate::ai::system::AdvisorInput<'_, '_>,
    ) -> Vec<crate::ai::system::Proposal> {
        crate::ai::combat::proposals(12, spaceship_orders_with(input.observation, input.rules, input.brief.space_finish))
    }
}
