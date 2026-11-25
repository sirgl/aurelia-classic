"""Exact and approximate tests for gate statistics (standard library only).

Conventions
  A = the reference net, B = the candidate net. "B worse" means B has fewer wins than A.
  Paired counts: b = plays A won and B lost, c = plays A lost and B won (discordant plays).
"""
import math
from statistics import NormalDist

Z95 = 1.959963984540054


def _lchoose(n, k):
    return math.lgamma(n + 1) - math.lgamma(k + 1) - math.lgamma(n - k + 1)


def binom_pmf(k, n, p=0.5):
    if k < 0 or k > n:
        return 0.0
    if p <= 0.0:
        return 1.0 if k == 0 else 0.0
    if p >= 1.0:
        return 1.0 if k == n else 0.0
    return math.exp(_lchoose(n, k) + k * math.log(p) + (n - k) * math.log1p(-p))


def binom_sf(k, n, p=0.5):
    """P(X >= k) for X ~ Bin(n, p), summed exactly from the smaller tail."""
    if k <= 0:
        return 1.0
    if k > n:
        return 0.0
    if k > n * p:
        return min(1.0, sum(binom_pmf(i, n, p) for i in range(k, n + 1)))
    return max(0.0, 1.0 - sum(binom_pmf(i, n, p) for i in range(0, k)))


def mcnemar_exact(b, c):
    """Exact McNemar test on the discordant plays. b = B worse (A won, B lost), c = B better.
    Returns p_worse (one-sided, H1: B worse), p_better (one-sided, H1: B better), p_two (two-sided)."""
    m = b + c
    if m == 0:
        return {'p_worse': 1.0, 'p_better': 1.0, 'p_two': 1.0}
    p_worse = binom_sf(b, m)
    p_better = binom_sf(c, m)
    p_two = min(1.0, 2.0 * min(p_worse, p_better))
    return {'p_worse': p_worse, 'p_better': p_better, 'p_two': p_two}


def fisher_exact(xa, na, xb, nb):
    """Fisher's exact test for two independent samples: A has xa wins of na, B xb of nb.
    Returns p_worse (H1: B rate < A rate), p_better, p_two (sum of the tables not more likely than the observed one)."""
    n, k = na + nb, xa + xb
    lo, hi = max(0, k - na), min(k, nb)

    def pmf(x):
        return math.exp(_lchoose(nb, x) + _lchoose(na, k - x) - _lchoose(n, k))
    probs = {x: pmf(x) for x in range(lo, hi + 1)}
    p_worse = min(1.0, sum(p for x, p in probs.items() if x <= xb))
    p_better = min(1.0, sum(p for x, p in probs.items() if x >= xb))
    p_obs = probs[xb]
    p_two = min(1.0, sum(p for p in probs.values() if p <= p_obs * (1 + 1e-7)))
    return {'p_worse': p_worse, 'p_better': p_better, 'p_two': p_two}


def wilson(k, n, z=Z95):
    """Wilson score interval for a rate."""
    if n <= 0:
        return (0.0, 1.0)
    p = k / n
    d = 1 + z * z / n
    centre = (p + z * z / (2 * n)) / d
    half = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / d
    return (max(0.0, centre - half), min(1.0, centre + half))


def newcombe_paired(n11, n10, n01, n00, z=Z95):
    """Newcombe's method 10: CI for the paired difference p_B - p_A.
    n11 both won, n10 only A won, n01 only B won, n00 neither."""
    n = n11 + n10 + n01 + n00
    if n == 0:
        return (-1.0, 1.0)
    p1, p2 = (n11 + n10) / n, (n11 + n01) / n
    l1, u1 = wilson(n11 + n10, n, z)
    l2, u2 = wilson(n11 + n01, n, z)
    den = math.sqrt((n11 + n10) * (n01 + n00) * (n11 + n01) * (n10 + n00))
    phi = (n11 * n00 - n10 * n01) / den if den > 0 else 0.0
    d = p2 - p1
    lo = d - math.sqrt(max(0.0, (p2 - l2) ** 2 + (u1 - p1) ** 2 - 2 * phi * (p2 - l2) * (u1 - p1)))
    hi = d + math.sqrt(max(0.0, (u2 - p2) ** 2 + (p1 - l1) ** 2 - 2 * phi * (u2 - p2) * (p1 - l1)))
    return (max(-1.0, lo), min(1.0, hi))


def holm(ps):
    """Holm step-down adjusted p-values (family-wise error control), in the input order."""
    m = len(ps)
    order = sorted(range(m), key=lambda i: ps[i])
    adj = [0.0] * m
    run = 0.0
    for rank, i in enumerate(order):
        run = max(run, min(1.0, (m - rank) * ps[i]))
        adj[i] = run
    return adj


def benjamini_hochberg(ps):
    """Benjamini-Hochberg adjusted p-values (false discovery rate), in the input order."""
    m = len(ps)
    order = sorted(range(m), key=lambda i: ps[i], reverse=True)
    adj = [0.0] * m
    run = 1.0
    for j, i in enumerate(order):
        rank = m - j
        run = min(run, ps[i] * m / rank)
        adj[i] = min(1.0, run)
    return adj


# ---- power -------------------------------------------------------------------------------------

_CRIT = {}


def _crit(m, alpha):
    """Smallest k with P(Bin(m, .5) >= k) <= alpha, or None when no k exists (too few discordant plays)."""
    key = (m, alpha)
    if key not in _CRIT:
        if binom_sf(m, m) > alpha:
            _CRIT[key] = None
        else:
            lo, hi = 0, m
            while lo < hi:
                mid = (lo + hi) // 2
                if binom_sf(mid, m) <= alpha:
                    hi = mid
                else:
                    lo = mid + 1
            _CRIT[key] = lo
    return _CRIT[key]


def power_paired(n, q, delta, alpha):
    """Power of the one-sided exact McNemar test (H1: B worse by delta) with n paired plays,
    a discordant share q of the plays, and a true drop delta (as a share of plays, 0 < delta <= q)."""
    if q <= 0 or delta <= 0:
        return 0.0
    delta = min(delta, q)
    pi = (q + delta) / (2 * q)
    sd = math.sqrt(n * q * (1 - q))
    lo, hi = max(0, int(n * q - 6 * sd)), min(n, int(n * q + 6 * sd) + 1)
    total = 0.0
    for m in range(lo, hi + 1):
        pm = binom_pmf(m, n, q)
        if pm < 1e-12:
            continue
        k = _crit(m, alpha)
        if k is not None:
            total += pm * binom_sf(k, m, pi)
    return total


def mde_paired(n, q, alpha, power=0.8):
    """Smallest true drop (share of plays) the paired exact test finds with the given power; None if q is too small."""
    lo, hi = 0.0, q
    if power_paired(n, q, hi, alpha) < power:
        return None
    for _ in range(14):
        mid = (lo + hi) / 2
        if power_paired(n, q, mid, alpha) >= power:
            hi = mid
        else:
            lo = mid
    return hi


def mde_unpaired(n, p0, alpha, power=0.8):
    """Normal-approximation smallest drop between two independent samples of n plays each at baseline rate p0."""
    nd = NormalDist()
    za, zb = nd.inv_cdf(1 - alpha), nd.inv_cdf(power)
    d = 0.05
    for _ in range(30):
        p = max(0.0, p0 - d / 2)
        d = (za + zb) * math.sqrt(2 * max(p * (1 - p), 1e-9) / n)
    return min(d, p0)
