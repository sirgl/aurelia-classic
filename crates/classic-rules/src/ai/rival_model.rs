//! The model of the rivals (`rival-net`): what a seat can tell of every nation it has met from its
//! public screens alone, and how sure it can be (classic/docs/ai-strategy-net.md, «The rivals»).
//! The strategy net reads it as inputs of its layer 0 (`strategy_net::features::rivals`).
//!
//! What is read: the score list, the demographics rows, the victory rows, the eras, the original
//! capitals as seen (`PublicScreens`), the Congress screen's delegates (`CongressInfo::delegates`)
//! and the culture screen's influence of each nation on the seat (`InfluenceView::their_level`).
//! Nothing else of the world: a nation not met has no estimate, and no hidden number is read.
//!
//! - **Demographics rows** (population, crop, goods, GNP, literacy, land, soldiers). A row gives the
//!   seat's own value, its rank, and the best, average and worst of the `n` living nations, met or
//!   not. The others' sum is `average × n − own` (the average is truncated toward zero, so up to
//!   `n − 1` more), each other lies in `[worst, best]` and in what the sum leaves after the rest.
//!   With two nations alive the rival's value is the best or the worst, whichever is not the seat's:
//!   exact. With more, the others' sum is shared by the score of each (the score list is public;
//!   a nation not met counts with the mean score of those met), clamped to the range (`row`).
//! - **Technologies**: literacy is the share of the rules' technologies known, a percent truncated
//!   (`standing::demographics`): a rival's count is every count whose share truncates into its
//!   literacy (`techs_of_literacy`).
//! - **Science** (not on the screens): the research its technologies gained since a sample
//!   `TREND_ROUNDS` back took, per round. The gained technologies are taken as the cheapest of
//!   the count (the rules' costs), at the cost factor of the game speed, the map size and the
//!   rival's cities, guessed from its population at the seat's citizens a city
//!   (`research::tech_cost`); the discount for technologies others know is not counted.
//! - **Score** and its change since the sample, per `TREND_ROUNDS`.
//! - **Victories**: original capitals held as seen (domination), technologies known over all
//!   (science), its influence on the seat (culture: the one nation the seat sees it from), its
//!   delegates over the needed (diplomacy), its era.
//!
//! The samples (`Book`) are kept between turns in the strategy net's memory
//! (`strategy_net::Memory::rivals`): a human remembers the screens of the last turns too.

use crate::derived::standing::{Demographic, Measure};
use crate::ids::PlayerId;
use crate::knowledge::observation::AIObservation;
use crate::rules::CompiledRuleset;
use crate::world::victory::VictoryKind;

/// The demographics rows, in `Measure::ALL`'s order.
pub const MEASURES: usize = Measure::ALL.len();

/// A sample of the book is taken every this many Standard rounds (× the speed's pace).
pub const SAMPLE_ROUNDS: u32 = 5;
/// The trends (science, score) are read over this many Standard rounds (× the pace): six
/// technologies of the early game, two or three of the late.
pub const TREND_ROUNDS: u32 = 30;
/// A trend needs a sample at least this many Standard rounds old (× the pace).
pub const TREND_MIN_ROUNDS: u32 = 15;

/// A number and the range it is known to lie in (`lo == hi`: known exactly).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Guess {
    pub value: i64,
    pub lo: i64,
    pub hi: i64,
}

impl Guess {
    pub fn exact(value: i64) -> Self {
        Self { value, lo: value, hi: value }
    }

    /// The width of the range over its size (0: exact), thousandths.
    pub fn doubt(&self) -> i64 {
        let size = self.lo.abs().max(self.hi.abs());
        if size == 0 {
            0
        } else {
            ((self.hi - self.lo) * 1000 / size).clamp(0, 1000)
        }
    }
}

/// What the seat can tell of one nation it has met.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rival {
    pub player: PlayerId,
    /// Its value of each demographics row (`Measure::ALL`'s order).
    pub rows: [Guess; MEASURES],
    /// Technologies it knows.
    pub techs: Guess,
    /// Beakers a round (None: no sample old enough).
    pub science: Option<i64>,
    pub score: i64,
    /// Its score's change per `TREND_ROUNDS` paced rounds (None: no sample old enough).
    pub score_trend: Option<i64>,
    pub era: Option<u8>,
    /// Original capitals it holds, as the seat has seen them.
    pub capitals: u32,
    /// Its delegates on the Congress screen (None: no Congress).
    pub delegates: Option<u16>,
    /// The level of its culture's influence on the seat (`InfluenceLevel::index`).
    pub influence_on_us: u8,
}

/// The seat's model of its rivals in a turn.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Rivals {
    pub round: u32,
    /// Nations alive, the seat included (the culture row's need + 1).
    pub living: u32,
    /// The seat's own value of each row.
    pub own: [i64; MEASURES],
    pub own_science: i64,
    pub own_score: i64,
    pub own_techs: i64,
    pub own_trend: Option<i64>,
    /// The rules' technologies.
    pub techs_total: i64,
    pub capitals_all: u32,
    pub delegates_needed: Option<u16>,
    /// Every living nation met, by id.
    pub rivals: Vec<Rival>,
}

impl Rivals {
    pub fn get(&self, player: PlayerId) -> Option<&Rival> {
        self.rivals.iter().find(|r| r.player == player)
    }
}

/// A sample of the screens: per nation (the seat's own included) its score and technologies.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Sample {
    pub round: u32,
    /// (nation, score, technologies), by id.
    pub nations: Vec<(PlayerId, i64, i64)>,
}

/// The samples of the last `TREND_ROUNDS` (and the one before), oldest first.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Book {
    pub samples: Vec<Sample>,
}

fn paced(rounds: u32, pace: u32) -> u32 {
    (rounds * pace.max(1) / 100).max(1)
}

impl Book {
    /// A sample is due: none yet, or the last `SAMPLE_ROUNDS` paced rounds old.
    pub fn due(&self, round: u32, pace: u32) -> bool {
        self.samples.last().is_none_or(|s| round >= s.round + paced(SAMPLE_ROUNDS, pace))
    }

    /// Keep the turn's screens; samples older than the trend's window are dropped but the newest of them.
    pub fn record(&mut self, me: PlayerId, rivals: &Rivals, pace: u32) {
        let mut nations: Vec<(PlayerId, i64, i64)> = rivals.rivals.iter().map(|r| (r.player, r.score, r.techs.value)).collect();
        nations.push((me, rivals.own_score, rivals.own_techs));
        nations.sort_by_key(|n| n.0 .0);
        self.samples.push(Sample { round: rivals.round, nations });
        let window = paced(TREND_ROUNDS, pace);
        while self.samples.len() > 1 && self.samples[1].round + window <= rivals.round {
            self.samples.remove(0);
        }
    }

    /// The oldest sample within the window that is old enough for a trend.
    fn base(&self, round: u32, pace: u32) -> Option<&Sample> {
        let window = paced(TREND_ROUNDS, pace);
        let least = paced(TREND_MIN_ROUNDS, pace);
        self.samples.iter().find(|s| s.round + window >= round && s.round + least <= round)
    }

    /// The bytes of the book (`strategy_net::Memory::encode`).
    pub fn encode(&self, out: &mut Vec<u8>) {
        out.push(self.samples.len().min(255) as u8);
        for s in self.samples.iter().take(255) {
            out.extend_from_slice(&s.round.to_le_bytes());
            out.push(s.nations.len().min(255) as u8);
            for (p, score, techs) in s.nations.iter().take(255) {
                out.push(p.0);
                out.extend_from_slice(&(*score as i32).to_le_bytes());
                out.extend_from_slice(&(*techs as u16).to_le_bytes());
            }
        }
    }

    /// The book of `bytes` and the bytes read (None: not a book).
    pub fn decode(bytes: &[u8]) -> Option<(Book, usize)> {
        let mut at = 0usize;
        let mut take = |n: usize| -> Option<&[u8]> {
            let s = bytes.get(at..at + n)?;
            at += n;
            Some(s)
        };
        let count = take(1)?[0];
        let mut samples = Vec::with_capacity(usize::from(count));
        for _ in 0..count {
            let round = u32::from_le_bytes(take(4)?.try_into().ok()?);
            let n = take(1)?[0];
            let mut nations = Vec::with_capacity(usize::from(n));
            for _ in 0..n {
                let p = PlayerId(take(1)?[0]);
                let score = i64::from(i32::from_le_bytes(take(4)?.try_into().ok()?));
                let techs = i64::from(u16::from_le_bytes(take(2)?.try_into().ok()?));
                nations.push((p, score, techs));
            }
            samples.push(Sample { round, nations });
        }
        Some((Book { samples }, at))
    }
}

/// The range a truncated average leaves for the sum of `n` values.
fn sum_range(average: i64, n: i64) -> (i64, i64) {
    let base = average * n;
    match average.signum() {
        1 => (base, base + n - 1),
        -1 => (base - (n - 1), base),
        _ => (-(n - 1), n - 1),
    }
}

/// The mean of the other nations of a row (`living` nations, the seat included; None: no other):
/// the guess without the scores, for the measurement of `row`'s sharing.
pub fn others_mean(d: &Demographic, living: u32) -> Option<i64> {
    let n = i64::from(living);
    (n > 1).then(|| {
        let (lo, hi) = sum_range(d.average, n);
        ((lo + hi) / 2 - d.value) / (n - 1)
    })
}

/// The guess of every met rival's value of one row. `living`: nations alive, the seat included;
/// `met`: the met rivals with their scores (by id).
pub fn row(d: &Demographic, living: u32, met: &[(PlayerId, i64)]) -> Vec<Guess> {
    let n = i64::from(living);
    let others = n - 1;
    if others <= 0 || met.is_empty() {
        return met.iter().map(|_| Guess::default()).collect();
    }
    let own = d.value;
    if others == 1 {
        // The other one is the best when the seat is not, else the worst.
        let value = if d.best > own { d.best } else { d.worst };
        return met.iter().map(|_| Guess::exact(value)).collect();
    }
    let (sum_lo, sum_hi) = sum_range(d.average, n);
    let (others_lo, others_hi) = (sum_lo - own, sum_hi - own);
    let lo = d.worst.max(others_lo - (others - 1) * d.best);
    let hi = d.best.min(others_hi - (others - 1) * d.worst).max(lo);
    let mid = (others_lo + others_hi) / 2;
    let known: i64 = met.iter().map(|(_, s)| (*s).max(0)).sum();
    let unmet = others - met.len() as i64;
    let total = known + unmet.max(0) * known / met.len() as i64;
    met.iter()
        .map(|(_, score)| {
            let value = if total > 0 { mid * (*score).max(0) / total } else { mid / others };
            Guess { value: value.clamp(lo, hi), lo, hi }
        })
        .collect()
}

/// The technology counts out of `total` whose share (percent, truncated) is within the literacy
/// guess.
pub fn techs_of_literacy(literacy: &Guess, total: i64) -> Guess {
    let total = total.max(1);
    let first = |l: i64| (l.max(0) * total + 99) / 100;
    let last = |l: i64| (((l.max(0) + 1) * total - 1) / 100).min(total);
    let lo = first(literacy.lo);
    let hi = last(literacy.hi).max(lo);
    let value = ((first(literacy.value) + last(literacy.value).max(first(literacy.value))) / 2).clamp(lo, hi);
    Guess { value, lo, hi }
}

/// The rules' technology costs at the game speed, cheapest first, as running sums (`[0]` = 0).
fn cost_sums(rules: &CompiledRuleset) -> Vec<i64> {
    let mut costs: Vec<i64> = rules.technologies.iter().map(|t| i64::from(t.cost)).collect();
    costs.sort_unstable();
    let mut sums = Vec::with_capacity(costs.len() + 1);
    sums.push(0);
    for c in costs {
        sums.push(sums.last().copied().unwrap_or(0) + c);
    }
    sums
}

/// Thousandths of a technology's rules cost a nation of `cities` pays (speed, map size, cities;
/// `research::tech_cost`, the discount for technologies others know left out).
fn cost_factor(obs: &AIObservation<'_>, cities: u32) -> i64 {
    let size = crate::world::size::WorldSize::of(obs.view.dimensions());
    let inputs = crate::derived::research::TechCostInputs {
        base: 1_000_000,
        difficulty_percent: 100,
        world_percent: crate::classic::world_size::research_percent(size),
        per_city_percent: crate::classic::world_size::tech_cost_per_city(size),
        cities,
        known: 0,
        living: 1,
        speed_percent: obs.speed.science_percent,
    };
    i64::from(crate::derived::research::tech_cost(inputs)) / 1000
}

/// The seat's model of its rivals from its observation's public screens and its book.
pub fn estimate(obs: &AIObservation<'_>, rules: &CompiledRuleset, book: &Book) -> Rivals {
    let public = &obs.public;
    let me = obs.player;
    let round = obs.round;
    let pace = obs.speed.pace_percent;
    let living = public.progress(VictoryKind::Cultural).map_or(1, |c| c.need + 1);
    let met: Vec<(PlayerId, i64)> = public.scores.iter().filter(|(p, _)| *p != me).copied().collect();
    let mut own = [0i64; MEASURES];
    let mut rows: Vec<Vec<Guess>> = Vec::with_capacity(MEASURES);
    for (i, measure) in Measure::ALL.iter().enumerate() {
        match public.row(*measure) {
            Some(d) => {
                own[i] = d.value;
                rows.push(row(d, living, &met));
            }
            None => rows.push(vec![Guess::default(); met.len()]),
        }
    }
    let total = rules.technologies.len().max(1) as i64;
    let literacy = Measure::ALL.iter().position(|m| *m == Measure::Literacy).unwrap_or(0);
    let population = Measure::ALL.iter().position(|m| *m == Measure::Population).unwrap_or(0);
    let own_score = public.score_of(me).unwrap_or(0);
    let own_techs = obs.economy.techs.len() as i64;
    let base = book.base(round, pace);
    let span = base.map_or(0, |b| i64::from(round.saturating_sub(b.round)));
    let window = i64::from(paced(TREND_ROUNDS, pace));
    let then = |p: PlayerId| base.and_then(|b| b.nations.iter().find(|n| n.0 == p)).filter(|_| span > 0);
    let trend = |now: i64, before: i64| (now - before) * window / span.max(1);
    let sums = cost_sums(rules);
    let own_cities = obs.own_cities.len().max(1) as i64;
    let own_citizens: i64 = obs.own_cities.iter().map(|c| i64::from(c.population)).sum::<i64>().max(1);
    let capitals = &public.capitals;
    let congress = obs.congress.as_ref();
    let rivals = met
        .iter()
        .enumerate()
        .map(|(at, (player, score))| {
            let row_of: [Guess; MEASURES] = std::array::from_fn(|i| rows[i][at]);
            let techs = techs_of_literacy(&row_of[literacy], total);
            let science = then(*player).map(|(_, _, before)| {
                let (now, before) = (techs.value.clamp(0, total) as usize, (*before).clamp(0, total) as usize);
                let gained = (sums[now] - sums[before.min(now)]).max(0);
                let cities = (row_of[population].value.max(0) * own_cities / own_citizens).max(1) as u32;
                gained * cost_factor(obs, cities) / 1000 / span.max(1)
            });
            Rival {
                player: *player,
                rows: row_of,
                techs,
                science,
                score: *score,
                score_trend: then(*player).map(|(_, before, _)| trend(*score, *before)),
                era: public.eras.iter().find(|(p, _)| p == player).map(|(_, e)| *e),
                capitals: capitals.iter().filter(|c| c.holder == *player).count() as u32,
                delegates: congress.and_then(|c| c.delegates.iter().find(|(p, _)| p == player).map(|(_, d)| *d)),
                influence_on_us: obs.tourism.influence.iter().find(|v| v.other == *player).map_or(0, |v| v.their_level.index()),
            }
        })
        .collect();
    Rivals {
        round,
        living,
        own,
        own_science: i64::from(obs.economy.income.science),
        own_score,
        own_techs,
        own_trend: then(me).map(|(_, before, _)| trend(own_score, *before)),
        techs_total: total,
        capitals_all: capitals.len() as u32,
        delegates_needed: congress.map(|c| c.needed),
        rivals,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn demo(value: i64, best: i64, average: i64, worst: i64) -> Demographic {
        Demographic { measure: Measure::Goods, value, rank: 1, best, average, worst }
    }

    #[test]
    fn two_nations_give_the_rival_exactly() {
        let met = [(PlayerId(1), 100)];
        assert_eq!(row(&demo(30, 50, 40, 30), 2, &met), vec![Guess::exact(50)], "the seat is the worst: the rival is the best");
        assert_eq!(row(&demo(50, 50, 40, 30), 2, &met), vec![Guess::exact(30)], "the seat is the best: the rival is the worst");
        assert_eq!(row(&demo(40, 40, 40, 40), 2, &met), vec![Guess::exact(40)], "a tie");
        assert_eq!(row(&demo(-3, 5, 1, -3), 2, &met), vec![Guess::exact(5)], "a negative gold");
    }

    #[test]
    fn more_nations_give_a_range_shared_by_the_score() {
        // Six nations: own 10, the others 10, 20, 30, 40, 50 (sum 160, average 26).
        let d = demo(10, 50, 26, 10);
        let met = [(PlayerId(1), 300), (PlayerId(2), 100)];
        let g = row(&d, 6, &met);
        assert!(g.iter().all(|g| g.lo == 10 && g.hi == 50), "{g:?}");
        assert!(g[0].value > g[1].value, "the higher score takes more of the sum: {g:?}");
        // The truncated average: the sum is 156 … 161, the others' 146 … 151.
        assert_eq!(sum_range(26, 6), (156, 161));
        assert_eq!(sum_range(-2, 3), (-8, -6));
        assert_eq!(sum_range(0, 3), (-2, 2));
    }

    #[test]
    fn literacy_gives_the_technology_count() {
        // 81 technologies: every count has its own percent.
        for k in 0..=81i64 {
            let l = k * 100 / 81;
            assert_eq!(techs_of_literacy(&Guess::exact(l), 81), Guess::exact(k), "{k} technologies, literacy {l}");
        }
        let g = techs_of_literacy(&Guess { value: 20, lo: 10, hi: 30 }, 81);
        assert_eq!((g.lo, g.hi), (9, 25));
    }

    #[test]
    fn the_book_keeps_the_window_and_round_trips() {
        let mut book = Book::default();
        let mut rivals = Rivals { own_score: 10, own_techs: 3, ..Default::default() };
        rivals.rivals.push(Rival {
            player: PlayerId(2),
            rows: [Guess::default(); MEASURES],
            techs: Guess::exact(4),
            science: None,
            score: 20,
            score_trend: None,
            era: None,
            capitals: 1,
            delegates: None,
            influence_on_us: 0,
        });
        for round in (0..100).step_by(5) {
            rivals.round = round;
            if book.due(round, 100) {
                book.record(PlayerId(0), &rivals, 100);
            }
        }
        assert_eq!(book.samples.first().map(|s| s.round), Some(65), "the newest sample older than the window stays");
        assert_eq!(book.base(100, 100).map(|s| s.round), Some(70));
        let mut bytes = Vec::new();
        book.encode(&mut bytes);
        assert_eq!(Book::decode(&bytes), Some((book.clone(), bytes.len())));
        assert_eq!(Book::decode(&bytes[..bytes.len() - 1]), None, "a cut book is no book");
    }
}
