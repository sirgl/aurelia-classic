//! What a citadel that takes a nation's land costs the taker in that nation's opinion, as Unciv 4.22.10
//! has it: `DiplomaticModifiers.StealingTerritory`, -10 per tile
//! (UniqueTriggerActivation.kt `OneTimeTakeOverTilesInRadius`), fading by 0.67 x 1/4 a turn at
//! Standard pace (DiplomacyTurnManager.kt `nextTurnDiplomaticModifiers`, `revertToZero`). The AI counts
//! it (`ai::diplomacy::trusted`, `denunciations`), the diplomacy screen words it.

use classic_content::core::core_rules;
use classic_rules::ClassicGame;
use classic_rules::replay::authoritative_hash;
use classic_rules::ai::diplomacy::{denunciations, trusted};
use classic_rules::ids::{CityHandle, PlayerId, TerrainId, TileId, UnitHandle, UnitKindId};
use classic_rules::math::HexCoord;
use classic_rules::rules::content::CityStateId;
use classic_rules::world::city_states::Personality;
use classic_rules::world::player::{ClassicKind, PlayerKind};
use classic_rules::world::relations::{GrievanceKind, GRIEVANCE_POINT, STOLEN_TILE_POINTS};
use classic_rules::{compile_rules, Command, CompiledRuleset, GameConfig, GameState};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const P2: PlayerId = PlayerId(2);

/// Fading of one round at Standard pace, in `GRIEVANCE_POINT`s: 0.67 x 1/4 of a point.
/// A city's culture radius, the reach of a citadel's tile (`CITADEL_CITY_REACH`).
const REACH: i32 = 5;

const ONE_ROUND: i32 = 1675;

struct W {
    game: GameState,
    rules: CompiledRuleset,
}

impl W {
    /// Open grassland, 30 x 16, everybody has met everybody.
    fn new(players: u8) -> Self {
        let rules = compile_rules(core_rules()).unwrap();
        let mut game = GameState::new(&rules, GameConfig { players, width: 30, height: 16, rng_seed: 5 });
        let grass = TerrainId(rules.terrains.iter().find(|t| t.key == "core:terrain/grassland").unwrap().map_code.unwrap());
        let all: Vec<_> = (0..30 * 16).map(|i| (TileId(i), grass)).collect();
        game.debug_set_terrain_batch(&rules, &all);
        game.debug_meet_all();
        W { game, rules }
    }

    fn tile(&self, q: i32, r: i32) -> TileId {
        self.game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
    }

    fn city(&mut self, owner: PlayerId, q: i32, r: i32) -> CityHandle {
        let at = self.tile(q, r);
        self.game.debug_found_city(&self.rules, owner, at)
    }

    fn spawn(&mut self, owner: PlayerId, key: &str, at: TileId) -> UnitHandle {
        let kind = UnitKindId(self.rules.units.iter().position(|u| u.key == key).unwrap() as u16);
        self.game.debug_spawn_unit(&self.rules, owner, at, kind)
    }

    fn exec(&mut self, command: Command) -> Result<(), classic_rules::command::CommandError> {
        self.game.execute(&self.rules, command).map(|_| ())
    }

    fn owner(&self, at: TileId) -> Option<PlayerId> {
        self.game.view().auth.map().owner(at).flatten()
    }

    /// The tiles of `victim` around `site` that a citadel there takes: its land, no city.
    fn land_of_around(&self, victim: PlayerId, site: TileId) -> usize {
        let auth = &self.game.view().auth;
        let dims = auth.map().dimensions;
        classic_rules::math::hex_disc(dims, site, 1)
            .filter(|&t| self.owner(t) == Some(victim) && auth.indexes().city_on_tile(t).is_none())
            .count()
    }

    fn citadel(&mut self, general_owner: PlayerId, site: TileId) {
        let general = self.spawn(general_owner, "core:unit/great-general", site);
        self.exec(Command::BuildCitadel { unit: general }).unwrap();
    }

    /// What `observer` holds against `offender` for stolen land, in points of opinion times
    /// `GRIEVANCE_POINT`.
    fn held(&self, observer: PlayerId, offender: PlayerId) -> i32 {
        self.game.view().auth.module().relations().grievance(observer, offender, GrievanceKind::StolenLand)
    }

    /// Every player ends its turn, once round.
    fn round(&mut self, players: u8) {
        for p in 0..players {
            self.exec(Command::EndTurn { player: PlayerId(p) }).unwrap();
        }
    }
}

/// The nation that lost the tiles holds -10 for each against the taker; the taker holds nothing
/// against it and a third nation holds nothing against either. Before: no memory of the kind, every
/// number was 0.
#[test]
fn a_taken_tile_costs_ten_points_with_the_loser_and_nobody_else() {
    let mut w = W::new(3);
    w.city(P0, 5, 7);
    w.city(P1, 8, 7);
    w.city(P2, 25, 12);
    let site = w.tile(7, 7);
    let lost = w.land_of_around(P1, site);
    assert!(lost >= 2, "the citadel takes several tiles of P1: {lost}");
    w.citadel(P0, site);
    assert_eq!(w.held(P1, P0), lost as i32 * 10 * GRIEVANCE_POINT, "10 points a tile");
    assert_eq!(w.held(P0, P1), 0, "the taker holds nothing");
    assert_eq!((w.held(P2, P0), w.held(P2, P1), w.held(P0, P2), w.held(P1, P2)), (0, 0, 0, 0), "a third nation holds nothing");
    w.game.validate(&w.rules).unwrap();
}

/// A tile that nobody owned costs nothing; a tile the taker already owned is not taken.
#[test]
fn land_that_was_nobodys_costs_nothing() {
    let mut w = W::new(2);
    w.city(P0, 5, 7);
    w.city(P1, 25, 12);
    w.citadel(P0, w.tile(7, 7));
    assert_eq!(w.held(P1, P0), 0);
    assert!(w.game.view().auth.module().relations().grievances().is_empty());
}

/// The grievance fades as Unciv's modifier does, at the owner's own turn end: 0.67 x 1/4 of a point a
/// round at Standard pace, the whole when what is left is no more than that.
#[test]
fn the_grievance_fades_as_the_modifier_of_unciv_does() {
    let mut w = W::new(3);
    w.city(P0, 5, 7);
    w.city(P1, 8, 7);
    w.city(P2, 25, 12);
    let site = w.tile(7, 7);
    let lost = w.land_of_around(P1, site) as i32;
    w.citadel(P0, site);
    let start = lost * 10 * GRIEVANCE_POINT;
    assert_eq!(w.held(P1, P0), start);
    w.round(3);
    assert_eq!(w.held(P1, P0), start - ONE_ROUND, "one turn of P1 has passed");
    w.round(3);
    w.round(3);
    assert_eq!(w.held(P1, P0), start - 3 * ONE_ROUND);
}

/// The decay arithmetic alone, to the end: the grievance of one tile is gone after 60 rounds, not 59
/// (Unciv: 10 / (0.67 x 0.25) = 59.7), and a quicker or a slower game fades it by its pace.
#[test]
fn one_tile_is_forgotten_in_sixty_rounds_at_standard_pace() {
    let kind = GrievanceKind::StolenLand;
    let one_tile = STOLEN_TILE_POINTS * GRIEVANCE_POINT;
    let step = kind.fading_at(100);
    assert_eq!((one_tile + step - 1) / step, 60, "rounds until nothing is left");
    assert_eq!((kind.fading_at(100), kind.fading_at(67), kind.fading_at(300)), (ONE_ROUND, 2500, 558), "Standard, Quick, Marathon");
}

/// A city-state keeps what it kept: -15 influence a tile, no opinion memory (it has none). The other
/// tiles of a major in the same blast cost that major.
#[test]
fn a_city_state_still_loses_fifteen_influence_a_tile_and_a_major_loses_opinion() {
    let mut w = W::new(3);
    w.game.debug_set_player_kind(P2, PlayerKind::Module(ClassicKind::CityState(CityStateId(0))));
    w.game.debug_set_personality(P2, Personality::Neutral);
    w.city(P0, 5, 7);
    w.city(P1, 25, 12);
    w.city(P2, 9, 7);
    w.game.debug_set_influence(&w.rules, P2, P0, 40);
    let site = w.tile(7, 7);
    let theirs = w.land_of_around(P2, site);
    assert!(theirs > 0);
    w.citadel(P0, site);
    let after = w.game.view().auth.module().city_states().influence(P2, P0);
    assert_eq!(after, 40 - 15 * theirs as i32, "{theirs} tiles");
    assert_eq!(w.held(P2, P0), 0, "a city-state keeps no opinion memory");
    assert_eq!(w.held(P1, P0), 0);
}

/// Several tiles are several times -10, and a second citadel adds to the first.
#[test]
fn grievances_add_up_across_citadels() {
    let mut w = W::new(2);
    w.city(P0, 5, 7);
    w.city(P1, 9, 7);
    let first = w.tile(7, 7);
    let one = w.land_of_around(P1, first) as i32;
    w.citadel(P0, first);
    assert_eq!(w.held(P1, P0), one * 10 * GRIEVANCE_POINT);
    let second = w.tile(8, 7);
    let two = w.land_of_around(P1, second) as i32;
    assert!(two > 0, "more of P1's land is left to take");
    w.citadel(P0, second);
    assert_eq!(w.held(P1, P0), (one + two) * 10 * GRIEVANCE_POINT);
}

/// The AI of the victim reads it: it no longer trusts the taker (one tile is 100 on the scale of the
/// warmonger memory, the line of distrust), and from three tiles on it denounces it (250).
#[test]
fn the_victim_ai_distrusts_and_denounces_the_taker() {
    let mut w = W::new(3);
    w.city(P0, 5, 7);
    w.city(P1, 8, 7);
    w.city(P2, 25, 12);
    let before = w.game.ai_observation(&w.rules, P1);
    let r = *before.relation(P0).unwrap();
    assert!(trusted(before.round, &r) && denunciations(&before).is_empty(), "peace and no memory: trusted");
    let site = w.tile(7, 7);
    let lost = w.land_of_around(P1, site);
    assert!(lost >= 3, "three tiles are taken at least: {lost}");
    w.citadel(P0, site);
    let after = w.game.ai_observation(&w.rules, P1);
    let r = *after.relation(P0).unwrap();
    assert_eq!(r.grievance_points(), 10.0 * lost as f32);
    assert_eq!(r.remembered_against(), 100 * lost as i32, "warmonger memory 0, grievances x 10");
    assert!(!trusted(after.round, &r), "no friendship, no pact");
    assert!(denunciations(&after).iter().any(|c| matches!(c, Command::Denounce { target, .. } if *target == P0)));
    let third = w.game.ai_observation(&w.rules, P2);
    assert_eq!(third.relation(P0).unwrap().remembered_against(), 0, "the third nation remembers nothing");
    assert!(trusted(third.round, third.relation(P0).unwrap()));
}

/// The grievance is part of the save: it survives a save and load and changes the state's hash.
#[test]
fn the_grievance_is_saved_and_hashed() {
    let mut w = W::new(2);
    w.city(P0, 5, 7);
    w.city(P1, 8, 7);
    let pristine = authoritative_hash(&w.game);
    w.citadel(P0, w.tile(7, 7));
    let held = w.held(P1, P0);
    assert!(held > 0);
    let save = classic_rules::persistence::encode::encode(&w.game, &w.rules);
    let bytes = classic_rules::save_codec::to_bytes(&save);
    let loaded = classic_rules::persistence::decode::decode(&classic_rules::save_codec::from_bytes(&bytes).unwrap(), &w.rules).unwrap();
    assert_eq!(loaded.view().auth.module().relations().grievance(P1, P0, GrievanceKind::StolenLand), held);
    assert_eq!(authoritative_hash(&loaded), authoritative_hash(&w.game));
    assert_ne!(authoritative_hash(&w.game), pristine);
}

/// A known difference from Unciv (`CITADEL_CITY_REACH`): the city that gets the tiles of a citadel
/// holds only the tiles within its culture radius. A citadel 5 tiles from the only city takes the
/// tiles around it that are within 5; Unciv gives it every tile of the disc, the ones 6 away too. A
/// pin of our behaviour, not an agreement with Unciv.
#[test]
fn a_citadel_at_the_edge_of_its_city_leaves_the_tiles_beyond_the_radius() {
    let mut w = W::new(2);
    w.city(P0, 3, 7);
    w.city(P1, 28, 3);
    for q in 5..=7 {
        w.citadel(P0, w.tile(q, 7));
    }
    let site = w.tile(8, 7);
    let dims = w.game.view().auth.map().dimensions;
    let home = w.tile(3, 7);
    assert_eq!(dims.distance(home, site), Some(REACH), "the site is at the edge");
    let (mut near, mut far) = (Vec::new(), Vec::new());
    for t in classic_rules::math::hex_disc(dims, site, 1) {
        if dims.distance(home, t).unwrap() <= REACH {
            near.push(t);
        } else {
            far.push(t);
        }
    }
    assert!(!far.is_empty(), "tiles of the disc lie 6 from the city");
    w.citadel(P0, site);
    for t in near {
        assert_eq!(w.owner(t), Some(PlayerId(0)), "within 5: taken");
    }
    for t in far {
        assert_eq!(w.owner(t), None, "6 from the city: not taken (Unciv takes it)");
    }
    w.game.validate(&w.rules).unwrap();
}
