//! Improvement rules on the classic ruleset: the Moai's culture per adjacent
//! Moai, placement flags (the Feitoria only on city-state land, the
//! Landmark only over another improvement, the dig ignores ownership),
//! pillage (`MapState::pillaged`: no yields until repaired; a fort is
//! destroyed instead) and its repair, save/load of pillaged tiles.

use classic_rules::ClassicGame;
use classic_content::core::core_rules;
use classic_rules::derived::citizens::{worked_tile_yields, EmpireContext};
use classic_rules::effects::city::city_tile_effects;
use classic_rules::ids::{CityHandle, ImprovementId, PlayerId, TechId, TerrainId, TileId, UnitKindId};
use classic_rules::math::HexCoord;
use classic_rules::persistence::{decode::decode, encode::encode};
use classic_rules::replay::authoritative_hash;
use classic_rules::rules::content::{CityStateId, Yields};
use classic_rules::world::builds::{improvement_site, REPAIR_WORK_TIME};
use classic_rules::world::player::PlayerKind;
use classic_rules::{compile_rules, Command, CompiledRuleset, GameConfig, GameState};
use classic_rules::world::player::ClassicKind;

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const GRASS: TerrainId = TerrainId(1);
const WATER: TerrainId = TerrainId(4);

struct World {
    game: GameState,
    rules: CompiledRuleset,
    city: CityHandle,
}

impl World {
    /// A 30×20 grassland map, P0's city at (8, 8).
    fn new() -> Self {
        let rules = compile_rules(core_rules()).unwrap();
        let mut game = GameState::new(&rules, GameConfig { players: 2, width: 30, height: 20, rng_seed: 4 });
        let all: Vec<_> = (0..600).map(|i| (TileId(i), GRASS)).collect();
        game.debug_set_terrain_batch(&rules, &all);
        let at = game.view().auth.map().dimensions.to_tile(HexCoord { q: 8, r: 8 }).unwrap();
        let city = game.debug_found_city(&rules, P0, at);
        World { game, rules, city }
    }

    fn tile(&self, q: i32, r: i32) -> TileId {
        self.game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
    }

    fn improvement(&self, key: &str) -> ImprovementId {
        ImprovementId(self.rules.improvements.iter().position(|i| i.key == key).unwrap() as u16)
    }

    fn place(&mut self, t: TileId, key: &str) {
        let i = self.improvement(key);
        self.game.debug_set_improvement_batch(&self.rules, &[(t, i)]);
    }

    fn nation(&mut self, player: PlayerId, key: &str) {
        let nation = self.rules.nations.iter().position(|c| c.key == key).unwrap();
        self.game.debug_set_player_nation(player, nation as u8);
        self.game.debug_learn(&self.rules, player, TechId(0));
    }

    fn learn(&mut self, player: PlayerId, key: &str) {
        let t = self.rules.technologies.iter().position(|t| t.key == key).unwrap();
        self.game.debug_learn(&self.rules, player, TechId(t as u16));
    }

    fn worked(&self, t: TileId) -> Yields {
        let auth = self.game.view().auth;
        let core = auth.cities().get(self.city).unwrap();
        let effects = city_tile_effects(auth, &self.rules, core);
        worked_tile_yields(auth, &self.rules, core, t, &effects, &EmpireContext::of(auth, P0))
    }

    fn until_active(&mut self, player: PlayerId) {
        for _ in 0..4 {
            let active = self.game.view().auth.active_player();
            if active == player {
                return;
            }
            self.game.execute(&self.rules, Command::EndTurn { player: active }).unwrap();
        }
        panic!("never active");
    }

    /// A P1 warrior at war with P0 pillages `t`.
    fn pillage(&mut self, t: TileId) -> Vec<classic_rules::classic::ClassicChange> {
        let warrior = UnitKindId(self.rules.units.iter().position(|u| u.key == "core:unit/warrior").unwrap() as u16);
        let unit = self.game.debug_spawn_unit(&self.rules, P1, t, warrior);
        self.game.debug_force_war(&self.rules, P1, P0);
        self.until_active(P1);
        let result = self.game.execute(&self.rules, Command::Pillage { unit }).unwrap();
        result.changes().module_changes.clone()
    }
}

#[test]
fn a_moai_gives_one_more_culture_per_adjacent_moai() {
    let mut w = World::new();
    w.nation(P0, "core:nation/polynesia");
    let (a, b, c) = (w.tile(9, 8), w.tile(10, 8), w.tile(11, 8));
    w.place(a, "core:improvement/moai");
    let alone = w.worked(a).culture;
    w.place(b, "core:improvement/moai");
    assert_eq!(w.worked(a).culture, alone + 1, "one neighbour");
    w.place(c, "core:improvement/moai");
    assert_eq!(w.worked(b).culture, alone + 2, "two neighbours");
    w.game.validate(&w.rules).unwrap();
}

#[test]
fn a_feitoria_goes_on_city_state_land_only() {
    let mut w = World::new();
    w.nation(P0, "core:nation/portugal");
    w.learn(P0, "core:tech/navigation");
    // A sea from row 13 on makes row 12 coastal.
    let sea: Vec<_> = (0..30).flat_map(|q| (13..20).map(move |r| (q, r))).map(|(q, r)| (w.tile(q, r), WATER)).collect();
    w.game.debug_set_terrain_batch(&w.rules, &sea);
    w.game.debug_set_player_kind(P1, PlayerKind::Module(ClassicKind::CityState(CityStateId(0))));
    let host = w.tile(20, 12);
    w.game.debug_found_city(&w.rules, P1, host);
    let feitoria = w.improvement("core:improvement/feitoria");
    let auth = w.game.view().auth;
    let on_city_state = w.tile(21, 12);
    assert_eq!(auth.map().owner(on_city_state).flatten(), Some(P1));
    assert_eq!(improvement_site(auth, &w.rules, P0, on_city_state, feitoria).err(), None);
    let own = w.tile(8, 10);
    let own_coast = auth.map().dimensions.neighbors(own).into_iter().find(|t| auth.map().owner(*t).flatten() == Some(P0));
    let own_tile = own_coast.unwrap_or(own);
    assert_eq!(
        improvement_site(auth, &w.rules, P0, own_tile, feitoria).unwrap_err(),
        "only on a city-state's land"
    );
}

#[test]
fn a_landmark_needs_an_improvement_to_replace() {
    let mut w = World::new();
    let landmark = w.improvement("core:improvement/landmark");
    let t = w.tile(9, 8);
    let auth = w.game.view().auth;
    assert_eq!(improvement_site(auth, &w.rules, P0, t, landmark).unwrap_err(), "needs an improvement to replace");
    w.place(t, "core:improvement/farm");
    let auth = w.game.view().auth;
    assert_ne!(improvement_site(auth, &w.rules, P0, t, landmark), Err("needs an improvement to replace"));
}

#[test]
fn a_pillaged_farm_yields_nothing_until_a_worker_repairs_it() {
    let mut w = World::new();
    let farm = w.improvement("core:improvement/farm");
    w.learn(P0, "core:tech/agriculture");
    let t = w.tile(9, 8);
    w.place(t, "core:improvement/farm");
    let intact = w.worked(t);
    w.pillage(t);
    let auth = w.game.view().auth;
    assert!(auth.map().pillaged(t), "the farm stays, pillaged");
    assert_eq!(auth.map().improvement(t).flatten(), Some(farm));
    assert_eq!(w.worked(t).food, intact.food - 1, "no farm food");
    w.game.validate(&w.rules).unwrap();
    let saved = decode(&encode(&w.game, &w.rules), &w.rules).unwrap();
    assert_eq!(authoritative_hash(&saved), authoritative_hash(&w.game), "save/load");
    assert!(saved.view().auth.map().pillaged(t));
    let plan = improvement_site(w.game.view().auth, &w.rules, P0, t, farm).unwrap();
    assert_eq!(u32::from(plan.work_turns) * 100, REPAIR_WORK_TIME, "a repair");
    w.place(t, "core:improvement/farm");
    assert!(!w.game.view().auth.map().pillaged(t), "repaired");
    assert_eq!(w.worked(t), intact);
}

#[test]
fn a_pillaged_fort_is_destroyed() {
    let mut w = World::new();
    let t = w.tile(9, 8);
    w.place(t, "core:improvement/fort");
    w.pillage(t);
    let auth = w.game.view().auth;
    assert_eq!(auth.map().improvement(t).flatten(), None, "destroyed when pillaged");
    assert!(!auth.map().pillaged(t));
    w.game.validate(&w.rules).unwrap();
}

/// A landmark yields its culture again for every era the owner has entered
/// since the dig it stands on (`yield_per_era`).
#[test]
fn a_landmark_yields_more_culture_for_each_era_since_its_dig() {
    let mut w = World::new();
    let t = w.tile(9, 8);
    w.place(t, "core:improvement/landmark");
    w.game.debug_set_landmark(t, 0);
    let per_era = i32::from(w.rules.improvements[w.improvement("core:improvement/landmark").0 as usize].culture_per_era);
    assert!(per_era > 0);
    let young = w.worked(t).culture;
    let medieval = w
        .rules
        .technologies
        .iter()
        .position(|t| t.content_era.is_some_and(|e| w.rules.eras[e.0 as usize].index == 2))
        .unwrap();
    w.game.debug_learn(&w.rules, P0, TechId(medieval as u16));
    assert_eq!(w.worked(t).culture, young + 2 * per_era, "two eras later");
    w.game.validate(&w.rules).unwrap();
}

/// Pillaging an improvement pays its `pillage_gold` (two draws below it) to
/// the pillager; the pillager's own land cannot be pillaged.
#[test]
fn pillaging_a_rivals_or_unowned_improvement_pays_gold_and_ones_own_is_refused() {
    let mut w = World::new();
    let farm_gold = w.rules.improvements[w.improvement("core:improvement/farm").0 as usize].pillage_gold;
    assert!(farm_gold > 0);
    let treasury = |w: &World, p| w.game.view().auth.players().treasury_of(p);
    let tiles = [w.tile(9, 8), w.tile(7, 8), w.tile(8, 9), w.tile(8, 7)];
    for t in tiles {
        w.place(t, "core:improvement/farm");
    }
    let before = treasury(&w, P1);
    let mut journaled = 0;
    for t in tiles {
        assert_eq!(w.game.view().auth.map().owner(t).flatten(), Some(P0));
        let before_one = treasury(&w, P1);
        let changes = w.pillage(t);
        let paid = treasury(&w, P1) - before_one;
        let told: Vec<u32> = changes
            .iter()
            .filter_map(|c| match c {
                classic_rules::classic::ClassicChange::Plundered { player, tile, source, gold }
                    if *player == P1 && *tile == t && *source == classic_rules::PlunderSource::Improvement =>
                {
                    Some(*gold)
                }
                _ => None,
            })
            .collect();
        assert_eq!(told, if paid > 0 { vec![paid] } else { vec![] }, "the journal names the gold of the pillaged farm");
        journaled += told.iter().sum::<u32>();
    }
    let earned = treasury(&w, P1) - before;
    assert_eq!(journaled, earned, "and nothing else");
    assert!(earned > 0 && earned <= 4 * 2 * (farm_gold - 1), "{earned} gold for four farms");

    let auth = w.game.view().auth;
    let city_tile = auth.cities().get(w.city).unwrap().tile;
    let home = auth
        .map()
        .dimensions
        .neighbors(city_tile)
        .into_iter()
        .find(|t| !tiles.contains(t) && auth.map().owner(*t).flatten() == Some(P0))
        .expect("an own tile next to the city");
    w.place(home, "core:improvement/farm");
    let warrior = UnitKindId(w.rules.units.iter().position(|u| u.key == "core:unit/warrior").unwrap() as u16);
    let unit = w.game.debug_spawn_unit(&w.rules, P0, home, warrior);
    w.until_active(P0);
    let refused = w.game.execute(&w.rules, Command::Pillage { unit }).unwrap_err();
    assert!(format!("{refused:?}").contains("own tile"), "{refused:?}");

    let wild = w.tile(22, 14);
    w.place(wild, "core:improvement/farm");
    let raider = w.game.debug_spawn_unit(&w.rules, P0, wild, warrior);
    let before = treasury(&w, P0);
    let result = w.game.execute(&w.rules, Command::Pillage { unit: raider }).unwrap();
    assert!(treasury(&w, P0) > before, "unowned land pays too");
    assert!(
        result.changes().module_changes.iter().any(|c| matches!(c,
            classic_rules::classic::ClassicChange::Plundered { player, gold, .. } if *player == P0 && *gold == treasury(&w, P0) - before)),
        "unowned land too"
    );
}

/// A fort or a citadel adds its `defense_pct` to the tile for a defender on
/// its owner's land or on land nobody owns; hills and forests add theirs
/// and do not stack.
#[test]
fn an_improvement_defends_its_owners_and_terrain_defends_everyone() {
    use classic_rules::combat::terrain::tile_defense_percent;
    use classic_rules::ids::FeatureId;
    use classic_rules::rules::roles::{Ground, TerrainPalette};
    let mut w = World::new();
    let defense = |w: &World, t: TileId, p: PlayerId| tile_defense_percent(w.game.view().auth, &w.rules, t, p);
    let fort_pct = i64::from(w.rules.improvements[w.improvement("core:improvement/fort").0 as usize].defense_pct);
    let citadel_pct = i64::from(w.rules.improvements[w.improvement("core:improvement/citadel").0 as usize].defense_pct);
    assert_eq!((fort_pct, citadel_pct), (50, 100));

    let own = w.tile(9, 8);
    w.place(own, "core:improvement/fort");
    assert_eq!(w.game.view().auth.map().owner(own).flatten(), Some(P0));
    assert_eq!((defense(&w, own, P0), defense(&w, own, P1)), (fort_pct, 0), "the owner's fort");

    let wild = w.tile(22, 14);
    w.place(wild, "core:improvement/citadel");
    assert_eq!(w.game.view().auth.map().owner(wild).flatten(), None);
    assert_eq!((defense(&w, wild, P0), defense(&w, wild, P1)), (citadel_pct, citadel_pct), "unowned ground");

    let hills = TerrainId(TerrainPalette::LEGACY.code(Ground::Hills));
    let mountains = TerrainId(TerrainPalette::LEGACY.code(Ground::Mountains));
    let forest = FeatureId(w.rules.features.iter().position(|f| f.key == "core:feature/forest").unwrap() as u16);
    let marsh = FeatureId(w.rules.features.iter().position(|f| f.key == "core:feature/marsh").unwrap() as u16);
    let (flat, hill, peak, wood, bog, wooded_hill) = (w.tile(15, 3), w.tile(16, 3), w.tile(17, 3), w.tile(18, 3), w.tile(19, 3), w.tile(20, 3));
    w.game.debug_set_terrain_batch(&w.rules, &[(hill, hills), (peak, mountains), (wooded_hill, hills)]);
    w.game.debug_set_feature_batch(&w.rules, &[(wood, forest), (bog, marsh), (wooded_hill, forest)]);
    let seen: Vec<i64> = [flat, hill, peak, wood, bog, wooded_hill].map(|t| defense(&w, t, P0)).to_vec();
    assert_eq!(seen, [0, 25, 25, 25, -15, 25], "flat, hills, mountains, forest, marsh, forest on hills");
}
