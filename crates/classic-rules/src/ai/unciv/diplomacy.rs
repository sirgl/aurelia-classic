// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Derived from Unciv (https://github.com/yairm210/Unciv), tag 4.22.10, MPL-2.0:
// core/src/com/unciv/logic/automation/civilization/DiplomacyAutomation.kt,
// MotivationToAttackAutomation.kt, DeclareWarTargetAutomation.kt, DeclareWarPlanEvaluator.kt,
// TradeAutomation.kt (`respondToTradeRequests`, `exchangeLuxuries`),
// logic/trade/TradeEvaluation.kt and logic/civilization/diplomacy/DiplomacyManager.kt
// (`relationshipLevel`, the opinion's modifiers).

//! Unciv's diplomacy: war is declared on the known nation (or city-state)
//! whose motivation to attack is highest (relative force, score, distance,
//! our other wars, its friendships with us), at once at 20 and above when the
//! plan holds, or prepared first; peace is offered when the war has lost its
//! motivation; friendships, embassies, open borders, research agreements and
//! defensive pacts are offered by Unciv's wishes; luxuries are swapped; the
//! deals offered to us are answered by Unciv's trade evaluation.
//!
//! What the player cannot see is estimated from what it saw: another
//! nation's force is the force of its units seen (the most at once, fading),
//! Unciv's opinion of another nation is built from the classic relation (years
//! of peace, open borders, friendship, pacts, denunciations, warmonger memory)
//! with Unciv's modifier values. Not played: wars with a partner (team war,
//! joining a war: the wars between third nations are not seen), counteroffers,
//! denunciations by opinion's fall, demands.

use crate::ai::planner::Decision;
use crate::ai::state::AiState;
use crate::ai::trace::{Actor, Reason};
use crate::command::Command;
use crate::ids::PlayerId;
use crate::knowledge::observation::{AIObservation, Relation};
use crate::knowledge::public::PlayerQueries;
use crate::world::player::{ClassicKind, PlayerKind};
use crate::world::relations::DealItem;

use super::construction::force_evaluation;
use super::next_turn::draw;
use super::personality::{UncivPersonality, Value};
use super::sight::Kind;

/// Unciv's `RelationshipLevel`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Unforgivable,
    Enemy,
    Competitor,
    Neutral,
    Favorable,
    Friend,
    Ally,
}

/// The diplomacy's view of the turn.
pub struct Diplomat<'o, 'a> {
    pub obs: &'o AIObservation<'a>,
    pub rules: &'a crate::rules::CompiledRuleset,
    pub state: &'o AiState,
    pub personality: UncivPersonality,
    pub own_force: f32,
    /// The force each met player was seen with (`UncivMind`'s memory).
    pub forces: Vec<(PlayerId, f32)>,
    pub scores: Vec<(PlayerId, i64)>,
    /// War plans in preparation (Unciv's negative `WaryOf`): target, round begun.
    pub plans: Vec<(PlayerId, u32)>,
}

fn act(command: Command, reason: Reason) -> Decision {
    Decision::act(Actor::Player, reason, command)
}

impl<'o, 'a> Diplomat<'o, 'a> {
    fn force_of(&self, p: PlayerId) -> f32 {
        self.forces.iter().find(|(q, _)| *q == p).map_or(0.0, |(_, f)| *f)
    }

    fn score_of(&self, p: PlayerId) -> f32 {
        self.scores.iter().find(|(q, _)| *q == p).map_or(0.0, |(_, s)| *s as f32)
    }

    fn is_major(r: &Relation) -> bool {
        r.kind == PlayerKind::Major
    }

    /// Unciv's `opinionOfOtherCiv` estimated from the classic relation.
    pub fn opinion(&self, r: &Relation) -> f32 {
        let mut opinion = 0.0;
        if !r.at_war {
            let since = self.state.relation(r.other).and_then(|m| m.last_change_round).or(r.contact.met_round).unwrap_or(self.obs.round);
            opinion += (self.obs.round.saturating_sub(since) as f32 * 0.5).min(30.0);
        }
        let borders = u8::from(r.they_grant_entry) + u8::from(r.we_grant_entry);
        opinion += f32::from(borders) * 5.0;
        if r.friends {
            opinion += 35.0;
        }
        if r.defensive_pact {
            opinion += 10.0;
        }
        if r.they_denounced {
            opinion -= 35.0;
        }
        // Our memory of their warmongering (100 points distrust) as Unciv's WarMongerer modifier.
        opinion -= r.warmonger as f32 / 10.0;
        opinion
    }

    /// `relationshipLevel`.
    pub fn level(&self, r: &Relation) -> Level {
        let o = self.opinion(r);
        if o <= -80.0 {
            Level::Unforgivable
        } else if o <= -40.0 || r.at_war {
            Level::Enemy
        } else if o <= -15.0 {
            Level::Competitor
        } else if o >= 80.0 {
            Level::Ally
        } else if o >= 40.0 {
            Level::Friend
        } else if o >= 15.0 {
            Level::Favorable
        } else {
            Level::Neutral
        }
    }

    fn known_cities_of(&self, p: PlayerId) -> Vec<crate::ids::TileId> {
        let mut out: Vec<crate::ids::TileId> = self.obs.visible_foreign_cities.iter().filter(|c| c.owner == p).map(|c| c.tile).collect();
        out.extend(self.obs.view.remembered_cities().filter(|(_, s)| s.owner == p).map(|(t, _)| t));
        out.extend(self.state.seen_cities().iter().filter(|c| c.owner == p).map(|c| c.tile));
        out.sort();
        out.dedup();
        out
    }

    fn at_war_majors(&self, except: PlayerId) -> usize {
        self.obs.relations.iter().filter(|r| r.at_war && Self::is_major(r) && r.other != except).count()
    }

    /// `hasAtLeastMotivationToAttack` from what the player knows.
    pub fn motivation_to_attack(&self, target: &Relation) -> f32 {
        let p = &self.personality;
        let dims = self.obs.view.dimensions();
        let theirs = self.known_cities_of(target.other);
        let pairs: Vec<(crate::ids::TileId, crate::ids::TileId)> = self
            .obs
            .own_cities
            .iter()
            .flat_map(|o| theirs.iter().filter(move |t| dims.distance(o.tile, **t).is_some_and(|d| d <= 12)).map(move |t| (o.tile, *t)))
            .collect();
        if pairs.is_empty() {
            return 0.0;
        }
        let base = 100.0;
        let ours = self.own_force + base;
        let enemies_force: f32 = self.obs.relations.iter().filter(|r| r.at_war && r.other != target.other).map(|r| self.force_of(r.other)).sum();
        let theirs_force = self.force_of(target.other) + base + 0.8 * enemies_force;
        let mut m = -(15.0 * p.inverse_scaled_focus(Value::DeclareWar));
        let ratio = ours / theirs_force.max(1.0);
        m += match ratio {
            r if r > 5.0 => 20.0,
            r if r > 4.0 => 15.0,
            r if r > 3.0 => 12.0,
            r if r > 2.0 => 10.0,
            r if r > 1.8 => 8.0,
            r if r > 1.6 => 6.0,
            r if r > 1.4 => 4.0,
            r if r > 1.2 => 2.0,
            r if r > 0.8 => -5.0,
            r if r > 0.6 => -10.0,
            r if r > 0.4 => -20.0,
            _ => -40.0,
        };
        m -= self.at_war_majors(target.other) as f32 * 20.0;
        m -= f32::from(target.pacts_known) * 2.0;
        if Self::is_major(target) {
            let ratio = self.score_of(target.other) / self.score_of(self.obs.player).max(1.0);
            m += match ratio {
                r if r > 2.0 => 15.0,
                r if r > 1.5 => 10.0,
                r if r > 1.25 => 5.0,
                r if r > 1.0 => 2.0,
                r if r > 0.8 => 0.0,
                r if r > 0.5 => -2.0,
                r if r > 0.25 => -5.0,
                _ => -10.0,
            };
        }
        let min_distance = pairs.iter().filter_map(|(a, b)| dims.distance(*a, *b)).min().unwrap_or(99);
        m += match min_distance {
            d if d > 20 => -10.0,
            d if d > 14 => -8.0,
            d if d > 10 => -3.0,
            _ => 0.0,
        } * p.inverse_scaled_focus(Value::Aggressive);
        if min_distance < 6 {
            m += 5.0 * p.inverse_scaled_focus(Value::Aggressive);
        }
        if target.research_agreement {
            m -= 5.0 * p.scaled_focus(Value::Science) * p.scaled_focus(Value::Commerce);
        }
        if target.friends {
            m -= 10.0 * p.scaled_focus(Value::Loyal);
        }
        if target.defensive_pact {
            m -= 15.0 * p.scaled_focus(Value::Loyal);
        }
        m += match self.level(target) {
            Level::Unforgivable => 10.0,
            Level::Enemy => 5.0,
            Level::Competitor => 2.0,
            Level::Favorable => -2.0,
            Level::Friend => -5.0,
            Level::Ally => -10.0,
            Level::Neutral => 0.0,
        } * p.scaled_focus(Value::Loyal);
        if target.we_denounced {
            m += 5.0 * p.inverse_scaled_focus(Value::Diplomacy);
        }
        match self.plans.iter().find(|(t, _)| *t == target.other) {
            Some((_, since)) => m += (self.obs.round.saturating_sub(*since) + 1) as f32 * p.scaled_focus(Value::Aggressive) / 2.0,
            None => m -= self.plans.len() as f32 * 5.0 * p.inverse_scaled_focus(Value::Aggressive),
        }
        if matches!(target.kind, PlayerKind::Module(ClassicKind::CityState(_))) {
            if let Some(info) = self.obs.city_states.iter().find(|c| c.player == target.other) {
                m -= info.influence as f32 / 10.0 * p.scaled_focus(Value::Diplomacy);
                if info.ally == Some(self.obs.player) {
                    m -= 20.0 * p.scaled_focus(Value::Diplomacy);
                }
            }
        }
        // Attack paths: a land route of fewer than 16 turns from one of our cities.
        m += self.attack_paths_modifier(&pairs);
        m
    }

    /// `getAttackPathsModifier`, reduced: a known land way between the cities.
    fn attack_paths_modifier(&self, pairs: &[(crate::ids::TileId, crate::ids::TileId)]) -> f32 {
        use kernel::ai::map::{find_path, Blockers, KnownMap, MapScratch, PathOutcome, SearchLimit, SearchStats};
        let map = KnownMap::land(&self.obs.view);
        let mut scratch = MapScratch::default();
        let mut stats = SearchStats::default();
        let mut value = -3.0;
        let mut any = false;
        let mut from_cities: Vec<crate::ids::TileId> = pairs.iter().map(|(a, _)| *a).collect();
        from_cities.dedup();
        for from in from_cities.into_iter().take(4) {
            let targets: Vec<crate::ids::TileId> = pairs.iter().filter(|(a, _)| *a == from).map(|(_, b)| *b).collect();
            for to in targets.into_iter().take(3) {
                let goal = self.obs.view.dimensions().neighbors(to).into_iter().next().unwrap_or(to);
                if let PathOutcome::Found { tiles, .. } = find_path(&map, from, goal, &Blockers::default(), &mut scratch, SearchLimit { max_nodes: 2000 }, &mut stats) {
                    if tiles.len() < 32 {
                        value += 3.0;
                        any = true;
                        break;
                    }
                }
            }
        }
        if !any {
            return -50.0;
        }
        value
    }

    /// `evaluateDeclareWarPlan`.
    fn declare_war_plan(&self, target: PlayerId, motivation: f32) -> f32 {
        if self.personality.get(Value::DeclareWar) == 0.0 {
            return -1000.0;
        }
        if let Some((_, since)) = self.plans.iter().find(|(t, _)| *t == target) {
            let planned = (self.obs.round.saturating_sub(*since) + 1) as f32;
            let turns_to_plan = (10.0 - motivation / 10.0).max(3.0);
            return motivation - (turns_to_plan - planned) * 3.0;
        }
        motivation - 40.0
    }
}

/// What the diplomacy decides this turn, and the war plans it begins or ends.
pub struct Diplomacy {
    pub decisions: Vec<Decision>,
    pub begin_plan: Option<PlayerId>,
    pub end_plans: Vec<PlayerId>,
}

/// `DiplomacyAutomation.declareWar`, `offerPeaceTreaty` and the offers of the turn.
pub fn turn(d: &Diplomat<'_, '_>, last_offers: &dyn Fn(PlayerId, u8) -> Option<u32>) -> Diplomacy {
    let mut out = Diplomacy { decisions: Vec::new(), begin_plan: None, end_plans: Vec::new() };
    let obs = d.obs;
    if obs.view.own_kind() != PlayerKind::Major {
        return out;
    }
    respond_to_offers(d, &mut out.decisions);
    declare_war(d, &mut out);
    offer_peace(d, &mut out.decisions);
    offers(d, &mut out.decisions, last_offers);
    out
}

fn declare_war(d: &Diplomat<'_, '_>, out: &mut Diplomacy) {
    let obs = d.obs;
    let rules = d.rules;
    // Plans whose target is gone or at war already end.
    for (t, _) in &d.plans {
        if obs.relation(*t).is_none_or(|r| r.at_war) {
            out.end_plans.push(*t);
        }
    }
    if obs.own_cities.is_empty() || d.personality.get(Value::DeclareWar) == 0.0 || obs.economy.happiness.excess() <= 0 {
        return;
    }
    let military = obs.own_units.iter().filter(|u| Kind { rules, kind: u.kind }.is_military()).count();
    if military < obs.own_cities.len() || military < 4 {
        return;
    }
    if obs.own_cities.iter().map(|c| c.population).sum::<u32>() < 12 {
        return;
    }
    let mut targets: Vec<(&Relation, f32)> = obs
        .relations
        .iter()
        .filter(|r| !r.at_war && r.kind != PlayerKind::Module(ClassicKind::Barbarian))
        .filter(|r| r.peace_treaty_until <= obs.round)
        .filter(|r| !d.known_cities_of(r.other).is_empty())
        .filter(|r| obs.view.accepts(&Command::DeclareWar { player: obs.player, enemy: r.other }))
        .map(|r| (r, d.motivation_to_attack(r)))
        .filter(|(_, m)| *m > 0.0)
        .collect();
    // `chooseDeclareWarTarget`: the highest score first.
    targets.sort_by(|a, b| d.score_of(b.0.other).total_cmp(&d.score_of(a.0.other)).then(a.0.other.cmp(&b.0.other)));
    for (target, motivation) in targets {
        if motivation >= 20.0 && d.declare_war_plan(target.other, motivation) > 0.0 {
            out.decisions.push(act(Command::DeclareWar { player: obs.player, enemy: target.other }, Reason::DeclareWar));
            out.end_plans.push(target.other);
            return;
        }
        let preparing = d.plans.iter().any(|(t, _)| *t == target.other);
        if motivation >= 15.0 && !preparing && motivation - 15.0 > 0.0 {
            out.begin_plan = Some(target.other);
            return;
        }
    }
}

fn offer_peace(d: &Diplomat<'_, '_>, out: &mut Vec<Decision>) {
    let obs = d.obs;
    let enemies: Vec<&Relation> = obs
        .relations
        .iter()
        .filter(|r| r.at_war && r.kind != PlayerKind::Module(ClassicKind::Barbarian))
        // Unciv's `DeclaredWar` flag: no peace in the first 10 turns of a war.
        .filter(|r| obs.round.saturating_sub(r.war_since) >= obs.speed.pace_rounds(10))
        .collect();
    for enemy in enemies {
        if d.motivation_to_attack(enemy) >= 10.0 {
            continue; // We can still fight.
        }
        if obs.visible_foreign_cities.iter().any(|c| c.owner == enemy.other && u32::from(c.hp) * 2 < u32::from(crate::world::city::CITY_BASE_HIT_POINTS)) {
            continue; // About to take their city.
        }
        let warring: f32 = obs.relations.iter().filter(|r| r.at_war).map(|r| d.force_of(r.other)).sum();
        if d.own_force - 0.8 * warring > 0.0 {
            let seed = u64::from(enemy.other.0) + obs.relations.iter().filter(|r| r.at_war).count() as u64 + 123 * u64::from(obs.round);
            if draw(obs.round, seed, 0x7065, 100) > 80 {
                continue;
            }
        }
        let command = Command::ProposeDeal { from: obs.player, to: enemy.other, items: vec![(obs.player, DealItem::Peace)] };
        if obs.view.accepts(&command) {
            out.push(act(command, Reason::MakePeace));
        }
    }
}

/// Unciv's `TradeEvaluation.getTradeAcceptability`, the items the classic
/// deals have: what we receive at Unciv's buy cost, what we give at its sell cost.
fn acceptability(d: &Diplomat<'_, '_>, from: PlayerId, items: &[(PlayerId, DealItem)]) -> i64 {
    let obs = d.obs;
    let Some(rel) = obs.relation(from) else {
        return i64::MIN;
    };
    let level = d.level(rel);
    let gpt = i64::from(obs.economy.income.gold.max(1));
    let inflation = 1000.0 / ((gpt as f64).powf(1.2) + 1660.0) + 0.4;
    let mut theirs = 0i64;
    let mut ours = 0i64;
    let mut peace = false;
    for (giver, item) in items {
        let we_give = *giver == obs.player;
        let mutual = item.is_mutual();
        let value: i64 = match *item {
            DealItem::Gold(g) => i64::from(g),
            DealItem::GoldPerTurn(g) => i64::from(g) * i64::from(crate::systems::diplomacy::DEAL_DURATION) * 4 / 5,
            DealItem::Embassy => if we_give { match level { Level::Unforgivable | Level::Enemy => 300, Level::Competitor => 60, _ => 30 } } else { 30 },
            DealItem::OpenBorders => {
                if we_give {
                    match level {
                        Level::Unforgivable => 10_000,
                        Level::Enemy => 2_000,
                        Level::Competitor => 500,
                        Level::Neutral => 200,
                        _ => 100,
                    }
                } else {
                    100
                }
            }
            DealItem::Resource { resource, amount } => {
                let class = d.rules.resources.get(resource.0 as usize).and_then(|r| r.class);
                let have = obs.economy.resources.available(resource);
                if we_give {
                    if class == Some(crate::rules::content::ResourceClass::Luxury) {
                        if have > 1 { 250 } else { 600 }
                    } else if !obs.relations.iter().any(|r| r.at_war) {
                        50 * i64::from(amount)
                    } else {
                        (0..i64::from(amount)).map(|k| { let n = i64::from(have) - k; if n > 5 { 100 } else { (6 - n).max(1) * 100 } }).sum()
                    }
                } else if class == Some(crate::rules::content::ResourceClass::Luxury) {
                    if have > 0 { 0 } else if obs.economy.happiness.excess() < 0 { 450 } else if obs.economy.happiness.excess() < 10 { 350 } else { 300 }
                } else {
                    50 * i64::from((2 - have).clamp(0, i32::from(amount)))
                }
            }
            DealItem::City(_) => if we_give { i64::MAX / 4 } else { 1_000 },
            DealItem::Peace => {
                peace = true;
                peace_cost_for_them(d, from)
            }
            DealItem::ResearchAgreement => 0,
            DealItem::DefensivePact => if we_give || mutual { if wants_defensive_pact(d, rel) { 0 } else { 100_000 } } else { 0 },
            DealItem::Friendship => if wants_friendship(d, rel) { 0 } else { 100_000 },
            DealItem::ThirdPartyWar(target) => {
                if we_give {
                    let motivation = obs.relation(target).map_or(-100.0, |r| d.declare_war_plan(target, d.motivation_to_attack(r)));
                    if motivation < -10.0 { i64::MAX / 4 } else if motivation < 0.0 { ((-motivation) * 1000.0) as i64 } else { 1000 }
                } else {
                    0
                }
            }
            DealItem::ThirdPartyPeace(_) => 0,
        };
        let adjusted = match item {
            DealItem::Gold(_) | DealItem::GoldPerTurn(_) => value,
            _ => (value as f64 / inflation) as i64,
        };
        if mutual {
            // A mutual item counts once for both sides: Unciv adds a treaty to our side only.
            ours += adjusted;
        } else if we_give {
            ours += adjusted;
        } else {
            theirs += adjusted;
        }
    }
    if !peace {
        match level {
            Level::Enemy => ours = ours * 3 / 2,
            Level::Unforgivable => ours *= 2,
            _ => {}
        }
    }
    theirs - ours
}

/// `evaluatePeaceCostForThem`: positive when we are much stronger and still
/// want this war, negative when they are.
fn peace_cost_for_them(d: &Diplomat<'_, '_>, other: PlayerId) -> i64 {
    let ours = d.own_force.max(1.0);
    let theirs = d.force_of(other).max(1.0);
    if ours * 1.5 >= theirs && theirs * 1.5 >= ours {
        return 0;
    }
    if ours > theirs {
        let rel = d.obs.relation(other);
        if rel.is_none_or(|r| d.motivation_to_attack(r) <= 0.0) {
            return 0;
        }
        let advantage = ours - theirs;
        ((advantage * advantage / theirs) / 2.0) as i64
    } else {
        let advantage = theirs - ours;
        -((advantage * advantage / ours) / 2.0).min(10_000.0) as i64
    }
}

/// `respondToTradeRequests`: accept what is acceptable, refuse the rest (no
/// counteroffer).
fn respond_to_offers(d: &Diplomat<'_, '_>, out: &mut Vec<Decision>) {
    let obs = d.obs;
    for offer in &obs.offers_to_us {
        let accept = acceptability(d, offer.from, &offer.items) >= 0;
        let command = Command::RespondDeal { player: obs.player, offer: offer.id, accept };
        if obs.view.accepts(&command) {
            out.push(act(command, if accept { Reason::AcceptDeal } else { Reason::RefuseDeal }));
        }
    }
}

/// `wantsToSignDeclarationOfFrienship`.
fn wants_friendship(d: &Diplomat<'_, '_>, r: &Relation) -> bool {
    let obs = d.obs;
    if d.level(r) < Level::Favorable {
        return false;
    }
    let friends = obs.relations.iter().filter(|x| x.friends && Diplomat::is_major(x)).count() as f32;
    let known = obs.relations.iter().filter(|x| Diplomat::is_major(x)).count() as f32;
    let alive = known.max(1.0);
    let mut m = d.opinion(r) - 40.0;
    m -= r.warmonger as f32 / 10.0 * d.personality.scaled_focus(Value::Diplomacy);
    let threat = d.force_of(r.other) / d.own_force.max(1.0);
    m += if threat > 2.0 { 10.0 } else if threat > 1.5 { 5.0 } else if threat < 0.5 { -5.0 } else { 0.0 };
    let to_ally = 0.25 * alive * d.personality.scaled_focus(Value::Diplomacy);
    if friends < to_ally {
        m += 10.0 - 10.0 * friends / to_ally.max(0.01);
    } else {
        m -= 120.0 * (friends - to_ally) / (known - to_ally).max(1.0);
    }
    m += obs.relations.iter().filter(|x| x.at_war && Diplomat::is_major(x)).count() as f32 * 10.0;
    m -= d.motivation_to_attack(r).max(m / 2.0) * 2.0;
    m > 0.0
}

/// `wantsToSignDefensivePact`.
fn wants_defensive_pact(d: &Diplomat<'_, '_>, r: &Relation) -> bool {
    let obs = d.obs;
    if d.opinion(r) < 65.0 * d.personality.inverse_scaled_focus(Value::Aggressive) {
        return false;
    }
    let mut m = d.opinion(r) - 80.0;
    let threat = d.force_of(r.other) / d.own_force.max(1.0);
    m += if threat > 2.0 { 10.0 } else if threat > 1.5 { 5.0 } else if threat < 0.5 { -7.0 } else if threat < 1.0 / 1.5 { -3.0 } else { 0.0 };
    m -= 15.0 * f32::from(r.pacts_known);
    m += obs.relations.iter().filter(|x| x.at_war && Diplomat::is_major(x)).count() as f32 * 5.0;
    m > 0.0
}

/// `wantsToOpenBorders`.
fn wants_open_borders(d: &Diplomat<'_, '_>, r: &Relation) -> bool {
    if d.level(r) < Level::Favorable {
        return false;
    }
    d.motivation_to_attack(r) <= d.opinion(r) * d.personality.scaled_focus(Value::Commerce) / 2.0
}

/// The offers of the turn: friendship, embassy, open borders, research
/// agreement, defensive pact, luxury swaps. `last` says the round of our last
/// offer of a kind to a player (Unciv's `Declined…` flags keep 5 turns).
fn offers(d: &Diplomat<'_, '_>, out: &mut Vec<Decision>, last: &dyn Fn(PlayerId, u8) -> Option<u32>) {
    let obs = d.obs;
    let me = obs.player;
    let recent = |p: PlayerId, kind: u8| last(p, kind).is_some_and(|r| obs.round < r + 5);
    let mut push = |to: PlayerId, items: Vec<(PlayerId, DealItem)>, reason: Reason| {
        let command = Command::ProposeDeal { from: me, to, items };
        if obs.view.accepts(&command) {
            out.push(act(command, reason));
        }
    };
    let majors: Vec<&Relation> = obs.relations.iter().filter(|r| Diplomat::is_major(r) && !r.at_war).collect();
    for r in &majors {
        let roll = |salt: u64| draw(obs.round, u64::from(r.other.0) << 8 | u64::from(me.0), salt, 10) + 1;
        if !r.friends && !recent(r.other, 0) && (roll(1) as f32) <= 2.0 * d.personality.scaled_focus(Value::Diplomacy) && wants_friendship(d, r) {
            push(r.other, vec![(me, DealItem::Friendship)], Reason::ProposeDeal);
        }
        if !r.our_embassy && !recent(r.other, 1) && roll(2) >= 7 {
            let mut items = vec![(r.other, DealItem::Embassy)];
            if !r.their_embassy {
                items.push((me, DealItem::Embassy));
            }
            push(r.other, items, Reason::ProposeDeal);
        }
        if r.can_open_borders && !r.we_grant_entry && !r.they_grant_entry && !recent(r.other, 2) && roll(3) >= 7 && wants_open_borders(d, r) {
            push(r.other, vec![(me, DealItem::OpenBorders), (r.other, DealItem::OpenBorders)], Reason::OpenBorders);
        }
        if !r.research_agreement && !recent(r.other, 3) && obs.view.deal_item_problem(me, r.other, DealItem::ResearchAgreement).is_none() {
            push(r.other, vec![(me, DealItem::ResearchAgreement)], Reason::ProposeDeal);
        }
        if !r.defensive_pact && !recent(r.other, 4) && d.opinion(r) > 70.0 * d.personality.inverse_scaled_focus(Value::Aggressive) && (roll(5) as f32) > 7.0 * d.personality.inverse_scaled_focus(Value::Loyal) && wants_defensive_pact(d, r) {
            push(r.other, vec![(me, DealItem::DefensivePact)], Reason::ProposeDeal);
        }
        // `exchangeLuxuries`: a spare luxury of ours for a spare one of theirs we lack.
        if !recent(r.other, 5) && d.level(r) > Level::Enemy {
            let theirs = obs.view.tradeable_luxuries(r.other);
            let ours: Vec<crate::ids::ResourceId> = (0..d.rules.resources.len())
                .map(|i| crate::ids::ResourceId(i as u16))
                .filter(|res| d.rules.resources[res.0 as usize].class == Some(crate::rules::content::ResourceClass::Luxury))
                .filter(|res| obs.economy.resources.tradeable(*res) > 1)
                .filter(|res| !theirs.iter().any(|(t, _)| t == res))
                .collect();
            let want: Vec<crate::ids::ResourceId> = theirs.iter().filter(|(res, n)| *n > 1 && obs.economy.resources.available(*res) <= 0).map(|(res, _)| *res).collect();
            if let (Some(give), Some(take)) = (ours.first(), want.first()) {
                push(r.other, vec![(me, DealItem::Resource { resource: *give, amount: 1 }), (r.other, DealItem::Resource { resource: *take, amount: 1 })], Reason::ProposeDeal);
            }
        }
    }
}

/// Our force (`RankingType.Force`): the force of our military units.
pub fn own_force(obs: &AIObservation<'_>, rules: &crate::rules::CompiledRuleset) -> f32 {
    obs.own_units.iter().filter(|u| Kind { rules, kind: u.kind }.is_military()).map(|u| force_evaluation(rules, u.kind) * f32::from(u.hp) / 100.0).sum()
}

/// The force of each other player's units the player sees now.
pub fn seen_forces(obs: &AIObservation<'_>, rules: &crate::rules::CompiledRuleset) -> Vec<(PlayerId, f32)> {
    let mut out: Vec<(PlayerId, f32)> = Vec::new();
    for u in &obs.visible_foreign_units {
        if !(Kind { rules, kind: u.kind }).is_military() {
            continue;
        }
        let f = force_evaluation(rules, u.kind) * f32::from(u.hp) / 100.0;
        match out.iter_mut().find(|(p, _)| *p == u.owner) {
            Some(entry) => entry.1 += f,
            None => out.push((u.owner, f)),
        }
    }
    out
}

/// The kinds of offer `offers` remembers (for the five-round wait).
pub fn offer_kind(items: &[(PlayerId, DealItem)]) -> Option<u8> {
    items.iter().find_map(|(_, i)| match i {
        DealItem::Friendship => Some(0),
        DealItem::Embassy => Some(1),
        DealItem::OpenBorders => Some(2),
        DealItem::ResearchAgreement => Some(3),
        DealItem::DefensivePact => Some(4),
        DealItem::Resource { .. } => Some(5),
        _ => None,
    })
}
