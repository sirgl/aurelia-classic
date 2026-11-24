#!/usr/bin/env bash
# Stage 2: a second Python venv with the NEWEST MLX (next to the pinned one) and a record of the versions.
#   tools/m5/setup2.sh      -> .venv-m5-new, lab/runs/campaign2/versions.json
# Idempotent. A failure is recorded (ok: false), not fatal: the speed step then skips the newest-MLX axis.
set -u
. "$(dirname "$0")/env.sh"
NEW=${M5_VENV_NEW:-$M5_REPO/.venv-m5-new}
OUTD=$M5_RUNS/campaign2; mkdir -p "$OUTD"
PYBIN=$(command -v python3.12 || ls /opt/homebrew/bin/python3.12 /usr/local/bin/python3.12 2>/dev/null | head -1 || true)
ok=false; note=""
if [ -n "$PYBIN" ]; then
  [ -x "$NEW/bin/python" ] || "$PYBIN" -m venv "$NEW"
  if "$NEW/bin/pip" install --quiet --upgrade pip && "$NEW/bin/pip" install --quiet --upgrade mlx; then
    "$NEW/bin/python" - <<'PY' && ok=true
import mlx.core as mx
assert mx.metal.is_available()
PY
    # the training code must at least import on it
    (cd "$M5_NN" && "$NEW/bin/python" -I -c "import sys; sys.path.insert(0, '.'); import aurelia_nn.army.train_m, aurelia_nn.army.train" 2>&1 | tail -3) || { ok=false; note="aurelia_nn does not import on the newest MLX"; }
  else note="pip install mlx failed"; fi
else note="python3.12 missing"; fi
"$M5_PY" -I - "$NEW" "$ok" "$note" "$OUTD/versions.json" <<'PY'
import json, subprocess, sys
new, ok, note, out = sys.argv[1:5]
def ver(py):
    code = "import importlib.metadata as m\nfor p in ('mlx', 'mlx-metal'):\n    try: print(p, m.version(p))\n    except Exception: pass"
    return subprocess.run([py, '-I', '-c', code], capture_output=True, text=True).stdout.strip().replace('\n', ', ')
sh = lambda *c: subprocess.run(c, capture_output=True, text=True).stdout.strip()
d = {'pinned_py': sys.executable, 'pinned_mlx': ver(sys.executable), 'new_py': new + '/bin/python', 'new_mlx': ver(new + '/bin/python'),
     'ok': ok == 'true', 'note': note, 'macos': sh('sw_vers', '-productVersion'), 'macos_build': sh('sw_vers', '-buildVersion'),
     'chip': sh('sysctl', '-n', 'machdep.cpu.brand_string')}
json.dump(d, open(out, 'w'), indent=1); print(json.dumps(d, indent=1))
PY
