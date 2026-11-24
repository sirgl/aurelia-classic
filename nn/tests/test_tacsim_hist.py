"""The history's strike facts read the target's hit points from any unit on the tile that is not the learner's (Rust
`observe::hp_at`: owner != side). At 9c80e36 the twin read only side 1, so after neutral-joins-war's declaration our
strikes on the third seat's units were facts with dealt 0 (1 132 moments of line 5)."""
import types

import mlx.core as mx

from aurelia_nn.tacsim import armyhist as H


def test_hp_at_reads_a_third_seat_unit():
    mx.set_default_device(mx.cpu)
    # One row, tiles 0..3: our unit on tile 0 (slot 0), the enemy's on 1 (slot 1), the third seat's on 2 (slot 2), 3 empty.
    s = {"u_side": mx.array([[0, 1, 2]], dtype=mx.int32), "u_hp": mx.array([[90, 70, 40]], dtype=mx.int32),
         "c_dmg": mx.zeros((1, 1), dtype=mx.int32)}
    mil = mx.array([[0, 1, 2, -1]], dtype=mx.int32)
    civ = mx.full((1, 4), -1, dtype=mx.int32)
    sim = types.SimpleNamespace(city_on=lambda s_, t: mx.full(t.shape, -1, dtype=mx.int32), occupancy=lambda s_: (mil, civ),
                                cs={"alive": mx.array([[False]])}, city_maxhp=lambda s_, c: mx.full(c.shape, 200, dtype=mx.int32))
    game = types.SimpleNamespace(sim=sim)
    hp, is_city, there = H.hp_at(game, s, mx.array([2], dtype=mx.int32))
    assert hp.tolist() == [40] and there.tolist() == [True]
    hp, _, there = H.hp_at(game, s, mx.array([1], dtype=mx.int32))
    assert hp.tolist() == [70] and there.tolist() == [True]
    hp, _, there = H.hp_at(game, s, mx.array([0], dtype=mx.int32))
    assert hp.tolist() == [0] and there.tolist() == [False]
