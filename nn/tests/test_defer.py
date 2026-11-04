"""The defer choice (format 9, weights version 5; classic/docs/ai-net-model.md,
Defer): the float net, the integer reference and the weights file agree, a
net that gains the choice scores every other candidate as before, and a
deferral stays out of the turn trace."""

import argparse

import numpy as np
import pytest
import torch

from aurelia_nn import data, export, fixture, quantize, synthetic, train_imitation
from aurelia_nn.common import DEFER_TAG, HEAD_UNIT
from aurelia_nn.defer import with_defer
from aurelia_nn.model import Net, defer_tag_row, load_checkpoint, padded_logits, save_checkpoint


@pytest.fixture(scope="module")
def shard(tmp_path_factory):
    root = tmp_path_factory.mktemp("shards")
    synthetic.write_dataset(root, games=4, rounds=12)
    return data.load_shards(root)[0]


def turns_of(shard, n=5):
    return [shard.turn(int(t)) for t in shard.windows[0][:n]]


def logits(net, turns):
    with torch.no_grad():
        batch = data.collate(turns, net.cfg)
        return padded_logits(batch, net(batch)[0])


def test_the_defer_bucket_is_the_defers_alone(shard):
    cfg = shard.layout.config()
    assert cfg.tag_buckets == 128 and defer_tag_row(cfg) == HEAD_UNIT * 128 + 1 + DEFER_TAG % 127
    assert defer_tag_row(shard.layout.config(tag_buckets=16)) is None, "a shared bucket is left as it is"


def test_a_version_5_file_round_trips_and_older_ones_stay(shard):
    torch.manual_seed(3)
    net = Net(shard.layout.config(trace=1), shard.layout)
    b3 = export.to_bytes(quantize.quantize(net, shard.layout))
    deferring = with_defer(net, -2.5)
    qm = quantize.quantize(deferring, shard.layout)
    b5 = export.to_bytes(qm)
    assert b3[4:8] == (3).to_bytes(4, "little") and b5[4:8] == (5).to_bytes(4, "little")
    back = export.from_bytes(b5)
    assert back.cfg.defer == 1 and back.defer_bias == -640 and export.from_bytes(b3).cfg.defer == 0
    assert (back.tag_emb[defer_tag_row(back.cfg)] == 0).all()
    assert export.to_bytes(back) == b5


def test_a_net_that_gains_the_choice_scores_the_rest_as_before(shard):
    """`aurelia_nn.defer`: every tensor stays, so on turns without the defer
    candidate (and on turns where it is offered and not taken) every other
    candidate's logit is the old net's, float and integer alike; the defer
    candidates' logits move with the bias alone."""
    torch.manual_seed(5)
    old = Net(shard.layout.config(trace=1), shard.layout).eval()
    with torch.no_grad():
        old.trace.weight.normal_(0, 0.3)
    new = with_defer(old, -3.0).eval()
    plain = turns_of(shard)
    offered = fixture.with_defer_candidates(plain, every=0)
    assert torch.equal(logits(old, plain), logits(new, plain))
    lo, ln = logits(old, plain), logits(new, offered)
    is_defer = torch.zeros_like(ln, dtype=torch.bool)
    i = 0
    for turn in offered:
        for d in turn.decisions:
            is_defer[i, : len(d.rows)] = torch.as_tensor(d.meta[:, 0] == DEFER_TAG)
            i += 1
    assert is_defer.any()
    keep = ~is_defer & torch.isfinite(ln)
    assert torch.allclose(ln[keep], lo[torch.isfinite(lo)], atol=1e-6)  # float sums over other shapes
    shifted = with_defer(new, 1.0).eval()
    assert torch.allclose(logits(shifted, offered)[is_defer], ln[is_defer] + 4.0, atol=1e-5)
    q_old, q_new = quantize.quantize(old, shard.layout), quantize.quantize(new, shard.layout)
    for turn, with_offer in zip(plain, offered):
        _, a = quantize.turn_logits(q_old, turn)
        _, b = quantize.turn_logits(q_new, with_offer)
        for x, y, d in zip(a, b, with_offer.decisions):
            assert (y[: len(x)] == x).all()
            if d.head == HEAD_UNIT:
                assert len(y) == len(x) + 1


def test_a_deferral_stays_out_of_the_turn_trace(shard):
    """A decision whose choice is the defer candidate adds nothing to the
    trace (as a wait): every other decision scores as in the turn without
    the deferrals — in the float net and the integer reference."""
    torch.manual_seed(9)
    net = with_defer(Net(shard.layout.config(trace=1), shard.layout), 0.0).eval()
    with torch.no_grad():
        net.trace.weight.normal_(0, 0.5)
    deferring = fixture.with_defer_candidates(turns_of(shard), every=2)
    took = [[d.meta[d.choice, 0] == DEFER_TAG for d in t.decisions] for t in deferring]
    without = [data.Turn(**{**t.__dict__, "decisions": [d for d, x in zip(t.decisions, k) if not x], "dec_arrays": None}) for t, k in zip(deferring, took)]
    assert sum(map(sum, took)) >= 3
    la, lb = logits(net, deferring), logits(net, without)
    kept = [i for i, x in enumerate(x for k in took for x in k) if not x]
    widths = [len(d.rows) for t in without for d in t.decisions]
    for row, (i, k) in enumerate(zip(kept, widths)):
        assert torch.allclose(la[i, :k], lb[row, :k], atol=1e-5), row
    qm = quantize.quantize(net, shard.layout)
    for a, b, k in zip(deferring, without, took):
        (sa, xa), (sb, xb) = quantize.turn_logits(qm, a), quantize.turn_logits(qm, b)
        assert (sa.trace == sb.trace).all()
        assert all((x == y).all() for x, y in zip([x for x, t in zip(xa, k) if not t], xb))


def test_float_and_integer_agree_on_the_defer_choice(shard):
    torch.manual_seed(11)
    net = with_defer(Net(shard.layout.config(trace=1), shard.layout), 0.5).eval()
    qm = quantize.quantize(net, shard.layout)
    windows = [fixture.with_defer_candidates(turns_of(shard, 6))]
    got = quantize.agreement(net, qm, windows, net.cfg)
    assert got["decisions"] > 10 and got["float_int"] >= 0.9, got


def test_imitation_from_a_deferring_net_keeps_the_choice(shard, tmp_path):
    """`train_imitation --init` keeps the start's config beside its tensors:
    a deferring net trains and exports as one (weights version 5, its bias
    loaded); `--no-defer` turns the choice off."""
    torch.manual_seed(13)
    start = with_defer(Net(shard.layout.config(trace=1), shard.layout), -2.5)
    save_checkpoint(tmp_path / "start.pt", start)
    args = ["--data", str(shard.path.parent), "--epochs", "1", "--max-steps", "1", "--device", "cpu", "--windows-per-batch", "4", "--window", "4",
            "--eval-int", "0", "--init", str(tmp_path / "start.pt")]
    summary = train_imitation.main(args + ["--out", str(tmp_path / "on")])
    assert "defer_bias" not in summary["init"]["new"]
    assert load_checkpoint(tmp_path / "on" / "last.pt").cfg.defer == 1
    assert export.read(tmp_path / "on" / "weights.bin").cfg.defer == 1
    train_imitation.main(args + ["--out", str(tmp_path / "off"), "--no-defer"])
    assert export.read(tmp_path / "off" / "weights.bin").cfg.defer == 0


def test_a_checkpoint_with_config_keys_unknown_here_loads(shard, tmp_path):
    """A checkpoint of a newer tree (a training-only head: its config keys and
    its layers) loads as the net this code knows; a run takes none of them."""
    torch.manual_seed(17)
    net = with_defer(Net(shard.layout.config(trace=1), shard.layout), -2.5)
    save_checkpoint(tmp_path / "new.pt", net)
    ck = torch.load(tmp_path / "new.pt", weights_only=False)
    ck["config"].update(future_head=1, future_head_cols=[3, 4])
    ck["state"]["future_head.proj.weight"] = torch.zeros(2, 2)
    torch.save(ck, tmp_path / "new.pt")
    back = load_checkpoint(tmp_path / "new.pt")
    assert back.cfg.defer == 1 and all(torch.equal(v, net.state_dict()[k]) for k, v in back.state_dict().items())
    flags = train_imitation.flag_config(argparse.Namespace(engram=0, engram_buckets=1 << 14, engram_dim=16, aim=0, aux=0, privileged_value=0,
                                                           coarse=0, turn_trace=1, events=0))
    carried = train_imitation.carried_config(str(tmp_path / "new.pt"), "small", flags)
    assert carried["defer"] == 1 and carried["trace_heads"] == net.cfg.trace_heads and carried["id_buckets"] == net.cfg.id_buckets
    assert not set(carried) & ({"future_head", "future_head_cols", "c_tile", "id_entity", "d_model"} | set(flags))
    del ck["state"]["defer_bias"]
    torch.save(ck, tmp_path / "new.pt")
    with pytest.raises(ValueError, match="lacks"):
        load_checkpoint(tmp_path / "new.pt")
