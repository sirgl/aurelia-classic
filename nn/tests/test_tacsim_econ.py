"""Parity of the MLX economy with the Rust rules: the yields of every tile of every round of recorded
whole games, and the multi-turn path costs of a whole-map scene dump.

    TACSIM_ECON=/path/econ.jsonl TACSIM_WHOLE=/path/whole.jsonl TACSIM_TURNS=/path/turns.jsonl python -m unittest tests.test_tacsim_econ

Skipped without the dumps (they need the Rust build: `AURELIA_ECON_DUMP=FILE ai-tournament ...`,
`ai-exam suite-dump --whole-map --paths N`; the end-turn lines: `AURELIA_ECON_TURNS=1` too).
"""
import os
import unittest

from aurelia_nn.tacsim import city, econ, happy, parity, turn

ECON = os.environ.get("TACSIM_ECON")
WHOLE = os.environ.get("TACSIM_WHOLE")
TURNS = os.environ.get("TACSIM_TURNS")


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


@unittest.skipUnless(TURNS, "TACSIM_TURNS not set")
class CityEconomyTest(unittest.TestCase):
    """Every city of every end of turn of the first game of an `AURELIA_ECON_TURNS` dump."""

    def test_city_yields(self):
        rep = city.run(TURNS, limit_games=1)
        self.assertGreater(rep["cities"], 0)
        self.assertEqual(rep["cities_ok"], rep["cities"], rep["examples"])

    def test_end_of_turn(self):
        rep = turn.run(TURNS, limit_games=1)
        self.assertGreater(rep["cities"], 0)
        self.assertEqual(rep["cities_ok"], rep["cities"], rep["examples"])
        self.assertEqual(rep["players_ok"], rep["players"], rep["examples"])

    def test_happiness_and_golden_age(self):
        rep = happy.run(TURNS, limit_games=1)
        self.assertGreater(rep["turns"], 0)
        self.assertEqual(rep["report_ok"], rep["turns"], rep["examples"])
        self.assertEqual(rep["ga_ok"], rep["ga_turns"], rep["examples"])


if __name__ == "__main__":
    unittest.main()
