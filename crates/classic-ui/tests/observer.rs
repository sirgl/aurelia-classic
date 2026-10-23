//! The observer mode over the classic rules: the match the observer plays is the match the plain AI
//! driver plays, whatever the slices of work, and the panels show only what each
//! civilization knows.
use std::time::Duration;

use classic_rules::ai::AiDriver;
use classic_rules::ids::PlayerId;
use classic_rules::replay::hash::authoritative_hash;
use classic_ui::observer::{ClassicMatch, ClassicPowers};
use classic_ui::ui::screens::{game_rules, prepare_game, GameOptions, MapSize, MatchRequest, NewGame, PreparedMatch};
use client::shell::observer::Observed;
use kernel::ai::{AiPlayers, StepOutcome};

const ROUNDS: u32 = 25;

fn prepared() -> PreparedMatch {
    let request = MatchRequest { civilization: "core:nation/rome".into(), seed: 7, size: MapSize::Medium, players: 4, water: 25 };
    prepare_game(NewGame { request, options: GameOptions::default() }).expect("a world")
}

/// The plain driver: every seat the AI's, stepped to round `ROUNDS`.
fn plain() -> u64 {
    let rules = game_rules();
    let mut game = prepared().game;
    let mut ai = AiDriver::for_game(&rules, rules.default_difficulty(), Vec::new());
    ai.configure_game(&mut game);
    while !game.is_over() && game.view().auth.turn().round < ROUNDS {
        let outcome = ai.step(&mut game, &rules);
        assert!(outcome != StepOutcome::Waiting || game.is_over(), "an AI-only game never waits");
    }
    authoritative_hash(&game)
}

fn observed() -> ClassicMatch {
    ClassicMatch::new(prepared().game, game_rules(), ClassicPowers, None)
}

#[test]
fn the_observer_plays_the_plain_drivers_game_whatever_the_slices() {
    let reference = plain();
    for slice in [Duration::ZERO, Duration::from_millis(3), Duration::from_secs(3600)] {
        let mut obs = observed();
        while !obs.is_over() && obs.game().view().auth.turn().round < ROUNDS {
            obs.advance(slice);
        }
        assert_eq!(obs.state_hash(), reference, "slice {slice:?}");
    }
}

#[test]
fn each_civilization_sees_only_its_own_knowledge() {
    let mut obs = observed();
    while obs.game().view().auth.turn().round < 20 {
        obs.advance(Duration::from_secs(3600));
    }
    let count = obs.powers().len();
    assert_eq!(count, 4, "the major nations have panels, the city-states none");
    let mut known = Vec::new();
    for power in 0..count {
        obs.refresh(power);
        let world = obs.world(power);
        let player = PlayerId(obs.powers()[power].player);
        assert_eq!(world.observer, player.0);
        let fresh = render::world::projection::rebuild_render_world(obs.game().view(), player, obs.rules());
        assert_eq!(world.explored, fresh.explored, "nation {power}: the refreshed view equals a rebuilt one");
        assert_eq!(world.visible, fresh.visible);
        assert!(world.units.iter().all(|u| world.visible.get(u.tile.0 as usize) == Some(&1)), "a unit on the fog");
        assert!(world.cities.iter().all(|c| world.is_known(c.tile.0 as usize)), "a city in the unexplored");
        let seen = (0..world.terrain.len()).filter(|&t| world.is_known(t)).count();
        assert!(seen > 0 && seen < world.terrain.len(), "it knows part of the map");
        known.push(world.explored.clone());
    }
    assert!(known.windows(2).any(|w| w[0] != w[1]), "the nations know different lands");
    for power in 0..count {
        assert!(obs.standing(power).len() >= 4, "the header has its figures");
    }
}

#[test]
fn one_civilization_shown_alone_has_its_details_and_events() {
    let mut obs = ClassicMatch::new(prepared().game, game_rules(), ClassicPowers, None).watching(1);
    assert_eq!(obs.first_view(), Some(1), "the view opens on the second nation alone");
    let (mut learned, mut progress) = (0, false);
    while obs.game().view().auth.turn().round < 60 {
        obs.advance(Duration::from_secs(3600));
        if !obs.mid_round() {
            obs.refresh(1);
            obs.refresh_details(1);
            learned = learned.max(obs.events(1).len());
            progress |= obs.details(1).iter().any(|d| d.percent.is_some());
        }
    }
    assert!(progress, "the research showed its progress in some round");
    assert!(obs.details(1).len() >= 2, "research and income: {:?}", obs.details(1));
    assert!(learned > 0, "a nation of sixty rounds has founded or learned something");
    assert!(obs.events(1).len() <= 5, "only the last events are kept");
    assert!(obs.details(0).is_empty(), "the ones not shown are not worked out");
    assert_eq!(obs.state_hash(), {
        let mut plain = observed();
        while plain.game().view().auth.turn().round < 60 {
            plain.advance(Duration::from_secs(3600));
        }
        plain.state_hash()
    }, "showing a nation alone does not reach the world");
}
