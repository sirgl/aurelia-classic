# classic

A rule module for the Aurelia platform: an Unciv / Civ 5 inspired game with a similar rule set (cities, a tech
tree, policies, religion, city states, diplomacy, trade, espionage, ...), with its AI and its user interface.
It is the large, strategy-classic game of the project. **This folder is meant to stay private**
(`docs/classic/SOURCES.md` and `docs/classic/RESOURCING.md` say where its data comes from and what is still being re-sourced).

| Crate | What it is |
|---|---|
| `classic-rules` | the rules: world state, systems, combat, derived mechanics, save sections, the AI (`src/ai`) |
| `classic-content` | the ruleset data (`assets/rules/classic/*.ron`) and its loader/importer |
| `classic-ui` | screens, view models and HUD panels on the platform shell; offscreen capture tools, benches and examples |
| `launcher/desktop` | the binary `aurelia-classic` (and the `visual-sandbox` showcase bench): registers `classic-ui` with `launcher_core::desktop` |
| `launcher/web`, `launcher/android` | the browser (wasm) and Android builds of the same launcher |
| `launcher/packaging`, `launcher/launcher.conf` | the product's identity (names, icons, `.desktop`, `Info.plist`) and the art pack it ships |

The tests and examples that play classic games through the platform's renderer and client live in
`crates/classic-ui/{tests,examples}` (they used to sit in `render` and `client`; the platform builds without any module).

## Build

```sh
# needs the platform folder beside this one: ../platform
cd classic
cargo +1.98.1 check --workspace --all-targets
cargo +1.98.1 test --release --workspace
cargo +1.98.1 run --release -p classic-desktop      # the game "Aurelia" (binary aurelia-classic)
```

The run-time art pack is not in git: `sh ../platform/tools/fetch-assets.sh` unpacks it into
`assets/visual/generated` (its version is `assets/PACK-VERSION`); without it the game runs with placeholder pictures.

The classic module's own 3D models (units, wonders, resources, improvements, city pieces: 238 files) and the
`core:` keys that bind them are in `assets/models/` (manifest, animation table, GLBs; generators in
`tools/blender/`); what both games draw stays in `platform/assets/models/`. See `../docs/models-ownership.md`.

## How it relates to the other parts

- Depends on **platform** (path dependencies `../../../platform/crates/...`) and on nothing else.
- Its launcher is in this folder (`launcher/`); there is no dependency on **lands** and none from lands on classic.
- Tools: `tools/rename-classic.sh --check` (naming guard), `tools/sources-lint.sh` (source-naming guard),
  `tools/unciv-*.py`, `tools/i18n/gen_content_classic.py`. Documents: `docs/` (rules reference `docs/classic/`, the AI, QA).
- The folder `integration/` of the monorepo holds the tools that link classic and lands together (the AI tournament,
  the gallery, the dedicated server); they are not part of this folder.

## Licence

This part stays private. Its code is dual-licensed MIT OR Apache-2.0 like the other parts ([LICENSE-MIT](LICENSE-MIT),
[LICENSE-APACHE](LICENSE-APACHE)); the rule data derived from Unciv stays under the Mozilla Public License 2.0
([LICENSE-MPL-2.0](LICENSE-MPL-2.0), attribution in [docs/classic/UNCIV-ATTRIBUTION.md](docs/classic/UNCIV-ATTRIBUTION.md));
art under [CC BY-SA 4.0](LICENSE-ART). See [NOTICE](NOTICE) and [THIRD-PARTY.md](THIRD-PARTY.md).
