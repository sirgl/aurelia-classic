"""Tile improvements of Civ V BNW (classic/docs/classic/improvements.json), one model each, plus
terrain variants of the farm.

Layout rule shared with resources.py: the yard (radius 0.30 around the tile anchor) stays
low (under 0.08) so the resource prop of an improved resource stands there; buildings of
the improvement keep to the ring 0.32..0.95. Fields reach from 0.32 to the hex edge.

Ownership split: the models only classic draws. The helpers shared with the platform's models
are imported from platform/tools/blender/world/improvements.py.
"""
import math

from geom import move, rot_y, rot_z, scale
import kit

from improvements import (
    _bale, _band, _beam, _cannon, _figure, _lattice, _logs, _model, _polar, _shrub, _sloped_slab, _spike)

# Shared parts -------------------------------------------------------------------------------
def _sector(m, a0, a1, r0, r1, z0, z1, mat, slope=0.0):
    """Annulus sector between angles a0 < a1 (degrees) as one convex slab."""
    def quad(ra, rb, z):
        return [(*_polar(r, a), z) for r, a in ((ra, a0), (ra, a1), (rb, a1), (rb, a0))]
    m.loft([quad(r0, r1, z0), quad(r0 + slope, r1 - slope, z1)], mat)


def _hex_half_width(apothem, y):
    """Half width of a pointy-top hex of the given apothem at height y."""
    return max(0.0, min(apothem, (apothem - 0.866 * abs(y)) / 0.5))


def _campfire(m, c, s=1.0, smoke=False):
    x, y = c
    if m.detail < 2:
        kit.cylinder(m, (x, y, -0.015), 0.055 * s, 0.03, "rock_dark", sides=m.pick(7, 5, 4))
    m.rod((x, y, 0.005), (x, y, 0.12 * s), 0.042 * s, 0.0, "fire", sides=m.pick(5, 4, 3))
    if m.detail == 0:
        m.rod((x + 0.02 * s, y - 0.015 * s, 0.01), (x + 0.025 * s, y - 0.02 * s, 0.08 * s), 0.02 * s, 0.0, "lava",
              sides=3)
        if smoke:
            m.ball((x + 0.03, y + 0.02, 0.2 * s), (0.04, 0.035, 0.03), "steam", u=5, v=3)


# Quarry -----------------------------------------------------------------------------------
def quarry(m):
    # Pale working floor and three cut benches stepping up around the back of the pit.
    kit.hex_patch(m, 0.74, "limestone", z0=-0.04, z1=0.006)
    spans = m.pick(3, 3, 2)
    for i, mat in enumerate(("stone", "rock", "rock_dark")):
        r0 = 0.44 + 0.13 * i
        for j in range(spans):
            a0 = 12 + 156 * j / spans
            _sector(m, a0, a0 + 156 / spans, r0, r0 + 0.14, -0.04, 0.075 * (i + 1), mat)
    # Cut blocks waiting at the front, one stacked.
    blocks = [(-0.56, -0.12, 0.10, 20), (-0.38, -0.40, 0.09, -15), (-0.14, -0.58, 0.08, 35), (0.20, -0.58, 0.08, 5)]
    for i, (x, y, s, yaw) in enumerate(blocks[:m.pick(4, 3, 1)]):
        kit.block(m, (x, y, -0.01), (s * 1.3, s, s), "limestone" if i % 2 == 0 else "sandstone", yaw)
    if m.detail == 0:
        kit.block(m, (-0.55, -0.11, 0.09), (0.09, 0.07, 0.07), "sandstone", 40)
        kit.block(m, (-0.18, 0.60, 0.15), (0.12, 0.08, 0.07), "limestone", 8)
    _derrick_crane(m)


def _derrick_crane(m):
    """Timber derrick at the right: mast, boom over the benches, a block on the rope."""
    foot, top = (0.58, -0.18, -0.02), (0.58, -0.18, 0.50)
    tip = (0.36, 0.30, 0.46)
    m.rod(foot, top, 0.016, 0.012, "wood", sides=m.pick(4, 4, 3))
    m.rod((0.58, -0.18, 0.06), tip, 0.012, 0.009, "wood", sides=m.pick(4, 3, 3))
    if m.detail == 2:
        return
    _beam(m, top, tip, 0.004, "rope", sides=3)
    _beam(m, tip, (0.36, 0.30, 0.24), 0.004, "rope", sides=3)
    kit.block(m, (0.36, 0.30, 0.17), (0.09, 0.07, 0.07), "limestone", 20)
    if m.detail == 0:
        for g in ((0.84, -0.30), (0.76, 0.18)):
            _beam(m, top, (g[0], g[1], -0.01), 0.004, "rope", sides=3)
        kit.block(m, (0.58, -0.18, -0.02), (0.08, 0.08, 0.04), "wood_dark", 0)


# Plantation -------------------------------------------------------------------------------
def plantation(m):
    for k in range(6):
        with m.at(move(*_polar(0.55, 60 * k), 0.0), rot_z(60 * k)):
            _bush_parcel(m, k)
    _drying_shed(m, (0.0, 0.80))


def _bush_parcel(m, k):
    """Soil bed with two rows of shrubs running toward the hex edge."""
    shade = "leaf_dark" if k % 2 == 0 else "leaf"
    if m.detail == 2:
        kit.patch(m, (0.0, 0.0), (0.16, 0.17), shade, z1=0.03)
        return
    kit.patch(m, (0.0, 0.0), (0.16, 0.18), "soil", z1=0.01)
    for row in (-0.085, 0.085):
        if m.detail == 1:
            m.ball((0.0, row, 0.04), (0.15, 0.05, 0.05), shade, u=4, v=2)
            continue
        for i, x in enumerate((-0.105, 0.0, 0.105)):
            h = 0.09 + 0.02 * kit.rand(k * 7 + int(row * 100), i)
            _shrub(m, (x, row), 0.05, h, shade)


def _drying_shed(m, c):
    x, y = c
    if m.detail == 2:
        kit.block(m, (x, y, -0.01), (0.24, 0.13, 0.1), "wood")
        kit.wedge_roof(m, (x, y, 0.09), 0.26, 0.15, 0.07, "grass_dry")
        return
    kit.block(m, (x, y + 0.05, -0.01), (0.24, 0.03, 0.12), "wood")
    for sx in (-1, 1):
        _beam(m, (x + sx * 0.11, y - 0.06, -0.01), (x + sx * 0.11, y - 0.06, 0.12), 0.008, "wood_dark", sides=3)
    kit.wedge_roof(m, (x, y, 0.11), 0.26, 0.16, 0.08, "grass_dry")
    if m.detail == 0:
        for i in range(3):
            kit.block(m, (x - 0.07 + 0.07 * i, y, 0.05), (0.035, 0.08, 0.05), "leaf_light" if i != 1 else "wheat")
        kit.cylinder(m, (x + 0.17, y - 0.10, -0.01), 0.03, 0.04, "wheat", sides=6)


# Camp -------------------------------------------------------------------------------------
def camp(m):
    if m.detail < 2:
        kit.hex_patch(m, 0.2, "soil", z0=-0.03, z1=0.004, center=(0.34, -0.26))
    tents = [((-0.46, 0.42), 0.17, 0.38, "hide"), ((0.10, 0.66), 0.16, 0.36, "leather"), ((0.56, 0.32), 0.15, 0.32, "hide")]
    for (x, y), r, h, mat in tents:
        _tipi(m, (x, y), r, h, mat)
    with m.at(move(-0.56, -0.16, 0.0), scale(1.3)):
        _hide_rack(m, (0.0, 0.0))
    _campfire(m, (0.34, -0.26), 1.0, smoke=True)
    if m.detail < 2:
        for a in (130, 250):
            px, py = _polar(0.11, a)
            _bale(m, (0.34 + px, -0.26 + py, -0.01), 0.018, 0.1, "wood", yaw=a + 90)
        for a in (0, 120, 240):
            px, py = _polar(0.07, a)
            _beam(m, (0.34 + px, -0.26 + py, -0.01), (0.34, -0.26, 0.17), 0.005, "wood_dark", sides=3)
    if m.detail == 0:
        _figure(m, (0.20, -0.34), body="leather")
        _canoe(m, (0.10, -0.62), 0.34)


def _tipi(m, c, r, h, mat):
    x, y = c
    kit.tent(m, (x, y, 0.0), r, h, mat)
    if m.detail < 2:
        for a in (60, 160, 280)[:m.pick(3, 2)]:
            px, py = _polar(r * 0.12, a)
            _spike(m, (x + px, y + py, h * 0.82), (x + px * 4, y + py * 4, h + 0.07), 0.006, "wood_dark")
    if m.detail == 0:
        with m.at(move(x, y - r * 0.62, 0.0)):
            m.prism([(-0.035, -0.005), (0.035, -0.005), (0.0, h * 0.42)], 0.01, "leather" if mat == "hide" else "hide")


def _hide_rack(m, c):
    x, y = c
    if m.detail < 2:
        for sx in (-1, 1):
            _beam(m, (x + sx * 0.13, y, -0.01), (x + sx * 0.13, y, 0.19), 0.008, "wood_dark", sides=3)
        _beam(m, (x - 0.15, y, 0.18), (x + 0.15, y, 0.18), 0.007, "wood_dark", sides=3)
    for i, mat in enumerate(("hide", "fur")):
        with m.at(move(x - 0.065 + 0.13 * i, y, 0.11)):
            m.prism([(0.0, -0.075), (0.058, -0.01), (0.04, 0.065), (-0.04, 0.065), (-0.058, -0.01)], 0.008, mat)


def _canoe(m, c, length):
    with m.at(move(c[0], c[1], 0.0)):
        L = length / 2
        m.loft([[(-L, 0.0, 0.035)], [(-L * 0.5, -0.03, 0.035), (-L * 0.5, 0.03, 0.035), (-L * 0.5, 0.0, -0.005)],
                [(L * 0.5, -0.03, 0.035), (L * 0.5, 0.03, 0.035), (L * 0.5, 0.0, -0.005)], [(L, 0.0, 0.035)]], "bark")


# Offshore platform ------------------------------------------------------------------------
def offshore_platform(m):
    cx, cy, deck = 0.0, 0.56, 0.22
    legs = [(sx * 0.24, cy + sy * 0.12) for sy in (-1, 1) for sx in (-1, 1)]
    for x, y in legs:
        m.rod((x, y, -0.07), (x, y, deck), 0.032, 0.03, "ochre", sides=m.pick(6, 5, 4))
    if m.detail == 0:
        for sx in (-1, 1):
            _beam(m, (sx * 0.24, cy - 0.12, 0.0), (-sx * 0.24, cy - 0.12, 0.16), 0.008, "ochre", sides=3)
    m.box((cx, cy, deck), (0.32, 0.18, 0.025), "hull_grey")
    _lattice(m, (0.12, cy + 0.04, deck + 0.025), 0.075, 0.022, 0.6, "steel", levels=m.pick(3, 2, 0), r=0.009)
    kit.block(m, (-0.16, cy + 0.02, deck + 0.02), (0.22, 0.2, 0.12), "white")
    kit.block(m, (-0.16, cy + 0.02, deck + 0.14), (0.18, 0.16, 0.06), "red")
    if m.detail < 2:
        kit.cylinder(m, (-0.16, cy + 0.02, deck + 0.20), 0.11, 0.012, "dark", sides=m.pick(8, 6))
        _beam(m, (0.30, cy - 0.10, deck), (0.62, cy - 0.30, deck + 0.24), 0.01, "steel", sides=3)
    m.rod((0.62, cy - 0.30, deck + 0.22), (0.63, cy - 0.31, deck + 0.38), 0.05, 0.0, "fire", sides=m.pick(5, 4, 3))
    if m.detail == 0:
        m.rod((0.625, cy - 0.31, deck + 0.22), (0.63, cy - 0.31, deck + 0.31), 0.026, 0.0, "lava", sides=4)
        kit.cylinder(m, (-0.16, cy + 0.02, deck + 0.212), 0.05, 0.004, "citrus", sides=6)
        kit.cylinder(m, (0.24, cy + 0.14, deck + 0.025), 0.03, 0.08, "ochre", sides=6)
        _beam(m, (0.24, cy + 0.14, deck + 0.1), (0.46, cy + 0.26, deck + 0.22), 0.01, "ochre", sides=3)
        for x, y in legs:
            kit.cylinder(m, (x, y, -0.02), 0.05, 0.024, "foam", sides=6)


# Citadel ----------------------------------------------------------------------------------
def citadel(m):
    """Massive hexagonal stone fortress: high curtain walls, round corner towers, a keep."""
    heights = {0: 0.2, 1: 0.24, 2: 0.24, 3: 0.2, 4: 0.13, 5: 0.13}
    for k in range(6):
        _band(m, k, 0.52, 0.64, -0.04, heights[k], "stone", trim=0.02)
        if m.detail == 0 and k in (0, 3, 4, 5):
            for t in (-0.12, 0.0, 0.12):
                p = (_polar(0.6, 60 * k)[0] - math.sin(math.radians(60 * k)) * t,
                     _polar(0.6, 60 * k)[1] + math.cos(math.radians(60 * k)) * t)
                kit.block(m, (p[0], p[1], heights[k]), (0.06, 0.05, 0.03), "stone", 60 * k + 90)
    towers = [30, 150, 210, 330, 270] if m.detail < 2 else [30, 150]
    for a in towers:
        x, y = _polar(0.70, a)
        h = 0.2 if a in (210, 270, 330) else 0.3
        kit.cylinder(m, (x, y, -0.03), 0.1, h + 0.03, "stone", sides=m.pick(7, 5, 4))
        if m.detail == 0:
            kit.cylinder(m, (x, y, h), 0.115, 0.04, "rock", sides=7)
    _keep(m, _polar(0.66, 90))
    if m.detail < 2:
        x, y = _polar(0.62, 270)
        kit.block(m, (x, y - 0.06, -0.02), (0.12, 0.04, 0.12), "rock_dark")
        kit.block(m, (x, y - 0.08, -0.02), (0.06, 0.012, 0.08), "black")


def _keep(m, c):
    x, y = c
    kit.block(m, (x, y, -0.03), (0.24, 0.22, 0.56), "rock")
    if m.detail < 2:
        kit.block(m, (x, y, 0.53), (0.27, 0.25, 0.04), "stone")
    if m.detail == 0:
        for sx in (-1, 1):
            for sy in (-1, 1):
                kit.block(m, (x + sx * 0.1, y + sy * 0.09, 0.57), (0.06, 0.06, 0.04), "stone")
        for z in (0.24, 0.40):
            kit.block(m, (x, y - 0.112, z), (0.03, 0.006, 0.06), "black")
    kit.flag(m, (x, y, 0.57), 0.22, size=0.12)


# Landmark ---------------------------------------------------------------------------------
def landmark(m):
    """Great-work monument: a bronze statue on a stepped marble plinth, flanked by obelisks."""
    if m.detail < 2:
        kit.patch(m, (0.0, 0.40), (0.58, 0.13), "limestone", z1=0.008)
    kit.steps(m, (0.0, 0.58, -0.02), 0.24, 0.05, 3, "marble")
    _bronze_statue(m, (0.0, 0.58, 0.13), 0.56)
    for sx in (-1, 1):
        x = sx * 0.56
        if m.detail < 2:
            kit.block(m, (x, 0.34, -0.02), (0.12, 0.12, 0.06), "marble")
        base = 0.04 if m.detail < 2 else -0.02
        m.taper((x, 0.34, base), (0.042, 0.042), (0.026, 0.026), 0.5 - base, "limestone")
        kit.pyramid(m, (x, 0.34, 0.5), 0.026, 0.06, "gold")


def _bronze_statue(m, base, h):
    """Robed figure raising a torch; a cheap proxy of the same pose below LOD0."""
    import people
    x, y, z = base
    if m.detail == 0:
        look = people.Look(body="bronze", legs="bronze", skin="bronze", boots="bronze", belt="bronze", robe=True,
                           head="laurel", head_mat="bronze", right="torch", left="book")
        with m.at(move(x, y, z), scale(h)):
            people.figure(m, look)
        return
    m.taper((x, y, z), (0.09 * h, 0.07 * h), (0.06 * h, 0.045 * h), 0.8 * h, "bronze")
    m.ball((x, y, z + 0.88 * h), 0.075 * h, "bronze", u=m.pick(6, 5, 4), v=3)
    if m.detail == 1:
        m.rod((x + 0.12 * h, y, z + 0.7 * h), (x + 0.18 * h, y, z + 1.12 * h), 0.03 * h, 0.025 * h, "bronze", sides=3)
        m.rod((x + 0.18 * h, y, z + 1.12 * h), (x + 0.18 * h, y, z + 1.22 * h), 0.04 * h, 0.0, "fire", sides=4)


# Academy ----------------------------------------------------------------------------------
def academy(m):
    """Domed hall of learning with a columned portico facing the camera."""
    with m.at(move(0.0, 0.56, 0.0), scale(1.22)):
        _academy_hall(m)
    if m.detail == 0:
        for sx in (-1, 1):
            kit.tree(m, (sx * 0.52, 0.28, 0.0), 0.3, "cypress", leaf="leaf_dark", seed=sx + 9)


def _academy_hall(m):
    kit.block(m, (0.0, 0.0, -0.03), (0.62, 0.32, 0.06), "limestone")
    kit.block(m, (0.0, -0.19, -0.03), (0.34, 0.08, 0.04), "limestone")
    kit.block(m, (0.0, 0.04, 0.03), (0.54, 0.2, 0.17), "cream")
    kit.block(m, (0.0, 0.04, 0.2), (0.56, 0.22, 0.016), "marble")
    cols = m.pick(6, 4, 0)
    for i in range(cols):
        x = -0.15 + 0.3 * i / (cols - 1)
        kit.cylinder(m, (x, -0.12, 0.03), 0.018, 0.16, "marble", sides=m.pick(6, 4))
    if m.detail < 2:
        kit.block(m, (0.0, -0.08, 0.19), (0.38, 0.1, 0.025), "marble")
        with m.at(move(0.0, -0.1, 0.215)):
            m.prism([(-0.19, 0.0), (0.19, 0.0), (0.0, 0.075)], 0.06, "marble")
    else:
        kit.block(m, (0.0, -0.1, 0.03), (0.34, 0.06, 0.18), "marble")
    kit.cylinder(m, (0.0, 0.04, 0.216), 0.12, 0.06, "marble", sides=m.pick(10, 8, 6))
    kit.dome(m, (0.0, 0.04, 0.276), 0.13, "copper_green", h=0.13)
    if m.detail < 2:
        kit.cylinder(m, (0.0, 0.04, 0.4), 0.022, 0.04, "marble", sides=m.pick(6, 4))
        kit.cone(m, (0.0, 0.04, 0.44), 0.028, 0.05, "gold", sides=m.pick(6, 4))
    if m.detail == 0:
        for i in range(3):
            kit.block(m, (-0.17 + 0.17 * i, -0.063, 0.03), (0.035, 0.006, 0.08), "wood_dark")


# Manufactory ------------------------------------------------------------------------------
def manufactory(m):
    """Brick factory hall with a sawtooth roof facing the camera and tall chimneys."""
    hx, hy, half_w, half_d, h = 0.0, 0.58, 0.36, 0.15, 0.14
    kit.block(m, (hx, hy, -0.02), (2 * half_w, 2 * half_d, h + 0.02), "brick")
    teeth = m.pick(4, 4, 3)
    width = 2 * half_w / teeth
    for i in range(teeth):
        x0 = -half_w + width * i
        with m.at(move(0.0, hy, h)):
            m.prism([(x0, 0.0), (x0 + width, 0.0), (x0 + width, 0.1)], 2 * half_d, "slate")
        if m.detail < 2:
            kit.block(m, (x0 + width - 0.006, hy, h), (0.008, 2 * half_d - 0.02, 0.085), "sky_glass")
    for x, y, ch in ((-0.22, 0.74, 0.70), (0.06, 0.76, 0.78))[:m.pick(2, 2, 2)]:
        kit.chimney(m, (x, y, h - 0.02), 0.042, ch - h, "brick", smoke=m.detail == 0)
        if m.detail == 0:
            kit.cylinder(m, (x, y, ch - 0.04), 0.048, 0.03, "dark", sides=6)
    if m.detail < 2:
        kit.block(m, (hx, hy - half_d - 0.003, -0.01), (0.1, 0.008, 0.09), "dark")
        kit.house(m, (0.54, 0.20, -0.01), 0.18, 0.14, 0.1, "brick", "slate", kind="hip", roof_h=0.06)
        kit.rock(m, (-0.52, 0.12, -0.02), 0.13, "coal", seed=3, squash=0.5)
    if m.detail == 0:
        for sx in (-1, 1):
            kit.block(m, (sx * 0.21, hy - half_d - 0.003, 0.07), (0.2, 0.006, 0.035), "glass_dark")
        kit.chimney(m, (0.58, 0.24, 0.12), 0.025, 0.14, "brick", smoke=False)
        for x, y in ((-0.40, -0.34), (-0.30, -0.40)):
            kit.crate(m, (x, y, -0.01), 0.08, "wood", 15)


# Archaeological dig -----------------------------------------------------------------------
def archaeological_dig(m):
    _trench(m, (-0.50, 0.20), (0.36, 0.26), 15)
    _trench(m, (0.20, -0.58), (0.30, 0.18), -5)
    kit.rock(m, (-0.62, -0.20, -0.03), 0.14, "soil", seed=4, squash=0.45)
    _dig_tent(m, (0.42, 0.56))
    if m.detail < 2:
        kit.cylinder(m, (-0.44, 0.24, -0.01), 0.04, 0.05, "terracotta", sides=m.pick(6, 5), r_top=0.025)
        _bale(m, (-0.58, 0.16, -0.01), 0.03, 0.1, "marble", yaw=30)
    if m.detail == 0:
        _wheelbarrow(m, (0.52, -0.20), 30)
        _beam(m, (-0.60, -0.16, 0.02), (-0.66, -0.12, 0.16), 0.005, "wood", sides=3)
        kit.block(m, (-0.67, -0.115, 0.15), (0.03, 0.008, 0.035), "iron", -60)
        kit.cylinder(m, (-0.30, -0.10, -0.01), 0.025, 0.04, "iron", sides=5)
        _figure(m, (-0.36, 0.38), body="khaki")


def _trench(m, c, size, yaw):
    """Dark excavated floor framed by spoil walls, with a string grid over it."""
    x, y = c
    sx, sy = size
    with m.at(move(x, y, 0.0), rot_z(yaw)):
        m.box((0, 0, -0.02), (sx / 2, sy / 2, 0.024), "soil_dark")
        if m.detail == 2:
            return
        wall = 0.035
        for s in (-1, 1):
            m.box((0, s * (sy / 2 + wall / 2), 0.0), (sx / 2 + wall, wall / 2, 0.045), "soil")
            m.box((s * (sx / 2 + wall / 2), 0, 0.0), (wall / 2, sy / 2, 0.045), "soil")
        if m.detail == 0:
            for i in (1, 2):
                m.box((-sx / 2 + sx * i / 3, 0, 0.048), (0.003, sy / 2 + 0.01, 0.003), "white")
            m.box((0, 0, 0.048), (sx / 2 + 0.01, 0.003, 0.003), "white")


def _dig_tent(m, c):
    x, y = c
    with m.at(move(x, y, 0.0), rot_z(-10)):
        m.loft([[(-0.14, -0.12, -0.01), (-0.14, 0.12, -0.01), (-0.14, 0.0, 0.17)],
                [(0.14, -0.12, -0.01), (0.14, 0.12, -0.01), (0.14, 0.0, 0.17)]], "linen")
        if m.detail == 0:
            m.loft([[(-0.142, -0.05, 0.0), (-0.142, 0.05, 0.0), (-0.142, 0.0, 0.09)],
                    [(-0.148, -0.05, 0.0), (-0.148, 0.05, 0.0), (-0.148, 0.0, 0.09)]], "khaki")
            kit.block(m, (0.0, -0.2, -0.01), (0.14, 0.07, 0.05), "wood")


def _wheelbarrow(m, c, yaw):
    with m.at(move(c[0], c[1], 0.0), rot_z(yaw)):
        m.taper((0, 0, 0.03), (0.04, 0.035), (0.055, 0.045), 0.035, "rust")
        m.rod((0.06, -0.012, 0.025), (0.06, 0.012, 0.025), 0.025, 0.025, "dark", sides=6)
        for s in (-1, 1):
            _beam(m, (0.05, s * 0.03, 0.035), (-0.12, s * 0.04, 0.05), 0.005, "wood", sides=3)


# Barbarian encampment ---------------------------------------------------------------------
def encampment(m):
    """Palisade of sharpened stakes around crude hide tents, a skull pole and a fire."""
    count = m.pick(34, 20, 0)
    for i in range(count):
        a = 280 + 340.0 * i / (count - 1)
        if 255 < a % 360 < 285:
            continue
        r = 0.80 + 0.02 * kit.rand(31, i)
        x, y = _polar(r, a)
        h = 0.15 + 0.05 * kit.rand(7, i)
        _spike(m, (x, y, -0.02), (x, y, h), m.pick(0.024, 0.032), "wood")
    if m.detail == 2:
        for k in range(6):
            _band(m, k, 0.78, 0.82, -0.02, 0.15, "wood", trim=0.02 if k not in (4, 5) else 0.1)
    for (x, y), r, h, mat in (((-0.42, 0.44), 0.13, 0.22, "fur"), ((0.16, 0.58), 0.15, 0.24, "hide"),
                              ((0.54, 0.22), 0.11, 0.19, "leather")):
        _tipi(m, (x, y), r, h, mat)
    _skull_pole(m, (-0.30, -0.42))
    _campfire(m, (0.30, -0.34), 1.1, smoke=True)
    if m.detail < 2:
        for s in (-1, 1):
            x, y = _polar(0.84, 270 + s * 14)
            _beam(m, (x, y, -0.02), (x, y, 0.26), 0.016, "wood_dark", sides=4)
        kit.flag(m, (-0.58, 0.02, 0.0), 0.32, mat="black", size=0.1)
    if m.detail == 0:
        _bale(m, (-0.60, -0.24, -0.01), 0.025, 0.14, "bark", yaw=40)


def _skull_pole(m, c):
    x, y = c
    m.rod((x, y, -0.02), (x, y, 0.26), 0.01, 0.008, "wood_dark", sides=m.pick(4, 3, 3))
    m.ball((x, y, 0.27), (0.03, 0.028, 0.03), "ivory", u=m.pick(6, 5, 4), v=m.pick(4, 3, 2))
    if m.detail < 2:
        for s in (-1, 1):
            m.rod((x + s * 0.02, y, 0.28), (x + s * 0.07, y, 0.33), 0.008, 0.0, "ivory", sides=3)
    if m.detail == 0:
        for s in (-1, 1):
            m.box((x + s * 0.011, y - 0.026, 0.275), (0.007, 0.004, 0.007), "black")
        m.ball((x, y, 0.19), (0.022, 0.02, 0.022), "ivory", u=5, v=3)


# Brazilwood camp --------------------------------------------------------------------------
def brazilwood_camp(m):
    """Jungle logging camp: palm-thatch shelter, stacks of red brazilwood logs, stumps."""
    kit.grove(m, [(-0.42, 0.62), (0.42, 0.62), (-0.62, 0.18)][:m.pick(3, 2, 1)], 0.44, "palm", leaf="leaf_dark",
              seed=6)
    if m.detail < 2:
        kit.grove(m, [(0.66, 0.12), (0.0, 0.80)][:m.pick(2, 1)], 0.36, "broadleaf", leaf="leaf_dark", seed=11)
    if m.detail == 0:
        for x, y, s in ((-0.24, 0.76, 0.08), (0.66, -0.12, 0.08), (-0.70, -0.10, 0.07)):
            _shrub(m, (x, y), s, s * 1.3, "leaf")
    _shelter(m, (0.02, 0.56))
    _logs(m, (-0.48, -0.06), 0.32, 0.035, "red", yaw=25, count=3)
    _logs(m, (0.46, -0.20), 0.28, 0.032, "red", yaw=-30, count=m.pick(3, 3, 1))
    if m.detail < 2:
        _logs(m, (-0.04, -0.58), 0.3, 0.03, "bark", yaw=5, count=m.pick(3, 2))
    if m.detail == 0:
        for x, y in ((-0.30, -0.40), (0.26, -0.50), (0.62, 0.36)):
            kit.cylinder(m, (x, y, -0.01), 0.034, 0.045, "red", sides=5)
        _figure(m, (0.30, -0.30), body="linen")


def _shelter(m, c):
    x, y = c
    if m.detail < 2:
        for sx in (-1, 1):
            for sy in (-1, 1):
                _beam(m, (x + sx * 0.12, y + sy * 0.07, -0.01), (x + sx * 0.12, y + sy * 0.07, 0.13), 0.009,
                      "wood_dark", sides=3)
        kit.block(m, (x, y, -0.01), (0.2, 0.1, 0.03), "red")
    kit.wedge_roof(m, (x, y, 0.12), 0.3, 0.2, 0.1, "grass_dry", overhang=0.02)


# Kasbah -----------------------------------------------------------------------------------
def kasbah(m):
    """Mud-brick compound: plain walls between tapered crenellated towers, a tall keep."""
    front, back, half = 0.36, 0.72, 0.42
    walls = [((-half, back), (half, back), 0.2), ((-half, front), (-half, back), 0.18),
             ((half, front), (half, back), 0.18), ((-half, front), (half, front), 0.14)]
    for a, b, h in walls:
        kit.wall_line(m, (a[0], a[1], -0.02), b, h + 0.02, 0.05, "mudbrick", crenels=False)
    towers = [(-half, front, 0.30), (half, front, 0.30), (-half, back, 0.34), (half, back, 0.34)]
    for x, y, h in towers:
        _kasbah_tower(m, (x, y), 0.075, h)
    _kasbah_tower(m, (0.0, 0.60), 0.1, 0.5)
    if m.detail < 2:
        kit.block(m, (0.0, front - 0.028, -0.01), (0.09, 0.008, 0.11), "wood_dark")
        kit.block(m, (0.0, front - 0.028, 0.10), (0.12, 0.01, 0.03), "ochre")
    if m.detail == 0:
        kit.grove(m, [(-0.62, -0.02), (0.64, 0.02)], 0.36, "palm", leaf="leaf", seed=8)


def _kasbah_tower(m, c, half, h):
    x, y = c
    m.taper((x, y, -0.02), (half, half), (half * 0.8, half * 0.8), h + 0.02, "mudbrick")
    if m.detail == 0:
        top = half * 0.8
        for sx in (-1, 1):
            for sy in (-1, 1):
                kit.block(m, (x + sx * top * 0.7, y + sy * top * 0.7, h), (top * 0.55, top * 0.55, 0.035), "mudbrick")
        for z in (h * 0.45, h * 0.72):
            kit.block(m, (x, y - half * (1 - 0.2 * z / h) - 0.002, z), (0.018, 0.006, 0.04), "wood_dark")
    elif m.detail == 1:
        kit.block(m, (x, y, h), (half * 1.7, half * 1.7, 0.025), "mudbrick")


# Feitoria ---------------------------------------------------------------------------------
def feitoria(m):
    """Portuguese coastal trading fort: whitewashed walls round a court, a red-roofed
    factor's house, a round tower with a red cone, a corner bastion and the flag."""
    front, back, left, right = 0.34, 0.70, -0.50, 0.42
    walls = [((left, front), (right, front)), ((left, front), (left, back - 0.02)), ((right, front), (right, back - 0.1))]
    for a, b in walls:
        kit.wall_line(m, (a[0], a[1], -0.02), b, 0.12, 0.045, "white", crenels=False)
        if m.detail == 0:
            kit.wall_line(m, (a[0], a[1], 0.1), b, 0.014, 0.055, "terracotta", crenels=False)
    kit.house(m, (-0.06, 0.60, -0.02), 0.42, 0.2, 0.17, "white", "terracotta", kind="hip", roof_h=0.11)
    kit.tower(m, (right, front + 0.04, -0.02), 0.09, 0.34, "white", roof="cone", roof_mat="terracotta", roof_h=0.15,
              sides=m.pick(8, 6, 4))
    _sloped_slab(m, [(left - 0.13, front - 0.05), (left - 0.02, front + 0.1), (left + 0.1, front - 0.02),
                     (left - 0.01, front - 0.16)], -0.02, 0.14, "white", 0.9)
    if m.detail < 2:
        kit.block(m, (-0.04, front - 0.025, -0.01), (0.09, 0.012, 0.09), "wood_dark")
        for sx in (-1, 1):
            kit.block(m, (-0.06 + sx * 0.13, 0.497, 0.07), (0.05, 0.008, 0.05), "roof_blue")
        kit.flag(m, (0.20, 0.66, 0.0), 0.52, size=0.14)
    if m.detail == 0:
        _cannon(m, (left - 0.03, front - 0.04), 225)
        for x, y, s in ((0.40, -0.30, 0.08), (0.50, -0.22, 0.07)):
            kit.crate(m, (x, y, -0.01), s, "wood", 20)
        kit.barrel(m, (0.30, -0.40, -0.01), 0.035, 0.07, "wood_dark")


# Chateau ----------------------------------------------------------------------------------
def chateau(m):
    """French chateau: cream main block under steep slate roofs, round towers with cones,
    and a formal parterre of clipped hedges on both sides of the yard."""
    with m.at(move(0.0, 0.62, 0.0), scale(1.2)):
        _chateau_house(m)
    for sx in (-1, 1):
        _parterre(m, (sx * 0.54, 0.0))


def _chateau_house(m):
    kit.block(m, (0.0, 0.0, -0.02), (0.46, 0.17, 0.18), "cream")
    kit.hip_roof(m, (0.0, 0.0, 0.16), 0.46, 0.17, 0.14, "slate")
    if m.detail < 2:
        kit.block(m, (0.0, -0.04, -0.02), (0.14, 0.14, 0.22), "cream")
        kit.hip_roof(m, (0.0, -0.04, 0.2), 0.14, 0.14, 0.16, "slate")
    for sx in (-1, 1):
        kit.tower(m, (sx * 0.27, -0.03, -0.02), 0.075, 0.24, "cream", roof="cone", roof_mat="slate", roof_h=0.2,
                  sides=m.pick(8, 6, 4))
    if m.detail == 0:
        for sx in (-1, 1):
            kit.block(m, (sx * 0.14, -0.086, 0.06), (0.05, 0.006, 0.06), "glass_dark")
            kit.block(m, (sx * 0.14, -0.07, 0.17), (0.04, 0.03, 0.04), "cream")
            kit.wedge_roof(m, (sx * 0.14, -0.07, 0.21), 0.04, 0.03, 0.03, "slate", yaw=90)
        kit.block(m, (0.0, -0.111, -0.01), (0.04, 0.006, 0.07), "wood_dark")


def _parterre(m, c):
    x, y = c
    kit.patch(m, (x, y), (0.16, 0.24), "sand", z1=0.006)
    if m.detail == 2:
        kit.patch(m, (x, y), (0.12, 0.2), "leaf_dark", z1=0.03)
        return
    for sy in (-1, 1):
        kit.block(m, (x, y + sy * 0.11, 0.0), (0.22, 0.12, 0.035), "leaf_dark")
    if m.detail == 0:
        kit.cone(m, (x, y, 0.0), 0.035, 0.13, "leaf", sides=6)
        for sy in (-1, 1):
            kit.cone(m, (x, y + sy * 0.21, 0.0), 0.025, 0.09, "leaf", sides=5)


# Moai -------------------------------------------------------------------------------------
def moai(m):
    """Row of four moai on an ahu platform, all facing the camera."""
    kit.block(m, (0.0, 0.54, -0.03), (1.1, 0.22, 0.1), "stone")
    if m.detail < 2:
        kit.block(m, (0.0, 0.40, -0.03), (1.0, 0.07, 0.05), "rock")
    for x, h, hat in ((-0.39, 0.58, False), (-0.13, 0.66, True), (0.13, 0.62, False), (0.39, 0.56, True)):
        with m.at(move(x, 0.56, 0.07), scale(1.3)):
            _moai(m, (0.0, 0.0, 0.0), h / 1.3, hat)


def _moai(m, c, h, hat):
    x, y, z = c
    torso, head = h * 0.4, h * 0.6
    m.taper((x, y, z), (0.075, 0.05), (0.068, 0.046), torso, "ash")
    hz = z + torso
    m.taper((x, y + 0.004, hz), (0.058, 0.05), (0.066, 0.056), head, "ash", shift=(0.0, 0.004))
    if m.detail < 2:
        m.box((x, y - 0.05, hz + head * 0.72), (0.066, 0.02, 0.022), "ash")
        m.taper((x, y - 0.062, hz + head * 0.2), (0.016, 0.02), (0.01, 0.01), head * 0.48, "ash")
    if m.detail == 0:
        for s in (-1, 1):
            m.box((x + s * 0.03, y - 0.052, hz + head * 0.6), (0.018, 0.004, 0.014), "obsidian")
            m.box((x + s * 0.066, y + 0.0, hz + head * 0.5), (0.006, 0.02, head * 0.3), "ash")
        m.box((x, y - 0.054, hz + head * 0.08), (0.04, 0.008, 0.012), "rock_dark")
    if hat and m.detail < 2:
        kit.cylinder(m, (x, y + 0.006, z + h - 0.004), 0.055, 0.05, "rock_red", sides=m.pick(7, 5))


# Polder -----------------------------------------------------------------------------------
def polder(m):
    """Dutch polder: a dike ring, a ring canal inside it, ditch-cut fields and a windmill."""
    kit.hex_patch(m, 0.74, "water" if m.detail < 2 else "leaf_light", z0=-0.04, z1=0.006)
    for k in range(6):
        _band(m, k, 0.73, 0.86, -0.04, 0.07, "grass", slope=0.035 if m.detail < 2 else 0.065)
    fields = m.pick(5, 4, 0)
    for i in range(fields):
        span = 1.2 / fields
        y0 = -0.6 + span * i + 0.02
        y1 = y0 + span - 0.04
        half = _hex_half_width(0.66, max(abs(y0), abs(y1))) - 0.02
        crop = ("leaf_light", "grass", "wheat", "leaf_light", "grass")[i % 5]
        kit.patch(m, (0.0, (y0 + y1) / 2), (half, (y1 - y0) / 2), crop, z1=0.016)
    with m.at(move(0.40, 0.44, 0.0), scale(1.35)):
        _windmill(m, (0.0, 0.0))
    if m.detail == 0:
        for x, y in ((-0.50, 0.56), (-0.36, 0.66), (-0.64, 0.44)):
            kit.tree(m, (x, y, 0.0), 0.26, "cypress", leaf="leaf", seed=int(x * 10))


def _windmill(m, c):
    x, y = c
    m.rings([((x, y, -0.02), 0.1, 0.1), ((x, y, 0.32), 0.065, 0.065)], "wood_dark", sides=m.pick(8, 6, 4))
    m.rings([((x, y, 0.32), 0.075, 0.075), ((x, y, 0.37), 0.06, 0.06), ((x, y, 0.4), 0.0, 0.0)], "slate",
            sides=m.pick(8, 6, 4))
    if m.detail == 0:
        kit.cylinder(m, (x, y, 0.12), 0.14, 0.012, "wood", sides=8)
        kit.block(m, (x, y - 0.1, -0.01), (0.04, 0.006, 0.07), "black")
    hub = (x, y - 0.09, 0.35)
    if m.detail == 2:
        for i in range(2):
            with m.at(move(*hub), rot_y(45 + 90 * i)):
                m.box((0.0, 0.0, 0.0), (0.02, 0.004, 0.25), "linen")
        return
    for i in range(4):
        with m.at(move(*hub), rot_y(45 + 90 * i)):
            if m.detail == 0:
                m.box((0.0, 0.0, 0.13), (0.008, 0.006, 0.13), "wood")
            m.box((0.024, 0.004, 0.15), (0.02, 0.003, 0.1), "linen")


def models():
    return [
        _model("quarry", quarry, "plane"),
        _model("plantation", plantation, "plane"),
        _model("camp", camp),
        _model("offshore-platform", offshore_platform, "water"),
        _model("citadel", citadel),
        _model("landmark", landmark),
        _model("academy", academy),
        _model("manufactory", manufactory),
        _model("archaeological-dig", archaeological_dig, "plane"),
        _model("encampment", encampment),
        _model("brazilwood-camp", brazilwood_camp),
        _model("kasbah", kasbah),
        _model("feitoria", feitoria),
        _model("chateau", chateau),
        _model("moai", moai),
        _model("polder", polder, "plane"),
    ]
