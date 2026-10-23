# PlayerAgent: one interface for whoever plays a seat

Design: `platform/docs/kernel-audit.md` section "PlayerAgent" (PA.1-PA.5). This file records what was built, the choices the design left open and why, and what is not done. Code: `platform/crates/kernel/src/agent/`.

## What it is

Every seat of a game is played through one trait. A human at a screen, the module's AI, a network client, a replay and a scripted test differ only in how they answer `poll`:

```rust
pub trait PlayerAgent<M: RulesModule>: Any {
    fn poll(&mut self, cx: &AgentContext<'_, M>) -> AgentPoll<M::Command>;
    fn observe(&mut self, event: AgentEvent<M>) {}
    fn is_interactive(&self) -> bool { false }
    fn configure_game(&self, game: &mut GameState<M>) {}
}
```

The agent receives the seat's observation (`cx.view()`, a `PlayerView`) and returns the next step: `Command(c)`, `EndTurn`, `Pending`, `Working`, `Boundary`. The host, `SeatTable<M>`, executes commands through `GameState::execute`, the only write boundary, and gives the result back through `observe`. The kernel stays generic over `RulesModule`; nothing in `agent/` names a module.

`SeatTable::step` has the outcomes of `AiPlayers::step` (`Worked`, `PlayerDone(seat)`, `RoundDone`, `Waiting`), so a host loop does not care whether a table or the AI machine plays. `SeatTable::run(game, rules, max_steps)` is the slicing loop.

Nothing here is saved or hashed. `GameSettings.ai` stays the saved default of who drives a seat on load; the table of live agents is session state.

## Choices the design left open

| Choice | Decision | Reason |
|---|---|---|
| A call that did work but has no command (the AI observing, planning) | a fifth answer, `Working`; `Pending` keeps its meaning "nothing until something arrives from outside" | the host must tell "poll again now" from "stop asking": one answer for both made every human seat spin and made the AI unsliceable. `Working` is also what lets the caller slice by wall time without a budget parameter |
| `StepBudget` in the context | dropped | every poll is one bounded piece of work (one `AiPlayers::advance`); the deterministic budget stays in the AI's own `DecisionBudget`; wall time is the caller's slicing of `step` |
| Who ends a turn | `EndTurn` makes the host execute `M::end_turn(seat)` | the agent need not know the shape of a module's end-turn command; the host reports `PlayerDone(seat)` when it applied |
| `Boundary` | passed through as `PlayerDone(seat)` without ending anything | a presentation hook for an agent that wants a redraw between commands |
| `AgentEvent` | owned (`Executed { command, result }`), no lifetime | the result moves into the agent, which keeps what it needs (its journal, a message); the host does not clone the journal of every command |
| One agent for several seats | `SeatTable::assign(&[seats], agent)`; `cx.seat` names the asked seat | the AI shares one machine and one memory (`AiStates`) across its players; one box per seat would need a shared lock |
| `Send` on the trait | dropped; `Any` instead | `M::Ai: Send` is not provable for every module (`AiDriver<M, P>` is Send only if the mind, its brief and the module's change type are; Civ V's `Box<dyn SystemAdvisor>` is not), so a `Send` supertrait would push bounds into every generic host. A host that moves a table to a thread checks `Send` at the concrete types. `Any` lets the host read an agent back (`SeatTable::agent_ref::<A>`) |
| When a round is done | the table reports `RoundDone` after the `PlayerDone` of an *unattended* agent (`!is_interactive()`: the AI, a script) once the turn passed to an interactive agent (human, remote) or to nobody, or the round moved on; a human's own end of turn gives none. The ended agent hears `AgentEvent::TurnsPassed` | this is exactly what `AiDriver` reported (`RoundDone` iff the next player is not an AI player or the round changed), so a host loop written for `AiPlayers::step` keeps working; the AI driver uses `TurnsPassed` to go back to `Idle` (`AiPlayers::turns_passed`) |
| Seats of a game whose size the host does not know | `SeatTable::assign_rest(agent)`: plays every seat without an agent of its own | the client builds its driver before the game exists; humans get explicit seats, the AI gets "the rest" |
| Execution of commands for a window of simultaneous seats | not in the table yet (P4) | the table plays the active seat; `execute_as` and multi-seat polling arrive with `net/multiplayer` |
| Setup of a game nobody configured | `PlayerAgent::configure_game`, called once before an agent's first poll | the AI driver did this inside `step`; the settings are saved and hashed, so forgetting it would change a hash silently |
| Agents and the authoritative world | `AgentContext` hands every agent a `PlayerView` only; the game is a private field readable inside `kernel::agent` | the module's sensors build the AI's observation from the game (classic/docs/ai-truth-reads.md: the driver is the sensor); only `AiAgent`, the kernel's adapter, reaches it. A human, scripted, remote or learned agent written anywhere else cannot |

## The AI as an agent: how `poll` meets the driver

`AiDriver::step(game, rules)` read the game, decided and executed. It is now two halves with the same state machine: `AiPlayers::advance(&game, rules)` reads the game and either did a piece of work (`Advance::Worked`), has a command (`Command`, `EndTurn`) or has nothing (`Waiting`, `RoundDone`); `AiPlayers::executed(result)` takes the result of the command back (journal, trace, the unit's command budget). `step` is `advance`, `GameState::execute`, `executed`, so the direct path and the agent path run the same code and the pins prove they agree. `AiAgent::poll` is one `advance`; `observe(Executed)` is `executed`.

## Phases

- **P1** (done): `PlayerAgent`, `AgentPoll`, `AgentEvent`, `AgentContext`, `SeatTable`, `ScriptedAgent`. Test: `sandbox-rules/tests/seat_table.rs` records the pinned AI game of the sandbox (seed 7_2026), splits the commands into one script per seat, plays them through a `SeatTable` of `ScriptedAgent`s and asserts the pinned hash `0x9c86cf50efeeff63`, also with the calls sliced to 7 steps.
- **P2** (done): `AiPlayers::{advance, executed, turns_passed}`, `AiAgent`, `AgentEvent::TurnsPassed`, `PlayerAgent::{is_interactive, configure_game}`. Tests: the sandbox pin `0x9c86cf50efeeff63` and the L&L pin `0x1e442d91a1c84fa9` through `SeatTable` + `AiAgent` (same commands as the direct driver); Civ V (4 players, 40 rounds): hash after every round, the command log and the AI memory equal to the direct driver at 1, 7 and 100 steps per call; a seat without an agent waits for its human (`classic-rules/tests/seat_table.rs`).
- **P3** (done for the end of a human turn; the rest of the UI is not moved): `client::human` (`HumanAgent` for the table, `HumanSeat` for the UI: `submit`, `end_turn`, `take_result`, journal), and `SimulationDriver` now owns a `SeatTable`: one `HumanAgent` per human seat, one `AiAgent` as `assign_rest` for every other seat. `end_human_turn` queues the end of turn on the human's seat and steps the table until the order is executed (same synchronous answer as before: the player hears at once if the game refuses); `tick`, `tick_game` and the AI memory calls go through the table and the AI agent; the humans list inside the AI driver (`AiPlayers::is_human`) is no longer asked by the client. Tests: the client's `driver_round`, `driver_offload` (offloaded rounds, slicing, journal) are unchanged and green; `client/tests/human_seat.rs` (queue, `Pending`, refusal reaches the player, journal, no round end after a human's own end of turn).

## Not done

- Interactive orders of the UI (`GameClient::submit`, the controller) still call `GameState::execute` directly with an immediate projection update; only the end of turn goes through the human agent. Moving them needs the table (or a handle to it) next to `GameClient`, which today lives in `SimulationDriver`; proposed: `GameClient` owns the `SeatTable` and `SimulationDriver` shrinks to the AI offload and the wall-time slicing.
- The offloaded AI round (`ai_round`) is still a worker beside the table, not an agent behind `AiAgent` (the doc's PA.3 intent). It reads the AI memory through the AI agent; making it an executor of `AiAgent::poll` is the next step.
- Windows of simultaneous seats, `RemoteAgent` and takeover (P4), recording and replay decorators and the learner hook (P5).
- `AiAgent` keeps the machine's own `humans` list (`AiPlayers::for_game(.., humans)`) and the table's seat assignment in agreement by construction; `SeatTable::replace` of an AI seat by a human does not call `set_humans` on the machine.
