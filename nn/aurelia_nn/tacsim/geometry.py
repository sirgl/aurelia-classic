"""Hex geometry of the window, shared by every scene of a batch (axial coordinates, as the kernel's
`math.rs`: tile index = r * W + q inside the window, the six directions of `HEX_DIRS`).

Tables (MLX int32 arrays):
- `nbr[T, 6]`: the neighbour in each direction (-1 off the window);
- `dist[T, T]`: hex distance;
- `disc`: the offsets of the radius-`R` disc in the kernel's `hex_disc` order, their distance, the
  tiles strictly between the centre and each offset on the hex line (the kernel's `hex_line_all`:
  cube lerp, nudge (1, 1, -2), `round_cube`; translation invariant, so one table of offsets),
  `disc_tile[T, O]` and `disc_mid[T, O, R-1]` (window tiles or -1);
- `rel[2R+1, 2R+1]`: the disc offset index of a (dq, dr) pair (-1 outside the disc); `words`: uint64
  words of a tile's line-of-sight bits over the disc;
- the attack disc (radius `RA` <= R, the reach of ranged units and cities): `att_tile[T, OA]`,
  `att_dist[OA]` (in disc order);
- `local`: the disc of radius `RM` around a unit for its movement wavefront: `ltile[T, L]` and the
  local neighbour table `lnbr[L, 6]`.
"""

import mlx.core as mx

DIRS = [(1, 0), (1, -1), (0, -1), (-1, 0), (-1, 1), (0, 1)]
LINE_SCALE = 1024
LINE_NUDGE = (1, 1, -2)
# Moves stay this far inside the window (the dump's rule), so paths stay in it.
INNER = 2


def hex_distance(dq, dr):
    return (abs(dq) + abs(dr) + abs(dq + dr)) // 2


def disc_offsets(radius):
    out = []
    for dq in range(-radius, radius + 1):
        for dr in range(max(-radius, -dq - radius), min(radius, radius - dq) + 1):
            out.append((dq, dr))
    return out


def round_cube(num, den):
    rounded = [(2 * n + den) // (2 * den) for n in num]  # Python // is div_euclid for den > 0
    err = [abs(rounded[i] * den - num[i]) for i in range(3)]
    if err[0] > err[1] and err[0] > err[2]:
        rounded[0] = -rounded[1] - rounded[2]
    elif err[1] > err[2]:
        rounded[1] = -rounded[0] - rounded[2]
    else:
        rounded[2] = -rounded[0] - rounded[1]
    return rounded


def line_between(dq, dr):
    """Offsets strictly between (0, 0) and (dq, dr) on the kernel's hex line."""
    n = hex_distance(dq, dr)
    if n < 2:
        return []
    b = (dq, dr, -dq - dr)
    out = []
    for i in range(1, n):
        num = [b[a] * i * LINE_SCALE + LINE_NUDGE[a] for a in range(3)]
        c = round_cube(num, n * LINE_SCALE)
        out.append((c[0], c[1]))
    return out


class Geometry:
    def __init__(self, w, radius=4, move_radius=5, att_radius=4):
        self.w = w
        self.t = w * w
        self.radius = radius
        self.move_radius = move_radius
        T = self.t
        qr = [(i % w, i // w) for i in range(T)]
        self.qr = qr

        def idx(q, r):
            return r * w + q if 0 <= q < w and 0 <= r < w else -1

        self.nbr_list = [[idx(q + dq, r + dr) for dq, dr in DIRS] for q, r in qr]
        self.nbr = mx.array(self.nbr_list, dtype=mx.int32)
        self.dist_list = [[hex_distance(q2 - q1, r2 - r1) for (q2, r2) in qr] for (q1, r1) in qr]
        self.dist = mx.array(self.dist_list, dtype=mx.int32)
        offs = disc_offsets(radius)
        self.offs = offs
        self.o = len(offs)
        self.off_dist = mx.array([hex_distance(*o) for o in offs], dtype=mx.int32)
        self.words = (self.o + 63) // 64
        ra = min(att_radius, radius)
        att = [i for i, o in enumerate(offs) if hex_distance(*o) <= ra]
        mids = [line_between(*o) for o in offs]
        km = max(1, radius - 1)
        self.km = km
        rel = [[-1] * (2 * radius + 1) for _ in range(2 * radius + 1)]
        for i, (dq, dr) in enumerate(offs):
            rel[dq + radius][dr + radius] = i
        self.rel_list = rel
        self.rel = mx.array(rel, dtype=mx.int32)
        dt, dm = [], []
        for q, r in qr:
            dt.append([idx(q + dq, r + dr) for dq, dr in offs])
            row = []
            for m in mids:
                cells = [idx(q + a, r + b) for a, b in m] + [-2] * (km - len(m))
                row.append(cells)
            dm.append(row)
        self.disc_tile = mx.array(dt, dtype=mx.int32)
        self.att_tile = mx.array([[row[i] for i in att] for row in dt], dtype=mx.int32)
        self.att_dist = mx.array([hex_distance(*offs[i]) for i in att], dtype=mx.int32)
        self.att_offs = [offs[i] for i in att]
        # -2: no mid in this slot; -1: a mid off the window (open land: never blocks)
        self.disc_mid = mx.array(dm, dtype=mx.int32)
        # The movement disc.
        loffs = disc_offsets(move_radius)
        loffs.sort(key=lambda o: (hex_distance(*o), o))  # the centre first
        self.loffs = loffs
        self.l = len(loffs)
        lpos = {o: i for i, o in enumerate(loffs)}
        self.lnbr = mx.array([[lpos.get((dq + a, dr + b), -1) for a, b in DIRS] for dq, dr in loffs], dtype=mx.int32)
        # The attack disc against the movement disc (the policies read attack masks per local tile).
        self.disc_to_local_list = [lpos.get(o, -1) for o in self.att_offs]
        dpos = {o: i for i, o in enumerate(self.att_offs)}
        self.local_to_disc = mx.array([dpos.get(o, -1) for o in loffs], dtype=mx.int32)
        self.ltile = mx.array([[idx(q + dq, r + dr) for dq, dr in loffs] for q, r in qr], dtype=mx.int32)
        self.inner = mx.array([INNER <= q < w - INNER and INNER <= r < w - INNER for q, r in qr])
        self.q = mx.array([q for q, _ in qr], dtype=mx.int32)
        self.r = mx.array([r for _, r in qr], dtype=mx.int32)

    def rel_index(self, src, dst):
        """Disc offset index of dst seen from src (window tiles, any shape; -1 outside the disc)."""
        R = self.radius
        dq = mx.take(self.q, mx.maximum(dst, 0)) - mx.take(self.q, mx.maximum(src, 0))
        dr = mx.take(self.r, mx.maximum(dst, 0)) - mx.take(self.r, mx.maximum(src, 0))
        inside = (mx.abs(dq) <= R) & (mx.abs(dr) <= R) & (src >= 0) & (dst >= 0)
        flat = mx.clip(dq + R, 0, 2 * R) * (2 * R + 1) + mx.clip(dr + R, 0, 2 * R)
        o = mx.take(self.rel.reshape(-1), flat)
        return mx.where(inside, o, -1)

    def distance(self, a, b):
        dq = mx.take(self.q, mx.maximum(a, 0)) - mx.take(self.q, mx.maximum(b, 0))
        dr = mx.take(self.r, mx.maximum(a, 0)) - mx.take(self.r, mx.maximum(b, 0))
        d = (mx.abs(dq) + mx.abs(dr) + mx.abs(dq + dr)) // 2
        return mx.where((a >= 0) & (b >= 0), d, 999)
