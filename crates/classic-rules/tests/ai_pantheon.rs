//! The pantheon the AI founds is the free belief that pays most on its land
//! and in its situation, read from the belief's declared effects
//! (`ai::pantheon`). Nothing here names a belief: the oracle is the effect
//! data of the chosen belief ("it rewards the terrain the land is made of").
//! The same checks run on two belief tables: the compiled classic one and a
//! small synthetic one with other kinds of effect.

use classic_content::core::core_rules;
use classic_rules::ai::pantheon::{choose_pantheon, score_belief, scores, Situation};
use classic_rules::ai::religion::religion_commands;
use classic_rules::game::ClassicGame;
use classic_rules::ids::{BeliefId, FeatureId, PlayerId, ResourceId, TerrainId, TileId, UnitKindId};
use classic_rules::math::{hex_disc, HexCoord};
use classic_rules::rules::content::{BeliefCategory, Personality};
use classic_rules::rules::effects::{CompiledEffect, Condition, Modifier, YieldKind};
use classic_rules::world::player::{ClassicKind, PlayerKind};
use classic_rules::{compile_rules, Command, CompiledRuleset, GameConfig, GameState};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const BARBARIANS: PlayerId = PlayerId(2);
const GRASS: TerrainId = TerrainId(1);

/// A pantheon table of other kinds of effect than classic's: a yield from the
/// tiles of a resource, of a terrain, a yield per city, growth, happiness
/// and a yield in coastal and river cities, a city's strike, a yield from
/// kills.
const SYNTHETIC: &str = r#"(
    beliefs: [
        (id: "core:belief/pantheon/synthetic-ore", name: "Ore", category: Some(Pantheon), text: "",
         effects: [(scope: Tile, selector: FollowingCityTiles, condition: TileResource("core:resource/wheat"), modifier: AddYield(yield_kind: Gold, amount: 2))]),
        (id: "core:belief/pantheon/synthetic-heights", name: "Heights", category: Some(Pantheon), text: "",
         effects: [(scope: Tile, selector: FollowingCityTiles, condition: TileTerrain("core:terrain/hill"), modifier: AddYield(yield_kind: Production, amount: 2))]),
        (id: "core:belief/pantheon/synthetic-hearth", name: "Hearth", category: Some(Pantheon), text: "",
         effects: [(scope: City, selector: FollowingCities, modifier: AddYield(yield_kind: Culture, amount: 1))]),
        (id: "core:belief/pantheon/synthetic-seed", name: "Seed", category: Some(Pantheon), text: "",
         effects: [(scope: City, selector: FollowingCities, modifier: ModifyGrowth(percent: 20))]),
        (id: "core:belief/pantheon/synthetic-harbour", name: "Harbour", category: Some(Pantheon), text: "",
         effects: [(scope: City, selector: FollowingCities, condition: CityCoastal, modifier: AddHappiness(amount: 2))]),
        (id: "core:belief/pantheon/synthetic-ford", name: "Ford", category: Some(Pantheon), text: "",
         effects: [(scope: City, selector: FollowingCities, condition: CityOnRiver, modifier: AddYield(yield_kind: Gold, amount: 3))]),
        (id: "core:belief/pantheon/synthetic-bastion", name: "Bastion", category: Some(Pantheon), text: "",
         effects: [(scope: City, selector: FollowingCities, modifier: ModifyCityCombat(stat: RangedStrikePercent, amount: 40))]),
        (id: "core:belief/pantheon/synthetic-spoils", name: "Spoils", category: Some(Pantheon), text: "",
         effects: [(scope: Player, selector: Owner, condition: EventNearSourceCity(4), trigger: OnUnitKilled, modifier: YieldFromKill(yield_kind: Gold, percent: 100), duration: Once)]),
    ],
)"#;

#[derive(Clone, Copy, Debug)]
enum Table {
    Classic,
    Synthetic,
}

fn rules_of(table: Table) -> CompiledRuleset {
    let mut source = core_rules();
    if let Table::Synthetic = table {
        source.module.beliefs.retain(|b| b.category != Some(BeliefCategory::Pantheon));
        let synthetic = classic_content::classic::parse_files([("synthetic.ron", SYNTHETIC)]).expect("the synthetic table parses");
        source.merge(synthetic);
    }
    compile_rules(source).expect("rules compile")
}

/// Every compiled effect of a belief.
fn effects_of(rules: &CompiledRuleset, belief: BeliefId) -> Vec<CompiledEffect> {
    let set = &rules.module.beliefs[belief.0 as usize].effects;
    set.passive_player
        .iter()
        .chain(&set.passive_city)
        .chain(&set.passive_unit)
        .chain(&set.passive_tile)
        .chain(set.triggered.iter().flatten())
        .copied()
        .collect()
}

fn rewards(rules: &CompiledRuleset, belief: BeliefId, condition: Condition) -> bool {
    effects_of(rules, belief).iter().any(|e| e.condition == condition)
}

fn is_military(rules: &CompiledRuleset, belief: BeliefId) -> bool {
    effects_of(rules, belief).iter().any(|e| {
        matches!(e.modifier, Modifier::YieldFromKill { .. } | Modifier::ModifyCityCombat { .. } | Modifier::ModifyUnitStat { .. } | Modifier::ModifyCombat { .. })
    })
}

fn pays_yield(rules: &CompiledRuleset, belief: BeliefId, kind: YieldKind) -> bool {
    effects_of(rules, belief)
        .iter()
        .any(|e| matches!(e.modifier, Modifier::AddYield { yield_kind, .. } | Modifier::YieldFromKill { yield_kind, .. } if yield_kind == kind))
}

struct Land {
    game: GameState,
    rules: CompiledRuleset,
}

impl Land {
    /// 40×24 grassland, two civilizations (P1 far away to the east) and a
    /// barbarian player; P0's capital at (8, 12).
    fn new(table: Table) -> Self {
        let rules = rules_of(table);
        let mut game = GameState::new(&rules, GameConfig { players: 3, width: 40, height: 24, rng_seed: 11 });
        game.debug_set_player_kind(BARBARIANS, PlayerKind::Module(ClassicKind::Barbarian));
        let tiles = game.view().auth.map().dimensions.tile_count() as u32;
        let land: Vec<(TileId, TerrainId)> = (0..tiles).map(|t| (TileId(t), GRASS)).collect();
        game.debug_set_terrain_batch(&rules, &land);
        let mut land = Land { game, rules };
        land.city(P0, 8, 12);
        land.city(P1, 36, 12);
        land
    }

    fn tile(&self, q: i32, r: i32) -> TileId {
        let dims = self.game.view().auth.map().dimensions;
        dims.to_tile(HexCoord { q, r }).unwrap()
    }

    fn city(&mut self, owner: PlayerId, q: i32, r: i32) {
        let t = self.tile(q, r);
        self.game.debug_found_city(&self.rules, owner, t);
        let dims = self.game.view().auth.map().dimensions;
        let seen: Vec<TileId> = hex_disc(dims, t, 5).collect();
        self.game.debug_reveal(owner, &seen);
    }

    /// The tiles of the ring 1..=3 around the capital.
    fn ring(&self) -> Vec<TileId> {
        let dims = self.game.view().auth.map().dimensions;
        let capital = self.tile(8, 12);
        hex_disc(dims, capital, 3).filter(|t| *t != capital).collect()
    }

    /// Makes the land of the condition: terrain or feature on many tiles, a
    /// resource on a few. False for a condition the land cannot show.
    fn lay(&mut self, condition: Condition) -> bool {
        let ring = self.ring();
        match condition {
            Condition::TileTerrain(kind) => {
                let code = self.rules.terrains[kind.0 as usize].map_code.unwrap();
                let tiles: Vec<_> = ring.into_iter().take(20).map(|t| (t, TerrainId(code))).collect();
                self.game.debug_set_terrain_batch(&self.rules, &tiles);
            }
            Condition::TileFeature(feature) => {
                let tiles: Vec<(TileId, FeatureId)> = ring.into_iter().take(20).map(|t| (t, feature)).collect();
                self.game.debug_set_feature_batch(&self.rules, &tiles);
            }
            Condition::TileResource(resource) => {
                // A resource the player cannot see yet is not on its map.
                if self.rules.resources[resource.0 as usize].required_tech.is_some() {
                    return false;
                }
                let tiles: Vec<(TileId, ResourceId)> = ring.into_iter().take(6).map(|t| (t, resource)).collect();
                self.game.debug_set_resource_batch(&self.rules, &tiles);
            }
            _ => return false,
        }
        true
    }

    /// A war with P1, whose second city stands six tiles away and is known.
    fn war(&mut self) {
        self.city(P1, 14, 12);
        let t = self.tile(14, 12);
        self.game.debug_reveal(P0, &[t]);
        self.game.debug_meet(P0, P1);
        self.game.execute(&self.rules, Command::DeclareWar { player: P0, enemy: P1 }).unwrap();
    }

    fn free(&self) -> Vec<BeliefId> {
        self.game.ai_observation(&self.rules, P0).faith.overview.free_pantheon.clone()
    }

    fn pick(&self) -> BeliefId {
        let obs = self.game.ai_observation(&self.rules, P0);
        choose_pantheon(&obs, &self.rules, &obs.faith.overview.free_pantheon).expect("a belief")
    }

    fn name(&self, belief: BeliefId) -> &str {
        &self.rules.module.beliefs[belief.0 as usize].name
    }

    /// The best scores, for failure messages.
    fn table(&self) -> String {
        let obs = self.game.ai_observation(&self.rules, P0);
        let situation = Situation::of(&obs, &self.rules);
        let threat = situation.threat();
        let mut table = scores(&situation, &obs.faith.overview.free_pantheon);
        table.sort_by_key(|(b, s)| (std::cmp::Reverse(*s), b.0));
        let top = table.iter().take(4).map(|(b, s)| format!("{} {s}", self.name(*b))).collect::<Vec<_>>().join(", ");
        format!("threat {threat:?}: {top}")
    }
}

const TABLES: [Table; 2] = [Table::Classic, Table::Synthetic];

/// The conditions of the tiles that the free pantheons reward.
fn tile_conditions(rules: &CompiledRuleset, free: &[BeliefId]) -> Vec<Condition> {
    let mut out: Vec<Condition> = Vec::new();
    for belief in free {
        for effect in effects_of(rules, *belief) {
            let land = matches!(effect.condition, Condition::TileTerrain(_) | Condition::TileFeature(_) | Condition::TileResource(_));
            if land && !out.contains(&effect.condition) {
                out.push(effect.condition);
            }
        }
    }
    out
}

/// Land of one kind makes the AI take a belief that rewards that kind, and
/// different kinds of land give different beliefs, on either table.
#[test]
fn the_land_decides_the_pantheon() {
    for table in TABLES {
        let plain = Land::new(table);
        let conditions = tile_conditions(&plain.rules, &plain.free());
        assert!(conditions.len() >= 2, "{table:?}: the table rewards kinds of land");
        let mut picks = vec![plain.pick()];
        for condition in conditions {
            let mut land = Land::new(table);
            if !land.lay(condition) {
                continue;
            }
            let pick = land.pick();
            assert!(rewards(&land.rules, pick, condition), "{table:?}: land of {condition:?} picked {} ({})", land.name(pick), land.table());
            picks.push(pick);
        }
        picks.sort_by_key(|b| b.0);
        picks.dedup();
        assert!(picks.len() >= 3, "{table:?}: distinct lands give distinct beliefs, got {picks:?}");
    }
}

/// A war and barbarians in sight turn a peaceful choice to a belief of arms.
#[test]
fn war_turns_the_choice_to_arms() {
    for table in TABLES {
        let peace = Land::new(table);
        let peaceful = peace.pick();
        assert!(!is_military(&peace.rules, peaceful), "{table:?}: peace picks {} ({})", peace.name(peaceful), peace.table());

        let mut war = Land::new(table);
        war.war();
        let at_war = war.pick();
        assert!(is_military(&war.rules, at_war), "{table:?}: war picks {} ({})", war.name(at_war), war.table());

        let mut raided = Land::new(table);
        raided.war();
        let warrior = UnitKindId(raided.rules.units.iter().position(|u| u.key == "core:unit/warrior").unwrap() as u16);
        for (q, r) in [(10, 12), (10, 11), (11, 11)] {
            let t = raided.tile(q, r);
            raided.game.debug_spawn_unit(&raided.rules, BARBARIANS, t, warrior);
        }
        let attacked = raided.pick();
        assert!(is_military(&raided.rules, attacked), "{table:?}: barbarians at the gate pick {} ({})", raided.name(attacked), raided.table());
    }
}

/// Several cities on a river make the belief that pays river cities worth
/// more than the belief that pays every city once.
#[test]
fn river_cities_take_the_river_belief() {
    for table in TABLES {
        let mut land = Land::new(table);
        let towns = [(8, 12), (11, 12), (8, 15)];
        for (q, r) in &towns[1..] {
            land.city(P0, *q, *r);
        }
        let banks: Vec<TileId> = towns.iter().map(|(q, r)| land.tile(*q, *r)).collect();
        land.game.debug_set_river_batch(&land.rules, &banks);
        let pick = land.pick();
        assert!(rewards(&land.rules, pick, Condition::CityOnRiver), "{table:?}: river cities picked {} ({})", land.name(pick), land.table());
        let dry = Land::new(table).pick();
        assert_ne!(pick, dry, "{table:?}: the river changes the choice");
    }
}

/// The leader's flavours weigh the yields: the more it loves gold, the more
/// a belief that pays gold is worth; a belief that pays none is unmoved.
#[test]
fn the_leaders_flavours_weigh_the_yields() {
    for table in TABLES {
        let land = Land::new(table);
        let obs = land.game.ai_observation(&land.rules, P0);
        let miser = Situation::with_personality(&obs, &land.rules, &Personality { gold: 10, ..Personality::NEUTRAL });
        let spendthrift = Situation::with_personality(&obs, &land.rules, &Personality { gold: 0, ..Personality::NEUTRAL });
        let free = land.free();
        let paying: Vec<_> = free.iter().filter(|b| pays_yield(&land.rules, **b, YieldKind::Gold)).collect();
        assert!(!paying.is_empty(), "{table:?}: some belief pays gold");
        let moved = free.iter().filter(|b| miser.score(**b) > spendthrift.score(**b)).count();
        assert!(moved >= 1, "{table:?}: the love of gold moves some score");
        for belief in &free {
            let (high, low) = (miser.score(*belief), spendthrift.score(*belief));
            if paying.contains(&belief) {
                assert!(high >= low, "{table:?}: {} pays gold: {high} vs {low}", land.name(*belief));
            }
            if !pays_yield(&land.rules, *belief, YieldKind::Gold) && !effects_of(&land.rules, *belief).iter().any(|e| matches!(e.modifier, Modifier::ModifyCost { .. })) {
                assert_eq!(high, low, "{table:?}: {} pays no gold", land.name(*belief));
            }
        }
    }
}

/// The choice reads the observation only and a tie goes to the table's
/// order, whatever the order of the free list.
#[test]
fn the_choice_is_deterministic_and_ties_go_to_the_table() {
    for table in TABLES {
        let land = Land::new(table);
        assert_eq!(land.pick(), land.pick());
        let obs = land.game.ai_observation(&land.rules, P0);
        let free = obs.faith.overview.free_pantheon.clone();
        let mut reversed = free.clone();
        reversed.reverse();
        assert_eq!(choose_pantheon(&obs, &land.rules, &free), choose_pantheon(&obs, &land.rules, &reversed), "{table:?}");
        // Beliefs of no worth here tie at zero and the earlier one wins.
        let situation = Situation::of(&obs, &land.rules);
        let worthless: Vec<BeliefId> = scores(&situation, &free).into_iter().filter(|(_, s)| *s == 0).map(|(b, _)| b).collect();
        if worthless.len() >= 2 {
            let backwards: Vec<BeliefId> = worthless.iter().rev().copied().collect();
            assert_eq!(choose_pantheon(&obs, &land.rules, &backwards), Some(worthless[0]), "{table:?}");
        }
        for belief in &free {
            assert_eq!(score_belief(&obs, &land.rules, *belief), situation.score(*belief), "{table:?}");
        }
    }
}

/// A taken belief is not chosen; one free belief is the choice; none, none.
#[test]
fn only_free_beliefs_are_chosen() {
    for table in TABLES {
        let mut land = Land::new(table);
        let first = land.free()[0];
        let condition = tile_conditions(&land.rules, &land.free())[0];
        assert!(land.lay(condition));
        let obs = land.game.ai_observation(&land.rules, P0);
        let best = choose_pantheon(&obs, &land.rules, &obs.faith.overview.free_pantheon).unwrap();
        let rest: Vec<BeliefId> = obs.faith.overview.free_pantheon.iter().copied().filter(|b| *b != best).collect();
        let next = choose_pantheon(&obs, &land.rules, &rest).unwrap();
        assert_ne!(next, best, "{table:?}");
        assert_eq!(choose_pantheon(&obs, &land.rules, &[first]), Some(first));
        assert_eq!(choose_pantheon(&obs, &land.rules, &[]), None);
    }
}

/// The advisor founds the belief the scores choose.
#[test]
fn the_advisor_founds_the_scored_pantheon() {
    for table in TABLES {
        let mut land = Land::new(table);
        let condition = tile_conditions(&land.rules, &land.free())[0];
        assert!(land.lay(condition));
        land.game.debug_grant_faith(P0, 200);
        let obs = land.game.ai_observation(&land.rules, P0);
        let founded = religion_commands(&obs, &land.rules)
            .into_iter()
            .find_map(|c| match c {
                Command::FoundPantheon { belief, .. } => Some(belief),
                _ => None,
            })
            .expect("a pantheon is founded");
        assert_eq!(founded, land.pick(), "{table:?}");
        assert!(rewards(&land.rules, founded, condition), "{table:?}");
        land.game.execute(&land.rules, Command::FoundPantheon { player: P0, belief: founded }).unwrap();
    }
}
