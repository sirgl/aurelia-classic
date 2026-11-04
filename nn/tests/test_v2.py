"""The v2 net (docs/ai-net-model.md, «v2 architecture»): it trains behind
the `v2` preset and round-trips through a checkpoint; the held-out siege
scenes keep every copy of a scene together."""

from pathlib import Path

import torch

from aurelia_nn import synthetic, train_imitation
from aurelia_nn.data import load_shards
from aurelia_nn.model import load_checkpoint
from aurelia_nn.model_v2 import NetV2, build_net
from aurelia_nn.v2_bench import holdout_split, scene_of


def test_v2_trains_and_its_checkpoint_loads(tmp_path):
    root = tmp_path / "data"
    synthetic.write_dataset(root, games=2, rounds=6)
    out = tmp_path / "run"
    summary = train_imitation.main(["--data", str(root), "--out", str(out), "--preset", "v2", "--epochs", "1", "--max-steps", "2",
                                    "--windows-per-batch", "2", "--device", "cpu", "--optim", "muon", "--schedule", "wsd", "--max-minutes", "5",
                                    "--val-fraction", "0.5"])
    assert summary["weights_bytes"] == 0  # float only: no weights file
    model = load_checkpoint(out / "best.pt")
    assert isinstance(model, NetV2)
    shard = load_shards(root)[0]
    same = build_net(model.cfg, shard.layout)
    same.load_state_dict(model.state_dict())
    assert all(torch.equal(a, b) for a, b in zip(model.state_dict().values(), same.state_dict().values()))


class _S:
    def __init__(self, name):
        self.path = Path("/x") / name


def test_the_holdout_keeps_the_copies_of_a_scene_together():
    names = [f"{dup}search-random-r{r}-g{g:05d}m00-2000{s}-standard-pangaea-duel-siege.shard"
             for r in (26, 27, 30, 34) for g, s in enumerate(range(1001, 1041)) for dup in ("", "dup2-", "dup3-")]
    names += [f"scripted-siege-r29-g{g:05d}m00-10{g:04d}-standard-fractal-duel-siege.shard" for g in range(40)]
    kept, held = holdout_split([_S(n) for n in names], "27-34:0.25")
    held_keys = {scene_of(s.path)[1] for s in held}
    assert all(27 <= scene_of(s.path)[0] <= 34 for s in held)
    assert not any(scene_of(s.path)[1] in held_keys for s in kept)
    assert 0.1 < len(held_keys) / 160 < 0.4
    assert scene_of("dup2-search-random-r27-g00001m00-200000002-standard-pangaea-duel-siege.shard") == scene_of(
        "search-random-r27-g00007m00-200000002-standard-pangaea-duel-siege.shard")
