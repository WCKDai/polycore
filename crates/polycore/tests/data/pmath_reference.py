#!/usr/bin/env python3
"""Generates `pmath_reference.txt`: the correctly rounded value of every `pmath` function at
named and pseudo-random arguments, from a 70-digit decimal reference (Python's `decimal`,
whose `exp`, `ln`, `log10` and `sqrt` are correctly rounded at the working precision; the
trigonometric functions are Taylor series summed at that precision). Never the platform's libm.

    python3 -I pmath_reference.py > pmath_reference.txt

Each line is `function hex-bits-of-input [hex-bits-of-second-input] hex-bits-of-result margin`,
the margin being the distance of the true value from the nearest rounding boundary in units of
the result's ULP (a tiny margin means the case is hard; the test tolerates an ULP there, and
reports it).
"""
import struct
from decimal import Decimal as D, getcontext, ROUND_HALF_EVEN

getcontext().prec = 70
EPS = D(10) ** -68


def bits(f):
    return struct.unpack("<Q", struct.pack("<d", f))[0]


def from_bits(b):
    return struct.unpack("<d", struct.pack("<Q", b))[0]


def pi():
    def atan_inv(n):
        x = D(1) / n
        x2 = x * x
        s = x
        t = x
        k = 1
        while True:
            t = -t * x2
            k += 2
            term = t / k
            if abs(term) < EPS:
                return s
            s += term

    return 4 * (4 * atan_inv(5) - atan_inv(239))


PI = pi()
LN2 = D(2).ln()


def sin_cos(x):
    # reduce about multiples of π/2 at 70 digits (|x| is modest in the samples)
    q = (x / (PI / 2)).to_integral_value(rounding=ROUND_HALF_EVEN)
    r = x - q * (PI / 2)
    s = D(0)
    c = D(0)
    term = D(1)
    k = 0
    while abs(term) > EPS:
        if k % 4 == 0:
            c += term
        elif k % 4 == 1:
            s += term
        elif k % 4 == 2:
            c -= term
        else:
            s -= term
        k += 1
        term = term * r / k
    q = int(q) % 4
    return [(s, c), (c, -s), (-s, -c), (-c, s)][q]


def atan_pos(t):
    # t >= 0: halve thrice, series
    t = D(t)
    for _ in range(3):
        t = t / (1 + (1 + t * t).sqrt())
    z = t * t
    s = t
    term = t
    k = 1
    while abs(term) > EPS:
        term = -term * z
        k += 2
        s += term / k
    return 8 * s


def atan2(y, x):
    y = D(y)
    x = D(x)
    if x > 0:
        return atan_pos(abs(y) / x) * (1 if y >= 0 else -1) if y != 0 else D(0)
    if x < 0:
        a = PI - atan_pos(abs(y) / -x)
        return a if y >= 0 else -a
    return (PI / 2) if y > 0 else -(PI / 2)


def nearest(v):
    """The nearest float to the decimal `v` and its margin from a rounding boundary in ULPs."""
    if v == 0:
        return 0.0, 1.0
    f = float(v)  # correctly rounded by Python
    if f == 0.0:
        tiny = from_bits(1)  # the smallest subnormal: the boundary is half of it
        return (0.0 if v > 0 else -0.0), float(abs(abs(v) - D(tiny) / 2) / D(tiny))
    if f in (float("inf"), float("-inf")):
        return f, 1.0
    # neighbours
    b = bits(f)
    if f > 0:
        lo, hi = from_bits(b - 1) if b > 0 else f, from_bits(b + 1)
    else:
        lo, hi = from_bits(b + 1), from_bits(b - 1)
    # the boundary below and above f
    m_lo = (D(f) + D(lo)) / 2
    m_hi = (D(f) + D(hi)) / 2
    ulp = abs(D(hi) - D(f))
    margin = min(abs(v - m_lo), abs(v - m_hi)) / ulp
    return f, float(margin)


def main():
    import random

    rng = random.Random(0x5EED_12)

    def rand(lo, hi):
        return lo + (hi - lo) * rng.random()

    def rand_log(lo, hi):
        import math

        return math.exp(rand(math.log(lo), math.log(hi)))

    out = []

    def emit(name, xs, res):
        f, margin = nearest(res)
        out.append(f"{name} {' '.join('%016x' % bits(x) for x in xs)} {bits(f):016x} {margin:.3e}")

    N = 220
    # the kernel's angles: k·2π/n for n ≤ 64, the band angles, quarter turns
    angles = []
    for n in range(1, 65):
        for k in range(1, n + 1):
            angles.append(float(k) * 2.0 * 3.141592653589793 / float(n))
            angles.append(2.0 * 3.141592653589793 * float(k) / float(n))
    angles += [2.0943951023931953, -2.0943951023931957, 4.1887902047863905, 1.5707963267948966, 3.141592653589793,
               6.283185307179586, 0.5, 1e-9, 1e-5, 123.456, -0.7853981633974483]
    angles = sorted(set(angles))
    for a in angles:
        s, c = sin_cos(D(a))
        emit("sin", [a], s)
        emit("cos", [a], c)
        if c != 0:
            emit("tan", [a], s / c)
    for _ in range(N):
        a = rand(-10.0, 10.0)
        s, c = sin_cos(D(a))
        emit("sin", [a], s)
        emit("cos", [a], c)
        emit("tan", [a], s / c)
    for _ in range(N):
        a = rand(-1.0, 1.0)
        emit("asin", [a], atan2(a, (1 - D(a) * D(a)).sqrt()))
        emit("acos", [a], atan2((1 - D(a) * D(a)).sqrt(), a))
    for a in [0.5, -0.5, 0.9999, 1e-7, 0.25, -0.75]:
        emit("asin", [a], atan2(a, (1 - D(a) * D(a)).sqrt()))
        emit("acos", [a], atan2((1 - D(a) * D(a)).sqrt(), a))
    for _ in range(N):
        a = rand(-20.0, 20.0) if rng.random() < 0.5 else rand_log(1e-6, 1e6) * rng.choice([-1, 1])
        emit("atan", [a], atan2(a, 1))
    for _ in range(N):
        y, x = rand(-5.0, 5.0), rand(-5.0, 5.0)
        emit("atan2", [y, x], atan2(y, x))
    for _ in range(N):
        a = rand(-700.0, 700.0) if rng.random() < 0.3 else rand(-5.0, 5.0)
        emit("exp", [a], D(a).exp())
    for a in [1.0, -1.0, 0.5, 1e-10, -1e-10, 709.0, -745.0, 2.5, 100.0]:
        emit("exp", [a], D(a).exp())
    for _ in range(N):
        a = rand(-1000.0, 1000.0) if rng.random() < 0.3 else rand(-5.0, 5.0)
        emit("exp2", [a], (D(a) * LN2).exp())
    for a in [-3.0, 10.0, 0.5, 1023.0, -1074.0, 52.0]:
        emit("exp2", [a], (D(a) * LN2).exp())
    for _ in range(N):
        a = rand(-1e-3, 1e-3) if rng.random() < 0.4 else rand(-5.0, 5.0)
        emit("exp_m1", [a], D(a).exp() - 1)
    for _ in range(N):
        a = rand_log(1e-300, 1e300) if rng.random() < 0.4 else rand(0.5, 2.0)
        emit("ln", [a], D(a).ln())
        emit("log2", [a], D(a).ln() / LN2)
        emit("log10", [a], D(a).log10())
    for a in [2.0, 1024.0, 10.0, 1000.0, 1e23, 1e22, 0.1, 3.0, 1.0000000000000002, 0.9999999999999999, 5e-324, 2.2250738585072014e-308]:
        emit("ln", [a], D(a).ln())
        emit("log2", [a], D(a).ln() / LN2)
        emit("log10", [a], D(a).log10())
    for _ in range(N):
        a = rand(-1e-3, 1e-3) if rng.random() < 0.4 else rand(-0.999, 10.0)
        emit("ln_1p", [a], (1 + D(a)).ln())
    for _ in range(N):
        x = rand_log(1e-3, 1e3)
        b = rand_log(1.5, 100.0)
        emit("log", [x, b], D(x).ln() / D(b).ln())
    for _ in range(N):
        x = rand_log(1e-5, 1e5)
        y = rand(-20.0, 20.0) if rng.random() < 0.5 else float(rng.randint(-30, 30))
        emit("powf", [x, y], D(x) ** int(y) if y == int(y) else (D(y) * D(x).ln()).exp())
    for x, y in [(10.0, 3.0), (10.0, -3.0), (10.0, 23.0), (10.0, 22.0), (3.0, 34.0), (2.0, 1.5), (2.0, 0.5), (9.0, 0.5),
                 (-2.0, 3.0), (-2.0, 4.0), (0.5, -3.0), (1e-200, 2.0), (7.0, 70.0), (1.0000001, 1e6), (2.0, -1074.0)]:
        v = D(abs(x)) ** int(y) if y == int(y) else (D(y) * D(abs(x)).ln()).exp()
        if x < 0 and y == int(y) and int(y) % 2 == 1:
            v = -v
        emit("powf", [x, y], v)
    for _ in range(N):
        x = rand(-100.0, 100.0) if rng.random() < 0.6 else rand_log(1e-200, 1e200)
        y = rand(-100.0, 100.0) if rng.random() < 0.6 else rand_log(1e-200, 1e200)
        emit("hypot", [x, y], (D(x) * D(x) + D(y) * D(y)).sqrt())
    for x, y in [(3.0, 4.0), (5.0, 12.0), (1.0, 1.0), (1e308, 1e308), (1e-320, 1e-320), (0.0, 2.5), (1.0, 1e-200)]:
        emit("hypot", [x, y], (D(x) * D(x) + D(y) * D(y)).sqrt())
    for _ in range(N):
        a = rand(-100.0, 100.0) if rng.random() < 0.6 else rand_log(1e-300, 1e300) * rng.choice([-1, 1])
        v = abs(D(a)) ** (D(1) / 3)
        emit("cbrt", [a], v if a > 0 else -v)
    for a in [27.0, -27.0, 8.0, 1e-300, 1e300, 2.0, 0.001, 1000.0, 5e-324]:
        v = abs(D(a)) ** (D(1) / 3)
        emit("cbrt", [a], v if a > 0 else -v)
    for _ in range(N):
        a = rand(-1e-3, 1e-3) if rng.random() < 0.3 else rand(-30.0, 30.0)
        e = D(a).exp()
        emit("sinh", [a], (e - 1 / e) / 2)
        emit("cosh", [a], (e + 1 / e) / 2)
        emit("tanh", [a], (e - 1 / e) / (e + 1 / e))
    for a in [1.0, -1.0, 0.5, 41.0, 100.0, 710.0, 1e-5, 20.5, -19.0]:
        e = D(a).exp()
        emit("sinh", [a], (e - 1 / e) / 2)
        emit("cosh", [a], (e + 1 / e) / 2)
        emit("tanh", [a], (e - 1 / e) / (e + 1 / e))
    def asinh(a):
        # 1 + a loses a's digits at the working precision below 1e-15: the series there
        if abs(a) < 1e-15:
            z = D(a) * D(a)
            return D(a) * (1 - z / 6 + 3 * z * z / 40)
        return (D(a) + (D(a) * D(a) + 1).sqrt()).ln()

    for _ in range(N):
        a = rand(-1e-3, 1e-3) if rng.random() < 0.3 else rand(-30.0, 30.0) if rng.random() < 0.7 else rand_log(1e-100, 1e100)
        emit("asinh", [a], asinh(a))
    for a in [1.0, -1.0, 0.5, 1e200, 1e-5, 1e160, 1e-20, -3e-9, 3.7e-9]:
        emit("asinh", [a], asinh(a))
    for _ in range(N):
        a = 1 + rand(0.0, 1e-3) if rng.random() < 0.3 else rand(1.0, 30.0) if rng.random() < 0.7 else rand_log(1.0, 1e100)
        emit("acosh", [a], (D(a) + (D(a) * D(a) - 1).sqrt()).ln())
    for a in [1.5, 2.0, 1e200, 1.0000000001, 1e160, 1.0000000000000002]:
        emit("acosh", [a], (D(a) + (D(a) * D(a) - 1).sqrt()).ln())
    for _ in range(N):
        a = rand(-1e-3, 1e-3) if rng.random() < 0.3 else rand(-0.999999, 0.999999)
        emit("atanh", [a], ((1 + D(a)) / (1 - D(a))).ln() / 2)
    for a in [0.5, -0.5, 0.9999999999, 1e-5]:
        emit("atanh", [a], ((1 + D(a)) / (1 - D(a))).ln() / 2)
    print("\n".join(out))


if __name__ == "__main__":
    main()
