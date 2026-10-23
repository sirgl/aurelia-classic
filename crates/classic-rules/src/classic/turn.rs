//! The classic turn: the module's steps and the order of a whole end turn
//! (platform/PLATFORM.md stage 3). [`SCHEDULE`] is the spec; the kernel checks it
//! at compile time (`turn::resolution::TurnSchedule::validate`) and the
//! pinned game hashes hold it to the order the game always had.
//!
//! End turn of one player (K: kernel step, M: this module's step):
//!
//! | # | Phase | Step | | Why here |
//! |---|---|---|---|---|
//! | 1 | Start | HealUnits | K | reads «did not act» before the moves return |
//! | 2 | Start | CitadelDamage | M | unit reset: after healing, with the turn's units |
//! | 3 | Start | MountainDamage | M | the same slot: ending the turn on a mountain hurts the units that may cross them |
//! | 4 | Start | RestoreMoves | K | after healing and citadel damage (both read the past turn) |
//! | 5 | Start | CapitalConnections | K | before the turn's effects and yields read the city status |
//! | 6 | Start | TurnStartEffects | K | OnTurnStart before anything of the resolution |
//! | 7 | Start | NaturalWonderFinds | M | settles the finds of the turn just played; no known dependency on its slot |
//! | 8 | Resolution | Happiness | M | the snapshot every later step reads |
//! | 9 | Resolution | GoldenAge | M | fills from that happiness; before the cities read it |
//! | 10 | Resolution | Trade | M | route values and moves before the cities and the gold (rules.md) |
//! | 11 | Resolution | Barbarians | M | camps and spawns; no known dependency, today's slot |
//! | 12 | Resolution | CityCombat | M | cities heal, resistance and razing before they resolve |
//! | 13 | Resolution | Cities | K | each city runs the city schedule below; yields summed |
//! | 14 | Resolution | Gold | K | the cities' gold; a deficit is recorded for science |
//! | 15 | Resolution | Culture | M | the cities' culture (from Borders) plus city-states, heritage, happiness |
//! | 16 | Resolution | Tourism | M | reads the culture just collected (lifetime culture) |
//! | 17 | Resolution | Science | K | after Gold: the deficit is taken from science |
//! | 18 | Resolution | Research | K | spends the science just collected |
//! | 19 | Resolution | Espionage | M | no known dependency, today's slot |
//! | 20 | Resolution | Diplomacy | M | after Science: research agreements accumulate it |
//! | 21 | Resolution | CityStates | M | influence drift and quests; no known dependency, today's slot |
//! | 22 | Resolution | Congress | M | boosts run out, then the World Congress (once a round) |
//! | 23 | Resolution | Faith | M | the cities' faith and the Great Prophet roll; no known dependency |
//! | 24 | Resolution | WorkerJobs | K | builds progress last: improvements count from the next turn |
//! | 25 | End | ConquestDecisions | M | an undecided captured city becomes a puppet |
//! | 26 | End | TimedEffects | K | charged after every step that read them |
//! | 27 | End | Victory | M | tested on the final state of the turn |
//!
//! A city's resolution (the city turn, rules.md), from one snapshot of its
//! yields:
//!
//! | # | Step | | Why here |
//! |---|---|---|---|
//! | 1 | GreatPeople | M | specialists, before growth (rules.md) |
//! | 2 | Growth | K | |
//! | 3 | Production | K | after growth (a settler eats the surplus) |
//! | 4 | Borders | K | the city's culture: borders grow, the player's culture sums |
//!
//! At the end of a round (after the last player's turn, rules.md):
//! religious pressure between cities.
//!
//! «No known dependency» marks a slot nothing is known to require: the
//! order is kept because the hashes pin it, and moving the step is a
//! behaviour change.

use crate::classic::Classic;
use crate::transaction::transaction::WorldTransaction;
use crate::turn::resolution::{KernelCityStep, KernelStep, Step, TurnSchedule};
use crate::turn::TurnPhase;

/// What a module step of a player's end turn reads.
pub type EndTurnInput<'t> = kernel::systems::turn::EndTurnInput<'t, Classic>;

/// What a module step of one city's resolution reads.
pub type CityStepInput<'t> = kernel::systems::turn::CityStepInput<'t, Classic>;

/// A classic step of a player's end turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassicStep {
    /// Units next to an enemy citadel take its damage (combat).
    CitadelDamage,
    /// Land units that cross mountains and end the turn on one are hurt.
    MountainDamage,
    /// Natural wonders the player explored this turn are found: the first
    /// finder's gold, the find triggers.
    NaturalWonderFinds,
    /// Happiness snapshot of the turn.
    Happiness,
    /// The golden age counts down or fills its meter with the excess
    /// happiness.
    GoldenAge,
    /// Trade route values, trade units move, finished routes end.
    Trade,
    /// The barbarian player places camps and spawns units.
    Barbarians,
    /// Cities heal, may strike again; resistance and razing advance.
    CityCombat,
    /// The cities' culture and its other sources into the player's pool.
    Culture,
    /// Influence on every met civilization, then public opinion.
    Tourism,
    /// Spies granted and processed.
    Espionage,
    /// Gold per turn of deals, research agreements, expiries.
    Diplomacy,
    /// A city-state's influence drift, quests, unit gifts.
    CityStates,
    /// The player's World Congress project boosts run out a turn; then the
    /// World Congress: completed projects rewarded, founding, sessions,
    /// the tally (once a round).
    Congress,
    /// The cities' faith into the player's pool; the Great Prophet roll.
    Faith,
    /// Captured cities still undecided become puppets.
    ConquestDecisions,
    /// Victories are tested.
    Victory,
}

/// A classic step of one city's resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassicCityStep {
    /// Great person points and births.
    GreatPeople,
}

use ClassicStep as M;
use KernelStep as K;
use TurnPhase::{End, Resolution, Start};

/// The order of a classic turn (the tables in the module doc).
pub const SCHEDULE: TurnSchedule<ClassicStep, ClassicCityStep> = TurnSchedule {
    end_turn: &[
        (Start, Step::Kernel(K::HealUnits)),
        (Start, Step::Module(M::CitadelDamage)),
        (Start, Step::Module(M::MountainDamage)),
        (Start, Step::Kernel(K::RestoreMoves)),
        (Start, Step::Kernel(K::CapitalConnections)),
        (Start, Step::Kernel(K::TurnStartEffects)),
        (Start, Step::Module(M::NaturalWonderFinds)),
        (Resolution, Step::Module(M::Happiness)),
        (Resolution, Step::Module(M::GoldenAge)),
        (Resolution, Step::Module(M::Trade)),
        (Resolution, Step::Module(M::Barbarians)),
        (Resolution, Step::Module(M::CityCombat)),
        (Resolution, Step::Kernel(K::Cities)),
        (Resolution, Step::Kernel(K::Gold)),
        (Resolution, Step::Module(M::Culture)),
        (Resolution, Step::Module(M::Tourism)),
        (Resolution, Step::Kernel(K::Science)),
        (Resolution, Step::Kernel(K::Research)),
        (Resolution, Step::Module(M::Espionage)),
        (Resolution, Step::Module(M::Diplomacy)),
        (Resolution, Step::Module(M::CityStates)),
        (Resolution, Step::Module(M::Congress)),
        (Resolution, Step::Module(M::Faith)),
        (Resolution, Step::Kernel(K::WorkerJobs)),
        (End, Step::Module(M::ConquestDecisions)),
        (End, Step::Kernel(K::TimedEffects)),
        (End, Step::Module(M::Victory)),
    ],
    city: &[
        Step::Module(ClassicCityStep::GreatPeople),
        Step::Kernel(KernelCityStep::Growth),
        Step::Kernel(KernelCityStep::Production),
        Step::Kernel(KernelCityStep::Borders),
    ],
};

/// Run one classic step of `input.player`'s end turn.
pub(crate) fn run_step(tx: &mut WorldTransaction<'_>, step: ClassicStep, input: &EndTurnInput<'_>) {
    let (player, rules, ledger) = (input.player, input.rules, input.ledger);
    match step {
        M::CitadelDamage => tx.citadel_damage(player, rules),
        M::MountainDamage => tx.mountain_damage(player, rules),
        M::NaturalWonderFinds => tx.settle_natural_wonder_finds(player, rules),
        M::Happiness => tx.update_happiness(player, rules),
        M::GoldenAge => tx.process_golden_age(player, rules),
        M::Trade => tx.trade_turn(player, rules),
        M::Barbarians => tx.barbarian_turn(player, rules, input.derived),
        M::CityCombat => tx.conquest_and_city_heal(player, rules),
        M::Culture => collect_culture(tx, input),
        M::Tourism => tx.tourism_turn(player, rules),
        M::Espionage => tx.espionage_turn(player, rules),
        M::Diplomacy => tx.diplomacy_turn(player, ledger.science, rules),
        M::CityStates => tx.city_state_turn(player, rules),
        M::Congress => {
            tx.league_rewards_turn(player);
            tx.congress_turn(rules)
        }
        M::Faith => tx.collect_faith(player, ledger.module_yield, rules),
        M::ConquestDecisions => tx.resolve_pending_decisions(player),
        M::Victory => tx.test_victories(rules),
    }
}

/// Run one classic step of a city's resolution.
pub(crate) fn run_city_step(tx: &mut WorldTransaction<'_>, step: ClassicCityStep, input: &CityStepInput<'_>) {
    match step {
        ClassicCityStep::GreatPeople => {
            tx.accumulate_great_people(input.owner, input.city, input.yields, input.rules)
        }
    }
}

/// The cities' culture, plus cultured city-states (§4.6), heritage and
/// excess happiness, boosted by the World's Fair (§6.8), into the pool.
fn collect_culture(tx: &mut WorldTransaction<'_>, input: &EndTurnInput<'_>) {
    let (player, rules) = (input.player, input.rules);
    let minors = crate::systems::city_states::player_bonus(
        tx.world,
        rules,
        player,
        crate::rules::content::CityStateBonusKind::Culture,
    );
    let heritage = crate::systems::congress::heritage_culture(tx.world, rules, player);
    let happiness = crate::derived::income::culture_from_happiness(tx.world, rules, player);
    let culture = input
        .ledger
        .culture
        .saturating_add(minors.max(0) as u32)
        .saturating_add(heritage)
        .saturating_add(happiness);
    let boost = crate::systems::league_projects::boost_percent(
        tx.world.module.congress.rewards_of(player).culture_boost_turns,
    );
    let culture = (u64::from(culture) * u64::from(100 + boost) / 100).min(u64::from(u32::MAX)) as u32;
    tx.collect_culture(player, culture)
}
