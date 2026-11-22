"""Parity of the MLX tactical simulator with the Rust rules, on a dump of `ai-exam suite-dump`.

    TACSIM_DUMP=/path/parity.jsonl python -m unittest tests.test_tacsim_parity

Skipped without TACSIM_DUMP (the dump needs the Rust build). Checks: every step of every scene (legal
options, visibility, state, RNG) in free-running mode; the same replay twice gives the same state
(determinism).
"""
import os
import unittest

import mlx.core as mx

from aurelia_nn.tacsim import parity

DUMP = os.environ.get("TACSIM_DUMP")
LIMIT = int(os.environ.get("TACSIM_LIMIT", "40"))


@unittest.skipUnless(DUMP, "TACSIM_DUMP not set")
class ParityTest(unittest.TestCase):
    def test_every_step_matches(self):
        rep = parity.run(DUMP, mode="free", limit=LIMIT, show=5)
        self.assertEqual(rep["scenes_ok"], rep["scenes"], rep["examples"])
        self.assertGreater(sum(r["masks"] for r in rep["rows"]), 0)

    def test_deterministic(self):
        dumps = parity.load_lines(DUMP, limit=8)
        finals = []
        for _ in range(2):
            batch, geo, sim = parity.build(dumps)
            s = batch.initial_state()
            for k in range(30):
                rows = [d["steps"][k] if k < len(d["steps"]) else None for d in dumps]
                acts = [parity.step_action(r) if r else (0, 0, 0, 0, -1) for r in rows]
                a = {n: mx.array([x[i] for x in acts], dtype=mx.int32) for i, n in enumerate(["typ", "side", "slot", "verb", "tgt"])}
                s = sim.step(s, a)
            finals.append({k: v.tolist() for k, v in s.items() if k.startswith("u_") or k == "rng"})
        self.assertEqual(finals[0], finals[1])


if __name__ == "__main__":
    unittest.main()
