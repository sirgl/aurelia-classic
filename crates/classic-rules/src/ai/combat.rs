//! Minimal combat decisions over `AIObservation` (M4 track C): which
//! attacks to make, city strikes, promotions, upgrades and what to do with
//! captured cities. Every number comes from the same public previews the
//! interface shows (`PlayerView::attack_preview`); nothing hidden is read.
//! The AI framework (planner, driver) decides when to call these.

use crate::ai::battlefield::{worth, KindStats};
use crate::ai::danger::{self, Place, Source, Threats};
use crate::ai::danger_net::Danger;
use crate::ai::planner::UnitView;
use crate::knowledge::public::PlayerQueries;
use crate::combat::exchange::{CombatPreview, Verdict};
use crate::command::{CityDecision, Command};
use crate::ids::{PlayerId, PromotionId, TileId, UnitHandle};
use crate::knowledge::observation::{AIObservation, OwnUnit};
use crate::rules::CompiledRuleset;
use crate::world::city::CityStatus;

/// A possible attack and its value for the attacker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttackChoice {
    pub unit: UnitHandle,
    pub target: TileId,
    pub score: i32,
    pub preview: CombatPreview,
}

/// Value of an attack: share of the defender's HP taken minus 60 % of the
/// share of the attacker's HP lost; +100 for a kill on the lowest roll.
pub fn attack_score(preview: &CombatPreview) -> i32 {
    let dealt = i32::from(preview.to_defender.1) * 100 / i32::from(preview.defender_hp.max(1));
    let taken = i32::from(preview.to_attacker.1) * 100 / i32::from(preview.attacker_hp.max(1));
    let kill = if preview.verdict == Verdict::DecisiveVictory {
        100
    } else {
        0
    };
    dealt - taken * 60 / 100 + kill
}

/// Tiles of enemies the player is at war with and sees now.
fn enemy_targets(obs: &AIObservation<'_>) -> Vec<TileId> {
    let at_war = |owner: PlayerId| obs.relation(owner).is_some_and(|r| r.at_war);
    let mut tiles: Vec<TileId> = obs
        .visible_foreign_units
        .iter()
        .filter(|u| at_war(u.owner))
        .map(|u| u.tile)
        .chain(
            obs.visible_foreign_cities
                .iter()
                .filter(|c| at_war(c.owner))
                .map(|c| c.tile),
        )
        .collect();
    tiles.sort_by_key(|t| t.0);
    tiles.dedup();
    tiles
}

fn distance(obs: &AIObservation<'_>, a: TileId, b: TileId) -> i32 {
    let dims = crate::math::MapDimensions {
        width: obs.map_width,
        height: obs.map_height,
    };
    dims.distance(a, b).unwrap_or(i32::MAX)
}

/// The best attack of one unit (None: nothing worth it). Melee attacks
/// that are likely lost are skipped; ranged attacks cost nothing.
pub fn best_attack(
    obs: &AIObservation<'_>,
    rules: &CompiledRuleset,
    unit: &OwnUnit,
) -> Option<AttackChoice> {
    if unit.moves_left == 0 || rules.is_civilian(unit.kind) {
        return None;
    }
    let reach = rules
        .units
        .get(unit.kind.0 as usize)
        .map(|d| i32::from(d.range).max(1))
        .unwrap_or(1);
    let mut best: Option<AttackChoice> = None;
    for target in enemy_targets(obs) {
        if distance(obs, unit.tile, target) > reach.max(1) + 8 {
            continue;
        }
        let Some(preview) = obs.view.attack_preview(unit.handle, target) else {
            continue;
        };
        let risky = matches!(
            preview.verdict,
            Verdict::MajorDefeat | Verdict::DecisiveDefeat
        );
        if risky {
            continue;
        }
        let score = attack_score(&preview);
        if best.map(|b| score > b.score).unwrap_or(true) {
            best = Some(AttackChoice {
                unit: unit.handle,
                target,
                score,
                preview,
            });
        }
    }
    best.filter(|b| b.score > 0)
}

/// Ranged attack orders (archers, siege, ships, aircraft) for every own
/// ranged unit with a worthwhile shot, best first (shots soften targets
/// for the charges of `melee_orders`).
pub fn ranged_orders(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Vec<Command> {
    let ranged = |u: &&OwnUnit| {
        rules
            .units
            .get(u.kind.0 as usize)
            .is_some_and(|d| d.ranged_strength > 0 && d.range > 0)
    };
    let mut choices: Vec<AttackChoice> = obs
        .own_units
        .iter()
        .filter(ranged)
        .filter_map(|u| best_attack(obs, rules, u))
        .collect();
    choices.sort_by_key(|c| (-c.score, c.unit.index));
    choices
        .into_iter()
        .map(|c| Command::RangedAttack {
            unit: c.unit,
            target: c.target,
        })
        .collect()
}

/// Melee orders (attack the tile's defender) for the units whose best
/// attack is a charge. Use after `ranged_orders`.
pub fn melee_orders(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Vec<Command> {
    let mut out = Vec::new();
    for unit in &obs.own_units {
        let ranged = rules
            .units
            .get(unit.kind.0 as usize)
            .is_some_and(|d| d.ranged_strength > 0 && d.range > 0);
        if ranged {
            continue;
        }
        let Some(choice) = best_attack(obs, rules, unit) else {
            continue;
        };
        if let Some(city) = obs
            .visible_foreign_cities
            .iter()
            .find(|c| c.tile == choice.target)
        {
            out.push(Command::AttackCity {
                attacker: unit.handle,
                city: city.handle,
            });
        } else if let Some(foe) = obs
            .visible_foreign_units
            .iter()
            .find(|u| u.tile == choice.target)
        {
            out.push(Command::AttackUnit {
                attacker: unit.handle,
                defender: foe.handle,
            });
        }
    }
    out
}

/// Each own city strikes the visible enemy it hurts most.
pub fn city_strikes(obs: &AIObservation<'_>) -> Vec<Command> {
    let mut out = Vec::new();
    let targets = enemy_targets(obs);
    for city in &obs.own_cities {
        let best = targets
            .iter()
            .filter_map(|t| {
                obs.view
                    .city_strike_damage(city.handle, *t)
                    .map(|d| (d, *t))
            })
            .max_by_key(|(d, t)| (*d, std::cmp::Reverse(t.0)));
        if let Some((_, target)) = best {
            out.push(Command::CityStrike {
                city: city.handle,
                target,
            });
        }
    }
    out
}

/// Promotion choices: heal a badly wounded unit when it can, else the
/// first available promotion in content order (deterministic).
pub fn promotion_choices(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Vec<Command> {
    let mut out = Vec::new();
    for unit in &obs.own_units {
        let available = obs.view.promotions_available(unit.handle);
        if available.is_empty() {
            continue;
        }
        let heals = |p: &PromotionId| rules.promotions[p.0 as usize].heals_instantly();
        let heal = available.iter().copied().find(heals);
        let pick = match heal {
            Some(p) if unit.hp < 50 => p,
            _ => *available.iter().find(|p| !heals(p)).unwrap_or(&available[0]),
        };
        out.push(Command::Promote {
            unit: unit.handle,
            promotion: pick,
        });
    }
    out
}

/// Upgrades while the treasury stays above `reserve` gold and the
/// strategic resources of this turn's upgrades last (each offer counts the
/// empire's free copies alone).
pub fn upgrade_orders(obs: &AIObservation<'_>, rules: &CompiledRuleset, reserve: u32) -> Vec<Command> {
    let mut gold = obs.economy.treasury;
    let mut taken: Vec<(crate::ids::ResourceId, i32)> = Vec::new();
    let mut out = Vec::new();
    for unit in &obs.own_units {
        let Some((to, price)) = obs.view.upgrade_offer(unit.handle) else {
            continue;
        };
        if gold < price + reserve {
            continue;
        }
        let extra = extra_resources(rules, unit.kind, to);
        let fits = extra.iter().all(|(r, n)| {
            let already: i32 = taken.iter().filter(|(t, _)| t == r).map(|(_, k)| *k).sum();
            obs.economy.resources.available(*r) - already >= *n
        });
        if !fits {
            continue;
        }
        gold -= price;
        taken.extend(extra);
        out.push(Command::UpgradeUnit { unit: unit.handle });
    }
    out
}

/// Copies the upgrade `from → to` takes beyond what the old unit holds.
fn extra_resources(
    rules: &CompiledRuleset,
    from: crate::ids::UnitKindId,
    to: crate::ids::UnitKindId,
) -> Vec<(crate::ids::ResourceId, i32)> {
    let held = &rules.units[from.0 as usize].required_resources;
    let mut out = Vec::new();
    for (r, n) in &rules.units[to.0 as usize].required_resources {
        let freed: u16 = held.iter().filter(|(h, _)| h == r).map(|(_, k)| *k).sum();
        let extra = i32::from(*n) - i32::from(freed);
        if extra > 0 {
            out.push((*r, extra));
        }
    }
    out
}

/// Citizens of a captured city the AI annexes; a smaller one is a puppet.
pub const ANNEX_POPULATION: u32 = 4;

/// Captured cities: keep cities of `ANNEX_POPULATION` citizens and more
/// (annex), puppet the rest; raze nothing (warmonger memory and lost land
/// outweigh it). A nation that may not annex (Venice) puppets every
/// city.
pub fn conquest_decisions(obs: &AIObservation<'_>) -> Vec<Command> {
    obs.own_cities
        .iter()
        .filter(|c| c.status == CityStatus::AwaitingDecision)
        .map(|c| Command::DecideCity {
            player: obs.player,
            city: c.handle,
            decision: if c.population >= ANNEX_POPULATION && obs.economy.may_annex {
                CityDecision::Annex
            } else {
                CityDecision::Puppet
            },
        })
        .collect()
}

/// Siege units with a visible enemy in range set up (they shoot after).
pub fn stance_orders(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Vec<Command> {
    let targets = enemy_targets(obs);
    obs.own_units
        .iter()
        .filter(|u| u.moves_left > 0 && obs.view.must_set_up_first(u.handle))
        .filter(|u| {
            let range = ranged_range(rules, u.kind).unwrap_or(0);
            targets.iter().any(|t| distance(obs, u.tile, *t) <= range)
        })
        .map(|u| Command::SetUpRangedAttack { unit: u.handle })
        .collect()
}

/// Range of a ranged unit kind (None: it fights in melee).
pub fn ranged_range(rules: &CompiledRuleset, kind: crate::ids::UnitKindId) -> Option<i32> {
    let def = rules.units.get(kind.0 as usize)?;
    (def.ranged_strength > 0 && def.range > 0).then_some(i32::from(def.range))
}

/// The best shot of the player's ranged `unit` at one of `targets` by the
/// public preview (None: no target worth a shot).
pub fn best_shot(
    view: &crate::knowledge::view::PlayerView<'_>,
    unit: UnitHandle,
    targets: &[TileId],
) -> Option<(TileId, i32)> {
    targets
        .iter()
        .filter_map(|t| {
            view.attack_preview(unit, *t)
                .map(|p| (*t, attack_score(&p)))
        })
        .filter(|(_, score)| *score > 0)
        .max_by_key(|(t, score)| (*score, std::cmp::Reverse(t.0)))
}

/// The fixes of the combat defects a variant carries
/// (classic/docs/ai-combat-fix.md, `TurnBrief::fight`): one notion of a
/// unit's danger where it ends its turn — its expected loss before its next
/// turn, read by `source` (`Exposure`) — in every decision to stay, strike
/// from, retreat to, heal at or guard.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct FightFix {
    pub source: Source,
    /// The wounded leave the enemy's reach: a unit below the one health of
    /// the wounded (`planner_units::HEAL_BELOW_HP`) steps to the reachable
    /// tile of the least loss, heals in the nearest city not in danger, and
    /// a guard is released when its death there costs more than it saves.
    pub retreat: bool,
    /// A strike is worth its expected exchange less the striker's extra
    /// loss at the tile it strikes from (`Exposure::strike_cost`): the
    /// advisor's shots, the units' own shots and the front's strikes.
    pub reply: bool,
    /// The advisor's shots and city strikes focus fire: the target that the
    /// fewest shots kill first, no shot at a target already expected dead,
    /// the rest at the target closest to death (`focus_plan`).
    pub focus: bool,
    /// A charge is judged by its preview's value (`strike_value`), the same
    /// rule as the front's, not by the planning power ratio.
    pub odds: bool,
    /// How the units choose their promotions (`ai::promo_fit`); not a fix
    /// of the danger's sight (`any`).
    pub promo: Promotions,
}

/// How the units choose their promotions (`FightFix::promo`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Promotions {
    /// The game's AI: at the end of the turn the instant heal below half
    /// health, else the first in content order (`promotion_choices`).
    #[default]
    Ordered,
    /// `Ordered`, and before a unit moves and strikes the instant heal when
    /// the healed unit takes a city or kills a unit this turn that it does
    /// not take without it (`promo_fit::before_unit`).
    HealFirst,
    /// Every promotion priced by its effect in the fights in view, at the
    /// unit's decision and at the end of the turn (`promo_fit`).
    Priced,
}

impl FightFix {
    pub const NONE: FightFix = FightFix { source: Source::Search, retreat: false, reply: false, focus: false, odds: false, promo: Promotions::Ordered };

    /// These fixes with the danger read by `source`.
    pub const fn read_by(self, source: Source) -> FightFix {
        FightFix { source, ..self }
    }

    /// Every fix, the danger read by `source`.
    pub const fn all(source: Source) -> FightFix {
        FightFix { source, retreat: true, reply: true, focus: true, odds: true, promo: Promotions::Ordered }
    }

    /// Some fix is on: the turn keeps the sight the danger reads
    /// (`TurnBrief::danger`).
    pub fn any(self) -> bool {
        self.retreat || self.reply || self.focus || self.odds
    }
}

/// One unit's danger where it may end its turn: its expected loss before
/// its next turn — the damage and the chance of its death times its worth,
/// in hit points (`danger::expected_loss`, `battlefield::worth`) — read from
/// the threats by the fix's `Source`.
pub struct Exposure<'t, 'v, 'a> {
    threats: &'t Threats<'v, 'a>,
    rules: &'a CompiledRuleset,
    source: Source,
    pub unit: OwnUnit,
    pub worth: i32,
}

impl<'t, 'v, 'a> Exposure<'t, 'v, 'a> {
    pub fn of(threats: &'t Threats<'v, 'a>, rules: &'a CompiledRuleset, source: Source, unit: &OwnUnit) -> Self {
        let worth = worth(&KindStats::of(rules, unit.kind), unit.level);
        Self { threats, rules, source, unit: *unit, worth }
    }

    /// The danger of the unit ending its turn on `tile` with `hp`, after a
    /// strike there or not; `hurt` the enemies the strike changed (0: gone).
    pub fn danger(&self, tile: TileId, hp: u16, struck: bool, hurt: &[(TileId, u16)]) -> Danger {
        let end = danger::after(&self.unit, tile, hp, struck);
        let posture = self.threats.posture_with(&self.unit, tile);
        let place = Place { tile, posture: &posture, hurt };
        self.threats.danger_by(self.source, self.rules, &end, &place)
    }

    /// The expected loss of the unit ending its turn on `tile` (`danger`).
    pub fn loss(&self, tile: TileId, hp: u16, struck: bool, hurt: &[(TileId, u16)]) -> i32 {
        danger::expected_loss(self.danger(tile, hp, struck, hurt), self.worth)
    }

    /// The tile of `stops` where the unit as it is now loses least, and the
    /// loss; ties by `rank` (lower first), then the tile.
    pub fn safest(&self, stops: &[TileId], rank: impl Fn(TileId) -> i32) -> Option<(TileId, i32)> {
        stops
            .iter()
            .map(|t| (*t, self.loss(*t, self.unit.hp, false, &[])))
            .min_by_key(|(t, loss)| (*loss, rank(*t), *t))
    }

    /// What a strike from `from` costs the striker beyond its best place
    /// without it: its loss there after the strike (`hp_after`, the foe on
    /// `foe` hurt to `foe_left`) less the loss at the safest of `stops`
    /// (`safe`, precomputed by `safest`).
    pub fn strike_cost(&self, from: TileId, hp_after: u16, foe: TileId, foe_left: u16, safe: i32) -> i32 {
        self.loss(from, hp_after, true, &[(foe, foe_left)]) - safe
    }
}

/// The value of a strike's exchange for the striker, in hit points of a
/// unit worth 100 (the scale of `danger::expected_loss`): the expected
/// damage dealt, the foe's worth when it is expected dead, less the damage
/// taken. `foe_worth` None: a city (it is not killed by the strike).
pub fn strike_value(dealt: u16, taken: u16, foe_hp: u16, foe_worth: Option<i32>) -> i32 {
    let kill = foe_worth.filter(|_| dealt >= foe_hp).unwrap_or(0);
    i32::from(dealt.min(foe_hp)) + kill - i32::from(taken)
}

/// Most tiles a unit may still walk this turn (every step on a road).
pub fn steps_left(view: &crate::knowledge::view::PlayerView<'_>, unit: &OwnUnit) -> i32 {
    let profile = view.own_unit_profile(unit.handle);
    let step = profile.map_or(1, |p| p.routes.road_points.max(1));
    (u32::from(unit.moves_left) / step) as i32
}

/// An enemy at war in the unit's sight: a military unit or a city.
pub fn foe_in_sight(view: &UnitView<'_, '_>) -> bool {
    let local = view.local;
    let rules = local.view.rules();
    let at_war = |owner: PlayerId| local.relation(owner).is_some_and(|r| r.at_war);
    local.foreign_units.iter().any(|u| at_war(u.owner) && crate::ai::planner::role_of(rules, u.kind) == crate::ai::planner::AiRole::Military)
        || local.foreign_cities.iter().any(|c| at_war(c.owner))
}

/// The threats a unit decision reads its danger from (`Threats::around`,
/// the unit a mover); None without the turn's sight or with no enemy in the
/// unit's sight (nothing threatens it then).
pub fn unit_threats<'o, 'a>(view: &UnitView<'o, 'a>) -> Option<Threats<'o, 'a>> {
    let sight = view.brief.danger.as_ref()?;
    if !foe_in_sight(view) {
        return None;
    }
    let unit = &view.local.unit;
    let movers = [(unit.tile, steps_left(&view.local.view, unit))];
    let started = prof::now_ns();
    let threats = Threats::around(view.local, view.local.view.rules(), view.brief, sight, &movers);
    danger::note_build(prof::now_ns().saturating_sub(started));
    Some(threats)
}

/// Tiles the unit of a decision may end its move on this turn (its own
/// first): the move preview, where it may stop.
pub fn unit_stops(view: &UnitView<'_, '_>) -> Vec<TileId> {
    let rules = view.local.view.rules();
    let unit = view.local.unit;
    let mut stops: Vec<TileId> = view
        .local
        .view
        .own_reachable(unit.handle)
        .into_iter()
        .map(|(t, _)| t)
        .filter(|t| *t == unit.tile || crate::ai::planner_units::can_stop(view, rules, *t))
        .collect();
    stops.sort_by_key(|t| (*t != unit.tile, *t));
    if stops.is_empty() {
        stops.push(unit.tile);
    }
    stops
}

/// One shot the advisor may order: a ranged unit's or a city's.
#[derive(Debug, Clone)]
struct Shot {
    command: Command,
    /// The unit's index or the city's, for a deterministic order.
    key: (bool, u32),
    target: TileId,
    /// Expected damage (the preview's).
    dealt: u16,
    /// The shooter's extra loss for the shot (`Exposure::strike_cost`; 0 for
    /// a city or without `FightFix::reply`).
    cost: i32,
}

/// An enemy the advisor may shoot.
#[derive(Debug, Clone, Copy)]
struct Mark {
    tile: TileId,
    /// Health left as the plan has it.
    left: u16,
    /// Its worth (None: a city, which a shot does not take).
    worth: Option<i32>,
}

/// The advisor's city strikes and shots with the fight fix: each shot worth
/// its exchange less its cost to the shooter (`FightFix::reply`), the shots
/// focused (`FightFix::focus`, `focus_plan`) or each shooter at its best.
fn fixed_shots(input: &crate::ai::system::AdvisorInput<'_, '_>, fix: FightFix) -> (Vec<Command>, Vec<Command>) {
    let obs = input.observation;
    let rules = input.rules;
    let targets = enemy_targets(obs);
    let at_war = |owner: PlayerId| obs.relation(owner).is_some_and(|r| r.at_war);
    // The enemies: a city on its tile, else the strongest unit there (the tile's defender).
    let mut marks: Vec<Mark> = Vec::new();
    for tile in &targets {
        if let Some(city) = obs.visible_foreign_cities.iter().find(|c| c.tile == *tile) {
            marks.push(Mark { tile: *tile, left: city.hp, worth: None });
            continue;
        }
        let defender = obs
            .visible_foreign_units
            .iter()
            .filter(|u| u.tile == *tile && at_war(u.owner))
            .max_by_key(|u| (worth(&KindStats::of(rules, u.kind), 0), u.hp));
        if let Some(u) = defender {
            marks.push(Mark { tile: *tile, left: u.hp, worth: Some(worth(&KindStats::of(rules, u.kind), 0)) });
        }
    }
    let mut shots: Vec<Shot> = Vec::new();
    for city in &obs.own_cities {
        for target in &targets {
            let Some(p) = obs.view.city_strike_preview(city.handle, *target) else { continue };
            let command = Command::CityStrike { city: city.handle, target: *target };
            shots.push(Shot { command, key: (false, city.handle.index), target: *target, dealt: p.to_defender.1, cost: 0 });
        }
    }
    let besieged = besieged_cities(input);
    let shooters: Vec<&OwnUnit> = obs.own_units.iter().filter(|u| u.moves_left > 0 && ranged_range(rules, u.kind).is_some() && !rules.is_civilian(u.kind)).collect();
    let threats = fix.reply.then(|| {
        let movers: Vec<(TileId, i32)> = shooters.iter().map(|u| (u.tile, steps_left(&obs.view, u))).collect();
        Threats::build_with(obs, rules, input.brief, input.state.seen_cities(), &movers)
    });
    for unit in &shooters {
        let reach = crate::ai::siege_tactics::range_of(rules, unit, input.brief.siege).unwrap_or(1);
        let exposure = threats.as_ref().map(|t| Exposure::of(t, rules, fix.source, unit));
        let safe = exposure.as_ref().and_then(|e| e.safest(&advisor_stops(obs, rules, unit), |t| i32::from(t != unit.tile)).map(|(_, loss)| loss));
        for target in &targets {
            if distance(obs, unit.tile, *target) > reach {
                continue;
            }
            let Some(p) = obs.view.attack_preview(unit.handle, *target) else { continue };
            let Some(mark) = marks.iter().find(|m| m.tile == *target) else { continue };
            let dealt = p.to_defender.1;
            let cost = match (&exposure, safe) {
                (Some(e), Some(safe)) => e.strike_cost(unit.tile, unit.hp, *target, mark.left.saturating_sub(dealt), safe),
                _ => 0,
            };
            // A shot at a city its siege group wears down is worth the city's share of
            // its damage (`SiegeFixes::worth`): the same as its cost scaled down by that share.
            let cost = match besieged.iter().find(|(t, _)| t == target) {
                Some((_, population)) if mark.worth.is_none() && dealt > 0 => {
                    cost * i32::from(dealt) / crate::ai::front::city_damage_worth(dealt, *population).max(1)
                }
                _ => cost,
            };
            let command = Command::RangedAttack { unit: unit.handle, target: *target };
            shots.push(Shot { command, key: (true, unit.handle.index), target: *target, dealt, cost });
        }
    }
    let planned = if fix.focus { focus_plan(&shots, &mut marks) } else { best_each(&shots, &marks) };
    let (cities, units): (Vec<Shot>, Vec<Shot>) = planned.into_iter().partition(|s| !s.key.0);
    (cities.into_iter().map(|s| s.command).collect(), units.into_iter().map(|s| s.command).collect())
}

/// The enemy cities in sight whose siege has a healthy melee member of a front next to it
/// (`front::GROUP_RADIUS`; a unit on an assault or a marching operation of that city), with
/// their population (`SiegeFixes::worth`; none without it).
fn besieged_cities(input: &crate::ai::system::AdvisorInput<'_, '_>) -> Vec<(TileId, u32)> {
    if !input.brief.war.siege.worth {
        return Vec::new();
    }
    let obs = input.observation;
    let state = input.state;
    let dims = obs.view.dimensions();
    let at_war = |owner: PlayerId| obs.relation(owner).is_some_and(|r| r.at_war);
    obs.visible_foreign_cities
        .iter()
        .filter(|c| at_war(c.owner))
        .filter(|c| {
            obs.own_units.iter().any(|u| {
                let task = state.task_of(u.id).map(|t| t.task);
                let of_city = matches!(task, Some(crate::ai::state::UnitTask::Assault { target }) | Some(crate::ai::state::UnitTask::Operation { target }) if target == c.tile);
                of_city
                    && crate::ai::front::is_member(state, task)
                    && ranged_range(input.rules, u.kind).is_none()
                    && u.hp >= crate::ai::planner_units::HEAL_BELOW_HP
                    && dims.distance(u.tile, c.tile).is_some_and(|d| d <= crate::ai::front::GROUP_RADIUS)
            })
        })
        .map(|c| (c.tile, c.population))
        .collect()
}

/// Tiles a ranged unit may end its move on this turn as the turn starts
/// (its own first): no foreign unit, no own unit of its stack class, land.
fn advisor_stops(obs: &AIObservation<'_>, rules: &CompiledRuleset, unit: &OwnUnit) -> Vec<TileId> {
    let class = crate::world::stacking::stack_class(rules, unit.kind);
    let kinds = obs.view.terrain_kinds();
    let free = |t: TileId| {
        let water = obs.view.tile(t).snapshot().is_none_or(|s| kinds.is_water(s.terrain.0));
        let own = obs.own_units.iter().any(|u| u.id != unit.id && u.tile == t && crate::world::stacking::stack_class(rules, u.kind) == class);
        !water && !own && !obs.visible_foreign_units.iter().any(|u| u.tile == t)
    };
    let mut stops: Vec<TileId> = obs.view.own_reachable(unit.handle).into_iter().map(|(t, _)| t).filter(|t| *t == unit.tile || free(*t)).collect();
    stops.sort_by_key(|t| (*t != unit.tile, *t));
    if stops.is_empty() {
        stops.push(unit.tile);
    }
    stops
}

/// The value of `shot` at `mark` as the plan has it: its exchange less its cost.
fn shot_value(shot: &Shot, mark: &Mark) -> i32 {
    let dealt = if mark.worth.is_none() { shot.dealt.min(mark.left.saturating_sub(1)) } else { shot.dealt };
    strike_value(dealt, 0, mark.left, mark.worth) - shot.cost
}

/// Each shooter at its best target by `shot_value` (without focus: every
/// shot planned on the turn-start state).
fn best_each(shots: &[Shot], marks: &[Mark]) -> Vec<Shot> {
    let mut shooters: Vec<(bool, u32)> = shots.iter().map(|s| s.key).collect();
    shooters.sort();
    shooters.dedup();
    let mut out = Vec::new();
    for key in shooters {
        let best = shots
            .iter()
            .filter(|s| s.key == key)
            .filter_map(|s| marks.iter().find(|m| m.tile == s.target).map(|m| (shot_value(s, m), s)))
            .filter(|(v, _)| *v > 0)
            .max_by_key(|(v, s)| (*v, std::cmp::Reverse(s.target)));
        out.extend(best.map(|(_, s)| s.clone()));
    }
    out
}

/// The order of the kill plans: the fewest shots, the greatest value, the tile.
type KillKey = (usize, std::cmp::Reverse<i32>, TileId);

/// Focus fire: while some enemy unit dies to the unused shots worth their
/// cost together, the one the fewest shots kill (then the greatest value)
/// takes them, the strongest shots first; then every shooter left fires at
/// the target its shot leaves closest to death, while the shot is worth its
/// cost; a target expected dead takes no more shots. Integer, deterministic.
fn focus_plan(shots: &[Shot], marks: &mut [Mark]) -> Vec<Shot> {
    let mut used: Vec<(bool, u32)> = Vec::new();
    let mut out: Vec<Shot> = Vec::new();
    loop {
        let mut best: Option<(KillKey, Vec<&Shot>)> = None;
        for mark in marks.iter().filter(|m| m.worth.is_some() && m.left > 0) {
            let mut own: Vec<&Shot> = shots.iter().filter(|s| s.target == mark.tile && !used.contains(&s.key)).collect();
            own.sort_by_key(|s| (std::cmp::Reverse(s.dealt), s.cost, s.key));
            let (mut left, mut cost, mut plan) = (mark.left, 0, Vec::new());
            for shot in own {
                if left == 0 {
                    break;
                }
                left = left.saturating_sub(shot.dealt);
                cost += shot.cost;
                plan.push(shot);
            }
            if left > 0 {
                continue;
            }
            let value = i32::from(mark.left) + mark.worth.unwrap_or(0) - cost;
            let key = (plan.len(), std::cmp::Reverse(value), mark.tile);
            if value > 0 && best.as_ref().is_none_or(|(k, _)| key < *k) {
                best = Some((key, plan));
            }
        }
        let Some((_, plan)) = best else { break };
        let target = plan[0].target;
        for shot in &plan {
            used.push(shot.key);
        }
        if let Some(mark) = marks.iter_mut().find(|m| m.tile == target) {
            mark.left = 0;
        }
        out.extend(plan.into_iter().cloned());
    }
    let mut shooters: Vec<(bool, u32)> = shots.iter().map(|s| s.key).filter(|k| !used.contains(k)).collect();
    shooters.sort();
    shooters.dedup();
    for key in shooters {
        let best = shots
            .iter()
            .filter(|s| s.key == key)
            .filter_map(|s| marks.iter().find(|m| m.tile == s.target && m.left > 0).map(|m| (s, m)))
            .filter(|(s, m)| shot_value(s, m) > 0)
            .min_by_key(|(s, m)| (m.left.saturating_sub(s.dealt), std::cmp::Reverse(shot_value(s, m)), s.target))
            .map(|(s, _)| s.clone());
        let Some(shot) = best else { continue };
        if let Some(mark) = marks.iter_mut().find(|m| m.tile == shot.target) {
            // A city keeps 1 HP against shots.
            let floor = u16::from(mark.worth.is_none());
            mark.left = mark.left.saturating_sub(shot.dealt).max(floor);
        }
        out.push(shot);
    }
    out
}

/// Gold the military advisor keeps when it upgrades units.
pub const UPGRADE_RESERVE: u32 = 150;

/// The military system's advisor (`ai::system`): before the units move,
/// city strikes, siege set-up and ranged shots; at the end of the turn,
/// captured cities, promotions and upgrades.
#[derive(Debug, Clone, Copy, Default)]
pub struct MilitaryAdvisor;

impl crate::ai::system::SystemAdvisor for MilitaryAdvisor {
    fn start_of_turn(
        &self,
        input: &crate::ai::system::AdvisorInput<'_, '_>,
    ) -> Vec<crate::ai::system::Proposal> {
        let obs = input.observation;
        let fix = input.brief.fight;
        let (strikes, mut shots) = if fix.reply || fix.focus {
            fixed_shots(input, fix)
        } else {
            (city_strikes(obs), ranged_orders(obs, input.rules))
        };
        let mut out = proposals(30, strikes);
        out.extend(proposals(25, stance_orders(obs, input.rules)));
        if input.brief.air {
            // The aircraft of a variant with an air arm are commanded by it
            // (`ai::air`: the answer of the target, the interceptors, the
            // base); the shots here would ignore them.
            shots.retain(|c| !matches!(c, Command::RangedAttack { unit, .. } if obs.own_units.iter().any(|u| u.handle == *unit && crate::ai::air::wing(input.rules, u.kind).is_some())));
        }
        if input.brief.strikes {
            // Missiles and nuclear weapons are used up by a shot: they are
            // commanded by `ai::strikes`, which weighs the shot.
            shots.retain(|c| !matches!(c, Command::RangedAttack { unit, .. } if obs.own_units.iter().any(|u| u.handle == *unit && crate::ai::strikes::weapon(input.rules, u.kind).is_some())));
        }
        // A land unit with a better firing tile shoots at its own decision (`SiegeTactics`).
        let shots = crate::ai::siege_tactics::keep_shots(input, shots);
        out.extend(proposals(20, shots));
        out
    }

    fn end_of_turn(
        &self,
        input: &crate::ai::system::AdvisorInput<'_, '_>,
    ) -> Vec<crate::ai::system::Proposal> {
        let obs = input.observation;
        crate::ai::war_probe::note(|p| {
            for city in obs.own_cities.iter().filter(|c| c.status == CityStatus::AwaitingDecision) {
                let from = input.state.seen_cities().iter().find(|c| c.tile == city.tile && c.owner != obs.player).map(|c| c.owner);
                p.captures.push((input.brief.round, obs.player, city.tile.0, from));
            }
        });
        let mut out = proposals(30, crate::ai::conquest::decisions(obs, input.rules, input.brief.prices.as_ref(), input.brief.conquest));
        let promotions = if input.brief.fight.promo == Promotions::Priced {
            crate::ai::promo_fit::end_of_turn(obs, input.rules, input.brief.dims)
        } else {
            let chosen = promotion_choices(obs, input.rules);
            crate::ai::promo_fit::trace_choices(obs, input.rules, &chosen);
            chosen
        };
        out.extend(proposals(10, promotions));
        out.extend(proposals(5, upgrade_orders(obs, input.rules, UPGRADE_RESERVE)));
        out
    }
}

/// Commands as proposals of one priority.
pub fn proposals(priority: i32, commands: Vec<Command>) -> Vec<crate::ai::system::Proposal> {
    commands
        .into_iter()
        .map(|command| crate::ai::system::Proposal { priority, command })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shot(unit: u32, target: u32, dealt: u16, cost: i32) -> Shot {
        let handle = UnitHandle { index: unit, generation: 0 };
        Shot { command: Command::RangedAttack { unit: handle, target: TileId(target) }, key: (true, unit), target: TileId(target), dealt, cost }
    }

    fn unit_mark(tile: u32, left: u16) -> Mark {
        Mark { tile: TileId(tile), left, worth: Some(100) }
    }

    fn aimed(plan: &[Shot]) -> Vec<(u32, u32)> {
        let mut out: Vec<(u32, u32)> = plan.iter().map(|s| (s.key.1, s.target.0)).collect();
        out.sort();
        out
    }

    /// Three archers that each deal 30 against two enemies of 50 and 80 health: each at its best
    /// spreads them; focused, two kill the weaker enemy and the third does not fire at it again.
    #[test]
    fn focus_fire_kills_what_spread_fire_only_hurts() {
        let shots: Vec<Shot> = (0..3).flat_map(|u| [shot(u, 1, 30, 0), shot(u, 2, 30, 0)]).collect();
        let mut marks = vec![unit_mark(1, 50), unit_mark(2, 80)];
        let plan = focus_plan(&shots, &mut marks);
        assert_eq!(aimed(&plan), vec![(0, 1), (1, 1), (2, 2)], "two kill the weaker, the third hurts the other");
        assert_eq!(marks[0].left, 0);
        assert_eq!(marks[1].left, 50);
    }

    /// A kill is planned only when the target's health and worth beat what the shots cost the
    /// shooters (`FightFix::reply`); a shot that costs its shooter more than its damage is not fired.
    #[test]
    fn a_shot_is_fired_only_when_worth_its_cost() {
        let shots = vec![shot(0, 1, 30, 200), shot(1, 1, 30, 0)];
        let mut marks = vec![unit_mark(1, 50)];
        let plan = focus_plan(&shots, &mut marks);
        assert_eq!(aimed(&plan), vec![(1, 1)], "the costly kill is refused, the free shot hurts");
        assert_eq!(marks[0].left, 20);
        let exposed = vec![shot(0, 1, 30, 40)];
        assert!(focus_plan(&exposed, &mut [unit_mark(1, 80)]).is_empty(), "30 damage does not pay a loss of 40");
    }

    /// A city keeps 1 HP against shots: no shot is planned as its capture, and the plan's health
    /// of the city stops at 1.
    #[test]
    fn a_city_is_shot_but_not_taken() {
        let shots = vec![shot(0, 1, 30, 0), shot(1, 1, 30, 0)];
        let mut marks = vec![Mark { tile: TileId(1), left: 40, worth: None }];
        let plan = focus_plan(&shots, &mut marks);
        assert_eq!(plan.len(), 2);
        assert_eq!(marks[0].left, 1);
    }

    /// The game's AI before the war fix carries no fight fix (its games stay byte-identical); the
    /// six fight-fix variants do, `fight-fix-net` reading the danger net, and the game's AI fights
    /// as `war-fix` since its promotion.
    #[test]
    fn only_the_fight_fix_variants_carry_the_fixes() {
        use crate::ai::variant::Variant;
        assert_eq!(Variant::PreWarFix.fight_fix(), FightFix::NONE);
        assert_eq!(Variant::FightFix.fight_fix(), FightFix::all(Source::Search));
        assert_eq!(Variant::FightFixNet.fight_fix(), FightFix::all(Source::Net));
        assert_eq!(Variant::WarFix.fight_fix(), FightFix::all(Source::Net), "`war-fix` fights as `fight-fix-net`");
        let war_fix = [Variant::Standard, Variant::PreSiege, Variant::PreTall, Variant::PreAbilities, Variant::UnitAbilities, Variant::ReligionFix, Variant::TallScience, Variant::TallBook, Variant::TallBook2, Variant::TallBook2Filler, Variant::TallBookNoOrder, Variant::TallBookCities, Variant::TallBookPolicies, Variant::ScienceFirst, Variant::WarFix, Variant::WarFixPlain, Variant::WarFixModel, Variant::WarFixUnpriced, Variant::WarFixShots, Variant::WarFixTactics];
        // `siege-tactics-net` fights as `fight-fix-net` with the siege tactics on top; `net` plays
        // its base, `pre-puppet` (`NET_BASE`); `pre-camps`, `pre-workers`, `pre-join`, `pre-puppet`, the conquest
        // variants and `siege-join` play `standard`; so do `unciv` and `unciv-military` wherever Unciv's AI plays no
        // part of its own, and Unciv's modules inside `standard` (`std-unciv-*`).
        let on_top = [Variant::SiegeTacticsNet, Variant::Net, Variant::PreCamps, Variant::PreWorkers, Variant::PreJoin, Variant::PrePuppet, Variant::PuppetPrice, Variant::PuppetAll, Variant::PuppetWar, Variant::SiegeJoin, Variant::Unciv, Variant::UncivMilitary, Variant::NoIdleCities, Variant::ScoutEarly, Variant::TradeFit, Variant::WorkersFit, Variant::ArmyFit, Variant::Camps, Variant::ArmyCamps, Variant::PromoFit, Variant::HealFirst, Variant::FogMarch, Variant::StrategyNet, Variant::UnitNet, Variant::Hybrid, Variant::StandardScripted, Variant::PreCsGifts];
        assert_eq!(Variant::ALL.iter().filter(|v| v.fight_fix().any() && !war_fix.contains(v) && !on_top.contains(v) && !v.unciv().parts().any()).count(), 6);
    }

    /// The value of an exchange: the damage dealt, the foe's worth when it dies, less the damage taken.
    #[test]
    fn the_value_of_a_strike_counts_the_kill_and_the_wounds() {
        assert_eq!(strike_value(30, 10, 50, Some(100)), 20);
        assert_eq!(strike_value(60, 10, 50, Some(100)), 140, "a kill: its health and its worth");
        assert_eq!(strike_value(60, 0, 50, None), 50, "a city is not killed by a strike");
    }
}
