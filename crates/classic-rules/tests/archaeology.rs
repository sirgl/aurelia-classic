//! Archaeology on the classic ruleset (classic/docs/classic/rules.md): battles leave history, the first Archaeology makes the sites
//! appear, an archaeologist digs one for 6 turns and its player takes an
//! artifact, a Landmark, a writing or a culture boost; save/load.

use classic_content::core::core_rules;
use classic_rules::derived::archaeology::{
    choice_status, dig_turns, site_resources, ArchaeologyChoice, SITES_PER_CIV,
};
use classic_rules::ids::{BuildingKindId, CityHandle, PlayerId, TechId, TileId, UnitHandle, UnitKindId};
use classic_rules::persistence::{decode::decode, encode::encode};
use classic_rules::replay::authoritative_hash;
use classic_rules::world::tourism::{ArtifactKind, DigSite, GreatWorkClass};
use classic_rules::{compile_rules, Command, CompiledRuleset, GameConfig, GameState};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn rules() -> CompiledRuleset {
    compile_rules(core_rules()).unwrap()
}

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    let dims = game.view().auth.map().dimensions;
    dims.to_tile(classic_rules::math::HexCoord { q, r }).unwrap()
}

fn unit_kind(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap() as u16)
}

fn building(rules: &CompiledRuleset, key: &str) -> BuildingKindId {
    BuildingKindId(rules.buildings.iter().position(|b| b.key == key).unwrap() as u16)
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

fn end_turn(game: &mut GameState, rules: &CompiledRuleset) {
    let player = game.view().auth.active_player();
    game.execute(rules, Command::EndTurn { player }).unwrap();
}

fn round(game: &mut GameState, rules: &CompiledRuleset) {
    end_turn(game, rules);
    end_turn(game, rules);
}

/// Two civilizations on grassland, P0's capital at (8, 10) with a Museum.
fn world(rules: &CompiledRuleset) -> (GameState, CityHandle) {
    let mut game = GameState::new(
        rules,
        GameConfig {
            players: 2,
            width: 40,
            height: 24,
            rng_seed: 5,
        },
    );
    let capital = {
        let t = tile(&game, 8, 10);
        game.debug_found_city(rules, P0, t)
    };
    let t = tile(&game, 30, 10);
    game.debug_found_city(rules, P1, t);
    game.debug_add_building(rules, capital, building(rules, "core:building/museum"));
    (game, capital)
}

/// P0 learns Archaeology and ends its turn: the sites appear.
fn trigger(game: &mut GameState, rules: &CompiledRuleset) {
    game.debug_learn(rules, P0, tech(rules, "core:tech/archaeology"));
    round(game, rules);
}

/// A visible site (writings are only in hidden ones).
fn visible_site(game: &GameState) -> DigSite {
    *game
        .view()
        .auth
        .module()
        .tourism()
        .archaeology()
        .sites
        .iter()
        .find(|s| !s.hidden)
        .expect("a visible site")
}

fn dig(game: &mut GameState, rules: &CompiledRuleset, site: TileId) -> UnitHandle {
    let unit = game.debug_spawn_unit(rules, P0, site, unit_kind(rules, "core:unit/archaeologist"));
    game.execute(rules, Command::Excavate { unit }).unwrap();
    for _ in 0..dig_turns(rules) {
        round(game, rules);
    }
    unit
}

#[test]
fn sites_appear_with_the_first_archaeology() {
    let rules = rules();
    let (mut game, _) = world(&rules);
    assert!(!game.view().auth.module().tourism().archaeology().triggered);
    trigger(&mut game, &rules);
    let auth = game.view().auth;
    let archaeology = auth.module().tourism().archaeology();
    assert!(archaeology.triggered);
    let count = archaeology.sites.len() as u32;
    assert!(
        (SITES_PER_CIV.0 * 2..=SITES_PER_CIV.1 * 2).contains(&count),
        "{count} sites"
    );
    let (visible, _) = site_resources(&rules);
    for site in &archaeology.sites {
        let on_map = auth.map().resource(site.tile).flatten();
        assert_eq!(on_map == visible, !site.hidden, "{site:?}");
    }
    game.validate(&rules).unwrap();
}

#[test]
fn a_battle_is_written_into_the_tile_history() {
    let rules = rules();
    let (mut game, _) = world(&rules);
    let warrior = unit_kind(&rules, "core:unit/warrior");
    let a = tile(&game, 15, 10);
    let b = tile(&game, 16, 10);
    let attacker = game.debug_spawn_unit(&rules, P0, a, warrior);
    let defender = game.debug_spawn_unit(&rules, P1, b, warrior);
    game.execute(
        &rules,
        Command::DeclareWar {
            player: P0,
            enemy: P1,
        },
    )
    .unwrap();
    game.execute(&rules, Command::AttackUnit { attacker, defender })
        .unwrap();
    let record = *game
        .view()
        .auth
        .module()
        .tourism()
        .archaeology()
        .record_at(b)
        .unwrap();
    assert_eq!(
        (record.kind, record.first, record.second),
        (ArtifactKind::BattleMelee, P0, Some(P1))
    );
}

#[test]
fn an_archaeologist_digs_up_an_artifact_for_the_museum() {
    let rules = rules();
    let (mut game, capital) = world(&rules);
    trigger(&mut game, &rules);
    let site = visible_site(&game);
    let unit = dig(&mut game, &rules, site.tile);
    let auth = game.view().auth;
    assert!(
        auth.units().get(unit).is_none(),
        "the archaeologist is spent"
    );
    assert!(auth.module().tourism().archaeology().site_at(site.tile).is_none());
    assert_eq!(
        auth.map().resource(site.tile).flatten(),
        None,
        "the site leaves the map"
    );
    let choice = ArchaeologyChoice::ArtifactFirst;
    choice_status(auth, &rules, P0, site.tile, choice).unwrap();
    game.execute(
        &rules,
        Command::ChooseArchaeology {
            player: P0,
            tile: site.tile,
            choice,
        },
    )
    .unwrap();
    // The nearest free art/artifact slot: the Palace's comes first.
    let tourism = game.view().auth.module().tourism();
    let placed = tourism
        .city(capital)
        .works
        .first()
        .expect("an artifact in the capital");
    let work = game.view().auth.module().tourism().work(placed.work).unwrap();
    assert_eq!(
        (work.class, work.creator, work.era),
        (GreatWorkClass::Artifact, site.record.first, site.record.era)
    );
    let again = Command::ChooseArchaeology {
        player: P0,
        tile: site.tile,
        choice,
    };
    assert!(game.execute(&rules, again).is_err(), "one choice per dig");
    game.validate(&rules).unwrap();
}

/// The dig improvement stands on the finished site (it removed the site's
/// resource, `removes_resource`) until the player's choice
/// (`prompt_when_complete`); an artifact empties the tile.
#[test]
fn the_dig_stands_until_an_artifact_empties_the_tile() {
    let rules = rules();
    let (mut game, _) = world(&rules);
    trigger(&mut game, &rules);
    let site = visible_site(&game);
    dig(&mut game, &rules, site.tile);
    let dig_improvement = classic_rules::derived::archaeology::dig_improvement(&rules).unwrap();
    let auth = game.view().auth;
    assert_eq!(auth.map().improvement(site.tile).flatten(), Some(dig_improvement));
    assert_eq!(auth.map().resource(site.tile).flatten(), None);
    let choice = ArchaeologyChoice::ArtifactFirst;
    game.execute(&rules, Command::ChooseArchaeology { player: P0, tile: site.tile, choice })
        .unwrap();
    assert_eq!(game.view().auth.map().improvement(site.tile).flatten(), None);
    game.validate(&rules).unwrap();
}

#[test]
fn a_landmark_stays_on_the_dig() {
    let rules = rules();
    let (mut game, _) = world(&rules);
    trigger(&mut game, &rules);
    let site = visible_site(&game);
    dig(&mut game, &rules, site.tile);
    let choice = ArchaeologyChoice::Landmark;
    game.execute(
        &rules,
        Command::ChooseArchaeology {
            player: P0,
            tile: site.tile,
            choice,
        },
    )
    .unwrap();
    let auth = game.view().auth;
    let improvement = auth.map().improvement(site.tile).flatten().unwrap();
    assert!(rules.improvements[improvement.0 as usize].culture_per_era > 0);
    assert_eq!(
        auth.module().tourism().archaeology().landmark_era(site.tile),
        Some(site.record.era)
    );
}

#[test]
fn hidden_sites_need_the_exploration_finisher() {
    let rules = rules();
    let (mut game, _) = world(&rules);
    trigger(&mut game, &rules);
    let hidden = game
        .view()
        .auth
        .module()
        .tourism()
        .archaeology()
        .sites
        .iter()
        .find(|s| s.hidden)
        .copied();
    let hidden = hidden.expect("about 30 % of the sites are hidden");
    let unit = game.debug_spawn_unit(
        &rules,
        P0,
        hidden.tile,
        unit_kind(&rules, "core:unit/archaeologist"),
    );
    let err = game
        .execute(&rules, Command::Excavate { unit })
        .unwrap_err();
    assert!(format!("{err:?}").contains("hidden"), "{err:?}");
}

#[test]
fn a_warrior_does_not_dig() {
    let rules = rules();
    let (mut game, _) = world(&rules);
    trigger(&mut game, &rules);
    let site = visible_site(&game);
    let unit = game.debug_spawn_unit(
        &rules,
        P0,
        site.tile,
        unit_kind(&rules, "core:unit/warrior"),
    );
    let err = game
        .execute(&rules, Command::Excavate { unit })
        .unwrap_err();
    assert!(format!("{err:?}").contains("archaeologist"), "{err:?}");
}

#[test]
fn sites_digs_and_choices_survive_save_and_load() {
    let rules = rules();
    let (mut game, _) = world(&rules);
    trigger(&mut game, &rules);
    let site = visible_site(&game);
    let unit = game.debug_spawn_unit(
        &rules,
        P0,
        site.tile,
        unit_kind(&rules, "core:unit/archaeologist"),
    );
    game.execute(&rules, Command::Excavate { unit }).unwrap();
    round(&mut game, &rules);
    let loaded = decode(&encode(&game, &rules), &rules).unwrap();
    assert_eq!(loaded.view().auth.module().tourism(), game.view().auth.module().tourism());
    assert_eq!(authoritative_hash(&loaded), authoritative_hash(&game));
    let (mut a, mut b) = (game, loaded);
    for _ in 0..dig_turns(&rules) {
        round(&mut a, &rules);
        round(&mut b, &rules);
    }
    assert_eq!(a.view().auth.module().tourism().archaeology().pending.len(), 1);
    assert_eq!(authoritative_hash(&a), authoritative_hash(&b));
}
