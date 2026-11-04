"""A format-9 net carried into format 10 (classic/docs/ai-net-data.md,
«Format 10: the strike from a move»): the candidate rows gain fourteen columns
after format 9's (`FORMAT_10_CANDIDATE`, what a unit could strike from a
move's destination this turn and the next, how near the enemy is); the
net gets zero input weights on them, so it scores every candidate as
before, float and integer alike.

    python -m aurelia_nn.move_strike IN.pt OUT.pt [--bin IN.bin] [--layout SHARD]
    python -m aurelia_nn.move_strike IN.bin OUT.bin --layout SHARD

Writes OUT.pt and the weights file OUT.bin beside it (a weights file alone:
OUT.bin, its format-9 layout read off the format-10 shard's). The checkpoint is
carried as `train_imitation --init` carries a net into a new data format
(`init_from`: each input column's weights by name, zero for a new one;
every other tensor as it was). The weights file is widened in place
(`widen_bin`: the candidate layer's int8 weights get zero columns, the
candidate table its input multipliers, the file the new layout hash; no
other byte moves) from `--bin`, by default IN.bin beside IN.pt when there is
one, else quantised from OUT.pt; either way it is checked to equal the
quantised OUT.pt when the old file equals the quantised IN.pt. The new
layout is a shard's (`--layout SHARD`, its meta.json; its candidate columns
must extend the net's) or, by default, the net's with `FORMAT_10_CANDIDATE`
appended. The defer choice, the heads and every flag stay the net's.
"""

from __future__ import annotations

import argparse
import copy
import dataclasses
import json
from pathlib import Path

import numpy as np

from .common import ID_SUFFIX
from .data import Layout
from .model import Net, load_checkpoint, save_checkpoint

# The format-10 candidate columns and their scales, in the order the encoder
# appends them (crates/classic-rules/src/ai/net_e2e/encode.rs, `C`, after
# `preview_city_floor`; `net_e2e_mind.rs` holds the layout text's tail).
FORMAT_10_CANDIDATE = (
    ("move_blow_captures", 1.0),
    ("move_blow_kills", 1.0),
    ("move_blow_damage_percent", 1.0),
    ("move_city_damage_percent", 1.0),
    ("move_city_adjacent", 1.0),
    ("move_city_in_strike", 1.0),
    ("move_strike_targets", 1.0),
    ("move_enemy_distance", 1.0),
    ("move_wounded_enemy_distance", 1.0),
    ("move_wounded_enemy_hp_percent", 1.0),
    ("move_next_turn_targets", 1.0),
    ("move_next_turn_kills", 1.0),
    ("move_city_distance", 1.0),
    ("move_unseen_city_in_reach", 1.0),
)


def is_format_10(layout: Layout) -> bool:
    return all(n in layout.names["candidate"] for n, _ in FORMAT_10_CANDIDATE)


def widened_layout(old: Layout, target: Layout | None = None) -> Layout:
    """`old` with the format-10 candidate columns: `target`'s columns (every
    other table must be `old`'s, and old's candidate columns a prefix of
    target's, none of the new ones an id), else `FORMAT_10_CANDIDATE`
    appended. The heads and tags stay `old`'s."""
    new = copy.deepcopy(old)
    if target is None:
        if is_format_10(old):
            raise ValueError("the layout has the format-10 columns already")
        new.names["candidate"] = list(old.names["candidate"]) + [n for n, _ in FORMAT_10_CANDIDATE]
        new.scales["candidate"] = list(old.scales["candidate"]) + [s for _, s in FORMAT_10_CANDIDATE]
        return new
    for table in target.tables():
        if table == "candidate":
            continue
        if old.names.get(table) != target.names.get(table) or old.scales.get(table) != target.scales.get(table):
            raise ValueError(f"the target layout's {table} table is not the net's")
    if set(old.tables()) != set(target.tables()):
        raise ValueError(f"the tables differ: {old.tables()} against {target.tables()}")
    n = old.count("candidate")
    if target.names["candidate"][:n] != old.names["candidate"] or target.scales["candidate"][:n] != old.scales["candidate"]:
        raise ValueError("the net's candidate columns are not a prefix of the target's")
    if any(c.endswith(ID_SUFFIX) for c in target.names["candidate"][n:]):
        raise ValueError("a new candidate column is an id")
    new.names["candidate"] = list(target.names["candidate"])
    new.scales["candidate"] = list(target.scales["candidate"])
    return new


def format_9_of(layout: Layout) -> Layout:
    """The format-9 layout a format-10 `layout` extends: its candidate
    columns without the `FORMAT_10_CANDIDATE` tail."""
    k = len(FORMAT_10_CANDIDATE)
    tail = list(zip(layout.names["candidate"][-k:], layout.scales["candidate"][-k:]))
    if tail != [(n, float(s)) for n, s in FORMAT_10_CANDIDATE]:
        raise ValueError(f"the layout's candidate columns do not end with format 10's: {tail}")
    old = copy.deepcopy(layout)
    old.names["candidate"] = list(layout.names["candidate"][:-k])
    old.scales["candidate"] = list(layout.scales["candidate"][:-k])
    return old


def widen_checkpoint(model: Net, source: str, layout: Layout) -> Net:
    """The net of checkpoint `source` (`model`, read from it) in `layout`:
    its config but the layout's widths, every tensor carried by
    `train_imitation.init_from` (zero input weights on the new columns)."""
    from .train_imitation import init_from

    keep = {k: v for k, v in model.cfg.as_dict().items() if not (k.startswith("c_") or k.startswith("id_") or k in ("n_decision_heads", "engram_entity_small", "n_aux", "n_truth"))}
    out = Net(layout.config(**keep), layout)
    report = init_from(out, source)
    if report["new"] or report["shape_changed"] or report["unused"]:
        raise ValueError(f"the net does not carry: {report}")
    return out.eval()


def widen_bin(qm, old: Layout, new: Layout):
    """A quantised net (`export.from_bytes`) of layout `old` carried into
    `new`, whose candidate columns extend old's: the candidate layer's
    weights get zero columns where the new numeric columns enter its input
    (after the old numeric ones, before the id embeddings and the rest), the
    candidate table the new columns' input multipliers, the file `new`'s
    layout hash. Every other value stays."""
    if qm.layout_hash != old.hash():
        raise ValueError(f"the weights file's layout hash {qm.layout_hash:#018x} is not the net's layout's {old.hash():#018x}")
    widened_layout(old, new)  # checks that new extends old
    n_old, n_new = old.count("candidate"), new.count("candidate")
    cfg = qm.cfg
    if cfg.c_candidate != n_old or tuple(cfg.id_candidate) != old.id_columns("candidate"):
        raise ValueError("the weights file's candidate table is not the layout's")
    at = n_old - len(old.id_columns("candidate"))
    out = copy.deepcopy(qm)
    layer = out.layers["candidate"]
    w = np.asarray(layer.w)
    layer.w = np.concatenate([w[:, :at], np.zeros((w.shape[0], n_new - n_old), dtype=w.dtype), w[:, at:]], axis=1)
    out.inputs["candidate"] = np.concatenate([np.asarray(qm.inputs["candidate"], np.int64), new.input_multipliers("candidate")[n_old:]])
    out.cfg = dataclasses.replace(cfg, c_candidate=n_new)
    out.layout_hash = new.hash()
    return out


def main(argv=None):
    from . import export
    from .quantize import quantize

    ap = argparse.ArgumentParser(description="Carry a format-9 net into format 10 (zero weights on the move-strike columns).")
    ap.add_argument("source", help="the net's checkpoint (.pt), or a weights file (.bin) alone with --layout")
    ap.add_argument("out", help="the new checkpoint (.pt) with its weights file beside it (.bin); for a .bin source the new weights file")
    ap.add_argument("--bin", help="the net's weights file to widen (default: IN.bin beside IN.pt when there is one)")
    ap.add_argument("--layout", help="a format-10 shard directory: its layout (default: the net's with the format-10 columns appended)")
    a = ap.parse_args(argv)
    target = Layout.from_meta(json.loads((Path(a.layout) / "meta.json").read_text())) if a.layout else None
    if a.source.endswith(".bin"):
        if target is None:
            raise SystemExit("a weights file alone needs --layout (a format-10 shard): the file holds its layout's hash only")
        qm = export.read(a.source)
        old = format_9_of(target)
        old.heads, old.tags = target.heads, target.tags
        data = export.to_bytes(widen_bin(qm, old, target))
        Path(a.out).write_bytes(data)
        print(f"{a.out}: {len(data)} bytes, widened from {a.source}; layout hash {old.hash():#018x} -> {target.hash():#018x}")
        return
    model = load_checkpoint(a.source)
    old = Layout.from_json(model.layout_json)
    layout = widened_layout(old, target)
    net = widen_checkpoint(model, a.source, layout)
    save_checkpoint(a.out, net)
    out_bin = Path(a.out).with_suffix(".bin")
    requantised = export.to_bytes(quantize(net, layout))
    src = Path(a.bin) if a.bin else Path(a.source).with_suffix(".bin")
    if src.exists():
        old_bytes = src.read_bytes()
        data = export.to_bytes(widen_bin(export.from_bytes(old_bytes), old, layout))
        same_source = old_bytes == export.to_bytes(quantize(model, old))
        if same_source and data != requantised:
            raise SystemExit(f"{out_bin}: the widened file differs from the quantised checkpoint")
        note = f"widened from {src}; " + ("equals the quantised checkpoint" if data == requantised else "its source is not the quantised checkpoint")
    else:
        data, note = requantised, "quantised from the checkpoint"
    out_bin.write_bytes(data)
    print(f"{a.out}; {out_bin}: {len(data)} bytes, {note}; candidate columns {old.count('candidate')} -> {layout.count('candidate')}, layout hash {old.hash():#018x} -> {layout.hash():#018x}")


if __name__ == "__main__":
    main()
