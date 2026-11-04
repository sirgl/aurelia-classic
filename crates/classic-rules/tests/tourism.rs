//! Great works, tourism, influence and public opinion on the classic ruleset
//! (classic/docs/classic/rules.md):
//! works in slots and their culture and tourism, theming, the Parthenon's
//! free work, the treatise and the concert, influence each turn and its
//! levels, the cultural victory query, public opinion and the ideology
//! switch, save/load.

use classic_rules::ClassicGame;
use classic_content::core::core_rules;
use classic_rules::derived::income::city_income;
use classic_rules::derived::tourism::{
    city_tourism, has_cultural_victory, influence_level, influence_per_turn, met, player_tourism,
    InfluenceLevel,
};
use classic_rules::ids::{
    BuildingKindId, CityHandle, PlayerId, ProductionItem, TechId, TileId, UnitHandle, UnitKindId,
};
use classic_rules::persistence::{decode::decode, encode::encode};
use classic_rules::replay::authoritative_hash;
use classic_rules::rules::content::PolicyBranchId;
use classic_rules::world::tourism::{GreatWorkClass, PublicOpinion};
use classic_rules::{compile_rules, Command, CompiledRuleset, GameConfig, GameState};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn rules() -> CompiledRuleset {
    compile_rules(core_rules()).unwrap()
}

fn world(rules: &CompiledRuleset, players: u8) -> GameState {
    GameState::new(
        rules,
        GameConfig {
            players,
            width: 40,
            height: 24,
            rng_seed: 7,
        },
    )
}

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    let dims = game.view().auth.map().dimensions;
    dims.to_tile(classic_rules::math::HexCoord { q, r }).unwrap()
}

fn building(rules: &CompiledRuleset, key: &str) -> BuildingKindId {
    BuildingKindId(rules.buildings.iter().position(|b| b.key == key).unwrap() as u16)
}

fn unit_kind(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap() as u16)
}

fn tech(rules: &CompiledRuleset, key: &str) -> TechId {
    TechId(
        rules
            .technologies
            .iter()
            .position(|t| t.key == key)
            .unwrap() as u16,
    )
}

fn branch(rules: &CompiledRuleset, key: &str) -> PolicyBranchId {
    PolicyBranchId(
        rules
            .module.policy_branches
            .iter()
            .position(|b| b.key == key)
            .unwrap() as u16,
    )
}

fn spawn(
    game: &mut GameState,
    rules: &CompiledRuleset,
    player: PlayerId,
    at: TileId,
    key: &str,
) -> UnitHandle {
    game.debug_spawn_unit(rules, player, at, unit_kind(rules, key))
}

/// Every alive player ends its turn once.
fn round(game: &mut GameState, rules: &CompiledRuleset) {
    let players = game.view().auth.players().count();
    for _ in 0..players {
        let player = game.view().auth.active_player();
        game.execute(rules, Command::EndTurn { player }).unwrap();
    }
}

/// P0's capital at (8, 10) and P1's at (14, 10): they see each other.
fn two_nations(rules: &CompiledRuleset) -> (GameState, CityHandle, CityHandle) {
    let mut game = world(rules, 2);
    let a = tile(&game, 8, 10);
    let b = tile(&game, 14, 10);
    let ca = game.debug_found_city(rules, P0, a);
    let cb = game.debug_found_city(rules, P1, b);
    {
        let t = tile(&game, 12, 10);
        spawn(&mut game, rules, P0, t, "core:unit/warrior")
    };
    {
        let t = tile(&game, 10, 10);
        spawn(&mut game, rules, P1, t, "core:unit/warrior")
    };
    round(&mut game, rules);
    (game, ca, cb)
}

fn end_turn(game: &mut GameState, rules: &CompiledRuleset, player: PlayerId) {
    assert_eq!(game.view().auth.active_player(), player);
    game.execute(rules, Command::EndTurn { player }).unwrap();
}

fn culture_of(game: &GameState, rules: &CompiledRuleset, city: CityHandle) -> u32 {
    let core = game.view().auth.cities().get(city).unwrap();
    city_income(game.view().auth, rules, core).culture
}

#[test]
fn a_great_writer_fills_the_nearest_literature_slot() {
    let rules = rules();
    let (mut game, capital, _) = two_nations(&rules);
    game.debug_add_building(
        &rules,
        capital,
        building(&rules, "core:building/amphitheater"),
    );
    let before = culture_of(&game, &rules, capital);
    let at = game.view().auth.cities().get(capital).unwrap().tile;
    let writer = spawn(&mut game, &rules, P0, at, "core:unit/great-writer");
    game.execute(&rules, Command::CreateGreatWork { unit: writer })
        .unwrap();
    assert!(
        game.view().auth.units().get(writer).is_none(),
        "the writer is spent"
    );
    let works = &game.view().auth.module().tourism().city(capital).works;
    assert_eq!(works.len(), 1);
    let work = game
        .view()
        .auth
        .module()
        .tourism()
        .work(works[0].work)
        .unwrap();
    assert_eq!((work.class, work.creator), (GreatWorkClass::Literature, P0));
    assert_eq!(
        culture_of(&game, &rules, capital),
        before + classic_rules::derived::tourism::work_culture(work.era),
        "culture of a work of its era (derived/tourism.rs `work_culture`)"
    );
    assert_eq!(
        city_tourism(game.view().auth, &rules, capital).total,
        classic_rules::derived::tourism::work_tourism(work.era),
        "tourism of a work of its era (`work_tourism`)"
    );
    let second = spawn(&mut game, &rules, P0, at, "core:unit/great-writer");
    let err = game
        .execute(&rules, Command::CreateGreatWork { unit: second })
        .unwrap_err();
    assert!(
        format!("{err:?}").contains("no free great work slot"),
        "{err:?}"
    );
    let far = {
        let t = tile(&game, 2, 2);
        spawn(&mut game, &rules, P0, t, "core:unit/great-artist")
    };
    let err = game
        .execute(&rules, Command::CreateGreatWork { unit: far })
        .unwrap_err();
    assert!(
        format!("{err:?}").contains("next to one of your cities"),
        "{err:?}"
    );
    game.validate(&rules).unwrap();
}

#[test]
fn a_full_museum_of_own_art_of_one_era_is_themed() {
    let rules = rules();
    let (mut game, capital, _) = two_nations(&rules);
    let museum = building(&rules, "core:building/museum");
    game.debug_add_building(&rules, capital, museum);
    let at = game.view().auth.cities().get(capital).unwrap().tile;
    // The Palace's art slot is in the city's building list first.
    for _ in 0..3 {
        let artist = spawn(&mut game, &rules, P0, at, "core:unit/great-artist");
        game.execute(&rules, Command::CreateGreatWork { unit: artist })
            .unwrap();
    }
    let in_museum = game
        .view()
        .auth
        .module()
        .tourism()
        .city(capital)
        .count_in(museum);
    assert_eq!(in_museum, 2);
    // Museum theme 1: same era, art only, all by the owner: +2 culture and tourism.
    let tourism = city_tourism(game.view().auth, &rules, capital);
    assert_eq!(tourism.theming, 2);
    assert_eq!(tourism.total, 3 * 2 + 2);
    game.validate(&rules).unwrap();
}

#[test]
fn the_parthenon_comes_with_its_frieze() {
    let rules = rules();
    let (mut game, capital, _) = two_nations(&rules);
    game.debug_learn(&rules, P0, tech(&rules, "core:tech/drama-and-poetry"));
    let parthenon = building(&rules, "core:building/parthenon");
    game.execute(
        &rules,
        Command::EnqueueProduction {
            player: P0,
            city: capital,
            item: ProductionItem::Building(parthenon),
        },
    )
    .unwrap();
    for _ in 0..200 {
        if game
            .view()
            .auth
            .cities()
            .get(capital)
            .unwrap()
            .buildings
            .contains(&parthenon)
        {
            break;
        }
        round(&mut game, &rules);
    }
    let city = game.view().auth.module().tourism().city(capital);
    assert_eq!(city.count_in(parthenon), 1, "the frieze in the Parthenon");
    let work = game
        .view()
        .auth
        .module()
        .tourism()
        .work(city.works[0].work)
        .unwrap();
    assert_eq!((work.class, work.creator), (GreatWorkClass::Art, P0));
}

#[test]
fn the_treatise_gives_the_culture_of_the_last_turns() {
    let rules = rules();
    let (mut game, capital, _) = two_nations(&rules);
    for _ in 0..10 {
        round(&mut game, &rules);
    }
    let state = game.view().auth.module().culture().of(P0).clone();
    let expected = state.recent_culture(8);
    assert!(expected > 0);
    let at = game.view().auth.cities().get(capital).unwrap().tile;
    let writer = spawn(&mut game, &rules, P0, at, "core:unit/great-writer");
    game.execute(&rules, Command::WriteTreatise { unit: writer })
        .unwrap();
    let after = game.view().auth.module().culture().of(P0);
    assert_eq!(after.culture, state.culture + expected);
    assert_eq!(after.lifetime, state.lifetime + u64::from(expected));
}

#[test]
fn influence_grows_each_turn_and_reaches_levels() {
    let rules = rules();
    let (mut game, capital, _) = two_nations(&rules);
    assert!(met(game.view().auth, P0, P1));
    game.debug_add_building(
        &rules,
        capital,
        building(&rules, "core:building/amphitheater"),
    );
    let at = game.view().auth.cities().get(capital).unwrap().tile;
    let writer = spawn(&mut game, &rules, P0, at, "core:unit/great-writer");
    game.execute(&rules, Command::CreateGreatWork { unit: writer })
        .unwrap();
    let per_turn = influence_per_turn(game.view().auth, &rules, P0, P1);
    assert_eq!(
        per_turn,
        u64::from(player_tourism(game.view().auth, &rules, P0))
    );
    let before = game
        .view()
        .auth
        .module()
        .tourism()
        .player(P0)
        .influence_on(P1);
    round(&mut game, &rules);
    let after = game
        .view()
        .auth
        .module()
        .tourism()
        .player(P0)
        .influence_on(P1);
    assert_eq!(after, classic_rules::derived::tourism::faded(before) + per_turn, "forgetting, then the new tourism");
    let permille = classic_rules::derived::tourism::influence_permille(game.view().auth, P0, P1).unwrap();
    let level = influence_level(game.view().auth, P0, P1).unwrap();
    // Own thresholds (derived/tourism.rs `level_threshold`): per mille of the target's culture of its last 8 turns.
    let expected = match permille {
        0..=749 => InfluenceLevel::Unknown,
        750..=2324 => InfluenceLevel::Noticed,
        2325..=4724 => InfluenceLevel::Admired,
        4725..=7949 => InfluenceLevel::Imitated,
        7950..=11999 => InfluenceLevel::Embraced,
        _ => InfluenceLevel::Prevailing,
    };
    assert_eq!(level, expected, "influence {after} is {permille} per mille");
}

#[test]
fn a_concert_wins_the_cultural_victory_query_over_a_young_nation() {
    let rules = rules();
    let (mut game, _, rival) = two_nations(&rules);
    assert!(!has_cultural_victory(game.view().auth, P0));
    let near_rival = tile(&game, 15, 10);
    let musician = spawn(
        &mut game,
        &rules,
        P0,
        near_rival,
        "core:unit/great-musician",
    );
    game.execute(&rules, Command::PerformConcert { unit: musician })
        .unwrap();
    // No tourism yet: the minimum blast, one level's worth of the audience's culture (own function:
    // derived/tourism.rs `concert_strength`): 750 per mille of the culture floor of 40.
    let recent = u64::from(game.view().auth.module().culture().of(P1).recent_culture(8));
    let blast = classic_rules::derived::tourism::level_threshold(1) * recent.max(40) / 1000 + 1;
    assert_eq!(
        game.view()
            .auth
            .module()
            .tourism()
            .player(P0)
            .influence_on(P1),
        blast
    );
    assert!(recent < 20, "a young nation: {recent}");
    assert_eq!(influence_level(game.view().auth, P0, P1), Some(InfluenceLevel::Noticed));
    assert!(!has_cultural_victory(game.view().auth, P0), "one concert is a first step, not a victory");
    // Sixteen concerts make the young nation's audience Prevailing (12000 per mille) and win the query.
    for _ in 0..16 {
        let t = tile(&game, 15, 10);
        let m = spawn(&mut game, &rules, P0, t, "core:unit/great-musician");
        game.execute(&rules, Command::PerformConcert { unit: m }).unwrap();
    }
    assert_eq!(influence_level(game.view().auth, P0, P1), Some(InfluenceLevel::Prevailing));
    assert!(has_cultural_victory(game.view().auth, P0));
    let _ = rival;
    let at_home = {
        let t = tile(&game, 8, 10);
        spawn(&mut game, &rules, P0, t, "core:unit/great-musician")
    };
    let err = game
        .execute(&rules, Command::PerformConcert { unit: at_home })
        .unwrap_err();
    assert!(
        format!("{err:?}").contains("land of a nation"),
        "{err:?}"
    );
}

#[test]
fn foreign_influence_turns_opinion_and_the_ideology_can_switch() {
    let rules = rules();
    let (mut game, _, _) = two_nations(&rules);
    let modern = tech(&rules, "core:tech/radio");
    for p in [P0, P1] {
        game.debug_learn(&rules, p, modern);
    }
    let (freedom, order) = (
        branch(&rules, "core:policy/freedom"),
        branch(&rules, "core:policy/order"),
    );
    game.execute(
        &rules,
        Command::ChooseIdeology {
            player: P0,
            ideology: freedom,
        },
    )
    .unwrap();
    end_turn(&mut game, &rules, P0);
    game.execute(
        &rules,
        Command::ChooseIdeology {
            player: P1,
            ideology: order,
        },
    )
    .unwrap();
    let musician = {
        let t = tile(&game, 9, 10);
        spawn(&mut game, &rules, P1, t, "core:unit/great-musician")
    };
    game.execute(&rules, Command::PerformConcert { unit: musician })
        .unwrap();
    end_turn(&mut game, &rules, P1);
    end_turn(&mut game, &rules, P0);
    let state = game.view().auth.module().tourism().player(P0).clone();
    assert_ne!(state.opinion, PublicOpinion::Content);
    assert_eq!(state.preferred, Some(order));
    // The unrest costs happiness: a share of the citizens (derived/tourism.rs `opinion_unhappy_percent`).
    let report = classic_rules::derived::happiness::player_happiness(game.view().auth, &rules, P0);
    let population: u32 = game
        .view()
        .auth
        .indexes()
        .cities_of(P0)
        .iter()
        .filter_map(|c| game.view().auth.cities().get(*c))
        .map(|c| c.population)
        .sum();
    let expected = (population * classic_rules::derived::tourism::opinion_unhappy_percent(state.dissatisfaction)).div_ceil(100);
    assert!(expected > 0, "dissatisfaction {} of {} citizens", state.dissatisfaction, population);
    assert_eq!(report.from_opinion as u32, expected, "the unhappiness of the opinion is part of the total");
    let err = game
        .execute(&rules, Command::SwitchIdeology { player: P1 })
        .unwrap_err();
    assert!(format!("{err:?}").contains("content"), "{err:?}");
    end_turn(&mut game, &rules, P1);
    game.debug_grant_culture(P0, 40);
    game.execute(&rules, Command::SwitchIdeology { player: P0 })
        .unwrap();
    let culture = game.view().auth.module().culture().of(P0);
    assert_eq!(culture.ideology, Some(order));
    assert_eq!(culture.culture, 0, "the pool burns");
    assert!(!culture.branches.contains(&freedom));
    game.validate(&rules).unwrap();
}

#[test]
fn works_and_influence_survive_save_and_load() {
    let rules = rules();
    let (mut game, capital, _) = two_nations(&rules);
    game.debug_add_building(
        &rules,
        capital,
        building(&rules, "core:building/amphitheater"),
    );
    let at = game.view().auth.cities().get(capital).unwrap().tile;
    let writer = spawn(&mut game, &rules, P0, at, "core:unit/great-writer");
    game.execute(&rules, Command::CreateGreatWork { unit: writer })
        .unwrap();
    round(&mut game, &rules);
    let saved = encode(&game, &rules);
    let loaded = decode(&saved, &rules).unwrap();
    assert_eq!(authoritative_hash(&loaded), authoritative_hash(&game));
    assert_eq!(
        loaded.view().auth.module().tourism(),
        game.view().auth.module().tourism()
    );
    let mut a = game;
    let mut b = loaded;
    for _ in 0..3 {
        round(&mut a, &rules);
        round(&mut b, &rules);
    }
    assert_eq!(authoritative_hash(&a), authoritative_hash(&b));
}

/// Dictatorship of the Proletariat adds its percent to the tourism toward a
/// nation with less excess happiness, and only that way round.
#[test]
fn the_happier_nation_gains_the_proletariat_tourism_bonus() {
    use classic_rules::derived::happiness::excess_snapshot;
    use classic_rules::derived::tourism::tourism_multiplier;
    let rules = rules();
    let (mut game, _, _) = two_nations(&rules);
    for (q, r) in [(20, 5), (20, 15), (30, 5), (30, 15)] {
        let t = tile(&game, q, r);
        let city = game.debug_found_city(&rules, P1, t);
        game.debug_set_population(&rules, city, 8);
    }
    let proletariat = classic_rules::ids::PolicyId(
        rules.module.policies.iter().position(|p| p.key == "core:policy/order/dictatorship-of-the-proletariat").unwrap() as u16,
    );
    for player in [P0, P1] {
        game.debug_adopt_policy(&rules, player, proletariat);
    }
    round(&mut game, &rules);
    let auth = game.view().auth;
    assert!(excess_snapshot(auth, P0) > excess_snapshot(auth, P1), "P1 carries five cities");
    assert_eq!(tourism_multiplier(auth, &rules, P0, P1), 30, "toward the less happy");
    assert_eq!(tourism_multiplier(auth, &rules, P1, P0), 0, "toward the happier");
}
