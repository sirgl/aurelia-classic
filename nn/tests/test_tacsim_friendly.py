""""Friendly land" is one rule in the twin (`sim.friendly_land`, Rust `combat::terrain::is_friendly_territory`): the land of
the unit's side or of a host that grants it entry. At f90213f every strength and the heal rate tested `owner == side`, so
our units on an ally's open land fought and healed as outside friendly land (defend-friend era 5: the Foreign Legion's
+20 % outside friendly land in the twin, not in Rust)."""
import re
from pathlib import Path

import mlx.core as mx

from aurelia_nn.tacsim import sim as S

TACSIM = Path(S.__file__).parent


def test_friendly_land_is_own_land_or_a_granting_host():
    mx.set_default_device(mx.cpu)
    owner = mx.array([[0, 1, 2, -1]], dtype=mx.int32)
    grant = mx.zeros((1, 3, 3), dtype=mx.bool_)
    grant = mx.array([[[False, False, False], [False, False, False], [True, False, False]]])  # the third (host 2) lets side 0 in
    s = {"owner": owner, "grant": grant}
    tiles = mx.array([[0, 1, 2, 3]], dtype=mx.int32)
    assert S.friendly_land(s, mx.array([[0]], dtype=mx.int32), tiles).tolist() == [[True, False, True, False]]
    assert S.friendly_land(s, mx.array([[1]], dtype=mx.int32), tiles).tolist() == [[False, True, False, False]]
    assert S.friendly_land({"owner": owner}, mx.array([[0]], dtype=mx.int32), tiles).tolist() == [[True, False, False, False]]
    barb = {"owner": owner, "barb": mx.array([True])}
    assert S.friendly_land(barb, mx.array([[1]], dtype=mx.int32), tiles).tolist() == [[False, False, False, False]]


def test_no_module_tests_friendly_land_by_hand():
    bad = []
    for p in sorted(TACSIM.glob("*.py")):
        for i, line in enumerate(p.read_text().splitlines(), 1):
            if re.search(r"\bfriendly\s*=\s*(?!friendly_land\()", line) and "friendly_land" not in line:
                bad.append(f"{p.name}:{i}: {line.strip()}")
    assert not bad, bad
