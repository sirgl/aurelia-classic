//! Every hand weight of the unit net (the commander and the tactician), each
//! with the reason it has its value (classic/docs/ai-unit-net.md; the
//! Russian pages work/unit-net-design.html, the design, and
//! work/unit-net-weights.html, these tables with the measurements). Nothing
//! here was trained. All values are thousandths (`Milli`: 1000 = 1.0)
//! unless a constant says otherwise.
//!
//! Sections: `fields` (the window and the channels), `conv` (the seven hand
//! kernels, design section 2), `concept` (the nine concept neurons, 4),
//! `motive` (the motives' inner weights, 5), `TABLE` (the motives' output
//! weights by role and phase, 5), `prices` (the prices and risk by posture,
//! 7), `plan` (the target values, the fire allocation, the flanks and the
//! reply check, 5a–5d), `op` (the operation head, 6).

use super::Milli;

/// The battlefield window and the channels (design section 1).
pub mod fields {
    use super::Milli;

    /// Tiles around every unit of a theatre the window covers: a unit's
    /// strike this turn and the next (2 rounds of a 2-move unit) and the
    /// convolutions' radius 2 beyond.
    pub const RADIUS: i32 = 5;
    /// An own military unit within this many tiles of a hostile unit or city
    /// (or of its operation's target) is in a theatre.
    pub const ENGAGE: i32 = 9;
    /// A hostile city this close is contact for any unit (its fire, an assault next turn); farther, only for an operation's member.
    pub const CITY_REACH: i32 = 4;
    /// The tiles the mind's sensor sees around a deciding unit of a theatre
    /// (`ClassicMind::sense_unit`): the whole theatre, so a fresh
    /// observation shows every member and every foe of it.
    pub const SIGHT: i32 = 8;
    /// Strength × health is divided by this for the force channels: a unit
    /// of strength 20 at full health is 0.2 (the design's «/100»).
    pub const FORCE_DIV: i64 = 100;
    /// A tile in the fog near a hostile city or a foe seen holds this share
    /// of a typical enemy (`battlefield::FOG_PRIOR_PERCENT`, 12 %).
    pub const FOG_SHARE: Milli = 120;
    /// The fog prior reaches this many tiles from a hostile city or foe.
    pub const FOG_REACH: i32 = 3;
    /// Damage in hit points is divided by this for the danger, support and
    /// focus channels (the design's «/100»: a unit's whole health is 1.0).
    pub const HP_DIV: i64 = 100;
    /// Healing per turn is divided by this (the design's «/20 hp»).
    pub const HEAL_DIV: i64 = 20;
    /// The value of a pillage on a hostile improvement (the design's «/10»):
    /// 1.0, the improvement's cost to its owner and our unit's heal of 25.
    pub const PILLAGE: Milli = 1000;
    /// The 2-turn reach is the 1-turn reach widened by the unit's moves less
    /// one tile (a road or rough ground costs some of the second turn).
    pub const SECOND_TURN_SLACK: i32 = 1;
    /// A city's full health when the player does not know it (the rules'
    /// base, `CITY_BASE_HIT_POINTS`); ch11 is health / this.
    pub const CITY_FULL_HP: i64 = 200;
    /// The movement cost channel: a tile costing 3 moves is 1.0.
    pub const COST_DIV: i64 = 3;
    /// The defence channel: a bonus of 50 % is 1.0.
    pub const DEFENCE_DIV: i64 = 50;
    /// Unused here; the count of the channels.
    pub const CHANNELS: usize = 24;
    pub const MAPS: usize = 14;
    /// Kept for the page: the design's 24 channels by name.
    pub const CHANNEL_NAMES: [&str; CHANNELS] = [
        "passable", "move_cost", "defence", "blocks_sight", "river", "territory", "ours", "ours_ranged", "theirs", "theirs_ranged", "hidden", "city", "our_reach1", "our_reach2",
        "their_reach1", "their_reach2", "enemy_zoc", "danger", "support", "focus", "op_target", "target_turns", "heal", "pillage",
    ];
    pub const MAP_NAMES: [&str; MAPS] = ["ours2", "theirs2", "balance", "front", "front_enemy", "choke", "gap", "siege", "cut_off", "our_cut", "safe2", "cohesion", "lonely", "flow"];
    /// Not a weight: keeps `Milli` used in this module for its doc.
    pub const _ONE: Milli = 1000;
}

/// The seven hand kernels (design section 2): conv(x; c, n) = c·x(t) + n·Σ
/// over the six neighbours; then the nonlinearity of the line.
pub mod conv {
    use super::Milli;

    /// V1: the centre of the density kernel: the force on the tile itself.
    pub const DENSITY_CENTRE: Milli = 1000;
    /// V1: a neighbour's force counts half: it can come, but not at once.
    pub const DENSITY_NEIGHBOUR: Milli = 500;
    /// V1: two passes (radius 2 ≈ a turn's strike) and divided by 4, so a
    /// lone unit's own tile reads about its own force.
    pub const DENSITY_DIV: i64 = 4;
    /// V1: balance = tanh(2 · (ours2 − theirs2)): a lead of 0.5 (a full
    /// strength-25 unit more nearby) is 0.76, almost sure.
    pub const BALANCE_GAIN: Milli = 2000;
    /// V2: the front = σ(4 · min(our force near, their reach and zone of
    /// control near) − 0.5): both sides touch the tile.
    pub const FRONT_GAIN: Milli = 4000;
    pub const FRONT_BIAS: Milli = 500;
    /// V3: a choke has at most 3 open neighbours: σ((3.5 − open) · 2).
    pub const CHOKE_OPEN: Milli = 3500;
    pub const CHOKE_OPEN_GAIN: Milli = 2000;
    /// V3: and the enemy's 2-turn reach flows through it: σ((flow − 0.2) · 8)
    /// (flow = the share of its neighbours they reach).
    pub const CHOKE_FLOW: Milli = 200;
    pub const CHOKE_FLOW_GAIN: Milli = 8000;
    /// V4: a siege forms with 2 or more of ours at the walls: σ(ring − 2).
    pub const SIEGE_RING: Milli = 2000;
    /// V4: and the fire nearly enough: σ((focus + 0.2) · 5), focus = (our
    /// summed blows − its health) / 100.
    pub const SIEGE_FIRE_BIAS: Milli = 200;
    pub const SIEGE_FIRE_GAIN: Milli = 5000;
    /// V5: a foe is cut off with no ally next to it: σ((1 − allies) · 3) …
    pub const CUT_ALLIES: Milli = 1000;
    pub const CUT_GAIN: Milli = 3000;
    /// V5: … and 2 or more of ours next to it: σ((ours − 1.5) · 3).
    pub const CUT_OURS: Milli = 1500;
    /// V6: safe where the danger is under 0.1 of a unit's health:
    /// 1 − σ((danger − 0.1) · 10).
    pub const SAFE_DANGER: Milli = 100;
    pub const SAFE_GAIN: Milli = 10000;
    /// V6: (0.5 + heal): a safe tile is worth half without healing.
    pub const SAFE_HEAL_BASE: Milli = 500;
    /// V6: the rear spreads one step losing 20 %. The design's sum over the
    /// neighbours would make a tile with many safe neighbours safer than a
    /// safe tile; the best neighbour is used.
    pub const SAFE_SPREAD: Milli = 800;
    /// V7: a group member's neighbours count 0.6 (two passes, radius 2) …
    pub const COHESION_NEIGHBOUR: Milli = 600;
    /// V7: … divided by 5: a tile with four members around reads about 1.
    pub const COHESION_DIV: i64 = 5;
    /// V7: lonely = σ((0.5 − cohesion) · 4).
    pub const LONELY_AT: Milli = 500;
    pub const LONELY_GAIN: Milli = 4000;
}

/// The nine concept neurons of a unit (design section 4).
pub mod concept {
    use super::Milli;

    /// Wounded: σ((0.5 − hp) / 0.1): below half health a unit's behaviour changes.
    pub const WOUNDED_AT: Milli = 500;
    pub const WOUNDED_WIDTH: Milli = 100;
    /// Under fire: σ((D_u / hp − 0.8) / 0.1): the enemy may take 80 % of
    /// what is left.
    pub const FIRE_AT: Milli = 800;
    pub const FIRE_WIDTH: Milli = 100;
    /// Capturer: a melee unit at a city that is about to fall:
    /// σ((focus(g) + 0.1) / 0.05).
    pub const CAPTURER_AT: Milli = -100;
    pub const CAPTURER_WIDTH: Milli = 50;
}

/// The motives' inner weights (design section 5).
pub mod motive {
    use super::Milli;

    /// m2: a kill of a cut-off foe is worth half more (V5).
    pub const KILL_CUT: Milli = 500;
    /// m4: danger above half the unit's health starts to cost.
    pub const SURVIVE_AT: Milli = 500;
    /// m4: ending the turn where the danger reaches the unit's health costs
    /// 10, unless the move takes a city or kills (the rule of the design).
    pub const DOOM: Milli = 10_000;
    /// m5: a turn closer to the target is worth 1.0. The design's «/10»
    /// scale made a turn of progress 0.1, less than any change of cohesion,
    /// and a group never set out; a turn is the unit here.
    pub const TOWARD: Milli = 1000;
    /// m6: cohesion − 0.5 · lonely.
    pub const LONELY: Milli = 500;
    /// m8: a firing position loses its worth as the danger there passes
    /// half the unit's health: 1 − σ((D_u − 0.5·hp) / 10 hp).
    pub const FIRE_SAFE_AT: Milli = 500;
    pub const FIRE_SAFE_WIDTH_HP: i64 = 10;
    /// m11: deferring a capturer while others still shoot is worth 2.
    pub const DEFER: Milli = 2000;
    /// m12: a pillage is worth (0.5 + wounded) of its value: a wounded unit
    /// heals by it.
    pub const PILLAGE_BASE: Milli = 500;
    /// m13: a melee unit covering a weak ally: the blows it keeps off,
    /// in the ally's health, times the unit price.
    pub const COVER: Milli = 1000;
    /// m14 (TurnPlan): a candidate that executes the commander's assignment
    /// for the unit is worth 2.5 — more than a kill elsewhere (m2 in war:
    /// 1.5 · 1.5), less than the doom rule (10): the plan leads, safety
    /// overrides.
    pub const ASSIGNED: Milli = 2500;
    /// A move toward the assigned strike's tile that keeps the strike for
    /// this turn is worth this share of the assignment.
    pub const ASSIGNED_STEP: Milli = 800;
}

/// A row of the output layer: the weight of each motive m1…m13 for one
/// role and phase.
#[derive(Debug, Clone, Copy)]
pub struct Row {
    pub name: &'static str,
    pub w: [Milli; 14],
    pub why: &'static str,
}

/// The motives' names in order (m1…m13).
pub const MOTIVES: [&str; 14] = ["take", "kill", "damage", "survive", "toward", "formation", "wall", "fire", "choke", "heal", "defer", "pillage", "cover", "finish"];

/// Rows of `TABLE`.
pub mod row {
    pub const MELEE_GATHER: usize = 0;
    pub const MELEE_APPROACH: usize = 1;
    pub const MELEE_ASSAULT: usize = 2;
    pub const RANGED_ASSAULT: usize = 3;
    pub const MOUNTED_ASSAULT: usize = 4;
    pub const WOUNDED: usize = 5;
    pub const DEFENCE: usize = 6;
    pub const RETREAT: usize = 7;
    pub const CIVILIAN: usize = 8;
    pub const RANGED_GATHER: usize = 9;
    pub const RANGED_APPROACH: usize = 10;
    pub const MOUNTED_GATHER: usize = 11;
    pub const MOUNTED_APPROACH: usize = 12;
}

/// The output layer (design section 5): the motives' weights by role and
/// phase. Rows 0–8 are the design's; rows 9–12 fill the roles and phases
/// the design's table leaves out. m13 (cover) is design 5b's motive; m14 (finish) is the siege design's N3, the kill chance a shot adds on top of the fire already assigned.
pub const TABLE: [Row; 13] = [
    Row { name: "melee · gather", w: [0, 500, 200, 2000, 500, 1500, 0, 0, 500, 500, 0, 0, 500, 0], why: "gathering: stay alive (2) and together (1.5); a kill on the way is taken, the target is far" },
    Row { name: "melee · approach", w: [0, 1000, 500, 1500, 1000, 1000, 0, 0, 1000, 200, 0, 200, 1000, 0], why: "approaching: forward (1) in formation (1), screening the shooters (cover 1), kills on the way (1)" },
    Row { name: "melee · assault", w: [3000, 1500, 800, 800, 500, 300, 1500, 0, 1000, 0, 1000, 500, 1000, 1000], why: "assault: take the city (3), stand at the wall (1.5), finish (1.5); risk is accepted (0.8)" },
    Row { name: "ranged · assault", w: [1000, 1500, 1000, 1200, 300, 200, 0, 2000, 0, 0, 0, 0, 0, 1500], why: "a shooter's assault: the firing position (2), kills (1.5) and damage (1), and it must live (1.2)" },
    Row { name: "mounted · assault", w: [1000, 2000, 500, 1000, 500, 0, 0, 500, 0, 0, 0, 1000, 0, 1000], why: "riders hunt (kill 2) and raid (pillage 1); they take a city when they can (1)" },
    Row { name: "any · wounded", w: [1000, 500, 200, 2500, 0, 300, 0, 200, 0, 2000, 0, 300, 0, 0], why: "wounded: survive (2.5) and heal (2) before anything; a capture still counts (1)" },
    Row { name: "any · defence", w: [0, 1500, 1000, 1000, 500, 500, 0, 1000, 1500, 300, 0, 0, 1000, 0], why: "defence: kill what comes (1.5), hold the chokes (1.5), cover the weak (1), fire (1)" },
    Row { name: "any · retreat", w: [0, 500, 300, 2000, -500, 500, 0, 300, 1500, 1000, 0, 0, 500, 0], why: "retreat: survive (2), away from the target (−0.5), hold the passes behind (1.5), heal (1)" },
    Row { name: "civilian", w: [0, 0, 0, 3000, 500, 0, 0, 0, 0, 500, 0, 0, 0, 0], why: "a civilian: out of danger above all (3), then on to its job (0.5)" },
    Row { name: "ranged · gather", w: [0, 500, 200, 2000, 500, 1500, 0, 500, 0, 500, 0, 0, 0, 0], why: "melee · gather without the choke and the cover (a shooter holds no pass), a firing position when foes come (0.5)" },
    Row { name: "ranged · approach", w: [0, 1000, 500, 1500, 1000, 1000, 0, 1000, 0, 200, 0, 0, 0, 0], why: "melee · approach without the choke, the pillage and the cover; a firing position on the way (1)" },
    Row { name: "mounted · gather", w: [0, 500, 200, 2000, 500, 1500, 0, 0, 500, 500, 0, 0, 500, 0], why: "as melee · gather: the group forms first" },
    Row { name: "mounted · approach", w: [0, 1500, 500, 1500, 1000, 1000, 0, 0, 500, 200, 0, 500, 500, 0], why: "melee · approach with more hunting (kill 1.5) and raiding (0.5), less pass holding" },
];

/// The prices and risk by the strategy's posture (design section 7).
pub mod prices {
    use super::Milli;

    /// A posture's prices: ц_убить, ц_город, ц_урон, ц_юнит, риск.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct PostureRow {
        pub name: &'static str,
        pub kill: Milli,
        pub city: Milli,
        pub damage: Milli,
        pub unit: Milli,
        pub risk: Milli,
        pub why: &'static str,
    }

    /// By posture: peace, defence, prepare, war (`strategy_net::weights::POSTURES`).
    pub const POSTURES: [PostureRow; 4] = [
        PostureRow { name: "peace", kill: 1000, city: 3000, damage: 100, unit: 2000, risk: 1500, why: "at peace a unit lost is a war lost before it starts: units dear (2), risk avoided (1.5)" },
        PostureRow { name: "defence", kill: 1000, city: 3000, damage: 100, unit: 2000, risk: 1500, why: "defending: as peace — hold and keep the army; the city's own value is in the key target" },
        PostureRow { name: "prepare", kill: 1200, city: 5000, damage: 100, unit: 1500, risk: 1000, why: "preparing a war: kills and cities start to pay, units still dear" },
        PostureRow { name: "war", kill: 1500, city: 8000, damage: 150, unit: 1000, risk: 700, why: "at war: a city is worth 8, a kill 1.5; units are spent for them, risk taken (0.7)" },
    ];
}

/// The commander's target values, fire allocation, flanks and reply check
/// (design sections 5a–5d).
pub mod plan {
    use super::Milli;

    /// A city's worth in hammers for the values (V): its buildings and land
    /// (base) and each citizen.
    pub const CITY_BASE_HAMMERS: i64 = 150;
    pub const CITY_HAMMERS_PER_POP: i64 = 50;
    /// Hammers per value point: V is in hundreds of hammers (a 100-hammer
    /// unit is 1.0).
    pub const HAMMERS_PER_POINT: i64 = 100;
    /// A wound removes half the threat in proportion: f = P·V + (1 − P) ·
    /// 0.5 · V · d / hp.
    pub const WOUND_SHARE: Milli = 500;
    /// Fire stops being added to a target once its kill chance reaches this:
    /// the next shooter is kept in reserve (for the fast update after a miss,
    /// or for another target).
    pub const NEAR_CERTAIN: Milli = 900;
    /// The reply check runs twice, its changes damped 0.5 then 0.25.
    pub const REPLY_ROUNDS: usize = 2;
    pub const REPLY_DAMPING: [Milli; 2] = [500, 250];
    /// A unit whose gain is less than its price times its chance to die gets
    /// this penalty share of its value on that variant.
    pub const DEATH_PENALTY: Milli = 1000;
    /// Players' danger weights by relation: at war 1, others 0 (peace keeps
    /// their units off ours), a war on a third × 0.5.
    pub const WAR_WEIGHT: Milli = 1000;
    pub const BUSY_WEIGHT: Milli = 500;
    /// A civilian's danger counts three times (design 5d): it cannot fight back.
    pub const CIVILIAN_DANGER: Milli = 3000;
    /// Most strike candidates the allocation keeps per unit (its best tiles).
    pub const STRIKE_TILES: usize = 4;
    /// The spread of a blow around its expected damage when the preview
    /// gives none (a shot of 30 lands 20–40).
    pub const SPREAD_PERCENT: i64 = 33;
}

/// The operation head (design section 6).
pub mod op {
    use super::Milli;

    /// Most operations at once.
    pub const MAX_OPS: usize = 8;
    /// Members within this many turns of the target count for readiness.
    pub const READY_TURNS: i64 = 2;
    /// Readiness r ≥ 1 moves the group from gathering to the approach.
    pub const READY: Milli = 1000;
    /// After this many rounds of gathering the group sets out anyway.
    pub const GATHER_DEADLINE: u32 = 10;
    /// Within this many turns of the target the approach is an assault.
    pub const ASSAULT_TURNS: i64 = 2;
    /// P(take) = σ((ρ − 1.2) / 0.2): an edge of 1.2 in the Lanchester ratio
    /// is even.
    pub const TAKE_AT: Milli = 1200;
    pub const TAKE_WIDTH: Milli = 200;
    /// Waiting a turn costs 0.3 of a city's price.
    pub const WAIT_COST: Milli = 300;
    /// The assault turns to retreat when the losses pass 1.5 × the gain.
    pub const RETREAT_RATIO: Milli = 1500;
    /// A change of option needs a lead of 15 %.
    pub const HYSTERESIS: Milli = 1150;
    /// Approaching out of contact, a member more than this many tiles ahead
    /// of the group's median waits (no trickle into the enemy's reach).
    pub const LEAD: i32 = 2;
    /// Most rounds a retreat lasts before the operation ends.
    pub const RETREAT_ROUNDS: u32 = 4;
    /// A defence operation for a city whose danger is at least this share
    /// of its defenders' health.
    pub const DEFEND_AT: Milli = 300;
    /// Members of a defence operation come from this many tiles.
    pub const DEFEND_RADIUS: i32 = 6;
    /// The city's defence bonus in N, percent.
    pub const CITY_DEFENCE_PERCENT: i64 = 25;
    /// The most turns to take a city the losses are counted over.
    pub const MAX_SIEGE_TURNS: i64 = 8;
    /// Orders: a defence operation short of this share of the threat's force
    /// orders a defender.
    pub const ORDER_SHORT: Milli = 800;
    /// An order's value per hammer of the threatened city's worth at risk.
    pub const ORDER_VALUE: Milli = 600;
}

/// The count of hand weights and parameters (the page prints it).
pub fn count() -> usize {
    let table = TABLE.len() * 13;
    let prices = prices::POSTURES.len() * 5;
    // conv 27, concepts 6, motives 12, fields 12, plan 12, op 17.
    table + prices + 27 + 6 + 12 + 12 + 12 + 17
}

/// Not a weight: the type of the tables above.
pub type W = Milli;
