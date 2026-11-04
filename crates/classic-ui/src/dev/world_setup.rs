//! Offscreen setups for the world screens and the combat overlays (`--screenshot`).
//! They change the world only through classic-rules's public harness setters and commands.
//!
//! AURELIA_GAME_CITY_STATES=n: the match is created with n city-states.
//! AURELIA_GAME_MAX_ROUNDS=n: the time victory comes after round n.
//! AURELIA_GAME_SETUP=token,token,... before the end turns: `meet` (every player met every
//! other), `war` (war on player 1), `printing` (Printing Press: World Congress and spies
//! after a turn), `gold` (1200 gold), `influence` (influence and quests with the
//! city-states); after them: `offer` (player 1 offers a deal), `apollo` (Apollo Program
//! and a booster), `spy` (spy 0 goes to the capital: its name on the banner), `combat` (an enemy warrior next to the own warrior, pointer on it),
//! `promotion` (the warrior's promotion list), `capture` (a captured enemy city), `ruins` (Ancient
//! Ruins under the own warrior: the card with the three rewards), `citystate` (the first met
//! city-state's city is revealed and its card opened over the map; use with `influence`),
//! `citystate-hover` (the same city, card shut, the pointer on it); the last
//! setup `embark` stages four land soldiers afloat beside the shore and a warrior ashore
//! (`embark_scene`; the camera goes there).
use classic_rules::ClassicGame;
use classic_rules::ids::{CityHandle, PlayerId, TileId, UnitHandle, UnitKindId};
use classic_rules::rules::content::ProjectId;
use classic_rules::world::city_states::QuestKind;
use classic_rules::world::relations::DealItem;
use classic_rules::{Command, CompiledRuleset, GameState};
use crate::ui::screens::{self, MatchRequest, PreparedMatch};

use super::GameApp;
use classic_rules::world::player::ClassicPlayers;

const ME: PlayerId = PlayerId(0);
const RIVAL: PlayerId = PlayerId(1);

fn tokens() -> Vec<String> {
    let list = std::env::var("AURELIA_GAME_SETUP").unwrap_or_default();
    list.split(',')
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect()
}

fn wants(token: &str) -> bool {
    tokens().iter().any(|t| t == token)
}

/// AURELIA_GAME_START_BALANCE=off: the starts as the generator made them before the balance (the
/// "before" screenshots of `docs/start-balance.md`); anything else: the balanced starts.
pub fn start_balance() -> classic_rules::scenario::StartBalance {
    match std::env::var("AURELIA_GAME_START_BALANCE").as_deref() {
        Ok("off") => classic_rules::scenario::StartBalance::OFF,
        _ => classic_rules::scenario::StartBalance::ON,
    }
}

/// The scripted match without city-states, with the balance the environment asks for.
pub fn match_plain(request: MatchRequest) -> Result<PreparedMatch, String> {
    screens::prepare_game(screens::NewGame {
        request,
        options: screens::GameOptions { start_balance: start_balance(), ..Default::default() },
    })
}

/// AURELIA_GAME_CITY_STATES: the scripted match with city-states (None: without).
pub fn match_with_city_states(request: &MatchRequest) -> Option<PreparedMatch> {
    let count: u8 = std::env::var("AURELIA_GAME_CITY_STATES").ok()?.parse().ok()?;
    let options = screens::GameOptions {
        city_states: count,
        start_balance: start_balance(),
        ..Default::default()
    };
    screens::prepare_game(screens::NewGame {
        request: request.clone(),
        options,
    })
    .ok()
}

/// Setups that the end turns build on (contacts, war, technologies, gold, influence).
pub fn before_turns(app: &mut GameApp) {
    let Some(session) = app.module.session.as_mut() else {
        return;
    };
    let (game, rules) = session.harness_game();
    if let Some(rounds) = std::env::var("AURELIA_GAME_MAX_ROUNDS")
        .ok()
        .and_then(|v| v.parse().ok())
    {
        let options = classic_rules::world::victory::VictoryOptions {
            disabled: 0,
            max_rounds: rounds,
        };
        game.debug_set_victory_options(options);
    }
    if wants("meet") {
        game.debug_meet_all();
    }
    if wants("war") {
        game.debug_meet(ME, RIVAL);
        report(
            game.execute(
                &rules,
                Command::DeclareWar {
                    player: ME,
                    enemy: RIVAL,
                },
            )
            .map(|_| ()),
            "war",
        );
    }
    if wants("printing") {
        learn(game, &rules, "core:tech/printing-press");
    }
    if wants("gold") {
        game.debug_earn(ME, 1200);
    }
    if wants("influence") {
        city_state_standings(game, &rules);
    }
    session.refresh_after_harness();
}

/// Setups shown as they are (an offer, projects, the combat overlays).
pub fn after_turns(app: &mut GameApp) -> Option<TileId> {
    let session = app.module.session.as_mut()?;
    let (game, rules) = session.harness_game();
    if wants("offer") {
        rival_offer(game, &rules);
    }
    if wants("apollo") {
        complete_projects(game, &rules);
    }
    if wants("spy") {
        send_spy_home(game, &rules);
    }
    let mut target = None;
    if wants("combat") || wants("promotion") {
        target = enemy_beside_warrior(game, &rules);
    }
    if wants("capture") {
        capture_city(game, &rules);
    }
    if wants("ruins") {
        ruins_under_warrior(game, &rules);
    }
    session.refresh_after_harness();
    if wants("citystate") || wants("citystate-hover") {
        target = target.or(city_state_scene(session, wants("citystate")));
    }
    let warrior = own_warrior(session.game(), &rules);
    if let Some(unit) = warrior.filter(|_| target.is_some()) {
        session.select_unit(unit);
    }
    target
}

/// A setup is on that puts the camera on a city-state's city (`citystate`, `citystate-hover`).
pub fn wants_city_state() -> bool {
    wants("citystate") || wants("citystate-hover")
}

/// `citystate`: the city of the first met city-state is revealed and its card opened as the
/// banner click does (`open`); `citystate-hover` leaves the card shut. The city's tile.
fn city_state_scene(session: &mut crate::game::Session, open: bool) -> Option<TileId> {
    let auth = session.game().view().auth;
    let tile = (0..auth.players().count() as u8).map(PlayerId).filter(|p| auth.players().is_city_state(*p)).find_map(|p| {
        let met = classic_rules::derived::tourism::met(auth, ME, p);
        let city = *auth.indexes().cities_of(p).first()?;
        met.then(|| auth.cities().get(city).map(|c| c.tile)).flatten()
    });
    let Some(tile) = tile else {
        println!("AURELIA_GAME_SETUP citystate: no met city-state with a city (use the setups meet or influence)");
        return None;
    };
    let (game, _) = session.harness_game();
    let dims = game.view().auth.map().dimensions;
    game.debug_reveal(ME, &classic_rules::math::tiles_in_hex_radius(&dims, tile, 3));
    session.refresh_after_harness();
    if open {
        session.open_city_state_at(tile);
    }
    Some(tile)
}

/// `ruins`: Ancient Ruins appear under the own warrior (it stands on them: the card opens).
fn ruins_under_warrior(game: &mut GameState, rules: &CompiledRuleset) {
    let Some(unit) = own_warrior(game, rules) else {
        println!("AURELIA_GAME_SETUP ruins: no own warrior");
        return;
    };
    let Some(core) = game.view().auth.units().get(unit).map(|u| u.core) else {
        return;
    };
    let Some(ruins) = rules.improvements.iter().position(|i| i.key == "core:improvement/ancient-ruins") else {
        println!("AURELIA_GAME_SETUP ruins: the ruleset has no Ancient Ruins");
        return;
    };
    game.debug_set_improvement_batch(rules, &[(core.tile, classic_rules::ids::ImprovementId(ruins as u16))]);
}

/// `spy`: the first spy goes to the capital for counter-intelligence.
fn send_spy_home(game: &mut GameState, rules: &CompiledRuleset) {
    let city = game.view().auth.cities().iter().find(|(_, c)| c.owner == ME).map(|(handle, _)| handle);
    let Some(city) = city else {
        println!("AURELIA_GAME_SETUP spy: no own city");
        return;
    };
    let order = Command::MoveSpy { player: ME, spy: 0, city: Some(city), diplomat: false };
    report(game.execute(rules, order).map(|_| ()), "spy");
}

pub fn wants_promotions() -> bool {
    wants("promotion")
}

fn report(result: Result<(), classic_rules::command::CommandError>, what: &str) {
    if let Err(error) = result {
        println!("AURELIA_GAME_SETUP {what}: {error:?}");
    }
}

fn learn(game: &mut GameState, rules: &CompiledRuleset, key: &str) {
    let Some(index) = rules.technologies.iter().position(|t| t.key == key) else {
        println!("AURELIA_GAME_SETUP: unknown technology {key}");
        return;
    };
    game.debug_learn(rules, ME, classic_rules::ids::TechId(index as u16));
}

/// Influence from war to allies with the city-states, a few quests, one pledge.
fn city_state_standings(game: &mut GameState, rules: &CompiledRuleset) {
    let auth = game.view().auth;
    let minors: Vec<PlayerId> = (0..auth.players().count() as u8)
        .map(PlayerId)
        .filter(|p| auth.players().is_city_state(*p))
        .collect();
    let levels = [72, 38, 12, -25];
    let quests = [
        QuestKind::PledgeToProtect,
        QuestKind::GiveGold { gifted: 0 },
        QuestKind::Invest,
        QuestKind::FindPlayer(RIVAL),
    ];
    for (i, minor) in minors.iter().enumerate() {
        game.debug_meet(ME, *minor);
        game.debug_set_influence(rules, *minor, ME, levels[i % levels.len()]);
        game.debug_add_quest(*minor, ME, quests[i % quests.len()], None);
    }
}

/// Player 1 offers gold for open borders and an embassy (on its own turn).
fn rival_offer(game: &mut GameState, rules: &CompiledRuleset) {
    game.debug_meet(ME, RIVAL);
    // The embassy offer must meet the real prerequisite on both sides.
    if let Some(index) = rules.technologies.iter().position(|t| t.key == "core:tech/writing") {
        for player in [ME, RIVAL] { game.debug_learn(rules, player, classic_rules::ids::TechId(index as u16)); }
    }
    game.debug_earn(RIVAL, 200);
    end_turns_until(game, rules, RIVAL);
    // At war (`war`): peace; otherwise an exchange of embassies.
    let items = if game.view().auth.diplomacy().at_war(ME, RIVAL) {
        vec![(RIVAL, DealItem::Peace)]
    } else {
        vec![(RIVAL, DealItem::Embassy), (ME, DealItem::Embassy)]
    };
    let offer = Command::ProposeDeal {
        from: RIVAL,
        to: ME,
        items,
    };
    report(game.execute(rules, offer).map(|_| ()), "offer");
    end_turns_until(game, rules, ME);
}

fn end_turns_until(game: &mut GameState, rules: &CompiledRuleset, player: PlayerId) {
    for _ in 0..game.view().auth.players().count() * 2 {
        let active = game.view().auth.active_player();
        if active == player {
            return;
        }
        report(
            game.execute(rules, Command::EndTurn { player: active })
                .map(|_| ()),
            "end turn",
        );
    }
}

fn complete_projects(game: &mut GameState, rules: &CompiledRuleset) {
    let keys = ["core:project/apollo_program", "core:project/ss_booster"];
    for key in keys {
        match rules.module.projects.iter().position(|p| p.key == key) {
            Some(i) => game.debug_add_project(ME, ProjectId(i as u16)),
            None => println!("AURELIA_GAME_SETUP: unknown project {key}"),
        }
    }
}

fn kind(rules: &CompiledRuleset, key: &str) -> Option<UnitKindId> {
    let index = rules.units.iter().position(|u| u.key == key)?;
    Some(UnitKindId(index as u16))
}

fn own_warrior(game: &GameState, rules: &CompiledRuleset) -> Option<UnitHandle> {
    let warrior = kind(rules, "core:unit/warrior")?;
    let auth = game.view().auth;
    let units = auth.indexes().units_of(ME);
    units
        .iter()
        .copied()
        .find(|u| auth.units().get(*u).is_some_and(|v| v.core.kind == warrior))
}

/// Free land next to `tile`: no city, no unit.
fn free_land_beside(game: &GameState, tile: TileId, radius: i32) -> Vec<TileId> {
    let auth = game.view().auth;
    let dims = auth.map().dimensions;
    classic_rules::math::tiles_in_hex_radius(&dims, tile, radius)
        .into_iter()
        .filter(|t| dims.distance(tile, *t) == Some(radius))
        .filter(|t| classic_rules::navigation::cost::enter_cost_on_map(auth.map(), *t).is_some())
        .filter(|t| {
            auth.indexes().city_on_tile(*t).is_none() && auth.indexes().units_on(*t).is_empty()
        })
        .collect()
}

/// A rival warrior beside the own warrior, at war; with `promotion` the warrior has
/// the experience for a promotion. Returns the enemy's tile.
fn enemy_beside_warrior(game: &mut GameState, rules: &CompiledRuleset) -> Option<TileId> {
    let unit = own_warrior(game, rules)?;
    let tile = game.view().auth.units().get(unit)?.core.tile;
    let target = *free_land_beside(game, tile, 1).first()?;
    game.debug_spawn_unit(rules, RIVAL, target, kind(rules, "core:unit/warrior")?);
    game.debug_meet(ME, RIVAL);
    if !game.view().auth.diplomacy().at_war(ME, RIVAL) {
        report(
            game.execute(
                rules,
                Command::DeclareWar {
                    player: ME,
                    enemy: RIVAL,
                },
            )
            .map(|_| ()),
            "war",
        );
    }
    if wants("promotion") {
        game.debug_set_experience(unit, 60);
    }
    Some(target)
}

/// Player 1 founds a capital and a second city three tiles from the own capital; a
/// fresh warrior takes the second one (it waits for the decision).
fn capture_city(game: &mut GameState, rules: &CompiledRuleset) {
    let Some(capital) = game.view().auth.indexes().cities_of(ME).first().copied() else {
        return;
    };
    let home = game.view().auth.cities().get(capital).map(|c| c.tile);
    let Some(home) = home else {
        return;
    };
    let sites = free_land_beside(game, home, 3);
    let (Some(first), Some(second)) = (sites.last().copied(), sites.first().copied()) else {
        return;
    };
    game.debug_found_city(rules, RIVAL, first);
    let city = game.debug_found_city(rules, RIVAL, second);
    let Some(attacker) = spawn_beside(game, rules, second) else {
        return;
    };
    game.debug_meet(ME, RIVAL);
    if !game.view().auth.diplomacy().at_war(ME, RIVAL) {
        report(
            game.execute(
                rules,
                Command::DeclareWar {
                    player: ME,
                    enemy: RIVAL,
                },
            )
            .map(|_| ()),
            "war",
        );
    }
    weaken(game, rules, city);
    report(
        game.execute(rules, Command::AttackCity { attacker, city })
            .map(|_| ()),
        "capture",
    );
}

fn spawn_beside(game: &mut GameState, rules: &CompiledRuleset, tile: TileId) -> Option<UnitHandle> {
    let beside = *free_land_beside(game, tile, 1).first()?;
    let swordsman = kind(rules, "core:unit/swordsman")?;
    Some(game.debug_spawn_unit(rules, ME, beside, swordsman))
}

/// The city is one hit point from falling.
fn weaken(game: &mut GameState, rules: &CompiledRuleset, city: CityHandle) {
    let auth = game.view().auth;
    let Some(core) = auth.cities().get(city) else {
        return;
    };
    let max = classic_rules::combat::city::max_hit_points(auth, rules, core);
    game.debug_set_city_damage(city, max.saturating_sub(1));
}

/// `embark`: four soldiers afloat on the water nearest to a shore tile, one warrior on the
/// shore, the first of the four selected (its panel says "Embarked"). Returns the shore
/// tile to centre the camera on.
pub fn embark_scene(app: &mut GameApp) -> Option<TileId> {
    if !wants("embark") {
        return None;
    }
    let session = app.module.session.as_mut()?;
    let (game, rules) = session.harness_game();
    let auth = game.view().auth;
    let map = auth.map();
    let dims = map.dimensions;
    let home = auth.units().iter_cores().find(|u| u.owner == ME).map(|u| u.tile)?;
    let is_water = |t: &TileId| map.terrain(*t).is_some_and(|k| map.terrain_kinds().is_water(k.0));
    let free = |t: &TileId| auth.indexes().units_on(*t).is_empty() && auth.indexes().city_on_tile(*t).is_none();
    // The shore tile nearest to home with four water tiles within two steps.
    let mut shores: Vec<TileId> = (0..map.tile_count() as u32).map(TileId).filter(|t| !is_water(t) && free(t)).collect();
    shores.sort_by_key(|t| dims.distance(home, *t).unwrap_or(i32::MAX));
    let (shore, mut water) = shores.into_iter().find_map(|shore| {
        let mut water: Vec<TileId> = classic_rules::math::tiles_in_hex_radius(&dims, shore, 3)
            .into_iter()
            .filter(|t| is_water(t) && free(t) && classic_rules::navigation::terrain::is_water(map, *t))
            .collect();
        water.sort_by_key(|t| (dims.distance(shore, *t).unwrap_or(i32::MAX), t.0));
        (water.len() >= 4).then_some((shore, water))
    })?;
    water.truncate(4);
    let kind_of = |key: &str| rules.units.iter().position(|u| u.key == key).map(|i| UnitKindId(i as u16));
    let soldiers = ["core:unit/warrior", "core:unit/swordsman", "core:unit/spearman", "core:unit/archer"];
    let mut first = None;
    for (tile, key) in water.iter().zip(soldiers) {
        let unit = game.debug_spawn_unit(&rules, ME, *tile, kind_of(key)?);
        first = first.or(Some(unit));
    }
    game.debug_spawn_unit(&rules, ME, shore, kind_of("core:unit/warrior")?);
    session.refresh_after_harness();
    session.select_unit(first?);
    Some(shore)
}
