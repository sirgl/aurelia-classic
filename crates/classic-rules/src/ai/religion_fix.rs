//! Religion priced by the rules' formulas (`Variant::ReligionFix`,
//! classic/docs/ai-religion-fix.md). Replaces the missionaries, their
//! purchase and adds the inquisitors of `ai::religion`; the pantheon, the
//! prophets and the beliefs stay as they are there.
//!
//! - A city's faith is worth, a turn, what the player's religion pays for it
//!   (`worth`): the founder and enhancer beliefs paying per following city
//!   and per follower (`classic::religion_effects::counted`), and in an own
//!   city the flat yields its majority's beliefs give a following city. The
//!   prices are the allocator's (`ai::value::Prices`: a yield a turn kept to
//!   the last round, in centi-points); a faith in the pool is worth a
//!   faith a turn spread over the rounds left.
//! - A spread is worth the change of that worth when the rules' spread
//!   lands (`derived::religion::apply_spread` on the city's pressure: the
//!   followers gained and the majority flip), from the round the unit
//!   arrives, with the strength it loses in rival land on the way. A missionary
//!   goes to the city where it is worth most; one is bought while the best
//!   city no missionary goes to is worth more than its faith.
//! - An own city whose majority is, or is becoming, another religion is
//!   worth its return (`derived::religion::remove_other_religions`): an
//!   inquisitor goes there and removes the heresy; one is bought when the
//!   return pays its faith. An own city following the religion that a
//!   foreign missionary or prophet nears is worth what its spread would
//!   take: an inquisitor stands guard there (`UnitAbility::ProhibitsSpread`
//!   blocks the spread), bought when that pays.
//!
//! One-off rewards of a spread (the beliefs fired by a spread or a city's
//! first conversion) and the religion's pressure between cities over the
//! rounds are not priced.

use crate::ai::planner::DecisionBudget;
use crate::ai::planner_map::{explore, explored_path, step_points, Blockers, KnownMap, MapScratch, SearchStats, Visit};
use crate::ai::religion::{crowded_tiles, has, march_toward, may_enter};
use crate::ai::value::{Centi, Prices};
use crate::classic::effects::{ClassicModifier, ClassicSelector, ClassicUnitStat};
use crate::command::Command;
use crate::derived::religion::{apply_spread, followers, majority, percent_of, remove_other_religions, touch, Spread};
use crate::ids::{BeliefId, CityHandle, PlayerId, ProductionItem, TileId, UnitKindId};
use crate::knowledge::observation::{AIObservation, CityFaithView, OwnUnit};
use crate::math::MapDimensions;
use crate::rules::content::{ReligionId, UnitAbility};
use crate::rules::effects::{Condition, Modifier, Selector, UnitStat};
use crate::rules::CompiledRuleset;
use crate::world::religion::{CityFaith, Creed};
use crate::world::unit::MOVE_DENOMINATOR;

/// The commands of the religious units and the faith purchase of the
/// variant: the missionaries' spreads and marches, the inquisitors'
/// removals, marches and guards, and at most one purchase (the one whose
/// worth beats its faith by most).
pub fn commands(obs: &AIObservation<'_>, rules: &CompiledRuleset, prices: &Prices) -> Vec<Command> {
    let Some(religion) = obs.faith.overview.religion else {
        return Vec::new();
    };
    let ctx = Ctx { obs, rules, prices, religion, dims: MapDimensions { width: obs.map_width, height: obs.map_height } };
    let mut out = Vec::new();
    let mut scratch = MapScratch::default();
    let mut claimed: Vec<CityHandle> = Vec::new();
    for unit in &obs.own_units {
        if has(rules, unit.kind, UnitAbility::SpreadReligion) && !has(rules, unit.kind, UnitAbility::FoundReligion) {
            out.extend(ctx.missionary(unit, &mut claimed, &mut scratch));
        }
    }
    let mut returning: Vec<CityHandle> = Vec::new();
    let mut guarded: Vec<CityHandle> = Vec::new();
    // An inquisitor already in a city to win back takes it first.
    let heresies: Vec<TileId> = ctx.heresies().iter().map(|(c, _)| c.tile).collect();
    let mut inquisitors: Vec<&OwnUnit> = obs.own_units.iter().filter(|u| has(rules, u.kind, UnitAbility::RemoveHeresy)).collect();
    inquisitors.sort_by_key(|u| !heresies.contains(&u.tile));
    for unit in inquisitors {
        out.extend(ctx.inquisitor(unit, &mut returning, &mut guarded));
    }
    let mut buys: Vec<(Centi, Command)> = Vec::new();
    buys.extend(ctx.missionary_purchase(&claimed, &mut scratch));
    buys.extend(ctx.inquisitor_purchase(&returning, &guarded));
    if let Some((_, cmd)) = buys.into_iter().filter(|(net, _)| *net > 0).max_by_key(|(net, _)| *net) {
        out.push(cmd);
    }
    out
}

/// Faith in the pool, in the allocator's centi-points: a faith a turn kept
/// to the last round is `prices.faith`, so a faith once is that over the
/// rounds left.
pub fn faith_worth(prices: &Prices, faith: u32) -> Centi {
    i64::from(faith) * prices.faith / i64::from(prices.rounds_left.max(1))
}

/// What a city's faith is worth to the player who founded `religion`, a
/// turn, at `prices` (see the module).
pub fn worth(
    obs: &AIObservation<'_>,
    rules: &CompiledRuleset,
    prices: &Prices,
    religion: ReligionId,
    city: &CityFaithView,
    faith: &CityFaith,
) -> Centi {
    let population = city.population.max(1);
    let creed = Creed::Religion(religion);
    let ours = followers(faith, population).iter().find(|(c, _)| *c == creed).map_or(0, |(_, n)| *n);
    let lead = majority(faith, population);
    let foreign = city.owner != obs.player;
    let mut total: Centi = 0;
    for belief in beliefs_of(obs, Some(creed)) {
        for effect in &rules.module.beliefs[belief.0 as usize].effects.passive_player {
            if effect.selector != Selector::Module(ClassicSelector::Founder) {
                continue;
            }
            total += match effect.modifier {
                Modifier::Module(ClassicModifier::AddYieldPerFollowingCity { yield_kind, amount, foreign_only }) => {
                    match lead == Some(creed) && (foreign || !foreign_only) {
                        true => i64::from(amount) * prices.of_yield(yield_kind),
                        false => 0,
                    }
                }
                Modifier::Module(ClassicModifier::AddYieldPerFollowers { yield_kind, per, foreign_only }) => {
                    match foreign || !foreign_only {
                        true => i64::from(ours) * prices.of_yield(yield_kind) / i64::from(per.max(1)),
                        false => 0,
                    }
                }
                Modifier::Module(ClassicModifier::AddHappinessPerFollowingCity { hundredths }) => match lead == Some(creed) {
                    true => i64::from(hundredths) * prices.happiness / 100,
                    false => 0,
                },
                _ => 0,
            };
        }
    }
    let own = obs.own_cities.iter().find(|c| c.handle == city.handle);
    if let Some(own) = own {
        let majority_followers = lead.map_or(0, |m| followers(faith, population).iter().find(|(c, _)| *c == m).map_or(0, |(_, n)| *n));
        for belief in beliefs_of(obs, lead) {
            total += following_city_yields(obs, rules, prices, belief, own, majority_followers);
        }
    }
    total
}

/// The flat yields and happiness a belief gives a city following it, at
/// `prices`: the effects whose conditions the city meets as far as its
/// owner knows them (always, its population, buildings and technologies,
/// the majority's followers); an effect under another condition counts
/// nothing.
fn following_city_yields(
    obs: &AIObservation<'_>,
    rules: &CompiledRuleset,
    prices: &Prices,
    belief: BeliefId,
    city: &crate::knowledge::observation::OwnCity,
    majority_followers: u32,
) -> Centi {
    let holds = |condition: &Condition| match condition {
        Condition::Always => true,
        Condition::MinPopulation(n) => city.population >= *n,
        Condition::HasBuilding(b) => city.buildings.contains(b),
        Condition::HasTechnology(t) => obs.economy.techs.contains(t),
        Condition::LacksTechnology(t) => !obs.economy.techs.contains(t),
        Condition::Module(crate::classic::effects::ClassicCondition::MinFollowers(n)) => majority_followers >= *n,
        _ => false,
    };
    let mut total = 0;
    for effect in &rules.module.beliefs[belief.0 as usize].effects.passive_city {
        let met = holds(&effect.condition) && holds(&effect.second_condition);
        if effect.selector != Selector::Module(ClassicSelector::FollowingCities) || !met {
            continue;
        }
        total += match effect.modifier {
            Modifier::AddYield { yield_kind, amount } => i64::from(amount) * prices.of_yield(yield_kind),
            Modifier::Module(ClassicModifier::AddHappiness { amount }) => i64::from(amount) * prices.happiness,
            _ => 0,
        };
    }
    total
}

/// The beliefs a city of `majority` follows (`religion_effects::beliefs_of`
/// from the religion overview): a religion's, the player's own pantheon.
fn beliefs_of(obs: &AIObservation<'_>, majority: Option<Creed>) -> Vec<BeliefId> {
    match majority {
        Some(Creed::Religion(r)) => obs.faith.overview.religions.iter().find(|s| s.religion == r).map(|s| s.beliefs.clone()).unwrap_or_default(),
        Some(Creed::Pantheon) => obs.faith.overview.pantheon.into_iter().collect(),
        Some(Creed::Atheism) | None => Vec::new(),
    }
}

/// The city's pressure as the rules bring it up to date before a change.
fn touched(city: &CityFaithView) -> CityFaith {
    let mut faith = city.faith.clone();
    touch(&mut faith, city.population);
    faith
}

/// What a spread of `religion` with `pressure` (by a prophet or not) changes
/// in the worth of `city` to the player.
#[allow(clippy::too_many_arguments)]
pub fn spread_gain(
    obs: &AIObservation<'_>,
    rules: &CompiledRuleset,
    prices: &Prices,
    ours: ReligionId,
    city: &CityFaithView,
    spreading: ReligionId,
    pressure: u32,
    prophet: bool,
) -> Centi {
    let before = touched(city);
    let mut after = before.clone();
    let erosion = match prophet {
        true => 0,
        false => percent_of(pressure, percent(&obs.faith.spread.erosion, spreading)),
    };
    let spread = Spread { city: city.handle, religion: spreading, pressure, prophet };
    apply_spread(&mut after, &spread, erosion, &obs.faith.spread.retention);
    worth(obs, rules, prices, ours, city, &after) - worth(obs, rules, prices, ours, city, &before)
}

/// What removing the other religions changes in the worth of an own city.
pub fn heresy_gain(obs: &AIObservation<'_>, rules: &CompiledRuleset, prices: &Prices, ours: ReligionId, city: &CityFaithView) -> Centi {
    let before = touched(city);
    let mut after = before.clone();
    remove_other_religions(&mut after, ours, &obs.faith.spread.retention);
    worth(obs, rules, prices, ours, city, &after) - worth(obs, rules, prices, ours, city, &before)
}

fn percent(list: &[(ReligionId, i32)], religion: ReligionId) -> i32 {
    list.iter().find(|(r, _)| *r == religion).map_or(0, |(_, p)| *p)
}

/// Percent of its strength a unit of `kind` loses for each turn it ends in
/// rival land (its free promotions' `RivalLandStrengthLossPercent`).
fn rival_loss_percent(rules: &CompiledRuleset, kind: UnitKindId) -> u32 {
    let stat = UnitStat::Module(ClassicUnitStat::RivalLandStrengthLossPercent);
    let mut total = 0i32;
    for promotion in &rules.units[kind.0 as usize].free_promotions {
        for effect in &rules.promotions[promotion.0 as usize].effects.passive_unit {
            if let Modifier::ModifyUnitStat { stat: s, amount } = effect.modifier {
                if s == stat {
                    total += amount;
                }
            }
        }
    }
    total.clamp(0, 100) as u32
}

/// A city a unit can reach: the tile it spreads from, the turns there and
/// the percent of its strength it loses on the way.
#[derive(Debug, Clone, Copy)]
struct Reach {
    station: TileId,
    turns: u32,
    lost: u32,
}

struct Ctx<'c, 'a> {
    obs: &'c AIObservation<'a>,
    rules: &'c CompiledRuleset,
    prices: &'c Prices,
    religion: ReligionId,
    dims: MapDimensions,
}

impl<'c, 'a> Ctx<'c, 'a> {
    fn creed(&self) -> Creed {
        Creed::Religion(self.religion)
    }

    fn distance(&self, a: TileId, b: TileId) -> i32 {
        self.dims.distance(a, b).unwrap_or(i32::MAX)
    }

    /// The religion an own unit carries: its record, else the player's
    /// (`derived::religion::unit_religion`).
    fn carried(&self, unit: &OwnUnit) -> Option<ReligionId> {
        let record = self.obs.faith.units.iter().find(|(h, _, _)| *h == unit.handle).map(|(_, r, _)| *r);
        record.or(Some(self.religion))
    }

    /// The religion `owner` founded (the religion overview).
    fn religion_of(&self, owner: PlayerId) -> Option<ReligionId> {
        self.obs.faith.overview.religions.iter().find(|r| r.founder == owner).map(|r| r.religion)
    }

    /// A unit that prohibits spreading and does not carry our religion
    /// stands in or next to the city, as far as the player sees: a foreign
    /// one carries its owner's religion.
    fn guarded_against_us(&self, city: &CityFaithView) -> bool {
        let near = |t: TileId| self.distance(t, city.tile) <= 1;
        let foreign = self.obs.visible_foreign_units.iter().any(|u| {
            near(u.tile) && has(self.rules, u.kind, UnitAbility::ProhibitsSpread) && self.religion_of(u.owner) != Some(self.religion)
        });
        let own = self.obs.own_units.iter().any(|u| {
            near(u.tile) && has(self.rules, u.kind, UnitAbility::ProhibitsSpread) && self.carried(u) != Some(self.religion)
        });
        foreign || own
    }

    /// The cities a spread of our religion may go to: seen now, not
    /// following it, not guarded against it.
    fn spread_targets(&self) -> Vec<&'c CityFaithView> {
        self.obs.faith.cities.iter().filter(|c| c.majority != Some(self.creed()) && !self.guarded_against_us(c)).collect()
    }

    /// The reach of each target from `from` over `map`, for a unit with
    /// `moves_max` points losing `loss` percent a turn ended in rival land
    /// (one exploration; `scratch` keeps its ways).
    #[allow(clippy::too_many_arguments)]
    fn reaches(
        &self,
        map: &KnownMap<'_, '_>,
        from: TileId,
        mover: Option<crate::ids::UnitHandle>,
        kind: UnitKindId,
        moves_max: u16,
        loss: u32,
        targets: &[&CityFaithView],
        scratch: &mut MapScratch,
    ) -> Vec<Option<Reach>> {
        let mut stations: Vec<(TileId, usize)> = Vec::new();
        for (i, city) in targets.iter().enumerate() {
            stations.push((city.tile, i));
            stations.extend(self.dims.neighbors(city.tile).into_iter().map(|t| (t, i)));
        }
        stations.sort_unstable_by_key(|(t, i)| (t.0, *i));
        let crowded = crowded_tiles(self.obs, self.rules, mover, kind);
        let blockers = Blockers::new(self.obs.visible_foreign_units.iter().map(|u| u.tile).collect());
        let limit = DecisionBudget::default().search();
        let mut best: Vec<Option<(TileId, u32)>> = vec![None; targets.len()];
        explore(map, from, &blockers, scratch, limit, &mut SearchStats::default(), &mut |tile, cost| {
            let free = tile == from || crowded.binary_search_by_key(&tile.0, |t| t.0).is_err();
            if free {
                let first = stations.partition_point(|(t, _)| t.0 < tile.0);
                for (t, i) in &stations[first..] {
                    if *t != tile {
                        break;
                    }
                    if best[*i].is_none() {
                        best[*i] = Some((tile, cost));
                    }
                }
            }
            Visit::Continue
        });
        best.into_iter()
            .map(|b| {
                let (station, cost) = b?;
                let rival = self.rival_points(map, from, station, scratch);
                let per_turn = u32::from(moves_max.max(1));
                Some(Reach {
                    station,
                    turns: cost.div_ceil(per_turn),
                    lost: (rival / per_turn * loss).min(100),
                })
            })
            .collect()
    }

    /// Move points of the way to `station` spent entering land the player
    /// may not enter in peace (where a religious unit loses strength).
    fn rival_points(&self, map: &KnownMap<'_, '_>, from: TileId, station: TileId, scratch: &MapScratch) -> u32 {
        let Some(way) = explored_path(scratch, from, station) else {
            return 0;
        };
        let mut points = 0u32;
        for step in way.windows(2) {
            let owner = self.obs.view.tile(step[1]).snapshot().and_then(|s| s.owner);
            if owner.is_some_and(|o| !may_enter(self.obs, o)) {
                points = points.saturating_add(step_points(map, step[0], step[1]).unwrap_or(0));
            }
        }
        points
    }

    /// The worth of a spread with `pressure` at `reach`: the gain from the
    /// round it lands.
    fn spread_value(&self, city: &CityFaithView, pressure: u32, reach: Reach) -> Centi {
        let kept = percent_of(pressure, 100 - reach.lost as i32);
        let gain = spread_gain(self.obs, self.rules, self.prices, self.religion, city, self.religion, kept, false);
        self.prices.later(gain, reach.turns)
    }

    /// The best target of a spread from `from` among the unclaimed ones:
    /// its index in `targets`, reach and worth.
    #[allow(clippy::too_many_arguments)]
    fn best_spread(
        &self,
        from: TileId,
        mover: Option<crate::ids::UnitHandle>,
        kind: UnitKindId,
        moves_max: u16,
        pressure: u32,
        claimed: &[CityHandle],
        scratch: &mut MapScratch,
    ) -> Option<(CityHandle, Reach, Centi)> {
        let targets: Vec<&CityFaithView> = self.spread_targets().into_iter().filter(|c| !claimed.contains(&c.handle)).collect();
        if targets.is_empty() {
            return None;
        }
        let map = match mover {
            Some(unit) => KnownMap::of_unit(&self.obs.view, unit),
            None => KnownMap::of_kind(&self.obs.view, kind),
        };
        let loss = rival_loss_percent(self.rules, kind);
        let reaches = self.reaches(&map, from, mover, kind, moves_max, loss, &targets, scratch);
        targets
            .iter()
            .zip(reaches)
            .filter_map(|(city, reach)| reach.map(|r| (city.handle, r, self.spread_value(city, pressure, r))))
            .max_by_key(|(_, r, value)| (*value, std::cmp::Reverse(r.turns)))
    }

    /// A missionary spreads where it stands when that is its best city's
    /// station, else marches toward it; its city is claimed.
    fn missionary(&self, unit: &OwnUnit, claimed: &mut Vec<CityHandle>, scratch: &mut MapScratch) -> Option<Command> {
        let pressure = self.obs.faith.spread.units.iter().find(|(h, _)| *h == unit.handle).map(|(_, p)| *p)?;
        if self.carried(unit) != Some(self.religion) {
            return None;
        }
        let (city, reach, _) = self.best_spread(unit.tile, Some(unit.handle), unit.kind, unit.moves_max, pressure, claimed, scratch)?;
        claimed.push(city);
        if reach.station == unit.tile {
            return Some(Command::SpreadReligion { unit: unit.handle });
        }
        let map = KnownMap::of_unit(&self.obs.view, unit.handle);
        march_toward(self.obs, self.rules, &map, unit, reach.station, scratch)
    }

    /// The own cities whose majority is, or is becoming, another religion
    /// (another religion has more followers than ours), with the worth of
    /// their return.
    fn heresies(&self) -> Vec<(&'c CityFaithView, Centi)> {
        let creed = self.creed();
        self.obs
            .faith
            .cities
            .iter()
            .filter(|c| c.owner == self.obs.player)
            .filter(|c| {
                let population = c.population.max(1);
                let counts = followers(&c.faith, population);
                let ours = counts.iter().find(|(k, _)| *k == creed).map_or(0, |(_, n)| *n);
                let other = counts.iter().filter(|(k, _)| matches!(k, Creed::Religion(r) if *r != self.religion)).map(|(_, n)| *n).max().unwrap_or(0);
                matches!(c.majority, Some(Creed::Religion(r)) if r != self.religion) || other > ours
            })
            .map(|c| (c, heresy_gain(self.obs, self.rules, self.prices, self.religion, c)))
            .filter(|(_, gain)| *gain > 0)
            .collect()
    }

    /// Own cities following our religion that a foreign missionary or
    /// prophet in sight could reach soon, with what its spread would take.
    fn threatened(&self) -> Vec<(&'c CityFaithView, Centi)> {
        let mut out = Vec::new();
        for city in self.obs.faith.cities.iter().filter(|c| c.owner == self.obs.player && c.majority == Some(self.creed())) {
            let mut loss: Centi = 0;
            for u in &self.obs.visible_foreign_units {
                if !has(self.rules, u.kind, UnitAbility::SpreadReligion) {
                    continue;
                }
                let Some(theirs) = self.religion_of(u.owner).filter(|r| *r != self.religion) else {
                    continue;
                };
                let reach = i32::from(self.rules.units[u.kind.0 as usize].movement) * GUARD_TURNS;
                if self.distance(u.tile, city.tile) > reach + 1 {
                    continue;
                }
                let prophet = has(self.rules, u.kind, UnitAbility::FoundReligion);
                let Some(pressure) = self.foreign_pressure(theirs, u.kind) else {
                    continue;
                };
                let gain = spread_gain(self.obs, self.rules, self.prices, self.religion, city, theirs, pressure, prophet);
                loss = loss.max(-gain);
            }
            if loss > 0 {
                out.push((city, loss));
            }
        }
        out
    }

    /// The pressure a unit of `kind` of a foreign religion adds: a new
    /// missionary's of that religion, scaled to the kind's strength.
    fn foreign_pressure(&self, religion: ReligionId, kind: UnitKindId) -> Option<u32> {
        let missionary = crate::derived::religion::missionary_kind(self.rules)?;
        let base = self.obs.faith.spread.missionary.iter().find(|(r, _)| *r == religion).map(|(_, p)| *p)?;
        let strength = |k: UnitKindId| {
            crate::derived::religion::ability_value(self.rules, k, |a| match a {
                UnitAbility::ReligiousStrength(n) => Some(n),
                _ => None,
            })
            .unwrap_or(1000)
        };
        Some((u64::from(base) * u64::from(strength(kind)) / u64::from(strength(missionary).max(1))) as u32)
    }

    /// An inquisitor removes the heresy of the city it stands in, else
    /// marches to the own city whose return is worth most, else stands guard
    /// in the threatened city worth most (or goes there).
    fn inquisitor(&self, unit: &OwnUnit, claimed: &mut Vec<CityHandle>, guarded: &mut Vec<CityHandle>) -> Option<Command> {
        if self.carried(unit) != Some(self.religion) {
            return None;
        }
        let movement = i32::from(self.rules.units[unit.kind.0 as usize].movement).max(1);
        let turns = |tile: TileId| (self.distance(unit.tile, tile).max(0) + movement - 1) / movement;
        let heresy = self
            .heresies()
            .into_iter()
            .filter(|(c, _)| !claimed.contains(&c.handle))
            .map(|(c, gain)| (c, self.prices.later(gain, turns(c.tile) as u32)))
            .max_by_key(|(_, value)| *value);
        if let Some((city, _)) = heresy {
            claimed.push(city.handle);
            if unit.tile == city.tile {
                return Some(Command::RemoveHeresy { unit: unit.handle });
            }
            return Some(Command::MarchTo { unit: unit.handle, destination: city.tile });
        }
        let guard = self
            .threatened()
            .into_iter()
            .filter(|(c, _)| !guarded.contains(&c.handle))
            .max_by_key(|(c, loss)| (*loss, std::cmp::Reverse(turns(c.tile))));
        let (city, _) = guard?;
        guarded.push(city.handle);
        if unit.tile == city.tile {
            return None;
        }
        Some(Command::MarchTo { unit: unit.handle, destination: city.tile })
    }

    /// The missionary purchase worth most over its faith: from the cities
    /// that may buy one, the best unclaimed city's spread.
    fn missionary_purchase(&self, claimed: &[CityHandle], scratch: &mut MapScratch) -> Option<(Centi, Command)> {
        let kind = crate::derived::religion::missionary_kind(self.rules)?;
        let pressure = self.obs.faith.spread.missionary.iter().find(|(r, _)| *r == self.religion).map(|(_, p)| *p)?;
        let moves_max = self.rules.units[kind.0 as usize].movement.max(1) * MOVE_DENOMINATOR;
        let mut best: Option<(Centi, Command)> = None;
        for (city, cost) in self.obs.faith.spread.missionary_purchase.iter().take(PURCHASE_CITIES) {
            let Some(tile) = self.obs.own_cities.iter().find(|c| c.handle == *city).map(|c| c.tile) else {
                continue;
            };
            let Some((_, _, value)) = self.best_spread(tile, None, kind, moves_max, pressure, claimed, scratch) else {
                continue;
            };
            let net = value - faith_worth(self.prices, *cost);
            if best.as_ref().is_none_or(|(b, _)| net > *b) {
                let cmd = Command::PurchaseWithFaith { player: self.obs.player, city: *city, item: ProductionItem::Unit(kind) };
                best = Some((net, cmd));
            }
        }
        best
    }

    /// The inquisitor purchase worth most over its faith: a heresy no
    /// inquisitor goes to, else a threatened city no inquisitor guards,
    /// bought in the city nearest to it.
    fn inquisitor_purchase(&self, claimed: &[CityHandle], guarded: &[CityHandle]) -> Option<(Centi, Command)> {
        let kind = crate::derived::religion::inquisitor_kind(self.rules)?;
        let movement = i32::from(self.rules.units[kind.0 as usize].movement).max(1);
        let heresies = self.heresies().into_iter().filter(|(c, _)| !claimed.contains(&c.handle));
        let guards = self.threatened().into_iter().filter(|(c, _)| !guarded.contains(&c.handle));
        // Every own inquisitor took a task first: what is left needs a new one.
        let wanted: Vec<(&CityFaithView, Centi)> = heresies.chain(guards).collect();
        let mut best: Option<(Centi, Command)> = None;
        for (target, worth) in wanted {
            let Some((city, cost, distance)) = self
                .obs
                .faith
                .spread
                .inquisitor_purchase
                .iter()
                .filter_map(|(h, cost)| self.obs.own_cities.iter().find(|c| c.handle == *h).map(|c| (*h, *cost, self.distance(c.tile, target.tile))))
                .min_by_key(|(_, _, d)| *d)
            else {
                continue;
            };
            let turns = ((distance.max(0) + movement - 1) / movement) as u32;
            let net = self.prices.later(worth, turns) - faith_worth(self.prices, cost);
            if best.as_ref().is_none_or(|(b, _)| net > *b) {
                best = Some((net, Command::PurchaseWithFaith { player: self.obs.player, city, item: ProductionItem::Unit(kind) }));
            }
        }
        best
    }
}

/// Turns ahead a foreign missionary in sight counts as a threat to a city.
const GUARD_TURNS: i32 = 2;
/// Purchase cities searched for a missionary's best target.
const PURCHASE_CITIES: usize = 3;
