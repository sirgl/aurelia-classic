"""The exact combat arithmetic of the MLX tactical simulator against the Rust formulas (python ints)."""
import random
import unittest

import mlx.core as mx

from aurelia_nn.tacsim import exact


def ratio_py(dmg, me, opp):
    me, opp, dmg = max(me, 1), max(opp, 1), max(dmg, 0)
    if opp > me:
        a = (4 * me) ** 4
        num, den = 2 * a, (opp + 3 * me) ** 4 + a
    else:
        a = (4 * opp) ** 4
        num, den = (me + 3 * opp) ** 4 + a, 2 * a
    return dmg * num // den


def xorshift_py(s):
    m = (1 << 64) - 1
    s ^= (s << 13) & m
    s ^= s >> 7
    s ^= (s << 17) & m
    return max(s, 1)


class ExactTest(unittest.TestCase):
    def test_ratio_matches_python_ints(self):
        rnd = random.Random(7)
        rows = [(rnd.randrange(0, 4800), rnd.randrange(100, 60000), rnd.randrange(100, 60000)) for _ in range(20000)]
        rows += [(3599, 45000, 100), (3599, 100, 45000), (2400, 1400, 1595), (0, 5, 5)]
        d, m, o = (mx.array([r[i] for r in rows], dtype=mx.int64) for i in range(3))
        got = exact.scale_by_ratio(d, m, o).tolist()
        want = [ratio_py(*r) for r in rows]
        lim = exact.EXACT_LIMIT - 4
        self.assertEqual([g for g, w in zip(got, want) if w <= lim], [w for w in want if w <= lim])
        self.assertTrue(all(g > lim for g, w in zip(got, want) if w > lim))
        self.assertGreater(sum(w <= lim for w in want), 15000)

    def test_damage_table_of_the_rules(self):
        # Rust damage.rs `damage_table_of_the_rules`: R, min, max at full hp.
        rows = [(1, 3, 8, 12), (1, 2, 14, 21), (2, 3, 18, 28), (1, 1, 24, 36), (5, 4, 27, 41), (3, 2, 31, 47), (2, 1, 41, 62), (3, 1, 73, 109), (4, 1, 125, 187)]
        for num, den, lo, hi in rows:
            me, opp = mx.array([num * 1200]), mx.array([den * 1200])
            z = mx.array([0])
            self.assertEqual(exact.unit_damage(me, opp, z, z, mx.array([0])).item(), lo)
            self.assertEqual(exact.unit_damage(me, opp, z, z, mx.array([1199])).item(), hi)
        # city strike 1482 on 800: 38..57
        self.assertEqual(exact.city_damage(mx.array([1482]), mx.array([800]), mx.array([0])).item(), 38)
        self.assertEqual(exact.city_damage(mx.array([1482]), mx.array([800]), mx.array([1199])).item(), 57)
        self.assertEqual(exact.wounded_factor(mx.array([60]), mx.array([0])).item(), 81)

    def test_mutual_kill(self):
        f = lambda a, b, c, d: tuple(x.item() for x in exact.resolve_mutual_kill(*(mx.array([v]) for v in (a, b, c, d))))
        self.assertEqual(f(60, 40, 50, 30), (39, 50))
        self.assertEqual(f(90, 40, 50, 30), (90, 29))
        self.assertEqual(f(30, 40, 50, 30), (30, 50))

    def test_rng_stream(self):
        seeds = [0x9E3779B97F4A7C15 + 5, 12345, (1 << 64) - 1]
        st = exact.u64_from_str(seeds)
        py = list(seeds)
        for _ in range(50):
            st, v = exact.draw(st, mx.array([1200, 100, 7]), mx.array([True, True, True]))
            py = [xorshift_py(s) for s in py]
            self.assertEqual(exact.u64_to_ints(st), py)
            self.assertEqual(v.tolist(), [py[0] % 1200, py[1] % 100, py[2] % 7])


if __name__ == "__main__":
    unittest.main()
