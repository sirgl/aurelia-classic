"""The remote player's server (docs/ai-net-model.md, «Remote MPS player»):
over a real socket, its choices on recorded windows are the offline
evaluation's (train_imitation.run_windows), with the memory carried along
each window and the turn trace forced by the recorded choices, also when
several games ask at once and are batched; the integer reference it can
run beside agrees with the small net."""

import shutil
import tempfile
import threading
from pathlib import Path

import numpy as np
import pytest
import torch

from aurelia_nn import synthetic
from aurelia_nn.data import WindowIndex, load_shards
from aurelia_nn.model import save_checkpoint
from aurelia_nn.model_v2 import build_net
from aurelia_nn.remote import Client, Player, Server, offline_choices


@pytest.fixture(scope="module")
def shards(tmp_path_factory):
    root = tmp_path_factory.mktemp("remote")
    synthetic.write_dataset(root, games=3, rounds=6, sizes=((16, 12),), events=True)
    return load_shards(root)


def _checkpoint(path: Path, preset: str, layout) -> str:
    torch.manual_seed(1)
    net = build_net(layout.config(preset, trace=1, events=1, defer=1), layout)
    with torch.no_grad():
        net.trace.weight.normal_(0, 0.05)  # the trace starts at zero: make it matter
        if hasattr(net, "xattn"):
            net.xattn.out.weight.normal_(0, 0.05)
    save_checkpoint(path, net)
    return str(path)


def _serve(checkpoint: str, int_ref: bool = False):
    """A server on a short socket path (macOS: 104 bytes) in a thread."""
    folder = tempfile.mkdtemp(prefix="aurm")
    sock = str(Path(folder) / "s")
    player = Player(checkpoint, "cpu", int_ref)
    server = Server(player, sock, gather_us=2000)
    threading.Thread(target=server.serve, daemon=True).start()
    for _ in range(200):
        if Path(sock).exists():
            break
        threading.Event().wait(0.01)
    return player, server, sock, folder


def _replay(client: Client, sid: int, window, c_event: int) -> list[np.ndarray]:
    out = []
    for t in window:
        client.begin(sid, t, c_event)
        prev = -1
        for d in t.decisions:
            out.append(client.score(sid, prev, d.head, d.actor, d.rows, d.meta))
            prev = d.choice
    return out


@pytest.mark.parametrize("preset", ["small", "v2"])
def test_the_server_chooses_as_the_offline_evaluation(shards, tmp_path, preset):
    ck = _checkpoint(tmp_path / f"{preset}.pt", preset, shards[0].layout)
    player, _server, sock, folder = _serve(ck)
    try:
        cfg = player.cfg
        idx = WindowIndex(shards, 32)
        windows = [[shards[si].turn(int(t)) for t in rows] for items in idx.groups.values() for si, rows in items]
        assert all(len(w) > 1 for w in windows)  # the memory is carried
        with torch.inference_mode():
            ref = offline_choices(player.model, windows, cfg, torch.device("cpu"))
        # Every window from its own client, all at once: the server batches them.
        got = [None] * len(windows)

        def play(i):
            c = Client(sock)
            assert c.hello()["layout_hash"] == shards[0].layout.hash()
            got[i] = _replay(c, i + 1, windows[i], cfg.c_event)

        threads = [threading.Thread(target=play, args=(i,)) for i in range(len(windows))]
        [t.start() for t in threads]
        [t.join() for t in threads]
        stats = Client(sock).stats()
        n = agree = 0
        for w, ref_w, got_w in zip(windows, ref, got):
            flat = [lg[: len(d.rows)] for t, ref_t in zip(w, ref_w) for d, lg in zip(t.decisions, ref_t)]
            assert len(flat) == len(got_w)
            for r, g in zip(flat, got_w):
                n += 1
                a, b = int(np.argmax(r)), int(np.argmax(g))
                agree += a == b
                assert np.abs(g / 256.0 - r).max() < 2e-3  # Q8 rounding
                assert a == b or r[a] - r[b] < 2 / 256  # a different pick only between logits Q8 cannot tell apart
        assert n > 100 and agree > 0.9 * n
        assert stats["score"]["mean_batch"] > 1.0  # several games in a batch
    finally:
        shutil.rmtree(folder, ignore_errors=True)


def test_the_integer_reference_beside_the_server_agrees_with_the_small_net(shards, tmp_path):
    ck = _checkpoint(tmp_path / "small.pt", "small", shards[0].layout)
    player, _server, sock, folder = _serve(ck, int_ref=True)
    try:
        c = Client(sock)
        w = [shards[0].turn(t) for t in range(len(shards[0]))]
        _replay(c, 1, w, player.cfg.c_event)
        counts = c.stats()["int_ref_agree_of"]
        agree, n = sum(v[0] for v in counts.values()), sum(v[1] for v in counts.values())
        assert n > 30 and agree / n > 0.9
    finally:
        shutil.rmtree(folder, ignore_errors=True)


def test_a_score_before_its_turn_is_an_error(shards, tmp_path):
    ck = _checkpoint(tmp_path / "small.pt", "small", shards[0].layout)
    _player, _server, sock, folder = _serve(ck)
    try:
        d = shards[0].turn(0).decisions[0]
        with pytest.raises(RuntimeError, match="SCORE before BEGIN"):
            Client(sock).score(5, -1, d.head, d.actor, d.rows, d.meta)
    finally:
        shutil.rmtree(folder, ignore_errors=True)
