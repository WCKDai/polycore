//! SolidCraft: sine, cosine and the angle between two vectors from IEEE 754 additions,
//! subtractions, multiplications, divisions and square roots alone, so that every platform
//! computes the same bits.
//!
//! `f64::sin`, `f64::cos` and `f64::atan2` are the platform's libm. glibc's are correctly rounded
//! for every angle the kernel builds with, but macOS returns `sin(2.0943951023931953)` (2π/3 to
//! the f64, the band of a full revolve) an ULP above the correctly rounded value, and that ULP
//! reached the revolved surfaces: Polykrom's robustness seed 36 built a different body there.
//! The functions here evaluate in double-double arithmetic (an unevaluated sum of two floats,
//! about 106 bits) and round once at the end. Their error is below 2⁻¹⁰⁰ of the result, so they
//! return the correctly rounded value unless the true one lies within that of a rounding
//! boundary, which `polycore`'s `portable_trig_tests` rule out for the kernel's angles with exact
//! integer arithmetic: 2π/3, 4π/3, π/2, π, 2π/n for n ≤ 64, the snapped rephase grid.
//!
//! The arithmetic must stay plain: no `f64::mul_add` (the platform's, and contracted or not by
//! hardware), and Rust does not contract `a * b + c` into a fused multiply-add on its own.
//! `Matrix4::from_axis_angle` and `Vector3::angle` on the revolve, sweep and rephase paths go
//! through [`rotation`] and [`angle`] below instead of cgmath's, which call the libm.

use crate::base::{InnerSpace, Matrix4, Rad, Vector3};

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
        if !(self.hi > 0.0) {
            return Dd::ZERO;
        }
        let s = self.hi.sqrt();
        let d = self.sub(two_prod(s, s));
        quick_two_sum(s, d.hi / (2.0 * s))
    }
}

/// π/2 in four pieces of 33 significant bits, so that `q * PIO2_n` is exact for `|q| < 2²⁰`
/// (Cody and Waite's reduction, as fdlibm's). Together they are π/2 to 2⁻¹³⁵.
const PIO2_1: f64 = 1.570_796_326_734_125_6; // 0x1.921fb544p+0
const PIO2_2: f64 = 6.077_100_506_303_966e-11; // 0x1.0b4611a6p-34
const PIO2_3: f64 = 2.022_266_248_711_166_5e-21; // 0x1.3198a2ep-69
const PIO2_4: f64 = 8.478_427_660_348_229e-32; // 0x1.b839a252p-104
/// π/2 and π as double-doubles.
const PIO2: Dd = Dd::new(std::f64::consts::FRAC_PI_2, 6.123_233_995_736_766e-17);
const PI: Dd = Dd::new(std::f64::consts::PI, 1.224_646_799_147_353_2e-16);

/// `1/k!` for k = 3..=31 as double-doubles (`hi` the nearest float, `lo` the nearest float to
/// the rest), from exact rational arithmetic; `portable_trig_tests` checks them against its own.
#[rustfmt::skip]
const INV_FACT: [(f64, f64); 29] = [
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

/// `1/(2k+1)` for k = 1..=19 as double-doubles, the series of the arc tangent.
#[rustfmt::skip]
const INV_ODD: [(f64, f64); 19] = [
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
];

/// sin r and cos r for `|r| ≤ π/4` (a little more does no harm) by their Taylor series to r³¹
/// and r³⁰, in double-double Horner form: the next terms are below 2⁻¹²⁹ of the result.
fn kernel(r: Dd) -> (Dd, Dd) {
    let z = r.mul(r);
    // sin r = r + r·z·(−1/3! + z·(1/5! − z·(1/7! − …)))
    let mut ps = Dd::ZERO;
    // cos r = 1 + z·(−1/2 + z·(1/4! − z·(1/6! − …)))
    let mut pc = Dd::ZERO;
    for k in (0..INV_FACT.len()).rev() {
        // INV_FACT[k] is 1/(k + 3)!: an odd k + 3 belongs to the sine (−1/3!, +1/5!, −1/7!, …),
        // an even one to the cosine (+1/4!, −1/6!, +1/8!, …).
        let (hi, lo) = INV_FACT[k];
        let positive = if k % 2 == 0 { (k / 2) % 2 == 1 } else { (k / 2) % 2 == 0 };
        let c = if positive { Dd::new(hi, lo) } else { Dd::new(-hi, -lo) };
        if k % 2 == 0 {
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

/// The sine and cosine of `x`, correctly rounded for the kernel's angles (see the module
/// documentation), from IEEE 754 arithmetic alone: the same bits on every platform. `x` is
/// reduced about the nearest multiple of π/2 (exactly for `|x| < 2²⁰·π/2`; beyond that the
/// result is still deterministic but no longer accurate), the series is evaluated in
/// double-double arithmetic and the quarter turns are put back. Not finite gives NaNs.
pub fn sin_cos(x: f64) -> (f64, f64) {
    if !x.is_finite() {
        return (f64::NAN, f64::NAN);
    }
    if x == 0.0 {
        return (x, 1.0); // keeps the sign of zero, as libm does
    }
    let q = (x * std::f64::consts::FRAC_2_PI).round();
    // x − q·π/2 in double-double: the first difference is exact (Sterbenz), the pieces' products
    // are exact, and the rest of π/2 beyond them is below 2⁻¹³⁵·q.
    let r = Dd::from_f64(x - q * PIO2_1)
        .sub(Dd::from_f64(q * PIO2_2))
        .sub(Dd::from_f64(q * PIO2_3))
        .sub(Dd::from_f64(q * PIO2_4));
    let (s, c) = kernel(r);
    match q.rem_euclid(4.0) {
        1.0 => (c.hi, -s.hi),
        2.0 => (-s.hi, -c.hi),
        3.0 => (-c.hi, s.hi),
        _ => (s.hi, c.hi),
    }
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
    for k in (0..INV_ODD.len()).rev() {
        let (hi, lo) = INV_ODD[k];
        let c = if k % 2 == 0 { Dd::new(-hi, -lo) } else { Dd::new(hi, lo) };
        p = c.add(z.mul(p));
    }
    let a = t.add(t.mul(z).mul(p));
    a.mul_f64(8.0)
}

/// The angle of the point `(x, y)` from the positive x axis in `(−π, π]`, as `f64::atan2` but
/// from IEEE 754 arithmetic alone and correctly rounded (see the module documentation); the
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
                    std::f64::consts::FRAC_PI_4
                } else {
                    3.0 * std::f64::consts::FRAC_PI_4
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
    let (ay, ax) = (y.abs(), x.abs());
    let a = if ay <= ax {
        let a = atan_unit(Dd::from_f64(ay).div(Dd::from_f64(ax)));
        if x > 0.0 {
            a
        } else {
            PI.sub(a)
        }
    } else {
        let a = atan_unit(Dd::from_f64(ax).div(Dd::from_f64(ay)));
        if x > 0.0 {
            PIO2.sub(a)
        } else {
            PIO2.add(a)
        }
    };
    a.hi.copysign(y)
}

/// The angle between `a` and `b` in `[0, π]`: cgmath's `InnerSpace::angle` (the arc tangent of
/// the cross product's length over the dot product) with [`atan2`] instead of the libm's.
pub fn angle(a: Vector3, b: Vector3) -> Rad<f64> {
    Rad(atan2(a.cross(b).magnitude(), a.dot(b)))
}

/// The rotation by `angle` about the unit vector `axis` through the origin: cgmath's
/// `Matrix4::from_axis_angle` (Rodrigues' formula in the same operation order) with the sine and
/// cosine of [`sin_cos`] instead of the libm's.
pub fn rotation(axis: Vector3, angle: Rad<f64>) -> Matrix4 {
    let (s, c) = sin_cos(angle.0);
    let k = 1.0 - c;
    let (x, y, z) = (axis.x, axis.y, axis.z);
    #[rustfmt::skip]
    let m = Matrix4::new(
        k * x * x + c,      k * x * y + s * z,  k * x * z - s * y,  0.0,
        k * x * y - s * z,  k * y * y + c,      k * y * z + s * x,  0.0,
        k * x * z + s * y,  k * y * z - s * x,  k * z * z + c,      0.0,
        0.0,                0.0,                0.0,                1.0,
    );
    m
}
