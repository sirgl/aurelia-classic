# Sourced by every tools/m5 script of the night-4 kit: paths relative to the repo, all overridable by environment.
M5_REPO=$(cd "$(dirname "${BASH_SOURCE:-$0}")/../.." && pwd)
export M5_REPO
export AURELIA_LAB=${AURELIA_LAB:-$M5_REPO/lab}          # recordings, runs, logs, data (not in git)
# The broker: the vendored one (tools/res) unless M5_RES names the machine's own (then RES_HOME is that broker's business).
if [ -z "${M5_RES:-}" ]; then M5_RES=$M5_REPO/tools/res/res; export RES_HOME=${RES_HOME:-$M5_REPO/.res}; export RES_CONFIG=${RES_CONFIG:-$M5_REPO/tools/m5/res4.toml}; fi
export M5_RES
export M5_PY=${M5_PY:-$M5_REPO/.venv-m5/bin/python}      # the pinned Python 3.12 + MLX 0.32.4 venv (setup4.sh)
export AURELIA_EXAM_BIN=${AURELIA_EXAM_BIN:-$M5_REPO/target/release/ai-exam}
export MLX_ENABLE_TF32=${MLX_ENABLE_TF32:-0}              # plain fp32 matmuls, as on the M4
mkdir -p "$AURELIA_LAB/runs" "$AURELIA_LAB/logs" "$AURELIA_LAB/data"
m5_res_up() { "$M5_RES" start >/dev/null 2>&1 || true; }
