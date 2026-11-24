"""The winners-only labels of train_m (`--teachers-from`): which teacher plays stay in the training mix."""
import json

from aurelia_nn.army.train_m import load_teachers, teacher_allowed


def test_a_listed_family_keeps_only_its_listed_teachers():
    t = {'planned-assault': {'ref-planned', 'ref-rotate'}}
    assert teacher_allowed({'family': 'planned-assault', 'player': 'ref-planned'}, t)
    assert not teacher_allowed({'family': 'planned-assault', 'player': 'unciv'}, t)


def test_the_nets_own_plays_unlisted_families_and_no_list_stay():
    t = {'planned-assault': {'ref-planned'}}
    assert teacher_allowed({'family': 'planned-assault', 'player': 'army-net'}, t)
    assert teacher_allowed({'family': 'duel', 'player': 'unciv'}, t)
    assert teacher_allowed({'family': 'planned-assault', 'player': 'unciv'}, None)


def test_the_file_takes_the_families_key_or_the_bare_map(tmp_path):
    a, b = tmp_path / 'a.json', tmp_path / 'b.json'
    a.write_text(json.dumps({'families': {'siege': ['ref-siege']}, 'rule': 'x'}))
    b.write_text(json.dumps({'siege': ['ref-siege']}))
    assert load_teachers(a) == load_teachers(b) == {'siege': {'ref-siege'}}
