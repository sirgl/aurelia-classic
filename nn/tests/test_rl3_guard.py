"""Guard of the rl3 record: a decision that picks a unit must carry that unit's army slot (the recorder reads the ids before the command runs)."""
from array import array
from types import SimpleNamespace

from aurelia_nn.rl3.learner import lost_slots


def flat_of(slots, ptr, off):
    return SimpleNamespace(slot=array("h", slots).tobytes(), ptr=ptr, tok_off=off)


def test_picked_unit_with_a_slot_passes():
    # two decisions of three tokens: picks token 1 (slot 4) and the end (-1)
    assert lost_slots(flat_of([0, 4, -1, 0, 4, -1], [1, -1], [0, 3])) == []


def test_picked_unit_without_a_slot_is_found():
    # the second decision picks token 2, whose slot is -1: the record lost the acting unit
    assert lost_slots(flat_of([0, 4, -1, 0, 4, -1], [1, 2], [0, 3])) == [1]


def test_unlabelled_moments_are_not_checked():
    assert lost_slots(flat_of([-1, -1, -1], [-2], [0])) == []


def test_selection_picks_the_best_mean_among_those_without_a_drop():
    from aurelia_nn.rl3.select_ckpt import pick
    t = [{"update": 0, "drops": [], "mean": 0.70}, {"update": 50, "drops": [], "mean": 0.72}, {"update": 100, "drops": ["siege"], "mean": 0.80}, {"update": 150, "drops": [], "mean": 0.74}]
    assert pick(t)["update"] == 150


def test_selection_falls_back_to_the_start():
    from aurelia_nn.rl3.select_ckpt import pick
    t = [{"update": 0, "drops": [], "mean": 0.70}, {"update": 50, "drops": ["siege"], "mean": 0.80}]
    assert pick(t)["update"] == 0


def refused(fn, *args):
    try:
        fn(*args)
    except SystemExit:
        return True
    return False


def test_records_of_the_full_state_are_refused():
    from aurelia_nn.rl3.learner import refuse_obs_versions
    refuse_obs_versions([{"obs_version": 2}, {"obs_version": 2}])
    for bad in ({}, {"obs_version": 1}):
        assert refused(refuse_obs_versions, [{"obs_version": 2}, bad])


def test_lengths_rows_of_the_full_state_are_refused():
    import json
    import tempfile
    from pathlib import Path
    from aurelia_nn.rl3.scenes import Pool
    d = Path(tempfile.mkdtemp())
    kept, rows = d / "k.jsonl", d / "r.jsonl"
    kept.write_text(json.dumps({"family": "duel", "era": "classical", "instance": 1, "attempt": 0}) + "\n")
    row = {"player": "army-net", "family": "duel", "era": "classical", "instance": 1, "attempt": 0, "rounds": 5, "win": True}
    rows.write_text(json.dumps(dict(row, info={"obs_version": "2"})) + "\n")
    assert Pool(str(kept), str(rows)).families == ["duel"]
    rows.write_text(json.dumps(row) + "\n")
    assert refused(Pool, str(kept), str(rows))
