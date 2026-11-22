"""The float twin of net2 against the integer net (design §3.2, gate G6).

The fixture is written by the integer net (`net2::tests::the_parity_fixture_is_the_integer_net`):
200 generated turns with their inputs and every neuron.
"""

from aurelia_nn.net2.forward import EXACT, FIXTURES, FLOAT, NEURONS, Weights, parity


def _lines():
    return (FIXTURES / "parity.jsonl").read_text().splitlines()


def test_the_exact_port_is_the_integer_net():
    worst = parity(Weights.load(), _lines(), EXACT, teacher=False)
    assert all(worst[k] == 0 for k in NEURONS), worst
    assert worst["decisions_differ"] == 0, worst


def test_every_float_neuron_within_two_thousandths():
    lines = _lines()
    assert len(lines) == 200
    worst = parity(Weights.load(), lines, FLOAT, teacher=True)
    assert all(worst[k] <= 2 for k in NEURONS if k not in ("y", "city_cap")), worst
    # `city_cap` counts thousandths of a city with two products at weight 6000 (`Shape::Times`): the integer net's
    # rounding of each product, times 6, plus the sum's: up to 13 thousandths of a city, not a defect of the twin.
    assert worst["city_cap"] <= 13, worst
    # The shares come from the carried integer softmax (`strategy_net::layers::softmax`), which gives
    # the largest share the rounding rest: up to 3 thousandths for four choices, not a defect of the twin.
    assert worst["y"] <= 3, worst
    assert worst["decisions_differ"] == 0, worst
