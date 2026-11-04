# nn: the trainer of the end-to-end neural AI

PyTorch side of `docs/ai-neural-architecture.md`: reads the shards of
`docs/ai-net-data.md` (formats 1 … 4), trains by imitation, quantises, writes
the weights file that `crates/classic-rules/src/ai/net_e2e/model.rs` plays
with. The model, memory, flags, the integer scheme and the file format are in
`docs/ai-net-model.md`.

## Setup

    python3 -m venv ../../.venv-nn && ../../.venv-nn/bin/pip install -r requirements.txt

## Modules (`aurelia_nn/`)

| Module | What |
|---|---|
| `common.py` | config, presets (small, medium, large), constants, id and engram hashing |
| `data.py` | shard loader (`Shard`, `Turn`, windows of consecutive turns), split by game, batches of one map size, `collate` to tensors |
| `materialize.py` | shards on demand from replay logs: a pool runs the feature binary into temporary directories, loads, deletes |
| `model.py` | the float model, its memory (`MemoryBank`) |
| `train_imitation.py` | the trainer: windows with truncated BPTT, cross-entropy over legal candidates (waits weighed down), top-1 per head on held-out games, checkpoints, weights file; `--optim adam|muon`; flags `--engram --aim --aux --privileged-value --coarse`; the turn trace (`--turn-trace`, on by default; `--no-turn-trace`: weights version 2); the events (`--events`, on by default for format-7 data; weights version 4) |
| `train_e2.py` | E2 policy gradient on the net's own games (`ai-selfplay`, shards by `ai-features --e2` on the fly): GRPO advantage per game (or GAE from the value head), PPO clip over the accepted candidates, off-policy masking, KL to the frozen imitation net, entropy, value regression; rewards `science` (default), `victory`, `points_share`; `--aim` for aim-conditioned games |
| `e2_loop.py` | the E2 loop: play groups with the current int8 weights (a league of past checkpoints), train, export, evaluate on the bench (standard: all heads and hybrid; pace), resumable from its state directory, `progress.csv` |
| `quantize.py` | int8 quantisation and the exact integer reference forward (NumPy int64), memory included |
| `export.py` | the weights file (write, read) |
| `synthetic.py` | contract-conformant synthetic shards (with a rule that needs memory); also a stand-in for the feature binary |
| `fixture.py` | the Rust parity fixtures |

## Use

Train on shard directories:

    python -m aurelia_nn.train_imitation --data ../../work/nn-data/e1-smoke --out runs/e1 --preset small

Train on replay logs, materialising shards on demand:

    python -m aurelia_nn.train_imitation --logs <dir-or-list.txt> \
        --features-bin "<path/to/ai-features>" --workers 6 --out runs/e1

Fine-tune on a mixture of the expert's logs and DAgger shards (half the
window batches from each; expert-val and dagger-val reported separately; the
best checkpoint by the mean of both):

    python -m aurelia_nn.train_imitation --logs <expert logs> --features-bin "<ai-features>" \
        --data <dagger shards> --mix 0.5 --init runs/e1/best.pt --out runs/e1-dagger

`runs/e1/` gets `best.pt`, `last.pt`, `weights.bin`, `summary.json`
(held-out top-1 per head per epoch, the integer reference's agreement).

Self-play (E2), resumable; the state directory holds checkpoints, evals,
`progress.csv`, `loop.log`:

    python -m aurelia_nn.e2_loop --state ../../work/nn-data/e2/run1 --init ../../work/nn-data/weights/small-prod10.pt --hours 10

Quantise a checkpoint by hand: `python -m aurelia_nn.export runs/e1/best.pt weights.bin`.

## Tests

    ../../.venv-nn/bin/python -m pytest tests

The Rust half of the parity check: `cargo +1.98.1 test --release -p classic-rules --test net_e2e_model`;
timings: the same with `-- --ignored --nocapture` (`NET_E2E_WEIGHTS=a.bin:b.bin`,
`NET_E2E_MAPS=64x48x150`).
