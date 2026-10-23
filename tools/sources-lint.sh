#!/bin/sh
# Sources lint (classic/docs/classic/SOURCES.md): fails when a file mentions the original
# game's files, SDK, DLL, decompiled code or a third-party copy of them more
# often than classic/tools/sources-lint.baseline allows. Civ V-derived data must cite
# Unciv (or be our own work); the baseline lists the places that still name something
# else (they are the work of the re-sourcing plan) and can only shrink.
#
# A new file (or a new kind of hit in an old file) is not in the baseline, so
# its first forbidden reference fails. classic/docs/classic/SOURCES.md itself is allowed
# to name them: it has to say where the data came from.
#
# Usage: sh classic/tools/sources-lint.sh            check against the baseline
#        sh classic/tools/sources-lint.sh --print    print the current counts in baseline format
set -e
ROOT="${AURELIA_ROOT:-$(git -C "$(dirname "$0")" rev-parse --show-toplevel)}"
cd "$ROOT"
export LC_ALL=C

# Pattern groups (ERE). The letter is the key in the baseline.
#   A SDK class and file names
#   B game XML and Lua file names, the global constants table
#   C third-party copies of the game files
#   D the database copy on a wiki
#   E DLL, SDK, decompiled code, the vendor
#   F database dump wording
#   G table, column and define citations of the original in prose: its difficulty
#     table, world-size and game-speed tables, "Civ V `NAME`" labels, "[verify]"
#     markers that name them
#   I the old sources by name (re-sourcing, classic/docs/classic/RESOURCING.md): the GitHub copy of the
#     game's XML, the SDK copy, the wiki database dump, the product and patch strings of the
#     old `edition` field, the original's database-table labels of effects (`Table.Column`),
#     the `game_type` keys. None of them may come back; the baseline for I is empty by rule.
#   H the original's define names (classic/tools/sources-lint.names, one per line,
#     whole words): a constant is described in our own words, never labelled
#     with the name the original gave it
A='\bCv[A-Z][A-Za-z]{3,}'
B='CIV5[A-Za-z]+\.xml|Civ5[A-Z][A-Za-z]+\.xml|Gameplay/XML|Assets/(DLC|Gameplay)|Civ5Pkg|GlobalDefines|AssignStartingPlots'
C='Bob-s-Mod|Keith7287|Community-Patch-DLL|LoneGazebo|CvGameCoreDLL'
D='fandom-bnw-dump|Module:Data/Civ5|fandomdb'
E='\bDLL\b|\bSDK\b|[Dd]ecompil|[Dd]isassembl|Firaxis'
F='final-patch (DB|dump)|DB dump|dump of the (game|final)|game database|tables\.json'
I='Keith7287|Bob-s-Mod|Community-Patch-DLL|LoneGazebo|CvGameCoreDLL|fandom-bnw-dump|Module:Data/Civ5|civilization\.fandom|fandomdb|CIV5[A-Za-z]*\.xml|Civ5[A-Z][A-Za-z]*\.xml|Civilization V: Brave New World|patch 1\.0\.3|\"game_type\"|\"edition\": \"Civilization|(Building|Unit|Policy|Policies|Trait|Traits|Technologies|UnitPromotions|Improvement|Resource|Terrain|Belief|Process|Project|Specialist)s?_?[A-Za-z]*\.[A-Z][A-Za-z]+Change|Building_[A-Za-z]+\.[A-Z]|unverified-legacy'  # rename-classic:keep
G='HandicapInfos?|Handicap_|\bWorlds\.[A-Za-z]|GameSpeeds?\.[A-Z]|\bEras\.[A-Z]|\bResolutions\.[A-Z]|\bDefines\b|HappinessDefault|Civ V `[A-Z][A-Za-z_.]*`|BNW `[A-Z][A-Za-z_.]*`|Prince (handicap|value|row|column)|\(Prince\)|final patch|\[verify'

# What is scanned: documents, rules data, code and tools. Agent logs and
# histories (ru/notes, output, ru/benches/results) are not.
SCOPE="platform classic lands integration"
# What may name the sources: the inventory itself, this script and its
# baseline, and the coverage tool (which names Unciv only).
SKIP=':!classic/docs/classic/SOURCES.md :!classic/docs/classic/RESOURCING.md :!classic/tools/sources-lint.sh :!classic/tools/sources-lint.baseline :!classic/tools/unciv-coverage.py :!classic/tools/rename-classic.sh :!classic/docs/rename-classic.md'

BASELINE=classic/tools/sources-lint.baseline
NOW="$(mktemp)"
trap 'rm -f "$NOW"' EXIT

NAMES=classic/tools/sources-lint.names
SKIP="$SKIP :!classic/tools/sources-lint.names"

for id in A B C D E F G H I; do
  eval pattern=\$$id
  # shellcheck disable=SC2086
  if [ "$id" = H ]; then
    git grep -IcwFf "$NAMES" -- $SCOPE $SKIP
  else
    git grep -IcE "$pattern" -- $SCOPE $SKIP
  fi | awk -F: -v id="$id" '$NF > 0 { n = $NF; sub(/:[0-9]+$/, ""); printf "%s\t%s\t%d\n", $0, id, n }'
done | sort > "$NOW"

if [ "${1:-}" = "--print" ]; then
  cat "$NOW"
  exit 0
fi

# The only permitted outside source is Unciv: the name of the original game's in-game encyclopedia must
# not appear anywhere outside ru/ and notes/ (case-insensitive; the pattern is written so that this line
# does not match itself). The encyclopedia SCREEN of the UI is a feature and is called "Encyclopedia".
JPAT='civilop[e]dia|wiki(pedia)? (article|page)s? (of|for|on) the (game|original)'
JHITS="$(git grep -IilE "$JPAT" -- . ':!ru' ':!notes' ':!classic/tools/sources-lint.sh' || true)"
if [ -n "$JHITS" ]; then
  echo "sources-lint: FAILED. The in-game encyclopedia / a wiki may not be named as a source (Unciv and own work only):" >&2
  echo "$JHITS" >&2
  exit 1
fi

# Naming guard (classic/docs/rename-classic.md): the old names of the own identifiers (civ5, Civ5, CIV_*, core:civ/) must not come back. rename-classic:keep
sh classic/tools/rename-classic.sh --check || {
  echo "sources-lint: FAILED (naming guard above)" >&2
  exit 1
}

if [ ! -f "$BASELINE" ]; then
  echo "sources-lint: $BASELINE is missing" >&2
  exit 2
fi

# Rows over their ceiling or absent from the baseline; rows under it (a ceiling to lower).
awk -F'\t' '
  NR == FNR { if ($0 !~ /^#/ && NF == 3) limit[$1 "\t" $2] = $3; next }
  {
    k = $1 "\t" $2
    if (!(k in limit))      { printf "NEW   %s group %s: %d line(s), none allowed\n", $1, $2, $3; bad++ }
    else if ($3 > limit[k]) { printf "GREW  %s group %s: %d line(s), ceiling %d\n", $1, $2, $3, limit[k]; bad++ }
    else if ($3 < limit[k]) { printf "lower %s group %s: %d line(s), ceiling %d\n", $1, $2, $3, limit[k] > "/dev/stderr" }
    seen[k] = 1
  }
  END {
    for (k in limit) if (!(k in seen)) { split(k, p, "\t"); printf "lower %s group %s: 0 line(s), ceiling %d\n", p[1], p[2], limit[k] > "/dev/stderr" }
    exit bad > 0
  }
' "$BASELINE" "$NOW" && echo "sources-lint: ok ($(wc -l < "$NOW" | tr -d ' ') file/group rows within the baseline)" || {
  echo "sources-lint: FAILED. Cite Unciv instead (classic/docs/classic/SOURCES.md), or, for a place that must name the original, add it to SOURCES.md and the baseline with a reason." >&2
  exit 1
}
