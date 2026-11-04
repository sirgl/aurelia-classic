//! The observer over the classic rules: a nation's panel knows what the regular
//! game knows of it, cell by cell, at the end of every round (units open the land round
//! them, a city that grows takes the land it grew onto, a unit that moves opens new land
//! and gives the old up to the fog), and the whole map, when the viewer opens it, shows
//! everything and changes nothing in the match.

use classic_rules::ids::{PlayerId, TileId};
use classic_ui::observer::{ClassicMatch, ClassicPowers};
use classic_ui::ui::screens::{game_rules, prepare_game, GameOptions, MapSize, MatchRequest, NewGame};
use client::shell::observer::shell::ObserverShell;
use client::shell::observer::speed::{Clock, Speed};
use client::shell::observer::Observed;
use kernel::derived::visibility::{covered_tiles, CITY_SIGHT_RADIUS, PLOT_SIGHT_RADIUS};
use render::gpu::terrain_gpu::TerrainSync;
use render::world::projection;
use render::world::state::RenderWorld;
use std::sync::Arc;

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const ROUNDS: u32 = 60;

fn observed() -> ClassicMatch {
    let request = MatchRequest { nation: "core:nation/rome".into(), seed: 7, size: MapSize::Small, players: 4, water: 25 };
    let prepared = prepare_game(NewGame { request, options: GameOptions::default() }).expect("a world");
    ClassicMatch::new(prepared.game, game_rules(), ClassicPowers, None)
}

fn shell_of(obs: &ClassicMatch) -> ObserverShell {
    ObserverShell::new(obs, FORMAT, Arc::new(Vec::new()), TerrainSync::Immediate, Clock::new(Speed::Max))
}

/// The first tile where two pictures of the same size differ.
fn first_difference(a: &[u8], b: &[u8]) -> Option<usize> {
    a.iter().zip(b).position(|(x, y)| x != y).or((a.len() != b.len()).then(|| a.len().min(b.len())))
}

/// What happened to a power between two rounds.
#[derive(Default)]
struct Tally {
    /// An own city grew.
    grown: u32,
    /// The territory of a power grew.
    expanded: u32,
    /// An own unit stood on another tile than a round before.
    moved: u32,
    /// The panel showed a unit of another power.
    strangers: u32,
    /// The picture of a power opened land it did not see a round before.
    opened: u32,
    /// The picture of a power lost sight of land it saw a round before.
    closed: u32,
}

struct Before {
    population: Vec<(u32, u32)>,
    owned: usize,
    units: Vec<(u64, u32)>,
    visible: Vec<u8>,
}

fn before_of(world: &RenderWorld, owned: usize) -> Before {
    Before {
        population: world.cities.iter().filter(|c| c.owner == world.observer).map(|c| (c.tile.0, c.population.expect("an own city shows its population"))).collect(),
        owned,
        units: world.units.iter().filter(|u| u.owner == world.observer).map(|u| (u.id, u.tile.0)).collect(),
        visible: world.visible.clone(),
    }
}

#[test]
fn every_panel_knows_what_the_regular_game_knows_after_every_round() {
    let mut obs = observed();
    let mut shell = shell_of(&obs);
    let mut tally = Tally::default();
    let mut last: Vec<Option<Before>> = (0..obs.powers().len()).map(|_| None).collect();
    for round in 2..=ROUNDS {
        shell.play_to(&mut obs, round);
        let game = obs.game().view();
        let (auth, derived) = (game.auth, game.derived);
        let map = auth.map();
        for power in 0..obs.powers().len() {
            let player = PlayerId(obs.powers()[power].player);
            let world = obs.world(power);
            // The regular game's view of the same power at the same moment.
            let mut regular = projection::rebuild_render_world(game, player, obs.rules());
            // The surface mode is the panel's own choice, not knowledge.
            regular.visual_surface = world.visual_surface;
            let at = |what: &str| format!("round {round}, power {power}: {what}");
            assert_eq!(first_difference(&world.visible, &regular.visible), None, "{}", at("the land it sees"));
            assert_eq!(first_difference(&world.explored, &regular.explored), None, "{}", at("the land it explored"));
            assert_eq!(world.units, regular.units, "{}", at("the units it sees"));
            assert_eq!(world.cities, regular.cities, "{}", at("the cities it knows"));
            assert!(projection::changed_layers(world, &regular).is_empty(), "{}", at("a layer of the picture differs"));
            // And it is the sight of the rules: round every own unit, every own city and every
            // tile its power holds, the land is in sight.
            let mut missing = Vec::new();
            let mut covered = std::collections::HashSet::new();
            let mut check = |at: TileId, radius: i32, what: &str| {
                for t in covered_tiles(map, at, radius) {
                    covered.insert(t.0);
                    if world.visible[t.0 as usize] != 1 {
                        missing.push(format!("{what} at {at:?} r{radius}: {t:?}"));
                    }
                }
            };
            for (handle, core) in auth.units().iter_handles() {
                if core.owner == player {
                    check(core.tile, derived.visibility.sight_of(handle), "unit");
                }
            }
            for (_, city) in auth.cities().iter() {
                if city.owner == player {
                    check(city.tile, CITY_SIGHT_RADIUS, "city");
                }
            }
            let mut owned = 0;
            for t in 0..map.tile_count() as u32 {
                if map.owner(TileId(t)).flatten() == Some(player) {
                    owned += 1;
                    check(TileId(t), PLOT_SIGHT_RADIUS, "owned tile");
                }
            }
            assert!(missing.is_empty(), "{}: {:?}", at("land the rules see is not in the panel"), &missing[..missing.len().min(4)]);
            // And the other way: the panel sees nothing that no unit, city or border of its power
            // sees (a panel that showed more would be a leak of the whole map).
            let extra: Vec<usize> = (0..world.visible.len()).filter(|&t| world.visible[t] == 1 && !covered.contains(&(t as u32))).take(4).collect();
            assert!(extra.is_empty(), "{}: {extra:?}", at("tiles in sight that nothing of the power sees"));
            if let Some(then) = &last[power] {
                let now = before_of(world, owned);
                let grew = now.population.iter().filter(|(tile, pop)| then.population.iter().any(|(t, p)| t == tile && p < pop)).count() as u32;
                if grew > 0 {
                    eprintln!("round {round}, power {power}: {grew} city grew; land in sight {} -> {}", then.visible.iter().filter(|v| **v == 1).count(), now.visible.iter().filter(|v| **v == 1).count());
                }
                tally.grown += grew;
                tally.expanded += u32::from(now.owned > then.owned);
                tally.moved += now.units.iter().filter(|(id, tile)| then.units.iter().any(|(i, t)| i == id && t != tile)).count() as u32;
                tally.opened += u32::from(now.visible.iter().zip(&then.visible).any(|(n, t)| *n == 1 && *t == 0));
                tally.closed += u32::from(now.visible.iter().zip(&then.visible).any(|(n, t)| *n == 0 && *t == 1));
            }
            tally.strangers += u32::from(world.units.iter().any(|u| u.owner != world.observer));
            last[power] = Some(before_of(world, owned));
        }
    }
    // The check means something only if the match did these things in front of the panels.
    assert!(tally.grown > 0, "no city grew in {ROUNDS} rounds");
    assert!(tally.expanded > 0, "no territory grew in {ROUNDS} rounds");
    assert!(tally.moved > 0, "no unit moved in {ROUNDS} rounds");
    assert!(tally.opened > 0 && tally.closed > 0, "the fog never moved: {} opened, {} closed", tally.opened, tally.closed);
    assert!(tally.strangers > 0, "no panel ever saw a foreign unit");
    eprintln!("grown {} expanded {} moved {} opened {} closed {} strangers {}", tally.grown, tally.expanded, tally.moved, tally.opened, tally.closed, tally.strangers);
}

#[test]
fn the_whole_map_shows_everything_on_the_fly_and_changes_nothing_in_the_match() {
    let mut plain = observed();
    let mut plain_shell = shell_of(&plain);
    plain_shell.play_to(&mut plain, 40);

    let mut open = observed();
    let mut shell = shell_of(&open);
    shell.play_to(&mut open, 10);
    let fog = open.world(0).visible.iter().filter(|v| **v == 1).count();
    shell.toggle_reveal(&mut open);
    assert!(open.revealed());
    for round in 11..=40 {
        shell.play_to(&mut open, round);
        if round == 12 {
            // Open: every tile in every picture, and every unit and city of the game.
            let game = open.game().view();
            let units = game.auth.units().iter_handles().count();
            let cities = game.auth.cities().iter().count();
            for power in 0..open.powers().len() {
                let world = open.world(power);
                assert!(world.visible.iter().all(|v| *v == 1) && world.explored.iter().all(|v| *v == 1), "power {power}: fog on an open map");
                assert_eq!((world.units.len(), world.cities.len()), (units, cities), "power {power}: every unit and city of the game");
            }
            assert!(units > 0 && cities > 0);
            assert!(open.world(0).visible.len() > fog, "the map was not open before");
            // Closed again, on the fly: the power's own knowledge.
            shell.toggle_reveal(&mut open);
            let game = open.game().view();
            let regular = projection::rebuild_render_world(game, PlayerId(open.powers()[0].player), open.rules());
            assert_eq!(open.world(0).visible, regular.visible, "the fog is back as the regular game has it");
            assert_eq!(open.world(0).units, regular.units);
            shell.toggle_reveal(&mut open);
        }
        if round % 7 == 0 {
            shell.toggle_reveal(&mut open);
        }
    }
    assert_eq!(open.state_hash(), plain.state_hash(), "opening the map reaches the view and not the world");
}
