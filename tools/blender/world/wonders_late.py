"""World wonders of BNW, renaissance to information (classic/docs/classic/wonders.json, kind world_wonder).
One landmark model each, drawn on a tile of the owning city's ring (wonders.py).

Every wonder stands on a ground slab (hex of radius about 0.86, z -0.04..0.015) and fills its
hex; the front (-y) shows the characteristic side. Small accessories are LOD0 only, LOD2 keeps
the silhouette in a few big solids.

Ownership split: the models only classic draws. The helpers shared with the platform's models
are imported from platform/tools/blender/world/wonders_late.py.
"""
import math

from geom import move, rot_x, rot_y, rot_z, scale
import kit
from wonders import wonder as _model

from wonders_late import _box, _chhatri, _cone, _disc, _gantry, _ground, _poly, _wall, _windows


# Shared helpers ----------------------------------------------------------------------------
def _onion(m, c, r, mat, h=None, sides=None):
    """Onion dome: bulge wider than its base, pointed tip."""
    h = r * 2.1 if h is None else h
    sides = sides or m.pick(8, 6, 5)
    x, y, z = c
    if m.detail == 2:
        m.rings([((x, y, z), r * 0.75, r * 0.75), ((x, y, z + h * 0.35), r, r), ((x, y, z + h), 0.0, 0.0)], mat,
                sides=sides)
        return
    m.rings([((x, y, z), r * 0.72, r * 0.72), ((x, y, z + h * 0.28), r, r), ((x, y, z + h * 0.55), r * 0.74, r * 0.74),
             ((x, y, z + h * 0.8), r * 0.24, r * 0.24), ((x, y, z + h), 0.0, 0.0)], mat, sides=sides)


# Porcelain Tower ---------------------------------------------------------------------------
def porcelain_tower(m):
    _ground(m, "stone", 0.84)
    _box(m, 0.0, 0.06, 0.0, 0.92, 0.9, 0.05, "marble")
    if m.detail < 2:
        for s in (-1, 1):
            kit.house(m, (s * 0.43, 0.16, 0.04), 0.13, 0.44, 0.1, "lacquer", "jade", kind="hip", roof_h=0.06,
                      door=None)
        kit.house(m, (0.0, -0.33, 0.04), 0.3, 0.1, 0.07, "lacquer", "jade", kind="hip", roof_h=0.05, door=None)
    _pagoda(m, (0.0, 0.1, 0.05), m.pick(9, 9, 5), 0.27, 0.14, 1.4, "white", "jade")


def _pagoda(m, base, stories, r0, r1, height, body, eave):
    x, y, z = base
    sides = m.pick(8, 6, 5)
    kit.cylinder(m, (x, y, z - 0.01), r0 * 1.25, 0.05, "marble", sides=sides)
    z += 0.04
    gap = 0.022
    story_sum = height - stories * gap
    for i in range(stories):
        t = i / max(stories - 1, 1)
        r = r0 + (r1 - r0) * t
        h = story_sum / stories * (1.18 - 0.36 * t)
        kit.cylinder(m, (x, y, z), r, h, body, sides=sides)
        if m.detail == 0:
            _box(m, x, y - r * 0.93, z + h * 0.18, r * 0.42, 0.012, h * 0.5, "gold")
        z += h
        m.rings([((x, y, z - 0.01), r * 1.24, r * 1.24), ((x, y, z + 0.004), r * 1.24, r * 1.24),
                 ((x, y, z + 0.022), r * 0.92, r * 0.92)], eave, sides=sides)
        z += gap
    _cone(m, (x, y, z), r1 * 0.55, 0.16, "gold", sides=m.pick(6, 5, 4))
    if m.detail < 2:
        m.ball((x, y, z + 0.07), 0.035, "gold", u=m.pick(6, 4, 4), v=3)


# Himeji Castle -----------------------------------------------------------------------------
HIMEJI_MAIN = ((0.5, 0.38, 0.17, 0.04, 1), (0.42, 0.32, 0.15, 0.04, 2), (0.34, 0.26, 0.14, 0.04, 1),
               (0.27, 0.21, 0.13, 0.04, 0), (0.21, 0.17, 0.13, 0.08, 1))
HIMEJI_SMALL = ((0.26, 0.2, 0.12, 0.035, 1), (0.2, 0.16, 0.11, 0.035, 0), (0.15, 0.12, 0.1, 0.06, 1))


def himeji_castle(m):
    _ground(m, "grass", 0.86)
    if m.detail < 2:
        pts = [(-0.74, -0.05), (-0.46, -0.58), (0.46, -0.58), (0.74, -0.05)]
        for a, b in zip(pts, pts[1:]):
            _wall(m, a, b, 0.07, 0.04, "white", z0=0.0)
            dx, dy = b[0] - a[0], b[1] - a[1]
            kit.wedge_roof(m, ((a[0] + b[0]) / 2, (a[1] + b[1]) / 2, 0.07), math.hypot(dx, dy), 0.05, 0.03, "slate",
                           yaw=math.degrees(math.atan2(dy, dx)))
        for x, y in ((-0.55, 0.45), (0.6, 0.3), (0.5, -0.32), (-0.58, -0.25)):
            kit.tree(m, (x, y, 0.0), 0.2, "conifer", "leaf_dark", seed=int(x * 100))
    _keep(m, (-0.47, 0.3), (0.2, 0.17, 0.16), HIMEJI_SMALL)
    _keep(m, (0.08, 0.1), (0.36, 0.28, 0.24), HIMEJI_MAIN)


def _keep(m, pos, base, tiers):
    x, y = pos
    bx, by, bh = base
    m.taper((x, y, -0.03), (bx, by), (bx * 0.8, by * 0.8), bh + 0.03, "stone")
    z = bh
    for i, (w, d, h, roof_h, gables) in enumerate(tiers):
        _box(m, x, y, z - 0.02, w, d, h + 0.02, "white")
        z += h
        kit.hip_roof(m, (x, y, z), w, d, roof_h, "slate", overhang=0.03)
        if m.detail < 2 and gables:
            offsets = (0.0,) if gables == 1 else (-w * 0.24, w * 0.24)
            for ox in offsets:
                _gable(m, x + ox, y - d / 2 + 0.03, z, 0.13 if gables == 1 else 0.1, 0.12, roof_h + 0.035)
        z += 0.022 if i < len(tiers) - 1 else roof_h
    if m.detail == 0:
        for s in (-1, 1):
            _box(m, x + s * (tiers[-1][0] / 2 - 0.02), y, z - 0.012, 0.018, 0.018, 0.035, "gold")


def _gable(m, x, y, z, width, depth, h):
    """Japanese front gable (chidori-hafu): slate wedge with a white pediment, facing -y."""
    kit.wedge_roof(m, (x, y, z - 0.01), depth, width, h, "slate", yaw=90, overhang=0.012)
    if m.detail == 0:
        with m.at(move(x, y - depth / 2 - 0.016, z - 0.008)):
            m.prism([(-width / 2 + 0.018, 0.0), (width / 2 - 0.018, 0.0), (0.0, h - 0.022)], 0.006, "white")


# Sistine Chapel ----------------------------------------------------------------------------
def sistine_chapel(m):
    _ground(m, "stone", 0.86)
    # The chapel: a tall plain brick box with merlons on top.
    cx, cy, w, d, h = -0.22, -0.2, 0.74, 0.27, 0.56
    _box(m, cx, cy, 0.0, w, d, h, "brick")
    _box(m, cx, cy, h - 0.06, w + 0.02, d + 0.02, 0.025, "terracotta")
    kit.wedge_roof(m, (cx, cy, h), w - 0.04, d - 0.04, 0.05, "terracotta", overhang=0.0)
    if m.detail == 0:
        for i in range(10):
            t = (i + 0.5) / 10
            _box(m, cx - w / 2 + w * t, cy - d / 2 + 0.008, h - 0.035, 0.035, 0.03, 0.06, "brick")
            _box(m, cx - w / 2 + w * t, cy + d / 2 - 0.008, h - 0.035, 0.035, 0.03, 0.06, "brick")
        _windows(m, cx - w / 2, cx + w / 2, cy - d / 2 - 0.004, h * 0.55, 6, 0.04, 0.12, "dark")
        _windows(m, cx - w / 2 + 0.2, cx - w / 2 + 0.4, cy - d / 2 - 0.004, 0.0, 1, 0.06, 0.1, "wood_dark")
    if m.detail < 2:
        for x, y in ((-0.66, 0.25), (-0.5, 0.42), (0.7, -0.2)):
            kit.tree(m, (x, y, 0.0), 0.2, "cypress", "leaf_dark")
    _st_peters(m, 0.3, 0.32)


def _st_peters(m, x, y):
    _box(m, x, y, 0.0, 0.5, 0.42, 0.3, "cream")
    kit.wedge_roof(m, (x, y - 0.1, 0.3), 0.5, 0.18, 0.05, "terracotta", overhang=0.0)
    if m.detail == 0:
        for i in range(6):
            _box(m, x - 0.2 + 0.08 * i, y - 0.215, 0.0, 0.026, 0.016, 0.3, "white")
    z = 0.3
    sides = m.pick(12, 8, 6)
    kit.cylinder(m, (x, y + 0.04, z), 0.2, 0.15, "cream", sides=sides)
    if m.detail == 0:
        for px, py in kit.ring(12, 0.205, 15):
            _box(m, x + px, y + 0.04 + py, z, 0.022, 0.022, 0.15, "white")
    z += 0.15
    kit.cylinder(m, (x, y + 0.04, z), 0.215, 0.025, "white", sides=sides)
    kit.dome(m, (x, y + 0.04, z + 0.02), 0.2, "hull_grey", h=0.3, sides=sides, steps=m.pick(5, 3, 2))
    z += 0.3
    kit.cylinder(m, (x, y + 0.04, z), 0.045, 0.08, "cream", sides=m.pick(6, 5, 4))
    _cone(m, (x, y + 0.04, z + 0.08), 0.05, 0.08, "hull_grey", sides=m.pick(6, 5, 4))
    if m.detail < 2:
        m.ball((x, y + 0.04, z + 0.17), 0.02, "gold", u=4, v=3)


# Kremlin -----------------------------------------------------------------------------------
def kremlin(m):
    _ground(m, "grass", 0.86)
    ring = kit.ring(6, 0.74, 90)
    for i in range(6):
        _wall(m, ring[i], ring[(i + 1) % 6], 0.12, 0.05, "brick", step=0.12)
    for i, (x, y) in enumerate(ring):
        if i != 3:
            _kremlin_tower(m, x, y, 0.11, 0.2, 0.17)
    _spasskaya(m, ring[3][0], ring[3][1] + 0.03)
    if m.detail < 2:
        _box(m, -0.34, 0.34, 0.0, 0.34, 0.16, 0.15, "cream")
        kit.hip_roof(m, (-0.34, 0.34, 0.15), 0.34, 0.16, 0.06, "green")
    _bell_tower(m, 0.38, 0.2)
    _cathedral(m, -0.02, 0.0)


def _kremlin_tower(m, x, y, s, h, spire):
    _box(m, x, y, 0.0, s, s, h, "brick")
    if m.detail == 0:
        _box(m, x, y, h - 0.02, s * 1.12, s * 1.12, 0.025, "white")
    if m.detail < 2:
        m.taper((x, y, h), (s * 0.45, s * 0.45), (0.004, 0.004), spire, "green")
        m.octa((x, y, h + spire + 0.015), 0.018, "gem_red")


def _spasskaya(m, x, y):
    _box(m, x, y, 0.0, 0.17, 0.17, 0.32, "brick")
    _box(m, x, y, 0.32, 0.13, 0.13, 0.15, "brick")
    if m.detail < 2:
        _box(m, x, y, 0.3, 0.19, 0.19, 0.03, "white")
        _disc(m, (x, y - 0.067, 0.4), 0.045, 0.01, "white")
        _box(m, x, y - 0.074, 0.39, 0.006, 0.004, 0.04, "black")
    if m.detail == 0:
        for sx in (-1, 1):
            for sy in (-1, 1):
                _cone(m, (x + sx * 0.075, y + sy * 0.075, 0.32), 0.016, 0.08, "white", sides=4)
    m.taper((x, y, 0.47), (0.06, 0.06), (0.004, 0.004), 0.32, "green")
    if m.detail < 2:
        m.octa((x, y, 0.81), 0.03, "gem_red")


def _bell_tower(m, x, y):
    sides = m.pick(8, 6, 5)
    tiers = ((0.085, 0.3), (0.07, 0.2), (0.055, 0.16)) if m.detail < 2 else ((0.075, 0.66),)
    z = 0.0
    for r, h in tiers:
        kit.cylinder(m, (x, y, z), r, h, "white", sides=sides)
        z += h
        if m.detail == 0:
            kit.cylinder(m, (x, y, z - 0.012), r * 1.15, 0.016, "gold", sides=sides)
    if m.detail < 2:
        kit.cylinder(m, (x, y, z), 0.045, 0.04, "gold", sides=sides)
    _onion(m, (x, y, z + 0.035), 0.065, "gold", h=0.15)


def _cathedral(m, x, y):
    _box(m, x, y, 0.0, 0.4, 0.34, 0.24, "white")
    if m.detail == 0:
        _windows(m, x - 0.17, x + 0.17, y - 0.174, 0.08, 3, 0.05, 0.1, "dark")
        for i in range(3):
            _cone(m, (x - 0.133 + 0.133 * i, y - 0.17, 0.24), 0.066, 0.06, "white", sides=4)
    kit.cylinder(m, (x, y, 0.24), 0.07, 0.14, "white", sides=m.pick(8, 6, 5))
    _onion(m, (x, y, 0.38), 0.105, "gold", h=0.24)
    if m.detail < 2:
        colors = ("green", "roof_blue", "gem_red", "gold")
        for k, (sx, sy) in enumerate(((-1, -1), (1, -1), (1, 1), (-1, 1))):
            px, py = x + sx * 0.125, y + sy * 0.105
            kit.cylinder(m, (px, py, 0.24), 0.048, 0.09, "white", sides=m.pick(6, 5, 4))
            _onion(m, (px, py, 0.33), 0.072, colors[k], h=0.16)


# Forbidden Palace --------------------------------------------------------------------------
def forbidden_palace(m):
    _ground(m, "stone", 0.86)
    x0, x1, y0, y1 = -0.68, 0.68, -0.54, 0.52
    corners = [(x0, y0), (-0.2, y0), (0.2, y0), (x1, y0), (x1, y1), (x0, y1)]
    for a, b in ((corners[0], corners[1]), (corners[2], corners[3]), (corners[3], corners[4]),
                 (corners[4], corners[5]), (corners[5], corners[0])):
        _red_wall(m, a, b)
    _meridian_gate(m, 0.0, y0)
    for s in (-1, 1):
        _box(m, s * 0.48, -0.2, 0.0, 0.26, 0.2, 0.04, "marble")
        _box(m, s * 0.48, -0.2, 0.04, 0.22, 0.15, 0.1, "lacquer")
        kit.hip_roof(m, (s * 0.48, -0.2, 0.14), 0.22, 0.15, 0.07, "gold", overhang=0.035)
    _hall_of_harmony(m, 0.0, 0.14)


def _red_wall(m, a, b):
    dx, dy = b[0] - a[0], b[1] - a[1]
    _wall(m, a, b, 0.1, 0.04, "lacquer")
    if m.detail < 2:
        kit.wedge_roof(m, ((a[0] + b[0]) / 2, (a[1] + b[1]) / 2, 0.1), math.hypot(dx, dy), 0.04, 0.025, "gold",
                       yaw=math.degrees(math.atan2(dy, dx)))


def _meridian_gate(m, x, y):
    _box(m, x, y, 0.0, 0.44, 0.13, 0.14, "lacquer")
    if m.detail == 0:
        for ox in (-0.1, 0.0, 0.1):
            _box(m, x + ox, y - 0.064, 0.0, 0.05, 0.01, 0.08, "dark")
    _box(m, x, y, 0.14, 0.3, 0.1, 0.07, "lacquer")
    kit.hip_roof(m, (x, y, 0.21), 0.3, 0.1, 0.07, "gold", overhang=0.04)


def _hall_of_harmony(m, x, y):
    for i, (w, d) in enumerate(((0.9, 0.52), (0.8, 0.44), (0.7, 0.36))):
        _box(m, x, y, i * 0.04, w, d, 0.04, "marble")
        if m.detail == 0:
            _box(m, x, y - d / 2 + 0.004, i * 0.04 + 0.04, w, 0.008, 0.018, "white")
    m.taper((x, y - 0.3, 0.0), (0.06, 0.06), (0.06, 0.004), 0.12, "marble", shift=(0.0, 0.1))
    z = 0.12
    _box(m, x, y, z, 0.56, 0.28, 0.22, "lacquer")
    if m.detail == 0:
        _windows(m, x - 0.26, x + 0.26, y - 0.142, z + 0.02, 7, 0.045, 0.15, "gold", depth=0.006)
    z += 0.22
    kit.hip_roof(m, (x, y, z), 0.56, 0.28, 0.08, "gold", overhang=0.07)
    _box(m, x, y, z + 0.03, 0.42, 0.16, 0.1, "lacquer")
    kit.hip_roof(m, (x, y, z + 0.13), 0.44, 0.18, 0.18, "gold", overhang=0.07)


# Taj Mahal ---------------------------------------------------------------------------------
def taj_mahal(m):
    _ground(m, "grass", 0.86)
    _garden(m)
    y = 0.28
    _box(m, 0.0, y, 0.0, 0.72, 0.72, 0.07, "marble")
    oct_pts = [(0.22, -0.15), (0.22, 0.15), (0.15, 0.22), (-0.15, 0.22), (-0.22, 0.15), (-0.22, -0.15),
               (-0.15, -0.22), (0.15, -0.22)]
    _poly(m, [(px, py + y) for px, py in oct_pts], 0.06, 0.38, "marble")
    if m.detail < 2:
        _box(m, 0.0, y - 0.222, 0.08, 0.17, 0.012, 0.28, "cream")
        _box(m, 0.0, y - 0.226, 0.08, 0.09, 0.012, 0.2, "dark")
    if m.detail == 0:
        for s in (-1, 1):
            for z in (0.09, 0.24):
                _box(m, s * 0.13, y - 0.224, z, 0.04, 0.008, 0.1, "dark")
    for sx in (-1, 1):
        for sy in (-1, 1):
            if m.detail < 2:
                _chhatri(m, (sx * 0.15, y + sy * 0.15, 0.38), 0.04, 0.05)
            _minaret(m, sx * 0.33, y + sy * 0.33)
    kit.cylinder(m, (0.0, y, 0.38), 0.125, 0.07, "marble", sides=m.pick(10, 7, 5))
    _onion(m, (0.0, y, 0.44), 0.18, "marble", h=0.36, sides=m.pick(10, 7, 6))
    m.rod((0.0, y, 0.78), (0.0, y, 0.88), 0.01, 0.004, "gold", sides=3)


def _garden(m):
    _box(m, 0.0, -0.46, -0.03, 0.17, 0.62, 0.05, "sandstone")
    _box(m, 0.0, -0.46, -0.03, 0.09, 0.6, 0.058, "water")
    if m.detail < 2:
        _box(m, 0.0, -0.46, -0.03, 1.0, 0.08, 0.048, "sandstone")
        _box(m, 0.0, -0.46, -0.03, 0.98, 0.04, 0.054, "water")
        for s in (-1, 1):
            _box(m, s * 0.66, 0.28, 0.0, 0.14, 0.36, 0.13, "rock_red")
            for oy in (-0.1, 0.0, 0.1):
                kit.dome(m, (s * 0.66, 0.28 + oy, 0.13), 0.04, "marble", sides=5, steps=2)
    if m.detail == 0:
        for s in (-1, 1):
            for ty in (-0.72, -0.6, -0.34, -0.22):
                kit.tree(m, (s * 0.14, ty, 0.0), 0.14, "cypress", "leaf_dark")


def _minaret(m, x, y):
    kit.cylinder(m, (x, y, 0.06), 0.032, 0.5, "marble", sides=m.pick(6, 5, 4), r_top=0.024)
    if m.detail == 0:
        for z in (0.24, 0.4):
            kit.cylinder(m, (x, y, z), 0.042, 0.014, "marble", sides=6)
    _chhatri(m, (x, y, 0.56), 0.03, 0.035)


# Louvre ------------------------------------------------------------------------------------
def louvre(m):
    _ground(m, "stone", 0.86)
    _wing(m, 0.0, 0.45, 1.2, 0.18, 0.24, 0)
    for s in (-1, 1):
        _wing(m, s * 0.62, -0.02, 0.76, 0.18, 0.24, 90)
        _pavilion(m, s * 0.6, 0.43)
    _pavilion(m, 0.0, 0.45, dome=True)
    for s in (-1, 1):
        _poly(m, [(s * 0.06, -0.38), (s * 0.3, -0.38), (s * 0.3, -0.28), (s * 0.06, -0.28)], -0.03, 0.024, "water")
    _glass_pyramid(m, (0.0, -0.02), 0.24, 0.34)
    if m.detail < 2:
        for x, y in ((-0.38, -0.46), (0.38, -0.46), (0.0, 0.2)):
            _glass_pyramid(m, (x, y), 0.06, 0.08)


def _wing(m, x, y, length, depth, h, yaw):
    _box(m, x, y, 0.0, length, depth, h, "cream", yaw)
    kit.hip_roof(m, (x, y, h), length, depth, 0.07, "slate", yaw=yaw)
    if m.detail == 0:
        with m.at(move(x, y, 0.0), rot_z(yaw)):
            _windows(m, -length / 2 + 0.05, length / 2 - 0.05, -depth / 2 - 0.003, 0.06, 8, 0.03, 0.06, "glass_dark")
            _windows(m, -length / 2 + 0.05, length / 2 - 0.05, -depth / 2 - 0.003, 0.15, 8, 0.03, 0.06, "glass_dark")


def _pavilion(m, x, y, dome=False):
    s = 0.24
    _box(m, x, y, 0.0, s, s, 0.32, "cream")
    if dome:
        m.taper((x, y, 0.32), (s / 2 + 0.01, s / 2 + 0.01), (0.04, 0.04), 0.2, "slate")
        if m.detail < 2:
            _disc(m, (x, y - s / 2 - 0.004, 0.25), 0.04, 0.008, "white")
    else:
        m.taper((x, y, 0.32), (s / 2 + 0.01, s / 2 + 0.01), (0.05, 0.02), 0.16, "slate")


def _glass_pyramid(m, c, half, h):
    x, y = c
    kit.pyramid(m, (x, y, 0.0), half, h, "sky_glass")
    if m.detail == 0 and half > 0.1:
        for sx in (-1, 1):
            for sy in (-1, 1):
                m.rod((x + sx * half, y + sy * half, 0.0), (x, y, h), 0.008, 0.008, "steel", sides=3)
        for t in (0.33, 0.66):
            hw = half * (1 - t)
            _box(m, x, y - hw - 0.002, h * t - 0.004, hw * 2, 0.006, 0.008, "steel")


# Statue of Liberty -------------------------------------------------------------------------
def statue_of_liberty(m):
    _ground(m, "water", 0.86, top=0.0)
    kit.hex_patch(m, 0.66, "grass", z0=-0.03, z1=0.02)
    _star_fort(m, 0.0, 0.05, 0.27, 0.5, 0.11)
    _box(m, 0.0, 0.05, 0.1, 0.34, 0.34, 0.05, "stone")
    m.taper((0.0, 0.05, 0.15), (0.14, 0.14), (0.11, 0.11), 0.32, "stone")
    _box(m, 0.0, 0.05, 0.47, 0.26, 0.26, 0.04, "limestone")
    _box(m, 0.0, 0.05, 0.51, 0.2, 0.2, 0.04, "stone")
    if m.detail == 0:
        for s in (-1, 1):
            _box(m, s * 0.07, 0.05 - 0.13, 0.2, 0.035, 0.012, 0.16, "dark")
    with m.at(move(0.0, 0.05, 0.55), scale(0.95)):
        _liberty(m)


def _star_fort(m, x, y, r_in, r_out, h):
    n = m.pick(11, 11, 6)
    core = [(x + px, y + py) for px, py in kit.ring(n, r_in, 90)]
    _poly(m, core, -0.03, h, "concrete")
    for k in range(n):
        a = math.radians(90 + 360.0 * (k + 0.5) / n)
        tip = (x + r_out * math.cos(a), y + r_out * math.sin(a))
        _poly(m, [core[k], tip, core[(k + 1) % n]], -0.03, h, "concrete")


def _liberty(m):
    mat = "copper_green"
    sides = m.pick(8, 6, 5)
    m.rings([((0, 0, 0), 0.15, 0.12), ((0, 0, 0.42), 0.12, 0.1), ((0, 0, 0.68), 0.125, 0.09)], mat, sides=sides)
    m.ball((0.0, -0.01, 0.77), (0.055, 0.055, 0.065), mat, u=m.pick(7, 5, 4), v=m.pick(4, 3, 3))
    m.rod((0.08, 0.0, 0.62), (0.12, 0.01, 1.02), 0.036, 0.028, mat, sides=m.pick(6, 4, 4))
    m.rod((0.12, 0.01, 0.98), (0.125, 0.01, 1.08), 0.028, 0.04, "gold", sides=m.pick(6, 4, 3))
    _cone(m, (0.125, 0.01, 1.07), 0.04, 0.11, "gold", sides=m.pick(6, 4, 3))
    if m.detail == 2:
        return
    m.rod((-0.09, 0.0, 0.62), (-0.11, -0.07, 0.46), 0.032, 0.026, mat, sides=4)
    with m.at(move(-0.1, -0.09, 0.4), rot_x(-12)):
        m.box((0, 0, 0.07), (0.035, 0.016, 0.075), mat)
    for i in range(7 if m.detail == 0 else 5):
        phi = math.radians(-80 + 160 * i / (6 if m.detail == 0 else 4))
        d = (math.sin(phi) * 0.12, -math.cos(phi) * 0.07, 0.07)
        m.rod((0.0, -0.01, 0.8), (d[0], -0.01 + d[1], 0.8 + d[2]), 0.012, 0.0, mat, sides=3)


# Cristo Redentor ---------------------------------------------------------------------------
def cristo_redentor(m):
    _ground(m, "leaf_dark", 0.86)
    kit.peak(m, (0.0, 0.06, 0.0), 0.72, 0.72, "leaf", seed=4)
    kit.peak(m, (0.02, 0.1, 0.32), 0.3, 0.66, "rock", seed=11)
    if m.detail < 2:
        for i, (x, y) in enumerate(((-0.62, -0.3), (0.55, -0.42), (-0.2, -0.62), (0.68, 0.05), (-0.6, 0.35))):
            kit.tree(m, (x, y, 0.0), 0.16, "broadleaf", "leaf_dark", seed=i)
    _box(m, 0.02, 0.1, 0.88, 0.13, 0.13, 0.1, "stone")
    with m.at(move(0.02, 0.1, 0.98), scale(1.2)):
        _redeemer(m, 0.0, 0.0, 0.0)


def _redeemer(m, x, y, z):
    mat = "marble"
    sides = m.pick(8, 6, 4)
    m.rings([((x, y, z), 0.06, 0.05), ((x, y, z + 0.3), 0.05, 0.042), ((x, y, z + 0.4), 0.065, 0.045)], mat,
            sides=sides)
    m.loft([[(x - 0.27, y - 0.02, z + 0.355), (x - 0.27, y + 0.02, z + 0.355), (x - 0.27, y + 0.02, z + 0.39),
             (x - 0.27, y - 0.02, z + 0.39)],
            [(x + 0.27, y - 0.02, z + 0.355), (x + 0.27, y + 0.02, z + 0.355), (x + 0.27, y + 0.02, z + 0.39),
             (x + 0.27, y - 0.02, z + 0.39)]], mat)
    if m.detail < 2:
        m.taper((x, y, z + 0.26), (0.05, 0.035), (0.13, 0.03), 0.12, mat)
    m.ball((x, y - 0.005, z + 0.45), (0.035, 0.035, 0.045), mat, u=m.pick(6, 5, 4), v=m.pick(4, 3, 3))


# Eiffel Tower ------------------------------------------------------------------------------
EIFFEL_PROFILE = ((0.0, 0.42), (0.2, 0.3), (0.38, 0.21), (0.8, 0.105), (1.58, 0.022))
IRON = "rust"


def eiffel_tower(m):
    _ground(m, "grass", 0.86)
    kit.patch(m, (0.0, 0.0), (0.56, 0.56), "sand", z0=-0.03, z1=0.02)
    if m.detail < 2:
        kit.patch(m, (0.0, -0.7), (0.12, 0.2), "sand", z0=-0.03, z1=0.02)
    _eiffel_legs(m)
    _box(m, 0.0, 0.0, 0.36, 0.5, 0.5, 0.06, IRON)
    _box(m, 0.0, 0.0, 0.79, 0.27, 0.27, 0.05, IRON)
    _box(m, 0.0, 0.0, 1.56, 0.07, 0.07, 0.07, IRON)
    m.rod((0.0, 0.0, 1.62), (0.0, 0.0, 1.86), 0.014, 0.006, IRON, sides=4)
    if m.detail < 2:
        for z, s in ((1.05, 0.13), (1.3, 0.085)):
            _box(m, 0.0, 0.0, z, s, s, 0.03, IRON)
        _eiffel_arches(m)
    if m.detail == 0:
        _eiffel_braces(m)


def _leg_point(i, sx, sy):
    z, d = EIFFEL_PROFILE[i]
    return (sx * d, sy * d, z)


def _eiffel_legs(m):
    radii = (0.065, 0.055, 0.045, 0.035, 0.016)
    sides = m.pick(4, 4, 3)
    for sx in (-1, 1):
        for sy in (-1, 1):
            for i in range(len(EIFFEL_PROFILE) - 1):
                a = _leg_point(i, sx, sy)
                if i == 0:
                    a = (a[0], a[1], -0.03)
                m.rod(a, _leg_point(i + 1, sx, sy), radii[i], radii[i + 1], IRON, sides=sides)


def _eiffel_arches(m):
    n = m.pick(5, 4, 3)
    for k in range(4):
        with m.at(rot_z(90 * k)):
            pts = []
            for j in range(n + 1):
                th = math.pi * j / n
                z = 0.1 + 0.2 * math.sin(th)
                d = 0.42 - (0.12 / 0.2) * min(z, 0.2) - (0.09 / 0.18) * max(z - 0.2, 0.0)
                pts.append((0.29 * math.cos(th), -d, z))
            for a, b in zip(pts, pts[1:]):
                m.rod(a, b, 0.016, 0.016, IRON, sides=3)


def _eiffel_braces(m):
    for k in range(4):
        with m.at(rot_z(90 * k)):
            for z0, z1 in ((0.42, 0.79), (0.84, 1.05), (1.08, 1.3)):
                d0, d1 = _width_at(z0), _width_at(z1)
                m.rod((-d0, -d0, z0), (d1, -d1, z1), 0.01, 0.01, IRON, sides=3)
                m.rod((d0, -d0, z0), (-d1, -d1, z1), 0.01, 0.01, IRON, sides=3)


def _width_at(z):
    for (z0, d0), (z1, d1) in zip(EIFFEL_PROFILE, EIFFEL_PROFILE[1:]):
        if z0 <= z <= z1:
            return d0 + (d1 - d0) * (z - z0) / (z1 - z0)
    return EIFFEL_PROFILE[-1][1]


# Pentagon ----------------------------------------------------------------------------------
def pentagon(m):
    _ground(m, "grass", 0.86)
    _pent_ring(m, 0.82, 0.62, 0.17, "limestone")
    _pent_ring(m, 0.5, 0.3, 0.19, "limestone")
    if m.detail < 2:
        _pent_ring(m, 0.62, 0.5, 0.025, "concrete", roof=False)
    _poly(m, _pent(0.3), -0.03, 0.025, "grass")
    if m.detail == 0:
        kit.flag(m, (0.0, -0.89 * 0.82, 0.0), 0.2, size=0.06)


def _pent(r):
    return kit.ring(5, r, -90 + 36)


def _pent_ring(m, r_out, r_in, h, mat, roof=True):
    outer, inner = _pent(r_out), _pent(r_in)
    for k in range(5):
        seg = [outer[k], outer[(k + 1) % 5], inner[(k + 1) % 5], inner[k]]
        _poly(m, seg, -0.03, h, mat)
        if roof:
            shrink = [(p[0] * 0.97 + q[0] * 0.03, p[1] * 0.97 + q[1] * 0.03) for p, q in
                      zip(seg, [inner[k], inner[(k + 1) % 5], outer[(k + 1) % 5], outer[k]])]
            _poly(m, shrink, h - 0.01, h + 0.025, "slate")
    if roof and m.detail == 0:
        for k in range(5):
            a, b = outer[k], outer[(k + 1) % 5]
            mid = ((a[0] + b[0]) / 2, (a[1] + b[1]) / 2)
            dx, dy = b[0] - a[0], b[1] - a[1]
            length = math.hypot(dx, dy)
            nrm = (-dy / length * 0.004, dx / length * 0.004)
            out = (mid[0] - nrm[0], mid[1] - nrm[1]) if mid[0] * nrm[0] + mid[1] * nrm[1] < 0 else \
                (mid[0] + nrm[0], mid[1] + nrm[1])
            _box(m, out[0], out[1], h * 0.35, length * 0.9, 0.012, 0.03, "glass_dark",
                 yaw=math.degrees(math.atan2(dy, dx)))


# Sydney Opera House ------------------------------------------------------------------------
def sydney_opera_house(m):
    _ground(m, "water", 0.86, top=0.0)
    _poly(m, [(-0.74, 0.3), (0.74, 0.3), (0.74, 0.42), (0.0, 0.85), (-0.74, 0.42)], -0.03, 0.02, "grass")
    _poly(m, [(-0.64, -0.3), (-0.3, -0.56), (0.45, -0.48), (0.64, -0.12), (0.56, 0.4), (-0.52, 0.42)], -0.03,
          0.09, "sandstone")
    if m.detail < 2:
        for i, (x, y) in enumerate(((-0.5, 0.6), (-0.3, 0.68), (0.4, 0.62), (0.62, 0.48))):
            kit.tree(m, (x, y, 0.0), 0.15, "broadleaf", "leaf", seed=i)
    with m.at(move(0.0, 0.0, 0.09), rot_z(-18)):
        for x, h, rev in ((-0.36, 0.56, False), (-0.15, 0.46, False), (0.04, 0.37, False), (0.2, 0.22, True)):
            _sail(m, x * 1.15, -0.17, h * 1.25, rev)
        for x, h, rev in ((-0.24, 0.42, False), (-0.06, 0.34, False), (0.1, 0.27, False), (0.24, 0.17, True)):
            _sail(m, x * 1.15, 0.2, h * 1.25, rev)


def _sail(m, x, y, h, reverse):
    w, d, lean = h * 0.3, h * 0.42, h * 0.3
    with m.at(move(x, y, 0.0), rot_z(180 if reverse else 0)):
        base = [(0.0, -w, -0.01), (0.0, w, -0.01), (d, 0.0, -0.01)]
        mid = [(-lean * 0.55, -w * 0.72, h * 0.5), (-lean * 0.55, w * 0.72, h * 0.5), (d * 0.5 - lean * 0.2, 0.0,
                                                                                        h * 0.6)]
        m.loft([base, mid, [(-lean, 0.0, h)]], "white")
        if m.detail < 2:
            gx = -0.012
            m.loft([[(gx, -w * 0.85, -0.01), (gx, w * 0.85, -0.01), (gx - lean * 0.42, 0.0, h * 0.44)],
                    [(gx + 0.01, -w * 0.85, -0.01), (gx + 0.01, w * 0.85, -0.01), (gx + 0.01 - lean * 0.42, 0.0,
                                                                                       h * 0.44)]], "glass_dark")


# Leaning Tower of Pisa ---------------------------------------------------------------------
def leaning_tower_of_pisa(m):
    _ground(m, "grass", 0.86)
    if m.detail < 2:
        kit.patch(m, (0.0, -0.05), (0.75, 0.05), "marble", z0=-0.03, z1=0.02)
    _duomo(m, -0.2, 0.3)
    if m.detail < 2:
        kit.cylinder(m, (-0.52, -0.38, 0.0), 0.16, 0.16, "marble", sides=m.pick(10, 7, 5))
        kit.dome(m, (-0.52, -0.38, 0.16), 0.13, "terracotta", h=0.12, sides=m.pick(10, 7, 5))
        _cone(m, (-0.52, -0.38, 0.27), 0.03, 0.06, "terracotta", sides=4)
    with m.at(move(0.42, -0.2, 0.0), rot_y(9)):
        _pisa_tower(m)


def _pisa_tower(m):
    sides = m.pick(12, 8, 6)
    kit.cylinder(m, (0.0, 0.0, -0.06), 0.165, 0.27, "marble", sides=sides)
    z = 0.21
    stories = m.pick(6, 6, 3)
    for i in range(stories):
        h = 0.78 / stories
        kit.cylinder(m, (0.0, 0.0, z), 0.135, h, "stone" if m.detail < 2 else "marble", sides=sides)
        kit.cylinder(m, (0.0, 0.0, z + h - 0.025), 0.175, 0.025, "marble", sides=sides)
        if m.detail == 0:
            for px, py in kit.ring(10, 0.158, 0):
                m.rod((px, py, z), (px, py, z + h - 0.02), 0.012, 0.012, "marble", sides=3)
        elif m.detail == 1:
            kit.cylinder(m, (0.0, 0.0, z), 0.16, h * 0.18, "marble", sides=sides)
        z += h
    kit.cylinder(m, (0.0, 0.0, z), 0.1, 0.13, "marble", sides=sides)
    kit.cylinder(m, (0.0, 0.0, z + 0.13), 0.11, 0.02, "marble", sides=sides)
    if m.detail < 2:
        _box(m, 0.0, -0.1, z + 0.03, 0.05, 0.02, 0.07, "dark")


def _duomo(m, x, y):
    _box(m, x, y, 0.0, 0.62, 0.22, 0.24, "marble")
    kit.wedge_roof(m, (x, y, 0.24), 0.62, 0.22, 0.08, "hull_grey")
    _box(m, x + 0.12, y, 0.0, 0.2, 0.5, 0.22, "marble")
    kit.wedge_roof(m, (x + 0.12, y, 0.22), 0.5, 0.2, 0.07, "hull_grey", yaw=90)
    kit.cylinder(m, (x + 0.12, y, 0.29), 0.07, 0.05, "marble", sides=m.pick(8, 6, 5))
    kit.dome(m, (x + 0.12, y, 0.34), 0.075, "hull_grey", h=0.08, sides=m.pick(8, 6, 5))
    _box(m, x - 0.3, y, 0.0, 0.04, 0.26, 0.34, "marble")
    if m.detail == 0:
        for z in (0.08, 0.16, 0.24):
            _box(m, x - 0.323, y, z, 0.006, 0.24, 0.012, "stone")
        _windows(m, x - 0.28, x + 0.28, y - 0.113, 0.06, 8, 0.024, 0.1, "stone", depth=0.006)


# Neuschwanstein ----------------------------------------------------------------------------
def neuschwanstein(m):
    _ground(m, "leaf_dark", 0.86)
    _crag(m, (0.0, 0.04), 0.7, 0.44, 0.32, "rock", 3)
    if m.detail < 2:
        for i, (x, y) in enumerate(((-0.62, -0.3), (-0.45, -0.55), (0.55, -0.4), (0.68, 0.05), (-0.68, 0.2),
                                    (0.3, -0.62), (-0.1, -0.7))):
            kit.tree(m, (x, y, 0.0), 0.2, "conifer", "leaf_dark", seed=i)
    with m.at(move(-0.03, 0.02, 0.3), scale(1.3)):
        _neuschwanstein_castle(m, 0.0)


def _neuschwanstein_castle(m, z):
    _box(m, 0.08, 0.14, z, 0.44, 0.16, 0.38, "white")
    kit.wedge_roof(m, (0.08, 0.14, z + 0.38), 0.44, 0.16, 0.16, "slate")
    if m.detail == 0:
        for row in (0.1, 0.2, 0.3):
            _windows(m, -0.12, 0.28, 0.059, z + row, 6, 0.025, 0.05, "dark")
    _tower_spire(m, (-0.16, 0.04, z), 0.065, 0.62, 0.24)
    _tower_spire(m, (0.31, 0.1, z), 0.045, 0.5, 0.17)
    _box(m, 0.28, -0.13, z, 0.2, 0.12, 0.22, "white")
    kit.wedge_roof(m, (0.28, -0.13, z + 0.22), 0.2, 0.12, 0.08, "slate")
    if m.detail < 2:
        _tower_spire(m, (0.38, -0.18, z), 0.035, 0.32, 0.12)
        _tower_spire(m, (-0.08, -0.12, z), 0.04, 0.3, 0.13)
    _box(m, -0.02, -0.3, z - 0.05, 0.24, 0.1, 0.2, "brick")
    if m.detail < 2:
        for s in (-1, 1):
            kit.tower(m, (-0.02 + s * 0.12, -0.33, z - 0.05), 0.03, 0.24, "brick", roof="cone", roof_mat="slate",
                      sides=m.pick(6, 5, 4))
        _box(m, -0.02, -0.355, z - 0.05, 0.06, 0.01, 0.1, "dark")


def _tower_spire(m, c, r, h, spire):
    sides = m.pick(8, 6, 4)
    kit.cylinder(m, c, r, h, "white", sides=sides)
    _cone(m, (c[0], c[1], c[2] + h), r * 1.25, spire, "roof_blue", sides=sides)
    if m.detail == 0:
        _box(m, c[0], c[1] - r, c[2] + h * 0.7, 0.02, 0.01, 0.05, "dark")
        m.rod((c[0], c[1], c[2] + h + spire), (c[0], c[1], c[2] + h + spire + 0.05), 0.006, 0.002, "gold", sides=3)


def _crag(m, c, r0, r1, h, mat, seed):
    sides = m.pick(9, 7, 5)
    levels = []
    for j, (t, k) in enumerate(((0.0, r0), (0.6, (r0 + r1) / 2 * 0.95), (1.0, r1))):
        ring_pts = []
        for i in range(sides):
            a = math.tau * (i + 0.5) / sides
            f = k * (0.88 + 0.2 * kit.rand(seed, i + 7 * j))
            ring_pts.append((c[0] + math.cos(a) * f, c[1] + math.sin(a) * f, -0.04 + (h + 0.04) * t))
        levels.append(ring_pts)
    m.loft(levels, mat)


# Great Firewall ----------------------------------------------------------------------------
def great_firewall(m):
    _ground(m, "concrete", 0.86)
    pts = [(-0.8, -0.04), (-0.4, -0.42), (0.02, -0.2), (0.42, -0.5), (0.78, -0.14)]
    for a, b in zip(pts, pts[1:]):
        _data_wall(m, a, b)
    for x, y in pts if m.detail < 2 else ():
        _server_tower(m, x, y, 0.12, 0.32)
    for x, y, s, h in ((-0.46, 0.26, 0.18, 0.7), (0.46, 0.28, 0.18, 0.86), (-0.2, 0.56, 0.14, 0.5),
                       (0.24, 0.6, 0.14, 0.56)):
        _server_tower(m, x, y, s, h)
    _server_tower(m, 0.0, 0.18, 0.26, 1.25)
    m.rod((0.0, 0.18, 1.25), (0.0, 0.18, 1.6), 0.02, 0.006, "steel", sides=m.pick(5, 4, 3))
    if m.detail < 2:
        m.ball((0.0, 0.18, 1.45), (0.07, 0.07, 0.02), "neon", u=m.pick(8, 6, 4), v=3)


def _data_wall(m, a, b):
    dx, dy = b[0] - a[0], b[1] - a[1]
    length = math.hypot(dx, dy)
    yaw = math.degrees(math.atan2(dy, dx))
    cx, cy = (a[0] + b[0]) / 2, (a[1] + b[1]) / 2
    _box(m, cx, cy, -0.03, length, 0.07, 0.23, "obsidian", yaw)
    if m.detail < 2:
        _box(m, cx, cy, 0.09, length, 0.078, 0.016, "neon", yaw)
        _box(m, cx, cy, 0.2, length + 0.002, 0.04, 0.012, "neon", yaw)
    if m.detail == 0:
        n = max(int(length / 0.09), 2)
        for i in range(n):
            t = (i + 0.5) / n
            _box(m, a[0] + dx * t, a[1] + dy * t, 0.2, length / n * 0.45, 0.07, 0.05, "obsidian", yaw)


def _server_tower(m, x, y, s, h):
    _box(m, x, y, -0.03, s, s, h + 0.03, "dark")
    if m.detail == 2:
        _box(m, x, y, h - 0.04, s + 0.01, s + 0.01, 0.03, "neon")
        return
    bands = max(int(h / 0.16), 1) if m.detail == 0 else max(int(h / 0.4), 1)
    for i in range(bands):
        z = h * (i + 0.7) / (bands + 0.3)
        _box(m, x, y, z, s + 0.008, s + 0.008, 0.012, "neon")
    if m.detail == 0 and s > 0.15:
        for ox in (-s * 0.25, 0.0, s * 0.25):
            _box(m, x + ox, y - s / 2 - 0.003, 0.04, 0.012, 0.006, h - 0.1, "gem_blue")
    _box(m, x, y, h, s * 0.8, s * 0.8, 0.02, "neon")


# Globe Theatre -----------------------------------------------------------------------------
def globe_theatre(m):
    _ground(m, "grass", 0.86)
    n = m.pick(14, 10, 8)
    r_out, r_in, wall_h = 0.66, 0.44, 0.3
    outer, inner = kit.ring(n, r_out, 90), kit.ring(n, r_in, 90)
    _poly(m, kit.ring(n, r_in + 0.01, 90), -0.03, 0.02, "soil")
    for k in range(n):
        a0, a1 = outer[k], outer[(k + 1) % n]
        b0, b1 = inner[k], inner[(k + 1) % n]
        _poly(m, [a0, a1, b1, b0], -0.03, wall_h, "plaster")
        _thatch(m, k, n, r_out + 0.05, r_in - 0.03, wall_h)
        if m.detail == 0:
            _box(m, a0[0] * 1.004, a0[1] * 1.004, 0.0, 0.022, 0.022, wall_h, "wood_dark",
                 yaw=90 + 360.0 * k / n)
            dx, dy = a1[0] - a0[0], a1[1] - a0[1]
            mid = ((a0[0] + a1[0]) / 2 * 1.006, (a0[1] + a1[1]) / 2 * 1.006)
            for z in (0.1, 0.2):
                _box(m, mid[0], mid[1], z, math.hypot(dx, dy), 0.012, 0.016, "wood_dark",
                     yaw=math.degrees(math.atan2(dy, dx)))
    _stage(m, 0.0, 0.28)


def _thatch(m, k, n, r_out, r_in, z):
    sec = []
    for a in (90 + 360.0 * k / n, 90 + 360.0 * (k + 1) / n):
        c, s = math.cos(math.radians(a)), math.sin(math.radians(a))
        rm = (r_out + r_in) / 2
        sec.append([(c * r_out, s * r_out, z - 0.02), (c * r_in, s * r_in, z - 0.02), (c * rm, s * rm, z + 0.13)])
    m.loft(sec, "wheat")


def _stage(m, x, y):
    _box(m, x, y, 0.0, 0.3, 0.16, 0.08, "wood")
    _box(m, x, y + 0.1, 0.0, 0.34, 0.1, 0.4, "plaster")
    kit.wedge_roof(m, (x, y + 0.06, 0.4), 0.34, 0.2, 0.1, "wheat")
    if m.detail < 2:
        for s in (-1, 1):
            kit.column(m, (x + s * 0.11, y - 0.06, 0.08), 0.2, 0.012, "wood_dark", capital=False, sides=4)
        kit.flag(m, (x, y + 0.06, 0.48), 0.14, size=0.09)


# Broadway ----------------------------------------------------------------------------------
BILLBOARDS = ("neon", "pink", "red", "citrus", "gem_blue", "banana", "purple", "turquoise")


def broadway(m):
    _ground(m, "concrete", 0.86)
    kit.patch(m, (0.0, -0.02), (0.82, 0.1), "asphalt", z0=-0.03, z1=0.022)
    for i, (x, y, w, h, mat) in enumerate(((-0.52, 0.32, 0.28, 0.8, "brick"), (-0.2, 0.36, 0.26, 1.1, "glass_dark"),
                                           (0.14, 0.34, 0.26, 0.9, "concrete"), (0.46, 0.3, 0.28, 0.7, "brick"))):
        _billboard_tower(m, x, y, w, h, mat, i)
    _box(m, -0.02, 0.15, 0.0, 0.1, 0.08, 1.25, "dark")
    for j, z in enumerate((0.35, 0.62, 0.9)):
        _box(m, -0.02, 0.105, z, 0.12, 0.01, 0.22, BILLBOARDS[(j + 3) % len(BILLBOARDS)])
    for i, x in enumerate((-0.45, 0.0, 0.45)):
        _theatre(m, x, -0.4, i)
    if m.detail == 0:
        for i, x in enumerate((-0.6, -0.28, 0.12, 0.5)):
            _box(m, x, -0.06 + 0.08 * (i % 2), 0.022, 0.09, 0.045, 0.04, "banana")


def _billboard_tower(m, x, y, w, h, mat, seed):
    _box(m, x, y, 0.0, w, 0.22, h, mat)
    _box(m, x, y, h, w * 0.6, 0.14, 0.04, "concrete")
    panels = m.pick(3, 2, 1)
    for j in range(panels):
        z = 0.12 + (h - 0.25) * j / max(panels, 1)
        pw = w * (0.92 if j % 2 == 0 else 0.7)
        _box(m, x, y - 0.115, z, pw, 0.012, min(0.2, (h - 0.2) / panels * 0.8),
             BILLBOARDS[(seed * 3 + j) % len(BILLBOARDS)])


def _theatre(m, x, y, i):
    w = 0.36
    _box(m, x, y, 0.0, w, 0.28, 0.3, ("limestone", "brick", "cream")[i])
    if m.detail < 2:
        _box(m, x, y - 0.16, 0.1, w * 0.8, 0.08, 0.05, "gold")
        _box(m, x, y - 0.2, 0.15, w * 0.8, 0.008, 0.035, "white")
        _box(m, x + w * 0.38, y - 0.15, 0.16, 0.02, 0.05, 0.3, BILLBOARDS[i * 2 % len(BILLBOARDS)])
    if m.detail == 0:
        for k in range(6):
            _box(m, x - w * 0.36 + k * w * 0.144, y - 0.205, 0.09, 0.012, 0.012, 0.012, "banana")
        _box(m, x, y - 0.142, 0.0, w * 0.5, 0.008, 0.09, "dark")


# Prora -------------------------------------------------------------------------------------
def prora(m):
    _ground(m, "leaf_dark", 0.86)
    _poly(m, [(-0.82, -0.12), (0.82, -0.12), (0.84, -0.42), (0.0, -0.86), (-0.84, -0.42)], -0.03, 0.02, "sand")
    _poly(m, [(-0.74, -0.44), (0.74, -0.44), (0.62, -0.62), (0.0, -0.86), (-0.62, -0.62)], -0.03, 0.025, "water")
    if m.detail < 2:
        for i in range(m.pick(9, 5, 0)):
            x = -0.66 + 1.32 * i / max(m.pick(9, 5, 1) - 1, 1)
            kit.tree(m, (x, 0.42 + 0.1 * kit.rand(9, i), 0.0), 0.2, "conifer", "leaf", seed=i)
    for x, y, yaw in ((-0.54, 0.08, -8), (0.0, 0.03, 0), (0.54, 0.08, 8)):
        _prora_block(m, x, y, yaw)


def _prora_block(m, x, y, yaw):
    length, depth, h = 0.58, 0.13, 0.34
    with m.at(move(x, y, 0.0), rot_z(yaw)):
        _box(m, 0.0, 0.0, 0.0, length, depth, h, "white")
        _box(m, 0.0, 0.0, h, length, depth, 0.012, "concrete")
        if m.detail < 2:
            floors = m.pick(5, 2, 0)
            for f in range(floors):
                _box(m, 0.0, -depth / 2 - 0.002, 0.05 + f * 0.058, length - 0.02, 0.008, 0.022, "glass_dark")
        if m.detail == 0:
            for i in range(3):
                _box(m, -length / 2 + 0.1 + i * 0.19, -depth / 2 - 0.02, 0.0, 0.05, 0.04, h + 0.03, "white")


# International Space Station (launch complex monument) ------------------------------------
def international_space_station(m):
    _ground(m, "grass_dry", 0.86)
    _box(m, -0.22, -0.08, -0.03, 0.62, 0.56, 0.1, "concrete")
    if m.detail < 2:
        _box(m, -0.3, -0.36, -0.03, 0.18, 0.04, 0.09, "dark")
    _rocket(m, -0.3, -0.12, 0.07)
    _gantry(m, -0.05, 0.02, 1.02)
    m.rod((0.44, 0.3, -0.03), (0.44, 0.3, 0.78), 0.04, 0.03, "steel", sides=m.pick(6, 5, 4))
    with m.at(move(0.42, 0.26, 0.98), rot_x(-20)):
        _iss(m)


def _rocket(m, x, y, z):
    sides = m.pick(10, 7, 5)
    kit.cylinder(m, (x, y, z), 0.075, 0.82, "white", sides=sides)
    if m.detail < 2:
        kit.cylinder(m, (x, y, z + 0.3), 0.078, 0.05, "black", sides=sides)
        kit.cylinder(m, (x, y, z + 0.62), 0.078, 0.03, "black", sides=sides)
    _cone(m, (x, y, z + 0.82), 0.075, 0.18, "white", sides=sides)
    for s in (-1, 1):
        bx = x + s * 0.1
        kit.cylinder(m, (bx, y - 0.01, z), 0.038, 0.42, "white", sides=m.pick(8, 6, 4))
        _cone(m, (bx, y - 0.01, z + 0.42), 0.038, 0.1, "white" if m.detail else "red", sides=m.pick(8, 6, 4))


def _iss(m):
    m.box((0.0, 0.0, 0.0), (0.36, 0.02, 0.02), "steel")
    for px in (-0.31, -0.21, 0.21, 0.31):
        for s in (-1, 1):
            m.box((px, 0.0, s * 0.15), (0.04, 0.005, 0.12), "ochre")
            if m.detail == 0:
                m.box((px, 0.0, s * 0.15), (0.042, 0.003, 0.004), "dark")
    sides = m.pick(8, 6, 4)
    m.rod((0.0, -0.16, 0.04), (0.0, 0.16, 0.04), 0.035, 0.035, "white", sides=sides)
    m.rod((-0.12, 0.0, 0.04), (0.12, 0.0, 0.04), 0.03, 0.03, "white", sides=sides)
    if m.detail < 2:
        for s in (-1, 1):
            m.box((s * 0.1, 0.0, -0.08), (0.03, 0.004, 0.06), "white")
        m.rod((0.0, -0.22, 0.04), (0.0, -0.16, 0.04), 0.02, 0.03, "white", sides=sides)


def models():
    return [
        _model("porcelain_tower", porcelain_tower, "upright", "land"),  # renaissance
        _model("himeji_castle", himeji_castle, "upright", "land"),  # renaissance
        _model("sistine_chapel", sistine_chapel, "upright", "land"),  # renaissance
        _model("kremlin", kremlin, "upright", "land"),  # modern
        _model("forbidden_palace", forbidden_palace, "upright", "land"),  # renaissance
        _model("taj_mahal", taj_mahal, "upright", "land"),  # renaissance
        _model("louvre", louvre, "upright", "land"),  # industrial
        _model("statue_of_liberty", statue_of_liberty, "upright", "land"),  # modern
        _model("cristo_redentor", cristo_redentor, "upright", "land"),  # modern
        _model("eiffel_tower", eiffel_tower, "upright", "land"),  # modern
        _model("pentagon", pentagon, "upright", "land"),  # atomic
        _model("sydney_opera_house", sydney_opera_house, "upright", "land"),  # atomic
        _model("leaning_tower_of_pisa", leaning_tower_of_pisa, "upright", "land"),  # renaissance
        _model("neuschwanstein", neuschwanstein, "upright", "land"),  # modern
        _model("great_firewall", great_firewall, "upright", "land"),  # atomic
        _model("globe_theatre", globe_theatre, "upright", "land"),  # renaissance
        _model("broadway", broadway, "upright", "land"),  # modern
        _model("prora", prora, "upright", "land"),  # modern
        _model("international_space_station", international_space_station, "upright", "land"),  # ancient
    ]
