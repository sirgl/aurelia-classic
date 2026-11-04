#!/bin/sh
# The game: the release binary (a debug build is a slideshow, not a game).
# Controls: LMB select/move, RMB attack, Enter end turn, B found a city,
# F fortify, G grid, Home to the selection, Esc menu (save/load).
# Without a window: --screenshot out.png (AURELIA_GAME_TURNS, AURELIA_GAME_SCREEN), --bench.
set -e
cd "$(dirname "$0")"
exec cargo run --release -p classic-desktop --bin aurelia-classic -- "$@"
