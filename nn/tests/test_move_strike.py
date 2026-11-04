"""Format 10 (classic/docs/ai-net-data.md, «Format 10: the strike from a
move»): a format-9 net carried into the layout with the move-strike columns
(`aurelia_nn.move_strike`) scores every candidate as before, whatever the new
columns hold — float and integer — and its weights file widened in place is
the quantised carried checkpoint byte for byte."""

import copy
import json

import numpy as np
import pytest
import torch

from aurelia_nn import data, export, quantize, synthetic
from aurelia_nn.data import Decision
from aurelia_nn.defer import with_defer
from aurelia_nn.model import Net, load_checkpoint, padded_logits, save_checkpoint
from aurelia_nn.move_strike import FORMAT_10_CANDIDATE, format_9_of, is_format_10, main, widen_bin, widen_checkpoint, widened_layout


@pytest.fixture(scope="module")
def shard(tmp_path_factory):
    root = tmp_path_factory.mktemp("shards")
    synthetic.write_dataset(root, games=2, rounds=10)
    return data.load_shards(root)[0]


def turns_of(shard, n=5):
    return [shard.turn(int(t)) for t in shard.windows[0][:n]]


def widened_turns(turns, k, seed=0):
    """The turns with `k` more candidate columns of random values (what the
    format-10 columns hold is not the old net's business)."""
    rng = np.random.default_rng(seed)
    out = []
    for turn in turns:
        turn = copy.copy(turn)
        turn.decisions = [
            Decision(d.head, d.actor, np.concatenate([d.rows, rng.integers(0, 100, (len(d.rows), k)).astype(d.rows.dtype)], axis=1), d.meta, d.choice, d.is_wait)
            for d in turn.decisions
        ]
        turn.dec_arrays = None
        out.append(turn)
    return out


def logits(net, turns):
    with torch.no_grad():
        batch = data.collate(turns, net.cfg)
        return padded_logits(batch, net(batch)[0])


def carried(shard, tmp_path, defer=True):
    torch.manual_seed(21)
    old = Net(shard.layout.config(trace=1), shard.layout)
    with torch.no_grad():
        old.trace.weight.normal_(0, 0.3)
    if defer:
        old = with_defer(old, -2.0)
    save_checkpoint(tmp_path / "old.pt", old)
    old = load_checkpoint(tmp_path / "old.pt").eval()
    layout = widened_layout(shard.layout)
    return old, widen_checkpoint(old, str(tmp_path / "old.pt"), layout), layout


def test_the_layout_gains_the_columns_after_format_9s(shard):
    layout = widened_layout(shard.layout)
    n = shard.layout.count("candidate")
    assert layout.names["candidate"][:n] == shard.layout.names["candidate"]
    assert [(a, b) for a, b in zip(layout.names["candidate"][n:], layout.scales["candidate"][n:])] == list(FORMAT_10_CANDIDATE)
    assert is_format_10(layout) and not is_format_10(shard.layout)
    assert format_9_of(layout).hash() == shard.layout.hash()
    assert widened_layout(shard.layout, layout).hash() == layout.hash()
    other = copy.deepcopy(layout)
    other.names["tile"] = list(other.names["tile"][::-1])
    with pytest.raises(ValueError):
        widened_layout(shard.layout, other)
    with pytest.raises(ValueError):
        widened_layout(layout)


def test_a_carried_net_scores_every_candidate_as_before(shard, tmp_path):
    old, new, layout = carried(shard, tmp_path)
    assert new.cfg.defer == old.cfg.defer == 1 and new.cfg.trace == old.cfg.trace
    assert new.cfg.c_candidate == old.cfg.c_candidate + len(FORMAT_10_CANDIDATE)
    plain = turns_of(shard)
    wide = widened_turns(plain, len(FORMAT_10_CANDIDATE))
    lo, ln = logits(old, plain), logits(new, wide)
    keep = torch.isfinite(lo)
    assert keep.any() and torch.equal(keep, torch.isfinite(ln))
    assert torch.allclose(lo[keep], ln[keep], atol=1e-5)
    q_old, q_new = quantize.quantize(old, shard.layout), quantize.quantize(new, layout)
    for a, b in zip(plain, wide):
        (sa, xa), (sb, xb) = quantize.turn_logits(q_old, a), quantize.turn_logits(q_new, b)
        assert all((x == y).all() for x, y in zip(xa, xb))
        assert (sa.trace == sb.trace).all()


def test_the_widened_weights_file_is_the_quantised_carried_net(shard, tmp_path):
    old, new, layout = carried(shard, tmp_path)
    q_old = quantize.quantize(old, shard.layout)
    widened = widen_bin(export.from_bytes(export.to_bytes(q_old)), shard.layout, layout)
    assert export.to_bytes(widened) == export.to_bytes(quantize.quantize(new, layout))
    assert widened.layout_hash == layout.hash() and widened.cfg.c_candidate == layout.count("candidate")
    with pytest.raises(ValueError, match="layout hash"):
        widen_bin(widened, shard.layout, layout)


def test_the_command_line_carries_a_checkpoint_and_a_weights_file(shard, tmp_path):
    old, _, layout = carried(shard, tmp_path)
    (tmp_path / "old.bin").write_bytes(export.to_bytes(quantize.quantize(old, shard.layout)))
    main([str(tmp_path / "old.pt"), str(tmp_path / "new.pt")])
    net = load_checkpoint(tmp_path / "new.pt")
    assert net.cfg.defer == 1 and data.Layout.from_json(net.layout_json).hash() == layout.hash()
    assert (tmp_path / "new.bin").read_bytes() == export.to_bytes(quantize.quantize(net, layout))
    # A weights file alone, its layout read off a format-10 shard's meta.
    shard10 = tmp_path / "shard10"
    shard10.mkdir()
    (shard10 / "meta.json").write_text(json.dumps({"format": 10, "layout": layout.to_json()}))
    main([str(tmp_path / "old.bin"), str(tmp_path / "alone.bin"), "--layout", str(shard10)])
    assert (tmp_path / "alone.bin").read_bytes() == (tmp_path / "new.bin").read_bytes()
    with pytest.raises(SystemExit):
        main([str(tmp_path / "old.bin"), str(tmp_path / "x.bin")])
