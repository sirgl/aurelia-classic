import json
import os
import sys
from pathlib import Path

import numpy as np
import pytest
import torch

from aurelia_nn import data, export, fixture, quantize, synthetic, train_imitation
from aurelia_nn.common import PRESETS, fnv1a64
from aurelia_nn.materialize import LogMaterializer, stream_batches, stream_shards
from aurelia_nn import model as model_module
from aurelia_nn.model import MemoryBank, Net, memory_batch, parameter_count

NN = Path(__file__).resolve().parents[1]
FIXTURE = NN.parent / "crates/classic-rules/tests/fixtures/net_e2e"
ALL_FLAGS = dict(engram=1, aim=1, coarse=1, aux=1, privileged_value=1, trace=1)


@pytest.fixture(scope="module")
def dataset(tmp_path_factory):
    root = tmp_path_factory.mktemp("shards")
    synthetic.write_dataset(root, games=16, rounds=16)
    return root


def test_fnv_known_values():
    assert fnv1a64(b"") == 0xCBF29CE484222325
    assert fnv1a64(b"a") == 0xAF63DC4C8601EC8C


def test_loader_reads_the_contract(dataset):
    shards = data.load_shards(dataset)
    assert len(shards) == 16
    s = shards[0]
    cfg = s.layout.config()
    assert (cfg.c_tile, cfg.c_global, cfg.c_entity, cfg.c_candidate) == (12, 10, 8, 6)
    assert cfg.id_tile == (2, 3) and cfg.id_entity == (2,) and cfg.engram_entity_small == (3,)
    assert s.n_regions == 4 * 3  # 16×12 in 4×4 blocks
    assert [len(w) for w in s.windows] == [16, 16]  # format 2: windows.npy, one per player
    t = s.turn(5)
    assert t.tiles.shape == (s.n_tiles, cfg.c_tile)
    assert t.entity_meta.shape == (len(t.entities), 3) and t.entity_ids.shape == (len(t.entities),)
    for d in t.decisions:
        assert 0 <= d.choice < len(d.rows)
        assert ((d.meta[:, 2] >= -1) & (d.meta[:, 2] < len(t.entities))).all()
        if d.head in (0, 2):
            assert 0 <= d.actor < len(t.entities)
    train, val = data.split_by_game(shards)
    assert train and val and not {x.game for x in train} & {x.game for x in val}
    for batch in data.WindowIndex(shards, 5).batches(4, np.random.default_rng(0)):
        assert len({sh.map_key for sh, _ in batch}) == 1 and all(len(rows) <= 5 for _, rows in batch)


def test_format_1_builds_windows_and_nation_ids(tmp_path):
    s = data.Shard(synthetic.write_shard(tmp_path / "f1.shard", 5, 12, 8, rounds=6, fmt=1))
    assert [len(w) for w in s.windows] == [6, 6]
    for w in s.windows:
        assert len({int(s.turns[t, 1]) for t in w}) == 1
        assert list(s.turns[w, 0]) == list(range(6))
    t = s.turn(0)
    kinds = t.entity_meta[:, 0]
    assert (t.entity_ids[kinds == 0] == -1).all()  # no unit ids before format 2
    assert (t.entity_ids[np.isin(kinds, (4, 5))] >= 0).all()


def test_layout_counts_come_from_meta(tmp_path):
    layout = synthetic.default_layout(c_tile=7, c_global=3, c_entity=5, c_candidate=9)
    synthetic.write_shard(tmp_path / "g.shard", 3, 8, 8, rounds=2, layout=layout)
    s = data.Shard(tmp_path / "g.shard")
    cfg = s.layout.config()
    assert (cfg.c_tile, cfg.c_global, cfg.c_entity, cfg.c_candidate) == (7, 3, 5, 9)
    net = Net(cfg, s.layout)
    logits, value = net(data.collate([s.turn(0)], cfg))
    assert logits.shape[0] == sum(len(d.rows) for d in s.turn(0).decisions)


def test_preset_sizes():
    """Parameters with the shipped layout's feature counts (47/247/86/90)."""
    layout = data.Layout.from_json(synthetic.default_layout(c_tile=47, c_global=247, c_entity=86, c_candidate=90))
    sizes = {p: parameter_count(Net(layout.config(p), layout)) for p in PRESETS}
    assert 150_000 < sizes["small"] < 400_000, sizes
    assert 0.8e6 < sizes["medium"] < 1.6e6, sizes
    assert 3.5e6 < sizes["large"] < 6e6, sizes


@pytest.mark.parametrize("flags", [{}, ALL_FLAGS])
def test_export_round_trip(dataset, flags):
    s = data.load_shards(dataset)[0]
    torch.manual_seed(0)
    cfg = s.layout.config(**flags)
    net = Net(cfg, s.layout)
    qm = quantize.quantize(net, s.layout)
    qm2 = export.from_bytes(export.to_bytes(qm))
    assert qm2.layout_hash == s.layout.hash()
    m1 = m2 = None
    for t in s.windows[0][:3]:
        turn = s.turn(int(t))
        st1, l1 = quantize.turn_logits(qm, turn, m1)
        st2, l2 = quantize.turn_logits(qm2, turn, m2)
        m1, m2 = quantize.end_turn(qm, st1, m1), quantize.end_turn(qm2, st2, m2)
        assert (st1.tokens == st2.tokens).all() and all((a == b).all() for a, b in zip(l1, l2))
    with pytest.raises(ValueError):
        export.from_bytes(export.to_bytes(qm)[:-1])


def test_committed_fixtures_match_the_reference():
    """The Rust test reads the same files: both sides are pinned to them."""
    checked = 0
    for name in fixture.CONFIGS:
        qm = export.read(FIXTURE / f"{name}.bin")
        for w in range(len(fixture.WINDOWS)):
            mem = quantize.Memory.empty(qm.cfg)
            for t in range(fixture.WINDOWS[w][3]):
                a = fixture.read_turn_file(FIXTURE / f"{name}_w{w}_t{t}.txt")
                st = quantize.encode_turn(qm, a["tiles"], a["neighbors"], a["region"], a["global"], a["entities"], a["entity_meta"], a["entity_ids"], mem,
                                          coarse=a["coarse"], aim=a["aim"] if len(a["aim"]) else None, events=a.get("events"), event_meta=a.get("event_meta"))
                assert (st.tile_h.ravel() == a["expect_tile_h"]).all()
                assert (st.tokens.ravel() == a["expect_tokens"]).all()
                assert st.value == a["expect_value"][0]
                cc = qm.cfg.c_candidate
                rows, metas = a["candidates"].reshape(-1, cc), a["candidate_meta"].reshape(-1, 4)
                logits = []
                for (h, actor, f, k), choice in zip(a["decisions"].reshape(-1, 4), a["choices"]):
                    logits.append(quantize.score(qm, st, h, actor, rows[f : f + k], metas[f : f + k]))
                    quantize.note_choice(qm, st, h, actor, rows[f : f + k], metas[f : f + k], int(choice))
                assert (np.concatenate(logits) == a["expect_logits"]).all()
                mem = quantize.end_turn(qm, st, mem)
                assert (mem.player == a["expect_mem_player"]).all()
                assert sorted(mem.units) == list(a["expect_mem_unit_ids"])
                assert sorted(mem.nations) == list(a["expect_mem_nation_ids"])
                checked += 1
    assert checked == len(fixture.CONFIGS) * 6
    assert export.read(FIXTURE / "trace.bin").cfg.trace == 1 and export.read(FIXTURE / "small.bin").cfg.trace == 0


def _random_trace(net: Net, std: float = 0.3):
    with torch.no_grad():
        net.trace.weight.normal_(0, std)


@pytest.mark.parametrize("pad", [False, True])
def test_turn_trace_is_the_sum_of_the_choices_before(dataset, pad):
    """Net.score's vectorised turn trace equals the trace built decision by
    decision: each decision's context adds W_tr · clamp(Σ / 2^shift), Σ the
    clamped candidate encodings of the turn's earlier choices of the traced
    heads, waits and other heads left out."""
    from aurelia_nn.common import HEAD_PRODUCTION, HEAD_UNIT

    s = data.load_shards(dataset)[0]
    torch.manual_seed(2)
    cfg = s.layout.config(trace=1, trace_heads=0b0101)  # unit and production only
    net = Net(cfg, s.layout).eval()
    _random_trace(net)
    turns = [s.turn(int(t)) for t in s.windows[0][:5]]
    batch = data.collate(turns, cfg, pad=pad)
    captured = {}
    net.cand.register_forward_hook(lambda m, i, o: captured.__setitem__("enc", o))
    with torch.no_grad():
        got = padded_logits_of(net, batch)
        enc = captured["enc"]
        tile_h, tokens, off = net.encode(batch)
        expect, row, d_i = [], 0, 0
        for b, turn in enumerate(turns):
            acc = torch.zeros(cfg.d_model)
            for d in turn.decisions:
                k = len(d.rows)
                uses_actor = d.head in (HEAD_UNIT, HEAD_PRODUCTION) and d.actor >= 0
                if uses_actor:
                    ctx = tokens[b, off["ent"] + d.actor].clone()
                    is_slot = int(turn.entity_meta[d.actor, 0]) in (0, 4, 5)
                    if is_slot:
                        ctx += tokens[b, off["slots"] + d.actor]
                else:
                    ctx = tokens[b, off["global"]] + tokens[b, off["player"]]
                ctx = ctx + net.trace((acc / 8).clamp(-128, 128))
                q = net.head_query[d.head](ctx)
                e = enc[row : row + k]
                logits = net.head_logit[d.head](torch.relu(net.head_hidden[d.head](torch.relu(e + q)))).squeeze(-1)
                expect.append(torch.cat([logits, logits.new_full((got.shape[1] - k,), float("-inf"))]))
                if d.head in (HEAD_UNIT, HEAD_PRODUCTION) and not d.is_wait:
                    acc = acc + e[d.choice].clamp(-128, 128)
                row += k
                d_i += 1
    assert d_i and torch.allclose(got[:d_i], torch.stack(expect), atol=1e-4)


def padded_logits_of(net: Net, batch: dict) -> torch.Tensor:
    from aurelia_nn.model import padded_logits

    return padded_logits(batch, net(batch)[0])


def test_the_turn_trace_starts_as_the_net_without_it(dataset, tmp_path):
    """--init from a checkpoint without the trace: W_tr starts at zero, so
    the float logits are the old net's exactly, and the integer net of the
    version-3 file computes bit for bit what the version-2 file did; once
    W_tr moves, the logits move. A version-2 file reads with the trace off."""
    from aurelia_nn.model import save_checkpoint

    s = data.load_shards(dataset)[0]
    torch.manual_seed(7)
    old = Net(s.layout.config(), s.layout).eval()
    save_checkpoint(tmp_path / "old.pt", old)
    new = Net(s.layout.config(trace=1), s.layout).eval()
    report = train_imitation.init_from(new, str(tmp_path / "old.pt"))
    assert report["new"] == ["trace.weight"] and not report["shape_changed"]
    turns = [s.turn(int(t)) for t in s.windows[0][:4]]
    with torch.no_grad():
        assert torch.equal(old(data.collate(turns, old.cfg))[0], new(data.collate(turns, new.cfg))[0])
    b2, b3 = export.to_bytes(quantize.quantize(old, s.layout)), export.to_bytes(quantize.quantize(new, s.layout))
    assert b2[4:8] == (2).to_bytes(4, "little") and b3[4:8] == (3).to_bytes(4, "little")
    q2, q3 = export.from_bytes(b2), export.from_bytes(b3)
    assert q2.cfg.trace == 0 and q3.cfg.trace == 1 and (q3.layers["trace"].w == 0).all()
    m2 = m3 = None
    for turn in turns:
        s2, l2 = quantize.turn_logits(q2, turn, m2)
        s3, l3 = quantize.turn_logits(q3, turn, m3)
        assert (s3.trace != 0).any() or not turn.decisions
        assert all((a == b).all() for a, b in zip(l2, l3)) and len(l2) == len(l3)
        m2, m3 = quantize.end_turn(q2, s2, m2), quantize.end_turn(q3, s3, m3)
    _random_trace(new)
    q3 = quantize.quantize(new, s.layout)
    m3 = None
    moved = 0
    for turn in turns:
        s2, l2 = quantize.turn_logits(q2, turn, None)
        s3, l3 = quantize.turn_logits(q3, turn, None)
        moved += sum(int((a != b).sum()) for a, b in zip(l2, l3))
    assert moved > 0


def test_integer_primitives():
    v = np.array([0, 1, 2, 3, 10**12, 2**60 + 12345], dtype=np.int64)
    s = quantize.isqrt(v)
    assert list(s[:4]) == [0, 1, 1, 1] and s[4] == 10**6
    assert quantize.round_shift(np.int64(-3), 1) == -1  # −1.5 rounds half up
    assert quantize.div_round(np.int64(-3), np.int64(2)) == -1
    assert quantize.div_round(np.int64(5), np.int64(2)) == 3


def test_training_learns_and_integer_agrees(dataset, tmp_path):
    """Imitation with memory on synthetic games learns the hidden rules, and
    the integer net, carrying its own memory, picks the float net's top-1 on
    ≥ 99 % of held-out decisions."""
    summary = train_imitation.main(
        ["--data", str(dataset), "--out", str(tmp_path / "run"), "--epochs", "8", "--device", "cpu", "--windows-per-batch", "4", "--window", "8",
         "--eval-int", "60", "--val-fraction", "0.25"]
    )
    assert summary["best_val"]["all"] > 0.6
    check = summary["int_check"]
    assert check["decisions"] >= 300
    assert check["float_int"] >= 0.99, check
    assert (tmp_path / "run" / "weights.bin").stat().st_size == summary["weights_bytes"]


def test_a_fifth_head_comes_from_meta(tmp_path):
    """The heads are meta.json's: a religion head trains, reports and exports."""
    synthetic.write_dataset(tmp_path / "d", games=6, rounds=8, religion=True)
    summary = train_imitation.main(["--data", str(tmp_path / "d"), "--out", str(tmp_path / "run"), "--epochs", "1", "--device", "cpu",
                                    "--windows-per-batch", "4", "--window", "8", "--eval-int", "16", "--val-fraction", "0.3"])
    assert summary["best_val"]["decisions"]["religion"] > 0
    assert export.read(tmp_path / "run" / "weights.bin").cfg.n_decision_heads == 5


def test_init_carries_matching_tensors_into_a_model_with_a_new_head(dataset, tmp_path):
    """--init: a 4-head checkpoint starts a 5-head model; matching tensors
    load, the per-head embeddings keep their old rows, the new head's layers
    are reported new."""
    first = train_imitation.main(["--data", str(dataset), "--out", str(tmp_path / "a"), "--epochs", "1", "--max-steps", "2", "--device", "cpu",
                                  "--windows-per-batch", "4", "--window", "4", "--eval-int", "0"])
    assert first["init"] is None
    synthetic.write_dataset(tmp_path / "d5", games=4, rounds=6, religion=True)
    second = train_imitation.main(["--data", str(tmp_path / "d5"), "--out", str(tmp_path / "b"), "--epochs", "1", "--max-steps", "1", "--device", "cpu",
                                   "--windows-per-batch", "4", "--window", "4", "--eval-int", "0", "--init", str(tmp_path / "a" / "best.pt")])
    report = second["init"]
    assert "head_query.4.weight" in report["new"] and "head_logit.4.bias" in report["new"]
    assert any(e.startswith("tag_emb.weight") for e in report["extended"]) and any(e.startswith("param_emb.weight") for e in report["extended"])
    assert report["loaded"] > 50 and not report["shape_changed"]
    old = torch.load(tmp_path / "a" / "best.pt", weights_only=False)["state"]["tag_emb.weight"]
    cfg = data.load_shards(tmp_path / "d5")[0].layout.config()
    net = Net(cfg, data.load_shards(tmp_path / "d5")[0].layout)
    train_imitation.init_from(net, str(tmp_path / "a" / "best.pt"))
    assert torch.equal(net.tag_emb.weight[: old.shape[0]], old)


def _widen(layout: data.Layout, turn: data.Turn, rng) -> tuple:
    """A newer layout with numeric columns added in every table (one inside,
    one at the end, as a data format adds them), and `turn` in it with random
    values in the new columns."""
    import dataclasses

    j = layout.to_json()
    at = {}
    for t in ("tile", "global", "entity", "candidate"):
        at[t] = (1, len(j[t]) + 1)
        j[t] = j[t][:1] + [{"name": f"new_{t}_inside", "scale": 10}] + j[t][1:] + [{"name": f"new_{t}_end", "scale": 1}]
    wide = lambda a, t: np.insert(np.insert(np.asarray(a), at[t][0], rng.integers(-60, 60, np.asarray(a).shape[:-1]), axis=-1), at[t][1], rng.integers(-60, 60, np.asarray(a).shape[:-1]), axis=-1).astype(np.int16)
    decisions = [dataclasses.replace(d, rows=wide(d.rows, "candidate")) for d in turn.decisions]
    out = dataclasses.replace(turn, tiles=wide(turn.tiles, "tile"), global_row=wide(turn.global_row, "global"), entities=wide(turn.entities, "entity"),
                              decisions=decisions, dec_arrays=None)
    return data.Layout.from_json(j), out


@pytest.mark.parametrize("flags", [{}, ALL_FLAGS])
def test_init_extends_the_input_layers_exactly_for_new_columns(dataset, tmp_path, flags):
    """--init from a checkpoint of an older feature layout (a new data
    format: columns added in every table): the input layers take the old
    weights by column name and zero weights for the new columns, so the
    integer net computes bit for bit what the old one did — tokens, value,
    logits, memory — whatever the new columns hold; the float net agrees."""
    from aurelia_nn.data import collate
    from aurelia_nn.model import save_checkpoint

    s = data.load_shards(dataset)[0]
    torch.manual_seed(5)
    old = Net(s.layout.config(**flags), s.layout).eval()
    save_checkpoint(tmp_path / "old.pt", old)
    rng = np.random.default_rng(1)
    turns = [s.turn(int(t)) for t in s.windows[0][:4]]
    layout = None
    wide_turns = []
    for turn in turns:
        layout, w = _widen(s.layout, turn, rng)
        wide_turns.append(w)
    new = Net(layout.config(**flags), layout).eval()
    report = train_imitation.init_from(new, str(tmp_path / "old.pt"))
    assert len(report["columns"]) == 4 and not report["shape_changed"] and not report["new"], report
    assert all(c.endswith("2 new (zero), 0 dropped") for c in report["columns"])
    assert layout.hash() != s.layout.hash()

    qa, qb = quantize.quantize(old, s.layout), quantize.quantize(new, layout)
    ma = mb = None
    for turn, w in zip(turns, wide_turns):
        sa, la = quantize.turn_logits(qa, turn, ma)
        sb, lb = quantize.turn_logits(qb, w, mb)
        assert (sa.tokens == sb.tokens).all() and sa.value == sb.value
        assert all((a == b).all() for a, b in zip(la, lb)) and len(la) == len(lb)
        ma, mb = quantize.end_turn(qa, sa, ma), quantize.end_turn(qb, sb, mb)
        assert (ma.player == mb.player).all()
    with torch.no_grad():
        fa = old(collate(turns, old.cfg))[0]
        fb = new(collate(wide_turns, new.cfg))[0]
    assert all(torch.allclose(a, b, atol=1e-4) for a, b in zip(fa, fb))


def test_decision_weights(tmp_path):
    synthetic.write_dataset(tmp_path / "d", games=1, rounds=6, religion=True)
    shard = data.load_shards(tmp_path / "d")[0]
    cfg = shard.layout.config()
    rules = train_imitation.parse_decision_weights(["religion=0.5", "religion_save=0.25", "policy:500=2", "wait=0.1"], shard.layout)
    # `religion_save` is a tag name of meta.json: the rule is that tag's.
    assert rules == [(4, None, 0.5), (None, 601, 0.25), (3, 500, 2.0), (None, 250, 0.1)]
    assert train_imitation.parse_decision_weights(["religion:religion_save=0.25"], shard.layout) == [(4, 601, 0.25)]
    turns = [shard.turn(int(t)) for t in shard.windows[0]]
    batch = data.collate(turns, cfg)
    w = train_imitation.decision_weights(batch, 1.0, rules)
    heads, tags = batch["dec_head"], batch["dec_tag"]
    expect = torch.ones_like(w)
    expect[heads == 4] *= 0.5
    expect[tags == 601] *= 0.25
    expect[(heads == 3) & (tags == 500)] *= 2.0
    expect[tags == 250] *= 0.1
    assert torch.allclose(w, expect) and (heads == 4).any()
    with pytest.raises(SystemExit):
        train_imitation.parse_decision_weights(["nonsense=1"], shard.layout)


def test_mixture_of_expert_logs_and_dagger_shards(dataset, tmp_path, monkeypatch):
    """--logs and --data together: batches from both, each held out and
    reported on its own; works with --init and --decision-weight."""
    monkeypatch.setenv("PYTHONPATH", str(NN) + os.pathsep + os.environ.get("PYTHONPATH", ""))
    logs = tmp_path / "logs"
    logs.mkdir()
    for i in range(6):
        (logs / f"game{i}.json").write_text(json.dumps({"seed": 70 + i, "width": 16, "height": 12, "rounds": 8, "players": 2}))
    first = train_imitation.main(["--data", str(dataset), "--out", str(tmp_path / "a"), "--epochs", "1", "--max-steps", "2", "--device", "cpu",
                                  "--windows-per-batch", "4", "--window", "4", "--eval-int", "0"])
    summary = train_imitation.main(
        ["--logs", str(logs), "--features-bin", f"{sys.executable} -m aurelia_nn.synthetic", "--data", str(dataset), "--mix", "0.5",
         "--out", str(tmp_path / "mix"), "--epochs", "2", "--device", "cpu", "--windows-per-batch", "4", "--window", "8", "--eval-int", "20",
         "--val-fraction", "0.3", "--workers", "2", "--init", str(tmp_path / "a" / "best.pt"), "--decision-weight", "research=0.5"]
    )
    assert summary["sources"] == ["expert", "dagger"] and summary["mix"] == 0.5
    by = summary["best_val_by_source"]
    assert by["expert"]["decisions"]["unit"] > 0 and by["dagger"]["decisions"]["unit"] > 0
    assert summary["init"]["loaded"] > 50 and summary["decision_weights"] == [[1, None, 0.5]]
    assert summary["int_check"]["decisions"] > 0


def test_flags_and_muon_train(dataset, tmp_path):
    """Every flag on and the Muon optimiser: a few steps run, export works."""
    summary = train_imitation.main(
        ["--data", str(dataset), "--out", str(tmp_path / "run"), "--epochs", "1", "--max-steps", "3", "--device", "cpu", "--windows-per-batch", "4",
         "--window", "4", "--eval-int", "8", "--optim", "muon", "--engram", "--engram-buckets", "1024", "--aim", "--aux", "--privileged-value", "--coarse"]
    )
    assert summary["flags"]["engram"] == 1 and summary["optim"] == "muon"
    assert summary["int_check"]["decisions"] > 0


def test_logs_are_materialised_on_demand(tmp_path, monkeypatch):
    monkeypatch.setenv("PYTHONPATH", str(NN) + os.pathsep + os.environ.get("PYTHONPATH", ""))
    logs = []
    for i in range(3):
        p = tmp_path / f"game{i}.json"
        p.write_text(json.dumps({"seed": 40 + i, "width": 12, "height": 8, "rounds": 6, "players": 2}))
        logs.append(p)
    tmp_root = tmp_path / "tmp"
    tmp_root.mkdir()
    mat = LogMaterializer([sys.executable, "-m", "aurelia_nn.synthetic"], turn_sample=2, tmp_root=str(tmp_root))
    shards = list(stream_shards(logs, mat, workers=2))
    assert [s.game for s in shards] == [p.name for p in logs]
    assert not any(tmp_root.iterdir()), "temporary shards are deleted"
    assert all(len(s) == 6 for s in shards)  # 12 player-turns, 1 in 2 kept
    cfg = shards[0].layout.config()
    batches = list(stream_batches(iter(shards), 4, games=2))
    assert sum(len(b) for b in batches) == sum(len(s.windows) for s in shards)
    bank = MemoryBank(cfg, "cpu")
    shard, rows = batches[0][0]
    logits, _ = Net(cfg, shards[0].layout)(memory_batch([bank], data.collate([shard.turn(int(rows[0]))], cfg), cfg))
    assert torch.isfinite(logits).all()


def _reference_run_windows(model, windows, cfg, device, stats, value_weight=0.0, wait_weight=1.0, opt=None, bptt=8, aux_weight=0.0, rules=None):
    """The trainer's step as first written: the windows' turn k batched, one
    forward per turn, memory in MemoryBank dicts (memory_batch,
    store_memory), the loss summed over `bptt` turns. run_windows must
    compute the same loss, gradients and memory."""
    from aurelia_nn.model import padded_logits, store_memory

    banks = [MemoryBank(cfg, device) for _ in windows]
    steps = max(len(w) for w in windows)
    loss_sum, weight_sum, turns_run = 0.0, 0.0, 0
    for k in range(steps):
        active = [i for i, w in enumerate(windows) if k < len(w)]
        turns = [windows[i][k] for i in active]
        act_banks = [banks[i] for i in active]
        batch = data.collate(turns, cfg, device)
        for key in ("region_pool", "coarse_pool", "dec_groups", "cand_groups", "k_max"):  # the forward without the collate's helpers
            batch.pop(key, None)
        batch = memory_batch(act_banks, batch, cfg)
        logits, value, (new_p, new_n, new_u), aux_pred = model(batch, with_memory=True, with_aux=True)
        store_memory(act_banks, batch, new_p, new_n, new_u)
        turns_run += len(turns)
        if len(batch["dec_b"]):
            pad = padded_logits(batch, logits)
            ce = torch.nn.functional.cross_entropy(pad, batch["dec_choice"], reduction="none")
            stats.add(batch, pad.detach(), ce.detach())
            if opt is not None:
                w = train_imitation.decision_weights(batch, wait_weight, rules).to(ce.dtype)
                loss_sum = loss_sum + (ce * w).sum()
                weight_sum += float(w.sum().item())
        if opt is not None:
            vt = batch["value_target"]
            ok = ~torch.isnan(vt)
            if value_weight > 0 and ok.any():
                loss_sum = loss_sum + value_weight * torch.nn.functional.mse_loss(value[ok], vt[ok], reduction="sum")
            al = train_imitation.aux_loss(model, aux_pred, batch) if aux_weight > 0 else None
            if al is not None:
                loss_sum = loss_sum + aux_weight * al
            if (k + 1) % bptt == 0 or k == steps - 1:
                if torch.is_tensor(loss_sum):
                    loss = loss_sum / max(weight_sum, 1.0)
                    opt.zero_grad(set_to_none=True)
                    loss.backward()
                    torch.nn.utils.clip_grad_norm_(model.parameters(), 1.0)
                    opt.step()
                loss_sum, weight_sum = 0.0, 0.0
                for b in banks:
                    b.detach()
    return turns_run


def _block_forward(self, x, mask):
    """model.Block.forward as first written: the attention spelled out."""
    import math

    b, n, d = x.shape
    h = self.heads
    dh = d // h
    a = torch.nn.functional.layer_norm(x, (d,), eps=1e-5)
    q, k, v = (t.view(b, n, h, dh).transpose(1, 2) for t in self.qkv(a).split(d, dim=-1))
    s = (q @ k.transpose(-1, -2)) / math.sqrt(dh)
    s = s.masked_fill(~mask[:, None, None, :], float("-inf"))
    x = x + self.out((torch.softmax(s, dim=-1) @ v).transpose(1, 2).reshape(b, n, d))
    a = torch.nn.functional.layer_norm(x, (d,), eps=1e-5)
    return x + self.ff2(torch.relu(self.ff1(a)))


def _masked_per_head(self, x, heads, groups, fn, width):
    """Net.per_head as first written: a boolean mask per head."""
    out = x.new_zeros(len(x), width)
    for h in range(self.cfg.n_decision_heads):
        sel = heads == h
        if sel.any():
            out[sel] = fn(h, x[sel])
    return out


@pytest.mark.parametrize("flags", [{}, ALL_FLAGS])
def test_run_windows_matches_the_turn_by_turn_reference(dataset, flags, monkeypatch):
    """run_windows (memory-free part batched over a BPTT piece, memory in
    device tables, heads grouped once, fused attention, dense max pooling)
    trains exactly like the turn-by-turn loop as first written: the same parameters after several optimiser steps, the same
    top-1 counts and loss. Windows of unequal length (cut at 5 turns),
    pieces of 3 turns, waits, decision weights, value and aux losses."""
    shards = data.load_shards(dataset)
    layout = shards[0].layout
    cfg = layout.config("small", **flags)
    rules = train_imitation.parse_decision_weights(["research=0.5", "wait=0.3"], layout)
    batches = list(data.WindowIndex(shards[:6], 5).batches(4, np.random.default_rng(1)))[:3]
    assert any(len({len(rows) for _, rows in b}) > 1 for b in batches), "some batch mixes window lengths"
    results = []
    for path in ("reference", "new"):
        torch.manual_seed(3)
        model = Net(cfg, layout)
        opt = torch.optim.SGD(model.parameters(), lr=0.05)
        stats = train_imitation.HeadStats(layout.heads)
        with monkeypatch.context() as m:
            if path == "reference":
                m.setattr(Net, "per_head", _masked_per_head)
                m.setattr(model_module.Block, "forward", _block_forward)
            run = _reference_run_windows if path == "reference" else train_imitation.run_windows
            for b in batches:
                run(model, train_imitation.window_turns(b), cfg, "cpu", stats, 0.5, 0.25, opt, 3, 0.2 if flags else 0.0, rules)
        results.append(({k: v.detach().clone() for k, v in model.state_dict().items()}, stats.report()))
    (ref_state, ref_stats), (new_state, new_stats) = results
    moved = max(float((ref_state[k] - v).abs().max()) for k, v in Net(cfg, layout).state_dict().items() if k in ref_state and v.is_floating_point())
    assert moved > 1e-3, "training moved the weights"
    for k, v in ref_state.items():
        assert torch.allclose(v, new_state[k], rtol=1e-4, atol=1e-5), (k, float((v - new_state[k]).abs().max()))
    assert ref_stats["decisions"] == new_stats["decisions"] and ref_stats["all"] == new_stats["all"]
    assert abs(ref_stats["ce"] - new_stats["ce"]) < 1e-5


def test_window_memory_reads_what_memory_bank_holds(dataset):
    """WindowMemory plans the same reads as memory_batch over MemoryBank:
    nation slots kept from any earlier turn, unit slots only from the
    previous one, a repeated id from its last entity."""
    from aurelia_nn.model import WindowMemory, store_memory

    shard = data.load_shards(dataset)[0]
    cfg = shard.layout.config()
    window = [shard.turn(int(t)) for t in shard.windows[0]]
    # A repeated unit id on the third turn: the last entity's slot is kept.
    units = np.nonzero(window[2].entity_meta[:, 0] == 0)[0]
    assert len(units) >= 2
    window[2].entity_ids = window[2].entity_ids.copy()
    window[2].entity_ids[units[0]] = window[2].entity_ids[units[1]]
    mem = WindowMemory([window], cfg)
    mem.start("cpu")
    bank = MemoryBank(cfg, "cpu")
    rng = torch.Generator().manual_seed(0)
    for k, turn in enumerate(window):
        batch = memory_batch([bank], data.collate([turn], cfg), cfg)
        (step,) = mem.views(mem.segment([(k, [0])], batch["ent_id"].shape[1], data.Uploader()))
        p, n, u = mem.read(step)
        assert torch.equal(p, batch["mem_player"]) and torch.equal(n, batch["mem_nation"]) and torch.equal(u, batch["mem_unit"])
        e = batch["ent_id"].shape[1]
        new_p = torch.randn(1, cfg.mem_player, generator=rng)
        new_n = torch.randn(1, e, cfg.mem_nation, generator=rng)
        new_u = torch.randn(1, e, cfg.mem_unit, generator=rng)
        store_memory([bank], batch, new_p, new_n, new_u)
        mem.player = new_p
        src, dst = step["wn"]
        mem.nation = mem.nation.index_put((dst,), new_n.reshape(-1, cfg.mem_nation)[src])
        src, dst = step["wu"]
        mem.unit = mem.unit.index_put((dst,), new_u.reshape(-1, cfg.mem_unit)[src])


def test_dense_pool_matches_the_scatter_pool():
    """Net.pool with the collate's layout (a dense max over padded groups)
    gives the scatter's values and gradients, also where the max ties with
    the zero every group includes (the gradient is shared among ties)."""
    torch.manual_seed(0)
    groups = torch.tensor([0, 0, 1, 2, 2, 2, 1, 0, 3, 3])
    h = torch.relu(torch.randn(3, len(groups), 5))
    h[:, groups == 3] = 0.0  # a group whose max is a three-way tie at zero
    w = torch.randn(3, 4, 10)
    grads = []
    for layout in (None, data.pool_layout(groups.numpy(), 4)):
        x = h.clone().requires_grad_()
        out = Net.pool(x, groups, 4, layout)
        (out * w).sum().backward()
        grads.append((out.detach(), x.grad))
    assert torch.allclose(grads[0][0], grads[1][0]) and torch.allclose(grads[0][1], grads[1][1])


def test_prefetch_keeps_order_and_raises():
    assert list(train_imitation.prefetch(range(5), lambda x: x * x)) == [0, 1, 4, 9, 16]

    def bad(x):
        if x == 2:
            raise ValueError("boom")
        return x

    got = []
    with pytest.raises(ValueError):
        for v in train_imitation.prefetch(range(5), bad):
            got.append(v)
    assert got == [0, 1]
    it = train_imitation.prefetch(iter(range(100)), lambda x: x)
    assert next(it) == 0
    it.close()  # stops the worker


def test_a_turn_is_read_as_the_player_saw_it_at_its_start(tmp_path):
    """Format 8: turns.npy's fifth column is the entity rows of the turn's
    start; the rows the turn appended later (a unit or city it named) are
    not the turn's, and a decision's actor or target among them is −1, as
    the playing net's cache has no row for it (model.rs, `n_entities`)."""
    path = synthetic.write_shard(tmp_path / "f8.shard", 5, 12, 8, rounds=4, fmt=2)
    turns = np.load(path / "turns.npy")
    cut = 3
    np.save(path / "turns.npy", np.concatenate([turns, (turns[:, 3:4] - cut)], 1).astype("<i4"))
    full = data.Shard(path)
    t = next(t for t in range(len(full)) if full.decision_count(t))
    first, count = int(full.turns[t, 2]), int(full.turns[t, 3])
    assert full.start_count(t) == count - cut
    turn = full.turn(t)
    assert len(turn.entities) == count - cut and len(turn.entity_ids) == count - cut
    dec = full.decisions[full._dec_order[full._dec_start[t] : full._dec_start[t + 1]]]
    for d, raw in zip(turn.decisions, dec):
        actor = int(raw[2]) - first
        assert d.actor == (actor if 0 <= actor < count - cut else -1)
        meta = np.asarray(full.candidate_meta[raw[3] : raw[3] + raw[4], 2]) - first
        assert (d.meta[:, 2] == np.where((meta >= 0) & (meta < count - cut), meta, -1)).all()


def test_before_format_8_the_start_ends_with_the_nations():
    """Before format 8 the start of a turn is read from the kinds: the rows
    of the turn's start end with the city-states and nations (kinds 4, 5);
    the turn appends units and cities (kinds 0–3) after them."""
    import types

    def start(kinds):
        s = types.SimpleNamespace(turns=np.array([[0, 0, 0, len(kinds)]]), entity_meta=np.array([[k, 0, 0] for k in kinds]))
        return data.Shard.start_count(s, 0)

    assert start([0, 0, 1, 2, 3, 4, 5, 5, 3, 2, 0]) == 8, "a city taken, a unit seen, one bought"
    assert start([0, 1, 4, 5]) == 4, "nothing appended"
    assert start([0, 0, 1, 1, 0]) == 4, "no nation met: up to the first kind below its predecessor"
    assert start([0, 1, 2]) == 3


def test_a_decision_weight_by_a_column_of_the_choice(dataset):
    """`--decision-weight column:NAME=W`: decisions whose expert choice has
    the candidate column NAME nonzero (format 8: `blow_captures`) weigh W,
    times the other rules they match."""
    s = data.load_shards(dataset)[0]
    name = s.layout.names["candidate"][1]
    rules = train_imitation.parse_decision_weights([f"column:{name}=4", "unit=2"], s.layout)
    assert rules[0] == (None, None, 4.0, 1)
    turns = [s.turn(t) for t in range(4)]
    batch = data.collate(turns, s.layout.config(), pad=True)
    w = train_imitation.host_decision_weights(batch, 1.0, rules)
    chosen = batch["np"]["dec_row"][:, 1] != 0
    unit = batch["np"]["dec_head"] == 0
    assert chosen.any() and (~chosen).any()
    assert np.allclose(w, np.where(chosen, 4.0, 1.0) * np.where(unit, 2.0, 1.0))
    with pytest.raises(SystemExit):
        train_imitation.parse_decision_weights(["column:no_such_column=2"], s.layout)
