"""Natural wonders of BNW (classic/docs/classic/natural_wonders.json): one recognisable model
each, covering its tile (radius up to 1.05, height up to 2.4).

Every wonder lays a terrain apron over most of the hex and builds its landform on it, so
the whole tile reads as the wonder. Mountains are stacks of jittered rings (`_massif`)
with one material per band: forest foot, rock, dark rock, snow cap.

Ownership split: the models only classic draws. The helpers shared with the platform's models
are imported from platform/tools/blender/world/natural_wonders.py.
"""
import math

from geom import move, rot_z
import kit

from natural_wonders import _apron, _ellipse, _glints, _massif, _model


# Shared shapes ----------------------------------------------------------------------------
def _k_at(layers, t):
    prof = [(0.0, 1.0)] + [(layer[0], layer[1]) for layer in layers]
    for (t0, k0), (t1, k1) in zip(prof, prof[1:]):
        if t <= t1:
            return k0 + (k1 - k0) * (t - t0) / max(t1 - t0, 1e-6)
    return prof[-1][1]


def _on_slope(c, r, h, layers, deg, t, out=0.96, elong=(1.0, 1.0)):
    """Point on the nominal surface of a `_massif` at height fraction t in direction deg."""
    k = _k_at(layers, t) * r * out
    a = math.radians(deg)
    return (c[0] + math.cos(a) * k * elong[0], c[1] + math.sin(a) * k * elong[1], c[2] + h * t)


def _path(m, pts, r, mat, sides=3):
    for a, b in zip(pts, pts[1:]):
        m.rod(a, b, r, r, mat, sides=sides)


def _acacia(m, c, h, leaf="olive"):
    """Savanna acacia: a leaning trunk under a flat umbrella crown."""
    x, y, z = c
    top = (x + h * 0.08, y, z + h * 0.78)
    m.rod((x, y, z - 0.01), top, h * 0.05, h * 0.035, "bark", sides=m.pick(4, 3, 3))
    m.rings([((top[0], y, top[2] - h * 0.04), h * 0.42, h * 0.36), ((top[0], y, top[2] + h * 0.08), h * 0.55, h * 0.47),
             ((top[0], y, top[2] + h * 0.2), h * 0.3, h * 0.26)], leaf, sides=m.pick(7, 5, 4))


def _adit(m, p, deg, w=0.07, h=0.08, frame="wood"):
    """Mine entrance at surface point p facing outward along deg: timber frame and a dark mouth."""
    with m.at(move(*p), rot_z(deg + 90.0)):
        m.box((0.0, 0.0, h * 0.45), (w * 0.42, 0.03, h * 0.45), "black")
        if m.detail < 2:
            for s in (-1, 1):
                m.box((s * w * 0.5, -0.012, h * 0.45), (0.011, 0.03, h * 0.5), frame)
            m.box((0.0, -0.012, h * 0.95), (w * 0.62, 0.03, 0.012), frame)


def _cart(m, c, yaw, load="gold"):
    with m.at(move(*c), rot_z(yaw)):
        for s in (-1, 1):
            m.box((0.0, s * 0.025, 0.004), (0.13, 0.004, 0.006), "iron")
        m.box((0.0, 0.0, 0.04), (0.038, 0.03, 0.024), "wood_dark")
        if m.detail == 0:
            for s in (-1, 1):
                m.rod((s * 0.024, -0.034, 0.016), (s * 0.024, 0.034, 0.016), 0.014, 0.014, "iron", sides=5)
        m.ball((0.0, 0.0, 0.066), (0.034, 0.026, 0.016), load, u=m.pick(5, 4, 4), v=3)


# The Grand Mesa ---------------------------------------------------------------------------
def _mesa(m, c, rx, ry, bands, top, seed, yaw=0.0, talus=1.25):
    """Flat-topped mesa: talus skirt, stacked cliff bands with ledges, flat cap."""
    n = m.pick(11, 8, 6)
    base = _ellipse(c, rx, ry, n, yaw, seed, 0.18)

    def ring(s, z):
        return [(c[0] + (x - c[0]) * s, c[1] + (y - c[1]) * s, z) for x, y in base]

    z_cliff = bands[0][0]
    m.loft([ring(talus, -0.04), ring(1.0, z_cliff + 0.02)], "sandstone")
    if m.detail == 2:
        bands = [(z_cliff, bands[-1][1], 0.96, "rock_red")]
    for z0, z1, s, mat in bands:
        m.loft([ring(s, z0), ring(s * 0.985, z1)], mat)
    z_top, s_top = bands[-1][1], bands[-1][2] * 0.985
    m.loft([ring(s_top * 1.005, z_top - 0.01), ring(s_top * 0.98, z_top + 0.014)], top)
    return z_top + 0.014


def the_grand_mesa(m):
    _apron(m, "khaki")
    bands = [(0.16, 0.30, 0.98, "rock_red"), (0.28, 0.38, 0.965, "sandstone"), (0.36, 0.50, 0.95, "rock_red"),
             (0.48, 0.56, 0.94, "ochre"), (0.54, 0.66, 0.93, "rock_red")]
    top = _mesa(m, (0.02, 0.14, 0.0), 0.66, 0.4, bands, "grass_dry", seed=8, yaw=8.0)
    butte = [(0.1, 0.24, 0.97, "rock_red"), (0.22, 0.3, 0.95, "sandstone"), (0.28, 0.42, 0.93, "rock_red")]
    _mesa(m, (0.52, -0.42, 0.0), 0.15, 0.12, butte, "grass_dry", seed=13, talus=1.35)
    if m.detail == 2:
        return
    # A lone spire at the front left and junipers on the flat top and the plain.
    _massif(m, (-0.55, -0.42, 0.0), 0.09, 0.36, [(0.3, 0.8, "sandstone"), (0.6, 0.65, "rock_red"),
                                                  (0.95, 0.4, "rock_red")], seed=4, sides=m.pick(6, 5), jit=0.2)
    pts = [(-0.3, 0.2), (0.2, 0.32), (0.38, 0.08), (-0.1, 0.05), (-0.42, 0.32)]
    for i, (x, y) in enumerate(pts[:m.pick(5, 2)]):
        kit.tree(m, (x, y, top - 0.01), 0.1, "conifer", leaf="leaf_dark", seed=i)
    for i, (x, y) in enumerate([(-0.2, -0.6), (0.1, -0.68), (-0.75, 0.05), (0.8, 0.1)][:m.pick(4, 2)]):
        kit.tree(m, (x, y, 0.0), 0.07, "bush", leaf="olive", seed=i)


# Great Barrier Reef -----------------------------------------------------------------------
REEF_FLATS = [((-0.42, 0.4), 0.32, 0.17, 30.0), ((0.4, 0.42), 0.3, 0.19, -25.0), ((0.0, -0.02), 0.42, 0.22, 8.0),
              ((-0.46, -0.4), 0.26, 0.15, -30.0), ((0.46, -0.4), 0.27, 0.16, 25.0)]
CORAL_MATS = ("coral", "coral_purple", "citrus", "pink", "jade", "ochre")


def _coral(m, c, kind, mat, s, seed):
    x, y, z = c
    if kind == 0:  # branching staghorn
        for i in range(m.pick(5, 3)):
            a = math.tau * (i + kit.rand(seed, i)) / 5
            tip = (x + math.cos(a) * s * 0.5, y + math.sin(a) * s * 0.5, z + s * (0.8 + 0.4 * kit.rand(seed, i + 9)))
            m.rod((x, y, z - 0.02), tip, s * 0.13, s * 0.07, mat, sides=m.pick(4, 3))
    elif kind == 1:  # brain coral
        kit.dome(m, (x, y, z - 0.01), s * 0.55, mat, h=s * 0.5, sides=m.pick(7, 5), steps=m.pick(3, 2))
    elif kind == 2:  # table coral
        m.rod((x, y, z - 0.02), (x, y, z + s * 0.45), s * 0.12, s * 0.1, mat, sides=4)
        m.rings([((x, y, z + s * 0.42), s * 0.45, s * 0.4), ((x, y, z + s * 0.56), s * 0.7, s * 0.62)], mat,
                sides=m.pick(7, 5))
    else:  # sea fan
        with m.at(move(x, y, z), rot_z(360 * kit.rand(seed, 3))):
            m.prism([(-s * 0.1, -0.02), (s * 0.1, -0.02), (s * 0.55, s * 0.7), (0.0, s * 1.05), (-s * 0.55, s * 0.7)],
                    0.012, mat)


def the_great_barrier_reef(m):
    """Turquoise reef flats ringed with surf, crowded with colourful corals that break the
    surface, and a sandy cay with palms in the middle."""
    z_flat = 0.012
    for i, (c, rx, ry, yaw) in enumerate(REEF_FLATS):
        n = m.pick(10, 7, 6)
        foam = _ellipse(c, rx * 1.14, ry * 1.22, n, yaw, 20 + i, 0.15)
        kit.slab(m, foam, -0.12, 0.004, "foam")
        flat = _ellipse(c, rx, ry, n, yaw, 20 + i, 0.15)
        kit.slab(m, flat, -0.1, z_flat, "turquoise")
        if m.detail == 2:
            kit.dome(m, (c[0], c[1], 0.0), min(rx, ry) * 0.6, CORAL_MATS[i % 3], h=0.07, sides=5, steps=1)
            continue
        count = m.pick(7, 4)
        pts = kit.scatter(50 + i, count, 0.0, 0.85, 0.07)
        ca, sa = math.cos(math.radians(yaw)), math.sin(math.radians(yaw))
        for k, (u, v) in enumerate(pts):
            x, y = u * rx * 0.85, v * ry * 0.85
            p = (c[0] + x * ca - y * sa, c[1] + x * sa + y * ca, z_flat)
            if i == 2 and math.hypot(p[0] - 0.05, p[1] + 0.02) < 0.2:
                continue
            kind = int(kit.rand(70 + i, k) * 4)
            mat = CORAL_MATS[int(kit.rand(90 + i, k) * len(CORAL_MATS))]
            _coral(m, p, kind, mat, 0.13 + 0.06 * kit.rand(80 + i, k), 100 * i + k)
    # Sandy cay with palms.
    sand = _ellipse((0.05, -0.02), 0.17, 0.11, m.pick(9, 7, 5), 10.0)
    m.loft([[(x, y, -0.04) for x, y in sand], [(0.05 + (x - 0.05) * 0.6, -0.02 + (y + 0.02) * 0.6, 0.05)
                                                 for x, y in sand]], "sand")
    if m.detail < 2:
        kit.tree(m, (0.0, -0.01, 0.04), 0.2, "palm", leaf="leaf", seed=3)
        kit.tree(m, (0.1, 0.0, 0.04), 0.16, "palm", leaf="leaf_light", seed=8)


# Krakatoa ---------------------------------------------------------------------------------
KRAKATOA = [(0.14, 0.76, "rock_dark"), (0.19, 0.66, "sand"), (0.3, 0.52, "ash"), (0.5, 0.36, "ash"),
            (0.72, 0.24, "obsidian", 0.07), (0.9, 0.17, "obsidian", 0.05)]


def krakatoa(m):
    """Black volcanic cone rising from the sea: surf ring, ash slopes, glowing crater, lava
    runs and a drifting smoke plume."""
    c, r, h = (0.0, 0.05, -0.16), 0.88, 1.15
    kit.cylinder(m, (c[0], c[1], -0.03), r * 0.8, 0.036, "foam", sides=m.pick(12, 9, 7))
    _massif(m, c, r, h, KRAKATOA, seed=21, sides=m.pick(12, 9, 7), jit=0.12)
    z_top = c[2] + h * 0.9
    kit.cylinder(m, (c[0], c[1], z_top - 0.04), r * 0.12, 0.055, "lava", sides=m.pick(8, 6, 5))
    if m.detail < 2:
        # Lava runs down the front slopes.
        for i, deg in enumerate((-70.0, -112.0, -150.0)[:m.pick(3, 2)]):
            pts = [_on_slope(c, r, h, KRAKATOA, deg + 6 * math.sin(t * 9), t, out=1.0)
                   for t in (0.88, 0.76, 0.64, 0.52, 0.42 - 0.06 * i)]
            _path(m, pts, 0.022 - 0.004 * i, "lava", sides=m.pick(4, 3))
        kit.cylinder(m, (c[0], c[1], z_top), r * 0.07, 0.04, "fire", sides=5)
        # Green scrub on the leeward foot.
        for i, deg in enumerate((120, 150, 30, 60)[:m.pick(4, 2)]):
            p = _on_slope(c, r, h, KRAKATOA, deg, 0.23, out=0.98)
            kit.tree(m, p, 0.12, "palm" if i % 2 else "bush", leaf="leaf", seed=i)
    # Smoke plume drifting right, darkest at the vent.
    puffs = [((0.0, 0.06, 1.1), 0.13, "rock_dark"), ((0.06, 0.08, 1.3), 0.18, "ash"),
             ((0.17, 0.1, 1.52), 0.23, "rock"), ((0.32, 0.12, 1.74), 0.26, "stone"), ((0.5, 0.14, 1.94), 0.24, "steam")]
    if m.detail == 2:
        puffs = [((0.04, 0.07, 1.22), 0.2, "ash"), ((0.26, 0.11, 1.62), 0.27, "stone")]
    for i, (p, s, mat) in enumerate(puffs):
        m.ball(p, (s, s * 0.9, s * 0.8), mat, u=m.pick(8, 6, 6), v=m.pick(5, 4, 4))
        if m.detail == 0 and i in (2, 3):
            m.ball((p[0] - s * 0.7, p[1] - 0.04, p[2] - s * 0.3), s * 0.55, mat, u=6, v=4)


# Cerro de Potosi --------------------------------------------------------------------------
POTOSI = [(0.08, 0.78, "khaki"), (0.26, 0.56, "rust"), (0.42, 0.42, "rock_red"), (0.58, 0.3, "terracotta"),
          (0.74, 0.2, "rock_red"), (0.9, 0.09, "rust"), (1.0, 0.0, "rust")]


def cerro_de_potosi(m):
    """Rounded red silver mountain pocked with mine mouths and pale tailings, silver glints
    and a colonial church at the foot."""
    _apron(m, "grass_dry")
    c, r, h = (0.0, 0.12, 0.0), 0.86, 1.4
    _massif(m, c, r, h, POTOSI, seed=33, sides=m.pick(12, 9, 6), jit=0.07)
    mines = [(-90.0, 0.16), (-122.0, 0.3), (-58.0, 0.36), (-96.0, 0.5), (-140.0, 0.12), (-36.0, 0.18),
             (-75.0, 0.64)]
    if m.detail == 2:
        return
    glints = []
    for i, (deg, t) in enumerate(mines[:m.pick(7, 4)]):
        p = _on_slope(c, r, h, POTOSI, deg, t, out=0.98)
        below = _on_slope(c, r, h, POTOSI, deg, max(t - 0.07, 0.02), out=1.02)
        kit.rock(m, (below[0], below[1], below[2] - 0.03), 0.07, "sand", seed=i, squash=0.45)
        _adit(m, p, deg, w=0.08, h=0.085)
        glints.append(_on_slope(c, r, h, POTOSI, deg + 9, t + 0.05, out=1.0))
    _glints(m, glints, "silver", 0.024)
    # Colonial church and houses at the front-left foot.
    kit.house(m, (-0.6, -0.56, -0.01), 0.14, 0.09, 0.08, "plaster", "terracotta", yaw=-20)
    kit.tower(m, (-0.48, -0.66, -0.01), 0.035, 0.17, "plaster", roof="dome", roof_mat="terracotta",
              sides=m.pick(6, 5))
    for i, (x, y) in enumerate([(-0.76, -0.36), (-0.32, -0.76)][:m.pick(2, 1)]):
        kit.house(m, (x, y, -0.01), 0.08, 0.07, 0.06, "mudbrick", "terracotta", yaw=15 * i)


# El Dorado --------------------------------------------------------------------------------
def el_dorado(m):
    """Round lake ringed by golden shores under a city of gold spires; a gilded raft with the
    golden man floats on the water among gold glints."""
    import people
    _apron(m, "leaf")
    lc = (0.0, -0.24)
    kit.water_pool(m, lc, 0.42, "water", z=0.03, sides=m.pick(14, 10, 7), rim="gold")
    # Jungle hills framing the lake.
    for i, (x, y, r) in enumerate([(-0.66, -0.22, 0.22), (0.66, -0.26, 0.2), (-0.62, 0.3, 0.2), (0.64, 0.28, 0.2)]):
        kit.dome(m, (x, y, -0.02), r, "leaf_dark" if i % 2 == 0 else "leaf", h=r * 0.8, sides=m.pick(8, 6, 5),
                 steps=m.pick(3, 2, 1))
    # The golden city on a stepped platform at the back.
    kit.steps(m, (0.0, 0.45, -0.03), 0.3, 0.06, 3, "sandstone", shrink=0.07)
    spires = [(0.0, 0.45, 0.17, 0.06, 0.62), (-0.32, 0.38, 0.02, 0.045, 0.46), (0.32, 0.36, 0.02, 0.045, 0.42),
              (-0.17, 0.66, 0.08, 0.04, 0.5), (0.18, 0.66, 0.08, 0.04, 0.54), (-0.12, 0.3, 0.1, 0.035, 0.3),
              (0.13, 0.28, 0.1, 0.035, 0.28)]
    for x, y, z, r, h in spires[:m.pick(7, 5, 3)]:
        kit.tower(m, (x, y, z), r, h, "bronze", roof="cone", roof_mat="gold", sides=m.pick(6, 5, 4),
                  roof_h=r * 5.0)
    if m.detail == 2:
        return
    kit.dome(m, (0.0, 0.45, 0.17), 0.11, "gold", h=0.1, sides=m.pick(8, 6))
    # Gilded raft with the golden man.
    with m.at(move(lc[0] + 0.02, lc[1] + 0.02, 0.03), rot_z(15)):
        for i in range(m.pick(5, 3)):
            m.rod((-0.1, -0.06 + 0.03 * i, 0.0), (0.1, -0.06 + 0.03 * i, 0.0), 0.014, 0.014, "wood", sides=4)
        m.box((0.0, 0.0, 0.018), (0.075, 0.05, 0.006), "gold")
        gold = people.Look(skin="gold", body="gold", legs="gold", boots="gold", belt="gold", head="bare")
        kit.person(m, (0.0, 0.0, 0.02), 0.0, gold, height=0.13)
        for s in (-1, 1):
            m.octa((s * 0.05, 0.02, 0.04), 0.014, "gold")
    _glints(m, [(-0.24, -0.28, 0.04), (0.22, -0.4, 0.04), (-0.08, -0.5, 0.04), (0.26, -0.12, 0.04)], "gold", 0.02)
    for i, (x, y) in enumerate([(-0.78, 0.0), (0.8, 0.0), (-0.42, -0.66), (0.42, -0.68)]):
        kit.tree(m, (x, y, 0.0), 0.2, "palm", leaf="leaf_light", seed=i)


# Sri Pada ---------------------------------------------------------------------------------
SRI_PADA = [(0.1, 0.7, "leaf_dark"), (0.3, 0.45, "leaf"), (0.48, 0.3, "leaf_dark"), (0.62, 0.21, "rock"),
            (0.86, 0.1, "rock_dark"), (0.93, 0.06, "rock_dark")]


def sri_pada(m):
    """Tall pointed jungle peak with the white summit shrine and the lit pilgrim stair
    zig-zagging up its face."""
    _apron(m, "leaf")
    c, r, h = (0.0, 0.12, 0.0), 0.84, 1.72
    _massif(m, c, r, h, SRI_PADA, seed=44, sides=m.pick(12, 9, 6), jit=0.08)
    z_top = h * 0.93
    kit.cylinder(m, (c[0], c[1], z_top - 0.03), 0.09, 0.05, "stone", sides=m.pick(7, 5, 4))
    kit.house(m, (c[0], c[1], z_top + 0.02), 0.11, 0.09, 0.06, "white", "red", kind="hip", roof_h=0.05,
              door=None)
    if m.detail == 2:
        return
    m.rod((c[0], c[1], z_top + 0.12), (c[0], c[1], z_top + 0.18), 0.012, 0.0, "gold", sides=4)
    # Pilgrim stair: a pale line zig-zagging up the front face, lanterns along it.
    steps = 16 if m.detail == 0 else 8
    pts = []
    for i in range(steps + 1):
        t = 0.02 + 0.89 * i / steps
        deg = -90.0 + 24.0 * math.sin(i * 1.35) * (0.6 + t)
        pts.append(_on_slope(c, r, h, SRI_PADA, deg, t, out=1.0))
    _path(m, pts, 0.014, "cream", sides=m.pick(4, 3))
    _glints(m, [(p[0], p[1] - 0.012, p[2] + 0.02) for p in pts[1::2]], "fire", 0.014)
    for i, (x, y) in enumerate([(-0.66, -0.48), (0.62, -0.5), (-0.8, 0.06), (0.8, 0.1), (-0.32, -0.74)][:m.pick(5, 2)]):
        kit.tree(m, (x, y, 0.0), 0.2, "palm" if i % 2 else "broadleaf", leaf="leaf_light", seed=i)


# Mt. Sinai --------------------------------------------------------------------------------
SINAI = [(0.22, 0.72, "sandstone"), (0.52, 0.44, "rust"), (0.8, 0.22, "rock_red", 0.1), (1.0, 0.06, "rock_red")]
SINAI_SPURS = [(-0.5, 0.32, 0.4, 0.86), (0.52, 0.3, 0.42, 0.95), (-0.32, -0.26, 0.3, 0.52), (0.36, -0.28, 0.3, 0.48),
               (0.02, 0.64, 0.32, 0.72)]


def mt_sinai(m):
    """Bare rugged red granite massif of jagged summits, the white chapel on the top and the
    walled monastery with cypresses at its foot."""
    _apron(m, "sand")
    c, r, h = (0.04, 0.16, 0.0), 0.52, 1.28
    spurs = SINAI_SPURS if m.detail < 2 else SINAI_SPURS[:2]
    for i, (x, y, sr, sh) in enumerate(spurs):
        _massif(m, (x, y, 0.0), sr, sh, [(0.22, 0.72, "sandstone"), (0.55, 0.42, "rust"), (0.85, 0.16, "rock_red", 0.1),
                                         (1.0, 0.0, "rock_red")], seed=50 + i, sides=m.pick(7, 6, 5), jit=0.4)
    _massif(m, c, r, h, SINAI, seed=57, sides=m.pick(9, 7, 6), jit=0.35)
    kit.house(m, (c[0], c[1], h - 0.01), 0.08, 0.06, 0.05, "white", "white", kind="flat", door=None)
    if m.detail == 2:
        return
    kit.cylinder(m, (c[0] + 0.05, c[1], h + 0.02), 0.016, 0.07, "white", sides=4)
    # St Catherine's: a walled square with a basilica and cypresses.
    with m.at(move(-0.08, -0.62, 0.0), rot_z(8)):
        for s in (-1, 1):
            m.box((0.0, s * 0.09, 0.04), (0.16, 0.012, 0.05), "sandstone")
            m.box((s * 0.15, 0.0, 0.04), (0.012, 0.09, 0.05), "sandstone")
        kit.house(m, (0.02, 0.0, -0.01), 0.12, 0.07, 0.07, "limestone", "slate", door=None)
        if m.detail == 0:
            kit.tower(m, (-0.08, 0.03, -0.01), 0.022, 0.13, "limestone", roof="cone", roof_mat="slate", sides=4)
    for i, (x, y) in enumerate([(0.22, -0.66), (0.3, -0.58), (-0.38, -0.66)][:m.pick(3, 1)]):
        kit.tree(m, (x, y, 0.0), 0.16, "cypress", leaf="leaf_dark", seed=i)


# Mt. Kailash ------------------------------------------------------------------------------
KAILASH = [(0.16, 0.82, "rock"), (0.33, 0.66, "rock_dark"), (0.39, 0.61, "snow"), (0.47, 0.54, "rock_dark"),
           (0.56, 0.46, "snow"), (0.61, 0.42, "rock_dark"), (0.75, 0.31, "snow"), (0.79, 0.27, "rock_dark"),
           (0.92, 0.15, "snow"), (1.0, 0.0, "snow")]


def mt_kailash(m):
    """Four-faced diamond peak: a square pyramid with dark rock strata between snow ledges,
    the vertical gully on its face, brown foothills and the sacred lake."""
    _apron(m, "khaki")
    half, h = 0.56, 1.62
    hills = [(-0.56, 0.36, 0.34, 0.5), (0.56, 0.32, 0.36, 0.55), (-0.62, -0.18, 0.28, 0.36), (0.64, -0.14, 0.26, 0.32),
             (0.0, 0.66, 0.3, 0.62)]
    for i, (x, y, r, hh) in enumerate(hills if m.detail < 2 else hills[:2]):
        kit.peak(m, (x, y, 0.0), r, hh, "rock_red" if i % 2 else "rock", snow="snow", snow_frac=0.25, seed=70 + i)
    layers = KAILASH if m.detail < 2 else [(0.3, 0.68, "rock_dark"), (0.6, 0.42, "snow"), (0.78, 0.28, "rock_dark"),
                                           (1.0, 0.0, "snow")]
    with m.at(move(0.0, 0.14, 0.0), rot_z(22.0)):
        prev = [(sx * half, sy * half, -0.04) for sx, sy in ((-1, -1), (1, -1), (1, 1), (-1, 1))]
        for t, k, mat in layers:
            if k <= 0.0:
                m.loft([prev, [(0.0, 0.0, h * t)]], mat)
                break
            ring = [(sx * half * k, sy * half * k, h * t) for sx, sy in ((-1, -1), (1, -1), (1, 1), (-1, 1))]
            m.loft([prev, ring], mat)
            prev = ring
        if m.detail < 2:
            # The vertical gully down the middle of the front face.
            gully = [(0.0, -half * _k_at(KAILASH, t) - 0.006, h * t) for t in (0.3, 0.39, 0.47, 0.56, 0.61, 0.72)]
            _path(m, gully, 0.026, "rock_dark", sides=4)
    if m.detail < 2:
        kit.water_pool(m, (-0.42, -0.58), 0.17, "turquoise", z=0.022, sides=m.pick(9, 6))


# Uluru ------------------------------------------------------------------------------------
ULURU_U = [-1.0, -0.96, -0.88, -0.74, -0.55, -0.32, -0.08, 0.18, 0.42, 0.62, 0.78, 0.89, 0.96, 1.0]
ULURU_STREAKS = [-0.62, 0.3, -0.05, 0.55, -0.36, 0.78]


def _uluru_jit(u):
    return kit.rand(15, int(round((u + 1.0) * 100))) - 0.5


def _uluru_section(u, half_len, n_arc, jit):
    """Rounded cross-section at u in [-1, 1]: steep flanks, domed flat top; the west end is
    higher and blunter than the long eastern tail."""
    x = u * half_len
    a = abs(u)
    w = 0.34 * max(1.0 - a ** 4, 0.0) ** 0.33 * (1.0 + 0.08 * jit) + 0.03
    h = 0.52 * max(1.0 - a ** 3, 0.0) ** 0.4 * (1.0 - 0.13 * u + 0.06 * jit) + 0.04
    pts = [(x, w, -0.04)]
    for i in range(n_arc):
        th = math.pi * (i + 0.5) / n_arc
        cs, sn = math.cos(th), math.sin(th)
        pts.append((x, w * math.copysign(abs(cs) ** 0.55, cs), -0.04 + (h + 0.04) * sn ** 0.45))
    pts.append((x, -w, -0.04))
    return pts


def uluru(m):
    """Huge red sandstone monolith: long, steep-sided, rounded flat top, with dark water
    streaks, on red sand with spinifex and desert oaks."""
    _apron(m, "ochre")
    n_arc = m.pick(7, 5, 4)
    us = ULURU_U if m.detail < 2 else [-1.0, -0.88, -0.5, 0.0, 0.5, 0.88, 1.0]
    with m.at(move(0.0, 0.1, 0.0), rot_z(16.0)):
        sections = [_uluru_section(u, 0.86, n_arc, _uluru_jit(u)) for u in us]
        for a, b in zip(sections, sections[1:]):
            m.loft([a, b], "rock_red")
        if m.detail == 2:
            return
        # Dark water streaks down the front flank and cave hollows at its foot.
        first = round(0.62 * n_arc - 0.5) + 1
        for u in ULURU_STREAKS[:m.pick(6, 3)]:
            arc = _uluru_section(u, 0.86, n_arc, 0.5)[first:]
            _path(m, [(x, y - 0.012, z) for x, y, z in arc], 0.02, "rust", sides=3)
        for u in (-0.5, 0.12, 0.62):
            x, y, _ = _uluru_section(u, 0.86, n_arc, _uluru_jit(u))[-1]
            m.ball((x, y + 0.02, 0.03), (0.05, 0.04, 0.035), "rock_dark", u=5, v=3)
    for i, (x, y) in enumerate([(-0.6, -0.42), (-0.2, -0.56), (0.25, -0.5), (0.62, -0.32), (-0.75, 0.2), (0.7, 0.45),
                                (0.0, -0.74)][:m.pick(7, 3)]):
        kit.tree(m, (x, y, 0.0), 0.06, "bush", leaf="grass_dry" if i % 2 else "olive", seed=i)
    for i, (x, y) in enumerate([(-0.45, -0.66), (0.48, -0.62), (-0.35, 0.62)][:m.pick(3, 1)]):
        kit.tree(m, (x, y, 0.0), 0.16, "broadleaf", leaf="olive", seed=i)


# Mt. Kilimanjaro --------------------------------------------------------------------------
KIBO = [(0.07, 0.74, "leaf_dark"), (0.2, 0.56, "leaf"), (0.36, 0.43, "grass_dry"), (0.58, 0.31, "rock"),
        (0.76, 0.24, "rock_dark", 0.08), (0.88, 0.21, "snow"), (0.92, 0.19, "snow")]


def mt_kilimanjaro(m):
    """Broad volcano with a flat glacier-capped summit, the jagged Mawenzi peak beside it and
    flat-topped acacias on the savanna at its foot."""
    _apron(m, "grass_dry")
    _massif(m, (0.1, 0.14, 0.0), 0.8, 1.2, KIBO, seed=61, sides=m.pick(13, 9, 7), jit=0.12, elong=(1.0, 0.9))
    _massif(m, (-0.5, 0.3, 0.0), 0.36, 0.82, [(0.2, 0.72, "leaf"), (0.5, 0.45, "rock_dark"),
                                               (0.8, 0.2, "rock_dark", 0.12), (1.0, 0.0, "snow")],
            seed=62, sides=m.pick(7, 6, 5), jit=0.35)
    if m.detail == 2:
        return
    acacias = [(-0.56, -0.56), (-0.24, -0.72), (0.12, -0.74), (0.46, -0.64), (0.74, -0.32), (-0.78, -0.12)]
    for i, (x, y) in enumerate(acacias[:m.pick(6, 3)]):
        _acacia(m, (x, y, 0.0), 0.15 + 0.03 * kit.rand(4, i), "olive" if i % 2 else "leaf")
    if m.detail == 0:
        kit.quadruped(m, (-0.02, -0.6, 0.0), 20.0, "ochre", length=0.09, height=0.1, neck_up=1.3, legs="ochre",
                      head="ochre")
        kit.quadruped(m, (0.3, -0.5, 0.0), 200.0, "elephant", length=0.12, height=0.07, horns="ivory")


# King Solomon's Mines ---------------------------------------------------------------------
SOLOMON = [(0.25, 0.75, "ochre"), (0.52, 0.52, "rock"), (0.8, 0.32, "rock_dark", 0.1), (1.0, 0.14, "rock")]


def _portal(m, c, w, h, depth=0.08):
    """Ancient carved gate in the hillside: columns, lintel, pediment and a dark doorway."""
    with m.at(move(*c)):
        m.box((0.0, 0.0, h * 0.42), (w * 0.34, depth * 0.5, h * 0.42), "black")
        for s in (-1, 1):
            kit.column(m, (s * w * 0.42, -depth * 0.3, -0.02), h * 0.82, w * 0.07, "limestone",
                       sides=m.pick(6, 5, 4))
        m.box((0.0, -depth * 0.2, h * 0.88), (w * 0.55, depth * 0.55, h * 0.07), "sandstone")
        kit.wedge_roof(m, (0.0, -depth * 0.2, h * 0.95), w * 1.05, depth * 0.9, h * 0.22, "limestone", yaw=0)
        if m.detail < 2:
            kit.steps(m, (0.0, -depth - 0.04, -0.03), 0.1, 0.02, 2, "sandstone", shrink=0.025)


def king_solomon_s_mines(m):
    """Rugged rocky hill with a carved ancient gate, a timbered adit, carts heaped with gold
    on rails, gold piles and glints in the rock."""
    _apron(m, "grass_dry")
    c, r, h, el = (0.0, 0.32, 0.0), 0.68, 0.86, (1.2, 0.78)
    _massif(m, c, r, h, SOLOMON, seed=81, sides=m.pick(10, 8, 6), jit=0.3, elong=el)
    crags = [(-0.3, 0.38, 0.2, 0.42), (0.28, 0.42, 0.18, 0.36), (0.02, 0.52, 0.16, 0.3)]
    for i, (x, y, cr, ch) in enumerate(crags if m.detail < 2 else crags[:1]):
        _massif(m, (x, y, h * 0.62), cr, ch, [(0.5, 0.6, "rock"), (1.0, 0.0, "rock_dark")], seed=90 + i,
                sides=m.pick(6, 5, 4), jit=0.4)
    kit.rock(m, (-0.66, 0.02, -0.02), 0.22, "rock_dark", seed=3, squash=0.9)
    kit.rock(m, (0.66, 0.06, -0.02), 0.24, "rock", seed=5, squash=0.8)
    _portal(m, (0.02, -0.18, 0.0), 0.26, 0.34)
    if m.detail == 2:
        return
    _adit(m, (-0.44, -0.04, 0.0), -110.0, w=0.09, h=0.1)
    with m.at(move(0.02, -0.5, 0.0)):
        for s in (-1, 1):
            m.box((s * 0.025, 0.08, 0.004), (0.004, 0.26, 0.006), "iron")
    _cart(m, (0.02, -0.46, 0.0), 90.0)
    if m.detail == 0:
        _cart(m, (0.4, -0.58, 0.0), 30.0)
    for i, (x, y, s) in enumerate([(-0.22, -0.4, 0.06), (0.24, -0.34, 0.05), (-0.36, -0.58, 0.04)][:m.pick(3, 1)]):
        kit.cone(m, (x, y, -0.01), s, s * 0.9, "gold", sides=m.pick(6, 5))
    glints = [_on_slope(c, r, h, SOLOMON, deg, t, out=1.0, elong=el)
              for deg, t in ((-60.0, 0.3), (-120.0, 0.42), (-100.0, 0.62), (-40.0, 0.5), (-150.0, 0.2), (-75.0, 0.75))]
    _glints(m, glints, "gold", 0.028)
    kit.rock(m, (0.56, -0.28, -0.03), 0.13, "rock_dark", seed=21, squash=0.5)


def models():
    return [
        _model("the-grand-mesa", the_grand_mesa, "upright"),
        _model("the-great-barrier-reef", the_great_barrier_reef, "water", "reef"),
        _model("krakatoa", krakatoa, "water"),
        _model("cerro-de-potosi", cerro_de_potosi, "upright"),
        _model("el-dorado", el_dorado, "upright"),
        _model("sri-pada", sri_pada, "upright"),
        _model("mt-sinai", mt_sinai, "upright"),
        _model("mt-kailash", mt_kailash, "upright"),
        _model("uluru", uluru, "upright"),
        _model("mt-kilimanjaro", mt_kilimanjaro, "upright"),
        _model("king-solomon-s-mines", king_solomon_s_mines, "upright"),
    ]
