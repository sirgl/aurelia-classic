# Sourcing policy: nothing is taken from the original game

**No content of the original game is used. Not its numbers, formulas, tables, texts, pictures, sounds, data files or orderings, and not "from memory" either. The only external source of rules data is Unciv (open source, MPL-2.0, attribution kept). Everything else is common knowledge, described in words, or our own design.** Decision of the project owner, 25 October 2026. It binds every contributor, every agent and every tool of this repository.

## 1. The three allowed kinds of source

| Kind | What it may contain | Record |
|---|---|---|
| **Unciv** | numbers, formulas, tables, rule structure taken from Unciv's data and code (open source, Mozilla Public License 2.0) | the entry names the file and the line (`Unciv: <file>`); attribution in [UNCIV-ATTRIBUTION.md](UNCIV-ATTRIBUTION.md) and `NOTICE` |
| **Common knowledge** | what anyone who knows the genre or history knows, stated **in words, without numbers**: that there are nations and which (Rome, Egypt, ...), who their well-known leaders are, which kinds of improvement exist (farm, mine, road, fort), what a kind of building is for (a library helps science, walls help defence), what a kind of unit is (archer, catapult, cavalry), the general effect of a nation's trait ("sea raiders", "strong early army") | the entry says `common knowledge` and what the statement is |
| **Own design** | everything else: every number, formula, table, cost curve, ordering and text that is not in Unciv | the entry says `own design` and why this shape (what it is meant to do in play), in `RESOURCING.md` / `SOURCES.md` |

## 2. What is forbidden, with no exception

1. **Any number from the original game**: costs, strengths, yields, times, thresholds, percentages, probabilities, level tables, era costs, upkeep, happiness values, culture curves, combat constants, AI constants.
2. **Any formula or table of the original**, including those that are "well known" in the community (damage, cost-growth, culture, happiness formulas, unit and building stat tables). If a formula is also in Unciv, take it from Unciv and cite the Unciv file; if it is not in Unciv, design our own.
3. **Anything "from memory" of the original.** Memory of the original game is not a source. A value that someone remembers is treated as copied.
4. **Its game files and anything extracted from them** (XML/JSON tables, text files, the in-game encyclopedia, localisation files), and third-party databases, wikis, guides and fan sites that reproduce its numbers or texts, and mods derived from it.
5. **No asset of any kind from the original game: images, icons, symbols, emblems, portraits, maps, 3D models, animations, videos, music, sounds, voices, fonts, texts, descriptions, the names of its specific content taken as a set (wonders, policies, beliefs, technologies).** Nothing at all. All pictures, icons, sounds and music of Aurelia are our own work (generated or drawn by us) or free under a licence that allows it, and their origin is recorded (`NOTICE`, `THIRD-PARTY.md`, the art pack lists).
6. **Its structure copied wholesale** (the same list of N techs with the same prerequisites and costs, the same policy trees with the same effects): where Unciv has a structure, take Unciv's; where it has none, design our own.
7. Naming the original game or its publisher in the project's files, except in the README notices that the owner decided on (the legality notice copied from the Unciv README and the recommendation to buy the original); `sources-lint.sh` guards this.

## 3. How a value is recorded

Every rule constant, formula, table row and AI constant has one of exactly four records:

- `Unciv: <file>[, line]` (equal or adapted, said which);
- `own design: <reason>`;
- `common knowledge: <statement>` (qualitative, no numbers);
- `unverified`, which is **not allowed at a release**: it is a debt that must become one of the three above before publication.

The generated reference [rules.md](rules.md) shows the record of each constant; RESOURCING.md holds the inventory and the plan; NAMING.md the names. A new constant without a record is a defect.

## 4. Release gate

Before any publication the audit `sh classic/tools/sources-audit.sh --strict` must report **zero** rows with the record `own value (source not recorded)` or `unverified` in the constants table, and zero catalog items whose provenance is `own value` (each must become `unciv:`, `own design:` or `common knowledge:`). Without `--strict` it prints the counts (state on 25 October 2026: see section 6).

## 5. What to do when a value has no source

1. Look for it in Unciv's data and code; if found, take it and cite it.
2. Otherwise design our own: choose the shape (a curve, a cap, a per-era step), say in the record what it should do in play, add a test or a bench check that the number is in a playable range.
3. Never "fix" a doubt by recalling the original's number.

## 6. State on 25 October 2026 [measured by `sources-audit.sh`]

See the output of `sh classic/tools/sources-audit.sh`. At the time of writing the constants table has about 355 rows with an unrecorded source (most of them constants of our own AI and engine, which are own design and only lack the record), and the content catalogs overlay Unciv fields on a skeleton that came from the original game's data files (techs 80 of 81 overlaid by field, units 74 of 82, buildings 55 of 62 and so on, see `provenance` and `unciv_fields` in each file; 243 catalog items carry the provenance `own value`): the skeleton fields that Unciv does not cover are the remaining debt and are to be replaced by Unciv data or by our own, then recorded. This is the first task of the re-sourcing that follows this policy.
