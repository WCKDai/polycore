//! The revolve, sweep and rephase paths build their rotations and arcs with
//! `truck_geometry::portable_trig` (vendored): sine, cosine and arc tangent from IEEE 754
//! arithmetic alone, the same bits on every platform. A full revolve is built in bands of a third
//! of a turn, each turned by cgmath's `from_axis_angle`, that is, the platform's `sin` and `cos`
//! of 2π/3; macOS rounds that sine an ULP off glibc's, and `rephase_revolved` then turns the band
//! back, so the ULP survived into the surface (`rephase_tests::revolved_ring_mesh_bounds_are_exact`
//! failed on macOS arm64). These tests prove the replacement returns the correctly rounded value
//! at every angle the kernel builds with, against exact integer arithmetic ([`Fx`], a 384-bit
//! fixed-point Taylor series): never against the platform's libm, which is not correctly rounded
//! (glibc's `sin(2π/39)` is an ULP above the nearest float to the true value, and it misrounds
//! about 0.3 % of all angles; see the report of PC-12.rephase-r3).

use std::cmp::Ordering;
use std::f64::consts::{FRAC_PI_2, PI, TAU};
use truck_modeling::portable_trig::{angle, atan2, rotation, sin_cos};
use truck_modeling::{self as mt, InnerSpace, Rad, builder};

/// A signed fixed-point number of 7 limbs of 64 bits, 6 of them (384 bits) below the point:
/// the exact arithmetic the sine, cosine and arc tangent are checked against. Only the
/// operations a Taylor series in a dyadic argument needs: add, subtract, multiply and divide by
/// a 64-bit integer, shift. Every operation truncates below 2⁻³⁸⁴, far below any rounding
/// boundary the checks decide.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Fx {
    neg: bool,
    mag: [u64; 7],
}

impl Fx {
    const FRAC_BITS: i64 = 384;

    const ZERO: Fx = Fx { neg: false, mag: [0; 7] };

    fn from_u64(n: u64) -> Fx {
        let mut mag = [0; 7];
        mag[6] = n;
        Fx { neg: false, mag }
    }

    fn is_zero(&self) -> bool {
        self.mag.iter().all(|&l| l == 0)
    }

    fn shl(self, n: u32) -> Fx {
        let mut out = Fx { neg: self.neg, mag: [0; 7] };
        let (limbs, bits) = ((n / 64) as usize, n % 64);
        for i in (0..7).rev() {
            let src = i as isize - limbs as isize;
            if src < 0 {
                continue;
            }
            let src = src as usize;
            let mut v = self.mag[src] << bits;
            if bits > 0 && src > 0 {
                v |= self.mag[src - 1] >> (64 - bits);
            }
            out.mag[i] = v;
        }
        // Nothing may fall off the top.
        let lost = (0..7).rev().take(limbs).any(|i| self.mag[i] != 0) || (bits > 0 && limbs < 7 && self.mag[6 - limbs] >> (64 - bits) != 0);
        assert!(!lost, "shl overflow");
        out
    }

    fn shr(self, n: u32) -> Fx {
        let mut out = Fx { neg: self.neg, mag: [0; 7] };
        let (limbs, bits) = ((n / 64) as usize, n % 64);
        for i in 0..7 {
            let src = i + limbs;
            if src >= 7 {
                break;
            }
            let mut v = self.mag[src] >> bits;
            if bits > 0 && src + 1 < 7 {
                v |= self.mag[src + 1] << (64 - bits);
            }
            out.mag[i] = v;
        }
        out
    }

    fn mul_u64(self, k: u64) -> Fx {
        let mut out = Fx { neg: self.neg, mag: [0; 7] };
        let mut carry = 0u128;
        for i in 0..7 {
            let v = self.mag[i] as u128 * k as u128 + carry;
            out.mag[i] = v as u64;
            carry = v >> 64;
        }
        assert_eq!(carry, 0, "mul_u64 overflow");
        out
    }

    fn div_u64(self, k: u64) -> Fx {
        let mut out = Fx { neg: self.neg, mag: [0; 7] };
        let mut rem = 0u128;
        for i in (0..7).rev() {
            let v = (rem << 64) | self.mag[i] as u128;
            out.mag[i] = (v / k as u128) as u64;
            rem = v % k as u128;
        }
        out
    }

    fn cmp_mag(&self, o: &Fx) -> Ordering {
        for i in (0..7).rev() {
            match self.mag[i].cmp(&o.mag[i]) {
                Ordering::Equal => continue,
                other => return other,
            }
        }
        Ordering::Equal
    }

    fn add_mag(a: &[u64; 7], b: &[u64; 7]) -> [u64; 7] {
        let mut out = [0; 7];
        let mut carry = 0u128;
        for i in 0..7 {
            let v = a[i] as u128 + b[i] as u128 + carry;
            out[i] = v as u64;
            carry = v >> 64;
        }
        assert_eq!(carry, 0, "add overflow");
        out
    }

    /// `a - b` for `a ≥ b`.
    fn sub_mag(a: &[u64; 7], b: &[u64; 7]) -> [u64; 7] {
        let mut out = [0; 7];
        let mut borrow = 0i128;
        for i in 0..7 {
            let v = a[i] as i128 - b[i] as i128 - borrow;
            out[i] = v.rem_euclid(1 << 64) as u64;
            borrow = i128::from(v < 0);
        }
        out
    }

    fn add(self, o: Fx) -> Fx {
        if self.neg == o.neg {
            return Fx { neg: self.neg, mag: Fx::add_mag(&self.mag, &o.mag) };
        }
        match self.cmp_mag(&o) {
            Ordering::Equal => Fx::ZERO,
            Ordering::Greater => Fx { neg: self.neg, mag: Fx::sub_mag(&self.mag, &o.mag) },
            Ordering::Less => Fx { neg: o.neg, mag: Fx::sub_mag(&o.mag, &self.mag) },
        }
    }

    fn sub(self, o: Fx) -> Fx {
        self.add(Fx { neg: !o.neg, ..o })
    }

    /// Times a finite float, exactly.
    fn mul_f64(self, x: f64) -> Fx {
        let (neg, m, e) = dyadic(x);
        let v = self.mul_u64(m);
        let v = if e >= 0 { v.shl(e as u32) } else { v.shr((-e) as u32) };
        Fx { neg: (self.neg != neg) && !v.is_zero(), ..v }
    }

    fn sign(&self) -> Ordering {
        if self.is_zero() {
            Ordering::Equal
        } else if self.neg {
            Ordering::Less
        } else {
            Ordering::Greater
        }
    }

    /// The 64 bits whose lowest is bit `pos` of the magnitude (bit 0 is 2⁻³⁸⁴; below it is zero).
    fn window(&self, pos: i64) -> u64 {
        let mut v = 0u64;
        for b in 0..64 {
            let i = pos + b;
            if (0..448).contains(&i) && (self.mag[(i / 64) as usize] >> (i % 64)) & 1 == 1 {
                v |= 1 << b;
            }
        }
        v
    }

    fn any_bit_below(&self, pos: i64) -> bool {
        (0..pos.min(448)).any(|i| (self.mag[(i / 64) as usize] >> (i % 64)) & 1 == 1)
    }

    /// The nearest float (ties to even) and the distance of the value from the nearest rounding
    /// boundary (the midpoint between two floats) in units of the result's ULP: how far from a
    /// hard case the rounding is.
    fn to_f64_nearest(self) -> (f64, f64) {
        let Some(h) = (0..448).rev().find(|&i| (self.mag[(i / 64) as usize] >> (i % 64)) & 1 == 1) else {
            return (0.0, 1.0);
        };
        let mut m = self.window(h - 52) & ((1 << 53) - 1);
        let rem = self.window(h - 116);
        let rest = self.any_bit_below(h - 116);
        let half = 1u64 << 63;
        let mut lead = h;
        if rem > half || (rem == half && (rest || m & 1 == 1)) {
            m += 1;
            if m == 1 << 53 {
                m = 1 << 52;
                lead += 1;
            }
        }
        let margin = (rem as f64 / 2f64.powi(64) - 0.5).abs();
        let exp = lead - Fx::FRAC_BITS + 1023;
        assert!((1..2047).contains(&exp), "exponent {exp} out of the normal range");
        let bits = ((self.neg as u64) << 63) | ((exp as u64) << 52) | (m & ((1 << 52) - 1));
        (f64::from_bits(bits), margin)
    }
}

/// A finite float as `±m · 2^e` with `m < 2⁵³`.
fn dyadic(x: f64) -> (bool, u64, i32) {
    assert!(x.is_finite());
    let bits = x.to_bits();
    let neg = bits >> 63 == 1;
    let exp = ((bits >> 52) & 0x7ff) as i32;
    let frac = bits & ((1 << 52) - 1);
    if exp == 0 { (neg, frac, -1074) } else { (neg, frac | (1 << 52), exp - 1075) }
}

/// sin x and cos x for `x = ±m · 2^e`, `|x| < 8`, by their Taylor series in exact arithmetic
/// (each term is the one before times x over k: a multiplication by `m`, a shift and a division
/// by `k`, every one exact or truncated below 2⁻³⁸⁴), summed until the terms vanish.
fn sin_cos_exact_dyadic(neg: bool, m: u64, e: i32) -> (Fx, Fx) {
    assert!((m as f64) * 2f64.powi(e) < 8.0, "|x| < 8");
    let (mut s, mut c) = (Fx::ZERO, Fx::ZERO);
    let mut term = Fx::from_u64(1);
    let mut k = 0u64;
    while !term.is_zero() {
        match k % 4 {
            0 => c = c.add(term),
            1 => s = s.add(term),
            2 => c = c.sub(term),
            _ => s = s.sub(term),
        }
        k += 1;
        assert!(k < 1000, "the series did not vanish");
        term = term.mul_u64(m);
        term = if e >= 0 { term.shl(e as u32) } else { term.shr((-e) as u32) };
        term = term.div_u64(k);
        term.neg = term.neg != neg;
    }
    (s, c)
}

/// The correctly rounded sine and cosine of the float `x`, with the smaller of the two margins
/// from a rounding boundary (in ULPs).
fn sin_cos_rounded(x: f64) -> (f64, f64, f64) {
    let (neg, m, e) = dyadic(x);
    let (s, c) = sin_cos_exact_dyadic(neg, m, e);
    let (s, ms) = s.to_f64_nearest();
    let (c, mc) = c.to_f64_nearest();
    (s, c, ms.min(mc))
}

fn ulps(a: f64, b: f64) -> u64 {
    (a.to_bits() as i64 - b.to_bits() as i64).unsigned_abs()
}

/// `rephase_revolved`'s turn for a face whose middle the search found at `v`
/// (`rephase_tests::phi_of`).
fn rephase_angle(v: f64) -> f64 {
    crate::body::snap_angle(v.rem_euclid(TAU) - PI)
}

/// Every angle the revolve, sweep, rephase and pattern paths take a sine or cosine of, as the
/// kernel computes them: a full revolve's bands (2π/3, and 4π/3 as the second band's turn), the
/// quarter and half turns, a partial revolve's bands (the angle over 2 or 3), `revolve_on_axis`'s
/// pieces from `FULL_TURN_START` and the arcs' transits halfway, the three recorded rephase turns,
/// and a circular pattern's instances k·2π/n for n up to 64, in both spellings of the product.
fn kernel_angles() -> Vec<(String, f64)> {
    let mut out: Vec<(String, f64)> = vec![
        ("2π/3 as whole_rsweep: PI * 2 / 3".into(), (Rad(PI) * 2.0 / 3.0).0),
        ("2π/3 as TAU / 3".into(), TAU / 3.0),
        ("4π/3 as 4 * PI / 3".into(), 4.0 * PI / 3.0),
        ("4π/3 as 2 * (TAU / 3)".into(), 2.0 * (TAU / 3.0)),
        ("4π/3 as TAU * 2 / 3".into(), TAU * 2.0 / 3.0),
        ("π/2".into(), FRAC_PI_2),
        ("-π/2".into(), -FRAC_PI_2),
        ("π".into(), PI),
        ("-π".into(), -PI),
        ("2π".into(), TAU),
        ("-2π".into(), -TAU),
        ("rephase, seed 36".into(), rephase_angle(-2.0943951023931957 + PI)),
        ("rephase, v = 1".into(), rephase_angle(1.0)),
        ("rephase, v = 4.5".into(), rephase_angle(4.5)),
    ];
    const FULL_TURN_START: f64 = 0.4142;
    for j in 0..3 {
        out.push((format!("revolve_on_axis piece {j}"), FULL_TURN_START + TAU / 3.0 * j as f64));
        out.push((format!("revolve_on_axis transit {j}"), FULL_TURN_START + TAU / 3.0 * (j as f64 + 0.5)));
    }
    for n in 1..=64u32 {
        out.push((format!("2π/{n} over 2"), TAU / n as f64 / 2.0));
        out.push((format!("2π/{n} over 3"), TAU / n as f64 / 3.0));
        for k in 1..=n {
            out.push((format!("{k} * (2π/{n})"), k as f64 * (TAU / n as f64)));
            out.push((format!("2π * {k} / {n}"), TAU * k as f64 / n as f64));
        }
    }
    out
}

/// At every angle of [`kernel_angles`], `sin_cos` returns the correctly rounded sine and cosine
/// (the nearest float to the true value), and none of them is a hard case: the true value is at
/// least 2⁻²⁰ ULP from a rounding boundary, where the evaluation's error is below 2⁻⁴⁰ ULP.
#[test]
fn sin_cos_is_correctly_rounded_at_the_kernels_angles() {
    let mut worst = (f64::INFINITY, String::new());
    for (name, x) in kernel_angles() {
        let (s, c) = sin_cos(x);
        let (es, ec, margin) = sin_cos_rounded(x);
        assert_eq!(
            (s.to_bits(), c.to_bits()),
            (es.to_bits(), ec.to_bits()),
            "{name}: sin, cos of {x:?} are {s:?} {c:?}, correctly rounded {es:?} {ec:?}"
        );
        assert!(margin > 2f64.powi(-20), "{name}: {x:?} is a hard case, {margin:e} ULP from a rounding boundary");
        if margin < worst.0 {
            worst = (margin, name);
        }
    }
    println!("nearest to a rounding boundary: {} at {:e} ULP", worst.1, worst.0);
}

/// The bits of the angles the contract names, recorded from the exact arithmetic (and agreeing
/// with an independent computation in exact rationals), so a change in either computation shows.
#[test]
fn sin_cos_bits_of_the_named_angles() {
    for (x, sin_bits, cos_bits) in [
        (TAU / 3.0, 0x3febb67ae8584cab, 0xbfdffffffffffffc),
        (4.0 * PI / 3.0, 0xbfebb67ae8584ca8, 0xbfe0000000000004),
        (FRAC_PI_2, 0x3ff0000000000000, 0x3c91a62633145c07),
        (PI, 0x3ca1a62633145c07, 0xbff0000000000000),
        // glibc returns 0x3fc4885b5a98c648 for this sine, an ULP above the correctly rounded value.
        (TAU / 39.0, 0x3fc4885b5a98c647, 0x3fef95ea6eb137ad),
        (rephase_angle(-2.0943951023931957 + PI), 0xbfebb67b58b330ca, 0xbfdffffe7aca5fd2),
        (rephase_angle(1.0), 0xbfeaed54308486cf, 0xbfe14a28a2e8dd84),
        (rephase_angle(4.5), 0x3fef47ed18e71d41, 0x3fcafb5e005c5056),
    ] {
        let (s, c) = sin_cos(x);
        let (es, ec, _) = sin_cos_rounded(x);
        assert_eq!((s.to_bits(), c.to_bits()), (sin_bits, cos_bits), "sin_cos({x:?}) = {s:?} {c:?}");
        assert_eq!((es.to_bits(), ec.to_bits()), (sin_bits, cos_bits), "exact sin, cos of {x:?} = {es:?} {ec:?}");
    }
}

/// The snapped rephase angles are the multiples of 2⁻²⁰ in (−π, π]: 6 588 397 of them. Every
/// 977th, and the hundred at each end, is checked against the exact arithmetic here; the whole
/// grid by [`sin_cos_is_correctly_rounded_on_the_whole_rephase_grid`] (ignored: minutes).
#[test]
fn sin_cos_is_correctly_rounded_on_the_rephase_grid() {
    let kmax = (PI * 1048576.0) as i64;
    let mut n = 0;
    let mut worst = f64::INFINITY;
    for k in (-kmax..=kmax).step_by(977).chain(-kmax..-kmax + 100).chain(kmax - 100..=kmax) {
        let x = crate::body::snap_angle(k as f64 / 1048576.0);
        let (s, c) = sin_cos(x);
        let (es, ec, margin) = sin_cos_rounded(x);
        assert_eq!(
            (s.to_bits(), c.to_bits()),
            (es.to_bits(), ec.to_bits()),
            "sin, cos of grid point {k} ({x:?}) are {s:?} {c:?}, correctly rounded {es:?} {ec:?}"
        );
        worst = worst.min(margin);
        n += 1;
    }
    println!("{n} grid points, nearest to a rounding boundary {worst:e} ULP");
    assert!(n > 6900);
}

/// The whole snapped rephase grid (see [`sin_cos_is_correctly_rounded_on_the_rephase_grid`]).
#[test]
#[ignore = "every grid point against the exact series: minutes; run with --ignored"]
fn sin_cos_is_correctly_rounded_on_the_whole_rephase_grid() {
    let kmax = (PI * 1048576.0) as i64;
    let mut worst = (f64::INFINITY, 0i64);
    for k in -kmax..=kmax {
        let x = crate::body::snap_angle(k as f64 / 1048576.0);
        let (s, c) = sin_cos(x);
        let (es, ec, margin) = sin_cos_rounded(x);
        assert_eq!(
            (s.to_bits(), c.to_bits()),
            (es.to_bits(), ec.to_bits()),
            "sin, cos of grid point {k} ({x:?}) are {s:?} {c:?}, correctly rounded {es:?} {ec:?}"
        );
        if margin < worst.0 {
            worst = (margin, k);
        }
    }
    println!("{} grid points, nearest to a rounding boundary {:e} ULP at k = {}", 2 * kmax + 1, worst.0, worst.1);
}

/// Zero keeps its sign (as the libm's), and what is not finite gives no number.
#[test]
fn sin_cos_special_values() {
    assert_eq!(sin_cos(0.0), (0.0, 1.0));
    assert!(sin_cos(-0.0).0.is_sign_negative() && sin_cos(-0.0).1 == 1.0);
    assert!(sin_cos(f64::NAN).0.is_nan() && sin_cos(f64::INFINITY).1.is_nan());
    // Tiny angles: the sine is the angle, the cosine one.
    assert_eq!(sin_cos(1e-20), (1e-20, 1.0));
    assert_eq!(sin_cos(-1e-300), (-1e-300, 1.0));
}

/// The columns of a matrix as bit patterns.
fn bits(m: mt::Matrix4) -> [[u64; 4]; 4] {
    let m: [[f64; 4]; 4] = m.into();
    m.map(|col| col.map(f64::to_bits))
}

/// cgmath's `from_axis_angle` layout from a given sine and cosine, as bits.
fn rodrigues_bits(axis: mt::Vector3, s: f64, c: f64) -> [[u64; 4]; 4] {
    let k = 1.0 - c;
    let (x, y, z) = (axis.x, axis.y, axis.z);
    [
        [k * x * x + c, k * x * y + s * z, k * x * z - s * y, 0.0],
        [k * x * y - s * z, k * y * y + c, k * y * z + s * x, 0.0],
        [k * x * z + s * y, k * y * z - s * x, k * z * z + c, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
    .map(|col| col.map(f64::to_bits))
}

/// The band rotations of a revolve, 2π/3 (a full turn's bands, `whole_rsweep`) and 4π/3 (the
/// second band's turn), have exactly the correctly rounded sine and cosine as their entries about
/// z (and the formula's entries from them about a tilted axis), and `builder::rsweep` places a
/// full turn's bands with them: the second band is the first's rotation applied twice.
#[test]
fn band_rotation_has_the_correctly_rounded_entries() {
    let z = mt::Vector3::new(0.0, 0.0, 1.0);
    let tilted = mt::Vector3::new(1.0, 2.0, 3.0).normalize();
    for x in [(Rad(PI) * 2.0 / 3.0).0, 4.0 * PI / 3.0, 2.0 * (TAU / 3.0)] {
        let (s, c, _) = sin_cos_rounded(x);
        let about_z = bits(rotation(z, Rad(x)));
        let one = 1.0f64.to_bits();
        assert_eq!(
            about_z,
            [[c.to_bits(), s.to_bits(), 0, 0], [(-s).to_bits(), c.to_bits(), 0, 0], [0, 0, ((1.0 - c) + c).to_bits(), 0], [0, 0, 0, one]],
            "about z by {x:?}"
        );
        assert_eq!(bits(rotation(tilted, Rad(x))), rodrigues_bits(tilted, s, c), "about (1, 2, 3)/√14 by {x:?}");
    }
    // A full turn of a vertex at radius 25: three arcs whose vertices are the band rotation
    // applied once and twice (what `whole_rsweep` maps the profile by).
    let p = mt::Point3::new(25.0, 0.0, 27.5);
    let wire: mt::Wire = builder::rsweep(&builder::vertex(p), mt::Point3::new(0.0, 0.0, 0.0), z, Rad(TAU));
    let band = rotation(z, Rad(PI) * 2.0 / 3.0);
    let (s3, c3, _) = sin_cos_rounded(TAU / 3.0);
    assert_eq!(bits(band), rodrigues_bits(z, s3, c3));
    let q = mt::Transform::transform_point(&band, p);
    let r = mt::Transform::transform_point(&band, q);
    let points: Vec<[u64; 3]> = wire.vertex_iter().map(|v| [v.point().x.to_bits(), v.point().y.to_bits(), v.point().z.to_bits()]).collect();
    let want: Vec<[u64; 3]> = [p, q, r].iter().map(|v| [v.x.to_bits(), v.y.to_bits(), v.z.to_bits()]).collect();
    assert_eq!(points, want, "the bands' vertices");
    assert_eq!((q.x, q.y), (25.0 * c3, 25.0 * s3));
    // A partial turn of 1 rad goes in two bands of 0.5 rad.
    let wire: mt::Wire = builder::rsweep(&builder::vertex(p), mt::Point3::new(0.0, 0.0, 0.0), z, Rad(1.0));
    let half = rotation(z, Rad(0.5));
    let (s, c, _) = sin_cos_rounded(0.5);
    assert_eq!(bits(half), rodrigues_bits(z, s, c));
    let q = mt::Transform::transform_point(&half, p);
    let r = mt::Transform::transform_point(&half, q);
    let points: Vec<[u64; 3]> = wire.vertex_iter().map(|v| [v.point().x.to_bits(), v.point().y.to_bits(), v.point().z.to_bits()]).collect();
    let want: Vec<[u64; 3]> = [p, q, r].iter().map(|v| [v.x.to_bits(), v.y.to_bits(), v.z.to_bits()]).collect();
    assert_eq!(points, want, "the partial bands' vertices");
    // The faces a revolved line makes carry the line turned by the band rotation.
    let line = builder::line(&builder::vertex(p), &builder::vertex(mt::Point3::new(2.5, 0.0, 27.5)));
    let shell: mt::Shell = builder::rsweep(&line, mt::Point3::new(0.0, 0.0, 0.0), z, Rad(TAU));
    let starts: Vec<u64> = shell
        .face_iter()
        .map(|f| {
            let mt::Surface::RevolutedCurve(proc_) = f.surface() else { panic!("a revolved face") };
            mt::ParametricCurve::subs(proc_.entity().entity_curve(), 0.0).y.to_bits()
        })
        .collect();
    assert_eq!(
        starts,
        vec![0.0f64.to_bits(), (25.0 * s3).to_bits(), mt::Transform::transform_point(&band, mt::Transform::transform_point(&band, p)).y.to_bits()]
    );
}

/// `atan2(y, x)` is correctly rounded when the true angle lies between the midpoints to the
/// neighbouring floats: at the midpoint m, `y·cos m − x·sin m` (which is `|v|·sin(θ − m)`) has
/// the sign of θ − m. Returns the smaller distance from a midpoint, relative to the angle's ULP.
fn assert_atan2_rounded(y: f64, x: f64) -> f64 {
    let a = atan2(y, x);
    assert!(a != 0.0 && a.is_finite(), "{y:?} {x:?} -> {a:?}");
    let mut margin = f64::INFINITY;
    for neighbour in [f64::from_bits(a.to_bits() - 1), f64::from_bits(a.to_bits() + 1)] {
        let above = neighbour > a;
        // Adjacent floats of one sign: the midpoint is the one nearer zero plus half its ULP.
        let lower = if a.abs() < neighbour.abs() { a } else { neighbour };
        let (neg, m, e) = dyadic(lower);
        let (sm, cm) = sin_cos_exact_dyadic(neg, 2 * m + 1, e - 1);
        let g = cm.mul_f64(y).sub(sm.mul_f64(x));
        // θ above the midpoint below `a`, below the one above it.
        let want = if above { Ordering::Less } else { Ordering::Greater };
        assert_eq!(g.sign(), want, "atan2({y:?}, {x:?}) = {a:?} is not the nearest float (midpoint toward {neighbour:?})");
        // |g| / |v| is |sin(θ − m)|; in ULPs of a.
        let v = (x * x + y * y).sqrt();
        let (gf, _) = g.to_f64_nearest();
        let ulp = f64::from_bits(a.abs().to_bits() + 1) - a.abs();
        margin = margin.min(gf.abs() / v / ulp);
    }
    margin
}

/// `atan2` is correctly rounded over the plane (a pseudo-random sample) and at the angles the
/// three-point arcs of a revolved profile and a sphere's patches take: a quarter, a third and
/// two thirds of a turn, and the odd angles `revolve_on_axis` turns by.
#[test]
fn atan2_is_correctly_rounded() {
    let mut worst = f64::INFINITY;
    let mut state = 0x2545_f491_4f6c_dd1du64;
    let mut next = || {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((state >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0) * 30.0
    };
    for _ in 0..3000 {
        let (y, x) = (next(), next());
        worst = worst.min(assert_atan2_rounded(y, x));
    }
    // The angle at the transit of an arc from p0 to p1, as `circle_arc_by_three_points` takes it:
    // atan2(|v0 × v1|, v0 · v1) with v0 = p0 − t, v1 = p1 − t.
    let z = mt::Vector3::new(0.0, 0.0, 1.0);
    for (radius, start, turn) in [
        (25.0, 0.0, TAU / 3.0),
        (2.5, 0.4142, TAU / 3.0),
        (10.0, 0.4142 + TAU / 3.0, TAU / 3.0),
        (1.0, 0.0, FRAC_PI_2),
        (7.5, 0.3, 2.0),
        (15.0, 1.0, PI - 1e-3),
    ] {
        let at = |a: f64| mt::Transform::transform_point(&rotation(z, Rad(a)), mt::Point3::new(radius, 0.0, 5.0));
        let (p0, p1, t) = (at(start), at(start + turn), at(start + turn / 2.0));
        let (v0, v1) = (p0 - t, p1 - t);
        let a = angle(v0, v1);
        assert_eq!(a.0.to_bits(), atan2(v0.cross(v1).magnitude(), v0.dot(v1)).to_bits());
        worst = worst.min(assert_atan2_rounded(v0.cross(v1).magnitude(), v0.dot(v1)));
    }
    println!("nearest to a rounding boundary: {worst:e} ULP");
    assert!(worst > 2f64.powi(-30));
}

/// The special values, as the libm's: zeros keep their sign on the positive side and are ±π on
/// the negative one, infinities take the quarter turns.
#[test]
fn atan2_special_values() {
    assert_eq!(atan2(0.0, 1.0).to_bits(), 0.0f64.to_bits());
    assert_eq!(atan2(-0.0, 1.0).to_bits(), (-0.0f64).to_bits());
    assert_eq!(atan2(0.0, -1.0), PI);
    assert_eq!(atan2(-0.0, -1.0), -PI);
    assert_eq!(atan2(0.0, -0.0), PI);
    assert_eq!(atan2(-0.0, 0.0).to_bits(), (-0.0f64).to_bits());
    assert_eq!(atan2(2.0, 0.0), FRAC_PI_2);
    assert_eq!(atan2(-2.0, -0.0), -FRAC_PI_2);
    assert_eq!(atan2(1.0, 1.0), std::f64::consts::FRAC_PI_4);
    assert_eq!(atan2(f64::INFINITY, f64::INFINITY), std::f64::consts::FRAC_PI_4);
    assert_eq!(atan2(-f64::INFINITY, -f64::INFINITY), -3.0 * std::f64::consts::FRAC_PI_4);
    assert_eq!(atan2(1.0, f64::INFINITY), 0.0);
    assert_eq!(atan2(-1.0, f64::NEG_INFINITY), -PI);
    assert_eq!(atan2(f64::INFINITY, -5.0), FRAC_PI_2);
    assert!(atan2(f64::NAN, 1.0).is_nan() && atan2(1.0, f64::NAN).is_nan());
}

/// A three-point arc (the arcs that bound a revolved profile's faces, a sphere's patches and
/// `revolve_on_axis`'s pieces) turns through twice the supplement of the correctly rounded angle
/// at its transit, and the unit arc it places has the correctly rounded sine and cosine of the
/// half and the whole of that angle as its control points (the same construction from them, knot
/// insertions included, bit for bit): the same arcs everywhere.
#[test]
fn circle_arc_is_built_from_correctly_rounded_values() {
    let z = mt::Vector3::new(0.0, 0.0, 1.0);
    for (radius, start, turn) in [(25.0, 0.4142, TAU / 3.0), (2.5, 0.4142 + TAU / 3.0, TAU / 3.0), (1.0, 0.0, FRAC_PI_2), (7.5, 0.3, 2.0)] {
        let at = |a: f64| mt::Transform::transform_point(&rotation(z, Rad(a)), mt::Point3::new(radius, 0.0, 5.0));
        let (p0, p1, t) = (at(start), at(start + turn), at(start + turn / 2.0));
        let (v0, v1) = (p0 - t, p1 - t);
        let (y, x) = (v0.cross(v1).magnitude(), v0.dot(v1));
        assert_atan2_rounded(y, x);
        let arc = (Rad(PI) - Rad(atan2(y, x))) * 2.0;
        assert_eq!(builder::circle_arc_angle(p0, p1, t).0.to_bits(), arc.0.to_bits(), "the arc's angle, {radius} from {start} by {turn}");
        assert!((arc.0 - turn).abs() < 1e-12, "the arc spans {turn}: {arc:?}");
        let (sin_half, cos_half, _) = sin_cos_rounded((arc / 2.0).0);
        let (sin, cos, _) = sin_cos_rounded(arc.0);
        let mut want = mt::NurbsCurve::new(mt::BSplineCurve::new(
            mt::KnotVec::bezier_knot(2),
            vec![mt::Vector4::new(1.0, 0.0, 0.0, 1.0), mt::Vector4::new(cos_half, sin_half, 0.0, cos_half), mt::Vector4::new(cos, sin, 0.0, 1.0)],
        ));
        for knot in [0.25, 0.5, 0.75] {
            want.add_knot(knot);
        }
        let got = builder::unit_circle_arc(arc);
        let points = |c: &mt::NurbsCurve<mt::Vector4>| {
            (0..6)
                .map(|i| [c.control_point(i).x, c.control_point(i).y, c.control_point(i).z, c.control_point(i).w].map(f64::to_bits))
                .collect::<Vec<_>>()
        };
        assert_eq!(points(&got), points(&want), "the unit arc's control points, {radius} from {start} by {turn}");
        // And the edge the builder makes from the three points is that arc, placed.
        let edge = builder::circle_arc(&builder::vertex(p0), &builder::vertex(p1), t);
        let mt::Curve::NurbsCurve(curve) = edge.curve() else { panic!("an arc") };
        assert_eq!(curve.control_point(5).w.to_bits(), got.control_point(5).w.to_bits());
    }
}

/// `rephase_revolved` keeps PC-12's arithmetic-only series (`body::sin_cos`) for its turn, so the
/// bits `rephase_tests` recorded and Polykrom's seed 94 record stay: that series is within an
/// ULP of the correctly rounded value at its angles, not always equal to it (at seed 36's angle
/// its cosine is the ULP below). This pins that distance, and shows what switching the turn to
/// `portable_trig::sin_cos` would change.
#[test]
fn rephase_series_is_within_an_ulp_of_correctly_rounded() {
    for (v, sin_off, cos_off) in [(-2.0943951023931957 + PI, 0, 1), (1.0, 1, 0), (4.5, 0, 0)] {
        let phi = rephase_angle(v);
        let (s, c) = crate::body::sin_cos(phi);
        let (es, ec, _) = sin_cos_rounded(phi);
        assert_eq!(
            (ulps(s, es), ulps(c, ec)),
            (sin_off, cos_off),
            "rephase series at {phi:?}: {s:?} {c:?} against the correctly rounded {es:?} {ec:?}"
        );
        assert_eq!((es, ec), sin_cos(phi));
    }
    let kmax = (PI * 1048576.0) as i64;
    let (mut off, mut n) = (0, 0);
    for k in (-kmax..=kmax).step_by(1511) {
        let x = crate::body::snap_angle(k as f64 / 1048576.0);
        let (s, c) = crate::body::sin_cos(x);
        let (es, ec) = sin_cos(x);
        assert!(ulps(s, es) <= 1 && ulps(c, ec) <= 1, "rephase series at {x:?}: {s:?} {c:?} against {es:?} {ec:?}");
        off += usize::from(s != es || c != ec);
        n += 1;
    }
    println!("the rephase series is an ULP off the correctly rounded value at {off} of {n} sampled grid angles");
}
