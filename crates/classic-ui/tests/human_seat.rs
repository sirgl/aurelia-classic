//! PlayerAgent P3 (classic/docs/player-agent.md): a human seat is a `HumanAgent` in the table of
//! seats. Its orders are queued by the UI's handle, answered `Pending` while the queue is
//! empty, executed through `GameState::execute`, and their results come back to the handle.

use classic_rules::classic::Classic;
use classic_rules::ids::PlayerId;
use classic_rules::rules::SourceRules;
use classic_rules::scenario::{generate, ScenarioConfig};
use classic_rules::{compile_rules, Command};
use client::human::human_seat;
use kernel::agent::SeatTable;
use kernel::ai::StepOutcome;

#[test]
fn a_human_seat_waits_executes_refuses_and_ends_its_turn() {
    let rules = compile_rules(SourceRules::default()).unwrap();
    let cfg = ScenarioConfig {
        seed: 5,
        players: 2,
        width: 12,
        height: 12,
        cities: 0,
        units: 0,
        water_ratio: 0,
    };
    let mut game = generate(&cfg, &rules);
    let dims = game.view().auth.map().dimensions;
    for (p, (q, r)) in [(2, 2), (9, 9)].into_iter().enumerate() {
        let tile = dims
            .to_tile(classic_rules::math::HexCoord { q, r })
            .unwrap();
        game.debug_found_city(&rules, PlayerId(p as u8), tile);
    }
    let (agent, seat) = human_seat::<Classic>();
    seat.record_journal();
    let mut table: SeatTable<Classic> = SeatTable::new();
    table.assign(&[PlayerId(0)], Box::new(agent));

    assert_eq!(
        table.step(&mut game, &rules),
        StepOutcome::Waiting,
        "an empty queue: the player is thinking"
    );
    assert!(!seat.has_orders());

    // An order for another player's turn is refused, and the player hears it.
    seat.submit(Command::EndTurn {
        player: PlayerId(1),
    });
    assert_eq!(table.step(&mut game, &rules), StepOutcome::Worked);
    assert!(seat.take_result().expect("a result").is_err());
    assert_eq!(game.view().auth.active_player(), PlayerId(0));
    assert!(
        seat.take_journal().is_empty(),
        "a refused command is not journaled"
    );

    seat.end_turn();
    assert_eq!(
        table.step(&mut game, &rules),
        StepOutcome::PlayerDone(PlayerId(0))
    );
    assert!(seat.take_result().expect("a result").is_ok());
    assert_eq!(
        seat.take_journal().len(),
        1,
        "the applied end of turn is journaled"
    );
    assert_eq!(
        game.view().auth.active_player(),
        PlayerId(1),
        "the turn moved on"
    );
    // The next seat has no agent: nothing is played, and a human's own end of turn is no round end.
    assert_eq!(table.step(&mut game, &rules), StepOutcome::Waiting);
}
