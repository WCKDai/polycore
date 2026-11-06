#!/usr/bin/env python3
"""Generates the tables of `pmath`'s fast paths (vendor/pmath/src/fast.rs) from a 70-digit
decimal reference (Python's `decimal`; never the platform's libm): the sine and cosine of
k·π/256 for k = 0..=64 and the arc tangent of k/32 for k = 0..=32, each as a double-double (the nearest
float and the nearest float to the rest), and the bit counts of the pieces of π/2 (which the
argument reduction scales to π/256).

    python3 -I pmath_tables.py

Prints the Rust constants; `pmath_tests::fast_path_tables_match_the_double_double_series` checks the ones
in the crate against the same reference arithmetic (the slow double-double path, which
`pmath_tests` proves against this script's `pmath_reference.txt`).
"""
import struct
from decimal import Decimal as D, getcontext

getcontext().prec = 70
EPS = D(10) ** -68


def bits(f):
    return struct.unpack("<Q", struct.pack("<d", f))[0]


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


def sin_cos(r):
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
    return s, c


def atan(t):
    # halve thrice, then the series
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


def dd(v):
    """`v` as the nearest float and the nearest float to the remainder."""
    hi = float(v)
    lo = float(v - D(hi))
    return hi, lo


def rust(v):
    hi, lo = dd(v)
    return f"({hi!r}, {lo!r})"


def main():
    print("/// sin(k·π/256), cos(k·π/256) for k = 0..=64 as (hi, lo) double-doubles (pmath_tables.py).")
    print("#[rustfmt::skip]")
    print("const SIN_COS_TABLE: [((f64, f64), (f64, f64)); 65] = [")
    for k in range(65):
        s, c = sin_cos(PI * k / 256)
        print(f"    ({rust(s)}, {rust(c)}),")
    print("];")
    print("/// atan(k/32) for k = 0..=32 as (hi, lo) double-doubles (pmath_tables.py).")
    print("#[rustfmt::skip]")
    print("const ATAN_TABLE: [(f64, f64); 33] = [")
    for k in range(33):
        print(f"    {rust(atan(D(k) / 32))},")
    print("];")
    # The pieces of π/2 (33 significant bits each) and what remains beyond them.
    pieces = [1.5707963267341256, 6.077100506303966e-11, 2.0222662487111665e-21, 8.478427660348229e-32]
    rest = PI / 2 - sum(D(p) for p in pieces)
    print(f"// π/2 − (PIO2_1 + PIO2_2 + PIO2_3 + PIO2_4) = {rest:.3e} (below 2^{float(abs(rest).ln() / D(2).ln()):.1f})")
    for p in pieces:
        b = bits(p)
        mant = (b & ((1 << 52) - 1)) | (1 << 52)
        trailing = (mant & -mant).bit_length() - 1
        print(f"// {p!r}: {53 - trailing} significant bits")
    print(f"// 256/π = {rust(256 / PI)}")


if __name__ == "__main__":
    main()
