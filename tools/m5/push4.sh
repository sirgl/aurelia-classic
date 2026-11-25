#!/usr/bin/env bash
# The one push of night 4 (docs/m5/PROMPT-4.md, step 5): the results as the NEW branch exp/m5-rl-honest-result.
#   tools/m5/push4.sh --dry-run     everything but the commit and the push (the checks, the file list)
#   tools/m5/push4.sh               commit on exp/m5-rl-honest-result and push it to the remote this clone came from (origin)
# Only small text files go in (docs/m5/night4/); the checks must print nothing, else it stops.
set -eu
. "$(dirname "$0")/env.sh"
cd "$M5_REPO"
DRY=${1:-}
BR=exp/m5-rl-honest-result
C4=$AURELIA_LAB/runs/campaign4
OUT=docs/m5/night4
[ -f "$C4/summary.md" ] || { echo "push4: no $C4/summary.md (run tools/m5/campaign4.sh summary)"; exit 1; }
rm -rf "$OUT"; mkdir -p "$OUT"
for f in results.jsonl summary.md summary.json status.json versions.json threads.json deadline.json; do [ -f "$C4/$f" ] && cp "$C4/$f" "$OUT/"; done
[ -f "$AURELIA_LAB/runs/sanity/sanity.json" ] && cp "$AURELIA_LAB/runs/sanity/sanity.json" "$OUT/"
for r in "$AURELIA_LAB"/runs/rl-*/; do   # the log of a run (the curve, the lambda, the parity, the seconds), gzipped; the validation rows' rates are in it
  [ -f "$r/log.jsonl" ] || continue
  n=$(basename "$r"); mkdir -p "$OUT/$n"; gzip -9 -c "$r/log.jsonl" > "$OUT/$n/log.jsonl.gz"
done
for t in "$AURELIA_LAB"/runs/tables/*/; do   # per checkpoint: the plays in a compact form and the gate reports
  [ -f "$t/done" ] || continue
  n=$(basename "$t"); mkdir -p "$OUT/tables/$n"
  "$M5_PY" -I - "$t" "$OUT/tables/$n/plays.tsv.gz" <<'PY'
import gzip, json, sys
t, out = sys.argv[1:3]
with gzip.open(out, "wt") as fo:
    fo.write("slice\tfamily\tera\tinstance\tattempt\twin\trounds\n")
    for s in ("full", "quick"):
        for line in open(f"{t}/{s}/rows.jsonl"):
            r = json.loads(line)
            fo.write(f"{s}\t{r['family']}\t{r.get('era')}\t{r.get('instance')}\t{r.get('attempt')}\t{int(bool(r['win']))}\t{r.get('rounds')}\n")
PY
  [ -f "$t/gate.json" ] && cp "$t/gate.json" "$OUT/tables/$n/"
  for g in "$t"/gate-*/; do [ -f "$g/report.md" ] && { mkdir -p "$OUT/tables/$n/$(basename "$g")"; cp "$g/report.md" "$OUT/tables/$n/$(basename "$g")/"; }; done
done
[ -f docs/m5/NIGHT-RESULTS-4.md ] || cp "$C4/summary.md" docs/m5/NIGHT-RESULTS-4.md
# absolute paths of this machine -> relative (the lab is <repo>/lab)
LC_ALL=C find "$OUT" docs/m5/NIGHT-RESULTS-4.md -type f \( -name '*.json' -o -name '*.jsonl' -o -name '*.md' \) -print0 | \
  xargs -0 sed -i '' -e "s#$AURELIA_LAB#lab#g" -e "s#$M5_REPO#.#g" -e "s#$HOME#~#g"
# the checks: sizes, secrets / absolute home paths, names that do not belong in the public repository
big=$(find "$OUT" -type f -size +3000k)
[ -z "$big" ] || { echo "push4: files too big: $big"; exit 1; }
bad=$(grep -rn -E '/Use''rs/|/ho''me/|BEGIN [A-Z ]*PRIVATE'' KEY|gh''p_|github''_pat_|sk-[A-Za-z0-9]{20}|AK''IA[0-9A-Z]{12}' "$OUT" docs/m5/NIGHT-RESULTS-4.md | head -5 || true)
[ -z "$bad" ] || { echo "push4: secrets or absolute paths:"; echo "$bad"; exit 1; }
names=$(grep -rn -i -E 'vo''x|po''puli|work/ne''ts|rom''an' "$OUT" docs/m5/NIGHT-RESULTS-4.md | head -5 || true)
[ -z "$names" ] || { echo "push4: a name that must not be public:"; echo "$names"; exit 1; }
du -sh "$OUT"; find "$OUT" -type f | head -80
if [ "$DRY" = --dry-run ]; then echo "push4: dry run, the checks passed; nothing committed or pushed"; exit 0; fi
URL=${M5_PUSH_URL:-$(git remote get-url origin)}
cur=$(git rev-parse --abbrev-ref HEAD)
[ "$cur" = "$BR" ] || git switch -c "$BR"
git add -f "$OUT" docs/m5/NIGHT-RESULTS-4.md
git diff --cached --stat | tail -3
git commit -q -m "m5: night 4 results (long RL on the honest input, base and anchor-boost, seeds 1-3)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
git push "$URL" "$BR" || { echo "push4: the push was refused"; tar czf "$AURELIA_LAB/m5-night4-results.tgz" "$OUT" docs/m5/NIGHT-RESULTS-4.md; echo "packed: $AURELIA_LAB/m5-night4-results.tgz"; exit 2; }
git log --oneline -1
