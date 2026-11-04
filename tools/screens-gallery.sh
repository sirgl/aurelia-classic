#!/bin/sh
# A gallery of every screen of the Classic game, drawn by the real game loop with no window
# (`aurelia-classic --screenshot`, classic/crates/classic-ui/src/dev/headless.rs), plus an index.html.
#   sh classic/tools/screens-gallery.sh [OUT_DIR]        (default OUT_DIR: out/screens)
# Env: BIN (default target/release/aurelia-classic), SIZE (default 1600x900), LATE_TURNS (default 200: how long
# the AI plays the late-game states), ONLY (a name prefix: run only the matching entries), GPU_LOCK (default
# unset: no lock; set it to a lock file to render one at a time).
# Needs the art pack (platform/tools/fetch-assets.sh); without it the pictures are placeholders.
# The early states are the opening of a new game; the late states let the AI play LATE_TURNS turns first.
set -eu
OUT="${1:-out/screens}"
BIN="${BIN:-target/release/aurelia-classic}"
SIZE="${SIZE:-1600x900}"
LATE_TURNS="${LATE_TURNS:-200}"
ONLY="${ONLY:-}"
[ -x "$BIN" ] || { echo "no binary at $BIN (cargo build --release -p classic-desktop --bin aurelia-classic)" >&2; exit 1; }
LOCK="${GPU_LOCK:-}"
mkdir -p "$OUT"
LIST="$OUT/.list"; : > "$LIST"
LATE="AURELIA_GAME_AUTOPLAY=1 AURELIA_GAME_TURNS=$LATE_TURNS"
# name | group | title | environment
cat > "$OUT/.table" <<TABLE
01-menu|Start|Main menu|AURELIA_GAME_SCREEN=menu
02-match-setup|Start|New game setup|AURELIA_GAME_SCREEN=setup
03-nations|Start|Nation choice|AURELIA_GAME_SCREEN=nations
04-map-opening|Map|Opening of a game|AURELIA_GAME_SCREEN=map
05-map-late|Map|Late game, $LATE_TURNS turns|$LATE AURELIA_GAME_SCREEN=map
06-city-opening|City|City screen, opening|AURELIA_GAME_SCREEN=city
07-city-buy|City|City: buy with gold|AURELIA_GAME_SCREEN=city AURELIA_GAME_CITY_BUY=1
08-city-rename|City|City: rename|AURELIA_GAME_SCREEN=city AURELIA_GAME_CITY_RENAME=1
09-city-late|City|City screen, late game|$LATE AURELIA_GAME_SCREEN=city
10-technologies-opening|Screens|Technology tree, opening|AURELIA_GAME_SCREEN=research
11-technologies-late|Screens|Technology tree, late game|$LATE AURELIA_GAME_SCREEN=research
12-policies-opening|Screens|Policies, opening|AURELIA_GAME_SCREEN=civics
13-policies-late|Screens|Policies, late game|$LATE AURELIA_GAME_SCREEN=civics
14-religion|Screens|Religion|$LATE AURELIA_GAME_SCREEN=religion
15-culture|Screens|Culture and tourism|$LATE AURELIA_GAME_SCREEN=culture
16-diplomacy|Screens|Diplomacy|$LATE AURELIA_GAME_SCREEN=diplomacy
17-city-states|Screens|City-states|$LATE AURELIA_GAME_SCREEN=city_states
18-congress|Screens|World Congress|$LATE AURELIA_GAME_SCREEN=congress
19-espionage|Screens|Espionage|$LATE AURELIA_GAME_SCREEN=espionage
20-victory|Screens|Victory progress|$LATE AURELIA_GAME_SCREEN=victory
21-results|Screens|End of game results|$LATE AURELIA_GAME_SCREEN=results
22-overview-cities|Overview|Overview: cities|$LATE AURELIA_GAME_SCREEN=overview AURELIA_OVERVIEW=cities
23-overview-resources|Overview|Overview: resources|$LATE AURELIA_GAME_SCREEN=overview AURELIA_OVERVIEW=resources
24-overview-trade|Overview|Overview: trade|$LATE AURELIA_GAME_SCREEN=overview AURELIA_OVERVIEW=trade
25-overview-units|Overview|Overview: units|$LATE AURELIA_GAME_SCREEN=overview AURELIA_OVERVIEW=units
26-overview-demographics|Overview|Overview: demographics|$LATE AURELIA_GAME_SCREEN=overview AURELIA_OVERVIEW=demographics
27-overview-diplomacy|Overview|Overview: diplomacy|$LATE AURELIA_GAME_SCREEN=overview AURELIA_OVERVIEW=diplomacy
28-overview-victory|Overview|Overview: victory|$LATE AURELIA_GAME_SCREEN=overview AURELIA_OVERVIEW=victory
29-overview-notifications|Overview|Overview: notifications|$LATE AURELIA_GAME_SCREEN=overview AURELIA_OVERVIEW=notifications
30-encyclopedia|Encyclopedia|Encyclopedia|AURELIA_GAME_SCREEN=pedia
31-settings-audio|Settings|Settings: audio|AURELIA_GAME_SCREEN=settings AURELIA_GAME_SETTINGS_TAB=audio
32-settings-graphics|Settings|Settings: graphics|AURELIA_GAME_SCREEN=settings AURELIA_GAME_SETTINGS_TAB=graphics
33-settings-game|Settings|Settings: game|AURELIA_GAME_SCREEN=settings AURELIA_GAME_SETTINGS_TAB=game
34-settings-controls|Settings|Settings: controls|AURELIA_GAME_SCREEN=settings AURELIA_GAME_SETTINGS_TAB=controls
35-shortcuts|Settings|Keyboard shortcuts|AURELIA_GAME_SHORTCUTS=1
36-trade|Map|Trade routes at work|AURELIA_GAME_TRADE=1 AURELIA_GAME_SCREEN=map
37-parade|Map|One unit of every kind|AURELIA_GAME_PARADE=1 AURELIA_GAME_SCREEN=map
TABLE
n=0; failed=0
while IFS='|' read -r name group title envs; do
  case "$name" in "$ONLY"*) ;; *) continue ;; esac
  printf '%s|%s|%s\n' "$name" "$group" "$title" >> "$LIST"
  echo "[$name] $title"
  run="env AURELIA_VISUAL_SIZE=$SIZE $envs $BIN --screenshot $OUT/$name.png"
  if [ -n "$LOCK" ]; then
    flock "$LOCK" sh -c "$run" > "$OUT/$name.log" 2>&1 || { echo "  failed: see $OUT/$name.log" >&2; failed=$((failed+1)); }
  else
    sh -c "$run" > "$OUT/$name.log" 2>&1 || { echo "  failed: see $OUT/$name.log" >&2; failed=$((failed+1)); }
  fi
  n=$((n+1))
done < "$OUT/.table"
# World art showcase (late-game units, buildings and wonders on the real terrain) and the animation strips.
if [ -z "$ONLY" ] || [ "$ONLY" = world ]; then
  for pair in "world|AURELIA_GAME_SHOWCASE=world"; do
    name="${pair%%|*}"; envs="${pair#*|}"
    printf '%s|World|World art showcase (several views)\n' "$name" >> "$LIST"
    echo "[$name] world showcase"
    run="env AURELIA_VISUAL_SIZE=$SIZE $envs $BIN --screenshot $OUT/$name.png"
    if [ -n "$LOCK" ]; then flock "$LOCK" sh -c "$run" > "$OUT/$name.log" 2>&1 || failed=$((failed+1)); else sh -c "$run" > "$OUT/$name.log" 2>&1 || failed=$((failed+1)); fi
  done
  for seq in move attack ranged missile nuke; do
    name="sequence-$seq"
    printf '%s|Animations|Action: %s (frames)\n' "$name" "$seq" >> "$LIST"
    echo "[$name]"
    run="env AURELIA_VISUAL_SIZE=$SIZE AURELIA_GAME_SEQUENCE=$seq AURELIA_GAME_SEQUENCE_FRAMES=8 AURELIA_GAME_SEQUENCE_STEP=0.15 $BIN --screenshot $OUT/$name.png"
    if [ -n "$LOCK" ]; then flock "$LOCK" sh -c "$run" > "$OUT/$name.log" 2>&1 || failed=$((failed+1)); else sh -c "$run" > "$OUT/$name.log" 2>&1 || failed=$((failed+1)); fi
  done
fi
python3 -I - "$OUT" <<'PY'
import os, sys, html, glob
out = sys.argv[1]
rows = [l.rstrip("\n").split("|") for l in open(os.path.join(out, ".list")) if l.strip()]
groups = {}
for name, group, title in rows:
    files = sorted(glob.glob(os.path.join(out, name + ".png")) + glob.glob(os.path.join(out, name + "-*.png")))
    groups.setdefault(group, []).append((name, title, [os.path.basename(f) for f in files]))
parts = ["<!doctype html><meta charset=utf-8><title>Aurelia: screens</title>",
         "<style>body{font:16px system-ui,sans-serif;margin:24px;background:#faf8f3;color:#222}h1{font-size:22px}h2{font-size:18px;margin-top:28px}"
         ".grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(420px,1fr));gap:16px}"
         "figure{margin:0;background:#fff;border:1px solid #ddd;border-radius:6px;padding:8px}img{width:100%;height:auto;display:block}"
         "figcaption{font-size:13px;color:#555;margin-top:6px}.strip img{display:inline-block;width:23%;margin:1%}</style>",
         "<h1>Aurelia (Classic): screens of the game</h1>"]
for g, items in groups.items():
    parts.append(f"<h2>{html.escape(g)}</h2><div class=grid>")
    for name, title, files in items:
        if not files:
            parts.append(f"<figure><figcaption>{html.escape(title)}: not produced (see {name}.log)</figcaption></figure>")
        elif len(files) == 1:
            parts.append(f"<figure><a href='{files[0]}'><img src='{files[0]}' loading=lazy></a><figcaption>{html.escape(title)}</figcaption></figure>")
        else:
            imgs = "".join(f"<a href='{f}'><img src='{f}' loading=lazy></a>" for f in files[:8])
            parts.append(f"<figure class=strip style='grid-column:1/-1'><div>{imgs}</div><figcaption>{html.escape(title)}: {len(files)} frames</figcaption></figure>")
    parts.append("</div>")
open(os.path.join(out, "index.html"), "w").write("\n".join(parts))
print("index:", os.path.join(out, "index.html"))
PY
echo "gallery: $n screens, $failed failed; open $OUT/index.html"
