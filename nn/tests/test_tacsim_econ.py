"""Parity of the MLX economy with the Rust rules: the yields of every tile of every round of recorded
whole games, and the multi-turn path costs of a whole-map scene dump.

    TACSIM_ECON=/path/econ.jsonl TACSIM_WHOLE=/path/whole.jsonl python -m unittest tests.test_tacsim_econ

Skipped without the dumps (they need the Rust build: `AURELIA_ECON_DUMP=FILE ai-tournament ...`,
`ai-exam suite-dump --whole-map --paths N`).
"""
import os
import unittest

from aurelia_nn.tacsim import econ, parity

ECON = os.environ.get("TACSIM_ECON")
WHOLE = os.environ.get("TACSIM_WHOLE")


@unittest.skipUnless(ECON, "TACSIM_ECON not set")
class TileYieldsTest(unittest.TestCase):
    def test_every_tile_of_every_round(self):
        rep = econ.run(ECON, limit_games=2)
        self.assertGreater(rep["rounds"], 0)
        self.assertEqual(rep["tiles_ok"], rep["tiles"], rep["examples"])


@unittest.skipUnless(WHOLE, "TACSIM_WHOLE not set")
class WholeMapTest(unittest.TestCase):
    def test_play_and_paths(self):
        rep = parity.run(WHOLE, mode="free", limit=6, show=3)
        self.assertEqual(rep["scenes_ok"], rep["scenes"], rep["examples"])
        self.assertGreater(rep["paths"]["checked"], 0)
        self.assertEqual(rep["paths"]["ok"], rep["paths"]["checked"], rep["paths"]["examples"])


if __name__ == "__main__":
    unittest.main()
