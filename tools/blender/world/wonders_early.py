"""World wonders of BNW, ancient to medieval (classic/docs/classic/wonders.json, kind world_wonder).
One landmark model each, drawn on a tile of the owning city's ring (wonders.py).

Every wonder stands on a hex plaza (radius 0.8-0.9, sunk to z -0.04) and keeps its parts
inside the tile hex; the front (-y) shows the characteristic facade. Shared private helpers
come first, then one section per wonder.

Ownership split: the models only classic draws. The helpers shared with the platform's models
are imported from platform/tools/blender/world/wonders_early.py.
"""
import math

from geom import move, rot_z, scale
import kit
from people import Look
from wonders import wonder as _model

from wonders_early import _column, _plaza, _ramp, _spaced


# Shared helpers -----------------------------------------------------------------------------
def _perimeter(hx, hy, nx, ny):
    """Points around a rectangle: nx along each x side, ny along each y side, corners shared."""
    pts = set()
    for x in _spaced(-hx, hx, nx):
        pts.add((round(x, 5), round(-hy, 5)))
        pts.add((round(x, 5), round(hy, 5)))
    for y in _spaced(-hy, hy, ny):
        pts.add((round(-hx, 5), round(y, 5)))
        pts.add((round(hx, 5), round(y, 5)))
    return sorted(pts)


def _temple(m, c, w, d, h, cols, r, yaw=0.0, mat="marble", roof="terracotta", cella="cream",
            frieze=None, ionic=False, inner=None, steps=2, step_h=0.022, gable=True):
    """Greek temple: stepped crepidoma, perimeter columns (plus an inner row for a dipteros:
    inner = (inset, nx, ny)), cella, entablature and a gable roof along the longer side.
    Returns the z of the entablature top (local)."""
    with m.at(move(*c), rot_z(yaw)):
        for i in range(steps):
            g = 0.026 * (steps - i)
            z0 = -0.03 if i == 0 else i * step_h
            kit.block(m, (0, 0, z0), (w + 2 * g, d + 2 * g, (i + 1) * step_h - z0), mat)
        base = steps * step_h
        hx, hy = w / 2 - r, d / 2 - r
        inset = inner[0] if inner else 0.0
        if m.detail < 2:
            cw, cd = w - 4 * r - 2 * inset - 0.04, d - 4 * r - 2 * inset - 0.04
        else:
            cw, cd = w - 2 * r, d - 2 * r
        kit.block(m, (0, 0, base), (cw, cd, h * 0.92), cella)
        pts = _perimeter(hx, hy, cols[0], cols[1])
        if m.detail == 2:
            pts = [p for p in pts if p[1] < -hy + 1e-4]
        for x, y in pts:
            _column(m, x, y, base, h, r, mat, ionic)
        if inner and m.detail < 2:
            for x, y in _perimeter(hx - inset, hy - inset, inner[1], inner[2]):
                _column(m, x, y, base, h, r, mat, ionic)
        top = base + h
        kit.block(m, (0, 0, top), (w + 0.012, d + 0.012, 0.036), mat)
        if frieze and m.detail < 2:
            kit.block(m, (0, 0, top + 0.012), (w + 0.016, d + 0.016, 0.013), frieze)
        top += 0.036
        if gable:
            if w >= d:
                kit.wedge_roof(m, (0, 0, top), w, d, d * 0.2, roof, overhang=0.01)
            else:
                kit.wedge_roof(m, (0, 0, top), d, w, w * 0.2, roof, yaw=90, overhang=0.01)
        return top


def _half_dome(m, c, r, h, mat, yaw=0.0):
    """Quarter-ellipsoid apse: flat side on the local x axis, bulging toward local -y."""
    n = m.pick(6, 4, 3)
    levels = m.pick(3, 2, 2)
    with m.at(move(*c), rot_z(yaw)):
        sections = []
        for j in range(levels):
            phi = j / levels * math.pi / 2
            rr, z = r * math.cos(phi), h * math.sin(phi)
            sections.append([(rr * math.cos(math.pi * i / n), -rr * math.sin(math.pi * i / n), z)
                             for i in range(n + 1)])
        sections.append([(0.0, 0.0, h)])
        m.loft(sections, mat)


def _lancet(m, c, w, h, mat="dark", depth=0.012, yaw=0.0):
    """Pointed gothic opening (portal, window) in the local x-z plane, centred at c."""
    hw = w / 2
    with m.at(move(*c), rot_z(yaw)):
        m.prism([(-hw, 0.0), (hw, 0.0), (hw, h - hw), (0.0, h + hw * 0.5), (-hw, h - hw)], depth, mat, axis="y")


def _crag(m, c, r, h, mat, seed):
    """Tall faceted rock pillar (cliff piece): jittered rings tapering to an uneven top."""
    n = m.pick(6, 5, 4)
    rings = []
    for j, (t, k) in enumerate(((0.0, 1.0), (0.55, 0.9), (1.0, 0.62))):
        ring = []
        for i in range(n):
            a = math.tau * (i + 0.3 * kit.rand(seed, i)) / n
            f = k * (0.85 + 0.25 * kit.rand(seed, i + 9 * j + 3))
            dz = (kit.rand(seed, i + 40) - 0.5) * h * 0.18 if j == 2 else 0.0
            ring.append((c[0] + math.cos(a) * r[0] * f, c[1] + math.sin(a) * r[1] * f,
                         c[2] + h * t + dz - (0.04 if j == 0 else 0.0)))
        rings.append(ring)
    m.loft(rings, mat)


def _bell_stupa(m, c, r, h, mat, sides=None):
    """Bell-shaped stupa with its spire as one star-shaped solid."""
    x, y, z = c
    m.rings([((x, y, z), r * 0.85, r * 0.85), ((x, y, z + h * 0.3), r, r), ((x, y, z + h * 0.65), r * 0.7, r * 0.7),
             ((x, y, z + h * 0.9), r * 0.22, r * 0.22), ((x, y, z + h * 1.3), 0.0, 0.0)], mat,
            sides=sides or m.pick(6, 4, 4))


def _lotus_tower(m, c, r, h, mat, tip="stone"):
    """Khmer prasat: square-ish shaft and a corncob of shrinking tiers to a point."""
    x, y, z = c
    sides = m.pick(8, 6, 4)
    kit.cylinder(m, (x, y, z), r, h * 0.3, mat, sides=sides)
    tiers = m.pick(5, 3, 0)
    span = h * 0.58
    z0 = z + h * 0.3
    for i in range(tiers):
        t = i / tiers
        rb = r * (1.08 - 0.82 * t ** 1.3)
        kit.cylinder(m, (x, y, z0 + span * t), rb, span / tiers, mat, sides=sides, r_top=rb * 0.8)
    if tiers:
        kit.cone(m, (x, y, z0 + span), r * 0.24, h * 0.12, tip, sides=sides)
    else:
        kit.cone(m, (x, y, z0), r * 1.05, h * 0.7, mat, sides=sides)


# Great Lighthouse ---------------------------------------------------------------------------
def great_lighthouse(m):
    """Pharos of Alexandria: square, octagonal and round tiers with a fire on top, on rocks."""
    _plaza(m, 0.8, "sand", top=0.008)
    for i, (x, y) in enumerate(kit.ring(m.pick(9, 6, 4), 0.44, 25.0)):
        kit.rock(m, (x, y, -0.02), 0.15 + 0.05 * kit.rand(3, i), "rock" if i % 3 else "rock_dark", seed=40 + i,
                 squash=0.85)
    kit.block(m, (0, 0, -0.03), (0.6, 0.6, 0.12), "stone")
    _ramp(m, 0.0, 0.12, -0.52, -0.3, -0.02, 0.09, "stone")
    m.taper((0, 0, 0.09), (0.19, 0.19), (0.15, 0.15), 0.52, "limestone")
    if m.detail == 0:
        for z in (0.24, 0.38, 0.5):
            half = 0.19 - 0.04 * (z - 0.09) / 0.52
            kit.block(m, (0, -half, z), (0.03, 0.012, 0.045), "dark")
            kit.block(m, (half, 0, z), (0.012, 0.03, 0.045), "dark")
    kit.block(m, (0, 0, 0.6), (0.36, 0.36, 0.035), "stone")
    kit.cylinder(m, (0, 0, 0.635), 0.125, 0.3, "marble", sides=m.pick(8, 8, 6), r_top=0.108)
    kit.cylinder(m, (0, 0, 0.93), 0.13, 0.025, "stone", sides=m.pick(8, 8, 6))
    kit.cylinder(m, (0, 0, 0.955), 0.075, 0.12, "marble", sides=m.pick(8, 6, 5))
    kit.cylinder(m, (0, 0, 1.07), 0.07, 0.035, "bronze", sides=m.pick(8, 6, 5), r_top=0.095)
    kit.cone(m, (0, 0, 1.1), 0.09, 0.22, "fire", sides=m.pick(7, 5, 4))
    if m.detail < 2:
        for i, (x, y) in enumerate(kit.ring(3, 0.045, 30.0)):
            m.rod((x, y, 1.1), (x * 1.6, y * 1.6, 1.3 + 0.05 * i), 0.035, 0.0, "lava", sides=4)


# Stonehenge ---------------------------------------------------------------------------------
def _trilithon(m, angle, radius, h, mat="stone"):
    a = math.radians(angle)
    x, y = radius * math.cos(a), radius * math.sin(a)
    yaw = angle + 90.0
    if m.detail == 2:
        kit.block(m, (x, y, -0.03), (0.17, 0.07, h + 0.03), mat, yaw)
        return
    for s in (-1, 1):
        kit.block(m, (x + s * 0.05 * math.cos(math.radians(yaw)), y + s * 0.05 * math.sin(math.radians(yaw)), -0.03),
                  (0.065, 0.065, h + 0.03), mat, yaw)
    kit.block(m, (x, y, h - 0.005), (0.19, 0.07, 0.05), "rock", yaw)


def stonehenge(m):
    """Ring of sarsen uprights with lintels around a horseshoe of five great trilithons."""
    _plaza(m, 0.86, "grass", top=0.01)
    m.rings([((0, 0, -0.03), 0.72, 0.72), ((0, 0, 0.018), 0.68, 0.68)], "grass_dry", sides=m.pick(16, 10, 6))
    n = m.pick(18, 12, 7)
    radius = 0.52
    missing = {n * 3 // 4, n * 3 // 4 + 1} if m.detail < 2 else set()
    for i in range(n):
        ang = 360.0 * i / n + 90.0
        a = math.radians(ang)
        if i == n * 3 // 4 and m.detail < 2:
            kit.block(m, (radius * math.cos(a), radius * math.sin(a) + 0.02, -0.02), (0.1, 0.24, 0.06), "stone",
                      ang + 20)
            continue
        h = 0.3 + 0.02 * kit.rand(7, i)
        kit.block(m, (radius * math.cos(a), radius * math.sin(a), -0.03), (0.1, 0.07, h), "stone", ang + 90 + 4 *
                  (kit.rand(8, i) - 0.5))
        if i in missing or (i + 1) % n in missing:
            continue
        mid = math.radians(ang + 180.0 / n)
        chord = 2 * radius * math.sin(math.pi / n) + 0.06
        kit.block(m, (radius * math.cos(mid), radius * math.sin(mid), 0.26), (chord, 0.08, 0.05), "rock",
                  math.degrees(mid) + 90)
    for ang, rr, h in ((90, 0.22, 0.42), (40, 0.26, 0.36), (140, 0.26, 0.36), (-5, 0.27, 0.31), (185, 0.27, 0.31)):
        if m.detail == 2 and ang in (40, 140):
            continue
        _trilithon(m, ang, rr, h)
    if m.detail < 2:
        kit.block(m, (0, 0.04, -0.02), (0.16, 0.06, 0.05), "rock_dark", 90)
        kit.rock(m, (0.18, -0.74, 0.0), 0.06, "stone", seed=3, squash=2.2)
    if m.detail == 0:
        for i, (x, y) in enumerate(kit.ring(12, 0.4, 8.0)):
            if y < -0.2:
                continue
            kit.block(m, (x, y, -0.02), (0.035, 0.03, 0.11 + 0.03 * kit.rand(9, i)), "rock", math.degrees(math.atan2(y, x)))


# Pyramids -----------------------------------------------------------------------------------
def _pyramid(m, c, half, h, yaw):
    cap = 0.16
    with m.at(move(*c), rot_z(yaw)):
        m.taper((0, 0, 0.0), (half, half), (half * cap, half * cap), h * (1 - cap), "limestone")
        m.taper((0, 0, h * (1 - cap)), (half * cap, half * cap), (1e-3, 1e-3), h * cap, "gold")


def pyramids(m):
    """Giza: the great pyramid in front, two smaller ones behind, golden caps, a sphinx."""
    _plaza(m, 0.88, "sand", top=0.01)
    _pyramid(m, (-0.38, 0.3, 0.0), 0.25, 0.5, 12)
    _pyramid(m, (0.42, 0.44, 0.0), 0.16, 0.32, 12)
    _pyramid(m, (0.12, -0.18, 0.0), 0.4, 0.74, 12)
    if m.detail < 2:
        with m.at(move(-0.52, -0.36, 0.0), rot_z(-8)):
            kit.block(m, (0, 0.03, -0.01), (0.07, 0.17, 0.05), "sandstone")
            kit.block(m, (0, -0.06, 0.03), (0.065, 0.06, 0.06), "sandstone")
            kit.block(m, (0, -0.06, 0.06), (0.08, 0.04, 0.05), "sandstone")
            if m.detail == 0:
                kit.block(m, (0, -0.1, -0.01), (0.06, 0.07, 0.02), "sandstone")
        kit.tree(m, (0.66, -0.12, 0.0), 0.2, "palm", seed=3)
        kit.tree(m, (0.58, -0.3, 0.0), 0.16, "palm", seed=8)


# Colossus -----------------------------------------------------------------------------------
def _colossus_figure(m):
    """Bronze Helios straddling the harbour: feet at local z 0 on x = +-0.33, torch raised."""
    b = "bronze"
    sides = m.pick(6, 5, 4)
    for s in (-1, 1):
        knee, hip = (s * 0.225, -0.02, 0.29), (s * 0.08, 0.0, 0.56)
        kit.block(m, (s * 0.33, -0.02, -0.005), (0.08, 0.13, 0.035), b)
        m.rod((s * 0.33, 0.0, 0.02), knee, 0.042, 0.048, b, sides=sides)
        m.rod(knee, hip, 0.048, 0.064, b, sides=sides)
    m.taper((0, 0, 0.5), (0.13, 0.07), (0.12, 0.07), 0.13, b)
    m.taper((0, 0, 0.63), (0.1, 0.06), (0.16, 0.08), 0.25, b)
    m.rod((0, 0, 0.87), (0, 0, 0.93), 0.036, 0.032, b, sides=sides)
    m.ball((0, -0.005, 0.975), (0.05, 0.055, 0.062), b, u=m.pick(7, 5, 4), v=m.pick(5, 4, 3))
    if m.detail < 2:
        for k in range(m.pick(7, 5, 0)):
            a = math.radians(-72 + 24 * k)
            base = (0.045 * math.sin(a), 0.01, 0.975 + 0.045 * math.cos(a))
            tip = (0.115 * math.sin(a), 0.015, 0.975 + 0.115 * math.cos(a))
            m.rod(base, tip, 0.014, 0.0, "gold", sides=3)
    elbow, hand = (0.23, -0.01, 1.0), (0.245, -0.02, 1.16)
    m.rod((0.15, 0.0, 0.85), elbow, 0.036, 0.031, b, sides=sides)
    m.rod(elbow, hand, 0.031, 0.026, b, sides=sides)
    m.rod((0.245, -0.02, 1.14), (0.25, -0.02, 1.22), 0.016, 0.032, "gold", sides=sides)
    kit.cone(m, (0.25, -0.02, 1.22), 0.04, 0.13, "fire", sides=m.pick(6, 5, 4))
    m.rod((-0.15, 0.0, 0.85), (-0.21, -0.02, 0.7), 0.036, 0.031, b, sides=sides)
    m.rod((-0.21, -0.02, 0.7), (-0.19, -0.08, 0.57), 0.031, 0.026, b, sides=sides)
    if m.detail < 2:
        m.taper((-0.215, -0.04, 0.3), (0.018, 0.035), (0.03, 0.05), 0.36, b, shift=(0.0, -0.02))


def colossus(m):
    """Colossus of Rhodes: bronze giant on two plinths across the harbour mouth."""
    for s in (-1, 1):
        kit.block(m, (s * 0.6, 0.06, -0.2), (0.46, 0.16, 0.27), "stone")
        kit.block(m, (s * 0.3, 0.05, -0.2), (0.21, 0.21, 0.41), "marble")
        if m.detail < 2:
            kit.block(m, (s * 0.3, 0.05, 0.17), (0.24, 0.24, 0.035), "stone")
            kit.tower(m, (s * 0.74, 0.06, 0.0), 0.065, 0.24, "stone", roof="crenel")
        if m.detail == 0:
            kit.cylinder(m, (s * 0.3, 0.05, -0.02), 0.14, 0.03, "foam", sides=8)
    with m.at(move(0.0, 0.05, 0.205), scale(0.9)):
        _colossus_figure(m)
    if m.detail < 2:
        kit.boat(m, (-0.04, -0.36, 0.0), 0.3, yaw=18)


# Oracle -------------------------------------------------------------------------------------
def _tholos(m, c):
    """Round temple of Delphi: stepped drum, ring of columns, entablature, conical roof."""
    x, y, z = c
    sides = m.pick(12, 8, 6)
    for i, r in enumerate((0.27, 0.25, 0.23)):
        kit.cylinder(m, (x, y, z - (0.04 if i == 0 else 0.0) + i * 0.022), r, 0.022 + (0.04 if i == 0 else 0.0),
                     "marble", sides=sides)
    base = z + 0.066
    kit.cylinder(m, (x, y, base), 0.15, 0.27, "cream", sides=m.pick(8, 6, 5))
    for px, py in kit.ring(m.pick(10, 8, 0), 0.205, 18.0):
        _column(m, x + px, y + py, base, 0.27, 0.017, "marble")
    if m.detail < 2:
        kit.block(m, (x, y - 0.15, base), (0.06, 0.012, 0.15), "dark")
    kit.cylinder(m, (x, y, base + 0.27), 0.23, 0.045, "marble", sides=sides)
    kit.cone(m, (x, y, base + 0.315), 0.25, 0.13, "terracotta", sides=sides)
    if m.detail < 2:
        m.ball((x, y, base + 0.45), 0.02, "gold", u=5, v=3)


def oracle(m):
    """Delphi: the tholos on a terrace under the crags of Parnassus, a tripod in the vapours."""
    _plaza(m, 0.86, "grass_dry", top=0.01)
    _crag(m, (-0.32, 0.46, 0.0), (0.3, 0.24), 0.62, "rock", seed=11)
    _crag(m, (0.2, 0.52, 0.0), (0.3, 0.22), 0.82, "stone", seed=12)
    if m.detail < 2:
        _crag(m, (0.58, 0.26, 0.0), (0.2, 0.18), 0.42, "rock", seed=13)
    kit.block(m, (0, -0.06, -0.03), (1.0, 0.56, 0.13), "stone")
    _ramp(m, -0.32, 0.14, -0.48, -0.34, -0.02, 0.1, "stone")
    _tholos(m, (0.0, -0.06, 0.1))
    if m.detail < 2:
        tx, ty, tz = 0.36, -0.2, 0.1
        for px, py in kit.ring(3, 0.035, 90.0):
            m.rod((tx + px, ty + py, tz), (tx + px * 0.5, ty + py * 0.5, tz + 0.1), 0.006, 0.006, "bronze", sides=3)
        kit.cylinder(m, (tx, ty, tz + 0.09), 0.03, 0.03, "bronze", sides=6, r_top=0.045)
        m.ball((tx, ty, tz + 0.2), (0.05, 0.045, 0.045), "steam", u=m.pick(8, 6), v=4)
        m.ball((tx + 0.03, ty + 0.02, tz + 0.29), (0.065, 0.05, 0.045), "steam", u=m.pick(8, 6), v=4)
        if m.detail == 0:
            m.ball((tx - 0.02, ty + 0.03, tz + 0.37), (0.05, 0.04, 0.035), "steam", u=8, v=4)
        for x, y in ((-0.46, -0.22), (0.44, 0.08), (-0.4, 0.06)):
            kit.tree(m, (x, y, 0.1), 0.22, "cypress", leaf="leaf_dark")


# Great Wall ---------------------------------------------------------------------------------
_WALL_PATH = ((-0.8, 0.1, 0.02), (-0.5, -0.3, 0.14), (-0.13, -0.12, 0.3), (0.2, -0.44, 0.2), (0.5, -0.12, 0.36),
              (0.5, 0.32, 0.24), (0.76, 0.42, 0.04))


def _band(a, b, half_bottom, half_top, z_bottom, height):
    """Two loft sections across the segment a-b: trapezoids from z_bottom to node z + height."""
    dx, dy = b[0] - a[0], b[1] - a[1]
    length = math.hypot(dx, dy)
    nx, ny = -dy / length, dx / length

    def section(p, hb, ht):
        return [(p[0] + nx * hb, p[1] + ny * hb, z_bottom if z_bottom is not None else p[2] - 0.02),
                (p[0] - nx * hb, p[1] - ny * hb, z_bottom if z_bottom is not None else p[2] - 0.02),
                (p[0] - nx * ht, p[1] - ny * ht, p[2] + height), (p[0] + nx * ht, p[1] + ny * ht, p[2] + height)]
    return section(a, half_bottom[0], half_top), section(b, half_bottom[1], half_top)


def _wall_tower(m, p, yaw, pavilion=False):
    x, y, z = p
    with m.at(move(x, y, z - 0.03), rot_z(yaw)):
        m.taper((0, 0, 0), (0.07, 0.07), (0.06, 0.06), 0.25, "stone")
        m.box((0, 0, 0.26), (0.068, 0.068, 0.012), "rock")
        if m.detail == 0 and not pavilion:
            for cx, cy in ((-1, -1), (-1, 1), (1, -1), (1, 1)):
                m.box((cx * 0.052, cy * 0.052, 0.285), (0.014, 0.014, 0.014), "stone")
        if pavilion and m.detail < 2:
            for cx, cy in ((-1, -1), (-1, 1), (1, -1), (1, 1)):
                m.box((cx * 0.045, cy * 0.045, 0.31), (0.008, 0.008, 0.04), "lacquer")
            kit.hip_roof(m, (0, 0, 0.35), 0.14, 0.14, 0.08, "slate", overhang=0.03)


def great_wall(m):
    """Crenellated wall with watchtowers snaking over green ridges across the tile."""
    _plaza(m, 0.86, "grass", top=0.01)
    _crag(m, (-0.3, 0.42, 0.0), (0.28, 0.24), 0.48, "rock", seed=21)
    if m.detail < 2:
        _crag(m, (0.12, 0.5, 0.0), (0.22, 0.2), 0.36, "rock_dark", seed=22)
    last = len(_WALL_PATH) - 1
    for i, (a, b) in enumerate(zip(_WALL_PATH, _WALL_PATH[1:])):
        hb = (0.06 if i == 0 else 0.17, 0.06 if i + 1 == last else 0.17)
        m.loft(list(_band(a, b, hb, 0.05, -0.04, 0.0)), "grass")
        m.loft(list(_band(a, b, (0.032, 0.032), 0.032, None, 0.1)), "stone")
        if m.detail == 0:
            dx, dy = b[0] - a[0], b[1] - a[1]
            length = math.hypot(dx, dy)
            yaw = math.degrees(math.atan2(dy, dx))
            nx, ny = -dy / length, dx / length
            count = max(int(length / 0.08), 2)
            for k in range(count):
                t = (k + 0.5) / count
                z = a[2] + (b[2] - a[2]) * t + 0.1
                for s in (-1, 1):
                    kit.block(m, (a[0] + dx * t + s * nx * 0.026, a[1] + dy * t + s * ny * 0.026, z - 0.01),
                              (0.03, 0.012, 0.035), "stone", yaw)
    for i, p in enumerate(_WALL_PATH[1:-1], start=1):
        a, b = _WALL_PATH[i - 1], _WALL_PATH[i + 1]
        if m.detail == 2 and i % 2 == 0:
            continue
        m.rings([((p[0], p[1], -0.04), 0.2, 0.2), ((p[0], p[1], p[2] - 0.01), 0.07, 0.07)], "grass",
                sides=m.pick(7, 5, 4))
        _wall_tower(m, p, math.degrees(math.atan2(b[1] - a[1], b[0] - a[0])), pavilion=i == 4)


# Angkor Wat ---------------------------------------------------------------------------------
def _gallery(m, hx, hy, z, h, mat, roof):
    """Square ring of roofed galleries (four sides) around a court."""
    t = 0.05
    for s in (-1, 1):
        kit.block(m, (0, s * hy, z), (2 * hx + t, t, h), mat)
        kit.block(m, (s * hx, 0, z), (t, 2 * hy - t, h), mat)
        if m.detail < 2:
            kit.wedge_roof(m, (0, s * hy, z + h), 2 * hx + t, t, 0.03, roof)
            kit.wedge_roof(m, (s * hx, 0, z + h), 2 * hy - t, t, 0.03, roof, yaw=90)


def angkor_wat(m):
    """Five lotus towers on a stepped galleried platform inside a moat, with a causeway."""
    _plaza(m, 0.88, "grass", top=0.01)
    kit.block(m, (0, 0, -0.03), (1.44, 0.92, 0.045), "water")
    kit.block(m, (0, 0, -0.03), (1.16, 0.74, 0.07), "grass")
    kit.block(m, (0, -0.56, -0.03), (0.1, 0.4, 0.06), "stone")
    _gallery(m, 0.5, 0.31, 0.04, 0.08, "sandstone", "rock")
    for i, (hx, hy) in enumerate(((0.32, 0.25), (0.25, 0.19), (0.17, 0.13))):
        z0 = 0.04 + 0.08 * i
        kit.block(m, (0, 0, z0 - 0.01), (2 * hx, 2 * hy, 0.09), "sandstone" if i % 2 == 0 else "stone")
    _ramp(m, 0.0, 0.08, -0.36, -0.13, 0.04, 0.28, "stone")
    _lotus_tower(m, (0, 0, 0.28), 0.088, 0.68, "stone")
    for sx in (-1, 1):
        for sy in (-1, 1):
            _lotus_tower(m, (sx * 0.2, sy * 0.15, 0.2), 0.062, 0.46, "stone")
    if m.detail < 2:
        _lotus_tower(m, (0, -0.31, 0.12), 0.04, 0.2, "stone")
        for x, y in ((-0.62, 0.2), (0.63, -0.18), (0.6, 0.25), (-0.6, -0.22)):
            kit.tree(m, (x, y, 0.04), 0.14, "palm", seed=int(x * 50 + y * 10))


# Hagia Sophia -------------------------------------------------------------------------------
def _minaret(m, x, y):
    sides = m.pick(8, 6, 4)
    kit.cylinder(m, (x, y, -0.03), 0.048, 0.14, "plaster", sides=sides)
    kit.cylinder(m, (x, y, 0.1), 0.032, 0.62, "cream", sides=sides, r_top=0.028)
    if m.detail < 2:
        for z in (0.48, 0.64):
            kit.cylinder(m, (x, y, z), 0.045, 0.016, "plaster", sides=sides)
    kit.cone(m, (x, y, 0.72), 0.034, 0.2, "slate", sides=sides)


def hagia_sophia(m):
    """Great central dome on a square block, half domes front and back, four minarets."""
    _plaza(m, 0.86, "stone", top=0.012)
    kit.block(m, (0, 0, -0.02), (0.64, 0.72, 0.24), "pink")
    if m.detail < 2:
        kit.block(m, (0, 0, 0.15), (0.65, 0.73, 0.025), "terracotta")
        kit.block(m, (0, 0, 0.215), (0.62, 0.7, 0.012), "hull_grey")
    for sx in (-1, 1):
        for sy in (-1, 1):
            kit.block(m, (sx * 0.24, sy * 0.15, 0.0), (0.12, 0.1, 0.36), "pink")
            if m.detail < 2:
                kit.hip_roof(m, (sx * 0.24, sy * 0.15, 0.36), 0.12, 0.1, 0.035, "hull_grey")
    sides = m.pick(16, 10, 8)
    kit.cylinder(m, (0, 0, 0.22), 0.2, 0.08, "pink", sides=sides)
    if m.detail == 0:
        for px, py in kit.ring(14, 0.202, 12.0):
            if py < 0.08:
                kit.block(m, (px, py, 0.24), (0.022, 0.02, 0.035), "dark", math.degrees(math.atan2(py, px)) + 90)
    kit.dome(m, (0, 0, 0.3), 0.215, "steel", h=0.16, sides=sides, steps=m.pick(4, 3, 2))
    for sy, yaw in ((-1, 0.0), (1, 180.0)):
        _half_dome(m, (0, sy * 0.19, 0.22), 0.17, 0.13, "hull_grey", yaw)
        if m.detail < 2:
            for sx in (-1, 1):
                _half_dome(m, (sx * 0.15, sy * 0.3, 0.2), 0.08, 0.07, "hull_grey", yaw)
    if m.detail < 2:
        m.rod((0, 0, 0.48), (0, 0, 0.56), 0.008, 0.004, "gold", sides=4)
    for sx in (-1, 1):
        for sy in (-1, 1):
            _minaret(m, sx * 0.5, sy * 0.44)


# Chichen Itza -------------------------------------------------------------------------------
def chichen_itza(m):
    """El Castillo: nine-terrace step pyramid with four stairways and a temple on top."""
    _plaza(m, 0.88, "grass", top=0.01)
    count, rise, half, top_half = 9, 0.064, 0.5, 0.2
    shrink = (half - top_half) / (count - 1)
    kit.steps(m, (0, 0, -0.03), half, rise, count, "limestone", shrink=shrink)
    if m.detail == 0:
        for i in range(1, count, 2):
            s = half - i * shrink + 0.004
            m.box((0, 0, -0.03 + rise * (i + 0.5)), (s, s, rise * 0.18), "stone")
    z_top = -0.03 + count * rise
    for k in range(4 if m.detail < 2 else 1):
        with m.at(rot_z(90 * k)):
            _ramp(m, 0.0, 0.15, -half - 0.1, -top_half, -0.03, z_top, "stone")
            if m.detail < 2:
                for s in (-1, 1):
                    _ramp(m, s * 0.085, 0.03, -half - 0.12, -top_half, -0.03, z_top + 0.015, "limestone")
            if k == 0 and m.detail < 2:
                for s in (-1, 1):
                    kit.block(m, (s * 0.085, -half - 0.12, -0.02), (0.045, 0.06, 0.05), "jade")
    kit.block(m, (0, 0, z_top), (0.26, 0.26, 0.13), "limestone")
    kit.block(m, (0, 0, z_top + 0.12), (0.3, 0.3, 0.025), "terracotta")
    if m.detail < 2:
        for x in (-0.07, 0.0, 0.07):
            kit.block(m, (x, -0.128, z_top), (0.04, 0.012, 0.085), "dark")
        kit.block(m, (0, 0, z_top + 0.145), (0.18, 0.18, 0.03), "limestone")
        for x, y in ((-0.66, 0.24), (0.66, 0.22), (-0.38, 0.62), (0.4, 0.6), (0.7, -0.2)):
            kit.tree(m, (x, y, 0.0), 0.2, "broadleaf", leaf="leaf_dark", seed=int(x * 100))


# Machu Picchu -------------------------------------------------------------------------------
_OCTAGON = ((1.0, -0.5), (1.0, 0.5), (0.6, 1.0), (-0.6, 1.0), (-1.0, 0.5), (-1.0, -0.5), (-0.6, -1.0), (0.6, -1.0))


def machu_picchu(m):
    """Inca citadel: grass terraces and stone houses on a ridge under the Huayna Picchu peak."""
    _plaza(m, 0.88, "grass", top=0.01)
    kit.peak(m, (0.18, 0.56, 0.0), 0.24, 1.15, "leaf_dark", snow="rock", snow_frac=0.35, seed=4,
             sides=m.pick(8, 6, 5))
    kit.peak(m, (-0.48, 0.42, 0.0), 0.22, 0.6, "rock", snow="leaf", snow_frac=0.4, seed=6, sides=m.pick(7, 5, 4))
    levels = m.pick(6, 4, 3)
    plateau = 0.42
    for i in range(levels):
        t = i / levels
        hx, hy, cy = 0.72 - 0.4 * t, 0.5 - 0.27 * t, -0.04 + 0.13 * t
        z = plateau * (i + 1) / levels
        outline = [(x * hx, cy + y * hy) for x, y in _OCTAGON]
        kit.slab(m, outline, -0.04 if i == 0 else z - plateau / levels - 0.01, z, "stone")
        if m.detail < 2:
            kit.slab(m, [(x * (hx - 0.008), cy + y * (hy - 0.008)) for x, y in _OCTAGON], z - 0.004, z + 0.012,
                     "grass")
    if m.detail < 2:
        for k, (x, y, yaw) in enumerate(((-0.2, 0.0, 0), (-0.06, -0.04, 0), (0.1, -0.03, 0), (0.24, 0.02, 10),
                                         (-0.24, 0.16, 90), (0.26, 0.17, 90), (-0.1, 0.22, 0), (0.08, 0.24, 0))):
            if m.detail == 1 and k % 2:
                continue
            kit.house(m, (x, y, plateau), 0.11, 0.08, 0.06, "stone", "wheat", yaw=yaw, roof_h=0.065, door="dark")


# Notre Dame ---------------------------------------------------------------------------------
def _buttresses(m, ys):
    for y in ys:
        for s in (-1, 1):
            kit.block(m, (s * 0.35, y, -0.02), (0.04, 0.045, 0.27), "limestone")
            m.rod((s * 0.35, y, 0.25), (s * 0.13, y, 0.31), 0.012, 0.012, "limestone", sides=4)
            if m.detail == 0:
                kit.cone(m, (s * 0.35, y, 0.25), 0.026, 0.07, "slate", sides=4)


def notre_dame(m):
    """Gothic cathedral: twin-towered west front with a rose window, flying buttresses, spire."""
    _plaza(m, 0.86, "stone", top=0.012)
    kit.block(m, (0, 0.1, -0.02), (0.26, 0.72, 0.36), "limestone")
    kit.wedge_roof(m, (0, 0.1, 0.34), 0.72, 0.26, 0.17, "slate", yaw=90)
    for s in (-1, 1):
        kit.block(m, (s * 0.21, 0.08, -0.02), (0.16, 0.6, 0.2), "limestone")
        kit.block(m, (s * 0.21, 0.08, 0.18), (0.17, 0.61, 0.02), "slate")
    kit.block(m, (0, 0.28, -0.02), (0.64, 0.17, 0.36), "limestone")
    kit.wedge_roof(m, (0, 0.28, 0.34), 0.64, 0.17, 0.17, "slate")
    kit.cylinder(m, (0, 0.46, -0.02), 0.13, 0.36, "limestone", sides=m.pick(8, 6, 4))
    kit.cone(m, (0, 0.46, 0.34), 0.14, 0.17, "slate", sides=m.pick(8, 6, 4))
    if m.detail < 2:
        _buttresses(m, (-0.14, 0.0, 0.14, 0.42) if m.detail == 0 else (-0.07, 0.42))
    # West front and towers.
    kit.block(m, (0, -0.33, -0.02), (0.48, 0.15, 0.46), "limestone")
    for s in (-1, 1):
        kit.block(m, (s * 0.155, -0.33, 0.44), (0.17, 0.15, 0.3), "limestone")
        kit.block(m, (s * 0.155, -0.33, 0.74), (0.18, 0.16, 0.02), "stone")
        if m.detail < 2:
            for dx in (-0.035, 0.035):
                _lancet(m, (s * 0.155 + dx, -0.408, 0.5), 0.04, 0.14)
    sides = m.pick(10, 8, 6)
    m.rod((0, -0.41, 0.3), (0, -0.4, 0.3), 0.088, 0.088, "stone", sides=sides)
    m.rod((0, -0.418, 0.3), (0, -0.41, 0.3), 0.07, 0.07, "gem_blue", sides=sides)
    if m.detail == 0:
        m.rod((0, -0.422, 0.3), (0, -0.418, 0.3), 0.022, 0.022, "gold", sides=6)
    if m.detail < 2:
        _lancet(m, (0, -0.41, 0.0), 0.09, 0.14)
        for s in (-1, 1):
            _lancet(m, (s * 0.155, -0.41, 0.0), 0.07, 0.12)
        kit.block(m, (0, -0.408, 0.42), (0.46, 0.012, 0.03), "stone")
    kit.block(m, (0, 0.28, 0.48), (0.07, 0.07, 0.05), "slate")
    kit.cone(m, (0, 0.28, 0.52), 0.038, 0.58, "slate", sides=m.pick(8, 6, 4))


# Statue of Zeus -----------------------------------------------------------------------------
def _zeus(m):
    """Seated gold-and-ivory Zeus on his throne, Nike in the right hand, sceptre in the left."""
    sides = m.pick(6, 5, 4)
    kit.block(m, (0, 0.05, 0.0), (0.34, 0.26, 0.17), "wood_dark")
    kit.block(m, (0, 0.17, 0.0), (0.36, 0.05, 0.56), "wood_dark")
    if m.detail < 2:
        kit.block(m, (0, 0.17, 0.56), (0.38, 0.06, 0.03), "gold")
    for s in (-1, 1):
        kit.block(m, (s * 0.065, -0.02, 0.17), (0.1, 0.22, 0.09), "gold")
        kit.block(m, (s * 0.065, -0.11, 0.0), (0.09, 0.08, 0.18), "gold")
    m.taper((0, 0.07, 0.24), (0.11, 0.075), (0.14, 0.075), 0.3, "ivory")
    if m.detail < 2:
        m.taper((-0.07, 0.07, 0.24), (0.06, 0.08), (0.05, 0.08), 0.32, "gold", shift=(-0.03, 0.0))
    m.rod((0, 0.07, 0.53), (0, 0.065, 0.6), 0.04, 0.036, "ivory", sides=sides)
    m.ball((0, 0.06, 0.64), (0.06, 0.06, 0.07), "ivory", u=m.pick(7, 5, 4), v=m.pick(5, 4, 3))
    if m.detail < 2:
        m.ball((0, 0.05, 0.6), (0.05, 0.04, 0.05), "gold", u=m.pick(6, 5), v=3)
        m.ball((0, 0.075, 0.67), (0.064, 0.055, 0.055), "gold", u=m.pick(6, 5), v=3)
        m.rod((0.14, 0.07, 0.5), (0.18, 0.0, 0.38), 0.034, 0.03, "ivory", sides=sides)
        m.rod((0.18, 0.0, 0.38), (0.18, -0.11, 0.39), 0.03, 0.026, "ivory", sides=sides)
        m.rod((-0.14, 0.07, 0.5), (-0.21, 0.03, 0.5), 0.034, 0.03, "ivory", sides=sides)
        m.rod((-0.21, 0.03, 0.5), (-0.22, -0.0, 0.62), 0.03, 0.026, "ivory", sides=sides)
    m.rod((-0.22, 0.0, 0.0), (-0.22, 0.0, 0.84), 0.011, 0.011, "gold", sides=4)
    m.octa((-0.22, 0.0, 0.87), (0.03, 0.02, 0.03), "gold")
    if m.detail < 2:
        m.taper((0.18, -0.12, 0.4), (0.015, 0.012), (0.02, 0.015), 0.06, "gold")
        m.ball((0.18, -0.12, 0.48), 0.015, "gold", u=5, v=3)
        m.prism([(0.14, 0.44), (0.22, 0.44), (0.18, 0.5)], 0.008, "gold")


def statue_of_zeus(m):
    """Olympia: the giant enthroned Zeus before his Doric temple, a dark oil pool at his feet."""
    _plaza(m, 0.86, "stone", top=0.012)
    _temple(m, (0, 0.36, 0.0), 0.74, 0.34, 0.3, (6, 3), 0.02, roof="terracotta", frieze="roof_blue")
    kit.block(m, (0, -0.06, -0.03), (0.46, 0.42, 0.15), "marble")
    if m.detail < 2:
        kit.block(m, (0, -0.06, 0.1), (0.48, 0.44, 0.025), "stone")
        kit.block(m, (0, -0.4, -0.02), (0.28, 0.1, 0.036), "obsidian")
    with m.at(move(0, -0.08, 0.12), scale(1.05)):
        _zeus(m)


# Alhambra -----------------------------------------------------------------------------------
_ALHAMBRA_WALL = ((-0.78, -0.05), (-0.5, -0.42), (0.5, -0.42), (0.78, -0.05), (0.55, 0.42), (-0.5, 0.45))


def _alhambra_court(m, z):
    """Court of the Myrtles: long pool between hedges, white arcades, palace wings."""
    kit.block(m, (0.05, 0.0, z - 0.01), (0.34, 0.52, 0.025), "marble")
    kit.block(m, (0.05, 0.02, z - 0.005), (0.12, 0.4, 0.025), "water")
    if m.detail < 2:
        for s in (-1, 1):
            kit.block(m, (0.05 + s * 0.09, 0.02, z), (0.03, 0.38, 0.035), "leaf_dark")
    kit.block(m, (0.05, -0.27, z), (0.34, 0.06, 0.12), "plaster")
    if m.detail == 0:
        for x in _spaced(-0.09, 0.19, 5):
            kit.block(m, (x, -0.302, z), (0.04, 0.008, 0.08), "dark")
    for x in (-0.17, 0.27):
        kit.block(m, (x, 0.0, z), (0.1, 0.48, 0.12), "plaster")
        kit.hip_roof(m, (x, 0.0, z + 0.12), 0.1, 0.48, 0.05, "terracotta")


def alhambra(m):
    """Red fortress palace on its hill: crenellated walls, square towers, the myrtle court pool."""
    _plaza(m, 0.88, "grass", top=0.01)
    z = 0.07
    m.loft([[(x * 1.08, y * 1.08, -0.04) for x, y in _ALHAMBRA_WALL], [(x, y, z) for x, y in _ALHAMBRA_WALL]],
           "grass_dry")
    pts = list(_ALHAMBRA_WALL)
    for a, b in zip(pts, pts[1:] + pts[:1]):
        kit.wall_line(m, (a[0], a[1], z - 0.02), b, 0.16, 0.05, "rock_red")
    for i, (x, y) in enumerate(pts):
        h = 0.26 if i != 0 else 0.34
        kit.block(m, (x, y, z - 0.02), (0.12, 0.12, h), "rock_red")
        if m.detail < 2:
            kit.block(m, (x, y, z - 0.02 + h), (0.135, 0.135, 0.02), "terracotta")
    _alhambra_court(m, z)
    kit.block(m, (0.05, 0.36, z - 0.02), (0.22, 0.16, 0.46), "rock_red")
    kit.block(m, (-0.58, 0.12, z - 0.02), (0.16, 0.14, 0.38), "rock_red")
    if m.detail < 2:
        kit.block(m, (0.05, 0.36, z + 0.44), (0.235, 0.175, 0.025), "terracotta")
        kit.block(m, (-0.58, 0.12, z + 0.36), (0.175, 0.155, 0.025), "terracotta")
        kit.flag(m, (-0.58, 0.12, z + 0.38), 0.12)
        kit.block(m, (0.55, 0.05, z), (0.24, 0.24, 0.1), "plaster")
        kit.hip_roof(m, (0.55, 0.05, z + 0.1), 0.24, 0.24, 0.06, "terracotta")
        for x, y in ((-0.4, -0.25), (-0.3, -0.3), (-0.38, 0.28), (0.5, -0.28), (0.62, -0.2)):
            kit.tree(m, (x, y, z), 0.2, "cypress", leaf="leaf_dark")


# Great Mosque of Djenne ---------------------------------------------------------------------
def _toron_grid(m, x, y, z0, rows, cols, dx=0.04):
    """Wooden palm beams sticking out of a mud wall facing -y."""
    for r in range(rows):
        for c in range(cols):
            px = x + (c - (cols - 1) / 2) * dx
            pz = z0 + r * 0.08
            m.rod((px, y + 0.01, pz), (px, y - 0.045, pz), 0.007, 0.007, "wood", sides=3)


def _mud_tower(m, x, y, h, half=0.075):
    m.taper((x, y, 0.06), (half, half * 0.8), (half * 0.7, half * 0.55), h, "mudbrick")
    kit.cone(m, (x, y, 0.06 + h), half * 0.72, half * 1.3, "mudbrick", sides=m.pick(6, 5, 4))
    if m.detail < 2:
        m.ball((x, y, 0.06 + h + half * 1.3), 0.018, "cream", u=5, v=3)


def great_mosque_of_djenne(m):
    """Mud-brick mosque: three conical towers on the front, pilasters, wooden torons."""
    _plaza(m, 0.86, "sand", top=0.01)
    kit.block(m, (0, 0.06, -0.03), (1.0, 0.82, 0.1), "soil")
    _ramp(m, 0.0, 0.2, -0.46, -0.35, -0.02, 0.07, "soil")
    kit.block(m, (0, 0.1, 0.06), (0.82, 0.58, 0.26), "mudbrick")
    for x, h in ((-0.26, 0.42), (0.0, 0.5), (0.26, 0.42)):
        _mud_tower(m, x, -0.22, h)
        if m.detail == 0:
            _toron_grid(m, x, -0.27, 0.14, 4 if h > 0.45 else 3, 3)
    pilasters = [(-0.41, -0.19), (-0.13, -0.2), (0.13, -0.2), (0.41, -0.19), (-0.41, 0.1), (0.41, 0.1),
                 (-0.41, 0.38), (0.41, 0.38), (0.0, 0.39)]
    if m.detail == 2:
        pilasters = pilasters[:4]
    for x, y in pilasters:
        m.taper((x, y, 0.06), (0.026, 0.026), (0.018, 0.018), 0.32, "mudbrick")
        if m.detail < 2:
            kit.cone(m, (x, y, 0.38), 0.018, 0.04, "mudbrick", sides=4)
    if m.detail == 0:
        for x in _spaced(-0.34, 0.34, 8):
            if min(abs(x - t) for t in (-0.26, 0.0, 0.26, -0.13, 0.13)) < 0.05:
                continue
            m.rod((x, -0.18, 0.2), (x, -0.235, 0.2), 0.006, 0.006, "wood", sides=3)
        for y in (-0.05, 0.25):
            for s in (-1, 1):
                for z in (0.14, 0.24):
                    m.rod((s * 0.41, y, z), (s * 0.46, y, z), 0.006, 0.006, "wood", sides=3)
        for x in _spaced(-0.36, 0.36, 9):
            kit.cone(m, (x, 0.38, 0.32), 0.016, 0.045, "mudbrick", sides=4)
        kit.tree(m, (-0.62, -0.2, 0.0), 0.24, "palm", seed=2)
        kit.tree(m, (0.64, -0.12, 0.0), 0.2, "palm", seed=9)


# Petra --------------------------------------------------------------------------------------
def _khazneh(m):
    """Al-Khazneh facade in the local frame: front face toward -y at y 0, base at z 0."""
    s = "sandstone"
    kit.block(m, (0, 0.06, -0.02), (0.56, 0.08, 0.88), s)
    kit.block(m, (0, -0.02, -0.02), (0.52, 0.12, 0.06), s)
    for x in _spaced(-0.22, 0.22, m.pick(6, 4, 2)):
        _column(m, x, -0.02, 0.04, 0.3, 0.02, s)
    kit.block(m, (0, -0.01, 0.34), (0.54, 0.1, 0.05), s)
    m.prism([(-0.17, 0.39), (0.17, 0.39), (0.0, 0.47)], 0.07, s)
    if m.detail < 2:
        kit.block(m, (0, 0.02, 0.04), (0.08, 0.02, 0.18), "black")
    kit.cylinder(m, (0, 0.0, 0.47), 0.07, 0.25, s, sides=m.pick(8, 6, 4))
    kit.cone(m, (0, 0.0, 0.72), 0.085, 0.08, s, sides=m.pick(8, 6, 4))
    if m.detail < 2:
        m.ball((0, 0.0, 0.83), (0.025, 0.025, 0.035), s, u=5, v=3)
        for x in _spaced(-0.06, 0.06, 3):
            if x:
                _column(m, x, -0.06, 0.47, 0.22, 0.012, s)
    for sx in (-1, 1):
        kit.block(m, (sx * 0.19, 0.0, 0.47), (0.12, 0.08, 0.24), s)
        with m.at(move(sx * 0.19, 0.0, 0.71)):
            m.prism([(-0.06 * sx, 0.0), (0.07 * sx, 0.0), (0.07 * sx, 0.06)], 0.09, s)
        if m.detail == 0:
            for dx in (-0.04, 0.04):
                _column(m, sx * 0.19 + dx, -0.05, 0.47, 0.22, 0.012, s)


def petra(m):
    """Rock-cut treasury facade set into a red sandstone cliff, a camel in the canyon."""
    _plaza(m, 0.86, "sand", top=0.01)
    _crag(m, (-0.52, 0.22, 0.0), (0.3, 0.32), 1.0, "rock_red", seed=31)
    _crag(m, (0.52, 0.22, 0.0), (0.3, 0.32), 0.95, "rock_red", seed=32)
    _crag(m, (0.0, 0.52, 0.0), (0.34, 0.24), 1.15, "rock_red", seed=33)
    if m.detail < 2:
        _crag(m, (-0.62, -0.22, 0.0), (0.16, 0.16), 0.5, "rock_red", seed=34)
        _crag(m, (0.64, -0.2, 0.0), (0.15, 0.15), 0.42, "rock_red", seed=35)
    with m.at(move(0, 0.12, 0.0)):
        _khazneh(m)
    if m.detail < 2:
        kit.quadruped(m, (0.24, -0.42, 0.0), 200, "camel", length=0.14, height=0.07, hump=True)
        kit.person(m, (0.12, -0.46, 0.0), 0, Look(body="linen", legs="linen", robe=True, head="hood",
                                                     head_mat="linen"), 0.11)


# Terracotta Army ----------------------------------------------------------------------------
def _warrior(m, x, y, armour):
    if m.detail == 0:
        m.taper((x, y, 0.01), (0.017, 0.013), (0.021, 0.015), 0.05, "terracotta")
        m.taper((x, y, 0.06), (0.022, 0.015), (0.027, 0.017), 0.045, armour)
    else:
        m.taper((x, y, 0.01), (0.018, 0.014), (0.026, 0.017), 0.095, armour)
    m.octa((x, y, 0.12), (0.016, 0.016, 0.019), "terracotta")


def terracotta_army(m):
    """Qin pit: rows of terracotta warriors between earthen walls, a hall at the back."""
    _plaza(m, 0.86, "soil", top=0.01)
    kit.block(m, (0, -0.06, -0.03), (1.16, 0.62, 0.046), "soil_dark")
    for s in (-1, 1):
        kit.block(m, (0, -0.06 + s * 0.31, -0.03), (1.22, 0.05, 0.1), "soil")
        kit.block(m, (s * 0.6, -0.06, -0.03), (0.05, 0.62, 0.1), "soil")
    rows = (-0.27, -0.13, 0.01, 0.15)
    for y in (-0.2, -0.06, 0.08):
        kit.block(m, (0, y, -0.03), (1.12, 0.03, 0.08), "soil")
    per_row = m.pick(10, 6, 0)
    for r, y in enumerate(rows):
        for k in range(per_row):
            x = -0.48 + 0.96 * k / (per_row - 1)
            if r == 0 and abs(x) < 0.12:
                continue
            _warrior(m, x, y, "brick" if (k + r) % 2 else "rust")
    if m.detail == 2:
        for y in rows:
            m.box((0, y, 0.05), (0.5, 0.025, 0.06), "terracotta")
    elif m.detail == 0:
        for s in (-1, 1):
            kit.quadruped(m, (s * 0.03, -0.3, 0.015), -90, "terracotta", length=0.11, height=0.055)
        kit.block(m, (0, -0.2, 0.015), (0.09, 0.06, 0.05), "wood_dark")
    _hall(m, (0, 0.52, 0.0))


def _hall(m, c):
    """Chinese hall: stone podium, red lacquer columns, double hipped roof."""
    x, y, z = c
    kit.block(m, (x, y, z - 0.03), (0.74, 0.22, 0.09), "stone")
    kit.block(m, (x, y, z + 0.06), (0.6, 0.12, 0.17), "lacquer")
    if m.detail < 2:
        for px in _spaced(-0.33, 0.33, m.pick(8, 5)):
            kit.cylinder(m, (x + px, y - 0.09, z + 0.06), 0.012, 0.17, "lacquer", sides=4)
    kit.hip_roof(m, (x, y, z + 0.23), 0.72, 0.22, 0.09, "slate", overhang=0.03)
    kit.block(m, (x, y, z + 0.29), (0.42, 0.1, 0.06), "lacquer")
    kit.hip_roof(m, (x, y, z + 0.35), 0.46, 0.14, 0.1, "slate", overhang=0.03)
    if m.detail < 2:
        kit.block(m, (x, y, z + 0.44), (0.22, 0.02, 0.025), "gold")


# Borobudur ----------------------------------------------------------------------------------
def borobudur(m):
    """Stepped mandala: square galleries, round terraces ringed with bell stupas, a crowning stupa."""
    _plaza(m, 0.88, "grass", top=0.01)
    rise = 0.056
    for i, half in enumerate((0.55, 0.48, 0.42, 0.36, 0.31)):
        z0 = -0.03 if i == 0 else -0.03 + i * rise
        kit.block(m, (0, 0, z0), (2 * half, 2 * half, -0.03 + (i + 1) * rise - z0), "stone" if i % 2 == 0 else "rock")
    z_sq = -0.03 + 5 * rise
    for k in range(4 if m.detail < 2 else 1):
        with m.at(rot_z(90 * k)):
            _ramp(m, 0.0, 0.07, -0.6, -0.3, -0.03, z_sq + 0.004, "rock_dark")
    rings = ((0.29, 0.25, m.pick(14, 8, 0)), (0.225, 0.185, m.pick(10, 6, 0)), (0.16, 0.12, m.pick(7, 4, 0)))
    z = z_sq
    for r, ring_r, count in rings:
        kit.cylinder(m, (0, 0, z - 0.005), r, 0.045, "stone", sides=m.pick(16, 10, 8))
        z += 0.04
        for x, y in kit.ring(count, ring_r, 360.0 / max(count, 1) / 2):
            _bell_stupa(m, (x, y, z), 0.034, 0.08, "rock")
    _bell_stupa(m, (0, 0, z), 0.1, 0.19, "stone", sides=m.pick(10, 8, 6))
    if m.detail < 2:
        for x, y in ((-0.66, 0.2), (0.66, 0.2), (-0.62, -0.3), (0.64, -0.3)):
            kit.tree(m, (x, y, 0.0), 0.22, "palm", seed=int(x * 30 + y * 7))


# Parthenon ----------------------------------------------------------------------------------
_ACROPOLIS = ((-0.8, -0.15), (-0.55, -0.5), (0.5, -0.52), (0.8, -0.12), (0.78, 0.2), (0.45, 0.5), (-0.45, 0.52),
              (-0.8, 0.2))


def parthenon(m):
    """Doric temple on the Acropolis rock, with the Propylaea gate and Athena's bronze statue."""
    _plaza(m, 0.88, "grass_dry", top=0.01)
    top = 0.28
    m.loft([[(x, y, -0.04) for x, y in _ACROPOLIS],
            [(x * 0.86, y * 0.86, top * 0.6) for x, y in _ACROPOLIS],
            [(x * 0.8, y * 0.8 + 0.01, top) for x, y in _ACROPOLIS]], "rock")
    m.loft([[(x * 0.77, y * 0.77 + 0.01, top - 0.01) for x, y in _ACROPOLIS],
            [(x * 0.77, y * 0.77 + 0.01, top + 0.008) for x, y in _ACROPOLIS]], "limestone")
    _temple(m, (0.1, 0.06, top), 0.4, 0.66, 0.26, (8, 12), 0.016, yaw=-20, roof="stone", cella="cream",
            frieze="terracotta")
    _ramp(m, -0.42, 0.12, -0.56, -0.36, -0.03, top + 0.005, "limestone")
    if m.detail < 2:
        with m.at(move(-0.42, -0.3, top)):
            kit.block(m, (0, 0, 0), (0.2, 0.1, 0.03), "marble")
            for x in _spaced(-0.075, 0.075, 4):
                _column(m, x, -0.03, 0.03, 0.14, 0.012, "marble")
            kit.block(m, (0, 0, 0.17), (0.2, 0.1, 0.025), "marble")
            kit.wedge_roof(m, (0, 0, 0.195), 0.1, 0.2, 0.04, "marble", yaw=90)
        kit.statue(m, (-0.28, -0.08, top), 0.24, "bronze", "marble",
                   Look(body="bronze", legs="bronze", skin="bronze", boots="bronze", belt="bronze", head="crest",
                        head_mat="bronze", right="spear", left="hoplon", robe=True))
        for x, y in ((-0.7, -0.32), (0.66, -0.4), (0.72, 0.3)):
            kit.tree(m, (x, y, 0.0), 0.15, "broadleaf", leaf="olive", seed=int(x * 40))


# Temple of Artemis --------------------------------------------------------------------------
def temple_of_artemis(m):
    """Ephesus: huge Ionic dipteros, a double forest of slender columns on a wide stepped base."""
    _plaza(m, 0.86, "grass", top=0.01)
    _temple(m, (0.0, 0.02, 0.0), 1.02, 0.52, 0.38, (11, 5), 0.017, yaw=12, roof="terracotta", cella="cream",
            frieze="roof_blue", ionic=True, inner=(0.075, 10, 3), steps=3, step_h=0.024)
    if m.detail < 2:
        with m.at(rot_z(12)):
            _ramp(m, 0.0, 0.3, -0.4, -0.31, -0.02, 0.072, "marble")
            for s in (-1, 1):
                m.octa((s * 0.53, 0.0, 0.62), (0.02, 0.02, 0.035), "gold")
        kit.statue(m, (-0.2, -0.56, 0.0), 0.18, "bronze", "marble",
                   Look(body="bronze", legs="bronze", skin="bronze", boots="bronze", belt="bronze", right="bow",
                        robe=True))


# Mausoleum of Halicarnassus -----------------------------------------------------------------
def _quadriga(m, z):
    for x in _spaced(-0.06, 0.06, m.pick(4, 2, 0)):
        kit.quadruped(m, (x, -0.05, z), -90, "bronze", length=0.11, height=0.055, neck_up=0.45)
    if m.detail < 2:
        kit.block(m, (0, 0.07, z), (0.12, 0.06, 0.05), "bronze")
        kit.person(m, (0, 0.08, z + 0.03), 0, Look(body="bronze", legs="bronze", skin="bronze", boots="bronze",
                                                   belt="bronze", robe=True), 0.12)
    else:
        kit.block(m, (0, 0.0, z), (0.12, 0.14, 0.08), "bronze")


def mausoleum_of_halicarnassus(m):
    """High podium, Ionic colonnade, stepped pyramid roof crowned by a bronze quadriga."""
    _plaza(m, 0.8, "stone", top=0.012)
    kit.block(m, (0, 0, -0.03), (0.72, 0.62, 0.07), "stone")
    kit.block(m, (0, 0, 0.04), (0.58, 0.48, 0.36), "limestone")
    if m.detail < 2:
        kit.block(m, (0, 0, 0.3), (0.6, 0.5, 0.04), "terracotta")
        kit.block(m, (0, 0, 0.12), (0.595, 0.495, 0.02), "marble")
    top = _temple(m, (0, 0, 0.4), 0.52, 0.42, 0.22, (6, 5), 0.016, mat="marble", cella="cream", ionic=True,
                  steps=1, step_h=0.02, gable=False)
    z = 0.4 + top
    count = m.pick(8, 5, 3)
    rise = 0.24 / count
    for i in range(count):
        t = i / count
        hx, hy = 0.25 - 0.15 * t, 0.2 - 0.12 * t
        kit.block(m, (0, 0, z + i * rise - 0.002), (2 * hx, 2 * hy, rise + 0.002), "stone" if i % 2 else "marble")
    z += 0.24
    _quadriga(m, z + 0.01)


def models():
    return [
        _model("great_lighthouse", great_lighthouse, "upright", "land"),  # classical
        _model("stonehenge", stonehenge, "upright", "land"),  # ancient
        _model("pyramids", pyramids, "upright", "land"),  # ancient
        _model("colossus", colossus, "water", "water"),  # classical
        _model("oracle", oracle, "upright", "land"),  # classical
        _model("great_wall", great_wall, "upright", "land"),  # classical
        _model("angkor_wat", angkor_wat, "upright", "land"),  # medieval
        _model("hagia_sophia", hagia_sophia, "upright", "land"),  # medieval
        _model("chichen_itza", chichen_itza, "upright", "land"),  # medieval
        _model("machu_picchu", machu_picchu, "upright", "land"),  # medieval
        _model("notre_dame", notre_dame, "upright", "land"),  # medieval
        _model("statue_of_zeus", statue_of_zeus, "upright", "land"),  # ancient
        _model("alhambra", alhambra, "upright", "land"),  # medieval
        _model("great_mosque_of_djenne", great_mosque_of_djenne, "upright", "land"),  # medieval
        _model("petra", petra, "upright", "land"),  # classical
        _model("terracotta_army", terracotta_army, "upright", "land"),  # classical
        _model("borobudur", borobudur, "upright", "land"),  # medieval
        _model("parthenon", parthenon, "upright", "land"),  # classical
        _model("temple_of_artemis", temple_of_artemis, "upright", "land"),  # ancient
        _model("mausoleum_of_halicarnassus", mausoleum_of_halicarnassus, "upright", "land"),  # ancient
    ]
