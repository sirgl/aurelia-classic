"""The defer choice on a checkpoint (classic/docs/ai-net-model.md, Defer;
format 9): the same net with `defer` on, its defer bias set and the defer
candidate's tag row zero (where the row is the defer's alone), written as a
checkpoint and a weights file (version 5).

    python -m aurelia_nn.defer IN.pt OUT.pt [--bias B] [--layout SHARD] [--off]

Every other tensor is the input's, so the net scores every other candidate
as before; the defer candidate's logit is the head's on a zero row plus B
(logits, default common.DEFER_BIAS_INIT). `--layout SHARD` also carries the
net into the layout of a shard (its meta.json): the input layers take the
old weights of each column by name and zero weights for a new one
(`train_imitation.init_from`), as a net of an older format is carried into
the current one. `--bias` on a net that already defers sets its bias.
`--off` writes the net without the choice (carried by `--layout` only): the
control a deferring net is compared with.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path

import torch

from .common import DEFER_BIAS_INIT
from .data import Layout
from .model import Net, load_checkpoint, save_checkpoint, zero_defer_tag


def with_defer(model: Net, bias: float | None, layout: Layout | None = None, source: str | None = None) -> Net:
    """`model` with the defer choice on and its bias `bias` (logits; None:
    the choice off); `layout` (with `source`, the checkpoint `model` was
    read from) carries it into another feature layout."""
    on = bias is not None
    if layout is None and bool(model.cfg.defer) == on:
        out = model
    else:
        target = layout or Layout.from_json(model.layout_json)
        keep = {k: v for k, v in model.cfg.as_dict().items() if not (k.startswith("c_") or k.startswith("id_") or k in ("n_decision_heads", "engram_entity_small", "n_aux", "n_truth"))}
        cfg = target.config(**{**keep, "defer": int(on)})
        out = Net(cfg, target)
        if layout is None:
            state = {k: v for k, v in model.state_dict().items() if k != "defer_bias"}
            missing, unexpected = out.load_state_dict(state, strict=False)
            if unexpected or any(m != "defer_bias" for m in missing):
                raise ValueError(f"the net does not carry: missing {missing}, unexpected {unexpected}")
        else:
            from .train_imitation import init_from

            init_from(out, source)
        if on:
            zero_defer_tag(out)
    if on:
        with torch.no_grad():
            out.defer_bias.fill_(float(bias))
    return out


def main():
    ap = argparse.ArgumentParser(description="Turn the defer choice on in a checkpoint and write its weights file.")
    ap.add_argument("checkpoint")
    ap.add_argument("out", help="the new checkpoint (.pt); the weights file beside it (.bin)")
    ap.add_argument("--bias", type=float, default=DEFER_BIAS_INIT, help="the defer candidate's logit bias (logits)")
    ap.add_argument("--layout", help="a shard directory: carry the net into its feature layout")
    ap.add_argument("--off", action="store_true", help="the net without the choice (the control)")
    a = ap.parse_args()
    model = load_checkpoint(a.checkpoint)
    layout = Layout.from_meta(json.loads((Path(a.layout) / "meta.json").read_text())) if a.layout else None
    net = with_defer(model, None if a.off else a.bias, layout, a.checkpoint)
    save_checkpoint(a.out, net)
    from . import export
    from .quantize import quantize

    out_bin = Path(a.out).with_suffix(".bin")
    size = export.write(quantize(net, Layout.from_json(net.layout_json)), out_bin)
    print(f"{a.out}; {out_bin}: {size} bytes, version {export.file_version(net.cfg)}, defer {'off' if a.off else a.bias} (layout hash {Layout.from_json(net.layout_json).hash():#018x})")


if __name__ == "__main__":
    main()
