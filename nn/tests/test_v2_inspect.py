"""v2_inspect (docs/ai-net-model.md, «v2: inspection and hand edits»): the
recorder does not change what the net computes, a hand edit of the logit
layers and the defer bias is exactly the offline calibration the fit
scores, the fit finds a known temperature, and an edited checkpoint
loads."""

import numpy as np
import torch

from aurelia_nn import synthetic
from aurelia_nn.data import load_shards
from aurelia_nn.model import load_checkpoint
from aurelia_nn.model_v2 import build_net
from aurelia_nn.train_imitation import HeadStats, run_windows
from aurelia_nn.v2_inspect import (Calib, Recorder, apply_calibration, apply_edit, calibration_report, fit_calibration, matrix_stats, save_edited,
                                   segments)


def _net(tmp_path, **flags):
    from aurelia_nn.fixture import with_defer_candidates

    root = tmp_path / "data"
    synthetic.write_dataset(root, games=2, rounds=6)
    shards = load_shards(root)
    if flags.get("defer"):
        for s in shards:  # the defer candidate in every unit decision, chosen in every third
            s.turn = lambda t, turn=s.turn: with_defer_candidates([turn(t)], every=3)[0]
    cfg = shards[0].layout.config("v2", **flags)
    torch.manual_seed(0)
    model = build_net(cfg, shards[0].layout).eval()
    with torch.no_grad():  # the defer bias's effect visible, the heads' logits not tiny
        if hasattr(model, "defer_bias"):
            model.defer_bias.fill_(-1.0)
    return model, shards, cfg


def _logits(model, shards, cfg) -> dict:
    from aurelia_nn.data import WindowIndex
    from aurelia_nn.train_imitation import Prepared, window_turns

    cal = Calib(HeadStats(model.heads))
    with torch.no_grad():
        for b in WindowIndex(shards, 32).batches(2):
            run_windows(model, Prepared(window_turns(b), cfg, 8), cfg, torch.device("cpu"), cal)
    return cal.arrays()


def test_matrix_stats_sees_the_rank():
    torch.manual_seed(0)
    low = torch.randn(64, 3) @ torch.randn(3, 48)
    full = torch.randn(64, 48)
    assert matrix_stats(low)["eff_rank"] < 3.5
    assert matrix_stats(full)["eff_rank"] > 30
    w = torch.randn(64, 48)
    w[0, 0] = 100.0
    assert matrix_stats(w)["outliers"] == 1


def test_the_recorder_leaves_the_logits_alone(tmp_path):
    model, shards, cfg = _net(tmp_path, defer=1, trace=1)
    before = _logits(model, shards, cfg)
    rec = Recorder(model)
    rec.on = True
    after = _logits(model, shards, cfg)
    assert np.allclose(before["logits"], after["logits"], atol=1e-5)
    rep = rec.report()
    assert len(rep["attention"]) == cfg.n_attn and len(rep["map_blocks"]) == cfg.n_conv
    assert rep["ffn"][0]["n"] > 0 and rep["gru"]["player"]["rows"] > 0
    assert 0 < rep["attention"][0][0]["entropy_share"] <= 1.0


def test_a_logit_scale_and_the_defer_bias_in_the_weights_are_the_offline_calibration(tmp_path):
    model, shards, cfg = _net(tmp_path, defer=1)
    arr = _logits(model, shards, cfg)
    assert arr["defer"].any()
    scales = {"0": 0.7, "1": 1.3}
    apply_edit(model, "scale=unit:0.7")
    apply_edit(model, "scale=1:1.3")
    apply_edit(model, "defer=-2.5")
    edited = _logits(model, shards, cfg)
    logits, defer, seg, chosen, head, n = segments(arr)
    want = apply_calibration(logits, defer, head, seg, scales, -1.0, -2.5)
    assert np.allclose(edited["logits"], want.numpy(), atol=1e-4)


def test_the_fit_finds_a_known_temperature():
    rng = np.random.default_rng(0)
    n, k = 4000, 5
    true = rng.normal(size=(n, k)) * 2.0
    choice = np.array([rng.choice(k, p=np.exp(r) / np.exp(r).sum()) for r in true])
    arr = {"logits": (true * 2.5).reshape(-1).astype(np.float32), "defer": np.zeros(n * k, bool), "length": np.full(n, k, np.int32),
           "choice": choice.astype(np.int32), "head": np.zeros(n, np.int8), "wait": np.zeros(n, bool)}
    fit = fit_calibration(arr, 1, 0.0)
    assert abs(fit["scales"]["0"] - 0.4) < 0.04
    rep0 = calibration_report(arr, ("unit",))
    rep1 = calibration_report(arr, ("unit",), fit["scales"], 0.0, fit["defer_bias"])
    assert rep1["all"]["ce"] < rep0["all"]["ce"] and rep1["all"]["top1"] == rep0["all"]["top1"]


def test_an_edited_checkpoint_loads_with_its_edits(tmp_path):
    from aurelia_nn import train_imitation

    root = tmp_path / "data"
    synthetic.write_dataset(root, games=2, rounds=6)
    out = tmp_path / "run"
    train_imitation.main(["--data", str(root), "--out", str(out), "--preset", "v2", "--epochs", "1", "--max-steps", "1", "--windows-per-batch", "2",
                          "--device", "cpu", "--val-fraction", "0.5"])
    model = load_checkpoint(out / "best.pt")
    w = model.blocks[1].out.weight.detach().clone()
    apply_edit(model, "damp=1.2:0")
    save_edited(str(out / "best.pt"), model, ["damp=1.2:0"], str(tmp_path / "edited.pt"))
    again = load_checkpoint(tmp_path / "edited.pt")
    dh = w.shape[1] // model.blocks[1].heads
    assert torch.all(again.blocks[1].out.weight[:, 2 * dh : 3 * dh] == 0)
    assert torch.equal(again.blocks[1].out.weight[:, :dh], w[:, :dh])
    assert torch.load(tmp_path / "edited.pt", weights_only=False)["surgery"]["edits"] == ["damp=1.2:0"]


def test_neutral_edits_change_nothing(tmp_path):
    from aurelia_nn import train_imitation

    root = tmp_path / "data"
    synthetic.write_dataset(root, games=2, rounds=6)
    out = tmp_path / "run"
    train_imitation.main(["--data", str(root), "--out", str(out), "--preset", "v2", "--epochs", "1", "--max-steps", "1", "--windows-per-batch", "2",
                          "--device", "cpu", "--val-fraction", "0.5"])
    model = load_checkpoint(out / "best.pt")
    before = {k: v.clone() for k, v in model.state_dict().items()}
    for e in ("dist=all:1", "dist=cand:1", f"mix={out / 'best.pt'}:0.5", "damp=0.0:1", "cand-damp=2:1"):
        apply_edit(model, e)
    assert all(torch.allclose(before[k], v, atol=1e-6) for k, v in model.state_dict().items())
    apply_edit(model, "dist=0:2")
    b = model.blocks[0].dist_bias.detach()
    b0 = before["blocks.0.dist_bias"]
    assert torch.allclose(b[:, :13] - b[:, :13].mean(1, keepdim=True), 2 * (b0[:, :13] - b0[:, :13].mean(1, keepdim=True)), atol=1e-6)
