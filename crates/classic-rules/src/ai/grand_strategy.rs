//! The grand strategy of the leaders (`leaders` and the game's AI,
//! `standard`; classic/docs/ai-leaders.md, M6.2 step 5): a leader
//! pursues one path to victory for a while, chosen from its situation and
//! its personality, and the allocator's prices follow it.
//!
//! Every path is valued in one measure: its chance to win the game. The
//! score path (the time victory) is every leader's, as the game's AI of 16
//! October measures everything by the score; another path counts only for
//! a leader whose personality leans to it above neutral, so a neutral
//! leader plays the score alone. The path is reviewed every
//! `REVIEW_ROUNDS` rounds and changes only when another is clearly better
//! (`SWITCH_PERCENT`): the hysteresis keeps a leader from flipping between
//! paths of almost equal worth.
//!
//! What a path changes: its yield is priced higher (science, culture, gold
//! for the city-states' votes); on the domination path the rivals'
//! original capitals carry their share of the victory in the value of a
//! war (`Stakes`). A capture that would win the game at once is worth the
//! victory to every leader. The personality picks among near-equal
//! options (`ai::taste`) and weighs wars by its aggression and war
//! tolerance (`Stakes`).
//!
//! Everything reads the public victory progress (`PlayerQueries::race`),
//! the player's observation and its memory; nothing hidden.

use crate::ai::planner::TurnBrief;
use crate::ai::planner_strategy::OBJECTIVE_RANGE;
use crate::ai::state::{AiState, Aim};
use crate::ai::value::{Centi, Prices};
use crate::ids::{PlayerId, TechId, TileId};
use crate::knowledge::observation::AIObservation;
use crate::knowledge::public::{CapitalStanding, PlayerQueries, Race};
use crate::rules::content::Personality;
use crate::rules::CompiledRuleset;
use crate::world::victory::VictoryKind;

/// A path to victory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum VictoryPath {
    /// The best score at the last round (the time victory).
    Score,
    /// Every original capital.
    Domination,
    /// The spaceship.
    Science,
    /// Embraced on every civilization.
    Culture,
    /// Elected World Leader.
    Diplomacy,
}

impl VictoryPath {
    pub const ALL: [VictoryPath; 5] =
        [VictoryPath::Score, VictoryPath::Domination, VictoryPath::Science, VictoryPath::Culture, VictoryPath::Diplomacy];

    /// Its code in the AI memory (`kernel::ai::state::Aim::goal`).
    pub fn code(self) -> u8 {
        match self {
            VictoryPath::Score => 0,
            VictoryPath::Domination => 1,
            VictoryPath::Science => 2,
            VictoryPath::Culture => 3,
            VictoryPath::Diplomacy => 4,
        }
    }

    pub fn from_code(code: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.code() == code)
    }

    pub fn name(self) -> &'static str {
        match self {
            VictoryPath::Score => "score",
            VictoryPath::Domination => "domination",
            VictoryPath::Science => "science",
            VictoryPath::Culture => "culture",
            VictoryPath::Diplomacy => "diplomacy",
        }
    }

    fn victory(self) -> VictoryKind {
        match self {
            VictoryPath::Score => VictoryKind::Time,
            VictoryPath::Domination => VictoryKind::Domination,
            VictoryPath::Science => VictoryKind::Science,
            VictoryPath::Culture => VictoryKind::Cultural,
            VictoryPath::Diplomacy => VictoryKind::Diplomatic,
        }
    }

    /// The personality value that leans a leader to this path (None: the
    /// score path, every leader's).
    fn leaning(self, personality: &Personality) -> Option<u8> {
        match self {
            VictoryPath::Score => None,
            VictoryPath::Domination => Some(personality.aggression),
            VictoryPath::Science => Some(personality.science),
            VictoryPath::Culture => Some(personality.culture),
            VictoryPath::Diplomacy => Some(personality.diplomacy),
        }
    }
}

/// The path a player's memory holds (the score before its first review).
pub fn path_of(state: &AiState) -> VictoryPath {
    state.aim.and_then(|aim| VictoryPath::from_code(aim.goal)).unwrap_or(VictoryPath::Score)
}

/// Chances in thousandths.
pub type Permille = i64;

/// Rounds between two reviews of the path (staggered by player).
pub const REVIEW_ROUNDS: u32 = 10;
/// Another path replaces the current one only when its value is this
/// share of the current one's, percent.
pub const SWITCH_PERCENT: i64 = 130;
/// The weight of a path at the top of its leaning (10), percent of the
/// score path's; 0 at neutral (5) and below.
pub const FULL_LEANING_PERCENT: i64 = 200;
/// The weight of the spaceship path for every leader of a planner
/// (`Variant::Planner`), percent of the score path's.
pub const FOUNDATION_PERCENT: i64 = 100;
/// The price of the chosen path's yield, percent.
pub const PATH_YIELD_PERCENT: i64 = 150;
/// The power of the scores in the chance of the time victory: the best
/// score wins, a lead counts more than its share.
pub const SCORE_POWER: u32 = 4;
/// Rounds one war operation takes: forming, marching, the siege.
pub const ROUNDS_PER_CAPITAL: u32 = 30;
/// Chance one war operation takes a rival's original capital known within
/// its reach (`OBJECTIVE_RANGE` of an own city), and farther or not yet
/// found, permille (step 3: of the operations that marched with their
/// army, about a third took their city).
pub const NEAR_CAPITAL_CHANCE: Permille = 350;
pub const FAR_CAPITAL_CHANCE: Permille = 150;
/// Chance of a science, culture or diplomatic victory in reach with no
/// time to spare, permille (rivals race too); more with time to spare.
pub const REACHABLE_CHANCE: Permille = 500;
/// Rounds the spaceship takes once its technologies are known.
pub const SPACESHIP_ROUNDS: u32 = 25;

/// What a player's situation promises on each path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outlook {
    /// By `VictoryPath::code`.
    chances: [Permille; 5],
    /// Science and the economy under it are every leader's foundation
    /// (`FOUNDATION_PERCENT`): the planner's weighing of the paths.
    foundation: bool,
}

impl Outlook {
    pub fn of(obs: &AIObservation<'_>, brief: &TurnBrief, race: &Race, rules: &CompiledRuleset) -> Self {
        let left = if brief.plans {
            crate::ai::value::horizon(obs, crate::ai::value::SciencePricing::Aim)
        } else {
            obs.victories.max_rounds.saturating_sub(obs.round)
        };
        let mut chances = [0; 5];
        for path in VictoryPath::ALL {
            if !obs.victories.enabled(path.victory()) {
                continue;
            }
            chances[usize::from(path.code())] = match path {
                VictoryPath::Score => score_chance(race, obs.round, obs.victories.max_rounds),
                VictoryPath::Domination => domination_chance(race, brief, left),
                VictoryPath::Science if brief.plans => grown_science_chance(obs, rules, left),
                VictoryPath::Science => science_chance(obs, rules, left),
                VictoryPath::Culture => culture_chance(obs, left),
                VictoryPath::Diplomacy => diplomacy_chance(obs),
            };
        }
        Self { chances, foundation: brief.plans }
    }

    pub fn chance(&self, path: VictoryPath) -> Permille {
        self.chances[usize::from(path.code())]
    }

    /// What the path is worth to a leader of `personality`: its chance
    /// weighted by the leaning (the score path at full weight).
    pub fn value(&self, path: VictoryPath, personality: &Personality) -> i64 {
        let weight = match path.leaning(personality) {
            None => 100,
            Some(value) => {
                let leaning = i64::from(value.saturating_sub(Personality::MIDDLE)) * FULL_LEANING_PERCENT / 5;
                // The foundation of every leader: the spaceship weighs as the
                // score, a leaning only adds to it.
                if self.foundation && path == VictoryPath::Science {
                    leaning.max(FOUNDATION_PERCENT)
                } else {
                    leaning
                }
            }
        };
        self.chance(path) * weight
    }

    /// The path to pursue from `current`: the most valuable one when it is
    /// clearly better than the current one (ties to the earlier path).
    pub fn choose(&self, current: VictoryPath, personality: &Personality) -> VictoryPath {
        let mut best = VictoryPath::Score;
        for path in VictoryPath::ALL {
            if self.value(path, personality) > self.value(best, personality) {
                best = path;
            }
        }
        let kept = self.value(current, personality);
        if best != current && self.value(best, personality) * 100 > kept * crate::ai::genome::get(crate::ai::genome::Knob::SwitchPercent) {
            best
        } else {
            current
        }
    }
}

/// The time victory: the best score at the last round, as the scores now
/// promise it — each known score to the power `SCORE_POWER`, our share of
/// their sum. The farther the last round, the less the standing now tells:
/// the chance moves from an even one (one in the civilizations of the
/// game, met or not: each has its capital on the screen) at the start to
/// the standing's at the end.
pub(crate) fn score_chance(race: &Race, round: u32, max_rounds: u32) -> Permille {
    let power = |score: i64| i128::from(score.max(1)).pow(crate::ai::genome::get_u32(crate::ai::genome::Knob::ScorePower));
    let total: i128 = race.scores.iter().map(|(_, s)| power(*s)).sum();
    if total == 0 {
        return 0;
    }
    let standing = (power(race.own_score()) * 1000 / total) as Permille;
    let even = 1000 / race.scores.len().max(race.capitals.len()) as Permille;
    let elapsed = i64::from(round.min(max_rounds)) * 1000 / i64::from(max_rounds.max(1));
    even + (standing - even) * elapsed / 1000
}

/// The rivals' original capitals not ours: every one must fall before the
/// end, each with its chance.
fn domination_chance(race: &Race, brief: &TurnBrief, rounds_left: u32) -> Permille {
    let remaining: Vec<&CapitalStanding> = race.capitals.iter().filter(|c| c.holder != brief.player).collect();
    if remaining.is_empty() {
        return 0;
    }
    let attempts = rounds_left / (ROUNDS_PER_CAPITAL * remaining.len() as u32);
    remaining.iter().fold(1000, |chance, capital| chance * capture_chance(capital, brief, attempts) / 1000)
}

/// The chance to take one capital in `attempts` war operations, each with
/// the chance of a capital known within an operation's reach of an own
/// city, or not.
fn capture_chance(capital: &CapitalStanding, brief: &TurnBrief, attempts: u32) -> Permille {
    let near = capital.tile.is_some_and(|tile| {
        brief.nearest_city(brief.dims, tile).and_then(|c| brief.dims.distance(c.tile, tile)).is_some_and(|d| d <= OBJECTIVE_RANGE)
    });
    let once = if near { NEAR_CAPITAL_CHANCE } else { FAR_CAPITAL_CHANCE };
    let fails = (0..attempts).fold(1000, |left, _| left * (1000 - once) / 1000);
    1000 - fails
}

/// A chance in reach of the time left: `REACHABLE_CHANCE` with no time to
/// spare, up to all of it with every round to spare.
fn reachable(rounds: u32, rounds_left: u32) -> Permille {
    if rounds > rounds_left || rounds_left == 0 {
        return 0;
    }
    let spare = i64::from(rounds_left - rounds) * 1000 / i64::from(rounds_left);
    REACHABLE_CHANCE + (1000 - REACHABLE_CHANCE) * spare / 1000
}

/// The spaceship: the beakers of every unknown technology the Apollo
/// Program and the spaceship parts need (with their prerequisites) at our
/// science now, then the parts.
fn science_chance(obs: &AIObservation<'_>, rules: &CompiledRuleset, rounds_left: u32) -> Permille {
    let beakers = remaining_beakers(obs, rules);
    let science = u64::from(obs.economy.income.science.max(1) as u32);
    let rounds = (beakers.div_ceil(science)).min(u64::from(u32::MAX)) as u32;
    reachable(rounds.saturating_add(SPACESHIP_ROUNDS), rounds_left)
}

/// Every technology the spaceship needs: the technologies its projects and
/// parts require with all their prerequisites.
pub fn space_closure(rules: &CompiledRuleset) -> std::collections::HashSet<TechId> {
    let mut seen = std::collections::HashSet::new();
    let mut open = space_technologies(rules);
    while let Some(tech) = open.pop() {
        if let Some(def) = rules.technologies.get(usize::from(tech.0)) {
            if seen.insert(tech) {
                open.extend(def.requires.iter().copied());
            }
        }
    }
    seen
}

/// Beakers of every unknown technology the spaceship needs (the base costs
/// of the rules).
pub fn remaining_beakers(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> u64 {
    let mut needed: Vec<TechId> = space_technologies(rules);
    let mut beakers: u64 = 0;
    let mut seen = vec![false; rules.technologies.len()];
    while let Some(tech) = needed.pop() {
        let Some(def) = rules.technologies.get(usize::from(tech.0)) else { continue };
        if std::mem::replace(&mut seen[usize::from(tech.0)], true) || obs.economy.techs.contains(&tech) {
            continue;
        }
        beakers += u64::from(def.cost);
        needed.extend(def.requires.iter().copied());
    }
    beakers
}

/// The spaceship's chance when the economy grows as it has grown so far
/// (the planner's, `Variant::Planner`): the science of a young empire is no
/// measure of its reach, so the beakers by the time the ship must start are
/// projected, not divided by today's science. Science grew in proportion to
/// the rounds played so far (`science / round` more beakers each round), so
/// `D` rounds on it adds `science * (D + D² / (2 * round))`. A projection
/// that falls short by half has half the chance of `REACHABLE_CHANCE`, one
/// that covers it has that chance and more.
fn grown_science_chance(obs: &AIObservation<'_>, rules: &CompiledRuleset, rounds_left: u32) -> Permille {
    let Some(rounds) = rounds_left.checked_sub(SPACESHIP_ROUNDS).filter(|r| *r > 0) else { return 0 };
    projected_chance(i64::from(obs.economy.income.science.max(1)), obs.round, rounds, remaining_beakers(obs, rules))
}

/// `grown_science_chance` from its numbers: `rounds` rounds on, at a
/// science of `science` after `played` rounds, against `beakers` needed.
pub(crate) fn projected_chance(science: i64, played: u32, rounds: u32, beakers: u64) -> Permille {
    let (d, played) = (i128::from(rounds), i128::from(played.max(1)));
    let reached = i128::from(science.max(1)) * (d + d * d / (2 * played));
    let ratio = (reached * 1000 / i128::from(beakers.max(1))).min(1_000_000) as i64;
    if ratio >= 1000 {
        REACHABLE_CHANCE + (1000 - REACHABLE_CHANCE) * (ratio - 1000) / ratio
    } else {
        REACHABLE_CHANCE * ratio / 1000
    }
}

/// The technologies the space race victory's projects and parts require.
fn space_technologies(rules: &CompiledRuleset) -> Vec<TechId> {
    let parts = crate::world::victory::spaceship_parts(rules);
    let projects = rules.module.projects.iter().filter(|p| p.victory.as_deref() == Some("VICTORY_SPACE_RACE"));
    let units = rules.units.iter().filter(|u| u.spaceship_project.is_some_and(|p| parts.iter().any(|(q, _)| *q == p)));
    projects.filter_map(|p| p.required_tech).chain(units.filter_map(|u| u.required_tech)).collect()
}

/// Embraced on every civilization: we met them all, and our influence stock
/// (it forgets a part each turn, so it tends to our tourism over the decay) reaches
/// what Embraced needs on each at the present tourism.
fn culture_chance(obs: &AIObservation<'_>, rounds_left: u32) -> Permille {
    let tourism = &obs.tourism;
    let needed = tourism.needed_for_victory as usize;
    if needed == 0 || tourism.influence.len() < needed {
        return 0;
    }
    let mut rounds = 0;
    for view in &tourism.influence {
        match crate::derived::tourism::turns_to_reach(view.influence, view.per_turn, view.needed) {
            Some(turns) => rounds = rounds.max(turns),
            None => return 0,
        }
    }
    reachable(rounds, rounds_left)
}

/// World Leader: the United Nations sits, and our votes — our own and our
/// city-state allies' — reach what it needs.
fn diplomacy_chance(obs: &AIObservation<'_>) -> Permille {
    let Some(congress) = obs.congress.as_ref().filter(|c| c.united_nations && c.needed > 0) else {
        return 0;
    };
    let allies = obs.city_states.iter().filter(|c| c.ally == Some(obs.player)).count() as i64;
    let votes = 2 + allies;
    REACHABLE_CHANCE * votes.min(i64::from(congress.needed)) / i64::from(congress.needed)
}

/// The turn's path of a leader and its prices: the path is reviewed when
/// due (the race read only then), the brief's prices follow the path and
/// its choices the leader's taste (`ai::taste`).
pub fn turn(obs: &AIObservation<'_>, brief: &mut TurnBrief, state: &mut AiState, rules: &CompiledRuleset, personality: &Personality) {
    if review_due(brief) {
        let outlook = Outlook::of(obs, brief, &obs.view.race(), rules);
        let current = path_of(state);
        let path = outlook.choose(current, personality);
        if state.aim.is_none() || path != current {
            state.aim = Some(Aim { goal: path.code(), since_round: brief.round });
        }
    }
    let path = path_of(state);
    brief.prices = brief.prices.map(|prices| on_path(prices, path));
    if brief.plans && path == VictoryPath::Science {
        brief.prices = brief.prices.map(|prices| aim_prices(obs, rules, prices));
    }
    brief.personality = *personality;
}

/// The prices of a planner on the spaceship path, from the aim backward:
/// the spaceship is worth its chance of the whole prize, shared by the
/// beakers it still needs, so a beaker kept to the last round is worth
/// that share of the rounds left; production reaches it through the
/// research process (`PROCESS_SHARE`), as when the technologies are
/// priced by what they open. Never below the prices the score gives.
pub fn aim_prices(obs: &AIObservation<'_>, rules: &CompiledRuleset, mut prices: Prices) -> Prices {
    let chance = i128::from(grown_science_chance(obs, rules, prices.rounds_left));
    let beakers = i128::from(remaining_beakers(obs, rules).max(1));
    let prize = i128::from(prize(&obs.view.race(), obs));
    let worth = (prize * chance / 1000 * i128::from(prices.rounds_left) / beakers).min(i128::from(Centi::MAX / 4)) as Centi;
    prices.science = prices.science.max(worth);
    prices.production = prices.production.max(prices.science * crate::ai::value::PROCESS_SHARE / 100);
    prices
}

/// Every `REVIEW_ROUNDS` rounds (staggered by player) once the player has
/// a city; until its first review a player plays the score.
fn review_due(brief: &TurnBrief) -> bool {
    !brief.cities.is_empty() && (brief.round + u32::from(brief.player.0)).is_multiple_of(crate::ai::genome::get_u32(crate::ai::genome::Knob::ReviewRounds))
}

/// Percent a personality value weighs a war's gain and the unrest it
/// minds: 100 at the neutral 5, `WAR_STEP` more or less per point (0 at 0,
/// 200 at 10).
pub fn leaning(value: u8) -> i64 {
    100 + WAR_STEP * (i64::from(value) - i64::from(Personality::MIDDLE))
}

/// Percent of a war's weight one point of aggression or war tolerance
/// adds.
pub const WAR_STEP: i64 = 20;

/// The prices on a path: the path's yield dearer (the score path and
/// domination change none; wars weigh domination by `Stakes`).
pub fn on_path(mut prices: Prices, path: VictoryPath) -> Prices {
    let dearer = |price: Centi| price * crate::ai::genome::get(crate::ai::genome::Knob::PathYieldPercent) / 100;
    match path {
        VictoryPath::Score | VictoryPath::Domination => {}
        VictoryPath::Science => prices.science = dearer(prices.science),
        VictoryPath::Culture => prices.culture = dearer(prices.culture),
        VictoryPath::Diplomacy => prices.gold = dearer(prices.gold),
    }
    prices
}

/// What wars are worth to a leader beyond the score: its appetite, the
/// unrest of conquest it minds, and the original capitals that bring a
/// victory closer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stakes {
    /// Percent of a war's expected gain that counts (0 to 200; a loss
    /// counts 200 less this).
    pub appetite: i64,
    /// Percent of a conquered city's unrest that counts.
    pub unrest: i64,
    pub capitals: Vec<CapitalStake>,
    /// The same victory in the capitals the leader values but has not
    /// found: what finding one is worth (an embassy reveals it).
    pub unfound: Vec<UnfoundCapital>,
}

/// The victory in one original capital whose tile the player does not
/// know.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnfoundCapital {
    pub founder: PlayerId,
    pub holder: PlayerId,
    /// What its capture adds to the chance of the victory, as a share of
    /// the prize (hundredths of score points), as if it lay far away.
    pub value: Centi,
}

/// The victory in one original capital.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapitalStake {
    pub tile: TileId,
    /// Its capture wins the game: nothing after it counts.
    pub decisive: bool,
    /// What the capture adds to the chance of the victory, as a share of
    /// the prize (hundredths of score points).
    pub value: Centi,
}

impl Stakes {
    /// The game's AI: the score alone.
    pub fn neutral() -> Self {
        Self { appetite: 100, unrest: 100, capitals: Vec::new(), unfound: Vec::new() }
    }

    pub fn capital(&self, tile: TileId) -> Option<&CapitalStake> {
        self.capitals.iter().find(|c| c.tile == tile)
    }

    /// A war's expected gain as the leader weighs it: a gain counts by its
    /// appetite, a loss by the rest of the scale (an eager leader minds a
    /// losing city less, a reluctant one more; neutral: as it is).
    pub fn weigh(&self, gain: Centi) -> Centi {
        if gain >= 0 {
            gain * self.appetite / 100
        } else {
            gain * (200 - self.appetite) / 100
        }
    }

    /// A leader's stakes: its leanings, and the capitals that bring the
    /// domination victory closer — the last one to everybody (it wins the
    /// game), every one on the domination path; known ones as targets,
    /// the others as what finding them is worth.
    pub fn of(obs: &AIObservation<'_>, brief: &TurnBrief, state: &AiState, personality: &Personality) -> Self {
        let mut stakes = Self {
            appetite: leaning(personality.aggression),
            unrest: leaning(Personality::MAX - personality.war_tolerance),
            capitals: Vec::new(),
            unfound: Vec::new(),
        };
        if !obs.victories.enabled(VictoryKind::Domination) {
            return stakes;
        }
        let race = obs.view.race();
        let prize = prize(&race, obs);
        let remaining: Vec<&CapitalStanding> = race.capitals.iter().filter(|c| c.holder != brief.player).collect();
        if let [last] = remaining.as_slice() {
            let value = prize * (1000 - score_chance(&race, obs.round, obs.victories.max_rounds)) / 1000;
            stakes.add(last, true, value);
            return stakes;
        }
        if path_of(state) == VictoryPath::Domination {
            let left = obs.victories.max_rounds.saturating_sub(obs.round);
            let now = domination_chance(&race, brief, left);
            let attempts = left / (ROUNDS_PER_CAPITAL * remaining.len() as u32);
            for capital in remaining {
                let after = now * 1000 / capture_chance(capital, brief, attempts).max(1);
                stakes.add(capital, false, prize * (after - now) / 1000);
            }
        }
        stakes
    }

    /// What finding `host`'s original capital is worth when `host` holds
    /// it (its palace is there: an embassy reveals it): the capital's stake
    /// at the chance one operation takes a capital found far away. Zero
    /// for a capital known, held by another or not valued.
    pub fn finding(&self, host: PlayerId) -> Centi {
        let stake: Centi = self.unfound.iter().filter(|c| c.founder == host && c.holder == host).map(|c| c.value).sum();
        stake * FAR_CAPITAL_CHANCE / 1000
    }

    fn add(&mut self, capital: &CapitalStanding, decisive: bool, value: Centi) {
        match capital.tile {
            Some(tile) => self.capitals.push(CapitalStake { tile, decisive, value }),
            None => self.unfound.push(UnfoundCapital { founder: capital.founder, holder: capital.holder, value }),
        }
    }
}

/// The prize of a victory in the allocator's measure: the whole game's
/// score — the best of ours and the rivals' at the last round, each grown
/// at its pace so far (hundredths of score points).
pub(crate) fn prize(race: &Race, obs: &AIObservation<'_>) -> Centi {
    let round = i64::from(obs.round.max(1));
    race.own_score().max(race.best_rival()) * 100 * i64::from(obs.victories.max_rounds) / round
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::PlayerId;

    fn outlook(chances: [Permille; 5]) -> Outlook {
        Outlook { chances, foundation: false }
    }

    /// A neutral leader plays the score whatever the other paths promise;
    /// a leaning leader takes its path only when it is clearly better.
    #[test]
    fn the_path_follows_the_leaning_with_hysteresis() {
        let neutral = Personality::NEUTRAL;
        let warlike = Personality { aggression: 10, ..Personality::NEUTRAL };
        let sure_conquest = outlook([100, 900, 0, 0, 0]);
        assert_eq!(sure_conquest.choose(VictoryPath::Score, &neutral), VictoryPath::Score);
        assert_eq!(sure_conquest.choose(VictoryPath::Score, &warlike), VictoryPath::Domination);
        // 2 × 150 = 300 against 250: better, not clearly better.
        let close = outlook([250, 150, 0, 0, 0]);
        assert_eq!(close.choose(VictoryPath::Score, &warlike), VictoryPath::Score);
        assert_eq!(close.choose(VictoryPath::Domination, &warlike), VictoryPath::Domination);
        let mild = Personality { aggression: 6, ..Personality::NEUTRAL };
        assert_eq!(outlook([250, 700, 0, 0, 0]).choose(VictoryPath::Score, &mild), VictoryPath::Score, "40 % of 700 is 280");
        assert_eq!(outlook([250, 900, 0, 0, 0]).choose(VictoryPath::Score, &mild), VictoryPath::Domination, "40 % of 900 is 360");
    }

    /// The best score wins: the leader's chance grows faster than its share.
    #[test]
    fn the_time_victory_favours_the_lead() {
        let race = |own: i64| Race { scores: vec![(PlayerId(0), own), (PlayerId(1), 100), (PlayerId(2), 100)], capitals: Vec::new() };
        let at_end = |own: i64| score_chance(&race(own), 300, 300);
        assert_eq!(at_end(100), 333);
        assert!(at_end(150) > 700);
        assert!(at_end(50) < 40);
        assert_eq!(score_chance(&race(150), 0, 300), 333, "at the start the standing tells nothing");
        let halfway = score_chance(&race(150), 150, 300);
        assert!(halfway > 333 && halfway < at_end(150));
    }

    /// A known capital is a target, an unknown one is worth finding: its
    /// stake at the chance of an operation on a far capital, and only when
    /// its founder holds it (an embassy reveals the holder's palace).
    #[test]
    fn an_unknown_capital_is_worth_finding() {
        let standing = |founder: u8, holder: u8, tile: Option<TileId>| CapitalStanding { founder: PlayerId(founder), holder: PlayerId(holder), tile };
        let mut stakes = Stakes::neutral();
        stakes.add(&standing(1, 1, Some(TileId(7))), true, 4_000);
        stakes.add(&standing(2, 2, None), false, 10_000);
        stakes.add(&standing(3, 2, None), false, 6_000);
        assert_eq!(stakes.capitals, vec![CapitalStake { tile: TileId(7), decisive: true, value: 4_000 }]);
        assert_eq!(stakes.unfound.len(), 2);
        assert_eq!(stakes.finding(PlayerId(2)), 10_000 * FAR_CAPITAL_CHANCE / 1000);
        assert_eq!(stakes.finding(PlayerId(1)), 0, "known already");
        assert_eq!(stakes.finding(PlayerId(3)), 0, "its palace is elsewhere");
    }

    /// An eager leader minds a war's gain more and its loss less.
    #[test]
    fn appetite_weighs_gains_and_losses() {
        let eager = Stakes { appetite: 160, ..Stakes::neutral() };
        assert_eq!((eager.weigh(1_000), eager.weigh(-1_000)), (1_600, -400));
        assert_eq!((Stakes::neutral().weigh(1_000), Stakes::neutral().weigh(-1_000)), (1_000, -1_000));
    }

    /// A planner weighs the spaceship at least as the score for every
    /// leader, a leaning leader still more; the game's AI weighs a neutral
    /// leader's spaceship at nothing.
    #[test]
    fn the_foundation_weighs_the_spaceship_for_every_leader() {
        let neutral = Personality::NEUTRAL;
        let scholar = Personality { science: 10, ..Personality::NEUTRAL };
        let mut planner = outlook([0, 0, 400, 0, 0]);
        planner.foundation = true;
        let standard = outlook([0, 0, 400, 0, 0]);
        assert_eq!(standard.value(VictoryPath::Science, &neutral), 0);
        assert_eq!(planner.value(VictoryPath::Science, &neutral), 400 * FOUNDATION_PERCENT);
        assert_eq!(planner.value(VictoryPath::Science, &scholar), 400 * FULL_LEANING_PERCENT);
        // With the time victory off the score has no chance: the spaceship is the path.
        assert_eq!(planner.choose(VictoryPath::Score, &neutral), VictoryPath::Science);
        assert_eq!(standard.choose(VictoryPath::Score, &neutral), VictoryPath::Score);
    }

    /// The chance grows with the economy and with the rounds, and is
    /// continuous at the line where the projection covers the need: no
    /// chicken and egg for an empire whose science today is small.
    #[test]
    fn the_projected_chance_grows_with_the_economy() {
        let need = 100_000;
        // 20 beakers after 100 rounds, 375 rounds on: 20 * (375 + 375² / 200) = 21 560.
        let young = projected_chance(20, 100, 375, need);
        assert!(young > 0 && young < REACHABLE_CHANCE / 2, "{young}");
        assert!(projected_chance(40, 100, 375, need) > young);
        assert!(projected_chance(20, 100, 475, need) > young, "more rounds, more chance");
        assert_eq!(projected_chance(1, 100, 10, need), 0);
        let (short, covered) = (projected_chance(20, 100, 375, 21_580), projected_chance(20, 100, 375, 21_540));
        assert!(short.abs_diff(covered) <= 2 && short >= REACHABLE_CHANCE - 2, "{short} {covered}");
        assert!(projected_chance(1_000, 100, 375, need) < 1000);
        assert_eq!(projected_chance(50, 300, 0, need), 0);
    }

    /// The score path keeps the game's AI's prices; another path makes its
    /// yield dearer.
    #[test]
    fn the_path_prices_its_yield() {
        assert_eq!((leaning(Personality::MIDDLE), leaning(0), leaning(Personality::MAX)), (100, 0, 200));
        let prices = Prices {
            model: crate::ai::value::PriceModel::Tiles,
            score: crate::ai::value::ScorePoints::of_map(2016),
            rounds_left: 120,
            food: 1_001,
            production: 333,
            gold: 77,
            science: 251,
            culture: 199,
            faith: 38,
            happiness: 2_700,
            unhappiness: crate::ai::value::Unhappiness::BASE,
            founding_rounds: 120,
            capped: 0,
            cities: 2,
        };
        assert_eq!(on_path(prices, VictoryPath::Score), prices);
        assert_eq!(on_path(prices, VictoryPath::Science).science, 251 * 150 / 100);
        for path in VictoryPath::ALL {
            assert_eq!(VictoryPath::from_code(path.code()), Some(path));
        }
    }
}
