"""Shards materialised on demand from replay logs.

The dataset is a set of replay logs (tens of KB per game); the feature
binary (`ai-features <log> --out <dir> [--turn-sample N]`, crate `qa`)
replays a log and writes the shard of classic/docs/ai-net-data.md. This
module runs that binary in a pool of workers into temporary directories,
reads each shard into memory, deletes the directory and hands the shard to
the trainer, so the replays overlap with training.

The binary's path is configurable (`--features-bin`, or the environment
variable `AURELIA_FEATURES_BIN`); any program with that command line works,
which is how the tests drive it with the synthetic generator.
"""

from __future__ import annotations

import os
import shlex
import shutil
import subprocess
import tempfile
from collections import deque
from concurrent.futures import Future, ThreadPoolExecutor
from pathlib import Path
from typing import Iterable, Iterator

import numpy as np

from .data import Shard, WindowIndex


def default_binary() -> str | None:
    return os.environ.get("AURELIA_FEATURES_BIN")


class LogMaterializer:
    """Runs the feature binary for one log at a time; `materialize` returns
    the shard in memory (its directory already deleted)."""

    def __init__(self, binary: str | list[str], turn_sample: int | None = None, tmp_root: str | None = None, timeout: float = 1800.0):
        self.command = shlex.split(binary) if isinstance(binary, str) else list(binary)
        self.turn_sample = turn_sample
        self.tmp_root = tmp_root or os.environ.get("TMPDIR") or tempfile.gettempdir()
        self.timeout = timeout

    def materialize(self, log: str | os.PathLike) -> Shard:
        work = tempfile.mkdtemp(prefix="aurelia-shard-", dir=self.tmp_root)
        try:
            # `ai-features` writes `<out>/<log stem>.shard/`.
            out = Path(work) / (Path(log).stem + ".shard")
            cmd = self.command + [str(log), "--out", str(work)]
            if self.turn_sample and self.turn_sample > 1:
                cmd += ["--turn-sample", str(self.turn_sample)]
            proc = subprocess.run(cmd, capture_output=True, text=True, timeout=self.timeout)
            if proc.returncode != 0:
                raise RuntimeError(f"{' '.join(cmd)} failed ({proc.returncode}): {proc.stderr.strip()[-2000:]}")
            if not (out / "meta.json").exists() and (Path(work) / "meta.json").exists():
                out = Path(work)  # a writer that puts the shard at `--out` itself
            if not (out / "meta.json").exists():
                # A self-play log (`g00012m00-…`) gives a shard named by its game's spec.
                made = [d for d in Path(work).iterdir() if (d / "meta.json").exists()]
                if len(made) == 1:
                    out = made[0]
            shard = Shard(out, in_memory=True)
            shard.game = Path(log).name
            return shard
        finally:
            shutil.rmtree(work, ignore_errors=True)


def stream_shards(logs: Iterable[str | os.PathLike], materializer: LogMaterializer, workers: int = 4, prefetch: int | None = None) -> Iterator[Shard]:
    """Shards of `logs` in order, `workers` binaries running at once and at
    most `prefetch` (default 2 × workers) shards waiting in memory. A log
    whose replay fails is reported and skipped."""
    logs = list(logs)
    prefetch = prefetch or 2 * workers
    with ThreadPoolExecutor(max_workers=workers) as pool:
        pending: deque[tuple[str, Future]] = deque()
        it = iter(logs)
        for log in it:
            pending.append((str(log), pool.submit(materializer.materialize, log)))
            if len(pending) >= prefetch:
                break
        while pending:
            log, fut = pending.popleft()
            nxt = next(it, None)
            if nxt is not None:
                pending.append((str(nxt), pool.submit(materializer.materialize, nxt)))
            try:
                yield fut.result()
            except Exception as e:  # noqa: BLE001 - one bad log must not stop a run
                print(f"skip {log}: {e}", flush=True)


def stream_batches(shards: Iterable[Shard], windows_per_batch: int, games: int = 8, max_len: int = 0, rng: np.random.Generator | None = None):
    """Batches of turn windows over a moving group of `games` shards: windows
    of one map size are batched together across the group's games."""
    buf: list[Shard] = []
    for shard in shards:
        buf.append(shard)
        if len(buf) >= games:
            yield from WindowIndex(buf, max_len).batches(windows_per_batch, rng)
            buf = []
    if buf:
        yield from WindowIndex(buf, max_len).batches(windows_per_batch, rng)
