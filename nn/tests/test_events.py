"""The events (format 7; classic/docs/ai-net-model.md, Events): read from
the shard, collated, attended by every token; zero event weights compute the
net without events bit for bit; the event layers get gradient and train."""

import pytest
import torch

from aurelia_nn import data, export, quantize, synthetic, train_imitation
from aurelia_nn.model import Net, save_checkpoint


@pytest.fixture(scope="module")
def shards(tmp_path_factory):
    """The same synthetic games without and with events (the other arrays
    equal: the events are drawn from a generator of their own)."""
    root = tmp_path_factory.mktemp("events")
    synthetic.write_dataset(root / "plain", games=4, rounds=10, religion=True)
    synthetic.write_dataset(root / "events", games=4, rounds=10, religion=True, events=True)
    return data.load_shards(root / "plain"), data.load_shards(root / "events")


def test_events_are_read_and_collated(shards):
    plain, with_events = shards
    s = with_events[0]
    assert s.meta["format"] == 7 and s.layout.tables()[-1] == "event"
    assert plain[0].layout.tables() == data.TABLES and plain[0].turn(0).events is None
    cfg = s.layout.config(events=1)
    assert cfg.c_event == s.layout.count("event") and cfg.id_event == (0, 1)
    turns = [s.turn(i) for i in range(len(s))]
    counts = [len(t.event_meta) for t in turns]
    assert sum(counts) > len(turns) and 0 in counts, counts
    for t in turns:
        assert t.events.shape == (len(t.event_meta), cfg.c_event)
        assert ((t.event_meta[:, 2] >= -1) & (t.event_meta[:, 2] < len(t.entities))).all(), "actors are local entities"
    b = data.collate(turns[:6], cfg, pad=True)
    assert b["ev_rows"].shape[:2] == b["ev_mask"].shape and b["ev_mask"].sum() == sum(counts[:6])
    assert b["ev_mask"].shape[1] % 8 == 0
    # The layout's text names the event table last (the Rust layout_text).
    assert s.layout.canonical().splitlines()[-1].startswith("event:event_kind_id_plus_1/1;")
    assert s.layout.hash() != plain[0].layout.hash()


def _checkpoint(layout, path, **flags):
    torch.manual_seed(11)
    net = Net(layout.config(**flags), layout).eval()
    save_checkpoint(path, net)
    return net


@pytest.mark.parametrize("trace", [0, 1])
def test_zero_event_weights_compute_the_net_without_events(shards, tmp_path, trace):
    """--init from a checkpoint without events (a format-6 layout) into the
    format-7 layout with the events on: the event layers are new and
    xattn.out is zero, so the float logits are the old net's exactly, and
    the version-4 file computes bit for bit what the old file did —
    tokens, value, logits, memory — on turns with events; once xattn.out
    moves, the logits move."""
    plain, with_events = shards
    old = _checkpoint(plain[0].layout, tmp_path / "old.pt", trace=trace)
    layout = with_events[0].layout
    new = Net(layout.config(events=1, trace=trace), layout).eval()
    report = train_imitation.init_from(new, str(tmp_path / "old.pt"))
    assert not report["shape_changed"] and not report["columns"]
    assert {n.split(".")[0] for n in report["new"]} <= {"in_event", "id_emb", "event", "xattn"}
    assert not new.xattn.out.weight.any() and not new.xattn.out.bias.any()
    wa = [plain[0].turn(int(t)) for t in plain[0].windows[0][:5]]
    wb = [with_events[0].turn(int(t)) for t in with_events[0].windows[0][:5]]
    assert sum(len(t.event_meta) for t in wb) > 0
    with torch.no_grad():
        assert torch.equal(old(data.collate(wa, old.cfg))[0], new(data.collate(wb, new.cfg))[0])
    qa, qb = quantize.quantize(old, plain[0].layout), quantize.quantize(new, layout)
    ba, bb = export.to_bytes(qa), export.to_bytes(qb)
    assert bb[4:8] == (4).to_bytes(4, "little") and ba[4:8] == (3 if trace else 2).to_bytes(4, "little")
    qa, qb = export.from_bytes(ba), export.from_bytes(bb)
    assert qb.cfg.events == 1 and qb.cfg.c_event == layout.count("event") and qb.layout_hash == layout.hash()
    ma = mb = None
    for a, b in zip(wa, wb):
        sa, la = quantize.turn_logits(qa, a, ma)
        sb, lb = quantize.turn_logits(qb, b, mb)
        assert (sa.tokens == sb.tokens).all() and sa.value == sb.value
        assert all((x == y).all() for x, y in zip(la, lb)) and len(la) == len(lb)
        ma, mb = quantize.end_turn(qa, sa, ma), quantize.end_turn(qb, sb, mb)
        assert (ma.player == mb.player).all()
    with torch.no_grad():
        new.xattn.out.weight.normal_(0, 0.3)
    q = quantize.quantize(new, layout)
    moved = sum(int((x != y).sum()) for a, b in zip(wa, wb) for x, y in zip(quantize.turn_logits(qa, a)[1], quantize.turn_logits(q, b)[1]))
    assert moved > 0


def test_event_layers_get_gradient_and_train(shards, tmp_path):
    """From a net whose xattn.out is zero, the loss reaches xattn.out at
    once and, after a step, the event token and the attention's queries and
    keys; a turn without events adds nothing and passes no gradient."""
    _, with_events = shards
    layout = with_events[0].layout
    torch.manual_seed(2)
    net = Net(layout.config(events=1, trace=1), layout)
    turns = [with_events[0].turn(i) for i in range(8)]
    batch = data.collate(turns, net.cfg)
    opt = torch.optim.SGD(net.parameters(), lr=0.1)

    def step():
        opt.zero_grad()
        logits, _ = net(batch)
        from aurelia_nn.model import padded_logits

        loss = torch.nn.functional.cross_entropy(padded_logits(batch, logits), batch["dec_choice"])
        loss.backward()
        return {n: float(p.grad.abs().sum()) if p.grad is not None else 0.0 for n, p in net.named_parameters() if n.startswith(("event.", "xattn.", "id_emb.event"))}

    g0 = step()
    assert g0["xattn.out.weight"] > 0 and g0["xattn.out.bias"] > 0
    assert g0["event.weight"] == 0 and g0["xattn.q.weight"] == 0, "nothing passes a zero output layer"
    opt.step()
    g1 = step()
    for name in ("event.weight", "xattn.q.weight", "xattn.kv.weight", "id_emb.event.weight"):
        assert g1[name] > 0, (name, g1)
    # A turn without events: the cross-attention adds zero.
    empty = next(t for t in (with_events[0].turn(i) for i in range(len(with_events[0]))) if len(t.event_meta) == 0)
    b = data.collate([empty], net.cfg)
    emb = net.embed(b)
    tok = torch.cat([emb["prefix"], net.slot_player(torch.zeros(1, net.cfg.mem_player))[:, None]], dim=1)
    assert not net.xattn(tok, emb["ev_tok"], emb["ev_mask"]).any()


def test_imitation_trains_with_events_and_the_integer_net_agrees(shards, tmp_path):
    """train_imitation turns the events on for format-7 data, trains, and
    the integer net (version 4) picks the float net's top-1."""
    root = tmp_path / "d"
    synthetic.write_dataset(root, games=8, rounds=16, events=True)
    summary = train_imitation.main(["--data", str(root), "--out", str(tmp_path / "run"), "--epochs", "8", "--device", "cpu", "--windows-per-batch", "4",
                                    "--window", "8", "--eval-int", "60", "--val-fraction", "0.25"])
    assert summary["flags"]["events"] == 1
    assert summary["int_check"]["float_int"] >= 0.99, summary["int_check"]
    weights = (tmp_path / "run" / "weights.bin").read_bytes()
    assert weights[4:8] == (4).to_bytes(4, "little")
    assert export.from_bytes(weights).cfg.events == 1
