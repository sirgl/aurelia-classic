use kernel::command::KernelCommand;

use crate::ids::{
    CityHandle, ImprovementId, PlayerId, ProductionItem, PromotionId, TechId, TileId, UnitHandle,
};

/// Intent of a player/AI. Bad: SetUnitTile/SetCityPopulation.
/// Good: MoveUnit/AttackUnit/FoundCity.
#[derive(Debug, Clone)]
pub enum Command {
    MoveUnit {
        unit: UnitHandle,
        destination: TileId,
    },
    FoundCity {
        settler: UnitHandle,
    },
    ChooseTechnology {
        player: PlayerId,
        technology: TechId,
    },
    /// Atomically replace the plan with missing prerequisites, then the target.
    PlanResearch { player: PlayerId, target: TechId },
    AttackUnit {
        attacker: UnitHandle,
        defender: UnitHandle,
    },
    /// Siege of a city by an adjacent unit (A4): damage to HP, at 0 a capture
    /// (the owner + territory follow the winner, annexation at once).
    AttackCity {
        attacker: UnitHandle,
        city: CityHandle,
    },
    /// Fortify (A5): +50% defence, healing 15 instead of 10.
    /// Cancelled by movement and attack (a stance, not a permanent state).
    Fortify {
        unit: UnitHandle,
    },
    /// Start construction on an own tile (B1: a worker, N turns in place).
    /// Validation: a builder kind, not busy, land, tech unlocked.
    BuildImprovement {
        unit: UnitHandle,
        improvement: ImprovementId,
    },
    /// Build a road on an own tile (B3: a worker, ROAD_WORK_TURNS turns).
    /// The same validation, plus there is no road yet.
    BuildRoad {
        unit: UnitHandle,
    },
    /// Remove the feature of the worker's tile (forest, jungle, marsh: a
    /// remove build); the removal technology and time come from the
    /// feature, the chop production goes to the tile's city.
    ChopForest {
        unit: UnitHandle,
    },
    /// An order "go there" (a goto): validate is cheap
    /// (bounds + passability of the target), commit makes EXACTLY ONE step
    /// along A* (turn economics are exact, unreachability is Err).
    /// A human clicks a far tile, the AI marches: one mechanic.
    MarchTo {
        unit: UnitHandle,
        destination: TileId,
    },
    DeclareWar {
        player: PlayerId,
        enemy: PlayerId,
    },
    /// Peace: the end of war (A3: before this wars were forever).
    MakePeace {
        player: PlayerId,
        enemy: PlayerId,
    },
    /// Open OWN borders to a guest (a one-way grant).
    /// A foreign grant is needed separately (the AI issues it in the driver pass, the UI by a button).
    OpenBorders {
        host: PlayerId,
        guest: PlayerId,
    },
    /// An order into a city's production queue: append to the tail.
    /// Validation: the city is alive and yours, the kind/building exists, the building is
    /// unlocked, the queue is not full.
    EnqueueProduction {
        player: PlayerId,
        city: CityHandle,
        item: ProductionItem,
    },
    /// Reset the queue (changing the order: clear first, then enqueue).
    ClearProduction {
        player: PlayerId,
        city: CityHandle,
    },
    /// Manual tile on/off (citizen management).
    /// Validation: own city, a ring tile (not the centre).
    ToggleWorkedTile {
        player: PlayerId,
        city: CityHandle,
        tile: TileId,
    },
    /// Buyout of the queue head with gold (hurry): instant completion.
    /// Validation: own city, the queue is non-empty, the treasury covers hurry_price.
    HurryProduction {
        player: PlayerId,
        city: CityHandle,
    },
    /// Buying a territory tile with gold (buy a tile): one tile,
    /// ring <=3 from the city, neutral, adjacent to the border.
    /// Validation: own city, the purchasable_ring rules, the treasury covers it.
    PurchaseTile {
        player: PlayerId,
        city: CityHandle,
        tile: TileId,
    },
    EndTurn {
        player: PlayerId,
    },
    /// Lock a tile so the city works it before any automatic choice, or
    /// unlock it (click a tile on the city screen).
    LockWorkedTile {
        player: PlayerId,
        city: CityHandle,
        tile: TileId,
    },
    /// What the automatic citizen assignment favours.
    SetCityFocus {
        player: PlayerId,
        city: CityHandle,
        focus: crate::world::citizens::CityFocus,
    },
    /// Stop the city's growth, or let it grow again («Avoid Growth»).
    SetAvoidGrowth {
        player: PlayerId,
        city: CityHandle,
        avoid: bool,
    },
    /// Give the city the name at this place of its owner's nation list.
    ChooseCityName {
        player: PlayerId,
        city: CityHandle,
        name: u16,
    },
    /// A great person acts where it stands and is used up (academy,
    /// discovery, hurry, golden age, ...: systems::great_people).
    GreatPersonAction {
        unit: UnitHandle,
        action: crate::derived::great_people::GreatPersonAction,
    },
    /// Hand-assigned specialists of one kind (0 gives them back to the
    /// automatic assignment); at most the city's slots.
    SetSpecialists {
        player: PlayerId,
        city: CityHandle,
        specialist: crate::rules::content::SpecialistId,
        count: u8,
    },
    /// The production process of a city with an empty queue (None: build
    /// the default unit).
    SetProcess {
        player: PlayerId,
        city: CityHandle,
        process: Option<crate::world::city_economy::Process>,
    },
    // ---- combat (systems::combat, classic/docs/classic/rules.md) ----
    /// Ranged attack (or air strike) on the target tile: its city, else its
    /// best defender, else a civilian.
    RangedAttack {
        unit: UnitHandle,
        target: TileId,
    },
    /// A city's ranged strike on a unit within two tiles (once a turn).
    CityStrike {
        city: CityHandle,
        target: TileId,
    },
    /// Siege units set up before a ranged attack (one move point).
    SetUpRangedAttack {
        unit: UnitHandle,
    },
    /// Take a promotion the unit's experience allows.
    Promote {
        unit: UnitHandle,
        promotion: PromotionId,
    },
    /// Upgrade a unit to its class's next unit for gold (in own territory).
    UpgradeUnit {
        unit: UnitHandle,
    },
    /// What a conqueror does with a city it just captured.
    DecideCity {
        player: PlayerId,
        city: CityHandle,
        decision: CityDecision,
    },
    /// Pillage the improvement (or road) the unit stands on.
    Pillage {
        unit: UnitHandle,
    },
    /// Move an aircraft to another own city within twice its range.
    Rebase {
        unit: UnitHandle,
        city: CityHandle,
    },
    /// Sell the exotic cargo (Portugal's Nau): gold and experience by the
    /// distance from the capital, next to a foreign land; the cargo is used.
    SellExoticGoods {
        unit: UnitHandle,
    },
    /// Base an aircraft or missile on an own carrier (aircraft carrier,
    /// missile cruiser, nuclear submarine) within twice its range.
    RebaseToCarrier {
        unit: UnitHandle,
        carrier: UnitHandle,
    },
    /// Fighters attack the best interceptor around the target tile.
    AirSweep {
        unit: UnitHandle,
        target: TileId,
    },
    /// An aircraft or anti-air unit waits to intercept air attacks.
    Intercept {
        unit: UnitHandle,
    },
    /// Detonate a nuclear weapon on the target tile.
    Nuke {
        unit: UnitHandle,
        target: TileId,
    },
    /// A great general builds a citadel on its tile and is consumed.
    BuildCitadel {
        unit: UnitHandle,
    },
    // ---- trade routes (systems::trade, classic/docs/classic/rules.md) ----
    /// A caravan or cargo ship in its city starts a route to `destination`.
    EstablishTradeRoute {
        unit: UnitHandle,
        destination: CityHandle,
        kind: crate::world::trade::RouteKind,
    },
    /// A trade unit moves to another own city at once.
    ChangeTradeHome {
        unit: UnitHandle,
        city: CityHandle,
    },
    /// A combat unit plunders the enemy trade route whose unit is on its tile.
    PlunderTradeRoute {
        unit: UnitHandle,
    },
    /// A land unit flies between two own cities that allow airlifts
    /// (Airport); its turn ends (systems::airlift).
    Airlift {
        unit: UnitHandle,
        city: CityHandle,
    },
    /// A paratrooper (a land unit with a paradrop range) that has not moved
    /// this turn jumps from an own city or fort to a known land tile within
    /// its range; the jump ends its turn (systems::paradrop).
    Paradrop {
        unit: UnitHandle,
        destination: TileId,
    },
    // ---- diplomacy (systems::diplomacy, classic/docs/classic/rules.md) ----
    /// Denounce another nation (30 rounds; embassies close).
    Denounce {
        player: PlayerId,
        target: PlayerId,
    },
    /// Offer a deal; the other side answers with RespondDeal.
    ProposeDeal {
        from: PlayerId,
        to: PlayerId,
        /// (giver, item); a mutual item (pact, agreement, friendship,
        /// peace) is listed once.
        items: Vec<(PlayerId, crate::world::relations::DealItem)>,
    },
    /// Accept or refuse an offer addressed to the player (allowed on any
    /// turn).
    RespondDeal {
        player: PlayerId,
        offer: u32,
        accept: bool,
    },
    /// Propose a resolution (or its repeal) for the next session of the
    /// World Congress; `choice` is a player or a resource id (0: none).
    ProposeResolution {
        player: PlayerId,
        kind: crate::world::congress::ResolutionKind,
        choice: u32,
        repeal: bool,
    },
    /// Put delegates on a proposal of the open session: yes (1) / no (0),
    /// or the player voted for in an election.
    CastVotes {
        player: PlayerId,
        proposal: u8,
        choice: u32,
        delegates: u16,
    },
    /// Send a spy (by index) to a city, as a diplomat to a foreign capital,
    /// or home (None).
    MoveSpy {
        player: PlayerId,
        spy: u8,
        city: Option<CityHandle>,
        diplomat: bool,
    },
    /// Spend a steal right on a technology of the victim.
    StealTechnology {
        player: PlayerId,
        from: PlayerId,
        tech: TechId,
    },
    /// The spy stages a coup in its city-state.
    StageCoup {
        player: PlayerId,
        spy: u8,
    },
    /// Install a spaceship part standing in the owner's capital.
    BuildSpaceshipPart {
        unit: UnitHandle,
    },
    // ---- city-states (systems::city_states, classic/docs/classic/rules.md) ----
    /// Give gold to a city-state for influence.
    GiftGold {
        player: PlayerId,
        city_state: PlayerId,
        gold: u32,
    },
    /// Give a combat unit standing on the city-state's land.
    GiftUnit {
        unit: UnitHandle,
        city_state: PlayerId,
    },
    /// Pay 200 gold for the improvement a resource of the city-state needs.
    GiftImprovement {
        player: PlayerId,
        city_state: PlayerId,
        tile: TileId,
    },
    /// Demand tribute in gold.
    BullyGold {
        player: PlayerId,
        city_state: PlayerId,
    },
    /// Demand a worker as tribute.
    BullyWorker {
        player: PlayerId,
        city_state: PlayerId,
    },
    /// Buy out an allied city-state (Austria's Diplomatic Marriage).
    BuyoutCityState {
        player: PlayerId,
        city_state: PlayerId,
    },
    /// Pledge to protect the city-state (or withdraw the pledge).
    PledgeProtection {
        player: PlayerId,
        city_state: PlayerId,
        pledge: bool,
    },
    /// Unlock a social policy branch: costs one policy (culture or
    /// a free policy) and adopts the branch's opener.
    UnlockPolicyBranch {
        player: PlayerId,
        branch: crate::rules::content::PolicyBranchId,
    },
    /// Adopt a policy of an unlocked branch, or a tenet of the ideology.
    AdoptPolicy {
        player: PlayerId,
        policy: crate::ids::PolicyId,
    },
    /// Choose an ideology (free; past the Industrial era or with three
    /// Factories).
    ChooseIdeology {
        player: PlayerId,
        ideology: crate::rules::content::PolicyBranchId,
    },
    /// Found the player's pantheon with a pantheon belief (pays faith).
    FoundPantheon {
        player: PlayerId,
        belief: crate::ids::BeliefId,
    },
    /// A Great Prophet in one of its owner's cities founds a religion there.
    /// `pantheon` is the pantheon belief when the player has no pantheon.
    FoundReligion {
        unit: UnitHandle,
        religion: crate::rules::content::ReligionId,
        founder: crate::ids::BeliefId,
        follower: crate::ids::BeliefId,
        pantheon: Option<crate::ids::BeliefId>,
        /// A bonus belief (Byzantium).
        bonus: Option<crate::ids::BeliefId>,
    },
    /// A Great Prophet adds a follower and an enhancer belief.
    EnhanceReligion {
        unit: UnitHandle,
        follower: crate::ids::BeliefId,
        enhancer: crate::ids::BeliefId,
    },
    /// The founder adds a Reformation belief (needs a Reformation policy).
    AddReformationBelief {
        player: PlayerId,
        belief: crate::ids::BeliefId,
    },
    /// A missionary or a prophet spreads its religion to the city it stands
    /// in or next to.
    SpreadReligion {
        unit: UnitHandle,
    },
    /// An inquisitor removes other religions from its owner's city.
    RemoveHeresy {
        unit: UnitHandle,
    },
    /// Buy a unit or a building with faith in one of the player's cities.
    PurchaseWithFaith {
        player: PlayerId,
        city: CityHandle,
        item: ProductionItem,
    },
    /// A great person creates its improvement (Holy Site, ...) on its tile.
    CreateGreatImprovement {
        unit: UnitHandle,
    },
    /// A Great Writer, Artist or Musician creates a great work in the
    /// nearest free slot.
    CreateGreatWork {
        unit: UnitHandle,
    },
    /// A Great Writer's Political Treatise (culture).
    WriteTreatise {
        unit: UnitHandle,
    },
    /// A Great Musician's concert in another nation's land (influence).
    PerformConcert {
        unit: UnitHandle,
    },
    /// Adopt the ideology the people prefer (public opinion not content).
    SwitchIdeology {
        player: PlayerId,
    },
    /// An archaeologist starts digging the antiquity site it stands on.
    Excavate {
        unit: UnitHandle,
    },
    /// What the player takes from its finished dig on `tile`.
    ChooseArchaeology {
        player: PlayerId,
        tile: TileId,
        choice: crate::derived::archaeology::ArchaeologyChoice,
    },
    /// Take the next free technology the player is owed (Great Library,
    /// Oxford University, Rationalism, Assyria).
    ChooseFreeTechnology {
        player: PlayerId,
        tech: crate::ids::TechId,
    },
    /// Take the next free great person the player is owed (Liberty,
    /// Leaning Tower of Pisa, the Maya Long Count): it appears in the
    /// capital.
    ChooseFreeGreatPerson {
        player: PlayerId,
        kind: crate::classic::effects::GreatPersonKind,
    },
    /// The unit stands on Ancient Ruins: the player takes one of the three rewards the ruins
    /// offer (`derived::ruins::offers_at`). An AI player is paid when its unit enters.
    ClaimRuins {
        unit: UnitHandle,
        reward: crate::systems::ruins::Reward,
    },
}

/// Choices for a captured city.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CityDecision {
    Annex,
    Puppet,
    Raze,
    /// Give the city back to the player who founded it.
    Liberate,
}

impl Command {
    /// The kernel's command this is (`kernel::command::KernelCommand`);
    /// None for the classic orders.
    pub fn kernel(&self) -> Option<KernelCommand> {
        use KernelCommand as K;
        Some(match *self {
            Command::MoveUnit { unit, destination } => K::MoveUnit { unit, destination },
            Command::MarchTo { unit, destination } => K::MarchTo { unit, destination },
            Command::FoundCity { settler } => K::FoundCity { settler },
            Command::Fortify { unit } => K::Fortify { unit },
            Command::BuildImprovement { unit, improvement } => K::BuildImprovement { unit, improvement },
            Command::BuildRoad { unit } => K::BuildRoad { unit },
            Command::ChopForest { unit } => K::ChopForest { unit },
            Command::ChooseTechnology { player, technology } => K::ChooseTechnology { player, technology },
            Command::PlanResearch { player, target } => K::PlanResearch { player, target },
            Command::EnqueueProduction { player, city, item } => K::EnqueueProduction { player, city, item },
            Command::ClearProduction { player, city } => K::ClearProduction { player, city },
            Command::HurryProduction { player, city } => K::HurryProduction { player, city },
            Command::PurchaseTile { player, city, tile } => K::PurchaseTile { player, city, tile },
            Command::ToggleWorkedTile { player, city, tile } => K::ToggleWorkedTile { player, city, tile },
            Command::LockWorkedTile { player, city, tile } => K::LockWorkedTile { player, city, tile },
            Command::SetCityFocus { player, city, focus } => K::SetCityFocus { player, city, focus },
            Command::SetAvoidGrowth { player, city, avoid } => K::SetAvoidGrowth { player, city, avoid },
            Command::ChooseCityName { player, city, name } => K::ChooseCityName { player, city, name },
            Command::SetSpecialists { player, city, specialist, count } => {
                K::SetSpecialists { player, city, specialist, count }
            }
            Command::EndTurn { player } => K::EndTurn { player },
            _ => return None,
        })
    }
}
