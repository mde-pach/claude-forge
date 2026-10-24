"""Small, dependency-free statistics toolkit used by analyze.py.

Every resampling routine takes an explicit seed and builds its own `random.Random`, so results do
not depend on call order. Percentile intervals are used for all bootstraps (Efron & Tibshirani
1993, ch. 13).
"""

from __future__ import annotations

import math
import random
import statistics

SEED = 20261024
N_BOOT = 10_000
N_PERM = 100_000


# ---------------------------------------------------------------- descriptive

def mean(xs):
    xs = [x for x in xs if x is not None]
    return sum(xs) / len(xs) if xs else None


def median(xs):
    xs = [x for x in xs if x is not None]
    return statistics.median(xs) if xs else None


def quartiles(xs):
    """(q1, median, q3) with linear interpolation (Hyndman & Fan type 7, `inclusive`)."""
    xs = sorted(x for x in xs if x is not None)
    if not xs:
        return (None, None, None)
    if len(xs) == 1:
        return (xs[0], xs[0], xs[0])
    q = statistics.quantiles(xs, n=4, method="inclusive")
    return (q[0], q[1], q[2])


def percentile(sorted_xs, p):
    """Type-7 percentile of an already sorted list, p in [0, 1]."""
    if not sorted_xs:
        return None
    h = (len(sorted_xs) - 1) * p
    lo = math.floor(h)
    hi = min(lo + 1, len(sorted_xs) - 1)
    return sorted_xs[lo] + (h - lo) * (sorted_xs[hi] - sorted_xs[lo])


# ---------------------------------------------------------------- bootstrap

def bootstrap_ci(units, stat, *, level=0.95, n_boot=N_BOOT, seed=SEED):
    """Cluster (unit-level) nonparametric bootstrap, percentile CI.

    `units` is a list of clusters (here: tasks); each replicate draws len(units) clusters with
    replacement and evaluates `stat(list_of_clusters)`. Replicates where stat returns None are
    discarded. Returns (point, lo, hi).
    """
    if not units:
        return (None, None, None)
    point = stat(units)
    rng = random.Random(seed)
    n = len(units)
    reps = []
    for _ in range(n_boot):
        v = stat([units[rng.randrange(n)] for _ in range(n)])
        if v is not None:
            reps.append(v)
    reps.sort()
    a = (1 - level) / 2
    return (point, percentile(reps, a), percentile(reps, 1 - a))


# ---------------------------------------------------------------- permutation

def sign_flip_test(diffs, *, exact_max=20, n_perm=N_PERM, seed=SEED):
    """Two-sided paired sign-flip permutation test of H0: the d_i are symmetric around 0.

    Statistic T = sum(d_i) (equivalent to the mean for fixed n). Zero differences do not change
    under a flip and are left out. With m <= exact_max nonzero d_i the null distribution over all
    2^m sign patterns is computed exactly (by convolution over distinct partial sums, which is
    exact and avoids enumerating 2^m patterns one by one); otherwise n_perm random sign patterns
    are drawn and p = (1 + #{|T*| >= |T|}) / (1 + n_perm) (Phipson & Smyth 2010).
    Returns dict(p, method, n_nonzero).
    """
    nz = [d for d in diffs if abs(d) > 1e-12]
    m = len(nz)
    if m == 0:
        return {"p": 1.0, "method": "exact", "n_nonzero": 0}
    t_obs = abs(sum(nz))
    tol = 1e-7  # distinct attainable sums differ by far more; guards float error
    if m <= exact_max:
        dist = {0.0: 1}
        for d in nz:
            new = {}
            for s, c in dist.items():
                for v in (round(s + d, 10), round(s - d, 10)):
                    new[v] = new.get(v, 0) + c
            dist = new
        hits = sum(c for s, c in dist.items() if abs(s) >= t_obs - tol)
        return {"p": hits / 2 ** m, "method": "exact", "n_nonzero": m}
    rng = random.Random(seed)
    hits = 0
    for _ in range(n_perm):
        s = 0.0
        for d in nz:
            s += d if rng.random() < 0.5 else -d
        if abs(s) >= t_obs - tol:
            hits += 1
    return {"p": (hits + 1) / (n_perm + 1), "method": f"monte_carlo_{n_perm}", "n_nonzero": m}


def label_permutation_heterogeneity(groups, *, n_perm=10_000, seed=SEED):
    """Permutation test of heterogeneity of paired differences across groups (e.g. repositories).

    groups: {label: [d_i, ...]}. Statistic = between-group sum of squares
    sum_g n_g (mean_g - grand_mean)^2 (one-way ANOVA numerator); group labels are permuted across
    tasks. Returns dict(stat, p).
    """
    labels, values = [], []
    for g, ds in groups.items():
        for d in ds:
            labels.append(g)
            values.append(d)
    if len(set(labels)) < 2:
        return {"stat": None, "p": None}

    def ss_between(lbls):
        grand = sum(values) / len(values)
        acc = {}
        for l, v in zip(lbls, values):
            s, n = acc.get(l, (0.0, 0))
            acc[l] = (s + v, n + 1)
        return sum(n * (s / n - grand) ** 2 for s, n in acc.values())

    obs = ss_between(labels)
    rng = random.Random(seed)
    lbls = labels[:]
    hits = 0
    for _ in range(n_perm):
        rng.shuffle(lbls)
        if ss_between(lbls) >= obs - 1e-12:
            hits += 1
    return {"stat": obs, "p": (hits + 1) / (n_perm + 1)}


# ---------------------------------------------------------------- exact tests

def binom_two_sided_p(k, n, p=0.5):
    """Exact two-sided binomial p-value. For p = 0.5 this is min(1, 2 * P(X <= min(k, n-k)))."""
    if n == 0:
        return 1.0
    if p == 0.5:
        k = min(k, n - k)
        tail = sum(math.comb(n, i) for i in range(k + 1)) / 2 ** n
        return min(1.0, 2 * tail)
    pmf = [math.comb(n, i) * p ** i * (1 - p) ** (n - i) for i in range(n + 1)]
    return min(1.0, sum(q for q in pmf if q <= pmf[k] * (1 + 1e-7)))


def mcnemar_exact(b, c):
    """Exact McNemar test: under H0 the b discordant pairs out of b + c are Binomial(b + c, 1/2)."""
    return {"b_forge_only": b, "c_baseline_only": c, "n_discordant": b + c,
            "p": binom_two_sided_p(b, b + c)}


def fisher_exact(a, b, c, d):
    """Two-sided Fisher exact test on [[a, b], [c, d]] (sum of hypergeometric probabilities of
    tables no more likely than the observed one)."""
    r1, r2, c1 = a + b, c + d, a + c
    n = r1 + r2
    lo, hi = max(0, c1 - r2), min(r1, c1)

    def pmf(x):
        return math.comb(r1, x) * math.comb(r2, c1 - x) / math.comb(n, c1)

    p_obs = pmf(a)
    return min(1.0, sum(pmf(x) for x in range(lo, hi + 1) if pmf(x) <= p_obs * (1 + 1e-7)))


def chi2_2x2(a, b, c, d):
    """Pearson chi-square (no continuity correction), df = 1; p = erfc(sqrt(X2 / 2))."""
    n = a + b + c + d
    rows, cols = (a + b, c + d), (a + c, b + d)
    if 0 in rows or 0 in cols:
        return {"chi2": 0.0, "p": 1.0}
    x2 = n * (a * d - b * c) ** 2 / (rows[0] * rows[1] * cols[0] * cols[1])
    return {"chi2": x2, "p": math.erfc(math.sqrt(x2 / 2))}


def mantel_haenszel_or(tables, level=0.95):
    """Mantel-Haenszel common odds ratio across strata with the Robins-Breslow-Greenland (1986)
    variance of log(OR_MH).

    tables: list of (a, b, c, d) = (exposed events, exposed non-events, unexposed events,
    unexposed non-events), here exposed = forge, event = resolved, stratum = task.
    Strata where either sum(ad/n) or sum(bc/n) gets no contribution (concordant outcomes) add
    nothing, as in the standard estimator.
    """
    R = S = PR = PS_QR = QS = 0.0
    used = 0
    for a, b, c, d in tables:
        n = a + b + c + d
        if n == 0:
            continue
        r, s = a * d / n, b * c / n
        p, q = (a + d) / n, (b + c) / n
        R += r
        S += s
        PR += p * r
        PS_QR += p * s + q * r
        QS += q * s
        if r or s:
            used += 1
    out = {"strata_informative": used, "or": None, "lo": None, "hi": None, "log_se": None}
    if R == 0 or S == 0:
        out["note"] = "OR undefined (no discordant information in one direction)"
        return out
    or_mh = R / S
    var = PR / (2 * R ** 2) + PS_QR / (2 * R * S) + QS / (2 * S ** 2)
    se = math.sqrt(var)
    z = statistics.NormalDist().inv_cdf(1 - (1 - level) / 2)
    out.update({"or": or_mh, "log_se": se,
                "lo": math.exp(math.log(or_mh) - z * se), "hi": math.exp(math.log(or_mh) + z * se),
                "p_wald": 2 * (1 - statistics.NormalDist().cdf(abs(math.log(or_mh)) / se))})
    return out
