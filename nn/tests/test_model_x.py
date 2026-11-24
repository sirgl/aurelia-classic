"""MBXNet (army/model_x.py): the arch flags, the loader, the Muon grouping. Plain unittest (pytest is absent from the
MLX venv): `python -m unittest tests.test_model_x` from nn/ (or `python -m pytest` where pytest exists)."""
import itertools
import json
import os
import tempfile
import unittest

import mlx.core as mx
import mlx.nn as nn
import mlx.optimizers as optim

from aurelia_nn.army.data import F_ACT, F_TOK
from aurelia_nn.army.model_m import PLANS
from aurelia_nn.army.model_mb import MBNet
from aurelia_nn.army.model_x import FLAGS, MBXNet, parse_arch
from aurelia_nn.army.train_m import MUON_MATRICES, muon_optimizer
from aurelia_nn.tacsim.armyloop import load_net

SEED_NETS = os.path.join(os.path.dirname(__file__), '..', '..', 'lab-seed', 'nets')
B, T, C, CH = 3, 9, 6, 64


def inputs(d, seed=0):
    mx.random.seed(seed)
    tmask = mx.array([[True] * 9, [True] * 6 + [False] * 3, [True] * 4 + [False] * 5])
    return dict(
        tok=mx.random.normal((B, T, F_TOK)), dist=mx.random.randint(0, 16, (B, T, T)), tmask=tmask,
        slot=mx.random.randint(-1, 16, (B, T)), um=mx.random.normal((B, 16, 16)) * 0.3, pm=mx.random.normal((B, PLANS, d)) * 0.3,
        gtok=mx.random.normal((B, T, CH)), tok_ok=mx.random.uniform(shape=(B, T)) > 0.3, mmean=mx.random.normal((B, CH)))


def action_inputs(d):
    return dict(unit=mx.array([0, 1, 2]), cand=mx.random.normal((B, C, F_ACT)), promo=mx.random.randint(0, 50, (B, C)),
                cmask=mx.random.uniform(shape=(B, C)) > 0.2, gcand=mx.random.normal((B, C, CH)), cand_ok=mx.random.uniform(shape=(B, C)) > 0.3)


def forward(net, d):
    x = inputs(d)
    h = net.body_mbg(**x)
    logits = net.action_logits_g(h, **action_inputs(d))
    return h, logits


def maxdiff(a, b):
    return float(mx.max(mx.abs(a - b)))


def finite(x):
    return bool(mx.all(mx.isfinite(x)))


class ParityTests(unittest.TestCase):
    def test_base_equals_mbnet(self):
        mx.random.seed(1)
        ref = MBNet(map_ch=CH)
        mx.eval(ref.parameters())
        net = MBXNet(map_ch=CH, arch={})
        net.load_weights(list(dict(nn.utils.tree_flatten(ref.parameters())).items()))
        h0, l0 = forward(ref, 64)
        h1, l1 = forward(net, 64)
        dh, dl = maxdiff(h0, h1), maxdiff(mx.where(l0 > -1e8, l0, 0), mx.where(l1 > -1e8, l1, 0))
        print(f'base vs MBNet: body {dh:.2e}, logits {dl:.2e}')
        self.assertLess(dh, 1e-5)
        self.assertLess(dl, 1e-5)

    def test_base_same_seed_same_init(self):
        mx.random.seed(5)
        a = MBNet(map_ch=CH)
        mx.random.seed(5)
        b = MBXNet(map_ch=CH, arch=parse_arch('base'))
        fa, fb = dict(nn.utils.tree_flatten(a.parameters())), dict(nn.utils.tree_flatten(b.parameters()))
        self.assertEqual(sorted(fa), sorted(fb))
        self.assertTrue(all(maxdiff(fa[k], fb[k]) == 0 for k in fa))

    def test_loop2_at_init_equals_loop1(self):
        for flags in ({}, {'swiglu': True, 'rmsnorm': True, 'qknorm': True, 'gate_attn': True}):
            mx.random.seed(2)
            one = MBXNet(map_ch=CH, arch={**flags, 'loop': 1})
            two = MBXNet(map_ch=CH, arch={**flags, 'loop': 2})
            two.load_weights(list(dict(nn.utils.tree_flatten(one.parameters())).items()), strict=False)
            h1, l1 = forward(one, 64)
            h2, l2 = forward(two, 64)
            d = maxdiff(h1, h2)
            print(f'loop=2 vs loop=1 at init {sorted(flags)}: {d:.2e}')
            self.assertLess(d, 1e-5)
            self.assertLess(maxdiff(mx.where(l1 > -1e8, l1, 0), mx.where(l2 > -1e8, l2, 0)), 1e-5)

    def test_loop_after_a_gate_opens_differs(self):
        net = MBXNet(map_ch=CH, arch={'loop': 2})
        h0, _ = forward(net, 64)
        net.pass_gate = mx.full(net.pass_gate.shape, 0.5)
        h1, _ = forward(net, 64)
        self.assertGreater(maxdiff(h0, h1), 1e-3)


class FlagTests(unittest.TestCase):
    def test_every_combination_forward_and_grad(self):
        n = 0
        for d, layers in ((64, 3), (64, 6)):
            for bits in itertools.product((False, True), repeat=len(FLAGS)):
                for loop in (1, 2):
                    arch = {**dict(zip(FLAGS, bits)), 'loop': loop}
                    mx.random.seed(3)
                    net = MBXNet(d=d, layers=layers, ff=2 * d, map_ch=CH, arch=arch)
                    act = action_inputs(d)
                    x = inputs(d)

                    def loss(net):
                        h = net.body_mbg(**x)
                        lg = net.action_logits_g(h, **act)
                        return mx.sum(mx.where(act['cmask'], lg, 0.0) ** 2) * 1e-3 + mx.sum(h ** 2) * 1e-3

                    val, grads = nn.value_and_grad(net, loss)(net)
                    mx.eval(val, grads)
                    self.assertTrue(finite(val), arch)
                    self.assertTrue(all(finite(g) for _, g in nn.utils.tree_flatten(grads)), arch)
                    n += 1
        print(f'{n} flag combinations ran forward + grad, finite')

    def test_swiglu_keeps_the_parameter_count(self):
        relu = sum(v.size for _, v in nn.utils.tree_flatten(MBXNet(map_ch=CH).parameters()))
        glu = sum(v.size for _, v in nn.utils.tree_flatten(MBXNet(map_ch=CH, arch={'swiglu': True}).parameters()))
        print(f'params relu {relu}, swiglu {glu}')
        self.assertLess(abs(glu - relu) / relu, 0.01)

    def test_parse(self):
        self.assertEqual(parse_arch('swiglu,loop=3')['loop'], 3)
        self.assertEqual(parse_arch('base'), parse_arch(''))
        with self.assertRaises(AssertionError):
            parse_arch('nope')


class LoaderTests(unittest.TestCase):
    def test_round_trip_with_flags(self):
        for layers, arch in ((3, 'swiglu,rmsnorm,qknorm,gate_attn,loop=2'), (6, 'swiglu'), (3, 'base'), (3, 'rmsnorm')):
            mx.random.seed(4)
            net = MBXNet(layers=layers, map_ch=CH, arch=parse_arch(arch))
            if 'loop' in arch:  # open the gates, so the loop is felt
                net.pass_gate = mx.full(net.pass_gate.shape, 0.3)
                net.pass_emb = mx.random.normal(net.pass_emb.shape) * 0.1
            with tempfile.TemporaryDirectory() as tmp:
                path = os.path.join(tmp, 'weights.safetensors')
                net.save_weights(path)
                with open(os.path.join(tmp, 'arch.json'), 'w') as f:
                    json.dump(net.config['arch'], f)
                loaded = load_net(path)
            self.assertIsInstance(loaded, MBXNet)
            self.assertEqual(loaded.config['arch'], net.config['arch'])
            h0, l0 = forward(net, 64)
            h1, l1 = forward(loaded, 64)
            d = max(maxdiff(h0, h1), maxdiff(mx.where(l0 > -1e8, l0, 0), mx.where(l1 > -1e8, l1, 0)))
            print(f'load_net round trip {arch} L{layers}: {d:.2e}')
            self.assertLess(d, 1e-5)

    def test_old_nets_load_as_before(self):
        for name in ('MBf6c', 'rl2-u175'):
            path = os.path.join(SEED_NETS, name + '.safetensors')
            if not os.path.exists(path):
                self.skipTest(path)
            net = load_net(path)
            print(f'{name}: {type(net).__name__}')
            self.assertIs(type(net), MBNet)
            self.assertEqual((net.config['d'], net.config['layers'], net.config['heads']), (64, 3, 4))


class MuonTests(unittest.TestCase):
    def test_multi_optimizer_step(self):
        mx.random.seed(6)
        arch = parse_arch('rmsnorm,qknorm,swiglu,gate_attn,loop=2')
        net = MBXNet(map_ch=CH, arch=arch)
        mx.eval(net.parameters())
        before = dict(nn.utils.tree_flatten(net.parameters()))
        args = type('A', (), {'muon_lr': 0.02})
        opt = muon_optimizer(args, 100, optim.AdamW(learning_rate=1e-3, weight_decay=1e-4))
        act = action_inputs(64)
        x = inputs(64)

        def loss(net):
            h = net.body_mbg(**x)
            return mx.sum(net.action_logits_g(h, **act) * act['cmask']) * 1e-2 + mx.sum(h ** 2) * 1e-3

        _, grads = nn.value_and_grad(net, loss)(net)
        opt.init(net.trainable_parameters())
        opt.update(net, grads)
        mx.eval(net.parameters(), opt.state)
        after = dict(nn.utils.tree_flatten(net.parameters()))
        muon_keys = [k for k in before if MUON_MATRICES.match(k)]
        self.assertEqual(len(muon_keys), 3 * 8)
        for k in muon_keys + ['e1.weight', 'um.weight', 'layers.0.norm1.weight', 'm_in.weight']:
            self.assertGreater(maxdiff(before[k], after[k]), 0, k)
        self.assertTrue(all(finite(v) for v in after.values()))
        # the state survives a flatten / unflatten (the resume path)
        state = nn.utils.tree_unflatten(list(dict(nn.utils.tree_flatten(opt.state)).items()))
        opt.state = state


if __name__ == '__main__':
    unittest.main()
