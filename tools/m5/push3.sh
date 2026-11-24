#!/usr/bin/env bash
# The one push of stage 3 (docs/m5/PROMPT-3.md, step 5): the results as the NEW branch exp/m5-night-result.
#   tools/m5/push3.sh --dry-run     everything but the commit and the push (the checks, the file list)
#   tools/m5/push3.sh               commit on exp/m5-night-result and push it to the public repo
# Only small text files go in (docs/m5/night/); the checks must print nothing, else it stops.
set -eu
. "$(dirname "$0")/env.sh"
cd "$M5_REPO"
DRY=${1:-}
BR=exp/m5-night-result
URL=${M5_PUSH_URL:-https://github.com/sirgl/aurelia-classic.git}
C3=$M5_RUNS/campaign3
OUT=docs/m5/night
[ -f "$C3/summary.md" ] || { echo "push3: no $C3/summary.md (run tools/m5/campaign3.sh summary)"; exit 1; }
rm -rf "$OUT"; mkdir -p "$OUT"
for f in results.jsonl summary.md status.json versions.json pick.json deadline.json; do [ -f "$C3/$f" ] && cp "$C3/$f" "$OUT/"; done
[ -f "$C3/needle/results.jsonl" ] && mkdir -p "$OUT/needle" && cp "$C3/needle/results.jsonl" "$C3/needle/summary.md" "$OUT/needle/" 2>/dev/null || true
for g in "$C3"/gate-*; do
  [ -d "$g" ] || continue
  mkdir -p "$OUT/$(basename "$g")"; cp "$g"/report.md "$g"/report.json "$OUT/$(basename "$g")/" 2>/dev/null || true
done
for r in "$M5_RUNS"/rl-s*/; do
  [ -f "$r/curve.json" ] || continue
  n=$(basename "$r"); mkdir -p "$OUT/$n"; cp "$r/curve.json" "$OUT/$n/"; [ -f "$r/selected.json" ] && cp "$r/selected.json" "$OUT/$n/"
done
[ -f docs/m5/NIGHT-RESULTS.md ] || cp "$C3/summary.md" docs/m5/NIGHT-RESULTS.md
# absolute paths of this machine -> relative (the lab is <repo>/lab)
LC_ALL=C find "$OUT" docs/m5/NIGHT-RESULTS.md -type f \( -name '*.json' -o -name '*.jsonl' -o -name '*.md' \) -print0 | \
  xargs -0 sed -i '' -e "s#$AURELIA_LAB#lab#g" -e "s#$M5_REPO#.#g" -e "s#$HOME#~#g"
# the checks: sizes, secrets / absolute home paths, another game's mod
big=$(find "$OUT" -type f -size +3000k)
[ -z "$big" ] || { echo "push3: files too big: $big"; exit 1; }
bad=$(grep -rn -E '/Use''rs/|/ho''me/|BEGIN [A-Z ]*PRIVATE'' KEY|gh''p_|github''_pat_|sk-[A-Za-z0-9]{20}|AK''IA[0-9A-Z]{12}' "$OUT" docs/m5/NIGHT-RESULTS.md | head -5 || true)
[ -z "$bad" ] || { echo "push3: secrets or absolute paths:"; echo "$bad"; exit 1; }
mod=$(grep -rn -i -E 'vo''x populi|community'' patch|\bV''P\b' "$OUT" docs/m5/NIGHT-RESULTS.md | head -5 || true)
[ -z "$mod" ] || { echo "push3: another game's mod named:"; echo "$mod"; exit 1; }
du -sh "$OUT"; find "$OUT" -type f | head -60
if [ "$DRY" = --dry-run ]; then echo "push3: dry run, the checks passed; nothing committed or pushed"; exit 0; fi
cur=$(git rev-parse --abbrev-ref HEAD)
[ "$cur" = "$BR" ] || git switch -c "$BR"
git add -f "$OUT" docs/m5/NIGHT-RESULTS.md
git diff --cached --stat | tail -3
git commit -q -m "m5: night results (needle test, architecture screening, seeds, RL seed variance)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
git push "$URL" "$BR" || { echo "push3: the push was refused"; tar czf "$AURELIA_LAB/m5-night-results.tgz" "$OUT" docs/m5/NIGHT-RESULTS.md; echo "packed: $AURELIA_LAB/m5-night-results.tgz"; exit 2; }
git log --oneline -1
