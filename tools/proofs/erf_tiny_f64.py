#!/usr/bin/env python3
"""Proof tool for the `double` erf at |x| < 2^-55 (LIBM-ERF-TINY-F64; no spec clause, a proof aid).

erf x = c*x*(1 - x^2/3 + x^4/10 - ...) with c = 2/sqrt(pi), so for 0 < x < 2^-55 the true value lies in
(c*x*(1 - B), c*x) with B = 2^-110/3 (the alternating series: the first dropped term bounds the rest).
The in-place entry answers this band with one of two evaluations (crates/math/src/fast_special.rs,
`erf_linear`):

  * F (FMA build, 2^-1022 <= |x| < 2^-55):  v = fma(x, CH, x*CL) * 2^-64,  CH + CL = c*2^64 (double-double).
    Every operand and intermediate is normal (|x*CL| >= 2^-1022*2^8), the last product is exact, so the
    evaluation is the same at every exponent: with X the 53-bit integer significand of x it is
    rnd(X*ch + rnd(X*cl)) relative to the grid of X*c.
  * I (integer, every |x| < 2^-55 in the SSE2 build, subnormal x in both): n = floor((X'*C + 2^127) / 2^128)
    with C = round(c*2^127) (128 bits) and X' in [2^53/c, 2^54/c) (X' = X or 2X for normal x, X' = 2m for a
    subnormal x = m*2^-1074): n = round(X'*c/2), the result's 53-bit (or subnormal-grid) significand.

Lemma (breakpoints, all four modes). Measured in ulps of the result, a result significand X*c (X*c < 2^53)
or X*c/2 (X*c >= 2^53) or m*c (subnormal) has distance ||q*c||/2 to the nearest breakpoint (a midpoint or
a representable number, i.e. the half-integers and the integers), where q = 2X, X or 2m, so q <= 2^54.
By the best-approximation property of the continued fraction of c, ||q*c|| >= ||q_k*c|| for q < q_{k+1}.
This tool computes c to 600 bits, the convergents, and that bound DELTA.

  * I: the evaluation error is X'*|c - C/2^127|/2 <= 2^-75 ulps (the floors are exact integer arithmetic),
    the series bias B*2^53.2 <= 2^-58.5 ulps; DELTA/2 exceeds their sum, so n is the correctly rounded
    significand of erf x in round-to-nearest, and floor/ceil (the directed modes) likewise.
  * F: the error is |rnd(X*cl) - X*cl| + X*|c - ch - cl| + the bias; it can exceed DELTA/2, so every q <= 2^54
    with ||q*c|| < EPS (EPS = twice that bound, rounded up) is ENUMERATED from the lattice basis
    (q_k, q_k*c - p_k), (q_{k+1}, q_{k+1}*c - p_{k+1}) (unimodular, so it spans every (q, q*c - p)), and F
    is simulated exactly for each such X in all four modes (each operation rounded in that mode) and
    compared with the correctly rounded ends of the interval (c*x*(1 - B), c*x). Every other X is at least
    EPS/2 from every breakpoint, more than F's error, so F rounds like erf x.

Flags. F: the fma is inexact (its exact sum is never a representable number: DELTA), every other operation
is exact or inexact on normal values, so inexact alone. I: the caller multiplies the result by
1 + 2^-60 (value unchanged in round-to-nearest, inexact; tiny, hence underflow, exactly when the result is
below 2^-1022). Exactly one subnormal x = M0*2^-1074 has m*c in (2^52 - 1/2, 2^52 - 1/4): its result is the normal
2^-1022, yet tiny when rounded to 53 bits with unbounded exponent (x86's tininess after rounding), so the
in-place path raises underflow for every m <= M0 (a probe of the reference shows the same flags there).

Run: python3 tools/proofs/erf_tiny_f64.py  (stdlib only; prints the constants and the candidates, exits 1
if any check fails). The candidates' operands are pinned in crates/math/tests/fast_special.rs.
"""
import math
import sys
from fractions import Fraction as F

P = 1400
K = 600


def atan_inv(n):
    s, t, k, n2 = 0, (1 << P) // n, 0, n * n
    while t:
        s += t // (2 * k + 1) if k % 2 == 0 else -(t // (2 * k + 1))
        t //= n2
        k += 1
    return s


PI = 16 * atan_inv(5) - 4 * atan_inv(239)  # pi*2^P within a few hundred units
CK = math.isqrt((4 << (2 * K + P)) // PI)  # floor(c*2^K) up to the last few bits
c = F(CK, 1 << K)
CERR = F(1, 1 << (K - 8))  # |c - CK/2^K| bound (generous)


def rnd(v, mode):
    """v rounded to a normal double in mode ('N', 'U', 'D', 'Z')."""
    if v == 0:
        return v
    s = -1 if v < 0 else 1
    a = abs(v)
    e = a.numerator.bit_length() - a.denominator.bit_length()
    if F(2) ** e > a:
        e -= 1
    ulp = F(2) ** (e - 52)
    q = a / ulp
    fl = q.numerator // q.denominator
    fr = q - fl
    if mode == 'N':
        n = fl + (1 if fr > F(1, 2) or (fr == F(1, 2) and fl % 2 == 1) else 0)
    elif mode == 'Z' or (mode == 'U' and s < 0) or (mode == 'D' and s > 0):
        n = fl
    else:
        n = fl + (1 if fr else 0)
    return s * n * ulp


def dbl(v):
    return rnd(v, 'N')


def hexd(v):
    return float(v).hex()


fails = 0


def check(cond, what):
    global fails
    if not cond:
        fails += 1
        print('FAIL:', what)


# Constants.
ch = dbl(c)
cl = dbl(c - ch)
C127 = (CK + (1 << (K - 128))) >> (K - 127)  # round(c*2^127)
T = math.ceil(F(1 << 53) / c)  # least X with X*c >= 2^53
print('c   =', float(c), 'ch =', hexd(ch), 'cl =', hexd(cl))
print('CH = ch*2^64 =', hexd(ch * 2 ** 64), ' CL = cl*2^64 =', hexd(cl * 2 ** 64))
print('C = round(c*2^127) = %#034x' % C127)
print('T = ceil(2^53/c) = %#x' % T)
check(C127 >> 127 == 1, 'C has 128 bits')
check(T * c >= 2 ** 53 and (T - 1) * c < 2 ** 53, 'T')

# Continued fraction and the bound DELTA for q <= 2^54.
Q = 1 << 54
x = c
p0, p1, q0, q1 = 0, 1, 1, 0
conv = []
while q1 <= 1 << 60:
    a = x.numerator // x.denominator
    p0, p1, q0, q1 = p1, a * p1 + p0, q1, a * q1 + q0
    conv.append((a, p1, q1))
    x = 1 / (x - a)
kk = max(i for i, (_, _, q) in enumerate(conv) if q <= Q)
_, pk, qk = conv[kk]
_, pn, qn = conv[kk + 1]
dk, dn = qk * c - pk, qn * c - pn
DELTA = abs(dk) - qk * CERR
print('convergent q_k = %d (2^%.2f), next 2^%.2f; DELTA = min ||q c|| (q <= 2^54) >= 2^%.3f'
      % (qk, math.log2(qk), math.log2(qn), math.log2(DELTA)))
check(abs(qk * pn - qn * pk) == 1, 'unimodular basis')

# I: error and bias against DELTA/2.
B = F(1, 3 * 2 ** 110)
err_i = F(1, 2 ** 75) + B * F(2 ** 54)  # 2^54 >= X'c/2 in ulps... generous: X'c/2 < 2^53.2
print('I: error + bias <= 2^%.2f ulps, DELTA/2 = 2^%.2f' % (math.log2(err_i), math.log2(DELTA / 2)))
check(err_i < DELTA / 2, 'I within DELTA/2')
# 2^-75: X' < 2^54, |c - C/2^127| <= 2^-128, halved.
check(abs(c - F(C127, 1 << 127)) <= F(1, 1 << 128) + CERR, 'C within 2^-128')

# F: error bound in ulps (X*c in [2^52.17, 2^53.17), ulp >= 1 in X units, x scaled to X).
tl = (1 << 53) * cl  # largest X*cl
ulp_t = F(2) ** (tl.numerator.bit_length() - tl.denominator.bit_length() - 52 + 1)
err_f = ulp_t + (1 << 53) * (abs(c - ch - cl) + CERR) + B * (1 << 54)
EPS = 2 * err_f
EPS = F(1, 2 ** math.floor(-math.log2(EPS)))  # a power of two >= 2*err_f
print('F: error <= 2^%.2f ulps (rnd(X*cl) <= 2^%.0f); enumerating ||q c|| < EPS = 2^%d'
      % (math.log2(err_f), math.log2(ulp_t), round(math.log2(EPS))))
check(err_f < EPS / 2, 'EPS')

# Enumerate every q <= 2^54 with ||q c|| < EPS.
D = qk * pn - qn * pk  # +-1
imax = math.ceil(Q * abs(dn) + EPS * qn) + 1
jmax = math.ceil(EPS * qk + Q * abs(dk)) + 1
cands = set()
for i in range(-imax, imax + 1):
    for j in range(-jmax, jmax + 1):
        q = i * qk + j * qn
        p = i * pk + j * pn
        if 1 <= q <= Q and abs(q * c - p) < EPS + q * CERR:
            cands.add(q)
print('candidates q:', len(cands), '(i, j ranges %d, %d)' % (imax, jmax))

hard = set()
for q in sorted(cands):
    for X in {q, q // 2} if q % 2 == 0 else {q}:
        if not (1 << 52) <= X < (1 << 53):
            continue
        xs = F(X, 1 << 112)  # x in [2^-60, 2^-59): the evaluation is exponent-free (all normal)
        hard.add(X)
        for mode in 'NUDZ':
            t = rnd(xs * cl * 2 ** 64, mode)
            s = xs * ch * 2 ** 64 + t
            v = rnd(s, mode) / 2 ** 64
            hi, lo = rnd(xs * c, mode), rnd(xs * c * (1 - B), mode)
            check(v == hi == lo, 'F at X=%#x mode %s' % (X, mode))
            check(rnd(s, mode) != s, 'F exact at X=%#x' % X)
for X in sorted(hard):
    print('  hard X = %#x  (x = %s)' % (X, hexd(F(X, 1 << 112))))

# The tininess window for subnormal x: tiny after rounding to 53 bits (x86) iff m*c < 2^52 - 1/4.
m0 = math.ceil((F(1 << 52) - F(1, 2)) / c)
check(F(1 << 52) - F(1, 2) < m0 * c < F(1 << 52) - F(1, 4), 'M0 in the window')
check((m0 + 1) * c > F(1 << 52) - F(1, 4), 'only M0 in the window')
print('M0 = %#x: m*c = 2^52 - %.4f rounds to 2^52 (result 2^-1022) yet is tiny at 53 bits;'
      ' underflow exactly for m <= M0' % (m0, float(F(1 << 52) - m0 * c)))

print('FAILED %d' % fails if fails else 'ALL CHECKS PASSED')
sys.exit(1 if fails else 0)
