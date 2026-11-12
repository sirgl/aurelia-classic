//! The public standing screens of a player: the demographics rows (its value, its rank and the range
//! of the living nations) and the progress of the player on each kind of victory. ONE computation
//! for the two readers: the interface's overview and victory screens (`classic-ui`) and the AI's
//! observation (`AIObservation::public`): what a human reads on the screens is what the AI reads.
//! Only the player's own figure and the range of the others are in the rows; no rival's value is.

use crate::derived::income::player_income;
use crate::derived::DerivedWorld;
use crate::ids::{PlayerId, TileId};
use crate::rules::content::ProjectId;
use crate::rules::CompiledRuleset;
use crate::world::state::AuthWorld;
use crate::world::victory::{capitals_held, foreign_capitals_held, spaceship_parts, VictoryKind};

/// What a demographics row measures, in the order of the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Measure {
    Population,
    Crop,
    Goods,
    Gnp,
    Literacy,
    Land,
    Soldiers,
}

impl Measure {
    pub const ALL: [Measure; 7] = [Measure::Population, Measure::Crop, Measure::Goods, Measure::Gnp, Measure::Literacy, Measure::Land, Measure::Soldiers];
}

/// A demographics row: the player's value, its rank (1 = best) and the range of the living nations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Demographic {
    pub measure: Measure,
    pub value: i64,
    pub rank: usize,
    pub best: i64,
    pub average: i64,
    pub worst: i64,
}

/// The demographics of `me`: one row per measure, ranked among every living player of the game
/// (the screen counts them all, met or not).
pub fn demographics(auth: &AuthWorld, rules: &CompiledRuleset, derived: &DerivedWorld, me: PlayerId) -> Vec<Demographic> {
    let players: Vec<PlayerId> = (0..auth.players().count() as u8).map(PlayerId).filter(|p| auth.players().is_alive(*p)).collect();
    let mut land = vec![0i64; auth.players().count()];
    for t in 0..auth.map().tile_count() as u32 {
        if let Some(Some(owner)) = auth.map().owner(TileId(t)) {
            if let Some(count) = land.get_mut(owner.0 as usize) {
                *count += 1;
            }
        }
    }
    let techs = rules.technologies.len().max(1) as i64;
    let yields = &derived.yields;
    let value = |measure: Measure, p: PlayerId| -> i64 {
        match measure {
            Measure::Population => population(auth, p),
            Measure::Crop => yields.of_player(p).food as i64,
            Measure::Goods => yields.of_player(p).production as i64,
            Measure::Gnp => player_income(auth, rules, p).gold as i64,
            Measure::Literacy => auth.players().techs_of(p).len() as i64 * 100 / techs,
            Measure::Land => land.get(p.0 as usize).copied().unwrap_or(0),
            Measure::Soldiers => soldiers(auth, rules, p),
        }
    };
    Measure::ALL
        .iter()
        .map(|&measure| {
            let values: Vec<(PlayerId, i64)> = players.iter().map(|p| (*p, value(measure, *p))).collect();
            let mine = values.iter().find(|(p, _)| *p == me).map(|(_, v)| *v).unwrap_or(0);
            let rank = 1 + values.iter().filter(|(_, v)| *v > mine).count();
            let best = values.iter().map(|(_, v)| *v).max().unwrap_or(0);
            let worst = values.iter().map(|(_, v)| *v).min().unwrap_or(0);
            let average = values.iter().map(|(_, v)| *v).sum::<i64>() / values.len().max(1) as i64;
            Demographic { measure, value: mine, rank, best, average, worst }
        })
        .collect()
}

fn population(auth: &AuthWorld, p: PlayerId) -> i64 {
    auth.indexes().cities_of(p).iter().filter_map(|c| auth.cities().get(*c)).map(|c| c.population as i64).sum()
}

fn soldiers(auth: &AuthWorld, rules: &CompiledRuleset, p: PlayerId) -> i64 {
    auth.indexes().units_of(p).iter().filter_map(|u| auth.units().get(*u)).filter(|u| !rules.is_civilian(u.core.kind)).count() as i64
}

/// The player's progress on one kind of victory, as the victory screen's row counts it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VictoryProgress {
    pub kind: VictoryKind,
    pub enabled: bool,
    /// Counts can exist before the path is active (a lone capital, delegates before the United
    /// Nations): the bar is drawn only when this is set.
    pub show_progress: bool,
    pub have: u32,
    pub need: u32,
    /// Domination: foreign capitals held. Science: 1 when the Apollo Program is built (it is the last
    /// unit of `have` and `need`). Others: 0.
    pub extra: u32,
}

/// The Apollo-like projects the spaceship's parts need before they may be built (their units'
/// `required_project`).
fn prerequisite_projects(rules: &CompiledRuleset, parts: &[(ProjectId, u16)]) -> Vec<ProjectId> {
    let mut out: Vec<ProjectId> = rules
        .units
        .iter()
        .filter(|d| d.spaceship_project.is_some_and(|p| parts.iter().any(|(q, _)| *q == p)))
        .filter_map(|d| d.required_project)
        .collect();
    out.sort_by_key(|p| p.0);
    out.dedup();
    out
}

/// The victory screen's rows of `me`: domination, science, culture, diplomacy, time.
pub fn victory_progress(auth: &AuthWorld, rules: &CompiledRuleset, me: PlayerId) -> Vec<VictoryProgress> {
    let options = auth.module().victory().options();
    let (total, held) = capitals_held(auth, me);
    let foreign = foreign_capitals_held(auth, me);
    let domination = VictoryProgress {
        kind: VictoryKind::Domination,
        enabled: options.enabled(VictoryKind::Domination),
        show_progress: total > held || foreign > 0,
        have: held as u32,
        need: total as u32,
        extra: foreign as u32,
    };
    let parts = spaceship_parts(rules);
    let (mut have, mut need) = (0u32, 0u32);
    for (project, count) in &parts {
        have += u32::from(auth.module().victory().project_count(me, *project).min(*count));
        need += u32::from(*count);
    }
    let prerequisites = prerequisite_projects(rules, &parts);
    let built = !prerequisites.is_empty() && prerequisites.iter().all(|p| auth.module().victory().project_count(me, *p) > 0);
    let science = VictoryProgress {
        kind: VictoryKind::Science,
        enabled: options.enabled(VictoryKind::Science),
        show_progress: true,
        have: have + u32::from(built),
        need: need + 1,
        extra: u32::from(built),
    };
    let culture = VictoryProgress {
        kind: VictoryKind::Cultural,
        enabled: options.enabled(VictoryKind::Cultural),
        show_progress: true,
        have: crate::derived::tourism::influential_on(auth, me),
        need: crate::derived::tourism::nations_needed_for_cultural_victory(auth),
        extra: 0,
    };
    let congress = auth.module().congress();
    let diplomacy = VictoryProgress {
        kind: VictoryKind::Diplomatic,
        enabled: options.enabled(VictoryKind::Diplomatic),
        show_progress: congress.united_nations() || auth.module().victory().world_leader().is_some(),
        have: u32::from(crate::systems::congress::delegates(auth, rules, me)),
        need: u32::from(crate::systems::congress::votes_needed(auth)),
        extra: 0,
    };
    let last = options.max_rounds + 1;
    let time = VictoryProgress { kind: VictoryKind::Time, enabled: options.enabled(VictoryKind::Time), show_progress: true, have: auth.turn().round + 1, need: last, extra: 0 };
    vec![domination, science, culture, diplomacy, time]
}
