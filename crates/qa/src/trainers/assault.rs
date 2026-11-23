//! The assault families (part 2 of the trainers): the planned assault on a city.
//!
//! WIN RULES AND MEASURES (fixed before the first run; "taken" = the enemy city is the learner's at a
//! round end, "lost" = our scene units dead or captured; a round is a game round):
//! - assault-heal: the city taken within 30 rounds with at most half of ours lost. Our army starts
//!   worn (3 units at 45-70 % hit points), a home city of ours (friendly land) stands 10 tiles from the
//!   enemy city. Measures: rounds to take, units lost, damage taken (all of it, and the part taken in
//!   the enemy's turn: the city's strike and the garrison), turns our units rested.
//! - promotion-choice: the city taken within 20 rounds with at most half of ours lost. A field force
//!   (a scripted hunter) stands between the army and the city, no friendly land near. Every unit holds
//!   two promotions of its role's ladder already and a few experience points short of a third: the
//!   first fight gives it the choice. Measures: the same, and the picks (`picks`).
//! - hill-archers: the city taken within 16 rounds with at most 1 of ours lost. The city's neighbours
//!   on our side are hills and so are some tiles two away: from open ground the hills shut the line of
//!   sight, only a shooter on a hill reaches the city. Measures: city damage dealt (per round
//!   fought), damage taken from the enemy's turn, rounds to take.
//! - city-fire: the city taken within 14 rounds with at most 1 of ours lost. Open ground, a strong
//!   walled city with one garrison unit: its own strike is what hurts. Measures: damage taken in the
//!   enemy's turn before the capture, units lost, rounds.
//! - planned-assault: the composite: hills round the city, worn units, a home city, promotions to
//!   choose: the city taken within 30 rounds with at most 1 of ours lost. Measures: all of the above.
//! - assault-reinforce ("at any cost"): the assault-heal ground and forces, the army at the edge of
//!   the city's strike; at round N (6-8, drawn from the instance alone) 2-3 of the era's strongest melee of the enemy land in and next to the city. Win:
//!   the city taken before them (`taken` <= N - 1, the scene's `deadline`); NO loss cap; the play ends
//!   at round N. Measures: the assault's, and `deadline`.
//! - assault-peace ("at any cost"): the city is the war's goal and the peace comes at round N (7-9,
//!   drawn from the instance alone): the planned-assault composite with a fourth melee unit and five
//!   worn units, the army at the edge of the city's strike. Win: the city taken by round N (`deadline`
//!   = N); NO loss cap; the play ends at round N. (N was 8-10 in the spec; at 10 the careful plan won
//!   half the scenes, so 7-9.)
//!
//! THE REFERENCES (one planner, `turn`, with three switches; every reference plays only by the rules'
//! own previews, data and the game's public state):
//! - stage: no unit enters the city's strike range (`CITY_ATTACK_RANGE`, 2) until every active attacker
//!   can reach its firing tile (shooters) or an adjacent tile (melee) in ONE move: they go on the same
//!   turn, the city strikes once a turn and is not given a trickle of targets.
//! - tiles (hill archers): a shooter takes, of the tiles it reaches with a clear line to the target
//!   and within its range, the one outside the city's strike if there is one, else the best defence
//!   (hills), else the least threatened.
//! - rotate: a unit under 70 hit points leaves for the nearest friendly land out of the city's
//!   reach (a city 25 a turn, friendly land 20, else 10 and a medic's 5) and rests (it does not move,
//!   so it heals) until 90 %, then returns.
//! - promote: a unit with the experience takes the promotion the plan prices highest (`promo_value`:
//!   the rules' effects, weighted by what the unit will do: the blows on the city, the shots, the
//!   city's fire, healing for an army with no friendly land near).
//!   ref-fire = stage; ref-hills = stage + tiles; ref-rotate = stage + tiles + rotate; ref-promo =
//!   stage + tiles + promote; ref-planned = all four.
//! - burst (ref-burst = stage + tiles + promote + burst, no rotation): once engaged every unit strikes the
//!   city every turn (the shooters as in the others; every melee unit too, from the tile of its best blow,
//!   a blow it survives by the preview or one that takes the city), no unit leaves to heal, and a melee
//!   unit goes in (takes the city) at the first blow that can. It pays units for time: the reference of
//!   the "at any cost" families, where `ref-planned` is the careful control.

use std::collections::{BTreeMap, BTreeSet};

use classic_rules::combat::city::CITY_ATTACK_RANGE;
use classic_rules::combat::experience::{available_promotions, experience_needed_by};
use classic_rules::combat::terrain::is_friendly_territory;
use classic_rules::ids::{PlayerId, PromotionId, TileId, UnitHandle};
use classic_rules::{ClassicGame, Command, CompiledRuleset, GameState};
use kernel::ids::EntityId;
use kernel::rules::effects::{CombatContext, Modifier, UnitAbility, UnitStat};
use kernel::world::unit::{UnitCore, UNIT_MAX_HP};

use super::act::{self, Aim, Nerve};
use super::families::{blank, defences, kinds_label, Fam, Painter};
use super::lab::{dist, tiles_within, Arm, Lab};
use super::{Piece, Scene};

/// A unit under this many hit points leaves the line (the city's strike takes up to about 55 here: a unit under 70 is one
/// strike from the rotation or from death) and rests until `ROTATE_BACK`.
pub const ROTATE_BELOW: u16 = 70;
pub const ROTATE_BACK: u16 = 90;
/// A shooter in the burst leaves below this (the city's strongest strike takes about 90).
pub const ROTATE_BURST: u16 = 45;
/// The turns `ref-burst` waits for the staging at most.
pub const BURST_WAIT: u32 = 2;

/// What a scene keeps between rounds (the measures and the references' memory).
#[derive(Debug, Clone, Default)]
pub struct Track {
    prev_hp: BTreeMap<EntityId, i64>,
    prev_promos: BTreeMap<EntityId, Vec<PromotionId>>,
    prev_city_hp: Option<i64>,
    /// Damage our units took in the enemy's turn (the city's strike, the garrison) and in ours.
    pub dmg_enemy_turn: i64,
    pub dmg_own_turn: i64,
    /// Damage our units dealt to the city (its hit points lost in our turn).
    pub city_dealt: i64,
    /// Rounds in which our units took no damage and rested, rounds of units resting.
    pub rested: u32,
    pub resting: BTreeSet<EntityId>,
    pub engaged: bool,
    pub waited: u32,
    pub picks: Vec<String>,
    /// The memory families' references (`memory.rs`): the foes seen, their last tile and round.
    pub seen: BTreeMap<EntityId, (TileId, u32)>,
}

impl Track {
    /// The track without the planner's commitments (engaged, waited, resting, the foes seen): what the scene of a seat the
    /// planner does not play holds (the army net's), the samples of the play kept.
    pub fn without_commitments(&self) -> Track {
        Track { engaged: false, waited: 0, resting: BTreeSet::new(), seen: BTreeMap::new(), ..self.clone() }
    }
}

pub fn is_assault(f: Fam) -> bool {
    matches!(f, Fam::AssaultHeal | Fam::PromotionChoice | Fam::HillArchers | Fam::CityFire | Fam::PlannedAssault) || any_cost(f)
}

/// The "at any cost" families: a deadline, no loss cap.
pub fn any_cost(f: Fam) -> bool {
    matches!(f, Fam::AssaultReinforce | Fam::AssaultPeace)
}

/// The most of ours that may be lost for the win (by family; the "at any cost" families: no cap).
pub fn loss_cap(f: Fam, n: usize) -> usize {
    match f {
        Fam::AssaultHeal | Fam::PromotionChoice => n / 2,
        f if any_cost(f) => usize::MAX,
        _ => 1,
    }
}

/// The round limit (the "at any cost" families: the longest N; the scene's own N is drawn in `build`).
pub fn rounds(f: Fam) -> u32 {
    match f {
        Fam::AssaultHeal | Fam::PlannedAssault => 30,
        Fam::PromotionChoice => 20,
        Fam::HillArchers => 16,
        Fam::AssaultReinforce => REINFORCE_N.1,
        Fam::AssaultPeace => PEACE_N.1,
        _ => 14,
    }
}

/// The round N of the "at any cost" families, (lowest, highest).
pub const REINFORCE_N: (u32, u32) = (6, 8);
pub const PEACE_N: (u32, u32) = (7, 9);
/// Where the army of an "at any cost" family stands (x from the arena centre; the city stands at 4).
const ANY_COST_ANCHOR: i64 = 0;

/// N of an instance: drawn from the family and the instance alone (the redraw of an attempt keeps it).
pub fn deadline_round(f: Fam, instance: usize) -> u32 {
    let (lo, hi) = if f == Fam::AssaultReinforce { REINFORCE_N } else { PEACE_N };
    lo + (super::lab::mix(0x0a11_c057 ^ (f as u64) << 32 ^ instance as u64) % u64::from(hi - lo + 1)) as u32
}

pub fn solvers(f: Fam) -> &'static [&'static str] {
    match f {
        Fam::AssaultHeal => &["ref-rotate", "ref-planned"],
        Fam::PromotionChoice => &["ref-promo", "ref-planned"],
        Fam::HillArchers => &["ref-hills", "ref-planned"],
        Fam::CityFire => &["ref-fire", "ref-planned"],
        f if any_cost(f) => &["ref-burst", "ref-planned"],
        _ => &["ref-planned", "ref-rotate"],
    }
}

pub fn task(f: Fam) -> &'static str {
    match f {
        Fam::AssaultHeal => "a multi-turn siege: rotate the wounded out of the city's fire, heal, come back",
        Fam::PromotionChoice => "promotions earned in a staged fight: heal, city-attack or cover by the plan",
        Fam::HillArchers => "archers take the hills from which they reach the city, minimising its return fire",
        Fam::CityFire => "invest a city while minimising the damage of its ranged strike",
        Fam::AssaultReinforce => "take the city before the enemy's reinforcements land, at any cost in units",
        Fam::AssaultPeace => "take the city, the war's goal, before the peace, at any cost in units",
        _ => "the whole plan: approach, hills, rotation, promotions, the capture",
    }
}

pub fn tricks(f: Fam) -> &'static [(&'static str, char)] {
    match f {
        Fam::AssaultHeal => &[("MO9", 'U'), ("V2", 'U')],
        Fam::PromotionChoice => &[("MT7", 'U')],
        Fam::HillArchers => &[("MT1", 'U'), ("MO9", 'U')],
        Fam::CityFire => &[("MO9", 'U'), ("MO10", 'U')],
        f if any_cost(f) => &[("MO9", 'U'), ("MO10", 'U')],
        _ => &[("MO9", 'U'), ("MO10", 'U'), ("V2", 'U')],
    }
}

// ---------------------------------------------------------------------------
// The scenes

/// Promotions of the unit's ladder, granted for the scene: the one with the most followers among those it
/// may take, twice (so the next choice has the heal, the cover and the city-attack branches).
fn grant_chain(p: &mut Painter, piece: &Piece) {
    let rules = p.rules;
    for _ in 0..2 {
        let Some(core) = act::core_of(&p.game, piece.h) else { return };
        let avail = available_promotions(p.game.view().auth, rules, &core);
        let followers = |a: PromotionId| rules.promotions.iter().filter(|q| q.choosable && (q.requires.contains(&a) || q.requires_any_of.contains(&a))).count();
        let best = avail.iter().copied().filter(|a| !rules.promotions[a.0 as usize].heals_instantly()).max_by_key(|a| (followers(*a), std::cmp::Reverse(a.0)));
        match best {
            Some(b) => p.game.debug_grant_promotion(rules, piece.h, b),
            None => return,
        }
    }
}

fn wound(p: &mut Painter, piece: &Piece, lo: i64, hi: i64) {
    let hp = (i64::from(UNIT_MAX_HP) * p.d.range(lo, hi) / 100).max(1) as u16;
    p.game.debug_set_unit_hp(piece.h, hp);
}

/// Hills on the neighbours of `city_t` on the learner's side and on `n2` tiles two away there.
fn hill_ring(p: &mut Painter, city_t: TileId, n2: usize) -> Vec<TileId> {
    let (cx, _) = p.xy(city_t);
    let west: Vec<TileId> = tiles_within(&p.game, city_t, 2).into_iter().filter(|t| p.xy(*t).0 <= cx && p.free(*t)).collect();
    let near: Vec<TileId> = west.iter().copied().filter(|t| dist(&p.game, *t, city_t) == 1).collect();
    let mut far: Vec<TileId> = west.iter().copied().filter(|t| dist(&p.game, *t, city_t) == 2).collect();
    p.d.shuffle(&mut far);
    far.truncate(n2);
    let hills = p.lab.grounds.hills;
    p.paint(&near, hills);
    p.paint(&far, hills);
    far
}

/// Builds the scene of an assault family.
pub(super) fn build(p: &mut Painter, fam: Fam, lab: &Lab, instance: usize, attempt: u32) -> Result<Scene, String> {
    let rules = p.rules;
    let mut s = blank(fam, lab, instance, attempt);
    // The ground and the forces of an "at any cost" family are those of an older family's scene (its draws in the same order).
    let fam_any = fam;
    let fam = set_up(fam);
    let (me, foe) = (lab.learner, lab.enemy);
    let at = |p: &Painter, dx: i64, dy: i64| p.at(dx, dy).ok_or_else(|| "off the map".to_owned());
    let city_dx = if fam == Fam::PromotionChoice { 7 } else { 4 };
    let city_t = at(p, city_dx, 0)?;
    let pop = match fam {
        Fam::PromotionChoice | Fam::HillArchers => p.d.range(4, 6),
        _ => p.d.range(5, 8),
    } as u32;
    let enemy_city = p.city(foe, city_t, pop, false);
    let all = defences(rules, lab.era);
    let n_def = if matches!(fam, Fam::PromotionChoice | Fam::HillArchers) { 1 } else { all.len() };
    for b in all.into_iter().take(n_def) {
        p.game.debug_add_building(rules, enemy_city, b);
    }
    // A home city of ours: friendly land to heal in.
    let home = matches!(fam, Fam::AssaultHeal | Fam::PlannedAssault);
    if home {
        let home_t = at(p, -2, 0)?;
        p.city(me, home_t, 3, false);
        s.refuge = Some(home_t);
    }
    // The ground.
    match fam {
        Fam::HillArchers => {
            hill_ring(p, city_t, 3);
        }
        Fam::PlannedAssault => {
            hill_ring(p, city_t, 2);
            p.clutter(p.d.range(2, 4) as usize, &[city_t]);
        }
        Fam::CityFire => p.clutter(p.d.range(1, 3) as usize, &[city_t]),
        Fam::PromotionChoice => p.clutter(p.d.range(2, 4) as usize, &[city_t]),
        _ => p.clutter(p.d.range(1, 3) as usize, &[city_t]),
    }
    // The garrison.
    let gk = p.kind(if p.d.chance(50) { Arm::Ranged } else { Arm::Melee }).ok_or("no garrison")?;
    let g = p.put(foe, city_t, gk, "garrison");
    s.theirs.push(g);
    if matches!(fam, Fam::AssaultHeal | Fam::PlannedAssault) && p.d.chance(60) {
        let t = p.spots(city_t, 1, 1)?[0];
        let k = p.kind(Arm::Melee).ok_or("no melee")?;
        let piece = p.put(foe, t, k, "garrison");
        s.theirs.push(piece);
    }
    if fam == Fam::PromotionChoice {
        let anchor = at(p, 1, 0)?;
        for t in p.spots(anchor, 1, 2)? {
            let k = p.kind(if p.d.chance(40) { Arm::Ranged } else { Arm::Melee }).ok_or("no field kind")?;
            let piece = p.put(foe, t, k, "field");
            s.theirs.push(piece);
        }
    }
    // The army.
    let mut plan: Vec<(Arm, &'static str)> = Vec::new();
    let add = |plan: &mut Vec<(Arm, &'static str)>, arm: Arm, role: &'static str, n: i64| {
        for _ in 0..n {
            plan.push((arm, role));
        }
    };
    match fam {
        Fam::AssaultHeal | Fam::CityFire | Fam::PlannedAssault => {
            // assault-peace: a fourth melee unit (the hills round the city cost the burst its melee).
            add(&mut plan, Arm::Melee, "capturer", if fam_any == Fam::AssaultPeace { 4 } else { 3 });
            add(&mut plan, Arm::Ranged, "ranged", 3);
            add(&mut plan, Arm::Siege, "siege", 3);
        }
        Fam::PromotionChoice => {
            add(&mut plan, Arm::Melee, "capturer", 3);
            add(&mut plan, Arm::Ranged, "ranged", 2);
            add(&mut plan, Arm::Siege, "siege", 2);
        }
        _ => {
            add(&mut plan, Arm::Melee, "capturer", 2);
            add(&mut plan, Arm::Ranged, "ranged", p.d.range(3, 4));
            add(&mut plan, Arm::Siege, "siege", p.d.range(1, 2));
        }
    }
    // The "at any cost" families: the army stands at the edge of the city's strike (the deadline is short).
    let anchor = if any_cost(fam_any) { at(p, ANY_COST_ANCHOR, 0)? } else if fam == Fam::PromotionChoice { at(p, -4, 0)? } else if home { at(p, -2, 0)? } else { at(p, -3, 0)? };
    let spots = p.spots(anchor, 2, plan.len())?;
    for ((arm, role), t) in plan.iter().zip(spots) {
        let k = p.kind(*arm).ok_or("no kind")?;
        let piece = p.put(me, t, k, role);
        s.mine.push(piece);
    }
    // Worn units: the march, the earlier fights.
    if matches!(fam, Fam::AssaultHeal | Fam::PlannedAssault) {
        let mut ix: Vec<usize> = (0..s.mine.len()).collect();
        p.d.shuffle(&mut ix);
        // assault-peace: five worn (the careful plan rests them; the deadline does not wait).
        for i in ix.into_iter().take(if fam_any == Fam::AssaultPeace { 5 } else { 3 }) {
            let piece = s.mine[i].clone();
            wound(p, &piece, 45, 70);
        }
    }
    // Promotions to choose: a ladder of two, experience a few points short of the third.
    if matches!(fam, Fam::PromotionChoice | Fam::PlannedAssault) {
        let pieces: Vec<Piece> = s.mine.clone();
        for (i, piece) in pieces.iter().enumerate() {
            if fam == Fam::PlannedAssault && i % 2 == 1 {
                continue;
            }
            grant_chain(p, piece);
            let ranged = act::range_of(rules, piece.kind).is_some();
            p.game.debug_set_experience(piece.h, if ranged { 8 } else { 5 });
        }
    }
    s.target = Some(city_t);
    s.aim_learner = Some(city_t);
    s.centre = if fam == Fam::PromotionChoice { at(p, 3, 0)? } else { city_t };
    s.rounds = rounds(fam);
    if any_cost(fam_any) {
        let n = deadline_round(fam_any, instance);
        s.rounds = n;
        if fam_any == Fam::AssaultReinforce {
            s.deadline = Some(n - 1);
            let strong = lab.kinds.by_arm.get(&Arm::Melee).and_then(|v| v.iter().copied().max_by_key(|k| (rules.units[k.0 as usize].strength, k.0))).ok_or("no melee")?;
            s.reinforce = vec![strong; p.d.range(2, 3) as usize];
            s.info.insert("reinforce".into(), format!("{} x{} at round {n}", rules.units[strong.0 as usize].name, s.reinforce.len()));
        } else {
            s.deadline = Some(n);
            s.info.insert("peace".into(), format!("round {n}"));
        }
    }
    s.info.insert("mine".into(), kinds_label(rules, &s.mine));
    s.info.insert("theirs".into(), kinds_label(rules, &s.theirs));
    Ok(s)
}

/// The older family whose scene an "at any cost" family is drawn as (the same ground, forces and draws).
fn set_up(f: Fam) -> Fam {
    match f {
        Fam::AssaultReinforce => Fam::AssaultHeal,
        Fam::AssaultPeace => Fam::PlannedAssault,
        f => f,
    }
}

// ---------------------------------------------------------------------------
// The measures: damage and promotions seen round by round

/// The assault scenes last longer than the war's minimum: the AI's peace is no part of the task, so the two
/// nations stay at war (the assault is the scene's given, as the closed arena is).
pub fn keep_war(rules: &CompiledRuleset, game: &mut GameState, sc: &Scene) {
    if is_assault(sc.fam) && !game.view().auth.diplomacy().at_war(sc.learner, sc.enemy) {
        game.debug_force_war(rules, sc.learner, sc.enemy);
    }
}

/// Called after every step of the game (`active`: whose turn the step played) and after the scripted
/// turn: what changed in our units' hit points, the city's, and their promotions.
pub fn sample(rules: &CompiledRuleset, game: &GameState, sc: &mut Scene, active: PlayerId) {
    if !is_assault(sc.fam) {
        return;
    }
    let mut now: BTreeMap<EntityId, i64> = BTreeMap::new();
    let mut promos: BTreeMap<EntityId, Vec<PromotionId>> = BTreeMap::new();
    let round = game.view().auth.turn().round.saturating_sub(sc.start_round);
    for (p, c) in sc.mine_alive(game) {
        now.insert(p.id, i64::from(c.hp));
        let list: Vec<PromotionId> = c.promotions.iter().collect();
        if let Some(prev) = sc.track.prev_promos.get(&p.id) {
            for q in &list {
                if !prev.contains(q) {
                    let name = rules.promotions.get(q.0 as usize).map_or("?", |d| d.name.as_str());
                    let kind = &rules.units[p.kind.0 as usize].name;
                    sc.track.picks.push(format!("r{round} {kind} {name}"));
                }
            }
        }
        promos.insert(p.id, list);
    }
    let mut dmg = 0;
    for (id, hp) in &sc.track.prev_hp {
        match now.get(id) {
            Some(n) if n < hp => dmg += hp - n,
            None => dmg += hp,
            _ => {}
        }
    }
    if active == sc.enemy {
        sc.track.dmg_enemy_turn += dmg;
    } else {
        sc.track.dmg_own_turn += dmg;
    }
    if let Some(t) = sc.target {
        let auth = game.view().auth;
        if let Some(c) = auth.indexes().city_on_tile(t).and_then(|c| auth.cities().get(c)) {
            let max = i64::from(classic_rules::combat::city::max_hit_points(auth, rules, c));
            let hp = max - i64::from(c.damage);
            if let Some(prev) = sc.track.prev_city_hp {
                if hp < prev && c.owner == sc.enemy && active != sc.enemy {
                    sc.track.city_dealt += prev - hp;
                }
            }
            sc.track.prev_city_hp = Some(hp);
        }
    }
    sc.track.prev_hp = now;
    sc.track.prev_promos = promos;
}

// ---------------------------------------------------------------------------
// Promotions priced by the plan

/// What a promotion is worth to `unit` in the plan: the rules' effects (no promotion is named) weighted by what the
/// unit will do in the assault. Healing counts by the hit points missing (the instant heal) or by the turns the
/// army has to stay in the field (no friendly land near: `field`).
pub fn promo_value(rules: &CompiledRuleset, p: PromotionId, unit: &UnitCore, shooter: bool, field: bool) -> i64 {
    let def = &rules.promotions[p.0 as usize];
    let missing = i64::from(UNIT_MAX_HP) - i64::from(unit.hp);
    let mut v: i64 = 0;
    for e in &def.effects.passive_unit {
        match &e.modifier {
            Modifier::ModifyCombat { context, percent } => {
                let w = match (context, shooter) {
                    (CombatContext::VsCity, _) => 100,
                    (CombatContext::RangedAttack, true) => 90,
                    (CombatContext::Any, _) => 80,
                    (CombatContext::Attack, _) => 60,
                    (CombatContext::VsFortified, true) => 70,
                    (CombatContext::RangedAttackOpen | CombatContext::RangedAttackRough, true) => 50,
                    (CombatContext::DefenseVsRanged, _) => 60,
                    (CombatContext::Defense, _) => 40,
                    (CombatContext::AttackOpen | CombatContext::AttackRough, false) => 40,
                    (CombatContext::DefenseOpen | CombatContext::DefenseRough, _) => 25,
                    (CombatContext::AdjacentFriendly | CombatContext::VsWounded, _) => 25,
                    _ => 0,
                };
                v += w * i64::from(*percent) / 100;
            }
            Modifier::ModifyUnitStat { stat, amount } => match stat {
                UnitStat::Range => v += 40 * i64::from(*amount),
                UnitStat::AttacksPerTurn => v += 30 * i64::from(*amount),
                UnitStat::HealAdjacent => v += (if field { 6 } else { 3 }) * i64::from(*amount),
                UnitStat::HealEnemyLand | UnitStat::HealNeutralLand => v += (if field { 3 } else { 1 }) * i64::from(*amount),
                _ => {}
            },
            Modifier::ModifyMovement { amount } => v += 15 * i64::from(*amount),
            Modifier::GrantAbility(UnitAbility::InstaHeal) => v += missing.min(i64::from(classic_rules::combat::experience::INSTANT_HEAL_HIT_POINTS)) * 3 / 2,
            Modifier::GrantAbility(UnitAbility::AlwaysHeal) => v += if field { 35 } else { 12 },
            Modifier::GrantAbility(UnitAbility::MoveAfterAttacking) => v += 30,
            _ => {}
        }
    }
    v
}

fn promote_all(rules: &CompiledRuleset, game: &mut GameState, sc: &mut Scene) {
    let me = sc.learner;
    let field = sc.refuge.is_none();
    for p in sc.mine.clone() {
        let Some(c) = act::alive(game, &p, me) else { continue };
        let auth = game.view().auth;
        if c.experience < experience_needed_by(auth, rules, &c) {
            continue;
        }
        let avail = available_promotions(auth, rules, &c);
        let shooter = act::range_of(rules, c.kind).is_some();
        let Some(best) = avail.iter().copied().max_by_key(|a| (promo_value(rules, *a, &c, shooter, field), std::cmp::Reverse(a.0))) else { continue };
        if act::try_cmd(game, rules, me, Command::Promote { unit: p.h, promotion: best }) {
            let round = game.view().auth.turn().round.saturating_sub(sc.start_round);
            let kind = &rules.units[c.kind.0 as usize].name;
            sc.track.picks.push(format!("r{round} {kind} {}", rules.promotions[best.0 as usize].name));
        }
    }
}

// ---------------------------------------------------------------------------
// The planner

#[derive(Debug, Clone, Copy)]
pub struct Flags {
    pub tiles: bool,
    pub rotate: bool,
    pub promote: bool,
    /// Every melee unit strikes the city every turn once engaged (`burst_melee`), the capture at the first chance.
    pub burst: bool,
}

pub fn flags(name: &str) -> Option<Flags> {
    Some(match name {
        "ref-fire" => Flags { tiles: false, rotate: false, promote: false, burst: false },
        "ref-hills" => Flags { tiles: true, rotate: false, promote: false, burst: false },
        "ref-rotate" => Flags { tiles: true, rotate: true, promote: false, burst: false },
        "ref-promo" => Flags { tiles: true, rotate: false, promote: true, burst: false },
        "ref-planned" => Flags { tiles: true, rotate: true, promote: true, burst: false },
        "ref-burst" => Flags { tiles: true, rotate: false, promote: true, burst: true },
        _ => return None,
    })
}

fn city_hp(rules: &CompiledRuleset, game: &GameState, t: TileId) -> Option<(i64, i64)> {
    let auth = game.view().auth;
    let c = auth.indexes().city_on_tile(t).and_then(|c| auth.cities().get(c))?;
    let max = i64::from(classic_rules::combat::city::max_hit_points(auth, rules, c));
    Some((max - i64::from(c.damage), max))
}

fn heal_rate(game: &GameState, rules: &CompiledRuleset, me: PlayerId, t: TileId) -> i64 {
    let auth = game.view().auth;
    let base = if auth.indexes().city_on_tile(t).is_some() {
        25
    } else if is_friendly_territory(auth, me, t) {
        20
    } else {
        10
    };
    let _ = rules;
    base
}

/// A unit rests: it stays (and fortifies where the rules let it), heals what the end of the turn would give, and the
/// seat's AI does not move it.
fn rest(game: &mut GameState, rules: &CompiledRuleset, me: PlayerId, h: UnitHandle) {
    let _ = act::fortify(game, rules, me, h);
    game.debug_hold_unit(rules, h);
}

/// The targets of the siege: the city's tile and the enemy units within two of it.
fn siege_targets(game: &GameState, sc: &Scene) -> Vec<TileId> {
    let city = sc.target.unwrap_or(sc.centre);
    let mut v: Vec<TileId> = sc.theirs_alive(game).iter().map(|(_, c)| c.tile).filter(|t| dist(game, *t, city) <= 2).collect();
    let auth = game.view().auth;
    if auth.indexes().city_on_tile(city).and_then(|c| auth.cities().get(c)).is_some_and(|c| c.owner == sc.enemy) {
        v.push(city);
    }
    v.sort_by_key(|t| t.0);
    v.dedup();
    v
}

fn can_fire_from(game: &GameState, _rules: &CompiledRuleset, kind_range: i32, t: TileId, targets: &[TileId]) -> bool {
    targets.iter().any(|x| dist(game, t, *x) <= kind_range && kernel::derived::visibility::los_clear(game.view().auth.map(), t, *x))
}

/// The tiles a shooter of `range` could fire at the siege targets from (standable, no unit of the foe, a clear line).
fn firing_tiles(game: &GameState, rules: &CompiledRuleset, city: TileId, range: i32, targets: &[TileId]) -> Vec<TileId> {
    let auth = game.view().auth;
    tiles_within(game, city, range + 2)
        .into_iter()
        .filter(|t| crate::battle::standable(rules, game, *t) && auth.indexes().city_on_tile(*t).is_none() && can_fire_from(game, rules, range, *t, targets))
        .collect()
}

/// One turn of the learner's seat.
pub fn turn(rules: &CompiledRuleset, game: &mut GameState, sc: &mut Scene, f: Flags) {
    let (me, foe) = (sc.learner, sc.enemy);
    let city = sc.target.unwrap_or(sc.centre);
    let round = game.view().auth.turn().round.saturating_sub(sc.start_round);
    if f.promote {
        promote_all(rules, game, sc);
    }
    let friends: Vec<UnitHandle> = sc.mine.iter().map(|p| p.h).collect();
    // Stage one of a fight with a field force: the competent fight (ranged behind the melee, a blow at even odds or a kill); the
    // assault on the city is stage two.
    let field: Vec<Piece> = sc.theirs_alive(game).into_iter().filter(|(p, _)| p.role == "field").map(|(p, _)| p).collect();
    if !field.is_empty() {
        let foes = |g: &GameState| -> Vec<TileId> { act::foe_tiles(g, foe, Some(&field), false) };
        let mut order: Vec<Piece> = sc.mine.clone();
        order.sort_by_key(|p| (u8::from(!matches!(p.role, "ranged" | "siege")), p.id.0));
        for p in order {
            let Some(c) = act::alive(game, &p, me) else { continue };
            let aim = if act::range_of(rules, c.kind).is_some() { Aim::Support } else { Aim::Close };
            act::unit_turn(game, rules, me, foe, p.h, &aim, Nerve::Even, &foes, &friends);
        }
        return;
    }
    let by_city = {
        let scc = sc.clone();
        move |g: &GameState| siege_targets(g, &scc)
    };
    // The rotation: who rests.
    let low = city_hp(rules, game, city).is_some_and(|(hp, max)| hp * 10 <= max * 3);
    for (p, c) in sc.mine_alive(game) {
        let rests = sc.track.resting.contains(&p.id);
        // In the burst a shooter keeps firing down to the weakest strike's reach (a strike takes up to 90): the city heals 20 a
        // turn, so a shooter that leaves at 70 gives the city its healing back.
        let below = if sc.track.engaged && matches!(p.role, "ranged" | "siege") { ROTATE_BURST } else { ROTATE_BELOW };
        if f.rotate && !low && !rests && c.hp < below {
            sc.track.resting.insert(p.id);
        }
        if rests && (c.hp >= ROTATE_BACK || low) {
            sc.track.resting.remove(&p.id);
        }
    }
    let alive = sc.mine_alive(game);
    let in_range = |g: &GameState, t: TileId| dist(g, t, city) <= CITY_ATTACK_RANGE;
    // 1. The resting go (or stay) out of the city's reach, to the best healing ground.
    for (p, c) in &alive {
        if !sc.track.resting.contains(&p.id) {
            continue;
        }
        let refuge = sc.refuge.unwrap_or(c.tile);
        let best = act::reachable(game, rules, me, p.h)
            .into_iter()
            .filter(|(t, _)| act::free_for(game, rules, p.h, *t))
            .map(|(t, _)| t)
            .min_by_key(|t| (in_range(game, *t), act::threat_on(game, rules, foe, *t) > 0, -heal_rate(game, rules, me, *t), dist(game, *t, refuge), t.0));
        if let Some(t) = best {
            if t != c.tile && (heal_rate(game, rules, me, t) > heal_rate(game, rules, me, c.tile) || in_range(game, c.tile) || act::threat_on(game, rules, foe, c.tile) > act::threat_on(game, rules, foe, t)) {
                let _ = act::try_cmd(game, rules, me, Command::MoveUnit { unit: p.h, destination: t });
                act::stand(game, rules, p.h);
                continue;
            }
        }
        sc.track.rested += 1;
        rest(game, rules, me, p.h);
    }
    let active: Vec<(Piece, UnitCore)> = alive.into_iter().filter(|(p, _)| !sc.track.resting.contains(&p.id)).collect();
    let targets = siege_targets(game, sc);
    let any_shooter_alive = sc.mine_alive(game).iter().any(|(p, _)| matches!(p.role, "ranged" | "siege"));
    let stage_key = |game: &GameState, t: TileId, from: TileId| (in_range(game, t), (dist(game, t, city) - 3).abs(), act::threat_on(game, rules, foe, t), dist(game, t, from), -act::defence(game, rules, t, me), t.0);
    // 2. Go together, or stage.
    if !sc.track.engaged {
        // (can fire this turn, can arrive on a firing tile): the melee wait for the city to be worn, ready when near enough.
        let ready = |game: &GameState, p: &Piece, c: &UnitCore| -> (bool, bool) {
            let reach = act::reachable(game, rules, me, p.h);
            if let Some(r) = act::range_of(rules, c.kind) {
                let firing = |left_needed: bool| reach.iter().any(|(t, left)| (!left_needed || *left > 0 || *t == c.tile) && act::free_for(game, rules, p.h, *t) && can_fire_from(game, rules, r, *t, &targets));
                (firing(true), firing(false))
            } else {
                let near = dist(game, c.tile, city) <= 5;
                (near, near)
            }
        };
        if act::debug() {
            for (p, c) in &active {
                eprintln!("  stage r{round} {} {:?} at {} d{} hp {} moves {} ready {:?} reach {}", rules.units[c.kind.0 as usize].name, p.role, c.tile.0, dist(game, c.tile, city), c.hp, c.moves_left, ready(game, p, c), act::reachable(game, rules, me, p.h).len());
            }
        }
        let states: Vec<(bool, bool)> = active.iter().map(|(p, c)| ready(game, p, c)).collect();
        // Every shooter can at least arrive on a firing tile (those that can fire at once do; the rest arrive and fire next turn).
        // With fewer firing tiles than shooters (hills round the city) the surplus shooters are a reserve: the army goes
        // once as many shooters can arrive as there are tiles for them.
        let shooters = active.iter().filter(|(p, _)| matches!(p.role, "ranged" | "siege")).count();
        let slots: std::collections::BTreeSet<TileId> = active
            .iter()
            .filter(|(p, _)| matches!(p.role, "ranged" | "siege"))
            .filter_map(|(_, c)| act::range_of(rules, c.kind))
            .flat_map(|r| firing_tiles(game, rules, city, r, &targets))
            .collect();
        let arrive = states.iter().zip(&active).filter(|(s, (p, _))| s.1 && matches!(p.role, "ranged" | "siege")).count();
        let all_ready = arrive >= shooters.min(slots.len().max(1)) && states.iter().zip(&active).all(|(s, (p, _))| s.1 || matches!(p.role, "ranged" | "siege"));
        // The army goes whole: while a unit still rests the others wait (the rotation's first turns), up to a limit.
        let wait_for_rest = !sc.track.resting.is_empty() && sc.track.waited < 4;
        // The burst waits at most `BURST_WAIT` turns (time is what it buys with units).
        if (all_ready && !wait_for_rest) || active.is_empty() || sc.track.waited >= 5 || (f.burst && sc.track.waited >= BURST_WAIT) {
            sc.track.engaged = true;
        } else {
            sc.track.waited += 1;
            for (p, c) in &active {
                // A unit that stands staged (outside the strike, in reach of its firing or striking tile) waits; the others close in.
                if !in_range(game, c.tile) && ready(game, p, c).1 && dist(game, c.tile, city) <= 3 {
                    act::stand(game, rules, p.h);
                    continue;
                }
                let best = act::reachable(game, rules, me, p.h)
                    .into_iter()
                    .filter(|(t, _)| act::free_for(game, rules, p.h, *t))
                    .map(|(t, _)| t)
                    .min_by_key(|t| stage_key(game, *t, c.tile))
                    .unwrap_or(c.tile);
                let moved = best != c.tile && act::try_cmd(game, rules, me, Command::MoveUnit { unit: p.h, destination: best });
                if act::debug() {
                    eprintln!("  stage-move {} {} -> {} d{} moved {moved}", rules.units[c.kind.0 as usize].name, c.tile.0, best.0, dist(game, best, city));
                }
                act::stand(game, rules, p.h);
            }
            return;
        }
    }
    // 3. The shooters, then the rest.
    let targets_fn: &dyn Fn(&GameState) -> Vec<TileId> = &by_city;
    for (p, c) in active.iter().filter(|(p, _)| matches!(p.role, "ranged" | "siege")) {
        let range = act::range_of(rules, c.kind).unwrap_or(1);
        let reach: Vec<(TileId, u16)> = act::reachable(game, rules, me, p.h).into_iter().filter(|(t, _)| act::free_for(game, rules, p.h, *t)).collect();
        // The firing tile: out of the city's strike if the range allows, else (tiles) a tile it can shoot from this turn, the best defence, the least threatened.
        let firing = reach.iter().filter(|(t, _)| can_fire_from(game, rules, range, *t, &targets)).map(|(t, left)| (*t, *left));
        let pick = if f.burst {
            // The burst fires this turn: from where it stands if it can, else from the nearest firing tile with moves left (a
            // siege unit sets up there).
            // Not next to the city (the melee strike from there).
            firing.filter(|(t, left)| *t == c.tile || *left > 0).min_by_key(|(t, _)| (dist(game, *t, city) <= 1, u8::from(*t != c.tile), in_range(game, *t), -act::defence(game, rules, *t, me), act::threat_on(game, rules, foe, *t), t.0)).map(|x| x.0)
        } else if f.tiles {
            firing.min_by_key(|(t, left)| (in_range(game, *t), u8::from(*left == 0 && *t != c.tile), -act::defence(game, rules, *t, me), act::threat_on(game, rules, foe, *t), u8::from(*t != c.tile), t.0)).map(|x| x.0)
        } else {
            firing.min_by_key(|(t, left)| (u8::from(*left == 0 && *t != c.tile), dist(game, *t, city), u8::from(*t != c.tile), t.0)).map(|x| x.0)
        };
        match pick {
            Some(t) => {
                if t != c.tile {
                    let _ = act::try_cmd(game, rules, me, Command::MoveUnit { unit: p.h, destination: t });
                }
                // A siege unit sets up first (the rules' set-up order); the preview of its blow reads the set-up stance.
                let _ = act::try_cmd(game, rules, me, Command::SetUpRangedAttack { unit: p.h });
                let blow = act::best_blow(game, rules, me, p.h, &targets_fn(game), Nerve::Reckless);
                let struck = blow.is_some_and(|(_, x)| act::strike(game, rules, me, p.h, x));
                if act::debug() && blow.is_none() {
                    for x in targets_fn(game) {
                        let how = classic_rules::combat::exchange::AttackKind::Ranged;
                        let e = classic_rules::combat::exchange::plan_attack(game.view(), rules, p.h, x, how).err();
                        eprintln!("     target {} accepts {:?} plan err {:?} cmd {:?}", x.0, act::attack_cmd(game, rules, p.h, x).map(|cmd| game.accepts_order(rules, me, &cmd)), e, act::attack_cmd(game, rules, p.h, x));
                    }
                }
                if act::debug() {
                    eprintln!("  shoot r{round} {} from {} d{} blow {:?} struck {struck} moves {:?}", rules.units[c.kind.0 as usize].name, t.0, dist(game, t, city), blow, act::core_of(game, p.h).map(|u| u.moves_left));
                }
            }
            None => {
                // No firing tile in reach this turn: toward the nearest one (outside the strike first).
                let goals = firing_tiles(game, rules, city, range, &targets);
                let goal = goals.iter().copied().min_by_key(|g| (in_range(game, *g), dist(game, c.tile, *g), g.0));
                if let Some(g) = goal {
                    let best = reach.iter().map(|(t, _)| *t).min_by_key(|t| (dist(game, *t, g), in_range(game, *t), t.0));
                    if let Some(t) = best.filter(|t| *t != c.tile) {
                        let _ = act::try_cmd(game, rules, me, Command::MoveUnit { unit: p.h, destination: t });
                    }
                }
            }
        }
        act::stand(game, rules, p.h);
    }
    let go_in = low || !any_shooter_alive;
    // The burst's capturer: the last melee unit alive waits two tiles off until a blow takes the city (the others spend
    // themselves on it).
    let melee: Vec<EntityId> = active.iter().filter(|(p, _)| !matches!(p.role, "ranged" | "siege")).map(|(p, _)| p.id).collect();
    let capturer = if f.burst && melee.len() == 1 { melee.first().copied() } else { None };
    for (p, c) in &active {
        if matches!(p.role, "ranged" | "siege") {
            continue;
        }
        if f.burst {
            burst_melee(rules, game, sc, p.h, city, targets_fn, capturer == Some(p.id));
            continue;
        }
        if go_in || city_hp(rules, game, city).is_some_and(|(hp, max)| hp * 2 <= max) {
            act::unit_turn(game, rules, me, foe, p.h, &Aim::To(city), if low { Nerve::Reckless } else { Nerve::Even }, targets_fn, &friends);
        } else {
            let best = act::reachable(game, rules, me, p.h)
                .into_iter()
                .filter(|(t, _)| act::free_for(game, rules, p.h, *t))
                .map(|(t, _)| t)
                .min_by_key(|t| stage_key(game, *t, c.tile))
                .unwrap_or(c.tile);
            if best != c.tile {
                let _ = act::try_cmd(game, rules, me, Command::MoveUnit { unit: p.h, destination: best });
            }
            // A foe that came out to us: strike it at good odds.
            if let Some((_, x)) = act::best_blow(game, rules, me, p.h, &act::foe_tiles(game, foe, Some(&sc.theirs), false), Nerve::Careful) {
                let _ = act::strike(game, rules, me, p.h, x);
            }
            act::stand(game, rules, p.h);
        }
    }
    let _ = round;
}

/// A melee unit of the burst: the best blow it survives by the preview (or one that kills: the capture), from where it
/// stands or from a reachable tile with moves left (a kill first, then the most dealt, then the least threatened tile);
/// else it closes on the city. The capturer (the last melee unit) strikes only a blow that takes the city (or kills) and else waits two tiles off (next to the
/// city once it is down to a third).
fn burst_melee(rules: &CompiledRuleset, game: &mut GameState, sc: &Scene, h: UnitHandle, city: TileId, targets: &dyn Fn(&GameState) -> Vec<TileId>, capturer: bool) {
    let (me, foe) = (sc.learner, sc.enemy);
    let Some(c) = act::core_of(game, h).filter(|c| c.owner == me && c.moves_left > 0) else { return };
    let ts = targets(game);
    let mut best: Option<((i64, i64, i32, u32), TileId, TileId)> = None;
    for (from, left) in act::reachable(game, rules, me, h) {
        if (left == 0 && from != c.tile) || !act::free_for(game, rules, h, from) {
            continue;
        }
        for &t in &ts {
            if dist(game, from, t) > 1 {
                continue;
            }
            let Some((dealt, taken, hp)) = act::preview(game, rules, h, (from != c.tile).then_some(from), t) else { continue };
            let kills = dealt >= hp;
            if !kills && (capturer || taken >= i64::from(c.hp)) {
                continue;
            }
            let key = (-i64::from(kills), -dealt, act::threat_on(game, rules, foe, from), from.0);
            if best.as_ref().is_none_or(|b| key < b.0) {
                best = Some((key, from, t));
            }
        }
    }
    if let Some((_, from, t)) = best {
        if from == c.tile || act::try_cmd(game, rules, me, Command::MoveUnit { unit: h, destination: from }) {
            let _ = act::strike(game, rules, me, h, t);
        }
    } else if capturer {
        // Two tiles off; next to the city once it is down to a third (the blow from where it stands next turn: a hill next to
        // the city takes the moves of a step).
        let wait = if city_hp(rules, game, city).is_some_and(|(hp, max)| hp * 3 <= max) { 1 } else { CITY_ATTACK_RANGE };
        let _ = act::move_by(game, rules, me, h, |g, x, _| ((dist(g, x, city) - wait).abs(), act::threat_on(g, rules, foe, x)));
    } else {
        let _ = act::move_by(game, rules, me, h, |g, x, _| (dist(g, x, city), act::threat_on(g, rules, foe, x)));
    }
    act::stand(game, rules, h);
}

/// The enemy's reinforcements (assault-reinforce) land at the start of the enemy's turn of round N (the round after the
/// deadline) if the city is still the enemy's: in the city if no military unit holds it, else on the free tiles next to
/// it, the farthest from our units first. They are scene pieces (the closed arena keeps them), played by the enemy's AI.
fn land_reinforcements(rules: &CompiledRuleset, game: &mut GameState, sc: &mut Scene) {
    let (Some(d), Some(city)) = (sc.deadline, sc.target) else { return };
    let round = game.view().auth.turn().round.saturating_sub(sc.start_round);
    if sc.reinforce.is_empty() || round < d {
        return;
    }
    let kinds = std::mem::take(&mut sc.reinforce);
    let auth = game.view().auth;
    if auth.indexes().city_on_tile(city).and_then(|c| auth.cities().get(c)).is_none_or(|c| c.owner != sc.enemy) {
        return;
    }
    let ours: Vec<TileId> = sc.mine_alive(game).iter().map(|(_, c)| c.tile).collect();
    let empty = |g: &GameState, t: TileId| {
        let a = g.view().auth;
        crate::battle::standable(rules, g, t) && a.indexes().units_on(t).iter().filter_map(|h| a.units().get(*h)).all(|u| act::is_civilian(rules, u.core.kind))
    };
    let mut tiles: Vec<TileId> = tiles_within(game, city, 1).into_iter().filter(|t| empty(game, *t) && (*t == city || auth.indexes().city_on_tile(*t).is_none())).collect();
    tiles.sort_by_key(|t| (u8::from(*t != city), -act::nearest(game, *t, &ours), t.0));
    for (k, t) in kinds.into_iter().zip(tiles) {
        let h = game.debug_spawn_unit(rules, sc.enemy, t, k);
        let id = game.view().auth.units().get(h).expect("a spawned unit").core.id;
        sc.theirs.push(Piece { h, id, kind: k, role: "reinforce" });
    }
}

/// The hunters of the field force (promotion-choice): they strike at even odds or better, else close on the nearest of ours.
/// The reinforcements of assault-reinforce land first.
pub fn enemy_turn(rules: &CompiledRuleset, game: &mut GameState, sc: &mut Scene) {
    land_reinforcements(rules, game, sc);
    let (me, foe) = (sc.enemy, sc.learner);
    let friends: Vec<UnitHandle> = sc.theirs.iter().map(|p| p.h).collect();
    let ours = |g: &GameState| -> Vec<TileId> { sc.mine_alive(g).iter().map(|(_, c)| c.tile).collect() };
    for p in &sc.theirs {
        if p.role != "field" {
            continue;
        }
        let Some(c) = act::alive(game, p, me) else { continue };
        let ranged = act::range_of(rules, c.kind).is_some();
        act::unit_turn(game, rules, me, foe, p.h, if ranged { &Aim::Support } else { &Aim::Close }, Nerve::Even, &ours, &friends);
    }
}
