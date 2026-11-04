"""Landmarks of the classic buildings on the city tile: building key ("core:building/<slug>") -> the
piece of the city kit that stands for it, its slot kind, its priority and the building it hides
(`supersedes`). The kit itself (styles, centers, houses, slots, composition rules) is the platform's
(platform/tools/blender/world/city_kit.py); the pieces are modeled in city_pieces.py here (classic-only)
and in the platform (shared with lands).
"""
from city_kit import _landmark


NATIONAL = ["heroic_epic", "national_college", "national_epic", "circus_maximus", "east_india_company", "ironworks",
            "oxford_university", "hermitage", "national_intelligence_agency", "grand_temple",
            "national_visitor_center"]

LANDMARKS = {
    "core:building/walls": _landmark("city/walls", "ring", 0),
    "core:building/palace": _landmark("city/palace", "inner", 1),
    "core:building/castle": _landmark("city/castle", "inner", 2),
    "core:building/harbor": _landmark("city/harbor", "water", 3),
    "core:building/seaport": _landmark("city/seaport", "water", 3, "core:building/harbor"),
    "core:building/factory": _landmark("city/factory", "outer", 5),
    "core:building/cathedral": _landmark("city/cathedral", "inner", 6),
    "core:building/mosque": _landmark("city/mosque", "inner", 6),
    "core:building/pagoda": _landmark("city/pagoda", "inner", 6),
    "core:building/temple": _landmark("city/temple", "inner", 7),
    "core:building/university": _landmark("city/university", "outer", 8),
    "core:building/colosseum": _landmark("city/arena", "outer", 9),
    "core:building/stadium": _landmark("city/stadium", "outer", 9, "core:building/colosseum"),
    "core:building/military_base": _landmark("city/military-base", "outer", 10),
    "core:building/broadcast_tower": _landmark("city/broadcast-tower", "outer", 11),
    "core:building/airport": _landmark("city/airport", "outer", 12),
    "core:building/lighthouse": _landmark("city/lighthouse", "water", 13),
    "core:building/monument": _landmark("city/monument", "inner", 14),
    "core:building/market": _landmark("city/market", "inner", 15),
    "core:building/granary": _landmark("city/granary", "outer", 16),
}
for _slug in NATIONAL:
    LANDMARKS["core:building/" + _slug] = _landmark("city/national-" + _slug.replace("_", "-"), "outer", 4)
