//! Synthetic battles: dense combat data for a learned model of danger. In
//! the AI's own games wars are rare (about 0.3 a game), and under 1 % of the
//! turns of military units end in damage taken in the enemies' turn. A
//! battle is a generated game played by the game's AI to a round, then the
//! two nations whose cities are closest are put at war, each with an army of
//! the units it can build at the border, facing the other's nearest city, so
//! the armies meet within a few rounds. The caller plays it on with any
//! mind; `Battle::play` plays it, keeps the war going (a peace is broken,
//! the losses come back every few rounds) and measures the fighting.
//!
//! Everything is deterministic: the same spec gives the same game (world
//! hash). The scene is set with the harness setters (war, contact, units,
//! gold, sight) and the AI memory a declaration of war leaves (the target
//! city as the war objective); the AI's code is not changed.
//!
//! Beside the exchange `Battle::play` credits every kill between the two
//! sides to the side whose strike or defence made it (`BattleStats::killed`)
//! and audits how each side fought (`FightAudit`, `Tally`): its strikes,
//! their odds and focus, where its units ended their turns and which of
//! them were lost (classic/docs/ai-combat-diagnosis.md). The audit only
//! reads the game: the world hash is the same with it.

use std::collections::BTreeMap;

use classic_rules::ai::driver::ClassicMind;
use classic_rules::classic::Classic;
use classic_rules::combat::exchange::CombatPreview;
use classic_rules::knowledge::public::PlayerQueries;
use classic_rules::ids::{PlayerId, ProductionItem, TileId, UnitHandle, UnitKindId};
use classic_rules::scenario::MapScript;
use classic_rules::{ClassicGame, Command, CompiledRuleset, GameState};
use kernel::ai::driver::AiDriver;
use kernel::ai::mind::AiMind;
use kernel::ai::plan::{role_of, AiRole, AiSettings};
use kernel::ai::state::{ObjectiveKind, OperationPhase, SeenCity, UnitTask, WarObjective};
use kernel::ai::{Advance, StepOutcome};
use kernel::ids::EntityId;
use kernel::persistence::ai::SavedAiStates;
use kernel::rules::content::{Domain, UnitRole};

use crate::spec::{GameSpec, WorldSize};

/// The earliest and the latest round a battle starts at by default.
pub const FIRST_START_ROUND: u32 = 60;
pub const LAST_START_ROUND: u32 = 160;
/// Units of each side's army by default.
pub const ARMY_UNITS: usize = 12;
/// Gold each side receives with its army: the upkeep of the new units would
/// otherwise empty the treasury and disband them.
pub const WAR_CHEST: u32 = 1_000;
/// An army stands at least this far from the enemy city it faces: outside
/// the city's strike (range 2), so the first blow is the AI's choice.
pub const MIN_GAP: i32 = 3;
/// Rounds between reinforcements by default (`BattleSpec::reinforce_every`):
/// a battle is densest in its first rounds (13 % of the unit-turns damaged
/// in the first three, 4 % in rounds 20-30 without reinforcements).
pub const REINFORCE_EVERY: u32 = 5;

/// A synthetic battle: a generated game played by the game's AI to a round,
/// then two neighbouring nations put at war with armies at their border.
#[derive(Debug, Clone)]
pub struct BattleSpec {
    pub seed: u64,
    pub script: MapScript,
    pub size: WorldSize,
    /// The round the war begins at.
    pub start_round: u32,
    /// Units of each side's army.
    pub army: usize,
    /// A peace the sides make is broken at the end of the round: the war
    /// goes on for the whole battle (the AI makes peace by a deal from 10
    /// rounds of war on, often after the first clash).
    pub hold_war: bool,
    /// Every this many rounds each army gets its lost units back on the
    /// land it was set down on (0: never): the fight keeps its density
    /// instead of fading as the armies bleed and go home to heal.
    pub reinforce_every: u32,
    /// Units of each army whose ranged slots (archers, siege) are taken by
    /// the cheapest land unit whose range exceeds a city's strike range
    /// (`outranger_kind`), whatever the side could build: the scene of a
    /// siege by artillery (0: none).
    pub outrangers: usize,
}

impl BattleSpec {
    /// A pangaea (one landmass: the armies can walk to each other) whose
    /// war begins at a round between 60 and 160 drawn from the seed.
    pub fn new(seed: u64, size: WorldSize) -> Self {
        let span = u64::from(LAST_START_ROUND - FIRST_START_ROUND + 1);
        let start_round = FIRST_START_ROUND + (mix(seed) % span) as u32;
        Self { seed, script: MapScript::Pangaea, size, start_round, army: ARMY_UNITS, hold_war: true, reinforce_every: REINFORCE_EVERY, outrangers: 0 }
    }

    pub fn start_round(mut self, round: u32) -> Self {
        self.start_round = round;
        self
    }

    pub fn army(mut self, units: usize) -> Self {
        self.army = units;
        self
    }

    pub fn hold_war(mut self, hold: bool) -> Self {
        self.hold_war = hold;
        self
    }

    pub fn reinforce_every(mut self, rounds: u32) -> Self {
        self.reinforce_every = rounds;
        self
    }

    pub fn outrangers(mut self, units: usize) -> Self {
        self.outrangers = units;
        self
    }

    /// The game the battle grows out of: the setup screen's defaults.
    pub fn game_spec(&self) -> GameSpec {
        GameSpec::new(self.script, self.size, self.seed)
    }

    pub fn label(&self) -> String {
        format!("{}/start {}", self.game_spec().label(), self.start_round)
    }
}

/// SplitMix64's finaliser: neighbouring seeds give unrelated start rounds.
fn mix(seed: u64) -> u64 {
    let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// One side's army as it was set down.
#[derive(Debug, Clone)]
pub struct Army {
    pub side: PlayerId,
    /// The enemy city the army faces.
    pub target: TileId,
    pub units: Vec<UnitHandle>,
    pub kinds: Vec<UnitKindId>,
    /// The distance from the army's nearest and farthest unit to the target.
    pub nearest: i32,
    pub farthest: i32,
    /// Units set down on land nobody owns (the rest on the side's own).
    pub outside: usize,
}

/// The game at the moment the war begins (the caller plays it on with any
/// mind: `driver` gives one with the AI memory of the rounds before).
#[derive(Debug)]
pub struct Battle {
    pub game: GameState,
    /// The declarer first.
    pub sides: [PlayerId; 2],
    pub armies: [Army; 2],
    /// The round the war began at.
    pub round: u32,
    /// The AI memory at the start of the war: the tasks, cities and
    /// relations the AI remembers from the rounds it played.
    pub memory: SavedAiStates,
    pub settings: AiSettings,
    /// See `BattleSpec::hold_war`.
    pub hold_war: bool,
    /// See `BattleSpec::reinforce_every`.
    pub reinforce_every: u32,
}

/// The game at the moment the war begins.
pub fn new_battle(rules: &CompiledRuleset, spec: &BattleSpec) -> Result<Battle, String> {
    let game_spec = spec.game_spec();
    let mut game = crate::record::new_game(rules, &game_spec);
    let settings = AiSettings::for_rules(rules, &game_spec.difficulty);
    // The world before the war: the scripted expert (`SCRIPTED_EXPERT`), as the scenes were measured.
    let mut driver = AiDriver::with_mind(settings.clone(), Vec::new(), ClassicMind::new(crate::bench::modules::SCRIPTED_EXPERT_VARIANT));
    driver.set_trace_capacity(0);
    let kinds = play_to(rules, &mut game, &mut driver, spec.start_round)?;
    let sides = declare_war(rules, &mut game)?;
    let kinds_of = |side: PlayerId| kinds.get(&side).map_or(&[][..], |k| &k[..]);
    let armies = [
        set_army(rules, &mut game, sides[0], sides[1], kinds_of(sides[0]), spec.army, spec.outrangers)?,
        set_army(rules, &mut game, sides[1], sides[0], kinds_of(sides[1]), spec.army, spec.outrangers)?,
    ];
    for side in sides {
        game.debug_earn(side, WAR_CHEST);
    }
    let round = game.view().auth.turn().round;
    for army in &armies {
        aim(&mut driver, &game, army.side, army.target, round);
        let tiles = approach(&game, army);
        game.debug_reveal(army.side, &tiles);
    }
    let memory = driver.save();
    Ok(Battle { game, sides, armies, round, memory, settings, hold_war: spec.hold_war, reinforce_every: spec.reinforce_every })
}

/// Plays `game` with `driver` to the start of round `round`. Returns the
/// land military units each player could build at the start of its turn of
/// the last round played (`buildable`: the rules answer for the active
/// player only, so the last round is played step by step).
pub(crate) fn play_to<P: AiMind<Classic>>(rules: &CompiledRuleset, game: &mut GameState, driver: &mut AiDriver<Classic, P>, round: u32) -> Result<BTreeMap<PlayerId, Vec<UnitKindId>>, String> {
    let mut kinds: BTreeMap<PlayerId, Vec<UnitKindId>> = BTreeMap::new();
    while game.view().auth.turn().round < round {
        if game.is_over() {
            return Err(format!("the game ended at round {}", game.view().auth.turn().round));
        }
        if game.view().auth.turn().round + 1 < round {
            let report = driver.run(game, rules, u32::MAX);
            if !report.round_done && !game.is_over() {
                return Err(format!("round {} waits for a human", game.view().auth.turn().round));
            }
            continue;
        }
        kinds.insert(game.view().auth.active_player(), buildable(rules, game));
        loop {
            match driver.step(game, rules) {
                StepOutcome::Worked => {}
                StepOutcome::PlayerDone(_) => {
                    kinds.insert(game.view().auth.active_player(), buildable(rules, game));
                }
                StepOutcome::RoundDone => break,
                StepOutcome::Waiting if game.is_over() => break,
                StepOutcome::Waiting => return Err(format!("round {} waits for a human", game.view().auth.turn().round)),
            }
        }
    }
    Ok(kinds)
}

/// The side remembers `target` as its war objective, the way the AI's own
/// declaration of war leaves its memory (`planner_strategy::consider_war`):
/// at its first turn of the war it reviews its strategy, attacks, and its
/// new units march on the city. Without it a forced war found both sides at
/// peace in their plans: the new units took garrison tasks and stood still
/// for the whole battle.
pub(crate) fn aim<P: AiMind<Classic>>(driver: &mut AiDriver<Classic, P>, game: &GameState, side: PlayerId, target: TileId, round: u32) {
    let auth = game.view().auth;
    let Some(city) = auth.indexes().city_on_tile(target).and_then(|c| auth.cities().get(c)) else {
        return;
    };
    let state = driver.state_mut(side, round);
    state.see_city(SeenCity { tile: target, owner: city.owner, population: city.population, last_seen_round: round });
    state.relation_mut(city.owner).objective = Some(WarObjective { tile: target, kind: ObjectiveKind::City, chosen_round: round });
}

/// The tiles around the army's target as far as the army stands: the side
/// is shown them (the AI's paths go over known land only; an army that
/// never saw the way found no path and stayed home).
fn approach(game: &GameState, army: &Army) -> Vec<TileId> {
    let map = game.view().auth.map();
    let reach = army.farthest + 2;
    (0..map.tile_count() as u32)
        .map(TileId)
        .filter(|t| map.dimensions.distance(*t, army.target).is_some_and(|d| d <= reach))
        .collect()
}

/// The living nations' pairs of cities on one landmass, nearest first; the
/// first pair whose war the rules accept goes to war.
fn declare_war(rules: &CompiledRuleset, game: &mut GameState) -> Result<[PlayerId; 2], String> {
    let auth = game.view().auth;
    let players = auth.players();
    let map = auth.map();
    let dims = map.dimensions;
    let majors: Vec<PlayerId> = crate::bench::modules::majors(game).into_iter().filter(|p| players.is_alive(*p)).collect();
    let cities = |p: PlayerId| -> Vec<TileId> {
        auth.indexes().cities_of(p).iter().filter_map(|h| auth.cities().get(*h)).map(|c| c.tile).collect()
    };
    let mut pairs: Vec<(i32, PlayerId, PlayerId)> = Vec::new();
    for (i, &a) in majors.iter().enumerate() {
        for &b in &majors[i + 1..] {
            let mut nearest: Option<i32> = None;
            for ta in cities(a) {
                for tb in cities(b) {
                    if map.land_area(ta).is_none() || map.land_area(ta) != map.land_area(tb) {
                        continue;
                    }
                    let d = dims.distance(ta, tb).unwrap_or(i32::MAX);
                    nearest = Some(nearest.map_or(d, |n| n.min(d)));
                }
            }
            if let Some(d) = nearest {
                pairs.push((d, a, b));
            }
        }
    }
    pairs.sort();
    for (_, a, b) in pairs {
        game.debug_meet(a, b);
        game.debug_force_war(rules, a, b);
        if game.view().auth.diplomacy().at_war(a, b) {
            return Ok([a, b]);
        }
    }
    Err("no two nations on one landmass could go to war".to_owned())
}

/// The arms of an army, in the order the front rows are filled: the melee
/// line first, the mounted beside it, the ranged and the siege behind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Arm {
    Melee,
    Mounted,
    Ranged,
    Siege,
}

/// The arms of an army of `n` units: two melee, four ranged, one mounted
/// and one siege in eight, repeated (twelve: four melee, six ranged). Half
/// ranged: the AI's ranged units shoot whenever a target is in range, its
/// melee units only at good odds, and the battle is meant to be dense (with
/// three melee and three ranged in eight, 7.7 % of the unit-turns took
/// damage on seeds 20-40, with four ranged 9.3 %).
fn arms(n: usize) -> Vec<Arm> {
    const PLAN: [Arm; 8] = [Arm::Melee, Arm::Ranged, Arm::Ranged, Arm::Melee, Arm::Ranged, Arm::Mounted, Arm::Ranged, Arm::Siege];
    (0..n).map(|i| PLAN[i % PLAN.len()]).collect()
}

fn arm_of(role: UnitRole) -> Option<Arm> {
    match role {
        UnitRole::Melee | UnitRole::Gun | UnitRole::Armor => Some(Arm::Melee),
        UnitRole::Mounted => Some(Arm::Mounted),
        UnitRole::Archer => Some(Arm::Ranged),
        UnitRole::Siege => Some(Arm::Siege),
        _ => None,
    }
}

/// A unique unit serves in the arm of the unit it replaces (the Assyrian
/// siege tower is a melee unit that attacks only cities, in place of the
/// catapult).
pub(crate) fn kind_arm(rules: &CompiledRuleset, kind: UnitKindId) -> Option<Arm> {
    let def = rules.units.get(kind.0 as usize)?;
    let base = def.replaces.and_then(|r| rules.units.get(r.0 as usize)).unwrap_or(def);
    base.role.and_then(arm_of)
}

/// The arm that stands in for one the side cannot build.
pub(crate) fn fallback(arm: Arm) -> Option<Arm> {
    match arm {
        Arm::Siege => Some(Arm::Ranged),
        Arm::Mounted | Arm::Ranged => Some(Arm::Melee),
        Arm::Melee => None,
    }
}

/// The land units of an arm the active player can build now: a city of it
/// would accept the order (technology, resources, nation, obsolescence).
/// The rules answer for the active player only, so this is asked at the
/// start of every player's turn of the last round before the war.
pub(crate) fn buildable(rules: &CompiledRuleset, game: &GameState) -> Vec<UnitKindId> {
    let side = game.view().auth.active_player();
    let cities = game.view().auth.indexes().cities_of(side).to_vec();
    let accepted = |kind: UnitKindId| {
        cities.iter().any(|&city| {
            let order = Command::EnqueueProduction { player: side, city, item: ProductionItem::Unit(kind) };
            game.accepts_order(rules, side, &order)
        })
    };
    (0..rules.units.len())
        .map(|i| UnitKindId(i as u16))
        .filter(|&kind| {
            let def = &rules.units[kind.0 as usize];
            kind_arm(rules, kind).is_some() && def.domain == Domain::Land && role_of(rules, kind) == AiRole::Military
        })
        .filter(|&kind| accepted(kind))
        .collect()
}

/// The strongest of `kinds` in each arm (the dearer one on a tie).
pub(crate) fn best_kinds(rules: &CompiledRuleset, kinds: &[UnitKindId]) -> BTreeMap<Arm, UnitKindId> {
    let mut best: BTreeMap<Arm, (u16, u32, UnitKindId)> = BTreeMap::new();
    for &kind in kinds {
        let def = &rules.units[kind.0 as usize];
        let Some(arm) = kind_arm(rules, kind) else { continue };
        let power = match arm {
            Arm::Ranged | Arm::Siege => def.ranged_strength.max(def.strength),
            Arm::Melee | Arm::Mounted => def.strength,
        };
        let better = best.get(&arm).is_none_or(|&(p, c, _)| (power, def.cost) > (p, c));
        if better {
            best.insert(arm, (power, def.cost, kind));
        }
    }
    best.into_iter().map(|(arm, (_, _, kind))| (arm, kind)).collect()
}

/// The cheapest land military unit of the ranged arms whose range exceeds a
/// city's strike range (`combat::city::CITY_ATTACK_RANGE`): the outranger of
/// `BattleSpec::outrangers`, from the rules' data.
pub fn outranger_kind(rules: &CompiledRuleset) -> Option<UnitKindId> {
    (0..rules.units.len())
        .map(|i| UnitKindId(i as u16))
        .filter(|&kind| {
            let def = &rules.units[kind.0 as usize];
            let outranges = classic_rules::ai::combat::ranged_range(rules, kind).is_some_and(|r| r > classic_rules::combat::city::CITY_ATTACK_RANGE);
            def.domain == Domain::Land && role_of(rules, kind) == AiRole::Military && outranges && matches!(kind_arm(rules, kind), Some(Arm::Ranged | Arm::Siege))
        })
        .min_by_key(|&kind| (rules.units[kind.0 as usize].cost, kind.0))
}

/// Land a unit may stand on: not water, not impassable terrain, feature
/// or natural wonder.
pub(crate) fn standable(rules: &CompiledRuleset, game: &GameState, tile: TileId) -> bool {
    let map = game.view().auth.map();
    if map.is_water(tile) || kernel::navigation::cost::enter_cost_on_map(map, tile).is_none() {
        return false;
    }
    let feature_blocks = map.feature(tile).flatten().and_then(|f| rules.features.get(f.0 as usize)).is_some_and(|f| f.impassable);
    let wonder_blocks = map.natural_wonder(tile).and_then(|w| rules.natural_wonders.get(w.0 as usize)).is_some_and(|w| w.impassable);
    !feature_blocks && !wonder_blocks
}

/// The city of `enemy` nearest a city of `side` on the same landmass.
pub(crate) fn front_city(game: &GameState, side: PlayerId, enemy: PlayerId) -> Option<TileId> {
    let auth = game.view().auth;
    let map = auth.map();
    let city_tiles = |p: PlayerId| -> Vec<TileId> {
        auth.indexes().cities_of(p).iter().filter_map(|h| auth.cities().get(*h)).map(|c| c.tile).collect()
    };
    let own = city_tiles(side);
    city_tiles(enemy)
        .into_iter()
        .filter_map(|t| {
            let d = own.iter().filter(|o| map.land_area(**o) == map.land_area(t)).filter_map(|o| map.dimensions.distance(*o, t)).min()?;
            Some((d, t))
        })
        .min()
        .map(|(_, t)| t)
}

/// Free land for `n` units of `side` facing `target`: its own land and the
/// land nobody owns on its half (no farther from its city nearest the
/// target than from the target), nearest the target first, at least
/// `MIN_GAP` from it. With the distance to the target.
pub(crate) fn front_tiles(rules: &CompiledRuleset, game: &GameState, side: PlayerId, target: TileId, n: usize) -> Vec<(i32, TileId)> {
    let auth = game.view().auth;
    let map = auth.map();
    let dims = map.dimensions;
    let area = map.land_area(target);
    let anchor = auth
        .indexes()
        .cities_of(side)
        .iter()
        .filter_map(|h| auth.cities().get(*h))
        .filter_map(|c| Some((dims.distance(c.tile, target)?, c.tile)))
        .min()
        .map_or(target, |(_, t)| t);
    let ours = |t: TileId| match map.owner(t).flatten() {
        Some(owner) => owner == side,
        None => dims.distance(t, anchor) <= dims.distance(t, target),
    };
    let mut tiles: Vec<(i32, TileId)> = (0..map.tile_count() as u32)
        .map(TileId)
        .filter(|&t| {
            ours(t)
                && map.land_area(t) == area
                && auth.indexes().city_on_tile(t).is_none()
                && auth.indexes().units_on(t).is_empty()
                && dims.distance(t, target).is_some_and(|d| d >= MIN_GAP)
                && standable(rules, game, t)
        })
        .map(|t| (dims.distance(t, target).unwrap_or(i32::MAX), t))
        .collect();
    tiles.sort();
    tiles.truncate(n);
    tiles
}

/// The army of `side` against `enemy`: the strongest unit of each arm it can
/// build (`kinds`), on the free land of its side nearest the enemy city
/// closest to its own cities (`front_tiles`), the melee line in front.
fn set_army(rules: &CompiledRuleset, game: &mut GameState, side: PlayerId, enemy: PlayerId, kinds: &[UnitKindId], n: usize, outrangers: usize) -> Result<Army, String> {
    let target = front_city(game, side, enemy).ok_or_else(|| format!("player {} has no city on the land of player {}", enemy.0, side.0))?;
    let tiles = front_tiles(rules, game, side, target, n);
    if tiles.len() < n {
        return Err(format!("player {} has room for {} of {n} units", side.0, tiles.len()));
    }
    let map = game.view().auth.map();
    let outside = tiles.iter().filter(|(_, t)| map.owner(*t).flatten().is_none()).count();
    let best = best_kinds(rules, kinds);
    let mut plan: Vec<(Arm, UnitKindId)> = Vec::with_capacity(n);
    for wanted in arms(n) {
        let mut arm = Some(wanted);
        while let Some(a) = arm {
            if let Some(&kind) = best.get(&a) {
                plan.push((a, kind));
                break;
            }
            arm = fallback(a);
        }
        if arm.is_none() {
            return Err(format!("player {} can build no melee unit", side.0));
        }
    }
    if let Some(kind) = (outrangers > 0).then(|| outranger_kind(rules)).flatten() {
        // The slots meant for the ranged arms, whatever stood in for them.
        let wanted = arms(n);
        let ranged = plan.iter_mut().zip(&wanted).filter(|(_, arm)| matches!(arm, Arm::Ranged | Arm::Siege));
        for (slot, _) in ranged.take(outrangers) {
            *slot = (Arm::Siege, kind);
        }
    }
    plan.sort_by_key(|(arm, kind)| (*arm, kind.0));
    let mut units = Vec::with_capacity(n);
    for (&(_, tile), &(_, kind)) in tiles.iter().zip(&plan) {
        units.push(game.debug_spawn_unit(rules, side, tile, kind));
    }
    Ok(Army {
        side,
        target,
        units,
        kinds: plan.iter().map(|(_, k)| *k).collect(),
        nearest: tiles.first().map_or(0, |t| t.0),
        farthest: tiles.last().map_or(0, |t| t.0),
        outside,
    })
}

/// What the battle came to, per side (the declarer first).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BattleStats {
    /// Rounds played after the war began.
    pub rounds: u32,
    /// Attacks applied (`AttackUnit`, `AttackCity`, `RangedAttack`,
    /// `CityStrike` that the rules accepted), on anyone.
    pub attacks: [u32; 2],
    /// Of those, attacks on the other side.
    pub attacks_on_enemy: [u32; 2],
    /// Military units lost in the enemies' turns.
    pub lost: [u32; 2],
    /// Hit points the military units lost in the enemies' turns (the whole
    /// health of a unit that was gone).
    pub hp_lost: [u32; 2],
    /// Cities taken from the other side.
    pub captured: [u32; 2],
    /// Turns of military units: from the end of one of the owner's turns to
    /// the start of its next.
    pub unit_turns: [u32; 2],
    /// Of those, turns whose unit lost hit points or was gone.
    pub damaged: [u32; 2],
    /// Units each side received as reinforcements.
    pub reinforcements: [u32; 2],
    /// Rounds that ended with the sides at peace (with `hold_war`, the war
    /// was forced again at each of them).
    pub peace_rounds: u32,
    /// Of `attacks_on_enemy`, strikes on the other side's cities (a melee
    /// attack on a city, a ranged strike on its tile).
    pub city_attacks: [u32; 2],
    /// The army's units (`Battle::armies`) at the end of each round on an
    /// assault or in a marching operation (front members), summed over the
    /// rounds of each window of `TASK_WINDOW` rounds since the war began (the
    /// last window takes the rest).
    pub assault: [[u32; TASK_WINDOWS]; 2],
    /// The army's units at the end of each round, alive, and of them those
    /// holding a garrison (`UnitTask::Defend`), summed over the rounds.
    pub army_rounds: [u32; 2],
    pub garrison_rounds: [u32; 2],
    /// Military units of the other side killed in the strikes between the
    /// two sides, credited to the side whose unit or city won the exchange:
    /// its strikes' victims, and the other side's attackers that died
    /// striking it. Unlike `lost`, it counts an attacker that dies in its own
    /// turn and leaves out the losses to third players (barbarians,
    /// city-states).
    pub killed: [u32; 2],
    /// Hit points the other side's military units lost in those strikes
    /// (an attacker's wounds from the defender's answer included).
    pub dealt: [u32; 2],
    /// How each side fought (`FightAudit`).
    pub audit: [FightAudit; 2],
    pub hash: u64,
}

/// What the fight audit counts for a side (`FightAudit`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tally {
    /// The side's strikes on the other side: melee on units, melee on
    /// cities, ranged on units, ranged on cities, its cities' strikes.
    MeleeOnUnits,
    MeleeOnCities,
    RangedOnUnits,
    RangedOnCities,
    CityStrikes,
    /// Units of the other side killed by the side's melee, ranged and city
    /// strikes, and attackers of the other side that died striking it.
    KillsMelee,
    KillsRanged,
    KillsCity,
    KillsDefending,
    /// Melee strikes the attacker's own preview expected to lose (more
    /// damage taken than dealt, the defender expected to live), and those
    /// whose worst roll kills it.
    BadOdds,
    DeadlyOdds,
    /// Melee strikes on a unit whose expected damage kills it.
    ExpectedKills,
    /// Side-turns with a strike on an enemy unit; the enemy units struck in
    /// a turn; of them those alive at its end; the strikes those took.
    StrikeTurns,
    Targets,
    TargetsSurvived,
    StrikesOnSurvivors,
    /// Of the strikes on a unit, those on the weakest (lowest health) enemy
    /// unit the striker could strike from where it stood.
    StrikesOnWeakest,
    /// Units that struck in a turn (counted once a turn), and of them those
    /// gone at the start of the side's next turn (lost to the reply).
    Strikers,
    LostAfterStriking,
    /// At the end of the side's turn: its land military units within 3
    /// tiles of an enemy military unit or city (front unit-turns); of them
    /// below half health out of its cities within 2 tiles of an enemy
    /// military unit or in an enemy city's strike; ranged ones out of its
    /// cities next to an enemy melee unit; ones that did not strike and
    /// could from where they stand; of those, ones with a strike the
    /// preview favours (a ranged shot, or melee dealing at least what it
    /// takes and never lethal).
    FrontTurns,
    WoundedExposed,
    RangedExposed,
    IdleWithStrike,
    IdleFavourable,
    /// Siege units' front unit-turns and strikes.
    SiegeTurns,
    SiegeStrikes,
    /// Land military units gone between the end of the side's turn and the
    /// start of its next, by where they had ended the turn (the first that
    /// fits): wounded and exposed, ranged next to an enemy melee unit, on
    /// the front out of its cities, in its cities, away from the front.
    LostWoundedExposed,
    LostRangedExposed,
    LostAtFront,
    LostInCity,
    LostAway,
    /// At the start of the side's turn, its land ranged units that can
    /// shoot an enemy unit from where they stand; the expected kills of
    /// their shots as the game's AI aims them (the military advisor's plan,
    /// `combat::ranged_orders`, then every shooter left re-aimed by the same
    /// score at what is left alive) and as a focus rule aims them (the
    /// enemy unit the fewest unused shooters kill first, the leftovers at
    /// the weakest unit alive), both on the previews' expected damage.
    Shooters,
    ShotKillsAi,
    ShotKillsFocus,
    /// The wounded left exposed (`WoundedExposed`) by their AI task: a
    /// guard of a city (`UnitTask::Defend`), a front member (an assault or a
    /// marching operation), told to heal, anything else.
    WoundedGuard,
    WoundedMember,
    WoundedHealing,
    WoundedOther,
    /// The units lost (as `LostWoundedExposed` and the next) by the task they
    /// ended their turn with.
    LostGuard,
    LostMember,
    LostHealing,
    LostOther,
    /// Of `WoundedHealing`: those standing on or next to the tile they were
    /// told to heal at, and those whose heal tile is itself exposed (within
    /// 2 tiles of an enemy military unit or in an enemy city's strike).
    HealHeld,
    HealTileExposed,
    /// Of the wounded left exposed (any task): those that stood where they
    /// had begun the turn, and those with moves left.
    WoundedStayed,
    WoundedMovesLeft,
    /// Of the wounded left exposed: ranged units, and units that struck in
    /// the turn.
    WoundedRanged,
    WoundedStruck,
    /// Wounded units exposed at the start of the side's turn that could
    /// reach a tile out of the enemy's reach (no enemy military unit within 2
    /// tiles, out of an enemy city's strike, or an own city); of them those
    /// still wounded and exposed at its end; and those lost before the
    /// next turn.
    WoundedCouldLeave,
    WoundedStayedExposed,
    LostCouldLeave,
    /// At the end of the side's turn, its land military units within reach of
    /// an enemy (within 2 tiles of an enemy land military unit or in an enemy
    /// city's strike), out of its cities (classic/docs/ai-siege-tactics.md);
    /// of them those on defensive ground (the rules' tile defence for them
    /// above 0: hills, forest, jungle, a fort); the tiles they could stop on
    /// at the start of the turn, summed, and of those the defensive ones;
    /// those that neither moved nor struck in the turn, and of them the
    /// fortified ones.
    NearFoe,
    NearFoeDefensive,
    NearFoeReach,
    NearFoeReachDefensive,
    NearFoeStood,
    NearFoeStoodFortified,
    /// Land ranged units with an enemy unit or city within their range (with
    /// promotions) of where they began the turn, and of them those that shot
    /// in the turn.
    RangedInRange,
    RangedInRangeShot,
    /// Ranged shots by the side's land units from a tile no enemy city
    /// strikes; of the shots at cities, those from beyond the city's strike.
    ShotsOutsideCityStrike,
    CityShotsOutsideStrike,
    /// The side's land ranged units killed by city strikes, melee strikes
    /// and ranged strikes.
    RangedLostToCity,
    RangedLostToMelee,
    RangedLostToRanged,
    /// The side's land ranged units on the front at the end of its turn
    /// (as `FrontTurns`, out of its cities); of them those with an own land
    /// melee unit nearer to their nearest enemy than they are (screened);
    /// those in an enemy city's strike.
    RangedFront,
    RangedScreened,
    RangedInCityStrike,
    /// Land units whose range exceeds a city's strike range ending their
    /// turn within their range + 2 of an enemy city; of them those out of
    /// every enemy city's strike.
    OutrangerTurns,
    OutrangerSafe,
    /// Of `NearFoe`: on open ground although a defensive tile was in its
    /// reach at the turn's start.
    NearFoeOpenCouldDefend,
    /// Of `RangedInRange`, those that did not shoot, by their AI task: a
    /// guard of a city, a front member, told to heal, anything else.
    RangedIdleGuard,
    RangedIdleMember,
    RangedIdleHealing,
    RangedIdleOther,
    /// Of `NearFoeOpenCouldDefend`, by their AI task (as `RangedIdleGuard`),
    /// and of them those that moved in the turn.
    OpenGuard,
    OpenMember,
    OpenHealing,
    OpenOther,
    OpenMoved,
    /// Of `NearFoe`: fortified at the end of the turn (movers and strikers
    /// included).
    NearFoeFortified,
    /// Of `OpenGuard`: those next to the city they guard.
    OpenGuardNextToCity,
}

impl Tally {
    pub const ALL: [Tally; 81] = [
        Tally::MeleeOnUnits,
        Tally::MeleeOnCities,
        Tally::RangedOnUnits,
        Tally::RangedOnCities,
        Tally::CityStrikes,
        Tally::KillsMelee,
        Tally::KillsRanged,
        Tally::KillsCity,
        Tally::KillsDefending,
        Tally::BadOdds,
        Tally::DeadlyOdds,
        Tally::ExpectedKills,
        Tally::StrikeTurns,
        Tally::Targets,
        Tally::TargetsSurvived,
        Tally::StrikesOnSurvivors,
        Tally::StrikesOnWeakest,
        Tally::Strikers,
        Tally::LostAfterStriking,
        Tally::FrontTurns,
        Tally::WoundedExposed,
        Tally::RangedExposed,
        Tally::IdleWithStrike,
        Tally::IdleFavourable,
        Tally::SiegeTurns,
        Tally::SiegeStrikes,
        Tally::LostWoundedExposed,
        Tally::LostRangedExposed,
        Tally::LostAtFront,
        Tally::LostInCity,
        Tally::LostAway,
        Tally::Shooters,
        Tally::ShotKillsAi,
        Tally::ShotKillsFocus,
        Tally::WoundedGuard,
        Tally::WoundedMember,
        Tally::WoundedHealing,
        Tally::WoundedOther,
        Tally::LostGuard,
        Tally::LostMember,
        Tally::LostHealing,
        Tally::LostOther,
        Tally::HealHeld,
        Tally::HealTileExposed,
        Tally::WoundedStayed,
        Tally::WoundedMovesLeft,
        Tally::WoundedRanged,
        Tally::WoundedStruck,
        Tally::WoundedCouldLeave,
        Tally::WoundedStayedExposed,
        Tally::LostCouldLeave,
        Tally::NearFoe,
        Tally::NearFoeDefensive,
        Tally::NearFoeReach,
        Tally::NearFoeReachDefensive,
        Tally::NearFoeStood,
        Tally::NearFoeStoodFortified,
        Tally::RangedInRange,
        Tally::RangedInRangeShot,
        Tally::ShotsOutsideCityStrike,
        Tally::CityShotsOutsideStrike,
        Tally::RangedLostToCity,
        Tally::RangedLostToMelee,
        Tally::RangedLostToRanged,
        Tally::RangedFront,
        Tally::RangedScreened,
        Tally::RangedInCityStrike,
        Tally::OutrangerTurns,
        Tally::OutrangerSafe,
        Tally::NearFoeOpenCouldDefend,
        Tally::RangedIdleGuard,
        Tally::RangedIdleMember,
        Tally::RangedIdleHealing,
        Tally::RangedIdleOther,
        Tally::OpenGuard,
        Tally::OpenMember,
        Tally::OpenHealing,
        Tally::OpenOther,
        Tally::OpenMoved,
        Tally::NearFoeFortified,
        Tally::OpenGuardNextToCity,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Tally::MeleeOnUnits => "melee strikes on units",
            Tally::MeleeOnCities => "melee strikes on cities",
            Tally::RangedOnUnits => "ranged strikes on units",
            Tally::RangedOnCities => "ranged strikes on cities",
            Tally::CityStrikes => "city strikes on units",
            Tally::KillsMelee => "kills by melee strikes",
            Tally::KillsRanged => "kills by ranged strikes",
            Tally::KillsCity => "kills by city strikes",
            Tally::KillsDefending => "kills in defence (attacker died)",
            Tally::BadOdds => "melee strikes at bad odds (taken > dealt, no kill expected)",
            Tally::DeadlyOdds => "melee strikes whose worst roll kills the attacker",
            Tally::ExpectedKills => "melee strikes expected to kill",
            Tally::StrikeTurns => "turns with a strike on a unit",
            Tally::Targets => "enemy units struck (per turn)",
            Tally::TargetsSurvived => "of them alive at the end of the turn",
            Tally::StrikesOnSurvivors => "strikes on units that survived the turn",
            Tally::StrikesOnWeakest => "strikes on the weakest unit in reach",
            Tally::Strikers => "units that struck (per turn)",
            Tally::LostAfterStriking => "of them lost before the next turn",
            Tally::FrontTurns => "front unit-turns (within 3 of an enemy)",
            Tally::WoundedExposed => "wounded (<50) left exposed",
            Tally::RangedExposed => "ranged next to an enemy melee unit",
            Tally::IdleWithStrike => "idle with a strike from where it stands",
            Tally::IdleFavourable => "idle with a favourable strike",
            Tally::SiegeTurns => "siege unit front turns",
            Tally::SiegeStrikes => "siege unit strikes",
            Tally::LostWoundedExposed => "lost after ending wounded and exposed",
            Tally::LostRangedExposed => "lost after ending ranged next to enemy melee",
            Tally::LostAtFront => "lost after ending elsewhere on the front",
            Tally::LostInCity => "lost after ending in an own city",
            Tally::LostAway => "lost after ending away from the front",
            Tally::Shooters => "ranged units with a shot at the turn's start",
            Tally::ShotKillsAi => "their expected kills as the AI aims",
            Tally::ShotKillsFocus => "their expected kills under a focus rule",
            Tally::WoundedGuard => "wounded exposed: guards of a city",
            Tally::WoundedMember => "wounded exposed: front members",
            Tally::WoundedHealing => "wounded exposed: told to heal",
            Tally::WoundedOther => "wounded exposed: other tasks",
            Tally::LostGuard => "lost: guards of a city",
            Tally::LostMember => "lost: front members",
            Tally::LostHealing => "lost: told to heal",
            Tally::LostOther => "lost: other tasks",
            Tally::HealHeld => "wounded told to heal: at or next to the heal tile",
            Tally::HealTileExposed => "wounded told to heal: the heal tile exposed",
            Tally::WoundedStayed => "wounded exposed: never moved this turn",
            Tally::WoundedMovesLeft => "wounded exposed: with moves left",
            Tally::WoundedRanged => "wounded exposed: ranged units",
            Tally::WoundedStruck => "wounded exposed: struck this turn",
            Tally::WoundedCouldLeave => "wounded exposed at the turn's start that could reach safety",
            Tally::WoundedStayedExposed => "of them still exposed at its end",
            Tally::LostCouldLeave => "of them lost before the next turn",
            Tally::NearFoe => "unit-turns ending within reach of an enemy (2 tiles of a unit, a city's strike)",
            Tally::NearFoeDefensive => "of them on defensive ground",
            Tally::NearFoeReach => "tiles they could stop on at the turn's start",
            Tally::NearFoeReachDefensive => "of those tiles, defensive ground",
            Tally::NearFoeStood => "of them neither moved nor struck",
            Tally::NearFoeStoodFortified => "of those, fortified",
            Tally::RangedInRange => "ranged units with an enemy in range at the turn's start",
            Tally::RangedInRangeShot => "of them shot in the turn",
            Tally::ShotsOutsideCityStrike => "ranged shots from a tile no enemy city strikes",
            Tally::CityShotsOutsideStrike => "ranged shots at a city from beyond its strike",
            Tally::RangedLostToCity => "ranged units lost to city strikes",
            Tally::RangedLostToMelee => "ranged units lost to melee strikes",
            Tally::RangedLostToRanged => "ranged units lost to ranged strikes",
            Tally::RangedFront => "ranged front unit-turns",
            Tally::RangedScreened => "of them screened (an own melee unit nearer the nearest enemy)",
            Tally::RangedInCityStrike => "of them in an enemy city's strike",
            Tally::OutrangerTurns => "outranger unit-turns near an enemy city",
            Tally::OutrangerSafe => "of them out of every enemy city's strike",
            Tally::NearFoeOpenCouldDefend => "within reach on open ground with defensive ground in reach",
            Tally::RangedIdleGuard => "in range, no shot: guards of a city",
            Tally::RangedIdleMember => "in range, no shot: front members",
            Tally::RangedIdleHealing => "in range, no shot: told to heal",
            Tally::RangedIdleOther => "in range, no shot: other tasks",
            Tally::OpenGuard => "open ground, defence in reach: guards of a city",
            Tally::OpenMember => "open ground, defence in reach: front members",
            Tally::OpenHealing => "open ground, defence in reach: told to heal",
            Tally::OpenOther => "open ground, defence in reach: other tasks",
            Tally::OpenMoved => "open ground, defence in reach: moved this turn",
            Tally::NearFoeFortified => "within reach and fortified (any unit)",
            Tally::OpenGuardNextToCity => "open ground, defence in reach: guards next to their city",
        }
    }
}

/// How a side fought in a battle: what its strikes were, their odds and
/// focus, and where its units ended their turns (`Tally`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FightAudit {
    pub counts: [u32; Tally::ALL.len()],
}

impl Default for FightAudit {
    fn default() -> Self {
        Self { counts: [0; Tally::ALL.len()] }
    }
}

impl FightAudit {
    pub fn get(&self, tally: Tally) -> u32 {
        self.counts[tally as usize]
    }

    fn add(&mut self, tally: Tally, n: u32) {
        self.counts[tally as usize] += n;
    }

    pub fn plus(self, other: FightAudit) -> FightAudit {
        FightAudit { counts: std::array::from_fn(|i| self.counts[i] + other.counts[i]) }
    }
}

/// Rounds of one window of `BattleStats::assault`.
pub const TASK_WINDOW: u32 = 5;
/// Windows of `BattleStats::assault`.
pub const TASK_WINDOWS: usize = 6;

impl BattleStats {
    /// The share of military unit-turns that took damage in the enemies'
    /// turn, both sides together.
    pub fn damaged_share(&self) -> f64 {
        let turns = self.unit_turns[0] + self.unit_turns[1];
        f64::from(self.damaged[0] + self.damaged[1]) / f64::from(turns.max(1))
    }
}

impl Battle {
    /// A driver for every player with the AI memory of the rounds before the
    /// war, deciding with `mind`.
    pub fn driver<P: AiMind<Classic>>(&self, rules: &CompiledRuleset, mind: P) -> Result<AiDriver<Classic, P>, String> {
        let mut driver = AiDriver::with_mind(self.settings.clone(), Vec::new(), mind);
        let tiles = self.game.view().auth.map().tile_count();
        driver.load(&self.memory, tiles, rules).map_err(|e| format!("the AI memory: {e:?}"))?;
        Ok(driver)
    }

    fn side_of(&self, player: PlayerId) -> Option<usize> {
        self.sides.iter().position(|s| *s == player)
    }

    /// Plays with `driver` on to `rounds` rounds after the war began (or to
    /// the end of the game) and measures the fighting of this call; between
    /// rounds a peace is broken (`hold_war`) and the losses come back
    /// (`reinforce_every`); `after_round` runs after every round (the
    /// recorder writes the round's outcomes there).
    pub fn play<P: AiMind<Classic>>(
        &mut self,
        rules: &CompiledRuleset,
        driver: &mut AiDriver<Classic, P>,
        rounds: u32,
        mut after_round: impl FnMut(&mut AiDriver<Classic, P>),
    ) -> Result<BattleStats, String> {
        let last = self.round + rounds;
        let mut stats = BattleStats::default();
        let mut watch = Watch::new(self);
        let mut audit = Audit::default();
        driver.configure_game(&mut self.game);
        while !self.game.is_over() && self.game.view().auth.turn().round < last {
            match driver.advance(&self.game, rules) {
                Advance::Worked => {}
                Advance::RoundDone => {
                    if !self.game.view().auth.diplomacy().at_war(self.sides[0], self.sides[1]) {
                        stats.peace_rounds += 1;
                        if self.hold_war {
                            self.renew_war(rules, driver);
                        }
                    }
                    let elapsed = self.game.view().auth.turn().round - self.round;
                    self.count_tasks(driver, elapsed, &mut stats);
                    if self.reinforce_every > 0 && elapsed % self.reinforce_every == 0 && self.game.view().auth.turn().round < last {
                        for side in 0..2 {
                            stats.reinforcements[side] += self.reinforce(rules, driver, side);
                        }
                    }
                    after_round(driver)
                }
                Advance::Waiting => return Err(format!("round {} waits for a human", self.game.view().auth.turn().round)),
                advance @ (Advance::Command(_) | Advance::EndTurn(_)) => {
                    let (command, ends) = match advance {
                        Advance::Command(command) => (command, false),
                        Advance::EndTurn(command) => (command, true),
                        _ => unreachable!(),
                    };
                    let actor = self.game.view().auth.active_player();
                    let attack = self.side_of(actor).and_then(|side| self.attack_target(&command).map(|target| (side, target)));
                    let city_target = self.city_target(&command);
                    let strike = self.side_of(actor).and_then(|side| self.watch_strike(rules, side, &command));
                    if let (true, Some(side)) = (ends, self.side_of(actor)) {
                        let tasks = driver.states().get(self.sides[side]);
                        let task = |id: EntityId| tasks.and_then(|s| task_class(s, id));
                        audit.end_of_turn(self, rules, side, &task, &mut stats);
                    }
                    let result = self.game.execute(rules, command);
                    if let (Some(strike), true) = (strike, result.is_ok()) {
                        audit.struck(self, strike, &mut stats);
                    }
                    if let (Some((side, target)), true) = (attack, result.is_ok()) {
                        stats.attacks[side] += 1;
                        if target == Some(self.sides[1 - side]) {
                            stats.attacks_on_enemy[side] += 1;
                        }
                        if city_target == Some(self.sides[1 - side]) {
                            stats.city_attacks[side] += 1;
                        }
                    }
                    if let StepOutcome::PlayerDone(done) = driver.executed(result) {
                        watch.turn_passed(self, rules, done, &mut stats);
                        if let Some(side) = self.side_of(self.game.view().auth.active_player()) {
                            audit.turn_starts(self, rules, side, &mut stats);
                        }
                    }
                }
            }
        }
        stats.rounds = self.game.view().auth.turn().round - self.round;
        stats.hash = kernel::replay::hash::authoritative_hash(&self.game);
        Ok(stats)
    }

    /// The war again after a peace, each side aimed at the enemy city
    /// nearest its own.
    fn renew_war<P: AiMind<Classic>>(&mut self, rules: &CompiledRuleset, driver: &mut AiDriver<Classic, P>) {
        let [a, b] = self.sides;
        self.game.debug_force_war(rules, a, b);
        let round = self.game.view().auth.turn().round;
        for (side, enemy) in [(a, b), (b, a)] {
            if let Some(target) = front_city(&self.game, side, enemy) {
                aim(driver, &self.game, side, target, round);
            }
        }
    }

    /// The army's lost units again, of the kinds they were, on the free
    /// land of the side nearest the enemy city now closest to its cities;
    /// the side is aimed at that city again. The number of units sent.
    fn reinforce<P: AiMind<Classic>>(&mut self, rules: &CompiledRuleset, driver: &mut AiDriver<Classic, P>, side: usize) -> u32 {
        let (own, enemy) = (self.sides[side], self.sides[1 - side]);
        if !self.game.view().auth.players().is_alive(own) {
            return 0;
        }
        let Some(target) = front_city(&self.game, own, enemy) else {
            return 0;
        };
        let auth = self.game.view().auth;
        let lost: Vec<usize> = (0..self.armies[side].units.len())
            .filter(|&i| auth.units().get(self.armies[side].units[i]).is_none_or(|u| u.core.owner != own))
            .collect();
        let tiles = front_tiles(rules, &self.game, own, target, lost.len());
        let mut sent = 0;
        for (&i, &(_, tile)) in lost.iter().zip(&tiles) {
            let kind = self.armies[side].kinds[i];
            self.armies[side].units[i] = self.game.debug_spawn_unit(rules, own, tile, kind);
            sent += 1;
        }
        let round = self.game.view().auth.turn().round;
        aim(driver, &self.game, own, target, round);
        sent
    }

    /// The owner of the city an attack command strikes: a melee attack on a
    /// city, a ranged strike on a city's tile; None for anything else.
    fn city_target(&self, command: &Command) -> Option<PlayerId> {
        let auth = self.game.view().auth;
        let city = match command {
            Command::AttackCity { city, .. } => Some(*city),
            Command::RangedAttack { target, .. } => auth.indexes().city_on_tile(*target),
            _ => None,
        };
        city.and_then(|c| auth.cities().get(c)).map(|c| c.owner)
    }

    /// The army's units by their AI task at the end of a round, `elapsed`
    /// rounds after the war began (`BattleStats::assault`, `garrison_rounds`).
    fn count_tasks<P: AiMind<Classic>>(&self, driver: &AiDriver<Classic, P>, elapsed: u32, stats: &mut BattleStats) {
        let auth = self.game.view().auth;
        let window = ((elapsed.saturating_sub(1) / TASK_WINDOW) as usize).min(TASK_WINDOWS - 1);
        for side in 0..2 {
            let own = self.sides[side];
            let Some(state) = driver.states().get(own) else { continue };
            for handle in &self.armies[side].units {
                let Some(unit) = auth.units().get(*handle).filter(|u| u.core.owner == own) else { continue };
                stats.army_rounds[side] += 1;
                match state.task_of(unit.core.id).map(|t| t.task) {
                    Some(UnitTask::Assault { .. }) => stats.assault[side][window] += 1,
                    Some(UnitTask::Operation { target }) if state.operation(target).is_some_and(|o| o.phase == OperationPhase::Marching) => stats.assault[side][window] += 1,
                    Some(UnitTask::Defend { .. }) => stats.garrison_rounds[side] += 1,
                    _ => {}
                }
            }
        }
    }

    /// Who an attack command strikes (Some(None): nobody's unit or city is
    /// there); None for other commands.
    fn attack_target(&self, command: &Command) -> Option<Option<PlayerId>> {
        let auth = self.game.view().auth;
        let on_tile = |tile: TileId| {
            let city = auth.indexes().city_on_tile(tile).and_then(|c| auth.cities().get(c)).map(|c| c.owner);
            city.or_else(|| auth.indexes().units_on(tile).first().and_then(|u| auth.units().get(*u)).map(|u| u.core.owner))
        };
        match command {
            Command::AttackUnit { defender, .. } => Some(auth.units().get(*defender).map(|u| u.core.owner)),
            Command::AttackCity { city, .. } => Some(auth.cities().get(*city).map(|c| c.owner)),
            Command::RangedAttack { target, .. } | Command::CityStrike { target, .. } => Some(on_tile(*target)),
            _ => None,
        }
    }

    /// A strike of `side` on the other side before it is made: the striker,
    /// the military units of the two sides at both ends with their health,
    /// the striker's own preview. None for any other command.
    fn watch_strike(&self, rules: &CompiledRuleset, side: usize, command: &Command) -> Option<StrikeWatch> {
        let auth = self.game.view().auth;
        let (attacker, tile, kind) = match command {
            Command::AttackUnit { attacker, defender } => (Some(*attacker), auth.units().get(*defender)?.core.tile, StrikeKind::Melee),
            Command::AttackCity { attacker, city } => (Some(*attacker), auth.cities().get(*city)?.tile, StrikeKind::Melee),
            Command::RangedAttack { unit, target } => (Some(*unit), *target, StrikeKind::Ranged),
            Command::CityStrike { target, .. } => (None, *target, StrikeKind::City),
            _ => return None,
        };
        let enemy = self.sides[1 - side];
        let city = auth.indexes().city_on_tile(tile).and_then(|c| auth.cities().get(c));
        let military = |h: &UnitHandle| auth.units().get(*h).filter(|u| role_of(rules, u.core.kind) == AiRole::Military).map(|u| (*h, *u.core));
        let defenders: Vec<(UnitHandle, kernel::world::unit::UnitCore)> = auth.indexes().units_on(tile).iter().filter_map(military).filter(|(_, u)| u.owner == enemy).collect();
        let on_city = city.is_some();
        if city.is_some_and(|c| c.owner != enemy) || (!on_city && defenders.is_empty()) {
            return None;
        }
        let striker = attacker.and_then(|h| military(&h));
        // A ranged unit's charge is a shot (the rules strike from its range).
        let kind = match striker {
            Some((_, u)) if kind == StrikeKind::Melee && classic_rules::ai::combat::ranged_range(rules, u.kind).is_some() => StrikeKind::Ranged,
            _ => kind,
        };
        let mut units: Vec<(UnitHandle, EntityId, usize, u16)> = defenders.iter().map(|(h, u)| (*h, u.id, 1 - side, u.hp)).collect();
        units.extend(striker.map(|(h, u)| (h, u.id, side, u.hp)));
        let view = self.game.player_view(rules, self.sides[side]);
        let preview = attacker.filter(|_| kind == StrikeKind::Melee).and_then(|h| view.attack_preview(h, tile));
        let target = (!on_city).then(|| defenders.first().map(|(h, u)| (*h, u.id, u.hp))).flatten();
        // The weakest enemy unit the striker could strike from where it stands.
        let weakest = striker.filter(|_| target.is_some()).and_then(|(h, u)| {
            let reach = classic_rules::ai::combat::ranged_range(rules, u.kind).filter(|_| kind == StrikeKind::Ranged).unwrap_or(1);
            let dims = auth.map().dimensions;
            auth.units()
                .iter_cores()
                .filter(|e| e.owner == enemy && role_of(rules, e.kind) == AiRole::Military)
                .filter(|e| auth.indexes().city_on_tile(e.tile).is_none())
                .filter(|e| dims.distance(u.tile, e.tile).is_some_and(|d| d <= reach))
                .filter(|e| view.attack_preview(h, e.tile).is_some())
                .map(|e| e.hp)
                .min()
        });
        let siege = striker.is_some_and(|(_, u)| kind_arm(rules, u.kind) == Some(Arm::Siege));
        let dims = auth.map().dimensions;
        let enemy_cities: Vec<TileId> = auth.indexes().cities_of(enemy).iter().filter_map(|c| auth.cities().get(*c)).map(|c| c.tile).collect();
        let from_in_strike = striker
            .filter(|_| kind == StrikeKind::Ranged)
            .map(|(_, u)| enemy_cities.iter().any(|c| dims.distance(u.tile, *c).is_some_and(|d| d <= classic_rules::combat::city::CITY_ATTACK_RANGE)));
        let land_ranged = |u: &kernel::world::unit::UnitCore| land_ranged_range(rules, u).is_some();
        let ranged = defenders.iter().map(|(_, u)| u).chain(striker.as_ref().map(|(_, u)| u)).filter(|u| land_ranged(u)).map(|u| u.id).collect();
        Some(StrikeWatch { side, kind, on_city, attacker: striker.map(|(h, u)| (h, u.id)), units, preview, target, weakest, siege, from_in_strike, ranged })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StrikeKind {
    Melee,
    Ranged,
    City,
}

/// A strike between the sides as it stood before it was made.
struct StrikeWatch {
    side: usize,
    kind: StrikeKind,
    on_city: bool,
    /// The striking unit (None: a city).
    attacker: Option<(UnitHandle, EntityId)>,
    /// The military units of both sides at its two ends: handle, id, side,
    /// health.
    units: Vec<(UnitHandle, EntityId, usize, u16)>,
    /// The striker's own preview of a melee strike.
    preview: Option<CombatPreview>,
    /// The enemy unit struck (the first on its tile) and its health.
    target: Option<(UnitHandle, EntityId, u16)>,
    /// The health of the weakest enemy unit the striker could strike.
    weakest: Option<u16>,
    siege: bool,
    /// A ranged shot by a unit: whether an enemy city strikes the tile it
    /// shoots from.
    from_in_strike: Option<bool>,
    /// The ids of the land ranged units among `units`.
    ranged: Vec<EntityId>,
}

/// The fight audit's memory within a battle (`FightAudit`).
#[derive(Default)]
struct Audit {
    /// Per side: the enemy units struck this turn, with the strikes they took.
    targets: [BTreeMap<EntityId, (UnitHandle, u32)>; 2],
    /// Per side: its units that struck this turn.
    strikers: [BTreeMap<EntityId, UnitHandle>; 2],
    /// Per side: the strikers alive at the end of its last turn.
    struck_alive: [Vec<(EntityId, UnitHandle)>; 2],
    /// Per side: its land military units at the end of its last turn, with
    /// where they stood (`Tally::LostWoundedExposed` and the next).
    ended: [Vec<(EntityId, UnitHandle, Tally, Tally)>; 2],
    /// Per side: where its units stood at the start of its turn.
    began: [BTreeMap<EntityId, TileId>; 2],
    /// Per side: its wounded units exposed at the start of its turn that
    /// could reach safety (`Tally::WoundedCouldLeave`), and those of them
    /// still exposed at its end.
    could_leave: [Vec<EntityId>; 2],
    stayed_exposed: [Vec<(EntityId, UnitHandle)>; 2],
    /// Per side: the tiles each land military unit near an enemy could stop
    /// on at the start of its turn, and of them the defensive ones
    /// (`Tally::NearFoeReach`).
    reach: [BTreeMap<EntityId, (u32, u32)>; 2],
    /// Per side: its land ranged units with an enemy in range at the start of
    /// its turn (`Tally::RangedInRange`).
    in_range: [Vec<EntityId>; 2],
}

/// The range of a land military unit that shoots, with its promotions
/// (`siege_tactics::unit_range`); None for melee and other domains.
fn land_ranged_range(rules: &CompiledRuleset, unit: &kernel::world::unit::UnitCore) -> Option<i32> {
    let land = rules.units.get(unit.kind.0 as usize).is_some_and(|d| d.domain == Domain::Land);
    let military = role_of(rules, unit.kind) == AiRole::Military;
    (land && military).then(|| classic_rules::ai::siege_tactics::unit_range(rules, unit.kind, &unit.promotions)).flatten()
}

/// What a unit's AI task makes it on the battlefield (`Tally::WoundedGuard`
/// and the next); None: anything else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TaskClass {
    /// Guards the city on the tile.
    Guard(TileId),
    Member,
    /// Told to heal at the tile.
    Healing(TileId),
}

fn task_class(state: &kernel::ai::state::AiState, id: EntityId) -> Option<TaskClass> {
    match state.task_of(id).map(|t| t.task)? {
        UnitTask::Defend { city } => Some(TaskClass::Guard(city)),
        UnitTask::Assault { .. } => Some(TaskClass::Member),
        UnitTask::Operation { target } if state.operation(target).is_some_and(|o| o.phase == OperationPhase::Marching) => Some(TaskClass::Member),
        UnitTask::Heal { at } => Some(TaskClass::Healing(at)),
        _ => None,
    }
}

fn lost_by(class: Option<TaskClass>) -> Tally {
    match class {
        Some(TaskClass::Guard(_)) => Tally::LostGuard,
        Some(TaskClass::Member) => Tally::LostMember,
        Some(TaskClass::Healing(_)) => Tally::LostHealing,
        None => Tally::LostOther,
    }
}

fn alive(game: &GameState, handle: UnitHandle, id: EntityId) -> Option<kernel::world::unit::UnitCore> {
    game.view().auth.units().get(handle).map(|u| *u.core).filter(|u| u.id == id)
}

impl Audit {
    /// A strike was made: who it killed and hurt, its odds and its focus.
    fn struck(&mut self, battle: &Battle, strike: StrikeWatch, stats: &mut BattleStats) {
        let side = strike.side;
        let audit = &mut stats.audit;
        let striker = strike.attacker.map(|(_, id)| id);
        for &(handle, id, owner, hp) in &strike.units {
            let now = alive(&battle.game, handle, id).filter(|u| u.owner == battle.sides[owner]);
            let lost = hp.saturating_sub(now.map_or(0, |u| u.hp));
            stats.dealt[1 - owner] += u32::from(lost);
            if now.is_some() {
                continue;
            }
            stats.killed[1 - owner] += 1;
            if strike.ranged.contains(&id) && Some(id) != striker {
                let lost = match strike.kind {
                    StrikeKind::City => Tally::RangedLostToCity,
                    StrikeKind::Melee => Tally::RangedLostToMelee,
                    StrikeKind::Ranged => Tally::RangedLostToRanged,
                };
                audit[owner].add(lost, 1);
            }
            let tally = match strike.kind {
                _ if Some(id) == striker => Tally::KillsDefending,
                StrikeKind::Melee => Tally::KillsMelee,
                StrikeKind::Ranged => Tally::KillsRanged,
                StrikeKind::City => Tally::KillsCity,
            };
            audit[1 - owner].add(tally, 1);
        }
        let mine = &mut audit[side];
        mine.add(
            match (strike.kind, strike.on_city) {
                (StrikeKind::Melee, false) => Tally::MeleeOnUnits,
                (StrikeKind::Melee, true) => Tally::MeleeOnCities,
                (StrikeKind::Ranged, false) => Tally::RangedOnUnits,
                (StrikeKind::Ranged, true) => Tally::RangedOnCities,
                (StrikeKind::City, _) => Tally::CityStrikes,
            },
            1,
        );
        if let Some(p) = strike.preview {
            mine.add(Tally::BadOdds, u32::from(p.to_attacker.1 > p.to_defender.1 && p.to_defender.1 < p.defender_hp));
            mine.add(Tally::DeadlyOdds, u32::from(p.to_attacker.2 >= p.attacker_hp));
            mine.add(Tally::ExpectedKills, u32::from(!strike.on_city && p.to_defender.1 >= p.defender_hp));
        }
        mine.add(Tally::SiegeStrikes, u32::from(strike.siege));
        if let Some(in_strike) = strike.from_in_strike {
            mine.add(Tally::ShotsOutsideCityStrike, u32::from(!in_strike));
            mine.add(Tally::CityShotsOutsideStrike, u32::from(!in_strike && strike.on_city));
        }
        if let (Some((handle, id, hp)), Some(weakest)) = (strike.target, strike.weakest) {
            mine.add(Tally::StrikesOnWeakest, u32::from(hp <= weakest));
            self.targets[side].entry(id).or_insert((handle, 0)).1 += 1;
        }
        if let Some((handle, id)) = strike.attacker {
            self.strikers[side].insert(id, handle);
        }
    }

    /// The side's turn ends: the focus of its strikes, and where its land
    /// military units stand.
    fn end_of_turn(&mut self, battle: &Battle, rules: &CompiledRuleset, side: usize, task: &dyn Fn(EntityId) -> Option<TaskClass>, stats: &mut BattleStats) {
        let game = &battle.game;
        let audit = &mut stats.audit[side];
        let targets = std::mem::take(&mut self.targets[side]);
        audit.add(Tally::StrikeTurns, u32::from(!targets.is_empty()));
        for (id, (handle, strikes)) in targets {
            audit.add(Tally::Targets, 1);
            if alive(game, handle, id).is_some() {
                audit.add(Tally::TargetsSurvived, 1);
                audit.add(Tally::StrikesOnSurvivors, strikes);
            }
        }
        let strikers = std::mem::take(&mut self.strikers[side]);
        audit.add(Tally::Strikers, strikers.len() as u32);
        self.ended[side].clear();
        self.struck_alive[side] = strikers.iter().filter(|(id, h)| alive(game, **h, **id).is_some()).map(|(id, h)| (*id, *h)).collect();
        let auth = game.view().auth;
        let (own, enemy) = (battle.sides[side], battle.sides[1 - side]);
        let dims = auth.map().dimensions;
        let land_military = |kind: UnitKindId| role_of(rules, kind) == AiRole::Military && rules.units.get(kind.0 as usize).is_some_and(|d| d.domain == Domain::Land);
        let ranged = |kind: UnitKindId| classic_rules::ai::combat::ranged_range(rules, kind);
        let foes: Vec<(TileId, bool)> = auth
            .units()
            .iter_cores()
            .filter(|u| u.owner == enemy && land_military(u.kind))
            .map(|u| (u.tile, ranged(u.kind).is_none()))
            .collect();
        let cities: Vec<TileId> = auth.indexes().cities_of(enemy).iter().filter_map(|c| auth.cities().get(*c)).map(|c| c.tile).collect();
        let view = game.player_view(rules, own);
        let near = |a: TileId, b: TileId, d: i32| dims.distance(a, b).is_some_and(|x| x <= d);
        for (handle, unit) in auth.units().iter_handles().filter(|(_, u)| u.owner == own && land_military(u.kind)) {
            let foe_within = |d: i32| foes.iter().any(|(t, _)| near(unit.tile, *t, d));
            let city_within = |d: i32| cities.iter().any(|t| near(unit.tile, *t, d));
            let at_home = auth.indexes().city_on_tile(unit.tile).and_then(|c| auth.cities().get(c)).is_some_and(|c| c.owner == own);
            if !foe_within(3) && !city_within(3) {
                self.ended[side].push((unit.id, handle, if at_home { Tally::LostInCity } else { Tally::LostAway }, lost_by(task(unit.id))));
                continue;
            }
            audit.add(Tally::FrontTurns, 1);
            let wounded = unit.hp < 50 && (foe_within(2) || city_within(classic_rules::combat::city::CITY_ATTACK_RANGE)) && !at_home;
            audit.add(Tally::WoundedExposed, u32::from(wounded));
            let range = ranged(unit.kind);
            let charged = foes.iter().any(|(t, melee)| *melee && near(unit.tile, *t, 1));
            let exposed = range.is_some() && charged && !at_home;
            audit.add(Tally::RangedExposed, u32::from(exposed));
            let place = match () {
                _ if wounded => Tally::LostWoundedExposed,
                _ if exposed => Tally::LostRangedExposed,
                _ if at_home => Tally::LostInCity,
                _ => Tally::LostAtFront,
            };
            self.ended[side].push((unit.id, handle, place, lost_by(task(unit.id))));
            if wounded && self.could_leave[side].contains(&unit.id) {
                audit.add(Tally::WoundedStayedExposed, 1);
                self.stayed_exposed[side].push((unit.id, handle));
            }
            if wounded {
                audit.add(Tally::WoundedStayed, u32::from(self.began[side].get(&unit.id) == Some(&unit.tile)));
                audit.add(Tally::WoundedMovesLeft, u32::from(unit.moves_left > 0));
                audit.add(Tally::WoundedRanged, u32::from(ranged(unit.kind).is_some()));
                audit.add(Tally::WoundedStruck, u32::from(strikers.contains_key(&unit.id)));
            }
            if let (true, Some(TaskClass::Healing(at))) = (wounded, task(unit.id)) {
                audit.add(Tally::HealHeld, u32::from(near(unit.tile, at, 1)));
                let exposed = foes.iter().any(|(t, _)| near(at, *t, 2)) || cities.iter().any(|t| near(at, *t, classic_rules::combat::city::CITY_ATTACK_RANGE));
                audit.add(Tally::HealTileExposed, u32::from(exposed));
            }
            if wounded {
                audit.add(
                    match task(unit.id) {
                        Some(TaskClass::Guard(_)) => Tally::WoundedGuard,
                        Some(TaskClass::Member) => Tally::WoundedMember,
                        Some(TaskClass::Healing(_)) => Tally::WoundedHealing,
                        None => Tally::WoundedOther,
                    },
                    1,
                );
            }
            audit.add(Tally::SiegeTurns, u32::from(kind_arm(rules, unit.kind) == Some(Arm::Siege)));
            let struck = strikers.contains_key(&unit.id);
            let in_strike = city_within(classic_rules::combat::city::CITY_ATTACK_RANGE);
            if (foe_within(2) || in_strike) && !at_home {
                audit.add(Tally::NearFoe, 1);
                let on_defence = classic_rules::combat::terrain::tile_defense_percent(auth, rules, unit.tile, own) > 0;
                audit.add(Tally::NearFoeDefensive, u32::from(on_defence));
                let (tiles, defensive) = self.reach[side].get(&unit.id).copied().unwrap_or((0, 0));
                audit.add(Tally::NearFoeOpenCouldDefend, u32::from(!on_defence && defensive > 0));
                audit.add(Tally::NearFoeFortified, u32::from(matches!(unit.stance, kernel::world::unit::Stance::Fortified { .. })));
                if !on_defence && defensive > 0 {
                    let open = match task(unit.id) {
                        Some(TaskClass::Guard(city)) => {
                            audit.add(Tally::OpenGuardNextToCity, u32::from(near(unit.tile, city, 1)));
                            Tally::OpenGuard
                        }
                        Some(TaskClass::Member) => Tally::OpenMember,
                        Some(TaskClass::Healing(_)) => Tally::OpenHealing,
                        None => Tally::OpenOther,
                    };
                    audit.add(open, 1);
                    audit.add(Tally::OpenMoved, u32::from(self.began[side].get(&unit.id) != Some(&unit.tile)));
                }
                audit.add(Tally::NearFoeReach, tiles);
                audit.add(Tally::NearFoeReachDefensive, defensive);
                if self.began[side].get(&unit.id) == Some(&unit.tile) && !struck {
                    audit.add(Tally::NearFoeStood, 1);
                    audit.add(Tally::NearFoeStoodFortified, u32::from(matches!(unit.stance, kernel::world::unit::Stance::Fortified { .. })));
                }
            }
            let reach_with_promotions = land_ranged_range(rules, unit);
            if self.in_range[side].contains(&unit.id) {
                audit.add(Tally::RangedInRange, 1);
                audit.add(Tally::RangedInRangeShot, u32::from(struck));
                if !struck {
                    let idle = match task(unit.id) {
                        Some(TaskClass::Guard(_)) => Tally::RangedIdleGuard,
                        Some(TaskClass::Member) => Tally::RangedIdleMember,
                        Some(TaskClass::Healing(_)) => Tally::RangedIdleHealing,
                        None => Tally::RangedIdleOther,
                    };
                    audit.add(idle, 1);
                }
            }
            if reach_with_promotions.is_some() && !at_home {
                audit.add(Tally::RangedFront, 1);
                audit.add(Tally::RangedInCityStrike, u32::from(in_strike));
                let nearest = foes.iter().map(|(t, _)| *t).chain(cities.iter().copied()).min_by_key(|t| (dims.distance(unit.tile, *t).unwrap_or(i32::MAX), *t));
                if let Some(enemy_tile) = nearest {
                    let mine = dims.distance(unit.tile, enemy_tile).unwrap_or(i32::MAX);
                    let screened = auth
                        .units()
                        .iter_cores()
                        .filter(|u| u.owner == own && land_military(u.kind) && land_ranged_range(rules, u).is_none())
                        .any(|u| dims.distance(u.tile, enemy_tile).is_some_and(|d| d < mine));
                    audit.add(Tally::RangedScreened, u32::from(screened));
                }
            }
            if let Some(range) = reach_with_promotions.filter(|r| *r > classic_rules::combat::city::CITY_ATTACK_RANGE) {
                if city_within(range + 2) {
                    audit.add(Tally::OutrangerTurns, 1);
                    audit.add(Tally::OutrangerSafe, u32::from(!in_strike));
                }
            }
            if struck {
                continue;
            }
            let reach = range.unwrap_or(1);
            let mut previews = foes
                .iter()
                .map(|(t, _)| *t)
                .chain(cities.iter().copied())
                .filter(|t| near(unit.tile, *t, reach))
                .filter_map(|t| view.attack_preview(handle, t));
            let mut any = false;
            let favourable = previews.any(|p| {
                any = true;
                p.to_defender.1 > 0 && (range.is_some() || (p.to_defender.1 >= p.to_attacker.1 && p.to_attacker.2 < unit.hp))
            });
            audit.add(Tally::IdleWithStrike, u32::from(any));
            audit.add(Tally::IdleFavourable, u32::from(favourable));
        }
    }

    /// The side's turn begins: its strikers of the last turn that are gone.
    fn turn_starts(&mut self, battle: &Battle, rules: &CompiledRuleset, side: usize, stats: &mut BattleStats) {
        let lost = self.stayed_exposed[side].drain(..).filter(|(id, h)| alive(&battle.game, *h, *id).is_none()).count();
        stats.audit[side].add(Tally::LostCouldLeave, lost as u32);
        self.could_leave[side] = could_leave(battle, rules, side);
        stats.audit[side].add(Tally::WoundedCouldLeave, self.could_leave[side].len() as u32);
        shots_compared(battle, rules, side, &mut stats.audit[side]);
        let own = battle.sides[side];
        self.began[side] = battle.game.view().auth.units().iter_cores().filter(|u| u.owner == own).map(|u| (u.id, u.tile)).collect();
        self.reach_and_range(battle, rules, side);
        let gone = self.struck_alive[side].drain(..).filter(|(id, h)| alive(&battle.game, *h, *id).is_none()).count();
        stats.audit[side].add(Tally::LostAfterStriking, gone as u32);
        for (id, handle, place, by_task) in self.ended[side].drain(..) {
            if alive(&battle.game, handle, id).is_none() {
                stats.audit[side].add(place, 1);
                stats.audit[side].add(by_task, 1);
            }
        }
    }
}

impl Audit {
    /// At the start of the side's turn: the tiles each of its land military
    /// units within 4 tiles of an enemy could stop on, and of them the
    /// defensive ones; its land ranged units with an enemy in range.
    fn reach_and_range(&mut self, battle: &Battle, rules: &CompiledRuleset, side: usize) {
        let game = &battle.game;
        let auth = game.view().auth;
        let (own, enemy) = (battle.sides[side], battle.sides[1 - side]);
        let dims = auth.map().dimensions;
        let near = |a: TileId, b: TileId, d: i32| dims.distance(a, b).is_some_and(|x| x <= d);
        let land_military = |kind: UnitKindId| role_of(rules, kind) == AiRole::Military && rules.units.get(kind.0 as usize).is_some_and(|d| d.domain == Domain::Land);
        let foes: Vec<TileId> = auth.units().iter_cores().filter(|u| u.owner == enemy && land_military(u.kind)).map(|u| u.tile).collect();
        let cities: Vec<TileId> = auth.indexes().cities_of(enemy).iter().filter_map(|c| auth.cities().get(*c)).map(|c| c.tile).collect();
        let view = game.player_view(rules, own);
        let map = auth.map();
        let free = |t: TileId| !map.is_water(t) && auth.indexes().units_on(t).iter().filter_map(|u| auth.units().get(*u)).all(|u| u.core.owner == own);
        self.reach[side].clear();
        self.in_range[side].clear();
        for (handle, unit) in auth.units().iter_handles().filter(|(_, u)| u.owner == own && land_military(u.kind)) {
            let close = foes.iter().chain(&cities).any(|t| near(unit.tile, *t, 4));
            if !close {
                continue;
            }
            let stops: Vec<TileId> = view.own_reachable(handle).into_iter().map(|(t, _)| t).filter(|t| *t == unit.tile || free(*t)).collect();
            let defensive = stops.iter().filter(|t| classic_rules::combat::terrain::tile_defense_percent(auth, rules, **t, own) > 0).count();
            self.reach[side].insert(unit.id, (stops.len() as u32, defensive as u32));
            if let Some(range) = land_ranged_range(rules, unit) {
                // A shot the rules would take now: in range, seen, previewed.
                let shot = foes.iter().chain(&cities).any(|t| near(unit.tile, *t, range) && view.attack_preview(handle, *t).is_some());
                if shot && unit.moves_left > 0 {
                    self.in_range[side].push(unit.id);
                }
            }
        }
    }
}

/// The side's wounded land military units exposed at the start of its turn (as
/// `Tally::WoundedExposed` counts them at its end) that can reach this turn a
/// tile out of the enemy's reach: no enemy military unit within 2 tiles and
/// out of an enemy city's strike, or an own city.
fn could_leave(battle: &Battle, rules: &CompiledRuleset, side: usize) -> Vec<EntityId> {
    let game = &battle.game;
    let auth = game.view().auth;
    let (own, enemy) = (battle.sides[side], battle.sides[1 - side]);
    let dims = auth.map().dimensions;
    let near = |a: TileId, b: TileId, d: i32| dims.distance(a, b).is_some_and(|x| x <= d);
    let land_military = |kind: UnitKindId| role_of(rules, kind) == AiRole::Military && rules.units.get(kind.0 as usize).is_some_and(|d| d.domain == Domain::Land);
    let foes: Vec<TileId> = auth.units().iter_cores().filter(|u| u.owner == enemy && land_military(u.kind)).map(|u| u.tile).collect();
    let cities: Vec<TileId> = auth.indexes().cities_of(enemy).iter().filter_map(|c| auth.cities().get(*c)).map(|c| c.tile).collect();
    let home = |t: TileId| auth.indexes().city_on_tile(t).and_then(|c| auth.cities().get(c)).is_some_and(|c| c.owner == own);
    let exposed = |t: TileId| !home(t) && (foes.iter().any(|f| near(t, *f, 2)) || cities.iter().any(|c| near(t, *c, classic_rules::combat::city::CITY_ATTACK_RANGE)));
    let view = game.player_view(rules, own);
    auth.units()
        .iter_handles()
        .filter(|(_, u)| u.owner == own && land_military(u.kind) && u.hp < 50 && u.moves_left > 0 && exposed(u.tile))
        .filter(|(h, _)| view.own_reachable(*h).iter().any(|(t, _)| !exposed(*t)))
        .map(|(_, u)| u.id)
        .collect()
}

/// The side's start-of-turn shots from where its land ranged units stand, aimed
/// as the game's AI aims them and as a focus rule would, on the same position
/// (`Tally::Shooters`, `ShotKillsAi`, `ShotKillsFocus`).
fn shots_compared(battle: &Battle, rules: &CompiledRuleset, side: usize, audit: &mut FightAudit) {
    let game = &battle.game;
    let auth = game.view().auth;
    let (own, enemy) = (battle.sides[side], battle.sides[1 - side]);
    let dims = auth.map().dimensions;
    let land = |kind: UnitKindId| rules.units.get(kind.0 as usize).is_some_and(|d| d.domain == Domain::Land);
    // Enemy units out of cities: tile, health.
    let targets: Vec<(TileId, u16)> = auth
        .units()
        .iter_cores()
        .filter(|u| u.owner == enemy && role_of(rules, u.kind) == AiRole::Military && auth.indexes().city_on_tile(u.tile).is_none())
        .map(|u| (u.tile, u.hp))
        .collect();
    if targets.is_empty() {
        return;
    }
    let view = game.player_view(rules, own);
    // Per shooter: its expected damage on each target it can shoot now.
    let mut shooters: Vec<(UnitHandle, Vec<u16>)> = Vec::new();
    for (handle, unit) in auth.units().iter_handles().filter(|(_, u)| u.owner == own && land(u.kind)) {
        let Some(range) = classic_rules::ai::combat::ranged_range(rules, unit.kind) else { continue };
        let damage: Vec<u16> = targets
            .iter()
            .map(|(t, _)| {
                let near = dims.distance(unit.tile, *t).is_some_and(|d| d <= range);
                near.then(|| view.attack_preview(handle, *t)).flatten().map_or(0, |p| p.to_defender.1)
            })
            .collect();
        if damage.iter().any(|d| *d > 0) {
            shooters.push((handle, damage));
        }
    }
    if shooters.is_empty() {
        return;
    }
    audit.add(Tally::Shooters, shooters.len() as u32);
    let hp: Vec<u16> = targets.iter().map(|(_, hp)| *hp).collect();
    let kills = |left: &[u16]| left.iter().filter(|l| **l == 0).count() as u32;
    // The game's AI: the advisor's plan in its order, every shooter left (not
    // in the plan, or its target gone) re-aimed by the advisor's score.
    let obs = game.ai_observation(rules, own);
    let plan = classic_rules::ai::combat::ranged_orders(&obs, rules);
    let mut left = hp.clone();
    let mut used = vec![false; shooters.len()];
    for command in &plan {
        let Command::RangedAttack { unit, target } = command else { continue };
        let Some(i) = shooters.iter().position(|(h, _)| h == unit) else { continue };
        let Some(t) = targets.iter().position(|(tile, _)| tile == target) else {
            used[i] = true;
            continue;
        };
        if left[t] > 0 {
            left[t] = left[t].saturating_sub(shooters[i].1[t]);
            used[i] = true;
        }
    }
    for (_, damage) in shooters.iter().enumerate().filter(|(i, _)| !used[*i]).map(|(_, s)| s) {
        let score = |t: usize| i64::from(damage[t].min(left[t])) * 100 / i64::from(hp[t].max(1)) + if damage[t] >= left[t] { 100 } else { 0 };
        if let Some(t) = (0..targets.len()).filter(|t| left[*t] > 0 && damage[*t] > 0).max_by_key(|t| (score(*t), std::cmp::Reverse(*t))) {
            left[t] = left[t].saturating_sub(damage[t]);
        }
    }
    audit.add(Tally::ShotKillsAi, kills(&left));
    // The focus rule.
    let mut left = hp;
    let mut free: Vec<usize> = (0..shooters.len()).collect();
    loop {
        // The live target the fewest free shooters kill, the weakest first.
        let best = (0..targets.len())
            .filter(|t| left[*t] > 0)
            .filter_map(|t| {
                let mut by: Vec<usize> = free.iter().copied().filter(|i| shooters[*i].1[t] > 0).collect();
                by.sort_by_key(|i| std::cmp::Reverse(shooters[*i].1[t]));
                let mut sum = 0u32;
                let n = by.iter().position(|i| {
                    sum += u32::from(shooters[*i].1[t]);
                    sum >= u32::from(left[t])
                })?;
                Some((n + 1, left[t], t, by[..=n].to_vec()))
            })
            .min_by_key(|(n, hp, t, _)| (*n, *hp, *t));
        let Some((_, _, t, by)) = best else { break };
        left[t] = 0;
        free.retain(|i| !by.contains(i));
    }
    for i in free {
        if let Some(t) = (0..targets.len()).filter(|t| left[*t] > 0 && shooters[i].1[*t] > 0).min_by_key(|t| (left[*t], *t)) {
            left[t] = left[t].saturating_sub(shooters[i].1[t]);
        }
    }
    audit.add(Tally::ShotKillsFocus, kills(&left));
}

/// The hit points of the sides' military units at the end of each side's
/// last turn, and the owners of their cities.
struct Watch {
    hp: [Option<BTreeMap<EntityId, u16>>; 2],
    cities: BTreeMap<EntityId, PlayerId>,
}

impl Watch {
    fn new(battle: &Battle) -> Self {
        Self { hp: [None, None], cities: city_owners(battle) }
    }

    /// `done` ended its turn and the active player's begins: the first's
    /// units are noted, the second's compared with its last note.
    fn turn_passed(&mut self, battle: &Battle, rules: &CompiledRuleset, done: PlayerId, stats: &mut BattleStats) {
        if let Some(side) = battle.side_of(done) {
            self.hp[side] = Some(military_hp(battle, rules, done));
        }
        let next = battle.game.view().auth.active_player();
        if let Some(side) = battle.side_of(next) {
            if let Some(before) = self.hp[side].take() {
                let now = military_hp(battle, rules, next);
                for (id, hp) in before {
                    stats.unit_turns[side] += 1;
                    match now.get(&id) {
                        Some(&left) if left >= hp => {}
                        Some(&left) => {
                            stats.damaged[side] += 1;
                            stats.hp_lost[side] += u32::from(hp - left);
                        }
                        None => {
                            stats.damaged[side] += 1;
                            stats.lost[side] += 1;
                            stats.hp_lost[side] += u32::from(hp);
                        }
                    }
                }
            }
        }
        let owners = city_owners(battle);
        for (id, owner) in &owners {
            let before = self.cities.get(id).copied();
            if let (Some(side), Some(from)) = (battle.side_of(*owner), before) {
                if from == battle.sides[1 - side] {
                    stats.captured[side] += 1;
                }
            }
        }
        self.cities = owners;
    }
}

fn military_hp(battle: &Battle, rules: &CompiledRuleset, player: PlayerId) -> BTreeMap<EntityId, u16> {
    battle
        .game
        .view()
        .auth
        .units()
        .iter_cores()
        .filter(|u| u.owner == player && role_of(rules, u.kind) == AiRole::Military)
        .map(|u| (u.id, u.hp))
        .collect()
}

fn city_owners(battle: &Battle) -> BTreeMap<EntityId, PlayerId> {
    let auth = battle.game.view().auth;
    auth.cities().iter().filter(|(_, c)| battle.side_of(c.owner).is_some()).map(|(_, c)| (c.id, c.owner)).collect()
}
