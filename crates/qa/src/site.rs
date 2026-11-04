//! The site probe: a dataset for a learned model of a city site's value
//! (`classic_rules::ai::site`). The probe changes nothing of the game: the
//! wrapper delegates every call of the AI contract to the mind it wraps and
//! only reads what passes through; the world hash of a probed game equals
//! the hash of the plain one.
//!
//! One row per city the AI founded:
//! - when a unit decision is `Command::FoundCity` (`decide_unit`), the
//!   site's features (`site::site_features`) from that decision's near
//!   observation, the turn's brief and the AI's memory: what the player
//!   knew at the moment of the choice;
//! - the player's next turn (`start_turn`) confirms the city: its own
//!   cities hold one on the tile; a site without it (the order failed) has
//!   no row;
//! - the city's state at the first turn at least `HORIZONS` rounds after
//!   the founding round (`OwnCity`: population, gross food, production,
//!   science, gold, culture, worked tiles, buildings);
//! - the round of the first turn the city is no longer the player's
//!   (`lost_round`); its later states stay empty, as do those the game
//!   (or the player) ended before.
//!
//! The rows come in the order of the founding decisions. The features never
//! read the truth; the states are the player's own sight.

use std::cell::{Cell, RefCell};
use std::io::{self, Write};
use std::time::Instant;

use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::planner::TurnBrief;
use classic_rules::ai::site::{site_features, SiteInputs, SITE_FEATURES, SITE_NAMES};
use classic_rules::ai::variant::Variant;
use classic_rules::classic::Classic;
use classic_rules::knowledge::observation::{AIObservation, LocalObservation, OwnCity};
use classic_rules::{Command, CompiledRuleset, GameState};
use kernel::ai::mind::{AiMind, TurnStart};
use kernel::ai::plan::{AiSettings, Decision, PlanTools};
use kernel::ai::state::AiState;
use kernel::ids::{PlayerId, TileId, UnitHandle};

use crate::record::new_game;
use crate::spec::GameSpec;

/// Rounds after the founding at which a city's state is written.
pub const HORIZONS: [u32; 3] = [25, 50, 100];

/// A city's state as its owner sees it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CityState {
    pub population: u32,
    /// Gross food of the city.
    pub food: i32,
    pub production: i32,
    pub science: i32,
    pub gold: i32,
    pub culture: u32,
    pub worked: u32,
    pub buildings: u32,
}

impl CityState {
    fn of(city: &OwnCity) -> Self {
        CityState {
            population: city.population,
            food: city.income.food,
            production: city.income.production,
            science: city.income.science,
            gold: city.income.gold,
            culture: city.income.culture,
            worked: city.worked.len() as u32,
            buildings: city.buildings.len() as u32,
        }
    }

    /// Food, production, science, gold and culture together.
    pub fn total_yields(&self) -> i64 {
        i64::from(self.food) + i64::from(self.production) + i64::from(self.science) + i64::from(self.gold) + i64::from(self.culture)
    }

    fn values(&self) -> [i64; 8] {
        [
            i64::from(self.population),
            i64::from(self.food),
            i64::from(self.production),
            i64::from(self.science),
            i64::from(self.gold),
            i64::from(self.culture),
            i64::from(self.worked),
            i64::from(self.buildings),
        ]
    }
}

/// The names of a state's columns, after the horizon's number.
const STATE_NAMES: [&str; 8] = ["population", "food", "production", "science", "gold", "culture", "worked", "buildings"];

/// One founded city: its site's features and how it developed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The game's seed.
    pub game: u64,
    pub player: u8,
    pub tile: u32,
    /// The round of the founding decision.
    pub round: u32,
    pub features: [i32; SITE_FEATURES],
    /// The state at each of `HORIZONS` (None: lost or ended before).
    pub states: [Option<CityState>; HORIZONS.len()],
    pub lost_round: Option<u32>,
}

impl Row {
    /// A feature by its name in `SITE_NAMES`.
    pub fn feature(&self, name: &str) -> Option<i32> {
        SITE_NAMES.iter().position(|n| *n == name).map(|i| self.features[i])
    }

    /// The state at the horizon `rounds` (one of `HORIZONS`).
    pub fn state_at(&self, rounds: u32) -> Option<CityState> {
        HORIZONS.iter().position(|h| *h == rounds).and_then(|i| self.states[i])
    }
}

pub fn header() -> String {
    let mut columns: Vec<String> = ["game", "player", "tile", "round"].map(str::to_owned).to_vec();
    columns.extend(SITE_NAMES.iter().map(|n| (*n).to_owned()));
    for horizon in HORIZONS {
        columns.extend(STATE_NAMES.iter().map(|n| format!("{n}_{horizon}")));
    }
    columns.push("lost_round".to_owned());
    columns.join(",")
}

/// The CSV line of a row (no newline); an empty field for a missing value.
pub fn line(row: &Row) -> String {
    let mut out = format!("{},{},{},{}", row.game, row.player, row.tile, row.round);
    for value in row.features {
        out.push(',');
        out.push_str(&value.to_string());
    }
    for state in row.states {
        for value in state.map(|s| s.values().map(Some)).unwrap_or([None; 8]) {
            out.push(',');
            if let Some(value) = value {
                out.push_str(&value.to_string());
            }
        }
    }
    out.push(',');
    if let Some(round) = row.lost_round {
        out.push_str(&round.to_string());
    }
    out
}

/// The header and the rows as CSV.
pub fn write_csv(out: &mut dyn Write, rows: &[Row]) -> io::Result<()> {
    writeln!(out, "{}", header())?;
    for row in rows {
        writeln!(out, "{}", line(row))?;
    }
    Ok(())
}

/// What computing the features cost (wall time, not in the rows).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Cost {
    pub sites: u64,
    pub nanos: u64,
    pub max_nanos: u64,
}

impl Cost {
    pub fn plus(self, other: Cost) -> Cost {
        Cost { sites: self.sites + other.sites, nanos: self.nanos + other.nanos, max_nanos: self.max_nanos.max(other.max_nanos) }
    }
}

/// A site chosen and not settled yet: confirmed by the next turn, then
/// filled in.
struct Pending {
    row: Row,
    confirmed: bool,
    /// The next turn showed no city there: no row.
    failed: bool,
}

/// A mind whose founding decisions become site rows; every call is
/// delegated to `P` unchanged.
pub struct SiteProbe<P> {
    mind: P,
    game: u64,
    pending: RefCell<Vec<Pending>>,
    cost: Cell<Cost>,
}

impl<P: std::fmt::Debug> std::fmt::Debug for SiteProbe<P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SiteProbe").field("mind", &self.mind).field("game", &self.game).finish_non_exhaustive()
    }
}

impl<P> SiteProbe<P> {
    /// Probe `mind`; the rows carry `game` (the seed).
    pub fn new(mind: P, game: u64) -> Self {
        Self { mind, game, pending: RefCell::new(Vec::new()), cost: Cell::new(Cost::default()) }
    }

    pub fn mind(&self) -> &P {
        &self.mind
    }

    /// The rows of the confirmed cities so far, in founding order.
    pub fn rows(&self) -> Vec<Row> {
        self.pending.borrow().iter().filter(|p| p.confirmed).map(|p| p.row.clone()).collect()
    }

    /// Founding orders the next turn showed no city for.
    pub fn failed(&self) -> u64 {
        self.pending.borrow().iter().filter(|p| p.failed).count() as u64
    }

    pub fn cost(&self) -> Cost {
        self.cost.get()
    }

    /// A founding decision: the site's features now.
    fn note_found(&self, near: &LocalObservation<'_>, brief: &TurnBrief, state: &AiState, settler: UnitHandle) {
        let started = Instant::now();
        let tile = if near.unit.handle == settler {
            Some(near.unit.tile)
        } else {
            near.own_units.iter().find(|u| u.handle == settler).map(|u| u.tile)
        };
        let Some(tile) = tile else { return };
        let own: Vec<TileId> = brief.cities.iter().map(|c| c.tile).collect();
        let foreign: Vec<TileId> = state.seen_cities().iter().map(|c| c.tile).collect();
        let inputs = SiteInputs {
            nation: brief.nation,
            techs: &brief.techs,
            own_cities: &own,
            foreign_cities: &foreign,
            round: near.round,
            excess_happiness: brief.happiness,
        };
        let features = site_features(&near.view, near.view.rules(), near.player, tile, &inputs);
        let row = Row {
            game: self.game,
            player: near.player.0,
            tile: tile.0,
            round: near.round,
            features,
            states: [None; HORIZONS.len()],
            lost_round: None,
        };
        self.pending.borrow_mut().push(Pending { row, confirmed: false, failed: false });
        let nanos = started.elapsed().as_nanos() as u64;
        let cost = self.cost.get();
        self.cost.set(Cost { sites: cost.sites + 1, nanos: cost.nanos + nanos, max_nanos: cost.max_nanos.max(nanos) });
    }

    /// The start of a player's turn: its sites confirmed, its cities' states
    /// at the horizons reached, its lost cities marked.
    fn note_start(&self, sight: &AIObservation<'_>) {
        for pending in self.pending.borrow_mut().iter_mut() {
            let row = &mut pending.row;
            if row.player != sight.player.0 || pending.failed || row.lost_round.is_some() || sight.round <= row.round {
                continue;
            }
            let city = sight.own_cities.iter().find(|c| c.tile.0 == row.tile);
            match (city, pending.confirmed) {
                (None, false) => pending.failed = true,
                (None, true) => row.lost_round = Some(sight.round),
                (Some(city), _) => {
                    pending.confirmed = true;
                    for (state, horizon) in row.states.iter_mut().zip(HORIZONS) {
                        if state.is_none() && sight.round >= row.round + horizon {
                            *state = Some(CityState::of(city));
                        }
                    }
                }
            }
        }
    }
}

impl<P> AiMind<Classic> for SiteProbe<P>
where
    P: for<'g> AiMind<Classic, Brief = TurnBrief, Sight<'g> = AIObservation<'g>, Near<'g> = LocalObservation<'g>>,
{
    type Brief = TurnBrief;
    type Sight<'g> = AIObservation<'g>;
    type Near<'g> = LocalObservation<'g>;

    fn note_humans(&mut self, humans: &[PlayerId]) {
        self.mind.note_humans(humans);
    }

    fn sense<'g>(&self, game: &'g GameState, rules: &'g CompiledRuleset, player: PlayerId) -> AIObservation<'g> {
        self.mind.sense(game, rules, player)
    }

    fn sense_unit<'g>(
        &self,
        game: &'g GameState,
        rules: &'g CompiledRuleset,
        settings: &AiSettings,
        player: PlayerId,
        unit: UnitHandle,
    ) -> Option<LocalObservation<'g>> {
        self.mind.sense_unit(game, rules, settings, player, unit)
    }

    fn start_turn(&self, sight: &AIObservation<'_>, rules: &CompiledRuleset, player: PlayerId, state: &mut AiState) -> TurnStart<Classic, TurnBrief> {
        self.note_start(sight);
        self.mind.start_turn(sight, rules, player, state)
    }

    fn order_units(&self, brief: &TurnBrief, rules: &CompiledRuleset, order: &mut Vec<(classic_rules::ids::EntityId, classic_rules::ids::UnitHandle)>) {
        self.mind.order_units(brief, rules, order);
    }

    fn plan_strategic(&self, brief: &TurnBrief, state: &mut AiState, rules: &CompiledRuleset) -> Vec<Decision<Classic>> {
        self.mind.plan_strategic(brief, state, rules)
    }

    fn decide_unit(
        &self,
        near: &LocalObservation<'_>,
        player: PlayerId,
        unit: UnitHandle,
        brief: &TurnBrief,
        state: &mut AiState,
        tools: &mut PlanTools<'_, Classic>,
    ) -> Option<Decision<Classic>> {
        let decision = self.mind.decide_unit(near, player, unit, brief, state, tools);
        if let Some(Command::FoundCity { settler }) = decision.as_ref().and_then(|d| d.command.as_ref()) {
            self.note_found(near, brief, state, *settler);
        }
        decision
    }

    fn permits(&self, game: &GameState, rules: &CompiledRuleset, player: PlayerId, command: &classic_rules::Command) -> bool {
        self.mind.permits(game, rules, player, command)
    }

    fn plan_end_of_turn(
        &self,
        sight: &AIObservation<'_>,
        player: PlayerId,
        brief: &TurnBrief,
        state: &AiState,
        tools: &mut PlanTools<'_, Classic>,
    ) -> Vec<Decision<Classic>> {
        self.mind.plan_end_of_turn(sight, player, brief, state, tools)
    }
}

/// The driver of a probed game.
pub type SiteDriver = kernel::ai::driver::AiDriver<Classic, SiteProbe<ClassicMind>>;

/// What a probed game came to.
#[derive(Debug, Clone)]
pub struct Probed {
    pub rows: Vec<Row>,
    /// Founding orders without a city the next turn.
    pub failed: u64,
    /// The last round played.
    pub rounds: u32,
    pub hash: u64,
    pub cost: Cost,
}

/// Plays `spec` to its end (or `spec.victories.max_rounds`) with the
/// variant's mind probed, every seat the AI's; the rows carry the seed.
pub fn probe_game(rules: &CompiledRuleset, spec: &GameSpec, variant: Variant) -> Result<Probed, String> {
    let mut game = new_game(rules, spec);
    let settings = AiSettings::for_rules(rules, &spec.difficulty);
    let mut driver = SiteDriver::with_mind(settings, Vec::new(), SiteProbe::new(ClassicMind::new(variant), spec.seed));
    let last = spec.victories.max_rounds;
    while !game.is_over() && game.view().auth.turn().round <= last {
        let report = driver.run(&mut game, rules, u32::MAX);
        if !report.round_done && !game.is_over() {
            return Err(format!("round {} waits for a human", game.view().auth.turn().round));
        }
    }
    let probe = driver.mind_mut();
    Ok(Probed {
        rows: probe.rows(),
        failed: probe.failed(),
        rounds: game.view().auth.turn().round,
        hash: kernel::replay::hash::authoritative_hash(&game),
        cost: probe.cost(),
    })
}

/// Pearson's correlation of `x` with `y` (0 for fewer than two pairs or no
/// spread).
pub fn pearson(x: &[f64], y: &[f64]) -> f64 {
    let n = x.len().min(y.len());
    if n < 2 {
        return 0.0;
    }
    let mean = |v: &[f64]| v[..n].iter().sum::<f64>() / n as f64;
    let (mx, my) = (mean(x), mean(y));
    let (mut sxy, mut sxx, mut syy) = (0.0, 0.0, 0.0);
    for i in 0..n {
        let (dx, dy) = (x[i] - mx, y[i] - my);
        sxy += dx * dy;
        sxx += dx * dx;
        syy += dy * dy;
    }
    if sxx == 0.0 || syy == 0.0 {
        return 0.0;
    }
    sxy / (sxx * syy).sqrt()
}

/// Ranks from 1, ties at their mean rank.
pub fn ranks(values: &[f64]) -> Vec<f64> {
    let mut order: Vec<usize> = (0..values.len()).collect();
    order.sort_by(|a, b| values[*a].total_cmp(&values[*b]));
    let mut out = vec![0.0; values.len()];
    let mut i = 0;
    while i < order.len() {
        let mut j = i;
        while j + 1 < order.len() && values[order[j + 1]] == values[order[i]] {
            j += 1;
        }
        let rank = (i + j) as f64 / 2.0 + 1.0;
        for k in &order[i..=j] {
            out[*k] = rank;
        }
        i = j + 1;
    }
    out
}

/// Spearman's correlation: Pearson's of the ranks.
pub fn spearman(x: &[f64], y: &[f64]) -> f64 {
    pearson(&ranks(x), &ranks(y))
}

/// How well a score orders the outcomes, overall and within groups (the
/// choice the AI faces is among the sites of one player at one moment).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Agreement {
    pub rows: usize,
    pub pearson: f64,
    pub spearman: f64,
    /// Rows in groups of at least two, and the groups.
    pub grouped_rows: usize,
    pub groups: usize,
    /// Within groups: Pearson's of the values minus their group's means,
    /// and Spearman's of the ranks within each group minus their mean, all
    /// groups pooled.
    pub within_pearson: f64,
    pub within_spearman: f64,
    /// Pairs within a group with different scores and different outcomes,
    /// and the share of them the score orders right.
    pub pairs: u64,
    pub concordant: f64,
}

impl Agreement {
    /// The agreement of `score` with `outcome` over `(group, score, outcome)`.
    pub fn of(points: &[(u64, f64, f64)]) -> Self {
        let xs: Vec<f64> = points.iter().map(|p| p.1).collect();
        let ys: Vec<f64> = points.iter().map(|p| p.2).collect();
        let mut agreement = Agreement { rows: points.len(), pearson: pearson(&xs, &ys), spearman: spearman(&xs, &ys), ..Default::default() };
        let mut groups: std::collections::BTreeMap<u64, Vec<(f64, f64)>> = std::collections::BTreeMap::new();
        for (group, x, y) in points {
            groups.entry(*group).or_default().push((*x, *y));
        }
        let (mut dx, mut dy, mut rx, mut ry) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let (mut pairs, mut right) = (0u64, 0u64);
        for members in groups.values().filter(|m| m.len() >= 2) {
            agreement.groups += 1;
            agreement.grouped_rows += members.len();
            let n = members.len() as f64;
            let (mx, my) = (members.iter().map(|m| m.0).sum::<f64>() / n, members.iter().map(|m| m.1).sum::<f64>() / n);
            let xs: Vec<f64> = members.iter().map(|m| m.0).collect();
            let ys: Vec<f64> = members.iter().map(|m| m.1).collect();
            let mean_rank = (n + 1.0) / 2.0;
            for ((x, y), (kx, ky)) in members.iter().zip(ranks(&xs).into_iter().zip(ranks(&ys))) {
                dx.push(x - mx);
                dy.push(y - my);
                rx.push(kx - mean_rank);
                ry.push(ky - mean_rank);
            }
            for i in 0..members.len() {
                for j in i + 1..members.len() {
                    let (a, b) = (members[i], members[j]);
                    if a.0 != b.0 && a.1 != b.1 {
                        pairs += 1;
                        right += u64::from((a.0 > b.0) == (a.1 > b.1));
                    }
                }
            }
        }
        agreement.within_pearson = pearson(&dx, &dy);
        agreement.within_spearman = pearson(&rx, &ry);
        agreement.pairs = pairs;
        agreement.concordant = if pairs == 0 { 0.0 } else { right as f64 / pairs as f64 };
        agreement
    }
}

impl std::fmt::Display for Agreement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "n {:>5}  pearson {:+.3}  spearman {:+.3} | within: {} groups, {} rows, pearson {:+.3}, spearman {:+.3}, {} pairs, {:.1}% ordered right",
            self.rows,
            self.pearson,
            self.spearman,
            self.groups,
            self.grouped_rows,
            self.within_pearson,
            self.within_spearman,
            self.pairs,
            100.0 * self.concordant
        )
    }
}

/// The group of a row: its game, its player and its window of `window`
/// rounds.
pub fn group_of(row: &Row, window: u32) -> u64 {
    (row.game << 24) ^ (u64::from(row.player) << 16) ^ u64::from(row.round / window.max(1))
}
