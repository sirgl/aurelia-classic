//! Trade, city-state, diplomacy and tourism effects of the classic ruleset
//! (classic/docs/classic/rules.md): Venice, Arabia, Portugal, Morocco, Merchant
//! Confederacy, Economic Union, the Hanse, Machu Picchu, research
//! agreements, the International Space Station, Gunboat Diplomacy, Treaty
//! Organization, United Front, Cultural Diplomacy, Scholasticism, the
//! Patronage finisher, Sweden, Entrepreneurship and Futurism.

use classic_rules::ClassicGame;
use classic_content::core::core_rules;
use classic_rules::derived::great_people::friendship_rate;
use classic_rules::derived::trade::{range, routes_possible, trade_offers};
use classic_rules::effects::city::city_modifiers;
use classic_rules::ids::{BuildingKindId, CityHandle, PlayerId, PolicyId, ResourceId, TechId, TerrainId, TileId, UnitHandle, UnitKindId};
use classic_rules::math::HexCoord;
use classic_rules::replay::authoritative_hash;
use classic_rules::rules::content::{CityStateId, ReligionId};
use classic_rules::rules::effects::YieldKind;
use classic_rules::rules::modifiers::Caps;
use classic_rules::world::city_states::Personality;
use classic_rules::world::player::PlayerKind;
use classic_rules::world::relations::DealItem;
use classic_rules::world::religion::Creed;
use classic_rules::world::trade::{RouteKind, TradeDomain};
use classic_rules::{compile_rules, Command, CompiledRuleset, GameConfig, GameState};
use classic_rules::world::player::ClassicKind;

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const CS: PlayerId = PlayerId(2);
const GRASS: TerrainId = TerrainId(1);

/// Two nations and one city-state of `city_state` on open grassland,
/// all met and every tile explored: P0 at (5,5), P1 at (13,5), the
/// city-state at (5,13).
struct World {
    game: GameState,
    rules: CompiledRuleset,
    home: CityHandle,
    away: CityHandle,
    minor: CityHandle,
}

impl World {
    fn new(city_state: &str) -> Self {
        let rules = compile_rules(core_rules()).unwrap();
        let mut game = GameState::new(&rules, GameConfig { players: 3, width: 40, height: 20, rng_seed: 5 });
        let all: Vec<_> = (0..800).map(|i| (TileId(i), GRASS)).collect();
        game.debug_set_terrain_batch(&rules, &all);
        let every: Vec<TileId> = (0..800).map(TileId).collect();
        for p in [P0, P1, CS] {
            game.debug_reveal(p, &every);
        }
        let id = CityStateId(rules.module.city_states.iter().position(|c| c.key == city_state).unwrap() as u16);
        game.debug_set_player_kind(CS, PlayerKind::Module(ClassicKind::CityState(id)));
        game.debug_set_personality(CS, Personality::Neutral);
        let at = |game: &GameState, q, r| game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap();
        let home = game.debug_found_city(&rules, P0, at(&game, 5, 5));
        let away = game.debug_found_city(&rules, P1, at(&game, 13, 5));
        let minor = game.debug_found_city(&rules, CS, at(&game, 5, 13));
        game.debug_meet_all();
        World { game, rules, home, away, minor }
    }

    fn tile(&self, q: i32, r: i32) -> TileId {
        self.game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
    }

    fn city_tile(&self, city: CityHandle) -> TileId {
        self.game.view().auth.cities().get(city).unwrap().tile
    }

    fn nation(&mut self, player: PlayerId, key: &str) {
        let nation = self.rules.nations.iter().position(|c| c.key == key).unwrap();
        self.game.debug_set_player_nation(player, nation as u8);
    }

    fn policy(&mut self, player: PlayerId, key: &str) {
        let id = PolicyId(self.rules.module.policies.iter().position(|p| p.key == key).unwrap() as u16);
        self.game.debug_adopt_policy(&self.rules, player, id);
    }

    fn learn(&mut self, player: PlayerId, key: &str) {
        let t = self.rules.technologies.iter().position(|t| t.key == key).unwrap();
        self.game.debug_learn(&self.rules, player, TechId(t as u16));
    }

    fn building(&self, key: &str) -> BuildingKindId {
        BuildingKindId(self.rules.buildings.iter().position(|b| b.key == key).unwrap() as u16)
    }

    fn unit_kind(&self, key: &str) -> UnitKindId {
        UnitKindId(self.rules.units.iter().position(|u| u.key == key).unwrap() as u16)
    }

    fn spawn(&mut self, player: PlayerId, at: TileId, key: &str) -> UnitHandle {
        let kind = self.unit_kind(key);
        self.game.debug_spawn_unit(&self.rules, player, at, kind)
    }

    fn exec(&mut self, command: Command) -> Result<(), String> {
        self.game.execute(&self.rules, command).map(|_| ()).map_err(|e| format!("{e:?}"))
    }

    /// The gold P0's caravan in its capital would earn on a route to `to`, divided by the route's
    /// distance factor (hundredths of gold before the factor), so that a flat bonus shows as itself.
    fn offer_gold(&mut self, to: CityHandle) -> u32 {
        let caravan = self.spawn(P0, self.city_tile(self.home), "core:unit/caravan");
        let offers = trade_offers(self.game.view().auth, &self.rules, caravan);
        let offer = offers
            .iter()
            .find(|o| o.destination == to && o.kind == RouteKind::International)
            .expect("an international offer");
        // Undo the distance factor (land routes only here); rounding stays below 1 hundredth per percent.
        (offer.origin.gold * 100 + classic_rules::derived::trade::distance_percent(offer.steps) / 2)
            / classic_rules::derived::trade::distance_percent(offer.steps)
    }

    /// P0 starts a caravan route from its capital to `to`.
    fn route_to(&mut self, to: CityHandle) {
        let unit = self.spawn(P0, self.city_tile(self.home), "core:unit/caravan");
        self.exec(Command::EstablishTradeRoute { unit, destination: to, kind: RouteKind::International })
            .unwrap();
    }

    fn until_active(&mut self, player: PlayerId) {
        for _ in 0..8 {
            let active = self.game.view().auth.active_player();
            if active == player {
                return;
            }
            self.exec(Command::EndTurn { player: active }).unwrap();
        }
        panic!("{player:?} never became active");
    }

    /// Every player ends its turn once.
    fn round(&mut self) {
        let start = self.game.view().auth.turn().round;
        while self.game.view().auth.turn().round == start {
            let active = self.game.view().auth.active_player();
            self.exec(Command::EndTurn { player: active }).unwrap();
        }
    }

    fn influence(&self, major: PlayerId) -> i32 {
        self.game.view().auth.module().city_states().influence(CS, major)
    }
}

// ---- trade routes ----

#[test]
fn venice_runs_twice_the_trade_routes() {
    let mut w = World::new("core:city-state/byblos");
    w.learn(P0, "core:tech/animal-husbandry");
    w.learn(P0, "core:tech/sailing");
    assert_eq!(routes_possible(w.game.view().auth, &w.rules, P0), 2);
    w.nation(P0, "core:nation/venice");
    assert_eq!(routes_possible(w.game.view().auth, &w.rules, P0), 4, "rules 3.1: ×2 after the sum");
}

#[test]
fn arabia_adds_five_tiles_of_land_range() {
    let mut w = World::new("core:city-state/byblos");
    let land = |w: &World| {
        let auth = w.game.view().auth;
        range(auth, &w.rules, auth.cities().get(w.home).unwrap(), TradeDomain::Land)
    };
    assert_eq!(land(&w), 12, "the first-era land range");
    w.nation(P0, "core:nation/arabia");
    assert_eq!(land(&w), 17, "+5 tiles before the percent");
}

#[test]
fn portugal_doubles_resource_gold_and_morocco_pays_whoever_sends_it_a_route() {
    let mut w = World::new("core:city-state/byblos");
    w.learn(P0, "core:tech/animal-husbandry");
    let wine = ResourceId(w.rules.resources.iter().position(|r| r.key == "core:resource/wine").unwrap() as u16);
    let home_tile = w.city_tile(w.home);
    w.game.debug_set_resource_batch(&w.rules, &[(home_tile, wine)]);
    let plain = w.offer_gold(w.away);
    w.nation(P0, "core:nation/portugal");
    assert_eq!(w.offer_gold(w.away), plain + 25, "one differing luxury: 25 → 50");
    w.nation(P1, "core:nation/morocco");
    assert_eq!(w.offer_gold(w.away), plain + 25 + 200, "Morocco: +2 gold to the sender");
}

#[test]
fn merchant_confederacy_and_economic_union_add_route_gold() {
    let mut w = World::new("core:city-state/byblos");
    w.learn(P0, "core:tech/animal-husbandry");
    let (to_minor, to_nation) = (w.offer_gold(w.minor), w.offer_gold(w.away));
    w.policy(P0, "core:policy/patronage/merchant-confederacy");
    assert_eq!(w.offer_gold(w.minor), to_minor + 100, "+1 gold on routes to city-states");
    w.policy(P0, "core:policy/freedom/economic-union");
    assert_eq!(w.offer_gold(w.away), to_nation, "no shared ideology yet");
    let freedom = w.rules.module.policy_branches.iter().position(|b| b.key == "core:policy/freedom").unwrap();
    let freedom = classic_rules::rules::content::PolicyBranchId(freedom as u16);
    w.game.debug_set_ideology(&w.rules, P0, freedom);
    w.game.debug_set_ideology(&w.rules, P1, freedom);
    assert_eq!(w.offer_gold(w.away), to_nation + 200, "+2 gold with a nation of the same ideology");
}

#[test]
fn morocco_capital_gains_gold_and_culture_per_trading_partner() {
    let mut w = World::new("core:city-state/byblos");
    w.nation(P1, "core:nation/morocco");
    w.learn(P0, "core:tech/animal-husbandry");
    let yields = |w: &World| w.game.derived().yields.of(w.away).unwrap();
    let before = yields(&w);
    w.route_to(w.away);
    let after = yields(&w);
    assert_eq!(after.culture - before.culture, 1, "+1 culture per partner");
    assert!(after.gold >= before.gold + 3, "+3 gold per partner (and the route's own gold)");
    w.game.validate(&w.rules).unwrap();
}

#[test]
fn the_hanse_adds_production_per_route_to_a_city_state() {
    let mut w = World::new("core:city-state/byblos");
    w.learn(P0, "core:tech/animal-husbandry");
    let hanse = w.building("core:building/hanse");
    w.game.debug_add_building(&w.rules, w.home, hanse);
    let production = |w: &World| {
        let auth = w.game.view().auth;
        let mods = city_modifiers(auth, &w.rules, auth.cities().get(w.home).unwrap());
        mods.yields[YieldKind::Production.index()].evaluate(100, Caps::at_least(0))
    };
    let before = production(&w);
    w.route_to(w.minor);
    assert_eq!(production(&w), before + 5, "+5% per route to a city-state");
    w.game.validate(&w.rules).unwrap();
}

#[test]
fn routes_carry_religious_pressure_far_and_arabia_doubles_it() {
    let pressure_per_round = |arabia: bool| {
        let mut w = World::new("core:city-state/byblos");
        let far = w.game.debug_found_city(&w.rules, P1, w.tile(19, 5));
        let at = w.city_tile(w.home);
        let prophet = w.spawn(P0, at, "core:unit/great-prophet");
        let belief = |w: &World, key: &str| {
            classic_rules::ids::BeliefId(w.rules.module.beliefs.iter().position(|b| b.key == key).unwrap() as u16)
        };
        let (founder, follower, pantheon) = (
            belief(&w, "core:belief/founder/tithe"),
            belief(&w, "core:belief/follower/pagodas"),
            belief(&w, "core:belief/pantheon/ancestor-worship"),
        );
        w.exec(Command::FoundReligion { unit: prophet, religion: ReligionId(0), founder, follower, pantheon: Some(pantheon), bonus: None })
            .unwrap();
        w.learn(P0, "core:tech/animal-husbandry");
        if arabia {
            w.nation(P0, "core:nation/arabia");
        } else {
            // Without Arabia's range the caravansary reaches 12 × 1.4 = 16 tiles.
            let caravansary = w.building("core:building/caravansary");
            w.game.debug_add_building(&w.rules, w.home, caravansary);
        }
        w.route_to(far);
        let pressure = |w: &World| w.game.view().auth.module().religion().city(far).pressure_of(Creed::Religion(ReligionId(0)));
        let before = pressure(&w);
        w.round();
        pressure(&w) - before
    };
    assert_eq!(pressure_per_round(false), 60, "rules 3.9: a route presses at any distance");
    assert_eq!(pressure_per_round(true), 120, "Arabia: ×2 from the route's origin");
}

#[test]
fn machu_picchu_raises_the_gold_of_city_connections() {
    let mut w = World::new("core:city-state/byblos");
    let other = w.game.debug_found_city(&w.rules, P0, w.tile(9, 5));
    w.game.debug_set_population(&w.rules, other, 4);
    let road: Vec<TileId> = (5..=9).map(|q| w.tile(q, 5)).collect();
    w.game.debug_set_road_batch(&w.rules, &road);
    let gold = |w: &World| classic_rules::derived::maintenance::connection_gold_times100(w.game.view().auth, &w.rules, P0);
    let before = gold(&w);
    assert!(before > 0, "the city is connected");
    let machu = w.building("core:building/machu_picchu");
    w.game.debug_add_building(&w.rules, w.home, machu);
    assert_eq!(gold(&w), before * 125 / 100);
}

// ---- research agreements, great scientists ----

#[test]
fn scientific_revolution_raises_the_research_agreement_payout() {
    let payout = |policy: bool| {
        let mut w = World::new("core:city-state/byblos");
        for p in [P0, P1] {
            for key in ["core:tech/writing", "core:tech/civil-service", "core:tech/chivalry", "core:tech/education"] {
                w.learn(p, key);
            }
            w.game.debug_earn(p, 1000);
            let library = w.building("core:building/library");
            let city = if p == P0 { w.home } else { w.away };
            w.game.debug_add_building(&w.rules, city, library);
            w.game.debug_set_population(&w.rules, city, 6);
        }
        if policy {
            w.policy(P0, "core:policy/rationalism/scientific-revolution");
        }
        for items in [
            vec![(P0, DealItem::Embassy), (P1, DealItem::Embassy)],
            vec![(P0, DealItem::Friendship)],
            vec![(P0, DealItem::ResearchAgreement)],
        ] {
            w.until_active(P0);
            let id = w.game.view().auth.module().relations().next_offer_id();
            w.exec(Command::ProposeDeal { from: P0, to: P1, items }).unwrap();
            w.exec(Command::RespondDeal { player: P1, offer: id, accept: true }).unwrap();
        }
        let mut pooled = 0;
        for _ in 0..40 {
            let r = w.game.view().auth.module().relations();
            let round = w.game.view().auth.turn().round;
            if r.research_agreement_until(P0, P1) == 0 && pooled > 0 {
                break;
            }
            if r.has_research_agreement(P0, P1, round) {
                pooled = r.research_points(P0, P1).min(r.research_points(P1, P0));
            }
            w.round();
        }
        (pooled, w.game.view().auth.players().science_of(P0))
    };
    let (pooled, plain) = payout(false);
    let (same, raised) = payout(true);
    assert!(pooled > 0 && same == pooled, "the same science pooled");
    // 50 % → 100 % of a third of the pooled science: the policy adds +50 %
    // (Unciv: Policies.json, Scientific Revolution). The last turn before the
    // payout still adds to it, hence the margin.
    let half = i64::from(pooled / 3 / 2);
    let gained = i64::from(raised) - i64::from(plain);
    assert!(gained >= half && gained <= half + 2, "{gained} vs {half}");
}

#[test]
fn the_international_space_station_raises_a_great_scientists_science() {
    let bulb = |station: bool| {
        let mut w = World::new("core:city-state/byblos");
        let library = w.building("core:building/library");
        w.game.debug_add_building(&w.rules, w.home, library);
        w.game.debug_set_population(&w.rules, w.home, 8);
        w.round();
        if station {
            let iss = w.building("core:building/international_space_station");
            w.game.debug_add_building(&w.rules, w.home, iss);
        }
        w.until_active(P0);
        let at = w.city_tile(w.home);
        let scientist = w.spawn(P0, at, "core:unit/great-scientist");
        let before = w.game.view().auth.players().science_of(P0);
        w.exec(Command::GreatPersonAction {
            unit: scientist,
            action: classic_rules::derived::great_people::GreatPersonAction::DiscoverTechnology,
        })
        .unwrap();
        w.game.view().auth.players().science_of(P0) - before
    };
    let plain = bulb(false);
    assert!(plain > 0);
    assert_eq!(bulb(true), plain * 133 / 100, "+33% of the bulb");
}

// ---- city-states ----

#[test]
fn treaty_organization_gives_influence_with_city_states_a_route_connects() {
    let mut w = World::new("core:city-state/byblos");
    w.learn(P0, "core:tech/animal-husbandry");
    w.policy(P0, "core:policy/freedom/treaty-organization");
    w.game.debug_set_influence(&w.rules, CS, P0, 40);
    w.until_active(CS);
    w.exec(Command::EndTurn { player: CS }).unwrap();
    assert_eq!(w.influence(P0), 39, "no route: the plain decay");
    w.until_active(P0);
    w.route_to(w.minor);
    w.until_active(CS);
    w.exec(Command::EndTurn { player: CS }).unwrap();
    assert_eq!(w.influence(P0), 39 - 1 + 3, "decay -1 and +3 a turn");
}

#[test]
fn gunboat_diplomacy_gives_influence_with_city_states_afraid_of_you() {
    let gain = |policy: bool| {
        let mut w = World::new("core:city-state/byblos");
        if policy {
            w.policy(P0, "core:policy/autocracy/gunboat-diplomacy");
        }
        for (q, r) in [(4, 12), (6, 12), (4, 13), (6, 13), (5, 14), (4, 14), (6, 11), (5, 12)] {
            let at = w.tile(q, r);
            w.spawn(P0, at, "core:unit/infantry");
        }
        // A friend: its units do not intrude.
        w.game.debug_set_influence(&w.rules, CS, P0, 35);
        w.until_active(CS);
        w.exec(Command::EndTurn { player: CS }).unwrap();
        w.influence(P0) - 35
    };
    assert_eq!(gain(false), -1);
    assert_eq!(gain(true), -1 + 4, "afraid: +4 a turn");
}

#[test]
fn united_front_speeds_up_unit_gifts_while_sharing_an_enemy() {
    let first_gift = |policy: bool| {
        let mut w = World::new("core:city-state/almaty");
        if policy {
            w.policy(P0, "core:policy/autocracy/united-front");
        }
        w.game.debug_force_war(&w.rules, P1, P0);
        w.game.debug_force_war(&w.rules, P1, CS);
        w.game.debug_set_influence(&w.rules, CS, P0, 35);
        w.game.view().auth.module().city_states().standing(CS, P0).spawn_counter
    };
    let plain = first_gift(false);
    let united = first_gift(true);
    // Unciv: DiplomacyTurnManager.kt, the first gift waits the full interval.
    assert!((19..=21).contains(&plain), "19 + 0..2: {plain}");
    assert!((10..=12).contains(&united), "19 × 100 / 175 + 0..2: {united}");
}

#[test]
fn cultural_diplomacy_doubles_allied_resources_and_their_happiness() {
    let mut w = World::new("core:city-state/byblos");
    let wine = ResourceId(w.rules.resources.iter().position(|r| r.key == "core:resource/wine").unwrap() as u16);
    let minor_tile = w.city_tile(w.minor);
    w.game.debug_set_resource_batch(&w.rules, &[(minor_tile, wine)]);
    let copies = |w: &World| classic_rules::derived::resources::resource_ledger(w.game.view().auth, &w.rules, P0).total(wine);
    let luxuries = |w: &World| classic_rules::derived::happiness::player_happiness(w.game.view().auth, &w.rules, P0).luxuries;
    let alone = luxuries(&w);
    assert_eq!(copies(&w), 0);
    w.game.debug_set_influence(&w.rules, CS, P0, 70);
    assert_eq!(copies(&w), 1, "rules 4.7: the ally gets the city-state's luxury");
    let allied = luxuries(&w);
    assert!(allied > alone, "the ally's luxury makes people happy");
    w.policy(P0, "core:policy/patronage/cultural-diplomacy");
    assert_eq!(copies(&w), 2, "×2 with Cultural Diplomacy");
    let own = w.rules.resources[wine.0 as usize].module.happiness;
    assert_eq!(luxuries(&w) - allied, own * 150 / 100 - own);
}

#[test]
fn scholasticism_gives_a_quarter_of_allied_science() {
    let mut w = World::new("core:city-state/byblos");
    let library = w.building("core:building/library");
    w.game.debug_add_building(&w.rules, w.minor, library);
    w.game.debug_set_population(&w.rules, w.minor, 8);
    w.game.debug_set_influence(&w.rules, CS, P0, 70);
    w.round();
    let science = |w: &World| classic_rules::derived::income::player_income(w.game.view().auth, &w.rules, P0).science;
    let before = science(&w);
    let theirs = w.game.view().auth.players().economy_of(CS).unwrap().science_history[0] as i32;
    assert!(theirs > 0, "the city-state made science");
    w.policy(P0, "core:policy/patronage/scholasticism");
    assert_eq!(science(&w) - before, theirs * 25 / 100);
}

#[test]
fn allied_city_states_give_great_people_with_the_patronage_finisher() {
    let mut w = World::new("core:city-state/byblos");
    w.policy(P0, "core:policy/patronage/finisher");
    w.game.debug_set_influence(&w.rules, CS, P0, 90);
    let great_people = |w: &World| {
        let auth = w.game.view().auth;
        auth.indexes()
            .units_of(P0)
            .iter()
            .filter(|u| {
                let kind = auth.units().get(**u).unwrap().core.kind;
                classic_rules::derived::great_people::great_person_of(&w.rules, kind).is_some()
            })
            .count()
    };
    let mut rounds = 0;
    while great_people(&w) == 0 && rounds < 30 {
        w.game.debug_set_influence(&w.rules, CS, P0, 90);
        w.round();
        rounds += 1;
        if rounds == 5 {
            let save = classic_rules::persistence::encode::encode(&w.game, &w.rules);
            let loaded = classic_rules::persistence::decode::decode(&save, &w.rules).unwrap();
            assert_eq!(authoritative_hash(&loaded), authoritative_hash(&w.game));
            assert!(w.game.view().auth.module().city_states().major(P0).ally_great_person_turns > 0);
        }
    }
    assert_eq!(great_people(&w), 1, "a great person within (37 + 0 + 0..6) / 2 turns");
    assert!((18..=22).contains(&rounds), "{rounds}");
    w.game.validate(&w.rules).unwrap();
}

#[test]
fn sweden_gifts_great_people_for_influence() {
    let mut w = World::new("core:city-state/byblos");
    let at = w.tile(5, 12);
    let general = w.spawn(P0, at, "core:unit/great-general");
    let gift = Command::GiftUnit { unit: general, city_state: CS };
    assert!(w.exec(gift.clone()).is_err(), "only Sweden gifts great people");
    w.nation(P0, "core:nation/sweden");
    let before = w.influence(P0);
    w.exec(gift).unwrap();
    assert_eq!(w.influence(P0) - before, 90);
    assert!(w.game.view().auth.units().get(general).is_none(), "the great person is used up");
}

#[test]
fn sweden_and_its_friends_gain_great_people_from_friendship() {
    let mut w = World::new("core:city-state/byblos");
    w.nation(P0, "core:nation/sweden");
    w.until_active(P0);
    let id = w.game.view().auth.module().relations().next_offer_id();
    w.exec(Command::ProposeDeal { from: P0, to: P1, items: vec![(P0, DealItem::Friendship)] }).unwrap();
    w.exec(Command::RespondDeal { player: P1, offer: id, accept: true }).unwrap();
    let auth = w.game.view().auth;
    assert_eq!(friendship_rate(auth, &w.rules, P0), 10);
    assert_eq!(friendship_rate(auth, &w.rules, P1), 10, "Sweden's friend gains too");
}

#[test]
fn entrepreneurship_doubles_the_gold_of_trade_missions() {
    let mission = |policy: bool| {
        let mut w = World::new("core:city-state/byblos");
        if policy {
            w.policy(P0, "core:policy/commerce/entrepreneurship");
        }
        w.until_active(P0);
        let at = w.tile(5, 12);
        let merchant = w.spawn(P0, at, "core:unit/great-merchant");
        let (gold, influence) = (w.game.view().auth.players().treasury_of(P0), w.influence(P0));
        w.exec(Command::GreatPersonAction {
            unit: merchant,
            action: classic_rules::derived::great_people::GreatPersonAction::TradeMission,
        })
        .unwrap();
        (w.game.view().auth.players().treasury_of(P0) - gold, w.influence(P0) - influence)
    };
    // Unciv: UnitActionsGreatPerson.kt, (350 + 50 × era) × speed, +30 influence.
    assert_eq!(mission(false), (350, 30), "350 + 50 × era, +30 influence");
    assert_eq!(mission(true), (700, 30));
}

#[test]
fn the_merchant_of_venice_doubles_gold_and_influence_of_a_trade_mission() {
    let mission = |kind: &str| {
        let mut w = World::new("core:city-state/byblos");
        w.until_active(P0);
        let at = w.tile(5, 12);
        let merchant = w.spawn(P0, at, kind);
        let (gold, influence) = (w.game.view().auth.players().treasury_of(P0), w.influence(P0));
        w.exec(Command::GreatPersonAction {
            unit: merchant,
            action: classic_rules::derived::great_people::GreatPersonAction::TradeMission,
        })
        .unwrap();
        (w.game.view().auth.players().treasury_of(P0) - gold, w.influence(P0) - influence)
    };
    assert_eq!(mission("core:unit/great-merchant"), (350, 30));
    assert_eq!(mission("core:unit/merchant-of-venice"), (700, 60));
}

#[test]
fn the_merchant_of_venice_buys_the_city_state_as_a_puppet() {
    let mut w = World::new("core:city-state/byblos");
    w.until_active(P0);
    let at = w.tile(5, 12);
    let ordinary = w.spawn(P0, at, "core:unit/great-merchant");
    let refused = w.exec(Command::GreatPersonAction {
        unit: ordinary,
        action: classic_rules::derived::great_people::GreatPersonAction::BuyCityState,
    });
    assert!(refused.unwrap_err().contains("cannot buy a city-state"));
    let venice = w.spawn(P0, at, "core:unit/merchant-of-venice");
    let cities = w.game.view().auth.indexes().cities_of(P0).len();
    w.exec(Command::GreatPersonAction {
        unit: venice,
        action: classic_rules::derived::great_people::GreatPersonAction::BuyCityState,
    })
    .unwrap();
    let auth = w.game.view().auth;
    assert!(auth.units().get(venice).is_none(), "the merchant is used up");
    assert_eq!(auth.indexes().cities_of(P0).len(), cities + 1);
    let bought = auth.cities().get(w.minor).unwrap();
    assert_eq!(bought.owner, P0);
    assert_eq!(bought.status, classic_rules::world::city::CityStatus::Puppet);
    w.game.validate(&w.rules).unwrap();
}

#[test]
fn a_nau_sells_its_cargo_once_next_to_a_foreign_land() {
    let mut w = World::new("core:city-state/byblos");
    w.nation(P0, "core:nation/portugal");
    w.until_active(P0);
    let sea: Vec<(TileId, TerrainId)> = (9..13).map(|q| (w.tile(q, 5), TerrainId(4))).collect();
    w.game.debug_set_terrain_batch(&w.rules, &sea);
    let nau_at = |w: &mut World, q| w.spawn(P0, w.tile(q, 5), "core:unit/nau");
    let home_waters = nau_at(&mut w, 9);
    let refused = w.exec(Command::SellExoticGoods { unit: home_waters });
    assert!(refused.unwrap_err().contains("foreign land"), "open sea sells nothing");
    let nau = nau_at(&mut w, 11);
    let (gold, xp) = (w.game.view().auth.players().treasury_of(P0), w.game.view().auth.units().get(nau).unwrap().core.experience);
    w.exec(Command::SellExoticGoods { unit: nau }).unwrap();
    let auth = w.game.view().auth;
    let earned = auth.players().treasury_of(P0) - gold;
    assert!((100..=400).contains(&earned), "{earned}");
    assert!(auth.units().get(nau).unwrap().core.experience > xp);
    assert_eq!(auth.units().get(nau).unwrap().core.moves_left, 0);
    // The cargo is gone: a later turn cannot sell again.
    w.round();
    let again = w.exec(Command::SellExoticGoods { unit: nau });
    assert!(again.unwrap_err().contains("no cargo"), "the cargo is sold only once");
    // The farther from the capital, the more it pays.
    let far = nau_at(&mut w, 12);
    let before = w.game.view().auth.players().treasury_of(P0);
    w.exec(Command::SellExoticGoods { unit: far }).unwrap();
    assert!(w.game.view().auth.players().treasury_of(P0) - before > earned);
    w.game.validate(&w.rules).unwrap();
}

// ---- tourism ----

#[test]
fn futurism_gives_tourism_with_every_known_nation_when_a_writer_is_born() {
    let mut w = World::new("core:city-state/byblos");
    let on_p1 = |w: &World| w.game.view().auth.module().tourism().player(P0).influence_on(P1);
    let at = w.city_tile(w.home);
    w.spawn(P0, at, "core:unit/great-writer");
    assert_eq!(on_p1(&w), 0);
    w.policy(P0, "core:policy/autocracy/futurism");
    w.spawn(P0, at, "core:unit/great-writer");
    assert_eq!(on_p1(&w), 200, "+200 with each valid nation");
    w.spawn(P0, at, "core:unit/great-scientist");
    assert_eq!(on_p1(&w), 200, "not for a scientist");
    w.spawn(P0, at, "core:unit/great-musician");
    assert_eq!(on_p1(&w), 400);
    assert_eq!(w.game.view().auth.module().tourism().player(P0).influence_on(CS), 0, "nations only");
}
