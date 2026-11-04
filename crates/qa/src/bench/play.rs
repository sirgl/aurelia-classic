//! One match: a new game of the suite, the candidate's mind on its seats
//! and the baseline's on the rest (`kernel::ai::seats::Seated`), played by
//! the kernel's AI driver to a victory; and a tournament: the suite's
//! matches across threads, gathered in the suite's order.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use kernel::ai::driver::AiDriver;
use serde::{Deserialize, Serialize};
use kernel::ai::seats::Seated;
use kernel::ai::{DriverPhase, StepOutcome};
use kernel::ids::PlayerId;
use kernel::replay::hash::authoritative_hash;
use kernel::rules::CompiledRuleset;
use kernel::transaction::journal::{AppliedCommand, CityChange, PlayerChange, UnitChange};
use kernel::world::state::GameState;

use crate::bench::modules::{majors, standing, BenchRules, Standing};
use crate::bench::suite::{Match, Side, Suite};

/// The two versions of a tournament (equal in solo mode).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pairing {
    pub baseline: String,
    pub candidate: String,
}

/// What one nation's seat did in one match.
#[derive(Debug, Clone)]
pub struct SeatResult {
    pub player: PlayerId,
    pub side: Side,
    /// Its leader (`BenchRules::leader`; empty without leaders).
    pub leader: String,
    /// The goal of its AI's grand strategy at the end (None: no grand
    /// strategy).
    pub path: Option<&'static str>,
    /// 1: the winner; then the living by score; then the eliminated, the
    /// later out the higher.
    pub rank: u32,
    /// The kind of the victory when this seat won.
    pub victory: Option<&'static str>,
    /// The round the nation was eliminated in.
    pub eliminated: Option<u32>,
    /// Standings at the suite's checkpoints (the end's when the game ended
    /// before), then at the end.
    pub standings: Vec<Standing>,
    /// AI commands the game applied and rejected, and decisions without a
    /// command.
    pub applied: u64,
    pub rejected: u64,
    pub no_command: u64,
    /// Wall time of each of its turns.
    pub turn_ms: Vec<f64>,
    /// Wall time of each of its turns without ending it (the end of the
    /// turn runs the module's systems, and the round's after the last
    /// player): the AI's own time.
    pub think_ms: Vec<f64>,
    /// Food delivered between its cities over the match
    /// (`BenchRules::delivered`).
    pub delivered: u64,
    pub combat: CombatRecord,
    /// Each rival's original capital by founder, and the round the seat
    /// first knew its tile (None: never).
    pub sightings: Vec<(PlayerId, Option<u32>)>,
    /// Cities it founded (the capital too).
    pub founded: u32,
    /// Its meaningful decisions in each of the first rounds, from the
    /// kernel's round 0 (`BenchRules::OPENING_ROUNDS`; empty: the module
    /// counts none).
    pub opening: Vec<u32>,
    /// The rounds it reached the milestones of the game's pace.
    pub pace: Pace,
}

/// The rounds a nation reached the milestones of a game's pace (the
/// bench's round, as the checkpoints): its first two cities, its first
/// technology beyond the start, each era (by the era's order; the start
/// era is not a milestone). None: not reached.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pace {
    pub first_city: Option<u32>,
    pub second_city: Option<u32>,
    pub first_tech: Option<u32>,
    /// Index = the era's order; index 0 (the start) stays None.
    pub eras: Vec<Option<u32>>,
    /// The round the spaceship's prerequisite projects were all done
    /// (`BenchRules::spaceship`; reports saved before read None).
    #[serde(default)]
    pub space_project: Option<u32>,
    /// The round each spaceship part was installed, in order (reports saved
    /// before read none).
    #[serde(default)]
    pub parts: Vec<u32>,
}

impl Pace {
    /// A city was founded in `round`.
    pub(crate) fn founded(&mut self, round: u32) {
        if self.first_city.is_none() {
            self.first_city = Some(round);
        } else if self.second_city.is_none() {
            self.second_city = Some(round);
        }
    }

    /// After `round`: `techs` learned beyond the start, the player's `era`.
    pub(crate) fn progress(&mut self, round: u32, techs: usize, era: u8) {
        if techs > 0 && self.first_tech.is_none() {
            self.first_tech = Some(round);
        }
        let era = usize::from(era);
        if self.eras.len() <= era {
            self.eras.resize(era + 1, None);
        }
        for entered in self.eras.iter_mut().take(era + 1).skip(1) {
            entered.get_or_insert(round);
        }
    }

    /// After `round`: the spaceship as `BenchRules::spaceship` counts it.
    pub(crate) fn spaceship(&mut self, round: u32, space: SpaceProgress) {
        if space.project_done && self.space_project.is_none() {
            self.space_project = Some(round);
        }
        while self.parts.len() < space.parts as usize {
            self.parts.push(round);
        }
    }
}

/// A nation's spaceship at one moment (`BenchRules::spaceship`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SpaceProgress {
    /// Every project the parts need first is done.
    pub project_done: bool,
    /// Parts installed.
    pub parts: u32,
}

/// What a nation won and lost in fights (the commands its module
/// calls combat) over a whole match.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CombatRecord {
    /// Cities it took, and cities taken from it.
    pub captured: u32,
    pub cities_lost: u32,
    /// Other players' units destroyed in its fights, and its own units
    /// destroyed in anybody's fights.
    pub kills: u32,
    pub units_lost: u32,
    /// Wars it declared, and how they ended: by an accepted deal, by one
    /// side alone, by an elimination, or not before the game's end (reports
    /// saved before these read as zeros).
    pub wars: u32,
    /// Its original capital taken from it (reports saved before read 0).
    #[serde(default)]
    pub capitals_lost: u32,
    /// Wars declared on it, and those of them in which the declarer took
    /// one of its cities before the peace: the attackers' success.
    #[serde(default)]
    pub attacked: u32,
    #[serde(default)]
    pub attacks_lost: u32,
    #[serde(default)]
    pub peace_by_deal: u32,
    #[serde(default)]
    pub peace_alone: u32,
    #[serde(default)]
    pub eliminated: u32,
    #[serde(default)]
    pub at_the_end: u32,
    /// Rounds its wars lasted, summed (to the end of the game for the wars
    /// still running).
    #[serde(default)]
    pub war_rounds: u32,
    /// How long its wars lasted, in buckets of [`WAR_BUCKET`] rounds (the
    /// last one takes the longer ones): the median war of the acceptance.
    #[serde(default)]
    pub war_lengths: [u32; WAR_BUCKETS],
    /// Cities it holds at the end that another player founded: the captures it kept
    /// (reports saved before read 0).
    #[serde(default)]
    pub held: u32,
    /// The bench round it first took a rival's original capital (reports saved
    /// before read none; summed records keep the earliest).
    #[serde(default)]
    pub capital_taken: Option<u32>,
    /// The bench round it first took any city of another player (reports saved
    /// before read none; summed records keep the earliest).
    #[serde(default)]
    pub first_capture: Option<u32>,
}

/// The width of a bucket of `war_lengths`, in rounds, and their count.
pub const WAR_BUCKET: u32 = 2;
pub const WAR_BUCKETS: usize = 32;

impl CombatRecord {
    pub fn add(self, other: CombatRecord) -> CombatRecord {
        CombatRecord {
            captured: self.captured + other.captured,
            cities_lost: self.cities_lost + other.cities_lost,
            kills: self.kills + other.kills,
            units_lost: self.units_lost + other.units_lost,
            wars: self.wars + other.wars,
            capitals_lost: self.capitals_lost + other.capitals_lost,
            attacked: self.attacked + other.attacked,
            attacks_lost: self.attacks_lost + other.attacks_lost,
            peace_by_deal: self.peace_by_deal + other.peace_by_deal,
            peace_alone: self.peace_alone + other.peace_alone,
            eliminated: self.eliminated + other.eliminated,
            at_the_end: self.at_the_end + other.at_the_end,
            war_rounds: self.war_rounds + other.war_rounds,
            war_lengths: std::array::from_fn(|i| self.war_lengths[i] + other.war_lengths[i]),
            held: self.held + other.held,
            capital_taken: match (self.capital_taken, other.capital_taken) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            },
            first_capture: match (self.first_capture, other.first_capture) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            },
        }
    }

    /// The bucket of rounds the median war falls in (None: no war).
    pub fn median_war(&self) -> Option<(u32, u32)> {
        let total: u32 = self.war_lengths.iter().sum();
        let mut seen = 0;
        let bucket = self.war_lengths.iter().position(|&n| {
            seen += n;
            total > 0 && seen * 2 >= total
        })? as u32;
        Some((bucket * WAR_BUCKET, (bucket + 1) * WAR_BUCKET - 1))
    }

    /// Count one ended war of this declarer.
    fn end_war(&mut self, end: WarEnd, rounds: u32) {
        let count = match end {
            WarEnd::Deal => &mut self.peace_by_deal,
            WarEnd::Alone => &mut self.peace_alone,
            WarEnd::Elimination => &mut self.eliminated,
            WarEnd::GameEnd => &mut self.at_the_end,
        };
        *count += 1;
        self.war_rounds += rounds;
        self.war_lengths[(rounds / WAR_BUCKET).min(WAR_BUCKETS as u32 - 1) as usize] += 1;
    }
}

/// How a war between two nations ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WarEnd {
    /// A deal both accepted made the peace (`BenchRules::by_agreement`).
    Deal,
    /// One side made peace by itself.
    Alone,
    /// One side was eliminated.
    Elimination,
    /// The game ended first.
    GameEnd,
}

/// A running war between two seats: who declared it, on whom, since when,
/// and whether the declarer took one of the enemy's cities since.
#[derive(Debug, Clone, Copy)]
struct OpenWar {
    declarer: PlayerId,
    enemy: PlayerId,
    since: u32,
    won: bool,
}

impl OpenWar {
    fn between(&self, a: PlayerId, b: PlayerId) -> bool {
        (self.declarer, self.enemy) == (a, b) || (self.declarer, self.enemy) == (b, a)
    }
}

#[derive(Debug, Clone)]
pub struct MatchResult {
    pub label: String,
    /// The suite's seed of the game: the games of one seed share the
    /// nations, the bench's unit of resampling (`stats`).
    pub seed: u64,
    pub rounds: u32,
    pub hash: u64,
    pub seats: Vec<SeatResult>,
    /// Wall time of each round.
    pub round_ms: Vec<f64>,
    /// The names of the rules' eras by their order (`Pace::eras`).
    pub eras: Vec<String>,
    /// The parts a spaceship needs (`BenchRules::SPACESHIP_PARTS`; 0: no
    /// science victory by a spaceship).
    pub space_parts: u32,
}

impl MatchResult {
    pub fn winner(&self) -> Option<&SeatResult> {
        self.seats.iter().find(|s| s.victory.is_some())
    }
}

/// Plays the suite's matches on `jobs` threads; the results come in the
/// suite's order whatever the threads did. Failed matches are reported by
/// label.
pub fn tournament<R: BenchRules>(suite: &Suite, matches: &[Match], pairing: &Pairing, jobs: usize) -> Result<Vec<MatchResult>, Vec<String>> {
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<Result<MatchResult, String>>>> = Mutex::new(vec![None; matches.len()]);
    std::thread::scope(|scope| {
        for _ in 0..jobs.clamp(1, matches.len().max(1)) {
            scope.spawn(|| {
                let rules = R::rules();
                loop {
                    let at = next.fetch_add(1, Ordering::Relaxed);
                    let Some(game) = matches.get(at) else { break };
                    let result = play_match::<R>(&rules, suite, game, pairing);
                    results.lock().expect("no thread panicked holding the results")[at] = Some(result);
                }
            });
        }
    });
    let results = results.into_inner().expect("no thread panicked holding the results");
    let (done, failed): (Vec<_>, Vec<_>) = results.into_iter().map(|r| r.expect("every match played")).partition(Result::is_ok);
    if failed.is_empty() {
        Ok(done.into_iter().map(Result::unwrap).collect())
    } else {
        Err(failed.into_iter().map(Result::unwrap_err).collect())
    }
}

/// One match to its victory.
pub fn play_match<R: BenchRules>(rules: &CompiledRuleset<R::M>, suite: &Suite, game_match: &Match, pairing: &Pairing) -> Result<MatchResult, String> {
    let label = game_match.label();
    let mut game = R::new_game_at(rules, &game_match.map, game_match.seed, suite.rounds, &suite.speed);
    let seats: Vec<(PlayerId, Side)> = majors(&game).into_iter().enumerate().map(|(i, p)| (p, game_match.seating.side(i))).collect();
    let mut driver = AiDriver::with_mind(R::settings(rules), Vec::new(), seated::<R>(&seats, pairing)?);
    driver.set_trace_capacity(0);
    driver.record_journal();
    let mut log = MatchLog::new(&seats, R::OPENING_ROUNDS);
    for seat in &mut log.seats {
        seat.leader = R::leader(&game, rules, seat.player);
    }
    log.start_techs = log.seats.iter().map(|s| game.view().auth.players().techs_of(s.player).len()).collect();
    let file = format!("{}-{}-{}-{}-{}-{}", game_match.seed, suite.name, game_match.map.script, game_match.map.size, game_match.seating.name(), pairing.candidate.replace(['/', ':', '+', ','], "_"));
    let mut recorder = R::recorder(&file, game_match.seed);
    while !game.is_over() && round(&game) <= suite.rounds {
        if let Some(r) = recorder.as_mut() {
            r.round_start(&game, rules);
        }
        play_round::<R>(&mut game, rules, &mut driver, &mut log, recorder.as_mut()).map_err(|e| format!("{label}: {e}"))?;
        log.after_round::<R>(&game, rules, &suite.checkpoints);
    }
    if let Some(r) = recorder {
        let seated: &Seated<R::Mind> = driver.mind_mut();
        let minds: Vec<&R::Mind> = seats.iter().map(|(p, _)| seated.mind(*p)).collect();
        r.finish(&game, rules, &minds);
    }
    for seat in &mut log.seats {
        let aim = driver.states().get(seat.player).and_then(|s| s.aim);
        seat.path = aim.and_then(|a| R::goal_name(a.goal));
    }
    Ok(log.finish::<R>(label, game_match.seed, &game, rules, suite.checkpoints.len()))
}

/// The candidate's mind on its seats, the baseline's on every other player.
fn seated<R: BenchRules>(seats: &[(PlayerId, Side)], pairing: &Pairing) -> Result<Seated<R::Mind>, String> {
    let mind = |name: &str| R::mind(name).ok_or_else(|| format!("{} has no AI variant {name}", R::NAME));
    let mut candidates = Vec::new();
    for (player, side) in seats {
        if *side == Side::Candidate {
            candidates.push((*player, mind(&pairing.candidate)?));
        }
    }
    Ok(Seated::new(mind(&pairing.baseline)?, candidates))
}

fn round<M: kernel::module::RulesModule>(game: &GameState<M>) -> u32 {
    game.view().auth.turn().round
}

type Driver<R> = AiDriver<<R as BenchRules>::M, Seated<<R as BenchRules>::Mind>>;

/// Every AI turn of one round; each player's turn is timed (with and
/// without its end) and its commands counted.
fn play_round<R: BenchRules>(
    game: &mut GameState<R::M>,
    rules: &CompiledRuleset<R::M>,
    driver: &mut Driver<R>,
    log: &mut MatchLog,
    mut recorder: Option<&mut Box<dyn crate::bench::modules::MatchRecorder<R>>>,
) -> Result<(), String> {
    let started = Instant::now();
    let this_round = round(game);
    let mut turn_started = started;
    let mut ending = std::time::Duration::ZERO;
    let mut before = Counts::of::<R>(driver);
    loop {
        let ends_turn = driver.phase() == DriverPhase::PlayerEnd;
        if driver.phase() == DriverPhase::Observe {
            if let Some(r) = recorder.as_mut() {
                r.turn_start(game, rules, game.view().auth.active_player());
            }
        }
        let step_started = Instant::now();
        let outcome = driver.step(game, rules);
        if ends_turn {
            ending += step_started.elapsed();
        }
        match outcome {
            StepOutcome::Worked => {}
            StepOutcome::PlayerDone(player) => {
                let now = Instant::now();
                let after = Counts::of::<R>(driver);
                let ms = |d: std::time::Duration| d.as_secs_f64() * 1000.0;
                log.turn(player, ms(now - turn_started), ms((now - turn_started).saturating_sub(ending)), after.minus(before));
                let journal = driver.take_journal();
                log.journal::<R>(game, player, &journal);
                log.deliveries::<R>(game, &journal);
                log.decisions::<R>(this_round, player, &journal);
                (turn_started, before, ending) = (now, after, std::time::Duration::ZERO);
            }
            StepOutcome::RoundDone => break,
            StepOutcome::Waiting if game.is_over() => break,
            StepOutcome::Waiting => return Err(format!("round {} waits for a human", round(game))),
        }
    }
    log.round_ms.push(started.elapsed().as_secs_f64() * 1000.0);
    Ok(())
}

/// The driver's running totals: applied and rejected commands, and every
/// traced decision.
#[derive(Debug, Clone, Copy, Default)]
struct Counts {
    applied: u64,
    rejected: u64,
    traced: u64,
}

impl Counts {
    fn of<R: BenchRules>(driver: &Driver<R>) -> Self {
        let counters = driver.counters();
        Self {
            applied: counters.commands_applied,
            rejected: counters.commands_rejected,
            traced: driver.trace().totals().values().sum(),
        }
    }

    fn minus(self, before: Counts) -> Counts {
        Counts {
            applied: self.applied - before.applied,
            rejected: self.rejected - before.rejected,
            traced: self.traced - before.traced,
        }
    }
}

/// What the bench records of every nation while a match runs.
struct MatchLog {
    seats: Vec<SeatResult>,
    round_ms: Vec<f64>,
    wars: Vec<OpenWar>,
    /// Technologies each seat started with, by seat.
    start_techs: Vec<usize>,
}

impl MatchLog {
    fn new(seats: &[(PlayerId, Side)], opening: u32) -> Self {
        let seats = seats
            .iter()
            .map(|&(player, side)| SeatResult {
                player,
                side,
                leader: String::new(),
                path: None,
                rank: 0,
                victory: None,
                eliminated: None,
                standings: Vec::new(),
                applied: 0,
                rejected: 0,
                no_command: 0,
                turn_ms: Vec::new(),
                think_ms: Vec::new(),
                delivered: 0,
                combat: CombatRecord::default(),
                sightings: Vec::new(),
                founded: 0,
                opening: vec![0; opening as usize],
                pace: Pace::default(),
            })
            .collect();
        Self { seats, round_ms: Vec::new(), wars: Vec::new(), start_techs: Vec::new() }
    }

    fn seat(&mut self, player: PlayerId) -> Option<&mut SeatResult> {
        self.seats.iter_mut().find(|s| s.player == player)
    }

    fn turn(&mut self, player: PlayerId, ms: f64, think_ms: f64, counts: Counts) {
        if let Some(seat) = self.seat(player) {
            seat.turn_ms.push(ms);
            seat.think_ms.push(think_ms);
            seat.applied += counts.applied;
            seat.rejected += counts.rejected;
            seat.no_command += counts.traced - counts.applied - counts.rejected;
        }
    }

    /// The combat record, the wars and the cities founded of one player's
    /// turn: its commands are `applied` (a turn's journal holds only the
    /// acting player's commands); `game` is the world after them.
    fn journal<R: BenchRules>(&mut self, game: &GameState<R::M>, player: PlayerId, applied: &[AppliedCommand<R::M>]) {
        let now = round(game);
        for command in applied {
            let founded = command.changes.city_changes.iter().filter(|c| matches!(c, CityChange::Founded(_))).count() as u32;
            if let Some(seat) = self.seat(player) {
                seat.founded += founded;
                for _ in 0..founded {
                    seat.pace.founded(now);
                }
            }
            for change in &command.changes.player_changes {
                match *change {
                    PlayerChange::WarDeclared { player: p, enemy } => {
                        self.record(p, |r| r.wars += 1);
                        self.open_war(p, enemy, now);
                    }
                    PlayerChange::PeaceMade { player: p, enemy } => {
                        let end = if R::by_agreement(&command.command) { WarEnd::Deal } else { WarEnd::Alone };
                        self.end_wars(|w| w.between(p, enemy), end, now);
                    }
                    _ => {}
                }
            }
            if !R::is_combat(&command.command) {
                continue;
            }
            for change in &command.changes.unit_changes {
                if let UnitChange::Destroyed { owner, .. } = change {
                    self.record(*owner, |r| r.units_lost += 1);
                    if *owner != player {
                        self.record(player, |r| r.kills += 1);
                    }
                }
            }
            for change in &command.changes.city_changes {
                if let CityChange::OwnerChanged { city, from, to } = change {
                    self.capture(game, *city, *from, *to);
                }
            }
        }
    }

    /// Food the rounds closed in these commands delivered, by player.
    fn deliveries<R: BenchRules>(&mut self, game: &GameState<R::M>, applied: &[AppliedCommand<R::M>]) {
        for command in applied {
            for (player, food) in R::delivered(game, &command.changes) {
                if let Some(seat) = self.seat(player) {
                    seat.delivered += food;
                }
            }
        }
    }

    /// The meaningful decisions of one player's turn in the kernel's
    /// `round`, each for the player it went to.
    fn decisions<R: BenchRules>(&mut self, round: u32, actor: PlayerId, applied: &[AppliedCommand<R::M>]) {
        for command in applied {
            for player in R::decisions(actor, &command.command, &command.changes) {
                if let Some(count) = self.seat(player).and_then(|s| s.opening.get_mut(round as usize)) {
                    *count += 1;
                }
            }
        }
    }

    /// A war between two seats begins (wars with city-states and
    /// barbarians are not counted); it is a war on its enemy.
    fn open_war(&mut self, declarer: PlayerId, enemy: PlayerId, since: u32) {
        let seat = |p: PlayerId| self.seats.iter().any(|s| s.player == p);
        if seat(declarer) && seat(enemy) {
            self.wars.push(OpenWar { declarer, enemy, since, won: false });
            self.record(enemy, |r| r.attacked += 1);
        }
    }

    /// A city taken: the loser's original capital counts as such, and the
    /// first city a declarer takes from its enemy wins its war.
    fn capture<M: kernel::module::RulesModule>(&mut self, game: &GameState<M>, city: kernel::ids::CityHandle, from: PlayerId, to: PlayerId) {
        let now = round(game);
        self.record(to, |r| {
            r.captured += 1;
            r.first_capture.get_or_insert(now);
        });
        self.record(from, |r| r.cities_lost += 1);
        let capital = game.view().auth.cities().get(city).is_some_and(|c| c.original_capital && c.original_owner == from);
        if capital {
            self.record(from, |r| r.capitals_lost += 1);
            self.record(to, |r| {
                r.capital_taken.get_or_insert(now);
            });
        }
        let war = self.wars.iter_mut().find(|w| (w.declarer, w.enemy, w.won) == (to, from, false));
        if let Some(war) = war {
            war.won = true;
            self.record(from, |r| r.attacks_lost += 1);
        }
    }

    /// End the running wars `which` selects: each counts for its declarer.
    fn end_wars(&mut self, which: impl Fn(&OpenWar) -> bool, end: WarEnd, now: u32) {
        let (ended, running): (Vec<OpenWar>, Vec<OpenWar>) = self.wars.iter().partition(|w| which(w));
        self.wars = running;
        for war in ended {
            self.record(war.declarer, |r| r.end_war(end, now - war.since));
        }
    }

    fn record(&mut self, player: PlayerId, change: impl FnOnce(&mut CombatRecord)) {
        if let Some(seat) = self.seat(player) {
            change(&mut seat.combat);
        }
    }

    /// Eliminations and sightings of rival capitals of the round, and the
    /// standings at a checkpoint.
    fn after_round<R: BenchRules>(&mut self, game: &GameState<R::M>, rules: &CompiledRuleset<R::M>, checkpoints: &[u32]) {
        let now = round(game);
        let players = game.view().auth.players();
        let at_checkpoint = checkpoints.contains(&now);
        let mut out = Vec::new();
        for (seat, start) in self.seats.iter_mut().zip(&self.start_techs) {
            let techs = players.techs_of(seat.player).len().saturating_sub(*start);
            seat.pace.progress(now, techs, kernel::world::player::player_era(game.view().auth, rules, seat.player));
            seat.pace.spaceship(now, R::spaceship(game, rules, seat.player));
            if seat.eliminated.is_none() && !players.is_alive(seat.player) {
                seat.eliminated = Some(now);
                out.push(seat.player);
            }
            if seat.eliminated.is_none() {
                note_sightings(&mut seat.sightings, R::rival_capitals(game, rules, seat.player), now);
            }
            if at_checkpoint {
                let mut now = standing::<R>(game, rules, seat.player);
                now.economy.delivered = seat.delivered;
                seat.standings.push(now);
            }
        }
        self.end_wars(|w| out.contains(&w.declarer) || out.contains(&w.enemy), WarEnd::Elimination, now);
    }

    /// The end: missing checkpoints take the end's standings, then the
    /// winner and the ranks.
    fn finish<R: BenchRules>(mut self, label: String, seed: u64, game: &GameState<R::M>, rules: &CompiledRuleset<R::M>, checkpoints: usize) -> MatchResult {
        let winner = R::victory(game);
        self.end_wars(|_| true, WarEnd::GameEnd, round(game));
        for seat in &mut self.seats {
            let mut last = standing::<R>(game, rules, seat.player);
            last.economy.delivered = seat.delivered;
            seat.standings.resize(checkpoints, last);
            seat.standings.push(last);
            seat.victory = winner.filter(|(p, _)| *p == seat.player).map(|(_, kind)| kind);
            let player = seat.player;
            seat.combat.held = game.view().auth.cities().iter().filter(|(_, c)| c.owner == player && c.original_owner != player).count() as u32;
        }
        rank(&mut self.seats);
        MatchResult {
            label,
            seed,
            rounds: round(game),
            hash: authoritative_hash(game),
            seats: self.seats,
            round_ms: self.round_ms,
            eras: era_names(rules),
            space_parts: R::spaceship_parts(rules),
        }
    }
}

/// The rules' era names by the era's order.
fn era_names<M: kernel::module::RulesModule>(rules: &CompiledRuleset<M>) -> Vec<String> {
    let mut eras: Vec<(u8, String)> = rules.eras.iter().map(|e| (e.index, e.name.clone())).collect();
    eras.sort();
    eras.into_iter().map(|(_, name)| name).collect()
}

/// The first round each rival capital's tile is known.
fn note_sightings(sightings: &mut Vec<(PlayerId, Option<u32>)>, capitals: Vec<(PlayerId, bool)>, round: u32) {
    for (founder, known) in capitals {
        let at = match sightings.iter().position(|(f, _)| *f == founder) {
            Some(at) => at,
            None => {
                sightings.push((founder, None));
                sightings.len() - 1
            }
        };
        if known && sightings[at].1.is_none() {
            sightings[at].1 = Some(round);
        }
    }
}

/// The winner first, then the living by their final score, then the
/// eliminated, the later out the higher; ties by player id.
fn rank(seats: &mut [SeatResult]) {
    let mut order: Vec<usize> = (0..seats.len()).collect();
    order.sort_by_key(|&i| {
        let s = &seats[i];
        let last = s.standings.last().copied().unwrap_or_default();
        let standing = match s.eliminated {
            None => -last.score,
            Some(round) => -i64::from(round),
        };
        (s.victory.is_none(), s.eliminated.is_some(), standing, s.player)
    });
    for (place, i) in order.into_iter().enumerate() {
        seats[i].rank = place as u32 + 1;
    }
}

