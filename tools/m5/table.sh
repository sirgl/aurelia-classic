#!/usr/bin/env bash
# The gate table: per family the best teacher and each net (win rate on the held-out scenes, "*" = at least 90 %
# of the best teacher).   tools/m5/table.sh OUT.json NAME=runs/EVALDIR/rows.jsonl ...   (paths relative to the repo, or absolute)
set -eu
. "$(dirname "$0")/env.sh"
cd "$M5_REPO"
exec "$M5_PY" "$M5_REPO/tools/m5/py/m_table.py" "$@"
