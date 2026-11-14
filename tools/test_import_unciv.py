#!/usr/bin/env python3
"""Tests of import-unciv.py that need no Unciv checkout (python3 -I classic/tools/test_import_unciv.py).

The importer once died on its first run after the catalogs moved: own-overrides.json named an effect of
three technologies that the catalog no longer has (IndexError in `own_overrides`).
"""
import glob
import importlib.util
import json
import os
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))  # the repository root: classic/tools/ -> classic/ -> root


def load_importer():
    spec = importlib.util.spec_from_file_location("import_unciv", os.path.join(HERE, "import-unciv.py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def catalog_entries():
    entries = {}
    for path in glob.glob(os.path.join(HERE, "..", "docs", "classic", "*.json")):
        with open(path, encoding="utf-8") as fh:
            data = json.load(fh)
        if not isinstance(data, dict):
            continue
        for lst in data.values():
            if isinstance(lst, list):
                for it in lst:
                    if isinstance(it, dict) and "id" in it:
                        entries[it["id"]] = it
    return entries


class OwnOverrides(unittest.TestCase):
    def test_every_override_names_an_entry_and_effects_it_has(self):
        with open(os.path.join(HERE, "own-overrides.json"), encoding="utf-8") as fh:
            overrides = json.load(fh)["entries"]
        entries = catalog_entries()
        stale = []
        for key, o in overrides.items():
            if key not in entries:
                stale.append(f"{key}: no such entry")
                continue
            count = len(entries[key].get("effects", []))
            named = [i for i in (o.get("effects") if isinstance(o.get("effects"), list) else []) if isinstance(i, int)]
            named += [int(i) for i in o.get("effect_set", {})]
            stale += [f"{key}: effect {i} of {count}" for i in named if not 0 <= i < count]
        self.assertEqual(stale, [], "overrides of effects the catalog does not have")

    def test_a_stale_override_is_a_clear_error_not_an_index_error(self):
        importer = load_importer()
        cats = {"techs": {"items": [{"id": "core:tech/x", "provenance": "unciv: Techs.json, X", "effects": []}]}}
        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, "own-overrides.json")
            with open(path, "w", encoding="utf-8") as fh:
                json.dump({"entries": {"core:tech/x": {"effects_only": True, "effects": [0]}}}, fh)
            importer.OVERRIDES = path
            with self.assertRaises(SystemExit) as raised:
                importer.own_overrides(cats)
        self.assertIn("core:tech/x", str(raised.exception))
        self.assertIn("effect 0", str(raised.exception))

    def test_the_overrides_apply_to_the_committed_catalogs(self):
        importer = load_importer()
        importer.OVERRIDES = os.path.join(HERE, "own-overrides.json")
        cats = {}
        for path in glob.glob(os.path.join(HERE, "..", "docs", "classic", "*.json")):
            with open(path, encoding="utf-8") as fh:
                data = json.load(fh)
            if isinstance(data, dict):
                cats[os.path.basename(path)[:-5]] = data
        importer.own_overrides(cats)  # raises when an override does not fit


if __name__ == "__main__":
    unittest.main()
