"""Run: python3 -m unittest -v   (from this directory)"""
import json
import math
import os
import tempfile
import unittest

import gate
import rows as R
import stats as S


class TestStats(unittest.TestCase):
    def test_ambush_fog_not_a_drop(self):
        # 156/160 vs 160/160: four discordant plays, all against B.
        mc = S.mcnemar_exact(4, 0)
        self.assertAlmostEqual(mc['p_worse'], 0.0625, places=6)
        self.assertAlmostEqual(mc['p_two'], 0.125, places=6)
        fi = S.fisher_exact(160, 160, 156, 160)
        self.assertGreater(fi['p_worse'], 0.05)
        self.assertAlmostEqual(fi['p_worse'], 0.0600, delta=0.002)

    def test_mcnemar_symmetric(self):
        a, b = S.mcnemar_exact(7, 2), S.mcnemar_exact(2, 7)
        self.assertAlmostEqual(a['p_worse'], b['p_better'])
        self.assertAlmostEqual(a['p_two'], b['p_two'])
        self.assertEqual(S.mcnemar_exact(5, 5)['p_two'], 1.0)
        self.assertEqual(S.mcnemar_exact(0, 0)['p_two'], 1.0)

    def test_mcnemar_known(self):
        # b=9, c=1: P(X>=9 | 10, .5) = 11/1024
        self.assertAlmostEqual(S.mcnemar_exact(9, 1)['p_worse'], 11 / 1024)
        self.assertAlmostEqual(S.mcnemar_exact(9, 1)['p_two'], 22 / 1024)

    def test_fisher_known(self):
        # lady tasting tea: 3/4 vs 1/4 -> one-sided 17/70... use 4 of 4 vs 0 of 4: p = 1/70
        f = S.fisher_exact(4, 4, 0, 4)
        self.assertAlmostEqual(f['p_worse'], 1 / 70)
        self.assertAlmostEqual(f['p_two'], 2 / 70)
        g = S.fisher_exact(0, 4, 4, 4)
        self.assertAlmostEqual(g['p_better'], 1 / 70)

    def test_fisher_symmetric(self):
        f, g = S.fisher_exact(30, 40, 20, 40), S.fisher_exact(20, 40, 30, 40)
        self.assertAlmostEqual(f['p_two'], g['p_two'])
        self.assertAlmostEqual(f['p_worse'], g['p_better'])

    def test_wilson(self):
        lo, hi = S.wilson(50, 100)
        self.assertAlmostEqual(lo, 0.4038, places=3)
        self.assertAlmostEqual(hi, 0.5962, places=3)
        lo, hi = S.wilson(160, 160)
        self.assertLess(lo, 1.0)
        self.assertEqual(hi, 1.0)
        self.assertEqual(S.wilson(0, 0), (0.0, 1.0))

    def test_newcombe_contains_delta_and_swaps(self):
        lo, hi = S.newcombe_paired(120, 4, 0, 36)
        self.assertLess(lo, -4 / 160)
        self.assertGreater(hi, -4 / 160)
        lo2, hi2 = S.newcombe_paired(120, 0, 4, 36)
        self.assertAlmostEqual(lo2, -hi)
        self.assertAlmostEqual(hi2, -lo)

    def test_holm_fixed(self):
        ps = [0.01, 0.04, 0.03, 0.005]
        # sorted 0.005 0.01 0.03 0.04 -> x4, x3, x2, x1 = .02, .03, .06, .04 -> monotone .02 .03 .06 .06
        self.assertEqual([round(x, 10) for x in S.holm(ps)], [0.03, 0.06, 0.06, 0.02])
        self.assertEqual(S.holm([0.5]), [0.5])
        self.assertEqual(S.holm([0.4, 0.9]), [0.8, 0.9])

    def test_bh_fixed(self):
        ps = [0.01, 0.04, 0.03, 0.005]
        # sorted .005 .01 .03 .04 -> .02 .02 .04 .04
        self.assertEqual([round(x, 10) for x in S.benjamini_hochberg(ps)], [0.02, 0.04, 0.04, 0.02])

    def test_power_monotone(self):
        a = S.mde_paired(140, 0.1, 0.05)
        b = S.mde_paired(140, 0.1, 0.05 / 40)
        c = S.mde_paired(400, 0.1, 0.05 / 40)
        self.assertLess(a, b)
        self.assertLess(c, b)
        self.assertIsNone(S.mde_paired(20, 0.05, 0.05 / 40))
        self.assertGreaterEqual(S.power_paired(140, 0.1, b, 0.05 / 40), 0.8 - 1e-6)


def _write_rows(path, family, wins, offset=0, player='army-net'):
    with open(path, 'a') as f:
        for i, w in enumerate(wins):
            f.write(json.dumps({'family': family, 'era': 'ancient', 'instance': 80 + i % 20, 'attempt': 0, 'player': player,
                                'win': bool(w), 'info': {}}) + '\n')


class TestGate(unittest.TestCase):
    def setUp(self):
        self.d = tempfile.mkdtemp()

    def _arm(self, name, fams):
        p = os.path.join(self.d, name + '.jsonl')
        for fam, wins in fams.items():
            _write_rows(p, fam, wins)
        return p

    def _kept(self, fams, n):
        p = os.path.join(self.d, 'kept.jsonl')
        with open(p, 'w') as f:
            for fam in fams:
                for i in range(20):
                    f.write(json.dumps({'family': fam, 'era': 'ancient', 'instance': 80 + i, 'attempt': 0}) + '\n')
        return p

    def test_160_is_20_instances_by_8_is_not_needed(self):
        # one play per unit: 20 units only; use 20-unit families here
        a = self._arm('a', {'fam': [1] * 20})
        b = self._arm('b', {'fam': [1] * 19 + [0]})
        inputs = {'slices': [{'name': 's', 'kept': self._kept(['fam'], 20), 'A': [a], 'B': [b]}]}
        rep = gate.run(inputs, {'min_n': 10, 'harm': {'tolerance_pts': 0}, 'bootstrap_reps': 50})
        s = rep['families']['s/fam']
        self.assertEqual((s['n'], s['b_worse'], s['b_better']), (20, 1, 0))
        self.assertFalse(s['drop'])
        self.assertTrue(rep['pass'])

    def test_clear_drop_detected_and_tolerance(self):
        n = 20
        a = self._arm('a', {'x': [1] * n, 'y': [1] * n})
        b = self._arm('b', {'x': [1] * 4 + [0] * 16, 'y': [1] * n})   # x: 100 % -> 20 %
        inputs = {'slices': [{'name': 's', 'kept': self._kept(['x', 'y'], n), 'A': [a], 'B': [b]}]}
        rep = gate.run(inputs, {'harm': {'tolerance_pts': 5}, 'bootstrap_reps': 20})
        self.assertTrue(rep['families']['s/x']['drop'])
        self.assertFalse(rep['pass'])
        rep = gate.run(inputs, {'harm': {'tolerance_pts': 90}, 'bootstrap_reps': 20})
        self.assertFalse(rep['families']['s/x']['drop'])   # a significant fall inside the tolerance
        self.assertEqual(rep['families']['s/x']['status'], 'down')

    def test_noninferiority_mode_and_deterministic(self):
        n = 20
        a = self._arm('a', {'x': [1] * n})
        b = self._arm('b', {'x': [1] * 19 + [0]})
        inputs = {'slices': [{'name': 's', 'kept': self._kept(['x'], n), 'A': [a], 'B': [b]}]}
        sp = {'harm': {'tolerance_pts': 5, 'mode': 'noninferiority'}, 'mean': {}, 'bootstrap_reps': 200}
        r1, r2 = gate.run(inputs, sp), gate.run(inputs, sp)
        self.assertFalse(r1['pass'])   # one lost play in 20: the CI reaches far below -5 points
        self.assertEqual(r1['summary']['mean'], r2['summary']['mean'])
        sp['harm']['mode'] = 'significance'
        self.assertTrue(gate.run(inputs, sp)['gates'][0]['pass'])

    def test_seeds_pooled_and_disagree(self):
        n = 20
        a1 = self._arm('a1', {'x': [1] * n})
        a2 = self._arm('a2', {'x': [1] * n})
        b1 = self._arm('b1', {'x': [1] * n})
        b2 = self._arm('b2', {'x': [0] * n})
        inputs = {'slices': [{'name': 's', 'kept': self._kept(['x'], n), 'A': [a1, a2], 'B': [b1, b2]}]}
        rep = gate.run(inputs, {'min_n': 10, 'harm': {'tolerance_pts': 5}, 'bootstrap_reps': 20})
        s = rep['families']['s/x']
        self.assertEqual(s['n'], 40)
        self.assertAlmostEqual(s['delta'], -0.5)
        self.assertTrue(s['seed_disagree'])
        self.assertEqual([round(x['delta'], 2) for x in s['seeds']], [0.0, -1.0])

    def test_loader_prey_split_and_kept(self):
        p = os.path.join(self.d, 'prey.jsonl')
        with open(p, 'w') as f:
            f.write(json.dumps({'family': 'prey', 'era': 'e', 'instance': 80, 'attempt': 1, 'player': 'army-net', 'win': True, 'info': {'context': 'land'}}) + '\n')
            f.write(json.dumps({'family': 'prey', 'era': 'e', 'instance': 81, 'attempt': 0, 'player': 'army-net', 'win': True, 'info': {}}) + '\n')
            f.write(json.dumps({'family': 'prey', 'era': 'e', 'instance': 80, 'attempt': 0, 'player': 'army-net', 'win': False, 'info': {}}) + '\n')
        k = os.path.join(self.d, 'k.jsonl')
        with open(k, 'w') as f:
            f.write(json.dumps({'family': 'prey', 'era': 'e', 'instance': 80, 'attempt': 1}) + '\n')
            f.write(json.dumps({'family': 'prey', 'era': 'e', 'instance': 81, 'attempt': 0}) + '\n')
        ld = R.Loader(k)
        ld.prescan([p])
        arm = ld.read_arm([p])
        self.assertEqual(set(arm), {'prey-land', 'prey-either'})
        self.assertEqual(len(arm['prey-land']), 1)


if __name__ == '__main__':
    unittest.main()
