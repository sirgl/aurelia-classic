//! Named versions of the classic-ruleset AI (platform/docs/ai-bench.md). The game plays
//! `Standard`; the AI bench seats the others against it
//! (`kernel::ai::seats::Seated`). A change of the AI is developed as a new
//! variant next to `Standard`; once it wins on the bench, it becomes the
//! standard and the old one stays as a named variant while the bench needs
//! it.
//!
//! The steps of ru/PLAN.en.md "M6.2. A whole AI", each on the previous:
//! `Legacy` is the M6 planners (the game's AI until 16 October, then named
//! `baseline`); `Tactics` decides the military units near an enemy as one
//! front (`ai::front`, classic/docs/ai-tactics.md); `Operations` fights its wars by
//! operations: an army formed for one city, built by the cities and
//! launched when assembled (`ai::operations`, classic/docs/ai-operations.md);
//! `Allocator` spends production, gold and research by one allocator
//! (`ai::allocator`, classic/docs/ai-allocator.md; named `economy`, then the
//! game's AI `standard`); `Leaders` is `Allocator` played by its leaders:
//! their personalities and a grand strategy that pursues a path to victory
//! (`ai::grand_strategy`, classic/docs/ai-leaders.md, step 5), and peace by value
//! (`ai::peace`, step 6); `Defence` is `Leaders` that values keeping its
//! cities (`ai::defence`, classic/docs/ai-defence.md, step 6 «defence»). `Direct`
//! is `Defence` whose allocator prices gold by its best use and happiness
//! by its effects (`ai::value::PriceModel::Uses`, classic/docs/ai-allocator.md
//! "Gold and happiness"; named `defence-econ2`, then the game's AI
//! `standard`, until the science economy): it prices a beaker at the score
//! of the technologies and orders what the rules refuse. `Standard` is
//! `Direct` with the science economy (classic/docs/ai-allocator.md "Science"): it
//! orders only what the rules accept (`Sound`) and prices a beaker by what
//! the technologies it buys open (`SciencePricing::Reach`; named `reach`
//! until it became the game's AI); `AllocatorEcon2` and `LeadersEcon2` are `Allocator` and `Leaders`
//! with those prices. `AllocatorPeace` is `Allocator` without new wars,
//! the control of its wars; `LeadersAlone` is `Leaders` without peace by
//! value, its control. The instruments of the bench itself are built on
//! `Standard`: `Nudged` plays as well but differently (the noise floor),
//! `OneCity` is known to be weaker (the bench sees a difference).
//!
//! Since 1 November `Standard` is `WarFix`; the variants below described as
//! `Standard` with or without a change are built on `PreWarFix`, the standard
//! before it. Later that day `Standard` took the switches of `UnitAbilities`
//! and `ReligionFix` (the fifth promotion); `PreAbilities` is the standard
//! before them, the one `UnitAbilities`, `ReligionFix` and the war-fix
//! variants describe as `Standard`. In the sixth promotion (1 November)
//! `Standard` took the switch of `TallScience` (`Tall::Science`); `PreTall`
//! is the standard before it, the one `TallScience`, `TallBook`, its probes
//! and `ScienceFirst` describe as `Standard`. `TallScience` stays as an alias
//! that plays as `Standard` did then (as `PreSiege` since the seventh
//! promotion). In the seventh promotion (1 November) `Standard`
//! took the siege of walled cities of `SiegeWalls` (`siege_walls`, the driver's
//! `ai::siege_walls` plans and the siege press); `PreSiege` is the standard
//! before it, the one `SiegeWalls` and the books describe as `Standard`.
//! `SiegeWalls` stays as an alias that plays as `Standard` did then (as
//! `PrePuppet` since the eighth promotion). `Net` plays on `NET_BASE`: every
//! switch but `net` is its base's.
//! `PuppetPrice` and `PuppetAll` are `Standard` but for the decision of a captured
//! city (`conquest`, `ai::conquest`; classic/docs/ai-conquest-payoff.md). In the
//! eighth promotion (2 November) `Standard` took the decision of `PuppetAll`: every
//! captured city a puppet; `PrePuppet` is the standard before it, the one the
//! books, `SiegeWalls` and `NET_BASE` play, and `PuppetAll` stays as an alias that
//! plays as `Standard`. `PuppetWar` is the new `Standard` whose wars price a
//! capture as the puppet it makes of it. In the ninth promotion (2 November)
//! `Standard` took the switches of `SiegeJoin` (`siege_join`: who besieges, the
//! melee closing in, the line of sight); `PreJoin` is the standard before it, the
//! one `PuppetAll`, `PuppetPrice` and `PuppetWar` play as, and `SiegeJoin` stays as
//! an alias that plays as `Standard` did then (as `PreWorkers` since the tenth
//! promotion). In the tenth promotion (2 November) `Standard` took the switches of
//! `WorkersFit` (`workers_fit`: a worker more for the work left, the flight
//! mid-build, the wait in a city, the repair); `PreWorkers` is the standard before
//! it, and `WorkersFit` stays as an alias that plays as `Standard`. The candidates
//! described as `Standard` with one change (`TradeFit`, `NoIdleCities`,
//! `ScoutEarly`, `ArmyFit`, `Camps`, `ArmyCamps`, `PromoFit`, `HealFirst`),
//! `UncivMilitary` and Unciv's
//! modules inside `Standard` carry the switches with it. In the eleventh promotion
//! (2 November) `Standard` took the switches of `ArmyCamps` (`army_fit`: the unit kind
//! by its purpose; `camps`: the barbarian camps cleared); `PreCamps` is the standard
//! before it, the one `WorkersFit` plays as since; `ArmyCamps` stays as an alias that
//! plays as `Standard`, and `ArmyFit` and `Camps` stay as measured: `PreCamps` with one
//! of the two switches. In the twelfth promotion (3 November) `Standard` took the strategy
//! net of `StrategyNet` (`strategy_net`: research, policies and the choice of orders;
//! classic/docs/ai-strategy-net.md "Promoted"); `StandardScripted` is the standard before it
//! (bench only, `experimental-ai`); `StrategyNet` stays as an alias that plays as `Standard`.

use crate::ai::grand_strategy::Stakes;
use crate::ai::operations::WarGate;
use crate::ai::peace::PeaceMaking;
use crate::ai::planner::{AiSettings, Decision, TurnBrief};
use crate::ai::trace::Reason;
use crate::ai::value::{PriceModel, Prices, SciencePricing};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Variant {
    /// The game's AI, with the heavy arms since 23 October (missiles and
    /// nuclear weapons, carriers, paratrooper jumps, the war case that counts
    /// them: classic/docs/ai-strikes.md). The nations' leaders (personalities, a grand
    /// strategy, peace by deal) defending their cities by value, with gold
    /// worth its best use, happiness worth its effects (M6.2 steps 4–6) and
    /// a beaker worth what the technologies it buys open (the science
    /// economy, classic/docs/ai-allocator.md "Science"). Since 1 November it
    /// fights as `FightFixNet` and wars with `SiegeFixes::WAR_FIX` and the recall
    /// of `WarFix` (the fourth promotion, classic/docs/ai-war-fix.md "Promoted").
    /// Since the fifth promotion (1 November) its units also use the abilities of
    /// `UnitAbilities` and its religion is priced as `ReligionFix` prices it
    /// (classic/docs/ai-unit-abilities.md and classic/docs/ai-religion-fix.md "Promoted").
    /// Since the sixth promotion (1 November) it plays the tall-science hypothesis of
    /// `TallScience` from the rules' data: a city count, sites, policies and trade
    /// routes priced by their science (`ai::tall`, classic/docs/ai-science-pace.md
    /// "Promoted").
    /// Since the seventh promotion (1 November) its armies besiege walled cities as
    /// `SiegeWalls` does: gathered out of a city's reach, one strike together, the
    /// city worth most a round of war, the reinforcements by their measured arrival
    /// (`ai::siege_walls`, classic/docs/ai-siege-walls.md "Promoted").
    /// Since the eighth promotion (2 November) it makes every city it captures a puppet,
    /// as `PuppetAll` does (`Conquest::PuppetAll`, classic/docs/ai-conquest-payoff.md
    /// "Promoted"); its wars still price a capture of 4+ citizens as annexed.
    /// Since the ninth promotion (2 November) it besieges as `SiegeJoin` does: a
    /// military unit at the walls of the war's objective without a task or drafted
    /// into a forming operation is a besieger, the melee members close in, the
    /// shooters fire from tiles with a line of sight (`siege_walls::SiegeJoin`,
    /// classic/docs/ai-siege-walls.md "Promoted: siege-join").
    /// Since the tenth promotion (2 November) its workers fit the work as `WorkersFit`'s
    /// do: a worker more only while the work the rules still offer in the cities'
    /// radius gives each its share, a worker leaves a hostile unit's reach also in
    /// the middle of a build, waits in an own city without a job, and repairs pillaged
    /// improvements (`planner_workers::WorkersFit`, classic/docs/ai-workers.md
    /// "Promoted").
    /// Since the eleventh promotion (2 November) its cities order the unit kind that fits
    /// its purpose and its armies take ranged escorts (`ai::army_fit`), and its free
    /// military units clear the barbarian camps near its cities when it pays
    /// (`ai::camps`), as `ArmyCamps` does (classic/docs/ai-army.md "Promoted").
    /// Since the twelfth promotion (3 November) its research, policies and choice among the
    /// allocator's best orders are the strategy net's, as `StrategyNet`'s are
    /// (`ai::strategy_net`, classic/docs/ai-strategy-net.md "Promoted").
    #[default]
    Standard,
    /// The game's AI before the eleventh promotion (2 November): `Standard` without the
    /// switches of `ArmyFit` and `Camps`; the control of the promotion and the base of the
    /// alias `WorkersFit`, of `ArmyFit` and of `Camps`.
    PreCamps,
    /// The game's AI before the tenth promotion (2 November): `PreCamps` without the
    /// switches of `WorkersFit`; the control of the promotion and the base of the
    /// alias `SiegeJoin`.
    PreWorkers,
    /// The game's AI before the ninth promotion (2 November): `PreWorkers` without the
    /// switches of `SiegeJoin`; the control of the promotion and the base of
    /// `PuppetAll`, `PuppetPrice` and `PuppetWar`.
    PreJoin,
    /// The game's AI before the eighth promotion (2 November): `Standard` annexing a
    /// captured city of `combat::ANNEX_POPULATION` citizens and more; the control of the
    /// promotion, the base of the books, of the alias `SiegeWalls` and of the net
    /// (`NET_BASE`).
    PrePuppet,
    /// The game's AI before the seventh promotion (1 November): `Standard` without
    /// the siege of walled cities; the control of the promotion.
    PreSiege,
    /// The game's AI before the sixth promotion (1 November): `Standard` without the
    /// tall-science play, with the unit abilities and the religion fix; the control
    /// of the promotion.
    PreTall,
    /// The game's AI before the fifth promotion (1 November): `Standard` without the
    /// unit abilities and the religion fix, playing as `WarFix`; the control of the
    /// promotion.
    PreAbilities,
    /// The game's AI before the war fix (1 November): `Standard` without the war and the
    /// fighting of `WarFix`; the control of the promotion and the base of the variants
    /// built on the standard before it.
    PreWarFix,
    /// `Standard` before the science economy: a beaker at the score of the
    /// technologies, orders the rules refuse kept; the game's AI until 19
    /// October.
    Direct,
    /// `Operations` whose production, gold, research and decision on war
    /// one allocator values in points of the final score (M6.2 step 4,
    /// classic/docs/ai-allocator.md); the game's AI before `Standard`.
    Allocator,
    /// `Allocator` that starts no war operation and leaves every war after
    /// its minimum length: the control that tells the allocator's economy
    /// from its wars.
    AllocatorPeace,
    /// `Allocator` whose gold is worth its best use and whose happiness is
    /// worth its effects (`PriceModel::Uses`).
    AllocatorEcon2,
    /// `Allocator` played by the nations' leaders: each one's
    /// personality chooses among near-equal options, its grand strategy
    /// pursues a path to victory (M6.2 step 5), and its wars with
    /// nations end by peace deals it values (step 6). With every
    /// personality neutral it plays as `Allocator` but for captures that
    /// win the game and how its wars end.
    Leaders,
    /// `Leaders` who end their wars alone, as `Allocator` does (`Leaders`
    /// before step 6): the control of peace by value.
    LeadersAlone,
    /// `Leaders` with the prices of `AllocatorEcon2`.
    LeadersEcon2,
    /// `Leaders` that defends by value: the cities in danger, guards sent,
    /// operations recalled and defenders built while they save more than
    /// they cost (M6.2 step 6); `Standard` without its prices.
    Defence,
    /// `Direct` that orders only what the rules accept: no building without
    /// its resource, no unit without its policy (the half of the science
    /// economy that is a repair; the control of the other half).
    Sound,
    /// The M6 planners, the game's AI before `Allocator`.
    Legacy,
    /// `Legacy` with coordinated combat: focus fire, sieges gathered out of
    /// the city's strike, the wounded rotated out (M6.2 step 2).
    Tactics,
    /// `Tactics` with war operations: armies sized for their target,
    /// mustered, built by the cities, marching together; a war only when
    /// an operation can take its city in time (M6.2 step 3).
    Operations,
    /// `Standard` that plans its investments toward an aim (classic/docs/ai-investment.md):
    /// every leader weighs the spaceship at least as the score, the chance of
    /// it grows with the economy it already has (no chicken and egg), a game
    /// without a time victory is judged over an open horizon, and a beaker
    /// is also worth the progress it makes toward the aim.
    Planner,
    /// `Standard` before the open horizon: the rounds left are counted to
    /// the nominal end of the game even when no time victory ends it there
    /// (`value::horizon`); the control of the open horizon, and the AI of the
    /// game until 20 October.
    Closed,
    /// `Standard` without work boats: the land workers improve only the
    /// land (the game's AI before 21 October, the control of
    /// `ai::sea_work`).
    Landwork,
    /// `Previous` with missiles and nuclear weapons, built and fired, and
    /// counted by the war case (`ai::strikes`, classic/docs/ai-strikes.md):
    /// one third of the heavy arms of `Standard`, for attribution.
    Strikes,
    /// `Previous` with paratroopers that jump to their army's staging tile
    /// (`ai::operations`, classic/docs/ai-strikes.md "Landings"): another third.
    Landings,
    /// The game's AI before the heavy arms (23 October): `Standard` without
    /// missiles, nuclear weapons, carriers and paratrooper jumps; the control
    /// of `ai::strikes`, `ai::carrier` and the war case with weapons.
    Previous,
    /// `Standard` that sends its orders unchecked, as before 23 October: the
    /// control of `ai::legal` (a refused production order is chosen again,
    /// a unit order or an advisor's proposal the rules refuse still goes to
    /// the game).
    Unchecked,
    /// `Standard` without its air arm: aircraft stay in their cities
    /// (the game's AI before 21 October, the control of `ai::air`).
    NoAir,
    /// `Standard` scoring fewer candidates (sites, jobs, targets) per
    /// decision: most games diverge from the standard's early, the
    /// strength stays (the bench's noise floor, platform/docs/ai-bench.md).
    Nudged,
    /// `Standard` that fights its battles with the tactical layer
    /// (`ai::battlefield`, `ai::roles`, `ai::plan_search`, `ai::tactical`;
    /// classic/docs/ai-tactical.md): one plan per turn over a battlefield model
    /// with a threat map, roles by unit characteristics, a two-ply beam
    /// search, a planned capture step and withdrawal discipline, instead of
    /// the front's one-turn local strike picker.
    Tactical,
    /// `Standard` that founds no city after its capital: known to be
    /// weaker (the bench's calibration).
    OneCity,
    /// `Standard` whose operations may march as long as the check that admits
    /// them allows (`operations::MARCH_LIMIT` follows `WAR_WEARINESS_ROUNDS`);
    /// the control of the first war fix (classic/docs/ai-war-audit-current.md).
    WarfixMarch,
    /// `Standard` that prices the unrest of a captured city by the size of the
    /// city and the empire's happiness curve (`Prices::captured_city_unrest`);
    /// the second war fix.
    WarfixUnrest,
    /// `Standard` whose standing garrisons beyond what a city in peace needs
    /// count as free units for a march (`operations::free_units`); the third
    /// war fix.
    WarfixGroup,
    /// `Standard` whose defence recalls an operation only for a saving it can make: the
    /// army reaches the city, its net saving beats the worth of the operation by a
    /// margin (`defence::recall`, classic/docs/ai-war-audit-current.md section 3).
    WarfixRecall,
    /// `Standard` before the war fixes (24 October): the march limit of the old
    /// constant and the flat price of a captured city's unrest. The control of
    /// `Standard`'s war fixes and the base the single-fix variants are measured
    /// against (classic/docs/ai-war-audit-current.md).
    Unfixed,
    /// `Standard` that also raids: a small fast group that pillages, plunders and
    /// kills lone units where a conquest does not pay (`ai::raid`,
    /// classic/docs/ai-raids.md).
    Raids,
    /// `Standard` with the old growth line: its cities stop growing as soon as
    /// the empire is unhappy (`citizens::GrowthPolicy::CAUTIOUS`). The control of
    /// the growth line (classic/docs/ai-growth.md).
    CautiousGrowth,
    /// `Standard` that founds cities until its empire is very unhappy (no
    /// `value::Prices::affords_city`), as before 31 October: the control of
    /// content founding (classic/docs/ai-growth.md).
    EagerFounding,
    /// `Standard` whose front ranks the tiles a unit strikes from and
    /// gathers on by the danger net's expected loss there (`ai::danger_net`:
    /// the damage and the chance of death times the unit's worth) instead of
    /// the count of the enemies that can strike it.
    DangerNet,
    /// `Standard` whose settlers value a site worth a city by the site net's
    /// expected population 50 rounds after the founding (`ai::site_net`,
    /// classic/docs/ai-site.md) instead of the planner's land score; the
    /// legality, the reservations, the distance terms and the travel stay.
    SiteNet,
    /// `Tactical` whose plan search reads a member's exposure and the lethal
    /// rule from the danger net at its final tile (`ai::plan_search`)
    /// instead of the sum of the enemy's blows.
    TacticalNet,
    /// `Standard` with the fixes of the defects that kept it from ever taking a
    /// city (`operations::SiegeFixes`): an assault worth its share of its target,
    /// garrisons by each city's danger and a Defend strategy with a hysteresis, a
    /// siege that closes in when its damage beats the city's healing, the chance
    /// of an operation from the siege model and the powers, a recovery that ends.
    Siege,
    /// `Standard` with the first fix of `Siege` alone: an assault is not free.
    SiegeAssault,
    /// `Standard` with the second fix of `Siege` alone: garrisons by danger.
    SiegeGarrison,
    /// `Standard` with the third fix of `Siege` alone: closing in on damage.
    SiegeClose,
    /// `Standard` with the fourth fix of `Siege` alone: the chance by the model.
    SiegeChance,
    /// `Standard` with the fifth fix of `Siege` alone: a recovery that ends.
    SiegeRecover,
    /// `Standard` whose allocator prices a building's conditional, gated and
    /// one-off effects as the planner's flat values, as before 31 October:
    /// the control of `value::Prices::true_effects` (classic/docs/ai-effects.md).
    NaiveEffects,
    /// `Standard` that judges its orders by their effect (`ai::effects`,
    /// classic/docs/ai-effects-checked.md): a walk that left the unit where it
    /// stood is a failed attempt, workers are wanted for the jobs the job
    /// finder offers, and jobs are ranked and kept by the unit's own path.
    EffectsChecked,
    /// `Standard` with the first check of `EffectsChecked` alone: walks.
    EffectsMoves,
    /// `Standard` whose allocator wants a worker for every worked tile
    /// without an improvement, as before 31 October: the control of the
    /// worker demand by the job finder (classic/docs/ai-effects-checked.md).
    GuessedDemand,
    /// `Standard` with the third check of `EffectsChecked` alone: jobs by
    /// path.
    EffectsJobs,
    /// `Standard` without the stops of its own on top of the rules
    /// (`planner_cities::PricedStops`, classic/docs/ai-own-stops.md): new upkeep
    /// while the treasury carries the deficit, a reserve of the deficit only,
    /// missionaries to any city they reach.
    NoOwnStops,
    /// `Standard` with the first price of `NoOwnStops` alone: upkeep.
    NoOwnStopsUpkeep,
    /// `Standard` with the second price of `NoOwnStops` alone: the reserve.
    NoOwnStopsReserve,
    /// `Standard` with the third price of `NoOwnStops` alone: missionaries.
    NoOwnStopsMissionaries,
    /// `Standard` that prices the rules' science levers by what they do for
    /// it (`ai::science_levers`, classic/docs/ai-science-levers.md): a policy
    /// by all its effects, a new branch and a started one on one scale;
    /// specialist slots by their yields and great person points against the
    /// tile; a research agreement by the beakers the rules pay against its
    /// gold.
    ScienceLevers,
    /// `Standard` with the policies of `ScienceLevers` alone.
    SciencePolicies,
    /// `Standard` with the specialists of `ScienceLevers` alone.
    ScienceSpecialists,
    /// `Standard` with the research agreements of `ScienceLevers` alone.
    ScienceAgreements,
    /// `Standard` whose cities and research choose among the allocator's
    /// best candidates by the choice net (`ai::choice_net`,
    /// classic/docs/ai-choice.md): the outcome learned from explored choices.
    ChoiceNet,
    /// `ChoiceNet` for production alone (attribution).
    ChoiceNetProduction,
    /// `ChoiceNet` for research alone (attribution).
    ChoiceNetResearch,
    /// `Standard` whose land military units never strike an enemy in sight:
    /// they hold (`ai::weak_fighter`, the battle bench's calibration).
    FightPassive,
    /// `Standard` whose land military units in sight of an enemy take a
    /// random legal command (`ai::weak_fighter`).
    FightRandom,
    /// `Standard` whose land military units strike the nearest enemy they
    /// can, whatever the odds (`ai::weak_fighter`).
    FightReckless,
    /// `Standard` with the fixes of the combat defects
    /// (`combat::FightFix`, classic/docs/ai-combat-fix.md), a unit's danger
    /// read from the search's capped estimate.
    FightFix,
    /// `FightFix` with a unit's danger read from the danger net.
    FightFixNet,
    /// `Standard` with the first fix of `FightFixNet` alone (the danger read
    /// from the net): the wounded leave the enemy's reach.
    FightFixRetreat,
    /// `Standard` with the second fix alone: strikes valued with the reply.
    FightFixReply,
    /// `Standard` with the third fix alone: focus fire.
    FightFixFocus,
    /// `Standard` with the fourth fix alone: charges by the preview.
    FightFixOdds,
    /// `Standard` that fights from its ground (`siege_tactics::SiegeTactics`,
    /// classic/docs/ai-siege-tactics.md): the front ranks its tiles by the
    /// danger with the ground's defence, a ranged unit steps to its least
    /// exposed firing tile before it shoots, and a unit that outranges a city
    /// bombards it from beyond the city's strike.
    SiegeTactics,
    /// `Standard` with the first switch of `SiegeTactics` alone: the ground.
    SiegeTacticsGround,
    /// `Standard` with the second switch alone: the firing tile.
    SiegeTacticsFiring,
    /// `Standard` with the third switch alone: outranging.
    SiegeTacticsOutrange,
    /// `Standard` with the fourth switch alone: digging in within reach.
    SiegeTacticsFortify,
    /// `FightFixNet` with every switch of `SiegeTactics`.
    SiegeTacticsNet,
    /// `Standard` that builds its spaceship where it is finished soonest
    /// (`victory::space_plan`, classic/docs/ai-science-victory.md): each
    /// space item in the city that finishes it first, before the items the
    /// allocator chose there, a part stuck in a weak city moved, and a
    /// production percent toward some items priced by what it speeds in the
    /// city (`value::SpaceDemand`).
    SpaceFinish,
    /// `Standard` that fights as `FightFixNet` and wars with the fixes of `Siege`, the
    /// capture chance learned from outcomes, a captured city priced by the rules and a
    /// siege's shots worth its city (`operations::SiegeFixes::WAR_FIX`,
    /// classic/docs/ai-war-fix.md).
    WarFix,
    /// `FightFixNet` with the five fixes of `Siege`: the combination alone, the control of
    /// `WarFix`'s own changes.
    WarFixPlain,
    /// `WarFix` with the siege model's chance in place of the learned one.
    WarFixModel,
    /// `WarFix` with a captured city priced as `Standard` prices it.
    WarFixUnpriced,
    /// `WarFix` whose siege shots are worth their damage alone.
    WarFixShots,
    /// `Standard` with the war of `WarFix` alone (its operations, recall, capture chance and
    /// price), fighting as `Standard`: the attribution of the war part.
    WarFixOps,
    /// `WarFix` with the siege tactics (`ai::siege_tactics`, all switches) on top.
    WarFixTactics,
    /// `Standard` whose units use the abilities the rules give them where they pay
    /// (`ai::unit_abilities`, classic/docs/ai-unit-abilities.md): a great person that may
    /// buy a city-state walks to the best one and buys it when the city is worth more
    /// than its other actions, a merchant walks to a city-state for a trade mission
    /// worth more than its improvement, a cargo of exotic goods is sold when it may be.
    UnitAbilities,
    /// `Standard` whose religion is priced by the rules' formulas
    /// (`ai::religion_fix`, classic/docs/ai-religion-fix.md): a missionary goes
    /// to and is bought for the city whose spread pays most (followers, the
    /// majority, the beliefs' yields there, against the way and the strength
    /// lost on it), an inquisitor wins back an own city of another religion
    /// and guards one a foreign missionary nears, each when it pays its faith.
    ReligionFix,
    /// `Standard` that plays the owner's hypothesis of a science victory from
    /// the rules' data (`ai::tall`, classic/docs/ai-science-pace.md): the number
    /// of cities with the most science per research cost, sites valued by the
    /// science buildings whose site conditions they meet, the policy branches
    /// priced by their science, trade routes priced by what they bring.
    /// Promoted to `Standard` on 1 November (the sixth promotion); kept under its
    /// name as an alias that plays as `Standard` did then: as `PreSiege` since the
    /// seventh promotion.
    TallScience,
    /// `Standard` that plays the same hypothesis literally (`ai::tall`,
    /// `Tall::Book`): exactly four cities, the growth branch then the science
    /// branch, sites next to a mountain, the science buildings in the book's
    /// order, internal routes, sea first. **A league opponent and an
    /// upper-bound probe, never a candidate for `Standard`: the only variant
    /// whose code names content.**
    TallBook,
    /// `TallBook` without its order of the science buildings: the attribution
    /// of the book's gain (a probe, content named as in `TallBook`).
    TallBookNoOrder,
    /// `Standard` with the book's exactly four cities alone (`Tall::BookCities`):
    /// the attribution of the book's gain to its number of cities.
    TallBookCities,
    /// `Standard` with the book's policies alone (`Tall::BookPolicies`): the
    /// attribution of the book's gain to its policy branches.
    TallBookPolicies,
    /// The book's second edition (`Tall::Book2`, `tall::BookPlan::SECOND`,
    /// classic/docs/ai-science-audit.md): `TallBook` with a scout before its
    /// first settler and, once the science branch is whole, the chosen
    /// ideology's tenets before any new branch. A league opponent, content
    /// named as in `TallBook`.
    TallBook2,
    /// `TallBook2` that takes a filler branch by the science lens while the
    /// science branch waits for its era (`Tall::Book2Filler`): a probe of
    /// the filler against saving the culture.
    TallBook2Filler,
    /// `Standard` whose cities order the building with a science effect that
    /// adds the most beakers per hammer before the allocator's choice, while
    /// no settler is wanted (`tall::BuildingOrder::Science`, from the rules'
    /// data): the book's building order without the book.
    ScienceFirst,
    /// `NET_BASE` (the game's AI) whose unit commands, research, production per
    /// city and policies are chosen by the end-to-end net (`ai::net_e2e::mind`,
    /// classic/docs/ai-neural-architecture.md, Playing) from the weights file
    /// named by `AURELIA_NET_WEIGHTS`; the scripted mind of `NET_BASE` does the rest.
    Net,
    /// LEAGUE OPPONENT, never a candidate for the standard: `Standard` playing the
    /// book of the ancient siege rush (`ai::books`, classic/docs/ai-books.md).
    RushSiege,
    /// LEAGUE OPPONENT: `Standard` playing the archer rush timed with Construction.
    RushArcher,
    /// LEAGUE OPPONENT: `Standard` playing the swordsman rush, Iron Working first.
    RushSword,
    /// LEAGUE OPPONENT: `Standard` massing its strongest early or medieval unique unit.
    TimingUu,
    /// LEAGUE OPPONENT: `Standard` playing Liberty and rapid expansion.
    RexWide,
    /// `Standard` with the siege of walled cities (`ai::siege_walls`, also played by
    /// the books): the operation members marching at war press the siege as the
    /// books' attacks do (`books::press_siege`), and the group near a city gathers
    /// out of its reach and strikes together, choosing the city worth most a round
    /// of war and waiting only for the members its measured arrival brings.
    /// Promoted to `Standard` on 1 November (the seventh promotion); kept under its
    /// name as an alias that plays as `Standard` did then: as `PrePuppet` since the
    /// eighth promotion.
    SiegeWalls,
    /// `Standard` that decides a captured city by its price (`ai::conquest`,
    /// `Conquest::Priced`): annexed, a puppet or razed, whichever is worth most over
    /// the rest of the game at the allocator's prices, from the rules' data
    /// (classic/docs/ai-conquest-payoff.md "The priced decision"). A candidate.
    PuppetPrice,
    /// `Standard` that makes every captured city a puppet (`Conquest::PuppetAll`,
    /// Unciv's AI at the capture): the control of `PuppetPrice`. Promoted to `Standard`
    /// on 2 November (the eighth promotion); kept under its name as an alias that plays
    /// as `Standard` did then: as `PreJoin` since the ninth promotion.
    PuppetAll,
    /// `Standard` whose wars price a capture as the puppet it makes of it
    /// (`SiegeFixes::puppet`, `operations::captured_parts`): no resistance before the
    /// city yields and a normal city's unrest, where `Standard` still prices a city of
    /// 4+ citizens as annexed (classic/docs/ai-conquest-payoff.md "Promoted"). A
    /// candidate, built on `PreJoin` since the ninth promotion.
    PuppetWar,
    /// The AI of Unciv 4.22.10 ported to the classic rules (`ai::unciv`,
    /// classic/docs/ai-unciv.md): a league opponent and a reference, never a
    /// candidate for `Standard`. It plays from the player's own knowledge; the
    /// mechanics Unciv's AI does not play (Brave New World's) stay `standard`'s.
    Unciv,
    /// `Standard` whose land and sea military units, city strikes and
    /// promotions are Unciv's (`ai::unciv`, `UncivPlay::Military`): the hybrid
    /// that measures Unciv's unit automation against the classic fighting.
    UncivMilitary,
    /// `Standard` whose siege counts a military unit at the walls of the war's
    /// objective, without a task or drafted into a forming operation, among the
    /// city's besiegers, whose melee members close in when the walls are out of
    /// reach this turn, and whose shooters fire from tiles with a line of sight
    /// (`siege_walls::SiegeJoin`, classic/docs/ai-siege-walls.md "Why `standard`
    /// fails the training grounds' siege"). Promoted to `Standard` on 2 November
    /// (the ninth promotion); kept under its name as an alias that plays as
    /// `Standard` did then, its decision of a captured city too (before, it annexed a
    /// city of 4+ citizens as `PrePuppet`): as `PreWorkers` since the tenth promotion.
    SiegeJoin,
    /// `Standard` whose city never ends its turn with an empty queue while something is
    /// worth its hammers at the allocator's prices (classic/docs/ai-allocator.md «Idle cities»): when
    /// the allocator finds nothing within its build-time limits, the process the rules open
    /// that the prices value most (by `systems::processes::process_yield`), else the
    /// allocator's best order without the limits. A candidate.
    NoIdleCities,
    /// `Standard` that builds its first recon unit as soon as the ruins it can expect to
    /// claim are worth more than the unit's hammers at the turn's prices (classic/docs/ai-scouting.md
    /// «The opening»: the unknown land near home, the generator's density of ruins, the
    /// rewards the rules give), instead of from the 30th Standard round. A candidate.
    ScoutEarly,
    /// `Standard` whose trade routes fill their slots (`ai::trade`, `TurnBrief::trade_fit`,
    /// classic/docs/ai-trade.md): every city that chooses its order may take a trade unit
    /// while slots are left, a route is priced by what it delivers (an internal food route by
    /// the growth it buys in its destination), kept away from hostile units and camps, a trade
    /// unit with no route moves to the city with the best one, and a technology that opens a
    /// slot is worth its route in research. A candidate.
    TradeFit,
    /// `Standard` whose research is Unciv's (`ai::unciv::civ::research`, the free
    /// technology too): one decision family of Unciv's AI measured inside the game's
    /// AI (`UncivParts`, classic/docs/ai-unciv.md "Unciv's modules inside standard").
    StdUncivResearch,
    /// `Standard` whose policies are Unciv's (`civ::adopt_policy`; an ideology stays
    /// the classic `policy_command`, as in `unciv`).
    StdUncivPolicies,
    /// `Standard` whose settlers choose their sites Unciv's way (`civilian::automate_settler`)
    /// and whose cities build settlers when Unciv's `trainSettler` says
    /// (`construction::train_settler`); the allocator orders none.
    StdUncivSettle,
    /// `Standard` whose cities' production is Unciv's (`construction::automate_cities`,
    /// trade units by `ai::trade` as in `unciv`); the settlers stay the allocator's.
    StdUncivConstruction,
    /// `Standard` whose workers and work boats are Unciv's (`worker::automate_worker`).
    StdUncivWorkers,
    /// `Standard` whose gold is spent Unciv's way (`gold::use_gold`: upgrades,
    /// purchases, city-state gifts and buyouts, tiles); none of the allocator's.
    StdUncivGold,
    /// `Standard` whose religion is Unciv's (`religion`: faith, beliefs, the
    /// prophets, missionaries and inquisitors).
    StdUncivReligion,
    /// `Standard` with the three families of Unciv's that measured level with it
    /// alone: research, gold and religion together.
    StdUncivLevel,
    /// `Standard` whose workers fit the work (`planner_workers::WorkersFit`,
    /// classic/docs/ai-workers.md): a worker more only while the work the rules
    /// still offer in the cities' radius gives each worker its share, a worker
    /// leaves a hostile unit's reach also in the middle of a build, a worker
    /// without a job waits in an own city, and pillaged improvements are
    /// repaired. Promoted to `Standard` on 2 November (the tenth promotion); kept
    /// under its name as an alias that plays as `Standard` did then: as `PreCamps` since
    /// the eleventh promotion.
    WorkersFit,
    /// `Standard` whose cities order the land unit kind that fits what it is for by the
    /// rules' combat against the foes it sees (a garrison or a defender: the duel with them;
    /// an army's melee and ranged members: the duel and the walls, per their price), and
    /// whose armies take ranged escorts and, under the Defend strategy, half their units
    /// ranged (`ai::army_fit`, classic/docs/ai-army.md). Since the eleventh promotion
    /// (2 November) `Standard` carries it; `ArmyFit` stays as measured: `PreCamps` with its
    /// switch, i.e. the new `Standard` without the camps.
    ArmyFit,
    /// `Standard` whose free military units clear the barbarian camps near its cities when
    /// the camp's gold and the losses it would cause are worth more than the group's risk,
    /// and whose cities train the group when none is free and it pays (`ai::camps`,
    /// classic/docs/ai-army.md). Since the eleventh promotion (2 November) `Standard`
    /// carries it; `Camps` stays as measured: `PreCamps` with its switch, i.e. the new
    /// `Standard` without the unit kinds of `ArmyFit`.
    Camps,
    /// `ArmyFit` and `Camps` together. Promoted to `Standard` on 2 November (the eleventh
    /// promotion); kept under its name as an alias that plays as `Standard`.
    ArmyCamps,
    /// `Standard` whose promotions are priced by their effect in the fights in view
    /// (`ai::promo_fit`, `combat::Promotions::Priced`, classic/docs/ai-combat-fix.md
    /// «Promotions»): every promotion, the instant heal too, by what it changes in the
    /// unit's strikes and in the foes' strikes on it, read from the rules' previews with
    /// the promotion applied, at the unit's decision before it moves and at the end of the
    /// turn. A candidate.
    PromoFit,
    /// `Standard` that takes the instant heal before a unit moves and strikes when
    /// the healed unit takes a city or kills a unit this turn that it does not take
    /// without it (`combat::Promotions::HealFirst`); its other promotions stay
    /// `Standard`'s. A probe.
    HealFirst,
    /// `Standard` whose armies march through the land they have never seen
    /// (`planner_units::fog_approach`, classic/docs/ai-books.md «The march into the
    /// fog»): where the known map has no way to the target, a member walks to the edge
    /// of the fog on the way that crosses the unseen tiles at the rules' mean price of a
    /// land step, instead of standing where the known way ends. A candidate; a book on
    /// it, `book@fog-march`, marches so too.
    FogMarch,
    /// `Standard` whose research, policies and choice among the allocator's best orders
    /// are made by the hand-written strategy net (`ai::strategy_net`,
    /// classic/docs/ai-strategy-net.md): concepts of the situation, an intent held with
    /// hysteresis, shadow prices of the intent, an economy cell that runs the empire
    /// forward by the rules' growth, 3–5 goals chosen by their marginal value, and the
    /// heads that score every candidate by prices · effects over its turns. Promoted to
    /// `Standard` on 3 November (the twelfth promotion); kept under its name as an alias that
    /// plays as `Standard`.
    StrategyNet,
    /// `Standard` whose tactical and operations layer is the hand-weighted unit net
    /// (`ai::unit_net`, classic/docs/ai-unit-net.md): a commander that runs the
    /// operations' phases by a Lanchester head and plans each turn's fire (target values
    /// with the key target, greedy allocation by marginal value, cover, the enemy's reply
    /// checked) and picks the next unit after each action (`AiMind::next_unit`), and a
    /// tactician that scores each unit's candidates by thirteen motives of its role and
    /// phase over 24 channels and 14 hand-convolved maps. A candidate.
    UnitNet,
    /// `StrategyNet` with the unit net (`UnitNet`'s commander and tactician): the
    /// strategy net's posture sets the unit net's prices and risk. A candidate.
    Hybrid,
    /// The game's AI before the twelfth promotion (3 November): `Standard` without the
    /// strategy net, its research, policies and choice among the allocator's best orders
    /// scripted. Kept for the bench (`experimental-ai` only) as the control of the promotion.
    StandardScripted,
    /// `Standard` before the city-state gift cap (`Prices::gift_cap`, `city_states::gift_ceiling`):
    /// the gold gifts go on to a city-state whose alliance holds past the next gift. Kept for
    /// the bench (`experimental-ai` only) as the control of that change.
    PreCsGifts,
}

/// The net's base: the expert the end-to-end net learns from and the mind it
/// plays on top of. `Variant::Net` plays exactly as it for everything its
/// heads do not decide (diplomacy, the scripted roles, the citizens …); the
/// exporter (`aurelia_qa::export`) records the expert's games with it by
/// default and reads every row with it, so the rows of the data are the rows
/// of play (classic/docs/ai-net-data.md). The game's AI from 1 November
/// (before: `PreWarFix`, the standard before the war fix) to the eighth promotion
/// (2 November): `PrePuppet` since, so that its data and play stay what they were.
pub const NET_BASE: Variant = Variant::PrePuppet;

/// Candidates per decision of `Nudged` (the game's budget: 48). A smaller
/// nudge diverges too few games (measured on `Legacy`: one node less per
/// path search changes none of the standard suite's 80, one candidate less
/// 32, this 73).
const NUDGED_CANDIDATES: u32 = 40;

impl Variant {
    /// The game's AI first.
    pub const ALL: [Variant; 134] = [
        Variant::Standard,
        Variant::PreCamps,
        Variant::PreWorkers,
        Variant::PreJoin,
        Variant::PrePuppet,
        Variant::PreSiege,
        Variant::PreTall,
        Variant::PreAbilities,
        Variant::PreWarFix,
        Variant::Tactical,
        Variant::Previous,
        Variant::Strikes,
        Variant::Landings,
        Variant::Unchecked,
        Variant::Landwork,
        Variant::NoAir,
        Variant::Planner,
        Variant::Closed,
        Variant::Direct,
        Variant::Sound,
        Variant::Allocator,
        Variant::AllocatorPeace,
        Variant::AllocatorEcon2,
        Variant::Leaders,
        Variant::LeadersAlone,
        Variant::LeadersEcon2,
        Variant::Defence,
        Variant::Legacy,
        Variant::Tactics,
        Variant::Operations,
        Variant::Nudged,
        Variant::OneCity,
        Variant::WarfixMarch,
        Variant::WarfixUnrest,
        Variant::WarfixGroup,
        Variant::WarfixRecall,
        Variant::Unfixed,
        Variant::Raids,
        Variant::CautiousGrowth,
        Variant::EagerFounding,
        Variant::DangerNet,
        Variant::SiteNet,
        Variant::TacticalNet,
        Variant::Siege,
        Variant::SiegeAssault,
        Variant::SiegeGarrison,
        Variant::SiegeClose,
        Variant::SiegeChance,
        Variant::SiegeRecover,
        Variant::NaiveEffects,
        Variant::EffectsChecked,
        Variant::EffectsMoves,
        Variant::GuessedDemand,
        Variant::EffectsJobs,
        Variant::NoOwnStops,
        Variant::NoOwnStopsUpkeep,
        Variant::NoOwnStopsReserve,
        Variant::NoOwnStopsMissionaries,
        Variant::ScienceLevers,
        Variant::SciencePolicies,
        Variant::ScienceSpecialists,
        Variant::ScienceAgreements,
        Variant::ChoiceNet,
        Variant::ChoiceNetProduction,
        Variant::ChoiceNetResearch,
        Variant::FightPassive,
        Variant::FightRandom,
        Variant::FightReckless,
        Variant::FightFix,
        Variant::FightFixNet,
        Variant::FightFixRetreat,
        Variant::FightFixReply,
        Variant::FightFixFocus,
        Variant::FightFixOdds,
        Variant::SiegeTactics,
        Variant::SiegeTacticsGround,
        Variant::SiegeTacticsFiring,
        Variant::SiegeTacticsOutrange,
        Variant::SiegeTacticsFortify,
        Variant::SiegeTacticsNet,
        Variant::SpaceFinish,
        Variant::WarFix,
        Variant::WarFixPlain,
        Variant::WarFixModel,
        Variant::WarFixUnpriced,
        Variant::WarFixShots,
        Variant::WarFixOps,
        Variant::WarFixTactics,
        Variant::UnitAbilities,
        Variant::ReligionFix,
        Variant::TallScience,
        Variant::TallBook,
        Variant::TallBook2,
        Variant::TallBook2Filler,
        Variant::TallBookNoOrder,
        Variant::TallBookCities,
        Variant::TallBookPolicies,
        Variant::ScienceFirst,
        Variant::Net,
        Variant::RushSiege,
        Variant::RushArcher,
        Variant::RushSword,
        Variant::TimingUu,
        Variant::RexWide,
        Variant::SiegeWalls,
        Variant::PuppetPrice,
        Variant::PuppetAll,
        Variant::PuppetWar,
        Variant::Unciv,
        Variant::UncivMilitary,
        Variant::SiegeJoin,
        Variant::TradeFit,
        Variant::NoIdleCities,
        Variant::ScoutEarly,
        Variant::StdUncivResearch,
        Variant::StdUncivPolicies,
        Variant::StdUncivSettle,
        Variant::StdUncivConstruction,
        Variant::StdUncivWorkers,
        Variant::StdUncivGold,
        Variant::StdUncivReligion,
        Variant::StdUncivLevel,
        Variant::WorkersFit,
        Variant::ArmyFit,
        Variant::Camps,
        Variant::ArmyCamps,
        Variant::PromoFit,
        Variant::HealFirst,
        Variant::FogMarch,
        Variant::StrategyNet,
        Variant::UnitNet,
        Variant::Hybrid,
        Variant::StandardScripted,
        Variant::PreCsGifts,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Variant::Standard => "standard",
            Variant::PreCamps => "pre-camps",
            Variant::PreWorkers => "pre-workers",
            Variant::PreJoin => "pre-join",
            Variant::PrePuppet => "pre-puppet",
            Variant::PreSiege => "pre-siege",
            Variant::PreTall => "pre-tall",
            Variant::PreAbilities => "pre-abilities",
            Variant::PreWarFix => "pre-war-fix",
            Variant::Tactical => "tactical",
            Variant::Planner => "planner",
            Variant::Landwork => "landwork",
            Variant::Unchecked => "unchecked",
            Variant::Strikes => "strikes",
            Variant::Landings => "landings",
            Variant::Previous => "previous",
            Variant::NoAir => "no-air",
            Variant::Closed => "closed",
            Variant::Allocator => "allocator",
            Variant::AllocatorPeace => "allocator-peace",
            Variant::AllocatorEcon2 => "allocator-econ2",
            Variant::Leaders => "leaders",
            Variant::LeadersAlone => "leaders-alone",
            Variant::LeadersEcon2 => "leaders-econ2",
            Variant::Defence => "defence",
            Variant::Direct => "direct",
            Variant::Sound => "sound",
            Variant::Legacy => "legacy",
            Variant::Tactics => "tactics",
            Variant::Operations => "operations",
            Variant::Nudged => "nudged",
            Variant::OneCity => "one-city",
            Variant::WarfixMarch => "warfix-march",
            Variant::WarfixUnrest => "warfix-unrest",
            Variant::WarfixGroup => "warfix-group",
            Variant::WarfixRecall => "warfix-recall",
            Variant::Unfixed => "unfixed",
            Variant::Raids => "raids",
            Variant::CautiousGrowth => "cautious-growth",
            Variant::EagerFounding => "eager-founding",
            Variant::DangerNet => "danger-net",
            Variant::SiteNet => "site-net",
            Variant::TacticalNet => "tactical-net",
            Variant::Siege => "siege",
            Variant::SiegeAssault => "siege-assault",
            Variant::SiegeGarrison => "siege-garrison",
            Variant::SiegeClose => "siege-close",
            Variant::SiegeChance => "siege-chance",
            Variant::SiegeRecover => "siege-recover",
            Variant::NaiveEffects => "naive-effects",
            Variant::EffectsChecked => "effects-checked",
            Variant::EffectsMoves => "effects-moves",
            Variant::GuessedDemand => "guessed-demand",
            Variant::EffectsJobs => "effects-jobs",
            Variant::NoOwnStops => "no-own-stops",
            Variant::NoOwnStopsUpkeep => "no-own-stops-upkeep",
            Variant::NoOwnStopsReserve => "no-own-stops-reserve",
            Variant::NoOwnStopsMissionaries => "no-own-stops-missionaries",
            Variant::ScienceLevers => "science-levers",
            Variant::SciencePolicies => "science-policies",
            Variant::ScienceSpecialists => "science-specialists",
            Variant::ScienceAgreements => "science-agreements",
            Variant::ChoiceNet => "choice-net",
            Variant::ChoiceNetProduction => "choice-net-production",
            Variant::ChoiceNetResearch => "choice-net-research",
            Variant::FightPassive => "fight-passive",
            Variant::FightRandom => "fight-random",
            Variant::FightReckless => "fight-reckless",
            Variant::FightFix => "fight-fix",
            Variant::FightFixNet => "fight-fix-net",
            Variant::FightFixRetreat => "fight-fix-retreat",
            Variant::FightFixReply => "fight-fix-reply",
            Variant::FightFixFocus => "fight-fix-focus",
            Variant::FightFixOdds => "fight-fix-odds",
            Variant::SiegeTactics => "siege-tactics",
            Variant::SiegeTacticsGround => "siege-tactics-ground",
            Variant::SiegeTacticsFiring => "siege-tactics-firing",
            Variant::SiegeTacticsOutrange => "siege-tactics-outrange",
            Variant::SiegeTacticsFortify => "siege-tactics-fortify",
            Variant::SiegeTacticsNet => "siege-tactics-net",
            Variant::SpaceFinish => "space-finish",
            Variant::WarFix => "war-fix",
            Variant::WarFixPlain => "war-fix-plain",
            Variant::WarFixModel => "war-fix-model",
            Variant::WarFixUnpriced => "war-fix-unpriced",
            Variant::WarFixShots => "war-fix-shots",
            Variant::WarFixOps => "war-fix-ops",
            Variant::WarFixTactics => "war-fix-tactics",
            Variant::UnitAbilities => "unit-abilities",
            Variant::ReligionFix => "religion-fix",
            Variant::TallScience => "tall-science",
            Variant::TallBook => "tall-book",
            Variant::TallBook2 => "tall-book-2",
            Variant::TallBook2Filler => "tall-book-2-filler",
            Variant::TallBookNoOrder => "tall-book-no-order",
            Variant::TallBookCities => "tall-book-cities",
            Variant::TallBookPolicies => "tall-book-policies",
            Variant::ScienceFirst => "science-first",
            Variant::Net => "net",
            Variant::RushSiege => "rush-siege",
            Variant::RushArcher => "rush-archer",
            Variant::RushSword => "rush-sword",
            Variant::TimingUu => "timing-uu",
            Variant::RexWide => "rex-wide",
            Variant::SiegeWalls => "siege-walls",
            Variant::PuppetPrice => "puppet-price",
            Variant::PuppetAll => "puppet-all",
            Variant::SiegeJoin => "siege-join",
            Variant::PromoFit => "promo-fit",
            Variant::HealFirst => "heal-first",
            Variant::FogMarch => "fog-march",
            Variant::StrategyNet => "strategy-net",
            Variant::UnitNet => "unit-net",
            Variant::Hybrid => "hybrid",
            Variant::StandardScripted => "standard-scripted",
            Variant::PreCsGifts => "pre-cs-gifts",
            Variant::ArmyFit => "army-fit",
            Variant::Camps => "camps",
            Variant::ArmyCamps => "army-camps",
            Variant::WorkersFit => "workers-fit",
            Variant::NoIdleCities => "no-idle-cities",
            Variant::ScoutEarly => "scout-early",
            Variant::TradeFit => "trade-fit",
            Variant::PuppetWar => "puppet-war",
            Variant::Unciv => "unciv",
            Variant::StdUncivResearch => "std-unciv-research",
            Variant::StdUncivPolicies => "std-unciv-policies",
            Variant::StdUncivSettle => "std-unciv-settle",
            Variant::StdUncivConstruction => "std-unciv-construction",
            Variant::StdUncivWorkers => "std-unciv-workers",
            Variant::StdUncivGold => "std-unciv-gold",
            Variant::StdUncivReligion => "std-unciv-religion",
            Variant::StdUncivLevel => "std-unciv-level",
            Variant::UncivMilitary => "unciv-military",
        }
    }

    pub fn from_name(name: &str) -> Option<Variant> {
        Variant::ALL.into_iter().find(|v| v.name() == name && v.compiled())
    }

    /// The variant can be played by this build: the nets and the variants that need them
    /// exist only with `experimental-ai` (the game is built without it), and so does the
    /// bench's control `StandardScripted`. The strategy net is the game's since the twelfth
    /// promotion: `strategy-net`, its alias, is in every build.
    pub fn compiled(self) -> bool {
        cfg!(feature = "experimental-ai")
            || !matches!(
                self,
                Variant::Net | Variant::UnitNet | Variant::Hybrid | Variant::SiteNet | Variant::ChoiceNet | Variant::ChoiceNetProduction | Variant::ChoiceNetResearch | Variant::StandardScripted | Variant::PreCsGifts
            )
    }

    /// The variant whose switches this one plays: `NET_BASE`'s for `Net`, whose
    /// scripted part (everything without a net head) is its base's; `Standard`
    /// for `PreWorkers`, `PreJoin`, `PrePuppet`, the conquest variants (`conquest`) and
    /// `SiegeJoin`; itself for the others. Every switch below reads it, `net`,
    /// `conquest`, `siege_join`, `workers_fit` and `PuppetWar`'s price of a capture
    /// (`war_tuning`) alone the variant.
    fn scripted(self) -> Variant {
        match self {
            // The book's second edition plays the book's switches but for its plan (`tall`).
            Variant::TallBook2 | Variant::TallBook2Filler => Variant::TallBook,
            Variant::Net => NET_BASE.scripted(),
            // `Standard` but for their promotions (`fight_fix`).
            Variant::PromoFit | Variant::HealFirst => Variant::Standard,
            // `Standard` but for its march through the fog (`fog_march`).
            Variant::FogMarch => Variant::Standard,
            // `Standard` (since the twelfth promotion its alias) and `Standard` before it, without
            // the strategy net (`strategy_net`).
            Variant::StrategyNet | Variant::StandardScripted | Variant::PreCsGifts => Variant::Standard,
            // `Standard` but for their tactical and operations layer (`unit_net`) and the
            // hybrid's strategy net.
            Variant::UnitNet | Variant::Hybrid => Variant::Standard,
            // `Standard` but for their workers (`workers_fit`), siege (`siege_join`) or decision of a
            // captured city (and `PuppetWar`'s price of it).
            Variant::PreCamps | Variant::PreWorkers | Variant::PreJoin | Variant::PrePuppet | Variant::PuppetPrice | Variant::PuppetAll | Variant::PuppetWar | Variant::SiegeJoin => Variant::Standard,
            // `Standard` but for their unit kinds and camps (`army_fit`, `camps`).
            Variant::ArmyFit | Variant::Camps | Variant::ArmyCamps => Variant::Standard,
            // `Standard` but for its workers (`workers_fit`).
            Variant::WorkersFit => Variant::Standard,
            // `Standard` but for their idle cities (`no_idle_cities`) and their first scout (`scout_early`).
            Variant::NoIdleCities | Variant::ScoutEarly => Variant::Standard,
            // `Standard` but for its trade routes (`trade_fit`).
            Variant::TradeFit => Variant::Standard,
            // Unciv's AI falls back on `Standard` wherever it plays no part of its own.
            Variant::Unciv | Variant::UncivMilitary => Variant::Standard,
            // One of Unciv's decision families inside `Standard` (`UncivParts`).
            v if v.unciv().parts().any() => Variant::Standard,
            other => other,
        }
    }

    /// How it decides a captured city (`ai::conquest`): a puppet always for the game's
    /// AI since the eighth promotion, its aliases `PuppetAll`, `SiegeJoin` and `WorkersFit`,
    /// `PreJoin` and `PreWorkers` (the game's AI before the ninth and the tenth), the
    /// candidates built on it and `PuppetWar`; by its price
    /// for `PuppetPrice`; `NET_BASE`'s for `Net`; the population threshold of the game's
    /// AI before (`PrePuppet`) for the others.
    pub fn conquest(self) -> crate::ai::conquest::Conquest {
        use crate::ai::conquest::Conquest;
        match self {
            // Unciv's modules inside the game's AI decide a capture as it does.
            v if v.unciv().parts().any() => Variant::Standard.conquest(),
            Variant::Standard | Variant::PreCamps | Variant::PreWorkers | Variant::PreJoin | Variant::PuppetAll | Variant::SiegeJoin | Variant::PuppetWar | Variant::Unciv | Variant::UncivMilitary | Variant::TradeFit | Variant::NoIdleCities | Variant::ScoutEarly | Variant::WorkersFit | Variant::PromoFit | Variant::HealFirst | Variant::FogMarch | Variant::StrategyNet | Variant::UnitNet | Variant::Hybrid | Variant::StandardScripted | Variant::PreCsGifts => Conquest::PuppetAll,
            Variant::ArmyFit | Variant::Camps | Variant::ArmyCamps => Conquest::PuppetAll,
            Variant::PuppetPrice => Conquest::Priced,
            Variant::Net => NET_BASE.conquest(),
            _ => Conquest::Threshold,
        }
    }

    /// The book a league opponent plays (`ai::books`); the mind plays every switch
    /// as `Standard` and the book on top.
    pub fn book(self) -> Option<crate::ai::books::Book> {
        use crate::ai::books::Book;
        match self {
            Variant::RushSiege => Some(Book::RushSiege),
            Variant::RushArcher => Some(Book::RushArcher),
            Variant::RushSword => Some(Book::RushSword),
            Variant::TimingUu => Some(Book::TimingUu),
            Variant::RexWide => Some(Book::RexWide),
            _ => None,
        }
    }

    /// It besieges walled cities (`ai::siege_walls` and the siege press,
    /// `books::press_siege`): the game's AI since the seventh promotion, its alias
    /// `SiegeWalls`, and `Net` on it.
    pub fn siege_walls(self) -> bool {
        matches!(self.scripted(), Variant::Standard | Variant::SiegeWalls) && !self.unciv().on()
    }

    /// The switches of its siege (`siege_walls::SiegeJoin`, `SiegeWalls::joined`):
    /// all of them for the game's AI since the ninth promotion, `PreCamps` and `PreWorkers`
    /// (the game's AI before the eleventh and the tenth), the aliases `SiegeJoin` and `WorkersFit` and the candidates
    /// built on it; none for the others: `PreJoin`, the variants built on it, the
    /// books, `SiegeWalls` and `Net` (its base, `NET_BASE`, has none).
    pub fn siege_join(self) -> crate::ai::siege_walls::SiegeJoin {
        if self.unciv().parts().any() {
            // Unciv's modules inside the game's AI besiege as it does.
            return Variant::Standard.siege_join();
        }
        match self {
            Variant::Standard | Variant::PreCamps | Variant::PreWorkers | Variant::SiegeJoin | Variant::TradeFit | Variant::NoIdleCities | Variant::ScoutEarly | Variant::WorkersFit | Variant::PromoFit | Variant::HealFirst | Variant::FogMarch | Variant::StrategyNet | Variant::UnitNet | Variant::Hybrid | Variant::StandardScripted | Variant::PreCsGifts => crate::ai::siege_walls::SiegeJoin::ALL,
            Variant::ArmyFit | Variant::Camps | Variant::ArmyCamps => crate::ai::siege_walls::SiegeJoin::ALL,
            Variant::Net => NET_BASE.siege_join(),
            _ => crate::ai::siege_walls::SiegeJoin::NONE,
        }
    }

    /// Its cities order the unit kind that fits the purpose and its armies take ranged
    /// escorts (`ai::army_fit`): the game's AI since the eleventh promotion, `ArmyFit`, the
    /// alias `ArmyCamps`, the candidates built on `Standard` (`on_standard`),
    /// `UncivMilitary` and Unciv's modules inside `Standard`; not `PreCamps`, `WorkersFit`,
    /// `Camps` or any variant built on an earlier standard, the books (on `PrePuppet`; a
    /// book on a base, `book@base`, plays the base's), `Net` (`NET_BASE`) or `Unciv`.
    pub fn army_fit(self) -> bool {
        if self.unciv().parts().any() {
            // Unciv's modules inside the game's AI order units as it does.
            return Variant::Standard.army_fit();
        }
        // `UncivMilitary`'s cities and operations are `Standard`'s, its units Unciv's.
        matches!(self, Variant::Standard | Variant::ArmyFit | Variant::ArmyCamps | Variant::UncivMilitary) || self.on_standard()
    }

    /// Its armies march through the land they have never seen (`planner_units::fog_approach`,
    /// `TurnBrief::fog_march`): `FogMarch` alone (and a book on it, `book@fog-march`).
    pub fn fog_march(self) -> bool {
        self == Variant::FogMarch
    }

    /// Its research, policies and choice among the allocator's best orders are the
    /// strategy net's (`ai::strategy_net`, `TurnBrief::strategy`): the game's AI since the
    /// twelfth promotion (3 November), its alias `StrategyNet`, and `Hybrid`. Not the variants
    /// built on an earlier standard, not `StandardScripted` (the standard before it) and not
    /// the other candidates on `Standard`: they stay as measured. Unciv's modules inside
    /// `Standard` (`std-unciv-*`) follow it.
    pub fn strategy_net(self) -> bool {
        if self.unciv().parts().any() {
            // Unciv's modules inside the game's AI play its strategy net but for their family.
            return Variant::Standard.strategy_net();
        }
        matches!(self, Variant::Standard | Variant::StrategyNet | Variant::Hybrid | Variant::PreCsGifts)
    }

    /// The gold gifts stop at a city-state whose alliance holds past the next gift
    /// (`Prices::gift_cap`, `city_states::gift_ceiling`): the game's AI since the thirteenth
    /// change (3 November), its alias `StrategyNet` and `Hybrid`, Unciv's modules inside
    /// `Standard`. Not `PreCsGifts` (the standard before it), not the variants built on an
    /// earlier standard: they stay as measured.
    pub fn cs_gift_cap(self) -> bool {
        if self.unciv().parts().any() {
            return Variant::Standard.cs_gift_cap();
        }
        matches!(self, Variant::Standard | Variant::StrategyNet | Variant::Hybrid)
    }

    /// Its tactical and operations layer is the unit net's (`ai::unit_net`,
    /// `TurnBrief::unit_net`): `UnitNet` and `Hybrid`.
    pub fn unit_net(self) -> bool {
        matches!(self, Variant::UnitNet | Variant::Hybrid)
    }

    /// Its free military units clear the barbarian camps near its cities (`ai::camps`):
    /// the game's AI since the eleventh promotion, `Camps`, the alias `ArmyCamps`, the
    /// candidates built on `Standard` (`on_standard`) and Unciv's modules inside it; not
    /// `UncivMilitary`, whose military units head for camps by Unciv's own rule.
    pub fn camps(self) -> bool {
        if self.unciv().parts().any() {
            return Variant::Standard.camps();
        }
        matches!(self, Variant::Standard | Variant::Camps | Variant::ArmyCamps) || self.on_standard()
    }

    /// The candidates described as `Standard` with one change elsewhere: they carry each
    /// switch `Standard` takes in a promotion (`army_fit`, `camps` since the eleventh).
    fn on_standard(self) -> bool {
        matches!(self, Variant::TradeFit | Variant::NoIdleCities | Variant::ScoutEarly | Variant::PromoFit | Variant::HealFirst | Variant::FogMarch | Variant::StrategyNet | Variant::UnitNet | Variant::Hybrid | Variant::StandardScripted | Variant::PreCsGifts)
    }

    /// How much of the turn Unciv's AI plays (`ai::unciv`).
    pub fn unciv(self) -> crate::ai::unciv::UncivPlay {
        use crate::ai::unciv::{UncivParts, UncivPlay};
        match self {
            Variant::Unciv => UncivPlay::Full,
            Variant::UncivMilitary => UncivPlay::Military,
            Variant::StdUncivResearch => UncivPlay::Parts(UncivParts { research: true, ..UncivParts::NONE }),
            Variant::StdUncivPolicies => UncivPlay::Parts(UncivParts { policies: true, ..UncivParts::NONE }),
            Variant::StdUncivSettle => UncivPlay::Parts(UncivParts { settle: true, ..UncivParts::NONE }),
            Variant::StdUncivConstruction => UncivPlay::Parts(UncivParts { construction: true, ..UncivParts::NONE }),
            Variant::StdUncivWorkers => UncivPlay::Parts(UncivParts { workers: true, ..UncivParts::NONE }),
            Variant::StdUncivGold => UncivPlay::Parts(UncivParts { gold: true, ..UncivParts::NONE }),
            Variant::StdUncivReligion => UncivPlay::Parts(UncivParts { religion: true, ..UncivParts::NONE }),
            Variant::StdUncivLevel => UncivPlay::Parts(UncivParts { research: true, gold: true, religion: true, ..UncivParts::NONE }),
            _ => UncivPlay::Off,
        }
    }

    /// The variant whose plays this one copies but for the war changes
    /// (`war_tuning`): `PreWarFix` for `Standard`, `PreTall`, `PreAbilities`, the war-fix
    /// variants and `Raids`, `DangerNet`; `Tactical` for `TacticalNet`; for `Net` its
    /// base's (`NET_BASE`).
    fn base(self) -> Variant {
        match self.scripted() {
            Variant::Standard | Variant::PreSiege | Variant::PreTall | Variant::PreAbilities => Variant::PreWarFix,
            Variant::WarfixMarch | Variant::WarfixUnrest | Variant::WarfixGroup | Variant::WarfixRecall | Variant::Unfixed | Variant::Raids | Variant::CautiousGrowth | Variant::EagerFounding | Variant::DangerNet | Variant::NaiveEffects => Variant::PreWarFix,
            Variant::Siege | Variant::SiegeAssault | Variant::SiegeGarrison | Variant::SiegeClose | Variant::SiegeChance | Variant::SiegeRecover => Variant::PreWarFix,
            Variant::SiteNet | Variant::ChoiceNet | Variant::ChoiceNetProduction | Variant::ChoiceNetResearch => Variant::PreWarFix,
            Variant::EffectsChecked | Variant::EffectsMoves | Variant::GuessedDemand | Variant::EffectsJobs => Variant::PreWarFix,
            Variant::NoOwnStops | Variant::NoOwnStopsUpkeep | Variant::NoOwnStopsReserve | Variant::NoOwnStopsMissionaries => Variant::PreWarFix,
            Variant::ScienceLevers | Variant::SciencePolicies | Variant::ScienceSpecialists | Variant::ScienceAgreements => Variant::PreWarFix,
            Variant::FightPassive | Variant::FightRandom | Variant::FightReckless => Variant::PreWarFix,
            Variant::FightFix | Variant::FightFixNet | Variant::FightFixRetreat | Variant::FightFixReply | Variant::FightFixFocus | Variant::FightFixOdds => Variant::PreWarFix,
            Variant::SiegeTactics | Variant::SiegeTacticsGround | Variant::SiegeTacticsFiring | Variant::SiegeTacticsOutrange | Variant::SiegeTacticsFortify | Variant::SiegeTacticsNet => Variant::PreWarFix,
            Variant::SpaceFinish | Variant::UnitAbilities | Variant::ReligionFix => Variant::PreWarFix,
            Variant::TallScience | Variant::TallBook | Variant::TallBookNoOrder | Variant::TallBookCities | Variant::TallBookPolicies | Variant::ScienceFirst => Variant::PreWarFix,
            Variant::WarFix | Variant::WarFixPlain | Variant::WarFixModel | Variant::WarFixUnpriced | Variant::WarFixShots | Variant::WarFixOps | Variant::WarFixTactics => Variant::PreWarFix,
            Variant::TacticalNet => Variant::Tactical,
            other => other,
        }
    }

    /// Where its cities stop growing for happiness.
    pub fn growth_policy(self) -> crate::ai::citizens::GrowthPolicy {
        match self.scripted() {
            Variant::CautiousGrowth => crate::ai::citizens::GrowthPolicy::CAUTIOUS,
            _ => crate::ai::citizens::GrowthPolicy::STANDARD,
        }
    }

    /// It founds a city beyond the opening only while the empire stays
    /// content: the game's AI and the instruments built on it, but the
    /// control `EagerFounding`.
    pub fn content_founding(self) -> bool {
        self.is_standard() && self.scripted() != Variant::EagerFounding
    }

    /// Its allocator prices a building's effects by what they change in the
    /// city (`value::Prices::true_effects`): the game's AI and the instruments
    /// built on it, but the control `NaiveEffects`.
    pub fn true_effects(self) -> bool {
        self.is_standard() && self.scripted() != Variant::NaiveEffects
    }

    /// Which of its orders it judges by their effect (`ai::effects`): the
    /// worker demand for the game's AI and the instruments built on it (but
    /// the control `GuessedDemand`), all for `EffectsChecked`, and one more
    /// check for each single-check variant.
    pub fn effect_checks(self) -> crate::ai::effects::EffectChecks {
        use crate::ai::effects::EffectChecks;
        let base = if self.is_standard() && self.scripted() != Variant::GuessedDemand {
            EffectChecks { demand: true, ..EffectChecks::NONE }
        } else {
            EffectChecks::NONE
        };
        match self.scripted() {
            Variant::EffectsChecked => EffectChecks::ALL,
            Variant::EffectsMoves => EffectChecks { moves: true, ..base },
            Variant::EffectsJobs => EffectChecks { jobs: true, ..base },
            _ => base,
        }
    }

    /// The stops of its own it replaces by the price or the risk of the rules
    /// (`planner_cities::PricedStops`): all for `NoOwnStops`, one for each of
    /// its single-price variants.
    pub fn priced_stops(self) -> crate::ai::planner_cities::PricedStops {
        use crate::ai::planner_cities::PricedStops;
        match self.scripted() {
            Variant::NoOwnStops => PricedStops::ALL,
            Variant::NoOwnStopsUpkeep => PricedStops { upkeep: true, ..PricedStops::NONE },
            Variant::NoOwnStopsReserve => PricedStops { reserve: true, ..PricedStops::NONE },
            Variant::NoOwnStopsMissionaries => PricedStops { missionaries: true, ..PricedStops::NONE },
            _ => PricedStops::NONE,
        }
    }

    /// The science levers it prices by what they do (`ai::science_levers`):
    /// all for `ScienceLevers`, one for each of its single-lever variants.
    pub fn science_levers(self) -> crate::ai::science_levers::ScienceLevers {
        use crate::ai::science_levers::ScienceLevers;
        match self.scripted() {
            Variant::ScienceLevers => ScienceLevers::ALL,
            Variant::SciencePolicies => ScienceLevers { policies: true, ..ScienceLevers::NONE },
            Variant::ScienceSpecialists => ScienceLevers { specialists: true, ..ScienceLevers::NONE },
            Variant::ScienceAgreements => ScienceLevers { agreements: true, ..ScienceLevers::NONE },
            _ => ScienceLevers::NONE,
        }
    }

    /// The changes of the war logic this variant carries (`operations::WarTuning`):
    /// `PuppetWar` is `Standard`'s with a capture priced as a puppet.
    pub fn war_tuning(self) -> crate::ai::operations::WarTuning {
        use crate::ai::operations::{SiegeFixes, WarTuning};
        let tuning = self.scripted_war_tuning();
        if self == Variant::PuppetWar {
            return WarTuning { siege: SiegeFixes { puppet: true, ..tuning.siege }, ..tuning };
        }
        tuning
    }

    /// `war_tuning` of the variant whose switches this one plays (`scripted`).
    fn scripted_war_tuning(self) -> crate::ai::operations::WarTuning {
        use crate::ai::operations::{SiegeFixes, WarTuning};
        // The promoted fix (24 October): the march limit. The unrest by steps is measured not worse
        // but loses the conquest at 8 tiles of the acceptance matrix (`the_games_ai_conquers_in_the_matrix`):
        // off in `standard`, on in `raids`, as `raids` was measured.
        const FIXED: WarTuning = WarTuning { march: true, unrest: false, group: false, raids: false, recall: false, siege: crate::ai::operations::SiegeFixes::NONE };
        match self.scripted() {
            Variant::Unfixed => WarTuning::NONE,
            Variant::WarfixMarch => WarTuning { march: true, ..WarTuning::NONE },
            Variant::WarfixUnrest => WarTuning { unrest: true, ..WarTuning::NONE },
            Variant::WarfixGroup => WarTuning { group: true, ..WarTuning::NONE },
            Variant::WarfixRecall => WarTuning { recall: true, ..FIXED },
            Variant::Raids => WarTuning { unrest: true, raids: true, ..FIXED },
            Variant::Siege => WarTuning { siege: SiegeFixes::ALL, ..FIXED },
            Variant::SiegeAssault => WarTuning { siege: SiegeFixes { assault: true, ..SiegeFixes::NONE }, ..FIXED },
            Variant::SiegeGarrison => WarTuning { siege: SiegeFixes { garrison: true, ..SiegeFixes::NONE }, ..FIXED },
            Variant::SiegeClose => WarTuning { siege: SiegeFixes { close: true, ..SiegeFixes::NONE }, ..FIXED },
            Variant::SiegeChance => WarTuning { siege: SiegeFixes { chance: true, ..SiegeFixes::NONE }, ..FIXED },
            Variant::SiegeRecover => WarTuning { siege: SiegeFixes { recover: true, ..SiegeFixes::NONE }, ..FIXED },
            // `war-fix` also recalls an operation only for what its members that reach save
            // (`warfix-recall`): 88 % of the operations of `war-fix-plain` ended recalled.
            Variant::Standard | Variant::PreSiege | Variant::PreTall | Variant::PreAbilities | Variant::UnitAbilities | Variant::ReligionFix | Variant::TallScience | Variant::TallBook | Variant::TallBookNoOrder | Variant::TallBookCities | Variant::TallBookPolicies | Variant::ScienceFirst => WarTuning { recall: true, siege: SiegeFixes::WAR_FIX, ..FIXED },
            Variant::WarFix | Variant::WarFixOps | Variant::WarFixTactics => WarTuning { recall: true, siege: SiegeFixes::WAR_FIX, ..FIXED },
            Variant::WarFixPlain => WarTuning { siege: SiegeFixes::ALL, ..FIXED },
            Variant::WarFixModel => WarTuning { recall: true, siege: SiegeFixes { chance: true, learned: false, ..SiegeFixes::WAR_FIX }, ..FIXED },
            Variant::WarFixUnpriced => WarTuning { recall: true, siege: SiegeFixes { priced: false, ..SiegeFixes::WAR_FIX }, ..FIXED },
            Variant::WarFixShots => WarTuning { recall: true, siege: SiegeFixes { worth: false, ..SiegeFixes::WAR_FIX }, ..FIXED },
            Variant::Direct | Variant::Sound => WarTuning::NONE,
            v if v.is_standard() => FIXED,
            _ => WarTuning::NONE,
        }
    }

    /// `Standard` or a bench instrument built on it.
    fn is_standard(self) -> bool {
        let this = self.base();
        matches!(this, Variant::PreWarFix | Variant::Tactical | Variant::Planner | Variant::Landwork | Variant::NoAir | Variant::Closed | Variant::Direct | Variant::Sound | Variant::Nudged | Variant::OneCity | Variant::Unchecked | Variant::Strikes | Variant::Landings | Variant::Previous)
    }

    /// One allocator spends its cities' production and its gold.
    pub(crate) fn allocates(self) -> bool {
        let this = self.base();
        !matches!(this, Variant::Legacy | Variant::Tactics | Variant::Operations)
    }

    /// Its wars are fought by operations (`ai::operations`).
    pub(crate) fn operates(self) -> bool {
        let this = self.base();
        !matches!(this, Variant::Legacy | Variant::Tactics)
    }

    /// Its military units near an enemy decide as one front (`ai::front`).
    pub(crate) fn fights_as_front(self) -> bool {
        let this = self.base();
        this != Variant::Legacy
    }

    /// Its battles are planned once a turn by the tactical layer
    /// (`ai::tactical`), the front deciding what the plan leaves.
    pub fn tactical(self) -> bool {
        self.base() == Variant::Tactical
    }

    /// Its front and its tactical search read the danger net
    /// (`TurnBrief::danger_net`).
    pub fn danger_net(self) -> bool {
        matches!(self.scripted(), Variant::DangerNet | Variant::TacticalNet)
    }

    /// The control fighter its land military units at war fight as
    /// (`ai::weak_fighter`); None: the game's own fighting.
    pub fn weak_fighter(self) -> Option<crate::ai::weak_fighter::WeakFighter> {
        use crate::ai::weak_fighter::WeakFighter;
        match self.scripted() {
            Variant::FightPassive => Some(WeakFighter::Passive),
            Variant::FightRandom => Some(WeakFighter::Random),
            Variant::FightReckless => Some(WeakFighter::Reckless),
            _ => None,
        }
    }

    /// The fixes of the combat defects it carries (`combat::FightFix`,
    /// `TurnBrief::fight`).
    pub fn fight_fix(self) -> crate::ai::combat::FightFix {
        use crate::ai::combat::FightFix;
        use crate::ai::danger::Source;
        match self {
            // `Standard`'s fixes and their own promotions (`ai::promo_fit`).
            Variant::PromoFit => return FightFix { promo: crate::ai::combat::Promotions::Priced, ..Variant::Standard.fight_fix() },
            Variant::HealFirst => return FightFix { promo: crate::ai::combat::Promotions::HealFirst, ..Variant::Standard.fight_fix() },
            _ => {}
        }
        match self.scripted() {
            Variant::FightFix => FightFix::all(Source::Search),
            Variant::FightFixNet => FightFix::all(Source::Net),
            Variant::Standard | Variant::PreSiege | Variant::PreTall | Variant::PreAbilities | Variant::UnitAbilities | Variant::ReligionFix | Variant::TallScience | Variant::TallBook | Variant::TallBookNoOrder | Variant::TallBookCities | Variant::TallBookPolicies | Variant::ScienceFirst => FightFix::all(Source::Net),
            Variant::WarFix | Variant::WarFixPlain | Variant::WarFixModel | Variant::WarFixUnpriced | Variant::WarFixShots | Variant::WarFixTactics => FightFix::all(Source::Net),
            Variant::FightFixRetreat => FightFix { retreat: true, ..FightFix::NONE.read_by(Source::Net) },
            Variant::FightFixReply => FightFix { reply: true, ..FightFix::NONE.read_by(Source::Net) },
            Variant::FightFixFocus => FightFix { focus: true, ..FightFix::NONE.read_by(Source::Net) },
            Variant::FightFixOdds => FightFix { odds: true, ..FightFix::NONE.read_by(Source::Net) },
            Variant::SiegeTacticsNet => FightFix::all(Source::Net),
            _ => FightFix::NONE,
        }
    }

    /// The siege-tactics switches it carries (`siege_tactics::SiegeTactics`,
    /// `TurnBrief::siege`): all for `SiegeTactics` and `SiegeTacticsNet`, one
    /// for each single-switch variant. A variant that fights with them names
    /// them here (`SiegeTactics::ALL`).
    pub fn siege_tactics(self) -> crate::ai::siege_tactics::SiegeTactics {
        use crate::ai::siege_tactics::SiegeTactics;
        match self.scripted() {
            Variant::SiegeTactics | Variant::SiegeTacticsNet | Variant::WarFixTactics => SiegeTactics::ALL,
            Variant::SiegeTacticsGround => SiegeTactics { ground: true, ..SiegeTactics::NONE },
            Variant::SiegeTacticsFiring => SiegeTactics { firing: true, ..SiegeTactics::NONE },
            Variant::SiegeTacticsOutrange => SiegeTactics { outrange: true, ..SiegeTactics::NONE },
            Variant::SiegeTacticsFortify => SiegeTactics { fortify: true, ..SiegeTactics::NONE },
            _ => SiegeTactics::NONE,
        }
    }

    /// How its production and research choose among the allocator's best
    /// (`TurnBrief::choice`): by the choice net for `ChoiceNet` and its two
    /// halves, the allocator's own choice for the others.
    pub fn choice_mode(self) -> crate::ai::choice::ChoiceMode {
        match self.scripted() {
            Variant::ChoiceNet => crate::ai::choice::ChoiceMode::NET,
            Variant::ChoiceNetProduction => crate::ai::choice::ChoiceMode::Net { production: true, research: false },
            Variant::ChoiceNetResearch => crate::ai::choice::ChoiceMode::Net { production: false, research: true },
            _ => crate::ai::choice::ChoiceMode::Off,
        }
    }

    /// Its settlers value the sites by the site net
    /// (`TurnBrief::site_net`).
    pub fn site_net(self) -> bool {
        self.scripted() == Variant::SiteNet
    }

    /// It builds the spaceship where it is finished soonest and prices a
    /// production percent toward some items by what it speeds
    /// (`TurnBrief::space_finish`, `value::Prices::space`).
    pub fn space_finish(self) -> bool {
        self.scripted() == Variant::SpaceFinish
    }

    /// Its units use the abilities the rules give them where they pay
    /// (`ai::unit_abilities`, `TurnBrief::abilities`): the game's AI since the fifth
    /// promotion and `UnitAbilities`.
    pub fn unit_abilities(self) -> bool {
        matches!(self.scripted(), Variant::Standard | Variant::PreSiege | Variant::PreTall | Variant::UnitAbilities | Variant::TallScience | Variant::TallBook | Variant::TallBookNoOrder | Variant::TallBookCities | Variant::TallBookPolicies | Variant::ScienceFirst)
    }

    /// Its cities never end a turn idle while something is worth their hammers
    /// (`TurnBrief::no_idle_cities`): `NoIdleCities` alone (it reads the variant).
    pub fn no_idle_cities(self) -> bool {
        self == Variant::NoIdleCities
    }

    /// It builds its first recon unit when the ruins it expects are worth the unit
    /// (`TurnBrief::scout_early`, `scouting::recon_worth`): `ScoutEarly` alone.
    pub fn scout_early(self) -> bool {
        self == Variant::ScoutEarly
    }

    /// How its workers are wanted and kept (`planner_workers::WorkersFit`,
    /// `TurnBrief::workers`; it reads the variant, not `scripted`): every switch for the
    /// game's AI since the tenth promotion, `PreCamps` (before the eleventh), its alias
    /// `WorkersFit`, the candidates
    /// described as `Standard` with one change elsewhere, `UncivMilitary` (Unciv's
    /// military units on `Standard`) and Unciv's modules inside `Standard`; none for the
    /// others: `PreWorkers` and every variant built on an earlier standard (`SiegeJoin`,
    /// `PreJoin`, the conquest variants, the books, the tall books, `SiegeWalls`), `Net`
    /// (its base, `NET_BASE`, has none), `Unciv` (its workers and cities are Unciv's) and
    /// the bench's instruments.
    pub fn workers_fit(self) -> crate::ai::planner_workers::WorkersFit {
        use crate::ai::planner_workers::WorkersFit;
        if self.unciv().parts().any() {
            // Unciv's modules inside the game's AI keep its workers where they play none of
            // them (`std-unciv-workers`: the allocator's demand for workers).
            return Variant::Standard.workers_fit();
        }
        match self {
            Variant::Standard | Variant::PreCamps | Variant::WorkersFit => WorkersFit::ALL,
            Variant::TradeFit | Variant::NoIdleCities | Variant::ScoutEarly | Variant::ArmyFit | Variant::Camps | Variant::ArmyCamps => WorkersFit::ALL,
            Variant::PromoFit | Variant::HealFirst | Variant::FogMarch | Variant::StrategyNet | Variant::UnitNet | Variant::Hybrid | Variant::StandardScripted | Variant::PreCsGifts => WorkersFit::ALL,
            Variant::UncivMilitary => WorkersFit::ALL,
            Variant::Net => NET_BASE.workers_fit(),
            _ => WorkersFit::NONE,
        }
    }

    /// Its trade routes fill their slots (`ai::trade`, `TurnBrief::trade_fit`):
    /// `TradeFit` alone (it reads the variant, not `scripted`).
    pub fn trade_fit(self) -> bool {
        self == Variant::TradeFit
    }

    /// Its religion is priced by the rules' formulas (`ai::religion_fix`,
    /// `TurnBrief::religion_fix`): the game's AI since the fifth promotion and
    /// `ReligionFix`.
    pub fn religion_fix(self) -> bool {
        matches!(self.scripted(), Variant::Standard | Variant::PreSiege | Variant::PreTall | Variant::ReligionFix | Variant::TallScience | Variant::TallBook | Variant::TallBookNoOrder | Variant::TallBookCities | Variant::TallBookPolicies | Variant::ScienceFirst)
    }

    /// The tall play it carries (`ai::tall`, `TurnBrief::tall`): the game's AI since
    /// the sixth promotion and `TallScience` play the hypothesis from the rules' data.
    pub fn tall(self) -> crate::ai::tall::Tall {
        match self {
            Variant::TallBook2 => return crate::ai::tall::Tall::Book2,
            Variant::TallBook2Filler => return crate::ai::tall::Tall::Book2Filler,
            _ => {}
        }
        match self.scripted() {
            Variant::Standard | Variant::PreSiege | Variant::TallScience => crate::ai::tall::Tall::Science,
            Variant::TallBook | Variant::TallBookNoOrder => crate::ai::tall::Tall::Book,
            Variant::TallBookCities => crate::ai::tall::Tall::BookCities,
            Variant::TallBookPolicies => crate::ai::tall::Tall::BookPolicies,
            _ => crate::ai::tall::Tall::Off,
        }
    }

    /// The science buildings its cities order first (`tall::BuildingOrder`).
    pub fn building_order(self) -> crate::ai::tall::BuildingOrder {
        match self.scripted() {
            Variant::TallBook => crate::ai::tall::BuildingOrder::Book,
            Variant::ScienceFirst => crate::ai::tall::BuildingOrder::Science,
            _ => crate::ai::tall::BuildingOrder::Off,
        }
    }

    /// The end-to-end net chooses its unit commands, research, production
    /// and policies (`ai::net_e2e::mind`).
    pub fn net(self) -> bool {
        self == Variant::Net
    }

    /// Its leaders have personalities and a grand strategy.
    pub(crate) fn has_leaders(self) -> bool {
        let this = self.base();
        self.defends() || matches!(this, Variant::Leaders | Variant::LeadersAlone | Variant::LeadersEcon2)
    }

    /// It values keeping its cities (`ai::defence`).
    pub(crate) fn defends(self) -> bool {
        let this = self.base();
        self.is_standard() || this == Variant::Defence
    }

    /// How its allocator prices gold and happiness.
    pub fn price_model(self) -> PriceModel {
        let this = self.base();
        if self.is_standard() || matches!(this, Variant::AllocatorEcon2 | Variant::LeadersEcon2) {
            PriceModel::Uses
        } else {
            PriceModel::Tiles
        }
    }

    /// It orders only what the rules accept (`TurnBrief::sound_orders`).
    pub fn sound_orders(self) -> bool {
        let this = self.base();
        matches!(this, Variant::PreWarFix | Variant::Tactical | Variant::Planner | Variant::Landwork | Variant::NoAir | Variant::Closed | Variant::Sound | Variant::Nudged | Variant::OneCity | Variant::Unchecked | Variant::Strikes | Variant::Landings | Variant::Previous)
    }

    /// Every order it sends passed the rules' own check (`ai::legal`): all
    /// the variants built on `Standard` but `Unchecked`, its control, and the
    /// ones before the science economy.
    pub fn checks_orders(self) -> bool {
        let this = self.base();
        self.sound_orders() && this != Variant::Unchecked
    }

    /// It builds and fires missiles and nuclear weapons (`ai::strikes`).
    pub fn strikes(self) -> bool {
        let this = self.base();
        matches!(this, Variant::PreWarFix | Variant::Tactical | Variant::Strikes)
    }

    /// It builds and sails carriers for the air arm and the weapons
    /// (`ai::carrier`).
    pub fn carriers(self) -> bool {
        let this = self.base();
        matches!(this, Variant::PreWarFix | Variant::Tactical)
    }

    /// Its paratroopers jump to their army's staging tile
    /// (`ai::operations`).
    pub fn landings(self) -> bool {
        let this = self.base();
        matches!(this, Variant::PreWarFix | Variant::Tactical | Variant::Landings)
    }

    /// How its allocator prices a beaker.
    pub fn science_pricing(self) -> SciencePricing {
        let this = self.base();
        if this == Variant::Planner {
            SciencePricing::Aim
        } else if this == Variant::Closed {
            SciencePricing::ReachClosed
        } else if matches!(this, Variant::PreWarFix | Variant::Tactical | Variant::Landwork | Variant::NoAir | Variant::Nudged | Variant::OneCity | Variant::Unchecked | Variant::Strikes | Variant::Landings | Variant::Previous) {
            SciencePricing::Reach
        } else {
            SciencePricing::Direct
        }
    }

    /// Its cities build work boats for the sea deposits in the borders
    /// (`ai::sea_work`): the game's AI and the variants built on it, but
    /// not their control `Landwork`.
    pub fn sea_work(self) -> bool {
        let this = self.base();
        matches!(this, Variant::PreWarFix | Variant::Tactical | Variant::Planner | Variant::Closed | Variant::NoAir | Variant::Unchecked | Variant::Strikes | Variant::Landings | Variant::Previous)
    }

    /// It builds and commands aircraft (`ai::air`): the game's AI and the
    /// variants built on it, but not their control `NoAir`.
    pub fn air_arm(self) -> bool {
        let this = self.base();
        matches!(this, Variant::PreWarFix | Variant::Tactical | Variant::Planner | Variant::Closed | Variant::Nudged | Variant::OneCity | Variant::Unchecked | Variant::Strikes | Variant::Landings | Variant::Previous)
    }

    /// It plans toward an aim (`ai::grand_strategy`, classic/docs/ai-investment.md).
    pub fn plans(self) -> bool {
        let this = self.base();
        this == Variant::Planner
    }

    /// How it ends its wars with nations: the leaders by deals they
    /// value (`ai::peace`), their control and the variants before them
    /// alone.
    pub fn peace_making(self) -> PeaceMaking {
        let this = self.base();
        if self.has_leaders() && this != Variant::LeadersAlone {
            PeaceMaking::ByDeal
        } else {
            PeaceMaking::Alone
        }
    }

    /// How its operations are admitted; `prices` are the allocator's,
    /// `stakes` the leader's.
    pub(crate) fn war_gate<'p>(self, prices: Option<&'p Prices>, stakes: &'p Stakes) -> WarGate<'p> {
        let this = self.base();
        match (this, prices) {
            (Variant::Operations, _) => WarGate::Orders,
            (Variant::AllocatorPeace, _) | (_, None) => WarGate::Never,
            (_, Some(prices)) => WarGate::Value(prices, stakes),
        }
    }

    /// The settings its decisions read, when they differ from the game's.
    pub(crate) fn settings(self, settings: &AiSettings) -> Option<AiSettings> {
        let this = self.base();
        (this == Variant::Nudged).then(|| {
            let mut nudged = settings.clone();
            nudged.budget.candidates = NUDGED_CANDIDATES;
            nudged
        })
    }

    /// Its change of a unit decision.
    pub(crate) fn adjust(self, decision: Decision, brief: &TurnBrief) -> Decision {
        let this = self.base();
        let settles = decision.reason == Reason::FoundCity && !brief.cities.is_empty();
        if this == Variant::OneCity && settles {
            return Decision::refuse(decision.actor, Reason::Idle);
        }
        // The book keeps exactly its cities: a settler left over founds none.
        if self.tall().book_cities() && settles && brief.cities.len() as u32 >= crate::ai::tall::BOOK_CITIES {
            return Decision::refuse(decision.actor, Reason::Idle);
        }
        decision
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every switch a variant carries, as text (`net` aside).
    fn switches(v: Variant) -> String {
        let crate_only = (v.allocates(), v.operates(), v.fights_as_front(), v.has_leaders(), v.defends());
        let public = (v.growth_policy(), v.content_founding(), v.true_effects(), v.effect_checks(), v.priced_stops(), v.science_levers(), v.war_tuning(), v.tactical(), v.danger_net(), v.weak_fighter(), v.fight_fix(), v.siege_tactics());
        let more = (v.choice_mode(), v.site_net(), v.space_finish(), v.unit_abilities(), v.religion_fix(), v.tall(), v.building_order(), v.price_model(), v.sound_orders(), v.checks_orders(), v.strikes(), v.carriers());
        let rest = (v.landings(), v.science_pricing(), v.sea_work(), v.air_arm(), v.plans(), v.peace_making(), v.siege_walls());
        format!("{crate_only:?} {public:?} {more:?} {rest:?}")
    }

    /// `Net` is its base but for the net: every scripted switch is `NET_BASE`'s, the game's AI
    /// before the eighth promotion, its decision of a captured city too.
    #[test]
    fn the_net_plays_its_base_but_for_its_heads() {
        assert_eq!(NET_BASE, Variant::PrePuppet, "the net learns from and plays on the game's AI before the eighth promotion");
        assert_eq!(switches(Variant::Net), switches(NET_BASE));
        assert_eq!(Variant::Net.conquest(), NET_BASE.conquest());
        assert_eq!(Variant::Net.siege_join(), NET_BASE.siege_join());
        assert!(Variant::Net.net() && !NET_BASE.net());
        assert_ne!(switches(Variant::Net), switches(Variant::PreWarFix), "no longer the standard before the war fix");
    }

    /// The conquest variants and `pre-puppet` are `Standard` in every switch but the decision of a
    /// captured city; `puppet-war` also prices a capture as a puppet in its wars.
    #[test]
    fn the_conquest_variants_play_standard_but_for_the_capture() {
        use crate::ai::conquest::Conquest;
        for (v, conquest) in [(Variant::PreCamps, Conquest::PuppetAll), (Variant::PreWorkers, Conquest::PuppetAll), (Variant::PreJoin, Conquest::PuppetAll), (Variant::PrePuppet, Conquest::Threshold), (Variant::PuppetPrice, Conquest::Priced), (Variant::PuppetAll, Conquest::PuppetAll)] {
            assert_eq!(switches(v), switches(Variant::Standard), "{}", v.name());
            assert_eq!(v.conquest(), conquest);
            assert_eq!(Variant::from_name(v.name()), Some(v));
        }
        // The eighth promotion: the game's AI decides as `puppet-all`, which stays as its alias.
        assert_eq!(Variant::Standard.conquest(), Conquest::PuppetAll);
        assert_eq!(Variant::ALL[4], Variant::PrePuppet);
        let war = Variant::PuppetWar;
        assert_eq!(war.conquest(), Conquest::PuppetAll);
        assert!(war.war_tuning().siege.puppet && !Variant::Standard.war_tuning().siege.puppet);
        let unpuppeted = crate::ai::operations::WarTuning { siege: crate::ai::operations::SiegeFixes { puppet: false, ..war.war_tuning().siege }, ..war.war_tuning() };
        assert_eq!(unpuppeted, Variant::Standard.war_tuning(), "`puppet-war` is `standard` but for the price of a capture");
        assert_eq!(Variant::from_name("puppet-war"), Some(war));
        // Every variant but those keeps the threshold: their games are unchanged by the promotion.
        let puppets = [Variant::Standard, Variant::PreCamps, Variant::PreWorkers, Variant::PreJoin, Variant::PuppetPrice, Variant::PuppetAll, Variant::SiegeJoin, Variant::PuppetWar, Variant::Unciv, Variant::UncivMilitary, Variant::TradeFit, Variant::NoIdleCities, Variant::ScoutEarly, Variant::WorkersFit, Variant::ArmyFit, Variant::Camps, Variant::ArmyCamps, Variant::PromoFit, Variant::HealFirst, Variant::FogMarch, Variant::StrategyNet, Variant::UnitNet, Variant::Hybrid, Variant::StandardScripted, Variant::PreCsGifts];
        assert!(Variant::ALL.iter().filter(|v| !puppets.contains(v) && !v.unciv().parts().any()).all(|v| v.conquest() == Conquest::Threshold));
    }

    /// The ninth promotion: the game's AI besieges as `siege-join`, which stays as its alias in
    /// everything (its decision of a captured city too); `pre-join` is the standard before it,
    /// and every other variant keeps its siege: none of them carries the switches.
    #[test]
    fn the_game_besieges_as_siege_join_and_pre_join_is_the_standard_before() {
        use crate::ai::siege_walls::SiegeJoin;
        let (standard, alias, before) = (Variant::Standard, Variant::SiegeJoin, Variant::PreJoin);
        assert_eq!(Variant::ALL[3], before);
        assert_eq!(Variant::from_name("pre-join"), Some(before));
        assert_eq!(standard.siege_join(), SiegeJoin::ALL);
        assert_eq!(before.siege_join(), SiegeJoin::NONE);
        for v in [alias, before] {
            assert_eq!(switches(v), switches(standard), "{}", v.name());
            assert_eq!(v.conquest(), standard.conquest(), "{}", v.name());
        }
        assert_eq!(alias.siege_join(), standard.siege_join());
        // Unciv's modules inside `standard` follow it (`the_unciv_hybrids_are_standard_but_for_one_family`).
        let joined: Vec<Variant> = Variant::ALL.into_iter().filter(|v| v.siege_join().any() && !v.unciv().parts().any()).collect();
        assert_eq!(joined, [standard, Variant::PreCamps, Variant::PreWorkers, alias, Variant::TradeFit, Variant::NoIdleCities, Variant::ScoutEarly, Variant::WorkersFit, Variant::ArmyFit, Variant::Camps, Variant::ArmyCamps, Variant::PromoFit, Variant::HealFirst, Variant::FogMarch, Variant::StrategyNet, Variant::UnitNet, Variant::Hybrid, Variant::StandardScripted, Variant::PreCsGifts], "only the game's AI and its alias besiege as `siege-join`");
    }

    /// `trade-fit` is `Standard` in every switch, its siege and its decision of a captured city,
    /// but for its trade routes; no other variant plays them.
    #[test]
    fn trade_fit_is_standard_but_for_its_routes() {
        let v = Variant::TradeFit;
        assert_eq!(switches(v), switches(Variant::Standard));
        assert_eq!(v.siege_join(), Variant::Standard.siege_join());
        assert_eq!(v.conquest(), Variant::Standard.conquest());
        assert_eq!(Variant::from_name("trade-fit"), Some(v));
        let fit: Vec<Variant> = Variant::ALL.into_iter().filter(|v| v.trade_fit()).collect();
        assert_eq!(fit, [v]);
    }

    /// Unciv's modules inside `standard` (`UncivParts`, classic/docs/ai-unciv.md): each
    /// hybrid is `Standard` in every switch, its capture and siege too, with one family of
    /// Unciv's decisions (`std-unciv-level`: the three level ones).
    #[test]
    fn the_unciv_hybrids_are_standard_but_for_one_family() {
        let hybrids: Vec<Variant> = Variant::ALL.into_iter().filter(|v| v.unciv().parts().any()).collect();
        assert_eq!(hybrids.len(), 8);
        for v in hybrids {
            assert_eq!(switches(v), switches(Variant::Standard), "{}", v.name());
            assert_eq!(v.conquest(), Variant::Standard.conquest(), "{}", v.name());
            assert_eq!(v.siege_join(), Variant::Standard.siege_join(), "{}", v.name());
            assert!(!v.unciv().on() && v.unciv().any(), "{}: no Unciv unit play, its order or sight", v.name());
            assert_eq!(v.unciv().parts().count(), if v == Variant::StdUncivLevel { 3 } else { 1 }, "{}", v.name());
            assert!(v.name().starts_with("std-unciv-") && Variant::from_name(v.name()) == Some(v));
        }
    }

    /// `no-idle-cities` and `scout-early` are `Standard` in every switch, their siege and their
    /// decision of a captured city, but for their own switch; no other variant plays it.
    #[test]
    fn the_audit_variants_are_standard_but_for_their_switch() {
        for (v, name) in [(Variant::NoIdleCities, "no-idle-cities"), (Variant::ScoutEarly, "scout-early")] {
            assert_eq!(switches(v), switches(Variant::Standard), "{name}");
            assert_eq!(v.siege_join(), Variant::Standard.siege_join());
            assert_eq!(v.conquest(), Variant::Standard.conquest());
            assert_eq!(Variant::from_name(name), Some(v));
        }
        let idle: Vec<Variant> = Variant::ALL.into_iter().filter(|v| v.no_idle_cities()).collect();
        assert_eq!(idle, [Variant::NoIdleCities]);
        let early: Vec<Variant> = Variant::ALL.into_iter().filter(|v| v.scout_early()).collect();
        assert_eq!(early, [Variant::ScoutEarly]);
    }

    /// The tenth promotion: the game's AI fits its workers as `workers-fit`, which stays as its
    /// alias in everything; `pre-workers` is the standard before it, `Standard` in every switch,
    /// its siege and its decision of a captured city. The candidates described as `Standard` with
    /// one change elsewhere, `unciv-military` and Unciv's modules inside `Standard` carry the
    /// switches with it; every variant built on an earlier standard keeps its workers.
    #[test]
    fn the_game_fits_its_workers_as_workers_fit_and_pre_workers_is_the_standard_before() {
        use crate::ai::planner_workers::WorkersFit;
        let (standard, alias, before) = (Variant::Standard, Variant::WorkersFit, Variant::PreWorkers);
        assert_eq!(Variant::ALL[2], before);
        assert_eq!(Variant::from_name("pre-workers"), Some(before));
        assert_eq!(Variant::from_name("workers-fit"), Some(alias));
        assert_eq!(standard.workers_fit(), WorkersFit::ALL);
        assert_eq!(before.workers_fit(), WorkersFit::NONE);
        for v in [alias, before] {
            assert_eq!(switches(v), switches(standard), "{}", v.name());
            assert_eq!(v.siege_join(), standard.siege_join(), "{}", v.name());
            assert_eq!(v.conquest(), standard.conquest(), "{}", v.name());
        }
        assert_eq!(alias.workers_fit(), standard.workers_fit());
        // `siege-join` plays as the standard of its promotion: `pre-workers`.
        assert_eq!(Variant::SiegeJoin.workers_fit(), before.workers_fit());
        assert_eq!(Variant::Net.workers_fit(), NET_BASE.workers_fit());
        let fit: Vec<Variant> = Variant::ALL.into_iter().filter(|v| v.workers_fit().any() && !v.unciv().parts().any()).collect();
        assert_eq!(fit, [standard, Variant::PreCamps, Variant::UncivMilitary, Variant::TradeFit, Variant::NoIdleCities, Variant::ScoutEarly, alias, Variant::ArmyFit, Variant::Camps, Variant::ArmyCamps, Variant::PromoFit, Variant::HealFirst, Variant::FogMarch, Variant::StrategyNet, Variant::UnitNet, Variant::Hybrid, Variant::StandardScripted, Variant::PreCsGifts]);
        assert!(Variant::ALL.into_iter().filter(|v| v.unciv().parts().any()).all(|v| v.workers_fit() == standard.workers_fit()));
    }

    /// `promo-fit` and `heal-first` are `Standard` in every switch, their siege and their
    /// decision of a captured city, but for their promotions; no other variant changes them.
    #[test]
    fn the_promotion_variants_are_standard_but_for_their_promotions() {
        use crate::ai::combat::{FightFix, Promotions};
        for (v, name, rule) in [(Variant::PromoFit, "promo-fit", Promotions::Priced), (Variant::HealFirst, "heal-first", Promotions::HealFirst)] {
            let fight = v.fight_fix();
            assert_eq!(fight.promo, rule, "{name}");
            assert_eq!(FightFix { promo: Promotions::Ordered, ..fight }, Variant::Standard.fight_fix());
            assert_eq!(v.siege_join(), Variant::Standard.siege_join());
            assert_eq!(v.conquest(), Variant::Standard.conquest());
            assert_eq!(v.scripted(), Variant::Standard);
            assert_eq!(Variant::from_name(name), Some(v));
        }
        let changed: Vec<Variant> = Variant::ALL.into_iter().filter(|v| v.fight_fix().promo != Promotions::Ordered).collect();
        assert_eq!(changed, [Variant::PromoFit, Variant::HealFirst]);
    }

    /// The eleventh promotion: the game's AI orders its units by their purpose and clears the
    /// barbarian camps as `army-camps`, which stays as its alias; `pre-camps` is the standard
    /// before it, `Standard` in every other switch, its siege, workers and decision of a
    /// captured city. `army-fit` and `camps` stay as measured, each `pre-camps` with its
    /// switch; `workers-fit` plays as `pre-camps`. The candidates built on `Standard` and
    /// Unciv's modules inside it carry both switches, `unciv-military` the unit kinds only.
    /// The twelfth promotion (3 November): the game's AI plays the strategy net, `strategy-net`
    /// stays as its alias (in every build); `standard-scripted` is the standard before it,
    /// `Standard` in every switch, its siege, workers, unit kinds, camps and decision of a
    /// captured city, without the net, and exists only for the bench (`experimental-ai`).
    #[test]
    fn strategy_net_is_standard_but_for_its_net() {
        let (standard, alias, before) = (Variant::Standard, Variant::StrategyNet, Variant::StandardScripted);
        for v in [alias, before] {
            assert_eq!(switches(v), switches(standard), "{}", v.name());
            assert_eq!(v.siege_join(), standard.siege_join(), "{}", v.name());
            assert_eq!(v.workers_fit(), standard.workers_fit(), "{}", v.name());
            assert_eq!(v.conquest(), standard.conquest(), "{}", v.name());
            assert!(v.army_fit() && v.camps(), "{}", v.name());
            assert!(!v.unit_net(), "{}", v.name());
        }
        assert!(standard.strategy_net() && alias.strategy_net() && !before.strategy_net());
        assert_eq!(Variant::from_name("strategy-net"), Some(alias));
        assert_eq!(Variant::from_name("standard-scripted"), cfg!(feature = "experimental-ai").then_some(before));
        let on: Vec<Variant> = Variant::ALL.into_iter().filter(|v| v.strategy_net() && !v.unciv().parts().any()).collect();
        assert_eq!(on, [standard, alias, Variant::Hybrid, Variant::PreCsGifts]);
        assert!(Variant::ALL.into_iter().filter(|v| v.unciv().parts().any()).all(|v| v.strategy_net()));
    }

    /// The gold gifts' cap (`cs_gift_cap`) is the game's AI and what is built on its net;
    /// `pre-cs-gifts` is `Standard` without it and exists only for the bench.
    #[test]
    fn the_gift_cap_is_standard_but_for_its_control() {
        let capped: Vec<Variant> = Variant::ALL.into_iter().filter(|v| v.cs_gift_cap() && !v.unciv().parts().any()).collect();
        assert_eq!(capped, [Variant::Standard, Variant::StrategyNet, Variant::Hybrid]);
        assert!(Variant::ALL.into_iter().filter(|v| v.unciv().parts().any()).all(|v| v.cs_gift_cap()));
        let before = Variant::PreCsGifts;
        assert_eq!(switches(before), switches(Variant::Standard));
        assert!(before.strategy_net() && !before.cs_gift_cap());
        assert_eq!(Variant::from_name("pre-cs-gifts"), cfg!(feature = "experimental-ai").then_some(before));
    }

    #[test]
    fn unit_net_is_standard_but_for_its_war_layer() {
        for v in [Variant::UnitNet, Variant::Hybrid] {
            assert_eq!(switches(v), switches(Variant::Standard), "{}", v.name());
            assert_eq!(v.siege_join(), Variant::Standard.siege_join());
            assert_eq!(v.workers_fit(), Variant::Standard.workers_fit());
            assert_eq!(v.conquest(), Variant::Standard.conquest());
            assert!(v.army_fit() && v.camps() && v.unit_net());
        }
        #[cfg(feature = "experimental-ai")]
        assert_eq!(Variant::from_name("unit-net"), Some(Variant::UnitNet));
        #[cfg(feature = "experimental-ai")]
        assert_eq!(Variant::from_name("hybrid"), Some(Variant::Hybrid));
        assert!(!Variant::UnitNet.strategy_net() && Variant::Hybrid.strategy_net());
        let on: Vec<Variant> = Variant::ALL.into_iter().filter(|v| v.unit_net()).collect();
        assert_eq!(on, [Variant::UnitNet, Variant::Hybrid]);
    }

    #[test]
    fn the_game_clears_camps_with_fitted_armies_and_pre_camps_is_the_standard_before() {
        let (standard, alias, before) = (Variant::Standard, Variant::ArmyCamps, Variant::PreCamps);
        assert_eq!(Variant::ALL[1], before);
        assert_eq!(Variant::from_name("pre-camps"), Some(before));
        assert!(standard.army_fit() && standard.camps() && alias.army_fit() && alias.camps());
        assert!(!before.army_fit() && !before.camps());
        for v in [alias, before, Variant::ArmyFit, Variant::Camps, Variant::WorkersFit] {
            assert_eq!(switches(v), switches(standard), "{}", v.name());
            assert_eq!(v.siege_join(), standard.siege_join(), "{}", v.name());
            assert_eq!(v.workers_fit(), standard.workers_fit(), "{}", v.name());
            assert_eq!(v.conquest(), standard.conquest(), "{}", v.name());
        }
        assert!(Variant::ArmyFit.army_fit() && !Variant::ArmyFit.camps());
        assert!(Variant::Camps.camps() && !Variant::Camps.army_fit());
        assert!(!Variant::WorkersFit.army_fit() && !Variant::WorkersFit.camps());
        assert!(!Variant::Net.army_fit() && !Variant::Net.camps() && !NET_BASE.army_fit() && !NET_BASE.camps());
        let fit: Vec<Variant> = Variant::ALL.into_iter().filter(|v| v.army_fit() && !v.unciv().parts().any()).collect();
        assert_eq!(fit, [standard, Variant::UncivMilitary, Variant::TradeFit, Variant::NoIdleCities, Variant::ScoutEarly, Variant::ArmyFit, alias, Variant::PromoFit, Variant::HealFirst, Variant::FogMarch, Variant::StrategyNet, Variant::UnitNet, Variant::Hybrid, Variant::StandardScripted, Variant::PreCsGifts]);
        let camps: Vec<Variant> = Variant::ALL.into_iter().filter(|v| v.camps() && !v.unciv().parts().any()).collect();
        assert_eq!(camps, [standard, Variant::TradeFit, Variant::NoIdleCities, Variant::ScoutEarly, Variant::Camps, alias, Variant::PromoFit, Variant::HealFirst, Variant::FogMarch, Variant::StrategyNet, Variant::UnitNet, Variant::Hybrid, Variant::StandardScripted, Variant::PreCsGifts]);
        assert!(Variant::ALL.into_iter().filter(|v| v.unciv().parts().any()).all(|v| v.army_fit() && v.camps()));
    }
}
