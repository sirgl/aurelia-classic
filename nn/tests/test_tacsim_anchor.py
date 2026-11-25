"""The RL anchor and the scene rule version: guard and convoy (crates/qa/src/trainers/rule_dependent.json) are drawn only from a
record made under the current rule version; the rest from the main record. Run without pytest by `logs/l8-runtests.py`.

The records are built here from the plays of the real files (the first plays of `data/p7-teach-train.bin`, which has guard,
convoy, snipe; the first plays of `data/teach-r-train.bin`, which has duel), with the meta rewritten to carry a rule version."""
import json
import random
import struct
import tempfile
from pathlib import Path

import mlx.core as mx

from aurelia_nn.army import train_m as TM
from aurelia_nn.army.data import map_file, skip_play_at
from aurelia_nn.army.model_mb import MBNet
from aurelia_nn.tacsim import armyrl

DATA = Path(armyrl.__file__).resolve().parents[4] / "data"
P7 = DATA / "p7-teach-train.bin"
R = DATA / "teach-r-train.bin"


def _plays(path, per_family, only):
    """{family: [bytes of its first `per_family` plays]} for the families in `only`, scanned from the head of a record."""
    buf = map_file(str(path))
    out, at = {f: [] for f in only}, 0
    while at < len(buf) and any(len(v) < per_family for v in out.values()):
        meta, after, *_ = skip_play_at(buf, at)
        got = out.get(meta["family"])
        if got is not None and len(got) < per_family:
            got.append(bytes(buf[at:after]))
        at = after
    return out


def _with_meta(play, **upd):
    magic, jl = struct.unpack_from("<II", play, 0)
    meta = json.loads(play[8:8 + jl])
    meta.update(upd)
    j = json.dumps(meta).encode()
    return struct.pack("<II", magic, len(j)) + j + play[8 + jl:]


def _write(tmp, name, plays):
    p = Path(tmp) / name
    p.write_bytes(b"".join(plays))
    return str(p)


def _anchor(path, **kw):
    mx.set_default_device(mx.cpu)
    return armyrl.Anchor(path, 4, 7, MBNet(), **kw)


def _sources(tmp, main_version=None):
    """main: duel + guard + convoy plays (marked `main_version`, or unmarked); v2: guard + convoy + snipe, unmarked (as p7's)."""
    duel = _plays(R, 4, only=["duel"])["duel"]
    p7 = _plays(P7, 4, only=["guard", "convoy", "snipe"])
    mark = {} if main_version is None else {"rule_version": main_version}
    dep = [_with_meta(x, **mark) for f in ("guard", "convoy") for x in p7[f]]
    main = _write(tmp, "main.bin", duel + dep)
    v2 = _write(tmp, "v2.bin", [x for f in ("guard", "convoy", "snipe") for x in p7[f]])
    return main, v2, p7


def _refused(fn):
    try:
        fn()
    except SystemExit as e:
        return str(e)
    return None


def test_the_rule_dependent_families_are_the_listed_ones():
    assert armyrl.rule_dependent_families() == {"guard", "convoy"}


def test_version_one_rows_of_guard_and_convoy_are_refused():
    with tempfile.TemporaryDirectory() as tmp:
        for mark in (None, 1):   # no field counts as version 1
            main, v2, _ = _sources(tmp, main_version=mark)
            msg = _refused(lambda: _anchor(main))
            assert msg and "guard" in msg and "convoy" in msg and "v1" in msg, msg
        # main's unmarked guard / convoy (version 1) are dropped when a v2 source is given; v2's unmarked plays count as version 1 unless named
        msg = _refused(lambda: _anchor(main, v2_path=v2, v2_unmarked_version=2))
        assert msg is None, "the main source's guard / convoy are dropped when a v2 source is given"
        main_ok = _write(tmp, "duel.bin", _plays(R, 4, only=["duel"])["duel"])
        msg = _refused(lambda: _anchor(main_ok, v2_path=v2))
        assert msg and "v1" in msg, msg
        assert _refused(lambda: _anchor(main_ok, v2_path=v2, v2_unmarked_version=2)) is None
        # a v2 play marked with another version is refused whatever the flag says
        old = _write(tmp, "old.bin", [_with_meta(x, rule_version=1) for x in _plays(P7, 2, only=["guard"])["guard"]])
        assert _refused(lambda: _anchor(main_ok, v2_path=old, v2_unmarked_version=2))
        # the legacy version is taken only when named, and only without a v2 source
        main, v2, _ = _sources(tmp, main_version=None)
        a = _anchor(main, legacy_version=1)
        assert {m["family"] for m in a.stream.metas} == {"duel", "guard", "convoy"}
        # excluded families are not checked
        assert _refused(lambda: _anchor(main, exclude=("guard", "convoy"))) is None


def test_guard_and_convoy_come_from_the_v2_source_and_the_rest_from_the_main_source():
    with tempfile.TemporaryDirectory() as tmp:
        main, v2, p7 = _sources(tmp, main_version=1)
        a = _anchor(main, v2_path=v2, v2_unmarked_version=2)
        by = {}
        for i in a.pool:
            k, _ = a.stream.where[i]
            by.setdefault(a.stream.metas[i]["family"], set()).add(k)
        assert by == {"duel": {0}, "guard": {1}, "convoy": {1}}, by   # snipe of v2 is not taken: not rule-dependent
        assert len(a.pool) == 4 + 4 + 4
        # the same plays as the v2 file's own: the draw reads them
        ids = a.draw()
        assert len(ids) == 4 and all(i in a.pool for i in ids)


def test_a_single_source_without_dependent_families_draws_as_before():
    """The old call, `Anchor(path, plays, seed, model, exclude)`, on a record without guard / convoy: the same stream, pool,
    best-teacher table, shuffles and plans as the construction before the change (rebuilt here from `train_m` directly)."""
    from aurelia_nn.army.train import best_teachers
    with tempfile.TemporaryDirectory() as tmp:
        main = _write(tmp, "duel.bin", _plays(R, 12, only=["duel"])["duel"])
        for exclude in ((), ("snipe",)):
            a = _anchor(main, exclude=exclude)
            s = TM.Stream([main], 1, {}, None)
            assert a.stream.where == s.where and a.stream.metas == s.metas and a.tm == s.t_max
            assert a.best == best_teachers(s.metas)[0]
            pool = [i for i, m in enumerate(s.metas) if m["family"] not in set(exclude)]
            assert a.pool == pool
            rng = random.Random(7 * 7919 + 1)
            order = []
            for _ in range(5):
                if len(order) < 4:
                    order = list(pool)
                    rng.shuffle(order)
                want, order = order[:4], order[4:]
                got = a.draw()
                assert got == want


def test_plays_of_two_observation_versions_are_not_one_anchor():
    """The net's input changed (obs_version 2: only what its side knows). An anchor holds tokens of ONE observation version:
    a record without the field counts as version 1 (the full state), two versions together are refused, and a run that
    names the version it expects refuses plays of another."""
    with tempfile.TemporaryDirectory() as tmp:
        duel = _plays(R, 4, only=["duel"])["duel"]
        v1 = _write(tmp, "v1.bin", duel)
        v2 = _write(tmp, "v2.bin", [_with_meta(x, obs_version=2) for x in duel])
        mixed = _write(tmp, "mixed.bin", duel[:2] + [_with_meta(x, obs_version=2) for x in duel[2:]])
        assert _refused(lambda: _anchor(v1)) is None and _anchor(v1).obs_version == 1
        assert _refused(lambda: _anchor(v2)) is None and _anchor(v2).obs_version == 2
        msg = _refused(lambda: _anchor(mixed))
        assert msg and "different observation versions" in msg, msg
        assert _refused(lambda: _anchor(v2, obs_version=2)) is None
        msg = _refused(lambda: _anchor(v1, obs_version=2))
        assert msg and "observation version 1" in msg, msg
