"""Model part "classic": the models only the classic module draws and its content keys ("core:...").
Interface: platform/tools/blender/catalogs.py. Run `sh classic/tools/blender/export_all.sh` (needs Blender)
to write classic/assets/models/ (GLB files and manifest.json); `python3 -I platform/tools/blender/
check_catalogs.py` checks the catalogs without Blender.

    units/catalog.py   class models and unique variants (the platform's builders and shared looks)
    world/*.py         improvements, resources, natural wonders, world wonders, city landmarks
    world/keys.py      content key -> world model (the models are the part's own or the platform's)
    world/city_kit.py  city kit landmarks of the buildings

The coverage checks read classic/docs/classic/*.json: every unit, improvement, resource, natural wonder
and wonder there has a model or an explicit exemption.
"""
import json

import parts
from catalog import EMBARKED_VESSEL

OWNER = "classic"
NAMESPACE = "core"
DOCS = parts.docs_dir(OWNER) / "classic"
# The authoring modules of world/, each with a models() function.
WORLD_MODULES = ("improvements", "resources", "natural_wonders", "wonders_early", "wonders_late", "city_pieces")


def _units():
    return parts.load(OWNER, "units/catalog")


def unit_class_models():
    return _units().class_models()


def unit_unique_models(classes):
    return _units().unique_models(classes)


def slug(key):
    return key.rsplit("/", 1)[1]


def items(name):
    return json.loads((DOCS / f"{name}.json").read_text())["items"]


def unit_keys(classes, uniques):
    """(classes map, units map, fallbacks) for every unit in classic/docs/classic. `classes` and `uniques`: the
    class models and variants classic can see (the platform's and its own), by slug."""
    units, unique_units = items("units"), items("unique_units")
    class_map, unit_map, fallbacks = {}, {}, {}
    for unit in units:
        cls = unit["class"]
        model = classes.get(slug(cls))
        if model is None:
            fallbacks[cls] = "unit/unknown"
            continue
        class_map[cls] = model.id
        # A regular unit sharing a class (Foreign Legion) may have its own variant.
        variant = uniques.get(slug(unit["id"]))
        unit_map[unit["id"]] = variant.id if variant is not None else model.id
    for unit in unique_units:
        variant = uniques.get(slug(unit["id"]))
        if variant is not None:
            unit_map[unit["id"]] = variant.id
            continue
        base = class_map.get(unit["class"])
        if base is None:
            fallbacks[unit["id"]] = "unit/unknown"
        else:
            fallbacks[unit["id"]] = base
    # The boat a land unit is drawn as while it is afloat: one binding per rules module
    # (platform/crates/render/src/unit_models.rs: EMBARKED_KEY_CORE, lal_ui::art).
    unit_map["core:embarked"] = EMBARKED_VESSEL
    return class_map, unit_map, fallbacks


def world_models():
    models = []
    for name in WORLD_MODULES:
        models += parts.load(OWNER, f"world/{name}").models()
    return models


def _keys():
    return parts.load(OWNER, "world/keys")


def world_keys():
    return _keys().KEYS


def world_icon_only():
    return _keys().ICON_ONLY


def world_no_map_presence():
    return _keys().NO_MAP_PRESENCE


def world_landmarks():
    return parts.load(OWNER, "world/city_kit").LANDMARKS


def world_problems(models, keys, landmarks):
    """Content of classic/docs/classic with neither a model nor an explicit exemption. `models`: world models
    by id (the platform's and classic's), `keys`: classic's keys, `landmarks`: classic's."""
    problems = []
    plain = {k.split("@", 1)[0] for k in keys}
    icon_only, no_map_presence = world_icon_only(), world_no_map_presence()

    def need(key, kind, what):
        model = keys.get(key)
        if key not in plain:
            problems.append(f"{what} {key} has no model")
        elif model is not None and not model.startswith(kind + "/"):
            problems.append(f"{what} {key} drawn by {model}, expected a {kind} model")

    for item in items("improvements"):
        need(item["id"], "improvement", "improvement")
    for item in items("resources"):
        if item["id"] not in icon_only:
            need(item["id"], "resource", "resource")
    for item in items("natural_wonders"):
        need(item["id"], "natural-wonder", "natural wonder")
    for item in items("wonders"):
        if item["kind"] == "world_wonder":
            need(item["id"], "wonder", "world wonder")
        elif item["kind"] == "national_wonder":
            if item["id"] not in landmarks:
                problems.append(f"national wonder {item['id']} has no city landmark")
        elif item["id"] not in no_map_presence:
            problems.append(f"{item['kind']} {item['id']} is neither drawn nor listed in NO_MAP_PRESENCE")
    for key in icon_only:
        if key in plain:
            problems.append(f"{key} is icon-only but also has a model")
    return problems
