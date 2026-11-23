"""Reads the teachers' plays that `ai-exam suite --record FILE` writes (`crates/qa/src/army_net/record.rs`)
into flat arrays (no numpy: MLX takes the bytes).

A play: "ARMP" u32 | json_len u32 | json | n u32 | n decisions. A decision: "ARMD" u32 | T u16 | C u16 |
pointer i16 | action i16 | round u16 | 0 u16 | T*F_TOK f16 | T*T u8 | T u8 | T f16 | C*F_ACT f16 | C u16.

`Flat` holds every decision's rows back to back: tokens (sum T x F_TOK), distances (sum T*T), selectable
and danger (sum T), candidates (sum C x F_ACT), promotions (sum C), and per decision its offsets, sizes,
labels and the play it comes from. A batch is gathered on the GPU from these by offsets (`batch.py`).
"""
import json
import mmap
import struct
from array import array

F_TOK = 48
F_ACT = 40
PLAY = struct.unpack('<I', b'ARMP')[0]
DEC = struct.unpack('<I', b'ARMD')[0]


class Flat:
    def __init__(self):
        self.tok = bytearray()
        self.dist = bytearray()
        self.sel = bytearray()
        self.danger = bytearray()
        self.cand = bytearray()
        self.promo = bytearray()
        # Per decision.
        self.t = array('H')
        self.c = array('H')
        self.tok_off = array('I')
        self.dist_off = array('I')
        self.cand_off = array('I')
        self.ptr = array('h')
        self.act = array('h')
        self.round = array('H')
        self.play = array('I')
        # Per play.
        self.plays = []

    def __len__(self):
        return len(self.t)


def read_into(flat, path, keep=None):
    """Appends the plays of `path` to `flat`; `keep(meta)` filters plays."""
    fh = open(path, 'rb')
    buf = memoryview(mmap.mmap(fh.fileno(), 0, access=mmap.ACCESS_READ))
    at = 0
    n_buf = len(buf)
    while at < n_buf:
        magic, jl = struct.unpack_from('<II', buf, at)
        assert magic == PLAY, f'{path}: bad play magic at {at}'
        at += 8
        meta = json.loads(bytes(buf[at:at + jl]))
        at += jl
        (n,) = struct.unpack_from('<I', buf, at)
        at += 4
        take = keep is None or keep(meta)
        if take:
            meta['first'] = len(flat)
            meta['count'] = 0
            pid = len(flat.plays)
            flat.plays.append(meta)
        for _ in range(n):
            magic, t, c, ptr, act, rnd, _z = struct.unpack_from('<IHHhhHH', buf, at)
            assert magic == DEC, f'{path}: bad decision magic at {at}'
            at += 16
            sizes = (2 * t * F_TOK, t * t, t, 2 * t, 2 * c * F_ACT, 2 * c)
            if not take:
                at += sum(sizes)
                continue
            flat.t.append(t)
            flat.c.append(c)
            flat.tok_off.append(len(flat.tok) // (2 * F_TOK))
            flat.dist_off.append(len(flat.dist))
            flat.cand_off.append(len(flat.cand) // (2 * F_ACT))
            flat.ptr.append(ptr)
            flat.act.append(act)
            flat.round.append(rnd)
            flat.play.append(pid)
            for dst, size in zip((flat.tok, flat.dist, flat.sel, flat.danger, flat.cand, flat.promo), sizes):
                dst += buf[at:at + size]
                at += size
            meta['count'] += 1
    return flat


def read_plays(path):
    """The plays of a file, one Flat each (for inspection and the parity check)."""
    flat = read_into(Flat(), path)
    return flat
