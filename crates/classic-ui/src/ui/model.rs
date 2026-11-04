//! Read-only game data the UI draws, and the typed requests it sends back.
//!
//! `game::Session` builds these from the authoritative `GameState` (observer knowledge only);
//! widgets never see `GameState` and never mutate anything. One frame: model in, requests out.
use classic_rules::command::CityDecision;
use classic_rules::ids::{
    BeliefId, CityHandle, PlayerId, PolicyId, ProductionItem, PromotionId, TechId, TileId,
    UnitHandle,
};
use classic_rules::rules::content::{PolicyBranchId, ReligionId};
use classic_rules::world::congress::ResolutionKind;
use classic_rules::world::relations::DealItem;

pub use super::hud::Action;
pub use super::world_model::*;

pub use client::shell::kit::Availability;
pub use client::shell::module::Outcome;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionState {
    pub action: Action,
    pub availability: Availability,
    /// What the order will do, e.g. "Build Farm (5 turns)".
    pub detail: Option<String>,
}

/// Order panel of the selected own unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitOrders {
    pub unit: UnitHandle,
    pub tile: u32,
    pub name: String,
    pub status: String,
    pub actions: Vec<ActionState>,
    /// Move or Attack waiting for a target click.
    pub targeting: Option<Action>,
    /// Promotions the unit may take now (the Promote button opens them).
    pub promotions: Vec<PromotionChoice>,
}

/// Route of the selected unit to the hovered tile, over explored tiles only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathPreview {
    pub tiles: Vec<u32>,
    /// Number of leading tiles (including the start) reachable this turn.
    pub this_turn: usize,
    pub turns: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeKind {
    Growth,
    Production,
    Research,
    Combat,
    Diplomacy,
    Culture,
    Faith,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub turn: u32,
    pub kind: NoticeKind,
    pub text: String,
    /// Where it happened, when on the map (a click centres the camera there).
    pub tile: Option<u32>,
}

/// What a pool (culture, faith) is being saved for: the next step the rules offer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Goal {
    /// The next policy or branch (culture).
    Policy,
    /// The next pantheon (faith).
    Pantheon,
    /// The Great Prophet that founds, then enhances, a religion (faith).
    Prophet,
    /// A Reformation belief that can be added (faith; it has no price of its own).
    Reformation,
}

/// The pool, the price of the next step and whether it can be taken now. The numbers are the
/// core's own (`derived::culture::next_policy_cost`, `derived::religion::pantheon_cost`,
/// `prophet_cost`): the policy and religion screens read the same.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Saving {
    pub goal: Goal,
    pub have: u32,
    /// 0: the step has no price (Reformation).
    pub cost: u32,
    /// The step can be taken now.
    pub ready: bool,
}

impl Saving {
    /// Culture or faith still missing (0 when the pool covers the price).
    pub fn missing(&self) -> u32 {
        self.cost.saturating_sub(self.have)
    }

    /// Turns until the pool covers the price at `per_turn`, rounded up; None without income.
    pub fn turns(&self, per_turn: u32) -> Option<u32> {
        turns_to(self.missing(), per_turn)
    }
}

/// Turns to save `missing` at `per_turn`, rounded up: 0 when nothing is missing, None when
/// there is no income (shown as a dash).
pub fn turns_to(missing: u32, per_turn: u32) -> Option<u32> {
    match (missing, per_turn) {
        (0, _) => Some(0),
        (_, 0) => None,
        (m, p) => Some(m.div_ceil(p)),
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Economy {
    pub gold: u32,
    pub gold_per_turn: i32,
    pub science: u32,
    pub science_per_turn: u32,
    pub food: u32,
    pub production: u32,
    /// Excess happiness of the empire now.
    pub happiness: i32,
    /// "Golden age · 7 turns" or "Golden age 120/500".
    pub golden_age: String,
    /// Culture pool and its income (policies).
    pub culture: u32,
    pub culture_per_turn: u32,
    /// Faith pool and its income.
    pub faith: u32,
    pub faith_per_turn: u32,
    /// What culture is saved for now (None: nothing to buy, or no city yet).
    pub culture_saving: Option<Saving>,
    /// What faith is saved for now.
    pub faith_saving: Option<Saving>,
    /// No technology is being researched although one can be (the end-turn gate's test).
    pub research_pending: bool,
    /// "2 Iron · 4 Horses": strategic resources free for new units.
    pub resources: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResearchSummary {
    pub name: String,
    pub art: String,
    pub stored: u32,
    pub cost: u32,
    pub turns: Option<u32>,
}

/// Everything the map HUD shows about the running game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HudModel {
    pub turn: u32,
    pub nation: String,
    /// The player's era: "Classical Era".
    pub era: String,
    pub economy: Economy,
    pub research: Option<ResearchSummary>,
    pub orders: Option<UnitOrders>,
    pub path: Option<PathPreview>,
    pub notices: Vec<Notice>,
    pub message: Option<String>,
    pub outcome: Outcome,
    pub can_end_turn: bool,
    /// Nations the player currently sees or is at war with.
    pub rivals: Vec<Rival>,
    /// Odds of the selected unit's attack on the hovered tile.
    pub combat: Option<CombatOdds>,
    /// A captured city waiting for annex, puppet, raze or liberate.
    pub capture: Option<CaptureChoice>,
    /// Ancient Ruins under a unit of the player: a card with the three rewards.
    pub ruins: Option<RuinsChoice>,
    /// The card of the city-state whose city the player clicked on the map.
    pub city_state: Option<CityStateWindow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rival {
    pub player: u8,
    pub name: String,
    pub at_war: bool,
    pub borders_open: bool,
    pub peace: Availability,
    pub open_borders: Availability,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductionOption {
    pub item: ProductionItem,
    pub name: String,
    pub art: String,
    pub kind: &'static str,
    pub cost: u32,
    pub turns: Option<u32>,
    pub availability: Availability,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueEntry {
    pub name: String,
    pub art: String,
    pub cost: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildingEntry {
    pub name: String,
    pub art: String,
    pub effect: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkedTile {
    pub tile: TileId,
    pub label: String,
    pub worked: bool,
    /// The player locked it: worked before any automatic choice.
    pub locked: bool,
    /// Take it off work / give it back.
    pub availability: Availability,
    /// Lock / unlock it.
    pub lock: Availability,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TileOffer {
    pub tile: TileId,
    pub label: String,
    pub price: u32,
    pub availability: Availability,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CityView {
    pub tile: u32,
    pub name: String,
    /// The names of the nation's list no city holds, with their place in it (a rename).
    pub names: Vec<(u16, String)>,
    pub subtitle: String,
    pub population: u32,
    pub growth: String,
    pub food_stored: u32,
    pub food_needed: u32,
    /// (label, value) rows. The first six are always food, production, science, gold,
    /// culture and faith in this order (the compact city panel picks them by position).
    pub yields: Vec<(&'static str, String)>,
    pub production_stored: u32,
    pub production_per_turn: i32,
    /// What actually happens when no order is queued.
    pub idle_production: String,
    pub treasury: u32,
    pub queue: Vec<QueueEntry>,
    pub options: Vec<ProductionOption>,
    pub hurry: (String, Availability),
    pub clear: Availability,
    pub buildings: Vec<BuildingEntry>,
    /// Where the citizens work: tiles, specialists, unemployed.
    pub citizens: String,
    /// Every focus: (focus, current, can switch).
    pub focus: Vec<(classic_rules::world::citizens::CityFocus, bool, Availability)>,
    /// «Avoid Growth»: (on now, can switch).
    pub avoid_growth: (bool, Availability),
    pub specialists: Vec<SpecialistRow>,
    pub great_people: Vec<GreatPersonProgress>,
    /// Production processes for an empty queue: (process, label, current,
    /// can switch).
    pub processes: Vec<(Option<classic_rules::world::city_economy::Process>, String, bool, Availability)>,
    pub tiles: Vec<WorkedTile>,
    pub offers: Vec<TileOffer>,
}

/// One specialist kind of a city: working now, slots, set by hand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecialistRow {
    pub kind: classic_rules::rules::content::SpecialistId,
    pub name: String,
    pub working: u8,
    pub slots: u16,
    pub forced: u8,
    /// Yields of one specialist of this kind.
    pub yields: String,
    pub more: Availability,
    pub fewer: Availability,
}

/// A city's great person points of one kind (whole points).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GreatPersonProgress {
    pub name: String,
    pub points: u32,
    pub threshold: u32,
    pub per_turn: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TechStatus {
    Known,
    Researching,
    Queued(usize),
    Available,
    Locked(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TechUnlock {
    pub name: String,
    pub art: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TechCard {
    pub id: TechId,
    pub name: String,
    pub art: String,
    pub era: String,
    pub cost: u32,
    pub column: usize,
    pub row: usize,
    pub requires: Vec<usize>,
    pub status: TechStatus,
    pub unlocks: Vec<TechUnlock>,
    pub availability: Availability,
    /// Missing prerequisites and target, in the same order as the core planner.
    pub plan: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResearchView {
    pub techs: Vec<TechCard>,
    /// A free grant must use its eligible choices, never a distant research plan.
    pub free_choice: bool,
    pub stored: u32,
    pub per_turn: u32,
}

/// One policy, tenet, opener or finisher on the Civics screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyCard {
    pub id: PolicyId,
    pub name: String,
    /// Policy art id (`tradition/legalism`; openers and finishers use the branch's).
    pub art: String,
    pub text: String,
    /// (column, row) in the branch tree; tenets: (position, tier).
    pub grid: (u8, u8),
    /// Tenet tier (0 for policies).
    pub tier: u8,
    /// Positions in the branch's `policies` that must be adopted first.
    pub requires: Vec<usize>,
    pub adopted: bool,
    pub availability: Availability,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchState {
    /// Unlocked (or the chosen ideology).
    Open,
    /// May be unlocked (or chosen) now.
    Available,
    Locked(String),
}

/// A policy branch or an ideology.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchCard {
    pub id: PolicyBranchId,
    pub name: String,
    pub art: String,
    pub ideology: bool,
    /// "Classical era", ...
    pub era: Option<String>,
    pub state: BranchState,
    pub opener: Option<PolicyCard>,
    pub finisher: Option<PolicyCard>,
    pub policies: Vec<PolicyCard>,
    /// Ideologies: free tenets for choosing it now.
    pub free_tenets: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CivicsView {
    pub culture: u32,
    pub per_turn: u32,
    pub next_cost: u32,
    /// Turns until the pool covers the next policy (None: no culture income).
    pub turns: Option<u32>,
    pub free_policies: u16,
    pub free_tenets: u16,
    /// Normal branches, then ideologies, table order.
    pub branches: Vec<BranchCard>,
}

/// A belief card: name, game text and artwork (an atlas id).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BeliefCard {
    pub id: BeliefId,
    pub name: String,
    pub text: String,
    pub art: String,
    /// "PANTHEON", "FOUNDER", ...
    pub category: &'static str,
}

/// A founded religion as everybody knows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReligionCard {
    pub id: ReligionId,
    pub name: String,
    pub founder: String,
    pub holy_city: String,
    pub cities: u32,
    pub followers: u32,
    pub enhanced: bool,
    pub beliefs: Vec<BeliefCard>,
    pub mine: bool,
}

/// What the religion screen offers the player now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FaithChoice {
    /// Pick a pantheon belief (available or not, with the reason).
    Pantheon(Availability),
    /// A Great Prophet may found a religion: pick a religion, a founder and a
    /// follower belief (and a pantheon belief without a pantheon).
    Found { needs_pantheon: bool },
    /// A Great Prophet may enhance the religion: a follower and an enhancer.
    Enhance,
    /// A Reformation belief may be added.
    Reformation,
    /// Nothing to choose now: the religions of the world.
    Browse(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReligionView {
    pub faith: u32,
    pub per_turn: u32,
    pub pantheon_cost: u32,
    pub prophet_cost: u32,
    pub religions_left: u32,
    pub pantheon: Option<BeliefCard>,
    pub religion: Option<ReligionCard>,
    pub choice: FaithChoice,
    /// Free beliefs of every category, for the pickers.
    pub pantheons: Vec<BeliefCard>,
    pub founders: Vec<BeliefCard>,
    pub followers: Vec<BeliefCard>,
    pub enhancers: Vec<BeliefCard>,
    pub reformations: Vec<BeliefCard>,
    /// Religions nobody founded yet (id, name).
    pub free_religions: Vec<(ReligionId, String)>,
    pub world: Vec<ReligionCard>,
    /// What faith can buy: the first own city where each item can be bought, else the
    /// reason in the capital.
    pub purchases: Vec<FaithOffer>,
}

/// Culture overview of the running game: tourism, influence on every met
/// nation, great works in their slots, public opinion, digs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CultureView {
    pub tourism: u32,
    pub culture_per_turn: u32,
    pub influential_on: u32,
    pub needed_for_victory: u32,
    /// "Freedom · Murmurs (2)" or "No ideology".
    pub ideology: String,
    pub preferred: Option<String>,
    pub switch: Availability,
    pub influence: Vec<InfluenceRow>,
    pub buildings: Vec<WorkBuildingCard>,
    pub digs: Vec<DigCard>,
    /// The kinds the next free great person the player is owed may be
    /// (Liberty, Leaning Tower of Pisa, the Maya Long Count), with names.
    pub free_great_people: Vec<(classic_rules::classic::effects::GreatPersonKind, &'static str)>,
    /// Antiquity sites the player sees.
    pub sites: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InfluenceRow {
    pub nation: String,
    pub level: &'static str,
    /// Influence over its culture of the last 8 turns, percent.
    pub percent: u64,
    pub per_turn: u64,
    pub trend: &'static str,
    pub their_level: &'static str,
}

/// A building with great work slots in one of the player's cities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkBuildingCard {
    pub city: String,
    pub building: String,
    pub art: String,
    pub slot: &'static str,
    /// One entry per slot, preserving the stable work ID across moves and saves.
    pub works: Vec<Option<GreatWorkCard>>,
    /// Theming bonus when the building is themed.
    pub theme: Option<u16>,
}

/// Presentation of an actual work; the core stores class/provenance, not a title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GreatWorkCard {
    pub id: u32,
    pub class: &'static str,
    pub provenance: String,
    pub art: Option<String>,
}

/// A finished dig waiting for the player's choice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DigCard {
    pub tile: TileId,
    pub label: String,
    pub choices: Vec<(classic_rules::derived::archaeology::ArchaeologyChoice, &'static str)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaithOffer {
    pub item: ProductionItem,
    pub name: String,
    pub art: String,
    pub city: String,
    pub city_tile: TileId,
    pub cost: Option<u32>,
    pub availability: Availability,
}

/// Game state the full screens draw. `None` in the showcase, which keeps its demo data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenModel {
    pub city: Option<CityView>,
    pub research: ResearchView,
    /// Built only while the Civics screen is shown.
    pub civics: Option<CivicsView>,
    /// Built only while the Religion screen is shown.
    pub religion: Option<ReligionView>,
    /// Built only while the Culture screen is shown.
    pub culture: Option<CultureView>,
    /// The world screens, each built only while it is shown.
    pub diplomacy: Option<DiplomacyView>,
    pub city_states: Option<CityStatesView>,
    pub congress: Option<CongressView>,
    pub espionage: Option<EspionageView>,
    pub victory: Option<VictoryView>,
    pub game_end: Option<GameEndView>,
    /// The last rejected request, shown on the world screens.
    pub message: Option<String>,
    /// Built only while the overview screens are shown.
    pub overview: Option<crate::ui::overview_model::OverviewView>,
    pub turn: u32,
    pub save_status: Option<String>,
}

/// Typed requests from widgets; the game turns them into core `Command`s.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameRequest {
    Unit(Action),
    EndTurn,
    SelectCity(u32),
    /// The own city on the tile aims its ranged strike (the next map click
    /// picks the target).
    CityStrike(u32),
    Enqueue(ProductionItem),
    /// The selected city takes the name at this place of its nation's list.
    RenameCity(u16),
    ClearQueue,
    Hurry,
    ToggleTile(TileId),
    LockTile(TileId),
    SetFocus(classic_rules::world::citizens::CityFocus),
    /// Stop or resume the selected city's growth.
    SetAvoidGrowth(bool),
    SetSpecialists(classic_rules::rules::content::SpecialistId, u8),
    SetProcess(Option<classic_rules::world::city_economy::Process>),
    BuyTile(TileId),
    Research(TechId),
    Save,
    Load,
    /// Load the newest save, autosave or quick save.
    Continue,
    MakePeace(u8),
    OpenBorders(u8),
    UnlockBranch(PolicyBranchId),
    AdoptPolicy(PolicyId),
    ChooseIdeology(PolicyBranchId),
    FoundPantheon(BeliefId),
    FoundReligion {
        religion: ReligionId,
        founder: BeliefId,
        follower: BeliefId,
        pantheon: Option<BeliefId>,
    },
    EnhanceReligion {
        follower: BeliefId,
        enhancer: BeliefId,
    },
    AddReformation(BeliefId),
    BuyWithFaith {
        city_tile: TileId,
        item: ProductionItem,
    },
    SwitchIdeology,
    ChooseArchaeology {
        tile: TileId,
        choice: classic_rules::derived::archaeology::ArchaeologyChoice,
    },
    /// Take the next owed free great person of this kind.
    ChooseFreeGreatPerson(classic_rules::classic::effects::GreatPersonKind),
    /// The selected unit takes the promotion.
    Promote(PromotionId),
    /// The player's unit on Ancient Ruins takes this reward.
    ClaimRuins(classic_rules::ruins::Reward),
    /// Close the ruins card; the ruins stay for later.
    PostponeRuins,
    /// Close the city-state card opened over the map.
    CloseCityState,
    /// "Open on map" on a city-state's card: the map centres on its city and its card opens over it.
    ShowCityState(PlayerId),
    /// What happens to the captured city on this tile.
    DecideCity {
        tile: TileId,
        decision: CityDecision,
    },
    DeclareWar(PlayerId),
    Denounce(PlayerId),
    ProposeDeal {
        to: PlayerId,
        items: Vec<(PlayerId, DealItem)>,
    },
    RespondDeal {
        offer: u32,
        accept: bool,
    },
    GiftGold {
        city_state: PlayerId,
        gold: u32,
    },
    BullyGold(PlayerId),
    BullyWorker(PlayerId),
    BuyoutCityState(PlayerId),
    Pledge {
        city_state: PlayerId,
        pledge: bool,
    },
    ProposeResolution {
        kind: ResolutionKind,
        choice: u32,
        repeal: bool,
    },
    CastVotes {
        proposal: u8,
        choice: u32,
        delegates: u16,
    },
    MoveSpy {
        spy: u8,
        city: Option<CityHandle>,
        diplomat: bool,
    },
    StealTechnology {
        from: PlayerId,
        tech: TechId,
    },
    StageCoup(u8),
    /// "One more turn" after a victory.
    ContinueAfterVictory,
    /// A trade unit starts a route (the trade panel).
    EstablishTradeRoute {
        unit: classic_rules::ids::UnitHandle,
        destination: CityHandle,
        kind: classic_rules::world::trade::RouteKind,
    },
}
