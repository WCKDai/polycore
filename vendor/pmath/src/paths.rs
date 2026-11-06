//! Each path on its own, for `polycore`'s `pmath_tests` (which compare them bit for bit and
//! check the fast paths' error bounds against the double-double values) and its benchmark.
//! The public functions take the fast path first and the double-double path where it does
//! not answer; the special values (zeros, infinities, NaNs) are decided before either.

pub use crate::fast::Approx;
use crate::{Dd, atan2_dd, cos_of_asin, fast, sin_cos_dd};

/// A double-double `hi + lo`; `hi + lo` rounded is what the double-double path returns.
pub type Pair = (f64, f64);

fn pair(d: Dd) -> Pair {
    (d.hi, d.lo)
}

/// The fast path of the sine and cosine (finite, non-zero `x`).
pub fn sin_cos_fast(x: f64) -> Option<(Approx, Approx)> {
    fast::sin_cos(x)
}

/// The fast path of the sine alone (finite, non-zero `x`).
pub fn sin_fast(x: f64) -> Option<Approx> {
    fast::sin(x)
}

/// The fast path of the cosine alone (finite, non-zero `x`).
pub fn cos_fast(x: f64) -> Option<Approx> {
    fast::cos(x)
}

/// The double-double path of the sine and cosine (finite, non-zero `x`).
pub fn sin_cos_slow(x: f64) -> (Pair, Pair) {
    let (s, c) = sin_cos_dd(x);
    (pair(s), pair(c))
}

/// The fast path of the tangent (finite, non-zero `x`).
pub fn tan_fast(x: f64) -> Option<Approx> {
    fast::tan(x)
}

/// The double-double path of the tangent (finite, non-zero `x`).
pub fn tan_slow(x: f64) -> Pair {
    let (s, c) = sin_cos_dd(x);
    pair(s.div(c))
}

/// The fast path of `atan2` (finite, non-zero `y` and `x`).
pub fn atan2_fast(y: f64, x: f64) -> Option<Approx> {
    fast::atan2(Dd::from_f64(y), Dd::from_f64(x))
}

/// The double-double path of `atan2` (finite, non-zero `y` and `x`).
pub fn atan2_slow(y: f64, x: f64) -> Pair {
    pair(atan2_dd(Dd::from_f64(y), Dd::from_f64(x)))
}

/// The fast path of the arc tangent (finite, non-zero `x`).
pub fn atan_fast(x: f64) -> Option<Approx> {
    fast::atan2(Dd::from_f64(x), Dd::ONE)
}

/// The double-double path of the arc tangent (finite, non-zero `x`).
pub fn atan_slow(x: f64) -> Pair {
    pair(atan2_dd(Dd::from_f64(x), Dd::ONE))
}

/// The fast path of the arc sine (`0 < |x| < 1`).
pub fn asin_fast(x: f64) -> Option<Approx> {
    fast::atan2(Dd::from_f64(x), fast::cos_of_asin(x))
}

/// The double-double path of the arc sine (`0 < |x| < 1`).
pub fn asin_slow(x: f64) -> Pair {
    pair(atan2_dd(Dd::from_f64(x), cos_of_asin(x)))
}

/// The fast path of the arc cosine (`0 < |x| < 1`).
pub fn acos_fast(x: f64) -> Option<Approx> {
    fast::atan2(fast::cos_of_asin(x), Dd::from_f64(x))
}

/// The double-double path of the arc cosine (`0 < |x| < 1`).
pub fn acos_slow(x: f64) -> Pair {
    pair(atan2_dd(cos_of_asin(x), Dd::from_f64(x)))
}

/// `√(1 − x²)` of the fast path, for the table and bound checks.
pub fn cos_of_asin_fast(x: f64) -> Pair {
    pair(fast::cos_of_asin(x))
}

/// `√(1 − x²)` of the double-double path.
pub fn cos_of_asin_slow(x: f64) -> Pair {
    pair(cos_of_asin(x))
}

/// The fast path's table entry `k`: the sine and cosine of `k·π/256` (k = 0..=64).
pub fn sin_cos_table(k: usize) -> (Pair, Pair) {
    crate::fast::SIN_COS_TABLE[k]
}

/// The sine and cosine of `k·π/256` by the double-double series (the angle as a double-double
/// product, within 2⁻¹⁰⁴): checks [`sin_cos_table`].
pub fn sin_cos_of_table_angle(k: usize) -> (Pair, Pair) {
    let (s, c) = crate::kernel(crate::PIO2.mul_f64(k as f64).scale2(-7));
    (pair(s), pair(c))
}

/// The fast path's table entry `k`: the arc tangent of `k/32` (k = 0..=32).
pub fn atan_table(k: usize) -> Pair {
    crate::fast::ATAN_TABLE[k]
}

/// The arc tangent of `k/32` by the double-double series: checks [`atan_table`].
pub fn atan_of_table_argument(k: usize) -> Pair {
    pair(crate::atan_unit(Dd::from_f64(k as f64 / 32.0)))
}
