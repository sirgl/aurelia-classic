#!/usr/bin/env bash
# One-time (and idempotent) setup of the night-4 kit: the Rust toolchain, a Python 3.12 venv with the pinned MLX, the release gate binary
# `ai-exam` built from THIS commit. No global installs: everything lands in the repo (.venv-m5, target).
#   tools/m5/setup4.sh [--no-build]
# Overrides: M5_VENV (default <repo>/.venv-m5), M5_TOOLCHAIN (default 1.98.1), M5_PYTHON (a python3.12 binary), M5_PLATFORM_ANY=1 (skip the platform commit check).
# What is NOT in git and is made by the campaign (not here): the teacher recordings of the anchor and the lengths (jobs `teach`, `lengths`).
set -eu
REPO=$(cd "$(dirname "$0")/../.." && pwd)
VENV=${M5_VENV:-$REPO/.venv-m5}
TOOLCHAIN=${M5_TOOLCHAIN:-1.98.1}
MLX_VERSION=0.32.4
say() { printf '== %s\n' "$*"; }
fail() { printf 'setup4: %s\n' "$*" >&2; exit 1; }

say "machine"
[ "$(uname -s)" = Darwin ] && [ "$(uname -m)" = arm64 ] || fail "needs an Apple silicon Mac (MLX)"
sysctl -n machdep.cpu.brand_string
echo "cores: $(sysctl -n hw.physicalcpu) (performance $(sysctl -n hw.perflevel0.physicalcpu 2>/dev/null || echo ?)), RAM: $(( $(sysctl -n hw.memsize) / 1073741824 )) GB, free disk: $(df -g "$REPO" | awk 'NR==2{print $4}') GB"
free=$(df -g "$REPO" | awk 'NR==2{print $4}')
[ "$free" -ge "${M5_MIN_FREE_GB:-45}" ] || fail "needs at least ${M5_MIN_FREE_GB:-45} GB free disk (the anchor recording is 10 GB, the broker keeps 25 GB free); have $free GB"

say "Xcode command line tools and git"
xcode-select -p >/dev/null 2>&1 || fail "Xcode CLT missing: run 'xcode-select --install', wait for it to finish, run setup again"
command -v git >/dev/null || fail "git missing"

say "platform clone beside this one"
[ -d "$REPO/../platform/crates" ] || fail "the platform clone must lie beside this one: $REPO/../platform"
want=$(cat "$REPO/tools/m5/platform.commit")
have=$(git -C "$REPO/../platform" rev-parse --short HEAD)
echo "platform at $have, the kit was made with $want"
if [ "$have" != "$want" ] && [ -z "${M5_PLATFORM_ANY:-}" ]; then
  git -C "$REPO/../platform" cat-file -e "$want^{commit}" 2>/dev/null || fail "the platform clone does not have commit $want (git -C ../platform fetch origin; or M5_PLATFORM_ANY=1 to go on anyway)"
  git -C "$REPO/../platform" checkout -q "$want" || fail "cannot check out platform $want"
fi

say "rust $TOOLCHAIN"
export PATH="$HOME/.cargo/bin:$PATH"
if ! command -v rustup >/dev/null 2>&1; then
  fail "rustup missing: install it from https://rustup.rs (curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal), then run setup again"
fi
rustup toolchain list | grep -q "^$TOOLCHAIN" || rustup toolchain install "$TOOLCHAIN" --profile minimal
RUSTUP_TOOLCHAIN=$TOOLCHAIN rustc --version

say "python venv ($VENV)"
PYBIN=${M5_PYTHON:-}
if [ -z "$PYBIN" ]; then
  for c in python3.12 /opt/homebrew/bin/python3.12 /usr/local/bin/python3.12; do
    command -v "$c" >/dev/null 2>&1 && { PYBIN=$(command -v "$c"); break; }
  done
fi
if [ ! -x "$VENV/bin/python" ]; then
  [ -n "$PYBIN" ] || fail "python3.12 missing: 'brew install python@3.12' (Homebrew from https://brew.sh), then run setup again"
  case $("$PYBIN" -c 'import sys;print(sys.version_info[:2])') in "(3, 12)") ;; *) fail "$PYBIN is not Python 3.12";; esac
  "$PYBIN" -m venv "$VENV"
fi
have=$("$VENV/bin/pip" freeze 2>/dev/null | grep -E '^mlx(-metal)?==' | tr '\n' ' ' || true)
case "$have" in
  *"mlx==$MLX_VERSION"*"mlx-metal==$MLX_VERSION"*) echo "mlx $MLX_VERSION already installed";;
  *) "$VENV/bin/pip" install --quiet --upgrade pip && "$VENV/bin/pip" install --quiet "mlx==$MLX_VERSION" "mlx-metal==$MLX_VERSION";;
esac
"$VENV/bin/python" - <<'PY'
import mlx.core as mx
print("mlx device", mx.default_device(), "metal", mx.metal.is_available())
assert mx.metal.is_available(), "Metal is not available to MLX"
PY
mkdir -p "$REPO/lab" && "$VENV/bin/pip" freeze > "$REPO/lab/venv-freeze.txt" 2>/dev/null || true

if [ "${1:-}" != "--no-build" ]; then
  say "release gate binary (cargo, through the broker)"
  . "$REPO/tools/m5/env.sh"
  m5_res_up
  cd "$REPO"
  RUSTUP_TOOLCHAIN=$TOOLCHAIN "$M5_RES" run --cpu 2-8 --name c4-setup-build -- cargo build --release -p aurelia-qa --bin ai-exam
  ls -la "${CARGO_TARGET_DIR:-$REPO/target}/release/ai-exam"
fi
say "done"
