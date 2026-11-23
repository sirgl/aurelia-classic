"""Reads the teachers' plays that `ai-exam suite --record FILE` writes (`crates/qa/src/army_net/record.rs`)
into flat arrays (no numpy: MLX takes the bytes).

A play: "ARMP" (v2) or "ARM3" (v3) u32 | json_len u32 | json | W*W*S_CH f16 | n u32 | n decisions. A decision:
"ARMD" u32 | T u16 | C u16 | pointer i16 (-1 end, -2 none: a memory state of the net's own play) | action i16 |
round u16 | flags u16 (v3; bit 0: our turn ends here, the memory updates) | T*F_TOK f16 | T*T u8 | T u8 | T f16 |
C*F_ACT f16 | C u16 | T i16 | C i16 | v3: T i16 slot (index of the token's unit in the army, -1 none) |
TRACK i16 (the teacher's own state at a memory state: engaged, waited, stage, resting mask over the slots; -1 unknown).

`Flat` holds every decision's rows back to back: tokens (sum T x F_TOK), distances (sum T*T), selectable, danger and
slots (sum T), candidates (sum C x F_ACT), promotions (sum C), and per decision its offsets, sizes, labels, flags,
track and the play it comes from. A batch is gathered on the GPU from these by offsets.
"""
import json
import mmap
import struct
from array import array

F_TOK = 48
F_ACT = 40
W = 24
S_CH = 16
TRACK = 4
PLAY = struct.unpack('<I', b'ARMP')[0]
PLAY3 = struct.unpack('<I', b'ARM3')[0]
PLAY4 = struct.unpack('<I', b'ARM4')[0]
F_HIST = 20
DEC = struct.unpack('<I', b'ARMD')[0]
# Flags of a decision.
MEMORY_STATE = 1


class Flat:
    def __init__(self):
        self.tok = bytearray()
        self.dist = bytearray()
        self.sel = bytearray()
        self.danger = bytearray()
        self.cand = bytearray()
        self.promo = bytearray()
        self.pos = bytearray()
        self.cpos = bytearray()
        self.slot = bytearray()
        self.hist = bytearray()
        self.hist_slot = bytearray()
        self.hist_age = bytearray()
        self.statics = bytearray()
        # Per decision.
        self.t = array('H')
        self.c = array('H')
        self.tok_off = array('I')
        self.dist_off = array('I')
        self.cand_off = array('I')
        self.ptr = array('h')
        self.act = array('h')
        self.round = array('H')
        self.flags = array('H')
        self.track = array('h')
        self.h = array('H')
        self.hist_off = array('I')
        self.play = array('I')
        # Per play.
        self.plays = []

    def __len__(self):
        return len(self.t)


def _play_header(buf, at):
    """The header of the play at `at`: (meta, v3, v4, its statics, decisions, the first decision's offset)."""
    magic, jl = struct.unpack_from('<II', buf, at)
    assert magic in (PLAY, PLAY3, PLAY4), f'bad play magic at {at}'
    v4 = magic == PLAY4
    v3 = magic == PLAY3 or v4
    at += 8
    meta = json.loads(bytes(buf[at:at + jl]))
    at += jl
    statics = buf[at:at + 2 * W * W * S_CH]
    at += 2 * W * W * S_CH
    (n,) = struct.unpack_from('<I', buf, at)
    return meta, v3, v4, statics, n, at + 4


def _decision_sizes(t, c):
    return (2 * t * F_TOK, t * t, t, 2 * t, 2 * c * F_ACT, 2 * c, 2 * t, 2 * c)


def _skip_decision(buf, at, v3, v4):
    """The offset after the decision at `at`, and its (T, pointer)."""
    magic, t, c, ptr = struct.unpack_from('<IHHh', buf, at)
    assert magic == DEC, f'bad decision magic at {at}'
    at += 16 + sum(_decision_sizes(t, c)) + (2 * t + 2 * TRACK if v3 else 0)
    if v4:
        (h,) = struct.unpack_from('<H', buf, at)
        at += 2 + h * (2 * F_HIST + 3)
    return at, t, ptr


def _read_decision(flat, buf, at, pid, v3, v4):
    """Appends the decision at `at` to `flat`; returns the offset after it."""
    magic, t, c, ptr, act, rnd, flags = struct.unpack_from('<IHHhhHH', buf, at)
    assert magic == DEC, f'bad decision magic at {at}'
    at += 16
    flat.t.append(t)
    flat.c.append(c)
    flat.tok_off.append(len(flat.tok) // (2 * F_TOK))
    flat.dist_off.append(len(flat.dist))
    flat.cand_off.append(len(flat.cand) // (2 * F_ACT))
    flat.ptr.append(ptr)
    flat.act.append(act)
    flat.round.append(rnd)
    flat.flags.append(flags if v3 else 0)
    flat.play.append(pid)
    for dst, size in zip((flat.tok, flat.dist, flat.sel, flat.danger, flat.cand, flat.promo, flat.pos, flat.cpos), _decision_sizes(t, c)):
        dst += buf[at:at + size]
        at += size
    if v3:
        flat.slot += buf[at:at + 2 * t]
        at += 2 * t
        flat.track.extend(struct.unpack_from(f'<{TRACK}h', buf, at))
        at += 2 * TRACK
    else:
        flat.slot += b'\xff\xff' * t
        flat.track.extend([-1] * TRACK)
    flat.hist_off.append(len(flat.hist_age))
    h = 0
    if v4:
        (h,) = struct.unpack_from('<H', buf, at)
        at += 2
        flat.hist += buf[at:at + 2 * h * F_HIST]
        at += 2 * h * F_HIST
        flat.hist_slot += buf[at:at + 2 * h]
        at += 2 * h
        flat.hist_age += buf[at:at + h]
        at += h
    flat.h.append(h)
    return at


def read_play_at(flat, buf, at):
    """Appends the play at offset `at` of a mapped file to `flat`; returns the offset after it."""
    meta, v3, v4, statics, n, at = _play_header(buf, at)
    meta['first'] = len(flat)
    meta['count'] = n
    meta['v3'] = v3
    pid = len(flat.plays)
    flat.plays.append(meta)
    flat.statics += statics
    for _ in range(n):
        at = _read_decision(flat, buf, at, pid, v3, v4)
    return at


def skip_play_at(buf, at):
    """The play at `at` without copying its rows: (meta, the offset after it, its largest T, labelled decisions,
    decisions)."""
    meta, v3, v4, _, n, at = _play_header(buf, at)
    t_max = 0
    labelled = 0
    for _ in range(n):
        at, t, ptr = _skip_decision(buf, at, v3, v4)
        t_max = max(t_max, t)
        labelled += ptr >= -1
    return meta, at, t_max, labelled, n


def map_file(path):
    fh = open(path, 'rb')
    return memoryview(mmap.mmap(fh.fileno(), 0, access=mmap.ACCESS_READ))


def read_into(flat, path, keep=None):
    """Appends the plays of `path` to `flat`; `keep(meta)` filters plays."""
    buf = map_file(path)
    at = 0
    while at < len(buf):
        if keep is None or keep(_play_header(buf, at)[0]):
            at = read_play_at(flat, buf, at)
        else:
            at = skip_play_at(buf, at)[1]
    return flat


def read_plays(path):
    """The plays of a file, one Flat each (for inspection and the parity check)."""
    flat = read_into(Flat(), path)
    return flat
