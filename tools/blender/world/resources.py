"""Resource props of Civ V BNW (classic/docs/classic/resources.json): what the player sees on a tile
that holds the resource. Props stand in the yard (radius 0.30-0.40 around the tile anchor),
the improvement of the resource is drawn around them (improvements.py). Resources with
no map presence are listed in world_catalog.ICON_ONLY instead.

Budget class "resource": LOD0 <= 300, LOD1 <= 140, LOD2 <= 48 triangles. The far marker
(detail 2) keeps a few big solids that carry the colour and silhouette of the prop.
Land props are authored a little small and scaled up as a whole by `_model` (factor k).

Ownership split: the models only classic draws. The helpers shared with the platform's models
are imported from platform/tools/blender/world/resources.py.
"""
import math

from geom import move, rot_x, rot_y, rot_z, scale
import kit

from resources import Beast, _beast, _crystal, _frame, _keep, _model, _pair_cones, _tail


# Shared small parts ------------------------------------------------------------------------
def _leaf(m, base, yaw, pitch, length, width, mat, thick=0.008):
    """Flat diamond leaf from `base` outward along yaw, raised `pitch` degrees."""
    with m.at(move(*base), rot_z(yaw), rot_y(-pitch)):
        m.octa((length / 2, 0, 0), (length / 2, width, thick), mat)


def _blade(m, base, tip, width, mat):
    """Thin pointed blade (grass, cane leaf): a tetrahedron from a small base to the tip."""
    x, y, z = base
    m.loft([[(x - width, y, z), (x + width, y, z), (x, y + width, z + width)], [tip]], mat)


def _sack(m, c, r, h, mat, top=None):
    """Open sack or basket with a heap of goods on top."""
    kit.cylinder(m, (c[0], c[1], -0.01), r, h + 0.01, mat, sides=m.pick(6, 5, 4), r_top=r * 0.88)
    if top:
        kit.cone(m, (c[0], c[1], h - 0.004), r * 0.9, r * 0.9, top, sides=m.pick(6, 5, 4))


def _foam(m, x, y, r, sides=None):
    """Flat splash disc on the water surface."""
    m.rings([((x, y, -0.03), r, r), ((x, y, 0.006), r, r)], "foam", sides=sides or m.pick(7, 5, 4))
COW = Beast(leg=0.42, R=0.22, W=0.19, neck=(0.16, 0.06), neck_r=0.11, head=(0.24, 0.1, 55, 0.7), leg_r=0.06,
            stride=0.04, neck_lod=0)
SHEEP = Beast(leg=0.30, R=0.30, W=0.28, neck=(0.12, 0.10), neck_r=0.1, head=(0.2, 0.09, 45, 0.6), leg_r=0.045,
              stride=0.03, neck_lod=0)
DEER = Beast(leg=0.68, R=0.16, W=0.12, neck=(0.14, 0.4), neck_r=0.07, head=(0.22, 0.07, 30, 0.5), leg_r=0.04)
BISON = Beast(leg=0.40, R=0.24, W=0.22, neck=(0.16, -0.06), neck_r=0.15, head=(0.2, 0.12, 75, 0.8), leg_r=0.07,
              rise=0.9, stride=0.04)
PIG = Beast(leg=0.24, R=0.24, W=0.22, neck=(0.08, 0.0), neck_r=0.15, head=(0.2, 0.12, 20, 0.85), leg_r=0.06,
            stride=0.03, neck_lod=0)
FOX = Beast(leg=0.34, R=0.13, W=0.11, neck=(0.12, 0.14), neck_r=0.07, head=(0.24, 0.08, 15, 0.0), leg_r=0.035)
ELEPHANT = Beast(leg=0.55, R=0.32, W=0.28, neck=(0.1, 0.04), neck_r=0.2, head=(0.6, 0.085, 80, 0.45),
                 leg_r=0.11, stride=0.04, neck_lod=0, sides=7)


def cow(m, x, y, yaw, L, coat="white", spots="leather", horns="cream"):
    with _frame(m, x, y, yaw, L):
        sh, hb, ht = _beast(m, COW, coat)
        zc = COW.leg + COW.R
        # Patches: flat prisms a little wider than the body, so they show on both flanks.
        if m.detail < 2:
            with m.at(move(-0.2, 0, zc)):
                m.prism([(-0.16, -0.08), (0.06, -0.12), (0.16, 0.04), (0.04, 0.17), (-0.18, 0.12)],
                        COW.W * 2.08, spots)
        if m.detail == 0:
            with m.at(move(0.17, 0, zc)):
                m.prism([(-0.08, -0.06), (0.07, -0.1), (0.1, 0.08), (-0.06, 0.14)], COW.W * 2.04, spots)
            _tail(m, (-0.49, 0, zc + 0.08), (-0.56, 0, zc - 0.32), 0.035, coat)
        _pair_cones(m, (hb[0] + 0.02, 0.05, hb[2] + 0.06), (0.04, 0.13, 0.1), 0.035, horns)


def sheep(m, x, y, yaw, L, wool="wool", face="dark"):
    with _frame(m, x, y, yaw, L):
        sh, hb, ht = _beast(m, SHEEP, wool, legs=face, face=face)
        _pair_cones(m, (hb[0] + 0.02, 0.06, hb[2]), (-0.04, 0.1, -0.03), 0.035, face, detail_max=0)


def deer(m, x, y, yaw, L, coat="hide", antler="horse_light"):
    with _frame(m, x, y, yaw, L):
        sh, hb, ht = _beast(m, DEER, coat)
        zc = DEER.leg + DEER.R
        if m.detail == 0:
            m.octa((-0.5, 0, zc + 0.04), (0.04, 0.06, 0.07), "white")
        r = 0.03
        for s in (-1, 1):
            root = (hb[0] - 0.02, s * 0.04, hb[2] + 0.06)
            top = (hb[0] - 0.14, s * 0.24, hb[2] + 0.5)
            m.rod(root, top, r, 0.0, antler, sides=3)
            if m.detail < 2:
                mid = (root[0] * 0.5 + top[0] * 0.5, s * 0.15, root[2] * 0.5 + top[2] * 0.5)
                m.rod(mid, (mid[0] + 0.12, s * 0.2, mid[2] + 0.16), r * 0.8, 0.0, antler, sides=3)
            if m.detail == 0:
                low = (root[0] * 0.75 + top[0] * 0.25, s * 0.09, root[2] * 0.75 + top[2] * 0.25)
                m.rod(low, (low[0] + 0.13, s * 0.1, low[2] + 0.07), r * 0.7, 0.0, antler, sides=3)
                m.rod(top, (top[0] + 0.03, s * 0.3, top[2] + 0.08), r * 0.6, 0.0, antler, sides=3)


def bison(m, x, y, yaw, L, coat="fur", front="bison", horns="dark"):
    with _frame(m, x, y, yaw, L):
        sh, hb, ht = _beast(m, BISON, coat, legs=front, face=front)
        zc = BISON.leg + BISON.R
        if m.detail < 2:
            # Shaggy dark hump and shoulders.
            m.ball((0.16, 0, zc + 0.08), (0.3, BISON.W * 1.18, BISON.R * 1.45), front, u=m.pick(6, 5, 4), v=3)
            if m.detail == 0:
                m.octa((ht[0] - 0.03, 0, ht[2] - 0.09), (0.05, 0.05, 0.08), front)  # beard
                _tail(m, (-0.49, 0, zc + 0.05), (-0.55, 0, zc - 0.25), 0.03, front)
        else:
            m.octa((0.16, 0, zc + 0.1), (0.28, BISON.W * 1.1, BISON.R * 1.4), front)
        _pair_cones(m, (hb[0] + 0.03, 0.09, hb[2] + 0.02), (0.02, 0.07, 0.09), 0.03, horns, detail_max=0)


def pig(m, x, y, yaw, L, coat="pink", snout="blossom"):
    with _frame(m, x, y, yaw, L):
        sh, hb, ht = _beast(m, PIG, coat)
        if m.detail == 0:
            m.rod((ht[0] - 0.02, 0, ht[2]), (ht[0] + 0.03, 0, ht[2] - 0.01), 0.07, 0.07, snout, sides=4)
        _pair_cones(m, (hb[0] + 0.03, 0.07, hb[2] + 0.08), (0.06, 0.04, 0.08), 0.04, coat, detail_max=0)


def fox(m, x, y, yaw, L, coat="spice", tip="white"):
    with _frame(m, x, y, yaw, L):
        sh, hb, ht = _beast(m, FOX, coat, legs="dark")
        zc = FOX.leg + FOX.R
        _pair_cones(m, (hb[0] - 0.01, 0.045, hb[2] + 0.04), (-0.02, 0.03, 0.14), 0.04, coat, detail_max=0)
        if m.detail < 2:
            # Big bushy tail with a white tip: the fox's signature.
            root, mid, end = (-0.46, 0, zc + 0.02), (-0.78, 0, zc - 0.12), (-1.0, 0, zc - 0.14)
            m.rod(root, mid, 0.05, 0.1, coat, sides=m.pick(5, 3, 3))
            m.rod(mid, end, 0.1, 0.0, tip, sides=m.pick(5, 3, 3))


def elephant(m, x, y, yaw, L, skin="elephant", ear="ash", tusk="ivory"):
    with _frame(m, x, y, yaw, L):
        b = ELEPHANT
        sh, hb, ht = _beast(m, b, skin)
        zc = b.leg + b.R
        if m.detail < 2:
            m.ball((hb[0] + 0.02, 0, hb[2] + 0.02), (0.15, 0.15, 0.17), skin, u=m.pick(6, 5, 4), v=3)
            for s in (-1, 1):
                # Large flat ears against the sides of the head, facing the viewer.
                with m.at(move(hb[0] - 0.06, s * 0.17, hb[2]), rot_z(-s * 20)):
                    m.prism([(-0.22, 0.15), (0.02, 0.2), (0.06, -0.15), (-0.1, -0.3), (-0.23, -0.15)], 0.03,
                            ear, axis="y")
            _tail(m, (-0.49, 0, zc + 0.1), (-0.56, 0, zc - 0.3), 0.03, skin)
        for s in (-1, 1):
            # Tusks sweep forward and up: the ivory the resource is about.
            root, bend = (hb[0] + 0.1, s * 0.08, hb[2] - 0.14), (hb[0] + 0.3, s * 0.11, hb[2] - 0.2)
            if m.detail < 2:
                m.rod(root, bend, 0.05, 0.04, tusk, sides=m.pick(4, 3, 3))
                m.rod(bend, (hb[0] + 0.46, s * 0.1, hb[2] - 0.04), 0.04, 0.0, tusk, sides=m.pick(4, 3, 3))
            else:
                m.rod(root, (hb[0] + 0.44, s * 0.1, hb[2] - 0.1), 0.05, 0.0, tusk, sides=3)


def uranium(m):
    kit.rock(m, (0.0, 0.1, -0.02), 0.22, "rock_dark", seed=30, squash=0.65)
    if m.detail < 2:
        kit.rock(m, (-0.2, -0.1, -0.02), 0.1, "obsidian", seed=31)
    crystals = ((0.0, 0.04, 0.3, 0.05, 6, 0), (0.12, 0.08, 0.22, 0.04, 28, 20), (-0.11, 0.06, 0.2, 0.04, -26, 60),
                (0.08, -0.1, 0.15, 0.035, 30, -50), (-0.06, -0.12, 0.12, 0.03, -20, -80),
                (0.2, -0.04, 0.1, 0.03, 40, 0))
    for i, (x, y, h, r, lean, yaw) in enumerate(crystals):
        if _keep(m, i, (6, 4, 2)):
            _crystal(m, x, y, h, r, "uranium", lean, yaw)


def stone(m):
    # Cut blocks stacked by a rough quarried boulder.
    kit.rock(m, (-0.17, 0.12, -0.02), 0.17, "rock", seed=40, squash=0.8)
    blocks = ((0.06, -0.06, 0.0, 0.16, 0.11, 0.09, 10), (0.2, 0.06, 0.0, 0.13, 0.12, 0.09, -12),
              (0.12, 0.0, 0.09, 0.14, 0.1, 0.08, 4), (-0.16, -0.18, 0.0, 0.12, 0.09, 0.08, 30))
    for i, (x, y, z, sx, sy, sz, yaw) in enumerate(blocks):
        if _keep(m, i, (4, 4, 2)):
            sink = 0.02 if z == 0 else 0.0
            kit.block(m, (x, y, z - sink), (sx, sy, sz + sink), "stone" if i != 2 else "limestone", yaw)
    if m.detail == 0:
        # Mason's chisel and mallet on the top block.
        m.rod((0.06, -0.04, 0.18), (0.16, 0.0, 0.2), 0.008, 0.006, "wood", sides=3)
        m.box((0.16, 0.0, 0.2), (0.02, 0.016, 0.016), "wood_dark")


def gold(m):
    kit.rock(m, (-0.04, 0.1, -0.02), 0.22, "rock_dark", seed=50, squash=0.7)
    if m.detail < 2:
        kit.rock(m, (0.2, 0.02, -0.02), 0.11, "rock", seed=51)
    nuggets = ((-0.02, 0.12, 0.17, 0.065), (-0.1, 0.0, 0.1, 0.06), (0.04, 0.0, 0.13, 0.055), (0.0, -0.18, 0.0, 0.06),
               (0.18, -0.06, 0.08, 0.045), (-0.2, 0.08, 0.08, 0.045), (0.11, -0.2, 0.0, 0.045),
               (-0.11, -0.19, 0.0, 0.04))
    for i, (x, y, z, r) in enumerate(nuggets):
        if _keep(m, i, (8, 6, 3)):
            m.octa((x, y, z + r * 0.4), (r, r * 0.85, r * 0.8), "gold")


def silver(m):
    kit.rock(m, (-0.06, 0.1, -0.02), 0.21, "slate", seed=60, squash=0.75)
    if m.detail < 2:
        kit.rock(m, (0.18, 0.12, -0.02), 0.11, "rock_dark", seed=61)
    for i, (x, y, h, r, lean, yaw) in enumerate(((-0.06, 0.04, 0.32, 0.05, 8, 0), (0.06, 0.08, 0.25, 0.045, 30, 10),
                                                 (-0.17, 0.06, 0.22, 0.04, -28, 40))):
        if _keep(m, i, (3, 3, 1)):
            _crystal(m, x, y, h, r, "silver", lean, yaw, sides=m.pick(4, 4, 3))
    # Stack of silver ingots in front.
    with m.at(move(0.06, -0.17, 0.0), rot_z(12)):
        bars = ((-0.04, 0.0, -0.01), (0.04, 0.0, -0.01), (0.0, 0.0, 0.025), (-0.04, 0.085, -0.01), (0.04, 0.085, -0.01))
        for i, (bx, by, bz) in enumerate(bars):
            if _keep(m, i, (5, 3, 1)):
                m.taper((bx, by - 0.04, bz), (0.034, 0.075), (0.026, 0.065), 0.035, "silver")


def gems(m):
    kit.rock(m, (0.0, 0.1, -0.02), 0.22, "rock", seed=70, squash=0.6)
    crystals = ((-0.04, 0.06, 0.3, 0.055, 4, 0, "gem_red"), (0.1, 0.12, 0.24, 0.05, 26, 20, "gem_blue"),
                (-0.15, 0.1, 0.2, 0.045, -28, 50, "jade"), (0.06, -0.08, 0.16, 0.04, 30, -40, "gem_blue"),
                (-0.1, -0.12, 0.14, 0.04, -24, -70, "gem_red"), (0.2, -0.04, 0.11, 0.035, 40, 0, "purple"))
    for i, (x, y, h, r, lean, yaw, mat) in enumerate(crystals):
        if _keep(m, i, (6, 4, 2)):
            _crystal(m, x, y, h, r, mat, lean, yaw)


def marble(m):
    blocks = ((-0.06, 0.06, 0.0, 0.2, 0.15, 0.13, 8), (0.16, 0.12, 0.0, 0.14, 0.13, 0.1, -14),
              (-0.04, 0.06, 0.13, 0.15, 0.12, 0.09, -4), (0.14, -0.14, 0.0, 0.13, 0.1, 0.08, 25))
    for i, (x, y, z, sx, sy, sz, yaw) in enumerate(blocks):
        if not _keep(m, i, (4, 4, 3)):
            continue
        z0 = z - 0.02 if z == 0 else z
        kit.block(m, (x, y, z0), (sx, sy, sz + (z - z0)), "marble", yaw)
        if _keep(m, i, (3, 2, 0)):
            # Grey vein across the front face.
            with m.at(move(x, y, z0), rot_z(yaw)):
                front, back = -sy / 2 - 0.003, -sy / 2 + 0.01
                m.loft([[(-sx * 0.3, front, 0.02), (-sx * 0.2, front, 0.02), (sx * 0.1, front, sz + z - z0 - 0.01)],
                        [(-sx * 0.3, back, 0.02), (-sx * 0.2, back, 0.02), (sx * 0.1, back, sz + z - z0 - 0.01)]],
                       "slate")
    if m.detail < 2:
        # A column drum cut from the same stone.
        kit.cylinder(m, (-0.2, -0.14, -0.02), 0.06, 0.1, "marble", sides=m.pick(8, 6, 5))


def copper(m):
    kit.rock(m, (-0.06, 0.1, -0.02), 0.2, "rock_dark", seed=80, squash=0.8)
    kit.rock(m, (0.18, 0.08, -0.02), 0.12, "terracotta", seed=81)
    if m.detail < 2:
        kit.rock(m, (-0.2, -0.14, -0.02), 0.09, "copper_green", seed=82)
    # Green malachite crusts on the ore.
    for i, (x, y, z, r) in enumerate(((-0.1, 0.0, 0.1, 0.07), (0.0, 0.12, 0.17, 0.06), (0.16, 0.02, 0.08, 0.05),
                                      (-0.18, 0.12, 0.08, 0.05))):
        if _keep(m, i, (4, 4, 1)):
            m.octa((x, y, z), (r, r * 0.9, r * 0.7), "copper_green")
    # Copper ingots in front.
    with m.at(move(0.1, -0.16, 0.0), rot_z(-10)):
        for i, (bx, bz) in enumerate(((-0.035, -0.01), (0.035, -0.01), (0.0, 0.025))):
            if _keep(m, i, (3, 3, 1)):
                m.taper((bx, 0, bz), (0.03, 0.07), (0.022, 0.06), 0.035, "terracotta")


def salt(m):
    # White salt pan with harvested salt mounds and a wooden rake.
    if m.detail < 2:
        m.rings([((0.0, 0.0, -0.04), 0.36, 0.3), ((0.0, 0.0, 0.01), 0.36, 0.3)], "sand", sides=m.pick(8, 6, 6))
    m.rings([((0.0, 0.0, -0.04), 0.32, 0.26), ((0.0, 0.0, 0.016), 0.32, 0.26)], "salt", sides=m.pick(8, 6, 6))
    for i, (x, y, r, h) in enumerate(((-0.1, 0.08, 0.1, 0.16), (0.1, 0.12, 0.085, 0.13), (0.02, -0.08, 0.08, 0.12),
                                      (0.2, -0.04, 0.06, 0.09))):
        if _keep(m, i, (4, 4, 3)):
            kit.cone(m, (x, y, 0.0), r, h, "salt", sides=m.pick(7, 5, 4))
    if m.detail == 0:
        m.rod((-0.24, -0.14, 0.02), (-0.06, -0.2, 0.16), 0.007, 0.007, "wood", sides=3)
        m.box((-0.24, -0.14, 0.022), (0.012, 0.04, 0.012), "wood_dark")
        for i in range(3):
            m.box((-0.05 + 0.06 * i, 0.24, 0.02), (0.022, 0.005, 0.006), "wood_dark")


# Crops and plants -------------------------------------------------------------------------
def _sheaf(m, x, y, h, yaw=0.0, lean=0.0):
    """Bound sheaf: a straw bundle narrowing to the tie, golden ears flaring above it."""
    with m.at(move(x, y, 0.0), rot_z(yaw), rot_y(lean)):
        if m.detail == 2:
            m.rings([((0, 0, -0.02), h * 0.15, h * 0.15), ((0, 0, h * 0.8), h * 0.2, h * 0.2), ((0, 0, h), 0, 0)],
                    "wheat", sides=4)
            return
        sides = m.pick(5, 4, 4)
        m.rings([((0, 0, -0.02), h * 0.15, h * 0.15), ((0, 0, h * 0.5), h * 0.09, h * 0.09)], "wheat", sides=sides)
        m.rings([((0, 0, h * 0.45), h * 0.09, h * 0.09), ((0, 0, h * 0.82), h * 0.22, h * 0.22), ((0, 0, h), 0, 0)],
                "ochre", sides=sides)


def wheat(m):
    # A stook of sheaves leaning together, flanked by standing stalks with ears.
    n = m.pick(5, 5, 3)
    for k in range(n):
        a = 90 + 360.0 * k / n
        c = (0.07 * math.cos(math.radians(a)), 0.06 + 0.07 * math.sin(math.radians(a)))
        _sheaf(m, c[0], c[1], 0.3, yaw=a + 180, lean=16)
    if m.detail == 0:
        for i, (x, y) in enumerate(((-0.2, -0.12), (-0.25, -0.02), (0.21, -0.1), (0.25, 0.0), (0.18, -0.2),
                                    (-0.14, -0.21))):
            h = 0.17 + 0.05 * kit.rand(90, i)
            lean = (kit.rand(91, i) - 0.5) * 0.06
            m.rod((x, y, -0.02), (x + lean, y, h), 0.006, 0.005, "wheat", sides=3)
            m.octa((x + lean, y, h + 0.03), (0.014, 0.014, 0.04), "ochre")


def _banana_plant(m, x, y, h, seed, bunch=True):
    if m.detail == 2:
        m.rod((x, y, -0.02), (x, y, h), 0.04, 0.0, "leaf_light", sides=3)
    else:
        m.rod((x, y, -0.02), (x, y, h), 0.026, 0.02, "leaf_light", sides=m.pick(5, 3, 3))
    n = m.pick(5, 4, 2)
    for i in range(n):
        a = 360.0 * (i / n + kit.rand(seed, i) * 0.1)
        _leaf(m, (x, y, h - 0.01), a, 25 - 30 * kit.rand(seed, i + 5), 0.2, 0.045, "leaf" if i % 2 else "green")
    # Hanging bunch with the purple flower bud below it.
    bx, by = x + 0.03, y - 0.035
    if m.detail == 2:
        if bunch:
            m.octa((bx, by, h - 0.07), (0.05, 0.05, 0.07), "banana")
        return
    m.rings([((bx, by, h - 0.02), 0.04, 0.04), ((bx, by, h - 0.09), 0.055, 0.055), ((bx, by, h - 0.15), 0.0, 0.0)],
            "banana", sides=m.pick(6, 4, 4))
    if m.detail == 0:
        m.rod((bx, by, h - 0.14), (bx + 0.01, by, h - 0.2), 0.016, 0.0, "grape", sides=3)


def bananas(m):
    _banana_plant(m, -0.1, 0.08, 0.3, 91)
    _banana_plant(m, 0.14, 0.14, 0.25, 92, bunch=False)
    if m.detail == 0:
        # A cut bunch on the ground.
        with m.at(move(0.06, -0.16, 0.03), rot_z(20)):
            m.rings([((-0.07, 0, 0.0), 0.0, 0.0), ((-0.03, 0, 0.0), 0.04, 0.035), ((0.04, 0, 0.0), 0.045, 0.04),
                     ((0.09, 0, 0.0), 0.0, 0.0)], "banana", axis="x", sides=5)


def sugar(m):
    stalks = kit.scatter(93, m.pick(10, 7, 4), 0.0, 0.2, 0.06, center=(0.0, 0.08))
    for i, (x, y) in enumerate(stalks):
        h = 0.3 + 0.08 * kit.rand(94, i)
        top = (x + (kit.rand(95, i) - 0.5) * 0.05, y, h)
        m.rod((x, y, -0.02), top, 0.014, 0.011, "cane", sides=3)
        if m.detail < 2:
            for j in range(m.pick(2, 1, 0)):
                a = math.tau * (kit.rand(96, i * 3 + j))
                _blade(m, (top[0], top[1], h - 0.05), (top[0] + 0.1 * math.cos(a), top[1] + 0.1 * math.sin(a),
                                                       h + 0.04), 0.012, "leaf_light")
        else:
            m.rod((top[0], top[1], h - 0.06), (top[0], top[1], h + 0.06), 0.03, 0.0, "leaf_light", sides=3)
    if m.detail < 2:
        # Bundle of cut canes tied with a cord.
        with m.at(move(0.06, -0.2, 0.0), rot_z(-12)):
            for i, (dy, dz) in enumerate(((-0.018, 0.012), (0.018, 0.012), (0.0, 0.04))):
                if _keep(m, i, (3, 2, 0)):
                    m.rod((-0.16, dy, dz), (0.16, dy, dz), 0.015, 0.015, "cane", sides=m.pick(4, 3, 3))
            m.box((0.0, 0.0, 0.025), (0.012, 0.04, 0.03), "rope")


def _cotton_bush(m, x, y, r, seed):
    m.ball((x, y, r * 0.7), (r, r * 0.9, r * 0.75), "leaf_dark", u=m.pick(6, 5, 4), v=m.pick(3, 3, 2))
    n = m.pick(5, 3, 2)
    for i in range(n):
        a = math.tau * (i / n + kit.rand(seed, i) * 0.2)
        k = 0.55 if i else 0.0
        c = (x + math.cos(a) * r * k, y + math.sin(a) * r * k * 0.9, r * (1.35 - 0.3 * k))
        m.octa(c, (r * 0.45, r * 0.45, r * 0.38), "cotton")


def cotton(m):
    _cotton_bush(m, -0.12, 0.1, 0.11, 100)
    _cotton_bush(m, 0.13, 0.12, 0.1, 101)
    if m.detail < 2:
        _cotton_bush(m, 0.0, -0.12, 0.09, 102)
    if m.detail == 0:
        _sack(m, (-0.22, -0.14), 0.05, 0.06, "rope")
        m.ball((-0.22, -0.14, 0.065), (0.05, 0.05, 0.035), "cotton", u=5, v=3)


def _vine_row(m, y, x0, x1):
    """Trellis row: two posts, a leafy canopy along the wire and grape bunches below it."""
    if m.detail == 2:
        m.rod(((x0 + x1) / 2, y, 0.2), ((x0 + x1) / 2, y, -0.02), 0.03, 0.0, "wood", sides=3)
    else:
        for x in (x0, x1):
            m.rod((x, y, -0.02), (x, y, 0.22), 0.013, 0.011, "wood", sides=m.pick(4, 3, 3))
    if m.detail == 0:
        for k, mat in ((0.27, "leaf"), (0.73, "leaf_light")):
            m.ball((x0 + (x1 - x0) * k, y, 0.2), ((x1 - x0) * 0.3 + 0.02, 0.06, 0.07), mat, u=5, v=3)
    else:
        m.ball(((x0 + x1) / 2, y, 0.19), ((x1 - x0) / 2 + 0.03, 0.06, 0.065), "leaf", u=m.pick(6, 5, 4),
               v=m.pick(3, 3, 2))
    n = m.pick(5, 3, 1)
    for i in range(n):
        gx = x0 + (x1 - x0) * (i + 0.5) / n
        gy = y - 0.04 if i % 2 == 0 else y + 0.035
        if m.detail == 2:
            m.octa((gx, y - 0.05, 0.13), 0.045, "grape")
        else:
            kit.cone(m, (gx, gy, 0.17), 0.038, -0.1, "grape", sides=4)


def wine(m):
    _vine_row(m, 0.14, -0.22, 0.2)
    _vine_row(m, -0.04, -0.2, 0.22)
    if m.detail == 0:
        kit.barrel(m, (0.26, -0.2, -0.01), 0.05, 0.11, "wood")
        kit.cylinder(m, (0.26, -0.2, 0.04), 0.053, 0.014, "iron", sides=6)
        _sack(m, (-0.2, -0.2), 0.05, 0.05, "rope", top="grape")


def _bush_flowers(m, x, y, r, leaf, flower, seed, count=(5, 3, 1)):
    m.ball((x, y, r * 0.65), (r, r * 0.9, r * 0.75), leaf, u=m.pick(6, 5, 4), v=m.pick(3, 3, 2))
    n = m.pick(*count)
    for i in range(n):
        a = math.tau * (i / n + kit.rand(seed, i) * 0.15)
        k = 0.62 if n > 1 else 0.0
        m.octa((x + math.cos(a) * r * k, y + math.sin(a) * r * k * 0.9, r * (1.25 - 0.25 * k)), r * 0.32, flower)


def dyes(m):
    _bush_flowers(m, -0.14, 0.12, 0.11, "leaf", "dye", 120, count=(5, 2, 1))
    _bush_flowers(m, 0.12, 0.16, 0.1, "leaf", "dye", 121, count=(5, 2, 1))
    # Dye vat and a dyed cloth drying on a rack.
    if m.detail < 2:
        kit.cylinder(m, (0.04, -0.1, -0.02), 0.07, 0.09, "terracotta", sides=m.pick(7, 5, 4), r_top=0.08)
        kit.cylinder(m, (0.04, -0.1, 0.06), 0.065, 0.012, "dye", sides=m.pick(7, 5, 4))
        for x in (-0.3, -0.12):
            m.rod((x, -0.12, -0.02), (x, -0.12, 0.22), 0.008, 0.008, "wood", sides=3)
        if m.detail == 0:
            m.rod((-0.31, -0.12, 0.22), (-0.11, -0.12, 0.22), 0.007, 0.007, "wood_dark", sides=3)
        m.box((-0.21, -0.125, 0.15), (0.07, 0.004, 0.065), "purple")
    else:
        kit.cylinder(m, (0.04, -0.1, -0.02), 0.075, 0.09, "dye", sides=4)


def spices(m):
    _bush_flowers(m, -0.12, 0.14, 0.12, "leaf_dark", "red", 130, count=(6, 3, 2))
    _bush_flowers(m, 0.15, 0.16, 0.09, "leaf_dark", "spice", 131, count=(4, 2, 1))
    if m.detail < 2:
        for i, (x, y, top) in enumerate(((-0.12, -0.12, "spice"), (0.04, -0.16, "ochre"), (0.18, -0.06, "red"))):
            if _keep(m, i, (3, 2, 0)):
                _sack(m, (x, y), 0.055, 0.05, "rope" if i != 1 else "linen", top=top)


def silk(m):
    # Mulberry tree hung with white cocoons, a tray of cocoons and bolts of dyed silk.
    kit.cylinder(m, (-0.08, 0.12, -0.02), 0.03, 0.16, "bark", sides=m.pick(5, 4, 3))
    m.ball((-0.08, 0.12, 0.24), (0.16, 0.14, 0.1), "leaf", u=m.pick(7, 5, 4), v=m.pick(3, 3, 2))
    cocoons = ((-0.08, -0.01, 0.2), (-0.18, 0.02, 0.18), (0.03, 0.03, 0.18), (-0.13, 0.0, 0.27), (0.0, 0.0, 0.26),
               (0.06, 0.12, 0.2))
    for i, c in enumerate(cocoons):
        if _keep(m, i, (6, 4, 1)):
            m.octa(c, (0.024, 0.024, 0.036), "white")
    # Silk bolts: two rolls with a third on top, bright dyed colours.
    with m.at(move(0.08, -0.17, 0.0), rot_z(-8)):
        for i, (dy, dz, mat) in enumerate(((-0.037, 0.03, "lacquer"), (0.037, 0.03, "roof_blue"),
                                           (0.0, 0.088, "cream"))):
            if _keep(m, i, (3, 3, 2)):
                m.rod((-0.12, dy, dz), (0.12, dy, dz), 0.036, 0.036, mat, sides=m.pick(6, 4, 3))
    if m.detail == 0:
        kit.cylinder(m, (0.22, 0.1, -0.02), 0.065, 0.04, "rope", sides=6)
        for dx, dy in ((-0.022, -0.01), (0.02, 0.02), (0.02, -0.025)):
            m.octa((0.22 + dx, 0.1 + dy, 0.03), (0.022, 0.016, 0.016), "white")


def _incense_tree(m, x, y, h, branches):
    """Frankincense: a short crooked trunk, twisted branches with flat leaf umbrellas."""
    knee = (x + 0.04, y, h * 0.45)
    m.rod((x, y, -0.02), knee, 0.028, 0.02, "ash", sides=m.pick(5, 3, 3))
    for i, (dx, dy, dz) in enumerate(((0.08, 0.02, 0.5), (-0.1, 0.03, 0.45), (0.0, -0.08, 0.55))[:branches]):
        tip = (knee[0] + dx, knee[1] + dy, knee[2] + h * dz * 0.9)
        m.rod(knee, tip, 0.016, 0.008, "ash", sides=3)
        m.octa((tip[0], tip[1], tip[2] + 0.012), (0.075, 0.065, 0.025), "olive")


def incense(m):
    _incense_tree(m, -0.14, 0.1, 0.3, m.pick(3, 3, 1))
    if m.detail < 2:
        _incense_tree(m, 0.14, 0.16, 0.24, m.pick(3, 2, 0))
    # Censer on a tripod with smoke curling up.
    with m.at(move(0.08, -0.12, 0.0)):
        if m.detail == 0:
            for k in range(3):
                a = math.tau * k / 3
                m.rod((0.06 * math.cos(a), 0.06 * math.sin(a), -0.02), (0, 0, 0.08), 0.006, 0.006, "bronze", sides=3)
        else:
            m.rod((0, 0, 0.08), (0, 0, -0.02), 0.02, 0.0, "bronze", sides=3)
        kit.dome(m, (0, 0, 0.12), 0.05, "bronze", h=-0.045, sides=m.pick(6, 5, 4), steps=m.pick(2, 1, 1))
        for i, (dx, dz, r) in enumerate(((0.0, 0.16, 0.035), (0.03, 0.23, 0.045), (-0.01, 0.31, 0.055))):
            if _keep(m, i, (3, 2, 1)):
                m.ball((dx, 0, dz + (0.06 if m.detail == 2 else 0.0)), (r, r * 0.9, r * 0.8), "steam",
                       u=m.pick(5, 4, 4), v=m.pick(3, 3, 2))


def _fruit_tree(m, x, y, h, crown, fruits, seed, fruit_r):
    """Round-crowned tree with fruit spread over the half that faces the camera."""
    if m.detail == 2:
        m.rod((x, y, -0.02), (x, y, h * 0.5), 0.04, 0.0, "bark", sides=3)
    else:
        kit.cylinder(m, (x, y, -0.02), 0.025, h * 0.5, "bark", sides=m.pick(5, 4, 3))
    r = h * 0.42
    cz = h * 0.62
    m.ball((x, y, cz), (r, r * 0.9, r * 0.82), crown, u=m.pick(7, 5, 4), v=m.pick(4, 3, 2))
    n = len(fruits)
    for i, mat in enumerate(fruits):
        a = math.radians(-170 + 160 * (i + 0.5) / n + 20 * (kit.rand(seed, i) - 0.5))
        e = math.radians(-35 + 70 * kit.rand(seed, i + 9))
        px, py, pz = math.cos(a) * math.cos(e), math.sin(a) * math.cos(e) * 0.9, math.sin(e) * 0.82
        m.octa((x + px * r, y + py * r, cz + pz * r), fruit_r * (1.5 if m.detail == 2 else 1.0), mat)


def citrus(m):
    _fruit_tree(m, -0.12, 0.1, 0.32, "leaf", ["citrus"] * m.pick(7, 4, 1), 150, 0.032)
    _fruit_tree(m, 0.15, 0.14, 0.27, "leaf", ["citrus"] * m.pick(6, 3, 1), 151, 0.03)
    if m.detail == 0:
        _sack(m, (0.04, -0.16), 0.06, 0.05, "rope")
        for dx, dy in ((-0.022, -0.02), (0.025, 0.0), (0.0, 0.025)):
            m.octa((0.04 + dx, -0.16 + dy, 0.065), 0.026, "citrus")


def _cocoa_tree(m, x, y, h, colors):
    """Cacao: pods grow straight from the trunk under a high, flat canopy."""
    if m.detail == 2:
        m.rod((x, y, -0.02), (x, y, h * 0.62), 0.05, 0.0, "bark", sides=3)
    else:
        kit.cylinder(m, (x, y, -0.02), 0.035, h * 0.62, "bark", sides=m.pick(5, 4, 3))
    m.ball((x, y, h * 0.76), (h * 0.44, h * 0.4, h * 0.24), "leaf", u=m.pick(7, 5, 4), v=m.pick(3, 3, 2))
    pods = ((0.0, -1.0, 0.36), (-0.8, -0.6, 0.22), (0.8, -0.6, 0.27), (0.6, 0.4, 0.42))
    for i, (dx, dy, t) in enumerate(pods[:len(colors)]):
        m.octa((x + dx * 0.04, y + dy * 0.04, h * t), (0.028, 0.028, 0.05), colors[i])


def cocoa(m):
    _cocoa_tree(m, -0.12, 0.1, 0.36, ["ochre", "citrus", "lacquer", "ochre"][:m.pick(4, 2, 1)])
    _cocoa_tree(m, 0.15, 0.15, 0.32, ["citrus", "lacquer", "ochre"][:m.pick(3, 2, 1)])
    if m.detail < 2:
        # Split pods and beans in a basket.
        _sack(m, (0.06, -0.15), 0.06, 0.05, "rope", top="cocoa")
        if m.detail == 0:
            for i, (x, y) in enumerate(((-0.08, -0.16), (-0.16, -0.1))):
                with m.at(move(x, y, 0.02), rot_z(30 + 50 * i)):
                    m.octa((0, 0, 0), (0.05, 0.028, 0.028), "ochre")


def truffles(m):
    # Oak stump with gnarled roots, dug-up truffles and the pig that found them.
    kit.cylinder(m, (-0.12, 0.14, -0.02), 0.07, 0.14, "bark", sides=m.pick(6, 5, 4), r_top=0.06)
    if m.detail == 0:
        kit.cylinder(m, (-0.12, 0.14, 0.12), 0.06, 0.01, "wood", sides=6)
    if m.detail < 2:
        for k in range(m.pick(4, 3, 0)):
            a = math.radians(-60 + 80 * k)
            m.rod((-0.12, 0.14, 0.05), (-0.12 + 0.15 * math.cos(a), 0.14 + 0.13 * math.sin(a), -0.02), 0.025, 0.006,
                  "bark", sides=3)
        m.rings([((0.02, -0.05, -0.03), 0.11, 0.08), ((0.02, -0.05, 0.012), 0.1, 0.07)], "soil",
                sides=m.pick(6, 5, 4))
    for i, (x, y, r) in enumerate(((0.0, -0.05, 0.045), (0.06, -0.03, 0.038), (0.03, -0.1, 0.035),
                                   (-0.05, -0.02, 0.032))):
        if _keep(m, i, (4, 2, 1)):
            m.ball((x, y, 0.025), (r, r, r * 0.85), "black" if i % 2 else "soil_dark", u=m.pick(5, 4, 4), v=3)
    pig(m, 0.17, 0.06, 160, 0.2)


def antiquity_site(m):
    # Excavated patch with a broken column, a toppled drum, an amphora and bones.
    m.rings([((0.0, 0.0, -0.04), 0.34, 0.28), ((0.0, 0.0, 0.01), 0.32, 0.26)], "soil", sides=m.pick(8, 6, 6))
    kit.cylinder(m, (-0.12, 0.1, -0.02), 0.06, 0.24, "marble", sides=m.pick(8, 6, 5))
    if m.detail < 2:
        kit.block(m, (-0.12, 0.1, -0.02), (0.16, 0.16, 0.04), "marble")
        # Broken top: a slanted cap.
        m.loft([[(-0.18, 0.04, 0.22), (-0.06, 0.04, 0.26), (-0.06, 0.16, 0.26), (-0.18, 0.16, 0.22)],
                [(-0.16, 0.06, 0.26), (-0.08, 0.06, 0.3), (-0.08, 0.14, 0.3), (-0.16, 0.14, 0.26)]], "marble")
    m.rod((0.02, 0.18, 0.05), (0.22, 0.12, 0.05), 0.06, 0.06, "limestone", sides=m.pick(8, 6, 4))
    if m.detail < 2:
        with m.at(move(0.14, -0.08, 0.0), rot_y(-35)):
            m.rings([((0, 0, -0.01), 0.02, 0.02), ((0, 0, 0.05), 0.05, 0.05), ((0, 0, 0.11), 0.025, 0.025),
                     ((0, 0, 0.15), 0.018, 0.018)], "terracotta", sides=m.pick(6, 5, 4))
    if m.detail == 0:
        for (ax, ay), (bx, by) in (((-0.14, -0.12), (-0.02, -0.18)), ((-0.1, -0.2), (-0.04, -0.08))):
            m.rod((ax, ay, 0.02), (bx, by, 0.02), 0.012, 0.012, "ivory", sides=3)
        m.ball((0.04, -0.2, 0.03), (0.035, 0.03, 0.03), "ivory", u=5, v=3)
        m.box((0.24, -0.02, 0.015), (0.03, 0.02, 0.01), "terracotta")


# Animals on the tile -----------------------------------------------------------------------
def cattle(m):
    cow(m, -0.1, -0.06, 12, 0.27, "white", "leather")
    cow(m, 0.13, 0.14, 195, 0.25, "hide", "white")


def sheep_flock(m):
    sheep(m, -0.13, -0.06, 20, 0.2)
    sheep(m, 0.14, -0.02, 170, 0.19)
    if m.detail < 2:
        sheep(m, 0.0, 0.16, 200, 0.18)


def deer_pair(m):
    deer(m, -0.1, -0.05, 15, 0.24)
    if m.detail < 2:
        deer(m, 0.15, 0.14, 205, 0.2, antler="hide")


def bison_pair(m):
    bison(m, -0.1, -0.06, 15, 0.28)
    bison(m, 0.14, 0.14, 200, 0.25)


def ivory(m):
    elephant(m, -0.04, 0.04, -12, 0.4)


def furs(m):
    # Pelt stretched on a wooden frame, a pile of pelts and a fox.
    with m.at(move(-0.1, 0.12, 0.0), rot_z(-8)):
        for x in (-0.11, 0.11):
            m.rod((x, 0, -0.02), (x, 0, 0.28), 0.01, 0.01, "wood", sides=m.pick(4, 3, 3))
        if m.detail < 2:
            for z in (0.06, 0.26):
                m.rod((-0.13, 0, z), (0.13, 0, z), 0.008, 0.008, "wood_dark", sides=3)
            m.prism([(-0.07, 0.08), (0.07, 0.08), (0.09, 0.15), (0.06, 0.24), (-0.06, 0.24), (-0.09, 0.15)], 0.01,
                    "hide")
        else:
            m.prism([(-0.08, 0.08), (0.08, 0.08), (0.07, 0.24), (-0.07, 0.24)], 0.01, "hide")
        if m.detail == 0:
            m.prism([(-0.02, 0.12), (0.02, 0.12), (0.03, 0.2), (-0.03, 0.2)], 0.014, "leather")
    for i, (z, mat, yaw) in enumerate(((0.0, "fur", 0), (0.02, "hide", 25), (0.04, "bison", -15))):
        if _keep(m, i, (3, 1, 0)):
            with m.at(move(0.18, -0.12, z), rot_z(yaw)):
                m.prism([(-0.08, -0.05), (0.08, -0.05), (0.1, 0.0), (0.08, 0.05), (-0.08, 0.05), (-0.1, 0.0)],
                        0.02, mat, axis="z")
    fox(m, 0.02, -0.14, 160, 0.2)


# Water resources -----------------------------------------------------------------------------
def _fish(m, c, yaw, L, pitch=0.0, mat="fish", fin="slate"):
    """Fish facing +x, centred at c, nose pitched up `pitch` degrees."""
    with m.at(move(*c), rot_z(yaw), rot_y(-pitch), scale(L)):
        m.rings([((0.5, 0, 0), 0.0, 0.0), ((0.25, 0, 0), 0.14, 0.2), ((-0.15, 0, 0), 0.1, 0.15),
                 ((-0.42, 0, 0), 0.0, 0.0)], mat, axis="x", sides=m.pick(5, 4, 4))
        m.prism([(-0.35, 0.0), (-0.62, 0.22), (-0.55, 0.0), (-0.62, -0.22)], 0.03, fin)
        if m.detail < 2:
            m.prism([(0.15, 0.12), (-0.18, 0.12), (-0.12, 0.34)], 0.03, fin)


def fish(m):
    # A shoal: backs and fins breaking the surface, one fish leaping over a splash.
    _foam(m, -0.02, 0.0, 0.1)
    _fish(m, (-0.02, 0.0, 0.15), 20, 0.34, pitch=25, mat="silver")
    for i, (x, y, yaw) in enumerate(((0.2, 0.14, 200), (-0.22, -0.1, -15), (0.16, -0.16, 160), (-0.18, 0.2, 30))):
        if _keep(m, i, (4, 2, 0)):
            _fish(m, (x, y, -0.015), yaw, 0.25)
            if m.detail == 0:
                _foam(m, x, y, 0.06, sides=5)


def whales(m):
    # The back rises toward the far side, the tail fluke stands in front, the spout at the head.
    with m.at(move(0.03, 0.0, 0.0), rot_z(70)):
        body = [((0.36, 0, -0.03), 0.0, 0.0), ((0.24, 0, 0.0), 0.085, 0.065), ((0.04, 0, 0.0), 0.115, 0.08),
                ((-0.18, 0, -0.01), 0.07, 0.05), ((-0.3, 0, -0.03), 0.0, 0.0)]
        if m.detail == 2:
            body = [body[0], body[2], body[4]]
        m.rings(body, "whale", axis="x", sides=m.pick(7, 5, 4))
        if m.detail < 2:
            m.prism([(-0.06, 0.04), (-0.16, 0.03), (-0.14, 0.11)], 0.02, "whale")
            m.ball((0.04, 0, 0.0), (0.32, 0.15, 0.02), "foam", u=m.pick(6, 5, 4), v=2)
        m.rod((-0.28, 0, -0.03), (-0.38, 0, 0.13), 0.035, 0.02, "whale", sides=m.pick(5, 4, 3))
        with m.at(move(-0.38, 0, 0.13)):
            for s in (-1, 1):
                m.prism([(0.0, -0.02), (s * 0.19, 0.05), (s * 0.17, 0.1), (s * 0.02, 0.04)], 0.018, "whale",
                        axis="x")
        if m.detail == 0:
            _foam(m, -0.32, 0.0, 0.07, sides=5)
        # Spout over the blowhole.
        spout = [((0.2, 0, 0.04), 0.012, 0.012), ((0.21, 0, 0.17), 0.03, 0.03), ((0.22, 0, 0.26), 0.065, 0.065),
                 ((0.22, 0, 0.31), 0.0, 0.0)]
        if m.detail == 2:
            spout = [((0.21, 0, 0.04), 0.0, 0.0), ((0.22, 0, 0.26), 0.06, 0.06), ((0.22, 0, 0.31), 0.0, 0.0)]
        m.rings(spout, "steam", sides=m.pick(6, 5, 4))


def _oyster(m, c, r, yaw, open_deg, pearl=0.0):
    """Oyster with the hinge at the back (+y) so the open mouth faces the camera."""
    with m.at(move(*c), rot_z(yaw)):
        m.ball((0, 0, 0.0), (r, r * 0.85, r * 0.32), "rock_dark", u=m.pick(6, 5, 4), v=m.pick(3, 3, 2))
        if m.detail < 2:
            m.ball((0, 0, r * 0.2), (r * 0.85, r * 0.7, r * 0.14), "pink", u=m.pick(6, 5, 4), v=2)
        if open_deg:
            with m.at(move(0, r * 0.8, r * 0.2), rot_x(-open_deg)):
                m.ball((0, -r * 0.8, 0.0), (r * 0.95, r * 0.85, r * 0.16), "rock_dark", u=m.pick(6, 5, 4), v=2)
                if m.detail < 2:
                    m.ball((0, -r * 0.8, -r * 0.06), (r * 0.8, r * 0.7, r * 0.12), "pink", u=m.pick(6, 5, 4), v=2)
        if pearl:
            m.ball((0, -r * 0.15, r * 0.25 + pearl * 0.8), pearl, "white", u=m.pick(7, 5, 4), v=m.pick(4, 3, 3))


def pearls(m):
    kit.rock(m, (0.0, 0.08, -0.04), 0.26, "rock", seed=170, squash=0.42)
    _oyster(m, (0.0, 0.02, 0.06), 0.15, 0, 105, pearl=0.06)
    if m.detail < 2:
        _oyster(m, (0.22, -0.12, 0.0), 0.08, -25, 95, pearl=0.028 if m.detail == 0 else 0.0)
    if m.detail == 0:
        _oyster(m, (-0.22, -0.1, 0.0), 0.08, 20, 0)
        _foam(m, 0.22, -0.12, 0.11, sides=6)


def _crab(m, c, yaw, L):
    with m.at(move(*c), rot_z(yaw), scale(L)):
        m.ball((0, 0, 0.18), (0.42, 0.32, 0.16), "crab", u=m.pick(6, 5, 4), v=3)
        if m.detail == 2:
            return
        for s in (-1, 1):
            # Claws held up in front, the camera side being -y.
            arm = (0.25 * s, -0.25, 0.32)
            m.rod((0.2 * s, -0.2, 0.18), arm, 0.06, 0.05, "crab", sides=3)
            claw = (arm[0] + 0.05 * s, arm[1] - 0.12, arm[2] + 0.06)
            if m.detail == 0:
                m.ball(claw, (0.12, 0.16, 0.09), "crab", u=4, v=3)
                for k in range(3):
                    x = 0.15 + 0.16 * k
                    m.rod((s * 0.28, x - 0.1, 0.16), (s * 0.55, x + 0.05, 0.0), 0.04, 0.0, "crab", sides=3)
                m.octa((s * 0.1, -0.28, 0.32), 0.05, "black")
            else:
                m.octa(claw, (0.12, 0.16, 0.09), "crab")


def crab(m):
    kit.rock(m, (0.0, 0.06, -0.04), 0.28, "rock", seed=180, squash=0.5)
    if m.detail != 1:
        kit.rock(m, (-0.2, -0.12, -0.04), 0.12, "rock_dark", seed=181, squash=0.6)
    _crab(m, (0.0, 0.0, 0.1), 0, 0.26)
    if m.detail < 2:
        _crab(m, (0.2, -0.12, -0.02), -20, 0.2)
    if m.detail == 0:
        _foam(m, 0.22, -0.1, 0.12, sides=6)


def models():
    return [
        _model("uranium", uranium),
        _model("wheat", wheat),
        _model("cattle", cattle),
        _model("sheep", sheep_flock),
        _model("deer", deer_pair),
        _model("bananas", bananas, k=1.1),
        _model("fish", fish, "water", k=1.0),
        _model("stone", stone),
        _model("whales", whales, "water", k=0.95),
        _model("pearls", pearls, "water", k=1.0),
        _model("gold", gold),
        _model("silver", silver),
        _model("gems", gems),
        _model("marble", marble),
        _model("ivory", ivory, k=1.1),
        _model("furs", furs),
        _model("dyes", dyes),
        _model("spices", spices),
        _model("silk", silk),
        _model("sugar", sugar),
        _model("cotton", cotton),
        _model("wine", wine, k=1.1),
        _model("incense", incense, k=1.1),
        _model("copper", copper),
        _model("salt", salt, k=1.15),
        _model("crab", crab, "water", k=1.0),
        _model("truffles", truffles),
        _model("citrus", citrus),
        _model("antiquity_site", antiquity_site),
        _model("bison", bison_pair),
        _model("cocoa", cocoa),
    ]
