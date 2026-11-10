//! `sharp` (`Variant::Sharp`, `TurnBrief::sharp`): the strategy net of
//! `standard` that chooses sharply and sees more. Nothing here runs for another variant.
//!
//! - **The choice**: the intent's softmaxes have explicit temperatures (`w::*_TEMPERATURE`), the
//!   price layer's rows give the held choice its share (`w::PRICE_PICK`) instead of the flat blend,
//!   and the leader's personality and the land move the style (`w::STYLE`): tall is the play of a
//!   leader who does not like new cities, of a scholar or a builder, of rich land without room. The
//!   neighbours count through the room they leave and, once met, the nearest one by land: at the
//!   lock (round 13 at Quick) a rival is known in 3 % of the traced reviews.
//! - **The features keep moving**: science, production and cities are read against what the
//!   rules' data makes available in the era and against the rivals' known cities (`features`), in
//!   place of the scales that saturate in the first third of the game; the nearest rival counts
//!   when it is reachable over known land (`survey`).
//! - **New inputs**: the way to the spaceship, ours (the beakers of its technologies, its parts)
//!   and the most advanced rival's (its era), the score's trend and a rival running away
//!   (`fe::SPACE_OWN` …), each wired into concepts and logits with its reason (`w::CONCEPTS`,
//!   `w::AIM`, `w::HORIZON`).
//! - **The cell's scale** (`cell::run` with `CellPrices::sharp`, `food_reference`): the run covers
//!   the horizon and no more, the food left in the store and the growth of the tail count as a share
//!   of a citizen (a food more is not a whole citizen or none), and the food multiplier compares the
//!   cell's price with the allocator's on one basis (a citizen worth its score and its yields), so it
//!   is the growth formula's verdict and not the yields the allocator leaves out.

use super::layers::{unit, ONE};
use super::weights::{fe, Act, Input, Neuron, Term};
use super::Milli;
use crate::ai::planner::TurnBrief;
use crate::ai::state::AiState;
use crate::ids::{PlayerId, TechId, TileId};
use crate::knowledge::observation::AIObservation;
use crate::knowledge::public::PlayerQueries;
use crate::rules::CompiledRuleset;
use crate::world::facts::TileFacts;
use crate::world::player::PlayerKind;

const fn f(i: usize, w: Milli, why: &'static str) -> Term {
    Term { input: Input::F(i), w, why }
}

const fn k(i: usize, w: Milli, why: &'static str) -> Term {
    Term { input: Input::K(i), w, why }
}

/// `sharp`'s weights, each with its reason (thousandths unless said otherwise).
pub mod w {
    use super::super::weights::fe::*;
    use super::super::weights::kc::*;
    use super::{f, k, Milli, Term};

    /// The temperatures of the intent's softmaxes: the logits are divided by them (1000 is
    /// `standard`'s softmax). Below 1000 the leading choice takes a larger share, so the prices,
    /// the cities and the horizon follow the choice and not a blend of all three.
    pub const STYLE_TEMPERATURE: Milli = 600;
    pub const AIM_TEMPERATURE: Milli = 600;
    /// The posture's logits are already apart by thousands (a war is +3000): its own scale.
    pub const POSTURE_TEMPERATURE: Milli = 1000;
    /// The share of each table of the price layer's rows the held choice takes (its one-hot), the
    /// rest the shares: at `standard`'s blend a science aim at 52 % priced science as a half-hearted
    /// one (1.15–1.34 for every seat).
    pub const PRICE_PICK: Milli = 500;

    /// A technology of our era in this many rounds is the science feature's 1 (the era's mean
    /// cost at the game's speed over our beakers): it moves with the era instead of saturating at
    /// 1.5 beakers a citizen by round 66.
    pub const SCIENCE_ROUNDS: i64 = 2;
    /// A regular building of our era in this many rounds a city is the production feature's 1.
    pub const PRODUCTION_ROUNDS: i64 = 4;
    /// The cities feature: ours over the rivals' known cities a nation, this ratio → 1 (level 0.25):
    /// the rivals' cities are known through the fog only, so ours were twice theirs in 44 % of the
    /// reviews at a ratio of 2 (tuning traces, step 4).
    pub const CITIES_RATIO: Milli = 4000;
    /// The share of the parts in the way to the spaceship (the technologies take the rest).
    pub const PARTS_SHARE: Milli = 200;
    /// The score's trend: a change of our score over the best rival's by 1 (thousandths) moves the
    /// feature this much: 0.1 of the ratio is the half range.
    pub const TREND_SCALE: Milli = 5000;
    /// A rival running away: its lead over the next best nation × this (50 % → 1).
    pub const RUNAWAY_SCALE: Milli = 2000;

    /// Terms added to the concepts before their activation: (concept, term).
    pub const CONCEPTS: &[(usize, Term)] = &[
        (SCIENCE_LEAD, f(SCIENCE, 300, "the era's technologies at our beakers (the era-relative scale): a fast research holds a lead")),
        (SCIENCE_LEAD, f(SPACE_OWN, 400, "the spaceship's technologies already ours: the race's end is near for us")),
        (SCIENCE_LEAD, f(RUNAWAY, -300, "a rival running away with the score is ahead in the race too")),
        (SCIENCE_LEAD, f(SCORE_TREND, 200, "a score gaining on the leader is a lead in the making (0.5 is steady)")),
        (WONDER_CHANCE, f(PRODUCTION, 400, "a city that builds its era's buildings fast wins its wonder races")),
    ];

    /// Terms added to the style logits: (style, term). The leader's personality from the rules
    /// (0…10, 5 neutral), the land at the lock.
    pub const STYLE: &[(usize, Term)] = &[
        (0, f(T_EXPANSION, -1500, "a leader who likes new cities does not stay small")),
        (0, f(T_SCIENCE, 600, "a scholar keeps a compact empire whose research costs less a city")),
        (0, f(T_CULTURE, 600, "a cultural leader: few cities make its policies cheap")),
        (0, f(T_WONDERS, 400, "a builder of wonders needs a great capital, not many towns")),
        (0, k(GOOD_LAND, 600, "rich land around the capital grows few big cities")),
        (1, f(HAPPY, -600, "at the lock every small empire is content: its happiness is no reason to go wide (its luxuries are)")),
        (1, f(T_EXPANSION, 400, "a leader who likes new cities")),
    ];
    /// The bias added to tall: the terms above sum to about −50 for a neutral leader on middling
    /// land; with this bias a third of the traced seats play tall (the replica on the step-4
    /// traces: korea, india, germany, venice, the netherlands, sweden most), the expanders wide.
    pub const TALL_BIAS: Milli = 400;

    /// Terms added to the aim logits: (aim, term). The traits against each other, so that a
    /// cultural, a diplomatic or a warlike leader's own victory can lead the science one.
    pub const AIM: &[(usize, Term)] = &[
        (0, f(SPACE_OWN, 600, "the spaceship within reach: the science victory is ours to finish")),
        (0, f(SPACE_RIVAL, -400, "a rival near the spaceship's era: our ship may come too late")),
        (1, f(T_CULTURE, 1600, "a cultural leader's own victory")),
        (1, f(T_SCIENCE, -600, "before a scientific leader's")),
        (1, f(CITIES, -600, "an empire wider than its rivals pays more culture a policy and more tourism a city")),
        (3, f(T_DIPLOMACY, 200, "a diplomatic leader's own victory (its city-state play already counts the trait)")),
        (3, f(T_SCIENCE, -400, "before a scientific leader's")),
        (3, f(RUNAWAY, 200, "a runaway leader is stopped by the votes of the others")),
        (2, f(RUNAWAY, 300, "or by taking its cities")),
        (2, k(WARLIKE, 600, "a warlike leader's own victory")),
    ];

    /// Terms of the horizon (Standard rounds per unit of the feature).
    pub const HORIZON: &[(usize, Milli, &str)] = &[(SPACE_RIVAL, -30, "a rival near the spaceship may end the game: plans pay sooner")];

    /// The features a locked style takes from the turn besides `weights::STYLE_EVENTS`: the
    /// memory keeps the lock's first `fe::BASE_N` features only, and these change with the race,
    /// not with the empire's growth.
    pub const STYLE_EVENTS: [usize; 4] = [SPACE_OWN, SPACE_RIVAL, SCORE_TREND, RUNAWAY];

    /// `sharp`'s weights, counted.
    pub fn count() -> usize {
        CONCEPTS.len() + STYLE.len() + 1 + AIM.len() + HORIZON.len() + 4 + 6
    }
}

/// What `sharp` keeps between turns beyond the net's memory (`Memory::sharp`): our score over the
/// best rival's at the last survey and the trend it gave.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Memory {
    pub ratio: Milli,
    pub trend: Milli,
}

/// A neuron with `sharp`'s terms of its index added before the activation.
pub fn fire_with(neuron: &Neuron, extra: &[(usize, Term)], at: usize, features: &[Milli], concepts: &[Milli]) -> Milli {
    let x = |input: Input| match input {
        Input::F(i) => features[i],
        Input::K(i) => concepts[i],
    };
    let mut sum = neuron.bias;
    for term in neuron.terms {
        sum += term.w * x(term.input) / ONE;
    }
    for (_, term) in extra.iter().filter(|(i, _)| *i == at) {
        sum += term.w * x(term.input) / ONE;
    }
    match neuron.act {
        Act::Unit => unit(sum),
        Act::Linear => sum,
    }
}

/// Logits divided by a temperature (thousandths).
pub fn tempered<const N: usize>(logits: &[Milli; N], temperature: Milli) -> [Milli; N] {
    logits.map(|l| l * ONE / temperature.max(1))
}

/// The rows of the price layer: each table's shares with the held choice's one-hot by
/// `w::PRICE_PICK`, the two gates as they are.
pub fn rows(intent: &super::Intent) -> [Milli; 13] {
    let mut r = super::rows(intent);
    let mix = |r: &mut [Milli], pick: usize| {
        for (i, v) in r.iter_mut().enumerate() {
            let one = if i == pick { ONE } else { 0 };
            *v = (*v * (ONE - w::PRICE_PICK) + one * w::PRICE_PICK) / ONE;
        }
    };
    mix(&mut r[0..3], intent.style_pick);
    mix(&mut r[3..7], intent.aim_pick);
    mix(&mut r[7..11], intent.posture_pick);
    r
}

// ---- the cell's scale ----

/// The allocator's food price on the cell's scale: the allocator prices a food a turn by the
/// citizens it grows to the end times a citizen's score (`value::Prices::food`), the cell by the
/// citizens it grows times their score and their yields (`cell::run`); this is the allocator's
/// price with its citizen worth what the cell's is — the score and the yields one citizen more
/// adds to each city now (the cell's own tile choice: `marginal`), for half the rounds left (the
/// citizens a food buys come through them). The cell's food price over it is the growth
/// formula's verdict alone (H, the mood, the store, the thresholds). Over the allocator's own
/// price it was 4.7 at the median and above 4.00 in 57 % of the reviews once the store counted;
/// with the mean citizen's yields (the centre's and the buildings' too) 0.46 and below 0.70 in 72 %
/// (tuning traces, step 2).
pub fn food_reference(model: &super::model::Model, prices: &super::cell::CellPrices, base: &super::Base) -> i64 {
    let half = prices.rounds_left / 2;
    let yields: i64 = (1..6).map(|s| marginal(model, prices)[s] * half / 100 * prices.unit[s] / 1000).sum();
    base.food * (prices.citizen + yields.max(0)) / base.citizen.max(1)
}

/// The yields (hundredths a turn) one citizen more adds to a city now, the mean over the cities:
/// the cell's first step with every city a citizen bigger, against it as it is.
pub fn marginal(model: &super::model::Model, prices: &super::cell::CellPrices) -> [i64; 6] {
    let mut grown = model.clone();
    for c in grown.cities.iter_mut() {
        c.population += 1;
    }
    let now = super::cell::run(model, &super::cell::Plan::new(model), prices, 1);
    let more = super::cell::run(&grown, &super::cell::Plan::new(&grown), prices, 1);
    let n = model.cities.len().max(1) as i64;
    let mut out = [0; 6];
    for (s, v) in out.iter_mut().enumerate() {
        *v = more.first.iter().zip(&now.first).map(|(a, b)| a[s] - b[s]).sum::<i64>() / n;
    }
    out
}

// ---- the features ----

/// The latest content era of a technology (its order).
fn tech_era(rules: &CompiledRuleset, t: TechId) -> Option<usize> {
    rules.technologies.get(usize::from(t.0)).and_then(|d| d.content_era).map(|e| super::features::era_index(rules, e))
}

/// The mean cost of the technologies of `era` (the rules' cost at the game's speed); the nearest
/// earlier era's when the era has none.
pub fn era_tech_cost(rules: &CompiledRuleset, era: usize, science_percent: i64) -> i64 {
    for e in (0..=era).rev() {
        let costs: Vec<i64> = (0..rules.technologies.len()).filter(|i| tech_era(rules, TechId(*i as u16)) == Some(e)).map(|i| i64::from(rules.technologies[i].cost)).collect();
        if !costs.is_empty() {
            return (costs.iter().sum::<i64>() / costs.len() as i64 * science_percent / 100).max(1);
        }
    }
    1
}

/// The cost of a regular building of `era` (no wonder, none a nation's own) at the game's speed:
/// the mean of those a technology of the era opens, at least the dearest earlier era's (the last
/// eras open a few cheap ones: 500, 480, 330 hammers in the classic rules' last three).
pub fn era_building_cost(rules: &CompiledRuleset, era: usize, production_percent: i64) -> i64 {
    let mut cost = 1;
    for e in 0..=era {
        let costs: Vec<i64> = rules
            .buildings
            .iter()
            .filter(|b| !b.wonder && !b.unbuildable && b.nation.is_none() && b.kind == crate::rules::content::BuildingKind::Regular && b.cost > 0)
            .filter(|b| b.required_tech.and_then(|t| tech_era(rules, t)) == Some(e))
            .map(|b| i64::from(b.cost))
            .collect();
        if !costs.is_empty() {
            cost = cost.max(costs.iter().sum::<i64>() / costs.len() as i64 * production_percent / 100);
        }
    }
    cost.max(1)
}

/// The technologies the spaceship needs: those that open a project and those its parts need.
pub fn space_techs(rules: &CompiledRuleset) -> Vec<TechId> {
    let mut out: Vec<TechId> = (0..rules.technologies.len()).filter(|i| !rules.technologies[*i].unlocks_projects.is_empty()).map(|i| TechId(i as u16)).collect();
    for (kind, _, _) in crate::ai::victory::part_kinds(rules) {
        if let Some(t) = rules.units.get(usize::from(kind.0)).and_then(|d| d.required_tech) {
            out.push(t);
        }
    }
    out.sort_by_key(|t| t.0);
    out.dedup();
    out
}

/// The beakers (the rules' costs) of every technology missing on the ways to `targets`.
fn missing_beakers(rules: &CompiledRuleset, known: &[TechId], targets: &[TechId]) -> i64 {
    let mut planned: Vec<TechId> = Vec::new();
    for t in targets {
        let way = super::goals::path(rules, known, &planned, *t);
        planned.extend(way);
    }
    super::goals::beakers(rules, &planned, 100)
}

/// Our way to the spaceship (thousandths): the share of its technologies' beakers known, and of
/// its parts built.
pub fn space_own(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Milli {
    let space = space_techs(rules);
    if space.is_empty() {
        return 0;
    }
    let all = missing_beakers(rules, &[], &space).max(1);
    let left = missing_beakers(rules, &obs.economy.techs, &space);
    let techs = unit((all - left) * ONE / all);
    let (mut done, mut needed) = (0i64, 0i64);
    for (_, project, n) in crate::ai::victory::part_kinds(rules) {
        needed += i64::from(n);
        done += i64::from(obs.economy.projects.get(usize::from(project.0)).copied().unwrap_or(0).min(n));
    }
    let parts = if needed > 0 { done * ONE / needed } else { 0 };
    (techs * (ONE - w::PARTS_SHARE) + parts * w::PARTS_SHARE) / ONE
}

/// The spaceship's era: the latest era of its technologies.
pub fn space_era(rules: &CompiledRuleset) -> usize {
    space_techs(rules).iter().filter_map(|t| tech_era(rules, *t)).max().unwrap_or(0)
}

/// The nearest city of a rival nation reachable from `from` over known land (hex distance), by a
/// walk over the known land tiles.
pub fn nearest_by_land(obs: &AIObservation<'_>, from: TileId, targets: &[TileId]) -> Option<i32> {
    let dims = obs.view.dimensions();
    let n = obs.view.tile_count();
    let mut seen = vec![false; n];
    let mut queue = std::collections::VecDeque::new();
    let land = |t: TileId| obs.view.terrain(t).is_some() && !obs.view.is_water(t);
    if usize::try_from(from.0).map_or(true, |i| i >= n) {
        return None;
    }
    seen[from.0 as usize] = true;
    queue.push_back(from);
    let mut best: Option<i32> = None;
    while let Some(t) = queue.pop_front() {
        if targets.contains(&t) {
            let d = dims.distance(from, t).unwrap_or(i32::MAX);
            best = Some(best.map_or(d, |b| b.min(d)));
        }
        for next in dims.neighbors(t) {
            let i = next.0 as usize;
            if i < n && !seen[i] && (land(next) || targets.contains(&next)) {
                seen[i] = true;
                queue.push_back(next);
            }
        }
    }
    best
}

/// `sharp`'s survey: the nearest rival nation counts by its cities reachable over known land (a
/// rival across the water takes no site and opens no early strike).
pub fn survey(obs: &AIObservation<'_>, state: &AiState, survey: &mut super::features::Survey) {
    let Some(capital) = super::features::capital_tile(obs) else { return };
    let is_major = |p: PlayerId| obs.relations.iter().any(|r| r.other == p && r.kind == PlayerKind::Major);
    let rivals: Vec<TileId> = state.seen_cities().iter().filter(|c| is_major(c.owner)).map(|c| c.tile).collect();
    let near = nearest_by_land(obs, capital, &rivals).map_or(0, |d| super::layers::ramp(18 - i64::from(d), 0, 11));
    if let Some(at) = super::features::SURVEY_FEATURES.iter().position(|f| *f == fe::RIVAL_NEAR) {
        survey.values[at] = near;
    }
}

/// `sharp`'s features of a turn: science, production and cities on scales that keep moving (in
/// place of `standard`'s), the ways to the spaceship, the score's trend (kept in `memory`, taken
/// anew when `survey_due`) and a rival running away.
pub fn features(obs: &AIObservation<'_>, brief: &TurnBrief, rules: &CompiledRuleset, state: &AiState, memory: &mut Option<Memory>, survey_due: bool, out: &mut [Milli; fe::N]) {
    let era = super::features::era_of(rules, &obs.economy.techs);
    let cities = obs.own_cities.len() as i64;
    // Science: a technology of our era in `SCIENCE_ROUNDS` rounds is 1.
    let tech = era_tech_cost(rules, era, i64::from(obs.speed.science_percent.max(1)));
    out[fe::SCIENCE] = unit(i64::from(brief.science.max(0)) * w::SCIENCE_ROUNDS * ONE / tech);
    // Production: a regular building of our era in `PRODUCTION_ROUNDS` rounds a city is 1.
    let building = era_building_cost(rules, era, i64::from(obs.speed.production_percent.max(1)));
    let production: i64 = obs.own_cities.iter().map(|c| i64::from(c.income.production.max(0))).sum();
    out[fe::PRODUCTION] = unit(production * w::PRODUCTION_ROUNDS * ONE / (cities.max(1) * building));
    // Cities: ours over the rivals' known cities a nation (level 0.5).
    let is_major = |p: PlayerId| obs.relations.iter().any(|r| r.other == p && r.kind == PlayerKind::Major);
    let mut owners: Vec<PlayerId> = state.seen_cities().iter().filter(|c| is_major(c.owner)).map(|c| c.owner).collect();
    let known = owners.len() as i64;
    owners.sort_by_key(|p| p.0);
    owners.dedup();
    out[fe::CITIES] = if owners.is_empty() { ONE / 2 } else { unit(cities * owners.len() as i64 * ONE * ONE / (known.max(1) * w::CITIES_RATIO)) };
    // The ways to the spaceship.
    out[fe::SPACE_OWN] = space_own(obs, rules);
    let space = space_era(rules).max(1) as i64;
    out[fe::SPACE_RIVAL] = obs.view.met_eras().iter().map(|(_, e)| i64::from(*e)).max().map_or(0, |e| unit(e * ONE / space));
    // The score: the trend of ours over the best rival's, and a rival running away.
    let race = obs.view.race();
    let ours = race.own_score();
    let mut rivals: Vec<i64> = race.scores.iter().skip(1).map(|(_, s)| *s).collect();
    rivals.sort_unstable_by(|a, b| b.cmp(a));
    let ratio = rivals.first().map_or(ONE, |best| ours * ONE / (*best).max(1));
    let trend = match memory {
        Some(m) if !survey_due => m.trend,
        Some(m) => unit(ONE / 2 + (ratio - m.ratio) * w::TREND_SCALE / ONE),
        None => ONE / 2,
    };
    if survey_due || memory.is_none() {
        *memory = Some(Memory { ratio, trend });
    }
    out[fe::SCORE_TREND] = trend;
    out[fe::RUNAWAY] = match rivals.first() {
        Some(best) => {
            let next = rivals.get(1).copied().unwrap_or(0).max(ours).max(1);
            unit((best - next).max(0) * w::RUNAWAY_SCALE / next)
        }
        None => 0,
    };
}
