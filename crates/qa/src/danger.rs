//! The danger probe: a dataset for a learned model of the damage an own
//! military unit takes between the end of its owner's turn and the start of
//! the next (`classic_rules::ai::danger`). The probe changes nothing of the
//! game: the wrapper delegates every call of the AI contract to the mind it
//! wraps and only reads what passes through; the world hash of a probed game
//! equals the hash of the plain one.
//!
//! One row per military land unit (`danger::is_subject`) per player turn:
//! - at the end of the turn (`plan_end_of_turn`, a fresh sight after the
//!   units moved) the unit's features (`danger::unit_features`, the player's
//!   own sight only) and the search's estimate (`danger::heuristic_exposure`);
//! - after the end of the turn (the next `sense` of anyone: the turn's
//!   healing is done) the unit's health in the game, the base of the label;
//! - at the start of the player's next turn (`start_turn`) the label: the
//!   damage, the base minus the health then, or the whole base and `dead`
//!   for a unit that is gone. A unit gone before the base was read (disbanded
//!   in its own turn's end) has no row; nor have the units of the last turn
//!   of a game.
//!
//! The labels read the game's truth; the features never do. A battle
//! (`aurelia_qa::battle`) is probed the same way from the moment its war
//! begins (`probe_battle`); its rows are marked.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::io::{self, Write};
use std::time::Instant;

use classic_rules::ai::danger::{self, Threats, FEATURES, NAMES};
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::planner::TurnBrief;
use classic_rules::ai::variant::Variant;
use classic_rules::classic::Classic;
use classic_rules::knowledge::observation::{AIObservation, LocalObservation};
use classic_rules::knowledge::public::PlayerQueries;
use classic_rules::world::player::{ClassicKind, PlayerKind};
use classic_rules::{CompiledRuleset, GameState};
use kernel::ai::mind::{AiMind, TurnStart};
use kernel::ai::plan::{AiSettings, Decision, PlanTools};
use kernel::ai::state::AiState;
use kernel::ids::{EntityId, PlayerId, UnitHandle};

use crate::battle::{new_battle, BattleSpec};

/// One unit at the end of one player turn and what came of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The game's label (the seed for a generated game or a battle).
    pub game: u64,
    /// 1: a row of a synthetic battle (`probe_battle`), 0: of a game.
    pub battle: u8,
    pub round: u32,
    pub player: u8,
    /// 0 a nation, 1 a city-state, 2 the barbarians.
    pub side: u8,
    pub unit: u64,
    pub features: [i32; FEATURES],
    /// The search's estimate (`danger::heuristic_exposure`).
    pub exposure: i32,
    /// Health the unit regained at the end of its turn (truth).
    pub healed: i32,
    /// Health lost before its owner's next turn (truth), 0..=100.
    pub damage: i32,
    /// 1: the unit was gone at its owner's next turn.
    pub dead: i32,
}

impl Row {
    pub fn hp(&self) -> i32 {
        self.features[0]
    }

    pub fn threats(&self) -> i32 {
        self.features[NAMES.iter().position(|n| *n == "threats").unwrap_or(0)]
    }
}

/// The CSV header: the row's columns.
pub fn header() -> String {
    let mut columns: Vec<&str> = vec!["game", "battle", "round", "player", "side", "unit"];
    columns.extend(NAMES);
    columns.extend(["exposure", "healed", "damage", "dead"]);
    columns.join(",")
}

/// The CSV line of a row (no newline).
pub fn line(row: &Row) -> String {
    let mut out = format!("{},{},{},{},{},{}", row.game, row.battle, row.round, row.player, row.side, row.unit);
    for value in row.features.iter().chain([&row.exposure, &row.healed, &row.damage, &row.dead]) {
        out.push(',');
        out.push_str(&value.to_string());
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

/// What computing the features cost: one build of the threats per player
/// turn, the features of every unit with it (wall time, not in the rows).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Cost {
    pub turns: u64,
    pub units: u64,
    pub nanos: u64,
    pub max_nanos: u64,
}

impl Cost {
    pub fn plus(self, other: Cost) -> Cost {
        Cost { turns: self.turns + other.turns, units: self.units + other.units, nanos: self.nanos + other.nanos, max_nanos: self.max_nanos.max(other.max_nanos) }
    }
}

/// The rows of one player turn waiting for their labels.
struct Pending {
    rows: Vec<Row>,
    /// The units' health after the end of the turn (None: not read yet;
    /// a gone unit's entry is None).
    base: Option<Vec<Option<u16>>>,
}

/// A mind whose end-of-turn sights are turned into danger rows; every call
/// is delegated to `P` unchanged.
pub struct DangerProbe<P> {
    mind: P,
    game: u64,
    battle: bool,
    pending: RefCell<BTreeMap<PlayerId, Pending>>,
    rows: RefCell<Vec<Row>>,
    cost: Cell<Cost>,
}

impl<P: std::fmt::Debug> std::fmt::Debug for DangerProbe<P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DangerProbe").field("mind", &self.mind).field("game", &self.game).field("rows", &self.rows.borrow().len()).finish_non_exhaustive()
    }
}

impl<P> DangerProbe<P> {
    pub fn new(mind: P, game: u64) -> Self {
        Self { mind, game, battle: false, pending: RefCell::new(BTreeMap::new()), rows: RefCell::new(Vec::new()), cost: Cell::new(Cost::default()) }
    }

    /// The probe of a battle: its rows are marked (`Row::battle`).
    pub fn in_battle(mut self) -> Self {
        self.battle = true;
        self
    }

    pub fn mind(&self) -> &P {
        &self.mind
    }

    /// The labelled rows so far, taken.
    pub fn take_rows(&self) -> Vec<Row> {
        std::mem::take(&mut self.rows.borrow_mut())
    }

    pub fn cost(&self) -> Cost {
        self.cost.get()
    }

    /// The rows of a turn's end: every military land unit of the sight.
    fn note_end(&self, sight: &AIObservation<'_>, brief: &TurnBrief, state: &AiState, rules: &CompiledRuleset) {
        let started = Instant::now();
        let threats = Threats::build(sight, rules, brief, state.seen_cities());
        let side = match sight.view.own_kind() {
            PlayerKind::Major => 0,
            PlayerKind::Module(ClassicKind::Barbarian) => 2,
            PlayerKind::Module(_) => 1,
        };
        let rows: Vec<Row> = threats
            .own
            .iter()
            .map(|unit| Row {
                game: self.game,
                battle: u8::from(self.battle),
                round: sight.round,
                player: sight.player.0,
                side,
                unit: unit.id.0,
                features: danger::unit_features(&threats, rules, unit),
                exposure: danger::heuristic_exposure(&threats, rules, unit),
                healed: 0,
                damage: 0,
                dead: 0,
            })
            .collect();
        let nanos = started.elapsed().as_nanos() as u64;
        let cost = self.cost.get();
        self.cost.set(cost.plus(Cost { turns: 1, units: rows.len() as u64, nanos, max_nanos: nanos }));
        self.pending.borrow_mut().insert(sight.player, Pending { rows, base: None });
    }

    /// The health of the waiting units now that their turn has ended.
    fn note_base(&self, game: &GameState) {
        let mut pending = self.pending.borrow_mut();
        if pending.values().all(|p| p.base.is_some()) {
            return;
        }
        let mut health: BTreeMap<EntityId, u16> = BTreeMap::new();
        for (_, core) in game.view().auth.units().iter_handles() {
            if pending.get(&core.owner).is_some_and(|p| p.base.is_none()) {
                health.insert(core.id, core.hp);
            }
        }
        for turn in pending.values_mut().filter(|p| p.base.is_none()) {
            turn.base = Some(turn.rows.iter().map(|r| health.get(&EntityId(r.unit)).copied()).collect());
        }
    }

    /// The labels of the player's last turn, from its new sight.
    fn note_start(&self, sight: &AIObservation<'_>) {
        let Some(turn) = self.pending.borrow_mut().remove(&sight.player) else { return };
        let Some(base) = turn.base else { return };
        let mut rows = self.rows.borrow_mut();
        for (mut row, base) in turn.rows.into_iter().zip(base) {
            let Some(base) = base else { continue };
            let base = i32::from(base);
            row.healed = base - row.hp();
            match sight.own_units.iter().find(|u| u.id.0 == row.unit) {
                Some(unit) => row.damage = (base - i32::from(unit.hp)).max(0),
                None => {
                    row.damage = base;
                    row.dead = 1;
                }
            }
            rows.push(row);
        }
    }
}

impl<P> AiMind<Classic> for DangerProbe<P>
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
        self.note_base(game);
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
        self.mind.decide_unit(near, player, unit, brief, state, tools)
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
        self.note_end(sight, brief, state, tools.rules);
        self.mind.plan_end_of_turn(sight, player, brief, state, tools)
    }
}

/// The driver of a probed game.
pub type ProbeDriver = kernel::ai::driver::AiDriver<Classic, DangerProbe<ClassicMind>>;

/// What a probed game came to.
#[derive(Debug, Clone)]
pub struct Probed {
    pub rows: Vec<Row>,
    /// The last round played.
    pub rounds: u32,
    pub hash: u64,
    pub cost: Cost,
}

/// Plays `game` from where it stands for `rounds` rounds (or to its end)
/// with the variant's mind probed, every seat the AI's; the rows carry
/// `label`. Any starting position will do: a new game, a battle.
pub fn probe_game(rules: &CompiledRuleset, mut game: GameState, rounds: u32, settings: AiSettings, variant: Variant, label: u64) -> Result<Probed, String> {
    let probe = DangerProbe::new(ClassicMind::new(variant), label);
    let mut driver = ProbeDriver::with_mind(settings, Vec::new(), probe);
    let last = game.view().auth.turn().round.saturating_add(rounds.saturating_sub(1));
    while !game.is_over() && game.view().auth.turn().round <= last {
        let report = driver.run(&mut game, rules, u32::MAX);
        if !report.round_done && !game.is_over() {
            return Err(format!("round {} waits for a human", game.view().auth.turn().round));
        }
    }
    let probe = driver.mind_mut();
    Ok(Probed { rows: probe.take_rows(), rounds: game.view().auth.turn().round, hash: kernel::replay::hash::authoritative_hash(&game), cost: probe.cost() })
}

/// Plays the battle of `spec` (`battle::new_battle`: its start and the AI
/// memory of the rounds before the war) for `rounds` rounds with the
/// variant's mind probed, the war kept and the losses sent back as
/// `Battle::play` does; the rows carry the seed and are marked a battle's.
pub fn probe_battle(rules: &CompiledRuleset, spec: &BattleSpec, rounds: u32, variant: Variant) -> Result<Probed, String> {
    let mut battle = new_battle(rules, spec)?;
    let probe = DangerProbe::new(ClassicMind::new(variant), spec.seed).in_battle();
    let mut driver = battle.driver(rules, probe)?;
    battle.play(rules, &mut driver, rounds, |_| {})?;
    let probe = driver.mind_mut();
    Ok(Probed { rows: probe.take_rows(), rounds: battle.game.view().auth.turn().round, hash: kernel::replay::hash::authoritative_hash(&battle.game), cost: probe.cost() })
}

/// The baseline's quality on the rows: how well the search's estimate
/// predicts the damage and the deaths.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Quality {
    pub rows: u64,
    /// Rows with damage > 0, and with a dead unit.
    pub hurt: u64,
    pub dead: u64,
    pub mean_damage: f64,
    /// Mean absolute error of the exposure, of the exposure capped at the
    /// unit's health, and of predicting 0.
    pub mae: f64,
    pub mae_capped: f64,
    pub mae_zero: f64,
    /// Pearson correlation of the exposure with the damage.
    pub correlation: f64,
    /// The kills the exposure predicts (exposure >= hp): right, wrong, missed.
    pub kills_right: u64,
    pub kills_wrong: u64,
    pub kills_missed: u64,
}

impl Quality {
    pub fn of<'r>(rows: impl Iterator<Item = &'r Row>) -> Self {
        let mut q = Quality::default();
        let (mut sx, mut sy, mut sxx, mut syy, mut sxy) = (0f64, 0f64, 0f64, 0f64, 0f64);
        let (mut abs, mut abs_capped, mut abs_zero) = (0f64, 0f64, 0f64);
        for row in rows {
            q.rows += 1;
            q.hurt += u64::from(row.damage > 0);
            q.dead += u64::from(row.dead == 1);
            let (x, y) = (f64::from(row.exposure), f64::from(row.damage));
            sx += x;
            sy += y;
            sxx += x * x;
            syy += y * y;
            sxy += x * y;
            abs += (x - y).abs();
            abs_capped += (f64::from(row.exposure.min(row.hp())) - y).abs();
            abs_zero += y;
            let predicted = row.exposure >= row.hp();
            match (predicted, row.dead == 1) {
                (true, true) => q.kills_right += 1,
                (true, false) => q.kills_wrong += 1,
                (false, true) => q.kills_missed += 1,
                (false, false) => {}
            }
        }
        if q.rows == 0 {
            return q;
        }
        let n = q.rows as f64;
        q.mean_damage = sy / n;
        q.mae = abs / n;
        q.mae_capped = abs_capped / n;
        q.mae_zero = abs_zero / n;
        let cov = sxy / n - (sx / n) * (sy / n);
        let vx = sxx / n - (sx / n).powi(2);
        let vy = syy / n - (sy / n).powi(2);
        q.correlation = if vx > 0.0 && vy > 0.0 { cov / (vx * vy).sqrt() } else { 0.0 };
        q
    }

    /// Of the predicted kills, the share that died.
    pub fn precision(&self) -> f64 {
        share(self.kills_right, self.kills_right + self.kills_wrong)
    }

    /// Of the deaths, the share predicted.
    pub fn recall(&self) -> f64 {
        share(self.kills_right, self.kills_right + self.kills_missed)
    }
}

fn share(part: u64, whole: u64) -> f64 {
    if whole == 0 {
        0.0
    } else {
        part as f64 / whole as f64
    }
}

impl std::fmt::Display for Quality {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let pct = |part: u64| 100.0 * share(part, self.rows);
        writeln!(f, "  rows {}, damage > 0 {:.1} %, dead {:.2} %, mean damage {:.2}", self.rows, pct(self.hurt), pct(self.dead), self.mean_damage)?;
        writeln!(f, "  MAE exposure {:.2}, capped at hp {:.2}, predicting 0 {:.2}; correlation {:.3}", self.mae, self.mae_capped, self.mae_zero, self.correlation)?;
        write!(
            f,
            "  kills (exposure >= hp): predicted {}, right {}, deaths {}; precision {:.3}, recall {:.3}",
            self.kills_right + self.kills_wrong,
            self.kills_right,
            self.dead,
            self.precision(),
            self.recall()
        )
    }
}
