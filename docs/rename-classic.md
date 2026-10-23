# The rename out of the original game's name (`classic/tools/rename-classic.sh`)

Decision (the project owner, 2026-10-20): the project does not carry the original game's name in its own
identifiers. The public description stays "Unciv / Civ 5 inspired game with a similar rule set";
the product is **Aurelia**. This file is the mapping, the rules of the script and what to do on a
branch that was started before the rename.

## For a branch that started before the rename

Base of the rename: `main` at `bdc85183` (the owner of `refactor/rename-classic` merges `main` into it and
re-runs the script whenever `main` moves; the base hash is then updated here).

```sh
git fetch origin
git status --short                                    # must be clean (commit or stash first)
printf 'ru/notes/devlog/ideas.md merge=union\n' >> .git/info/attributes   # both sides append to the devlog: keep both
git merge bdc85183                                    # 1. bring your branch up to the main the rename was cut from
git checkout origin/refactor/rename-classic -- classic/tools/rename-classic.sh   # 2. the script itself, tracked on your branch
sh classic/tools/rename-classic.sh                            # 3. rewrites the tracked files and moves the renamed paths
sh classic/tools/rename-classic.sh --repin                    #    the ruleset fingerprint pin (the only pin that moves)
git add -A && git commit -m "Apply classic/tools/rename-classic.sh"
git merge origin/refactor/rename-classic              # 4. now only your own edits can conflict
```

Why this order: the script renames your files and rewrites your lines the same way it did on the
integrated tree, so in step 4 both sides carry the same text and only the lines you edited next to a
renamed line can conflict. Step 1 matters for a branch that is behind `main`: a rename of old text meets
`main`'s newer text on the same lines (a branch 97 commits behind conflicted in about 70 files without it, in none with it).
Conflicts that remain are adjacent edits, not disagreements:

- Text conflicts in files you changed: your side is the right one (it is your edit, already renamed).
  `git merge -X ours origin/refactor/rename-classic` takes it for every hunk at once. Measured on `i18n/localization`
  (51 commits, 692 moved paths): 30 conflicting files with a plain merge, 3 with `-X ours`.
- "renamed to classic-... in origin/refactor/rename-classic, but deleted in HEAD" (with an add/add on the same path):
  git did not recognise your rewritten file as the renamed one (too little of the old text is left). Keep
  YOUR file: `git checkout --ours <the classic-... path> && git add <the same path>` (never `git rm` it: your file lives there).
- `Cargo.lock`: take your side (`git checkout --ours Cargo.lock`), then `cargo metadata --offline` normalises it.
- The ruleset fingerprint (`CLASSIC_FINGERPRINT` in `classic/crates/classic-rules/src/classic/mod.rs`, the value in
  `tests/rules_fingerprint.rs`): `--repin` moves the value of `main`. If your branch changes rule data, its own
  fingerprint is another value: run `cargo +1.98.1 test --release -p classic-rules --test rules_fingerprint`, and
  pin the number it prints (the old number stays in the list of earlier editions).

The script is idempotent (a second run changes nothing) and re-runnable (your new files and lines are
rewritten too, run it again after every merge of `main`). It touches tracked files only: `git add` a new file
before you run it. It ends with `cargo metadata --offline` (Cargo.lock keeps its packages in name order), so the
toolchain must be installed; a branch with a new external dependency may need the registry cache for it, the script then
says so and you run `cargo metadata` yourself. It does not run `cargo fmt`: the tree is not rustfmt-formatted
(the base commit is not `fmt --check` clean).

After `refactor/rename-classic` is merged into `main`, step 4 reads `git merge origin/main`; steps 1 to 3 stay as they are.

Things the script cannot do for you:

- Save files and snapshots written by your branch: regenerate them (`save_fixture`, `INSTA_UPDATE=always`
  or the test's own regeneration switch) after the merge. Of the pins only the ruleset fingerprint moved with the
  rename (`--repin`): game hashes, world hashes of frozen saves and screen snapshots do not read names.
- A `.civsave` written before the rename does not load any more (the magic is `AURELIA`, the extension `.aurelia`).
  The script rewrites the magic of the binary fixtures it renames, which is enough for fixtures that
  do not store the module name.
- A call site you wrote that builds a name by hand (`format!("civ5_{x}")`) is not seen as a word.

## The guard

`sh classic/tools/rename-classic.sh --check` changes nothing and fails (exit 1, with the first lines) when the
script would still change a tracked file, that is when `civ5`, `Civ5`, `CIV_*`, `core:civ/`, `civsave`,
`civlog`, `bnw` and the other old names came back. CI runs it (`.github/workflows/ci.yml`). The only places
that keep an old word are listed in `PROTECT` at the top of the script: the original game's file and table
names (`CIV5Buildings`, `Civ5Pkg`, `Data/Civ5/BNW`), URLs, other people's local paths, our past branch
names (`civ5/unit-quality`), the dump name `fandom-bnw-dump`, the atlas file names `units-bnw-*` and the art-pack ids `bnw:<kind>/<slug>`
(`bnw:unit/scout`: the ids in the atlas indexes under `assets/visual`, which the script does not rewrite).
A line that has to name the old words (the inventory in `classic/docs/classic/SOURCES.md`) carries the marker
`rename-classic:keep`; the script and the guard skip it.

## The mapping

| Old | New |
|---|---|
| crates `civ5-rules`, `civ5-content`, `civ5-ui` (dirs, packages, path deps, `use civ5_*`) | `classic-rules`, `classic-content`, `classic-ui`, `use classic_*` |
| crate `civ-qa` (the only other crate or binary with `civ` in its name) | `aurelia-qa`, `use aurelia_qa` |
| Rust type `Civ5`, `Civ5Ui`, `Civ5Game`, ... (`Civ5` + anything) | `Classic`, `ClassicUi`, `ClassicGame`, ... |
| module `civ5` (`crate::civ5`, `src/civ5/`), ModuleId `"civ5"` | `classic`, `"classic"` |
| `CIV_*` environment variables (no fallback to the old names) | `AURELIA_*` |
| `CIV_RULES=civ5` / `CIV_RULES=lal` | `AURELIA_RULES=classic` / `AURELIA_RULES=lands` (`sandbox` stays; the module ids `lal`, `sandbox` stay) |
| `CIV_LAL_MODE` | `AURELIA_LANDS_MODE` (mode names `short_80`, `full_200` stay) |
| `CIV_GAME_CIV`, `CIV_VISUAL_CIV` | `AURELIA_GAME_NATION`, `AURELIA_VISUAL_NATION` |
| consts `CIV_SLUG`, `CIV_COEFFICIENT`, `CIV_CONSTANT` (not environment) | `NATION_SLUG`, `NATION_COEFFICIENT`, `NATION_CONSTANT` |
| content keys `core:civ/<x>` (also `test:civ/`, `mod:civ/`) | `core:nation/<x>` |
| save magic `CIVSAVE\0`, extension `.civsave` | `AURELIA\0`, `.aurelia` |
| journal magic `CIVLOG\0`, extension `.civlog`; slot tag `CIVMODS\0` | `AURLOG\0`, `.aurlog`; `AURMODS\0` (all 8 bytes as before) |
| `docs/civ5/`, `civ5-*` file names, snapshots `civ5-*.snap`, `describe_civ5.rs`, `metrics_civ5.rs` | `classic/docs/classic/`, `classic-*`, ... |
| `classic/assets/rules/bnw`, `bnw_import`, `bnw_game`, `BNW_FINGERPRINT`, test file names `*_bnw.rs`, labels `*.bnw` | `classic/assets/rules/classic/`, `classic_import`, `classic_game`, `CLASSIC_FINGERPRINT`, `*_classic.rs`, `*.classic` |
| `civ5-bnw-spec/1` (schema tag of `classic/docs/classic/*.json`) | `classic-spec/1` |
| `civdb` (the generator's parser of the original database) | `srcdb` |
| `civ-bench` (bench line protocol tag), `civ-models/2` (schema tag of `platform/assets/models/manifest.json`, `render/build/units.rs`, the Blender exporter), test scratch directories `civ-*` | `aurelia-bench`, `aurelia-models/2`, `aurelia-*` |
| prose `Civ5` outside backticks (comments, Markdown) | `Civ V`: it cites the reference game |
| `Ok("lal")` in `lands/app/desktop/src/module.rs`, the module ids `"civ"` of the client tests | `Ok("lands")`, `"classic"` (`patch()` in the script) |
| UI hover text "From the Civ V catalog" | "From the ruleset catalog" (`patch()`) |

Kept on purpose (classified in the final grep report of the rename commit): the word "civilization" and its
abbreviation `civ`/`civs` as the name of the game concept (`CivDefinition`, `civ: u8`, `civs.ron`,
`civilizations.ron`, `Screen::Civilizations`), "civic(s)", "civil", "civilian"; the citations "Civ V", "Civ 5",
"Unciv", "BNW" (the original expansion) in provenance texts and comments; the original game's own
names in the importer (`CIV5*` tables, `MINOR_CIV_*`, `CvMinorCivAI`, `Civilization_*`); historical crate names
in old documents (`civ-core`, `civ-render`, ...); the repository name `civ-visual-sandbox`; local paths of the
machines the work was done on; `ru/notes/`, git history, `assets/visual/`.
