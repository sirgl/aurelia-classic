//! Separates "is it allowed?" (validate) from "what do we apply?" (commit).
//! Commit still protects the invariants.

use crate::ids::{CityHandle, ImprovementId, PlayerId, ProductionItem, TileId, UnitHandle};

#[derive(Debug, Clone)]
pub enum ResolvedCommand {
    /// A kernel command (`kernel::command::validate_kernel`).
    Kernel(kernel::command::ResolvedKernel),
    /// A planned attack (melee or ranged). Damage is rolled in the commit
    /// from the combat stream (validate has no side effect on RNG, §11).
    Attack(crate::combat::AttackPlan),
    CityStrike(crate::combat::exchange::CityStrikePlan),
    SetUpRangedAttack {
        unit: UnitHandle,
    },
    Promote {
        unit: UnitHandle,
        promotion: crate::ids::PromotionId,
    },
    UpgradeUnit {
        unit: UnitHandle,
        to: crate::ids::UnitKindId,
        price: u32,
    },
    DecideCity {
        player: PlayerId,
        city: CityHandle,
        decision: crate::command::CityDecision,
    },
    Pillage {
        unit: UnitHandle,
        tile: TileId,
    },
    SellExoticGoods {
        unit: UnitHandle,
        /// The promotion that carries the cargo (used up).
        cargo: crate::ids::PromotionId,
        gold: u32,
        experience: u16,
    },
    Rebase {
        unit: UnitHandle,
        to: TileId,
        /// The carrier the unit is based on afterwards (None: a city).
        carrier: Option<crate::ids::EntityId>,
    },
    AirSweep {
        unit: UnitHandle,
        target: TileId,
    },
    Intercept {
        unit: UnitHandle,
    },
    Nuke {
        unit: UnitHandle,
        target: TileId,
    },
    BuildCitadel {
        unit: UnitHandle,
        tile: TileId,
    },
    EstablishTradeRoute {
        unit: UnitHandle,
        destination: CityHandle,
        kind: crate::world::trade::RouteKind,
    },
    ChangeTradeHome {
        unit: UnitHandle,
        city: CityHandle,
    },
    PlunderTradeRoute {
        unit: UnitHandle,
    },
    Airlift {
        unit: UnitHandle,
        city: CityHandle,
    },
    Paradrop {
        unit: UnitHandle,
        to: TileId,
    },
    Denounce {
        player: PlayerId,
        target: PlayerId,
    },
    ProposeDeal {
        from: PlayerId,
        to: PlayerId,
        items: Vec<(PlayerId, crate::world::relations::DealItem)>,
    },
    RespondDeal {
        player: PlayerId,
        offer: u32,
        accept: bool,
    },
    ProposeResolution {
        player: PlayerId,
        kind: crate::world::congress::ResolutionKind,
        choice: u32,
        repeal: bool,
    },
    CastVotes {
        player: PlayerId,
        proposal: u8,
        choice: u32,
        delegates: u16,
    },
    MoveSpy {
        player: PlayerId,
        spy: u8,
        city: Option<CityHandle>,
        diplomat: bool,
    },
    StealTechnology {
        player: PlayerId,
        from: PlayerId,
        tech: crate::ids::TechId,
    },
    StageCoup {
        player: PlayerId,
        spy: u8,
    },
    InstallSpaceshipPart {
        unit: UnitHandle,
        project: crate::rules::content::ProjectId,
    },
    GiftGold {
        player: PlayerId,
        city_state: PlayerId,
        gold: u32,
    },
    GiftUnit {
        unit: UnitHandle,
        city_state: PlayerId,
    },
    GiftImprovement {
        player: PlayerId,
        tile: TileId,
        improvement: ImprovementId,
    },
    BullyGold {
        player: PlayerId,
        city_state: PlayerId,
    },
    BullyWorker {
        player: PlayerId,
        city_state: PlayerId,
    },
    Pledge {
        player: PlayerId,
        city_state: PlayerId,
        pledge: bool,
    },
    BuyoutCityState {
        player: PlayerId,
        city_state: PlayerId,
    },
    DeclareWar {
        player: PlayerId,
        enemy: PlayerId,
    },
    MakePeace {
        player: PlayerId,
        enemy: PlayerId,
    },
    OpenBorders {
        host: PlayerId,
        guest: PlayerId,
    },
    GreatPersonAction {
        unit: UnitHandle,
        action: crate::derived::great_people::GreatPersonAction,
    },
    SetProcess {
        player: PlayerId,
        city: CityHandle,
        process: Option<crate::world::city_economy::Process>,
    },
    UnlockPolicyBranch {
        player: PlayerId,
        branch: crate::rules::content::PolicyBranchId,
    },
    AdoptPolicy {
        player: PlayerId,
        policy: crate::ids::PolicyId,
    },
    ChooseIdeology {
        player: PlayerId,
        ideology: crate::rules::content::PolicyBranchId,
    },
    FoundPantheon {
        player: PlayerId,
        belief: crate::ids::BeliefId,
    },
    FoundReligion {
        unit: UnitHandle,
        religion: crate::rules::content::ReligionId,
        founder: crate::ids::BeliefId,
        follower: crate::ids::BeliefId,
        pantheon: Option<crate::ids::BeliefId>,
        bonus: Option<crate::ids::BeliefId>,
    },
    EnhanceReligion {
        unit: UnitHandle,
        follower: crate::ids::BeliefId,
        enhancer: crate::ids::BeliefId,
    },
    AddReformationBelief {
        player: PlayerId,
        belief: crate::ids::BeliefId,
    },
    SpreadReligion {
        unit: UnitHandle,
    },
    RemoveHeresy {
        unit: UnitHandle,
    },
    PurchaseWithFaith {
        player: PlayerId,
        city: CityHandle,
        item: ProductionItem,
    },
    CreateGreatImprovement {
        unit: UnitHandle,
    },
    CreateGreatWork {
        unit: UnitHandle,
    },
    WriteTreatise {
        unit: UnitHandle,
    },
    PerformConcert {
        unit: UnitHandle,
    },
    SwitchIdeology {
        player: PlayerId,
    },
    Excavate {
        unit: UnitHandle,
    },
    ChooseArchaeology {
        player: PlayerId,
        tile: TileId,
        choice: crate::derived::archaeology::ArchaeologyChoice,
    },
    ChooseFreeTechnology {
        player: PlayerId,
        tech: crate::ids::TechId,
    },
    ChooseFreeGreatPerson {
        player: PlayerId,
        kind: crate::classic::effects::GreatPersonKind,
    },
    ClaimRuins {
        unit: UnitHandle,
        tile: TileId,
        reward: crate::systems::ruins::Reward,
    },
}
