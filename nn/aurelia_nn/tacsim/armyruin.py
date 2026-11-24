"""Ancient ruins (twin package 7; Rust `systems/ruins.rs` `enter_ruins`, called for every step a unit takes into a tile).

The first unit of an AI major that enters a tile with ruins removes them and takes the FIRST of three different rewards,
which are drawn from the combat stream: `below(6)`, `below(5)`, `below(4)` (three draws of the one stream, whatever the reward).
The rewards the twin reads: citizen (+1 population of the player's nearest city), experience (+10). Culture, faith, gold and the
map reward change nothing the twin keeps; a unit that chooses its reward (the Native Tongue promotion) is not modelled.

State: `s["ruin"]` [B, T] int32 (1: ruins stand on the tile), set by the loaders from the dump (`ruins`); absent: no ruins.
"""
import mlx.core as mx

from . import exact
from .sim import gn, g1, put1, I32

REWARD_CITIZEN, REWARD_EXPERIENCE = 0, 4  # the index in Rust's `REWARDS` (citizen, culture, faith, gold, experience, map)
EXPERIENCE = 10


def _drawn(s, hit):
    """The three draws of the offers; the first one is the reward taken ([B])."""
    rng, first = exact.draw(s["rng"], mx.full(hit.shape, 6, dtype=mx.int64), hit)
    rng, _ = exact.draw(rng, mx.full(hit.shape, 5, dtype=mx.int64), hit)
    rng, _ = exact.draw(rng, mx.full(hit.shape, 4, dtype=mx.int64), hit)
    s["rng"] = rng
    return first


def _nearest_city(sim, s, side, tile):
    """The player's nearest city to `tile` (the first one in handle order on a tie), -1 none: [B]."""
    ct = sim.cs["tile"]
    mine = sim.cs["alive"] & (s["c_owner"] == side[:, None]) & (ct >= 0)
    dist = sim.geo.distance(mx.maximum(ct, 0), mx.broadcast_to(tile[:, None], ct.shape))
    key = mx.where(mine, dist * 64 + mx.arange(ct.shape[1])[None, :], 1 << 30)
    best = mx.argmin(key, axis=1).astype(I32)
    return mx.where(mx.min(key, axis=1) < (1 << 30), best, -1)


def enter(sim, s, slot, tile, where):
    """The unit `slot` of every row in `where` has entered `tile`: the ruins there give their reward."""
    if "ruin" not in s:
        return s
    side = g1(s["u_side"], slot)
    hit = where & (tile >= 0) & (gn(s["ruin"], tile[:, None])[:, 0] > 0) & (side >= 0) & (side <= 1)
    if "camp_tile" in s:  # the barbarians of a camp scene are no major: no reward
        hit = hit & ~((s["camp_tile"] >= 0) & (side == 1))
    if not bool(mx.any(hit).item()):
        return s
    s = dict(s)
    s["ruin"] = put1(s["ruin"], tile, mx.zeros_like(tile), hit)
    first = _drawn(s, hit)
    xp = hit & (first == REWARD_EXPERIENCE)
    s["u_xp"] = put1(s["u_xp"], slot, g1(s["u_xp"], slot) + EXPERIENCE, xp)
    city = _nearest_city(sim, s, side, tile)
    grow = hit & (first == REWARD_CITIZEN) & (city >= 0)
    s["c_pop"] = put1(s["c_pop"], city, g1(s["c_pop"], city) + 1, grow)
    return s


def enter_path(sim, s, slot, path, walked):
    """The tiles of a walk, `path` [B, P] in order, `walked` [B, P] those the unit really entered: every ruin on them, in order."""
    if "ruin" not in s:
        return s
    on = walked & (path >= 0) & (gn(s["ruin"], path) > 0)
    if not bool(mx.any(on).item()):
        return s
    for j in range(path.shape[1]):
        if not bool(mx.any(on[:, j]).item()):
            continue
        s = enter(sim, s, slot, path[:, j], on[:, j])
    return s
