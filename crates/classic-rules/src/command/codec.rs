//! The journal's layout of the classic commands (`kernel::replay::codec`,
//! platform/docs/saves.md §3). A variant keeps its tag for good: a new command takes
//! the next number, a changed one bumps `COMMAND_CODEC_VERSION`.

use kernel::replay::codec::CommandCodec;

use crate::classic::content::{GreatPersonKind, PolicyBranchId, ReligionId};
use crate::classic::Classic;
use crate::derived::archaeology::ArchaeologyChoice;
use crate::derived::great_people::GreatPersonAction;
use crate::ids::{BeliefId, PolicyId};
use crate::systems::ruins::Reward;
use crate::world::congress::ResolutionKind;
use crate::world::relations::DealItem;
use crate::world::trade::RouteKind;

use super::command::{CityDecision, Command};

impl CommandCodec for Classic {
    const COMMAND_CODEC_VERSION: u32 = 1;
}

kernel::command_ids!(PolicyId, BeliefId, PolicyBranchId, ReligionId);

kernel::command_field!(Command {
    0 => MoveUnit { unit, destination },
    1 => FoundCity { settler },
    2 => ChooseTechnology { player, technology },
    3 => PlanResearch { player, target },
    4 => AttackUnit { attacker, defender },
    5 => AttackCity { attacker, city },
    6 => Fortify { unit },
    7 => BuildImprovement { unit, improvement },
    8 => BuildRoad { unit },
    9 => ChopForest { unit },
    10 => MarchTo { unit, destination },
    11 => DeclareWar { player, enemy },
    12 => MakePeace { player, enemy },
    13 => OpenBorders { host, guest },
    14 => EnqueueProduction { player, city, item },
    15 => ClearProduction { player, city },
    16 => ToggleWorkedTile { player, city, tile },
    17 => HurryProduction { player, city },
    18 => PurchaseTile { player, city, tile },
    19 => EndTurn { player },
    20 => LockWorkedTile { player, city, tile },
    21 => SetCityFocus { player, city, focus },
    22 => GreatPersonAction { unit, action },
    23 => SetSpecialists { player, city, specialist, count },
    24 => SetProcess { player, city, process },
    25 => RangedAttack { unit, target },
    26 => CityStrike { city, target },
    27 => SetUpRangedAttack { unit },
    28 => Promote { unit, promotion },
    29 => UpgradeUnit { unit },
    30 => DecideCity { player, city, decision },
    31 => Pillage { unit },
    32 => Rebase { unit, city },
    33 => AirSweep { unit, target },
    34 => Intercept { unit },
    35 => Nuke { unit, target },
    36 => BuildCitadel { unit },
    37 => EstablishTradeRoute { unit, destination, kind },
    38 => ChangeTradeHome { unit, city },
    39 => PlunderTradeRoute { unit },
    40 => Airlift { unit, city },
    41 => Denounce { player, target },
    42 => ProposeDeal { from, to, items },
    43 => RespondDeal { player, offer, accept },
    44 => ProposeResolution { player, kind, choice, repeal },
    45 => CastVotes { player, proposal, choice, delegates },
    46 => MoveSpy { player, spy, city, diplomat },
    47 => StealTechnology { player, from, tech },
    48 => StageCoup { player, spy },
    49 => BuildSpaceshipPart { unit },
    50 => GiftGold { player, city_state, gold },
    51 => GiftUnit { unit, city_state },
    52 => GiftImprovement { player, city_state, tile },
    53 => BullyGold { player, city_state },
    54 => BullyWorker { player, city_state },
    55 => BuyoutCityState { player, city_state },
    56 => PledgeProtection { player, city_state, pledge },
    57 => UnlockPolicyBranch { player, branch },
    58 => AdoptPolicy { player, policy },
    59 => ChooseIdeology { player, ideology },
    60 => FoundPantheon { player, belief },
    61 => FoundReligion { unit, religion, founder, follower, pantheon, bonus },
    62 => EnhanceReligion { unit, follower, enhancer },
    63 => AddReformationBelief { player, belief },
    64 => SpreadReligion { unit },
    65 => RemoveHeresy { unit },
    66 => PurchaseWithFaith { player, city, item },
    67 => CreateGreatImprovement { unit },
    68 => CreateGreatWork { unit },
    69 => WriteTreatise { unit },
    70 => PerformConcert { unit },
    71 => SwitchIdeology { player },
    72 => Excavate { unit },
    73 => ChooseArchaeology { player, tile, choice },
    74 => ChooseFreeTechnology { player, tech },
    75 => ChooseFreeGreatPerson { player, kind },
    76 => SetAvoidGrowth { player, city, avoid },
    77 => RebaseToCarrier { unit, carrier },
    78 => SellExoticGoods { unit },
    79 => ChooseCityName { player, city, name },
    80 => ClaimRuins { unit, reward },
    81 => Paradrop { unit, destination },
});

kernel::command_field!(GreatPersonAction {
    0 => Construct(improvement),
    1 => DiscoverTechnology {},
    2 => HurryProduction {},
    3 => StartGoldenAge {},
    4 => TradeMission {},
    5 => BuyCityState {},
});

kernel::command_field!(CityDecision {
    0 => Annex {},
    1 => Puppet {},
    2 => Raze {},
    3 => Liberate {},
});

kernel::command_field!(RouteKind {
    0 => International {},
    1 => Food {},
    2 => Production {},
});

kernel::command_field!(DealItem {
    0 => Gold(amount),
    1 => GoldPerTurn(amount),
    2 => Resource { resource, amount },
    3 => City(city),
    4 => Embassy {},
    5 => OpenBorders {},
    6 => DefensivePact {},
    7 => ResearchAgreement {},
    8 => Friendship {},
    9 => Peace {},
    10 => ThirdPartyWar(player),
    11 => ThirdPartyPeace(player),
});

kernel::command_field!(ResolutionKind {
    0 => DiplomaticVictory {},
    1 => ChangeHost {},
    2 => StandingArmyTax {},
    3 => ScholarsInResidence {},
    4 => NuclearNonProliferation {},
    5 => CulturalHeritageSites {},
    6 => CityStateEmbargo {},
    7 => PlayerEmbargo {},
    8 => BanLuxury {},
    9 => ArtsFunding {},
    10 => SciencesFunding {},
    11 => InternationalProject {},
});

kernel::command_field!(ArchaeologyChoice {
    0 => Landmark {},
    1 => ArtifactFirst {},
    2 => ArtifactSecond {},
    3 => Writing {},
    4 => CultureBoost {},
});

kernel::command_field!(Reward {
    0 => Citizen {},
    1 => Culture {},
    2 => Faith {},
    3 => Gold {},
    4 => Experience {},
    5 => Map {},
});

kernel::command_field!(GreatPersonKind {
    0 => Artist {},
    1 => Engineer {},
    2 => Merchant {},
    3 => Scientist {},
    4 => Writer {},
    5 => Musician {},
    6 => General {},
    7 => Admiral {},
    8 => Prophet {},
});
