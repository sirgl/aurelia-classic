//! Defence against humans (classic/docs/ai-stress.md, "Defence against humans"): one seat plays a
//! trick of a strong human player (work/human-playbook-research.md) as a small explicit plan, the
//! game's AI plays every other seat and the rest of the trickster's seat; the AI nation the trick
//! targets is judged against the same position played on without the trick.
//!
//! Each trick is a `Trick`; its plan (who is who, what both arms share), its edit of one strength and
//! its script (`Order`, a state machine behind `seat::Seat`) live in the file of its group:
//! `campaign` (an army that takes cities: rush, sneak attack, new-city vulture), `bait`, `squat`,
//! `deals` (swindle, drain, white peace), `turtle`, `sniper`, `lobby` (congress lobby, city-state
//! poaching). `army` holds the moves a human army makes; `metrics` the numbers; `gates` the gates.

pub mod army;
pub mod bait;
pub mod campaign;
pub mod deals;
pub mod gates;
pub mod lobby;
pub mod metrics;
pub mod offence;
pub mod sniper;
pub mod squat;
pub mod turtle;

use classic_rules::ids::PlayerId;
use classic_rules::{CompiledRuleset, GameState};
use serde::{Deserialize, Serialize};

use super::position::{self, Position};
use super::scene::{Edit, Plan};
use super::seat::Seat;
use crate::bench::modules::majors;

/// The human tricks built (the ids of work/human-playbook-research.md in the doc comments).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Trick {
    /// MO1: an early rush on the capital, archers shoot from two tiles, melee takes it.
    Rush,
    /// MO3 (D12): an army walks in under open borders at peace, declares war and strikes the same turn.
    Sneak,
    /// MO5 (D14): the seat's luxury and strategic tiles pillaged and held by units standing on them.
    Squat,
    /// MT2, MT3: a worker or a weak unit as bait next to a city; a hidden force kills what comes out.
    Bait,
    /// MO8 (M4): an army waits at the border and takes every city the seat founds.
    Vulture,
    /// D1, D3, D6, E2: a friend sells a luxury (or its gold a turn) for a lump sum, declares war,
    /// makes peace after ten rounds and sells again.
    Swindle,
    /// D2: the seat's treasury drained by a gold-a-turn promise for a lump sum, then a war.
    Drain,
    /// D8: the losing side asks white peace every round.
    Beg,
    /// MO11 (D18): the score leader keeps almost no army.
    Turtle,
    /// V1 (E5): every world wonder of the seat completed by the sniper a few rounds before it is done.
    Sniper,
    /// W2, W3, W4: city-states bought before each congress session, an embargo on the leader proposed.
    Lobby,
    /// CS2-CS5, S3, CS1: the city-states allied to the seat bought away with gold and coups.
    Poach,
    // The offence: the AI seat judged is the one that could play the trick (`offence`); the
    // situation hands it the opportunity, its own AI decides.
    /// MO1: an archer army at home next to an early rival's capital, at peace.
    OffRush,
    /// MO3: the rival's borders open to the seat, the seat's army at home next to them.
    OffSneak,
    /// MO6, MO7: at war, the seat's fast units in the rival's countryside, beside its workers.
    OffRaid,
    /// MO2, MO4: the seat's great generals on its border next to a rival's city.
    OffCitadel,
    /// MO8: at war, the rival's new city founded three to four tiles from the seat's city.
    OffVulture,
    /// W2: the seat, second by score, rich before the congress sessions.
    OffLobby,
    /// CS6: the seat rich, a rival holding city-state allies.
    OffPoach,
}

/// Who the trick is judged on: the AI it targets (defence) or the AI that could play it (offence).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Defence,
    Offence,
}

impl Trick {
    pub const ALL: [Trick; 12] = [
        Trick::Rush,
        Trick::Sneak,
        Trick::Squat,
        Trick::Bait,
        Trick::Vulture,
        Trick::Swindle,
        Trick::Drain,
        Trick::Beg,
        Trick::Turtle,
        Trick::Sniper,
        Trick::Lobby,
        Trick::Poach,
    ];

    /// The offence tricks.
    pub const OFFENCE: [Trick; 7] = [Trick::OffRush, Trick::OffSneak, Trick::OffRaid, Trick::OffCitadel, Trick::OffVulture, Trick::OffLobby, Trick::OffPoach];

    /// Every trick, defence then offence.
    pub fn every() -> Vec<Trick> {
        Self::ALL.into_iter().chain(Self::OFFENCE).collect()
    }

    pub fn side(self) -> Side {
        match self {
            Trick::OffRush | Trick::OffSneak | Trick::OffRaid | Trick::OffCitadel | Trick::OffVulture | Trick::OffLobby | Trick::OffPoach => Side::Offence,
            _ => Side::Defence,
        }
    }

    /// The situation's name (`h` and the research id; "h" keeps them apart from the catalogue's).
    pub fn name(self) -> &'static str {
        match self {
            Trick::Rush => "hmo1",
            Trick::Sneak => "hmo3",
            Trick::Squat => "hmo5",
            Trick::Bait => "hmt2",
            Trick::Vulture => "hmo8",
            Trick::Swindle => "hd1",
            Trick::Drain => "hd2",
            Trick::Beg => "hd8",
            Trick::Turtle => "hmo11",
            Trick::Sniper => "hv1",
            Trick::Lobby => "hw2",
            Trick::Poach => "hcs",
            Trick::OffRush => "omo1",
            Trick::OffSneak => "omo3",
            Trick::OffRaid => "omo7",
            Trick::OffCitadel => "omo2",
            Trick::OffVulture => "omo8",
            Trick::OffLobby => "ow2",
            Trick::OffPoach => "ocs",
        }
    }

    /// The strengths: white peace has one.
    pub fn levels(self) -> u8 {
        match self {
            Trick::Beg => 1,
            _ => 3,
        }
    }

    /// The rounds both arms are played on.
    pub fn window(self) -> u32 {
        match self {
            Trick::Rush => 30,
            Trick::Sneak => 25,
            Trick::Squat => 20,
            Trick::Bait => 20,
            Trick::Vulture => 40,
            Trick::Swindle => 40,
            Trick::Drain => 20,
            Trick::Beg => 25,
            Trick::Turtle => 40,
            Trick::Sniper => 30,
            Trick::Lobby => 40,
            Trick::Poach => 40,
            Trick::OffRush => 30,
            Trick::OffSneak => 25,
            Trick::OffRaid => 20,
            Trick::OffCitadel => 25,
            Trick::OffVulture => 25,
            Trick::OffLobby => 40,
            Trick::OffPoach => 40,
        }
    }

    /// The round an instance starts at: the rush and the vulture at rounds 50 to 70 (any set of logs
    /// that reaches round 70 serves, e.g. `runs/positions-early`), the rest at the run's round.
    pub fn start(self, index: usize, round: u32, jitter: u32) -> u32 {
        match self {
            Trick::Rush | Trick::Vulture | Trick::OffRush => position::round_of(index, 50, 20),
            _ => position::round_of(index, round, jitter),
        }
    }
}

/// Who is who in a trick: `human` plays it, `seat` is the AI nation it targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cast {
    pub trick: Trick,
    pub seat: u8,
    pub human: u8,
}

impl Cast {
    /// The nations judged: the target, or for the turtle every nation but the turtle (does anyone
    /// take what a defenceless leader holds).
    pub fn focus(&self, game: &GameState) -> Vec<PlayerId> {
        match self.trick {
            Trick::Turtle => majors(game).into_iter().filter(|p| p.0 != self.human).collect(),
            _ => vec![PlayerId(self.seat)],
        }
    }
}

/// The script of a trick, as data (the saved scene's `script`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "human", rename_all = "snake_case")]
pub enum Order {
    Campaign(campaign::Campaign),
    Bait(bait::Bait),
    Squat(squat::Squat),
    Swindle(deals::Swindle),
    Drain(deals::Drain),
    Beg(deals::Beg),
    Turtle(turtle::Turtle),
    Sniper(sniper::Sniper),
    Lobby(lobby::Lobby),
    Poach(lobby::Poach),
}

impl Order {
    /// The script with the units the edit spawned (an army plays them).
    pub fn with_units(self, spawned: &[u64]) -> Order {
        match self {
            Order::Campaign(c) => Order::Campaign(campaign::Campaign { units: spawned.to_vec(), ..c }),
            Order::Bait(b) => Order::Bait(bait::Bait { units: spawned.to_vec(), ..b }),
            Order::Squat(s) => Order::Squat(squat::Squat { units: spawned.to_vec(), ..s }),
            other => other,
        }
    }

    /// The state machine of the script.
    pub fn start(&self) -> Box<dyn Seat> {
        match self {
            Order::Campaign(c) => Box::new(campaign::Play::new(c.clone())),
            Order::Bait(b) => Box::new(bait::Play::new(b.clone())),
            Order::Squat(s) => Box::new(squat::Play::new(s.clone())),
            Order::Swindle(s) => Box::new(deals::SwindlePlay::new(s.clone())),
            Order::Drain(d) => Box::new(deals::DrainPlay::new(d.clone())),
            Order::Beg(b) => Box::new(deals::BegPlay::new(b.clone())),
            Order::Turtle(t) => Box::new(turtle::Play::new(t.clone())),
            Order::Sniper(s) => Box::new(sniper::Play::new(s.clone())),
            Order::Lobby(l) => Box::new(lobby::LobbyPlay::new(l.clone())),
            Order::Poach(p) => Box::new(lobby::PoachPlay::new(p.clone())),
        }
    }
}

/// The roles and the base of `trick` on the position.
pub fn plan(trick: Trick, rules: &CompiledRuleset, position: &Position, instance: usize) -> Result<Plan, String> {
    match trick {
        Trick::Rush | Trick::Sneak | Trick::Vulture => campaign::plan(trick, rules, position, instance),
        Trick::Bait => bait::plan(rules, position, instance),
        Trick::Squat => squat::plan(rules, position, instance),
        Trick::Swindle | Trick::Drain | Trick::Beg => deals::plan(trick, rules, position, instance),
        Trick::Turtle => turtle::plan(rules, position, instance),
        Trick::Sniper => sniper::plan(rules, position, instance),
        Trick::Lobby | Trick::Poach => lobby::plan(trick, rules, position, instance),
        _ => offence::plan(trick, rules, position, instance),
    }
}

/// The edit of `trick` at `level` (0 mild, 1 medium, 2 strong).
pub fn edit_of(trick: Trick, level: u8, rules: &CompiledRuleset, position: &Position, plan: &Plan) -> Result<Edit, String> {
    let super::scene::Roles::Human(cast) = plan.roles else {
        return Err(format!("{} plans no human cast: {:?}", trick.name(), plan.roles));
    };
    let level = usize::from(level.min(2));
    match trick {
        Trick::Rush | Trick::Sneak | Trick::Vulture => campaign::edit_of(cast, level, rules, position),
        Trick::Bait => bait::edit_of(cast, level, rules, position),
        Trick::Squat => squat::edit_of(cast, level, rules, position),
        Trick::Swindle | Trick::Drain | Trick::Beg => deals::edit_of(cast, level, rules, position),
        Trick::Turtle => turtle::edit_of(cast, level, rules, position),
        Trick::Sniper => sniper::edit_of(cast, level, rules, position),
        Trick::Lobby | Trick::Poach => lobby::edit_of(cast, level, rules, position),
        _ => offence::edit_of(cast, level, rules, position),
    }
}

/// An edit made of a script only (and the changes before it).
pub(crate) fn scripted(changes: Vec<super::edit::Change>, order: Order) -> Edit {
    Edit { changes, gift: None, script: Some(super::seat::Script::Human(order)) }
}
