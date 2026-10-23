"""Pieces of the modular city kit (city_kit.py): per era a center and three houses, the
walls ring, building landmarks and national wonders. Every piece is a world model
(kit.py frame) placed by the city composition on a slot of the city tile.

Layout of a city tile (authoring frame, front = -y):
- Center: its mass stays inside radius 0.20 (the inner slots sit at 0.34 and hold pieces of
  radius 0.13); only narrow parts reach out to 0.28 toward the front and the back, where no
  slot is.
- House pieces: a small block of one to three buildings on a yard of radius ~0.12. Later eras
  get taller: ancient huts ~0.1, information towers up to ~0.6.
- Landmarks: main mass inside radius ~0.17 so neighbouring slots stay readable; low parts
  (quays, runways, tracks) may reach further.
- Walls: a hex ring with corner radius 0.815, outside the outer slots (0.60 + 0.13).

Ownership split: the models only classic draws. The helpers shared with the platform's models
are imported from platform/tools/blender/world/city_pieces.py.
"""
import math

from geom import move, rot_z
import kit
import parts
from world_catalog import WorldModel

from city_pieces import _band, _columns, _figure, _flag, _floors, _front, _gable, _smoke, _tree


# Walls --------------------------------------------------------------------------------------------
WALL_R = 0.815
GATE_EDGE = 3  # edge from corner 3 (front) to corner 4: the front-right edge (faces the camera)


def walls(m):
    """Stone ring along the hex with round corner towers and a gatehouse on a front edge."""
    corners = kit.hex_corners(WALL_R)
    h, thick = m.pick(0.11, 0.11, 0.1), 0.04
    for k in range(6):
        a, b = corners[k], corners[(k + 1) % 6]
        if k == GATE_EDGE:
            mid = ((a[0] + b[0]) / 2, (a[1] + b[1]) / 2)
            ln = math.hypot(b[0] - a[0], b[1] - a[1])
            gap = 0.07 / ln
            _wall_run(m, a, (mid[0] + (a[0] - b[0]) * gap, mid[1] + (a[1] - b[1]) * gap), h, thick)
            _wall_run(m, (mid[0] + (b[0] - a[0]) * gap, mid[1] + (b[1] - a[1]) * gap), b, h, thick)
            _gatehouse(m, mid, math.degrees(math.atan2(b[1] - a[1], b[0] - a[0])), h)
        else:
            _wall_run(m, a, b, h, thick)
    for x, y in corners:
        if m.detail == 2:
            if y < 0:
                kit.block(m, (x, y, -0.02), (0.07, 0.07, 0.18), "stone")
            continue
        sides = m.pick(6, 5, 4)
        kit.cylinder(m, (x, y, -0.02), 0.036, 0.19, "stone", sides=sides)
        kit.cylinder(m, (x, y, 0.16), 0.04, 0.03, "stone", sides=sides)
        if m.detail == 0:
            kit.cone(m, (x, y, 0.19), 0.036, 0.06, "slate", sides=sides)


def _wall_run(m, a, b, h, thick):
    dx, dy = b[0] - a[0], b[1] - a[1]
    length = math.hypot(dx, dy)
    yaw = math.degrees(math.atan2(dy, dx))
    kit.block(m, ((a[0] + b[0]) / 2, (a[1] + b[1]) / 2, -0.02), (length, thick, h + 0.02), "stone", yaw)
    if m.detail != 0:
        return
    n = max(int(length / 0.13), 2)
    for i in range(n):
        t = (i + 0.5) / n
        kit.block(m, (a[0] + dx * t, a[1] + dy * t, h), (0.03, thick + 0.01, 0.022), "stone", yaw)


def _gatehouse(m, c, yaw, h):
    if m.detail == 2:
        return
    with m.at(move(c[0], c[1], 0.0), rot_z(yaw)):
        for s in (-1, 1):
            m.box((s * 0.06, 0.0, (h + 0.12) / 2 - 0.02), (0.03, 0.035, (h + 0.12) / 2), "stone")
            if m.detail == 0:
                m.taper((s * 0.06, 0.0, h + 0.1), (0.034, 0.039), (0.002, 0.002), 0.06, "slate")
        if m.detail < 2:
            m.box((0.0, 0.0, h + 0.02), (0.035, 0.03, 0.03), "stone")
            m.box((0.0, 0.0, h * 0.4), (0.032, 0.026, h * 0.4), "wood_dark")
            _flag(m, 0.06, 0.0, h + 0.1, 0.07, size=0.04)


def cathedral(m):
    """Gothic cathedral: nave toward the camera, twin spired towers, rose window, transept."""
    kit.block(m, (0.0, 0.07, -0.02), (0.1, 0.2, 0.14), "limestone")
    _gable(m, 0.0, 0.07, 0.12, 0.1, 0.2, 0.08, "slate", front=True)
    for s in (-1, 1):
        m.taper((s * 0.05, -0.065, 0.33), (0.024, 0.024), (0.001, 0.001), 0.22, "slate")
    if m.detail == 2:
        kit.block(m, (0.0, -0.065, -0.02), (0.15, 0.05, 0.35), "limestone")
        return
    for s in (-1, 1):
        kit.block(m, (s * 0.05, -0.065, -0.02), (0.05, 0.05, 0.35), "limestone")
    kit.block(m, (0.0, -0.065, -0.02), (0.06, 0.04, 0.22), "limestone")
    _gable(m, 0.0, -0.065, 0.2, 0.06, 0.04, 0.05, "slate", front=True, overhang=0.004)
    kit.block(m, (0.0, 0.09, -0.02), (0.22, 0.06, 0.13), "limestone")
    _gable(m, 0.0, 0.09, 0.11, 0.22, 0.06, 0.07, "slate")
    kit.cylinder(m, (0.0, 0.17, -0.02), 0.05, 0.15, "limestone", sides=m.pick(7, 5, 4))
    kit.cone(m, (0.0, 0.17, 0.13), 0.056, 0.07, "slate", sides=m.pick(7, 5, 4))
    m.taper((0.0, 0.09, 0.17), (0.014, 0.014), (0.001, 0.001), 0.12, "slate")
    for s in (-1, 1):
        _band(m, s * 0.05, -0.065, 0.32, 0.05, 0.05, 0.015, "stone", grow=0.01)
    if m.detail == 0:
        m.rod((0.0, -0.085, 0.15), (0.0, -0.091, 0.15), 0.022, 0.022, "glass", sides=8)
        _front(m, 0.0, -0.085, -0.01, 0.03, 0.08, "wood_dark")
        for s in (-1, 1):
            _front(m, s * 0.05, -0.09, 0.17, 0.014, 0.1, "glass_dark")
            for y in (0.03, 0.09, 0.15):
                kit.block(m, (s * 0.056, y, -0.02), (0.012, 0.016, 0.13), "stone")
            m.rod((s * 0.05, -0.065, 0.55), (s * 0.05, -0.065, 0.58), 0.003, 0.002, "gold", sides=3)


def mosque(m):
    """Prayer hall with a large dome, two small domes and a tall minaret."""
    kit.block(m, (-0.02, 0.03, -0.02), (0.18, 0.15, 0.1), "white")
    kit.cylinder(m, (-0.02, 0.03, 0.08), 0.06, 0.04, "white", sides=m.pick(10, 7, 5))
    kit.dome(m, (-0.02, 0.03, 0.12), 0.065, "turquoise", h=0.09, sides=m.pick(10, 7, 5), steps=m.pick(4, 3, 2))
    kit.cylinder(m, (0.12, -0.07, -0.02), 0.018, 0.44, "white", sides=m.pick(7, 5, 4))
    kit.cone(m, (0.12, -0.07, 0.42), 0.022, 0.07, "turquoise", sides=m.pick(7, 5, 4))
    if m.detail == 2:
        return
    kit.cylinder(m, (0.12, -0.07, 0.3), 0.026, 0.02, "sandstone", sides=m.pick(7, 5, 4))
    for s in (-1, 1):
        kit.dome(m, (-0.02 + s * 0.065, -0.02, 0.08), 0.025, "turquoise", h=0.03, sides=m.pick(7, 5, 4), steps=2)
    kit.block(m, (-0.02, -0.06, -0.02), (0.14, 0.04, 0.12), "sandstone")
    if m.detail == 0:
        m.rod((-0.02, 0.03, 0.21), (-0.02, 0.03, 0.25), 0.004, 0.002, "gold", sides=3)
        _front(m, -0.02, -0.08, -0.01, 0.04, 0.09, "roof_blue")
        _front(m, -0.02, -0.08, 0.02, 0.02, 0.055, "dark")
        kit.cylinder(m, (0.12, -0.07, 0.17), 0.023, 0.012, "sandstone", sides=7)
        m.rod((0.12, -0.07, 0.49), (0.12, -0.07, 0.52), 0.003, 0.002, "gold", sides=3)


def pagoda(m):
    """Five-storey pagoda: red storeys, wide dark roofs, gold spire."""
    tiers = 5
    if m.detail == 2:
        m.taper((0.0, 0.0, -0.02), (0.045, 0.045), (0.025, 0.025), 0.42, "lacquer")
    else:
        kit.block(m, (0.0, 0.0, -0.02), (0.16, 0.16, 0.04), "stone")
    z = 0.02
    for i in range(tiers):
        f = 1.0 - i * 0.13
        body = 0.085 * f
        h = 0.06 if i == 0 else 0.05
        if m.detail < 2:
            kit.block(m, (0.0, 0.0, z), (body, body, h), "lacquer")
        roof = body * 0.5 + 0.03
        if m.detail < 2 or i % 2 == 0:
            m.taper((0.0, 0.0, z + h), (roof, roof), (body * 0.3, body * 0.3), 0.025, "jade")
        z += h + 0.02
    m.rod((0.0, 0.0, z), (0.0, 0.0, z + 0.1), 0.008, 0.002, "gold", sides=m.pick(5, 4, 3))
    if m.detail == 2:
        return
    if m.detail == 0:
        for x in (-0.12, 0.12):
            kit.block(m, (x, -0.1, -0.02), (0.02, 0.02, 0.05), "stone")
            m.taper((x, -0.1, 0.03), (0.018, 0.018), (0.002, 0.002), 0.02, "stone")
        _front(m, 0.0, -0.04, 0.02, 0.025, 0.045, "gold")
        for x in (-0.05, 0.05):
            m.rod((x, -0.14, -0.02), (x, -0.14, 0.09), 0.006, 0.005, "lacquer", sides=4)
        kit.block(m, (0.0, -0.14, 0.085), (0.14, 0.012, 0.012), "lacquer")
        kit.block(m, (0.0, -0.14, 0.07), (0.1, 0.008, 0.008), "lacquer")


def temple(m):
    """Greek peripteral temple in marble, pediment toward the camera."""
    w, d, h = 0.14, 0.2, 0.12
    kit.block(m, (0.0, 0.0, -0.02), (w + 0.05, d + 0.05, 0.03), "limestone")
    kit.block(m, (0.0, 0.0, 0.01), (w + 0.025, d + 0.025, 0.02), "marble")
    kit.block(m, (0.0, 0.0, 0.03 + h), (w + 0.01, d + 0.01, 0.022), "marble")
    _gable(m, 0.0, 0.0, 0.03 + h + 0.022, w + 0.01, d + 0.01, 0.045, "marble", front=True)
    if m.detail == 2:
        kit.block(m, (0.0, 0.0, 0.03), (w - 0.02, d - 0.02, h), "marble")
        return
    sides = m.pick(5, 3, 3)
    xs = [-w / 2 + w * i / 3 for i in range(4)]
    _columns(m, xs, -d / 2, 0.03, h, 0.01, "marble", sides=sides)
    _columns(m, xs, d / 2, 0.03, h, 0.01, "marble", sides=sides)
    for y in (-d / 6, d / 6):
        _columns(m, (-w / 2, w / 2), y, 0.03, h, 0.01, "marble", sides=sides)
    kit.block(m, (0.0, 0.01, 0.03), (w - 0.05, d - 0.07, h), "plaster")
    if m.detail == 0:
        _front(m, 0.0, -d / 2 - 0.006, 0.152, 0.04, 0.02, "gold")
        m.octa((0.0, -d / 2 - 0.01, 0.222), 0.012, "gold")
        _band(m, 0.0, 0.0, 0.03 + h + 0.006, w + 0.01, d + 0.01, 0.008, "gold", grow=0.004)
        _front(m, 0.0, 0.01 - (d - 0.07) / 2, 0.03, 0.03, 0.07, "bronze")


def stadium(m):
    """Modern bowl of concrete stands round a green pitch, roof canopy, four light masts."""
    rx, ry = 0.18, 0.14
    n = m.pick(14, 10, 6)
    if m.detail == 2:
        m.rings([((0.0, 0.0, -0.02), rx, ry), ((0.0, 0.0, 0.1), rx, ry)], "concrete", sides=8)
        m.rings([((0.0, 0.0, 0.08), rx * 0.6, ry * 0.6), ((0.0, 0.0, 0.102), rx * 0.6, ry * 0.6)], "grass", sides=8)
        return
    m.rings([((0.0, 0.0, -0.03), rx * 0.62, ry * 0.6), ((0.0, 0.0, 0.012), rx * 0.62, ry * 0.6)], "grass",
            sides=m.pick(10, 7, 6))
    for k in range(n):
        a0, a1 = math.tau * k / n, math.tau * (k + 1) / n
        z_out = 0.08 + 0.05 * max(math.sin((a0 + a1) / 2), 0.0)
        sections = []
        for a in (a0, a1):
            c, s = math.cos(a) * rx, math.sin(a) * ry
            sections.append([(c * 0.66, s * 0.66, -0.02), (c, s, -0.02), (c, s, z_out), (c * 0.66, s * 0.66, 0.02)])
        m.loft(sections, "concrete" if k % 2 or m.detail else "white")
    if m.detail == 2:
        return
    for sx in (-1, 1):
        for sy in (-1, 1):
            x, y = sx * rx * 0.78, sy * ry * 0.82
            m.rod((x, y, 0.0), (x, y, 0.3), 0.005, 0.004, "steel", sides=3)
            if m.detail == 0:
                kit.block(m, (x, y - 0.004, 0.29), (0.04, 0.012, 0.025), "white")
    if m.detail == 0:
        kit.block(m, (0.0, 0.0, 0.0), (0.004, ry * 1.1, 0.0135), "white")
        kit.block(m, (0.0, ry * 0.95, 0.14), (rx * 1.2, 0.05, 0.01), "white")
        for x in (-0.14, 0.14):
            _flag(m, x, -0.12, 0.08, 0.06, "team", size=0.035)


def airport(m):
    """Runway piece, glass terminal, control tower and a parked airliner."""
    kit.patch(m, (0.0, -0.1), (0.2, 0.045), "asphalt", z0=-0.04, z1=0.008)
    kit.block(m, (-0.02, 0.07, -0.02), (0.2, 0.08, 0.07), "sky_glass")
    kit.cylinder(m, (0.13, 0.06, -0.02), 0.016, 0.3, "concrete", sides=m.pick(6, 5, 4))
    kit.cylinder(m, (0.13, 0.06, 0.28), 0.032, 0.04, "glass_dark", sides=m.pick(8, 6, 4))
    if m.detail == 2:
        return
    kit.block(m, (-0.02, 0.07, 0.05), (0.21, 0.09, 0.012), "white")
    kit.cylinder(m, (0.13, 0.06, 0.32), 0.036, 0.012, "white", sides=m.pick(8, 6, 4))
    _airliner(m, -0.02, -0.02, 0.18)
    if m.detail == 0:
        for i in range(5):
            kit.block(m, (-0.16 + 0.08 * i, -0.1, 0.0), (0.04, 0.006, 0.0095), "white")
        m.rod((0.13, 0.06, 0.33), (0.13, 0.06, 0.38), 0.003, 0.002, "red", sides=3)
        _flag(m, -0.13, 0.03, 0.0, 0.12)


def _airliner(m, x, y, length):
    L = length / 2
    m.rod((x - L, y, 0.035), (x + L * 0.8, y, 0.035), 0.012, 0.014, "white", sides=m.pick(6, 4, 3))
    m.rod((x + L * 0.8, y, 0.035), (x + L, y, 0.032), 0.014, 0.004, "white", sides=m.pick(6, 4, 3))
    m.loft([[(x - 0.005, y - L * 0.9, 0.03), (x + 0.03, y - 0.01, 0.03), (x + 0.03, y + 0.01, 0.03),
             (x - 0.005, y + L * 0.9, 0.03)],
            [(x - 0.005, y - L * 0.9, 0.034), (x + 0.03, y - 0.01, 0.034), (x + 0.03, y + 0.01, 0.034),
             (x - 0.005, y + L * 0.9, 0.034)]], "steel")
    m.prism([(x - L, 0.035), (x - L * 0.75, 0.04), (x - L * 0.9, 0.085), (x - L * 1.0, 0.085)], 0.004, "team")


def lighthouse(m):
    """Striped tower on rocks with a gallery, lantern and red cap, keeper's house."""
    kit.rock(m, (0.0, 0.0, -0.02), 0.11, "rock", seed=3, squash=0.5)
    kit.cylinder(m, (0.0, 0.0, 0.02), 0.04, 0.4, "white", sides=m.pick(8, 6, 5), r_top=0.028)
    kit.cone(m, (0.0, 0.0, 0.49), 0.034, 0.06, "red", sides=m.pick(8, 6, 5))
    if m.detail == 2:
        kit.cylinder(m, (0.0, 0.0, 0.42), 0.026, 0.07, "fire", sides=5)
        return
    for z, r in ((0.12, 0.037), (0.26, 0.033)):
        kit.cylinder(m, (0.0, 0.0, z), r + 0.002, 0.05, "red", sides=m.pick(8, 6, 5))
    kit.cylinder(m, (0.0, 0.0, 0.415), 0.04, 0.012, "dark", sides=m.pick(8, 6, 5))
    kit.cylinder(m, (0.0, 0.0, 0.427), 0.024, 0.065, "fire", sides=m.pick(8, 6, 5))
    if m.detail == 0:
        kit.block(m, (0.08, -0.06, -0.01), (0.07, 0.05, 0.06), "white")
        _gable(m, 0.08, -0.06, 0.05, 0.07, 0.05, 0.03, "red")
        m.ball((0.0, 0.0, 0.46), (0.05, 0.05, 0.02), "foam", u=6, v=3)


def monument(m):
    """Marble obelisk on a stepped plinth with two bronze braziers."""
    kit.steps(m, (0.0, 0.0, -0.02), 0.09, 0.025, 3, "marble")
    m.taper((0.0, 0.0, 0.055), (0.028, 0.028), (0.017, 0.017), 0.3, "white")
    m.taper((0.0, 0.0, 0.355), (0.017, 0.017), (0.001, 0.001), 0.04, "gold")
    if m.detail == 2:
        return
    for x in (-0.065, 0.065):
        kit.cylinder(m, (x, -0.065, 0.03), 0.008, 0.06, "bronze", sides=m.pick(5, 4, 3))
        kit.cone(m, (x, -0.065, 0.09), 0.016, 0.03, "fire", sides=m.pick(5, 4, 3))
    if m.detail == 0:
        _front(m, 0.0, -0.028, 0.09, 0.03, 0.05, "bronze")
        kit.block(m, (0.0, -0.13, -0.02), (0.16, 0.03, 0.03), "leaf_dark")


def granary(m):
    """Two round granaries with conical roofs, a storehouse on stilts and grain sacks."""
    for x, y, r, h in ((-0.07, 0.04, 0.045, 0.13), (0.03, 0.07, 0.04, 0.11)):
        kit.cylinder(m, (x, y, -0.02), r, h + 0.02, "mudbrick", sides=m.pick(8, 6, 5))
        kit.cone(m, (x, y, h - 0.005), r * 1.3, r * 1.6, "wheat", sides=m.pick(8, 6, 5))
    if m.detail == 2:
        return
    kit.block(m, (0.09, -0.05, 0.04), (0.08, 0.06, 0.05), "wood")
    _gable(m, 0.09, -0.05, 0.09, 0.08, 0.06, 0.035, "terracotta")
    for x in (0.06, 0.12):
        kit.block(m, (x, -0.05, -0.02), (0.01, 0.05, 0.06), "wood_dark")
    if m.detail == 0:
        for i, (x, y) in enumerate(((-0.06, -0.07), (-0.03, -0.08), (-0.045, -0.055), (0.0, -0.08))):
            m.ball((x, y, 0.014), (0.016, 0.013, 0.016), "linen" if i % 2 else "cloth", u=5, v=3)
        kit.cone(m, (-0.11, -0.05, -0.01), 0.018, 0.05, "wheat", sides=5)
        _front(m, -0.07, 0.04 - 0.044, -0.01, 0.02, 0.05, "wood_dark")


# National wonders ---------------------------------------------------------------------------------------
def national_heroic_epic(m):
    """Bronze hero statue raising a sword on a marble plinth, flanked by banners."""
    kit.steps(m, (0.0, 0.0, -0.02), 0.1, 0.03, 2, "marble")
    kit.block(m, (0.0, 0.0, 0.04), (0.07, 0.07, 0.09), "stone")
    if m.detail == 2:
        kit.cylinder(m, (0.0, 0.0, 0.13), 0.025, 0.2, "bronze", sides=4)
        return
    _figure(m, 0.0, 0.0, 0.13, 0.19, "bronze", weapon="gold")
    for x in (-0.09, 0.09):
        _flag(m, x, -0.08, 0.04, 0.17, "team", size=0.05)
    if m.detail == 0:
        _front(m, 0.0, -0.035, 0.07, 0.04, 0.03, "gold")
        for x in (-0.09, 0.09):
            kit.cylinder(m, (x, 0.08, 0.04), 0.012, 0.04, "bronze", sides=5)
            kit.cone(m, (x, 0.08, 0.08), 0.016, 0.035, "fire", sides=5)


def national_national_epic(m):
    """Poet's hall: a cream hall with a gilded half-dome exedra and a bronze poet."""
    kit.block(m, (0.0, 0.05, -0.02), (0.22, 0.12, 0.12), "cream")
    _gable(m, 0.0, 0.05, 0.1, 0.22, 0.12, 0.06, "roof_blue")
    kit.block(m, (0.0, -0.025, -0.02), (0.11, 0.04, 0.17), "marble")
    if m.detail == 2:
        return
    with m.at(move(0.0, -0.045, 0.06)):
        m.rings([((0.0, 0.0, 0.0), 0.04, 0.004), ((0.0, 0.0, 0.03), 0.038, 0.004), ((0.0, 0.0, 0.07), 0.0, 0.0)],
                "gold", sides=m.pick(8, 6, 4))
    _front(m, 0.0, -0.045, -0.01, 0.07, 0.07, "glass_dark")
    _gable(m, 0.0, -0.025, 0.15, 0.11, 0.04, 0.04, "marble", front=True, overhang=0.006)
    kit.block(m, (0.0, -0.12, -0.02), (0.05, 0.05, 0.05), "marble")
    _figure(m, 0.0, -0.12, 0.03, 0.1, "bronze", arm_up=m.detail == 0, weapon="paper")
    if m.detail == 0:
        for x in (-0.085, 0.085):
            _front(m, x, 0.05 - 0.06, 0.04, 0.03, 0.05, "glass_dark")
            _tree(m, x, -0.1, 0.1, "leaf_dark", kind="cypress")


def national_circus_maximus(m):
    """Chariot track: a long oval of sand, the spina with obelisks, stands along the back."""
    rx, ry = 0.22, 0.1
    m.rings([((0.0, -0.02, -0.03), rx, ry), ((0.0, -0.02, 0.01), rx, ry)], "sand", sides=m.pick(12, 8, 6))
    kit.block(m, (0.0, -0.02, -0.02), (0.24, 0.022, 0.05), "marble")
    m.taper((0.0, -0.02, 0.03), (0.012, 0.012), (0.001, 0.001), 0.2, "sandstone")
    if m.detail == 2:
        kit.block(m, (0.0, 0.11, -0.02), (0.4, 0.05, 0.08), "limestone")
        return
    for i in range(3):
        kit.block(m, (0.0, 0.1 + 0.016 * i, -0.02), (0.38 - 0.03 * i, 0.03, 0.04 + 0.025 * i), "limestone")
    kit.block(m, (0.0, 0.142, 0.08), (0.32, 0.01, 0.03), "red")
    for x in (-0.09, 0.09):
        m.taper((x, -0.02, 0.03), (0.008, 0.008), (0.001, 0.001), 0.1, "marble")
    kit.arch(m, (-0.215, -0.02, -0.02), 0.12, 0.09, 0.03, "limestone", yaw=90)
    if m.detail == 0:
        with m.at(move(0.1, -0.08, 0.0), rot_z(180)):
            m.box((0.0, 0.0, 0.02), (0.012, 0.012, 0.012), "wood")
            kit.quadruped(m, (0.04, 0.0, 0.0), 0.0, "horse", length=0.05, height=0.03)
        m.rod((0.0, -0.02, 0.23), (0.0, -0.02, 0.25), 0.004, 0.002, "gold", sides=3)
        _flag(m, 0.17, 0.12, 0.08, 0.06, "team", size=0.035)
        _flag(m, -0.17, 0.12, 0.08, 0.06, "team", size=0.035)


def national_east_india_company(m):
    """Big trading warehouse with loading doors, company flags and stacked goods."""
    kit.block(m, (0.0, 0.05, -0.02), (0.24, 0.12, 0.14), "white")
    kit.hip_roof(m, (0.0, 0.05, 0.12), 0.24, 0.12, 0.06, "slate")
    if m.detail == 2:
        kit.block(m, (0.0, -0.06, -0.02), (0.12, 0.05, 0.04), "wood")
        return
    for x in (-0.07, 0.0, 0.07):
        _front(m, x, 0.05 - 0.06, -0.01, 0.035, 0.06, "wood_dark")
        _front(m, x, 0.05 - 0.06, 0.08, 0.02, 0.03, "glass_dark")
    _band(m, 0.0, 0.05, 0.06, 0.24, 0.12, 0.008, "wood_dark")
    for i, (x, mat) in enumerate(((-0.08, "red"), (0.08, "team"))):
        _flag(m, x, 0.05, 0.15, 0.14, mat, size=0.06)
    _flag(m, 0.125, -0.06, 0.0, 0.2, "white", size=0.05)
    if m.detail == 0:
        for x, y, s, mat in ((-0.09, -0.07, 0.03, "wood"), (-0.06, -0.08, 0.025, "wood"), (-0.075, -0.075, 0.022, "wood")):
            kit.crate(m, (x, y, -0.01 if mat else 0), s, mat)
        kit.crate(m, (-0.075, -0.075, 0.02), 0.022, "spice")
        for x in (0.05, 0.075):
            kit.barrel(m, (x, -0.07, -0.01), 0.012, 0.03, "wood_dark")
        m.ball((0.1, -0.1, 0.012), (0.018, 0.014, 0.016), "linen", u=5, v=3)
        m.ball((0.07, -0.1, 0.012), (0.016, 0.013, 0.015), "spice", u=5, v=3)


def national_ironworks(m):
    """Blast furnace: a tapered brick stack with a glowing top, hot-blast stoves, a chimney."""
    m.rings([((0.0, 0.03, -0.02), 0.07, 0.07), ((0.0, 0.03, 0.12), 0.06, 0.06), ((0.0, 0.03, 0.34), 0.035, 0.035)],
            "brick", sides=m.pick(8, 6, 5))
    kit.cylinder(m, (0.0, 0.03, 0.34), 0.03, 0.03, "fire", sides=m.pick(8, 6, 5))
    kit.cylinder(m, (0.12, 0.08, -0.02), 0.018, 0.48, "brick", sides=m.pick(6, 5, 4), r_top=0.014)
    if m.detail == 2:
        return
    for x in (-0.11, -0.06):
        kit.cylinder(m, (x, 0.1, -0.02), 0.026, 0.2, "rust", sides=m.pick(7, 5, 4))
        kit.dome(m, (x, 0.1, 0.18), 0.026, "rust", h=0.025, sides=m.pick(7, 5, 4), steps=2)
    for z in (0.08, 0.2):
        kit.cylinder(m, (0.0, 0.03, z), 0.07 - z * 0.1, 0.012, "iron", sides=m.pick(8, 6, 5))
    m.rod((0.12, -0.12, 0.0), (0.02, 0.0, 0.33), 0.008, 0.008, "iron", sides=4)
    kit.block(m, (0.0, -0.06, -0.02), (0.1, 0.06, 0.05), "iron")
    if m.detail == 0:
        kit.block(m, (0.0, -0.09, 0.0), (0.06, 0.02, 0.012), "lava")
        _smoke(m, 0.0, 0.03, 0.37, 0.03)
        _smoke(m, 0.12, 0.08, 0.46, 0.022)
        kit.rock(m, (0.12, -0.08, -0.02), 0.035, "coal", seed=4, squash=0.5)


def national_oxford_university(m):
    """Gothic quad: four ranges round a lawn, a gate tower with an octagonal cupola, spired chapel."""
    half, thick, h = 0.13, 0.045, 0.1
    if m.detail == 2:
        kit.block(m, (0.0, 0.0, -0.02), (2 * half, 2 * half, h + 0.02), "sandstone")
        kit.block(m, (0.0, -half + thick / 2, -0.02), (0.07, 0.07, 0.24), "sandstone")
        m.taper((0.0, -half + thick / 2, 0.22), (0.04, 0.04), (0.001, 0.001), 0.07, "copper_green")
        m.taper((0.07, half - 0.03, 0.1), (0.03, 0.03), (0.001, 0.001), 0.24, "slate")
        return
    kit.block(m, (0.0, 0.0, -0.02), (2 * half, 2 * half, 0.025), "grass")
    for (x, y, w, d) in ((0.0, -half + thick / 2, 2 * half, thick), (0.0, half - thick / 2, 2 * half, thick),
                         (-half + thick / 2, 0.0, thick, 2 * half), (half - thick / 2, 0.0, thick, 2 * half)):
        kit.block(m, (x, y, -0.02), (w, d, h + 0.02), "sandstone")
        _gable(m, x, y, h, w, d, 0.04, "slate", front=w < d)
    kit.cylinder(m, (0.0, -half + thick / 2, -0.02), 0.04, 0.22, "sandstone", sides=m.pick(8, 6, 4))
    kit.dome(m, (0.0, -half + thick / 2, 0.2), 0.042, "copper_green", h=0.07, sides=m.pick(8, 6, 4),
             steps=m.pick(3, 2, 2))
    kit.block(m, (0.07, half - 0.03, -0.02), (0.05, 0.05, 0.2), "sandstone")
    m.taper((0.07, half - 0.03, 0.18), (0.03, 0.03), (0.001, 0.001), 0.16, "slate")
    if m.detail == 0:
        _front(m, 0.0, -half - 0.035, -0.01, 0.03, 0.07, "wood_dark")
        for x in (-0.09, -0.05, 0.05, 0.09):
            _front(m, x, -half, 0.03, 0.012, 0.05, "glass_dark")
        for x in (-half, half):
            m.taper((x, -half, 0.1), (0.008, 0.008), (0.001, 0.001), 0.07, "sandstone")
        m.rod((0.0, -half + thick / 2, 0.27), (0.0, -half + thick / 2, 0.3), 0.004, 0.001, "gold", sides=3)


def national_hermitage(m):
    """Green-and-white baroque palace: long facade, white pilasters, gold cupolas."""
    w, d, h = 0.3, 0.11, 0.15
    kit.block(m, (0.0, 0.03, -0.02), (w, d, h + 0.02), "jade")
    kit.hip_roof(m, (0.0, 0.03, h), w, d, 0.03, "steel")
    if m.detail == 2:
        kit.dome(m, (0.0, 0.03, h), 0.04, "gold", h=0.06, sides=4, steps=1)
        return
    _band(m, 0.0, 0.03, h - 0.015, w, d, 0.015, "white", grow=0.012)
    _band(m, 0.0, 0.03, 0.055, w, d, 0.008, "white")
    kit.block(m, (0.0, -0.035, -0.02), (0.08, 0.03, h + 0.04), "jade")
    for x in (-0.12, -0.08, -0.025, 0.025, 0.08, 0.12) if m.detail == 0 else (-0.1, 0.1):
        _front(m, x, 0.03 - d / 2, -0.01, 0.01, h, "white")
    for x in (-0.1, 0.0, 0.1):
        kit.dome(m, (x, 0.03, h + 0.025), 0.025, "gold", h=0.04, sides=m.pick(7, 5, 4), steps=2)
    if m.detail == 0:
        _front(m, 0.0, -0.05, h + 0.02, 0.06, 0.02, "white")
        for x in (-0.05, 0.05, -0.1, 0.1):
            _front(m, x, 0.03 - d / 2, 0.07, 0.02, 0.05, "glass_dark")
        _front(m, 0.0, -0.05, -0.01, 0.03, 0.07, "white")
        for x in (-0.1, 0.0, 0.1):
            m.rod((x, 0.03, h + 0.064), (x, 0.03, h + 0.09), 0.003, 0.001, "gold", sides=3)


def national_national_intelligence_agency(m):
    """Dark modern block with antenna masts, a radome and satellite dishes."""
    kit.block(m, (0.0, 0.03, -0.02), (0.2, 0.15, 0.2), "obsidian")
    kit.block(m, (0.04, 0.05, 0.18), (0.08, 0.07, 0.1), "glass_dark")
    if m.detail == 2:
        m.ball((-0.05, 0.05, 0.22), 0.04, "white", u=5, v=3)
        return
    _floors(m, 0.0, 0.03, 0.02, 0.2, 0.15, 0.16, m.pick(4, 2, 1), "glass_dark", frac=0.3)
    kit.dome(m, (-0.05, 0.06, 0.18), 0.04, "white", h=0.05, sides=m.pick(8, 6, 4), steps=m.pick(3, 2, 2))
    for x, h in ((0.02, 0.2), (0.07, 0.16)):
        m.rod((x, 0.06, 0.28), (x, 0.06, 0.28 + h), 0.004, 0.002, "steel", sides=3)
    for x, y in ((-0.12, -0.08), (0.12, -0.07)):
        m.rod((x, y, -0.02), (x, y, 0.05), 0.005, 0.005, "steel", sides=3)
        m.rings([((x, y - 0.004, 0.05), 0.004, 0.004), ((x, y - 0.018, 0.075), 0.03, 0.03)], "white",
                sides=m.pick(7, 5, 4))
    if m.detail == 0:
        kit.patch(m, (0.0, -0.08), (0.08, 0.03), "asphalt")
        _front(m, 0.0, 0.03 - 0.075, -0.01, 0.06, 0.04, "steel")
        for x in (0.02, 0.07):
            m.ball((x, 0.06, 0.28 + (0.2 if x < 0.05 else 0.16)), 0.005, "red", u=4, v=3)


def national_grand_temple(m):
    """Big stepped temple: tiered sandstone platform, tall central tower, four corner shrines."""
    kit.steps(m, (0.0, 0.02, -0.02), 0.16, 0.035, 3, "sandstone", shrink=0.03)
    m.rings([((0.0, 0.02, 0.085), 0.065, 0.065), ((0.0, 0.02, 0.25), 0.05, 0.05), ((0.0, 0.02, 0.4), 0.025, 0.025),
             ((0.0, 0.02, 0.46), 0.0, 0.0)], "sandstone", sides=m.pick(8, 6, 4), phase=0.0)
    if m.detail == 2:
        return
    for x in (-0.09, 0.09):
        for y in (-0.07, 0.11):
            m.rings([((x, y, 0.08), 0.026, 0.026), ((x, y, 0.17), 0.018, 0.018), ((x, y, 0.22), 0.0, 0.0)],
                    "sandstone", sides=m.pick(6, 4, 4), phase=0.0)
    kit.cylinder(m, (0.0, 0.02, 0.17), 0.058, 0.012, "gold", sides=m.pick(8, 6, 4))
    kit.cylinder(m, (0.0, 0.02, 0.32), 0.04, 0.012, "gold", sides=m.pick(8, 6, 4))
    m.rod((0.0, 0.02, 0.45), (0.0, 0.02, 0.53), 0.01, 0.0, "gold", sides=m.pick(5, 4, 3))
    if m.detail == 0:
        kit.block(m, (0.0, -0.15, -0.02), (0.06, 0.04, 0.05), "sandstone")
        _front(m, 0.0, -0.045, 0.085, 0.03, 0.06, "dark")
        for x in (-0.09, 0.09):
            m.rod((x, -0.07, 0.21), (x, -0.07, 0.25), 0.005, 0.0, "gold", sides=3)
            m.rod((x, 0.11, 0.21), (x, 0.11, 0.25), 0.005, 0.0, "gold", sides=3)


def national_national_visitor_center(m):
    """Modern pavilion: a white curved canopy over glass walls, flags and a lawn."""
    kit.patch(m, (0.0, 0.0), (0.17, 0.13), "concrete", z0=-0.04, z1=0.008)
    kit.block(m, (0.0, 0.04, -0.02), (0.2, 0.08, 0.09), "sky_glass")
    arc = [(-0.07 + 0.2 * i / 6, 0.07 + 0.1 * math.sin(math.pi * i / 6)) for i in range(7)]
    m.prism([(y, z) for y, z in arc] + [(0.13, 0.064), (-0.07, 0.064)], 0.28, "white", axis="x")
    if m.detail == 2:
        return
    for x in (-0.15, -0.075, 0.0, 0.075):
        m.rod((x, -0.11, -0.01), (x, -0.11, 0.17), 0.003, 0.003, "steel", sides=3)
        if m.detail == 0:
            kit.block(m, (x + 0.02, -0.11, 0.13), (0.04, 0.004, 0.03), ("team", "red", "roof_blue", "citrus")[
                int((x + 0.15) / 0.075 + 0.5)])
    _flag(m, 0.13, -0.1, 0.0, 0.17)
    if m.detail == 0:
        kit.patch(m, (-0.11, 0.1), (0.05, 0.03), "grass", z1=0.012)
        _tree(m, 0.13, 0.1, 0.08, "leaf_light")
        _tree(m, -0.13, 0.1, 0.08, "leaf_light")
        _front(m, 0.0, -0.02, -0.01, 0.04, 0.06, "glass_dark")
LANDMARK_BUILDERS = {
    "city/walls": walls, "city/cathedral": cathedral, "city/mosque": mosque, "city/pagoda": pagoda,
    "city/temple": temple, "city/stadium": stadium, "city/airport": airport, "city/lighthouse": lighthouse,
    "city/monument": monument, "city/granary": granary, "city/national-heroic-epic": national_heroic_epic,
    "city/national-national-epic": national_national_epic,
    "city/national-circus-maximus": national_circus_maximus,
    "city/national-east-india-company": national_east_india_company,
    "city/national-ironworks": national_ironworks,
    "city/national-oxford-university": national_oxford_university,
    "city/national-hermitage": national_hermitage,
    "city/national-national-intelligence-agency": national_national_intelligence_agency,
    "city/national-grand-temple": national_grand_temple,
    "city/national-national-visitor-center": national_national_visitor_center,
}


def models():
    """The classic landmarks; the pieces of the ring (walls) have their own budget."""
    landmarks = parts.load("classic", "world/city_kit").LANDMARKS
    ring = {e["model"] for e in landmarks.values() if e["slot"] == "ring"}
    out = []
    for model_id, build in LANDMARK_BUILDERS.items():
        out.append(WorldModel(model_id, build, "city-walls" if model_id in ring else "city-landmark",
                              ground="upright"))
    return out
