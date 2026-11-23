"""Exact integer combat arithmetic of the Rust rules on the GPU (MLX).

Mirrors `classic/crates/classic-rules/src/combat/damage.rs` and the combat RNG stream
(`platform/crates/kernel/src/rng.rs`, xorshift64, `below(n) = next % n`):

- the strength-ratio factor is evaluated as the exact floor of a fraction of 4th powers. Rust uses i128;
  Metal has no 128-bit integers, so the products are 128-bit numbers kept as (hi, lo) uint64 pairs with
  32-bit limb multiplication, and the quotient is an f32 estimate corrected by exact 128-bit comparisons;
- every damage function rounds like Rust (`(dmg + 50) / 100`, integer divisions on non-negative values).

All functions are elementwise over arrays of any shape (one value per scene).
"""

import mlx.core as mx

U64 = mx.uint64
I64 = mx.int64
MASK32 = 0xFFFFFFFF

MAX_HP = 100
BASE = 2400
RAND = 1200
WOUNDED_PERCENT = 33
EXACT_LIMIT = 1 << 22
CIVILIAN_DAMAGE = 40


def _u(x):
    return x.astype(U64)


def mul64(a, b):
    """Full 128-bit product of two uint64 arrays below 2^64: (hi, lo)."""
    a0, a1 = a & MASK32, a >> 32
    b0, b1 = b & MASK32, b >> 32
    p00 = a0 * b0
    p01 = a0 * b1
    p10 = a1 * b0
    p11 = a1 * b1
    mid = (p00 >> 32) + (p01 & MASK32) + (p10 & MASK32)
    lo = (p00 & MASK32) | ((mid & MASK32) << 32)
    hi = p11 + (p01 >> 32) + (p10 >> 32) + (mid >> 32)
    return hi, lo


def add128(a, b):
    hi = a[0] + b[0]
    lo = a[1] + b[1]
    carry = (lo < a[1]).astype(U64)
    return hi + carry, lo


def mul128_small(a, s):
    """(hi, lo) times s < 2^31 (the product must fit in 128 bits)."""
    hi, lo = a
    l0, l1 = lo & MASK32, lo >> 32
    p0 = l0 * s
    p1 = l1 * s
    mid = (p0 >> 32) + (p1 & MASK32)
    new_lo = (p0 & MASK32) | ((mid & MASK32) << 32)
    new_hi = hi * s + (p1 >> 32) + (mid >> 32)
    return new_hi, new_lo


def le128(a, b):
    return (a[0] < b[0]) | ((a[0] == b[0]) & (a[1] <= b[1]))


def pow4(x):
    x2 = x * x  # < 2^40 for x < 2^20
    return mul64(x2, x2)


def scale_by_ratio(dmg, me, opp, fast=False):
    """floor(dmg * k(me, opp)), Rust `damage::scale_by_ratio` (strengths in hundredths). `fast`: the
    float32 value only (no exact correction; off by one where the float rounds across an integer)."""
    me = mx.maximum(me, 1).astype(I64)
    opp = mx.maximum(opp, 1).astype(I64)
    dmg = mx.maximum(dmg, 0).astype(I64)
    weaker = opp > me
    # k = 2 (4 me)^4 / ((opp + 3 me)^4 + (4 me)^4) when opp > me, else ((me + 3 opp)^4 + (4 opp)^4) / (2 (4 opp)^4)
    small = mx.where(weaker, me, opp)
    big_x = mx.where(weaker, opp + 3 * me, me + 3 * opp)
    a = pow4(_u(4 * small))
    x4 = pow4(_u(big_x))
    s = add128(x4, a)
    two_a = (a[0] * 2 + (a[1] >> 63), a[1] * 2)
    num = (mx.where(weaker, two_a[0], s[0]), mx.where(weaker, two_a[1], s[1]))
    den = (mx.where(weaker, s[0], two_a[0]), mx.where(weaker, s[1], two_a[1]))
    # f32 estimate of the quotient from the ratio, then the exact correction.
    r = big_x.astype(mx.float32) / (4.0 * small.astype(mx.float32))
    r4 = r * r * r * r
    k = mx.where(weaker, 2.0 / (r4 + 1.0), (r4 + 1.0) / 2.0)
    q0 = mx.floor(dmg.astype(mx.float32) * k).astype(I64)
    if fast:
        return q0
    target = mul128_small(num, _u(dmg))
    best = mx.zeros_like(q0)
    for delta in (-3, -2, -1, 0, 1, 2, 3):
        q = mx.clip(q0 + delta, 0, EXACT_LIMIT)
        ok = le128(mul128_small(den, _u(q)), target)
        best = mx.where(ok, mx.maximum(best, q), best)
    # Beyond EXACT_LIMIT hundredths (419 HP: a kill whatever the hit points) the f32 value stands;
    # the other side of such an exchange deals under 1/1000 of it, so no rule compares two such values.
    return mx.where(q0 > EXACT_LIMIT - 4, q0, best)


def to_hp(dmg):
    return (mx.maximum(dmg, 0) + 50) // 100


def wounded_factor(damage, wounded_mod):
    w = mx.maximum(WOUNDED_PERCENT + wounded_mod, 0)
    return MAX_HP - damage * w // 100


def unit_damage(me, opp, my_damage, wounded_mod, roll, fast=False):
    """Melee (unit vs unit or vs city) and ranged damage of a unit (the same constants)."""
    dr = wounded_factor(my_damage.astype(I64), wounded_mod.astype(I64))
    dmg = BASE * dr // MAX_HP + roll.astype(I64) * dr // MAX_HP
    return to_hp(scale_by_ratio(dmg, me, opp, fast))


def city_damage(city, opp, roll):
    """A city's melee answer or ranged strike: no wounded reduction."""
    dmg = BASE + roll.astype(I64) * MAX_HP // MAX_HP
    return to_hp(scale_by_ratio(dmg, city, opp))


def interception_damage(interceptor, attacker, interceptor_damage, taken_percent, roll, fast=False):
    """Damage of an interception (`damage::interception_damage`): the interceptor's wounded factor is
    100 - its damage (all of it, not the 33 % rule), the rounded damage times the attacker's
    InterceptionDamageTaken factor, truncated."""
    hp = unit_damage(interceptor, attacker, interceptor_damage, mx.full(interceptor_damage.shape, 100 - WOUNDED_PERCENT, dtype=mx.int32), roll, fast)
    return hp * mx.maximum(100 + taken_percent.astype(I64), 0) // 100


def resolve_mutual_kill(to_def, def_hp, to_att, att_hp):
    dt = to_def + (MAX_HP - def_hp)
    at = to_att + (MAX_HP - att_hp)
    both = (dt >= MAX_HP) & (at >= MAX_HP)
    att_dies = dt <= at  # a tie keeps the defender
    new_def = mx.where(both & att_dies, def_hp - 1, to_def)
    new_att = mx.where(both & ~att_dies, att_hp - 1, to_att)
    return new_def, new_att


# --- the combat RNG stream ---------------------------------------------------------------------

def xorshift(state):
    x = state
    x = x ^ (x << 13)
    x = x ^ (x >> 7)
    x = x ^ (x << 17)
    return mx.maximum(x, 1)


def draw(state, n, take):
    """One `below(n)` where `take` holds: (new state, value); elsewhere the state is unchanged."""
    nxt = xorshift(state)
    val = (nxt % _u(mx.maximum(n, 1))).astype(I64)
    return mx.where(take, nxt, state), mx.where(take, val, 0)


def u64_from_str(values):
    """Python ints (strings) above 2^63 to a uint64 array (MLX takes no such Python int)."""
    ints = [int(v) for v in values]
    hi = mx.array([v >> 32 for v in ints], dtype=U64)
    lo = mx.array([v & MASK32 for v in ints], dtype=U64)
    return (hi << 32) | lo


def u64_to_ints(arr):
    return [int(v) for v in arr.tolist()]
