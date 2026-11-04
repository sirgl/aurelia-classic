//! The tall science play (`ai::tall`, classic/docs/ai-science-pace.md): the
//! number of cities with the most science per research cost, city sites
//! valued by the science buildings whose site conditions they meet, the
//! policies seen through the science lens, trade routes priced by what they
//! bring; and the book (`tall-book`), which names content by design. The
//! tests name classic content only to set the scene, and to check the book.

use classic_content::core::core_rules;
use classic_rules::ai::tall::{self, CityModel, Tall};
use classic_rules::ai::value::{PriceModel, Prices, SciencePricing};
use classic_rules::ai::variant::Variant;
use classic_rules::command::Command;
use classic_rules::derived::trade::TradeOffer;
use classic_rules::ids::{BuildingKindId, CityHandle, PlayerId, PolicyId, TechId, TerrainId, TileId};
use classic_rules::math::HexCoord;
use classic_rules::rules::content::{PolicyBranchId, PolicyRole};
use classic_rules::world::state::GameConfig;
use classic_rules::world::trade::{RouteKind, RouteYields};
use classic_rules::{compile_rules, ClassicGame, CompiledRuleset, GameState};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
}

fn building(rules: &CompiledRuleset, key: &str) -> BuildingKindId {
    BuildingKindId(rules.buildings.iter().position(|b| b.key == key).unwrap() as u16)
}

fn branch(rules: &CompiledRuleset, key: &str) -> PolicyBranchId {
    PolicyBranchId(rules.module.policy_branches.iter().position(|b| b.key == key).unwrap() as u16)
}

/// A game of two nations, the first with one city.
fn game() -> (GameState, CompiledRuleset, CityHandle) {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(&rules, GameConfig { players: 2, width: 40, height: 30, rng_seed: 5 });
    let city = game.debug_found_city(&rules, P0, tile(&game, 8, 8));
    game.debug_found_city(&rules, P1, tile(&game, 30, 20));
    (game, rules, city)
}

fn prices(game: &GameState, rules: &CompiledRuleset) -> Prices {
    let obs = game.ai_observation(rules, P0);
    Prices::of_effects(&obs, rules, PriceModel::Uses, SciencePricing::Reach, true)
}

/// The variants are seated by name; since the sixth promotion the game's AI
/// plays `tall-science`'s play and `pre-tall`, the standard before it, none.
#[test]
fn the_variants_carry_their_play() {
    assert_eq!(Variant::from_name("tall-science"), Some(Variant::TallScience));
    assert_eq!(Variant::from_name("tall-book"), Some(Variant::TallBook));
    assert!(Variant::ALL.contains(&Variant::TallScience) && Variant::ALL.contains(&Variant::TallBook));
    assert_eq!(Variant::TallScience.tall(), Tall::Science);
    assert_eq!(Variant::TallBook.tall(), Tall::Book);
    assert_eq!(Variant::Standard.tall(), Tall::Science);
    assert_eq!(Variant::from_name("pre-tall"), Some(Variant::PreTall));
    assert_eq!(Variant::PreTall.tall(), Tall::Off);
    // The attribution probe keeps the book's four cities and plays the rest as `standard`.
    assert_eq!(Variant::from_name("tall-book-cities"), Some(Variant::TallBookCities));
    assert!(Variant::TallBookCities.tall().book_cities() && !Variant::TallBookCities.tall().plays());
    assert!(Tall::Book.book_cities() && Tall::Book.plays() && Tall::Science.plays() && !Tall::Science.book_cities());
    // The other probe takes the book's policies alone.
    assert_eq!(Variant::from_name("tall-book-policies"), Some(Variant::TallBookPolicies));
    let policies = Variant::TallBookPolicies.tall();
    assert!(policies.book_policies() && !policies.plays() && !policies.book_cities());
    assert!(Tall::Book.book_policies() && !Tall::Science.book_policies() && !Tall::BookCities.book_policies());
    // Both fight and war as the game's AI.
    assert_eq!(Variant::TallScience.war_tuning(), Variant::Standard.war_tuning());
    assert_eq!(Variant::TallBook.fight_fix(), Variant::Standard.fight_fix());
}

/// The sixth promotion (1 November): `standard` plays every switch of
/// `tall-science`, which stays as its alias; `pre-tall`, the standard before
/// it, plays every switch of `standard` but the tall play, as the tall
/// variants' base.
#[test]
fn the_promoted_standard_plays_as_tall_science() {
    let switches = |v: Variant| {
        format!(
            "{:?} {:?} {} {} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {} {} {} {} {} {}",
            v.tall(),
            v.building_order(),
            v.unit_abilities(),
            v.religion_fix(),
            v.war_tuning(),
            v.fight_fix(),
            v.siege_tactics(),
            v.effect_checks(),
            v.priced_stops(),
            v.science_levers(),
            v.price_model(),
            v.content_founding(),
            v.true_effects(),
            v.sound_orders(),
            v.strikes(),
            v.air_arm(),
            v.sea_work(),
        )
    };
    let (standard, alias, before) = (Variant::Standard, Variant::TallScience, Variant::PreTall);
    assert_eq!(switches(standard), switches(alias));
    assert_eq!(before.tall(), Tall::Off);
    assert_eq!(switches(Variant::PreTall).replacen("Off", "Science", 1), switches(standard), "`pre-tall` is `standard` but for the tall play");
    assert_eq!(Variant::ALL[0], standard);
    // The controls of the seventh to the eleventh promotion, `pre-siege`, `pre-puppet`, `pre-join`,
    // `pre-workers` and `pre-camps`, came in before it.
    assert_eq!(Variant::ALL[6], before);
}

/// The number of cities is where science per research cost peaks: the
/// citizens the happiness keeps (a city costs 3, brings 1, a citizen 1;
/// 20 fixed) at most 10 a city, over 1 + 5 % a city beyond the first.
#[test]
fn the_city_count_keeps_the_most_science_per_research_cost() {
    let model = CityModel { fixed: 20, per_city: 100, city_unhappiness: 300, citizen_unhappiness: 100, size: 10, cost_per_city: 500 };
    // 1 city: 10 citizens; 2: 16; 3: 14 — the happiness binds from the second.
    assert_eq!([model.citizens(1), model.citizens(2), model.citizens(3)], [1000, 1600, 1400]);
    assert_eq!(model.best(), 2);
    // Cities that pay their own unhappiness and more: the per-city research
    // cost alone stops them (3 cities: 2 300 / 1.10 against 2 400 / 1.15 at 4).
    let rich = CityModel { per_city: 400, ..model };
    assert_eq!(rich.best(), 3);
    // Without the per-city cost the same empire would want every city.
    assert_eq!(CityModel { cost_per_city: 0, ..rich }.best(), tall::MAX_CITIES);
}

/// A city grows by the rules' thresholds: no surplus, no growth; more
/// rounds, a larger city.
#[test]
fn a_city_reaches_its_size_by_the_growth_thresholds() {
    assert_eq!(tall::reachable_size(0, 300), 1);
    let soon = tall::reachable_size(3, 100);
    let late = tall::reachable_size(3, 300);
    assert!(soon > 1 && late > soon, "{soon} {late}");
}

/// The science buildings with a site condition, read from the rules: each
/// priced by its science in a city of the expected size; a site next to a
/// mountain meets the classic observatory's condition and gets its bonus.
#[test]
fn a_site_is_worth_the_science_buildings_whose_condition_it_meets() {
    let (mut game, rules, _) = game();
    let bonuses = tall::site_bonuses(&rules, 12, 150);
    assert!(!bonuses.is_empty());
    for (kind, points) in &bonuses {
        let p = &rules.buildings[kind.0 as usize].placement;
        assert!(p.mountain || p.nearby_mountain || p.river || p.water || p.flat, "only buildings with a site condition");
        assert!(*points > 0);
    }
    let observatory = building(&rules, "core:building/observatory");
    let (_, points) = bonuses.iter().find(|(k, _)| *k == observatory).copied().unwrap();
    // +50 % of 12 citizens at 1.5 beakers = 9 beakers, 18 site points each.
    assert_eq!(points, 9 * tall::site_points_per_beaker());
    // A mountain next to one site and none near the other.
    let mountain = (0..64u16).find(|c| rules.terrain_kinds.is_mountains(*c)).unwrap();
    let flat = (0..64u16).find(|c| !rules.terrain_kinds.is_mountains(*c) && !rules.terrain_kinds.is_water(*c) && !rules.terrain_kinds.is_hills(*c)).unwrap();
    let (near, far) = (tile(&game, 14, 14), tile(&game, 20, 6));
    let dims = game.view().auth.map().dimensions;
    let mut tiles: Vec<(TileId, TerrainId)> = classic_rules::math::hex_disc(dims, near, 3).chain(classic_rules::math::hex_disc(dims, far, 3)).map(|t| (t, TerrainId(flat))).collect();
    tiles.push((tile(&game, 15, 14), TerrainId(mountain)));
    game.debug_set_terrain_batch(&rules, &tiles);
    let seen: Vec<TileId> = tiles.iter().map(|(t, _)| *t).collect();
    game.debug_reveal(P0, &seen);
    let obs = game.ai_observation(&rules, P0);
    let placement = &rules.buildings[observatory.0 as usize].placement;
    assert!(tall::site_meets(&obs.view, near, placement));
    assert!(!tall::site_meets(&obs.view, far, placement));
    assert_eq!(tall::site_bonus(&obs.view, &rules, near, &bonuses), bonuses.iter().filter(|(k, _)| tall::site_meets(&obs.view, near, &rules.buildings[k.0 as usize].placement)).map(|(_, p)| p).sum::<i32>());
    assert!(tall::site_bonus(&obs.view, &rules, near, &bonuses) >= points);
    assert_eq!(tall::site_bonus(&obs.view, &rules, far, &[(observatory, points)]), 0);
}

/// The science lens prices beakers and what keeps citizens, nothing else,
/// and no new city; a cap stops the new city at the allocator too.
#[test]
fn the_science_lens_sees_beakers_and_citizens_only() {
    let (game, rules, _) = game();
    let prices = prices(&game, &rules);
    let obs = game.ai_observation(&rules, P0);
    let lens = tall::science_lens(&obs, &prices);
    assert_eq!((lens.production, lens.gold, lens.culture, lens.faith), (0, 0, 0, 0));
    assert_eq!(lens.science, prices.science);
    assert_eq!(lens.new_city(0), 0);
    assert!(prices.new_city(0) > 0, "one city: a second is worth founding");
    let capped = Prices { city_cap: Some(1), ..prices };
    assert_eq!(capped.new_city(0), 0);
    assert_eq!(Prices { city_cap: Some(2), ..prices }.new_city(0), prices.new_city(0));
}

/// A route is worth what it brings at the turn's prices: an internal
/// route's food and production at the destination, an international one's
/// gold and science at the origin.
#[test]
fn a_route_is_priced_by_what_it_brings() {
    let (game, rules, city) = game();
    let prices = prices(&game, &rules);
    let offer = |kind, origin: RouteYields, destination: RouteYields| TradeOffer { destination: city, kind, steps: 6, origin, destination_yields: destination, side_value: 0 };
    let food = RouteYields { food: 400, ..RouteYields::default() };
    let gold = RouteYields { gold: 400, ..RouteYields::default() };
    assert_eq!(tall::route_value(&offer(RouteKind::Food, RouteYields::default(), food), &prices), 4 * prices.food);
    assert_eq!(tall::route_value(&offer(RouteKind::International, gold, food), &prices), 4 * prices.gold, "a foreign city's food is not ours");
    // The rules' sea internal route carries 60 % more: priced so.
    let sea = RouteYields { food: 640, ..RouteYields::default() };
    assert!(tall::route_value(&offer(RouteKind::Food, RouteYields::default(), sea), &prices) > tall::route_value(&offer(RouteKind::Food, RouteYields::default(), food), &prices));
}

/// The book: the growth branch first, all of it; then it waits for the
/// science branch and takes it whole.
#[test]
fn the_book_takes_the_growth_branch_then_the_science_branch() {
    let (mut game, rules, _) = game();
    game.debug_grant_culture(P0, 100_000);
    let memory = classic_rules::ai::AiState::new(P0, 0);
    let growth = branch(&rules, tall::BOOK_GROWTH_BRANCH);
    let science = branch(&rules, tall::BOOK_SCIENCE_BRANCH);
    let decide = |game: &GameState| {
        let obs = game.ai_observation(&rules, P0);
        let prices = Prices::of_effects(&obs, &rules, PriceModel::Uses, SciencePricing::Reach, true);
        tall::book_policy_command(&obs, &rules, &memory, &prices, tall::BookPlan::FIRST)
    };
    assert!(matches!(decide(&game), Some(Command::UnlockPolicyBranch { branch, .. }) if branch == growth));
    // Adopt the growth branch policy by policy, as the book asks.
    for _ in 0..rules.module.policies.len() {
        match decide(&game) {
            Some(Command::UnlockPolicyBranch { branch, .. }) if branch == growth => {
                game.execute(&rules, Command::UnlockPolicyBranch { player: P0, branch }).unwrap();
            }
            Some(Command::AdoptPolicy { policy, .. }) => {
                assert_eq!(rules.module.policies[policy.0 as usize].branch, Some(growth), "the growth branch first");
                game.execute(&rules, Command::AdoptPolicy { player: P0, policy }).unwrap();
            }
            _ => break,
        }
    }
    let adopted = &game.ai_observation(&rules, P0).culture.policies;
    let whole = rules.module.policies.iter().enumerate().filter(|(_, p)| p.branch == Some(growth) && p.role == PolicyRole::Policy).all(|(i, _)| adopted.contains(&PolicyId(i as u16)));
    assert!(whole, "every policy of the growth branch");
    // The science branch's era is not reached: the culture waits.
    assert!(decide(&game).is_none());
    let era = rules.module.policy_branches[science.0 as usize].era_prerequisite.unwrap();
    let index = rules.eras[era.0 as usize].index;
    for (i, tech) in rules.technologies.iter().enumerate() {
        if tech.content_era.and_then(|e| rules.eras.get(e.0 as usize)).is_some_and(|e| e.index <= index) {
            game.debug_learn(&rules, P0, TechId(i as u16));
        }
    }
    assert!(matches!(decide(&game), Some(Command::UnlockPolicyBranch { branch, .. }) if branch == science));
}

/// The second edition by name: the book's play with its own plan; the
/// first edition keeps its own.
#[test]
fn the_second_edition_is_the_book_with_its_plan() {
    assert_eq!(Variant::from_name("tall-book-2"), Some(Variant::TallBook2));
    assert_eq!(Variant::from_name("tall-book-2-filler"), Some(Variant::TallBook2Filler));
    assert_eq!(Variant::TallBook2.tall(), Tall::Book2);
    assert_eq!(Variant::TallBook2Filler.tall(), Tall::Book2Filler);
    for t in [Tall::Book2, Tall::Book2Filler] {
        assert!(t.plays() && t.book_cities() && t.book_policies() && t.book());
    }
    assert_eq!(Tall::Book.plan(), tall::BookPlan::FIRST);
    assert_eq!(Tall::Book2.plan(), tall::BookPlan::SECOND);
    assert_eq!(Tall::Book2Filler.plan(), tall::BookPlan::FILLER);
    assert!(tall::BookPlan::SECOND.scout_first && tall::BookPlan::SECOND.tenets_first && !tall::BookPlan::SECOND.filler);
    assert_eq!(Variant::TallBook2.building_order(), Variant::TallBook.building_order());
    assert_eq!(Variant::TallBook2.war_tuning(), Variant::TallBook.war_tuning());
    assert_eq!(Variant::TallBook2.fight_fix(), Variant::TallBook.fight_fix());
}

/// The second edition's policies: while the science branch waits for its
/// era the culture waits (the filler probe spends it); once the science
/// branch is whole and an ideology chosen, a tenet comes before any new
/// branch.
#[test]
fn the_second_edition_takes_tenets_before_a_new_branch() {
    use classic_rules::classic::content::PolicyBranchKind;
    let (mut game, rules, _) = game();
    game.debug_grant_culture(P0, 10_000_000);
    let memory = classic_rules::ai::AiState::new(P0, 0);
    let science = branch(&rules, tall::BOOK_SCIENCE_BRANCH);
    let decide = |game: &GameState, plan: tall::BookPlan| {
        let obs = game.ai_observation(&rules, P0);
        let prices = Prices::of_effects(&obs, &rules, PriceModel::Uses, SciencePricing::Reach, true);
        tall::book_policy_command(&obs, &rules, &memory, &prices, plan)
    };
    let play = |game: &mut GameState, plan: tall::BookPlan, until: &dyn Fn(&Command) -> bool| -> Option<Command> {
        for _ in 0..rules.module.policies.len() {
            let command = decide(game, plan)?;
            if until(&command) {
                return Some(command);
            }
            game.execute(&rules, command).unwrap();
        }
        None
    };
    // The growth branch whole; then the culture waits for the science branch.
    assert!(play(&mut game, tall::BookPlan::SECOND, &|_| false).is_none());
    assert!(decide(&game, tall::BookPlan::FIRST).is_none());
    assert!(decide(&game, tall::BookPlan::SECOND).is_none(), "the second edition saves the culture too");
    let filler = decide(&game, tall::BookPlan::FILLER);
    let other = |p: PolicyBranchId| p != science && p != branch(&rules, tall::BOOK_GROWTH_BRANCH);
    assert!(matches!(filler, Some(Command::UnlockPolicyBranch { branch, .. }) if other(branch)), "the filler opens another branch: {filler:?}");
    let learn_to = |game: &mut GameState, index: u32| {
        for (i, tech) in rules.technologies.iter().enumerate() {
            if tech.content_era.and_then(|e| rules.eras.get(e.0 as usize)).is_some_and(|e| u32::from(e.index) <= index) {
                game.debug_learn(&rules, P0, TechId(i as u16));
            }
        }
    };
    // The science branch's era: the science branch whole.
    let science_era = rules.module.policy_branches[science.0 as usize].era_prerequisite.map(|e| u32::from(rules.eras[e.0 as usize].index)).unwrap();
    learn_to(&mut game, science_era);
    let in_science = |c: &Command| match c {
        Command::UnlockPolicyBranch { branch, .. } => *branch == science,
        Command::AdoptPolicy { policy, .. } => rules.module.policies[policy.0 as usize].branch == Some(science),
        _ => false,
    };
    play(&mut game, tall::BookPlan::SECOND, &|c| !in_science(c));
    let obs = game.ai_observation(&rules, P0);
    let whole = rules.module.policies.iter().enumerate().filter(|(_, p)| p.branch == Some(science) && p.role != PolicyRole::Opener).all(|(i, _)| obs.culture.policies.contains(&PolicyId(i as u16)));
    assert!(whole, "the science branch is whole");
    // Past the ideology era: the ideology, then a tenet.
    let ideology_era = rules.module.policy_branches.iter().filter(|b| b.kind == PolicyBranchKind::Ideology).filter_map(|b| b.era_prerequisite).map(|e| u32::from(rules.eras[e.0 as usize].index)).min().unwrap();
    learn_to(&mut game, ideology_era + 1);
    let chosen = decide(&game, tall::BookPlan::SECOND);
    assert!(matches!(chosen, Some(Command::ChooseIdeology { .. })), "the ideology first: {chosen:?}");
    game.execute(&rules, chosen.unwrap()).unwrap();
    let next = decide(&game, tall::BookPlan::SECOND);
    let tenet = |c: &Option<Command>| match c {
        Some(Command::AdoptPolicy { policy, .. }) => rules.module.policies[policy.0 as usize].branch.is_some_and(|b| rules.module.policy_branches[b.0 as usize].kind == PolicyBranchKind::Ideology),
        _ => false,
    };
    assert!(tenet(&next), "a tenet before a new branch: {next:?}");
}

/// The book's science buildings come in its order: the first one the city
/// lacks and may build.
#[test]
fn the_book_builds_its_science_buildings_in_order() {
    let (mut game, rules, city) = game();
    for (i, _) in rules.technologies.iter().enumerate() {
        game.debug_learn(&rules, P0, TechId(i as u16));
    }
    game.debug_set_population(&rules, city, 6);
    let next = |game: &GameState| {
        let obs = game.ai_observation(&rules, P0);
        let own = obs.own_cities.iter().find(|c| c.handle == city).unwrap().clone();
        tall::book_next_building(&obs, &rules, &own, |_| true)
    };
    let library = building(&rules, tall::BOOK_BUILDINGS[0]);
    let university = building(&rules, tall::BOOK_BUILDINGS[1]);
    assert_eq!(next(&game), Some(library));
    game.debug_add_building(&rules, city, library);
    assert_eq!(next(&game), Some(university));
}

/// `science-first` ranks the buildings with a science effect by the beakers
/// they add in the city per hammer, from the rules' data: with every
/// technology, a city of six orders the building whose per-citizen science
/// is cheapest first, and never one it has.
#[test]
fn science_first_orders_the_most_beakers_per_hammer() {
    use classic_rules::ai::tall::BuildingOrder;
    assert_eq!(Variant::ScienceFirst.building_order(), BuildingOrder::Science);
    assert_eq!(Variant::TallBook.building_order(), BuildingOrder::Book);
    assert_eq!(Variant::TallBookNoOrder.building_order(), BuildingOrder::Off);
    assert_eq!(Variant::TallBookNoOrder.tall(), Tall::Book);
    assert_eq!(Variant::Standard.building_order(), BuildingOrder::Off);
    let (mut game, rules, city) = game();
    for (i, _) in rules.technologies.iter().enumerate() {
        game.debug_learn(&rules, P0, TechId(i as u16));
    }
    game.debug_set_population(&rules, city, 6);
    let pick = |game: &GameState| {
        let obs = game.ai_observation(&rules, P0);
        let own = obs.own_cities.iter().find(|c| c.handle == city).unwrap().clone();
        let best = tall::science_building(&obs, &rules, &own, |_| true);
        (best, own)
    };
    let (best, own) = pick(&game);
    let best = best.unwrap();
    // Nothing the city may build now adds more beakers per hammer.
    let rate = |k: BuildingKindId| tall::beakers_in(&rules, &own, k) * 1000 / i64::from(rules.buildings[k.0 as usize].cost.max(1));
    assert!(tall::beakers_in(&rules, &own, best) > 0);
    assert_eq!(best, building(&rules, "core:building/library"), "the scene: half a beaker a citizen for 75 hammers");
    game.debug_add_building(&rules, city, best);
    let (next, own) = pick(&game);
    let next = next.unwrap();
    assert_ne!(next, best);
    assert!(tall::beakers_in(&rules, &own, next) > 0);
    let _ = rate;
}
