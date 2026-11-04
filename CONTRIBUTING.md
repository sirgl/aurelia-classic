# Contributing

Thank you for helping. This is a small project; a short issue before a big change saves everyone's time.

## Ground rules

* **English only** in everything public: code comments, documentation, commit messages, branch names, file names, issues.
  (Translations of the game texts live in `assets/lang/<code>/`.)
* Be kind and keep discussions on the work.
* **Sources:** nothing from the original game is used (no numbers, formulas, tables, texts, pictures, icons, sounds or music, not from memory either). Rule data comes from Unciv, or is our own design, or is common knowledge stated in words: see [docs/classic/SOURCING-POLICY.md](docs/classic/SOURCING-POLICY.md).
* Do not file security problems as public issues; use the repository's private vulnerability reporting.

## Licence of contributions

By submitting a contribution you agree that it is licensed under the terms of this repository: code under
"MIT OR Apache-2.0" (as in section 5 of the Apache License 2.0, inbound = outbound), art under CC BY-SA 4.0.
There is no CLA and no sign-off line is required. Do not submit code or media that you did not write or that has
an incompatible licence; if you add a dependency, add its licence to `THIRD-PARTY.md` (regenerate the table) and
mention it in the pull request.

## Workflow

1. Open an issue (bug or idea), or pick one.
2. Branch from `main`; name the branch `topic/short-description` (for example `fix/fog-edge`).
3. Keep commits small, with an imperative English subject line (`Fix the fog edge at the map border`) and a body that
   says why. Work done with an AI assistant carries a `Co-Authored-By:` trailer naming it.
4. Run the checks below, then open a pull request using the template.

## Checks before a pull request

This repository is private; the platform repository sits next to it (folders `platform/` and `classic/`). Run the commands from `classic/`.

```sh
cargo +1.98.1 check --workspace --all-targets
sh tools/rename-classic.sh --check
sh tools/sources-lint.sh
cargo +1.98.1 nextest run --cargo-profile fasttest --build-jobs 4 --test-threads 4 --workspace
```

* The game is deterministic: pinned hashes and snapshots in the tests fail when a rule changes. If you change a rule on
  purpose, update the pin in its own commit and say why.
* Rendering and UI changes need a screenshot in the pull request (`--screenshot out.png` renders without opening a window).
* The Rust toolchain is pinned (1.98.1); the tree is not yet rustfmt-clean, so do not reformat files you do not change.

## Running the tests and sharing builds

The details and the measurements are in [docs/testing.md](docs/testing.md).

* **Changed only**, while working: `tools/test-changed.sh` (the crates your diff touches, everything that depends on
  them, in classic and in `../platform`, plus the cheap pinned games).
* **Fast**, before a commit: `cargo +1.98.1 nextest run --cargo-profile fasttest --build-jobs 4 --test-threads 4 --workspace`.
* **Full**, at a release milestone or when a pin moves: the same with `--profile full`, then the same command in `../platform`.
* Build tests with `--cargo-profile fasttest` (release semantics at opt-level 1; an edit rebuilds the tests in minutes,
  not in half an hour). Keep `--release` for benches, timings and the game.
* **One target directory per checkout**, the default `target/`. Do not point `CARGO_TARGET_DIR` or `--target-dir` at a
  private directory: each is a cold build of 13-31 minutes on four cores. A second build waits for the first one's lock
  and then reuses its work.
* Select tests with `--workspace -E 'package(=X)'`, not `-p X`: `-p` resolves other features and rebuilds the shared
  crates.
* To keep binaries of a fixed commit while the tree moves on, archive them (`cargo nextest archive`) or copy the built
  binary out of `target/release/`; do not keep a tree copy with its own target directory.
