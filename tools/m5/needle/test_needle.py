"""Tests for needle.py: python test_needle.py  (fast); NEEDLE_SLOW=1 python test_needle.py also runs the learning test."""
import os
import sys
import unittest

import mlx.core as mx

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import needle as nd  # noqa: E402


class Generator(unittest.TestCase):
    def check(self, n, k, b=64):
        ent, att, val, tgt, hard = nd.make_batch(mx.random.key(5), b, n, k)
        ent, att, val, tgt, hard = ent.tolist(), att.tolist(), val.tolist(), tgt.tolist(), hard.tolist()
        c = n - k
        n_hard = n_dis = 0
        for s in range(b):
            qs = [(ent[s][c + i], att[s][c + i]) for i in range(k)]
            self.assertEqual(len(set(qs)), k)
            self.assertEqual(len({q[0] for q in qs}), k)
            self.assertTrue(all(val[s][c + i] == nd.QUERY for i in range(k)))
            for i, (qe, qa) in enumerate(qs):
                hits = [p for p in range(c) if (ent[s][p], att[s][p]) == (qe, qa)]
                self.assertEqual(len(hits), 1)
                self.assertEqual(val[s][hits[0]], tgt[s][i])
                self.assertTrue(0 <= tgt[s][i] < nd.N_VAL)
            for p in range(c):
                f = (ent[s][p], att[s][p])
                if f in qs:
                    continue
                n_dis += 1
                shares = [(f[0] == qe) != (f[1] == qa) for qe, qa in qs]  # exactly one field
                if hard[s][p]:
                    n_hard += 1
                    self.assertTrue(any(shares))
                self.assertFalse(any(f[0] == qe and f[1] == qa for qe, qa in qs))
        self.assertGreaterEqual(n_hard / n_dis, 0.3)
        self.assertEqual(max(max(r) for r in ent) < nd.N_ENT and max(max(r) for r in att) < nd.N_ATT, True)

    def test_invariants(self):
        for n in (64, 128, 256):
            for k in (1, 3, 5):
                self.check(n, k, 32)

    def test_heldout_deterministic(self):
        a = nd.heldout(1, 64, 3, 64)
        b = nd.heldout(1, 64, 3, 64)
        c = nd.heldout(2, 64, 3, 64)
        self.assertTrue(all(bool((x == y).all().item()) for x, y in zip(a, b)))
        self.assertFalse(bool((a[0] == c[0]).all().item()))

    def test_model_shape(self):
        m = nd.Needle(64, 3, 64, 5)
        ent, att, val, tgt = nd.heldout(1, 64, 5, 8)
        self.assertEqual(tuple(m(ent, att, val).shape), (8, 5, nd.N_VAL))


@unittest.skipUnless(os.environ.get('NEEDLE_SLOW'), 'slow: NEEDLE_SLOW=1')
class Slow(unittest.TestCase):
    def test_learns_above_chance(self):
        held = nd.heldout(1, 64, 1, 512)
        r = nd.train_net(64, 1, 64, 3, 400, 200, 1, held)
        self.assertGreater(r['best_acc'], 3 / 32)


if __name__ == '__main__':
    unittest.main()
