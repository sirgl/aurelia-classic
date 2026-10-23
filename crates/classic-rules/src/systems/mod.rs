//! Gameplay systems: one module per mechanic, each with its own functions
//! and journal entries. `transaction::transaction` only dispatches resolved
//! commands to them (platform/structure.md §13, §48); systems call each other
//! explicitly — no callback or event bus (platform/project.md §10, §56). Entries of
//! the classic mechanics (happiness, golden age, policies, religion,
//! tourism, trade, relations) are the module's `ClassicChange` in
//! `ChangeJournal::module_changes`; the rest are kernel changes.
//!
//! | module          | commands                         | journal                          |
//! |-----------------|----------------------------------|----------------------------------|
//! | movement        | MoveUnit, MarchTo                | UnitChange::Moved                |
//! | combat          | AttackUnit, AttackCity, Fortify  | Damaged/Destroyed, OwnerChanged  |
//! | city_founding   | FoundCity                        | Founded, tile OwnerChanged       |
//! | production      | Enqueue/ClearProduction          | Created, BuildingAdded           |
//! | growth          | —  (turn)                        | PopulationChanged                |
//! | happiness       | —  (turn)                        | HappinessChanged                 |
//! | golden_age      | —  (turn)                        | GoldenAgeChanged                 |
//! | great_people    | GreatPersonAction (+ turn)       | Created, Destroyed, Improvement… |
//! | worked_tiles    | Toggle/LockWorkedTile, focus, …  | WorkedChanged                    |
//! | culture         | —  (turn: border growth)         | tile OwnerChanged                |
//! | policies        | UnlockPolicyBranch, AdoptPolicy, ChooseIdeology (turn: culture pool) | PolicyAdopted |
//! | religion        | FoundPantheon, FoundReligion, EnhanceReligion, AddReformationBelief, SpreadReligion, RemoveHeresy, PurchaseWithFaith, CreateGreatImprovement (turn: faith, prophet; round: pressure) | ReligionChanged |
//! | tourism         | CreateGreatWork, WriteTreatise, PerformConcert, SwitchIdeology (turn: influence, public opinion; FreeGreatWork on completion) | PolicyAdopted (switch), GreatWorksChanged |
//! | ruins           | — (a unit enters Ancient Ruins)  | tile ImprovementBuilt, RuinsExplored |
//! | trade           | EstablishTradeRoute, ChangeTradeHome, PlunderTradeRoute (turn: values, moves, finished routes; war, a city changing hands or razed ends routes) | TradeChanged, Created/Destroyed |
//! | archaeology     | Excavate, ChooseArchaeology (turn, in Tourism: sites appear, digs advance); battles record history | ResourceSet, ImprovementBuilt |
//! | improvements    | BuildImprovement/Road, Chop      | ImprovementBuilt/RoadBuilt/...   |
//! | research        | ChooseTechnology                 | TechnologyChosen                 |
//! | diplomacy       | DeclareWar, MakePeace, OpenBorders | WarDeclared/PeaceMade/...      |
//! | purchase        | HurryProduction, PurchaseTile    | (production), tile OwnerChanged  |
//! | economy         | —  (turn: upkeep, gold)          | —                                |
//! | turn            | EndTurn (ordered step list)      | TurnEnded                        |
//! | effects         | — (triggers of the others)       | Promoted, EffectsChanged, ...    |
//! | elimination     | — (every commit)                 | —                                |

pub(crate) mod airlift;
pub(crate) mod paradrop;
pub(crate) mod archaeology;
pub(crate) mod barbarians;
pub(crate) mod cargo;
pub(crate) mod city_states;
pub(crate) mod combat;
pub(crate) mod congress;
pub(crate) mod diplomacy;
pub(crate) mod economy;
pub(crate) mod effects;
pub(crate) mod golden_age;
pub(crate) mod grants;
pub(crate) mod great_people;
pub(crate) mod espionage;
pub(crate) mod growth;
pub(crate) mod happiness;
pub(crate) mod founding;
pub(crate) mod league_projects;
pub(crate) mod natural_wonders;
pub(crate) mod plunder;
pub(crate) mod processes;
pub(crate) mod purchase;
pub(crate) mod policies;
pub(crate) mod religion;
pub mod ruins;
pub(crate) mod research;
pub(crate) mod tourism;
pub(crate) mod trade;
pub(crate) mod victory;

#[cfg(test)]
#[path = "research_plan_tests.rs"]
mod research_plan_tests;

#[cfg(test)]
mod tests;
