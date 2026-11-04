//! The classic commands as trees of named values (`PlayerQueries::command_value`,
//! `command_from_value`; `kernel::replay::codec::Value`): every variant of `Command` goes
//! through the tree and back to the same journal bytes, the tree spells the variant and
//! its fields by name, and a tree of an unknown variant is refused.

use classic_content::core::core_rules;
use classic_rules::classic::content::{GreatPersonKind, PolicyBranchId, ReligionId};
use classic_rules::command::CityDecision;
use classic_rules::derived::archaeology::ArchaeologyChoice;
use classic_rules::derived::great_people::GreatPersonAction;
use classic_rules::ids::{
    BeliefId, BuildingKindId, CityHandle, ImprovementId, PlayerId, PolicyId, ProductionItem, PromotionId, ResourceId,
    TechId, TileId, UnitHandle, UnitKindId,
};
use classic_rules::knowledge::public::PlayerQueries;
use classic_rules::math::HexCoord;
use classic_rules::rules::content::{ProjectId, SpecialistId};
use classic_rules::ruins::Reward;
use classic_rules::world::citizens::CityFocus;
use classic_rules::world::city_economy::Process;
use classic_rules::world::congress::ResolutionKind;
use classic_rules::world::relations::DealItem;
use classic_rules::world::trade::RouteKind;
use classic_rules::{compile_rules, Command, CompiledRuleset, GameConfig, GameState};
use kernel::persistence::codec::CodecError;
use kernel::replay::codec::{CommandField, Value};

const ME: PlayerId = PlayerId(0);
const OTHER: PlayerId = PlayerId(1);

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
}

fn kind(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

/// A city and a warrior of player 0.
fn scene(rules: &CompiledRuleset) -> (GameState, UnitHandle, CityHandle) {
    let mut game = GameState::new(rules, GameConfig { players: 2, width: 20, height: 16, rng_seed: 3 });
    let city = game.debug_found_city(rules, ME, tile(&game, 5, 5));
    let unit = game.debug_spawn_unit(rules, ME, tile(&game, 6, 5), kind(rules, "core:unit/warrior"));
    (game, unit, city)
}

/// One instance of every variant of `Command` (its validity does not matter to the codec).
fn every_command(unit: UnitHandle, city: CityHandle) -> Vec<Command> {
    let (player, tile) = (ME, TileId(0));
    vec![
        Command::MoveUnit { unit, destination: tile },
        Command::FoundCity { settler: unit },
        Command::ChooseTechnology { player, technology: TechId(0) },
        Command::PlanResearch { player, target: TechId(0) },
        Command::AttackUnit { attacker: unit, defender: unit },
        Command::AttackCity { attacker: unit, city },
        Command::Fortify { unit },
        Command::BuildImprovement { unit, improvement: ImprovementId(0) },
        Command::BuildRoad { unit },
        Command::ChopForest { unit },
        Command::MarchTo { unit, destination: tile },
        Command::DeclareWar { player, enemy: OTHER },
        Command::MakePeace { player, enemy: OTHER },
        Command::OpenBorders { host: player, guest: OTHER },
        Command::EnqueueProduction { player, city, item: ProductionItem::Unit(UnitKindId(0)) },
        Command::ClearProduction { player, city },
        Command::ToggleWorkedTile { player, city, tile },
        Command::HurryProduction { player, city },
        Command::PurchaseTile { player, city, tile },
        Command::EndTurn { player },
        Command::LockWorkedTile { player, city, tile },
        Command::SetCityFocus { player, city, focus: CityFocus::Production },
        Command::GreatPersonAction { unit, action: GreatPersonAction::Construct(ImprovementId(0)) },
        Command::GreatPersonAction { unit, action: GreatPersonAction::DiscoverTechnology },
        Command::SetSpecialists { player, city, specialist: SpecialistId(0), count: 2 },
        Command::SetProcess { player, city, process: Some(Process::Contribute(ProjectId(0))) },
        Command::SetProcess { player, city, process: None },
        Command::RangedAttack { unit, target: tile },
        Command::CityStrike { city, target: tile },
        Command::SetUpRangedAttack { unit },
        Command::Promote { unit, promotion: PromotionId(0) },
        Command::UpgradeUnit { unit },
        Command::DecideCity { player, city, decision: CityDecision::Puppet },
        Command::Pillage { unit },
        Command::Rebase { unit, city },
        Command::AirSweep { unit, target: tile },
        Command::Intercept { unit },
        Command::Nuke { unit, target: tile },
        Command::BuildCitadel { unit },
        Command::EstablishTradeRoute { unit, destination: city, kind: RouteKind::Food },
        Command::ChangeTradeHome { unit, city },
        Command::PlunderTradeRoute { unit },
        Command::Airlift { unit, city },
        Command::Denounce { player, target: OTHER },
        Command::ProposeDeal {
            from: player,
            to: OTHER,
            items: vec![
                (player, DealItem::Gold(30)),
                (OTHER, DealItem::GoldPerTurn(2)),
                (player, DealItem::Resource { resource: ResourceId(0), amount: 1 }),
                (player, DealItem::City(city)),
                (OTHER, DealItem::Embassy),
                (player, DealItem::OpenBorders),
                (player, DealItem::DefensivePact),
                (player, DealItem::ResearchAgreement),
                (player, DealItem::Friendship),
                (player, DealItem::Peace),
                (OTHER, DealItem::ThirdPartyWar(player)),
                (OTHER, DealItem::ThirdPartyPeace(player)),
            ],
        },
        Command::RespondDeal { player, offer: 4, accept: true },
        Command::ProposeResolution { player, kind: ResolutionKind::PlayerEmbargo, choice: 1, repeal: false },
        Command::CastVotes { player, proposal: 0, choice: 1, delegates: 3 },
        Command::MoveSpy { player, spy: 0, city: Some(city), diplomat: false },
        Command::MoveSpy { player, spy: 1, city: None, diplomat: true },
        Command::StealTechnology { player, from: OTHER, tech: TechId(0) },
        Command::StageCoup { player, spy: 0 },
        Command::BuildSpaceshipPart { unit },
        Command::GiftGold { player, city_state: OTHER, gold: 250 },
        Command::GiftUnit { unit, city_state: OTHER },
        Command::GiftImprovement { player, city_state: OTHER, tile },
        Command::BullyGold { player, city_state: OTHER },
        Command::BullyWorker { player, city_state: OTHER },
        Command::BuyoutCityState { player, city_state: OTHER },
        Command::PledgeProtection { player, city_state: OTHER, pledge: true },
        Command::UnlockPolicyBranch { player, branch: PolicyBranchId(0) },
        Command::AdoptPolicy { player, policy: PolicyId(0) },
        Command::ChooseIdeology { player, ideology: PolicyBranchId(0) },
        Command::FoundPantheon { player, belief: BeliefId(0) },
        Command::FoundReligion {
            unit,
            religion: ReligionId(0),
            founder: BeliefId(0),
            follower: BeliefId(1),
            pantheon: Some(BeliefId(2)),
            bonus: None,
        },
        Command::EnhanceReligion { unit, follower: BeliefId(0), enhancer: BeliefId(1) },
        Command::AddReformationBelief { player, belief: BeliefId(0) },
        Command::SpreadReligion { unit },
        Command::RemoveHeresy { unit },
        Command::PurchaseWithFaith { player, city, item: ProductionItem::Building(BuildingKindId(0)) },
        Command::CreateGreatImprovement { unit },
        Command::CreateGreatWork { unit },
        Command::WriteTreatise { unit },
        Command::PerformConcert { unit },
        Command::SwitchIdeology { player },
        Command::Excavate { unit },
        Command::ChooseArchaeology { player, tile, choice: ArchaeologyChoice::Writing },
        Command::ChooseFreeTechnology { player, tech: TechId(0) },
        Command::ChooseFreeGreatPerson { player, kind: GreatPersonKind::Admiral },
        Command::SetAvoidGrowth { player, city, avoid: true },
        Command::RebaseToCarrier { unit, carrier: unit },
        Command::SellExoticGoods { unit },
        Command::ChooseCityName { player, city, name: 2 },
        Command::ClaimRuins { unit, reward: Reward::Map },
        Command::Paradrop { unit, destination: tile },
    ]
}

/// The variant names of `Command`, in the order of its declaration: the list above is complete
/// when every one of them is there.
const VARIANTS: [&str; 82] = [
    "MoveUnit", "FoundCity", "ChooseTechnology", "PlanResearch", "AttackUnit", "AttackCity", "Fortify",
    "BuildImprovement", "BuildRoad", "ChopForest", "MarchTo", "DeclareWar", "MakePeace", "OpenBorders",
    "EnqueueProduction", "ClearProduction", "ToggleWorkedTile", "HurryProduction", "PurchaseTile", "EndTurn",
    "LockWorkedTile", "SetCityFocus", "SetAvoidGrowth", "ChooseCityName", "GreatPersonAction", "SetSpecialists",
    "SetProcess", "RangedAttack", "CityStrike", "SetUpRangedAttack", "Promote", "UpgradeUnit", "DecideCity", "Pillage",
    "Rebase", "SellExoticGoods", "RebaseToCarrier", "AirSweep", "Intercept", "Nuke", "BuildCitadel",
    "EstablishTradeRoute", "ChangeTradeHome", "PlunderTradeRoute", "Airlift", "Paradrop", "Denounce", "ProposeDeal",
    "RespondDeal", "ProposeResolution", "CastVotes", "MoveSpy", "StealTechnology", "StageCoup", "BuildSpaceshipPart",
    "GiftGold", "GiftUnit", "GiftImprovement", "BullyGold", "BullyWorker", "BuyoutCityState", "PledgeProtection",
    "UnlockPolicyBranch", "AdoptPolicy", "ChooseIdeology", "FoundPantheon", "FoundReligion", "EnhanceReligion",
    "AddReformationBelief", "SpreadReligion", "RemoveHeresy", "PurchaseWithFaith", "CreateGreatImprovement",
    "CreateGreatWork", "WriteTreatise", "PerformConcert", "SwitchIdeology", "Excavate", "ChooseArchaeology",
    "ChooseFreeTechnology", "ChooseFreeGreatPerson", "ClaimRuins",
];

fn bytes(command: &Command, game: &GameState) -> Vec<u8> {
    let mut out = Vec::new();
    command.put(game.view().auth, &mut out);
    out
}

fn name(value: &Value) -> &'static str {
    match value {
        Value::Record { name, .. } => name,
        other => panic!("a command is a record, not {other:?}"),
    }
}

#[test]
fn every_command_reads_back_from_its_tree_to_the_same_bytes() {
    let rules = compile_rules(core_rules()).expect("core rules compile");
    let (game, unit, city) = scene(&rules);
    let view = game.player_view(&rules, ME);
    let commands = every_command(unit, city);
    let mut seen: Vec<&str> = Vec::new();
    for command in &commands {
        let value = view.command_value(command);
        let back = view.command_from_value(&value).unwrap_or_else(|e| panic!("{value:?}: {e}"));
        assert_eq!(bytes(&back, &game), bytes(command, &game), "{value:?}");
        if !seen.contains(&name(&value)) {
            seen.push(name(&value));
        }
    }
    for variant in VARIANTS {
        assert!(seen.contains(&variant), "no instance of Command::{variant}");
    }
    assert_eq!(seen.len(), VARIANTS.len(), "a variant the list of names has not: {seen:?}");
}

#[test]
fn a_tree_names_the_variant_and_its_fields() {
    let rules = compile_rules(core_rules()).expect("core rules compile");
    let (game, unit, city) = scene(&rules);
    let view = game.player_view(&rules, ME);
    let auth = game.view().auth;
    let unit_id = auth.units().get(unit).expect("spawned").core.id.0;
    let city_id = auth.cities().get(city).expect("founded").id.0;
    let record = |name, fields| Value::Record { name, fields };

    let moved = Command::MoveUnit { unit, destination: TileId(7) };
    assert_eq!(
        view.command_value(&moved),
        record("MoveUnit", vec![("unit", Value::Uint(unit_id)), ("destination", Value::Uint(7))])
    );

    let academy = Command::GreatPersonAction { unit, action: GreatPersonAction::Construct(ImprovementId(2)) };
    assert_eq!(
        view.command_value(&academy),
        record(
            "GreatPersonAction",
            vec![
                ("unit", Value::Uint(unit_id)),
                ("action", record("Construct", vec![("improvement", Value::Uint(2))])),
            ],
        )
    );

    let deal = Command::ProposeDeal { from: ME, to: OTHER, items: vec![(ME, DealItem::Gold(30)), (OTHER, DealItem::City(city))] };
    assert_eq!(
        view.command_value(&deal),
        record(
            "ProposeDeal",
            vec![
                ("from", Value::Uint(0)),
                ("to", Value::Uint(1)),
                (
                    "items",
                    Value::List(vec![
                        Value::List(vec![Value::Uint(0), record("Gold", vec![("amount", Value::Uint(30))])]),
                        Value::List(vec![Value::Uint(1), record("City", vec![("city", Value::Uint(city_id))])]),
                    ]),
                ),
            ],
        )
    );
}

#[test]
fn a_tree_of_an_unknown_variant_field_or_entity_is_refused() {
    let rules = compile_rules(core_rules()).expect("core rules compile");
    let (game, unit, _) = scene(&rules);
    let view = game.player_view(&rules, ME);
    let unit_id = game.view().auth.units().get(unit).expect("spawned").core.id.0;
    let record = |name, fields| Value::Record { name, fields };
    let refused = |value: Value| view.command_from_value(&value).expect_err("refused");
    assert_eq!(refused(record("TeleportUnit", Vec::new())), CodecError::Invalid("variant of Command"));
    assert_eq!(refused(record("MoveUnit", vec![("unit", Value::Uint(unit_id))])), CodecError::Invalid("missing field destination"));
    assert_eq!(
        refused(record("MoveUnit", vec![("unit", Value::Uint(99_999)), ("destination", Value::Uint(0))])),
        CodecError::Invalid("unknown unit")
    );
    assert_eq!(
        refused(record("GreatPersonAction", vec![("unit", Value::Uint(unit_id)), ("action", record("Retire", Vec::new()))])),
        CodecError::Invalid("variant of GreatPersonAction")
    );
}
