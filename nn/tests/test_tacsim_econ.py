"""Parity of the MLX economy with the Rust rules: the yields of every tile of every round of recorded
whole games, and the multi-turn path costs of a whole-map scene dump.

    TACSIM_ECON=/path/econ.jsonl TACSIM_WHOLE=/path/whole.jsonl TACSIM_TURNS=/path/turns.jsonl python -m unittest tests.test_tacsim_econ

Skipped without the dumps (they need the Rust build: `AURELIA_ECON_DUMP=FILE ai-tournament ...`,
`ai-exam suite-dump --whole-map --paths N`; the end-turn lines: `AURELIA_ECON_TURNS=1` too).
"""
import os
import unittest

from aurelia_nn.tacsim import city, culture, econ, great, gpu, happy, ledger, parity, player, treasury, turn

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


@unittest.skipUnless(TURNS, "TACSIM_TURNS not set")
class EmpireTest(unittest.TestCase):
    """The empire's subsystems on the first game of an `AURELIA_ECON_TURNS` dump (part 4 fields)."""

    def test_player_stats(self):
        n = ok = 0
        for g, game in city.each_game(TURNS, 1):
            model = city.Model(game["tables"], game["rules"])
            prow = player.Rows(model.rows, model.er)
            for t in player.prepare(game, game["turns"]):
                mine, _ = player.stats_and_costs(prow, t)
                rust = player.rust_stats(t["pre"]["state"]["rust"]["stats"])
                n += 1
                ok += int({k: v for k, v in mine.items() if v} == {k: v for k, v in rust.items() if v})
        self.assertGreater(n, 0)
        self.assertEqual(ok, n)

    def test_resource_ledger(self):
        rep = ledger.run(TURNS, limit_games=1)
        self.assertGreater(rep["ledgers"], 0)
        self.assertEqual(rep["ledgers_ok"], rep["ledgers"], rep["examples"])
        self.assertEqual(rep["orders_ok"], rep["orders"], rep["examples"])

    def test_upkeep_and_connections(self):
        rep = treasury.run(TURNS, limit_games=1)
        self.assertGreater(rep["worlds"], 0)
        self.assertEqual(rep["ok"], rep["worlds"], rep["examples"])
        self.assertEqual(rep.get("reached_ok"), rep.get("reached"))

    def test_policies(self):
        rep = culture.run(TURNS, limit_games=1)
        self.assertEqual(rep["prices_ok"], rep["prices"], rep["examples"])
        self.assertEqual(rep["adoptions_ok"], rep["adoptions"], rep["examples"])

    def test_great_people(self):
        rep = great.run(TURNS, limit_games=1)
        self.assertEqual(rep["points_ok"], rep["cities"], rep["examples"])
        self.assertEqual(rep["thresholds_ok"], rep["thresholds"])


@unittest.skipUnless(TURNS, "TACSIM_TURNS not set")
class DeviceStepTest(unittest.TestCase):
    """The state resident on the device: one step from every dumped world of the first game."""

    def test_device_step(self):
        rep = gpu.run(TURNS, limit_games=1)
        self.assertGreater(rep["cities"], 0)
        # The parallel resolution within one empire leaves a handful of in-order cases (`gpu.py`).
        self.assertGreaterEqual(rep["cities_ok"], rep["cities"] * 0.999, rep["examples"])
        self.assertGreaterEqual(rep["worlds_ok"], rep["worlds"] * 0.99, rep["examples"])


if __name__ == "__main__":
    unittest.main()
