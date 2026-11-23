# Sourced by every tools/m5 script: paths relative to the repo, all overridable by environment.
M5_REPO=$(cd "$(dirname "${BASH_SOURCE:-$0}")/../.." && pwd)
export M5_REPO
export AURELIA_LAB=${AURELIA_LAB:-$M5_REPO/lab}          # recordings, runs, logs, data (not in git)
export RES_HOME=${RES_HOME:-$M5_REPO/.res}
M5_RES=$M5_REPO/tools/res/res
M5_PY=${M5_PY:-$M5_REPO/.venv-m5/bin/python}            # the pinned Python 3.12 + MLX venv (setup.sh)
M5_EXAM=${AURELIA_EXAM_BIN:-$M5_REPO/target/release/ai-exam}
M5_NN=$M5_REPO/nn
M5_RUNS=$AURELIA_LAB/runs
M5_LOGS=$AURELIA_LAB/logs
export AURELIA_EXAM_BIN=$M5_EXAM
mkdir -p "$M5_RUNS" "$M5_LOGS" "$AURELIA_LAB/data"
[ -f "$AURELIA_LAB/data/kept-m.jsonl" ] || cp "$M5_REPO/tools/m5/data/kept-m.jsonl" "$AURELIA_LAB/data/kept-m.jsonl"
# The broker's daemon must run for the budget; start it when it is not.
m5_res_up() { "$M5_RES" start >/dev/null 2>&1 || true; }
# MLX memory limit (GB) for a training job: the broker's detected value unless M5_MLX_GB is set.
m5_mlx_gb() { echo "${M5_MLX_GB:-$("$M5_PY" -I -c "import json,subprocess,sys;print(json.loads(subprocess.run(['$M5_RES','info'],capture_output=True,text=True).stdout)['mlx_gb'])")}"; }
