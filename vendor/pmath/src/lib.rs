//! PolyCore portable math: the transcendental functions of `f64` from IEEE 754 additions,
//! subtractions, multiplications, divisions and square roots alone, so that every platform
//! computes the same bits.
//!
//! `f64::sin`, `f64::exp`, `f64::powf` and the rest are the platform's libm, and the platforms
//! disagree: macOS returns `sin(2.0943951023931953)` (2π/3 to the f64, the band of a full
//! revolve) an ULP above the correctly rounded value, and that ULP reached revolved surfaces,
//! meshes and robustness records (PC-12). The functions here evaluate in double-double
//! arithmetic (an unevaluated sum of two floats, about 106 bits) and round once at the end.
//! Their error is below 2⁻⁹⁶ of the result (2⁻¹⁰⁰ for the trigonometric functions at the
//! kernel's angles), so they return the correctly rounded value unless the true one lies within
//! that of a rounding boundary, which `polycore`'s `portable_trig_tests` rule out for the
//! kernel's angles with exact integer arithmetic, and `pmath_tests` check for every function
//! against a 70-digit reference. Results that are exactly representable (`cbrt(27.0)`,
//! `hypot(3.0, 4.0)`, `exp2(-3.0)`, `powf(10.0, 3.0)`) come out exact.
//!
//! The arithmetic must stay plain: no `f64::mul_add` (the platform's, and contracted or not by
//! hardware), and Rust does not contract `a * b + c` into a fused multiply-add on its own. The
//! kernel's `clippy.toml` disallows the platform's methods everywhere; this crate is the one
//! place that computes them, and it calls none of them.
//!
//! Special values follow the libm (zeros, infinities, NaNs, the sign of zero). Subnormal results
//! are deterministic but may be an ULP off correctly rounded (the second rounding of the
//! scaling). The argument reduction of the trigonometric functions is exact below `|x| < 2³⁰`;
//! beyond that the result is still the same on every platform but no longer accurate.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::f64::consts::{FRAC_2_PI, FRAC_PI_2, FRAC_PI_4, PI as PI_F64};

/// An unevaluated sum `hi + lo` with `|lo| ≤ ulp(hi) / 2` (a double-double).
#[derive(Clone, Copy, Debug)]
struct Dd {
    hi: f64,
    lo: f64,
}

/// `a + b` as the rounded sum and the exact rounding error (Knuth's two-sum).
#[inline]
fn two_sum(a: f64, b: f64) -> Dd {
    let s = a + b;
    let bb = s - a;
    Dd { hi: s, lo: (a - (s - bb)) + (b - bb) }
}

/// `a + b` as the rounded sum and the exact rounding error, for `|a| ≥ |b|` (Dekker's fast sum).
#[inline]
fn quick_two_sum(a: f64, b: f64) -> Dd {
    let s = a + b;
    Dd { hi: s, lo: b - (s - a) }
}

/// `a` as the sum of two floats of 26 significant bits each (Veltkamp's split).
#[inline]
fn split(a: f64) -> (f64, f64) {
    let t = 134_217_729.0 * a; // 2²⁷ + 1
    let hi = t - (t - a);
    (hi, a - hi)
}

/// `a * b` as the rounded product and the exact rounding error (Dekker's product, no fused
/// multiply-add).
#[inline]
fn two_prod(a: f64, b: f64) -> Dd {
    let p = a * b;
    let (ah, al) = split(a);
    let (bh, bl) = split(b);
    Dd { hi: p, lo: ((ah * bh - p) + ah * bl + al * bh) + al * bl }
}

impl Dd {
    const ZERO: Dd = Dd { hi: 0.0, lo: 0.0 };
    const ONE: Dd = Dd { hi: 1.0, lo: 0.0 };

    #[inline]
    const fn new(hi: f64, lo: f64) -> Dd {
        Dd { hi, lo }
    }

    #[inline]
    fn from_f64(x: f64) -> Dd {
        Dd { hi: x, lo: 0.0 }
    }

    #[inline]
    fn neg(self) -> Dd {
        Dd { hi: -self.hi, lo: -self.lo }
    }

    #[inline]
    fn add(self, o: Dd) -> Dd {
        let s = two_sum(self.hi, o.hi);
        let t = two_sum(self.lo, o.lo);
        let s = quick_two_sum(s.hi, s.lo + t.hi);
        quick_two_sum(s.hi, s.lo + t.lo)
    }

    #[inline]
    fn sub(self, o: Dd) -> Dd {
        self.add(o.neg())
    }

    #[inline]
    fn mul(self, o: Dd) -> Dd {
        let p = two_prod(self.hi, o.hi);
        quick_two_sum(p.hi, p.lo + (self.hi * o.lo + self.lo * o.hi))
    }

    #[inline]
    fn mul_f64(self, b: f64) -> Dd {
        let p = two_prod(self.hi, b);
        quick_two_sum(p.hi, p.lo + self.lo * b)
    }

    /// `self / o`, three quotient digits (about 2⁻¹⁰⁴ relative error).
    #[inline]
    fn div(self, o: Dd) -> Dd {
        let q1 = self.hi / o.hi;
        let r = self.sub(o.mul_f64(q1));
        let q2 = r.hi / o.hi;
        let r = r.sub(o.mul_f64(q2));
        let q3 = r.hi / o.hi;
        quick_two_sum(q1, q2).add(Dd::from_f64(q3))
    }

    /// `√self` for `self > 0` (one Newton step from the float square root).
    #[inline]
    fn sqrt(self) -> Dd {
        if self.hi.is_nan() || self.hi <= 0.0 {
            return Dd::ZERO;
        }
        let s = self.hi.sqrt();
        let d = self.sub(two_prod(s, s));
        quick_two_sum(s, d.hi / (2.0 * s))
    }

    /// `self · 2^k`, exact (both parts scaled).
    #[inline]
    fn scale2(self, k: i64) -> Dd {
        Dd { hi: ldexp(self.hi, k), lo: ldexp(self.lo, k) }
    }

    /// The nearest float to `self` (ties to even): `hi` once normalized.
    #[inline]
    fn round(self) -> f64 {
        self.hi + self.lo
    }
}

/// `2^k` as a float for `-1022 ≤ k ≤ 1023`.
#[inline]
fn pow2(k: i64) -> f64 {
    f64::from_bits(((k + 1023) as u64) << 52)
}

/// `x · 2^k` by at most two multiplications by powers of two (exact unless the result is
/// subnormal or overflows).
fn ldexp(x: f64, k: i64) -> f64 {
    if (-1022..=1023).contains(&k) {
        x * pow2(k)
    } else if k > 1023 {
        let k2 = (k - 1023).min(1023);
        x * pow2(1023) * pow2(k2)
    } else {
        let k2 = (k + 1022).max(-1022);
        x * pow2(-1022) * pow2(k2)
    }
}

/// π/2 in four pieces of 33 significant bits, so that `q * PIO2_n` is exact for `|q| < 2²⁰`
/// (Cody and Waite's reduction, as fdlibm's). Together they are π/2 to 2⁻¹³⁵.
const PIO2_1: f64 = 1.570_796_326_734_125_6; // 0x1.921fb544p+0
const PIO2_2: f64 = 6.077_100_506_303_966e-11; // 0x1.0b4611a6p-34
const PIO2_3: f64 = 2.022_266_248_711_166_5e-21; // 0x1.3198a2ep-69
const PIO2_4: f64 = 8.478_427_660_348_229e-32; // 0x1.b839a252p-104
/// π/2 and π as double-doubles.
const PIO2: Dd = Dd::new(FRAC_PI_2, 6.123_233_995_736_766e-17);
const PI: Dd = Dd::new(PI_F64, 1.224_646_799_147_353_2e-16);
/// ln 2 in three pieces (42, 42 and 53 significant bits), so that `k * LN2_n` is exact for
/// `|k| < 2¹¹`; together they are ln 2 to 2⁻¹³⁷. And ln 2 and ln 10 as double-doubles.
const LN2_1: f64 = 0.693_147_180_559_890_3; // 0x1.62e42fefa38p-1
const LN2_2: f64 = 5.497_923_018_708_502_4e-14; // 0x1.ef35793c768p-45
const LN2_3: f64 = -1.312_469_841_778_525_5e-27; // -0x1.9ff0342542fc3p-90
const LN2: Dd = Dd::new(std::f64::consts::LN_2, 2.319_046_813_846_299_6e-17);
const LN10: Dd = Dd::new(std::f64::consts::LN_10, -2.170_756_223_382_249_4e-16);

/// `1/k!` for k = 2..=31 as double-doubles (`hi` the nearest float, `lo` the nearest float to
/// the rest), from exact rational arithmetic; `portable_trig_tests` checks them against its own.
#[rustfmt::skip]
const INV_FACT: [(f64, f64); 30] = [
    (0.5, 0.0),
    (0.166_666_666_666_666_66, 9.251_858_538_542_97e-18),
    (0.041_666_666_666_666_664, 2.312_964_634_635_742_7e-18),
    (0.008_333_333_333_333_333, 1.156_482_317_317_871_4e-19),
    (0.001_388_888_888_888_889, -5.300_543_954_373_577e-20),
    (0.000_198_412_698_412_698_4, 1.720_955_829_342_070_5e-22),
    (2.480_158_730_158_73e-5, 2.151_194_786_677_588_2e-23),
    (2.755_731_922_398_589_3e-6, -1.858_393_274_046_472e-22),
    (2.755_731_922_398_589e-7, 2.376_771_462_225_029_7e-23),
    (2.505_210_838_544_172e-8, -1.448_814_070_935_912e-24),
    (2.087_675_698_786_81e-9, -1.207_345_059_113_26e-25),
    (1.605_904_383_682_161_3e-10, 1.258_529_458_875_209_8e-26),
    (1.147_074_559_772_972_5e-11, 2.065_551_275_283_074_5e-28),
    (7.647_163_731_819_816e-13, 7.038_728_777_334_53e-30),
    (4.779_477_332_387_385e-14, 4.399_205_485_834_081e-31),
    (2.811_457_254_345_520_6e-15, 1.650_884_273_086_143_3e-31),
    (1.561_920_696_858_622_5e-16, 1.191_067_966_027_375_4e-32),
    (8.220_635_246_624_33e-18, 2.214_189_411_960_426_5e-34),
    (4.110_317_623_312_165e-19, 1.441_297_337_865_952_7e-36),
    (1.957_294_106_339_126_3e-20, -1.364_350_383_008_790_8e-36),
    (8.896_791_392_450_574e-22, -7.911_402_614_872_376e-38),
    (3.868_170_170_630_684e-23, -8.843_177_655_482_344e-40),
    (1.611_737_571_096_118_4e-24, -3.684_657_356_450_976_6e-41),
    (6.446_950_284_384_474e-26, -1.933_040_423_370_346_5e-42),
    (2.479_596_263_224_797_6e-27, -1.295_373_096_476_522_9e-43),
    (9.183_689_863_795_546e-29, 1.430_315_039_678_732_2e-45),
    (3.279_889_237_069_838e-30, 1.511_754_274_402_987_9e-46),
    (1.130_996_288_644_771_6e-31, 1.049_801_541_295_950_6e-47),
    (3.769_987_628_815_905_4e-33, 2.587_034_783_275_032_4e-49),
    (1.216_125_041_553_518e-34, 5.586_290_567_888_806e-51),
];

/// `1/(2k+1)` for k = 1..=22 as double-doubles, the series of the arc tangent and the logarithm.
#[rustfmt::skip]
const INV_ODD: [(f64, f64); 22] = [
    (0.333_333_333_333_333_3, 1.850_371_707_708_594e-17),
    (0.2, -1.110_223_024_625_156_6e-17),
    (0.142_857_142_857_142_85, 7.930_164_461_608_26e-18),
    (0.111_111_111_111_111_1, 6.167_905_692_361_980_4e-18),
    (0.090_909_090_909_090_91, -2.523_234_146_875_356e-18),
    (0.076_923_076_923_076_93, -4.270_088_556_250_602e-18),
    (0.066_666_666_666_666_67, 9.251_858_538_542_971e-19),
    (0.058_823_529_411_764_705, 8.163_404_592_832_033e-19),
    (0.052_631_578_947_368_42, 2.921_639_538_487_254e-18),
    (0.047_619_047_619_047_616, 2.643_388_153_869_42e-18),
    (0.043_478_260_869_565_216, 1.206_764_157_201_257e-18),
    (0.04, -8.326_672_684_688_674e-19),
    (0.037_037_037_037_037_035, 2.055_968_564_120_66e-18),
    (0.034_482_758_620_689_655, 4.785_444_071_660_157e-19),
    (0.032_258_064_516_129_03, 8.953_411_488_912_552e-19),
    (0.030_303_030_303_030_304, -8.410_780_489_584_519e-19),
    (0.028_571_428_571_428_57, 8.921_435_019_309_293e-19),
    (0.027_027_027_027_027_03, -1.500_301_384_628_59e-18),
    (0.025_641_025_641_025_64, 8.896_017_825_522_087e-19),
    (0.024_390_243_902_439_025, -8.462_065_736_472_23e-19),
    (0.023_255_813_953_488_372, 3.227_392_513_445_222_5e-19),
    (0.022_222_222_222_222_223, -8.480_870_326_997_723e-19),
];

// ---------------------------------------------------------------------------------------------
// Trigonometry

/// sin r and cos r for `|r| ≤ π/4` (a little more does no harm) by their Taylor series to r³¹
/// and r³⁰, in double-double Horner form: the next terms are below 2⁻¹²⁹ of the result.
fn kernel(r: Dd) -> (Dd, Dd) {
    let z = r.mul(r);
    // sin r = r + r·z·(−1/3! + z·(1/5! − z·(1/7! − …)))
    let mut ps = Dd::ZERO;
    // cos r = 1 + z·(−1/2 + z·(1/4! − z·(1/6! − …)))
    let mut pc = Dd::ZERO;
    for k in (1..INV_FACT.len()).rev() {
        // INV_FACT[k] is 1/(k + 2)!: an odd k + 2 belongs to the sine (−1/3!, +1/5!, −1/7!, …),
        // an even one to the cosine (+1/4!, −1/6!, +1/8!, …).
        let j = k - 1;
        let (hi, lo) = INV_FACT[k];
        let positive = if j % 2 == 0 { (j / 2) % 2 == 1 } else { (j / 2) % 2 == 0 };
        let c = if positive { Dd::new(hi, lo) } else { Dd::new(-hi, -lo) };
        if j % 2 == 0 {
            ps = c.add(z.mul(ps));
        } else {
            pc = c.add(z.mul(pc));
        }
    }
    pc = Dd::new(-0.5, 0.0).add(z.mul(pc));
    let s = r.add(r.mul(z).mul(ps));
    let c = Dd::ONE.add(z.mul(pc));
    (s, c)
}

/// The sine and cosine of a finite, non-zero `x` as double-doubles: `x` reduced about the
/// nearest multiple of π/2 (the pieces' products are exact as double-doubles), the series
/// evaluated and the quarter turns put back.
fn sin_cos_dd(x: f64) -> (Dd, Dd) {
    let q = (x * FRAC_2_PI).round();
    // x − q·π/2 in double-double: the first difference is exact (Sterbenz), the pieces' products
    // are exact, and the rest of π/2 beyond them is below 2⁻¹³⁵·q.
    let r = Dd::from_f64(x).sub(two_prod(q, PIO2_1)).sub(two_prod(q, PIO2_2)).sub(two_prod(q, PIO2_3)).sub(two_prod(q, PIO2_4));
    let (s, c) = kernel(r);
    match q.rem_euclid(4.0) {
        1.0 => (c, s.neg()),
        2.0 => (s.neg(), c.neg()),
        3.0 => (c.neg(), s),
        _ => (s, c),
    }
}

/// The sine and cosine of `x`, correctly rounded for the kernel's angles (see the crate
/// documentation), from IEEE 754 arithmetic alone: the same bits on every platform. Not finite
/// gives NaNs.
pub fn sin_cos(x: f64) -> (f64, f64) {
    if !x.is_finite() {
        return (f64::NAN, f64::NAN);
    }
    if x == 0.0 {
        return (x, 1.0); // keeps the sign of zero, as libm does
    }
    let (s, c) = sin_cos_dd(x);
    (s.round(), c.round())
}

/// The sine of `x` (see [`sin_cos`]).
pub fn sin(x: f64) -> f64 {
    sin_cos(x).0
}

/// The cosine of `x` (see [`sin_cos`]).
pub fn cos(x: f64) -> f64 {
    sin_cos(x).1
}

/// The tangent of `x`: the sine over the cosine, both in double-double, rounded once.
pub fn tan(x: f64) -> f64 {
    if !x.is_finite() {
        return f64::NAN;
    }
    if x == 0.0 {
        return x;
    }
    let (s, c) = sin_cos_dd(x);
    s.div(c).round()
}

/// atan t for `0 ≤ t ≤ 1` in double-double: three halvings `atan t = 2·atan(t / (1 + √(1 + t²)))`
/// bring t under 0.1, then the series to t³⁹ (the next term is below 2⁻¹³⁰).
fn atan_unit(t: Dd) -> Dd {
    let mut t = t;
    for _ in 0..3 {
        let root = Dd::ONE.add(t.mul(t)).sqrt();
        t = t.div(Dd::ONE.add(root));
    }
    let z = t.mul(t);
    let mut p = Dd::ZERO;
    for k in (0..19).rev() {
        let (hi, lo) = INV_ODD[k];
        let c = if k % 2 == 0 { Dd::new(-hi, -lo) } else { Dd::new(hi, lo) };
        p = c.add(z.mul(p));
    }
    let a = t.add(t.mul(z).mul(p));
    a.mul_f64(8.0)
}

/// The angle of the point `(x, y)` from the positive x axis in `(−π, π]`, for finite `x` and `y`
/// that are not both zero, as a double-double.
fn atan2_dd(y: Dd, x: Dd) -> Dd {
    let neg_y = y.hi < 0.0 || (y.hi == 0.0 && y.hi.is_sign_negative());
    if y.hi == 0.0 {
        let a = if x.hi.is_sign_positive() { Dd::ZERO } else { PI };
        return if neg_y { a.neg() } else { a };
    }
    if x.hi == 0.0 {
        return if neg_y { PIO2.neg() } else { PIO2 };
    }
    let ay = if neg_y { y.neg() } else { y };
    let ax = if x.hi < 0.0 { x.neg() } else { x };
    let a = if ay.hi <= ax.hi {
        let a = atan_unit(ay.div(ax));
        if x.hi > 0.0 { a } else { PI.sub(a) }
    } else {
        let a = atan_unit(ax.div(ay));
        if x.hi > 0.0 { PIO2.sub(a) } else { PIO2.add(a) }
    };
    if neg_y { a.neg() } else { a }
}

/// The angle of the point `(x, y)` from the positive x axis in `(−π, π]`, as `f64::atan2` but
/// from IEEE 754 arithmetic alone and correctly rounded (see the crate documentation); the
/// same special cases as the libm for zeros, infinities and NaNs.
pub fn atan2(y: f64, x: f64) -> f64 {
    if y.is_nan() || x.is_nan() {
        return f64::NAN;
    }
    let pi = PI.hi;
    let pio2 = PIO2.hi;
    if y == 0.0 {
        // ±0 on the positive side, ±π on the negative one (x = −0 counts as negative).
        return if x.is_sign_positive() { y } else { pi.copysign(y) };
    }
    if x == 0.0 {
        return pio2.copysign(y);
    }
    if x.is_infinite() || y.is_infinite() {
        let a = match (x.is_infinite(), y.is_infinite()) {
            (true, true) => {
                if x > 0.0 {
                    FRAC_PI_4
                } else {
                    3.0 * FRAC_PI_4
                }
            }
            (true, false) => {
                if x > 0.0 {
                    0.0
                } else {
                    pi
                }
            }
            _ => pio2,
        };
        return a.copysign(y);
    }
    atan2_dd(Dd::from_f64(y), Dd::from_f64(x)).round()
}

/// The arc tangent of `x` in `[−π/2, π/2]` (see [`atan2`]).
pub fn atan(x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    if x == 0.0 {
        return x;
    }
    if x.is_infinite() {
        return PIO2.hi.copysign(x);
    }
    atan2_dd(Dd::from_f64(x), Dd::ONE).round()
}

/// `√(1 − x²)` as a double-double for `|x| ≤ 1` (the difference is exact as a double-double).
fn cos_of_asin(x: f64) -> Dd {
    Dd::ONE.sub(two_prod(x, x)).sqrt()
}

/// The arc sine of `x` in `[−π/2, π/2]`: NaN outside `[−1, 1]`.
pub fn asin(x: f64) -> f64 {
    if x.is_nan() || x.abs() > 1.0 {
        return f64::NAN;
    }
    if x.abs() == 1.0 {
        return PIO2.hi.copysign(x);
    }
    if x.abs() < 3.725_290_298_461_914e-9 {
        return x; // 2⁻²⁸: the cubic term is below half an ULP
    }
    atan2_dd(Dd::from_f64(x), cos_of_asin(x)).round()
}

/// The arc cosine of `x` in `[0, π]`: NaN outside `[−1, 1]`.
pub fn acos(x: f64) -> f64 {
    if x.is_nan() || x.abs() > 1.0 {
        return f64::NAN;
    }
    if x == 1.0 {
        return 0.0;
    }
    if x == -1.0 {
        return PI.hi;
    }
    atan2_dd(cos_of_asin(x), Dd::from_f64(x)).round()
}

// ---------------------------------------------------------------------------------------------
// Exponentials and logarithms

/// `e^z` for a finite double-double `z` as `m · 2^k` with `m` a double-double near 1: `z`
/// reduced about the nearest multiple of ln 2 (the pieces' products are exact for `|k| < 2¹¹`)
/// and the Taylor series to r²⁶ (the next term is below 2⁻¹²⁰ for `|r| ≤ ln 2 / 2`).
fn exp_reduced(z: Dd) -> (Dd, i64) {
    let k = (z.hi / LN2.hi).round().clamp(-2047.0, 2047.0);
    let r = if k == 0.0 { z } else { z.sub(two_prod(k, LN2_1)).sub(two_prod(k, LN2_2)).sub(two_prod(k, LN2_3)) };
    // e^r = 1 + r·(1 + r·(1/2! + r·(1/3! + …)))
    let mut p = Dd::ZERO;
    for k in (0..25).rev() {
        let (hi, lo) = INV_FACT[k];
        p = Dd::new(hi, lo).add(r.mul(p));
    }
    let e = Dd::ONE.add(r.mul(Dd::ONE.add(r.mul(p))));
    (e, k as i64)
}

/// `e^z` for a finite double-double `z`, rounded (with the overflow and underflow of the libm).
fn exp_dd(z: Dd) -> f64 {
    if z.hi > 709.8 {
        return f64::INFINITY;
    }
    if z.hi < -745.2 {
        return 0.0;
    }
    let (m, k) = exp_reduced(z);
    ldexp(m.round(), k)
}

/// `e^x − 1` for a finite `x` as a double-double: the series without its leading 1 for
/// `|x| ≤ ln 2 / 2`, `e^x − 1` in double-double beyond.
fn expm1_dd(x: f64) -> Dd {
    if x.abs() <= 0.35 {
        let r = Dd::from_f64(x);
        let mut p = Dd::ZERO;
        for k in (0..25).rev() {
            let (hi, lo) = INV_FACT[k];
            p = Dd::new(hi, lo).add(r.mul(p));
        }
        return r.mul(Dd::ONE.add(r.mul(p)));
    }
    if x < -745.2 {
        return Dd::new(-1.0, 0.0);
    }
    let (m, k) = exp_reduced(Dd::from_f64(x));
    m.scale2(k).sub(Dd::ONE)
}

/// `e^x`: the same bits on every platform (see the crate documentation).
pub fn exp(x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    if x == f64::INFINITY {
        return f64::INFINITY;
    }
    if x == f64::NEG_INFINITY {
        return 0.0;
    }
    exp_dd(Dd::from_f64(x))
}

/// `2^x`: exact for integer `x`.
pub fn exp2(x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    if x > 1024.5 {
        return f64::INFINITY;
    }
    if x < -1075.5 {
        return 0.0;
    }
    let k = x.round();
    let r = x - k; // exact
    let (m, k2) = exp_reduced(Dd::from_f64(r).mul(LN2));
    ldexp(m.round(), k as i64 + k2)
}

/// `e^x − 1`, accurate for small `x`.
pub fn exp_m1(x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    if x == f64::INFINITY {
        return f64::INFINITY;
    }
    if x == f64::NEG_INFINITY {
        return -1.0;
    }
    if x > 709.8 {
        return f64::INFINITY;
    }
    if x == 0.0 {
        return x;
    }
    expm1_dd(x).round()
}

/// `x` as `m · 2^e` with `m` in `[√½, √2)`, exactly (`x` positive, finite).
fn frexp_centered(x: Dd) -> (Dd, i64) {
    let mut x = x;
    let mut e = 0i64;
    if x.hi < 2.225_073_858_507_201_4e-308 {
        x = x.scale2(54); // a subnormal, brought into the normal range first
        e -= 54;
    }
    let bits = x.hi.to_bits();
    let ex = ((bits >> 52) & 0x7ff) as i64 - 1023;
    let mut m = x.scale2(-ex);
    e += ex;
    if m.hi > std::f64::consts::SQRT_2 {
        m = m.scale2(-1);
        e += 1;
    }
    (m, e)
}

/// ln m for a double-double `m` in `[√½, √2]` by `2·atanh((m − 1)/(m + 1))`, the series to
/// s⁴⁵ (the next term is below 2⁻¹¹⁰ for `|s| ≤ 0.1716`).
fn ln_centered(m: Dd) -> Dd {
    let s = m.sub(Dd::ONE).div(m.add(Dd::ONE));
    let z = s.mul(s);
    let mut p = Dd::ZERO;
    for k in (0..INV_ODD.len()).rev() {
        let (hi, lo) = INV_ODD[k];
        p = Dd::new(hi, lo).add(z.mul(p));
    }
    s.add(s.mul(z).mul(p)).mul_f64(2.0)
}

/// ln x for a positive, finite double-double `x`: `e·ln 2 + ln m`.
fn ln_dd(x: Dd) -> Dd {
    let (m, e) = frexp_centered(x);
    let l = ln_centered(m);
    if e == 0 { l } else { LN2.mul_f64(e as f64).add(l) }
}

/// The natural logarithm: NaN below zero, −∞ at zero.
pub fn ln(x: f64) -> f64 {
    if x.is_nan() || x < 0.0 {
        return f64::NAN;
    }
    if x == 0.0 {
        return f64::NEG_INFINITY;
    }
    if x == f64::INFINITY {
        return f64::INFINITY;
    }
    if x == 1.0 {
        return 0.0;
    }
    ln_dd(Dd::from_f64(x)).round()
}

/// `ln(1 + x)`, accurate for small `x`.
pub fn ln_1p(x: f64) -> f64 {
    if x.is_nan() || x < -1.0 {
        return f64::NAN;
    }
    if x == -1.0 {
        return f64::NEG_INFINITY;
    }
    if x == f64::INFINITY {
        return f64::INFINITY;
    }
    if x == 0.0 {
        return x;
    }
    ln_dd(two_sum(1.0, x)).round()
}

/// The base-2 logarithm: exact at powers of two.
pub fn log2(x: f64) -> f64 {
    if x.is_nan() || x < 0.0 {
        return f64::NAN;
    }
    if x == 0.0 {
        return f64::NEG_INFINITY;
    }
    if x == f64::INFINITY {
        return f64::INFINITY;
    }
    let (m, e) = frexp_centered(Dd::from_f64(x));
    if m.hi == 1.0 {
        return e as f64;
    }
    Dd::from_f64(e as f64).add(ln_centered(m).div(LN2)).round()
}

/// The base-10 logarithm.
pub fn log10(x: f64) -> f64 {
    if x.is_nan() || x < 0.0 {
        return f64::NAN;
    }
    if x == 0.0 {
        return f64::NEG_INFINITY;
    }
    if x == f64::INFINITY {
        return f64::INFINITY;
    }
    if x == 1.0 {
        return 0.0;
    }
    ln_dd(Dd::from_f64(x)).div(LN10).round()
}

/// The logarithm of `x` in base `base`: `ln x / ln base`, both in double-double.
pub fn log(x: f64, base: f64) -> f64 {
    if x.is_nan() || base.is_nan() || x < 0.0 || base < 0.0 {
        return f64::NAN;
    }
    if x == 1.0 && base.is_finite() && base != 0.0 {
        return 0.0;
    }
    if base == 1.0 || x == 0.0 || base == 0.0 || !x.is_finite() || !base.is_finite() {
        // The libm's `log(x) / log(base)` with infinities and zeros in both.
        let (lx, lb) = (ln(x), ln(base));
        return lx / lb;
    }
    ln_dd(Dd::from_f64(x)).div(ln_dd(Dd::from_f64(base))).round()
}

/// Whether `y` is an odd integer.
fn is_odd_integer(y: f64) -> bool {
    y.is_finite() && y == y.trunc() && y.abs() < 9_007_199_254_740_992.0 && (y as i64) % 2 != 0
}

/// `x^y`: the special cases of the libm, exact integer powers of modest size by repeated
/// squaring in double-double (so `powf(10.0, 3.0)` is 1000 and halfway cases round to even),
/// `e^(y·ln x)` in double-double otherwise.
pub fn powf(x: f64, y: f64) -> f64 {
    if y == 0.0 || x == 1.0 {
        return 1.0;
    }
    if x.is_nan() || y.is_nan() {
        return f64::NAN;
    }
    if y.is_infinite() {
        let ax = x.abs();
        return if ax == 1.0 {
            1.0
        } else if (ax > 1.0) == (y > 0.0) {
            f64::INFINITY
        } else {
            0.0
        };
    }
    let odd = is_odd_integer(y);
    if x.is_infinite() {
        let big = if y > 0.0 { f64::INFINITY } else { 0.0 };
        return if x < 0.0 && odd { -big } else { big };
    }
    if x == 0.0 {
        let big = if y < 0.0 { f64::INFINITY } else { 0.0 };
        return if odd { big.copysign(x) } else { big };
    }
    if x < 0.0 && y != y.trunc() {
        return f64::NAN;
    }
    let ax = x.abs();
    let sign = if x < 0.0 && odd { -1.0 } else { 1.0 };
    if y == y.trunc()
        && y.abs() <= 64.0
        && let Some(p) = int_pow(ax, y.abs() as u32)
    {
        let p = if y < 0.0 { Dd::ONE.div(p) } else { p };
        return sign * p.round();
    }
    let z = ln_dd(Dd::from_f64(ax)).mul_f64(y);
    sign * exp_dd(z)
}

/// `a^n` by repeated squaring in double-double, exact while the values fit; `None` where an
/// intermediate leaves the range where double-double arithmetic is meaningful.
fn int_pow(a: f64, n: u32) -> Option<Dd> {
    let mut result = Dd::ONE;
    let mut base = Dd::from_f64(a);
    let mut n = n;
    loop {
        if n & 1 == 1 {
            result = result.mul(base);
        }
        n >>= 1;
        if n == 0 {
            break;
        }
        base = base.mul(base);
        if !(base.hi.abs() > 1e-290 && base.hi.abs() < 1e290) {
            return None;
        }
    }
    if result.hi.abs() > 1e-290 && result.hi.abs() < 1e290 { Some(result) } else { None }
}

// ---------------------------------------------------------------------------------------------
// Roots and norms

/// `√(x² + y²)` without undue overflow or underflow: the sum of the exact squares in
/// double-double, its square root rounded once (so `hypot(3.0, 4.0)` is 5).
pub fn hypot(x: f64, y: f64) -> f64 {
    if x.is_infinite() || y.is_infinite() {
        return f64::INFINITY;
    }
    if x.is_nan() || y.is_nan() {
        return f64::NAN;
    }
    let (mut a, mut b) = (x.abs(), y.abs());
    if a < b {
        std::mem::swap(&mut a, &mut b);
    }
    if a == 0.0 {
        return 0.0;
    }
    if b == 0.0 {
        return a;
    }
    let shift = if a > 1e150 {
        600
    } else if a < 1e-150 {
        -600
    } else {
        0
    };
    let (a, b) = (ldexp(a, -shift), ldexp(b, -shift));
    let s = two_prod(a, a).add(two_prod(b, b));
    ldexp(s.sqrt().round(), shift)
}

/// The cube root, with the sign of `x`: Newton's method in double-double from a float
/// approximation (so `cbrt(27.0)` is 3).
pub fn cbrt(x: f64) -> f64 {
    if !x.is_finite() || x == 0.0 {
        return x;
    }
    let a = x.abs();
    let (mut a, mut unscale) = (a, 0i64);
    if a < 2.225_073_858_507_201_4e-308 {
        a = ldexp(a, 54);
        unscale = -18;
    }
    // a = m · 2^(3e) with m in [1, 8).
    let ex = ((a.to_bits() >> 52) & 0x7ff) as i64 - 1023;
    let e3 = ex.div_euclid(3);
    let m = ldexp(a, -3 * e3);
    let mut y = (m + 2.0) / 3.0;
    for _ in 0..8 {
        y = (2.0 * y + m / (y * y)) / 3.0;
    }
    let mut yd = Dd::from_f64(y);
    let md = Dd::from_f64(m);
    for _ in 0..2 {
        let y2 = yd.mul(yd);
        let y3 = y2.mul(yd);
        yd = yd.sub(y3.sub(md).div(y2.mul_f64(3.0)));
    }
    ldexp(yd.round(), e3 + unscale).copysign(x)
}

// ---------------------------------------------------------------------------------------------
// Hyperbolic functions

/// The hyperbolic sine.
pub fn sinh(x: f64) -> f64 {
    if !x.is_finite() || x == 0.0 {
        return x;
    }
    let a = x.abs();
    if a < 3.725_290_298_461_914e-9 {
        return x; // 2⁻²⁸: the cubic term is below half an ULP
    }
    if a >= 40.0 {
        // e^−a is below 2⁻¹¹⁵ of e^a: sinh a = e^a / 2.
        if a > 710.5 {
            return f64::INFINITY.copysign(x);
        }
        let (m, k) = exp_reduced(Dd::from_f64(a));
        return ldexp(m.round(), k - 1).copysign(x);
    }
    let e = expm1_dd(a);
    let s = e.add(e.div(e.add(Dd::ONE))).mul_f64(0.5);
    s.round().copysign(x)
}

/// The hyperbolic cosine.
pub fn cosh(x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    let a = x.abs();
    if a == f64::INFINITY {
        return f64::INFINITY;
    }
    if a < 1.818_989_403_545_856_4e-12 {
        return 1.0; // 2⁻³⁹: the quadratic term is below half an ULP
    }
    if a >= 40.0 {
        if a > 710.5 {
            return f64::INFINITY;
        }
        let (m, k) = exp_reduced(Dd::from_f64(a));
        return ldexp(m.round(), k - 1);
    }
    let (m, k) = exp_reduced(Dd::from_f64(a));
    let e = m.scale2(k);
    e.add(Dd::ONE.div(e)).mul_f64(0.5).round()
}

/// The hyperbolic tangent.
pub fn tanh(x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    if x == 0.0 {
        return x;
    }
    let a = x.abs();
    if a < 3.725_290_298_461_914e-9 {
        return x;
    }
    if a >= 20.0 {
        return 1.0f64.copysign(x); // 1 − tanh a is below 2⁻⁵⁶
    }
    let e = expm1_dd(2.0 * a);
    e.div(e.add(Dd::new(2.0, 0.0))).round().copysign(x)
}

/// The inverse hyperbolic sine: `ln(x + √(x² + 1))`, as `ln(1 + (x + x²/(1 + √(x² + 1))))` so
/// that small arguments keep their precision.
pub fn asinh(x: f64) -> f64 {
    if !x.is_finite() || x == 0.0 {
        return x;
    }
    let a = x.abs();
    if a < 3.725_290_298_461_914e-9 {
        return x;
    }
    if a > 1e150 {
        // √(x² + 1) is x to 2⁻¹⁰⁰⁰: ln(2x).
        return ln_dd(Dd::from_f64(a)).add(LN2).round().copysign(x);
    }
    let r = two_prod(a, a).add(Dd::ONE).sqrt();
    let u = two_prod(a, a).div(Dd::ONE.add(r));
    let w = Dd::from_f64(a).add(u);
    ln_dd(Dd::ONE.add(w)).round().copysign(x)
}

/// The inverse hyperbolic cosine: `ln(x + √(x² − 1))` for `x ≥ 1`, NaN below.
pub fn acosh(x: f64) -> f64 {
    if x.is_nan() || x < 1.0 {
        return f64::NAN;
    }
    if x == 1.0 {
        return 0.0;
    }
    if x == f64::INFINITY {
        return f64::INFINITY;
    }
    if x > 1e150 {
        return ln_dd(Dd::from_f64(x)).add(LN2).round();
    }
    // ln(1 + (t + √(x² − 1))) with t = x − 1: the sum is exact as a double-double.
    let t = two_sum(x, -1.0);
    let r = two_prod(x, x).sub(Dd::ONE).sqrt();
    ln_dd(Dd::ONE.add(t.add(r))).round()
}

/// The inverse hyperbolic tangent: `½·ln((1 + x)/(1 − x))` for `|x| < 1`, ±∞ at ±1, NaN beyond.
pub fn atanh(x: f64) -> f64 {
    if x.is_nan() || x.abs() > 1.0 {
        return f64::NAN;
    }
    if x.abs() == 1.0 {
        return f64::INFINITY.copysign(x);
    }
    if x == 0.0 {
        return x;
    }
    let a = x.abs();
    if a < 3.725_290_298_461_914e-9 {
        return x;
    }
    // ½·ln(1 + 2a/(1 − a)): the difference is exact as a double-double.
    let w = Dd::from_f64(2.0 * a).div(two_sum(1.0, -a));
    ln_dd(Dd::ONE.add(w)).mul_f64(0.5).round().copysign(x)
}
