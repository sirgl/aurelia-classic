#!/bin/sh
set -eu
cd "$(dirname "$0")"
cargo run --release --offline -p classic-desktop --bin visual-sandbox -- "$@"
