#!/usr/bin/env bash
# One-time (and idempotent) setup of the M5 kit: Xcode CLT, rustup + the pinned toolchain, a Python 3.12 venv with
# pinned MLX, the release binaries. No global installs: everything lands in the repo (.venv-m5, target).
#   tools/m5/setup.sh [--no-build]
# Overrides: M5_VENV (default <repo>/.venv-m5), M5_TOOLCHAIN (default 1.98.1), M5_PYTHON (a python3.12 binary).
set -eu
REPO=$(cd "$(dirname "$0")/../.." && pwd)
VENV=${M5_VENV:-$REPO/.venv-m5}
TOOLCHAIN=${M5_TOOLCHAIN:-1.98.1}
MLX_VERSION=0.32.4        # the version of the M4 lab's venv (mlx + mlx-metal, nothing else)
say() { printf '== %s\n' "$*"; }
fail() { printf 'setup: %s\n' "$*" >&2; exit 1; }

say "machine"
[ "$(uname -s)" = Darwin ] && [ "$(uname -m)" = arm64 ] || fail "needs an Apple silicon Mac (MLX)"
sysctl -n machdep.cpu.brand_string
echo "cores: $(sysctl -n hw.physicalcpu) (performance $(sysctl -n hw.perflevel0.physicalcpu 2>/dev/null || echo ?)), RAM: $(( $(sysctl -n hw.memsize) / 1073741824 )) GB, free disk: $(df -g "$REPO" | awk 'NR==2{print $4}') GB"

say "Xcode command line tools"
xcode-select -p >/dev/null 2>&1 || fail "Xcode CLT missing: run 'xcode-select --install', wait for it to finish, run setup again"
command -v git >/dev/null || fail "git missing"
[ -d "$REPO/../platform/crates" ] || fail "the platform clone must lie beside this one: $REPO/../platform (git clone -b exp/m5-kit <platform url> platform)"

say "rust $TOOLCHAIN"
if ! command -v rustup >/dev/null 2>&1 && [ ! -x "$HOME/.cargo/bin/rustup" ]; then
  fail "rustup missing: install it from https://rustup.rs (curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal), then run setup again"
fi
export PATH="$HOME/.cargo/bin:$PATH"
rustup toolchain list | grep -q "^$TOOLCHAIN" || rustup toolchain install "$TOOLCHAIN" --profile minimal
RUSTUP_TOOLCHAIN=$TOOLCHAIN rustc --version

say "python venv ($VENV)"
PYBIN=${M5_PYTHON:-}
if [ -z "$PYBIN" ]; then
  for c in python3.12 /opt/homebrew/bin/python3.12 /usr/local/bin/python3.12; do
    command -v "$c" >/dev/null 2>&1 && { PYBIN=$(command -v "$c"); break; }
  done
fi
[ -n "$PYBIN" ] || fail "python3.12 missing: 'brew install python@3.12' (Homebrew from https://brew.sh), then run setup again"
case $("$PYBIN" -c 'import sys;print(sys.version_info[:2])') in "(3, 12)") ;; *) fail "$PYBIN is not Python 3.12";; esac
if [ -x "$VENV/bin/python" ] && "$VENV/bin/python" -c "import mlx.core" 2>/dev/null; then
  have=$("$VENV/bin/pip" freeze | grep -E '^mlx(-metal)?==' | tr '\n' ' ')
else
  have=""
fi
[ -x "$VENV/bin/python" ] || "$PYBIN" -m venv "$VENV"
case "$have" in
  *"mlx==$MLX_VERSION"*"mlx-metal==$MLX_VERSION"*) echo "mlx $MLX_VERSION already installed";;
  *) "$VENV/bin/pip" install --quiet --upgrade pip && "$VENV/bin/pip" install --quiet "mlx==$MLX_VERSION" "mlx-metal==$MLX_VERSION";;
esac
"$VENV/bin/python" - <<'PY'
import mlx.core as mx
print("mlx", mx.__version__ if hasattr(mx, "__version__") else "?", "device", mx.default_device(), "metal", mx.metal.is_available())
assert mx.metal.is_available(), "Metal is not available to MLX"
PY
"$VENV/bin/pip" freeze > "$REPO/lab-venv-freeze.txt" 2>/dev/null && mkdir -p "$REPO/lab" && mv "$REPO/lab-venv-freeze.txt" "$REPO/lab/venv-freeze.txt"

say "broker"
"$REPO/tools/res/res" info | head -12

if [ "${1:-}" != "--no-build" ]; then
  say "release binaries (cargo, through the broker)"
  "$REPO/tools/res/res" start >/dev/null
  cd "$REPO"
  RUSTUP_TOOLCHAIN=$TOOLCHAIN "$REPO/tools/res/res" run --cpu 2-8 --name m5-setup -- \
    cargo build --release -p aurelia-qa --bin ai-exam --bin ai-tournament --bin ai-record
  ls -la "$REPO/target/release/ai-exam"
fi
say "done"
