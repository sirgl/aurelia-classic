"""The compiled step loop of the throughput bench plays exactly like the eager one (TACSIM_DUMP needed)."""
import os
import unittest

import mlx.core as mx

from aurelia_nn.tacsim import bench

DUMP = os.environ.get("TACSIM_DUMP")


@unittest.skipUnless(DUMP, "TACSIM_DUMP not set")
class CompiledLoopTest(unittest.TestCase):
    def test_compiled_equals_eager(self):
        finals = []
        for compiled in (True, False):
            sim, s0, tb, _ = bench.replicate(DUMP, 16, limit=16)
            runner = bench.Runner(sim, "random", None, compile_=compiled)
            s, *_ = bench.play(runner, dict(s0), 2, mode="seq", seed=5)
            finals.append({k: v.tolist() for k, v in s.items() if k.startswith(("u_", "c_")) or k == "rng"})
        self.assertEqual(finals[0], finals[1])

    def test_masks_respected(self):
        # Every action the random policy takes is legal: no unit ends on a foreign unit's tile, hp stays in range.
        sim, s0, tb, _ = bench.replicate(DUMP, 32, limit=32)
        runner = bench.Runner(sim, "random", None)
        s, *_ = bench.play(runner, dict(s0), 3, mode="seq", seed=3)
        alive = s["u_alive"].tolist()
        tiles = s["u_tile"].tolist()
        civ = s["p_civilian"].tolist()
        side = s["u_side"].tolist()
        for b in range(32):
            seen = {}
            for i, a in enumerate(alive[b]):
                if a:
                    key = (tiles[b][i], civ[b][i])
                    self.assertNotIn(key, seen, f"two units of a class on one tile, scene {b}")
                    seen[key] = side[b][i]
            for (t, c), sd in seen.items():
                self.assertNotEqual(seen.get((t, not c), sd), 1 - sd, f"enemies share a tile, scene {b}")
        hp = s["u_hp"].tolist()
        self.assertTrue(all(0 <= h <= 100 for row in hp for h in row))


if __name__ == "__main__":
    unittest.main()
