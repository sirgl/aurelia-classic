//! The unit abilities the AI used to leave unused (`ai::unit_abilities`,
//! classic/docs/ai-unit-abilities.md): a great person that may buy a city-state
//! buys it when the city is worth more than its other actions, under
//! `unit-abilities` and, since the fifth promotion (1 November), `standard`;
//! not under `pre-abilities`, the standard before it.

use classic_content::core::core_rules;
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::planner::TurnBrief;
use classic_rules::ai::state::{AiState, SeenCity};
use classic_rules::ai::system::AdvisorInput;
use classic_rules::ai::unit_abilities::{great_person_commands, improvement_value, mission_value};
use classic_rules::ai::value::{PriceModel, Prices};
use classic_rules::ai::variant::Variant;
use classic_rules::ai::AiSettings;
use classic_rules::derived::great_people::GreatPersonAction;
use classic_rules::ids::{CityHandle, PlayerId, TerrainId, TileId, UnitHandle, UnitKindId};
use classic_rules::math::HexCoord;
use classic_rules::rules::content::CityStateId;
use classic_rules::world::city::CityStatus;
use classic_rules::world::city_states::Personality;
use classic_rules::world::player::{ClassicKind, PlayerKind};
use classic_rules::{compile_rules, ClassicGame, Command, CompiledRuleset, GameConfig, GameState};
use kernel::ai::driver::AiDriver;

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const CS: PlayerId = PlayerId(2);
const GRASS: TerrainId = TerrainId(1);
/// A city-state's size in the middle of a game.
const CITIZENS: u32 = 6;

/// Venice at (5,5), another nation at (13,5), a city-state of `CITIZENS` at
/// (5,13) on open grassland, all met and every tile explored.
struct World {
    game: GameState,
    rules: CompiledRuleset,
    minor: CityHandle,
}

impl World {
    fn new() -> Self {
        let rules = compile_rules(core_rules()).unwrap();
        let mut game = GameState::new(&rules, GameConfig { players: 3, width: 40, height: 20, rng_seed: 5 });
        let all: Vec<_> = (0..800).map(|i| (TileId(i), GRASS)).collect();
        game.debug_set_terrain_batch(&rules, &all);
        let id = CityStateId(rules.module.city_states.iter().position(|c| c.key == "core:city-state/byblos").unwrap() as u16);
        game.debug_set_player_kind(CS, PlayerKind::Module(ClassicKind::CityState(id)));
        game.debug_set_personality(CS, Personality::Neutral);
        let venice = rules.nations.iter().position(|n| n.key == "core:nation/venice").unwrap();
        game.debug_set_player_nation(P0, venice as u8);
        let at = |game: &GameState, q, r| game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap();
        game.debug_found_city(&rules, P0, at(&game, 5, 5));
        game.debug_found_city(&rules, P1, at(&game, 13, 5));
        let minor = game.debug_found_city(&rules, CS, at(&game, 5, 13));
        game.debug_set_population(&rules, minor, CITIZENS);
        // Revealed after the founding: the map is remembered with the borders.
        let every: Vec<TileId> = (0..800).map(TileId).collect();
        for p in [P0, P1, CS] {
            game.debug_reveal(p, &every);
        }
        game.debug_meet_all();
        World { game, rules, minor }
    }

    fn tile(&self, q: i32, r: i32) -> TileId {
        self.game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
    }

    /// The first unit kind whose data gives it this great person action: the
    /// purchase (`UnitAbility::BuyCityState`) or only the trade mission.
    fn merchant(&self, buys: bool) -> UnitKindId {
        use classic_rules::classic::content::UnitAbility;
        let at = (0..self.rules.units.len()).position(|i| {
            let u = &self.rules.units[i];
            classic_rules::derived::great_people::great_person_of(&self.rules, UnitKindId(i as u16)).is_some()
                && u.module.economy.base_gold > 0
                && u.module.abilities.contains(&UnitAbility::BuyCityState) == buys
        });
        UnitKindId(at.expect("a merchant in the rules") as u16)
    }

    fn spawn(&mut self, at: TileId, kind: UnitKindId) -> UnitHandle {
        self.game.debug_spawn_unit(&self.rules, P0, at, kind)
    }

    fn until_active(&mut self, player: PlayerId) {
        for _ in 0..8 {
            let active = self.game.view().auth.active_player();
            if active == player {
                return;
            }
            self.game.execute(&self.rules, Command::EndTurn { player: active }).unwrap();
        }
        panic!("{player:?} never became active");
    }

    /// The orders of `unit-abilities`'s great people now, and the standard's.
    fn orders(&self) -> (Vec<Command>, Vec<Command>) {
        let obs = self.game.ai_observation(&self.rules, P0);
        let mut brief = TurnBrief::from_observation(&obs, &self.rules);
        brief.prices = Some(Prices::of(&obs, &self.rules, PriceModel::Uses));
        brief.abilities = true;
        let mut state = AiState::new(P0, obs.round);
        let minor = self.game.view().auth.cities().get(self.minor).unwrap();
        state.see_city(SeenCity { tile: minor.tile, owner: CS, population: minor.population, last_seen_round: obs.round });
        let input = AdvisorInput { observation: &obs, brief: &brief, state: &state, rules: &self.rules };
        (great_person_commands(&input), classic_rules::ai::great_people::great_person_commands(&obs, &self.rules))
    }
}

fn action_of(commands: &[Command], unit: UnitHandle) -> Option<GreatPersonAction> {
    commands.iter().find_map(|c| match c {
        Command::GreatPersonAction { unit: u, action } if *u == unit => Some(*action),
        _ => None,
    })
}

/// On the city-state's land the merchant that may buy it buys it (a city of
/// six is worth more than a mission's gold and its improvement); the standard
/// sends a trade mission. The rules accept the order and the city-state
/// becomes Venice's puppet.
#[test]
fn a_merchant_that_may_buy_the_city_state_buys_it_where_the_standard_sends_a_mission() {
    let mut w = World::new();
    w.until_active(P0);
    let buyer = w.merchant(true);
    let at = w.tile(5, 12);
    let merchant = w.spawn(at, buyer);
    let (variant, standard) = w.orders();
    assert_eq!(action_of(&standard, merchant), Some(GreatPersonAction::TradeMission));
    assert_eq!(action_of(&variant, merchant), Some(GreatPersonAction::BuyCityState));
    let order = variant.into_iter().find(|c| matches!(c, Command::GreatPersonAction { .. })).unwrap();
    w.game.execute(&w.rules, order).unwrap();
    let auth = w.game.view().auth;
    let bought = auth.cities().get(w.minor).unwrap();
    assert_eq!((bought.owner, bought.status), (P0, CityStatus::Puppet));
    w.game.validate(&w.rules).unwrap();
}

/// At home the merchant that may buy walks to the city-state's land (the AI
/// remembers its size).
#[test]
fn a_merchant_that_may_buy_walks_to_the_city_state() {
    let mut w = World::new();
    w.until_active(P0);
    let buyer = w.merchant(true);
    let at = w.tile(5, 6);
    let merchant = w.spawn(at, buyer);
    let (variant, _) = w.orders();
    let minor_tile = w.game.view().auth.cities().get(w.minor).unwrap().tile;
    let destination = variant.iter().find_map(|c| match c {
        Command::MarchTo { unit, destination } if *unit == merchant => Some(*destination),
        _ => None,
    });
    let destination = destination.expect("a walk to the city-state");
    assert_eq!(w.game.view().auth.map().owner(destination).flatten(), Some(CS));
    let dims = w.game.view().auth.map().dimensions;
    assert!(dims.distance(destination, minor_tile).unwrap() <= 3);
}

/// A city-state of one citizen is worth less than the improvement at these
/// prices (its unrest takes the content empire into unhappiness): the merchant
/// does what the standard does.
#[test]
fn a_purchase_that_does_not_pay_is_left() {
    let mut w = World::new();
    let minor = w.minor;
    w.game.debug_set_population(&w.rules, minor, 1);
    w.until_active(P0);
    let buyer = w.merchant(true);
    let at = w.tile(5, 12);
    let merchant = w.spawn(at, buyer);
    let (variant, standard) = w.orders();
    assert_eq!(action_of(&variant, merchant), action_of(&standard, merchant));
    assert_ne!(action_of(&variant, merchant), Some(GreatPersonAction::BuyCityState));
}

/// The prices that decide: the bought city is worth more than the mission's
/// gold and than the improvement; an ordinary merchant's mission is weighed
/// against its improvement, never a purchase.
#[test]
fn the_purchase_is_priced_above_the_mission_and_the_improvement() {
    let mut w = World::new();
    w.until_active(P0);
    let obs = w.game.ai_observation(&w.rules, P0);
    let prices = Prices::of(&obs, &w.rules, PriceModel::Uses);
    let ordinary = w.merchant(false);
    let gold = w.rules.units[ordinary.0 as usize].module.economy.base_gold;
    let gp = classic_rules::derived::great_people::great_person_of(&w.rules, ordinary).unwrap();
    let improvement = improvement_value(&prices, &w.rules, gp.improvements[0]);
    let mission = mission_value(&prices, gold);
    let city = classic_rules::ai::operations::city_worth(&prices, CITIZENS, 0);
    assert!(city > mission && city > improvement, "city {city}, mission {mission}, improvement {improvement}");
    let at = w.tile(5, 12);
    let merchant = w.spawn(at, ordinary);
    let (variant, _) = w.orders();
    let action = action_of(&variant, merchant);
    assert_ne!(action, Some(GreatPersonAction::BuyCityState), "an ordinary merchant cannot buy");
    let expected = if mission > improvement { Some(GreatPersonAction::TradeMission) } else { action_of(&w.orders().1, merchant) };
    assert_eq!(action, expected);
}

/// The whole AI: `unit-abilities` and the promoted `standard` buy the city-state
/// they see with a merchant that may, a step off its land; `pre-abilities`, the
/// standard before the promotion, never does.
#[test]
fn the_unit_abilities_ai_buys_the_city_state_and_the_standard_does_not() {
    let bought = |variant: Variant| {
        let mut w = World::new();
        let buyer = w.merchant(true);
        let at = w.tile(5, 11);
        w.spawn(at, buyer);
        let settings = AiSettings::for_rules(&w.rules, w.rules.default_difficulty());
        let mut driver = AiDriver::with_mind(settings, Vec::new(), ClassicMind::new(variant));
        for _ in 0..8 {
            driver.run(&mut w.game, &w.rules, u32::MAX);
        }
        w.game.view().auth.cities().get(w.minor).is_some_and(|c| c.owner == P0)
    };
    assert!(bought(Variant::UnitAbilities));
    assert!(bought(Variant::Standard));
    assert!(!bought(Variant::PreAbilities));
}

/// The fifth promotion (1 November): `standard` carries the switches of `unit-abilities` and
/// `religion-fix` and wars and fights as `pre-abilities`, the standard before it, which carries
/// neither; the two variants keep their one switch each.
#[test]
fn the_promoted_standard_carries_both_switches() {
    let (standard, before) = (Variant::Standard, Variant::PreAbilities);
    assert!(standard.unit_abilities() && standard.religion_fix());
    assert!(!before.unit_abilities() && !before.religion_fix());
    assert!(Variant::UnitAbilities.unit_abilities() && !Variant::UnitAbilities.religion_fix());
    assert!(Variant::ReligionFix.religion_fix() && !Variant::ReligionFix.unit_abilities());
    for v in [before, Variant::UnitAbilities, Variant::ReligionFix] {
        assert_eq!(v.war_tuning(), standard.war_tuning(), "{}", v.name());
        assert_eq!(v.fight_fix(), standard.fight_fix(), "{}", v.name());
        assert_eq!(v.siege_tactics(), standard.siege_tactics(), "{}", v.name());
    }
    assert_eq!(before.war_tuning(), Variant::WarFix.war_tuning(), "`pre-abilities` wars as `war-fix`");
    assert_eq!(Variant::from_name("pre-abilities"), Some(before));
    // Since the sixth promotion `pre-tall` (the standard before it) and the tall-science family,
    // built on that standard, carry both switches too; since the seventh `pre-siege` as well.
    let tall = [Variant::PreSiege, Variant::PreTall, Variant::TallScience, Variant::TallBook, Variant::TallBook2, Variant::TallBook2Filler, Variant::TallBookNoOrder, Variant::TallBookCities, Variant::TallBookPolicies, Variant::ScienceFirst];
    // `net` plays its base, `pre-puppet` (`NET_BASE`), for every switch; `pre-camps`, `pre-workers`, `pre-join`, `pre-puppet`, the
    // conquest variants and `siege-join` play `standard`, and so do the Unciv variants where Unciv plays no part
    // and the candidates that change one thing of `standard` (`no-idle-cities`, `scout-early`, `workers-fit`,
    // `fog-march`, the nets' candidates) and `standard-scripted`, the standard before the twelfth promotion.
    let plays_standard = [Variant::Net, Variant::PreCamps, Variant::PreWorkers, Variant::PreJoin, Variant::PrePuppet, Variant::PuppetPrice, Variant::PuppetAll, Variant::PuppetWar, Variant::SiegeJoin, Variant::Unciv, Variant::UncivMilitary, Variant::NoIdleCities, Variant::ScoutEarly, Variant::WorkersFit, Variant::TradeFit, Variant::ArmyFit, Variant::Camps, Variant::ArmyCamps, Variant::PromoFit, Variant::HealFirst, Variant::FogMarch, Variant::StrategyNet, Variant::UnitNet, Variant::Hybrid, Variant::StandardScripted, Variant::PreCsGifts];
    // Unciv's modules inside `standard` (`std-unciv-*`) play it too but for their family.
    let carriers = |f: fn(Variant) -> bool| Variant::ALL.into_iter().filter(|v| f(*v) && !tall.contains(v) && !plays_standard.contains(v) && !v.unciv().parts().any()).count();
    assert!(carriers(Variant::unit_abilities) == 2 && carriers(Variant::religion_fix) == 2);
    assert!(tall.iter().all(|v| v.unit_abilities() && v.religion_fix()));
}




/// A cargo of exotic goods is sold where the rules accept the sale (next to
/// foreign land), and kept on the open sea.
#[test]
fn a_cargo_is_sold_where_the_rules_accept_the_sale() {
    use classic_rules::ai::planner::UnitView;
    let mut w = World::new();
    let portugal = w.rules.nations.iter().position(|n| n.key == "core:nation/portugal").unwrap();
    w.game.debug_set_player_nation(P0, portugal as u8);
    w.until_active(P0);
    let sea: Vec<(TileId, TerrainId)> = (9..13).map(|q| (w.tile(q, 5), TerrainId(4))).collect();
    w.game.debug_set_terrain_batch(&w.rules, &sea);
    let nau = UnitKindId(w.rules.units.iter().position(|u| u.key == "core:unit/nau").unwrap() as u16);
    let sells = |w: &mut World, q: i32| {
        let at = w.tile(q, 5);
        let unit = w.spawn(at, nau);
        let obs = w.game.ai_observation(&w.rules, P0);
        let brief = TurnBrief::from_observation(&obs, &w.rules);
        let (local, _) = w.game.ai_local_observation(&w.rules, P0, unit, 4).unwrap();
        classic_rules::ai::unit_abilities::sale(&UnitView { local: &local, brief: &brief }).and_then(|d| d.command)
    };
    assert!(sells(&mut w, 9).is_none(), "the open sea");
    let command = sells(&mut w, 11).expect("next to foreign land");
    assert!(matches!(command, Command::SellExoticGoods { .. }));
    w.game.execute(&w.rules, command).unwrap();
}
