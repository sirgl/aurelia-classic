//! The plan search of the tactical layer (classic/docs/ai-tactical.md,
//! docs/ai-war-design.md 4.4.3): a small deterministic beam search over what
//! each member of a front does this turn — strike a target from a tile, go to
//! a tile, withdraw to a tile, stay — followed by sweeps that improve one
//! member at a time, scored over two plies:
//!
//! 1. our moves, executed in the order the rules reward — ranged strikes
//!    first, then melee strikes by damage, the one that brings a city to zero
//!    last (the capture step), then everybody else's moves — with the damage
//!    dealt and taken by the previews of the rules and the kills they make;
//! 2. the enemy's best reply to the position that results: every enemy unit
//!    and city that can reach a member (the battlefield's reach tree under
//!    the posture of the plan: bodies and zones of control) strikes the
//!    member it hurts most, kills first, in turn.
//!
//! Beside the damage the score holds the value of the position (the ground,
//! the line, the artillery's cover), the progress to the objective (the
//! gathering ring or the city) and the rule that no unit ends in a tile where
//! the enemy together can deal more than its health unless its strike is the
//! kill or the capture. The search is bounded by a count of evaluations
//! taken from the decision budget; ties go to the lower option index.
//!
//! With the danger net (`Problem::danger`, `TurnBrief::danger_net`) the
//! reply is the net's instead: each member's exposure is the damage the net
//! expects at its final tile (the plan's posture, the enemy as the plan's
//! strikes left it), the reply's cost its expected loss, and the lethal rule
//! holds for a member whose chance of death there is `LETHAL_PERMILLE` or
//! more.

use std::cmp::Reverse;

use crate::ai::battlefield::{worth, Battlefield, KindStats, Posture, Target};
use crate::ai::danger::{self, Place, Threats};
use crate::ai::roles::Role;
use crate::combat::city::CITY_ATTACK_RANGE;
use crate::ids::TileId;
use crate::knowledge::observation::OwnUnit;
use crate::knowledge::public::PlayerQueries;

/// States the beam keeps after each member.
pub const BEAM_WIDTH: usize = 8;
/// Strike options a member keeps per target.
pub const STRIKE_TILES: usize = 2;
/// Strike options a member keeps in all.
pub const MAX_STRIKES: usize = 5;
/// Positions (go or withdraw) a member keeps.
pub const MAX_GOES: usize = 4;
/// Sweeps of one-member improvements after the beam.
pub const SWEEPS: usize = 2;
/// With the danger net, a member whose chance of death at its final tile is
/// this or more (permille) falls under the lethal rule: the threshold of the
/// net's best F1 on the validation rows (`ai-train-danger`'s report, 0.36).
pub const LETHAL_PERMILLE: u16 = 360;

/// The points of the score: one hit point of a unit worth 100 is one point.
#[derive(Debug, Clone, Copy)]
pub struct Weights {
    /// A captured city: this, plus `city_per_citizen` for each citizen.
    pub capture: i64,
    pub city_per_citizen: i64,
    /// Damage to a city that is not taken, as the share of its health, in
    /// points per 100 %.
    pub city_progress: i64,
    /// The same when the siege is ready (the group commits to the city):
    /// the damage is then the way to the capture, not an exchange of blows.
    pub city_progress_ready: i64,
    /// A kill, as a share (percent) of the victim's worth, on top of the
    /// damage.
    pub kill_percent: i64,
    /// A unit of ours that dies costs this many times its worth beyond the
    /// health it lost.
    pub death_times: i64,
    /// The extra cost, in worths, of ending a turn where the enemy together
    /// can kill the unit and the strike is not the kill or the capture.
    pub lethal_times: i64,
    /// A strike that finds its target dead: nothing is gained.
    pub waste: i64,
    /// Terrain: points per percent of defence per worth of 100, by role in
    /// tenths.
    pub terrain_tenths: i64,
    /// A neighbour of the line (a member or an own unit), up to two.
    pub line: i64,
    /// Per tile too far from the objective when the siege is ready / the
    /// distance to the gathering ring when it is not / to the nearest enemy.
    pub approach_ready: i64,
    pub approach_gather: i64,
    pub approach_field: i64,
    /// Health a withdrawn unit heals in a city, in points.
    pub heal_city: i64,
    /// Per tile of the way home of a unit that withdraws.
    pub way_home: i64,
    /// A share (percent) of the answer a defender gives counts for us.
    pub answer_percent: i64,
    /// Per tile beyond two from the middle of the group.
    pub cohesion: i64,
    /// The weight (percent) of the enemy's reply in the score.
    pub reply_percent: i64,
    /// The weight (percent) of our own losses in the damage taken.
    pub taken_percent: i64,
    /// The share (percent) of a later strike's value a member earns for
    /// standing in reach of its target, and the fall per tile beyond reach
    /// (percent of the share).
    pub engage_percent: i64,
    pub engage_fall: i64,
}

impl Default for Weights {
    fn default() -> Self {
        Self {
            capture: 1000,
            city_per_citizen: 20,
            city_progress: 300,
            city_progress_ready: 1200,
            kill_percent: 50,
            death_times: 2,
            lethal_times: 3,
            waste: 20,
            terrain_tenths: 4,
            line: 3,
            approach_ready: 60,
            approach_gather: 10,
            approach_field: 3,
            heal_city: 15,
            way_home: 1,
            answer_percent: 50,
            cohesion: 4,
            reply_percent: 60,
            taken_percent: 100,
            engage_percent: 60,
            engage_fall: 25,
        }
    }
}

/// What one member may do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    /// Keep the place (fortify).
    Stay,
    /// Go to a tile.
    Go(TileId),
    /// Leave the front to a tile and heal.
    Withdraw(TileId),
    /// Strike target `.0` (an index into the battlefield's targets) from
    /// tile `.1`, the unit's own or one it walks to first.
    Strike(usize, TileId),
}

/// A strike's expected exchange by the rules' preview.
#[derive(Debug, Clone, Copy)]
pub struct Exchange {
    pub dealt: u16,
    pub taken: u16,
    pub ranged: bool,
    /// The spread of the damage (standard deviation, hit points).
    pub sd: u16,
}

/// One option of a member and its value alone.
#[derive(Debug, Clone, Copy)]
pub struct Opt {
    pub act: Act,
    pub exchange: Option<Exchange>,
    /// The value of the option before the plan: used to prune and to order.
    pub base: i64,
}

/// A member of the front as the search sees it.
#[derive(Debug, Clone)]
pub struct Member {
    pub unit: OwnUnit,
    pub stats: KindStats,
    pub role: Role,
    pub worth: i32,
    pub withdraw: bool,
    pub options: Vec<Opt>,
}

/// The problem of one turn of one theatre.
pub struct Problem<'f, 'v, 'a> {
    pub field: &'f Battlefield<'v, 'a>,
    pub members: Vec<Member>,
    /// Own units that do not take part: bodies and zones of control.
    pub statics: Vec<TileId>,
    pub own_cities: Vec<TileId>,
    /// The tile of the city the group works on (seen or not), if any.
    pub focus: Option<TileId>,
    /// The middle of the group at the start of the turn: the members keep
    /// together around it.
    pub centre: Option<TileId>,
    /// Where the group gathers while the siege is not ready: the tile on
    /// the ring out of the city's strike nearest the middle of the group.
    pub staging: Option<TileId>,
    /// The siege group would take the focus city within its rounds.
    pub ready: bool,
    /// The place each member works toward, one tile each: at its range from
    /// the focus city when the siege is ready, around the staging tile when
    /// it is not (None: no focus).
    pub goals: Vec<Option<TileId>>,
    /// The exchange each member would have with each enemy unit next turn
    /// if it stood in reach (the rules' preview from a tile next to the
    /// target, at range for a ranged unit): (dealt, taken), (0, 0) for none;
    /// [member][target]. The incentive to come within reach together.
    pub engage: Vec<Vec<(u16, u16)>>,
    pub weights: Weights,
    /// The danger net's threats of the turn: the reply is read from the
    /// net (the module's last paragraph); None: the enemy's blows.
    pub danger: Option<&'f Threats<'v, 'a>>,
}

/// What a plan is worth and how it ends.
#[derive(Debug, Clone, Default)]
pub struct Eval {
    pub score: i64,
    pub dealt: i64,
    pub taken: i64,
    pub reply: i64,
    pub kills: u32,
    pub captures: u32,
    pub wasted: u32,
    pub lethal: u32,
    /// Members all the enemy together could kill that its best reply does
    /// not (it has too few blows for all of them).
    pub overlap: u32,
    pub conflicts: u32,
    pub deaths: u32,
    /// Members that end screened (no enemy reaches them), of the
    /// artillery.
    pub screened: u32,
    pub artillery: u32,
    /// The final tile of every member, and whether its strike was the
    /// kill; `order` is the execution order of the strikes.
    pub tiles: Vec<TileId>,
    pub killer: Vec<bool>,
    /// The members whose strike is on the focus city while the capture is
    /// within reach (the city is dead or at most half alive after the
    /// plan's strikes): they stay in the siege on a tile the enemy's reply
    /// could kill them on.
    pub sieger: Vec<bool>,
    pub order: Vec<usize>,
    pub skipped: Vec<bool>,
    pub exposure: Vec<i32>,
    /// The members the enemy's best reply kills.
    pub reply_dead: Vec<bool>,
    /// Strikes made only if the target still lives after the others.
    pub backup: Vec<bool>,
}

/// The chosen option of every member, and its evaluation.
#[derive(Debug, Clone)]
pub struct Solution {
    pub choices: Vec<usize>,
    pub eval: Eval,
    pub evaluations: u32,
}

/// A target the strikes so far have killed with this chance (percent) is dead:
/// the next strike on it is wasted.
const SURE_PERCENT: i64 = 92;

/// The normal distribution's cumulative chance, in percent, at `z` tenths of a
/// standard deviation (a table: no floating point in the AI).
fn phi_percent(z_tenths: i32) -> i64 {
    const TABLE: [i64; 31] = [50, 54, 58, 62, 66, 69, 73, 76, 79, 82, 84, 86, 88, 90, 92, 93, 94, 95, 96, 97, 98, 98, 99, 99, 99, 99, 99, 99, 100, 100, 100];
    let z = z_tenths.clamp(-30, 30);
    if z >= 0 {
        TABLE[z as usize]
    } else {
        100 - TABLE[(-z) as usize]
    }
}

fn isqrt(n: i32) -> i32 {
    let mut x = (n.max(0) as f64).sqrt() as i32;
    while x * x > n {
        x -= 1;
    }
    while (x + 1) * (x + 1) <= n {
        x += 1;
    }
    x
}

/// A member the search has not decided yet: it is left out of the picture
/// (no body, no strike, no exposure), so that the members decided first do
/// not collide with the places the others would leave.
const ABSENT: usize = usize::MAX;

impl<'f, 'v, 'a> Problem<'f, 'v, 'a> {
    fn act(&self, member: usize, choice: usize) -> Act {
        if choice == ABSENT {
            return Act::Stay;
        }
        self.members[member].options[choice].act
    }

    fn strike_of(&self, member: usize, choice: usize) -> Option<(usize, TileId, Exchange)> {
        if choice == ABSENT {
            return None;
        }
        let opt = &self.members[member].options[choice];
        match (opt.act, opt.exchange) {
            (Act::Strike(target, from), Some(exchange)) => Some((target, from, exchange)),
            _ => None,
        }
    }

    /// The posture of the player's units for the final tiles `tiles`
    /// (members alive), beside the statics and the own cities.
    fn posture(&self, tiles: &[Option<TileId>]) -> Posture {
        let mut bodies: Vec<TileId> = self.statics.clone();
        bodies.extend(tiles.iter().flatten());
        let mut sources = bodies.clone();
        sources.extend(self.own_cities.iter());
        Posture { bodies, sources }
    }

    /// Evaluate the choices: strikes in their order, then the enemy's reply.
    pub fn evaluate(&self, choices: &[usize]) -> Eval {
        let w = &self.weights;
        let field = self.field;
        let m = self.members.len();
        let mut hp: Vec<i32> = self.members.iter().map(|x| i32::from(x.unit.hp)).collect();
        let mut thp: Vec<i32> = field.targets.iter().map(|t| i32::from(t.hp)).collect();
        let mut tile: Vec<TileId> = self.members.iter().map(|x| x.unit.tile).collect();
        let mut struck = vec![false; m];
        let mut killer = vec![false; m];
        let mut sieger = vec![false; m];
        let mut skipped = vec![false; m];
        let mut eval = Eval::default();
        let mut score: i64 = 0;

        // 1. our strikes: ranged first, then melee by damage.
        let mut order: Vec<usize> = (0..m).filter(|&i| self.strike_of(i, choices[i]).is_some()).collect();
        order.sort_by_key(|&i| {
            let (_, _, x) = self.strike_of(i, choices[i]).expect("a strike");
            (!x.ranged, Reverse(x.dealt), i)
        });
        let city_hp: Vec<i32> = field.targets.iter().map(|t| i32::from(t.hp).max(1)).collect();
        // Per target: the damage the strikes so far deal in all (ranged and melee
        // apart: a city does not fall to a shot), the variance of it, and the
        // chance, in percent, that the target is dead by now. A strike after a
        // sure kill is wasted; one after a likely kill is a back-up that is
        // made only if the target lives (its value is the rise in the chance).
        let mut mu_ranged = vec![0i32; field.targets.len()];
        let mut mu_melee = vec![0i32; field.targets.len()];
        let mut variance = vec![0i32; field.targets.len()];
        let mut dead_chance = vec![0i64; field.targets.len()];
        let mut backup = vec![false; m];
        for &i in &order {
            let (target, from, x) = self.strike_of(i, choices[i]).expect("a strike");
            let t = &field.targets[target];
            let hp0 = i32::from(t.hp).max(1);
            let before = dead_chance[target];
            if before >= SURE_PERCENT || x.dealt == 0 {
                eval.wasted += 1;
                skipped[i] = true;
                score -= w.waste;
                continue;
            }
            let mine = i64::from(self.members[i].worth);
            let ran = t.is_city() && x.ranged;
            let previous_mu = if t.is_city() { mu_ranged[target].min(hp0 - 1) + mu_melee[target] } else { mu_ranged[target] + mu_melee[target] };
            if x.ranged {
                mu_ranged[target] += i32::from(x.dealt);
            } else {
                mu_melee[target] += i32::from(x.dealt);
            }
            variance[target] += i32::from(x.sd) * i32::from(x.sd);
            let mu = if t.is_city() { mu_ranged[target].min(hp0 - 1) + mu_melee[target] } else { mu_ranged[target] + mu_melee[target] };
            let sd = isqrt(variance[target]).max(1);
            let after = if ran { before } else { phi_percent(10 * (mu - hp0) / sd) };
            let after = after.max(before);
            dead_chance[target] = after;
            // The value: the damage that counts and the rise in the chance of the kill.
            let gained = i64::from(mu.min(hp0) - previous_mu.min(hp0)).max(0);
            let value = if t.is_city() {
                (if self.ready { w.city_progress_ready } else { w.city_progress }) * gained / i64::from(city_hp[target])
                    + (w.capture + w.city_per_citizen * i64::from(t.population)) * (after - before) / 100
            } else {
                let victim = i64::from(t.worth);
                victim * gained / 100 + victim * w.kill_percent / 100 * (after - before) / 100
            };
            let executed = 100 - before;
            let cost = mine * i64::from(x.taken) / 100 * w.taken_percent / 100 * executed / 100;
            eval.dealt += value;
            eval.taken += cost;
            score += value - cost;
            if thp[target] <= 0 {
                // In the expected course the target is dead already: this strike is a back-up.
                backup[i] = true;
                skipped[i] = true;
                continue;
            }
            let dealt = i32::from(x.dealt).min(thp[target] - i32::from(ran));
            thp[target] -= dealt.max(0);
            let killed = thp[target] <= 0;
            hp[i] -= i32::from(x.taken);
            if killed && t.is_city() && !x.ranged {
                hp[i] = hp[i].max(1);
            }
            tile[i] = from;
            struck[i] = true;
            sieger[i] = t.is_city() && self.focus == Some(t.tile);
            if killed {
                eval.kills += 1;
                killer[i] = true;
                if t.is_city() {
                    eval.captures += 1;
                }
                if !x.ranged {
                    let alone = !field.targets.iter().enumerate().any(|(j, o)| j != target && o.tile == t.tile && thp[j] > 0);
                    if alone {
                        tile[i] = t.tile;
                    }
                }
            }
            if hp[i] <= 0 {
                eval.deaths += 1;
                score -= mine * w.death_times;
            }
        }
        eval.backup = backup;

        // 2. everybody else's moves.
        for i in 0..m {
            if struck[i] && !skipped[i] {
                continue;
            }
            match self.act(i, choices[i]) {
                Act::Go(t) | Act::Withdraw(t) => tile[i] = t,
                _ => {}
            }
        }

        // 3. conflicts: two bodies on a tile, or on an enemy that lives.
        let mut taken_tiles: Vec<TileId> = self.statics.clone();
        for t in field.targets.iter().enumerate().filter(|(j, _)| thp[*j] > 0).map(|(_, t)| t.tile) {
            taken_tiles.push(t);
        }
        let alive: Vec<bool> = hp.iter().enumerate().map(|(i, h)| *h > 0 && choices[i] != ABSENT).collect();
        for i in 0..m {
            if !alive[i] {
                continue;
            }
            if taken_tiles.contains(&tile[i]) {
                eval.conflicts += 1;
            } else {
                taken_tiles.push(tile[i]);
            }
        }
        score -= i64::from(eval.conflicts) * 10_000;
        // A member that waits for a place another member must leave first, in a
        // circle: nobody can move in the turn.
        let waits: Vec<Option<usize>> = (0..m)
            .map(|i| {
                if !alive[i] || tile[i] == self.members[i].unit.tile {
                    return None;
                }
                (0..m).find(|&j| j != i && alive[j] && self.members[j].unit.tile == tile[i] && tile[j] != tile[i])
            })
            .collect();
        let mut circular = 0;
        for start in 0..m {
            let mut at = start;
            for _ in 0..m {
                match waits[at] {
                    Some(next) => at = next,
                    None => break,
                }
                if at == start {
                    circular += 1;
                    break;
                }
            }
        }
        eval.conflicts += circular;
        score -= i64::from(circular) * 10_000;

        // 4. the enemy's reply to the posture.
        let finals: Vec<Option<TileId>> = (0..m).map(|i| alive[i].then_some(tile[i])).collect();
        let posture = self.posture(&finals);
        let (exposure, mut reply, killed) = match self.danger {
            Some(threats) => self.net_reply(threats, &posture, choices, &alive, &tile, &hp, &thp, &struck, &mut eval),
            None => self.blow_reply(&posture, choices, &alive, &tile, &hp, &thp, &struck, &mut eval),
        };
        // The lethal rule: a member the enemy's best reply kills, and whose strike
        // is not the kill or the capture, pays dearly (and the search below
        // moves it to a place where it lives when there is one).
        let mut dead = vec![false; m];
        let near = field.targets.iter().enumerate().any(|(j, t)| t.is_city() && self.focus == Some(t.tile) && (thp[j] <= 0 || thp[j] * 2 <= i32::from(t.hp)));
        for i in 0..m {
            sieger[i] = sieger[i] && near;
            dead[i] = killed[i];
            if dead[i] && !killer[i] && !sieger[i] && !self.members[i].withdraw_safe(tile[i], &self.own_cities) {
                eval.lethal += 1;
                reply += i64::from(self.members[i].worth) * w.lethal_times;
            } else if !(dead[i]) && exposure[i] >= hp[i] {
                eval.overlap += 1;
            }
        }
        eval.reply = reply;
        score -= reply * w.reply_percent / 100;

        // 5. the position.
        for i in 0..m {
            if !alive[i] {
                continue;
            }
            let member = &self.members[i];
            let held = !struck[i] && matches!(self.act(i, choices[i]), Act::Stay) && member.unit.fortified;
            let percent = field.defence_percent(tile[i], field.player, held);
            let role_tenths = match member.role {
                Role::Anvil => 10,
                Role::Screen | Role::Artillery => 6,
                Role::Hammer => 4,
                Role::Reserve => 3,
            };
            score += percent * i64::from(member.worth) * role_tenths * w.terrain_tenths / 10_000;
            if matches!(member.role, Role::Anvil | Role::Screen) {
                let near = (0..m).filter(|&j| j != i && alive[j] && field.dims.adjacent(tile[j], tile[i])).count()
                    + self.statics.iter().filter(|s| field.dims.adjacent(**s, tile[i])).count();
                score += w.line * near.min(2) as i64;
            }
            if member.role == Role::Artillery {
                eval.artillery += 1;
                if exposure[i] == 0 {
                    eval.screened += 1;
                }
            }
            score += self.progress(i, member, tile[i], struck[i] && !skipped[i]);
        }
        score += self.next_turn(&alive, &tile, &thp, &struck);
        eval.score = score;
        eval.tiles = tile;
        eval.killer = killer;
        eval.sieger = sieger;
        eval.order = order;
        eval.skipped = skipped;
        eval.exposure = exposure;
        eval.reply_dead = dead;
        eval
    }

    /// The enemy's best reply by its blows: every member's exposure (the
    /// blows of every enemy that can strike its final tile under the
    /// posture, and the fog's), the reply's cost (the greedy reply: the strongest first,
    /// each on the member it hurts most, a kill first; the answers of the
    /// defenders count for us) and the members it kills.
    #[allow(clippy::too_many_arguments)]
    fn blow_reply(
        &self,
        posture: &Posture,
        choices: &[usize],
        alive: &[bool],
        tile: &[TileId],
        hp: &[i32],
        thp: &[i32],
        struck: &[bool],
        eval: &mut Eval,
    ) -> (Vec<i32>, i64, Vec<bool>) {
        let w = &self.weights;
        let field = self.field;
        let m = self.members.len();
        let mut dmg: Vec<Vec<i32>> = vec![vec![0; m]; field.targets.len()];
        let mut attackers: Vec<usize> = Vec::new();
        for (f, t) in field.targets.iter().enumerate() {
            if thp[f] <= 0 || !t.threatens() {
                continue;
            }
            let mut foe = *t;
            foe.hp = thp[f] as u16;
            let mut any = false;
            for i in 0..m {
                if !alive[i] {
                    continue;
                }
                let tiles_close = field.distance(t.tile, tile[i]) <= t.stats.range.max(1) + t.stats.movement.div_ceil(60) as i32 + 1;
                if !tiles_close || !field.can_strike(f, tile[i], posture) {
                    continue;
                }
                let held = !struck[i] && matches!(self.act(i, choices[i]), Act::Stay) && self.members[i].unit.fortified;
                dmg[f][i] = i32::from(field.blow_by(&foe, &self.members[i].stats, hp[i].max(1) as u16, tile[i], held));
                any = true;
            }
            if any {
                attackers.push(f);
            }
        }
        let mut exposure: Vec<i32> = vec![0; m];
        for &f in &attackers {
            for i in 0..m {
                exposure[i] += dmg[f][i];
            }
        }
        for i in 0..m {
            if alive[i] {
                exposure[i] += i32::from(field.fog_blow(tile[i]));
            }
        }
        // Greedy reply: the strongest first, each on the member it hurts most (a kill first).
        attackers.sort_by_key(|&f| (Reverse(i64::from(field.targets[f].worth) * i64::from(thp[f])), f));
        let mut left: Vec<i32> = hp.to_vec();
        let mut reply: i64 = 0;
        for &f in &attackers {
            let pick = (0..m)
                .filter(|&i| alive[i] && left[i] > 0 && dmg[f][i] > 0)
                .max_by_key(|&i| (dmg[f][i] >= left[i], i64::from(dmg[f][i].min(left[i])) * i64::from(self.members[i].worth), Reverse(i)));
            let Some(i) = pick else { continue };
            let hit = dmg[f][i].min(left[i]);
            left[i] -= hit;
            let mine = i64::from(self.members[i].worth);
            reply += mine * i64::from(hit) / 100;
            if left[i] <= 0 {
                reply += mine * w.death_times;
                eval.deaths += 1;
            }
            let t = &field.targets[f];
            if !t.is_city() && !t.stats.is_ranged() {
                let answer = field.answer_by(t, &self.members[i].stats, left[i].max(1) as u16, tile[i], false);
                reply -= i64::from(t.worth) * i64::from(answer) * w.answer_percent / 10_000;
            }
        }
        for i in 0..m {
            if alive[i] {
                let fog = i64::from(field.fog_blow(tile[i]));
                reply += i64::from(self.members[i].worth) * fog / 100;
            }
        }
        let killed = (0..m).map(|i| alive[i] && left[i] <= 0).collect();
        (exposure, reply, killed)
    }

    /// The enemy's reply read from the danger net at each member's final
    /// tile, with the plan's posture and the enemy as the plan's strikes
    /// left it: the exposure is the damage it expects (the fog's blow where
    /// no enemy strikes), the cost the expected loss (the damage and the
    /// chance of death times `death_times` worths), the killed the members
    /// whose chance of death is `LETHAL_PERMILLE` or more.
    #[allow(clippy::too_many_arguments)]
    fn net_reply(
        &self,
        threats: &Threats<'_, '_>,
        posture: &Posture,
        choices: &[usize],
        alive: &[bool],
        tile: &[TileId],
        hp: &[i32],
        thp: &[i32],
        struck: &[bool],
        eval: &mut Eval,
    ) -> (Vec<i32>, i64, Vec<bool>) {
        let w = &self.weights;
        let field = self.field;
        let rules = field.rules;
        let m = self.members.len();
        let hurt: Vec<(TileId, u16)> =
            field.targets.iter().enumerate().filter(|(j, t)| thp[*j] != i32::from(t.hp)).map(|(j, t)| (t.tile, thp[j].max(0) as u16)).collect();
        let mut exposure = vec![0; m];
        let mut reply = 0i64;
        let mut killed = vec![false; m];
        for i in 0..m {
            if !alive[i] {
                continue;
            }
            let started = prof::now_ns();
            let member = &self.members[i];
            let held = !struck[i] && matches!(self.act(i, choices[i]), Act::Stay) && member.unit.fortified;
            let mut unit = danger::moved(&member.unit, tile[i]);
            unit.hp = hp[i].clamp(1, i32::from(crate::world::unit::UNIT_MAX_HP)) as u16;
            if struck[i] {
                unit.attacks_made = unit.attacks_made.saturating_add(1);
                unit.moves_left = unit.moves_left.min(unit.moves_max.saturating_sub(1));
            }
            if !held {
                unit.fortified = false;
                unit.stance = crate::world::unit::Stance::Ready;
            }
            let place = Place { tile: tile[i], posture, hurt: &hurt };
            let mine = i64::from(member.worth);
            match threats.net_danger(rules, &unit, &place) {
                Some(answer) => {
                    exposure[i] = i32::from(answer.damage);
                    reply += mine * i64::from(answer.damage) / 100 + mine * w.death_times * i64::from(answer.death_permille) / 1000;
                    killed[i] = answer.death_permille >= LETHAL_PERMILLE;
                    eval.deaths += u32::from(killed[i]);
                }
                None => {
                    let fog = i32::from(field.fog_blow(tile[i]));
                    exposure[i] = fog;
                    reply += mine * i64::from(fog) / 100;
                }
            }
            danger::note_row(prof::now_ns().saturating_sub(started));
        }
        (exposure, reply, killed)
    }

    /// The value of the strikes our members could make together next turn
    /// from where they end, if the enemy stayed: for each enemy unit the
    /// members within reach (their moves and range from the final tile)
    /// deal their previewed damage until it dies; the value of the damage
    /// and of the kill less the answers taken, a share of it (the enemy
    /// moves). Members that struck this turn do not count.
    fn next_turn(&self, alive: &[bool], tile: &[TileId], thp: &[i32], struck: &[bool]) -> i64 {
        let w = &self.weights;
        let field = self.field;
        let mut total = 0i64;
        for (j, target) in field.targets.iter().enumerate() {
            if target.is_city() || thp[j] <= 0 {
                continue;
            }
            let mut parts: Vec<(u16, u16, i64)> = Vec::new();
            for (i, member) in self.members.iter().enumerate() {
                if !alive[i] || struck[i] || member.withdraw {
                    continue;
                }
                let (dealt, taken) = self.engage.get(i).and_then(|r| r.get(j)).copied().unwrap_or((0, 0));
                if dealt == 0 {
                    continue;
                }
                let reach = member.stats.range.max(1) + ((member.stats.movement / 60) as i32 - 1).max(0);
                if field.distance(target.tile, tile[i]) <= reach {
                    parts.push((dealt, taken, i64::from(member.worth)));
                }
            }
            parts.sort_by_key(|p| (Reverse(p.0), p.1));
            let (mut left, mut gain, mut cost) = (thp[j], 0i64, 0i64);
            for (dealt, taken, mine) in parts {
                if left <= 0 {
                    break;
                }
                let dealt = i32::from(dealt).min(left);
                left -= dealt;
                gain += i64::from(target.worth) * i64::from(dealt) / 100;
                cost += mine * i64::from(taken) / 100;
            }
            if left <= 0 {
                gain += i64::from(target.worth) * w.kill_percent / 100;
            }
            total += (gain - cost).max(0);
        }
        total * w.engage_percent / 100
    }

    /// The progress of a member standing on `tile` to the objective, and
    /// for a withdrawing member the way home and the healing.
    fn progress(&self, index: usize, member: &Member, tile: TileId, struck: bool) -> i64 {
        let w = &self.weights;
        let field = self.field;
        if member.withdraw {
            // Close behind the line, in a city if one is near; very hurt units go home.
            let home = self.own_cities.iter().map(|c| field.distance(*c, tile)).min().unwrap_or(0);
            let heal = w.heal_city * (field.rest_heal(tile) - 10) / 15;
            let behind = self.centre.map_or(0, |c| (field.distance(c, tile) - 4).max(0));
            let away = if member.unit.hp < crate::ai::roles::withdraw_below(member.unit.level) { home.min(12) } else { behind.min(12) };
            return heal - w.way_home * i64::from(away);
        }
        if struck {
            return 0;
        }
        let together = self.centre.map_or(0, |c| -w.cohesion * i64::from((field.distance(c, tile) - 2).max(0)));
        if let Some(goal) = self.goals.get(index).copied().flatten() {
            let weight = if self.ready { w.approach_ready } else { w.approach_gather };
            return together - weight * i64::from(field.distance(goal, tile));
        }
        let reach = member.stats.range.max(1);
        let objective = if let Some(city) = self.focus {
            let d = field.distance(city, tile);
            if self.ready {
                -w.approach_ready * i64::from((d - reach).max(0))
            } else {
                let ring = crate::ai::front::STAGING_DISTANCE.max(CITY_ATTACK_RANGE + 1);
                let gather = match self.staging {
                    Some(point) => (field.distance(point, tile) - 1).max(0).max(ring - d),
                    None => (d - ring).abs(),
                };
                -w.approach_gather * i64::from(gather)
            }
        } else {
            let nearest = field.targets.iter().filter(|t| t.threatens()).map(|t| field.distance(t.tile, tile)).min().unwrap_or(0);
            -w.approach_field * i64::from((nearest - reach).max(0))
        };
        objective + together
    }
}

impl Member {
    /// A withdrawing unit in an own city is safe whatever the enemy could
    /// do: the city defends it.
    fn withdraw_safe(&self, tile: TileId, cities: &[TileId]) -> bool {
        cities.contains(&tile)
    }
}

/// Search the best choices: a beam over the members (the strikers first),
/// then sweeps of one-member improvements. At most `max_evaluations`
/// evaluations; the default choice is every member staying.
pub fn search(problem: &Problem<'_, '_, '_>, max_evaluations: u32) -> Solution {
    let m = problem.members.len();
    let mut evaluations = 0u32;
    let start = vec![ABSENT; m];
    let first = problem.evaluate(&start);
    evaluations += 1;
    let mut beam: Vec<(Vec<usize>, Eval)> = vec![(start, first)];
    let mut order: Vec<usize> = (0..m).collect();
    order.sort_by_key(|&i| (role_rank(problem.members[i].role), problem.members[i].unit.id, i));
    'depth: for &i in &order {
        let mut next: Vec<(Vec<usize>, Eval)> = Vec::new();
        for (choices, _) in &beam {
            for k in 0..problem.members[i].options.len() {
                if evaluations >= max_evaluations {
                    break 'depth;
                }
                let mut c = choices.clone();
                c[i] = k;
                if next.iter().any(|(o, _)| *o == c) {
                    continue;
                }
                let eval = problem.evaluate(&c);
                evaluations += 1;
                next.push((c, eval));
            }
        }
        next.sort_by(|a, b| b.1.score.cmp(&a.1.score).then_with(|| a.0.cmp(&b.0)));
        next.truncate(BEAM_WIDTH);
        if !next.is_empty() {
            beam = next;
        }
    }
    beam.sort_by(|a, b| b.1.score.cmp(&a.1.score).then_with(|| a.0.cmp(&b.0)));
    let (mut choices, mut eval) = beam.swap_remove(0);
    // A member the budget never reached stays where it is.
    if choices.contains(&ABSENT) {
        for c in choices.iter_mut().filter(|c| **c == ABSENT) {
            *c = 0;
        }
        eval = problem.evaluate(&choices);
        evaluations += 1;
    }
    // Sweeps: one member at a time, the others fixed; when the new place is one
    // another member stands on, that member gets its options tried too (a
    // swap or a step aside).
    for _ in 0..SWEEPS {
        let mut improved = false;
        for &i in order.iter().rev() {
            let mut best: (i64, Vec<usize>) = (eval.score, choices.clone());
            for k in 0..problem.members[i].options.len() {
                if k == choices[i] {
                    continue;
                }
                if evaluations >= max_evaluations {
                    break;
                }
                let mut c = choices.clone();
                c[i] = k;
                let e = problem.evaluate(&c);
                evaluations += 1;
                if e.score > best.0 {
                    best = (e.score, c.clone());
                }
                if e.conflicts > 0 {
                    for j in (0..m).filter(|&j| j != i && e.tiles[j] == e.tiles[i]) {
                        for l in 0..problem.members[j].options.len() {
                            if l == c[j] || evaluations >= max_evaluations {
                                continue;
                            }
                            let mut c2 = c.clone();
                            c2[j] = l;
                            let e2 = problem.evaluate(&c2);
                            evaluations += 1;
                            if e2.score > best.0 {
                                best = (e2.score, c2);
                            }
                        }
                    }
                }
            }
            if best.1 != choices {
                choices = best.1;
                eval = problem.evaluate(&choices);
                evaluations += 1;
                improved = true;
            }
        }
        if !improved || evaluations >= max_evaluations {
            break;
        }
    }
    // The lethal rule as a rule, not only a cost: a member that ends where
    // the enemy together can kill it, and whose strike is not the kill or the
    // capture, takes its safest option (the best of the safe ones).
    for _ in 0..3 {
        let mut changed = false;
        for &i in &order {
            if eval.killer[i] || eval.sieger[i] || !eval.reply_dead[i] || problem.own_cities.contains(&eval.tiles[i]) {
                continue;
            }
            let mut best: Option<(bool, i64, usize)> = None;
            for k in 0..problem.members[i].options.len() {
                if k == choices[i] {
                    continue;
                }
                let mut c = choices.clone();
                c[i] = k;
                let e = problem.evaluate(&c);
                evaluations += 1;
                let safe = e.killer[i] || e.sieger[i] || !e.reply_dead[i] || problem.own_cities.contains(&e.tiles[i]);
                let key = (safe, e.score, k);
                if best.is_none_or(|b| (key.0, key.1) > (b.0, b.1)) {
                    best = Some(key);
                }
            }
            if let Some((safe, _, k)) = best {
                if safe {
                    choices[i] = k;
                    eval = problem.evaluate(&choices);
                    evaluations += 1;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    Solution { choices, eval, evaluations }
}

fn role_rank(role: Role) -> u8 {
    match role {
        Role::Artillery => 0,
        Role::Hammer => 1,
        Role::Anvil => 2,
        Role::Screen => 3,
        Role::Reserve => 4,
    }
}

/// The worth of a unit of ours for the plan.
pub fn member_worth(stats: &KindStats, level: u8) -> i32 {
    worth(stats, level)
}

/// The value of a target for ordering options: what a strike gains alone.
pub fn strike_value(target: &Target, exchange: &Exchange, mine: i32) -> i64 {
    let gained = if target.is_city() { 40 * i64::from(exchange.dealt) / 10 } else { i64::from(target.worth) * i64::from(exchange.dealt) / 100 };
    gained - i64::from(mine) * i64::from(exchange.taken) / 100
}

/// Preview of a strike of `unit` from `from` on the target's tile by the
/// rules' own answer (None: not possible, or no damage).
pub fn preview(field: &Battlefield<'_, '_>, unit: &OwnUnit, from: TileId, target: &Target, ranged: bool) -> Option<Exchange> {
    let p = field.view.attack_preview_from(unit.handle, from, target.tile)?;
    let dealt = p.to_defender.1;
    let taken = p.to_attacker.1;
    if dealt == 0 {
        return None;
    }
    if taken >= unit.hp && dealt < target.hp {
        return None;
    }
    let spread = p.to_defender.2.saturating_sub(p.to_defender.0);
    Some(Exchange { dealt, taken, ranged, sd: (spread * 29 / 100).max(1) })
}
