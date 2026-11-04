#!/usr/bin/env bash
# Runs the tests a change can break, and nothing else (docs/testing.md).
#
#   tools/test-changed.sh                  # working tree (and untracked files) against HEAD
#   tools/test-changed.sh origin/main      # working tree against a ref (classic)
#   tools/test-changed.sh --platform-base origin/main ...   # the same for ../platform
#   tools/test-changed.sh --dry-run        # print what would run
#   tools/test-changed.sh -- --no-capture  # anything after -- goes to cargo nextest
#
# What runs:
#   * every changed file is mapped to its crate (the nearest Cargo.toml in either workspace);
#   * the tests of that crate and of every crate that depends on it (dev-dependencies too),
#     in classic and in platform: a change in platform's kernel runs everything above the kernel;
#   * the `pins` group of classic/.config/nextest.toml (the cheap pinned games), always;
#   * only the fast tests: the nextest `default` profile leaves out the slow and GPU groups.
#     `--profile full` takes them in.
# Files outside the crates: assets/rules -> classic-rules and up, assets/{lang,visual,models} ->
# classic-ui and up, docs/classic -> classic-content; other docs, tools/, nn/, saves/ and *.md
# run only the pins; anything else (Cargo.lock, a root Cargo.toml, .cargo/, .config/) runs all.
#
# Environment: JOBS (build jobs and test threads, default 4), CARGO_PROFILE (default fasttest),
# TOOLCHAIN (default +1.98.1), CARGO_TARGET_DIR (shared, see docs/testing.md).
set -euo pipefail

classic=$(cd "$(dirname "$0")/.." && pwd)
platform=$(cd "$classic/../platform" && pwd)
base=HEAD
platform_base=HEAD
nextest_profile=default
dry_run=0
extra=()
while [ $# -gt 0 ]; do
    case "$1" in
        --platform-base) platform_base=$2; shift 2 ;;
        --profile) nextest_profile=$2; shift 2 ;;
        --dry-run) dry_run=1; shift ;;
        --) shift; extra=("$@"); break ;;
        -h|--help) sed -n '2,24p' "$0"; exit 0 ;;
        *) base=$1; shift ;;
    esac
done
jobs=${JOBS:-4}
profile=${CARGO_PROFILE:-fasttest}
toolchain=${TOOLCHAIN:-+1.98.1}

changed() { # repo base -> changed and untracked paths, relative to the repo
    git -C "$1" diff --name-only "$2" --
    git -C "$1" ls-files --others --exclude-standard
}

plan=$(
    {
        echo "@classic $classic"
        changed "$classic" "$base"
        echo "@platform $platform"
        changed "$platform" "$platform_base"
    } | python3 -I -c '
import json, subprocess, sys

roots, files, current = {}, {"classic": [], "platform": []}, None
for line in sys.stdin.read().splitlines():
    if line.startswith("@"):
        current, path = line[1:].split(" ", 1)
        roots[current] = path
    elif line:
        files[current].append(line)

import os
toolchain = os.environ.get("TOOLCHAIN", "+1.98.1")

def members(root):
    """The workspace packages of a checkout (no resolution, no network)."""
    out = subprocess.run(["cargo", toolchain, "metadata", "--format-version", "1", "--no-deps", "--offline"],
                         cwd=root, capture_output=True, text=True, check=True).stdout
    return json.loads(out)["packages"]

def directory(manifest_path):
    return manifest_path.rsplit("/", 1)[0] + "/"

packages = {repo: members(root) for repo, root in roots.items()}
dirs = {directory(p["manifest_path"]): p["name"] for ps in packages.values() for p in ps}
# Who uses whom among the local crates of both checkouts (path dependencies, dev ones too).
users = {}
for ps in packages.values():
    for p in ps:
        for dep in p["dependencies"]:
            if dep.get("path") and dep["name"] != p["name"]:
                users.setdefault(dep["name"], set()).add(p["name"])

def owner(repo, rel):
    path = roots[repo] + "/" + rel
    best = max((d for d in dirs if path.startswith(d)), key=len, default=None)
    return dirs[best] if best else None

seeds, everything, reasons = set(), set(), []
for repo, paths in files.items():
    for rel in paths:
        pkg = owner(repo, rel)
        if pkg:
            seeds.add(pkg)
            continue
        top = rel.split("/", 1)[0]
        if repo == "classic" and rel.startswith("assets/rules/"):
            seeds.add("classic-rules")
        elif repo == "classic" and top == "assets":
            seeds.add("classic-ui")
        elif repo == "classic" and rel.startswith("docs/classic/"):
            seeds.add("classic-content")
        elif top in ("docs", "tools", "nn", "saves", ".github", ".github-template") or rel.endswith(".md"):
            pass
        else:
            # Cargo.lock, a root Cargo.toml, .cargo/, .config/, assets of platform, ...: all of it.
            everything.update({"classic", "platform"} if repo == "platform" else {"classic"})
            reasons.append(f"{repo}/{rel}")

hit, todo = set(), list(seeds)
while todo:
    name = todo.pop()
    if name not in hit:
        hit.add(name)
        todo.extend(users.get(name, ()))

for repo in ("classic", "platform"):
    names = {p["name"] for p in packages[repo]}
    chosen = names if repo in everything else hit & names
    print(f"{repo}\t{int(repo in everything)}\t{chr(32).join(sorted(chosen))}")
print("why\t" + " ".join(reasons[:5]))
'
)

classic_line=$(printf '%s\n' "$plan" | grep '^classic' || true)
platform_line=$(printf '%s\n' "$plan" | grep '^platform' || true)
why=$(printf '%s\n' "$plan" | grep '^why' | cut -f2)
expr_of() { # "pkg pkg" -> nextest filterset
    local e="" p
    for p in $1; do e="${e:+$e | }package(=$p)"; done
    printf '%s' "$e"
}

run() { # dir filterset label
    local cmd=(cargo "$toolchain" nextest run --cargo-profile "$profile" --profile "$nextest_profile"
               --build-jobs "$jobs" --test-threads "$jobs" --workspace -E "$2" ${extra[@]+"${extra[@]}"})
    echo "== $3: ${cmd[*]}"
    [ "$dry_run" = 1 ] || (cd "$1" && "${cmd[@]}")
}

[ -n "$why" ] && echo "everything: $why"
classic_pkgs=$(printf '%s' "$classic_line" | cut -f3)
platform_pkgs=$(printf '%s' "$platform_line" | cut -f3)
echo "classic crates:  ${classic_pkgs:-none} (+ the pins)"
echo "platform crates: ${platform_pkgs:-none}"

classic_expr=$(expr_of "$classic_pkgs")
run "$classic" "${classic_expr:+$classic_expr | }group(pins)" classic
if [ -n "$platform_pkgs" ]; then
    run "$platform" "$(expr_of "$platform_pkgs")" platform
fi
