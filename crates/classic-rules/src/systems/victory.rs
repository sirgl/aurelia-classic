//! Victories and the end of the game (classic/docs/classic/rules.md): domination
//! (every original capital), science (the spaceship parts installed in the
//! capital), culture (influential on everyone: the tourism of track B),
//! diplomacy (the World Leader of the United Nations), time (the best
//! score at the last round); the score formula; the spaceship.
//!
//! Domination is tested at every capture; the other victories once a turn
//! (the `Victory` step), never in the first ten rounds. Several winners in
//! one test: the victory RNG picks one.

use crate::command::CommandError;
use crate::ids::{PlayerId, TileId, UnitHandle};
use crate::rules::content::{BuildingKind, ProjectId};
use crate::rules::CompiledRuleset;
use crate::classic::ClassicChange;
use crate::transaction::transaction::WorldTransaction;
use crate::world::state::AuthWorld;
use crate::world::victory::{Outcome, VictoryKind};

/// Rounds that must pass before victory conditions are tested. Own value
/// (no Unciv equivalent: Unciv tests every turn).
pub const MIN_ROUNDS_TO_TEST: u32 = 10;
/// Score points per city, citizen, land tile, wonder and technology.
/// Unciv: logic/civilization/Civilization.kt (`calculateScoreBreakdown`:
/// city 10, land tile 1, technology 4) and models/ModConstants.kt
/// (citizen 3, wonder 40); equal.
pub const SCORE_CITY: i64 = 10;
/// Unciv: Civilization.kt and ModConstants.kt (score), equal.
pub const SCORE_POPULATION: i64 = 3;
/// Unciv: Civilization.kt and ModConstants.kt (score), equal.
pub const SCORE_LAND: i64 = 1;
/// Unciv: Civilization.kt and ModConstants.kt (score), equal.
pub const SCORE_WONDER: i64 = 40;
/// Unciv: Civilization.kt and ModConstants.kt (score), equal.
pub const SCORE_TECH: i64 = 4;
/// Plots of a Standard map (the reference size of the map score
/// modifier: 80 × 52). Same shape as Unciv: logic/civilization/Civilization.kt
/// (reference / tiles, above 1 damped to (m - 1) / 3 + 1), whose reference
/// 1276 is the tile count of its own Medium map; ours is the tile count of
/// our Standard map, so the modifier stays 1 on the reference size. Own
/// value (Unciv's 1276 would scale every map of ours by about 0.3).
pub const STANDARD_MAP_PLOTS: i64 = 4160;

/// Map score modifier, percent: smaller maps score more per city and tile.
pub fn map_score_mod(auth: &AuthWorld) -> i64 {
    let plots = (auth.map().tile_count() as i64).max(1);
    let m = 100 * STANDARD_MAP_PLOTS / plots;
    if m > 100 {
        (m - 100) / 3 + 100
    } else {
        m
    }
}

/// The score without its policy, great work, religion and
/// future-technology terms (4 per adopted policy, 4 per great work, 20 per
/// belief of the own religion plus 1 per city following it, 10 per future
/// technology; classic/docs/classic/rules.md). Policies, religion and great works
/// exist in the game, but the score of the time victory counts only the five
/// terms below (cities, citizens, land, wonders, technologies): the same
/// terms as Unciv: logic/civilization/Civilization.kt, which adds only 10 per
/// future technology (this game has no repeatable technology). The AI's
/// allocator values them as well (classic/docs/ai-allocator.md, "Score").
pub fn score(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId) -> i64 {
    let cities: Vec<_> = auth
        .indexes()
        .cities_of(player)
        .iter()
        .filter_map(|c| auth.cities().get(*c))
        .collect();
    let map_mod = map_score_mod(auth);
    let population: i64 = cities.iter().map(|c| i64::from(c.population)).sum();
    let land = (0..auth.map().tile_count() as u32)
        .filter(|t| auth.map().owner(TileId(*t)).flatten() == Some(player))
        .filter(|t| !crate::navigation::terrain::is_water(auth.map(), TileId(*t)))
        .count() as i64;
    let wonders = cities
        .iter()
        .flat_map(|c| c.buildings.iter())
        .filter(|b| {
            rules
                .buildings
                .get(b.0 as usize)
                .is_some_and(|d| d.kind == BuildingKind::WorldWonder)
        })
        .count() as i64;
    let techs = auth.players().techs_of(player).len() as i64;
    cities.len() as i64 * SCORE_CITY * map_mod / 100
        + population * SCORE_POPULATION * map_mod / 100
        + land * SCORE_LAND * map_mod / 100
        + wonders * SCORE_WONDER
        + techs * SCORE_TECH
}

/// The spaceship projects and how many of each the victory needs (the
/// projects of the space race victory; their team limit is the
/// threshold).
pub fn spaceship_parts(rules: &CompiledRuleset) -> Vec<(ProjectId, u16)> {
    rules
        .module.projects
        .iter()
        .enumerate()
        .filter(|(_, p)| p.unbuildable && p.victory.as_deref() == Some("VICTORY_SPACE_RACE"))
        .map(|(i, p)| (ProjectId(i as u16), p.max_team_instances.max(1)))
        .collect()
}

/// Original capitals held by alive civilizations, and how many `player`
/// holds (§8.2).
pub fn capitals_held(auth: &AuthWorld, player: PlayerId) -> (usize, usize) {
    let mut total = 0;
    let mut mine = 0;
    for (_, city) in auth.cities().iter() {
        let counted = city.original_capital
            && auth.players().is_major(city.original_owner)
            && auth.players().is_major(city.owner)
            && auth.players().is_alive(city.owner);
        if counted {
            total += 1;
            mine += usize::from(city.owner == player);
        }
    }
    (total, mine)
}

/// Original capitals of other civilizations `player` holds (a lone
/// civilization whose rivals never founded a city does not dominate).
pub fn foreign_capitals_held(auth: &AuthWorld, player: PlayerId) -> usize {
    auth.cities()
        .iter()
        .filter(|(_, c)| c.original_capital && c.owner == player && c.original_owner != player)
        .filter(|(_, c)| auth.players().is_major(c.original_owner))
        .count()
}

/// Is the player influential on every other civilization (cultural
/// victory; tourism and influence are track B's `derived::tourism`)?
pub fn influential_on_all(auth: &AuthWorld, _rules: &CompiledRuleset, player: PlayerId) -> bool {
    crate::derived::tourism::has_cultural_victory(auth, player)
}

fn alive_majors(auth: &AuthWorld) -> Vec<PlayerId> {
    crate::systems::city_states::majors(auth)
}

/// Every civilization that meets a victory now (one kind).
fn contenders(auth: &AuthWorld, rules: &CompiledRuleset, kind: VictoryKind) -> Vec<PlayerId> {
    let majors = alive_majors(auth);
    match kind {
        VictoryKind::Domination => majors
            .into_iter()
            .filter(|p| {
                let (total, mine) = capitals_held(auth, *p);
                total > 0 && mine == total && foreign_capitals_held(auth, *p) > 0
            })
            .collect(),
        VictoryKind::Science => {
            let parts = spaceship_parts(rules);
            majors
                .into_iter()
                .filter(|p| {
                    !parts.is_empty()
                        && parts
                            .iter()
                            .all(|(id, n)| auth.module().victory().project_count(*p, *id) >= *n)
                })
                .collect()
        }
        VictoryKind::Cultural => majors
            .into_iter()
            .filter(|p| influential_on_all(auth, rules, *p))
            .collect(),
        VictoryKind::Diplomatic => auth
            .module()
            .victory()
            .world_leader()
            .into_iter()
            .filter(|p| majors.contains(p))
            .collect(),
        VictoryKind::Time => time_leader(auth, rules).into_iter().collect(),
    }
}

/// At the last round: the civilization whose score is strictly the best.
fn time_leader(auth: &AuthWorld, rules: &CompiledRuleset) -> Option<PlayerId> {
    if auth.turn().round < auth.module().victory().options().max_rounds {
        return None;
    }
    let mut scores: Vec<(i64, PlayerId)> = alive_majors(auth)
        .into_iter()
        .map(|p| (score(auth, rules, p), p))
        .collect();
    scores.sort_by_key(|(s, p)| (std::cmp::Reverse(*s), p.0));
    match scores.as_slice() {
        [(best, p), (second, _), ..] if best > second => Some(*p),
        [(_, p)] => Some(*p),
        _ => None,
    }
}

impl<'a> WorldTransaction<'a> {
    /// Domination after a capture (no ten-round wait).
    pub(crate) fn test_domination(&mut self, rules: &CompiledRuleset) {
        self.test_victory_kinds(&[VictoryKind::Domination], rules);
    }

    /// The `Victory` turn step: every enabled victory after round ten; the
    /// last civilization standing wins by domination.
    pub(crate) fn test_victories(&mut self, rules: &CompiledRuleset) {
        if self.world.turn.round <= MIN_ROUNDS_TO_TEST {
            return;
        }
        self.test_victory_kinds(&VictoryKind::ALL, rules);
        let majors = alive_majors(self.world);
        if let [last] = majors.as_slice() {
            let enabled = self.world.module.victory.options.enabled(VictoryKind::Domination);
            if enabled && self.world.module.victory.outcome.is_none() && self.world.players.count() > 1 {
                self.set_outcome(*last, VictoryKind::Domination);
            }
        }
    }

    fn test_victory_kinds(&mut self, kinds: &[VictoryKind], rules: &CompiledRuleset) {
        if self.world.module.victory.outcome.is_some() {
            return;
        }
        let options = self.world.module.victory.options;
        for kind in kinds.iter().copied().filter(|k| options.enabled(*k)) {
            let winners = contenders(self.world, rules, kind);
            if winners.is_empty() {
                continue;
            }
            let pick = self.world.rng.events.below(winners.len() as u64) as usize;
            self.set_outcome(winners[pick], kind);
            return;
        }
    }

    fn set_outcome(&mut self, winner: PlayerId, kind: VictoryKind) {
        let round = self.world.turn.round;
        self.world.module.victory.outcome = Some(Outcome {
            winner,
            kind,
            round,
        });
        self.journal
            .module_changes
            .push(ClassicChange::RelationsChanged { player: winner });
    }

    /// The World Leader was elected (United Nations).
    pub(crate) fn test_diplomatic_victory(&mut self, rules: &CompiledRuleset) {
        self.test_victory_kinds(&[VictoryKind::Diplomatic], rules);
    }

    /// A project finished in a city of `player`.
    pub(crate) fn complete_project(&mut self, player: PlayerId, project: ProjectId) {
        self.world.module.victory.add_project(player, project);
        self.relations_changed(player);
    }

    /// A spaceship part in its owner's capital becomes part of the ship.
    pub(crate) fn install_spaceship_part(
        &mut self,
        unit: UnitHandle,
        project: ProjectId,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        let core = self.destroy_unit(unit)?;
        self.complete_project(core.owner, project);
        self.test_victory_kinds(&[VictoryKind::Science], rules);
        Ok(())
    }
}

/// Why the unit cannot be installed now (None: it can): a spaceship part
/// standing in its owner's capital.
pub fn spaceship_problem(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    unit: UnitHandle,
) -> Result<ProjectId, &'static str> {
    let core = auth.units().get(unit).ok_or("unknown unit")?.core;
    let project = rules
        .units
        .get(core.kind.0 as usize)
        .and_then(|d| d.spaceship_project)
        .ok_or("not a spaceship part")?;
    let capital = crate::combat::city::capital_of(auth, rules, core.owner)
        .and_then(|c| auth.cities().get(c))
        .map(|c| c.tile);
    if capital != Some(core.tile) {
        return Err("only in the capital");
    }
    Ok(project)
}

/// Why `player` may not start `project` now (Ok: it may): a buildable
/// project of a civilization, its technology known, its prerequisites
/// done, fewer than its team limit done or queued here.
/// The player may build nuclear weapons: it completed a project that
/// allows them (the Manhattan Project). Unverified against a public source:
/// the project's text says "your cities", so one civilization's Manhattan
/// Project does not arm the others.
pub fn nukes_allowed(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId) -> bool {
    rules.module.projects.iter().enumerate().any(|(i, p)| {
        p.allows_nukes && auth.module().victory().project_count(player, crate::rules::content::ProjectId(i as u16)) > 0
    })
}

pub fn project_problem(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    player: PlayerId,
    project: ProjectId,
    queue: &[crate::ids::ProductionItem],
) -> Result<(), &'static str> {
    let def = rules
        .module.projects
        .get(project.0 as usize)
        .ok_or("unknown project")?;
    if !auth.players().is_major(player) {
        return Err("only nations build projects");
    }
    if def
        .required_tech
        .is_some_and(|t| !auth.players().techs_of(player).contains(&t))
    {
        return Err("project locked");
    }
    let victory = auth.module().victory();
    if def
        .prerequisite_projects
        .iter()
        .any(|(p, n)| victory.project_count(player, *p) < *n)
    {
        return Err("needs another project first");
    }
    let queued = queue.contains(&crate::ids::ProductionItem::Project(project));
    let limit = if def.max_team_instances == 0 {
        u16::MAX
    } else {
        def.max_team_instances
    };
    if queued || victory.project_count(player, project) >= limit {
        return Err("project already done");
    }
    Ok(())
}
