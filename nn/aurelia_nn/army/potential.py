"""The price of a capture and the potential Phi of a moment (M-HANDOFF.md, addition 8).

The price (per play, 3 numbers): the capture's worth in units / 8, the time pressure (1 / rounds until the deadline
that matters; 0 none), and 1 ("price given"). A recording from `record.rs` carries it in the play's json (`price`);
an older recording gets it from the family by the same rule as `Scene::price` (`trainers.rs`): the worth is the
number of units the win rule lets us lose (the loss cap), the whole army where the rule sets no cap.

Phi (potential-based shaping: reward += Phi(s') - Phi(s), which keeps the best policy; never a plain distance to the
city, which would march the wounded into its fire), from the tokens of a moment (`OBSERVATION-SCHEMA.md`):
  Phi = city + 0.3 * mean over our combat units of place(u) + 0.1 * in_range - lost / worth
  city     = the objective city's hit points taken off (1 - its hp share; 1 once it is ours; 0 without a city)
  place(u) = hp >= 60 %: -(hex steps to a firing tile) / 12 = -max(0, distance - range) / 12 (melee range 1);
             below:      -(missing hp share) * (distance to the healing ground (refuge) / 12)
  in_range = the share of our healthy combat units that stand within their range of the city (the army's damage per
             turn on the city, which must beat its heal of 20 a turn)
  lost     = our units at the play's start - now (the objective token's own army count)
"""
import mlx.core as mx

# Token columns (observe.rs).
K_OWN, K_CITY = 1, 3
DIST, HP, RANGE, CIVILIAN, OWN_COUNT, REFUGE, IS_OBJECTIVE, OURS = 6, 7, 13, 26, 18, 41, 43, 45
HEALTHY = 0.6

# The loss caps of the win rules (trainers/families.rs, assault.rs): units we may lose; None = half the army.
CAP = {'assault-heal': None, 'promotion-choice': None, 'choke': None, 'citadel': None, 'hill-archers': 1, 'city-fire': 1,
       'planned-assault': 1, 'focus': 1, 'bait': 1, 'raid': 1, 'flank': 1, 'wounded': 1, 'harass': 1, 'kite': 0}


def price_of(meta):
    """The play's price [worth / 8, pressure, 1]; a recorded price must agree with the family's rule (the guard of the
    two tables: this one and `army_net.rs::loss_cap`)."""
    rule = family_price(meta)
    if 'price' in meta:
        got = list(meta['price'])
        if abs(got[0] - rule[0]) > 1e-6:
            raise ValueError(f"{meta['family']}: recorded price {got} disagrees with the rule {rule}")
        return got
    return rule


def family_price(meta):
    units = int(meta.get('measures', {}).get('units', 8)) or 8
    fam = meta['family']
    if fam in CAP:
        cap = CAP[fam]
        worth = units // 2 if cap is None else cap
    else:
        worth = units
    return [max(worth, 0.5) / 8.0, 0.0, 1.0]


def potential(tok, tmask, own0, worth):
    """Phi of moments: tok [N, T, F] (f32), tmask [N, T], own0 [N] (our units at the play's start), worth [N] (units)."""
    kind_own = (tok[..., K_OWN] > 0.5) & (tok[..., CIVILIAN] < 0.5) & tmask
    city_tok = (tok[..., K_CITY] > 0.5) & (tok[..., IS_OBJECTIVE] > 0.5) & tmask
    has_city = mx.any(city_tok, axis=1)
    taken_off = mx.where(tok[..., OURS] > 0.5, 1.0, 1.0 - tok[..., HP])
    city = mx.sum(mx.where(city_tok, taken_off, 0.0), axis=1)
    hp = tok[..., HP]
    dist = tok[..., DIST] * 12.0
    rng = mx.maximum(mx.round(tok[..., RANGE] * 3.0), 1.0)
    fire = -mx.maximum(dist - rng, 0.0) / 12.0
    heal = -(1.0 - hp) * tok[..., REFUGE]
    healthy = hp >= HEALTHY
    place = mx.where(healthy, fire, heal)
    n = mx.maximum(mx.sum(kind_own, axis=1), 1)
    place_mean = mx.sum(mx.where(kind_own, place, 0.0), axis=1) / n
    in_range = mx.sum(kind_own & healthy & (dist <= rng), axis=1) / n
    lost = mx.maximum(own0 - tok[:, 0, OWN_COUNT] * 8.0, 0.0)
    city_terms = mx.where(has_city, city + 0.3 * place_mean + 0.1 * in_range, 0.0)
    return city_terms - lost / mx.maximum(worth, 0.5)
