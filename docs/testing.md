# Testing

Run the commands from `classic/`. They use [cargo-nextest](https://nexte.st)
(`cargo install cargo-nextest --locked`, or the prebuilt binary:
`curl -LsSf https://get.nexte.st/latest/mac | tar zxf - -C ~/.cargo/bin`).
On a shared machine keep `--build-jobs 4 --test-threads 4` (in nextest `-j` is `--test-threads`, not the build jobs).

| What | Command | When |
|---|---|---|
| Changed only | `tools/test-changed.sh` | while working |
| Fast | `cargo +1.98.1 nextest run --cargo-profile fasttest --build-jobs 4 --test-threads 4 --workspace` | before a commit |
| Full | the same with `--profile full`, then `cargo +1.98.1 test --profile fasttest --workspace --doc`, and both again in `../platform` | release milestones, a moved pin |

Without nextest, `cargo +1.98.1 test --profile fasttest --workspace` runs the full set.

## Where the time went

Measured on 1 November 2026 (M-series, 16 cores, four build jobs and four test threads, the machine at load 45-120 from other
work, so the wall times are rough; the CPU times are steadier).

| | release | `fasttest` |
|---|---|---|
| Cold build of every test of classic | 31 min (4,100 CPU s) | 13 min (1,900 CPU s) |
| Rebuild after an edit in `classic-rules` | ~24 min (from the timings) | 2.3 min |
| Rebuild after an edit in a generic function of the kernel | ~25 min (from the timings) | 3.7 min |
| Fast set, 1,481 tests | ~40 s (estimated) | 2 min |
| Full set, 1,499 tests (no GPU) | 1.3 min | 5.5 min |
| `tools/test-changed.sh` after an edit in `classic-ui` (258 tests) | | 1.7 min, 8 s when built |
| platform: build of its tests (third-party crates already built) | 8.5 min | 2.4 min |
| platform: all 807 tests (no GPU) | 25 s | 14 s |

The first run after a rebuild spends up to a minute or two before the first test starts: nextest
lists the tests of every fresh binary, and the first launch of a new executable is slow on macOS
(most likely its security check; not measured separately).

The build is the cost, not the tests: all of classic's tests together take 300 CPU seconds in
release. Half of the build is the 124 integration-test binaries of `classic-rules` (each file in
`crates/classic-rules/tests/` is its own binary of ~25 s), and at opt-level 2 or 3 every one of them
is rebuilt from scratch after any edit below it, incremental compilation or not.

## The `fasttest` profile

`[profile.fasttest]` in `Cargo.toml` (and the same in `../platform/Cargo.toml`) inherits `release`
(no debug assertions, no overflow checks) with `opt-level = 1`, `codegen-units = 256` and
`incremental = true`. At opt-level 1 rustc shares generic instances between crates: a test binary
takes the kernel's and the rules' generics from the `classic-rules` library instead of compiling
its own, so an edit relinks the test binaries instead of recompiling them. Games run 3-4x slower
than in release, which the fast set absorbs.

Measured and rejected: `opt-level = 2` with incremental on. Its cold build costs as much as
release (4,200 CPU s) and a one-line edit in `classic-rules` still rebuilt everything (25 min).

Every pin passes under `fasttest`: the game is integer arithmetic and its hashes do not depend on
the opt-level (the full suite passed at opt-levels 2 and 3; at 1 every test passed but
`ai_selfplay::mid_game_starts_branch_from_a_log`, which is flaky at any level: it compares a
wall-clock `f64` after a JSON round trip without serde_json's `float_roundtrip`). Benches and
timings still use `--release`.

## The fast and the full set

`.config/nextest.toml` holds the sets; no test is changed or removed by them.

* `default` (fast) leaves out the GPU tests and the slow set: the 18 tests of 3 s and more in
  release (long AI games over many seeds or 200-500 rounds, two scans of the art catalogue). They are
  60% of the suite's test time.
* `full` runs everything `cargo test --workspace` runs. `--run-ignored all` adds the `#[ignore]`d
  measurements.
* The `pins` group is in the fast set and `tools/test-changed.sh` always runs it, whatever changed.
  Each pin takes under 3 s in release (12 s at `fasttest`), and between them any change of the rules, the AI, the kernel, the net's
  encoding or the save format moves a hash:

| Pin | Catches |
|---|---|
| `ai_bench` smoke suite (`standard` against itself, 70 rounds) | the AI, the rules and the kernel through a real AI game |
| `net_e2e_mind` encoding of a fixed state; a game of nets with pinned weights | the net's encoding and its mind |
| `baseline_hashes::core_game_hash_baseline` | a scripted core game: kernel and core rules without the AI |
| `classic_game` 100 rounds | a scripted classic game: the classic rules without the AI |
| `fuzz_sim::fuzz_world_hash_baseline` | random commands: the kernel's validation and transactions |
| `rules_fingerprint::the_classic_fingerprint_is_pinned` | the rule data (content, numbers) |
| `save_codec` classic saves byte for byte; `main_snapshot` | the save format and old saves |
| `ai_export::the_layout_is_pinned` | the training-data layout |

## Changed only: `tools/test-changed.sh`

It maps every changed file (`git diff --name-only` of the working tree against `HEAD` or a given ref,
plus untracked files) in classic and in `../platform` to its crate, then runs the fast tests of
those crates and of every crate that depends on them (dev-dependencies too), plus the pins. An edit in
the kernel therefore runs everything above it, in both checkouts. Files outside the crates:
`assets/rules` count as `classic-rules`, other assets as `classic-ui`, `docs/classic` as
`classic-content`; other docs, `tools/`, `nn/` and `*.md` run the pins only; anything else (a lock
file, a root `Cargo.toml`, `.config/`) runs everything.

```sh
tools/test-changed.sh                       # working tree against HEAD
tools/test-changed.sh origin/main           # against a ref; --platform-base REF for ../platform
tools/test-changed.sh --dry-run             # print the crates and the commands
tools/test-changed.sh --profile full        # the slow tests of those crates too
JOBS=8 tools/test-changed.sh -- --no-capture
```

It always builds with `--workspace` and selects tests with `-E`: `cargo test -p X` resolves the
features of `X` alone and rebuilds the shared crates with other features (a second copy of
`classic-rules` and everything above it).

## Shared builds

Several agents share this machine; the same crates were being built 5-6 times over.

* **One target directory per checkout: the default `target/`.** Do not set `CARGO_TARGET_DIR` or
  `--target-dir` to a private directory (`target/<name>/`): each one is a cold build, 13-31 minutes
  of four cores. Cargo locks the build directory, so a second build waits for the first
  (`Blocking waiting for file lock`) and then finds everything built; that wait is the point.
* **Pick the profile by the job:** `fasttest` for tests, `release` for benches, timings and the
  game. They live side by side in `target/fasttest/` and `target/release/`.
* **Frozen binaries.** To test or bench a fixed commit while the tree moves on, build once and copy
  the result out instead of keeping a copy of the tree with its own target directory:
  `cargo +1.98.1 nextest archive --cargo-profile fasttest --workspace --archive-file ~/civ/work/<name>.tar.zst`
  runs later with `cargo +1.98.1 nextest run --archive-file ~/civ/work/<name>.tar.zst --workspace-remap .`
  (no build); a bench binary is copied the same way (`cp target/release/ai-tournament ~/civ/work/bin/ai-tournament-<rev>`).
* **A scratch copy of the tree**, when one is unavoidable (a before/after comparison), holds both
  `classic/` and `platform/` in a fresh directory of its own: never put a copy beside `classic/`, the
  renderer's build script reads every sibling of `platform/` as a part and fails on a second
  `classic`. Give all copies one shared `CARGO_TARGET_DIR` (`~/civ/work/target-scratch`): the
  third-party crates are then built once for all of them.
* **sccache is not used.** It skips incremental crates (all local crates under `fasttest`) and
  linking, which is where the time goes; the shared target directory already shares the
  third-party crates.
