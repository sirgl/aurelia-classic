"""The Rust parity fixtures: weights files and windows of consecutive synthetic
turns with the integer reference's outputs, memory carried from turn to turn
(crates/classic-rules/tests/net_e2e_model.rs).

    python -m aurelia_nn.fixture ../crates/classic-rules/tests/fixtures/net_e2e

`<name>.bin` is a weights file; `<name>_w<window>_t<turn>.txt` a turn, one
line per array: `name count v1 v2 …`. Inputs: tiles, neighbors, region,
global, entities, entity_meta, entity_ids (local entity indices), decisions
(per decision: head, actor, first candidate, candidate count), choices (per
decision the chosen candidate, which joins the turn trace after it), candidates,
candidate_meta (target entity local), and for the `events` net events and
event_meta (actor local); expected: tile_h, tokens, value,
logits (all decisions' candidates in order), the memory after the turn
(mem_player, mem_nation_ids, mem_nations, mem_unit_ids, mem_units).
"""

from __future__ import annotations

import sys
import tempfile
from pathlib import Path

import numpy as np
import torch

from . import export, quantize, synthetic
from .data import Shard
from .model import Net

# Windows: (width, height, seed, turns); both run through one TurnCache.
WINDOWS = ((18, 17, 101, 3), (9, 7, 202, 3))
AIM = (256, 0, 128)  # science 1, score 0, conquest 0.5 (Q8), for the flags fixture
CONFIGS = {
    # The small preset's shape.
    "small": {},
    # Wider than the hot paths assume: conv fan-in 560 and ff fan-in 600
    # (chunked i32 sums), one 64-wide attention head, odd output widths.
    "wide": dict(d_tile=80, n_conv=1, d_model=64, n_attn=1, n_attn_heads=1, d_ff=600, d_head=24, mem_player=40, param_buckets=32, tag_buckets=16),
    # The flags of the integer forward: engram (a small table), aim, coarse
    # regions (the first window's map has 2 × 2 blocks).
    "flags": dict(engram=1, engram_buckets=1 << 10, engram_dim=8, aim=1, coarse=1),
    # The turn trace (weights version 3): each decision's context adds the
    # trace of the turn's choices before it (`choices`: the label of each
    # decision, as the trainer forces them).
    "trace": dict(trace=1),
    # The events (weights version 4, format 7): every token attends over the
    # turn's event tokens; on shards with events (EVENT_WINDOWS).
    "events": dict(events=1, trace=1),
    # The defer choice (weights version 5, format 9): every unit decision
    # ends with the defer candidate, every third one takes it (out of the
    # turn trace), and the bias moves its logit (DEFER_WINDOWS).
    "defer": dict(trace=1, defer=1),
}
# The nets of shards with events: their own windows (the others keep the
# shards without events, so their files stay those before format 7).
EVENT_CONFIGS = ("events",)
# The defer net's bias (logits) and its windows: the plain ones with the
# defer candidate (`with_defer_candidates`).
DEFER_BIAS = 1.5
DEFER_CONFIGS = ("defer",)


def with_defer_candidates(window, every: int = 3):
    """The turns of `window` with the defer candidate (tag DEFER_TAG, a zero
    row, no target, no parameter) last in every unit decision; every
    `every`-th unit decision of a turn chooses it (0: none)."""
    import copy

    from .common import DEFER_TAG, HEAD_UNIT
    from .data import Decision

    out = []
    for turn in window:
        turn = copy.copy(turn)
        decisions, n = [], 0
        for d in turn.decisions:
            if d.head == HEAD_UNIT:
                rows = np.concatenate([d.rows, np.zeros((1, d.rows.shape[1]), d.rows.dtype)])
                meta = np.concatenate([d.meta, np.asarray([[DEFER_TAG, -1, -1, -1]], d.meta.dtype)])
                choice = len(d.rows) if every and n % every == every - 1 else d.choice
                d = Decision(d.head, d.actor, rows, meta, choice, d.is_wait and choice == d.choice)
                n += 1
            decisions.append(d)
        turn.decisions = decisions
        turn.dec_arrays = None
        out.append(turn)
    return out


def build_model(layout, overrides: dict) -> Net:
    torch.manual_seed(1234)
    net = Net(layout.config(**overrides), layout)
    with torch.no_grad():
        # Sharper attention and larger activations than at init, so the
        # fixture reaches the exp table's end and the saturation; larger GRU
        # weights so the gates leave the middle of their tables.
        for block in net.blocks:
            block.qkv.weight.mul_(10.0)
        net.edges.normal_(0, 1.0)
        net.tile_embed.weight.mul_(3.0)
        net.convs[-1].weight.mul_(40.0)
        for gru in (net.gru_player, net.gru_nation, net.gru_unit):
            gru.weight_ih.mul_(8.0)
            gru.bias_hh.normal_(0, 1.0)
        if net.cfg.trace:
            # W_tr starts at zero; here large enough to move the queries.
            net.trace.weight.normal_(0, 0.5)
        if net.cfg.events:
            # xattn.out starts at zero; here large enough to move the tokens,
            # and sharp event attention.
            net.xattn.out.weight.normal_(0, 0.5)
            net.xattn.q.weight.mul_(10.0)
            net.event.weight.mul_(4.0)
    return net


def turn_arrays(turn, st: quantize.TurnState, logits, mem: quantize.Memory, aim) -> dict[str, np.ndarray]:
    dec, rows, metas, choices = [], [], [], []
    first = 0
    for d in turn.decisions:
        dec += [d.head, d.actor, first, len(d.rows)]
        choices.append(d.choice)
        rows.append(d.rows)
        metas.append(d.meta)
        first += len(d.rows)
    nid = sorted(mem.nations)
    uid = sorted(mem.units)
    ev = {} if turn.events is None else {"events": turn.events, "event_meta": turn.event_meta}
    return ev | {
        "tiles": turn.tiles,
        "neighbors": turn.neighbors,
        "region": turn.region,
        "global": turn.global_row,
        "entities": turn.entities,
        "entity_meta": turn.entity_meta,
        "entity_ids": turn.entity_ids,
        "coarse": turn.coarse,
        "aim": np.asarray(aim if aim is not None else [], np.int64),
        "decisions": np.asarray(dec),
        "choices": np.asarray(choices),
        "candidates": np.concatenate(rows),
        "candidate_meta": np.concatenate(metas),
        "expect_tile_h": st.tile_h,
        "expect_tokens": st.tokens,
        "expect_value": np.asarray([st.value]),
        "expect_logits": np.concatenate(logits),
        "expect_mem_player": mem.player,
        "expect_mem_nation_ids": np.asarray(nid),
        "expect_mem_nations": np.asarray([mem.nations[i] for i in nid]).ravel(),
        "expect_mem_unit_ids": np.asarray(uid),
        "expect_mem_units": np.asarray([mem.units[i] for i in uid]).ravel(),
    }


def write_turn_file(path: Path, arrays: dict[str, np.ndarray]):
    with open(path, "w") as f:
        f.write("# aurelia net_e2e parity fixture: name count values...\n")
        for name, a in arrays.items():
            a = np.asarray(a, np.int64).ravel()
            f.write(f"{name} {a.size} {' '.join(str(int(v)) for v in a)}\n")


def read_turn_file(path: Path) -> dict[str, np.ndarray]:
    out = {}
    for line in Path(path).read_text().splitlines():
        if not line or line.startswith("#"):
            continue
        parts = line.split()
        n = int(parts[1])
        vals = np.asarray([int(v) for v in parts[2:]], np.int64)
        assert len(vals) == n, parts[0]
        out[parts[0]] = vals
    return out


def make(out_dir: str | Path):
    out_dir = Path(out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    for old in list(out_dir.glob("*.txt")) + list(out_dir.glob("*.bin")):
        old.unlink()
    with tempfile.TemporaryDirectory() as tmp:
        plain, with_events = [], []
        for events, windows in ((False, plain), (True, with_events)):
            for i, (w, h, seed, n) in enumerate(WINDOWS):
                shard = Shard(synthetic.write_shard(Path(tmp) / f"{i}-{int(events)}.shard", seed, w, h, rounds=8, players=2, religion=True, events=events))
                # A window from its second turn on (units already born and dying),
                # every head present somewhere in it.
                rows = shard.windows[0][1 : 1 + n]
                windows.append([shard.turn(int(t)) for t in rows])
        layouts = {False: Shard(Path(tmp) / "0-0.shard").layout, True: Shard(Path(tmp) / "0-1.shard").layout}
        layout = layouts[False]
        for name, overrides in CONFIGS.items():
            windows = with_events if name in EVENT_CONFIGS else plain
            model = build_model(layouts[name in EVENT_CONFIGS], overrides)
            if name in DEFER_CONFIGS:
                windows = [with_defer_candidates(w) for w in windows]
                with torch.no_grad():
                    model.defer_bias.fill_(DEFER_BIAS)
            qm = quantize.quantize(model, layouts[name in EVENT_CONFIGS])
            size = export.write(qm, out_dir / f"{name}.bin")
            for wi, window in enumerate(windows):
                mem = quantize.Memory.empty(qm.cfg)
                aim = AIM if qm.cfg.aim else None
                for ti, turn in enumerate(window):
                    st = quantize.encode_turn(qm, turn.tiles, turn.neighbors, turn.region, turn.global_row, turn.entities, turn.entity_meta,
                                              turn.entity_ids, mem, coarse=turn.coarse, aim=aim, events=turn.events, event_meta=turn.event_meta)
                    logits = []
                    for d in turn.decisions:
                        logits.append(quantize.score(qm, st, d.head, d.actor, d.rows, d.meta))
                        quantize.note_choice(qm, st, d.head, d.actor, d.rows, d.meta, d.choice)
                    mem = quantize.end_turn(qm, st, mem)
                    write_turn_file(out_dir / f"{name}_w{wi}_t{ti}.txt", turn_arrays(turn, st, logits, mem, aim))
            print(f"{out_dir}/{name}.bin: {size} bytes; windows of {[len(w) for w in windows]} turns")
        # The `small` net with the turn trace on and its layer zero (a net
        # warm-started from a checkpoint without the trace, before a step):
        # version 3, and it computes what small.bin does.
        small = build_model(layout, CONFIGS["small"])
        zero = Net(layout.config(trace=1), layout)
        missing, unexpected = zero.load_state_dict(small.state_dict(), strict=False)
        assert missing == ["trace.weight"] and not unexpected and not zero.trace.weight.any()
        size = export.write(quantize.quantize(zero, layout), out_dir / "small-zero-trace.bin")
        print(f"{out_dir}/small-zero-trace.bin: {size} bytes")
        # The same net with the events on and xattn.out zero (a net
        # warm-started from one without events, before a step): version 4,
        # and on the windows with events (whose other arrays are the plain
        # windows') it computes what small.bin does on the plain ones.
        ev_layout = layouts[True]
        zero = Net(ev_layout.config(events=1), ev_layout)
        missing, unexpected = zero.load_state_dict(small.state_dict(), strict=False)
        assert not unexpected and all(m.split(".")[0] in ("in_event", "event", "xattn", "id_emb") for m in missing), missing
        assert not zero.xattn.out.weight.any() and not zero.xattn.out.bias.any()
        size = export.write(quantize.quantize(zero, ev_layout), out_dir / "small-zero-events.bin")
        print(f"{out_dir}/small-zero-events.bin: {size} bytes")


if __name__ == "__main__":
    make(sys.argv[1] if len(sys.argv) > 1 else "fixtures")
