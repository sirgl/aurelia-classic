# Sources of Civ V-derived data

Status: inventory taken on 2026-10-20 at base commit `c04d510f` (branch `docs/civ-sources`). This file is the one place in the repository that may name the original game's files, SDK and the third-party copies of them, because it has to say where the data really came from. Everywhere else those names are forbidden (see "Guard" at the end).

**Update 2026-10-21: the catalogs are re-sourced.** The inventory below is the state before; its "Verdict" column now says what was done (`done`, `provenance only`). The record of the work, the list of what could not be re-sourced and the method are in [RESOURCING.md](RESOURCING.md); which file comes from which Unciv file is in [UNCIV-ATTRIBUTION.md](UNCIV-ATTRIBUTION.md). The generator that downloaded the original game's data is deleted. Unciv has no Brave New World rule set (checked 2026-10-21 at its commit `b6377fdb`), so BNW-only content is `own value, unverified`. What stays open: the formulas of `rules.md` (row 20), the code constants (rows 21-22) and 272 typed effects without a matching Unciv rule (row 19).

## The rule

The project owner, 2026-10-20: "go through all of Civ and set the sources: we must not refer to Civ directly anywhere: except to Unciv". With the standing rule (`ru/notes/work/PLANS-AND-INSTRUCTIONS.md`, section 3: Civ V rules come from observable behaviour and public sources, like Unciv; no game XML/SDK/DLL code and no verbatim game texts; own rebalancing allowed) this means:

- every Civ V-derived datum, rule, number or text must cite the only permitted source, or be our own work (`own value`, `own formula`):
  1. **Unciv** (<https://github.com/yairm210/Unciv>, MPL-2.0): its rule sets `android/assets/jsons/Civ V - Vanilla/` and `Civ V - Gods & Kings/`, its Kotlin sources (`core/src/com/unciv/...`) and `docs/`; cite file and entry, and pin a commit when it matters;
- the repository must not point to the original game's files, XML, SDK, DLL, decompiled code, or to third-party repositories and dumps that copy them.

A citation is added only after the claim was checked against the cited source. Where it was not, the datum is flagged here instead (never invent a citation).

## Verdicts

| Code | Meaning |
|---|---|
| **OK** | already cites the Unciv, or is the project's own work (own judgement, own words) |
| **RECITE** | the data is observable or public; only the label points at the wrong thing. Fix by relabelling after checking against Unciv |
| **RESOURCE** | the only origin is the game's XML, DLL or a database dump. The numbers stay for now but are flagged; the re-sourcing plan names the Unciv file that could replace them |
| **VERBATIM** | text copied from the game (descriptions). To be replaced by text generated from typed effects or written anew |

## What Unciv has, and what it does not

Unciv's repository (pinned commit `70d0f5c80289950d1073cc9a7c5ce8c1c3f27b0b`, master of 2026-10-20) contains the rule sets **Civ V - Vanilla** and **Civ V - Gods & Kings**. It contains **no Brave New World rule set** (tree listing of that commit through the GitHub API: under `android/assets/jsons/` there are `Civ V - Gods & Kings`, `Civ V - Vanilla`, `TileSets`, `translations` and a few single files; no path in the whole repository contains "Brave" or "BNW"). So anything BNW-only (the Internet technology, ideologies and tenets, Exploration branch, tourism and great works, trade routes, World Congress, spies, archaeology, the DLC civilizations of BNW) has no Unciv source and is our own design. Coverage numbers in the "Re-sourcing plan" are therefore name-by-name matches against G&K/Vanilla.

## Inventory (state before the re-sourcing; the Verdict column is updated)

Counts below were measured on the base commit; the commands are in the next section. "Entered" says how the data came into the repository, "Real origin" what it was made from, as far as the repository itself records it (README of `classic/docs/classic`, commit messages, generator scripts). The generator was **not** re-run for this inventory [not re-run; origin taken from `classic/docs/classic/README.md` and the scripts]. The importer was re-run and reproduces the committed `classic/assets/rules/classic/*.ron` and `classic/docs/classic/implemented.md` byte for byte [measured by me: `git status` clean after `classic-import`].

| # | Category | Where | Count | Entered | Real origin | Verdict |
|---|---|---|---:|---|---|---|
| 1 | Regular units (default unit per class, great people, ship parts) | `classic/docs/classic/units.json` -> `classic/assets/rules/classic/units.ron` | 82 | `dde1395b` (2026-10-12) generator output; `0a127619` importer | game XML; 6 entries carry `verify_note` about a "final-patch DB dump" | **done**: Unciv `Units.json` (74 of 82); 8 own value, unverified; the 82 texts removed |
| 2 | Unique units | `unique_units.json` -> `unique_units.ron` | 59 | same | game XML; 12 entries from a wiki copy of the game database (`source: fandom-bnw-dump`); 2 with `verify_note` | **done**: Unciv `Units.json` (47 of 59); 12 own value, unverified (Brave New World units); the 59 texts removed |
| 3 | Buildings (regular, religious, guilds, palace) | `buildings.json` -> `buildings.ron` | 62 | same | game XML | **done**: Unciv `Buildings.json` (55 of 62); 7 own value, unverified; the 62 texts removed |
| 4 | Unique buildings | `unique_buildings.json` -> `unique_buildings.ron` | 20 | same | game XML; 3 entries from the database copy | **done**: Unciv `Buildings.json` (15 of 20); 5 own value, unverified; the 20 texts removed |
| 5 | Wonders, national wonders, projects, World Congress projects | `wonders.json` -> `wonders.ron`, `projects.ron` | 67 (47 + 11 + 6 + 3) | same | game XML | **done**: Unciv `Buildings.json` (50 of 67, projects have none); 17 own value, unverified; the 74 texts removed |
| 6 | Technologies | `techs.json` -> `techs.ron` | 81 | same | game XML (costs also from the formulas of row 20) | **done**: Unciv `Techs.json` (80 of 81, costs equal in 80); 1 own value, unverified (The Internet); the 81 texts removed |
| 7 | Policies, tenets, openers, finishers, branches | `policies.json` -> `policies.ron` | 111 + 12 branches | same | game XML | **done** (provenance, texts): Unciv `Policies.json` (64 of 123 entries); 59 own value, unverified (ideologies, Exploration, BNW changes); the 123 texts removed, the UI describes policies from their effects |
| 8 | Beliefs and religions | `beliefs.json` -> `beliefs.ron` | 69 + 13 | same | game XML | **done** (provenance, texts): Unciv `Beliefs.json`, `Religions.json` (55 of 69); 14 own value, unverified; the 69 texts removed |
| 9 | Resources | `resources.json` -> `resources.ron` | 42 | same | game XML (2 entries flagged `verify`) | **done**: Unciv `TileResources.json` (35 of 42: class, yields, revealing technology, improvement); 7 own value, unverified; texts removed |
| 10 | Improvements | `improvements.json` -> `improvements.ron` | 29 | same | game XML; the Polder from the database copy | **done**: Unciv `TileImprovements.json` (23 of 29: technology, work time); 6 own value, unverified; texts removed |
| 11 | Terrain, features, natural wonders | `terrain.json`, `features.json`, `natural_wonders.json` -> `.ron` | 9 + 8 + 17 | same | game XML; the work time of removing forest, jungle, marsh "written from memory", flagged `verify` | **done**: Unciv `Terrains.json` (terrain 9 of 9, features 8 of 8, natural wonders 11 of 17); 6 own value, unverified; table notes about the original's tables removed |
| 12 | Promotions | `promotions.json` -> `promotions.ron` | 214 | same | game XML | **done**: Unciv `UnitPromotions.json` (93 of 214 by name or id), 47 more through the Unciv unit, wonder, policy or nation that carries or grants them; 44 own value (checked by hand); 30 own value, unverified (listed in RESOURCING.md section 3); the 214 texts removed |
| 13 | Civilizations, leaders, abilities | `civilizations.json` -> `civilizations.ron` | 43 | same; 8 civilizations of Gods & Kings from the database copy (16 `source: fandom-bnw-dump` marks) | game XML + database copy | **done** (provenance): Unciv `Nations.json` (35 of 43); 8 own value, unverified; ability texts removed |
| 14 | City-state types and names | `city_states.json` -> `city_states.ron` | 58 + 5 types | same | game XML | **done** (provenance): Unciv `Nations.json`, `CityStateTypes.json` (42 of 58, 5 types); 16 own value, unverified |
| 15 | Eras, great people | `eras.json`, `great_people.json` -> `.ron` | 8 + 9 | same | game XML | **done**: Unciv `Eras.json` (8 of 8: research agreement cost, embarked defence), `Units.json`/`Specialists.json` for great people (7 of 9); texts removed |
| 16 | City-name and spy-name pools of the 43 civilizations | `civilizations.ron` (`city_names` 763 names, `spy_names` 272) | 1035 names | `d68dfd1e` (2026-10-15, "Codex session", commit message gives no origin) | not recorded; 700 of the 763 city names are in Unciv's `cities` lists of the civilizations it has (35 of 43), only 8 of 272 spy names are in Unciv's `spyNames` | **done**: Unciv `Nations.json` `cities` (35 nations) and `spyNames` (34); the lists of the nations Unciv lacks stay as they were (place names are facts); the Huns' one-name list is filled up with the old names |
| 17 | Difficulty levels (player and AI percentages) | `classic/assets/rules/classic/handicaps.ron` | 8 levels | `0e370e15` (2026-10-13), written by hand from a table of `rules.md` | game database table `HandicapInfos` (header says so); 115 of 120 compared values equal Unciv's `Difficulties.json` | **done**: Unciv `Difficulties.json` rewrites the fields it carries (classic/tools/import-unciv.py); one value kept (Prince AI unit upkeep 85, see RESOURCING.md) |
| 18 | Leader personalities (AI traits 0..10) | `classic/assets/rules/classic/personalities.ron`, generator `import/personalities.rs` | 43 leaders | `de4dd5a4` (2026-10-16) | "this project's own judgement from public descriptions of the leaders" (file header; `classic/docs/ai-leaders.md`) | OK (own judgement; the "public descriptions" are not named) |
| 19 | Effect model: modifier names and per-effect `source` | `effect_vocabulary.json` (354 modifiers, 33 selector kinds, 18 triggers); `source` on effects | 1213 effects with a `source` naming a game table and column (91 distinct tables) | `dde1395b` | the game database schema (`Building_YieldChanges.Yield`, `UnitPromotions.CityAttack`, ...) | **done** (labels, 2026-10-21 comparison): 597 of 1213 effects `source: unciv: <File>.json, <entry>` (stats and the unique translation table of `import-unciv.py`), 344 `own value` (numbers checked by hand), 272 `own value, unverified`; the vocabulary file is regenerated without column names; six effects the importer recognises by role carry a neutral `tag` |
| 20 | Formulas and turn order | `classic/docs/classic/rules.md` (5438 lines, 541 KB, Russian) | 122 "Source:" lines, 32 distinct SDK source files, 261 lines naming SDK classes, 24 lines naming game XML files, 76 `[verify]` | `dde1395b` | written from the Firaxis SDK sources (`CvCity.cpp`, ...) and the game XML, close to line by line (`notes/memory/civ-rules-from-behaviour.md`); a few paragraphs cite CivFanatics, Carl's Guides and Unciv | RESOURCE (the largest group); 3 paragraphs already cite Unciv: OK |
| 21 | Code comments pointing at `rules.md`. The SDK/XML/DLL names were removed in the formulas cleanup, the define, table and column names in the cleanup of 2026-10-21 (see the section below) | `crates/**` | 236 `rules.md` references in 138 files; 53 doc comments of the form "Civ V `DEFINE_NAME`"; 54 comment lines naming `CvXxx::method` or `CvXxx.cpp`; 15 comments saying "the DLL"; 48 `[verify` | from `dde1395b` on | transitive: the numbers and rules in the code come from row 20 | RESOURCE (transitive); the 15 "DLL" comments are RECITE/RESOURCE case by case |
| 22 | Code constants named after the game's defines (labels and about 90 identifiers cleaned on 2026-10-21; remaining variants listed below) | `classic/crates/classic-rules`, `platform/crates/kernel` | 518 `pub const/static` in `classic-rules`; 125 with a doc comment naming an UPPER_CASE define | from `dde1395b` on | the game's `GlobalDefines` (names and values) | RESOURCE |
| 23 | Generator pipeline | `classic/docs/classic/generator/` (15 files, 2625 lines of Python and shell) | 1 | `dde1395b`, `8be213a3` | downloads the game's XML from a GitHub copy of an installed game and the database dump from a wiki; builds the JSON of rows 1-15 | **done**: the generator is deleted; `classic/tools/import-unciv.py` replaces it (reads a read-only Unciv checkout) |
| 24 | Generated report | `classic/docs/classic/implemented.md` (header text in `import/report.rs`) | 1 | `0a127619` | the importer; two sentences name `AssignStartingPlots.lua` and `CIV5Builds.xml` | **done**: the header text of `import/report.rs` is neutral; `implemented.md` is regenerated by the importer |
| 25 | Spec README of the catalogs | `classic/docs/classic/README.md` | 1 | `dde1395b` | table of sources S1-S5 with links to the XML copy, the SDK copy and the wiki dump | **done**: README rewritten, the table of sources S1-S5 is gone |
| 26 | encyclopedia screen and concept pages | `classic/crates/classic-ui/src/ui/pedia/` | 1 screen; 30 concept pages | `9f647b90` (move), earlier | built from the compiled ruleset (`describe.rs`); the 30 concept pages are the project's own text | OK (a product feature, not a source) |
| 27 | AI numbers and measurements | `docs/ai*.md`, `docs/QA.md`, `docs/LAB.md`, benches | n/a | 2026-10-13 .. 2026-10-20 | own measurements and designs; they use rows 1-17 as inputs. Some name `rules.md` sections | OK (own); inputs inherit rows 1-22 |
| 28 | Third-party public references | `classic/docs/classic/rules.md`, `classic/docs/ai-allocator.md`, `classic/docs/ai-bench.md`, `own-game-docs/` | 23 lines: 10 CivFanatics threads, 5 Carl's Guides lines, 3 designer-notes (Old World, not Civ V), Civilization Wiki pages | various | public wiki, forum and guide pages | **done**: the forum, guide and wiki citations are removed from comments, `rules.md` and the AI documents |
| 29 | Art, audio, models, fonts | `assets/visual`, `assets/models`, `assets/audio` | n/a | n/a | generated or hand-made for the project; they carry Civ V names only as ids | OK (out of scope; not checked image by image) |
| 30 | Project documents that describe the situation | `PLAN.md`, `project.md`, `structure.md`, `docs/archive/*`, `notes/*` | see counts | n/a | statements such as "sources S1-S3 take numbers from XML and game code" | RECITE (point here) |

## Counts

Run from the repository root at the base commit (`git checkout c04d510f`; `LC_ALL=C` because `git grep` rejects Cyrillic ranges otherwise). `ru/notes/`, `output/` and `ru/benches/results/` (agent logs and histories) are excluded from the totals and counted separately.

```sh
export LC_ALL=C
A='\bCv[A-Z][A-Za-z]{3,}'                                        # SDK class and file names
B='CIV5[A-Za-z]+\.xml|Civ5[A-Z][A-Za-z]+\.xml|Gameplay/XML|Assets/(DLC|Gameplay)|Civ5Pkg|GlobalDefines|AssignStartingPlots'   # game XML/Lua
C='Bob-s-Mod|Keith7287|Community-Patch-DLL|LoneGazebo|CvGameCoreDLL'   # third-party copies of the game files
D='fandom-bnw-dump|Module:Data/Civ5|fandomdb'                    # the database copy on a wiki
E='\bDLL\b|\bSDK\b|[Dd]ecompil|[Dd]isassembl|Firaxis'            # code and vendor
F='final-patch (DB|dump)|DB dump|dump of the (game|final)|game database|tables\.json'
for id in A B C D E F; do eval p=\$$id
  git grep -IE "$p" -- . ':!ru/notes' ':!output' ':!ru/benches/results' | wc -l      # lines
  git grep -IlE "$p" -- . ':!ru/notes' ':!output' ':!ru/benches/results' | wc -l     # files
done
```

| Pattern | What it finds | Lines | Files |
|---|---|---:|---:|
| A | SDK class/file names (`CvCity::...`, `CvPlayer.cpp`) | 363 | 64 |
| B | game XML / Lua file names, `GlobalDefines` | 70 | 18 |
| C | third-party copies of the game files | 8 | 5 |
| D | database copy on a wiki | 42 | 11 |
| E | "DLL", "SDK", decompiled, Firaxis | 96 | 25 |
| F | database dump wording | 27 | 9 |
| any of A-F | lines / files (a line can match several) | **542** | **100** |

Biggest holders of the 542 lines [measured]: `classic/docs/classic/rules.md` 339; `classic/docs/classic/civilizations.json` 16; `classic/docs/classic/unique_units.json` 14; `classic/docs/classic/README.md` 10; `classic/docs/classic/generator/*` 38 in 9 files; the rest are single-line code comments in `crates/` (about 90 lines in 60 files) and a few docs.

Other counts [measured by script, same commit]:

| What | Command | Result |
|---|---|---:|
| `text` strings in the catalogs | Python over `classic/docs/classic/*.json`, key `text` | 859 strings, 95,129 characters (all keys `text` in the catalogs except `effect_vocabulary.json`) |
| of them copied into the compiled ruleset | `grep -o 'text: "' classic/assets/rules/classic/*.ron` | 180 (policies 111, beliefs 69) |
| catalog items with a `game_type` key (UNIT_..., BUILDING_...) | Python | 988 |
| effects carrying `source` (a game table.column) | Python over `modifier`+`source` | 1213 |
| entities with `source: fandom-bnw-dump` | `git grep -c fandom-bnw-dump -- classic/docs/classic/*.json` | 16 (civilizations) + 12 (unique units) + 3 (unique buildings) + 1 (improvement) = 32 |
| `verify: true` marks in the catalogs | Python | 17 |
| `verify_note` fields | `git grep -c verify_note -- classic/docs/classic/*.json` | 11 (6 in `units.json`, 2 in `unique_units.json`, 3 in `features.json`) |
| lines of `rules.md` starting "Source" | `grep -c '^Source' classic/docs/classic/rules.md` | 122 |
| `[verify]` in `rules.md` | `grep -o '\[verify\]' ...` | 76 |
| `rules.md` mentioned in code comments | `git grep -IE 'rules\.md' -- crates` | 236 lines, 138 files |
| docs that already cite Unciv | `git grep -il unciv -- docs` | 6 files (`rules.md` in 3 places, `ai-leaders.md`, 4 archived status and plan files) |

## Source labels S1-S5

Older documents (`classic/docs/classic/README.md`, `classic/docs/classic/rules.md`, JSON notes) use the labels S1-S5. They are defined only here. All five are verdict RESOURCE: none of them is the Unciv.

| # | What it is | Used for |
|---|---|---|
| S1 | The game's XML tables of Brave New World: a copy of an installed game in the GitHub repository `Keith7287/Bob-s-Mod` at commit `0b4f885` (the first commit of that repository, before the mod's edits). Loaded in the game's own order: base `Gameplay/XML`, then `DLC/Expansion2/Gameplay/XML`, then the DLC with BNW versions | all catalog tables, texts (en_US), the global constants table |
| S2 | A dump of the final BNW game database published on the Civilization Wiki as `Module:Data/Civ5/BNW/*` (150 modules) | 8 Gods & Kings civilizations missing in S1 (Austria, Byzantium, Carthage, Celts, Huns, Maya, Netherlands, Sweden), 12 units, 3 buildings, the Polder, 8 abilities; 13 values of the final patch where S1 is older; fallback texts |
| S3 | The Firaxis game-core source (SDK) of BNW as published in `LoneGazebo/Community-Patch-DLL` at commit `84de31b`, June 2014 (about patch 1.0.3.144) | all formulas of `rules.md`; the "Source:" lines name its files and functions |
| S4 | Public pages: Civilization Wiki, CivFanatics, Carl's Guides, patch notes | confirmation of behaviour where S3 is older; marked in `rules.md` |
| S5 | A mod-free branch of the same source from early 2015, built on the final patch (commit `be73076`) | only the diplomacy section of `rules.md` (warmonger scale by era), each place marked [verify] |

## Re-citing log (2026-10-20)

Each change below replaces a pointer at the game's files or at the database copy with a citation of Unciv, **only where the claim was compared with Unciv's file at commit `70d0f5c8`** by script or by reading. Where the comparison failed, the entry stays flagged (`verify: true`, `source: "unverified-legacy"`) and the failing value is written into its note. No gameplay number was changed. The generator was not touched, so a re-run would restore the old labels: the catalogs are the source of truth.

| What | Result of the check | New label |
|---|---|---|
| `units.json` Frigate, Galleass (`verify_note`) | Combat and RangedCombat equal Unciv G&K `Units.json` (25/28, 16/17) | citation of Unciv, `verify: false` |
| `unique_units.json` Ship of the Line | Combat 30 and RangedCombat 35 equal; Cost 170 differs (Unciv 185) | note says so, `verify: true` |
| `units.json` four spaceship parts | Cost 1500 differs from Unciv's 750 | note says so, `verify: true`, **unresolved** |
| `unique_units.json` Great Galleass | not in Unciv | note says so, `verify: true` |
| `units.json` Settler `cost_formula` | 106 equals Unciv's Settler cost; the formula itself is not in Unciv | number cited, formula pointer to `rules.md` |
| 11 unique units from the database copy (Pictish Warrior, Atlatlist, Dromon, Cataphract, African Forest Elephant, Quinquereme, Horse Archer, Hussar, Carolean, Hakkapeliitta, Battering Ram) | strength, ranged strength, movement, cost, required and obsolete technology, upgrade and replaced unit all equal Unciv G&K `Units.json` | `source: "unciv: ..."` |
| Sea Beggar | all equal except the obsolete technology (Combustion here, none in Unciv) | `unverified-legacy`, `verify: true` |
| Unique buildings Pyramid and Coffee House | cost (own or by the tech column of `Techs.json`), maintenance, technology, replaced building equal Unciv G&K `Buildings.json` | `source: "unciv: ..."` |
| Ceilidh Hall | maintenance 1 here, 2 in Unciv; the rest equal | `unverified-legacy`, `verify: true` |
| Polder (`improvements.json`) | food, technology, work time, bonuses after Economics, pillage gold, terrains equal Unciv G&K `TileImprovements.json` | `source: "unciv: ..."` |
| 8 civilizations and their 8 abilities from the database copy (16 marks) | names exist in Unciv's `Nations.json`, effect amounts not compared | `unverified-legacy` |
| `features.json` removal of jungle, marsh, forest | work time (7, 6, 4 turns) and technology (Bronze Working, Masonry, Mining) equal Unciv `TileImprovements.json`; the chop production is not in Unciv | citation for time and technology; `verify_note` keeps production unverified |
| `great_people.json` improvements of Scientist, Engineer, Merchant, General | Academy, Manufactory, Customs house, Citadel equal Unciv `Units.json` | citation |
| `city_states.json` five type notes | "allies get friends' bonus plus more" agrees with Unciv for Cultured, Maritime, Mercantile; Militaristic and Religious values differ or are absent | notes name which part agrees; Religious flagged |
| `resources.json` Bison and Cocoa | Unciv keeps both commented out of its rule set, which agrees with "not on random maps" | note cites it |
| `terrain.json` source and note | not checked | `unverified-legacy` |
| four `text_only` effect sources (1 policy, 3 promotions) and `effect_vocabulary.json` | a pointer to the game code only; no claim to check | neutral wording "not data-driven" |
| `classic/assets/rules/classic/handicaps.ron` header | 115 of 120 values equal Unciv G&K `Difficulties.json`; 5 AI percents differ (listed in the header) | citation with the 5 exceptions |
| `import/tables.rs` comment on hills and mountains | Unciv: Mountain impassable, Hill movement 2 and defense 0.25 | citation |
| `combat/damage.rs` header and test oracle | Unciv `BattleDamage.kt` has the same ratio factor and 24 + 12 x random base; `BattleConstants.kt` the 40 for civilians | citation; other constants unverified |
| `combat/strength.rs` header | Unciv adds all modifiers into one factor (`modifiersToFinalBonus`); enemy auras not in Unciv | citation, aura part flagged |
| `systems/growth.rs` `growth_threshold` | Unciv `CityPopulationManager.kt` has `15 + 8 (pop-1) + floor((pop-1)^1.5)` | citation |
| `import/report.rs` -> `classic/docs/classic/implemented.md` | regenerated by the importer; only two sentences changed | neutral wording |
| `classic/docs/classic/README.md` | the table of sources S1-S5 and the generator paragraph | pointer here; no links to the copies |
| `classic/docs/classic/rules.md` heading lines | the title, the sources paragraph, two convention lines | pointer here; the 122 "Source:" lines stay (RESOURCE, counted in the guard baseline). Superseded: the file is generated now |

Reproducibility: `cargo run --release --offline -p classic-content --features importer --bin classic-import` after these edits changes only `classic/docs/classic/implemented.md` (two sentences) [measured by me]; `classic/assets/rules/classic/*.ron` other than the hand-written `handicaps.ron` are byte-identical.

## Re-sourcing plan

Scope: the groups with verdict RESOURCE or VERBATIM that remain after the re-citing above. Nothing here is done by this branch; no gameplay number or hash is touched. "Coverage" is the output of `python3 -I classic/tools/unciv-coverage.py --fetch DIR` (Unciv commit `70d0f5c8`, names matched after dropping case, a leading "the", punctuation, and the renames Railroad/Railroads, Penicillin/Pharmaceuticals, Gold/Gold Ore) [measured by me, 2026-10-20]. "Equal" counts entries present in Unciv G&K whose compared numbers are all equal. Sizes are my estimates in sessions of one agent (S under one, M one to three, L more) [estimate, not measured].

| Group | Size of the work | Unciv file that could replace it | Coverage (G&K / catalog) | Equal | What has no Unciv source |
|---|---|---|---|---|---|
| Units, default per class (row 1) | S: relabel after comparing 8 unequal ones | `Civ V - Gods & Kings/Units.json` | 74 / 82 | 66 of 74 (strength, ranged, movement, cost) | Work Boat, Cargo Ship, Caravan, Great Writer, Great Musician, Archaeologist, Bazooka, XCOM Squad (BNW); the 8 unequal are the 4 spaceship parts (750 vs 1500), Giant Death Robot, Artillery, Battleship, Foreign Legion |
| Unique units (row 2) | S | same | 47 / 59 | 46 of 47 | 12 BNW and DLC units (Immortal, Impi, Winged Hussar, Pathfinder, Comanche Riders, Nau, Berber Cavalry, Kris Swordsman, Siege Tower, Pracinha, Merchant of Venice, Great Galleass) |
| Buildings (row 3) | S | `Buildings.json` | 55 / 62 | 47 of 51 with a cost (cost from the tech column of `Techs.json` when absent) | Zoo, Hotel, Caravansary, Airport, the three Guilds (BNW) |
| Unique buildings (row 4) | S | `Buildings.json` | 15 / 20 | 14 of 15 | Ikanda, Ducal Stable, Royal Library, Candi, Hanse |
| Wonders and projects (row 5) | S | `Buildings.json` (isWonder, isNationalWonder) | world 39 / 47, national 9 / 11, projects 2 / 6, World Congress projects 0 / 3 | 48 of 50 | Uffizi, Globe Theatre, Broadway, Red Fort, Prora, Borobudur, Parthenon, International Space Station; East India Company, National Visitor Center; the four ship-part projects; all three World Congress projects |
| Technologies (row 6) | S | `Techs.json` (columns carry the era costs) | 80 / 81 | prerequisites equal in 78 of 80 | The Internet (BNW) |
| Policies (row 7) | M | `Policies.json` | branches 11 / 12, policies 30 / 45, openers, finishers and tenets 23 / 66 | not compared | Exploration branch; the three ideologies with their 48 tenets; 15 BNW-changed policies (Cultural Centers, Fine Arts, Treasure Fleets, ...) |
| Beliefs and religions (row 8) | S | `Beliefs.json`, `Religions.json` (G&K only; Vanilla has none) | 55 / 69 | not compared | 14 BNW beliefs (Reformation tier and others) |
| Resources, improvements, terrain, features, natural wonders (rows 9-11) | S | `TileResources.json`, `TileImprovements.json`, `Terrains.json` | resources 35 / 42, improvements 23 / 29, terrain 9 / 9, features 8 / 8, natural wonders 11 / 17 | not compared | Antiquity Sites, Nutmeg/Cloves/Pepper, Bison/Cocoa (disabled in Unciv); Encampment, Archaeological Dig, Brazilwood Camp, Kasbah, Feitoria, Chateau; Mt. Fuji, Mt. Sinai, Mt. Kailash, Lake Victoria, Mt. Kilimanjaro, King Solomon's Mines |
| Promotions (row 12) | M | `UnitPromotions.json` | 79 / 214 by name | not compared | 135 entries (numbered variants, air, hard-coded and civilization-specific ones); describe with own words |
| Civilizations, leaders, abilities (row 13) | M | `Nations.json` | 35 / 43 | not compared | Assyria, Brazil, Indonesia, Morocco, Poland, Portugal, Shoshone, Zulu (BNW). Do **not** copy Unciv's leader quotes and introduction texts: they come from the game |
| City-states (row 14) | S | `Nations.json` (cityStateType), `CityStateTypes.json` | 42 / 58 | not compared | 16 BNW city-states |
| Eras (row 15) | S | `Eras.json` | 8 / 8 | not compared | |
| City names (row 16) | S | `Nations.json` `cities` | 35 of 43 civilizations; 700 of 763 names are in Unciv's lists | n/a | 63 names and 8 civilizations |
| Spy names (row 16) | S | `Nations.json` `spyNames` | 34 of 43 civilizations; only 8 of 272 names match | n/a | origin unknown; either adopt Unciv's lists or write new ones |
| Difficulty levels (row 17) | done | `Difficulties.json` | 8 / 8 | 115 of 120 values | route cost, free-unit percent, AI create and world percents, per-era modifier, the AI default level |
| Descriptions: 859 `text` strings, 95,129 characters (VERBATIM) | M: the encyclopedia screen already builds effect descriptions from typed effects (`ui/pedia/describe.rs`); the 180 strings in `policies.ron` and `beliefs.ron` are the first to replace because they are in the compiled ruleset | none: Unciv's own descriptions are generated from its `uniques`, which is the model to follow | n/a | n/a | all; write new text or generate from effects |
| Effect `source` labels: 1213 effects, 91 game table names | M: rename to neutral ids; the importer matches some of them in at least five places (`import/effects.rs` lines 52-53 and `lost_tile_filter`, `import/military.rs` lines 227 and 331) and must change with them; the RON output must stay byte-identical | none | n/a | n/a | all |
| `game_type` keys: 988 catalog items | S: only `import/tables.rs` (city-state types) reads it | Unciv names the entries by plain name; no key needed | n/a | n/a | all |
| Formulas, `rules.md` (row 20): 5438 lines | L, per section, in this order of value: growth, border and tile costs, production and purchase, policies, science, golden age, combat strength and damage, healing and experience, movement; then the sections Unciv has no source for | see the section table below | see below | see below | tourism, great works, trade routes, World Congress, score, happiness details: only the own design |
| Code comments and constants (rows 21-22): 236 `rules.md` references, 54 `CvXxx` comment lines, 15 "DLL" comments, 53 "Civ V `DEFINE`" doc comments | M, follows the rewrite of `rules.md` | n/a | n/a | n/a | the 15 "DLL" lines and what each claims are listed by `grep -rnw DLL crates` |
| Generator `classic/docs/classic/generator` (row 23) | S: delete after the above, or keep frozen; the project owner's decision | n/a | n/a | n/a | n/a |

Sections of `rules.md` and the Unciv file that has an equivalent (**the file exists in the tree at the pinned commit; whether it computes the same thing is not checked unless a "checked" mark says so**):

| `rules.md` section (lines) | Unciv file under `core/src/com/unciv/logic/` | State |
|---|---|---|
| 2. City growth (94) | `city/managers/CityPopulationManager.kt` | **checked**: same growth formula (see the re-citing log) |
| 3-4. Borders, buying tiles (132) | `city/managers/CityExpansionManager.kt` | exists, not compared |
| 5. Production and purchase (174) | `city/CityConstructions.kt` | exists, not compared |
| 6. Gold, 7. Happiness (248) | `city/CityStats.kt`, `civilization/transients/CivInfoStatsForNextTurn.kt` | exists, not compared |
| 8. Golden ages (64) | `civilization/managers/GoldenAgeManager.kt` | exists, not compared |
| 9. Policies (79) | `civilization/managers/PolicyManager.kt` | exists, not compared |
| 10. Science (115) | `civilization/managers/TechManager.kt` | partly **checked**: the share-of-known-technologies discount is already cited from it in `rules.md` |
| 11. Great people (83) | `civilization/managers/GreatPersonManager.kt` | exists, not compared |
| 12. Faith and religion (128) | `civilization/managers/ReligionManager.kt`, `city/managers/CityReligionManager.kt` | exists, not compared |
| 13. Score (47), tourism and influence (332), great works and archaeology (202), trade routes (423), World Congress (210) | none (no tourism, trade-route, congress or score file in the tree) | own design |
| 4. City-states (613) | `civilization/diplomacy/CityStateFunctions.kt` | exists, not compared |
| 5. Diplomacy (465) | `civilization/diplomacy/DiplomacyManager.kt`, `DiplomacyFunctions.kt` | exists, not compared |
| 7. Espionage (514) | `civilization/managers/EspionageManager.kt` | exists, not compared |
| 8. Victory (171) | `civilization/managers/VictoryManager.kt` | exists, not compared |
| Combat strength (274), damage (265), ranged (38), cities (113) | `battle/BattleDamage.kt`, `Battle.kt`, `CityCombatant.kt`, `BattleConstants.kt` | damage ratio factor and base **checked**; the additive modifier sum **checked**; the rest not compared |
| Healing (72), experience (85), movement (145), stacking (30), air (26) | `map/mapunit/MapUnit.kt`, `map/mapunit/movement/UnitMovement.kt`, `MovementCost.kt`, `battle/AirInterception.kt` | exists, not compared |

## Formulas and tables

The project owner, 2026-10-20 (via the coordinator): formulas and number tables must not be an explicit repeat of the original, and no formula, table or constant may mention the original's sources (XML, DLL, SDK, dumps). This section inventories them, says which are copies, proposes replacements and records what was done now. **No formula and no number was changed** (a changed number moves the play hashes and the balance: a decision for the project owner).

### Classes

| Class | Meaning | How decided |
|---|---|---|
| **PUBLIC** | same form and coefficients as a file of Unciv (MPL-2.0) or a public page that Unciv itself cites; the form is public knowledge | read Unciv's Kotlin or JSON at commit `70d0f5c8`, compared by `classic/tools/unciv-coverage.py` or `classic/tools/unciv-formulas.py` or by hand |
| **COPY** | same form and coefficients as the original, with no public statement found; the only recorded origin was the original's code or XML (`rules.md` was written from it, close to line by line). **Since 2026-10-21 no value is called COPY: each is `OWN, unverified` (labelled as such in its comment) and its difference from Unciv is listed below; adopting Unciv's value is a separate decision with a series run and a re-pin** | Unciv has a different form or no equivalent; checked by reading its file |
| **DERIVED** | own parametrization, an approximation or a simplification | the code or comment says so |
| **OWN** | own design (AI, L&L, interface) | project documents |

"Names the original" = the doc comment or field points at the original's code, tables or defines. After the cleanup of this branch no comment in `crates/` or `apps/` names SDK classes, the DLL or XML files [measured: `sh classic/tools/sources-lint.sh`, groups A-F, 0 lines in `crates` and `apps`]. What remains is listed per row.

### Inventory of formulas and tables

Reproduce the numbers in rows 1-6, 9-11 with `python3 -I classic/tools/unciv-formulas.py` (its Python copies of our functions are asserted against the tables of our Rust unit tests first) and with `python3 -I classic/tools/unciv-coverage.py --fetch DIR`.

| # | Formula or table | Where (file, key) | Entered from | Class | Evidence | Names the original? | Proposal |
|---|---|---|---|---|---|---|---|
| 1 | Technology cost: base, difficulty, map size, discount for known techs, city count | `derived/research.rs` `tech_cost`; `classic/world_size.rs`; base cost `techs.ron` field `cost` | importer (base cost from the game XML), rest hand-coded from `rules.md` §10.2 | PUBLIC (form and 80 of 80 base costs); the city term and two map-size values differ from Unciv | Unciv `TechManager.costOfTech`, `getScienceModifier` (the same 1 + 0.3 x known/living, citing a CivFanatics thread); `Techs.json` costs equal in 80 of 80; Unciv charges `(cities - 1) x` the per-city percent, we charge `cities x` it (factor 1.05 / 1.20 / 1.40 at 1 / 4 / 8 cities against 1.00 / 1.15 / 1.35); Large and Huge per-city percent here 3 and 2, Unciv 3.75 and 2.5 | no (cites Unciv and CivFanatics) | keep the form; **decision**: adopt Unciv's `(cities - 1)` and 3.75 / 2.5. Effect: at most 5 percentage points of cost; moves ai hashes |
| 2 | City growth threshold | `systems/growth.rs` `growth_threshold` | hand-coded | PUBLIC | `15 + 8 (n-1) + floor((n-1)^1.5)`: equal for population 1..30 (30 of 30) | no (cites Unciv) | none |
| 3 | Policy cost | `derived/culture.rs` `policy_cost`; `world_size.rs` `policy_cost_per_city` | hand-coded from `rules.md` §9.1 | PUBLIC (form); small differences | Unciv `PolicyManager.getPolicyCultureCost`: `25 + (3n)^2.01`, per-city percent 10 / 7.5 / 5, rounded down to 5, citing a CivFanatics thread; here: 25 + trunc((3n)^2.01) then per-city, round down to 5, Large 7; with 1 / 4 / 8 cities equal in 10 / 9 / 9 of 12 policy counts, worst difference 1.0 % / 6.2 % / 4.8 % | doc comments of the 10 constants of `culture.rs` (rows 3 and 4 together) carry the original's define names (`POLICY_CULTURE_COST_BASE`, `POLICY_CULTURE_COST_EXPONENT`, ...) | cite Unciv for the form; **decision**: adopt Unciv's rounding and 7.5. Rename the doc comments |
| 4 | Culture needed for the next border tile | `derived/culture.rs` `border_cost`; `kernel/src/world/city.rs` `CULTURE_COSTS` (128 values) | hand-coded from `rules.md` §3.1 | **OWN, unverified** (was COPY) | here `15 + (10 level)^1.1`, rounded down to 5; Unciv `CityExpansionManager.getCultureToNextTile`: `6 (claimed + 1.4813)^1.3`. Equal in 0 of 31 levels; ours / Unciv at level 0, 5, 10, 20, 30: 15/10, 85/68, 170/143, 350/323, 545/532 | (the same 10 constants) | replace by Unciv's formula (Unciv is cheaper by a third at level 0, 20 % at level 5, 16 % at 10, 8 % at 20, 2 % at 30) **or** a smooth own fit. Medium risk for the early game; verify below |
| 5 | Gold price of a tile | `kernel/src/world/territory.rs` `tile_purchase_offer`; `kernel/src/world/city.rs` older `purchase_price` | hand-coded from `rules.md` §4 | **OWN, unverified** (was COPY) | here `(50 + 5 bought) x (100 + 100 d / 3) %`, resource -100 %, rounded down to 5; Unciv `getGoldCostOfTile`: `50 (distance - 1) + 5 claimed`. Ratios ours / Unciv for ring 2 / 3 with 0, 10, 30 bought: 1.00/0.65, 1.00/0.87, 1.00/1.06 | comment cites no code now; 6 constants named after defines | replace by Unciv's formula; tolerance 35 % on the cheapest ring-3 tiles |
| 6 | Gold price of an item | `kernel/src/world/city.rs` `gold_purchase_price` | hand-coded from `rules.md` §5.4 | PUBLIC | `(30 P)^0.75` x item modifier, rounded to 10: Unciv `IConstruction.getBaseGoldCost` (cites a CivFanatics thread) | no | none |
| 7 | Production cost of units, buildings, wonders; building maintenance | `units.ron`, `buildings.ron`, `unique_*.ron`, `wonders.ron` | importer from the game XML | PUBLIC for the compared entries | Unciv G&K: unit strength / ranged / movement / cost equal in 66 of 74 default units and 46 of 47 unique units; building cost and maintenance equal in 47 of 51, unique buildings 14 of 15, wonders 48 of 50 | JSON field `game_type` (988 items) and `source` labels (1213 effects) still name the original's keys and table columns | cite Unciv per entry; the entries Unciv lacks are BNW-only (own) |
| 8 | Unit gold maintenance | `derived/maintenance.rs` (`GOLD_PER_UNIT_TIMES100` 50, multiplier 8, divisor 7, end turn 500) | hand-coded from `rules.md` §6.3 | **OWN, unverified** (was COPY) | here `((1 + 8 p) x 0.5 x paid) ^ (1 + p / 7)` with p = turn / 500; Unciv `CivInfoStatsForNextTurn.getUnitMaintenance` has the same shape with lighter coefficients: `(0.5 x paid x (1 + p)) ^ (1 + p / 3)`, 3 units free. Gold for 5 / 10 / 20 / 40 paid units at turn 50 / 150 / 300, ours / Unciv: 4/2, 9/3, 18/5; 9/5, 19/7, 38/12; 18/11, 39/16, 82/27; 37/24, 81/36, 174/63 (Unciv 1.5 to 2.8 times lower) | 9 constants, 5 with define names | **own parametrization** of the shape (two coefficients) fitted to the upkeep a player observes; Unciv's coefficients are a balance change, not a re-citation. Tolerance +-15 % of the total upkeep for 5 / 10 / 20 / 40 units at turns 50 / 150 / 300; check on series |
| 9 | Happiness: per city, per citizen, thresholds, effects | `derived/happiness.rs` (11 constants) | hand-coded from `rules.md` §7 | PUBLIC for city 3, citizen 1, captured city 5, threshold -10, growth -75 % and (since 2026-10-21) the citizen of an occupied city 2; **OWN, unverified** (was COPY) for the combat penalty 2 per point up to 40 | Unciv `CityStats.kt` (-3 per city, +2 for an annexed city, citizen x2 when annexed), `GlobalUniques.json` (-75 % growth between -10 and -1, -33 % strength below -10) | 10 constants name defines | cite Unciv for the PUBLIC ones; the combat penalty stays own (Unciv: flat -33 %) |
| 10 | Golden age: threshold, length | `systems/golden_age.rs` (5 constants) | hand-coded from `rules.md` §8 | PUBLIC | Unciv `GoldenAgeManager`: `(500 + 250 n) x (1 + cities / 100)`, length 10 | 5 constants name defines | cite Unciv |
| 11 | Great person threshold | `derived/great_people.rs` (100, doubling since 2026-10-21, as Unciv) | hand-coded from `rules.md` §11 | **OWN, unverified** (was COPY) (step) | Unciv: base 100 equal, but it doubles the threshold after each person; general: +50 per general equal | 3 constants name defines | decision: Unciv doubling or keep +100 as an own rule |
| 12 | Combat: damage and strength ratio | `combat/damage.rs` | hand-coded | PUBLIC | Unciv `BattleDamage.kt`: ratio factor, `24 + 12 random`, civilians 40 | no (cites Unciv) | none |
| 13 | Combat modifiers: fortify 20, flanking 10, river -20, amphibious -50 | `combat/strength.rs` | hand-coded from `rules.md` | PUBLIC | Unciv `BattleConstants.kt` (cites Carl's Guides): same four values | 8 constants name defines | cite Unciv |
| 14 | City strength (800, 40 per citizen, tech 5.5 and 2.8) | `combat/city.rs` | hand-coded | PUBLIC | Unciv `ModConstants`: base 8.0, per citizen 0.4, techs multiplier 5.5, exponent 2.8 (values x 100 here) | no | cite Unciv |
| 15 | Other combat constants: Great General 15 and range 2, hills 25, barbarian 40, sapper, experience table, healing, city hit points | `combat/*.rs` (46 constants) | hand-coded | **OWN, unverified** (was COPY) (not compared) | no Unciv value read | partly named | compare with Unciv `GreatGeneralImplementation.kt`, `MapUnit.kt` first; the rest own |
| 16 | Tourism, influence, great works, archaeology, trade routes, World Congress, spies, score, diplomacy opinion, city-state influence, quests, religion | `derived/tourism.rs`, `trade.rs`, `archaeology.rs`, `religion.rs`; `systems/city_states/` (47 constants), `diplomacy` (12), `espionage` (16), `congress` (9), `victory` (7), `barbarians` (11) | hand-coded from `rules.md` | **OWN, unverified** (was COPY) | Unciv has no tourism, trade routes, congress or score; city states, diplomacy, spies, religion and victory exist in other forms (see the section table above) | most of the 180 define-named comments | largest piece; see below |
| 17 | Difficulty levels | `handicaps.ron` | by hand | PUBLIC (115 of 120) | Unciv `Difficulties.json` | no | five AI percents decided |
| 18 | AI numbers | `classic-rules/src/ai/` (193 constants), `personalities.ron` | own | OWN | derived from ruleset data; no names | no | none |
| 19 | Effect model: modifier names, per-effect `source` | `effect_vocabulary.json`, `source` fields | importer | **OWN, unverified** (was COPY) of the original's schema | 1213 effects name a game table and column (91 tables) | yes, in data | rename to neutral ids with the importer (size M); the compiled output must stay byte-identical |
| 20 | Tables of defines in `rules.md` | `classic/docs/classic/rules.md` | written from the code | **OWN, unverified** (was COPY) | 362 table rows and 492 distinct UPPER_CASE names | yes, 214 lines | **done**: the file is generated from the code constants and catalogs |

Constants in the code: 430 `pub const` / `static` in `classic-rules`, `kernel`, `classic-content`, outside `ai/` and test files (the 193 constants of `classic-rules/src/ai/` are OWN); 180 of the 430 have a doc comment that names the original's define [measured by a script over the three crates: a doc line within three lines above the constant containing an UPPER_CASE name in backquotes]. The comment is a label only. The values are the original's and are the COPY class unless a row above says PUBLIC. Defines also give the constants their identifiers (`POLICY_CULTURE_COST_BASE`): a rename is free for the build but touches many files.

### What changed now

| Change | Result |
|---|---|
| 88 comment lines in 66 files of `crates/` (`CvXxx::method`, "the DLL", "GlobalDefines", "of the SDK") | rewritten to "Civ V" plus a pointer to `classic/docs/classic/rules.md` or to "not checked" [measured: group A-F lines in `crates` and `apps` 85 -> 0] |
| 117 "Source" lines of `rules.md` listing SDK functions and XML files | one neutral line; six part introductions stripped of their provenance paragraph |
| `platform/structure.md` (two lines) | neutral |
| `classic/tools/sources-lint.baseline` | ceilings lowered |
| Formulas, constants, rule data, hashes | **unchanged**; the importer output is byte-identical [measured: see Re-citing log] |

### Remaining mentions (by file)

`classic/docs/classic/rules.md` 214 lines (headings such as "`CvCity::doTurn()`", pseudo-code with the original's function names, defines, the DLL version); the frozen generator 24 lines; `lands/own-game-docs/DECISIONS.md` and `HISTORY.md` one line each (statements of the rule "never take the publisher's files"; kept on purpose); `ru/docs/archive-en/PLAN-2026-10-19-full.md` 2 lines (frozen). The 180 define names in doc comments and the 988 `game_type` keys and 1213 effect `source` labels in the data are not matched by the lint patterns and are listed in the table above.

### Replacement proposal for the COPY rows

Order of work (each step is its own commit, each followed by a series run and a re-pin with the reason in the message):

1. **Take Unciv's value where Unciv has the thing** (rows 1, 3, 4, 5, 9, 11): the numbers are public and cited. Size S per row; five rows change play. Risk: early-game balance (Unciv's border culture is a third cheaper at the start, its tile gold a third cheaper on the cheapest ring-3 tiles).
2. **Own parametrization where Unciv has no equivalent** (rows 8, 15, 16): a few parameters fitted to the behaviour a player sees (observed numbers), not to the original's coefficients. Size L (144 constants of the city-state, diplomacy, espionage, congress, victory, barbarian, tourism, trade, archaeology and religion modules [measured: `grep` of `pub const` per module]). Risk: these systems are rare in a game, so balance risk is low per row, but there are many.
3. **Tolerance and the check on series.** A replacement is accepted when, over 48 seeds on two trees (the existing AI tournament harness, `ai-tournament`), (a) the mean of the numbers it controls (tech cost, policy cost, border culture, tile gold, upkeep) at turns 50 / 150 / 300 is within +-10 % of the current rule for upkeep and policy cost, +-25 % for border culture and tile gold in the first 100 turns (Unciv's border formula passes only from level 5 on), and (b) the mean score at round 150, cities at round 100, techs at round 200 and the win-type shares move by less than the noise between two runs of the same code (run it twice to measure the noise first) [proposed, not run].
4. **Hashes.** The constants are code, not rule data: they are not in the ruleset fingerprint (`kernel/src/rules/fingerprint.rs` hashes the compiled tables) [from reading, not re-measured by changing a constant], but they move the play hashes (`ai_game`, `end_turns`, `ai_bench`, smoke): one re-pin per step. Data edits (rows 7, 19) can change the fingerprint; label-only edits do not [measured: the 20 RON files differ in a comment line only; tests green].
5. **Rule texts** (`rules.md`): done differently: the file is generated from the code (`classic/tools/gen-rules-doc.py`), so it follows every constant the replacements change.

Decisions of the project owner (2026-10-20):

- **(Superseded on 2026-10-21 by the project owner's order and the "Formula pass" section: rows 1, 3, 4, 5, 8, 9, 11 and the rest are adopted.)** Earlier decision: Unciv's values for rows 1, 3, 4, 5, 9, 11 were NOT adopted, only after the AI work. The plan and the table of differences stay here: the differences are in the "Evidence" column of the rows (cost per city 1.05 / 1.20 / 1.40 against 1.00 / 1.15 / 1.35 at 1 / 4 / 8 cities, Large and Huge per-city percent 3 / 2 against 3.75 / 2.5; policy cost worst difference 6.2 %, Large 7 against 7.5; border culture 15 against 10 at level 0, 545 against 532 at level 30; tile gold 65 against 100 on a ring-3 tile with nothing bought; occupied-city citizen 1.34 against doubled, combat penalty 2 per point (cap 40) against a flat -33 %; great person threshold +100 against doubling). A change of any of them is its own commit with a series run and a re-pin.
- **The constants without a Unciv equivalent (row 16) stay own value** and flagged (superseded wording: they were called COPY). An own parametrization is a separate task to be done before the repository is opened.
- Row 1, the question whether the per-city percent starts from the second city (Unciv) or the first (here), stays open and is part of the first step above.

## Formula pass (2026-10-21, the project owner: "take ALL remaining Unciv formulas, wherever the code does not already use official ones")

the project owner's order of 2026-10-21 replaces the decision of 2026-10-20 ("not adopted now"): every constant or formula for which Unciv has an equivalent now takes Unciv's shape and value; a constant stays ours only where Unciv has nothing (then it is marked `own value` in its doc comment, and the generated rules document `rules.md` lists it as such). Unciv read for this pass: commit `18827b1` (4.22.13), Kotlin under `core/src/com/unciv/` and the Gods & Kings JSON files. Each row below is its own commit on the branch `refactor/classic-unciv-formulas` (subject `classic: <what> <old> -> <new> (Unciv: <file>)`). Gameplay numbers change; hash pins were re-pinned after the merge of the parallel work, with the reason in the re-pin commit.

### Adopted from Unciv

| Constant or formula | Our file | Old | New | Unciv file |
|---|---|---|---|---|
| tech cost per city | `derived/research.rs`, `classic/world_size.rs` | percent x most cities ever (puppets counted), Large 3, Huge 2, rounded up | percent x (non-puppet cities - 1), Large 3.75, Huge 2.5, truncated (Writing on Standard with 1/4/8 cities: 63/72/84 -> 60/69/81) | `TechManager.kt`, `MapSize.kt` |
| policy cost | `derived/culture.rs`, `world_size.rs` | `trunc((3n)^2.01)`, cities x percent, Large 7 | power not truncated, (cities - 1) x percent, Large 7.5, rounded, then down to 5 (one city, 7 / 8 policies: 475 / 615 -> 480 / 620) | `PolicyManager.kt`, `MapSize.kt` |
| AI research and policy percent (default AI row) | `handicaps.ron` | 85 / 75 | 100 / 100 (Unciv applies the difficulty to the human player only) | `TechManager.kt`, `PolicyManager.kt` |
| border culture | `derived/culture.rs`, kernel `world/city.rs` `CULTURE_COSTS` | `floor5(15 + (10 t)^1.1)` | `round(6 (t + 1.4813)^1.3)` (levels 0 / 5 / 10 / 20 / 30: 15 / 85 / 170 / 350 / 545 -> 10 / 68 / 143 / 323 / 532) | `CityExpansionManager.kt` |
| border level input | kernel `systems/borders.rs` | culture claims only | tiles owned beyond the first ring, bought ones included | `CityExpansionManager.kt` |
| Tradition opener | `policies.json` | border exponent -25 % | border culture cost -25 % | `Policies.json` |
| tile gold price | kernel `world/territory.rs` | `(50 + 5 bought) x (100 + 100 d/3) %`, resource -100 %, rounded to 5 | `50 (d - 1) + 5 claimed`, then the purchase percents | `CityExpansionManager.kt` |
| unit upkeep | `derived/maintenance.rs` | `(0.5 paid (1 + 8p))^(1 + p/7)` | `(0.5 paid (1 + p))^(1 + p/3)`, 3 free units, progress capped at turn 500 (10 units at turn 300: 38 -> 12 gold) | `CivInfoStatsForNextTurn.kt` |
| religions per game | `world_size.rs` | table by map size 2..7 | `1 + major nations / 2` | `ReligionManager.kt`, `ModConstants.kt` |
| unhappiness effects | `derived/happiness.rs`, `classic/yields.rs` | -2 % per point (max -40 %) on production, gold, combat | below -10: production -50 %, strength -33 %, no gold effect; rebels from -21 | `GlobalUniques.json` |
| occupied city unhappiness | `derived/happiness.rs` | 5 per city | 5 plus a flat 2 | `CityStats.kt` |
| city-count unhappiness by map size | `world_size.rs` | Large 80 %, Huge 60 % | 100 % everywhere | `CityStats.kt` |
| golden age threshold | `systems/golden_age.rs` | +1 % per city beyond the first, rounded to 5 | +1 % per city including the first, truncated (1 / 4 / 8 cities: 500 / 515 / 535 -> 505 / 520 / 540) | `GoldenAgeManager.kt` |
| great general threshold | `combat/experience.rs` | 200, 300, 400 ... | 200 + 50 per one born | `GreatPersonManager.kt` |
| specialist yields | `great_people.json` | artist, writer, musician culture 3; merchant gold 2 | culture 2; merchant gold 3 | `Specialists.json` |
| largest lake | kernel `world/map.rs`, render `biomes.rs` | 9 | 10 | `ModConstants.kt` |
| captured city population | `systems/combat/city.rs` | keeps 50 % | loses `1 + pop/4` | `CityConquestFunctions.kt` |
| capture gold | `systems/plunder.rs` | 20 + 15 per citizen + two rolls (25, 50), whole-step percents | 20 + 10 per citizen + one roll below 40, exact percents | `CityConquestFunctions.kt` |
| bonus against barbarians | `combat/strength.rs` | 40, majors only | 75 / 50 / 40 / 33 / 25 / 20 / 10 / 0 by game level (Prince 33), every non-barbarian | `Difficulties.json`, `BattleDamage.kt` |
| floor of the modifier sum | `combat/strength.rs` | -90 | none; strength at least 1.0 | `BattleDamage.kt` |
| flanking | `combat/strength.rs` | any warring unit, attack and defence | own melee units, melee attack only | `BattleDamage.kt` |
| missing strategic resource | `derived/resources.rs` | up to -50 by shortfall | flat -25, none for barbarians | `BattleConstants.kt` |
| barbarian camps | `systems/barbarians.rs` | 27 fog tiles per camp; from round 15; 4 tiles from any city; delay 2 + rand 4 | map tiles ^ 0.4; camps from turn 1, wanderers from round 10; 4 tiles from major capitals; delay 8 + rand 5 (halved when raging, plus 8 / 5 / 3 / 0 by level) | `BarbarianManager.kt`, `BarbarianEncampment.kt` |
| ruins | `systems/ruins.rs` | culture 40, radius 5 | culture 20, radius 4 | `Ruins.json` |
| damage rounding | `combat/damage.rs` | down, at least 1; city strike at least 10 | to nearest, no minimum | `BattleDamage.kt` |
| wounded air units | `combat/damage.rs` | 1 % per missing hit point | 1 % per 3 missing hit points | `BattleDamage.kt` |
| interceptor experience | `systems/combat/air.rs` | 2 always | 2 only on a hit | `AirInterception.kt` |
| city healing | `combat/city.rs` | 20 + 1 capital + defence / 500 | flat 20 | `CityTurnManager.kt` |
| city strike strength | `combat/city.rs` | (strength - walls) x 75 % | strength x 75 % | `CityCombatant.kt` |
| score weights | `systems/victory.rs` | city 8, citizen 4, wonder 25 | 10 / 3 / 40 | `Civilization.kt`, `ModConstants.kt` |
| friendship and denunciation length | `systems/diplomacy/mod.rs` | 50 | 30 | `DiplomacyManager.kt` |
| protector resting point | `systems/city_states/mod.rs` | +5 | +10 | `DiplomacyManager.kt` |
| withdrawing a protection pledge | `city_states/gifts.rs` | free (the old rule never fired) | -20 influence and no new pledge for 20 rounds | `CityStateFunctions.kt` |
| Great Merchant trade mission | `units.ron` | 300 + 100 x era | 350 + 50 x era | `UnitActionsGreatPerson.kt` |
| shared religion with a city-state | `city_states/turn.rs` | none | influence decays 25 % slower, recovers 50 % faster | `DiplomacyManager.kt` |
| gold gift and tribute | `city_states/gifts.rs` | end round 500, whole steps; tribute 50 + 350 x turn / 500 | end round 400, one rounding down to 5; tribute 50 + 5 per 6.5 turns | `CityStateFunctions.kt`, `Speeds.json` |
| intimidation | `city_states/gifts.rs` | 75 / 125 (>=), Militaristic -10 | 100 / 100 (>), radius width / 10 within 5..10, no Militaristic term | `ModConstants.kt` |
| Religious city-state faith | `city_states.json` | 2 / 4 / 8 (ally 4 / 8 / 16) | 3 / 6 / 13 (ally 6 / 12 / 26); 4 faith on meeting one | `CityStateTypes.json` |
| espionage | `systems/espionage.rs` | no thief-rank term; coup `100 - gap x factor`; election by rank squared | detection `0..299 + 30 x defender rank - 30 x thief rank`, 200 and above kills; coup `50 - gap/2 + skill difference/2` (cap 85); election influence/2 + 30 x (rank + 1) | `Spy.kt`, `CityStateFunctions.kt` |
| city-state quests | `city_states/quests.rs` | camp range 12; Contest Technologies weight for Religious 1 % | 8; 50 % | `QuestTargetHelpers.kt`, `Quests.json` |
| World Leader threshold | `systems/congress.rs` | log formula | 67 % / 1.1 / 28 / 35 % scheme (16 / 28 / 35 / 40 delegates: 9 / 11 / 17 / 23 votes) | `VictoryManager.kt` |
| city-state intrusion | `city_states/turn.rs` | -6 per unit | -10 per round | `CityStateFunctions.kt` |
| Militaristic gifts | `city_states/turn.rs` | spawn bias 50 %, countdown always running | 100 %, countdown restarts at ally, first gift after the full interval | `DiplomacyTurnManager.kt` |
| buyout scrap gold | `city_states/buyout.rs` | 10 % of production | `(30 x cost)^0.75 / 20` | `BaseUnit.kt` |
| majority religion | `derived/religion.rs` | `2n >= population` | `n >= population / 2` (integer) | `CityReligionManager.kt` |
| building culture and some stats (BNW buildings keep their Great Work slots as well) | `buildings.json` | e.g. Opera House 1, Museum 1, Stadium happiness 2, Market gold 1 | Unciv's G&K stats (4, 5, 4, 2 ...) | `Buildings.json` |
| about 13 effect magnitudes of policies, beliefs, wonders, promotions and units | catalogs | see `RESOURCING.md` section 3 | Unciv's | `Policies.json`, `Beliefs.json`, `UnitPromotions.json`, `Units.json`, `Buildings.json` |

### Kept although Unciv has an equivalent

| Constant | Ours | Unciv | Reason |
|---|---|---|---|
| ruins gold and faith | 60 and 30 | random 50..100; a pantheon's price | the ruins dialog shows a fixed number |
| `WOUNDED_DAMAGE_PERCENT` | 33 | one third | equal up to the whole percent |
| `STANDARD_MAP_PLOTS` (score) | 4160 | 1276 (Unciv's medium map) | ours is the map-size scale of the standard map; Unciv's value would cut every map by 0.3 |
| policy cost and border culture discount floors | -75 % / -85 % | none (modifiers multiply) | Unciv multiplies the modifiers; ours are added together and floored |

| minimal city distance | 2 | 3 | the kernel constant `MIN_CITY_RANGE` is shared with Lands & Laws (its AI and its Colonies law settle at 2); adopting 3 for the classic module needs a per-module rule: not done |

### Own values: Unciv has no equivalent

Tourism, great works, archaeology, ideology tenets switch penalty, trade routes (ranges, speeds, gold), the World Congress (sessions, proposers, league projects), war-monger weights, deal limits and introduction rounds, double-agent catch percent, sapper bonus 50 and range 2, heavy charge 1.5, barbarian conversion gold 25, cleared-camp memory 30, sea barbarian spawn 1 in 6, exotic goods amounts, mountain move cost 3 and turn damage 50, ally science 25 %, ally luxury happiness 150 %, natural wonder happiness 1, minimum great-person golden age turns 3, league tier percents. They are listed with their source line `own value` in the generated `rules.md`.

### Not compared or not done in this pass

Nuclear damage, railroad speed, Unciv's shorter spawn delay for camps that already spawned units (needs a camp counter in saves), the barbarian difficulty bonus for cities, zone of control, embarkation, stacking, unit gold purchase and the mutual-kill rule, purchase hurry rate and production overflow, border culture paid by city-states at 150 %, cities on different continents at distance 2, coup without an ally, "other city-states grow wary" after an attack, the Contest Culture / Faith and Conquer City State quests (not implemented here), religion founding conditions and belief limits.

## Original names in comments and constants (2026-10-21)

The project owner, 2026-10-21: "All these things must be deleted! Rewrite all such places!" Comments in the rule crates labelled constants with the names the reference game gives its internal defines, tables and columns (UPPER_CASE defines, `Table.Column` pairs, "Civ V `Name`", `[verify]` markers that named them). The values came from an unrecorded origin, so the labels were false authority. What was done (no value, key, hash or behaviour changed):

- Every such comment in `crates/` was rewritten: the original name is gone, the constant is described in our own words (what it is, its unit), and the provenance is one of two honest clauses: "Unciv: <file>, equal" (only where the value was compared with the local Unciv checkout at commit `24765a9a`), or "own value, not compared with a public source (unverified)". The word `unverified` replaces the old `[verify]`.
- Where an identifier itself was a copy of a define name (about 70 constants, for example the policy-cost, border-tile, unhappiness, combat-strength, heal, plunder, capture and growth constants) it was renamed to a descriptive name of its own; the compiler checked the renames. Serialized keys and hashed names were not touched.
- `classic/docs/classic/rules.md` was rewritten the same way (see the section on it below).
- `classic/tools/sources-lint.sh` got two groups. G: the original's table and column citations and `[verify]` markers in prose (difficulty table, world-size, game-speed, era and resolution tables, "Civ V `Name`" labels). H: the list of the original's define names in `classic/tools/sources-lint.names` (whole words, one per line); a define name can no longer come back in a comment or document unnoticed. The baseline was lowered accordingly.

Measured with the same script before and after (comment lines in `crates/` and `apps/` that carry a backticked name that is not an identifier of our code, or one of the group G expressions) [measured by me]: 857 lines before (classic-rules 540, kernel 262, classic-content 32, other crates 23), 24 after: 6 quote the names of Unciv's own constants as the Unciv citation, 1 is a name of the importer's own notes table, 17 are own names in the `lal-*`, `sandbox-ui` and `i18n` crates and in test placeholders (none names the reference game). Per crate, before to after: classic-rules 540 to 5 (Unciv's names), kernel 262 to 1 (Unciv), classic-content 32 to 1, classic-ui 3 to 0, aurelia-qa 2 to 0, session-profile 1 to 0. `classic/docs/classic/rules.md` had 873 UPPER_CASE names not found in the code, 339 lines with SDK, XML or database wording and 76 `[verify]` markers; the lint groups A-H now find 0 lines in it. The counter takes a comment line that holds a backticked UPPER_CASE, CamelCase, `Table.Column` or `getXxx` name that does not occur as an identifier in the code of the same tree.

### Constants whose value differs from Unciv's public behaviour

**Resolved on 2026-10-21 by the Formula pass above** (the table that stood here listed the differences; each row was either adopted, kept with a stated reason, or has no Unciv equivalent: see the three tables of that section). Nothing is left in this heading.

### Constants checked equal to Unciv

Compared by reading the local Unciv checkout; the crate comments name the Unciv file next to each constant, and the generated `classic/docs/classic/rules.md` lists them with that source. Equal: the growth threshold; city and citizen unhappiness (3 and 1) and the -10 / -75 % thresholds; policy cost base, step, exponent and rounding; the known-technology discount 30 %; the item gold price form; road and railroad upkeep; city-connection gold; the pantheon and prophet prices; religious pressure, its range and the holy-city factor; golden age length 10, first threshold 500, step 250; deal and peace durations by speed; city-state friend 30, ally 60, minimum -60 and its resting points; heal rates 25 / 20 / 10 / 10; the damage formula (ratio, `24 + 12 random`), civilian damage 40, flanking 10, river -20, landing -50, fortification 20 per turn up to 2 turns; city strength constants; city hit points 200; embarked defence by era; the barbarian camp odds, spacing 7, nearby limit and camp gold 25; air capacity 6 and rebase range; the barbarian experience cap 30; capture gold base 20 and the 50-round cap; steal cost 125 %, +25 % per spy rank, 3 ranks; score for technologies and land; spaceship parts; quest rewards and durations.

### Left as is, on purpose

- Identifiers named `handicap` (types `HandicapValues`, `SourceHandicap`, `HandicapDefinition`, the module row, the file `handicaps.ron`, the content keys `core:handicap/<level>`) stay: the keys are saved and hashed. The word is the engine's term for a difficulty level; a rename to `difficulty` is a cross-crate change with a save-format bump and is proposed, not done.
- Enum variants and fields whose names look like the original's columns (`ExtraSpies`, `PressureRetention`, `PurchaseOnly`, `NoBadGoodies`, `UniqueLuxury*`, `LandBarbarianConversionPercent`, the `League*` family, and others listed by the rewrite) are written into content files and hash tags; renaming them moves the fingerprint, so they are listed for the project owner's decision.
- The `game_type` keys of the JSON catalogs (988 items) and the per-effect `source` strings (1213 effects) still carry the original's key and table-column names: they are data read by the importer; renaming them means regenerating the catalogs with the compiled output byte-identical.
- The frozen generator (`classic/docs/classic/generator/`) and the archived documents keep naming their inputs (baseline rows of groups A-H).
- Citations of CivFanatics and Carl's Guides remain in `classic/world_size.rs` and `derived/research.rs` (the old `rules.md` that also had them is replaced by a generated file); they are not Unciv (row 28 decision: remove after the re-sourcing).
- The title of the old `rules.md` read "Civ V Brave New World" (the generated file's does not), and assert messages in two test files say "BNW" in string literals.

## Licence note (Unciv, MPL-2.0)

Unciv is licensed under the Mozilla Public License 2.0 (GitHub reports `MPL-2.0` for the repository). I do not decide the licence of this project; this is what the MPL asks for, as I read it, and a lawyer should confirm [not legal advice].

- **Since 2026-10-21 data derived from Unciv is in the repository**: the catalogs `classic/docs/classic/*.json` (fields of 697 entries), the compiled `classic/assets/rules/classic/*.ron`, `handicaps.ron` and the name pools of `civilizations.ron` (see [UNCIV-ATTRIBUTION.md](UNCIV-ATTRIBUTION.md)). Each carries the notice 'Derived from Unciv (https://github.com/yairm210/Unciv), MPL-2.0'; `THIRD_PARTY_NOTICES.md` at the repository root lists Unciv, its commit and the files. Citing a file and recording that two numbers agree (the Kotlin comparisons in code comments) is not a copy.
- **If Unciv's JSON entries are copied or adapted** (for example when a catalog is rebuilt from `Units.json`), the copied file or the part of a file taken from Unciv becomes "Covered Software" and stays under MPL-2.0 (section 3.1). Then: keep Unciv's licence and copyright notices in that file (3.4), say in the file that it contains Unciv data, with the repository URL and the commit, give the licence text or its URL (3.1), and when the game is distributed in executable form tell recipients where the source of those files can be obtained (3.2). Other files of the project stay under the project's own terms as long as they are separate files (3.3, "Larger Work").
- **Where the notice should live:** a `THIRD_PARTY_NOTICES.md` at the repository root (it does not exist yet) listing Unciv, its URL, the pinned commit, the licence and the files that contain its data; plus a header comment in each copied file. A line in the in-game credits screen if the game ships with such data. Create these when the first copy lands, not before.
- Unciv's own data contains texts that come from the game (`quote`, `startIntroPart1`, encyclopedia texts). The MPL does not give us rights to those; do not copy them (see the plan above).

## Naming

Decision (the project owner, via the coordinator, 2026-10-20): the game is nowhere called "Civilization". Publicly it is described as "Unciv / Civ 5 inspired game with a similar rule set"; the product name is **Aurelia** (window title in `lands/app/desktop/src/main.rs`, settings and cache folders). Citing Unciv as the source stays allowed. The project owner then decided that the project's own identifiers do not carry the original's name either: **done on 2026-10-20 by `classic/tools/rename-classic.sh`** (mapping, rules and the instructions for other branches: [../rename-classic.md](../rename-classic.md)). This section lists where the words Civilization, Civ, Civ V and civ5 occurred, what was fixed, and what the rename changed. Git history is not touched. <!-- rename-classic:keep -->

Counts are from `git grep` at the commit after the fixes below, excluding `ru/notes/`, `output/` and `ru/benches/results/` unless stated [measured by me].

### Publicly visible places

| Place | Occurrences | State |
|---|---|---|
| Window titles | `lands/app/desktop/src/main.rs:246` "Aurelia"; `lands/app/desktop/src/visual_sandbox.rs:636` was "Civ · Low-poly visual sandbox" | **fixed**: "Aurelia · Low-poly visual sandbox". `with_title` is the only window-title call in `apps` and `crates` |
| Installers, desktop files, bundles, icons | none: `git ls-files` has no `.desktop`, `.plist`, `.msi`, `.wxs`, `.nsi`, `.dmg`, `AppImage`, `.ico`, `.icns` | nothing to rename. The settings and cache folders are already `Aurelia` (`client/src/shell/settings.rs`, `art_loader.rs`) |
| Save and replay file extensions | were `.civsave`, `.civlog` (77 lines in 39 files) | **renamed** by the script: `.aurelia`, `.aurlog`; the magic bytes `CIVSAVE`, `CIVLOG`, `CIVMODS` became `AURELIA`, `AURLOG`, `AURMODS` (8 bytes as before). Saves written before do not load any more (accepted). | <!-- rename-classic:keep -->
| Interface strings naming the product | none: no UI string contains "Civilization V", "Civ V" or "Civ 5" (regex over all string literals of `crates` and `apps`) | clean |
| Main-menu slogan | `platform/crates/client/src/shell/module.rs` `MenuCover::AURELIA.kicker` was "A STRATEGY OF CIVILIZATIONS" (found by the dist agent; the first regex looked for product names only) | **fixed**: "A STRATEGY GAME". The cover art `classic/assets/visual/generated/menu-v1.png` (1672 x 941) was looked at: it has no text, so nothing to repaint. Snapshot `classic/crates/classic-ui/tests/snapshots/classic-menu.snap` updated knowingly with `SNAPSHOT_UPDATE=1`: the slogan line, its box (150x13 became 93x13) and the accessibility label of the menu click area (it now fits more words: "... / Continue / New game ..."). No other snapshot contained the string [measured: `git grep`] |
| Interface strings with the in-game concept "civilization" (a nation) | 127 string literals in 63 files of `classic-ui`, `client`, `classic-rules`, `kernel` (labels, hints, Encyclopedia texts, refusal reasons, test messages) and the four screen labels | **fixed (the project owner, 2026-10-20)**: the word is "Nation" (as in Unciv) everywhere a player can read it: "Nations" screen and Encyclopedia category, "NATIONS MET", "Nation: <name>", and so on. Identifiers (`Screen::Civilizations`, `civilization` fields, `core:nation/...` keys, the art folder `civilizations/`, the CLI value `civilizations`) are not renamed. The L&L module keeps "faction"; the generic kernel refusal texts now say "nation" there too (three keys of `lal-ui/src/view/refusal.rs`, which maps kernel texts to Russian by exact text, were updated; its Russian wording (the word "civilization" in the genitive case) is unchanged, open). Snapshot updated knowingly: `classic/crates/classic-ui/tests/snapshots/classic-victory.snap` (3 texts, their boxes got narrower) |
| the old name of the encyclopedia screen | 7 player-visible strings: screen label (`ui/screens.rs`), F1 hotkey help and pointer help (`ui/hotkeys.rs`), `module.rs` reference button, `match_setup.rs` hint, `screen_overview.rs` hover, `pedia/concepts.rs` text; plus 7 labels of dev benches (`dev/headless.rs`, `dev/art_bench.rs`, not player-visible) | **fixed**: the 7 player-visible strings now say "Encyclopedia" (no test or snapshot contained them [measured: `grep` over `crates/*/tests`]). Identifiers (`Screen::Encyclopedia`, the shortcut id `encyclopedia`, env `AURELIA_PEDIA`, module `ui/pedia`) and the dev bench labels stay |
| Generated ruleset headers | first line of the 20 generated `classic/assets/rules/classic/*.ron` said "Civilization V: Brave New World ruleset" (importer constant `HEADER`) | **fixed**: "Ruleset in the style of Civ V Brave New World"; comment-only, the RON data is unchanged |
| Top-level docs | `ru/PLAN.en.md` title and goal sentence ("full Civilization", "Civilization V", "Civilization VI"); `classic/docs/classic/README.md` title and target line; `classic/docs/classic/rules.md` title; `platform/project.md` lines 7 and 1444; `lands/own-game-docs/README.md` line 34 | **fixed**: now "Civ V" / "Civ VI" or the neutral title |
| Docs left as they are | `lands/own-game-docs/DECISIONS.md` 39 lines (the criterion "similar to Civilization", decision texts), `FULL_GAME_PLAN.md` 3, `living-world/*.md` 5, `edition2.md` 1, `classic/docs/classic/rules.md` 2 part introductions (RESOURCE, rewritten with the file), `docs/archive/*` 2 | **not changed now**: `DECISIONS.md` is appended to by other agents (a mass edit would conflict); archived files are frozen. Mechanical fix: "Civilization" -> "Civ V" in these files, 1 commit |
| `README` at the repository root | none exists; `docs/README.md` is the index and says "Civ V: rules and parity" | fine; becomes the public README later |
| Comments naming the product ("as in Civilization", "Civilization's banners", "Civilization V's scout") | 25 lines in 19 files of `crates` and `apps`, mostly `lal-ui`, `lal-rules`, `client/shell/hud`, `lal-content/data/pack/*.ron` | **not changed now** (comments; other agents edit these files). Mechanical fix: "Civilization" -> "Civ V", 1 commit |
| Remaining "Civ V", "Civ 5" in crates and tools | comments, doc comments, test names, strings in tests: they name the reference game's rules, which the public description allows | keep, except where a rule is cited; the citations are the work of the re-sourcing plan. The script turned the prose word `Civ5` into `Civ V`; the identifier `Civ5` is now `Classic` | <!-- rename-classic:keep -->

### Internal identifiers (renamed by `classic/tools/rename-classic.sh`)

| Identifier | Now | Note |
|---|---|---|
| Crates `civ5-rules`, `civ5-content`, `civ5-ui`, `civ-qa` | `classic-rules`, `classic-content`, `classic-ui`, `aurelia-qa` | directories, packages, path deps, `use`, `Cargo.lock`, `platform/tools/platform-lint.sh` (matches `classic-[a-z-]+`) | <!-- rename-classic:keep -->
| Rust types and modules `Civ5*`, module `civ5` | `Classic*`, module `classic` | `Rules::Classic`, `ClassicUi`, ... | <!-- rename-classic:keep -->
| Ruleset (module) id `civ5` | `classic` | enters the rules fingerprint and every pinned game hash: re-pinned once, see the re-pin commit | <!-- rename-classic:keep -->
| Content keys `core:civ/<x>` | `core:nation/<x>` | data, docs, tests, snapshots; enters the ruleset fingerprint | <!-- rename-classic:keep -->
| Environment variables `CIV_*` | `AURELIA_*` (`CIV_RULES=civ5|lal` -> `AURELIA_RULES=classic|lands`, `CIV_LAL_MODE` -> `AURELIA_LANDS_MODE`) | no fallback to the old names | <!-- rename-classic:keep -->
| Save and replay files | `.aurelia` / `AURELIA`, `.aurlog` / `AURLOG`, slot tag `AURMODS` | old local saves stop loading | <!-- rename-classic:keep -->
| Rule-set files named after the original | `classic/docs/classic/`, `classic/assets/rules/classic/`, `bnw_*` -> `classic_*`, `civ5*` file names | | <!-- rename-classic:keep -->
| Git remote and repository name `civ-visual-sandbox`, branch names, local folders `~/civ` | unchanged | the public URL of the repository: The project owner's decision; the split plan names the new repository `aurelia-lands`; `lal-*` crates keep their names | <!-- rename-classic:keep -->

Kept on purpose and checked by `sh classic/tools/rename-classic.sh --check` (the list `PROTECT` at the top of the script): the original game's own file and table names (`CIV5*` tables, `Civ5Pkg`, `Data/Civ5/BNW` of the wiki dump), URLs, the names of our past branches (`civ5/unit-quality`), other people's local paths, `fandom-bnw-dump`, the atlas file names `units-bnw-*`. <!-- rename-classic:keep -->

## Guard

`sh classic/tools/sources-lint.sh` (about a second; no build needed) scans `docs`, `classic/assets/rules`, `crates`, `tools`, `apps`, `lands/own-game-docs` and the root documents for eight pattern groups: A SDK class and file names (`\bCv[A-Z][A-Za-z]{3,}`), B game XML and Lua file names and the constants table, C third-party copies of the game files, D the database copy on a wiki, E "DLL", "SDK", decompiled code and the vendor, F database dump wording, G the original's table, column and define citations in prose (its difficulty, world-size, game-speed, era and resolution tables, "Civ V `Name`" labels, `[verify]` markers), H the original's define names, whole words, listed one per line in `classic/tools/sources-lint.names` (a new place that needs an original name goes here first, with the reason). The exact expressions are at the top of the script and are the ones used in the "Counts" section above.

- `sh classic/tools/rename-classic.sh --check` is the naming guard (classic/docs/rename-classic.md): it fails when the old names of the project's own identifiers (see the table above) come back. Its allowlist is the `PROTECT` list in the script and the marker `rename-classic:keep`.
- It compares the number of matching lines per file and group with the ceilings in `classic/tools/sources-lint.baseline` (the places that still name the original: the frozen generator, three single lines of other documents, one test message). A file that is not in the baseline, a new group in an old file, or a count above its ceiling fails the check (exit 1) and names the file.
- Allowlist (never scanned): this file, the script, its baseline, `classic/tools/unciv-coverage.py` and `classic/tools/unciv-formulas.py`. Agent logs and histories (`ru/notes/`, `output/`, `ru/benches/results/`) are outside the scope.
- A re-sourced file lowers its ceiling: the script prints `lower ...` lines on stderr, then run `sh classic/tools/sources-lint.sh --print` and put the output under the header comment of the baseline. Never raise a ceiling; a place that really must name the original goes here first, with the reason.
- It is listed in `platform/docs/quality.md` next to `platform/tools/platform-lint.sh`. It is not part of `cargo test`.

Counts after this branch (same commands and scope as in "Counts", except that this file, the script, its baseline and the two Unciv tools are not counted) [measured by me, after the formulas cleanup]:

| Pattern | Lines before | Lines after | Files before | Files after |
|---|---:|---:|---:|---:|
| A SDK names | 363 | 163 | 64 | 2 |
| B game XML and Lua names | 70 | 16 | 18 | 6 |
| C third-party copies | 8 | 4 | 5 | 4 |
| D database copy | 42 | 7 | 11 | 6 |
| E DLL, SDK, vendor | 96 | 64 | 25 | 7 |
| F dump wording | 27 | 7 | 9 | 4 |
| any | **542** | **242** | **100** | **13** |

Of the 242 lines left: `classic/docs/classic/rules.md` 214 (RESOURCE, the whole file), the frozen generator 24, four lines in other documents (the two statements of the rule in `lands/own-game-docs/`, two in the frozen archive). `crates/`, `apps/` and the JSON catalogs have none. (An intermediate state, after the re-citing commits and before the formulas cleanup, was 451 lines in 80 files.)

## Decisions and deferred tasks

Answered by the project owner on 2026-10-20 (via the coordinator):

1. **Wiki and forum references** (row 28: Civilization Wiki, CivFanatics, Carl's Guides): to be removed **after** the re-sourcing; for now they are only recorded here. New texts (docs, comments, commits) cite only Unciv. Where Unciv's own source cites a forum thread (for instance the growth, research and policy-cost formulas), that is Unciv's citation; this project cites the Unciv file.
2. **Generator `classic/docs/classic/generator/`** (row 23) and the third-party copies it downloads from: delete **after** the re-sourcing. Deferred task, conditions: (a) every catalog row of rows 1-15 has a Unciv citation or is flagged `unverified-legacy` with a reason; (b) the importer reads only `classic/docs/classic/*.json`, so nothing needs the generator (true today); (c) `sh classic/tools/sources-lint.sh --print` shows no row for `classic/docs/classic/generator/` left after the deletion; (d) the same commit removes the generator rows from `classic/tools/sources-lint.baseline` and the S1-S5 paragraph above. Until then it stays frozen.
3. **Wiki and forum links in the documents** (the old `rules.md` is gone with row 20) are removed under the same conditions.
4. **Nation** instead of "civilization" in player-visible text: done (Naming section). L&L keeps "faction".
5. **Unciv's values and the 144 COPY constants**: see "Decisions of the project owner" in "Formulas and tables".

Still open: Unciv has no BNW rule set (see above). BNW-only content is our own design. Update 2026-10-21: 207 of the 303 BNW and own entries (all units, unique units, buildings, unique buildings, resources, improvements, natural wonders, wonders and projects, policies and tenets, beliefs, nations, city-states, The Internet; 39 of 135 promotions) were checked by hand and carry `provenance: own value` (322 confirmed effects); 14 promotions cite Unciv entries of another name; the record is `classic/tools/own-overrides.json`, the counts are in [RESOURCING.md](RESOURCING.md) section 3. The "own value, unverified" remarks of the inventory rows above are superseded by that for the entries the file lists.

## Own design: the expansion-only systems (2026-10-22)

Unciv has no tourism, cultural influence, great works, ideologies, trade routes, World Congress (only its United Nations
vote) or archaeology. For these systems the functions and numbers are our own design: the same kind of play, our own
shapes and constants. Every constant carries the doc-comment phrase `Own design`; the generated `rules.md` lists it as
`own design`; the data rows are in `classic/tools/own-design.json` (applied by `classic/tools/apply-own-design.py`);
the reasoning of every function is in [BNW-DESIGN.md](BNW-DESIGN.md) and the inventory of what was found and changed, with
the class of each item before and now, is [BNW-AUDIT.md](BNW-AUDIT.md). The earlier pass of 2026-10-22 (small shifts of
tourism and trade constants) is superseded by that work.

Result (100 rules, constants and formulas, [measured by me], counted in BNW-AUDIT.md): before 9 from Unciv, 18 own and
reasoned, 66 suspected memory or unexplained, 7 missing or never applied; now 11 from Unciv, 89 own design with a reason,
0 and 0. Catalog rows of the expansion's content tables (units, nations, beliefs, promotions: 226 rows) are listed there as a
known gap.
